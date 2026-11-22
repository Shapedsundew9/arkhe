//! Milestone 3 (`H4`) free-rider resistance and verified local excision verification module.
//!
//! - [`detours`]: synthetic free-rider library (`D0`–`D7b`) across widths $k \in \{1, 2\}$ and
//!   [`crate::substrate::WireDelayMode`] (`Combinational` & `TicksPerHop`).
//! - [`explore`]: exploratory and paired-ablation harness (`arkhe explore-m3`).
//! - Top-level confirmatory verification suite (`arkhe verify-m3`, gates `G3.1`–`G3.5`) with
//!   multi-bank Clopper–Pearson + homogeneity statistics and pre-registered holdout guards.

pub mod detours;
pub mod explore;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

pub use detours::{DetourClass, DetourEdit, build_detour_edit};
pub use explore::{ExploreM3Report, check_explore_m3_seed_range_holdout, run_explore_m3};

use crate::baselines::{
    DEFAULT_MAX_CERTIFY_JOINT_STEPS, PredictorBaseline, SynthesisCertStatus, TickBudget,
    certify_circuit_against_spec, exact_synthesize_certified_circuit,
};
use crate::config::{BuildProvenance, CliffCountersSummary};
use crate::env::{
    CrossLinearOp, CrossNonlinearOp, EpsilonMachineScope, FiniteStateGenerator, GeneratorConfig,
    LfsrStage, MultiViewDriver, MultiViewTarget, MultiViewTransform, NoiseModel, NoisyEnvironment,
    TierGenerator,
};
use crate::metrics::mdl::{
    EpistemicComparatorTrace, EpistemicTickPhase, epistemic_null_calibrated_mdl_delta_with_tax,
};
use crate::metrics::{circuit_code_bits_with_tax, facts_code_bits};
use crate::rng::SeedBank;
use crate::stats::{
    BankSample, BankTailMetricMap, BankTaskCountMap, CaseOutcome, GateRegime, HoldoutRangeSpec,
    MultiBankGateResult, StratifiedGateAudit, check_seed_range_holdouts, classify_case_outcome,
    evaluate_multi_bank_gate, evaluate_stratified_from_bank_maps, merge_tail_metric_map,
    percentile_sorted, strip_seed_suffix, update_max_finite, update_min_finite,
};
use crate::structural::graph_edit::source_has_cycle;
use crate::structural::growth::ExcisionMode;
use crate::structural::learner::InjectionMode;
use crate::structural::surgery::{primary_cone_nodes, primary_cone_nodes_without_comparator_taps};
use crate::structural::{
    ArkheLearner, EconomyConfig, EconomyState, EditableCircuit, GrowthConfig, ReserveCap,
    build_autonomous_ring, cone_channel_mask, materialize_const_loop, observation_horizon,
    splice_comparator_onto_port,
};
use crate::substrate::{
    Circuit, NodeId, Primitive, ScalarState, SignalSink, SignalSource, Width, WireDelayMode,
};

/// Pre-registered M1 holdout range start seed (unconditionally rejected in `verify-m3`).
pub const M1_HOLDOUT_SEED_START: u32 = 10_000;
/// Pre-registered M1 holdout range end seed (inclusive).
pub const M1_HOLDOUT_SEED_END: u32 = 10_019;
/// Pre-registered M2a holdout range start seed (unconditionally rejected in `verify-m3`).
pub const M2A_HOLDOUT_SEED_START: u32 = 30_000;
/// Pre-registered M2a holdout range end seed (inclusive).
pub const M2A_HOLDOUT_SEED_END: u32 = 30_019;
/// Pre-registered M2b holdout range start seed (unconditionally rejected in `verify-m3`).
pub const M2B_HOLDOUT_SEED_START: u32 = 40_000;
/// Pre-registered M2b holdout range end seed (inclusive).
pub const M2B_HOLDOUT_SEED_END: u32 = 40_019;
/// Pre-registered M3 v1 holdout range start seed (consumed at `dd13839`; permanently closed).
pub const M3_HOLDOUT_SEED_START: u32 = 50_000;
/// Pre-registered M3 v1 holdout range end seed (inclusive).
pub const M3_HOLDOUT_SEED_END: u32 = 50_019;
/// Pre-registered M3 v2 single-shot 100-seed holdout range start seed (`--holdout` required).
pub const M3_V2_HOLDOUT_SEED_START: u32 = 50_100;
/// Pre-registered M3 v2 single-shot 100-seed holdout range end seed (inclusive).
pub const M3_V2_HOLDOUT_SEED_END: u32 = 50_199;
/// Pre-registered M4a reserved holdout range start seed (100 seeds; unconditionally rejected in `verify-m3`).
pub const M4A_HOLDOUT_SEED_START: u32 = 60_000;
/// Pre-registered M4a reserved holdout range end seed (inclusive).
pub const M4A_HOLDOUT_SEED_END: u32 = 60_099;

/// Default maximum ticks to settle a primary circuit before detour injection.
pub const M3_MAX_SETTLE_TICKS: u64 = 2_000;
/// Default compute budget for injection verification runs.
pub const M3_INJECTION_TICK_BUDGET: u64 = 200_000;
/// Spontaneous intermediate horizon (`T = 2000`).
pub const M3_SPONTANEOUS_T2000: u64 = 2_000;
/// Spontaneous extended horizon (`T = 8000`).
pub const M3_SPONTANEOUS_T8000: u64 = 8_000;
/// Proportional compute budget for `T = 8000` spontaneous runs.
pub const M3_SPONTANEOUS_BUDGET_T8000: u64 = 800_000;
/// Warmup ticks excluded from post-warmup spontaneous KT averages.
pub const M3_SPONTANEOUS_WARMUP: u64 = 500;

/// Maximum epistemic MDL conservation residual in bits/symbol (`G3.1`).
pub const G31_CONSERVATION_TOL_BITS_PER_SYMBOL: f64 = 1e-11;
/// Maximum clean post-excision paired KT difference `|KT_learner - KT_oracle|` (`G3.2`).
pub const G32_CLEAN_EXCESS_MAX: f64 = 0.005;
/// Maximum raw unsubtracted finite-window KT loss over `N_post >= 3 * W_obs` on clean `G3.2`.
pub const G32_CLEAN_RAW_KT_MAX: f64 = 0.0075;
/// Maximum noisy paired KT difference vs structural-class Bayes oracle (`G3.2`).
pub const G32_NOISY_KT_DIFF_MAX: f64 = 0.040;
/// Maximum raw transient KT excess on `D2OneFire` over `4 * W_obs = 1024` ticks in `G3.3`.
/// Closed-form 1-error Dir(1/2) KT bound is `facts_code_bits(2, 1024, 1) / 1024 = 0.01798 <= 0.020`.
pub const G33_D2_TRANSIENT_KT_EXCESS_MAX: f64 = 0.020;
/// Maximum paired `|KT_learner - KT_oracle|` on `D2OneFire` over `4 * W_obs = 1024` ticks (`G3.3`).
pub const G33_D2_PAIRED_KT_DIFF_MAX: f64 = 0.015;
/// Analytical minimum `Split` pricing increment (bits per fan-out `Split`) in `G3.5(b)`.
pub const G35B_MIN_SPLIT_TAX_BITS: f64 = 6.58;

/// Pre-registered target rate for `G3.1` (exact 100%).
pub const G31_TARGET_RATE: f64 = 1.0;
/// Pre-registered target rate for `G3.2` (pooled 99% CP LB >= 0.90).
pub const G32_TARGET_RATE: f64 = 0.90;
/// Pre-registered target rate for `G3.3` (pooled 99% CP LB >= 0.90).
pub const G33_TARGET_RATE: f64 = 0.90;
/// Pre-registered target rate for `G3.4` (pooled 99% CP LB >= 0.90).
pub const G34_TARGET_RATE: f64 = 0.90;
/// Pre-registered target rate for `G3.5(a)` (pooled 99% CP LB >= 0.95).
pub const G35A_TARGET_RATE: f64 = 0.95;
/// Pre-registered target rate for `G3.5(b)` (exact 100%).
pub const G35B_TARGET_RATE: f64 = 1.0;

/// Guards `verify-m3` runs against pre-registered milestone holdout seed banks.
///
/// Unconditionally rejects overlaps with M1 (`10000..=10019`), closed M2a (`30000..=30019`),
/// closed M2b (`40000..=40019`), closed M3 v1 (`50000..=50019`), and reserved M4a
/// (`60000..=60099`) even when `allow_holdout == true`, and rejects fresh M3 v2 holdout
/// (`50100..=50199`) unless `allow_holdout == true`.
///
/// # Errors
///
/// Returns a descriptive error string if `[seed_offset, seed_offset + seeds)` overlaps a guarded range.
pub fn check_m3_seed_range_holdout(
    seed_offset: u32,
    seeds: u32,
    allow_holdout: bool,
) -> Result<(), String> {
    let ranges = [
        HoldoutRangeSpec {
            label: "M1",
            start: u64::from(M1_HOLDOUT_SEED_START),
            end_inclusive: u64::from(M1_HOLDOUT_SEED_END),
            allow_with_flag: false,
            status_note: "reserved M1 holdout",
        },
        HoldoutRangeSpec {
            label: "M2a",
            start: u64::from(M2A_HOLDOUT_SEED_START),
            end_inclusive: u64::from(M2A_HOLDOUT_SEED_END),
            allow_with_flag: false,
            status_note: "consumed at aa6fb6d; permanently closed",
        },
        HoldoutRangeSpec {
            label: "M2b",
            start: u64::from(M2B_HOLDOUT_SEED_START),
            end_inclusive: u64::from(M2B_HOLDOUT_SEED_END),
            allow_with_flag: false,
            status_note: "consumed at b211afa; permanently closed",
        },
        HoldoutRangeSpec {
            label: "M3",
            start: u64::from(M3_HOLDOUT_SEED_START),
            end_inclusive: u64::from(M3_HOLDOUT_SEED_END),
            allow_with_flag: false,
            status_note: "consumed at dd13839; permanently closed",
        },
        HoldoutRangeSpec {
            label: "M3_v2",
            start: u64::from(M3_V2_HOLDOUT_SEED_START),
            end_inclusive: u64::from(M3_V2_HOLDOUT_SEED_END),
            allow_with_flag: true,
            status_note: "fresh 100-seed M3 v2 holdout",
        },
        HoldoutRangeSpec {
            label: "M4a",
            start: u64::from(M4A_HOLDOUT_SEED_START),
            end_inclusive: u64::from(M4A_HOLDOUT_SEED_END),
            allow_with_flag: false,
            status_note: "reserved M4a holdout",
        },
    ];
    check_seed_range_holdouts(
        "verify-m3",
        u64::from(seed_offset),
        u64::from(seeds.max(1)),
        allow_holdout,
        &ranges,
    )
}

/// Summary report for one M3 gate in single-bank verification mode (`arkhe verify-m3`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M3GateReport {
    /// Gate identifier (`G3.1`, `G3.2`, `G3.3`, `G3.4`, `G3.5a`, `G3.5b`).
    pub gate: String,
    /// Overall pass/fail decision for this gate.
    pub passed: bool,
    /// Pre-registered target success rate (`1.0` denotes exact 100%).
    pub target_rate: f64,
    /// Count of functionally passing non-excluded cases.
    pub passed_cases: usize,
    /// Count of functionally failing non-excluded cases.
    pub failed_cases: usize,
    /// Count of excluded cases (`sprout_blocked_max_nodes > 0`).
    pub excluded_cases: usize,
    /// Total evaluated cases (`passed_cases + failed_cases + excluded_cases`).
    pub total_cases: usize,
    /// Empirical pass rate (`passed_cases / total_cases`).
    pub empirical_rate: f64,
    /// Accumulated cliff, bound, and guardrail telemetry counters.
    pub cliff_counters: CliffCountersSummary,
    /// Optional auxiliary metric summary for this gate.
    pub aux_metric_summary: Option<String>,
    /// Sample failure diagnostics recorded during evaluation.
    pub failures: Vec<String>,
    /// Statistical Policy Option A + D regime-stratified & per-task tail audit.
    pub stratified: StratifiedGateAudit,
}

/// Complete verification summary emitted by `arkhe verify-m3` in single-bank mode.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M3VerificationSummary {
    /// Compile-time binary and source provenance.
    pub provenance: BuildProvenance,
    /// Starting master seed offset.
    pub seed_offset: u32,
    /// Number of master seeds tested.
    pub seeds_tested: u32,
    /// Per-gate summaries for `G3.1`, `G3.2`, `G3.3`, `G3.4`, `G3.5a`, `G3.5b`.
    pub gates: Vec<M3GateReport>,
    /// True iff all gates pass and `excluded_cases == 0`.
    pub all_passed: bool,
    /// Total excluded cases across all gates.
    pub excluded_cases: usize,
    /// Total cliff counters across all gates.
    pub cliff_counters: CliffCountersSummary,
    /// Maximum epistemic MDL conservation residual (bits/symbol) observed in `G3.1`.
    pub g31_max_epistemic_mdl_err: f64,
    /// Maximum `Established` excision latency observed in `G3.2`.
    pub g32_max_excise_dt: Option<u64>,
    /// Maximum `Juvenile` excision latency observed in `G3.3`.
    pub g33_max_excise_dt: Option<u64>,
    /// Analytical minimum `Split` pricing delta (bits/Split) verified in `G3.5(b)`.
    pub g35b_min_analytical_split_tax_bits: f64,
}

/// Multi-bank gate report for one M3 gate (`arkhe verify-m3 --banks N`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M3BankGateReport {
    /// Gate identifier.
    pub gate: String,
    /// Overall multi-bank pass decision.
    pub passed: bool,
    /// Total excluded cases across all banks.
    pub excluded_cases: usize,
    /// Clopper-Pearson 99% lower bound + chi-squared/permutation homogeneity result.
    pub multi_bank: MultiBankGateResult,
    /// Optional auxiliary continuous metric name.
    pub pooled_metric_name: Option<String>,
    /// Optional auxiliary continuous metric value.
    pub pooled_metric_value: Option<f64>,
    /// Whether any auxiliary continuous check passed.
    pub pooled_metric_passed: bool,
    /// Sample failure strings recorded across banks.
    pub failures: Vec<String>,
    /// Statistical Policy Option A + D regime-stratified & per-task tail audit.
    pub stratified: StratifiedGateAudit,
}

/// Multi-bank Milestone 3 verification report (`arkhe verify-m3 --banks N`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M3BankReport {
    /// Compile-time binary and source provenance.
    pub provenance: BuildProvenance,
    /// Starting master seed offset.
    pub seed_offset: u32,
    /// Master seeds per bank.
    pub seeds_per_bank: u32,
    /// Number of consecutive banks evaluated.
    pub banks: u32,
    /// Multi-bank gate outcomes for `G3.1`, `G3.2`, `G3.3`, `G3.4`, `G3.5a`, `G3.5b`.
    pub gates: Vec<M3BankGateReport>,
    /// True iff all gates pass and `excluded_cases == 0`.
    pub all_passed: bool,
    /// Total excluded cases across all banks and gates.
    pub excluded_cases: usize,
    /// Accumulated cliff counters across all banks.
    pub cliff_counters: CliffCountersSummary,
    /// Maximum epistemic MDL conservation residual (bits/symbol) observed across all banks.
    pub g31_max_epistemic_mdl_err: f64,
    /// Maximum `Established` excision latency observed across all banks.
    pub g32_max_excise_dt: Option<u64>,
    /// Maximum `Juvenile` excision latency observed across all banks.
    pub g33_max_excise_dt: Option<u64>,
    /// Analytical minimum `Split` pricing delta (bits/Split) verified in `G3.5(b)`.
    pub g35b_min_analytical_split_tax_bits: f64,
}

#[derive(Clone, Debug, Default)]
struct GateTally {
    passed_cases: usize,
    failed_cases: usize,
    excluded_cases: usize,
    cliff_counters: CliffCountersSummary,
    failures: Vec<String>,
    task_counts: BTreeMap<(String, GateRegime), (usize, usize)>,
    task_tails: BTreeMap<String, BTreeMap<String, f64>>,
    task_dt_samples: BTreeMap<String, Vec<f64>>,
    task_n8k_samples: BTreeMap<String, Vec<f64>>,
    clean_seed_conj_succ: usize,
    clean_seed_conj_trials: usize,
}

impl GateTally {
    const fn total_cases(&self) -> usize {
        self.passed_cases + self.failed_cases + self.excluded_cases
    }

    fn record(
        &mut self,
        label: String,
        functional_pass: bool,
        cliff: &CliffCountersSummary,
        detail: Option<String>,
    ) {
        self.record_with_regime(label, GateRegime::Clean, functional_pass, cliff, detail);
    }

