//! Milestone 2b verification suite: epistemic MDL conservation, comparator formation,
//! offline loopback falsification, stationary noise robustness, and fault survival (`G2B.1`–`G2B.5`).

use std::collections::VecDeque;

use rand_chacha::ChaCha8Rng;
use rand_core::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};

use crate::baselines::{PredictorBaseline, TickBudget};
use crate::config::{BuildProvenance, CliffCountersSummary};
use crate::env::{
    CrossLinearOp, CrossNonlinearOp, FiniteStateGenerator, GeneratorConfig, LfsrStage,
    MultiViewDriver, MultiViewTarget, MultiViewTransform, NoiseModel, NoisyEnvironment,
    channel_substitution_entropy_bits, compound_reactive_entropy_bits,
    compound_reactive_error_rate,
};
use crate::metrics::facts_code_bits;
use crate::metrics::mdl::{
    EpistemicComparatorTrace, EpistemicTickPhase, epistemic_null_calibrated_mdl_delta_with_tax,
};
use crate::rng::SeedBank;
use crate::stats::{
    BankSample, CaseOutcome, MultiBankGateResult, classify_case_outcome, evaluate_multi_bank_gate,
};
use crate::structural::{
    ArkheLearner, EconomyConfig, EconomyState, EditableCircuit, ExecutionPhase, GrowthConfig,
    LiveCircuit, ReserveCap, build_autonomous_ring_source, derivation_cone_info,
    node_output_values, splice_comparator_onto_port, tap_source, well_mixed_description_debts,
};
use crate::substrate::{
    Circuit, NodeId, Primitive, SignalSink, SignalSource, Width, WireDelayMode,
};

/// Default prequential horizon $T$ for Milestone 2b verification (`docs/research_record.md` §11).
pub const DEFAULT_M2B_HORIZON: u64 = 2_000;
/// Default transient warmup window $W$ for Milestone 2b verification.
pub const DEFAULT_M2B_WARMUP: u64 = 500;
/// Default search TickBudget per verification run (`docs/research_record.md` §11).
pub const DEFAULT_M2B_BUDGET: u64 = 200_000;
/// Pre-registered offline loopback horizon $T_{\text{loop}}$ in ticks (`G2B.3`, §6).
pub const DEFAULT_M2B_LOOPBACK_TICKS: u64 = 128;

/// Pre-registered M1 holdout range start seed (unconditionally rejected in M2b).
pub const M1_HOLDOUT_SEED_START: u32 = 10_000;
/// Pre-registered M1 holdout range end seed (inclusive).
pub const M1_HOLDOUT_SEED_END: u32 = 10_019;
/// Pre-registered M2a holdout range start seed (unconditionally rejected in M2b).
pub const M2A_HOLDOUT_SEED_START: u32 = 30_000;
/// Pre-registered M2a holdout range end seed (inclusive).
pub const M2A_HOLDOUT_SEED_END: u32 = 30_019;
/// Pre-registered M2b holdout range start seed.
pub const M2B_HOLDOUT_SEED_START: u32 = 40_000;
/// Pre-registered M2b holdout range end seed (inclusive).
pub const M2B_HOLDOUT_SEED_END: u32 = 40_019;
/// Pre-registered M3 holdout range start seed (unconditionally rejected in M2b).
pub const M3_HOLDOUT_SEED_START: u32 = 50_000;
/// Pre-registered M3 holdout range end seed (inclusive).
pub const M3_HOLDOUT_SEED_END: u32 = 50_019;
/// Pre-registered M4a holdout range start seed (unconditionally rejected in M2b).
pub const M4A_HOLDOUT_SEED_START: u32 = 60_000;
/// Pre-registered M4a holdout range end seed (inclusive).
pub const M4A_HOLDOUT_SEED_END: u32 = 60_019;

/// Pre-registered target pass rate for `G2B.1`.
pub const G2B1_TARGET_RATE: f64 = 0.90;
/// Pre-registered target pass rate for `G2B.2`.
pub const G2B2_TARGET_RATE: f64 = 0.90;
/// Pre-registered target pass rate for `G2B.3`.
pub const G2B3_TARGET_RATE: f64 = 0.95;
/// Pre-registered target pass rate for `G2B.4`.
pub const G2B4_TARGET_RATE: f64 = 0.85;
/// Pre-registered target pass rate for `G2B.5`.
pub const G2B5_TARGET_RATE: f64 = 0.90;

/// Maximum allowed epistemic MDL conservation discrepancy in bits/symbol (`G2B.1a`).
pub const G2B1_EPISTEMIC_MDL_TOL_BITS_PER_SYMBOL: f64 = 1e-11;
/// Maximum allowed post-warmup target excess code length in bits/symbol (`G2B.1b`).
pub const G2B1_EXCESS_BITS_MAX: f64 = 0.005;
/// Stationary noise rate tested in `G2B.4`.
pub const G2B4_NOISE_EPSILON: f64 = 0.05;
/// Maximum allowed |KT - Oracle| or pooled |E[KT] - H_bayes_class| deviation in `G2B.4`.
pub const G2B4_KT_DIFF_MAX: f64 = 0.040;
/// Maximum allowed disagreement-rate deviation from theoretical flip rate in `G2B.4`.
pub const G2B4_DISAGREEMENT_TOL: f64 = 0.040;
/// Pre-registered channel-fault onset tick for `G2B.5`.
pub const G2B5_FAULT_TICK: u64 = 1_000;
/// Pre-registered clean target error recovery horizon after `G2B5_FAULT_TICK` (`G2B.5`).
pub const G2B5_RECOVERY_WINDOW_TICKS: u64 = 64;
/// Minimum required pooled post-fault `[1000, T)` KT advantage in bits/symbol (`G2B.5`).
pub const G2B5_POST_FAULT_KT_ADVANTAGE_MIN: f64 = 0.15;

/// Guards M2b runs against pre-registered holdout seed ranges.
///
/// Unconditionally rejects overlaps with M1 (`10000..=10019`), M2a (`30000..=30019`), M3
/// (`50000..=50019`), and M4a (`60000..=60019`) holdout ranges, and rejects M2b (`40000..=40019`)
/// unless `allow_holdout` is true.
///
/// # Errors
///
/// Returns an error description if `[seed_offset, seed_offset + seeds)` overlaps a forbidden holdout range.
pub fn check_m2b_seed_range_holdout(
    seed_offset: u32,
    seeds: u32,
    allow_holdout: bool,
) -> Result<(), String> {
    let start = seed_offset;
    let end = seed_offset.saturating_add(seeds.max(1));
    let unconditional = [
        ("M1", M1_HOLDOUT_SEED_START, M1_HOLDOUT_SEED_END),
        ("M2a", M2A_HOLDOUT_SEED_START, M2A_HOLDOUT_SEED_END),
        ("M3", M3_HOLDOUT_SEED_START, M3_HOLDOUT_SEED_END),
        ("M4a", M4A_HOLDOUT_SEED_START, M4A_HOLDOUT_SEED_END),
    ];
    for (label, h_start, h_end) in unconditional {
        if start <= h_end && end > h_start {
            return Err(format!(
                "refusing seed range {start}..{end} which overlaps pre-registered {label} holdout range {h_start}..={h_end} (unconditionally forbidden under verify-m2b)"
            ));
        }
    }
    if start <= M2B_HOLDOUT_SEED_END && end > M2B_HOLDOUT_SEED_START && !allow_holdout {
        return Err(format!(
            "refusing seed range {start}..{end} which overlaps pre-registered holdout range {M2B_HOLDOUT_SEED_START}..={M2B_HOLDOUT_SEED_END} without --holdout flag"
        ));
    }
    Ok(())
}

/// Summary report for one M2b gate in single-bank verification mode.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M2bGateReport {
    /// Gate identifier (`G2B.1`..`G2B.5`).
    pub gate: String,
    /// Overall pass/fail decision for this gate.
    pub passed: bool,
    /// Pre-registered target success rate.
    pub target_rate: f64,
    /// Count of functionally passing non-excluded cases.
    pub passed_cases: usize,
    /// Count of functionally failing non-excluded cases.
    pub failed_cases: usize,
    /// Count of excluded cases (`sprout_blocked_max_nodes > 0`).
    pub excluded_cases: usize,
    /// Total evaluated cases.
    pub total_cases: usize,
    /// Empirical pass rate (`passed_cases / total_cases`).
    pub empirical_rate: f64,
    /// Accumulated cliff and guardrail telemetry counters.
    pub cliff_counters: CliffCountersSummary,
    /// Optional auxiliary metric summary for this gate.
    pub aux_metric_summary: Option<String>,
    /// Failure diagnostics recorded during evaluation.
    pub failures: Vec<String>,
}

/// Complete verification summary emitted by `arkhe verify-m2b` in single-bank mode.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M2bVerificationSummary {
    /// Compile-time binary and source provenance.
    pub provenance: BuildProvenance,
    /// Starting master seed offset.
    pub seed_offset: u32,
    /// Number of master seeds tested.
    pub seeds_tested: u32,
    /// Simulation horizon $T$ in ticks.
    pub horizon: u64,
    /// Per-gate summaries for `G2B.1`..`G2B.5`.
    pub gates: Vec<M2bGateReport>,
    /// True iff all gates pass and `excluded_cases == 0`.
    pub all_passed: bool,
    /// Total excluded cases across all gates.
    pub excluded_cases: usize,
    /// Total cliff counters across all gates.
    pub cliff_counters: CliffCountersSummary,
    /// Maximum epistemic MDL conservation residual (bits/symbol) observed in `G2B.1a`.
    pub g2b1_max_epistemic_mdl_err: f64,
    /// Pooled `|E[KT] - H_bayes_class|` across `G2B.4` positive-structure runs (bits/symbol).
    pub g2b4_pooled_abs_kt_minus_h_bayes_class: f64,
    /// Pooled post-fault `[1000, T)` KT advantage (bits/symbol) in `G2B.5`.
    pub g2b5_pooled_post_fault_kt_advantage: f64,
}

/// Multi-bank gate report for one M2b gate (`G2B.1`..`G2B.5`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M2bBankGateReport {
    /// Gate identifier.
    pub gate: String,
    /// Overall multi-bank pass decision.
    pub passed: bool,
    /// Total excluded cases across all banks.
    pub excluded_cases: usize,
    /// Clopper-Pearson 99% lower bound + chi-squared homogeneity result.
    pub multi_bank: MultiBankGateResult,
    /// Optional pooled continuous metric name (`G2B.4` / `G2B.5`).
    pub pooled_metric_name: Option<String>,
    /// Optional pooled continuous metric value.
    pub pooled_metric_value: Option<f64>,
    /// Whether the pooled continuous check passed.
    pub pooled_metric_passed: bool,
    /// Sample failure strings recorded across banks.
    pub failures: Vec<String>,
}

/// Multi-bank Milestone 2b report emitted by `arkhe verify-m2b --banks N`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M2bBankReport {
    /// Compile-time provenance.
    pub provenance: BuildProvenance,
    /// Starting master seed offset.
    pub seed_offset: u32,
    /// Master seeds per bank.
    pub seeds_per_bank: u32,
    /// Number of consecutive banks evaluated.
    pub banks: u32,
    /// Simulation horizon $T$ in ticks.
    pub horizon: u64,
    /// Multi-bank gate outcomes for `G2B.1`..`G2B.5`.
    pub gates: Vec<M2bBankGateReport>,
    /// True iff all gates pass and `excluded_cases == 0`.
    pub all_passed: bool,
    /// Total excluded cases across all banks and gates.
    pub excluded_cases: usize,
    /// Accumulated cliff counters across all banks.
    pub cliff_counters: CliffCountersSummary,
    /// Maximum epistemic MDL conservation residual (bits/symbol) observed across all banks.
    pub g2b1_max_epistemic_mdl_err: f64,
    /// Pooled `|E[KT] - H_bayes_class|` across all banks in `G2B.4`.
    pub g2b4_pooled_abs_kt_minus_h_bayes_class: f64,
    /// Pooled post-fault `[1000, T)` KT advantage (bits/symbol) across all banks in `G2B.5`.
    pub g2b5_pooled_post_fault_kt_advantage: f64,
}

#[derive(Clone, Debug, Default)]
struct GateTally {
    passed_cases: usize,
    failed_cases: usize,
    excluded_cases: usize,
    cliff_counters: CliffCountersSummary,
    failures: Vec<String>,
}

impl GateTally {
    fn total_cases(&self) -> usize {
        self.passed_cases + self.failed_cases + self.excluded_cases
    }

    fn record(
        &mut self,
        label: String,
        functional_pass: bool,
        cliff: &CliffCountersSummary,
        detail: Option<String>,
    ) {
        self.cliff_counters.accumulate(cliff);
        match classify_case_outcome(functional_pass, cliff) {
            CaseOutcome::Passed => self.passed_cases += 1,
            CaseOutcome::Failed => {
                self.failed_cases += 1;
                if let Some(msg) = detail {
                    self.failures.push(format!("{label}: {msg}"));
                } else {
                    self.failures.push(format!("{label}: failed"));
                }
            }
            CaseOutcome::Excluded => {
                self.excluded_cases += 1;
                self.failures.push(format!(
                    "{label}: excluded (sprout_blocked_max_nodes={})",
                    cliff.sprout_blocked_max_nodes
                ));
            }
        }
    }

