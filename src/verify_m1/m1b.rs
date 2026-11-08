//! Milestone 1b acceptance verification suite (Gates G1B.1–G1B.4 and §8 Ablations).

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::baselines::run_prequential_harness;
use crate::config::{
    BaselineConfig, BudgetConfig, BuildProvenance, CONFIG_SCHEMA_VERSION, EnvironmentConfig,
    ExperimentConfig, SubstrateConfig, compute_ground_truth_yardstick,
};
use crate::env::{GeneratorConfig, NoiseModel, NoisyEnvironment};
use crate::metrics::Scorecard;
use crate::rng::SeedBank;
use crate::structural::economy::{EconomyConfig, RewardMode};
use crate::structural::growth::GrowthConfig;
use crate::structural::lattice::{
    BoundaryAnchor, Coord, GridShape, LatticeCircuit, PerimeterSide, WiringTopology,
};
use crate::structural::learner::ArkheLearner;
use crate::structural::surgery::EditableLattice;
use crate::substrate::{Primitive, SignalSink, SignalSource, Width, WireDelayMode};
use crate::verify_m1::{
    M1aBankReport, M1aVerificationSummary, check_seed_range_holdout, run_m1a_bank_evaluation,
    run_m1a_verification_suite_with_horizon_and_holdout,
};

/// Default evaluation horizon for M1b online runs (2000 ticks).
pub const DEFAULT_M1B_HORIZON: u64 = 2000;

static TMP_NONCE: AtomicU64 = AtomicU64::new(1);

/// Individual gate outcome report for M1b.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M1bGateReport {
    /// Gate identifier (e.g. "G1B.1").
    pub gate: String,
    /// Whether all checks for this gate passed.
    pub passed: bool,
    /// Total cases or seeds evaluated.
    pub cases: usize,
    /// Failure explanations, if any.
    pub failures: Vec<String>,
}

/// Structured summary emitted by `verify-m1b`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M1bVerificationSummary {
    /// Build and source-tree provenance.
    pub provenance: BuildProvenance,
    /// Starting master seed offset.
    pub seed_offset: u32,
    /// Master seeds evaluated.
    pub seeds_tested: u32,
    /// Simulation horizon in ticks.
    pub horizon: u64,
    /// Results for gates G1B.1 through G1B.4.
    pub gates: Vec<M1bGateReport>,
    /// Whether all M1b gates passed.
    pub all_passed: bool,
}

/// Gate pass count across scored M1b seed banks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M1bGatePassCount {
    /// Gate identifier.
    pub gate: String,
    /// Number of banks in which all gate checks passed.
    pub passed_banks: u32,
    /// Total number of banks evaluated.
    pub total_banks: u32,
}

/// Structured report emitted by `verify-m1b --banks <N>`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M1bBankReport {
    /// Build and source-tree provenance.
    pub provenance: BuildProvenance,
    /// Starting master seed offset.
    pub seed_offset: u32,
    /// Master seeds per bank.
    pub seeds_per_bank: u32,
    /// Number of consecutive banks evaluated.
    pub banks_evaluated: u32,
    /// Simulation horizon in ticks.
    pub horizon: u64,
    /// Gate pass counts across all evaluated banks.
    pub gate_passes: Vec<M1bGatePassCount>,
    /// Number of banks in which all four M1b gates passed.
    pub all_passed_banks: u32,
    /// Diagnostic failure messages from any failed bank (empty when all pass).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub failures: Vec<String>,
}

impl M1bBankReport {
    /// Returns `true` if every evaluated bank passed all four M1b gates.
    #[must_use]
    pub fn all_passed(&self) -> bool {
        self.banks_evaluated > 0 && self.all_passed_banks == self.banks_evaluated
    }
}

/// Combined Milestone 1 summary enforcing the hard M1a stage gate before M1b.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M1CombinedSummary {
    /// M1a metabolic economy verification summary.
    pub m1a: M1aVerificationSummary,
    /// M1b neurogenesis verification summary (omitted if stage gate aborted).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub m1b: Option<M1bVerificationSummary>,
    /// Whether M1b was aborted due to M1a failure.
    pub stage_gate_aborted: bool,
    /// Whether both M1a and M1b passed all gates.
    pub all_passed: bool,
}

/// Combined Milestone 1 multi-bank summary enforcing the hard M1a stage gate before M1b.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M1CombinedBankReport {
    /// M1a multi-bank evaluation report.
    pub m1a: M1aBankReport,
    /// M1b multi-bank evaluation report (omitted if M1a stage gate aborted).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub m1b: Option<M1bBankReport>,
    /// Whether M1b bank evaluation was aborted due to M1a bank failure.
    pub stage_gate_aborted: bool,
    /// Whether both M1a and M1b passed 100% of evaluated banks.
    pub all_passed: bool,
}

#[derive(Clone, Debug)]
struct OnlineRunAudit {
    label: String,
    circuit_compiles: bool,
    budget_respected: bool,
    max_nodes_respected: bool,
}

impl OnlineRunAudit {
    fn record(
        label: impl Into<String>,
        learner: &ArkheLearner,
        scorecard: &Scorecard,
        max_eval_ticks: u64,
    ) -> Self {
        let circuit_compiles = learner.editable().circuit().compile().is_ok();
        let budget_respected = scorecard.candidate_replay_ticks_consumed <= max_eval_ticks;
        let max_nodes_respected =
            learner.editable().circuit().node_count() <= learner.growth_config().max_nodes;
        Self {
            label: label.into(),
            circuit_compiles,
            budget_respected,
            max_nodes_respected,
        }
    }
}

fn locate_arkhe_binary() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("CARGO_BIN_EXE_arkhe") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Some(pb);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if exe.file_stem().is_some_and(|s| s == "arkhe") && exe.is_file() {
            return Some(exe);
        }
        if let Some(parent) = exe.parent().and_then(|p| p.parent()) {
            let sibling = parent.join(if cfg!(windows) { "arkhe.exe" } else { "arkhe" });
            if sibling.is_file() {
                return Some(sibling);
            }
        }
    }
    [
        PathBuf::from("target/release/arkhe"),
        PathBuf::from("target/debug/arkhe"),
        PathBuf::from("./target/release/arkhe"),
        PathBuf::from("./target/debug/arkhe"),
    ]
    .into_iter()
    .find(|candidate| candidate.is_file())
}

/// Enforces the hard M1a stage gate before invoking `run_m1b`.
#[must_use]
pub fn evaluate_m1_stage_gate<F>(m1a: M1aVerificationSummary, run_m1b: F) -> M1CombinedSummary
where
    F: FnOnce() -> M1bVerificationSummary,
{
    if !m1a.all_passed {
        return M1CombinedSummary {
            m1a,
            m1b: None,
            stage_gate_aborted: true,
            all_passed: false,
        };
    }
    let m1b = run_m1b();
    let all_passed = m1a.all_passed && m1b.all_passed;
    M1CombinedSummary {
        m1a,
        m1b: Some(m1b),
        stage_gate_aborted: false,
        all_passed,
    }
}

/// Runs combined Milestone 1 verification (hard M1a stage gate -> M1b neurogenesis).
///
/// # Errors
///
/// Returns an error if the seed range overlaps `10000..=10019` without `allow_holdout`.
pub fn run_m1_combined_verification(
    seed_offset: u32,
    seeds: u32,
    m1a_horizon: u64,
    m1b_horizon: u64,
    allow_holdout: bool,
) -> Result<M1CombinedSummary, String> {
    check_seed_range_holdout(seed_offset, seeds, allow_holdout)?;
    let m1a = run_m1a_verification_suite_with_horizon_and_holdout(
        seed_offset,
        seeds,
        m1a_horizon,
        allow_holdout,
    )?;
    Ok(evaluate_m1_stage_gate(m1a, || {
        run_m1b_verification_suite(seed_offset, seeds, m1b_horizon)
    }))
}

