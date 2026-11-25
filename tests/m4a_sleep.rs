//! Milestone M4a-0 Phase 0.1 wake-sleep invariants (`I1`, `I2`, `I3`, `I5`, `R2`, `R3`, `R7`).

use arkhe::baselines::{PredictorBaseline, TickBudget};
use arkhe::env::{
    FiniteStateGenerator, GeneratorConfig, LfsrStage, MultiViewDriver, MultiViewTarget,
    MultiViewTransform, NoiseModel, NoisyEnvironment,
};
use arkhe::rng::SeedBank;
use arkhe::structural::sleep::SleepSchedule;
use arkhe::structural::{
    ArkheLearner, EconomyConfig, EditableCircuit, ExecutionPhase, FactBuffer, GrowthConfig,
    ReserveCap, build_autonomous_ring_source, derived_wake_sleep_windows, observation_horizon,
    port_cone_register_depth, run_wake_sleep_episode, run_wake_sleep_episode_audited,
    splice_comparator_onto_port, tap_source, two_sample_kt_match_test,
    verify_epistemic_mdl_conservation,
};
use arkhe::substrate::{Circuit, Primitive, SignalSink, SignalSource, Width, WireDelayMode};

fn multi_view_periodic_generator() -> GeneratorConfig {
    GeneratorConfig::T2_MultiViewCoupled {
        driver: MultiViewDriver::PeriodicMotif {
            motif: vec![1, 3, 2, 0],
        },
        view_0: MultiViewTransform {
            delay: 0,
            mul_a: 1,
            add_b: 0,
        },
        view_1: MultiViewTransform {
            delay: 1,
            mul_a: 3,
            add_b: 1,
        },
        target: MultiViewTarget::Affine {
            lag: 2,
            mul_a: 1,
            add_b: 0,
        },
    }
}

fn multi_view_lfsr_generator() -> GeneratorConfig {
    GeneratorConfig::T2_MultiViewCoupled {
        driver: MultiViewDriver::Lfsr {
            stage: LfsrStage {
                taps: vec![1, 1],
                init: vec![1, 2],
            },
        },
        view_0: MultiViewTransform {
            delay: 0,
            mul_a: 1,
            add_b: 0,
        },
        view_1: MultiViewTransform {
            delay: 0,
            mul_a: 3,
            add_b: 1,
        },
        target: MultiViewTarget::AddEqz { lag_a: 1, lag_b: 1 },
    }
}

fn make_learner(
    width: Width,
    channels: u16,
    econ: EconomyConfig,
    growth: GrowthConfig,
) -> ArkheLearner {
    ArkheLearner::new_with_config(
        width,
        channels,
        WireDelayMode::Combinational,
        econ,
        growth,
        SeedBank::new(0),
    )
    .expect("learner initialization must succeed")
}

type ScorerSnapshot = (u64, u64, u64, [u64; 4]);

fn snapshot_scorers(learner: &ArkheLearner) -> Vec<ScorerSnapshot> {
    learner
        .editable()
        .economy()
        .scorers()
        .iter()
        .map(|s| {
            let nm = s.null_model();
            (
                s.ticks(),
                s.matches(),
                s.q_hat().to_bits(),
                [
                    nm.probability(0).to_bits(),
                    nm.probability(1).to_bits(),
                    nm.probability(2).to_bits(),
                    nm.probability(3).to_bits(),
                ],
            )
        })
        .collect()
}

#[test]
fn i1_sleep_schedule_none_is_bit_identical_to_wake_only() {
    let default_growth = GrowthConfig::default();
    assert_eq!(default_growth.sleep_schedule, SleepSchedule::None);
    assert_eq!(default_growth.sleep_window_override, None);

    // Legacy JSON configs without `sleep_schedule` or `sleep_window_override` deserialize cleanly.
    let legacy_json = r#"{"diagnostic_window":32}"#;
    let parsed: GrowthConfig = serde_json::from_str(legacy_json).expect("legacy GrowthConfig json");
    assert_eq!(parsed.sleep_schedule, SleepSchedule::None);
    assert_eq!(parsed.sleep_window_override, None);

    let width = Width::BITS_2;
    let gen_cfg = multi_view_periodic_generator();
    let econ = EconomyConfig::default();
    let growth = GrowthConfig {
        redundancy_sprouting: true,
        ..GrowthConfig::default()
    };

    let t_total = 640u64;
    let max_eval = 50_000u64;

    // 1. Canonical wake-sleep episode under SleepSchedule::None.
    let gen_a = gen_cfg.build(width).unwrap();
    let channels = gen_a.channels();
    let mut env_a = NoisyEnvironment::new(gen_a, NoiseModel::None, SeedBank::new(11)).unwrap();
    let mut learner_a = make_learner(width, channels, econ, growth);
    let report_a = run_wake_sleep_episode(&mut learner_a, &mut env_a, t_total, 256, max_eval);

    // 2. Direct manual prequential wake loop.
    let gen_b = gen_cfg.build(width).unwrap();
    let mut env_b = NoisyEnvironment::new(gen_b, NoiseModel::None, SeedBank::new(11)).unwrap();
    let mut learner_b = make_learner(width, channels, econ, growth);
    let mut budget_b = TickBudget::new(max_eval);
    let mut clean = vec![0u64; usize::from(channels)];
    let mut obs = vec![0u64; usize::from(channels)];
    let mut preds_b = Vec::with_capacity(t_total as usize);
    let mut obs_b = Vec::with_capacity(t_total as usize);
    for _ in 0..t_total {
        let p = learner_b.predict(&mut budget_b);
        env_b.step_frame(&mut clean, &mut obs);
        let _ = learner_b.observe(&obs, &mut budget_b);
        preds_b.push(p);
        obs_b.push(obs.clone());
    }

    assert_eq!(report_a.total_substrate_ticks, t_total);
    assert_eq!(report_a.env_steps, t_total);
    assert_eq!(report_a.wake_predictions, preds_b);
    assert_eq!(report_a.wake_observations, obs_b);
    assert_eq!(report_a.budget_consumed, budget_b.consumed());
    let expected_evictions = t_total - (observation_horizon(&econ, &growth) as u64 / 2);
    assert_eq!(
        report_a.sleep_telemetry.fact_buffer_evictions,
        expected_evictions
    );
    assert_eq!(
        report_a.sleep_telemetry,
        arkhe::structural::SleepTelemetry {
            fact_buffer_evictions: expected_evictions,
            ..Default::default()
        }
    );
    assert_eq!(report_a.sleep_telemetry, learner_b.sleep_telemetry());
    assert_eq!(report_a.compute_ledger.wake_substrate_ticks, t_total);
    assert_eq!(report_a.compute_ledger.sleep_loopback_ticks, 0);
    assert_eq!(report_a.compute_ledger.sleep_replay_ticks, 0);
    assert_eq!(report_a.compute_ledger.total_substrate_ticks(), t_total);
    assert_eq!(
        learner_a.editable().circuit().nodes(),
        learner_b.editable().circuit().nodes()
    );
    assert_eq!(
        learner_a.editable().circuit().edges(),
        learner_b.editable().circuit().edges()
    );
    assert_eq!(learner_a.telemetry(), learner_b.telemetry());
    assert_eq!(
        learner_a.editable().state().registers(),
        learner_b.editable().state().registers()
    );
}

#[test]
fn i2_no_comparator_task_skips_sleep_and_matches_wake_only_bit_for_bit() {
    let width = Width::BITS_1;
    let gen_cfg = GeneratorConfig::T1_Constant { value: 1 };
    let econ = EconomyConfig::default();
    let total_ticks = 600u64; // Crosses 256 and 512 W_obs boundaries.

    let run_arm = |schedule| {
        let tier_gen = gen_cfg.build(width).unwrap();
        let channels = tier_gen.channels();
        let mut env = NoisyEnvironment::new(tier_gen, NoiseModel::None, SeedBank::new(7)).unwrap();
        let growth = GrowthConfig {
            sleep_schedule: schedule,
            ..GrowthConfig::default()
        };
        let mut learner = make_learner(width, channels, econ, growth);
        let report = run_wake_sleep_episode(&mut learner, &mut env, total_ticks, 64, 20_000);
        (report, learner)
    };

    let (rep_none, l_none) = run_arm(SleepSchedule::None);
    let (rep_sa, l_sa) = run_arm(SleepSchedule::ConsistencyOnly);
    let (rep_sb, l_sb) = run_arm(SleepSchedule::ConsistencyReplay);

    for (name, rep, learner) in [("S-A", &rep_sa, &l_sa), ("S-B", &rep_sb, &l_sb)] {
        assert_eq!(
            rep.sleep_telemetry.sleep_segments_executed, 0,
            "{name}: must execute 0 sleep segments without comparator"
        );
        assert_eq!(
            rep.sleep_telemetry
                .sleep_segment_skipped_no_scoreable_comparator,
            2,
            "{name}: must record 2 skipped W_wake boundaries"
        );
        assert_eq!(rep.sleep_telemetry.first_onset_wake_tick, None, "{name}");
        assert_eq!(rep.sleep_telemetry.facts_forgone, 0, "{name}");
        assert_eq!(rep.wake_predictions, rep_none.wake_predictions, "{name}");
        assert_eq!(rep.wake_observations, rep_none.wake_observations, "{name}");
        assert_eq!(rep.env_steps, total_ticks, "{name}");
        assert_eq!(rep.compute_ledger, rep_none.compute_ledger, "{name}");
        assert_eq!(
            learner.editable().circuit().nodes(),
            l_none.editable().circuit().nodes(),
            "{name}"
        );
        assert_eq!(learner.telemetry(), l_none.telemetry(), "{name}");
    }
}

