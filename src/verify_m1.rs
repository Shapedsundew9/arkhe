//! Pre-growth Milestone 1a metabolic-economy gates.

use serde::{Deserialize, Serialize};

use crate::config::BuildProvenance;
use crate::env::{FiniteStateGenerator, GeneratorConfig, NoiseModel, NoisyEnvironment};
use crate::metrics::{NullModelKind, null_calibrated_mdl_delta};
use crate::rng::SeedBank;
use crate::structural::{
    EconomyConfig, EconomyState, WiringTopology, counterfactual_sensitivity,
    ring_identity_input_weights,
};
use crate::substrate::{
    Circuit, Primitive, ScalarState, SignalSink, SignalSource, Width, WireDelayMode,
};

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
    /// Whether every M1a gate passed.
    pub all_passed: bool,
}

/// Runs the fixed-structure M1a gates. No growth or graph mutation is performed.
#[must_use]
pub fn run_m1a_verification_suite(seed_offset: u32, seeds: u32) -> M1aVerificationSummary {
    let seeds_tested = seeds.max(1);
    let mut gates = vec![
        verify_g1a1(),
        verify_g1a2(seed_offset, seeds_tested),
        verify_g1a3(),
        verify_g1a4(),
    ];
    for gate in &mut gates {
        gate.failures.sort();
    }
    let all_passed = gates.iter().all(|gate| gate.passed);
    M1aVerificationSummary {
        provenance: BuildProvenance::current(),
        seed_offset,
        seeds_tested,
        gates,
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

fn verify_g1a1() -> M1aGateReport {
    let mut failures = Vec::new();
    let substrate_sources = [
        include_str!("substrate/circuit.rs"),
        include_str!("substrate/eval.rs"),
        include_str!("substrate/invariants.rs"),
        include_str!("substrate/mod.rs"),
        include_str!("substrate/primitive.rs"),
        include_str!("substrate/value.rs"),
    ];
    if substrate_sources
        .iter()
        .any(|source| source.contains("f32") || source.contains("f64"))
    {
        failures.push("floating-point token found in src/substrate".to_owned());
    }

    for width in [Width::BITS_1, Width::BITS_2] {
        let config = EconomyConfig {
            juvenile_ticks: 0,
            max_energy_reserve: None,
            ..EconomyConfig::default()
        };
        let mut economy = EconomyState::new(config, width, 3, 1, 0).unwrap();
        let before: f64 = economy.nodes().iter().map(|node| node.energy).sum();
        let tick = economy
            .apply_tick(&[0.75], &[vec![1.0, 2.0, 1.0]], &[0.02, 0.035, 0.02])
            .unwrap();
        let after: f64 = economy.nodes().iter().map(|node| node.energy).sum();
        if ((after - before) - (tick.net_flux - tick.maintenance)).abs() >= 1e-12 {
            failures.push(format!("energy conservation failed at k={}", width.bits()));
        }
    }

    let width8 = Width::new(8).expect("k=8 is valid");
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
    state.step(&compiled, &[1], &mut [0]).unwrap();
    let exact =
        counterfactual_sensitivity(&circuit, &compiled, &state, SeedBank::new(5), 0, 255).unwrap();
    let sampled =
        counterfactual_sensitivity(&circuit, &compiled, &state, SeedBank::new(5), 0, 32).unwrap();
    if (exact.output_by_node[split.index()][0] - sampled.output_by_node[split.index()][0]).abs()
        > 0.05
    {
        failures
            .push("k=8 sampled sensitivity deviated from exact sensitivity by >0.05".to_owned());
    }
    gate_report("G1A.1", 5, failures)
}

fn verify_g1a2(seed_offset: u32, seeds: u32) -> M1aGateReport {
    let mut failures = Vec::new();
    let mut cases = 0u64;
    for width in [Width::BITS_1, Width::BITS_2] {
        let high = if width.bits() == 1 { 1 } else { 3 };
        let conditions = [
            (
                "alternating",
                GeneratorConfig::T1_Alternating { a: 0, b: high },
                PredictionKind::Clean,
                true,
                false,
            ),
            (
                "anti-correlated",
                GeneratorConfig::T1_Alternating { a: 0, b: high },
                PredictionKind::OffsetOne,
                false,
                false,
            ),
            (
                "sparse-exact",
                GeneratorConfig::T1_PeriodicMotif {
                    motif: vec![0, 0, 0, 0, 0, high],
                },
                PredictionKind::Clean,
                true,
                false,
            ),
            (
                "sparse-lazy-majority",
                GeneratorConfig::T1_PeriodicMotif {
                    motif: vec![0, 0, 0, 0, 0, high],
                },
                PredictionKind::Zero,
                false,
                false,
            ),
            (
                "silent-stream",
                GeneratorConfig::T1_Constant { value: 0 },
                PredictionKind::Clean,
                false,
                true,
            ),
        ];
        for epsilon in [0.0, 0.05] {
            for seed in seed_offset..seed_offset.saturating_add(seeds) {
                for (name, generator, predictor, should_gain, constant) in &conditions {
                    cases += 1;
                    match energy_delta(
                        width,
                        generator,
                        *predictor,
                        epsilon,
                        u64::from(seed),
                        *constant,
                    ) {
                        Ok(delta) if (*should_gain && delta > 0.0) || (!*should_gain && delta < 0.0) => {}
                        Ok(delta) => failures.push(format!(
                            "{name} at k={}, epsilon={epsilon}, seed={seed}: expected {} energy, got {delta}",
                            width.bits(),
                            if *should_gain { "gain" } else { "loss" }
                        )),
                        Err(error) => failures.push(format!("{name}: {error}")),
                    }
                }
            }
        }
    }
    gate_report("G1A.2", cases, failures)
}

#[derive(Clone, Copy)]
enum PredictionKind {
    Clean,
    OffsetOne,
    Zero,
}

fn energy_delta(
    width: Width,
    generator_config: &GeneratorConfig,
    prediction_kind: PredictionKind,
    epsilon: f64,
    seed: u64,
    constant: bool,
) -> Result<f64, String> {
    let generator = generator_config
        .build(width)
        .map_err(|error| error.to_string())?;
    let channels = generator.channels();
    let noise = if epsilon == 0.0 {
        NoiseModel::None
    } else {
        NoiseModel::IndependentSubstitution {
            epsilon,
            channels: (0..channels).collect(),
        }
    };
    let mut environment =
        NoisyEnvironment::new(generator, noise, SeedBank::new(seed)).map_err(|e| e.to_string())?;
    let config = EconomyConfig {
        juvenile_ticks: 0,
        max_energy_reserve: None,
        ..EconomyConfig::default()
    };
    let mut economy = EconomyState::new(config, width, 1, 1, 0).map_err(|e| e.to_string())?;
    let mut clean = vec![0; usize::from(channels)];
    let mut observed = vec![0; usize::from(channels)];
    for _ in 0..512 {
        environment.step_frame(&mut clean, &mut observed);
        let prediction = match prediction_kind {
            PredictionKind::Clean => clean[0],
            PredictionKind::OffsetOne => width.add(clean[0], 1),
            PredictionKind::Zero => 0,
        };
        let rewards = economy
            .observe_predictions(&[prediction], &[observed[0]])
            .map_err(|e| e.to_string())?;
        economy
            .apply_tick(&rewards, &[vec![1.0]], &[config.c_base])
            .map_err(|e| e.to_string())?;
    }
    let delta = economy.nodes()[0].energy - config.initial_energy;
    if constant && delta >= 0.0 {
        return Err(format!("constant stream reserve did not decrease: {delta}"));
    }
    Ok(delta)
}

fn verify_g1a3() -> M1aGateReport {
    let mut failures = Vec::new();
    for width in [Width::BITS_1, Width::BITS_2] {
        let mut circuit = Circuit::new(width, 2, 1, WireDelayMode::Combinational);
        let branch = circuit.add_node(Primitive::Split);
        let add = circuit.add_node(Primitive::Add);
        let sub = circuit.add_node(Primitive::Sub);
        circuit.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn {
                node: branch,
                port: 0,
            },
        );
        circuit.connect(
            SignalSource::BoundaryIn(1),
            SignalSink::NodeIn { node: add, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut {
                node: branch,
                port: 0,
            },
            SignalSink::NodeIn { node: add, port: 1 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: add, port: 0 },
            SignalSink::NodeIn { node: sub, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut {
                node: branch,
                port: 1,
            },
            SignalSink::NodeIn { node: sub, port: 1 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: sub, port: 0 },
            SignalSink::BoundaryOut(0),
        );
        let compiled = circuit.compile().unwrap();
        let mut state = ScalarState::new(&compiled);
        state.step(&compiled, &[1, 0], &mut [0]).unwrap();
        let snapshot =
            counterfactual_sensitivity(&circuit, &compiled, &state, SeedBank::new(1), 0, 8)
                .unwrap();
        if snapshot.output_by_node[branch.index()][0] != 0.0 {
            failures.push(format!(
                "cancelling branch received sensitivity at k={}",
                width.bits()
            ));
        }
        if ring_identity_input_weights(Primitive::Add, &[1, 0], &[true, false], width) != [1.0, 0.0]
        {
            failures.push(format!(
                "stuck-zero additive identity was not gated at k={}",
                width.bits()
            ));
        }
        if ring_identity_input_weights(Primitive::Mul, &[1, 0], &[true, false], width) != [0.0, 1.0]
        {
            failures.push(format!(
                "multiplicative-zero detour was not gated at k={}",
                width.bits()
            ));
        }
    }
    gate_report("G1A.3", 6, failures)
}

fn verify_g1a4() -> M1aGateReport {
    let mut failures = Vec::new();
    let mut correlations = Vec::new();
    for width in [Width::BITS_1, Width::BITS_2] {
        let motif = if width.bits() == 1 {
            vec![0, 1, 1]
        } else {
            vec![0, 1, 3]
        };
        let facts: Vec<u64> = (0..600).map(|tick| motif[tick % motif.len()]).collect();
        let predictions: Vec<Vec<u64>> = vec![
            facts.clone(),
            (0..facts.len())
                .map(|tick| if tick % 3 == 2 { 0 } else { motif[tick % 3] })
                .collect(),
            vec![0; facts.len()],
            facts.iter().map(|&fact| width.add(fact, 1)).collect(),
            facts.clone(),
            facts.clone(),
        ];
        let node_counts = [1usize, 2, 2, 2, 3, 4];
        let mut energy_changes = Vec::new();
        let mut mdl_improvements = Vec::new();
        for (case, (prediction, &node_count)) in predictions.iter().zip(&node_counts).enumerate() {
            let mut circuit = Circuit::new(width, 0, 1, WireDelayMode::Combinational);
            for _ in 0..node_count {
                circuit.add_node(Primitive::Delay { init: 0 });
            }
            let delta_l = null_calibrated_mdl_delta(
                &circuit,
                WiringTopology::WellMixed,
                None,
                0.0,
                0,
                &facts,
                prediction,
                NullModelKind::LaplaceFrequency,
            )
            .unwrap();

            let config = EconomyConfig {
                juvenile_ticks: 0,
                max_energy_reserve: None,
                ..EconomyConfig::default()
            };
            let mut economy = EconomyState::new(config, width, node_count, 1, 0).unwrap();
            let ancestry = if case >= 4 {
                let mut weights = vec![0.0; node_count];
                weights[0] = 1.0;
                weights
            } else {
                vec![1.0; node_count]
            };
            for (&prediction, &fact) in prediction.iter().zip(&facts) {
                let reward = economy.observe_predictions(&[prediction], &[fact]).unwrap();
                economy
                    .apply_tick(
                        &reward,
                        std::slice::from_ref(&ancestry),
                        &vec![config.c_base; node_count],
                    )
                    .unwrap();
            }
            let delta_e: f64 = economy
                .nodes()
                .iter()
                .map(|node| node.energy - config.initial_energy)
                .sum();
            if delta_e > 0.0 && delta_l > 0.0 {
                failures.push(format!(
                    "sign inversion at k={}, panel case {case}: deltaE={delta_e}, deltaL={delta_l}",
                    width.bits()
                ));
            }
            energy_changes.push(delta_e);
            mdl_improvements.push(-delta_l);
        }
        correlations.push((
            spearman(&energy_changes, &mdl_improvements),
            energy_changes,
            mdl_improvements,
        ));
    }
    for (index, (rho, energy, mdl)) in correlations.iter().enumerate() {
        if *rho < 0.90 {
            failures.push(format!(
                "Spearman correlation for k={} was {rho:.4}, expected >=0.90 (energy={energy:?}, -deltaL={mdl:?})",
                index + 1,
            ));
        }
    }
    gate_report("G1A.4", 12, failures)
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
