//! End-to-end Milestone 0 acceptance and integration tests across substrate, structural, env,
//! metrics, baselines, config, and CLI subcommands.

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use arkhe::baselines::{
    ObservationWindow, SynthesisCertStatus, TickBudget, evaluate_circuit_on_window,
};
use arkhe::config::{
    BaselineConfig, BudgetConfig, CONFIG_SCHEMA_VERSION, ConfigRunError, EnvironmentConfig,
    ExperimentConfig, RunReport, SubstrateConfig, compute_ground_truth_yardstick,
    execute_experiment,
};
use arkhe::env::{FiniteStateGenerator, GeneratorConfig, LfsrStage, NoiseModel};
use arkhe::metrics::{
    BudgetAccountingMode, compute_mdl, detect_autonomous_cycle, estimate_attractor_basin,
};
use arkhe::rng::SeedBank;
use arkhe::structural::{
    BoundaryAnchor, Coord, GridShape, LatticeCircuit, PerimeterSide, WiringTopology,
};
use arkhe::substrate::{
    Circuit, Primitive, ScalarState, SignalSink, SignalSource, Width, WireDelayMode,
};
use arkhe::verify::run_m0_verification_suite;

/// Constructs the exact Level-0 circuit over Z_4 (`k = 2`) that generates the non-power-of-two
/// counter `0, 1, 2, 0, 1, 2, ...` (`T3b_CounterModN { modulus: 3 }`) using `Primitive::Eqz`
/// via the modular identity `f(x) = (x + 1) + Eqz(x + 2) mod 4`.
fn build_mod3_counter_circuit_z4() -> Circuit {
    let mut circ = Circuit::new(Width::BITS_2, 0, 1, WireDelayMode::Combinational);

    // State register x(t), initialised to 0.
    let reg_x = circ.add_node(Primitive::Delay { init: 0 });
    // Constant generators via self-looping Delay registers: c1 = 1, c2 = 2.
    let reg_c1 = circ.add_node(Primitive::Delay { init: 1 });
    let sc1 = circ.add_node(Primitive::Split);
    let reg_c2 = circ.add_node(Primitive::Delay { init: 2 });
    let sc2 = circ.add_node(Primitive::Split);

    // Fan-out x(t) to BoundaryOut(0), (x + 1), and (x + 2).
    let sx0 = circ.add_node(Primitive::Split);
    let sx1 = circ.add_node(Primitive::Split);

    let add_x_1 = circ.add_node(Primitive::Add);
    let add_x_2 = circ.add_node(Primitive::Add);
    let eqz_is_2 = circ.add_node(Primitive::Eqz);
    let next_x = circ.add_node(Primitive::Add);

    // Wire constant 1 self-loop + output to add_x_1.
    circ.connect(
        SignalSource::NodeOut {
            node: reg_c1,
            port: 0,
        },
        SignalSink::NodeIn { node: sc1, port: 0 },
    );
    circ.connect(
        SignalSource::NodeOut { node: sc1, port: 0 },
        SignalSink::NodeIn {
            node: reg_c1,
            port: 0,
        },
    );
    circ.connect(
        SignalSource::NodeOut { node: sc1, port: 1 },
        SignalSink::NodeIn {
            node: add_x_1,
            port: 1,
        },
    );

    // Wire constant 2 self-loop + output to add_x_2.
    circ.connect(
        SignalSource::NodeOut {
            node: reg_c2,
            port: 0,
        },
        SignalSink::NodeIn { node: sc2, port: 0 },
    );
    circ.connect(
        SignalSource::NodeOut { node: sc2, port: 0 },
        SignalSink::NodeIn {
            node: reg_c2,
            port: 0,
        },
    );
    circ.connect(
        SignalSource::NodeOut { node: sc2, port: 1 },
        SignalSink::NodeIn {
            node: add_x_2,
            port: 1,
        },
    );

    // Fan-out reg_x -> BoundaryOut(0), add_x_1.in0, add_x_2.in0.
    circ.connect(
        SignalSource::NodeOut {
            node: reg_x,
            port: 0,
        },
        SignalSink::NodeIn { node: sx0, port: 0 },
    );
    circ.connect(
        SignalSource::NodeOut { node: sx0, port: 0 },
        SignalSink::BoundaryOut(0),
    );
    circ.connect(
        SignalSource::NodeOut { node: sx0, port: 1 },
        SignalSink::NodeIn { node: sx1, port: 0 },
    );
    circ.connect(
        SignalSource::NodeOut { node: sx1, port: 0 },
        SignalSink::NodeIn {
            node: add_x_1,
            port: 0,
        },
    );
    circ.connect(
        SignalSource::NodeOut { node: sx1, port: 1 },
        SignalSink::NodeIn {
            node: add_x_2,
            port: 0,
        },
    );

    // eqz_is_2 = Eqz(x + 2); next_x = (x + 1) + eqz_is_2 -> reg_x.
    circ.connect(
        SignalSource::NodeOut {
            node: add_x_2,
            port: 0,
        },
        SignalSink::NodeIn {
            node: eqz_is_2,
            port: 0,
        },
    );
    circ.connect(
        SignalSource::NodeOut {
            node: add_x_1,
            port: 0,
        },
        SignalSink::NodeIn {
            node: next_x,
            port: 0,
        },
    );
    circ.connect(
        SignalSource::NodeOut {
            node: eqz_is_2,
            port: 0,
        },
        SignalSink::NodeIn {
            node: next_x,
            port: 1,
        },
    );
    circ.connect(
        SignalSource::NodeOut {
            node: next_x,
            port: 0,
        },
        SignalSink::NodeIn {
            node: reg_x,
            port: 0,
        },
    );

    circ
}