    fn merge(&mut self, other: Self) {
        self.passed_cases += other.passed_cases;
        self.failed_cases += other.failed_cases;
        self.excluded_cases += other.excluded_cases;
        self.cliff_counters.accumulate(&other.cliff_counters);
        self.failures.extend(other.failures);
    }

    fn bank_sample(&self) -> BankSample {
        BankSample::new(self.passed_cases, self.total_cases())
    }

    fn into_report(
        mut self,
        gate: &str,
        target_rate: f64,
        extra_ok: bool,
        aux_metric_summary: Option<String>,
    ) -> M2bGateReport {
        self.failures.sort();
        if self.failures.len() > 64 {
            let total = self.failures.len();
            self.failures.truncate(64);
            self.failures.push(format!(
                "... ({} additional failures truncated)",
                total - 64
            ));
        }
        let total = self.total_cases();
        let empirical_rate = if total == 0 {
            0.0
        } else {
            (self.passed_cases as f64) / (total as f64)
        };
        let passed =
            total > 0 && self.excluded_cases == 0 && empirical_rate >= target_rate && extra_ok;
        M2bGateReport {
            gate: gate.to_owned(),
            passed,
            target_rate,
            passed_cases: self.passed_cases,
            failed_cases: self.failed_cases,
            excluded_cases: self.excluded_cases,
            total_cases: total,
            empirical_rate,
            cliff_counters: self.cliff_counters,
            aux_metric_summary,
            failures: self.failures,
        }
    }
}

fn cliff_from_telemetry(tel: &crate::config::ArkheM1Telemetry) -> CliffCountersSummary {
    CliffCountersSummary {
        reserve_clipped_count: tel.reserve_clipped_count,
        reserve_clipped_bits: tel.reserve_clipped_bits,
        reserve_mismatch_floored: tel.reserve_mismatch_floored,
        sprout_blocked_max_nodes: tel.sprout_blocked_max_nodes,
        candidate_undecidable: tel.candidate_undecidable,
        candidate_list_truncated: tel.candidate_list_truncated,
        lag_horizon_bound: tel.lag_horizon_bound,
        ring_period_bound: tel.ring_period_bound,
        lag_deepening_steps: tel.lag_deepening_steps,
        sprout_budget_exhausted: tel.sprout_budget_exhausted,
    }
}

fn lfsr_deg4() -> LfsrStage {
    LfsrStage {
        taps: vec![1, 0, 0, 1],
        init: vec![1, 0, 1, 1],
    }
}

fn lfsr_deg5() -> LfsrStage {
    LfsrStage {
        taps: vec![0, 1, 0, 0, 1],
        init: vec![1, 1, 0, 1, 0],
    }
}

fn lfsr_deg11() -> LfsrStage {
    let mut taps = vec![0u64; 11];
    taps[8] = 1;
    taps[10] = 1;
    let init = vec![1, 0, 1, 1, 0, 1, 0, 1, 1, 0, 1];
    LfsrStage { taps, init }
}

fn lfsr_deg9() -> LfsrStage {
    let mut taps = vec![0u64; 9];
    taps[3] = 1;
    taps[8] = 1;
    let init = vec![1, 1, 0, 1, 0, 1, 1, 0, 1];
    LfsrStage { taps, init }
}

struct OnlineTraceResult {
    learner: ArkheLearner,
    telemetry: crate::config::ArkheM1Telemetry,
    cliff: CliffCountersSummary,
    post_warmup_clean_mismatches: Vec<u64>,
    post_warmup_kt: Vec<f64>,
    post_fault_obs_mismatches: Vec<u64>,
    post_recovery_clean_mismatches: Vec<u64>,
    settled_comparator_ticks: u64,
    settled_comparator_disagreements: u64,
    settled_max_circular_distance: u64,
    active_comparator_ticks_by_comp: Vec<(u64, u64)>,
}

#[allow(clippy::too_many_arguments)]
fn run_online_case(
    width: Width,
    wire_delay: WireDelayMode,
    editable_init: Option<EditableCircuit>,
    gen_cfg: &GeneratorConfig,
    noise: &NoiseModel,
    growth: GrowthConfig,
    seed: u64,
    horizon: u64,
    warmup: u64,
    fault_tick: u64,
    budget_limit: u64,
) -> OnlineTraceResult {
    let sb = SeedBank::new(seed);
    let generator = gen_cfg.build(width).expect("valid M2b generator config");
    let channels = usize::from(generator.channels());
    let mut env = NoisyEnvironment::new(generator, noise.clone(), sb).expect("valid noisy env");
    let mut learner = match editable_init {
        Some(editable) => ArkheLearner::from_editable(editable, growth, sb).expect("valid learner"),
        None => ArkheLearner::new(width, channels as u16, wire_delay, growth, sb)
            .expect("valid learner"),
    };

    let mut budget = TickBudget::new(budget_limit);
    let mut clean = vec![0u64; channels];
    let mut obs = vec![0u64; channels];

    let mut post_warmup_clean_mismatches = vec![0u64; channels];
    let mut post_warmup_obs_mismatches = vec![0u64; channels];
    let mut post_fault_obs_mismatches = vec![0u64; channels];
    let mut post_recovery_clean_mismatches = vec![0u64; channels];
    let recovery_deadline = fault_tick.saturating_add(G2B5_RECOVERY_WINDOW_TICKS);

    let mut last_structure_sig = (0usize, 0u64, 0u64, 0u64, 0u64, 0u64, 0u64, 0usize);
    let mut last_structure_tick = 0u64;
    let mut settled_comparator_ticks = 0u64;
    let mut settled_comparator_disagreements = 0u64;
    let mut settled_max_circular_distance = 0u64;
    let mut comp_tick_map: std::collections::BTreeMap<(u16, u64, u64), (u64, u64)> =
        std::collections::BTreeMap::new();

    for t in 0..horizon {
        env.step_frame(&mut clean, &mut obs);
        let pred = learner.predict(&mut budget);

        // Inspect live comparator predictions immediately after `predict`.
        let tel_now = learner.telemetry();
        let comps_len = learner.editable().economy().comparators().len();
        let sig_now = (
            learner.editable().circuit().node_count(),
            tel_now.sprout_events,
            tel_now.redundancy_sprout_events,
            tel_now.ring_closure_events,
            tel_now.starvation_prune_events,
            tel_now.disagreement_prune_events,
            tel_now.driver_handoff_events,
            comps_len,
        );
        if sig_now != last_structure_sig {
            last_structure_sig = sig_now;
            last_structure_tick = t;
        }

        let comps = learner.editable().economy().comparators();
        if !comps.is_empty() {
            let circuit = learner.editable().circuit();
            let compiled = learner.editable().compiled();
            let state = learner.editable().state();
            let wires = state.wire_values();
            let node_outs = node_output_values(circuit, state);
            let econ_nodes = learner.editable().economy().nodes();

            let is_settled = t >= warmup && t >= last_structure_tick.saturating_add(32);
            for comp in comps {
                let sub_in = &compiled.node_input_wires()[comp.sub_node.index()];
                let pri_v = width.normalize(wires[sub_in[0]]);
                let alt_v = width.normalize(wires[sub_in[1]]);
                let eqz_v = width.normalize(node_outs[comp.eqz_node.index()]);
                let circ_d = width.circular_distance(pri_v, alt_v);

                let mature = econ_nodes
                    .get(comp.eqz_node.index())
                    .is_some_and(|n| n.juvenile_ticks == 0);
                if mature {
                    let key = (
                        comp.target_port,
                        comp.primary_channel_mask,
                        comp.alt_channel_mask,
                    );
                    let entry = comp_tick_map.entry(key).or_insert((0, 0));
                    entry.0 += 1;
                    if pri_v != alt_v {
                        entry.1 += 1;
                    }
                }

                if is_settled {
                    settled_comparator_ticks += 1;
                    if pri_v != alt_v || eqz_v != 1 {
                        settled_comparator_disagreements += 1;
                    }
                    if circ_d > settled_max_circular_distance {
                        settled_max_circular_distance = circ_d;
                    }
                }
            }
        }

        for ch in 0..channels {
            let p = width.normalize(pred[ch]);
            if t >= warmup {
                if p != clean[ch] {
                    post_warmup_clean_mismatches[ch] += 1;
                }
                if p != obs[ch] {
                    post_warmup_obs_mismatches[ch] += 1;
                }
            }
            if t >= fault_tick {
                if p != obs[ch] {
                    post_fault_obs_mismatches[ch] += 1;
                }
                if t >= recovery_deadline && p != clean[ch] {
                    post_recovery_clean_mismatches[ch] += 1;
                }
            }
        }

        learner.observe(&obs, &mut budget);
    }

    let eval_ticks = horizon.saturating_sub(warmup).max(1);
    let post_warmup_kt = post_warmup_obs_mismatches
        .iter()
        .map(|&mis| facts_code_bits(width, eval_ticks, mis) / (eval_ticks as f64))
        .collect();
    let telemetry = learner.telemetry();
    let cliff = cliff_from_telemetry(&telemetry);

    let active_comparator_ticks_by_comp = learner
        .editable()
        .economy()
        .comparators()
        .iter()
        .map(|comp| {
            comp_tick_map
                .get(&(
                    comp.target_port,
                    comp.primary_channel_mask,
                    comp.alt_channel_mask,
                ))
                .copied()
                .unwrap_or((0, 0))
        })
        .collect();

    OnlineTraceResult {
        learner,
        telemetry,
        cliff,
        post_warmup_clean_mismatches,
        post_warmup_kt,
        post_fault_obs_mismatches,
        post_recovery_clean_mismatches,
        settled_comparator_ticks,
        settled_comparator_disagreements,
        settled_max_circular_distance,
        active_comparator_ticks_by_comp,
    }
}

/// Causal Bayes-optimal reactive oracle for `G2B.4` paired KT reference on `T2_MultiViewCoupled`
/// and `T1_Alternating`.
struct M2bCausalReactiveOracle {
    width: Width,
    channels: usize,
    mode: OracleMode,
    history: Vec<VecDeque<u64>>,
    tick: u64,
}

enum OracleMode {
    Alternating {
        a: u64,
        b: u64,
    },
    MultiView {
        view_0: MultiViewTransform,
        target: MultiViewTarget,
    },
}

impl M2bCausalReactiveOracle {
    fn from_config(width: Width, channels: u16, cfg: &GeneratorConfig) -> Self {
        let ch_count = usize::from(channels);
        let mode = match cfg {
            GeneratorConfig::T1_Alternating { a, b } => OracleMode::Alternating {
                a: width.normalize(*a),
                b: width.normalize(*b),
            },
            GeneratorConfig::T2_MultiViewCoupled { view_0, target, .. } => OracleMode::MultiView {
                view_0: view_0.normalized(width),
                target: *target,
            },
            other => panic!("unsupported config for M2bCausalReactiveOracle: {other:?}"),
        };
        Self {
            width,
            channels: ch_count,
            mode,
            history: vec![VecDeque::with_capacity(64); ch_count],
            tick: 0,
        }
    }

    fn recover_z_at_lag(&self, v0: MultiViewTransform, z_lag: usize) -> u64 {
        let t = self.tick as usize;
        if t < z_lag {
            return 0;
        }
        let obs_lag = z_lag - v0.delay;
        let len = self.history[0].len();
        if obs_lag == 0 || obs_lag > len {
            return 0;
        }
        let obs_sym = self.history[0][len - obs_lag];
        v0.invert_z(self.width, obs_sym)
    }
}

impl PredictorBaseline for M2bCausalReactiveOracle {
    fn name(&self) -> &str {
        "M2bCausalReactiveOracle"
    }

    fn predict(&mut self, _budget: &mut TickBudget) -> Vec<u64> {
        let mut out = vec![0u64; self.channels];
        match &self.mode {
            OracleMode::Alternating { a, b } => {
                if self.history[0].len() >= 2 {
                    out[0] = self.history[0][self.history[0].len() - 2];
                } else if self.tick.is_multiple_of(2) {
                    out[0] = *a;
                } else {
                    out[0] = *b;
                }
            }
            OracleMode::MultiView { view_0, target } => {
                if !self.history[0].is_empty() {
                    out[0] = *self.history[0].back().expect("non-empty history");
                }
                if self.channels > 1 && !self.history[1].is_empty() {
                    out[1] = *self.history[1].back().expect("non-empty history");
                }
                let pred2 = match *target {
                    MultiViewTarget::Affine { lag, mul_a, add_b } => {
                        let z = self.recover_z_at_lag(*view_0, lag);
                        self.width.add(self.width.mul(mul_a, z), add_b)
                    }
                    MultiViewTarget::Eqz { lag, add_b } => {
                        let z = self.recover_z_at_lag(*view_0, lag);
                        let shifted = self.width.add(z, add_b);
                        u64::from(shifted == 0)
                    }
                    MultiViewTarget::AddEqz { lag_a, lag_b } => {
                        let za = self.recover_z_at_lag(*view_0, lag_a);
                        let zb = self.recover_z_at_lag(*view_0, lag_b);
                        self.width.add(za, u64::from(zb == 0))
                    }
                    MultiViewTarget::MulEqz { lag_a, lag_b } => {
                        let za = self.recover_z_at_lag(*view_0, lag_a);
                        let zb = self.recover_z_at_lag(*view_0, lag_b);
                        self.width.mul(za, u64::from(zb == 0))
                    }
                    MultiViewTarget::MulEqzPair { lag_a, lag_b } => {
                        let za = self.recover_z_at_lag(*view_0, lag_a);
                        let zb = self.recover_z_at_lag(*view_0, lag_b);
                        u64::from(za == 0 && zb == 0)
                    }
                };
                out[2] = self.width.normalize(pred2);
            }
        }
        out
    }

