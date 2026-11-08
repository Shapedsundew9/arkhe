//! Transactional graph edits that preserve live register phases and node economy.

use std::collections::BTreeSet;
use std::fmt;

use crate::metrics::{allocate_circuit_description_bits, circuit_code_bits_with_tax};
use crate::structural::economy::{EconomyConfig, EconomyError, EconomyState};
use crate::structural::lattice::{Coord, GridShape, LatticeCircuit, LatticeError, WiringTopology};
use crate::substrate::{
    Circuit, CompiledCircuit, EvalError, NodeId, Primitive, ScalarState, SignalSink, SignalSource,
};

/// Errors returned by a state-preserving lattice edit.
#[derive(Debug)]
pub enum SurgeryError {
    /// The edited lattice could not be built or routed.
    Lattice(LatticeError),
    /// The edited circuit violated a substrate invariant.
    Substrate(crate::substrate::InvariantError),
    /// Node or register economy state could not be remapped.
    Economy(EconomyError),
    /// The new scalar state rejected its preserved register values.
    Evaluation(EvalError),
    /// A node map was not a partial one-to-one mapping into the edited circuit.
    InvalidNodeMapping,
}

impl fmt::Display for SurgeryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lattice(error) => write!(f, "lattice surgery failed: {error}"),
            Self::Substrate(error) => write!(f, "surgery produced an invalid circuit: {error}"),
            Self::Economy(error) => write!(f, "surgery economy remap failed: {error}"),
            Self::Evaluation(error) => write!(f, "surgery register remap failed: {error}"),
            Self::InvalidNodeMapping => write!(f, "invalid old-to-new node mapping"),
        }
    }
}

impl std::error::Error for SurgeryError {}

impl From<LatticeError> for SurgeryError {
    fn from(value: LatticeError) -> Self {
        Self::Lattice(value)
    }
}

impl From<crate::substrate::InvariantError> for SurgeryError {
    fn from(value: crate::substrate::InvariantError) -> Self {
        Self::Substrate(value)
    }
}

impl From<EconomyError> for SurgeryError {
    fn from(value: EconomyError) -> Self {
        Self::Economy(value)
    }
}

impl From<EvalError> for SurgeryError {
    fn from(value: EvalError) -> Self {
        Self::Evaluation(value)
    }
}

/// A live lattice together with the compiled circuit, synchronous phase, and metabolic state.
#[derive(Clone, Debug)]
pub struct EditableLattice {
    lattice: LatticeCircuit,
    circuit: Circuit,
    compiled: CompiledCircuit,
    state: ScalarState,
    economy: EconomyState,
    structure_epoch: u64,
}

impl EditableLattice {
    /// Builds an editable circuit with fresh scalar and metabolic state.
    pub fn new(
        lattice: LatticeCircuit,
        economy_config: EconomyConfig,
    ) -> Result<Self, SurgeryError> {
        let circuit = lattice.lower()?;
        let compiled = circuit.compile()?;
        let state = ScalarState::new(&compiled);
        let economy = EconomyState::new(
            economy_config,
            circuit.width(),
            circuit.node_count(),
            usize::from(circuit.num_outputs()),
            compiled.total_registers(),
        )?;
        Ok(Self {
            lattice,
            circuit,
            compiled,
            state,
            economy,
            structure_epoch: 0,
        })
    }

    /// Monotonic structural edit / explicit register-overwrite epoch counter.
    #[must_use]
    pub const fn structure_epoch(&self) -> u64 {
        self.structure_epoch
    }

    /// Current spatial graph.
    #[must_use]
    pub const fn lattice(&self) -> &LatticeCircuit {
        &self.lattice
    }

    /// Lowered substrate circuit.
    #[must_use]
    pub const fn circuit(&self) -> &Circuit {
        &self.circuit
    }

    /// Compiled execution plan for the current graph.
    #[must_use]
    pub const fn compiled(&self) -> &CompiledCircuit {
        &self.compiled
    }

    /// Live synchronous state.
    #[must_use]
    pub const fn state(&self) -> &ScalarState {
        &self.state
    }

    /// Mutable live synchronous state.
    pub fn state_mut(&mut self) -> &mut ScalarState {
        &mut self.state
    }

    /// Evaluates one synchronous step on the active circuit and state.
    ///
    /// # Errors
    ///
    /// Returns [`EvalError`] if input or output buffer dimensions mismatch.
    pub fn step(&mut self, inputs: &[u64], outputs: &mut [u64]) -> Result<(), EvalError> {
        self.state.step(&self.compiled, inputs, outputs)
    }

    /// Overwrites live registers with `regs`.
    ///
    /// # Errors
    ///
    /// Returns [`EvalError`] if register slice length mismatches.
    pub fn set_registers(&mut self, regs: &[u64]) -> Result<(), EvalError> {
        self.state.set_registers(&self.compiled, regs)?;
        self.structure_epoch = self.structure_epoch.wrapping_add(1);
        Ok(())
    }

    /// Current node reserves, null models, and register ancestry.
    #[must_use]
    pub const fn economy(&self) -> &EconomyState {
        &self.economy
    }

    /// Mutable node economy for online tick accounting.
    pub fn economy_mut(&mut self) -> &mut EconomyState {
        &mut self.economy
    }

    /// Applies a graph edit and installs the supplied exact old-to-new node mapping.
    ///
    /// The edit is transactional: any routing, compilation, or remapping failure leaves this
    /// `EditableLattice` unchanged. Nodes omitted from the map are treated as new.
    pub fn edit<F>(&mut self, edit: F) -> Result<Vec<Option<NodeId>>, SurgeryError>
    where
        F: FnOnce(&mut LatticeCircuit) -> Result<Vec<Option<NodeId>>, LatticeError>,
    {
        let mut staged = self.lattice.clone();
        let old_to_new = edit(&mut staged)?;
        self.recompile_preserving_state(staged, &old_to_new)?;
        Ok(old_to_new)
    }

