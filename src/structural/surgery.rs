//! Transactional graph edits that preserve live register phases and node economy.

use std::collections::BTreeSet;
use std::fmt;

use crate::metrics::{allocate_circuit_description_bits, circuit_code_bits_with_tax};
use crate::rng::SeedBank;
use crate::structural::economy::{EconomyConfig, EconomyError, EconomyState};
use crate::structural::sensitivity::{
    ComparatorTarget, EXACT_SENSITIVITY_MAX_BITS, counterfactual_sensitivity_with_comparators,
};
use crate::substrate::{
    Circuit, CompiledCircuit, EvalError, NodeId, Primitive, ScalarState, SignalSink, SignalSource,
};

/// Errors returned by a state-preserving circuit edit.
#[derive(Debug)]
pub enum SurgeryError {
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
            Self::Substrate(error) => write!(f, "surgery produced an invalid circuit: {error}"),
            Self::Economy(error) => write!(f, "surgery economy remap failed: {error}"),
            Self::Evaluation(error) => write!(f, "surgery register remap failed: {error}"),
            Self::InvalidNodeMapping => write!(f, "invalid old-to-new node mapping"),
        }
    }
}

impl std::error::Error for SurgeryError {}

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

/// A live circuit together with the compiled execution plan, synchronous phase, and metabolic state.
#[derive(Clone, Debug)]
pub struct EditableCircuit {
    circuit: Circuit,
    compiled: CompiledCircuit,
    state: ScalarState,
    economy: EconomyState,
    structure_epoch: u64,
}

impl EditableCircuit {
    /// Builds an editable circuit with default growth configuration for observation horizon sizing.
    pub fn new(circuit: Circuit, economy_config: EconomyConfig) -> Result<Self, SurgeryError> {
        Self::new_with_growth(
            circuit,
            economy_config,
            &crate::structural::growth::GrowthConfig::default(),
        )
    }

