//! Pre-growth Milestone 1a metabolic-economy gates.

pub mod panel;

use serde::{Deserialize, Serialize};

use crate::config::BuildProvenance;
use crate::env::{GeneratorConfig, NoiseModel, NoisyEnvironment};
use crate::rng::SeedBank;
use crate::structural::economy::EconomyConfig;
use crate::structural::lattice::{
    BoundaryAnchor, Coord, GridShape, LatticeCircuit, PerimeterSide, WiringTopology,
};
use crate::structural::live::LiveCircuit;
use crate::structural::sensitivity::counterfactual_sensitivity;
use crate::substrate::{
    Circuit, Primitive, ScalarState, SignalSink, SignalSource, Width, WireDelayMode,
};

pub use panel::{CircuitClass, PanelRun};

/// One pre-registered M1a gate outcome.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M1aGateReport {
    /// Gate identifier.
    pub gate: String,
    /// Whether all checks in this gate passed.
    pub passed: bool,
    /// Number of individual cases evaluated.
    pub cases: u64,
    /// Failure details; empty when passed.
    pub failures: Vec<String>,
}

/// Aggregated panel metrics across all seeds for one (task, k, circuit, ε) tuple.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M1aPanelAggregate {
    /// Benchmark task identifier.
    pub task: String,
    /// Bit width.
    pub k: u8,
    /// Circuit identifier.
    pub circuit: String,
    /// Diagnostic classification.
    pub class: CircuitClass,
    /// Substitution noise rate.
    pub epsilon: f64,
    /// Total nodes in the circuit.
    pub node_count: usize,
    /// Number of evaluated runs.
    pub runs: u64,
    /// Minimum ΔE across evaluated runs.
    pub min_delta_e: f64,
    /// Mean ΔE across evaluated runs.
    pub mean_delta_e: f64,
    /// Maximum ΔE across evaluated runs.
    pub max_delta_e: f64,
    /// Minimum of min_node_energy across evaluated runs.
    pub min_min_node_energy: f64,
    /// Mean null-calibrated ΔL_null across evaluated runs.
    pub mean_delta_l_null: f64,
    /// Minimum post-transient accuracy across evaluated runs.
    pub min_accuracy: f64,
    /// Maximum credit paid to detour nodes after tick 0 across runs.
    pub max_detour_credit_after_tick0: f64,
}

/// Structured report emitted by `verify-m1a`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M1aVerificationSummary {
    /// Build and source-tree provenance.
    pub provenance: BuildProvenance,
    /// Initial seed for stochastic channel-noise cases.
    pub seed_offset: u32,
    /// Number of seeds used for stochastic conditions.
    pub seeds_tested: u32,
    /// Results for G1A.1 through G1A.4.
    pub gates: Vec<M1aGateReport>,
    /// Pre-registered circuit panel aggregates.
    pub panel: Vec<M1aPanelAggregate>,
    /// Whether every M1a gate passed.
    pub all_passed: bool,
}