#[test]
fn i2_pre_onset_prefix_on_comparator_task_is_bit_identical_to_wake_only() {
    let width = Width::BITS_2;
    let gen_cfg = multi_view_periodic_generator();
    let econ = EconomyConfig::default();
    let total_ticks = 900u64;

    let run_arm = |schedule| {
        let tier_gen = gen_cfg.build(width).unwrap();
        let channels = tier_gen.channels();
        let mut env = NoisyEnvironment::new(tier_gen, NoiseModel::None, SeedBank::new(19)).unwrap();
        let growth = GrowthConfig {
            redundancy_sprouting: true,
            sleep_schedule: schedule,
            ..GrowthConfig::default()
        };
        let mut learner = make_learner(width, channels, econ, growth);
        let report = run_wake_sleep_episode(&mut learner, &mut env, total_ticks, 64, 50_000);
        (report, learner)
    };

    let (rep_none, _l_none) = run_arm(SleepSchedule::None);
    let (rep_sa, l_sa) = run_arm(SleepSchedule::ConsistencyOnly);
    let (rep_sb, l_sb) = run_arm(SleepSchedule::ConsistencyReplay);

    assert!(
        l_sa.active_comparator_count() >= 1 && l_sb.active_comparator_count() >= 1,
        "expected comparators to form on T2_MultiViewCoupled PeriodicMotif"
    );

    for (name, rep) in [("S-A", &rep_sa), ("S-B", &rep_sb)] {
        let onset = rep
            .sleep_telemetry
            .first_onset_wake_tick
            .unwrap_or_else(|| panic!("{name}: expected sleep onset to occur"));
        assert!(onset >= 256, "{name}: onset must be >= W_wake=256");
        let pre_len = onset as usize;
        assert_eq!(
            rep.wake_predictions[..pre_len],
            rep_none.wake_predictions[..pre_len],
            "{name}: pre-onset predictions must be bit-identical to wake-only"
        );
        assert_eq!(
            rep.wake_observations[..pre_len],
            rep_none.wake_observations[..pre_len],
            "{name}: pre-onset observations must be bit-identical to wake-only"
        );
        assert!(
            rep.sleep_telemetry.sleep_segments_executed >= 1,
            "{name}: expected at least one sleep segment after onset"
        );
    }
}

#[test]
fn i2_option_b_onset_gates_sa_leaves_vs_sb_target_and_requires_maturity() {
    let width = Width::BITS_1;
    let econ = EconomyConfig {
        juvenile_ticks: 0,
        ..EconomyConfig::default()
    };

    // 2-channel circuit:
    // - Channel 0: BoundaryIn(0) -> Eqz -> BoundaryOut(0) (predicts alternating 0/1 from lag-1 prev_frame, grounded)
    // - Channel 1: BoundaryOut(1) unconnected (ungrounded)
    // - Spliced comparator compares primary (ch 0 cone) vs alt Delay(BoundaryIn(1)) on target port 0.
    let mut circ = Circuit::new(width, 2, 2, WireDelayMode::Combinational);
    let inv0 = circ.add_node(Primitive::Eqz);
    let d_alt = circ.add_node(Primitive::Delay { init: 0 });
    circ.connect(
        SignalSource::BoundaryIn(0),
        SignalSink::NodeIn {
            node: inv0,
            port: 0,
        },
    );
    circ.connect(
        SignalSource::NodeOut {
            node: inv0,
            port: 0,
        },
        SignalSink::BoundaryOut(0),
    );
    circ.connect(
        SignalSource::BoundaryIn(1),
        SignalSink::NodeIn {
            node: d_alt,
            port: 0,
        },
    );
    let alt_src = SignalSource::NodeOut {
        node: d_alt,
        port: 0,
    };
    let spliced =
        splice_comparator_onto_port(&mut circ, 0, alt_src).expect("splice comparator onto port 0");

    let mut editable = EditableCircuit::new(circ, econ).unwrap();
    editable
        .economy_mut()
        .register_comparator(spliced.sub_node, spliced.eqz_node, 0, 1u64, 1u64 << 1)
        .expect("register comparator");
    let mut learner_sa = ArkheLearner::from_editable(
        editable.clone(),
        GrowthConfig {
            sleep_schedule: SleepSchedule::ConsistencyOnly,
            ..GrowthConfig::default()
        },
        SeedBank::new(1),
    )
    .unwrap();
    let mut learner_sb = ArkheLearner::from_editable(
        editable,
        GrowthConfig {
            sleep_schedule: SleepSchedule::ConsistencyReplay,
            ..GrowthConfig::default()
        },
        SeedBank::new(1),
    )
    .unwrap();

    let mut budget_a = TickBudget::new(100);
    let mut budget_b = TickBudget::new(100);
    for t in 0..20usize {
        let x0 = (t % 2) as u64;
        let frame = [x0, x0];
        let _ = learner_sa.predict(&mut budget_a);
        let _ = learner_sa.observe(&frame, &mut budget_a);
        let _ = learner_sb.predict(&mut budget_b);
        let _ = learner_sb.observe(&frame, &mut budget_b);
    }

    assert!(learner_sa.channel_is_grounded(0));
    assert!(
        !learner_sa.channel_is_grounded(1),
        "unconnected port 1 must NOT be grounded"
    );
    // Option B: S-A requires ALL leaf channels in primary_channel_mask | alt_channel_mask grounded.
    assert!(
        !learner_sa.has_scoreable_comparator_for_sleep(),
        "S-A must reject comparator touching ungrounded leaf channel 1"
    );
    // Option B: S-B requires only target port grounded + comparator mature.
    assert!(
        learner_sb.has_scoreable_comparator_for_sleep(),
        "S-B must accept mature comparator whose target port 0 is grounded"
    );

    // If any node in the comparator cone is immature, BOTH S-A and S-B must return false.
    let sub_node = learner_sb.editable().economy().comparators()[0].sub_node;
    learner_sb
        .editable_mut()
        .economy_mut()
        .node_mut(sub_node.index())
        .unwrap()
        .juvenile_ticks = 1;
    assert!(!learner_sb.has_scoreable_comparator_for_sleep());
}

#[test]
fn i3a_and_i3b_sleep_uses_live_state_and_restores_without_edits() {
    // 2-route agreeing predictor on constant non-zero stream `[2, 2, ...]`:
    // Both `d_prim` and `d_alt` are `Delay { init: 0 }`, hold live register value `2` at sleep
    // entry, emit `2` during all loopback ticks (`first_loopback_predictions == [2]`, distinct from
    // `init = 0`), and keep `q_hat > p_null` under LaplaceFrequency.
    let width = Width::BITS_2;
    let econ = EconomyConfig {
        juvenile_ticks: 0,
        ..EconomyConfig::default()
    };
    let mut circ = Circuit::new(width, 1, 1, WireDelayMode::Combinational);
    let d_prim = circ.add_node(Primitive::Delay { init: 0 });
    let d_alt = circ.add_node(Primitive::Delay { init: 0 });
    circ.connect(
        SignalSource::BoundaryIn(0),
        SignalSink::NodeIn {
            node: d_prim,
            port: 0,
        },
    );
    circ.connect(
        SignalSource::NodeOut {
            node: d_prim,
            port: 0,
        },
        SignalSink::BoundaryOut(0),
    );
    let tap = tap_source(&mut circ, SignalSource::BoundaryIn(0));
    circ.connect(
        tap,
        SignalSink::NodeIn {
            node: d_alt,
            port: 0,
        },
    );
    let alt_src = SignalSource::NodeOut {
        node: d_alt,
        port: 0,
    };
    let spliced =
        splice_comparator_onto_port(&mut circ, 0, alt_src).expect("splice agreeing comparator");

    let mut editable = EditableCircuit::new(circ, econ).unwrap();
    editable
        .economy_mut()
        .register_comparator(spliced.sub_node, spliced.eqz_node, 0, 1u64, 1u64)
        .expect("register comparator");
    for schedule in [
        SleepSchedule::ConsistencyOnly,
        SleepSchedule::ConsistencyReplay,
    ] {
        let mut learner = ArkheLearner::from_editable(
            editable.clone(),
            GrowthConfig {
                sleep_schedule: schedule,
                ..GrowthConfig::default()
            },
            SeedBank::new(2),
        )
        .unwrap();

        let mut budget = TickBudget::new(100);
        for t in 0..20usize {
            let obs = if t % 2 == 0 { [1] } else { [2] };
            let _ = learner.predict(&mut budget);
            let _ = learner.observe(&obs, &mut budget);
        }
        assert!(learner.has_scoreable_comparator_for_sleep());

        let pre_regs = learner.editable().state().registers().to_vec();
        assert_eq!(pre_regs, vec![1, 1]);
        let pre_prev = learner.prev_observed_frame().to_vec();
        assert_eq!(pre_prev, vec![2]);
        let pre_scorers = snapshot_scorers(&learner);
        let pre_epoch = learner.editable().structure_epoch();
        let pre_agree = learner.editable().economy().comparator_agreement_ticks();
        let pre_energies: Vec<f64> = learner
            .editable()
            .economy()
            .nodes()
            .iter()
            .map(|n| n.energy)
            .collect();

        let seg = learner.run_sleep_segment(64);
        assert_eq!(seg.entry_registers, pre_regs);
        // Live DELAY state emits `[1]` on first loopback step (whereas reset-to-init=0 would emit `[0]`).
        assert_eq!(seg.first_loopback_predictions, Some(vec![1]));
        assert_eq!(seg.sleep_excisions, 0);
        if schedule == SleepSchedule::ConsistencyOnly {
            assert_eq!(seg.replay_ticks, 0);
            assert_eq!(seg.loopback_ticks, 64);
        } else {
            // S-B clamps replay_ticks to min(seg_ticks / 2, fact_buffer.len()) = min(32, 20) = 20.
            assert_eq!(seg.replay_ticks, 20);
            assert_eq!(seg.loopback_ticks, 44);
        }

        // I3(b): wake phase, prev_observed_frame, registers, structure_epoch, and scorers restored.
        assert_eq!(learner.editable().economy().phase(), ExecutionPhase::Wake);
        assert_eq!(
            learner.editable().state().registers(),
            pre_regs.as_slice(),
            "{schedule:?}: live registers must restore bit-for-bit"
        );
        assert_eq!(
            learner.prev_observed_frame(),
            pre_prev.as_slice(),
            "{schedule:?}: prev_observed_frame must restore bit-for-bit"
        );
        assert_eq!(learner.editable().structure_epoch(), pre_epoch);
        assert_eq!(snapshot_scorers(&learner), pre_scorers);

        // Economy updates persist across sleep exit.
        assert!(seg.comparator_agreement_ticks > 0);
        assert_eq!(
            learner.editable().economy().comparator_agreement_ticks(),
            pre_agree + seg.comparator_agreement_ticks
        );
        let post_energies: Vec<f64> = learner
            .editable()
            .economy()
            .nodes()
            .iter()
            .map(|n| n.energy)
            .collect();
        assert_ne!(post_energies, pre_energies);
    }
}

