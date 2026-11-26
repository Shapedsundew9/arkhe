//! Milestone M4a confirmatory verification suite (`arkhe verify-m4a`, gates `G4A.1`–`G4A.7`).
//!
//! Shares the exact canonical paired 3-arm (`wake-only`, `S-A`, `S-B`) trajectory runner from
//! [`crate::verify_m4a::explore`] (zero duplicate simulation code), scores all six pre-registered
//! gates `G4A.1`–`G4A.6` via [`crate::stats::evaluate_stratified_from_bank_maps`] + Option D
//! `tail_metrics`, and emits the `G4A.7` (`H6`) pre-registered arm-selection decision.
//!
//! Prior holdout banks (`10000..=10019`, `30000..=30019`, `40000..=40019`, `50000..=50019`,
//! `50100..=50199`) and the consumed M4a single-shot holdout bank (`60000..=60099`, consumed once
//! at `0f62e2f` under S3 `#65`) remain permanently and unconditionally closed
//! (`allow_with_flag: false`), rejected even when `--holdout` is passed.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::config::{BuildProvenance, CliffCountersSummary};
use crate::env::FiniteStateSpec;
use crate::stats::{
    BankSample, BankTailMetricMap, BankTaskCountMap, CaseOutcome, GateRegime, HoldoutRangeSpec,
    RegimeStratumResult, StratifiedGateAudit, binomial_lower_tail_p_value,
    check_seed_range_holdouts, classify_case_outcome, evaluate_bank_homogeneity_with_alpha,
    evaluate_stratified_from_bank_maps, percentile_sorted, update_max_finite, update_min_finite,
};
use crate::verify_m4a::explore::{
    ArmRunOutcome, PairedSeedResult, family_name, kind_name, regime_name, run_paired_task_seed,
    sleep_over_wake_node_eval_ratio,
};
use crate::verify_m4a::oracle::PortLanguageOracle;
use crate::verify_m4a::tasks::{
    M4aRegime, M4aTaskFamily, M4aTaskKind, M4aTaskSpec, m4a0_task_registry,
};

/// Pre-registered epistemic MDL conservation residual limit (`G4A.1`, bits/symbol).
pub const G4A1_MAX_MDL_ERR: f64 = 1e-9;
/// Pre-registered one-sided sleep-over-wake node-evaluation ceiling (`G4A.1`, `R2`).
pub const G4A1_MAX_NODE_EVAL_RATIO: f64 = 1.0;
/// Exact target rate for `G4A.1` in both `Clean` and `Noisy` strata.
pub const G4A1_TARGET_RATE: f64 = 1.0;

/// Exact target rate for `G4A.2` in both `Clean` and `Noisy` strata.
pub const G4A2_TARGET_RATE: f64 = 1.0;

/// Pre-registered target rate for `G4A.3` in the gated `Noisy` positive-opportunity stratum.
pub const G4A3_TARGET_RATE: f64 = 0.55;
/// Pre-registered minimum per-task mean epistemic MDL advantage (`G4A.3`, bits).
pub const G4A3_MIN_MEAN_ADV_BITS: f64 = 15.0;
/// Gated `Noisy` positive-opportunity spontaneous tasks (`G4A.3`, `K2`).
pub const G4A3_GATED_TASKS: [&str; 3] = [
    "SP:Period6_LagRedundancy:k2:noisy",
    "SP:MV_Periodic:k1:noisy",
    "SP:MV_Periodic:k2:noisy",
];

/// Pre-registered `Clean` boundary KT regret ceiling (`G4A.4`, bits/symbol).
pub const G4A4_CLEAN_REGRET_MAX: f64 = 0.05;
/// Pre-registered `Noisy` per-seed boundary KT regret ceiling (`G4A.4`, bits/symbol).
pub const G4A4_NOISY_REGRET_MAX: f64 = 0.35;
/// Pre-registered `Noisy` per-task mean KT regret ceiling (`G4A.4`, bits/symbol).
pub const G4A4_NOISY_MEAN_REGRET_MAX: f64 = 0.10;
/// Pre-registered `Noisy` per-task 95th-percentile KT regret ceiling (`G4A.4`, bits/symbol).
pub const G4A4_NOISY_P95_REGRET_MAX: f64 = 0.35;
/// Exact target rate for `G4A.4` in `Clean`.
pub const G4A4_CLEAN_TARGET_RATE: f64 = 1.0;
/// Pre-registered target rate for `G4A.4` in `Noisy`.
pub const G4A4_NOISY_TARGET_RATE: f64 = 0.85;

/// Pre-registered `Clean` maximum spurious-cycle survival segments (`G4A.5`).
pub const G4A5_CLEAN_MAX_SURVIVAL_SEGMENTS: u64 = 1;
/// Pre-registered `Noisy` maximum spurious-cycle survival segments (`G4A.5`).
pub const G4A5_NOISY_MAX_SURVIVAL_SEGMENTS: u64 = 4;
/// Pre-registered `Noisy` per-task median survival segments ceiling (`G4A.5`).
pub const G4A5_NOISY_MEDIAN_SURVIVAL_SEGMENTS_MAX: f64 = 2.0;
/// Exact target rate for `G4A.5` in `Clean`.
pub const G4A5_CLEAN_TARGET_RATE: f64 = 1.0;
/// Pre-registered target rate for `G4A.5` in `Noisy`.
pub const G4A5_NOISY_TARGET_RATE: f64 = 0.75;

/// Exact target rate for `G4A.6` in `Clean`.
pub const G4A6_CLEAN_TARGET_RATE: f64 = 1.0;
/// Pre-registered target rate for `G4A.6` in `Noisy`.
pub const G4A6_NOISY_TARGET_RATE: f64 = 0.90;

/// Pre-registered `H6` equivalence margin (`G4A.7`, bits).
pub const G4A7_EQUIVALENCE_MARGIN_BITS: f64 = 10.0;

/// Holdout isolation table shared with `arkhe verify-m4a`.
///
/// All consumed holdouts—including M4a (`60000..=60099`, consumed once at `0f62e2f` under
/// `S3` `#65`)—are permanently and unconditionally closed (`allow_with_flag: false`).
pub const VERIFY_M4A_HOLDOUT_BANKS: &[HoldoutRangeSpec] = crate::stats::HOLDOUT_BANKS;

/// Guards `verify-m4a` runs against all permanently closed milestone holdout banks (`M1`–`M4a`).
///
/// # Errors
///
/// Returns a descriptive refusal string when `[seed_offset, seed_offset + total_seeds)` overlaps
/// any unconditionally closed bank (including `60000..=60099`), even when `allow_holdout == true`.
pub fn check_m4a_seed_range_holdout(
    seed_offset: u64,
    total_seeds: u64,
    allow_holdout: bool,
) -> Result<(), String> {
    check_seed_range_holdouts(
        "verify-m4a",
        seed_offset,
        total_seeds.max(1),
        allow_holdout,
        VERIFY_M4A_HOLDOUT_BANKS,
    )
}

/// Sleep arm selector (`S-A` consistency-only vs `S-B` consistency+replay).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SleepArm {
    Sa,
    Sb,
}

impl SleepArm {
    fn select<'a>(&self, pair: &'a PairedSeedResult) -> &'a ArmRunOutcome {
        match self {
            Self::Sa => &pair.sa,
            Self::Sb => &pair.sb,
        }
    }
}

/// Pre-registered `H6` (`G4A.7`) decision outcome comparing `S-A` against `S-B`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum H6Decision {
    SaSuperiorDropReplay,
    SaEquivalentDropReplay,
    SbSuperiorRetainReplay,
}

/// Per-task paired difference summary for `G4A.7` (`L_eval(S-B) - L_eval(S-A)`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct H6TaskDelta {
    pub task_id: String,
    pub family: String,
    pub regime: String,
    pub count: usize,
    pub mean_delta_6_sb_minus_sa_l_epi_bits: f64,
    pub median_delta_6_bits: f64,
    pub min_delta_6_bits: f64,
    pub max_delta_6_bits: f64,
    pub sa_mean_l_epi_advantage_bits: f64,
    pub sb_mean_l_epi_advantage_bits: f64,
}

/// Complete `G4A.7` (`H6`) decision report (`S-A` vs `S-B` across the 11 active falsification tasks).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct H6DecisionReport {
    pub gate: String,
    pub active_task_count: usize,
    pub total_paired_observations: usize,
    pub equivalence_margin_bits: f64,
    pub delta_6_sb_minus_sa_l_epi_bits: f64,
    pub decision: H6Decision,
    pub canonical_arm: SleepArm,
    pub canonical_arm_passed_all_gates: bool,
    pub per_task: Vec<H6TaskDelta>,
}

/// Per-arm stratified + Bonferroni + tail-metric audit for gates `G4A.3`–`G4A.6`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArmGateAudit {
    pub arm: SleepArm,
    pub passed: bool,
    pub stratified_passed: bool,
    pub bonferroni_half_alpha: Option<f64>,
    pub bonferroni_half_passed: bool,
    pub tail_checks_passed: bool,
    pub passed_cases: usize,
    pub failed_cases: usize,
    pub excluded_cases: usize,
    pub total_cases: usize,
    pub empirical_rate: f64,
    pub failures: Vec<String>,
    pub stratified: StratifiedGateAudit,
}

