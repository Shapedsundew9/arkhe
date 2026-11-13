use arkhe::baselines::{PredictorBaseline, TickBudget};
use arkhe::rng::SeedBank;
use arkhe::structural::{ArkheLearner, GrowthConfig, ResidualFeature, diagnose_residual};
use arkhe::substrate::{Width, WireDelayMode};

#[test]
fn residual_diagnostics_gate_structured_growth_and_stop_at_zero_error() {
    let config = GrowthConfig {
        mi_threshold_bits: 0.0,
        min_match_gain: 2,
        ..GrowthConfig::default()
    };
    let structured = diagnose_residual(
        Width::BITS_2,
        &[0, 1, 0, 1, 0, 1, 0, 1],
        &[ResidualFeature {
            name: "lagged-input".to_owned(),
            values: vec![0, 1, 0, 1, 0, 1, 0, 1],
        }],
        config,
    )
    .unwrap();
    assert!(structured.growth_criterion_met);

    let exact = diagnose_residual(
        Width::BITS_2,
        &[0; 8],
        &[ResidualFeature {
            name: "lagged-input".to_owned(),
            values: vec![0, 1, 0, 1, 0, 1, 0, 1],
        }],
        config,
    )
    .unwrap();
    assert!(exact.zero_error);
    assert!(!exact.growth_criterion_met);
}

#[test]
fn arkhe_learner_runs_with_a_metered_candidate_budget() {
    let mut learner = ArkheLearner::new(
        Width::BITS_1,
        1,
        WireDelayMode::Combinational,
        GrowthConfig {
            max_nodes: 2,
            ..GrowthConfig::default()
        },
        SeedBank::new(17),
    )
    .unwrap();
    let mut budget = TickBudget::new(10_000);
    for tick in 0..96 {
        let _prediction = learner.predict(&mut budget);
        learner.observe(&[(tick % 2) as u64], &mut budget);
    }

    assert!(learner.best_circuit().unwrap().compile().is_ok());
    assert!(budget.consumed() <= 10_000);
}

#[test]
fn execute_experiment_arkhe_m1_across_wire_delays() {
    use arkhe::config::{
        BaselineConfig, BudgetConfig, CONFIG_SCHEMA_VERSION, EnvironmentConfig, ExperimentConfig,
        SubstrateConfig, execute_experiment,
    };
    use arkhe::env::{GeneratorConfig, NoiseModel};
    use arkhe::structural::EconomyConfig;

    let cases = [WireDelayMode::Combinational, WireDelayMode::TicksPerHop];

    for wire_delay in cases {
        let cfg = ExperimentConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            master_seed: 42,
            substrate: SubstrateConfig {
                width_bits: Width::BITS_1,
                wire_delay,
            },
            environment: EnvironmentConfig {
                horizon_ticks: 200,
                transient_warmup_ticks: 50,
                generator: GeneratorConfig::T1_Alternating { a: 0, b: 1 },
                noise: NoiseModel::None,
            },
            budget: BudgetConfig {
                max_eval_ticks: 10_000,
            },
            baseline: BaselineConfig::ArkheM1 {
                economy: EconomyConfig::default(),
                growth: GrowthConfig::default(),
            },
        };

        let report = execute_experiment(&cfg).expect("execute_experiment succeeds for ArkheM1");
        assert_eq!(report.baseline_name, "ArkheM1");
        assert!(report.m1_telemetry.is_some());
        let tel = report.m1_telemetry.unwrap();
        assert!(tel.final_total_energy.is_finite());
    }
}

#[test]
fn cli_verify_m1b_refuses_holdout_seeds_without_flag() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_arkhe"))
        .args(["verify-m1b", "--seed-offset", "10000", "--seeds", "20"])
        .output()
        .expect("spawn arkhe verify-m1b on holdout");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("overlaps pre-registered holdout range"),
        "stderr must mention holdout refusal, got: {stderr}"
    );
}

#[test]
fn cli_verify_m1_refuses_holdout_seeds_without_flag() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_arkhe"))
        .args(["verify-m1", "--seed-offset", "10000", "--seeds", "20"])
        .output()
        .expect("spawn arkhe verify-m1 on holdout");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("overlaps pre-registered holdout range"),
        "stderr must mention holdout refusal, got: {stderr}"
    );
}

#[test]
fn cli_verify_m1b_and_m1_banks_refuse_holdout_overlap_without_flag() {
    for subcmd in ["verify-m1b", "verify-m1"] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_arkhe"))
            .args([
                subcmd,
                "--banks",
                "10",
                "--seed-offset",
                "9900",
                "--seeds",
                "20",
            ])
            .output()
            .expect("spawn arkhe bank mode overlapping holdout");

        assert!(
            !output.status.success(),
            "{subcmd} --banks must refuse 9900..10100 holdout overlap"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("overlaps pre-registered holdout range"),
            "{subcmd} stderr must mention holdout refusal, got: {stderr}"
        );
    }
}

#[test]
fn fast_seed_slice_m1b_all_gates_pass() {
    use arkhe::verify_m1::run_m1b_verification_suite;

    let summary = run_m1b_verification_suite(0, 2, 2000);
    assert_eq!(summary.gates.len(), 4);
    assert!(
        summary.gates[0].passed,
        "G1B.1 failed: {:?}",
        summary.gates[0].failures
    );
    assert!(
        summary.gates[1].passed,
        "G1B.2 failed: {:?}",
        summary.gates[1].failures
    );
    assert!(
        summary.gates[2].passed,
        "G1B.3 failed: {:?}",
        summary.gates[2].failures
    );
    assert!(
        summary.gates[3].passed,
        "G1B.4 failed: {:?}",
        summary.gates[3].failures
    );
    assert!(summary.all_passed);
}

#[test]
fn m1_stage_gate_aborts_m1b_when_m1a_fails() {
    use arkhe::config::{BuildProvenance, CliffCountersSummary};
    use arkhe::verify_m1::{M1aVerificationSummary, evaluate_m1_stage_gate};

    let failing_m1a = M1aVerificationSummary {
        provenance: BuildProvenance::current(),
        seed_offset: 0,
        seeds_tested: 1,
        horizon: 64,
        gates: Vec::new(),
        panel: Vec::new(),
        all_passed: false,
        cliff_counters: CliffCountersSummary::default(),
    };

    let combined = evaluate_m1_stage_gate(failing_m1a, || {
        panic!("M1b must never execute when M1a stage gate fails")
    });

    assert!(combined.stage_gate_aborted);
    assert!(combined.m1b.is_none());
    assert!(!combined.all_passed);

    let json = serde_json::to_string(&combined).expect("serialize combined");
    assert!(json.contains("\"stage_gate_aborted\":true"));
    assert!(!json.contains("\"m1b\":"));
}
