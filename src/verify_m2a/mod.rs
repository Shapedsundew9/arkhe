//! Milestone 2a cross-channel coupling acceptance verification suite (`G2A.1`–`G2A.5`).

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::{Deserialize, Serialize};

use crate::baselines::{
    CgpBaseline, CrossChannelCtwBaseline, CtwBaseline, PredictorBaseline, RandomSearchBaseline,
    ReedsSloaneBaseline, RnnBaseline, TickBudget, run_prequential_harness,
};
use crate::config::{BuildProvenance, CliffCountersSummary, compute_ground_truth_yardstick};
use crate::env::{
    CrossLinearOp, CrossNonlinearOp, EpsilonMachineSummary, FiniteStateGenerator, FiniteStateSpec,
    GeneratorConfig, LfsrStage, NoiseModel, NoisyEnvironment, SpecExportError, TierGenerator,
    compound_reactive_entropy_bits,
};
use crate::metrics::{BudgetAccountingMode, Scorecard};
use crate::rng::SeedBank;
use crate::stats::{
    BankSample, BankTailMetricMap, BankTaskCountMap, CaseOutcome, GateRegime, HOLDOUT_BANKS,
    MultiBankGateResult, OnlineCaseSummary, StratifiedGateAudit, check_seed_range_holdouts,
    classify_case_outcome, evaluate_multi_bank_gate, evaluate_stratified_from_bank_maps,
    update_max_finite, update_min_finite,
};
use crate::structural::{ArkheLearner, EconomyConfig, GrowthConfig};
use crate::substrate::{Circuit, SignalSink, SignalSource, Width, WireDelayMode};

/// Default Milestone 2a simulation horizon $T$ in ticks (§4 / §6).
pub const DEFAULT_M2A_HORIZON: u64 = 2_000;
/// Default Milestone 2a post-transient evaluation warmup in ticks (§4 / §6).
pub const DEFAULT_M2A_WARMUP: u64 = 500;
/// Default Milestone 2a per-run candidate-replay / compute budget (§6).
pub const DEFAULT_M2A_BUDGET: u64 = 200_000;

/// Checks whether `[seed_offset, seed_offset + seeds)` overlaps any reserved or closed holdout range.
///
/// Unconditionally refuses `10000..=10019` (M1), `30000..=30019` (closed M2a), `40000..=40019`
/// (closed M2b), `50000..=50019` (closed M3 v1), `50100..=50199` (M3 v2), and `60000..=60099`
/// (M4a), even when `allow_holdout == true`.
///
/// # Errors
///
/// Returns an explanatory error string whenever a guarded holdout range overlaps.
pub fn check_m2a_seed_range_holdout(
    seed_offset: u32,
    seeds: u32,
    allow_holdout: bool,
) -> Result<(), String> {
    check_seed_range_holdouts(
        "verify-m2a",
        u64::from(seed_offset),
        u64::from(seeds.max(1)),
        allow_holdout,
        HOLDOUT_BANKS,
    )
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
    /// Statistical Policy Option A + D regime-stratified & per-task tail audit.
    pub stratified: StratifiedGateAudit,
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
    /// Whether `stats.passed`, `stratified.passed`, `excluded_cases == 0`, `G2A.4b` mean deviation, and `G2A.5` margin passed.
    pub passed: bool,
    /// Statistical Policy Option A + D regime-stratified & per-task tail audit.
    pub stratified: StratifiedGateAudit,
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

#[derive(Clone, Debug, Default)]
struct TaskOutcomePiece {
    ok: bool,
    excluded: bool,
    cliff: CliffCountersSummary,
    failure: Option<String>,
    target_kt: f64,
}

struct G2a1TaskSpec {
    label: &'static str,
    width: Width,
    gen_cfg: GeneratorConfig,
}

fn g2a1_task_specs() -> Vec<G2a1TaskSpec> {
    let d0 = driver_small_0();
    let d1 = driver_small_1();
    vec![
        G2a1TaskSpec {
            label: "DelayedAffine(d=1,k=1)",
            width: Width::BITS_1,
            gen_cfg: GeneratorConfig::T2_DelayedAffine {
                driver: d0.clone(),
                lag: 1,
                mul_a: 1,
                add_b: 1,
            },
        },
        G2a1TaskSpec {
            label: "DelayedAffine(d=1,k=2)",
            width: Width::BITS_2,
            gen_cfg: GeneratorConfig::T2_DelayedAffine {
                driver: d0.clone(),
                lag: 1,
                mul_a: 1,
                add_b: 1,
            },
        },
        G2a1TaskSpec {
            label: "DelayedAffine(d=2,k=1)",
            width: Width::BITS_1,
            gen_cfg: GeneratorConfig::T2_DelayedAffine {
                driver: d0.clone(),
                lag: 2,
                mul_a: 1,
                add_b: 1,
            },
        },
        G2a1TaskSpec {
            label: "DelayedAffine(d=2,3x+2,k=2)",
            width: Width::BITS_2,
            gen_cfg: GeneratorConfig::T2_DelayedAffine {
                driver: d0.clone(),
                lag: 2,
                mul_a: 3,
                add_b: 2,
            },
        },
        G2a1TaskSpec {
            label: "DelayedAffine(d=4,k=1)",
            width: Width::BITS_1,
            gen_cfg: GeneratorConfig::T2_DelayedAffine {
                driver: d0.clone(),
                lag: 4,
                mul_a: 1,
                add_b: 1,
            },
        },
        G2a1TaskSpec {
            label: "DelayedAffine(d=4,k=2)",
            width: Width::BITS_2,
            gen_cfg: GeneratorConfig::T2_DelayedAffine {
                driver: d0.clone(),
                lag: 4,
                mul_a: 1,
                add_b: 1,
            },
        },
        G2a1TaskSpec {
            label: "CrossAdd(d=1,2,k=1)",
            width: Width::BITS_1,
            gen_cfg: GeneratorConfig::T2_CrossLinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossLinearOp::Add,
            },
        },
        G2a1TaskSpec {
            label: "CrossAdd(d=1,2,k=2)",
            width: Width::BITS_2,
            gen_cfg: GeneratorConfig::T2_CrossLinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossLinearOp::Add,
            },
        },
        G2a1TaskSpec {
            label: "CrossSub(d=1,2,k=2)",
            width: Width::BITS_2,
            gen_cfg: GeneratorConfig::T2_CrossLinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossLinearOp::Sub,
            },
        },
        G2a1TaskSpec {
            label: "CrossMul(d=1,2,k=1)",
            width: Width::BITS_1,
            gen_cfg: GeneratorConfig::T2_CrossNonlinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossNonlinearOp::Mul,
            },
        },
        G2a1TaskSpec {
            label: "CrossMul(d=1,2,k=2)",
            width: Width::BITS_2,
            gen_cfg: GeneratorConfig::T2_CrossNonlinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossNonlinearOp::Mul,
            },
        },
        G2a1TaskSpec {
            label: "CrossMulEqz(d=1,2,k=1)",
            width: Width::BITS_1,
            gen_cfg: GeneratorConfig::T2_CrossNonlinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossNonlinearOp::MulEqz,
            },
        },
        G2a1TaskSpec {
            label: "CrossMulEqz(d=1,2,k=2)",
            width: Width::BITS_2,
            gen_cfg: GeneratorConfig::T2_CrossNonlinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossNonlinearOp::MulEqz,
            },
        },
        G2a1TaskSpec {
            label: "CrossAddEqz(d=1,2,k=1)",
            width: Width::BITS_1,
            gen_cfg: GeneratorConfig::T2_CrossNonlinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossNonlinearOp::AddEqz,
            },
        },
        G2a1TaskSpec {
            label: "CrossAddEqz(d=1,2,k=2)",
            width: Width::BITS_2,
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

fn run_g2a1_task(spec: &G2a1TaskSpec, master_seed: u64, horizon: u64) -> (TaskOutcomePiece, usize) {
    let seeds_bank = SeedBank::new(master_seed);
    let mut cliff = CliffCountersSummary::default();
    let mut excluded_count = 0usize;

    let generator = spec.gen_cfg.build(spec.width).expect("valid T2 generator");
    let channels = generator.channels();
    let target = usize::from(channels) - 1;
    let mut env = NoisyEnvironment::new(generator, NoiseModel::None, seeds_bank).unwrap();
    let coupled = run_learner_on_env(
        &mut env,
        spec.width,
        channels,
        WireDelayMode::Combinational,
        horizon,
        seeds_bank,
        None,
    );
    cliff.accumulate(&coupled.cliff);
    if coupled.excluded {
        excluded_count += 1;
    }
    let target_sc = &coupled.scorecard.per_channel[target];
    let cross_wired =
        circuit_target_cone_uses_cross_channel(coupled.learner.editable().circuit(), target as u16);
    let coupled_ok = !coupled.excluded
        && target_sc.post_transient_clean_mismatches == 0
        && target_sc.excess_residual_bits_over_h_mu <= 0.005
        && cross_wired;

    // Marginal K=1 target-only control must fail to lock (clean_err >= 0.10).
    let marg_gen = MarginalTargetGenerator::new(
        spec.gen_cfg.build(spec.width).expect("valid T2 gen"),
        target,
    );
    let mut marg_env = NoisyEnvironment::new(marg_gen, NoiseModel::None, seeds_bank).unwrap();
    let marg = run_learner_on_env(
        &mut marg_env,
        spec.width,
        1,
        WireDelayMode::Combinational,
        horizon,
        seeds_bank,
        None,
    );
    cliff.accumulate(&marg.cliff);
    if marg.excluded {
        excluded_count += 1;
    }
    let marg_err = (marg.scorecard.per_channel[0].post_transient_clean_mismatches as f64)
        / (marg.scorecard.evaluated_ticks.max(1) as f64);
    let marginal_ok = !marg.excluded && marg_err >= 0.10;

    let ok = coupled_ok && marginal_ok;
    let failure = (!ok).then(|| {
        format!(
            "G2A.1 {} seed={master_seed}: clean_mis={} excess={:.4} cross_wired={cross_wired} marg_err={marg_err:.4} excl={}/{}",
            spec.label,
            target_sc.post_transient_clean_mismatches,
            target_sc.excess_residual_bits_over_h_mu,
            coupled.excluded,
            marg.excluded
        )
    });
    (
        TaskOutcomePiece {
            ok,
            excluded: excluded_count > 0,
            cliff,
            failure,
            target_kt: target_sc.prequential_kt_residual_bits_per_symbol,
        },
        excluded_count,
    )
}

struct G2a2DeepSpec {
    width: Width,
    lag: usize,
    gen_cfg: GeneratorConfig,
}

struct G2a2TphSpec {
    label: &'static str,
    width: Width,
    gen_cfg: GeneratorConfig,
}

fn g2a2_deep_specs() -> Vec<G2a2DeepSpec> {
    let d_hi = driver_hi_0();
    let mut out = Vec::with_capacity(6);
    for width in [Width::BITS_1, Width::BITS_2] {
        for lag in [5usize, 6, 8] {
            out.push(G2a2DeepSpec {
                width,
                lag,
                gen_cfg: GeneratorConfig::T2_DelayedAffine {
                    driver: d_hi.clone(),
                    lag,
                    mul_a: 1,
                    add_b: 1,
                },
            });
        }
    }
    out
}

fn g2a2_tph_specs() -> Vec<G2a2TphSpec> {
    let d0 = driver_small_0();
    let d1 = driver_small_1();
    vec![
        G2a2TphSpec {
            label: "TphDelayedAffine(d=3,k=1)",
            width: Width::BITS_1,
            gen_cfg: GeneratorConfig::T2_DelayedAffine {
                driver: d0.clone(),
                lag: 3,
                mul_a: 1,
                add_b: 1,
            },
        },
        G2a2TphSpec {
            label: "TphDelayedAffine(d=3,k=2)",
            width: Width::BITS_2,
            gen_cfg: GeneratorConfig::T2_DelayedAffine {
                driver: d0.clone(),
                lag: 3,
                mul_a: 3,
                add_b: 1,
            },
        },
        G2a2TphSpec {
            label: "TphCrossSum(d=3,3,k=2)",
            width: Width::BITS_2,
            gen_cfg: GeneratorConfig::T2_CrossLinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 3,
                lag_1: 3,
                op: CrossLinearOp::Add,
            },
        },
        G2a2TphSpec {
            label: "TphCrossSum(d=3,5,k=2)",
            width: Width::BITS_2,
            gen_cfg: GeneratorConfig::T2_CrossLinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 3,
                lag_1: 5,
                op: CrossLinearOp::Add,
            },
        },
        G2a2TphSpec {
            label: "TphAddEqz(d=3,4,k=2)",
            width: Width::BITS_2,
            gen_cfg: GeneratorConfig::T2_CrossNonlinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 3,
                lag_1: 4,
                op: CrossNonlinearOp::AddEqz,
            },
        },
        G2a2TphSpec {
            label: "TphMulEqzPair(d=4,4,k=1)",
            width: Width::BITS_1,
            gen_cfg: GeneratorConfig::T2_CrossNonlinear {
                driver_0: d0,
                driver_1: d1,
                lag_0: 4,
                lag_1: 4,
                op: CrossNonlinearOp::MulEqzPair,
            },
        },
    ]
}

