//! Live circuit driver executing closed-loop substrate and metabolic steps.

use std::fmt;

use crate::rng::SeedBank;
use crate::structural::economy::{
    ComparatorSite, EconomyConfig, EconomyError, EconomyState, EconomyTick, ExecutionPhase,
};
use crate::structural::sensitivity::{
    ComparatorCausalAncestry, ComparatorTarget, SensitivityError, assemble_causal_ancestry,
    assemble_causal_ancestry_with_comparators, counterfactual_sensitivity,
    counterfactual_sensitivity_with_comparators,
};
use crate::substrate::{
    Circuit, CompiledCircuit, EvalError, InvariantError, NodeId, Primitive, ScalarState,
    SignalSink, SignalSource,
};

/// Result of settling one live tick against observed facts.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveTick {
    /// Clock index of the evaluated tick.
    pub tick: u64,
    /// Predictions emitted across all output ports for this tick.
    pub predictions: Vec<u64>,
    /// Centered surprisal or score paid across all output ports (`0.0` in Sleep).
    pub port_rewards: Vec<f64>,
    /// Centered prequential rewards paid across registered comparators (empty if `M == 0`).
    pub comparator_rewards: Vec<f64>,
    /// Primary wire values observed at each comparator's `sub_node.in(0)`.
    pub comparator_primary: Vec<u64>,
    /// Alternative wire values observed at each comparator's `sub_node.in(1)`.
    pub comparator_alt: Vec<u64>,
    /// Equality detector values observed at each comparator's `eqz_node` output.
    pub comparator_eqz: Vec<u64>,
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
    ancestry: ComparatorCausalAncestry,
    comparator_primary: Vec<u64>,
    comparator_alt: Vec<u64>,
    comparator_eqz: Vec<u64>,
}

/// Closed-loop driver running a fixed circuit through metabolic accounting.
pub struct LiveCircuit {
    circuit: Circuit,
    compiled: CompiledCircuit,
    state: ScalarState,
    economy: EconomyState,
    seed_bank: SeedBank,
    sample_count: usize,
    tick: u64,
    pending: Option<Pending>,
}

impl LiveCircuit {
    /// Initializes a live execution harness for `circuit` with description debts.
    pub fn new(
        circuit: Circuit,
        description_debts: &[f64],
        config: EconomyConfig,
        seed_bank: SeedBank,
        sample_count: usize,
    ) -> Result<Self, LiveError> {
        if description_debts.len() != circuit.node_count() {
            return Err(LiveError::Economy(EconomyError::DimensionMismatch));
        }
        let compiled = circuit.compile().map_err(LiveError::Invariant)?;
        let state = ScalarState::new(&compiled);
        let obs_horizon = crate::structural::growth::observation_horizon(
            &config,
            &crate::structural::growth::GrowthConfig::default(),
        );
        let mut economy = EconomyState::new(
            config,
            circuit.width(),
            circuit.node_count(),
            usize::from(circuit.num_outputs()),
            compiled.total_registers(),
            obs_horizon,
        )
        .map_err(LiveError::Economy)?;
        economy.assign_sprout_debts(description_debts);

        Ok(Self {
            circuit,
            compiled,
            state,
            economy,
            seed_bank,
            sample_count,
            tick: 0,
            pending: None,
        })
    }

    /// Current execution phase (`Wake` or `Sleep`).
    #[must_use]
    pub const fn phase(&self) -> ExecutionPhase {
        self.economy.phase()
    }

    /// Switches execution phase between `Wake` and `Sleep` (freezes wake null models in `Sleep`).
    pub fn set_phase(&mut self, phase: ExecutionPhase) -> Result<(), LiveError> {
        if self.pending.is_some() {
            return Err(LiveError::Protocol(
                "set_phase called with pending prediction",
            ));
        }
        self.economy.set_phase(phase);
        Ok(())
    }