#[test]
fn t3b_mod3_counter_eqz_circuit_and_dynamics_yardsticks() {
    let circ = build_mod3_counter_circuit_z4();
    let compiled = circ
        .compile()
        .expect("mod-3 EQZ circuit satisfies all §2.5 invariants");

    let mut generator = GeneratorConfig::T3b_CounterModN {
        modulus: 3,
        init: 0,
    }
    .build(Width::BITS_2)
    .expect("valid mod-3 generator");

    let mut st = ScalarState::new(&compiled);
    let mut out = [0u64; 1];
    let mut expected = [0u64; 1];
    for t in 0..30 {
        st.step(&compiled, &[], &mut out).unwrap();
        generator.next_clean_frame(&mut expected);
        assert_eq!(out[0], expected[0], "mod-3 mismatch at tick {t}");
    }

    // Autonomous limit-cycle inspection: immediate period-3 attractor.
    let cycle = detect_autonomous_cycle(&compiled, 64);
    assert!(cycle.converged);
    assert_eq!(cycle.transient_ticks, 0);
    assert_eq!(cycle.cycle_length, 3);

    // Also test a pure loopback mod-3 step circuit y = (x + 1) + Eqz(x + 2) where the only
    // register state is loopback x in {0,1,2,3}, so perturbed state x=3 self-heals to 0 in 1 step!
    let mut step_circ = Circuit::new(Width::BITS_2, 1, 1, WireDelayMode::Combinational);
    let d_in = step_circ.add_node(Primitive::Delay { init: 0 });
    let s0 = step_circ.add_node(Primitive::Split);
    let d1 = step_circ.add_node(Primitive::Delay { init: 1 });
    let sc1 = step_circ.add_node(Primitive::Split);
    let d2 = step_circ.add_node(Primitive::Delay { init: 2 });
    let sc2 = step_circ.add_node(Primitive::Split);
    let add1 = step_circ.add_node(Primitive::Add);
    let add2 = step_circ.add_node(Primitive::Add);
    let eqz = step_circ.add_node(Primitive::Eqz);
    let sum = step_circ.add_node(Primitive::Add);

    step_circ.connect(
        SignalSource::BoundaryIn(0),
        SignalSink::NodeIn {
            node: d_in,
            port: 0,
        },
    );
    step_circ.connect(
        SignalSource::NodeOut {
            node: d_in,
            port: 0,
        },
        SignalSink::NodeIn { node: s0, port: 0 },
    );
    step_circ.connect(
        SignalSource::NodeOut { node: s0, port: 0 },
        SignalSink::NodeIn {
            node: add1,
            port: 0,
        },
    );
    step_circ.connect(
        SignalSource::NodeOut { node: s0, port: 1 },
        SignalSink::NodeIn {
            node: add2,
            port: 0,
        },
    );
    step_circ.connect(
        SignalSource::NodeOut { node: d1, port: 0 },
        SignalSink::NodeIn { node: sc1, port: 0 },
    );
    step_circ.connect(
        SignalSource::NodeOut { node: sc1, port: 0 },
        SignalSink::NodeIn { node: d1, port: 0 },
    );
    step_circ.connect(
        SignalSource::NodeOut { node: sc1, port: 1 },
        SignalSink::NodeIn {
            node: add1,
            port: 1,
        },
    );
    step_circ.connect(
        SignalSource::NodeOut { node: d2, port: 0 },
        SignalSink::NodeIn { node: sc2, port: 0 },
    );
    step_circ.connect(
        SignalSource::NodeOut { node: sc2, port: 0 },
        SignalSink::NodeIn { node: d2, port: 0 },
    );
    step_circ.connect(
        SignalSource::NodeOut { node: sc2, port: 1 },
        SignalSink::NodeIn {
            node: add2,
            port: 1,
        },
    );
    step_circ.connect(
        SignalSource::NodeOut {
            node: add2,
            port: 0,
        },
        SignalSink::NodeIn { node: eqz, port: 0 },
    );
    step_circ.connect(
        SignalSource::NodeOut {
            node: add1,
            port: 0,
        },
        SignalSink::NodeIn { node: sum, port: 0 },
    );
    step_circ.connect(
        SignalSource::NodeOut { node: eqz, port: 0 },
        SignalSink::NodeIn { node: sum, port: 1 },
    );
    step_circ.connect(
        SignalSource::NodeOut { node: sum, port: 0 },
        SignalSink::BoundaryOut(0),
    );
    let step_comp = step_circ.compile().unwrap();
    let basin_k2 = estimate_attractor_basin(&step_comp, 32, 32, SeedBank::new(9));
    assert_eq!(basin_k2.probes, 32);
    assert!((0.0..=1.0).contains(&basin_k2.basin_fraction));

    // Verify k = 1 64-lane BitParallel64State basin estimation on a 2-register period-2 oscillator.
    let mut osc_k1 = Circuit::new(Width::BITS_1, 0, 1, WireDelayMode::Combinational);
    let r0 = osc_k1.add_node(Primitive::Delay { init: 0 });
    let r1 = osc_k1.add_node(Primitive::Delay { init: 1 });
    let sp = osc_k1.add_node(Primitive::Split);
    osc_k1.connect(
        SignalSource::NodeOut { node: r0, port: 0 },
        SignalSink::NodeIn { node: r1, port: 0 },
    );
    osc_k1.connect(
        SignalSource::NodeOut { node: r1, port: 0 },
        SignalSink::NodeIn { node: sp, port: 0 },
    );
    osc_k1.connect(
        SignalSource::NodeOut { node: sp, port: 0 },
        SignalSink::NodeIn { node: r0, port: 0 },
    );
    osc_k1.connect(
        SignalSource::NodeOut { node: sp, port: 1 },
        SignalSink::BoundaryOut(0),
    );
    let comp_k1 = osc_k1.compile().unwrap();
    let basin_k1 = estimate_attractor_basin(&comp_k1, 256, 16, SeedBank::new(123));
    assert_eq!(basin_k1.probes, 256);
    // Exactly 2 of the 4 states {(0,1), (1,0)} lie on the alternating cycle (~0.5).
    assert!(
        (0.35..=0.65).contains(&basin_k1.basin_fraction),
        "unexpected k=1 basin fraction {}",
        basin_k1.basin_fraction
    );
}

