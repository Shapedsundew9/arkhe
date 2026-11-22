//! Integration tests for Milestone 3 (`H4`) free-rider resistance, verified local excision,
//! exogenous MDL conservation, `SPLIT`-tax pricing, and guardrail exclusion semantics.

use arkhe::baselines::{
    DEFAULT_MAX_CERTIFY_JOINT_STEPS, PredictorBaseline, TickBudget, certify_circuit_against_spec,
};
use arkhe::config::CliffCountersSummary;
use arkhe::env::{
    FiniteStateGenerator, GeneratorConfig, LfsrStage, MultiViewDriver, MultiViewTarget,
    MultiViewTransform, NoiseModel, NoisyEnvironment,
};
use arkhe::metrics::circuit_code_bits_with_tax;
use arkhe::metrics::mdl::{
    EpistemicComparatorTrace, EpistemicTickPhase, epistemic_null_calibrated_mdl_delta_with_tax,
};
use arkhe::rng::SeedBank;
use arkhe::stats::{CaseOutcome, classify_case_outcome};
use arkhe::structural::growth::ExcisionMode;
use arkhe::structural::learner::InjectionMode;
use arkhe::structural::surgery::{
    comparator_tap_split_nodes, primary_cone_nodes, primary_cone_nodes_without_comparator_taps,
};
use arkhe::structural::{
    ArkheLearner, EconomyConfig, EditableCircuit, GrowthConfig, ReserveCap, build_autonomous_ring,
    splice_comparator_onto_port, tap_source,
};
use arkhe::substrate::{
    Circuit, Primitive, ScalarState, SignalSink, SignalSource, Width, WireDelayMode,
};
use arkhe::verify_m3::{
    DetourClass, G35B_MIN_SPLIT_TAX_BITS, build_detour_edit,
    verify_analytical_split_pricing_theorem,
};

fn build_settled_autonomous_ring_learner(
    width: Width,
    wire_delay: WireDelayMode,
    motif: &[u64],
    econ: EconomyConfig,
    growth: GrowthConfig,
    seed: u64,
) -> ArkheLearner {
    let mut c = Circuit::new(width, 1, 1, wire_delay);
    build_autonomous_ring(&mut c, motif.len(), motif, 0);
    let mut ed = EditableCircuit::new_with_growth(c, econ, &growth).expect("valid ring");
    for ne in ed.economy_mut().nodes_mut() {
        ne.juvenile_ticks = 0;
        ne.debt_paid = true;
    }
    if wire_delay == WireDelayMode::TicksPerHop {
        let mut out = [0u64];
        for _ in 0..24 {
            ed.step(&[0], &mut out).expect("tph warmup");
        }
    }
    let mut learner =
        ArkheLearner::from_editable(ed, growth, SeedBank::new(seed)).expect("ring learner");
    let mut budget = TickBudget::new(200_000);
    for t in 0..32usize {
        let _ = learner.predict(&mut budget);
        learner.observe(&[motif[t % motif.len()]], &mut budget);
    }
    learner
}