#[test]
fn i3c_sleep_excisions_compose_old_to_new_and_remap_surviving_registers() {
    // Subcase 1: 3-route majority pruning with a surviving DELAY node placed AFTER the excised
    // minority comparator arm so its register index shifts during sleep excision!
    {
        let width = Width::BITS_2;
        let econ = EconomyConfig {
            juvenile_ticks: 0,
            ..EconomyConfig::default()
        };
        let mut circ = Circuit::new(width, 2, 2, WireDelayMode::Combinational);
        let d_prim = circ.add_node(Primitive::Delay { init: 0 });
        circ.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn {
                node: d_prim,
                port: 0,
            },
        );
        circ.connect(
            SignalSource::NodeOut {
                node: d_prim,
                port: 0,
            },
            SignalSink::BoundaryOut(0),
        );

        // Agreeing route: outputs 2 (matches primary on `[2, 2, ...]`).
        let d_good = circ.add_node(Primitive::Delay { init: 0 });
        let tap = tap_source(&mut circ, SignalSource::BoundaryIn(0));
        circ.connect(
            tap,
            SignalSink::NodeIn {
                node: d_good,
                port: 0,
            },
        );
        let spliced_good = splice_comparator_onto_port(
            &mut circ,
            0,
            SignalSource::NodeOut {
                node: d_good,
                port: 0,
            },
        )
        .unwrap();

        // Faulty minority alt route BEFORE `d_tail`: BoundaryIn(0) -> d_bad -> Eqz (outputs 0 != 2).
        let d_bad = circ.add_node(Primitive::Delay { init: 0 });
        let eqz_bad = circ.add_node(Primitive::Eqz);
        let tap = tap_source(&mut circ, SignalSource::BoundaryIn(0));
        circ.connect(
            tap,
            SignalSink::NodeIn {
                node: d_bad,
                port: 0,
            },
        );
        circ.connect(
            SignalSource::NodeOut {
                node: d_bad,
                port: 0,
            },
            SignalSink::NodeIn {
                node: eqz_bad,
                port: 0,
            },
        );
        let spliced_bad = splice_comparator_onto_port(
            &mut circ,
            0,
            SignalSource::NodeOut {
                node: eqz_bad,
                port: 0,
            },
        )
        .unwrap();

        // Surviving reachable Delay AFTER `d_bad` whose register index MUST shift left on excision.
        let d_tail = circ.add_node(Primitive::Delay { init: 0 });
        let tap = tap_source(&mut circ, SignalSource::BoundaryIn(0));
        circ.connect(
            tap,
            SignalSink::NodeIn {
                node: d_tail,
                port: 0,
            },
        );
        circ.connect(
            SignalSource::NodeOut {
                node: d_tail,
                port: 0,
            },
            SignalSink::BoundaryOut(1),
        );

        let mut editable = EditableCircuit::new(circ, econ).unwrap();
        editable
            .economy_mut()
            .register_comparator(spliced_good.sub_node, spliced_good.eqz_node, 0, 1u64, 1u64)
            .expect("register good comparator");
        editable
            .economy_mut()
            .register_comparator(spliced_bad.sub_node, spliced_bad.eqz_node, 0, 1u64, 1u64)
            .expect("register bad comparator");
        let mut learner = ArkheLearner::from_editable(
            editable,
            GrowthConfig {
                sleep_schedule: SleepSchedule::ConsistencyOnly,
                ..GrowthConfig::default()
            },
            SeedBank::new(3),
        )
        .unwrap();

        let mut budget = TickBudget::new(100);
        for t in 0..10usize {
            let v = if t % 2 == 0 { 1 } else { 2 };
            let _ = learner.predict(&mut budget);
            let _ = learner.observe(&[v, v], &mut budget);
        }

        let pre_node_count = learner.editable().circuit().node_count();
        let pre_regs = learner.editable().state().registers().to_vec();
        let pre_prev = learner.prev_observed_frame().to_vec();
        let pre_scorers = snapshot_scorers(&learner);
        assert_eq!(pre_regs, vec![1, 1, 1, 1]);
        // Override tail register to distinct marker `3` (reachable/valid BITS_2 value) so we prove
        // index remapping preserved the exact pre-sleep value from slot 3 into its new slot 2!
        let mut seeded_regs = pre_regs.clone();
        seeded_regs[3] = 3;
        learner
            .editable_mut()
            .set_registers(&seeded_regs)
            .expect("seed distinct tail register");

        assert_eq!(learner.active_comparator_count(), 2);
        let seg = learner.run_sleep_segment(64);
        assert!(seg.minority_routes_pruned >= 1);
        assert!(seg.sleep_excisions >= 1);
        assert_eq!(learner.active_comparator_count(), 1);
        assert!(learner.editable().circuit().node_count() < pre_node_count);

        let post_regs = learner.editable().state().registers();
        assert!(
            post_regs.len() < seeded_regs.len(),
            "excised minority Delay must shrink register file so surviving tail register shifts"
        );
        assert_eq!(post_regs[0], 1);
        assert_eq!(post_regs[1], 1);
        assert_eq!(
            *post_regs.last().unwrap(),
            3,
            "d_tail register must remap cleanly from old slot 3 -> new tail slot"
        );
        assert_eq!(learner.editable().economy().phase(), ExecutionPhase::Wake);
        assert_eq!(learner.prev_observed_frame(), pre_prev.as_slice());
        assert_eq!(snapshot_scorers(&learner), pre_scorers);
        assert!(learner.sleep_telemetry().sleep_minority_routes_pruned >= 1);
    }

    // Subcase 2: Starvation excision during sleep removes a depleted Delay node and remaps a
    // subsequent surviving Delay node's distinct live register bit-for-bit.
    {
        let width = Width::BITS_2;
        let econ = EconomyConfig {
            juvenile_ticks: 0,
            ..EconomyConfig::default()
        };
        let mut circ = Circuit::new(width, 2, 2, WireDelayMode::Combinational);
        let d_keep = circ.add_node(Primitive::Delay { init: 0 });
        circ.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn {
                node: d_keep,
                port: 0,
            },
        );
        circ.connect(
            SignalSource::NodeOut {
                node: d_keep,
                port: 0,
            },
            SignalSink::BoundaryOut(0),
        );

        let d_alt = circ.add_node(Primitive::Delay { init: 0 });
        let tap = tap_source(&mut circ, SignalSource::BoundaryIn(0));
        circ.connect(
            tap,
            SignalSink::NodeIn {
                node: d_alt,
                port: 0,
            },
        );
        let spliced_alt = splice_comparator_onto_port(
            &mut circ,
            0,
            SignalSource::NodeOut {
                node: d_alt,
                port: 0,
            },
        )
        .unwrap();

        // Isolated Delay node in the middle that will starve during sleep without reward flux.
        let d_starve = circ.add_node(Primitive::Delay { init: 0 });
        let tap = tap_source(&mut circ, SignalSource::BoundaryIn(0));
        circ.connect(
            tap,
            SignalSink::NodeIn {
                node: d_starve,
                port: 0,
            },
        );

        // Surviving reachable Delay node AFTER `d_starve` whose register index MUST shift left on excision.
        let d_after = circ.add_node(Primitive::Delay { init: 0 });
        let tap = tap_source(&mut circ, SignalSource::BoundaryIn(0));
        circ.connect(
            tap,
            SignalSink::NodeIn {
                node: d_after,
                port: 0,
            },
        );
        circ.connect(
            SignalSource::NodeOut {
                node: d_after,
                port: 0,
            },
            SignalSink::BoundaryOut(1),
        );

        let mut editable = EditableCircuit::new(circ, econ).unwrap();
        editable
            .economy_mut()
            .register_comparator(spliced_alt.sub_node, spliced_alt.eqz_node, 0, 1u64, 1u64)
            .expect("register alt comparator");
        let mut learner = ArkheLearner::from_editable(
            editable,
            GrowthConfig {
                sleep_schedule: SleepSchedule::ConsistencyOnly,
                ..GrowthConfig::default()
            },
            SeedBank::new(4),
        )
        .unwrap();

        let mut budget = TickBudget::new(100);
        for t in 0..20usize {
            let v = if t % 2 == 0 { 1 } else { 2 };
            let _ = learner.predict(&mut budget);
            let _ = learner.observe(&[v, v], &mut budget);
        }
        // Keep d_keep / d_alt / comparator / d_after healthy and starve d_starve (`energy == 0.0`).
        for n in learner.editable_mut().economy_mut().nodes_mut() {
            n.energy = 100.0;
        }
        learner
            .editable_mut()
            .economy_mut()
            .node_mut(d_starve.index())
            .unwrap()
            .energy = 0.0;

        let mut seeded_regs = learner.editable().state().registers().to_vec();
        assert_eq!(seeded_regs, vec![1, 1, 1, 1]);
        seeded_regs[3] = 3;
        learner
            .editable_mut()
            .set_registers(&seeded_regs)
            .expect("seed distinct d_after register");

        let pre_node_count = learner.editable().circuit().node_count();
        let pre_prev = learner.prev_observed_frame().to_vec();
        let pre_scorers = snapshot_scorers(&learner);

        let seg = learner.run_sleep_segment(64);
        assert!(seg.starvation_excisions >= 1);
        assert!(seg.sleep_excisions >= 1);
        assert!(learner.editable().circuit().node_count() < pre_node_count);

        let post_regs = learner.editable().state().registers();
        assert_eq!(post_regs.len(), 3, "d_starve Delay must be excised");
        assert_eq!(post_regs[0], 1);
        assert_eq!(post_regs[1], 1);
        assert_eq!(
            post_regs[2], 3,
            "d_after register must remap cleanly from slot 3 -> slot 2"
        );
        assert_eq!(learner.editable().economy().phase(), ExecutionPhase::Wake);
        assert_eq!(learner.prev_observed_frame(), pre_prev.as_slice());
        assert_eq!(snapshot_scorers(&learner), pre_scorers);
        assert!(learner.sleep_telemetry().sleep_starvation_excisions >= 1);
    }
}