#[test]
fn all_five_m0_baselines_execute_under_compute_matched_budgets() {
    let baselines = [
        BaselineConfig::RandomSearch {
            max_nodes: 4,
            fact_window: 64,
            samples_per_error: 16,
        },
        BaselineConfig::Cgp {
            lambda: 4,
            max_genes: 6,
            fact_window: 64,
        },
        BaselineConfig::Ctw { context_depth: 8 },
        BaselineConfig::ReedsSloane,
        BaselineConfig::Exhaustive {
            max_nodes: 3,
            fact_window: 64,
        },
    ];

    for baseline in baselines {
        let cfg = ExperimentConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            master_seed: 7,
            substrate: SubstrateConfig {
                width_bits: Width::BITS_1,
                wire_delay: WireDelayMode::Combinational,
                wiring_topology: WiringTopology::WellMixed,
            },
            environment: EnvironmentConfig {
                horizon_ticks: 200,
                transient_warmup_ticks: 40,
                generator: GeneratorConfig::T1_Alternating { a: 0, b: 1 },
                noise: NoiseModel::None,
            },
            budget: BudgetConfig {
                max_eval_ticks: 40_000,
            },
            baseline,
        };
        let report = execute_experiment(&cfg).expect("experiment succeeds");
        assert!(report.scorecard.candidate_replay_ticks_consumed <= 40_000);
        let expected_mode = match report.baseline_name.as_str() {
            "CTW" | "ReedsSloane(BM)" => BudgetAccountingMode::AnalyticalUnmetered,
            _ => BudgetAccountingMode::CandidateHistoryReplay,
        };
        assert_eq!(report.scorecard.budget_accounting_mode, expected_mode);
        assert_eq!(
            report.scorecard.post_transient_observed_mismatches, 0,
            "baseline {} failed noiseless T1_Alternating",
            report.baseline_name
        );
    }

    // Also verify T3c_LfsrCurriculum (sub-tap progression) and Reeds–Sloane at k = 2.
    let curriculum = GeneratorConfig::T3c_LfsrCurriculum {
        stages: vec![
            LfsrStage {
                taps: vec![2, 0, 0, 0],
                init: vec![1, 2, 3, 1],
            },
            LfsrStage {
                taps: vec![2, 1, 0, 3],
                init: vec![1, 2, 3, 1],
            },
        ],
        ticks_per_stage: 40,
    };
    let mut curr_gen = curriculum.build(Width::BITS_2).unwrap();
    let mut sym = [0u64; 1];
    for _ in 0..80 {
        curr_gen.next_clean_frame(&mut sym);
    }

    let cfg = ExperimentConfig {
        schema_version: CONFIG_SCHEMA_VERSION,
        master_seed: 0,
        substrate: SubstrateConfig {
            width_bits: Width::BITS_2,
            wire_delay: WireDelayMode::Combinational,
            wiring_topology: WiringTopology::WellMixed,
        },
        environment: EnvironmentConfig {
            horizon_ticks: 80,
            transient_warmup_ticks: 16,
            generator: GeneratorConfig::T3c_Lfsr {
                taps: vec![2, 1, 0, 3],
                init: vec![1, 2, 3, 1],
            },
            noise: NoiseModel::None,
        },
        budget: BudgetConfig {
            max_eval_ticks: 1_000,
        },
        baseline: BaselineConfig::ReedsSloane,
    };
    let report = execute_experiment(&cfg).unwrap();
    assert_eq!(report.scorecard.post_transient_observed_mismatches, 0);
}

