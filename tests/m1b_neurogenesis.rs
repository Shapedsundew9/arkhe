use arkhe::baselines::{PredictorBaseline, TickBudget};
use arkhe::rng::SeedBank;
use arkhe::structural::{
    ArkheLearner, BoundaryAnchor, Coord, GridShape, GrowthConfig, LatticeCircuit, PerimeterSide,
    ResidualFeature, WiringTopology, diagnose_residual,
};
use arkhe::substrate::{SignalSink, SignalSource, Width, WireDelayMode};

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
fn spatial_relay_routing_keeps_edges_local() {
    let shape = GridShape::new(2, 2).unwrap();
    let mut lattice = LatticeCircuit::new(
        shape,
        Width::BITS_1,
        WireDelayMode::Combinational,
        WiringTopology::LocalVonNeumann,
        vec![BoundaryAnchor {
            side: PerimeterSide::West,
            offset: 0,
        }],
        vec![BoundaryAnchor {
            side: PerimeterSide::East,
            offset: 1,
        }],
    )
    .unwrap();
    let relays = lattice
        .route_connect(SignalSource::BoundaryIn(0), SignalSink::BoundaryOut(0), 4)
        .unwrap();
    assert_eq!(relays.len(), 3);
    for &(source, sink) in lattice.edges() {
        let (source_coord, source_boundary) = match source {
            SignalSource::BoundaryIn(0) => (Coord { row: 0, col: 0 }, true),
            SignalSource::NodeOut { node, .. } => (lattice.node_coord(node).unwrap(), false),
            SignalSource::BoundaryIn(_) => unreachable!(),
        };
        let (sink_coord, sink_boundary) = match sink {
            SignalSink::NodeIn { node, .. } => (lattice.node_coord(node).unwrap(), false),
            SignalSink::BoundaryOut(0) => (Coord { row: 1, col: 1 }, true),
            SignalSink::BoundaryOut(_) => unreachable!(),
        };
        assert_eq!(
            source_coord.manhattan_distance(sink_coord)
                + u16::from(source_boundary)
                + u16::from(sink_boundary),
            1
        );
    }
    assert!(lattice.lower().is_ok());
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
fn well_mixed_direct_graph_wiring_connects_without_planar_relays() {
    let shape = GridShape::new(8, 8).unwrap();
    let mut lattice = LatticeCircuit::new(
        shape,
        Width::BITS_2,
        WireDelayMode::Combinational,
        WiringTopology::WellMixed,
        vec![BoundaryAnchor {
            side: PerimeterSide::North,
            offset: 0,
        }],
        vec![BoundaryAnchor {
            side: PerimeterSide::South,
            offset: 7,
        }],
    )
    .unwrap();

    // Place a primitive at row 0, col 0 and another at row 7, col 7
    let n0 = lattice
        .place_node(Coord { row: 0, col: 0 }, arkhe::substrate::Primitive::Eqz)
        .unwrap();
    let n1 = lattice
        .place_node(Coord { row: 7, col: 7 }, arkhe::substrate::Primitive::Add)
        .unwrap();

    // In WellMixed, route_connect places zero relay cells regardless of distance
    let relays_in = lattice
        .route_connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: n0, port: 0 },
            4,
        )
        .unwrap();
    assert_eq!(relays_in.len(), 0);

    let relays_mid = lattice
        .route_connect(
            SignalSource::NodeOut { node: n0, port: 0 },
            SignalSink::NodeIn { node: n1, port: 0 },
            4,
        )
        .unwrap();
    assert_eq!(relays_mid.len(), 0);

    // Tap BoundaryIn(0) to also feed n1 port 1
    let tapped_in = lattice.tap_source(SignalSource::BoundaryIn(0), 4).unwrap();
    lattice
        .connect(tapped_in, SignalSink::NodeIn { node: n1, port: 1 })
        .unwrap();

    let relays_out = lattice
        .route_connect(
            SignalSource::NodeOut { node: n1, port: 0 },
            SignalSink::BoundaryOut(0),
            4,
        )
        .unwrap();
    assert_eq!(relays_out.len(), 0);

    let circuit = lattice.lower().unwrap();
    let compiled = circuit.compile().unwrap();
    assert_eq!(compiled.total_registers(), 0);
}
