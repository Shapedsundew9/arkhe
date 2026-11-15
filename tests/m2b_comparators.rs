//! Integration tests for Milestone M2b redundancy sprouting, internal comparators,
//! telemetry wiring, noise resilience, channel-fault handoff, and offline loopback falsification.

use arkhe::baselines::{PredictorBaseline, TickBudget};
use arkhe::config::{
    ArkheM1Telemetry, BaselineConfig, BudgetConfig, CONFIG_SCHEMA_VERSION, EnvironmentConfig,
    ExperimentConfig, SubstrateConfig, execute_experiment,
};
use arkhe::env::{
    GeneratorConfig, LfsrStage, MultiViewDriver, MultiViewTarget, MultiViewTransform, NoiseModel,
    NoisyEnvironment,
};
use arkhe::rng::SeedBank;
use arkhe::structural::{
    ArkheLearner, EconomyConfig, EditableCircuit, GrowthConfig, build_autonomous_ring_source,
    splice_comparator_onto_port,
};
use arkhe::substrate::{Circuit, Primitive, SignalSink, SignalSource, Width, WireDelayMode};

fn multiview_config_default() -> GeneratorConfig {
    GeneratorConfig::T2_MultiViewCoupled {
        driver: MultiViewDriver::Lfsr {
            stage: LfsrStage {
                taps: vec![1, 1, 0, 1],
                init: vec![1, 0, 1, 1],
            },
        },
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
    }
}

fn arkhe_experiment_config(
    width: Width,
    wire_delay: WireDelayMode,
    generator: GeneratorConfig,
    noise: NoiseModel,
    horizon_ticks: u64,
    transient_warmup_ticks: u64,
    master_seed: u64,
) -> ExperimentConfig {
    ExperimentConfig {
        schema_version: CONFIG_SCHEMA_VERSION,
        master_seed,
        substrate: SubstrateConfig {
            width_bits: width,
            wire_delay,
        },
        environment: EnvironmentConfig {
            horizon_ticks,
            transient_warmup_ticks,
            generator,
            noise,
        },
        budget: BudgetConfig {
            max_eval_ticks: 200_000,
        },
        baseline: BaselineConfig::ArkheM1 {
            economy: EconomyConfig::default(),
            growth: GrowthConfig::default(),
        },
    }
}

