//! Milestone 2a cross-channel coupling acceptance verification suite (`G2A.1`–`G2A.5`).

use std::collections::{BTreeMap, VecDeque};

use serde::{Deserialize, Serialize};

use crate::baselines::{
    CgpBaseline, CrossChannelCtwBaseline, CtwBaseline, PredictorBaseline, RandomSearchBaseline,
    ReedsSloaneBaseline, RnnBaseline, TickBudget, run_prequential_harness,
};
use crate::config::{BuildProvenance, CliffCountersSummary, compute_ground_truth_yardstick};
use crate::env::{
    CrossLinearOp, CrossNonlinearOp, FiniteStateGenerator, FiniteStateSpec, GeneratorConfig,
    LfsrStage, NoiseModel, NoisyEnvironment, SpecExportError, TierGenerator,
    compound_reactive_entropy_bits,
};
use crate::metrics::{BudgetAccountingMode, Scorecard};
use crate::rng::SeedBank;
use crate::structural::{ArkheLearner, EconomyConfig, GrowthConfig};
use crate::substrate::{Circuit, SignalSink, SignalSource, Width, WireDelayMode};
use crate::verify_m1::{
    BankSample, CaseOutcome, MultiBankGateResult, OnlineCaseSummary, evaluate_multi_bank_gate,
    m1b::classify_case_outcome,
};

/// Default Milestone 2a simulation horizon $T$ in ticks (§4 / §6).
pub const DEFAULT_M2A_HORIZON: u64 = 2_000;
/// Default Milestone 2a post-transient evaluation warmup in ticks (§4 / §6).
pub const DEFAULT_M2A_WARMUP: u64 = 500;
/// Default Milestone 2a per-run candidate-replay / compute budget (§6).
pub const DEFAULT_M2A_BUDGET: u64 = 200_000;

/// Reserved M1 holdout seed range start (never touched by M2a).
pub const M1_HOLDOUT_SEED_START: u32 = 10_000;
/// Reserved M1 holdout seed range end (inclusive).
pub const M1_HOLDOUT_SEED_END: u32 = 10_019;
/// Pre-registered M2a single-shot holdout seed range start (§4).
pub const M2A_HOLDOUT_SEED_START: u32 = 30_000;
/// Pre-registered M2a single-shot holdout seed range end (inclusive).
pub const M2A_HOLDOUT_SEED_END: u32 = 30_019;
/// Reserved M2b holdout seed range start (never touched by M2a).
pub const M2B_HOLDOUT_SEED_START: u32 = 40_000;
/// Reserved M2b holdout seed range end (inclusive).
pub const M2B_HOLDOUT_SEED_END: u32 = 40_019;

/// Checks whether `[seed_offset, seed_offset + seeds)` overlaps any reserved holdout seed range.
///
/// - `10000..=10019` (M1 holdout) and `40000..=40019` (M2b holdout) are unconditionally refused
///   by `verify-m2a`.
/// - `30000..=30019` (M2a single-shot holdout) is refused unless `allow_holdout == true`.
///
/// # Errors
///
/// Returns an explanatory error string whenever a guarded holdout range overlaps without valid
/// authorization.
pub fn check_m2a_seed_range_holdout(
    seed_offset: u32,
    seeds: u32,
    allow_holdout: bool,
) -> Result<(), String> {
    let start = seed_offset;
    let end = seed_offset.saturating_add(seeds.max(1));

    let overlaps = |lo: u32, hi: u32| start <= hi && end > lo;

    if overlaps(M1_HOLDOUT_SEED_START, M1_HOLDOUT_SEED_END) {
        return Err(format!(
            "refusing seed range {start}..{end} which overlaps pre-registered holdout range \
             {M1_HOLDOUT_SEED_START}..={M1_HOLDOUT_SEED_END} (reserved for M1)"
        ));
    }
    if overlaps(M2B_HOLDOUT_SEED_START, M2B_HOLDOUT_SEED_END) {
        return Err(format!(
            "refusing seed range {start}..{end} which overlaps pre-registered holdout range \
             {M2B_HOLDOUT_SEED_START}..={M2B_HOLDOUT_SEED_END} (reserved for M2b)"
        ));
    }
    if overlaps(M2A_HOLDOUT_SEED_START, M2A_HOLDOUT_SEED_END) && !allow_holdout {
        return Err(format!(
            "refusing seed range {start}..{end} which overlaps pre-registered holdout range \
             {M2A_HOLDOUT_SEED_START}..={M2A_HOLDOUT_SEED_END} without --holdout flag"
        ));
    }
    Ok(())
}

/// Summary of one baseline evaluated on a `G2A.5` benchmark task.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct G2a5BaselineAggregate {
    /// Baseline name (`ArkheM1`, `RandomSearch`, `CGP`, `CTW`, `ReedsSloane`, `CrossChannelCTW`, `RnnBptt`).
    pub baseline: String,
    /// Declared compute metering protocol.
    pub budget_accounting_mode: BudgetAccountingMode,
    /// Mean target-channel prequential KT residual code length (bits/symbol).
    pub mean_target_kt_bits_per_symbol: f64,
    /// Mean whole-frame native probability log-loss (bits/symbol), if probability-emitting.
    pub mean_native_prob_log_loss_bits_per_symbol: Option<f64>,
    /// Mean post-transient target-channel clean match rate.
    pub mean_target_clean_match_rate: f64,
    /// Mean `TickBudget` compute ticks consumed across evaluated runs.
    pub mean_compute_ticks_consumed: f64,
    /// Mean `baseline_target_kt - arkhe_target_kt` advantage (bits/symbol; positive means Arkhe wins).
    pub mean_arkhe_kt_advantage_bits: f64,
    /// Number of evaluated seed runs aggregated.
    pub runs: usize,
}

/// Per-task benchmark report for Gate `G2A.5`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct G2a5TaskBenchmarkReport {
    /// Task label.
    pub task: String,
    /// Uniform bit width $k$.
    pub k_bits: u8,
    /// Whether this task is nonlinear (`T2_CrossNonlinear`).
    pub nonlinear: bool,
    /// Baseline comparison entries on this task.
    pub baselines: Vec<G2a5BaselineAggregate>,
}

/// Aggregate report for one M2a gate (`G2A.1`–`G2A.5`) within a single run (`verify-m2a`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M2aGateReport {
    /// Gate identifier (`G2A.1`..`G2A.5`).
    pub gate: String,
    /// Pre-registered target success rate (`P_target`).
    pub target_rate: f64,
    /// Whether this gate passed on the evaluated slice.
    pub passed: bool,
    /// Number of seeds that passed every task check in this gate.
    pub passed_seeds: usize,
    /// Total seeds evaluated in this gate.
    pub total_seeds: usize,
    /// Empirical fraction `passed_seeds / total_seeds`.
    pub empirical_rate: f64,
    /// `evaluate_multi_bank_gate` statistics over the single evaluated bank (`lower_bound_99`, etc.).
    pub bank_gate_stats: MultiBankGateResult,
    /// Cases excluded because a compute guardrail (`sprout_blocked_max_nodes`) bound.
    pub excluded_cases: usize,
    /// Aggregated cliff, bound, and guardrail telemetry counters.
    pub cliff_counters: CliffCountersSummary,
    /// For `G2A.5`, minimum pairwise mean KT residual advantage (`>= 0.25` b/sym) across required margins.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_required_mean_margin_bits: Option<f64>,
    /// For `G2A.4`, maximum task-level `|mean(kt_learner) - H_reactive(eps, m, k)|` (`<= 0.040` b/sym)
    /// across all all-channel observation noise tasks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_g2a4b_mean_h_reactive_dev_bits: Option<f64>,
    /// Failure explanations, if any.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub failures: Vec<String>,
}

/// Complete Milestone 2a verification report for one seed slice (`verify-m2a`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M2aVerificationSummary {
    /// Binary and source-tree provenance (includes `source_tree_fnv1a128`).
    pub provenance: BuildProvenance,
    /// Starting master seed offset.
    pub seed_offset: u32,
    /// Number of seeds evaluated.
    pub seeds_tested: u32,
    /// Simulation horizon $T$ in ticks.
    pub horizon: u64,
    /// Results for gates `G2A.1` through `G2A.5`.
    pub gates: Vec<M2aGateReport>,
    /// Explicit common-compute / common-loss baseline table for `G2A.5`.
    pub g2a5_benchmarks: Vec<G2a5TaskBenchmarkReport>,
    /// Total cases excluded by compute guardrails across all gates.
    pub excluded_cases: usize,
    /// Aggregated cliff and bound counters across all online runs.
    pub cliff_counters: CliffCountersSummary,
    /// Whether every M2a gate passed with zero guardrail exclusions.
    pub all_passed: bool,
}

/// Pooled multi-bank outcome for one M2a gate across `N` banks (`verify-m2a --banks N`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M2aBankGateReport {
    /// Gate identifier (`G2A.1`..`G2A.5`).
    pub gate: String,
    /// Pooled Clopper–Pearson 99% lower bound and Pearson $\chi^2$ homogeneity result.
    pub stats: MultiBankGateResult,
    /// Number of individual banks achieving empirical rate >= `target_rate`.
    pub banks_above_target: u32,
    /// Total banks evaluated.
    pub total_banks: u32,
    /// Total cases excluded by compute guardrails across all banks.
    pub excluded_cases: usize,
    /// Aggregated cliff and bound counters across all banks.
    pub cliff_counters: CliffCountersSummary,
    /// For `G2A.5`, minimum pooled mean KT residual advantage (`>= 0.25` b/sym).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_required_mean_margin_bits: Option<f64>,
    /// For `G2A.4`, maximum pooled task-level `|mean(kt_learner) - H_reactive(eps, m, k)|` (`<= 0.040` b/sym).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_g2a4b_mean_h_reactive_dev_bits: Option<f64>,
    /// Whether `stats.passed`, `excluded_cases == 0`, `G2A.4b` mean deviation, and `G2A.5` margin passed.
    pub passed: bool,
}