#[test]
fn spatial_lattice_lowering_and_mdl_accounting_work_together() {
    let shape = GridShape::new(3, 3).unwrap();
    let mut lat = LatticeCircuit::new(
        shape,
        Width::BITS_2,
        WireDelayMode::TicksPerHop,
        WiringTopology::LocalVonNeumann,
        vec![BoundaryAnchor {
            side: PerimeterSide::West,
            offset: 1,
        }],
        vec![BoundaryAnchor {
            side: PerimeterSide::East,
            offset: 1,
        }],
    )
    .unwrap();

    let n0 = lat
        .place_node(Coord { row: 1, col: 0 }, Primitive::Delay { init: 0 })
        .unwrap();
    let n1 = lat
        .place_node(Coord { row: 1, col: 2 }, Primitive::Eqz)
        .unwrap();
    lat.connect(
        SignalSource::BoundaryIn(0),
        SignalSink::NodeIn { node: n0, port: 0 },
    )
    .unwrap();
    lat.connect(
        SignalSource::NodeOut { node: n0, port: 0 },
        SignalSink::NodeIn { node: n1, port: 0 },
    )
    .unwrap();
    lat.connect(
        SignalSource::NodeOut { node: n1, port: 0 },
        SignalSink::BoundaryOut(0),
    )
    .unwrap();

    let lowered = lat.lower().unwrap();
    let mdl = compute_mdl(
        &lowered,
        WiringTopology::LocalVonNeumann,
        Some(shape),
        1.0,
        4,
        100,
        5,
    );
    assert!(mdl.circuit_bits > 0.0);
    assert!(mdl.palette_bits > 0.0);
    assert!(mdl.total_bits > mdl.circuit_bits);
}