#[test]
fn t2_multiview_coupled_comparator_formation_and_telemetry_wiring() {
    for width in [Width::BITS_1, Width::BITS_2] {
        let cfg = arkhe_experiment_config(
            width,
            WireDelayMode::Combinational,
            multiview_config_default(),
            NoiseModel::None,
            1_500,
            500,
            5,
        );
        let report = execute_experiment(&cfg).expect("T2_MultiViewCoupled experiment succeeds");
        let ch2 = report
            .scorecard
            .per_channel
            .get(2)
            .expect("target channel 2 scorecard present");
        assert_eq!(
            ch2.post_transient_clean_mismatches, 0,
            "target channel 2 must achieve zero post-transient clean mismatches ({width:?})"
        );

        let tel = report
            .m1_telemetry
            .as_ref()
            .expect("ArkheM1 telemetry present in RunReport");
        assert!(
            tel.redundancy_sprout_events >= 1,
            "expected redundancy_sprout_events >= 1, got {}",
            tel.redundancy_sprout_events
        );
        assert!(
            tel.active_comparator_count >= 1,
            "expected active_comparator_count >= 1, got {}",
            tel.active_comparator_count
        );
        assert!(
            tel.comparator_agreement_ticks > 0,
            "expected comparator_agreement_ticks > 0, got {}",
            tel.comparator_agreement_ticks
        );
        assert_eq!(tel.disagreement_prune_events, 0);
        assert_eq!(tel.sprout_blocked_max_nodes, 0);

        // Serde round-trip on live populated ArkheM1Telemetry.
        let serialized = serde_json::to_string(tel).expect("serialize ArkheM1Telemetry");
        let deserialized: ArkheM1Telemetry =
            serde_json::from_str(&serialized).expect("deserialize ArkheM1Telemetry");
        assert_eq!(deserialized.final_node_count, tel.final_node_count);
        assert_eq!(
            deserialized.redundancy_sprout_events,
            tel.redundancy_sprout_events
        );
        assert_eq!(
            deserialized.active_comparator_count,
            tel.active_comparator_count
        );
        assert_eq!(
            deserialized.comparator_agreement_ticks,
            tel.comparator_agreement_ticks
        );
        assert_eq!(
            deserialized.comparator_disagreement_ticks,
            tel.comparator_disagreement_ticks
        );
        assert!(
            (deserialized.cumulative_comparator_flux - tel.cumulative_comparator_flux).abs()
                < 1e-12
        );
        assert_eq!(
            deserialized.driver_handoff_events,
            tel.driver_handoff_events
        );
        assert_eq!(
            deserialized.disagreement_prune_events,
            tel.disagreement_prune_events
        );
    }

    // Legacy JSON without the 7 M2b comparator fields must deserialize cleanly with 0 / 0.0 defaults.
    let legacy_json = r#"{
        "final_node_count": 3,
        "final_split_count": 0,
        "final_delay_count": 2,
        "final_total_energy": 9.5,
        "min_mature_node_energy": 2.0,
        "cumulative_net_flux": 32.0,
        "cumulative_maturity_debt_paid": 6.0,
        "null_calibrated_mdl_delta_bits": -14.0,
        "sprout_events": 1,
        "ring_closure_events": 0,
        "starvation_prune_events": 0,
        "idle_prune_events": 0
    }"#;
    let legacy: ArkheM1Telemetry =
        serde_json::from_str(legacy_json).expect("legacy ArkheM1Telemetry JSON deserializes");
    assert_eq!(legacy.redundancy_sprout_events, 0);
    assert_eq!(legacy.active_comparator_count, 0);
    assert_eq!(legacy.comparator_agreement_ticks, 0);
    assert_eq!(legacy.comparator_disagreement_ticks, 0);
    assert_eq!(legacy.cumulative_comparator_flux, 0.0);
    assert_eq!(legacy.driver_handoff_events, 0);
    assert_eq!(legacy.disagreement_prune_events, 0);
}

#[test]
fn ticks_per_hop_phase_non_regression_on_t2_multiview_coupled() {
    // Causal Split-primary TicksPerHop on T2_MultiViewCoupled (verifying Stage 3 phase non-regression
    // and comparator formation with unit-hop edges).
    for width in [Width::BITS_1, Width::BITS_2] {
        let mv_cfg = GeneratorConfig::T2_MultiViewCoupled {
            driver: MultiViewDriver::Lfsr {
                stage: LfsrStage {
                    taps: vec![1, 1, 0, 0, 1, 1],
                    init: vec![1, 0, 1, 1, 0, 1],
                },
            },
            view_0: MultiViewTransform {
                delay: 1,
                mul_a: 1,
                add_b: 0,
            },
            view_1: MultiViewTransform {
                delay: 0,
                mul_a: 1,
                add_b: 0,
            },
            target: MultiViewTarget::Affine {
                lag: 4,
                mul_a: 1,
                add_b: 0,
            },
        };
        let sb = SeedBank::new(5);
        let mut env = NoisyEnvironment::new(
            mv_cfg.build(width).expect("build mv_cfg"),
            NoiseModel::None,
            sb,
        )
        .expect("build env");

        let mut tph_circ = Circuit::new(width, 3, 3, WireDelayMode::TicksPerHop);
        let split_p = tph_circ.add_node(Primitive::Split);
        tph_circ.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn {
                node: split_p,
                port: 0,
            },
        );
        tph_circ.connect(
            SignalSource::NodeOut {
                node: split_p,
                port: 0,
            },
            SignalSink::BoundaryOut(2),
        );
        let econ = EconomyConfig::default();
        let mut editable = EditableCircuit::new(tph_circ, econ).expect("build editable circuit");
        if let Some(node_econ) = editable.economy_mut().node_mut(split_p.index()) {
            node_econ.juvenile_ticks = econ.juvenile_ticks;
        }
        let mut learner =
            ArkheLearner::from_editable(editable, GrowthConfig::default(), sb).expect("learner");
        let mut budget = TickBudget::new(200_000);
        let mut clean = [0u64; 3];
        let mut obs = [0u64; 3];
        let mut post_mismatches = 0usize;
        for t in 0..1_200 {
            env.step_frame(&mut clean, &mut obs);
            let pred = learner.predict(&mut budget);
            if t >= 700 && pred[2] != clean[2] {
                post_mismatches += 1;
            }
            learner.observe(&obs, &mut budget);
        }
        let tel = learner.telemetry();
        assert_eq!(
            post_mismatches, 0,
            "Split-primary TicksPerHop must have 0 clean mismatches ({width:?})"
        );
        assert!(
            learner
                .editable()
                .circuit()
                .edges()
                .iter()
                .all(|e| e.hops == 1),
            "all TicksPerHop edges must have hops == 1"
        );
        assert!(tel.redundancy_sprout_events >= 1);
        assert!(tel.active_comparator_count >= 1);
        assert_eq!(tel.starvation_prune_events, 0);
        assert_eq!(tel.sprout_blocked_max_nodes, 0);
    }
}