/// Evaluates `banks` consecutive seed banks on M1b gates `G1B.1`–`G1B.4` in parallel.
///
/// # Errors
///
/// Returns an error if `banks == 0` or if the spanned seed range overlaps holdout seeds without permission.
pub fn run_m1b_bank_evaluation(
    seed_offset: u32,
    seeds_per_bank: u32,
    banks: u32,
    horizon: u64,
    allow_holdout: bool,
) -> Result<M1bBankReport, String> {
    if banks == 0 {
        return Err("banks count must be >= 1".to_owned());
    }
    let total_seeds = seeds_per_bank.saturating_mul(banks);
    check_seed_range_holdout(seed_offset, total_seeds, allow_holdout)?;

    let num_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(banks as usize)
        .max(1);

    let mut bank_summaries: Vec<Option<M1bVerificationSummary>> =
        (0..banks).map(|_| None).collect();

    std::thread::scope(|s| {
        let chunk_size = (banks as usize).div_ceil(num_threads);
        let slices: Vec<&mut [Option<M1bVerificationSummary>]> =
            bank_summaries.chunks_mut(chunk_size).collect();
        for (thread_idx, slice) in slices.into_iter().enumerate() {
            let start_bank = thread_idx * chunk_size;
            s.spawn(move || {
                for (offset_in_slice, slot) in slice.iter_mut().enumerate() {
                    let b = (start_bank + offset_in_slice) as u32;
                    let bank_offset = seed_offset.saturating_add(b.saturating_mul(seeds_per_bank));
                    *slot = Some(run_m1b_verification_suite(
                        bank_offset,
                        seeds_per_bank,
                        horizon,
                    ));
                }
            });
        }
    });

    let mut gate_counts = [0u32; 4];
    let mut all_passed_banks = 0u32;
    let mut failures = Vec::new();

    for (b_idx, summary) in bank_summaries.into_iter().enumerate() {
        let summary = summary.expect("bank summary computed");
        for (g_idx, g) in summary.gates.iter().enumerate().take(4) {
            if g.passed {
                gate_counts[g_idx] += 1;
            } else {
                for f in &g.failures {
                    failures.push(format!(
                        "bank {b_idx} (offset {}): {f}",
                        summary.seed_offset
                    ));
                }
            }
        }
        if summary.all_passed {
            all_passed_banks += 1;
        }
    }

    let gate_names = ["G1B.1", "G1B.2", "G1B.3", "G1B.4"];
    let gate_passes = gate_names
        .into_iter()
        .enumerate()
        .map(|(i, name)| M1bGatePassCount {
            gate: name.to_owned(),
            passed_banks: gate_counts[i],
            total_banks: banks,
        })
        .collect();

    Ok(M1bBankReport {
        provenance: BuildProvenance::current(),
        seed_offset,
        seeds_per_bank,
        banks_evaluated: banks,
        horizon,
        gate_passes,
        all_passed_banks,
        failures,
    })
}

/// Evaluates `banks` consecutive seed banks across combined Milestone 1 (hard M1a stage gate -> M1b).
///
/// # Errors
///
/// Returns an error if `banks == 0` or if the spanned seed range overlaps holdout seeds without permission.
pub fn run_m1_combined_bank_evaluation(
    seed_offset: u32,
    seeds_per_bank: u32,
    banks: u32,
    m1a_horizon: u64,
    m1b_horizon: u64,
    allow_holdout: bool,
) -> Result<M1CombinedBankReport, String> {
    if banks == 0 {
        return Err("banks count must be >= 1".to_owned());
    }
    let total_seeds = seeds_per_bank.saturating_mul(banks);
    check_seed_range_holdout(seed_offset, total_seeds, allow_holdout)?;

    let m1a = run_m1a_bank_evaluation(
        seed_offset,
        seeds_per_bank,
        banks,
        m1a_horizon,
        allow_holdout,
    )?;
    if !m1a.all_passed() {
        return Ok(M1CombinedBankReport {
            m1a,
            m1b: None,
            stage_gate_aborted: true,
            all_passed: false,
        });
    }

    let m1b = run_m1b_bank_evaluation(
        seed_offset,
        seeds_per_bank,
        banks,
        m1b_horizon,
        allow_holdout,
    )?;
    let all_passed = m1a.all_passed() && m1b.all_passed();
    Ok(M1CombinedBankReport {
        m1a,
        m1b: Some(m1b),
        stage_gate_aborted: false,
        all_passed,
    })
}

/// Runs the complete M1b verification suite across gates G1B.1 through G1B.4.
#[must_use]
pub fn run_m1b_verification_suite(
    seed_offset: u32,
    seeds: u32,
    horizon: u64,
) -> M1bVerificationSummary {
    let seeds = seeds.max(1);
    let mut online_audits = Vec::new();

    let mut g1_failures = Vec::new();
    let (g1_passed, g1_cases) = verify_gate_g1b1(
        seed_offset,
        seeds,
        horizon,
        &mut online_audits,
        &mut g1_failures,
    );

    let mut g2_failures = Vec::new();
    let (g2_passed, g2_cases) = verify_gate_g1b2(
        seed_offset,
        seeds,
        horizon,
        &mut online_audits,
        &mut g2_failures,
    );

    let mut g4_failures = Vec::new();
    let (g4_passed, g4_cases) = verify_gate_g1b4(
        seed_offset,
        seeds,
        horizon,
        &mut online_audits,
        &mut g4_failures,
    );

    let mut g3_failures = Vec::new();
    let (g3_passed, g3_cases) = verify_gate_g1b3(&online_audits, &mut g3_failures);

    let gates = vec![
        M1bGateReport {
            gate: "G1B.1".to_owned(),
            passed: g1_passed,
            cases: g1_cases,
            failures: g1_failures,
        },
        M1bGateReport {
            gate: "G1B.2".to_owned(),
            passed: g2_passed,
            cases: g2_cases,
            failures: g2_failures,
        },
        M1bGateReport {
            gate: "G1B.3".to_owned(),
            passed: g3_passed,
            cases: g3_cases,
            failures: g3_failures,
        },
        M1bGateReport {
            gate: "G1B.4".to_owned(),
            passed: g4_passed,
            cases: g4_cases,
            failures: g4_failures,
        },
    ];

    let all_passed = gates.iter().all(|g| g.passed);

    M1bVerificationSummary {
        provenance: BuildProvenance::current(),
        seed_offset,
        seeds_tested: seeds,
        horizon,
        gates,
        all_passed,
    }
}

/// Gate G1B.1: Stepwise first-to-second neuron reduction.
fn verify_gate_g1b1(
    seed_offset: u32,
    seeds: u32,
    horizon: u64,
    audits: &mut Vec<OnlineRunAudit>,
    failures: &mut Vec<String>,
) -> (bool, usize) {
    let tasks = [
        (
            Width::BITS_1,
            GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 1, 1],
            },
            "PeriodicMotif([0,1,1])",
        ),
        (
            Width::BITS_2,
            GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 1, 3],
            },
            "PeriodicMotif([0,1,3])",
        ),
        (
            Width::BITS_2,
            GeneratorConfig::T1_AffineLag1 {
                mul_a: 1,
                add_b: 1,
                init: 0,
            },
            "AffineLag1(1,1)",
        ),
        (
            Width::BITS_2,
            GeneratorConfig::T3b_CounterModN {
                modulus: 3,
                init: 0,
            },
            "CounterModN(3)",
        ),
    ];

    let min_pass = ((seeds as f64) * 0.95).ceil() as u32;
    let mut cases = 0usize;

    for (width, gen_cfg, task_name) in tasks {
        let mut seeds_passed = 0u32;

        for seed_idx in 0..seeds {
            cases += 1;
            let master_seed = u64::from(seed_offset + seed_idx);
            let seeds_bank = SeedBank::new(master_seed);

            let generator = gen_cfg.build(width).expect("valid generator");
            let mut env = NoisyEnvironment::new(generator, NoiseModel::None, seeds_bank)
                .expect("valid environment");

            let mut learner = ArkheLearner::new_with_spatial(
                width,
                1,
                WireDelayMode::Combinational,
                WiringTopology::WellMixed,
                GridShape::new(8, 8).unwrap(),
                4,
                EconomyConfig::default(),
                GrowthConfig::default(),
                seeds_bank,
            )
            .expect("learner creation succeeds");

            let scorecard = run_prequential_harness(
                &mut env,
                &mut learner,
                WiringTopology::WellMixed,
                horizon,
                500,
                50_000,
                None,
                None,
            );
            audits.push(OnlineRunAudit::record(
                format!("G1B.1:{task_name}:seed={master_seed}"),
                &learner,
                &scorecard,
                50_000,
            ));

            let snaps = learner.epoch_snapshots();
            if snaps.len() >= 3 {
                let l0 = snaps[0].kt_loss_bits_per_symbol;
                let l1 = snaps[1].kt_loss_bits_per_symbol;
                let l2 = snaps[2].kt_loss_bits_per_symbol;
                let hr1 = snaps[1].residual_entropy_bits;
                let hr2 = snaps[2].residual_entropy_bits;
                let mi = snaps[1].best_feature_mi_bits.unwrap_or(0.0);

                let c1 = l1 < l0;
                let c2 = l2 < l1;
                let c3 = learner.starvation_prune_events() == 0;
                let c4 = ((hr1 - hr2) - mi).abs() <= 0.05;

                if c1 && c2 && c3 && c4 {
                    seeds_passed += 1;
                }
            }
        }

        if seeds_passed < min_pass {
            failures.push(format!(
                "G1B.1: task {task_name} passed on {seeds_passed}/{seeds} seeds (required {min_pass})"
            ));
        }
    }

    (failures.is_empty(), cases)
}