#[test]
fn i5_equal_compute_substrate_tick_equality_and_node_eval_audit_ledger() {
    let width = Width::BITS_2;
    let gen_cfg = multi_view_periodic_generator();
    let econ = EconomyConfig::default();
    let w_obs = observation_horizon(&econ, &GrowthConfig::default()) as u64;
    assert_eq!(
        derived_wake_sleep_windows(&econ, &GrowthConfig::default()),
        (w_obs, w_obs)
    );

    // 1. Multiple of W_obs (4 * W_obs = 1024 substrate ticks).
    let t_mult = 4 * w_obs;
    let run_arm = |schedule, total_ticks| {
        let tier_gen = gen_cfg.build(width).unwrap();
        let channels = tier_gen.channels();
        let mut env = NoisyEnvironment::new(tier_gen, NoiseModel::None, SeedBank::new(23)).unwrap();
        let growth = GrowthConfig {
            redundancy_sprouting: true,
            sleep_schedule: schedule,
            ..GrowthConfig::default()
        };
        let mut learner = make_learner(width, channels, econ, growth);
        run_wake_sleep_episode(&mut learner, &mut env, total_ticks, 64, 50_000)
    };

    let rep_none = run_arm(SleepSchedule::None, t_mult);
    let rep_sa = run_arm(SleepSchedule::ConsistencyOnly, t_mult);
    let rep_sb = run_arm(SleepSchedule::ConsistencyReplay, t_mult);

    for (name, rep) in [("None", &rep_none), ("S-A", &rep_sa), ("S-B", &rep_sb)] {
        let l = rep.compute_ledger;
        assert_eq!(
            l.total_substrate_ticks(),
            t_mult,
            "{name}: T_total diff != 0"
        );
        assert_eq!(rep.total_substrate_ticks, t_mult, "{name}");
        assert_eq!(rep.env_steps, l.wake_substrate_ticks, "{name}");
        assert_eq!(
            rep.sleep_telemetry.facts_forgone,
            l.sleep_loopback_ticks + l.sleep_replay_ticks,
            "{name}"
        );
        assert!(l.live_step_node_evals > 0, "{name}: live_step_node_evals");
        assert!(
            l.candidate_replay_node_evals > 0,
            "{name}: candidate_replay_node_evals"
        );
        assert!(
            l.sensitivity_node_evals > 0,
            "{name}: sensitivity_node_evals"
        );
        assert_eq!(
            l.total_node_evals(),
            l.live_step_node_evals + l.candidate_replay_node_evals + l.sensitivity_node_evals,
            "{name}"
        );
    }

    assert!(rep_sa.compute_ledger.sleep_loopback_ticks >= w_obs);
    assert_eq!(rep_sa.compute_ledger.sleep_replay_ticks, 0);
    assert!(rep_sb.compute_ledger.sleep_loopback_ticks > 0);
    assert!(rep_sb.compute_ledger.sleep_replay_ticks > 0);

    // 2. Non-multiple of W_obs forcing a clamped partial final sleep segment.
    let onset = rep_sa
        .sleep_telemetry
        .first_onset_wake_tick
        .expect("S-A must reach onset");
    let t_partial = onset + 73;
    let part_none = run_arm(SleepSchedule::None, t_partial);
    let part_sa = run_arm(SleepSchedule::ConsistencyOnly, t_partial);
    let part_sb = run_arm(SleepSchedule::ConsistencyReplay, t_partial);

    for (name, rep) in [("None", &part_none), ("S-A", &part_sa), ("S-B", &part_sb)] {
        assert_eq!(
            rep.compute_ledger.total_substrate_ticks(),
            t_partial,
            "{name}: partial T_total diff != 0"
        );
        assert_eq!(
            rep.env_steps, rep.compute_ledger.wake_substrate_ticks,
            "{name}"
        );
    }
    assert_eq!(part_sa.segment_reports.len(), 1);
    assert_eq!(part_sa.segment_reports[0].ticks, 73);
    assert_eq!(part_sa.compute_ledger.sleep_loopback_ticks, 73);
    assert_eq!(part_sa.compute_ledger.sleep_replay_ticks, 0);

    assert_eq!(part_sb.segment_reports.len(), 1);
    assert_eq!(part_sb.segment_reports[0].ticks, 73);
    assert_eq!(part_sb.compute_ledger.sleep_replay_ticks, 36);
    assert_eq!(part_sb.compute_ledger.sleep_loopback_ticks, 37);

    // LFSR generator smoke check ensures LFSR task compiles cleanly too.
    let _ = multi_view_lfsr_generator();
}

#[test]
fn r6_fact_buffer_capacity_half_horizon_and_evictions_under_all_schedules() {
    let econ = EconomyConfig::default();
    let growth = GrowthConfig::default();
    let w_obs = observation_horizon(&econ, &growth);
    assert_eq!(w_obs, 256);

    let learner = make_learner(Width::BITS_1, 2, econ, growth);
    assert_eq!(learner.fact_buffer().capacity(), w_obs / 2);
    assert_eq!(learner.fact_buffer().capacity(), 128);
    assert!(learner.fact_buffer().is_empty());

    // Custom horizon also derives exact W_obs / 2 without hardcoding 128.
    let custom_econ = EconomyConfig {
        juvenile_ticks: 16,
        ..econ
    };
    let custom_growth = GrowthConfig {
        diagnostic_window: 12,
        deepening_max_lag: 4,
        ..growth
    };
    let custom_w_obs = observation_horizon(&custom_econ, &custom_growth);
    assert_eq!(custom_w_obs, 40);
    let mut custom_learner = make_learner(Width::BITS_1, 1, custom_econ, custom_growth);
    assert_eq!(custom_learner.fact_buffer().capacity(), 20);

    let mut budget = TickBudget::new(1_000);
    for t in 0..35u64 {
        let _ = custom_learner.predict(&mut budget);
        let _ = custom_learner.observe(&[t % 2], &mut budget);
    }
    assert_eq!(custom_learner.fact_buffer().len(), 20);
    assert_eq!(custom_learner.fact_buffer().evictions(), 15);
    assert_eq!(custom_learner.sleep_telemetry().fact_buffer_evictions, 15);
    assert_eq!(custom_learner.fact_buffer().start_tick(), 15);
    assert_eq!(custom_learner.fact_buffer().prior_frame(), &[14 % 2]);
    assert_eq!(
        custom_learner.fact_buffer().frame_at(0),
        Some(&[15 % 2][..])
    );

    // Under SleepSchedule::None, FactBuffer records and evicts without changing wake predictions.
    let manual_fb = FactBuffer::new(1, 20);
    assert_eq!(manual_fb.capacity(), 20);
}

#[test]
fn i7_and_r4_suspended_comparators_in_sa_receive_zero_flux_agreements_disagreements_and_excisions()
{
    let width = Width::BITS_1;
    let econ = EconomyConfig {
        juvenile_ticks: 0,
        reserve_cap: ReserveCap::Uncapped,
        ..EconomyConfig::default()
    };

    // Build 2-channel circuit:
    // - Port 0 (grounded): BoundaryIn(0) -> Eqz -> BoundaryOut(0) (predicts alternating 0/1).
    // - Port 1 (ungrounded): BoundaryIn(1) -> Delay(0) -> BoundaryOut(1) when driven with unpredictable
    //   data, OR unconnected. Here we drive BoundaryOut(1) with a bad constant-0 predictor on alternating
    //   input so q_hat <= p_null (ungrounded!) while emitting predictions != 0 or changing inputs!
    let mut circ = Circuit::new(width, 2, 2, WireDelayMode::Combinational);
    let inv0 = circ.add_node(Primitive::Eqz);
    circ.connect(
        SignalSource::BoundaryIn(0),
        SignalSink::NodeIn {
            node: inv0,
            port: 0,
        },
    );
    circ.connect(
        SignalSource::NodeOut {
            node: inv0,
            port: 0,
        },
        SignalSink::BoundaryOut(0),
    );

    // Bad predictor on port 1: BoundaryIn(1) -> BoundaryOut(1) (on alternating 0/1 stream, lag-1 identity is 100% wrong -> !grounded!).
    let tap1 = tap_source(&mut circ, SignalSource::BoundaryIn(1));
    circ.connect(tap1, SignalSink::BoundaryOut(1));

    // Suspended comparator on port 0: compares primary (ch 0) vs alt touching ungrounded channel 1 (`d_alt`).
    let d_alt_ungrounded = circ.add_node(Primitive::Delay { init: 0 });
    let tap1_alt = tap_source(&mut circ, SignalSource::BoundaryIn(1));
    circ.connect(
        tap1_alt,
        SignalSink::NodeIn {
            node: d_alt_ungrounded,
            port: 0,
        },
    );
    let sp_susp = splice_comparator_onto_port(
        &mut circ,
        0,
        SignalSource::NodeOut {
            node: d_alt_ungrounded,
            port: 0,
        },
    )
    .expect("splice suspended comparator");

    // Second comparator on port 0: grounded agreeing route on channel 0 (`inv_ok`), so 2 comparators exist
    // on port 0 (would trigger 3-route majority pruning if suspended comparator weren't excluded!).
    let inv_ok = circ.add_node(Primitive::Eqz);
    let tap0 = tap_source(&mut circ, SignalSource::BoundaryIn(0));
    circ.connect(
        tap0,
        SignalSink::NodeIn {
            node: inv_ok,
            port: 0,
        },
    );
    let sp_ok = splice_comparator_onto_port(
        &mut circ,
        0,
        SignalSource::NodeOut {
            node: inv_ok,
            port: 0,
        },
    )
    .expect("splice grounded comparator");

    let mut editable = EditableCircuit::new(circ, econ).unwrap();
    editable
        .register_comparator(sp_susp.sub_node, sp_susp.eqz_node, 0, 1u64, 1u64 << 1)
        .expect("register suspended comparator");
    editable
        .register_comparator(sp_ok.sub_node, sp_ok.eqz_node, 0, 1u64, 1u64)
        .expect("register grounded comparator");

    let mut learner = ArkheLearner::from_editable(
        editable,
        GrowthConfig {
            sleep_schedule: SleepSchedule::ConsistencyOnly,
            ..GrowthConfig::default()
        },
        SeedBank::new(11),
    )
    .unwrap();

    let mut budget = TickBudget::new(1_000);
    for t in 0..32usize {
        let b = (t % 2) as u64;
        let _ = learner.predict(&mut budget);
        let _ = learner.observe(&[b, b], &mut budget);
    }
    assert!(learner.channel_is_grounded(0));
    assert!(!learner.channel_is_grounded(1));

    // Deplete d_alt_ungrounded to 0.0 to also verify suspended comparator is excluded from sleep starvation excision!
    learner
        .editable_mut()
        .economy_mut()
        .node_mut(d_alt_ungrounded.index())
        .unwrap()
        .energy = 0.0;

    let pre_d_alt_energy = learner.editable().economy().nodes()[d_alt_ungrounded.index()].energy;
    let pre_sub_energy = learner.editable().economy().nodes()[sp_susp.sub_node.index()].energy;
    let pre_eqz_energy = learner.editable().economy().nodes()[sp_susp.eqz_node.index()].energy;
    let pre_comp_count = learner.active_comparator_count();
    let pre_node_count = learner.editable().circuit().node_count();

    let seg = learner.run_sleep_segment(64);
    assert_eq!(seg.suspended_comparator_ticks, 64);
    assert_eq!(
        learner
            .sleep_telemetry()
            .sleep_comparator_suspended_ungrounded,
        64
    );
    // Suspended comparator must NOT be excised (by 3-route pruning OR starvation excision).
    assert_eq!(seg.sleep_excisions, 0);
    assert_eq!(seg.minority_routes_pruned, 0);
    assert_eq!(seg.starvation_excisions, 0);
    assert_eq!(learner.active_comparator_count(), pre_comp_count);
    assert_eq!(learner.editable().circuit().node_count(), pre_node_count);

    // Suspended comparator nodes receive exact 0.0 flux (energies bit-identical).
    assert_eq!(
        learner.editable().economy().nodes()[d_alt_ungrounded.index()]
            .energy
            .to_bits(),
        pre_d_alt_energy.to_bits()
    );
    assert_eq!(
        learner.editable().economy().nodes()[sp_susp.sub_node.index()]
            .energy
            .to_bits(),
        pre_sub_energy.to_bits()
    );
    assert_eq!(
        learner.editable().economy().nodes()[sp_susp.eqz_node.index()]
            .energy
            .to_bits(),
        pre_eqz_energy.to_bits()
    );
    // Only the grounded comparator agrees (exactly 64 agreement ticks, 0 disagreements).
    assert_eq!(seg.comparator_agreement_ticks, 64);
    assert_eq!(seg.comparator_disagreement_ticks, 0);

    // Also test circuit with ONLY a suspended comparator: exact 0.0 flux, 0 agreements, 0 disagreements.
    let mut circ_only_susp = Circuit::new(width, 2, 2, WireDelayMode::Combinational);
    let inv_p = circ_only_susp.add_node(Primitive::Eqz);
    circ_only_susp.connect(
        SignalSource::BoundaryIn(0),
        SignalSink::NodeIn {
            node: inv_p,
            port: 0,
        },
    );
    circ_only_susp.connect(
        SignalSource::NodeOut {
            node: inv_p,
            port: 0,
        },
        SignalSink::BoundaryOut(0),
    );
    let d_u = circ_only_susp.add_node(Primitive::Delay { init: 0 });
    circ_only_susp.connect(
        SignalSource::BoundaryIn(1),
        SignalSink::NodeIn { node: d_u, port: 0 },
    );
    let sp = splice_comparator_onto_port(
        &mut circ_only_susp,
        0,
        SignalSource::NodeOut { node: d_u, port: 0 },
    )
    .unwrap();
    let mut ed_only = EditableCircuit::new(circ_only_susp, econ).unwrap();
    ed_only
        .register_comparator(sp.sub_node, sp.eqz_node, 0, 1, 2)
        .unwrap();
    let mut l_only = ArkheLearner::from_editable(
        ed_only,
        GrowthConfig {
            sleep_schedule: SleepSchedule::ConsistencyOnly,
            ..GrowthConfig::default()
        },
        SeedBank::new(12),
    )
    .unwrap();
    let mut b2 = TickBudget::new(1_000);
    for t in 0..20usize {
        let _ = l_only.predict(&mut b2);
        let _ = l_only.observe(&[(t % 2) as u64, (t % 2) as u64], &mut b2);
    }
    let e_before: Vec<u64> = l_only
        .editable()
        .economy()
        .nodes()
        .iter()
        .map(|n| n.energy.to_bits())
        .collect();
    let seg_only = l_only.run_sleep_segment(32);
    let e_after: Vec<u64> = l_only
        .editable()
        .economy()
        .nodes()
        .iter()
        .map(|n| n.energy.to_bits())
        .collect();
    assert_eq!(seg_only.sleep_comparator_flux, 0.0);
    assert_eq!(seg_only.comparator_agreement_ticks, 0);
    assert_eq!(seg_only.comparator_disagreement_ticks, 0);
    assert_eq!(seg_only.suspended_comparator_ticks, 32);
    assert_eq!(e_before, e_after);
}