    fn observe(&mut self, truth: &[u64], _budget: &mut TickBudget) -> Option<f64> {
        for (ch, slot) in self.history.iter_mut().enumerate() {
            slot.push_back(self.width.normalize(truth[ch]));
            if slot.len() > 64 {
                slot.pop_front();
            }
        }
        self.tick += 1;
        None
    }

    fn best_circuit(&self) -> Option<&Circuit> {
        None
    }
}

fn run_causal_oracle_target_kt(
    width: Width,
    gen_cfg: &GeneratorConfig,
    noise: &NoiseModel,
    seed: u64,
    horizon: u64,
    warmup: u64,
) -> f64 {
    let sb = SeedBank::new(seed);
    let generator = gen_cfg.build(width).expect("valid oracle gen cfg");
    let channels = usize::from(generator.channels());
    let target_ch = channels - 1;
    let mut env = NoisyEnvironment::new(generator, noise.clone(), sb).expect("valid env");
    let mut oracle = M2bCausalReactiveOracle::from_config(width, channels as u16, gen_cfg);
    let mut budget = TickBudget::new(horizon * 10);
    let mut clean = vec![0u64; channels];
    let mut obs = vec![0u64; channels];
    let mut obs_mismatches = 0u64;

    for t in 0..horizon {
        env.step_frame(&mut clean, &mut obs);
        let pred = oracle.predict(&mut budget);
        if t >= warmup && width.normalize(pred[target_ch]) != obs[target_ch] {
            obs_mismatches += 1;
        }
        oracle.observe(&obs, &mut budget);
    }
    let eval_ticks = horizon.saturating_sub(warmup).max(1);
    facts_code_bits(width, eval_ticks, obs_mismatches) / (eval_ticks as f64)
}

fn run_clean_phase_oracle_target_kt(
    width: Width,
    gen_cfg: &GeneratorConfig,
    noise: &NoiseModel,
    seed: u64,
    horizon: u64,
    warmup: u64,
) -> f64 {
    let sb = SeedBank::new(seed);
    let generator = gen_cfg.build(width).expect("valid oracle gen cfg");
    let channels = usize::from(generator.channels());
    let target_ch = channels - 1;
    let mut env = NoisyEnvironment::new(generator, noise.clone(), sb).expect("valid env");
    let mut clean = vec![0u64; channels];
    let mut obs = vec![0u64; channels];
    let mut obs_mismatches = 0u64;

    for t in 0..horizon {
        env.step_frame(&mut clean, &mut obs);
        if t >= warmup && width.normalize(clean[target_ch]) != obs[target_ch] {
            obs_mismatches += 1;
        }
    }
    let eval_ticks = horizon.saturating_sub(warmup).max(1);
    facts_code_bits(width, eval_ticks, obs_mismatches) / (eval_ticks as f64)
}

fn cone_substitution_error_rate(eps: f64, num_noisy_inputs: usize, k_bits: u8) -> f64 {
    if num_noisy_inputs == 0 {
        0.0
    } else {
        compound_reactive_error_rate(eps, num_noisy_inputs - 1, k_bits)
    }
}

// ---------------------------------------------------------------------------
// G2B.1: Epistemic MDL conservation, starvation, T2 panel, & M1 compatibility
// ---------------------------------------------------------------------------

type ComparatorSpecList = Vec<(NodeId, NodeId, u16, u64, u64)>;

fn build_epistemic_conservation_circuit(
    width: Width,
    num_comps: usize,
) -> (Circuit, ComparatorSpecList) {
    let mut c = Circuit::new(width, 3, 1, WireDelayMode::Combinational);
    let d0 = c.add_node(Primitive::Delay { init: 0 });
    c.connect(
        SignalSource::BoundaryIn(0),
        SignalSink::NodeIn { node: d0, port: 0 },
    );
    c.connect(
        SignalSource::NodeOut { node: d0, port: 0 },
        SignalSink::BoundaryOut(0),
    );
    let mut comp_defs = Vec::with_capacity(num_comps);
    for m in 0..num_comps {
        let alt_ch = if m == 0 { 1u16 } else { 2u16 };
        let pri_src = tap_source(&mut c, SignalSource::NodeOut { node: d0, port: 0 });
        let d_alt = c.add_node(Primitive::Delay { init: 0 });
        let sub = c.add_node(Primitive::Sub);
        let eqz = c.add_node(Primitive::Eqz);
        c.connect(
            SignalSource::BoundaryIn(alt_ch),
            SignalSink::NodeIn {
                node: d_alt,
                port: 0,
            },
        );
        c.connect(pri_src, SignalSink::NodeIn { node: sub, port: 0 });
        c.connect(
            SignalSource::NodeOut {
                node: d_alt,
                port: 0,
            },
            SignalSink::NodeIn { node: sub, port: 1 },
        );
        c.connect(
            SignalSource::NodeOut { node: sub, port: 0 },
            SignalSink::NodeIn { node: eqz, port: 0 },
        );
        comp_defs.push((sub, eqz, 0, 1u64 << 0, 1u64 << alt_ch));
    }
    (c, comp_defs)
}

fn check_epistemic_mdl_for_seed(seed: u64) -> Result<f64, String> {
    let mut max_err_b_per_sym = 0.0f64;
    let mut rng = ChaCha8Rng::seed_from_u64(seed ^ 0x51a7_c0de);

    for width in [Width::BITS_1, Width::BITS_2] {
        let card = if width == Width::BITS_1 { 2u64 } else { 4u64 };
        for m in [0usize, 1, 2] {
            for noisy in [false, true] {
                let (circuit, comp_defs) = build_epistemic_conservation_circuit(width, m);
                let debts = well_mixed_description_debts(&circuit);
                let config = EconomyConfig {
                    juvenile_ticks: 0,
                    reserve_cap: ReserveCap::Uncapped,
                    ..EconomyConfig::default()
                };
                let initial_sum: f64 = (circuit.node_count() as f64) * config.initial_energy;
                let mut live =
                    LiveCircuit::new(circuit.clone(), &debts, config, SeedBank::new(seed), 8)
                        .map_err(|e| format!("LiveCircuit init failed: {e:?}"))?;
                for &(sub, eqz, tp, pm, am) in &comp_defs {
                    live.register_comparator(sub, eqz, tp, pm, am)
                        .map_err(|e| format!("register_comparator failed: {e:?}"))?;
                }

                let t_total = 180usize;
                let mut facts = Vec::with_capacity(t_total);
                let mut preds = Vec::with_capacity(t_total);
                let mut phases = Vec::with_capacity(t_total);
                let mut comp_pri_traces = vec![Vec::with_capacity(t_total); m];
                let mut comp_alt_traces = vec![Vec::with_capacity(t_total); m];

                let mut prev_driver = 0u64;
                for tick in 0..t_total {
                    let phase = if (60..120).contains(&tick) {
                        EpistemicTickPhase::Sleep
                    } else {
                        EpistemicTickPhase::Wake
                    };
                    let exec_phase = match phase {
                        EpistemicTickPhase::Wake => ExecutionPhase::Wake,
                        EpistemicTickPhase::Sleep => ExecutionPhase::Sleep,
                    };
                    live.set_phase(exec_phase)
                        .map_err(|e| format!("set_phase failed: {e:?}"))?;
                    phases.push(phase);

                    let clean_next = (prev_driver + 1) % card;
                    let ch0 = if noisy && (rng.next_u64() % 20 == 0) {
                        (prev_driver + 1) % card
                    } else {
                        prev_driver
                    };
                    let ch1 = if noisy && (rng.next_u64() % 20 == 0) {
                        (ch0 + 1) % card
                    } else {
                        ch0
                    };
                    let ch2 = ch0;

                    let pred = live
                        .predict(&[ch0, ch1, ch2])
                        .map_err(|e| format!("predict failed: {e:?}"))?;
                    preds.push(pred[0]);
                    let node_outs = node_output_values(live.circuit(), live.state());
                    let compiled = live.circuit().compile().expect("compiled");
                    let wires = live.state().wire_values();
                    for (ci, &(sub, ..)) in comp_defs.iter().enumerate() {
                        let sub_in = &compiled.node_input_wires()[sub.index()];
                        comp_pri_traces[ci].push(width.normalize(wires[sub_in[0]]));
                        comp_alt_traces[ci].push(width.normalize(wires[sub_in[1]]));
                    }
                    let _ = node_outs;

                    match phase {
                        EpistemicTickPhase::Wake => {
                            let fact_sym = if noisy && (rng.next_u64() % 20 == 0) {
                                (clean_next + 1) % card
                            } else {
                                clean_next
                            };
                            facts.push(fact_sym);
                            live.settle(&[fact_sym])
                                .map_err(|e| format!("settle failed: {e:?}"))?;
                        }
                        EpistemicTickPhase::Sleep => {
                            facts.push(0);
                            live.settle_sleep()
                                .map_err(|e| format!("settle_sleep failed: {e:?}"))?;
                        }
                    }
                    prev_driver = clean_next;
                }

                let final_sum: f64 = live.economy().nodes().iter().map(|n| n.energy).sum();
                let live_delta_e = final_sum - initial_sum;
                let comp_traces: Vec<EpistemicComparatorTrace<'_>> = comp_defs
                    .iter()
                    .enumerate()
                    .map(|(ci, &(_, _, tp, pm, am))| EpistemicComparatorTrace {
                        target_port: usize::from(tp),
                        registered_at_tick: 0,
                        primary_channel_mask: pm,
                        alt_channel_mask: am,
                        primary_predictions: &comp_pri_traces[ci],
                        alt_predictions: &comp_alt_traces[ci],
                    })
                    .collect();

                let mdl_delta = epistemic_null_calibrated_mdl_delta_with_tax(
                    &circuit,
                    0.0,
                    0,
                    &[&facts],
                    &[&preds],
                    &phases,
                    &comp_traces,
                    config.null_model,
                    config.split_code_tax,
                )
                .map_err(|e| format!("epistemic mdl eval error: {e:?}"))?;

                let err_b_per_sym = (live_delta_e - (-mdl_delta)).abs() / (t_total as f64);
                max_err_b_per_sym = max_err_b_per_sym.max(err_b_per_sym);
                if err_b_per_sym > G2B1_EPISTEMIC_MDL_TOL_BITS_PER_SYMBOL {
                    return Err(format!(
                        "width={width:?} M={m} noisy={noisy}: err={err_b_per_sym:.3e} > {:.3e}",
                        G2B1_EPISTEMIC_MDL_TOL_BITS_PER_SYMBOL
                    ));
                }
            }
        }
    }
    Ok(max_err_b_per_sym)
}

fn build_wake_comparator_starve_live(
    config: EconomyConfig,
    seed: u64,
) -> Result<
    (
        LiveCircuit,
        crate::substrate::NodeId,
        crate::substrate::NodeId,
    ),
    String,
> {
    let mut circuit = Circuit::new(Width::BITS_1, 2, 1, WireDelayMode::Combinational);
    let pri_d = circuit.add_node(Primitive::Delay { init: 0 });
    let split = circuit.add_node(Primitive::Split);
    let alt_d = circuit.add_node(Primitive::Delay { init: 0 });
    let sub = circuit.add_node(Primitive::Sub);
    let eqz = circuit.add_node(Primitive::Eqz);
    circuit.connect(
        SignalSource::BoundaryIn(0),
        SignalSink::NodeIn {
            node: pri_d,
            port: 0,
        },
    );
    circuit.connect(
        SignalSource::NodeOut {
            node: pri_d,
            port: 0,
        },
        SignalSink::NodeIn {
            node: split,
            port: 0,
        },
    );
    circuit.connect(
        SignalSource::NodeOut {
            node: split,
            port: 0,
        },
        SignalSink::BoundaryOut(0),
    );
    circuit.connect(
        SignalSource::NodeOut {
            node: split,
            port: 1,
        },
        SignalSink::NodeIn { node: sub, port: 0 },
    );
    circuit.connect(
        SignalSource::BoundaryIn(1),
        SignalSink::NodeIn {
            node: alt_d,
            port: 0,
        },
    );
    circuit.connect(
        SignalSource::NodeOut {
            node: alt_d,
            port: 0,
        },
        SignalSink::NodeIn { node: sub, port: 1 },
    );
    circuit.connect(
        SignalSource::NodeOut { node: sub, port: 0 },
        SignalSink::NodeIn { node: eqz, port: 0 },
    );
    let debts = well_mixed_description_debts(&circuit);
    let mut live = LiveCircuit::new(circuit, &debts, config, SeedBank::new(seed), 8)
        .map_err(|e| format!("LiveCircuit err: {e:?}"))?;
    live.register_comparator(sub, eqz, 0, 0b01, 0b10)
        .map_err(|e| format!("register err: {e:?}"))?;
    Ok((live, sub, eqz))
}

