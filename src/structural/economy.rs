//! Metabolic reserves, surprisal rewards, maintenance, and conserved trophic flux.

use serde::{Deserialize, Serialize};

use crate::metrics::{NullModelKind, PointForecastScorer};
use crate::structural::sensitivity::{CausalAncestry, NodeActivity};
use crate::substrate::Width;

/// Reward rule used to pay exact-match predictions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RewardMode {
    /// Calibrated sequential point-forecast reward over prequential null model (D2, default).
    #[default]
    CalibratedPointForecast,
    /// Exact-match surprisal minus its pre-observation blind-luck expectation.
    CenteredSurprisal,
    /// Exact-match surprisal without chance-baseline subtraction.
    UncenteredSurprisal,
    /// Exact-match reward centered at the uniform chance accuracy.
    RawAccuracy,
}

/// Scores a single causal `(prediction, fact)` step against `scorer` under `mode`, advancing the
/// underlying null model identically to online [`EconomyState::score_port_single`].
pub fn score_with_reward_mode(
    scorer: &mut PointForecastScorer,
    mode: RewardMode,
    width: Width,
    prediction: u64,
    fact: u64,
) -> f64 {
    match mode {
        RewardMode::CalibratedPointForecast => scorer.observe(prediction, fact),
        RewardMode::CenteredSurprisal => {
            let prediction = width.normalize(prediction);
            let fact = width.normalize(fact);
            let p = scorer.null_model().probability(prediction);
            let surprisal = -p.log2();
            let correct = if prediction == fact { 1.0 } else { 0.0 };
            let r = (correct - p) * surprisal;
            scorer.null_model_mut().observe_and_surprisal(fact);
            r
        }
        RewardMode::UncenteredSurprisal => {
            let prediction = width.normalize(prediction);
            let fact = width.normalize(fact);
            let r = if prediction == fact {
                -scorer.null_model().probability(fact).log2()
            } else {
                0.0
            };
            scorer.null_model_mut().observe_and_surprisal(fact);
            r
        }
        RewardMode::RawAccuracy => {
            let prediction = width.normalize(prediction);
            let fact = width.normalize(fact);
            let correct = if prediction == fact { 1.0 } else { 0.0 };
            let r = correct - 2.0f64.powi(-i32::from(width.bits()));
            scorer.null_model_mut().observe_and_surprisal(fact);
            r
        }
    }
}

const fn default_true() -> bool {
    true
}

/// Numerical floor on `1 - q` in the HorizonDerived mismatch-loss ceiling to prevent `log2(./0)`.
pub const RESERVE_MISMATCH_DENOM_FLOOR: f64 = 1e-12;

/// Policy governing metabolic reserve ceilings on mature nodes (§3.1.2).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
pub enum ReserveCap {
    /// Grounded horizon-derived reserve ceiling (Option B, Charter §3.1.2).
    /// $E_{\max}(v) = \max(E_{\text{init}}, W_{\text{obs}} \cdot \text{drain\_loss})$.
    #[default]
    HorizonDerived,
    /// Uncapped: reserves accumulate unbounded (exact conservation).
    Uncapped,
    /// Fixed scalar reserve cap in bits.
    Fixed(f64),
}

/// Node-economy settings. The defaults are the preregistered M1 values.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EconomyConfig {
    /// Discount applied to ancestry carried through synchronous registers.
    pub trace_discount: f64,
    /// Initial reserve assigned to each placed node.
    pub initial_energy: f64,
    /// Ticks for which new nodes are protected from maintenance and negative payout.
    pub juvenile_ticks: u32,
    /// Reserve ceiling policy.
    pub reserve_cap: ReserveCap,
    /// Null model used to compute prequential reward.
    pub null_model: NullModelKind,
    /// Exact-match reward rule.
    pub reward_mode: RewardMode,
    /// Whether Split nodes and their second outgoing wire pay description debt / code length.
    #[serde(default = "default_true")]
    pub split_code_tax: bool,
}

impl Default for EconomyConfig {
    fn default() -> Self {
        Self {
            trace_discount: 0.90,
            initial_energy: 4.0,
            juvenile_ticks: 128,
            reserve_cap: ReserveCap::HorizonDerived,
            null_model: NullModelKind::LaplaceFrequency,
            reward_mode: RewardMode::CalibratedPointForecast,
            split_code_tax: true,
        }
    }
}