fn run_g2a2_deep_task(spec: &G2a2DeepSpec, master_seed: u64, horizon: u64) -> TaskOutcomePiece {
    let seeds_bank = SeedBank::new(master_seed);
    let generator = spec
        .gen_cfg
        .build(spec.width)
        .expect("valid deep T2 generator");
    let mut env = NoisyEnvironment::new(generator, NoiseModel::None, seeds_bank).unwrap();
    let res = run_learner_on_env(
        &mut env,
        spec.width,
        2,
        WireDelayMode::Combinational,
        horizon,
        seeds_bank,
        None,
    );
    let target_sc = &res.scorecard.per_channel[1];
    let ok = !res.excluded
        && target_sc.post_transient_clean_mismatches == 0
        && res.cliff.lag_deepening_steps >= 1;
    let failure = (!ok).then(|| {
        format!(
            "G2A.2a DeepAffine(d={},k={}) seed={master_seed}: clean_mis={} deep_steps={} excl={}",
            spec.lag,
            spec.width.bits(),
            target_sc.post_transient_clean_mismatches,
            res.cliff.lag_deepening_steps,
            res.excluded
        )
    });
    TaskOutcomePiece {
        ok,
        excluded: res.excluded,
        cliff: res.cliff,
        failure,
        target_kt: target_sc.prequential_kt_residual_bits_per_symbol,
    }
}

fn run_g2a2_m1_ring_task(master_seed: u64, horizon: u64) -> TaskOutcomePiece {
    let seeds_bank = SeedBank::new(master_seed);
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
    let ok = !res.excluded
        && res.scorecard.per_channel[0].post_transient_clean_mismatches == 0
        && res.learner.ring_closure_events() >= 1
        && res.cliff.lag_deepening_steps == 0;
    let failure = (!ok).then(|| {
        format!(
            "G2A.2a M1-RingInvariance seed={master_seed}: clean_mis={} rings={} deep_steps={}",
            res.scorecard.per_channel[0].post_transient_clean_mismatches,
            res.learner.ring_closure_events(),
            res.cliff.lag_deepening_steps
        )
    });
    TaskOutcomePiece {
        ok,
        excluded: res.excluded,
        cliff: res.cliff,
        failure,
        target_kt: res.scorecard.per_channel[0].prequential_kt_residual_bits_per_symbol,
    }
}