    fn record_with_regime(
        &mut self,
        label: String,
        regime: GateRegime,
        functional_pass: bool,
        cliff: &CliffCountersSummary,
        detail: Option<String>,
    ) {
        self.cliff_counters.accumulate(cliff);
        let outcome = classify_case_outcome(functional_pass, cliff);
        let task_key = (strip_seed_suffix(&label).to_owned(), regime);
        let entry = self.task_counts.entry(task_key).or_insert((0, 0));
        entry.1 += 1;
        match outcome {
            CaseOutcome::Passed => {
                self.passed_cases += 1;
                entry.0 += 1;
            }
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

    fn record_tail_max(&mut self, task_label: &str, key: &str, value: f64) {
        let map = self.task_tails.entry(task_label.to_owned()).or_default();
        update_max_finite(map, key, value);
    }

    fn record_tail_min(&mut self, task_label: &str, key: &str, value: f64) {
        let map = self.task_tails.entry(task_label.to_owned()).or_default();
        update_min_finite(map, key, value);
    }

    fn record_dt_samples(&mut self, task_label: &str, dts: &[f64]) {
        if dts.is_empty() {
            return;
        }
        let buf = self
            .task_dt_samples
            .entry(task_label.to_owned())
            .or_default();
        let mut max_dt = f64::NEG_INFINITY;
        for &dt in dts {
            if dt.is_finite() {
                buf.push(dt);
                if dt > max_dt {
                    max_dt = dt;
                }
            }
        }
        if max_dt.is_finite() {
            self.record_tail_max(task_label, "max_excise_dt", max_dt);
        }
    }

    fn record_n8k_sample(&mut self, task_label: &str, value: f64) {
        if value.is_finite() {
            self.task_n8k_samples
                .entry(task_label.to_owned())
                .or_default()
                .push(value);
        }
    }

    fn clean_seed_passed(&self) -> Option<bool> {
        let mut has_clean = false;
        let mut all_ok = self.excluded_cases == 0;
        for ((_, regime), &(succ, trials)) in &self.task_counts {
            if *regime == GateRegime::Clean && trials > 0 {
                has_clean = true;
                if succ != trials {
                    all_ok = false;
                }
            }
        }
        has_clean.then_some(all_ok)
    }

    fn clean_seed_bank(&self) -> Option<Vec<BankSample>> {
        if self.clean_seed_conj_trials > 0 {
            Some(vec![BankSample::new(
                self.clean_seed_conj_succ,
                self.clean_seed_conj_trials,
            )])
        } else {
            self.clean_seed_passed()
                .map(|ok| vec![BankSample::new(usize::from(ok), 1)])
        }
    }

    fn merge_seed(&mut self, other: Self) {
        if let Some(ok) = other.clean_seed_passed() {
            self.clean_seed_conj_trials += 1;
            if ok {
                self.clean_seed_conj_succ += 1;
            }
        }
        self.passed_cases += other.passed_cases;
        self.failed_cases += other.failed_cases;
        self.excluded_cases += other.excluded_cases;
        self.cliff_counters.accumulate(&other.cliff_counters);
        self.failures.extend(other.failures);
        for (k, (s, n)) in other.task_counts {
            let entry = self.task_counts.entry(k).or_insert((0, 0));
            entry.0 += s;
            entry.1 += n;
        }
        for (task, tails) in other.task_tails {
            let entry = self.task_tails.entry(task).or_default();
            merge_tail_metric_map(entry, &tails);
        }
        for (task, dts) in other.task_dt_samples {
            self.task_dt_samples.entry(task).or_default().extend(dts);
        }
        for (task, vals) in other.task_n8k_samples {
            self.task_n8k_samples.entry(task).or_default().extend(vals);
        }
    }

    const fn bank_sample(&self) -> BankSample {
        BankSample::new(self.passed_cases, self.total_cases())
    }

    fn into_report(
        mut self,
        gate: &str,
        target_rate: f64,
        extra_ok: bool,
        aux_metric_summary: Option<String>,
    ) -> M3GateReport {
        self.failures.sort();
        if self.failures.len() > 64 {
            let total = self.failures.len();
            self.failures.truncate(64);
            self.failures.push(format!(
                "... ({} additional failures truncated)",
                total - 64
            ));
        }
        let overrides =
            build_m3_percentile_overrides(&self.task_dt_samples, &self.task_n8k_samples);
        let clean_conj = self.clean_seed_bank();
        let stratified = evaluate_stratified_from_bank_maps(
            &[&self.task_counts],
            &[&self.task_tails],
            Some(&overrides),
            clean_conj.as_deref(),
            target_rate,
            target_rate,
            false,
        );
        let total = self.total_cases();
        let empirical_rate = if total == 0 {
            0.0
        } else {
            (self.passed_cases as f64) / (total as f64)
        };
        let passed = if (target_rate - 1.0).abs() <= 1e-12 {
            total > 0
                && self.excluded_cases == 0
                && self.passed_cases == total
                && extra_ok
                && stratified.passed
        } else {
            total > 0
                && self.excluded_cases == 0
                && empirical_rate >= target_rate
                && extra_ok
                && stratified.passed
        };
        M3GateReport {
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
            stratified,
        }
    }
}

fn build_m3_percentile_overrides(
    dt_samples: &BTreeMap<String, Vec<f64>>,
    n8k_samples: &BTreeMap<String, Vec<f64>>,
) -> BTreeMap<String, BTreeMap<String, f64>> {
    let mut out: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();
    for (task, raw) in dt_samples {
        if !raw.is_empty() {
            let mut sorted = raw.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            out.entry(task.clone())
                .or_default()
                .insert("p95_excise_dt".to_owned(), percentile_sorted(&sorted, 0.95));
        }
    }
    for (task, raw) in n8k_samples {
        if !raw.is_empty() {
            let mut sorted = raw.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            out.entry(task.clone()).or_default().insert(
                "p95_n_primary_8000".to_owned(),
                percentile_sorted(&sorted, 0.95),
            );
        }
    }
    out
}

#[derive(Clone)]
struct TaskSpec {
    label: &'static str,
    tier: &'static str,
    width: Width,
    target_port: u16,
    is_multiview: bool,
    is_autonomous_t1: bool,
    tph_ring_motif: Option<&'static [u64]>,
    comb_cfg: GeneratorConfig,
    tph_cfg: Option<GeneratorConfig>,
    noise_channels: Vec<u16>,
    split_tax_ablate_inj: bool,
}

fn d0_lfsr() -> LfsrStage {
    LfsrStage {
        taps: vec![0, 1, 0, 0, 1],
        init: vec![1, 1, 0, 1, 0],
    }
}

fn d1_lfsr() -> LfsrStage {
    LfsrStage {
        taps: vec![1, 0, 0, 1],
        init: vec![1, 0, 1, 1],
    }
}

fn d11_lfsr() -> LfsrStage {
    let mut taps = vec![0u64; 11];
    taps[8] = 1;
    taps[10] = 1;
    LfsrStage {
        taps,
        init: vec![1, 0, 1, 1, 0, 1, 0, 1, 1, 0, 1],
    }
}

fn injection_task_panel() -> Vec<TaskSpec> {
    let d0 = d0_lfsr();
    let d1 = d1_lfsr();
    let d11 = d11_lfsr();
    vec![
        TaskSpec {
            label: "T1_Alternating(k=1)",
            tier: "T1",
            width: Width::BITS_1,
            target_port: 0,
            is_multiview: false,
            is_autonomous_t1: true,
            tph_ring_motif: Some(&[0, 1]),
            comb_cfg: GeneratorConfig::T1_Alternating { a: 0, b: 1 },
            tph_cfg: Some(GeneratorConfig::T1_Alternating { a: 0, b: 1 }),
            noise_channels: vec![0],
            split_tax_ablate_inj: true,
        },
        TaskSpec {
            label: "T1_Alternating(k=2)",
            tier: "T1",
            width: Width::BITS_2,
            target_port: 0,
            is_multiview: false,
            is_autonomous_t1: true,
            tph_ring_motif: Some(&[0, 3]),
            comb_cfg: GeneratorConfig::T1_Alternating { a: 0, b: 3 },
            tph_cfg: Some(GeneratorConfig::T1_Alternating { a: 0, b: 3 }),
            noise_channels: vec![0],
            split_tax_ablate_inj: true,
        },
        TaskSpec {
            label: "T1_PeriodicMotif([0,1,1],k=1)",
            tier: "T1",
            width: Width::BITS_1,
            target_port: 0,
            is_multiview: false,
            is_autonomous_t1: true,
            tph_ring_motif: Some(&[0, 1, 1]),
            comb_cfg: GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 1, 1],
            },
            tph_cfg: Some(GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 1, 1],
            }),
            noise_channels: vec![0],
            split_tax_ablate_inj: true,
        },
        TaskSpec {
            label: "T1_PeriodicMotif([0,1,3],k=2)",
            tier: "T1",
            width: Width::BITS_2,
            target_port: 0,
            is_multiview: false,
            is_autonomous_t1: true,
            tph_ring_motif: Some(&[0, 1, 3]),
            comb_cfg: GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 1, 3],
            },
            tph_cfg: Some(GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 1, 3],
            }),
            noise_channels: vec![0],
            split_tax_ablate_inj: true,
        },
        TaskSpec {
            label: "T3b_CounterModN(3,k=2)",
            tier: "T1",
            width: Width::BITS_2,
            target_port: 0,
            is_multiview: false,
            is_autonomous_t1: true,
            tph_ring_motif: Some(&[0, 1, 2]),
            comb_cfg: GeneratorConfig::T3b_CounterModN {
                modulus: 3,
                init: 0,
            },
            tph_cfg: Some(GeneratorConfig::T3b_CounterModN {
                modulus: 3,
                init: 0,
            }),
            noise_channels: vec![0],
            split_tax_ablate_inj: false,
        },
        TaskSpec {
            label: "T1_AffineLag1(1,1,0,k=2)",
            tier: "T1",
            width: Width::BITS_2,
            target_port: 0,
            is_multiview: false,
            is_autonomous_t1: true,
            tph_ring_motif: Some(&[0, 1, 2, 3]),
            comb_cfg: GeneratorConfig::T1_AffineLag1 {
                mul_a: 1,
                add_b: 1,
                init: 0,
            },
            tph_cfg: Some(GeneratorConfig::T1_AffineLag1 {
                mul_a: 1,
                add_b: 1,
                init: 0,
            }),
            noise_channels: vec![0],
            split_tax_ablate_inj: false,
        },
        TaskSpec {
            label: "T2_CrossSum(k=1)",
            tier: "T2",
            width: Width::BITS_1,
            target_port: 2,
            is_multiview: false,
            is_autonomous_t1: false,
            tph_ring_motif: None,
            comb_cfg: GeneratorConfig::T2_CrossLinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossLinearOp::Add,
            },
            tph_cfg: Some(GeneratorConfig::T2_CrossLinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 3,
                lag_1: 3,
                op: CrossLinearOp::Add,
            }),
            noise_channels: vec![0, 1, 2],
            split_tax_ablate_inj: false,
        },
        TaskSpec {
            label: "T2_CrossSum(k=2)",
            tier: "T2",
            width: Width::BITS_2,
            target_port: 2,
            is_multiview: false,
            is_autonomous_t1: false,
            tph_ring_motif: None,
            comb_cfg: GeneratorConfig::T2_CrossLinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossLinearOp::Add,
            },
            tph_cfg: Some(GeneratorConfig::T2_CrossLinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 3,
                lag_1: 3,
                op: CrossLinearOp::Add,
            }),
            noise_channels: vec![0, 1, 2],
            split_tax_ablate_inj: true,
        },
        TaskSpec {
            label: "T2_CrossMulEqz(k=2)",
            tier: "T2",
            width: Width::BITS_2,
            target_port: 2,
            is_multiview: false,
            is_autonomous_t1: false,
            tph_ring_motif: None,
            comb_cfg: GeneratorConfig::T2_CrossNonlinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossNonlinearOp::MulEqz,
            },
            tph_cfg: Some(GeneratorConfig::T2_CrossNonlinear {
                driver_0: d0,
                driver_1: d1,
                lag_0: 3,
                lag_1: 4,
                op: CrossNonlinearOp::MulEqz,
            }),
            noise_channels: vec![2],
            split_tax_ablate_inj: true,
        },
        TaskSpec {
            label: "T2_MultiViewCoupled(k=1)",
            tier: "T2_MultiViewCoupled",
            width: Width::BITS_1,
            target_port: 2,
            is_multiview: true,
            is_autonomous_t1: false,
            tph_ring_motif: None,
            comb_cfg: GeneratorConfig::T2_MultiViewCoupled {
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
            tph_cfg: Some(GeneratorConfig::T2_MultiViewCoupled {
                driver: MultiViewDriver::Lfsr { stage: d11.clone() },
                view_0: MultiViewTransform {
                    delay: 1,
                    mul_a: 1,
                    add_b: 0,
                },
                view_1: MultiViewTransform {
                    delay: 1,
                    mul_a: 1,
                    add_b: 1,
                },
                target: MultiViewTarget::Affine {
                    lag: 4,
                    mul_a: 1,
                    add_b: 0,
                },
            }),
            noise_channels: vec![0, 1, 2],
            split_tax_ablate_inj: true,
        },
        TaskSpec {
            label: "T2_MultiViewCoupled(k=2)",
            tier: "T2_MultiViewCoupled",
            width: Width::BITS_2,
            target_port: 2,
            is_multiview: true,
            is_autonomous_t1: false,
            tph_ring_motif: None,
            comb_cfg: GeneratorConfig::T2_MultiViewCoupled {
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
            tph_cfg: Some(GeneratorConfig::T2_MultiViewCoupled {
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
            }),
            noise_channels: vec![0, 1, 2],
            split_tax_ablate_inj: true,
        },
    ]
}

#[derive(Clone)]
struct SpontaneousSpec {
    label: &'static str,
    tier: &'static str,
    width: Width,
    target_port: u16,
    wire_delay: WireDelayMode,
    epsilon: f64,
    noise_channels: Vec<u16>,
    gen_cfg: GeneratorConfig,
}

fn spontaneous_panel() -> Vec<SpontaneousSpec> {
    let d0 = d0_lfsr();
    let mut specs = Vec::new();

    for width in [Width::BITS_1, Width::BITS_2] {
        specs.push(SpontaneousSpec {
            label: if width == Width::BITS_1 {
                "T1_Constant(0,k=1)"
            } else {
                "T1_Constant(0,k=2)"
            },
            tier: "T1",
            width,
            target_port: 0,
            wire_delay: WireDelayMode::Combinational,
            epsilon: 0.0,
            noise_channels: Vec::new(),
            gen_cfg: GeneratorConfig::T1_Constant { value: 0 },
        });
        specs.push(SpontaneousSpec {
            label: if width == Width::BITS_1 {
                "T1_NoisyConstant(0,k=1)"
            } else {
                "T1_NoisyConstant(0,k=2)"
            },
            tier: "T1",
            width,
            target_port: 0,
            wire_delay: WireDelayMode::Combinational,
            epsilon: 0.05,
            noise_channels: vec![0],
            gen_cfg: GeneratorConfig::T1_Constant { value: 0 },
        });
        specs.push(SpontaneousSpec {
            label: if width == Width::BITS_1 {
                "T1_NoisyConstant(max,k=1)"
            } else {
                "T1_NoisyConstant(max,k=2)"
            },
            tier: "T1",
            width,
            target_port: 0,
            wire_delay: WireDelayMode::Combinational,
            epsilon: 0.05,
            noise_channels: vec![0],
            gen_cfg: GeneratorConfig::T1_Constant {
                value: width.mask(),
            },
        });
    }

    for task in injection_task_panel() {
        for &wire_delay in &[WireDelayMode::Combinational, WireDelayMode::TicksPerHop] {
            let cfg = match wire_delay {
                WireDelayMode::Combinational => task.comb_cfg.clone(),
                WireDelayMode::TicksPerHop => {
                    let Some(c) = task.tph_cfg.clone() else {
                        continue;
                    };
                    c
                }
            };
            specs.push(SpontaneousSpec {
                label: task.label,
                tier: task.tier,
                width: task.width,
                target_port: task.target_port,
                wire_delay,
                epsilon: 0.0,
                noise_channels: Vec::new(),
                gen_cfg: cfg,
            });
        }
        if !task.is_autonomous_t1 || task.label.starts_with("T1_Alternating") {
            specs.push(SpontaneousSpec {
                label: task.label,
                tier: task.tier,
                width: task.width,
                target_port: task.target_port,
                wire_delay: WireDelayMode::Combinational,
                epsilon: 0.05,
                noise_channels: task.noise_channels.clone(),
                gen_cfg: task.comb_cfg.clone(),
            });
        }
    }

    specs.push(SpontaneousSpec {
        label: "T2_DelayedAffine(d=2,k=2)",
        tier: "T2",
        width: Width::BITS_2,
        target_port: 1,
        wire_delay: WireDelayMode::Combinational,
        epsilon: 0.0,
        noise_channels: Vec::new(),
        gen_cfg: GeneratorConfig::T2_DelayedAffine {
            driver: d0.clone(),
            lag: 2,
            mul_a: 3,
            add_b: 1,
        },
    });
    specs.push(SpontaneousSpec {
        label: "T2_DelayedAffine(d=3,k=2)",
        tier: "T2",
        width: Width::BITS_2,
        target_port: 1,
        wire_delay: WireDelayMode::TicksPerHop,
        epsilon: 0.0,
        noise_channels: Vec::new(),
        gen_cfg: GeneratorConfig::T2_DelayedAffine {
            driver: d0,
            lag: 3,
            mul_a: 3,
            add_b: 1,
        },
    });
    specs
}

fn build_noise(epsilon: f64, channels: &[u16]) -> NoiseModel {
    if epsilon <= 0.0 || channels.is_empty() {
        NoiseModel::None
    } else {
        NoiseModel::IndependentSubstitution {
            epsilon,
            channels: channels.to_vec(),
        }
    }
}

fn build_causal_split_tph_editable(
    width: Width,
    econ: EconomyConfig,
    growth: &GrowthConfig,
) -> EditableCircuit {
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
    let mut editable =
        EditableCircuit::new_with_growth(c, econ, growth).expect("valid split-primary editable");
    if let Some(ne) = editable.economy_mut().node_mut(split.index()) {
        ne.juvenile_ticks = econ.juvenile_ticks;
    }
    editable
}

fn build_autonomous_ring_tph_editable(
    width: Width,
    motif: &[u64],
    econ: EconomyConfig,
    growth: &GrowthConfig,
) -> EditableCircuit {
    let mut c = Circuit::new(width, 1, 1, WireDelayMode::TicksPerHop);
    build_autonomous_ring(&mut c, motif.len(), motif, 0);
    let mut ed = EditableCircuit::new_with_growth(c, econ, growth).expect("valid autonomous ring");
    for node_econ in ed.economy_mut().nodes_mut() {
        node_econ.juvenile_ticks = 0;
        node_econ.debt_paid = true;
    }
    let mut out = vec![0u64; 1];
    for _ in 0..24 {
        ed.step(&[0], &mut out).expect("tph warmup");
    }
    ed
}

fn target_active_comparator_count(economy: &EconomyState, target_port: u16) -> usize {
    economy
        .comparators()
        .iter()
        .filter(|c| c.target_port == target_port)
        .count()
}

fn max_comparators_on_any_port(circuit: &Circuit, economy: &EconomyState) -> usize {
    let mut counts = vec![0usize; usize::from(circuit.num_outputs())];
    for comp in economy.comparators() {
        let idx = usize::from(comp.target_port);
        if idx < counts.len() {
            counts[idx] += 1;
        }
    }
    counts.into_iter().max().unwrap_or(0)
}

fn comparator_cone_set(
    circuit: &Circuit,
    economy: &EconomyState,
    target_port: Option<u16>,
) -> BTreeSet<NodeId> {
    let comps = economy.comparators();
    if comps.is_empty() {
        return BTreeSet::new();
    }
    let mut boundary_live = BTreeSet::new();
    let mut stack = Vec::new();
    for edge in circuit.edges() {
        if let SignalSink::BoundaryOut(_) = edge.dst
            && let SignalSource::NodeOut { node, .. } = edge.src
            && boundary_live.insert(node)
        {
            stack.push(node);
        }
    }
    while let Some(node) = stack.pop() {
        for edge in circuit.edges() {
            if let SignalSink::NodeIn { node: dst, .. } = edge.dst
                && dst == node
                && let SignalSource::NodeOut { node: src, .. } = edge.src
                && boundary_live.insert(src)
            {
                stack.push(src);
            }
        }
    }
    let mut comp_nodes = BTreeSet::new();
    let mut c_stack: Vec<NodeId> = comps
        .iter()
        .filter(|c| target_port.is_none_or(|tp| c.target_port == tp))
        .flat_map(|c| [c.sub_node, c.eqz_node])
        .collect();
    for &n in &c_stack {
        comp_nodes.insert(n);
    }
    while let Some(node) = c_stack.pop() {
        for edge in circuit.edges() {
            if let SignalSink::NodeIn { node: dst, .. } = edge.dst
                && dst == node
                && let SignalSource::NodeOut { node: src, .. } = edge.src
                && !boundary_live.contains(&src)
                && comp_nodes.insert(src)
            {
                c_stack.push(src);
            }
        }
    }
    comp_nodes
}

fn target_cone_signature(
    circuit: &Circuit,
    economy: &EconomyState,
    target_port: u16,
) -> (Vec<(NodeId, Primitive)>, usize) {
    target_cone_signature_inner(circuit, economy, target_port, false)
}

fn target_cone_signature_without_comparator_taps(
    circuit: &Circuit,
    economy: &EconomyState,
    target_port: u16,
) -> (Vec<(NodeId, Primitive)>, usize) {
    target_cone_signature_inner(circuit, economy, target_port, true)
}

fn target_cone_signature_inner(
    circuit: &Circuit,
    economy: &EconomyState,
    target_port: u16,
    exclude_comparator_taps: bool,
) -> (Vec<(NodeId, Primitive)>, usize) {
    let pri = if exclude_comparator_taps {
        primary_cone_nodes_without_comparator_taps(circuit, economy, target_port)
    } else {
        primary_cone_nodes(circuit, economy, target_port)
    };
    let comp = comparator_cone_set(circuit, economy, Some(target_port));
    let mut nodes: Vec<(NodeId, Primitive)> = pri
        .union(&comp)
        .filter_map(|&id| circuit.nodes().get(id.index()).map(|&p| (id, p)))
        .collect();
    nodes.sort_by_key(|(id, _)| id.index());
    (nodes, target_active_comparator_count(economy, target_port))
}

fn target_cone_all_mature(circuit: &Circuit, economy: &EconomyState, target_port: u16) -> bool {
    let pri = primary_cone_nodes(circuit, economy, target_port);
    let comp = comparator_cone_set(circuit, economy, Some(target_port));
    let econ_nodes = economy.nodes();
    pri.union(&comp).all(|id| {
        econ_nodes
            .get(id.index())
            .is_none_or(|n| n.juvenile_ticks == 0)
    })
}

fn port_is_driven(circuit: &Circuit, port: u16) -> bool {
    circuit
        .edges()
        .iter()
        .any(|e| e.dst == SignalSink::BoundaryOut(port))
}

fn dead_or_unregistered_nodes(circuit: &Circuit, economy: &EconomyState) -> BTreeSet<NodeId> {
    let comp_set = comparator_cone_set(circuit, economy, None);
    let mut live_boundary = BTreeSet::new();
    let mut stack = Vec::new();
    for edge in circuit.edges() {
        if let SignalSink::BoundaryOut(_) = edge.dst
            && let SignalSource::NodeOut { node, .. } = edge.src
            && live_boundary.insert(node)
        {
            stack.push(node);
        }
    }
    while let Some(node) = stack.pop() {
        for edge in circuit.edges() {
            if let SignalSink::NodeIn { node: dst, .. } = edge.dst
                && dst == node
                && let SignalSource::NodeOut { node: src, .. } = edge.src
                && live_boundary.insert(src)
            {
                stack.push(src);
            }
        }
    }
    (0..circuit.node_count())
        .map(|i| NodeId(u32::try_from(i).expect("fits")))
        .filter(|n| !live_boundary.contains(n) && !comp_set.contains(n))
        .collect()
}

fn target_primary_and_dead_counts(
    circuit: &Circuit,
    economy: &EconomyState,
    target_port: u16,
    split_code_tax: bool,
) -> (usize, usize, f64) {
    let pri = primary_cone_nodes(circuit, economy, target_port);
    let dead = dead_or_unregistered_nodes(circuit, economy);
    let keep: BTreeSet<NodeId> = pri.union(&dead).copied().collect();
    let to_remove: BTreeSet<NodeId> = (0..circuit.node_count())
        .map(|i| NodeId(u32::try_from(i).expect("fits")))
        .filter(|n| !keep.contains(n))
        .collect();
    let bits = if to_remove.is_empty() {
        circuit_code_bits_with_tax(circuit, 0, split_code_tax)
    } else {
        let mut stripped = circuit.clone();
        stripped.remove_nodes(&to_remove);
        circuit_code_bits_with_tax(&stripped, 0, split_code_tax)
    };
    (pri.len(), dead.len(), bits)
}

/// Paired structural-class Bayes oracle for `G3.2` noisy evaluation across `T1`, `T2`, and
/// `T2_MultiViewCoupled`.
#[derive(Clone)]
struct M3BayesOracle {
    width: Width,
    channels: usize,
    target_ch: usize,
    mode: M3OracleMode,
    history: Vec<VecDeque<u64>>,
}

#[derive(Clone, Copy)]
enum M3OracleMode {
    AutonomousCleanTarget,
    CrossLinear {
        lag_0: usize,
        lag_1: usize,
        op: CrossLinearOp,
    },
    CrossNonlinear {
        lag_0: usize,
        lag_1: usize,
        op: CrossNonlinearOp,
    },
    MultiView {
        view_0: MultiViewTransform,
        target: MultiViewTarget,
        tick: u64,
    },
}

impl M3BayesOracle {
    fn from_task(task: &TaskSpec, gen_cfg: &GeneratorConfig, channels: usize) -> Self {
        let mode = if task.is_autonomous_t1 {
            M3OracleMode::AutonomousCleanTarget
        } else {
            match gen_cfg {
                GeneratorConfig::T2_CrossLinear {
                    lag_0, lag_1, op, ..
                } => M3OracleMode::CrossLinear {
                    lag_0: *lag_0,
                    lag_1: *lag_1,
                    op: *op,
                },
                GeneratorConfig::T2_CrossNonlinear {
                    lag_0, lag_1, op, ..
                } => M3OracleMode::CrossNonlinear {
                    lag_0: *lag_0,
                    lag_1: *lag_1,
                    op: *op,
                },
                GeneratorConfig::T2_MultiViewCoupled { view_0, target, .. } => {
                    M3OracleMode::MultiView {
                        view_0: view_0.normalized(task.width),
                        target: *target,
                        tick: 0,
                    }
                }
                _ => M3OracleMode::AutonomousCleanTarget,
            }
        };
        Self {
            width: task.width,
            channels,
            target_ch: usize::from(task.target_port),
            mode,
            history: vec![VecDeque::with_capacity(64); channels],
        }
    }

    fn recover_z_at_lag(&self, v0: MultiViewTransform, z_lag: usize, tick: u64) -> u64 {
        let t = tick as usize;
        if t < z_lag {
            return 0;
        }
        let obs_lag = z_lag - v0.delay;
        let len = self.history[0].len();
        if obs_lag == 0 || obs_lag > len {
            return 0;
        }
        v0.invert_z(self.width, self.history[0][len - obs_lag])
    }