    /// Registers an internal comparator `EQZ(SUB(primary, alt))` attached to `target_port`.
    pub fn register_comparator(
        &mut self,
        sub_node: NodeId,
        eqz_node: NodeId,
        target_port: u16,
        primary_channel_mask: u64,
        alt_channel_mask: u64,
    ) -> Result<usize, LiveError> {
        if self.pending.is_some() {
            return Err(LiveError::Protocol(
                "register_comparator called with pending prediction",
            ));
        }
        let valid_nodes = matches!(
            self.circuit.nodes().get(sub_node.index()),
            Some(Primitive::Sub)
        ) && matches!(
            self.circuit.nodes().get(eqz_node.index()),
            Some(Primitive::Eqz)
        ) && self
            .compiled
            .node_input_wires()
            .get(sub_node.index())
            .is_some_and(|wires| wires.len() == 2);
        if !valid_nodes {
            return Err(LiveError::Economy(EconomyError::DimensionMismatch));
        }
        self.economy
            .register_comparator(
                sub_node,
                eqz_node,
                target_port,
                primary_channel_mask,
                alt_channel_mask,
            )
            .map_err(LiveError::Economy)
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

        let comparators = self.economy.comparators();
        let is_sleep = self.economy.phase() == ExecutionPhase::Sleep;
        let (ancestry, comparator_primary, comparator_alt, comparator_eqz) =
            if comparators.is_empty() && !is_sleep {
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
                let base = assemble_causal_ancestry(
                    &self.circuit,
                    &self.compiled,
                    &self.state,
                    &snapshot,
                    &activity,
                    self.economy.register_eligibility(),
                    self.economy.config().trace_discount,
                )
                .map_err(LiveError::Sensitivity)?;
                (
                    ComparatorCausalAncestry {
                        base,
                        comparator_shares: Vec::new(),
                    },
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                )
            } else {
                let targets: Vec<ComparatorTarget> =
                    comparators.iter().map(ComparatorSite::target).collect();
                let wires = self.state.wire_values();
                let mut pri_vals = Vec::with_capacity(comparators.len());
                let mut alt_vals = Vec::with_capacity(comparators.len());
                let mut eqz_vals = Vec::with_capacity(comparators.len());
                for comp in comparators {
                    let sub_inputs = &self.compiled.node_input_wires()[comp.sub_node.index()];
                    pri_vals.push(wires[sub_inputs[0]]);
                    alt_vals.push(wires[sub_inputs[1]]);
                    eqz_vals.push(node_outputs[comp.eqz_node.index()]);
                }
                let snapshot = counterfactual_sensitivity_with_comparators(
                    &self.circuit,
                    &self.compiled,
                    &self.state,
                    self.seed_bank,
                    self.tick,
                    self.sample_count,
                    &targets,
                )
                .map_err(LiveError::Sensitivity)?;
                let activity = self.economy.activity();
                let comp_ancestry = assemble_causal_ancestry_with_comparators(
                    &self.circuit,
                    &self.compiled,
                    &self.state,
                    &snapshot,
                    &activity,
                    self.economy.register_eligibility(),
                    self.economy.config().trace_discount,
                    &targets,
                    is_sleep,
                )
                .map_err(LiveError::Sensitivity)?;
                (comp_ancestry, pri_vals, alt_vals, eqz_vals)
            };

        self.pending = Some(Pending {
            predictions: predictions.clone(),
            ancestry,
            comparator_primary,
            comparator_alt,
            comparator_eqz,
        });
        Ok(predictions)
    }

    /// Scores the cached predictions against `facts`, applies the conserved payout and
    /// maintenance via `apply_causal_tick`, increments `tick`. Errors if no prediction is pending.
    pub fn settle(&mut self, facts: &[u64]) -> Result<LiveTick, LiveError> {
        self.settle_core(Some(facts))
    }

    /// Settles one pending prediction during [`ExecutionPhase::Sleep`] with no external observations.
    pub fn settle_sleep(&mut self) -> Result<LiveTick, LiveError> {
        if self.economy.phase() != ExecutionPhase::Sleep {
            return Err(LiveError::Protocol(
                "settle_sleep called outside ExecutionPhase::Sleep",
            ));
        }
        self.settle_core(None)
    }

