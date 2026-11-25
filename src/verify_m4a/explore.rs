//! Milestone M4a-0 Phase 0.4 exploratory characterisation and opportunity-analysis harness
//! (`arkhe explore-m4a`).
//!
//! Per Rule 8 (`docs/instructions/repo-common.md`) and Plan §2, `explore-m4a` is strictly
//! exploratory: it measures and reports regime-stratified distributions, mechanistic opportunity
//! bounds (`K2`), equal-compute / node-eval audits (`R2`), `I2`/`I4`/`I5`/`I6`/`I7` invariants,
//! guardrail telemetry, and descriptive sensitivity / runtime extrapolation without gating.
//! Unconditionally rejects any overlap with `crate::stats::HOLDOUT_BANKS` and the M4a
//! confirmation bank `20200..=20299`.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use rand_core::Rng;
use serde::{Deserialize, Serialize};

use crate::baselines::{PredictorBaseline, TickBudget};
use crate::config::{BuildProvenance, CliffCountersSummary};
use crate::env::{FiniteStateGenerator, FiniteStateSpec, NoisyEnvironment};
use crate::metrics::{PointForecastScorer, circuit_code_bits_with_tax, facts_code_bits};
use crate::rng::{SeedBank, StreamDomain};
use crate::stats::{HOLDOUT_BANKS, HoldoutRangeSpec, check_seed_range_holdouts};
use crate::structural::economy::score_with_reward_mode;
use crate::structural::surgery::register_mapping;
use crate::structural::{
    ArkheLearner, EconomyConfig, GrowthConfig, SleepSchedule, SleepSegmentReport, SleepTelemetry,
    cone_channel_mask, derived_wake_sleep_windows, excise_comparator_branch, observation_horizon,
    verify_epistemic_mdl_conservation,
};
use crate::substrate::{Circuit, NodeId, ScalarState, SignalSink, SignalSource};
use crate::verify_m4a::oracle::{
    CircuitOracleLabels, PortLanguageOracle, audit_sleep_segment_oracle, detect_loopback_attractor,
    extract_derivation_circuit, label_learner_oracle,
};
use crate::verify_m4a::planted::{inject_pl1_short_cycle_alt, inject_pl2_rare_context_alt};
use crate::verify_m4a::tasks::{
    M4aRegime, M4aTaskFamily, M4aTaskKind, M4aTaskSpec, PlantedSpec, m4a0_task_registry,
};

const MAX_PL_SETTLE_TICKS: u64 = 2_000;
const PL_SETTLE_BUDGET: u64 = 200_000;
#[cfg(test)]
const CONSERVATION_TOL: f64 = 1e-11;
const DREAM_LANE_COUNT: u32 = 8;
const DREAM_LANE_MAX_STEPS: usize = 4_096;
const SENSITIVITY_SLEEP_WINDOWS: [u64; 3] = [64, 128, 256];

const M4A_CONFIRMATION_BANKS: &[HoldoutRangeSpec] = &[HoldoutRangeSpec {
    label: "M4a_Confirmation",
    start: 20_200,
    end_inclusive: 20_299,
    allow_with_flag: false,
    status_note: "reserved M4a confirmation bank; unconditionally blocked in explore-m4a",
}];

/// Unconditionally rejects any seed range `[seed_offset, seed_offset + seeds)` overlapping
/// pre-registered milestone holdout banks (`HOLDOUT_BANKS`) or the M4a confirmation bank
/// (`20200..=20299`).
///
/// # Errors
///
/// Returns a descriptive error string if any overlap is detected.
pub fn check_explore_m4a_seed_range_holdout(seed_offset: u64, seeds: usize) -> Result<(), String> {
    let span = u64::try_from(seeds.max(1)).unwrap_or(u64::MAX);
    check_seed_range_holdouts("explore-m4a", seed_offset, span, false, HOLDOUT_BANKS)?;
    check_seed_range_holdouts(
        "explore-m4a",
        seed_offset,
        span,
        false,
        M4A_CONFIRMATION_BANKS,
    )
}

/// Descriptive distribution summary over a finite sample of scalar values.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScalarStats {
    pub count: usize,
    pub mean: f64,
    pub std_dev: f64,
    pub min: f64,
    pub p05: f64,
    pub median: f64,
    pub p95: f64,
    pub max: f64,
}

/// Paired difference distribution summary including directional win/loss/tie fractions.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PairedDeltaStats {
    pub count: usize,
    pub mean: f64,
    pub std_dev: f64,
    pub min: f64,
    pub p05: f64,
    pub median: f64,
    pub p95: f64,
    pub max: f64,
    pub frac_gt_0: f64,
    pub frac_lt_0: f64,
    pub frac_eq_0: f64,
}

fn linear_quantile(sorted: &[f64], q: f64) -> f64 {
    debug_assert!(!sorted.is_empty());
    if sorted.len() == 1 {
        return sorted[0];
    }
    let idx = q.clamp(0.0, 1.0) * ((sorted.len() - 1) as f64);
    let lo = idx.floor() as usize;
    let hi = idx.ceil() as usize;
    let frac = idx - (lo as f64);
    sorted[lo] * (1.0 - frac) + sorted[hi] * frac
}

fn summarize_scalars(values: &[f64]) -> ScalarStats {
    let mut finite: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if finite.is_empty() {
        return ScalarStats::default();
    }
    finite.sort_by(f64::total_cmp);
    let count = finite.len();
    let mean = finite.iter().sum::<f64>() / (count as f64);
    let std_dev = if count > 1 {
        let var = finite
            .iter()
            .map(|&v| {
                let d = v - mean;
                d * d
            })
            .sum::<f64>()
            / ((count - 1) as f64);
        var.sqrt()
    } else {
        0.0
    };
    ScalarStats {
        count,
        mean,
        std_dev,
        min: finite[0],
        p05: linear_quantile(&finite, 0.05),
        median: linear_quantile(&finite, 0.50),
        p95: linear_quantile(&finite, 0.95),
        max: finite[count - 1],
    }
}

fn summarize_paired_deltas(values: &[f64]) -> PairedDeltaStats {
    let base = summarize_scalars(values);
    if base.count == 0 {
        return PairedDeltaStats::default();
    }
    let denom = base.count as f64;
    let mut gt = 0usize;
    let mut lt = 0usize;
    let mut eq = 0usize;
    for &v in values {
        if !v.is_finite() {
            continue;
        }
        if v > 1e-12 {
            gt += 1;
        } else if v < -1e-12 {
            lt += 1;
        } else {
            eq += 1;
        }
    }
    PairedDeltaStats {
        count: base.count,
        mean: base.mean,
        std_dev: base.std_dev,
        min: base.min,
        p05: base.p05,
        median: base.median,
        p95: base.p95,
        max: base.max,
        frac_gt_0: (gt as f64) / denom,
        frac_lt_0: (lt as f64) / denom,
        frac_eq_0: (eq as f64) / denom,
    }
}

fn summarize_opt_u64(values: &[Option<u64>]) -> Option<ScalarStats> {
    let finite: Vec<f64> = values.iter().flatten().map(|&v| v as f64).collect();
    (!finite.is_empty()).then(|| summarize_scalars(&finite))
}

fn summarize_u64_slice(values: &[u64]) -> Option<ScalarStats> {
    let finite: Vec<f64> = values.iter().map(|&v| v as f64).collect();
    (!finite.is_empty()).then(|| summarize_scalars(&finite))
}

/// Aggregated sleep telemetry and guardrail totals across seeds in one cell.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SleepTelemetryTotals {
    pub sleep_segments_executed: u64,
    pub sleep_segment_skipped_no_scoreable_comparator: u64,
    pub sleep_excisions: u64,
    pub sleep_minority_routes_pruned: u64,
    pub sleep_starvation_excisions: u64,
    pub sleep_comparator_suspended_ungrounded: u64,
    pub max_suspended_comparator_ticks: u64,
    pub sleep_match_failures: u64,
    pub fact_buffer_evictions: u64,
    pub total_facts_forgone: u64,
    pub max_facts_forgone: u64,
    pub mean_facts_forgone: f64,
}

/// Arm-level distribution metrics for `wake-only`, `S-A`, or `S-B`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArmDistributionSummary {
    pub eval_l_epistemic_bits: ScalarStats,
    pub eval_l_epistemic_target_bits: ScalarStats,
    pub eval_delta_l_epistemic_bits: ScalarStats,
    pub eval_delta_l_epistemic_target_bits: ScalarStats,
    pub cumulative_delta_l_epistemic_bits: ScalarStats,
    pub cumulative_delta_l_epistemic_per_tick: ScalarStats,
    pub final_circuit_code_bits: ScalarStats,
    pub eval_kt_all_ch_bits_per_symbol: ScalarStats,
    pub eval_kt_target_bits_per_symbol: ScalarStats,
    pub final_node_count: ScalarStats,
    pub final_port_oracle_correct_rate: f64,
    pub eval_clean_zero_error_rate: f64,
    pub active_comparator_rate: f64,
    pub mean_active_comparators: f64,
    pub max_epistemic_mdl_err: f64,
    pub max_compute_ledger_diff: u64,
    pub mean_episode_node_evals: f64,
    pub mean_live_step_node_evals: f64,
    pub mean_candidate_replay_node_evals: f64,
    pub mean_sensitivity_node_evals: f64,
    pub cliff_counters: CliffCountersSummary,
    pub sprout_blocked_max_nodes_runs: usize,
    pub sprout_budget_exhausted_runs: usize,
    pub lag_horizon_bound_runs: usize,
    pub candidate_undecidable_runs: usize,
    pub sleep_telemetry_totals: SleepTelemetryTotals,
}

/// Mechanistic opportunity analysis and paired-vs-wake metrics for one sleep arm (`S-A` or `S-B`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SleepArmSummary {
    pub arm: ArmDistributionSummary,
    // G4A.1 / R2 one-sided node evaluation audit
    pub node_eval_ratio: ScalarStats,
    pub frac_node_eval_ratio_le_1: f64,
    pub live_step_eval_ratio_mean: f64,
    pub candidate_replay_eval_ratio_mean: f64,
    pub sensitivity_eval_ratio_mean: f64,
    // G4A.2 structural & phase invariants
    pub i2_pre_onset_bit_identical_rate: f64,
    pub i2_no_onset_exact_match_rate: f64,
    pub i6_null_frozen_and_zero_boundary_reward_rate: f64,
    pub i7_suspended_zero_flux_rate: f64,
    // G4A.3 H5 paired advantages (`wake - sleep`, positive => sleep achieves lower MDL)
    pub eval_l_epi_advantage_bits: PairedDeltaStats,
    pub eval_l_epi_target_advantage_bits: PairedDeltaStats,
    pub eval_delta_l_epi_advantage_bits: PairedDeltaStats,
    pub cumulative_delta_l_epi_advantage_bits: PairedDeltaStats,
    pub cumulative_delta_l_epi_per_tick_advantage: PairedDeltaStats,
    pub final_code_bits_advantage: PairedDeltaStats,
    // G4A.4 boundary non-inferiority (`sleep - wake`, <= 0 => sleep non-inferior)
    pub eval_kt_regret_all_ch: PairedDeltaStats,
    pub eval_kt_regret_target: PairedDeltaStats,
    // G4A.5 PL spurious rejection (populated for PL tasks)
    pub pl_excision_rate: Option<f64>,
    pub pl_spurious_cycle_extinction_rate: Option<f64>,
    pub spurious_survival_segments: Option<ScalarStats>,
    pub segments_to_excise: Option<ScalarStats>,
    pub sleep_refutation_latency_wake_ticks: Option<ScalarStats>,
    pub sleep_refutation_latency_substrate_ticks: Option<ScalarStats>,
    pub wake_refutation_latency_ticks: Option<ScalarStats>,
    // G4A.6 oracle-correct structure preservation
    pub total_wrongful_excisions: usize,
    pub max_wrongful_excisions: usize,
    pub runs_with_wrongful_excision: usize,
    pub total_oracle_correct_port_locks_lost: usize,
    pub max_oracle_correct_port_locks_lost: usize,
    // Opportunity analysis (§2 item 1, Rule 9 & K2)
    pub onset_rate: f64,
    pub mean_t_onset: Option<f64>,
    pub max_t_onset: Option<u64>,
    pub p_onset_oracle_incorrect_given_onset: Option<f64>,
    pub p_onset_oracle_incorrect_overall: f64,
    pub p_onset_wrong_primary_overall: f64,
    pub p_onset_wrong_alt_overall: f64,
    pub p_any_segment_oracle_incorrect_given_onset: Option<f64>,
    pub p_any_segment_oracle_incorrect_overall: f64,
    pub recoverable_bits_at_onset: ScalarStats,
    pub recoverable_bits_at_onset_given_onset: Option<ScalarStats>,
    pub recoverable_bits_at_wake_end: ScalarStats,
    pub recoverable_bits_at_sleep_end: ScalarStats,
    pub fact_loss_eval_bits: ScalarStats,
    pub fact_loss_eval_kt_bits: ScalarStats,
    pub fact_loss_cum_flux_bits: ScalarStats,
    pub opportunity_bound_onset_eval_bits: ScalarStats,
    pub opportunity_bound_wake_end_eval_bits: ScalarStats,
    pub opportunity_bound_onset_cum_bits: ScalarStats,
    pub opportunity_bound_wake_end_cum_bits: ScalarStats,
    pub spontaneous_incorrect_comp_born_count: usize,
    pub spontaneous_refuted_wake_only_count: usize,
    pub spontaneous_refuted_sleep_arm_count: usize,
    pub spontaneous_wake_refutation_latency: Option<ScalarStats>,
    pub spontaneous_sleep_refutation_latency: Option<ScalarStats>,
}

/// Paired `S-A` vs `S-B` contrast (`G4A.7`, `H6`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct H6PairedContrast {
    pub sb_minus_sa_eval_l_epi_advantage: PairedDeltaStats,
    pub sb_minus_sa_eval_delta_l_epi_advantage: PairedDeltaStats,
    pub sb_minus_sa_cum_delta_l_epi_advantage: PairedDeltaStats,
    pub sb_minus_sa_final_code_bits_advantage: PairedDeltaStats,
    pub sb_minus_sa_eval_kt_diff_all_ch: PairedDeltaStats,
    pub sb_minus_sa_eval_kt_diff_target: PairedDeltaStats,
    pub sa_pl_excision_rate: Option<f64>,
    pub sb_pl_excision_rate: Option<f64>,
    pub sa_mean_sleep_refutation_latency: Option<f64>,
    pub sb_mean_sleep_refutation_latency: Option<f64>,
}

/// Complete stratified report cell for one `(family, task_id, k, regime)`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskCellSummary {
    pub task_id: String,
    pub family: String,
    pub kind: String,
    pub width_bits: u8,
    pub regime: String,
    pub epsilon: f64,
    pub horizon: u64,
    pub target_port: u16,
    pub num_channels: u16,
    pub seeds_tested: usize,
    pub pl_settled_runs: usize,
    pub wake_only: ArmDistributionSummary,
    pub sa: SleepArmSummary,
    pub sb: SleepArmSummary,
    pub h6_contrast: H6PairedContrast,
}

/// Compact per-task `K2` opportunity table row across the 16 spontaneous (`SP`) tasks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpOpportunityRow {
    pub task_id: String,
    pub width_bits: u8,
    pub regime: String,
    pub wake_only_oracle_correct_rate: f64,
    pub sa_onset_rate: f64,
    pub sb_onset_rate: f64,
    pub sa_p_onset_incorrect: f64,
    pub sb_p_onset_incorrect: f64,
    pub sa_mean_recov_onset_bits: f64,
    pub sb_mean_recov_onset_bits: f64,
    pub wake_end_mean_recov_bits: f64,
    pub sa_mean_fact_loss_eval_bits: f64,
    pub sb_mean_fact_loss_eval_bits: f64,
    pub sa_mean_opp_onset_eval_bits: f64,
    pub sb_mean_opp_onset_eval_bits: f64,
    pub sa_mean_opp_wake_end_eval_bits: f64,
    pub sb_mean_opp_wake_end_eval_bits: f64,
    pub sa_mean_opp_onset_cum_bits: f64,
    pub sb_mean_opp_onset_cum_bits: f64,
    pub sa_mean_eval_l_epi_adv_bits: f64,
    pub sb_mean_eval_l_epi_adv_bits: f64,
}

/// Aggregate `K2` opportunity verdict across the 16 spontaneous (`SP`) tasks (never pooled across Clean/Noisy).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct K2OpportunitySummary {
    pub sp_tasks_total: usize,
    pub sp_clean_tasks: usize,
    pub sp_noisy_tasks: usize,
    pub sp_tasks_positive_mean_opp_onset_eval_sa: usize,
    pub sp_tasks_positive_mean_opp_onset_eval_sb: usize,
    pub sp_tasks_positive_mean_opp_wake_end_eval_sa: usize,
    pub sp_tasks_positive_mean_opp_wake_end_eval_sb: usize,
    pub sp_tasks_positive_mean_opp_onset_cum_sa: usize,
    pub sp_tasks_positive_mean_opp_onset_cum_sb: usize,
    pub sp_tasks_positive_mean_eval_l_epi_adv_sa: usize,
    pub sp_tasks_positive_mean_eval_l_epi_adv_sb: usize,
    pub max_sp_opportunity_bound_eval_bits: f64,
    pub max_sp_opportunity_bound_cum_bits: f64,
    pub k2_null_triggered_on_onset_eval_opp: bool,
    pub k2_null_triggered_on_wake_end_eval_opp: bool,
    pub k2_null_triggered_on_cum_opp: bool,
    pub per_sp_task_rows: Vec<SpOpportunityRow>,
}

/// Descriptive sensitivity row for $W_{\text{sleep}} \in \{W_{\text{diag}}(64), 2 W_{\text{diag}}(128), W_{\text{obs}}(256)\}$ (`§2` item 5).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SleepWindowSensitivityRow {
    pub task_id: String,
    pub family: String,
    pub width_bits: u8,
    pub regime: String,
    pub arm: String,
    pub w_sleep: u64,
    pub seeds_tested: usize,
    pub onset_rate: f64,
    pub mean_eval_l_epi_adv_bits: f64,
    pub mean_cum_delta_l_epi_adv_bits: f64,
    pub mean_final_code_bits_adv: f64,
    pub mean_eval_kt_regret_target: f64,
    pub max_eval_kt_regret_target: f64,
    pub max_node_eval_ratio: f64,
    pub total_wrongful_excisions: usize,
    pub pl_excision_rate: Option<f64>,
    pub pl_mean_survival_segments: Option<f64>,
}

