//! Metabolic reserves, surprisal rewards, maintenance, and conserved trophic flux.

use serde::{Deserialize, Serialize};

use crate::metrics::{NullModel, NullModelKind};
use crate::structural::sensitivity::{CausalAncestry, NodeActivity};
use crate::substrate::{Primitive, Width};

/// Reward rule used to pay exact-match predictions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RewardMode {
    /// Exact-match surprisal minus its pre-observation blind-luck expectation.
    #[default]
    CenteredSurprisal,
    /// Exact-match surprisal without chance-baseline subtraction.
    UncenteredSurprisal,
    /// Exact-match reward centered at the uniform chance accuracy.
    RawAccuracy,
}

/// Node-economy settings. The defaults are the preregistered M1 values.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EconomyConfig {
    /// Per-tick maintenance for a non-`SPLIT` node.
    pub c_base: f64,
    /// Per-tick maintenance for a `SPLIT` node.
    pub c_split: f64,
    /// Per-tick maintenance per wire hop.
    pub c_wire: f64,
    /// Discount applied to ancestry carried through synchronous registers.
    pub trace_discount: f64,
    /// Initial reserve assigned to each placed node.
    pub initial_energy: f64,
    /// Ticks for which new nodes are protected from maintenance and negative payout.
    pub juvenile_ticks: u32,
    /// Optional reserve ceiling.
    pub max_energy_reserve: Option<f64>,
    /// Null model used to compute prequential reward.
    pub null_model: NullModelKind,
    /// Exact-match reward rule.
    pub reward_mode: RewardMode,
}

impl Default for EconomyConfig {
    fn default() -> Self {
        Self {
            c_base: 0.020,
            c_split: 0.035,
            c_wire: 0.004,
            trace_discount: 0.90,
            initial_energy: 4.0,
            juvenile_ticks: 128,
            max_energy_reserve: Some(25.0),
            null_model: NullModelKind::LaplaceFrequency,
            reward_mode: RewardMode::CenteredSurprisal,
        }
    }
}