    fn predict_target(&self, clean_frame: &[u64]) -> u64 {
        let hist_at = |ch: usize, lag: usize| -> u64 {
            let len = self.history[ch].len();
            if lag == 0 || lag > len {
                0
            } else {
                self.history[ch][len - lag]
            }
        };
        match &self.mode {
            M3OracleMode::AutonomousCleanTarget => clean_frame[self.target_ch],
            M3OracleMode::CrossLinear { lag_0, lag_1, op } => {
                let x0 = hist_at(0, *lag_0);
                let x1 = hist_at(1, *lag_1);
                match op {
                    CrossLinearOp::Add => self.width.add(x0, x1),
                    CrossLinearOp::Sub => self.width.sub(x0, x1),
                }
            }
            M3OracleMode::CrossNonlinear { lag_0, lag_1, op } => {
                let x0 = hist_at(0, *lag_0);
                let x1 = hist_at(1, *lag_1);
                match op {
                    CrossNonlinearOp::Mul => self.width.mul(x0, x1),
                    CrossNonlinearOp::MulEqz => self.width.mul(x0, self.width.eqz(x1)),
                    CrossNonlinearOp::AddEqz => self.width.add(x0, self.width.eqz(x1)),
                    CrossNonlinearOp::MulEqzPair => {
                        self.width.mul(self.width.eqz(x0), self.width.eqz(x1))
                    }
                }
            }
            M3OracleMode::MultiView {
                view_0,
                target,
                tick,
            } => {
                let pred = match *target {
                    MultiViewTarget::Affine { lag, mul_a, add_b } => {
                        let z = self.recover_z_at_lag(*view_0, lag, *tick);
                        self.width.add(self.width.mul(mul_a, z), add_b)
                    }
                    MultiViewTarget::Eqz { lag, add_b } => {
                        let z = self.recover_z_at_lag(*view_0, lag, *tick);
                        self.width.eqz(self.width.add(z, add_b))
                    }
                    MultiViewTarget::AddEqz { lag_a, lag_b } => {
                        let za = self.recover_z_at_lag(*view_0, lag_a, *tick);
                        let zb = self.recover_z_at_lag(*view_0, lag_b, *tick);
                        self.width.add(za, self.width.eqz(zb))
                    }
                    MultiViewTarget::MulEqz { lag_a, lag_b } => {
                        let za = self.recover_z_at_lag(*view_0, lag_a, *tick);
                        let zb = self.recover_z_at_lag(*view_0, lag_b, *tick);
                        self.width.mul(za, self.width.eqz(zb))
                    }
                    MultiViewTarget::MulEqzPair { lag_a, lag_b } => {
                        let za = self.recover_z_at_lag(*view_0, lag_a, *tick);
                        let zb = self.recover_z_at_lag(*view_0, lag_b, *tick);
                        self.width.mul(self.width.eqz(za), self.width.eqz(zb))
                    }
                };
                self.width.normalize(pred)
            }
        }
    }