struct FixedReactiveEchoBaseline {
    circuit: crate::substrate::Circuit,
    compiled: crate::substrate::CompiledCircuit,
    state: crate::substrate::ScalarState,
    prev_observed: Vec<u64>,
    pending_pred: Vec<u64>,
}

impl FixedReactiveEchoBaseline {
    fn new(lattice: &LatticeCircuit) -> Self {
        let circuit = lattice.lower().expect("valid fixed echo lattice");
        let compiled = circuit.compile().expect("valid fixed echo circuit");
        let state = crate::substrate::ScalarState::new(&compiled);
        let inputs = usize::from(compiled.num_inputs());
        let outputs = usize::from(compiled.num_outputs());
        Self {
            circuit,
            compiled,
            state,
            prev_observed: vec![0; inputs],
            pending_pred: vec![0; outputs],
        }
    }
}

impl crate::baselines::PredictorBaseline for FixedReactiveEchoBaseline {
    fn name(&self) -> &'static str {
        "FixedReactiveEcho"
    }

    fn predict(&mut self, _budget: &mut crate::baselines::TickBudget) -> Vec<u64> {
        self.state
            .step(&self.compiled, &self.prev_observed, &mut self.pending_pred)
            .expect("fixed echo step succeeds");
        self.pending_pred.clone()
    }

    fn observe(
        &mut self,
        truth: &[u64],
        _budget: &mut crate::baselines::TickBudget,
    ) -> Option<f64> {
        for (slot, &val) in self.prev_observed.iter_mut().zip(truth) {
            *slot = self.circuit.width().normalize(val);
        }
        None
    }

    fn best_circuit(&self) -> Option<&crate::substrate::Circuit> {
        Some(&self.circuit)
    }

    fn budget_accounting_mode(&self) -> crate::metrics::BudgetAccountingMode {
        crate::metrics::BudgetAccountingMode::AnalyticalUnmetered
    }
}

fn build_fixed_reactive_echo(width: Width, lag_ticks: usize, invert: bool) -> LatticeCircuit {
    let mut circ = LatticeCircuit::new(
        GridShape::new(8, 8).unwrap(),
        width,
        WireDelayMode::Combinational,
        WiringTopology::WellMixed,
        vec![BoundaryAnchor {
            side: PerimeterSide::North,
            offset: 3,
        }],
        vec![BoundaryAnchor {
            side: PerimeterSide::North,
            offset: 4,
        }],
    )
    .unwrap();
    assert!(lag_ticks >= 1);
    let mut curr_src = SignalSource::BoundaryIn(0);
    for step in 0..lag_ticks.saturating_sub(1) {
        let d = circ
            .place_node(
                Coord {
                    row: (step / 8) as u16,
                    col: (step % 8) as u16,
                },
                Primitive::Delay { init: 0 },
            )
            .unwrap();
        circ.connect(curr_src, SignalSink::NodeIn { node: d, port: 0 })
            .unwrap();
        curr_src = SignalSource::NodeOut { node: d, port: 0 };
    }
    if invert {
        let cell = circ.nearest_empty_cell(Coord { row: 0, col: 0 }).unwrap();
        if width == Width::BITS_1 {
            let eqz = circ.place_node(cell, Primitive::Eqz).unwrap();
            circ.connect(curr_src, SignalSink::NodeIn { node: eqz, port: 0 })
                .unwrap();
            curr_src = SignalSource::NodeOut { node: eqz, port: 0 };
        } else {
            let sub = circ.place_node(cell, Primitive::Sub).unwrap();
            let c_cell = circ.nearest_empty_cell(Coord { row: 0, col: 0 }).unwrap();
            let s_cell = circ.nearest_empty_cell(Coord { row: 1, col: 0 }).unwrap();
            let d_const = circ
                .place_node(c_cell, Primitive::Delay { init: width.mask() })
                .unwrap();
            let sp = circ.place_node(s_cell, Primitive::Split).unwrap();
            circ.connect(
                SignalSource::NodeOut {
                    node: d_const,
                    port: 0,
                },
                SignalSink::NodeIn { node: sp, port: 0 },
            )
            .unwrap();
            circ.connect(
                SignalSource::NodeOut { node: sp, port: 0 },
                SignalSink::NodeIn {
                    node: d_const,
                    port: 0,
                },
            )
            .unwrap();
            circ.connect(
                SignalSource::NodeOut { node: sp, port: 1 },
                SignalSink::NodeIn { node: sub, port: 0 },
            )
            .unwrap();
            circ.connect(curr_src, SignalSink::NodeIn { node: sub, port: 1 })
                .unwrap();
            curr_src = SignalSource::NodeOut { node: sub, port: 0 };
        }
    } else if matches!(curr_src, SignalSource::BoundaryIn(_)) {
        let sp = circ
            .place_node(Coord { row: 0, col: 0 }, Primitive::Split)
            .unwrap();
        circ.connect(curr_src, SignalSink::NodeIn { node: sp, port: 0 })
            .unwrap();
        curr_src = SignalSource::NodeOut { node: sp, port: 0 };
    }
    circ.connect(curr_src, SignalSink::BoundaryOut(0)).unwrap();
    circ
}