/// Descriptive perturbed multi-start dream-lane sensitivity summary (Charter §9, `§2` item 5).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PerturbedDreamLaneSummary {
    pub task_id: String,
    pub family: String,
    pub width_bits: u8,
    pub regime: String,
    pub probes_executed: usize,
    pub alt_cones_probed: usize,
    pub alt_single_live_spurious: usize,
    pub alt_perturbed_8lanes_spurious: usize,
    pub alt_perturbed_only_reveals: usize,
    pub primary_cones_probed: usize,
    pub primary_single_live_spurious: usize,
    pub primary_perturbed_8lanes_spurious: usize,
    pub primary_perturbed_only_reveals: usize,
}

/// Aggregate guardrail counts across all evaluated tasks and arms (`§2` item 4).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GuardrailAggregateSummary {
    pub sprout_blocked_max_nodes: u64,
    pub sprout_budget_exhausted: u64,
    pub lag_horizon_bound: u64,
    pub candidate_undecidable: u64,
    pub sleep_comparator_suspended_ungrounded: u64,
    pub sleep_segment_skipped_no_scoreable_comparator: u64,
    pub fact_buffer_evictions: u64,
    pub sleep_match_failures: u64,
    pub max_suspended_comparator_ticks: u64,
    pub max_facts_forgone: u64,
    pub binding_events: Vec<String>,
}

/// Aggregate structural invariant audit across all runs (`I2`, `I4`, `I5`, `I6`, `I7`, `G4A.6`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InvariantAggregateSummary {
    pub max_epistemic_mdl_err: f64,
    pub max_compute_ledger_diff: u64,
    pub i2_pre_onset_violations: usize,
    pub i6_violations: usize,
    pub i7_violations: usize,
    pub g4a6_wrongful_excisions_total: usize,
    pub g4a6_port_locks_lost_total: usize,
}

/// Runtime measurements and projections for 100-seed and 500-seed sweeps (`§2` item 6).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RuntimeExtrapolation {
    pub measured_seeds: usize,
    pub measured_wall_clock_seconds: f64,
    pub seconds_per_seed: f64,
    pub projected_100_seeds_seconds: f64,
    pub projected_500_seeds_seconds: f64,
    pub projected_1000_seeds_seconds: f64,
    pub per_family_seconds_per_seed: BTreeMap<String, f64>,
}

/// Complete JSON manifest emitted by `arkhe explore-m4a`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExploreM4aReport {
    pub provenance: BuildProvenance,
    pub seed_offset: u64,
    pub seeds_tested: usize,
    pub include_sensitivity: bool,
    pub threads_used: usize,
    pub wall_clock_seconds: f64,
    pub per_family_wall_clock_seconds: BTreeMap<String, f64>,
    pub runtime_projections: RuntimeExtrapolation,
    pub cells: Vec<TaskCellSummary>,
    pub k2_opportunity_summary: K2OpportunitySummary,
    pub sensitivity_w_sleep: Vec<SleepWindowSensitivityRow>,
    pub perturbed_dream_lanes: Vec<PerturbedDreamLaneSummary>,
    pub guardrail_summary: GuardrailAggregateSummary,
    pub invariant_summary: InvariantAggregateSummary,
}

#[derive(Clone, Debug)]
struct FrozenEvalMetrics {
    eval_kt_all_ch: f64,
    eval_kt_target: f64,
    eval_l_epistemic_bits: f64,
    eval_l_epistemic_target_bits: f64,
    eval_delta_l_epistemic_bits: f64,
    eval_delta_l_epistemic_target_bits: f64,
    eval_clean_zero_error_target: bool,
}

#[derive(Clone, Debug)]
struct CheckpointSnapshot {
    cumulative_net_flux: f64,
    frozen: FrozenEvalMetrics,
}

#[derive(Clone, Debug)]
struct DreamProbeCounts {
    alt_cones_probed: usize,
    alt_single_live_spurious: usize,
    alt_perturbed_8lanes_spurious: usize,
    alt_perturbed_only_reveals: usize,
    primary_cones_probed: usize,
    primary_single_live_spurious: usize,
    primary_perturbed_8lanes_spurious: usize,
    primary_perturbed_only_reveals: usize,
}

#[derive(Clone, Debug)]
struct ArmRunOutcome {
    pl_settled: bool,
    episode_ticks: u64,
    env_steps: u64,
    facts_forgone: u64,
    wake_preds_prefix: Vec<Vec<u64>>,
    wake_obs_prefix: Vec<Vec<u64>>,
    final_circuit_code_bits: f64,
    final_node_count: usize,
    cumulative_net_flux: f64,
    cumulative_delta_l_epistemic_bits: f64,
    cumulative_delta_l_epistemic_per_tick: f64,
    frozen: FrozenEvalMetrics,
    final_port_oracle_correct: bool,
    final_active_comparators: usize,
    epistemic_mdl_err: f64,
    episode_total_node_evals: u64,
    episode_live_step_node_evals: u64,
    episode_candidate_replay_node_evals: u64,
    episode_sensitivity_node_evals: u64,
    cliff: CliffCountersSummary,
    sleep_telemetry: SleepTelemetry,
    first_onset_wake_tick: Option<u64>,
    onset_has_incorrect_comp: bool,
    onset_wrong_primary: bool,
    onset_wrong_alt: bool,
    any_segment_has_incorrect_comp: bool,
    recoverable_bits_at_onset: f64,
    recoverable_bits_at_end: f64,
    wrongful_excisions: usize,
    oracle_correct_port_locks_lost: usize,
    i6_ok: bool,
    i7_ok: bool,
    pl_excised: bool,
    pl_spurious_extinguished: bool,
    pl_spurious_survival_segments: u64,
    pl_segments_to_excise: Option<u64>,
    pl_refutation_wake_ticks: Option<u64>,
    pl_refutation_substrate_ticks: Option<u64>,
    spontaneous_incorrect_born: usize,
    spontaneous_incorrect_refuted: usize,
    spontaneous_refutation_latencies: Vec<u64>,
    checkpoints_by_wake_step: BTreeMap<u64, CheckpointSnapshot>,
    dream_probe: Option<DreamProbeCounts>,
}

fn family_name(family: M4aTaskFamily) -> &'static str {
    match family {
        M4aTaskFamily::Sp => "SP",
        M4aTaskFamily::Pl => "PL",
        M4aTaskFamily::Ct => "CT",
    }
}

fn regime_name(regime: M4aRegime) -> &'static str {
    match regime {
        M4aRegime::Clean => "clean",
        M4aRegime::Noisy => "noisy",
    }
}

fn kind_name(kind: M4aTaskKind) -> &'static str {
    match kind {
        M4aTaskKind::SpMultiViewPeriodic => "SpMultiViewPeriodic",
        M4aTaskKind::SpMultiViewLfsr => "SpMultiViewLfsr",
        M4aTaskKind::SpPeriod6LagRedundancy => "SpPeriod6LagRedundancy",
        M4aTaskKind::SpChannelFault => "SpChannelFault",
        M4aTaskKind::Pl1ShortCycle => "Pl1ShortCycle",
        M4aTaskKind::Pl2RareContext => "Pl2RareContext",
        M4aTaskKind::Ct1ExogenousLfsr => "Ct1ExogenousLfsr",
        M4aTaskKind::Ct2RareEventCorrect => "Ct2RareEventCorrect",
        M4aTaskKind::Ct3NoComparator => "Ct3NoComparator",
    }
}

fn find_port_driver_edge(circuit: &Circuit, target_port: u16) -> Option<(SignalSource, u16)> {
    circuit
        .edges()
        .iter()
        .find(|e| e.dst == SignalSink::BoundaryOut(target_port))
        .map(|e| (e.src, e.hops))
}

fn find_node_input_edge(circuit: &Circuit, node: NodeId, port: u8) -> Option<(SignalSource, u16)> {
    circuit
        .edges()
        .iter()
        .find(|e| e.dst == SignalSink::NodeIn { node, port })
        .map(|e| (e.src, e.hops))
}

fn target_port_is_driven(circuit: &Circuit, target_port: u16) -> bool {
    find_port_driver_edge(circuit, target_port).is_some()
}

fn target_cone_all_mature(learner: &ArkheLearner, target_port: u16) -> bool {
    let circuit = learner.editable().circuit();
    let economy = learner.editable().economy();
    let mut nodes = crate::structural::surgery::primary_cone_nodes(circuit, economy, target_port);
    for comp in economy.comparators() {
        if comp.target_port == target_port {
            nodes.insert(comp.sub_node);
            nodes.insert(comp.eqz_node);
            let mut stack = vec![comp.sub_node, comp.eqz_node];
            let mut seen = BTreeSet::new();
            while let Some(cur) = stack.pop() {
                if !seen.insert(cur) {
                    continue;
                }
                nodes.insert(cur);
                for edge in circuit.edges() {
                    if let SignalSink::NodeIn { node: dst, .. } = edge.dst
                        && dst == cur
                        && let SignalSource::NodeOut { node: src, .. } = edge.src
                    {
                        stack.push(src);
                    }
                }
            }
        }
    }
    let econ_nodes = economy.nodes();
    nodes.iter().all(|id| {
        econ_nodes
            .get(id.index())
            .is_none_or(|ne| ne.juvenile_ticks == 0)
    })
}

fn target_cone_signature(learner: &ArkheLearner, target_port: u16) -> (Vec<(NodeId, u8)>, usize) {
    let circuit = learner.editable().circuit();
    let economy = learner.editable().economy();
    let nodes = crate::structural::surgery::primary_cone_nodes(circuit, economy, target_port);
    let mut sig: Vec<(NodeId, u8)> = nodes
        .into_iter()
        .filter_map(|id| {
            circuit
                .nodes()
                .get(id.index())
                .map(|p| (id, p.input_arity()))
        })
        .collect();
    sig.sort_by_key(|(id, _)| id.index());
    let comp_count = economy
        .comparators()
        .iter()
        .filter(|c| c.target_port == target_port)
        .count();
    (sig, comp_count)
}

/// Computes code length bits held by oracle-incorrect comparator derivations (`L(C) - L(C_pruned)`).
#[must_use]
pub fn compute_recoverable_comparator_bits(
    circuit: &Circuit,
    economy_comps: &[crate::structural::ComparatorSite],
    labels: &CircuitOracleLabels,
    split_code_tax: bool,
) -> f64 {
    if economy_comps.is_empty() || labels.comparators.is_empty() {
        return 0.0;
    }
    let initial_bits = circuit_code_bits_with_tax(circuit, 0, split_code_tax);
    let mut cur_circ = circuit.clone();
    let mut active: Vec<(NodeId, NodeId, u16, bool, bool)> = economy_comps
        .iter()
        .zip(labels.comparators.iter())
        .map(|(c, l)| {
            (
                c.sub_node,
                c.eqz_node,
                c.target_port,
                l.primary_oracle_correct,
                l.alt_oracle_correct,
            )
        })
        .collect();

    while let Some(pos) = active
        .iter()
        .position(|&(_, _, _, pri_ok, alt_ok)| !pri_ok || !alt_ok)
    {
        let (sub_node, eqz_node, target_port, pri_ok, alt_ok) = active[pos];
        // Excise the wrong arm (or alt arm if both wrong so the port remains driven).
        let prune_primary = !pri_ok && alt_ok;
        let target_port_subs: Vec<NodeId> = active
            .iter()
            .filter(|c| c.2 == target_port)
            .map(|c| c.0)
            .collect();
        let surviving_comp_nodes: Vec<(NodeId, NodeId)> = active
            .iter()
            .enumerate()
            .filter_map(|(i, c)| (i != pos).then_some((c.0, c.1)))
            .collect();
        let Some((next_circ, old_to_new)) = excise_comparator_branch(
            &cur_circ,
            sub_node,
            eqz_node,
            target_port,
            prune_primary,
            &target_port_subs,
            &surviving_comp_nodes,
        ) else {
            break;
        };
        cur_circ = next_circ;
        active.remove(pos);
        let mut remapped = Vec::with_capacity(active.len());
        for (s, e, tp, pok, aok) in active {
            if let (Some(ns), Some(ne)) = (
                old_to_new.get(s.index()).copied().flatten(),
                old_to_new.get(e.index()).copied().flatten(),
            ) {
                remapped.push((ns, ne, tp, pok, aok));
            }
        }
        active = remapped;
    }

    let reduced_bits = circuit_code_bits_with_tax(&cur_circ, 0, split_code_tax);
    (initial_bits - reduced_bits).max(0.0)
}

#[allow(clippy::too_many_arguments)]
fn evaluate_frozen_window_state(
    circuit: &Circuit,
    compiled: &crate::substrate::CompiledCircuit,
    registers: &[u64],
    prev_observed_frame: &[u64],
    env: &NoisyEnvironment<crate::env::TierGenerator>,
    econ_cfg: EconomyConfig,
    target_port: u16,
    w_eval: u64,
    split_code_tax: bool,
) -> FrozenEvalMetrics {
    let width = circuit.width();
    let num_in = usize::from(circuit.num_inputs());
    let num_out = usize::from(circuit.num_outputs());
    let channels = usize::from(env.generator().channels());

    let mut state = ScalarState::new(compiled);
    state
        .set_registers(compiled, registers)
        .expect("valid live registers for frozen evaluation");
    let mut eval_env = env.clone();

    let mut curr_input = prev_observed_frame.to_vec();
    curr_input.resize(num_in, 0);
    let mut preds = vec![0u64; num_out];
    let mut clean = vec![0u64; channels];
    let mut obs = vec![0u64; channels];

    let port_driven: Vec<bool> = (0..num_out)
        .map(|p| target_port_is_driven(circuit, p as u16))
        .collect();
    let mut scorers: Vec<PointForecastScorer> = (0..num_out)
        .map(|_| PointForecastScorer::new(width, econ_cfg.null_model))
        .collect();
    let mut obs_mismatches = vec![0u64; num_out];
    let mut clean_mismatches_target = 0u64;
    let mut net_reward_all = 0.0f64;
    let mut net_reward_target = 0.0f64;
    let target_idx = usize::from(target_port);

    for _ in 0..w_eval {
        state
            .step(compiled, &curr_input, &mut preds)
            .expect("frozen step");
        eval_env.step_frame(&mut clean, &mut obs);
        for ch in 0..num_out {
            let p = width.normalize(preds[ch]);
            let o = width.normalize(obs[ch]);
            let c = width.normalize(clean[ch]);
            if p != o {
                obs_mismatches[ch] += 1;
            }
            if ch == target_idx && p != c {
                clean_mismatches_target += 1;
            }
            let r = if port_driven[ch] {
                score_with_reward_mode(&mut scorers[ch], econ_cfg.reward_mode, width, p, o)
            } else {
                scorers[ch].null_model_mut().observe_and_surprisal(o);
                0.0
            };
            net_reward_all += r;
            if ch == target_idx {
                net_reward_target += r;
            }
        }
        for (dst, &v) in curr_input.iter_mut().zip(obs.iter()) {
            *dst = width.normalize(v);
        }
    }

    let w_f64 = w_eval.max(1) as f64;
    let mut sum_facts_all_ch = 0.0f64;
    for &mis in &obs_mismatches {
        sum_facts_all_ch += facts_code_bits(width, w_eval, mis);
    }
    let facts_target = facts_code_bits(width, w_eval, obs_mismatches[target_idx]);
    let eval_kt_all_ch = sum_facts_all_ch / (w_f64 * (num_out.max(1) as f64));
    let eval_kt_target = facts_target / w_f64;
    let l_c = circuit_code_bits_with_tax(circuit, 0, split_code_tax);

    FrozenEvalMetrics {
        eval_kt_all_ch,
        eval_kt_target,
        eval_l_epistemic_bits: l_c + sum_facts_all_ch,
        eval_l_epistemic_target_bits: l_c + facts_target,
        eval_delta_l_epistemic_bits: l_c - net_reward_all,
        eval_delta_l_epistemic_target_bits: l_c - net_reward_target,
        eval_clean_zero_error_target: clean_mismatches_target == 0,
    }
}

fn evaluate_frozen_window(
    learner: &ArkheLearner,
    env: &NoisyEnvironment<crate::env::TierGenerator>,
    target_port: u16,
    w_eval: u64,
    split_code_tax: bool,
) -> FrozenEvalMetrics {
    evaluate_frozen_window_state(
        learner.editable().circuit(),
        learner.editable().compiled(),
        learner.editable().state().registers(),
        learner.prev_observed_frame(),
        env,
        *learner.economy_config(),
        target_port,
        w_eval,
        split_code_tax,
    )
}

type ScorerSnap = (u64, u64, u64, Vec<u64>);

fn snapshot_single_scorer(s: &PointForecastScorer, card: usize) -> ScorerSnap {
    let nm = s.null_model();
    let probs = (0..card)
        .map(|sym| nm.probability(sym as u64).to_bits())
        .collect();
    (s.ticks(), s.matches(), s.q_hat().to_bits(), probs)
}

fn snapshot_port_and_comp_scorers(
    learner: &ArkheLearner,
) -> (Vec<ScorerSnap>, Vec<(u16, ScorerSnap)>) {
    let card = 1usize << learner.editable().circuit().width().bits().min(6);
    let ports = learner
        .editable()
        .economy()
        .scorers()
        .iter()
        .map(|s| snapshot_single_scorer(s, card))
        .collect();
    let comps = learner
        .editable()
        .economy()
        .comparators()
        .iter()
        .map(|c| (c.target_port, snapshot_single_scorer(&c.scorer, card)))
        .collect();
    (ports, comps)
}

fn learner_grounded_mask(learner: &ArkheLearner) -> u64 {
    let k = usize::from(
        learner
            .editable()
            .circuit()
            .num_inputs()
            .max(learner.editable().circuit().num_outputs()),
    )
    .min(64);
    let mut mask = 0u64;
    for ch in 0..k {
        if learner.channel_is_grounded(ch) {
            mask |= 1u64 << ch;
        }
    }
    mask
}