    fn observe(&mut self, obs_frame: &[u64]) {
        for (ch, slot) in self.history.iter_mut().enumerate().take(self.channels) {
            slot.push_back(self.width.normalize(obs_frame[ch]));
            if slot.len() > 64 {
                slot.pop_front();
            }
        }
        if let M3OracleMode::MultiView { tick, .. } = &mut self.mode {
            *tick += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// G3.1: Epistemic MDL conservation & detour functional transparency
// ---------------------------------------------------------------------------

fn build_conservation_base_editable(
    family: &str,
    width: Width,
    econ: EconomyConfig,
    growth: GrowthConfig,
    init_pri: u64,
    init_alt: u64,
) -> (EditableCircuit, u16) {
    let (mut ed, target_port) = match family {
        "T1" => {
            let mut c = Circuit::new(width, 1, 1, WireDelayMode::Combinational);
            let motif = if width == Width::BITS_1 {
                vec![0, 0, 0, 0, 1]
            } else {
                vec![0, 1, 3, 2, 1]
            };
            build_autonomous_ring(&mut c, motif.len(), &motif, 0);
            (
                EditableCircuit::new_with_growth(c, econ, &growth).expect("T1 ring"),
                0,
            )
        }
        "T2" => {
            let mut c = Circuit::new(width, 2, 2, WireDelayMode::Combinational);
            let d = c.add_node(Primitive::Delay {
                init: width.normalize(init_pri),
            });
            c.connect(
                SignalSource::BoundaryIn(0),
                SignalSink::NodeIn { node: d, port: 0 },
            );
            c.connect(
                SignalSource::NodeOut { node: d, port: 0 },
                SignalSink::BoundaryOut(1),
            );
            (
                EditableCircuit::new_with_growth(c, econ, &growth).expect("T2 circuit"),
                1,
            )
        }
        _ => {
            let mut c = Circuit::new(width, 3, 3, WireDelayMode::Combinational);
            let d_pri = c.add_node(Primitive::Delay {
                init: width.normalize(init_pri),
            });
            c.connect(
                SignalSource::BoundaryIn(0),
                SignalSink::NodeIn {
                    node: d_pri,
                    port: 0,
                },
            );
            c.connect(
                SignalSource::NodeOut {
                    node: d_pri,
                    port: 0,
                },
                SignalSink::BoundaryOut(2),
            );
            let d_alt = c.add_node(Primitive::Delay {
                init: width.normalize(init_alt),
            });
            c.connect(
                SignalSource::BoundaryIn(1),
                SignalSink::NodeIn {
                    node: d_alt,
                    port: 0,
                },
            );
            let c1 = materialize_const_loop(&mut c, 1);
            let add_alt = c.add_node(Primitive::Add);
            c.connect(
                SignalSource::NodeOut {
                    node: d_alt,
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: add_alt,
                    port: 0,
                },
            );
            c.connect(
                c1,
                SignalSink::NodeIn {
                    node: add_alt,
                    port: 1,
                },
            );
            let alt_src = SignalSource::NodeOut {
                node: add_alt,
                port: 0,
            };
            let comp = splice_comparator_onto_port(&mut c, 2, alt_src).expect("comp splice");
            let mut ed =
                EditableCircuit::new_with_growth(c, econ, &growth).expect("MV comp circuit");
            ed.register_comparator(comp.sub_node, comp.eqz_node, 2, 1 << 0, 1 << 1)
                .expect("register comp");
            (ed, 2)
        }
    };
    for node_econ in ed.economy_mut().nodes_mut() {
        node_econ.juvenile_ticks = 0;
        node_econ.debt_paid = true;
    }
    (ed, target_port)
}

fn check_edit_window_transparency(
    editable: &EditableCircuit,
    edit: &DetourEdit,
    target_port: u16,
    input_frames: &[Vec<u64>],
) -> Result<(), String> {
    let width = editable.circuit().width();
    let orig_compiled = editable.compiled();
    let mut orig_state = ScalarState::new(orig_compiled);
    orig_state
        .set_registers(orig_compiled, editable.state().registers())
        .map_err(|e| format!("orig set_registers: {e}"))?;
    let edit_compiled = edit
        .circuit
        .compile()
        .map_err(|e| format!("edit compile: {e}"))?;
    let mut edit_state = ScalarState::new(&edit_compiled);
    edit_state
        .set_registers(&edit_compiled, &edit.initial_registers)
        .map_err(|e| format!("edit set_registers: {e}"))?;

    let mut o1 = vec![0u64; usize::from(orig_compiled.num_outputs())];
    let mut o2 = vec![0u64; usize::from(edit_compiled.num_outputs())];
    let tp = usize::from(target_port);

    for (t, inp) in input_frames.iter().enumerate() {
        orig_state
            .step(orig_compiled, inp, &mut o1)
            .map_err(|e| format!("orig step: {e}"))?;
        edit_state
            .step(&edit_compiled, inp, &mut o2)
            .map_err(|e| format!("edit step: {e}"))?;
        let expected = if edit.class == DetourClass::D2OneFire && t == 0 {
            width.add(o1[tp], 1)
        } else {
            o1[tp]
        };
        if o2[tp] != expected {
            return Err(format!(
                "transparency mismatch ({}, t={t}): got {}, expected {expected}",
                edit.class, o2[tp]
            ));
        }
    }
    Ok(())
}

struct G31SeedResult {
    tally: GateTally,
    max_mdl_err: f64,
}

fn evaluate_g31_seed(seed: u32) -> G31SeedResult {
    let mut tally = GateTally::default();
    let mut max_mdl_err = 0.0f64;
    let seed64 = u64::from(seed);
    let growth = GrowthConfig {
        redundancy_sprouting: false,
        ..GrowthConfig::default()
    };
    let d0 = d0_lfsr();
    let d11 = d11_lfsr();

    // 1. Epistemic MDL conservation probes (`ReserveCap::Uncapped`, Established `juv=0` & Juvenile).
    for family in ["T1", "T2", "T2_MultiViewCoupled"] {
        for width in [Width::BITS_1, Width::BITS_2] {
            for &eps in &[0.0f64, 0.05f64] {
                for &mode in &[InjectionMode::Established, InjectionMode::Juvenile] {
                    let juv_ticks = match mode {
                        InjectionMode::Established => 0,
                        InjectionMode::Juvenile => EconomyConfig::default().juvenile_ticks,
                    };
                    let econ = EconomyConfig {
                        juvenile_ticks: juv_ticks,
                        reserve_cap: ReserveCap::Uncapped,
                        ..EconomyConfig::default()
                    };
                    let gen_cfg = match family {
                        "T1" => GeneratorConfig::T1_PeriodicMotif {
                            motif: if width == Width::BITS_1 {
                                vec![0, 0, 0, 0, 1]
                            } else {
                                vec![0, 1, 3, 2, 1]
                            },
                        },
                        "T2" => GeneratorConfig::T2_DelayedAffine {
                            driver: d0.clone(),
                            lag: 2,
                            mul_a: 1,
                            add_b: 0,
                        },
                        _ => GeneratorConfig::T2_MultiViewCoupled {
                            driver: MultiViewDriver::Lfsr { stage: d11.clone() },
                            view_0: MultiViewTransform {
                                delay: 0,
                                mul_a: 1,
                                add_b: 0,
                            },
                            view_1: MultiViewTransform {
                                delay: 0,
                                mul_a: 1,
                                add_b: width.mask(),
                            },
                            target: MultiViewTarget::Affine {
                                lag: 2,
                                mul_a: 1,
                                add_b: 0,
                            },
                        },
                    };
                    let target_port_hint: u16 = match family {
                        "T1" => 0,
                        "T2" => 1,
                        _ => 2,
                    };

                    for &class in &DetourClass::ALL {
                        let sb = SeedBank::new(seed64);
                        let generator = gen_cfg.build(width).expect("valid conservation gen");
                        let channels = usize::from(generator.channels());
                        let noise = if eps > 0.0 {
                            NoiseModel::IndependentSubstitution {
                                epsilon: eps,
                                channels: vec![target_port_hint],
                            }
                        } else {
                            NoiseModel::None
                        };
                        let mut env = NoisyEnvironment::new(generator, noise, sb).expect("env");
                        let mut warm0 = vec![0u64; channels];
                        let mut warm1 = vec![0u64; channels];
                        let mut obs = vec![0u64; channels];
                        if family != "T1" {
                            env.step_frame(&mut warm0, &mut obs);
                            env.step_frame(&mut warm1, &mut obs);
                        }
                        let (base_ed, target_port) = build_conservation_base_editable(
                            family,
                            width,
                            econ,
                            growth,
                            warm0[0],
                            *warm0.get(1).unwrap_or(&0),
                        );
                        let mut probe_growth = growth;
                        probe_growth.sprout_interval = 1_000_000;
                        let init_e_sum: f64 =
                            base_ed.economy().nodes().iter().map(|n| n.energy).sum();
                        let init_code_bits =
                            circuit_code_bits_with_tax(base_ed.circuit(), 0, econ.split_code_tax);
                        let mut learner =
                            ArkheLearner::from_editable(base_ed, probe_growth, sb).expect("lrn");
                        if family != "T1" {
                            learner.set_prev_observed_frame(&warm1);
                        }
                        let mut budget = TickBudget::new(100_000);
                        let mut clean = vec![0u64; channels];
                        let num_out = usize::from(learner.editable().circuit().num_outputs());

                        let pre_ticks = 24usize;
                        let post_ticks = 160usize;
                        let total_ticks = pre_ticks + post_ticks;
                        let mut facts = vec![Vec::with_capacity(total_ticks); num_out];
                        let mut preds = vec![Vec::with_capacity(total_ticks); num_out];
                        let mut phases = Vec::with_capacity(total_ticks);
                        let has_comp = !learner.editable().economy().comparators().is_empty();
                        let mut comp_pri = Vec::with_capacity(total_ticks);
                        let mut comp_alt = Vec::with_capacity(total_ticks);
                        let comp_masks =
                            learner.editable().economy().comparators().first().map(|c| {
                                (c.target_port, c.primary_channel_mask, c.alt_channel_mask)
                            });

                        let mut total_endowments = init_e_sum;
                        let mut exog_err = 0.0f64;
                        let mut pre_excision_de_err = 0.0f64;
                        let mut checked_pre_excision = false;
                        let mut transp_ok = true;
                        let mut transp_err: Option<String> = None;

                        for t in 0..total_ticks {
                            if t == pre_ticks {
                                let pre_n = learner.editable().circuit().node_count();
                                let pre_bits = circuit_code_bits_with_tax(
                                    learner.editable().circuit(),
                                    0,
                                    econ.split_code_tax,
                                );
                                let edit =
                                    build_detour_edit(learner.editable(), target_port, class)
                                        .expect("cons edit");
                                // Window equivalence check at T_inj (on clean frame sequence).
                                let probe_inputs: Vec<Vec<u64>> = (0..32u64)
                                    .map(|dt| {
                                        (0..channels)
                                            .map(|c| width.normalize((dt + c as u64 + seed64) * 3))
                                            .collect()
                                    })
                                    .collect();
                                if let Err(msg) = check_edit_window_transparency(
                                    learner.editable(),
                                    &edit,
                                    target_port,
                                    &probe_inputs,
                                ) {
                                    transp_ok = false;
                                    transp_err = Some(msg);
                                }
                                let post_n = edit.circuit.node_count();
                                let post_bits = circuit_code_bits_with_tax(
                                    &edit.circuit,
                                    0,
                                    econ.split_code_tax,
                                );
                                total_endowments += ((post_n - pre_n) as f64) * econ.initial_energy;
                                let expected_exog = match mode {
                                    InjectionMode::Established => post_bits - pre_bits,
                                    InjectionMode::Juvenile => 0.0,
                                };
                                edit.apply_to_learner(&mut learner, mode).expect("apply");
                                exog_err =
                                    (learner.exogenous_injection_bits() - expected_exog).abs();
                            }

                            env.step_frame(&mut clean, &mut obs);
                            let p = learner.predict(&mut budget);
                            let (pri_val, alt_val) = if has_comp
                                && let Some(comp) =
                                    learner.editable().economy().comparators().first()
                            {
                                let wires = learner.editable().state().wire_values();
                                let sub_in = &learner.editable().compiled().node_input_wires()
                                    [comp.sub_node.index()];
                                (
                                    width.normalize(wires[sub_in[0]]),
                                    width.normalize(wires[sub_in[1]]),
                                )
                            } else {
                                (0, 0)
                            };
                            learner.observe(&obs, &mut budget);
                            if learner.port_scored_last_observe(usize::from(target_port)) {
                                for ch in 0..num_out {
                                    if ch == usize::from(target_port) {
                                        preds[ch].push(p[ch]);
                                        facts[ch].push(obs[ch]);
                                    } else {
                                        preds[ch].push(0);
                                        facts[ch].push(0);
                                    }
                                }
                                phases.push(EpistemicTickPhase::Wake);
                                comp_pri.push(pri_val);
                                comp_alt.push(alt_val);
                            }

                            if t == pre_ticks + 16 {
                                let tel = learner.telemetry();
                                let juv_mismatch_exempt = mode == InjectionMode::Juvenile
                                    && (eps > 0.0 || class == DetourClass::D2OneFire);
                                if tel.idle_prune_events == 0
                                    && tel.starvation_prune_events == 0
                                    && !juv_mismatch_exempt
                                {
                                    checked_pre_excision = true;
                                    let delta_e = tel.final_total_energy - total_endowments;
                                    if mode == InjectionMode::Established {
                                        let post_inj_bits = circuit_code_bits_with_tax(
                                            learner.editable().circuit(),
                                            0,
                                            econ.split_code_tax,
                                        );
                                        let delta_l_null = post_inj_bits - tel.cumulative_net_flux;
                                        pre_excision_de_err = ((delta_e
                                            - tel.exogenous_injection_bits)
                                            + (delta_l_null - init_code_bits))
                                            .abs()
                                            / ((t + 1) as f64);
                                    } else {
                                        pre_excision_de_err = ((delta_e
                                            + tel.cumulative_maturity_debt_paid)
                                            - tel.cumulative_net_flux)
                                            .abs()
                                            / ((t + 1) as f64);
                                    }
                                }
                            }
                        }

                        let tel = learner.telemetry();
                        let fact_slices: Vec<&[u64]> = facts.iter().map(Vec::as_slice).collect();
                        let pred_slices: Vec<&[u64]> = preds.iter().map(Vec::as_slice).collect();
                        let comp_traces: Vec<EpistemicComparatorTrace<'_>> =
                            if let Some((tp, pm, am)) = comp_masks {
                                if learner.active_comparator_count() > 0 {
                                    vec![EpistemicComparatorTrace {
                                        target_port: usize::from(tp),
                                        registered_at_tick: 0,
                                        primary_channel_mask: pm,
                                        alt_channel_mask: am,
                                        primary_predictions: &comp_pri,
                                        alt_predictions: &comp_alt,
                                    }]
                                } else {
                                    Vec::new()
                                }
                            } else {
                                Vec::new()
                            };
                        let ref_mdl = epistemic_null_calibrated_mdl_delta_with_tax(
                            learner.editable().circuit(),
                            0.0,
                            0,
                            &fact_slices,
                            &pred_slices,
                            &phases,
                            &comp_traces,
                            econ.null_model,
                            econ.split_code_tax,
                        )
                        .expect("ref epistemic mdl");
                        let mdl_err = (tel.null_calibrated_mdl_delta_bits - ref_mdl).abs()
                            / (total_ticks as f64);
                        max_mdl_err = max_mdl_err.max(mdl_err);
                        let cliff = CliffCountersSummary::from(&tel);
                        let ok = transp_ok
                            && mdl_err <= G31_CONSERVATION_TOL_BITS_PER_SYMBOL
                            && (!checked_pre_excision
                                || pre_excision_de_err <= G31_CONSERVATION_TOL_BITS_PER_SYMBOL)
                            && exog_err <= G31_CONSERVATION_TOL_BITS_PER_SYMBOL;
                        let msg = (!ok).then(|| {
                            format!(
                                "mdl_err={mdl_err:.3e}, de_err={pre_excision_de_err:.3e}, exog_err={exog_err:.3e}, transp={transp_err:?}"
                            )
                        });
                        let g31_task = format!(
                            "G3.1:Cons({family},k={},eps={eps},{mode:?},{class})",
                            width.bits()
                        );
                        let g31_regime = if eps > 0.0 {
                            GateRegime::Noisy
                        } else {
                            GateRegime::Clean
                        };
                        tally.record_with_regime(
                            format!("{g31_task}:seed={seed}"),
                            g31_regime,
                            ok,
                            &cliff,
                            msg,
                        );
                        tally.record_tail_max(&g31_task, "max_epistemic_mdl_err", mdl_err);
                    }
                }
            }
        }
    }

    // 2. Formal `certify_circuit_against_spec` + `TicksPerHop` phase transparency on T1 periodic rings.
    for width in [Width::BITS_1, Width::BITS_2] {
        let motif = if width == Width::BITS_1 {
            vec![0, 0, 0, 0, 1]
        } else {
            vec![0, 1, 3, 2, 1]
        };
        let spec_gen = GeneratorConfig::T1_PeriodicMotif {
            motif: motif.clone(),
        }
        .build(width)
        .expect("valid spec gen");
        let spec = spec_gen.to_finite_state_spec().expect("finite state spec");

        for wire_delay in [WireDelayMode::Combinational, WireDelayMode::TicksPerHop] {
            let mut c = Circuit::new(width, 1, 1, wire_delay);
            build_autonomous_ring(&mut c, motif.len(), &motif, 0);
            let mut ed =
                EditableCircuit::new_with_growth(c, EconomyConfig::default(), &growth).expect("ed");
            for ne in ed.economy_mut().nodes_mut() {
                ne.juvenile_ticks = 0;
                ne.debt_paid = true;
            }
            if wire_delay == WireDelayMode::TicksPerHop {
                let mut out = [0u64];
                for _ in 0..24 {
                    ed.step(&[0], &mut out).expect("tph warm");
                }
            }
            for &class in &DetourClass::ALL {
                let edit = build_detour_edit(&ed, 0, class).expect("t1 transparency edit");
                let win_inputs = vec![vec![0u64]; 64];
                let win_res = check_edit_window_transparency(&ed, &edit, 0, &win_inputs);
                let spec_ok = if wire_delay == WireDelayMode::Combinational
                    && class != DetourClass::D2OneFire
                {
                    let comp = edit.circuit.compile().expect("compile transparency edit");
                    certify_circuit_against_spec(&comp, &spec, DEFAULT_MAX_CERTIFY_JOINT_STEPS)
                        .is_some()
                } else {
                    true
                };
                let ok = win_res.is_ok() && spec_ok;
                let msg = (!ok).then(|| format!("win={win_res:?}, spec_ok={spec_ok}"));
                let g31_spec_task =
                    format!("G3.1:SpecTransp(k={},{wire_delay:?},{class})", width.bits());
                tally.record(
                    format!("{g31_spec_task}:seed={seed}"),
                    ok,
                    &CliffCountersSummary::default(),
                    msg,
                );
                tally.record_tail_max(
                    &g31_spec_task,
                    "max_spec_transp_failed",
                    if ok { 0.0 } else { 1.0 },
                );
                tally.record_tail_max(
                    &g31_spec_task,
                    "max_post_edit_nodes",
                    edit.circuit.node_count() as f64,
                );
            }
        }
    }

    G31SeedResult { tally, max_mdl_err }
}

// ---------------------------------------------------------------------------
// G3.2, G3.3, G3.5(a): Injection excision, comparator preservation & ablation
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct InjCaseResult {
    settled: bool,
    tph_non_autonomous_skipped: bool,
    excised_within_w_obs: bool,
    excised_within_3w_obs: bool,
    collapsed: bool,
    excise_dt: Option<u64>,
    final_total_code_bits: f64,
    primary_restored: bool,
    comparator_preserved: bool,
    post_excision_clean_mismatches: u64,
    post_excision_kt_excess: f64,
    post_excision_paired_kt_diff: f64,
    post_inj_clean_mismatches: u64,
    post_inj_kt_excess: f64,
    post_inj_paired_kt_diff: f64,
    cliff: CliffCountersSummary,
    detail: Option<String>,
}

#[derive(Clone)]
struct SettledSnapshot {
    env: NoisyEnvironment<TierGenerator>,
    oracle: M3BayesOracle,
    tph_auto_probe: Option<(crate::substrate::CompiledCircuit, ScalarState)>,
    learner: ArkheLearner,
    budget: TickBudget,
    clean: Vec<u64>,
    obs: Vec<u64>,
    settled: bool,
    split_code_tax: bool,
    w_obs: u64,
    target_ch: usize,
    pre_inj_primary_nodes: usize,
    pre_inj_dead_nodes: usize,
    pre_inj_primary_code_bits: f64,
    pre_inj_total_code_bits: f64,
    pre_inj_disagree_prunes: u64,
    pre_inj_handoffs: u64,
    pre_inj_rings: u64,
    pre_driven: bool,
    is_tph_autonomous: bool,
}

fn step_settled_pair(
    env: &mut NoisyEnvironment<TierGenerator>,
    tph_probe: &mut Option<(crate::substrate::CompiledCircuit, ScalarState)>,
    clean_buf: &mut [u64],
    obs_buf: &mut [u64],
) {
    if let Some((compiled, state)) = tph_probe.as_mut() {
        state
            .step(compiled, &[0], clean_buf)
            .expect("tph clean step");
        obs_buf[0] = clean_buf[0];
    } else {
        env.step_frame(clean_buf, obs_buf);
    }
}

fn settle_injection_snapshot(
    task: &TaskSpec,
    wire_delay: WireDelayMode,
    epsilon: f64,
    excision_mode: ExcisionMode,
    split_code_tax: bool,
    seed: u64,
) -> SettledSnapshot {
    let econ_cfg = EconomyConfig {
        split_code_tax,
        ..EconomyConfig::default()
    };
    let growth_cfg = GrowthConfig {
        excision_mode,
        ..GrowthConfig::default()
    };
    let w_obs = observation_horizon(&econ_cfg, &growth_cfg) as u64;
    let gen_cfg = match wire_delay {
        WireDelayMode::Combinational => &task.comb_cfg,
        WireDelayMode::TicksPerHop => task.tph_cfg.as_ref().unwrap_or(&task.comb_cfg),
    };
    let noise = build_noise(epsilon, &task.noise_channels);
    let sb = SeedBank::new(seed);
    let generator = gen_cfg.build(task.width).expect("valid M3 task");
    let channels = usize::from(generator.channels());
    let target_ch = usize::from(task.target_port);

    let mut env = NoisyEnvironment::new(generator, noise, sb).expect("valid env");
    let mut oracle = M3BayesOracle::from_task(task, gen_cfg, channels);
    let mut tph_auto_probe: Option<(crate::substrate::CompiledCircuit, ScalarState)> = None;
    let mut learner = if wire_delay == WireDelayMode::TicksPerHop && task.is_autonomous_t1 {
        let motif = task
            .tph_ring_motif
            .expect("autonomous T1 task must define tph_ring_motif");
        let base_ed = build_autonomous_ring_tph_editable(task.width, motif, econ_cfg, &growth_cfg);
        let probe_compiled = base_ed.compiled().clone();
        let mut probe_state = ScalarState::new(&probe_compiled);
        probe_state
            .set_registers(&probe_compiled, base_ed.state().registers())
            .expect("set probe registers");
        tph_auto_probe = Some((probe_compiled, probe_state));
        ArkheLearner::from_editable(base_ed, growth_cfg, sb).expect("valid tph ring learner")
    } else if task.is_multiview && wire_delay == WireDelayMode::TicksPerHop {
        let base_ed = build_causal_split_tph_editable(task.width, econ_cfg, &growth_cfg);
        ArkheLearner::from_editable(base_ed, growth_cfg, sb).expect("valid tph split learner")
    } else {
        ArkheLearner::new_with_config(
            task.width,
            channels as u16,
            wire_delay,
            econ_cfg,
            growth_cfg,
            sb,
        )
        .expect("valid learner")
    };
    let mut budget = TickBudget::new(M3_INJECTION_TICK_BUDGET);
    let mut clean = vec![0u64; channels];
    let mut obs = vec![0u64; channels];

    let mut settled = false;
    if tph_auto_probe.is_some() {
        for _ in 0..w_obs {
            step_settled_pair(&mut env, &mut tph_auto_probe, &mut clean, &mut obs);
            let _ = learner.predict(&mut budget);
            learner.observe(&obs, &mut budget);
            oracle.observe(&obs);
        }
        settled = true;
    } else if epsilon == 0.0 {
        let mut last_sig = target_cone_signature(
            learner.editable().circuit(),
            learner.editable().economy(),
            task.target_port,
        );
        let mut cached_sig_epoch = learner.editable().structure_epoch();
        let mut cached_sig_comps = learner.editable().economy().comparators().len();
        let mut cached_driven = port_is_driven(learner.editable().circuit(), task.target_port);
        let mut last_sig_change_tick = 0u64;
        let mut clean_window: VecDeque<bool> = VecDeque::with_capacity(w_obs as usize);
        for t in 0..M3_MAX_SETTLE_TICKS {
            step_settled_pair(&mut env, &mut tph_auto_probe, &mut clean, &mut obs);
            let pred = learner.predict(&mut budget);
            let is_clean_match = task.width.normalize(pred[target_ch]) == clean[target_ch];
            if clean_window.len() == w_obs as usize {
                clean_window.pop_front();
            }
            clean_window.push_back(is_clean_match);
            learner.observe(&obs, &mut budget);
            oracle.observe(&obs);

            let epoch_now = learner.editable().structure_epoch();
            let comps_now = learner.editable().economy().comparators().len();
            if epoch_now != cached_sig_epoch || comps_now != cached_sig_comps {
                cached_sig_epoch = epoch_now;
                cached_sig_comps = comps_now;
                cached_driven = port_is_driven(learner.editable().circuit(), task.target_port);
                let curr_sig = target_cone_signature(
                    learner.editable().circuit(),
                    learner.editable().economy(),
                    task.target_port,
                );
                if curr_sig != last_sig {
                    last_sig = curr_sig;
                    last_sig_change_tick = t + 1;
                }
            }
            let stable_ticks = (t + 1).saturating_sub(last_sig_change_tick);
            let all_mature = learner
                .editable()
                .economy()
                .nodes()
                .iter()
                .all(|n| n.juvenile_ticks == 0)
                || target_cone_all_mature(
                    learner.editable().circuit(),
                    learner.editable().economy(),
                    task.target_port,
                );
            let driven = cached_driven;
            let comp_ok = !task.is_multiview || learner.active_comparator_count() >= 1;
            let window_matches = clean_window.iter().filter(|&&ok| ok).count();
            if t + 1 >= w_obs
                && stable_ticks >= w_obs
                && all_mature
                && driven
                && comp_ok
                && clean_window.len() == w_obs as usize
                && window_matches == w_obs as usize
            {
                settled = true;
                break;
            }
        }
    } else {
        let mut last_sig = target_cone_signature(
            learner.editable().circuit(),
            learner.editable().economy(),
            task.target_port,
        );
        let mut cached_sig_epoch = learner.editable().structure_epoch();
        let mut cached_sig_comps = learner.editable().economy().comparators().len();
        let mut cached_driven = port_is_driven(learner.editable().circuit(), task.target_port);
        let mut last_sig_change_tick = 0u64;
        let mut clean_window: VecDeque<bool> = VecDeque::with_capacity(w_obs as usize);
        let mut locked = false;
        for t in 0..M3_MAX_SETTLE_TICKS {
            step_settled_pair(&mut env, &mut tph_auto_probe, &mut clean, &mut obs);
            let pred = learner.predict(&mut budget);
            let is_clean_match = task.width.normalize(pred[target_ch]) == clean[target_ch];
            if clean_window.len() == w_obs as usize {
                clean_window.pop_front();
            }
            clean_window.push_back(is_clean_match);
            learner.observe(&clean, &mut budget);
            oracle.observe(&clean);

            let epoch_now = learner.editable().structure_epoch();
            let comps_now = learner.editable().economy().comparators().len();
            if epoch_now != cached_sig_epoch || comps_now != cached_sig_comps {
                cached_sig_epoch = epoch_now;
                cached_sig_comps = comps_now;
                cached_driven = port_is_driven(learner.editable().circuit(), task.target_port);
                let curr_sig = target_cone_signature(
                    learner.editable().circuit(),
                    learner.editable().economy(),
                    task.target_port,
                );
                if curr_sig != last_sig {
                    last_sig = curr_sig;
                    last_sig_change_tick = t + 1;
                }
            }
            let stable_ticks = (t + 1).saturating_sub(last_sig_change_tick);
            let all_mature = learner
                .editable()
                .economy()
                .nodes()
                .iter()
                .all(|n| n.juvenile_ticks == 0)
                || target_cone_all_mature(
                    learner.editable().circuit(),
                    learner.editable().economy(),
                    task.target_port,
                );
            let driven = cached_driven;
            let comp_ok = !task.is_multiview || learner.active_comparator_count() >= 1;
            let window_matches = clean_window.iter().filter(|&&ok| ok).count();
            if t + 1 >= w_obs
                && stable_ticks >= w_obs
                && all_mature
                && driven
                && comp_ok
                && clean_window.len() == w_obs as usize
                && window_matches == w_obs as usize
            {
                locked = true;
                break;
            }
        }
        if locked {
            for _ in 0..w_obs {
                step_settled_pair(&mut env, &mut tph_auto_probe, &mut clean, &mut obs);
                let _ = learner.predict(&mut budget);
                learner.observe(&obs, &mut budget);
                oracle.observe(&obs);
            }
            settled = true;
        }
    }

    let pre_circuit = learner.editable().circuit();
    let pre_econ = learner.editable().economy();
    let (pre_inj_primary_nodes, pre_inj_dead_nodes, pre_inj_primary_code_bits) =
        target_primary_and_dead_counts(pre_circuit, pre_econ, task.target_port, split_code_tax);
    let pre_inj_total_code_bits = circuit_code_bits_with_tax(pre_circuit, 0, split_code_tax);
    let pre_inj_disagree_prunes = learner.disagreement_prune_events();
    let pre_inj_handoffs = learner.driver_handoff_events();
    let pre_inj_rings = learner.ring_closure_events();
    let pre_driven = port_is_driven(pre_circuit, task.target_port);
    let is_tph_autonomous = pre_circuit
        .edges()
        .iter()
        .find(|e| e.dst == SignalSink::BoundaryOut(task.target_port))
        .map(|e| e.src)
        .is_some_and(|src| {
            cone_channel_mask(pre_circuit, src) == 0 && source_has_cycle(pre_circuit, src)
        });

    SettledSnapshot {
        env,
        oracle,
        tph_auto_probe,
        learner,
        budget,
        clean,
        obs,
        settled,
        split_code_tax,
        w_obs,
        target_ch,
        pre_inj_primary_nodes,
        pre_inj_dead_nodes,
        pre_inj_primary_code_bits,
        pre_inj_total_code_bits,
        pre_inj_disagree_prunes,
        pre_inj_handoffs,
        pre_inj_rings,
        pre_driven,
        is_tph_autonomous,
    }
}

/// Extends an already-settled clean Combinational `SettledSnapshot` (`0..t_lock`) with its
/// post-lock `W_obs` noisy phase (`eps > 0.0`), fast-forwarding only the cheap noise RNG stream.
fn settle_noisy_from_clean_prefix(
    clean_snap: &SettledSnapshot,
    task: &TaskSpec,
    epsilon: f64,
    seed: u64,
) -> SettledSnapshot {
    if !clean_snap.settled {
        return settle_injection_snapshot(
            task,
            WireDelayMode::Combinational,
            epsilon,
            ExcisionMode::VerifiedLocalExcision,
            clean_snap.split_code_tax,
            seed,
        );
    }

    let mut snap = clean_snap.clone();
    let generator = task.comb_cfg.build(task.width).expect("valid M3 task");
    let noise = build_noise(epsilon, &task.noise_channels);
    let mut noisy_env =
        NoisyEnvironment::new(generator, noise, SeedBank::new(seed)).expect("valid noisy env");
    let prefix_ticks = clean_snap.env.tick();
    let mut tmp_clean = vec![0u64; snap.clean.len()];
    let mut tmp_obs = vec![0u64; snap.obs.len()];
    for _ in 0..prefix_ticks {
        noisy_env.step_frame(&mut tmp_clean, &mut tmp_obs);
    }
    snap.env = noisy_env;

    for _ in 0..snap.w_obs {
        step_settled_pair(
            &mut snap.env,
            &mut snap.tph_auto_probe,
            &mut snap.clean,
            &mut snap.obs,
        );
        let _ = snap.learner.predict(&mut snap.budget);
        snap.learner.observe(&snap.obs, &mut snap.budget);
        snap.oracle.observe(&snap.obs);
    }

    let pre_circuit = snap.learner.editable().circuit();
    let pre_econ = snap.learner.editable().economy();
    let (pre_inj_primary_nodes, pre_inj_dead_nodes, pre_inj_primary_code_bits) =
        target_primary_and_dead_counts(
            pre_circuit,
            pre_econ,
            task.target_port,
            snap.split_code_tax,
        );
    snap.pre_inj_primary_nodes = pre_inj_primary_nodes;
    snap.pre_inj_dead_nodes = pre_inj_dead_nodes;
    snap.pre_inj_primary_code_bits = pre_inj_primary_code_bits;
    snap.pre_inj_total_code_bits = circuit_code_bits_with_tax(pre_circuit, 0, snap.split_code_tax);
    snap.pre_inj_disagree_prunes = snap.learner.disagreement_prune_events();
    snap.pre_inj_handoffs = snap.learner.driver_handoff_events();
    snap.pre_inj_rings = snap.learner.ring_closure_events();
    snap.pre_driven = port_is_driven(pre_circuit, task.target_port);
    snap.is_tph_autonomous = pre_circuit
        .edges()
        .iter()
        .find(|e| e.dst == SignalSink::BoundaryOut(task.target_port))
        .map(|e| e.src)
        .is_some_and(|src| {
            cone_channel_mask(pre_circuit, src) == 0 && source_has_cycle(pre_circuit, src)
        });
    snap
}

fn run_detour_from_settled(
    mut snap: SettledSnapshot,
    task: &TaskSpec,
    wire_delay: WireDelayMode,
    injection_mode: InjectionMode,
    detour_class: DetourClass,
) -> InjCaseResult {
    if !snap.settled {
        let tel = snap.learner.telemetry();
        return InjCaseResult {
            settled: false,
            tph_non_autonomous_skipped: false,
            excised_within_w_obs: false,
            excised_within_3w_obs: false,
            collapsed: false,
            excise_dt: None,
            final_total_code_bits: snap.pre_inj_total_code_bits,
            primary_restored: false,
            comparator_preserved: false,
            post_excision_clean_mismatches: 0,
            post_excision_kt_excess: 0.0,
            post_excision_paired_kt_diff: 0.0,
            post_inj_clean_mismatches: 0,
            post_inj_kt_excess: 0.0,
            post_inj_paired_kt_diff: 0.0,
            cliff: CliffCountersSummary::from(&tel),
            detail: Some("did not settle within MAX_SETTLE_TICKS".to_owned()),
        };
    }

    if wire_delay == WireDelayMode::TicksPerHop
        && detour_class.tph_series_hop_delta() > 0
        && !snap.is_tph_autonomous
    {
        let tel = snap.learner.telemetry();
        return InjCaseResult {
            settled: true,
            tph_non_autonomous_skipped: true,
            excised_within_w_obs: false,
            excised_within_3w_obs: false,
            collapsed: false,
            excise_dt: None,
            final_total_code_bits: snap.pre_inj_total_code_bits,
            primary_restored: true,
            comparator_preserved: true,
            post_excision_clean_mismatches: 0,
            post_excision_kt_excess: 0.0,
            post_excision_paired_kt_diff: 0.0,
            post_inj_clean_mismatches: 0,
            post_inj_kt_excess: 0.0,
            post_inj_paired_kt_diff: 0.0,
            cliff: CliffCountersSummary::from(&tel),
            detail: None,
        };
    }

    let edit = build_detour_edit(snap.learner.editable(), task.target_port, detour_class)
        .expect("build_detour_edit succeeds on settled circuit");
    edit.apply_to_learner(&mut snap.learner, injection_mode)
        .expect("apply detour edit");

    let post_horizon = 4 * snap.w_obs;
    let mut post_inj_clean_mismatches = 0u64;
    let mut post_inj_obs_mismatches = 0u64;
    let mut post_inj_oracle_obs_mismatches = 0u64;
    let mut post_excision_clean_mismatches = 0u64;
    let mut post_excision_obs_mismatches = 0u64;
    let mut post_excision_oracle_obs_mismatches = 0u64;
    let mut post_excision_ticks = 0u64;
    let mut collapsed = false;
    let mut excise_dt: Option<u64> = None;
    let mut cached_epoch = u64::MAX;
    let mut cached_comp_len = usize::MAX;
    let mut cached_juv_zero = usize::MAX;
    let mut curr_driven = snap.pre_driven;
    let mut curr_pri = snap.pre_inj_primary_nodes;
    let mut curr_dead = snap.pre_inj_dead_nodes;
    let mut curr_pri_bits = snap.pre_inj_primary_code_bits;

    for dt in 1..=post_horizon {
        step_settled_pair(
            &mut snap.env,
            &mut snap.tph_auto_probe,
            &mut snap.clean,
            &mut snap.obs,
        );
        let oracle_pred = snap.oracle.predict_target(&snap.clean);
        let pred = snap.learner.predict(&mut snap.budget);
        let p_norm = task.width.normalize(pred[snap.target_ch]);
        let clean_mis = u64::from(p_norm != snap.clean[snap.target_ch]);
        let obs_mis = u64::from(p_norm != snap.obs[snap.target_ch]);
        let oracle_obs_mis = u64::from(oracle_pred != snap.obs[snap.target_ch]);
        post_inj_clean_mismatches += clean_mis;
        post_inj_obs_mismatches += obs_mis;
        post_inj_oracle_obs_mismatches += oracle_obs_mis;
        if excise_dt.is_some() {
            post_excision_ticks += 1;
            post_excision_clean_mismatches += clean_mis;
            post_excision_obs_mismatches += obs_mis;
            post_excision_oracle_obs_mismatches += oracle_obs_mis;
        }
        snap.learner.observe(&snap.obs, &mut snap.budget);
        snap.oracle.observe(&snap.obs);

        let epoch_now = snap.learner.editable().structure_epoch();
        let e_now = snap.learner.editable().economy();
        let comp_len_now = e_now.comparators().len();
        let juv_zero_now = e_now
            .nodes()
            .iter()
            .filter(|n| n.juvenile_ticks == 0)
            .count();
        if epoch_now != cached_epoch
            || comp_len_now != cached_comp_len
            || juv_zero_now != cached_juv_zero
        {
            let c_now = snap.learner.editable().circuit();
            curr_driven = port_is_driven(c_now, task.target_port);
            (curr_pri, curr_dead, curr_pri_bits) =
                target_primary_and_dead_counts(c_now, e_now, task.target_port, snap.split_code_tax);
            cached_epoch = epoch_now;
            cached_comp_len = comp_len_now;
            cached_juv_zero = juv_zero_now;
        }
        let endogenous_primary_swap = snap.learner.driver_handoff_events() > snap.pre_inj_handoffs
            || snap.learner.ring_closure_events() > snap.pre_inj_rings;
        let size_and_mdl_restored = curr_pri <= snap.pre_inj_primary_nodes
            && curr_pri_bits <= snap.pre_inj_primary_code_bits + 1e-9;
        let iso_mdl_improved = curr_pri <= snap.pre_inj_primary_nodes
            && curr_pri_bits + 1e-9 < snap.pre_inj_primary_code_bits;
        let restored_now = curr_dead <= snap.pre_inj_dead_nodes
            && ((snap.learner.surviving_injected_detour_nodes() == 0
                && (size_and_mdl_restored || endogenous_primary_swap))
                || iso_mdl_improved);

        if (snap.pre_driven && !curr_driven) || (snap.pre_inj_primary_nodes > 0 && curr_pri == 0) {
            collapsed = true;
        }
        if !collapsed
            && excise_dt.is_none()
            && restored_now
            && (!snap.pre_driven
                || (curr_driven && (snap.pre_inj_primary_nodes == 0 || curr_pri > 0)))
        {
            excise_dt = Some(dt);
        }
    }

    let final_circuit = snap.learner.editable().circuit();
    let final_econ = snap.learner.editable().economy();
    let (final_primary_nodes, final_dead_nodes, final_primary_code_bits) =
        target_primary_and_dead_counts(
            final_circuit,
            final_econ,
            task.target_port,
            snap.split_code_tax,
        );
    let final_total_code_bits = circuit_code_bits_with_tax(final_circuit, 0, snap.split_code_tax);
    let endogenous_primary_swap = snap.learner.driver_handoff_events() > snap.pre_inj_handoffs
        || snap.learner.ring_closure_events() > snap.pre_inj_rings;
    let final_size_and_mdl_restored = final_primary_nodes <= snap.pre_inj_primary_nodes
        && final_primary_code_bits <= snap.pre_inj_primary_code_bits + 1e-9;
    let final_iso_mdl_improved = final_primary_nodes <= snap.pre_inj_primary_nodes
        && final_primary_code_bits + 1e-9 < snap.pre_inj_primary_code_bits;
    let primary_restored = final_dead_nodes <= snap.pre_inj_dead_nodes
        && ((snap.learner.surviving_injected_detour_nodes() == 0
            && (final_size_and_mdl_restored || endogenous_primary_swap))
            || final_iso_mdl_improved);

    let excised_by_end = !collapsed && primary_restored && excise_dt.is_some();
    let excised_within_w_obs = excised_by_end && excise_dt.is_some_and(|dt| dt <= snap.w_obs);
    let excised_within_3w_obs = excised_by_end && excise_dt.is_some_and(|dt| dt <= 3 * snap.w_obs);

    let post_inj_kt_excess =
        facts_code_bits(task.width, post_horizon, post_inj_obs_mismatches) / (post_horizon as f64);
    let post_inj_oracle_kt =
        facts_code_bits(task.width, post_horizon, post_inj_oracle_obs_mismatches)
            / (post_horizon as f64);
    let post_inj_paired_kt_diff = (post_inj_kt_excess - post_inj_oracle_kt).abs();
    let (post_excision_kt_excess, post_excision_paired_kt_diff) = if post_excision_ticks > 0 {
        let l_kt = facts_code_bits(
            task.width,
            post_excision_ticks,
            post_excision_obs_mismatches,
        ) / (post_excision_ticks as f64);
        let o_kt = facts_code_bits(
            task.width,
            post_excision_ticks,
            post_excision_oracle_obs_mismatches,
        ) / (post_excision_ticks as f64);
        (l_kt, (l_kt - o_kt).abs())
    } else {
        (f64::INFINITY, f64::INFINITY)
    };

    let tel = snap.learner.telemetry();
    let comparator_preserved = !task.is_multiview
        || (tel.active_comparator_count >= 1
            && tel.disagreement_prune_events == snap.pre_inj_disagree_prunes);

    let detail = if !excised_by_end || collapsed || !comparator_preserved {
        Some(format!(
            "excise_dt={excise_dt:?}, collapsed={collapsed}, pri=({final_primary_nodes}/{}), bits=({final_primary_code_bits:.2}/{:.2}), comp_ok={comparator_preserved}, post_exc_clean_mis={post_excision_clean_mismatches}, post_exc_kt={post_excision_kt_excess:.5}, paired_kt_diff={post_excision_paired_kt_diff:.5}",
            snap.pre_inj_primary_nodes, snap.pre_inj_primary_code_bits
        ))
    } else {
        None
    };

    InjCaseResult {
        settled: true,
        tph_non_autonomous_skipped: false,
        excised_within_w_obs,
        excised_within_3w_obs,
        collapsed,
        excise_dt,
        final_total_code_bits,
        primary_restored,
        comparator_preserved,
        post_excision_clean_mismatches,
        post_excision_kt_excess,
        post_excision_paired_kt_diff,
        post_inj_clean_mismatches,
        post_inj_kt_excess,
        post_inj_paired_kt_diff,
        cliff: CliffCountersSummary::from(&tel),
        detail,
    }
}

// ---------------------------------------------------------------------------
// G3.4 & G3.5(b): Spontaneous long-horizon discipline & SPLIT-tax ablation
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct SpontCaseResult {
    locked: bool,
    t_lock: Option<u64>,
    t_settle: u64,
    n_primary_at_settle: usize,
    n_primary_2000: usize,
    n_primary_8000: usize,
    max_comp_per_port_8000: usize,
    final_split_count: usize,
    final_code_bits: f64,
    analytical_split_tax_delta: Option<f64>,
    clean_locked_8000: bool,
    cliff: CliffCountersSummary,
}

fn analytical_split_tax_delta_for_circuit(circuit: &Circuit) -> Option<f64> {
    let split_nodes = circuit
        .nodes()
        .iter()
        .filter(|&&p| p == Primitive::Split)
        .count();
    if split_nodes == 0 {
        return None;
    }
    let port1_edges = circuit
        .edges()
        .iter()
        .filter(|e| {
            matches!(e.src, SignalSource::NodeOut { node, port: 1 } if circuit.nodes()[node.index()] == Primitive::Split)
        })
        .count();
    if port1_edges == 0 {
        return None;
    }
    let with_tax = circuit_code_bits_with_tax(circuit, 0, true);
    let without_tax = circuit_code_bits_with_tax(circuit, 0, false);
    Some(with_tax - without_tax)
}

fn run_spontaneous_case(
    spec: &SpontaneousSpec,
    split_code_tax: bool,
    seed: u64,
) -> SpontCaseResult {
    let econ_cfg = EconomyConfig {
        split_code_tax,
        ..EconomyConfig::default()
    };
    let growth_cfg = GrowthConfig::default();
    let w_obs = observation_horizon(&econ_cfg, &growth_cfg) as u64;
    let noise = build_noise(spec.epsilon, &spec.noise_channels);
    let sb = SeedBank::new(seed);
    let generator = spec
        .gen_cfg
        .build(spec.width)
        .expect("valid spontaneous generator");
    let channels = usize::from(generator.channels());
    let target_ch = usize::from(spec.target_port);

    let mut env = NoisyEnvironment::new(generator, noise, sb).expect("valid env");
    let mut learner =
        if spec.wire_delay == WireDelayMode::TicksPerHop && spec.tier == "T2_MultiViewCoupled" {
            let base_ed = build_causal_split_tph_editable(spec.width, econ_cfg, &growth_cfg);
            ArkheLearner::from_editable(base_ed, growth_cfg, sb).expect("valid tph split learner")
        } else {
            ArkheLearner::new_with_config(
                spec.width,
                channels as u16,
                spec.wire_delay,
                econ_cfg,
                growth_cfg,
                sb,
            )
            .expect("valid learner")
        };
    let mut budget = TickBudget::new(M3_SPONTANEOUS_BUDGET_T8000);
    let mut clean = vec![0u64; channels];
    let mut obs = vec![0u64; channels];

    let is_noisy_constant =
        matches!(spec.gen_cfg, GeneratorConfig::T1_Constant { .. }) && spec.epsilon > 0.0;
    let mut clean_window: VecDeque<bool> = VecDeque::with_capacity(w_obs as usize);
    let mut last_sig = target_cone_signature_without_comparator_taps(
        learner.editable().circuit(),
        learner.editable().economy(),
        spec.target_port,
    );
    let mut cached_sig_epoch = learner.editable().structure_epoch();
    let mut cached_sig_comps = learner.editable().economy().comparators().len();
    let mut last_sig_change_tick = 0u64;
    let mut t_lock: Option<u64> = None;
    let mut raw_t_settle: Option<u64> = None;
    let mut n_primary_at_settle: Option<usize> = None;
    let mut n_primary_2000 = 0usize;
    let mut tail_clean_mis_8000 = 0u64;

    for t in 0..M3_SPONTANEOUS_T8000 {
        env.step_frame(&mut clean, &mut obs);
        let pred = learner.predict(&mut budget);
        let p_norm = spec.width.normalize(pred[target_ch]);
        let clean_ok = p_norm == clean[target_ch];
        if clean_window.len() == w_obs as usize {
            clean_window.pop_front();
        }
        clean_window.push_back(clean_ok);
        if t + w_obs >= M3_SPONTANEOUS_T8000 && !clean_ok {
            tail_clean_mis_8000 += 1;
        }
        learner.observe(&obs, &mut budget);

        let epoch_now = learner.editable().structure_epoch();
        let comps_now = learner.editable().economy().comparators().len();
        if epoch_now != cached_sig_epoch || comps_now != cached_sig_comps {
            cached_sig_epoch = epoch_now;
            cached_sig_comps = comps_now;
            let curr_sig = target_cone_signature_without_comparator_taps(
                learner.editable().circuit(),
                learner.editable().economy(),
                spec.target_port,
            );
            if curr_sig != last_sig {
                last_sig = curr_sig;
                last_sig_change_tick = t + 1;
            }
        }
        if raw_t_settle.is_none() && learner.is_developmental_settled() {
            raw_t_settle = Some(t + 1);
            if t < M3_SPONTANEOUS_T2000 {
                n_primary_at_settle = Some(
                    primary_cone_nodes_without_comparator_taps(
                        learner.editable().circuit(),
                        learner.editable().economy(),
                        spec.target_port,
                    )
                    .len(),
                );
            }
        }
        if t_lock.is_none() && t + 1 >= w_obs {
            let all_mature = learner
                .editable()
                .economy()
                .nodes()
                .iter()
                .all(|n| n.juvenile_ticks == 0)
                || target_cone_all_mature(
                    learner.editable().circuit(),
                    learner.editable().economy(),
                    spec.target_port,
                );
            let stable_ticks = (t + 1).saturating_sub(last_sig_change_tick);
            let matches = clean_window.iter().filter(|&&ok| ok).count();
            let is_locked = if is_noisy_constant {
                stable_ticks >= w_obs
                    && all_mature
                    && learner.editable().circuit().node_count() == 0
            } else if spec.epsilon == 0.0 {
                clean_window.len() == w_obs as usize && matches == w_obs as usize && all_mature
            } else {
                stable_ticks >= w_obs && all_mature && (matches as f64) / (w_obs as f64) >= 0.80
            };
            if is_locked {
                t_lock = Some(t + 1);
            }
        }
        if t + 1 == M3_SPONTANEOUS_T2000 {
            n_primary_2000 = primary_cone_nodes_without_comparator_taps(
                learner.editable().circuit(),
                learner.editable().economy(),
                spec.target_port,
            )
            .len();
            if n_primary_at_settle.is_none() {
                n_primary_at_settle = Some(n_primary_2000);
            }
        }
    }

    let final_circuit = learner.editable().circuit();
    let final_econ = learner.editable().economy();
    let n_primary_8000 =
        primary_cone_nodes_without_comparator_taps(final_circuit, final_econ, spec.target_port)
            .len();
    let max_comp_per_port_8000 = max_comparators_on_any_port(final_circuit, final_econ);
    let tel = learner.telemetry();
    let final_code_bits = circuit_code_bits_with_tax(final_circuit, 0, split_code_tax);
    let analytical_split_tax_delta = analytical_split_tax_delta_for_circuit(final_circuit);
    let clean_locked_8000 = if is_noisy_constant {
        n_primary_8000 == 0
    } else if spec.epsilon == 0.0 {
        tail_clean_mis_8000 == 0
    } else {
        t_lock.is_some()
    };
    let t_settle = raw_t_settle
        .unwrap_or(M3_SPONTANEOUS_T2000)
        .min(M3_SPONTANEOUS_T2000);
    let n_primary_at_settle = n_primary_at_settle.unwrap_or(n_primary_2000);

    SpontCaseResult {
        locked: t_lock.is_some(),
        t_lock,
        t_settle,
        n_primary_at_settle,
        n_primary_2000,
        n_primary_8000,
        max_comp_per_port_8000,
        final_split_count: tel.final_split_count,
        final_code_bits,
        analytical_split_tax_delta,
        clean_locked_8000,
        cliff: CliffCountersSummary::from(&tel),
    }
}

fn compute_certified_n_min(spec: &SpontaneousSpec) -> Option<usize> {
    let Ok(mut generator) = spec.gen_cfg.build(spec.width) else {
        return None;
    };
    let channels = generator.channels();
    if channels > 2 {
        return None;
    }
    let spec_opt = generator.to_finite_state_spec().ok();
    let stationary = spec_opt
        .as_ref()
        .filter(|s| s.scope == EpsilonMachineScope::StationaryProcess);
    let mut clean_trace = Vec::with_capacity(256);
    let mut frame = vec![0u64; usize::from(channels)];
    for _ in 0..256 {
        generator.next_clean_frame(&mut frame);
        clean_trace.push(frame.clone());
    }
    let max_nodes = if channels == 1 { 4 } else { 3 };
    let (circ_opt, status) = exact_synthesize_certified_circuit(
        spec.width,
        channels,
        spec.wire_delay,
        stationary,
        &clean_trace,
        max_nodes,
        DEFAULT_MAX_CERTIFY_JOINT_STEPS,
    );
    match status {
        SynthesisCertStatus::CertifiedExact { .. } => circ_opt.map(|c| c.node_count()),
        _ => None,
    }
}

/// Analytical `Split` pricing theorem verifier for `G3.5(b)`.
///
/// Confirms `circuit_code_bits_with_tax(c, 0, true) - circuit_code_bits_with_tax(c, 0, false)`
/// equals `log2(PALETTE_SIZE) + log2(N_src * N_dst) >= 6.58` bits on every `Primitive::Split`
/// fan-out edge.
#[must_use]
pub fn verify_analytical_split_pricing_theorem(circuit: &Circuit) -> Option<f64> {
    let split_nodes = circuit
        .nodes()
        .iter()
        .filter(|&&p| p == Primitive::Split)
        .count();
    let port1_edges = circuit
        .edges()
        .iter()
        .filter(|e| {
            matches!(e.src, SignalSource::NodeOut { node, port: 1 } if circuit.nodes()[node.index()] == Primitive::Split)
        })
        .count();
    if split_nodes == 0 || port1_edges != split_nodes {
        return None;
    }
    let src_ports = usize::from(circuit.num_inputs())
        + circuit
            .nodes()
            .iter()
            .map(|p| usize::from(p.output_arity()))
            .sum::<usize>();
    let dst_ports = usize::from(circuit.num_outputs())
        + circuit
            .nodes()
            .iter()
            .map(|p| usize::from(p.input_arity()))
            .sum::<usize>();
    let expected_per_split = (Primitive::PALETTE_SIZE as f64).log2()
        + ((src_ports.max(1) * dst_ports.max(1)) as f64).log2();
    let observed = circuit_code_bits_with_tax(circuit, 0, true)
        - circuit_code_bits_with_tax(circuit, 0, false);
    if (observed - (split_nodes as f64) * expected_per_split).abs() <= 1e-11 {
        Some(expected_per_split)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Unified single-bank & multi-bank execution with trajectory sharing
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct SettleGroupDesc {
    task_idx: usize,
    wire_delay: WireDelayMode,
    epsilon: f64,
    split_code_tax: bool,
    need_baseline: bool,
    prefix_group_idx: Option<usize>,
}

#[derive(Clone, Copy)]
struct InjJobDesc {
    settle_group_idx: usize,
    task_idx: usize,
    wire_delay: WireDelayMode,
    epsilon: f64,
    injection_mode: InjectionMode,
    detour_class: DetourClass,
}

fn build_settle_groups_and_jobs(
    tasks: &[TaskSpec],
) -> (
    Vec<SettleGroupDesc>,
    Vec<InjJobDesc>,
    Vec<InjJobDesc>,
    Vec<InjJobDesc>,
) {
    let mut groups = Vec::new();
    let mut vle_jobs = Vec::new();
    let mut base_jobs = Vec::new();
    let mut split_false_jobs = Vec::new();
    let ablate_classes = [
        DetourClass::D1AddZero,
        DetourClass::D2OneFire,
        DetourClass::D3MulZero,
        DetourClass::D4MulOne,
        DetourClass::D5CancelSub,
        DetourClass::D6Involution,
        DetourClass::D7bAgreementGate,
    ];
    let split_ablate_classes = [
        DetourClass::D0Dead,
        DetourClass::D1AddZero,
        DetourClass::D4MulOne,
        DetourClass::D5CancelSub,
        DetourClass::D7bAgreementGate,
    ];

    for (ti, task) in tasks.iter().enumerate() {
        let mut clean_comb_group_idx = None;
        for &eps in &[0.0, 0.05] {
            let group_idx = groups.len();
            let need_baseline = eps == 0.0;
            let prefix_group_idx = if eps > 0.0 {
                clean_comb_group_idx
            } else {
                clean_comb_group_idx = Some(group_idx);
                None
            };
            groups.push(SettleGroupDesc {
                task_idx: ti,
                wire_delay: WireDelayMode::Combinational,
                epsilon: eps,
                split_code_tax: true,
                need_baseline,
                prefix_group_idx,
            });
            for &inj_mode in &[InjectionMode::Established, InjectionMode::Juvenile] {
                for &class in &DetourClass::ALL {
                    vle_jobs.push(InjJobDesc {
                        settle_group_idx: group_idx,
                        task_idx: ti,
                        wire_delay: WireDelayMode::Combinational,
                        epsilon: eps,
                        injection_mode: inj_mode,
                        detour_class: class,
                    });
                }
            }
            if need_baseline {
                for &class in &ablate_classes {
                    base_jobs.push(InjJobDesc {
                        settle_group_idx: group_idx,
                        task_idx: ti,
                        wire_delay: WireDelayMode::Combinational,
                        epsilon: 0.0,
                        injection_mode: InjectionMode::Established,
                        detour_class: class,
                    });
                }
                if task.split_tax_ablate_inj {
                    let sf_group_idx = groups.len();
                    groups.push(SettleGroupDesc {
                        task_idx: ti,
                        wire_delay: WireDelayMode::Combinational,
                        epsilon: 0.0,
                        split_code_tax: false,
                        need_baseline: false,
                        prefix_group_idx: None,
                    });
                    for &inj_mode in &[InjectionMode::Established, InjectionMode::Juvenile] {
                        for &class in &split_ablate_classes {
                            split_false_jobs.push(InjJobDesc {
                                settle_group_idx: sf_group_idx,
                                task_idx: ti,
                                wire_delay: WireDelayMode::Combinational,
                                epsilon: 0.0,
                                injection_mode: inj_mode,
                                detour_class: class,
                            });
                        }
                    }
                }
            }
        }
        if task.tph_cfg.is_some() {
            let group_idx = groups.len();
            groups.push(SettleGroupDesc {
                task_idx: ti,
                wire_delay: WireDelayMode::TicksPerHop,
                epsilon: 0.0,
                split_code_tax: true,
                need_baseline: false,
                prefix_group_idx: None,
            });
            let classes: &[DetourClass] = if task.is_autonomous_t1 {
                &DetourClass::ALL
            } else {
                &[DetourClass::D0Dead, DetourClass::D7aFakeComparator]
            };
            for &inj_mode in &[InjectionMode::Established, InjectionMode::Juvenile] {
                for &class in classes {
                    vle_jobs.push(InjJobDesc {
                        settle_group_idx: group_idx,
                        task_idx: ti,
                        wire_delay: WireDelayMode::TicksPerHop,
                        epsilon: 0.0,
                        injection_mode: inj_mode,
                        detour_class: class,
                    });
                }
            }
        }
    }
    (groups, vle_jobs, base_jobs, split_false_jobs)
}

struct SeedAllGatesRaw {
    g31: GateTally,
    g31_max_mdl_err: f64,
    g32: GateTally,
    g32_max_excise_dt: Option<u64>,
    g32_raw_detour_passed: usize,
    g32_raw_detour_total: usize,
    g33: GateTally,
    g33_max_excise_dt: Option<u64>,
    g33_raw_detour_passed: usize,
    g33_raw_detour_total: usize,
    g34: GateTally,
    g35a: GateTally,
    g35b: GateTally,
    g35b_min_analytical_split_tax_bits: f64,
    spont_clean_splits_true: Vec<usize>,
    spont_clean_splits_false: Vec<usize>,
    spont_noisy_splits_true: usize,
    spont_noisy_splits_false: usize,
    spont_all_splits_true: usize,
    spont_all_splits_false: usize,
    inj_paired_count: usize,
    inj_excised_tax_true: usize,
    inj_excised_tax_false: usize,
}

struct BankAggRaw {
    seed_offset: u32,
    seeds_tested: u32,
    g31: GateTally,
    g31_max_mdl_err: f64,
    g32: GateTally,
    g32_max_excise_dt: Option<u64>,
    g32_raw_detour_passed: usize,
    g32_raw_detour_total: usize,
    g33: GateTally,
    g33_max_excise_dt: Option<u64>,
    g33_raw_detour_passed: usize,
    g33_raw_detour_total: usize,
    g34: GateTally,
    g35a: GateTally,
    g35b: GateTally,
    g35b_min_analytical_split_tax_bits: f64,
    spont_clean_splits_true: Vec<usize>,
    spont_clean_splits_false: Vec<usize>,
    spont_noisy_splits_true: usize,
    spont_noisy_splits_false: usize,
    spont_all_splits_true: usize,
    spont_all_splits_false: usize,
    inj_paired_count: usize,
    inj_excised_tax_true: usize,
    inj_excised_tax_false: usize,
}

#[derive(Clone, Debug)]
struct TrajectoryDetourObservation<'a> {
    settle_group_idx: usize,
    detour_class: DetourClass,
    functional_ok: bool,
    excise_dt: Option<u64>,
    post_excision_kt_excess: f64,
    post_excision_paired_kt_diff: f64,
    cliff: &'a CliffCountersSummary,
    detail: Option<String>,
}

#[derive(Default)]
struct TrajectoryUnitAccum {
    raw_passed: usize,
    raw_total: usize,
    cliff: CliffCountersSummary,
    failing_details: Vec<String>,
    excise_dts: Vec<f64>,
    max_post_exc_kt_excess: Option<f64>,
    max_paired_kt_diff: Option<f64>,
}

fn evaluate_vle_detour_outcome(
    job: &InjJobDesc,
    out: &InjCaseResult,
    w_obs: u64,
) -> (bool, Option<String>) {
    match job.injection_mode {
        InjectionMode::Established => {
            let pred_ok = if job.epsilon == 0.0 {
                out.post_excision_clean_mismatches == 0
                    && out.post_excision_paired_kt_diff <= G32_CLEAN_EXCESS_MAX
                    && out.post_excision_kt_excess <= G32_CLEAN_RAW_KT_MAX
            } else {
                out.post_excision_paired_kt_diff <= G32_NOISY_KT_DIFF_MAX
            };
            let ok = out.settled
                && out.excised_within_w_obs
                && !out.collapsed
                && out.primary_restored
                && out.comparator_preserved
                && pred_ok;
            let detail = (!ok).then(|| {
                out.detail.clone().unwrap_or_else(|| {
                    format!(
                        "dt={:?} (limit={w_obs}), clean_mis={}, clean_kt={:.5}, paired_diff={:.5}",
                        out.excise_dt,
                        out.post_excision_clean_mismatches,
                        out.post_excision_kt_excess,
                        out.post_excision_paired_kt_diff
                    )
                })
            });
            (ok, detail)
        }
        InjectionMode::Juvenile => {
            let transient_ok = if job.epsilon == 0.0 {
                if job.detour_class == DetourClass::D2OneFire {
                    out.post_inj_clean_mismatches <= 1
                        && out.post_excision_clean_mismatches == 0
                        && out.post_inj_kt_excess <= G33_D2_TRANSIENT_KT_EXCESS_MAX
                        && out.post_inj_paired_kt_diff <= G33_D2_PAIRED_KT_DIFF_MAX
                } else {
                    out.post_inj_clean_mismatches == 0
                }
            } else {
                true
            };
            let ok = out.settled
                && out.excised_within_3w_obs
                && !out.collapsed
                && out.primary_restored
                && out.comparator_preserved
                && transient_ok;
            let detail = (!ok).then(|| {
                out.detail.clone().unwrap_or_else(|| {
                    format!(
                        "dt={:?} (limit={}), post_inj_mis={}, post_inj_kt={:.5}, post_inj_paired={:.5}",
                        out.excise_dt,
                        3 * w_obs,
                        out.post_inj_clean_mismatches,
                        out.post_inj_kt_excess,
                        out.post_inj_paired_kt_diff
                    )
                })
            });
            (ok, detail)
        }
    }
}

fn record_injection_trajectory_units<'a, F, R>(
    gate_label: &str,
    seed: u32,
    num_groups: usize,
    observations: impl IntoIterator<Item = TrajectoryDetourObservation<'a>>,
    label_for_group: F,
    regime_for_group: R,
    tally: &mut GateTally,
) -> (Option<u64>, usize, usize)
where
    F: Fn(usize) -> String,
    R: Fn(usize) -> GateRegime,
{
    let fallback_excise_dt =
        (3 * observation_horizon(&EconomyConfig::default(), &GrowthConfig::default())) as f64;
    let mut units: Vec<Option<TrajectoryUnitAccum>> = (0..num_groups).map(|_| None).collect();
    let mut max_dt: Option<u64> = None;
    for obs in observations {
        if let Some(dt) = obs.excise_dt {
            max_dt = Some(max_dt.map_or(dt, |m| m.max(dt)));
        }
        let acc = units[obs.settle_group_idx].get_or_insert_with(TrajectoryUnitAccum::default);
        acc.cliff.accumulate(obs.cliff);
        if let Some(dt) = obs.excise_dt {
            acc.excise_dts.push(dt as f64);
        }
        if obs.post_excision_kt_excess.is_finite() {
            acc.max_post_exc_kt_excess = Some(
                acc.max_post_exc_kt_excess
                    .map_or(obs.post_excision_kt_excess, |m| {
                        m.max(obs.post_excision_kt_excess)
                    }),
            );
        }
        if obs.post_excision_paired_kt_diff.is_finite() {
            acc.max_paired_kt_diff = Some(
                acc.max_paired_kt_diff
                    .map_or(obs.post_excision_paired_kt_diff, |m| {
                        m.max(obs.post_excision_paired_kt_diff)
                    }),
            );
        }
        acc.raw_total += 1;
        if classify_case_outcome(obs.functional_ok, obs.cliff) == CaseOutcome::Passed {
            acc.raw_passed += 1;
        }
        if !obs.functional_ok {
            let reason = obs.detail.unwrap_or_else(|| "failed".to_owned());
            acc.failing_details
                .push(format!("{}: {}", obs.detour_class, reason));
        }
    }

    let mut raw_passed = 0usize;
    let mut raw_total = 0usize;
    for (group_idx, unit_opt) in units.into_iter().enumerate() {
        let Some(unit) = unit_opt else {
            continue;
        };
        raw_passed += unit.raw_passed;
        raw_total += unit.raw_total;
        let unit_ok = unit.failing_details.is_empty();
        let detail = (!unit_ok).then(|| {
            format!(
                "failed_detours={}/{}: {}",
                unit.failing_details.len(),
                unit.raw_total,
                unit.failing_details.join("; ")
            )
        });
        let task_name = format!("{gate_label}:{}", label_for_group(group_idx));
        let regime = regime_for_group(group_idx);
        let unit_label = format!("{task_name}:seed={seed}");
        tally.record_with_regime(unit_label, regime, unit_ok, &unit.cliff, detail);
        if unit.excise_dts.is_empty() {
            tally.record_dt_samples(&task_name, &[fallback_excise_dt]);
        } else {
            tally.record_dt_samples(&task_name, &unit.excise_dts);
        }
        if let Some(v) = unit.max_post_exc_kt_excess {
            tally.record_tail_max(&task_name, "max_post_exc_kt_excess", v);
        }
        if let Some(v) = unit.max_paired_kt_diff {
            tally.record_tail_max(&task_name, "max_paired_kt_diff", v);
        }
    }
    (max_dt, raw_passed, raw_total)
}

fn format_excise_raw_summary(max_dt: Option<u64>, raw_passed: usize, raw_total: usize) -> String {
    match max_dt {
        Some(dt) => format!("max_excise_dt={dt} ticks; raw_detours={raw_passed}/{raw_total}"),
        None => format!("max_excise_dt=None; raw_detours={raw_passed}/{raw_total}"),
    }
}

const fn g35b_solvability_ok(epsilon: f64, res: &SpontCaseResult) -> bool {
    epsilon > 0.0 || res.clean_locked_8000
}

fn g35b_analytical_tax_ok(res: &SpontCaseResult) -> bool {
    res.analytical_split_tax_delta.is_none_or(|delta| {
        delta >= G35B_MIN_SPLIT_TAX_BITS * (res.final_split_count.max(1) as f64)
    })
}

fn clean_spont_spec_labels(spont_specs: &[SpontaneousSpec]) -> Vec<String> {
    spont_specs
        .iter()
        .filter(|s| s.epsilon == 0.0)
        .map(|s| format!("{}({:?})", s.label, s.wire_delay))
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn evaluate_g35b_split_discipline(
    clean_labels: &[String],
    clean_true: &[usize],
    clean_false: &[usize],
    noisy_true: usize,
    noisy_false: usize,
    all_true: usize,
    all_false: usize,
    inj_paired: usize,
    inj_exc_true: usize,
    inj_exc_false: usize,
) -> Vec<String> {
    let mut errs = Vec::new();
    for (ci, label) in clean_labels.iter().enumerate() {
        if clean_true[ci] > clean_false[ci] {
            errs.push(format!(
                "G3.5b clean Split count regression on {label}: tax_true={} > tax_false={}",
                clean_true[ci], clean_false[ci]
            ));
        }
    }
    if noisy_true > noisy_false {
        errs.push(format!(
            "G3.5b pooled noisy Split count regression: tax_true={noisy_true} > tax_false={noisy_false}"
        ));
    }
    if all_true > all_false {
        errs.push(format!(
            "G3.5b pooled overall Split count regression: tax_true={all_true} > tax_false={all_false}"
        ));
    }
    if inj_paired > 0 && inj_exc_true < inj_exc_false {
        errs.push(format!(
            "G3.5b paired injection Split-tax excision regression: tax_true={inj_exc_true}/{inj_paired} < tax_false={inj_exc_false}/{inj_paired}"
        ));
    }
    errs
}

fn evaluate_seed_range_parallel(seed_offset: u32, total_seeds: u32) -> Vec<SeedAllGatesRaw> {
    const SEED_CHUNK_SIZE: usize = 20;

    let n_seeds = total_seeds.max(1) as usize;
    let tasks = injection_task_panel();
    let spont_specs = spontaneous_panel();
    let clean_spec_count = spont_specs.iter().filter(|s| s.epsilon == 0.0).count();
    let (settle_groups, vle_jobs, base_jobs, split_false_jobs) =
        build_settle_groups_and_jobs(&tasks);

    let max_threads = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1)
        .max(1);

    // Precompute certified exact-synthesis minima once per unique `(gen_cfg, width, wire_delay)` in parallel.
    let mut n_min_cache: BTreeMap<(String, u8, u8), usize> = BTreeMap::new();
    let mut unique_specs: Vec<&SpontaneousSpec> = Vec::new();
    let mut spec_to_unique = Vec::with_capacity(spont_specs.len());
    for spec in &spont_specs {
        let key = (
            format!("{:?}", spec.gen_cfg),
            spec.width.bits(),
            u8::from(spec.wire_delay == WireDelayMode::TicksPerHop),
        );
        let u_idx = if let Some(&cached) = n_min_cache.get(&key) {
            cached
        } else {
            let next = unique_specs.len();
            unique_specs.push(spec);
            n_min_cache.insert(key, next);
            next
        };
        spec_to_unique.push(u_idx);
    }
    let unique_slots: Vec<Mutex<Option<Option<usize>>>> =
        (0..unique_specs.len()).map(|_| Mutex::new(None)).collect();
    let next_nmin = AtomicUsize::new(0);
    std::thread::scope(|s| {
        let t_count = max_threads.min(unique_specs.len()).max(1);
        for _ in 0..t_count {
            let next_ref = &next_nmin;
            let unique_ref = &unique_specs;
            let slots_ref = &unique_slots;
            s.spawn(move || {
                loop {
                    let i = next_ref.fetch_add(1, Ordering::Relaxed);
                    if i >= unique_ref.len() {
                        break;
                    }
                    let val = compute_certified_n_min(unique_ref[i]);
                    *slots_ref[i].lock().expect("nmin lock") = Some(val);
                }
            });
        }
    });
    let unique_n_mins: Vec<Option<usize>> = unique_slots
        .into_iter()
        .map(|m| m.into_inner().expect("nmin lock").expect("nmin computed"))
        .collect();
    let spont_n_mins: Vec<Option<usize>> = spec_to_unique
        .into_iter()
        .map(|u| unique_n_mins[u])
        .collect();

    // Analytical G3.5(b) Split pricing check on canonical Split-bearing circuit (`T1_PeriodicMotif`).
    let mut canonical_split_circ = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);
    build_autonomous_ring(&mut canonical_split_circ, 3, &[0, 1, 1], 0);
    let canonical_split_tax = verify_analytical_split_pricing_theorem(&canonical_split_circ)
        .filter(|&bits| bits >= G35B_MIN_SPLIT_TAX_BITS)
        .unwrap_or(0.0);

    let w_obs = observation_horizon(&EconomyConfig::default(), &GrowthConfig::default()) as u64;

    let mut all_results = Vec::with_capacity(n_seeds);

    for chunk_start in (0..n_seeds).step_by(SEED_CHUNK_SIZE) {
        let chunk_len = (n_seeds - chunk_start).min(SEED_CHUNK_SIZE);
        let chunk_offset = seed_offset.saturating_add(chunk_start as u32);

        // Stage 1a: Settle ONCE per clean / TPH / split-false `(seed, task, wire_delay, split_code_tax)`.
        // Reuse for `BaselineD1b1` via `set_excision_mode` iff no pre-injection pruning occurred;
        // otherwise settle separately in `BaselineD1b1` so trajectories stay 100% bit-identical.
        let mut prefix_items = Vec::new();
        let mut suffix_items = Vec::new();
        for si in 0..chunk_len {
            for (gi, g) in settle_groups.iter().enumerate() {
                if g.prefix_group_idx.is_some() {
                    suffix_items.push((si, gi));
                } else {
                    prefix_items.push((si, gi));
                }
            }
        }
        let vle_snap_slots: Vec<Vec<Mutex<Option<Arc<SettledSnapshot>>>>> = (0..chunk_len)
            .map(|_| (0..settle_groups.len()).map(|_| Mutex::new(None)).collect())
            .collect();
        let base_snap_slots: Vec<Vec<Mutex<Option<Arc<SettledSnapshot>>>>> = (0..chunk_len)
            .map(|_| (0..settle_groups.len()).map(|_| Mutex::new(None)).collect())
            .collect();

        let next_prefix = AtomicUsize::new(0);
        let prefix_threads = max_threads.min(prefix_items.len()).max(1);
        std::thread::scope(|s| {
            for _ in 0..prefix_threads {
                let next_ref = &next_prefix;
                let items_ref = &prefix_items;
                let tasks_ref = &tasks;
                let groups_ref = &settle_groups;
                let vle_ref = &vle_snap_slots;
                let base_ref = &base_snap_slots;
                s.spawn(move || {
                    loop {
                        let idx = next_ref.fetch_add(1, Ordering::Relaxed);
                        if idx >= items_ref.len() {
                            break;
                        }
                        let (seed_idx, group_idx) = items_ref[idx];
                        let g = &groups_ref[group_idx];
                        let seed = u64::from(chunk_offset.saturating_add(seed_idx as u32));
                        let vle_snap = settle_injection_snapshot(
                            &tasks_ref[g.task_idx],
                            g.wire_delay,
                            g.epsilon,
                            ExcisionMode::VerifiedLocalExcision,
                            g.split_code_tax,
                            seed,
                        );
                        if g.need_baseline {
                            let can_reuse = vle_snap.settled
                                && vle_snap.learner.starvation_prune_events() == 0
                                && vle_snap.learner.idle_prune_events() == 0
                                && vle_snap.learner.local_excision_events() == 0;
                            let base_snap = if can_reuse {
                                let mut cloned = vle_snap.clone();
                                cloned.learner.set_excision_mode(ExcisionMode::BaselineD1b1);
                                cloned
                            } else {
                                settle_injection_snapshot(
                                    &tasks_ref[g.task_idx],
                                    g.wire_delay,
                                    g.epsilon,
                                    ExcisionMode::BaselineD1b1,
                                    g.split_code_tax,
                                    seed,
                                )
                            };
                            *base_ref[seed_idx][group_idx].lock().expect("base snap") =
                                Some(Arc::new(base_snap));
                        }
                        *vle_ref[seed_idx][group_idx].lock().expect("vle snap") =
                            Some(Arc::new(vle_snap));
                    }
                });
            }
        });

        // Stage 1b: Extend settled Comb eps=0.0 `t_lock` snapshots to Comb eps=0.05 (bit-identical).
        let next_suffix = AtomicUsize::new(0);
        let suffix_threads = max_threads.min(suffix_items.len()).max(1);
        std::thread::scope(|s| {
            for _ in 0..suffix_threads {
                let next_ref = &next_suffix;
                let items_ref = &suffix_items;
                let tasks_ref = &tasks;
                let groups_ref = &settle_groups;
                let vle_ref = &vle_snap_slots;
                s.spawn(move || {
                    loop {
                        let idx = next_ref.fetch_add(1, Ordering::Relaxed);
                        if idx >= items_ref.len() {
                            break;
                        }
                        let (seed_idx, group_idx) = items_ref[idx];
                        let g = &groups_ref[group_idx];
                        let prefix_idx = g.prefix_group_idx.expect("prefix group index");
                        let seed = u64::from(chunk_offset.saturating_add(seed_idx as u32));
                        let clean_arc = {
                            let guard = vle_ref[seed_idx][prefix_idx].lock().expect("clean lock");
                            guard.as_ref().expect("clean settled").clone()
                        };
                        let noisy_snap = settle_noisy_from_clean_prefix(
                            &clean_arc,
                            &tasks_ref[g.task_idx],
                            g.epsilon,
                            seed,
                        );
                        *vle_ref[seed_idx][group_idx].lock().expect("noisy lock") =
                            Some(Arc::new(noisy_snap));
                    }
                });
            }
        });

        let vle_snaps: Vec<Vec<Arc<SettledSnapshot>>> = vle_snap_slots
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|m| m.into_inner().expect("vle lock").expect("vle snap"))
                    .collect()
            })
            .collect();
        let base_snaps: Vec<Vec<Option<Arc<SettledSnapshot>>>> = base_snap_slots
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|m| m.into_inner().expect("base lock"))
                    .collect()
            })
            .collect();

        // Stage 2: Fan out all detour runs from cloned settled snapshots + G3.1 + spontaneous runs.
        #[derive(Clone, Copy)]
        enum WorkItem {
            G31 {
                seed_idx: usize,
            },
            VleInj {
                seed_idx: usize,
                job_idx: usize,
            },
            BaseInj {
                seed_idx: usize,
                job_idx: usize,
            },
            SplitFalseInj {
                seed_idx: usize,
                job_idx: usize,
            },
            Spont {
                seed_idx: usize,
                spec_idx: usize,
                tax_idx: usize,
            },
        }

        let mut work_items = Vec::new();
        for seed_idx in 0..chunk_len {
            work_items.push(WorkItem::G31 { seed_idx });
            for job_idx in 0..vle_jobs.len() {
                work_items.push(WorkItem::VleInj { seed_idx, job_idx });
            }
            for job_idx in 0..base_jobs.len() {
                work_items.push(WorkItem::BaseInj { seed_idx, job_idx });
            }
            for job_idx in 0..split_false_jobs.len() {
                work_items.push(WorkItem::SplitFalseInj { seed_idx, job_idx });
            }
            for spec_idx in 0..spont_specs.len() {
                for tax_idx in 0..2 {
                    work_items.push(WorkItem::Spont {
                        seed_idx,
                        spec_idx,
                        tax_idx,
                    });
                }
            }
        }

        let g31_slots: Vec<Mutex<Option<G31SeedResult>>> =
            (0..chunk_len).map(|_| Mutex::new(None)).collect();
        let vle_slots: Vec<Vec<Mutex<Option<InjCaseResult>>>> = (0..chunk_len)
            .map(|_| (0..vle_jobs.len()).map(|_| Mutex::new(None)).collect())
            .collect();
        let base_slots: Vec<Vec<Mutex<Option<InjCaseResult>>>> = (0..chunk_len)
            .map(|_| (0..base_jobs.len()).map(|_| Mutex::new(None)).collect())
            .collect();
        let sf_slots: Vec<Vec<Mutex<Option<InjCaseResult>>>> = (0..chunk_len)
            .map(|_| {
                (0..split_false_jobs.len())
                    .map(|_| Mutex::new(None))
                    .collect()
            })
            .collect();
        let spont_slots: Vec<Vec<Mutex<Option<SpontCaseResult>>>> = (0..chunk_len)
            .map(|_| {
                (0..spont_specs.len() * 2)
                    .map(|_| Mutex::new(None))
                    .collect()
            })
            .collect();

        let next_item = AtomicUsize::new(0);
        let stage2_threads = max_threads.min(work_items.len()).max(1);

        std::thread::scope(|s| {
            for _ in 0..stage2_threads {
                let next_ref = &next_item;
                let items_ref = &work_items;
                let tasks_ref = &tasks;
                let spont_ref = &spont_specs;
                let vle_ref = &vle_jobs;
                let base_ref = &base_jobs;
                let sf_ref = &split_false_jobs;
                let vle_snaps_ref = &vle_snaps;
                let base_snaps_ref = &base_snaps;
                let g31_ref = &g31_slots;
                let vle_slots_ref = &vle_slots;
                let base_slots_ref = &base_slots;
                let sf_slots_ref = &sf_slots;
                let spont_slots_ref = &spont_slots;
                s.spawn(move || {
                    loop {
                        let idx = next_ref.fetch_add(1, Ordering::Relaxed);
                        if idx >= items_ref.len() {
                            break;
                        }
                        match items_ref[idx] {
                            WorkItem::G31 { seed_idx } => {
                                let seed = chunk_offset.saturating_add(seed_idx as u32);
                                let res = evaluate_g31_seed(seed);
                                *g31_ref[seed_idx].lock().expect("g31 lock") = Some(res);
                            }
                            WorkItem::VleInj { seed_idx, job_idx } => {
                                let j = &vle_ref[job_idx];
                                let snap = (*vle_snaps_ref[seed_idx][j.settle_group_idx]).clone();
                                let res = run_detour_from_settled(
                                    snap,
                                    &tasks_ref[j.task_idx],
                                    j.wire_delay,
                                    j.injection_mode,
                                    j.detour_class,
                                );
                                *vle_slots_ref[seed_idx][job_idx].lock().expect("vle lock") =
                                    Some(res);
                            }
                            WorkItem::BaseInj { seed_idx, job_idx } => {
                                let j = &base_ref[job_idx];
                                let snap_arc = base_snaps_ref[seed_idx][j.settle_group_idx]
                                    .as_ref()
                                    .expect("baseline settled snapshot present");
                                let res = run_detour_from_settled(
                                    (**snap_arc).clone(),
                                    &tasks_ref[j.task_idx],
                                    j.wire_delay,
                                    j.injection_mode,
                                    j.detour_class,
                                );
                                *base_slots_ref[seed_idx][job_idx].lock().expect("base lock") =
                                    Some(res);
                            }
                            WorkItem::SplitFalseInj { seed_idx, job_idx } => {
                                let j = &sf_ref[job_idx];
                                let snap = (*vle_snaps_ref[seed_idx][j.settle_group_idx]).clone();
                                let res = run_detour_from_settled(
                                    snap,
                                    &tasks_ref[j.task_idx],
                                    j.wire_delay,
                                    j.injection_mode,
                                    j.detour_class,
                                );
                                *sf_slots_ref[seed_idx][job_idx].lock().expect("sf lock") =
                                    Some(res);
                            }
                            WorkItem::Spont {
                                seed_idx,
                                spec_idx,
                                tax_idx,
                            } => {
                                let seed = u64::from(chunk_offset.saturating_add(seed_idx as u32));
                                let split_tax = tax_idx == 0;
                                let res =
                                    run_spontaneous_case(&spont_ref[spec_idx], split_tax, seed);
                                *spont_slots_ref[seed_idx][spec_idx * 2 + tax_idx]
                                    .lock()
                                    .expect("spont lock") = Some(res);
                            }
                        }
                    }
                });
            }
        });

        for seed_idx in 0..chunk_len {
            let seed = chunk_offset.saturating_add(seed_idx as u32);
            let g31_res = g31_slots[seed_idx]
                .lock()
                .expect("g31 lock")
                .take()
                .expect("g31 computed");

            let vle_outs: Vec<InjCaseResult> = vle_slots[seed_idx]
                .iter()
                .map(|m| m.lock().expect("vle lock").take().expect("vle computed"))
                .collect();
            let base_outs: Vec<InjCaseResult> = base_slots[seed_idx]
                .iter()
                .map(|m| m.lock().expect("base lock").take().expect("base computed"))
                .collect();
            let sf_outs: Vec<InjCaseResult> = sf_slots[seed_idx]
                .iter()
                .map(|m| m.lock().expect("sf lock").take().expect("sf computed"))
                .collect();
            let spont_outs: Vec<SpontCaseResult> = spont_slots[seed_idx]
                .iter()
                .map(|m| {
                    m.lock()
                        .expect("spont lock")
                        .take()
                        .expect("spont computed")
                })
                .collect();

            let mut g32 = GateTally::default();
            let mut g33 = GateTally::default();
            let mut g32_obs = Vec::new();
            let mut g33_obs = Vec::new();

            for (job, out) in vle_jobs.iter().zip(vle_outs.iter()) {
                if out.tph_non_autonomous_skipped {
                    continue;
                }
                let (ok, detail) = evaluate_vle_detour_outcome(job, out, w_obs);
                let obs = TrajectoryDetourObservation {
                    settle_group_idx: job.settle_group_idx,
                    detour_class: job.detour_class,
                    functional_ok: ok,
                    excise_dt: out.excise_dt,
                    post_excision_kt_excess: out.post_excision_kt_excess,
                    post_excision_paired_kt_diff: out.post_excision_paired_kt_diff,
                    cliff: &out.cliff,
                    detail,
                };
                match job.injection_mode {
                    InjectionMode::Established => g32_obs.push(obs),
                    InjectionMode::Juvenile => g33_obs.push(obs),
                }
            }

            let label_for_group = |group_idx: usize| -> String {
                let g = &settle_groups[group_idx];
                let task = &tasks[g.task_idx];
                format!("{}({:?},eps={:.2})", task.label, g.wire_delay, g.epsilon)
            };
            let regime_for_group = |group_idx: usize| -> GateRegime {
                if settle_groups[group_idx].epsilon > 0.0 {
                    GateRegime::Noisy
                } else {
                    GateRegime::Clean
                }
            };
            let (g32_max_dt, g32_raw_detour_passed, g32_raw_detour_total) =
                record_injection_trajectory_units(
                    "G3.2",
                    seed,
                    settle_groups.len(),
                    g32_obs,
                    label_for_group,
                    regime_for_group,
                    &mut g32,
                );
            let (g33_max_dt, g33_raw_detour_passed, g33_raw_detour_total) =
                record_injection_trajectory_units(
                    "G3.3",
                    seed,
                    settle_groups.len(),
                    g33_obs,
                    label_for_group,
                    regime_for_group,
                    &mut g33,
                );

            // G3.5(a): Share CombMain Established clean VLE trajectories vs BaselineD1b1 (`D1`–`D6`, `D7b`).
            let mut g35a = GateTally::default();
            for (base_job, base_out) in base_jobs.iter().zip(base_outs.iter()) {
                let Some(vle_idx) = vle_jobs.iter().position(|j| {
                    j.task_idx == base_job.task_idx
                        && j.wire_delay == WireDelayMode::Combinational
                        && j.epsilon == 0.0
                        && j.injection_mode == InjectionMode::Established
                        && j.detour_class == base_job.detour_class
                }) else {
                    continue;
                };
                let vle_out = &vle_outs[vle_idx];
                if !vle_out.settled || !base_out.settled {
                    continue;
                }
                let mut combined_cliff = vle_out.cliff;
                combined_cliff.accumulate(&base_out.cliff);
                let baseline_failed_or_collapsed = !base_out.primary_restored || base_out.collapsed;
                if baseline_failed_or_collapsed {
                    let ok = vle_out.primary_restored
                        && !vle_out.collapsed
                        && vle_out.final_total_code_bits + 1e-9 < base_out.final_total_code_bits;
                    let task = &tasks[base_job.task_idx];
                    let detail = (!ok).then(|| {
                        format!(
                            "vle_restored={}, vle_bits={:.3}, base_bits={:.3}",
                            vle_out.primary_restored,
                            vle_out.final_total_code_bits,
                            base_out.final_total_code_bits
                        )
                    });
                    let g35a_task = format!("G3.5a:{}({})", task.label, base_job.detour_class);
                    g35a.record(
                        format!("{g35a_task}:seed={seed}"),
                        ok,
                        &combined_cliff,
                        detail,
                    );
                    g35a.record_tail_min(
                        &g35a_task,
                        "min_vle_code_bits_advantage",
                        base_out.final_total_code_bits - vle_out.final_total_code_bits,
                    );
                    g35a.record_tail_max(
                        &g35a_task,
                        "max_vle_final_code_bits",
                        vle_out.final_total_code_bits,
                    );
                }
            }

            // G3.4 + G3.5(b): Share `split_code_tax = true` spontaneous trajectories and paired injection ablation.
            let mut g34 = GateTally::default();
            let mut g35b = GateTally::default();
            let mut min_split_tax_delta = canonical_split_tax;
            let mut spont_clean_splits_true = Vec::with_capacity(clean_spec_count);
            let mut spont_clean_splits_false = Vec::with_capacity(clean_spec_count);
            let mut spont_noisy_splits_true = 0usize;
            let mut spont_noisy_splits_false = 0usize;
            let mut spont_all_splits_true = 0usize;
            let mut spont_all_splits_false = 0usize;

            for (si, spec) in spont_specs.iter().enumerate() {
                let r_true = &spont_outs[si * 2];
                let r_false = &spont_outs[si * 2 + 1];
                let is_noisy_constant = matches!(spec.gen_cfg, GeneratorConfig::T1_Constant { .. })
                    && spec.epsilon > 0.0;

                let non_inc_ok = r_true.locked
                    && r_true.n_primary_8000 <= r_true.n_primary_2000
                    && r_true.n_primary_2000 <= r_true.n_primary_at_settle;
                let comp_ok = r_true.max_comp_per_port_8000 <= 1;
                let noisy_const_ok = !is_noisy_constant || r_true.n_primary_8000 == 0;
                let is_noisy_periodic_t1 = spec.epsilon > 0.0
                    && matches!(
                        spec.gen_cfg,
                        GeneratorConfig::T1_Alternating { .. }
                            | GeneratorConfig::T1_PeriodicMotif { .. }
                    );
                let n_min_ok = spont_n_mins[si].is_none_or(|nm| {
                    let n_min_eff = if is_noisy_periodic_t1 { nm.max(3) } else { nm };
                    r_true.n_primary_8000 <= nm + 4 && r_true.n_primary_8000 <= 3 * n_min_eff.max(1)
                });
                let g34_ok = non_inc_ok && comp_ok && noisy_const_ok && n_min_ok;
                let g34_detail = (!g34_ok).then(|| {
                    format!(
                        "lock={:?}, settle=({},{}), 2k={}, 8k={}, comp_per_port={}, n_min={:?}",
                        r_true.t_lock,
                        r_true.t_settle,
                        r_true.n_primary_at_settle,
                        r_true.n_primary_2000,
                        r_true.n_primary_8000,
                        r_true.max_comp_per_port_8000,
                        spont_n_mins[si]
                    )
                });
                let g34_task = format!(
                    "G3.4:{}({:?},eps={:.2})",
                    spec.label, spec.wire_delay, spec.epsilon
                );
                let spont_regime = if spec.epsilon > 0.0 {
                    GateRegime::Noisy
                } else {
                    GateRegime::Clean
                };
                g34.record_with_regime(
                    format!("{g34_task}:seed={seed}"),
                    spont_regime,
                    g34_ok,
                    &r_true.cliff,
                    g34_detail,
                );
                g34.record_tail_max(
                    &g34_task,
                    "max_comp_per_port_8000",
                    r_true.max_comp_per_port_8000 as f64,
                );
                g34.record_tail_max(
                    &g34_task,
                    "max_n_primary_8000",
                    r_true.n_primary_8000 as f64,
                );
                g34.record_n8k_sample(&g34_task, r_true.n_primary_8000 as f64);
                if let Some(tl) = r_true.t_lock {
                    g34.record_tail_max(&g34_task, "max_t_lock", tl as f64);
                }

                // G3.5(b): exact pricing & solvability per-case + aggregate Split discipline.
                spont_all_splits_true += r_true.final_split_count;
                spont_all_splits_false += r_false.final_split_count;
                if spec.epsilon == 0.0 {
                    spont_clean_splits_true.push(r_true.final_split_count);
                    spont_clean_splits_false.push(r_false.final_split_count);
                } else {
                    spont_noisy_splits_true += r_true.final_split_count;
                    spont_noisy_splits_false += r_false.final_split_count;
                }

                let mut combined_cliff = r_true.cliff;
                combined_cliff.accumulate(&r_false.cliff);
                let per_split_tax = match r_true.analytical_split_tax_delta {
                    Some(delta) if r_true.final_split_count > 0 => {
                        let per_split = delta / (r_true.final_split_count as f64);
                        min_split_tax_delta = min_split_tax_delta.min(per_split);
                        per_split
                    }
                    _ => canonical_split_tax,
                };
                let split_count_delta =
                    (r_true.final_split_count as f64) - (r_false.final_split_count as f64);
                let analytical_ok = g35b_analytical_tax_ok(r_true);
                let solvability_ok = g35b_solvability_ok(spec.epsilon, r_true);
                let g35b_ok = canonical_split_tax >= G35B_MIN_SPLIT_TAX_BITS
                    && analytical_ok
                    && solvability_ok;
                let g35b_detail = (!g35b_ok).then(|| {
                    format!(
                        "solv={}, splits=({} vs {}), tax_delta={:?}, bits=({:.2} vs {:.2})",
                        solvability_ok,
                        r_true.final_split_count,
                        r_false.final_split_count,
                        r_true.analytical_split_tax_delta,
                        r_true.final_code_bits,
                        r_false.final_code_bits
                    )
                });
                let g35b_task = format!(
                    "G3.5b:{}({:?},eps={:.2})",
                    spec.label, spec.wire_delay, spec.epsilon
                );
                g35b.record_with_regime(
                    format!("{g35b_task}:seed={seed}"),
                    spont_regime,
                    g35b_ok,
                    &combined_cliff,
                    g35b_detail,
                );
                g35b.record_tail_min(&g35b_task, "min_split_tax_bits", per_split_tax);
                g35b.record_tail_max(&g35b_task, "max_split_count_delta", split_count_delta);
            }

            // Paired injection `split_code_tax` ablation (sharing tax=true VLE runs with tax=false).
            let mut inj_paired_count = 0usize;
            let mut inj_excised_tax_true = 0usize;
            let mut inj_excised_tax_false = 0usize;
            for (sf_job, sf_out) in split_false_jobs.iter().zip(sf_outs.iter()) {
                g35b.cliff_counters.accumulate(&sf_out.cliff);
                if sf_out.cliff.sprout_blocked_max_nodes > 0 {
                    g35b.excluded_cases += 1;
                    let task = &tasks[sf_job.task_idx];
                    g35b.failures.push(format!(
                        "G3.5b:inj_split_false:{}({:?},{}):seed={seed}: excluded (sprout_blocked_max_nodes={})",
                        task.label,
                        sf_job.injection_mode,
                        sf_job.detour_class,
                        sf_out.cliff.sprout_blocked_max_nodes
                    ));
                }
                let Some(vle_idx) = vle_jobs.iter().position(|j| {
                    j.task_idx == sf_job.task_idx
                        && j.wire_delay == WireDelayMode::Combinational
                        && j.epsilon == 0.0
                        && j.injection_mode == sf_job.injection_mode
                        && j.detour_class == sf_job.detour_class
                }) else {
                    continue;
                };
                let vle_out = &vle_outs[vle_idx];
                if vle_out.settled && sf_out.settled {
                    inj_paired_count += 1;
                    if !vle_out.collapsed && vle_out.primary_restored && vle_out.excise_dt.is_some()
                    {
                        inj_excised_tax_true += 1;
                    }
                    if !sf_out.collapsed && sf_out.primary_restored && sf_out.excise_dt.is_some() {
                        inj_excised_tax_false += 1;
                    }
                }
            }

            all_results.push(SeedAllGatesRaw {
                g31: g31_res.tally,
                g31_max_mdl_err: g31_res.max_mdl_err,
                g32,
                g32_max_excise_dt: g32_max_dt,
                g32_raw_detour_passed,
                g32_raw_detour_total,
                g33,
                g33_max_excise_dt: g33_max_dt,
                g33_raw_detour_passed,
                g33_raw_detour_total,
                g34,
                g35a,
                g35b,
                g35b_min_analytical_split_tax_bits: min_split_tax_delta,
                spont_clean_splits_true,
                spont_clean_splits_false,
                spont_noisy_splits_true,
                spont_noisy_splits_false,
                spont_all_splits_true,
                spont_all_splits_false,
                inj_paired_count,
                inj_excised_tax_true,
                inj_excised_tax_false,
            });
        }
    }

    all_results
}

