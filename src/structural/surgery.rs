//! Transactional graph edits that preserve live register phases and node economy.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

use crate::baselines::DEFAULT_MAX_CERTIFY_JOINT_STEPS;
use crate::metrics::{allocate_circuit_description_bits, circuit_code_bits_with_tax};
use crate::rng::SeedBank;
use crate::structural::economy::{EconomyConfig, EconomyError, EconomyState};
use crate::structural::graph_edit::{cone_channel_mask, source_has_cycle};
use crate::structural::growth::ExcisionMode;
use crate::structural::sensitivity::{
    ComparatorTarget, EXACT_SENSITIVITY_MAX_BITS,
    counterfactual_sensitivity_with_comparators_audited,
};
use crate::substrate::{
    Circuit, CompiledCircuit, EvalError, NodeId, Primitive, ScalarState, SignalSink, SignalSource,
    WireDelayMode,
};

/// Audited node-evaluation counters recorded during structural verification and pruning.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SurgeryEvalAudit {
    pub replay_node_evals: u64,
    pub sensitivity_node_evals: u64,
}

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
    diagnostic_window: usize,
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
            diagnostic_window: growth_config.diagnostic_window,
        })
    }

    /// Diagnostic window $W_{\text{diag}}$ tied to this editable circuit.
    #[must_use]
    pub const fn diagnostic_window(&self) -> usize {
        self.diagnostic_window
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

    /// Overwrites live registers with `regs` without advancing `structure_epoch`.
    pub(crate) fn restore_registers_preserving_epoch(
        &mut self,
        regs: &[u64],
    ) -> Result<(), EvalError> {
        self.state.set_registers(&self.compiled, regs)
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
        self.prune_starved_nodes_with_mode(
            ExcisionMode::VerifiedLocalExcision,
            self.diagnostic_window,
        )
    }

    /// Excises mature depleted nodes using the selected [`ExcisionMode`] and diagnostic window.
    pub fn prune_starved_nodes_with_mode(
        &mut self,
        excision_mode: ExcisionMode,
        diagnostic_window: usize,
    ) -> Result<Vec<Option<NodeId>>, SurgeryError> {
        self.prune_starved_nodes_with_mode_audited(
            excision_mode,
            diagnostic_window,
            &mut SurgeryEvalAudit::default(),
        )
    }

    pub(crate) fn prune_starved_nodes_with_mode_audited(
        &mut self,
        excision_mode: ExcisionMode,
        diagnostic_window: usize,
        audit: &mut SurgeryEvalAudit,
    ) -> Result<Vec<Option<NodeId>>, SurgeryError> {
        let removed: BTreeSet<NodeId> = self
            .economy
            .nodes()
            .iter()
            .enumerate()
            .filter(|(_, node)| node.juvenile_ticks == 0 && node.energy <= 0.0)
            .map(|(index, _)| NodeId(u32::try_from(index).expect("node count fits in u32")))
            .collect();
        self.prune_starved_set_with_mode_audited(&removed, excision_mode, diagnostic_window, audit)
    }

    /// Excises an explicit set of starved mature nodes using the selected [`ExcisionMode`].
    pub(crate) fn prune_starved_set_with_mode_audited(
        &mut self,
        removed: &BTreeSet<NodeId>,
        excision_mode: ExcisionMode,
        diagnostic_window: usize,
        audit: &mut SurgeryEvalAudit,
    ) -> Result<Vec<Option<NodeId>>, SurgeryError> {
        if removed.is_empty() {
            return Ok((0..self.circuit.node_count())
                .map(|index| {
                    Some(NodeId(
                        u32::try_from(index).expect("node count fits in u32"),
                    ))
                })
                .collect());
        }
        let pre_circuit = self.circuit.clone();
        let pre_compiled = self.compiled.clone();
        let pre_regs = self.state.registers().to_vec();
        let (staged, old_to_new) =
            self.build_pruned_circuit_with_mode(removed, excision_mode, diagnostic_window, None)?;
        let staged_compiled = staged.compile()?;
        let shifted_live = (excision_mode == ExcisionMode::VerifiedLocalExcision
            && pre_circuit.wire_delay() == WireDelayMode::TicksPerHop)
            .then(|| {
                let reg_map = register_mapping(
                    &pre_circuit,
                    &staged,
                    &pre_compiled,
                    &staged_compiled,
                    &old_to_new,
                );
                let mut remapped = ScalarState::new(&staged_compiled).registers().to_vec();
                for (old_r, new_r) in reg_map.iter().enumerate() {
                    if let Some(nr) = new_r
                        && old_r < pre_regs.len()
                    {
                        remapped[*nr] = pre_regs[old_r];
                    }
                }
                align_tph_autonomous_after_prune(
                    &pre_circuit,
                    &pre_compiled,
                    &pre_regs,
                    &staged,
                    &staged_compiled,
                    &remapped,
                    &mut audit.replay_node_evals,
                )
            })
            .flatten();
        self.recompile_preserving_state(staged, &old_to_new)?;
        if let Some(regs) = shifted_live {
            self.set_registers(&regs)?;
        }
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
        self.prune_zero_sensitivity_mature_nodes_with_mode(
            idle_nodes,
            past_inputs,
            past_predictions,
            split_code_tax,
            ExcisionMode::VerifiedLocalExcision,
            self.diagnostic_window,
            None,
            None,
        )
    }

    /// Excises causally idle / ring-identity mature nodes under the specified [`ExcisionMode`].
    #[allow(clippy::too_many_arguments)]
    pub fn prune_zero_sensitivity_mature_nodes_with_mode(
        &mut self,
        idle_nodes: &BTreeSet<NodeId>,
        past_inputs: &[Vec<u64>],
        past_predictions: &[Vec<u64>],
        split_code_tax: bool,
        excision_mode: ExcisionMode,
        diagnostic_window: usize,
        past_node_outputs: Option<&[Vec<u64>]>,
        replay_initial_regs: Option<&[u64]>,
    ) -> Result<Vec<Option<NodeId>>, SurgeryError> {
        self.prune_zero_sensitivity_mature_nodes_with_mode_audited(
            idle_nodes,
            past_inputs,
            past_predictions,
            split_code_tax,
            excision_mode,
            diagnostic_window,
            past_node_outputs,
            replay_initial_regs,
            &mut SurgeryEvalAudit::default(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prune_zero_sensitivity_mature_nodes_with_mode_audited(
        &mut self,
        idle_nodes: &BTreeSet<NodeId>,
        past_inputs: &[Vec<u64>],
        past_predictions: &[Vec<u64>],
        split_code_tax: bool,
        excision_mode: ExcisionMode,
        diagnostic_window: usize,
        past_node_outputs: Option<&[Vec<u64>]>,
        replay_initial_regs: Option<&[u64]>,
        audit: &mut SurgeryEvalAudit,
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
        } else if excision_mode == ExcisionMode::VerifiedLocalExcision {
            let mut protected_comp: BTreeSet<NodeId> =
                comparator_exclusive_nodes(&self.circuit, &self.economy);
            for comp in self.economy.comparators() {
                protected_comp.insert(comp.sub_node);
                protected_comp.insert(comp.eqz_node);
            }
            let retained: BTreeSet<NodeId> = idle_nodes
                .iter()
                .copied()
                .filter(|node| !protected_comp.contains(node))
                .collect();
            if retained.is_empty() {
                return Ok(identity_map());
            }
            std::borrow::Cow::Owned(retained)
        } else {
            let active =
                self.comparator_and_boundary_active_nodes(past_inputs, &comp_targets, audit);
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

        let Ok((staged, old_to_new)) = self.build_pruned_circuit_with_mode(
            filtered_idle.as_ref(),
            excision_mode,
            diagnostic_window,
            past_node_outputs,
        ) else {
            return Ok(identity_map());
        };

        let Ok(staged_compiled) = staged.compile() else {
            return Ok(identity_map());
        };

        let base_regs = replay_initial_regs.unwrap_or_else(|| self.state.registers());
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
                && old_r < base_regs.len()
            {
                staged_regs[*nr] = base_regs[old_r];
            }
        }
        if excision_mode == ExcisionMode::VerifiedLocalExcision
            && self.circuit.wire_delay() == WireDelayMode::TicksPerHop
            && let Some(aligned) = align_tph_autonomous_after_prune(
                &self.circuit,
                &self.compiled,
                base_regs,
                &staged,
                &staged_compiled,
                &staged_regs,
                &mut audit.replay_node_evals,
            )
        {
            staged_regs = aligned;
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
        if !comp_targets.is_empty()
            && (surviving_eqz_wires.len() != comp_targets.len()
                || comp_targets.iter().any(|t| {
                    old_to_new
                        .get(t.sub_node.index())
                        .copied()
                        .flatten()
                        .is_none()
                }))
        {
            return Ok(identity_map());
        }

        let mut orig_probe_state = if surviving_eqz_wires.is_empty() {
            None
        } else {
            let mut st = ScalarState::new(&self.compiled);
            if st.set_registers(&self.compiled, base_regs).is_err() {
                return Ok(identity_map());
            }
            Some(st)
        };
        let mut orig_pred_buf = vec![0u64; usize::from(self.circuit.num_outputs())];
        let mut staged_pred = vec![0u64; usize::from(staged.num_outputs())];
        let orig_nodes = self.circuit.node_count() as u64;
        let staged_nodes = staged.node_count() as u64;
        for (frame_idx, input_frame) in past_inputs.iter().enumerate() {
            audit.replay_node_evals = audit.replay_node_evals.saturating_add(staged_nodes);
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
                audit.replay_node_evals = audit.replay_node_evals.saturating_add(orig_nodes);
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

        let live_shifted = (excision_mode == ExcisionMode::VerifiedLocalExcision
            && self.circuit.wire_delay() == WireDelayMode::TicksPerHop)
            .then(|| {
                let live_regs = self.state.registers();
                let mut remapped = ScalarState::new(&staged_compiled).registers().to_vec();
                for (old_r, new_r) in reg_map.iter().enumerate() {
                    if let Some(nr) = new_r
                        && old_r < live_regs.len()
                    {
                        remapped[*nr] = live_regs[old_r];
                    }
                }
                align_tph_autonomous_after_prune(
                    &self.circuit,
                    &self.compiled,
                    live_regs,
                    &staged,
                    &staged_compiled,
                    &remapped,
                    &mut audit.replay_node_evals,
                )
            })
            .flatten();

        self.recompile_preserving_state(staged, &old_to_new)?;
        if let Some(regs) = live_shifted {
            self.set_registers(&regs)?;
        }
        Ok(old_to_new)
    }

    /// Attempts `M3-E2` verified local pass-through excision on `port`'s mature primary cone.
    ///
    /// Finds a mature primary-cone node `v` and upstream source `x` (`x in UpstreamCone(v)`)
    /// empirically matching over `W_diag` frames, proves formal equivalence (`v === x`) over
    /// `S = ExcisedSubgraph(v, x)` via `ConstLoop(c)` fixed-point binding + exhaustive truth-table
    /// evaluation (or bounded joint-state certification + replay for stateful/`TicksPerHop` cones),
    /// verifies strict MDL reduction `L(C') < L(C)`, and commits atomically preserving state.
    pub fn excise_verified_passthrough_subgraphs(
        &mut self,
        port: u16,
        past_inputs: &[Vec<u64>],
        past_predictions: &[Vec<u64>],
        past_node_outputs: &[Vec<u64>],
        replay_initial_regs: Option<&[u64]>,
        split_code_tax: bool,
    ) -> Result<Option<Vec<Option<NodeId>>>, SurgeryError> {
        self.excise_verified_passthrough_subgraphs_audited(
            port,
            past_inputs,
            past_predictions,
            past_node_outputs,
            replay_initial_regs,
            split_code_tax,
            &mut SurgeryEvalAudit::default(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn excise_verified_passthrough_subgraphs_audited(
        &mut self,
        port: u16,
        past_inputs: &[Vec<u64>],
        past_predictions: &[Vec<u64>],
        past_node_outputs: &[Vec<u64>],
        replay_initial_regs: Option<&[u64]>,
        split_code_tax: bool,
        audit: &mut SurgeryEvalAudit,
    ) -> Result<Option<Vec<Option<NodeId>>>, SurgeryError> {
        let w_diag = past_node_outputs.len();
        if w_diag == 0 || past_inputs.len() != w_diag {
            return Ok(None);
        }
        let primary_nodes = primary_cone_nodes(&self.circuit, &self.economy, port);
        let comp_exclusive = comparator_exclusive_nodes(&self.circuit, &self.economy);
        let candidate_cone: BTreeSet<NodeId> =
            primary_nodes.union(&comp_exclusive).copied().collect();
        if candidate_cone.is_empty() {
            return Ok(None);
        }

        let protected: BTreeSet<NodeId> = self
            .economy
            .comparators()
            .iter()
            .flat_map(|c| [c.sub_node, c.eqz_node])
            .collect();
        let mut const_loops_cache = None;
        let width = self.circuit.width();
        let mut l_old_cache: Option<f64> = None;
        let mut comp_targets_cache: Option<Vec<ComparatorTarget>> = None;

        // Order candidate `v` by topological output proximity (outermost primary/comp nodes first).
        let mut candidates_v: Vec<NodeId> = candidate_cone
            .iter()
            .copied()
            .filter(|&v| {
                !protected.contains(&v)
                    && self.circuit.nodes().get(v.index()) != Some(&Primitive::Split)
                    && self
                        .economy
                        .nodes()
                        .get(v.index())
                        .is_some_and(|n| n.juvenile_ticks == 0)
            })
            .collect();
        candidates_v.sort_by_key(|v| std::cmp::Reverse(v.index()));

        for v in candidates_v {
            // Collect upstream sources `x` in reversed BFS order, run eligibility + cheap Step-1
            // empirical `W_diag` matching first, and only score/sort if `matched.len() > 1`.
            let mut upstream_sources = collect_upstream_sources_bfs(&self.circuit, v);
            upstream_sources.reverse();
            let mut matched: Vec<(usize, SignalSource, usize)> = Vec::new();
            for (rev_idx, x) in upstream_sources.into_iter().enumerate() {
                if let SignalSource::NodeOut { node: u, .. } = x
                    && (u == v
                        || protected.contains(&u)
                        || (primary_nodes.contains(&v) != primary_nodes.contains(&u))
                        || (comp_exclusive.contains(&v) != comp_exclusive.contains(&u))
                        || self
                            .economy
                            .nodes()
                            .get(u.index())
                            .is_some_and(|n| n.juvenile_ticks > 0))
                {
                    continue;
                }

                // Step 1: Empirical W_diag screen.
                let delta_h = match self.circuit.wire_delay() {
                    WireDelayMode::Combinational => {
                        let matched_w = (0..w_diag).all(|t| {
                            let v_val = past_node_outputs[t]
                                .get(v.index())
                                .map(|&val| width.normalize(val));
                            let x_val = signal_source_frame_value(
                                x,
                                &past_inputs[t],
                                &past_node_outputs[t],
                                width,
                            );
                            v_val.is_some() && v_val == x_val
                        });
                        if !matched_w {
                            continue;
                        }
                        0usize
                    }
                    WireDelayMode::TicksPerHop => {
                        let direct_match = (0..w_diag).all(|t| {
                            let v_val = past_node_outputs[t]
                                .get(v.index())
                                .map(|&val| width.normalize(val));
                            let x_val = signal_source_frame_value(
                                x,
                                &past_inputs[t],
                                &past_node_outputs[t],
                                width,
                            );
                            v_val.is_some() && v_val == x_val
                        });
                        if direct_match {
                            0usize
                        } else if cone_channel_mask(&self.circuit, x) == 0
                            && source_has_cycle(&self.circuit, x)
                        {
                            let Some(h) = min_hop_distance(&self.circuit, x, v) else {
                                continue;
                            };
                            if h == 0 || h >= w_diag {
                                continue;
                            }
                            let shifted_match = (h..w_diag).all(|t| {
                                let v_val = past_node_outputs[t]
                                    .get(v.index())
                                    .map(|&val| width.normalize(val));
                                let x_val = signal_source_frame_value(
                                    x,
                                    &past_inputs[t - h],
                                    &past_node_outputs[t - h],
                                    width,
                                );
                                v_val.is_some() && v_val == x_val
                            });
                            if !shifted_match {
                                continue;
                            }
                            h
                        } else {
                            continue;
                        }
                    }
                };
                matched.push((rev_idx, x, delta_h));
            }

            if matched.is_empty() {
                continue;
            }
            let ordered_matches = if matched.len() == 1 {
                matched
                    .into_iter()
                    .map(|(_, x, delta_h)| (x, delta_h))
                    .collect()
            } else {
                self.prioritize_matched_passthrough_sources(v, matched, &protected, split_code_tax)
            };

            let const_loops = const_loops_cache.get_or_insert_with(|| {
                detect_fixed_point_const_loops(
                    &self.circuit,
                    &self.compiled,
                    &self.state,
                    &mut audit.replay_node_evals,
                )
            });
            let l_old = *l_old_cache.get_or_insert_with(|| {
                circuit_code_bits_with_tax(&self.circuit, 0, split_code_tax)
            });
            let comp_targets = comp_targets_cache.get_or_insert_with(|| self.comparator_targets());

            // Steps 2 & 3: build candidate C', verify equivalence over S, verify strict MDL reduction, commit.
            for (x, delta_h) in ordered_matches {
                if let Some(old_to_new) = self.try_certify_and_commit_passthrough_bypass(
                    v,
                    x,
                    &protected,
                    const_loops,
                    comp_targets,
                    past_inputs,
                    past_predictions,
                    replay_initial_regs,
                    delta_h,
                    l_old,
                    split_code_tax,
                    false,
                    &mut audit.replay_node_evals,
                )? {
                    return Ok(Some(old_to_new));
                }
            }
        }

        Ok(None)
    }

    fn prioritize_matched_passthrough_sources<T: Copy>(
        &self,
        v: NodeId,
        matched: Vec<(usize, SignalSource, T)>,
        protected: &BTreeSet<NodeId>,
        split_code_tax: bool,
    ) -> Vec<(SignalSource, T)> {
        let mut scored: Vec<(f64, usize, usize, SignalSource, T)> = matched
            .into_iter()
            .map(|(rev_idx, x, meta)| {
                let (l_new, exc_len) = self
                    .build_passthrough_bypass_candidate(v, x, protected)
                    .map_or((f64::INFINITY, 0), |(staged, _, exc)| {
                        (
                            circuit_code_bits_with_tax(&staged, 0, split_code_tax),
                            exc.len(),
                        )
                    });
                (l_new, exc_len, rev_idx, x, meta)
            })
            .collect();
        scored.sort_by(|a, b| {
            a.0.total_cmp(&b.0)
                .then_with(|| b.1.cmp(&a.1))
                .then_with(|| a.2.cmp(&b.2))
        });
        scored
            .into_iter()
            .map(|(_, _, _, x, meta)| (x, meta))
            .collect()
    }

    fn prioritize_passthrough_sources(
        &self,
        v: NodeId,
        mut upstream_sources: Vec<SignalSource>,
        protected: &BTreeSet<NodeId>,
        split_code_tax: bool,
    ) -> Vec<SignalSource> {
        upstream_sources.reverse();
        let indexed: Vec<(usize, SignalSource, ())> = upstream_sources
            .into_iter()
            .enumerate()
            .map(|(rev_idx, x)| (rev_idx, x, ()))
            .collect();
        self.prioritize_matched_passthrough_sources(v, indexed, protected, split_code_tax)
            .into_iter()
            .map(|(x, ())| x)
            .collect()
    }

    /// Attempts M3-E2 local pass-through excision when `starved_set` threatens a node inside
    /// a primary cone under `ExcisionMode::VerifiedLocalExcision`, preventing destructive primary
    /// cone collapse when a side-arm or pass-through operator starves before `W_diag` completes.
    pub fn excise_verified_passthrough_for_starved_cone(
        &mut self,
        starved_set: &BTreeSet<NodeId>,
        split_code_tax: bool,
    ) -> Result<Option<Vec<Option<NodeId>>>, SurgeryError> {
        self.excise_verified_passthrough_for_starved_cone_audited(
            starved_set,
            split_code_tax,
            &mut SurgeryEvalAudit::default(),
        )
    }

    pub(crate) fn excise_verified_passthrough_for_starved_cone_audited(
        &mut self,
        starved_set: &BTreeSet<NodeId>,
        split_code_tax: bool,
        audit: &mut SurgeryEvalAudit,
    ) -> Result<Option<Vec<Option<NodeId>>>, SurgeryError> {
        if starved_set.is_empty() {
            return Ok(None);
        }
        let protected: BTreeSet<NodeId> = self
            .economy
            .comparators()
            .iter()
            .flat_map(|c| [c.sub_node, c.eqz_node])
            .collect();
        let comp_exclusive = comparator_exclusive_nodes(&self.circuit, &self.economy);
        let const_loops = detect_fixed_point_const_loops(
            &self.circuit,
            &self.compiled,
            &self.state,
            &mut audit.replay_node_evals,
        );
        let l_old = circuit_code_bits_with_tax(&self.circuit, 0, split_code_tax);
        let comp_targets = self.comparator_targets();
        let num_outputs = self.circuit.num_outputs();

        for port in 0..num_outputs {
            let primary_nodes = primary_cone_nodes(&self.circuit, &self.economy, port);
            if primary_nodes.is_empty() || primary_nodes.is_disjoint(starved_set) {
                continue;
            }
            let mut candidates_v: Vec<NodeId> = primary_nodes
                .iter()
                .copied()
                .filter(|&v| {
                    !protected.contains(&v)
                        && self.circuit.nodes().get(v.index()) != Some(&Primitive::Split)
                        && self
                            .economy
                            .nodes()
                            .get(v.index())
                            .is_some_and(|n| n.juvenile_ticks == 0)
                })
                .collect();
            candidates_v.sort_by_key(|v| std::cmp::Reverse(v.index()));

            for v in candidates_v {
                let upstream_sources = collect_upstream_sources_bfs(&self.circuit, v);
                // Only inspect `v` if `v` itself or its upstream cone contains a starved node.
                let touches_starved = starved_set.contains(&v)
                    || upstream_sources.iter().any(|src| {
                        matches!(src, SignalSource::NodeOut { node, .. } if starved_set.contains(node))
                    });
                if !touches_starved {
                    continue;
                }
                for x in self.prioritize_passthrough_sources(
                    v,
                    upstream_sources,
                    &protected,
                    split_code_tax,
                ) {
                    if let SignalSource::NodeOut { node: u, .. } = x {
                        if u == v
                            || starved_set.contains(&u)
                            || protected.contains(&u)
                            || comp_exclusive.contains(&u)
                            || self
                                .economy
                                .nodes()
                                .get(u.index())
                                .is_some_and(|n| n.juvenile_ticks > 0)
                        {
                            continue;
                        }
                        let x_anc = collect_upstream_sources_bfs(&self.circuit, u);
                        let candidate_excised = self
                            .build_passthrough_bypass_candidate(v, x, &protected)
                            .map(|(_, _, exc)| exc);
                        if x_anc.iter().any(|src| {
                            matches!(src, SignalSource::NodeOut { node, .. }
                                if starved_set.contains(node)
                                    && candidate_excised
                                        .as_ref()
                                        .is_none_or(|exc| !exc.contains(node)))
                        }) {
                            continue;
                        }
                    }
                    if let Some(old_to_new) = self.try_certify_and_commit_passthrough_bypass(
                        v,
                        x,
                        &protected,
                        &const_loops,
                        &comp_targets,
                        &[],
                        &[],
                        None,
                        0,
                        l_old,
                        split_code_tax,
                        true,
                        &mut audit.replay_node_evals,
                    )? {
                        return Ok(Some(old_to_new));
                    }
                }
            }
        }
        Ok(None)
    }

    #[allow(clippy::too_many_arguments)]
    fn try_certify_and_commit_passthrough_bypass(
        &mut self,
        v: NodeId,
        x: SignalSource,
        protected: &BTreeSet<NodeId>,
        const_loops: &BTreeMap<NodeId, u64>,
        comp_targets: &[ComparatorTarget],
        past_inputs: &[Vec<u64>],
        past_predictions: &[Vec<u64>],
        replay_initial_regs: Option<&[u64]>,
        delta_h: usize,
        l_old: f64,
        split_code_tax: bool,
        allow_juvenile: bool,
        replay_node_evals: &mut u64,
    ) -> Result<Option<Vec<Option<NodeId>>>, SurgeryError> {
        // Build candidate bypassed circuit C' and S = ExcisedSubgraph(v, x).
        let Some((staged, old_to_new, excised_set)) =
            self.build_passthrough_bypass_candidate(v, x, protected)
        else {
            return Ok(None);
        };
        if !excised_set.contains(&v) {
            return Ok(None);
        }
        // Never excise juvenile nodes (unless explicitly enabled) or protected comparator nodes.
        if excised_set.iter().any(|n| {
            protected.contains(n)
                || (!allow_juvenile
                    && self
                        .economy
                        .nodes()
                        .get(n.index())
                        .is_some_and(|e| e.juvenile_ticks > 0))
        }) {
            return Ok(None);
        }
        let Ok(staged_compiled) = staged.compile() else {
            return Ok(None);
        };

        // Step 3 pre-filter: strict MDL reduction L(C') < L(C).
        let l_new = circuit_code_bits_with_tax(&staged, 0, split_code_tax);
        if l_new >= l_old {
            return Ok(None);
        }

        // Step 2: Exact functional equivalence certification over S = ExcisedSubgraph(v, x).
        let has_non_const_stateful = excised_set.iter().any(|n| {
            !const_loops.contains_key(n)
                && self
                    .circuit
                    .nodes()
                    .get(n.index())
                    .is_some_and(|p| p.is_stateful())
        });
        let comb_cert = if self.circuit.wire_delay() == WireDelayMode::Combinational {
            certify_combinational_passthrough(&self.circuit, v, x, &excised_set, const_loops)
        } else {
            None
        };

        if comb_cert == Some(false)
            && !has_non_const_stateful
            && cone_channel_mask(&self.circuit, x) != 0
        {
            return Ok(None);
        }

        let reg_map = register_mapping(
            &self.circuit,
            &staged,
            &self.compiled,
            &staged_compiled,
            &old_to_new,
        );
        let live_regs = self.state.registers();
        let mut staged_live_regs = ScalarState::new(&staged_compiled).registers().to_vec();
        for (old_r, new_r) in reg_map.iter().enumerate() {
            if let Some(nr) = new_r
                && old_r < live_regs.len()
            {
                staged_live_regs[*nr] = live_regs[old_r];
            }
        }
        if self.circuit.wire_delay() == WireDelayMode::TicksPerHop
            && let Some(aligned) = align_tph_autonomous_after_prune(
                &self.circuit,
                &self.compiled,
                live_regs,
                &staged,
                &staged_compiled,
                &staged_live_regs,
                replay_node_evals,
            )
        {
            staged_live_regs = aligned;
        }

        let replay_ok = if let Some(init_regs) = replay_initial_regs {
            let mut staged_start_regs = ScalarState::new(&staged_compiled).registers().to_vec();
            for (old_r, new_r) in reg_map.iter().enumerate() {
                if let Some(nr) = new_r
                    && old_r < init_regs.len()
                {
                    staged_start_regs[*nr] = init_regs[old_r];
                }
            }
            if self.circuit.wire_delay() == WireDelayMode::TicksPerHop
                && let Some(aligned) = align_tph_autonomous_after_prune(
                    &self.circuit,
                    &self.compiled,
                    init_regs,
                    &staged,
                    &staged_compiled,
                    &staged_start_regs,
                    replay_node_evals,
                )
            {
                staged_start_regs = aligned;
            }
            verify_replay_and_comparators(
                &self.circuit,
                &self.compiled,
                init_regs,
                &staged,
                &staged_compiled,
                &staged_start_regs,
                &old_to_new,
                comp_targets,
                past_inputs,
                past_predictions,
                replay_node_evals,
            )
        } else {
            true
        };
        if !replay_ok {
            return Ok(None);
        }

        let certified = if comb_cert == Some(true) {
            true
        } else {
            let _ = delta_h;
            let joint_start_orig = replay_initial_regs.unwrap_or(live_regs);
            let joint_start_staged = if let Some(init_regs) = replay_initial_regs {
                let mut regs = ScalarState::new(&staged_compiled).registers().to_vec();
                for (old_r, new_r) in reg_map.iter().enumerate() {
                    if let Some(nr) = new_r
                        && old_r < init_regs.len()
                    {
                        regs[*nr] = init_regs[old_r];
                    }
                }
                if self.circuit.wire_delay() == WireDelayMode::TicksPerHop
                    && let Some(aligned) = align_tph_autonomous_after_prune(
                        &self.circuit,
                        &self.compiled,
                        init_regs,
                        &staged,
                        &staged_compiled,
                        &regs,
                        replay_node_evals,
                    )
                {
                    regs = aligned;
                }
                regs
            } else {
                staged_live_regs.clone()
            };
            certify_bounded_joint_state_equivalence(
                &self.circuit,
                &self.compiled,
                joint_start_orig,
                &staged,
                &staged_compiled,
                &joint_start_staged,
                &old_to_new,
                comp_targets,
                replay_node_evals,
            )
        };

        if !certified {
            return Ok(None);
        }

        // Step 3: Commit state-preserving bypass.
        self.recompile_preserving_state(staged, &old_to_new)?;
        if self.circuit.wire_delay() == WireDelayMode::TicksPerHop {
            self.set_registers(&staged_live_regs)?;
        }
        Ok(Some(old_to_new))
    }

    fn comparator_and_boundary_active_nodes(
        &self,
        past_inputs: &[Vec<u64>],
        comp_targets: &[ComparatorTarget],
        audit: &mut SurgeryEvalAudit,
    ) -> Vec<bool> {
        let node_count = self.circuit.node_count();
        let mut direct_active = vec![false; node_count];
        let mut any_latch_active = vec![false; node_count];
        let sample_count = usize::from(EXACT_SENSITIVITY_MAX_BITS) * 2;

        let mut accumulate =
            |state: &ScalarState, tick_block: u64, audit: &mut SurgeryEvalAudit| {
                if let Ok(snap) = counterfactual_sensitivity_with_comparators_audited(
                    &self.circuit,
                    &self.compiled,
                    state,
                    SeedBank::new(0),
                    tick_block,
                    sample_count,
                    comp_targets,
                    &mut audit.sensitivity_node_evals,
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

        accumulate(&self.state, 0, audit);
        if !past_inputs.is_empty() {
            let mut probe_state = self.state.clone();
            let mut pred_buf = vec![0u64; usize::from(self.circuit.num_outputs())];
            for (idx, frame) in past_inputs.iter().enumerate() {
                audit.replay_node_evals = audit.replay_node_evals.saturating_add(node_count as u64);
                if probe_state
                    .step(&self.compiled, frame, &mut pred_buf)
                    .is_err()
                {
                    break;
                }
                accumulate(&probe_state, (idx + 1) as u64, audit);
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

    fn build_pruned_circuit_with_mode(
        &self,
        initially_removed: &BTreeSet<NodeId>,
        excision_mode: ExcisionMode,
        diagnostic_window: usize,
        past_node_outputs: Option<&[Vec<u64>]>,
    ) -> Result<(Circuit, Vec<Option<NodeId>>), SurgeryError> {
        if excision_mode == ExcisionMode::BaselineD1b1 {
            return self.build_pruned_circuit_baseline_d1b1(initially_removed);
        }

        let node_count = self.circuit.node_count();
        let width = self.circuit.width();
        let w_diag = diagnostic_window.max(1);
        let gamma = self.economy.config().trace_discount;
        let decay_floor = gamma.powi(w_diag as i32) * (1.0 + 1e-9);

        let protected_sub_nodes: BTreeSet<NodeId> = self
            .economy
            .comparators()
            .iter()
            .map(|comp| comp.sub_node)
            .collect();
        let protected_comp_nodes: BTreeSet<NodeId> = self
            .economy
            .comparators()
            .iter()
            .flat_map(|comp| [comp.sub_node, comp.eqz_node])
            .collect();

        let mut logical_edges: Vec<(SignalSource, SignalSink)> = self
            .circuit
            .edges()
            .iter()
            .map(|e| (e.src, e.dst))
            .collect();
        let mut bypasses = Vec::new();
        let mut direct_bypass_ports: BTreeSet<u16> = BTreeSet::new();

        let mut orig_split_out_count = vec![0usize; node_count];
        for edge in self.circuit.edges() {
            if let SignalSource::NodeOut { node, .. } = edge.src
                && node.index() < node_count
            {
                orig_split_out_count[node.index()] += 1;
            }
        }

        let mut additive_zero_sources = BTreeSet::new();
        let mut multiplicative_one_sources = BTreeSet::new();
        for &node in initially_removed {
            if protected_comp_nodes.contains(&node) {
                continue;
            }
            let act = self
                .economy
                .nodes()
                .get(node.index())
                .map(|n| n.activity)
                .unwrap_or_default();
            let act_recorded = act.ever_nonzero() || act.ever_non_one();
            let buf_zero = past_node_outputs.is_some_and(|frames| {
                !frames.is_empty()
                    && frames
                        .iter()
                        .all(|f| f.get(node.index()).map(|&v| width.normalize(v)) == Some(0))
            });
            let buf_one = past_node_outputs.is_some_and(|frames| {
                !frames.is_empty()
                    && frames
                        .iter()
                        .all(|f| f.get(node.index()).map(|&v| width.normalize(v)) == Some(1))
            });
            let is_zero = buf_zero
                || (act_recorded && (!act.ever_nonzero() || act.nonzero_weight <= decay_floor))
                || (!act_recorded
                    && matches!(
                        self.circuit.nodes().get(node.index()),
                        Some(Primitive::Delay { init: 0 })
                    ));
            let is_one = buf_one
                || (act_recorded && (!act.ever_non_one() || act.non_one_weight <= decay_floor))
                || (!act_recorded
                    && matches!(
                        self.circuit.nodes().get(node.index()),
                        Some(Primitive::Delay { init: 1 })
                    ));
            if is_zero && !is_one {
                additive_zero_sources.insert(node);
            } else if is_one && !is_zero {
                multiplicative_one_sources.insert(node);
            } else if is_zero {
                additive_zero_sources.insert(node);
            }
        }

        // Propagate constant identity sources through Split/zero-Mul ahead of initializing `removed`,
        // so non-constant Split taps and identity-bypassable Add/Sub/Mul consumers can be deferred.
        loop {
            let mut propagated = false;
            for (index, &primitive) in self.circuit.nodes().iter().enumerate() {
                let node = NodeId(u32::try_from(index).expect("node count fits in u32"));
                if protected_comp_nodes.contains(&node) {
                    continue;
                }
                match primitive {
                    Primitive::Split => {
                        if let Some(src_node) =
                            input_source(&logical_edges, node, 0).and_then(source_node)
                        {
                            if additive_zero_sources.contains(&src_node) {
                                propagated |= additive_zero_sources.insert(node);
                            } else if multiplicative_one_sources.contains(&src_node) {
                                propagated |= multiplicative_one_sources.insert(node);
                            }
                        }
                    }
                    Primitive::Mul => {
                        let zero_in = (0..2).any(|p| {
                            input_source(&logical_edges, node, p)
                                .and_then(source_node)
                                .is_some_and(|s| additive_zero_sources.contains(&s))
                        });
                        if zero_in {
                            propagated |= additive_zero_sources.insert(node);
                        }
                    }
                    _ => {}
                }
            }
            if !propagated {
                break;
            }
        }

        let mut removed = BTreeSet::new();
        for &node in initially_removed {
            let prim = self.circuit.nodes().get(node.index()).copied();
            let defer_split = prim == Some(Primitive::Split)
                && !additive_zero_sources.contains(&node)
                && !multiplicative_one_sources.contains(&node)
                && input_source(&logical_edges, node, 0)
                    .and_then(source_node)
                    .is_none_or(|src| src != node);
            let defer_consumer = !protected_sub_nodes.contains(&node)
                && match prim {
                    Some(Primitive::Add) => (0..2).any(|p| {
                        input_source(&logical_edges, node, p)
                            .and_then(source_node)
                            .is_some_and(|s| additive_zero_sources.contains(&s))
                    }),
                    Some(Primitive::Sub) => input_source(&logical_edges, node, 1)
                        .and_then(source_node)
                        .is_some_and(|s| additive_zero_sources.contains(&s)),
                    Some(Primitive::Mul) => (0..2).any(|p| {
                        input_source(&logical_edges, node, p)
                            .and_then(source_node)
                            .is_some_and(|s| multiplicative_one_sources.contains(&s))
                    }),
                    _ => false,
                };
            if !defer_split && !defer_consumer {
                removed.insert(node);
            }
        }

        loop {
            let mut outer_changed = false;

            // 1. Propagate additive-0 and multiplicative-1 sources forward through Split and zero-Mul.
            loop {
                let mut propagated = false;
                for (index, &primitive) in self.circuit.nodes().iter().enumerate() {
                    let node = NodeId(u32::try_from(index).expect("node count fits in u32"));
                    if protected_comp_nodes.contains(&node) {
                        continue;
                    }
                    match primitive {
                        Primitive::Split => {
                            if let Some(src_node) =
                                input_source(&logical_edges, node, 0).and_then(source_node)
                            {
                                if additive_zero_sources.contains(&src_node) {
                                    propagated |= additive_zero_sources.insert(node);
                                    propagated |= removed.insert(node);
                                } else if multiplicative_one_sources.contains(&src_node) {
                                    propagated |= multiplicative_one_sources.insert(node);
                                    propagated |= removed.insert(node);
                                }
                            }
                        }
                        Primitive::Mul => {
                            let zero_in = (0..2).any(|p| {
                                input_source(&logical_edges, node, p)
                                    .and_then(source_node)
                                    .is_some_and(|s| additive_zero_sources.contains(&s))
                            });
                            if zero_in {
                                propagated |= additive_zero_sources.insert(node);
                                propagated |= removed.insert(node);
                            }
                        }
                        _ => {}
                    }
                }
                if !propagated {
                    break;
                }
            }

            // 2. Bypass Add/Sub consumers of additive-0 and Mul consumers of multiplicative-1.
            for (index, &primitive) in self.circuit.nodes().iter().enumerate() {
                let consumer = NodeId(u32::try_from(index).expect("node count fits in u32"));
                if removed.contains(&consumer) || protected_sub_nodes.contains(&consumer) {
                    continue;
                }
                let (Some(s0), Some(s1)) = (
                    input_source(&logical_edges, consumer, 0),
                    input_source(&logical_edges, consumer, 1),
                ) else {
                    continue;
                };
                let s0_zero = source_node(s0).is_some_and(|n| additive_zero_sources.contains(&n));
                let s1_zero = source_node(s1).is_some_and(|n| additive_zero_sources.contains(&n));
                let s0_one =
                    source_node(s0).is_some_and(|n| multiplicative_one_sources.contains(&n));
                let s1_one =
                    source_node(s1).is_some_and(|n| multiplicative_one_sources.contains(&n));
                let s0_removed = source_node(s0).is_some_and(|n| removed.contains(&n));
                let s1_removed = source_node(s1).is_some_and(|n| removed.contains(&n));

                let bypass_survivor = match primitive {
                    Primitive::Add => {
                        if s0_zero && s1_zero {
                            additive_zero_sources.insert(consumer);
                            removed.insert(consumer);
                            outer_changed = true;
                            None
                        } else if s1_zero && !s0_removed {
                            Some(s0)
                        } else if s0_zero && !s1_removed {
                            Some(s1)
                        } else {
                            None
                        }
                    }
                    Primitive::Sub => {
                        if s0_zero && s1_zero {
                            additive_zero_sources.insert(consumer);
                            removed.insert(consumer);
                            outer_changed = true;
                            None
                        } else if s1_zero && !s0_removed {
                            Some(s0)
                        } else {
                            None
                        }
                    }
                    Primitive::Mul => {
                        if s0_one && s1_one {
                            multiplicative_one_sources.insert(consumer);
                            removed.insert(consumer);
                            outer_changed = true;
                            None
                        } else if s1_one && !s0_removed {
                            Some(s0)
                        } else if s0_one && !s1_removed {
                            Some(s1)
                        } else {
                            None
                        }
                    }
                    _ => None,
                };

                let Some(survivor) = bypass_survivor else {
                    continue;
                };
                let outputs = output_sinks(&logical_edges, consumer);
                if outputs.len() > 1 {
                    continue;
                }
                removed.insert(consumer);
                logical_edges
                    .retain(|(source, sink)| !edge_touches_nodes(*source, *sink, &removed));
                for output in outputs {
                    if let (SignalSource::BoundaryIn(_), SignalSink::BoundaryOut(p)) =
                        (survivor, output)
                    {
                        direct_bypass_ports.insert(p);
                    }
                    logical_edges.push((survivor, output));
                    bypasses.push((survivor, output));
                }
                outer_changed = true;
            }

            logical_edges.retain(|(source, sink)| !edge_touches_nodes(*source, *sink, &removed));

            // 3. Remove any node missing an input.
            for (index, &primitive) in self.circuit.nodes().iter().enumerate() {
                let node = NodeId(u32::try_from(index).expect("node count fits in u32"));
                if removed.contains(&node) {
                    continue;
                }
                if (0..primitive.input_arity())
                    .any(|port| input_source(&logical_edges, node, port).is_none())
                {
                    removed.insert(node);
                    outer_changed = true;
                }
            }
            logical_edges.retain(|(source, sink)| !edge_touches_nodes(*source, *sink, &removed));

            // 4. Transitive dead-cone sweep: mature non-comparator nodes that can no longer
            // reach any BoundaryOut or registered ComparatorSite.
            let live_from_targets =
                compute_live_target_reachable(&logical_edges, node_count, &protected_comp_nodes);
            for (index, live) in live_from_targets.iter().enumerate() {
                let node = NodeId(u32::try_from(index).expect("node count fits in u32"));
                if removed.contains(&node) || protected_comp_nodes.contains(&node) {
                    continue;
                }
                let is_mature = self
                    .economy
                    .nodes()
                    .get(index)
                    .is_none_or(|n| n.juvenile_ticks == 0);
                if (is_mature || initially_removed.contains(&node)) && !*live {
                    removed.insert(node);
                    outer_changed = true;
                }
            }
            logical_edges.retain(|(source, sink)| !edge_touches_nodes(*source, *sink, &removed));

            // 5. Collapse any Primitive::Split left with <= 1 surviving output sink.
            for (index, &primitive) in self.circuit.nodes().iter().enumerate() {
                let node = NodeId(u32::try_from(index).expect("node count fits in u32"));
                if removed.contains(&node) || primitive != Primitive::Split {
                    continue;
                }
                let outputs = output_sinks(&logical_edges, node);
                if outputs.len() > 1 {
                    continue;
                }
                if let Some(output) = outputs.first().copied() {
                    let Some(input) = input_source(&logical_edges, node, 0) else {
                        removed.insert(node);
                        outer_changed = true;
                        continue;
                    };
                    if source_node(input).is_some_and(|src| removed.contains(&src)) {
                        continue;
                    }
                    if matches!(
                        (input, output),
                        (SignalSource::BoundaryIn(_), SignalSink::BoundaryOut(_))
                    ) && !initially_removed.contains(&node)
                    {
                        continue;
                    }
                    if self.circuit.wire_delay() == WireDelayMode::TicksPerHop {
                        let lost_branch = orig_split_out_count[index] > 1;
                        let auto_in = cone_channel_mask(&self.circuit, input) == 0
                            && source_has_cycle(&self.circuit, input);
                        if !initially_removed.contains(&node) && !(lost_branch && auto_in) {
                            continue;
                        }
                    }
                    logical_edges.retain(|(source, sink)| {
                        !matches!(source, SignalSource::NodeOut { node: cur, .. } if *cur == node)
                            && !matches!(sink, SignalSink::NodeIn { node: cur, .. } if *cur == node)
                    });
                    if matches!(
                        (input, output),
                        (SignalSource::BoundaryIn(_), SignalSink::BoundaryOut(_))
                    ) {
                        removed.insert(node);
                        outer_changed = true;
                        break;
                    }
                    logical_edges.push((input, output));
                    bypasses.push((input, output));
                    removed.insert(node);
                    outer_changed = true;
                    break;
                } else {
                    removed.insert(node);
                    outer_changed = true;
                    break;
                }
            }

            if !outer_changed {
                let mut forced_any = false;
                for &node in initially_removed {
                    if !protected_comp_nodes.contains(&node) && removed.insert(node) {
                        forced_any = true;
                    }
                }
                if forced_any {
                    logical_edges
                        .retain(|(source, sink)| !edge_touches_nodes(*source, *sink, &removed));
                    continue;
                }
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
            if !staged
                .edges()
                .iter()
                .any(|e| e.src == source && e.dst == sink)
            {
                staged.connect(source, sink);
            }
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
            let wrap_bypass =
                matches!(dst, SignalSink::BoundaryOut(p) if direct_bypass_ports.contains(&p));
            if wrap_bypass || !staged.nodes().is_empty() {
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

    fn build_pruned_circuit_baseline_d1b1(
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

    fn build_passthrough_bypass_candidate(
        &self,
        v: NodeId,
        x: SignalSource,
        protected: &BTreeSet<NodeId>,
    ) -> Option<(Circuit, Vec<Option<NodeId>>, BTreeSet<NodeId>)> {
        let node_count = self.circuit.node_count();
        let mut logical_edges: Vec<(SignalSource, SignalSink)> = self
            .circuit
            .edges()
            .iter()
            .map(|e| (e.src, e.dst))
            .collect();
        let v_sinks = output_sinks(&logical_edges, v);
        if v_sinks.is_empty() {
            return None;
        }

        let mut orig_split_out_count = vec![0usize; node_count];
        for edge in self.circuit.edges() {
            if let SignalSource::NodeOut { node, .. } = edge.src
                && node.index() < node_count
            {
                orig_split_out_count[node.index()] += 1;
            }
        }

        logical_edges
            .retain(|(src, _)| !matches!(src, SignalSource::NodeOut { node, .. } if *node == v));
        for &sink in &v_sinks {
            logical_edges.push((x, sink));
        }

        // Compute nodes no longer reaching any BoundaryOut or protected comparator site.
        let live = compute_live_target_reachable(&logical_edges, node_count, protected);
        let mut removed = BTreeSet::new();
        for (idx, &is_live) in live.iter().enumerate() {
            if !is_live {
                removed.insert(NodeId(u32::try_from(idx).expect("fits in u32")));
            }
        }
        logical_edges.retain(|(src, dst)| !edge_touches_nodes(*src, *dst, &removed));

        // Ensure surviving fanout on `x` is valid (switching free Split port or inserting Split).
        let mut x_chosen = x;
        let x_out_edges: Vec<SignalSink> = logical_edges
            .iter()
            .filter_map(|(src, dst)| (*src == x_chosen).then_some(*dst))
            .collect();
        let mut need_extra_split = false;
        if x_out_edges.len() > 1 {
            if let SignalSource::NodeOut { node: xn, port: xp } = x_chosen
                && self.circuit.nodes().get(xn.index()) == Some(&Primitive::Split)
            {
                let other_src = SignalSource::NodeOut {
                    node: xn,
                    port: 1 - xp,
                };
                if !logical_edges.iter().any(|(src, _)| *src == other_src) && v_sinks.len() == 1 {
                    logical_edges.retain(|(src, dst)| !(*src == x_chosen && *dst == v_sinks[0]));
                    logical_edges.push((other_src, v_sinks[0]));
                    x_chosen = other_src;
                } else {
                    need_extra_split = true;
                }
            } else {
                need_extra_split = true;
            }
        }
        if need_extra_split && self.circuit.wire_delay() == WireDelayMode::TicksPerHop {
            return None;
        }

        let mut bypasses: Vec<(SignalSource, SignalSink)> =
            v_sinks.iter().map(|&sink| (x_chosen, sink)).collect();
        let mut direct_bypass_ports = BTreeSet::new();
        for &sink in &v_sinks {
            if let (SignalSource::BoundaryIn(_), SignalSink::BoundaryOut(p)) = (x_chosen, sink) {
                direct_bypass_ports.insert(p);
            }
        }

        // Iteratively remove disconnected nodes and collapse degenerate Split nodes.
        loop {
            logical_edges.retain(|(src, dst)| !edge_touches_nodes(*src, *dst, &removed));
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
            if !need_extra_split {
                for (index, &primitive) in self.circuit.nodes().iter().enumerate() {
                    let node = NodeId(u32::try_from(index).expect("node count fits in u32"));
                    if removed.contains(&node) || primitive != Primitive::Split {
                        continue;
                    }
                    let outputs = output_sinks(&logical_edges, node);
                    if outputs.len() > 1 {
                        continue;
                    }
                    if let Some(output) = outputs.first().copied() {
                        let Some(input) = input_source(&logical_edges, node, 0) else {
                            removed.insert(node);
                            changed = true;
                            break;
                        };
                        if source_node(input).is_some_and(|src| removed.contains(&src)) {
                            continue;
                        }
                        if matches!(
                            (input, output),
                            (SignalSource::BoundaryIn(_), SignalSink::BoundaryOut(_))
                        ) {
                            continue;
                        }
                        if self.circuit.wire_delay() == WireDelayMode::TicksPerHop {
                            let lost_branch = orig_split_out_count[index] > 1;
                            let auto_in = cone_channel_mask(&self.circuit, input) == 0
                                && source_has_cycle(&self.circuit, input);
                            if !(lost_branch && auto_in) {
                                continue;
                            }
                        }
                        logical_edges.retain(|(src, dst)| {
                            !matches!(src, SignalSource::NodeOut { node: cur, .. } if *cur == node)
                                && !matches!(dst, SignalSink::NodeIn { node: cur, .. } if *cur == node)
                        });
                        logical_edges.push((input, output));
                        bypasses.push((input, output));
                    }
                    removed.insert(node);
                    changed = true;
                    break;
                }
            }
            if !changed {
                break;
            }
        }

        let excised_set = removed.clone();
        let mut staged = self.circuit.clone();
        let old_to_new = staged.remove_nodes(&removed);
        if need_extra_split {
            let mapped_x = remap_source(x_chosen, &old_to_new)?;
            let existing_sinks: Vec<SignalSink> = staged
                .edges()
                .iter()
                .filter_map(|e| (e.src == mapped_x).then_some(e.dst))
                .collect();
            let sp = staged.add_node(Primitive::Split);
            for old_dst in existing_sinks {
                staged.disconnect(mapped_x, old_dst);
                staged.connect(SignalSource::NodeOut { node: sp, port: 0 }, old_dst);
            }
            staged.connect(mapped_x, SignalSink::NodeIn { node: sp, port: 0 });
            for &sink in &v_sinks {
                let mapped_sink = remap_sink(sink, &old_to_new)?;
                staged.connect(SignalSource::NodeOut { node: sp, port: 1 }, mapped_sink);
            }
        } else {
            for (src, sink) in bypasses {
                let Some(r_src) = remap_source(src, &old_to_new) else {
                    continue;
                };
                let Some(r_sink) = remap_sink(sink, &old_to_new) else {
                    continue;
                };
                if !staged
                    .edges()
                    .iter()
                    .any(|e| e.src == r_src && e.dst == r_sink)
                {
                    staged.connect(r_src, r_sink);
                }
            }
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
            let wrap_bypass =
                matches!(dst, SignalSink::BoundaryOut(p) if direct_bypass_ports.contains(&p));
            if wrap_bypass || !staged.nodes().is_empty() {
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

        Some((staged, old_to_new, excised_set))
    }
}

/// Returns the primary-derivation upstream cone of `BoundaryOut(port)` minus all registered
/// `ComparatorSite` subgraphs (`sub_node`, `eqz_node`, and exclusive `alt` nodes).
#[must_use]
pub fn primary_cone_nodes(
    circuit: &Circuit,
    economy: &EconomyState,
    port: u16,
) -> BTreeSet<NodeId> {
    let (mut cone, _) = port_upstream_cone_and_edges(circuit, port);
    for comp in economy.comparators() {
        cone.remove(&comp.sub_node);
        cone.remove(&comp.eqz_node);
    }
    let comp_excl = comparator_exclusive_nodes(circuit, economy);
    for node in comp_excl {
        cone.remove(&node);
    }
    cone
}

/// Returns shared `Primitive::Split` fan-out tap nodes whose extra fan-out branch only exists to
/// feed registered [`ComparatorSite`] subgraphs (i.e., collapsing registered comparators reduces the
/// `Split` to `<= 1` non-comparator consumer while leaving unregistered free-rider `Split`s intact).
#[must_use]
pub fn comparator_tap_split_nodes(circuit: &Circuit, economy: &EconomyState) -> BTreeSet<NodeId> {
    let comps = economy.comparators();
    if comps.is_empty() {
        return BTreeSet::new();
    }
    let mut comp_nodes = comparator_exclusive_nodes(circuit, economy);
    for comp in comps {
        comp_nodes.insert(comp.sub_node);
        comp_nodes.insert(comp.eqz_node);
    }

    let n = circuit.node_count();
    let mut out_edges: Vec<Vec<(usize, Option<NodeId>)>> = vec![Vec::new(); n];
    for (ei, edge) in circuit.edges().iter().enumerate() {
        if let SignalSource::NodeOut { node: src, .. } = edge.src {
            let dst_node = match edge.dst {
                SignalSink::NodeIn { node, .. } => Some(node),
                SignalSink::BoundaryOut(_) => None,
            };
            out_edges[src.index()].push((ei, dst_node));
        }
    }

    let mut eliminated_edge = vec![false; circuit.edges().len()];
    for (ei, edge) in circuit.edges().iter().enumerate() {
        if let SignalSink::NodeIn { node, .. } = edge.dst
            && comp_nodes.contains(&node)
        {
            eliminated_edge[ei] = true;
        }
    }

    let mut tap_splits = BTreeSet::new();
    loop {
        let mut changed = false;
        for (idx, edges) in out_edges.iter().enumerate().take(n) {
            if circuit.nodes()[idx] != Primitive::Split {
                continue;
            }
            let node = NodeId(u32::try_from(idx).expect("node index fits in u32"));
            if comp_nodes.contains(&node) || tap_splits.contains(&node) {
                continue;
            }
            let mut surv = 0usize;
            let mut elim = 0usize;
            for &(ei, _) in edges {
                if eliminated_edge[ei] {
                    elim += 1;
                } else {
                    surv += 1;
                }
            }
            if elim > 0 && surv <= 1 {
                tap_splits.insert(node);
                changed = true;
                if surv == 0 {
                    for (ei, edge) in circuit.edges().iter().enumerate() {
                        if matches!(edge.dst, SignalSink::NodeIn { node: d, .. } if d == node)
                            && !eliminated_edge[ei]
                        {
                            eliminated_edge[ei] = true;
                        }
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
    tap_splits
}

/// Returns [`primary_cone_nodes`] excluding pure registered-comparator tap `Split` nodes from
/// [`comparator_tap_split_nodes`].
#[must_use]
pub fn primary_cone_nodes_without_comparator_taps(
    circuit: &Circuit,
    economy: &EconomyState,
    port: u16,
) -> BTreeSet<NodeId> {
    let mut cone = primary_cone_nodes(circuit, economy, port);
    for tap in comparator_tap_split_nodes(circuit, economy) {
        cone.remove(&tap);
    }
    cone
}

fn comparator_exclusive_nodes(circuit: &Circuit, economy: &EconomyState) -> BTreeSet<NodeId> {
    let comps = economy.comparators();
    if comps.is_empty() {
        return BTreeSet::new();
    }
    let mut boundary_live = BTreeSet::new();
    let mut stack = Vec::new();
    for edge in circuit.edges() {
        if let SignalSink::BoundaryOut(_) = edge.dst
            && let SignalSource::NodeOut { node, .. } = edge.src
            && boundary_live.insert(node)
        {
            stack.push(node);
        }
    }
    while let Some(node) = stack.pop() {
        for edge in circuit.edges() {
            if let SignalSink::NodeIn { node: dst, .. } = edge.dst
                && dst == node
                && let SignalSource::NodeOut { node: src, .. } = edge.src
                && boundary_live.insert(src)
            {
                stack.push(src);
            }
        }
    }

    let mut excl = BTreeSet::new();
    let mut c_stack: Vec<NodeId> = comps
        .iter()
        .flat_map(|c| [c.sub_node, c.eqz_node])
        .collect();
    while let Some(node) = c_stack.pop() {
        if boundary_live.contains(&node) || !excl.insert(node) {
            continue;
        }
        for edge in circuit.edges() {
            if let SignalSink::NodeIn { node: dst, .. } = edge.dst
                && dst == node
                && let SignalSource::NodeOut { node: src, .. } = edge.src
            {
                c_stack.push(src);
            }
        }
    }
    excl
}

fn compute_live_target_reachable(
    edges: &[(SignalSource, SignalSink)],
    node_count: usize,
    protected: &BTreeSet<NodeId>,
) -> Vec<bool> {
    let mut live = vec![false; node_count];
    let mut stack = Vec::new();
    let mut reverse_adj: Vec<Vec<NodeId>> = vec![Vec::new(); node_count];
    for &(src, dst) in edges {
        if let SignalSource::NodeOut { node: src_n, .. } = src
            && src_n.index() < node_count
        {
            match dst {
                SignalSink::BoundaryOut(_) => {
                    if !live[src_n.index()] {
                        live[src_n.index()] = true;
                        stack.push(src_n);
                    }
                }
                SignalSink::NodeIn { node: dst_n, .. } if dst_n.index() < node_count => {
                    reverse_adj[dst_n.index()].push(src_n);
                }
                _ => {}
            }
        }
    }
    for &p in protected {
        if p.index() < node_count && !live[p.index()] {
            live[p.index()] = true;
            stack.push(p);
        }
    }
    while let Some(node) = stack.pop() {
        for &pred in &reverse_adj[node.index()] {
            if !live[pred.index()] {
                live[pred.index()] = true;
                stack.push(pred);
            }
        }
    }
    live
}

fn collect_upstream_sources_bfs(circuit: &Circuit, root: NodeId) -> Vec<SignalSource> {
    let mut sources = Vec::new();
    let mut seen_sources = BTreeSet::new();
    let mut visited_nodes: BTreeSet<NodeId> = [root].into_iter().collect();
    let mut queue = VecDeque::new();
    queue.push_back(root);
    while let Some(node) = queue.pop_front() {
        for edge in circuit.edges() {
            if let SignalSink::NodeIn { node: dst, .. } = edge.dst
                && dst == node
            {
                if seen_sources.insert(edge.src) {
                    sources.push(edge.src);
                }
                if let SignalSource::NodeOut { node: src_n, .. } = edge.src
                    && visited_nodes.insert(src_n)
                {
                    queue.push_back(src_n);
                }
            }
        }
    }
    sources
}

fn signal_source_frame_value(
    src: SignalSource,
    input_frame: &[u64],
    node_outs: &[u64],
    width: crate::substrate::Width,
) -> Option<u64> {
    match src {
        SignalSource::BoundaryIn(c) => input_frame.get(usize::from(c)).map(|&v| width.normalize(v)),
        SignalSource::NodeOut { node, .. } => {
            node_outs.get(node.index()).map(|&v| width.normalize(v))
        }
    }
}

fn min_hop_distance(circuit: &Circuit, from: SignalSource, to: NodeId) -> Option<usize> {
    let node_count = circuit.node_count();
    let mut dist = vec![usize::MAX; node_count];
    let mut queue = VecDeque::new();
    for edge in circuit.edges() {
        if edge.src == from
            && let SignalSink::NodeIn { node: dst, .. } = edge.dst
            && dst.index() < node_count
        {
            let h = usize::from(edge.hops);
            if h < dist[dst.index()] {
                dist[dst.index()] = h;
                queue.push_back(dst);
            }
        }
    }
    while let Some(cur) = queue.pop_front() {
        let d = dist[cur.index()];
        for edge in circuit.edges() {
            if let SignalSource::NodeOut { node: src, .. } = edge.src
                && src == cur
                && let SignalSink::NodeIn { node: dst, .. } = edge.dst
                && dst.index() < node_count
            {
                let nd = d.saturating_add(usize::from(edge.hops));
                if nd < dist[dst.index()] {
                    dist[dst.index()] = nd;
                    queue.push_back(dst);
                }
            }
        }
    }
    let res = dist.get(to.index()).copied().unwrap_or(usize::MAX);
    (res != usize::MAX).then_some(res)
}

fn detect_fixed_point_const_loops(
    circuit: &Circuit,
    compiled: &CompiledCircuit,
    state: &ScalarState,
    replay_node_evals: &mut u64,
) -> BTreeMap<NodeId, u64> {
    let mut result = BTreeMap::new();
    let node_count = circuit.node_count();
    let width = circuit.width();
    let wires = state.wire_values();
    let out_bases = node_out_bases(circuit);

    // 1. Canonical 2-node closed ConstLoop(c): Delay{init: c} <-> Split holding `c`.
    let mut inputs_of: Vec<Vec<SignalSource>> = vec![Vec::new(); node_count];
    for edge in circuit.edges() {
        if let SignalSink::NodeIn { node: dst, .. } = edge.dst
            && dst.index() < node_count
        {
            inputs_of[dst.index()].push(edge.src);
        }
    }
    let mut delay_reg_of = vec![None; node_count];
    let mut next_r = 0usize;
    for (idx, prim) in circuit.nodes().iter().enumerate() {
        if prim.is_stateful() {
            delay_reg_of[idx] = Some(next_r);
            next_r += 1;
        }
    }
    for (d_idx, prim) in circuit.nodes().iter().enumerate() {
        if let Primitive::Delay { init } = *prim
            && let [SignalSource::NodeOut { node: sp, .. }] = inputs_of[d_idx].as_slice()
            && circuit.nodes().get(sp.index()) == Some(&Primitive::Split)
            && let [SignalSource::NodeOut { node: d_back, .. }] = inputs_of[sp.index()].as_slice()
            && d_back.index() == d_idx
            && let Some(reg_idx) = delay_reg_of[d_idx]
            && state.registers().get(reg_idx).copied() == Some(width.normalize(init))
        {
            let c = width.normalize(init);
            result.insert(NodeId(u32::try_from(d_idx).expect("fits")), c);
            result.insert(*sp, c);
        }
    }

    // 2. Closed autonomous subgraphs with 0 BoundaryIn inputs whose live registers are a
    // step-1 fixed point (handles TicksPerHop `materialize_const_loop` and settled constants).
    let mut zero_in_probe = state.clone();
    let zeros_in = vec![0u64; usize::from(compiled.num_inputs())];
    let mut dummy_out = vec![0u64; usize::from(compiled.num_outputs())];
    *replay_node_evals = replay_node_evals.saturating_add(node_count as u64);
    if zero_in_probe
        .step(compiled, &zeros_in, &mut dummy_out)
        .is_ok()
    {
        // Identify nodes with zero BoundaryIn paths AND zero changing registers in their upstream cone.
        let mut has_boundary_in = vec![false; node_count];
        let mut stack = Vec::new();
        for (idx, ins) in inputs_of.iter().enumerate() {
            if ins.iter().any(|s| matches!(s, SignalSource::BoundaryIn(_))) {
                has_boundary_in[idx] = true;
                stack.push(NodeId(u32::try_from(idx).expect("fits")));
            }
        }
        // Any explicit Delay or TicksPerHop edge whose register changed on `step` is not fixed.
        let pre_regs = state.registers();
        let post_regs = zero_in_probe.registers();
        for (idx, reg_opt) in delay_reg_of.iter().enumerate() {
            if let Some(r) = reg_opt
                && pre_regs.get(*r) != post_regs.get(*r)
                && !has_boundary_in[idx]
            {
                has_boundary_in[idx] = true;
                stack.push(NodeId(u32::try_from(idx).expect("fits")));
            }
        }
        if circuit.wire_delay() == WireDelayMode::TicksPerHop {
            let bases = edge_register_bases(circuit, compiled.explicit_delay_count());
            for (e_idx, edge) in circuit.edges().iter().enumerate() {
                let base = bases[e_idx];
                let changed = (0..usize::from(edge.hops))
                    .any(|s| pre_regs.get(base + s) != post_regs.get(base + s));
                if changed
                    && let SignalSink::NodeIn { node: dst, .. } = edge.dst
                    && dst.index() < node_count
                    && !has_boundary_in[dst.index()]
                {
                    has_boundary_in[dst.index()] = true;
                    stack.push(dst);
                }
            }
        }
        while let Some(n) = stack.pop() {
            for edge in circuit.edges() {
                if let SignalSource::NodeOut { node: src, .. } = edge.src
                    && src == n
                    && let SignalSink::NodeIn { node: dst, .. } = edge.dst
                    && dst.index() < node_count
                    && !has_boundary_in[dst.index()]
                {
                    has_boundary_in[dst.index()] = true;
                    stack.push(dst);
                }
            }
        }
        // Any node with !has_boundary_in[idx] and valid current wire output is fixed-point constant.
        if !wires.is_empty() {
            let probe_wires = zero_in_probe.wire_values();
            for (idx, &flag) in has_boundary_in.iter().enumerate() {
                let nid = NodeId(u32::try_from(idx).expect("fits"));
                if !flag
                    && let Some(&w) = out_bases.get(idx)
                    && w < probe_wires.len()
                {
                    result
                        .entry(nid)
                        .or_insert_with(|| width.normalize(probe_wires[w]));
                }
            }
        }
    }

    result
}

fn certify_combinational_passthrough(
    circuit: &Circuit,
    v: NodeId,
    x: SignalSource,
    excised_set: &BTreeSet<NodeId>,
    const_loops: &BTreeMap<NodeId, u64>,
) -> Option<bool> {
    let width = circuit.width();
    let k = width.bits();
    if k > EXACT_SENSITIVITY_MAX_BITS {
        return None;
    }
    let node_count = circuit.node_count();
    let mut node_inputs: Vec<Vec<SignalSource>> = vec![Vec::new(); node_count];
    for (idx, prim) in circuit.nodes().iter().enumerate() {
        node_inputs[idx].resize(usize::from(prim.input_arity()), SignalSource::BoundaryIn(0));
    }
    for edge in circuit.edges() {
        if let SignalSink::NodeIn { node, port } = edge.dst
            && node.index() < node_count
            && usize::from(port) < node_inputs[node.index()].len()
        {
            node_inputs[node.index()][usize::from(port)] = edge.src;
        }
    }

    let collect_comb_ancestors = |start: SignalSource| -> BTreeSet<SignalSource> {
        let mut anc = BTreeSet::new();
        let mut stack = vec![start];
        while let Some(src) = stack.pop() {
            if !anc.insert(src) {
                continue;
            }
            if let SignalSource::NodeOut { node, .. } = src {
                if const_loops.contains_key(&node) {
                    continue;
                }
                if !circuit.nodes()[node.index()].is_stateful() {
                    for &inp in &node_inputs[node.index()] {
                        stack.push(inp);
                    }
                }
            }
        }
        anc
    };

    let anc_x = collect_comb_ancestors(x);
    let mut anc_s_other = BTreeSet::new();
    for &s_node in excised_set {
        if const_loops.contains_key(&s_node) {
            continue;
        }
        for &inp in &node_inputs[s_node.index()] {
            let in_s =
                matches!(inp, SignalSource::NodeOut { node, .. } if excised_set.contains(&node));
            if !in_s && inp != x {
                anc_s_other.extend(collect_comb_ancestors(inp));
            }
        }
    }
    let shared_roots: BTreeSet<SignalSource> = anc_x.intersection(&anc_s_other).copied().collect();

    // Canonicalize Split sources when building the symbolic DAG.
    let mut free_var_index: BTreeMap<SignalSource, usize> = BTreeMap::new();
    let mut memo_roots_check = vec![None; node_count];

    fn has_shared_ancestor(
        src: SignalSource,
        node_inputs: &[Vec<SignalSource>],
        const_loops: &BTreeMap<NodeId, u64>,
        shared_roots: &BTreeSet<SignalSource>,
        circuit: &Circuit,
        memo: &mut [Option<bool>],
    ) -> bool {
        if shared_roots.contains(&src) {
            return true;
        }
        let SignalSource::NodeOut { node, .. } = src else {
            return false;
        };
        if const_loops.contains_key(&node) || circuit.nodes()[node.index()].is_stateful() {
            return false;
        }
        if let Some(cached) = memo[node.index()] {
            return cached;
        }
        memo[node.index()] = Some(false);
        let res = node_inputs[node.index()].iter().any(|&inp| {
            has_shared_ancestor(inp, node_inputs, const_loops, shared_roots, circuit, memo)
        });
        memo[node.index()] = Some(res);
        res
    }

    #[derive(Clone, Debug)]
    enum SymExpr {
        Const(u64),
        Var(usize),
        Eqz(usize),
        Add(usize, usize),
        Sub(usize, usize),
        Mul(usize, usize),
    }

    #[allow(clippy::too_many_arguments)]
    fn build_expr(
        src: SignalSource,
        circuit: &Circuit,
        node_inputs: &[Vec<SignalSource>],
        excised_set: &BTreeSet<NodeId>,
        const_loops: &BTreeMap<NodeId, u64>,
        shared_roots: &BTreeSet<SignalSource>,
        memo_shared: &mut [Option<bool>],
        free_var_index: &mut BTreeMap<SignalSource, usize>,
        exprs: &mut Vec<SymExpr>,
        src_cache: &mut BTreeMap<SignalSource, usize>,
    ) -> usize {
        if let Some(&id) = src_cache.get(&src) {
            return id;
        }
        let res = match src {
            SignalSource::BoundaryIn(_) => {
                let next_idx = free_var_index.len();
                let var_idx = *free_var_index.entry(src).or_insert(next_idx);
                let id = exprs.len();
                exprs.push(SymExpr::Var(var_idx));
                id
            }
            SignalSource::NodeOut { node, .. } => {
                if let Some(&c) = const_loops.get(&node) {
                    let id = exprs.len();
                    exprs.push(SymExpr::Const(c));
                    id
                } else if circuit.nodes()[node.index()] == Primitive::Split {
                    let inp = node_inputs[node.index()][0];
                    build_expr(
                        inp,
                        circuit,
                        node_inputs,
                        excised_set,
                        const_loops,
                        shared_roots,
                        memo_shared,
                        free_var_index,
                        exprs,
                        src_cache,
                    )
                } else if circuit.nodes()[node.index()].is_stateful() {
                    let canon = SignalSource::NodeOut { node, port: 0 };
                    let next_idx = free_var_index.len();
                    let var_idx = *free_var_index.entry(canon).or_insert(next_idx);
                    let id = exprs.len();
                    exprs.push(SymExpr::Var(var_idx));
                    id
                } else {
                    let in_s = excised_set.contains(&node);
                    let expand_for_shared = !shared_roots.contains(&src)
                        && has_shared_ancestor(
                            src,
                            node_inputs,
                            const_loops,
                            shared_roots,
                            circuit,
                            memo_shared,
                        );
                    if !in_s && !expand_for_shared {
                        let canon = SignalSource::NodeOut { node, port: 0 };
                        let next_idx = free_var_index.len();
                        let var_idx = *free_var_index.entry(canon).or_insert(next_idx);
                        let id = exprs.len();
                        exprs.push(SymExpr::Var(var_idx));
                        id
                    } else {
                        let ins = &node_inputs[node.index()];
                        let prim = circuit.nodes()[node.index()];
                        match prim {
                            Primitive::Eqz => {
                                let a = build_expr(
                                    ins[0],
                                    circuit,
                                    node_inputs,
                                    excised_set,
                                    const_loops,
                                    shared_roots,
                                    memo_shared,
                                    free_var_index,
                                    exprs,
                                    src_cache,
                                );
                                let id = exprs.len();
                                exprs.push(SymExpr::Eqz(a));
                                id
                            }
                            Primitive::Add | Primitive::Sub | Primitive::Mul => {
                                let a = build_expr(
                                    ins[0],
                                    circuit,
                                    node_inputs,
                                    excised_set,
                                    const_loops,
                                    shared_roots,
                                    memo_shared,
                                    free_var_index,
                                    exprs,
                                    src_cache,
                                );
                                let b = build_expr(
                                    ins[1],
                                    circuit,
                                    node_inputs,
                                    excised_set,
                                    const_loops,
                                    shared_roots,
                                    memo_shared,
                                    free_var_index,
                                    exprs,
                                    src_cache,
                                );
                                let id = exprs.len();
                                let node_expr = match prim {
                                    Primitive::Add => SymExpr::Add(a, b),
                                    Primitive::Sub => SymExpr::Sub(a, b),
                                    Primitive::Mul => SymExpr::Mul(a, b),
                                    _ => unreachable!(),
                                };
                                exprs.push(node_expr);
                                id
                            }
                            Primitive::Split | Primitive::Delay { .. } => unreachable!(),
                        }
                    }
                }
            }
        };
        src_cache.insert(src, res);
        res
    }

    let mut exprs = Vec::new();
    let mut src_cache = BTreeMap::new();
    let v_id = build_expr(
        SignalSource::NodeOut { node: v, port: 0 },
        circuit,
        &node_inputs,
        excised_set,
        const_loops,
        &shared_roots,
        &mut memo_roots_check,
        &mut free_var_index,
        &mut exprs,
        &mut src_cache,
    );
    let x_id = build_expr(
        x,
        circuit,
        &node_inputs,
        excised_set,
        const_loops,
        &shared_roots,
        &mut memo_roots_check,
        &mut free_var_index,
        &mut exprs,
        &mut src_cache,
    );

    let num_vars = free_var_index.len();
    if num_vars > usize::from(EXACT_SENSITIVITY_MAX_BITS) {
        return None;
    }

    let card = 1usize << k;
    let total_assignments = card.checked_pow(num_vars as u32)?;
    let mut var_vals = vec![0u64; num_vars];
    let mut eval_vals = vec![0u64; exprs.len()];

    for assign in 0..total_assignments {
        let mut rem = assign;
        for slot in &mut var_vals {
            *slot = (rem % card) as u64;
            rem /= card;
        }
        for (i, expr) in exprs.iter().enumerate() {
            eval_vals[i] = match *expr {
                SymExpr::Const(c) => c,
                SymExpr::Var(vi) => var_vals[vi],
                SymExpr::Eqz(a) => width.eqz(eval_vals[a]),
                SymExpr::Add(a, b) => width.add(eval_vals[a], eval_vals[b]),
                SymExpr::Sub(a, b) => width.sub(eval_vals[a], eval_vals[b]),
                SymExpr::Mul(a, b) => width.mul(eval_vals[a], eval_vals[b]),
            };
        }
        if eval_vals[v_id] != eval_vals[x_id] {
            return Some(false);
        }
    }
    Some(true)
}

#[allow(clippy::too_many_arguments)]
fn verify_replay_and_comparators(
    orig_circuit: &Circuit,
    orig_compiled: &CompiledCircuit,
    orig_init_regs: &[u64],
    staged: &Circuit,
    staged_compiled: &CompiledCircuit,
    staged_init_regs: &[u64],
    old_to_new: &[Option<NodeId>],
    comp_targets: &[ComparatorTarget],
    past_inputs: &[Vec<u64>],
    past_predictions: &[Vec<u64>],
    replay_node_evals: &mut u64,
) -> bool {
    let mut staged_state = ScalarState::new(staged_compiled);
    if staged_state
        .set_registers(staged_compiled, staged_init_regs)
        .is_err()
    {
        return false;
    }
    let surviving_eqz_wires: Vec<(usize, usize)> = if comp_targets.is_empty() {
        Vec::new()
    } else {
        let orig_bases = node_out_bases(orig_circuit);
        let staged_bases = node_out_bases(staged);
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
    let mut orig_probe = if surviving_eqz_wires.is_empty() {
        None
    } else {
        let mut st = ScalarState::new(orig_compiled);
        if st.set_registers(orig_compiled, orig_init_regs).is_err() {
            return false;
        }
        Some(st)
    };
    let mut orig_pred = vec![0u64; usize::from(orig_circuit.num_outputs())];
    let mut staged_pred = vec![0u64; usize::from(staged.num_outputs())];
    let orig_nodes = orig_circuit.node_count() as u64;
    let staged_nodes = staged.node_count() as u64;
    for (frame_idx, input_frame) in past_inputs.iter().enumerate() {
        *replay_node_evals = replay_node_evals.saturating_add(staged_nodes);
        if staged_state
            .step(staged_compiled, input_frame, &mut staged_pred)
            .is_err()
        {
            return false;
        }
        if frame_idx < past_predictions.len() && staged_pred != past_predictions[frame_idx] {
            return false;
        }
        if let Some(ref mut orig_st) = orig_probe {
            *replay_node_evals = replay_node_evals.saturating_add(orig_nodes);
            if orig_st
                .step(orig_compiled, input_frame, &mut orig_pred)
                .is_err()
            {
                return false;
            }
            for &(orig_wire, staged_wire) in &surviving_eqz_wires {
                if staged_state.wire_values()[staged_wire] != orig_st.wire_values()[orig_wire] {
                    return false;
                }
            }
        }
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn certify_bounded_joint_state_equivalence(
    orig_circuit: &Circuit,
    orig_compiled: &CompiledCircuit,
    orig_init_regs: &[u64],
    staged: &Circuit,
    staged_compiled: &CompiledCircuit,
    staged_init_regs: &[u64],
    old_to_new: &[Option<NodeId>],
    comp_targets: &[ComparatorTarget],
    replay_node_evals: &mut u64,
) -> bool {
    let width = orig_circuit.width();
    let k = width.bits();
    let num_in = usize::from(orig_circuit.num_inputs());

    let mut active_in_channels = Vec::new();
    for ch in 0..num_in {
        let src = SignalSource::BoundaryIn(u16::try_from(ch).expect("fits"));
        let used_orig = circuit_uses_boundary_in(orig_circuit, src);
        let used_staged = circuit_uses_boundary_in(staged, src);
        if used_orig || used_staged {
            active_in_channels.push(ch);
        }
    }

    let m_in = active_in_channels.len();
    if m_in > usize::from(EXACT_SENSITIVITY_MAX_BITS) || k > EXACT_SENSITIVITY_MAX_BITS {
        return false;
    }
    let card = 1usize << k;
    let Some(total_in_combos) = card.checked_pow(m_in as u32) else {
        return false;
    };
    if total_in_combos > 64 {
        return false;
    }

    let surviving_eqz_wires: Vec<(usize, usize)> = if comp_targets.is_empty() {
        Vec::new()
    } else {
        let orig_bases = node_out_bases(orig_circuit);
        let staged_bases = node_out_bases(staged);
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

    let mut visited: BTreeSet<(Vec<u64>, Vec<u64>)> = BTreeSet::new();
    let mut queue: VecDeque<(Vec<u64>, Vec<u64>)> = VecDeque::new();
    let start_key = (orig_init_regs.to_vec(), staged_init_regs.to_vec());
    visited.insert(start_key.clone());
    queue.push_back(start_key);

    let mut orig_state = ScalarState::new(orig_compiled);
    let mut staged_state = ScalarState::new(staged_compiled);
    let mut orig_out = vec![0u64; usize::from(orig_circuit.num_outputs())];
    let mut staged_out = vec![0u64; usize::from(staged.num_outputs())];
    let mut input_buf = vec![0u64; num_in];
    let mut steps = 0usize;

    let orig_nodes = orig_circuit.node_count() as u64;
    let staged_nodes = staged.node_count() as u64;
    while let Some((orig_regs, staged_regs)) = queue.pop_front() {
        for assign in 0..total_in_combos {
            steps += 1;
            if steps > DEFAULT_MAX_CERTIFY_JOINT_STEPS {
                return false;
            }
            input_buf.fill(0);
            let mut rem = assign;
            for &ch in &active_in_channels {
                input_buf[ch] = (rem % card) as u64;
                rem /= card;
            }
            if orig_state.set_registers(orig_compiled, &orig_regs).is_err()
                || staged_state
                    .set_registers(staged_compiled, &staged_regs)
                    .is_err()
            {
                return false;
            }
            *replay_node_evals =
                replay_node_evals.saturating_add(orig_nodes.saturating_add(staged_nodes));
            if orig_state
                .step(orig_compiled, &input_buf, &mut orig_out)
                .is_err()
                || staged_state
                    .step(staged_compiled, &input_buf, &mut staged_out)
                    .is_err()
            {
                return false;
            }
            if orig_out != staged_out {
                return false;
            }
            for &(orig_wire, staged_wire) in &surviving_eqz_wires {
                if staged_state.wire_values()[staged_wire] != orig_state.wire_values()[orig_wire] {
                    return false;
                }
            }
            let next_key = (
                orig_state.registers().to_vec(),
                staged_state.registers().to_vec(),
            );
            if visited.insert(next_key.clone()) {
                queue.push_back(next_key);
            }
        }
    }

    true
}

fn circuit_uses_boundary_in(circuit: &Circuit, target: SignalSource) -> bool {
    let mut live_nodes = BTreeSet::new();
    let mut stack = Vec::new();
    for edge in circuit.edges() {
        if matches!(edge.dst, SignalSink::BoundaryOut(_)) {
            if edge.src == target {
                return true;
            }
            if let SignalSource::NodeOut { node, .. } = edge.src
                && live_nodes.insert(node)
            {
                stack.push(node);
            }
        }
    }
    while let Some(node) = stack.pop() {
        for edge in circuit.edges() {
            if let SignalSink::NodeIn { node: dst, .. } = edge.dst
                && dst == node
            {
                if edge.src == target {
                    return true;
                }
                if let SignalSource::NodeOut { node: src, .. } = edge.src
                    && live_nodes.insert(src)
                {
                    stack.push(src);
                }
            }
        }
    }
    false
}

fn align_tph_autonomous_after_prune(
    orig_circuit: &Circuit,
    orig_compiled: &CompiledCircuit,
    orig_regs: &[u64],
    staged: &Circuit,
    staged_compiled: &CompiledCircuit,
    remapped_regs: &[u64],
    replay_node_evals: &mut u64,
) -> Option<Vec<u64>> {
    if orig_circuit.wire_delay() != WireDelayMode::TicksPerHop {
        return None;
    }
    let num_outputs = usize::from(orig_circuit.num_outputs());
    let num_inputs = usize::from(orig_circuit.num_inputs());
    let zeros_in = vec![0u64; num_inputs];
    let orig_nodes = orig_circuit.node_count() as u64;
    let staged_nodes = staged.node_count() as u64;

    let mut orig_state = ScalarState::new(orig_compiled);
    orig_state.set_registers(orig_compiled, orig_regs).ok()?;
    let mut orig_seq = vec![Vec::with_capacity(64); num_outputs];
    let mut out_buf = vec![0u64; num_outputs];
    for _ in 0..64 {
        *replay_node_evals = replay_node_evals.saturating_add(orig_nodes);
        orig_state
            .step(orig_compiled, &zeros_in, &mut out_buf)
            .ok()?;
        for (p, &val) in out_buf.iter().enumerate() {
            orig_seq[p].push(val);
        }
    }

    let mut final_regs = remapped_regs.to_vec();
    let mut changed_any = false;

    // Collect delay and edge register indices for staged circuit.
    let mut delay_reg_of = vec![None; staged.node_count()];
    let mut next_d = 0usize;
    for (idx, prim) in staged.nodes().iter().enumerate() {
        if prim.is_stateful() {
            delay_reg_of[idx] = Some(next_d);
            next_d += 1;
        }
    }
    let staged_edge_bases = edge_register_bases(staged, staged_compiled.explicit_delay_count());

    for port in 0..orig_circuit.num_outputs() {
        let p_idx = usize::from(port);
        let Some(orig_driver) = orig_circuit
            .edges()
            .iter()
            .find(|e| e.dst == SignalSink::BoundaryOut(port))
            .map(|e| e.src)
        else {
            continue;
        };
        let Some(staged_driver) = staged
            .edges()
            .iter()
            .find(|e| e.dst == SignalSink::BoundaryOut(port))
            .map(|e| e.src)
        else {
            continue;
        };
        let is_auto = cone_channel_mask(orig_circuit, orig_driver) == 0
            && source_has_cycle(orig_circuit, orig_driver)
            && cone_channel_mask(staged, staged_driver) == 0
            && source_has_cycle(staged, staged_driver);
        if !is_auto {
            continue;
        }

        // Check if already aligned at s = 0.
        let mut check0 = ScalarState::new(staged_compiled);
        check0.set_registers(staged_compiled, &final_regs).ok()?;
        let mut already_ok = true;
        for &exp in &orig_seq[p_idx] {
            *replay_node_evals = replay_node_evals.saturating_add(staged_nodes);
            check0.step(staged_compiled, &zeros_in, &mut out_buf).ok()?;
            if out_buf[p_idx] != exp {
                already_ok = false;
                break;
            }
        }
        if already_ok {
            continue;
        }

        // Collect all register indices inside port's autonomous cone in `staged`.
        let (cone_nodes, cone_edges) = port_upstream_cone_and_edges(staged, port);
        let mut cone_regs = Vec::new();
        for &n in &cone_nodes {
            if let Some(r) = delay_reg_of[n.index()] {
                cone_regs.push(r);
            }
        }
        for (e_idx, edge) in staged.edges().iter().enumerate() {
            if cone_edges.contains(&(edge.src, edge.dst)) {
                let base = staged_edge_bases[e_idx];
                for s in 0..usize::from(edge.hops) {
                    cone_regs.push(base + s);
                }
            }
        }

        // Search shifts s in 1..=128 (starting after flushing at least 1 step so new edges populate).
        let mut probe = ScalarState::new(staged_compiled);
        probe.set_registers(staged_compiled, &final_regs).ok()?;
        let mut reg_history = Vec::with_capacity(192);
        let mut out_history = Vec::with_capacity(192);
        for _ in 0..192 {
            reg_history.push(probe.registers().to_vec());
            *replay_node_evals = replay_node_evals.saturating_add(staged_nodes);
            probe.step(staged_compiled, &zeros_in, &mut out_buf).ok()?;
            out_history.push(out_buf[p_idx]);
        }
        let mut found_s = None;
        for s in 1..=128usize {
            if out_history[s..s + 64] == orig_seq[p_idx][..] {
                found_s = Some(s);
                break;
            }
        }
        if let Some(s) = found_s {
            for r in cone_regs {
                if r < final_regs.len() && r < reg_history[s].len() {
                    final_regs[r] = reg_history[s][r];
                }
            }
            changed_any = true;
        }
    }

    changed_any.then_some(final_regs)
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