/// Gate G1B.2: T1 suite convergence & noise-floor halting.
fn verify_gate_g1b2(
    seed_offset: u32,
    seeds: u32,
    horizon: u64,
    audits: &mut Vec<OnlineRunAudit>,
    failures: &mut Vec<String>,
) -> (bool, usize) {
    let min_pass = ((seeds as f64) * 0.90).ceil() as u32;
    let mut cases = 0usize;

    // (a) Noiseless cases
    // 1. Silent/constant streams: final_node_count == 0 and 0 sprouts on 100% of runs
    for width in [Width::BITS_1, Width::BITS_2] {
        let max_val = width.mask();
        for &val in &[0, max_val] {
            let gen_cfg = GeneratorConfig::T1_Constant { value: val };
            let mut zero_count = 0u32;
            for seed_idx in 0..seeds {
                cases += 1;
                let master_seed = u64::from(seed_offset + seed_idx);
                let seeds_bank = SeedBank::new(master_seed);
                let generator = gen_cfg.build(width).expect("valid generator");
                let mut env =
                    NoisyEnvironment::new(generator, NoiseModel::None, seeds_bank).unwrap();
                let mut learner = ArkheLearner::new_with_spatial(
                    width,
                    1,
                    WireDelayMode::Combinational,
                    WiringTopology::WellMixed,
                    GridShape::new(8, 8).unwrap(),
                    4,
                    EconomyConfig::default(),
                    GrowthConfig::default(),
                    seeds_bank,
                )
                .unwrap();
                let scorecard = run_prequential_harness(
                    &mut env,
                    &mut learner,
                    WiringTopology::WellMixed,
                    horizon,
                    500,
                    50_000,
                    None,
                    None,
                );
                audits.push(OnlineRunAudit::record(
                    format!(
                        "G1B.2a:Constant({val}):k={}:seed={master_seed}",
                        width.bits()
                    ),
                    &learner,
                    &scorecard,
                    50_000,
                ));
                if learner.editable().circuit().node_count() == 0
                    && learner.sprout_events() == 0
                    && learner.ring_closure_events() == 0
                {
                    zero_count += 1;
                }
            }
            if zero_count < seeds {
                failures.push(format!(
                    "G1B.2a: T1_Constant({val}) width {width:?} sprouted nodes on {}/{seeds} runs (expected 0)",
                    seeds - zero_count
                ));
            }
        }
    }

    // 2. Structured noiseless streams across both k=1 and k=2
    let noiseless_tasks = [
        (
            Width::BITS_1,
            GeneratorConfig::T1_Alternating { a: 0, b: 1 },
            "Alternating(k=1)",
        ),
        (
            Width::BITS_2,
            GeneratorConfig::T1_Alternating { a: 0, b: 3 },
            "Alternating(k=2)",
        ),
        (
            Width::BITS_1,
            GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 1, 1],
            },
            "Motif011(k=1)",
        ),
        (
            Width::BITS_2,
            GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 1, 3],
            },
            "Motif013(k=2)",
        ),
        (
            Width::BITS_1,
            GeneratorConfig::T1_AffineLag1 {
                mul_a: 1,
                add_b: 1,
                init: 0,
            },
            "AffineLag1(k=1)",
        ),
        (
            Width::BITS_2,
            GeneratorConfig::T1_AffineLag1 {
                mul_a: 1,
                add_b: 1,
                init: 0,
            },
            "AffineLag1(k=2)",
        ),
    ];

    for (width, gen_cfg, name) in noiseless_tasks {
        let mut clean_pass = 0u32;
        for seed_idx in 0..seeds {
            cases += 1;
            let master_seed = u64::from(seed_offset + seed_idx);
            let seeds_bank = SeedBank::new(master_seed);
            let generator = gen_cfg.build(width).expect("valid generator");
            let mut env = NoisyEnvironment::new(generator, NoiseModel::None, seeds_bank).unwrap();
            let mut learner = ArkheLearner::new_with_spatial(
                width,
                1,
                WireDelayMode::Combinational,
                WiringTopology::WellMixed,
                GridShape::new(8, 8).unwrap(),
                4,
                EconomyConfig::default(),
                GrowthConfig::default(),
                seeds_bank,
            )
            .unwrap();
            let scorecard = run_prequential_harness(
                &mut env,
                &mut learner,
                WiringTopology::WellMixed,
                horizon,
                500,
                50_000,
                None,
                None,
            );
            audits.push(OnlineRunAudit::record(
                format!("G1B.2a:{name}:seed={master_seed}"),
                &learner,
                &scorecard,
                50_000,
            ));
            if scorecard.post_transient_clean_mismatches == 0 {
                clean_pass += 1;
            }
        }
        if clean_pass < min_pass {
            failures.push(format!(
                "G1B.2a: {name} achieved 0 post-transient clean mismatches on only {clean_pass}/{seeds} seeds (required {min_pass})"
            ));
        }
    }

    // (b) Noisy cases (epsilon = 0.05, IndependentSubstitution)
    let noise = NoiseModel::IndependentSubstitution {
        epsilon: 0.05,
        channels: vec![0],
    };

    // 1. Noisy Constant(0) and Constant(width.mask()) noise-floor halting without thrashing
    for width in [Width::BITS_1, Width::BITS_2] {
        for &val in &[0, width.mask()] {
            let const_cfg = GeneratorConfig::T1_Constant { value: val };
            let gt = compute_ground_truth_yardstick(
                &const_cfg,
                width,
                WireDelayMode::Combinational,
                &noise,
                512,
                None,
            )
            .unwrap();
            let mut halt_pass = 0u32;
            let mut regret_pass = 0u32;

            for seed_idx in 0..seeds {
                cases += 1;
                let master_seed = u64::from(seed_offset + seed_idx);
                let seeds_bank = SeedBank::new(master_seed);
                let generator = const_cfg.build(width).unwrap();
                let mut env = NoisyEnvironment::new(generator, noise.clone(), seeds_bank).unwrap();
                let mut learner = ArkheLearner::new_with_spatial(
                    width,
                    1,
                    WireDelayMode::Combinational,
                    WiringTopology::WellMixed,
                    GridShape::new(8, 8).unwrap(),
                    4,
                    EconomyConfig::default(),
                    GrowthConfig::default(),
                    seeds_bank,
                )
                .unwrap();
                let scorecard = run_prequential_harness(
                    &mut env,
                    &mut learner,
                    WiringTopology::WellMixed,
                    horizon,
                    500,
                    50_000,
                    gt.epsilon_machine.as_ref(),
                    None,
                );
                audits.push(OnlineRunAudit::record(
                    format!(
                        "G1B.2b:NoisyConstant({val}):k={}:seed={master_seed}",
                        width.bits()
                    ),
                    &learner,
                    &scorecard,
                    50_000,
                ));

                if learner.editable().circuit().node_count() == 0
                    && learner.sprout_events() == 0
                    && learner.starvation_prune_events() == 0
                    && learner.ring_closure_events() == 0
                {
                    halt_pass += 1;
                }
                if val == 0
                    && scorecard
                        .paired_regret_bits_per_symbol
                        .is_some_and(|regret| regret <= 0.025)
                {
                    regret_pass += 1;
                }
            }

            if halt_pass < min_pass {
                failures.push(format!(
                    "G1B.2b: noisy T1_Constant({val}) width {width:?} halted cleanly without thrashing on only {halt_pass}/{seeds} seeds (required {min_pass})"
                ));
            }
            if val == 0 && regret_pass < min_pass {
                failures.push(format!(
                    "G1B.2b: noisy T1_Constant(0) width {width:?} regret <= 0.025 on only {regret_pass}/{seeds} seeds (required {min_pass})"
                ));
            }
        }
    }

    // 2. Noisy Alternating and PeriodicMotif convergence + dominance over fixed reactive echo
    let noisy_periodic_tasks = [
        (
            Width::BITS_1,
            GeneratorConfig::T1_Alternating { a: 0, b: 1 },
            1usize,
            true,
            "NoisyAlternating(k=1)",
        ),
        (
            Width::BITS_2,
            GeneratorConfig::T1_Alternating { a: 0, b: 3 },
            1usize,
            true,
            "NoisyAlternating(k=2)",
        ),
        (
            Width::BITS_1,
            GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 1, 1],
            },
            3usize,
            false,
            "NoisyMotif011(k=1)",
        ),
        (
            Width::BITS_2,
            GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 1, 3],
            },
            3usize,
            false,
            "NoisyMotif013(k=2)",
        ),
    ];

    for (width, task_cfg, echo_lag, echo_invert, label) in noisy_periodic_tasks {
        let gt = compute_ground_truth_yardstick(
            &task_cfg,
            width,
            WireDelayMode::Combinational,
            &noise,
            512,
            None,
        )
        .unwrap();
        let echo_template_circ = build_fixed_reactive_echo(width, echo_lag, echo_invert);

        let mut regret_pass = 0u32;
        let mut arkhe_regrets = Vec::with_capacity(seeds as usize);
        let mut echo_regrets = Vec::with_capacity(seeds as usize);

        for seed_idx in 0..seeds {
            cases += 1;
            let master_seed = u64::from(seed_offset + seed_idx);
            let seeds_bank = SeedBank::new(master_seed);
            let generator = task_cfg.build(width).unwrap();
            let mut env = NoisyEnvironment::new(generator, noise.clone(), seeds_bank).unwrap();
            let mut learner = ArkheLearner::new_with_spatial(
                width,
                1,
                WireDelayMode::Combinational,
                WiringTopology::WellMixed,
                GridShape::new(8, 8).unwrap(),
                4,
                EconomyConfig::default(),
                GrowthConfig::default(),
                seeds_bank,
            )
            .unwrap();
            let scorecard = run_prequential_harness(
                &mut env,
                &mut learner,
                WiringTopology::WellMixed,
                horizon,
                500,
                50_000,
                gt.epsilon_machine.as_ref(),
                None,
            );
            audits.push(OnlineRunAudit::record(
                format!("G1B.2b:{label}:seed={master_seed}"),
                &learner,
                &scorecard,
                50_000,
            ));

            if let Some(regret) = scorecard.paired_regret_bits_per_symbol {
                if regret <= 0.025 {
                    regret_pass += 1;
                }
                arkhe_regrets.push(regret);
            }

            let echo_seed_bank = SeedBank::new(master_seed);
            let echo_gen = task_cfg.build(width).unwrap();
            let mut echo_env =
                NoisyEnvironment::new(echo_gen, noise.clone(), echo_seed_bank).unwrap();
            let mut echo_baseline = FixedReactiveEchoBaseline::new(&echo_template_circ);
            let echo_score = run_prequential_harness(
                &mut echo_env,
                &mut echo_baseline,
                WiringTopology::WellMixed,
                horizon,
                500,
                50_000,
                gt.epsilon_machine.as_ref(),
                None,
            );
            if let Some(r) = echo_score.paired_regret_bits_per_symbol {
                echo_regrets.push(r);
            }
        }

        if regret_pass < min_pass {
            failures.push(format!(
                "G1B.2b: {label} regret <= 0.025 on only {regret_pass}/{seeds} seeds (required {min_pass})"
            ));
        }

        if !arkhe_regrets.is_empty() && !echo_regrets.is_empty() {
            let mean_arkhe: f64 = arkhe_regrets.iter().sum::<f64>() / (arkhe_regrets.len() as f64);
            let mean_echo: f64 = echo_regrets.iter().sum::<f64>() / (echo_regrets.len() as f64);
            if mean_arkhe >= mean_echo {
                failures.push(format!(
                    "G1B.2b: {label} mean regret ({mean_arkhe:.4}) not strictly lower than reactive echo ({mean_echo:.4})"
                ));
            }
        }
    }

    (failures.is_empty(), cases)
}