/// Confirmatory gate report for one Milestone M4a gate (`G4A.1`–`G4A.6`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M4aGateReport {
    pub gate: String,
    pub description: String,
    pub passed: bool,
    pub excluded_cases: usize,
    pub passed_cases: usize,
    pub failed_cases: usize,
    pub total_cases: usize,
    pub empirical_rate: f64,
    pub cliff_counters: CliffCountersSummary,
    pub failures: Vec<String>,
    pub stratified: StratifiedGateAudit,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sa_audit: Option<ArmGateAudit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sb_audit: Option<ArmGateAudit>,
}

/// Per-arm descriptive summary for one spontaneous (`SP`) task in `sp_characterisation` (`K2`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpArmCharacterisation {
    pub min_l_epi_advantage: f64,
    pub p05_l_epi_advantage: f64,
    pub mean_l_epi_advantage: f64,
    pub median_l_epi_advantage: f64,
    pub frac_l_epi_advantage_gt_0: f64,
    pub mean_eval_kt_regret: f64,
    pub p95_eval_kt_regret: f64,
    pub max_wrongful_excisions: f64,
    pub max_port_locks_lost: f64,
}

/// Spontaneous (`SP`) task characterisation entry covering all 16 `SP` tasks (3 gated + 13 non-gated per `K2`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpCharacterisationRow {
    pub task_id: String,
    pub kind: String,
    pub width_bits: u8,
    pub regime: String,
    pub gated_in_g4a3: bool,
    pub seeds_tested: usize,
    pub sa: SpArmCharacterisation,
    pub sb: SpArmCharacterisation,
}

/// Complete confirmatory verification report emitted by `arkhe verify-m4a`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M4aVerificationReport {
    pub provenance: BuildProvenance,
    pub seed_offset: u64,
    pub seeds_per_bank: usize,
    pub banks: usize,
    pub total_seeds: usize,
    pub threads_used: usize,
    pub wall_clock_seconds: f64,
    pub all_passed: bool,
    pub overall_passed: bool,
    pub excluded_cases: usize,
    pub cliff_counters: CliffCountersSummary,
    pub gates: Vec<M4aGateReport>,
    pub h6_decision: H6DecisionReport,
    pub sp_characterisation: Vec<SpCharacterisationRow>,
}

fn gate_regime(regime: M4aRegime) -> GateRegime {
    match regime {
        M4aRegime::Clean => GateRegime::Clean,
        M4aRegime::Noisy => GateRegime::Noisy,
    }
}

fn max_cliff_two(a: &CliffCountersSummary, b: &CliffCountersSummary) -> CliffCountersSummary {
    CliffCountersSummary {
        reserve_clipped_count: a.reserve_clipped_count.max(b.reserve_clipped_count),
        reserve_clipped_bits: a.reserve_clipped_bits.max(b.reserve_clipped_bits),
        reserve_mismatch_floored: a.reserve_mismatch_floored.max(b.reserve_mismatch_floored),
        sprout_blocked_max_nodes: a.sprout_blocked_max_nodes.max(b.sprout_blocked_max_nodes),
        candidate_undecidable: a.candidate_undecidable.max(b.candidate_undecidable),
        candidate_list_truncated: a.candidate_list_truncated.max(b.candidate_list_truncated),
        lag_horizon_bound: a.lag_horizon_bound.max(b.lag_horizon_bound),
        ring_period_bound: a.ring_period_bound.max(b.ring_period_bound),
        lag_deepening_steps: a.lag_deepening_steps.max(b.lag_deepening_steps),
        sprout_budget_exhausted: a.sprout_budget_exhausted.max(b.sprout_budget_exhausted),
    }
}

fn max_cliff_pair(pair: &PairedSeedResult) -> CliffCountersSummary {
    let ab = max_cliff_two(&pair.sa.cliff, &pair.sb.cliff);
    max_cliff_two(&pair.wake.cliff, &ab)
}

fn ledger_diff(arm: &ArmRunOutcome, task_horizon: u64) -> u64 {
    let target_ticks = if arm.pl_settled { task_horizon } else { 0 };
    arm.episode_ticks.abs_diff(target_ticks)
}

fn i2_arm_ok(
    task: &M4aTaskSpec,
    pre_ok: bool,
    no_onset_exact: Option<bool>,
    arm: &ArmRunOutcome,
) -> bool {
    let ct3_ok = task.kind != M4aTaskKind::Ct3NoComparator
        || arm.sleep_telemetry.sleep_segments_executed == 0;
    pre_ok && no_onset_exact.unwrap_or(true) && ct3_ok
}

fn truncate_failures(failures: &mut Vec<String>) {
    failures.sort();
    if failures.len() > 64 {
        let extra = failures.len() - 64;
        failures.truncate(64);
        failures.push(format!("... ({extra} additional failures truncated)"));
    }
}

fn sorted_finite(values: &[f64]) -> Vec<f64> {
    let mut out: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    out.sort_by(f64::total_cmp);
    out
}

fn mean_finite(sorted: &[f64]) -> f64 {
    if sorted.is_empty() {
        0.0
    } else {
        sorted.iter().sum::<f64>() / (sorted.len() as f64)
    }
}

fn record_common_tail_metrics(
    map: &mut BTreeMap<String, f64>,
    cliff: &CliffCountersSummary,
    suspended_ticks: u64,
    t_onset: Option<u64>,
    facts_forgone: u64,
) {
    update_max_finite(
        map,
        "sprout_blocked_max_nodes",
        cliff.sprout_blocked_max_nodes as f64,
    );
    update_max_finite(
        map,
        "sprout_budget_exhausted",
        cliff.sprout_budget_exhausted as f64,
    );
    update_max_finite(
        map,
        "max_suspended_comparator_ticks",
        suspended_ticks as f64,
    );
    update_max_finite(map, "max_t_onset", t_onset.unwrap_or(0) as f64);
    update_max_finite(map, "max_facts_forgone", facts_forgone as f64);
}

fn record_sp_advantage_overrides(
    map: &mut BTreeMap<String, f64>,
    raw: &[f64],
    include_mean_med: bool,
) {
    let sorted = sorted_finite(raw);
    map.insert(
        "p05_l_epi_advantage".to_owned(),
        percentile_sorted(&sorted, 0.05),
    );
    if include_mean_med {
        map.insert("mean_l_epi_advantage".to_owned(), mean_finite(&sorted));
        map.insert(
            "median_l_epi_advantage".to_owned(),
            percentile_sorted(&sorted, 0.50),
        );
    }
}

/// Shared multi-bank accumulator for `Clean` / `Noisy` stratified gates (`G4A.1`–`G4A.6`).
struct BankGateCollector {
    bank_counts: Vec<BankTaskCountMap>,
    bank_tails: Vec<BankTailMetricMap>,
    clean_conj_banks: Vec<BankSample>,
    passed_cases: usize,
    failed_cases: usize,
    excluded_cases: usize,
    cliff_total: CliffCountersSummary,
    failures: Vec<String>,
}

