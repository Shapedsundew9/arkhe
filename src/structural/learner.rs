//! Online closed-loop spatial lattice neurogenesis learner (Milestone 1b, Hypothesis H2).

use serde::{Deserialize, Serialize};

use crate::baselines::{ObservationWindow, PredictorBaseline, TickBudget};
use crate::config::ArkheM1Telemetry;
use crate::metrics::circuit_code_bits_with_tax;
use crate::metrics::facts_code_bits;
use crate::rng::SeedBank;
use crate::structural::economy::EconomyConfig;
use crate::structural::growth::{
    DiagnosticBuffer, GrowthConfig, GrowthError, SproutTelemetry, diagnose_residual,
    propose_autonomous_delay_ring, propose_sprout_with_telemetry,
};
use crate::structural::lattice::{
    BoundaryAnchor, Coord, GridShape, LatticeCircuit, PerimeterSide, WiringTopology,
};
use crate::structural::live::node_output_values;
use crate::structural::sensitivity::{
    CausalAncestry, SensitivitySnapshot, assemble_causal_ancestry, counterfactual_sensitivity,
};
use crate::structural::surgery::{
    EditableLattice, MarginalDebtLedger, explicit_delay_register_index, port_cone_changed,
};
use crate::substrate::{Circuit, NodeId, Primitive, SignalSink, Width, WireDelayMode};

/// Telemetry record capturing residual entropy and diagnostic mutual information across growth epochs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GrowthEpochSnapshot {
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

fn default_anchors(channels: u16, shape: GridShape) -> (Vec<BoundaryAnchor>, Vec<BoundaryAnchor>) {
    if channels <= 1 {
        let mut in_anchors = Vec::with_capacity(channels as usize);
        let mut out_anchors = Vec::with_capacity(channels as usize);
        if channels == 1 {
            in_anchors.push(BoundaryAnchor {
                side: PerimeterSide::North,
                offset: 3 % shape.cols,
            });
            out_anchors.push(BoundaryAnchor {
                side: PerimeterSide::North,
                offset: 4 % shape.cols,
            });
        }
        return (in_anchors, out_anchors);
    }

    let mut pairs: Vec<(BoundaryAnchor, BoundaryAnchor)> = Vec::new();
    let mut used_border_cells = std::collections::BTreeSet::<Coord>::new();
    let sides = [
        PerimeterSide::North,
        PerimeterSide::South,
        PerimeterSide::West,
        PerimeterSide::East,
    ];
    let max_dim = shape.rows.max(shape.cols);

    // Pair adjacent interior border cells per side, starting near the side midpoint so each
    // channel's (in_c, out_c) pair is 1 hop apart with ample surrounding lattice cells.
    let mut base_offsets = Vec::new();
    if max_dim >= 6 {
        base_offsets.push(3);
    }
    let mut off = 1u16;
    while off + 2 < max_dim {
        if off != 3 && off != 4 {
            base_offsets.push(off);
        }
        off += 2;
    }

    for base in base_offsets {
        for side in sides {
            let limit = match side {
                PerimeterSide::North | PerimeterSide::South => shape.cols,
                PerimeterSide::West | PerimeterSide::East => shape.rows,
            };
            if base + 2 < limit {
                let a_in = BoundaryAnchor { side, offset: base };
                let a_out = BoundaryAnchor {
                    side,
                    offset: base + 1,
                };
                if let (Ok(c_in), Ok(c_out)) = (
                    a_in.adjacent_border_cell(shape),
                    a_out.adjacent_border_cell(shape),
                ) && !used_border_cells.contains(&c_in)
                    && !used_border_cells.contains(&c_out)
                    && c_in != c_out
                {
                    used_border_cells.insert(c_in);
                    used_border_cells.insert(c_out);
                    pairs.push((a_in, a_out));
                }
            }
        }
    }

    // Fallback pass over any remaining unclaimed border cells for small grids or high channel counts.
    let mut leftovers = Vec::new();
    for side in sides {
        let limit = match side {
            PerimeterSide::North | PerimeterSide::South => shape.cols,
            PerimeterSide::West | PerimeterSide::East => shape.rows,
        };
        for offset in 0..limit {
            let anchor = BoundaryAnchor { side, offset };
            if let Ok(cell) = anchor.adjacent_border_cell(shape)
                && used_border_cells.insert(cell)
            {
                leftovers.push(anchor);
            }
        }
    }
    for chunk in leftovers.chunks(2) {
        if chunk.len() == 2 {
            pairs.push((chunk[0], chunk[1]));
        }
    }
    if pairs.is_empty() {
        pairs.push((
            BoundaryAnchor {
                side: PerimeterSide::North,
                offset: 0,
            },
            BoundaryAnchor {
                side: PerimeterSide::North,
                offset: 1 % shape.cols,
            },
        ));
    }

    let count = usize::from(channels);
    let mut in_anchors = Vec::with_capacity(count);
    let mut out_anchors = Vec::with_capacity(count);
    for c in 0..count {
        let (a_in, a_out) = pairs[c % pairs.len()];
        in_anchors.push(a_in);
        out_anchors.push(a_out);
    }
    (in_anchors, out_anchors)
}