/// Multi-bank Milestone 2a verification report (`verify-m2a --banks N`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M2aBankReport {
    /// Binary and source-tree provenance (includes `source_tree_fnv1a128`).
    pub provenance: BuildProvenance,
    /// Starting master seed offset.
    pub seed_offset: u32,
    /// Master seeds per bank.
    pub seeds_per_bank: u32,
    /// Number of consecutive banks evaluated.
    pub banks_evaluated: u32,
    /// Simulation horizon $T$ in ticks.
    pub horizon: u64,
    /// Multi-bank statistical evaluation for `G2A.1`..`G2A.5`.
    pub gates: Vec<M2aBankGateReport>,
    /// Pooled equal-compute / common-loss benchmark table across all evaluated banks.
    pub g2a5_benchmarks: Vec<G2a5TaskBenchmarkReport>,
    /// Total cases excluded by compute guardrails across all banks.
    pub excluded_cases: usize,
    /// Aggregated cliff and bound counters across all banks.
    pub cliff_counters: CliffCountersSummary,
    /// Diagnostic failure details across banks (truncated cleanly if many).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub failures: Vec<String>,
    /// Whether all five M2a gates passed pooled + homogeneity + zero-exclusion checks.
    pub all_passed: bool,
}

impl M2aBankReport {
    /// Returns `true` when every gate passed pooled + homogeneity + zero-exclusion checks.
    #[must_use]
    pub fn all_passed(&self) -> bool {
        self.all_passed
    }
}

/// Single-channel projection of a multi-channel [`TierGenerator`] for `G2A.1(iii)` marginal controls.
struct MarginalTargetGenerator {
    label: String,
    inner: TierGenerator,
    target_channel: usize,
    scratch: Vec<u64>,
}

impl MarginalTargetGenerator {
    fn new(inner: TierGenerator, target_channel: usize) -> Self {
        let label = format!("Marginal({}:ch{target_channel})", inner.name());
        let scratch = vec![0u64; usize::from(inner.channels())];
        Self {
            label,
            inner,
            target_channel,
            scratch,
        }
    }
}

impl FiniteStateGenerator for MarginalTargetGenerator {
    fn name(&self) -> &str {
        &self.label
    }

    fn width(&self) -> Width {
        self.inner.width()
    }

    fn channels(&self) -> u16 {
        1
    }

    fn reset(&mut self) {
        self.inner.reset();
    }

    fn next_clean_frame(&mut self, out: &mut [u64]) {
        self.inner.next_clean_frame(&mut self.scratch);
        out[0] = self.scratch[self.target_channel];
    }

    fn to_finite_state_spec(&self) -> Result<FiniteStateSpec, SpecExportError> {
        let mut spec = self.inner.to_finite_state_spec()?;
        spec.channels = 1;
        for (_, frame) in &mut spec.transitions {
            let sym = frame[self.target_channel];
            frame.clear();
            frame.push(sym);
        }
        Ok(spec)
    }
}

/// Deterministic independent multi-channel periodic motif generator for `G2A.3`.
#[derive(Clone)]
struct MultiChannelPeriodicGenerator {
    label: String,
    width: Width,
    motifs: Vec<Vec<u64>>,
    tick: u64,
}

impl MultiChannelPeriodicGenerator {
    fn new(label: impl Into<String>, width: Width, motifs: Vec<Vec<u64>>) -> Self {
        Self {
            label: label.into(),
            width,
            motifs,
            tick: 0,
        }
    }
}

impl FiniteStateGenerator for MultiChannelPeriodicGenerator {
    fn name(&self) -> &str {
        &self.label
    }

    fn width(&self) -> Width {
        self.width
    }

    fn channels(&self) -> u16 {
        u16::try_from(self.motifs.len()).expect("channel count fits in u16")
    }

    fn reset(&mut self) {
        self.tick = 0;
    }

    fn next_clean_frame(&mut self, out: &mut [u64]) {
        for (slot, motif) in out.iter_mut().zip(&self.motifs) {
            *slot = self
                .width
                .normalize(motif[(self.tick as usize) % motif.len()]);
        }
        self.tick += 1;
    }

    fn to_finite_state_spec(&self) -> Result<FiniteStateSpec, SpecExportError> {
        let mut period = 1usize;
        for m in &self.motifs {
            let g = gcd_usize(period, m.len());
            period = period.saturating_mul(m.len() / g);
            if period > crate::env::DEFAULT_MAX_EXPORT_STATES {
                return Err(SpecExportError::StateSpaceTooLarge {
                    estimated_states: period as u128,
                    limit: crate::env::DEFAULT_MAX_EXPORT_STATES,
                });
            }
        }
        let mut transitions = Vec::with_capacity(period);
        for s in 0..period {
            let next_state = (s + 1) % period;
            let frame = self
                .motifs
                .iter()
                .map(|m| self.width.normalize(m[s % m.len()]))
                .collect();
            transitions.push((next_state, frame));
        }
        Ok(FiniteStateSpec {
            width: self.width,
            channels: self.channels(),
            num_states: period,
            initial_state: 0,
            transitions,
            scope: crate::env::EpsilonMachineScope::StationaryProcess,
        })
    }
}

const fn gcd_usize(mut a: usize, mut b: usize) -> usize {
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a
}

fn driver_small_0() -> LfsrStage {
    LfsrStage {
        taps: vec![0, 1, 0, 0, 1],
        init: vec![1, 1, 0, 1, 0],
    }
}

fn driver_small_1() -> LfsrStage {
    LfsrStage {
        taps: vec![1, 0, 0, 1],
        init: vec![1, 0, 1, 1],
    }
}

fn driver_hi_0() -> LfsrStage {
    LfsrStage {
        taps: vec![0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1],
        init: vec![1, 0, 1, 1, 0, 1, 0, 1, 1, 0, 1],
    }
}

fn driver_hi_1() -> LfsrStage {
    LfsrStage {
        taps: vec![0, 0, 0, 1, 0, 0, 0, 0, 1],
        init: vec![1, 1, 0, 1, 0, 1, 1, 0, 1],
    }
}

/// Checks via reverse graph BFS whether `BoundaryOut(target_out)` has at least one upstream
/// connection from `BoundaryIn(c)` with `c != target_out`.
#[must_use]
pub fn circuit_target_cone_uses_cross_channel(circuit: &Circuit, target_out: u16) -> bool {
    let mut visited = vec![false; circuit.node_count()];
    let mut queue = VecDeque::new();
    let edges = circuit.edges();

    for edge in edges {
        if edge.dst == SignalSink::BoundaryOut(target_out) {
            match edge.src {
                SignalSource::BoundaryIn(ch) if ch != target_out => return true,
                SignalSource::BoundaryIn(_) => {}
                SignalSource::NodeOut { node, .. } => {
                    let idx = node.index();
                    if idx < visited.len() && !visited[idx] {
                        visited[idx] = true;
                        queue.push_back(node);
                    }
                }
            }
        }
    }

    while let Some(node) = queue.pop_front() {
        for edge in edges {
            if matches!(edge.dst, SignalSink::NodeIn { node: dst_node, .. } if dst_node == node) {
                match edge.src {
                    SignalSource::BoundaryIn(ch) if ch != target_out => return true,
                    SignalSource::BoundaryIn(_) => {}
                    SignalSource::NodeOut { node: src_node, .. } => {
                        let idx = src_node.index();
                        if idx < visited.len() && !visited[idx] {
                            visited[idx] = true;
                            queue.push_back(src_node);
                        }
                    }
                }
            }
        }
    }
    false
}

struct LearnerEvalResult {
    learner: ArkheLearner,
    scorecard: Scorecard,
    cliff: CliffCountersSummary,
    excluded: bool,
}

fn run_learner_on_env<G: FiniteStateGenerator>(
    env: &mut NoisyEnvironment<G>,
    width: Width,
    channels: u16,
    wire_delay: WireDelayMode,
    horizon: u64,
    seeds_bank: SeedBank,
    em_ground_truth: Option<&crate::env::EpsilonMachineSummary>,
) -> LearnerEvalResult {
    let mut learner = ArkheLearner::new_with_config(
        width,
        channels,
        wire_delay,
        EconomyConfig::default(),
        GrowthConfig::default(),
        seeds_bank,
    )
    .expect("learner creation succeeds");

    let scorecard = run_prequential_harness(
        env,
        &mut learner,
        horizon,
        DEFAULT_M2A_WARMUP,
        DEFAULT_M2A_BUDGET,
        em_ground_truth,
        None,
    );
    let tel = learner.telemetry();
    let cliff = CliffCountersSummary::from(&tel);
    let excluded = cliff.guardrail_bound();
    LearnerEvalResult {
        learner,
        scorecard,
        cliff,
        excluded,
    }
}

struct SeedOutcome {
    passed: bool,
    cliff: CliffCountersSummary,
    excluded_cases: usize,
    failures: Vec<String>,
}