impl EconomyConfig {
    /// Checks that energy and trace settings are finite and physically meaningful.
    pub fn validate(self) -> Result<(), EconomyError> {
        if !self.trace_discount.is_finite()
            || !(0.0..1.0).contains(&self.trace_discount)
            || !self.initial_energy.is_finite()
            || self.initial_energy <= 0.0
        {
            return Err(EconomyError::InvalidConfig);
        }
        if let ReserveCap::Fixed(cap) = self.reserve_cap
            && (!cap.is_finite() || cap < self.initial_energy)
        {
            return Err(EconomyError::InvalidConfig);
        }
        Ok(())
    }
}

/// Energy and persistent activity state for one node.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeEconomy {
    /// Current metabolic reserve in bits.
    pub energy: f64,
    /// Remaining juvenile grace ticks.
    pub juvenile_ticks: u32,
    /// Monotonic ring-identity activity history for this node.
    pub activity: NodeActivity,
    /// Allocated marginal description debt $\Delta L_v$ in bits (D1b.3).
    pub description_debt: f64,
    /// Whether this node has paid its lump-sum maturity debt upon graduation.
    pub debt_paid: bool,
}

/// Error returned by invalid economy configuration or incompatible tick data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EconomyError {
    /// One or more economy configuration values violate the documented bounds.
    InvalidConfig,
    /// Reward, ancestry, or maintenance dimensions do not match the state.
    DimensionMismatch,
    /// Energy, maintenance, or ancestry contains a non-finite or invalid value.
    InvalidTickData,
    /// A non-zero reward has no causal ancestry to receive it.
    MissingAncestry { port: usize },
    /// The requested register index is outside the eligibility table.
    InvalidRegister { register: usize },
    /// A node or register remapping contains an out-of-range or duplicate destination.
    InvalidStructureMapping,
}

impl std::fmt::Display for EconomyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfig => write!(f, "invalid metabolic economy configuration"),
            Self::DimensionMismatch => write!(f, "economy tick dimensions do not match state"),
            Self::InvalidTickData => write!(f, "economy tick data must be finite and non-negative"),
            Self::MissingAncestry { port } => {
                write!(f, "reward on output port {port} has no causal ancestry")
            }
            Self::InvalidRegister { register } => {
                write!(f, "register {register} is outside the eligibility table")
            }
            Self::InvalidStructureMapping => {
                write!(
                    f,
                    "node or register mapping is not a valid partial bijection"
                )
            }
        }
    }
}

impl std::error::Error for EconomyError {}

/// Result of applying one tick of metabolic flux and maintenance.
#[derive(Clone, Debug, PartialEq)]
pub struct EconomyTick {
    /// Total net reward across output ports.
    pub net_flux: f64,
    /// Total maintenance actually deducted (juveniles are exempt).
    pub maintenance: f64,
    /// Per-node payout from port rewards before maintenance and clipping.
    pub node_credit: Vec<f64>,
    /// Per-node net energy change after reserve clipping.
    pub node_delta: Vec<f64>,
    /// Nodes whose mature reserves are now depleted.
    pub starved_nodes: Vec<usize>,
    /// Number of nodes whose reserves were clipped by max_energy_reserve on this tick.
    pub clipped_nodes: usize,
    /// Total energy bits clipped by max_energy_reserve on this tick.
    pub clipped_bits: f64,
    /// Number of mature nodes for which `1 - q` hit `RESERVE_MISMATCH_DENOM_FLOOR` on this tick.
    pub mismatch_floored_nodes: usize,
}

/// Node-local reserves, port null models, and synchronous-register eligibility traces.
#[derive(Clone, Debug)]
pub struct EconomyState {
    config: EconomyConfig,
    width: Width,
    observation_horizon: usize,
    nodes: Vec<NodeEconomy>,
    scorers: Vec<PointForecastScorer>,
    register_eligibility: Vec<Vec<f64>>,
}

impl EconomyState {
    /// Creates energy state for `nodes`, `outputs`, `registers`, and unified observation horizon `W_obs`.
    pub fn new(
        config: EconomyConfig,
        width: Width,
        nodes: usize,
        outputs: usize,
        registers: usize,
        observation_horizon: usize,
    ) -> Result<Self, EconomyError> {
        config.validate()?;
        if observation_horizon == 0 {
            return Err(EconomyError::InvalidConfig);
        }
        let node_state = NodeEconomy {
            energy: config.initial_energy,
            juvenile_ticks: config.juvenile_ticks,
            activity: NodeActivity::default(),
            description_debt: 0.0,
            debt_paid: false,
        };
        Ok(Self {
            config,
            width,
            observation_horizon,
            nodes: vec![node_state; nodes],
            scorers: (0..outputs)
                .map(|_| PointForecastScorer::new(width, config.null_model))
                .collect(),
            register_eligibility: vec![vec![0.0; nodes]; registers],
        })
    }

