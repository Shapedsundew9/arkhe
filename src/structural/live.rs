//! Live circuit driver executing closed-loop substrate and metabolic steps.

use std::fmt;

use crate::rng::SeedBank;
use crate::structural::economy::{EconomyConfig, EconomyError, EconomyState, EconomyTick};
use crate::structural::sensitivity::{
    CausalAncestry, SensitivityError, assemble_causal_ancestry, counterfactual_sensitivity,
};
use crate::substrate::{
    Circuit, CompiledCircuit, EvalError, InvariantError, ScalarState, SignalSink, SignalSource,
};

/// Result of settling one live tick against observed facts.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveTick {
    /// Clock index of the evaluated tick.
    pub tick: u64,
    /// Predictions emitted across all output ports for this tick.
    pub predictions: Vec<u64>,
    /// Centered surprisal or score paid across all output ports.
    pub port_rewards: Vec<f64>,
    /// Metabolic reserve delta and maintenance accounting.
    pub economy: EconomyTick,
}

/// Errors raised during closed-loop live circuit stepping.
#[derive(Debug)]
pub enum LiveError {
    /// Substrate evaluation failure.
    Eval(EvalError),
    /// Counterfactual sensitivity or causal ancestry failure.
    Sensitivity(SensitivityError),
    /// Metabolic accounting, reward scoring, or trace failure.
    Economy(EconomyError),
    /// Circuit lowering or compilation invariant failure.
    Invariant(InvariantError),
    /// Out-of-order execution of the predict/settle cycle.
    Protocol(&'static str),
}

impl fmt::Display for LiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Eval(err) => write!(f, "evaluation error: {err}"),
            Self::Sensitivity(err) => write!(f, "sensitivity error: {err}"),
            Self::Economy(err) => write!(f, "economy error: {err}"),
            Self::Invariant(err) => write!(f, "invariant error: {err}"),
            Self::Protocol(msg) => write!(f, "protocol error: {msg}"),
        }
    }
}

impl std::error::Error for LiveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Eval(err) => Some(err),
            Self::Sensitivity(err) => Some(err),
            Self::Economy(err) => Some(err),
            Self::Invariant(err) => Some(err),
            Self::Protocol(_) => None,
        }
    }
}

#[derive(Clone, Debug)]
struct Pending {
    predictions: Vec<u64>,
    ancestry: CausalAncestry,
}

/// Closed-loop driver running a fixed circuit through metabolic accounting.
pub struct LiveCircuit {
    circuit: Circuit,
    compiled: CompiledCircuit,
    state: ScalarState,
    economy: EconomyState,
    maintenance: Vec<f64>,
    seed_bank: SeedBank,
    sample_count: usize,
    tick: u64,
    pending: Option<Pending>,
}

impl LiveCircuit {
    /// Initializes a live execution harness for `circuit` with wire-step maintenance.
    pub fn new(
        circuit: Circuit,
        wire_steps: &[u32],
        config: EconomyConfig,
        seed_bank: SeedBank,
        sample_count: usize,
    ) -> Result<Self, LiveError> {
        if wire_steps.len() != circuit.node_count() {
            return Err(LiveError::Economy(EconomyError::DimensionMismatch));
        }
        let compiled = circuit.compile().map_err(LiveError::Invariant)?;
        let state = ScalarState::new(&compiled);
        let economy = EconomyState::new(
            config,
            circuit.width(),
            circuit.node_count(),
            usize::from(circuit.num_outputs()),
            compiled.total_registers(),
        )
        .map_err(LiveError::Economy)?;
        let maintenance: Vec<f64> = circuit
            .nodes()
            .iter()
            .zip(wire_steps)
            .map(|(&primitive, &steps)| economy.maintenance_cost(primitive, steps))
            .collect();

        Ok(Self {
            circuit,
            compiled,
            state,
            economy,
            maintenance,
            seed_bank,
            sample_count,
            tick: 0,
            pending: None,
        })
    }