fn evaluate_g2a1_seed(master_seed: u64, horizon: u64) -> SeedOutcome {
    let d0 = driver_small_0();
    let d1 = driver_small_1();

    let tasks: Vec<(&str, Width, GeneratorConfig)> = vec![
        (
            "DelayedAffine(d=1,k=1)",
            Width::BITS_1,
            GeneratorConfig::T2_DelayedAffine {
                driver: d0.clone(),
                lag: 1,
                mul_a: 1,
                add_b: 1,
            },
        ),
        (
            "DelayedAffine(d=1,k=2)",
            Width::BITS_2,
            GeneratorConfig::T2_DelayedAffine {
                driver: d0.clone(),
                lag: 1,
                mul_a: 1,
                add_b: 1,
            },
        ),
        (
            "DelayedAffine(d=2,k=1)",
            Width::BITS_1,
            GeneratorConfig::T2_DelayedAffine {
                driver: d0.clone(),
                lag: 2,
                mul_a: 1,
                add_b: 1,
            },
        ),
        (
            "DelayedAffine(d=2,3x+2,k=2)",
            Width::BITS_2,
            GeneratorConfig::T2_DelayedAffine {
                driver: d0.clone(),
                lag: 2,
                mul_a: 3,
                add_b: 2,
            },
        ),
        (
            "DelayedAffine(d=4,k=1)",
            Width::BITS_1,
            GeneratorConfig::T2_DelayedAffine {
                driver: d0.clone(),
                lag: 4,
                mul_a: 1,
                add_b: 1,
            },
        ),
        (
            "DelayedAffine(d=4,k=2)",
            Width::BITS_2,
            GeneratorConfig::T2_DelayedAffine {
                driver: d0.clone(),
                lag: 4,
                mul_a: 1,
                add_b: 1,
            },
        ),
        (
            "CrossAdd(d=1,2,k=1)",
            Width::BITS_1,
            GeneratorConfig::T2_CrossLinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossLinearOp::Add,
            },
        ),
        (
            "CrossAdd(d=1,2,k=2)",
            Width::BITS_2,
            GeneratorConfig::T2_CrossLinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossLinearOp::Add,
            },
        ),
        (
            "CrossSub(d=1,2,k=2)",
            Width::BITS_2,
            GeneratorConfig::T2_CrossLinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossLinearOp::Sub,
            },
        ),
        (
            "CrossMul(d=1,2,k=1)",
            Width::BITS_1,
            GeneratorConfig::T2_CrossNonlinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossNonlinearOp::Mul,
            },
        ),
        (
            "CrossMul(d=1,2,k=2)",
            Width::BITS_2,
            GeneratorConfig::T2_CrossNonlinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossNonlinearOp::Mul,
            },
        ),
        (
            "CrossMulEqz(d=1,2,k=1)",
            Width::BITS_1,
            GeneratorConfig::T2_CrossNonlinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossNonlinearOp::MulEqz,
            },
        ),
        (
            "CrossMulEqz(d=1,2,k=2)",
            Width::BITS_2,
            GeneratorConfig::T2_CrossNonlinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossNonlinearOp::MulEqz,
            },
        ),
        (
            "CrossAddEqz(d=1,2,k=1)",
            Width::BITS_1,
            GeneratorConfig::T2_CrossNonlinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossNonlinearOp::AddEqz,
            },
        ),
        (
            "CrossAddEqz(d=1,2,k=2)",
            Width::BITS_2,
            GeneratorConfig::T2_CrossNonlinear {
                driver_0: d0,
                driver_1: d1,
                lag_0: 1,
                lag_1: 2,
                op: CrossNonlinearOp::AddEqz,
            },
        ),
    ];

    let seeds_bank = SeedBank::new(master_seed);
    let mut seed_cliff = CliffCountersSummary::default();
    let mut excluded_cases = 0usize;
    let mut failures = Vec::new();
    let mut all_ok = true;

    for (label, width, gen_cfg) in tasks {
        let generator = gen_cfg.build(width).expect("valid T2 generator");
        let channels = generator.channels();
        let target = usize::from(channels) - 1;
        let mut env = NoisyEnvironment::new(generator, NoiseModel::None, seeds_bank).unwrap();
        let coupled = run_learner_on_env(
            &mut env,
            width,
            channels,
            WireDelayMode::Combinational,
            horizon,
            seeds_bank,
            None,
        );
        seed_cliff.accumulate(&coupled.cliff);
        if coupled.excluded {
            excluded_cases += 1;
        }
        let target_sc = &coupled.scorecard.per_channel[target];
        let cross_wired = circuit_target_cone_uses_cross_channel(
            coupled.learner.editable().circuit(),
            target as u16,
        );
        let coupled_ok = !coupled.excluded
            && target_sc.post_transient_clean_mismatches == 0
            && target_sc.excess_residual_bits_over_h_mu <= 0.005
            && cross_wired;

        // Marginal K=1 target-only control must fail to lock (clean_err >= 0.10).
        let marg_gen =
            MarginalTargetGenerator::new(gen_cfg.build(width).expect("valid T2 gen"), target);
        let mut marg_env = NoisyEnvironment::new(marg_gen, NoiseModel::None, seeds_bank).unwrap();
        let marg = run_learner_on_env(
            &mut marg_env,
            width,
            1,
            WireDelayMode::Combinational,
            horizon,
            seeds_bank,
            None,
        );
        seed_cliff.accumulate(&marg.cliff);
        if marg.excluded {
            excluded_cases += 1;
        }
        let marg_err = (marg.scorecard.per_channel[0].post_transient_clean_mismatches as f64)
            / (marg.scorecard.evaluated_ticks.max(1) as f64);
        let marginal_ok = !marg.excluded && marg_err >= 0.10;

        if !(coupled_ok && marginal_ok) {
            all_ok = false;
            failures.push(format!(
                "G2A.1 {label} seed={master_seed}: clean_mis={} excess={:.4} cross_wired={cross_wired} marg_err={marg_err:.4} excl={}/{}",
                target_sc.post_transient_clean_mismatches,
                target_sc.excess_residual_bits_over_h_mu,
                coupled.excluded,
                marg.excluded
            ));
        }
    }

    SeedOutcome {
        passed: all_ok && excluded_cases == 0,
        cliff: seed_cliff,
        excluded_cases,
        failures,
    }
}

fn evaluate_g2a2_seed(master_seed: u64, horizon: u64) -> SeedOutcome {
    let d_hi = driver_hi_0();
    let d0 = driver_small_0();
    let d1 = driver_small_1();
    let seeds_bank = SeedBank::new(master_seed);
    let mut seed_cliff = CliffCountersSummary::default();
    let mut excluded_cases = 0usize;
    let mut failures = Vec::new();
    let mut all_ok = true;

    // (a) Deep non-aliasing Combinational tasks (d in {5, 6, 8}) across k in {1, 2}.
    for width in [Width::BITS_1, Width::BITS_2] {
        for lag in [5usize, 6, 8] {
            let gen_cfg = GeneratorConfig::T2_DelayedAffine {
                driver: d_hi.clone(),
                lag,
                mul_a: 1,
                add_b: 1,
            };
            let generator = gen_cfg.build(width).expect("valid deep T2 generator");
            let mut env = NoisyEnvironment::new(generator, NoiseModel::None, seeds_bank).unwrap();
            let res = run_learner_on_env(
                &mut env,
                width,
                2,
                WireDelayMode::Combinational,
                horizon,
                seeds_bank,
                None,
            );
            seed_cliff.accumulate(&res.cliff);
            if res.excluded {
                excluded_cases += 1;
            }
            let target_sc = &res.scorecard.per_channel[1];
            let ok = !res.excluded
                && target_sc.post_transient_clean_mismatches == 0
                && res.cliff.lag_deepening_steps >= 1;
            if !ok {
                all_ok = false;
                failures.push(format!(
                    "G2A.2a DeepAffine(d={lag},k={}) seed={master_seed}: clean_mis={} deep_steps={} excl={}",
                    width.bits(),
                    target_sc.post_transient_clean_mismatches,
                    res.cliff.lag_deepening_steps,
                    res.excluded
                ));
            }
        }
    }

    // (a.2) M1 K=1 ring-before-deepening invariance check.
    {
        let gen_cfg = GeneratorConfig::T1_PeriodicMotif {
            motif: vec![0, 0, 0, 0, 1],
        };
        let generator = gen_cfg.build(Width::BITS_1).unwrap();
        let mut env = NoisyEnvironment::new(generator, NoiseModel::None, seeds_bank).unwrap();
        let res = run_learner_on_env(
            &mut env,
            Width::BITS_1,
            1,
            WireDelayMode::Combinational,
            horizon,
            seeds_bank,
            None,
        );
        seed_cliff.accumulate(&res.cliff);
        if res.excluded {
            excluded_cases += 1;
        }
        let ok = !res.excluded
            && res.scorecard.per_channel[0].post_transient_clean_mismatches == 0
            && res.learner.ring_closure_events() >= 1
            && res.cliff.lag_deepening_steps == 0;
        if !ok {
            all_ok = false;
            failures.push(format!(
                "G2A.2a M1-RingInvariance seed={master_seed}: clean_mis={} rings={} deep_steps={}",
                res.scorecard.per_channel[0].post_transient_clean_mismatches,
                res.learner.ring_closure_events(),
                res.cliff.lag_deepening_steps
            ));
        }
    }

    // (b) TicksPerHop causal latency alignment (strict edge.hops == 1).
    let tph_tasks: Vec<(&str, Width, GeneratorConfig)> = vec![
        (
            "TphDelayedAffine(d=3,k=1)",
            Width::BITS_1,
            GeneratorConfig::T2_DelayedAffine {
                driver: d0.clone(),
                lag: 3,
                mul_a: 1,
                add_b: 1,
            },
        ),
        (
            "TphDelayedAffine(d=3,k=2)",
            Width::BITS_2,
            GeneratorConfig::T2_DelayedAffine {
                driver: d0.clone(),
                lag: 3,
                mul_a: 3,
                add_b: 1,
            },
        ),
        (
            "TphCrossSum(d=3,3,k=2)",
            Width::BITS_2,
            GeneratorConfig::T2_CrossLinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 3,
                lag_1: 3,
                op: CrossLinearOp::Add,
            },
        ),
        (
            "TphCrossSum(d=3,5,k=2)",
            Width::BITS_2,
            GeneratorConfig::T2_CrossLinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 3,
                lag_1: 5,
                op: CrossLinearOp::Add,
            },
        ),
        (
            "TphAddEqz(d=3,4,k=2)",
            Width::BITS_2,
            GeneratorConfig::T2_CrossNonlinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 3,
                lag_1: 4,
                op: CrossNonlinearOp::AddEqz,
            },
        ),
        (
            "TphMulEqzPair(d=4,4,k=1)",
            Width::BITS_1,
            GeneratorConfig::T2_CrossNonlinear {
                driver_0: d0,
                driver_1: d1,
                lag_0: 4,
                lag_1: 4,
                op: CrossNonlinearOp::MulEqzPair,
            },
        ),
    ];

    for (label, width, gen_cfg) in tph_tasks {
        let generator = gen_cfg.build(width).expect("valid Tph generator");
        let channels = generator.channels();
        let target = usize::from(channels) - 1;
        let mut env = NoisyEnvironment::new(generator, NoiseModel::None, seeds_bank).unwrap();
        let res = run_learner_on_env(
            &mut env,
            width,
            channels,
            WireDelayMode::TicksPerHop,
            horizon,
            seeds_bank,
            None,
        );
        seed_cliff.accumulate(&res.cliff);
        if res.excluded {
            excluded_cases += 1;
        }
        let edges_unit_hop = res
            .learner
            .editable()
            .circuit()
            .edges()
            .iter()
            .all(|e| e.hops == 1);
        let target_sc = &res.scorecard.per_channel[target];
        let ok = !res.excluded && target_sc.post_transient_clean_mismatches == 0 && edges_unit_hop;
        if !ok {
            all_ok = false;
            failures.push(format!(
                "G2A.2b {label} seed={master_seed}: clean_mis={} edges_unit_hop={edges_unit_hop} excl={}",
                target_sc.post_transient_clean_mismatches, res.excluded
            ));
        }
    }

    SeedOutcome {
        passed: all_ok && excluded_cases == 0,
        cliff: seed_cliff,
        excluded_cases,
        failures,
    }
}

