//! Milestone M4a-0 Phase 0.1 wake-sleep invariants (`I1`, `I2`, `I3`, `I5`, `R2`, `R3`, `R7`).

use arkhe::baselines::{PredictorBaseline, TickBudget};
use arkhe::env::{
    FiniteStateGenerator, GeneratorConfig, LfsrStage, MultiViewDriver, MultiViewTarget,
    MultiViewTransform, NoiseModel, NoisyEnvironment,
};
use arkhe::rng::SeedBank;
use arkhe::structural::sleep::SleepSchedule;
use arkhe::structural::{
    ArkheLearner, EconomyConfig, EditableCircuit, ExecutionPhase, GrowthConfig,
    derived_wake_sleep_windows, observation_horizon, run_wake_sleep_episode,
    splice_comparator_onto_port, tap_source,
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
    assert_eq!(report_a.sleep_telemetry, Default::default());
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
            assert_eq!(seg.replay_ticks, 32);
            assert_eq!(seg.loopback_ticks, 32);
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