    /// Recompiles `lattice`, preserving state for each explicitly mapped surviving node and edge.
    pub fn recompile_preserving_state(
        &mut self,
        lattice: LatticeCircuit,
        old_to_new_nodes: &[Option<NodeId>],
    ) -> Result<(), SurgeryError> {
        if old_to_new_nodes.len() != self.circuit.node_count() {
            return Err(SurgeryError::InvalidNodeMapping);
        }
        let circuit = lattice.lower()?;
        let mut seen = vec![false; circuit.node_count()];
        for &mapped in old_to_new_nodes.iter().flatten() {
            let Some(slot) = seen.get_mut(mapped.index()) else {
                return Err(SurgeryError::InvalidNodeMapping);
            };
            if *slot {
                return Err(SurgeryError::InvalidNodeMapping);
            }
            *slot = true;
        }
        let compiled = circuit.compile()?;
        let register_mapping = register_mapping(
            &self.circuit,
            &circuit,
            &self.compiled,
            &compiled,
            old_to_new_nodes,
        );
        let mut registers = ScalarState::new(&compiled).registers().to_vec();
        for (old, mapped) in register_mapping.iter().enumerate() {
            if let Some(new) = mapped {
                registers[*new] = self.state.registers()[old];
            }
        }
        let mut state = ScalarState::new(&compiled);
        state.set_registers(&compiled, &registers)?;
        let node_mapping: Vec<Option<usize>> = old_to_new_nodes
            .iter()
            .map(|mapped| mapped.map(NodeId::index))
            .collect();
        let economy = self.economy.remap_structure(
            &node_mapping,
            circuit.node_count(),
            &register_mapping,
            compiled.total_registers(),
        )?;

        self.lattice = lattice;
        self.circuit = circuit;
        self.compiled = compiled;
        self.state = state;
        self.economy = economy;
        self.structure_epoch = self.structure_epoch.wrapping_add(1);
        Ok(())
    }

    /// Routes a new edge, adding deterministic `Split` relays when required.
    pub fn route_connect(
        &mut self,
        src: SignalSource,
        dst: SignalSink,
        max_relay_hops: usize,
    ) -> Result<Vec<NodeId>, SurgeryError> {
        let mut relays = Vec::new();
        self.edit(|lattice| {
            let old_node_count = lattice.nodes().len();
            relays = lattice.route_connect(src, dst, max_relay_hops)?;
            Ok((0..old_node_count)
                .map(|index| {
                    Some(NodeId(
                        u32::try_from(index).expect("node count fits in u32"),
                    ))
                })
                .collect())
        })?;
        Ok(relays)
    }

    /// Excises mature depleted nodes, collapses single-use `Split` nodes, and bypasses inactive
    /// zero-valued correctors. Returns the old-to-new mapping for the pre-prune nodes.
    pub fn prune_starved_nodes(&mut self) -> Result<Vec<Option<NodeId>>, SurgeryError> {
        let removed: BTreeSet<NodeId> = self
            .economy
            .nodes()
            .iter()
            .enumerate()
            .filter(|(_, node)| node.juvenile_ticks == 0 && node.energy <= 0.0)
            .map(|(index, _)| NodeId(u32::try_from(index).expect("node count fits in u32")))
            .collect();
        if removed.is_empty() {
            return Ok((0..self.circuit.node_count())
                .map(|index| {
                    Some(NodeId(
                        u32::try_from(index).expect("node count fits in u32"),
                    ))
                })
                .collect());
        }
        let (staged, old_to_new) = self.build_pruned_lattice(&removed)?;
        self.recompile_preserving_state(staged, &old_to_new)?;
        Ok(old_to_new)
    }

    /// Excises causally idle mature nodes (zero counterfactual sensitivity over `W_diag`),
    /// validating that outputs on `past_inputs` match `past_predictions` and code length does not increase (D1b.1).
    pub fn prune_zero_sensitivity_mature_nodes(
        &mut self,
        idle_nodes: &BTreeSet<NodeId>,
        past_inputs: &[Vec<u64>],
        past_predictions: &[Vec<u64>],
        split_code_tax: bool,
    ) -> Result<Vec<Option<NodeId>>, SurgeryError> {
        if idle_nodes.is_empty() {
            return Ok((0..self.circuit.node_count())
                .map(|index| {
                    Some(NodeId(
                        u32::try_from(index).expect("node count fits in u32"),
                    ))
                })
                .collect());
        }

        let Ok((staged, old_to_new)) = self.build_pruned_lattice(idle_nodes) else {
            return Ok((0..self.circuit.node_count())
                .map(|index| {
                    Some(NodeId(
                        u32::try_from(index).expect("node count fits in u32"),
                    ))
                })
                .collect());
        };

        let Ok(staged_circuit) = staged.lower() else {
            return Ok((0..self.circuit.node_count())
                .map(|index| {
                    Some(NodeId(
                        u32::try_from(index).expect("node count fits in u32"),
                    ))
                })
                .collect());
        };

        let Ok(staged_compiled) = staged_circuit.compile() else {
            return Ok((0..self.circuit.node_count())
                .map(|index| {
                    Some(NodeId(
                        u32::try_from(index).expect("node count fits in u32"),
                    ))
                })
                .collect());
        };

        let reg_map = register_mapping(
            &self.circuit,
            &staged_circuit,
            &self.compiled,
            &staged_compiled,
            &old_to_new,
        );
        let mut staged_state = ScalarState::new(&staged_compiled);
        let mut staged_regs = staged_state.registers().to_vec();
        for (old_r, new_r) in reg_map.iter().enumerate() {
            if let Some(nr) = new_r
                && old_r < self.state.registers().len()
            {
                staged_regs[*nr] = self.state.registers()[old_r];
            }
        }
        if staged_state
            .set_registers(&staged_compiled, &staged_regs)
            .is_err()
        {
            return Ok((0..self.circuit.node_count())
                .map(|index| {
                    Some(NodeId(
                        u32::try_from(index).expect("node count fits in u32"),
                    ))
                })
                .collect());
        }

        let mut staged_pred = vec![0u64; usize::from(staged_circuit.num_outputs())];
        for (frame_idx, input_frame) in past_inputs.iter().enumerate() {
            if staged_state
                .step(&staged_compiled, input_frame, &mut staged_pred)
                .is_err()
            {
                return Ok((0..self.circuit.node_count())
                    .map(|index| {
                        Some(NodeId(
                            u32::try_from(index).expect("node count fits in u32"),
                        ))
                    })
                    .collect());
            }
            if frame_idx < past_predictions.len() && staged_pred != past_predictions[frame_idx] {
                return Ok((0..self.circuit.node_count())
                    .map(|index| {
                        Some(NodeId(
                            u32::try_from(index).expect("node count fits in u32"),
                        ))
                    })
                    .collect());
            }
        }

        let shape = if self.lattice.topology() == WiringTopology::WellMixed {
            None
        } else {
            Some(self.lattice.shape())
        };
        let l_old = circuit_code_bits_with_tax(
            &self.circuit,
            self.lattice.topology(),
            shape,
            0,
            split_code_tax,
        );
        let l_new = circuit_code_bits_with_tax(
            &staged_circuit,
            self.lattice.topology(),
            shape,
            0,
            split_code_tax,
        );
        if l_new > l_old {
            return Ok((0..self.circuit.node_count())
                .map(|index| {
                    Some(NodeId(
                        u32::try_from(index).expect("node count fits in u32"),
                    ))
                })
                .collect());
        }

        self.recompile_preserving_state(staged, &old_to_new)?;
        Ok(old_to_new)
    }