type PendingPrediction = (Vec<u64>, CausalAncestry, SensitivitySnapshot, u64, Vec<u64>);

/// Online gradient-free closed-loop learner driving spatial lattice neurogenesis and metabolic economy.
pub struct ArkheLearner {
    editable: EditableLattice,
    ledger: MarginalDebtLedger,
    economy_config: EconomyConfig,
    growth_config: GrowthConfig,
    seed_bank: SeedBank,
    sample_count: usize,
    max_relay_hops: u8,
    tick: u64,
    ticks_since_last_check: u64,
    prev_observed_frame: Vec<u64>,
    pending_prediction: Option<PendingPrediction>,
    obs_window: ObservationWindow,
    post_juvenile_diag: DiagnosticBuffer,
    sprout_events: u64,
    starvation_prune_events: u64,
    idle_prune_events: u64,
    ring_closure_events: u64,
    epoch_snapshots: Vec<GrowthEpochSnapshot>,
    cumulative_net_flux: f64,
    cumulative_debt_paid: f64,
    reserve_clipped_count: u64,
    reserve_clipped_bits: f64,
    sprout_blocked_max_nodes: u64,
    candidate_undecidable: u64,
    candidate_list_truncated: u64,
    lag_horizon_exhausted: u64,
    grid_shape: Option<GridShape>,
}