    /// Unified observation horizon $W_{\text{obs}}$ in ticks used by HorizonDerived reserve capping.
    #[must_use]
    pub const fn observation_horizon(&self) -> usize {
        self.observation_horizon
    }

    /// Economy settings.
    #[must_use]
    pub const fn config(&self) -> EconomyConfig {
        self.config
    }

    /// Per-node energy state.
    #[must_use]
    pub fn nodes(&self) -> &[NodeEconomy] {
        &self.nodes
    }

    /// Mutable reference to per-node energy state.
    pub fn nodes_mut(&mut self) -> &mut [NodeEconomy] {
        &mut self.nodes
    }

    /// Returns a reference to the economy state of node `index`.
    #[must_use]
    pub fn node(&self, index: usize) -> Option<&NodeEconomy> {
        self.nodes.get(index)
    }

    /// Returns a mutable reference to the economy state of node `index`.
    pub fn node_mut(&mut self, index: usize) -> Option<&mut NodeEconomy> {
        self.nodes.get_mut(index)
    }

    /// Assigns allocated description debt to nodes after a structural sprout (§2.2).
    pub fn assign_sprout_debts(&mut self, debts: &[f64]) {
        for (i, &debt) in debts.iter().enumerate() {
            if let Some(node) = self.nodes.get_mut(i) {
                node.description_debt = debt;
                node.debt_paid = false;
            }
        }
    }

    /// Output port point-forecast scorers.
    #[must_use]
    pub fn scorers(&self) -> &[PointForecastScorer] {
        &self.scorers
    }

    /// Mutable reference to an output port point-forecast scorer.
    pub fn port_scorer_mut(&mut self, port: usize) -> &mut PointForecastScorer {
        &mut self.scorers[port]
    }

    /// Updates node emission recency weights against outputs emitted on this tick (M1a redesign D4).
    ///
    /// If an output is non-identity, its weight is set to 1.0; otherwise it is decayed by
    /// `trace_discount` ($\gamma$).
    pub fn record_activity(&mut self, outputs: &[u64]) -> Result<(), EconomyError> {
        if outputs.len() != self.nodes.len() {
            return Err(EconomyError::DimensionMismatch);
        }
        let gamma = self.config.trace_discount;
        for (node, &output) in self.nodes.iter_mut().zip(outputs) {
            let norm = self.width.normalize(output);
            if norm != 0 {
                node.activity.nonzero_weight = 1.0;
            } else {
                node.activity.nonzero_weight *= gamma;
            }
            if norm != 1 {
                node.activity.non_one_weight = 1.0;
            } else {
                node.activity.non_one_weight *= gamma;
            }
        }
        Ok(())
    }

    /// Monotonic emission activity history across all nodes.
    #[must_use]
    pub fn activity(&self) -> Vec<NodeActivity> {
        self.nodes.iter().map(|node| node.activity).collect()
    }

    /// Eligibility ancestry for a synchronous register.
    #[must_use]
    pub fn register_eligibility(&self) -> &[Vec<f64>] {
        &self.register_eligibility
    }

    /// Rebuilds node and register state after graph compaction or growth.
    ///
    /// Existing entries are copied through partial one-to-one maps; unmapped nodes receive the
    /// configured initial reserve and unmapped registers receive zero eligibility.
    pub fn remap_structure(
        &self,
        old_to_new_nodes: &[Option<usize>],
        new_node_count: usize,
        old_to_new_registers: &[Option<usize>],
        new_register_count: usize,
    ) -> Result<Self, EconomyError> {
        if old_to_new_nodes.len() != self.nodes.len()
            || old_to_new_registers.len() != self.register_eligibility.len()
        {
            return Err(EconomyError::DimensionMismatch);
        }
        let mut occupied_nodes = vec![false; new_node_count];
        for &mapped in old_to_new_nodes.iter().flatten() {
            let Some(occupied) = occupied_nodes.get_mut(mapped) else {
                return Err(EconomyError::InvalidStructureMapping);
            };
            if *occupied {
                return Err(EconomyError::InvalidStructureMapping);
            }
            *occupied = true;
        }
        let mut occupied_registers = vec![false; new_register_count];
        for &mapped in old_to_new_registers.iter().flatten() {
            let Some(occupied) = occupied_registers.get_mut(mapped) else {
                return Err(EconomyError::InvalidStructureMapping);
            };
            if *occupied {
                return Err(EconomyError::InvalidStructureMapping);
            }
            *occupied = true;
        }

        let mut remapped = Self::new(
            self.config,
            self.width,
            new_node_count,
            self.scorers.len(),
            new_register_count,
            self.observation_horizon,
        )?;
        remapped.scorers.clone_from(&self.scorers);
        for (old, mapped) in old_to_new_nodes.iter().enumerate() {
            if let Some(new) = mapped {
                remapped.nodes[*new] = self.nodes[old];
            }
        }
        for (old_register, mapped_register) in old_to_new_registers.iter().enumerate() {
            let Some(new_register) = mapped_register else {
                continue;
            };
            for (old_node, mapped_node) in old_to_new_nodes.iter().enumerate() {
                if let Some(new_node) = mapped_node {
                    remapped.register_eligibility[*new_register][*new_node] =
                        self.register_eligibility[old_register][old_node];
                }
            }
        }
        Ok(remapped)
    }

