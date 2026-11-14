//! Online closed-loop coordinate-free neurogenesis learner (Milestone 1b, Hypothesis H2).

use serde::{Deserialize, Serialize};

use crate::baselines::{ObservationWindow, PredictorBaseline, TickBudget};
use crate::config::ArkheM1Telemetry;
use crate::metrics::circuit_code_bits_with_tax;
use crate::metrics::facts_code_bits;
use crate::rng::SeedBank;
use crate::structural::economy::EconomyConfig;
use crate::structural::growth::{
    AutonomousRingProposal, DiagnosticBuffer, GrowthConfig, GrowthError, ScoredSproutProposal,
    SproutTelemetry, check_deeper_lag_for_port, diagnose_residual, effective_juvenile_ticks,
    port_cone_register_depth, propose_autonomous_delay_ring_scored_with_telemetry,
    propose_sprout_band_scored_internal,
};
use crate::structural::live::node_output_values;
use crate::structural::sensitivity::{
    CausalAncestry, SensitivitySnapshot, assemble_causal_ancestry, counterfactual_sensitivity,
};
use crate::structural::surgery::{
    EditableCircuit, MarginalDebtLedger, explicit_delay_register_index, port_cone_changed,
    port_is_cone_mature, port_is_primed,
};
use crate::substrate::{Circuit, NodeId, Primitive, SignalSink, Width, WireDelayMode};

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
        }
    }
}

type PendingPrediction = (Vec<u64>, CausalAncestry, SensitivitySnapshot, u64, Vec<u64>);