fn run_g2a2_tph_task(spec: &G2a2TphSpec, master_seed: u64, horizon: u64) -> TaskOutcomePiece {
    let seeds_bank = SeedBank::new(master_seed);
    let generator = spec.gen_cfg.build(spec.width).expect("valid Tph generator");
    let channels = generator.channels();
    let target = usize::from(channels) - 1;
    let mut env = NoisyEnvironment::new(generator, NoiseModel::None, seeds_bank).unwrap();
    let res = run_learner_on_env(
        &mut env,
        spec.width,
        channels,
        WireDelayMode::TicksPerHop,
        horizon,
        seeds_bank,
        None,
    );
    let edges_unit_hop = res
        .learner
        .editable()
        .circuit()
        .edges()
        .iter()
        .all(|e| e.hops == 1);
    let target_sc = &res.scorecard.per_channel[target];
    let ok = !res.excluded && target_sc.post_transient_clean_mismatches == 0 && edges_unit_hop;
    let failure = (!ok).then(|| {
        format!(
            "G2A.2b {} seed={master_seed}: clean_mis={} edges_unit_hop={edges_unit_hop} excl={}",
            spec.label, target_sc.post_transient_clean_mismatches, res.excluded
        )
    });
    TaskOutcomePiece {
        ok,
        excluded: res.excluded,
        cliff: res.cliff,
        failure,
        target_kt: target_sc.prequential_kt_residual_bits_per_symbol,
    }
}

struct G2a3SoloSpec {
    panel_label: &'static str,
    width: Width,
    wire_delay: WireDelayMode,
    motif: Vec<u64>,
}

struct G2a3MultiSpec {
    panel_label: &'static str,
    width: Width,
    wire_delay: WireDelayMode,
    motifs: Vec<Vec<u64>>,
    solo_indices: Vec<usize>,
}