    fn build_pruned_lattice(
        &self,
        initially_removed: &BTreeSet<NodeId>,
    ) -> Result<(LatticeCircuit, Vec<Option<NodeId>>), SurgeryError> {
        let mut removed = initially_removed.clone();
        let mut logical_edges = self.lattice.edges().to_vec();
        let mut bypasses = Vec::new();

        let mut inactive_zero_nodes: BTreeSet<NodeId> = initially_removed
            .iter()
            .copied()
            .filter(|&node| {
                matches!(
                    self.lattice.node_primitive(node),
                    Some(Primitive::Delay { init: 0 })
                ) && !self.economy.nodes()[node.index()].activity.ever_nonzero()
            })
            .collect();

        loop {
            let mut propagated = false;
            for (index, &(_, primitive)) in self.lattice.nodes().iter().enumerate() {
                let node = NodeId(u32::try_from(index).expect("node count fits in u32"));
                if primitive == Primitive::Split
                    && input_source(&logical_edges, node, 0)
                        .and_then(source_node)
                        .is_some_and(|source| inactive_zero_nodes.contains(&source))
                {
                    propagated |= inactive_zero_nodes.insert(node);
                }
            }
            if !propagated {
                break;
            }
        }

        for zero_node in inactive_zero_nodes.iter().copied() {
            let consumers: Vec<(NodeId, u8)> = logical_edges
                .iter()
                .filter_map(|(source, sink)| {
                    if let (
                        SignalSource::NodeOut { node, .. },
                        SignalSink::NodeIn {
                            node: consumer,
                            port,
                        },
                    ) = (*source, *sink)
                        && node == zero_node
                    {
                        Some((consumer, port))
                    } else {
                        None
                    }
                })
                .collect();
            for (consumer, port) in consumers {
                if removed.contains(&consumer) {
                    continue;
                }
                let Some(primitive) = self.lattice.node_primitive(consumer) else {
                    continue;
                };
                let other_port = match (primitive, port) {
                    (Primitive::Add, 0) => Some(1),
                    (Primitive::Add, 1) | (Primitive::Sub, 1) => Some(0),
                    _ => None,
                };
                let Some(other_port) = other_port else {
                    continue;
                };
                let Some(other_source) = input_source(&logical_edges, consumer, other_port) else {
                    continue;
                };
                if source_node(other_source).is_some_and(|node| {
                    removed.contains(&node) || inactive_zero_nodes.contains(&node)
                }) {
                    continue;
                }
                let outputs = output_sinks(&logical_edges, consumer);
                if outputs.len() != 1 {
                    continue;
                }
                let output = outputs[0];

                if matches!(
                    (other_source, output),
                    (SignalSource::BoundaryIn(_), SignalSink::BoundaryOut(_))
                ) {
                    if initially_removed.contains(&consumer) {
                        removed.insert(consumer);
                        logical_edges
                            .retain(|(source, sink)| !edge_touches_nodes(*source, *sink, &removed));
                    } else {
                        continue;
                    }
                } else {
                    removed.insert(consumer);
                    logical_edges
                        .retain(|(source, sink)| !edge_touches_nodes(*source, *sink, &removed));
                    logical_edges.push((other_source, output));
                    bypasses.push((other_source, output));
                }
            }
        }

        loop {
            logical_edges.retain(|(source, sink)| !edge_touches_nodes(*source, *sink, &removed));
            let mut changed = false;
            for (index, &(_, primitive)) in self.lattice.nodes().iter().enumerate() {
                let node = NodeId(u32::try_from(index).expect("node count fits in u32"));
                if removed.contains(&node) {
                    continue;
                }
                if (0..primitive.input_arity())
                    .any(|port| input_source(&logical_edges, node, port).is_none())
                {
                    removed.insert(node);
                    changed = true;
                }
            }
            if changed {
                continue;
            }
            for (index, &(_, primitive)) in self.lattice.nodes().iter().enumerate() {
                let node = NodeId(u32::try_from(index).expect("node count fits in u32"));
                if removed.contains(&node) || primitive != Primitive::Split {
                    continue;
                }
                let outputs = output_sinks(&logical_edges, node);
                if outputs.len() > 1 {
                    continue;
                }
                if let Some(output) = outputs.first().copied()
                    && let Some(input) = input_source(&logical_edges, node, 0)
                    && !source_node(input).is_some_and(|source| removed.contains(&source))
                {
                    if matches!(
                        (input, output),
                        (SignalSource::BoundaryIn(_), SignalSink::BoundaryOut(_))
                    ) {
                        if initially_removed.contains(&node) {
                            logical_edges.retain(|(source, sink)| {
                                !matches!(source, SignalSource::NodeOut { node: current, .. } if *current == node)
                                    && !matches!(sink, SignalSink::NodeIn { node: current, .. } if *current == node)
                            });
                            removed.insert(node);
                            changed = true;
                            break;
                        } else {
                            continue;
                        }
                    }
                    logical_edges.retain(|(source, sink)| {
                        !matches!(source, SignalSource::NodeOut { node: current, .. } if *current == node)
                            && !matches!(sink, SignalSink::NodeIn { node: current, .. } if *current == node)
                    });
                    logical_edges.push((input, output));
                    bypasses.push((input, output));
                }
                removed.insert(node);
                changed = true;
                break;
            }
            if !changed {
                break;
            }
        }

        let mut staged = self.lattice.clone();
        let old_to_new = staged.remove_nodes(&removed);
        for (source, sink) in bypasses {
            let Some(source) = remap_source(source, &old_to_new) else {
                continue;
            };
            let Some(sink) = remap_sink(sink, &old_to_new) else {
                continue;
            };
            staged.route_connect(source, sink, 4)?;
        }

        let mut direct_wires = Vec::new();
        for &(src, dst) in staged.edges() {
            if matches!(
                (src, dst),
                (SignalSource::BoundaryIn(_), SignalSink::BoundaryOut(_))
            ) {
                direct_wires.push((src, dst));
            }
        }
        for (src, dst) in direct_wires {
            staged.disconnect(src, dst);
            if !staged.nodes().is_empty() {
                let cell = staged
                    .nearest_empty_cell(Coord { row: 0, col: 0 })
                    .ok_or(LatticeError::UnknownEndpoint)?;
                let split = staged.place_node(cell, Primitive::Split)?;
                staged.route_connect(
                    src,
                    SignalSink::NodeIn {
                        node: split,
                        port: 0,
                    },
                    4,
                )?;
                staged.route_connect(
                    SignalSource::NodeOut {
                        node: split,
                        port: 0,
                    },
                    dst,
                    4,
                )?;
            }
        }

        Ok((staged, old_to_new))
    }
}