fn evaluate_g2a3_seed(master_seed: u64, horizon: u64) -> SeedOutcome {
    let panels: Vec<(&str, Width, Vec<Vec<u64>>)> = vec![
        ("K=2,k=1", Width::BITS_1, vec![vec![0, 1], vec![0, 1, 1]]),
        ("K=2,k=2", Width::BITS_2, vec![vec![0, 3], vec![0, 1, 3]]),
        (
            "K=3,k=1",
            Width::BITS_1,
            vec![vec![0, 1], vec![0, 1, 1], vec![0, 0, 1]],
        ),
        (
            "K=3,k=2",
            Width::BITS_2,
            vec![vec![0, 3], vec![0, 1, 3], vec![0, 0, 3]],
        ),
        (
            "K=4,k=1",
            Width::BITS_1,
            vec![vec![0, 1], vec![0, 1, 1], vec![0, 0, 1], vec![0, 0, 0, 1]],
        ),
        (
            "K=4,k=2",
            Width::BITS_2,
            vec![vec![0, 3], vec![0, 1, 3], vec![0, 0, 3], vec![1, 2, 3]],
        ),
    ];

    let w_diag = GrowthConfig::default().diagnostic_window as f64;
    let seeds_bank = SeedBank::new(master_seed);
    let mut seed_cliff = CliffCountersSummary::default();
    let mut excluded_cases = 0usize;
    let mut failures = Vec::new();
    let mut all_ok = true;

    // Cache solo K=1 runs by (k_bits, wire_delay_tag, motif) within this seed so shared channels
    // across K=2,3,4 are evaluated only once per (k, wire_delay, motif).
    let mut solo_cache: BTreeMap<(u8, u8, Vec<u64>), Option<u64>> = BTreeMap::new();

    for wire_delay in [WireDelayMode::Combinational, WireDelayMode::TicksPerHop] {
        let wd_tag = match wire_delay {
            WireDelayMode::Combinational => 0u8,
            WireDelayMode::TicksPerHop => 1u8,
        };
        for (panel_label, width, motifs) in &panels {
            let mut solo_locks = Vec::with_capacity(motifs.len());
            for motif in motifs {
                let key = (width.bits(), wd_tag, motif.clone());
                let t_solo = if let Some(&cached) = solo_cache.get(&key) {
                    cached
                } else {
                    let s_gen = MultiChannelPeriodicGenerator::new(
                        format!("G2A3Solo:{panel_label}"),
                        *width,
                        vec![motif.clone()],
                    );
                    let mut s_env =
                        NoisyEnvironment::new(s_gen, NoiseModel::None, seeds_bank).unwrap();
                    let s_res = run_learner_on_env(
                        &mut s_env, *width, 1, wire_delay, horizon, seeds_bank, None,
                    );
                    seed_cliff.accumulate(&s_res.cliff);
                    if s_res.excluded {
                        excluded_cases += 1;
                        all_ok = false;
                    }
                    let t_lock = if s_res.excluded
                        || s_res.scorecard.per_channel[0].post_transient_clean_mismatches != 0
                    {
                        None
                    } else {
                        s_res.scorecard.per_channel[0].ticks_to_clean_solution
                    };
                    solo_cache.insert(key, t_lock);
                    t_lock
                };
                solo_locks.push(t_solo);
            }

            let m_gen = MultiChannelPeriodicGenerator::new(
                format!("G2A3Multi:{panel_label}"),
                *width,
                motifs.clone(),
            );
            let k_ports = m_gen.channels();
            let mut m_env = NoisyEnvironment::new(m_gen, NoiseModel::None, seeds_bank).unwrap();
            let m_res = run_learner_on_env(
                &mut m_env, *width, k_ports, wire_delay, horizon, seeds_bank, None,
            );
            seed_cliff.accumulate(&m_res.cliff);
            if m_res.excluded {
                excluded_cases += 1;
                all_ok = false;
            }

            for (ch, solo_t) in solo_locks.iter().enumerate() {
                let ch_sc = &m_res.scorecard.per_channel[ch];
                let ch_ok = match (*solo_t, ch_sc.ticks_to_clean_solution) {
                    (Some(ts), Some(tm)) if !m_res.excluded => {
                        ch_sc.post_transient_clean_mismatches == 0
                            && (tm as f64) <= 1.50 * (ts as f64).max(w_diag)
                    }
                    _ => false,
                };
                if !ch_ok {
                    all_ok = false;
                    failures.push(format!(
                        "G2A.3 {panel_label} {wire_delay:?} ch={ch} seed={master_seed}: \
                         solo={solo_t:?} multi={:?} clean_mis={}",
                        ch_sc.ticks_to_clean_solution, ch_sc.post_transient_clean_mismatches
                    ));
                }
            }
        }
    }

    SeedOutcome {
        passed: all_ok && excluded_cases == 0,
        cliff: seed_cliff,
        excluded_cases,
        failures,
    }
}

enum ReactiveTargetRule {
    DelayedAffine {
        lag: usize,
        mul_a: u64,
        add_b: u64,
    },
    CrossLinear {
        lag_0: usize,
        lag_1: usize,
        op: CrossLinearOp,
    },
}

/// Strictly causal reactive oracle reading ONLY realized observable boundary inputs (`obs[0..t-1]`,
/// zero latent-state leakage) for paired `G2A.4b` regret evaluation under all-channel noise.
struct CausalReactiveOracle {
    width: Width,
    channels: u16,
    rule: ReactiveTargetRule,
    observed_history: Vec<Vec<u64>>,
}

impl CausalReactiveOracle {
    fn from_config(width: Width, channels: u16, cfg: &GeneratorConfig) -> Self {
        let rule = match cfg {
            GeneratorConfig::T2_DelayedAffine {
                lag, mul_a, add_b, ..
            } => ReactiveTargetRule::DelayedAffine {
                lag: *lag,
                mul_a: width.normalize(*mul_a),
                add_b: width.normalize(*add_b),
            },
            GeneratorConfig::T2_CrossLinear {
                lag_0, lag_1, op, ..
            } => ReactiveTargetRule::CrossLinear {
                lag_0: *lag_0,
                lag_1: *lag_1,
                op: *op,
            },
            other => panic!("unsupported G2A.4b config for CausalReactiveOracle: {other:?}"),
        };
        Self {
            width,
            channels,
            rule,
            observed_history: Vec::new(),
        }
    }
}

impl PredictorBaseline for CausalReactiveOracle {
    fn name(&self) -> &str {
        "CausalReactiveOracle"
    }

    fn budget_accounting_mode(&self) -> BudgetAccountingMode {
        BudgetAccountingMode::AnalyticalUnmetered
    }

    fn predict(&mut self, _budget: &mut TickBudget) -> Vec<u64> {
        let mut out = vec![0u64; usize::from(self.channels)];
        let t = self.observed_history.len();
        let target_idx = usize::from(self.channels).saturating_sub(1);
        let pred_target = match self.rule {
            ReactiveTargetRule::DelayedAffine { lag, mul_a, add_b } => {
                let x = if t >= lag {
                    self.observed_history[t - lag][0]
                } else {
                    0
                };
                self.width
                    .normalize(mul_a.wrapping_mul(x).wrapping_add(add_b))
            }
            ReactiveTargetRule::CrossLinear { lag_0, lag_1, op } => {
                let x0 = if t >= lag_0 {
                    self.observed_history[t - lag_0][0]
                } else {
                    0
                };
                let x1 = if t >= lag_1 {
                    self.observed_history[t - lag_1][1]
                } else {
                    0
                };
                match op {
                    CrossLinearOp::Add => self.width.normalize(x0.wrapping_add(x1)),
                    CrossLinearOp::Sub => self.width.normalize(x0.wrapping_sub(x1)),
                }
            }
        };
        if target_idx < out.len() {
            out[target_idx] = pred_target;
        }
        out
    }

    fn observe(&mut self, observed: &[u64], _budget: &mut TickBudget) -> Option<f64> {
        let frame: Vec<u64> = observed.iter().map(|&v| self.width.normalize(v)).collect();
        self.observed_history.push(frame);
        None
    }

    fn best_circuit(&self) -> Option<&Circuit> {
        None
    }
}

struct G2a4SeedOutcome {
    outcome: SeedOutcome,
    /// Per-task `(kt_learner, h_reactive)` samples for the 4 all-channel noise tasks.
    all_noise_samples: [(f64, f64); 4],
}

fn max_g2a4b_mean_h_reactive_dev(samples: &[[(f64, f64); 4]]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let n = samples.len() as f64;
    let mut max_dev = 0.0f64;
    for task_idx in 0..4 {
        let mean_kt = samples.iter().map(|s| s[task_idx].0).sum::<f64>() / n;
        let h_react = samples[0][task_idx].1;
        max_dev = max_dev.max((mean_kt - h_react).abs());
    }
    max_dev
}