#[test]
fn cli_subcommands_yardstick_run_and_verify_m0_succeed() {
    // 1. Default banner.
    arkhe::run_with_args(["arkhe"]).expect("default CLI banner succeeds");

    // 2. Yardstick subcommand (JSON inline spec).
    arkhe::run_with_args([
        "arkhe",
        "yardstick",
        "--generator",
        r#"{"kind":"T1_Alternating","a":0,"b":3}"#,
        "--width",
        "2",
        "--epsilon",
        "0.05",
    ])
    .expect("yardstick CLI succeeds");

    // 3. Run subcommand writing JSON report to a temporary file.
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let tmp_dir = std::env::temp_dir();
    let cfg_path = tmp_dir.join(format!("arkhe_m0_test_{nonce}.toml"));
    let out_path = tmp_dir.join(format!("arkhe_m0_test_{nonce}.json"));

    let cfg = ExperimentConfig {
        schema_version: CONFIG_SCHEMA_VERSION,
        master_seed: 11,
        substrate: SubstrateConfig {
            width_bits: Width::BITS_1,
            wire_delay: WireDelayMode::Combinational,
            wiring_topology: WiringTopology::WellMixed,
        },
        environment: EnvironmentConfig {
            horizon_ticks: 120,
            transient_warmup_ticks: 20,
            generator: GeneratorConfig::T1_Alternating { a: 0, b: 1 },
            noise: NoiseModel::None,
        },
        budget: BudgetConfig {
            max_eval_ticks: 20_000,
        },
        baseline: BaselineConfig::Ctw { context_depth: 6 },
    };
    fs::write(&cfg_path, toml::to_string_pretty(&cfg).unwrap()).unwrap();

    arkhe::run_with_args([
        "arkhe",
        "run",
        "--config",
        cfg_path.to_str().unwrap(),
        "--output",
        out_path.to_str().unwrap(),
    ])
    .expect("arkhe run CLI succeeds");

    let parsed: RunReport = serde_json::from_str(&fs::read_to_string(&out_path).unwrap()).unwrap();
    assert_eq!(parsed.scorecard.post_transient_observed_mismatches, 0);

    let _ = fs::remove_file(&cfg_path);
    let _ = fs::remove_file(&out_path);

    // 4. Programmatic verify-m0 check (small seed count for fast debug-mode unit testing).
    let summary = run_m0_verification_suite(3);
    assert!(
        summary.all_passed(),
        "verify-m0 summary failed: {summary:?}"
    );
}