fn check_comparator_starvation_for_seed(seed: u64) -> Result<(), String> {
    let width = Width::BITS_1;
    let config = EconomyConfig::default();

    // 1. Silent comparator (0==0) in Wake on alternating truth starves.
    {
        let (mut live, sub, eqz) = build_wake_comparator_starve_live(config, seed)?;
        for t in 0..256u64 {
            let x = t % 2;
            let next_x = (t + 1) % 2;
            live.predict(&[x, 0]).expect("predict");
            live.settle(&[next_x]).expect("settle");
        }
        if live.economy().nodes()[eqz.index()].energy > 0.0
            && live.economy().nodes()[sub.index()].energy > 0.0
        {
            return Err("silent comparator failed to starve in Wake".to_owned());
        }
    }

    // 2. Constant comparator on T1_Constant(0) across Wake+Sleep gains 0.0 flux and starves.
    {
        let mut circuit = Circuit::new(width, 1, 1, WireDelayMode::Combinational);
        let alt_d = circuit.add_node(Primitive::Delay { init: 0 });
        let sub = circuit.add_node(Primitive::Sub);
        let eqz = circuit.add_node(Primitive::Eqz);
        circuit.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn {
                node: alt_d,
                port: 0,
            },
        );
        circuit.connect(
            SignalSource::NodeOut {
                node: alt_d,
                port: 0,
            },
            SignalSink::NodeIn { node: sub, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut {
                node: alt_d,
                port: 0,
            },
            SignalSink::NodeIn { node: sub, port: 1 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: sub, port: 0 },
            SignalSink::NodeIn { node: eqz, port: 0 },
        );
        let debts = well_mixed_description_debts(&circuit);
        let mut econ =
            EconomyState::new(config, width, circuit.node_count(), 1, 1, 192).expect("econ init");
        econ.assign_sprout_debts(&debts);
        econ.register_comparator(sub, eqz, 0, 0b01, 0b01)
            .expect("register comp");
        let ancestry = crate::structural::ComparatorCausalAncestry {
            base: crate::structural::CausalAncestry {
                port_shares: vec![vec![0.0; circuit.node_count()]],
                next_register_eligibility: vec![vec![0.0; circuit.node_count()]],
            },
            comparator_shares: vec![vec![0.0, 0.5, 0.5]],
        };
        let mut flux_sleep = 0.0f64;
        for t in 0..256 {
            econ.set_phase(ExecutionPhase::Wake);
            let _ = econ.score_port_single(0, 0, 0);
            let obs = econ
                .observe_comparators(&[0], &[0], &[1], Some(&[0]))
                .expect("observe");
            let maint = if t == usize::try_from(config.juvenile_ticks).unwrap_or(128) {
                debts.clone()
            } else {
                vec![0.0; circuit.node_count()]
            };
            econ.apply_causal_tick_with_comparators(
                &[0.0],
                &obs.rewards,
                obs.agreements,
                obs.disagreements,
                &ancestry,
                &maint,
            )
            .expect("tick");
        }
        for _ in 0..192 {
            econ.set_phase(ExecutionPhase::Sleep);
            let obs = econ
                .observe_comparators(&[0], &[0], &[1], None)
                .expect("observe");
            flux_sleep += obs.rewards[0];
            let maint = vec![0.0; circuit.node_count()];
            econ.apply_causal_tick_with_comparators(
                &[0.0],
                &obs.rewards,
                obs.agreements,
                obs.disagreements,
                &ancestry,
                &maint,
            )
            .expect("tick");
        }
        if flux_sleep.abs() > 1e-12 || econ.nodes()[eqz.index()].energy > 0.0 {
            return Err(format!(
                "constant Sleep comparator did not starve with 0 flux (flux={flux_sleep:.3e}, e_eqz={:.3})",
                econ.nodes()[eqz.index()].energy
            ));
        }
    }

    // 3. Agreeing-wrong comparator in Wake (pri == alt, wrong vs truth after prefix) drains negative flux & starves.
    {
        let (mut live, sub, eqz) = build_wake_comparator_starve_live(config, seed)?;
        for t in 0..256u64 {
            let truth_now = t % 2;
            let truth_next = (t + 1) % 2;
            let input_sym = if t < 12 { truth_next } else { 1 - truth_next };
            live.predict(&[input_sym, input_sym]).expect("predict");
            live.settle(&[truth_now]).expect("settle");
        }
        let comp_flux = live.economy().cumulative_comparator_flux();
        let e_eqz = live.economy().nodes()[eqz.index()].energy;
        let e_sub = live.economy().nodes()[sub.index()].energy;
        if comp_flux >= 0.0
            || live.economy().comparator_disagreement_ticks() != 0
            || (e_eqz > 0.0 && e_sub > 0.0)
        {
            return Err(format!(
                "agreeing-wrong comparator failed to starve (flux={comp_flux:.3}, disagree={}, e_eqz={e_eqz:.3}, e_sub={e_sub:.3})",
                live.economy().comparator_disagreement_ticks()
            ));
        }
    }

    Ok(())
}

fn evaluate_g2b1_seed(seed: u32, horizon: u64, warmup: u64) -> (GateTally, f64) {
    let mut tally = GateTally::default();
    let seed64 = u64::from(seed);
    let mut max_mdl_err = 0.0f64;

    // (a) Deterministic epistemic MDL + starvation
    let (mdl_ok, mdl_msg) = match check_epistemic_mdl_for_seed(seed64) {
        Ok(err) => {
            max_mdl_err = err;
            (true, None)
        }
        Err(msg) => (false, Some(msg)),
    };
    tally.record(
        format!("G2B.1a:epistemic_mdl:seed={seed}"),
        mdl_ok,
        &CliffCountersSummary::default(),
        mdl_msg,
    );

    let (starv_ok, starv_msg) = match check_comparator_starvation_for_seed(seed64) {
        Ok(()) => (true, None),
        Err(msg) => (false, Some(msg)),
    };
    tally.record(
        format!("G2B.1a:starvation:seed={seed}"),
        starv_ok,
        &CliffCountersSummary::default(),
        starv_msg,
    );

    // (b) T2 coupling panel + M1 core compatibility with default redundancy_sprouting = true
    let d0 = lfsr_deg5();
    let d1 = lfsr_deg4();
    let t2_panel = [
        (
            "T2_DelayedAffine(Comb)",
            Width::BITS_2,
            WireDelayMode::Combinational,
            1usize,
            GeneratorConfig::T2_DelayedAffine {
                driver: d0.clone(),
                lag: 2,
                mul_a: 3,
                add_b: 1,
            },
        ),
        (
            "T2_DelayedAffine(TPH)",
            Width::BITS_2,
            WireDelayMode::TicksPerHop,
            1usize,
            GeneratorConfig::T2_DelayedAffine {
                driver: d0.clone(),
                lag: 3,
                mul_a: 3,
                add_b: 1,
            },
        ),
        (
            "T2_CrossLinear(Add,Comb)",
            Width::BITS_2,
            WireDelayMode::Combinational,
            2usize,
            GeneratorConfig::T2_CrossLinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossLinearOp::Add,
            },
        ),
        (
            "T2_CrossLinear(Sub,TPH)",
            Width::BITS_2,
            WireDelayMode::TicksPerHop,
            2usize,
            GeneratorConfig::T2_CrossLinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 3,
                lag_1: 3,
                op: CrossLinearOp::Sub,
            },
        ),
        (
            "T2_CrossNonlinear(AddEqz,Comb)",
            Width::BITS_2,
            WireDelayMode::Combinational,
            2usize,
            GeneratorConfig::T2_CrossNonlinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossNonlinearOp::AddEqz,
            },
        ),
        (
            "T2_CrossNonlinear(MulEqz,TPH)",
            Width::BITS_2,
            WireDelayMode::TicksPerHop,
            2usize,
            GeneratorConfig::T2_CrossNonlinear {
                driver_0: d0.clone(),
                driver_1: d1,
                lag_0: 3,
                lag_1: 4,
                op: CrossNonlinearOp::MulEqz,
            },
        ),
    ];

    for (label, width, wire_delay, target_ch, cfg) in t2_panel {
        let trace = run_online_case(
            width,
            wire_delay,
            None,
            &cfg,
            &NoiseModel::None,
            GrowthConfig::default(),
            seed64,
            horizon,
            warmup,
            u64::MAX,
            DEFAULT_M2B_BUDGET,
        );
        let clean_mis = trace.post_warmup_clean_mismatches[target_ch];
        let kt = trace.post_warmup_kt[target_ch];
        let ok = clean_mis == 0 && kt <= G2B1_EXCESS_BITS_MAX;
        let msg = (!ok).then(|| format!("clean_mis={clean_mis}, excess={kt:.5}"));
        tally.record(format!("G2B.1b:{label}:seed={seed}"), ok, &trace.cliff, msg);
    }

    let m1_compat = [
        (
            "M1_Const(0)",
            Width::BITS_2,
            WireDelayMode::Combinational,
            GeneratorConfig::T1_Constant { value: 0 },
            true,
        ),
        (
            "M1_Alternating(Comb)",
            Width::BITS_1,
            WireDelayMode::Combinational,
            GeneratorConfig::T1_Alternating { a: 0, b: 1 },
            false,
        ),
        (
            "M1_Alternating(TPH)",
            Width::BITS_1,
            WireDelayMode::TicksPerHop,
            GeneratorConfig::T1_Alternating { a: 0, b: 1 },
            false,
        ),
        (
            "M1_PeriodicMotif",
            Width::BITS_2,
            WireDelayMode::Combinational,
            GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 1, 3, 2],
            },
            false,
        ),
        (
            "M1_CounterMod3",
            Width::BITS_2,
            WireDelayMode::Combinational,
            GeneratorConfig::T3b_CounterModN {
                modulus: 3,
                init: 0,
            },
            false,
        ),
        (
            "M1_AffineLag1",
            Width::BITS_2,
            WireDelayMode::Combinational,
            GeneratorConfig::T1_AffineLag1 {
                mul_a: 3,
                add_b: 1,
                init: 0,
            },
            false,
        ),
        (
            "M1_CounterPow2",
            Width::BITS_2,
            WireDelayMode::Combinational,
            GeneratorConfig::T3b_CounterPow2 {
                log2_modulus: 2,
                init: 0,
            },
            false,
        ),
    ];

    for (label, width, wire_delay, cfg, expect_zero_nodes) in m1_compat {
        let trace = run_online_case(
            width,
            wire_delay,
            None,
            &cfg,
            &NoiseModel::None,
            GrowthConfig::default(),
            seed64,
            horizon,
            warmup,
            u64::MAX,
            DEFAULT_M2B_BUDGET,
        );
        let clean_mis = trace.post_warmup_clean_mismatches[0];
        let kt = trace.post_warmup_kt[0];
        let zero_node_ok = !expect_zero_nodes || trace.telemetry.final_node_count == 0;
        let ok = clean_mis == 0 && kt <= G2B1_EXCESS_BITS_MAX && zero_node_ok;
        let msg = (!ok).then(|| {
            format!(
                "clean_mis={clean_mis}, excess={kt:.5}, nodes={}",
                trace.telemetry.final_node_count
            )
        });
        tally.record(format!("G2B.1b:{label}:seed={seed}"), ok, &trace.cliff, msg);
    }

    (tally, max_mdl_err)
}

// ---------------------------------------------------------------------------
// G2B.2: Spontaneous redundancy sprouting, causal disjointness & anti-bloat
// ---------------------------------------------------------------------------

fn build_causal_split_tph_editable(width: Width) -> EditableCircuit {
    let mut c = Circuit::new(width, 3, 3, WireDelayMode::TicksPerHop);
    let split = c.add_node(Primitive::Split);
    c.connect(
        SignalSource::BoundaryIn(0),
        SignalSink::NodeIn {
            node: split,
            port: 0,
        },
    );
    c.connect(
        SignalSource::NodeOut {
            node: split,
            port: 0,
        },
        SignalSink::BoundaryOut(2),
    );
    let econ = EconomyConfig::default();
    let mut editable = EditableCircuit::new(c, econ).expect("valid split-primary editable");
    if let Some(ne) = editable.economy_mut().node_mut(split.index()) {
        ne.juvenile_ticks = econ.juvenile_ticks;
    }
    editable
}

fn check_comparator_disjointness(
    learner: &ArkheLearner,
    target_port: u16,
    is_multiview: bool,
) -> Result<usize, String> {
    let circuit = learner.editable().circuit();
    let comps = learner.editable().economy().comparators();
    let mut target_port_comps = 0usize;
    let mut counts_by_port = vec![0usize; usize::from(circuit.num_outputs())];

    for comp in comps {
        let port_idx = usize::from(comp.target_port);
        counts_by_port[port_idx] += 1;
        if comp.target_port == target_port {
            target_port_comps += 1;
        }
        let pri_info = derivation_cone_info(
            circuit,
            &[SignalSink::NodeIn {
                node: comp.sub_node,
                port: 0,
            }],
        );
        let alt_info = derivation_cone_info(
            circuit,
            &[SignalSink::NodeIn {
                node: comp.sub_node,
                port: 1,
            }],
        );
        if !pri_info.excl_nodes.is_disjoint(&alt_info.excl_nodes) {
            return Err(format!(
                "port {} comparator shares interior nodes",
                comp.target_port
            ));
        }
        if !pri_info
            .excl_reactive_leaves
            .is_disjoint(&alt_info.excl_reactive_leaves)
        {
            return Err(format!(
                "port {} comparator shares reactive leaves",
                comp.target_port
            ));
        }
        if (!is_multiview || comp.target_port == target_port)
            && (comp.primary_channel_mask & comp.alt_channel_mask) != 0
        {
            return Err(format!(
                "port {} comparator masks overlap ({:#x} & {:#x})",
                comp.target_port, comp.primary_channel_mask, comp.alt_channel_mask
            ));
        }
        if is_multiview
            && comp.target_port == target_port
            && (comp.primary_channel_mask == 0 || comp.alt_channel_mask == 0)
        {
            return Err(format!(
                "multiview port {target_port} comparator missing disjoint nonzero channels"
            ));
        }
    }

    for (p, &cnt) in counts_by_port.iter().enumerate() {
        if cnt > 1 {
            return Err(format!(
                "anti-bloat violation: port {p} has {cnt} comparators"
            ));
        }
    }
    Ok(target_port_comps)
}

