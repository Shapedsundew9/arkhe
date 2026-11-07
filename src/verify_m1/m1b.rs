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
use crate::rng::SeedBank;
use crate::structural::economy::{EconomyConfig, RewardMode};
use crate::structural::growth::GrowthConfig;
use crate::structural::lattice::{
    BoundaryAnchor, Coord, GridShape, LatticeCircuit, PerimeterSide, WiringTopology,
};
use crate::structural::learner::ArkheLearner;
use crate::structural::surgery::EditableLattice;
use crate::substrate::{Primitive, SignalSink, SignalSource, Width, WireDelayMode};
use crate::verify_m1::M1aVerificationSummary;

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

/// Runs the complete M1b verification suite across gates G1B.1 through G1B.4.
#[must_use]
pub fn run_m1b_verification_suite(
    seed_offset: u32,
    seeds: u32,
    horizon: u64,
) -> M1bVerificationSummary {
    let mut gates = Vec::new();

    // Gate G1B.1
    let mut g1_failures = Vec::new();
    let g1_passed = verify_gate_g1b1(seed_offset, seeds, horizon, &mut g1_failures);
    gates.push(M1bGateReport {
        gate: "G1B.1".to_owned(),
        passed: g1_passed,
        cases: 4 * (seeds as usize),
        failures: g1_failures,
    });

    // Gate G1B.2
    let mut g2_failures = Vec::new();
    let g2_passed = verify_gate_g1b2(seed_offset, seeds, horizon, &mut g2_failures);
    gates.push(M1bGateReport {
        gate: "G1B.2".to_owned(),
        passed: g2_passed,
        cases: 10 * (seeds as usize),
        failures: g2_failures,
    });

    // Gate G1B.3
    let mut g3_failures = Vec::new();
    let g3_passed = verify_gate_g1b3(seed_offset, seeds, horizon, &mut g3_failures);
    gates.push(M1bGateReport {
        gate: "G1B.3".to_owned(),
        passed: g3_passed,
        cases: 4,
        failures: g3_failures,
    });

    // Gate G1B.4
    let mut g4_failures = Vec::new();
    let g4_passed = verify_gate_g1b4(seed_offset, seeds, horizon, &mut g4_failures);
    gates.push(M1bGateReport {
        gate: "G1B.4".to_owned(),
        passed: g4_passed,
        cases: 10,
        failures: g4_failures,
    });

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
    failures: &mut Vec<String>,
) -> bool {
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

    for (width, gen_cfg, task_name) in tasks {
        let mut seeds_passed = 0u32;

        for seed_idx in 0..seeds {
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

            let _scorecard = run_prequential_harness(
                &mut env,
                &mut learner,
                WiringTopology::WellMixed,
                horizon,
                500,
                50_000,
                None,
                None,
            );

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

    failures.is_empty()
}

/// Gate G1B.2: T1 suite convergence & noise-floor halting.
fn verify_gate_g1b2(
    seed_offset: u32,
    seeds: u32,
    horizon: u64,
    failures: &mut Vec<String>,
) -> bool {
    let min_pass = ((seeds as f64) * 0.90).ceil() as u32;

    // (a) Noiseless cases
    // 1. Silent/constant streams: final_node_count == 0 on 100% of runs
    for width in [Width::BITS_1, Width::BITS_2] {
        let max_val = width.mask();
        for &val in &[0, max_val] {
            let gen_cfg = GeneratorConfig::T1_Constant { value: val };
            let mut zero_count = 0u32;
            for seed_idx in 0..seeds {
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
                run_prequential_harness(
                    &mut env,
                    &mut learner,
                    WiringTopology::WellMixed,
                    horizon,
                    500,
                    50_000,
                    None,
                    None,
                );
                if learner.editable().circuit().node_count() == 0 {
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

    // 2. Structured noiseless streams: post_transient_clean_mismatches == 0 on >= 90% seeds
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

    for width in [Width::BITS_1, Width::BITS_2] {
        let alt_cfg = GeneratorConfig::T1_Alternating {
            a: 0,
            b: width.mask(),
        };
        let mut regret_pass = 0u32;
        let mut arkhe_regrets = Vec::new();
        let mut echo_regrets = Vec::new();

        for seed_idx in 0..seeds {
            let master_seed = u64::from(seed_offset + seed_idx);
            let seeds_bank = SeedBank::new(master_seed);
            let generator = alt_cfg.build(width).unwrap();
            let gt = compute_ground_truth_yardstick(
                &alt_cfg,
                width,
                WireDelayMode::Combinational,
                &noise,
                512,
                Some(3),
            )
            .unwrap();
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
                gt.exact_synthesis_min_nodes,
            );

            if let Some(regret) = scorecard.paired_regret_bits_per_symbol {
                if regret <= 0.025 {
                    regret_pass += 1;
                }
                arkhe_regrets.push(regret);
            }
            if learner.editable().circuit().node_count() > 32 {
                failures.push(format!(
                    "G1B.2b: node count exceeded max_nodes=32 on seed {seed_idx}"
                ));
            }

            // Fixed 1-lag reactive echo predictor on identical stream
            let echo_seed_bank = SeedBank::new(master_seed);
            let echo_gen = alt_cfg.build(width).unwrap();
            let mut echo_env =
                NoisyEnvironment::new(echo_gen, noise.clone(), echo_seed_bank).unwrap();
            let mut echo_circ = LatticeCircuit::new(
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
            let eqz = echo_circ
                .place_node(Coord { row: 0, col: 0 }, Primitive::Eqz)
                .unwrap();
            echo_circ
                .connect(
                    SignalSource::BoundaryIn(0),
                    SignalSink::NodeIn { node: eqz, port: 0 },
                )
                .unwrap();
            echo_circ
                .connect(
                    SignalSource::NodeOut { node: eqz, port: 0 },
                    SignalSink::BoundaryOut(0),
                )
                .unwrap();
            let echo_editable = EditableLattice::new(echo_circ, EconomyConfig::default()).unwrap();
            let mut echo_learner = ArkheLearner::new_with_spatial(
                width,
                1,
                WireDelayMode::Combinational,
                WiringTopology::WellMixed,
                GridShape::new(8, 8).unwrap(),
                4,
                EconomyConfig {
                    juvenile_ticks: 0,
                    ..EconomyConfig::default()
                },
                GrowthConfig {
                    max_nodes: 1,
                    ..GrowthConfig::default()
                },
                echo_seed_bank,
            )
            .unwrap();
            *echo_learner.editable_mut() = echo_editable;
            let n_echo = echo_learner.editable().circuit().node_count();
            echo_learner.ledger_mut().debt_paid = vec![true; n_echo];
            echo_learner.ledger_mut().node_debts = vec![0.0; n_echo];
            let echo_score = run_prequential_harness(
                &mut echo_env,
                &mut echo_learner,
                WiringTopology::WellMixed,
                horizon,
                500,
                50_000,
                gt.epsilon_machine.as_ref(),
                gt.exact_synthesis_min_nodes,
            );
            if let Some(r) = echo_score.paired_regret_bits_per_symbol {
                echo_regrets.push(r);
            }
        }

        if regret_pass < min_pass {
            failures.push(format!(
                "G1B.2b: noisy Alternating width {width:?} regret <= 0.025 on only {regret_pass}/{seeds} seeds (required {min_pass})"
            ));
        }

        if !arkhe_regrets.is_empty() && !echo_regrets.is_empty() {
            let mean_arkhe: f64 = arkhe_regrets.iter().sum::<f64>() / (arkhe_regrets.len() as f64);
            let mean_echo: f64 = echo_regrets.iter().sum::<f64>() / (echo_regrets.len() as f64);
            if mean_arkhe >= mean_echo {
                failures.push(format!(
                    "G1B.2b: mean regret ({mean_arkhe:.4}) not strictly lower than reactive echo ({mean_echo:.4}) on width {width:?}"
                ));
            }
        }
    }

    failures.is_empty()
}

/// Gate G1B.3: Live invariant, phase, & budget integrity.
fn verify_gate_g1b3(
    _seed_offset: u32,
    _seeds: u32,
    _horizon: u64,
    failures: &mut Vec<String>,
) -> bool {
    // Forced 500-edit phase stress battery across k in {1, 2} and {Combinational, TicksPerHop}
    for width in [Width::BITS_1, Width::BITS_2] {
        for delay_mode in [WireDelayMode::Combinational, WireDelayMode::TicksPerHop] {
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

            // Construct 3-stage autonomous oscillator driving BoundaryOut(0)
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
                // Live prediction step for reference clean oscillator
                if ref_editable.step(&zeros, &mut ref_pred).is_err() {
                    failures.push(format!(
                        "G1B.3b: ref_step failed at edit {step} on width {width:?} {delay_mode:?}"
                    ));
                    break;
                }

                // Live prediction step for mutated circuit
                if editable.step(&zeros, &mut pred).is_err() {
                    failures.push(format!(
                        "G1B.3b: step failed at edit {step} on width {width:?} {delay_mode:?}"
                    ));
                    break;
                }
                // Verify output follows clean Osc3 trajectory without phase slips
                if pred[0] != ref_pred[0] {
                    phase_slips += 1;
                }

                // Execute deterministic transaction on secondary cell
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
                        // Immediately starve and excise the temporary node
                        let mut idle = std::collections::BTreeSet::new();
                        idle.insert(node);
                        let _ = editable.prune_zero_sensitivity_mature_nodes(
                            &idle,
                            &[vec![0]],
                            &[vec![ref_pred[0]]],
                            true,
                        );
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

    failures.is_empty()
}

/// Gate G1B.4: Charter §8 M1 ablations & subprocess reproducibility.
fn verify_gate_g1b4(
    seed_offset: u32,
    _seeds: u32,
    horizon: u64,
    failures: &mut Vec<String>,
) -> bool {
    // (a) Four-axis ablation matrix
    // Axis 1: Reward weighting (CalibratedPointForecast vs RawAccuracy) on T1_Constant(0)
    for width in [Width::BITS_1, Width::BITS_2] {
        let gen_cfg = GeneratorConfig::T1_Constant { value: 0 };
        let seeds_bank = SeedBank::new(u64::from(seed_offset));
        let generator1 = gen_cfg.build(width).unwrap();
        let mut env1 = NoisyEnvironment::new(generator1, NoiseModel::None, seeds_bank).unwrap();

        // Seeded ConstLoop(0) under CalibratedPointForecast
        let shape = GridShape::new(8, 8).unwrap();
        let mut lat1 = LatticeCircuit::new(
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
        let d = lat1
            .place_node(Coord { row: 0, col: 0 }, Primitive::Delay { init: 0 })
            .unwrap();
        let s = lat1
            .place_node(Coord { row: 0, col: 1 }, Primitive::Split)
            .unwrap();
        lat1.connect(
            SignalSource::NodeOut { node: d, port: 0 },
            SignalSink::NodeIn { node: s, port: 0 },
        )
        .unwrap();
        lat1.connect(
            SignalSource::NodeOut { node: s, port: 0 },
            SignalSink::NodeIn { node: d, port: 0 },
        )
        .unwrap();
        lat1.connect(
            SignalSource::NodeOut { node: s, port: 1 },
            SignalSink::BoundaryOut(0),
        )
        .unwrap();

        let mut learner_cal = ArkheLearner::new_with_spatial(
            width,
            1,
            WireDelayMode::Combinational,
            WiringTopology::WellMixed,
            shape,
            4,
            EconomyConfig {
                reward_mode: RewardMode::CalibratedPointForecast,
                ..EconomyConfig::default()
            },
            GrowthConfig::default(),
            seeds_bank,
        )
        .unwrap();
        *learner_cal.editable_mut() = EditableLattice::new(
            lat1.clone(),
            EconomyConfig {
                reward_mode: RewardMode::CalibratedPointForecast,
                ..EconomyConfig::default()
            },
        )
        .unwrap();

        run_prequential_harness(
            &mut env1,
            &mut learner_cal,
            WiringTopology::WellMixed,
            horizon.min(500),
            100,
            20_000,
            None,
            None,
        );

        let generator2 = gen_cfg.build(width).unwrap();
        let mut env2 = NoisyEnvironment::new(generator2, NoiseModel::None, seeds_bank).unwrap();
        let mut learner_raw = ArkheLearner::new_with_spatial(
            width,
            1,
            WireDelayMode::Combinational,
            WiringTopology::WellMixed,
            shape,
            4,
            EconomyConfig {
                reward_mode: RewardMode::RawAccuracy,
                ..EconomyConfig::default()
            },
            GrowthConfig::default(),
            seeds_bank,
        )
        .unwrap();
        *learner_raw.editable_mut() = EditableLattice::new(
            lat1,
            EconomyConfig {
                reward_mode: RewardMode::RawAccuracy,
                ..EconomyConfig::default()
            },
        )
        .unwrap();

        run_prequential_harness(
            &mut env2,
            &mut learner_raw,
            WiringTopology::WellMixed,
            horizon.min(500),
            100,
            20_000,
            None,
            None,
        );

        let n_cal = learner_cal.editable().circuit().node_count();
        let n_raw = learner_raw.editable().circuit().node_count();
        if n_cal != 0 || n_raw == 0 {
            failures.push(format!(
                "G1B.4a (Axis 1): CalibratedPointForecast nodes={n_cal} (expected 0) vs RawAccuracy nodes={n_raw} (expected >0) on width {width:?}"
            ));
        }
    }

    // Axis 2: SPLIT fan-out tax (split_code_tax: true vs false)
    let alt_cfg = GeneratorConfig::T1_Alternating { a: 0, b: 1 };
    let seeds_bank = SeedBank::new(u64::from(seed_offset));
    let generator_tax = alt_cfg.build(Width::BITS_1).unwrap();
    let mut env_tax = NoisyEnvironment::new(generator_tax, NoiseModel::None, seeds_bank).unwrap();
    let mut learner_tax = ArkheLearner::new_with_spatial(
        Width::BITS_1,
        1,
        WireDelayMode::Combinational,
        WiringTopology::LocalVonNeumann,
        GridShape::new(8, 8).unwrap(),
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
        WiringTopology::LocalVonNeumann,
        horizon,
        500,
        50_000,
        None,
        None,
    );

    let generator_notax = alt_cfg.build(Width::BITS_1).unwrap();
    let mut env_notax =
        NoisyEnvironment::new(generator_notax, NoiseModel::None, seeds_bank).unwrap();
    let mut learner_notax = ArkheLearner::new_with_spatial(
        Width::BITS_1,
        1,
        WireDelayMode::Combinational,
        WiringTopology::LocalVonNeumann,
        GridShape::new(8, 8).unwrap(),
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
        WiringTopology::LocalVonNeumann,
        horizon,
        500,
        50_000,
        None,
        None,
    );

    let splits_tax = learner_tax
        .editable()
        .circuit()
        .nodes()
        .iter()
        .filter(|&&p| p == Primitive::Split)
        .count();
    let splits_notax = learner_notax
        .editable()
        .circuit()
        .nodes()
        .iter()
        .filter(|&&p| p == Primitive::Split)
        .count();
    if splits_tax > splits_notax {
        failures.push(format!(
            "G1B.4a (Axis 2): split_code_tax:true splits={splits_tax} > split_code_tax:false splits={splits_notax}"
        ));
    }
    if (score_tax.post_transient_clean_match_rate - score_notax.post_transient_clean_match_rate)
        .abs()
        > 0.02
    {
        failures.push("G1B.4a (Axis 2): accuracy degraded under split tax by > 0.02".to_owned());
    }

    // Axis 3: Spatial locality (LocalVonNeumann vs WellMixed)
    let generator_wm = alt_cfg.build(Width::BITS_1).unwrap();
    let mut env_wm = NoisyEnvironment::new(generator_wm, NoiseModel::None, seeds_bank).unwrap();
    let mut learner_wm = ArkheLearner::new_with_spatial(
        Width::BITS_1,
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
    if score_wm.post_transient_clean_mismatches > 0 {
        failures.push("G1B.4a (Axis 3): WellMixed failed Alternating solve rate".to_owned());
    }
    let n_local = learner_tax.editable().circuit().node_count();
    let n_wm = learner_wm.editable().circuit().node_count();
    if n_local < n_wm {
        failures.push(format!(
            "G1B.4a (Axis 3): expected LocalVonNeumann nodes ({n_local}) >= WellMixed nodes ({n_wm})"
        ));
    }

    // Axis 4: Wire propagation delay (TicksPerHop)
    let generator_tph = alt_cfg.build(Width::BITS_1).unwrap();
    let mut env_tph = NoisyEnvironment::new(generator_tph, NoiseModel::None, seeds_bank).unwrap();
    let mut learner_tph = ArkheLearner::new_with_spatial(
        Width::BITS_1,
        1,
        WireDelayMode::TicksPerHop,
        WiringTopology::LocalVonNeumann,
        GridShape::new(8, 8).unwrap(),
        4,
        EconomyConfig::default(),
        GrowthConfig::default(),
        seeds_bank,
    )
    .unwrap();
    let score_tph = run_prequential_harness(
        &mut env_tph,
        &mut learner_tph,
        WiringTopology::LocalVonNeumann,
        horizon,
        500,
        50_000,
        None,
        None,
    );
    if score_tph.post_transient_clean_mismatches > 0 {
        failures.push(format!(
            "G1B.4a (Axis 4): TicksPerHop recorded {} post-transient mismatches (expected 0)",
            score_tph.post_transient_clean_mismatches
        ));
    }

    // (b) Separate-process JSON determinism
    let Some(arkhe_bin) = locate_arkhe_binary() else {
        failures
            .push("G1B.4b: could not locate arkhe binary for child process determinism".to_owned());
        return failures.is_empty();
    };

    let pid = std::process::id();
    let tmp_dir = std::env::temp_dir();

    let child_matrix = [
        (
            Width::BITS_1,
            WiringTopology::WellMixed,
            WireDelayMode::Combinational,
            NoiseModel::None,
        ),
        (
            Width::BITS_1,
            WiringTopology::LocalVonNeumann,
            WireDelayMode::Combinational,
            NoiseModel::IndependentSubstitution {
                epsilon: 0.05,
                channels: vec![0],
            },
        ),
        (
            Width::BITS_2,
            WiringTopology::WellMixed,
            WireDelayMode::Combinational,
            NoiseModel::None,
        ),
        (
            Width::BITS_2,
            WiringTopology::WellMixed,
            WireDelayMode::TicksPerHop,
            NoiseModel::None,
        ),
        (
            Width::BITS_2,
            WiringTopology::LocalVonNeumann,
            WireDelayMode::Combinational,
            NoiseModel::IndependentSubstitution {
                epsilon: 0.05,
                channels: vec![0],
            },
        ),
        (
            Width::BITS_2,
            WiringTopology::LocalVonNeumann,
            WireDelayMode::TicksPerHop,
            NoiseModel::None,
        ),
    ];

    for (c_idx, (width, topology, delay_mode, noise)) in child_matrix.into_iter().enumerate() {
        let seq = TMP_NONCE.fetch_add(1, Ordering::Relaxed);
        let cfg_path = tmp_dir.join(format!("arkhe_g1b4_{pid}_{seq}_{c_idx}.toml"));
        let out1_path = tmp_dir.join(format!("arkhe_g1b4_{pid}_{seq}_{c_idx}_a.json"));
        let out2_path = tmp_dir.join(format!("arkhe_g1b4_{pid}_{seq}_{c_idx}_b.json"));

        let cfg = ExperimentConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            master_seed: 42 + (c_idx as u64),
            substrate: SubstrateConfig {
                width_bits: width,
                wire_delay: delay_mode,
                wiring_topology: topology,
            },
            environment: EnvironmentConfig {
                horizon_ticks: 120,
                transient_warmup_ticks: 30,
                generator: GeneratorConfig::T1_Alternating {
                    a: 0,
                    b: width.mask(),
                },
                noise,
            },
            budget: BudgetConfig {
                max_eval_ticks: 10_000,
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

    failures.is_empty()
}