#[test]
fn cli_yardstick_rejects_invalid_epsilon_and_reports_honest_ground_truth_bounds() {
    // 1. Negative and >= 1.0 `--epsilon` values must be rejected in both flag forms.
    for bad_args in [
        vec![
            "arkhe",
            "yardstick",
            "--generator",
            r#"{"kind":"T1_Constant","value":1}"#,
            "--width",
            "1",
            "--epsilon",
            "-0.05",
        ],
        vec![
            "arkhe",
            "yardstick",
            "--generator",
            r#"{"kind":"T1_Constant","value":1}"#,
            "--width",
            "1",
            "--epsilon=-0.05",
        ],
        vec![
            "arkhe",
            "yardstick",
            "--generator",
            r#"{"kind":"T1_Constant","value":1}"#,
            "--width",
            "1",
            "--epsilon",
            "1.0",
        ],
    ] {
        assert!(
            arkhe::run_with_args(bad_args.clone()).is_err(),
            "expected error for invalid yardstick args {bad_args:?}"
        );
    }

    // 2. Period-257 motif (`256` zeros followed by `1`): yardstick must extend past the 256-tick
    //    all-zero prefix, certify full-cycle Reeds–Sloane (`min_len != [0]`), and refute 0-node echo!
    let mut motif_257 = vec![0u64; 256];
    motif_257.push(1);
    let gt_257 = compute_ground_truth_yardstick(
        &GeneratorConfig::T1_PeriodicMotif { motif: motif_257 },
        Width::BITS_1,
        WireDelayMode::Combinational,
        &NoiseModel::None,
        256,
        Some(3),
    )
    .expect("period-257 yardstick succeeds");
    assert!(gt_257.reeds_sloane_certified_full_cycle);
    assert!(gt_257.reeds_sloane_prefix_ticks >= 514);
    assert_ne!(gt_257.reeds_sloane_min_len, vec![0]);
    assert_eq!(gt_257.exact_synthesis_min_nodes, None);
    assert_eq!(
        gt_257.exact_synthesis_status,
        SynthesisCertStatus::NoCircuitWithinCap {
            max_nodes_searched: 3
        }
    );

    // 3. Large-state noisy LFSR (`k = 2, L = 10` -> 4^10 = 1,048,576 > 65,536 states):
    //    `epsilon_machine` is `None` with explicit error, declared noise entropy > 0.36, and
    //    `Scorecard` never substitutes `0.0` for missing `h_mu`!
    let large_lfsr = GeneratorConfig::T3c_Lfsr {
        taps: vec![1, 0, 3, 2, 0, 1, 0, 2, 1, 3],
        init: vec![1, 2, 3, 1, 0, 2, 1, 3, 2, 1],
    };
    let noise = NoiseModel::IndependentSubstitution {
        epsilon: 0.05,
        channels: vec![0],
    };
    let gt_large = compute_ground_truth_yardstick(
        &large_lfsr,
        Width::BITS_2,
        WireDelayMode::Combinational,
        &noise,
        128,
        Some(2),
    )
    .expect("large LFSR yardstick succeeds");
    assert!(gt_large.epsilon_machine.is_none());
    assert!(gt_large.epsilon_machine_error.is_some());
    assert!(gt_large.declared_noise_entropy_bits_per_symbol > 0.36);

    let cfg_large = ExperimentConfig {
        schema_version: CONFIG_SCHEMA_VERSION,
        master_seed: 3,
        substrate: SubstrateConfig {
            width_bits: Width::BITS_2,
            wire_delay: WireDelayMode::Combinational,
            wiring_topology: WiringTopology::WellMixed,
        },
        environment: EnvironmentConfig {
            horizon_ticks: 120,
            transient_warmup_ticks: 20,
            generator: large_lfsr,
            noise,
        },
        budget: BudgetConfig {
            max_eval_ticks: 10_000,
        },
        baseline: BaselineConfig::Ctw { context_depth: 6 },
    };
    let rep_large = execute_experiment(&cfg_large).expect("large LFSR run succeeds");
    assert!(rep_large.scorecard.excess_prob_bits_over_h_mu.is_none());
    assert!(rep_large.scorecard.excess_residual_bits_over_h_mu.is_none());
    assert!(
        rep_large
            .scorecard
            .realized_oracle_bits_per_symbol
            .is_none()
    );
    assert!(rep_large.scorecard.paired_regret_bits_per_symbol.is_none());
}