fn build_cone_loopback_circuit(
    circuit: &Circuit,
    target_port: u16,
    root_src: SignalSource,
    root_hops: u16,
    grounded_mask: u64,
) -> Option<(Circuit, Vec<Option<NodeId>>, u64)> {
    let cone_mask = cone_channel_mask(circuit, root_src);
    if cone_mask == 0 {
        let (ext, map) =
            extract_derivation_circuit(circuit, root_src, target_port, Some(root_hops))?;
        let mask = if target_port < 64 {
            1u64 << target_port
        } else {
            0
        };
        return Some((ext, map, mask));
    }
    if (cone_mask & !grounded_mask) != 0 {
        return None;
    }

    let collect_cone = |src: SignalSource, out: &mut BTreeSet<NodeId>| {
        let mut stack = Vec::new();
        if let SignalSource::NodeOut { node, .. } = src {
            stack.push(node);
        }
        while let Some(node) = stack.pop() {
            if !out.insert(node) {
                continue;
            }
            for edge in circuit.edges() {
                if let SignalSink::NodeIn { node: dst, .. } = edge.dst
                    && dst == node
                    && let SignalSource::NodeOut { node: s, .. } = edge.src
                {
                    stack.push(s);
                }
            }
        }
    };

    let mut keep_nodes = BTreeSet::new();
    collect_cone(root_src, &mut keep_nodes);
    let mut port_drivers = vec![(target_port, root_src, root_hops)];

    for p in 0..usize::from(circuit.num_outputs()) {
        let p_u16 = p as u16;
        if p_u16 == target_port || (p < 64 && (grounded_mask & (1u64 << p)) == 0) {
            continue;
        }
        if let Some((drv_src, drv_hops)) = find_port_driver_edge(circuit, p_u16) {
            collect_cone(drv_src, &mut keep_nodes);
            port_drivers.push((p_u16, drv_src, drv_hops));
        }
    }

    let mut test_circ = Circuit::new(
        circuit.width(),
        circuit.num_inputs(),
        circuit.num_outputs(),
        circuit.wire_delay(),
    );
    let mut old_to_new = vec![None; circuit.node_count()];
    for &orig in &keep_nodes {
        let prim = circuit.nodes().get(orig.index()).copied()?;
        let new_id = test_circ.add_node(prim);
        old_to_new[orig.index()] = Some(new_id);
    }
    let remap_src = |src: SignalSource| -> Option<SignalSource> {
        match src {
            SignalSource::BoundaryIn(c) => Some(SignalSource::BoundaryIn(c)),
            SignalSource::NodeOut { node, port } => Some(SignalSource::NodeOut {
                node: old_to_new.get(node.index()).copied().flatten()?,
                port,
            }),
        }
    };
    for edge in circuit.edges() {
        if let SignalSink::NodeIn { node: dst, port } = edge.dst
            && keep_nodes.contains(&dst)
        {
            let mapped_dst = SignalSink::NodeIn {
                node: old_to_new[dst.index()]?,
                port,
            };
            let mapped_src = remap_src(edge.src)?;
            test_circ.connect_hops(mapped_src, mapped_dst, edge.hops);
        }
    }
    for (p_u16, drv_src, drv_hops) in port_drivers {
        let mapped_src = remap_src(drv_src)?;
        test_circ.connect_hops(mapped_src, SignalSink::BoundaryOut(p_u16), drv_hops);
    }
    test_circ.compile().ok()?;
    Some((test_circ, old_to_new, grounded_mask))
}

#[derive(Clone, Copy)]
struct DreamConeTarget {
    target_port: u16,
    root_src: SignalSource,
    root_hops: u16,
    grounded_mask: u64,
}

fn probe_cone_dream_lanes(
    learner: &ArkheLearner,
    oracle: &PortLanguageOracle,
    cone: DreamConeTarget,
    seed: u64,
    probe_salt: u32,
) -> Option<(bool, bool)> {
    let circuit = learner.editable().circuit();
    let compiled = learner.editable().compiled();
    let live_regs = learner.editable().state().registers();
    let prev_frame = learner.prev_observed_frame();
    let (test_circ, old_to_new, test_mask) = build_cone_loopback_circuit(
        circuit,
        cone.target_port,
        cone.root_src,
        cone.root_hops,
        cone.grounded_mask,
    )?;
    let test_compiled = test_circ.compile().ok()?;
    if test_compiled.total_registers() == 0 {
        return None;
    }
    let reg_map = register_mapping(circuit, &test_circ, compiled, &test_compiled, &old_to_new);
    let mut test_regs = ScalarState::new(&test_compiled).registers().to_vec();
    for (old_r, mapped) in reg_map.into_iter().enumerate() {
        if let Some(new_r) = mapped
            && old_r < live_regs.len()
            && new_r < test_regs.len()
        {
            test_regs[new_r] = live_regs[old_r];
        }
    }

    let single_spurious = detect_loopback_attractor(
        &test_compiled,
        oracle,
        &test_regs,
        prev_frame,
        test_mask,
        Some(DREAM_LANE_MAX_STEPS),
    )
    .ok()
    .is_some_and(|rep| rep.closed && rep.spurious_ports.contains(&cone.target_port));

    let width_mask = test_circ.width().mask();
    let sb = SeedBank::new(seed);
    let mut perturbed_spurious = false;
    for lane in 0..DREAM_LANE_COUNT {
        let mut rng = sb.stream(StreamDomain::BasinPerturbation {
            probe: probe_salt.saturating_mul(16).saturating_add(lane),
        });
        let mut pert_regs = test_regs.clone();
        for reg in &mut pert_regs {
            *reg = rng.next_u64() & width_mask;
        }
        if pert_regs == test_regs && !pert_regs.is_empty() {
            pert_regs[0] ^= 1 & width_mask;
        }
        if let Ok(rep) = detect_loopback_attractor(
            &test_compiled,
            oracle,
            &pert_regs,
            prev_frame,
            test_mask,
            Some(DREAM_LANE_MAX_STEPS),
        ) && rep.closed
            && rep.spurious_ports.contains(&cone.target_port)
        {
            perturbed_spurious = true;
            break;
        }
    }

    Some((single_spurious, perturbed_spurious))
}

fn run_perturbed_dream_probe(
    learner: &ArkheLearner,
    oracle: &PortLanguageOracle,
    target_port: u16,
    seed: u64,
    probe_salt: u32,
) -> DreamProbeCounts {
    let mut counts = DreamProbeCounts {
        alt_cones_probed: 0,
        alt_single_live_spurious: 0,
        alt_perturbed_8lanes_spurious: 0,
        alt_perturbed_only_reveals: 0,
        primary_cones_probed: 0,
        primary_single_live_spurious: 0,
        primary_perturbed_8lanes_spurious: 0,
        primary_perturbed_only_reveals: 0,
    };
    let circuit = learner.editable().circuit();
    let grounded_mask = learner_grounded_mask(learner);
    if usize::from(target_port) >= usize::from(oracle.channels())
        || (target_port < 64 && (grounded_mask & (1u64 << target_port)) == 0)
    {
        return counts;
    }

    if let Some((pri_src, pri_hops)) = find_port_driver_edge(circuit, target_port)
        && let Some((single_s, pert_s)) = probe_cone_dream_lanes(
            learner,
            oracle,
            DreamConeTarget {
                target_port,
                root_src: pri_src,
                root_hops: pri_hops,
                grounded_mask,
            },
            seed,
            probe_salt,
        )
    {
        counts.primary_cones_probed += 1;
        if single_s {
            counts.primary_single_live_spurious += 1;
        }
        if pert_s {
            counts.primary_perturbed_8lanes_spurious += 1;
        }
        if !single_s && pert_s {
            counts.primary_perturbed_only_reveals += 1;
        }
    }

    let out_hops = find_port_driver_edge(circuit, target_port).map(|(_, h)| h);
    for (ci, comp) in learner
        .editable()
        .economy()
        .comparators()
        .iter()
        .enumerate()
    {
        if comp.target_port != target_port {
            continue;
        }
        if let Some((alt_src, alt_hops)) = find_node_input_edge(circuit, comp.sub_node, 1)
            && let Some((single_s, pert_s)) = probe_cone_dream_lanes(
                learner,
                oracle,
                DreamConeTarget {
                    target_port: comp.target_port,
                    root_src: alt_src,
                    root_hops: out_hops.unwrap_or(alt_hops),
                    grounded_mask,
                },
                seed,
                probe_salt.saturating_add((ci as u32) + 1),
            )
        {
            counts.alt_cones_probed += 1;
            if single_s {
                counts.alt_single_live_spurious += 1;
            }
            if pert_s {
                counts.alt_perturbed_8lanes_spurious += 1;
            }
            if !single_s && pert_s {
                counts.alt_perturbed_only_reveals += 1;
            }
        }
    }

    counts
}

fn scoreable_comparator_incorrect_flags(
    learner: &ArkheLearner,
    labels: &CircuitOracleLabels,
    schedule: SleepSchedule,
) -> (bool, bool, bool) {
    let mut any_incorrect = false;
    let mut wrong_primary = false;
    let mut wrong_alt = false;
    for (comp, lbl) in learner
        .editable()
        .economy()
        .comparators()
        .iter()
        .zip(labels.comparators.iter())
    {
        if learner.comparator_is_scoreable_for_sleep(comp, schedule) {
            if !lbl.primary_oracle_correct {
                wrong_primary = true;
                any_incorrect = true;
            }
            if !lbl.alt_oracle_correct {
                wrong_alt = true;
                any_incorrect = true;
            }
        }
    }
    (any_incorrect, wrong_primary, wrong_alt)
}

fn planted_spurious_still_present(labels: &CircuitOracleLabels, target_port: u16) -> bool {
    labels
        .comparators
        .iter()
        .any(|c| c.target_port == target_port && !c.alt_oracle_correct)
}

fn planted_spurious_cycle_present(labels: &CircuitOracleLabels, target_port: u16) -> bool {
    labels
        .comparators
        .iter()
        .any(|c| c.target_port == target_port && c.alt_is_spurious_cycle)
}

fn settle_pl_prefix(
    task: &M4aTaskSpec,
    learner: &mut ArkheLearner,
    env: &mut NoisyEnvironment<crate::env::TierGenerator>,
    budget: &mut TickBudget,
    spec: &FiniteStateSpec,
    oracle: &PortLanguageOracle,
) -> bool {
    let w_obs = observation_horizon(learner.economy_config(), learner.growth_config()) as u64;
    let channels = usize::from(env.generator().channels());
    let target_ch = usize::from(task.target_port);
    let mut clean = vec![0u64; channels];
    let mut obs = vec![0u64; channels];
    let mut clean_window: VecDeque<bool> = VecDeque::with_capacity(w_obs as usize);
    let mut last_sig = target_cone_signature(learner, task.target_port);
    let mut last_sig_change = 0u64;
    let mut locked = false;

    for t in 0..MAX_PL_SETTLE_TICKS {
        let pred = learner.predict(budget);
        env.step_frame(&mut clean, &mut obs);
        let is_clean_match = task.width.normalize(pred[target_ch]) == clean[target_ch];
        if clean_window.len() == w_obs as usize {
            clean_window.pop_front();
        }
        clean_window.push_back(is_clean_match);
        // Settle cleanly first on Noisy PL tasks so primary locks cleanly before noise warmup.
        let feed = if task.epsilon > 0.0 { &clean } else { &obs };
        let _ = learner.observe(feed, budget);

        let cur_sig = target_cone_signature(learner, task.target_port);
        if cur_sig != last_sig {
            last_sig = cur_sig;
            last_sig_change = t + 1;
        }
        let stable_ticks = (t + 1).saturating_sub(last_sig_change);
        let window_matches = clean_window.iter().filter(|&&ok| ok).count();
        if t + 1 >= w_obs
            && stable_ticks >= w_obs
            && clean_window.len() == w_obs as usize
            && window_matches == w_obs as usize
            && target_port_is_driven(learner.editable().circuit(), task.target_port)
            && target_cone_all_mature(learner, task.target_port)
        {
            let lbl = label_learner_oracle(learner, spec, oracle);
            if lbl.oracle_correct_ports[target_ch] {
                locked = true;
                break;
            }
        }
    }

    if !locked {
        return false;
    }

    if task.epsilon > 0.0 {
        for _ in 0..w_obs {
            let _ = learner.predict(budget);
            env.step_frame(&mut clean, &mut obs);
            let _ = learner.observe(&obs, budget);
        }
        let lbl = label_learner_oracle(learner, spec, oracle);
        if !lbl.oracle_correct_ports[target_ch]
            || !target_port_is_driven(learner.editable().circuit(), task.target_port)
        {
            return false;
        }
    }
    true
}

#[derive(Clone)]
struct PreOnsetForkState {
    learner: ArkheLearner,
    env: NoisyEnvironment<crate::env::TierGenerator>,
    budget: TickBudget,
    start_ledger: crate::structural::ComputeLedger,
    start_ticks: u64,
    env_steps: u64,
    skipped_sleep_segments: u64,
    cached_epoch: u64,
    cached_labels: Option<CircuitOracleLabels>,
    pl_excised: bool,
    pl_spurious_extinguished: bool,
    pl_refutation_wake_ticks: Option<u64>,
    pl_refutation_substrate_ticks: Option<u64>,
    tracked_incorrect: BTreeMap<NodeId, u64>,
    spontaneous_incorrect_born: usize,
    spontaneous_incorrect_refuted: usize,
    spontaneous_refutation_latencies: Vec<u64>,
    wake_preds_prefix: Vec<Vec<u64>>,
    wake_obs_prefix: Vec<Vec<u64>>,
}

struct WakeStepState {
    cumulative_net_flux: f64,
    prev_observed_frame: Vec<u64>,
    registers: Vec<u64>,
    structure_epoch: u64,
}

struct WakeTrajectoryRecord {
    base_env: NoisyEnvironment<crate::env::TierGenerator>,
    circuits_by_epoch: BTreeMap<u64, Arc<Circuit>>,
    states_by_step: Vec<WakeStepState>,
    sa_fork: Option<PreOnsetForkState>,
    sb_fork: Option<PreOnsetForkState>,
    sa_skipped: u64,
    sb_skipped: u64,
}

impl WakeTrajectoryRecord {
    fn resolve_checkpoints(
        &self,
        checkpoint_steps: &[u64],
        econ_cfg: EconomyConfig,
        target_port: u16,
        w_obs: u64,
    ) -> BTreeMap<u64, CheckpointSnapshot> {
        let limit = self.states_by_step.len() as u64;
        let mut sorted: Vec<u64> = checkpoint_steps
            .iter()
            .copied()
            .filter(|&s| s > 0 && s < limit)
            .collect();
        sorted.sort_unstable();
        sorted.dedup();
        let mut out = BTreeMap::new();
        if sorted.is_empty() {
            return out;
        }
        let mut eval_env = self.base_env.clone();
        let ch_usize = usize::from(eval_env.generator().channels());
        let mut clean = vec![0u64; ch_usize];
        let mut obs = vec![0u64; ch_usize];
        let mut cur_step = 0u64;
        for step in sorted {
            while cur_step < step {
                eval_env.step_frame(&mut clean, &mut obs);
                cur_step += 1;
            }
            let st = &self.states_by_step[(step - 1) as usize];
            if let Some(circ) = self.circuits_by_epoch.get(&st.structure_epoch)
                && let Ok(compiled) = circ.compile()
            {
                let frozen = evaluate_frozen_window_state(
                    circ,
                    &compiled,
                    &st.registers,
                    &st.prev_observed_frame,
                    &eval_env,
                    econ_cfg,
                    target_port,
                    w_obs,
                    econ_cfg.split_code_tax,
                );
                out.insert(
                    step,
                    CheckpointSnapshot {
                        cumulative_net_flux: st.cumulative_net_flux,
                        frozen,
                    },
                );
            }
        }
        out
    }
}

fn learner_has_scoreable_for_schedule(learner: &ArkheLearner, schedule: SleepSchedule) -> bool {
    schedule != SleepSchedule::None
        && learner
            .editable()
            .economy()
            .comparators()
            .iter()
            .any(|comp| learner.comparator_is_scoreable_for_sleep(comp, schedule))
}

#[allow(clippy::too_many_arguments)]
fn run_arm_episode(
    task: &M4aTaskSpec,
    spec: &FiniteStateSpec,
    oracle: &PortLanguageOracle,
    seed: u64,
    schedule: SleepSchedule,
    sleep_window_override: Option<u64>,
    checkpoint_wake_steps: &[u64],
    record_wake_prefix_len: Option<usize>,
    run_dream_probe: bool,
    wake_node_eval_cap: Option<u64>,
) -> ArmRunOutcome {
    run_arm_episode_core(
        task,
        spec,
        oracle,
        seed,
        schedule,
        sleep_window_override,
        checkpoint_wake_steps,
        record_wake_prefix_len,
        run_dream_probe,
        wake_node_eval_cap,
        None,
        false,
    )
    .0
}

