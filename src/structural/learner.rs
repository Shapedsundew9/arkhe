//! Online closed-loop coordinate-free neurogenesis learner (Milestone 1b, Hypothesis H2).

use std::collections::{BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::baselines::{ObservationWindow, PredictorBaseline, TickBudget};
use crate::config::ArkheM1Telemetry;
use crate::metrics::circuit_code_bits_with_tax;
use crate::metrics::facts_code_bits;
use crate::rng::SeedBank;
use crate::structural::economy::{ComparatorSite, EconomyConfig, EconomyState, ExecutionPhase};
use crate::structural::graph_edit::{
    advance_tph_autonomous_ring_phase, comparator_exclusive_arm_nodes, excise_comparator_branch,
    is_pure_boundary_split, prune_starved_preserving_healthy_split_branches,
    swap_comparator_driver,
};
use crate::structural::growth::{
    AutonomousRingProposal, DiagnosticBuffer, ExcisionMode, GrowthConfig, GrowthError,
    RedundancySproutProposal, ScoredSproutProposal, SproutTelemetry, assign_stage3_juvenile_ticks,
    check_deeper_lag_for_port, diagnose_residual, effective_juvenile_ticks,
    evaluate_comparator_ring_promotion, observation_horizon, port_primary_cone_is_mature,
    propose_autonomous_delay_ring_scored_with_telemetry, propose_redundancy_sprout,
    propose_sprout_band_scored_internal,
};
use crate::structural::live::node_output_values;
use crate::structural::sensitivity::{
    CausalAncestry, ComparatorCausalAncestry, ComparatorSensitivitySnapshot, SensitivitySnapshot,
    assemble_causal_ancestry, assemble_causal_ancestry_with_comparators,
    counterfactual_sensitivity, counterfactual_sensitivity_with_comparators,
};
use crate::structural::surgery::{
    EditableCircuit, MarginalDebtLedger, explicit_delay_register_index, port_cone_changed,
    port_is_cone_mature, port_is_primed, port_upstream_cone_and_edges,
};
use crate::substrate::{
    Circuit, NodeId, Primitive, SignalSink, SignalSource, Width, WireDelayMode,
};

/// Telemetry record capturing residual entropy and diagnostic mutual information across growth epochs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GrowthEpochSnapshot {
    /// Output port for which this snapshot was captured.
    #[serde(default)]
    pub port: u16,
    /// Global evaluation tick at which this snapshot was captured.
    pub tick: u64,
    /// Sequential epoch counter (0 = pre-sprout 1, 1 = post-sprout 1 / pre-sprout 2, 2 = post-sprout 2).
    pub epoch: usize,
    /// Prequential KT residual loss over the diagnostic window (bits per symbol).
    pub kt_loss_bits_per_symbol: f64,
    /// Empirical residual entropy $H(R)$ in bits over the diagnostic window.
    pub residual_entropy_bits: f64,
    /// Diagnostic plug-in mutual information $\hat{I}(R; U^*)$ with the selected candidate feature, if applicable.
    pub best_feature_mi_bits: Option<f64>,
    /// Active circuit node count.
    pub node_count: usize,
}

/// Developmental state maintained independently for each output port (§2.4).
#[derive(Clone, Debug)]
pub struct PortDevelopmentalState {
    pub diagnostic_buffer: DiagnosticBuffer,
    pub ticks_since_last_check: u64,
    pub port_sprout_events: u64,
    pub epoch_snapshots: Vec<GrowthEpochSnapshot>,
    pub replay_ticks_consumed: u64,
    pub stage2_checks_completed: u64,
    pub stage2_mature_at_tick: u64,
    pub stage2_exhausted: bool,
    pub stage3_exhausted: bool,
    pub scored_last_observe: bool,
}

impl PortDevelopmentalState {
    #[must_use]
    pub fn new(diagnostic_window: usize) -> Self {
        Self {
            diagnostic_buffer: DiagnosticBuffer::new(diagnostic_window),
            ticks_since_last_check: 0,
            port_sprout_events: 0,
            epoch_snapshots: Vec::new(),
            replay_ticks_consumed: 0,
            stage2_checks_completed: 0,
            stage2_mature_at_tick: 0,
            stage2_exhausted: false,
            stage3_exhausted: false,
            scored_last_observe: false,
        }
    }
}

/// Summary report returned by [`ArkheLearner::run_offline_loopback`] (`M2B-D3`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OfflineLoopbackReport {
    /// Total unsupervised loopback sleep ticks executed.
    pub ticks: u64,
    /// Comparator agreement ticks (`eqz == 1`) observed during this loopback run.
    pub comparator_agreement_ticks: u64,
    /// Comparator disagreement ticks (`eqz == 0`) observed during this loopback run.
    pub comparator_disagreement_ticks: u64,
    /// Cumulative net comparator flux (in bits) accumulated during this loopback run.
    pub sleep_comparator_flux: f64,
    /// Maximum circular distance $d_{\mathbb{Z}_{2^k}}(\text{primary}, \text{alt})$ seen on any comparator.
    pub max_circular_discrepancy: u64,
    /// Number of minority faulty branches pruned during 3-route loopback consensus.
    pub minority_routes_pruned: u64,
    /// Whether any comparator disagreement was observed during this loopback run.
    pub contradiction_detected: bool,
}

#[derive(Clone, Debug)]
struct ComparatorWakeWindow {
    sub_node: NodeId,
    capacity: usize,
    disagree: VecDeque<bool>,
    r_primary: VecDeque<f64>,
    r_alt: VecDeque<f64>,
    e_primary: VecDeque<bool>,
    e_alt: VecDeque<bool>,
}

impl ComparatorWakeWindow {
    fn new(sub_node: NodeId, capacity: usize) -> Self {
        Self {
            sub_node,
            capacity,
            disagree: VecDeque::with_capacity(capacity),
            r_primary: VecDeque::with_capacity(capacity),
            r_alt: VecDeque::with_capacity(capacity),
            e_primary: VecDeque::with_capacity(capacity),
            e_alt: VecDeque::with_capacity(capacity),
        }
    }

    fn clear(&mut self) {
        self.disagree.clear();
        self.r_primary.clear();
        self.r_alt.clear();
        self.e_primary.clear();
        self.e_alt.clear();
    }

    fn is_full(&self) -> bool {
        self.disagree.len() >= self.capacity
    }

    fn push(&mut self, disagree: bool, r_primary: f64, r_alt: f64, e_primary: bool, e_alt: bool) {
        if self.capacity == 0 {
            return;
        }
        if self.disagree.len() >= self.capacity {
            self.disagree.pop_front();
            self.r_primary.pop_front();
            self.r_alt.pop_front();
            self.e_primary.pop_front();
            self.e_alt.pop_front();
        }
        self.disagree.push_back(disagree);
        self.r_primary.push_back(r_primary);
        self.r_alt.push_back(r_alt);
        self.e_primary.push_back(e_primary);
        self.e_alt.push_back(e_alt);
    }

    fn swap_primary_alt(&mut self) {
        std::mem::swap(&mut self.r_primary, &mut self.r_alt);
        std::mem::swap(&mut self.e_primary, &mut self.e_alt);
    }

    fn disagreement_count(&self) -> usize {
        self.disagree.iter().filter(|&&d| d).count()
    }

    fn flux_primary(&self) -> f64 {
        self.r_primary.iter().sum()
    }

    fn flux_alt(&self) -> f64 {
        self.r_alt.iter().sum()
    }

    fn disagreement_count_tail(&self, w: usize) -> usize {
        let start = self.disagree.len().saturating_sub(w);
        self.disagree.iter().skip(start).filter(|&&d| d).count()
    }

    fn e_primary_count_tail(&self, w: usize) -> usize {
        let start = self.e_primary.len().saturating_sub(w);
        self.e_primary.iter().skip(start).filter(|&&e| e).count()
    }

    fn e_alt_count_tail(&self, w: usize) -> usize {
        let start = self.e_alt.len().saturating_sub(w);
        self.e_alt.iter().skip(start).filter(|&&e| e).count()
    }

    fn flux_primary_tail(&self, w: usize) -> f64 {
        let start = self.r_primary.len().saturating_sub(w);
        self.r_primary.iter().skip(start).sum()
    }

    fn flux_alt_tail(&self, w: usize) -> f64 {
        let start = self.r_alt.len().saturating_sub(w);
        self.r_alt.iter().skip(start).sum()
    }
}

#[derive(Clone)]
enum PendingSnapshot {
    Base {
        ancestry: CausalAncestry,
        snapshot: SensitivitySnapshot,
    },
    WithComparators {
        ancestry: ComparatorCausalAncestry,
        snapshot: ComparatorSensitivitySnapshot,
    },
}

/// Mode governing maturation and description-debt billing for externally injected edits (`M3-E3`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InjectionMode {
    /// Injected nodes enter mature (`juvenile_ticks = 0`, `debt_paid = true`,
    /// `energy = initial_energy`), and $L(C_{\text{post}}) - L(C_{\text{pre}})$ is credited
    /// to `exogenous_injection_bits`.
    Established,
    /// Injected nodes enter as juveniles (`debt_paid = false`), charged marginal debt via
    /// [`MarginalDebtLedger::on_sprout`], leaving `exogenous_injection_bits` unchanged.
    Juvenile,
}

type PendingPrediction = (Vec<u64>, PendingSnapshot, u64, Vec<u64>, Vec<u64>);

/// Online gradient-free closed-loop learner driving coordinate-free structural neurogenesis and metabolic economy.
#[derive(Clone)]
pub struct ArkheLearner {
    editable: EditableCircuit,
    ledger: MarginalDebtLedger,
    economy_config: EconomyConfig,
    growth_config: GrowthConfig,
    seed_bank: SeedBank,
    sample_count: usize,
    tick: u64,
    prev_observed_frame: Vec<u64>,
    pending_prediction: Option<PendingPrediction>,
    obs_window: ObservationWindow,
    port_states: Vec<PortDevelopmentalState>,
    comparator_windows: Vec<ComparatorWakeWindow>,
    sprout_events: u64,
    starvation_prune_events: u64,
    port_starvation_prune_events: Vec<u64>,
    comparator_starvation_prune_events: u64,
    idle_prune_events: u64,
    local_excision_events: usize,
    exogenous_injection_bits: f64,
    ring_closure_events: u64,
    redundancy_sprout_events: u64,
    driver_handoff_events: u64,
    disagreement_prune_events: u64,
    cumulative_net_flux: f64,
    cumulative_debt_paid: f64,
    reserve_clipped_count: u64,
    reserve_clipped_bits: f64,
    reserve_mismatch_floored: u64,
    sprout_blocked_max_nodes: u64,
    candidate_undecidable: u64,
    candidate_list_truncated: u64,
    lag_horizon_bound: u64,
    ring_period_bound: u64,
    lag_deepening_steps: u64,
    sprout_budget_exhausted: u64,
    total_budget_seen: Option<u64>,
    injected_detour_nodes: BTreeSet<NodeId>,
}

impl ArkheLearner {
    /// Creates a deterministic learner in coordinate-free mode.
    ///
    /// # Errors
    ///
    /// Returns [`GrowthError::InvalidConfig`] if `growth` limits are invalid or substrate creation fails.
    pub fn new(
        width: Width,
        channels: u16,
        wire_delay: WireDelayMode,
        growth: GrowthConfig,
        seeds: SeedBank,
    ) -> Result<Self, GrowthError> {
        Self::new_with_config(
            width,
            channels,
            wire_delay,
            EconomyConfig::default(),
            growth,
            seeds,
        )
    }

    /// Full constructor supporting custom economy configuration.
    ///
    /// # Errors
    ///
    /// Returns [`GrowthError::InvalidConfig`] if configuration validation or circuit initialization fails.
    pub fn new_with_config(
        width: Width,
        channels: u16,
        wire_delay: WireDelayMode,
        economy_config: EconomyConfig,
        growth_config: GrowthConfig,
        seed_bank: SeedBank,
    ) -> Result<Self, GrowthError> {
        if !growth_config.is_valid() {
            return Err(GrowthError::InvalidConfig);
        }
        let circuit = Circuit::new(width, channels, channels, wire_delay);
        let editable = EditableCircuit::new_with_growth(circuit, economy_config, &growth_config)
            .map_err(|_| GrowthError::InvalidConfig)?;

        let channels_usize = usize::from(channels);
        let obs_capacity =
            crate::structural::growth::observation_horizon(&economy_config, &growth_config);

        let port_states = (0..channels_usize)
            .map(|_| PortDevelopmentalState::new(growth_config.diagnostic_window))
            .collect();

        Ok(Self {
            editable,
            ledger: MarginalDebtLedger::new(),
            economy_config,
            growth_config,
            seed_bank,
            sample_count: growth_config.sensitivity_sample_count,
            tick: 0,
            prev_observed_frame: vec![0; channels_usize],
            pending_prediction: None,
            obs_window: ObservationWindow::new(channels, obs_capacity),
            port_states,
            comparator_windows: Vec::new(),
            sprout_events: 0,
            starvation_prune_events: 0,
            port_starvation_prune_events: vec![0; channels_usize],
            comparator_starvation_prune_events: 0,
            idle_prune_events: 0,
            local_excision_events: 0,
            exogenous_injection_bits: 0.0,
            ring_closure_events: 0,
            redundancy_sprout_events: 0,
            driver_handoff_events: 0,
            disagreement_prune_events: 0,
            cumulative_net_flux: 0.0,
            cumulative_debt_paid: 0.0,
            reserve_clipped_count: 0,
            reserve_clipped_bits: 0.0,
            reserve_mismatch_floored: 0,
            sprout_blocked_max_nodes: 0,
            candidate_undecidable: 0,
            candidate_list_truncated: 0,
            lag_horizon_bound: 0,
            ring_period_bound: 0,
            lag_deepening_steps: 0,
            sprout_budget_exhausted: 0,
            total_budget_seen: None,
            injected_detour_nodes: BTreeSet::new(),
        })
    }

    /// Constructs a learner around a pre-populated [`EditableCircuit`] (e.g., for loopback or handoff tests).
    ///
    /// # Errors
    ///
    /// Returns [`GrowthError::InvalidConfig`] if `growth_config` limits are invalid.
    pub fn from_editable(
        editable: EditableCircuit,
        growth_config: GrowthConfig,
        seed_bank: SeedBank,
    ) -> Result<Self, GrowthError> {
        if !growth_config.is_valid() {
            return Err(GrowthError::InvalidConfig);
        }
        let economy_config = editable.economy().config();
        let num_inputs = editable.circuit().num_inputs();
        let num_outputs = usize::from(editable.circuit().num_outputs());
        let obs_capacity =
            crate::structural::growth::observation_horizon(&economy_config, &growth_config);
        let mut ledger = MarginalDebtLedger::new();
        ledger.ensure_capacity(editable.circuit(), economy_config.split_code_tax);
        ledger.last_committed_code_bits =
            circuit_code_bits_with_tax(editable.circuit(), 0, economy_config.split_code_tax);
        for (i, node_econ) in editable.economy().nodes().iter().enumerate() {
            if i < ledger.debt_paid.len() && (node_econ.debt_paid || node_econ.juvenile_ticks == 0)
            {
                ledger.debt_paid[i] = true;
            }
        }
        let port_states = (0..num_outputs)
            .map(|p| {
                let mut state = PortDevelopmentalState::new(growth_config.diagnostic_window);
                let (cone, _) = port_upstream_cone_and_edges(editable.circuit(), p as u16);
                let max_juv = cone
                    .iter()
                    .filter_map(|n| editable.economy().nodes().get(n.index()))
                    .map(|e| u64::from(e.juvenile_ticks))
                    .max()
                    .unwrap_or(0);
                state.stage2_mature_at_tick = max_juv;
                state
            })
            .collect();
        let comp_cap = observation_horizon(&economy_config, &growth_config)
            .max(growth_config.diagnostic_window);
        let comparator_windows = editable
            .economy()
            .comparators()
            .iter()
            .map(|c| ComparatorWakeWindow::new(c.sub_node, comp_cap))
            .collect();
        Ok(Self {
            editable,
            ledger,
            economy_config,
            growth_config,
            seed_bank,
            sample_count: growth_config.sensitivity_sample_count,
            tick: 0,
            prev_observed_frame: vec![0; usize::from(num_inputs)],
            pending_prediction: None,
            obs_window: ObservationWindow::new(num_inputs, obs_capacity),
            port_states,
            comparator_windows,
            sprout_events: 0,
            starvation_prune_events: 0,
            port_starvation_prune_events: vec![0; num_outputs],
            comparator_starvation_prune_events: 0,
            idle_prune_events: 0,
            local_excision_events: 0,
            exogenous_injection_bits: 0.0,
            ring_closure_events: 0,
            redundancy_sprout_events: 0,
            driver_handoff_events: 0,
            disagreement_prune_events: 0,
            cumulative_net_flux: 0.0,
            cumulative_debt_paid: 0.0,
            reserve_clipped_count: 0,
            reserve_clipped_bits: 0.0,
            reserve_mismatch_floored: 0,
            sprout_blocked_max_nodes: 0,
            candidate_undecidable: 0,
            candidate_list_truncated: 0,
            lag_horizon_bound: 0,
            ring_period_bound: 0,
            lag_deepening_steps: 0,
            sprout_budget_exhausted: 0,
            total_budget_seen: None,
            injected_detour_nodes: BTreeSet::new(),
        })
    }

    /// Sets the normalized previous observed input frame for closed-loop stepping.
    pub fn set_prev_observed_frame(&mut self, frame: &[u64]) {
        let width = self.editable.circuit().width();
        for (slot, &val) in self.prev_observed_frame.iter_mut().zip(frame) {
            *slot = width.normalize(val);
        }
    }

    /// Active port developmental states.
    #[must_use]
    pub fn port_states(&self) -> &[PortDevelopmentalState] {
        &self.port_states
    }

    /// Whether output `port` was driven with non-zero causal ancestry and scored
    /// during the most recent [`ArkheLearner::observe`] call.
    #[must_use]
    pub fn port_scored_last_observe(&self, port: usize) -> bool {
        self.port_states
            .get(port)
            .is_some_and(|state| state.scored_last_observe)
    }

    /// Primary port 0 diagnostic buffer (for telemetry and backward-compatible access).
    #[must_use]
    pub fn post_juvenile_diag(&self) -> &DiagnosticBuffer {
        &self.port_states[0].diagnostic_buffer
    }

    /// Current editable circuit.
    #[must_use]
    pub const fn editable(&self) -> &EditableCircuit {
        &self.editable
    }

    /// Mutable reference to current editable circuit.
    pub fn editable_mut(&mut self) -> &mut EditableCircuit {
        &mut self.editable
    }

    /// Configured growth limits.
    #[must_use]
    pub const fn growth_config(&self) -> &GrowthConfig {
        &self.growth_config
    }

    /// Updates the wake structural excision mode without modifying any other configuration or state.
    pub fn set_excision_mode(&mut self, mode: ExcisionMode) {
        self.growth_config.excision_mode = mode;
    }

    /// Whether developmental growth on all ports has settled (`tick >= diagnostic_window`,
    /// no active Stage 2 or Stage 3 needs on any port, and every node is mature).
    #[must_use]
    pub fn is_developmental_settled(&self) -> bool {
        self.tick >= self.growth_config.diagnostic_window as u64
            && (0..self.port_states.len())
                .all(|q| !self.port_needs_stage2(q) && !self.port_needs_stage3(q))
            && self
                .editable
                .economy()
                .nodes()
                .iter()
                .all(|n| n.juvenile_ticks == 0)
    }

    /// Configured economy parameters.
    #[must_use]
    pub const fn economy_config(&self) -> &EconomyConfig {
        &self.economy_config
    }

    /// Total sprout transactions committed.
    #[must_use]
    pub const fn sprout_events(&self) -> u64 {
        self.sprout_events
    }

    /// Total starvation prunings committed.
    #[must_use]
    pub const fn starvation_prune_events(&self) -> u64 {
        self.starvation_prune_events
    }

    /// Per-output-port starvation prunings where a starved node lay in `BoundaryOut(p)`'s backward
    /// cone or in a comparator cone targeting port `p` (`M2B-B2`).
    #[must_use]
    pub fn port_starvation_prune_events(&self) -> &[u64] {
        &self.port_starvation_prune_events
    }

    /// Total starvation prunings hitting a registered comparator cone (`sub_node`, `eqz_node`, or
    /// upstream branch) (`M2B-B2`).
    #[must_use]
    pub const fn comparator_starvation_prune_events(&self) -> u64 {
        self.comparator_starvation_prune_events
    }

    /// Total causal zero-sensitivity idle prunings committed.
    #[must_use]
    pub const fn idle_prune_events(&self) -> u64 {
        self.idle_prune_events
    }

    /// Total `M3-E2` verified local pass-through excisions committed.
    #[must_use]
    pub const fn local_excision_events(&self) -> usize {
        self.local_excision_events
    }

    /// Cumulative exogenous description debt bits prepaid by `InjectionMode::Established` edits.
    #[must_use]
    pub const fn exogenous_injection_bits(&self) -> f64 {
        self.exogenous_injection_bits
    }

    /// Applies an external structural edit (`M3-E3`) preserving surviving register states and node
    /// economies via `old_to_new`, billing injected nodes according to `mode`.
    ///
    /// # Errors
    ///
    /// Returns [`GrowthError::InvalidConfig`] if `old_to_new` or `injected` are malformed or
    /// `edited` fails substrate invariant compilation.
    pub fn apply_external_edit(
        &mut self,
        edited: Circuit,
        old_to_new: &[Option<NodeId>],
        injected: &[NodeId],
        mode: InjectionMode,
    ) -> Result<(), GrowthError> {
        let n_post = edited.node_count();
        let mut is_surviving = vec![false; n_post];
        for mapped in old_to_new.iter().flatten() {
            let idx = mapped.index();
            if idx >= n_post || is_surviving[idx] {
                return Err(GrowthError::InvalidConfig);
            }
            is_surviving[idx] = true;
        }
        for &inj in injected {
            let idx = inj.index();
            if idx >= n_post || is_surviving[idx] {
                return Err(GrowthError::InvalidConfig);
            }
        }

        let split_tax = self.economy_config.split_code_tax;
        let pre_circuit = self.editable.circuit().clone();
        let l_pre = circuit_code_bits_with_tax(&pre_circuit, 0, split_tax);
        let prev_debts = self.ledger.node_debts.clone();
        let prev_paid = self.ledger.debt_paid.clone();

        self.editable
            .recompile_preserving_state(edited, old_to_new)
            .map_err(|_| GrowthError::InvalidConfig)?;

        let l_post = circuit_code_bits_with_tax(self.editable.circuit(), 0, split_tax);
        let post_circuit = self.editable.circuit().clone();

        match mode {
            InjectionMode::Established => {
                let mut next_debts = vec![0.0; n_post];
                let mut next_paid = vec![true; n_post];
                for (old_idx, new_opt) in old_to_new.iter().enumerate() {
                    if let Some(new_id) = new_opt {
                        let idx = new_id.index();
                        if old_idx < prev_debts.len() {
                            next_debts[idx] = prev_debts[old_idx];
                            next_paid[idx] = prev_paid[old_idx];
                        }
                    }
                }
                self.ledger.node_debts = next_debts;
                self.ledger.debt_paid = next_paid;
                self.ledger.last_committed_code_bits = l_post;

                let initial_energy = self.economy_config.initial_energy;
                let econ_nodes = self.editable.economy_mut().nodes_mut();
                for (idx, node_econ) in econ_nodes.iter_mut().enumerate() {
                    if !is_surviving[idx] {
                        node_econ.juvenile_ticks = 0;
                        node_econ.debt_paid = true;
                        node_econ.description_debt = 0.0;
                        node_econ.energy = initial_energy;
                    }
                }
                for &inj in injected {
                    let idx = inj.index();
                    self.ledger.debt_paid[idx] = true;
                    if let Some(node_econ) = econ_nodes.get_mut(idx) {
                        node_econ.juvenile_ticks = 0;
                        node_econ.debt_paid = true;
                        node_econ.energy = initial_energy;
                    }
                }
                self.exogenous_injection_bits += l_post - l_pre;
            }
            InjectionMode::Juvenile => {
                self.ledger.last_committed_code_bits = l_pre;
                self.ledger.on_sprout(&post_circuit, split_tax, old_to_new);

                let max_depth = (0..n_post)
                    .filter(|&i| !is_surviving[i])
                    .map(|i| {
                        injected_subgraph_register_depth(
                            &post_circuit,
                            &is_surviving,
                            NodeId(u32::try_from(i).expect("fits in u32")),
                        )
                    })
                    .max()
                    .unwrap_or(0);
                let max_juv = u32::try_from(
                    observation_horizon(&self.economy_config, &self.growth_config)
                        .saturating_mul(2),
                )
                .unwrap_or(u32::MAX);
                let eff_juv =
                    effective_juvenile_ticks(max_depth, &self.economy_config, &self.growth_config)
                        .min(max_juv);
                let initial_energy = self.economy_config.initial_energy;
                let econ_nodes = self.editable.economy_mut().nodes_mut();
                for (idx, node_econ) in econ_nodes.iter_mut().enumerate() {
                    if idx < self.ledger.node_debts.len() {
                        node_econ.description_debt = self.ledger.node_debts[idx];
                        node_econ.debt_paid = self.ledger.debt_paid[idx];
                    }
                    if !is_surviving[idx] {
                        node_econ.juvenile_ticks = eff_juv;
                        node_econ.debt_paid = false;
                        node_econ.energy = initial_energy;
                        if idx < self.ledger.debt_paid.len() {
                            self.ledger.debt_paid[idx] = false;
                        }
                    }
                }
            }
        }

        // In TicksPerHop, pre-settle newly injected Delay-free constant subgraphs (e.g. ConstLoop(1/3)).
        if post_circuit.wire_delay() == WireDelayMode::TicksPerHop {
            settle_injected_tph_constant_subgraphs(&mut self.editable, &is_surviving);
        }

        for q in 0..self.port_states.len() {
            if self.is_port_driven(q as u16) {
                self.port_states[q].stage2_exhausted = true;
                self.port_states[q].stage3_exhausted = true;
            }
            self.port_states[q].diagnostic_buffer.clear();
            self.port_states[q].ticks_since_last_check = 0;
        }
        self.apply_node_remap_to_windows(old_to_new);
        self.injected_detour_nodes.extend(injected.iter().copied());
        for win in &mut self.comparator_windows {
            win.clear();
        }
        Ok(())
    }