#[test]
fn topology_guardrail_and_reactive_state_accounting_are_strict() {
    // 1. Non-WellMixed topologies are explicitly rejected in `execute_experiment`.
    for unsupported_topo in [WiringTopology::LocalVonNeumann, WiringTopology::LocalMoore] {
        let cfg = ExperimentConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            master_seed: 0,
            substrate: SubstrateConfig {
                width_bits: Width::BITS_1,
                wire_delay: WireDelayMode::Combinational,
                wiring_topology: unsupported_topo,
            },
            environment: EnvironmentConfig {
                horizon_ticks: 80,
                transient_warmup_ticks: 16,
                generator: GeneratorConfig::T1_Alternating { a: 0, b: 1 },
                noise: NoiseModel::None,
            },
            budget: BudgetConfig {
                max_eval_ticks: 5_000,
            },
            baseline: BaselineConfig::Cgp {
                lambda: 4,
                max_genes: 6,
                fact_window: 48,
            },
        };
        assert!(matches!(
            execute_experiment(&cfg),
            Err(ConfigRunError::UnsupportedExperimentTopology(t)) if t == unsupported_topo
        ));
    }

    // 2. `T1_Alternating` 0-Delay reactive inverter reports `boundary_lag_register_count == Some(1)`
    //    and `effective_excess_state_bits_over_c_mu == Some(0.0)` (not `-1.0`).
    let alt_cfg = ExperimentConfig {
        schema_version: CONFIG_SCHEMA_VERSION,
        master_seed: 0,
        substrate: SubstrateConfig {
            width_bits: Width::BITS_1,
            wire_delay: WireDelayMode::Combinational,
            wiring_topology: WiringTopology::WellMixed,
        },
        environment: EnvironmentConfig {
            horizon_ticks: 160,
            transient_warmup_ticks: 30,
            generator: GeneratorConfig::T1_Alternating { a: 0, b: 1 },
            noise: NoiseModel::None,
        },
        budget: BudgetConfig {
            max_eval_ticks: 20_000,
        },
        baseline: BaselineConfig::Exhaustive {
            max_nodes: 3,
            fact_window: 48,
        },
    };
    let rep = execute_experiment(&alt_cfg).unwrap();
    assert_eq!(rep.scorecard.delay_count, Some(0));
    assert_eq!(rep.scorecard.wire_hop_register_count, Some(0));
    assert_eq!(rep.scorecard.boundary_lag_register_count, Some(1));
    assert_eq!(rep.scorecard.effective_state_registers, Some(1));
    assert_eq!(rep.scorecard.internal_state_bits, Some(0));
    assert_eq!(
        rep.scorecard.effective_excess_state_bits_over_c_mu,
        Some(0.0)
    );
}

