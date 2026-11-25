//! Metabolic reserves, surprisal rewards, maintenance, and conserved trophic flux.

use serde::{Deserialize, Serialize};

pub use crate::metrics::mdl::channel_independence_weight;
use crate::metrics::{NullModelKind, PointForecastScorer};
use crate::structural::sensitivity::{
    CausalAncestry, ComparatorCausalAncestry, ComparatorTarget, NodeActivity,
};
use crate::substrate::{NodeId, Width};

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
    /// Minimum statistical maturation floor $t_{\text{juv,min}}$ (`128 = 2 * W_diag`; `0` in
    /// conservation tests) protecting new nodes from maintenance and negative payout; actual
    /// Stage-2 / Stage-3 grace extends dynamically up to $2 W_{\text{obs}}$ via per-node debt
    /// amortization (`M2B-B1`).
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
    /// Smoothed effective mismatch drain rate for `ReserveCap::HorizonDerived` reserve stability.
    pub smoothed_drain_rate: f64,
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
    /// A non-zero comparator reward has no causal ancestry to receive it.
    MissingComparatorAncestry { comparator: usize },
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
            Self::MissingComparatorAncestry { comparator } => {
                write!(
                    f,
                    "reward on comparator {comparator} has no causal ancestry"
                )
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

/// Execution phase controlling boundary supervision and comparator scoring (Charter DR #40).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExecutionPhase {
    /// Supervised wake phase: boundary ports and comparators score against external ground truth.
    #[default]
    Wake,
    /// Unsupervised sleep/loopback phase: boundary rewards are zero (`0.0`) and comparators score
    /// internal agreement `alt == primary` against each target port's frozen wake null model.
    Sleep,
}

/// Registered internal comparator site `EQZ(SUB(primary, alt))` attached to boundary port `p`.
#[derive(Clone, Debug)]
pub struct ComparatorSite {
    /// Difference node `SUB(primary, alt)` (`in(0)` = primary, `in(1)` = alt).
    pub sub_node: NodeId,
    /// Equality detector `EQZ(sub_node)`.
    pub eqz_node: NodeId,
    /// Target boundary output port $p$ supplying the calibrated wake null model $P_{\text{null}}^{(p)}$.
    pub target_port: u16,
    /// Source-channel bitmask for the primary derivation (`sub_node.in(0)`).
    pub primary_channel_mask: u64,
    /// Source-channel bitmask for the alternative derivation (`sub_node.in(1)`).
    pub alt_channel_mask: u64,
    /// Wake prequential scorer tracking port $p$'s wake null model $P_{\text{null}}^{(p)}$.
    pub scorer: PointForecastScorer,
    /// Sleep-segment scorer operating against a frozen copy of the wake null model $P_{\text{null}}^{(p)}$.
    sleep_scorer: Option<PointForecastScorer>,
}

impl ComparatorSite {
    /// Substrate node pair (`sub_node`, `eqz_node`) used for causal sensitivity and ancestry.
    #[must_use]
    pub const fn target(&self) -> ComparatorTarget {
        ComparatorTarget {
            sub_node: self.sub_node,
            eqz_node: self.eqz_node,
        }
    }

    /// Source-channel independence weight $\iota(a, b) = \frac{1}{|C_a \cap C_b| + 1}$
    /// applied to positive internal agreement in sleep.
    #[must_use]
    pub fn sleep_agreement_weight(&self) -> f64 {
        channel_independence_weight(self.primary_channel_mask, self.alt_channel_mask)
    }

    /// Active sleep-segment scorer when inside [`ExecutionPhase::Sleep`].
    #[must_use]
    pub const fn sleep_scorer(&self) -> Option<&PointForecastScorer> {
        self.sleep_scorer.as_ref()
    }

    /// Active scorer for the current phase (Sleep segment scorer if present, else Wake scorer).
    #[must_use]
    pub fn active_scorer(&self) -> &PointForecastScorer {
        self.sleep_scorer.as_ref().unwrap_or(&self.scorer)
    }
}

/// Output of scoring all registered comparators for a single tick.
#[derive(Clone, Debug, PartialEq)]
pub struct ComparatorObservation {
    /// Net prequential reward for each comparator in bits (zero when empty).
    pub rewards: Vec<f64>,
    /// Count of comparators reporting agreement (`eqz_node == 1`) on this tick.
    pub agreements: u64,
    /// Count of comparators reporting disagreement (`eqz_node == 0`) on this tick.
    pub disagreements: u64,
}