    /// Number of externally injected detour nodes still present in the live circuit.
    #[must_use]
    pub fn surviving_injected_detour_nodes(&self) -> usize {
        self.injected_detour_nodes.len()
    }

    /// Total autonomous ring-closure events committed.
    #[must_use]
    pub const fn ring_closure_events(&self) -> u64 {
        self.ring_closure_events
    }

    /// Total Stage 3 causal-disjoint redundancy sprouts committed.
    #[must_use]
    pub const fn redundancy_sprout_events(&self) -> u64 {
        self.redundancy_sprout_events
    }

    /// Total comparator driver handoff promotions committed in Wake.
    #[must_use]
    pub const fn driver_handoff_events(&self) -> u64 {
        self.driver_handoff_events
    }

    /// Total comparator disagreement refutation prunings committed in Wake.
    #[must_use]
    pub const fn disagreement_prune_events(&self) -> u64 {
        self.disagreement_prune_events
    }

    /// Current number of active registered comparators.
    #[must_use]
    pub fn active_comparator_count(&self) -> usize {
        self.editable.economy().comparators().len()
    }

    /// Cumulative comparator agreement ticks (`eqz == 1`) recorded in the economy.
    #[must_use]
    pub const fn comparator_agreement_ticks(&self) -> u64 {
        self.editable.economy().comparator_agreement_ticks()
    }

    /// Cumulative comparator disagreement ticks (`eqz == 0`) recorded in the economy.
    #[must_use]
    pub const fn comparator_disagreement_ticks(&self) -> u64 {
        self.editable.economy().comparator_disagreement_ticks()
    }

    /// Cumulative net flux (in bits) credited through registered comparators.
    #[must_use]
    pub const fn cumulative_comparator_flux(&self) -> f64 {
        self.editable.economy().cumulative_comparator_flux()
    }

    /// Diagnostic epoch snapshots captured for port 0 across developmental milestones.
    #[must_use]
    pub fn epoch_snapshots(&self) -> &[GrowthEpochSnapshot] {
        self.port_states
            .first()
            .map_or(&[], |state| state.epoch_snapshots.as_slice())
    }

    /// Diagnostic epoch snapshots captured for a specific output port `port`.
    #[must_use]
    pub fn port_epoch_snapshots(&self, port: u16) -> Option<&[GrowthEpochSnapshot]> {
        self.port_states
            .get(usize::from(port))
            .map(|state| state.epoch_snapshots.as_slice())
    }

    fn reset_changed_ports(&mut self, pre_circuit: &Circuit, old_to_new: &[Option<NodeId>]) {
        self.clear_changed_ports_internal(pre_circuit, old_to_new, true);
    }

    fn clear_changed_port_diagnostics_preserving_scorer(
        &mut self,
        pre_circuit: &Circuit,
        old_to_new: &[Option<NodeId>],
    ) {
        self.clear_changed_ports_internal(pre_circuit, old_to_new, false);
    }

    fn clear_changed_ports_internal(
        &mut self,
        pre_circuit: &Circuit,
        old_to_new: &[Option<NodeId>],
        reset_scorer_counts: bool,
    ) {
        let num_ports = self.port_states.len();
        let changed_ports: Vec<usize> = (0..num_ports)
            .filter(|&q| {
                port_cone_changed(pre_circuit, self.editable.circuit(), old_to_new, q as u16)
            })
            .collect();
        for q in changed_ports {
            if reset_scorer_counts {
                self.editable
                    .economy_mut()
                    .port_scorer_mut(q)
                    .reset_predictor_counts();
            }
            let preserve_settlement_flags = !reset_scorer_counts && self.is_port_driven(q as u16);
            self.port_states[q].diagnostic_buffer.clear();
            self.port_states[q].ticks_since_last_check = 0;
            if !preserve_settlement_flags {
                self.port_states[q].stage2_mature_at_tick = 0;
                self.port_states[q].stage2_exhausted = false;
                self.port_states[q].stage3_exhausted = false;
            }
        }
    }

    fn record_epoch_snapshot(&mut self, port_idx: usize, epoch: usize, include_mi: bool) {
        let diag_buf = &self.port_states[port_idx].diagnostic_buffer;
        let residuals: Vec<u64> = diag_buf.residuals.iter().map(|f| f[port_idx]).collect();
        let port_u16 = port_idx as u16;
        let features = diag_buf.build_signal_pool(
            self.editable.circuit(),
            &self.obs_window,
            port_u16,
            self.growth_config.max_reactive_lag,
            Some(self.editable.economy()),
        );
        let Ok(diag_res) = diagnose_residual(
            self.editable.circuit().width(),
            &residuals,
            &features,
            self.growth_config,
        ) else {
            return;
        };
        let mismatches = diag_buf
            .truth
            .iter()
            .zip(&diag_buf.predictions)
            .filter(|(t, p)| t[port_idx] != p[port_idx])
            .count();
        let w = diag_buf.len() as u64;
        let loss =
            facts_code_bits(self.editable.circuit().width(), w, mismatches as u64) / (w as f64);
        let best_feature_mi_bits = if include_mi {
            diag_res
                .best_feature
                .as_ref()
                .map(|f| f.mutual_information_bits)
        } else {
            None
        };
        self.port_states[port_idx]
            .epoch_snapshots
            .push(GrowthEpochSnapshot {
                port: port_u16,
                tick: self.tick,
                epoch,
                kt_loss_bits_per_symbol: loss,
                residual_entropy_bits: diag_res.residual_entropy_bits,
                best_feature_mi_bits,
                node_count: self.editable.circuit().node_count(),
            });
    }

    /// Active marginal debt ledger.
    #[must_use]
    pub const fn ledger(&self) -> &MarginalDebtLedger {
        &self.ledger
    }

    /// Mutable reference to active marginal debt ledger.
    pub fn ledger_mut(&mut self) -> &mut MarginalDebtLedger {
        &mut self.ledger
    }

    fn is_port_zero_error(&self, port_idx: usize) -> bool {
        let buf = &self.port_states[port_idx].diagnostic_buffer;
        let width = self.editable.circuit().width();
        buf.len() >= self.growth_config.diagnostic_window
            && buf
                .residuals
                .iter()
                .all(|frame| width.normalize(frame[port_idx]) == 0)
    }

    fn port_needs_stage2(&self, port_idx: usize) -> bool {
        !self.is_port_zero_error(port_idx) && !self.port_states[port_idx].stage2_exhausted
    }

    fn port_needs_stage3(&self, port_idx: usize) -> bool {
        self.growth_config.redundancy_sprouting
            && !self.port_needs_stage2(port_idx)
            && self.is_port_driven(port_idx as u16)
            && self.port_redundancy_arity_eligible(port_idx as u16)
            && !self.port_states[port_idx].stage3_exhausted
    }

    fn port_budget_allowance(&self, port_idx: usize, global_budget: &TickBudget) -> u64 {
        let num_ports = self.port_states.len();
        if num_ports <= 1 {
            return global_budget.remaining();
        }
        let total_budget = self.total_budget_seen.unwrap_or_else(|| {
            global_budget
                .remaining()
                .saturating_add(global_budget.consumed())
        });
        let base_share = total_budget / (num_ports as u64);
        let mut idle_pool = 0u64;
        let mut stage2_active_ports = 0usize;
        let mut stage3_active_ports = 0usize;
        for q in 0..num_ports {
            if self.port_needs_stage2(q) {
                stage2_active_ports += 1;
            } else if self.port_needs_stage3(q) {
                stage3_active_ports += 1;
            } else {
                idle_pool += base_share.saturating_sub(self.port_states[q].replay_ticks_consumed);
            }
        }
        let extra = if stage2_active_ports > 0 {
            if self.port_needs_stage2(port_idx) {
                idle_pool / (stage2_active_ports as u64)
            } else {
                0
            }
        } else if stage3_active_ports > 0 && self.port_needs_stage3(port_idx) {
            idle_pool / (stage3_active_ports as u64)
        } else {
            0
        };
        let cap = base_share.saturating_add(extra);
        cap.saturating_sub(self.port_states[port_idx].replay_ticks_consumed)
            .min(global_budget.remaining())
    }

    fn commit_sprout(
        &mut self,
        port_idx: usize,
        sprout_circ: Circuit,
        effective_juvenile_ticks: u32,
    ) -> bool {
        let port_sprouts = self.port_states[port_idx].port_sprout_events;
        if port_sprouts == 0 || port_sprouts == 1 {
            self.record_epoch_snapshot(port_idx, port_sprouts as usize, true);
        }
        let pre_sprout_circuit = self.editable.circuit().clone();
        let old_to_new: Vec<Option<NodeId>> = (0..self.editable.circuit().node_count())
            .map(|i| Some(NodeId(i as u32)))
            .collect();
        if self
            .editable
            .recompile_preserving_state(sprout_circ, &old_to_new)
            .is_ok()
        {
            let mut surviving = vec![false; self.editable.circuit().node_count()];
            for new_id in old_to_new.iter().flatten() {
                if let Some(slot) = surviving.get_mut(new_id.index()) {
                    *slot = true;
                }
            }
            for (idx, node) in self
                .editable
                .economy_mut()
                .nodes_mut()
                .iter_mut()
                .enumerate()
            {
                if !surviving[idx] {
                    node.juvenile_ticks = effective_juvenile_ticks;
                }
            }
            self.ledger.on_sprout(
                self.editable.circuit(),
                self.economy_config.split_code_tax,
                &old_to_new,
            );
            self.editable
                .economy_mut()
                .assign_sprout_debts(&self.ledger.node_debts);
            self.reset_changed_ports(&pre_sprout_circuit, &old_to_new);
            self.port_states[port_idx].stage2_mature_at_tick = self
                .tick
                .saturating_add(u64::from(effective_juvenile_ticks));
            self.port_states[port_idx].stage2_exhausted = false;
            self.port_states[port_idx].stage3_exhausted = false;
            self.port_states[port_idx].port_sprout_events += 1;
            self.sprout_events += 1;
            true
        } else {
            false
        }
    }

    fn commit_ring_proposal(&mut self, ring_prop: AutonomousRingProposal) -> bool {
        let pre_ring_circuit = self.editable.circuit().clone();
        let old_to_new = ring_prop.old_to_new;
        let ring_delay_nodes = ring_prop.ring_delay_nodes;
        let period = ring_prop.period;
        let template = ring_prop.template;
        if self
            .editable
            .recompile_preserving_state(ring_prop.candidate_circuit, &old_to_new)
            .is_ok()
        {
            self.ledger.on_sprout(
                self.editable.circuit(),
                self.economy_config.split_code_tax,
                &old_to_new,
            );
            self.editable
                .economy_mut()
                .assign_sprout_debts(&self.ledger.node_debts);
            let mut regs = self.editable.state().registers().to_vec();
            let mut ok_regs = true;
            for (j, &delay_node) in ring_delay_nodes.iter().enumerate() {
                if let Some(reg_idx) =
                    explicit_delay_register_index(self.editable.circuit(), delay_node)
                {
                    regs[reg_idx] =
                        template[(((self.tick + 1) as usize) + (period - 1 - j)) % period];
                } else {
                    ok_regs = false;
                    break;
                }
            }
            if ok_regs {
                let _ = self.editable.set_registers(&regs);
            }
            self.reset_changed_ports(&pre_ring_circuit, &old_to_new);
            self.ring_closure_events += 1;
            true
        } else {
            false
        }
    }

    /// Assembles current M1 telemetry summary.
    #[must_use]
    pub fn telemetry(&self) -> ArkheM1Telemetry {
        let final_node_count = self.editable.circuit().node_count();
        let final_split_count = self
            .editable
            .circuit()
            .nodes()
            .iter()
            .filter(|&&p| p == Primitive::Split)
            .count();
        let final_delay_count = self
            .editable
            .circuit()
            .nodes()
            .iter()
            .filter(|&&p| matches!(p, Primitive::Delay { .. }))
            .count();
        let final_total_energy: f64 = self
            .editable
            .economy()
            .nodes()
            .iter()
            .map(|n| n.energy)
            .sum();
        let min_mature_node_energy = self
            .editable
            .economy()
            .nodes()
            .iter()
            .filter(|n| n.juvenile_ticks == 0)
            .map(|n| n.energy)
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let code_bits = circuit_code_bits_with_tax(
            self.editable.circuit(),
            0,
            self.economy_config.split_code_tax,
        );
        let null_calibrated_mdl_delta_bits = code_bits - self.cumulative_net_flux;

        ArkheM1Telemetry {
            final_node_count,
            final_split_count,
            final_delay_count,
            final_total_energy,
            min_mature_node_energy,
            cumulative_net_flux: self.cumulative_net_flux,
            cumulative_maturity_debt_paid: self.cumulative_debt_paid,
            null_calibrated_mdl_delta_bits,
            sprout_events: self.sprout_events,
            ring_closure_events: self.ring_closure_events,
            starvation_prune_events: self.starvation_prune_events,
            idle_prune_events: self.idle_prune_events,
            reserve_clipped_count: self.reserve_clipped_count,
            reserve_clipped_bits: self.reserve_clipped_bits,
            sprout_blocked_max_nodes: self.sprout_blocked_max_nodes,
            candidate_undecidable: self.candidate_undecidable,
            candidate_list_truncated: self.candidate_list_truncated,
            lag_horizon_bound: self.lag_horizon_bound,
            ring_period_bound: self.ring_period_bound,
            reserve_mismatch_floored: self.reserve_mismatch_floored,
            lag_deepening_steps: self.lag_deepening_steps,
            sprout_budget_exhausted: self.sprout_budget_exhausted,
            redundancy_sprout_events: self.redundancy_sprout_events,
            active_comparator_count: self.active_comparator_count(),
            comparator_agreement_ticks: self.comparator_agreement_ticks(),
            comparator_disagreement_ticks: self.comparator_disagreement_ticks(),
            cumulative_comparator_flux: self.cumulative_comparator_flux(),
            driver_handoff_events: self.driver_handoff_events,
            disagreement_prune_events: self.disagreement_prune_events,
            local_excision_events: self.local_excision_events,
            exogenous_injection_bits: self.exogenous_injection_bits,
        }
    }

    fn sync_comparator_windows(&mut self) {
        let diag_cap = observation_horizon(&self.economy_config, &self.growth_config)
            .max(self.growth_config.diagnostic_window);
        let comps = self.editable.economy().comparators();
        let existing = std::mem::take(&mut self.comparator_windows);
        let mut synced = Vec::with_capacity(comps.len());
        for comp in comps {
            if let Some(win) = existing.iter().find(|w| w.sub_node == comp.sub_node) {
                synced.push(win.clone());
            } else {
                synced.push(ComparatorWakeWindow::new(comp.sub_node, diag_cap));
            }
        }
        self.comparator_windows = synced;
    }

    fn apply_node_remap_to_windows(&mut self, old_to_new: &[Option<NodeId>]) {
        self.injected_detour_nodes = self
            .injected_detour_nodes
            .iter()
            .filter_map(|id| old_to_new.get(id.index()).copied().flatten())
            .collect();
        let new_node_count = self.editable.circuit().node_count();
        for state in &mut self.port_states {
            state
                .diagnostic_buffer
                .remap_nodes(old_to_new, new_node_count);
        }
        for win in &mut self.comparator_windows {
            if let Some(Some(mapped)) = old_to_new.get(win.sub_node.index()) {
                win.sub_node = *mapped;
            } else {
                win.sub_node = NodeId(u32::MAX);
            }
        }
        self.sync_comparator_windows();
    }

    fn is_port_driven(&self, target_port: u16) -> bool {
        self.editable
            .circuit()
            .edges()
            .iter()
            .any(|e| e.dst == SignalSink::BoundaryOut(target_port))
    }

    fn port_comparators_primed(
        &self,
        comp_ancestry: Option<&ComparatorCausalAncestry>,
        target_port: u16,
    ) -> bool {
        let comps = self.editable.economy().comparators();
        let mut any_on_port = false;
        let mut all_primed = true;
        for (m, comp) in comps.iter().enumerate() {
            if comp.target_port == target_port {
                any_on_port = true;
                let primed = comp_ancestry
                    .and_then(|a| a.comparator_shares.get(m))
                    .is_some_and(|shares| shares.iter().sum::<f64>() > 0.0);
                if !primed {
                    all_primed = false;
                    break;
                }
            }
        }
        !any_on_port || all_primed
    }

    fn port_q_hat_exceeds_null(&self, port_idx: usize, prediction: u64) -> bool {
        let scorer = &self.editable.economy().scorers()[port_idx];
        let pred_norm = self.editable.circuit().width().normalize(prediction);
        let p_null = scorer.null_model().probability(pred_norm);
        scorer.q_hat() > p_null
    }

    fn port_redundancy_arity_eligible(&self, target_port: u16) -> bool {
        let comps = self.editable.economy().comparators();
        let port_comp_indices: Vec<usize> = comps
            .iter()
            .enumerate()
            .filter_map(|(m, c)| (c.target_port == target_port).then_some(m))
            .collect();
        match port_comp_indices.len() {
            0 => true,
            1 => {
                let m = port_comp_indices[0];
                let min_gain = self.growth_config.min_match_gain;
                self.comparator_windows.get(m).is_some_and(|w| {
                    w.is_full()
                        && w.disagreement_count() >= min_gain
                        && w.disagreement_count().saturating_mul(2) >= w.disagree.len()
                })
            }
            _ => false,
        }
    }

    fn sibling_has_juvenile_nodes(&self, port_idx: usize) -> bool {
        let num_ports = self.port_states.len();
        let circuit = self.editable.circuit();
        let economy = self.editable.economy();
        for q in 0..num_ports {
            if q == port_idx {
                continue;
            }
            if !port_primary_cone_is_mature(circuit, economy, q as u16)
                && (self.port_states[q].port_sprout_events == 0
                    || self.tick < self.port_states[q].stage2_mature_at_tick)
            {
                return true;
            }
        }
        false
    }

    fn swap_port_and_comparator_scorer(&mut self, port_idx: usize, comp_idx: usize) {
        let width = self.editable.circuit().width();
        let null_kind = self.economy_config.null_model;
        let econ = self.editable.economy_mut();
        let port_scorer = std::mem::replace(
            econ.port_scorer_mut(port_idx),
            crate::metrics::PointForecastScorer::new(width, null_kind),
        );
        let comp_scorer =
            std::mem::replace(&mut econ.comparators_mut()[comp_idx].scorer, port_scorer);
        *econ.port_scorer_mut(port_idx) = comp_scorer;
    }

    fn commit_redundancy_sprout(
        &mut self,
        port_idx: usize,
        proposal: RedundancySproutProposal,
    ) -> bool {
        let pre_epoch = self.editable.structure_epoch();
        let pre_circuit = self.editable.circuit().clone();
        let Ok(pre_compiled) = pre_circuit.compile() else {
            return false;
        };
        let pre_regs = self.editable.state().registers().to_vec();
        let old_to_new = proposal.old_to_new;
        let sub_node = proposal.sub_node;
        let eqz_node = proposal.eqz_node;
        let eff_juv = proposal.effective_juvenile_ticks;
        let primary_mask = proposal.primary_channel_mask;
        let alt_mask = proposal.alt_channel_mask;
        let autonomous_ring = proposal.autonomous_ring;
        let primary_tph_split = proposal.primary_tph_split;

        if self
            .editable
            .recompile_preserving_state(proposal.circuit, &old_to_new)
            .is_err()
        {
            return false;
        }
        self.ledger.on_sprout(
            self.editable.circuit(),
            self.economy_config.split_code_tax,
            &old_to_new,
        );
        self.editable
            .economy_mut()
            .assign_sprout_debts(&self.ledger.node_debts);

        let mut surviving = vec![false; self.editable.circuit().node_count()];
        for new_id in old_to_new.iter().flatten() {
            if let Some(slot) = surviving.get_mut(new_id.index()) {
                *slot = true;
            }
        }
        {
            let circuit = self.editable.circuit().clone();
            assign_stage3_juvenile_ticks(
                &circuit,
                self.editable.economy_mut(),
                &surviving,
                eff_juv,
                &self.economy_config,
                &self.growth_config,
            );
        }

        if let Some(split_node) = primary_tph_split
            && let Some(adj_regs) = advance_tph_autonomous_ring_phase(
                &pre_circuit,
                &pre_compiled,
                &pre_regs,
                self.editable.circuit(),
                self.editable.compiled(),
                &old_to_new,
                port_idx as u16,
                split_node,
                sub_node,
            )
        {
            let _ = self.editable.set_registers(&adj_regs);
        }

        // Prime newly added non-autonomous registers across recent obs_window frames so reactive
        // Stage 3 derivations begin flush-aligned without spurious init=0 startup disagreements.
        if !self.obs_window.is_empty() {
            let reg_map = crate::structural::surgery::register_mapping(
                &pre_circuit,
                self.editable.circuit(),
                &pre_compiled,
                self.editable.compiled(),
                &old_to_new,
            );
            let total_new_regs = self.editable.compiled().total_registers();
            let mut is_surviving_reg = vec![false; total_new_regs];
            for nr in reg_map.iter().flatten().copied() {
                if nr < is_surviving_reg.len() {
                    is_surviving_reg[nr] = true;
                }
            }
            if is_surviving_reg.iter().any(|&s| !s) {
                let compiled = self.editable.compiled();
                let mut shadow = crate::substrate::ScalarState::new(compiled);
                let w_replay = self
                    .obs_window
                    .len()
                    .min(2 * self.growth_config.diagnostic_window);
                let start_idx = self.obs_window.len() - w_replay;
                if let Some((snap_epoch, snap_regs)) = self.obs_window.state_snapshot_at(start_idx)
                    && snap_epoch == pre_epoch
                {
                    let mut shadow_regs = shadow.registers().to_vec();
                    for (old_r, new_r) in reg_map.iter().enumerate() {
                        if let Some(nr) = new_r
                            && old_r < snap_regs.len()
                            && *nr < shadow_regs.len()
                        {
                            shadow_regs[*nr] = snap_regs[old_r];
                        }
                    }
                    let _ = shadow.set_registers(compiled, &shadow_regs);
                }
                let mut dummy_pred = vec![0u64; usize::from(compiled.num_outputs())];
                for idx in start_idx..self.obs_window.len() {
                    let input = if idx == 0 {
                        self.obs_window.prior_frame()
                    } else {
                        &self.obs_window.frames()[idx - 1]
                    };
                    let _ = shadow.step(compiled, input, &mut dummy_pred);
                }
                let mut live_regs = self.editable.state().registers().to_vec();
                let shadow_regs = shadow.registers();
                for (r, surviving_reg) in is_surviving_reg.iter().enumerate() {
                    if !*surviving_reg && r < live_regs.len() && r < shadow_regs.len() {
                        live_regs[r] = shadow_regs[r];
                    }
                }
                let _ = self.editable.set_registers(&live_regs);
            }
        }

        if let Some((period, ref template, ref delay_nodes)) = autonomous_ring {
            let mut regs = self.editable.state().registers().to_vec();
            let mut ok_regs = true;
            for (j, &delay_node) in delay_nodes.iter().enumerate() {
                if let Some(reg_idx) =
                    explicit_delay_register_index(self.editable.circuit(), delay_node)
                {
                    regs[reg_idx] =
                        template[(((self.tick + 1) as usize) + (period - 1 - j)) % period];
                } else {
                    ok_regs = false;
                    break;
                }
            }
            if ok_regs {
                let _ = self.editable.set_registers(&regs);
            }
        }

        if self
            .editable
            .register_comparator(sub_node, eqz_node, port_idx as u16, primary_mask, alt_mask)
            .is_err()
        {
            return false;
        }
        self.sync_comparator_windows();
        self.port_states[port_idx].diagnostic_buffer.clear();
        self.port_states[port_idx].ticks_since_last_check = 0;
        // Keep Pass 1 active when an autonomous ring comparator is committed so
        // `evaluate_comparator_ring_promotion` can promote it; non-ring Stage-3
        // comparators leave Stage 2 exhausted (`#15.1`).
        self.port_states[port_idx].stage2_exhausted = autonomous_ring.is_none();
        self.port_states[port_idx].stage3_exhausted = false;
        self.redundancy_sprout_events += 1;
        true
    }

