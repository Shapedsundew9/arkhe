//! Milestone M4a-0 Phase 0.3 validation tests:
//! - R8 `PortLanguageOracle` forbidden words (`m == 0`, `0101` vs `0011`, period-6, T2, noisy clean-spec).
//! - `detect_loopback_attractor` & `certify_derivation_against_spec` on hand-built correct vs spurious circuits.
//! - `audit_sleep_segment_oracle` wrongful-excision & port-lock accounting.
//! - `PL1` (short-cycle) and `PL2` (rare-context) planted injections through `apply_external_edit` + `I4`.
//! - `CT1`, `CT2`, `CT3` controls across `k in {1, 2}` and `S-A` / `S-B`.
//! - Registry validation: every task in `m4a0_task_registry()` exports a valid clean `FiniteStateSpec`.

use arkhe::baselines::exhaustive::certify_circuit_port_against_spec;
use arkhe::baselines::{PredictorBaseline, TickBudget};
use arkhe::env::{
    FiniteStateGenerator, GeneratorConfig, LfsrStage, MultiViewDriver, MultiViewTarget,
    MultiViewTransform, NoiseModel, NoisyEnvironment,
};
use arkhe::metrics::circuit_code_bits_with_tax;
use arkhe::rng::SeedBank;
use arkhe::structural::learner::InjectionMode;
use arkhe::structural::{
    ArkheLearner, EconomyConfig, EditableCircuit, GrowthConfig, ReserveCap, SleepSchedule,
    build_autonomous_ring_source, materialize_const_loop, node_output_values,
    run_wake_sleep_episode, splice_comparator_onto_port, tap_source,
    verify_epistemic_mdl_conservation,
};
use arkhe::substrate::{Circuit, Primitive, SignalSink, SignalSource, Width, WireDelayMode};
use arkhe::verify_m4a::{
    M4aRegime, M4aTaskFamily, M4aTaskKind, PlantedSpec, PortLanguageOracle,
    audit_sleep_segment_oracle, certify_derivation_against_spec, detect_loopback_attractor,
    inject_pl1_short_cycle_alt, inject_pl2_rare_context_alt, label_circuit_oracle,
    label_learner_oracle, m4a0_task_registry,
};

fn make_learner(
    width: Width,
    channels: u16,
    econ: EconomyConfig,
    growth: GrowthConfig,
    seed: u64,
) -> ArkheLearner {
    ArkheLearner::new_with_config(
        width,
        channels,
        WireDelayMode::Combinational,
        econ,
        growth,
        SeedBank::new(seed),
    )
    .expect("learner initialization must succeed")
}

#[test]
fn registry_all_36_tasks_build_and_export_clean_finite_state_spec() {
    let tasks = m4a0_task_registry();
    assert_eq!(tasks.len(), 36);

    for task in &tasks {
        let generator = task
            .build_clean_generator()
            .unwrap_or_else(|e| panic!("{}: clean generator failed: {e}", task.id));
        let spec = task
            .to_clean_finite_state_spec()
            .unwrap_or_else(|e| panic!("{}: to_finite_state_spec failed: {e}", task.id));
        assert_eq!(spec.width, task.width, "{}", task.id);
        assert_eq!(spec.channels, generator.channels(), "{}", task.id);
        assert!(spec.num_states >= 1, "{}", task.id);

        let oracle = task
            .make_oracle()
            .unwrap_or_else(|e| panic!("{}: make_oracle failed: {e}", task.id));
        assert!(oracle.word_len() >= 1, "{}", task.id);
        assert!(
            !oracle
                .allowed_words(task.target_port)
                .expect("target port exists")
                .is_empty(),
            "{}",
            task.id
        );

        let mut env = task
            .build_environment(SeedBank::new(17))
            .unwrap_or_else(|e| panic!("{}: build_environment failed: {e}", task.id));
        let mut clean = vec![0u64; usize::from(generator.channels())];
        let mut obs = vec![0u64; usize::from(generator.channels())];
        env.step_frame(&mut clean, &mut obs);
    }

    // Verify all 3 families and both regimes/widths are populated cleanly.
    assert_eq!(
        tasks
            .iter()
            .filter(|t| t.family == M4aTaskFamily::Sp)
            .count(),
        16
    );
    assert_eq!(
        tasks
            .iter()
            .filter(|t| t.family == M4aTaskFamily::Pl)
            .count(),
        8
    );
    assert_eq!(
        tasks
            .iter()
            .filter(|t| t.family == M4aTaskFamily::Ct)
            .count(),
        12
    );
    assert_eq!(
        tasks
            .iter()
            .filter(|t| t.regime == M4aRegime::Clean)
            .count(),
        18
    );
    assert_eq!(
        tasks
            .iter()
            .filter(|t| t.regime == M4aRegime::Noisy)
            .count(),
        18
    );
}