#[allow(clippy::too_many_arguments)]
fn run_arm_episode_core(
    task: &M4aTaskSpec,
    spec: &FiniteStateSpec,
    oracle: &PortLanguageOracle,
    seed: u64,
    schedule: SleepSchedule,
    sleep_window_override: Option<u64>,
    checkpoint_wake_steps: &[u64],
    record_wake_prefix_len: Option<usize>,
    run_dream_probe: bool,
    wake_node_eval_cap: Option<u64>,
    resume_fork: Option<PreOnsetForkState>,
    capture_wake_trajectory: bool,
) -> (ArmRunOutcome, Option<WakeTrajectoryRecord>) {
    let econ_cfg = EconomyConfig::default();
    let growth_cfg = GrowthConfig {
        sleep_schedule: schedule,
        sleep_window_override,
        ..GrowthConfig::default()
    };
    let w_obs = observation_horizon(&econ_cfg, &growth_cfg) as u64;
    let (w_wake, w_sleep) = derived_wake_sleep_windows(&econ_cfg, &growth_cfg);

    let (
        pl_settled,
        mut learner,
        mut env,
        mut budget,
        start_ledger,
        start_ticks,
        mut env_steps,
        mut wake_in_segment,
        mut cached_epoch,
        mut cached_labels,
        mut pl_excised,
        mut pl_spurious_extinguished,
        mut pl_refutation_wake_ticks,
        mut pl_refutation_substrate_ticks,
        mut tracked_incorrect,
        mut spontaneous_incorrect_born,
        mut spontaneous_incorrect_refuted,
        mut spontaneous_refutation_latencies,
        mut wake_preds_prefix,
        mut wake_obs_prefix,
        mut enter_sleep_immediately,
    ) = if let Some(mut fork) = resume_fork {
        fork.learner.set_sleep_schedule_for_fork(schedule);
        fork.learner
            .add_skipped_sleep_segments(fork.skipped_sleep_segments);
        (
            true,
            fork.learner,
            fork.env,
            fork.budget,
            fork.start_ledger,
            fork.start_ticks,
            fork.env_steps,
            0u64,
            fork.cached_epoch,
            fork.cached_labels,
            fork.pl_excised,
            fork.pl_spurious_extinguished,
            fork.pl_refutation_wake_ticks,
            fork.pl_refutation_substrate_ticks,
            fork.tracked_incorrect,
            fork.spontaneous_incorrect_born,
            fork.spontaneous_incorrect_refuted,
            fork.spontaneous_refutation_latencies,
            fork.wake_preds_prefix,
            fork.wake_obs_prefix,
            true,
        )
    } else {
        let sb = SeedBank::new(seed);
        let mut env = task.build_environment(sb).expect("valid M4a environment");
        let channels = env.generator().channels();
        let mut learner = ArkheLearner::new_with_config(
            task.width,
            channels,
            task.wire_delay,
            econ_cfg,
            growth_cfg,
            sb,
        )
        .expect("valid ArkheLearner");

        let mut pl_settled = true;
        if let Some(planted) = &task.planted {
            let mut settle_budget = TickBudget::new(PL_SETTLE_BUDGET);
            pl_settled = settle_pl_prefix(
                task,
                &mut learner,
                &mut env,
                &mut settle_budget,
                spec,
                oracle,
            );
            if pl_settled {
                learner.enable_epistemic_audit();
                let phase_tick = learner.tick();
                match planted {
                    PlantedSpec::Pl1 { short_motif } => {
                        inject_pl1_short_cycle_alt(
                            &mut learner,
                            task.target_port,
                            short_motif,
                            phase_tick,
                            crate::structural::learner::InjectionMode::Established,
                        )
                        .expect("PL1 injection after clean settlement");
                    }
                    PlantedSpec::Pl2 { clauses } => {
                        inject_pl2_rare_context_alt(
                            &mut learner,
                            task.target_port,
                            clauses,
                            crate::structural::learner::InjectionMode::Established,
                        )
                        .expect("PL2 injection after clean settlement");
                    }
                }
            }
        } else {
            learner.enable_epistemic_audit();
        }

        if task.planted.is_some() && !pl_settled {
            let tel = learner.telemetry();
            let ledger = learner.compute_ledger();
            let frozen = evaluate_frozen_window(
                &learner,
                &env,
                task.target_port,
                w_obs,
                econ_cfg.split_code_tax,
            );
            return (
                ArmRunOutcome {
                    pl_settled: false,
                    episode_ticks: 0,
                    env_steps: 0,
                    facts_forgone: 0,
                    wake_preds_prefix: Vec::new(),
                    wake_obs_prefix: Vec::new(),
                    final_circuit_code_bits: circuit_code_bits_with_tax(
                        learner.editable().circuit(),
                        0,
                        econ_cfg.split_code_tax,
                    ),
                    final_node_count: tel.final_node_count,
                    cumulative_net_flux: tel.cumulative_net_flux,
                    cumulative_delta_l_epistemic_bits: tel.null_calibrated_mdl_delta_bits,
                    cumulative_delta_l_epistemic_per_tick: 0.0,
                    frozen,
                    final_port_oracle_correct: false,
                    final_active_comparators: tel.active_comparator_count,
                    epistemic_mdl_err: 0.0,
                    episode_total_node_evals: ledger.total_node_evals(),
                    episode_live_step_node_evals: ledger.live_step_node_evals,
                    episode_candidate_replay_node_evals: ledger.candidate_replay_node_evals,
                    episode_sensitivity_node_evals: ledger.sensitivity_node_evals,
                    cliff: CliffCountersSummary::from(&tel),
                    sleep_telemetry: learner.sleep_telemetry(),
                    first_onset_wake_tick: None,
                    onset_has_incorrect_comp: false,
                    onset_wrong_primary: false,
                    onset_wrong_alt: false,
                    any_segment_has_incorrect_comp: false,
                    recoverable_bits_at_onset: 0.0,
                    recoverable_bits_at_end: 0.0,
                    wrongful_excisions: 0,
                    oracle_correct_port_locks_lost: 0,
                    i6_ok: true,
                    i7_ok: true,
                    pl_excised: false,
                    pl_spurious_extinguished: false,
                    pl_spurious_survival_segments: 0,
                    pl_segments_to_excise: None,
                    pl_refutation_wake_ticks: None,
                    pl_refutation_substrate_ticks: None,
                    spontaneous_incorrect_born: 0,
                    spontaneous_incorrect_refuted: 0,
                    spontaneous_refutation_latencies: Vec::new(),
                    checkpoints_by_wake_step: BTreeMap::new(),
                    dream_probe: None,
                },
                None,
            );
        }

        let total_budget = task.horizon.saturating_mul(100);
        let budget = TickBudget::new(total_budget);
        let start_ledger = learner.compute_ledger();
        let start_ticks = start_ledger.total_substrate_ticks();
        let mut cached_epoch = u64::MAX;
        let mut cached_labels = None;
        if task.planted.is_some() {
            let lbl = label_learner_oracle(&learner, spec, oracle);
            cached_epoch = learner.editable().structure_epoch();
            cached_labels = Some(lbl);
        }
        (
            pl_settled,
            learner,
            env,
            budget,
            start_ledger,
            start_ticks,
            0u64,
            0u64,
            cached_epoch,
            cached_labels,
            false,
            false,
            None,
            None,
            BTreeMap::new(),
            0usize,
            0usize,
            Vec::new(),
            Vec::with_capacity(record_wake_prefix_len.unwrap_or(0)),
            Vec::with_capacity(record_wake_prefix_len.unwrap_or(0)),
            false,
        )
    };

    let ch_usize = usize::from(env.generator().channels());
    let mut clean = vec![0u64; ch_usize];
    let mut obs = vec![0u64; ch_usize];

    let checkpoint_set: BTreeSet<u64> = checkpoint_wake_steps
        .iter()
        .copied()
        .filter(|&step| step > 0 && step < task.horizon)
        .collect();
    let mut checkpoints_by_wake_step = BTreeMap::new();

    let mut onset_captured = false;
    let mut first_onset_wake_tick = None;
    let mut onset_has_incorrect_comp = false;
    let mut onset_wrong_primary = false;
    let mut onset_wrong_alt = false;
    let mut any_segment_has_incorrect_comp = false;
    let mut recoverable_bits_at_onset = 0.0f64;
    let mut wrongful_excisions = 0usize;
    let mut oracle_correct_port_locks_lost = 0usize;
    let mut i6_ok = true;
    let mut i7_ok = true;
    let mut dream_probe: Option<DreamProbeCounts> = None;
    let mut pl_spurious_survival_segments = 0u64;
    let mut pl_segments_to_excise: Option<u64> = None;
    let mut executed_segments = 0u64;
    let mut wake_envelope_armed = false;

    let mut traj = if capture_wake_trajectory && schedule == SleepSchedule::None {
        let mut circuits_by_epoch = BTreeMap::new();
        circuits_by_epoch.insert(
            learner.editable().structure_epoch(),
            Arc::new(learner.editable().circuit().clone()),
        );
        Some(WakeTrajectoryRecord {
            base_env: env.clone(),
            circuits_by_epoch,
            states_by_step: Vec::with_capacity(task.horizon as usize),
            sa_fork: None,
            sb_fork: None,
            sa_skipped: 0,
            sb_skipped: 0,
        })
    } else {
        None
    };

    let get_or_refresh_labels = |learner: &ArkheLearner,
                                 cached_epoch: &mut u64,
                                 cached_labels: &mut Option<CircuitOracleLabels>,
                                 force: bool|
     -> CircuitOracleLabels {
        let cur_epoch = learner.editable().structure_epoch();
        if force || *cached_epoch != cur_epoch || cached_labels.is_none() {
            let lbl = label_learner_oracle(learner, spec, oracle);
            *cached_epoch = cur_epoch;
            *cached_labels = Some(lbl.clone());
            lbl
        } else {
            cached_labels
                .as_ref()
                .expect("cached labels present")
                .clone()
        }
    };

    loop {
        if !enter_sleep_immediately {
            let elapsed_before = learner
                .compute_ledger()
                .total_substrate_ticks()
                .saturating_sub(start_ticks);
            if elapsed_before >= task.horizon {
                break;
            }
            let pre_wake_epoch = learner.editable().structure_epoch();
            let pred = learner.predict(&mut budget);
            env.step_frame(&mut clean, &mut obs);
            let _ = learner.observe(&obs, &mut budget);

            if record_wake_prefix_len.is_some_and(|cap| wake_preds_prefix.len() < cap) {
                wake_preds_prefix.push(pred);
                wake_obs_prefix.push(obs.clone());
            }
            env_steps += 1;
            wake_in_segment += 1;

            let post_wake_epoch = learner.editable().structure_epoch();
            if post_wake_epoch != pre_wake_epoch {
                let had_tracked = !tracked_incorrect.is_empty();
                let has_comps = learner.active_comparator_count() > 0;
                if has_comps || had_tracked || task.planted.is_some() {
                    let lbl = get_or_refresh_labels(
                        &learner,
                        &mut cached_epoch,
                        &mut cached_labels,
                        true,
                    );
                    let elapsed_now = learner
                        .compute_ledger()
                        .total_substrate_ticks()
                        .saturating_sub(start_ticks);
                    if task.planted.is_some()
                        && !pl_excised
                        && !planted_spurious_still_present(&lbl, task.target_port)
                    {
                        pl_excised = true;
                        pl_refutation_wake_ticks = Some(env_steps);
                        pl_refutation_substrate_ticks = Some(elapsed_now);
                        if !planted_spurious_cycle_present(&lbl, task.target_port) {
                            pl_spurious_extinguished = true;
                        }
                    }
                    if task.planted.is_none() {
                        let mut still_incorrect = BTreeSet::new();
                        for c_lbl in &lbl.comparators {
                            if !c_lbl.primary_oracle_correct || !c_lbl.alt_oracle_correct {
                                still_incorrect.insert(c_lbl.sub_node);
                                if let std::collections::btree_map::Entry::Vacant(e) =
                                    tracked_incorrect.entry(c_lbl.sub_node)
                                {
                                    e.insert(elapsed_now);
                                    spontaneous_incorrect_born += 1;
                                }
                            }
                        }
                        let gone: Vec<NodeId> = tracked_incorrect
                            .keys()
                            .filter(|id| !still_incorrect.contains(id))
                            .copied()
                            .collect();
                        for id in gone {
                            if let Some(born_at) = tracked_incorrect.remove(&id) {
                                spontaneous_incorrect_refuted += 1;
                                spontaneous_refutation_latencies
                                    .push(elapsed_now.saturating_sub(born_at));
                            }
                        }
                    }
                }
            }

            if let Some(ref mut tr) = traj {
                tr.circuits_by_epoch
                    .entry(post_wake_epoch)
                    .or_insert_with(|| Arc::new(learner.editable().circuit().clone()));
                tr.states_by_step.push(WakeStepState {
                    cumulative_net_flux: learner.cumulative_net_flux(),
                    prev_observed_frame: learner.prev_observed_frame().to_vec(),
                    registers: learner.editable().state().registers().to_vec(),
                    structure_epoch: post_wake_epoch,
                });
            }

            if checkpoint_set.contains(&env_steps) {
                let tel_cp = learner.telemetry();
                let frozen_cp = evaluate_frozen_window(
                    &learner,
                    &env,
                    task.target_port,
                    w_obs,
                    econ_cfg.split_code_tax,
                );
                checkpoints_by_wake_step.insert(
                    env_steps,
                    CheckpointSnapshot {
                        cumulative_net_flux: tel_cp.cumulative_net_flux,
                        frozen: frozen_cp,
                    },
                );
            }
        }
        let resumed_this_tick = enter_sleep_immediately;
        enter_sleep_immediately = false;

        let elapsed = learner
            .compute_ledger()
            .total_substrate_ticks()
            .saturating_sub(start_ticks);
        let remaining = task.horizon.saturating_sub(elapsed);
        if remaining == 0 {
            break;
        }

        if resumed_this_tick || wake_in_segment >= w_wake {
            wake_in_segment = 0;
            if schedule == SleepSchedule::None {
                if let Some(ref mut tr) = traj {
                    let build_fork = |skipped: u64| PreOnsetForkState {
                        learner: learner.clone(),
                        env: env.clone(),
                        budget,
                        start_ledger,
                        start_ticks,
                        env_steps,
                        skipped_sleep_segments: skipped,
                        cached_epoch,
                        cached_labels: cached_labels.clone(),
                        pl_excised,
                        pl_spurious_extinguished,
                        pl_refutation_wake_ticks,
                        pl_refutation_substrate_ticks,
                        tracked_incorrect: tracked_incorrect.clone(),
                        spontaneous_incorrect_born,
                        spontaneous_incorrect_refuted,
                        spontaneous_refutation_latencies: spontaneous_refutation_latencies.clone(),
                        wake_preds_prefix: wake_preds_prefix.clone(),
                        wake_obs_prefix: wake_obs_prefix.clone(),
                    };
                    if tr.sa_fork.is_none() {
                        if learner_has_scoreable_for_schedule(
                            &learner,
                            SleepSchedule::ConsistencyOnly,
                        ) {
                            tr.sa_fork = Some(build_fork(tr.sa_skipped));
                        } else {
                            tr.sa_skipped += 1;
                        }
                    }
                    if tr.sb_fork.is_none() {
                        if learner_has_scoreable_for_schedule(
                            &learner,
                            SleepSchedule::ConsistencyReplay,
                        ) {
                            tr.sb_fork = Some(build_fork(tr.sb_skipped));
                        } else {
                            tr.sb_skipped += 1;
                        }
                    }
                }
            } else if learner.has_scoreable_comparator_for_sleep() {
                if !wake_envelope_armed {
                    wake_envelope_armed = true;
                    if let Some(cap) = wake_node_eval_cap {
                        learner.set_episode_node_eval_envelope(
                            Some(start_ledger.total_node_evals().saturating_add(cap)),
                            Some(start_ticks.saturating_add(task.horizon)),
                        );
                    }
                }
                let seg_ticks = w_sleep.min(remaining);
                let pre_labels = get_or_refresh_labels(
                    &learner,
                    &mut cached_epoch,
                    &mut cached_labels,
                    task.planted.is_some(),
                );
                let (has_inc, wrong_pri, wrong_alt) =
                    scoreable_comparator_incorrect_flags(&learner, &pre_labels, schedule);
                if has_inc {
                    any_segment_has_incorrect_comp = true;
                }
                if !onset_captured {
                    onset_captured = true;
                    first_onset_wake_tick = Some(env_steps);
                    onset_has_incorrect_comp = has_inc;
                    onset_wrong_primary = wrong_pri;
                    onset_wrong_alt = wrong_alt;
                    recoverable_bits_at_onset = compute_recoverable_comparator_bits(
                        learner.editable().circuit(),
                        learner.editable().economy().comparators(),
                        &pre_labels,
                        econ_cfg.split_code_tax,
                    );
                }

                if run_dream_probe && dream_probe.is_none() {
                    dream_probe = Some(run_perturbed_dream_probe(
                        &learner,
                        oracle,
                        task.target_port,
                        seed,
                        executed_segments as u32,
                    ));
                }

                let (pre_port_scorers, pre_comp_scorers) = snapshot_port_and_comp_scorers(&learner);
                let pre_comp_flux = learner.cumulative_comparator_flux();
                let pre_comps_len = learner.active_comparator_count();
                let all_suspended = learner
                    .editable()
                    .economy()
                    .comparators()
                    .iter()
                    .all(|comp| {
                        let tp = usize::from(comp.target_port);
                        if !learner.channel_is_grounded(tp) {
                            return true;
                        }
                        let mask = comp.primary_channel_mask | comp.alt_channel_mask;
                        (0..64u32).any(|b| {
                            (mask & (1u64 << b)) != 0 && !learner.channel_is_grounded(b as usize)
                        })
                    });

                let seg_report: SleepSegmentReport = learner.run_sleep_segment(seg_ticks);
                executed_segments += 1;

                let (post_port_scorers, _) = snapshot_port_and_comp_scorers(&learner);
                let null_frozen = post_port_scorers
                    .iter()
                    .zip(pre_port_scorers.iter())
                    .all(|(post, pre)| post.3 == pre.3);
                let scorers_unmutated = if seg_report.sleep_excisions == 0 {
                    post_port_scorers == pre_port_scorers
                } else {
                    post_port_scorers.iter().enumerate().all(|(p, post)| {
                        *post == pre_port_scorers[p]
                            || pre_comp_scorers
                                .iter()
                                .any(|(tp, cs)| usize::from(*tp) == p && cs == post)
                    })
                };
                if !null_frozen || !scorers_unmutated {
                    i6_ok = false;
                }
                if pre_comps_len > 0
                    && all_suspended
                    && ((learner.cumulative_comparator_flux() - pre_comp_flux).abs() > 1e-12
                        || seg_report.sleep_excisions > 0)
                {
                    i7_ok = false;
                }

                if seg_report.sleep_excisions > 0 || task.planted.is_some() {
                    let post_labels = get_or_refresh_labels(
                        &learner,
                        &mut cached_epoch,
                        &mut cached_labels,
                        true,
                    );
                    let audit = audit_sleep_segment_oracle(
                        &pre_labels,
                        &post_labels,
                        &seg_report.composed_old_to_new,
                    );
                    wrongful_excisions += audit.wrongful_excisions;
                    oracle_correct_port_locks_lost += audit.oracle_correct_port_locks_lost;

                    let elapsed_after = learner
                        .compute_ledger()
                        .total_substrate_ticks()
                        .saturating_sub(start_ticks);

                    if task.planted.is_some() {
                        let still_spurious =
                            planted_spurious_still_present(&post_labels, task.target_port);
                        if still_spurious {
                            pl_spurious_survival_segments += 1;
                        } else if !pl_excised {
                            pl_excised = true;
                            pl_segments_to_excise = Some(executed_segments);
                            pl_refutation_wake_ticks = Some(env_steps);
                            pl_refutation_substrate_ticks = Some(elapsed_after);
                        }
                        if !planted_spurious_cycle_present(&post_labels, task.target_port) {
                            pl_spurious_extinguished = true;
                        }
                    }

                    if task.planted.is_none() {
                        let mut still_incorrect = BTreeSet::new();
                        for c_lbl in &post_labels.comparators {
                            if !c_lbl.primary_oracle_correct || !c_lbl.alt_oracle_correct {
                                still_incorrect.insert(c_lbl.sub_node);
                            }
                        }
                        let gone: Vec<NodeId> = tracked_incorrect
                            .keys()
                            .filter(|id| !still_incorrect.contains(id))
                            .copied()
                            .collect();
                        for id in gone {
                            if let Some(born_at) = tracked_incorrect.remove(&id) {
                                spontaneous_incorrect_refuted += 1;
                                spontaneous_refutation_latencies
                                    .push(elapsed_after.saturating_sub(born_at));
                            }
                        }
                    }
                } else if task.planted.is_none() {
                    // No sleep edits => topology and oracle labels unchanged (`0` wrongful excisions).
                    cached_epoch = learner.editable().structure_epoch();
                }
            } else {
                learner.record_skipped_sleep_segment();
            }
        }
    }

    let end_ledger = learner.compute_ledger();
    let episode_ticks = end_ledger
        .total_substrate_ticks()
        .saturating_sub(start_ticks);
    let trace = learner
        .take_epistemic_audit()
        .expect("epistemic audit enabled");
    let cons = verify_epistemic_mdl_conservation(&trace, &learner, episode_ticks);
    let epistemic_mdl_err = cons
        .flux_mdl_err_bits_per_symbol
        .max(cons.energy_flux_err_bits_per_symbol);

    let end_labels = get_or_refresh_labels(
        &learner,
        &mut cached_epoch,
        &mut cached_labels,
        task.planted.is_some(),
    );
    let recoverable_bits_at_end = compute_recoverable_comparator_bits(
        learner.editable().circuit(),
        learner.editable().economy().comparators(),
        &end_labels,
        econ_cfg.split_code_tax,
    );
    if task.planted.is_some() {
        if !planted_spurious_still_present(&end_labels, task.target_port) {
            pl_excised = true;
        }
        if !planted_spurious_cycle_present(&end_labels, task.target_port) {
            pl_spurious_extinguished = true;
        }
    }
    let frozen = evaluate_frozen_window(
        &learner,
        &env,
        task.target_port,
        w_obs,
        econ_cfg.split_code_tax,
    );

    let tel = learner.telemetry();
    let sleep_telemetry = learner.sleep_telemetry();
    let facts_forgone = episode_ticks.saturating_sub(env_steps);
    let final_circuit_code_bits =
        circuit_code_bits_with_tax(learner.editable().circuit(), 0, econ_cfg.split_code_tax);

    let outcome = ArmRunOutcome {
        pl_settled,
        episode_ticks,
        env_steps,
        facts_forgone,
        wake_preds_prefix,
        wake_obs_prefix,
        final_circuit_code_bits,
        final_node_count: tel.final_node_count,
        cumulative_net_flux: tel.cumulative_net_flux,
        cumulative_delta_l_epistemic_bits: tel.null_calibrated_mdl_delta_bits,
        cumulative_delta_l_epistemic_per_tick: tel.null_calibrated_mdl_delta_bits
            / (episode_ticks.max(1) as f64),
        frozen,
        final_port_oracle_correct: end_labels
            .oracle_correct_ports
            .get(usize::from(task.target_port))
            .copied()
            .unwrap_or(false),
        final_active_comparators: tel.active_comparator_count,
        epistemic_mdl_err,
        episode_total_node_evals: end_ledger
            .total_node_evals()
            .saturating_sub(start_ledger.total_node_evals()),
        episode_live_step_node_evals: end_ledger
            .live_step_node_evals
            .saturating_sub(start_ledger.live_step_node_evals),
        episode_candidate_replay_node_evals: end_ledger
            .candidate_replay_node_evals
            .saturating_sub(start_ledger.candidate_replay_node_evals),
        episode_sensitivity_node_evals: end_ledger
            .sensitivity_node_evals
            .saturating_sub(start_ledger.sensitivity_node_evals),
        cliff: CliffCountersSummary::from(&tel),
        sleep_telemetry,
        first_onset_wake_tick,
        onset_has_incorrect_comp,
        onset_wrong_primary,
        onset_wrong_alt,
        any_segment_has_incorrect_comp,
        recoverable_bits_at_onset,
        recoverable_bits_at_end,
        wrongful_excisions,
        oracle_correct_port_locks_lost,
        i6_ok,
        i7_ok,
        pl_excised,
        pl_spurious_extinguished,
        pl_spurious_survival_segments,
        pl_segments_to_excise,
        pl_refutation_wake_ticks,
        pl_refutation_substrate_ticks,
        spontaneous_incorrect_born,
        spontaneous_incorrect_refuted,
        spontaneous_refutation_latencies,
        checkpoints_by_wake_step,
        dream_probe,
    };
    (outcome, traj)
}