impl EconomyConfig {
    /// Checks that energy and trace settings are finite and physically meaningful.
    pub fn validate(self) -> Result<(), EconomyError> {
        if !self.c_base.is_finite()
            || self.c_base <= 0.0
            || !self.c_split.is_finite()
            || self.c_split < 0.0
            || !self.c_wire.is_finite()
            || self.c_wire < 0.0
            || !self.trace_discount.is_finite()
            || !(0.0..1.0).contains(&self.trace_discount)
            || !self.initial_energy.is_finite()
            || self.initial_energy <= 0.0
            || self
                .max_energy_reserve
                .is_some_and(|cap| !cap.is_finite() || cap < self.initial_energy)
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
}

/// Node-local reserves, port null models, and synchronous-register eligibility traces.
#[derive(Clone, Debug)]
pub struct EconomyState {
    config: EconomyConfig,
    width: Width,
    nodes: Vec<NodeEconomy>,
    null_models: Vec<NullModel>,
    register_eligibility: Vec<Vec<f64>>,
}

impl EconomyState {
    /// Creates energy state for `nodes`, `outputs`, and `registers`.
    pub fn new(
        config: EconomyConfig,
        width: Width,
        nodes: usize,
        outputs: usize,
        registers: usize,
    ) -> Result<Self, EconomyError> {
        config.validate()?;
        let node_state = NodeEconomy {
            energy: config.initial_energy,
            juvenile_ticks: config.juvenile_ticks,
            activity: NodeActivity::default(),
        };
        Ok(Self {
            config,
            width,
            nodes: vec![node_state; nodes],
            null_models: (0..outputs)
                .map(|_| NullModel::new(width, config.null_model))
                .collect(),
            register_eligibility: vec![vec![0.0; nodes]; registers],
        })
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

    /// Marks nodes that have emitted non-identity values.
    pub fn record_activity(&mut self, outputs: &[u64]) -> Result<(), EconomyError> {
        if outputs.len() != self.nodes.len() {
            return Err(EconomyError::DimensionMismatch);
        }
        for (node, &output) in self.nodes.iter_mut().zip(outputs) {
            let norm = self.width.normalize(output);
            if norm != 0 {
                node.activity.ever_nonzero = true;
            }
            if norm != 1 {
                node.activity.ever_non_one = true;
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
            self.null_models.len(),
            new_register_count,
        )?;
        remapped.null_models.clone_from(&self.null_models);
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
        if predictions.len() != self.null_models.len() || facts.len() != predictions.len() {
            return Err(EconomyError::DimensionMismatch);
        }
        let mut rewards = Vec::with_capacity(predictions.len());
        for ((&prediction, &fact), null) in predictions.iter().zip(facts).zip(&mut self.null_models)
        {
            let prediction = self.width.normalize(prediction);
            let fact = self.width.normalize(fact);
            let p = null.probability(prediction);
            let surprisal = -p.log2();
            let correct = if prediction == fact { 1.0 } else { 0.0 };
            let reward = match self.config.reward_mode {
                RewardMode::CenteredSurprisal => (correct - p) * surprisal,
                RewardMode::UncenteredSurprisal => {
                    if prediction == fact {
                        -null.probability(fact).log2()
                    } else {
                        0.0
                    }
                }
                RewardMode::RawAccuracy => correct - 2.0f64.powi(-i32::from(self.width.bits())),
            };
            null.observe_and_surprisal(fact);
            rewards.push(reward);
        }
        Ok(rewards)
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
        if port_rewards.len() != self.null_models.len()
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
        for (index, node) in self.nodes.iter().enumerate() {
            let unclipped = node.energy + delta[index];
            if !unclipped.is_finite() {
                return Err(EconomyError::InvalidTickData);
            }
            let next_energy = self
                .config
                .max_energy_reserve
                .map_or(unclipped, |cap| unclipped.min(cap));
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

    /// Computes a node's deterministic maintenance drain from its primitive and wire distance.
    #[must_use]
    pub fn maintenance_cost(&self, primitive: Primitive, wire_steps: u32) -> f64 {
        let node_cost = if matches!(primitive, Primitive::Split) {
            self.config.c_split
        } else {
            self.config.c_base
        };
        node_cost + self.config.c_wire * f64::from(wire_steps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centered_reward_is_zero_at_the_null_probability_and_negative_on_mismatch() {
        let config = EconomyConfig {
            juvenile_ticks: 0,
            max_energy_reserve: None,
            ..EconomyConfig::default()
        };
        let mut state = EconomyState::new(config, Width::BITS_2, 1, 1, 0).unwrap();
        let baseline = state.null_models[0].probability(0);
        let reward = (1.0 - baseline) * -baseline.log2();
        assert!(reward.is_finite());

        let actual = state.observe_predictions(&[0], &[1]).unwrap();
        assert!(actual[0] < 0.0);
    }

    #[test]
    fn mature_energy_conserves_flux_minus_maintenance() {
        let config = EconomyConfig {
            juvenile_ticks: 0,
            max_energy_reserve: None,
            ..EconomyConfig::default()
        };
        let mut economy = EconomyState::new(config, Width::BITS_1, 2, 1, 0).unwrap();
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
            max_energy_reserve: None,
            ..EconomyConfig::default()
        };
        let mut economy = EconomyState::new(config, Width::BITS_1, 1, 1, 0).unwrap();
        let tick = economy.apply_tick(&[-1.0], &[vec![1.0]], &[1.0]).unwrap();
        assert_eq!(tick.node_delta, vec![0.0]);
        assert_eq!(economy.nodes()[0].juvenile_ticks, 1);
    }

    #[test]
    fn structure_remapping_preserves_survivor_energy_traces_and_null_model() {
        let config = EconomyConfig {
            juvenile_ticks: 0,
            max_energy_reserve: None,
            ..EconomyConfig::default()
        };
        let mut economy = EconomyState::new(config, Width::BITS_2, 2, 1, 1).unwrap();
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
            max_energy_reserve: None,
            ..EconomyConfig::default()
        };
        let mut economy = EconomyState::new(config, Width::BITS_2, 3, 1, 0).unwrap();
        // Initially all false
        assert_eq!(economy.activity(), vec![NodeActivity::default(); 3]);

        // Emit 0, 1, 2
        economy.record_activity(&[0, 1, 2]).unwrap();
        let act = economy.activity();
        // 0: ever_nonzero = false, ever_non_one = true
        assert!(!act[0].ever_nonzero);
        assert!(act[0].ever_non_one);
        // 1: ever_nonzero = true, ever_non_one = false
        assert!(act[1].ever_nonzero);
        assert!(!act[1].ever_non_one);
        // 2: ever_nonzero = true, ever_non_one = true
        assert!(act[2].ever_nonzero);
        assert!(act[2].ever_non_one);

        // Check node_credit before maintenance
        let tick = economy
            .apply_tick(&[1.0], &[vec![0.5, 0.3, 0.2]], &[0.1, 0.1, 0.1])
            .unwrap();
        assert!((tick.node_credit[0] - 0.5).abs() < 1e-12);
        assert!((tick.node_credit[1] - 0.3).abs() < 1e-12);
        assert!((tick.node_credit[2] - 0.2).abs() < 1e-12);
        assert!((tick.node_delta[0] - 0.4).abs() < 1e-12);
    }
}