/// Runs the fixed-structure M1a gates against the live circuit panel.
#[must_use]
pub fn run_m1a_verification_suite(seed_offset: u32, seeds: u32) -> M1aVerificationSummary {
    let seeds_tested = seeds.max(1);
    let runs = panel::run_panel_matrix(seed_offset, seeds_tested);

    let mut gates = vec![
        verify_g1a1(&runs),
        verify_g1a2(&runs),
        verify_g1a3(&runs),
        verify_g1a4(&runs, seed_offset),
    ];
    for gate in &mut gates {
        gate.failures.sort();
    }
    let all_passed = gates.iter().all(|gate| gate.passed);

    let mut panel_aggregates = Vec::new();
    let epsilons = [0.0, 0.05];
    for task in panel::panel_tasks() {
        for pc in panel::panel_circuits(&task) {
            for &epsilon in &epsilons {
                let matching: Vec<&PanelRun> = runs
                    .iter()
                    .filter(|r| {
                        r.k == task.width.bits()
                            && r.task == task.id
                            && r.circuit == pc.id
                            && (r.epsilon - epsilon).abs() < 1e-9
                    })
                    .collect();
                if matching.is_empty() {
                    continue;
                }
                let count = matching.len() as f64;
                let min_delta_e = matching
                    .iter()
                    .map(|r| r.delta_e)
                    .fold(f64::INFINITY, f64::min);
                let mean_delta_e = matching.iter().map(|r| r.delta_e).sum::<f64>() / count;
                let max_delta_e = matching
                    .iter()
                    .map(|r| r.delta_e)
                    .fold(f64::NEG_INFINITY, f64::max);
                let min_min_node_energy = matching
                    .iter()
                    .map(|r| r.min_node_energy)
                    .fold(f64::INFINITY, f64::min);
                let mean_delta_l_null =
                    matching.iter().map(|r| r.delta_l_null).sum::<f64>() / count;
                let min_accuracy = matching
                    .iter()
                    .map(|r| r.accuracy)
                    .fold(f64::INFINITY, f64::min);
                let max_detour_credit_after_tick0 = matching
                    .iter()
                    .map(|r| r.detour_credit_after_tick0)
                    .fold(0.0, f64::max);

                panel_aggregates.push(M1aPanelAggregate {
                    task: task.id.to_owned(),
                    k: task.width.bits(),
                    circuit: pc.id.to_owned(),
                    class: pc.class,
                    epsilon,
                    node_count: pc.circuit.node_count(),
                    runs: matching.len() as u64,
                    min_delta_e,
                    mean_delta_e,
                    max_delta_e,
                    min_min_node_energy,
                    mean_delta_l_null,
                    min_accuracy,
                    max_detour_credit_after_tick0,
                });
            }
        }
    }

    M1aVerificationSummary {
        provenance: BuildProvenance::current(),
        seed_offset,
        seeds_tested,
        gates,
        panel: panel_aggregates,
        all_passed,
    }
}

fn gate_report(gate: &str, cases: u64, failures: Vec<String>) -> M1aGateReport {
    M1aGateReport {
        gate: gate.to_owned(),
        passed: failures.is_empty(),
        cases,
        failures,
    }
}