/// Gate G1B.3: Live invariant, phase, & budget integrity across all online runs + 500-edit stress battery.
fn verify_gate_g1b3(online_audits: &[OnlineRunAudit], failures: &mut Vec<String>) -> (bool, usize) {
    for audit in online_audits {
        if !audit.circuit_compiles {
            failures.push(format!(
                "G1B.3a: final circuit violated §2.5 compile invariants on {}",
                audit.label
            ));
        }
        if !audit.budget_respected {
            failures.push(format!(
                "G1B.3a: candidate_replay_ticks_consumed exceeded max_eval_ticks on {}",
                audit.label
            ));
        }
        if !audit.max_nodes_respected {
            failures.push(format!(
                "G1B.3a: final_node_count exceeded max_nodes on {}",
                audit.label
            ));
        }
    }

    let mut stress_cases = 0usize;
    for width in [Width::BITS_1, Width::BITS_2] {
        for delay_mode in [WireDelayMode::Combinational, WireDelayMode::TicksPerHop] {
            stress_cases += 1;
            let shape = GridShape::new(8, 8).unwrap();
            let mut lattice = LatticeCircuit::new(
                shape,
                width,
                delay_mode,
                WiringTopology::LocalVonNeumann,
                vec![BoundaryAnchor {
                    side: PerimeterSide::North,
                    offset: 3,
                }],
                vec![BoundaryAnchor {
                    side: PerimeterSide::North,
                    offset: 4,
                }],
            )
            .unwrap();

            let out_cell = Coord { row: 0, col: 4 };
            let split = lattice.place_node(out_cell, Primitive::Split).unwrap();
            lattice
                .route_connect(
                    SignalSource::NodeOut {
                        node: split,
                        port: 1,
                    },
                    SignalSink::BoundaryOut(0),
                    4,
                )
                .unwrap();

            let template = if width.bits() == 1 {
                [0, 1, 1]
            } else {
                [0, 1, 2]
            };

            let d0 = lattice
                .place_node(
                    Coord { row: 1, col: 4 },
                    Primitive::Delay { init: template[2] },
                )
                .unwrap();
            let d1 = lattice
                .place_node(
                    Coord { row: 2, col: 4 },
                    Primitive::Delay { init: template[1] },
                )
                .unwrap();
            let d2 = lattice
                .place_node(
                    Coord { row: 3, col: 4 },
                    Primitive::Delay { init: template[0] },
                )
                .unwrap();

            lattice
                .route_connect(
                    SignalSource::NodeOut { node: d0, port: 0 },
                    SignalSink::NodeIn { node: d1, port: 0 },
                    4,
                )
                .unwrap();
            lattice
                .route_connect(
                    SignalSource::NodeOut { node: d1, port: 0 },
                    SignalSink::NodeIn { node: d2, port: 0 },
                    4,
                )
                .unwrap();
            lattice
                .route_connect(
                    SignalSource::NodeOut { node: d2, port: 0 },
                    SignalSink::NodeIn {
                        node: split,
                        port: 0,
                    },
                    4,
                )
                .unwrap();
            lattice
                .route_connect(
                    SignalSource::NodeOut {
                        node: split,
                        port: 0,
                    },
                    SignalSink::NodeIn { node: d0, port: 0 },
                    4,
                )
                .unwrap();

            let mut editable =
                EditableLattice::new(lattice.clone(), EconomyConfig::default()).unwrap();
            let mut ref_editable = EditableLattice::new(lattice, EconomyConfig::default()).unwrap();

            let zeros = [0u64];
            let mut pred = [0u64];
            let mut ref_pred = [0u64];
            let mut phase_slips = 0u32;

            for step in 0..500u64 {
                if ref_editable.step(&zeros, &mut ref_pred).is_err() {
                    failures.push(format!(
                        "G1B.3b: ref_step failed at edit {step} on width {width:?} {delay_mode:?}"
                    ));
                    break;
                }

                if editable.step(&zeros, &mut pred).is_err() {
                    failures.push(format!(
                        "G1B.3b: step failed at edit {step} on width {width:?} {delay_mode:?}"
                    ));
                    break;
                }
                if pred[0] != ref_pred[0] {
                    phase_slips += 1;
                }

                let sec_cell = Coord {
                    row: 7,
                    col: (step % 8) as u16,
                };
                let mut staged = editable.lattice().clone();
                if let Ok(node) = staged.place_node(sec_cell, Primitive::Eqz) {
                    let old_to_new: Vec<Option<crate::substrate::NodeId>> =
                        (0..editable.circuit().node_count())
                            .map(|i| Some(crate::substrate::NodeId(i as u32)))
                            .collect();
                    if editable
                        .recompile_preserving_state(staged, &old_to_new)
                        .is_ok()
                    {
                        if editable.circuit().compile().is_err() {
                            failures.push(format!(
                                "G1B.3b: post-edit circuit failed compile at edit {step}"
                            ));
                            break;
                        }
                        let mut idle = std::collections::BTreeSet::new();
                        idle.insert(node);
                        let _ = editable.prune_zero_sensitivity_mature_nodes(
                            &idle,
                            &[vec![0]],
                            &[vec![ref_pred[0]]],
                            true,
                        );
                        if editable.circuit().compile().is_err() {
                            failures.push(format!(
                                "G1B.3b: post-prune circuit failed compile at edit {step}"
                            ));
                            break;
                        }
                    }
                }
            }

            if phase_slips > 0 {
                failures.push(format!(
                    "G1B.3b: recorded {phase_slips} phase slips across 500 edits on width {width:?} {delay_mode:?}"
                ));
            }
        }
    }

    (failures.is_empty(), online_audits.len() + stress_cases)
}