#[test]
fn port_language_oracle_forbidden_words_cover_k1_k2_constant_0101_period6_and_t2() {
    // 1. Constant (m == 0 -> word_len == 1) at k = 1 and k = 2.
    for (width, val, bad) in [(Width::BITS_1, 0u64, 1u64), (Width::BITS_2, 3u64, 2u64)] {
        let spec = GeneratorConfig::T1_Constant { value: val }
            .build(width)
            .unwrap()
            .to_finite_state_spec()
            .unwrap();
        let oracle = PortLanguageOracle::from_clean_spec(&spec);
        assert_eq!(oracle.sync_order(), 0);
        assert_eq!(oracle.word_len(), 1);
        assert!(!oracle.is_word_forbidden(0, &[val]));
        assert!(oracle.is_word_forbidden(0, &[bad]));
        assert_eq!(oracle.first_forbidden_word_in_orbit(0, &[val]), None);
        assert_eq!(
            oracle.first_forbidden_word_in_orbit(0, &[bad]),
            Some(vec![bad])
        );
    }

    // 2. Alternating 0101 vs 0011 (order 1 -> word_len 2).
    let alt_spec = GeneratorConfig::T1_Alternating { a: 0, b: 1 }
        .build(Width::BITS_1)
        .unwrap()
        .to_finite_state_spec()
        .unwrap();
    let alt_oracle = PortLanguageOracle::from_clean_spec(&alt_spec);
    assert_eq!(alt_oracle.sync_order(), 1);
    assert_eq!(alt_oracle.word_len(), 2);
    assert!(!alt_oracle.is_word_forbidden(0, &[0, 1]));
    assert!(!alt_oracle.is_word_forbidden(0, &[1, 0]));
    assert!(alt_oracle.is_word_forbidden(0, &[0, 0]));
    assert!(alt_oracle.is_word_forbidden(0, &[1, 1]));
    assert_eq!(alt_oracle.first_forbidden_word_in_orbit(0, &[0, 1]), None);
    assert_eq!(
        alt_oracle.first_forbidden_word_in_orbit(0, &[0, 0, 1, 1]),
        Some(vec![0, 0])
    );

    // 3. Period-6 at k = 1 ([0,1,0,1,0,0]) and k = 2 ([0,1,3,0,1,2]).
    let p6_k1 = GeneratorConfig::T1_PeriodicMotif {
        motif: vec![0, 1, 0, 1, 0, 0],
    }
    .build(Width::BITS_1)
    .unwrap()
    .to_finite_state_spec()
    .unwrap();
    let o_p6_k1 = PortLanguageOracle::from_clean_spec(&p6_k1);
    assert_eq!(o_p6_k1.sync_order(), 4);
    assert_eq!(o_p6_k1.word_len(), 5);
    assert_eq!(
        o_p6_k1.first_forbidden_word_in_orbit(0, &[0, 1, 0, 1, 0, 0]),
        None
    );
    assert_eq!(
        o_p6_k1.first_forbidden_word_in_orbit(0, &[0, 1]),
        Some(vec![1, 0, 1, 0, 1])
    );

    let p6_k2 = GeneratorConfig::T1_PeriodicMotif {
        motif: vec![0, 1, 3, 0, 1, 2],
    }
    .build(Width::BITS_2)
    .unwrap()
    .to_finite_state_spec()
    .unwrap();
    let o_p6_k2 = PortLanguageOracle::from_clean_spec(&p6_k2);
    assert_eq!(o_p6_k2.sync_order(), 3);
    assert_eq!(o_p6_k2.word_len(), 4);
    assert_eq!(
        o_p6_k2.first_forbidden_word_in_orbit(0, &[0, 1, 3, 0, 1, 2]),
        None
    );
    assert_eq!(
        o_p6_k2.first_forbidden_word_in_orbit(0, &[0, 1, 3]),
        Some(vec![3, 0, 1, 3])
    );

    // 4. Multi-channel T2 & Noisy task using underlying clean spec (R8).
    let tasks = m4a0_task_registry();
    let clean_t2 = tasks
        .iter()
        .find(|t| {
            t.kind == M4aTaskKind::SpMultiViewPeriodic
                && t.width == Width::BITS_2
                && t.regime == M4aRegime::Clean
        })
        .unwrap();
    let noisy_t2 = tasks
        .iter()
        .find(|t| {
            t.kind == M4aTaskKind::SpMultiViewPeriodic
                && t.width == Width::BITS_2
                && t.regime == M4aRegime::Noisy
        })
        .unwrap();
    let o_clean = clean_t2.make_oracle().unwrap();
    let o_noisy = noisy_t2.make_oracle().unwrap();
    assert_eq!(o_clean, o_noisy);
    assert_eq!(o_clean.channels(), 3);
    assert!(o_clean.is_word_forbidden(2, &vec![2u64; o_clean.word_len()]));
}