fn evaluate_g2b2_seed(seed: u32, horizon: u64, warmup: u64) -> GateTally {
    let mut tally = GateTally::default();
    let seed64 = u64::from(seed);
    let d11 = lfsr_deg11();

    let mv_tasks = [
        (
            "MV_Affine_k1_Comb",
            Width::BITS_1,
            WireDelayMode::Combinational,
            false,
            GeneratorConfig::T2_MultiViewCoupled {
                driver: MultiViewDriver::Lfsr { stage: d11.clone() },
                view_0: MultiViewTransform {
                    delay: 1,
                    mul_a: 1,
                    add_b: 0,
                },
                view_1: MultiViewTransform {
                    delay: 0,
                    mul_a: 1,
                    add_b: 1,
                },
                target: MultiViewTarget::Affine {
                    lag: 2,
                    mul_a: 1,
                    add_b: 0,
                },
            },
        ),
        (
            "MV_Affine_k2_Comb",
            Width::BITS_2,
            WireDelayMode::Combinational,
            false,
            GeneratorConfig::T2_MultiViewCoupled {
                driver: MultiViewDriver::Lfsr { stage: d11.clone() },
                view_0: MultiViewTransform {
                    delay: 1,
                    mul_a: 1,
                    add_b: 0,
                },
                view_1: MultiViewTransform {
                    delay: 0,
                    mul_a: 3,
                    add_b: 2,
                },
                target: MultiViewTarget::Affine {
                    lag: 2,
                    mul_a: 3,
                    add_b: 1,
                },
            },
        ),
        (
            "MV_Eqz_k2_Comb",
            Width::BITS_2,
            WireDelayMode::Combinational,
            false,
            GeneratorConfig::T2_MultiViewCoupled {
                driver: MultiViewDriver::Lfsr { stage: d11.clone() },
                view_0: MultiViewTransform {
                    delay: 1,
                    mul_a: 1,
                    add_b: 0,
                },
                view_1: MultiViewTransform {
                    delay: 0,
                    mul_a: 3,
                    add_b: 1,
                },
                target: MultiViewTarget::Eqz { lag: 3, add_b: 2 },
            },
        ),
        (
            "MV_AddEqz_k2_Comb",
            Width::BITS_2,
            WireDelayMode::Combinational,
            false,
            GeneratorConfig::T2_MultiViewCoupled {
                driver: MultiViewDriver::Lfsr { stage: d11.clone() },
                view_0: MultiViewTransform {
                    delay: 0,
                    mul_a: 1,
                    add_b: 0,
                },
                view_1: MultiViewTransform {
                    delay: 3,
                    mul_a: 1,
                    add_b: 0,
                },
                target: MultiViewTarget::AddEqz { lag_a: 5, lag_b: 6 },
            },
        ),
        (
            "MV_MulEqz_k2_Comb",
            Width::BITS_2,
            WireDelayMode::Combinational,
            false,
            GeneratorConfig::T2_MultiViewCoupled {
                driver: MultiViewDriver::Lfsr { stage: d11.clone() },
                view_0: MultiViewTransform {
                    delay: 0,
                    mul_a: 1,
                    add_b: 0,
                },
                view_1: MultiViewTransform {
                    delay: 3,
                    mul_a: 1,
                    add_b: 0,
                },
                target: MultiViewTarget::MulEqz { lag_a: 5, lag_b: 6 },
            },
        ),
        (
            "MV_Affine_k2_TphSplit",
            Width::BITS_2,
            WireDelayMode::TicksPerHop,
            true,
            GeneratorConfig::T2_MultiViewCoupled {
                driver: MultiViewDriver::Lfsr { stage: d11 },
                view_0: MultiViewTransform {
                    delay: 1,
                    mul_a: 1,
                    add_b: 0,
                },
                view_1: MultiViewTransform {
                    delay: 1,
                    mul_a: 3,
                    add_b: 2,
                },
                target: MultiViewTarget::Affine {
                    lag: 4,
                    mul_a: 1,
                    add_b: 0,
                },
            },
        ),
    ];

    for (label, width, wire_delay, split_tph, cfg) in mv_tasks {
        let init_ed = split_tph.then(|| build_causal_split_tph_editable(width));
        let trace = run_online_case(
            width,
            wire_delay,
            init_ed,
            &cfg,
            &NoiseModel::None,
            GrowthConfig::default(),
            seed64,
            horizon,
            warmup,
            u64::MAX,
            DEFAULT_M2B_BUDGET,
        );
        let disjoint_res = check_comparator_disjointness(&trace.learner, 2, true);
        let (ok, msg) = match disjoint_res {
            Err(e) => (false, Some(e)),
            Ok(target_comps) => {
                let target_starv = trace.learner.port_starvation_prune_events()[2];
                let comp_starv = trace.learner.comparator_starvation_prune_events();
                let good = target_comps == 1
                    && target_starv == 0
                    && comp_starv == 0
                    && trace.post_warmup_clean_mismatches[2] == 0
                    && trace.settled_comparator_ticks > 0
                    && trace.settled_comparator_disagreements == 0
                    && trace.settled_max_circular_distance == 0;
                let err_msg = (!good).then(|| {
                    format!(
                        "target_comps={target_comps}, target_starv={target_starv}, comp_starv={comp_starv}, clean_mis={}, settled=({}/{}, circ={})",
                        trace.post_warmup_clean_mismatches[2],
                        trace.settled_comparator_disagreements,
                        trace.settled_comparator_ticks,
                        trace.settled_max_circular_distance
                    )
                });
                (good, err_msg)
            }
        };
        tally.record(format!("G2B.2:{label}:seed={seed}"), ok, &trace.cliff, msg);
    }

    let t1_tasks = [
        (
            "T1_Alternating_k1",
            Width::BITS_1,
            GeneratorConfig::T1_Alternating { a: 0, b: 1 },
        ),
        (
            "T1_PeriodicMotif_k2",
            Width::BITS_2,
            GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 1, 3, 2],
            },
        ),
    ];
    for (label, width, cfg) in t1_tasks {
        let trace = run_online_case(
            width,
            WireDelayMode::Combinational,
            None,
            &cfg,
            &NoiseModel::None,
            GrowthConfig::default(),
            seed64,
            horizon,
            warmup,
            u64::MAX,
            DEFAULT_M2B_BUDGET,
        );
        let (ok, msg) = match check_comparator_disjointness(&trace.learner, 0, false) {
            Err(e) => (false, Some(e)),
            Ok(target_comps) => {
                let target_starv = trace.learner.port_starvation_prune_events()[0];
                let comp_starv = trace.learner.comparator_starvation_prune_events();
                let good = target_comps == 1
                    && target_starv == 0
                    && comp_starv == 0
                    && trace.post_warmup_clean_mismatches[0] == 0
                    && trace.settled_comparator_ticks > 0
                    && trace.settled_comparator_disagreements == 0
                    && trace.settled_max_circular_distance == 0;
                let err_msg = (!good).then(|| {
                    format!(
                        "target_comps={target_comps}, target_starv={target_starv}, comp_starv={comp_starv}, clean_mis={}, settled=({}/{}, circ={})",
                        trace.post_warmup_clean_mismatches[0],
                        trace.settled_comparator_disagreements,
                        trace.settled_comparator_ticks,
                        trace.settled_max_circular_distance
                    )
                });
                (good, err_msg)
            }
        };
        tally.record(format!("G2B.2:{label}:seed={seed}"), ok, &trace.cliff, msg);
    }

    let d11 = lfsr_deg11();
    let d9 = lfsr_deg9();
    for (label, wire_delay) in [
        (
            "T2_NonRedundant_deg11xdeg9_Comb",
            WireDelayMode::Combinational,
        ),
        ("T2_NonRedundant_deg11xdeg9_TPH", WireDelayMode::TicksPerHop),
    ] {
        let cfg = GeneratorConfig::T2_CrossNonlinear {
            driver_0: d11.clone(),
            driver_1: d9.clone(),
            lag_0: 4,
            lag_1: 4,
            op: CrossNonlinearOp::MulEqzPair,
        };
        let trace = run_online_case(
            Width::BITS_1,
            wire_delay,
            None,
            &cfg,
            &NoiseModel::None,
            GrowthConfig::default(),
            seed64,
            horizon,
            warmup,
            u64::MAX,
            DEFAULT_M2B_BUDGET,
        );
        let ok = trace.telemetry.active_comparator_count == 0
            && trace.telemetry.redundancy_sprout_events == 0;
        let msg = (!ok).then(|| {
            format!(
                "active_comps={}, red_sprouts={}",
                trace.telemetry.active_comparator_count, trace.telemetry.redundancy_sprout_events
            )
        });
        tally.record(format!("G2B.2:{label}:seed={seed}"), ok, &trace.cliff, msg);
    }

    tally
}

// ---------------------------------------------------------------------------
// G2B.3: Offline loopback verification & majority pruning (T_loop = 128)
// ---------------------------------------------------------------------------

fn build_lag2_alternating_base_editable(width: Width, num_channels: u16) -> EditableCircuit {
    let mut c = Circuit::new(
        width,
        num_channels,
        num_channels,
        WireDelayMode::Combinational,
    );
    // ArkheLearner feeds prev_observed_frame = obs(t - 1) onto BoundaryIn, so one
    // Delay node yields lag 2 (obs(t - 2) = obs(t) for period-2 alternation).
    let d1 = c.add_node(Primitive::Delay { init: 0 });
    c.connect(
        SignalSource::BoundaryIn(0),
        SignalSink::NodeIn { node: d1, port: 0 },
    );
    c.connect(
        SignalSource::NodeOut { node: d1, port: 0 },
        SignalSink::BoundaryOut(0),
    );
    EditableCircuit::new(c, EconomyConfig::default()).expect("valid lag2 editable")
}