    /// Prequential reward on each output port, updating its null model after scoring the fact.
    pub fn observe_predictions(
        &mut self,
        predictions: &[u64],
        facts: &[u64],
    ) -> Result<Vec<f64>, EconomyError> {
        if predictions.len() != self.scorers.len() || facts.len() != predictions.len() {
            return Err(EconomyError::DimensionMismatch);
        }
        let mut rewards = Vec::with_capacity(predictions.len());
        for (i, (&prediction, &fact)) in predictions.iter().zip(facts).enumerate() {
            rewards.push(self.score_port_single(i, prediction, fact)?);
        }
        Ok(rewards)
    }

    /// Computes reward for a single output port against its observation.
    pub fn score_port_single(
        &mut self,
        port: usize,
        prediction: u64,
        fact: u64,
    ) -> Result<f64, EconomyError> {
        if port >= self.scorers.len() {
            return Err(EconomyError::DimensionMismatch);
        }
        let scorer = &mut self.scorers[port];
        Ok(score_with_reward_mode(
            scorer,
            self.config.reward_mode,
            self.width,
            prediction,
            fact,
        ))
    }

    /// Advances null-model statistics for an unconnected output port without scoring or requiring ancestry.
    pub fn observe_unconnected_port(
        &mut self,
        port: usize,
        raw_fact: u64,
    ) -> Result<(), EconomyError> {
        if port >= self.scorers.len() {
            return Err(EconomyError::DimensionMismatch);
        }
        self.scorers[port]
            .null_model_mut()
            .observe_and_surprisal(raw_fact);
        Ok(())
    }