fn verify_g1a1(runs: &[PanelRun]) -> M1aGateReport {
    let mut failures = Vec::new();
    let mut cases = 0u64;

    // 1. Float scan check (verified at compile time via build.rs)
    cases += 1;
    if env!("ARKHE_SUBSTRATE_FLOAT_FREE") != "1" {
        failures.push("G1A.1: floating-point token found in src/substrate".to_owned());
    }

    // 2. Exact macro-micro conservation on every panel run
    for run in runs {
        cases += 1;
        if run.max_conservation_error >= 1e-12 {
            failures.push(format!(
                "G1A.1 / {} / {} / {} / {} / {}: max_conservation_error {} >= 1e-12",
                run.task, run.k, run.circuit, run.epsilon, run.seed, run.max_conservation_error
            ));
        }
    }

    // 3. k = 8 sampling verification
    let width8 = Width::new(8).expect("k=8 is valid");

    // Circuit 1: Split -> Eqz
    {
        let mut circuit = Circuit::new(width8, 1, 1, WireDelayMode::Combinational);
        let split = circuit.add_node(Primitive::Split);
        let eqz = circuit.add_node(Primitive::Eqz);
        circuit.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn {
                node: split,
                port: 0,
            },
        );
        circuit.connect(
            SignalSource::NodeOut {
                node: split,
                port: 0,
            },
            SignalSink::NodeIn { node: eqz, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: eqz, port: 0 },
            SignalSink::BoundaryOut(0),
        );
        let compiled = circuit.compile().unwrap();
        let mut state = ScalarState::new(&compiled);
        for t in 0..64u64 {
            let input = t % 256;
            state.step(&compiled, &[input], &mut [0]).unwrap();
            let exact =
                counterfactual_sensitivity(&circuit, &compiled, &state, SeedBank::new(5), t, 255)
                    .unwrap();
            let sampled =
                counterfactual_sensitivity(&circuit, &compiled, &state, SeedBank::new(5), t, 32)
                    .unwrap();
            for &node in &[split, eqz] {
                cases += 1;
                let diff = (exact.output_by_node[node.index()][0]
                    - sampled.output_by_node[node.index()][0])
                    .abs();
                if diff > 0.05 {
                    failures.push(format!(
                        "G1A.1 / k=8 Split->Eqz / node {} / tick {t}: |sampled - exact| = {diff} > 0.05",
                        node.index()
                    ));
                }
            }
        }
    }

    // Circuit 2: Add(x, ConstLoop(1))
    {
        let mut circuit = Circuit::new(width8, 1, 1, WireDelayMode::Combinational);
        let d = circuit.add_node(Primitive::Delay { init: 1 });
        let s = circuit.add_node(Primitive::Split);
        let add = circuit.add_node(Primitive::Add);
        circuit.connect(
            SignalSource::NodeOut { node: d, port: 0 },
            SignalSink::NodeIn { node: s, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: s, port: 0 },
            SignalSink::NodeIn { node: d, port: 0 },
        );
        circuit.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: add, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: s, port: 1 },
            SignalSink::NodeIn { node: add, port: 1 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: add, port: 0 },
            SignalSink::BoundaryOut(0),
        );
        let compiled = circuit.compile().unwrap();
        let mut state = ScalarState::new(&compiled);
        for t in 0..64u64 {
            let input = t % 256;
            state.step(&compiled, &[input], &mut [0]).unwrap();
            let exact =
                counterfactual_sensitivity(&circuit, &compiled, &state, SeedBank::new(5), t, 255)
                    .unwrap();
            let sampled =
                counterfactual_sensitivity(&circuit, &compiled, &state, SeedBank::new(5), t, 32)
                    .unwrap();
            for &node in &[d, s, add] {
                cases += 1;
                let diff = (exact.output_by_node[node.index()][0]
                    - sampled.output_by_node[node.index()][0])
                    .abs();
                if diff > 0.05 {
                    failures.push(format!(
                        "G1A.1 / k=8 AddConst1 / node {} / tick {t}: |sampled - exact| = {diff} > 0.05",
                        node.index()
                    ));
                }
            }
        }
    }

    gate_report("G1A.1", cases, failures)
}