struct PairedSeedResult {
    wake: ArmRunOutcome,
    sa: ArmRunOutcome,
    sb: ArmRunOutcome,
    i2_sa_pre_onset_ok: bool,
    i2_sb_pre_onset_ok: bool,
    i2_sa_no_onset_exact: Option<bool>,
    i2_sb_no_onset_exact: Option<bool>,
    sensitivity_runs: Vec<(SleepSchedule, u64, ArmRunOutcome)>,
}

fn check_i2_pre_onset(wake: &ArmRunOutcome, sleep: &ArmRunOutcome) -> (bool, Option<bool>) {
    let prefix_limit = wake
        .wake_preds_prefix
        .len()
        .min(sleep.wake_preds_prefix.len());
    let pre_len = sleep
        .first_onset_wake_tick
        .unwrap_or(prefix_limit as u64)
        .min(prefix_limit as u64) as usize;
    let pre_ok = wake.wake_preds_prefix[..pre_len] == sleep.wake_preds_prefix[..pre_len]
        && wake.wake_obs_prefix[..pre_len] == sleep.wake_obs_prefix[..pre_len];
    let no_onset_exact = if sleep.sleep_telemetry.sleep_segments_executed == 0 {
        Some(
            pre_ok
                && wake.final_circuit_code_bits.to_bits()
                    == sleep.final_circuit_code_bits.to_bits()
                && wake.cumulative_net_flux.to_bits() == sleep.cumulative_net_flux.to_bits(),
        )
    } else {
        None
    };
    (pre_ok, no_onset_exact)
}

fn run_paired_task_seed(
    task: &M4aTaskSpec,
    spec: &FiniteStateSpec,
    oracle: &PortLanguageOracle,
    seed: u64,
    include_sensitivity: bool,
) -> PairedSeedResult {
    let econ_cfg = EconomyConfig::default();
    let growth_cfg = GrowthConfig::default();
    let w_obs = observation_horizon(&econ_cfg, &growth_cfg) as u64;
    let run_dream =
        include_sensitivity && matches!(task.family, M4aTaskFamily::Sp | M4aTaskFamily::Pl);

    let (mut wake, traj_opt) = run_arm_episode_core(
        task,
        spec,
        oracle,
        seed,
        SleepSchedule::None,
        None,
        &[],
        Some(task.horizon as usize),
        false,
        None,
        None,
        true,
    );
    let mut traj = traj_opt.expect("wake trajectory captured for paired run");
    let wake_node_evals = wake.episode_total_node_evals;

    let mut sa = if let Some(fork) = traj.sa_fork.take() {
        run_arm_episode_core(
            task,
            spec,
            oracle,
            seed,
            SleepSchedule::ConsistencyOnly,
            None,
            &[],
            Some(task.horizon as usize),
            run_dream,
            Some(wake_node_evals),
            Some(fork),
            false,
        )
        .0
    } else {
        let mut cloned = wake.clone();
        cloned
            .sleep_telemetry
            .sleep_segment_skipped_no_scoreable_comparator = traj.sa_skipped;
        cloned
    };

    let mut sb = if let Some(fork) = traj.sb_fork.take() {
        run_arm_episode_core(
            task,
            spec,
            oracle,
            seed,
            SleepSchedule::ConsistencyReplay,
            None,
            &[],
            Some(task.horizon as usize),
            false,
            Some(wake_node_evals),
            Some(fork),
            false,
        )
        .0
    } else {
        let mut cloned = wake.clone();
        cloned
            .sleep_telemetry
            .sleep_segment_skipped_no_scoreable_comparator = traj.sb_skipped;
        cloned
    };

    let checkpoints = [sa.env_steps, sb.env_steps];
    wake.checkpoints_by_wake_step =
        traj.resolve_checkpoints(&checkpoints, econ_cfg, task.target_port, w_obs);

    let (i2_sa_pre_onset_ok, i2_sa_no_onset_exact) = check_i2_pre_onset(&wake, &sa);
    let (i2_sb_pre_onset_ok, i2_sb_no_onset_exact) = check_i2_pre_onset(&wake, &sb);
    // Drop large prefix vectors once I2 is verified to keep memory compact across 500 seeds.
    wake.wake_preds_prefix.clear();
    wake.wake_preds_prefix.shrink_to_fit();
    wake.wake_obs_prefix.clear();
    wake.wake_obs_prefix.shrink_to_fit();

    sa.wake_preds_prefix.clear();
    sa.wake_preds_prefix.shrink_to_fit();
    sa.wake_obs_prefix.clear();
    sa.wake_obs_prefix.shrink_to_fit();
    sb.wake_preds_prefix.clear();
    sb.wake_preds_prefix.shrink_to_fit();
    sb.wake_obs_prefix.clear();
    sb.wake_obs_prefix.shrink_to_fit();

    let mut sensitivity_runs = Vec::new();
    if include_sensitivity && matches!(task.family, M4aTaskFamily::Sp | M4aTaskFamily::Pl) {
        for &w_sleep in &SENSITIVITY_SLEEP_WINDOWS {
            if w_sleep == 256 {
                continue;
            }
            for schedule in [
                SleepSchedule::ConsistencyOnly,
                SleepSchedule::ConsistencyReplay,
            ] {
                let res = run_arm_episode(
                    task,
                    spec,
                    oracle,
                    seed,
                    schedule,
                    Some(w_sleep),
                    &[],
                    None,
                    false,
                    Some(wake_node_evals),
                );
                sensitivity_runs.push((schedule, w_sleep, res));
            }
        }
    }

    PairedSeedResult {
        wake,
        sa,
        sb,
        i2_sa_pre_onset_ok,
        i2_sb_pre_onset_ok,
        i2_sa_no_onset_exact,
        i2_sb_no_onset_exact,
        sensitivity_runs,
    }
}

fn summarize_arm_outcomes(
    task_horizon: u64,
    outcomes: &[&ArmRunOutcome],
) -> ArmDistributionSummary {
    let n = outcomes.len().max(1) as f64;
    let mut eval_l_epi = Vec::with_capacity(outcomes.len());
    let mut eval_l_epi_t = Vec::with_capacity(outcomes.len());
    let mut eval_dl_epi = Vec::with_capacity(outcomes.len());
    let mut eval_dl_epi_t = Vec::with_capacity(outcomes.len());
    let mut cum_dl_epi = Vec::with_capacity(outcomes.len());
    let mut cum_dl_epi_pt = Vec::with_capacity(outcomes.len());
    let mut final_bits = Vec::with_capacity(outcomes.len());
    let mut kt_all = Vec::with_capacity(outcomes.len());
    let mut kt_tgt = Vec::with_capacity(outcomes.len());
    let mut node_cnt = Vec::with_capacity(outcomes.len());

    let mut oracle_ok = 0usize;
    let mut clean_zero = 0usize;
    let mut active_comp = 0usize;
    let mut active_comp_sum = 0usize;
    let mut max_mdl_err = 0.0f64;
    let mut max_ledger_diff = 0u64;
    let mut tot_evals = 0u64;
    let mut live_evals = 0u64;
    let mut cand_evals = 0u64;
    let mut sens_evals = 0u64;
    let mut cliff = CliffCountersSummary::default();
    let mut blocked_max_nodes_runs = 0usize;
    let mut budget_exh_runs = 0usize;
    let mut lag_bound_runs = 0usize;
    let mut cand_undec_runs = 0usize;
    let mut sleep_totals = SleepTelemetryTotals::default();

    for r in outcomes {
        eval_l_epi.push(r.frozen.eval_l_epistemic_bits);
        eval_l_epi_t.push(r.frozen.eval_l_epistemic_target_bits);
        eval_dl_epi.push(r.frozen.eval_delta_l_epistemic_bits);
        eval_dl_epi_t.push(r.frozen.eval_delta_l_epistemic_target_bits);
        cum_dl_epi.push(r.cumulative_delta_l_epistemic_bits);
        cum_dl_epi_pt.push(r.cumulative_delta_l_epistemic_per_tick);
        final_bits.push(r.final_circuit_code_bits);
        kt_all.push(r.frozen.eval_kt_all_ch);
        kt_tgt.push(r.frozen.eval_kt_target);
        node_cnt.push(r.final_node_count as f64);

        if r.final_port_oracle_correct {
            oracle_ok += 1;
        }
        if r.frozen.eval_clean_zero_error_target {
            clean_zero += 1;
        }
        if r.final_active_comparators >= 1 {
            active_comp += 1;
        }
        active_comp_sum += r.final_active_comparators;
        max_mdl_err = max_mdl_err.max(r.epistemic_mdl_err);
        let target_ticks = if r.pl_settled { task_horizon } else { 0 };
        max_ledger_diff = max_ledger_diff.max(r.episode_ticks.abs_diff(target_ticks));

        tot_evals = tot_evals.saturating_add(r.episode_total_node_evals);
        live_evals = live_evals.saturating_add(r.episode_live_step_node_evals);
        cand_evals = cand_evals.saturating_add(r.episode_candidate_replay_node_evals);
        sens_evals = sens_evals.saturating_add(r.episode_sensitivity_node_evals);

        cliff.accumulate(&r.cliff);
        if r.cliff.sprout_blocked_max_nodes > 0 {
            blocked_max_nodes_runs += 1;
        }
        if r.cliff.sprout_budget_exhausted > 0 {
            budget_exh_runs += 1;
        }
        if r.cliff.lag_horizon_bound > 0 {
            lag_bound_runs += 1;
        }
        if r.cliff.candidate_undecidable > 0 {
            cand_undec_runs += 1;
        }

        let st = &r.sleep_telemetry;
        sleep_totals.sleep_segments_executed += st.sleep_segments_executed;
        sleep_totals.sleep_segment_skipped_no_scoreable_comparator +=
            st.sleep_segment_skipped_no_scoreable_comparator;
        sleep_totals.sleep_excisions += st.sleep_excisions;
        sleep_totals.sleep_minority_routes_pruned += st.sleep_minority_routes_pruned;
        sleep_totals.sleep_starvation_excisions += st.sleep_starvation_excisions;
        sleep_totals.sleep_comparator_suspended_ungrounded +=
            st.sleep_comparator_suspended_ungrounded;
        sleep_totals.max_suspended_comparator_ticks = sleep_totals
            .max_suspended_comparator_ticks
            .max(st.sleep_comparator_suspended_ungrounded);
        sleep_totals.sleep_match_failures += st.sleep_match_failures;
        sleep_totals.fact_buffer_evictions += st.fact_buffer_evictions;
        sleep_totals.total_facts_forgone += r.facts_forgone;
        sleep_totals.max_facts_forgone = sleep_totals.max_facts_forgone.max(r.facts_forgone);
    }
    sleep_totals.mean_facts_forgone = (sleep_totals.total_facts_forgone as f64) / n;

    ArmDistributionSummary {
        eval_l_epistemic_bits: summarize_scalars(&eval_l_epi),
        eval_l_epistemic_target_bits: summarize_scalars(&eval_l_epi_t),
        eval_delta_l_epistemic_bits: summarize_scalars(&eval_dl_epi),
        eval_delta_l_epistemic_target_bits: summarize_scalars(&eval_dl_epi_t),
        cumulative_delta_l_epistemic_bits: summarize_scalars(&cum_dl_epi),
        cumulative_delta_l_epistemic_per_tick: summarize_scalars(&cum_dl_epi_pt),
        final_circuit_code_bits: summarize_scalars(&final_bits),
        eval_kt_all_ch_bits_per_symbol: summarize_scalars(&kt_all),
        eval_kt_target_bits_per_symbol: summarize_scalars(&kt_tgt),
        final_node_count: summarize_scalars(&node_cnt),
        final_port_oracle_correct_rate: (oracle_ok as f64) / n,
        eval_clean_zero_error_rate: (clean_zero as f64) / n,
        active_comparator_rate: (active_comp as f64) / n,
        mean_active_comparators: (active_comp_sum as f64) / n,
        max_epistemic_mdl_err: max_mdl_err,
        max_compute_ledger_diff: max_ledger_diff,
        mean_episode_node_evals: (tot_evals as f64) / n,
        mean_live_step_node_evals: (live_evals as f64) / n,
        mean_candidate_replay_node_evals: (cand_evals as f64) / n,
        mean_sensitivity_node_evals: (sens_evals as f64) / n,
        cliff_counters: cliff,
        sprout_blocked_max_nodes_runs: blocked_max_nodes_runs,
        sprout_budget_exhausted_runs: budget_exh_runs,
        lag_horizon_bound_runs: lag_bound_runs,
        candidate_undecidable_runs: cand_undec_runs,
        sleep_telemetry_totals: sleep_totals,
    }
}