fn build_seeded_autonomous_ring(width: Width, template: &[u64]) -> LatticeCircuit {
    let shape = GridShape::new(8, 8).unwrap();
    let mut lat = LatticeCircuit::new(
        shape,
        width,
        WireDelayMode::Combinational,
        WiringTopology::WellMixed,
        vec![BoundaryAnchor {
            side: PerimeterSide::North,
            offset: 3,
        }],
        vec![BoundaryAnchor {
            side: PerimeterSide::North,
            offset: 4,
        }],
    )
    .unwrap();
    let period = template.len();
    let mut delays = Vec::with_capacity(period);
    for j in 0..period {
        let init = template[(period - 1 - j) % period];
        let cell = lat.nearest_empty_cell(Coord { row: 0, col: 0 }).unwrap();
        delays.push(lat.place_node(cell, Primitive::Delay { init }).unwrap());
    }
    let split_cell = lat.nearest_empty_cell(Coord { row: 0, col: 0 }).unwrap();
    let split = lat.place_node(split_cell, Primitive::Split).unwrap();
    for i in 0..period.saturating_sub(1) {
        lat.connect(
            SignalSource::NodeOut {
                node: delays[i],
                port: 0,
            },
            SignalSink::NodeIn {
                node: delays[i + 1],
                port: 0,
            },
        )
        .unwrap();
    }
    if let Some(&last_d) = delays.last() {
        lat.connect(
            SignalSource::NodeOut {
                node: last_d,
                port: 0,
            },
            SignalSink::NodeIn {
                node: split,
                port: 0,
            },
        )
        .unwrap();
        lat.connect(
            SignalSource::NodeOut {
                node: split,
                port: 0,
            },
            SignalSink::NodeIn {
                node: delays[0],
                port: 0,
            },
        )
        .unwrap();
    }
    lat.connect(
        SignalSource::NodeOut {
            node: split,
            port: 1,
        },
        SignalSink::BoundaryOut(0),
    )
    .unwrap();
    lat
}