fn g2a3_specs() -> (Vec<G2a3SoloSpec>, Vec<G2a3MultiSpec>) {
    let panels: [(&'static str, Width, Vec<Vec<u64>>); 6] = [
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

    let mut solo_map: BTreeMap<(u8, u8, Vec<u64>), usize> = BTreeMap::new();
    let mut solos = Vec::new();
    let mut multis = Vec::with_capacity(panels.len() * 2);

    for wire_delay in [WireDelayMode::Combinational, WireDelayMode::TicksPerHop] {
        let wd_tag = match wire_delay {
            WireDelayMode::Combinational => 0u8,
            WireDelayMode::TicksPerHop => 1u8,
        };
        for (panel_label, width, motifs) in &panels {
            let mut solo_indices = Vec::with_capacity(motifs.len());
            for motif in motifs {
                let key = (width.bits(), wd_tag, motif.clone());
                let idx = if let Some(&existing) = solo_map.get(&key) {
                    existing
                } else {
                    let next_idx = solos.len();
                    solos.push(G2a3SoloSpec {
                        panel_label,
                        width: *width,
                        wire_delay,
                        motif: motif.clone(),
                    });
                    solo_map.insert(key, next_idx);
                    next_idx
                };
                solo_indices.push(idx);
            }
            multis.push(G2a3MultiSpec {
                panel_label,
                width: *width,
                wire_delay,
                motifs: motifs.clone(),
                solo_indices,
            });
        }
    }

    (solos, multis)
}

#[derive(Default)]
struct G2a3SoloOutcome {
    cliff: CliffCountersSummary,
    excluded: bool,
    t_lock: Option<u64>,
}

#[derive(Default)]
struct G2a3MultiOutcome {
    cliff: CliffCountersSummary,
    excluded: bool,
    horizon: u64,
    channels: Vec<(Option<u64>, u64)>,
}

fn run_g2a3_solo_task(spec: &G2a3SoloSpec, master_seed: u64, horizon: u64) -> G2a3SoloOutcome {
    let seeds_bank = SeedBank::new(master_seed);
    let s_gen = MultiChannelPeriodicGenerator::new(
        format!("G2A3Solo:{}", spec.panel_label),
        spec.width,
        vec![spec.motif.clone()],
    );
    let mut s_env = NoisyEnvironment::new(s_gen, NoiseModel::None, seeds_bank).unwrap();
    let s_res = run_learner_on_env(
        &mut s_env,
        spec.width,
        1,
        spec.wire_delay,
        horizon,
        seeds_bank,
        None,
    );
    let t_lock =
        if s_res.excluded || s_res.scorecard.per_channel[0].post_transient_clean_mismatches != 0 {
            None
        } else {
            s_res.scorecard.per_channel[0].ticks_to_clean_solution
        };
    G2a3SoloOutcome {
        cliff: s_res.cliff,
        excluded: s_res.excluded,
        t_lock,
    }
}

fn run_g2a3_multi_task(spec: &G2a3MultiSpec, master_seed: u64, horizon: u64) -> G2a3MultiOutcome {
    let seeds_bank = SeedBank::new(master_seed);
    let m_gen = MultiChannelPeriodicGenerator::new(
        format!("G2A3Multi:{}", spec.panel_label),
        spec.width,
        spec.motifs.clone(),
    );
    let k_ports = m_gen.channels();
    let mut m_env = NoisyEnvironment::new(m_gen, NoiseModel::None, seeds_bank).unwrap();
    let m_res = run_learner_on_env(
        &mut m_env,
        spec.width,
        k_ports,
        spec.wire_delay,
        horizon,
        seeds_bank,
        None,
    );
    let channels = m_res
        .scorecard
        .per_channel
        .iter()
        .map(|sc| {
            (
                sc.ticks_to_clean_solution,
                sc.post_transient_clean_mismatches,
            )
        })
        .collect();
    G2a3MultiOutcome {
        cliff: m_res.cliff,
        excluded: m_res.excluded,
        horizon,
        channels,
    }
}

fn reduce_g2a3_seed(
    master_seed: u64,
    solos: &[G2a3SoloOutcome],
    multis: &[G2a3MultiOutcome],
    multi_specs: &[G2a3MultiSpec],
) -> (SeedOutcome, Vec<M2aTaskRecord>) {
    let w_diag = GrowthConfig::default().diagnostic_window as f64;
    let mut seed_cliff = CliffCountersSummary::default();
    let mut excluded_cases = 0usize;
    let mut failures = Vec::new();
    let mut all_ok = true;
    let mut task_records = Vec::with_capacity(multi_specs.len());

    let mut solo_seen = vec![false; solos.len()];
    for (m_out, m_spec) in multis.iter().zip(multi_specs) {
        let mut solo_excl = false;
        for &solo_idx in &m_spec.solo_indices {
            let s = &solos[solo_idx];
            if s.excluded {
                solo_excl = true;
            }
            if !solo_seen[solo_idx] {
                solo_seen[solo_idx] = true;
                seed_cliff.accumulate(&s.cliff);
                if s.excluded {
                    excluded_cases += 1;
                    all_ok = false;
                }
            }
        }
        seed_cliff.accumulate(&m_out.cliff);
        if m_out.excluded {
            excluded_cases += 1;
            all_ok = false;
        }
        let mut task_ok = !m_out.excluded && !solo_excl;
        let mut max_multi_lock_tick = 0.0f64;
        let mut max_lock_time_ratio: Option<f64> = None;
        for (ch, &solo_idx) in m_spec.solo_indices.iter().enumerate() {
            let solo_t = solos[solo_idx].t_lock;
            let (tm_opt, clean_mis) = m_out.channels[ch];
            let multi_tick = tm_opt.unwrap_or(m_out.horizon) as f64;
            max_multi_lock_tick = max_multi_lock_tick.max(multi_tick);
            if let (Some(ts), Some(tm)) = (solo_t, tm_opt) {
                let ratio = (tm as f64) / (ts as f64).max(w_diag);
                if ratio.is_finite() {
                    max_lock_time_ratio = Some(max_lock_time_ratio.map_or(ratio, |m| m.max(ratio)));
                }
            }
            let ch_ok = match (solo_t, tm_opt) {
                (Some(ts), Some(tm)) if !m_out.excluded => {
                    clean_mis == 0 && (tm as f64) <= 1.50 * (ts as f64).max(w_diag)
                }
                _ => false,
            };
            if !ch_ok {
                all_ok = false;
                task_ok = false;
                failures.push(format!(
                    "G2A.3 {} {:?} ch={ch} seed={master_seed}: \
                     solo={solo_t:?} multi={tm_opt:?} clean_mis={clean_mis}",
                    m_spec.panel_label, m_spec.wire_delay
                ));
            }
        }
        let mut tail_metrics = vec![("max_multi_lock_tick", max_multi_lock_tick)];
        if let Some(ratio) = max_lock_time_ratio {
            tail_metrics.push(("max_lock_time_ratio", ratio));
        }
        task_records.push(M2aTaskRecord {
            gate_idx: 2,
            task_label: format!("G2A.3:{}({:?})", m_spec.panel_label, m_spec.wire_delay),
            regime: GateRegime::Clean,
            passed: task_ok,
            tail_metrics,
        });
    }

    (
        SeedOutcome {
            passed: all_ok && excluded_cases == 0,
            cliff: seed_cliff,
            excluded_cases,
            failures,
        },
        task_records,
    )
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

const G2A4_EPSILON: f64 = 0.05;

struct G2a4aTaskSpec {
    label: &'static str,
    width: Width,
    gen_cfg: GeneratorConfig,
    epsilon_machine: Option<EpsilonMachineSummary>,
}

struct G2a4bTaskSpec {
    label: &'static str,
    width: Width,
    m_inputs: usize,
    gen_cfg: GeneratorConfig,
}

fn g2a4_base_specs() -> (
    Vec<(&'static str, Width, GeneratorConfig)>,
    Vec<G2a4bTaskSpec>,
) {
    let d0 = driver_small_0();
    let d1 = driver_small_1();
    let a = vec![
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
    let b = vec![
        G2a4bTaskSpec {
            label: "AllNoise:DelayedAffine(d=2,k=1)",
            width: Width::BITS_1,
            m_inputs: 1,
            gen_cfg: GeneratorConfig::T2_DelayedAffine {
                driver: d0.clone(),
                lag: 2,
                mul_a: 1,
                add_b: 1,
            },
        },
        G2a4bTaskSpec {
            label: "AllNoise:DelayedAffine(d=2,k=2)",
            width: Width::BITS_2,
            m_inputs: 1,
            gen_cfg: GeneratorConfig::T2_DelayedAffine {
                driver: d0.clone(),
                lag: 2,
                mul_a: 1,
                add_b: 1,
            },
        },
        G2a4bTaskSpec {
            label: "AllNoise:CrossAdd(d=1,2,k=1)",
            width: Width::BITS_1,
            m_inputs: 2,
            gen_cfg: GeneratorConfig::T2_CrossLinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossLinearOp::Add,
            },
        },
        G2a4bTaskSpec {
            label: "AllNoise:CrossAdd(d=1,2,k=2)",
            width: Width::BITS_2,
            m_inputs: 2,
            gen_cfg: GeneratorConfig::T2_CrossLinear {
                driver_0: d0,
                driver_1: d1,
                lag_0: 1,
                lag_1: 2,
                op: CrossLinearOp::Add,
            },
        },
    ];
    (a, b)
}

fn build_g2a4a_machine(width: Width, gen_cfg: &GeneratorConfig) -> Option<EpsilonMachineSummary> {
    let generator = gen_cfg.build(width).unwrap();
    let target = usize::from(generator.channels()) - 1;
    let noise = NoiseModel::IndependentSubstitution {
        epsilon: G2A4_EPSILON,
        channels: vec![target as u16],
    };
    compute_ground_truth_yardstick(
        gen_cfg,
        width,
        WireDelayMode::Combinational,
        &noise,
        512,
        None,
    )
    .expect("ground truth yardstick succeeds on small coprime LFSRs")
    .epsilon_machine
}

fn run_g2a4a_task(
    spec: &G2a4aTaskSpec,
    master_seed: u64,
    horizon: u64,
) -> (TaskOutcomePiece, Option<f64>) {
    let seeds_bank = SeedBank::new(master_seed);
    let generator = spec.gen_cfg.build(spec.width).unwrap();
    let channels = generator.channels();
    let target = usize::from(channels) - 1;
    let noise = NoiseModel::IndependentSubstitution {
        epsilon: G2A4_EPSILON,
        channels: vec![target as u16],
    };
    let mut env = NoisyEnvironment::new(generator, noise, seeds_bank).unwrap();
    let res = run_learner_on_env(
        &mut env,
        spec.width,
        channels,
        WireDelayMode::Combinational,
        horizon,
        seeds_bank,
        spec.epsilon_machine.as_ref(),
    );
    let target_sc = &res.scorecard.per_channel[target];
    let regret = target_sc.paired_residual_regret_bits_per_symbol;
    let ok = !res.excluded && regret.is_some_and(|r| r <= 0.030);
    let failure = (!ok).then(|| {
        format!(
            "G2A.4a {} seed={master_seed}: paired_regret={regret:?} kt={:.4} excl={}",
            spec.label, target_sc.prequential_kt_residual_bits_per_symbol, res.excluded
        )
    });
    (
        TaskOutcomePiece {
            ok,
            excluded: res.excluded,
            cliff: res.cliff,
            failure,
            target_kt: target_sc.prequential_kt_residual_bits_per_symbol,
        },
        regret,
    )
}

fn run_g2a4b_task(
    spec: &G2a4bTaskSpec,
    master_seed: u64,
    horizon: u64,
) -> (TaskOutcomePiece, (f64, f64), f64) {
    let seeds_bank = SeedBank::new(master_seed);
    let generator = spec.gen_cfg.build(spec.width).unwrap();
    let channels = generator.channels();
    let target = usize::from(channels) - 1;
    let noise = NoiseModel::IndependentSubstitution {
        epsilon: G2A4_EPSILON,
        channels: (0..channels).collect(),
    };
    let mut oracle_env = NoisyEnvironment::new(
        spec.gen_cfg.build(spec.width).unwrap(),
        noise.clone(),
        seeds_bank,
    )
    .unwrap();
    let mut oracle = CausalReactiveOracle::from_config(spec.width, channels, &spec.gen_cfg);
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
        spec.width,
        channels,
        WireDelayMode::Combinational,
        horizon,
        seeds_bank,
        None,
    );
    let target_sc = &res.scorecard.per_channel[target];
    let kt = target_sc.prequential_kt_residual_bits_per_symbol;
    let h_react = compound_reactive_entropy_bits(G2A4_EPSILON, spec.m_inputs, spec.width.bits());
    let paired_diff = (kt - oracle_kt).abs();
    let ok = !res.excluded && paired_diff <= 0.040;
    let failure = (!ok).then(|| {
        format!(
            "G2A.4b {} seed={master_seed}: kt={kt:.4} oracle_kt={oracle_kt:.4} |paired_diff|={paired_diff:.4} H_reactive={h_react:.4} excl={}",
            spec.label, res.excluded
        )
    });
    (
        TaskOutcomePiece {
            ok,
            excluded: res.excluded,
            cliff: res.cliff,
            failure,
            target_kt: kt,
        },
        (kt, h_react),
        paired_diff,
    )
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

/// Per-seed G2A.5 pass condition against online search baselines (`RandomSearch`, `CGP`):
/// either Arkhe achieves >= 0.25 b/sym KT residual advantage, or both predictors attain
/// exact `100%` post-transient clean match rate within floating-point parity (`<= 1e-6` b/sym).
fn g2a5_search_baseline_seed_ok(arkhe_m: &G2a5RunMetrics, baseline_m: &G2a5RunMetrics) -> bool {
    (baseline_m.target_kt - arkhe_m.target_kt) >= 0.25
        || (arkhe_m.target_clean_match_rate >= 1.0 - 1e-12
            && baseline_m.target_clean_match_rate >= 1.0 - 1e-12
            && (arkhe_m.target_kt - baseline_m.target_kt).abs() <= 1e-6)
}

fn run_g2a5_task(
    spec: &G2a5TaskSpec,
    det_idx: usize,
    det_cache: &DeterministicG2a5Cache,
    master_seed: u64,
    horizon: u64,
) -> (TaskOutcomePiece, [G2a5RunMetrics; 7]) {
    let seeds_bank = SeedBank::new(master_seed);
    let mut cliff = CliffCountersSummary::default();

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
    let ctw_m = det_cache.ctw[det_idx].clone();
    let bm_m = det_cache.reeds_sloane[det_idx].clone();
    let cross_ctw_m = det_cache.cross_ctw[det_idx].clone();
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
    let failure = (!task_ok).then(|| {
        format!(
            "G2A.5 {} seed={master_seed}: arkhe={:.4} rs={:.4} cgp={:.4} ctw={:.4} bm={:.4}",
            spec.label,
            arkhe_m.target_kt,
            rs_m.target_kt,
            cgp_m.target_kt,
            ctw_m.target_kt,
            bm_m.target_kt
        )
    });

    (
        TaskOutcomePiece {
            ok: task_ok,
            excluded: arkhe.excluded,
            cliff,
            failure,
            target_kt: arkhe_m.target_kt,
        },
        [arkhe_m, rs_m, cgp_m, ctw_m, bm_m, cross_ctw_m, rnn_m],
    )
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

struct M2aTaskRecord {
    gate_idx: usize,
    task_label: String,
    regime: GateRegime,
    passed: bool,
    tail_metrics: Vec<(&'static str, f64)>,
}

struct EvaluatedSingleBank {
    summary: M2aVerificationSummary,
    raw_g2a4b_metrics: Vec<[(f64, f64); 4]>,
    raw_g2a5_metrics: Vec<Vec<[G2a5RunMetrics; 7]>>,
    gate_task_counts: [BankTaskCountMap; 5],
    gate_task_tails: [BankTailMetricMap; 5],
}

const M2A_GATE_SPECS: [(&str, f64); 5] = [
    ("G2A.1", 0.90),
    ("G2A.2", 0.90),
    ("G2A.3", 0.90),
    ("G2A.4", 0.85),
    ("G2A.5", 0.85),
];

struct AllGatesSeedOutcome {
    gates: [SeedOutcome; 5],
    g4b_samples: [(f64, f64); 4],
    g5_metrics: Vec<[G2a5RunMetrics; 7]>,
    task_records: Vec<M2aTaskRecord>,
}

/// Shared task specs plus deterministic yardstick/baseline caches built in parallel once per run.
struct M2aSharedSpecs {
    g1: Vec<G2a1TaskSpec>,
    g2_deep: Vec<G2a2DeepSpec>,
    g2_tph: Vec<G2a2TphSpec>,
    g3_solos: Vec<G2a3SoloSpec>,
    g3_multis: Vec<G2a3MultiSpec>,
    g4a: Vec<G2a4aTaskSpec>,
    g4b: Vec<G2a4bTaskSpec>,
    g5_panel: Vec<G2a5TaskSpec>,
    det_g5: DeterministicG2a5Cache,
}

enum DeterministicInitOut {
    G4a(usize, Option<EpsilonMachineSummary>),
    Ctw(usize, G2a5RunMetrics),
    Bm(usize, G2a5RunMetrics),
    CrossCtw(usize, G2a5RunMetrics),
}

impl M2aSharedSpecs {
    fn build(horizon: u64) -> Self {
        let g1 = g2a1_task_specs();
        let g2_deep = g2a2_deep_specs();
        let g2_tph = g2a2_tph_specs();
        let (g3_solos, g3_multis) = g2a3_specs();
        let (g4a_bases, g4b) = g2a4_base_specs();
        let g5_panel = g2a5_panel_specs();

        let n4a = g4a_bases.len();
        let n5 = g5_panel.len();
        let total_jobs = n4a + n5 * 3;
        let next_job = AtomicUsize::new(0);
        let num_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .min(total_jobs)
            .max(1);
        let zero_seed = SeedBank::new(0);

        let run_job = |job_idx: usize| -> DeterministicInitOut {
            if job_idx < n4a {
                let (_, width, ref gen_cfg) = g4a_bases[job_idx];
                DeterministicInitOut::G4a(job_idx, build_g2a4a_machine(width, gen_cfg))
            } else {
                let rem = job_idx - n4a;
                let task_idx = rem / 3;
                let kind = rem % 3;
                let spec = &g5_panel[task_idx];
                let channels = spec.gen_cfg.build(spec.width).unwrap().channels();
                match kind {
                    0 => DeterministicInitOut::Ctw(
                        task_idx,
                        run_baseline_on_g2a5(
                            spec,
                            Box::new(CtwBaseline::new(spec.width, channels, 9)),
                            horizon,
                            zero_seed,
                        ),
                    ),
                    1 => DeterministicInitOut::Bm(
                        task_idx,
                        run_baseline_on_g2a5(
                            spec,
                            Box::new(ReedsSloaneBaseline::new(spec.width, channels)),
                            horizon,
                            zero_seed,
                        ),
                    ),
                    _ => DeterministicInitOut::CrossCtw(
                        task_idx,
                        run_baseline_on_g2a5(
                            spec,
                            Box::new(CrossChannelCtwBaseline::with_default_depth(
                                spec.width, channels,
                            )),
                            horizon,
                            zero_seed,
                        ),
                    ),
                }
            }
        };

        let mut outputs = Vec::with_capacity(total_jobs);
        if num_threads <= 1 {
            for job_idx in 0..total_jobs {
                outputs.push(run_job(job_idx));
            }
        } else {
            let mut per_thread = Vec::with_capacity(num_threads);
            std::thread::scope(|s| {
                let mut handles = Vec::with_capacity(num_threads);
                for _ in 0..num_threads {
                    handles.push(s.spawn(|| {
                        let mut local = Vec::new();
                        loop {
                            let idx = next_job.fetch_add(1, Ordering::Relaxed);
                            if idx >= total_jobs {
                                break;
                            }
                            local.push(run_job(idx));
                        }
                        local
                    }));
                }
                for handle in handles {
                    per_thread.push(handle.join().expect("deterministic cache worker panicked"));
                }
            });
            for local in per_thread {
                outputs.extend(local);
            }
        }

        let mut g4a_machines: Vec<Option<Option<EpsilonMachineSummary>>> = vec![None; n4a];
        let mut ctw = vec![None; n5];
        let mut reeds_sloane = vec![None; n5];
        let mut cross_ctw = vec![None; n5];
        for out in outputs {
            match out {
                DeterministicInitOut::G4a(idx, m) => g4a_machines[idx] = Some(m),
                DeterministicInitOut::Ctw(idx, m) => ctw[idx] = Some(m),
                DeterministicInitOut::Bm(idx, m) => reeds_sloane[idx] = Some(m),
                DeterministicInitOut::CrossCtw(idx, m) => cross_ctw[idx] = Some(m),
            }
        }

        let g4a = g4a_bases
            .into_iter()
            .zip(g4a_machines)
            .map(|((label, width, gen_cfg), machine)| G2a4aTaskSpec {
                label,
                width,
                gen_cfg,
                epsilon_machine: machine.expect("g4a machine populated"),
            })
            .collect();

        Self {
            g1,
            g2_deep,
            g2_tph,
            g3_solos,
            g3_multis,
            g4a,
            g4b,
            g5_panel,
            det_g5: DeterministicG2a5Cache {
                ctw: ctw.into_iter().map(|x| x.expect("ctw cached")).collect(),
                reeds_sloane: reeds_sloane
                    .into_iter()
                    .map(|x| x.expect("reeds_sloane cached"))
                    .collect(),
                cross_ctw: cross_ctw
                    .into_iter()
                    .map(|x| x.expect("cross_ctw cached"))
                    .collect(),
            },
        }
    }
}

#[derive(Clone, Copy)]
enum M2aTaskKind {
    G1(usize),
    G2Deep(usize),
    G2M1Ring,
    G2Tph(usize),
    G3Solo(usize),
    G3Multi(usize),
    G4a(usize),
    G4b(usize),
    G5(usize),
}

enum M2aTaskPayload {
    G1 {
        idx: usize,
        piece: TaskOutcomePiece,
        excl: usize,
    },
    G2Deep {
        idx: usize,
        piece: TaskOutcomePiece,
    },
    G2M1Ring(TaskOutcomePiece),
    G2Tph {
        idx: usize,
        piece: TaskOutcomePiece,
    },
    G3Solo {
        idx: usize,
        out: G2a3SoloOutcome,
    },
    G3Multi {
        idx: usize,
        out: G2a3MultiOutcome,
    },
    G4a {
        idx: usize,
        piece: TaskOutcomePiece,
        regret: Option<f64>,
    },
    G4b {
        idx: usize,
        piece: TaskOutcomePiece,
        sample: (f64, f64),
        paired_diff: f64,
    },
    G5 {
        idx: usize,
        piece: TaskOutcomePiece,
        metrics: Box<[G2a5RunMetrics; 7]>,
    },
}

struct M2aTaskOutput {
    seed_idx: usize,
    payload: M2aTaskPayload,
}

fn per_seed_task_schedule_lpt(specs: &M2aSharedSpecs) -> Vec<M2aTaskKind> {
    let total = specs.g5_panel.len()
        + specs.g1.len()
        + specs.g4b.len()
        + specs.g2_deep.len()
        + specs.g4a.len()
        + specs.g2_tph.len()
        + specs.g3_multis.len()
        + specs.g3_solos.len()
        + 1;
    let mut order = Vec::with_capacity(total);
    // Longest-processing-time-first ordering minimizes parallel tail latency on small seed counts.
    order.extend((0..specs.g5_panel.len()).map(M2aTaskKind::G5));
    order.extend((0..specs.g1.len()).map(M2aTaskKind::G1));
    order.extend((0..specs.g4b.len()).map(M2aTaskKind::G4b));
    order.extend((0..specs.g2_deep.len()).map(M2aTaskKind::G2Deep));
    order.extend((0..specs.g4a.len()).map(M2aTaskKind::G4a));
    order.extend((0..specs.g2_tph.len()).map(M2aTaskKind::G2Tph));
    order.extend((0..specs.g3_multis.len()).map(M2aTaskKind::G3Multi));
    order.extend((0..specs.g3_solos.len()).map(M2aTaskKind::G3Solo));
    order.push(M2aTaskKind::G2M1Ring);
    order
}

fn execute_m2a_task(
    specs: &M2aSharedSpecs,
    seed_idx: usize,
    kind: M2aTaskKind,
    master_seed: u64,
    horizon: u64,
) -> M2aTaskOutput {
    let payload = match kind {
        M2aTaskKind::G1(idx) => {
            let (piece, excl) = run_g2a1_task(&specs.g1[idx], master_seed, horizon);
            M2aTaskPayload::G1 { idx, piece, excl }
        }
        M2aTaskKind::G2Deep(idx) => M2aTaskPayload::G2Deep {
            idx,
            piece: run_g2a2_deep_task(&specs.g2_deep[idx], master_seed, horizon),
        },
        M2aTaskKind::G2M1Ring => {
            M2aTaskPayload::G2M1Ring(run_g2a2_m1_ring_task(master_seed, horizon))
        }
        M2aTaskKind::G2Tph(idx) => M2aTaskPayload::G2Tph {
            idx,
            piece: run_g2a2_tph_task(&specs.g2_tph[idx], master_seed, horizon),
        },
        M2aTaskKind::G3Solo(idx) => M2aTaskPayload::G3Solo {
            idx,
            out: run_g2a3_solo_task(&specs.g3_solos[idx], master_seed, horizon),
        },
        M2aTaskKind::G3Multi(idx) => M2aTaskPayload::G3Multi {
            idx,
            out: run_g2a3_multi_task(&specs.g3_multis[idx], master_seed, horizon),
        },
        M2aTaskKind::G4a(idx) => {
            let (piece, regret) = run_g2a4a_task(&specs.g4a[idx], master_seed, horizon);
            M2aTaskPayload::G4a { idx, piece, regret }
        }
        M2aTaskKind::G4b(idx) => {
            let (piece, sample, paired_diff) =
                run_g2a4b_task(&specs.g4b[idx], master_seed, horizon);
            M2aTaskPayload::G4b {
                idx,
                piece,
                sample,
                paired_diff,
            }
        }
        M2aTaskKind::G5(idx) => {
            let (piece, metrics) = run_g2a5_task(
                &specs.g5_panel[idx],
                idx,
                &specs.det_g5,
                master_seed,
                horizon,
            );
            M2aTaskPayload::G5 {
                idx,
                piece,
                metrics: Box::new(metrics),
            }
        }
    };
    M2aTaskOutput { seed_idx, payload }
}

fn reduce_task_pieces(pieces: impl IntoIterator<Item = (TaskOutcomePiece, usize)>) -> SeedOutcome {
    let mut cliff = CliffCountersSummary::default();
    let mut excluded_cases = 0usize;
    let mut failures = Vec::new();
    let mut all_ok = true;
    for (piece, excl) in pieces {
        cliff.accumulate(&piece.cliff);
        excluded_cases += excl;
        if !piece.ok {
            all_ok = false;
        }
        if let Some(f) = piece.failure {
            failures.push(f);
        }
    }
    SeedOutcome {
        passed: all_ok && excluded_cases == 0,
        cliff,
        excluded_cases,
        failures,
    }
}

type G4bTaskSample = (TaskOutcomePiece, (f64, f64), f64);

struct SeedTaskAccumulator {
    g1: Vec<Option<(TaskOutcomePiece, usize)>>,
    g2_deep: Vec<Option<TaskOutcomePiece>>,
    g2_ring: Option<TaskOutcomePiece>,
    g2_tph: Vec<Option<TaskOutcomePiece>>,
    g3_solos: Vec<Option<G2a3SoloOutcome>>,
    g3_multis: Vec<Option<G2a3MultiOutcome>>,
    g4a: Vec<Option<(TaskOutcomePiece, Option<f64>)>>,
    g4b: Vec<Option<G4bTaskSample>>,
    g5: Vec<Option<(TaskOutcomePiece, [G2a5RunMetrics; 7])>>,
}

fn vec_none<T>(len: usize) -> Vec<Option<T>> {
    (0..len).map(|_| None).collect()
}

impl SeedTaskAccumulator {
    fn new(specs: &M2aSharedSpecs) -> Self {
        Self {
            g1: vec_none(specs.g1.len()),
            g2_deep: vec_none(specs.g2_deep.len()),
            g2_ring: None,
            g2_tph: vec_none(specs.g2_tph.len()),
            g3_solos: vec_none(specs.g3_solos.len()),
            g3_multis: vec_none(specs.g3_multis.len()),
            g4a: vec_none(specs.g4a.len()),
            g4b: vec_none(specs.g4b.len()),
            g5: vec_none(specs.g5_panel.len()),
        }
    }

    fn insert(&mut self, payload: M2aTaskPayload) {
        match payload {
            M2aTaskPayload::G1 { idx, piece, excl } => self.g1[idx] = Some((piece, excl)),
            M2aTaskPayload::G2Deep { idx, piece } => self.g2_deep[idx] = Some(piece),
            M2aTaskPayload::G2M1Ring(piece) => self.g2_ring = Some(piece),
            M2aTaskPayload::G2Tph { idx, piece } => self.g2_tph[idx] = Some(piece),
            M2aTaskPayload::G3Solo { idx, out } => self.g3_solos[idx] = Some(out),
            M2aTaskPayload::G3Multi { idx, out } => self.g3_multis[idx] = Some(out),
            M2aTaskPayload::G4a { idx, piece, regret } => self.g4a[idx] = Some((piece, regret)),
            M2aTaskPayload::G4b {
                idx,
                piece,
                sample,
                paired_diff,
            } => self.g4b[idx] = Some((piece, sample, paired_diff)),
            M2aTaskPayload::G5 {
                idx,
                piece,
                metrics,
            } => self.g5[idx] = Some((piece, *metrics)),
        }
    }

    fn into_outcome(self, master_seed: u64, specs: &M2aSharedSpecs) -> AllGatesSeedOutcome {
        let unit_excl = |p: TaskOutcomePiece| {
            let ex = usize::from(p.excluded);
            (p, ex)
        };
        let mut task_records = Vec::new();

        let g1_items: Vec<(TaskOutcomePiece, usize)> = self
            .g1
            .into_iter()
            .enumerate()
            .map(|(i, x)| {
                let (p, ex) = x.expect("G2A.1 task completed");
                task_records.push(M2aTaskRecord {
                    gate_idx: 0,
                    task_label: format!("G2A.1:{}", specs.g1[i].label),
                    regime: GateRegime::Clean,
                    passed: p.ok && !p.excluded,
                    tail_metrics: vec![("max_target_kt_bits", p.target_kt)],
                });
                (p, ex)
            })
            .collect();
        let g1 = reduce_task_pieces(g1_items);

        let mut g2_items = Vec::new();
        for (i, x) in self.g2_deep.into_iter().enumerate() {
            let p = x.expect("G2A.2 deep task completed");
            task_records.push(M2aTaskRecord {
                gate_idx: 1,
                task_label: format!(
                    "G2A.2:DeepLag(d={},k={})",
                    specs.g2_deep[i].lag,
                    specs.g2_deep[i].width.bits()
                ),
                regime: GateRegime::Clean,
                passed: p.ok && !p.excluded,
                tail_metrics: vec![("max_target_kt_bits", p.target_kt)],
            });
            g2_items.push(unit_excl(p));
        }
        let ring_p = self.g2_ring.expect("G2A.2 M1 ring task completed");
        task_records.push(M2aTaskRecord {
            gate_idx: 1,
            task_label: "G2A.2:M1_PeriodicRing(k=1)".to_owned(),
            regime: GateRegime::Clean,
            passed: ring_p.ok && !ring_p.excluded,
            tail_metrics: vec![("max_target_kt_bits", ring_p.target_kt)],
        });
        g2_items.push(unit_excl(ring_p));
        for (i, x) in self.g2_tph.into_iter().enumerate() {
            let p = x.expect("G2A.2 Tph task completed");
            task_records.push(M2aTaskRecord {
                gate_idx: 1,
                task_label: format!("G2A.2:{}", specs.g2_tph[i].label),
                regime: GateRegime::Clean,
                passed: p.ok && !p.excluded,
                tail_metrics: vec![("max_target_kt_bits", p.target_kt)],
            });
            g2_items.push(unit_excl(p));
        }
        let g2 = reduce_task_pieces(g2_items);

        let solos: Vec<G2a3SoloOutcome> = self
            .g3_solos
            .into_iter()
            .map(|x| x.expect("G2A.3 solo task completed"))
            .collect();
        let multis: Vec<G2a3MultiOutcome> = self
            .g3_multis
            .into_iter()
            .map(|x| x.expect("G2A.3 multi task completed"))
            .collect();
        let (g3, g3_records) = reduce_g2a3_seed(master_seed, &solos, &multis, &specs.g3_multis);
        task_records.extend(g3_records);

        let mut g4_pieces = Vec::with_capacity(self.g4a.len() + self.g4b.len());
        for (i, x) in self.g4a.into_iter().enumerate() {
            let (piece, regret) = x.expect("G2A.4a task completed");
            let mut tail_metrics = vec![("max_target_kt_bits", piece.target_kt)];
            if let Some(r) = regret {
                tail_metrics.push(("max_paired_regret", r.abs()));
            }
            task_records.push(M2aTaskRecord {
                gate_idx: 3,
                task_label: format!("G2A.4a:{}", specs.g4a[i].label),
                regime: GateRegime::Noisy,
                passed: piece.ok && !piece.excluded,
                tail_metrics,
            });
            g4_pieces.push(unit_excl(piece));
        }
        let mut g4b_samples = Vec::with_capacity(self.g4b.len());
        for (i, item) in self.g4b.into_iter().enumerate() {
            let (piece, sample, paired_diff) = item.expect("G2A.4b task completed");
            task_records.push(M2aTaskRecord {
                gate_idx: 3,
                task_label: format!("G2A.4b:{}", specs.g4b[i].label),
                regime: GateRegime::Noisy,
                passed: piece.ok && !piece.excluded,
                tail_metrics: vec![
                    ("max_target_kt_bits", piece.target_kt),
                    ("max_abs_kt_diff", paired_diff),
                    ("max_h_reactive_dev", (sample.0 - sample.1).abs()),
                ],
            });
            g4_pieces.push(unit_excl(piece));
            g4b_samples.push(sample);
        }
        let g4 = reduce_task_pieces(g4_pieces);

        let mut g5_pieces = Vec::with_capacity(self.g5.len());
        let mut g5_metrics = Vec::with_capacity(self.g5.len());
        for (i, item) in self.g5.into_iter().enumerate() {
            let (piece, metrics) = item.expect("G2A.5 task completed");
            let arkhe_kt = metrics[0].target_kt;
            let mut min_adv =
                (metrics[1].target_kt - arkhe_kt).min(metrics[2].target_kt - arkhe_kt);
            if specs.g5_panel[i].nonlinear {
                min_adv = min_adv
                    .min(metrics[3].target_kt - arkhe_kt)
                    .min(metrics[4].target_kt - arkhe_kt);
            }
            task_records.push(M2aTaskRecord {
                gate_idx: 4,
                task_label: format!("G2A.5:{}", specs.g5_panel[i].label),
                regime: GateRegime::Clean,
                passed: piece.ok && !piece.excluded,
                tail_metrics: vec![("min_kt_advantage", min_adv)],
            });
            g5_pieces.push(unit_excl(piece));
            g5_metrics.push(metrics);
        }
        let g5 = reduce_task_pieces(g5_pieces);

        AllGatesSeedOutcome {
            gates: [g1, g2, g3, g4, g5],
            g4b_samples: g4b_samples.try_into().expect("4 G2A.4b samples per seed"),
            g5_metrics,
            task_records,
        }
    }
}

fn evaluate_m2a_seed_range(
    specs: &M2aSharedSpecs,
    seed_offset: u32,
    total_seeds: u32,
    horizon: u64,
) -> Vec<AllGatesSeedOutcome> {
    let total_seeds_usize = total_seeds.max(1) as usize;
    let per_seed = per_seed_task_schedule_lpt(specs);
    let mut queue = Vec::with_capacity(total_seeds_usize * per_seed.len());
    for task in per_seed {
        for seed_idx in 0..total_seeds_usize {
            queue.push((seed_idx, task));
        }
    }

    let total_tasks = queue.len();
    let next_idx = AtomicUsize::new(0);
    let num_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(total_tasks)
        .max(1);

    let mut accumulators: Vec<SeedTaskAccumulator> = (0..total_seeds_usize)
        .map(|_| SeedTaskAccumulator::new(specs))
        .collect();

    if num_threads <= 1 {
        for &(seed_idx, kind) in &queue {
            let master_seed = u64::from(seed_offset) + (seed_idx as u64);
            let out = execute_m2a_task(specs, seed_idx, kind, master_seed, horizon);
            accumulators[out.seed_idx].insert(out.payload);
        }
    } else {
        let mut per_thread = Vec::with_capacity(num_threads);
        std::thread::scope(|s| {
            let queue_ref = &queue;
            let next_ref = &next_idx;
            let mut handles = Vec::with_capacity(num_threads);
            for _ in 0..num_threads {
                handles.push(s.spawn(move || {
                    let mut local = Vec::new();
                    loop {
                        let idx = next_ref.fetch_add(1, Ordering::Relaxed);
                        if idx >= queue_ref.len() {
                            break;
                        }
                        let (seed_idx, kind) = queue_ref[idx];
                        let master_seed = u64::from(seed_offset) + (seed_idx as u64);
                        local.push(execute_m2a_task(
                            specs,
                            seed_idx,
                            kind,
                            master_seed,
                            horizon,
                        ));
                    }
                    local
                }));
            }
            for handle in handles {
                per_thread.push(handle.join().expect("m2a worker thread panicked"));
            }
        });
        for local in per_thread {
            for out in local {
                accumulators[out.seed_idx].insert(out.payload);
            }
        }
    }

    accumulators
        .into_iter()
        .enumerate()
        .map(|(seed_idx, acc)| {
            let master_seed = u64::from(seed_offset) + (seed_idx as u64);
            acc.into_outcome(master_seed, specs)
        })
        .collect()
}

fn summarize_single_bank(
    seed_offset: u32,
    seeds_tested: u32,
    horizon: u64,
    g5_panel: &[G2a5TaskSpec],
    outcomes: Vec<AllGatesSeedOutcome>,
) -> EvaluatedSingleBank {
    let mut pass_counts = [0usize; 5];
    let mut excl_counts = [0usize; 5];
    let mut gate_cliffs = [CliffCountersSummary::default(); 5];
    let mut gate_failures: [Vec<String>; 5] = Default::default();
    let mut gate_task_counts: [std::collections::BTreeMap<(String, GateRegime), (usize, usize)>;
        5] = Default::default();
    let mut gate_task_tails: [std::collections::BTreeMap<
        String,
        std::collections::BTreeMap<String, f64>,
    >; 5] = Default::default();
    let mut raw_g2a4b_metrics = Vec::with_capacity(outcomes.len());
    let mut raw_g2a5_metrics = Vec::with_capacity(outcomes.len());

    for item in outcomes {
        for (idx, g) in item.gates.into_iter().enumerate() {
            if g.passed {
                pass_counts[idx] += 1;
            }
            excl_counts[idx] += g.excluded_cases;
            gate_cliffs[idx].accumulate(&g.cliff);
            gate_failures[idx].extend(g.failures);
        }
        for rec in item.task_records {
            let entry = gate_task_counts[rec.gate_idx]
                .entry((rec.task_label.clone(), rec.regime))
                .or_insert((0, 0));
            entry.1 += 1;
            if rec.passed {
                entry.0 += 1;
            }
            if !rec.tail_metrics.is_empty() {
                let tm = gate_task_tails[rec.gate_idx]
                    .entry(rec.task_label)
                    .or_default();
                for (k, v) in rec.tail_metrics {
                    if k.starts_with("min_") {
                        update_min_finite(tm, k, v);
                    } else {
                        update_max_finite(tm, k, v);
                    }
                }
            }
        }
        raw_g2a4b_metrics.push(item.g4b_samples);
        raw_g2a5_metrics.push(item.g5_metrics);
    }

    let max_g4_mean_dev = max_g2a4b_mean_h_reactive_dev(&raw_g2a4b_metrics);
    let (g2a5_benchmarks, min_margin) = aggregate_g2a5_benchmarks(g5_panel, &raw_g2a5_metrics);

    let total = seeds_tested as usize;
    let mut total_cliff = CliffCountersSummary::default();
    let mut total_excluded = 0usize;
    let mut gates = Vec::with_capacity(5);
    let mut all_passed = true;

    for (idx, &(gate_name, target_rate)) in M2A_GATE_SPECS.iter().enumerate() {
        let passed_seeds = pass_counts[idx];
        let excl = excl_counts[idx];
        let empirical_rate = (passed_seeds as f64) / (total as f64);
        let bank_gate_stats =
            evaluate_multi_bank_gate(&[BankSample::new(passed_seeds, total)], target_rate);
        let clean_conj = [BankSample::new(passed_seeds, total)];
        let stratified = evaluate_stratified_from_bank_maps(
            &[&gate_task_counts[idx]],
            &[&gate_task_tails[idx]],
            None,
            (idx != 3).then_some(&clean_conj[..]),
            target_rate,
            target_rate,
            false,
        );
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
        let passed =
            excl == 0 && passed_seeds >= min_pass && g4_mean_ok && margin_ok && stratified.passed;
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
            stratified,
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
        gate_task_counts,
        gate_task_tails,
    }
}

/// Runs the full Milestone 2a verification suite across `G2A.1`–`G2A.5` on `[seed_offset, seed_offset + seeds)`.
///
/// # Errors
///
/// Returns an error if `horizon <= DEFAULT_M2A_WARMUP` or if the seed range overlaps a guarded
/// holdout bank in [`HOLDOUT_BANKS`], even when `allow_holdout` is true.
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
    let specs = M2aSharedSpecs::build(horizon);
    let outcomes = evaluate_m2a_seed_range(&specs, seed_offset, seeds_tested, horizon);
    Ok(summarize_single_bank(
        seed_offset,
        seeds_tested,
        horizon,
        &specs.g5_panel,
        outcomes,
    )
    .summary)
}

/// Runs Milestone 2a multi-bank evaluation across `banks` consecutive banks using a unified
/// parallel task pool, scoring each gate `G2A.1`–`G2A.5` via [`evaluate_multi_bank_gate`].
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

    let specs = M2aSharedSpecs::build(horizon);
    let all_outcomes = evaluate_m2a_seed_range(&specs, seed_offset, total_seeds, horizon);
    let mut outcomes_iter = all_outcomes.into_iter();
    let mut bank_outputs = Vec::with_capacity(banks as usize);
    for b in 0..banks {
        let b_offset = seed_offset.saturating_add(b.saturating_mul(seeds_per_bank));
        let slice: Vec<_> = outcomes_iter
            .by_ref()
            .take(seeds_per_bank as usize)
            .collect();
        bank_outputs.push(summarize_single_bank(
            b_offset,
            seeds_per_bank,
            horizon,
            &specs.g5_panel,
            slice,
        ));
    }

    let mut samples_per_gate: [Vec<BankSample>; 5] = Default::default();
    let mut banks_above_target = [0u32; 5];
    let mut gate_excl = [0usize; 5];
    let mut gate_cliffs = [CliffCountersSummary::default(); 5];
    let mut per_bank_task_counts: [Vec<BankTaskCountMap>; 5] = Default::default();
    let mut per_bank_task_tails: [Vec<BankTailMetricMap>; 5] = Default::default();
    let mut all_g2a4b_raw = Vec::with_capacity(total_seeds as usize);
    let mut all_g2a5_raw = Vec::with_capacity(total_seeds as usize);
    let mut failures = Vec::new();
    let mut total_cliff = CliffCountersSummary::default();
    let mut total_excluded = 0usize;

    for (b_idx, out) in bank_outputs.into_iter().enumerate() {
        total_cliff.accumulate(&out.summary.cliff_counters);
        total_excluded += out.summary.excluded_cases;
        all_g2a4b_raw.extend(out.raw_g2a4b_metrics);
        all_g2a5_raw.extend(out.raw_g2a5_metrics);
        for (gi, tc) in out.gate_task_counts.into_iter().enumerate() {
            per_bank_task_counts[gi].push(tc);
        }
        for (gi, tt) in out.gate_task_tails.into_iter().enumerate() {
            per_bank_task_tails[gi].push(tt);
        }

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
    let (g2a5_benchmarks, pooled_min_margin) =
        aggregate_g2a5_benchmarks(&specs.g5_panel, &all_g2a5_raw);

    let mut gates = Vec::with_capacity(5);
    let mut all_passed = total_excluded == 0;
    for (idx, &(gate_name, target_rate)) in M2A_GATE_SPECS.iter().enumerate() {
        let stats = evaluate_multi_bank_gate(&samples_per_gate[idx], target_rate);
        let count_refs: Vec<&_> = per_bank_task_counts[idx].iter().collect();
        let tail_refs: Vec<&_> = per_bank_task_tails[idx].iter().collect();
        let stratified = evaluate_stratified_from_bank_maps(
            &count_refs,
            &tail_refs,
            None,
            (idx != 3).then_some(samples_per_gate[idx].as_slice()),
            target_rate,
            target_rate,
            true,
        );
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
        let passed = stats.passed && stratified.passed && excl == 0 && g4_mean_ok && margin_ok;
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
            stratified,
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
        // M1 (10000..=10019), closed M2a (30000..=30019), closed M2b (40000..=40019),
        // closed M3 v1 (50000..=50019), M3 v2 (50100..=50199), and M4a (60000..=60099)
        // are always refused even with allow_holdout=true.
        for blocked in [10_000u32, 30_000, 40_000, 50_000, 50_100, 60_000, 60_050] {
            assert!(check_m2a_seed_range_holdout(blocked, 20, false).is_err());
            assert!(check_m2a_seed_range_holdout(blocked, 20, true).is_err());
            assert!(check_m2a_seed_range_holdout(blocked - 10, 20, true).is_err());
        }
        assert!(check_m2a_seed_range_holdout(29_900, 101, false).is_err());

        // Disjoint dev and confirmation ranges succeed cleanly
        for safe in [0u32, 20, 20_020, 49_980, 50_020, 50_200, 59_980, 60_100] {
            assert!(check_m2a_seed_range_holdout(safe, 20, false).is_ok());
        }
        assert!(check_m2a_seed_range_holdout(20, 1_000, false).is_ok());
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