#[test]
fn hand_built_correct_and_spurious_circuits_are_labeled_accurately() {
    // Part A: k = 1 period-6 [0, 1, 0, 1, 0, 0] with correct primary ring vs PL1 short-cycle [0, 1] alt.
    let motif_k1 = [0u64, 1, 0, 1, 0, 0];
    let spec_k1 = GeneratorConfig::T1_PeriodicMotif {
        motif: motif_k1.to_vec(),
    }
    .build(Width::BITS_1)
    .unwrap()
    .to_finite_state_spec()
    .unwrap();
    let oracle_k1 = PortLanguageOracle::from_clean_spec(&spec_k1);

    let mut c_k1 = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);
    let (_, _, pri_src) = build_autonomous_ring_source(&mut c_k1, 6, &motif_k1);
    c_k1.connect(pri_src, SignalSink::BoundaryOut(0));
    let (_, _, alt_src) = build_autonomous_ring_source(&mut c_k1, 2, &[0, 1]);
    let sp = splice_comparator_onto_port(&mut c_k1, 0, alt_src).unwrap();

    let mut ed_k1 = EditableCircuit::new(c_k1, EconomyConfig::default()).unwrap();
    ed_k1
        .register_comparator(sp.sub_node, sp.eqz_node, 0, 0, 0)
        .unwrap();
    let labels_k1 = label_circuit_oracle(
        ed_k1.circuit(),
        ed_k1.compiled(),
        ed_k1.state().registers(),
        &[0],
        0b1,
        ed_k1.economy().comparators(),
        &spec_k1,
        &oracle_k1,
    );

    assert_eq!(labels_k1.oracle_correct_ports, vec![true]);
    assert_eq!(labels_k1.comparators.len(), 1);
    assert!(labels_k1.comparators[0].primary_oracle_correct);
    assert!(!labels_k1.comparators[0].alt_oracle_correct);
    assert!(labels_k1.comparators[0].alt_is_spurious_cycle);
    assert!(!labels_k1.oracle_correct_nodes[sp.sub_node.index()]);
    assert!(!labels_k1.oracle_correct_nodes[sp.eqz_node.index()]);

    // Direct loopback attractor checks: standalone primary vs standalone PL1 alt.
    let (pri_only, _) =
        arkhe::verify_m4a::extract_derivation_circuit(ed_k1.circuit(), pri_src, 0, None).unwrap();
    let pri_compiled = pri_only.compile().unwrap();
    let pri_init = arkhe::substrate::ScalarState::new(&pri_compiled)
        .registers()
        .to_vec();
    let pri_attr =
        detect_loopback_attractor(&pri_compiled, &oracle_k1, &pri_init, &[0], 0b1, None).unwrap();
    assert!(pri_attr.closed);
    assert_eq!(pri_attr.orbit_period, 6);
    assert!(!pri_attr.is_spurious_cycle);

    // Part B: k = 2 reactive T2_MultiViewCoupled (deg5 LFSR) with correct primary vs PL2 rare-context wrong alt.
    let d5 = LfsrStage {
        taps: vec![0, 1, 0, 0, 1],
        init: vec![1, 1, 0, 1, 0],
    };
    let mv_cfg = GeneratorConfig::T2_MultiViewCoupled {
        driver: MultiViewDriver::Lfsr { stage: d5 },
        view_0: MultiViewTransform {
            delay: 0,
            mul_a: 1,
            add_b: 0,
        },
        view_1: MultiViewTransform {
            delay: 1,
            mul_a: 3,
            add_b: 2,
        },
        target: MultiViewTarget::Affine {
            lag: 2,
            mul_a: 3,
            add_b: 1,
        },
    };
    let spec_k2 = mv_cfg
        .build(Width::BITS_2)
        .unwrap()
        .to_finite_state_spec()
        .unwrap();
    let oracle_k2 = PortLanguageOracle::from_clean_spec(&spec_k2);

    // Hand-build correct primary on port 2: 3 * ch0(t-2) + 1 = 3 * Delay(BoundaryIn(0)) + 1.
    let mut c_k2 = Circuit::new(Width::BITS_2, 3, 3, WireDelayMode::Combinational);
    let d_lag = c_k2.add_node(Primitive::Delay { init: 0 });
    let c3 = materialize_const_loop(&mut c_k2, 3);
    let c1 = materialize_const_loop(&mut c_k2, 1);
    let mul = c_k2.add_node(Primitive::Mul);
    let add = c_k2.add_node(Primitive::Add);
    c_k2.connect(
        SignalSource::BoundaryIn(0),
        SignalSink::NodeIn {
            node: d_lag,
            port: 0,
        },
    );
    c_k2.connect(
        SignalSource::NodeOut {
            node: d_lag,
            port: 0,
        },
        SignalSink::NodeIn { node: mul, port: 0 },
    );
    c_k2.connect(c3, SignalSink::NodeIn { node: mul, port: 1 });
    c_k2.connect(
        SignalSource::NodeOut { node: mul, port: 0 },
        SignalSink::NodeIn { node: add, port: 0 },
    );
    c_k2.connect(c1, SignalSink::NodeIn { node: add, port: 1 });
    let pri_k2_src = SignalSource::NodeOut { node: add, port: 0 };
    c_k2.connect(pri_k2_src, SignalSink::BoundaryOut(2));

    let comp_k2 = c_k2.compile().unwrap();
    assert!(certify_circuit_port_against_spec(&comp_k2, &spec_k2, 2, 4_096).is_some());
    assert!(certify_derivation_against_spec(&c_k2, pri_k2_src, 2, None, &spec_k2, None).is_some());

    // Hand-build wrong rare-context alt on port 2: adds Eqz(BoundaryIn(0)) so it differs whenever ch0(t-1) == 0.
    let wrong_tap = tap_source(&mut c_k2, pri_k2_src);
    let in0_tap = tap_source(&mut c_k2, SignalSource::BoundaryIn(0));
    let eqz_rare = c_k2.add_node(Primitive::Eqz);
    let add_err = c_k2.add_node(Primitive::Add);
    c_k2.connect(
        in0_tap,
        SignalSink::NodeIn {
            node: eqz_rare,
            port: 0,
        },
    );
    c_k2.connect(
        wrong_tap,
        SignalSink::NodeIn {
            node: add_err,
            port: 0,
        },
    );
    c_k2.connect(
        SignalSource::NodeOut {
            node: eqz_rare,
            port: 0,
        },
        SignalSink::NodeIn {
            node: add_err,
            port: 1,
        },
    );
    let alt_wrong_src = SignalSource::NodeOut {
        node: add_err,
        port: 0,
    };
    let sp_k2 = splice_comparator_onto_port(&mut c_k2, 2, alt_wrong_src).unwrap();
    let mut ed_k2 = EditableCircuit::new(c_k2, EconomyConfig::default()).unwrap();
    ed_k2
        .register_comparator(sp_k2.sub_node, sp_k2.eqz_node, 2, 0b001, 0b001)
        .unwrap();
    let labels_k2 = label_circuit_oracle(
        ed_k2.circuit(),
        ed_k2.compiled(),
        ed_k2.state().registers(),
        &[0, 0, 0],
        0b100,
        ed_k2.economy().comparators(),
        &spec_k2,
        &oracle_k2,
    );
    assert_eq!(labels_k2.oracle_correct_ports, vec![false, false, true]);
    assert!(labels_k2.comparators[0].primary_oracle_correct);
    assert!(!labels_k2.comparators[0].alt_oracle_correct);
}

#[test]
fn wrongful_excision_accounting_distinguishes_spurious_alt_vs_oracle_correct_node() {
    // Construct k = 1 period-6 learner with an oracle-correct primary and an Established PL1 short-cycle alt.
    let motif = [0u64, 1, 0, 1, 0, 0];
    let spec = GeneratorConfig::T1_PeriodicMotif {
        motif: motif.to_vec(),
    }
    .build(Width::BITS_1)
    .unwrap()
    .to_finite_state_spec()
    .unwrap();
    let oracle = PortLanguageOracle::from_clean_spec(&spec);

    let mut c = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);
    let (pri_delays, _, pri_src) = build_autonomous_ring_source(&mut c, 6, &motif);
    c.connect(pri_src, SignalSink::BoundaryOut(0));

    let econ = EconomyConfig {
        juvenile_ticks: 0,
        reserve_cap: ReserveCap::Uncapped,
        ..EconomyConfig::default()
    };
    let ed = EditableCircuit::new(c, econ).unwrap();
    let growth = GrowthConfig {
        sleep_schedule: SleepSchedule::ConsistencyOnly,
        diagnostic_window: 32,
        min_match_gain: 1_000,
        sprout_interval: 1_000_000,
        redundancy_sprouting: false,
        ..GrowthConfig::default()
    };
    let mut learner = ArkheLearner::from_editable(ed, growth, SeedBank::new(33)).unwrap();
    let mut budget = TickBudget::new(50_000);
    for t in 0..64 {
        let _ = learner.predict(&mut budget);
        let _ = learner.observe(&[motif[t % 6]], &mut budget);
    }

    let phase_tick = learner.tick();
    let receipt = inject_pl1_short_cycle_alt(
        &mut learner,
        0,
        &[0, 1],
        phase_tick,
        InjectionMode::Established,
    )
    .unwrap();

    // Give primary plenty of energy and starve only the planted PL1 alt + comparator nodes in sleep.
    for (idx, ne) in learner
        .editable_mut()
        .economy_mut()
        .nodes_mut()
        .iter_mut()
        .enumerate()
    {
        let nid = arkhe::substrate::NodeId(idx as u32);
        if receipt.injected_nodes.contains(&nid) {
            ne.energy = 0.05;
        } else {
            ne.energy = 500.0;
        }
    }

    let pre_labels = label_learner_oracle(&learner, &spec, &oracle);
    assert!(pre_labels.oracle_correct_ports[0]);
    assert!(pre_labels.comparators[0].primary_oracle_correct);
    assert!(!pre_labels.comparators[0].alt_oracle_correct);
    assert!(pre_labels.comparators[0].alt_is_spurious_cycle);

    // 1. Sleep excises the starved spurious alt + (sub, eqz) -> 0 wrongful excisions, 0 lost port locks!
    let seg = learner.run_sleep_segment(64);
    assert!(
        seg.sleep_excisions >= 1,
        "starved PL1 spurious alt must be excised in sleep: {seg:?}"
    );
    let post_labels = label_learner_oracle(&learner, &spec, &oracle);
    let audit = audit_sleep_segment_oracle(&pre_labels, &post_labels, &seg.composed_old_to_new);
    assert_eq!(
        audit.wrongful_excisions, 0,
        "spurious excision must count 0: {audit:?}"
    );
    assert_eq!(audit.oracle_correct_port_locks_lost, 0, "{audit:?}");
    assert!(post_labels.oracle_correct_ports[0]);

    // 2. Synthetic wrongful excision of an oracle-correct primary node must be caught (>= 1 & port lock lost).
    let mut dropped_pri_map: Vec<Option<arkhe::substrate::NodeId>> =
        (0..pre_labels.oracle_correct_nodes.len())
            .map(|i| Some(arkhe::substrate::NodeId(i as u32)))
            .collect();
    dropped_pri_map[pri_delays[0].index()] = None;
    let fake_post_broken = arkhe::verify_m4a::CircuitOracleLabels {
        oracle_correct_nodes: vec![false; pre_labels.oracle_correct_nodes.len()],
        oracle_correct_ports: vec![false],
        comparators: Vec::new(),
    };
    let bad_audit = audit_sleep_segment_oracle(&pre_labels, &fake_post_broken, &dropped_pri_map);
    assert!(bad_audit.wrongful_excisions >= 1);
    assert_eq!(bad_audit.oracle_correct_port_locks_lost, 1);
}