/// Gate G1B.4: Charter §8 M1 ablations across seeds & subprocess reproducibility.
fn verify_gate_g1b4(
    seed_offset: u32,
    seeds: u32,
    horizon: u64,
    audits: &mut Vec<OnlineRunAudit>,
    failures: &mut Vec<String>,
) -> (bool, usize) {
    let min_pass = ((seeds as f64) * 0.90).ceil() as u32;
    let shape = GridShape::new(8, 8).unwrap();
    let mut cases = 0usize;

    // Axis 1: All 4 RewardModes across T1_Constant(0) and sparse T1_PeriodicMotif([0,0,0,0,0,1]),
    // plus online rare-event autonomous ring discovery on T1_PeriodicMotif([0,0,0,0,1]).
    for width in [Width::BITS_1, Width::BITS_2] {
        let const_loop_zero = build_seeded_autonomous_ring(width, &[0]);
        let osc6 = build_seeded_autonomous_ring(width, &[0, 0, 0, 0, 0, 1]);

        for (task_label, gen_cfg) in [
            ("Constant(0)", GeneratorConfig::T1_Constant { value: 0 }),
            (
                "SparseMotif6",
                GeneratorConfig::T1_PeriodicMotif {
                    motif: vec![0, 0, 0, 0, 0, 1],
                },
            ),
        ] {
            for mode in [
                RewardMode::CalibratedPointForecast,
                RewardMode::CenteredSurprisal,
                RewardMode::UncenteredSurprisal,
                RewardMode::RawAccuracy,
            ] {
                for seed_idx in 0..seeds {
                    cases += 1;
                    let master_seed = u64::from(seed_offset + seed_idx);
                    let seeds_bank = SeedBank::new(master_seed);
                    let econ = EconomyConfig {
                        reward_mode: mode,
                        ..EconomyConfig::default()
                    };
                    let generator = gen_cfg.build(width).unwrap();
                    let mut env =
                        NoisyEnvironment::new(generator, NoiseModel::None, seeds_bank).unwrap();
                    let mut learner = ArkheLearner::new_with_spatial(
                        width,
                        1,
                        WireDelayMode::Combinational,
                        WiringTopology::WellMixed,
                        shape,
                        4,
                        econ,
                        GrowthConfig {
                            max_nodes: 2,
                            ..GrowthConfig::default()
                        },
                        seeds_bank,
                    )
                    .unwrap();
                    *learner.editable_mut() =
                        EditableLattice::new(const_loop_zero.clone(), econ).unwrap();

                    let scorecard = run_prequential_harness(
                        &mut env,
                        &mut learner,
                        WiringTopology::WellMixed,
                        horizon.min(400),
                        100,
                        20_000,
                        None,
                        None,
                    );
                    audits.push(OnlineRunAudit::record(
                        format!("G1B.4a:Axis1:{task_label}:{mode:?}:seed={master_seed}"),
                        &learner,
                        &scorecard,
                        20_000,
                    ));
                    let n_final = learner.editable().circuit().node_count();
                    match mode {
                        RewardMode::CalibratedPointForecast => {
                            if n_final != 0 {
                                failures.push(format!(
                                    "G1B.4a (Axis 1): CalibratedPointForecast failed to starve ConstLoop(0) on {task_label} width {width:?} seed {master_seed}"
                                ));
                            }
                        }
                        RewardMode::CenteredSurprisal => {
                            if n_final != 0 {
                                failures.push(format!(
                                    "G1B.4a (Axis 1): CenteredSurprisal failed to starve ConstLoop(0) on {task_label} width {width:?}"
                                ));
                            }
                        }
                        RewardMode::UncenteredSurprisal => {
                            if task_label == "SparseMotif6" && n_final == 0 {
                                failures.push(format!(
                                    "G1B.4a (Axis 1): UncenteredSurprisal unexpectedly starved lazy ConstLoop(0) on SparseMotif6 width {width:?}"
                                ));
                            }
                        }
                        RewardMode::RawAccuracy => {
                            if n_final == 0 {
                                failures.push(format!(
                                    "G1B.4a (Axis 1): RawAccuracy unexpectedly starved ConstLoop(0) on {task_label} width {width:?}"
                                ));
                            }
                        }
                    }
                }
            }
        }

        // Seeded mature Osc6([0,0,0,0,0,1]) survives & locks phase under CalibratedPointForecast
        for seed_idx in 0..seeds {
            cases += 1;
            let master_seed = u64::from(seed_offset + seed_idx);
            let seeds_bank = SeedBank::new(master_seed);
            let econ = EconomyConfig::default();
            let gen_osc6 = GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 0, 0, 0, 0, 1],
            }
            .build(width)
            .unwrap();
            let mut env_osc6 =
                NoisyEnvironment::new(gen_osc6, NoiseModel::None, seeds_bank).unwrap();
            let mut learner_osc6 = ArkheLearner::new_with_spatial(
                width,
                1,
                WireDelayMode::Combinational,
                WiringTopology::WellMixed,
                shape,
                4,
                econ,
                GrowthConfig::default(),
                seeds_bank,
            )
            .unwrap();
            *learner_osc6.editable_mut() = EditableLattice::new(osc6.clone(), econ).unwrap();
            let n_osc6 = learner_osc6.editable().circuit().node_count();
            learner_osc6.ledger_mut().node_debts = vec![0.0; n_osc6];
            learner_osc6.ledger_mut().debt_paid = vec![true; n_osc6];
            let score_osc6 = run_prequential_harness(
                &mut env_osc6,
                &mut learner_osc6,
                WiringTopology::WellMixed,
                horizon.min(500),
                100,
                20_000,
                None,
                None,
            );
            audits.push(OnlineRunAudit::record(
                format!(
                    "G1B.4a:Axis1:Osc6Mature:k={}:seed={master_seed}",
                    width.bits()
                ),
                &learner_osc6,
                &score_osc6,
                20_000,
            ));
            if score_osc6.post_transient_clean_mismatches != 0
                || learner_osc6.starvation_prune_events() != 0
            {
                failures.push(format!(
                    "G1B.4a (Axis 1): mature Osc6 failed rare-event survival on width {width:?}"
                ));
            }
        }

        // Online rare-event ring discovery from empty graph on T1_PeriodicMotif([0,0,0,0,1])
        let mut rare_ring_pass = 0u32;
        for seed_idx in 0..seeds {
            cases += 1;
            let master_seed = u64::from(seed_offset + seed_idx);
            let seeds_bank = SeedBank::new(master_seed);
            let gen_rare = GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 0, 0, 0, 1],
            }
            .build(width)
            .unwrap();
            let mut env_rare =
                NoisyEnvironment::new(gen_rare, NoiseModel::None, seeds_bank).unwrap();
            let mut learner_rare = ArkheLearner::new_with_spatial(
                width,
                1,
                WireDelayMode::Combinational,
                WiringTopology::WellMixed,
                shape,
                4,
                EconomyConfig::default(),
                GrowthConfig::default(),
                seeds_bank,
            )
            .unwrap();
            let score_rare = run_prequential_harness(
                &mut env_rare,
                &mut learner_rare,
                WiringTopology::WellMixed,
                horizon,
                500,
                50_000,
                None,
                None,
            );
            audits.push(OnlineRunAudit::record(
                format!(
                    "G1B.4a:Axis1:OnlineRareMotif5:k={}:seed={master_seed}",
                    width.bits()
                ),
                &learner_rare,
                &score_rare,
                50_000,
            ));
            if learner_rare.ring_closure_events() >= 1
                && learner_rare.starvation_prune_events() == 0
                && score_rare.post_transient_clean_mismatches == 0
            {
                rare_ring_pass += 1;
            }
        }
        if rare_ring_pass < min_pass {
            failures.push(format!(
                "G1B.4a (Axis 1): online rare-event Motif5 ring passed on only {rare_ring_pass}/{seeds} seeds (width {width:?})"
            ));
        }
    }

    // Axis 2: SPLIT fan-out tax (split_code_tax: true vs false) across multi-stage fan-out tasks
    let fanout_tasks = [
        (
            Width::BITS_1,
            WiringTopology::WellMixed,
            GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 1, 1],
            },
            "WellMixed:Motif011(k=1)",
        ),
        (
            Width::BITS_2,
            WiringTopology::WellMixed,
            GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 1, 3],
            },
            "WellMixed:Motif013(k=2)",
        ),
        (
            Width::BITS_2,
            WiringTopology::WellMixed,
            GeneratorConfig::T1_AffineLag1 {
                mul_a: 1,
                add_b: 1,
                init: 0,
            },
            "WellMixed:AffineLag1(k=2)",
        ),
        (
            Width::BITS_2,
            WiringTopology::WellMixed,
            GeneratorConfig::T3b_CounterModN {
                modulus: 3,
                init: 0,
            },
            "WellMixed:CounterModN3(k=2)",
        ),
        (
            Width::BITS_2,
            WiringTopology::LocalVonNeumann,
            GeneratorConfig::T1_AffineLag1 {
                mul_a: 1,
                add_b: 1,
                init: 0,
            },
            "Local:AffineLag1(k=2)",
        ),
    ];

    for (width, topology, task_cfg, task_name) in fanout_tasks {
        let mut total_splits_tax = 0usize;
        let mut total_splits_notax = 0usize;
        let mut acc_sum_tax = 0.0;
        let mut acc_sum_notax = 0.0;

        for seed_idx in 0..seeds {
            cases += 2;
            let master_seed = u64::from(seed_offset + seed_idx);
            let seeds_bank = SeedBank::new(master_seed);

            let gen_tax = task_cfg.build(width).unwrap();
            let mut env_tax = NoisyEnvironment::new(gen_tax, NoiseModel::None, seeds_bank).unwrap();
            let mut learner_tax = ArkheLearner::new_with_spatial(
                width,
                1,
                WireDelayMode::Combinational,
                topology,
                shape,
                4,
                EconomyConfig {
                    split_code_tax: true,
                    ..EconomyConfig::default()
                },
                GrowthConfig::default(),
                seeds_bank,
            )
            .unwrap();
            let score_tax = run_prequential_harness(
                &mut env_tax,
                &mut learner_tax,
                topology,
                horizon,
                500,
                50_000,
                None,
                None,
            );
            audits.push(OnlineRunAudit::record(
                format!("G1B.4a:Axis2:Tax:{task_name}:seed={master_seed}"),
                &learner_tax,
                &score_tax,
                50_000,
            ));

            let gen_notax = task_cfg.build(width).unwrap();
            let mut env_notax =
                NoisyEnvironment::new(gen_notax, NoiseModel::None, seeds_bank).unwrap();
            let mut learner_notax = ArkheLearner::new_with_spatial(
                width,
                1,
                WireDelayMode::Combinational,
                topology,
                shape,
                4,
                EconomyConfig {
                    split_code_tax: false,
                    ..EconomyConfig::default()
                },
                GrowthConfig::default(),
                seeds_bank,
            )
            .unwrap();
            let score_notax = run_prequential_harness(
                &mut env_notax,
                &mut learner_notax,
                topology,
                horizon,
                500,
                50_000,
                None,
                None,
            );
            audits.push(OnlineRunAudit::record(
                format!("G1B.4a:Axis2:NoTax:{task_name}:seed={master_seed}"),
                &learner_notax,
                &score_notax,
                50_000,
            ));

            total_splits_tax += learner_tax
                .editable()
                .circuit()
                .nodes()
                .iter()
                .filter(|&&p| p == Primitive::Split)
                .count();
            total_splits_notax += learner_notax
                .editable()
                .circuit()
                .nodes()
                .iter()
                .filter(|&&p| p == Primitive::Split)
                .count();
            acc_sum_tax += score_tax.post_transient_clean_match_rate;
            acc_sum_notax += score_notax.post_transient_clean_match_rate;
        }

        if total_splits_tax > total_splits_notax {
            failures.push(format!(
                "G1B.4a (Axis 2): {task_name} split_code_tax:true total splits={total_splits_tax} > split_code_tax:false total splits={total_splits_notax}"
            ));
        }
        let mean_acc_tax = acc_sum_tax / (seeds as f64);
        let mean_acc_notax = acc_sum_notax / (seeds as f64);
        if (mean_acc_tax - mean_acc_notax).abs() > 0.02 || mean_acc_tax < 0.95 {
            failures.push(format!(
                "G1B.4a (Axis 2): {task_name} accuracy mismatch or degradation (tax={mean_acc_tax:.4}, notax={mean_acc_notax:.4})"
            ));
        }
    }

    // Axis 3: Spatial locality (LocalVonNeumann vs WellMixed) across Alternating and AffineLag1
    let locality_tasks = [
        (
            Width::BITS_1,
            GeneratorConfig::T1_Alternating { a: 0, b: 1 },
            "Alternating(k=1)",
        ),
        (
            Width::BITS_2,
            GeneratorConfig::T1_Alternating { a: 0, b: 3 },
            "Alternating(k=2)",
        ),
        (
            Width::BITS_2,
            GeneratorConfig::T1_AffineLag1 {
                mul_a: 1,
                add_b: 1,
                init: 0,
            },
            "AffineLag1(k=2)",
        ),
    ];

    for (width, task_cfg, task_name) in locality_tasks {
        let mut local_pass = 0u32;
        let mut wm_pass = 0u32;
        let mut total_nodes_local = 0usize;
        let mut total_nodes_wm = 0usize;

        for seed_idx in 0..seeds {
            cases += 2;
            let master_seed = u64::from(seed_offset + seed_idx);
            let seeds_bank = SeedBank::new(master_seed);

            let gen_local = task_cfg.build(width).unwrap();
            let mut env_local =
                NoisyEnvironment::new(gen_local, NoiseModel::None, seeds_bank).unwrap();
            let mut learner_local = ArkheLearner::new_with_spatial(
                width,
                1,
                WireDelayMode::Combinational,
                WiringTopology::LocalVonNeumann,
                shape,
                4,
                EconomyConfig::default(),
                GrowthConfig::default(),
                seeds_bank,
            )
            .unwrap();
            let score_local = run_prequential_harness(
                &mut env_local,
                &mut learner_local,
                WiringTopology::LocalVonNeumann,
                horizon,
                500,
                50_000,
                None,
                None,
            );
            audits.push(OnlineRunAudit::record(
                format!("G1B.4a:Axis3:Local:{task_name}:seed={master_seed}"),
                &learner_local,
                &score_local,
                50_000,
            ));

            let gen_wm = task_cfg.build(width).unwrap();
            let mut env_wm = NoisyEnvironment::new(gen_wm, NoiseModel::None, seeds_bank).unwrap();
            let mut learner_wm = ArkheLearner::new_with_spatial(
                width,
                1,
                WireDelayMode::Combinational,
                WiringTopology::WellMixed,
                shape,
                4,
                EconomyConfig::default(),
                GrowthConfig::default(),
                seeds_bank,
            )
            .unwrap();
            let score_wm = run_prequential_harness(
                &mut env_wm,
                &mut learner_wm,
                WiringTopology::WellMixed,
                horizon,
                500,
                50_000,
                None,
                None,
            );
            audits.push(OnlineRunAudit::record(
                format!("G1B.4a:Axis3:WellMixed:{task_name}:seed={master_seed}"),
                &learner_wm,
                &score_wm,
                50_000,
            ));

            if score_local.post_transient_clean_mismatches == 0 {
                local_pass += 1;
            }
            if score_wm.post_transient_clean_mismatches == 0 {
                wm_pass += 1;
            }
            total_nodes_local += learner_local.editable().circuit().node_count();
            total_nodes_wm += learner_wm.editable().circuit().node_count();
        }

        if local_pass < min_pass || wm_pass < min_pass {
            failures.push(format!(
                "G1B.4a (Axis 3): {task_name} solve rate local={local_pass}/{seeds}, well_mixed={wm_pass}/{seeds} (required {min_pass})"
            ));
        }
        if total_nodes_local < total_nodes_wm {
            failures.push(format!(
                "G1B.4a (Axis 3): {task_name} expected LocalVonNeumann nodes ({total_nodes_local}) >= WellMixed nodes ({total_nodes_wm})"
            ));
        }
    }

    // Axis 4: Wire propagation delay (TicksPerHop across Alternating, PeriodicMotif, and AffineLag1)
    let tph_tasks = [
        (
            Width::BITS_1,
            WiringTopology::LocalVonNeumann,
            GeneratorConfig::T1_Alternating { a: 0, b: 1 },
            "TPH:Local:Alternating(k=1)",
        ),
        (
            Width::BITS_2,
            WiringTopology::LocalVonNeumann,
            GeneratorConfig::T1_Alternating { a: 0, b: 3 },
            "TPH:Local:Alternating(k=2)",
        ),
        (
            Width::BITS_1,
            WiringTopology::WellMixed,
            GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 1, 1],
            },
            "TPH:WellMixed:Motif011(k=1)",
        ),
        (
            Width::BITS_2,
            WiringTopology::WellMixed,
            GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 1, 3],
            },
            "TPH:WellMixed:Motif013(k=2)",
        ),
        (
            Width::BITS_2,
            WiringTopology::LocalVonNeumann,
            GeneratorConfig::T1_AffineLag1 {
                mul_a: 1,
                add_b: 1,
                init: 0,
            },
            "TPH:Local:AffineLag1(k=2)",
        ),
    ];

    for (width, topology, task_cfg, label) in tph_tasks {
        let mut tph_pass = 0u32;
        for seed_idx in 0..seeds {
            cases += 1;
            let master_seed = u64::from(seed_offset + seed_idx);
            let seeds_bank = SeedBank::new(master_seed);
            let gen_tph = task_cfg.build(width).unwrap();
            let mut env_tph = NoisyEnvironment::new(gen_tph, NoiseModel::None, seeds_bank).unwrap();
            let mut learner_tph = ArkheLearner::new_with_spatial(
                width,
                1,
                WireDelayMode::TicksPerHop,
                topology,
                shape,
                4,
                EconomyConfig::default(),
                GrowthConfig::default(),
                seeds_bank,
            )
            .unwrap();
            let score_tph = run_prequential_harness(
                &mut env_tph,
                &mut learner_tph,
                topology,
                horizon,
                500,
                50_000,
                None,
                None,
            );
            audits.push(OnlineRunAudit::record(
                format!("G1B.4a:Axis4:{label}:seed={master_seed}"),
                &learner_tph,
                &score_tph,
                50_000,
            ));
            if score_tph.post_transient_clean_mismatches == 0 {
                tph_pass += 1;
            }
        }
        if tph_pass < min_pass {
            failures.push(format!(
                "G1B.4a (Axis 4): {label} achieved 0 post-transient clean mismatches on only {tph_pass}/{seeds} seeds (required {min_pass})"
            ));
        }
    }

    // (b) Separate-process JSON determinism across structural edits & maturation (horizon=320)
    let Some(arkhe_bin) = locate_arkhe_binary() else {
        failures
            .push("G1B.4b: could not locate arkhe binary for child process determinism".to_owned());
        return (failures.is_empty(), cases);
    };

    let pid = std::process::id();
    let tmp_dir = std::env::temp_dir();

    let child_matrix = [
        (
            Width::BITS_1,
            WiringTopology::WellMixed,
            WireDelayMode::Combinational,
            GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 1, 1],
            },
            NoiseModel::None,
        ),
        (
            Width::BITS_1,
            WiringTopology::LocalVonNeumann,
            WireDelayMode::Combinational,
            GeneratorConfig::T1_Alternating { a: 0, b: 1 },
            NoiseModel::IndependentSubstitution {
                epsilon: 0.05,
                channels: vec![0],
            },
        ),
        (
            Width::BITS_2,
            WiringTopology::WellMixed,
            WireDelayMode::Combinational,
            GeneratorConfig::T1_AffineLag1 {
                mul_a: 1,
                add_b: 1,
                init: 0,
            },
            NoiseModel::None,
        ),
        (
            Width::BITS_2,
            WiringTopology::WellMixed,
            WireDelayMode::TicksPerHop,
            GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 1, 3],
            },
            NoiseModel::None,
        ),
        (
            Width::BITS_2,
            WiringTopology::LocalVonNeumann,
            WireDelayMode::Combinational,
            GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 1, 3],
            },
            NoiseModel::IndependentSubstitution {
                epsilon: 0.05,
                channels: vec![0],
            },
        ),
        (
            Width::BITS_2,
            WiringTopology::LocalVonNeumann,
            WireDelayMode::TicksPerHop,
            GeneratorConfig::T1_AffineLag1 {
                mul_a: 1,
                add_b: 1,
                init: 0,
            },
            NoiseModel::None,
        ),
    ];

    for (c_idx, (width, topology, delay_mode, generator, noise)) in
        child_matrix.into_iter().enumerate()
    {
        cases += 1;
        let seq = TMP_NONCE.fetch_add(1, Ordering::Relaxed);
        let cfg_path = tmp_dir.join(format!("arkhe_g1b4_{pid}_{seq}_{c_idx}.toml"));
        let out1_path = tmp_dir.join(format!("arkhe_g1b4_{pid}_{seq}_{c_idx}_a.json"));
        let out2_path = tmp_dir.join(format!("arkhe_g1b4_{pid}_{seq}_{c_idx}_b.json"));

        let cfg = ExperimentConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            master_seed: u64::from(seed_offset + (c_idx as u32) % seeds),
            substrate: SubstrateConfig {
                width_bits: width,
                wire_delay: delay_mode,
                wiring_topology: topology,
            },
            environment: EnvironmentConfig {
                horizon_ticks: 320,
                transient_warmup_ticks: 80,
                generator,
                noise,
            },
            budget: BudgetConfig {
                max_eval_ticks: 30_000,
            },
            baseline: BaselineConfig::ArkheM1 {
                grid_rows: 8,
                grid_cols: 8,
                max_relay_hops: 4,
                economy: EconomyConfig::default(),
                growth: GrowthConfig::default(),
            },
        };

        let toml_text = toml::to_string_pretty(&cfg).expect("serializes to toml");
        if let Err(e) = fs::write(&cfg_path, &toml_text) {
            failures.push(format!("G1B.4b: failed writing temp config: {e}"));
            continue;
        }

        let st1 = Command::new(&arkhe_bin)
            .args([
                "run",
                "--config",
                cfg_path.to_str().unwrap(),
                "--output",
                out1_path.to_str().unwrap(),
            ])
            .status();
        let st2 = Command::new(&arkhe_bin)
            .args([
                "run",
                "--config",
                cfg_path.to_str().unwrap(),
                "--output",
                out2_path.to_str().unwrap(),
            ])
            .status();

        match (st1, st2) {
            (Ok(s1), Ok(s2)) if s1.success() && s2.success() => {
                let bytes1 = fs::read(&out1_path).unwrap_or_default();
                let bytes2 = fs::read(&out2_path).unwrap_or_default();
                if bytes1.is_empty() || bytes1 != bytes2 {
                    failures.push(format!(
                        "G1B.4b (case {c_idx}): separate processes produced differing JSON"
                    ));
                }
            }
            _ => {
                failures.push(format!(
                    "G1B.4b (case {c_idx}): child process execution failed"
                ));
            }
        }

        let _ = fs::remove_file(cfg_path);
        let _ = fs::remove_file(out1_path);
        let _ = fs::remove_file(out2_path);
    }

    (failures.is_empty(), cases)
}