    /// Builds an editable circuit with fresh scalar and metabolic state tied to `growth_config`.
    pub fn new_with_growth(
        circuit: Circuit,
        economy_config: EconomyConfig,
        growth_config: &crate::structural::growth::GrowthConfig,
    ) -> Result<Self, SurgeryError> {
        let compiled = circuit.compile()?;
        let state = ScalarState::new(&compiled);
        let obs_horizon =
            crate::structural::growth::observation_horizon(&economy_config, growth_config);
        let economy = EconomyState::new(
            economy_config,
            circuit.width(),
            circuit.node_count(),
            usize::from(circuit.num_outputs()),
            compiled.total_registers(),
            obs_horizon,
        )?;
        Ok(Self {
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

    /// Substrate circuit.
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

    /// Registers an internal comparator `EQZ(SUB(primary, alt))` on this circuit and initializes
    /// its wake scorer from `target_port`'s current wake null model.
    pub fn register_comparator(
        &mut self,
        sub_node: NodeId,
        eqz_node: NodeId,
        target_port: u16,
        primary_channel_mask: u64,
        alt_channel_mask: u64,
    ) -> Result<usize, SurgeryError> {
        let valid_prims = matches!(
            self.circuit.nodes().get(sub_node.index()),
            Some(Primitive::Sub)
        ) && matches!(
            self.circuit.nodes().get(eqz_node.index()),
            Some(Primitive::Eqz)
        );
        if !valid_prims {
            return Err(SurgeryError::Economy(EconomyError::DimensionMismatch));
        }
        Ok(self.economy.register_comparator(
            sub_node,
            eqz_node,
            target_port,
            primary_channel_mask,
            alt_channel_mask,
        )?)
    }

    /// Returns the substrate `(sub_node, eqz_node)` pairs for all registered comparators.
    #[must_use]
    pub fn comparator_targets(&self) -> Vec<ComparatorTarget> {
        self.economy
            .comparators()
            .iter()
            .map(crate::structural::economy::ComparatorSite::target)
            .collect()
    }

    /// Applies a graph edit and installs the supplied exact old-to-new node mapping.
    ///
    /// The edit is transactional: any compilation or remapping failure leaves this
    /// `EditableCircuit` unchanged. Nodes omitted from the map are treated as new.
    pub fn edit<F>(&mut self, edit: F) -> Result<Vec<Option<NodeId>>, SurgeryError>
    where
        F: FnOnce(&mut Circuit) -> Result<Vec<Option<NodeId>>, SurgeryError>,
    {
        let mut staged = self.circuit.clone();
        let old_to_new = edit(&mut staged)?;
        self.recompile_preserving_state(staged, &old_to_new)?;
        Ok(old_to_new)
    }

    /// Recompiles `circuit`, preserving state for each explicitly mapped surviving node and edge.
    pub fn recompile_preserving_state(
        &mut self,
        circuit: Circuit,
        old_to_new_nodes: &[Option<NodeId>],
    ) -> Result<(), SurgeryError> {
        if old_to_new_nodes.len() != self.circuit.node_count() {
            return Err(SurgeryError::InvalidNodeMapping);
        }
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

        self.circuit = circuit;
        self.compiled = compiled;
        self.state = state;
        self.economy = economy;
        self.structure_epoch = self.structure_epoch.wrapping_add(1);
        Ok(())
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
        let (staged, old_to_new) = self.build_pruned_circuit(&removed)?;
        self.recompile_preserving_state(staged, &old_to_new)?;
        Ok(old_to_new)
    }

    /// Excises causally idle mature nodes (zero counterfactual sensitivity over `W_diag` to both
    /// `BoundaryOut` and any registered `ComparatorSite::eqz_node`), validating that outputs and
    /// comparator `eqz_node` trajectories on `past_inputs` are preserved and code length does not increase (D1b.1).
    pub fn prune_zero_sensitivity_mature_nodes(
        &mut self,
        idle_nodes: &BTreeSet<NodeId>,
        past_inputs: &[Vec<u64>],
        past_predictions: &[Vec<u64>],
        split_code_tax: bool,
    ) -> Result<Vec<Option<NodeId>>, SurgeryError> {
        let identity_map = || -> Vec<Option<NodeId>> {
            (0..self.circuit.node_count())
                .map(|index| {
                    Some(NodeId(
                        u32::try_from(index).expect("node count fits in u32"),
                    ))
                })
                .collect()
        };
        if idle_nodes.is_empty() {
            return Ok(identity_map());
        }

        let comp_targets = self.comparator_targets();
        let filtered_idle: std::borrow::Cow<'_, BTreeSet<NodeId>> = if comp_targets.is_empty() {
            std::borrow::Cow::Borrowed(idle_nodes)
        } else {
            let active = self.comparator_and_boundary_active_nodes(past_inputs, &comp_targets);
            let retained: BTreeSet<NodeId> = idle_nodes
                .iter()
                .copied()
                .filter(|node| !active.get(node.index()).copied().unwrap_or(false))
                .collect();
            if retained.is_empty() {
                return Ok(identity_map());
            }
            std::borrow::Cow::Owned(retained)
        };

        let Ok((staged, old_to_new)) = self.build_pruned_circuit(filtered_idle.as_ref()) else {
            return Ok(identity_map());
        };

        let Ok(staged_compiled) = staged.compile() else {
            return Ok(identity_map());
        };

        let reg_map = register_mapping(
            &self.circuit,
            &staged,
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
            return Ok(identity_map());
        }

        let surviving_eqz_wires: Vec<(usize, usize)> = if comp_targets.is_empty() {
            Vec::new()
        } else {
            let orig_bases = node_out_bases(&self.circuit);
            let staged_bases = node_out_bases(&staged);
            comp_targets
                .iter()
                .filter_map(|target| {
                    let mapped_eqz = old_to_new.get(target.eqz_node.index()).copied().flatten()?;
                    Some((
                        orig_bases[target.eqz_node.index()],
                        staged_bases[mapped_eqz.index()],
                    ))
                })
                .collect()
        };

        let mut orig_probe_state = if surviving_eqz_wires.is_empty() {
            None
        } else {
            Some(self.state.clone())
        };
        let mut orig_pred_buf = vec![0u64; usize::from(self.circuit.num_outputs())];
        let mut staged_pred = vec![0u64; usize::from(staged.num_outputs())];
        for (frame_idx, input_frame) in past_inputs.iter().enumerate() {
            if staged_state
                .step(&staged_compiled, input_frame, &mut staged_pred)
                .is_err()
            {
                return Ok(identity_map());
            }
            if frame_idx < past_predictions.len() && staged_pred != past_predictions[frame_idx] {
                return Ok(identity_map());
            }
            if let Some(ref mut orig_state) = orig_probe_state {
                if orig_state
                    .step(&self.compiled, input_frame, &mut orig_pred_buf)
                    .is_err()
                {
                    return Ok(identity_map());
                }
                for &(orig_wire, staged_wire) in &surviving_eqz_wires {
                    if staged_state.wire_values()[staged_wire]
                        != orig_state.wire_values()[orig_wire]
                    {
                        return Ok(identity_map());
                    }
                }
            }
        }

        let l_old = circuit_code_bits_with_tax(&self.circuit, 0, split_code_tax);
        let l_new = circuit_code_bits_with_tax(&staged, 0, split_code_tax);
        if l_new > l_old {
            return Ok(identity_map());
        }

        self.recompile_preserving_state(staged, &old_to_new)?;
        Ok(old_to_new)
    }

    fn comparator_and_boundary_active_nodes(
        &self,
        past_inputs: &[Vec<u64>],
        comp_targets: &[ComparatorTarget],
    ) -> Vec<bool> {
        let node_count = self.circuit.node_count();
        let mut direct_active = vec![false; node_count];
        let mut any_latch_active = vec![false; node_count];
        let sample_count = usize::from(EXACT_SENSITIVITY_MAX_BITS) * 2;

        let mut accumulate = |state: &ScalarState, tick_block: u64| {
            if let Ok(snap) = counterfactual_sensitivity_with_comparators(
                &self.circuit,
                &self.compiled,
                state,
                SeedBank::new(0),
                tick_block,
                sample_count,
                comp_targets,
            ) {
                for node in 0..node_count {
                    if snap.base.output_by_node[node].iter().any(|&s| s > 0.0)
                        || snap.comparator_eqz_by_node[node].iter().any(|&s| s > 0.0)
                    {
                        direct_active[node] = true;
                    }
                    if snap.base.latch_by_node[node].iter().any(|&s| s > 0.0) {
                        any_latch_active[node] = true;
                    }
                }
            }
        };

        accumulate(&self.state, 0);
        if !past_inputs.is_empty() {
            let mut probe_state = self.state.clone();
            let mut pred_buf = vec![0u64; usize::from(self.circuit.num_outputs())];
            for (idx, frame) in past_inputs.iter().enumerate() {
                if probe_state
                    .step(&self.compiled, frame, &mut pred_buf)
                    .is_err()
                {
                    break;
                }
                accumulate(&probe_state, (idx + 1) as u64);
            }
        }

        // Propagate backwards across synchronous latch/hop edges from directly active nodes.
        let mut reverse_adj = vec![Vec::new(); node_count];
        for edge in self.circuit.edges() {
            if let (SignalSource::NodeOut { node: src, .. }, SignalSink::NodeIn { node: dst, .. }) =
                (edge.src, edge.dst)
            {
                reverse_adj[dst.index()].push(src.index());
            }
        }
        let mut active = direct_active.clone();
        let mut stack: Vec<usize> = (0..node_count).filter(|&i| direct_active[i]).collect();
        while let Some(node) = stack.pop() {
            for &pred in &reverse_adj[node] {
                if !active[pred] && (direct_active[pred] || any_latch_active[pred]) {
                    active[pred] = true;
                    stack.push(pred);
                }
            }
        }
        active
    }

    fn build_pruned_circuit(
        &self,
        initially_removed: &BTreeSet<NodeId>,
    ) -> Result<(Circuit, Vec<Option<NodeId>>), SurgeryError> {
        let mut removed = initially_removed.clone();
        let mut logical_edges: Vec<(SignalSource, SignalSink)> = self
            .circuit
            .edges()
            .iter()
            .map(|e| (e.src, e.dst))
            .collect();
        let mut bypasses = Vec::new();

        let mut inactive_zero_nodes: BTreeSet<NodeId> = initially_removed
            .iter()
            .copied()
            .filter(|&node| {
                matches!(
                    self.circuit.nodes().get(node.index()),
                    Some(Primitive::Delay { init: 0 })
                ) && !self.economy.nodes()[node.index()].activity.ever_nonzero()
            })
            .collect();

        loop {
            let mut propagated = false;
            for (index, &primitive) in self.circuit.nodes().iter().enumerate() {
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

        let protected_sub_nodes: BTreeSet<NodeId> = self
            .economy
            .comparators()
            .iter()
            .map(|comp| comp.sub_node)
            .collect();

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
                if removed.contains(&consumer) || protected_sub_nodes.contains(&consumer) {
                    continue;
                }
                let Some(&primitive) = self.circuit.nodes().get(consumer.index()) else {
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
            for (index, &primitive) in self.circuit.nodes().iter().enumerate() {
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
            for (index, &primitive) in self.circuit.nodes().iter().enumerate() {
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

        let mut staged = self.circuit.clone();
        let old_to_new = staged.remove_nodes(&removed);
        for (source, sink) in bypasses {
            let Some(source) = remap_source(source, &old_to_new) else {
                continue;
            };
            let Some(sink) = remap_sink(sink, &old_to_new) else {
                continue;
            };
            staged.connect(source, sink);
        }

        let mut direct_wires = Vec::new();
        for edge in staged.edges() {
            if matches!(
                (edge.src, edge.dst),
                (SignalSource::BoundaryIn(_), SignalSink::BoundaryOut(_))
            ) {
                direct_wires.push((edge.src, edge.dst));
            }
        }
        for (src, dst) in direct_wires {
            staged.disconnect(src, dst);
            if !staged.nodes().is_empty() {
                let split = staged.add_node(Primitive::Split);
                staged.connect(
                    src,
                    SignalSink::NodeIn {
                        node: split,
                        port: 0,
                    },
                );
                staged.connect(
                    SignalSource::NodeOut {
                        node: split,
                        port: 0,
                    },
                    dst,
                );
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
        split_code_tax: bool,
        old_to_new: &[Option<NodeId>],
    ) {
        let l_old = self.last_committed_code_bits;
        let l_new = circuit_code_bits_with_tax(new_circuit, 0, split_code_tax);
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
            let alloc = allocate_circuit_description_bits(new_circuit, 0, split_code_tax);
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
        self.last_committed_code_bits = circuit_code_bits_with_tax(new_circuit, 0, split_code_tax);
    }

    /// Ensures ledger vectors match circuit node count, allocating debts for new nodes.
    pub fn ensure_capacity(&mut self, circuit: &Circuit, split_code_tax: bool) {
        let n = circuit.node_count();
        if self.node_debts.len() < n {
            let alloc = allocate_circuit_description_bits(circuit, 0, split_code_tax);
            while self.node_debts.len() < n {
                let idx = self.node_debts.len();
                let debt = alloc.node_debts.get(idx).copied().unwrap_or(0.0);
                self.node_debts.push(debt);
                self.debt_paid.push(false);
            }
            self.last_committed_code_bits = circuit_code_bits_with_tax(circuit, 0, split_code_tax);
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

fn node_out_bases(circuit: &Circuit) -> Vec<usize> {
    let mut bases = Vec::with_capacity(circuit.node_count());
    let mut next_wire = usize::from(circuit.num_inputs());
    for primitive in circuit.nodes() {
        bases.push(next_wire);
        next_wire += usize::from(primitive.output_arity());
    }
    bases
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
    circuit: &Circuit,
    out_port: u16,
) -> (BTreeSet<NodeId>, BTreeSet<(SignalSource, SignalSink)>) {
    let mut cone_nodes = BTreeSet::new();
    let mut cone_edges = BTreeSet::new();
    let mut stack = Vec::new();
    let target = SignalSink::BoundaryOut(out_port);
    for edge in circuit.edges() {
        let (src, dst) = (edge.src, edge.dst);
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
        for edge in circuit.edges() {
            let (src, dst) = (edge.src, edge.dst);
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

pub(crate) fn port_uses_boundary_in(circuit: &Circuit, out_port: u16) -> bool {
    let (_, cone_edges) = port_upstream_cone_and_edges(circuit, out_port);
    cone_edges
        .iter()
        .any(|&(src, _)| matches!(src, SignalSource::BoundaryIn(_)))
}

/// Returns true if all nodes in the upstream cone of `out_port` are mature (§2.4).
pub(crate) fn port_is_cone_mature(
    circuit: &Circuit,
    economy: &crate::structural::economy::EconomyState,
    out_port: u16,
) -> bool {
    let (cone_nodes, _) = port_upstream_cone_and_edges(circuit, out_port);
    cone_nodes.iter().all(|&node| {
        economy
            .nodes()
            .get(node.index())
            .is_none_or(|n| n.juvenile_ticks == 0)
    })
}

/// Returns true if `out_port` is not driven or has non-zero positive ancestry share (§2.4).
pub(crate) fn port_is_primed(
    circuit: &Circuit,
    ancestry: &crate::structural::sensitivity::CausalAncestry,
    out_port: u16,
) -> bool {
    let is_driven = circuit
        .edges()
        .iter()
        .any(|edge| edge.dst == SignalSink::BoundaryOut(out_port));
    !is_driven
        || ancestry
            .port_shares
            .get(out_port as usize)
            .is_some_and(|shares| shares.iter().sum::<f64>() > 0.0)
}

pub(crate) fn port_cone_changed(
    old_circuit: &Circuit,
    new_circuit: &Circuit,
    old_to_new: &[Option<NodeId>],
    out_port: u16,
) -> bool {
    let (old_nodes, old_edges) = port_upstream_cone_and_edges(old_circuit, out_port);
    let (new_nodes, new_edges) = port_upstream_cone_and_edges(new_circuit, out_port);
    if old_nodes.len() != new_nodes.len() || old_edges.len() != new_edges.len() {
        return true;
    }
    for &old_node in &old_nodes {
        let Some(new_node) = old_to_new.get(old_node.index()).copied().flatten() else {
            return true;
        };
        if !new_nodes.contains(&new_node)
            || old_circuit.nodes().get(old_node.index())
                != new_circuit.nodes().get(new_node.index())
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
    use crate::substrate::{Width, WireDelayMode};

    fn oscillator_circuit() -> (Circuit, [NodeId; 5]) {
        let mut circuit = Circuit::new(Width::BITS_2, 0, 2, WireDelayMode::TicksPerHop);
        let delay0 = circuit.add_node(Primitive::delay_zero());
        let split0 = circuit.add_node(Primitive::Split);
        let removable = circuit.add_node(Primitive::Eqz);
        let delay1 = circuit.add_node(Primitive::delay_zero());
        let split1 = circuit.add_node(Primitive::Split);
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
            circuit.connect(source, sink);
        }
        (circuit, [delay0, split0, removable, delay1, split1])
    }

    #[test]
    fn recompilation_preserves_delay_phase_and_unchanged_hop_registers() {
        let (circuit, nodes) = oscillator_circuit();
        let mut editable = EditableCircuit::new(circuit, EconomyConfig::default()).unwrap();
        let compiled = editable.compiled().clone();
        editable
            .state_mut()
            .set_registers(&compiled, &[1, 2, 3, 0, 1, 2, 3, 0, 1])
            .unwrap();

        let mapping = editable
            .edit(|circuit| {
                let mapping = circuit.remove_nodes(&BTreeSet::from([nodes[2]]));
                let source = mapping[nodes[3].index()].unwrap();
                let sink = mapping[nodes[4].index()].unwrap();
                circuit.connect(
                    SignalSource::NodeOut {
                        node: source,
                        port: 0,
                    },
                    SignalSink::NodeIn {
                        node: sink,
                        port: 0,
                    },
                );
                Ok(mapping)
            })
            .unwrap();

        assert_eq!(mapping[nodes[2].index()], None);
        assert_eq!(editable.state().registers(), &[1, 2, 3, 0, 1, 0, 1, 0]);
        assert!(editable.circuit().compile().is_ok());
    }

    #[test]
    fn pruning_zero_corrector_bypasses_add_and_preserves_surviving_state() {
        let mut circuit = Circuit::new(Width::BITS_1, 0, 1, WireDelayMode::Combinational);
        let base_delay = circuit.add_node(Primitive::delay_zero());
        let base_split = circuit.add_node(Primitive::Split);
        let zero_delay = circuit.add_node(Primitive::delay_zero());
        let zero_split = circuit.add_node(Primitive::Split);
        let corrector_add = circuit.add_node(Primitive::Add);
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
            circuit.connect(source, sink);
        }
        let economy_config = EconomyConfig {
            initial_energy: 0.1,
            juvenile_ticks: 0,
            ..EconomyConfig::default()
        };
        let mut editable = EditableCircuit::new(circuit, economy_config).unwrap();
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
        assert_eq!(output, [1], "edges: {:?}", editable.circuit().edges());
    }

    #[test]
    fn coordinate_free_stacking_eqz_and_addself() {
        use crate::structural::graph_edit::{stack_output_binary_op, tap_source};
        let mut circuit = Circuit::new(Width::BITS_2, 1, 1, WireDelayMode::Combinational);

        // 1. First neuron: Eqz(x) connected to BoundaryOut(0)
        let eqz = circuit.add_node(Primitive::Eqz);
        circuit.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: eqz, port: 0 },
        );
        stack_output_binary_op(
            &mut circuit,
            Primitive::Add,
            0,
            SignalSource::NodeOut { node: eqz, port: 0 },
        );

        assert!(circuit.compile().is_ok());

        // 2. Stack corrector: AddSelf(x) = x + x (mod 4)
        let tapped = tap_source(&mut circuit, SignalSource::BoundaryIn(0));
        let split_self = circuit.add_node(Primitive::Split);
        let add_self = circuit.add_node(Primitive::Add);
        circuit.connect(
            tapped,
            SignalSink::NodeIn {
                node: split_self,
                port: 0,
            },
        );
        circuit.connect(
            SignalSource::NodeOut {
                node: split_self,
                port: 0,
            },
            SignalSink::NodeIn {
                node: add_self,
                port: 0,
            },
        );
        let tapped_self = tap_source(
            &mut circuit,
            SignalSource::NodeOut {
                node: split_self,
                port: 1,
            },
        );
        circuit.connect(
            tapped_self,
            SignalSink::NodeIn {
                node: add_self,
                port: 1,
            },
        );

        stack_output_binary_op(
            &mut circuit,
            Primitive::Add,
            0,
            SignalSource::NodeOut {
                node: add_self,
                port: 0,
            },
        );

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
        ledger.on_sprout(&circuit, true, &[None]);
        let l1 = circuit_code_bits_with_tax(&circuit, 0, true);
        assert!((ledger.node_debts.iter().sum::<f64>() - l1).abs() < 1e-12);

        // Step 2: Add node 1 (Delay)
        let _ = circuit.add_node(Primitive::Delay { init: 0 });
        ledger.on_sprout(&circuit, true, &[Some(NodeId(0)), None]);
        let l2 = circuit_code_bits_with_tax(&circuit, 0, true);
        assert!((ledger.node_debts.iter().sum::<f64>() - l2).abs() < 1e-12);
    }

    #[test]
    fn prune_zero_sensitivity_mature_nodes_excises_idle_const_loop() {
        let mut circuit = Circuit::new(Width::BITS_2, 1, 1, WireDelayMode::Combinational);
        let eqz = circuit.add_node(Primitive::Eqz);
        let zero_delay = circuit.add_node(Primitive::Delay { init: 0 });
        let zero_split = circuit.add_node(Primitive::Split);
        let add = circuit.add_node(Primitive::Add);

        circuit.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: eqz, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: eqz, port: 0 },
            SignalSink::NodeIn { node: add, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut {
                node: zero_delay,
                port: 0,
            },
            SignalSink::NodeIn {
                node: zero_split,
                port: 0,
            },
        );
        circuit.connect(
            SignalSource::NodeOut {
                node: zero_split,
                port: 0,
            },
            SignalSink::NodeIn {
                node: zero_delay,
                port: 0,
            },
        );
        circuit.connect(
            SignalSource::NodeOut {
                node: zero_split,
                port: 1,
            },
            SignalSink::NodeIn { node: add, port: 1 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: add, port: 0 },
            SignalSink::BoundaryOut(0),
        );

        let economy_config = EconomyConfig {
            initial_energy: 10.0,
            juvenile_ticks: 10,
            ..EconomyConfig::default()
        };
        let mut editable = EditableCircuit::new(circuit, economy_config).unwrap();

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

    #[test]
    fn d1b1_retains_comparator_cone_and_prunes_truly_idle_node() {
        let mut circuit = Circuit::new(Width::BITS_1, 3, 1, WireDelayMode::Combinational);
        let p_d = circuit.add_node(Primitive::Delay { init: 0 });
        let split_p = circuit.add_node(Primitive::Split);
        let a_d1 = circuit.add_node(Primitive::Delay { init: 0 });
        let a_d2 = circuit.add_node(Primitive::Delay { init: 0 });
        let sub = circuit.add_node(Primitive::Sub);
        let eqz = circuit.add_node(Primitive::Eqz);
        let dead_idle = circuit.add_node(Primitive::Delay { init: 0 });

        circuit.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: p_d, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: p_d, port: 0 },
            SignalSink::NodeIn {
                node: split_p,
                port: 0,
            },
        );
        circuit.connect(
            SignalSource::NodeOut {
                node: split_p,
                port: 0,
            },
            SignalSink::BoundaryOut(0),
        );
        circuit.connect(
            SignalSource::NodeOut {
                node: split_p,
                port: 1,
            },
            SignalSink::NodeIn { node: sub, port: 0 },
        );
        circuit.connect(
            SignalSource::BoundaryIn(1),
            SignalSink::NodeIn {
                node: a_d1,
                port: 0,
            },
        );
        circuit.connect(
            SignalSource::NodeOut {
                node: a_d1,
                port: 0,
            },
            SignalSink::NodeIn {
                node: a_d2,
                port: 0,
            },
        );
        circuit.connect(
            SignalSource::NodeOut {
                node: a_d2,
                port: 0,
            },
            SignalSink::NodeIn { node: sub, port: 1 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: sub, port: 0 },
            SignalSink::NodeIn { node: eqz, port: 0 },
        );
        circuit.connect(
            SignalSource::BoundaryIn(2),
            SignalSink::NodeIn {
                node: dead_idle,
                port: 0,
            },
        );

        let mut editable = EditableCircuit::new(circuit, EconomyConfig::default()).unwrap();
        editable
            .register_comparator(sub, eqz, 0, 0b01, 0b10)
            .unwrap();

        // Warm up Delay latches, then record an aligned periodic window so staged replay matches state.
        let compiled_init = editable.compiled().clone();
        for t in 0..4u64 {
            let mut out = [0u64];
            editable
                .state_mut()
                .step(&compiled_init, &[t % 2, (t + 1) % 2, t % 2], &mut out)
                .unwrap();
        }
        let mut past_inputs = Vec::new();
        let mut past_preds = Vec::new();
        for t in 4..36u64 {
            let x0 = t % 2;
            let x1 = (t + 1) % 2;
            let mut out = [0u64];
            editable
                .state_mut()
                .step(&compiled_init, &[x0, x1, x0], &mut out)
                .unwrap();
            past_inputs.push(vec![x0, x1, x0]);
            past_preds.push(out.to_vec());
        }

        // Simulate caller passing all non-boundary-sensitive nodes in idle_nodes.
        let mut idle_nodes = BTreeSet::new();
        idle_nodes.insert(a_d1);
        idle_nodes.insert(a_d2);
        idle_nodes.insert(sub);
        idle_nodes.insert(eqz);
        idle_nodes.insert(dead_idle);

        let mapping = editable
            .prune_zero_sensitivity_mature_nodes(&idle_nodes, &past_inputs, &past_preds, true)
            .unwrap();

        assert_eq!(mapping[dead_idle.index()], None);
        let new_sub = mapping[sub.index()].expect("sub_node must survive D1b.1");
        let new_eqz = mapping[eqz.index()].expect("eqz_node must survive D1b.1");
        assert!(
            mapping[a_d1.index()].is_some(),
            "alt delay 1 must survive D1b.1"
        );
        assert!(
            mapping[a_d2.index()].is_some(),
            "alt delay 2 must survive D1b.1"
        );
        assert_eq!(editable.economy().comparators().len(), 1);
        assert_eq!(editable.economy().comparators()[0].sub_node, new_sub);
        assert_eq!(editable.economy().comparators()[0].eqz_node, new_eqz);
    }

    #[test]
    fn inactive_zero_alt_does_not_bypass_registered_comparator_sub_node() {
        let mut circuit = Circuit::new(Width::BITS_1, 2, 1, WireDelayMode::Combinational);
        let p_d = circuit.add_node(Primitive::Delay { init: 0 });
        let split_p = circuit.add_node(Primitive::Split);
        let zero_alt = circuit.add_node(Primitive::Delay { init: 0 });
        let sub = circuit.add_node(Primitive::Sub);
        let eqz = circuit.add_node(Primitive::Eqz);

        circuit.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: p_d, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: p_d, port: 0 },
            SignalSink::NodeIn {
                node: split_p,
                port: 0,
            },
        );
        circuit.connect(
            SignalSource::NodeOut {
                node: split_p,
                port: 0,
            },
            SignalSink::BoundaryOut(0),
        );
        circuit.connect(
            SignalSource::NodeOut {
                node: split_p,
                port: 1,
            },
            SignalSink::NodeIn { node: sub, port: 0 },
        );
        circuit.connect(
            SignalSource::BoundaryIn(1),
            SignalSink::NodeIn {
                node: zero_alt,
                port: 0,
            },
        );
        circuit.connect(
            SignalSource::NodeOut {
                node: zero_alt,
                port: 0,
            },
            SignalSink::NodeIn { node: sub, port: 1 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: sub, port: 0 },
            SignalSink::NodeIn { node: eqz, port: 0 },
        );

        let config = EconomyConfig {
            juvenile_ticks: 0,
            ..EconomyConfig::default()
        };
        let mut editable = EditableCircuit::new(circuit, config).unwrap();
        editable
            .register_comparator(sub, eqz, 0, 0b01, 0b10)
            .unwrap();

        // Force zero_alt to starve while primary keeps positive energy (zero_alt never emitted non-zero).
        editable.economy_mut().nodes_mut()[zero_alt.index()].energy = 0.0;
        let mapping = editable.prune_starved_nodes().unwrap();

        assert_eq!(mapping[zero_alt.index()], None);
        assert_eq!(mapping[sub.index()], None);
        assert_eq!(mapping[eqz.index()], None);
        assert_eq!(mapping[split_p.index()], None);
        assert!(mapping[p_d.index()].is_some());
        assert!(editable.economy().comparators().is_empty());
        assert_eq!(editable.circuit().node_count(), 1);
    }
}