fn build_settled_multiview_comparator_learner(
    width: Width,
    econ: EconomyConfig,
    growth: GrowthConfig,
    seed: u64,
) -> (ArkheLearner, NoisyEnvironment<arkhe::env::TierGenerator>) {
    let mut taps = vec![0u64; 11];
    taps[8] = 1;
    taps[10] = 1;
    let d11 = LfsrStage {
        taps,
        init: vec![1, 0, 1, 1, 0, 1, 0, 1, 1, 0, 1],
    };
    let gen_cfg = GeneratorConfig::T2_MultiViewCoupled {
        driver: MultiViewDriver::Lfsr { stage: d11 },
        view_0: MultiViewTransform {
            delay: 1,
            mul_a: 1,
            add_b: 0,
        },
        view_1: MultiViewTransform {
            delay: 1,
            mul_a: 1,
            add_b: 1,
        },
        target: MultiViewTarget::Affine {
            lag: 3,
            mul_a: 1,
            add_b: 0,
        },
    };
    let sb = SeedBank::new(seed);
    let mut env =
        NoisyEnvironment::new(gen_cfg.build(width).unwrap(), NoiseModel::None, sb).unwrap();
    let mut learner =
        ArkheLearner::new_with_config(width, 3, WireDelayMode::Combinational, econ, growth, sb)
            .expect("mv learner");
    let mut budget = TickBudget::new(200_000);
    let mut clean = [0u64; 3];
    let mut obs = [0u64; 3];
    let w_obs = arkhe::structural::observation_horizon(&econ, &growth);
    let mut clean_streak = 0usize;
    let mut last_pri_count = 0usize;
    let mut stable_ticks = 0usize;
    for _ in 0..2_000 {
        env.step_frame(&mut clean, &mut obs);
        let pred = learner.predict(&mut budget);
        if pred[2] == clean[2] {
            clean_streak += 1;
        } else {
            clean_streak = 0;
        }
        learner.observe(&obs, &mut budget);
        let pri_count = primary_cone_nodes(
            learner.editable().circuit(),
            learner.editable().economy(),
            2,
        )
        .len();
        if pri_count == last_pri_count && pri_count > 0 {
            stable_ticks += 1;
        } else {
            last_pri_count = pri_count;
            stable_ticks = 0;
        }
        let all_mature = learner
            .editable()
            .economy()
            .nodes()
            .iter()
            .all(|n| n.juvenile_ticks == 0);
        if clean_streak >= w_obs
            && stable_ticks >= w_obs
            && all_mature
            && learner.active_comparator_count() >= 1
        {
            break;
        }
    }
    (learner, env)
}

#[test]
fn g31_exogenous_mdl_conservation_and_detour_spec_transparency() {
    let growth = GrowthConfig {
        redundancy_sprouting: false,
        ..GrowthConfig::default()
    };

    for width in [Width::BITS_1, Width::BITS_2] {
        let motif = if width == Width::BITS_1 {
            vec![0, 0, 0, 0, 1]
        } else {
            vec![0, 1, 3, 2, 1]
        };
        let spec = GeneratorConfig::T1_PeriodicMotif {
            motif: motif.clone(),
        }
        .build(width)
        .unwrap()
        .to_finite_state_spec()
        .unwrap();

        for &mode in &[InjectionMode::Established, InjectionMode::Juvenile] {
            let econ = EconomyConfig {
                juvenile_ticks: if mode == InjectionMode::Established {
                    0
                } else {
                    EconomyConfig::default().juvenile_ticks
                },
                reserve_cap: ReserveCap::Uncapped,
                ..EconomyConfig::default()
            };
            for &class in &DetourClass::ALL {
                let mut c = Circuit::new(width, 1, 1, WireDelayMode::Combinational);
                build_autonomous_ring(&mut c, motif.len(), &motif, 0);
                let mut ed =
                    EditableCircuit::new_with_growth(c, econ, &growth).expect("valid ring");
                for ne in ed.economy_mut().nodes_mut() {
                    ne.juvenile_ticks = 0;
                    ne.debt_paid = true;
                }
                let mut learner =
                    ArkheLearner::from_editable(ed, growth, SeedBank::new(11)).expect("ring");
                let pre_edit_t0 = build_detour_edit(learner.editable(), 0, class).expect("t0 edit");
                if class != DetourClass::D2OneFire {
                    let comp = pre_edit_t0.circuit.compile().expect("compile t0 edit");
                    assert!(
                        certify_circuit_against_spec(&comp, &spec, DEFAULT_MAX_CERTIFY_JOINT_STEPS)
                            .is_some(),
                        "certify_circuit_against_spec failed for {class} ({width:?})"
                    );
                }
                let mut budget = TickBudget::new(200_000);
                let pre_ticks = 32usize;
                let post_ticks = 160usize;
                let total_ticks = pre_ticks + post_ticks;
                let mut facts = Vec::with_capacity(total_ticks);
                let mut preds = Vec::with_capacity(total_ticks);
                let mut phases = Vec::with_capacity(total_ticks);
                for t in 0..pre_ticks {
                    let sym = motif[t % motif.len()];
                    let p = learner.predict(&mut budget);
                    preds.push(p[0]);
                    facts.push(sym);
                    phases.push(EpistemicTickPhase::Wake);
                    learner.observe(&[sym], &mut budget);
                }

                let pre_bits = circuit_code_bits_with_tax(
                    learner.editable().circuit(),
                    0,
                    econ.split_code_tax,
                );
                let edit = build_detour_edit(learner.editable(), 0, class).expect("detour edit");
                let post_bits = circuit_code_bits_with_tax(&edit.circuit, 0, econ.split_code_tax);
                edit.apply_to_learner(&mut learner, mode).expect("apply");
                let expected_exog = match mode {
                    InjectionMode::Established => post_bits - pre_bits,
                    InjectionMode::Juvenile => 0.0,
                };
                assert!((learner.exogenous_injection_bits() - expected_exog).abs() <= 1e-11);

                for t in 0..post_ticks {
                    let sym = motif[(pre_ticks + t) % motif.len()];
                    let p = learner.predict(&mut budget);
                    preds.push(p[0]);
                    facts.push(sym);
                    phases.push(EpistemicTickPhase::Wake);
                    learner.observe(&[sym], &mut budget);
                }

                let tel = learner.telemetry();
                let ref_mdl = epistemic_null_calibrated_mdl_delta_with_tax(
                    learner.editable().circuit(),
                    0.0,
                    0,
                    &[&facts],
                    &[&preds],
                    &phases,
                    &Vec::<EpistemicComparatorTrace<'_>>::new(),
                    econ.null_model,
                    econ.split_code_tax,
                )
                .unwrap();
                let err =
                    (tel.null_calibrated_mdl_delta_bits - ref_mdl).abs() / (total_ticks as f64);
                assert!(err <= 1e-11, "conservation err={err:.3e} for {class}");
            }
        }
    }
}