fn sleep_over_wake_node_eval_ratio(wake: &ArmRunOutcome, sleep: &ArmRunOutcome) -> f64 {
    let wake_active = wake
        .episode_candidate_replay_node_evals
        .saturating_add(wake.episode_sensitivity_node_evals);
    let sleep_active = sleep
        .episode_candidate_replay_node_evals
        .saturating_add(sleep.episode_sensitivity_node_evals);
    let passive_live_only = sleep_active <= wake_active
        && sleep.episode_candidate_replay_node_evals <= wake.episode_candidate_replay_node_evals
        && sleep.episode_sensitivity_node_evals <= wake.episode_sensitivity_node_evals
        && sleep.episode_live_step_node_evals > wake.episode_live_step_node_evals;
    let denom = if passive_live_only {
        wake.episode_total_node_evals.max(
            sleep
                .episode_live_step_node_evals
                .saturating_add(wake_active),
        )
    } else {
        wake.episode_total_node_evals
    }
    .max(1) as f64;
    (sleep.episode_total_node_evals as f64) / denom
}

fn summarize_sleep_arm(
    task: &M4aTaskSpec,
    pairs: &[(&ArmRunOutcome, &ArmRunOutcome, bool, Option<bool>)],
) -> SleepArmSummary {
    let is_pl = task.family == M4aTaskFamily::Pl;
    let sleep_refs: Vec<&ArmRunOutcome> = pairs.iter().map(|(_, s, _, _)| *s).collect();
    let arm = summarize_arm_outcomes(task.horizon, &sleep_refs);
    let n = pairs.len().max(1) as f64;
    let num_out_f = f64::from(task.target_port.max(1));
    let eval_ch_count = {
        let ch = outcomes_num_outputs_hint(task);
        f64::from(ch)
    };
    let _ = num_out_f;
    let w_obs_f = observation_horizon(&EconomyConfig::default(), &GrowthConfig::default()) as f64;

    let mut ratios = Vec::with_capacity(pairs.len());
    let mut le_1 = 0usize;
    let mut live_ratios = Vec::with_capacity(pairs.len());
    let mut cand_ratios = Vec::with_capacity(pairs.len());
    let mut sens_ratios = Vec::with_capacity(pairs.len());

    let mut i2_ok_cnt = 0usize;
    let mut no_onset_cnt = 0usize;
    let mut no_onset_exact_cnt = 0usize;
    let mut i6_ok_cnt = 0usize;
    let mut i7_ok_cnt = 0usize;

    let mut eval_l_epi_adv = Vec::with_capacity(pairs.len());
    let mut eval_l_epi_t_adv = Vec::with_capacity(pairs.len());
    let mut eval_dl_epi_adv = Vec::with_capacity(pairs.len());
    let mut cum_dl_epi_adv = Vec::with_capacity(pairs.len());
    let mut cum_dl_epi_pt_adv = Vec::with_capacity(pairs.len());
    let mut code_bits_adv = Vec::with_capacity(pairs.len());
    let mut kt_reg_all = Vec::with_capacity(pairs.len());
    let mut kt_reg_tgt = Vec::with_capacity(pairs.len());

    let mut pl_excised_cnt = 0usize;
    let mut pl_ext_cnt = 0usize;
    let mut pl_surv_segs = Vec::new();
    let mut pl_segs_to_excise = Vec::new();
    let mut pl_sleep_wake_lat = Vec::new();
    let mut pl_sleep_sub_lat = Vec::new();
    let mut pl_wake_lat = Vec::new();

    let mut tot_wrongful = 0usize;
    let mut max_wrongful = 0usize;
    let mut runs_wrongful = 0usize;
    let mut tot_locks_lost = 0usize;
    let mut max_locks_lost = 0usize;

    let mut onset_cnt = 0usize;
    let mut t_onsets = Vec::new();
    let mut onset_inc_cnt = 0usize;
    let mut onset_wpri_cnt = 0usize;
    let mut onset_walt_cnt = 0usize;
    let mut any_seg_inc_cnt = 0usize;
    let mut recov_onset_all = Vec::with_capacity(pairs.len());
    let mut recov_onset_given = Vec::new();
    let mut recov_wake_end = Vec::with_capacity(pairs.len());
    let mut recov_sleep_end = Vec::with_capacity(pairs.len());
    let mut fact_loss_eval = Vec::with_capacity(pairs.len());
    let mut fact_loss_kt = Vec::with_capacity(pairs.len());
    let mut fact_loss_flux = Vec::with_capacity(pairs.len());
    let mut opp_onset_eval = Vec::with_capacity(pairs.len());
    let mut opp_wake_end_eval = Vec::with_capacity(pairs.len());
    let mut opp_onset_cum = Vec::with_capacity(pairs.len());
    let mut opp_wake_end_cum = Vec::with_capacity(pairs.len());

    let mut spont_born = 0usize;
    let mut spont_ref_wake = 0usize;
    let mut spont_ref_sleep = 0usize;
    let mut spont_wake_lats = Vec::new();
    let mut spont_sleep_lats = Vec::new();

    for &(wake, sleep, i2_pre, i2_no_onset) in pairs {
        let ratio = sleep_over_wake_node_eval_ratio(wake, sleep);
        if ratio <= 1.0 + 1e-12 {
            le_1 += 1;
        }
        ratios.push(ratio);
        live_ratios.push(
            (sleep.episode_live_step_node_evals as f64)
                / (wake.episode_live_step_node_evals.max(1) as f64),
        );
        cand_ratios.push(
            (sleep.episode_candidate_replay_node_evals as f64)
                / (wake.episode_candidate_replay_node_evals.max(1) as f64),
        );
        sens_ratios.push(
            (sleep.episode_sensitivity_node_evals as f64)
                / (wake.episode_sensitivity_node_evals.max(1) as f64),
        );

        if i2_pre {
            i2_ok_cnt += 1;
        }
        if let Some(exact) = i2_no_onset {
            no_onset_cnt += 1;
            if exact {
                no_onset_exact_cnt += 1;
            }
        }
        if sleep.i6_ok {
            i6_ok_cnt += 1;
        }
        if sleep.i7_ok {
            i7_ok_cnt += 1;
        }

        eval_l_epi_adv.push(wake.frozen.eval_l_epistemic_bits - sleep.frozen.eval_l_epistemic_bits);
        eval_l_epi_t_adv.push(
            wake.frozen.eval_l_epistemic_target_bits - sleep.frozen.eval_l_epistemic_target_bits,
        );
        eval_dl_epi_adv.push(
            wake.frozen.eval_delta_l_epistemic_bits - sleep.frozen.eval_delta_l_epistemic_bits,
        );
        cum_dl_epi_adv
            .push(wake.cumulative_delta_l_epistemic_bits - sleep.cumulative_delta_l_epistemic_bits);
        cum_dl_epi_pt_adv.push(
            wake.cumulative_delta_l_epistemic_per_tick
                - sleep.cumulative_delta_l_epistemic_per_tick,
        );
        code_bits_adv.push(wake.final_circuit_code_bits - sleep.final_circuit_code_bits);
        kt_reg_all.push(sleep.frozen.eval_kt_all_ch - wake.frozen.eval_kt_all_ch);
        kt_reg_tgt.push(sleep.frozen.eval_kt_target - wake.frozen.eval_kt_target);

        if is_pl {
            if sleep.pl_excised {
                pl_excised_cnt += 1;
            }
            if sleep.pl_spurious_extinguished {
                pl_ext_cnt += 1;
            }
            pl_surv_segs.push(sleep.pl_spurious_survival_segments as f64);
            if let Some(s) = sleep.pl_segments_to_excise {
                pl_segs_to_excise.push(s as f64);
            }
            pl_sleep_wake_lat.push(sleep.pl_refutation_wake_ticks);
            pl_sleep_sub_lat.push(sleep.pl_refutation_substrate_ticks);
            pl_wake_lat.push(wake.pl_refutation_wake_ticks);
        }

        tot_wrongful += sleep.wrongful_excisions;
        max_wrongful = max_wrongful.max(sleep.wrongful_excisions);
        if sleep.wrongful_excisions > 0 {
            runs_wrongful += 1;
        }
        tot_locks_lost += sleep.oracle_correct_port_locks_lost;
        max_locks_lost = max_locks_lost.max(sleep.oracle_correct_port_locks_lost);

        if let Some(t_on) = sleep.first_onset_wake_tick {
            onset_cnt += 1;
            t_onsets.push(t_on as f64);
            recov_onset_given.push(sleep.recoverable_bits_at_onset);
            if sleep.onset_has_incorrect_comp {
                onset_inc_cnt += 1;
            }
            if sleep.onset_wrong_primary {
                onset_wpri_cnt += 1;
            }
            if sleep.onset_wrong_alt {
                onset_walt_cnt += 1;
            }
            if sleep.any_segment_has_incorrect_comp {
                any_seg_inc_cnt += 1;
            }
        }
        recov_onset_all.push(sleep.recoverable_bits_at_onset);
        recov_wake_end.push(wake.recoverable_bits_at_end);
        recov_sleep_end.push(sleep.recoverable_bits_at_end);

        let (fl_eval, fl_kt, fl_flux) = if sleep.facts_forgone == 0 {
            (0.0, 0.0, 0.0)
        } else if let Some(cp) = wake.checkpoints_by_wake_step.get(&sleep.env_steps) {
            let d_eval = cp.frozen.eval_l_epistemic_bits - wake.frozen.eval_l_epistemic_bits;
            let d_kt =
                w_obs_f * eval_ch_count * (cp.frozen.eval_kt_all_ch - wake.frozen.eval_kt_all_ch);
            let d_flux = wake.cumulative_net_flux - cp.cumulative_net_flux;
            (d_eval, d_kt, d_flux)
        } else {
            (0.0, 0.0, 0.0)
        };
        fact_loss_eval.push(fl_eval);
        fact_loss_kt.push(fl_kt);
        fact_loss_flux.push(fl_flux);
        opp_onset_eval.push(sleep.recoverable_bits_at_onset - fl_eval);
        opp_wake_end_eval.push(wake.recoverable_bits_at_end - fl_eval);
        opp_onset_cum.push(sleep.recoverable_bits_at_onset - fl_flux);
        opp_wake_end_cum.push(wake.recoverable_bits_at_end - fl_flux);

        spont_born += sleep.spontaneous_incorrect_born;
        spont_ref_wake += wake.spontaneous_incorrect_refuted;
        spont_ref_sleep += sleep.spontaneous_incorrect_refuted;
        spont_wake_lats.extend_from_slice(&wake.spontaneous_refutation_latencies);
        spont_sleep_lats.extend_from_slice(&sleep.spontaneous_refutation_latencies);
    }

    let ratio_stats = summarize_scalars(&ratios);
    SleepArmSummary {
        arm,
        node_eval_ratio: ratio_stats,
        frac_node_eval_ratio_le_1: (le_1 as f64) / n,
        live_step_eval_ratio_mean: live_ratios.iter().sum::<f64>() / n,
        candidate_replay_eval_ratio_mean: cand_ratios.iter().sum::<f64>() / n,
        sensitivity_eval_ratio_mean: sens_ratios.iter().sum::<f64>() / n,
        i2_pre_onset_bit_identical_rate: (i2_ok_cnt as f64) / n,
        i2_no_onset_exact_match_rate: if no_onset_cnt == 0 {
            1.0
        } else {
            (no_onset_exact_cnt as f64) / (no_onset_cnt as f64)
        },
        i6_null_frozen_and_zero_boundary_reward_rate: (i6_ok_cnt as f64) / n,
        i7_suspended_zero_flux_rate: (i7_ok_cnt as f64) / n,
        eval_l_epi_advantage_bits: summarize_paired_deltas(&eval_l_epi_adv),
        eval_l_epi_target_advantage_bits: summarize_paired_deltas(&eval_l_epi_t_adv),
        eval_delta_l_epi_advantage_bits: summarize_paired_deltas(&eval_dl_epi_adv),
        cumulative_delta_l_epi_advantage_bits: summarize_paired_deltas(&cum_dl_epi_adv),
        cumulative_delta_l_epi_per_tick_advantage: summarize_paired_deltas(&cum_dl_epi_pt_adv),
        final_code_bits_advantage: summarize_paired_deltas(&code_bits_adv),
        eval_kt_regret_all_ch: summarize_paired_deltas(&kt_reg_all),
        eval_kt_regret_target: summarize_paired_deltas(&kt_reg_tgt),
        pl_excision_rate: is_pl.then(|| (pl_excised_cnt as f64) / n),
        pl_spurious_cycle_extinction_rate: is_pl.then(|| (pl_ext_cnt as f64) / n),
        spurious_survival_segments: is_pl.then(|| summarize_scalars(&pl_surv_segs)),
        segments_to_excise: (is_pl && !pl_segs_to_excise.is_empty())
            .then(|| summarize_scalars(&pl_segs_to_excise)),
        sleep_refutation_latency_wake_ticks: if is_pl {
            summarize_opt_u64(&pl_sleep_wake_lat)
        } else {
            None
        },
        sleep_refutation_latency_substrate_ticks: if is_pl {
            summarize_opt_u64(&pl_sleep_sub_lat)
        } else {
            None
        },
        wake_refutation_latency_ticks: if is_pl {
            summarize_opt_u64(&pl_wake_lat)
        } else {
            None
        },
        total_wrongful_excisions: tot_wrongful,
        max_wrongful_excisions: max_wrongful,
        runs_with_wrongful_excision: runs_wrongful,
        total_oracle_correct_port_locks_lost: tot_locks_lost,
        max_oracle_correct_port_locks_lost: max_locks_lost,
        onset_rate: (onset_cnt as f64) / n,
        mean_t_onset: (!t_onsets.is_empty())
            .then(|| t_onsets.iter().sum::<f64>() / (t_onsets.len() as f64)),
        max_t_onset: pairs
            .iter()
            .filter_map(|(_, s, _, _)| s.first_onset_wake_tick)
            .max(),
        p_onset_oracle_incorrect_given_onset: (onset_cnt > 0)
            .then(|| (onset_inc_cnt as f64) / (onset_cnt as f64)),
        p_onset_oracle_incorrect_overall: (onset_cnt as f64 / n)
            * if onset_cnt > 0 {
                (onset_inc_cnt as f64) / (onset_cnt as f64)
            } else {
                0.0
            },
        p_onset_wrong_primary_overall: (onset_wpri_cnt as f64) / n,
        p_onset_wrong_alt_overall: (onset_walt_cnt as f64) / n,
        p_any_segment_oracle_incorrect_given_onset: (onset_cnt > 0)
            .then(|| (any_seg_inc_cnt as f64) / (onset_cnt as f64)),
        p_any_segment_oracle_incorrect_overall: (any_seg_inc_cnt as f64) / n,
        recoverable_bits_at_onset: summarize_scalars(&recov_onset_all),
        recoverable_bits_at_onset_given_onset: (!recov_onset_given.is_empty())
            .then(|| summarize_scalars(&recov_onset_given)),
        recoverable_bits_at_wake_end: summarize_scalars(&recov_wake_end),
        recoverable_bits_at_sleep_end: summarize_scalars(&recov_sleep_end),
        fact_loss_eval_bits: summarize_scalars(&fact_loss_eval),
        fact_loss_eval_kt_bits: summarize_scalars(&fact_loss_kt),
        fact_loss_cum_flux_bits: summarize_scalars(&fact_loss_flux),
        opportunity_bound_onset_eval_bits: summarize_scalars(&opp_onset_eval),
        opportunity_bound_wake_end_eval_bits: summarize_scalars(&opp_wake_end_eval),
        opportunity_bound_onset_cum_bits: summarize_scalars(&opp_onset_cum),
        opportunity_bound_wake_end_cum_bits: summarize_scalars(&opp_wake_end_cum),
        spontaneous_incorrect_comp_born_count: spont_born,
        spontaneous_refuted_wake_only_count: spont_ref_wake,
        spontaneous_refuted_sleep_arm_count: spont_ref_sleep,
        spontaneous_wake_refutation_latency: summarize_u64_slice(&spont_wake_lats),
        spontaneous_sleep_refutation_latency: summarize_u64_slice(&spont_sleep_lats),
    }
}

fn outcomes_num_outputs_hint(task: &M4aTaskSpec) -> u16 {
    if task.target_port >= 2 { 3 } else { 1 }
}

fn summarize_task_cell(
    task: &M4aTaskSpec,
    num_channels: u16,
    seed_results: &[&PairedSeedResult],
) -> TaskCellSummary {
    let wake_refs: Vec<&ArmRunOutcome> = seed_results.iter().map(|r| &r.wake).collect();
    let wake_only = summarize_arm_outcomes(task.horizon, &wake_refs);

    let sa_pairs: Vec<(&ArmRunOutcome, &ArmRunOutcome, bool, Option<bool>)> = seed_results
        .iter()
        .map(|r| (&r.wake, &r.sa, r.i2_sa_pre_onset_ok, r.i2_sa_no_onset_exact))
        .collect();
    let sb_pairs: Vec<(&ArmRunOutcome, &ArmRunOutcome, bool, Option<bool>)> = seed_results
        .iter()
        .map(|r| (&r.wake, &r.sb, r.i2_sb_pre_onset_ok, r.i2_sb_no_onset_exact))
        .collect();
    let sa = summarize_sleep_arm(task, &sa_pairs);
    let sb = summarize_sleep_arm(task, &sb_pairs);

    let sb_vs_sa_eval: Vec<f64> = seed_results
        .iter()
        .map(|r| r.sa.frozen.eval_l_epistemic_bits - r.sb.frozen.eval_l_epistemic_bits)
        .collect();
    let sb_vs_sa_eval_dl: Vec<f64> = seed_results
        .iter()
        .map(|r| r.sa.frozen.eval_delta_l_epistemic_bits - r.sb.frozen.eval_delta_l_epistemic_bits)
        .collect();
    let sb_vs_sa_cum: Vec<f64> = seed_results
        .iter()
        .map(|r| r.sa.cumulative_delta_l_epistemic_bits - r.sb.cumulative_delta_l_epistemic_bits)
        .collect();
    let sb_vs_sa_code: Vec<f64> = seed_results
        .iter()
        .map(|r| r.sa.final_circuit_code_bits - r.sb.final_circuit_code_bits)
        .collect();
    let sb_vs_sa_kt_all: Vec<f64> = seed_results
        .iter()
        .map(|r| r.sb.frozen.eval_kt_all_ch - r.sa.frozen.eval_kt_all_ch)
        .collect();
    let sb_vs_sa_kt_tgt: Vec<f64> = seed_results
        .iter()
        .map(|r| r.sb.frozen.eval_kt_target - r.sa.frozen.eval_kt_target)
        .collect();

    let h6_contrast = H6PairedContrast {
        sb_minus_sa_eval_l_epi_advantage: summarize_paired_deltas(&sb_vs_sa_eval),
        sb_minus_sa_eval_delta_l_epi_advantage: summarize_paired_deltas(&sb_vs_sa_eval_dl),
        sb_minus_sa_cum_delta_l_epi_advantage: summarize_paired_deltas(&sb_vs_sa_cum),
        sb_minus_sa_final_code_bits_advantage: summarize_paired_deltas(&sb_vs_sa_code),
        sb_minus_sa_eval_kt_diff_all_ch: summarize_paired_deltas(&sb_vs_sa_kt_all),
        sb_minus_sa_eval_kt_diff_target: summarize_paired_deltas(&sb_vs_sa_kt_tgt),
        sa_pl_excision_rate: sa.pl_excision_rate,
        sb_pl_excision_rate: sb.pl_excision_rate,
        sa_mean_sleep_refutation_latency: sa
            .sleep_refutation_latency_substrate_ticks
            .as_ref()
            .map(|s| s.mean),
        sb_mean_sleep_refutation_latency: sb
            .sleep_refutation_latency_substrate_ticks
            .as_ref()
            .map(|s| s.mean),
    };

    let pl_settled_runs = seed_results
        .iter()
        .filter(|r| r.wake.pl_settled && r.sa.pl_settled && r.sb.pl_settled)
        .count();

    TaskCellSummary {
        task_id: task.id.clone(),
        family: family_name(task.family).to_owned(),
        kind: kind_name(task.kind).to_owned(),
        width_bits: task.width.bits(),
        regime: regime_name(task.regime).to_owned(),
        epsilon: task.epsilon,
        horizon: task.horizon,
        target_port: task.target_port,
        num_channels,
        seeds_tested: seed_results.len(),
        pl_settled_runs,
        wake_only,
        sa,
        sb,
        h6_contrast,
    }
}