fn evaluate_g2b3_seed(seed: u32, horizon: u64, warmup: u64) -> GateTally {
    let mut tally = GateTally::default();
    let seed64 = u64::from(seed);
    let t_loop = DEFAULT_M2B_LOOPBACK_TICKS;

    // (a.1) T1 Alternating consistent loopback -> 0 disagreements, positive flux
    {
        let mut trace = run_online_case(
            Width::BITS_1,
            WireDelayMode::Combinational,
            None,
            &GeneratorConfig::T1_Alternating { a: 0, b: 1 },
            &NoiseModel::None,
            GrowthConfig::default(),
            seed64,
            horizon,
            warmup,
            u64::MAX,
            DEFAULT_M2B_BUDGET,
        );
        let rep = trace.learner.run_offline_loopback(t_loop);
        let ok = trace.learner.active_comparator_count() >= 1
            && rep.ticks == t_loop
            && rep.comparator_disagreement_ticks == 0
            && rep.max_circular_discrepancy == 0
            && !rep.contradiction_detected
            && rep.sleep_comparator_flux > 0.0
            && rep.minority_routes_pruned == 0;
        let msg = (!ok).then(|| format!("loopback_rep={rep:?}"));
        tally.record(
            format!("G2B.3a:T1_Alternating:seed={seed}"),
            ok,
            &trace.cliff,
            msg,
        );
    }

    // (a.2) T2_MultiViewCoupled (entrained PeriodicMotif) consistent loopback
    {
        let cfg = GeneratorConfig::T2_MultiViewCoupled {
            driver: MultiViewDriver::PeriodicMotif {
                motif: vec![0, 1, 3, 2],
            },
            view_0: MultiViewTransform {
                delay: 1,
                mul_a: 1,
                add_b: 0,
            },
            view_1: MultiViewTransform {
                delay: 0,
                mul_a: 3,
                add_b: 1,
            },
            target: MultiViewTarget::Affine {
                lag: 2,
                mul_a: 3,
                add_b: 1,
            },
        };
        let mut trace = run_online_case(
            Width::BITS_2,
            WireDelayMode::Combinational,
            None,
            &cfg,
            &NoiseModel::None,
            GrowthConfig::default(),
            seed64,
            horizon,
            warmup,
            u64::MAX,
            DEFAULT_M2B_BUDGET,
        );
        let pre_comps = trace.learner.active_comparator_count();
        let rep = trace.learner.run_offline_loopback(t_loop);
        let ok = pre_comps >= 1
            && rep.ticks == t_loop
            && rep.comparator_disagreement_ticks == 0
            && rep.max_circular_discrepancy == 0
            && !rep.contradiction_detected
            && rep.sleep_comparator_flux > 0.0;
        let msg = (!ok).then(|| format!("pre_comps={pre_comps}, loopback_rep={rep:?}"));
        tally.record(
            format!("G2B.3a:T2_MultiViewEntrained:seed={seed}"),
            ok,
            &trace.cliff,
            msg,
        );
    }

    // (a.3) Constant(0) loopback yields exact 0.0 flux
    {
        let mut c = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);
        let pri = c.add_node(Primitive::Delay { init: 0 });
        c.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: pri, port: 0 },
        );
        c.connect(
            SignalSource::NodeOut { node: pri, port: 0 },
            SignalSink::BoundaryOut(0),
        );
        let mut ed = EditableCircuit::new(c, EconomyConfig::default()).expect("editable");
        let mut next_c = ed.circuit().clone();
        let (_, _, alt_src) = build_autonomous_ring_source(&mut next_c, 2, &[0, 0]);
        let spliced = splice_comparator_onto_port(&mut next_c, 0, alt_src).expect("splice");
        let old_to_new: Vec<Option<crate::substrate::NodeId>> = (0..ed.circuit().node_count())
            .map(|i| Some(crate::substrate::NodeId(i as u32)))
            .collect();
        ed.recompile_preserving_state(next_c, &old_to_new)
            .expect("recompile");
        ed.register_comparator(spliced.sub_node, spliced.eqz_node, 0, 0b1, 0)
            .expect("register");
        let mut learner =
            ArkheLearner::from_editable(ed, GrowthConfig::default(), SeedBank::new(seed64))
                .expect("learner");
        let mut budget = TickBudget::new(20_000);
        for _ in 0..256 {
            let _ = learner.predict(&mut budget);
            learner.observe(&[0], &mut budget);
        }
        let rep = learner.run_offline_loopback(t_loop);
        let ok = rep.sleep_comparator_flux == 0.0 && rep.comparator_disagreement_ticks == 0;
        let msg = (!ok).then(|| format!("const0 loopback_rep={rep:?}"));
        tally.record(
            format!("G2B.3a:Constant0:seed={seed}"),
            ok,
            &CliffCountersSummary::default(),
            msg,
        );
    }

    // (b.1) Deceptive lag-2 vs period-6 [0,1,0,1,0,0] (Charter §1.3)
    {
        let mut c = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);
        let d1 = c.add_node(Primitive::Delay { init: 0 });
        let d2 = c.add_node(Primitive::Delay { init: 1 });
        c.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: d1, port: 0 },
        );
        c.connect(
            SignalSource::NodeOut { node: d1, port: 0 },
            SignalSink::NodeIn { node: d2, port: 0 },
        );
        c.connect(
            SignalSource::NodeOut { node: d2, port: 0 },
            SignalSink::BoundaryOut(0),
        );
        let motif = [0u64, 1, 0, 1, 0, 0];
        let (_, _, ring_src) = build_autonomous_ring_source(&mut c, 6, &motif);
        let spliced = splice_comparator_onto_port(&mut c, 0, ring_src).expect("splice");
        let mut ed = EditableCircuit::new(c, EconomyConfig::default()).expect("editable");
        ed.register_comparator(spliced.sub_node, spliced.eqz_node, 0, 0b1, 0)
            .expect("register");
        let mut learner =
            ArkheLearner::from_editable(ed, GrowthConfig::default(), SeedBank::new(seed64))
                .expect("learner");
        let mut budget = TickBudget::new(20_000);
        for t in 0..24 {
            let _ = learner.predict(&mut budget);
            learner.observe(&[motif[t % 6]], &mut budget);
        }
        let rep = learner.run_offline_loopback(t_loop);
        let ok = rep.contradiction_detected
            && rep.comparator_disagreement_ticks >= 1
            && rep.max_circular_discrepancy >= 1
            && rep.sleep_comparator_flux < 0.0;
        let msg = (!ok).then(|| format!("deceptive_p6 loopback_rep={rep:?}"));
        tally.record(
            format!("G2B.3b:DeceptivePeriod6:seed={seed}"),
            ok,
            &CliffCountersSummary::default(),
            msg,
        );
    }

    // (b.2) Spurious cross-channel lag falsification in loopback (prefix-aliased across channels)
    {
        let mut c = Circuit::new(Width::BITS_2, 2, 2, WireDelayMode::Combinational);
        // Both channels agree on Wake ticks 0..8 and the first 4 Sleep ticks 8..12,
        // then diverge on the remaining 52 ticks of the 64-tick cycle (12/64 = 18.75% < 25%).
        let mut motif_0 = [0u64; 64];
        let mut motif_1 = [0u64; 64];
        for i in 0..64 {
            let v = (i % 4) as u64;
            motif_0[i] = v;
            motif_1[i] = if i < 12 { v } else { (v + 1) % 4 };
        }
        let (_, _, r0) = build_autonomous_ring_source(&mut c, 64, &motif_0);
        let (_, _, r1) = build_autonomous_ring_source(&mut c, 64, &motif_1);
        c.connect(r0, SignalSink::BoundaryOut(0));
        c.connect(r1, SignalSink::BoundaryOut(1));
        let alt_src = tap_source(&mut c, r1);
        let spliced = splice_comparator_onto_port(&mut c, 0, alt_src).expect("splice");
        let mut ed = EditableCircuit::new(c, EconomyConfig::default()).expect("editable");
        ed.register_comparator(spliced.sub_node, spliced.eqz_node, 0, 0b01, 0b10)
            .expect("register");
        let mut learner =
            ArkheLearner::from_editable(ed, GrowthConfig::default(), SeedBank::new(seed64))
                .expect("learner");
        let mut budget = TickBudget::new(20_000);
        for t in 0..8 {
            let _ = learner.predict(&mut budget);
            learner.observe(&[motif_0[t], motif_1[t]], &mut budget);
        }
        let rep = learner.run_offline_loopback(t_loop);
        let ok = rep.contradiction_detected
            && rep.comparator_disagreement_ticks >= 1
            && rep.max_circular_discrepancy >= 1
            && rep.sleep_comparator_flux < 0.0;
        let msg = (!ok).then(|| format!("spurious_cross loopback_rep={rep:?}"));
        tally.record(
            format!("G2B.3b:SpuriousCrossLag:seed={seed}"),
            ok,
            &CliffCountersSummary::default(),
            msg,
        );
    }

    // (c) 3-route loopback majority consensus pruning (faulty alt vs faulty primary)
    for primary_is_minority in [false, true] {
        let mut ed = build_lag2_alternating_base_editable(Width::BITS_1, 1);
        let (p1, alt1_motif, p2, alt2_motif): (usize, &[u64], usize, &[u64]) =
            if primary_is_minority {
                (6, &[0, 1, 0, 1, 0, 0], 6, &[0, 1, 0, 1, 0, 0])
            } else {
                (2, &[0, 1], 6, &[0, 1, 0, 1, 0, 0])
            };
        let mut next_c = ed.circuit().clone();
        let (_, _, r1) = build_autonomous_ring_source(&mut next_c, p1, alt1_motif);
        let s1 = splice_comparator_onto_port(&mut next_c, 0, r1).expect("splice 1");
        let (_, _, r2) = build_autonomous_ring_source(&mut next_c, p2, alt2_motif);
        let s2 = splice_comparator_onto_port(&mut next_c, 0, r2).expect("splice 2");
        let otn: Vec<Option<crate::substrate::NodeId>> = (0..ed.circuit().node_count())
            .map(|i| Some(crate::substrate::NodeId(i as u32)))
            .collect();
        ed.recompile_preserving_state(next_c, &otn).expect("recomp");
        ed.register_comparator(s1.sub_node, s1.eqz_node, 0, 0b1, 0)
            .expect("reg1");
        ed.register_comparator(s2.sub_node, s2.eqz_node, 0, 0b1, 0)
            .expect("reg2");

        let mut learner =
            ArkheLearner::from_editable(ed, GrowthConfig::default(), SeedBank::new(seed64))
                .expect("learner");
        let mut budget = TickBudget::new(20_000);
        // 60 Wake ticks leaves 68 juvenile ticks so maturity + 4-tick consensus trigger within T_loop = 128.
        for t in 0..60 {
            let _ = learner.predict(&mut budget);
            learner.observe(&[(t % 2) as u64], &mut budget);
        }
        let rep = learner.run_offline_loopback(t_loop);
        let follow = learner.run_offline_loopback(64);
        let ok = rep.minority_routes_pruned == 1
            && learner.active_comparator_count() == 1
            && !follow.contradiction_detected
            && follow.comparator_disagreement_ticks == 0
            && follow.max_circular_discrepancy == 0;
        let msg = (!ok).then(|| {
            format!(
                "primary_minority={primary_is_minority}: rep={rep:?}, rem={}, follow={follow:?}",
                learner.active_comparator_count()
            )
        });
        tally.record(
            format!("G2B.3c:Majority3(pri_min={primary_is_minority}):seed={seed}"),
            ok,
            &CliffCountersSummary::default(),
            msg,
        );
    }

    tally
}

// ---------------------------------------------------------------------------
// G2B.4: Stationary 5% noise robustness & NoisyConstant halting
// ---------------------------------------------------------------------------

struct G2b4SeedResult {
    tally: GateTally,
    kt_sum: f64,
    h_bayes_class_sum: f64,
    sample_count: usize,
}

fn theoretical_channel_flip_disagreement(eps_a: f64, eps_b: f64, k_bits: u8) -> f64 {
    if k_bits == 0 {
        return 0.0;
    }
    let collide = if k_bits == 1 {
        1.0
    } else {
        1.0 / ((1u64 << k_bits) as f64 - 1.0)
    };
    let p_agree = (1.0 - eps_a) * (1.0 - eps_b) + eps_a * eps_b * collide;
    (1.0 - p_agree).clamp(0.0, 1.0)
}