    /// Applies conserved port rewards and per-node maintenance for one tick.
    ///
    /// Each ancestry vector is normalised independently per output port before payout.
    pub fn apply_tick(
        &mut self,
        port_rewards: &[f64],
        port_ancestry: &[Vec<f64>],
        maintenance_costs: &[f64],
    ) -> Result<EconomyTick, EconomyError> {
        if port_rewards.len() != self.scorers.len()
            || port_ancestry.len() != port_rewards.len()
            || maintenance_costs.len() != self.nodes.len()
        {
            return Err(EconomyError::DimensionMismatch);
        }
        if port_rewards.iter().any(|v| !v.is_finite())
            || maintenance_costs.iter().any(|v| !v.is_finite() || *v < 0.0)
            || !port_rewards.iter().sum::<f64>().is_finite()
            || !maintenance_costs.iter().sum::<f64>().is_finite()
            || port_ancestry.iter().any(|ancestry| {
                ancestry.len() != self.nodes.len()
                    || ancestry.iter().any(|v| !v.is_finite() || *v < 0.0)
                    || !ancestry.iter().sum::<f64>().is_finite()
            })
        {
            return Err(EconomyError::InvalidTickData);
        }

        let mut delta = vec![0.0; self.nodes.len()];
        for (port, (&reward, ancestry)) in port_rewards.iter().zip(port_ancestry).enumerate() {
            let total_ancestry: f64 = ancestry.iter().sum();
            if total_ancestry == 0.0 {
                if reward != 0.0 {
                    return Err(EconomyError::MissingAncestry { port });
                }
                continue;
            }
            for (index, (&share, node)) in ancestry.iter().zip(&self.nodes).enumerate() {
                let credit = reward * (share / total_ancestry);
                if credit >= 0.0 || node.juvenile_ticks == 0 {
                    delta[index] += credit;
                }
            }
        }
        let node_credit = delta.clone();

        let mut maintenance = 0.0;
        for (index, node) in self.nodes.iter().enumerate() {
            if node.juvenile_ticks == 0 {
                delta[index] -= maintenance_costs[index];
                maintenance += maintenance_costs[index];
            }
        }
        if delta.iter().any(|value| !value.is_finite()) || !maintenance.is_finite() {
            return Err(EconomyError::InvalidTickData);
        }

        let mut starved_nodes = Vec::new();
        let mut next_energies = Vec::with_capacity(self.nodes.len());
        let mut clipped_nodes = 0usize;
        let mut clipped_bits = 0.0f64;
        let mut mismatch_floored_nodes = 0usize;
        for (index, node) in self.nodes.iter().enumerate() {
            let unclipped = node.energy + delta[index];
            if !unclipped.is_finite() {
                return Err(EconomyError::InvalidTickData);
            }
            let mut node_floored = false;
            let (next_energy, clipped) = if node.juvenile_ticks > 0 {
                (unclipped, 0.0)
            } else {
                let maybe_cap = match self.config.reserve_cap {
                    ReserveCap::Uncapped => None,
                    ReserveCap::Fixed(cap) => Some(cap),
                    ReserveCap::HorizonDerived => {
                        let w_obs = self.observation_horizon as f64;
                        let mut drain_rate = 0.0;
                        for (port, ancestry) in port_ancestry.iter().enumerate() {
                            let total_ancestry: f64 = ancestry.iter().sum();
                            if total_ancestry > 0.0 {
                                let share = ancestry[index] / total_ancestry;
                                if share > 0.0 && port < self.scorers.len() {
                                    let scorer = &self.scorers[port];
                                    let q = scorer.q_hat();
                                    let p_null = 1.0 / (2.0f64.powi(i32::from(self.width.bits())));
                                    if q > p_null {
                                        let one_minus_q = 1.0 - q;
                                        let denom = if one_minus_q < RESERVE_MISMATCH_DENOM_FLOOR {
                                            node_floored = true;
                                            RESERVE_MISMATCH_DENOM_FLOOR
                                        } else {
                                            one_minus_q
                                        };
                                        let mismatch_loss =
                                            ((1.0 - p_null) / denom).log2().max(0.0);
                                        drain_rate += share * mismatch_loss;
                                    }
                                }
                            }
                        }
                        Some((w_obs * drain_rate).max(self.config.initial_energy))
                    }
                };
                if let Some(cap) = maybe_cap {
                    if unclipped > cap {
                        (cap, unclipped - cap)
                    } else {
                        (unclipped, 0.0)
                    }
                } else {
                    (unclipped, 0.0)
                }
            };
            if node_floored {
                mismatch_floored_nodes += 1;
            }
            if clipped > 0.0 {
                clipped_nodes += 1;
                clipped_bits += clipped;
            }
            next_energies.push(next_energy);
        }
        for (index, (node, next_energy)) in self.nodes.iter_mut().zip(next_energies).enumerate() {
            let previous_energy = node.energy;
            node.energy = next_energy;
            if node.juvenile_ticks > 0 {
                node.juvenile_ticks -= 1;
            } else if node.energy <= 0.0 {
                starved_nodes.push(index);
            }
            delta[index] = node.energy - previous_energy;
        }

        Ok(EconomyTick {
            net_flux: port_rewards.iter().sum(),
            maintenance,
            node_credit,
            node_delta: delta,
            starved_nodes,
            clipped_nodes,
            clipped_bits,
            mismatch_floored_nodes,
        })
    }

    /// Applies a sensitivity-assembled tick and latches its register ancestry traces.
    pub fn apply_causal_tick(
        &mut self,
        port_rewards: &[f64],
        ancestry: &CausalAncestry,
        maintenance_costs: &[f64],
    ) -> Result<EconomyTick, EconomyError> {
        if ancestry.next_register_eligibility.len() != self.register_eligibility.len()
            || ancestry.next_register_eligibility.iter().any(|trace| {
                trace.len() != self.nodes.len()
                    || trace.iter().any(|value| !value.is_finite() || *value < 0.0)
            })
        {
            return Err(EconomyError::DimensionMismatch);
        }
        let tick = self.apply_tick(port_rewards, &ancestry.port_shares, maintenance_costs)?;
        for (register, trace) in ancestry.next_register_eligibility.iter().enumerate() {
            self.register_eligibility[register].copy_from_slice(trace);
        }
        Ok(tick)
    }