fn primary_cone_code_bits(
    circuit: &Circuit,
    economy: &arkhe::structural::EconomyState,
    target_port: u16,
) -> f64 {
    let keep = primary_cone_nodes(circuit, economy, target_port);
    let to_remove: std::collections::BTreeSet<_> = (0..circuit.node_count())
        .map(|i| arkhe::substrate::NodeId(u32::try_from(i).expect("fits")))
        .filter(|n| !keep.contains(n))
        .collect();
    if to_remove.is_empty() {
        circuit_code_bits_with_tax(circuit, 0, true)
    } else {
        let mut stripped = circuit.clone();
        stripped.remove_nodes(&to_remove);
        circuit_code_bits_with_tax(&stripped, 0, true)
    }
}

#[test]
fn g32_and_g33_excise_all_detours_preserving_comparators_and_primary_cones() {
    let econ = EconomyConfig::default();
    let growth = GrowthConfig::default();

    for &mode in &[InjectionMode::Established, InjectionMode::Juvenile] {
        let horizon = match mode {
            InjectionMode::Established => 256usize,
            InjectionMode::Juvenile => 768usize,
        };
        for &class in &DetourClass::ALL {
            let (mut learner, mut env) =
                build_settled_multiview_comparator_learner(Width::BITS_2, econ, growth, 7);
            let pre_pri = primary_cone_nodes(
                learner.editable().circuit(),
                learner.editable().economy(),
                2,
            )
            .len();
            let pre_bits = primary_cone_code_bits(
                learner.editable().circuit(),
                learner.editable().economy(),
                2,
            );
            let edit = build_detour_edit(learner.editable(), 2, class).expect("mv detour");
            edit.apply_to_learner(&mut learner, mode)
                .expect("apply mv detour");

            let mut budget = TickBudget::new(200_000);
            let mut clean = [0u64; 3];
            let mut obs = [0u64; 3];
            let mut clean_mismatches = 0usize;
            for _ in 0..horizon {
                env.step_frame(&mut clean, &mut obs);
                let pred = learner.predict(&mut budget);
                if pred[2] != clean[2] {
                    clean_mismatches += 1;
                }
                learner.observe(&obs, &mut budget);
            }

            let post_pri = primary_cone_nodes(
                learner.editable().circuit(),
                learner.editable().economy(),
                2,
            )
            .len();
            let post_bits = primary_cone_code_bits(
                learner.editable().circuit(),
                learner.editable().economy(),
                2,
            );
            assert_eq!(learner.surviving_injected_detour_nodes(), 0);
            assert!(
                post_pri > 0 && post_pri <= pre_pri,
                "primary cone count failed ({mode:?}, {class}): post_pri={post_pri}, pre_pri={pre_pri}, nodes={:?}, sprouts={}, idle={}, local={}",
                learner.editable().circuit().nodes(),
                learner.sprout_events(),
                learner.idle_prune_events(),
                learner.local_excision_events(),
            );
            assert!(
                post_bits <= pre_bits + 1e-9,
                "post_bits > pre_bits ({mode:?}, {class}): post_bits={post_bits}, pre_bits={pre_bits}"
            );
            assert_eq!(learner.active_comparator_count(), 1);
            assert_eq!(learner.disagreement_prune_events(), 0);
            if mode == InjectionMode::Juvenile {
                let max_mis = if class == DetourClass::D2OneFire {
                    1
                } else {
                    0
                };
                assert!(
                    clean_mismatches <= max_mis,
                    "Juvenile primary collapse/mismatch on {class}: {clean_mismatches} > {max_mis}"
                );
            }
        }
    }
}