fn evaluate_g2a4_seed(master_seed: u64, horizon: u64) -> G2a4SeedOutcome {
    const EPSILON: f64 = 0.05;
    let d0 = driver_small_0();
    let d1 = driver_small_1();
    let seeds_bank = SeedBank::new(master_seed);
    let mut seed_cliff = CliffCountersSummary::default();
    let mut excluded_cases = 0usize;
    let mut failures = Vec::new();
    let mut all_ok = true;

    // (a) Target-only noise (paired target-channel KT regret vs clean-target Bayes oracle <= 0.030).
    let target_only_tasks: Vec<(&str, Width, GeneratorConfig)> = vec![
        (
            "TargetNoise:DelayedAffine(d=2,k=1)",
            Width::BITS_1,
            GeneratorConfig::T2_DelayedAffine {
                driver: d0.clone(),
                lag: 2,
                mul_a: 1,
                add_b: 1,
            },
        ),
        (
            "TargetNoise:DelayedAffine(d=2,k=2)",
            Width::BITS_2,
            GeneratorConfig::T2_DelayedAffine {
                driver: d0.clone(),
                lag: 2,
                mul_a: 1,
                add_b: 1,
            },
        ),
        (
            "TargetNoise:CrossAdd(d=1,2,k=1)",
            Width::BITS_1,
            GeneratorConfig::T2_CrossLinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossLinearOp::Add,
            },
        ),
        (
            "TargetNoise:CrossAdd(d=1,2,k=2)",
            Width::BITS_2,
            GeneratorConfig::T2_CrossLinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossLinearOp::Add,
            },
        ),
        (
            "TargetNoise:CrossMulEqz(d=1,2,k=2)",
            Width::BITS_2,
            GeneratorConfig::T2_CrossNonlinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossNonlinearOp::MulEqz,
            },
        ),
    ];

    for (label, width, gen_cfg) in target_only_tasks {
        let generator = gen_cfg.build(width).unwrap();
        let channels = generator.channels();
        let target = usize::from(channels) - 1;
        let noise = NoiseModel::IndependentSubstitution {
            epsilon: EPSILON,
            channels: vec![target as u16],
        };
        let gt = compute_ground_truth_yardstick(
            &gen_cfg,
            width,
            WireDelayMode::Combinational,
            &noise,
            512,
            None,
        )
        .expect("ground truth yardstick succeeds on small coprime LFSRs");
        let mut env = NoisyEnvironment::new(generator, noise, seeds_bank).unwrap();
        let res = run_learner_on_env(
            &mut env,
            width,
            channels,
            WireDelayMode::Combinational,
            horizon,
            seeds_bank,
            gt.epsilon_machine.as_ref(),
        );
        seed_cliff.accumulate(&res.cliff);
        if res.excluded {
            excluded_cases += 1;
        }
        let target_sc = &res.scorecard.per_channel[target];
        let regret = target_sc.paired_residual_regret_bits_per_symbol;
        let ok = !res.excluded && regret.is_some_and(|r| r <= 0.030);
        if !ok {
            all_ok = false;
            failures.push(format!(
                "G2A.4a {label} seed={master_seed}: paired_regret={regret:?} kt={:.4} excl={}",
                target_sc.prequential_kt_residual_bits_per_symbol, res.excluded
            ));
        }
    }

    // (b) All-channel observation noise (channels = 0..K): within <= 0.040 b/sym of H_reactive(eps, m, k).
    let all_channel_tasks: Vec<(&str, Width, usize, GeneratorConfig)> = vec![
        (
            "AllNoise:DelayedAffine(d=2,k=1)",
            Width::BITS_1,
            1,
            GeneratorConfig::T2_DelayedAffine {
                driver: d0.clone(),
                lag: 2,
                mul_a: 1,
                add_b: 1,
            },
        ),
        (
            "AllNoise:DelayedAffine(d=2,k=2)",
            Width::BITS_2,
            1,
            GeneratorConfig::T2_DelayedAffine {
                driver: d0.clone(),
                lag: 2,
                mul_a: 1,
                add_b: 1,
            },
        ),
        (
            "AllNoise:CrossAdd(d=1,2,k=1)",
            Width::BITS_1,
            2,
            GeneratorConfig::T2_CrossLinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossLinearOp::Add,
            },
        ),
        (
            "AllNoise:CrossAdd(d=1,2,k=2)",
            Width::BITS_2,
            2,
            GeneratorConfig::T2_CrossLinear {
                driver_0: d0,
                driver_1: d1,
                lag_0: 1,
                lag_1: 2,
                op: CrossLinearOp::Add,
            },
        ),
    ];

    let mut all_noise_samples = [(0.0f64, 0.0f64); 4];
    for (task_idx, (label, width, m_inputs, gen_cfg)) in all_channel_tasks.into_iter().enumerate() {
        let generator = gen_cfg.build(width).unwrap();
        let channels = generator.channels();
        let target = usize::from(channels) - 1;
        let noise = NoiseModel::IndependentSubstitution {
            epsilon: EPSILON,
            channels: (0..channels).collect(),
        };
        let mut oracle_env =
            NoisyEnvironment::new(gen_cfg.build(width).unwrap(), noise.clone(), seeds_bank)
                .unwrap();
        let mut oracle = CausalReactiveOracle::from_config(width, channels, &gen_cfg);
        let oracle_sc = run_prequential_harness(
            &mut oracle_env,
            &mut oracle,
            horizon,
            DEFAULT_M2A_WARMUP,
            DEFAULT_M2A_BUDGET,
            None,
            None,
        );
        let oracle_kt = oracle_sc.per_channel[target].prequential_kt_residual_bits_per_symbol;

        let mut env = NoisyEnvironment::new(generator, noise, seeds_bank).unwrap();
        let res = run_learner_on_env(
            &mut env,
            width,
            channels,
            WireDelayMode::Combinational,
            horizon,
            seeds_bank,
            None,
        );
        seed_cliff.accumulate(&res.cliff);
        if res.excluded {
            excluded_cases += 1;
        }
        let target_sc = &res.scorecard.per_channel[target];
        let kt = target_sc.prequential_kt_residual_bits_per_symbol;
        let h_react = compound_reactive_entropy_bits(EPSILON, m_inputs, width.bits());
        all_noise_samples[task_idx] = (kt, h_react);
        let paired_diff = (kt - oracle_kt).abs();
        let ok = !res.excluded && paired_diff <= 0.040;
        if !ok {
            all_ok = false;
            failures.push(format!(
                "G2A.4b {label} seed={master_seed}: kt={kt:.4} oracle_kt={oracle_kt:.4} |paired_diff|={paired_diff:.4} H_reactive={h_react:.4} excl={}",
                res.excluded
            ));
        }
    }

    G2a4SeedOutcome {
        outcome: SeedOutcome {
            passed: all_ok && excluded_cases == 0,
            cliff: seed_cliff,
            excluded_cases,
            failures,
        },
        all_noise_samples,
    }
}

#[derive(Clone, Debug)]
struct G2a5TaskSpec {
    label: &'static str,
    width: Width,
    nonlinear: bool,
    gen_cfg: GeneratorConfig,
}

fn g2a5_panel_specs() -> Vec<G2a5TaskSpec> {
    let d0 = driver_hi_0();
    let d1 = driver_hi_1();
    vec![
        G2a5TaskSpec {
            label: "CrossSum(d=1,2,k=1)",
            width: Width::BITS_1,
            nonlinear: false,
            gen_cfg: GeneratorConfig::T2_CrossLinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossLinearOp::Add,
            },
        },
        G2a5TaskSpec {
            label: "CrossSum(d=1,2,k=2)",
            width: Width::BITS_2,
            nonlinear: false,
            gen_cfg: GeneratorConfig::T2_CrossLinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossLinearOp::Add,
            },
        },
        G2a5TaskSpec {
            label: "CrossMulEqz(d=1,2,k=2)",
            width: Width::BITS_2,
            nonlinear: true,
            gen_cfg: GeneratorConfig::T2_CrossNonlinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossNonlinearOp::MulEqz,
            },
        },
        G2a5TaskSpec {
            label: "CrossAddEqz(d=1,2,k=2)",
            width: Width::BITS_2,
            nonlinear: true,
            gen_cfg: GeneratorConfig::T2_CrossNonlinear {
                driver_0: d0,
                driver_1: d1,
                lag_0: 1,
                lag_1: 2,
                op: CrossNonlinearOp::AddEqz,
            },
        },
    ]
}

#[derive(Clone, Debug)]
struct G2a5RunMetrics {
    target_kt: f64,
    native_prob_log_loss: Option<f64>,
    target_clean_match_rate: f64,
    compute_ticks_consumed: u64,
    mode: BudgetAccountingMode,
}

impl G2a5RunMetrics {
    fn from_scorecard(sc: &Scorecard, target: usize) -> Self {
        let ch = &sc.per_channel[target];
        Self {
            target_kt: ch.prequential_kt_residual_bits_per_symbol,
            native_prob_log_loss: sc.native_prob_log_loss_bits_per_symbol,
            target_clean_match_rate: ch.post_transient_clean_match_rate,
            compute_ticks_consumed: sc.candidate_replay_ticks_consumed,
            mode: sc.budget_accounting_mode,
        }
    }
}

fn run_baseline_on_g2a5(
    spec: &G2a5TaskSpec,
    mut baseline: Box<dyn PredictorBaseline>,
    horizon: u64,
    noise_seed: SeedBank,
) -> G2a5RunMetrics {
    let generator = spec.gen_cfg.build(spec.width).unwrap();
    let target = usize::from(generator.channels()) - 1;
    let mut env = NoisyEnvironment::new(generator, NoiseModel::None, noise_seed).unwrap();
    let sc = run_prequential_harness(
        &mut env,
        baseline.as_mut(),
        horizon,
        DEFAULT_M2A_WARMUP,
        DEFAULT_M2A_BUDGET,
        None,
        None,
    );
    G2a5RunMetrics::from_scorecard(&sc, target)
}

#[derive(Clone, Debug)]
struct DeterministicG2a5Cache {
    ctw: Vec<G2a5RunMetrics>,
    reeds_sloane: Vec<G2a5RunMetrics>,
    cross_ctw: Vec<G2a5RunMetrics>,
}