fn verify_g1a2(runs: &[PanelRun]) -> M1aGateReport {
    let mut failures = Vec::new();
    let mut cases = 0u64;
    let initial_energy = EconomyConfig::default().initial_energy;

    let mut expected_map = std::collections::BTreeMap::new();
    for task in panel::panel_tasks() {
        for pc in panel::panel_circuits(&task) {
            expected_map.insert((task.id, task.width.bits(), pc.id), pc.expected_accuracy);
        }
    }

    for run in runs {
        // Accuracy self-check on ε = 0.0 runs
        if run.epsilon == 0.0
            && let Some(&expected) = expected_map.get(&(run.task, run.k, run.circuit))
        {
            cases += 1;
            match expected {
                panel::AccuracyCheck::Exact(v) => {
                    if (run.accuracy - v).abs() >= 1e-12 {
                        failures.push(format!(
                            "panel-construction: G1A.2 / {} / {} / {} / {} / {}: accuracy {} != expected {}",
                            run.task, run.k, run.circuit, run.epsilon, run.seed, run.accuracy, v
                        ));
                    }
                }
                panel::AccuracyCheck::Range(min, max) => {
                    if run.accuracy < min || run.accuracy > max {
                        failures.push(format!(
                            "panel-construction: G1A.2 / {} / {} / {} / {} / {}: accuracy {} not in [{}, {}]",
                            run.task, run.k, run.circuit, run.epsilon, run.seed, run.accuracy, min, max
                        ));
                    }
                }
                panel::AccuracyCheck::None => {}
            }
        }

        // C_opt runs: delta_e > 0.0 and min_node_energy > initial_energy
        if run.class == panel::CircuitClass::Opt {
            cases += 1;
            if run.delta_e <= 0.0 {
                failures.push(format!(
                    "G1A.2 / {} / {} / {} / {} / {}: C_opt delta_e {} <= 0.0",
                    run.task, run.k, run.circuit, run.epsilon, run.seed, run.delta_e
                ));
            }
            cases += 1;
            if run.min_node_energy <= initial_energy {
                failures.push(format!(
                    "G1A.2 / {} / {} / {} / {} / {}: C_opt min_node_energy {} <= initial_energy {}",
                    run.task,
                    run.k,
                    run.circuit,
                    run.epsilon,
                    run.seed,
                    run.min_node_energy,
                    initial_energy
                ));
            }
        }

        // C_wrong, C_chance, C_silence: delta_e < 0.0
        if matches!(
            run.class,
            panel::CircuitClass::Wrong | panel::CircuitClass::Chance | panel::CircuitClass::Silence
        ) {
            cases += 1;
            if run.delta_e >= 0.0 {
                failures.push(format!(
                    "G1A.2 / {} / {} / {} / {} / {}: {:?} delta_e {} >= 0.0",
                    run.task, run.k, run.circuit, run.epsilon, run.seed, run.class, run.delta_e
                ));
            }
        }
    }

    // Spatial case: build Osc2(0, hi) for alt at k ∈ {1, 2}
    for width in [Width::BITS_1, Width::BITS_2] {
        cases += 2;
        match run_spatial_osc2(width) {
            Ok((delta_e, min_node_energy)) => {
                if delta_e <= 0.0 {
                    failures.push(format!(
                        "G1A.2 / spatial / alt / {} / osc2: delta_e {} <= 0.0",
                        width.bits(),
                        delta_e
                    ));
                }
                if min_node_energy <= initial_energy {
                    failures.push(format!(
                        "G1A.2 / spatial / alt / {} / osc2: min_node_energy {} <= initial_energy {}",
                        width.bits(),
                        min_node_energy,
                        initial_energy
                    ));
                }
            }
            Err(e) => failures.push(format!("G1A.2 / spatial / alt / {}: {e}", width.bits())),
        }
    }

    gate_report("G1A.2", cases, failures)
}