#[test]
fn i6_sleep_zero_boundary_reward_and_frozen_wake_null_models_and_scorers() {
    let width = Width::BITS_2;
    let econ = EconomyConfig {
        juvenile_ticks: 0,
        reserve_cap: ReserveCap::Uncapped,
        ..EconomyConfig::default()
    };

    for schedule in [
        SleepSchedule::ConsistencyOnly,
        SleepSchedule::ConsistencyReplay,
    ] {
        // 1. No comparators: boundary reward is 0.0 throughout sleep, all node energies & scorers untouched.
        let mut circ = Circuit::new(width, 1, 1, WireDelayMode::Combinational);
        let d0 = circ.add_node(Primitive::Delay { init: 0 });
        circ.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: d0, port: 0 },
        );
        circ.connect(
            SignalSource::NodeOut { node: d0, port: 0 },
            SignalSink::BoundaryOut(0),
        );
        let ed = EditableCircuit::new(circ, econ).unwrap();
        let mut l0 = ArkheLearner::from_editable(
            ed,
            GrowthConfig {
                sleep_schedule: schedule,
                ..GrowthConfig::default()
            },
            SeedBank::new(5),
        )
        .unwrap();
        let mut b = TickBudget::new(1_000);
        for t in 0..24usize {
            let _ = l0.predict(&mut b);
            let _ = l0.observe(&[(t % 4) as u64], &mut b);
        }
        let pre_e: Vec<u64> = l0
            .editable()
            .economy()
            .nodes()
            .iter()
            .map(|n| n.energy.to_bits())
            .collect();
        let pre_scorers = snapshot_scorers(&l0);
        let seg0 = l0.run_sleep_segment(32);
        let post_e: Vec<u64> = l0
            .editable()
            .economy()
            .nodes()
            .iter()
            .map(|n| n.energy.to_bits())
            .collect();
        assert_eq!(seg0.sleep_comparator_flux, 0.0);
        assert_eq!(pre_e, post_e, "{schedule:?}: boundary reward must be 0");
        assert_eq!(snapshot_scorers(&l0), pre_scorers);

        // 2. With comparator: wake port scorers AND wake comparator scorers bit-identical before & after sleep.
        let mut circ_c = Circuit::new(width, 1, 1, WireDelayMode::Combinational);
        let d_pri = circ_c.add_node(Primitive::Delay { init: 0 });
        let d_alt = circ_c.add_node(Primitive::Delay { init: 0 });
        circ_c.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn {
                node: d_pri,
                port: 0,
            },
        );
        circ_c.connect(
            SignalSource::NodeOut {
                node: d_pri,
                port: 0,
            },
            SignalSink::BoundaryOut(0),
        );
        let tap = tap_source(&mut circ_c, SignalSource::BoundaryIn(0));
        circ_c.connect(
            tap,
            SignalSink::NodeIn {
                node: d_alt,
                port: 0,
            },
        );
        let sp = splice_comparator_onto_port(
            &mut circ_c,
            0,
            SignalSource::NodeOut {
                node: d_alt,
                port: 0,
            },
        )
        .unwrap();
        let mut ed_c = EditableCircuit::new(circ_c, econ).unwrap();
        ed_c.register_comparator(sp.sub_node, sp.eqz_node, 0, 1, 1)
            .unwrap();
        let mut lc = ArkheLearner::from_editable(
            ed_c,
            GrowthConfig {
                sleep_schedule: schedule,
                ..GrowthConfig::default()
            },
            SeedBank::new(6),
        )
        .unwrap();
        let mut bc = TickBudget::new(1_000);
        for t in 0..32usize {
            let v = if t % 2 == 0 { 1 } else { 2 };
            let _ = lc.predict(&mut bc);
            let _ = lc.observe(&[v], &mut bc);
        }
        let pre_port_scorers = snapshot_scorers(&lc);
        let pre_comp_scorer = &lc.editable().economy().comparators()[0].scorer;
        let pre_comp_snap = (
            pre_comp_scorer.ticks(),
            pre_comp_scorer.matches(),
            pre_comp_scorer.q_hat().to_bits(),
            [
                pre_comp_scorer.null_model().probability(0).to_bits(),
                pre_comp_scorer.null_model().probability(1).to_bits(),
                pre_comp_scorer.null_model().probability(2).to_bits(),
                pre_comp_scorer.null_model().probability(3).to_bits(),
            ],
        );
        let _ = lc.run_sleep_segment(64);
        assert_eq!(snapshot_scorers(&lc), pre_port_scorers);
        let post_comp_scorer = &lc.editable().economy().comparators()[0].scorer;
        let post_comp_snap = (
            post_comp_scorer.ticks(),
            post_comp_scorer.matches(),
            post_comp_scorer.q_hat().to_bits(),
            [
                post_comp_scorer.null_model().probability(0).to_bits(),
                post_comp_scorer.null_model().probability(1).to_bits(),
                post_comp_scorer.null_model().probability(2).to_bits(),
                post_comp_scorer.null_model().probability(3).to_bits(),
            ],
        );
        assert_eq!(post_comp_snap, pre_comp_snap);
    }
}