fn build_deterministic_g2a5_cache(panel: &[G2a5TaskSpec], horizon: u64) -> DeterministicG2a5Cache {
    let zero_seed = SeedBank::new(0);
    let mut ctw = Vec::with_capacity(panel.len());
    let mut reeds_sloane = Vec::with_capacity(panel.len());
    let mut cross_ctw = Vec::with_capacity(panel.len());

    for spec in panel {
        let channels = spec.gen_cfg.build(spec.width).unwrap().channels();
        ctw.push(run_baseline_on_g2a5(
            spec,
            Box::new(CtwBaseline::new(spec.width, channels, 9)),
            horizon,
            zero_seed,
        ));
        reeds_sloane.push(run_baseline_on_g2a5(
            spec,
            Box::new(ReedsSloaneBaseline::new(spec.width, channels)),
            horizon,
            zero_seed,
        ));
        cross_ctw.push(run_baseline_on_g2a5(
            spec,
            Box::new(CrossChannelCtwBaseline::with_default_depth(
                spec.width, channels,
            )),
            horizon,
            zero_seed,
        ));
    }

    DeterministicG2a5Cache {
        ctw,
        reeds_sloane,
        cross_ctw,
    }
}

struct G2a5SeedResult {
    passed: bool,
    cliff: CliffCountersSummary,
    excluded_cases: usize,
    /// Per task index: [Arkhe, RandomSearch, CGP, CTW, ReedsSloane, CrossCTW, RNN].
    per_task_metrics: Vec<[G2a5RunMetrics; 7]>,
    failures: Vec<String>,
}

/// Per-seed G2A.5 pass condition against online search baselines (`RandomSearch`, `CGP`):
/// either Arkhe achieves >= 0.25 b/sym KT residual advantage, or both predictors attain
/// exact `100%` post-transient clean match rate within floating-point parity (`<= 1e-6` b/sym).
fn g2a5_search_baseline_seed_ok(arkhe_m: &G2a5RunMetrics, baseline_m: &G2a5RunMetrics) -> bool {
    (baseline_m.target_kt - arkhe_m.target_kt) >= 0.25
        || (arkhe_m.target_clean_match_rate >= 1.0 - 1e-12
            && baseline_m.target_clean_match_rate >= 1.0 - 1e-12
            && (arkhe_m.target_kt - baseline_m.target_kt).abs() <= 1e-6)
}

fn evaluate_g2a5_seed(
    master_seed: u64,
    horizon: u64,
    panel: &[G2a5TaskSpec],
    det_cache: &DeterministicG2a5Cache,
) -> G2a5SeedResult {
    let seeds_bank = SeedBank::new(master_seed);
    let mut cliff = CliffCountersSummary::default();
    let mut excluded_cases = 0usize;
    let mut per_task_metrics = Vec::with_capacity(panel.len());
    let mut failures = Vec::new();
    let mut all_ok = true;

    for (idx, spec) in panel.iter().enumerate() {
        let generator = spec.gen_cfg.build(spec.width).unwrap();
        let channels = generator.channels();
        let target = usize::from(channels) - 1;
        let mut env = NoisyEnvironment::new(generator, NoiseModel::None, seeds_bank).unwrap();
        let arkhe = run_learner_on_env(
            &mut env,
            spec.width,
            channels,
            WireDelayMode::Combinational,
            horizon,
            seeds_bank,
            None,
        );
        cliff.accumulate(&arkhe.cliff);
        if arkhe.excluded {
            excluded_cases += 1;
            all_ok = false;
        }
        let arkhe_m = G2a5RunMetrics::from_scorecard(&arkhe.scorecard, target);
        let rs_m = run_baseline_on_g2a5(
            spec,
            Box::new(RandomSearchBaseline::new(
                spec.width,
                channels,
                WireDelayMode::Combinational,
                5,
                128,
                32,
                seeds_bank,
            )),
            horizon,
            seeds_bank,
        );
        let cgp_m = run_baseline_on_g2a5(
            spec,
            Box::new(CgpBaseline::new(
                spec.width,
                channels,
                WireDelayMode::Combinational,
                4,
                8,
                128,
                seeds_bank,
            )),
            horizon,
            seeds_bank,
        );
        let ctw_m = det_cache.ctw[idx].clone();
        let bm_m = det_cache.reeds_sloane[idx].clone();
        let cross_ctw_m = det_cache.cross_ctw[idx].clone();
        let rnn_m = run_baseline_on_g2a5(
            spec,
            Box::new(RnnBaseline::with_defaults(spec.width, channels, seeds_bank).unwrap()),
            horizon,
            seeds_bank,
        );

        let rs_ok = g2a5_search_baseline_seed_ok(&arkhe_m, &rs_m);
        let cgp_ok = g2a5_search_baseline_seed_ok(&arkhe_m, &cgp_m);
        let mut task_ok = !arkhe.excluded && rs_ok && cgp_ok;
        if spec.nonlinear {
            let margin_ctw = ctw_m.target_kt - arkhe_m.target_kt;
            let margin_bm = bm_m.target_kt - arkhe_m.target_kt;
            task_ok = task_ok && margin_ctw >= 0.25 && margin_bm >= 0.25;
        }
        if !task_ok {
            all_ok = false;
            failures.push(format!(
                "G2A.5 {} seed={master_seed}: arkhe={:.4} rs={:.4} cgp={:.4} ctw={:.4} bm={:.4}",
                spec.label,
                arkhe_m.target_kt,
                rs_m.target_kt,
                cgp_m.target_kt,
                ctw_m.target_kt,
                bm_m.target_kt
            ));
        }

        per_task_metrics.push([arkhe_m, rs_m, cgp_m, ctw_m, bm_m, cross_ctw_m, rnn_m]);
    }

    G2a5SeedResult {
        passed: all_ok && excluded_cases == 0,
        cliff,
        excluded_cases,
        per_task_metrics,
        failures,
    }
}

const G2A5_BASELINE_NAMES: [&str; 7] = [
    "ArkheM1",
    "RandomSearch",
    "CGP",
    "CTW",
    "ReedsSloane",
    "CrossChannelCTW",
    "RnnBptt",
];

fn aggregate_g2a5_benchmarks(
    panel: &[G2a5TaskSpec],
    seed_results: &[Vec<[G2a5RunMetrics; 7]>],
) -> (Vec<G2a5TaskBenchmarkReport>, f64) {
    let mut reports = Vec::with_capacity(panel.len());
    let mut min_required_margin = f64::INFINITY;
    let total_runs = seed_results.len();

    for (task_idx, spec) in panel.iter().enumerate() {
        let mut baselines = Vec::with_capacity(7);
        let arkhe_mean_kt = if total_runs == 0 {
            0.0
        } else {
            seed_results
                .iter()
                .map(|r| r[task_idx][0].target_kt)
                .sum::<f64>()
                / (total_runs as f64)
        };

        for (b_idx, &b_name) in G2A5_BASELINE_NAMES.iter().enumerate() {
            if total_runs == 0 {
                continue;
            }
            let sum_kt: f64 = seed_results
                .iter()
                .map(|r| r[task_idx][b_idx].target_kt)
                .sum();
            let sum_match: f64 = seed_results
                .iter()
                .map(|r| r[task_idx][b_idx].target_clean_match_rate)
                .sum();
            let sum_ticks: f64 = seed_results
                .iter()
                .map(|r| r[task_idx][b_idx].compute_ticks_consumed as f64)
                .sum();
            let prob_vals: Vec<f64> = seed_results
                .iter()
                .filter_map(|r| r[task_idx][b_idx].native_prob_log_loss)
                .collect();
            let mean_prob = if prob_vals.len() == total_runs {
                Some(prob_vals.iter().sum::<f64>() / (total_runs as f64))
            } else {
                None
            };
            let mean_kt = sum_kt / (total_runs as f64);
            let mean_adv = mean_kt - arkhe_mean_kt;
            if matches!(b_idx, 1 | 2) || (spec.nonlinear && matches!(b_idx, 3 | 4)) {
                min_required_margin = min_required_margin.min(mean_adv);
            }
            baselines.push(G2a5BaselineAggregate {
                baseline: b_name.to_owned(),
                budget_accounting_mode: seed_results[0][task_idx][b_idx].mode,
                mean_target_kt_bits_per_symbol: mean_kt,
                mean_native_prob_log_loss_bits_per_symbol: mean_prob,
                mean_target_clean_match_rate: sum_match / (total_runs as f64),
                mean_compute_ticks_consumed: sum_ticks / (total_runs as f64),
                mean_arkhe_kt_advantage_bits: mean_adv,
                runs: total_runs,
            });
        }

        reports.push(G2a5TaskBenchmarkReport {
            task: spec.label.to_owned(),
            k_bits: spec.width.bits(),
            nonlinear: spec.nonlinear,
            baselines,
        });
    }

    if !min_required_margin.is_finite() {
        min_required_margin = 0.0;
    }
    (reports, min_required_margin)
}

struct EvaluatedSingleBank {
    summary: M2aVerificationSummary,
    raw_g2a4b_metrics: Vec<[(f64, f64); 4]>,
    raw_g2a5_metrics: Vec<Vec<[G2a5RunMetrics; 7]>>,
}

struct AllGatesSeedOutcome {
    gates: [SeedOutcome; 4],
    g4b_samples: [(f64, f64); 4],
    g5: G2a5SeedResult,
}