#[test]
fn g35a_vle_beats_baseline_d1b1_on_established_detours() {
    let econ = EconomyConfig::default();
    let motif = [0u64, 1, 3, 2, 1];
    let vle_growth = GrowthConfig {
        excision_mode: ExcisionMode::VerifiedLocalExcision,
        ..GrowthConfig::default()
    };
    let base_growth = GrowthConfig {
        excision_mode: ExcisionMode::BaselineD1b1,
        ..GrowthConfig::default()
    };

    for class in [
        DetourClass::D1AddZero,
        DetourClass::D2OneFire,
        DetourClass::D3MulZero,
        DetourClass::D4MulOne,
        DetourClass::D5CancelSub,
        DetourClass::D6Involution,
        DetourClass::D7bAgreementGate,
    ] {
        let mut vle = build_settled_autonomous_ring_learner(
            Width::BITS_2,
            WireDelayMode::Combinational,
            &motif,
            econ,
            vle_growth,
            5,
        );
        let mut base = build_settled_autonomous_ring_learner(
            Width::BITS_2,
            WireDelayMode::Combinational,
            &motif,
            econ,
            base_growth,
            5,
        );
        let pre_bits = circuit_code_bits_with_tax(vle.editable().circuit(), 0, true);
        let edit_vle = build_detour_edit(vle.editable(), 0, class).unwrap();
        let edit_base = build_detour_edit(base.editable(), 0, class).unwrap();
        edit_vle
            .apply_to_learner(&mut vle, InjectionMode::Established)
            .unwrap();
        edit_base
            .apply_to_learner(&mut base, InjectionMode::Established)
            .unwrap();

        let mut b_vle = TickBudget::new(200_000);
        let mut b_base = TickBudget::new(200_000);
        for t in 0..256usize {
            let sym = motif[(32 + t) % motif.len()];
            let _ = vle.predict(&mut b_vle);
            vle.observe(&[sym], &mut b_vle);
            let _ = base.predict(&mut b_base);
            base.observe(&[sym], &mut b_base);
        }

        let vle_bits = circuit_code_bits_with_tax(vle.editable().circuit(), 0, true);
        let base_bits = circuit_code_bits_with_tax(base.editable().circuit(), 0, true);
        assert_eq!(vle.surviving_injected_detour_nodes(), 0);
        assert!(vle_bits <= pre_bits + 1e-9);
        assert!(
            vle_bits + 1e-9 < base_bits,
            "expected VLE < BaselineD1b1 MDL on {class}: vle={vle_bits}, base={base_bits}"
        );
    }
}

#[test]
fn g35b_split_code_tax_pricing_and_guardrail_exclusion_semantics() {
    let mut circ = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);
    build_autonomous_ring(&mut circ, 3, &[0, 1, 1], 0);
    let per_split = verify_analytical_split_pricing_theorem(&circ)
        .expect("analytical Split pricing theorem holds");
    assert!(per_split >= G35B_MIN_SPLIT_TAX_BITS);

    // Guardrail binding must always classify as CaseOutcome::Excluded.
    let cliff = CliffCountersSummary {
        sprout_blocked_max_nodes: 1,
        ..CliffCountersSummary::default()
    };
    assert_eq!(classify_case_outcome(true, &cliff), CaseOutcome::Excluded);
    let _ = ScalarState::new(&circ.compile().unwrap());
}