impl BankGateCollector {
    fn new(banks: usize) -> Self {
        Self {
            bank_counts: vec![BankTaskCountMap::new(); banks],
            bank_tails: vec![BankTailMetricMap::new(); banks],
            clean_conj_banks: Vec::with_capacity(banks),
            passed_cases: 0,
            failed_cases: 0,
            excluded_cases: 0,
            cliff_total: CliffCountersSummary::default(),
            failures: Vec::new(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn record_case(
        &mut self,
        bank: usize,
        task: &M4aTaskSpec,
        seed: u64,
        tag: &str,
        functional_pass: bool,
        cliff: &CliffCountersSummary,
        fail_detail: &str,
        seed_clean_ok: &mut bool,
    ) -> &mut BTreeMap<String, f64> {
        self.cliff_total.accumulate(cliff);
        let outcome = classify_case_outcome(functional_pass, cliff);
        let entry = self.bank_counts[bank]
            .entry((task.id.clone(), gate_regime(task.regime)))
            .or_insert((0, 0));
        entry.1 += 1;
        match outcome {
            CaseOutcome::Passed => {
                self.passed_cases += 1;
                entry.0 += 1;
            }
            CaseOutcome::Failed => {
                self.failed_cases += 1;
                if task.regime == M4aRegime::Clean {
                    *seed_clean_ok = false;
                }
                self.failures
                    .push(format!("{}:seed={seed}{tag}: {fail_detail}", task.id));
            }
            CaseOutcome::Excluded => {
                self.excluded_cases += 1;
                if task.regime == M4aRegime::Clean {
                    *seed_clean_ok = false;
                }
                self.failures.push(format!(
                    "{}:seed={seed}{tag}: excluded (sprout_blocked_max_nodes={})",
                    task.id, cliff.sprout_blocked_max_nodes
                ));
            }
        }
        self.bank_tails[bank].entry(task.id.clone()).or_default()
    }

    fn finish_clean_bank(&mut self, succ: usize, trials: usize) {
        self.clean_conj_banks.push(BankSample::new(succ, trials));
    }

    const fn total_cases(&self) -> usize {
        self.passed_cases + self.failed_cases + self.excluded_cases
    }

    fn empirical_rate(&self) -> f64 {
        let total = self.total_cases();
        if total == 0 {
            0.0
        } else {
            (self.passed_cases as f64) / (total as f64)
        }
    }

    fn evaluate_stratified(
        &self,
        pooled_overrides: Option<&BankTailMetricMap>,
        include_clean_conj: bool,
        clean_target_rate: f64,
        noisy_target_rate: f64,
    ) -> StratifiedGateAudit {
        let count_refs: Vec<&BankTaskCountMap> = self.bank_counts.iter().collect();
        let tail_refs: Vec<&BankTailMetricMap> = self.bank_tails.iter().collect();
        let conj_opt = include_clean_conj.then_some(self.clean_conj_banks.as_slice());
        evaluate_stratified_from_bank_maps(
            &count_refs,
            &tail_refs,
            pooled_overrides,
            conj_opt,
            clean_target_rate,
            noisy_target_rate,
            true,
        )
    }

    fn into_arm_audit(
        mut self,
        arm: SleepArm,
        stratified: StratifiedGateAudit,
        bonf_alpha: Option<f64>,
        bonf_ok: bool,
        tail_ok: bool,
    ) -> (ArmGateAudit, CliffCountersSummary) {
        truncate_failures(&mut self.failures);
        let total_cases = self.total_cases();
        let empirical_rate = self.empirical_rate();
        let passed =
            total_cases > 0 && self.excluded_cases == 0 && stratified.passed && bonf_ok && tail_ok;
        (
            ArmGateAudit {
                arm,
                passed,
                stratified_passed: stratified.passed,
                bonferroni_half_alpha: bonf_alpha,
                bonferroni_half_passed: bonf_ok,
                tail_checks_passed: tail_ok,
                passed_cases: self.passed_cases,
                failed_cases: self.failed_cases,
                excluded_cases: self.excluded_cases,
                total_cases,
                empirical_rate,
                failures: self.failures,
                stratified,
            },
            self.cliff_total,
        )
    }
}

fn check_bonferroni_half_for_stratum(
    stratum: Option<&RegimeStratumResult>,
    bank_counts: &[BankTaskCountMap],
    target_rate: f64,
    alpha_half: f64,
    failures: &mut Vec<String>,
) -> bool {
    let Some(st) = stratum else {
        return false;
    };
    let mut ok = true;
    for task_res in &st.tasks {
        let p_binom = binomial_lower_tail_p_value(
            task_res.total_successes,
            task_res.total_trials,
            target_rate,
        );
        let task_banks: Vec<BankSample> = bank_counts
            .iter()
            .map(|bm| {
                let (s, n) = bm
                    .get(&(task_res.task_label.clone(), st.regime))
                    .copied()
                    .unwrap_or((0, 0));
                BankSample::new(s, n)
            })
            .collect();
        let hom = evaluate_bank_homogeneity_with_alpha(&task_banks, alpha_half);
        if p_binom < alpha_half || !hom.passes_homogeneity {
            ok = false;
            failures.push(format!(
                "{}: Bonferroni/2 FWER failed (alpha={alpha_half:.6}, p_binom={p_binom:.6}, p_hom={:.6})",
                task_res.task_label, hom.p_value
            ));
        }
    }
    ok
}

fn tail_val(t: &crate::stats::PerTaskGateResult, key: &str) -> Option<f64> {
    t.tail_metrics.get(key).copied().filter(|v| v.is_finite())
}

fn check_tail_stratum<F>(
    stratum: Option<&RegimeStratumResult>,
    label: &str,
    failures: &mut Vec<String>,
    predicate: F,
) -> bool
where
    F: Fn(&crate::stats::PerTaskGateResult) -> bool,
{
    let Some(st) = stratum else {
        failures.push(format!("{label}: missing required stratum"));
        return false;
    };
    let mut ok = true;
    for t in &st.tasks {
        let blocked_ok = tail_val(t, "sprout_blocked_max_nodes").is_some_and(|v| v == 0.0);
        if !(blocked_ok && predicate(t)) {
            ok = false;
            failures.push(format!(
                "{} ({label}): tail bound failed ({:?})",
                t.task_label, t.tail_metrics
            ));
        }
    }
    ok
}

fn max_pair_onset_and_diag(pair: &PairedSeedResult) -> (u64, Option<u64>, u64) {
    let susp = pair
        .sa
        .sleep_telemetry
        .sleep_comparator_suspended_ungrounded
        .max(
            pair.sb
                .sleep_telemetry
                .sleep_comparator_suspended_ungrounded,
        );
    let t_on = match (pair.sa.first_onset_wake_tick, pair.sb.first_onset_wake_tick) {
        (Some(a), Some(b_on)) => Some(a.max(b_on)),
        (Some(a), None) => Some(a),
        (None, Some(b_on)) => Some(b_on),
        (None, None) => None,
    };
    let forgone = pair.sa.facts_forgone.max(pair.sb.facts_forgone);
    (susp, t_on, forgone)
}

#[derive(Clone, Copy)]
struct EvalSlice<'a> {
    tasks: &'a [M4aTaskSpec],
    results_by_task: &'a [Vec<PairedSeedResult>],
    banks: usize,
    seeds_per_bank: usize,
    seed_offset: u64,
}

fn evaluate_joint_exact_gate<F>(
    gate: &str,
    description: &str,
    ctx: EvalSlice<'_>,
    mut record_fn: F,
    tail_predicate: fn(&crate::stats::PerTaskGateResult) -> bool,
) -> M4aGateReport
where
    F: FnMut(&M4aTaskSpec, &PairedSeedResult, &mut BTreeMap<String, f64>) -> (bool, String),
{
    let mut col = BankGateCollector::new(ctx.banks);
    for b in 0..ctx.banks {
        let mut clean_succ = 0usize;
        for s_off in 0..ctx.seeds_per_bank {
            let seed_idx = b * ctx.seeds_per_bank + s_off;
            let seed = ctx.seed_offset.saturating_add(seed_idx as u64);
            let mut seed_clean_ok = true;
            for (t_idx, task) in ctx.tasks.iter().enumerate() {
                let pair = &ctx.results_by_task[t_idx][seed_idx];
                let cliff = max_cliff_pair(pair);
                let mut extra_tails = BTreeMap::new();
                let (functional_pass, detail) = record_fn(task, pair, &mut extra_tails);
                let tmap = col.record_case(
                    b,
                    task,
                    seed,
                    "",
                    functional_pass,
                    &cliff,
                    &detail,
                    &mut seed_clean_ok,
                );
                for (k, v) in extra_tails {
                    update_max_finite(tmap, &k, v);
                }
                let (susp, t_on, forgone) = max_pair_onset_and_diag(pair);
                record_common_tail_metrics(tmap, &cliff, susp, t_on, forgone);
            }
            if seed_clean_ok {
                clean_succ += 1;
            }
        }
        col.finish_clean_bank(clean_succ, ctx.seeds_per_bank);
    }

    let stratified = col.evaluate_stratified(None, true, 1.0, 1.0);
    let clean_ok = check_tail_stratum(
        stratified.clean.as_ref(),
        &format!("{gate}:Clean"),
        &mut col.failures,
        tail_predicate,
    );
    let noisy_ok = check_tail_stratum(
        stratified.noisy.as_ref(),
        &format!("{gate}:Noisy"),
        &mut col.failures,
        tail_predicate,
    );
    let tail_ok = clean_ok && noisy_ok;

    truncate_failures(&mut col.failures);
    let total_cases = col.total_cases();
    let empirical_rate = col.empirical_rate();
    let passed = total_cases > 0
        && col.excluded_cases == 0
        && col.passed_cases == total_cases
        && stratified.passed
        && tail_ok;

    M4aGateReport {
        gate: gate.to_owned(),
        description: description.to_owned(),
        passed,
        excluded_cases: col.excluded_cases,
        passed_cases: col.passed_cases,
        failed_cases: col.failed_cases,
        total_cases,
        empirical_rate,
        cliff_counters: col.cliff_total,
        failures: col.failures,
        stratified,
        sa_audit: None,
        sb_audit: None,
    }
}

fn evaluate_g4a1(ctx: EvalSlice<'_>) -> M4aGateReport {
    evaluate_joint_exact_gate(
        "G4A.1",
        "Conservation & Compute Accounting (I4, I5, R2, zero guardrail exclusions)",
        ctx,
        |task, pair, tails| {
            let max_mdl_err = pair
                .wake
                .epistemic_mdl_err
                .max(pair.sa.epistemic_mdl_err)
                .max(pair.sb.epistemic_mdl_err);
            let max_diff = ledger_diff(&pair.wake, task.horizon)
                .max(ledger_diff(&pair.sa, task.horizon))
                .max(ledger_diff(&pair.sb, task.horizon))
                .max(pair.wake.episode_ticks.abs_diff(pair.sa.episode_ticks))
                .max(pair.wake.episode_ticks.abs_diff(pair.sb.episode_ticks));
            let max_ratio = sleep_over_wake_node_eval_ratio(&pair.wake, &pair.sa)
                .max(sleep_over_wake_node_eval_ratio(&pair.wake, &pair.sb));
            update_max_finite(tails, "max_epistemic_mdl_err", max_mdl_err);
            update_max_finite(tails, "max_compute_ledger_diff", max_diff as f64);
            update_max_finite(tails, "max_node_eval_ratio_sleep_over_wake", max_ratio);
            let ok = max_mdl_err.is_finite()
                && max_mdl_err <= G4A1_MAX_MDL_ERR
                && max_diff == 0
                && max_ratio.is_finite()
                && max_ratio <= G4A1_MAX_NODE_EVAL_RATIO;
            (
                ok,
                format!(
                    "mdl_err={max_mdl_err:.3e}, ledger_diff={max_diff}, node_ratio={max_ratio:.6}"
                ),
            )
        },
        |t| {
            tail_val(t, "max_epistemic_mdl_err").is_some_and(|v| v <= G4A1_MAX_MDL_ERR)
                && tail_val(t, "max_compute_ledger_diff").is_some_and(|v| v == 0.0)
                && tail_val(t, "max_node_eval_ratio_sleep_over_wake")
                    .is_some_and(|v| v <= G4A1_MAX_NODE_EVAL_RATIO)
        },
    )
}

fn evaluate_g4a2(ctx: EvalSlice<'_>) -> M4aGateReport {
    evaluate_joint_exact_gate(
        "G4A.2",
        "Backward Invariance & Sleep Isolation (I1, I2, I6, I7)",
        ctx,
        |task, pair, tails| {
            let i2_sa = i2_arm_ok(
                task,
                pair.i2_sa_pre_onset_ok,
                pair.i2_sa_no_onset_exact,
                &pair.sa,
            );
            let i2_sb = i2_arm_ok(
                task,
                pair.i2_sb_pre_onset_ok,
                pair.i2_sb_no_onset_exact,
                &pair.sb,
            );
            let i2_viol = usize::from(!i2_sa) + usize::from(!i2_sb);
            let i6_viol = usize::from(!pair.sa.i6_ok) + usize::from(!pair.sb.i6_ok);
            let i7_viol = usize::from(!pair.sa.i7_ok) + usize::from(!pair.sb.i7_ok);
            update_max_finite(tails, "max_i2_violations", i2_viol as f64);
            update_max_finite(tails, "max_i6_violations", i6_viol as f64);
            update_max_finite(tails, "max_i7_violations", i7_viol as f64);
            (
                i2_viol == 0 && i6_viol == 0 && i7_viol == 0,
                format!("i2_viol={i2_viol}, i6_viol={i6_viol}, i7_viol={i7_viol}"),
            )
        },
        |t| {
            tail_val(t, "max_i2_violations").is_some_and(|v| v == 0.0)
                && tail_val(t, "max_i6_violations").is_some_and(|v| v == 0.0)
                && tail_val(t, "max_i7_violations").is_some_and(|v| v == 0.0)
        },
    )
}

fn audit_g4a3_arm(arm: SleepArm, ctx: EvalSlice<'_>) -> (ArmGateAudit, CliffCountersSummary) {
    let gated_indices: Vec<usize> = ctx
        .tasks
        .iter()
        .enumerate()
        .filter_map(|(i, t)| G4A3_GATED_TASKS.contains(&t.id.as_str()).then_some(i))
        .collect();
    let tag = format!(" ({arm:?})");
    let mut col = BankGateCollector::new(ctx.banks);
    let mut adv_samples: BTreeMap<String, Vec<f64>> = BTreeMap::new();

    for b in 0..ctx.banks {
        for s_off in 0..ctx.seeds_per_bank {
            let seed_idx = b * ctx.seeds_per_bank + s_off;
            let seed = ctx.seed_offset.saturating_add(seed_idx as u64);
            let mut dummy_clean = true;
            for &t_idx in &gated_indices {
                let task = &ctx.tasks[t_idx];
                let pair = &ctx.results_by_task[t_idx][seed_idx];
                let arm_out = arm.select(pair);
                let cliff = max_cliff_two(&pair.wake.cliff, &arm_out.cliff);

                let adv =
                    pair.wake.frozen.eval_l_epistemic_bits - arm_out.frozen.eval_l_epistemic_bits;
                let pass = adv.is_finite()
                    && adv > 0.0
                    && arm_out.wrongful_excisions == 0
                    && arm_out.oracle_correct_port_locks_lost == 0;
                let detail = format!(
                    "adv={adv:.3}b, we={}, locks_lost={}",
                    arm_out.wrongful_excisions, arm_out.oracle_correct_port_locks_lost
                );
                let tmap =
                    col.record_case(b, task, seed, &tag, pass, &cliff, &detail, &mut dummy_clean);
                if adv.is_finite() {
                    adv_samples.entry(task.id.clone()).or_default().push(adv);
                }
                update_min_finite(tmap, "min_l_epi_advantage", adv);
                update_max_finite(
                    tmap,
                    "max_wrongful_excisions",
                    arm_out.wrongful_excisions as f64,
                );
                update_max_finite(
                    tmap,
                    "max_port_locks_lost",
                    arm_out.oracle_correct_port_locks_lost as f64,
                );
                record_common_tail_metrics(
                    tmap,
                    &cliff,
                    arm_out
                        .sleep_telemetry
                        .sleep_comparator_suspended_ungrounded,
                    arm_out.first_onset_wake_tick,
                    arm_out.facts_forgone,
                );
            }
        }
    }

    let mut pooled_overrides = BankTailMetricMap::new();
    for (task_id, raw) in &adv_samples {
        let map = pooled_overrides.entry(task_id.clone()).or_default();
        record_sp_advantage_overrides(map, raw, true);
    }

    let stratified = col.evaluate_stratified(Some(&pooled_overrides), false, 1.0, G4A3_TARGET_RATE);
    let bonf_alpha = (0.01 / (gated_indices.len().max(1) as f64)) / 2.0;
    let bonf_ok = check_bonferroni_half_for_stratum(
        stratified.noisy.as_ref(),
        &col.bank_counts,
        G4A3_TARGET_RATE,
        bonf_alpha,
        &mut col.failures,
    );
    let tail_ok = check_tail_stratum(
        stratified.noisy.as_ref(),
        &format!("G4A.3:{arm:?}:Noisy"),
        &mut col.failures,
        |t| {
            tail_val(t, "mean_l_epi_advantage").is_some_and(|v| v >= G4A3_MIN_MEAN_ADV_BITS)
                && tail_val(t, "median_l_epi_advantage").is_some_and(|v| v > 0.0)
                && tail_val(t, "min_l_epi_advantage").is_some()
                && tail_val(t, "p05_l_epi_advantage").is_some()
        },
    );

    col.into_arm_audit(arm, stratified, Some(bonf_alpha), bonf_ok, tail_ok)
}

fn audit_g4a4_arm(arm: SleepArm, ctx: EvalSlice<'_>) -> (ArmGateAudit, CliffCountersSummary) {
    let gated_indices: Vec<usize> = ctx
        .tasks
        .iter()
        .enumerate()
        .filter_map(|(i, t)| (t.family != M4aTaskFamily::Pl).then_some(i))
        .collect();
    let tag = format!(" ({arm:?})");
    let mut col = BankGateCollector::new(ctx.banks);
    let mut regret_samples: BTreeMap<String, Vec<f64>> = BTreeMap::new();

    for b in 0..ctx.banks {
        let mut clean_succ = 0usize;
        for s_off in 0..ctx.seeds_per_bank {
            let seed_idx = b * ctx.seeds_per_bank + s_off;
            let seed = ctx.seed_offset.saturating_add(seed_idx as u64);
            let mut seed_clean_ok = true;
            for &t_idx in &gated_indices {
                let task = &ctx.tasks[t_idx];
                let pair = &ctx.results_by_task[t_idx][seed_idx];
                let arm_out = arm.select(pair);
                let cliff = max_cliff_two(&pair.wake.cliff, &arm_out.cliff);

                let max_regret = match task.regime {
                    M4aRegime::Clean => G4A4_CLEAN_REGRET_MAX,
                    M4aRegime::Noisy => G4A4_NOISY_REGRET_MAX,
                };
                let regret = arm_out.frozen.eval_kt_target - pair.wake.frozen.eval_kt_target;
                let regret_all = arm_out.frozen.eval_kt_all_ch - pair.wake.frozen.eval_kt_all_ch;
                let pass = regret.is_finite() && regret <= max_regret;
                let detail = format!("eval_kt_regret={regret:.6} > {max_regret:.3}");
                let tmap = col.record_case(
                    b,
                    task,
                    seed,
                    &tag,
                    pass,
                    &cliff,
                    &detail,
                    &mut seed_clean_ok,
                );
                if regret.is_finite() {
                    regret_samples
                        .entry(task.id.clone())
                        .or_default()
                        .push(regret);
                }
                update_max_finite(tmap, "max_eval_kt_regret", regret);
                update_max_finite(tmap, "max_eval_kt_regret_all_ch", regret_all);
                record_common_tail_metrics(
                    tmap,
                    &cliff,
                    arm_out
                        .sleep_telemetry
                        .sleep_comparator_suspended_ungrounded,
                    arm_out.first_onset_wake_tick,
                    arm_out.facts_forgone,
                );
            }
            if seed_clean_ok {
                clean_succ += 1;
            }
        }
        col.finish_clean_bank(clean_succ, ctx.seeds_per_bank);
    }

    let mut pooled_overrides = BankTailMetricMap::new();
    for (task_id, raw) in &regret_samples {
        let sorted = sorted_finite(raw);
        let map = pooled_overrides.entry(task_id.clone()).or_default();
        map.insert("mean_eval_kt_regret".to_owned(), mean_finite(&sorted));
        map.insert(
            "p95_eval_kt_regret".to_owned(),
            percentile_sorted(&sorted, 0.95),
        );
    }

    let stratified = col.evaluate_stratified(
        Some(&pooled_overrides),
        true,
        G4A4_CLEAN_TARGET_RATE,
        G4A4_NOISY_TARGET_RATE,
    );
    let clean_ok = check_tail_stratum(
        stratified.clean.as_ref(),
        &format!("G4A.4:{arm:?}:Clean"),
        &mut col.failures,
        |t| tail_val(t, "max_eval_kt_regret").is_some_and(|v| v <= G4A4_CLEAN_REGRET_MAX),
    );
    let noisy_ok = check_tail_stratum(
        stratified.noisy.as_ref(),
        &format!("G4A.4:{arm:?}:Noisy"),
        &mut col.failures,
        |t| {
            tail_val(t, "mean_eval_kt_regret").is_some_and(|v| v <= G4A4_NOISY_MEAN_REGRET_MAX)
                && tail_val(t, "p95_eval_kt_regret").is_some_and(|v| v <= G4A4_NOISY_P95_REGRET_MAX)
        },
    );

    col.into_arm_audit(arm, stratified, None, true, clean_ok && noisy_ok)
}

fn audit_g4a5_arm(arm: SleepArm, ctx: EvalSlice<'_>) -> (ArmGateAudit, CliffCountersSummary) {
    let gated_indices: Vec<usize> = ctx
        .tasks
        .iter()
        .enumerate()
        .filter_map(|(i, t)| (t.family == M4aTaskFamily::Pl).then_some(i))
        .collect();
    let noisy_task_count = gated_indices
        .iter()
        .filter(|&&i| ctx.tasks[i].regime == M4aRegime::Noisy)
        .count()
        .max(1);
    let tag = format!(" ({arm:?})");
    let mut col = BankGateCollector::new(ctx.banks);
    let mut surv_samples: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut exc_counts: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut ext_counts: BTreeMap<String, (usize, usize)> = BTreeMap::new();

    for b in 0..ctx.banks {
        let mut clean_succ = 0usize;
        for s_off in 0..ctx.seeds_per_bank {
            let seed_idx = b * ctx.seeds_per_bank + s_off;
            let seed = ctx.seed_offset.saturating_add(seed_idx as u64);
            let mut seed_clean_ok = true;
            for &t_idx in &gated_indices {
                let task = &ctx.tasks[t_idx];
                let pair = &ctx.results_by_task[t_idx][seed_idx];
                let arm_out = arm.select(pair);
                let cliff = max_cliff_two(&pair.wake.cliff, &arm_out.cliff);

                let max_surv = match task.regime {
                    M4aRegime::Clean => G4A5_CLEAN_MAX_SURVIVAL_SEGMENTS,
                    M4aRegime::Noisy => G4A5_NOISY_MAX_SURVIVAL_SEGMENTS,
                };
                let pass = arm_out.pl_settled
                    && (arm_out.pl_excised || arm_out.pl_spurious_extinguished)
                    && arm_out.pl_spurious_survival_segments <= max_surv
                    && arm_out.wrongful_excisions == 0;
                let detail = format!(
                    "settled={}, excised={}, ext={}, surv={}, we={}",
                    arm_out.pl_settled,
                    arm_out.pl_excised,
                    arm_out.pl_spurious_extinguished,
                    arm_out.pl_spurious_survival_segments,
                    arm_out.wrongful_excisions
                );
                let tmap = col.record_case(
                    b,
                    task,
                    seed,
                    &tag,
                    pass,
                    &cliff,
                    &detail,
                    &mut seed_clean_ok,
                );
                surv_samples
                    .entry(task.id.clone())
                    .or_default()
                    .push(arm_out.pl_spurious_survival_segments as f64);
                let e1 = exc_counts.entry(task.id.clone()).or_insert((0, 0));
                e1.1 += 1;
                e1.0 += usize::from(arm_out.pl_excised);
                let e2 = ext_counts.entry(task.id.clone()).or_insert((0, 0));
                e2.1 += 1;
                e2.0 += usize::from(arm_out.pl_spurious_extinguished);

                update_max_finite(
                    tmap,
                    "max_spurious_survival_segments",
                    arm_out.pl_spurious_survival_segments as f64,
                );
                update_max_finite(
                    tmap,
                    "max_wrongful_excisions",
                    arm_out.wrongful_excisions as f64,
                );
                record_common_tail_metrics(
                    tmap,
                    &cliff,
                    arm_out
                        .sleep_telemetry
                        .sleep_comparator_suspended_ungrounded,
                    arm_out.first_onset_wake_tick,
                    arm_out.facts_forgone,
                );
            }
            if seed_clean_ok {
                clean_succ += 1;
            }
        }
        col.finish_clean_bank(clean_succ, ctx.seeds_per_bank);
    }

    let mut pooled_overrides = BankTailMetricMap::new();
    for (task_id, raw) in &surv_samples {
        let sorted = sorted_finite(raw);
        let (exc_s, exc_n) = exc_counts.get(task_id).copied().unwrap_or((0, 1));
        let (ext_s, ext_n) = ext_counts.get(task_id).copied().unwrap_or((0, 1));
        let map = pooled_overrides.entry(task_id.clone()).or_default();
        map.insert(
            "median_spurious_survival_segments".to_owned(),
            percentile_sorted(&sorted, 0.50),
        );
        map.insert(
            "pl_excision_rate".to_owned(),
            (exc_s as f64) / (exc_n.max(1) as f64),
        );
        map.insert(
            "pl_spurious_extinction_rate".to_owned(),
            (ext_s as f64) / (ext_n.max(1) as f64),
        );
    }

    let stratified = col.evaluate_stratified(
        Some(&pooled_overrides),
        true,
        G4A5_CLEAN_TARGET_RATE,
        G4A5_NOISY_TARGET_RATE,
    );
    let bonf_alpha = (0.01 / (noisy_task_count as f64)) / 2.0;
    let bonf_ok = check_bonferroni_half_for_stratum(
        stratified.noisy.as_ref(),
        &col.bank_counts,
        G4A5_NOISY_TARGET_RATE,
        bonf_alpha,
        &mut col.failures,
    );
    let clean_ok = check_tail_stratum(
        stratified.clean.as_ref(),
        &format!("G4A.5:{arm:?}:Clean"),
        &mut col.failures,
        |t| {
            tail_val(t, "max_spurious_survival_segments")
                .is_some_and(|v| v <= (G4A5_CLEAN_MAX_SURVIVAL_SEGMENTS as f64))
                && tail_val(t, "max_wrongful_excisions").is_some_and(|v| v == 0.0)
        },
    );
    let noisy_ok = check_tail_stratum(
        stratified.noisy.as_ref(),
        &format!("G4A.5:{arm:?}:Noisy"),
        &mut col.failures,
        |t| {
            tail_val(t, "median_spurious_survival_segments")
                .is_some_and(|v| v <= G4A5_NOISY_MEDIAN_SURVIVAL_SEGMENTS_MAX)
        },
    );

    col.into_arm_audit(
        arm,
        stratified,
        Some(bonf_alpha),
        bonf_ok,
        clean_ok && noisy_ok,
    )
}

fn audit_g4a6_arm(arm: SleepArm, ctx: EvalSlice<'_>) -> (ArmGateAudit, CliffCountersSummary) {
    let tag = format!(" ({arm:?})");
    let mut col = BankGateCollector::new(ctx.banks);
    let mut sp_adv_samples: BTreeMap<String, Vec<f64>> = BTreeMap::new();

    for b in 0..ctx.banks {
        let mut clean_succ = 0usize;
        for s_off in 0..ctx.seeds_per_bank {
            let seed_idx = b * ctx.seeds_per_bank + s_off;
            let seed = ctx.seed_offset.saturating_add(seed_idx as u64);
            let mut seed_clean_ok = true;
            for (t_idx, task) in ctx.tasks.iter().enumerate() {
                let pair = &ctx.results_by_task[t_idx][seed_idx];
                let arm_out = arm.select(pair);
                let cliff = max_cliff_two(&pair.wake.cliff, &arm_out.cliff);

                let pass =
                    arm_out.wrongful_excisions == 0 && arm_out.oracle_correct_port_locks_lost == 0;
                let detail = format!(
                    "wrongful_excisions={}, port_locks_lost={}",
                    arm_out.wrongful_excisions, arm_out.oracle_correct_port_locks_lost
                );
                let tmap = col.record_case(
                    b,
                    task,
                    seed,
                    &tag,
                    pass,
                    &cliff,
                    &detail,
                    &mut seed_clean_ok,
                );
                update_max_finite(
                    tmap,
                    "max_wrongful_excisions",
                    arm_out.wrongful_excisions as f64,
                );
                update_max_finite(
                    tmap,
                    "max_port_locks_lost",
                    arm_out.oracle_correct_port_locks_lost as f64,
                );
                if task.family == M4aTaskFamily::Sp {
                    let adv = pair.wake.frozen.eval_l_epistemic_bits
                        - arm_out.frozen.eval_l_epistemic_bits;
                    update_min_finite(tmap, "min_l_epi_advantage", adv);
                    if adv.is_finite() {
                        sp_adv_samples.entry(task.id.clone()).or_default().push(adv);
                    }
                }
                record_common_tail_metrics(
                    tmap,
                    &cliff,
                    arm_out
                        .sleep_telemetry
                        .sleep_comparator_suspended_ungrounded,
                    arm_out.first_onset_wake_tick,
                    arm_out.facts_forgone,
                );
            }
            if seed_clean_ok {
                clean_succ += 1;
            }
        }
        col.finish_clean_bank(clean_succ, ctx.seeds_per_bank);
    }

    let mut pooled_overrides = BankTailMetricMap::new();
    for (task_id, raw) in &sp_adv_samples {
        let map = pooled_overrides.entry(task_id.clone()).or_default();
        record_sp_advantage_overrides(map, raw, false);
    }

    let stratified = col.evaluate_stratified(
        Some(&pooled_overrides),
        true,
        G4A6_CLEAN_TARGET_RATE,
        G4A6_NOISY_TARGET_RATE,
    );
    let clean_ok = check_tail_stratum(
        stratified.clean.as_ref(),
        &format!("G4A.6:{arm:?}:Clean"),
        &mut col.failures,
        |t| {
            tail_val(t, "max_wrongful_excisions").is_some_and(|v| v == 0.0)
                && tail_val(t, "max_port_locks_lost").is_some_and(|v| v == 0.0)
        },
    );
    let noisy_ok = check_tail_stratum(
        stratified.noisy.as_ref(),
        &format!("G4A.6:{arm:?}:Noisy"),
        &mut col.failures,
        |t| {
            tail_val(t, "max_wrongful_excisions").is_some()
                && tail_val(t, "max_port_locks_lost").is_some_and(|v| v == 0.0)
        },
    );

    col.into_arm_audit(arm, stratified, None, true, clean_ok && noisy_ok)
}

fn evaluate_two_arm_gate<F>(
    gate: &str,
    description: &str,
    ctx: EvalSlice<'_>,
    canonical_arm: SleepArm,
    either_arm_passes: bool,
    audit_fn: F,
) -> M4aGateReport
where
    F: Fn(SleepArm, EvalSlice<'_>) -> (ArmGateAudit, CliffCountersSummary),
{
    let (sa, sa_cliff) = audit_fn(SleepArm::Sa, ctx);
    let (sb, sb_cliff) = audit_fn(SleepArm::Sb, ctx);

    let canonical_chosen = match canonical_arm {
        SleepArm::Sa => &sa,
        SleepArm::Sb => &sb,
    };
    let chosen = if either_arm_passes {
        if canonical_chosen.passed {
            canonical_chosen
        } else if sa.passed {
            &sa
        } else if sb.passed {
            &sb
        } else {
            canonical_chosen
        }
    } else {
        canonical_chosen
    };

    let total_excluded = sa.excluded_cases + sb.excluded_cases;
    let passed = if either_arm_passes {
        (sa.passed || sb.passed) && total_excluded == 0
    } else {
        canonical_chosen.passed && total_excluded == 0
    };

    let mut failures = Vec::new();
    if !passed {
        if either_arm_passes {
            failures.extend(sa.failures.iter().cloned());
            failures.extend(sb.failures.iter().cloned());
            truncate_failures(&mut failures);
        } else {
            failures.clone_from(&canonical_chosen.failures);
        }
    }

    M4aGateReport {
        gate: gate.to_owned(),
        description: description.to_owned(),
        passed,
        excluded_cases: total_excluded,
        passed_cases: chosen.passed_cases,
        failed_cases: chosen.failed_cases,
        total_cases: chosen.total_cases,
        empirical_rate: chosen.empirical_rate,
        cliff_counters: max_cliff_two(&sa_cliff, &sb_cliff),
        failures,
        stratified: chosen.stratified.clone(),
        sa_audit: Some(sa),
        sb_audit: Some(sb),
    }
}

fn compute_g4a7_deltas(
    tasks: &[M4aTaskSpec],
    results_by_task: &[Vec<PairedSeedResult>],
) -> (f64, H6Decision, SleepArm, Vec<H6TaskDelta>) {
    let mut per_task = Vec::new();
    let mut all_sum = 0.0f64;
    let mut all_cnt = 0usize;

    for (t_idx, task) in tasks.iter().enumerate() {
        let is_active =
            task.family == M4aTaskFamily::Pl || G4A3_GATED_TASKS.contains(&task.id.as_str());
        if !is_active {
            continue;
        }
        let pairs = &results_by_task[t_idx];
        let mut diffs = Vec::with_capacity(pairs.len());
        let mut sa_adv_sum = 0.0f64;
        let mut sb_adv_sum = 0.0f64;
        for p in pairs {
            // L_eval(S-B) - L_eval(S-A) == adv(S-A) - adv(S-B); positive => S-A achieves lower MDL.
            let diff = p.sb.frozen.eval_l_epistemic_bits - p.sa.frozen.eval_l_epistemic_bits;
            diffs.push(diff);
            all_sum += diff;
            all_cnt += 1;
            sa_adv_sum += p.wake.frozen.eval_l_epistemic_bits - p.sa.frozen.eval_l_epistemic_bits;
            sb_adv_sum += p.wake.frozen.eval_l_epistemic_bits - p.sb.frozen.eval_l_epistemic_bits;
        }
        let sorted = sorted_finite(&diffs);
        per_task.push(H6TaskDelta {
            task_id: task.id.clone(),
            family: family_name(task.family).to_owned(),
            regime: regime_name(task.regime).to_owned(),
            count: sorted.len(),
            mean_delta_6_sb_minus_sa_l_epi_bits: mean_finite(&sorted),
            median_delta_6_bits: percentile_sorted(&sorted, 0.50),
            min_delta_6_bits: sorted.first().copied().unwrap_or(0.0),
            max_delta_6_bits: sorted.last().copied().unwrap_or(0.0),
            sa_mean_l_epi_advantage_bits: sa_adv_sum / (pairs.len().max(1) as f64),
            sb_mean_l_epi_advantage_bits: sb_adv_sum / (pairs.len().max(1) as f64),
        });
    }

    let delta_6 = if all_cnt == 0 {
        0.0
    } else {
        all_sum / (all_cnt as f64)
    };
    let (decision, canonical_arm) = if delta_6 > G4A7_EQUIVALENCE_MARGIN_BITS {
        (H6Decision::SaSuperiorDropReplay, SleepArm::Sa)
    } else if delta_6 < -G4A7_EQUIVALENCE_MARGIN_BITS {
        (H6Decision::SbSuperiorRetainReplay, SleepArm::Sb)
    } else {
        (H6Decision::SaEquivalentDropReplay, SleepArm::Sa)
    };
    (delta_6, decision, canonical_arm, per_task)
}

fn summarize_sp_arm_char(pairs: &[PairedSeedResult], arm: SleepArm) -> SpArmCharacterisation {
    let n = pairs.len().max(1) as f64;
    let mut advs = Vec::with_capacity(pairs.len());
    let mut regs = Vec::with_capacity(pairs.len());
    let mut gt_0 = 0usize;
    let mut max_we = 0usize;
    let mut max_locks = 0usize;

    for p in pairs {
        let a = arm.select(p);
        let adv = p.wake.frozen.eval_l_epistemic_bits - a.frozen.eval_l_epistemic_bits;
        let reg = a.frozen.eval_kt_target - p.wake.frozen.eval_kt_target;
        if adv > 0.0 {
            gt_0 += 1;
        }
        advs.push(adv);
        regs.push(reg);
        max_we = max_we.max(a.wrongful_excisions);
        max_locks = max_locks.max(a.oracle_correct_port_locks_lost);
    }

    let adv_sorted = sorted_finite(&advs);
    let reg_sorted = sorted_finite(&regs);
    SpArmCharacterisation {
        min_l_epi_advantage: adv_sorted.first().copied().unwrap_or(0.0),
        p05_l_epi_advantage: percentile_sorted(&adv_sorted, 0.05),
        mean_l_epi_advantage: mean_finite(&adv_sorted),
        median_l_epi_advantage: percentile_sorted(&adv_sorted, 0.50),
        frac_l_epi_advantage_gt_0: (gt_0 as f64) / n,
        mean_eval_kt_regret: mean_finite(&reg_sorted),
        p95_eval_kt_regret: percentile_sorted(&reg_sorted, 0.95),
        max_wrongful_excisions: max_we as f64,
        max_port_locks_lost: max_locks as f64,
    }
}

fn build_sp_characterisation(
    tasks: &[M4aTaskSpec],
    results_by_task: &[Vec<PairedSeedResult>],
) -> Vec<SpCharacterisationRow> {
    tasks
        .iter()
        .enumerate()
        .filter(|(_, t)| t.family == M4aTaskFamily::Sp)
        .map(|(t_idx, task)| {
            let pairs = &results_by_task[t_idx];
            SpCharacterisationRow {
                task_id: task.id.clone(),
                kind: kind_name(task.kind).to_owned(),
                width_bits: task.width.bits(),
                regime: regime_name(task.regime).to_owned(),
                gated_in_g4a3: G4A3_GATED_TASKS.contains(&task.id.as_str()),
                seeds_tested: pairs.len(),
                sa: summarize_sp_arm_char(pairs, SleepArm::Sa),
                sb: summarize_sp_arm_char(pairs, SleepArm::Sb),
            }
        })
        .collect()
}

fn evaluate_all_gates_from_results(
    tasks: &[M4aTaskSpec],
    results_by_task: &[Vec<PairedSeedResult>],
    seed_offset: u64,
    seeds_per_bank: usize,
    banks: usize,
    threads_used: usize,
    wall_clock_seconds: f64,
) -> M4aVerificationReport {
    let total_seeds = banks * seeds_per_bank;
    let (delta_6, decision, canonical_arm, h6_per_task) =
        compute_g4a7_deltas(tasks, results_by_task);
    let h6_obs: usize = h6_per_task.iter().map(|r| r.count).sum();

    let ctx = EvalSlice {
        tasks,
        results_by_task,
        banks,
        seeds_per_bank,
        seed_offset,
    };

    let g1 = evaluate_g4a1(ctx);
    let g2 = evaluate_g4a2(ctx);
    let g3 = evaluate_two_arm_gate(
        "G4A.3",
        "H5 Spontaneous Falsification Superiority (Noisy gated M=3 positive-opportunity SP tasks)",
        ctx,
        canonical_arm,
        true,
        audit_g4a3_arm,
    );
    let g4 = evaluate_two_arm_gate(
        "G4A.4",
        "Boundary KT Non-Inferiority (Clean/Noisy SP+CT M=14 per stratum)",
        ctx,
        canonical_arm,
        false,
        audit_g4a4_arm,
    );
    let g5 = evaluate_two_arm_gate(
        "G4A.5",
        "Planted Spurious-Cycle Rejection (PL1, PL2; Clean/Noisy M=4 per stratum)",
        ctx,
        canonical_arm,
        true,
        audit_g4a5_arm,
    );
    let g6 = evaluate_two_arm_gate(
        "G4A.6",
        "Fact-Supported Preservation (zero port locks lost; Clean exact / Noisy >=0.90)",
        ctx,
        canonical_arm,
        false,
        audit_g4a6_arm,
    );

    let arm_ok = |g: &M4aGateReport| {
        let audit = match canonical_arm {
            SleepArm::Sa => g.sa_audit.as_ref(),
            SleepArm::Sb => g.sb_audit.as_ref(),
        };
        audit.is_some_and(|a| a.passed)
    };
    let canonical_ok =
        g1.passed && g2.passed && arm_ok(&g3) && arm_ok(&g4) && arm_ok(&g5) && arm_ok(&g6);

    let h6_decision = H6DecisionReport {
        gate: "G4A.7".to_owned(),
        active_task_count: h6_per_task.len(),
        total_paired_observations: h6_obs,
        equivalence_margin_bits: G4A7_EQUIVALENCE_MARGIN_BITS,
        delta_6_sb_minus_sa_l_epi_bits: delta_6,
        decision,
        canonical_arm,
        canonical_arm_passed_all_gates: canonical_ok,
        per_task: h6_per_task,
    };

    let mut total_cliff = CliffCountersSummary::default();
    let mut total_excluded = 0usize;
    for task_rows in results_by_task {
        for pair in task_rows {
            let cliff = max_cliff_pair(pair);
            total_cliff.accumulate(&cliff);
            if cliff.guardrail_bound() {
                total_excluded += 1;
            }
        }
    }

    let gates = vec![g1, g2, g3, g4, g5, g6];
    let gate_excluded: usize = gates.iter().map(|g| g.excluded_cases).sum();
    let excluded_cases = total_excluded.max(gate_excluded);
    let overall_passed = gates.iter().all(|g| g.passed)
        && h6_decision.canonical_arm_passed_all_gates
        && excluded_cases == 0;
    let sp_characterisation = build_sp_characterisation(tasks, results_by_task);

    M4aVerificationReport {
        provenance: BuildProvenance::current(),
        seed_offset,
        seeds_per_bank,
        banks,
        total_seeds,
        threads_used,
        wall_clock_seconds,
        all_passed: overall_passed,
        overall_passed,
        excluded_cases,
        cliff_counters: total_cliff,
        gates,
        h6_decision,
        sp_characterisation,
    }
}

/// Runs Milestone M4a confirmatory verification (`G4A.1`–`G4A.7`) across `banks` consecutive
/// seed banks of `seeds` seeds each starting at `seed_offset`.
///
/// # Errors
///
/// Returns an error if `banks == 0`, `banks * seeds` overflows, or the seed range overlaps any
/// permanently closed holdout bank (including M4a `60000..=60099`, even with `--holdout`).
pub fn run_m4a_verification(
    seed_offset: u64,
    seeds: usize,
    banks: usize,
    allow_holdout: bool,
    threads: Option<usize>,
) -> Result<M4aVerificationReport, String> {
    let guard_banks = u64::try_from(banks.max(1)).map_err(|_| "banks count overflow".to_owned())?;
    let guard_seeds = u64::try_from(seeds.max(1)).map_err(|_| "seeds count overflow".to_owned())?;
    let guard_span = guard_banks.saturating_mul(guard_seeds);
    check_m4a_seed_range_holdout(seed_offset, guard_span, allow_holdout)?;

    if banks == 0 {
        return Err("banks count must be >= 1".to_owned());
    }
    let seeds_per_bank = seeds.max(1);
    let total_seeds = banks
        .checked_mul(seeds_per_bank)
        .ok_or_else(|| "total seeds (banks * seeds) overflowed".to_owned())?;

    let tasks = m4a0_task_registry();
    let task_meta: Vec<(FiniteStateSpec, PortLanguageOracle)> = tasks
        .iter()
        .map(|t| {
            let spec = t
                .to_clean_finite_state_spec()
                .expect("registered M4a task spec exports cleanly");
            let oracle = PortLanguageOracle::from_clean_spec(&spec);
            (spec, oracle)
        })
        .collect();

    let default_threads = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1)
        .max(1);
    let requested_threads = threads.unwrap_or(default_threads).max(1);

    let mut work_items = Vec::with_capacity(tasks.len() * total_seeds);
    for seed_idx in 0..total_seeds {
        for task_idx in 0..tasks.len() {
            work_items.push((task_idx, seed_idx));
        }
    }
    let threads_used = requested_threads.min(work_items.len()).max(1);

    let slots: Vec<Vec<Mutex<Option<PairedSeedResult>>>> = (0..tasks.len())
        .map(|_| (0..total_seeds).map(|_| Mutex::new(None)).collect())
        .collect();
    let next_idx = AtomicUsize::new(0);

    let start = Instant::now();
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
                    let res = run_paired_task_seed(
                        &tasks_ref[task_idx],
                        &meta_ref[task_idx].0,
                        &meta_ref[task_idx].1,
                        seed,
                        false,
                    );
                    *slots_ref[task_idx][seed_idx]
                        .lock()
                        .expect("verify-m4a slot lock") = Some(res);
                }
            });
        }
    });
    let wall_clock_seconds = start.elapsed().as_secs_f64();

    let mut results_by_task: Vec<Vec<PairedSeedResult>> = Vec::with_capacity(tasks.len());
    for row in slots {
        let mut task_row = Vec::with_capacity(total_seeds);
        for slot in row {
            task_row.push(
                slot.into_inner()
                    .expect("verify-m4a slot lock")
                    .expect("verify-m4a seed evaluated"),
            );
        }
        results_by_task.push(task_row);
    }

    Ok(evaluate_all_gates_from_results(
        &tasks,
        &results_by_task,
        seed_offset,
        seeds_per_bank,
        banks,
        threads_used,
        wall_clock_seconds,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structural::SleepTelemetry;
    use crate::verify_m4a::explore::FrozenEvalMetrics;

    fn mock_arm_outcome(
        horizon: u64,
        eval_l_epi: f64,
        eval_kt_target: f64,
        is_pl: bool,
    ) -> ArmRunOutcome {
        ArmRunOutcome {
            pl_settled: true,
            episode_ticks: horizon,
            env_steps: horizon,
            facts_forgone: 0,
            wake_preds_prefix: Vec::new(),
            wake_obs_prefix: Vec::new(),
            final_circuit_code_bits: 100.0,
            final_node_count: 10,
            cumulative_net_flux: 0.0,
            cumulative_delta_l_epistemic_bits: 0.0,
            cumulative_delta_l_epistemic_per_tick: 0.0,
            frozen: FrozenEvalMetrics {
                eval_kt_all_ch: eval_kt_target,
                eval_kt_target,
                eval_l_epistemic_bits: eval_l_epi,
                eval_l_epistemic_target_bits: eval_l_epi,
                eval_delta_l_epistemic_bits: 0.0,
                eval_delta_l_epistemic_target_bits: 0.0,
                eval_clean_zero_error_target: true,
            },
            final_port_oracle_correct: true,
            final_active_comparators: 1,
            epistemic_mdl_err: 1e-13,
            episode_total_node_evals: 1000,
            episode_live_step_node_evals: 900,
            episode_candidate_replay_node_evals: 50,
            episode_sensitivity_node_evals: 50,
            cliff: CliffCountersSummary::default(),
            sleep_telemetry: SleepTelemetry::default(),
            first_onset_wake_tick: Some(256),
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
            pl_excised: is_pl,
            pl_spurious_extinguished: is_pl,
            pl_spurious_survival_segments: 0,
            pl_segments_to_excise: is_pl.then_some(1),
            pl_refutation_wake_ticks: None,
            pl_refutation_substrate_ticks: None,
            spontaneous_incorrect_born: 0,
            spontaneous_incorrect_refuted: 0,
            spontaneous_refutation_latencies: Vec::new(),
            checkpoints_by_wake_step: BTreeMap::new(),
            dream_probe: None,
        }
    }

    fn mock_passing_results(
        tasks: &[M4aTaskSpec],
        total_seeds: usize,
    ) -> Vec<Vec<PairedSeedResult>> {
        tasks
            .iter()
            .map(|t| {
                let is_pl = t.family == M4aTaskFamily::Pl;
                let is_gated_sp = G4A3_GATED_TASKS.contains(&t.id.as_str());
                (0..total_seeds)
                    .map(|_| {
                        let wake = mock_arm_outcome(t.horizon, 500.0, 0.10, is_pl);
                        let sa_l = if is_gated_sp {
                            450.0
                        } else if is_pl && t.regime == M4aRegime::Noisy {
                            300.0
                        } else {
                            500.0
                        };
                        let sb_l = if is_gated_sp {
                            450.0
                        } else if is_pl && t.regime == M4aRegime::Noisy {
                            400.0
                        } else {
                            500.0
                        };
                        let sa = mock_arm_outcome(t.horizon, sa_l, 0.10, is_pl);
                        let sb = mock_arm_outcome(t.horizon, sb_l, 0.10, is_pl);
                        PairedSeedResult {
                            wake,
                            sa,
                            sb,
                            i2_sa_pre_onset_ok: true,
                            i2_sb_pre_onset_ok: true,
                            i2_sa_no_onset_exact: Some(true),
                            i2_sb_no_onset_exact: Some(true),
                            sensitivity_runs: Vec::new(),
                        }
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn verify_m4a_holdout_guard_unconditionally_closes_m4a_and_prior_ranges() {
        for closed in [10_000u64, 30_000, 40_000, 50_000, 50_100, 60_000] {
            for allow in [false, true] {
                assert!(check_m4a_seed_range_holdout(closed, 20, allow).is_err());
                assert!(check_m4a_seed_range_holdout(closed + 10, 1, allow).is_err());
                assert!(check_m4a_seed_range_holdout(closed - 5, 10, allow).is_err());
                assert!(check_m4a_seed_range_holdout(closed, 0, allow).is_err());
            }
        }

        // Consumed M4a holdout 60_000..=60_099 is unconditionally rejected even when allow_holdout=true.
        for interior in [60_000u64, 60_050, 60_099] {
            for allow in [false, true] {
                let err = check_m4a_seed_range_holdout(interior, 1, allow).unwrap_err();
                assert!(err.contains("60000..=60099"));
                assert!(
                    err.contains("consumed once at 0f62e2f under S3 #65 and permanently closed")
                );
                assert!(err.contains("unconditionally forbidden under verify-m4a"));
            }
        }
        assert!(check_m4a_seed_range_holdout(59_920, 100, false).is_err());
        assert!(check_m4a_seed_range_holdout(59_920, 100, true).is_err());

        for safe in [(0u64, 20), (520, 200), (20_200, 100)] {
            assert!(check_m4a_seed_range_holdout(safe.0, safe.1, false).is_ok());
            assert!(check_m4a_seed_range_holdout(safe.0, safe.1, true).is_ok());
        }
    }

    #[test]
    fn synthetic_gates_pass_when_all_criteria_hold_and_detect_violations() {
        let tasks = m4a0_task_registry();
        let baseline = mock_passing_results(&tasks, 100);
        let rep_ok = evaluate_all_gates_from_results(&tasks, &baseline, 0, 20, 5, 1, 0.1);
        assert!(rep_ok.overall_passed);
        assert!(rep_ok.all_passed);
        assert_eq!(rep_ok.excluded_cases, 0);
        assert_eq!(rep_ok.gates.len(), 6);
        assert_eq!(rep_ok.sp_characterisation.len(), 16);
        assert_eq!(rep_ok.h6_decision.active_task_count, 11);
        assert_eq!(
            rep_ok.h6_decision.decision,
            H6Decision::SaSuperiorDropReplay
        );
        assert!(rep_ok.h6_decision.canonical_arm_passed_all_gates);

        // 1. Guardrail exclusion marks CaseOutcome::Excluded and fails overall_passed.
        let mut excl = mock_passing_results(&tasks, 100);
        excl[0][0].sa.cliff.sprout_blocked_max_nodes = 1;
        let rep_excl = evaluate_all_gates_from_results(&tasks, &excl, 0, 20, 5, 1, 0.1);
        assert!(!rep_excl.overall_passed);
        assert!(rep_excl.excluded_cases > 0);
        assert!(!rep_excl.gates[0].passed);

        // 2. Hard G4A.6 port_locks_lost invariant fails even on a single noisy seed.
        let mut lost_lock = mock_passing_results(&tasks, 100);
        let noisy_idx = tasks
            .iter()
            .position(|t| t.regime == M4aRegime::Noisy)
            .unwrap();
        lost_lock[noisy_idx][0].sa.oracle_correct_port_locks_lost = 1;
        let rep_lock = evaluate_all_gates_from_results(&tasks, &lost_lock, 0, 20, 5, 1, 0.1);
        assert!(!rep_lock.gates[5].passed);
        assert!(!rep_lock.overall_passed);

        // 3. G4A.3 fails if mean_l_epi_advantage < 15.0 despite 100% > 0.
        let mut low_adv = mock_passing_results(&tasks, 100);
        let sp_idx = tasks
            .iter()
            .position(|t| t.id == G4A3_GATED_TASKS[0])
            .unwrap();
        for p in &mut low_adv[sp_idx] {
            p.sa.frozen.eval_l_epistemic_bits = 495.0;
            p.sb.frozen.eval_l_epistemic_bits = 495.0;
        }
        let rep_adv = evaluate_all_gates_from_results(&tasks, &low_adv, 0, 20, 5, 1, 0.1);
        assert!(!rep_adv.gates[2].passed);
        assert!(!rep_adv.overall_passed);

        // 4. G4A.7 selects SbSuperiorRetainReplay when S-B leads S-A by > 10 bits.
        let mut sb_wins = mock_passing_results(&tasks, 100);
        for (ti, t) in tasks.iter().enumerate() {
            if t.family == M4aTaskFamily::Pl || G4A3_GATED_TASKS.contains(&t.id.as_str()) {
                for p in &mut sb_wins[ti] {
                    p.sa.frozen.eval_l_epistemic_bits = 450.0;
                    p.sb.frozen.eval_l_epistemic_bits = 400.0;
                }
            }
        }
        let rep_sb = evaluate_all_gates_from_results(&tasks, &sb_wins, 0, 20, 5, 1, 0.1);
        assert_eq!(
            rep_sb.h6_decision.decision,
            H6Decision::SbSuperiorRetainReplay
        );
        assert_eq!(rep_sb.h6_decision.canonical_arm, SleepArm::Sb);
        assert!(rep_sb.overall_passed);

        // 5. G4A.7 selects SaEquivalentDropReplay when |delta_6| <= 10.0 and fails overall if S-A fails G4A.5.
        let mut equiv_sa_fails = mock_passing_results(&tasks, 100);
        let pl_noisy_idx = tasks
            .iter()
            .position(|t| t.family == M4aTaskFamily::Pl && t.regime == M4aRegime::Noisy)
            .unwrap();
        for (ti, t) in tasks.iter().enumerate() {
            if t.family == M4aTaskFamily::Pl || G4A3_GATED_TASKS.contains(&t.id.as_str()) {
                for p in &mut equiv_sa_fails[ti] {
                    p.sa.frozen.eval_l_epistemic_bits = 450.0;
                    p.sb.frozen.eval_l_epistemic_bits = 452.0;
                }
            }
        }
        for p in &mut equiv_sa_fails[pl_noisy_idx] {
            p.sa.pl_spurious_survival_segments = 3;
        }
        let rep_eq = evaluate_all_gates_from_results(&tasks, &equiv_sa_fails, 0, 20, 5, 1, 0.1);
        assert_eq!(
            rep_eq.h6_decision.decision,
            H6Decision::SaEquivalentDropReplay
        );
        assert_eq!(rep_eq.h6_decision.canonical_arm, SleepArm::Sa);
        assert!(rep_eq.gates[4].passed); // S-B still passed G4A.5 (either arm)
        assert!(!rep_eq.h6_decision.canonical_arm_passed_all_gates); // S-A failed median_surv <= 2
        assert!(!rep_eq.overall_passed);

        // 6. G4A.4 fails when Noisy mean_eval_kt_regret > 0.10 despite passing per-seed <= 0.35.
        let mut high_regret = mock_passing_results(&tasks, 100);
        let sp_clean_or_noisy = tasks
            .iter()
            .position(|t| t.family == M4aTaskFamily::Sp && t.regime == M4aRegime::Noisy)
            .unwrap();
        for p in &mut high_regret[sp_clean_or_noisy] {
            p.sa.frozen.eval_kt_target = 0.25;
        }
        let rep_reg = evaluate_all_gates_from_results(&tasks, &high_regret, 0, 20, 5, 1, 0.1);
        assert!(!rep_reg.gates[3].passed);
        assert!(!rep_reg.overall_passed);
    }
}