fn verify_g1a3(runs: &[PanelRun]) -> M1aGateReport {
    let mut failures = Vec::new();
    let mut cases = 0u64;

    let mut run_by_case = std::collections::BTreeMap::new();
    for run in runs {
        let eps_key = (run.epsilon * 1000.0).round() as u64;
        run_by_case.insert((run.task, run.k, run.circuit, eps_key, run.seed), run);
    }

    let mut ref_by_detour = std::collections::BTreeMap::new();
    let mut detour_nodes_by_circuit = std::collections::BTreeMap::new();
    for task in panel::panel_tasks() {
        for pc in panel::panel_circuits(&task) {
            if pc.class == panel::CircuitClass::Detour {
                if let Some(reference) = pc.reference {
                    ref_by_detour.insert((task.id, task.width.bits(), pc.id), reference);
                }
                detour_nodes_by_circuit.insert(
                    (task.id, task.width.bits(), pc.id),
                    !pc.detour_nodes.is_empty(),
                );
            }
        }
    }

    for run in runs {
        if run.class != panel::CircuitClass::Detour {
            continue;
        }

        let has_detour_nodes = detour_nodes_by_circuit
            .get(&(run.task, run.k, run.circuit))
            .copied()
            .unwrap_or(false);

        if has_detour_nodes {
            cases += 1;
            if run.detour_credit_after_tick0 != 0.0 {
                failures.push(format!(
                    "G1A.3 / {} / {} / {} / {} / {}: detour_credit_after_tick0 {} != 0.0",
                    run.task,
                    run.k,
                    run.circuit,
                    run.epsilon,
                    run.seed,
                    run.detour_credit_after_tick0
                ));
            }
        }

        if let Some(&reference_id) = ref_by_detour.get(&(run.task, run.k, run.circuit)) {
            let eps_key = (run.epsilon * 1000.0).round() as u64;
            cases += 1;
            if let Some(&ref_run) =
                run_by_case.get(&(run.task, run.k, reference_id, eps_key, run.seed))
            {
                if run.delta_e >= ref_run.delta_e {
                    failures.push(format!(
                        "G1A.3 / {} / {} / {} / {} / {}: detour delta_e {} >= reference ({}) delta_e {}",
                        run.task, run.k, run.circuit, run.epsilon, run.seed, run.delta_e, reference_id, ref_run.delta_e
                    ));
                }
            } else {
                failures.push(format!(
                    "G1A.3 / {} / {} / {} / {} / {}: missing reference run for {}",
                    run.task, run.k, run.circuit, run.epsilon, run.seed, reference_id
                ));
            }
        }
    }

    // Spatial case: Add(Osc2, ConstLoop(0)) as a LocalVonNeumann lattice
    for width in [Width::BITS_1, Width::BITS_2] {
        cases += 2;
        match run_spatial_detour(width) {
            Ok((detour_credit, detour_delta_e, ref_delta_e)) => {
                if detour_credit != 0.0 {
                    failures.push(format!(
                        "G1A.3 / spatial / alt / {} / detour: detour_credit_after_tick0 {} != 0.0",
                        width.bits(),
                        detour_credit
                    ));
                }
                if detour_delta_e >= ref_delta_e {
                    failures.push(format!(
                        "G1A.3 / spatial / alt / {} / detour: delta_e {} >= reference delta_e {}",
                        width.bits(),
                        detour_delta_e,
                        ref_delta_e
                    ));
                }
            }
            Err(e) => failures.push(format!("G1A.3 / spatial / alt / {}: {e}", width.bits())),
        }
    }

    gate_report("G1A.3", cases, failures)
}

fn verify_g1a4(runs: &[PanelRun], seed_offset: u32) -> M1aGateReport {
    let mut failures = Vec::new();
    let mut cases = 0u64;

    // 1. Sign check (all runs): zero runs with delta_e > 0.0 && delta_l_null > 0.0
    for run in runs {
        cases += 1;
        if run.delta_e > 0.0 && run.delta_l_null > 0.0 {
            failures.push(format!(
                "G1A.4 / {} / {} / {} / {} / {}: sign inversion: delta_e {} > 0 but delta_l_null {} > 0",
                run.task, run.k, run.circuit, run.epsilon, run.seed, run.delta_e, run.delta_l_null
            ));
        }
    }

    // 2. Rank check: for each (k, non-constant task, ε) group, take runs with seed == seed_offset
    let epsilons = [0.0, 0.05];
    let non_constant_tasks = [
        ("alt", Width::BITS_1),
        ("alt", Width::BITS_2),
        ("motif3", Width::BITS_1),
        ("motif3", Width::BITS_2),
        ("sparse6", Width::BITS_1),
        ("sparse6", Width::BITS_2),
        ("affine", Width::BITS_1),
        ("affine", Width::BITS_2),
        ("counter3", Width::BITS_2),
    ];

    for (task_id, width) in non_constant_tasks {
        let k = width.bits();
        for &eps in &epsilons {
            cases += 1;
            let group_runs: Vec<&PanelRun> = runs
                .iter()
                .filter(|r| {
                    r.k == k
                        && r.task == task_id
                        && (r.epsilon - eps).abs() < 1e-9
                        && r.seed == u64::from(seed_offset)
                })
                .collect();

            if group_runs.len() < 6 {
                failures.push(format!(
                    "G1A.4 / {} / k={} / eps={}: group has only {} members, expected >= 6",
                    task_id,
                    k,
                    eps,
                    group_runs.len()
                ));
                continue;
            }

            let delta_es: Vec<f64> = group_runs.iter().map(|r| r.delta_e).collect();
            let neg_delta_ls: Vec<f64> = group_runs.iter().map(|r| -r.delta_l_null).collect();
            let rho = spearman(&delta_es, &neg_delta_ls);

            if rho < 0.90 {
                failures.push(format!(
                    "G1A.4 / {} / k={} / eps={}: Spearman rho {:.4} < 0.90 (delta_e={:?}, -delta_l={:?})",
                    task_id, k, eps, rho, delta_es, neg_delta_ls
                ));
            }
        }
    }

    gate_report("G1A.4", cases, failures)
}