#[test]
fn r5a_sb_fact_adjudication_excises_wrong_primary_while_ties_default_to_dr40() {
    // Subcase 1: Wrong primary (lag-3 Delay chain on period-6 motif `[0, 1, 2, 0, 1, 3]` at k=2) vs
    // oracle-correct period-6 autonomous ring alt.
    // On `[0, 1, 2, 0, 1, 3]`, lag-3 agrees on 4/6 = 66.7% (> p_null <= 33.3%, unconditionally grounded)
    // and disagrees on 2/6 ticks, while the period-6 ring agrees with facts on 6/6 ticks.
    // In S-A, DR #40 charges alt + SUB + EQZ, so wrong primary is NOT excised.
    // In S-B, R5(a) detects `primary_fact_disagreements > alt_fact_disagreements`, charges primary,
    // starves it, and promotes alt via `excise_port_comparator(m, true)`!
    let width = Width::BITS_2;
    let motif = [0u64, 1, 2, 0, 1, 3];
    let econ = EconomyConfig {
        juvenile_ticks: 0,
        reserve_cap: ReserveCap::Uncapped,
        ..EconomyConfig::default()
    };

    let build_period6_conflict = || {
        let mut circ = Circuit::new(width, 2, 2, WireDelayMode::Combinational);
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

        let (_, _, ring_src) = build_autonomous_ring_source(&mut circ, 6, &motif);
        let sp = splice_comparator_onto_port(&mut circ, 0, ring_src).expect("splice period-6 ring");

        // Tail Delay on port 1 AFTER the primary nodes so register index shifts on primary excision!
        let d_tail = circ.add_node(Primitive::Delay { init: 0 });
        let tap_tail = tap_source(&mut circ, SignalSource::BoundaryIn(0));
        circ.connect(
            tap_tail,
            SignalSink::NodeIn {
                node: d_tail,
                port: 0,
            },
        );
        circ.connect(
            SignalSource::NodeOut {
                node: d_tail,
                port: 0,
            },
            SignalSink::BoundaryOut(1),
        );

        let mut ed = EditableCircuit::new(circ, econ).unwrap();
        ed.register_comparator(sp.sub_node, sp.eqz_node, 0, 1, 0)
            .unwrap();
        (ed, d1, d2, d_tail)
    };

    let run_conflict_case = |schedule| {
        let (ed, d1, d2, d_tail) = build_period6_conflict();
        let mut learner = ArkheLearner::from_editable(
            ed,
            GrowthConfig {
                sleep_schedule: schedule,
                // Keep W_diag > 72 wake ticks so wake Step-A handoff / Step-B refutation
                // does not swap or prune the primary before sleep entry.
                diagnostic_window: 100,
                ..GrowthConfig::default()
            },
            SeedBank::new(31),
        )
        .unwrap();
        let mut regs = learner.editable().state().registers().to_vec();
        // Align d1=y(-2)=motif[4], d2=y(-3)=motif[3] so step 0 emits y(-3)=motif[3]=0=motif[0]!
        regs[0] = motif[4];
        regs[1] = motif[3];
        learner.editable_mut().set_registers(&regs).unwrap();
        learner.set_prev_observed_frame(&[motif[5], motif[5]]);

        let mut b = TickBudget::new(10_000);
        for t in 0..72usize {
            let sym = motif[t % 6];
            let _ = learner.predict(&mut b);
            let _ = learner.observe(&[sym, sym], &mut b);
        }
        assert!(learner.channel_is_grounded(0));
        // Keep alt + sub/eqz well-funded and primary near threshold so disagreement charge starves primary cleanly.
        for (idx, ne) in learner
            .editable_mut()
            .economy_mut()
            .nodes_mut()
            .iter_mut()
            .enumerate()
        {
            if idx != d1.index() && idx != d2.index() {
                ne.energy = 200.0;
            }
        }
        learner
            .editable_mut()
            .economy_mut()
            .node_mut(d2.index())
            .unwrap()
            .energy = 1.5;
        let _ = d_tail;
        let seg = learner.run_sleep_segment(128);
        (learner, seg)
    };

    // In S-B, primary disagrees on facts (~24/72) while alt ring has 0 fact disagreements -> primary excised & alt promoted!
    let (mut l_sb, seg_sb) = run_conflict_case(SleepSchedule::ConsistencyReplay);
    assert!(
        seg_sb.starvation_excisions >= 1,
        "S-B R5(a) must excise wrong primary: {seg_sb:?}"
    );
    assert_eq!(l_sb.active_comparator_count(), 0);
    // Verify promoted alt predicts full 6-cycle `[0, 1, 0, 1, 0, 0]` with 0 errors!
    let mut b_check = TickBudget::new(1_000);
    let mut errs = 0usize;
    for t in 0..12usize {
        let p = l_sb.predict(&mut b_check);
        let expected = motif[t % 6];
        if p[0] != expected {
            errs += 1;
        }
        let _ = l_sb.observe(&[expected, expected], &mut b_check);
    }
    assert_eq!(errs, 0, "promoted alt must predict period-6 motif exactly");

    // In S-A (same setup), primary cannot be excised by 2-route sleep (DR #40 charges alt+SUB+EQZ)!
    let (l_sa, seg_sa) = run_conflict_case(SleepSchedule::ConsistencyOnly);
    assert_eq!(
        seg_sa.sleep_excisions, 0,
        "S-A must not excise wrong primary in 2-route sleep"
    );
    assert_eq!(l_sa.active_comparator_count(), 1);

    // Subcase 2: Tie (`primary_fact_disagreements == alt_fact_disagreements`) defaults to DR #40 (`prune_primary = false`).
    {
        let width2 = Width::BITS_2;
        let mut circ = Circuit::new(width2, 1, 1, WireDelayMode::Combinational);
        // Primary: BoundaryIn(0) -> Delay(0) -> BoundaryOut(0)
        // Alt:     BoundaryIn(0) -> Delay(0) -> Eqz -> sub.in(1)
        // Stream: `[0, 1, 0, 1, ...]`? No: on `[0, 1, 0, 1]` Delay predicts `[1, 0, 1, 0]` (100% wrong)
        // while `Eqz(Delay)` predicts `[0, 1, 0, 1]`? No, at k=2 Eqz(0)=1, Eqz(1)=0.
        // Use `[0, 1, 0, 1]` with Delay(init=0) as primary (predicts `[0, 1, ...]`? At t=0:0, t=1:0, t=2:1, t=3:0...):
        // Better: autonomous rings of period 4!
        // Stream: `[0, 1, 2, 3]`.
        // Primary ring: `[0, 1, 0, 0]` (disagrees with facts at t=2,3: 2/4 = 50%!).
        // Alt ring:     `[2, 3, 2, 3]` (disagrees with facts at t=0,1: 2/4 = 50% — exact TIE!).
        // Wait: in `[0, 1, 2, 3]` null_model is ~0.25, primary hits 2/4 = 0.50 > 0.25 -> grounded!
        // And in loopback, primary (`[0, 1, 0, 0]`) vs alt (`[2, 3, 2, 3]`) ALWAYS disagree (`pri != alt`)!
        //Wait: for negative reward in loopback, sleep_scorer needs q_hat > p_null!
        // So make both rings agree on t=0,1 (`[0, 1, 2, 0]` vs `[0, 1, 0, 3]` on `[0, 1, 2, 3]`):
        //   - Facts:      [0, 1, 2, 3]
        //   - Primary:    [0, 1, 2, 0] (disagrees at t=3 -> 1/4 = 25% fact disagreements, 75% accurate!)
        //   - Alt:        [0, 1, 0, 3] (disagrees at t=2 -> 1/4 = 25% fact disagreements — EXACT TIE!)
        //   - Loopback:   agree on t=0,1 (q_hat rises to ~0.6 > 0.25!), disagree on t=2,3 (negative reward!)
        let (_, _, pri_src) = build_autonomous_ring_source(&mut circ, 4, &[0, 1, 2, 0]);
        circ.connect(pri_src, SignalSink::BoundaryOut(0));
        let (_, _, alt_src) = build_autonomous_ring_source(&mut circ, 4, &[0, 1, 0, 3]);
        let sp = splice_comparator_onto_port(&mut circ, 0, alt_src).unwrap();

        let mut ed = EditableCircuit::new(circ, econ).unwrap();
        ed.register_comparator(sp.sub_node, sp.eqz_node, 0, 1 << 60, 1 << 61)
            .unwrap();
        let mut learner = ArkheLearner::from_editable(
            ed,
            GrowthConfig {
                sleep_schedule: SleepSchedule::ConsistencyReplay,
                ..GrowthConfig::default()
            },
            SeedBank::new(32),
        )
        .unwrap();
        let mut b = TickBudget::new(2_000);
        for t in 0..32usize {
            let _ = learner.predict(&mut b);
            let _ = learner.observe(&[(t % 4) as u64], &mut b);
        }
        assert!(learner.channel_is_grounded(0));
        // Equalize small initial energies so whichever side DR #40 charges starves cleanly.
        for ne in learner.editable_mut().economy_mut().nodes_mut() {
            ne.energy = 1.5;
        }
        let seg = learner.run_sleep_segment(64);
        assert!(
            seg.starvation_excisions >= 1,
            "tie must still excise under DR #40"
        );
        assert_eq!(learner.active_comparator_count(), 0);
        // Primary (`[0, 1, 2, 0]`) must be preserved on tie (`alt` excised with `prune_primary = false`)!
        let mut preds_after = Vec::new();
        for _ in 0..4 {
            preds_after.push(learner.predict(&mut b)[0]);
            let _ = learner.observe(&[0], &mut b);
        }
        assert_eq!(preds_after, vec![0, 1, 2, 0]);
    }
}