/// Online gradient-free closed-loop learner driving coordinate-free structural neurogenesis and metabolic economy.
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
    sprout_events: u64,
    starvation_prune_events: u64,
    idle_prune_events: u64,
    ring_closure_events: u64,
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
            sprout_events: 0,
            starvation_prune_events: 0,
            idle_prune_events: 0,
            ring_closure_events: 0,
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
        })
    }

    /// Active port developmental states.
    #[must_use]
    pub fn port_states(&self) -> &[PortDevelopmentalState] {
        &self.port_states
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

    /// Total causal zero-sensitivity idle prunings committed.
    #[must_use]
    pub const fn idle_prune_events(&self) -> u64 {
        self.idle_prune_events
    }

    /// Total autonomous ring-closure events committed.
    #[must_use]
    pub const fn ring_closure_events(&self) -> u64 {
        self.ring_closure_events
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
        let num_ports = self.port_states.len();
        let changed_ports: Vec<usize> = (0..num_ports)
            .filter(|&q| {
                port_cone_changed(pre_circuit, self.editable.circuit(), old_to_new, q as u16)
            })
            .collect();
        for q in changed_ports {
            self.editable
                .economy_mut()
                .port_scorer_mut(q)
                .reset_predictor_counts();
            self.port_states[q].diagnostic_buffer.clear();
            self.port_states[q].ticks_since_last_check = 0;
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
        let mut active_ports = 0usize;
        for q in 0..num_ports {
            if self.is_port_zero_error(q) {
                idle_pool += base_share.saturating_sub(self.port_states[q].replay_ticks_consumed);
            } else {
                active_ports += 1;
            }
        }
        let extra = if active_ports > 0 && !self.is_port_zero_error(port_idx) {
            idle_pool / (active_ports as u64)
        } else {
            0
        };
        let cap = base_share.saturating_add(extra);
        cap.saturating_sub(self.port_states[port_idx].replay_ticks_consumed)
            .min(global_budget.remaining())
    }

    fn commit_sprout(&mut self, port_idx: usize, sprout_circ: Circuit) -> bool {
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
            let d_cone = port_cone_register_depth(self.editable.circuit(), port_idx as u16);
            if d_cone > usize::from(self.growth_config.max_reactive_lag) {
                let eff_juv =
                    effective_juvenile_ticks(d_cone, &self.economy_config, &self.growth_config);
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
                        node.juvenile_ticks = eff_juv;
                    }
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
        }
    }
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

        self.pending_prediction = Some((
            predictions.clone(),
            ancestry,
            snapshot,
            pre_epoch,
            pre_registers,
        ));
        predictions
    }

    fn observe(&mut self, truth: &[u64], budget: &mut TickBudget) -> Option<f64> {
        if self.total_budget_seen.is_none() {
            self.total_budget_seen = Some(budget.remaining().saturating_add(budget.consumed()));
        }
        let (predictions, ancestry, snapshot, pre_epoch, pre_registers) = self
            .pending_prediction
            .take()
            .expect("observe called with pending prediction");

        let input_frame = self.prev_observed_frame.clone();
        self.obs_window
            .push_frame_with_state(truth, pre_epoch, &pre_registers);
        for (slot, &val) in self.prev_observed_frame.iter_mut().zip(truth) {
            *slot = self.editable.circuit().width().normalize(val);
        }

        let num_ports = self.editable.economy().scorers().len();
        let mut port_rewards = vec![0.0; num_ports];
        for p in 0..num_ports {
            let is_driven = self
                .editable
                .circuit()
                .edges()
                .iter()
                .any(|edge| edge.dst == SignalSink::BoundaryOut(p as u16));
            let has_causal_ancestry = ancestry
                .port_shares
                .get(p)
                .is_some_and(|shares| shares.iter().sum::<f64>() > 0.0);
            if is_driven && has_causal_ancestry {
                port_rewards[p] = self
                    .editable
                    .economy_mut()
                    .score_port_single(p, predictions[p], truth[p])
                    .expect("valid port scoring");
            } else {
                let _ = self
                    .editable
                    .economy_mut()
                    .observe_unconnected_port(p, truth[p]);
                port_rewards[p] = 0.0;
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
        let mut maintenance_costs = vec![0.0; n];
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

        let econ_tick = self
            .editable
            .economy_mut()
            .apply_causal_tick(&port_rewards, &ancestry, &maintenance_costs)
            .expect("valid causal tick");
        self.cumulative_net_flux += econ_tick.net_flux;
        self.cumulative_debt_paid += econ_tick.maintenance;
        self.reserve_clipped_count += econ_tick.clipped_nodes as u64;
        self.reserve_clipped_bits += econ_tick.clipped_bits;
        self.reserve_mismatch_floored += econ_tick.mismatch_floored_nodes as u64;

        // Starvation pruning
        if !econ_tick.starved_nodes.is_empty() {
            let pre_prune_circuit = self.editable.circuit().clone();
            if let Ok(old_to_new) = self.editable.prune_starved_nodes() {
                self.ledger.on_prune(
                    self.editable.circuit(),
                    self.economy_config.split_code_tax,
                    &old_to_new,
                );
                self.reset_changed_ports(&pre_prune_circuit, &old_to_new);
                self.starvation_prune_events += 1;
            }
        }

        // Post-juvenile diagnostic buffer
        let mut res = vec![0u64; num_ports];
        for p in 0..num_ports {
            res[p] = self
                .editable
                .circuit()
                .width()
                .sub(truth[p], predictions[p]);
        }
        let node_outs = node_output_values(self.editable.circuit(), self.editable.state());
        let node_count = self.editable.circuit().node_count();
        let mut sensitive = vec![false; node_count];
        for (i, sens) in sensitive.iter_mut().enumerate().take(node_count) {
            *sens = snapshot.output_by_node[i].iter().any(|&s| s > 0.0)
                || snapshot.latch_by_node[i].iter().any(|&s| s > 0.0);
        }

        for p in 0..num_ports {
            let port_u16 = p as u16;
            if port_is_cone_mature(self.editable.circuit(), self.editable.economy(), port_u16)
                && port_is_primed(self.editable.circuit(), &ancestry, port_u16)
            {
                self.port_states[p].diagnostic_buffer.push_frame(
                    &input_frame,
                    truth,
                    &predictions,
                    &res,
                    &node_outs,
                    &sensitive,
                );
                self.port_states[p].ticks_since_last_check += 1;
            }
        }

        // Sprouting and autonomous delay-ring check
        let start_port = (self.tick as usize) % num_ports;
        for offset in 0..num_ports {
            let p = (start_port + offset) % num_ports;
            let port_u16 = p as u16;

            let is_cone_mature =
                port_is_cone_mature(self.editable.circuit(), self.editable.economy(), port_u16);
            let is_primed = port_is_primed(self.editable.circuit(), &ancestry, port_u16);
            let buf_ready =
                self.port_states[p].diagnostic_buffer.len() >= self.growth_config.diagnostic_window;
            let timer_ready =
                self.port_states[p].ticks_since_last_check >= self.growth_config.sprout_interval;

            if !(is_cone_mature && is_primed && buf_ready && timer_ready) {
                continue;
            }

            self.port_states[p].ticks_since_last_check = 0;

            // 1. Idle mature node pruning (D1b.1)
            let idle_nodes = self.port_states[p]
                .diagnostic_buffer
                .idle_mature_nodes(self.editable.economy());
            if !idle_nodes.is_empty() {
                let pre_prune_circuit = self.editable.circuit().clone();
                if let Ok(old_to_new) = self.editable.prune_zero_sensitivity_mature_nodes(
                    &idle_nodes,
                    &self.port_states[p].diagnostic_buffer.inputs,
                    &self.port_states[p].diagnostic_buffer.predictions,
                    self.economy_config.split_code_tax,
                ) {
                    let any_pruned = old_to_new.iter().any(Option::is_none);
                    if any_pruned {
                        self.ledger.on_prune(
                            self.editable.circuit(),
                            self.economy_config.split_code_tax,
                            &old_to_new,
                        );
                        self.reset_changed_ports(&pre_prune_circuit, &old_to_new);
                        self.idle_prune_events += 1;
                        self.tick += 1;
                        return None;
                    }
                }
            }

            // Check if Snapshot 2 needs to be recorded after this port's Sprout 2 has settled
            if self.port_states[p].port_sprout_events == 2
                && self.port_states[p].epoch_snapshots.len() == 2
            {
                self.record_epoch_snapshot(p, 2, false);
            }

            let port_zero_error = self.is_port_zero_error(p);
            let mut port_budget = if num_ports == 1 {
                *budget
            } else {
                TickBudget::new(self.port_budget_allowance(p, budget))
            };

            // 2a. Stage 0 candidate motif sprouting (up to GrowthConfig::max_reactive_lag)
            enum DeferredProposal {
                Sprout(ScoredSproutProposal),
                Ring(AutonomousRingProposal, f64),
            }
            impl DeferredProposal {
                fn j_score(&self) -> f64 {
                    match self {
                        Self::Sprout(prop) => prop.j_cand,
                        Self::Ring(_, j) => *j,
                    }
                }
            }

            let mut sprouted = false;
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
                    sprouted = self.commit_sprout(p, prop.circuit);
                    None
                }
                Ok(Some(prop)) => Some(DeferredProposal::Sprout(prop)),
                _ => None,
            };

            // 2b. Autonomous delay ring proposal under noise (checked BEFORE lag deepening per C2-a)
            let mut ring_tel = SproutTelemetry::default();
            if !sprouted
                && let Some((ring_prop, e_ring_diag, j_ring_diag)) =
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
                    let (band_res, band_pending_bound) = match propose_sprout_band_scored_internal(
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
                            sprouted = self.commit_sprout(p, band_prop.circuit);
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
                    DeferredProposal::Sprout(prop) => self.commit_sprout(p, prop.circuit),
                    DeferredProposal::Ring(ring_prop, _) => self.commit_ring_proposal(ring_prop),
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
            self.sprout_budget_exhausted += stage0_tel.sprout_budget_exhausted
                + ring_tel.sprout_budget_exhausted
                + if sprouted { 0 } else { band_exhausted };
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
            }

            if sprouted {
                break;
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
}