fn run_spatial_osc2(width: Width) -> Result<(f64, f64), String> {
    let k = width.bits();
    let hi = (1u64 << k) - 1;
    let shape = GridShape::new(2, 2).map_err(|e| e.to_string())?;
    let mut lattice = LatticeCircuit::new(
        shape,
        width,
        WireDelayMode::Combinational,
        WiringTopology::LocalVonNeumann,
        vec![BoundaryAnchor {
            side: PerimeterSide::North,
            offset: 0,
        }],
        vec![BoundaryAnchor {
            side: PerimeterSide::South,
            offset: 1,
        }],
    )
    .map_err(|e| e.to_string())?;

    let d1 = lattice
        .place_node(Coord { row: 0, col: 0 }, Primitive::Delay { init: hi })
        .map_err(|e| e.to_string())?;
    let d2 = lattice
        .place_node(Coord { row: 0, col: 1 }, Primitive::Delay { init: 0 })
        .map_err(|e| e.to_string())?;
    let s = lattice
        .place_node(Coord { row: 1, col: 1 }, Primitive::Split)
        .map_err(|e| e.to_string())?;
    let relay = lattice
        .place_node(Coord { row: 1, col: 0 }, Primitive::Split)
        .map_err(|e| e.to_string())?;

    lattice
        .connect(
            SignalSource::NodeOut { node: d1, port: 0 },
            SignalSink::NodeIn { node: d2, port: 0 },
        )
        .map_err(|e| e.to_string())?;
    lattice
        .connect(
            SignalSource::NodeOut { node: d2, port: 0 },
            SignalSink::NodeIn { node: s, port: 0 },
        )
        .map_err(|e| e.to_string())?;
    lattice
        .connect(
            SignalSource::NodeOut { node: s, port: 0 },
            SignalSink::NodeIn {
                node: relay,
                port: 0,
            },
        )
        .map_err(|e| e.to_string())?;
    lattice
        .connect(
            SignalSource::NodeOut {
                node: relay,
                port: 0,
            },
            SignalSink::NodeIn { node: d1, port: 0 },
        )
        .map_err(|e| e.to_string())?;
    lattice
        .connect(
            SignalSource::NodeOut { node: s, port: 1 },
            SignalSink::BoundaryOut(0),
        )
        .map_err(|e| e.to_string())?;

    let wire_steps = lattice.wire_steps_per_node().map_err(|e| e.to_string())?;
    let circuit = lattice.lower().map_err(|e| e.to_string())?;

    let generator = GeneratorConfig::T1_Alternating { a: 0, b: hi }
        .build(width)
        .map_err(|e| e.to_string())?;
    let mut env = NoisyEnvironment::new(generator, NoiseModel::None, SeedBank::new(0))
        .map_err(|e| e.to_string())?;
    let config = EconomyConfig {
        juvenile_ticks: 0,
        max_energy_reserve: None,
        ..EconomyConfig::default()
    };
    let mut live = LiveCircuit::new(circuit, &wire_steps, config, SeedBank::new(0), 8)
        .map_err(|e| e.to_string())?;

    let mut clean = vec![0u64; 1];
    let mut observed = vec![0u64; 1];
    let mut prev = vec![0u64; 1];

    for _ in 0..1024 {
        env.step_frame(&mut clean, &mut observed);
        let _ = live.predict(&prev).map_err(|e| e.to_string())?;
        live.settle(&observed).map_err(|e| e.to_string())?;
        prev[0] = observed[0];
    }

    let delta_e = live
        .economy()
        .nodes()
        .iter()
        .map(|n| n.energy - config.initial_energy)
        .sum();
    let min_node_energy = live
        .economy()
        .nodes()
        .iter()
        .map(|n| n.energy)
        .fold(f64::INFINITY, f64::min);
    Ok((delta_e, min_node_energy))
}