impl ArkheLearner {
    /// Creates a deterministic learner on the default 8x8 lattice in WellMixed mode.
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
        let shape = GridShape::new(8, 8).map_err(|_| GrowthError::InvalidConfig)?;
        Self::new_with_spatial(
            width,
            channels,
            wire_delay,
            WiringTopology::WellMixed,
            shape,
            4,
            EconomyConfig::default(),
            growth,
            seeds,
        )
    }

    /// Full spatial constructor supporting arbitrary grid shapes and topologies.
    ///
    /// # Errors
    ///
    /// Returns [`GrowthError::InvalidConfig`] if configuration validation or lattice lowering fails.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_spatial(
        width: Width,
        channels: u16,
        wire_delay: WireDelayMode,
        topology: WiringTopology,
        shape: GridShape,
        max_relay_hops: u8,
        economy_config: EconomyConfig,
        growth_config: GrowthConfig,
        seed_bank: SeedBank,
    ) -> Result<Self, GrowthError> {
        if !growth_config.is_valid() {
            return Err(GrowthError::InvalidConfig);
        }
        let (in_anchors, out_anchors) = default_anchors(channels, shape);
        let lattice =
            LatticeCircuit::new(shape, width, wire_delay, topology, in_anchors, out_anchors)
                .map_err(|_| GrowthError::InvalidConfig)?;

        let editable = EditableLattice::new(lattice, economy_config)
            .map_err(|_| GrowthError::InvalidConfig)?;

        let channels_usize = usize::from(channels);
        let obs_capacity =
            (economy_config.juvenile_ticks as usize) + 2 * growth_config.diagnostic_window;

        Ok(Self {
            editable,
            ledger: MarginalDebtLedger::new(),
            economy_config,
            growth_config,
            seed_bank,
            sample_count: 8,
            max_relay_hops,
            tick: 0,
            ticks_since_last_check: 0,
            prev_observed_frame: vec![0; channels_usize],
            pending_prediction: None,
            obs_window: ObservationWindow::new(channels, obs_capacity),
            post_juvenile_diag: DiagnosticBuffer::new(growth_config.diagnostic_window),
            sprout_events: 0,
            starvation_prune_events: 0,
            idle_prune_events: 0,
            ring_closure_events: 0,
            epoch_snapshots: Vec::new(),
            cumulative_net_flux: 0.0,
            cumulative_debt_paid: 0.0,
            reserve_clipped_count: 0,
            reserve_clipped_bits: 0.0,
            sprout_blocked_max_nodes: 0,
            candidate_undecidable: 0,
            candidate_list_truncated: 0,
            lag_horizon_exhausted: 0,
            grid_shape: if topology == WiringTopology::WellMixed {
                None
            } else {
                Some(shape)
            },
        })
    }

    /// Current editable lattice.
    #[must_use]
    pub const fn editable(&self) -> &EditableLattice {
        &self.editable
    }

    /// Mutable reference to current editable lattice.
    pub fn editable_mut(&mut self) -> &mut EditableLattice {
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

    /// Diagnostic epoch snapshots captured across developmental milestones.
    #[must_use]
    pub fn epoch_snapshots(&self) -> &[GrowthEpochSnapshot] {
        &self.epoch_snapshots
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
        let shape = if self.editable.lattice().topology() == WiringTopology::WellMixed {
            None
        } else {
            self.grid_shape
        };
        let code_bits = circuit_code_bits_with_tax(
            self.editable.circuit(),
            self.editable.lattice().topology(),
            shape,
            0,
            self.economy_config.split_code_tax,
        );
        let null_calibrated_mdl_delta_bits = code_bits - self.cumulative_net_flux;

        ArkheM1Telemetry {
            grid_rows: self.grid_shape.map_or(8, |s| s.rows),
            grid_cols: self.grid_shape.map_or(8, |s| s.cols),
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
            lag_horizon_exhausted: self.lag_horizon_exhausted,
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
            self.ledger.ensure_capacity(
                self.editable.circuit(),
                self.editable.lattice().topology(),
                self.grid_shape,
                self.economy_config.split_code_tax,
            );
        }
        let mut maintenance_costs = vec![0.0; n];
        for (i, cost) in maintenance_costs.iter_mut().enumerate().take(n) {
            let node = &self.editable.economy().nodes()[i];
            if node.juvenile_ticks == 0 && !self.ledger.debt_paid[i] {
                *cost = self.ledger.node_debts[i];
                self.ledger.debt_paid[i] = true;
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

        // Starvation pruning
        if !econ_tick.starved_nodes.is_empty() {
            let pre_prune_lattice = self.editable.lattice().clone();
            if let Ok(old_to_new) = self.editable.prune_starved_nodes() {
                self.ledger.on_prune(
                    self.editable.circuit(),
                    self.editable.lattice().topology(),
                    self.grid_shape,
                    self.economy_config.split_code_tax,
                    &old_to_new,
                );
                let changed_ports: Vec<usize> = (0..num_ports)
                    .filter(|&p| {
                        port_cone_changed(
                            &pre_prune_lattice,
                            self.editable.lattice(),
                            &old_to_new,
                            p as u16,
                        )
                    })
                    .collect();
                for p in changed_ports {
                    self.editable
                        .economy_mut()
                        .port_scorer_mut(p)
                        .reset_predictor_counts();
                }
                self.starvation_prune_events += 1;
                self.post_juvenile_diag.clear();
                self.ticks_since_last_check = 0;
            }
        }

        // Post-juvenile diagnostic buffer
        let all_mature = self.editable.circuit().node_count() == 0
            || self
                .editable
                .economy()
                .nodes()
                .iter()
                .all(|node| node.juvenile_ticks == 0);
        let all_driven_ports_primed = (0..num_ports).all(|p| {
            let is_driven = self
                .editable
                .circuit()
                .edges()
                .iter()
                .any(|edge| edge.dst == SignalSink::BoundaryOut(p as u16));
            !is_driven
                || ancestry
                    .port_shares
                    .get(p)
                    .is_some_and(|shares| shares.iter().sum::<f64>() > 0.0)
        });

        if all_mature && all_driven_ports_primed {
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
            self.post_juvenile_diag.push_frame(
                &input_frame,
                truth,
                &predictions,
                &res,
                &node_outs,
                &sensitive,
            );
            self.ticks_since_last_check += 1;
        }

        // Sprouting and autonomous delay-ring check
        if all_mature
            && all_driven_ports_primed
            && self.post_juvenile_diag.len() >= self.growth_config.diagnostic_window
            && self.ticks_since_last_check >= self.growth_config.sprout_interval
        {
            self.ticks_since_last_check = 0;

            // 1. Idle mature node pruning (D1b.1)
            let idle_nodes = self
                .post_juvenile_diag
                .idle_mature_nodes(self.editable.economy());
            if !idle_nodes.is_empty() {
                let pre_prune_lattice = self.editable.lattice().clone();
                if let Ok(old_to_new) = self.editable.prune_zero_sensitivity_mature_nodes(
                    &idle_nodes,
                    &self.post_juvenile_diag.inputs,
                    &self.post_juvenile_diag.predictions,
                    self.economy_config.split_code_tax,
                ) {
                    let any_pruned = old_to_new.iter().any(Option::is_none);
                    if any_pruned {
                        self.ledger.on_prune(
                            self.editable.circuit(),
                            self.editable.lattice().topology(),
                            self.grid_shape,
                            self.economy_config.split_code_tax,
                            &old_to_new,
                        );
                        let changed_ports: Vec<usize> = (0..num_ports)
                            .filter(|&p| {
                                port_cone_changed(
                                    &pre_prune_lattice,
                                    self.editable.lattice(),
                                    &old_to_new,
                                    p as u16,
                                )
                            })
                            .collect();
                        for p in changed_ports {
                            self.editable
                                .economy_mut()
                                .port_scorer_mut(p)
                                .reset_predictor_counts();
                        }
                        self.idle_prune_events += 1;
                        self.post_juvenile_diag.clear();
                        self.tick += 1;
                        return None;
                    }
                }
            }

            // Check if Snapshot 2 needs to be recorded after Sprout 2 has settled
            if self.sprout_events == 2 && self.epoch_snapshots.len() == 2 {
                let residuals: Vec<u64> = self
                    .post_juvenile_diag
                    .residuals
                    .iter()
                    .map(|f| f[0])
                    .collect();
                let features = self.post_juvenile_diag.build_signal_pool(
                    self.editable.lattice(),
                    &self.obs_window,
                    self.max_relay_hops,
                    0,
                );
                if let Ok(diag_res) = diagnose_residual(
                    self.editable.circuit().width(),
                    &residuals,
                    &features,
                    self.growth_config,
                ) {
                    let mismatches = self
                        .post_juvenile_diag
                        .truth
                        .iter()
                        .zip(&self.post_juvenile_diag.predictions)
                        .filter(|(t, p)| t[0] != p[0])
                        .count();
                    let w = self.post_juvenile_diag.len() as u64;
                    let l2 = facts_code_bits(self.editable.circuit().width(), w, mismatches as u64)
                        / (w as f64);
                    self.epoch_snapshots.push(GrowthEpochSnapshot {
                        tick: self.tick,
                        epoch: 2,
                        kt_loss_bits_per_symbol: l2,
                        residual_entropy_bits: diag_res.residual_entropy_bits,
                        best_feature_mi_bits: None,
                        node_count: self.editable.circuit().node_count(),
                    });
                }
            }

            // 2. Evaluate Candidate Sprout or Ring Closure across output ports
            for p in 0..num_ports {
                let port_u16 = p as u16;

                // 2a. Candidate motif sprouting
                let mut sprouted = false;
                let mut sprout_tel = SproutTelemetry::default();
                if self.editable.circuit().node_count() < self.growth_config.max_nodes {
                    if let Ok(Some(sprout_lat)) = propose_sprout_with_telemetry(
                        &self.editable,
                        &self.obs_window,
                        &self.post_juvenile_diag,
                        &self.growth_config,
                        &self.economy_config,
                        budget,
                        port_u16,
                        self.max_relay_hops,
                        &mut sprout_tel,
                    ) {
                        // Diagnostic snapshot for G1B.1 before committing sprout
                    if p == 0 && (self.sprout_events == 0 || self.sprout_events == 1) {
                        let residuals: Vec<u64> = self
                            .post_juvenile_diag
                            .residuals
                            .iter()
                            .map(|f| f[0])
                            .collect();
                        let features = self.post_juvenile_diag.build_signal_pool(
                            self.editable.lattice(),
                            &self.obs_window,
                            self.max_relay_hops,
                            0,
                        );
                        if let Ok(diag_res) = diagnose_residual(
                            self.editable.circuit().width(),
                            &residuals,
                            &features,
                            self.growth_config,
                        ) {
                            let mismatches = self
                                .post_juvenile_diag
                                .truth
                                .iter()
                                .zip(&self.post_juvenile_diag.predictions)
                                .filter(|(t, p)| t[0] != p[0])
                                .count();
                            let w = self.post_juvenile_diag.len() as u64;
                            let loss = facts_code_bits(
                                self.editable.circuit().width(),
                                w,
                                mismatches as u64,
                            ) / (w as f64);
                            let mi = diag_res
                                .best_feature
                                .as_ref()
                                .map(|f| f.mutual_information_bits);
                            self.epoch_snapshots.push(GrowthEpochSnapshot {
                                tick: self.tick,
                                epoch: self.sprout_events as usize,
                                kt_loss_bits_per_symbol: loss,
                                residual_entropy_bits: diag_res.residual_entropy_bits,
                                best_feature_mi_bits: mi,
                                node_count: self.editable.circuit().node_count(),
                            });
                        }
                    }

                    let old_to_new: Vec<Option<NodeId>> = (0..self.editable.circuit().node_count())
                        .map(|i| Some(NodeId(i as u32)))
                        .collect();
                    if self
                        .editable
                        .recompile_preserving_state(sprout_lat, &old_to_new)
                        .is_ok()
                    {
                        self.ledger.on_sprout(
                            self.editable.circuit(),
                            self.editable.lattice().topology(),
                            self.grid_shape,
                            self.economy_config.split_code_tax,
                            &old_to_new,
                        );
                        self.editable
                            .economy_mut()
                            .port_scorer_mut(p)
                            .reset_predictor_counts();
                        self.sprout_events += 1;
                        self.post_juvenile_diag.clear();
                        sprouted = true;
                    }
                }
            } else {
                self.sprout_blocked_max_nodes += 1;
            }
                self.sprout_blocked_max_nodes += sprout_tel.sprout_blocked_max_nodes;
                self.candidate_undecidable += sprout_tel.candidate_undecidable;
                self.candidate_list_truncated += sprout_tel.candidate_list_truncated;
                self.lag_horizon_exhausted += sprout_tel.lag_horizon_exhausted;

                if sprouted {
                    break;
                }

                // 2b. Autonomous delay ring proposal under noise
                if let Some(ring_prop) = propose_autonomous_delay_ring(
                    &self.editable,
                    &self.obs_window,
                    &self.growth_config,
                    &self.economy_config,
                    budget,
                    port_u16,
                    self.max_relay_hops,
                ) {
                    let old_to_new = ring_prop.old_to_new;
                    let ring_delay_nodes = ring_prop.ring_delay_nodes;
                    let period = ring_prop.period;
                    let template = ring_prop.template;
                    if self
                        .editable
                        .recompile_preserving_state(ring_prop.lattice, &old_to_new)
                        .is_ok()
                    {
                        self.ledger.on_sprout(
                            self.editable.circuit(),
                            self.editable.lattice().topology(),
                            self.grid_shape,
                            self.economy_config.split_code_tax,
                            &old_to_new,
                        );
                        let mut regs = self.editable.state().registers().to_vec();
                        let mut ok_regs = true;
                        for (j, &delay_node) in ring_delay_nodes.iter().enumerate() {
                            if let Some(reg_idx) =
                                explicit_delay_register_index(self.editable.circuit(), delay_node)
                            {
                                regs[reg_idx] = template
                                    [(((self.tick + 1) as usize) + (period - 1 - j)) % period];
                            } else {
                                ok_regs = false;
                                break;
                            }
                        }
                        if ok_regs {
                            let _ = self.editable.set_registers(&regs);
                        }
                        self.editable
                            .economy_mut()
                            .port_scorer_mut(p)
                            .reset_predictor_counts();
                        self.ring_closure_events += 1;
                        self.post_juvenile_diag.clear();
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

    fn grid_shape(&self) -> Option<GridShape> {
        self.grid_shape
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
    fn default_anchors_distribute_multi_channel_without_border_collisions() {
        let shape = GridShape::new(8, 8).unwrap();
        let (ins, outs) = default_anchors(8, shape);
        assert_eq!(ins.len(), 8);
        assert_eq!(outs.len(), 8);
        let mut cells = std::collections::BTreeSet::new();
        for a in ins.iter().chain(outs.iter()) {
            let cell = a.adjacent_border_cell(shape).unwrap();
            assert!(cells.insert(cell), "duplicate border cell {cell:?}");
        }
    }

    #[test]
    fn ticks_per_hop_multistage_tasks_run_without_missing_ancestry_panic() {
        let shape = GridShape::new(8, 8).unwrap();
        for topology in [WiringTopology::WellMixed, WiringTopology::LocalVonNeumann] {
            let mut learner = ArkheLearner::new_with_spatial(
                Width::BITS_1,
                1,
                WireDelayMode::TicksPerHop,
                topology,
                shape,
                4,
                EconomyConfig::default(),
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
        let shape = GridShape::new(8, 8).unwrap();
        let mut learner = ArkheLearner::new_with_spatial(
            Width::BITS_1,
            2,
            WireDelayMode::Combinational,
            WiringTopology::LocalVonNeumann,
            shape,
            4,
            EconomyConfig::default(),
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
}