fn aggregate_bank_slice(seed_offset: u32, outcomes: Vec<SeedAllGatesRaw>) -> BankAggRaw {
    let seeds_tested = u32::try_from(outcomes.len()).unwrap_or(u32::MAX);
    let mut g31 = GateTally::default();
    let mut g31_max_mdl_err = 0.0f64;
    let mut g32 = GateTally::default();
    let mut g32_max_excise_dt: Option<u64> = None;
    let mut g32_raw_detour_passed = 0usize;
    let mut g32_raw_detour_total = 0usize;
    let mut g33 = GateTally::default();
    let mut g33_max_excise_dt: Option<u64> = None;
    let mut g33_raw_detour_passed = 0usize;
    let mut g33_raw_detour_total = 0usize;
    let mut g34 = GateTally::default();
    let mut g35a = GateTally::default();
    let mut g35b = GateTally::default();
    let mut g35b_min_tax = f64::INFINITY;
    let mut spont_clean_splits_true: Vec<usize> = Vec::new();
    let mut spont_clean_splits_false: Vec<usize> = Vec::new();
    let mut spont_noisy_splits_true = 0usize;
    let mut spont_noisy_splits_false = 0usize;
    let mut spont_all_splits_true = 0usize;
    let mut spont_all_splits_false = 0usize;
    let mut inj_paired_count = 0usize;
    let mut inj_excised_tax_true = 0usize;
    let mut inj_excised_tax_false = 0usize;

    for r in outcomes {
        g31.merge_seed(r.g31);
        g31_max_mdl_err = g31_max_mdl_err.max(r.g31_max_mdl_err);
        g32.merge_seed(r.g32);
        if let Some(dt) = r.g32_max_excise_dt {
            g32_max_excise_dt = Some(g32_max_excise_dt.map_or(dt, |m| m.max(dt)));
        }
        g32_raw_detour_passed += r.g32_raw_detour_passed;
        g32_raw_detour_total += r.g32_raw_detour_total;
        g33.merge_seed(r.g33);
        if let Some(dt) = r.g33_max_excise_dt {
            g33_max_excise_dt = Some(g33_max_excise_dt.map_or(dt, |m| m.max(dt)));
        }
        g33_raw_detour_passed += r.g33_raw_detour_passed;
        g33_raw_detour_total += r.g33_raw_detour_total;
        g34.merge_seed(r.g34);
        g35a.merge_seed(r.g35a);
        g35b.merge_seed(r.g35b);
        g35b_min_tax = g35b_min_tax.min(r.g35b_min_analytical_split_tax_bits);
        if spont_clean_splits_true.is_empty() {
            spont_clean_splits_true = r.spont_clean_splits_true;
            spont_clean_splits_false = r.spont_clean_splits_false;
        } else {
            for (acc, v) in spont_clean_splits_true
                .iter_mut()
                .zip(r.spont_clean_splits_true)
            {
                *acc += v;
            }
            for (acc, v) in spont_clean_splits_false
                .iter_mut()
                .zip(r.spont_clean_splits_false)
            {
                *acc += v;
            }
        }
        spont_noisy_splits_true += r.spont_noisy_splits_true;
        spont_noisy_splits_false += r.spont_noisy_splits_false;
        spont_all_splits_true += r.spont_all_splits_true;
        spont_all_splits_false += r.spont_all_splits_false;
        inj_paired_count += r.inj_paired_count;
        inj_excised_tax_true += r.inj_excised_tax_true;
        inj_excised_tax_false += r.inj_excised_tax_false;
    }

    BankAggRaw {
        seed_offset,
        seeds_tested,
        g31,
        g31_max_mdl_err,
        g32,
        g32_max_excise_dt,
        g32_raw_detour_passed,
        g32_raw_detour_total,
        g33,
        g33_max_excise_dt,
        g33_raw_detour_passed,
        g33_raw_detour_total,
        g34,
        g35a,
        g35b,
        g35b_min_analytical_split_tax_bits: g35b_min_tax,
        spont_clean_splits_true,
        spont_clean_splits_false,
        spont_noisy_splits_true,
        spont_noisy_splits_false,
        spont_all_splits_true,
        spont_all_splits_false,
        inj_paired_count,
        inj_excised_tax_true,
        inj_excised_tax_false,
    }
}