/// Result of applying one tick of metabolic flux and maintenance.
#[derive(Clone, Debug, PartialEq)]
pub struct EconomyTick {
    /// Total net reward across output ports and registered comparators.
    pub net_flux: f64,
    /// Net reward contributed by registered comparators on this tick (`0.0` if none).
    pub comparator_flux: f64,
    /// Number of registered comparators reporting agreement (`eqz_node == 1`) on this tick.
    pub comparator_agreements: u64,
    /// Number of registered comparators reporting disagreement (`eqz_node == 0`) on this tick.
    pub comparator_disagreements: u64,
    /// Total maintenance actually deducted (juveniles are exempt).
    pub maintenance: f64,
    /// Per-node payout from port and comparator rewards before maintenance and clipping.
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
    phase: ExecutionPhase,
    comparators: Vec<ComparatorSite>,
    comparator_agreement_ticks: u64,
    comparator_disagreement_ticks: u64,
    cumulative_comparator_flux: f64,
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
            smoothed_drain_rate: 0.0,
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
            phase: ExecutionPhase::Wake,
            comparators: Vec::new(),
            comparator_agreement_ticks: 0,
            comparator_disagreement_ticks: 0,
            cumulative_comparator_flux: 0.0,
        })
    }

    /// Current execution phase (`Wake` or `Sleep`).
    #[must_use]
    pub const fn phase(&self) -> ExecutionPhase {
        self.phase
    }

    /// Transitions execution phase, initializing or clearing per-segment Sleep scorers against
    /// frozen wake null models without mutating Wake state.
    pub fn set_phase(&mut self, phase: ExecutionPhase) {
        if self.phase == phase {
            return;
        }
        match phase {
            ExecutionPhase::Sleep => {
                for comp in &mut self.comparators {
                    let wake_null = self
                        .scorers
                        .get(usize::from(comp.target_port))
                        .map_or_else(|| comp.scorer.null_model(), PointForecastScorer::null_model);
                    let mut sleep_scorer = PointForecastScorer::new(self.width, wake_null.kind());
                    *sleep_scorer.null_model_mut() = wake_null.clone();
                    comp.sleep_scorer = Some(sleep_scorer);
                }
            }
            ExecutionPhase::Wake => {
                for comp in &mut self.comparators {
                    comp.sleep_scorer = None;
                }
            }
        }
        self.phase = phase;
    }

    /// Registered comparator sites.
    #[must_use]
    pub fn comparators(&self) -> &[ComparatorSite] {
        &self.comparators
    }

    /// Mutable reference to registered comparator sites.
    pub fn comparators_mut(&mut self) -> &mut Vec<ComparatorSite> {
        &mut self.comparators
    }

    /// Cumulative comparator agreement ticks (`eqz_node == 1`).
    #[must_use]
    pub const fn comparator_agreement_ticks(&self) -> u64 {
        self.comparator_agreement_ticks
    }

    /// Cumulative comparator disagreement ticks (`eqz_node == 0`).
    #[must_use]
    pub const fn comparator_disagreement_ticks(&self) -> u64 {
        self.comparator_disagreement_ticks
    }

    /// Cumulative net flux (in bits) credited through registered comparators.
    #[must_use]
    pub const fn cumulative_comparator_flux(&self) -> f64 {
        self.cumulative_comparator_flux
    }

    /// Registers a comparator `EQZ(SUB(primary, alt))` targeting output port `target_port`,
    /// initializing its scorer to port `target_port`'s current wake null model $P_{\text{null}}^{(p)}$.
    pub fn register_comparator(
        &mut self,
        sub_node: NodeId,
        eqz_node: NodeId,
        target_port: u16,
        primary_channel_mask: u64,
        alt_channel_mask: u64,
    ) -> Result<usize, EconomyError> {
        if sub_node.index() >= self.nodes.len()
            || eqz_node.index() >= self.nodes.len()
            || usize::from(target_port) >= self.scorers.len()
        {
            return Err(EconomyError::DimensionMismatch);
        }
        if sub_node == eqz_node
            || self
                .comparators
                .iter()
                .any(|c| c.sub_node == sub_node || c.eqz_node == eqz_node)
        {
            return Err(EconomyError::InvalidStructureMapping);
        }
        let wake_null = self.scorers[usize::from(target_port)].null_model().clone();
        let mut scorer = PointForecastScorer::new(self.width, wake_null.kind());
        *scorer.null_model_mut() = wake_null;
        let sleep_scorer = if self.phase == ExecutionPhase::Sleep {
            Some(scorer.clone())
        } else {
            None
        };
        self.comparators.push(ComparatorSite {
            sub_node,
            eqz_node,
            target_port,
            primary_channel_mask,
            alt_channel_mask,
            scorer,
            sleep_scorer,
        });
        Ok(self.comparators.len() - 1)
    }

    /// Removes comparator `index`, returning it if it existed.
    pub fn remove_comparator(&mut self, index: usize) -> Option<ComparatorSite> {
        if index < self.comparators.len() {
            Some(self.comparators.remove(index))
        } else {
            None
        }
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
        remapped.phase = self.phase;
        remapped.comparator_agreement_ticks = self.comparator_agreement_ticks;
        remapped.comparator_disagreement_ticks = self.comparator_disagreement_ticks;
        remapped.cumulative_comparator_flux = self.cumulative_comparator_flux;
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
        for comp in &self.comparators {
            let Some(mapped_sub) = old_to_new_nodes.get(comp.sub_node.index()).copied() else {
                return Err(EconomyError::InvalidStructureMapping);
            };
            let Some(mapped_eqz) = old_to_new_nodes.get(comp.eqz_node.index()).copied() else {
                return Err(EconomyError::InvalidStructureMapping);
            };
            if let (Some(new_sub), Some(new_eqz)) = (mapped_sub, mapped_eqz) {
                let Ok(sub_u32) = u32::try_from(new_sub) else {
                    return Err(EconomyError::InvalidStructureMapping);
                };
                let Ok(eqz_u32) = u32::try_from(new_eqz) else {
                    return Err(EconomyError::InvalidStructureMapping);
                };
                let mut cloned = comp.clone();
                cloned.sub_node = NodeId(sub_u32);
                cloned.eqz_node = NodeId(eqz_u32);
                remapped.comparators.push(cloned);
            }
        }
        Ok(remapped)
    }

    /// Prequential reward on each output port, updating its null model after scoring the fact.
    ///
    /// In [`ExecutionPhase::Sleep`], returns zero reward per port without mutating wake null models.
    pub fn observe_predictions(
        &mut self,
        predictions: &[u64],
        facts: &[u64],
    ) -> Result<Vec<f64>, EconomyError> {
        if predictions.len() != self.scorers.len() || facts.len() != predictions.len() {
            return Err(EconomyError::DimensionMismatch);
        }
        if self.phase == ExecutionPhase::Sleep {
            return Ok(vec![0.0; predictions.len()]);
        }
        let mut rewards = Vec::with_capacity(predictions.len());
        for (i, (&prediction, &fact)) in predictions.iter().zip(facts).enumerate() {
            rewards.push(self.score_port_single(i, prediction, fact)?);
        }
        Ok(rewards)
    }

    /// Computes reward for a single output port against its observation.
    ///
    /// Returns `0.0` without mutating the port's wake null model in [`ExecutionPhase::Sleep`].
    pub fn score_port_single(
        &mut self,
        port: usize,
        prediction: u64,
        fact: u64,
    ) -> Result<f64, EconomyError> {
        if port >= self.scorers.len() {
            return Err(EconomyError::DimensionMismatch);
        }
        if self.phase == ExecutionPhase::Sleep {
            return Ok(0.0);
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
    ///
    /// No-op in [`ExecutionPhase::Sleep`] so wake null models remain frozen.
    pub fn observe_unconnected_port(
        &mut self,
        port: usize,
        raw_fact: u64,
    ) -> Result<(), EconomyError> {
        if port >= self.scorers.len() {
            return Err(EconomyError::DimensionMismatch);
        }
        if self.phase == ExecutionPhase::Sleep {
            return Ok(());
        }
        self.scorers[port]
            .null_model_mut()
            .observe_and_surprisal(raw_fact);
        Ok(())
    }

    /// Scores all registered comparators for one tick (Wake against ground truth `facts[target_port]`,
    /// Sleep against `primary` under each port's frozen wake null model with agreement weight $\iota$).
    pub fn observe_comparators(
        &mut self,
        primary_values: &[u64],
        alt_values: &[u64],
        eqz_values: &[u64],
        facts: Option<&[u64]>,
    ) -> Result<ComparatorObservation, EconomyError> {
        self.observe_comparators_with_suspension(
            primary_values,
            alt_values,
            eqz_values,
            facts,
            None,
        )
    }

    /// Scores all non-suspended registered comparators for one tick (`R4`, `I7`).
    ///
    /// Comparators marked `true` in `suspended` emit exact `0.0` reward, record `0` agreements
    /// and `0` disagreements, and leave their sleep scorers untouched.
    pub fn observe_comparators_with_suspension(
        &mut self,
        primary_values: &[u64],
        alt_values: &[u64],
        eqz_values: &[u64],
        facts: Option<&[u64]>,
        suspended: Option<&[bool]>,
    ) -> Result<ComparatorObservation, EconomyError> {
        let count = self.comparators.len();
        if primary_values.len() != count
            || alt_values.len() != count
            || eqz_values.len() != count
            || suspended.is_some_and(|s| s.len() != count)
        {
            return Err(EconomyError::DimensionMismatch);
        }
        if self.phase == ExecutionPhase::Wake {
            let Some(facts) = facts else {
                return Err(EconomyError::DimensionMismatch);
            };
            if facts.len() != self.scorers.len() {
                return Err(EconomyError::DimensionMismatch);
            }
        }

        let mut rewards = Vec::with_capacity(count);
        let mut agreements = 0u64;
        let mut disagreements = 0u64;
        let mode = self.config.reward_mode;
        let width = self.width;

        for m in 0..count {
            if suspended.is_some_and(|s| s[m]) {
                rewards.push(0.0);
                continue;
            }
            let eqz_active = width.normalize(eqz_values[m]) != 0;
            if eqz_active {
                agreements += 1;
            } else {
                disagreements += 1;
            }
            let reward = match self.phase {
                ExecutionPhase::Wake => {
                    let target_port = usize::from(self.comparators[m].target_port);
                    let fact = facts.expect("Wake phase checked non-empty facts")[target_port];
                    score_with_reward_mode(
                        &mut self.comparators[m].scorer,
                        mode,
                        width,
                        alt_values[m],
                        fact,
                    )
                }
                ExecutionPhase::Sleep => {
                    let comp = &mut self.comparators[m];
                    let iota = comp.sleep_agreement_weight();
                    let sleep_scorer = comp.sleep_scorer.get_or_insert_with(|| {
                        let frozen = comp.scorer.null_model();
                        let mut fresh = PointForecastScorer::new(width, frozen.kind());
                        *fresh.null_model_mut() = frozen.clone();
                        fresh
                    });
                    let frozen_null = sleep_scorer.null_model().clone();
                    let raw_r = score_with_reward_mode(
                        sleep_scorer,
                        mode,
                        width,
                        alt_values[m],
                        primary_values[m],
                    );
                    *sleep_scorer.null_model_mut() = frozen_null;
                    if width.normalize(alt_values[m]) == width.normalize(primary_values[m]) {
                        raw_r * iota
                    } else {
                        raw_r
                    }
                }
            };
            rewards.push(reward);
        }

        Ok(ComparatorObservation {
            rewards,
            agreements,
            disagreements,
        })
    }

    /// Releases escrowed non-negative `S-B` comparator agreement payouts through the conserved
    /// reserve ceiling path without double-advancing `juvenile_ticks` or `smoothed_drain_rate` (`R5(b)`, `I4`).
    pub fn apply_escrowed_comparator_flux(
        &mut self,
        node_credits: &[f64],
    ) -> Result<EconomyTick, EconomyError> {
        if node_credits.len() != self.nodes.len() {
            return Err(EconomyError::DimensionMismatch);
        }
        if node_credits.iter().any(|v| !v.is_finite() || *v < 0.0)
            || !node_credits.iter().sum::<f64>().is_finite()
        {
            return Err(EconomyError::InvalidTickData);
        }

        let total_flux: f64 = node_credits.iter().sum();
        if total_flux == 0.0 {
            return Ok(EconomyTick {
                net_flux: 0.0,
                comparator_flux: 0.0,
                comparator_agreements: 0,
                comparator_disagreements: 0,
                maintenance: 0.0,
                node_credit: vec![0.0; self.nodes.len()],
                node_delta: vec![0.0; self.nodes.len()],
                starved_nodes: Vec::new(),
                clipped_nodes: 0,
                clipped_bits: 0.0,
                mismatch_floored_nodes: 0,
            });
        }

        let w_obs = self.observation_horizon as f64;
        let mut node_delta = Vec::with_capacity(self.nodes.len());
        let mut clipped_nodes = 0usize;
        let mut clipped_bits = 0.0f64;

        for (node, &credit) in self.nodes.iter_mut().zip(node_credits) {
            let previous_energy = node.energy;
            let unclipped = previous_energy + credit;
            if !unclipped.is_finite() {
                return Err(EconomyError::InvalidTickData);
            }
            let maybe_cap = if node.juvenile_ticks > 0 {
                None
            } else {
                match self.config.reserve_cap {
                    ReserveCap::Uncapped => None,
                    ReserveCap::Fixed(cap) => Some(cap),
                    ReserveCap::HorizonDerived => {
                        Some((w_obs * node.smoothed_drain_rate).max(self.config.initial_energy))
                    }
                }
            };
            let (next_energy, clipped) = match maybe_cap {
                Some(cap) if unclipped > cap => (cap, unclipped - cap),
                _ => (unclipped, 0.0),
            };
            if clipped > 0.0 {
                clipped_nodes += 1;
                clipped_bits += clipped;
            }
            node.energy = next_energy;
            node_delta.push(next_energy - previous_energy);
        }

        self.cumulative_comparator_flux += total_flux;

        Ok(EconomyTick {
            net_flux: total_flux,
            comparator_flux: total_flux,
            comparator_agreements: 0,
            comparator_disagreements: 0,
            maintenance: 0.0,
            node_credit: node_credits.to_vec(),
            node_delta,
            starved_nodes: Vec::new(),
            clipped_nodes,
            clipped_bits,
            mismatch_floored_nodes: 0,
        })
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
        self.apply_tick_core(
            port_rewards,
            port_ancestry,
            &[],
            &[],
            0,
            0,
            maintenance_costs,
        )
    }

    /// Applies conserved port + comparator rewards and per-node maintenance for one tick.
    #[expect(
        clippy::too_many_arguments,
        reason = "Preserves exact conservation across port and comparator flux alongside telemetry counts"
    )]
    pub fn apply_tick_with_comparators(
        &mut self,
        port_rewards: &[f64],
        port_ancestry: &[Vec<f64>],
        comparator_rewards: &[f64],
        comparator_ancestry: &[Vec<f64>],
        comparator_agreements: u64,
        comparator_disagreements: u64,
        maintenance_costs: &[f64],
    ) -> Result<EconomyTick, EconomyError> {
        self.apply_tick_core(
            port_rewards,
            port_ancestry,
            comparator_rewards,
            comparator_ancestry,
            comparator_agreements,
            comparator_disagreements,
            maintenance_costs,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Shared core guarantees bit-identical M2a path when comparators are empty"
    )]
    fn apply_tick_core(
        &mut self,
        port_rewards: &[f64],
        port_ancestry: &[Vec<f64>],
        comparator_rewards: &[f64],
        comparator_ancestry: &[Vec<f64>],
        comparator_agreements: u64,
        comparator_disagreements: u64,
        maintenance_costs: &[f64],
    ) -> Result<EconomyTick, EconomyError> {
        if port_rewards.len() != self.scorers.len()
            || port_ancestry.len() != port_rewards.len()
            || comparator_rewards.len() != self.comparators.len()
            || comparator_ancestry.len() != comparator_rewards.len()
            || maintenance_costs.len() != self.nodes.len()
        {
            return Err(EconomyError::DimensionMismatch);
        }
        if port_rewards.iter().any(|v| !v.is_finite())
            || comparator_rewards.iter().any(|v| !v.is_finite())
            || maintenance_costs.iter().any(|v| !v.is_finite() || *v < 0.0)
            || !port_rewards.iter().sum::<f64>().is_finite()
            || !comparator_rewards.iter().sum::<f64>().is_finite()
            || !maintenance_costs.iter().sum::<f64>().is_finite()
            || port_ancestry
                .iter()
                .chain(comparator_ancestry)
                .any(|ancestry| {
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
        let mut comparator_flux = 0.0;
        for (comp_idx, (&reward, ancestry)) in comparator_rewards
            .iter()
            .zip(comparator_ancestry)
            .enumerate()
        {
            comparator_flux += reward;
            let total_ancestry: f64 = ancestry.iter().sum();
            if total_ancestry == 0.0 {
                if reward != 0.0 {
                    return Err(EconomyError::MissingComparatorAncestry {
                        comparator: comp_idx,
                    });
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
        let mut next_smoothed_drains = Vec::with_capacity(self.nodes.len());
        let mut clipped_nodes = 0usize;
        let mut clipped_bits = 0.0f64;
        let mut mismatch_floored_nodes = 0usize;
        for (index, node) in self.nodes.iter().enumerate() {
            let unclipped = node.energy + delta[index];
            if !unclipped.is_finite() {
                return Err(EconomyError::InvalidTickData);
            }
            let mut node_floored = false;
            let (next_energy, clipped, next_smoothed_drain) = if node.juvenile_ticks > 0 {
                (unclipped, 0.0, node.smoothed_drain_rate)
            } else {
                let (maybe_cap, next_smoothed_drain) = match self.config.reserve_cap {
                    ReserveCap::Uncapped => (None, node.smoothed_drain_rate),
                    ReserveCap::Fixed(cap) => (Some(cap), node.smoothed_drain_rate),
                    ReserveCap::HorizonDerived => {
                        let w_obs = self.observation_horizon as f64;
                        let mut drain_rate = 0.0;
                        for (port, ancestry) in port_ancestry.iter().enumerate() {
                            let total_ancestry: f64 = ancestry.iter().sum();
                            if total_ancestry > 0.0 {
                                let share = ancestry[index] / total_ancestry;
                                if share > 0.0 && port < self.scorers.len() {
                                    drain_rate += share
                                        * reserve_drain_loss(
                                            &self.scorers[port],
                                            self.width,
                                            &mut node_floored,
                                        );
                                }
                            }
                        }
                        for (comp_idx, ancestry) in comparator_ancestry.iter().enumerate() {
                            let total_ancestry: f64 = ancestry.iter().sum();
                            if total_ancestry > 0.0 {
                                let share = ancestry[index] / total_ancestry;
                                if share > 0.0 && comp_idx < self.comparators.len() {
                                    drain_rate += share
                                        * reserve_drain_loss(
                                            self.comparators[comp_idx].active_scorer(),
                                            self.width,
                                            &mut node_floored,
                                        );
                                }
                            }
                        }
                        let effective_drain =
                            drain_rate.max(self.config.trace_discount * node.smoothed_drain_rate);
                        (
                            Some((w_obs * effective_drain).max(self.config.initial_energy)),
                            effective_drain,
                        )
                    }
                };
                if let Some(cap) = maybe_cap {
                    if unclipped > cap {
                        (cap, unclipped - cap, next_smoothed_drain)
                    } else {
                        (unclipped, 0.0, next_smoothed_drain)
                    }
                } else {
                    (unclipped, 0.0, next_smoothed_drain)
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
            next_smoothed_drains.push(next_smoothed_drain);
        }
        for (index, ((node, next_energy), next_smoothed_drain)) in self
            .nodes
            .iter_mut()
            .zip(next_energies)
            .zip(next_smoothed_drains)
            .enumerate()
        {
            let previous_energy = node.energy;
            node.energy = next_energy;
            node.smoothed_drain_rate = next_smoothed_drain;
            if node.juvenile_ticks > 0 {
                node.juvenile_ticks -= 1;
            } else if node.energy <= 0.0 {
                starved_nodes.push(index);
            }
            delta[index] = node.energy - previous_energy;
        }

        self.comparator_agreement_ticks = self
            .comparator_agreement_ticks
            .saturating_add(comparator_agreements);
        self.comparator_disagreement_ticks = self
            .comparator_disagreement_ticks
            .saturating_add(comparator_disagreements);
        self.cumulative_comparator_flux += comparator_flux;

        let net_flux = if comparator_rewards.is_empty() {
            port_rewards.iter().sum()
        } else {
            port_rewards.iter().sum::<f64>() + comparator_flux
        };

        Ok(EconomyTick {
            net_flux,
            comparator_flux,
            comparator_agreements,
            comparator_disagreements,
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

    /// Applies a sensitivity-assembled tick with comparator shares and latches register eligibility.
    pub fn apply_causal_tick_with_comparators(
        &mut self,
        port_rewards: &[f64],
        comparator_rewards: &[f64],
        comparator_agreements: u64,
        comparator_disagreements: u64,
        ancestry: &ComparatorCausalAncestry,
        maintenance_costs: &[f64],
    ) -> Result<EconomyTick, EconomyError> {
        if ancestry.base.next_register_eligibility.len() != self.register_eligibility.len()
            || ancestry.base.next_register_eligibility.iter().any(|trace| {
                trace.len() != self.nodes.len()
                    || trace.iter().any(|value| !value.is_finite() || *value < 0.0)
            })
        {
            return Err(EconomyError::DimensionMismatch);
        }
        let tick = self.apply_tick_with_comparators(
            port_rewards,
            &ancestry.base.port_shares,
            comparator_rewards,
            &ancestry.comparator_shares,
            comparator_agreements,
            comparator_disagreements,
            maintenance_costs,
        )?;
        for (register, trace) in ancestry.base.next_register_eligibility.iter().enumerate() {
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

fn reserve_drain_loss(scorer: &PointForecastScorer, width: Width, node_floored: &mut bool) -> f64 {
    let q = scorer.q_hat();
    let p_null = 1.0 / (2.0f64.powi(i32::from(width.bits())));
    if q > p_null {
        let one_minus_q = 1.0 - q;
        let denom = if one_minus_q < RESERVE_MISMATCH_DENOM_FLOOR {
            *node_floored = true;
            RESERVE_MISMATCH_DENOM_FLOOR
        } else {
            one_minus_q
        };
        ((1.0 - p_null) / denom).log2().max(0.0)
    } else {
        0.0
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

    #[test]
    fn horizon_derived_reserve_cap_retains_smoothed_share_across_transient_dropouts() {
        let config = EconomyConfig {
            initial_energy: 4.0,
            juvenile_ticks: 0,
            trace_discount: 0.9,
            reserve_cap: ReserveCap::HorizonDerived,
            ..EconomyConfig::default()
        };
        let horizon = test_obs_horizon(&config);
        let mut economy = EconomyState::new(config, Width::BITS_1, 2, 1, 0, horizon).unwrap();
        for _ in 0..10 {
            let _ = economy.port_scorer_mut(0).observe(1, 1);
        }
        let q = economy.scorers()[0].q_hat();
        let drain_loss = ((1.0 - 0.5f64) / (1.0 - q)).log2();
        let w_obs = horizon as f64;
        let full_cap = (w_obs * 0.5 * drain_loss).max(config.initial_energy);

        // Tick 0: both nodes split share [0.5, 0.5], building reserve to full_cap.
        let tick0 = economy
            .apply_tick(&[1000.0], &[vec![0.5, 0.5]], &[0.0, 0.0])
            .unwrap();
        assert_eq!(tick0.clipped_nodes, 2);
        assert!((economy.nodes()[1].energy - full_cap).abs() < 1e-6);

        // Tick 1: node 1 has zero instantaneous share; cap must decay by trace_discount, not snap to 4.0.
        let decayed_cap =
            (w_obs * config.trace_discount * 0.5 * drain_loss).max(config.initial_energy);
        assert!(decayed_cap > config.initial_energy * 2.0);
        let _ = economy
            .apply_tick(&[0.0], &[vec![1.0, 0.0]], &[0.0, 0.0])
            .unwrap();
        assert!((economy.nodes()[1].energy - decayed_cap).abs() < 1e-6);
    }

    #[test]
    fn sleep_freezes_wake_null_model_and_applies_iota_asymmetrically() {
        let config = EconomyConfig {
            juvenile_ticks: 0,
            reserve_cap: ReserveCap::Uncapped,
            ..EconomyConfig::default()
        };
        let mut economy =
            EconomyState::new(config, Width::BITS_1, 4, 1, 0, test_obs_horizon(&config)).unwrap();
        let sub_a = NodeId(2);
        let eqz_a = NodeId(3);
        let comp_idx = economy
            .register_comparator(sub_a, eqz_a, 0, 0b01, 0b01)
            .unwrap();
        assert_eq!(comp_idx, 0);

        // Warm up Wake with balanced alternating stream so p_null(0) == p_null(1) == 0.5.
        for t in 0..40u64 {
            let s = t % 2;
            let r_port = economy.observe_predictions(&[s], &[s]).unwrap();
            let comp_obs = economy
                .observe_comparators(&[s], &[s], &[1], Some(&[s]))
                .unwrap();
            economy
                .apply_tick_with_comparators(
                    &r_port,
                    &[vec![1.0, 0.0, 0.0, 0.0]],
                    &comp_obs.rewards,
                    &[vec![0.0, 1.0, 1.0, 1.0]],
                    comp_obs.agreements,
                    comp_obs.disagreements,
                    &[0.0; 4],
                )
                .unwrap();
        }
        let wake_null_before_sleep = economy.scorers()[0].null_model().clone();
        let wake_comp_q_before = economy.comparators()[0].scorer.q_hat();

        // Enter Sleep: boundary rewards are zero, null model stays frozen, agreement scaled by iota = 0.5.
        economy.set_phase(ExecutionPhase::Sleep);
        let mut sleep_agree_sum = 0.0;
        for t in 0..16u64 {
            let s = t % 2;
            let r_port = economy.observe_predictions(&[s], &[0]).unwrap();
            assert_eq!(r_port, vec![0.0]);
            let comp_obs = economy.observe_comparators(&[s], &[s], &[1], None).unwrap();
            assert!(comp_obs.rewards[0] >= 0.0);
            sleep_agree_sum += comp_obs.rewards[0];
            economy
                .apply_tick_with_comparators(
                    &r_port,
                    &[vec![1.0, 0.0, 0.0, 0.0]],
                    &comp_obs.rewards,
                    &[vec![0.25, 0.25, 0.25, 0.25]],
                    comp_obs.agreements,
                    comp_obs.disagreements,
                    &[0.0; 4],
                )
                .unwrap();
        }
        assert!(sleep_agree_sum > 0.0);
        // Compare against disjoint channels (iota = 1.0) -> exactly 2x positive Sleep agreement flux.
        let mut disjoint_econ =
            EconomyState::new(config, Width::BITS_1, 4, 1, 0, test_obs_horizon(&config)).unwrap();
        disjoint_econ
            .register_comparator(sub_a, eqz_a, 0, 0b01, 0b10)
            .unwrap();
        for t in 0..40u64 {
            let s = t % 2;
            let _ = disjoint_econ.observe_predictions(&[s], &[s]).unwrap();
            let _ = disjoint_econ
                .observe_comparators(&[s], &[s], &[1], Some(&[s]))
                .unwrap();
        }
        disjoint_econ.set_phase(ExecutionPhase::Sleep);
        let mut disjoint_sum = 0.0;
        for t in 0..16u64 {
            let s = t % 2;
            let obs = disjoint_econ
                .observe_comparators(&[s], &[s], &[1], None)
                .unwrap();
            disjoint_sum += obs.rewards[0];
        }
        assert!((disjoint_sum - 2.0 * sleep_agree_sum).abs() < 1e-12);

        // Verify disagreement pays full 1.0 loss regardless of channel overlap (iota = 0.5 == iota = 1.0).
        let shared_disagree = economy.observe_comparators(&[0], &[1], &[0], None).unwrap();
        let disjoint_disagree = disjoint_econ
            .observe_comparators(&[0], &[1], &[0], None)
            .unwrap();
        assert!(shared_disagree.rewards[0] < 0.0);
        assert!((shared_disagree.rewards[0] - disjoint_disagree.rewards[0]).abs() < 1e-12);

        assert_eq!(
            economy.scorers()[0].null_model().probability(0).to_bits(),
            wake_null_before_sleep.probability(0).to_bits()
        );
        assert_eq!(
            economy.scorers()[0].null_model().probability(1).to_bits(),
            wake_null_before_sleep.probability(1).to_bits()
        );
        assert_eq!(
            economy.comparators()[0]
                .scorer
                .null_model()
                .probability(0)
                .to_bits(),
            wake_null_before_sleep.probability(0).to_bits()
        );
        assert_eq!(
            economy.comparators()[0]
                .scorer
                .null_model()
                .probability(1)
                .to_bits(),
            wake_null_before_sleep.probability(1).to_bits()
        );
        assert_eq!(economy.comparators()[0].scorer.q_hat(), wake_comp_q_before);
        assert_eq!(economy.comparator_agreement_ticks(), 56);
        assert_eq!(economy.comparator_disagreement_ticks(), 0);
    }

    #[test]
    fn comparator_hallucinating_wrong_agreement_starves_in_wake() {
        let config = EconomyConfig {
            juvenile_ticks: 0,
            reserve_cap: ReserveCap::Uncapped,
            ..EconomyConfig::default()
        };
        let mut economy =
            EconomyState::new(config, Width::BITS_1, 4, 1, 0, test_obs_horizon(&config)).unwrap();
        economy
            .register_comparator(NodeId(2), NodeId(3), 0, 0b01, 0b10)
            .unwrap();

        // Warm up briefly (16 ticks), then both primary and alt agree (`eqz == 1`) on `1 - fact`
        // while settling realistic maturity description debt (> initial_energy = 4.0).
        // Net comparator flux after the drift turns strictly negative and starves all comparator nodes.
        let debts = [12.0, 12.0, 12.0, 12.0];
        let mut post_drift_comp_flux = 0.0;
        for t in 0..80u64 {
            let fact = t % 2;
            let hallucinated = if t < 16 { fact } else { 1 - fact };
            let r_port = economy
                .observe_predictions(&[hallucinated], &[fact])
                .unwrap();
            let comp_obs = economy
                .observe_comparators(&[hallucinated], &[hallucinated], &[1], Some(&[fact]))
                .unwrap();
            let maint = if t == 16 { debts } else { [0.0; 4] };
            let tick = economy
                .apply_tick_with_comparators(
                    &r_port,
                    &[vec![1.0, 0.0, 0.0, 0.0]],
                    &comp_obs.rewards,
                    &[vec![0.0, 1.0, 1.0, 1.0]],
                    comp_obs.agreements,
                    comp_obs.disagreements,
                    &maint,
                )
                .unwrap();
            if t >= 16 {
                assert!(
                    comp_obs.rewards[0] <= 0.0,
                    "mutually agreeing wrong oscillator must not gain wake reward at tick {t}"
                );
                assert!(tick.comparator_flux <= 0.0);
                post_drift_comp_flux += tick.comparator_flux;
            }
        }
        assert!(post_drift_comp_flux < 0.0);
        assert!(economy.nodes()[1].energy <= 0.0);
        assert!(economy.nodes()[2].energy <= 0.0);
        assert!(economy.nodes()[3].energy <= 0.0);
    }

    #[test]
    fn economy_remap_structure_updates_and_prunes_comparators() {
        let config = EconomyConfig::default();
        let mut economy =
            EconomyState::new(config, Width::BITS_1, 5, 1, 0, test_obs_horizon(&config)).unwrap();
        economy
            .register_comparator(NodeId(1), NodeId(2), 0, 0b01, 0b10)
            .unwrap();
        economy
            .register_comparator(NodeId(3), NodeId(4), 0, 0b01, 0b01)
            .unwrap();
        economy.set_phase(ExecutionPhase::Sleep);

        // Drop node 0, compact 1->0, 2->1, drop 3->None (prunes comp 1), 4->2.
        let remapped = economy
            .remap_structure(&[None, Some(0), Some(1), None, Some(2)], 3, &[], 0)
            .unwrap();
        assert_eq!(remapped.phase(), ExecutionPhase::Sleep);
        assert_eq!(remapped.comparators().len(), 1);
        assert_eq!(remapped.comparators()[0].sub_node, NodeId(0));
        assert_eq!(remapped.comparators()[0].eqz_node, NodeId(1));
    }
}