#[test]
fn five_percent_noise_non_thrashing_preserves_comparators() {
    // T1_Alternating under 5% IndependentSubstitution noise via execute_experiment
    // (validated Phase 2 noise non-thrashing configuration).
    let cfg_t1 = arkhe_experiment_config(
        Width::BITS_1,
        WireDelayMode::Combinational,
        GeneratorConfig::T1_Alternating { a: 0, b: 1 },
        NoiseModel::IndependentSubstitution {
            epsilon: 0.05,
            channels: vec![0],
        },
        1_600,
        500,
        0,
    );
    let report_t1 = execute_experiment(&cfg_t1).expect("5% noisy T1 experiment succeeds");
    let tel_t1 = report_t1
        .m1_telemetry
        .as_ref()
        .expect("m1_telemetry present");
    assert_eq!(tel_t1.disagreement_prune_events, 0);
    assert_eq!(tel_t1.starvation_prune_events, 0);
    assert_eq!(tel_t1.redundancy_sprout_events, 1);
    assert_eq!(tel_t1.active_comparator_count, 1);
    assert!(tel_t1.comparator_agreement_ticks > 0);
    assert_eq!(tel_t1.sprout_blocked_max_nodes, 0);
}

#[test]
fn channel_fault_handoff_and_clean_recovery_via_execute_experiment() {
    let fault_tick = 1_000u64;
    for width in [Width::BITS_1, Width::BITS_2] {
        let fault_eps = 1.0 - 1.0 / (2.0f64.powi(i32::from(width.bits())));
        let cfg = arkhe_experiment_config(
            width,
            WireDelayMode::Combinational,
            multiview_config_default(),
            NoiseModel::ChannelFaultSchedule {
                base_epsilon: 0.0,
                base_channels: Vec::new(),
                fault_tick,
                fault_epsilon: fault_eps,
                fault_channels: vec![0],
            },
            2_000,
            500,
            11,
        );
        let report = execute_experiment(&cfg).expect("ChannelFaultSchedule experiment succeeds");
        let tel = report.m1_telemetry.as_ref().expect("m1_telemetry present");
        assert!(
            tel.redundancy_sprout_events >= 1,
            "comparator must form prior to fault ({width:?})"
        );
        assert!(
            tel.driver_handoff_events >= 1,
            "channel fault must trigger driver handoff ({width:?})"
        );
        assert!(
            tel.disagreement_prune_events >= 1,
            "channel fault must prune faulty primary route ({width:?})"
        );
        assert!(tel.comparator_disagreement_ticks >= 1);
        assert_eq!(tel.sprout_blocked_max_nodes, 0);

        let ch2 = report
            .scorecard
            .per_channel
            .get(2)
            .expect("target channel 2 scorecard present");
        let recover_tick = ch2
            .ticks_to_clean_solution
            .expect("channel 2 must lock back onto clean truth post-fault");
        assert!(
            recover_tick <= fault_tick + 64,
            "channel 2 recovery tick {recover_tick} exceeded fault_tick + 64 ({width:?})"
        );
    }
}