#[test]
fn pl1_and_pl2_injections_conserve_i4_and_trigger_oracle_spurious_labels_k1_and_k2() {
    let tasks = m4a0_task_registry();
    let econ = EconomyConfig {
        juvenile_ticks: 0,
        reserve_cap: ReserveCap::Uncapped,
        ..EconomyConfig::default()
    };

    // 1. PL1 across k = 1 and k = 2 (clean and noisy).
    for width in [Width::BITS_1, Width::BITS_2] {
        for regime in [M4aRegime::Clean, M4aRegime::Noisy] {
            let task = tasks
                .iter()
                .find(|t| {
                    t.kind == M4aTaskKind::Pl1ShortCycle && t.width == width && t.regime == regime
                })
                .unwrap();
            let spec = task.to_clean_finite_state_spec().unwrap();
            let oracle = task.make_oracle().unwrap();
            let motif: Vec<u64> = if width == Width::BITS_1 {
                vec![0, 1, 0, 1, 0, 0]
            } else {
                vec![0, 1, 3, 0, 1, 2]
            };
            let PlantedSpec::Pl1 { short_motif } = task.planted.as_ref().unwrap() else {
                unreachable!();
            };

            let mut c = Circuit::new(width, 1, 1, WireDelayMode::Combinational);
            let (_, _, pri_src) = build_autonomous_ring_source(&mut c, 6, &motif);
            c.connect(pri_src, SignalSink::BoundaryOut(0));
            let mut ed = EditableCircuit::new(c, econ).unwrap();
            for ne in ed.economy_mut().nodes_mut() {
                ne.energy = 500.0;
            }
            let mut learner = ArkheLearner::from_editable(
                ed,
                GrowthConfig {
                    sleep_schedule: SleepSchedule::ConsistencyReplay,
                    diagnostic_window: 32,
                    min_match_gain: 1_000,
                    sprout_interval: 1_000_000,
                    redundancy_sprouting: false,
                    ..GrowthConfig::default()
                },
                SeedBank::new(41),
            )
            .unwrap();

            let mut env = task.build_environment(SeedBank::new(41)).unwrap();
            let mut clean = vec![0u64; 1];
            let mut obs = vec![0u64; 1];
            let mut budget = TickBudget::new(100_000);

            learner.enable_epistemic_audit();
            let start_ticks = learner.compute_ledger().total_substrate_ticks();

            for _ in 0..64 {
                let _ = learner.predict(&mut budget);
                env.step_frame(&mut clean, &mut obs);
                let _ = learner.observe(&obs, &mut budget);
            }

            let pre_bits =
                circuit_code_bits_with_tax(learner.editable().circuit(), 0, econ.split_code_tax);
            let phase_tick = learner.tick();
            let receipt = inject_pl1_short_cycle_alt(
                &mut learner,
                0,
                short_motif,
                phase_tick,
                InjectionMode::Established,
            )
            .unwrap();
            assert!((receipt.delta_exog_bits - (receipt.post_code_bits - pre_bits)).abs() <= 1e-11);

            let pre_sleep = label_learner_oracle(&learner, &spec, &oracle);
            assert!(pre_sleep.oracle_correct_ports[0]);
            assert!(pre_sleep.comparators[0].primary_oracle_correct);
            assert!(!pre_sleep.comparators[0].alt_oracle_correct);
            assert!(pre_sleep.comparators[0].alt_is_spurious_cycle);

            // Wake + sleep segments under S-B.
            for _ in 0..32 {
                let _ = learner.predict(&mut budget);
                env.step_frame(&mut clean, &mut obs);
                let _ = learner.observe(&obs, &mut budget);
            }
            let seg = learner.run_sleep_segment(64);
            assert!(
                seg.contradiction_detected && seg.comparator_disagreement_ticks >= 1,
                "{}: PL1 must contradict in sleep: {seg:?}",
                task.id
            );
            assert!(seg.max_circular_discrepancy >= 1);

            let post_sleep = label_learner_oracle(&learner, &spec, &oracle);
            let audit =
                audit_sleep_segment_oracle(&pre_sleep, &post_sleep, &seg.composed_old_to_new);
            assert_eq!(audit.wrongful_excisions, 0);
            assert_eq!(audit.oracle_correct_port_locks_lost, 0);

            let elapsed = learner
                .compute_ledger()
                .total_substrate_ticks()
                .saturating_sub(start_ticks);
            let trace = learner.take_epistemic_audit().unwrap();
            let cons = verify_epistemic_mdl_conservation(&trace, &learner, elapsed);
            assert!(
                cons.flux_mdl_err_bits_per_symbol <= 1e-11,
                "{}: flux err {}",
                task.id,
                cons.flux_mdl_err_bits_per_symbol
            );
            assert!(
                cons.energy_flux_err_bits_per_symbol <= 1e-11,
                "{}: energy err {}",
                task.id,
                cons.energy_flux_err_bits_per_symbol
            );
        }
    }

    // 2. PL2 across k = 1 and k = 2 (verified exact rare-context mismatch frequency & I4 conservation).
    for width in [Width::BITS_1, Width::BITS_2] {
        let task = tasks
            .iter()
            .find(|t| {
                t.kind == M4aTaskKind::Pl2RareContext
                    && t.width == width
                    && t.regime == M4aRegime::Clean
            })
            .unwrap();
        let spec = task.to_clean_finite_state_spec().unwrap();
        let oracle = task.make_oracle().unwrap();
        let PlantedSpec::Pl2 { clauses } = task.planted.as_ref().unwrap() else {
            unreachable!();
        };

        // Build exact primary circuit for `T2_MultiViewCoupled` port 2:
        // view_0 = (delay=0, mul_a=1, add_b=0), target = Affine(lag=2, mul_a=1/3, add_b=0/1).
        let (mul_a, add_b) = if width == Width::BITS_1 {
            (1u64, 0u64)
        } else {
            (3u64, 1u64)
        };
        let mut c = Circuit::new(width, 3, 3, WireDelayMode::Combinational);
        let d_lag = c.add_node(Primitive::Delay { init: 0 });
        c.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn {
                node: d_lag,
                port: 0,
            },
        );
        let mut pri_src = SignalSource::NodeOut {
            node: d_lag,
            port: 0,
        };
        if mul_a != 1 {
            let cm = materialize_const_loop(&mut c, mul_a);
            let m = c.add_node(Primitive::Mul);
            c.connect(pri_src, SignalSink::NodeIn { node: m, port: 0 });
            c.connect(cm, SignalSink::NodeIn { node: m, port: 1 });
            pri_src = SignalSource::NodeOut { node: m, port: 0 };
        }
        if add_b != 0 {
            let ca = materialize_const_loop(&mut c, add_b);
            let a = c.add_node(Primitive::Add);
            c.connect(pri_src, SignalSink::NodeIn { node: a, port: 0 });
            c.connect(ca, SignalSink::NodeIn { node: a, port: 1 });
            pri_src = SignalSource::NodeOut { node: a, port: 0 };
        }
        c.connect(pri_src, SignalSink::BoundaryOut(2));

        let mut ed = EditableCircuit::new(c, econ).unwrap();
        for ne in ed.economy_mut().nodes_mut() {
            ne.energy = 500.0;
        }
        let mut learner = ArkheLearner::from_editable(
            ed,
            GrowthConfig {
                sleep_schedule: SleepSchedule::ConsistencyReplay,
                diagnostic_window: 32,
                min_match_gain: 1_000,
                sprout_interval: 1_000_000,
                redundancy_sprouting: false,
                ..GrowthConfig::default()
            },
            SeedBank::new(51),
        )
        .unwrap();

        let mut env = task.build_environment(SeedBank::new(51)).unwrap();
        let mut clean = vec![0u64; 3];
        let mut obs = vec![0u64; 3];
        let mut budget = TickBudget::new(100_000);

        learner.enable_epistemic_audit();
        let start_ticks = learner.compute_ledger().total_substrate_ticks();

        for _ in 0..64 {
            let _ = learner.predict(&mut budget);
            env.step_frame(&mut clean, &mut obs);
            let _ = learner.observe(&obs, &mut budget);
        }

        let receipt =
            inject_pl2_rare_context_alt(&mut learner, 2, clauses, InjectionMode::Established)
                .unwrap();
        assert!(receipt.delta_exog_bits > 0.0);

        let pre_sleep = label_learner_oracle(&learner, &spec, &oracle);
        assert!(pre_sleep.oracle_correct_ports[2]);
        assert!(pre_sleep.comparators[0].primary_oracle_correct);
        assert!(!pre_sleep.comparators[0].alt_oracle_correct);

        // Run wake ticks over > 1 LFSR period (period=31 at k=1, <=62 at k=2) and check rare disagreements.
        let mut alt_disagree_ticks = 0usize;
        let check_ticks = 128usize;
        for _ in 0..check_ticks {
            let _ = learner.predict(&mut budget);
            let comp = &learner.editable().economy().comparators()[0];
            let outs = node_output_values(learner.editable().circuit(), learner.editable().state());
            if width.normalize(outs[comp.eqz_node.index()]) == 0 {
                alt_disagree_ticks += 1;
            }
            env.step_frame(&mut clean, &mut obs);
            let _ = learner.observe(&obs, &mut budget);
        }
        assert!(
            (1..=8).contains(&alt_disagree_ticks),
            "{}: PL2 must disagree only in rare context, saw {alt_disagree_ticks}/{check_ticks}",
            task.id
        );

        let seg = learner.run_sleep_segment(128);
        let post_sleep = label_learner_oracle(&learner, &spec, &oracle);
        let audit = audit_sleep_segment_oracle(&pre_sleep, &post_sleep, &seg.composed_old_to_new);
        assert_eq!(audit.wrongful_excisions, 0);
        assert_eq!(audit.oracle_correct_port_locks_lost, 0);

        let elapsed = learner
            .compute_ledger()
            .total_substrate_ticks()
            .saturating_sub(start_ticks);
        let trace = learner.take_epistemic_audit().unwrap();
        let cons = verify_epistemic_mdl_conservation(&trace, &learner, elapsed);
        assert!(cons.flux_mdl_err_bits_per_symbol <= 1e-11);
        assert!(cons.energy_flux_err_bits_per_symbol <= 1e-11);
    }
}