    /// Steps the substrate with `inputs`, records activity, computes sensitivity and
    /// ancestry, caches them, and returns the predictions. Errors if called twice without `settle`.
    pub fn predict(&mut self, inputs: &[u64]) -> Result<Vec<u64>, LiveError> {
        if self.pending.is_some() {
            return Err(LiveError::Protocol(
                "predict called with pending prediction",
            ));
        }
        let mut predictions = vec![0u64; usize::from(self.circuit.num_outputs())];
        self.state
            .step(&self.compiled, inputs, &mut predictions)
            .map_err(LiveError::Eval)?;
        let node_outputs = node_output_values(&self.circuit, &self.state);
        self.economy
            .record_activity(&node_outputs)
            .map_err(LiveError::Economy)?;
        let snapshot = counterfactual_sensitivity(
            &self.circuit,
            &self.compiled,
            &self.state,
            self.seed_bank,
            self.tick,
            self.sample_count,
        )
        .map_err(LiveError::Sensitivity)?;
        let activity = self.economy.activity();
        let ancestry = assemble_causal_ancestry(
            &self.circuit,
            &self.compiled,
            &self.state,
            &snapshot,
            &activity,
            self.economy.register_eligibility(),
            self.economy.config().trace_discount,
        )
        .map_err(LiveError::Sensitivity)?;

        self.pending = Some(Pending {
            predictions: predictions.clone(),
            ancestry,
        });
        Ok(predictions)
    }

    /// Scores the cached predictions against `facts`, applies the conserved payout and
    /// maintenance via `apply_causal_tick`, increments `tick`. Errors if no prediction is pending.
    pub fn settle(&mut self, facts: &[u64]) -> Result<LiveTick, LiveError> {
        let Some(pending) = self.pending.take() else {
            return Err(LiveError::Protocol(
                "settle called without pending prediction",
            ));
        };
        let port_rewards = self
            .economy
            .observe_predictions(&pending.predictions, facts)
            .map_err(LiveError::Economy)?;
        let economy_tick = self
            .economy
            .apply_causal_tick(&port_rewards, &pending.ancestry, &self.maintenance)
            .map_err(LiveError::Economy)?;
        let live_tick = LiveTick {
            tick: self.tick,
            predictions: pending.predictions,
            port_rewards,
            economy: economy_tick,
        };
        self.tick += 1;
        Ok(live_tick)
    }

    /// Abstract circuit topology.
    #[must_use]
    pub const fn circuit(&self) -> &Circuit {
        &self.circuit
    }

    /// Metabolic economy state.
    #[must_use]
    pub const fn economy(&self) -> &EconomyState {
        &self.economy
    }

    /// Live substrate register and wire state.
    #[must_use]
    pub const fn state(&self) -> &ScalarState {
        &self.state
    }

    /// Current tick index.
    #[must_use]
    pub const fn tick(&self) -> u64 {
        self.tick
    }
}

/// WellMixed wire steps: 1 per incoming NodeIn edge + 1 per BoundaryOut edge driven by the node.
#[must_use]
pub fn well_mixed_wire_steps(circuit: &Circuit) -> Vec<u32> {
    let mut steps = vec![0u32; circuit.node_count()];
    for edge in circuit.edges() {
        let charged_node = match (edge.src, edge.dst) {
            (_, SignalSink::NodeIn { node, .. }) => Some(node),
            (SignalSource::NodeOut { node, .. }, SignalSink::BoundaryOut(_)) => Some(node),
            _ => None,
        };
        if let Some(total) = charged_node.and_then(|node| steps.get_mut(node.index())) {
            *total = total.saturating_add(1);
        }
    }
    steps
}