#[test]
fn r5b_sb_kt_match_test_pass_and_fail_voids_positive_flux_only_preserving_i4() {
    // Unit verification of exact order-d_cone two-sample KT test.
    let s_0101 = [0u64, 1, 0, 1];
    let s_0011 = [0u64, 0, 1, 1];
    assert!(two_sample_kt_match_test(&s_0101, &s_0101, 2, Width::BITS_1).passed);
    assert!(two_sample_kt_match_test(&s_0101, &s_0011, 0, Width::BITS_1).passed);
    assert!(!two_sample_kt_match_test(&s_0101, &s_0011, 1, Width::BITS_1).passed);

    let econ = EconomyConfig {
        juvenile_ticks: 0,
        reserve_cap: ReserveCap::Uncapped,
        ..EconomyConfig::default()
    };

    // Case 1: Passing S-B port releases positive agreement flux cleanly and passes I4.
    {
        let mut circ = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);
        let (_, _, pri) = build_autonomous_ring_source(&mut circ, 4, &[0, 1, 0, 1]);
        circ.connect(pri, SignalSink::BoundaryOut(0));
        let (_, _, alt) = build_autonomous_ring_source(&mut circ, 4, &[0, 1, 0, 1]);
        let sp = splice_comparator_onto_port(&mut circ, 0, alt).unwrap();
        let mut ed = EditableCircuit::new(circ, econ).unwrap();
        ed.register_comparator(sp.sub_node, sp.eqz_node, 0, 1 << 60, 1 << 61)
            .unwrap();
        let mut learner = ArkheLearner::from_editable(
            ed,
            GrowthConfig {
                sleep_schedule: SleepSchedule::ConsistencyReplay,
                sprout_interval: 1_000_000,
                redundancy_sprouting: false,
                ..GrowthConfig::default()
            },
            SeedBank::new(41),
        )
        .unwrap();
        let mut env = NoisyEnvironment::new(
            GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 1, 0, 1],
            }
            .build(Width::BITS_1)
            .unwrap(),
            NoiseModel::None,
            SeedBank::new(41),
        )
        .unwrap();
        let (rep, cons) = run_wake_sleep_episode_audited(&mut learner, &mut env, 512, 0, 10_000);
        assert!(rep.sleep_telemetry.sleep_segments_executed >= 1);
        assert_eq!(rep.sleep_telemetry.sleep_match_failures, 0);
        assert_eq!(rep.segment_reports[0].voided_agreement_flux_bits, 0.0);
        assert!(rep.segment_reports[0].sleep_comparator_flux > 0.0);
        assert!(
            cons.flux_mdl_err_bits_per_symbol <= 1e-11,
            "flux_mdl_err={}",
            cons.flux_mdl_err_bits_per_symbol
        );
        assert!(
            cons.energy_flux_err_bits_per_symbol <= 1e-11,
            "energy_flux_err={}",
            cons.energy_flux_err_bits_per_symbol
        );
        assert!(
            cons.direct_i4_err_bits_per_symbol.unwrap() <= 1e-11,
            "direct_i4={:?}",
            cons.direct_i4_err_bits_per_symbol
        );
    }

    // Case 2: Failing S-B port (autonomous `[0, 1, 0, 1]` free-run vs `[0, 0, 1, 1]` replayed facts;
    // identical order-0 marginals, rejected at order d_cone >= 1!) voids positive agreement flux
    // while keeping disagreement charges intact and preserving exact I4 conservation!
    {
        let mut circ = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);
        let (_, _, pri) = build_autonomous_ring_source(&mut circ, 4, &[0, 1, 0, 1]);
        circ.connect(pri, SignalSink::BoundaryOut(0));
        // Comp 0: agrees with primary (`[0, 1, 0, 1]`), generating escrowed positive agreement flux!
        let (_, _, alt_agree) = build_autonomous_ring_source(&mut circ, 4, &[0, 1, 0, 1]);
        let sp_agree = splice_comparator_onto_port(&mut circ, 0, alt_agree).unwrap();
        // Comp 1: agrees on `[0, 1, 0, ...]` but disagrees at index 3 (`[0, 1, 0, 0]`), generating a
        // real disagreement charge that must NOT be voided when port 0 fails R5(b)!
        let (_, _, alt_dis) = build_autonomous_ring_source(&mut circ, 4, &[0, 1, 0, 0]);
        let sp_dis = splice_comparator_onto_port(&mut circ, 0, alt_dis).unwrap();

        assert!(port_cone_register_depth(&circ, 0) >= 1);

        let mut ed = EditableCircuit::new(circ, econ).unwrap();
        ed.register_comparator(sp_agree.sub_node, sp_agree.eqz_node, 0, 1 << 60, 1 << 61)
            .unwrap();
        ed.register_comparator(sp_dis.sub_node, sp_dis.eqz_node, 0, 1 << 60, 1 << 62)
            .unwrap();
        // Fund nodes generously so no excision blurs the flux breakdown check.
        for ne in ed.economy_mut().nodes_mut() {
            ne.energy = 1_000.0;
        }
        let mut learner = ArkheLearner::from_editable(
            ed,
            GrowthConfig {
                sleep_schedule: SleepSchedule::ConsistencyReplay,
                diagnostic_window: 32, // W_obs = 64 -> sleep triggers after 64 wake ticks
                sprout_interval: 1_000_000,
                redundancy_sprouting: false,
                // Prevent 3-route majority pruning during this 64-tick segment so both positive and
                // negative fluxes stay cleanly isolated.
                min_match_gain: 100,
                ..GrowthConfig::default()
            },
            SeedBank::new(42),
        )
        .unwrap();

        // Build wake q_hat >> 0.5 on 96 ticks of `[0, 1, 0, 1]`, then run an audited 128-tick
        // episode (64 wake ticks on `[0, 0, 1, 1]` + 64 sleep ticks) so FactBuffer (`W_obs/2 = 32`)
        // holds pure `[0, 0, 1, 1]` while `primary` & `alt_agree` free-run `[0, 1, 0, 1]`!
        let mut b = TickBudget::new(10_000);
        for t in 0..96usize {
            let _ = learner.predict(&mut b);
            let _ = learner.observe(&[(t % 2) as u64], &mut b);
        }
        let mut env_0011 = NoisyEnvironment::new(
            GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 0, 1, 1],
            }
            .build(Width::BITS_1)
            .unwrap(),
            NoiseModel::None,
            SeedBank::new(42),
        )
        .unwrap();

        let pre_sleep_energy: f64 = learner
            .editable()
            .economy()
            .nodes()
            .iter()
            .map(|n| n.energy)
            .sum();
        let pre_sleep_flux = learner.cumulative_net_flux();

        let (rep, cons) =
            run_wake_sleep_episode_audited(&mut learner, &mut env_0011, 128, 0, 10_000);
        assert_eq!(rep.segment_reports.len(), 1);
        let seg = &rep.segment_reports[0];
        assert_eq!(seg.replay_ticks, 32);
        assert_eq!(seg.loopback_ticks, 32);
        assert_eq!(seg.match_failures, 1);
        assert_eq!(learner.sleep_telemetry().sleep_match_failures, 1);
        assert!(
            seg.voided_agreement_flux_bits > 0.0,
            "positive agreement flux on failing port must be voided: {seg:?}"
        );
        // Disagreement charges from comp 1 must remain intact (< 0.0)!
        assert!(
            seg.sleep_comparator_flux < 0.0,
            "disagreement charges must persist when positive flux is voided: {seg:?}"
        );
        assert!(
            cons.flux_mdl_err_bits_per_symbol <= 1e-11,
            "R5(b) fail flux_mdl_err={}",
            cons.flux_mdl_err_bits_per_symbol
        );
        assert!(
            cons.energy_flux_err_bits_per_symbol <= 1e-11,
            "R5(b) fail energy_flux_err={}",
            cons.energy_flux_err_bits_per_symbol
        );
        assert!(
            cons.direct_i4_err_bits_per_symbol.unwrap() <= 1e-11,
            "R5(b) fail direct_i4={:?}",
            cons.direct_i4_err_bits_per_symbol
        );
        let post_sleep_energy: f64 = learner
            .editable()
            .economy()
            .nodes()
            .iter()
            .map(|n| n.energy)
            .sum();
        let post_sleep_flux = learner.cumulative_net_flux();
        assert!(
            ((post_sleep_energy - pre_sleep_energy) - (post_sleep_flux - pre_sleep_flux)).abs()
                <= 1e-11
        );
    }
}