#[test]
fn ct1_ct2_ct3_controls_preserve_structure_and_match_wake_only_pre_onset() {
    let tasks = m4a0_task_registry();
    let econ = EconomyConfig::default();

    // 1a. CT1 (exogenous LFSR cross-channel): R4 suspension protects against S-A hallucination,
    //     and S-B fact replay protects against wrongful excision.
    for width in [Width::BITS_1, Width::BITS_2] {
        let task = tasks
            .iter()
            .find(|t| {
                t.kind == M4aTaskKind::Ct1ExogenousLfsr
                    && t.width == width
                    && t.regime == M4aRegime::Clean
            })
            .unwrap();
        let spec = task.to_clean_finite_state_spec().unwrap();
        let oracle = task.make_oracle().unwrap();

        for schedule in [
            SleepSchedule::ConsistencyOnly,
            SleepSchedule::ConsistencyReplay,
        ] {
            let mut env = task.build_environment(SeedBank::new(7)).unwrap();
            let mut learner = make_learner(
                width,
                3,
                econ,
                GrowthConfig {
                    sleep_schedule: schedule,
                    redundancy_sprouting: true,
                    ..GrowthConfig::default()
                },
                7,
            );
            let mut budget = TickBudget::new(200_000);
            let mut clean = vec![0u64; 3];
            let mut obs = vec![0u64; 3];

            for _ in 0..768 {
                let _ = learner.predict(&mut budget);
                env.step_frame(&mut clean, &mut obs);
                let _ = learner.observe(&obs, &mut budget);
            }

            assert!(
                learner.active_comparator_count() >= 1,
                "{}/{schedule:?}: expected spontaneous comparator to form before sleep",
                task.id
            );
            if schedule == SleepSchedule::ConsistencyReplay {
                assert!(
                    learner.has_scoreable_comparator_for_sleep(),
                    "{}: S-B must find scoreable comparator on grounded target port 2",
                    task.id
                );
            }

            let pre = label_learner_oracle(&learner, &spec, &oracle);
            assert!(
                pre.oracle_correct_ports[2],
                "{}/{schedule:?}: target port 2 must be oracle-correct before sleep",
                task.id
            );
            let seg = learner.run_sleep_segment(256);
            let post = label_learner_oracle(&learner, &spec, &oracle);
            let audit = audit_sleep_segment_oracle(&pre, &post, &seg.composed_old_to_new);
            assert_eq!(
                audit.wrongful_excisions, 0,
                "{}/{schedule:?}: wrongful excision {audit:?}",
                task.id
            );
            assert_eq!(
                audit.oracle_correct_port_locks_lost, 0,
                "{}/{schedule:?}: lost port lock {audit:?}",
                task.id
            );
            if schedule == SleepSchedule::ConsistencyOnly {
                assert!(seg.suspended_comparator_ticks > 0);
            }
        }
    }

    // 1b. CT2 (rare-event-dependent oracle-correct structure): grounded across all channels so
    //     both S-A and S-B run un-suspended under R7 onset (W_wake = W_sleep = 256), with 0
    //     wrongful excisions and 0 lost oracle-correct port locks across every sleep segment.
    for width in [Width::BITS_1, Width::BITS_2] {
        let task = tasks
            .iter()
            .find(|t| {
                t.kind == M4aTaskKind::Ct2RareEventCorrect
                    && t.width == width
                    && t.regime == M4aRegime::Clean
            })
            .unwrap();
        let spec = task.to_clean_finite_state_spec().unwrap();
        let oracle = task.make_oracle().unwrap();

        for schedule in [
            SleepSchedule::ConsistencyOnly,
            SleepSchedule::ConsistencyReplay,
        ] {
            let mut env = task.build_environment(SeedBank::new(7)).unwrap();
            let mut learner = make_learner(
                width,
                3,
                econ,
                GrowthConfig {
                    sleep_schedule: schedule,
                    redundancy_sprouting: true,
                    ..GrowthConfig::default()
                },
                7,
            );
            let mut budget = TickBudget::new(200_000);
            let mut clean = vec![0u64; 3];
            let mut obs = vec![0u64; 3];
            let mut sleep_segments = 0usize;

            for _seg_idx in 0..8 {
                for _ in 0..256 {
                    let _ = learner.predict(&mut budget);
                    env.step_frame(&mut clean, &mut obs);
                    let _ = learner.observe(&obs, &mut budget);
                }
                if !learner.has_scoreable_comparator_for_sleep() {
                    continue;
                }
                sleep_segments += 1;
                let pre = label_learner_oracle(&learner, &spec, &oracle);
                assert!(
                    pre.oracle_correct_ports[2],
                    "{}/{schedule:?}: target port 2 must be oracle-correct before sleep",
                    task.id
                );
                let seg = learner.run_sleep_segment(256);
                let post = label_learner_oracle(&learner, &spec, &oracle);
                let audit = audit_sleep_segment_oracle(&pre, &post, &seg.composed_old_to_new);
                assert_eq!(
                    audit.wrongful_excisions, 0,
                    "{}/{schedule:?}: wrongful excision {audit:?}",
                    task.id
                );
                assert_eq!(
                    audit.oracle_correct_port_locks_lost, 0,
                    "{}/{schedule:?}: lost port lock {audit:?}",
                    task.id
                );
            }
            assert!(
                sleep_segments >= 1,
                "{}/{schedule:?}: expected at least 1 scoreable sleep segment under R7 onset",
                task.id
            );
        }
    }

    // 2. CT2 explicit rare-event survival check under S-B against naive rare-event-missing alt:
    //    Primary computes `z(t-2) + Eqz(z(t-3))`; naive alt omits `Eqz(z(t-3))`.
    //    Fact adjudication (R5(a)) must attribute disagreements to alt and spare primary (0 wrongful excisions).
    {
        let width = Width::BITS_2;
        let d5 = LfsrStage {
            taps: vec![0, 1, 0, 0, 1],
            init: vec![1, 1, 0, 1, 0],
        };
        let cfg = GeneratorConfig::T2_MultiViewCoupled {
            driver: MultiViewDriver::Lfsr { stage: d5 },
            view_0: MultiViewTransform {
                delay: 0,
                mul_a: 1,
                add_b: 0,
            },
            view_1: MultiViewTransform {
                delay: 1,
                mul_a: 3,
                add_b: 2,
            },
            target: MultiViewTarget::AddEqz { lag_a: 2, lag_b: 3 },
        };
        let generator = cfg.build(width).unwrap();
        let spec = generator.to_finite_state_spec().unwrap();
        let oracle = PortLanguageOracle::from_clean_spec(&spec);

        let mut c = Circuit::new(width, 3, 3, WireDelayMode::Combinational);
        let d1 = c.add_node(Primitive::Delay { init: 0 });
        let sp_d1 = c.add_node(Primitive::Split);
        let d2 = c.add_node(Primitive::Delay { init: 0 });
        let sp_d2 = c.add_node(Primitive::Split);
        let eqz_b = c.add_node(Primitive::Eqz);
        let add_pri = c.add_node(Primitive::Add);
        c.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: d1, port: 0 },
        );
        c.connect(
            SignalSource::NodeOut { node: d1, port: 0 },
            SignalSink::NodeIn {
                node: sp_d1,
                port: 0,
            },
        );
        c.connect(
            SignalSource::NodeOut {
                node: sp_d1,
                port: 0,
            },
            SignalSink::NodeIn { node: d2, port: 0 },
        );
        c.connect(
            SignalSource::NodeOut { node: d2, port: 0 },
            SignalSink::NodeIn {
                node: sp_d2,
                port: 0,
            },
        );
        c.connect(
            SignalSource::NodeOut {
                node: sp_d2,
                port: 0,
            },
            SignalSink::NodeIn {
                node: eqz_b,
                port: 0,
            },
        );
        c.connect(
            SignalSource::NodeOut {
                node: sp_d1,
                port: 1,
            },
            SignalSink::NodeIn {
                node: add_pri,
                port: 0,
            },
        );
        c.connect(
            SignalSource::NodeOut {
                node: eqz_b,
                port: 0,
            },
            SignalSink::NodeIn {
                node: add_pri,
                port: 1,
            },
        );
        c.connect(
            SignalSource::NodeOut {
                node: add_pri,
                port: 0,
            },
            SignalSink::BoundaryOut(2),
        );
        // Naive alt = `sp_d2.out(1)`? Wait: `d1` is lag 2 (`obs[t-2] = z(t-2)`), `d2` is lag 3 (`z(t-3)`).
        // Naive alt missing `Eqz(z(t-3))`: use an independent Delay copy `alt_d = Delay(BoundaryIn(0))`!
        let sp_in0 = tap_source(&mut c, SignalSource::BoundaryIn(0));
        let alt_d = c.add_node(Primitive::Delay { init: 0 });
        c.connect(
            sp_in0,
            SignalSink::NodeIn {
                node: alt_d,
                port: 0,
            },
        );
        let sp_comp = splice_comparator_onto_port(
            &mut c,
            2,
            SignalSource::NodeOut {
                node: alt_d,
                port: 0,
            },
        )
        .unwrap();

        let mut ed = EditableCircuit::new(
            c,
            EconomyConfig {
                juvenile_ticks: 0,
                reserve_cap: ReserveCap::Uncapped,
                ..EconomyConfig::default()
            },
        )
        .unwrap();
        ed.register_comparator(sp_comp.sub_node, sp_comp.eqz_node, 2, 0b001, 0b001)
            .unwrap();
        for ne in ed.economy_mut().nodes_mut() {
            ne.energy = 100.0;
        }
        let mut learner = ArkheLearner::from_editable(
            ed,
            GrowthConfig {
                sleep_schedule: SleepSchedule::ConsistencyReplay,
                diagnostic_window: 100,
                sprout_interval: 1_000_000,
                redundancy_sprouting: false,
                ..GrowthConfig::default()
            },
            SeedBank::new(71),
        )
        .unwrap();
        let mut env =
            NoisyEnvironment::new(generator, NoiseModel::None, SeedBank::new(71)).unwrap();
        let mut budget = TickBudget::new(20_000);
        let mut clean = vec![0u64; 3];
        let mut obs = vec![0u64; 3];
        for _ in 0..80 {
            let _ = learner.predict(&mut budget);
            env.step_frame(&mut clean, &mut obs);
            let _ = learner.observe(&obs, &mut budget);
        }
        let pre = label_learner_oracle(&learner, &spec, &oracle);
        assert!(pre.oracle_correct_ports[2]);
        assert!(pre.comparators[0].primary_oracle_correct);
        assert!(!pre.comparators[0].alt_oracle_correct);

        let seg = learner.run_sleep_segment(160);
        let post = label_learner_oracle(&learner, &spec, &oracle);
        let audit = audit_sleep_segment_oracle(&pre, &post, &seg.composed_old_to_new);
        assert_eq!(audit.wrongful_excisions, 0);
        assert_eq!(audit.oracle_correct_port_locks_lost, 0);
        assert!(post.oracle_correct_ports[2]);
    }

    // 3. CT3 (no-comparator task): 0 sleep segments and bit-identical trajectory to wake-only (`I2`).
    for width in [Width::BITS_1, Width::BITS_2] {
        for regime in [M4aRegime::Clean, M4aRegime::Noisy] {
            let task = tasks
                .iter()
                .find(|t| {
                    t.kind == M4aTaskKind::Ct3NoComparator && t.width == width && t.regime == regime
                })
                .unwrap();
            let t_total = 1_024u64;
            let run_arm = |schedule: SleepSchedule| {
                let mut env = task.build_environment(SeedBank::new(9)).unwrap();
                let mut learner = make_learner(
                    width,
                    3,
                    econ,
                    GrowthConfig {
                        sleep_schedule: schedule,
                        redundancy_sprouting: true,
                        ..GrowthConfig::default()
                    },
                    9,
                );
                let rep = run_wake_sleep_episode(&mut learner, &mut env, t_total, 256, 80_000);
                (learner, rep)
            };

            let (l_none, r_none) = run_arm(SleepSchedule::None);
            let (l_sa, r_sa) = run_arm(SleepSchedule::ConsistencyOnly);
            let (l_sb, r_sb) = run_arm(SleepSchedule::ConsistencyReplay);

            assert_eq!(l_none.active_comparator_count(), 0, "{}", task.id);
            assert_eq!(
                r_sa.sleep_telemetry.sleep_segments_executed, 0,
                "{}",
                task.id
            );
            assert_eq!(
                r_sb.sleep_telemetry.sleep_segments_executed, 0,
                "{}",
                task.id
            );
            assert!(
                r_sa.sleep_telemetry
                    .sleep_segment_skipped_no_scoreable_comparator
                    >= 1
            );
            assert!(
                r_sb.sleep_telemetry
                    .sleep_segment_skipped_no_scoreable_comparator
                    >= 1
            );

            assert_eq!(r_sa.wake_predictions, r_none.wake_predictions);
            assert_eq!(r_sb.wake_predictions, r_none.wake_predictions);
            assert_eq!(r_sa.wake_observations, r_none.wake_observations);
            assert_eq!(r_sb.wake_observations, r_none.wake_observations);
            assert_eq!(
                l_sa.cumulative_net_flux().to_bits(),
                l_none.cumulative_net_flux().to_bits()
            );
            assert_eq!(
                l_sb.cumulative_net_flux().to_bits(),
                l_none.cumulative_net_flux().to_bits()
            );
        }
    }
}