fn evaluate_g2b4_seed(seed: u32, horizon: u64, warmup: u64) -> G2b4SeedResult {
    let mut tally = GateTally::default();
    let seed64 = u64::from(seed);
    let mut kt_sum = 0.0f64;
    let mut h_bayes_class_sum = 0.0f64;
    let mut sample_count = 0usize;

    let d11 = lfsr_deg11();

    // Positive stationary 5% noise tasks:
    // (label, width, cfg, noisy_channels, oracle_noisy_inputs, is_periodic_class)
    let pos_tasks = [
        (
            "MV_TargetNoise_k1",
            Width::BITS_1,
            GeneratorConfig::T2_MultiViewCoupled {
                driver: MultiViewDriver::Lfsr { stage: d11.clone() },
                view_0: MultiViewTransform {
                    delay: 1,
                    mul_a: 1,
                    add_b: 0,
                },
                view_1: MultiViewTransform {
                    delay: 0,
                    mul_a: 1,
                    add_b: 1,
                },
                target: MultiViewTarget::Affine {
                    lag: 2,
                    mul_a: 1,
                    add_b: 0,
                },
            },
            vec![2u16],
            0usize,
            false,
        ),
        (
            "MV_TargetNoise_k2",
            Width::BITS_2,
            GeneratorConfig::T2_MultiViewCoupled {
                driver: MultiViewDriver::Lfsr { stage: d11.clone() },
                view_0: MultiViewTransform {
                    delay: 1,
                    mul_a: 1,
                    add_b: 0,
                },
                view_1: MultiViewTransform {
                    delay: 0,
                    mul_a: 3,
                    add_b: 2,
                },
                target: MultiViewTarget::Affine {
                    lag: 2,
                    mul_a: 3,
                    add_b: 1,
                },
            },
            vec![2u16],
            0usize,
            false,
        ),
        (
            "MV_AllChNoise_k1",
            Width::BITS_1,
            GeneratorConfig::T2_MultiViewCoupled {
                driver: MultiViewDriver::Lfsr { stage: d11.clone() },
                view_0: MultiViewTransform {
                    delay: 1,
                    mul_a: 1,
                    add_b: 0,
                },
                view_1: MultiViewTransform {
                    delay: 0,
                    mul_a: 1,
                    add_b: 1,
                },
                target: MultiViewTarget::Affine {
                    lag: 2,
                    mul_a: 1,
                    add_b: 0,
                },
            },
            vec![0u16, 1u16, 2u16],
            1usize,
            false,
        ),
        (
            "MV_AllChNoise_k2",
            Width::BITS_2,
            GeneratorConfig::T2_MultiViewCoupled {
                driver: MultiViewDriver::Lfsr { stage: d11 },
                view_0: MultiViewTransform {
                    delay: 1,
                    mul_a: 1,
                    add_b: 0,
                },
                view_1: MultiViewTransform {
                    delay: 0,
                    mul_a: 3,
                    add_b: 2,
                },
                target: MultiViewTarget::Affine {
                    lag: 2,
                    mul_a: 3,
                    add_b: 1,
                },
            },
            vec![0u16, 1u16, 2u16],
            1usize,
            false,
        ),
        (
            "NoisyAlternating_k1",
            Width::BITS_1,
            GeneratorConfig::T1_Alternating { a: 0, b: 1 },
            vec![0u16],
            0usize,
            true,
        ),
    ];

    for (label, width, cfg, noisy_chs, num_noisy_inputs, is_periodic_class) in pos_tasks {
        let mut noisy_mask = 0u64;
        for &ch in &noisy_chs {
            if ch < 64 {
                noisy_mask |= 1u64 << ch;
            }
        }
        let noise = NoiseModel::IndependentSubstitution {
            epsilon: G2B4_NOISE_EPSILON,
            channels: noisy_chs,
        };
        let trace = run_online_case(
            width,
            WireDelayMode::Combinational,
            None,
            &cfg,
            &noise,
            GrowthConfig::default(),
            seed64,
            horizon,
            warmup,
            u64::MAX,
            DEFAULT_M2B_BUDGET,
        );
        let (oracle_kt, h_bayes_class) = if is_periodic_class {
            (
                run_clean_phase_oracle_target_kt(width, &cfg, &noise, seed64, horizon, warmup),
                channel_substitution_entropy_bits(G2B4_NOISE_EPSILON, width.bits()),
            )
        } else {
            (
                run_causal_oracle_target_kt(width, &cfg, &noise, seed64, horizon, warmup),
                compound_reactive_entropy_bits(G2B4_NOISE_EPSILON, num_noisy_inputs, width.bits()),
            )
        };
        let target_ch = trace.post_warmup_kt.len() - 1;
        let learner_kt = trace.post_warmup_kt[target_ch];
        let paired_diff = (learner_kt - oracle_kt).abs();

        kt_sum += learner_kt;
        h_bayes_class_sum += h_bayes_class;
        sample_count += 1;

        // Disagreement rate and theoretical flip rate from formed target comparator's leaf masks
        let target_comp = trace
            .learner
            .editable()
            .economy()
            .comparators()
            .iter()
            .enumerate()
            .find(|(_, c)| usize::from(c.target_port) == target_ch);
        let (target_comp_dis_rate, theory_dis_rate, m_pri, m_alt) = match target_comp {
            Some((idx, comp)) => {
                let m_pri = (comp.primary_channel_mask & noisy_mask).count_ones() as usize;
                let m_alt = (comp.alt_channel_mask & noisy_mask).count_ones() as usize;
                let eps_pri = cone_substitution_error_rate(G2B4_NOISE_EPSILON, m_pri, width.bits());
                let eps_alt = cone_substitution_error_rate(G2B4_NOISE_EPSILON, m_alt, width.bits());
                let theory = theoretical_channel_flip_disagreement(eps_pri, eps_alt, width.bits());
                let (ticks, dis) = trace
                    .active_comparator_ticks_by_comp
                    .get(idx)
                    .copied()
                    .unwrap_or((0, 0));
                let rate = (ticks > 0).then_some((dis as f64) / (ticks as f64));
                (rate, theory, m_pri, m_alt)
            }
            None => (None, 0.0, 0, 0),
        };

        let dis_rate_ok = target_comp_dis_rate
            .is_some_and(|r| (r - theory_dis_rate).abs() <= G2B4_DISAGREEMENT_TOL);
        let ok = paired_diff <= G2B4_KT_DIFF_MAX
            && trace.telemetry.disagreement_prune_events == 0
            && trace.telemetry.active_comparator_count >= 1
            && dis_rate_ok;
        let msg = (!ok).then(|| {
            format!(
                "kt={learner_kt:.4}, oracle_kt={oracle_kt:.4}, h_bayes_class={h_bayes_class:.4}, paired_diff={paired_diff:.4}, dis_prunes={}, active_comps={}, dis_rate={target_comp_dis_rate:?} (theory={theory_dis_rate:.4}, m_pri={m_pri}, m_alt={m_alt})",
                trace.telemetry.disagreement_prune_events,
                trace.telemetry.active_comparator_count
            )
        });
        tally.record(format!("G2B.4:{label}:seed={seed}"), ok, &trace.cliff, msg);
    }

    // NoisyConstant(0) and NoisyConstant(2^k - 1) halting at 0 nodes and 0 redundancy sprouts
    for width in [Width::BITS_1, Width::BITS_2] {
        let mask = width.mask();
        for (tag, val) in [("Const0", 0u64), ("ConstMax", mask)] {
            let cfg = GeneratorConfig::T1_Constant { value: val };
            let noise = NoiseModel::IndependentSubstitution {
                epsilon: G2B4_NOISE_EPSILON,
                channels: vec![0],
            };
            let trace = run_online_case(
                width,
                WireDelayMode::Combinational,
                None,
                &cfg,
                &noise,
                GrowthConfig::default(),
                seed64,
                horizon,
                warmup,
                u64::MAX,
                DEFAULT_M2B_BUDGET,
            );
            let ok = trace.telemetry.final_node_count == 0
                && trace.telemetry.redundancy_sprout_events == 0
                && trace.telemetry.active_comparator_count == 0;
            let msg = (!ok).then(|| {
                format!(
                    "nodes={}, red_sprouts={}, comps={}",
                    trace.telemetry.final_node_count,
                    trace.telemetry.redundancy_sprout_events,
                    trace.telemetry.active_comparator_count
                )
            });
            tally.record(
                format!("G2B.4:Noisy{tag}(k={}):seed={seed}", width.bits()),
                ok,
                &trace.cliff,
                msg,
            );
        }
    }

    G2b4SeedResult {
        tally,
        kt_sum,
        h_bayes_class_sum,
        sample_count,
    }
}

// ---------------------------------------------------------------------------
// G2B.5: Mid-run channel fault survival & recovery
// ---------------------------------------------------------------------------

struct G2b5SeedResult {
    tally: GateTally,
    advantage_sum: f64,
    pair_count: usize,
}

fn evaluate_g2b5_seed(seed: u32, horizon: u64, warmup: u64) -> G2b5SeedResult {
    let mut tally = GateTally::default();
    let seed64 = u64::from(seed);
    let mut advantage_sum = 0.0f64;
    let mut pair_count = 0usize;

    let d11 = lfsr_deg11();
    let fault_tick = G2B5_FAULT_TICK.min(horizon / 2);

    let configs = [
        (
            "MV_Fault_k1",
            Width::BITS_1,
            GeneratorConfig::T2_MultiViewCoupled {
                driver: MultiViewDriver::Lfsr { stage: d11.clone() },
                view_0: MultiViewTransform {
                    delay: 1,
                    mul_a: 1,
                    add_b: 0,
                },
                view_1: MultiViewTransform {
                    delay: 0,
                    mul_a: 1,
                    add_b: 1,
                },
                target: MultiViewTarget::Affine {
                    lag: 2,
                    mul_a: 1,
                    add_b: 0,
                },
            },
        ),
        (
            "MV_Fault_k2",
            Width::BITS_2,
            GeneratorConfig::T2_MultiViewCoupled {
                driver: MultiViewDriver::Lfsr { stage: d11 },
                view_0: MultiViewTransform {
                    delay: 1,
                    mul_a: 1,
                    add_b: 0,
                },
                view_1: MultiViewTransform {
                    delay: 0,
                    mul_a: 3,
                    add_b: 2,
                },
                target: MultiViewTarget::Affine {
                    lag: 2,
                    mul_a: 3,
                    add_b: 1,
                },
            },
        ),
    ];

    for (label, width, cfg) in configs {
        let fault_eps = 1.0 - 1.0 / 2.0f64.powi(i32::from(width.bits()));
        let noise = NoiseModel::ChannelFaultSchedule {
            base_epsilon: 0.0,
            base_channels: Vec::new(),
            fault_tick,
            fault_epsilon: fault_eps,
            fault_channels: vec![0],
        };
        let pair = run_g2b5_fault_pair(width, &cfg, &noise, seed64, horizon, warmup, fault_tick);

        let mut combined_cliff = pair.cliff_true;
        combined_cliff.accumulate(&pair.cliff_false);

        let kt_adv = pair.kt_false_post_fault - pair.kt_true_post_fault;
        advantage_sum += kt_adv;
        pair_count += 1;

        let tel = &pair.tel_true;
        let ok = tel.redundancy_sprout_events >= 1
            && tel.comparator_disagreement_ticks >= 1
            && tel.driver_handoff_events >= 1
            && tel.disagreement_prune_events >= 1
            && pair.post_recovery_clean_mismatches == 0
            && kt_adv > 0.0;
        let msg = (!ok).then(|| {
            format!(
                "red={}, dis={}, handoff={}, dis_prune={}, post_rec_clean_mis={}, kt_adv={kt_adv:.4} (true={:.4}, false={:.4})",
                tel.redundancy_sprout_events,
                tel.comparator_disagreement_ticks,
                tel.driver_handoff_events,
                tel.disagreement_prune_events,
                pair.post_recovery_clean_mismatches,
                pair.kt_true_post_fault,
                pair.kt_false_post_fault
            )
        });
        tally.record(
            format!("G2B.5:{label}:seed={seed}"),
            ok,
            &combined_cliff,
            msg,
        );
    }

    G2b5SeedResult {
        tally,
        advantage_sum,
        pair_count,
    }
}

struct FaultPairStats {
    tel_true: crate::config::ArkheM1Telemetry,
    cliff_true: CliffCountersSummary,
    cliff_false: CliffCountersSummary,
    post_recovery_clean_mismatches: u64,
    kt_true_post_fault: f64,
    kt_false_post_fault: f64,
}

fn run_g2b5_fault_pair(
    width: Width,
    cfg: &GeneratorConfig,
    noise: &NoiseModel,
    seed: u64,
    horizon: u64,
    warmup: u64,
    fault_tick: u64,
) -> FaultPairStats {
    let trace_true = run_online_case(
        width,
        WireDelayMode::Combinational,
        None,
        cfg,
        noise,
        GrowthConfig::default(),
        seed,
        horizon,
        warmup,
        fault_tick,
        DEFAULT_M2B_BUDGET,
    );
    let trace_false = run_online_case(
        width,
        WireDelayMode::Combinational,
        None,
        cfg,
        noise,
        GrowthConfig {
            redundancy_sprouting: false,
            ..GrowthConfig::default()
        },
        seed,
        horizon,
        warmup,
        fault_tick,
        DEFAULT_M2B_BUDGET,
    );
    let eval_ticks = horizon.saturating_sub(fault_tick).max(1);
    FaultPairStats {
        tel_true: trace_true.telemetry,
        cliff_true: trace_true.cliff,
        cliff_false: trace_false.cliff,
        post_recovery_clean_mismatches: trace_true.post_recovery_clean_mismatches[2],
        kt_true_post_fault: facts_code_bits(
            width,
            eval_ticks,
            trace_true.post_fault_obs_mismatches[2],
        ) / (eval_ticks as f64),
        kt_false_post_fault: facts_code_bits(
            width,
            eval_ticks,
            trace_false.post_fault_obs_mismatches[2],
        ) / (eval_ticks as f64),
    }
}

// ---------------------------------------------------------------------------
// Single-bank & multi-bank verification orchestration
// ---------------------------------------------------------------------------

struct SeedAllGatesResult {
    g1: GateTally,
    g1_max_mdl_err: f64,
    g2: GateTally,
    g3: GateTally,
    g4: G2b4SeedResult,
    g5: G2b5SeedResult,
}

fn evaluate_all_gates_for_seed(seed: u32, horizon: u64, warmup: u64) -> SeedAllGatesResult {
    let (g1, g1_max_mdl_err) = evaluate_g2b1_seed(seed, horizon, warmup);
    let g2 = evaluate_g2b2_seed(seed, horizon, warmup);
    let g3 = evaluate_g2b3_seed(seed, horizon, warmup);
    let g4 = evaluate_g2b4_seed(seed, horizon, warmup);
    let g5 = evaluate_g2b5_seed(seed, horizon, warmup);
    SeedAllGatesResult {
        g1,
        g1_max_mdl_err,
        g2,
        g3,
        g4,
        g5,
    }
}

struct RawBankAggregate {
    seed_offset: u32,
    seeds_tested: u32,
    g1: GateTally,
    g1_max_mdl_err: f64,
    g2: GateTally,
    g3: GateTally,
    g4: GateTally,
    g4_kt_sum: f64,
    g4_h_bayes_class_sum: f64,
    g4_samples: usize,
    g5: GateTally,
    g5_adv_sum: f64,
    g5_pairs: usize,
}

fn run_single_bank_internal(seed_offset: u32, seeds: u32, horizon: u64) -> RawBankAggregate {
    let seeds_tested = seeds.max(1);
    let warmup = DEFAULT_M2B_WARMUP.min(horizon / 2).max(1);
    let n = seeds_tested as usize;
    let num_threads = std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(1)
        .min(n)
        .max(1);

    let next_idx = std::sync::atomic::AtomicUsize::new(0);
    let slots: Vec<std::sync::Mutex<Option<SeedAllGatesResult>>> =
        (0..n).map(|_| std::sync::Mutex::new(None)).collect();
    std::thread::scope(|s| {
        for _ in 0..num_threads {
            let next_ref = &next_idx;
            let slots_ref = &slots;
            s.spawn(move || {
                loop {
                    let idx = next_ref.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if idx >= n {
                        break;
                    }
                    let seed = seed_offset.saturating_add(idx as u32);
                    let res = evaluate_all_gates_for_seed(seed, horizon, warmup);
                    *slots_ref[idx].lock().expect("slot lock") = Some(res);
                }
            });
        }
    });

    let mut g1 = GateTally::default();
    let mut g1_max_mdl_err = 0.0f64;
    let mut g2 = GateTally::default();
    let mut g3 = GateTally::default();
    let mut g4 = GateTally::default();
    let mut g4_kt_sum = 0.0f64;
    let mut g4_h_bayes_class_sum = 0.0f64;
    let mut g4_samples = 0usize;
    let mut g5 = GateTally::default();
    let mut g5_adv_sum = 0.0f64;
    let mut g5_pairs = 0usize;

    for slot in slots {
        let res = slot
            .into_inner()
            .expect("slot lock")
            .expect("seed result computed");
        g1.merge(res.g1);
        g1_max_mdl_err = g1_max_mdl_err.max(res.g1_max_mdl_err);
        g2.merge(res.g2);
        g3.merge(res.g3);
        g4.merge(res.g4.tally);
        g4_kt_sum += res.g4.kt_sum;
        g4_h_bayes_class_sum += res.g4.h_bayes_class_sum;
        g4_samples += res.g4.sample_count;
        g5.merge(res.g5.tally);
        g5_adv_sum += res.g5.advantage_sum;
        g5_pairs += res.g5.pair_count;
    }

    RawBankAggregate {
        seed_offset,
        seeds_tested,
        g1,
        g1_max_mdl_err,
        g2,
        g3,
        g4,
        g4_kt_sum,
        g4_h_bayes_class_sum,
        g4_samples,
        g5,
        g5_adv_sum,
        g5_pairs,
    }
}