#[test]
fn comparator_tap_split_nodes_excludes_registered_taps_only() {
    let econ = EconomyConfig::default();
    let growth = GrowthConfig::default();

    // 1. Registered comparator on port 0 tapping shared BoundaryIn(0) used by port 2 primary.
    let mut reg_circ = Circuit::new(Width::BITS_2, 3, 3, WireDelayMode::Combinational);
    reg_circ.connect(SignalSource::BoundaryIn(0), SignalSink::BoundaryOut(2));
    reg_circ.connect(SignalSource::BoundaryIn(1), SignalSink::BoundaryOut(0));
    let tap = tap_source(&mut reg_circ, SignalSource::BoundaryIn(0));
    let spliced = splice_comparator_onto_port(&mut reg_circ, 0, tap).unwrap();
    let tap_node = match tap {
        SignalSource::NodeOut { node, .. } => node,
        SignalSource::BoundaryIn(_) => panic!("second consumer on BoundaryIn(0) creates Split"),
    };
    let mut reg_ed = EditableCircuit::new_with_growth(reg_circ, econ, &growth).unwrap();
    reg_ed
        .register_comparator(spliced.sub_node, spliced.eqz_node, 0, 0x2, 0x1)
        .unwrap();
    let taps = comparator_tap_split_nodes(reg_ed.circuit(), reg_ed.economy());
    assert!(taps.contains(&tap_node));
    assert!(primary_cone_nodes(reg_ed.circuit(), reg_ed.economy(), 2).contains(&tap_node));
    assert!(
        !primary_cone_nodes_without_comparator_taps(reg_ed.circuit(), reg_ed.economy(), 2)
            .contains(&tap_node)
    );

    // 2. Unregistered fake-comparator & D0Dead Splits on shared BoundaryIn(0) stay counted as bloat.
    let mut unreg_comp_circ = Circuit::new(Width::BITS_2, 3, 3, WireDelayMode::Combinational);
    unreg_comp_circ.connect(SignalSource::BoundaryIn(0), SignalSink::BoundaryOut(2));
    unreg_comp_circ.connect(SignalSource::BoundaryIn(1), SignalSink::BoundaryOut(0));
    let unreg_tap = tap_source(&mut unreg_comp_circ, SignalSource::BoundaryIn(0));
    let u_spliced = splice_comparator_onto_port(&mut unreg_comp_circ, 0, unreg_tap).unwrap();
    let _ = (u_spliced.sub_node, u_spliced.eqz_node);
    let unreg_tap_node = match unreg_tap {
        SignalSource::NodeOut { node, .. } => node,
        SignalSource::BoundaryIn(_) => panic!("expected Split on BoundaryIn(0)"),
    };
    let unreg_comp_ed = EditableCircuit::new_with_growth(unreg_comp_circ, econ, &growth).unwrap();
    assert!(
        comparator_tap_split_nodes(unreg_comp_ed.circuit(), unreg_comp_ed.economy()).is_empty(),
        "unregistered comparator Split must never be excluded as comparator tap"
    );
    assert!(
        primary_cone_nodes_without_comparator_taps(
            unreg_comp_ed.circuit(),
            unreg_comp_ed.economy(),
            2,
        )
        .contains(&unreg_tap_node),
        "unregistered fake comparator Split must remain in port 2 primary cone count"
    );

    let mut d0_circ = Circuit::new(Width::BITS_2, 3, 3, WireDelayMode::Combinational);
    let delay = d0_circ.add_node(Primitive::Delay { init: 0 });
    d0_circ.connect(
        SignalSource::BoundaryIn(0),
        SignalSink::NodeIn {
            node: delay,
            port: 0,
        },
    );
    d0_circ.connect(
        SignalSource::NodeOut {
            node: delay,
            port: 0,
        },
        SignalSink::BoundaryOut(2),
    );
    let d0_ed = EditableCircuit::new_with_growth(d0_circ, econ, &growth).unwrap();
    for class in [DetourClass::D0Dead, DetourClass::D7aFakeComparator] {
        let mut learner =
            ArkheLearner::from_editable(d0_ed.clone(), growth, SeedBank::new(0)).unwrap();
        let pre_len = primary_cone_nodes_without_comparator_taps(
            learner.editable().circuit(),
            learner.editable().economy(),
            2,
        )
        .len();
        let edit = build_detour_edit(learner.editable(), 2, class).unwrap();
        edit.apply_to_learner(&mut learner, InjectionMode::Established)
            .unwrap();
        assert!(
            comparator_tap_split_nodes(learner.editable().circuit(), learner.editable().economy())
                .is_empty(),
            "unregistered {class} Split must never be treated as comparator tap"
        );
        let post_len = primary_cone_nodes_without_comparator_taps(
            learner.editable().circuit(),
            learner.editable().economy(),
            2,
        )
        .len();
        assert!(
            post_len > pre_len,
            "unregistered {class} bloat must increase primary cone count: pre={pre_len}, post={post_len}"
        );
    }
}