/// Tracks marginal description debt allocated to newly sprouted nodes across structural edits (D1b.3).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MarginalDebtLedger {
    /// Owed maturity debt in bits for each node index.
    pub node_debts: Vec<f64>,
    /// Whether each node index has already paid its lump-sum maturity debt.
    pub debt_paid: Vec<bool>,
    /// Last committed total circuit code length $L_{\text{eff}}(C)$.
    pub last_committed_code_bits: f64,
}

impl MarginalDebtLedger {
    /// Creates a fresh [`MarginalDebtLedger`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a sprout transaction $C_{\text{old}} \to C_{\text{new}}$ and assigns marginal debt to new nodes.
    pub fn on_sprout(
        &mut self,
        new_circuit: &Circuit,
        topology: WiringTopology,
        grid_shape: Option<GridShape>,
        split_code_tax: bool,
        old_to_new: &[Option<NodeId>],
    ) {
        let l_old = self.last_committed_code_bits;
        let l_new =
            circuit_code_bits_with_tax(new_circuit, topology, grid_shape, 0, split_code_tax);
        let delta_l_sprout = (l_new - l_old).max(0.0);
        let n_new = new_circuit.node_count();

        let mut next_debts = vec![0.0; n_new];
        let mut next_paid = vec![false; n_new];

        let mut mapped_survivors = vec![false; n_new];
        for (old_idx, &new_node) in old_to_new.iter().enumerate() {
            if let Some(new_id) = new_node {
                let idx = new_id.index();
                if idx < n_new {
                    mapped_survivors[idx] = true;
                    if old_idx < self.node_debts.len() {
                        next_debts[idx] = self.node_debts[old_idx];
                        next_paid[idx] = self.debt_paid[old_idx];
                    }
                }
            }
        }

        let mut new_node_indices = Vec::new();
        for (idx, &is_survivor) in mapped_survivors.iter().enumerate().take(n_new) {
            if !is_survivor {
                new_node_indices.push(idx);
            }
        }

        if !new_node_indices.is_empty() {
            let alloc = allocate_circuit_description_bits(
                new_circuit,
                topology,
                grid_shape,
                0,
                split_code_tax,
            );
            let d_new: f64 = new_node_indices
                .iter()
                .map(|&i| alloc.node_debts.get(i).copied().unwrap_or(0.0))
                .sum();
            for &idx in &new_node_indices {
                let raw_d = alloc.node_debts.get(idx).copied().unwrap_or(0.0);
                let debt = if d_new > 0.0 {
                    delta_l_sprout * (raw_d / d_new)
                } else {
                    delta_l_sprout / (new_node_indices.len() as f64)
                };
                next_debts[idx] = debt;
                next_paid[idx] = false;
            }
        }

        self.node_debts = next_debts;
        self.debt_paid = next_paid;
        self.last_committed_code_bits = l_new;
    }

    /// Remaps survivor debts and paid status across a pruning transaction.
    pub fn on_prune(
        &mut self,
        new_circuit: &Circuit,
        topology: WiringTopology,
        grid_shape: Option<GridShape>,
        split_code_tax: bool,
        old_to_new: &[Option<NodeId>],
    ) {
        let n_new = new_circuit.node_count();
        let mut next_debts = vec![0.0; n_new];
        let mut next_paid = vec![false; n_new];

        for (old_idx, &new_node) in old_to_new.iter().enumerate() {
            if let Some(new_id) = new_node {
                let idx = new_id.index();
                if idx < n_new && old_idx < self.node_debts.len() {
                    next_debts[idx] = self.node_debts[old_idx];
                    next_paid[idx] = self.debt_paid[old_idx];
                }
            }
        }

        self.node_debts = next_debts;
        self.debt_paid = next_paid;
        self.last_committed_code_bits =
            circuit_code_bits_with_tax(new_circuit, topology, grid_shape, 0, split_code_tax);
    }