fn run_spatial_detour(width: Width) -> Result<(f64, f64, f64), String> {
    let k = width.bits();
    let hi = (1u64 << k) - 1;
    let shape = GridShape::new(3, 3).map_err(|e| e.to_string())?;
    let mut lattice = LatticeCircuit::new(
        shape,
        width,
        WireDelayMode::Combinational,
        WiringTopology::LocalVonNeumann,
        vec![BoundaryAnchor {
            side: PerimeterSide::North,
            offset: 0,
        }],
        vec![BoundaryAnchor {
            side: PerimeterSide::East,
            offset: 1,
        }],
    )
    .map_err(|e| e.to_string())?;

    // Osc2 ring
    let d1 = lattice
        .place_node(Coord { row: 0, col: 0 }, Primitive::Delay { init: hi })
        .map_err(|e| e.to_string())?;
    let d2 = lattice
        .place_node(Coord { row: 0, col: 1 }, Primitive::Delay { init: 0 })
        .map_err(|e| e.to_string())?;
    let s = lattice
        .place_node(Coord { row: 1, col: 1 }, Primitive::Split)
        .map_err(|e| e.to_string())?;
    let relay = lattice
        .place_node(Coord { row: 1, col: 0 }, Primitive::Split)
        .map_err(|e| e.to_string())?;

    lattice
        .connect(
            SignalSource::NodeOut { node: d1, port: 0 },
            SignalSink::NodeIn { node: d2, port: 0 },
        )
        .map_err(|e| e.to_string())?;
    lattice
        .connect(
            SignalSource::NodeOut { node: d2, port: 0 },
            SignalSink::NodeIn { node: s, port: 0 },
        )
        .map_err(|e| e.to_string())?;
    lattice
        .connect(
            SignalSource::NodeOut { node: s, port: 0 },
            SignalSink::NodeIn {
                node: relay,
                port: 0,
            },
        )
        .map_err(|e| e.to_string())?;
    lattice
        .connect(
            SignalSource::NodeOut {
                node: relay,
                port: 0,
            },
            SignalSink::NodeIn { node: d1, port: 0 },
        )
        .map_err(|e| e.to_string())?;

    // Add node at (1, 2)
    let add = lattice
        .place_node(Coord { row: 1, col: 2 }, Primitive::Add)
        .map_err(|e| e.to_string())?;
    lattice
        .connect(
            SignalSource::NodeOut { node: s, port: 1 },
            SignalSink::NodeIn { node: add, port: 0 },
        )
        .map_err(|e| e.to_string())?;

    // ConstLoop(0): dc at (2, 1), sc at (2, 2)
    let dc = lattice
        .place_node(Coord { row: 2, col: 1 }, Primitive::Delay { init: 0 })
        .map_err(|e| e.to_string())?;
    let sc = lattice
        .place_node(Coord { row: 2, col: 2 }, Primitive::Split)
        .map_err(|e| e.to_string())?;
    lattice
        .connect(
            SignalSource::NodeOut { node: dc, port: 0 },
            SignalSink::NodeIn { node: sc, port: 0 },
        )
        .map_err(|e| e.to_string())?;
    lattice
        .connect(
            SignalSource::NodeOut { node: sc, port: 0 },
            SignalSink::NodeIn { node: dc, port: 0 },
        )
        .map_err(|e| e.to_string())?;
    lattice
        .connect(
            SignalSource::NodeOut { node: sc, port: 1 },
            SignalSink::NodeIn { node: add, port: 1 },
        )
        .map_err(|e| e.to_string())?;

    lattice
        .connect(
            SignalSource::NodeOut { node: add, port: 0 },
            SignalSink::BoundaryOut(0),
        )
        .map_err(|e| e.to_string())?;

    let detour_nodes = vec![dc, sc];
    let wire_steps = lattice.wire_steps_per_node().map_err(|e| e.to_string())?;
    let circuit = lattice.lower().map_err(|e| e.to_string())?;

    let generator = GeneratorConfig::T1_Alternating { a: 0, b: hi }
        .build(width)
        .map_err(|e| e.to_string())?;
    let mut env = NoisyEnvironment::new(generator, NoiseModel::None, SeedBank::new(0))
        .map_err(|e| e.to_string())?;
    let config = EconomyConfig {
        juvenile_ticks: 0,
        max_energy_reserve: None,
        ..EconomyConfig::default()
    };
    let mut live = LiveCircuit::new(circuit, &wire_steps, config, SeedBank::new(0), 8)
        .map_err(|e| e.to_string())?;

    let mut clean = vec![0u64; 1];
    let mut observed = vec![0u64; 1];
    let mut prev = vec![0u64; 1];
    let mut detour_credit_after_tick0 = 0.0f64;

    for t in 0..1024u64 {
        env.step_frame(&mut clean, &mut observed);
        let _ = live.predict(&prev).map_err(|e| e.to_string())?;
        let tick = live.settle(&observed).map_err(|e| e.to_string())?;
        if t >= 1 {
            for &node in &detour_nodes {
                detour_credit_after_tick0 += tick.economy.node_credit[node.index()];
            }
        }
        prev[0] = observed[0];
    }

    let detour_delta_e = live
        .economy()
        .nodes()
        .iter()
        .map(|n| n.energy - config.initial_energy)
        .sum();

    let (ref_delta_e, _) = run_spatial_osc2(width)?;
    Ok((detour_credit_after_tick0, detour_delta_e, ref_delta_e))
}