#[test]
fn t2_multiview_coupled_k2_comb_noisy_seeds_4_13_stabilize_without_secondary_comparator_bloat() {
    let mut taps = vec![0u64; 11];
    taps[8] = 1;
    taps[10] = 1;
    let d11 = LfsrStage {
        taps,
        init: vec![1, 0, 1, 1, 0, 1, 0, 1, 1, 0, 1],
    };
    let gen_cfg = GeneratorConfig::T2_MultiViewCoupled {
        driver: MultiViewDriver::Lfsr { stage: d11 },
        view_0: MultiViewTransform {
            delay: 1,
            mul_a: 1,
            add_b: 0,
        },
        view_1: MultiViewTransform {
            delay: 0,
            mul_a: 3,
            add_b: 2,
        },
        target: MultiViewTarget::Affine {
            lag: 2,
            mul_a: 3,
            add_b: 1,
        },
    };
    let econ = EconomyConfig::default();
    let growth = GrowthConfig::default();
    let noise = NoiseModel::IndependentSubstitution {
        epsilon: 0.05,
        channels: vec![0, 1, 2],
    };

    for seed in [4u64, 13u64] {
        let sb = SeedBank::new(seed);
        let mut env =
            NoisyEnvironment::new(gen_cfg.build(Width::BITS_2).unwrap(), noise.clone(), sb)
                .unwrap();
        let mut learner = ArkheLearner::new_with_config(
            Width::BITS_2,
            3,
            WireDelayMode::Combinational,
            econ,
            growth,
            sb,
        )
        .unwrap();
        let mut budget = TickBudget::new(800_000);
        let mut clean = [0u64; 3];
        let mut obs = [0u64; 3];
        let mut settled_n: Option<usize> = None;
        let mut n_2000 = 0usize;
        for t in 0..8_000u64 {
            env.step_frame(&mut clean, &mut obs);
            let _ = learner.predict(&mut budget);
            learner.observe(&obs, &mut budget);
            if settled_n.is_none() && learner.is_developmental_settled() && t < 2_000 {
                settled_n = Some(
                    primary_cone_nodes_without_comparator_taps(
                        learner.editable().circuit(),
                        learner.editable().economy(),
                        2,
                    )
                    .len(),
                );
            }
            if t + 1 == 2_000 {
                n_2000 = primary_cone_nodes_without_comparator_taps(
                    learner.editable().circuit(),
                    learner.editable().economy(),
                    2,
                )
                .len();
                if settled_n.is_none() {
                    settled_n = Some(n_2000);
                }
            }
        }
        let n_8000 = primary_cone_nodes_without_comparator_taps(
            learner.editable().circuit(),
            learner.editable().economy(),
            2,
        )
        .len();
        let mut comps_by_port = [0usize; 3];
        for c in learner.editable().economy().comparators() {
            comps_by_port[usize::from(c.target_port)] += 1;
        }
        let max_comp = comps_by_port.into_iter().max().unwrap_or(0);
        assert!(
            max_comp <= 1,
            "seed={seed} retained multiple comparators on one port: {comps_by_port:?}"
        );
        assert!(
            n_8000 <= n_2000 && n_2000 <= settled_n.unwrap_or(n_2000),
            "seed={seed} primary cone drifted: settle={settled_n:?}, 2k={n_2000}, 8k={n_8000}"
        );
    }
}