/// Runs Milestone 2b verification gates `G2B.1`–`G2B.5` on a single bank `[seed_offset, seed_offset + seeds)`.
///
/// # Errors
///
/// Returns an error if the seed range overlaps a forbidden holdout range or M2b holdout without `--holdout`.
pub fn run_m2b_verification_suite(
    seed_offset: u32,
    seeds: u32,
    horizon: u64,
    allow_holdout: bool,
) -> Result<M2bVerificationSummary, String> {
    check_m2b_seed_range_holdout(seed_offset, seeds, allow_holdout)?;
    let agg = run_single_bank_internal(seed_offset, seeds, horizon);

    let g4_pooled_abs_diff = if agg.g4_samples == 0 {
        f64::INFINITY
    } else {
        ((agg.g4_kt_sum - agg.g4_h_bayes_class_sum) / (agg.g4_samples as f64)).abs()
    };
    let g4_extra_ok = g4_pooled_abs_diff <= G2B4_KT_DIFF_MAX;

    let g5_pooled_adv = if agg.g5_pairs == 0 {
        f64::NEG_INFINITY
    } else {
        agg.g5_adv_sum / (agg.g5_pairs as f64)
    };
    let g5_extra_ok = g5_pooled_adv >= G2B5_POST_FAULT_KT_ADVANTAGE_MIN;

    let gates = vec![
        agg.g1.into_report(
            "G2B.1",
            G2B1_TARGET_RATE,
            agg.g1_max_mdl_err <= G2B1_EPISTEMIC_MDL_TOL_BITS_PER_SYMBOL,
            Some(format!(
                "max_epistemic_mdl_err={:.3e} b/sym",
                agg.g1_max_mdl_err
            )),
        ),
        agg.g2.into_report("G2B.2", G2B2_TARGET_RATE, true, None),
        agg.g3.into_report("G2B.3", G2B3_TARGET_RATE, true, None),
        agg.g4.into_report(
            "G2B.4",
            G2B4_TARGET_RATE,
            g4_extra_ok,
            Some(format!(
                "pooled_|E[KT]-H_bayes_class|={g4_pooled_abs_diff:.5} b/sym"
            )),
        ),
        agg.g5.into_report(
            "G2B.5",
            G2B5_TARGET_RATE,
            g5_extra_ok,
            Some(format!(
                "pooled_post_fault_kt_advantage={g5_pooled_adv:.5} b/sym"
            )),
        ),
    ];

    let mut total_cliff = CliffCountersSummary::default();
    let mut excluded_cases = 0usize;
    for g in &gates {
        total_cliff.accumulate(&g.cliff_counters);
        excluded_cases += g.excluded_cases;
    }
    let all_passed = gates.iter().all(|g| g.passed) && excluded_cases == 0;

    Ok(M2bVerificationSummary {
        provenance: BuildProvenance::current(),
        seed_offset: agg.seed_offset,
        seeds_tested: agg.seeds_tested,
        horizon,
        gates,
        all_passed,
        excluded_cases,
        cliff_counters: total_cliff,
        g2b1_max_epistemic_mdl_err: agg.g1_max_mdl_err,
        g2b4_pooled_abs_kt_minus_h_bayes_class: g4_pooled_abs_diff,
        g2b5_pooled_post_fault_kt_advantage: g5_pooled_adv,
    })
}

/// Evaluates `banks` consecutive seed banks on Milestone 2b gates `G2B.1`–`G2B.5` using
/// `evaluate_multi_bank_gate` (99% Clopper-Pearson lower bound + Pearson homogeneity).
///
/// # Errors
///
/// Returns an error if `banks == 0` or if the seed range overlaps a forbidden holdout range without permission.
pub fn run_m2b_bank_evaluation(
    seed_offset: u32,
    seeds_per_bank: u32,
    banks: u32,
    horizon: u64,
    allow_holdout: bool,
) -> Result<M2bBankReport, String> {
    if banks == 0 {
        return Err("banks count must be >= 1".to_owned());
    }
    let seeds_per_bank = seeds_per_bank.max(1);
    let total_seeds = seeds_per_bank.saturating_mul(banks);
    check_m2b_seed_range_holdout(seed_offset, total_seeds, allow_holdout)?;

    let mut bank_aggs = Vec::with_capacity(banks as usize);
    for b in 0..banks {
        let bank_offset = seed_offset.saturating_add(b.saturating_mul(seeds_per_bank));
        bank_aggs.push(run_single_bank_internal(
            bank_offset,
            seeds_per_bank,
            horizon,
        ));
    }

    let mut g1_samples = Vec::with_capacity(banks as usize);
    let mut g2_samples = Vec::with_capacity(banks as usize);
    let mut g3_samples = Vec::with_capacity(banks as usize);
    let mut g4_samples = Vec::with_capacity(banks as usize);
    let mut g5_samples = Vec::with_capacity(banks as usize);

    let mut g_excl = [0usize; 5];
    let mut g_failures: [Vec<String>; 5] = Default::default();
    let mut total_cliff = CliffCountersSummary::default();

    let mut max_mdl_err = 0.0f64;
    let mut g4_kt_sum = 0.0f64;
    let mut g4_h_sum = 0.0f64;
    let mut g4_count = 0usize;
    let mut g5_adv_sum = 0.0f64;
    let mut g5_pairs = 0usize;

    for (b_idx, agg) in bank_aggs.into_iter().enumerate() {
        max_mdl_err = max_mdl_err.max(agg.g1_max_mdl_err);
        g4_kt_sum += agg.g4_kt_sum;
        g4_h_sum += agg.g4_h_bayes_class_sum;
        g4_count += agg.g4_samples;
        g5_adv_sum += agg.g5_adv_sum;
        g5_pairs += agg.g5_pairs;

        g1_samples.push(agg.g1.bank_sample());
        g2_samples.push(agg.g2.bank_sample());
        g3_samples.push(agg.g3.bank_sample());
        g4_samples.push(agg.g4.bank_sample());
        g5_samples.push(agg.g5.bank_sample());

        let tallies = [agg.g1, agg.g2, agg.g3, agg.g4, agg.g5];
        for (gi, tally) in tallies.into_iter().enumerate() {
            g_excl[gi] += tally.excluded_cases;
            total_cliff.accumulate(&tally.cliff_counters);
            for f in tally.failures {
                if g_failures[gi].len() < 32 {
                    g_failures[gi].push(format!("bank {b_idx} (offset {}): {f}", agg.seed_offset));
                }
            }
        }
    }

    let g4_pooled_abs_diff = if g4_count == 0 {
        f64::INFINITY
    } else {
        ((g4_kt_sum - g4_h_sum) / (g4_count as f64)).abs()
    };
    let g5_pooled_adv = if g5_pairs == 0 {
        f64::NEG_INFINITY
    } else {
        g5_adv_sum / (g5_pairs as f64)
    };

    let mb1 = evaluate_multi_bank_gate(&g1_samples, G2B1_TARGET_RATE);
    let mb2 = evaluate_multi_bank_gate(&g2_samples, G2B2_TARGET_RATE);
    let mb3 = evaluate_multi_bank_gate(&g3_samples, G2B3_TARGET_RATE);
    let mb4 = evaluate_multi_bank_gate(&g4_samples, G2B4_TARGET_RATE);
    let mb5 = evaluate_multi_bank_gate(&g5_samples, G2B5_TARGET_RATE);

    let g1_extra_ok = max_mdl_err <= G2B1_EPISTEMIC_MDL_TOL_BITS_PER_SYMBOL;
    let g4_extra_ok = g4_pooled_abs_diff <= G2B4_KT_DIFF_MAX;
    let g5_extra_ok = g5_pooled_adv >= G2B5_POST_FAULT_KT_ADVANTAGE_MIN;

    let gates = vec![
        M2bBankGateReport {
            gate: "G2B.1".to_owned(),
            passed: mb1.passed && g_excl[0] == 0 && g1_extra_ok,
            excluded_cases: g_excl[0],
            multi_bank: mb1,
            pooled_metric_name: Some("max_epistemic_mdl_err_b_per_sym".to_owned()),
            pooled_metric_value: Some(max_mdl_err),
            pooled_metric_passed: g1_extra_ok,
            failures: std::mem::take(&mut g_failures[0]),
        },
        M2bBankGateReport {
            gate: "G2B.2".to_owned(),
            passed: mb2.passed && g_excl[1] == 0,
            excluded_cases: g_excl[1],
            multi_bank: mb2,
            pooled_metric_name: None,
            pooled_metric_value: None,
            pooled_metric_passed: true,
            failures: std::mem::take(&mut g_failures[1]),
        },
        M2bBankGateReport {
            gate: "G2B.3".to_owned(),
            passed: mb3.passed && g_excl[2] == 0,
            excluded_cases: g_excl[2],
            multi_bank: mb3,
            pooled_metric_name: None,
            pooled_metric_value: None,
            pooled_metric_passed: true,
            failures: std::mem::take(&mut g_failures[2]),
        },
        M2bBankGateReport {
            gate: "G2B.4".to_owned(),
            passed: mb4.passed && g_excl[3] == 0 && g4_extra_ok,
            excluded_cases: g_excl[3],
            multi_bank: mb4,
            pooled_metric_name: Some("pooled_abs_kt_minus_h_bayes_class_b_per_sym".to_owned()),
            pooled_metric_value: Some(g4_pooled_abs_diff),
            pooled_metric_passed: g4_extra_ok,
            failures: std::mem::take(&mut g_failures[3]),
        },
        M2bBankGateReport {
            gate: "G2B.5".to_owned(),
            passed: mb5.passed && g_excl[4] == 0 && g5_extra_ok,
            excluded_cases: g_excl[4],
            multi_bank: mb5,
            pooled_metric_name: Some("pooled_post_fault_kt_advantage_b_per_sym".to_owned()),
            pooled_metric_value: Some(g5_pooled_adv),
            pooled_metric_passed: g5_extra_ok,
            failures: std::mem::take(&mut g_failures[4]),
        },
    ];

    let excluded_cases: usize = g_excl.into_iter().sum();
    let all_passed = gates.iter().all(|g| g.passed) && excluded_cases == 0;

    Ok(M2bBankReport {
        provenance: BuildProvenance::current(),
        seed_offset,
        seeds_per_bank,
        banks,
        horizon,
        gates,
        all_passed,
        excluded_cases,
        cliff_counters: total_cliff,
        g2b1_max_epistemic_mdl_err: max_mdl_err,
        g2b4_pooled_abs_kt_minus_h_bayes_class: g4_pooled_abs_diff,
        g2b5_pooled_post_fault_kt_advantage: g5_pooled_adv,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holdout_guards_reject_m1_m2a_m2b_and_m3_ranges() {
        for blocked in [10_000u32, 30_000, 50_000, 60_000] {
            assert!(check_m2b_seed_range_holdout(blocked, 20, false).is_err());
            assert!(check_m2b_seed_range_holdout(blocked, 20, true).is_err());
            assert!(check_m2b_seed_range_holdout(blocked - 5, 10, true).is_err());
        }
        assert!(check_m2b_seed_range_holdout(40_000, 20, false).is_err());
        assert!(check_m2b_seed_range_holdout(39_990, 20, false).is_err());
        assert!(check_m2b_seed_range_holdout(40_000, 20, true).is_ok());

        for safe in [
            0u32, 9_980, 10_020, 20_040, 29_980, 30_020, 39_980, 40_020, 49_980, 50_020, 59_980,
            60_020,
        ] {
            assert!(
                check_m2b_seed_range_holdout(safe, 20, false).is_ok(),
                "expected safe seed range at {safe}"
            );
        }
    }

    #[test]
    fn g2b1a_epistemic_mdl_conservation_and_starvation_single_seed() {
        let max_err = check_epistemic_mdl_for_seed(0).expect("epistemic MDL conservation");
        assert!(max_err <= G2B1_EPISTEMIC_MDL_TOL_BITS_PER_SYMBOL);
        check_comparator_starvation_for_seed(0).expect("comparator starvation calibrated");
    }

    #[test]
    fn g2b3_loopback_consistency_and_falsification_single_seed() {
        let tally = evaluate_g2b3_seed(0, 1_200, 300);
        let report = tally.into_report("G2B.3", G2B3_TARGET_RATE, true, None);
        assert!(
            report.passed,
            "G2B.3 single seed failures: {:?}",
            report.failures
        );
    }

    #[test]
    fn g2b5_fault_recovery_paired_advantage_single_seed() {
        let res = evaluate_g2b5_seed(11, DEFAULT_M2B_HORIZON, DEFAULT_M2B_WARMUP);
        let adv = res.advantage_sum / (res.pair_count as f64);
        let report = res.tally.into_report("G2B.5", G2B5_TARGET_RATE, true, None);
        assert!(report.passed, "G2B.5 failures: {:?}", report.failures);
        assert!(adv >= G2B5_POST_FAULT_KT_ADVANTAGE_MIN, "adv={adv}");
    }
}