fn run_single_bank_internal(
    seed_offset: u32,
    seeds: u32,
    horizon: u64,
    parallel_seeds: bool,
    det_g2a5: &DeterministicG2a5Cache,
) -> EvaluatedSingleBank {
    let seeds_tested = seeds.max(1);
    let panel = g2a5_panel_specs();

    let eval_one_seed = |seed_idx: u32| -> AllGatesSeedOutcome {
        let master_seed = u64::from(seed_offset + seed_idx);
        let g4 = evaluate_g2a4_seed(master_seed, horizon);
        AllGatesSeedOutcome {
            gates: [
                evaluate_g2a1_seed(master_seed, horizon),
                evaluate_g2a2_seed(master_seed, horizon),
                evaluate_g2a3_seed(master_seed, horizon),
                g4.outcome,
            ],
            g4b_samples: g4.all_noise_samples,
            g5: evaluate_g2a5_seed(master_seed, horizon, &panel, det_g2a5),
        }
    };

    let mut per_seed: Vec<Option<AllGatesSeedOutcome>> = (0..seeds_tested).map(|_| None).collect();

    if parallel_seeds && seeds_tested > 1 {
        let num_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .min(seeds_tested as usize)
            .max(1);
        std::thread::scope(|s| {
            let chunk_size = (seeds_tested as usize).div_ceil(num_threads);
            for (chunk_idx, slice) in per_seed.chunks_mut(chunk_size).enumerate() {
                let start = chunk_idx * chunk_size;
                s.spawn(move || {
                    for (off, slot) in slice.iter_mut().enumerate() {
                        *slot = Some(eval_one_seed((start + off) as u32));
                    }
                });
            }
        });
    } else {
        for (idx, slot) in per_seed.iter_mut().enumerate() {
            *slot = Some(eval_one_seed(idx as u32));
        }
    }

    let mut pass_counts = [0usize; 5];
    let mut excl_counts = [0usize; 5];
    let mut gate_cliffs = [CliffCountersSummary::default(); 5];
    let mut gate_failures: [Vec<String>; 5] = Default::default();
    let mut raw_g2a4b_metrics = Vec::with_capacity(seeds_tested as usize);
    let mut raw_g2a5_metrics = Vec::with_capacity(seeds_tested as usize);

    for item in per_seed {
        let AllGatesSeedOutcome {
            gates: [g1, g2, g3, g4],
            g4b_samples,
            g5,
        } = item.expect("seed evaluated");
        let outs = [
            (0, g1.passed, g1.excluded_cases, g1.cliff, g1.failures),
            (1, g2.passed, g2.excluded_cases, g2.cliff, g2.failures),
            (2, g3.passed, g3.excluded_cases, g3.cliff, g3.failures),
            (3, g4.passed, g4.excluded_cases, g4.cliff, g4.failures),
            (4, g5.passed, g5.excluded_cases, g5.cliff, g5.failures),
        ];
        for (idx, passed, excl, cliff, fails) in outs {
            if passed {
                pass_counts[idx] += 1;
            }
            excl_counts[idx] += excl;
            gate_cliffs[idx].accumulate(&cliff);
            gate_failures[idx].extend(fails);
        }
        raw_g2a4b_metrics.push(g4b_samples);
        raw_g2a5_metrics.push(g5.per_task_metrics);
    }

    let max_g4_mean_dev = max_g2a4b_mean_h_reactive_dev(&raw_g2a4b_metrics);
    let (g2a5_benchmarks, min_margin) = aggregate_g2a5_benchmarks(&panel, &raw_g2a5_metrics);
    let specs: [(&str, f64); 5] = [
        ("G2A.1", 0.90),
        ("G2A.2", 0.90),
        ("G2A.3", 0.90),
        ("G2A.4", 0.85),
        ("G2A.5", 0.85),
    ];

    let total = seeds_tested as usize;
    let mut total_cliff = CliffCountersSummary::default();
    let mut total_excluded = 0usize;
    let mut gates = Vec::with_capacity(5);
    let mut all_passed = true;

    for (idx, &(gate_name, target_rate)) in specs.iter().enumerate() {
        let passed_seeds = pass_counts[idx];
        let excl = excl_counts[idx];
        let empirical_rate = (passed_seeds as f64) / (total as f64);
        let bank_gate_stats =
            evaluate_multi_bank_gate(&[BankSample::new(passed_seeds, total)], target_rate);
        let g4_mean_ok = idx != 3 || max_g4_mean_dev <= 0.040;
        if idx == 3 && !g4_mean_ok {
            gate_failures[idx].push(format!(
                "G2A.4b bank mean |E[kt] - H_reactive| = {max_g4_mean_dev:.4} > 0.040"
            ));
        }
        let margin_ok = idx != 4 || total <= 1 || min_margin >= 0.25;
        if idx == 4 && !margin_ok {
            gate_failures[idx].push(format!(
                "G2A.5 bank min required mean KT margin = {min_margin:.4} < 0.2500"
            ));
        }
        let min_pass = ((total as f64) * target_rate).ceil() as usize;
        let passed = excl == 0 && passed_seeds >= min_pass && g4_mean_ok && margin_ok;
        if !passed {
            all_passed = false;
        }
        total_excluded += excl;
        total_cliff.accumulate(&gate_cliffs[idx]);
        gates.push(M2aGateReport {
            gate: gate_name.to_owned(),
            target_rate,
            passed,
            passed_seeds,
            total_seeds: total,
            empirical_rate,
            bank_gate_stats,
            excluded_cases: excl,
            cliff_counters: gate_cliffs[idx],
            min_required_mean_margin_bits: (idx == 4).then_some(min_margin),
            max_g2a4b_mean_h_reactive_dev_bits: (idx == 3).then_some(max_g4_mean_dev),
            failures: gate_failures[idx].clone(),
        });
    }

    // Also include explicit CaseOutcome classification check for public consistency.
    let _sanity_outcome: CaseOutcome = classify_case_outcome(all_passed, &total_cliff);
    let _unused: Option<OnlineCaseSummary> = None;

    let summary = M2aVerificationSummary {
        provenance: BuildProvenance::current(),
        seed_offset,
        seeds_tested,
        horizon,
        gates,
        g2a5_benchmarks,
        excluded_cases: total_excluded,
        cliff_counters: total_cliff,
        all_passed: all_passed && total_excluded == 0,
    };
    EvaluatedSingleBank {
        summary,
        raw_g2a4b_metrics,
        raw_g2a5_metrics,
    }
}

/// Runs the full Milestone 2a verification suite across `G2A.1`–`G2A.5` on `[seed_offset, seed_offset + seeds)`.
///
/// # Errors
///
/// Returns an error if `horizon <= DEFAULT_M2A_WARMUP` or if the seed range overlaps a guarded
/// holdout bank (`10000..=10019`, `30000..=30019` without `--holdout`, or `40000..=40019`).
pub fn run_m2a_verification_suite(
    seed_offset: u32,
    seeds: u32,
    horizon: u64,
    allow_holdout: bool,
) -> Result<M2aVerificationSummary, String> {
    if horizon <= DEFAULT_M2A_WARMUP {
        return Err(format!(
            "horizon {horizon} must exceed warmup {DEFAULT_M2A_WARMUP}"
        ));
    }
    let seeds_tested = seeds.max(1);
    check_m2a_seed_range_holdout(seed_offset, seeds_tested, allow_holdout)?;
    let panel = g2a5_panel_specs();
    let det_g2a5 = build_deterministic_g2a5_cache(&panel, horizon);
    Ok(run_single_bank_internal(seed_offset, seeds_tested, horizon, true, &det_g2a5).summary)
}