    /// Ensures ledger vectors match circuit node count, allocating debts for new nodes.
    pub fn ensure_capacity(
        &mut self,
        circuit: &Circuit,
        topology: WiringTopology,
        grid_shape: Option<GridShape>,
        split_code_tax: bool,
    ) {
        let n = circuit.node_count();
        if self.node_debts.len() < n {
            let alloc =
                allocate_circuit_description_bits(circuit, topology, grid_shape, 0, split_code_tax);
            while self.node_debts.len() < n {
                let idx = self.node_debts.len();
                let debt = alloc.node_debts.get(idx).copied().unwrap_or(0.0);
                self.node_debts.push(debt);
                self.debt_paid.push(false);
            }
            self.last_committed_code_bits =
                circuit_code_bits_with_tax(circuit, topology, grid_shape, 0, split_code_tax);
        } else if self.node_debts.len() > n {
            self.node_debts.truncate(n);
            self.debt_paid.truncate(n);
        }
    }
}

fn input_source(
    edges: &[(SignalSource, SignalSink)],
    node: NodeId,
    port: u8,
) -> Option<SignalSource> {
    edges
        .iter()
        .find_map(|&(source, sink)| (sink == SignalSink::NodeIn { node, port }).then_some(source))
}

fn output_sinks(edges: &[(SignalSource, SignalSink)], node: NodeId) -> Vec<SignalSink> {
    edges
        .iter()
        .filter_map(|&(source, sink)| {
            matches!(source, SignalSource::NodeOut { node: source_node, .. } if source_node == node)
                .then_some(sink)
        })
        .collect()
}

fn source_node(source: SignalSource) -> Option<NodeId> {
    match source {
        SignalSource::BoundaryIn(_) => None,
        SignalSource::NodeOut { node, .. } => Some(node),
    }
}

fn edge_touches_nodes(source: SignalSource, sink: SignalSink, removed: &BTreeSet<NodeId>) -> bool {
    source_node(source).is_some_and(|node| removed.contains(&node))
        || matches!(sink, SignalSink::NodeIn { node, .. } if removed.contains(&node))
}

fn remap_source(source: SignalSource, mapping: &[Option<NodeId>]) -> Option<SignalSource> {
    match source {
        SignalSource::BoundaryIn(_) => Some(source),
        SignalSource::NodeOut { node, port } => Some(SignalSource::NodeOut {
            node: mapping.get(node.index()).copied().flatten()?,
            port,
        }),
    }
}

fn remap_sink(sink: SignalSink, mapping: &[Option<NodeId>]) -> Option<SignalSink> {
    match sink {
        SignalSink::BoundaryOut(_) => Some(sink),
        SignalSink::NodeIn { node, port } => Some(SignalSink::NodeIn {
            node: mapping.get(node.index()).copied().flatten()?,
            port,
        }),
    }
}

pub(crate) fn explicit_delay_register_index(circuit: &Circuit, target: NodeId) -> Option<usize> {
    let mut reg_idx = 0usize;
    for (idx, primitive) in circuit.nodes().iter().enumerate() {
        if primitive.is_stateful() {
            if idx == target.index() {
                return Some(reg_idx);
            }
            reg_idx += 1;
        }
    }
    None
}

pub(crate) fn port_upstream_cone_and_edges(
    lattice: &LatticeCircuit,
    out_port: u16,
) -> (BTreeSet<NodeId>, BTreeSet<(SignalSource, SignalSink)>) {
    let mut cone_nodes = BTreeSet::new();
    let mut cone_edges = BTreeSet::new();
    let mut stack = Vec::new();
    let target = SignalSink::BoundaryOut(out_port);
    for &(src, dst) in lattice.edges() {
        if dst == target {
            cone_edges.insert((src, dst));
            if let SignalSource::NodeOut { node, .. } = src
                && cone_nodes.insert(node)
            {
                stack.push(node);
            }
        }
    }
    while let Some(node) = stack.pop() {
        for &(src, dst) in lattice.edges() {
            if let SignalSink::NodeIn { node: dst_node, .. } = dst
                && dst_node == node
            {
                cone_edges.insert((src, dst));
                if let SignalSource::NodeOut { node: src_node, .. } = src
                    && cone_nodes.insert(src_node)
                {
                    stack.push(src_node);
                }
            }
        }
    }
    (cone_nodes, cone_edges)
}

pub(crate) fn port_uses_boundary_in(lattice: &LatticeCircuit, out_port: u16) -> bool {
    let (_, cone_edges) = port_upstream_cone_and_edges(lattice, out_port);
    cone_edges
        .iter()
        .any(|&(src, _)| matches!(src, SignalSource::BoundaryIn(_)))
}

pub(crate) fn port_cone_changed(
    old_lattice: &LatticeCircuit,
    new_lattice: &LatticeCircuit,
    old_to_new: &[Option<NodeId>],
    out_port: u16,
) -> bool {
    let (old_nodes, old_edges) = port_upstream_cone_and_edges(old_lattice, out_port);
    let (new_nodes, new_edges) = port_upstream_cone_and_edges(new_lattice, out_port);
    if old_nodes.len() != new_nodes.len() || old_edges.len() != new_edges.len() {
        return true;
    }
    for &old_node in &old_nodes {
        let Some(new_node) = old_to_new.get(old_node.index()).copied().flatten() else {
            return true;
        };
        if !new_nodes.contains(&new_node)
            || old_lattice.node_primitive(old_node) != new_lattice.node_primitive(new_node)
        {
            return true;
        }
    }
    for &(src, dst) in &old_edges {
        let Some(mapped_src) = remap_source(src, old_to_new) else {
            return true;
        };
        let Some(mapped_dst) = remap_sink(dst, old_to_new) else {
            return true;
        };
        if !new_edges.contains(&(mapped_src, mapped_dst)) {
            return true;
        }
    }
    false
}