/// Runs Milestone 3 verification gates `G3.1`–`G3.5` on a single bank `[seed_offset, seed_offset + seeds)`.
///
/// # Errors
///
/// Returns an error if the seed range overlaps a forbidden holdout range or M3 holdout without `--holdout`.
pub fn run_m3_verification_suite(
    seed_offset: u32,
    seeds: u32,
    allow_holdout: bool,
) -> Result<M3VerificationSummary, String> {
    let seeds_tested = seeds.max(1);
    check_m3_seed_range_holdout(seed_offset, seeds_tested, allow_holdout)?;
    let outcomes = evaluate_seed_range_parallel(seed_offset, seeds_tested);
    let mut agg = aggregate_bank_slice(seed_offset, outcomes);
    let clean_labels = clean_spont_spec_labels(&spontaneous_panel());
    let split_errs = evaluate_g35b_split_discipline(
        &clean_labels,
        &agg.spont_clean_splits_true,
        &agg.spont_clean_splits_false,
        agg.spont_noisy_splits_true,
        agg.spont_noisy_splits_false,
        agg.spont_all_splits_true,
        agg.spont_all_splits_false,
        agg.inj_paired_count,
        agg.inj_excised_tax_true,
        agg.inj_excised_tax_false,
    );
    agg.g35b.failures.extend(split_errs.iter().cloned());

    let g31_extra_ok = agg.g31_max_mdl_err <= G31_CONSERVATION_TOL_BITS_PER_SYMBOL;
    let g35b_extra_ok =
        agg.g35b_min_analytical_split_tax_bits >= G35B_MIN_SPLIT_TAX_BITS && split_errs.is_empty();

    let gates = vec![
        agg.g31.into_report(
            "G3.1",
            G31_TARGET_RATE,
            g31_extra_ok,
            Some(format!(
                "max_epistemic_mdl_err={:.3e} b/sym",
                agg.g31_max_mdl_err
            )),
        ),
        agg.g32.into_report(
            "G3.2",
            G32_TARGET_RATE,
            true,
            Some(format_excise_raw_summary(
                agg.g32_max_excise_dt,
                agg.g32_raw_detour_passed,
                agg.g32_raw_detour_total,
            )),
        ),
        agg.g33.into_report(
            "G3.3",
            G33_TARGET_RATE,
            true,
            Some(format_excise_raw_summary(
                agg.g33_max_excise_dt,
                agg.g33_raw_detour_passed,
                agg.g33_raw_detour_total,
            )),
        ),
        agg.g34.into_report("G3.4", G34_TARGET_RATE, true, None),
        agg.g35a.into_report("G3.5a", G35A_TARGET_RATE, true, None),
        agg.g35b.into_report(
            "G3.5b",
            G35B_TARGET_RATE,
            g35b_extra_ok,
            Some(format!(
                "min_analytical_split_tax={:.4} bits/Split, noisy_splits={}/{}, all_splits={}/{}, inj_excised={}/{} vs {}/{}",
                agg.g35b_min_analytical_split_tax_bits,
                agg.spont_noisy_splits_true,
                agg.spont_noisy_splits_false,
                agg.spont_all_splits_true,
                agg.spont_all_splits_false,
                agg.inj_excised_tax_true,
                agg.inj_paired_count,
                agg.inj_excised_tax_false,
                agg.inj_paired_count
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

    Ok(M3VerificationSummary {
        provenance: BuildProvenance::current(),
        seed_offset: agg.seed_offset,
        seeds_tested: agg.seeds_tested,
        gates,
        all_passed,
        excluded_cases,
        cliff_counters: total_cliff,
        g31_max_epistemic_mdl_err: agg.g31_max_mdl_err,
        g32_max_excise_dt: agg.g32_max_excise_dt,
        g33_max_excise_dt: agg.g33_max_excise_dt,
        g35b_min_analytical_split_tax_bits: agg.g35b_min_analytical_split_tax_bits,
    })
}

/// Evaluates `banks` consecutive seed banks on Milestone 3 gates `G3.1`–`G3.5` using
/// `evaluate_multi_bank_gate` (99% Clopper–Pearson lower bound + Pearson/permutation homogeneity).
///
/// # Errors
///
/// Returns an error if `banks == 0` or if `[seed_offset, seed_offset + banks * seeds_per_bank)`
/// overlaps a forbidden holdout range without authorization.
pub fn run_m3_bank_evaluation(
    seed_offset: u32,
    seeds_per_bank: u32,
    banks: u32,
    allow_holdout: bool,
) -> Result<M3BankReport, String> {
    if banks == 0 {
        return Err("banks count must be >= 1".to_owned());
    }
    let seeds_per_bank = seeds_per_bank.max(1);
    let total_seeds = seeds_per_bank.saturating_mul(banks);
    check_m3_seed_range_holdout(seed_offset, total_seeds, allow_holdout)?;

    let all_outcomes = evaluate_seed_range_parallel(seed_offset, total_seeds);
    let mut iter = all_outcomes.into_iter();
    let mut bank_aggs = Vec::with_capacity(banks as usize);
    for b in 0..banks {
        let bank_offset = seed_offset.saturating_add(b.saturating_mul(seeds_per_bank));
        let slice: Vec<_> = iter.by_ref().take(seeds_per_bank as usize).collect();
        bank_aggs.push(aggregate_bank_slice(bank_offset, slice));
    }

    let mut samples: [Vec<BankSample>; 6] = Default::default();
    let mut excl = [0usize; 6];
    let mut failures: [Vec<String>; 6] = Default::default();
    let mut bank_counts: [Vec<BankTaskCountMap>; 6] = Default::default();
    let mut bank_tails: [Vec<BankTailMetricMap>; 6] = Default::default();
    let mut pooled_dt_samples: [BTreeMap<String, Vec<f64>>; 6] = Default::default();
    let mut pooled_n8k_samples: [BTreeMap<String, Vec<f64>>; 6] = Default::default();
    let mut clean_conj_banks: [Vec<BankSample>; 6] = Default::default();
    let mut total_cliff = CliffCountersSummary::default();

    let mut g31_max_mdl_err = 0.0f64;
    let mut g32_max_dt: Option<u64> = None;
    let mut g32_raw_passed = 0usize;
    let mut g32_raw_total = 0usize;
    let mut g33_max_dt: Option<u64> = None;
    let mut g33_raw_passed = 0usize;
    let mut g33_raw_total = 0usize;
    let mut g35b_min_tax = f64::INFINITY;
    let mut pooled_clean_true: Vec<usize> = Vec::new();
    let mut pooled_clean_false: Vec<usize> = Vec::new();
    let mut pooled_noisy_true = 0usize;
    let mut pooled_noisy_false = 0usize;
    let mut pooled_all_true = 0usize;
    let mut pooled_all_false = 0usize;
    let mut pooled_inj_paired = 0usize;
    let mut pooled_inj_exc_true = 0usize;
    let mut pooled_inj_exc_false = 0usize;

    for (b_idx, agg) in bank_aggs.into_iter().enumerate() {
        g31_max_mdl_err = g31_max_mdl_err.max(agg.g31_max_mdl_err);
        if let Some(dt) = agg.g32_max_excise_dt {
            g32_max_dt = Some(g32_max_dt.map_or(dt, |m| m.max(dt)));
        }
        g32_raw_passed += agg.g32_raw_detour_passed;
        g32_raw_total += agg.g32_raw_detour_total;
        if let Some(dt) = agg.g33_max_excise_dt {
            g33_max_dt = Some(g33_max_dt.map_or(dt, |m| m.max(dt)));
        }
        g33_raw_passed += agg.g33_raw_detour_passed;
        g33_raw_total += agg.g33_raw_detour_total;
        g35b_min_tax = g35b_min_tax.min(agg.g35b_min_analytical_split_tax_bits);
        if pooled_clean_true.is_empty() {
            pooled_clean_true = agg.spont_clean_splits_true;
            pooled_clean_false = agg.spont_clean_splits_false;
        } else {
            for (acc, v) in pooled_clean_true
                .iter_mut()
                .zip(agg.spont_clean_splits_true)
            {
                *acc += v;
            }
            for (acc, v) in pooled_clean_false
                .iter_mut()
                .zip(agg.spont_clean_splits_false)
            {
                *acc += v;
            }
        }
        pooled_noisy_true += agg.spont_noisy_splits_true;
        pooled_noisy_false += agg.spont_noisy_splits_false;
        pooled_all_true += agg.spont_all_splits_true;
        pooled_all_false += agg.spont_all_splits_false;
        pooled_inj_paired += agg.inj_paired_count;
        pooled_inj_exc_true += agg.inj_excised_tax_true;
        pooled_inj_exc_false += agg.inj_excised_tax_false;

        let seed_offset = agg.seed_offset;
        let tallies = [agg.g31, agg.g32, agg.g33, agg.g34, agg.g35a, agg.g35b];
        for (gi, tally) in tallies.into_iter().enumerate() {
            samples[gi].push(tally.bank_sample());
            excl[gi] += tally.excluded_cases;
            total_cliff.accumulate(&tally.cliff_counters);
            if let Some(cb) = tally.clean_seed_bank() {
                clean_conj_banks[gi].extend(cb);
            }
            for (task, dts) in tally.task_dt_samples {
                pooled_dt_samples[gi].entry(task).or_default().extend(dts);
            }
            for (task, vals) in tally.task_n8k_samples {
                pooled_n8k_samples[gi].entry(task).or_default().extend(vals);
            }
            bank_counts[gi].push(tally.task_counts);
            bank_tails[gi].push(tally.task_tails);
            for f in tally.failures {
                if failures[gi].len() < 32 {
                    failures[gi].push(format!("bank {b_idx} (offset {seed_offset}): {f}"));
                }
            }
        }
    }

    let clean_labels = clean_spont_spec_labels(&spontaneous_panel());
    let split_errs = evaluate_g35b_split_discipline(
        &clean_labels,
        &pooled_clean_true,
        &pooled_clean_false,
        pooled_noisy_true,
        pooled_noisy_false,
        pooled_all_true,
        pooled_all_false,
        pooled_inj_paired,
        pooled_inj_exc_true,
        pooled_inj_exc_false,
    );
    for err in &split_errs {
        if failures[5].len() < 32 {
            failures[5].push(err.clone());
        }
    }

    let g31_extra_ok = g31_max_mdl_err <= G31_CONSERVATION_TOL_BITS_PER_SYMBOL;
    let g35b_extra_ok = g35b_min_tax >= G35B_MIN_SPLIT_TAX_BITS && split_errs.is_empty();

    let mb1 = evaluate_multi_bank_gate(&samples[0], G31_TARGET_RATE);
    let mb2 = evaluate_multi_bank_gate(&samples[1], G32_TARGET_RATE);
    let mb3 = evaluate_multi_bank_gate(&samples[2], G33_TARGET_RATE);
    let mb4 = evaluate_multi_bank_gate(&samples[3], G34_TARGET_RATE);
    let mb5a = evaluate_multi_bank_gate(&samples[4], G35A_TARGET_RATE);
    let mb5b = evaluate_multi_bank_gate(&samples[5], G35B_TARGET_RATE);

    let targets = [
        G31_TARGET_RATE,
        G32_TARGET_RATE,
        G33_TARGET_RATE,
        G34_TARGET_RATE,
        G35A_TARGET_RATE,
        G35B_TARGET_RATE,
    ];
    let mut strat: [Option<StratifiedGateAudit>; 6] = Default::default();
    for gi in 0..6 {
        let count_refs: Vec<&_> = bank_counts[gi].iter().collect();
        let tail_refs: Vec<&_> = bank_tails[gi].iter().collect();
        let overrides =
            build_m3_percentile_overrides(&pooled_dt_samples[gi], &pooled_n8k_samples[gi]);
        let conj_opt =
            (!clean_conj_banks[gi].is_empty()).then_some(clean_conj_banks[gi].as_slice());
        strat[gi] = Some(evaluate_stratified_from_bank_maps(
            &count_refs,
            &tail_refs,
            Some(&overrides),
            conj_opt,
            targets[gi],
            targets[gi],
            true,
        ));
    }
    let s1 = strat[0].take().expect("strat g31");
    let s2 = strat[1].take().expect("strat g32");
    let s3 = strat[2].take().expect("strat g33");
    let s4 = strat[3].take().expect("strat g34");
    let s5a = strat[4].take().expect("strat g35a");
    let s5b = strat[5].take().expect("strat g35b");

    let g31_exact_ok = mb1.pooled.total_trials > 0
        && mb1.pooled.total_successes == mb1.pooled.total_trials
        && excl[0] == 0
        && g31_extra_ok
        && s1.passed;
    let g35b_exact_ok = mb5b.pooled.total_trials > 0
        && mb5b.pooled.total_successes == mb5b.pooled.total_trials
        && excl[5] == 0
        && g35b_extra_ok
        && s5b.passed;

    let gates = vec![
        M3BankGateReport {
            gate: "G3.1".to_owned(),
            passed: g31_exact_ok,
            excluded_cases: excl[0],
            multi_bank: mb1,
            pooled_metric_name: Some("max_epistemic_mdl_err_b_per_sym".to_owned()),
            pooled_metric_value: Some(g31_max_mdl_err),
            pooled_metric_passed: g31_extra_ok,
            failures: std::mem::take(&mut failures[0]),
            stratified: s1,
        },
        M3BankGateReport {
            gate: "G3.2".to_owned(),
            passed: mb2.passed && s2.passed && excl[1] == 0,
            excluded_cases: excl[1],
            multi_bank: mb2,
            pooled_metric_name: Some(format_excise_raw_summary(
                g32_max_dt,
                g32_raw_passed,
                g32_raw_total,
            )),
            pooled_metric_value: g32_max_dt.map(|dt| dt as f64),
            pooled_metric_passed: true,
            failures: std::mem::take(&mut failures[1]),
            stratified: s2,
        },
        M3BankGateReport {
            gate: "G3.3".to_owned(),
            passed: mb3.passed && s3.passed && excl[2] == 0,
            excluded_cases: excl[2],
            multi_bank: mb3,
            pooled_metric_name: Some(format_excise_raw_summary(
                g33_max_dt,
                g33_raw_passed,
                g33_raw_total,
            )),
            pooled_metric_value: g33_max_dt.map(|dt| dt as f64),
            pooled_metric_passed: true,
            failures: std::mem::take(&mut failures[2]),
            stratified: s3,
        },
        M3BankGateReport {
            gate: "G3.4".to_owned(),
            passed: mb4.passed && s4.passed && excl[3] == 0,
            excluded_cases: excl[3],
            multi_bank: mb4,
            pooled_metric_name: None,
            pooled_metric_value: None,
            pooled_metric_passed: true,
            failures: std::mem::take(&mut failures[3]),
            stratified: s4,
        },
        M3BankGateReport {
            gate: "G3.5a".to_owned(),
            passed: mb5a.passed && s5a.passed && excl[4] == 0,
            excluded_cases: excl[4],
            multi_bank: mb5a,
            pooled_metric_name: None,
            pooled_metric_value: None,
            pooled_metric_passed: true,
            failures: std::mem::take(&mut failures[4]),
            stratified: s5a,
        },
        M3BankGateReport {
            gate: "G3.5b".to_owned(),
            passed: g35b_exact_ok,
            excluded_cases: excl[5],
            multi_bank: mb5b,
            pooled_metric_name: Some("min_analytical_split_tax_bits".to_owned()),
            pooled_metric_value: Some(g35b_min_tax),
            pooled_metric_passed: g35b_extra_ok,
            failures: std::mem::take(&mut failures[5]),
            stratified: s5b,
        },
    ];

    let excluded_cases: usize = excl.into_iter().sum();
    let all_passed = gates.iter().all(|g| g.passed) && excluded_cases == 0;

    Ok(M3BankReport {
        provenance: BuildProvenance::current(),
        seed_offset,
        seeds_per_bank,
        banks,
        gates,
        all_passed,
        excluded_cases,
        cliff_counters: total_cliff,
        g31_max_epistemic_mdl_err: g31_max_mdl_err,
        g32_max_excise_dt: g32_max_dt,
        g33_max_excise_dt: g33_max_dt,
        g35b_min_analytical_split_tax_bits: g35b_min_tax,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holdout_guards_reject_m1_m2a_m2b_m3_and_m4a_ranges() {
        // M1 (10_000..=10_019), closed M2a (30_000..=30_019), closed M2b (40_000..=40_019),
        // closed M3 v1 (50_000..=50_019), and M4a (60_000..=60_099) are unconditionally rejected
        // even with allow_holdout=true.
        for blocked in [10_000u32, 30_000, 40_000, 50_000, 60_000, 60_050] {
            assert!(check_m3_seed_range_holdout(blocked, 20, false).is_err());
            assert!(check_m3_seed_range_holdout(blocked, 20, true).is_err());
            assert!(check_m3_seed_range_holdout(blocked - 5, 10, true).is_err());
        }

        // Fresh M3 v2 holdout 50_100..=50_199 requires allow_holdout=true.
        let err = check_m3_seed_range_holdout(50_100, 20, false).unwrap_err();
        assert!(err.contains("50100..=50199"));
        assert!(check_m3_seed_range_holdout(50_090, 20, false).is_err());
        assert!(check_m3_seed_range_holdout(50_100, 100, true).is_ok());

        for safe in [
            0u32, 9_980, 10_020, 20_060, 20_100, 29_980, 30_020, 39_980, 40_020, 49_980, 50_020,
            50_200, 59_980, 60_100,
        ] {
            assert!(
                check_m3_seed_range_holdout(safe, 20, false).is_ok(),
                "expected safe seed range at {safe}"
            );
        }
    }

    #[test]
    fn g31_conservation_and_transparency_single_seed() {
        let res = evaluate_g31_seed(0);
        let report = res.tally.into_report("G3.1", G31_TARGET_RATE, true, None);
        assert!(
            report.passed,
            "G3.1 single seed failed: {:?}",
            report.failures
        );
        assert!(res.max_mdl_err <= G31_CONSERVATION_TOL_BITS_PER_SYMBOL);
    }

    #[test]
    fn g35b_analytical_split_tax_theorem_exact_and_non_trivial() {
        let mut circ = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);
        build_autonomous_ring(&mut circ, 3, &[0, 1, 1], 0);
        let per_split = verify_analytical_split_pricing_theorem(&circ)
            .expect("ring with Split satisfies exact SPLIT-tax identity");
        assert!(
            per_split >= G35B_MIN_SPLIT_TAX_BITS,
            "expected >= {G35B_MIN_SPLIT_TAX_BITS} bits/Split, got {per_split}"
        );
    }

    #[test]
    fn guardrail_binding_marks_gate_case_excluded_and_fails() {
        let mut tally = GateTally::default();
        let cliff = CliffCountersSummary {
            sprout_blocked_max_nodes: 1,
            ..CliffCountersSummary::default()
        };
        tally.record("test_excluded".to_owned(), true, &cliff, None);
        let report = tally.into_report("G3.2", G32_TARGET_RATE, true, None);
        assert_eq!(report.excluded_cases, 1);
        assert_eq!(report.passed_cases, 0);
        assert!(!report.passed);
    }

    #[test]
    fn hoisted_baseline_set_excision_mode_matches_fresh_baseline_settle() {
        let tasks = injection_task_panel();
        let task = &tasks[0];
        let vle_snap = settle_injection_snapshot(
            task,
            WireDelayMode::Combinational,
            0.0,
            ExcisionMode::VerifiedLocalExcision,
            true,
            0,
        );
        assert!(vle_snap.settled);
        assert_eq!(vle_snap.learner.starvation_prune_events(), 0);
        assert_eq!(vle_snap.learner.idle_prune_events(), 0);
        assert_eq!(vle_snap.learner.local_excision_events(), 0);

        let mut hoisted_snap = vle_snap.clone();
        hoisted_snap
            .learner
            .set_excision_mode(ExcisionMode::BaselineD1b1);
        let fresh_snap = settle_injection_snapshot(
            task,
            WireDelayMode::Combinational,
            0.0,
            ExcisionMode::BaselineD1b1,
            true,
            0,
        );

        for class in [DetourClass::D0Dead, DetourClass::D5CancelSub] {
            let hoisted_res = run_detour_from_settled(
                hoisted_snap.clone(),
                task,
                WireDelayMode::Combinational,
                InjectionMode::Established,
                class,
            );
            let fresh_res = run_detour_from_settled(
                fresh_snap.clone(),
                task,
                WireDelayMode::Combinational,
                InjectionMode::Established,
                class,
            );
            assert_eq!(hoisted_res.settled, fresh_res.settled);
            assert_eq!(
                hoisted_res.excised_within_w_obs,
                fresh_res.excised_within_w_obs
            );
            assert_eq!(hoisted_res.collapsed, fresh_res.collapsed);
            assert_eq!(hoisted_res.excise_dt, fresh_res.excise_dt);
            assert_eq!(
                hoisted_res.final_total_code_bits.to_bits(),
                fresh_res.final_total_code_bits.to_bits()
            );
            assert_eq!(hoisted_res.primary_restored, fresh_res.primary_restored);
            assert_eq!(
                hoisted_res.comparator_preserved,
                fresh_res.comparator_preserved
            );
            assert_eq!(
                hoisted_res.post_excision_clean_mismatches,
                fresh_res.post_excision_clean_mismatches
            );
            assert_eq!(
                hoisted_res.post_excision_kt_excess.to_bits(),
                fresh_res.post_excision_kt_excess.to_bits()
            );
            assert_eq!(
                hoisted_res.post_inj_clean_mismatches,
                fresh_res.post_inj_clean_mismatches
            );
            assert_eq!(hoisted_res.cliff, fresh_res.cliff);
        }
    }

    #[test]
    fn prefix_shared_noisy_settle_matches_fresh_noisy_settle() {
        let tasks = injection_task_panel();
        for label in ["T2_CrossSum(k=1)", "T2_MultiViewCoupled(k=1)"] {
            let task = tasks
                .iter()
                .find(|t| t.label == label)
                .expect("task present");
            let clean_snap = settle_injection_snapshot(
                task,
                WireDelayMode::Combinational,
                0.0,
                ExcisionMode::VerifiedLocalExcision,
                true,
                0,
            );
            let shared_snap = settle_noisy_from_clean_prefix(&clean_snap, task, 0.05, 0);
            let fresh_snap = settle_injection_snapshot(
                task,
                WireDelayMode::Combinational,
                0.05,
                ExcisionMode::VerifiedLocalExcision,
                true,
                0,
            );
            assert_eq!(shared_snap.settled, fresh_snap.settled);
            assert_eq!(shared_snap.env.tick(), fresh_snap.env.tick());
            assert_eq!(shared_snap.budget.consumed(), fresh_snap.budget.consumed());
            assert_eq!(
                shared_snap.learner.editable().circuit().nodes(),
                fresh_snap.learner.editable().circuit().nodes()
            );
            assert_eq!(
                shared_snap.learner.editable().circuit().edges(),
                fresh_snap.learner.editable().circuit().edges()
            );
            assert_eq!(
                shared_snap.learner.editable().state().registers(),
                fresh_snap.learner.editable().state().registers()
            );
            assert_eq!(
                shared_snap.learner.telemetry(),
                fresh_snap.learner.telemetry()
            );
            assert_eq!(
                shared_snap.pre_inj_total_code_bits.to_bits(),
                fresh_snap.pre_inj_total_code_bits.to_bits()
            );
        }
    }

    #[test]
    fn g31_conservation_handles_seed23_starved_unconnected_probe() {
        let res = evaluate_g31_seed(23);
        let report = res.tally.into_report("G3.1", G31_TARGET_RATE, true, None);
        assert!(report.passed, "G3.1 seed=23 failed: {:?}", report.failures);
        assert!(res.max_mdl_err <= G31_CONSERVATION_TOL_BITS_PER_SYMBOL);
    }

    #[test]
    fn g32_trajectory_unit_aggregation_reduces_detours_and_preserves_raw_counts() {
        let zero_cliff = CliffCountersSummary::default();
        let blocked_cliff = CliffCountersSummary {
            sprout_blocked_max_nodes: 1,
            ..CliffCountersSummary::default()
        };
        let obs = vec![
            TrajectoryDetourObservation {
                settle_group_idx: 0,
                detour_class: DetourClass::D0Dead,
                functional_ok: true,
                excise_dt: Some(12),
                post_excision_kt_excess: 0.001,
                post_excision_paired_kt_diff: 0.001,
                cliff: &zero_cliff,
                detail: None,
            },
            TrajectoryDetourObservation {
                settle_group_idx: 0,
                detour_class: DetourClass::D1AddZero,
                functional_ok: true,
                excise_dt: Some(18),
                post_excision_kt_excess: 0.002,
                post_excision_paired_kt_diff: 0.002,
                cliff: &zero_cliff,
                detail: None,
            },
            TrajectoryDetourObservation {
                settle_group_idx: 1,
                detour_class: DetourClass::D0Dead,
                functional_ok: true,
                excise_dt: Some(5),
                post_excision_kt_excess: 0.001,
                post_excision_paired_kt_diff: 0.001,
                cliff: &zero_cliff,
                detail: None,
            },
            TrajectoryDetourObservation {
                settle_group_idx: 1,
                detour_class: DetourClass::D7bAgreementGate,
                functional_ok: false,
                excise_dt: None,
                post_excision_kt_excess: f64::INFINITY,
                post_excision_paired_kt_diff: f64::INFINITY,
                cliff: &zero_cliff,
                detail: Some("paired_diff=inf".to_owned()),
            },
            TrajectoryDetourObservation {
                settle_group_idx: 2,
                detour_class: DetourClass::D0Dead,
                functional_ok: false,
                excise_dt: None,
                post_excision_kt_excess: f64::INFINITY,
                post_excision_paired_kt_diff: f64::INFINITY,
                cliff: &zero_cliff,
                detail: Some("miss".to_owned()),
            },
            TrajectoryDetourObservation {
                settle_group_idx: 2,
                detour_class: DetourClass::D2OneFire,
                functional_ok: true,
                excise_dt: Some(40),
                post_excision_kt_excess: 0.003,
                post_excision_paired_kt_diff: 0.003,
                cliff: &blocked_cliff,
                detail: None,
            },
        ];

        let mut tally = GateTally::default();
        let (max_dt, raw_passed, raw_total) = record_injection_trajectory_units(
            "G3.2",
            7,
            3,
            obs,
            |g| format!("Task{g}(Combinational,eps=0.05)"),
            |_| GateRegime::Noisy,
            &mut tally,
        );
        assert_eq!(max_dt, Some(40));
        assert_eq!(raw_passed, 3);
        assert_eq!(raw_total, 6);
        assert_eq!(tally.passed_cases, 1);
        assert_eq!(tally.failed_cases, 1);
        assert_eq!(tally.excluded_cases, 1);
        assert_eq!(tally.total_cases(), 3);
        assert_eq!(tally.bank_sample(), BankSample::new(1, 3));
        assert_eq!(
            format_excise_raw_summary(max_dt, raw_passed, raw_total),
            "max_excise_dt=40 ticks; raw_detours=3/6"
        );
    }

    #[test]
    fn g34_stratified_audit_rejects_concentrated_single_task_failure() {
        let mut agg = GateTally::default();
        let cliff = CliffCountersSummary::default();
        for seed in 0..20 {
            let mut per_seed = GateTally::default();
            for ti in 0..9 {
                per_seed.record_with_regime(
                    format!("G3.4:NoisyTask{ti}:seed={seed}"),
                    GateRegime::Noisy,
                    true,
                    &cliff,
                    None,
                );
            }
            per_seed.record_with_regime(
                format!("G3.4:BrokenNoisyTask:seed={seed}"),
                GateRegime::Noisy,
                false,
                &cliff,
                Some("comp_per_port=2".to_owned()),
            );
            per_seed.record_tail_max("G3.4:BrokenNoisyTask", "max_comp_per_port_8000", 2.0);
            agg.merge_seed(per_seed);
        }
        let report = agg.into_report("G3.4", G34_TARGET_RATE, true, None);
        assert!(
            !report.passed,
            "G3.4 must fail when one noisy task fails 0/20 despite 90% pooled"
        );
        assert!(!report.stratified.passed);
    }

    #[test]
    fn g35b_exempts_noisy_solvability_for_seed156_t2_cross_sum() {
        let spec = spontaneous_panel()
            .into_iter()
            .find(|s| {
                s.label == "T2_CrossSum(k=1)"
                    && s.wire_delay == WireDelayMode::Combinational
                    && s.epsilon > 0.0
            })
            .expect("noisy T2_CrossSum(k=1) spec present");
        let res = run_spontaneous_case(&spec, true, 156);
        assert_eq!(res.cliff.sprout_blocked_max_nodes, 0);
        assert!(g35b_solvability_ok(spec.epsilon, &res));
        assert!(g35b_analytical_tax_ok(&res));
    }

    #[test]
    fn multiview_k2_noisy_seeds_535_812_833_clear_stage3_without_max_nodes_block() {
        let spec = spontaneous_panel()
            .into_iter()
            .find(|s| {
                s.label == "T2_MultiViewCoupled(k=2)"
                    && s.wire_delay == WireDelayMode::Combinational
                    && s.epsilon > 0.0
            })
            .expect("noisy T2_MultiViewCoupled(k=2) spec present");
        for seed in [535u64, 812, 833] {
            for split_tax in [true, false] {
                let res = run_spontaneous_case(&spec, split_tax, seed);
                assert_eq!(
                    res.cliff.sprout_blocked_max_nodes, 0,
                    "seed={seed}, split_tax={split_tax} hit sprout_blocked_max_nodes"
                );
            }
        }
    }
}