fn spearman(left: &[f64], right: &[f64]) -> f64 {
    fn ranks(values: &[f64]) -> Vec<f64> {
        let mut indices: Vec<usize> = (0..values.len()).collect();
        indices.sort_by(|&a, &b| values[a].total_cmp(&values[b]));
        let mut result = vec![0.0; values.len()];
        let mut start = 0usize;
        while start < indices.len() {
            let mut end = start + 1;
            while end < indices.len() && values[indices[end]] == values[indices[start]] {
                end += 1;
            }
            let rank = ((start + 1 + end) as f64) / 2.0;
            for &index in &indices[start..end] {
                result[index] = rank;
            }
            start = end;
        }
        result
    }
    if left.len() != right.len() || left.len() < 2 {
        return 0.0;
    }
    let left = ranks(left);
    let right = ranks(right);
    let n = left.len() as f64;
    let left_mean = left.iter().sum::<f64>() / n;
    let right_mean = right.iter().sum::<f64>() / n;
    let covariance: f64 = left
        .iter()
        .zip(&right)
        .map(|(&a, &b)| (a - left_mean) * (b - right_mean))
        .sum();
    let left_variance: f64 = left.iter().map(|&value| (value - left_mean).powi(2)).sum();
    let right_variance: f64 = right
        .iter()
        .map(|&value| (value - right_mean).powi(2))
        .sum();
    if left_variance == 0.0 || right_variance == 0.0 {
        0.0
    } else {
        covariance / (left_variance * right_variance).sqrt()
    }
}