    /// Stores ancestry for the next tick of a synchronous register.
    pub fn set_register_eligibility(
        &mut self,
        register: usize,
        ancestry: &[f64],
    ) -> Result<(), EconomyError> {
        if ancestry.len() != self.nodes.len()
            || ancestry
                .iter()
                .any(|value| !value.is_finite() || *value < 0.0)
        {
            return Err(EconomyError::InvalidTickData);
        }
        let Some(trace) = self.register_eligibility.get_mut(register) else {
            return Err(EconomyError::InvalidRegister { register });
        };
        trace.copy_from_slice(ancestry);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structural::growth::{GrowthConfig, observation_horizon};

    fn test_obs_horizon(config: &EconomyConfig) -> usize {
        observation_horizon(config, &GrowthConfig::default())
    }

    #[test]
    fn centered_reward_is_zero_at_the_null_probability_and_negative_on_mismatch() {
        let config = EconomyConfig {
            juvenile_ticks: 0,
            reserve_cap: ReserveCap::Uncapped,
            reward_mode: RewardMode::CenteredSurprisal,
            ..EconomyConfig::default()
        };
        let mut state =
            EconomyState::new(config, Width::BITS_2, 1, 1, 0, test_obs_horizon(&config)).unwrap();
        let baseline = state.scorers[0].null_model().probability(0);
        let reward = (1.0 - baseline) * -baseline.log2();
        assert!(reward.is_finite());

        let actual = state.observe_predictions(&[0], &[1]).unwrap();
        assert!(actual[0] < 0.0);
    }

    #[test]
    fn economy_reward_and_null_calibrated_mdl_delta_agree_on_random_sequences() {
        use crate::metrics::circuit_code_bits;
        use crate::substrate::{Circuit, SignalSink, SignalSource, WireDelayMode};
        use rand_chacha::ChaCha8Rng;
        use rand_core::{Rng, SeedableRng};

        let width = Width::BITS_2;
        let mut circuit = Circuit::new(width, 1, 1, WireDelayMode::Combinational);
        circuit.connect(SignalSource::BoundaryIn(0), SignalSink::BoundaryOut(0));
        let code_bits = circuit_code_bits(&circuit, 0);

        let mut rng = ChaCha8Rng::seed_from_u64(12345);
        let mask = 3u64;
        let facts: Vec<u64> = (0..256).map(|_| rng.next_u64() & mask).collect();
        let preds: Vec<u64> = (0..256).map(|_| rng.next_u64() & mask).collect();

        let mut economy = EconomyState::new(
            EconomyConfig::default(),
            width,
            1,
            1,
            0,
            test_obs_horizon(&EconomyConfig::default()),
        )
        .unwrap();
        let mut total_flux = 0.0f64;
        for (&p, &f) in preds.iter().zip(&facts) {
            let r = economy.observe_predictions(&[p], &[f]).unwrap();
            total_flux += r[0];
        }

        let delta_l = crate::metrics::null_calibrated_mdl_delta(
            &circuit,
            0.0,
            0,
            &facts,
            &preds,
            NullModelKind::LaplaceFrequency,
        )
        .unwrap();

        let mdl_flux = code_bits - delta_l;
        assert!(
            (total_flux - mdl_flux).abs() < 1e-12,
            "economy flux {total_flux} != MDL flux {mdl_flux}"
        );
    }

    #[test]
    fn mature_energy_conserves_flux_minus_maintenance() {
        let config = EconomyConfig {
            juvenile_ticks: 0,
            reserve_cap: ReserveCap::Uncapped,
            ..EconomyConfig::default()
        };
        let mut economy =
            EconomyState::new(config, Width::BITS_1, 2, 1, 0, test_obs_horizon(&config)).unwrap();
        let before: f64 = economy.nodes().iter().map(|node| node.energy).sum();
        let tick = economy
            .apply_tick(&[0.75], &[vec![1.0, 3.0]], &[0.02, 0.03])
            .unwrap();
        let after: f64 = economy.nodes().iter().map(|node| node.energy).sum();
        assert!(((after - before) - (tick.net_flux - tick.maintenance)).abs() < 1e-12);
    }

    #[test]
    fn juveniles_keep_positive_rewards_but_are_exempt_from_costs_and_losses() {
        let config = EconomyConfig {
            juvenile_ticks: 2,
            reserve_cap: ReserveCap::Uncapped,
            ..EconomyConfig::default()
        };
        let mut economy =
            EconomyState::new(config, Width::BITS_1, 1, 1, 0, test_obs_horizon(&config)).unwrap();
        let tick = economy.apply_tick(&[-1.0], &[vec![1.0]], &[1.0]).unwrap();
        assert_eq!(tick.node_delta, vec![0.0]);
        assert_eq!(economy.nodes()[0].juvenile_ticks, 1);
    }

    #[test]
    fn structure_remapping_preserves_survivor_energy_traces_and_null_model() {
        let config = EconomyConfig {
            juvenile_ticks: 0,
            reserve_cap: ReserveCap::Uncapped,
            ..EconomyConfig::default()
        };
        let mut economy =
            EconomyState::new(config, Width::BITS_2, 2, 1, 1, test_obs_horizon(&config)).unwrap();
        economy.set_register_eligibility(0, &[0.25, 0.75]).unwrap();
        economy
            .observe_predictions(&[1], &[2])
            .expect("prequential model accepts normalized symbols");
        economy
            .apply_tick(&[0.5], &[vec![1.0, 0.0]], &[0.0, 0.0])
            .unwrap();

        let mut remapped = economy
            .remap_structure(&[Some(1), None], 3, &[Some(1)], 2)
            .unwrap();

        assert_eq!(remapped.nodes()[1], economy.nodes()[0]);
        assert_eq!(remapped.nodes()[0].energy, config.initial_energy);
        assert_eq!(remapped.register_eligibility()[1], vec![0.0, 0.25, 0.0]);
        assert_eq!(
            remapped.observe_predictions(&[1], &[3]).unwrap(),
            economy.observe_predictions(&[1], &[3]).unwrap()
        );
    }

    #[test]
    fn record_activity_tracks_nonzero_and_non_one_and_reports_node_credit() {
        let config = EconomyConfig {
            juvenile_ticks: 0,
            reserve_cap: ReserveCap::Uncapped,
            ..EconomyConfig::default()
        };
        let mut economy =
            EconomyState::new(config, Width::BITS_2, 3, 1, 0, test_obs_horizon(&config)).unwrap();
        // Initially all zero weights
        assert_eq!(economy.activity(), vec![NodeActivity::default(); 3]);

        // Emit 0, 1, 2
        economy.record_activity(&[0, 1, 2]).unwrap();
        let act = economy.activity();
        // 0: nonzero_weight = 0.0, non_one_weight = 1.0
        assert_eq!(act[0].nonzero_weight, 0.0);
        assert_eq!(act[0].non_one_weight, 1.0);
        assert!(!act[0].ever_nonzero());
        assert!(act[0].ever_non_one());
        // 1: nonzero_weight = 1.0, non_one_weight = 0.0
        assert_eq!(act[1].nonzero_weight, 1.0);
        assert_eq!(act[1].non_one_weight, 0.0);
        assert!(act[1].ever_nonzero());
        assert!(!act[1].ever_non_one());
        // 2: nonzero_weight = 1.0, non_one_weight = 1.0
        assert_eq!(act[2].nonzero_weight, 1.0);
        assert_eq!(act[2].non_one_weight, 1.0);
        assert!(act[2].ever_nonzero());
        assert!(act[2].ever_non_one());

        // Check node_credit before maintenance
        let tick = economy
            .apply_tick(&[1.0], &[vec![0.5, 0.3, 0.2]], &[0.1, 0.1, 0.1])
            .unwrap();
        assert!((tick.node_credit[0] - 0.5).abs() < 1e-12);
        assert!((tick.node_credit[1] - 0.3).abs() < 1e-12);
        assert!((tick.node_credit[2] - 0.2).abs() < 1e-12);
        assert!((tick.node_delta[0] - 0.4).abs() < 1e-12);
    }

    #[test]
    fn recency_update_and_sparse_corrector_weight_floor() {
        let config = EconomyConfig {
            trace_discount: 0.90,
            juvenile_ticks: 0,
            ..EconomyConfig::default()
        };
        let mut economy =
            EconomyState::new(config, Width::BITS_2, 1, 1, 0, test_obs_horizon(&config)).unwrap();
        let gamma = config.trace_discount;
        assert_eq!(economy.activity()[0].nonzero_weight, 0.0);
        assert_eq!(economy.activity()[0].non_one_weight, 0.0);

        // Emit non-zero (2): nonzero_weight becomes 1.0, non_one_weight becomes 1.0
        economy.record_activity(&[2]).unwrap();
        assert_eq!(economy.activity()[0].nonzero_weight, 1.0);
        assert_eq!(economy.activity()[0].non_one_weight, 1.0);

        // Gap = 3: fires at t = 0, then emits 0 at t = 1 and t = 2, then fires again at t = 3.
        // t = 1: emits 0 (identity for Add)
        economy.record_activity(&[0]).unwrap();
        assert!((economy.activity()[0].nonzero_weight - gamma).abs() < 1e-12);
        // t = 2: emits 0 again
        economy.record_activity(&[0]).unwrap();
        let expected_floor = gamma.powi(3 - 1); // gamma^(gap - 1) = 0.90^2 = 0.81
        assert!((economy.activity()[0].nonzero_weight - expected_floor).abs() < 1e-12);

        // t = 3: fires again
        economy.record_activity(&[2]).unwrap();
        assert_eq!(economy.activity()[0].nonzero_weight, 1.0);
    }

    #[test]
    fn juvenile_escrow_accumulates_unclipped_until_maturity_debt_settlement() {
        let config = EconomyConfig {
            initial_energy: 4.0,
            juvenile_ticks: 128,
            reserve_cap: ReserveCap::Fixed(25.0),
            ..EconomyConfig::default()
        };
        let mut economy =
            EconomyState::new(config, Width::BITS_1, 1, 1, 0, test_obs_horizon(&config)).unwrap();
        assert_eq!(economy.nodes()[0].energy, 4.0);
        assert_eq!(economy.nodes()[0].juvenile_ticks, 128);

        // Earn 0.30 bits/tick during juvenile grace (ticks 0..128)
        // At tick t=0 (remaining juvenile ticks 128 -> decrements to 127):
        // energy becomes 4.0 + 0.30 = 4.30.
        // Over 128 juvenile ticks (ticks 0..127), energy accumulates 128 * 0.30 = 38.4 bits.
        // Total unclipped energy at tick 127 = 4.0 + 38.4 = 42.4 bits.
        for _ in 0..128 {
            let tick = economy.apply_tick(&[0.30], &[vec![1.0]], &[0.0]).unwrap();
            assert_eq!(tick.starved_nodes.len(), 0);
        }
        assert_eq!(economy.nodes()[0].juvenile_ticks, 0);
        assert!((economy.nodes()[0].energy - 42.4).abs() < 1e-12);

        // On tick 128 (now mature, juvenile_ticks == 0):
        // pay one-time maturity debt of 28.0 bits.
        // Net unclipped energy: 42.4 + 0.30 - 28.0 = 14.7.
        // Clamped by max_energy_reserve (25.0): 14.7 <= 25.0. Survives!
        let tick = economy.apply_tick(&[0.30], &[vec![1.0]], &[28.0]).unwrap();
        assert_eq!(tick.starved_nodes.len(), 0);
        assert!((economy.nodes()[0].energy - 14.7).abs() < 1e-12);

        // Subsequent mature ticks: earns 0.30 bits, energy accumulates toward cap
        // If energy would exceed 25.0, it is clamped to 25.0.
        for _ in 0..100 {
            economy.apply_tick(&[0.30], &[vec![1.0]], &[0.0]).unwrap();
        }
        assert!((economy.nodes()[0].energy - 25.0).abs() < 1e-12);
    }

    #[test]
    fn horizon_derived_reserve_cap_grounds_ceiling_from_mismatch_loss() {
        let config = EconomyConfig {
            initial_energy: 4.0,
            juvenile_ticks: 0,
            reserve_cap: ReserveCap::HorizonDerived,
            ..EconomyConfig::default()
        };
        let horizon = test_obs_horizon(&config);
        let mut economy = EconomyState::new(config, Width::BITS_1, 1, 1, 0, horizon).unwrap();
        // Train scorer with 10 correct matches on 1-bit port
        for _ in 0..10 {
            let _ = economy.port_scorer_mut(0).observe(1, 1);
        }
        // Scorer KT estimator for k=1 (cardinality 2):
        // q_hat = (10 + 0.5) / (10 + 1.0) = 10.5 / 11.0 ≈ 0.9545
        let q = economy.scorers()[0].q_hat();
        let p_null = 0.5f64;
        let drain_loss = ((1.0 - p_null) / (1.0 - q)).log2();
        let w_obs = horizon as f64;
        let expected_cap = (w_obs * drain_loss).max(config.initial_energy);

        // Apply a huge reward 1000.0 bits
        let tick = economy.apply_tick(&[1000.0], &[vec![1.0]], &[0.0]).unwrap();
        assert_eq!(tick.clipped_nodes, 1);
        assert!((economy.nodes()[0].energy - expected_cap).abs() < 1e-6);

        let custom_growth = GrowthConfig {
            diagnostic_window: 32,
            ..GrowthConfig::default()
        };
        assert_eq!(observation_horizon(&config, &custom_growth), 64);
    }
}