pub(crate) fn register_mapping(
    old: &Circuit,
    new: &Circuit,
    old_compiled: &CompiledCircuit,
    new_compiled: &CompiledCircuit,
    old_to_new_nodes: &[Option<NodeId>],
) -> Vec<Option<usize>> {
    let mut mapping = vec![None; old_compiled.total_registers()];
    let mut old_delay_register = 0usize;
    let mut new_delay_registers = vec![None; new.node_count()];
    let mut next_new_delay_register = 0usize;
    for (index, primitive) in new.nodes().iter().enumerate() {
        if primitive.is_stateful() {
            new_delay_registers[index] = Some(next_new_delay_register);
            next_new_delay_register += 1;
        }
    }
    for (old_index, primitive) in old.nodes().iter().enumerate() {
        if !primitive.is_stateful() {
            continue;
        }
        if let Some(new_node) = old_to_new_nodes[old_index]
            && new
                .nodes()
                .get(new_node.index())
                .is_some_and(|primitive| primitive.is_stateful())
        {
            mapping[old_delay_register] = new_delay_registers[new_node.index()];
        }
        old_delay_register += 1;
    }

    if old.wire_delay() != crate::substrate::WireDelayMode::TicksPerHop {
        return mapping;
    }
    let old_bases = edge_register_bases(old, old_compiled.explicit_delay_count());
    let new_bases = edge_register_bases(new, new_compiled.explicit_delay_count());
    for (old_index, edge) in old.edges().iter().enumerate() {
        let Some(source) = remap_source(edge.src, old_to_new_nodes) else {
            continue;
        };
        let Some(sink) = remap_sink(edge.dst, old_to_new_nodes) else {
            continue;
        };
        let Some(new_index) = new.edges().iter().position(|candidate| {
            candidate.src == source && candidate.dst == sink && candidate.hops == edge.hops
        }) else {
            continue;
        };
        let old_base = old_bases[old_index];
        let new_base = new_bases[new_index];
        for stage in 0..usize::from(edge.hops) {
            mapping[old_base + stage] = Some(new_base + stage);
        }
    }
    mapping
}