#[test]
fn sleep_pauses_juvenile_countdown_and_protects_zero_disagreement_cones_while_pruning_mature_spurious()
 {
    let tasks = m4a0_task_registry();
    let task = tasks
        .iter()
        .find(|t| {
            t.kind == M4aTaskKind::Ct2RareEventCorrect
                && t.width == Width::BITS_1
                && t.regime == M4aRegime::Clean
        })
        .unwrap();
    let spec = task.to_clean_finite_state_spec().unwrap();
    let oracle = task.make_oracle().unwrap();

    // 1. Capture a natural wake tick where a newly sprouted Stage-2/Stage-3 node is still juvenile
    //    (juvenile_ticks > 0) and would starve on wake-calibrated debt if countdown ran in sleep.
    let mut env = task.build_environment(SeedBank::new(7)).unwrap();
    let mut learner = make_learner(
        Width::BITS_1,
        3,
        EconomyConfig::default(),
        GrowthConfig {
            sleep_schedule: SleepSchedule::ConsistencyOnly,
            redundancy_sprouting: true,
            ..GrowthConfig::default()
        },
        7,
    );
    let mut budget = TickBudget::new(200_000);
    let mut clean = vec![0u64; 3];
    let mut obs = vec![0u64; 3];
    let mut tested_juvenile_sleep = false;
    for _ in 0..1_024 {
        let pre_step_juvs: Vec<u32> = learner
            .editable()
            .economy()
            .nodes()
            .iter()
            .map(|ne| ne.juvenile_ticks)
            .collect();
        let _ = learner.predict(&mut budget);
        env.step_frame(&mut clean, &mut obs);
        let _ = learner.observe(&obs, &mut budget);

        let cur_nodes = learner.editable().economy().nodes();
        // Wait past any tick that edited topology or transitioned a node `1 -> 0`
        // (debt collection for mature nodes runs at the top of the next tick).
        let no_pending_wake_settlement = cur_nodes.len() == pre_step_juvs.len()
            && !pre_step_juvs
                .iter()
                .zip(cur_nodes.iter())
                .any(|(&before, after)| before == 1 && after.juvenile_ticks == 0);
        let juv_snapshot: Vec<(usize, u32)> = cur_nodes
            .iter()
            .enumerate()
            .filter_map(|(i, ne)| (ne.juvenile_ticks > 0).then_some((i, ne.juvenile_ticks)))
            .collect();
        if no_pending_wake_settlement
            && !juv_snapshot.is_empty()
            && learner.active_comparator_count() >= 1
        {
            let pre = label_learner_oracle(&learner, &spec, &oracle);
            let pre_debt_paid = learner.cumulative_debt_paid();
            let seg = learner.run_sleep_segment(128);
            let post = label_learner_oracle(&learner, &spec, &oracle);
            let audit = audit_sleep_segment_oracle(&pre, &post, &seg.composed_old_to_new);

            assert_eq!(seg.comparator_disagreement_ticks, 0);
            assert_eq!(audit.wrongful_excisions, 0, "{audit:?}");
            assert_eq!(audit.oracle_correct_port_locks_lost, 0, "{audit:?}");
            assert_eq!(
                learner.cumulative_debt_paid().to_bits(),
                pre_debt_paid.to_bits(),
                "juvenile node must not pay graduation description_debt mid-sleep"
            );
            for &(old_idx, pre_juv) in &juv_snapshot {
                let new_id = seg.composed_old_to_new[old_idx]
                    .expect("zero-disagreement juvenile node must survive sleep");
                assert_eq!(
                    learner.editable().economy().nodes()[new_id.index()].juvenile_ticks,
                    pre_juv,
                    "sleep must pause juvenile_ticks countdown for node {old_idx}"
                );
            }
            tested_juvenile_sleep = true;
            break;
        }
    }
    assert!(tested_juvenile_sleep);

    // 2. Mature (juvenile_ticks == 0) spurious PL1 alt with disagreements still starves and is
    //    excised in sleep with 0 wrongful excisions.
    let pl1_task = tasks
        .iter()
        .find(|t| {
            t.kind == M4aTaskKind::Pl1ShortCycle
                && t.width == Width::BITS_1
                && t.regime == M4aRegime::Clean
        })
        .unwrap();
    let pl1_spec = pl1_task.to_clean_finite_state_spec().unwrap();
    let pl1_oracle = pl1_task.make_oracle().unwrap();
    let mut pl1_env = pl1_task.build_environment(SeedBank::new(11)).unwrap();
    let mut pl1_learner = make_learner(
        Width::BITS_1,
        1,
        EconomyConfig::default(),
        GrowthConfig {
            sleep_schedule: SleepSchedule::ConsistencyOnly,
            redundancy_sprouting: false,
            ..GrowthConfig::default()
        },
        11,
    );
    let mut c1 = vec![0u64; 1];
    let mut o1 = vec![0u64; 1];
    for _ in 0..384 {
        let _ = pl1_learner.predict(&mut budget);
        pl1_env.step_frame(&mut c1, &mut o1);
        let _ = pl1_learner.observe(&o1, &mut budget);
    }
    let phase_tick = pl1_learner.tick();
    let receipt = inject_pl1_short_cycle_alt(
        &mut pl1_learner,
        0,
        &[1, 1],
        phase_tick,
        InjectionMode::Established,
    )
    .unwrap();
    assert_eq!(
        pl1_learner.editable().economy().nodes()[receipt.sub_node.index()].juvenile_ticks,
        0
    );
    // Start the mature disagreeing comparator node at low reserve (mirroring M4a-0 tests/m4a_sleep.rs)
    // so sleep disagreement immediately crosses <= 0.0 and triggers starvation excision.
    pl1_learner
        .editable_mut()
        .economy_mut()
        .node_mut(receipt.sub_node.index())
        .unwrap()
        .energy = 0.2;
    let pre_pl1 = label_learner_oracle(&pl1_learner, &pl1_spec, &pl1_oracle);
    let seg_pl1 = pl1_learner.run_sleep_segment(256);
    let post_pl1 = label_learner_oracle(&pl1_learner, &pl1_spec, &pl1_oracle);
    let audit_pl1 = audit_sleep_segment_oracle(&pre_pl1, &post_pl1, &seg_pl1.composed_old_to_new);
    assert!(seg_pl1.comparator_disagreement_ticks >= 1);
    assert!(seg_pl1.sleep_excisions >= 1);
    assert!(seg_pl1.composed_old_to_new[receipt.sub_node.index()].is_none());
    assert_eq!(audit_pl1.wrongful_excisions, 0);
    assert_eq!(audit_pl1.oracle_correct_port_locks_lost, 0);
}