/// Runs Milestone 2a multi-bank evaluation across `banks` consecutive banks in parallel via
/// `std::thread::scope`, scoring each gate `G2A.1`–`G2A.5` via [`evaluate_multi_bank_gate`].
///
/// # Errors
///
/// Returns an error if `banks == 0`, `horizon <= DEFAULT_M2A_WARMUP`, or if `[seed_offset,
/// seed_offset + banks * seeds_per_bank)` overlaps any guarded holdout range.
pub fn run_m2a_bank_evaluation(
    seed_offset: u32,
    seeds_per_bank: u32,
    banks: u32,
    horizon: u64,
    allow_holdout: bool,
) -> Result<M2aBankReport, String> {
    if banks == 0 {
        return Err("banks count must be >= 1".to_owned());
    }
    if horizon <= DEFAULT_M2A_WARMUP {
        return Err(format!(
            "horizon {horizon} must exceed warmup {DEFAULT_M2A_WARMUP}"
        ));
    }
    let seeds_per_bank = seeds_per_bank.max(1);
    let total_seeds = seeds_per_bank.saturating_mul(banks);
    check_m2a_seed_range_holdout(seed_offset, total_seeds, allow_holdout)?;

    let panel = g2a5_panel_specs();
    let det_g2a5 = build_deterministic_g2a5_cache(&panel, horizon);

    let num_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(banks as usize)
        .max(1);

    let mut bank_outputs: Vec<Option<EvaluatedSingleBank>> = (0..banks).map(|_| None).collect();
    std::thread::scope(|s| {
        let chunk_size = (banks as usize).div_ceil(num_threads);
        for (chunk_idx, slice) in bank_outputs.chunks_mut(chunk_size).enumerate() {
            let start_bank = chunk_idx * chunk_size;
            let det_ref = &det_g2a5;
            s.spawn(move || {
                for (off, slot) in slice.iter_mut().enumerate() {
                    let b = (start_bank + off) as u32;
                    let b_offset = seed_offset.saturating_add(b.saturating_mul(seeds_per_bank));
                    *slot = Some(run_single_bank_internal(
                        b_offset,
                        seeds_per_bank,
                        horizon,
                        false,
                        det_ref,
                    ));
                }
            });
        }
    });

    let mut samples_per_gate: [Vec<BankSample>; 5] = Default::default();
    let mut banks_above_target = [0u32; 5];
    let mut gate_excl = [0usize; 5];
    let mut gate_cliffs = [CliffCountersSummary::default(); 5];
    let mut all_g2a4b_raw = Vec::with_capacity(total_seeds as usize);
    let mut all_g2a5_raw = Vec::with_capacity(total_seeds as usize);
    let mut failures = Vec::new();
    let mut total_cliff = CliffCountersSummary::default();
    let mut total_excluded = 0usize;

    for (b_idx, out_opt) in bank_outputs.into_iter().enumerate() {
        let out = out_opt.expect("bank evaluated");
        total_cliff.accumulate(&out.summary.cliff_counters);
        total_excluded += out.summary.excluded_cases;
        all_g2a4b_raw.extend(out.raw_g2a4b_metrics);
        all_g2a5_raw.extend(out.raw_g2a5_metrics);

        for (g_idx, g) in out.summary.gates.iter().enumerate() {
            samples_per_gate[g_idx].push(BankSample::new(g.passed_seeds, g.total_seeds));
            if g.passed {
                banks_above_target[g_idx] += 1;
            }
            gate_excl[g_idx] += g.excluded_cases;
            gate_cliffs[g_idx].accumulate(&g.cliff_counters);
            if !g.passed {
                for f in g.failures.iter().take(3) {
                    if failures.len() < 100 {
                        failures.push(format!(
                            "bank {b_idx} (offset {}): {f}",
                            out.summary.seed_offset
                        ));
                    }
                }
            }
        }
    }

    let pooled_max_g4_mean_dev = max_g2a4b_mean_h_reactive_dev(&all_g2a4b_raw);
    let (g2a5_benchmarks, pooled_min_margin) = aggregate_g2a5_benchmarks(&panel, &all_g2a5_raw);
    let specs: [(&str, f64); 5] = [
        ("G2A.1", 0.90),
        ("G2A.2", 0.90),
        ("G2A.3", 0.90),
        ("G2A.4", 0.85),
        ("G2A.5", 0.85),
    ];

    let mut gates = Vec::with_capacity(5);
    let mut all_passed = total_excluded == 0;
    for (idx, &(gate_name, target_rate)) in specs.iter().enumerate() {
        let stats = evaluate_multi_bank_gate(&samples_per_gate[idx], target_rate);
        let excl = gate_excl[idx];
        let g4_mean_ok = idx != 3 || pooled_max_g4_mean_dev <= 0.040;
        if idx == 3 && !g4_mean_ok && failures.len() < 100 {
            failures.push(format!(
                "G2A.4b pooled mean |E[kt] - H_reactive| = {pooled_max_g4_mean_dev:.4} > 0.040"
            ));
        }
        let margin_ok = idx != 4 || total_seeds <= 1 || pooled_min_margin >= 0.25;
        if idx == 4 && !margin_ok && failures.len() < 100 {
            failures.push(format!(
                "G2A.5 pooled min required mean KT margin = {pooled_min_margin:.4} < 0.2500"
            ));
        }
        let passed = stats.passed && excl == 0 && g4_mean_ok && margin_ok;
        if !passed {
            all_passed = false;
        }
        gates.push(M2aBankGateReport {
            gate: gate_name.to_owned(),
            stats,
            banks_above_target: banks_above_target[idx],
            total_banks: banks,
            excluded_cases: excl,
            cliff_counters: gate_cliffs[idx],
            min_required_mean_margin_bits: (idx == 4).then_some(pooled_min_margin),
            max_g2a4b_mean_h_reactive_dev_bits: (idx == 3).then_some(pooled_max_g4_mean_dev),
            passed,
        });
    }

    Ok(M2aBankReport {
        provenance: BuildProvenance::current(),
        seed_offset,
        seeds_per_bank,
        banks_evaluated: banks,
        horizon,
        gates,
        g2a5_benchmarks,
        excluded_cases: total_excluded,
        cliff_counters: total_cliff,
        failures,
        all_passed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::substrate::Primitive;

    #[test]
    fn holdout_guards_reject_m1_m2a_and_m2b_ranges() {
        // M1 holdout 10000..=10019 always refused even with allow_holdout=true
        assert!(check_m2a_seed_range_holdout(10_000, 20, false).is_err());
        assert!(check_m2a_seed_range_holdout(10_000, 20, true).is_err());
        assert!(check_m2a_seed_range_holdout(9_990, 20, true).is_err());

        // M2b holdout 40000..=40019 always refused even with allow_holdout=true
        assert!(check_m2a_seed_range_holdout(40_000, 20, false).is_err());
        assert!(check_m2a_seed_range_holdout(40_000, 20, true).is_err());
        assert!(check_m2a_seed_range_holdout(39_990, 20, true).is_err());

        // M2a holdout 30000..=30019 requires allow_holdout=true
        let err = check_m2a_seed_range_holdout(30_000, 20, false).unwrap_err();
        assert!(err.contains("30000..=30019"));
        assert!(check_m2a_seed_range_holdout(29_900, 101, false).is_err());
        assert!(check_m2a_seed_range_holdout(30_000, 20, true).is_ok());

        // Disjoint dev and confirmation ranges succeed cleanly
        assert!(check_m2a_seed_range_holdout(0, 20, false).is_ok());
        assert!(check_m2a_seed_range_holdout(20, 1_000, false).is_ok());
        assert!(check_m2a_seed_range_holdout(20_020, 20, false).is_ok());
    }

    #[test]
    fn g2a5_search_baseline_seed_ok_accepts_exact_solution_parity_only_at_100pct_match() {
        let mk = |target_kt: f64, match_rate: f64| G2a5RunMetrics {
            target_kt,
            native_prob_log_loss: None,
            target_clean_match_rate: match_rate,
            compute_ticks_consumed: 0,
            mode: BudgetAccountingMode::CandidateHistoryReplay,
        };
        // Strict >= 0.25 margin passes regardless of match rate.
        assert!(g2a5_search_baseline_seed_ok(
            &mk(0.10, 0.90),
            &mk(0.36, 0.70)
        ));
        // Smaller margin without exact 100% clean match fails.
        assert!(!g2a5_search_baseline_seed_ok(
            &mk(0.10, 0.999),
            &mk(0.10, 0.999)
        ));
        assert!(!g2a5_search_baseline_seed_ok(
            &mk(0.10, 1.0),
            &mk(0.20, 0.98)
        ));
        // Exact 100% clean-match tie within 1e-6 passes (Rec 5-a).
        assert!(g2a5_search_baseline_seed_ok(
            &mk(0.0123450, 1.0),
            &mk(0.0123455, 1.0)
        ));
        // Exceeding 1e-6 KT gap even at 100% match fails.
        assert!(!g2a5_search_baseline_seed_ok(
            &mk(0.012345, 1.0),
            &mk(0.012348, 1.0)
        ));
    }

    #[test]
    fn causal_reactive_oracle_matches_clean_t2_and_mean_dev_helper_tracks_bias() {
        let d0 = driver_small_0();
        let d1 = driver_small_1();
        let sb = SeedBank::new(0);
        for (width, cfg) in [
            (
                Width::BITS_1,
                GeneratorConfig::T2_DelayedAffine {
                    driver: d0.clone(),
                    lag: 2,
                    mul_a: 1,
                    add_b: 1,
                },
            ),
            (
                Width::BITS_2,
                GeneratorConfig::T2_CrossLinear {
                    driver_0: d0,
                    driver_1: d1,
                    lag_0: 1,
                    lag_1: 2,
                    op: CrossLinearOp::Add,
                },
            ),
        ] {
            let gen_cfg = cfg.build(width).unwrap();
            let ch = gen_cfg.channels();
            let target = usize::from(ch) - 1;
            let mut env = NoisyEnvironment::new(gen_cfg, NoiseModel::None, sb).unwrap();
            let mut oracle = CausalReactiveOracle::from_config(width, ch, &cfg);
            let sc = run_prequential_harness(
                &mut env,
                &mut oracle,
                DEFAULT_M2A_HORIZON,
                DEFAULT_M2A_WARMUP,
                DEFAULT_M2A_BUDGET,
                None,
                None,
            );
            assert_eq!(sc.per_channel[target].post_transient_clean_mismatches, 0);
        }

        let s_ok = [
            [(0.49, 0.4529), (0.60, 0.6116), (0.50, 0.50), (0.80, 0.8071)],
            [(0.42, 0.4529), (0.62, 0.6116), (0.50, 0.50), (0.81, 0.8071)],
        ];
        assert!(max_g2a4b_mean_h_reactive_dev(&s_ok) <= 0.040);
        let s_fail = [[(0.50, 0.4529), (0.61, 0.6116), (0.50, 0.50), (0.81, 0.8071)]];
        assert!(max_g2a4b_mean_h_reactive_dev(&s_fail) > 0.040);
    }

    #[test]
    fn coprime_high_degree_lfsrs_reach_maximal_periods_and_cross_cone_bfs_detects_coupling() {
        // Verify deg 11 (2047) and deg 9 (511) drivers are full period at Width::BITS_1.
        for (driver, expected_period) in [(driver_hi_0(), 2047usize), (driver_hi_1(), 511usize)] {
            let mut gen_lfsr = GeneratorConfig::T3c_Lfsr {
                taps: driver.taps,
                init: driver.init,
            }
            .build(Width::BITS_1)
            .unwrap();
            let mut first_window = Vec::with_capacity(expected_period);
            let mut out = [0u64];
            for _ in 0..expected_period {
                gen_lfsr.next_clean_frame(&mut out);
                first_window.push(out[0]);
            }
            let mut second_window = Vec::with_capacity(expected_period);
            for _ in 0..expected_period {
                gen_lfsr.next_clean_frame(&mut out);
                second_window.push(out[0]);
            }
            assert_eq!(first_window, second_window);
            assert!(first_window[..expected_period - 1].iter().any(|&b| b != 0));
        }

        // Cross-channel cone BFS test.
        let mut circ = Circuit::new(Width::BITS_2, 2, 2, WireDelayMode::Combinational);
        let eqz = circ.add_node(Primitive::Eqz);
        circ.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: eqz, port: 0 },
        );
        circ.connect(
            SignalSource::NodeOut { node: eqz, port: 0 },
            SignalSink::BoundaryOut(1),
        );
        assert!(circuit_target_cone_uses_cross_channel(&circ, 1));
        assert!(!circuit_target_cone_uses_cross_channel(&circ, 0));
    }

    #[test]
    fn fast_single_seed_m2a_suite_executes_cleanly() {
        let summary =
            run_m2a_verification_suite(0, 1, DEFAULT_M2A_HORIZON, false).expect("m2a seed 0 runs");
        assert_eq!(summary.gates.len(), 5);
        assert_eq!(summary.excluded_cases, 0);
        for g in &summary.gates {
            assert!(
                g.passed,
                "gate {} failed on seed 0: {:?}",
                g.gate, g.failures
            );
        }
        assert!(summary.all_passed);
    }
}