fn edge_register_bases(circuit: &Circuit, first_register: usize) -> Vec<usize> {
    let mut next = first_register;
    circuit
        .edges()
        .iter()
        .map(|edge| {
            let base = next;
            if circuit.wire_delay() == crate::substrate::WireDelayMode::TicksPerHop {
                next += usize::from(edge.hops);
            }
            base
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structural::lattice::{Coord, GridShape, WiringTopology};
    use crate::substrate::{Width, WireDelayMode};

    fn oscillator_lattice() -> (LatticeCircuit, [NodeId; 5]) {
        let mut lattice = LatticeCircuit::new(
            GridShape::new(1, 5).unwrap(),
            Width::BITS_2,
            WireDelayMode::TicksPerHop,
            WiringTopology::WellMixed,
            vec![],
            vec![
                crate::structural::lattice::BoundaryAnchor {
                    side: crate::structural::lattice::PerimeterSide::South,
                    offset: 0,
                },
                crate::structural::lattice::BoundaryAnchor {
                    side: crate::structural::lattice::PerimeterSide::South,
                    offset: 4,
                },
            ],
        )
        .unwrap();
        let delay0 = lattice
            .place_node(Coord { row: 0, col: 0 }, Primitive::delay_zero())
            .unwrap();
        let split0 = lattice
            .place_node(Coord { row: 0, col: 1 }, Primitive::Split)
            .unwrap();
        let removable = lattice
            .place_node(Coord { row: 0, col: 2 }, Primitive::Eqz)
            .unwrap();
        let delay1 = lattice
            .place_node(Coord { row: 0, col: 3 }, Primitive::delay_zero())
            .unwrap();
        let split1 = lattice
            .place_node(Coord { row: 0, col: 4 }, Primitive::Split)
            .unwrap();
        for (source, sink) in [
            (
                SignalSource::NodeOut {
                    node: delay0,
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: split0,
                    port: 0,
                },
            ),
            (
                SignalSource::NodeOut {
                    node: split0,
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: delay0,
                    port: 0,
                },
            ),
            (
                SignalSource::NodeOut {
                    node: split0,
                    port: 1,
                },
                SignalSink::BoundaryOut(0),
            ),
            (
                SignalSource::NodeOut {
                    node: delay1,
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: removable,
                    port: 0,
                },
            ),
            (
                SignalSource::NodeOut {
                    node: removable,
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: split1,
                    port: 0,
                },
            ),
            (
                SignalSource::NodeOut {
                    node: split1,
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: delay1,
                    port: 0,
                },
            ),
            (
                SignalSource::NodeOut {
                    node: split1,
                    port: 1,
                },
                SignalSink::BoundaryOut(1),
            ),
        ] {
            lattice.connect(source, sink).unwrap();
        }
        (lattice, [delay0, split0, removable, delay1, split1])
    }

    #[test]
    fn recompilation_preserves_delay_phase_and_unchanged_hop_registers() {
        let (lattice, nodes) = oscillator_lattice();
        let mut editable = EditableLattice::new(lattice, EconomyConfig::default()).unwrap();
        let compiled = editable.compiled().clone();
        editable
            .state_mut()
            .set_registers(&compiled, &[1, 2, 3, 0, 1, 2, 3, 0, 1])
            .unwrap();

        let mapping = editable
            .edit(|lattice| {
                let mapping = lattice.remove_nodes(&BTreeSet::from([nodes[2]]));
                let source = mapping[nodes[3].index()].unwrap();
                let sink = mapping[nodes[4].index()].unwrap();
                lattice.route_connect(
                    SignalSource::NodeOut {
                        node: source,
                        port: 0,
                    },
                    SignalSink::NodeIn {
                        node: sink,
                        port: 0,
                    },
                    4,
                )?;
                Ok(mapping)
            })
            .unwrap();

        assert_eq!(mapping[nodes[2].index()], None);
        assert_eq!(editable.state().registers(), &[1, 2, 3, 0, 1, 0, 1, 0]);
        assert!(editable.lattice().lower().is_ok());
    }

    #[test]
    fn pruning_zero_corrector_bypasses_add_and_preserves_surviving_state() {
        let mut lattice = LatticeCircuit::new(
            GridShape::new(1, 5).unwrap(),
            Width::BITS_1,
            WireDelayMode::Combinational,
            WiringTopology::WellMixed,
            vec![],
            vec![crate::structural::lattice::BoundaryAnchor {
                side: crate::structural::lattice::PerimeterSide::South,
                offset: 2,
            }],
        )
        .unwrap();
        let base_delay = lattice
            .place_node(Coord { row: 0, col: 0 }, Primitive::delay_zero())
            .unwrap();
        let base_split = lattice
            .place_node(Coord { row: 0, col: 1 }, Primitive::Split)
            .unwrap();
        let zero_delay = lattice
            .place_node(Coord { row: 0, col: 2 }, Primitive::delay_zero())
            .unwrap();
        let zero_split = lattice
            .place_node(Coord { row: 0, col: 3 }, Primitive::Split)
            .unwrap();
        let corrector_add = lattice
            .place_node(Coord { row: 0, col: 4 }, Primitive::Add)
            .unwrap();
        for (source, sink) in [
            (
                SignalSource::NodeOut {
                    node: base_delay,
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: base_split,
                    port: 0,
                },
            ),
            (
                SignalSource::NodeOut {
                    node: base_split,
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: base_delay,
                    port: 0,
                },
            ),
            (
                SignalSource::NodeOut {
                    node: zero_delay,
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: zero_split,
                    port: 0,
                },
            ),
            (
                SignalSource::NodeOut {
                    node: zero_split,
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: zero_delay,
                    port: 0,
                },
            ),
            (
                SignalSource::NodeOut {
                    node: base_split,
                    port: 1,
                },
                SignalSink::NodeIn {
                    node: corrector_add,
                    port: 0,
                },
            ),
            (
                SignalSource::NodeOut {
                    node: zero_split,
                    port: 1,
                },
                SignalSink::NodeIn {
                    node: corrector_add,
                    port: 1,
                },
            ),
            (
                SignalSource::NodeOut {
                    node: corrector_add,
                    port: 0,
                },
                SignalSink::BoundaryOut(0),
            ),
        ] {
            lattice.connect(source, sink).unwrap();
        }
        let economy_config = EconomyConfig {
            initial_energy: 0.1,
            juvenile_ticks: 0,
            ..EconomyConfig::default()
        };
        let mut editable = EditableLattice::new(lattice, economy_config).unwrap();
        let compiled = editable.compiled().clone();
        editable
            .state_mut()
            .set_registers(&compiled, &[1, 0])
            .unwrap();
        editable
            .economy_mut()
            .apply_tick(&[0.0], &[vec![0.0; 5]], &[0.0, 0.0, 0.2, 0.0, 0.0])
            .unwrap();

        let mapping = editable.prune_starved_nodes().unwrap();

        assert_eq!(mapping[zero_delay.index()], None);
        assert_eq!(mapping[zero_split.index()], None);
        assert_eq!(mapping[corrector_add.index()], None);
        assert_eq!(editable.state().registers(), &[1]);
        let mut output = [0];
        let compiled = editable.compiled().clone();
        editable
            .state_mut()
            .step(&compiled, &[], &mut output)
            .unwrap();
        assert_eq!(output, [1], "edges: {:?}", editable.lattice().edges());
    }

    #[test]
    fn local_von_neumann_border_stacking_eqz_and_addself() {
        use crate::structural::lattice::{BoundaryAnchor, PerimeterSide};
        let shape = GridShape::new(8, 8).unwrap();
        let mut lattice = LatticeCircuit::new(
            shape,
            Width::BITS_2,
            WireDelayMode::Combinational,
            WiringTopology::LocalVonNeumann,
            vec![BoundaryAnchor {
                side: PerimeterSide::North,
                offset: 3,
            }],
            vec![BoundaryAnchor {
                side: PerimeterSide::North,
                offset: 4,
            }],
        )
        .unwrap();

        // 1. First neuron: Eqz(x) at (0, 3) connected to BoundaryOut(0) at (0, 4)
        let eqz = lattice
            .place_node(Coord { row: 0, col: 3 }, Primitive::Eqz)
            .unwrap();
        lattice
            .connect(
                SignalSource::BoundaryIn(0),
                SignalSink::NodeIn { node: eqz, port: 0 },
            )
            .unwrap();
        lattice
            .stack_output_adder(0, SignalSource::NodeOut { node: eqz, port: 0 }, 4)
            .unwrap();

        let lower1 = lattice.lower().unwrap();
        assert!(lower1.compile().is_ok());

        // 2. Stack corrector: AddSelf(x) = x + x (mod 4)
        let tapped = lattice.tap_source(SignalSource::BoundaryIn(0), 8).unwrap();
        let split_self = lattice
            .place_node(Coord { row: 0, col: 2 }, Primitive::Split)
            .unwrap();
        let add_self = lattice
            .place_node(Coord { row: 1, col: 2 }, Primitive::Add)
            .unwrap();
        lattice
            .route_connect(
                tapped,
                SignalSink::NodeIn {
                    node: split_self,
                    port: 0,
                },
                8,
            )
            .unwrap();
        lattice
            .route_connect(
                SignalSource::NodeOut {
                    node: split_self,
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: add_self,
                    port: 0,
                },
                8,
            )
            .unwrap();
        let tapped_self = lattice
            .tap_source(
                SignalSource::NodeOut {
                    node: split_self,
                    port: 1,
                },
                8,
            )
            .unwrap();
        lattice
            .route_connect(
                tapped_self,
                SignalSink::NodeIn {
                    node: add_self,
                    port: 1,
                },
                8,
            )
            .unwrap();

        lattice
            .stack_output_adder(
                0,
                SignalSource::NodeOut {
                    node: add_self,
                    port: 0,
                },
                8,
            )
            .unwrap();

        let circuit = lattice.lower().unwrap();
        let compiled = circuit.compile().unwrap();
        let mut state = ScalarState::new(&compiled);
        let mut out = [0u64];

        // Verify truth table: EQZ(x) + 2x (mod 4)
        // x = 0: 1 + 0 = 1
        // x = 1: 0 + 2 = 2
        // x = 2: 0 + 4 = 0
        // x = 3: 0 + 6 = 2
        let expected = [1, 2, 0, 2];
        for (x, &exp) in expected.iter().enumerate() {
            state.step(&compiled, &[x as u64], &mut out).unwrap();
            assert_eq!(out[0], exp, "x={x}: expected {exp}, got {}", out[0]);
        }
    }

    #[test]
    fn marginal_debt_ledger_telescoping_exact_conservation() {
        let mut ledger = MarginalDebtLedger::new();
        let mut circuit = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);

        // Step 1: Add node 0
        let n0 = circuit.add_node(Primitive::Eqz);
        circuit.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: n0, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: n0, port: 0 },
            SignalSink::BoundaryOut(0),
        );
        ledger.on_sprout(&circuit, WiringTopology::WellMixed, None, true, &[None]);
        let l1 = circuit_code_bits_with_tax(&circuit, WiringTopology::WellMixed, None, 0, true);
        assert!((ledger.node_debts.iter().sum::<f64>() - l1).abs() < 1e-12);

        // Step 2: Add node 1 (Delay)
        let _ = circuit.add_node(Primitive::Delay { init: 0 });
        ledger.on_sprout(
            &circuit,
            WiringTopology::WellMixed,
            None,
            true,
            &[Some(NodeId(0)), None],
        );
        let l2 = circuit_code_bits_with_tax(&circuit, WiringTopology::WellMixed, None, 0, true);
        assert!((ledger.node_debts.iter().sum::<f64>() - l2).abs() < 1e-12);
    }

    #[test]
    fn prune_zero_sensitivity_mature_nodes_excises_idle_const_loop() {
        use crate::structural::lattice::{BoundaryAnchor, PerimeterSide};
        let shape = GridShape::new(4, 4).unwrap();
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
                offset: 0,
            }],
        )
        .unwrap();

        let eqz = lattice
            .place_node(Coord { row: 0, col: 0 }, Primitive::Eqz)
            .unwrap();
        let zero_delay = lattice
            .place_node(Coord { row: 0, col: 1 }, Primitive::Delay { init: 0 })
            .unwrap();
        let zero_split = lattice
            .place_node(Coord { row: 0, col: 2 }, Primitive::Split)
            .unwrap();
        let add = lattice
            .place_node(Coord { row: 0, col: 3 }, Primitive::Add)
            .unwrap();

        lattice
            .connect(
                SignalSource::BoundaryIn(0),
                SignalSink::NodeIn { node: eqz, port: 0 },
            )
            .unwrap();
        lattice
            .connect(
                SignalSource::NodeOut { node: eqz, port: 0 },
                SignalSink::NodeIn { node: add, port: 0 },
            )
            .unwrap();
        lattice
            .connect(
                SignalSource::NodeOut {
                    node: zero_delay,
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: zero_split,
                    port: 0,
                },
            )
            .unwrap();
        lattice
            .connect(
                SignalSource::NodeOut {
                    node: zero_split,
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: zero_delay,
                    port: 0,
                },
            )
            .unwrap();
        lattice
            .connect(
                SignalSource::NodeOut {
                    node: zero_split,
                    port: 1,
                },
                SignalSink::NodeIn { node: add, port: 1 },
            )
            .unwrap();
        lattice
            .connect(
                SignalSource::NodeOut { node: add, port: 0 },
                SignalSink::BoundaryOut(0),
            )
            .unwrap();

        let economy_config = EconomyConfig {
            initial_energy: 10.0,
            juvenile_ticks: 10,
            ..EconomyConfig::default()
        };
        let mut editable = EditableLattice::new(lattice, economy_config).unwrap();

        let mut past_inputs = Vec::new();
        let mut past_preds = Vec::new();
        for t in 0..64 {
            let x = (t % 4) as u64;
            let mut out = [0u64];
            let compiled = editable.compiled().clone();
            editable
                .state_mut()
                .step(&compiled, &[x], &mut out)
                .unwrap();
            past_inputs.push(vec![x]);
            past_preds.push(out.to_vec());
        }

        let mut idle_nodes = BTreeSet::new();
        idle_nodes.insert(zero_delay);
        idle_nodes.insert(zero_split);

        let mapping = editable
            .prune_zero_sensitivity_mature_nodes(&idle_nodes, &past_inputs, &past_preds, true)
            .unwrap();

        assert_eq!(mapping[zero_delay.index()], None);
        assert_eq!(mapping[zero_split.index()], None);
        assert_eq!(mapping[add.index()], None);
        assert!(mapping[eqz.index()].is_some());

        // Verify output is bit-exact to original truth table
        let compiled = editable.compiled().clone();
        for x in 0..4u64 {
            let mut out = [0u64];
            editable
                .state_mut()
                .step(&compiled, &[x], &mut out)
                .unwrap();
            let exp = if x == 0 { 1 } else { 0 };
            assert_eq!(out[0], exp);
        }
    }
}