fn build_k2_opportunity_summary(cells: &[TaskCellSummary]) -> K2OpportunitySummary {
    let sp_cells: Vec<&TaskCellSummary> = cells.iter().filter(|c| c.family == "SP").collect();
    let mut rows = Vec::with_capacity(sp_cells.len());
    let mut pos_onset_eval_sa = 0usize;
    let mut pos_onset_eval_sb = 0usize;
    let mut pos_wake_end_eval_sa = 0usize;
    let mut pos_wake_end_eval_sb = 0usize;
    let mut pos_onset_cum_sa = 0usize;
    let mut pos_onset_cum_sb = 0usize;
    let mut pos_l_epi_adv_sa = 0usize;
    let mut pos_l_epi_adv_sb = 0usize;
    let mut max_opp_eval = f64::NEG_INFINITY;
    let mut max_opp_cum = f64::NEG_INFINITY;

    for cell in &sp_cells {
        let sa_o_eval = cell.sa.opportunity_bound_onset_eval_bits.mean;
        let sb_o_eval = cell.sb.opportunity_bound_onset_eval_bits.mean;
        let sa_w_eval = cell.sa.opportunity_bound_wake_end_eval_bits.mean;
        let sb_w_eval = cell.sb.opportunity_bound_wake_end_eval_bits.mean;
        let sa_o_cum = cell.sa.opportunity_bound_onset_cum_bits.mean;
        let sb_o_cum = cell.sb.opportunity_bound_onset_cum_bits.mean;
        let sa_adv = cell.sa.eval_l_epi_advantage_bits.mean;
        let sb_adv = cell.sb.eval_l_epi_advantage_bits.mean;

        if sa_o_eval > 0.0 {
            pos_onset_eval_sa += 1;
        }
        if sb_o_eval > 0.0 {
            pos_onset_eval_sb += 1;
        }
        if sa_w_eval > 0.0 {
            pos_wake_end_eval_sa += 1;
        }
        if sb_w_eval > 0.0 {
            pos_wake_end_eval_sb += 1;
        }
        if sa_o_cum > 0.0 {
            pos_onset_cum_sa += 1;
        }
        if sb_o_cum > 0.0 {
            pos_onset_cum_sb += 1;
        }
        if sa_adv > 0.0 {
            pos_l_epi_adv_sa += 1;
        }
        if sb_adv > 0.0 {
            pos_l_epi_adv_sb += 1;
        }
        max_opp_eval = max_opp_eval.max(sa_o_eval).max(sb_o_eval);
        max_opp_cum = max_opp_cum.max(sa_o_cum).max(sb_o_cum);

        rows.push(SpOpportunityRow {
            task_id: cell.task_id.clone(),
            width_bits: cell.width_bits,
            regime: cell.regime.clone(),
            wake_only_oracle_correct_rate: cell.wake_only.final_port_oracle_correct_rate,
            sa_onset_rate: cell.sa.onset_rate,
            sb_onset_rate: cell.sb.onset_rate,
            sa_p_onset_incorrect: cell.sa.p_onset_oracle_incorrect_overall,
            sb_p_onset_incorrect: cell.sb.p_onset_oracle_incorrect_overall,
            sa_mean_recov_onset_bits: cell.sa.recoverable_bits_at_onset.mean,
            sb_mean_recov_onset_bits: cell.sb.recoverable_bits_at_onset.mean,
            wake_end_mean_recov_bits: cell.sa.recoverable_bits_at_wake_end.mean,
            sa_mean_fact_loss_eval_bits: cell.sa.fact_loss_eval_bits.mean,
            sb_mean_fact_loss_eval_bits: cell.sb.fact_loss_eval_bits.mean,
            sa_mean_opp_onset_eval_bits: sa_o_eval,
            sb_mean_opp_onset_eval_bits: sb_o_eval,
            sa_mean_opp_wake_end_eval_bits: sa_w_eval,
            sb_mean_opp_wake_end_eval_bits: sb_w_eval,
            sa_mean_opp_onset_cum_bits: sa_o_cum,
            sb_mean_opp_onset_cum_bits: sb_o_cum,
            sa_mean_eval_l_epi_adv_bits: sa_adv,
            sb_mean_eval_l_epi_adv_bits: sb_adv,
        });
    }

    if !max_opp_eval.is_finite() {
        max_opp_eval = 0.0;
    }
    if !max_opp_cum.is_finite() {
        max_opp_cum = 0.0;
    }
    let clean_count = sp_cells.iter().filter(|c| c.regime == "clean").count();
    let noisy_count = sp_cells.iter().filter(|c| c.regime == "noisy").count();

    K2OpportunitySummary {
        sp_tasks_total: sp_cells.len(),
        sp_clean_tasks: clean_count,
        sp_noisy_tasks: noisy_count,
        sp_tasks_positive_mean_opp_onset_eval_sa: pos_onset_eval_sa,
        sp_tasks_positive_mean_opp_onset_eval_sb: pos_onset_eval_sb,
        sp_tasks_positive_mean_opp_wake_end_eval_sa: pos_wake_end_eval_sa,
        sp_tasks_positive_mean_opp_wake_end_eval_sb: pos_wake_end_eval_sb,
        sp_tasks_positive_mean_opp_onset_cum_sa: pos_onset_cum_sa,
        sp_tasks_positive_mean_opp_onset_cum_sb: pos_onset_cum_sb,
        sp_tasks_positive_mean_eval_l_epi_adv_sa: pos_l_epi_adv_sa,
        sp_tasks_positive_mean_eval_l_epi_adv_sb: pos_l_epi_adv_sb,
        max_sp_opportunity_bound_eval_bits: max_opp_eval,
        max_sp_opportunity_bound_cum_bits: max_opp_cum,
        k2_null_triggered_on_onset_eval_opp: pos_onset_eval_sa == 0 && pos_onset_eval_sb == 0,
        k2_null_triggered_on_wake_end_eval_opp: pos_wake_end_eval_sa == 0
            && pos_wake_end_eval_sb == 0,
        k2_null_triggered_on_cum_opp: pos_onset_cum_sa == 0 && pos_onset_cum_sb == 0,
        per_sp_task_rows: rows,
    }
}

fn summarize_sensitivity_row(
    task: &M4aTaskSpec,
    schedule: SleepSchedule,
    w_sleep: u64,
    pairs: &[(&ArmRunOutcome, &ArmRunOutcome)],
) -> SleepWindowSensitivityRow {
    let n = pairs.len().max(1) as f64;
    let is_pl = task.family == M4aTaskFamily::Pl;
    let mut onset_cnt = 0usize;
    let mut eval_adv = Vec::with_capacity(pairs.len());
    let mut cum_adv = Vec::with_capacity(pairs.len());
    let mut code_adv = Vec::with_capacity(pairs.len());
    let mut kt_reg_t = Vec::with_capacity(pairs.len());
    let mut max_ratio = 0.0f64;
    let mut tot_wrongful = 0usize;
    let mut pl_exc = 0usize;
    let mut pl_surv_sum = 0.0f64;

    for &(wake, sleep) in pairs {
        if sleep.first_onset_wake_tick.is_some() {
            onset_cnt += 1;
        }
        eval_adv.push(wake.frozen.eval_l_epistemic_bits - sleep.frozen.eval_l_epistemic_bits);
        cum_adv
            .push(wake.cumulative_delta_l_epistemic_bits - sleep.cumulative_delta_l_epistemic_bits);
        code_adv.push(wake.final_circuit_code_bits - sleep.final_circuit_code_bits);
        kt_reg_t.push(sleep.frozen.eval_kt_target - wake.frozen.eval_kt_target);
        let ratio = sleep_over_wake_node_eval_ratio(wake, sleep);
        max_ratio = max_ratio.max(ratio);
        tot_wrongful += sleep.wrongful_excisions;
        if is_pl {
            if sleep.pl_excised {
                pl_exc += 1;
            }
            pl_surv_sum += sleep.pl_spurious_survival_segments as f64;
        }
    }

    let kt_stats = summarize_scalars(&kt_reg_t);
    SleepWindowSensitivityRow {
        task_id: task.id.clone(),
        family: family_name(task.family).to_owned(),
        width_bits: task.width.bits(),
        regime: regime_name(task.regime).to_owned(),
        arm: match schedule {
            SleepSchedule::ConsistencyOnly => "S-A",
            SleepSchedule::ConsistencyReplay => "S-B",
            SleepSchedule::None => "wake-only",
        }
        .to_owned(),
        w_sleep,
        seeds_tested: pairs.len(),
        onset_rate: (onset_cnt as f64) / n,
        mean_eval_l_epi_adv_bits: eval_adv.iter().sum::<f64>() / n,
        mean_cum_delta_l_epi_adv_bits: cum_adv.iter().sum::<f64>() / n,
        mean_final_code_bits_adv: code_adv.iter().sum::<f64>() / n,
        mean_eval_kt_regret_target: kt_stats.mean,
        max_eval_kt_regret_target: kt_stats.max,
        max_node_eval_ratio: max_ratio,
        total_wrongful_excisions: tot_wrongful,
        pl_excision_rate: is_pl.then(|| (pl_exc as f64) / n),
        pl_mean_survival_segments: is_pl.then(|| pl_surv_sum / n),
    }
}