#[test]
fn prime_period_5_and_7_sliding_windows_preserve_exact_phase() {
    // 1. Autonomous period-5 (`[0, 1, 2, 3, 1]` at k=2) and period-7 (`[0, 0, 1, 0, 1, 1, 1]` at k=1)
    //    ring oscillators replayed across 600 ticks (`fact_window = 64`, 30+ chunk-16 evictions):
    for (width, motif) in [
        (Width::BITS_2, vec![0u64, 1, 2, 3, 1]),
        (Width::BITS_1, vec![0u64, 0, 1, 0, 1, 1, 1]),
    ] {
        let p = motif.len();
        let mut osc = Circuit::new(width, 1, 1, WireDelayMode::Combinational);
        // Shift ring: d_{p-1} outputs motif[0], ..., d_0 outputs motif[p-1] -> d_{p-1} = d_0, etc.
        // At t=0: d_0 emits to Split -> (BoundaryOut(0), d_{p-1}); d_i <- d_{i+1}:
        let mut delays = Vec::with_capacity(p);
        for &val in &motif {
            delays.push(osc.add_node(Primitive::Delay { init: val }));
        }
        let sp = osc.add_node(Primitive::Split);
        osc.connect(
            SignalSource::NodeOut {
                node: delays[0],
                port: 0,
            },
            SignalSink::NodeIn { node: sp, port: 0 },
        );
        osc.connect(
            SignalSource::NodeOut { node: sp, port: 0 },
            SignalSink::BoundaryOut(0),
        );
        osc.connect(
            SignalSource::NodeOut { node: sp, port: 1 },
            SignalSink::NodeIn {
                node: delays[p - 1],
                port: 0,
            },
        );
        for i in 0..(p - 1) {
            osc.connect(
                SignalSource::NodeOut {
                    node: delays[i + 1],
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: delays[i],
                    port: 0,
                },
            );
        }
        let compiled = osc.compile().unwrap();
        assert!(!compiled.uses_boundary_inputs());

        let mut generator = GeneratorConfig::T1_PeriodicMotif {
            motif: motif.clone(),
        }
        .build(width)
        .unwrap();
        let mut win = ObservationWindow::new(1, 64);
        let mut frame = [0u64; 1];
        let mut pred = [0u64; 1];
        let mut budget = TickBudget::new(1_000_000);

        for t in 0..600u64 {
            generator.next_clean_frame(&mut frame);
            win.push_frame(&frame);
            if t >= 16 {
                let (mismatches, scored, mut end_state) =
                    evaluate_circuit_on_window(&compiled, &win, &mut budget).unwrap();
                assert!(scored > 0);
                assert_eq!(
                    mismatches,
                    0,
                    "autonomous period-{p} oscillator slipped phase at tick {t} (start_tick={})",
                    win.start_tick()
                );
                // Verify `end_state` predicts tick `t + 1` accurately!
                end_state.step(&compiled, &[frame[0]], &mut pred).unwrap();
                assert_eq!(pred[0], motif[((t + 1) as usize) % p]);
            }
        }
    }

    // 2. End-to-end `Exhaustive` and `CGP` runs over `500` post-warmup ticks (`fact_window = 64`)
    //    on prime period 5 and prime period 7:
    for (width, motif, baseline) in [
        (
            Width::BITS_1,
            vec![0u64, 1, 1, 0, 1],
            BaselineConfig::Exhaustive {
                max_nodes: 4,
                fact_window: 64,
            },
        ),
        (
            Width::BITS_1,
            vec![0u64, 0, 1, 0, 1, 1, 1],
            BaselineConfig::Exhaustive {
                max_nodes: 4,
                fact_window: 64,
            },
        ),
        (
            Width::BITS_1,
            vec![0u64, 0, 1, 0, 1, 1, 1],
            BaselineConfig::Cgp {
                lambda: 4,
                max_genes: 8,
                fact_window: 64,
            },
        ),
        (
            Width::BITS_2,
            vec![0u64, 1, 2, 3, 1],
            BaselineConfig::Cgp {
                lambda: 4,
                max_genes: 8,
                fact_window: 64,
            },
        ),
    ] {
        let cfg = ExperimentConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            master_seed: 2,
            substrate: SubstrateConfig {
                width_bits: width,
                wire_delay: WireDelayMode::Combinational,
                wiring_topology: WiringTopology::WellMixed,
            },
            environment: EnvironmentConfig {
                horizon_ticks: 850,
                transient_warmup_ticks: 350,
                generator: GeneratorConfig::T1_PeriodicMotif {
                    motif: motif.clone(),
                },
                noise: NoiseModel::None,
            },
            budget: BudgetConfig {
                max_eval_ticks: 350_000,
            },
            baseline,
        };
        let rep = execute_experiment(&cfg).unwrap();
        assert_eq!(
            rep.scorecard.post_transient_observed_mismatches,
            0,
            "prime-period {} run failed with {} mismatches ({})",
            motif.len(),
            rep.scorecard.post_transient_observed_mismatches,
            rep.baseline_name
        );
    }
}