#[test]
fn charter_1_3_offline_loopback_falsification_and_majority_pruning() {
    // 1. Deceptive lag-2 vs. period-6 [0, 1, 0, 1, 0, 0] offline loopback contradiction.
    {
        let width = Width::BITS_1;
        let mut circ = Circuit::new(width, 1, 1, WireDelayMode::Combinational);
        let d1 = circ.add_node(Primitive::Delay { init: 0 });
        let d2 = circ.add_node(Primitive::Delay { init: 1 });
        circ.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: d1, port: 0 },
        );
        circ.connect(
            SignalSource::NodeOut { node: d1, port: 0 },
            SignalSink::NodeIn { node: d2, port: 0 },
        );
        circ.connect(
            SignalSource::NodeOut { node: d2, port: 0 },
            SignalSink::BoundaryOut(0),
        );

        let motif = [0u64, 1, 0, 1, 0, 0];
        let (_, _, ring_src) = build_autonomous_ring_source(&mut circ, 6, &motif);
        let spliced = splice_comparator_onto_port(&mut circ, 0, ring_src).expect("splice");
        let econ = EconomyConfig {
            juvenile_ticks: 0,
            ..EconomyConfig::default()
        };
        let mut editable = EditableCircuit::new(circ, econ).expect("editable");
        editable
            .register_comparator(spliced.sub_node, spliced.eqz_node, 0, 1, 0)
            .expect("register comparator");

        let mut learner =
            ArkheLearner::from_editable(editable, GrowthConfig::default(), SeedBank::new(0))
                .expect("learner");
        let mut budget = TickBudget::new(20_000);
        for t in 0..24 {
            let _ = learner.predict(&mut budget);
            learner.observe(&[motif[t % 6]], &mut budget);
        }

        let report = learner.run_offline_loopback(128);
        assert!(report.contradiction_detected);
        assert!(report.comparator_disagreement_ticks >= 1);
        assert!(report.max_circular_discrepancy >= 1);
        assert!(report.sleep_comparator_flux < 0.0);

        let tel = learner.telemetry();
        assert!(tel.comparator_disagreement_ticks >= 1);
        assert_eq!(tel.active_comparator_count, 1);
    }

    // 2. 3-route majority pruning (minority faulty alt and minority faulty primary).
    for primary_is_minority in [false, true] {
        let width = Width::BITS_1;
        let mut circ = Circuit::new(width, 1, 1, WireDelayMode::Combinational);
        let (_, _, r1) = build_autonomous_ring_source(&mut circ, 2, &[0, 1]);
        let (_, _, r2) = build_autonomous_ring_source(&mut circ, 2, &[0, 1]);
        let (_, _, r_fault) = build_autonomous_ring_source(&mut circ, 2, &[0, 0]);

        let (pri, alt_a, alt_b) = if primary_is_minority {
            (r_fault, r1, r2)
        } else {
            (r1, r2, r_fault)
        };
        circ.connect(pri, SignalSink::BoundaryOut(0));
        let sp_a = splice_comparator_onto_port(&mut circ, 0, alt_a).expect("splice a");
        let sp_b = splice_comparator_onto_port(&mut circ, 0, alt_b).expect("splice b");

        let econ = EconomyConfig {
            juvenile_ticks: 0,
            ..EconomyConfig::default()
        };
        let mut editable = EditableCircuit::new(circ, econ).expect("editable");
        editable
            .register_comparator(sp_a.sub_node, sp_a.eqz_node, 0, 1 << 60, 1 << 61)
            .expect("register comp a");
        editable
            .register_comparator(sp_b.sub_node, sp_b.eqz_node, 0, 1 << 60, 1 << 62)
            .expect("register comp b");

        let mut learner =
            ArkheLearner::from_editable(editable, GrowthConfig::default(), SeedBank::new(9))
                .expect("learner");
        let mut budget = TickBudget::new(20_000);
        for t in 0..16 {
            let _ = learner.predict(&mut budget);
            learner.observe(&[(t % 2) as u64], &mut budget);
        }
        assert_eq!(learner.telemetry().active_comparator_count, 2);

        let report = learner.run_offline_loopback(128);
        assert_eq!(report.minority_routes_pruned, 1);
        assert_eq!(learner.telemetry().active_comparator_count, 1);

        let clean_report = learner.run_offline_loopback(64);
        assert!(!clean_report.contradiction_detected);
        assert_eq!(clean_report.comparator_disagreement_ticks, 0);
        assert_eq!(clean_report.max_circular_discrepancy, 0);
    }
}