/// Runs the Milestone M4a-0 exploratory characterisation sweep across all 36 tasks in
/// `m4a0_task_registry()` over `[seed_offset, seed_offset + seeds)`.
///
/// # Errors
///
/// Returns an error if `[seed_offset, seed_offset + seeds)` overlaps any holdout range in
/// `HOLDOUT_BANKS` or the M4a confirmation bank `20200..=20299`.
pub fn run_explore_m4a(
    seed_offset: u64,
    seeds: usize,
    include_sensitivity: bool,
    threads: Option<usize>,
) -> Result<ExploreM4aReport, String> {
    check_explore_m4a_seed_range_holdout(seed_offset, seeds)?;

    let seeds_tested = seeds.max(1);
    let tasks = m4a0_task_registry();
    let task_meta: Vec<(FiniteStateSpec, PortLanguageOracle, u16)> = tasks
        .iter()
        .map(|t| {
            let spec = t
                .to_clean_finite_state_spec()
                .expect("registered task spec exports cleanly");
            let oracle = PortLanguageOracle::from_clean_spec(&spec);
            let ch = oracle.channels();
            (spec, oracle, ch)
        })
        .collect();

    let default_threads = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1)
        .max(1);
    let requested_threads = threads.unwrap_or(default_threads).max(1);

    let mut work_items = Vec::with_capacity(tasks.len() * seeds_tested);
    for seed_idx in 0..seeds_tested {
        for task_idx in 0..tasks.len() {
            work_items.push((task_idx, seed_idx));
        }
    }
    let threads_used = requested_threads.min(work_items.len()).max(1);

    type ExploreSlot = Mutex<Option<(PairedSeedResult, f64)>>;
    let slots: Vec<Vec<ExploreSlot>> = (0..tasks.len())
        .map(|_| (0..seeds_tested).map(|_| Mutex::new(None)).collect())
        .collect();
    let next_idx = AtomicUsize::new(0);

    let overall_start = Instant::now();
    std::thread::scope(|s| {
        for _ in 0..threads_used {
            let next_ref = &next_idx;
            let items_ref = &work_items;
            let tasks_ref = &tasks;
            let meta_ref = &task_meta;
            let slots_ref = &slots;
            s.spawn(move || {
                loop {
                    let idx = next_ref.fetch_add(1, Ordering::Relaxed);
                    if idx >= items_ref.len() {
                        break;
                    }
                    let (task_idx, seed_idx) = items_ref[idx];
                    let seed = seed_offset.saturating_add(seed_idx as u64);
                    let item_start = Instant::now();
                    let res = run_paired_task_seed(
                        &tasks_ref[task_idx],
                        &meta_ref[task_idx].0,
                        &meta_ref[task_idx].1,
                        seed,
                        include_sensitivity,
                    );
                    let elapsed_sec = item_start.elapsed().as_secs_f64();
                    *slots_ref[task_idx][seed_idx]
                        .lock()
                        .expect("explore-m4a slot lock") = Some((res, elapsed_sec));
                }
            });
        }
    });
    let wall_clock_seconds = overall_start.elapsed().as_secs_f64();

    let mut results_by_task: Vec<Vec<PairedSeedResult>> = Vec::with_capacity(tasks.len());
    let mut family_cpu_seconds: BTreeMap<String, f64> = BTreeMap::new();
    for (task_idx, row) in slots.into_iter().enumerate() {
        let fam = family_name(tasks[task_idx].family).to_owned();
        let mut task_row = Vec::with_capacity(seeds_tested);
        let mut fam_sum = 0.0f64;
        for slot in row {
            let (res, sec) = slot
                .into_inner()
                .expect("explore-m4a slot lock")
                .expect("explore-m4a seed computed");
            fam_sum += sec;
            task_row.push(res);
        }
        *family_cpu_seconds.entry(fam).or_insert(0.0) += fam_sum;
        results_by_task.push(task_row);
    }

    // Scale summed CPU-thread time to effective wall-clock share across families.
    let total_cpu_seconds: f64 = family_cpu_seconds.values().sum();
    let mut per_family_wall_clock_seconds = BTreeMap::new();
    let mut per_family_seconds_per_seed = BTreeMap::new();
    for (fam, cpu_sec) in &family_cpu_seconds {
        let wall_share = if total_cpu_seconds > 0.0 {
            wall_clock_seconds * (cpu_sec / total_cpu_seconds)
        } else {
            0.0
        };
        per_family_wall_clock_seconds.insert(fam.clone(), wall_share);
        per_family_seconds_per_seed.insert(fam.clone(), wall_share / (seeds_tested as f64));
    }

    let seconds_per_seed = wall_clock_seconds / (seeds_tested as f64);
    let runtime_projections = RuntimeExtrapolation {
        measured_seeds: seeds_tested,
        measured_wall_clock_seconds: wall_clock_seconds,
        seconds_per_seed,
        projected_100_seeds_seconds: seconds_per_seed * 100.0,
        projected_500_seeds_seconds: seconds_per_seed * 500.0,
        projected_1000_seeds_seconds: seconds_per_seed * 1000.0,
        per_family_seconds_per_seed,
    };

    let mut cells = Vec::with_capacity(tasks.len());
    let mut sensitivity_w_sleep = Vec::new();
    let mut perturbed_dream_lanes = Vec::new();
    let mut guardrail_summary = GuardrailAggregateSummary::default();
    let mut invariant_summary = InvariantAggregateSummary::default();

    for (task_idx, task) in tasks.iter().enumerate() {
        let num_channels = task_meta[task_idx].2;
        let refs: Vec<&PairedSeedResult> = results_by_task[task_idx].iter().collect();
        let cell = summarize_task_cell(task, num_channels, &refs);

        for (arm_label, arm) in [
            ("wake-only", &cell.wake_only),
            ("S-A", &cell.sa.arm),
            ("S-B", &cell.sb.arm),
        ] {
            invariant_summary.max_epistemic_mdl_err = invariant_summary
                .max_epistemic_mdl_err
                .max(arm.max_epistemic_mdl_err);
            invariant_summary.max_compute_ledger_diff = invariant_summary
                .max_compute_ledger_diff
                .max(arm.max_compute_ledger_diff);
            guardrail_summary.sprout_blocked_max_nodes +=
                arm.cliff_counters.sprout_blocked_max_nodes;
            guardrail_summary.sprout_budget_exhausted += arm.cliff_counters.sprout_budget_exhausted;
            guardrail_summary.lag_horizon_bound += arm.cliff_counters.lag_horizon_bound;
            guardrail_summary.candidate_undecidable += arm.cliff_counters.candidate_undecidable;
            guardrail_summary.sleep_comparator_suspended_ungrounded += arm
                .sleep_telemetry_totals
                .sleep_comparator_suspended_ungrounded;
            guardrail_summary.sleep_segment_skipped_no_scoreable_comparator += arm
                .sleep_telemetry_totals
                .sleep_segment_skipped_no_scoreable_comparator;
            guardrail_summary.fact_buffer_evictions +=
                arm.sleep_telemetry_totals.fact_buffer_evictions;
            guardrail_summary.sleep_match_failures +=
                arm.sleep_telemetry_totals.sleep_match_failures;
            guardrail_summary.max_suspended_comparator_ticks = guardrail_summary
                .max_suspended_comparator_ticks
                .max(arm.sleep_telemetry_totals.max_suspended_comparator_ticks);
            guardrail_summary.max_facts_forgone = guardrail_summary
                .max_facts_forgone
                .max(arm.sleep_telemetry_totals.max_facts_forgone);

            if arm.cliff_counters.sprout_blocked_max_nodes > 0 {
                guardrail_summary.binding_events.push(format!(
                    "{}/{arm_label}: sprout_blocked_max_nodes={}",
                    cell.task_id, arm.cliff_counters.sprout_blocked_max_nodes
                ));
            }
            if arm.cliff_counters.sprout_budget_exhausted > 0 {
                guardrail_summary.binding_events.push(format!(
                    "{}/{arm_label}: sprout_budget_exhausted={}",
                    cell.task_id, arm.cliff_counters.sprout_budget_exhausted
                ));
            }
        }

        for r in &results_by_task[task_idx] {
            if !r.i2_sa_pre_onset_ok {
                invariant_summary.i2_pre_onset_violations += 1;
            }
            if !r.i2_sb_pre_onset_ok {
                invariant_summary.i2_pre_onset_violations += 1;
            }
            if !r.sa.i6_ok {
                invariant_summary.i6_violations += 1;
            }
            if !r.sb.i6_ok {
                invariant_summary.i6_violations += 1;
            }
            if !r.sa.i7_ok {
                invariant_summary.i7_violations += 1;
            }
            if !r.sb.i7_ok {
                invariant_summary.i7_violations += 1;
            }
        }
        invariant_summary.g4a6_wrongful_excisions_total +=
            cell.sa.total_wrongful_excisions + cell.sb.total_wrongful_excisions;
        invariant_summary.g4a6_port_locks_lost_total +=
            cell.sa.total_oracle_correct_port_locks_lost
                + cell.sb.total_oracle_correct_port_locks_lost;

        if include_sensitivity && matches!(task.family, M4aTaskFamily::Sp | M4aTaskFamily::Pl) {
            for schedule in [
                SleepSchedule::ConsistencyOnly,
                SleepSchedule::ConsistencyReplay,
            ] {
                for &w_sleep in &SENSITIVITY_SLEEP_WINDOWS {
                    let pairs: Vec<(&ArmRunOutcome, &ArmRunOutcome)> = results_by_task[task_idx]
                        .iter()
                        .filter_map(|r| {
                            if w_sleep == 256 {
                                let sleep_ref = match schedule {
                                    SleepSchedule::ConsistencyOnly => &r.sa,
                                    SleepSchedule::ConsistencyReplay => &r.sb,
                                    SleepSchedule::None => &r.wake,
                                };
                                Some((&r.wake, sleep_ref))
                            } else {
                                r.sensitivity_runs
                                    .iter()
                                    .find(|(sc, ws, _)| *sc == schedule && *ws == w_sleep)
                                    .map(|(_, _, res)| (&r.wake, res))
                            }
                        })
                        .collect();
                    sensitivity_w_sleep
                        .push(summarize_sensitivity_row(task, schedule, w_sleep, &pairs));
                }
            }

            let mut probes_executed = 0usize;
            let mut alt_cones_probed = 0usize;
            let mut alt_single = 0usize;
            let mut alt_pert = 0usize;
            let mut alt_only = 0usize;
            let mut pri_cones_probed = 0usize;
            let mut pri_single = 0usize;
            let mut pri_pert = 0usize;
            let mut pri_only = 0usize;
            for r in &results_by_task[task_idx] {
                if let Some(dp) = &r.sa.dream_probe {
                    probes_executed += 1;
                    alt_cones_probed += dp.alt_cones_probed;
                    alt_single += dp.alt_single_live_spurious;
                    alt_pert += dp.alt_perturbed_8lanes_spurious;
                    alt_only += dp.alt_perturbed_only_reveals;
                    pri_cones_probed += dp.primary_cones_probed;
                    pri_single += dp.primary_single_live_spurious;
                    pri_pert += dp.primary_perturbed_8lanes_spurious;
                    pri_only += dp.primary_perturbed_only_reveals;
                }
            }
            perturbed_dream_lanes.push(PerturbedDreamLaneSummary {
                task_id: task.id.clone(),
                family: family_name(task.family).to_owned(),
                width_bits: task.width.bits(),
                regime: regime_name(task.regime).to_owned(),
                probes_executed,
                alt_cones_probed,
                alt_single_live_spurious: alt_single,
                alt_perturbed_8lanes_spurious: alt_pert,
                alt_perturbed_only_reveals: alt_only,
                primary_cones_probed: pri_cones_probed,
                primary_single_live_spurious: pri_single,
                primary_perturbed_8lanes_spurious: pri_pert,
                primary_perturbed_only_reveals: pri_only,
            });
        }

        cells.push(cell);
    }

    let k2_opportunity_summary = build_k2_opportunity_summary(&cells);

    Ok(ExploreM4aReport {
        provenance: BuildProvenance::current(),
        seed_offset,
        seeds_tested,
        include_sensitivity,
        threads_used,
        wall_clock_seconds,
        per_family_wall_clock_seconds,
        runtime_projections,
        cells,
        k2_opportunity_summary,
        sensitivity_w_sleep,
        perturbed_dream_lanes,
        guardrail_summary,
        invariant_summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structural::{
        build_autonomous_ring_source, run_wake_sleep_episode_audited, splice_comparator_onto_port,
    };
    use crate::substrate::{Primitive, SignalSink, Width, WireDelayMode};

    #[test]
    fn holdout_and_confirmation_guards_reject_blocked_ranges() {
        for blocked in [10_000u64, 20_200, 30_000, 40_000, 50_000, 50_100, 60_000] {
            assert!(check_explore_m4a_seed_range_holdout(blocked, 20).is_err());
            assert!(check_explore_m4a_seed_range_holdout(blocked + 19, 1).is_err());
            assert!(check_explore_m4a_seed_range_holdout(blocked - 5, 10).is_err());
            assert!(check_explore_m4a_seed_range_holdout(blocked, 0).is_err());
        }
        for interior in [20_250u64, 20_299, 50_150, 50_199, 60_020, 60_099] {
            assert!(check_explore_m4a_seed_range_holdout(interior, 1).is_err());
        }
        for safe in [
            0u64, 20, 520, 9_980, 10_020, 20_180, 20_300, 29_980, 30_020, 59_980, 60_100,
        ] {
            assert!(
                check_explore_m4a_seed_range_holdout(safe, 20).is_ok(),
                "expected safe seed range at {safe}..+20"
            );
        }
    }

    #[test]
    fn recoverable_bits_helper_isolates_oracle_incorrect_comparator_structure() {
        let motif = [0u64, 1, 0, 1, 0, 0];
        let spec = crate::env::GeneratorConfig::T1_PeriodicMotif {
            motif: motif.to_vec(),
        }
        .build(Width::BITS_1)
        .unwrap()
        .to_finite_state_spec()
        .unwrap();
        let oracle = PortLanguageOracle::from_clean_spec(&spec);

        let mut c_spurious = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);
        let (_, _, pri_src) = build_autonomous_ring_source(&mut c_spurious, 6, &motif);
        c_spurious.connect(pri_src, SignalSink::BoundaryOut(0));
        let (_, _, alt_src) = build_autonomous_ring_source(&mut c_spurious, 2, &[0, 1]);
        let sp = splice_comparator_onto_port(&mut c_spurious, 0, alt_src).unwrap();
        let mut ed_s =
            crate::structural::EditableCircuit::new(c_spurious, EconomyConfig::default()).unwrap();
        ed_s.register_comparator(sp.sub_node, sp.eqz_node, 0, 0, 0)
            .unwrap();
        let l_spurious =
            ArkheLearner::from_editable(ed_s, GrowthConfig::default(), SeedBank::new(1)).unwrap();
        let lbl_spurious = label_learner_oracle(&l_spurious, &spec, &oracle);
        let recov_spurious = compute_recoverable_comparator_bits(
            l_spurious.editable().circuit(),
            l_spurious.editable().economy().comparators(),
            &lbl_spurious,
            true,
        );
        assert!(
            recov_spurious > 0.0,
            "spurious comparator must yield positive recoverable bits, got {recov_spurious}"
        );

        let mut c_clean = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);
        let (_, _, pri2) = build_autonomous_ring_source(&mut c_clean, 6, &motif);
        c_clean.connect(pri2, SignalSink::BoundaryOut(0));
        let (_, _, alt2) = build_autonomous_ring_source(&mut c_clean, 6, &motif);
        let sp2 = splice_comparator_onto_port(&mut c_clean, 0, alt2).unwrap();
        let mut ed_c =
            crate::structural::EditableCircuit::new(c_clean, EconomyConfig::default()).unwrap();
        ed_c.register_comparator(sp2.sub_node, sp2.eqz_node, 0, 0, 0)
            .unwrap();
        let l_clean =
            ArkheLearner::from_editable(ed_c, GrowthConfig::default(), SeedBank::new(1)).unwrap();
        let lbl_clean = label_learner_oracle(&l_clean, &spec, &oracle);
        let recov_clean = compute_recoverable_comparator_bits(
            l_clean.editable().circuit(),
            l_clean.editable().economy().comparators(),
            &lbl_clean,
            true,
        );
        assert_eq!(recov_clean, 0.0);
    }

    #[test]
    fn episode_runner_matches_canonical_run_wake_sleep_episode_audited() {
        let tasks = m4a0_task_registry();
        let task = tasks
            .iter()
            .find(|t| {
                t.kind == M4aTaskKind::SpMultiViewPeriodic
                    && t.width == Width::BITS_2
                    && t.regime == M4aRegime::Clean
            })
            .unwrap();
        let mut short_task = task.clone();
        short_task.horizon = 1_024;
        let spec = short_task.to_clean_finite_state_spec().unwrap();
        let oracle = PortLanguageOracle::from_clean_spec(&spec);

        for schedule in [
            SleepSchedule::None,
            SleepSchedule::ConsistencyOnly,
            SleepSchedule::ConsistencyReplay,
        ] {
            let sb = SeedBank::new(5);
            let mut env_ref = short_task.build_environment(sb).unwrap();
            let mut learner_ref = ArkheLearner::new_with_config(
                short_task.width,
                3,
                short_task.wire_delay,
                EconomyConfig::default(),
                GrowthConfig {
                    sleep_schedule: schedule,
                    ..GrowthConfig::default()
                },
                sb,
            )
            .unwrap();
            let (rep_ref, cons_ref) = run_wake_sleep_episode_audited(
                &mut learner_ref,
                &mut env_ref,
                short_task.horizon,
                64,
                short_task.horizon * 100,
            );

            let out = run_arm_episode(
                &short_task,
                &spec,
                &oracle,
                5,
                schedule,
                None,
                &[],
                Some(short_task.horizon as usize),
                false,
                None,
            );

            assert_eq!(out.episode_ticks, rep_ref.total_substrate_ticks);
            assert_eq!(out.env_steps, rep_ref.env_steps);
            assert_eq!(out.wake_preds_prefix, rep_ref.wake_predictions);
            assert_eq!(out.wake_obs_prefix, rep_ref.wake_observations);
            assert_eq!(out.sleep_telemetry, rep_ref.sleep_telemetry);
            assert_eq!(
                out.episode_total_node_evals,
                rep_ref.compute_ledger.total_node_evals()
            );
            assert_eq!(
                out.cumulative_net_flux.to_bits(),
                learner_ref.cumulative_net_flux().to_bits()
            );
            assert!(
                out.epistemic_mdl_err <= CONSERVATION_TOL
                    && cons_ref.flux_mdl_err_bits_per_symbol <= CONSERVATION_TOL
            );
        }
    }

    #[test]
    fn explore_m4a_single_seed_determinism_and_invariants() {
        let rep = run_explore_m4a(0, 1, false, None).expect("explore_m4a seed 0");
        assert_eq!(rep.cells.len(), 36);
        assert!(rep.invariant_summary.max_epistemic_mdl_err <= CONSERVATION_TOL);
        assert_eq!(rep.invariant_summary.max_compute_ledger_diff, 0);
        assert_eq!(rep.invariant_summary.i2_pre_onset_violations, 0);
        assert_eq!(rep.invariant_summary.i6_violations, 0);
        assert_eq!(rep.invariant_summary.i7_violations, 0);

        // Verify deterministic reproducibility across repeated paired executions on SP, PL, and CT.
        let tasks = m4a0_task_registry();
        for kind in [
            M4aTaskKind::SpPeriod6LagRedundancy,
            M4aTaskKind::Pl1ShortCycle,
            M4aTaskKind::Ct1ExogenousLfsr,
        ] {
            let task = tasks
                .iter()
                .find(|t| {
                    t.kind == kind && t.width == Width::BITS_1 && t.regime == M4aRegime::Clean
                })
                .unwrap();
            let spec = task.to_clean_finite_state_spec().unwrap();
            let oracle = PortLanguageOracle::from_clean_spec(&spec);
            let r1 = run_paired_task_seed(task, &spec, &oracle, 3, false);
            let r2 = run_paired_task_seed(task, &spec, &oracle, 3, false);
            assert_eq!(
                r1.sa.cumulative_net_flux.to_bits(),
                r2.sa.cumulative_net_flux.to_bits()
            );
            assert_eq!(
                r1.sb.cumulative_net_flux.to_bits(),
                r2.sb.cumulative_net_flux.to_bits()
            );
            assert_eq!(r1.sa.final_node_count, r2.sa.final_node_count);
            assert_eq!(r1.sb.final_node_count, r2.sb.final_node_count);
        }
        let _ = Primitive::Eqz;
    }

    #[test]
    fn explore_trajectory_sharing_is_bit_identical_to_cold_execution() {
        let tasks = m4a0_task_registry();
        // Cover zero-onset reuse (CT1 / MV_Lfsr noisy S-A), late-onset spontaneous (SP k=2),
        // and planted falsification (PL1).
        for &(kind, regime) in &[
            (M4aTaskKind::Ct1ExogenousLfsr, M4aRegime::Clean),
            (M4aTaskKind::SpPeriod6LagRedundancy, M4aRegime::Noisy),
            (M4aTaskKind::Pl1ShortCycle, M4aRegime::Clean),
        ] {
            let task = tasks
                .iter()
                .find(|t| t.kind == kind && t.width == Width::BITS_2 && t.regime == regime)
                .or_else(|| {
                    tasks
                        .iter()
                        .find(|t| t.kind == kind && t.width == Width::BITS_1 && t.regime == regime)
                })
                .expect("representative task exists");
            let spec = task.to_clean_finite_state_spec().unwrap();
            let oracle = PortLanguageOracle::from_clean_spec(&spec);

            let paired = run_paired_task_seed(task, &spec, &oracle, 2, false);
            let cold_wake = run_arm_episode(
                task,
                &spec,
                &oracle,
                2,
                SleepSchedule::None,
                None,
                &[paired.sa.env_steps, paired.sb.env_steps],
                None,
                false,
                None,
            );
            let cold_sa = run_arm_episode(
                task,
                &spec,
                &oracle,
                2,
                SleepSchedule::ConsistencyOnly,
                None,
                &[],
                None,
                false,
                Some(cold_wake.episode_total_node_evals),
            );
            let cold_sb = run_arm_episode(
                task,
                &spec,
                &oracle,
                2,
                SleepSchedule::ConsistencyReplay,
                None,
                &[],
                None,
                false,
                Some(cold_wake.episode_total_node_evals),
            );

            for (label, shared, cold) in [
                ("wake", &paired.wake, &cold_wake),
                ("sa", &paired.sa, &cold_sa),
                ("sb", &paired.sb, &cold_sb),
            ] {
                assert_eq!(shared.episode_ticks, cold.episode_ticks, "{kind:?} {label}");
                assert_eq!(shared.env_steps, cold.env_steps, "{kind:?} {label}");
                assert_eq!(
                    shared.episode_total_node_evals, cold.episode_total_node_evals,
                    "{kind:?} {label}"
                );
                assert_eq!(
                    shared.episode_live_step_node_evals, cold.episode_live_step_node_evals,
                    "{kind:?} {label}"
                );
                assert_eq!(
                    shared.episode_candidate_replay_node_evals,
                    cold.episode_candidate_replay_node_evals,
                    "{kind:?} {label}"
                );
                assert_eq!(
                    shared.episode_sensitivity_node_evals, cold.episode_sensitivity_node_evals,
                    "{kind:?} {label}"
                );
                assert_eq!(
                    shared.cumulative_net_flux.to_bits(),
                    cold.cumulative_net_flux.to_bits(),
                    "{kind:?} {label}"
                );
                assert_eq!(
                    shared.final_circuit_code_bits.to_bits(),
                    cold.final_circuit_code_bits.to_bits(),
                    "{kind:?} {label}"
                );
                assert_eq!(
                    shared.frozen.eval_l_epistemic_bits.to_bits(),
                    cold.frozen.eval_l_epistemic_bits.to_bits(),
                    "{kind:?} {label}"
                );
                assert_eq!(
                    shared.sleep_telemetry, cold.sleep_telemetry,
                    "{kind:?} {label}"
                );
                assert_eq!(
                    shared.wrongful_excisions, cold.wrongful_excisions,
                    "{kind:?} {label}"
                );
                assert_eq!(shared.pl_excised, cold.pl_excised, "{kind:?} {label}");
                assert_eq!(
                    shared.first_onset_wake_tick, cold.first_onset_wake_tick,
                    "{kind:?} {label}"
                );
                assert_eq!(
                    shared.checkpoints_by_wake_step.keys().collect::<Vec<_>>(),
                    cold.checkpoints_by_wake_step.keys().collect::<Vec<_>>(),
                    "{kind:?} {label}"
                );
                for (step, cp_shared) in &shared.checkpoints_by_wake_step {
                    let cp_cold = &cold.checkpoints_by_wake_step[step];
                    assert_eq!(
                        cp_shared.cumulative_net_flux.to_bits(),
                        cp_cold.cumulative_net_flux.to_bits(),
                        "{kind:?} {label} cp {step}"
                    );
                    assert_eq!(
                        cp_shared.frozen.eval_l_epistemic_bits.to_bits(),
                        cp_cold.frozen.eval_l_epistemic_bits.to_bits(),
                        "{kind:?} {label} cp {step}"
                    );
                }
            }
        }
    }
}