    fn settle_core(&mut self, facts: Option<&[u64]>) -> Result<LiveTick, LiveError> {
        let Some(pending) = self.pending.take() else {
            return Err(LiveError::Protocol(
                "settle called without pending prediction",
            ));
        };
        let is_sleep = self.economy.phase() == ExecutionPhase::Sleep;
        let has_comparators = !self.economy.comparators().is_empty();

        let port_rewards = if is_sleep {
            if let Some(f) = facts
                && f.len() != self.economy.scorers().len()
            {
                return Err(LiveError::Economy(EconomyError::DimensionMismatch));
            }
            vec![0.0; self.economy.scorers().len()]
        } else {
            let Some(f) = facts else {
                return Err(LiveError::Protocol("settle called without wake facts"));
            };
            self.economy
                .observe_predictions(&pending.predictions, f)
                .map_err(LiveError::Economy)?
        };

        let mut maintenance = vec![0.0; self.circuit.node_count()];
        for (index, node) in self.economy.nodes_mut().iter_mut().enumerate() {
            if !node.debt_paid && node.juvenile_ticks == 0 {
                maintenance[index] = node.description_debt;
                node.debt_paid = true;
            }
        }

        let (comparator_rewards, economy_tick) = if !has_comparators && !is_sleep {
            let econ_tick = self
                .economy
                .apply_causal_tick(&port_rewards, &pending.ancestry.base, &maintenance)
                .map_err(LiveError::Economy)?;
            (Vec::new(), econ_tick)
        } else {
            let comp_obs = self
                .economy
                .observe_comparators(
                    &pending.comparator_primary,
                    &pending.comparator_alt,
                    &pending.comparator_eqz,
                    facts,
                )
                .map_err(LiveError::Economy)?;
            let econ_tick = self
                .economy
                .apply_causal_tick_with_comparators(
                    &port_rewards,
                    &comp_obs.rewards,
                    comp_obs.agreements,
                    comp_obs.disagreements,
                    &pending.ancestry,
                    &maintenance,
                )
                .map_err(LiveError::Economy)?;
            (comp_obs.rewards, econ_tick)
        };

        let live_tick = LiveTick {
            tick: self.tick,
            predictions: pending.predictions,
            port_rewards,
            comparator_rewards,
            comparator_primary: pending.comparator_primary,
            comparator_alt: pending.comparator_alt,
            comparator_eqz: pending.comparator_eqz,
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

    /// Mutable metabolic economy state.
    pub fn economy_mut(&mut self) -> &mut EconomyState {
        &mut self.economy
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

/// Description debts for a circuit (Level-0 palette).
#[must_use]
pub fn well_mixed_description_debts(circuit: &Circuit) -> Vec<f64> {
    crate::metrics::circuit_node_description_debts(circuit, 0)
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
    use crate::structural::economy::ReserveCap;
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

        let debts = well_mixed_description_debts(&circuit);
        let config = EconomyConfig {
            juvenile_ticks: 0,
            reserve_cap: ReserveCap::Uncapped,
            ..EconomyConfig::default()
        };
        let mut live = LiveCircuit::new(circuit, &debts, config, SeedBank::new(42), 8).unwrap();

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

        let debts = well_mixed_description_debts(&circuit);
        let config = EconomyConfig {
            juvenile_ticks: 0,
            reserve_cap: ReserveCap::Uncapped,
            ..EconomyConfig::default()
        };
        let mut live = LiveCircuit::new(circuit, &debts, config, SeedBank::new(7), 8).unwrap();

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

        let debts = well_mixed_description_debts(&circuit);
        let config = EconomyConfig {
            juvenile_ticks: 8,
            reserve_cap: ReserveCap::Uncapped,
            ..EconomyConfig::default()
        };
        let mut live = LiveCircuit::new(circuit, &debts, config, SeedBank::new(1), 8).unwrap();

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

        let debts = well_mixed_description_debts(&circuit);
        let config = EconomyConfig::default();
        let mut live = LiveCircuit::new(circuit, &debts, config, SeedBank::new(1), 8).unwrap();

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

            let debts = well_mixed_description_debts(&circuit);
            let config = EconomyConfig {
                juvenile_ticks: 4,
                ..EconomyConfig::default()
            };
            LiveCircuit::new(circuit, &debts, config, SeedBank::new(99), 8).unwrap()
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

    #[test]
    fn epistemic_mdl_conservation_across_m_clean_noisy_wake_and_sleep() {
        use crate::metrics::NullModelKind;
        use crate::metrics::mdl::{
            EpistemicComparatorTrace, EpistemicTickPhase, epistemic_null_calibrated_mdl_delta,
        };

        // Build parameterized circuit with 0, 1, or 2 comparators on BoundaryOut(0).
        fn run_case(num_comparators: usize, noisy: bool, include_sleep: bool) {
            let num_inputs = 1 + num_comparators as u16;
            let mut circuit =
                Circuit::new(Width::BITS_1, num_inputs, 1, WireDelayMode::Combinational);
            let pri_d = circuit.add_node(Primitive::Delay { init: 0 });
            circuit.connect(
                SignalSource::BoundaryIn(0),
                SignalSink::NodeIn {
                    node: pri_d,
                    port: 0,
                },
            );

            let mut pri_sink_source = SignalSource::NodeOut {
                node: pri_d,
                port: 0,
            };
            let mut comp_pairs = Vec::new();
            for m in 0..num_comparators {
                let split = circuit.add_node(Primitive::Split);
                circuit.connect(
                    pri_sink_source,
                    SignalSink::NodeIn {
                        node: split,
                        port: 0,
                    },
                );
                let alt_d = circuit.add_node(Primitive::Delay { init: 0 });
                circuit.connect(
                    SignalSource::BoundaryIn((m + 1) as u16),
                    SignalSink::NodeIn {
                        node: alt_d,
                        port: 0,
                    },
                );
                let sub = circuit.add_node(Primitive::Sub);
                circuit.connect(
                    SignalSource::NodeOut {
                        node: split,
                        port: 1,
                    },
                    SignalSink::NodeIn { node: sub, port: 0 },
                );
                circuit.connect(
                    SignalSource::NodeOut {
                        node: alt_d,
                        port: 0,
                    },
                    SignalSink::NodeIn { node: sub, port: 1 },
                );
                let eqz = circuit.add_node(Primitive::Eqz);
                circuit.connect(
                    SignalSource::NodeOut { node: sub, port: 0 },
                    SignalSink::NodeIn { node: eqz, port: 0 },
                );
                pri_sink_source = SignalSource::NodeOut {
                    node: split,
                    port: 0,
                };
                comp_pairs.push((sub, eqz, m + 1));
            }
            circuit.connect(pri_sink_source, SignalSink::BoundaryOut(0));

            let debts = well_mixed_description_debts(&circuit);
            let config = EconomyConfig {
                juvenile_ticks: 0,
                reserve_cap: ReserveCap::Uncapped,
                ..EconomyConfig::default()
            };
            let mut live =
                LiveCircuit::new(circuit.clone(), &debts, config, SeedBank::new(7), 8).unwrap();
            for &(sub, eqz, ch_idx) in &comp_pairs {
                live.register_comparator(sub, eqz, 0, 0b01, 1u64 << ch_idx)
                    .unwrap();
            }

            let e0: f64 = live.economy().nodes().iter().map(|n| n.energy).sum();
            let wake_ticks = 64usize;
            let sleep_ticks = if include_sleep { 24usize } else { 0usize };

            let mut facts = Vec::new();
            let mut preds = Vec::new();
            let mut phases = Vec::new();
            let mut pri_traces = vec![Vec::new(); num_comparators];
            let mut alt_traces = vec![Vec::new(); num_comparators];

            for t in 0..wake_ticks {
                let signal = (t % 2) as u64;
                // Under eps ~ 0.05 (1 in 20 ticks), flip ground-truth target observation or alt input.
                let fact = if noisy && t % 20 == 13 {
                    1 - signal
                } else {
                    signal
                };
                let mut inputs = vec![signal; usize::from(num_inputs)];
                if noisy && t % 19 == 7 && num_inputs > 1 {
                    inputs[1] = 1 - signal;
                }
                let pred = live.predict(&inputs).unwrap();
                let tick = live.settle(&[fact]).unwrap();
                assert_eq!(tick.economy.clipped_nodes, 0);
                facts.push(fact);
                preds.push(pred[0]);
                phases.push(EpistemicTickPhase::Wake);
                for m in 0..num_comparators {
                    pri_traces[m].push(tick.comparator_primary[m]);
                    alt_traces[m].push(tick.comparator_alt[m]);
                }
            }

            if include_sleep {
                live.set_phase(ExecutionPhase::Sleep).unwrap();
                let mut prev = preds.last().copied().unwrap_or(0);
                for t in 0..sleep_ticks {
                    // Self-loop loopback input plus occasional noisy disagreement for Sleep mismatch check.
                    let mut inputs = vec![prev; usize::from(num_inputs)];
                    if noisy && num_inputs > 1 && t % 11 == 3 {
                        inputs[1] = 1 - prev;
                    }
                    let pred = live.predict(&inputs).unwrap();
                    let tick = live.settle_sleep().unwrap();
                    assert_eq!(tick.port_rewards, vec![0.0]);
                    facts.push(0);
                    preds.push(pred[0]);
                    phases.push(EpistemicTickPhase::Sleep);
                    for m in 0..num_comparators {
                        pri_traces[m].push(tick.comparator_primary[m]);
                        alt_traces[m].push(tick.comparator_alt[m]);
                    }
                    prev = pred[0];
                }
            }

            let e_end: f64 = live.economy().nodes().iter().map(|n| n.energy).sum();
            let delta_e = e_end - e0;

            let comp_traces: Vec<EpistemicComparatorTrace<'_>> = comp_pairs
                .iter()
                .enumerate()
                .map(|(m, &(_, _, ch_idx))| EpistemicComparatorTrace {
                    target_port: 0,
                    registered_at_tick: 0,
                    primary_channel_mask: 0b01,
                    alt_channel_mask: 1u64 << ch_idx,
                    primary_predictions: &pri_traces[m],
                    alt_predictions: &alt_traces[m],
                })
                .collect();

            let epistemic_mdl = epistemic_null_calibrated_mdl_delta(
                &circuit,
                0.0,
                0,
                &[facts.as_slice()],
                &[preds.as_slice()],
                &phases,
                &comp_traces,
                NullModelKind::LaplaceFrequency,
            )
            .unwrap();

            assert!(
                (delta_e + epistemic_mdl).abs() < 1e-12,
                "MDL conservation violated (M={num_comparators}, noisy={noisy}, sleep={include_sleep}): \
                 delta_e={delta_e}, -epistemic_mdl={}",
                -epistemic_mdl
            );
        }

        for m in [0usize, 1, 2] {
            for noisy in [false, true] {
                for sleep in [false, true] {
                    run_case(m, noisy, sleep);
                }
            }
        }
    }

    #[test]
    fn silent_comparator_scores_zero_and_wake_isolates_primary() {
        // Silent 0==0 circuit: pri_d=Delay(0), split, alt_d=Delay(0), sub, eqz
        let mut circuit = Circuit::new(Width::BITS_1, 2, 1, WireDelayMode::Combinational);
        let pri_d = circuit.add_node(Primitive::Delay { init: 0 });
        let split = circuit.add_node(Primitive::Split);
        let alt_d = circuit.add_node(Primitive::Delay { init: 0 });
        let sub = circuit.add_node(Primitive::Sub);
        let eqz = circuit.add_node(Primitive::Eqz);

        circuit.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn {
                node: pri_d,
                port: 0,
            },
        );
        circuit.connect(
            SignalSource::NodeOut {
                node: pri_d,
                port: 0,
            },
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
            SignalSink::BoundaryOut(0),
        );
        circuit.connect(
            SignalSource::NodeOut {
                node: split,
                port: 1,
            },
            SignalSink::NodeIn { node: sub, port: 0 },
        );
        circuit.connect(
            SignalSource::BoundaryIn(1),
            SignalSink::NodeIn {
                node: alt_d,
                port: 0,
            },
        );
        circuit.connect(
            SignalSource::NodeOut {
                node: alt_d,
                port: 0,
            },
            SignalSink::NodeIn { node: sub, port: 1 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: sub, port: 0 },
            SignalSink::NodeIn { node: eqz, port: 0 },
        );

        let debts = well_mixed_description_debts(&circuit);
        let config = EconomyConfig {
            juvenile_ticks: 0,
            reserve_cap: ReserveCap::Uncapped,
            ..EconomyConfig::default()
        };
        let mut live = LiveCircuit::new(circuit, &debts, config, SeedBank::new(1), 8).unwrap();
        live.register_comparator(sub, eqz, 0, 0b01, 0b10).unwrap();

        for _ in 0..80 {
            live.predict(&[0, 0]).unwrap();
            let tick = live.settle(&[0]).unwrap();
            assert_eq!(tick.economy.comparator_flux, 0.0);
        }
        live.set_phase(ExecutionPhase::Sleep).unwrap();
        for _ in 0..40 {
            live.predict(&[0, 0]).unwrap();
            let tick = live.settle_sleep().unwrap();
            assert_eq!(tick.economy.comparator_flux, 0.0);
        }

        // Now verify Wake isolates primary when an informative signal runs.
        let mut circuit2 = Circuit::new(Width::BITS_1, 2, 1, WireDelayMode::Combinational);
        let p = circuit2.add_node(Primitive::Delay { init: 0 });
        let s = circuit2.add_node(Primitive::Split);
        let a = circuit2.add_node(Primitive::Delay { init: 0 });
        let sub2 = circuit2.add_node(Primitive::Sub);
        let eqz2 = circuit2.add_node(Primitive::Eqz);
        circuit2.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: p, port: 0 },
        );
        circuit2.connect(
            SignalSource::NodeOut { node: p, port: 0 },
            SignalSink::NodeIn { node: s, port: 0 },
        );
        circuit2.connect(
            SignalSource::NodeOut { node: s, port: 0 },
            SignalSink::BoundaryOut(0),
        );
        circuit2.connect(
            SignalSource::NodeOut { node: s, port: 1 },
            SignalSink::NodeIn {
                node: sub2,
                port: 0,
            },
        );
        circuit2.connect(
            SignalSource::BoundaryIn(1),
            SignalSink::NodeIn { node: a, port: 0 },
        );
        circuit2.connect(
            SignalSource::NodeOut { node: a, port: 0 },
            SignalSink::NodeIn {
                node: sub2,
                port: 1,
            },
        );
        circuit2.connect(
            SignalSource::NodeOut {
                node: sub2,
                port: 0,
            },
            SignalSink::NodeIn {
                node: eqz2,
                port: 0,
            },
        );

        let debts2 = well_mixed_description_debts(&circuit2);
        let mut live2 = LiveCircuit::new(circuit2, &debts2, config, SeedBank::new(2), 8).unwrap();
        live2
            .register_comparator(sub2, eqz2, 0, 0b01, 0b10)
            .unwrap();

        for t in 0..20u64 {
            // Delay(init=0) emits prior tick's input: supply truth(t) on ch0 so p emits 1-truth(t)
            // after t >= 1, and truth(t+1) on ch1 so alt emits truth(t). Comparator pays alt+sub+eqz ONLY.
            let truth = t % 2;
            let next_truth = (t + 1) % 2;
            live2.predict(&[truth, next_truth]).unwrap();
            let tick = live2.settle(&[truth]).unwrap();
            let comp_r = tick.comparator_rewards[0];
            if t >= 3 {
                assert!(
                    comp_r > 0.0,
                    "expected positive comp_r at tick {t}, got {comp_r}"
                );
                // Primary cone receives strictly 0.0 share of comparator reward (at most port_rewards[0] / 2.0).
                assert!((tick.economy.node_credit[a.index()] - comp_r / 3.0).abs() < 1e-12);
                assert!((tick.economy.node_credit[sub2.index()] - comp_r / 3.0).abs() < 1e-12);
                assert!((tick.economy.node_credit[eqz2.index()] - comp_r / 3.0).abs() < 1e-12);
                assert!(tick.economy.node_credit[p.index()] <= 0.0);
                assert!(tick.economy.node_credit[s.index()] <= 0.0);
                assert!(
                    (tick.economy.node_credit[p.index()] + tick.economy.node_credit[s.index()]
                        - tick.port_rewards[0])
                        .abs()
                        < 1e-12
                );
            }
        }
    }
}