/// Value of output port 0 of every node on the current tick (from `state.wire_values()`).
#[must_use]
pub fn node_output_values(circuit: &Circuit, state: &ScalarState) -> Vec<u64> {
    let mut node_out_base = Vec::with_capacity(circuit.node_count());
    let mut next_wire = usize::from(circuit.num_inputs());
    for primitive in circuit.nodes() {
        node_out_base.push(next_wire);
        next_wire += usize::from(primitive.output_arity());
    }
    node_out_base
        .into_iter()
        .map(|wire| state.wire_values()[wire])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::substrate::{NodeId, Primitive, Width, WireDelayMode};

    #[test]
    fn conservation_holds_every_tick() {
        // Osc2(0, 1): D1 = Delay(1), D2 = Delay(0), S = Split
        // D1 -> D2 -> S; S.out0 -> D1, S.out1 -> Out
        let mut circuit = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);
        let d1 = circuit.add_node(Primitive::Delay { init: 1 });
        let d2 = circuit.add_node(Primitive::Delay { init: 0 });
        let s = circuit.add_node(Primitive::Split);

        circuit.connect(
            SignalSource::NodeOut { node: d1, port: 0 },
            SignalSink::NodeIn { node: d2, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: d2, port: 0 },
            SignalSink::NodeIn { node: s, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: s, port: 0 },
            SignalSink::NodeIn { node: d1, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: s, port: 1 },
            SignalSink::BoundaryOut(0),
        );

        let wire_steps = well_mixed_wire_steps(&circuit);
        let config = EconomyConfig {
            juvenile_ticks: 0,
            max_energy_reserve: None,
            ..EconomyConfig::default()
        };
        let mut live =
            LiveCircuit::new(circuit, &wire_steps, config, SeedBank::new(42), 8).unwrap();

        for t in 0..500 {
            let fact = [t % 2];
            let _pred = live.predict(&[0]).unwrap();
            let tick = live.settle(&fact).unwrap();
            let sum_delta: f64 = tick.economy.node_delta.iter().sum();
            let expected_change = tick.economy.net_flux - tick.economy.maintenance;
            assert!(
                (sum_delta - expected_change).abs() < 1e-12,
                "conservation failed at tick {t}: sum_delta={sum_delta}, expected={expected_change}"
            );
        }
    }

    #[test]
    fn delay_chain_shares_match_plan() {
        // 5-Delay shift chain: x -> D0 -> D1 -> D2 -> D3 -> D4 -> Out
        let mut circuit = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);
        let delays: Vec<NodeId> = (0..5)
            .map(|_| circuit.add_node(Primitive::Delay { init: 0 }))
            .collect();
        circuit.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn {
                node: delays[0],
                port: 0,
            },
        );
        for pair in delays.windows(2) {
            circuit.connect(
                SignalSource::NodeOut {
                    node: pair[0],
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: pair[1],
                    port: 0,
                },
            );
        }
        circuit.connect(
            SignalSource::NodeOut {
                node: delays[4],
                port: 0,
            },
            SignalSink::BoundaryOut(0),
        );

        let wire_steps = well_mixed_wire_steps(&circuit);
        let config = EconomyConfig {
            juvenile_ticks: 0,
            max_energy_reserve: None,
            ..EconomyConfig::default()
        };
        let mut live = LiveCircuit::new(circuit, &wire_steps, config, SeedBank::new(7), 8).unwrap();

        let mut last_tick = None;
        for t in 0..64 {
            let fact = (t % 2) as u64;
            let x = ((t + 5) % 2) as u64;
            let _pred = live.predict(&[x]).unwrap();
            let tick = live.settle(&[fact]).unwrap();
            last_tick = Some(tick);
        }

        let tick = last_tick.unwrap();
        assert!(tick.port_rewards[0] > 0.0);
        let total_credit: f64 = tick.economy.node_credit.iter().sum();
        assert!(total_credit > 0.0);
        let expected = [0.9f64.powi(4), 0.9f64.powi(3), 0.9f64.powi(2), 0.9, 1.0];
        let normalizer: f64 = expected.iter().sum();
        for (&credit, exp) in tick.economy.node_credit.iter().zip(expected) {
            let share = credit / total_credit;
            let target = exp / normalizer;
            assert!(
                (share - target).abs() < 1e-9,
                "share mismatch: got {share}, expected {target}"
            );
        }
    }

    #[test]
    fn juvenile_nodes_are_protected() {
        // ConstLoop(0) -> Out, facts are [1] (always wrong)
        let mut circuit = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);
        let d = circuit.add_node(Primitive::Delay { init: 0 });
        let s = circuit.add_node(Primitive::Split);
        circuit.connect(
            SignalSource::NodeOut { node: d, port: 0 },
            SignalSink::NodeIn { node: s, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: s, port: 0 },
            SignalSink::NodeIn { node: d, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: s, port: 1 },
            SignalSink::BoundaryOut(0),
        );

        let wire_steps = well_mixed_wire_steps(&circuit);
        let config = EconomyConfig {
            juvenile_ticks: 8,
            max_energy_reserve: None,
            ..EconomyConfig::default()
        };
        let mut live = LiveCircuit::new(circuit, &wire_steps, config, SeedBank::new(1), 8).unwrap();

        for t in 0..8 {
            let _pred = live.predict(&[0]).unwrap();
            let tick = live.settle(&[1]).unwrap();
            assert_eq!(
                tick.economy.maintenance, 0.0,
                "juvenile node charged maintenance at tick {t}"
            );
            for &delta in &tick.economy.node_delta {
                assert!(
                    delta >= 0.0,
                    "juvenile node lost energy at tick {t}: delta={delta}"
                );
            }
        }

        // From tick 8 onward, mature nodes pay maintenance and lose energy
        let _pred = live.predict(&[0]).unwrap();
        let tick = live.settle(&[1]).unwrap();
        assert!(
            tick.economy.maintenance > 0.0,
            "mature node must pay maintenance"
        );
        for &delta in &tick.economy.node_delta {
            assert!(delta < 0.0, "mature wrong node must lose energy: {delta}");
        }
    }

    #[test]
    fn predict_settle_protocol_errors() {
        let mut circuit = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);
        let d = circuit.add_node(Primitive::Delay { init: 0 });
        circuit.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: d, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: d, port: 0 },
            SignalSink::BoundaryOut(0),
        );

        let wire_steps = well_mixed_wire_steps(&circuit);
        let config = EconomyConfig::default();
        let mut live = LiveCircuit::new(circuit, &wire_steps, config, SeedBank::new(1), 8).unwrap();

        // Calling settle first is a protocol error
        assert!(matches!(live.settle(&[0]), Err(LiveError::Protocol(_))));

        // Calling predict once succeeds
        assert!(live.predict(&[0]).is_ok());

        // Calling predict twice without settle is a protocol error
        assert!(matches!(live.predict(&[0]), Err(LiveError::Protocol(_))));

        // Settling then succeeds
        assert!(live.settle(&[0]).is_ok());
    }

    #[test]
    fn deterministic_replay() {
        let build = || {
            let mut circuit = Circuit::new(Width::BITS_2, 1, 1, WireDelayMode::Combinational);
            let d1 = circuit.add_node(Primitive::Delay { init: 1 });
            let d2 = circuit.add_node(Primitive::Delay { init: 2 });
            let s = circuit.add_node(Primitive::Split);
            let add = circuit.add_node(Primitive::Add);

            circuit.connect(
                SignalSource::NodeOut { node: d1, port: 0 },
                SignalSink::NodeIn { node: d2, port: 0 },
            );
            circuit.connect(
                SignalSource::NodeOut { node: d2, port: 0 },
                SignalSink::NodeIn { node: s, port: 0 },
            );
            circuit.connect(
                SignalSource::NodeOut { node: s, port: 0 },
                SignalSink::NodeIn { node: d1, port: 0 },
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

            let wire_steps = well_mixed_wire_steps(&circuit);
            let config = EconomyConfig {
                juvenile_ticks: 4,
                ..EconomyConfig::default()
            };
            LiveCircuit::new(circuit, &wire_steps, config, SeedBank::new(99), 8).unwrap()
        };

        let mut live1 = build();
        let mut live2 = build();

        for t in 0..50 {
            let input = [t % 4];
            let fact = [(t + 1) % 4];
            live1.predict(&input).unwrap();
            live1.settle(&fact).unwrap();

            live2.predict(&input).unwrap();
            live2.settle(&fact).unwrap();
        }

        let energies1: Vec<f64> = live1.economy().nodes().iter().map(|n| n.energy).collect();
        let energies2: Vec<f64> = live2.economy().nodes().iter().map(|n| n.energy).collect();
        assert_eq!(energies1, energies2);
    }
}