#[test]
fn i4_epistemic_mdl_conservation_across_multi_segment_wake_sleep_runs() {
    let econ_uncapped_juv0 = EconomyConfig {
        juvenile_ticks: 0,
        reserve_cap: ReserveCap::Uncapped,
        ..EconomyConfig::default()
    };
    let econ_uncapped_default_juv = EconomyConfig {
        reserve_cap: ReserveCap::Uncapped,
        ..EconomyConfig::default()
    };
    let econ_default = EconomyConfig::default();

    // 1. Multi-segment wake+sleep conservation for None, S-A, and S-B on multi-view periodic task
    //    with dynamic Stage-2 + Stage-3 sprouting and excisions (both Uncapped and Default/HorizonDerived).
    for (label, econ) in [
        ("UncappedDefaultJuv", econ_uncapped_default_juv),
        ("Default", econ_default),
    ] {
        for schedule in [
            SleepSchedule::None,
            SleepSchedule::ConsistencyOnly,
            SleepSchedule::ConsistencyReplay,
        ] {
            let width = Width::BITS_2;
            let gen_cfg = multi_view_periodic_generator();
            let tier_gen = gen_cfg.build(width).unwrap();
            let channels = tier_gen.channels();
            let mut env =
                NoisyEnvironment::new(tier_gen, NoiseModel::None, SeedBank::new(19)).unwrap();
            let growth = GrowthConfig {
                redundancy_sprouting: true,
                sleep_schedule: schedule,
                ..GrowthConfig::default()
            };
            let mut learner = make_learner(width, channels, econ, growth);
            let (rep, cons) =
                run_wake_sleep_episode_audited(&mut learner, &mut env, 1_024, 64, 80_000);
            if schedule != SleepSchedule::None {
                assert!(
                    rep.sleep_telemetry.sleep_segments_executed >= 1,
                    "{label}/{schedule:?}: expected sleep execution"
                );
            }
            assert!(
                cons.flux_mdl_err_bits_per_symbol <= 1e-11,
                "{label}/{schedule:?}: flux_mdl_err = {:.3e}",
                cons.flux_mdl_err_bits_per_symbol
            );
            assert!(
                cons.energy_flux_err_bits_per_symbol <= 1e-11,
                "{label}/{schedule:?}: energy_flux_err = {:.3e}",
                cons.energy_flux_err_bits_per_symbol
            );
        }
    }

    // 2. Direct `ΔE == -ΔL_null^epistemic + E_exog` (without pruning/debt/clip) across multi-segment
    //    S-A (grounded + suspended ungrounded), S-B (R5(b) pass), and S-B (R5(b) fail/voided).
    for schedule in [
        SleepSchedule::None,
        SleepSchedule::ConsistencyOnly,
        SleepSchedule::ConsistencyReplay,
    ] {
        let width = Width::BITS_1;
        let mut circ = Circuit::new(width, 3, 3, WireDelayMode::Combinational);
        let inv0 = circ.add_node(Primitive::Eqz);
        circ.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn {
                node: inv0,
                port: 0,
            },
        );
        circ.connect(
            SignalSource::NodeOut {
                node: inv0,
                port: 0,
            },
            SignalSink::BoundaryOut(0),
        );
        // Grounded comparator on ch 0
        let inv_alt = circ.add_node(Primitive::Eqz);
        let tap0 = tap_source(&mut circ, SignalSource::BoundaryIn(0));
        circ.connect(
            tap0,
            SignalSink::NodeIn {
                node: inv_alt,
                port: 0,
            },
        );
        let sp_g = splice_comparator_onto_port(
            &mut circ,
            0,
            SignalSource::NodeOut {
                node: inv_alt,
                port: 0,
            },
        )
        .unwrap();
        // Ungrounded-leaf comparator on ch 1 (suspended in S-A, replayed in S-B)
        let d_u = circ.add_node(Primitive::Delay { init: 0 });
        circ.connect(
            SignalSource::BoundaryIn(1),
            SignalSink::NodeIn { node: d_u, port: 0 },
        );
        let sp_u =
            splice_comparator_onto_port(&mut circ, 0, SignalSource::NodeOut { node: d_u, port: 0 })
                .unwrap();

        let mut ed = EditableCircuit::new(circ, econ_uncapped_juv0).unwrap();
        ed.register_comparator(sp_g.sub_node, sp_g.eqz_node, 0, 1, 1)
            .unwrap();
        ed.register_comparator(sp_u.sub_node, sp_u.eqz_node, 0, 1, 2)
            .unwrap();
        for ne in ed.economy_mut().nodes_mut() {
            ne.energy = 1_000.0;
        }
        let mut learner = ArkheLearner::from_editable(
            ed,
            GrowthConfig {
                sleep_schedule: schedule,
                min_match_gain: 1_000,
                diagnostic_window: 32,
                sprout_interval: 1_000_000,
                redundancy_sprouting: false,
                ..GrowthConfig::default()
            },
            SeedBank::new(61),
        )
        .unwrap();
        let gen_cfg = GeneratorConfig::T1_PeriodicMotif { motif: vec![0, 1] };
        let tier_gen = gen_cfg.build(width).unwrap();
        // Wrap in 2-channel T2_CrossLinear or 2-channel env: use T2_MultiViewCoupled with periodic driver!
        let _ = tier_gen;
        let mv_gen = GeneratorConfig::T2_MultiViewCoupled {
            driver: MultiViewDriver::PeriodicMotif { motif: vec![0, 1] },
            view_0: MultiViewTransform {
                delay: 0,
                mul_a: 1,
                add_b: 0,
            },
            view_1: MultiViewTransform {
                delay: 0,
                mul_a: 1,
                add_b: 1,
            },
            target: MultiViewTarget::Affine {
                lag: 1,
                mul_a: 1,
                add_b: 0,
            },
        }
        .build(width)
        .unwrap();
        let mut env = NoisyEnvironment::new(mv_gen, NoiseModel::None, SeedBank::new(61)).unwrap();
        let (rep, cons) = run_wake_sleep_episode_audited(&mut learner, &mut env, 320, 0, 5_000);
        if schedule == SleepSchedule::ConsistencyOnly {
            assert!(rep.sleep_telemetry.sleep_comparator_suspended_ungrounded > 0);
        }
        assert!(
            cons.flux_mdl_err_bits_per_symbol <= 1e-11,
            "{schedule:?}: flux_mdl_err={}",
            cons.flux_mdl_err_bits_per_symbol
        );
        assert!(
            cons.energy_flux_err_bits_per_symbol <= 1e-11,
            "{schedule:?}: energy_flux_err={}",
            cons.energy_flux_err_bits_per_symbol
        );
        assert!(
            cons.direct_i4_err_bits_per_symbol.unwrap() <= 1e-11,
            "{schedule:?}: direct_i4={:?}",
            cons.direct_i4_err_bits_per_symbol
        );
    }

    // 3a. S-A sleep excision (3-route majority pruning) under audited I4.
    {
        let width = Width::BITS_1;
        let mut circ = Circuit::new(width, 1, 1, WireDelayMode::Combinational);
        let (_, _, r1) = build_autonomous_ring_source(&mut circ, 2, &[0, 1]);
        let (_, _, r2) = build_autonomous_ring_source(&mut circ, 2, &[0, 1]);
        let (_, _, r_fault) = build_autonomous_ring_source(&mut circ, 2, &[1, 0]);
        circ.connect(r1, SignalSink::BoundaryOut(0));
        let sp_a = splice_comparator_onto_port(&mut circ, 0, r2).unwrap();
        let sp_b = splice_comparator_onto_port(&mut circ, 0, r_fault).unwrap();

        let mut ed = EditableCircuit::new(circ, econ_uncapped_juv0).unwrap();
        ed.register_comparator(sp_a.sub_node, sp_a.eqz_node, 0, 0, 0)
            .unwrap();
        ed.register_comparator(sp_b.sub_node, sp_b.eqz_node, 0, 0, 0)
            .unwrap();
        for ne in ed.economy_mut().nodes_mut() {
            ne.energy = 1_000.0;
        }
        let mut learner = ArkheLearner::from_editable(
            ed,
            GrowthConfig {
                sleep_schedule: SleepSchedule::ConsistencyOnly,
                diagnostic_window: 32,
                // Disagreements = 64 in W_obs=64 wake (< 65 so Step B does not prune during wake),
                // while sleep clamps consensus_thr to loopback_ticks=64 so 3-route majority fires in sleep!
                min_match_gain: 65,
                sprout_interval: 1_000_000,
                redundancy_sprouting: false,
                ..GrowthConfig::default()
            },
            SeedBank::new(70),
        )
        .unwrap();
        let tier_gen = GeneratorConfig::T1_PeriodicMotif { motif: vec![0, 1] }
            .build(width)
            .unwrap();
        let mut env = NoisyEnvironment::new(tier_gen, NoiseModel::None, SeedBank::new(70)).unwrap();
        let (rep, cons) = run_wake_sleep_episode_audited(&mut learner, &mut env, 256, 0, 10_000);
        assert!(
            rep.sleep_telemetry.sleep_minority_routes_pruned >= 1,
            "audited S-A episode must commit 3-route majority excision: {rep:?}"
        );
        assert!(cons.flux_mdl_err_bits_per_symbol <= 1e-11);
        assert!(cons.energy_flux_err_bits_per_symbol <= 1e-11);
    }

    // 3b. S-B R5(a) primary adjudication + primary excision under audited I4 across pre-sleep wake,
    //     run_sleep_segment (R5(a) primary excision + alt promotion), and post-sleep wake.
    {
        let width = Width::BITS_2;
        let motif = [0u64, 1, 2, 0, 1, 3];
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
        let (_, _, ring_src) = build_autonomous_ring_source(&mut circ, 6, &motif);
        let sp = splice_comparator_onto_port(&mut circ, 0, ring_src).unwrap();
        let mut ed = EditableCircuit::new(circ, econ_uncapped_juv0).unwrap();
        ed.register_comparator(sp.sub_node, sp.eqz_node, 0, 1, 0)
            .unwrap();
        for (idx, ne) in ed.economy_mut().nodes_mut().iter_mut().enumerate() {
            if idx != d1.index() && idx != d2.index() {
                ne.energy = 200.0;
            }
        }
        // Start d2 low BEFORE enabling epistemic audit so zero manual energy edits happen post-audit.
        ed.economy_mut().node_mut(d2.index()).unwrap().energy = 0.01;

        let mut learner = ArkheLearner::from_editable(
            ed,
            GrowthConfig {
                sleep_schedule: SleepSchedule::ConsistencyReplay,
                // Keep W_diag > 72 pre-sleep wake ticks so wake Step-A/Step-B does not swap/prune
                // before S-B R5(a) sleep adjudication runs.
                diagnostic_window: 100,
                sprout_interval: 1_000_000,
                redundancy_sprouting: false,
                ..GrowthConfig::default()
            },
            SeedBank::new(71),
        )
        .unwrap();
        let mut regs = learner.editable().state().registers().to_vec();
        regs[0] = motif[4];
        regs[1] = motif[3];
        learner.editable_mut().set_registers(&regs).unwrap();
        learner.set_prev_observed_frame(&[motif[5]]);

        learner.enable_epistemic_audit();
        let start_ticks = learner.compute_ledger().total_substrate_ticks();
        let mut budget = TickBudget::new(10_000);

        // Phase 1: Pre-sleep wake (72 ticks).
        for t in 0..72usize {
            let sym = motif[t % 6];
            let _ = learner.predict(&mut budget);
            let _ = learner.observe(&[sym], &mut budget);
        }
        assert!(learner.channel_is_grounded(0));

        // Phase 2: S-B sleep segment -> R5(a) fact adjudication excises primary and promotes alt.
        let seg = learner.run_sleep_segment(160);
        assert!(
            seg.starvation_excisions >= 1
                && learner.sleep_telemetry().sleep_starvation_excisions >= 1,
            "S-B R5(a) sleep must commit primary starvation excision: {seg:?}"
        );
        assert_eq!(learner.active_comparator_count(), 0);

        // Phase 3: Post-sleep wake (24 ticks) -> promoted alt predicts with 0 errors.
        let mut post_errs = 0usize;
        for t in 0..24usize {
            let pred = learner.predict(&mut budget);
            let expected = motif[t % 6];
            if pred[0] != expected {
                post_errs += 1;
            }
            let _ = learner.observe(&[expected], &mut budget);
        }
        assert_eq!(
            post_errs, 0,
            "promoted alt must predict period-6 motif exactly"
        );

        let elapsed_ticks = learner
            .compute_ledger()
            .total_substrate_ticks()
            .saturating_sub(start_ticks);
        let audit = learner.take_epistemic_audit().unwrap();
        let cons = verify_epistemic_mdl_conservation(&audit, &learner, elapsed_ticks);
        assert!(
            cons.flux_mdl_err_bits_per_symbol <= 1e-11,
            "R5(a) flux_mdl_err={}",
            cons.flux_mdl_err_bits_per_symbol
        );
        assert!(
            cons.energy_flux_err_bits_per_symbol <= 1e-11,
            "R5(a) energy_flux_err={}",
            cons.energy_flux_err_bits_per_symbol
        );
    }

    // 4. S-B with ungrounded LFSR driver preserves cross-channel structure without wrongful excision (R4).
    {
        let width = Width::BITS_1;
        let gen_cfg = multi_view_lfsr_generator();
        let tier_gen = gen_cfg.build(width).unwrap();
        let channels = tier_gen.channels();
        let mut env = NoisyEnvironment::new(tier_gen, NoiseModel::None, SeedBank::new(81)).unwrap();
        let mut learner = make_learner(
            width,
            channels,
            econ_uncapped_default_juv,
            GrowthConfig {
                sleep_schedule: SleepSchedule::ConsistencyReplay,
                redundancy_sprouting: true,
                ..GrowthConfig::default()
            },
        );
        let (rep, cons) = run_wake_sleep_episode_audited(&mut learner, &mut env, 1_024, 64, 80_000);
        assert_eq!(rep.sleep_telemetry.sleep_starvation_excisions, 0);
        assert!(cons.flux_mdl_err_bits_per_symbol <= 1e-11);
        assert!(cons.energy_flux_err_bits_per_symbol <= 1e-11);
    }
}