    fn excise_port_comparator(&mut self, comp_idx: usize, prune_primary: bool) -> bool {
        let comps = self.editable.economy().comparators();
        if comp_idx >= comps.len() {
            return false;
        }
        let comp = comps[comp_idx].clone();
        let target_port = comp.target_port;
        let target_idx = usize::from(target_port);
        let new_primary_mask = if prune_primary {
            comp.alt_channel_mask
        } else {
            comp.primary_channel_mask
        };
        let target_port_subs: Vec<NodeId> = comps
            .iter()
            .filter(|c| c.target_port == target_port)
            .map(|c| c.sub_node)
            .collect();
        let surviving_comp_nodes: Vec<(NodeId, NodeId)> = comps
            .iter()
            .enumerate()
            .filter_map(|(i, c)| (i != comp_idx).then_some((c.sub_node, c.eqz_node)))
            .collect();

        let Some((pruned, old_to_new)) = excise_comparator_branch(
            self.editable.circuit(),
            comp.sub_node,
            comp.eqz_node,
            target_port,
            prune_primary,
            &target_port_subs,
            &surviving_comp_nodes,
        ) else {
            return false;
        };

        if prune_primary {
            self.swap_port_and_comparator_scorer(target_idx, comp_idx);
            for (idx, other) in self
                .editable
                .economy_mut()
                .comparators_mut()
                .iter_mut()
                .enumerate()
            {
                if idx != comp_idx && other.target_port == target_port {
                    other.primary_channel_mask = new_primary_mask;
                }
            }
        }

        if self
            .editable
            .recompile_preserving_state(pruned, &old_to_new)
            .is_err()
        {
            return false;
        }
        self.ledger.on_prune(
            self.editable.circuit(),
            self.economy_config.split_code_tax,
            &old_to_new,
        );
        self.apply_node_remap_to_windows(&old_to_new);
        if prune_primary {
            let remaining = self.editable.economy().comparators().to_vec();
            for (i, other) in remaining.iter().enumerate() {
                if other.target_port == target_port
                    && let Some(win) = self.comparator_windows.get_mut(i)
                {
                    win.clear();
                }
            }
        }
        self.refresh_stage_gates_after_comparator_transition(target_port, true);
        true
    }

    /// Return whether `target_port` remains driven and its primary scorer still
    /// beats the trailing diagnostic null model before diagnostic buffers reset.
    fn surviving_driver_beats_null(&self, target_port: u16) -> bool {
        let target_idx = usize::from(target_port);
        if !self.is_port_driven(target_port) {
            return false;
        }
        let width = self.editable.circuit().width();
        let scorer = &self.editable.economy().scorers()[target_idx];
        let p_null = self.port_states[target_idx]
            .diagnostic_buffer
            .predictions
            .last()
            .and_then(|frame| frame.get(target_idx).copied())
            .map_or_else(
                || 1.0 / (2.0f64.powi(i32::from(width.bits()))),
                |pred| scorer.null_model().probability(width.normalize(pred)),
            );
        scorer.q_hat() > p_null
    }

    /// Reset post-comparator stage gates for `target_port`, reopening Stage 2
    /// only when the surviving primary driver is absent or no longer beats null.
    fn refresh_stage_gates_after_comparator_transition(
        &mut self,
        target_port: u16,
        clear_all_diag_buffers: bool,
    ) {
        let beats_null = self.surviving_driver_beats_null(target_port);
        if clear_all_diag_buffers {
            for state in &mut self.port_states {
                state.diagnostic_buffer.clear();
            }
        } else {
            self.port_states[usize::from(target_port)]
                .diagnostic_buffer
                .clear();
        }
        let state = &mut self.port_states[usize::from(target_port)];
        state.ticks_since_last_check = 0;
        if !beats_null {
            state.stage2_exhausted = false;
        }
        state.stage3_exhausted = false;
    }

    fn perform_comparator_handoff(&mut self, comp_idx: usize) -> bool {
        let Some(comp) = self.editable.economy().comparators().get(comp_idx).cloned() else {
            return false;
        };
        let split_tax = self.economy_config.split_code_tax;
        let target_port = comp.target_port;
        let target_idx = usize::from(target_port);
        let target_port_subs: Vec<NodeId> = self
            .editable
            .economy()
            .comparators()
            .iter()
            .filter(|c| c.target_port == target_port)
            .map(|c| c.sub_node)
            .collect();
        let mut swapped = self.editable.circuit().clone();
        if !swap_comparator_driver(&mut swapped, target_port, comp.sub_node, &target_port_subs) {
            return false;
        }
        let pre_n = self.editable.circuit().node_count();
        let old_to_new: Vec<Option<NodeId>> = (0..pre_n).map(|i| Some(NodeId(i as u32))).collect();
        if self
            .editable
            .recompile_preserving_state(swapped, &old_to_new)
            .is_err()
        {
            return false;
        }
        let post_n = self.editable.circuit().node_count();
        if post_n > pre_n {
            self.ledger.node_debts.resize(post_n, 0.0);
            self.ledger.debt_paid.resize(post_n, true);
            for idx in pre_n..post_n {
                if let Some(ne) = self.editable.economy_mut().node_mut(idx) {
                    ne.juvenile_ticks = 0;
                    ne.debt_paid = true;
                    ne.description_debt = 0.0;
                }
            }
        }
        self.ledger.last_committed_code_bits =
            circuit_code_bits_with_tax(self.editable.circuit(), 0, split_tax);
        self.swap_port_and_comparator_scorer(target_idx, comp_idx);
        let new_primary_mask = comp.alt_channel_mask;
        {
            let comps_mut = self.editable.economy_mut().comparators_mut();
            comps_mut[comp_idx].primary_channel_mask = comp.alt_channel_mask;
            comps_mut[comp_idx].alt_channel_mask = comp.primary_channel_mask;
            for (i, other) in comps_mut.iter_mut().enumerate() {
                if i != comp_idx && other.target_port == target_port {
                    other.primary_channel_mask = new_primary_mask;
                }
            }
        }
        if let Some(w) = self.comparator_windows.get_mut(comp_idx) {
            w.swap_primary_alt();
        }
        let sibling_subs: Vec<NodeId> = self
            .editable
            .economy()
            .comparators()
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                (i != comp_idx && c.target_port == target_port).then_some(c.sub_node)
            })
            .collect();
        for w in &mut self.comparator_windows {
            if sibling_subs.contains(&w.sub_node) {
                w.clear();
            }
        }
        self.refresh_stage_gates_after_comparator_transition(target_port, false);
        self.driver_handoff_events += 1;
        true
    }

    fn try_wake_comparator_bankruptcy_refutation(
        &mut self,
        starved_set: &BTreeSet<NodeId>,
    ) -> bool {
        let min_gain = self.growth_config.min_match_gain;
        let w_diag = self.growth_config.diagnostic_window;
        let comps = self.editable.economy().comparators().to_vec();
        for (comp_idx, comp) in comps.iter().enumerate() {
            if !comparator_cone_is_mature(self.editable.circuit(), self.editable.economy(), comp) {
                continue;
            }
            let Some(win) = self.comparator_windows.get(comp_idx) else {
                continue;
            };
            if win.disagree.len() < w_diag || win.disagreement_count_tail(w_diag) < min_gain {
                continue;
            }
            let target_port_subs: Vec<NodeId> = comps
                .iter()
                .filter(|c| c.target_port == comp.target_port)
                .map(|c| c.sub_node)
                .collect();
            let surviving_comp_nodes: Vec<(NodeId, NodeId)> = comps
                .iter()
                .enumerate()
                .filter_map(|(i, c)| (i != comp_idx).then_some((c.sub_node, c.eqz_node)))
                .collect();
            let circuit = self.editable.circuit();
            let Some(pri_excl) = comparator_exclusive_arm_nodes(
                circuit,
                comp.sub_node,
                comp.eqz_node,
                comp.target_port,
                true,
                &target_port_subs,
                &surviving_comp_nodes,
            ) else {
                continue;
            };
            let Some(alt_excl) = comparator_exclusive_arm_nodes(
                circuit,
                comp.sub_node,
                comp.eqz_node,
                comp.target_port,
                false,
                &target_port_subs,
                &surviving_comp_nodes,
            ) else {
                continue;
            };
            let comp_node_starved =
                starved_set.contains(&comp.sub_node) || starved_set.contains(&comp.eqz_node);
            let pri_starved = pri_excl.iter().any(|n| starved_set.contains(n));
            let alt_starved = alt_excl.iter().any(|n| starved_set.contains(n));
            if !(comp_node_starved || pri_starved || alt_starved) {
                continue;
            }
            if pri_starved && alt_starved {
                continue;
            }
            let prune_decision = if !pri_starved && !alt_starved {
                if win.flux_alt() <= 0.0 && win.flux_primary_tail(w_diag) > 0.0 {
                    Some(false)
                } else if win.flux_primary() <= 0.0 && win.flux_alt_tail(w_diag) > 0.0 {
                    Some(true)
                } else {
                    None
                }
            } else if alt_starved {
                (win.flux_alt() <= 0.0 && win.flux_primary_tail(w_diag) > 0.0).then_some(false)
            } else {
                (win.flux_primary() <= 0.0 && win.flux_alt_tail(w_diag) > 0.0).then_some(true)
            };
            if let Some(prune_primary) = prune_decision
                && self.excise_port_comparator(comp_idx, prune_primary)
            {
                self.disagreement_prune_events += 1;
                return true;
            }
        }
        false
    }

    fn record_scoped_starvation_prune(
        &mut self,
        pre_prune_circuit: &Circuit,
        pre_comps: &[ComparatorSite],
        starved_set: &BTreeSet<NodeId>,
    ) {
        let num_ports = self.port_starvation_prune_events.len();
        let mut port_hit = vec![false; num_ports];
        for (p, hit) in port_hit.iter_mut().enumerate() {
            let (cone, _) = port_upstream_cone_and_edges(pre_prune_circuit, p as u16);
            if cone.iter().any(|n| starved_set.contains(n)) {
                *hit = true;
            }
        }
        let mut comp_hit = false;
        for comp in pre_comps {
            let mut visited = BTreeSet::new();
            let mut stack = vec![comp.sub_node, comp.eqz_node];
            let mut hit_this = false;
            while let Some(node) = stack.pop() {
                if !visited.insert(node) {
                    continue;
                }
                if starved_set.contains(&node) {
                    hit_this = true;
                    break;
                }
                for edge in pre_prune_circuit.edges() {
                    if let SignalSink::NodeIn { node: dst, .. } = edge.dst
                        && dst == node
                        && let SignalSource::NodeOut { node: src, .. } = edge.src
                    {
                        stack.push(src);
                    }
                }
            }
            if hit_this {
                comp_hit = true;
                let tp = usize::from(comp.target_port);
                if tp < port_hit.len() {
                    port_hit[tp] = true;
                }
            }
        }
        for (p, &hit) in port_hit.iter().enumerate() {
            if hit {
                self.port_starvation_prune_events[p] += 1;
            }
        }
        if comp_hit {
            self.comparator_starvation_prune_events += 1;
        }
    }

    fn process_wake_comparator_events(&mut self) -> bool {
        let min_gain = self.growth_config.min_match_gain;
        let w_diag = self.growth_config.diagnostic_window;
        let split_tax = self.economy_config.split_code_tax;
        let count = self.editable.economy().comparators().len();

        for comp_idx in 0..count {
            let comp = self.editable.economy().comparators()[comp_idx].clone();
            if !comparator_cone_is_mature(self.editable.circuit(), self.editable.economy(), &comp) {
                continue;
            }
            let Some(win) = self.comparator_windows.get(comp_idx) else {
                continue;
            };
            if win.disagree.len() < w_diag {
                continue;
            }

            let e_primary = win.e_primary_count_tail(w_diag);
            let e_alt = win.e_alt_count_tail(w_diag);
            let flux_primary = win.flux_primary_tail(w_diag);
            let flux_alt = win.flux_alt_tail(w_diag);
            let delta_l_comp =
                comparator_marginal_code_bits(self.editable.circuit(), &comp, split_tax);

            // Step A: Reversible Wake Driver Handoff over trailing W_diag ticks.
            if e_alt < e_primary && (flux_alt - flux_primary) > delta_l_comp {
                let _ = self.perform_comparator_handoff(comp_idx);
            }

            // Step B: Irreversible Two-Clause Refutation Pruning over full W_obs horizon.
            let Some(win_after) = self.comparator_windows.get(comp_idx) else {
                continue;
            };
            if win_after.is_full() {
                let disagreements = win_after.disagreement_count();
                if disagreements >= min_gain {
                    let f_pri = win_after.flux_primary();
                    let f_alt = win_after.flux_alt();
                    let prune_decision = if f_alt <= 0.0 && f_pri > 0.0 {
                        Some(false)
                    } else if f_pri <= 0.0 && f_alt > 0.0 {
                        Some(true)
                    } else {
                        None
                    };
                    if let Some(prune_primary) = prune_decision
                        && self.excise_port_comparator(comp_idx, prune_primary)
                    {
                        self.disagreement_prune_events += 1;
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Runs unsupervised offline loopback (`M2B-D3`) by cutting external inputs
    /// ($A(t+1) \leftarrow \hat{A}(t+1)$) under [`ExecutionPhase::Sleep`] for `loopback_ticks`.
    ///
    /// Preserves frozen wake null models and executes 3-route majority pruning when two agreed
    /// routes conflict with one minority faulty route on a port.
    pub fn run_offline_loopback(&mut self, loopback_ticks: u64) -> OfflineLoopbackReport {
        self.pending_prediction = None;
        let width = self.editable.circuit().width();
        let num_in = usize::from(self.editable.compiled().num_inputs());
        let num_out = usize::from(self.editable.compiled().num_outputs());
        let mut curr_input = self.prev_observed_frame.clone();
        curr_input.resize(num_in, 0);

        let start_agree = self.editable.economy().comparator_agreement_ticks();
        let start_disagree = self.editable.economy().comparator_disagreement_ticks();
        let start_flux = self.editable.economy().cumulative_comparator_flux();

        self.editable.economy_mut().set_phase(ExecutionPhase::Sleep);

        let mut max_circular_discrepancy = 0u64;
        let mut minority_routes_pruned = 0u64;
        let zeros_port_rewards = vec![0.0; num_out];
        let mut per_port_consensus: Vec<(u64, u64, u64)> = vec![(0, 0, 0); num_out];
        let consensus_thr = self
            .growth_config
            .min_match_gain
            .min(loopback_ticks.max(1) as usize) as u64;

        for _ in 0..loopback_ticks {
            let mut predictions = vec![0u64; num_out];
            let _ = self.editable.step(&curr_input, &mut predictions);
            for (slot, &pred) in curr_input.iter_mut().zip(&predictions) {
                *slot = width.normalize(pred);
            }

            let node_outs = node_output_values(self.editable.circuit(), self.editable.state());
            let wires = self.editable.state().wire_values().to_vec();
            let _ = self.editable.economy_mut().record_activity(&node_outs);

            let n = self.editable.circuit().node_count();
            if self.ledger.node_debts.len() != n {
                self.ledger
                    .ensure_capacity(self.editable.circuit(), self.economy_config.split_code_tax);
                self.editable
                    .economy_mut()
                    .assign_sprout_debts(&self.ledger.node_debts);
            }
            let mut maintenance_costs = vec![0.0; n];
            for (i, cost) in maintenance_costs.iter_mut().enumerate().take(n) {
                let node = &self.editable.economy().nodes()[i];
                if node.juvenile_ticks == 0 && !self.ledger.debt_paid[i] {
                    *cost = self.ledger.node_debts[i];
                    self.ledger.debt_paid[i] = true;
                    if let Some(n_mut) = self.editable.economy_mut().node_mut(i) {
                        n_mut.debt_paid = true;
                    }
                }
            }

            let comp_count = self.editable.economy().comparators().len();
            if comp_count == 0 {
                let snapshot = counterfactual_sensitivity(
                    self.editable.circuit(),
                    self.editable.compiled(),
                    self.editable.state(),
                    self.seed_bank,
                    self.tick,
                    self.sample_count,
                )
                .expect("valid sensitivity snapshot");
                let ancestry = assemble_causal_ancestry(
                    self.editable.circuit(),
                    self.editable.compiled(),
                    self.editable.state(),
                    &snapshot,
                    &self.editable.economy().activity(),
                    self.editable.economy().register_eligibility(),
                    self.economy_config.trace_discount,
                )
                .expect("valid causal ancestry");
                let econ_tick = self
                    .editable
                    .economy_mut()
                    .apply_causal_tick(&zeros_port_rewards, &ancestry, &maintenance_costs)
                    .expect("valid sleep tick");
                self.cumulative_debt_paid += econ_tick.maintenance;
            } else {
                let targets = self.editable.comparator_targets();
                let snapshot = counterfactual_sensitivity_with_comparators(
                    self.editable.circuit(),
                    self.editable.compiled(),
                    self.editable.state(),
                    self.seed_bank,
                    self.tick,
                    self.sample_count,
                    &targets,
                )
                .expect("valid comparator sensitivity snapshot");
                let ancestry = assemble_causal_ancestry_with_comparators(
                    self.editable.circuit(),
                    self.editable.compiled(),
                    self.editable.state(),
                    &snapshot,
                    &self.editable.economy().activity(),
                    self.editable.economy().register_eligibility(),
                    self.economy_config.trace_discount,
                    &targets,
                    true,
                )
                .expect("valid sleep comparator ancestry");

                let mut pri_vals = Vec::with_capacity(comp_count);
                let mut alt_vals = Vec::with_capacity(comp_count);
                let mut eqz_vals = Vec::with_capacity(comp_count);
                for comp in self.editable.economy().comparators() {
                    let sub_in =
                        &self.editable.compiled().node_input_wires()[comp.sub_node.index()];
                    let p_v = width.normalize(wires[sub_in[0]]);
                    let a_v = width.normalize(wires[sub_in[1]]);
                    max_circular_discrepancy =
                        max_circular_discrepancy.max(width.circular_distance(p_v, a_v));
                    pri_vals.push(p_v);
                    alt_vals.push(a_v);
                    eqz_vals.push(width.normalize(node_outs[comp.eqz_node.index()]));
                }
                let comp_obs = self
                    .editable
                    .economy_mut()
                    .observe_comparators(&pri_vals, &alt_vals, &eqz_vals, None)
                    .expect("valid sleep comparator observation");
                let econ_tick = self
                    .editable
                    .economy_mut()
                    .apply_causal_tick_with_comparators(
                        &zeros_port_rewards,
                        &comp_obs.rewards,
                        comp_obs.agreements,
                        comp_obs.disagreements,
                        &ancestry,
                        &maintenance_costs,
                    )
                    .expect("valid sleep causal tick");
                self.cumulative_net_flux += econ_tick.net_flux;
                self.cumulative_debt_paid += econ_tick.maintenance;

                // 3-route majority pruning: when a port has 2 mature comparators (3 routes) and 2
                // routes consistently agree while 1 minority route disagrees, prune only the minority.
                for (p, counts) in per_port_consensus.iter_mut().enumerate() {
                    let p_u16 = p as u16;
                    let comps_on_p: Vec<usize> = self
                        .editable
                        .economy()
                        .comparators()
                        .iter()
                        .enumerate()
                        .filter_map(|(i, c)| (c.target_port == p_u16).then_some(i))
                        .collect();
                    if comps_on_p.len() != 2 {
                        *counts = (0, 0, 0);
                        continue;
                    }
                    let idx_a = comps_on_p[0];
                    let idx_b = comps_on_p[1];
                    let comps = self.editable.economy().comparators();
                    if !comparator_cone_is_mature(
                        self.editable.circuit(),
                        self.editable.economy(),
                        &comps[idx_a],
                    ) || !comparator_cone_is_mature(
                        self.editable.circuit(),
                        self.editable.economy(),
                        &comps[idx_b],
                    ) {
                        continue;
                    }
                    let pri = pri_vals[idx_a];
                    let alt_a = alt_vals[idx_a];
                    let alt_b = alt_vals[idx_b];
                    if pri != alt_a {
                        counts.0 += 1;
                    }
                    if pri != alt_b {
                        counts.1 += 1;
                    }
                    if alt_a != alt_b {
                        counts.2 += 1;
                    }
                    let (d_pa, d_pb, d_ab) = *counts;
                    let action = if d_pa == 0 && d_pb >= consensus_thr && d_ab >= consensus_thr {
                        Some((idx_b, false))
                    } else if d_pb == 0 && d_pa >= consensus_thr && d_ab >= consensus_thr {
                        Some((idx_a, false))
                    } else if d_ab == 0 && d_pa >= consensus_thr && d_pb >= consensus_thr {
                        Some((idx_a, true))
                    } else {
                        None
                    };
                    if let Some((prune_idx, prune_primary)) = action
                        && self.excise_port_comparator(prune_idx, prune_primary)
                    {
                        self.disagreement_prune_events += 1;
                        minority_routes_pruned += 1;
                        *counts = (0, 0, 0);
                        break;
                    }
                }
            }

            self.tick += 1;
        }

        self.editable.economy_mut().set_phase(ExecutionPhase::Wake);
        let comparator_agreement_ticks =
            self.editable.economy().comparator_agreement_ticks() - start_agree;
        let comparator_disagreement_ticks =
            self.editable.economy().comparator_disagreement_ticks() - start_disagree;
        let sleep_comparator_flux =
            self.editable.economy().cumulative_comparator_flux() - start_flux;

        OfflineLoopbackReport {
            ticks: loopback_ticks,
            comparator_agreement_ticks,
            comparator_disagreement_ticks,
            sleep_comparator_flux,
            max_circular_discrepancy,
            minority_routes_pruned,
            contradiction_detected: comparator_disagreement_ticks > 0,
        }
    }
}

fn comparator_cone_is_mature(
    circuit: &Circuit,
    economy: &EconomyState,
    comp: &ComparatorSite,
) -> bool {
    let mut visited = BTreeSet::new();
    let mut stack = vec![comp.sub_node, comp.eqz_node];
    while let Some(node) = stack.pop() {
        if !visited.insert(node) {
            continue;
        }
        let Some(econ) = economy.nodes().get(node.index()) else {
            return false;
        };
        let is_comb_split = circuit.wire_delay() == WireDelayMode::Combinational
            && matches!(circuit.nodes().get(node.index()), Some(Primitive::Split));
        if !is_pure_boundary_split(circuit, node) && !is_comb_split && econ.juvenile_ticks > 0 {
            return false;
        }
        for edge in circuit.edges() {
            if let SignalSink::NodeIn { node: dst, .. } = edge.dst
                && dst == node
                && let crate::substrate::SignalSource::NodeOut { node: src, .. } = edge.src
            {
                stack.push(src);
            }
        }
    }
    true
}

fn port_comparators_mature(circuit: &Circuit, economy: &EconomyState, target_port: u16) -> bool {
    economy
        .comparators()
        .iter()
        .filter(|c| c.target_port == target_port)
        .all(|c| comparator_cone_is_mature(circuit, economy, c))
}

fn comparator_marginal_code_bits(
    circuit: &Circuit,
    comp: &ComparatorSite,
    split_code_tax: bool,
) -> f64 {
    let full = circuit_code_bits_with_tax(circuit, 0, split_code_tax);
    let mut without = circuit.clone();
    let mut removed = BTreeSet::new();
    removed.insert(comp.sub_node);
    removed.insert(comp.eqz_node);
    without.remove_nodes(&removed);
    let reduced = circuit_code_bits_with_tax(&without, 0, split_code_tax);
    (full - reduced).max(0.0)
}

fn compute_register_target_reachability(
    circuit: &Circuit,
    compiled: &crate::substrate::CompiledCircuit,
    economy: &EconomyState,
) -> Vec<bool> {
    let node_count = circuit.node_count();
    let mut target_reachable = vec![false; node_count];
    let mut reverse_adj = vec![Vec::new(); node_count];
    let mut stack = Vec::new();

    for edge in circuit.edges() {
        if let SignalSource::NodeOut { node: src, .. } = edge.src
            && src.index() < node_count
        {
            match edge.dst {
                SignalSink::BoundaryOut(_) => {
                    if !target_reachable[src.index()] {
                        target_reachable[src.index()] = true;
                        stack.push(src.index());
                    }
                }
                SignalSink::NodeIn { node: dst, .. } if dst.index() < node_count => {
                    reverse_adj[dst.index()].push(src.index());
                }
                _ => {}
            }
        }
    }
    for comp in economy.comparators() {
        for root in [comp.eqz_node, comp.sub_node] {
            if root.index() < node_count && !target_reachable[root.index()] {
                target_reachable[root.index()] = true;
                stack.push(root.index());
            }
        }
    }
    while let Some(node) = stack.pop() {
        for &pred in &reverse_adj[node] {
            if !target_reachable[pred] {
                target_reachable[pred] = true;
                stack.push(pred);
            }
        }
    }

    let mut reg_reachable = vec![false; compiled.total_registers()];
    let mut reg_idx = 0usize;
    for (idx, prim) in circuit.nodes().iter().enumerate() {
        if prim.is_stateful() {
            if reg_idx < reg_reachable.len() {
                reg_reachable[reg_idx] = target_reachable[idx];
            }
            reg_idx += 1;
        }
    }
    if circuit.wire_delay() == WireDelayMode::TicksPerHop {
        let mut base = compiled.explicit_delay_count();
        for edge in circuit.edges() {
            let reaches = match edge.dst {
                SignalSink::BoundaryOut(_) => true,
                SignalSink::NodeIn { node, .. } => {
                    target_reachable.get(node.index()).copied().unwrap_or(false)
                }
            };
            for s in 0..usize::from(edge.hops) {
                if base + s < reg_reachable.len() {
                    reg_reachable[base + s] = reaches;
                }
            }
            base += usize::from(edge.hops);
        }
    }
    reg_reachable
}

fn injected_subgraph_register_depth(
    circuit: &Circuit,
    is_surviving: &[bool],
    start: NodeId,
) -> usize {
    fn rec(circuit: &Circuit, is_surviving: &[bool], node: NodeId, on_path: &mut [bool]) -> usize {
        if is_surviving.get(node.index()).copied().unwrap_or(false) || on_path[node.index()] {
            return 0;
        }
        on_path[node.index()] = true;
        let self_reg = usize::from(matches!(
            circuit.nodes().get(node.index()),
            Some(Primitive::Delay { .. })
        ));
        let is_tph = circuit.wire_delay() == WireDelayMode::TicksPerHop;
        let mut max_up = 0usize;
        for edge in circuit.edges() {
            if let SignalSink::NodeIn { node: dst, .. } = edge.dst
                && dst == node
            {
                let hop_regs = if is_tph { usize::from(edge.hops) } else { 0 };
                let up = match edge.src {
                    SignalSource::BoundaryIn(_) => hop_regs,
                    SignalSource::NodeOut { node: src_node, .. } => {
                        if on_path[src_node.index()] {
                            0
                        } else if is_surviving.get(src_node.index()).copied().unwrap_or(false) {
                            hop_regs
                        } else {
                            hop_regs + rec(circuit, is_surviving, src_node, on_path)
                        }
                    }
                };
                max_up = max_up.max(up);
            }
        }
        on_path[node.index()] = false;
        self_reg + max_up
    }

    let mut on_path = vec![false; circuit.node_count()];
    rec(circuit, is_surviving, start, &mut on_path)
}

fn settle_injected_tph_constant_subgraphs(editable: &mut EditableCircuit, is_surviving: &[bool]) {
    let circuit = editable.circuit().clone();
    let compiled = editable.compiled().clone();
    if circuit.wire_delay() != WireDelayMode::TicksPerHop {
        return;
    }
    let node_count = circuit.node_count();
    let mut const_eligible = vec![false; node_count];
    for i in 0..node_count {
        if !is_surviving[i] && !circuit.nodes()[i].is_stateful() {
            const_eligible[i] = true;
        }
    }
    // Any node fed by BoundaryIn, a surviving node, or a Delay primitive is not a pure
    // newly-injected constant subgraph.
    let mut queue = Vec::new();
    for edge in circuit.edges() {
        if let SignalSink::NodeIn { node: dst, .. } = edge.dst
            && dst.index() < node_count
            && const_eligible[dst.index()]
        {
            let disqualified_src = match edge.src {
                SignalSource::BoundaryIn(_) => true,
                SignalSource::NodeOut { node: src, .. } => !const_eligible[src.index()],
            };
            if disqualified_src {
                const_eligible[dst.index()] = false;
                queue.push(dst);
            }
        }
    }
    while let Some(bad) = queue.pop() {
        for edge in circuit.edges() {
            if let SignalSource::NodeOut { node: src, .. } = edge.src
                && src == bad
                && let SignalSink::NodeIn { node: dst, .. } = edge.dst
                && dst.index() < node_count
                && const_eligible[dst.index()]
            {
                const_eligible[dst.index()] = false;
                queue.push(dst);
            }
        }
    }

    let mut settle_reg_indices = Vec::new();
    let mut base = compiled.explicit_delay_count();
    for edge in circuit.edges() {
        let hops = usize::from(edge.hops);
        if let SignalSource::NodeOut { node: src, .. } = edge.src
            && src.index() < node_count
            && const_eligible[src.index()]
        {
            for s in 0..hops {
                settle_reg_indices.push(base + s);
            }
        }
        base += hops;
    }
    if settle_reg_indices.is_empty() {
        return;
    }

    let mut probe = crate::substrate::ScalarState::new(&compiled);
    let _ = probe.set_registers(&compiled, editable.state().registers());
    let zeros_in = vec![0u64; usize::from(compiled.num_inputs())];
    let mut out = vec![0u64; usize::from(compiled.num_outputs())];
    for _ in 0..(node_count + 4) {
        if probe.step(&compiled, &zeros_in, &mut out).is_err() {
            return;
        }
    }
    let mut live_regs = editable.state().registers().to_vec();
    let probe_regs = probe.registers();
    for r in settle_reg_indices {
        if r < live_regs.len() && r < probe_regs.len() {
            live_regs[r] = probe_regs[r];
        }
    }
    let _ = editable.set_registers(&live_regs);
}

impl PredictorBaseline for ArkheLearner {
    fn name(&self) -> &str {
        "ArkheM1"
    }

    fn predict(&mut self, _budget: &mut TickBudget) -> Vec<u64> {
        let pre_epoch = self.editable.structure_epoch();
        let pre_registers = self.editable.state().registers().to_vec();
        let num_out = usize::from(self.editable.compiled().num_outputs());
        let mut predictions = vec![0u64; num_out];
        let _ = self
            .editable
            .step(&self.prev_observed_frame, &mut predictions);
        let node_outputs = node_output_values(self.editable.circuit(), self.editable.state());
        let _ = self.editable.economy_mut().record_activity(&node_outputs);
        let snap = if self.editable.economy().comparators().is_empty() {
            let snapshot = counterfactual_sensitivity(
                self.editable.circuit(),
                self.editable.compiled(),
                self.editable.state(),
                self.seed_bank,
                self.tick,
                self.sample_count,
            )
            .expect("valid sensitivity snapshot");
            let ancestry = assemble_causal_ancestry(
                self.editable.circuit(),
                self.editable.compiled(),
                self.editable.state(),
                &snapshot,
                &self.editable.economy().activity(),
                self.editable.economy().register_eligibility(),
                self.economy_config.trace_discount,
            )
            .expect("valid causal ancestry");
            PendingSnapshot::Base { ancestry, snapshot }
        } else {
            let targets = self.editable.comparator_targets();
            let snapshot = counterfactual_sensitivity_with_comparators(
                self.editable.circuit(),
                self.editable.compiled(),
                self.editable.state(),
                self.seed_bank,
                self.tick,
                self.sample_count,
                &targets,
            )
            .expect("valid comparator sensitivity snapshot");
            let ancestry = assemble_causal_ancestry_with_comparators(
                self.editable.circuit(),
                self.editable.compiled(),
                self.editable.state(),
                &snapshot,
                &self.editable.economy().activity(),
                self.editable.economy().register_eligibility(),
                self.economy_config.trace_discount,
                &targets,
                false,
            )
            .expect("valid comparator causal ancestry");
            PendingSnapshot::WithComparators { ancestry, snapshot }
        };

        self.pending_prediction = Some((
            predictions.clone(),
            snap,
            pre_epoch,
            pre_registers,
            node_outputs,
        ));
        predictions
    }

    fn observe(&mut self, truth: &[u64], budget: &mut TickBudget) -> Option<f64> {
        if self.total_budget_seen.is_none() {
            self.total_budget_seen = Some(budget.remaining().saturating_add(budget.consumed()));
        }
        let (predictions, pending_snap, pre_epoch, pre_registers, pre_step_node_outs) = self
            .pending_prediction
            .take()
            .expect("observe called with pending prediction");
        let (base_ancestry, comp_ancestry, output_by_node, latch_by_node, comp_eqz_by_node) =
            match &pending_snap {
                PendingSnapshot::Base { ancestry, snapshot } => (
                    ancestry,
                    None,
                    &snapshot.output_by_node,
                    &snapshot.latch_by_node,
                    None,
                ),
                PendingSnapshot::WithComparators { ancestry, snapshot } => (
                    &ancestry.base,
                    Some(ancestry),
                    &snapshot.base.output_by_node,
                    &snapshot.base.latch_by_node,
                    Some(&snapshot.comparator_eqz_by_node),
                ),
            };

        let mut input_frame_stack = [0u64; 16];
        let input_frame_heap;
        let input_frame_len = self.prev_observed_frame.len();
        let input_frame: &[u64] = if input_frame_len <= input_frame_stack.len() {
            input_frame_stack[..input_frame_len].copy_from_slice(&self.prev_observed_frame);
            &input_frame_stack[..input_frame_len]
        } else {
            input_frame_heap = self.prev_observed_frame.clone();
            &input_frame_heap
        };
        self.obs_window
            .push_frame_with_state(truth, pre_epoch, &pre_registers);
        for (slot, &val) in self.prev_observed_frame.iter_mut().zip(truth) {
            *slot = self.editable.circuit().width().normalize(val);
        }

        let num_ports = self.editable.economy().scorers().len();
        let mut port_rewards_stack = [0.0f64; 8];
        let mut port_rewards_heap = Vec::new();
        let port_rewards: &mut [f64] = if num_ports <= port_rewards_stack.len() {
            &mut port_rewards_stack[..num_ports]
        } else {
            port_rewards_heap.resize(num_ports, 0.0);
            &mut port_rewards_heap
        };
        for (p, reward_slot) in port_rewards.iter_mut().enumerate() {
            let is_driven = self
                .editable
                .circuit()
                .edges()
                .iter()
                .any(|edge| edge.dst == SignalSink::BoundaryOut(p as u16));
            let has_causal_ancestry = base_ancestry
                .port_shares
                .get(p)
                .is_some_and(|shares| shares.iter().sum::<f64>() > 0.0);
            let scored = is_driven && has_causal_ancestry;
            self.port_states[p].scored_last_observe = scored;
            if scored {
                *reward_slot = self
                    .editable
                    .economy_mut()
                    .score_port_single(p, predictions[p], truth[p])
                    .expect("valid port scoring");
            } else {
                let _ = self
                    .editable
                    .economy_mut()
                    .observe_unconnected_port(p, truth[p]);
                *reward_slot = 0.0;
            }
        }

        let n = self.editable.circuit().node_count();
        if self.ledger.node_debts.len() != n {
            self.ledger
                .ensure_capacity(self.editable.circuit(), self.economy_config.split_code_tax);
            self.editable
                .economy_mut()
                .assign_sprout_debts(&self.ledger.node_debts);
        }
        let mut maintenance_stack = [0.0f64; 128];
        let mut maintenance_heap = Vec::new();
        let maintenance_costs: &mut [f64] = if n <= maintenance_stack.len() {
            let slice = &mut maintenance_stack[..n];
            slice.fill(0.0);
            slice
        } else {
            maintenance_heap.resize(n, 0.0);
            &mut maintenance_heap
        };
        for (i, cost) in maintenance_costs.iter_mut().enumerate().take(n) {
            let node = &self.editable.economy().nodes()[i];
            if node.juvenile_ticks == 0 && !self.ledger.debt_paid[i] {
                *cost = self.ledger.node_debts[i];
                self.ledger.debt_paid[i] = true;
                if let Some(n) = self.editable.economy_mut().node_mut(i) {
                    n.debt_paid = true;
                }
            }
        }

        let mut comp_pri_vals = Vec::new();
        let mut comp_alt_vals = Vec::new();
        let mut comp_eqz_vals = Vec::new();
        let mut comp_rewards = Vec::new();

        let econ_tick = if let Some(comp_anc) = comp_ancestry {
            let comps_count = self.editable.economy().comparators().len();
            comp_pri_vals.reserve(comps_count);
            comp_alt_vals.reserve(comps_count);
            comp_eqz_vals.reserve(comps_count);
            {
                let wires = self.editable.state().wire_values();
                let node_input_wires = self.editable.compiled().node_input_wires();
                for comp in self.editable.economy().comparators() {
                    let sub_in = &node_input_wires[comp.sub_node.index()];
                    comp_pri_vals.push(wires[sub_in[0]]);
                    comp_alt_vals.push(wires[sub_in[1]]);
                    comp_eqz_vals.push(pre_step_node_outs[comp.eqz_node.index()]);
                }
            }
            let fact_slice = truth.get(..num_ports).unwrap_or(truth);
            let comp_obs = self
                .editable
                .economy_mut()
                .observe_comparators(
                    &comp_pri_vals,
                    &comp_alt_vals,
                    &comp_eqz_vals,
                    Some(fact_slice),
                )
                .expect("valid comparator observation");
            comp_rewards = comp_obs.rewards;
            self.editable
                .economy_mut()
                .apply_causal_tick_with_comparators(
                    port_rewards,
                    &comp_rewards,
                    comp_obs.agreements,
                    comp_obs.disagreements,
                    comp_anc,
                    maintenance_costs,
                )
                .expect("valid comparator causal tick")
        } else {
            self.editable
                .economy_mut()
                .apply_causal_tick(port_rewards, base_ancestry, maintenance_costs)
                .expect("valid causal tick")
        };
        self.cumulative_net_flux += econ_tick.net_flux;
        self.cumulative_debt_paid += econ_tick.maintenance;
        self.reserve_clipped_count += econ_tick.clipped_nodes as u64;
        self.reserve_clipped_bits += econ_tick.clipped_bits;
        self.reserve_mismatch_floored += econ_tick.mismatch_floored_nodes as u64;

        // Update mature comparator rolling diagnostic windows and check Wake handoff / refutation.
        if !comp_rewards.is_empty() {
            self.sync_comparator_windows();
            let width = self.editable.circuit().width();
            let comps: Vec<ComparatorSite> = self.editable.economy().comparators().to_vec();
            for (m, comp) in comps.iter().enumerate() {
                let mature = comparator_cone_is_mature(
                    self.editable.circuit(),
                    self.editable.economy(),
                    comp,
                );
                let primed = comp_ancestry
                    .and_then(|a| a.comparator_shares.get(m))
                    .is_some_and(|s| s.iter().sum::<f64>() > 0.0);
                if mature
                    && primed
                    && let Some(win) = self.comparator_windows.get_mut(m)
                {
                    let tp = usize::from(comp.target_port);
                    let truth_norm = width.normalize(truth[tp]);
                    let disagree = width.normalize(comp_eqz_vals[m]) == 0;
                    let e_pri = width.normalize(comp_pri_vals[m]) != truth_norm;
                    let e_alt = width.normalize(comp_alt_vals[m]) != truth_norm;
                    win.push(disagree, port_rewards[tp], comp_rewards[m], e_pri, e_alt);
                }
            }
            if self.process_wake_comparator_events() {
                self.tick += 1;
                return None;
            }
        }

        // Starvation pruning
        if !econ_tick.starved_nodes.is_empty() {
            let starved_set: BTreeSet<NodeId> = econ_tick
                .starved_nodes
                .iter()
                .map(|&idx| NodeId(idx as u32))
                .collect();
            if self.try_wake_comparator_bankruptcy_refutation(&starved_set) {
                self.tick += 1;
                return None;
            }
            let pre_prune_circuit = self.editable.circuit().clone();
            let pre_comps = self.editable.economy().comparators().to_vec();
            let juvenile_set: BTreeSet<NodeId> = self
                .editable
                .economy()
                .nodes()
                .iter()
                .enumerate()
                .filter_map(|(idx, econ)| (econ.juvenile_ticks > 0).then_some(NodeId(idx as u32)))
                .collect();
            let mut used_split_preserving = false;
            let mut excised_starved_passthrough = false;
            let prune_res = if let Some((pruned, old_to_new)) =
                prune_starved_preserving_healthy_split_branches(
                    self.editable.circuit(),
                    &starved_set,
                    &juvenile_set,
                ) {
                used_split_preserving = true;
                self.editable
                    .recompile_preserving_state(pruned, &old_to_new)
                    .map(|()| old_to_new)
            } else if self.growth_config.excision_mode == ExcisionMode::VerifiedLocalExcision
                && let Ok(Some(old_to_new)) =
                    self.editable.excise_verified_passthrough_for_starved_cone(
                        &starved_set,
                        self.economy_config.split_code_tax,
                    )
            {
                excised_starved_passthrough = true;
                Ok(old_to_new)
            } else {
                self.editable.prune_starved_nodes_with_mode(
                    self.growth_config.excision_mode,
                    self.growth_config.diagnostic_window,
                )
            };
            if let Ok(old_to_new) = prune_res {
                self.ledger.on_prune(
                    self.editable.circuit(),
                    self.economy_config.split_code_tax,
                    &old_to_new,
                );
                let preserve_scorer = self.growth_config.excision_mode
                    == ExcisionMode::VerifiedLocalExcision
                    && (used_split_preserving
                        || excised_starved_passthrough
                        || (0..num_ports).all(|q| {
                            let sink = SignalSink::BoundaryOut(q as u16);
                            let pre_drv = pre_prune_circuit.edges().iter().find(|e| e.dst == sink);
                            let post_drv = self
                                .editable
                                .circuit()
                                .edges()
                                .iter()
                                .find(|e| e.dst == sink);
                            match (pre_drv, post_drv) {
                                (Some(pe), Some(_)) => match pe.src {
                                    SignalSource::NodeOut { node, .. } => {
                                        !starved_set.contains(&node)
                                    }
                                    SignalSource::BoundaryIn(_) => true,
                                },
                                (None, None) => true,
                                _ => false,
                            }
                        }));
                if preserve_scorer {
                    self.clear_changed_port_diagnostics_preserving_scorer(
                        &pre_prune_circuit,
                        &old_to_new,
                    );
                } else {
                    self.reset_changed_ports(&pre_prune_circuit, &old_to_new);
                }
                self.apply_node_remap_to_windows(&old_to_new);
                if excised_starved_passthrough {
                    self.idle_prune_events += 1;
                    self.local_excision_events += 1;
                } else {
                    self.starvation_prune_events += 1;
                    self.record_scoped_starvation_prune(
                        &pre_prune_circuit,
                        &pre_comps,
                        &starved_set,
                    );
                }
            }
        }

        // Post-juvenile diagnostic buffer
        let mut res_stack = [0u64; 8];
        let mut res_heap = Vec::new();
        let res: &mut [u64] = if num_ports <= res_stack.len() {
            &mut res_stack[..num_ports]
        } else {
            res_heap.resize(num_ports, 0);
            &mut res_heap
        };
        let width = self.editable.circuit().width();
        for (slot, (&tr, &pr)) in res.iter_mut().zip(truth.iter().zip(&predictions)) {
            *slot = width.sub(tr, pr);
        }
        let node_outs = pre_step_node_outs;
        let node_count = self.editable.circuit().node_count();
        let mut sensitive_stack = [false; 128];
        let mut sensitive_heap = Vec::new();
        let sensitive: &mut [bool] = if node_count <= sensitive_stack.len() {
            let slice = &mut sensitive_stack[..node_count];
            slice.fill(false);
            slice
        } else {
            sensitive_heap.resize(node_count, false);
            &mut sensitive_heap
        };
        match self.growth_config.excision_mode {
            ExcisionMode::BaselineD1b1 => {
                for (i, sens) in sensitive.iter_mut().enumerate().take(node_count) {
                    if i >= output_by_node.len() || i >= latch_by_node.len() {
                        continue;
                    }
                    *sens = output_by_node[i].iter().any(|&s| s > 0.0)
                        || latch_by_node[i].iter().any(|&s| s > 0.0)
                        || comp_eqz_by_node.is_some_and(|by_node| {
                            by_node
                                .get(i)
                                .is_some_and(|scores| scores.iter().any(|&s| s > 0.0))
                        });
                }
            }
            ExcisionMode::VerifiedLocalExcision => {
                let reg_reaches_target = compute_register_target_reachability(
                    self.editable.circuit(),
                    self.editable.compiled(),
                    self.editable.economy(),
                );
                for (i, sens) in sensitive.iter_mut().enumerate().take(node_count) {
                    if i >= output_by_node.len() || i >= latch_by_node.len() {
                        continue;
                    }
                    let direct_port = output_by_node[i].iter().enumerate().any(|(p, &s)| {
                        s > 0.0
                            && base_ancestry
                                .port_shares
                                .get(p)
                                .and_then(|sh| sh.get(i))
                                .copied()
                                .unwrap_or(0.0)
                                > 0.0
                    });
                    let direct_comp = comp_eqz_by_node.is_some_and(|by_node| {
                        by_node.get(i).is_some_and(|scores| {
                            scores.iter().enumerate().any(|(m, &s)| {
                                s > 0.0
                                    && comp_ancestry
                                        .and_then(|ca| ca.comparator_shares.get(m))
                                        .and_then(|sh| sh.get(i))
                                        .copied()
                                        .unwrap_or(0.0)
                                        > 0.0
                            })
                        })
                    });
                    let anchored_latch = latch_by_node[i].iter().enumerate().any(|(r, &s)| {
                        s > 0.0 && reg_reaches_target.get(r).copied().unwrap_or(false)
                    });
                    *sens = direct_port || direct_comp || anchored_latch;
                }
            }
        }

        for p in 0..num_ports {
            let port_u16 = p as u16;
            let diag_mature =
                port_is_cone_mature(self.editable.circuit(), self.editable.economy(), port_u16)
                    || (self.growth_config.redundancy_sprouting
                        && port_primary_cone_is_mature(
                            self.editable.circuit(),
                            self.editable.economy(),
                            port_u16,
                        ));
            if pre_epoch == self.editable.structure_epoch()
                && diag_mature
                && port_is_primed(self.editable.circuit(), base_ancestry, port_u16)
            {
                self.port_states[p]
                    .diagnostic_buffer
                    .push_frame_with_snapshot(
                        input_frame,
                        truth,
                        &predictions,
                        res,
                        &node_outs,
                        sensitive,
                        pre_epoch,
                        &pre_registers,
                    );
                self.port_states[p].ticks_since_last_check += 1;
            }
        }

        // Pass 1 (Stage 1 + Stage 2 across all eligible ports) before Pass 2 (Stage 3).
        let start_port = (self.tick as usize) % num_ports;
        let mut sprouted_any = false;
        let mut pass1_timers: Vec<Option<u64>> = vec![None; num_ports];
        for offset in 0..num_ports {
            let p = (start_port + offset) % num_ports;
            let port_u16 = p as u16;

            let is_cone_mature =
                port_is_cone_mature(self.editable.circuit(), self.editable.economy(), port_u16);
            let is_primed = port_is_primed(self.editable.circuit(), base_ancestry, port_u16);
            let buf_ready =
                self.port_states[p].diagnostic_buffer.len() >= self.growth_config.diagnostic_window;
            let timer_ready =
                self.port_states[p].ticks_since_last_check >= self.growth_config.sprout_interval;

            if !(is_cone_mature && is_primed && buf_ready && timer_ready) {
                continue;
            }

            let prev_ticks_since_check = self.port_states[p].ticks_since_last_check;
            self.port_states[p].ticks_since_last_check = 0;
            pass1_timers[p] = Some(prev_ticks_since_check);

            // 1. Idle / ring-identity / pass-through excision (M3-E1 + M3-E2)
            let replay_initial_regs: Option<Vec<u64>> = self.port_states[p]
                .diagnostic_buffer
                .first_frame_snapshot()
                .and_then(|(epoch, regs)| {
                    (epoch == self.editable.structure_epoch()
                        && regs.len() == self.editable.compiled().total_registers())
                    .then(|| regs.to_vec())
                });
            let idle_nodes = match self.growth_config.excision_mode {
                ExcisionMode::BaselineD1b1 => self.port_states[p]
                    .diagnostic_buffer
                    .idle_mature_nodes(self.editable.economy()),
                ExcisionMode::VerifiedLocalExcision => self.port_states[p]
                    .diagnostic_buffer
                    .idle_mature_nodes_for_circuit(
                        self.editable.circuit(),
                        self.editable.economy(),
                    ),
            };
            if !idle_nodes.is_empty() {
                let pre_prune_circuit = self.editable.circuit().clone();
                let diag_buf = std::mem::replace(
                    &mut self.port_states[p].diagnostic_buffer,
                    DiagnosticBuffer::new(0),
                );
                let prune_res = self.editable.prune_zero_sensitivity_mature_nodes_with_mode(
                    &idle_nodes,
                    &diag_buf.inputs,
                    &diag_buf.predictions,
                    self.economy_config.split_code_tax,
                    self.growth_config.excision_mode,
                    self.growth_config.diagnostic_window,
                    Some(&diag_buf.node_outputs),
                    replay_initial_regs.as_deref(),
                );
                self.port_states[p].diagnostic_buffer = diag_buf;
                if let Ok(old_to_new) = prune_res {
                    let any_pruned = old_to_new.iter().any(Option::is_none);
                    if any_pruned {
                        self.ledger.on_prune(
                            self.editable.circuit(),
                            self.economy_config.split_code_tax,
                            &old_to_new,
                        );
                        match self.growth_config.excision_mode {
                            ExcisionMode::BaselineD1b1 => {
                                self.reset_changed_ports(&pre_prune_circuit, &old_to_new);
                            }
                            ExcisionMode::VerifiedLocalExcision => {
                                self.clear_changed_port_diagnostics_preserving_scorer(
                                    &pre_prune_circuit,
                                    &old_to_new,
                                );
                            }
                        }
                        self.apply_node_remap_to_windows(&old_to_new);
                        self.idle_prune_events += 1;
                        self.tick += 1;
                        return None;
                    }
                }
            }
            if self.growth_config.excision_mode == ExcisionMode::VerifiedLocalExcision {
                let pre_prune_circuit = self.editable.circuit().clone();
                let diag_buf = std::mem::replace(
                    &mut self.port_states[p].diagnostic_buffer,
                    DiagnosticBuffer::new(0),
                );
                let excise_res = self.editable.excise_verified_passthrough_subgraphs(
                    port_u16,
                    &diag_buf.inputs,
                    &diag_buf.predictions,
                    &diag_buf.node_outputs,
                    replay_initial_regs.as_deref(),
                    self.economy_config.split_code_tax,
                );
                self.port_states[p].diagnostic_buffer = diag_buf;
                if let Ok(Some(old_to_new)) = excise_res {
                    self.ledger.on_prune(
                        self.editable.circuit(),
                        self.economy_config.split_code_tax,
                        &old_to_new,
                    );
                    self.clear_changed_port_diagnostics_preserving_scorer(
                        &pre_prune_circuit,
                        &old_to_new,
                    );
                    self.apply_node_remap_to_windows(&old_to_new);
                    self.idle_prune_events += 1;
                    self.local_excision_events += 1;
                    self.tick += 1;
                    return None;
                }
            }

            // Check if Snapshot 2 needs to be recorded after this port's Sprout 2 has settled
            if self.port_states[p].port_sprout_events == 2
                && self.port_states[p].epoch_snapshots.len() == 2
            {
                self.record_epoch_snapshot(p, 2, false);
            }

            let mut sprouted = false;
            let port_zero_error = self.is_port_zero_error(p);
            if !self.port_states[p].stage2_exhausted
                && self.tick >= self.port_states[p].stage2_mature_at_tick
            {
                let mut port_budget = if num_ports == 1 {
                    *budget
                } else {
                    TickBudget::new(self.port_budget_allowance(p, budget))
                };

                // 2a. Stage 0 candidate motif sprouting (up to GrowthConfig::max_reactive_lag)
                enum DeferredProposal {
                    Sprout(ScoredSproutProposal),
                    Ring(AutonomousRingProposal, f64),
                    ComparatorHandoff(usize, f64),
                }
                impl DeferredProposal {
                    fn j_score(&self) -> f64 {
                        match self {
                            Self::Sprout(prop) => prop.j_cand,
                            Self::Ring(_, j) | Self::ComparatorHandoff(_, j) => *j,
                        }
                    }
                }

                let mut stage0_tel = SproutTelemetry::default();
                let can_potentially_deepen = num_ports > 1
                    && !port_zero_error
                    && self.growth_config.deepening_max_lag > self.growth_config.max_reactive_lag
                    && port_budget.remaining() > 0;
                let mut deferred_lag_bound_tau: Option<u8> = None;
                let (stage0_res, stage0_pending_bound) = match propose_sprout_band_scored_internal(
                    &self.editable,
                    &self.obs_window,
                    &self.port_states[p].diagnostic_buffer,
                    &self.growth_config,
                    &self.economy_config,
                    &mut port_budget,
                    port_u16,
                    1,
                    self.growth_config.max_reactive_lag,
                    &mut stage0_tel,
                    !can_potentially_deepen,
                ) {
                    Ok((prop, pending)) => (Ok(prop), pending),
                    Err(e) => (Err(e), false),
                };
                if stage0_pending_bound {
                    deferred_lag_bound_tau = Some(self.growth_config.max_reactive_lag);
                }
                let can_deepen_eligible = num_ports > 1
                    && !port_zero_error
                    && self.growth_config.deepening_max_lag > self.growth_config.max_reactive_lag
                    && stage0_tel.sprout_blocked_max_nodes == 0
                    && stage0_tel.sprout_budget_exhausted == 0
                    && port_budget.remaining() > 0;

                let mut deferred_best: Option<DeferredProposal> = match stage0_res {
                    Ok(Some(prop)) if prop.e_cand == 0 || !can_deepen_eligible => {
                        sprouted =
                            self.commit_sprout(p, prop.circuit, prop.effective_juvenile_ticks);
                        None
                    }
                    Ok(Some(prop)) => Some(DeferredProposal::Sprout(prop)),
                    _ => None,
                };

                // 2b. Autonomous delay ring proposal under noise (checked BEFORE lag deepening per C2-a)
                let mut ring_tel = SproutTelemetry::default();
                if !sprouted {
                    if let Some((comp_idx, e_comp_diag, j_comp_diag)) =
                        evaluate_comparator_ring_promotion(
                            &self.editable,
                            &self.obs_window,
                            &self.growth_config,
                            &self.economy_config,
                            &mut port_budget,
                            port_u16,
                            &mut ring_tel,
                        )
                    {
                        if deferred_best.is_none() || e_comp_diag == 0 {
                            sprouted = self.perform_comparator_handoff(comp_idx);
                        } else if deferred_best
                            .as_ref()
                            .is_none_or(|prev| j_comp_diag < prev.j_score())
                        {
                            deferred_best =
                                Some(DeferredProposal::ComparatorHandoff(comp_idx, j_comp_diag));
                        }
                    } else if let Some((ring_prop, e_ring_diag, j_ring_diag)) =
                        propose_autonomous_delay_ring_scored_with_telemetry(
                            &self.editable,
                            &self.obs_window,
                            &self.growth_config,
                            &self.economy_config,
                            &mut port_budget,
                            port_u16,
                            &mut ring_tel,
                        )
                    {
                        if deferred_best.is_none() || e_ring_diag == 0 {
                            sprouted = self.commit_ring_proposal(ring_prop);
                        } else if deferred_best
                            .as_ref()
                            .is_none_or(|prev| j_ring_diag < prev.j_score())
                        {
                            deferred_best = Some(DeferredProposal::Ring(ring_prop, j_ring_diag));
                        }
                    }
                }

                // 2c. Progressive lag-deepening incremental bands (3..=6], (6..=12] per C2-a
                let mut last_band_tel: Option<SproutTelemetry> = None;
                let mut band_blocked = 0u64;
                let mut band_exhausted = 0u64;
                if !sprouted && can_deepen_eligible {
                    let mut bands = Vec::new();
                    let mut prev_max = self.growth_config.max_reactive_lag;
                    for &cap in &[6u8, 12u8] {
                        let band_max = cap.min(self.growth_config.deepening_max_lag);
                        if band_max > prev_max {
                            bands.push((prev_max + 1, band_max));
                            prev_max = band_max;
                        }
                    }
                    if self.growth_config.deepening_max_lag > prev_max {
                        bands.push((prev_max + 1, self.growth_config.deepening_max_lag));
                    }

                    let num_bands = bands.len();
                    for (band_idx, &(min_tau, max_tau)) in bands.iter().enumerate() {
                        if port_budget.remaining() == 0 {
                            break;
                        }
                        self.lag_deepening_steps += 1;
                        deferred_lag_bound_tau = None;
                        let mut band_tel = SproutTelemetry::default();
                        let is_last_band = band_idx + 1 == num_bands;
                        let (band_res, band_pending_bound) =
                            match propose_sprout_band_scored_internal(
                                &self.editable,
                                &self.obs_window,
                                &self.port_states[p].diagnostic_buffer,
                                &self.growth_config,
                                &self.economy_config,
                                &mut port_budget,
                                port_u16,
                                min_tau,
                                max_tau,
                                &mut band_tel,
                                is_last_band,
                            ) {
                                Ok((prop, pending)) => (Ok(prop), pending),
                                Err(e) => (Err(e), false),
                            };
                        if band_pending_bound {
                            deferred_lag_bound_tau = Some(max_tau);
                        }
                        band_blocked += band_tel.sprout_blocked_max_nodes;
                        band_exhausted += band_tel.sprout_budget_exhausted;
                        let band_stopped = band_tel.sprout_blocked_max_nodes > 0
                            || band_tel.sprout_budget_exhausted > 0;
                        last_band_tel = Some(band_tel);

                        if let Ok(Some(band_prop)) = band_res {
                            if band_prop.e_cand == 0 {
                                sprouted = self.commit_sprout(
                                    p,
                                    band_prop.circuit,
                                    band_prop.effective_juvenile_ticks,
                                );
                                break;
                            }
                            if deferred_best
                                .as_ref()
                                .is_none_or(|prev| band_prop.j_cand < prev.j_score())
                            {
                                deferred_best = Some(DeferredProposal::Sprout(band_prop));
                            }
                        }
                        if band_stopped {
                            break;
                        }
                    }
                }

                if !sprouted && let Some(winner) = deferred_best {
                    sprouted = match winner {
                        DeferredProposal::Sprout(prop) => {
                            self.commit_sprout(p, prop.circuit, prop.effective_juvenile_ticks)
                        }
                        DeferredProposal::Ring(ring_prop, _) => {
                            self.commit_ring_proposal(ring_prop)
                        }
                        DeferredProposal::ComparatorHandoff(comp_idx, _) => {
                            self.perform_comparator_handoff(comp_idx)
                        }
                    };
                }

                if num_ports == 1 {
                    *budget = port_budget;
                } else {
                    let spent = port_budget.consumed();
                    let _ = budget.try_consume(spent);
                    self.port_states[p].replay_ticks_consumed += spent;
                }

                self.sprout_blocked_max_nodes += stage0_tel.sprout_blocked_max_nodes
                    + ring_tel.sprout_blocked_max_nodes
                    + if sprouted { 0 } else { band_blocked };
                let stage2_budget_exhausted = stage0_tel.sprout_budget_exhausted
                    + ring_tel.sprout_budget_exhausted
                    + if sprouted { 0 } else { band_exhausted };
                self.sprout_budget_exhausted += stage2_budget_exhausted;
                self.ring_period_bound += ring_tel.ring_period_bound;

                let bound_tel = last_band_tel.as_ref().unwrap_or(&stage0_tel);
                self.candidate_undecidable +=
                    bound_tel.candidate_undecidable + ring_tel.candidate_undecidable;
                self.candidate_list_truncated +=
                    bound_tel.candidate_list_truncated + ring_tel.candidate_list_truncated;
                if !sprouted {
                    let mut bound = bound_tel.lag_horizon_bound + ring_tel.lag_horizon_bound;
                    if let Some(tau_max) = deferred_lag_bound_tau
                        && check_deeper_lag_for_port(
                            &self.editable,
                            &self.obs_window,
                            &self.port_states[p].diagnostic_buffer,
                            &self.growth_config,
                            &self.economy_config,
                            port_u16,
                            tau_max,
                        )
                    {
                        bound += 1;
                    }
                    self.lag_horizon_bound += bound;
                    let obs_full = self.obs_window.len()
                        >= observation_horizon(&self.economy_config, &self.growth_config);
                    if obs_full
                        && ((!port_zero_error && stage2_budget_exhausted == 0)
                            || (self.is_port_driven(port_u16)
                                && !self.port_q_hat_exceeds_null(p, predictions[p])))
                    {
                        self.port_states[p].stage2_exhausted = true;
                    }
                }
            }
            self.port_states[p].stage2_checks_completed += 1;

            if sprouted {
                sprouted_any = true;
                break;
            }
        }

        // Pass 2: Causal-disjoint redundancy sprouting (Stage 3, M2B-D2) runs only if Pass 1 did not sprout.
        if !sprouted_any && self.growth_config.redundancy_sprouting {
            for offset in 0..num_ports {
                let p = (start_port + offset) % num_ports;
                let port_u16 = p as u16;

                let is_primary_mature = port_primary_cone_is_mature(
                    self.editable.circuit(),
                    self.editable.economy(),
                    port_u16,
                );
                let diag_zero_res = self.port_states[p]
                    .diagnostic_buffer
                    .residuals
                    .iter()
                    .all(|frame| frame[p] == 0);
                let ready_for_stage3_only = is_primary_mature && diag_zero_res;
                let ran_pass1 = pass1_timers[p].is_some();
                let is_primed = port_is_primed(self.editable.circuit(), base_ancestry, port_u16);
                let buf_ready = self.port_states[p].diagnostic_buffer.len()
                    >= self.growth_config.diagnostic_window;
                let timer_ready = ran_pass1
                    || self.port_states[p].ticks_since_last_check
                        >= self.growth_config.sprout_interval;

                if !((ran_pass1 || ready_for_stage3_only)
                    && is_primed
                    && buf_ready
                    && timer_ready
                    && self.tick >= self.port_states[p].stage2_mature_at_tick)
                {
                    continue;
                }

                let prev_ticks_since_check =
                    pass1_timers[p].unwrap_or(self.port_states[p].ticks_since_last_check);
                self.port_states[p].ticks_since_last_check = 0;

                let stage3_eligible = !self.port_states[p].stage3_exhausted
                    && self.is_port_driven(port_u16)
                    && port_comparators_mature(
                        self.editable.circuit(),
                        self.editable.economy(),
                        port_u16,
                    )
                    && self.port_comparators_primed(comp_ancestry, port_u16)
                    && self.port_q_hat_exceeds_null(p, predictions[p])
                    && self.port_redundancy_arity_eligible(port_u16);
                let sibling_waiting_stage2 = (0..num_ports)
                    .any(|q| q != p && self.port_states[q].stage2_checks_completed == 0);
                if stage3_eligible && (self.sibling_has_juvenile_nodes(p) || sibling_waiting_stage2)
                {
                    self.port_states[p].ticks_since_last_check = prev_ticks_since_check;
                } else if stage3_eligible {
                    let any_sibling_needs_stage2 =
                        (0..num_ports).any(|q| q != p && self.port_needs_stage2(q));
                    let mut stage3_tel = SproutTelemetry::default();
                    let mut stage3_ok_none = false;
                    let mut sprouted = false;
                    if num_ports == 1 {
                        match propose_redundancy_sprout(
                            &self.editable,
                            &self.obs_window,
                            &self.port_states[p].diagnostic_buffer,
                            &self.growth_config,
                            &self.economy_config,
                            budget,
                            port_u16,
                            &mut stage3_tel,
                        ) {
                            Ok(Some(prop)) => {
                                sprouted = self.commit_redundancy_sprout(p, prop);
                            }
                            Ok(None) => stage3_ok_none = true,
                            Err(_) => {}
                        }
                    } else {
                        let total_budget = self.total_budget_seen.unwrap_or_else(|| {
                            budget.remaining().saturating_add(budget.consumed())
                        });
                        let base_share = total_budget / (num_ports as u64);
                        let cap = if any_sibling_needs_stage2 {
                            base_share
                                .saturating_sub(self.port_states[p].replay_ticks_consumed)
                                .min(budget.remaining())
                        } else {
                            self.port_budget_allowance(p, budget)
                        };
                        let mut stage3_budget = TickBudget::new(cap);
                        match propose_redundancy_sprout(
                            &self.editable,
                            &self.obs_window,
                            &self.port_states[p].diagnostic_buffer,
                            &self.growth_config,
                            &self.economy_config,
                            &mut stage3_budget,
                            port_u16,
                            &mut stage3_tel,
                        ) {
                            Ok(Some(prop)) => {
                                sprouted = self.commit_redundancy_sprout(p, prop);
                            }
                            Ok(None) => stage3_ok_none = true,
                            Err(_) => {}
                        }
                        let spent = stage3_budget.consumed();
                        let _ = budget.try_consume(spent);
                        self.port_states[p].replay_ticks_consumed += spent;
                    }
                    if !sprouted {
                        self.sprout_blocked_max_nodes += stage3_tel.sprout_blocked_max_nodes;
                        self.sprout_budget_exhausted += stage3_tel.sprout_budget_exhausted;
                        let obs_full = self.obs_window.len()
                            >= observation_horizon(&self.economy_config, &self.growth_config);
                        if obs_full
                            && stage3_ok_none
                            && stage3_tel.sprout_budget_exhausted == 0
                            && (self.is_port_zero_error(p) || !self.port_needs_stage2(p))
                        {
                            self.port_states[p].stage3_exhausted = true;
                        }
                    }
                    self.candidate_undecidable += stage3_tel.candidate_undecidable;
                    self.lag_horizon_bound += stage3_tel.lag_horizon_bound;
                    self.candidate_list_truncated += stage3_tel.candidate_list_truncated;
                    if sprouted {
                        break;
                    }
                }
            }
        }

        self.tick += 1;
        None
    }

    fn best_circuit(&self) -> Option<&Circuit> {
        Some(self.editable.circuit())
    }

    fn split_code_tax(&self) -> bool {
        self.economy_config.split_code_tax
    }

    fn m1_telemetry(&self) -> Option<ArkheM1Telemetry> {
        Some(telemetry_ref(self))
    }
}

fn telemetry_ref(learner: &ArkheLearner) -> ArkheM1Telemetry {
    learner.telemetry()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::{FiniteStateGenerator, GeneratorConfig, NoiseModel, NoisyEnvironment};

    #[test]
    fn ticks_per_hop_multistage_tasks_run_without_missing_ancestry_panic() {
        let mut learner = ArkheLearner::new(
            Width::BITS_1,
            1,
            WireDelayMode::TicksPerHop,
            GrowthConfig::default(),
            SeedBank::new(7),
        )
        .unwrap();
        let mut budget = TickBudget::new(200_000);
        let motif = [0u64, 1, 1];
        for t in 0..800 {
            let _ = learner.predict(&mut budget);
            learner.observe(&[motif[t % 3]], &mut budget);
        }
        assert!(learner.best_circuit().unwrap().compile().is_ok());
        assert!(learner.sprout_events() >= 1);
    }

    #[test]
    fn noisy_nonzero_constants_halt_at_zero_nodes_without_thrashing() {
        let cases = [(Width::BITS_1, 1u64), (Width::BITS_2, 3u64)];
        for (width, val) in cases {
            for seed in 0..10u64 {
                let seeds = SeedBank::new(seed);
                let mut learner = ArkheLearner::new(
                    width,
                    1,
                    WireDelayMode::Combinational,
                    GrowthConfig::default(),
                    seeds,
                )
                .unwrap();
                let generator = GeneratorConfig::T1_Constant { value: val }
                    .build(width)
                    .unwrap();
                let mut env = NoisyEnvironment::new(
                    generator,
                    NoiseModel::IndependentSubstitution {
                        epsilon: 0.05,
                        channels: vec![0],
                    },
                    seeds,
                )
                .unwrap();
                let mut budget = TickBudget::new(300_000);
                let mut clean = [0u64];
                let mut obs = [0u64];
                for _ in 0..1024 {
                    let _ = learner.predict(&mut budget);
                    env.step_frame(&mut clean, &mut obs);
                    learner.observe(&obs, &mut budget);
                }
                let tel = learner.telemetry();
                assert_eq!(
                    tel.sprout_events,
                    0,
                    "width={} val={val} seed={seed} sprouted on noisy constant",
                    width.bits()
                );
                assert_eq!(tel.starvation_prune_events, 0);
                assert_eq!(tel.final_node_count, 0);
            }
        }
    }

    #[test]
    fn sparse_rare_event_ring_discovery_and_mdl_sign_convention() {
        let mut learner = ArkheLearner::new(
            Width::BITS_1,
            1,
            WireDelayMode::Combinational,
            GrowthConfig::default(),
            SeedBank::new(11),
        )
        .unwrap();
        let mut generator = GeneratorConfig::T1_PeriodicMotif {
            motif: vec![0, 0, 0, 0, 1],
        }
        .build(Width::BITS_1)
        .unwrap();
        let mut budget = TickBudget::new(300_000);
        let mut tail_matches = 0usize;
        let mut truth = [0u64];
        for t in 0..800 {
            let pred = learner.predict(&mut budget);
            generator.next_clean_frame(&mut truth);
            if t >= 400 && pred[0] == truth[0] {
                tail_matches += 1;
            }
            learner.observe(&truth, &mut budget);
        }
        let tel = learner.telemetry();
        assert!(tel.ring_closure_events >= 1);
        assert_eq!(tel.starvation_prune_events, 0);
        assert_eq!(tail_matches, 400);
        assert!(
            tel.null_calibrated_mdl_delta_bits < 0.0,
            "compression must yield negative MDL delta, got {}",
            tel.null_calibrated_mdl_delta_bits
        );
    }

    #[test]
    fn two_channel_ring_closure_preserves_other_channel_subcircuit() {
        let mut learner = ArkheLearner::new(
            Width::BITS_1,
            2,
            WireDelayMode::Combinational,
            GrowthConfig::default(),
            SeedBank::new(23),
        )
        .unwrap();
        let mut budget = TickBudget::new(500_000);
        let motif_ch1 = [0u64, 0, 0, 0, 1];
        let mut ch0_tail_matches = 0usize;
        let mut ch1_tail_matches = 0usize;
        for t in 0..900 {
            let pred = learner.predict(&mut budget);
            let truth = [(t % 2) as u64, motif_ch1[t % 5]];
            if t >= 500 {
                if pred[0] == truth[0] {
                    ch0_tail_matches += 1;
                }
                if pred[1] == truth[1] {
                    ch1_tail_matches += 1;
                }
            }
            learner.observe(&truth, &mut budget);
        }
        assert!(learner.sprout_events() >= 1);
        assert!(learner.ring_closure_events() >= 1);
        assert_eq!(
            ch0_tail_matches, 400,
            "channel 0 subcircuit must survive channel 1 ring closure"
        );
        assert_eq!(
            ch1_tail_matches, 400,
            "channel 1 ring must lock phase cleanly"
        );
    }

    #[test]
    fn s3a_neg_add_const_solves_three_x_plus_one_t2_delayed_affine_k2() {
        use crate::env::LfsrStage;
        use crate::substrate::Primitive;

        let driver = LfsrStage {
            taps: vec![0, 1, 0, 0, 1], // x^5 + x^2 + 1 primitive
            init: vec![1, 0, 1, 1, 0],
        };
        let mut generator = GeneratorConfig::T2_DelayedAffine {
            driver,
            lag: 1,
            mul_a: 3,
            add_b: 1,
        }
        .build(Width::BITS_2)
        .unwrap();

        let mut learner = ArkheLearner::new(
            Width::BITS_2,
            2,
            WireDelayMode::Combinational,
            GrowthConfig::default(),
            SeedBank::new(5),
        )
        .unwrap();
        let mut budget = TickBudget::new(200_000);
        let mut truth = [0u64, 0];
        let mut ch1_tail_matches = 0usize;
        for t in 0..1200 {
            let pred = learner.predict(&mut budget);
            generator.next_clean_frame(&mut truth);
            if t >= 700 && pred[1] == truth[1] {
                ch1_tail_matches += 1;
            }
            learner.observe(&truth, &mut budget);
        }
        assert_eq!(ch1_tail_matches, 500);
        let has_sub = learner
            .best_circuit()
            .unwrap()
            .nodes()
            .iter()
            .any(|node| matches!(node, Primitive::Sub));
        assert!(
            has_sub,
            "expected NegAddConst to materialize Primitive::Sub"
        );
    }

    #[test]
    fn tph_arm_offset_alignment_solves_all_five_m2a_tasks() {
        use crate::env::{CrossLinearOp, CrossNonlinearOp, LfsrStage};

        let d0 = LfsrStage {
            taps: vec![0, 1, 0, 0, 1], // deg 5 primitive
            init: vec![1, 1, 0, 1, 0],
        };
        let d1 = LfsrStage {
            taps: vec![1, 0, 0, 1], // deg 4 primitive
            init: vec![1, 0, 1, 1],
        };

        let cases: Vec<(Width, &str, usize, GeneratorConfig)> = vec![
            (
                Width::BITS_2,
                "DelayedAffine(d=3)",
                2,
                GeneratorConfig::T2_DelayedAffine {
                    driver: d0.clone(),
                    lag: 3,
                    mul_a: 3,
                    add_b: 1,
                },
            ),
            (
                Width::BITS_2,
                "CrossSum(d=3,3)",
                3,
                GeneratorConfig::T2_CrossLinear {
                    driver_0: d0.clone(),
                    driver_1: d1.clone(),
                    lag_0: 3,
                    lag_1: 3,
                    op: CrossLinearOp::Add,
                },
            ),
            (
                Width::BITS_2,
                "CrossSum(d=3,5)",
                3,
                GeneratorConfig::T2_CrossLinear {
                    driver_0: d0.clone(),
                    driver_1: d1.clone(),
                    lag_0: 3,
                    lag_1: 5,
                    op: CrossLinearOp::Add,
                },
            ),
            (
                Width::BITS_2,
                "AddEqz(d=3,4)",
                3,
                GeneratorConfig::T2_CrossNonlinear {
                    driver_0: d0.clone(),
                    driver_1: d1.clone(),
                    lag_0: 3,
                    lag_1: 4,
                    op: CrossNonlinearOp::AddEqz,
                },
            ),
            (
                Width::BITS_2,
                "MulEqz(d=3,4)",
                3,
                GeneratorConfig::T2_CrossNonlinear {
                    driver_0: d0.clone(),
                    driver_1: d1.clone(),
                    lag_0: 3,
                    lag_1: 4,
                    op: CrossNonlinearOp::MulEqz,
                },
            ),
            (
                Width::BITS_1,
                "MulEqzPair(d=4,4)",
                3,
                GeneratorConfig::T2_CrossNonlinear {
                    driver_0: d0,
                    driver_1: d1,
                    lag_0: 4,
                    lag_1: 4,
                    op: CrossNonlinearOp::MulEqzPair,
                },
            ),
        ];

        for (width, label, k_ports, gen_cfg) in cases {
            let mut generator = gen_cfg.build(width).unwrap();
            let mut learner = ArkheLearner::new(
                width,
                k_ports as u16,
                WireDelayMode::TicksPerHop,
                GrowthConfig::default(),
                SeedBank::new(13),
            )
            .unwrap();
            let mut budget = TickBudget::new(250_000);
            let mut truth = vec![0u64; k_ports];
            let target_port = k_ports - 1;
            let mut tail_matches = 0usize;
            for t in 0..1600 {
                let pred = learner.predict(&mut budget);
                generator.next_clean_frame(&mut truth);
                if t >= 1100 && pred[target_port] == truth[target_port] {
                    tail_matches += 1;
                }
                learner.observe(&truth, &mut budget);
            }
            let circuit = learner.best_circuit().unwrap();
            assert!(
                circuit.edges().iter().all(|e| e.hops == 1),
                "{label}: every edge in TicksPerHop must have edge.hops == 1"
            );
            assert_eq!(
                tail_matches, 500,
                "{label}: expected 500/500 tail matches, got {tail_matches}"
            );
        }
    }

    #[test]
    fn c1a_epoch1_cross_channel_selection_solves_nonlinear_mul_eqz() {
        use crate::env::{CrossNonlinearOp, LfsrStage};

        let d0 = LfsrStage {
            taps: vec![0, 1, 0, 0, 1],
            init: vec![1, 0, 1, 1, 0],
        };
        let d1 = LfsrStage {
            taps: vec![1, 0, 0, 1],
            init: vec![1, 1, 0, 1],
        };
        let mut generator = GeneratorConfig::T2_CrossNonlinear {
            driver_0: d0,
            driver_1: d1,
            lag_0: 1,
            lag_1: 2,
            op: CrossNonlinearOp::MulEqz,
        }
        .build(Width::BITS_2)
        .unwrap();

        let mut learner = ArkheLearner::new(
            Width::BITS_2,
            3,
            WireDelayMode::Combinational,
            GrowthConfig::default(),
            SeedBank::new(17),
        )
        .unwrap();
        let mut budget = TickBudget::new(200_000);
        let mut truth = [0u64; 3];
        let mut tail_matches = 0usize;
        for t in 0..1200 {
            let pred = learner.predict(&mut budget);
            generator.next_clean_frame(&mut truth);
            if t >= 700 && pred[2] == truth[2] {
                tail_matches += 1;
            }
            learner.observe(&truth, &mut budget);
        }
        assert!(learner.sprout_events() >= 1);
        assert_eq!(tail_matches, 500);
    }

    #[test]
    fn c2a_ring_before_deepening_closes_ring_first_and_locks_deep_lags() {
        use crate::env::LfsrStage;

        // 1. Rare-event period-5 ring closes via C2-a Stage 0 -> Ring BEFORE any deepening step.
        let mut ring_learner = ArkheLearner::new(
            Width::BITS_1,
            1,
            WireDelayMode::Combinational,
            GrowthConfig::default(),
            SeedBank::new(11),
        )
        .unwrap();
        let mut ring_gen = GeneratorConfig::T1_PeriodicMotif {
            motif: vec![0, 0, 0, 0, 1],
        }
        .build(Width::BITS_1)
        .unwrap();
        let mut ring_budget = TickBudget::new(200_000);
        let mut truth1 = [0u64];
        let mut ring_tail = 0usize;
        for t in 0..800 {
            let pred = ring_learner.predict(&mut ring_budget);
            ring_gen.next_clean_frame(&mut truth1);
            if t >= 400 && pred[0] == truth1[0] {
                ring_tail += 1;
            }
            ring_learner.observe(&truth1, &mut ring_budget);
        }
        let ring_tel = ring_learner.telemetry();
        assert!(ring_tel.ring_closure_events >= 1);
        assert_eq!(
            ring_tel.lag_deepening_steps, 0,
            "ring closure must run before progressive lag deepening"
        );
        assert_eq!(ring_tail, 400);

        // 2. Non-aliasing deep T2 tasks (d = 5, 6, 8) lock via progressive deepening.
        let driver = LfsrStage {
            taps: vec![0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1], // x^11 + x^9 + 1 primitive (taps at lags 9, 11)
            init: vec![1, 0, 1, 1, 0, 1, 0, 1, 1, 0, 1],
        };
        for width in [Width::BITS_1, Width::BITS_2] {
            for lag in [5usize, 6, 8] {
                let mut gen_deep = GeneratorConfig::T2_DelayedAffine {
                    driver: driver.clone(),
                    lag,
                    mul_a: 1,
                    add_b: 1,
                }
                .build(width)
                .unwrap();
                let mut learner = ArkheLearner::new(
                    width,
                    2,
                    WireDelayMode::Combinational,
                    GrowthConfig::default(),
                    SeedBank::new(19 + lag as u64),
                )
                .unwrap();
                let mut budget = TickBudget::new(200_000);
                let mut truth = [0u64; 2];
                let mut tail = 0usize;
                for t in 0..1600 {
                    let pred = learner.predict(&mut budget);
                    gen_deep.next_clean_frame(&mut truth);
                    if t >= 1100 && pred[1] == truth[1] {
                        tail += 1;
                    }
                    learner.observe(&truth, &mut budget);
                }
                let tel = learner.telemetry();
                assert!(
                    tel.lag_deepening_steps >= 1,
                    "k={} lag={lag}: expected lag_deepening_steps >= 1",
                    width.bits()
                );
                assert_eq!(
                    tail,
                    500,
                    "k={} lag={lag}: expected 500/500 tail matches, got {tail}",
                    width.bits()
                );
            }
        }

        // 3. Unreachable deep lag (d = 15 > deepening_max_lag = 12) at Width::BITS_1 with
        //    default GrowthConfig emits lag_horizon_bound at most once per multi-band check.
        let mut gen_unreach = GeneratorConfig::T2_DelayedAffine {
            driver,
            lag: 15,
            mul_a: 1,
            add_b: 0,
        }
        .build(Width::BITS_1)
        .unwrap();
        let mut learner_unreach = ArkheLearner::new(
            Width::BITS_1,
            2,
            WireDelayMode::Combinational,
            GrowthConfig::default(),
            SeedBank::new(41),
        )
        .unwrap();
        let mut budget_unreach = TickBudget::new(200_000);
        let mut truth_u = [0u64; 2];
        for _ in 0..180 {
            let _ = learner_unreach.predict(&mut budget_unreach);
            gen_unreach.next_clean_frame(&mut truth_u);
            learner_unreach.observe(&truth_u, &mut budget_unreach);
        }
        let tel_u = learner_unreach.telemetry();
        assert!(tel_u.lag_deepening_steps >= 2);
        assert!(tel_u.lag_horizon_bound >= 1);
    }

    #[test]
    fn s2_per_port_tick_budget_fairness_prevents_starvation_and_redistributes_idle_share() {
        use crate::env::LfsrStage;

        // 1. Noisy unlearnable channel 0 must not starve learnable channel 1.
        let mut noisy_ch0 = NoisyEnvironment::new(
            GeneratorConfig::T1_Constant { value: 0 }
                .build(Width::BITS_2)
                .unwrap(),
            NoiseModel::IndependentSubstitution {
                epsilon: 0.75,
                channels: vec![0],
            },
            SeedBank::new(71),
        )
        .unwrap();
        let mut alt_ch1 = GeneratorConfig::T1_Alternating { a: 0, b: 1 }
            .build(Width::BITS_2)
            .unwrap();
        let mut learner = ArkheLearner::new(
            Width::BITS_2,
            2,
            WireDelayMode::Combinational,
            GrowthConfig::default(),
            SeedBank::new(73),
        )
        .unwrap();
        let mut budget = TickBudget::new(100_000);
        let mut c0 = [0u64];
        let mut o0 = [0u64];
        let mut c1 = [0u64];
        let mut ch1_tail = 0usize;
        for t in 0..900 {
            let pred = learner.predict(&mut budget);
            noisy_ch0.step_frame(&mut c0, &mut o0);
            alt_ch1.next_clean_frame(&mut c1);
            if t >= 400 && pred[1] == c1[0] {
                ch1_tail += 1;
            }
            learner.observe(&[o0[0], c1[0]], &mut budget);
        }
        assert_eq!(
            ch1_tail, 500,
            "learnable channel 1 must lock despite noisy channel 0 consuming its port share"
        );

        // 2. Idle zero_error port 0 redistributes its unused share to deep active port 1.
        let mut gen_deep = GeneratorConfig::T2_DelayedAffine {
            driver: LfsrStage {
                taps: vec![1, 0, 0, 0, 0, 0, 1],
                init: vec![1, 0, 1, 1, 0, 1, 0],
            },
            lag: 15,
            mul_a: 1,
            add_b: 1,
        }
        .build(Width::BITS_2)
        .unwrap();
        let mut const0 = GeneratorConfig::T1_Constant { value: 0 }
            .build(Width::BITS_2)
            .unwrap();
        let mut idle_learner = ArkheLearner::new(
            Width::BITS_2,
            2,
            WireDelayMode::Combinational,
            GrowthConfig::default(),
            SeedBank::new(79),
        )
        .unwrap();
        let total_budget = 20_000u64;
        let mut idle_budget = TickBudget::new(total_budget);
        let mut f0 = [0u64];
        let mut f_deep = [0u64; 2];
        for _ in 0..500 {
            let _ = idle_learner.predict(&mut idle_budget);
            const0.next_clean_frame(&mut f0);
            gen_deep.next_clean_frame(&mut f_deep);
            idle_learner.observe(&[f0[0], f_deep[1]], &mut idle_budget);
        }
        assert_eq!(idle_learner.port_states[0].replay_ticks_consumed, 0);
        assert!(
            idle_learner.port_states[1].replay_ticks_consumed > total_budget / 2,
            "active port 1 ({}) should inherit unused budget beyond base_share ({})",
            idle_learner.port_states[1].replay_ticks_consumed,
            total_budget / 2
        );
    }

    #[test]
    fn deep_d6_exact_rule_beats_transient_lossy_stage0_correlation_at_t63() {
        use crate::env::LfsrStage;

        let mut gen_deep = GeneratorConfig::T2_DelayedAffine {
            driver: LfsrStage {
                taps: vec![0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1],
                init: vec![1, 0, 1, 1, 0, 1, 0, 1, 1, 0, 1],
            },
            lag: 6,
            mul_a: 1,
            add_b: 1,
        }
        .build(Width::BITS_1)
        .unwrap();
        let mut learner = ArkheLearner::new(
            Width::BITS_1,
            2,
            WireDelayMode::Combinational,
            GrowthConfig::default(),
            SeedBank::new(0),
        )
        .unwrap();
        let mut budget = TickBudget::new(200_000);
        let mut truth = [0u64; 2];
        let mut mismatches_after_70 = 0usize;
        for t in 0..800 {
            let pred = learner.predict(&mut budget);
            gen_deep.next_clean_frame(&mut truth);
            if t >= 70 && pred[1] != truth[1] {
                mismatches_after_70 += 1;
            }
            learner.observe(&truth, &mut budget);
        }
        let tel = learner.telemetry();
        assert_eq!(
            mismatches_after_70, 0,
            "deep d=6 must immediately lock after t=63 without stalling on transient Stage-0 correlation"
        );
        assert!(tel.lag_deepening_steps >= 1);
        assert_eq!(tel.starvation_prune_events, 0);
    }

    #[test]
    fn redundancy_sprouting_disabled_is_bit_identical_to_all_golden_fixtures() {
        use crate::env::{GeneratorConfig, NoiseModel, NoisyEnvironment};

        struct Fnv1a {
            hash: u128,
        }
        impl Fnv1a {
            fn new() -> Self {
                Self {
                    hash: 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d,
                }
            }
            fn write(&mut self, bytes: &[u8]) {
                const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;
                for &b in bytes {
                    self.hash ^= u128::from(b);
                    self.hash = self.hash.wrapping_mul(PRIME);
                }
            }
            fn finish_hex(&self) -> String {
                format!("{:032x}", self.hash)
            }
        }

        #[derive(Deserialize)]
        struct GoldenStub {
            trajectory_digest_fnv1a: String,
        }

        // Combinational fixtures are re-blessed under M2B-D3 + Rule 4 for Stage 3 comparators
        // (`redundancy_sprouting: true`), so pin their exact pre-M2b (8ca68a2) digests here while
        // keeping TicksPerHop fixtures checked directly against `tests/fixtures/golden/`.
        let cases = [
            (
                "alt_k1_comb_s20",
                GeneratorConfig::T1_Alternating { a: 0, b: 1 },
                Width::BITS_1,
                WireDelayMode::Combinational,
                20u64,
                400u64,
                Some("fb7491d27d1417a707a3bcdea8b1a402"),
            ),
            (
                "alt_k1_tph_s20",
                GeneratorConfig::T1_Alternating { a: 0, b: 1 },
                Width::BITS_1,
                WireDelayMode::TicksPerHop,
                20u64,
                400u64,
                None,
            ),
            (
                "alt_k2_comb_s21",
                GeneratorConfig::T1_Alternating { a: 0, b: 3 },
                Width::BITS_2,
                WireDelayMode::Combinational,
                21u64,
                400u64,
                Some("6040a5b86c2b7aeb8b2e8cf18cb7bc0b"),
            ),
            (
                "alt_k2_tph_s21",
                GeneratorConfig::T1_Alternating { a: 0, b: 3 },
                Width::BITS_2,
                WireDelayMode::TicksPerHop,
                21u64,
                400u64,
                None,
            ),
            (
                "motif011_k1_comb_s22",
                GeneratorConfig::T1_PeriodicMotif {
                    motif: vec![0, 1, 1],
                },
                Width::BITS_1,
                WireDelayMode::Combinational,
                22u64,
                500u64,
                Some("bf33dc95e006382067d40ee87ce2dded"),
            ),
            (
                "motif013_k2_comb_s22",
                GeneratorConfig::T1_PeriodicMotif {
                    motif: vec![0, 1, 3],
                },
                Width::BITS_2,
                WireDelayMode::Combinational,
                22u64,
                500u64,
                Some("d2268bc3dd78fc545a533ac2522e9850"),
            ),
            (
                "affine_k2_comb_s23",
                GeneratorConfig::T1_AffineLag1 {
                    mul_a: 1,
                    add_b: 1,
                    init: 0,
                },
                Width::BITS_2,
                WireDelayMode::Combinational,
                23u64,
                500u64,
                Some("0bc8774428ff3a39aa4cf1a384953df1"),
            ),
            (
                "affine_k2_tph_s23",
                GeneratorConfig::T1_AffineLag1 {
                    mul_a: 1,
                    add_b: 1,
                    init: 0,
                },
                Width::BITS_2,
                WireDelayMode::TicksPerHop,
                23u64,
                500u64,
                None,
            ),
        ];

        for (name, gen_cfg, width, wire_delay, seed, horizon, pinned_phase1_digest) in cases {
            let expected_digest = if let Some(pinned) = pinned_phase1_digest {
                pinned.to_owned()
            } else {
                let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("tests")
                    .join("fixtures")
                    .join("golden")
                    .join(format!("{name}.json"));
                let golden: GoldenStub =
                    serde_json::from_str(&std::fs::read_to_string(&path).expect("read fixture"))
                        .expect("parse fixture");
                golden.trajectory_digest_fnv1a
            };

            let seed_bank = SeedBank::new(seed);
            let generator = gen_cfg.build(width).expect("build generator");
            let mut env =
                NoisyEnvironment::new(generator, NoiseModel::None, seed_bank).expect("build env");
            let mut learner = ArkheLearner::new_with_config(
                width,
                1,
                wire_delay,
                EconomyConfig::default(),
                GrowthConfig {
                    redundancy_sprouting: false,
                    ..GrowthConfig::default()
                },
                seed_bank,
            )
            .expect("learner");
            let mut budget = TickBudget::new(100_000);
            let mut clean = [0u64; 1];
            let mut obs = [0u64; 1];
            let mut hasher = Fnv1a::new();
            for t in 0..horizon {
                env.step_frame(&mut clean, &mut obs);
                let pred = learner.predict(&mut budget);
                learner.observe(&obs, &mut budget);
                hasher.write(&t.to_le_bytes());
                for p in &pred {
                    hasher.write(&p.to_le_bytes());
                }
                hasher.write(&learner.editable().structure_epoch().to_le_bytes());
                for n in learner.editable().economy().nodes() {
                    hasher.write(&n.energy.to_bits().to_le_bytes());
                }
            }
            assert_eq!(learner.redundancy_sprout_events(), 0);
            assert_eq!(learner.active_comparator_count(), 0);
            assert_eq!(
                hasher.finish_hex(),
                expected_digest,
                "redundancy_sprouting=false must be bit-identical on {name}"
            );
        }
    }

    #[test]
    fn alt_k1_comb_s20_stage3_comparator_audit() {
        use crate::env::{GeneratorConfig, NoiseModel, NoisyEnvironment};
        use crate::structural::graph_edit::derivation_cone_info;

        #[derive(Debug, PartialEq, Eq, Deserialize)]
        struct AuditEvent {
            tick: u64,
            kind: String,
            node_count: usize,
            details: String,
        }
        #[derive(Deserialize)]
        struct GoldenEventsStub {
            events: Vec<AuditEvent>,
        }

        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("golden")
            .join("alt_k1_comb_s20.json");
        let golden: GoldenEventsStub =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read fixture"))
                .expect("parse fixture");

        let seed_bank = SeedBank::new(20);
        let generator = GeneratorConfig::T1_Alternating { a: 0, b: 1 }
            .build(Width::BITS_1)
            .expect("build generator");
        let mut env =
            NoisyEnvironment::new(generator, NoiseModel::None, seed_bank).expect("build env");
        let mut learner = ArkheLearner::new_with_config(
            Width::BITS_1,
            1,
            WireDelayMode::Combinational,
            EconomyConfig::default(),
            GrowthConfig::default(),
            seed_bank,
        )
        .expect("learner");

        let mut budget = TickBudget::new(100_000);
        let mut clean = [0u64; 1];
        let mut obs = [0u64; 1];
        let mut events = Vec::new();
        let mut prev_sprouts = 0;
        let mut prev_prunes = 0;
        let mut prev_rings = 0;

        for t in 0..400u64 {
            env.step_frame(&mut clean, &mut obs);
            let _ = learner.predict(&mut budget);
            learner.observe(&obs, &mut budget);

            if learner.sprout_events() > prev_sprouts {
                prev_sprouts = learner.sprout_events();
                events.push(AuditEvent {
                    tick: t,
                    kind: "Sprout".to_owned(),
                    node_count: learner.editable().circuit().node_count(),
                    details: format!("sprout_events={}", learner.sprout_events()),
                });
            }
            if learner.starvation_prune_events() > prev_prunes {
                prev_prunes = learner.starvation_prune_events();
                events.push(AuditEvent {
                    tick: t,
                    kind: "StarvationPrune".to_owned(),
                    node_count: learner.editable().circuit().node_count(),
                    details: format!("prune_events={}", learner.starvation_prune_events()),
                });
            }
            if learner.ring_closure_events() > prev_rings {
                prev_rings = learner.ring_closure_events();
                events.push(AuditEvent {
                    tick: t,
                    kind: "RingClosure".to_owned(),
                    node_count: learner.editable().circuit().node_count(),
                    details: format!("ring_events={}", learner.ring_closure_events()),
                });
            }
        }

        assert_eq!(events, golden.events);
        assert!(learner.redundancy_sprout_events() >= 1);
        assert_eq!(learner.starvation_prune_events(), 0);
        assert_eq!(learner.active_comparator_count(), 1);
        assert_eq!(learner.comparator_disagreement_ticks(), 0);

        let circ = learner.editable().circuit();
        let comp = &learner.editable().economy().comparators()[0];
        let pri_cone = derivation_cone_info(circ, &[SignalSink::BoundaryOut(0)]);
        let alt_cone = derivation_cone_info(
            circ,
            &[SignalSink::NodeIn {
                node: comp.sub_node,
                port: 1,
            }],
        );
        assert!(pri_cone.excl_nodes.is_disjoint(&alt_cone.excl_nodes));
        assert!(
            pri_cone
                .excl_reactive_leaves
                .is_disjoint(&alt_cone.excl_reactive_leaves)
        );

        let nodes: Vec<String> = circ.nodes().iter().map(|n| format!("{n:?}")).collect();
        let edges: Vec<String> = circ
            .edges()
            .iter()
            .map(|e| format!("{:?} -> {:?} (hops={})", e.src, e.dst, e.hops))
            .collect();
        eprintln!(
            "alt_k1_comb_s20_audit: nodes={nodes:?} edges={edges:?} \
             redundancy_sprout_events={} starvation_prune_events={} active_comparator_count={} \
             primary_mask={} alt_mask={} agree={} disagree={}",
            learner.redundancy_sprout_events(),
            learner.starvation_prune_events(),
            learner.active_comparator_count(),
            comp.primary_channel_mask,
            comp.alt_channel_mask,
            learner.comparator_agreement_ticks(),
            learner.comparator_disagreement_ticks(),
        );
    }

    #[test]
    fn stage3_forms_causal_disjoint_comparators_on_multiview_and_t1_preserving_epochs() {
        use crate::env::{
            GeneratorConfig, LfsrStage, MultiViewDriver, MultiViewTarget, MultiViewTransform,
            NoiseModel, NoisyEnvironment,
        };
        use crate::structural::graph_edit::derivation_cone_info;

        for width in [Width::BITS_1, Width::BITS_2] {
            let wire_delay = WireDelayMode::Combinational;
            let mv_cfg = GeneratorConfig::T2_MultiViewCoupled {
                driver: MultiViewDriver::Lfsr {
                    stage: LfsrStage {
                        taps: vec![1, 1, 0, 1],
                        init: vec![1, 0, 1, 1],
                    },
                },
                view_0: MultiViewTransform {
                    delay: 1,
                    mul_a: 1,
                    add_b: 0,
                },
                view_1: MultiViewTransform {
                    delay: 0,
                    mul_a: 1,
                    add_b: 1,
                },
                target: MultiViewTarget::Affine {
                    lag: 2,
                    mul_a: 1,
                    add_b: 0,
                },
            };
            let sb = SeedBank::new(5);
            let mut env =
                NoisyEnvironment::new(mv_cfg.build(width).unwrap(), NoiseModel::None, sb).unwrap();
            let mut learner =
                ArkheLearner::new(width, 3, wire_delay, GrowthConfig::default(), sb).unwrap();
            let mut budget = TickBudget::new(200_000);
            let mut clean = [0u64; 3];
            let mut obs = [0u64; 3];
            let mut post_mismatches = 0usize;
            for t in 0..1200 {
                env.step_frame(&mut clean, &mut obs);
                let pred = learner.predict(&mut budget);
                if t >= 700 && pred[2] != obs[2] {
                    post_mismatches += 1;
                }
                learner.observe(&obs, &mut budget);
            }
            assert_eq!(
                post_mismatches, 0,
                "T2_MultiViewCoupled ({width:?}, {wire_delay:?}) must maintain 0 clean mismatches"
            );
            assert!(
                learner.redundancy_sprout_events() >= 1,
                "T2_MultiViewCoupled ({width:?}, {wire_delay:?}) must sprout Stage 3 redundancy"
            );
            assert_eq!(learner.starvation_prune_events(), 0);
            assert_eq!(learner.telemetry().sprout_blocked_max_nodes, 0);

            // Audit causal-support disjointness on comparator protecting port 2.
            let comp = learner
                .editable()
                .economy()
                .comparators()
                .iter()
                .find(|c| c.target_port == 2)
                .expect("comparator on port 2");
            assert_eq!(
                comp.primary_channel_mask & comp.alt_channel_mask,
                0,
                "T2_MultiViewCoupled comparator must pick channel-disjoint Tier 1 route"
            );
            let circ = learner.editable().circuit();
            let pri_cone = derivation_cone_info(circ, &[SignalSink::BoundaryOut(2)]);
            let alt_cone = derivation_cone_info(
                circ,
                &[SignalSink::NodeIn {
                    node: comp.sub_node,
                    port: 1,
                }],
            );
            assert!(pri_cone.excl_nodes.is_disjoint(&alt_cone.excl_nodes));
            assert!(
                pri_cone
                    .excl_reactive_leaves
                    .is_disjoint(&alt_cone.excl_reactive_leaves)
            );
        }

        // Causal Split-primary TicksPerHop on T2_MultiViewCoupled (G2B.2 & Invariant 2).
        for width in [Width::BITS_1, Width::BITS_2] {
            let mv_cfg = GeneratorConfig::T2_MultiViewCoupled {
                driver: MultiViewDriver::Lfsr {
                    stage: LfsrStage {
                        taps: vec![1, 1, 0, 0, 1, 1],
                        init: vec![1, 0, 1, 1, 0, 1],
                    },
                },
                view_0: MultiViewTransform {
                    delay: 1,
                    mul_a: 1,
                    add_b: 0,
                },
                view_1: MultiViewTransform {
                    delay: 0,
                    mul_a: 1,
                    add_b: 0,
                },
                target: MultiViewTarget::Affine {
                    lag: 4,
                    mul_a: 1,
                    add_b: 0,
                },
            };
            let sb = SeedBank::new(5);
            let mut env =
                NoisyEnvironment::new(mv_cfg.build(width).unwrap(), NoiseModel::None, sb).unwrap();
            let mut tph_circ = Circuit::new(width, 3, 3, WireDelayMode::TicksPerHop);
            let split_p = tph_circ.add_node(Primitive::Split);
            tph_circ.connect(
                crate::substrate::SignalSource::BoundaryIn(0),
                SignalSink::NodeIn {
                    node: split_p,
                    port: 0,
                },
            );
            tph_circ.connect(
                crate::substrate::SignalSource::NodeOut {
                    node: split_p,
                    port: 0,
                },
                SignalSink::BoundaryOut(2),
            );
            let econ = EconomyConfig::default();
            let mut editable = EditableCircuit::new(tph_circ, econ).unwrap();
            if let Some(node_econ) = editable.economy_mut().node_mut(split_p.index()) {
                node_econ.juvenile_ticks = econ.juvenile_ticks;
            }
            let mut learner =
                ArkheLearner::from_editable(editable, GrowthConfig::default(), sb).unwrap();
            let mut budget = TickBudget::new(200_000);
            let mut clean = [0u64; 3];
            let mut obs = [0u64; 3];
            let mut post_mismatches = 0usize;
            for t in 0..1200 {
                env.step_frame(&mut clean, &mut obs);
                let pred = learner.predict(&mut budget);
                if t >= 700 && pred[2] != obs[2] {
                    post_mismatches += 1;
                }
                learner.observe(&obs, &mut budget);
            }
            assert_eq!(
                post_mismatches, 0,
                "Split-primary T2_MultiViewCoupled ({width:?}, TicksPerHop) must maintain 0 clean mismatches"
            );
            assert!(
                learner.redundancy_sprout_events() >= 1,
                "Split-primary T2_MultiViewCoupled ({width:?}, TicksPerHop) must sprout Stage 3 redundancy"
            );
            assert_eq!(learner.starvation_prune_events(), 0);
            assert_eq!(learner.telemetry().sprout_blocked_max_nodes, 0);
            let comp = learner
                .editable()
                .economy()
                .comparators()
                .iter()
                .find(|c| c.target_port == 2)
                .expect("comparator on port 2 in Split-primary TicksPerHop");
            assert_eq!(comp.primary_channel_mask & comp.alt_channel_mask, 0);
            let circ = learner.editable().circuit();
            assert!(circ.edges().iter().all(|e| e.hops == 1));
            let pri_cone = derivation_cone_info(circ, &[SignalSink::BoundaryOut(2)]);
            let alt_cone = derivation_cone_info(
                circ,
                &[SignalSink::NodeIn {
                    node: comp.sub_node,
                    port: 1,
                }],
            );
            assert!(pri_cone.excl_nodes.is_disjoint(&alt_cone.excl_nodes));
            assert!(
                pri_cone
                    .excl_reactive_leaves
                    .is_disjoint(&alt_cone.excl_reactive_leaves)
            );
        }

        // K=1 multi-route T1 preserves Stage 2 epoch_snapshots identically vs redundancy_sprouting: false.
        for (is_alternating, t1_cfg) in [
            (true, GeneratorConfig::T1_Alternating { a: 0, b: 1 }),
            (
                false,
                GeneratorConfig::T1_PeriodicMotif {
                    motif: vec![0, 1, 1],
                },
            ),
        ] {
            let run_t1 = |redundancy_sprouting: bool| {
                let sb = SeedBank::new(2);
                let mut env = NoisyEnvironment::new(
                    t1_cfg.build(Width::BITS_1).unwrap(),
                    NoiseModel::None,
                    sb,
                )
                .unwrap();
                let mut learner = ArkheLearner::new(
                    Width::BITS_1,
                    1,
                    WireDelayMode::Combinational,
                    GrowthConfig {
                        redundancy_sprouting,
                        ..GrowthConfig::default()
                    },
                    sb,
                )
                .unwrap();
                let mut budget = TickBudget::new(200_000);
                let mut clean = [0u64; 1];
                let mut obs = [0u64; 1];
                for _ in 0..800 {
                    env.step_frame(&mut clean, &mut obs);
                    let _ = learner.predict(&mut budget);
                    learner.observe(&obs, &mut budget);
                }
                learner
            };

            let learner = run_t1(true);
            let baseline_learner = run_t1(false);
            assert!(learner.redundancy_sprout_events() >= 1);
            assert_eq!(learner.active_comparator_count(), 1);
            assert_eq!(learner.starvation_prune_events(), 0);

            let snapshots_with_stage3 = learner.epoch_snapshots();
            let snapshots_without_stage3 = baseline_learner.epoch_snapshots();
            assert!(!snapshots_with_stage3.is_empty());
            assert_eq!(snapshots_with_stage3, snapshots_without_stage3);
            let epochs: Vec<usize> = snapshots_with_stage3.iter().map(|s| s.epoch).collect();
            if is_alternating {
                assert_eq!(epochs, vec![0]);
            } else {
                assert!(snapshots_with_stage3.len() >= 2);
            }
            assert_eq!(learner.comparator_disagreement_ticks(), 0);
            assert!(learner.comparator_agreement_ticks() > 0);
        }
    }

    #[test]
    fn stage3_halts_cleanly_on_noisy_constant_non_redundant_t2_and_resists_5pct_thrashing() {
        use crate::env::{
            CrossNonlinearOp, GeneratorConfig, LfsrStage, NoiseModel, NoisyEnvironment,
        };

        // 1. NoisyConstant halts at 0 nodes / 0 redundancy sprouts.
        for value in [0u64, 1] {
            let sb = SeedBank::new(3);
            let mut env = NoisyEnvironment::new(
                GeneratorConfig::T1_Constant { value }
                    .build(Width::BITS_1)
                    .unwrap(),
                NoiseModel::IndependentSubstitution {
                    epsilon: 0.05,
                    channels: vec![0],
                },
                sb,
            )
            .unwrap();
            let mut learner = ArkheLearner::new(
                Width::BITS_1,
                1,
                WireDelayMode::Combinational,
                GrowthConfig::default(),
                sb,
            )
            .unwrap();
            let mut budget = TickBudget::new(100_000);
            let mut clean = [0u64; 1];
            let mut obs = [0u64; 1];
            for _ in 0..500 {
                env.step_frame(&mut clean, &mut obs);
                let _ = learner.predict(&mut budget);
                learner.observe(&obs, &mut budget);
            }
            assert_eq!(learner.editable().circuit().node_count(), 0);
            assert_eq!(learner.redundancy_sprout_events(), 0);
            assert_eq!(learner.active_comparator_count(), 0);
        }

        // 2a. Genuinely non-redundant T2_CrossNonlinear (deg 11 x deg 9, period 1,046,017) halts at 0 comparators on channel 2.
        {
            let non_red_cfg = GeneratorConfig::T2_CrossNonlinear {
                driver_0: LfsrStage {
                    taps: vec![0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1],
                    init: vec![1, 0, 1, 1, 0, 1, 0, 1, 1, 0, 1],
                },
                driver_1: LfsrStage {
                    taps: vec![0, 0, 0, 1, 0, 0, 0, 0, 1],
                    init: vec![1, 1, 0, 1, 0, 1, 1, 0, 1],
                },
                lag_0: 1,
                lag_1: 2,
                op: CrossNonlinearOp::MulEqz,
            };
            let sb_nr = SeedBank::new(4);
            let mut env_nr = NoisyEnvironment::new(
                non_red_cfg.build(Width::BITS_1).unwrap(),
                NoiseModel::None,
                sb_nr,
            )
            .unwrap();
            let mut learner_nr = ArkheLearner::new(
                Width::BITS_1,
                3,
                WireDelayMode::Combinational,
                GrowthConfig::default(),
                sb_nr,
            )
            .unwrap();
            let mut budget_nr = TickBudget::new(200_000);
            let mut clean_nr = [0u64; 3];
            let mut obs_nr = [0u64; 3];
            for _ in 0..800 {
                env_nr.step_frame(&mut clean_nr, &mut obs_nr);
                let _ = learner_nr.predict(&mut budget_nr);
                learner_nr.observe(&obs_nr, &mut budget_nr);
            }
            assert!(
                learner_nr
                    .editable()
                    .economy()
                    .comparators()
                    .iter()
                    .all(|c| c.target_port != 2),
                "non-redundant T2_CrossNonlinear (deg 11 x deg 9) target port 2 must form 0 comparators"
            );
        }

        // 2b. Short-period T2_CrossNonlinear (deg 4 x deg 4, ch2 period 21) forms a valid causal-disjoint
        //     port-2 comparator (primary {ch0, ch1} vs alt {ch2} = 0b100) with 0 starvation/disagreement prunes.
        {
            let short_cfg = GeneratorConfig::T2_CrossNonlinear {
                driver_0: LfsrStage {
                    taps: vec![1, 1, 0, 1],
                    init: vec![1, 0, 0, 1],
                },
                driver_1: LfsrStage {
                    taps: vec![1, 0, 1, 1],
                    init: vec![1, 1, 0, 1],
                },
                lag_0: 1,
                lag_1: 2,
                op: CrossNonlinearOp::MulEqz,
            };
            let sb_sp = SeedBank::new(4);
            let mut env_sp = NoisyEnvironment::new(
                short_cfg.build(Width::BITS_1).unwrap(),
                NoiseModel::None,
                sb_sp,
            )
            .unwrap();
            let mut learner_sp = ArkheLearner::new(
                Width::BITS_1,
                3,
                WireDelayMode::Combinational,
                GrowthConfig::default(),
                sb_sp,
            )
            .unwrap();
            let mut budget_sp = TickBudget::new(200_000);
            let mut clean_sp = [0u64; 3];
            let mut obs_sp = [0u64; 3];
            for _ in 0..800 {
                env_sp.step_frame(&mut clean_sp, &mut obs_sp);
                let _ = learner_sp.predict(&mut budget_sp);
                learner_sp.observe(&obs_sp, &mut budget_sp);
            }
            let port2_comps: Vec<_> = learner_sp
                .editable()
                .economy()
                .comparators()
                .iter()
                .filter(|c| c.target_port == 2)
                .collect();
            assert_eq!(
                port2_comps.len(),
                1,
                "short-period deg 4 x deg 4 target port 2 must form 1 comparator"
            );
            assert_eq!(
                port2_comps[0].primary_channel_mask & port2_comps[0].alt_channel_mask,
                0,
                "port 2 comparator source masks must be disjoint"
            );
            assert_eq!(
                port2_comps[0].alt_channel_mask, 0b100,
                "port 2 alt derivation must route via channel 2 self-history (0b100)"
            );
            assert_eq!(learner_sp.starvation_prune_events(), 0);
            assert_eq!(learner_sp.disagreement_prune_events(), 0);
        }

        // 3. NoisyAlternating (eps=0.05) forms comparator without sprout/prune thrashing.
        let sb_na = SeedBank::new(0);
        let mut env_na = NoisyEnvironment::new(
            GeneratorConfig::T1_Alternating { a: 0, b: 1 }
                .build(Width::BITS_1)
                .unwrap(),
            NoiseModel::IndependentSubstitution {
                epsilon: 0.05,
                channels: vec![0],
            },
            sb_na,
        )
        .unwrap();
        let mut learner_na = ArkheLearner::new(
            Width::BITS_1,
            1,
            WireDelayMode::Combinational,
            GrowthConfig::default(),
            sb_na,
        )
        .unwrap();
        let mut budget_na = TickBudget::new(200_000);
        let mut clean_na = [0u64; 1];
        let mut obs_na = [0u64; 1];
        for _ in 0..1600 {
            env_na.step_frame(&mut clean_na, &mut obs_na);
            let _ = learner_na.predict(&mut budget_na);
            learner_na.observe(&obs_na, &mut budget_na);
        }
        assert_eq!(learner_na.redundancy_sprout_events(), 1);
        assert_eq!(learner_na.active_comparator_count(), 1);
        assert_eq!(learner_na.disagreement_prune_events(), 0);
        assert_eq!(learner_na.starvation_prune_events(), 0);
    }

    #[test]
    fn stage3_respects_cross_port_budget_and_juvenile_sibling_non_starvation() {
        use crate::substrate::SignalSource;
        // Port 0 solved (mature); Port 1 unsolved with juvenile node blocks Stage 3 on Port 0.
        let width = Width::BITS_1;
        let mut circ = Circuit::new(width, 2, 2, WireDelayMode::Combinational);
        let eq0 = circ.add_node(Primitive::Eqz);
        circ.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: eq0, port: 0 },
        );
        circ.connect(
            SignalSource::NodeOut { node: eq0, port: 0 },
            SignalSink::BoundaryOut(0),
        );
        let d1 = circ.add_node(Primitive::Delay { init: 0 });
        circ.connect(
            SignalSource::BoundaryIn(1),
            SignalSink::NodeIn { node: d1, port: 0 },
        );
        circ.connect(
            SignalSource::NodeOut { node: d1, port: 0 },
            SignalSink::BoundaryOut(1),
        );

        let econ = EconomyConfig::default();
        let growth = GrowthConfig::default();
        let mut editable = EditableCircuit::new(circ, econ).unwrap();
        // Make node 0 (port 0) mature and node 1 (port 1) juvenile.
        editable.economy_mut().node_mut(0).unwrap().juvenile_ticks = 0;
        editable.economy_mut().node_mut(1).unwrap().juvenile_ticks = 128;

        let mut learner = ArkheLearner::from_editable(editable, growth, SeedBank::new(7)).unwrap();
        assert!(learner.sibling_has_juvenile_nodes(0));

        // Feed alternating port 0 and noisy/unsolved port 1; Stage 3 must wait until sibling matures.
        let mut budget = TickBudget::new(20_000);
        for t in 0..80u64 {
            let a = t % 2;
            let b = if t % 3 == 0 { 1 } else { 0 };
            let _ = learner.predict(&mut budget);
            learner.observe(&[a, b], &mut budget);
            if learner.sibling_has_juvenile_nodes(0) {
                assert_eq!(learner.redundancy_sprout_events(), 0);
            }
        }
    }

    #[test]
    fn wake_disagreement_triggers_handoff_and_refutation_prune_within_64_ticks() {
        use crate::env::{
            GeneratorConfig, LfsrStage, MultiViewDriver, MultiViewTarget, MultiViewTransform,
            NoiseModel, NoisyEnvironment,
        };

        for width in [Width::BITS_1, Width::BITS_2] {
            let fault_tick = 1000u64;
            let cfg = GeneratorConfig::T2_MultiViewCoupled {
                driver: MultiViewDriver::Lfsr {
                    stage: LfsrStage {
                        taps: vec![1, 1, 0, 1],
                        init: vec![1, 0, 1, 1],
                    },
                },
                // ch0 has shorter lag (tau = 1) so Stage 2 locks ch0 first; ch1 has tau = 2 so Stage 3 uses ch1.
                view_0: MultiViewTransform {
                    delay: 1,
                    mul_a: 1,
                    add_b: 0,
                },
                view_1: MultiViewTransform {
                    delay: 0,
                    mul_a: 1,
                    add_b: 1,
                },
                target: MultiViewTarget::Affine {
                    lag: 2,
                    mul_a: 1,
                    add_b: 0,
                },
            };
            let fault_eps = 1.0 - 1.0 / (2.0f64.powi(i32::from(width.bits())));
            let noise = NoiseModel::ChannelFaultSchedule {
                base_epsilon: 0.0,
                base_channels: Vec::new(),
                fault_tick,
                fault_epsilon: fault_eps,
                fault_channels: vec![0],
            };
            let sb = SeedBank::new(11);
            let mut env = NoisyEnvironment::new(cfg.build(width).unwrap(), noise, sb).unwrap();
            let mut learner = ArkheLearner::new(
                width,
                3,
                WireDelayMode::Combinational,
                GrowthConfig::default(),
                sb,
            )
            .unwrap();
            let mut budget = TickBudget::new(200_000);
            let mut clean = [0u64; 3];
            let mut obs = [0u64; 3];
            let mut recovered_mismatches = 0usize;
            for t in 0..1400 {
                env.step_frame(&mut clean, &mut obs);
                let pred = learner.predict(&mut budget);
                if t >= fault_tick + 64 && pred[2] != clean[2] {
                    recovered_mismatches += 1;
                }
                learner.observe(&obs, &mut budget);
            }
            assert!(
                learner.redundancy_sprout_events() >= 1,
                "pre-fault redundancy must form"
            );
            assert!(
                learner.driver_handoff_events() >= 1,
                "fault on primary ch0 must trigger driver handoff ({width:?})"
            );
            assert!(
                learner.disagreement_prune_events() >= 1,
                "fault on primary ch0 must prune broken ch0 route ({width:?})"
            );
            assert_eq!(
                recovered_mismatches, 0,
                "target port 2 must recover 0 clean error within <= 64 ticks ({width:?})"
            );
        }
    }

    #[test]
    fn offline_loopback_detects_deceptive_contradiction_freezes_null_and_prunes_minority_of_3() {
        use crate::structural::build_autonomous_ring_source;
        use crate::structural::graph_edit::splice_comparator_onto_port;
        use crate::substrate::SignalSource;

        // 1. Deceptive lag-2 vs period-6 [0, 1, 0, 1, 0, 0] (Charter §1.3) offline falsification + frozen null.
        {
            let width = Width::BITS_1;
            let mut circ = Circuit::new(width, 1, 1, WireDelayMode::Combinational);
            // Primary: deceptive lag-2 reactive predictor Delay(Delay(BoundaryIn(0)))
            let d1 = circ.add_node(Primitive::Delay { init: 0 });
            let d2 = circ.add_node(Primitive::Delay { init: 1 });
            circ.connect(
                SignalSource::BoundaryIn(0),
                SignalSink::NodeIn { node: d1, port: 0 },
            );
            circ.connect(
                SignalSource::NodeOut { node: d1, port: 0 },
                SignalSink::NodeIn { node: d2, port: 0 },
            );
            circ.connect(
                SignalSource::NodeOut { node: d2, port: 0 },
                SignalSink::BoundaryOut(0),
            );
            // Alt: exact period-6 autonomous delay ring [0, 1, 0, 1, 0, 0]
            let motif = [0u64, 1, 0, 1, 0, 0];
            let (_, _, ring_src) = build_autonomous_ring_source(&mut circ, 6, &motif);
            let spliced = splice_comparator_onto_port(&mut circ, 0, ring_src).unwrap();
            let econ = EconomyConfig {
                juvenile_ticks: 0,
                ..EconomyConfig::default()
            };
            let mut editable = EditableCircuit::new(circ, econ).unwrap();
            let comp_idx = editable
                .register_comparator(spliced.sub_node, spliced.eqz_node, 0, 1, 0)
                .unwrap();
            let _ = comp_idx;

            let mut learner =
                ArkheLearner::from_editable(editable, GrowthConfig::default(), SeedBank::new(0))
                    .unwrap();
            let mut budget = TickBudget::new(20_000);
            // Seed wake null model cleanly over 24 ticks (before W_diag=64 refutation prunes lag-2).
            for t in 0..24 {
                let _ = learner.predict(&mut budget);
                learner.observe(&[motif[t % 6]], &mut budget);
            }
            let wake_null_before = (
                learner.editable().economy().scorers()[0]
                    .null_model()
                    .probability(0)
                    .to_bits(),
                learner.editable().economy().scorers()[0]
                    .null_model()
                    .probability(1)
                    .to_bits(),
                learner.editable().economy().scorers()[0]
                    .null_model()
                    .last_symbol(),
            );

            let report = learner.run_offline_loopback(128);
            let wake_null_after = (
                learner.editable().economy().scorers()[0]
                    .null_model()
                    .probability(0)
                    .to_bits(),
                learner.editable().economy().scorers()[0]
                    .null_model()
                    .probability(1)
                    .to_bits(),
                learner.editable().economy().scorers()[0]
                    .null_model()
                    .last_symbol(),
            );
            assert_eq!(
                wake_null_before, wake_null_after,
                "Sleep loopback must keep wake NullModel frozen"
            );
            assert!(report.contradiction_detected);
            assert!(report.comparator_disagreement_ticks >= 1);
            assert!(report.max_circular_discrepancy >= 1);
            assert!(report.sleep_comparator_flux < 0.0);
        }

        // 2. 3-route majority pruning: tests BOTH minority faulty alt AND minority faulty primary.
        for primary_is_minority in [false, true] {
            let width = Width::BITS_1;
            let mut circ = Circuit::new(width, 1, 1, WireDelayMode::Combinational);
            let (_, _, r1) = build_autonomous_ring_source(&mut circ, 2, &[0, 1]);
            let (_, _, r2) = build_autonomous_ring_source(&mut circ, 2, &[0, 1]);
            // Faulty 3rd route: constant-0 delay loop `[0, 0]` (disagrees on odd ticks).
            let (_, _, r_fault) = build_autonomous_ring_source(&mut circ, 2, &[0, 0]);

            let (pri, alt_a, alt_b) = if primary_is_minority {
                (r_fault, r1, r2)
            } else {
                (r1, r2, r_fault)
            };
            circ.connect(pri, SignalSink::BoundaryOut(0));
            let sp_a = splice_comparator_onto_port(&mut circ, 0, alt_a).unwrap();
            let sp_b = splice_comparator_onto_port(&mut circ, 0, alt_b).unwrap();

            let econ = EconomyConfig {
                juvenile_ticks: 0,
                ..EconomyConfig::default()
            };
            let mut editable = EditableCircuit::new(circ, econ).unwrap();
            editable
                .register_comparator(sp_a.sub_node, sp_a.eqz_node, 0, 1 << 60, 1 << 61)
                .unwrap();
            editable
                .register_comparator(sp_b.sub_node, sp_b.eqz_node, 0, 1 << 60, 1 << 62)
                .unwrap();
            let mut learner =
                ArkheLearner::from_editable(editable, GrowthConfig::default(), SeedBank::new(9))
                    .unwrap();
            let mut budget = TickBudget::new(20_000);
            // Warm wake NullModel for 16 ticks (< W_diag=64) so comparators stay intact until Sleep.
            for t in 0..16 {
                let _ = learner.predict(&mut budget);
                learner.observe(&[(t % 2) as u64], &mut budget);
            }
            assert_eq!(learner.active_comparator_count(), 2);

            let report = learner.run_offline_loopback(128);
            assert_eq!(report.minority_routes_pruned, 1);
            assert_eq!(
                learner.active_comparator_count(),
                1,
                "exact 1 surviving comparator between the 2 agreeing true routes must remain"
            );
            // After pruning the faulty minority route, a second loopback sees zero disagreement.
            let clean_report = learner.run_offline_loopback(64);
            assert!(!clean_report.contradiction_detected);
            assert_eq!(clean_report.comparator_disagreement_ticks, 0);
            assert_eq!(clean_report.max_circular_discrepancy, 0);
        }
    }

    #[test]
    fn stage2_promotes_autonomous_ring_comparator_on_noisy_alternating() {
        for seed in [1u64, 3u64] {
            let sb = SeedBank::new(seed);
            let cfg = GeneratorConfig::T1_Alternating { a: 0, b: 3 };
            let generator = cfg.build(Width::BITS_2).unwrap();
            let mut env = NoisyEnvironment::new(
                generator,
                NoiseModel::IndependentSubstitution {
                    epsilon: 0.05,
                    channels: vec![0],
                },
                sb,
            )
            .unwrap();
            let mut learner = ArkheLearner::new_with_config(
                Width::BITS_2,
                1,
                WireDelayMode::Combinational,
                EconomyConfig::default(),
                GrowthConfig::default(),
                sb,
            )
            .unwrap();
            let mut budget = TickBudget::new(50_000);
            let mut clean = [0u64];
            let mut obs = [0u64];
            let mut post_warmup_clean_mismatches = 0u64;
            for t in 0..2_000u64 {
                env.step_frame(&mut clean, &mut obs);
                let pred = learner.predict(&mut budget);
                if t >= 600 && pred[0] != clean[0] {
                    post_warmup_clean_mismatches += 1;
                }
                learner.observe(&obs, &mut budget);
            }
            assert!(
                learner.active_comparator_count() >= 1,
                "seed={seed} should retain autonomous-vs-reactive comparator"
            );
            assert!(
                learner.driver_handoff_events() >= 1 || learner.ring_closure_events() >= 1,
                "seed={seed} must promote autonomous ring over noisy reactive driver"
            );
            assert_eq!(learner.starvation_prune_events(), 0);
            if seed == 3 {
                assert_eq!(
                    learner.disagreement_prune_events(),
                    0,
                    "seed={seed} should avoid false refutation pruning of reactive peer"
                );
            }
            assert_eq!(
                post_warmup_clean_mismatches, 0,
                "seed={seed} must settle cleanly to zero clean mismatches after ring handoff"
            );
        }
    }

    #[test]
    fn all_golden_trace_configs_comparator_audit() {
        use crate::env::{GeneratorConfig, NoiseModel, NoisyEnvironment};
        use crate::structural::graph_edit::derivation_cone_info;

        #[derive(Debug, PartialEq, Eq, Deserialize)]
        struct AuditEdge {
            src: String,
            dst: String,
            hops: u16,
        }
        #[derive(Debug, PartialEq, Eq, Deserialize)]
        struct AuditEvent {
            tick: u64,
            kind: String,
            node_count: usize,
            details: String,
        }
        #[derive(Deserialize)]
        struct GoldenFixtureStub {
            events: Vec<AuditEvent>,
            final_circuit_nodes: Vec<String>,
            final_circuit_edges: Vec<AuditEdge>,
        }

        let cases = [
            (
                "alt_k1_comb_s20",
                GeneratorConfig::T1_Alternating { a: 0, b: 1 },
                Width::BITS_1,
                WireDelayMode::Combinational,
                20u64,
                400u64,
            ),
            (
                "alt_k1_tph_s20",
                GeneratorConfig::T1_Alternating { a: 0, b: 1 },
                Width::BITS_1,
                WireDelayMode::TicksPerHop,
                20u64,
                400u64,
            ),
            (
                "alt_k2_comb_s21",
                GeneratorConfig::T1_Alternating { a: 0, b: 3 },
                Width::BITS_2,
                WireDelayMode::Combinational,
                21u64,
                400u64,
            ),
            (
                "alt_k2_tph_s21",
                GeneratorConfig::T1_Alternating { a: 0, b: 3 },
                Width::BITS_2,
                WireDelayMode::TicksPerHop,
                21u64,
                400u64,
            ),
            (
                "motif011_k1_comb_s22",
                GeneratorConfig::T1_PeriodicMotif {
                    motif: vec![0, 1, 1],
                },
                Width::BITS_1,
                WireDelayMode::Combinational,
                22u64,
                500u64,
            ),
            (
                "motif013_k2_comb_s22",
                GeneratorConfig::T1_PeriodicMotif {
                    motif: vec![0, 1, 3],
                },
                Width::BITS_2,
                WireDelayMode::Combinational,
                22u64,
                500u64,
            ),
            (
                "affine_k2_comb_s23",
                GeneratorConfig::T1_AffineLag1 {
                    mul_a: 1,
                    add_b: 1,
                    init: 0,
                },
                Width::BITS_2,
                WireDelayMode::Combinational,
                23u64,
                500u64,
            ),
            (
                "affine_k2_tph_s23",
                GeneratorConfig::T1_AffineLag1 {
                    mul_a: 1,
                    add_b: 1,
                    init: 0,
                },
                Width::BITS_2,
                WireDelayMode::TicksPerHop,
                23u64,
                500u64,
            ),
        ];

        for (name, gen_cfg, width, wire_delay, seed, horizon) in cases {
            let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("fixtures")
                .join("golden")
                .join(format!("{name}.json"));
            let golden: GoldenFixtureStub =
                serde_json::from_str(&std::fs::read_to_string(&path).expect("read fixture"))
                    .expect("parse fixture");

            let sb = SeedBank::new(seed);
            let generator = gen_cfg.build(width).expect("build generator");
            let mut env =
                NoisyEnvironment::new(generator, NoiseModel::None, sb).expect("build env");
            let mut learner = ArkheLearner::new_with_config(
                width,
                1,
                wire_delay,
                EconomyConfig::default(),
                GrowthConfig::default(),
                sb,
            )
            .expect("learner");
            let mut budget = TickBudget::new(100_000);
            let mut clean = [0u64; 1];
            let mut obs = [0u64; 1];
            let mut events = Vec::new();
            let mut prev_sprouts = 0u64;
            let mut prev_prunes = 0u64;
            let mut prev_rings = 0u64;

            for t in 0..horizon {
                env.step_frame(&mut clean, &mut obs);
                let _ = learner.predict(&mut budget);
                learner.observe(&obs, &mut budget);

                if learner.sprout_events() > prev_sprouts {
                    prev_sprouts = learner.sprout_events();
                    events.push(AuditEvent {
                        tick: t,
                        kind: "Sprout".to_owned(),
                        node_count: learner.editable().circuit().node_count(),
                        details: format!("sprout_events={}", learner.sprout_events()),
                    });
                }
                if learner.starvation_prune_events() > prev_prunes {
                    prev_prunes = learner.starvation_prune_events();
                    events.push(AuditEvent {
                        tick: t,
                        kind: "StarvationPrune".to_owned(),
                        node_count: learner.editable().circuit().node_count(),
                        details: format!("prune_events={}", learner.starvation_prune_events()),
                    });
                }
                if learner.ring_closure_events() > prev_rings {
                    prev_rings = learner.ring_closure_events();
                    events.push(AuditEvent {
                        tick: t,
                        kind: "RingClosure".to_owned(),
                        node_count: learner.editable().circuit().node_count(),
                        details: format!("ring_events={}", learner.ring_closure_events()),
                    });
                }
            }

            assert_eq!(events, golden.events, "{name} events must match golden");
            assert_eq!(
                learner.starvation_prune_events(),
                0,
                "{name} must not starve"
            );

            let circ = learner.editable().circuit();
            let nodes: Vec<String> = circ.nodes().iter().map(|n| format!("{n:?}")).collect();
            let edges: Vec<AuditEdge> = circ
                .edges()
                .iter()
                .map(|e| AuditEdge {
                    src: format!("{:?}", e.src),
                    dst: format!("{:?}", e.dst),
                    hops: e.hops,
                })
                .collect();

            if learner.active_comparator_count() == 0 {
                assert_eq!(nodes, golden.final_circuit_nodes, "{name} nodes");
                assert_eq!(edges, golden.final_circuit_edges, "{name} edges");
            } else {
                for comp in learner.editable().economy().comparators() {
                    let pri_cone =
                        derivation_cone_info(circ, &[SignalSink::BoundaryOut(comp.target_port)]);
                    let alt_cone = derivation_cone_info(
                        circ,
                        &[SignalSink::NodeIn {
                            node: comp.sub_node,
                            port: 1,
                        }],
                    );
                    assert!(
                        pri_cone.excl_nodes.is_disjoint(&alt_cone.excl_nodes),
                        "{name} comparator must have disjoint interior cones"
                    );
                    assert!(
                        pri_cone
                            .excl_reactive_leaves
                            .is_disjoint(&alt_cone.excl_reactive_leaves),
                        "{name} comparator must have disjoint causal leaves"
                    );
                }
            }

            let masks: Vec<(u64, u64)> = learner
                .editable()
                .economy()
                .comparators()
                .iter()
                .map(|c| (c.primary_channel_mask, c.alt_channel_mask))
                .collect();
            eprintln!(
                "golden_audit case={name} events_eq={} redundancy_sprout_events={} \
                 starvation_prune_events={} active_comparator_count={} masks={masks:?} \
                 agree={} disagree={} final_nodes={nodes:?}",
                events == golden.events,
                learner.redundancy_sprout_events(),
                learner.starvation_prune_events(),
                learner.active_comparator_count(),
                learner.comparator_agreement_ticks(),
                learner.comparator_disagreement_ticks(),
            );
        }
    }

    #[test]
    fn t2_cross_nonlinear_deg11_deg9_zero_redundancy_sprouts() {
        use crate::env::{
            CrossNonlinearOp, GeneratorConfig, LfsrStage, NoiseModel, NoisyEnvironment,
        };

        let driver_0 = LfsrStage {
            taps: vec![0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1],
            init: vec![1, 0, 1, 1, 0, 1, 0, 1, 1, 0, 1],
        };
        let driver_1 = LfsrStage {
            taps: vec![0, 0, 0, 1, 0, 0, 0, 0, 1],
            init: vec![1, 1, 0, 1, 0, 1, 1, 0, 1],
        };
        for (label, op, lag_0, lag_1, wire_delay) in [
            (
                "MulEqz(1,2,Comb)",
                CrossNonlinearOp::MulEqz,
                1,
                2,
                WireDelayMode::Combinational,
            ),
            (
                "MulEqzPair(4,4,Comb)",
                CrossNonlinearOp::MulEqzPair,
                4,
                4,
                WireDelayMode::Combinational,
            ),
            (
                "MulEqzPair(4,4,TPH)",
                CrossNonlinearOp::MulEqzPair,
                4,
                4,
                WireDelayMode::TicksPerHop,
            ),
        ] {
            let cfg = GeneratorConfig::T2_CrossNonlinear {
                driver_0: driver_0.clone(),
                driver_1: driver_1.clone(),
                lag_0,
                lag_1,
                op,
            };
            let sb = SeedBank::new(0);
            let mut env =
                NoisyEnvironment::new(cfg.build(Width::BITS_1).unwrap(), NoiseModel::None, sb)
                    .unwrap();
            let mut learner =
                ArkheLearner::new(Width::BITS_1, 3, wire_delay, GrowthConfig::default(), sb)
                    .unwrap();
            let mut budget = TickBudget::new(200_000);
            let mut clean = [0u64; 3];
            let mut obs = [0u64; 3];
            for _ in 0..2_000 {
                env.step_frame(&mut clean, &mut obs);
                let _ = learner.predict(&mut budget);
                learner.observe(&obs, &mut budget);
            }
            assert_eq!(
                learner.redundancy_sprout_events(),
                0,
                "{label} (deg 11 x deg 9) must have zero redundancy_sprout_events"
            );
            assert_eq!(
                learner.active_comparator_count(),
                0,
                "{label} (deg 11 x deg 9) must have zero active comparators"
            );
        }
    }

    #[test]
    fn comparator_wake_window_tail_helpers_match_dual_timescale() {
        let mut win = ComparatorWakeWindow::new(NodeId(0), 256);
        for _ in 0..192 {
            win.push(false, 1.0, 1.0, false, false);
        }
        assert!(!win.is_full());
        assert_eq!(win.disagree.len(), 192);
        for _ in 0..64 {
            win.push(true, -1.0, 1.0, true, false);
        }
        assert!(win.is_full());
        assert_eq!(win.disagreement_count_tail(64), 64);
        assert_eq!(win.e_primary_count_tail(64), 64);
        assert_eq!(win.e_alt_count_tail(64), 0);
        assert!((win.flux_primary_tail(64) + 64.0).abs() < 1e-12);
        assert!((win.flux_alt_tail(64) - 64.0).abs() < 1e-12);
        assert_eq!(win.disagreement_count(), 64);
        assert!((win.flux_primary() - 128.0).abs() < 1e-12);
        assert!((win.flux_alt() - 256.0).abs() < 1e-12);
    }

    #[test]
    fn m2b_b2_bankruptcy_refutation_excises_one_sided_vs_both_starved_regime_shift() {
        use crate::structural::graph_edit::splice_comparator_onto_port;
        use std::collections::BTreeSet;

        let build_two_arm_learner = || {
            let mut circ = Circuit::new(Width::BITS_1, 2, 2, WireDelayMode::Combinational);
            let pri_eqz = circ.add_node(Primitive::Eqz);
            circ.connect(
                SignalSource::BoundaryIn(0),
                SignalSink::NodeIn {
                    node: pri_eqz,
                    port: 0,
                },
            );
            circ.connect(
                SignalSource::NodeOut {
                    node: pri_eqz,
                    port: 0,
                },
                SignalSink::BoundaryOut(0),
            );
            let alt_delay = circ.add_node(Primitive::Delay { init: 0 });
            circ.connect(
                SignalSource::BoundaryIn(1),
                SignalSink::NodeIn {
                    node: alt_delay,
                    port: 0,
                },
            );
            let spliced = splice_comparator_onto_port(
                &mut circ,
                0,
                SignalSource::NodeOut {
                    node: alt_delay,
                    port: 0,
                },
            )
            .unwrap();
            let econ = EconomyConfig {
                juvenile_ticks: 0,
                ..EconomyConfig::default()
            };
            let mut editable = EditableCircuit::new(circ, econ).unwrap();
            editable
                .register_comparator(spliced.sub_node, spliced.eqz_node, 0, 1, 2)
                .unwrap();
            let mut learner =
                ArkheLearner::from_editable(editable, GrowthConfig::default(), SeedBank::new(0))
                    .unwrap();
            let w_diag = learner.growth_config.diagnostic_window;
            for _ in 0..w_diag {
                learner.comparator_windows[0].push(true, 1.0, -1.0, false, true);
            }
            (learner, pri_eqz, alt_delay)
        };

        // 1. Both exclusive arms starved (regime shift): 1-sided bankruptcy refutation MUST NOT fire;
        //    scoped starvation counters attribute the event to port 0 and comparators.
        {
            let (mut learner, pri_eqz, alt_delay) = build_two_arm_learner();
            let mut starved_both = BTreeSet::new();
            starved_both.insert(pri_eqz);
            starved_both.insert(alt_delay);
            assert!(
                !learner.try_wake_comparator_bankruptcy_refutation(&starved_both),
                "both-starved comparator must decline 1-sided refutation"
            );
            assert_eq!(learner.active_comparator_count(), 1);
            assert_eq!(learner.disagreement_prune_events(), 0);

            let pre_circ = learner.editable.circuit().clone();
            let pre_comps = learner.editable.economy().comparators().to_vec();
            learner.record_scoped_starvation_prune(&pre_circ, &pre_comps, &starved_both);
            assert_eq!(learner.port_starvation_prune_events(), &[1, 0]);
            assert_eq!(learner.comparator_starvation_prune_events(), 1);
        }

        // 2. End-to-end observe() on 1-sided bankruptcy (alt_delay energy < 0 while pri_eqz solvent):
        //    triggers atomic 1-sided comparator refutation without recording starvation prune.
        {
            let (mut learner, pri_eqz, alt_delay) = build_two_arm_learner();
            learner
                .editable
                .economy_mut()
                .node_mut(pri_eqz.index())
                .unwrap()
                .energy = 100.0;
            learner
                .editable
                .economy_mut()
                .node_mut(alt_delay.index())
                .unwrap()
                .energy = -1.0;
            let mut budget = TickBudget::new(10_000);
            let _ = learner.predict(&mut budget);
            learner.observe(&[1, 0], &mut budget);

            assert_eq!(learner.active_comparator_count(), 0);
            assert_eq!(learner.disagreement_prune_events(), 1);
            assert_eq!(learner.starvation_prune_events(), 0);
            assert_eq!(learner.port_starvation_prune_events(), &[0, 0]);
            assert_eq!(learner.comparator_starvation_prune_events(), 0);
            // Primary BoundaryOut(0) route remains intact after alt excision.
            let mut check_budget = TickBudget::new(10_000);
            let pred = learner.predict(&mut check_budget);
            assert_eq!(pred[0], 0);
        }
    }
}
