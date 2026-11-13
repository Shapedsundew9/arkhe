//! Finite-ring residual diagnostics for bounded corrector growth.

use std::collections::{BTreeMap, BTreeSet};

use crate::baselines::{ObservationWindow, TickBudget};
use crate::metrics::{PointForecastScorer, circuit_code_bits_with_tax, facts_code_bits};
use crate::rng::SeedBank;
use crate::structural::economy::{EconomyConfig, EconomyState, score_with_reward_mode};
use crate::structural::graph_edit::{
    build_autonomous_ring, materialize_const_loop, stack_output_binary_op, tap_source,
};
use crate::structural::live::node_output_values;
use crate::structural::sensitivity::{assemble_causal_ancestry, counterfactual_sensitivity};
use crate::structural::surgery::{
    EditableCircuit, MarginalDebtLedger, explicit_delay_register_index,
    port_upstream_cone_and_edges, port_uses_boundary_in, register_mapping,
};
use crate::substrate::{
    Circuit, NodeId, Primitive, ScalarState, SignalSink, SignalSource, Width, WireDelayMode,
};

/// Numerical tolerance for deterministic candidate $\Delta J$ tie-breaking.
pub const MDL_TIE_BREAK_EPS: f64 = 1e-9;

/// Bounded sensitivity sample count during autonomous delay-ring juvenile simulation.
pub const RING_SIM_SENSITIVITY_SAMPLES: usize = 4;

/// Maximum small nonzero constant literal enumerated unconditionally in motif generation
/// (supplemented dynamically by any detected dominant `constant_offset`).
pub const SMALL_CONST_CANDIDATE_CAP: u64 = 3;

/// Derived unified observation horizon $W_{\text{obs}} = t_{\text{juv}} + 2 W_{\text{diag}}$ (ticks).
#[must_use]
pub const fn observation_horizon(economy: &EconomyConfig, growth: &GrowthConfig) -> usize {
    (economy.juvenile_ticks as usize).saturating_add(growth.diagnostic_window.saturating_mul(2))
}

/// Growth thresholds and resource bounds used by an online M1 learner.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct GrowthConfig {
    /// Number of post-juvenile symbols used for residual diagnostics.
    pub diagnostic_window: usize,
    /// Minimum number of ticks between structural growth attempts.
    pub sprout_interval: u64,
    /// Bias-corrected mutual-information threshold in bits.
    pub mi_threshold_bits: f64,
    /// Minimum discrete exact-match gain over predicting residual zero.
    pub min_match_gain: usize,
    /// Maximum number of live substrate nodes (compute guardrail).
    pub max_nodes: usize,
    /// Pre-registered upper bound on reactive causal input/output lags $\tau_{\max}$.
    pub max_reactive_lag: u8,
    /// Pre-registered cap on ranked passing candidate feature groups evaluated per sprout.
    pub candidate_feature_cap: usize,
    /// Pre-registered minimum autonomous delay-ring period.
    pub min_ring_period: usize,
    /// Pre-registered maximum autonomous delay-ring period.
    pub max_ring_period: usize,
    /// Counterfactual sensitivity sample budget per tick when $k > 4$ (exact below).
    pub sensitivity_sample_count: usize,
}

impl Default for GrowthConfig {
    fn default() -> Self {
        Self {
            diagnostic_window: 64,
            sprout_interval: 32,
            mi_threshold_bits: 0.05,
            min_match_gain: 4,
            max_nodes: 32,
            max_reactive_lag: 3,
            candidate_feature_cap: 6,
            min_ring_period: 2,
            max_ring_period: 6,
            sensitivity_sample_count: 8,
        }
    }
}

impl GrowthConfig {
    /// Returns whether all growth limits and search bounds are usable.
    #[must_use]
    pub fn is_valid(self) -> bool {
        self.diagnostic_window >= 2
            && self.sprout_interval > 0
            && self.mi_threshold_bits.is_finite()
            && self.mi_threshold_bits >= 0.0
            && self.min_match_gain > 0
            && self.max_nodes > 0
            && self.max_reactive_lag >= 1
            && self.candidate_feature_cap >= 1
            && self.min_ring_period >= 2
            && self.max_ring_period >= self.min_ring_period
            && self.sensitivity_sample_count >= 1
    }
}

/// A time-aligned causal feature considered for predicting the residual.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResidualFeature {
    /// Stable source label, used for deterministic tie-breaking and reports.
    pub name: String,
    /// Symbol trace aligned with `residuals`.
    pub values: Vec<u64>,
}

/// Mutual information and discrete correction gain for one feature or feature pair.
#[derive(Clone, Debug, PartialEq)]
pub struct FeatureScore {
    /// Indices into the supplied feature list.
    pub feature_indices: Vec<usize>,
    /// Uncorrected plug-in mutual information.
    pub mutual_information_bits: f64,
    /// Miller–Madow corrected mutual information.
    pub corrected_mutual_information_bits: f64,
    /// Match-count gain over predicting residual zero independently per feature value.
    pub match_gain: usize,
    /// Number of ticks on which this feature directly matches the residual.
    pub direct_matches: usize,
}

/// Summary of residual bias, entropy, autocorrelation, and predictive features.
#[derive(Clone, Debug, PartialEq)]
pub struct ResidualDiagnostics {
    /// Number of symbols in the analyzed window.
    pub sample_count: usize,
    /// Empirical residual entropy in bits.
    pub residual_entropy_bits: f64,
    /// Most frequent non-zero residual and its excess count over zero, when significant.
    pub constant_offset: Option<(u64, usize)>,
    /// Whether all residuals are zero.
    pub zero_error: bool,
    /// Mutual information between the residual and its lags `1..=config.max_reactive_lag`.
    pub lagged_mutual_information_bits: Vec<f64>,
    /// Best singleton or pair candidate, selected deterministically.
    pub best_feature: Option<FeatureScore>,
    /// Secondary candidate (second index of pair, or highest-scoring distinct singleton).
    pub secondary_feature: Option<FeatureScore>,
    /// Top candidate feature groups clearing the growth guards.
    pub candidate_features: Vec<FeatureScore>,
    /// Whether candidate features were truncated by the candidate cap.
    pub candidate_list_truncated: bool,
    /// Whether a non-zero constant offset or feature clears the growth guards.
    pub growth_criterion_met: bool,
}

/// Telemetry counters for structural growth guardrails and search horizons.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SproutTelemetry {
    /// Count of sprout or ring proposals blocked by `max_nodes`.
    pub sprout_blocked_max_nodes: u64,
    /// Count of rejected candidates whose required observation window exceeded `obs_window`.
    pub candidate_undecidable: u64,
    /// Count of candidate feature lists truncated by `candidate_feature_cap`.
    pub candidate_list_truncated: u64,
    /// Count of diagnostic checks where `max_reactive_lag` excluded a predictive longer lag or yielded no motif.
    pub lag_horizon_bound: u64,
    /// Count of autonomous ring checks where `max_ring_period` excluded an MDL-improving longer ring.
    pub ring_period_bound: u64,
}

/// Invalid or dimensionally inconsistent residual diagnostics input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GrowthError {
    /// Residual or feature traces are empty.
    EmptyTrace,
    /// At least one feature trace does not align with the residual trace.
    TraceLengthMismatch,
    /// Growth thresholds or node limits are invalid.
    InvalidConfig,
}

impl std::fmt::Display for GrowthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyTrace => write!(f, "residual diagnostic traces must not be empty"),
            Self::TraceLengthMismatch => {
                write!(f, "residual and feature traces must have equal lengths")
            }
            Self::InvalidConfig => write!(f, "residual growth configuration is invalid"),
        }
    }
}

impl std::error::Error for GrowthError {}

/// Computes plug-in information, Miller–Madow bias correction, and match gain for residuals.
pub fn diagnose_residual(
    width: Width,
    residuals: &[u64],
    features: &[ResidualFeature],
    config: GrowthConfig,
) -> Result<ResidualDiagnostics, GrowthError> {
    if !config.is_valid() {
        return Err(GrowthError::InvalidConfig);
    }
    if residuals.is_empty() {
        return Err(GrowthError::EmptyTrace);
    }
    if features
        .iter()
        .any(|feature| feature.values.len() != residuals.len())
    {
        return Err(GrowthError::TraceLengthMismatch);
    }
    let residuals: Vec<u64> = residuals
        .iter()
        .map(|&value| width.normalize(value))
        .collect();
    let zero_error = residuals.iter().all(|&value| value == 0);
    let residual_entropy_bits = entropy(residuals.iter().copied());
    let counts = frequencies(residuals.iter().copied());
    let zero_count = counts.get(&0).copied().unwrap_or(0);
    let constant_offset = counts
        .iter()
        .filter(|(symbol, _)| **symbol != 0)
        .max_by(|(symbol_a, count_a), (symbol_b, count_b)| {
            count_a.cmp(count_b).then_with(|| symbol_b.cmp(symbol_a))
        })
        .and_then(|(&symbol, &count)| {
            (count.saturating_sub(zero_count) >= config.min_match_gain)
                .then_some((symbol, count.saturating_sub(zero_count)))
        });

    let mut scores = Vec::new();
    for first in 0..features.len() {
        scores.push(score_feature_group(&residuals, features, &[first], width));
        for second in (first + 1)..features.len() {
            scores.push(score_feature_group(
                &residuals,
                features,
                &[first, second],
                width,
            ));
        }
    }
    scores.sort_by(|left, right| {
        right
            .corrected_mutual_information_bits
            .total_cmp(&left.corrected_mutual_information_bits)
            .then_with(|| right.match_gain.cmp(&left.match_gain))
            .then_with(|| left.feature_indices.len().cmp(&right.feature_indices.len()))
            .then_with(|| right.direct_matches.cmp(&left.direct_matches))
            .then_with(|| left.feature_indices.cmp(&right.feature_indices))
    });
    let best_feature = scores.first().cloned();
    let secondary_feature = if let Some(ref best) = best_feature {
        if best.feature_indices.len() > 1 {
            Some(FeatureScore {
                feature_indices: vec![best.feature_indices[1]],
                mutual_information_bits: 0.0,
                corrected_mutual_information_bits: 0.0,
                match_gain: 0,
                direct_matches: 0,
            })
        } else {
            let u1_idx = best.feature_indices[0];
            scores
                .iter()
                .find(|s| s.feature_indices.len() == 1 && s.feature_indices[0] != u1_idx)
                .cloned()
        }
    } else {
        None
    };

    let max_lag = usize::from(config.max_reactive_lag);
    let mut lagged_mutual_information_bits = Vec::with_capacity(max_lag);
    for lag in 1..=max_lag {
        let count = residuals.len().saturating_sub(lag);
        if count == 0 {
            lagged_mutual_information_bits.push(0.0);
        } else {
            lagged_mutual_information_bits.push(mutual_information(
                &residuals[lag..],
                &[residuals[..count].to_vec()],
            ));
        }
    }
    let passing_candidates = scores.iter().filter(|s| {
        s.corrected_mutual_information_bits > config.mi_threshold_bits
            && s.match_gain >= config.min_match_gain
    });
    let total_passing = passing_candidates.clone().count();
    let candidate_features: Vec<FeatureScore> = passing_candidates
        .take(config.candidate_feature_cap)
        .cloned()
        .collect();
    let candidate_list_truncated = total_passing > config.candidate_feature_cap;

    let growth_criterion_met = !zero_error
        && (constant_offset.is_some()
            || best_feature.as_ref().is_some_and(|score| {
                score.corrected_mutual_information_bits > config.mi_threshold_bits
                    && score.match_gain >= config.min_match_gain
            }));

    Ok(ResidualDiagnostics {
        sample_count: residuals.len(),
        residual_entropy_bits,
        constant_offset,
        zero_error,
        lagged_mutual_information_bits,
        best_feature,
        secondary_feature,
        candidate_features,
        candidate_list_truncated,
        growth_criterion_met,
    })
}

fn score_feature_group(
    residuals: &[u64],
    features: &[ResidualFeature],
    indices: &[usize],
    width: Width,
) -> FeatureScore {
    let feature_values: Vec<Vec<u64>> = indices
        .iter()
        .map(|&index| {
            features[index]
                .values
                .iter()
                .map(|&value| width.normalize(value))
                .collect()
        })
        .collect();
    let mutual_information_bits = mutual_information(residuals, &feature_values);
    let alphabet_size = joint_frequencies(&feature_values).len();
    let residual_alphabet_size = frequencies(residuals.iter().copied()).len();
    let correction = (alphabet_size.saturating_sub(1) as f64)
        * (residual_alphabet_size.saturating_sub(1) as f64)
        / (2.0 * std::f64::consts::LN_2 * residuals.len() as f64);
    let corrected_mutual_information_bits = mutual_information_bits - correction;
    let match_gain = match_gain(residuals, &feature_values);
    let direct_matches = if indices.len() == 1 {
        residuals
            .iter()
            .zip(&feature_values[0])
            .filter(|&(r, v)| *r == *v)
            .count()
    } else {
        0
    };
    FeatureScore {
        feature_indices: indices.to_vec(),
        mutual_information_bits,
        corrected_mutual_information_bits,
        match_gain,
        direct_matches,
    }
}

fn mutual_information(residuals: &[u64], features: &[Vec<u64>]) -> f64 {
    let feature_counts = joint_frequencies(features);
    let residual_counts = frequencies(residuals.iter().copied());
    let mut joint = BTreeMap::<(Vec<u64>, u64), usize>::new();
    for index in 0..residuals.len() {
        let key = (
            features
                .iter()
                .map(|feature| feature[index])
                .collect::<Vec<_>>(),
            residuals[index],
        );
        *joint.entry(key).or_default() += 1;
    }
    let n = residuals.len() as f64;
    let mut information = 0.0;
    for ((feature_values, residual), count) in joint {
        let p_joint = count as f64 / n;
        let p_feature = feature_counts[&feature_values] as f64 / n;
        let p_residual = residual_counts[&residual] as f64 / n;
        information += p_joint * (p_joint / (p_feature * p_residual)).log2();
    }
    information.max(0.0)
}

fn match_gain(residuals: &[u64], features: &[Vec<u64>]) -> usize {
    let mut counts = BTreeMap::<Vec<u64>, BTreeMap<u64, usize>>::new();
    for (index, &residual) in residuals.iter().enumerate() {
        let key = features
            .iter()
            .map(|feature| feature[index])
            .collect::<Vec<_>>();
        *counts.entry(key).or_default().entry(residual).or_default() += 1;
    }
    counts
        .values()
        .map(|symbols| {
            symbols
                .values()
                .copied()
                .max()
                .unwrap_or(0)
                .saturating_sub(symbols.get(&0).copied().unwrap_or(0))
        })
        .sum()
}

fn joint_frequencies(features: &[Vec<u64>]) -> BTreeMap<Vec<u64>, usize> {
    let sample_count = features.first().map_or(0, Vec::len);
    let mut counts = BTreeMap::new();
    for index in 0..sample_count {
        let key = features
            .iter()
            .map(|feature| feature[index])
            .collect::<Vec<_>>();
        *counts.entry(key).or_default() += 1;
    }
    counts
}

fn frequencies(values: impl IntoIterator<Item = u64>) -> BTreeMap<u64, usize> {
    let mut counts = BTreeMap::new();
    for value in values {
        *counts.entry(value).or_default() += 1;
    }
    counts
}

fn entropy(values: impl IntoIterator<Item = u64>) -> f64 {
    let counts = frequencies(values);
    let total = counts.values().sum::<usize>() as f64;
    counts
        .values()
        .map(|&count| {
            let probability = count as f64 / total;
            -probability * probability.log2()
        })
        .sum()
}

/// A causal signal source considered for residual prediction.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SourceFeature {
    /// Direct boundary input `in:c`.
    BoundaryIn(u16),
    /// Live internal node output `node:{id}:{port}`.
    NodeOut { node: NodeId, port: u8 },
    /// Virtual causal delay of boundary input `lag_in:{c}:{tau}`.
    LagIn { channel: u16, tau: u8 },
    /// Virtual causal delay of current prediction `lag_yhat:{p}:{tau}`.
    LagYhat { port: u16, tau: u8 },
    /// Virtual causal delay of residual `lag_res:{p}:{tau}`.
    LagRes { port: u16, tau: u8 },
}

impl SourceFeature {
    /// Formats this feature into its canonical label.
    #[must_use]
    pub fn name(&self) -> String {
        match self {
            Self::BoundaryIn(c) => format!("in:{c}"),
            Self::NodeOut { node, port } => format!("node:{}:{port}", node.index()),
            Self::LagIn { channel, tau } => format!("lag_in:{channel}:{tau}"),
            Self::LagYhat { port, tau } => format!("lag_yhat:{port}:{tau}"),
            Self::LagRes { port, tau } => format!("lag_res:{port}:{tau}"),
        }
    }

    /// Parses a feature name into a [`SourceFeature`].
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split(':').collect();
        match parts.as_slice() {
            ["in", c] => Some(Self::BoundaryIn(c.parse().ok()?)),
            ["node", n, p] => Some(Self::NodeOut {
                node: NodeId(n.parse().ok()?),
                port: p.parse().ok()?,
            }),
            ["lag_in", c, tau] => Some(Self::LagIn {
                channel: c.parse().ok()?,
                tau: tau.parse().ok()?,
            }),
            ["lag_yhat", p, tau] => Some(Self::LagYhat {
                port: p.parse().ok()?,
                tau: tau.parse().ok()?,
            }),
            ["lag_res", p, tau] => Some(Self::LagRes {
                port: p.parse().ok()?,
                tau: tau.parse().ok()?,
            }),
            _ => None,
        }
    }
}

/// A candidate micro-motif in Level-0 bank $\mathcal{K}(u_1, u_2)$ (Charter §3.3 & M1b §4.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CandidateMotif {
    // 1. Unary / Direct Receptors
    Identity(SourceFeature),
    Eqz(SourceFeature),
    // 2. Binary & Self-Interaction
    AddSelf(SourceFeature),
    AddPair(SourceFeature, SourceFeature),
    SubPair(SourceFeature, SourceFeature),
    MulPair(SourceFeature, SourceFeature),
    // 3. Constant-Loop & Shifted/Scaled Motifs
    ConstLoop(u64),
    AddConst(SourceFeature, u64),
    EqzAddConst(SourceFeature, u64),
    Neg(SourceFeature),
    ScaleEqz(SourceFeature, u64),
    // 4. Nonlinear Interaction Motifs
    AddEqz(SourceFeature, SourceFeature),
    MulEqz(SourceFeature, SourceFeature),
    MulEqzPair(SourceFeature, SourceFeature),
}

/// Rolling buffer of post-juvenile frames for residual diagnosis and feature extraction.
#[derive(Clone, Debug)]
pub struct DiagnosticBuffer {
    pub capacity: usize,
    pub inputs: Vec<Vec<u64>>,
    pub truth: Vec<Vec<u64>>,
    pub predictions: Vec<Vec<u64>>,
    pub residuals: Vec<Vec<u64>>,
    pub node_outputs: Vec<Vec<u64>>,
    pub counterfactual_sensitivity: Vec<Vec<bool>>,
}

impl DiagnosticBuffer {
    /// Creates an empty buffer with capacity `capacity`.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            inputs: Vec::with_capacity(capacity),
            truth: Vec::with_capacity(capacity),
            predictions: Vec::with_capacity(capacity),
            residuals: Vec::with_capacity(capacity),
            node_outputs: Vec::with_capacity(capacity),
            counterfactual_sensitivity: Vec::with_capacity(capacity),
        }
    }

    /// Appends a newly observed frame.
    pub fn push_frame(
        &mut self,
        input: &[u64],
        truth: &[u64],
        prediction: &[u64],
        residual: &[u64],
        node_outs: &[u64],
        sensitive: &[bool],
    ) {
        if self.inputs.len() >= self.capacity {
            self.inputs.remove(0);
            self.truth.remove(0);
            self.predictions.remove(0);
            self.residuals.remove(0);
            self.node_outputs.remove(0);
            self.counterfactual_sensitivity.remove(0);
        }
        self.inputs.push(input.to_vec());
        self.truth.push(truth.to_vec());
        self.predictions.push(prediction.to_vec());
        self.residuals.push(residual.to_vec());
        self.node_outputs.push(node_outs.to_vec());
        self.counterfactual_sensitivity.push(sensitive.to_vec());
    }

    /// Clears all buffered frames.
    pub fn clear(&mut self) {
        self.inputs.clear();
        self.truth.clear();
        self.predictions.clear();
        self.residuals.clear();
        self.node_outputs.clear();
        self.counterfactual_sensitivity.clear();
    }

    /// Number of buffered frames.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inputs.len()
    }

    /// Whether buffer is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inputs.is_empty()
    }

    /// Whether buffer reached target capacity.
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.inputs.len() >= self.capacity
    }

    /// Returns mature nodes with zero counterfactual sensitivity across all frames (D1b.1).
    pub fn idle_mature_nodes(&self, economy: &EconomyState) -> BTreeSet<NodeId> {
        let mut idle = BTreeSet::new();
        if !self.is_full() {
            return idle;
        }
        for (node_idx, node_econ) in economy.nodes().iter().enumerate() {
            if node_econ.juvenile_ticks > 0 {
                continue;
            }
            let zero_sensitivity = self
                .counterfactual_sensitivity
                .iter()
                .all(|frame| !frame.get(node_idx).copied().unwrap_or(false));
            if zero_sensitivity {
                idle.insert(NodeId(u32::try_from(node_idx).expect("fits in u32")));
            }
        }
        idle
    }

    /// Builds the causal signal pool $\mathcal{U}$ over buffered ticks (§4.2).
    ///
    /// When `economy` is provided, features originating from juvenile nodes are excluded (§2.4).
    pub fn build_signal_pool(
        &self,
        circuit: &Circuit,
        obs_window: &ObservationWindow,
        out_port: u16,
        max_tau: u8,
        economy: Option<&EconomyState>,
    ) -> Vec<ResidualFeature> {
        let mut features = Vec::new();
        let len = self.len();
        if len == 0 {
            return features;
        }

        // 1. Direct boundary inputs: in:c
        let num_channels = self.inputs[0].len();
        for c in 0..num_channels {
            let values = self.inputs.iter().map(|frame| frame[c]).collect();
            features.push(ResidualFeature {
                name: SourceFeature::BoundaryIn(c as u16).name(),
                values,
            });
        }

        // 2. Live internal node outputs (excluding juvenile nodes, §2.4)
        for (node_idx, &primitive) in circuit.nodes().iter().enumerate() {
            if let Some(econ) = economy
                && econ
                    .nodes()
                    .get(node_idx)
                    .is_some_and(|n| n.juvenile_ticks > 0)
            {
                continue;
            }
            let node = NodeId(u32::try_from(node_idx).expect("fits in u32"));
            if primitive == Primitive::Split {
                let consumers = circuit
                    .edges()
                    .iter()
                    .filter(|e| matches!(e.src, SignalSource::NodeOut { node: n, .. } if n == node))
                    .count();
                if consumers <= 1 {
                    continue;
                }
            }
            let values = self
                .node_outputs
                .iter()
                .map(|frame| frame.get(node_idx).copied().unwrap_or(0))
                .collect();
            features.push(ResidualFeature {
                name: SourceFeature::NodeOut { node, port: 0 }.name(),
                values,
            });
        }

        // 3. Virtual causal z^-tau delay taps (tau in 1..=max_tau)
        let window_len = obs_window.len();
        let start_in_window = window_len.saturating_sub(len);

        for tau in 1..=max_tau {
            for c in 0..num_channels {
                let values =
                    lagged_boundary_values(obs_window, start_in_window, len, c, usize::from(tau));
                features.push(ResidualFeature {
                    name: SourceFeature::LagIn {
                        channel: c as u16,
                        tau,
                    }
                    .name(),
                    values,
                });
            }

            let is_driven = circuit
                .edges()
                .iter()
                .any(|e| e.dst == SignalSink::BoundaryOut(out_port));

            if is_driven {
                let mut yhat_vals = Vec::with_capacity(len);
                for t in 0..len {
                    let val = if t >= usize::from(tau) {
                        self.predictions[t - usize::from(tau)][usize::from(out_port)]
                    } else {
                        0
                    };
                    yhat_vals.push(val);
                }
                features.push(ResidualFeature {
                    name: SourceFeature::LagYhat {
                        port: out_port,
                        tau,
                    }
                    .name(),
                    values: yhat_vals,
                });

                let mut res_vals = Vec::with_capacity(len);
                for t in 0..len {
                    let val = if t >= usize::from(tau) {
                        self.residuals[t - usize::from(tau)][usize::from(out_port)]
                    } else {
                        0
                    };
                    res_vals.push(val);
                }
                features.push(ResidualFeature {
                    name: SourceFeature::LagRes {
                        port: out_port,
                        tau,
                    }
                    .name(),
                    values: res_vals,
                });
            }
        }

        features
    }
}

fn lagged_boundary_values(
    obs_window: &ObservationWindow,
    start_in_window: usize,
    len: usize,
    channel: usize,
    tau: usize,
) -> Vec<u64> {
    let mut values = Vec::with_capacity(len);
    for t in 0..len {
        let win_idx = start_in_window + t;
        let lag_t = (win_idx as isize) - 1 - (tau as isize);
        let val = if lag_t >= 0 {
            obs_window
                .frames()
                .get(lag_t as usize)
                .map_or(0, |f| f[channel])
        } else if lag_t == -1 {
            obs_window.prior_frame().get(channel).copied().unwrap_or(0)
        } else {
            0
        };
        values.push(val);
    }
    values
}
#[must_use]
pub fn generate_candidate_motifs(
    u1: &SourceFeature,
    u2: Option<&SourceFeature>,
    width: Width,
    constant_offset: Option<(u64, usize)>,
    is_epoch_1: bool,
    num_channels: u16,
) -> Vec<CandidateMotif> {
    let mut candidates = Vec::new();

    // In Epoch 1, Charter §3.3 prioritizes single-operator reactive receptors on BoundaryIn
    if is_epoch_1 {
        for c in 0..num_channels {
            candidates.push(CandidateMotif::Identity(SourceFeature::BoundaryIn(c)));
            candidates.push(CandidateMotif::Eqz(SourceFeature::BoundaryIn(c)));
        }
    }

    // 1. Unary / Direct Receptors
    if !candidates.contains(&CandidateMotif::Identity(u1.clone())) {
        candidates.push(CandidateMotif::Identity(u1.clone()));
    }
    if !candidates.contains(&CandidateMotif::Eqz(u1.clone())) {
        candidates.push(CandidateMotif::Eqz(u1.clone()));
    }
    if let Some(u2_f) = u2
        && u2_f != u1
    {
        if !candidates.contains(&CandidateMotif::Identity(u2_f.clone())) {
            candidates.push(CandidateMotif::Identity(u2_f.clone()));
        }
        if !candidates.contains(&CandidateMotif::Eqz(u2_f.clone())) {
            candidates.push(CandidateMotif::Eqz(u2_f.clone()));
        }
    }

    // 2. Binary & Self-Interaction
    if width.bits() > 1 {
        candidates.push(CandidateMotif::AddSelf(u1.clone()));
    }
    if let Some(u2_f) = u2
        && u2_f != u1
    {
        candidates.push(CandidateMotif::AddPair(u1.clone(), u2_f.clone()));
        candidates.push(CandidateMotif::SubPair(u1.clone(), u2_f.clone()));
        candidates.push(CandidateMotif::SubPair(u2_f.clone(), u1.clone()));
        candidates.push(CandidateMotif::MulPair(u1.clone(), u2_f.clone()));
    }

    // 3. Constant-Loop & Shifted/Scaled Motifs
    let max_c = width.mask().min(SMALL_CONST_CANDIDATE_CAP);
    let mut constants = Vec::new();
    for c in 1..=max_c {
        constants.push(c);
    }
    if let Some((c_off, _)) = constant_offset {
        let norm_c = width.normalize(c_off);
        if norm_c != 0 && !constants.contains(&norm_c) {
            constants.push(norm_c);
        }
    }
    for &c in &constants {
        candidates.push(CandidateMotif::ConstLoop(c));
        candidates.push(CandidateMotif::AddConst(u1.clone(), c));
        if width.bits() > 1 {
            candidates.push(CandidateMotif::EqzAddConst(u1.clone(), c));
        }
        if width.bits() > 1 && c > 1 {
            candidates.push(CandidateMotif::ScaleEqz(u1.clone(), c));
        }
    }
    candidates.push(CandidateMotif::Neg(u1.clone()));

    // 4. Nonlinear Interaction Motifs
    if let Some(u2_f) = u2
        && u2_f != u1
    {
        candidates.push(CandidateMotif::AddEqz(u1.clone(), u2_f.clone()));
        candidates.push(CandidateMotif::AddEqz(u2_f.clone(), u1.clone()));
        candidates.push(CandidateMotif::MulEqz(u1.clone(), u2_f.clone()));
        candidates.push(CandidateMotif::MulEqz(u2_f.clone(), u1.clone()));
        candidates.push(CandidateMotif::MulEqzPair(u1.clone(), u2_f.clone()));
    }

    candidates
}

fn materialize_source(
    circuit: &mut Circuit,
    source: &SourceFeature,
    _out_port: u16,
) -> SignalSource {
    match source {
        SourceFeature::BoundaryIn(c) => SignalSource::BoundaryIn(*c),
        SourceFeature::NodeOut { node, port } => SignalSource::NodeOut {
            node: *node,
            port: *port,
        },
        SourceFeature::LagIn { channel, tau } => {
            let mut curr = tap_source(circuit, SignalSource::BoundaryIn(*channel));
            for _ in 0..*tau {
                let delay = circuit.add_node(Primitive::Delay { init: 0 });
                circuit.connect(
                    curr,
                    SignalSink::NodeIn {
                        node: delay,
                        port: 0,
                    },
                );
                curr = SignalSource::NodeOut {
                    node: delay,
                    port: 0,
                };
            }
            curr
        }
        SourceFeature::LagYhat { port, tau } => {
            let old_edge = circuit
                .edges()
                .iter()
                .find(|e| e.dst == SignalSink::BoundaryOut(*port))
                .expect("driven output port exists");
            let mut driver = old_edge.src;
            if let SignalSource::NodeOut { node, .. } = driver
                && circuit.nodes().get(node.index()) == Some(&Primitive::Split)
            {
                let consumers = circuit
                    .edges()
                    .iter()
                    .filter(|e| matches!(e.src, SignalSource::NodeOut { node: n, .. } if n == node))
                    .count();
                if consumers == 1
                    && let Some(in_edge) = circuit
                        .edges()
                        .iter()
                        .find(|e| e.dst == SignalSink::NodeIn { node, port: 0 })
                {
                    driver = in_edge.src;
                }
            }
            let mut curr = tap_source(circuit, driver);
            for _ in 0..*tau {
                let delay = circuit.add_node(Primitive::Delay { init: 0 });
                circuit.connect(
                    curr,
                    SignalSink::NodeIn {
                        node: delay,
                        port: 0,
                    },
                );
                curr = SignalSource::NodeOut {
                    node: delay,
                    port: 0,
                };
            }
            curr
        }
        SourceFeature::LagRes { port, tau } => {
            let s_tap = tap_source(circuit, SignalSource::BoundaryIn(*port));
            let old_edge = circuit
                .edges()
                .iter()
                .find(|e| e.dst == SignalSink::BoundaryOut(*port))
                .expect("driven output port exists");
            let mut driver = old_edge.src;
            if let SignalSource::NodeOut { node, .. } = driver
                && circuit.nodes().get(node.index()) == Some(&Primitive::Split)
            {
                let consumers = circuit
                    .edges()
                    .iter()
                    .filter(|e| matches!(e.src, SignalSource::NodeOut { node: n, .. } if n == node))
                    .count();
                if consumers == 1
                    && let Some(in_edge) = circuit
                        .edges()
                        .iter()
                        .find(|e| e.dst == SignalSink::NodeIn { node, port: 0 })
                {
                    driver = in_edge.src;
                }
            }
            let y_tap = tap_source(circuit, driver);
            let d_node = circuit.add_node(Primitive::Delay { init: 0 });
            circuit.connect(
                y_tap,
                SignalSink::NodeIn {
                    node: d_node,
                    port: 0,
                },
            );
            let y_lag1 = SignalSource::NodeOut {
                node: d_node,
                port: 0,
            };

            let sub_node = circuit.add_node(Primitive::Sub);
            let s_tap2 = tap_source(circuit, s_tap);
            circuit.connect(
                s_tap2,
                SignalSink::NodeIn {
                    node: sub_node,
                    port: 0,
                },
            );
            circuit.connect(
                y_lag1,
                SignalSink::NodeIn {
                    node: sub_node,
                    port: 1,
                },
            );
            let mut curr = SignalSource::NodeOut {
                node: sub_node,
                port: 0,
            };

            for _ in 1..*tau {
                let delay = circuit.add_node(Primitive::Delay { init: 0 });
                circuit.connect(
                    curr,
                    SignalSink::NodeIn {
                        node: delay,
                        port: 0,
                    },
                );
                curr = SignalSource::NodeOut {
                    node: delay,
                    port: 0,
                };
            }
            curr
        }
    }
}

fn materialize_motif(circuit: &mut Circuit, motif: &CandidateMotif, out_port: u16) -> SignalSource {
    match motif {
        CandidateMotif::Identity(u) => {
            let src = materialize_source(circuit, u, out_port);
            tap_source(circuit, src)
        }
        CandidateMotif::Eqz(u) => {
            let eqz = circuit.add_node(Primitive::Eqz);
            let src = materialize_source(circuit, u, out_port);
            let tapped = tap_source(circuit, src);
            circuit.connect(tapped, SignalSink::NodeIn { node: eqz, port: 0 });
            SignalSource::NodeOut { node: eqz, port: 0 }
        }
        CandidateMotif::AddSelf(u) => {
            let add = circuit.add_node(Primitive::Add);
            let split = circuit.add_node(Primitive::Split);
            let src = materialize_source(circuit, u, out_port);
            let tapped = tap_source(circuit, src);
            circuit.connect(
                tapped,
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
                SignalSink::NodeIn { node: add, port: 0 },
            );
            circuit.connect(
                SignalSource::NodeOut {
                    node: split,
                    port: 1,
                },
                SignalSink::NodeIn { node: add, port: 1 },
            );
            SignalSource::NodeOut { node: add, port: 0 }
        }
        CandidateMotif::AddPair(u1, u2) => {
            let add = circuit.add_node(Primitive::Add);
            let s1 = materialize_source(circuit, u1, out_port);
            let t1 = tap_source(circuit, s1);
            circuit.connect(t1, SignalSink::NodeIn { node: add, port: 0 });
            let s2 = materialize_source(circuit, u2, out_port);
            let t2 = tap_source(circuit, s2);
            circuit.connect(t2, SignalSink::NodeIn { node: add, port: 1 });
            SignalSource::NodeOut { node: add, port: 0 }
        }
        CandidateMotif::SubPair(u1, u2) => {
            let sub = circuit.add_node(Primitive::Sub);
            let s1 = materialize_source(circuit, u1, out_port);
            let t1 = tap_source(circuit, s1);
            circuit.connect(t1, SignalSink::NodeIn { node: sub, port: 0 });
            let s2 = materialize_source(circuit, u2, out_port);
            let t2 = tap_source(circuit, s2);
            circuit.connect(t2, SignalSink::NodeIn { node: sub, port: 1 });
            SignalSource::NodeOut { node: sub, port: 0 }
        }
        CandidateMotif::MulPair(u1, u2) => {
            let mul = circuit.add_node(Primitive::Mul);
            let s1 = materialize_source(circuit, u1, out_port);
            let t1 = tap_source(circuit, s1);
            circuit.connect(t1, SignalSink::NodeIn { node: mul, port: 0 });
            let s2 = materialize_source(circuit, u2, out_port);
            let t2 = tap_source(circuit, s2);
            circuit.connect(t2, SignalSink::NodeIn { node: mul, port: 1 });
            SignalSource::NodeOut { node: mul, port: 0 }
        }
        CandidateMotif::ConstLoop(c) => materialize_const_loop(circuit, *c),
        CandidateMotif::AddConst(u, c) => {
            let add = circuit.add_node(Primitive::Add);
            let u_src = materialize_source(circuit, u, out_port);
            let u_tap = tap_source(circuit, u_src);
            circuit.connect(u_tap, SignalSink::NodeIn { node: add, port: 0 });
            let c_src = materialize_const_loop(circuit, *c);
            circuit.connect(c_src, SignalSink::NodeIn { node: add, port: 1 });
            SignalSource::NodeOut { node: add, port: 0 }
        }
        CandidateMotif::EqzAddConst(u, c) => {
            let eqz = circuit.add_node(Primitive::Eqz);
            let add_src =
                materialize_motif(circuit, &CandidateMotif::AddConst(u.clone(), *c), out_port);
            circuit.connect(add_src, SignalSink::NodeIn { node: eqz, port: 0 });
            SignalSource::NodeOut { node: eqz, port: 0 }
        }
        CandidateMotif::Neg(u) => {
            if circuit.width().bits() > 1 {
                let mult = (1u64 << circuit.width().bits()) - 1;
                let mul = circuit.add_node(Primitive::Mul);
                let u_src = materialize_source(circuit, u, out_port);
                let u_tap = tap_source(circuit, u_src);
                circuit.connect(u_tap, SignalSink::NodeIn { node: mul, port: 0 });
                let c_src = materialize_const_loop(circuit, mult);
                circuit.connect(c_src, SignalSink::NodeIn { node: mul, port: 1 });
                SignalSource::NodeOut { node: mul, port: 0 }
            } else {
                let sub = circuit.add_node(Primitive::Sub);
                let u_src = materialize_source(circuit, u, out_port);
                let u_tap = tap_source(circuit, u_src);
                circuit.connect(u_tap, SignalSink::NodeIn { node: sub, port: 1 });
                let zero_src = materialize_const_loop(circuit, 0);
                circuit.connect(zero_src, SignalSink::NodeIn { node: sub, port: 0 });
                SignalSource::NodeOut { node: sub, port: 0 }
            }
        }
        CandidateMotif::ScaleEqz(u, c) => {
            let mul = circuit.add_node(Primitive::Mul);
            let eqz = circuit.add_node(Primitive::Eqz);
            let u_src = materialize_source(circuit, u, out_port);
            let u_tap = tap_source(circuit, u_src);
            circuit.connect(u_tap, SignalSink::NodeIn { node: eqz, port: 0 });
            circuit.connect(
                SignalSource::NodeOut { node: eqz, port: 0 },
                SignalSink::NodeIn { node: mul, port: 0 },
            );
            let c_src = materialize_const_loop(circuit, *c);
            circuit.connect(c_src, SignalSink::NodeIn { node: mul, port: 1 });
            SignalSource::NodeOut { node: mul, port: 0 }
        }
        CandidateMotif::AddEqz(u1, u2) => {
            let add = circuit.add_node(Primitive::Add);
            let s1 = materialize_source(circuit, u1, out_port);
            let t1 = tap_source(circuit, s1);
            circuit.connect(t1, SignalSink::NodeIn { node: add, port: 0 });
            let eqz = circuit.add_node(Primitive::Eqz);
            let s2 = materialize_source(circuit, u2, out_port);
            let t2 = tap_source(circuit, s2);
            circuit.connect(t2, SignalSink::NodeIn { node: eqz, port: 0 });
            circuit.connect(
                SignalSource::NodeOut { node: eqz, port: 0 },
                SignalSink::NodeIn { node: add, port: 1 },
            );
            SignalSource::NodeOut { node: add, port: 0 }
        }
        CandidateMotif::MulEqz(u1, u2) => {
            let mul = circuit.add_node(Primitive::Mul);
            let s1 = materialize_source(circuit, u1, out_port);
            let t1 = tap_source(circuit, s1);
            circuit.connect(t1, SignalSink::NodeIn { node: mul, port: 0 });
            let eqz = circuit.add_node(Primitive::Eqz);
            let s2 = materialize_source(circuit, u2, out_port);
            let t2 = tap_source(circuit, s2);
            circuit.connect(t2, SignalSink::NodeIn { node: eqz, port: 0 });
            circuit.connect(
                SignalSource::NodeOut { node: eqz, port: 0 },
                SignalSink::NodeIn { node: mul, port: 1 },
            );
            SignalSource::NodeOut { node: mul, port: 0 }
        }
        CandidateMotif::MulEqzPair(u1, u2) => {
            let mul = circuit.add_node(Primitive::Mul);
            let eqz1 = circuit.add_node(Primitive::Eqz);
            let s1 = materialize_source(circuit, u1, out_port);
            let t1 = tap_source(circuit, s1);
            circuit.connect(
                t1,
                SignalSink::NodeIn {
                    node: eqz1,
                    port: 0,
                },
            );
            circuit.connect(
                SignalSource::NodeOut {
                    node: eqz1,
                    port: 0,
                },
                SignalSink::NodeIn { node: mul, port: 0 },
            );
            let eqz2 = circuit.add_node(Primitive::Eqz);
            let s2 = materialize_source(circuit, u2, out_port);
            let t2 = tap_source(circuit, s2);
            circuit.connect(
                t2,
                SignalSink::NodeIn {
                    node: eqz2,
                    port: 0,
                },
            );
            circuit.connect(
                SignalSource::NodeOut {
                    node: eqz2,
                    port: 0,
                },
                SignalSink::NodeIn { node: mul, port: 1 },
            );
            SignalSource::NodeOut { node: mul, port: 0 }
        }
    }
}

/// Materializes a candidate motif onto a staged circuit and stacks it onto `out_port`.
pub fn materialize_candidate(
    circuit: &mut Circuit,
    motif: &CandidateMotif,
    out_port: u16,
) -> NodeId {
    let is_driven = circuit
        .edges()
        .iter()
        .any(|edge| edge.dst == SignalSink::BoundaryOut(out_port));
    match motif {
        CandidateMotif::Neg(u) if is_driven => {
            let u_src = materialize_source(circuit, u, out_port);
            stack_output_binary_op(circuit, Primitive::Sub, out_port, u_src)
        }
        CandidateMotif::ScaleEqz(u, c) if is_driven && *c == circuit.width().mask() => {
            let eqz = circuit.add_node(Primitive::Eqz);
            let u_src = materialize_source(circuit, u, out_port);
            let u_tap = tap_source(circuit, u_src);
            circuit.connect(u_tap, SignalSink::NodeIn { node: eqz, port: 0 });
            stack_output_binary_op(
                circuit,
                Primitive::Sub,
                out_port,
                SignalSource::NodeOut { node: eqz, port: 0 },
            )
        }
        _ => {
            let motif_src = materialize_motif(circuit, motif, out_port);
            stack_output_binary_op(circuit, Primitive::Add, out_port, motif_src)
        }
    }
}

/// Evaluates candidates from $\mathcal{K}(u_1, u_2)$ on `obs_window` and returns the best staged circuit if admissible (§4.3).
#[allow(clippy::too_many_arguments)]
pub fn propose_sprout(
    editable: &EditableCircuit,
    obs_window: &ObservationWindow,
    diag: &DiagnosticBuffer,
    growth_config: &GrowthConfig,
    economy_config: &EconomyConfig,
    budget: &mut TickBudget,
    out_port: u16,
) -> Result<Option<Circuit>, GrowthError> {
    propose_sprout_with_telemetry(
        editable,
        obs_window,
        diag,
        growth_config,
        economy_config,
        budget,
        out_port,
        growth_config.max_reactive_lag,
        &mut SproutTelemetry::default(),
    )
}

/// Baseline performance summary over the replay window prior to candidate sprouting.
struct BaselineReplayMetrics {
    e_curr: usize,
    e_const_mode: usize,
    flux_curr: f64,
    l_curr: f64,
    j_curr: f64,
    start_idx: usize,
    eval_start_idx: usize,
    w_replay: usize,
    survivor_map: Vec<Option<NodeId>>,
}

#[allow(clippy::too_many_arguments)]
fn replay_current_baseline(
    editable: &EditableCircuit,
    obs_window: &ObservationWindow,
    growth_config: &GrowthConfig,
    economy_config: &EconomyConfig,
    out_port: u16,
    is_epoch_1: bool,
) -> BaselineReplayMetrics {
    let w_diag = growth_config.diagnostic_window;
    let w_replay = obs_window.len().min(2 * w_diag);
    let start_idx = obs_window.len() - w_replay;
    let eval_start_idx = obs_window.len() - w_diag;

    let max_sym_count = frequencies(obs_window.frames()[eval_start_idx..].iter().map(|f| {
        editable
            .circuit()
            .width()
            .normalize(f[usize::from(out_port)])
    }))
    .values()
    .copied()
    .max()
    .unwrap_or(0);
    let e_const_mode = w_diag.saturating_sub(max_sym_count);

    let mut curr_state = ScalarState::new(editable.compiled());
    if let Some((snap_epoch, snap_regs)) = obs_window.state_snapshot_at(start_idx)
        && snap_epoch == editable.structure_epoch()
    {
        let _ = curr_state.set_registers(editable.compiled(), snap_regs);
    }
    let mut curr_pred = vec![0u64; usize::from(editable.compiled().num_outputs())];
    let mut curr_scorer =
        PointForecastScorer::new(editable.circuit().width(), economy_config.null_model);

    let mut e_curr = 0usize;
    let mut flux_curr = 0.0;
    for i in 0..w_replay {
        let idx = start_idx + i;
        let input = if idx == 0 {
            obs_window.prior_frame()
        } else {
            &obs_window.frames()[idx - 1]
        };
        let truth = &obs_window.frames()[idx];
        let _ = curr_state.step(editable.compiled(), input, &mut curr_pred);
        let p_curr = if is_epoch_1 {
            0
        } else {
            curr_pred[usize::from(out_port)]
        };
        let p_truth = truth[usize::from(out_port)];
        let reward_curr = if is_epoch_1 {
            curr_scorer.null_model_mut().observe_and_surprisal(p_truth);
            0.0
        } else {
            score_with_reward_mode(
                &mut curr_scorer,
                economy_config.reward_mode,
                editable.circuit().width(),
                p_curr,
                p_truth,
            )
        };
        if idx >= eval_start_idx {
            if p_curr != p_truth {
                e_curr += 1;
            }
            flux_curr += reward_curr;
        }
    }

    let l_curr = circuit_code_bits_with_tax(editable.circuit(), 0, economy_config.split_code_tax);
    let j_curr = l_curr + facts_code_bits(editable.circuit().width(), w_diag as u64, e_curr as u64);
    let survivor_map: Vec<Option<NodeId>> = (0..editable.circuit().node_count())
        .map(|i| Some(NodeId(i as u32)))
        .collect();

    BaselineReplayMetrics {
        e_curr,
        e_const_mode,
        flux_curr,
        l_curr,
        j_curr,
        start_idx,
        eval_start_idx,
        w_replay,
        survivor_map,
    }
}

enum SproutMotifOutcome {
    Accepted {
        circuit: Circuit,
        e_cand: usize,
        j_cand: f64,
        delta_flux: f64,
        is_1_op: bool,
    },
    BlockedMaxNodes,
    Rejected,
    OutOfBudget,
}

#[allow(clippy::too_many_arguments)]
fn evaluate_sprout_motif(
    editable: &EditableCircuit,
    obs_window: &ObservationWindow,
    growth_config: &GrowthConfig,
    economy_config: &EconomyConfig,
    out_port: u16,
    motif: &CandidateMotif,
    is_epoch_1: bool,
    baseline: &BaselineReplayMetrics,
    budget: Option<&mut TickBudget>,
    mut telemetry: Option<&mut SproutTelemetry>,
) -> SproutMotifOutcome {
    let w_diag = growth_config.diagnostic_window;
    let mut staged = editable.circuit().clone();
    materialize_candidate(&mut staged, motif, out_port);
    if staged.nodes().len() > growth_config.max_nodes {
        if let Some(tel) = telemetry.as_deref_mut() {
            tel.sprout_blocked_max_nodes += 1;
        }
        return SproutMotifOutcome::BlockedMaxNodes;
    }
    if let Some(b) = budget
        && !b.try_consume(baseline.w_replay as u64)
    {
        return SproutMotifOutcome::OutOfBudget;
    }
    let Ok(staged_compiled) = staged.compile() else {
        return SproutMotifOutcome::Rejected;
    };

    let mut state = ScalarState::new(&staged_compiled);
    if let Some((snap_epoch, snap_regs)) = obs_window.state_snapshot_at(baseline.start_idx)
        && snap_epoch == editable.structure_epoch()
    {
        let reg_map = register_mapping(
            editable.circuit(),
            &staged,
            editable.compiled(),
            &staged_compiled,
            &baseline.survivor_map,
        );
        let mut staged_regs = state.registers().to_vec();
        for (old_r, new_r) in reg_map.iter().enumerate() {
            if let Some(nr) = new_r
                && old_r < snap_regs.len()
            {
                staged_regs[*nr] = snap_regs[old_r];
            }
        }
        let _ = state.set_registers(&staged_compiled, &staged_regs);
    }
    let mut pred = vec![0u64; usize::from(staged_compiled.num_outputs())];
    let mut scorer = PointForecastScorer::new(staged.width(), economy_config.null_model);

    let mut e_cand = 0usize;
    let mut flux_cand = 0.0;
    let mut pos_flux_cand = 0.0;

    for i in 0..baseline.w_replay {
        let idx = baseline.start_idx + i;
        let input = if idx == 0 {
            obs_window.prior_frame()
        } else {
            &obs_window.frames()[idx - 1]
        };
        let truth = &obs_window.frames()[idx];
        if state.step(&staged_compiled, input, &mut pred).is_err() {
            return SproutMotifOutcome::Rejected;
        }
        let p_cand = pred[usize::from(out_port)];
        let p_truth = truth[usize::from(out_port)];
        let reward = score_with_reward_mode(
            &mut scorer,
            economy_config.reward_mode,
            staged.width(),
            p_cand,
            p_truth,
        );
        if idx >= baseline.eval_start_idx {
            if p_cand != p_truth {
                e_cand += 1;
            }
            flux_cand += reward;
            pos_flux_cand += reward.max(0.0);
        }
    }

    if e_cand + growth_config.min_match_gain > baseline.e_curr {
        return SproutMotifOutcome::Rejected;
    }
    if is_epoch_1 && !matches!(motif, CandidateMotif::ConstLoop(_)) {
        let is_binary_eqz_scaffold = staged.width() == Width::BITS_1
            && matches!(motif, CandidateMotif::Eqz(SourceFeature::BoundaryIn(_)));
        if is_binary_eqz_scaffold {
            if e_cand > baseline.e_const_mode.saturating_add(1) {
                return SproutMotifOutcome::Rejected;
            }
        } else if e_cand + growth_config.min_match_gain > baseline.e_const_mode {
            return SproutMotifOutcome::Rejected;
        }
    }

    let delta_flux = flux_cand - baseline.flux_curr;
    let l_cand = circuit_code_bits_with_tax(&staged, 0, economy_config.split_code_tax);
    let delta_l_sprout = (l_cand - baseline.l_curr).max(0.0);
    let is_1_op = is_epoch_1
        && matches!(
            motif,
            CandidateMotif::Identity(SourceFeature::BoundaryIn(_))
                | CandidateMotif::Eqz(SourceFeature::BoundaryIn(_))
        );

    if economy_config.juvenile_ticks > 0 && delta_l_sprout > 0.0 {
        let projected_survival = economy_config.initial_energy
            + (economy_config.juvenile_ticks as f64 / w_diag as f64) * pos_flux_cand;
        if projected_survival <= delta_l_sprout {
            let delta_i_hat = pos_flux_cand / (w_diag as f64);
            if delta_i_hat > 0.0 {
                let w_star = delta_l_sprout / delta_i_hat;
                if w_star > (obs_window.capacity() as f64)
                    && let Some(tel) = telemetry
                {
                    tel.candidate_undecidable += 1;
                }
            }
            return SproutMotifOutcome::Rejected;
        }
    }

    let allow_eqz_scaffold_zero_flux = is_1_op
        && staged.width() == Width::BITS_1
        && matches!(motif, CandidateMotif::Eqz(SourceFeature::BoundaryIn(_)))
        && pos_flux_cand > delta_l_sprout;
    if delta_flux <= 0.0 && !allow_eqz_scaffold_zero_flux {
        return SproutMotifOutcome::Rejected;
    }

    let j_cand = l_cand + facts_code_bits(staged.width(), w_diag as u64, e_cand as u64);

    if !is_epoch_1 && j_cand >= baseline.j_curr && e_cand > 0 {
        return SproutMotifOutcome::Rejected;
    }

    SproutMotifOutcome::Accepted {
        circuit: staged,
        e_cand,
        j_cand,
        delta_flux,
        is_1_op,
    }
}

#[allow(clippy::too_many_arguments)]
fn has_admissible_deeper_lag(
    editable: &EditableCircuit,
    obs_window: &ObservationWindow,
    residuals: &[u64],
    max_tau: u8,
    growth_config: &GrowthConfig,
    economy_config: &EconomyConfig,
    out_port: u16,
    is_epoch_1: bool,
    baseline: &BaselineReplayMetrics,
) -> bool {
    let probe_max = (growth_config.diagnostic_window / 2)
        .min(obs_window.len().saturating_sub(1))
        .min(usize::from(u8::MAX));
    let start_tau = usize::from(max_tau).saturating_add(1);
    if start_tau > probe_max || residuals.is_empty() {
        return false;
    }
    let width = editable.circuit().width();
    let norm_residuals: Vec<u64> = residuals.iter().map(|&v| width.normalize(v)).collect();
    let len = norm_residuals.len();
    let start_in_window = obs_window.len().saturating_sub(len);
    let num_channels = editable.circuit().num_inputs();
    for tau in start_tau..=probe_max {
        for c in 0..usize::from(num_channels) {
            let values = lagged_boundary_values(obs_window, start_in_window, len, c, tau);
            let feat = [ResidualFeature {
                name: String::new(),
                values,
            }];
            let score = score_feature_group(&norm_residuals, &feat, &[0], width);
            if score.corrected_mutual_information_bits > growth_config.mi_threshold_bits
                && score.match_gain >= growth_config.min_match_gain
            {
                let u1 = SourceFeature::LagIn {
                    channel: c as u16,
                    tau: tau as u8,
                };
                for motif in
                    generate_candidate_motifs(&u1, None, width, None, is_epoch_1, num_channels)
                {
                    if matches!(
                        evaluate_sprout_motif(
                            editable,
                            obs_window,
                            growth_config,
                            economy_config,
                            out_port,
                            &motif,
                            is_epoch_1,
                            baseline,
                            None,
                            None,
                        ),
                        SproutMotifOutcome::Accepted { .. }
                    ) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Evaluates candidates from $\mathcal{K}(u_1, u_2)$ on `obs_window` with telemetry collection.
#[allow(clippy::too_many_arguments)]
pub fn propose_sprout_with_telemetry(
    editable: &EditableCircuit,
    obs_window: &ObservationWindow,
    diag: &DiagnosticBuffer,
    growth_config: &GrowthConfig,
    economy_config: &EconomyConfig,
    budget: &mut TickBudget,
    out_port: u16,
    max_tau: u8,
    telemetry: &mut SproutTelemetry,
) -> Result<Option<Circuit>, GrowthError> {
    let len = diag.len();
    if len < growth_config.diagnostic_window {
        return Ok(None);
    }
    let residuals: Vec<u64> = diag
        .residuals
        .iter()
        .map(|f| f[usize::from(out_port)])
        .collect();

    let features = diag.build_signal_pool(
        editable.circuit(),
        obs_window,
        out_port,
        max_tau,
        Some(editable.economy()),
    );

    let is_epoch_1 = !editable
        .circuit()
        .edges()
        .iter()
        .any(|edge| edge.dst == SignalSink::BoundaryOut(out_port));

    let diagnostics = diagnose_residual(
        editable.circuit().width(),
        &residuals,
        &features,
        *growth_config,
    )?;
    if diagnostics.candidate_list_truncated {
        telemetry.candidate_list_truncated += 1;
    }
    if diagnostics.zero_error {
        return Ok(None);
    }

    let baseline = replay_current_baseline(
        editable,
        obs_window,
        growth_config,
        economy_config,
        out_port,
        is_epoch_1,
    );

    if !diagnostics.growth_criterion_met {
        if has_admissible_deeper_lag(
            editable,
            obs_window,
            &residuals,
            max_tau,
            growth_config,
            economy_config,
            out_port,
            is_epoch_1,
            &baseline,
        ) {
            telemetry.lag_horizon_bound += 1;
        }
        return Ok(None);
    }

    let mut feature_pairs: Vec<(SourceFeature, Option<SourceFeature>)> = Vec::new();
    if let Some(first) = diagnostics.candidate_features.first() {
        let u1_name = &features[first.feature_indices[0]].name;
        if let Some(u1) = SourceFeature::parse(u1_name) {
            let u2 = if first.feature_indices.len() > 1 {
                let u2_name = &features[first.feature_indices[1]].name;
                SourceFeature::parse(u2_name)
            } else {
                diagnostics.secondary_feature.as_ref().and_then(|sec| {
                    let u2_name = &features[sec.feature_indices[0]].name;
                    SourceFeature::parse(u2_name)
                })
            };
            feature_pairs.push((u1, u2));
        }
    }

    for feat in &diagnostics.candidate_features {
        let u1_name = &features[feat.feature_indices[0]].name;
        if let Some(u1) = SourceFeature::parse(u1_name) {
            let u2 = if feat.feature_indices.len() > 1 {
                let u2_name = &features[feat.feature_indices[1]].name;
                SourceFeature::parse(u2_name)
            } else {
                None
            };
            if !feature_pairs.contains(&(u1.clone(), u2.clone())) {
                feature_pairs.push((u1, u2));
            }
        }
    }

    let num_channels = editable.circuit().num_inputs();
    let mut candidates = Vec::new();
    if feature_pairs.is_empty() {
        if let Some((c_off, _)) = diagnostics.constant_offset {
            let norm_c = editable.circuit().width().normalize(c_off);
            if norm_c != 0 {
                candidates.push(CandidateMotif::ConstLoop(norm_c));
            }
        }
    } else {
        for (u1_feat, u2_feat) in &feature_pairs {
            let cands = generate_candidate_motifs(
                u1_feat,
                u2_feat.as_ref(),
                editable.circuit().width(),
                diagnostics.constant_offset,
                is_epoch_1,
                num_channels,
            );
            for c in cands {
                if !candidates.contains(&c) {
                    candidates.push(c);
                }
            }
        }
    }

    if candidates.is_empty() {
        if !feature_pairs.is_empty() {
            telemetry.lag_horizon_bound += 1;
        }
        return Ok(None);
    }

    let mut best_proposal: Option<(Circuit, usize, f64, f64, bool)> = None;
    let mut blocked_by_max_nodes = 0u64;
    for motif in &candidates {
        match evaluate_sprout_motif(
            editable,
            obs_window,
            growth_config,
            economy_config,
            out_port,
            motif,
            is_epoch_1,
            &baseline,
            Some(budget),
            Some(telemetry),
        ) {
            SproutMotifOutcome::Accepted {
                circuit: staged,
                e_cand,
                j_cand,
                delta_flux,
                is_1_op,
            } => {
                let is_better = match &best_proposal {
                    None => true,
                    Some((_, _, prev_j, prev_flux, prev_1_op)) => {
                        if is_epoch_1 {
                            if is_1_op && !prev_1_op {
                                true
                            } else if !is_1_op && *prev_1_op {
                                false
                            } else {
                                j_cand < *prev_j
                                    || ((j_cand - *prev_j).abs() < MDL_TIE_BREAK_EPS
                                        && delta_flux > *prev_flux)
                            }
                        } else {
                            j_cand < *prev_j
                                || ((j_cand - *prev_j).abs() < MDL_TIE_BREAK_EPS
                                    && delta_flux > *prev_flux)
                        }
                    }
                };
                if is_better {
                    best_proposal = Some((staged, e_cand, j_cand, delta_flux, is_1_op));
                }
            }
            SproutMotifOutcome::BlockedMaxNodes => {
                blocked_by_max_nodes += 1;
            }
            SproutMotifOutcome::Rejected => {}
            SproutMotifOutcome::OutOfBudget => break,
        }
    }

    if best_proposal.is_none()
        && blocked_by_max_nodes == 0
        && has_admissible_deeper_lag(
            editable,
            obs_window,
            &residuals,
            max_tau,
            growth_config,
            economy_config,
            out_port,
            is_epoch_1,
            &baseline,
        )
    {
        telemetry.lag_horizon_bound += 1;
    }

    Ok(best_proposal.map(|(lat, _, _, _, _)| lat))
}

/// Autonomous delay ring proposal under channel noise (§4.5).
#[derive(Clone, Debug, PartialEq)]
pub struct AutonomousRingProposal {
    /// Staged circuit containing the autonomous delay ring.
    pub candidate_circuit: Circuit,
    /// Node survival map from pre-edit circuit nodes (`None` for replaced `out_port` cone nodes).
    pub old_to_new: Vec<Option<NodeId>>,
    /// Explicit `Delay` node IDs in `candidate_circuit` forming the period-`p` ring (`0..period`).
    pub ring_delay_nodes: Vec<NodeId>,
    /// Period of the delay ring.
    pub period: usize,
    /// Learned clean symbol template for the cycle.
    pub template: Vec<u64>,
}

fn build_autonomous_ring_circuit(
    base: &Circuit,
    period: usize,
    mu: &[u64],
    out_port: u16,
) -> (Circuit, Vec<Option<NodeId>>, Vec<NodeId>) {
    let (target_cone, _) = port_upstream_cone_and_edges(base, out_port);
    let mut protected_nodes = BTreeSet::new();
    for q in 0..base.num_outputs() {
        if q != out_port {
            let (other_cone, _) = port_upstream_cone_and_edges(base, q);
            protected_nodes.extend(other_cone);
        }
    }
    let removed: BTreeSet<NodeId> = target_cone
        .into_iter()
        .filter(|node| !protected_nodes.contains(node))
        .collect();

    let mut staged = base.clone();
    let old_out_edges: Vec<(SignalSource, SignalSink)> = staged
        .edges()
        .iter()
        .filter_map(|e| (e.dst == SignalSink::BoundaryOut(out_port)).then_some((e.src, e.dst)))
        .collect();
    for (src, dst) in old_out_edges {
        staged.disconnect(src, dst);
    }
    let old_to_new = staged.remove_nodes(&removed);
    let (delays, _split) = build_autonomous_ring(&mut staged, period, mu, out_port);
    (staged, old_to_new, delays)
}

#[allow(clippy::too_many_arguments)]
fn simulate_ring_juvenile_survival(
    editable: &EditableCircuit,
    obs_window: &ObservationWindow,
    staged: &Circuit,
    old_to_new: &[Option<NodeId>],
    ring_delay_nodes: &[NodeId],
    period: usize,
    mu: &[u64],
    out_port: u16,
    l_curr: f64,
    economy_config: &EconomyConfig,
) -> bool {
    let j_ticks = economy_config.juvenile_ticks as usize;
    let mut sim = editable.clone();
    if sim
        .recompile_preserving_state(staged.clone(), old_to_new)
        .is_err()
    {
        return false;
    }
    let mut sim_ledger = MarginalDebtLedger {
        last_committed_code_bits: l_curr,
        ..MarginalDebtLedger::default()
    };
    sim_ledger.on_sprout(sim.circuit(), economy_config.split_code_tax, old_to_new);

    let mut is_survivor = vec![false; sim.circuit().node_count()];
    for &mapped in old_to_new.iter().flatten() {
        if mapped.index() < is_survivor.len() {
            is_survivor[mapped.index()] = true;
        }
    }

    if j_ticks == 0 {
        return is_survivor
            .iter()
            .enumerate()
            .filter(|(_, surv)| !**surv)
            .all(|(idx, _)| economy_config.initial_energy > sim_ledger.node_debts[idx]);
    }

    let w_replay = obs_window.len();
    let start_offset = w_replay.saturating_sub(j_ticks);
    let align_tick = obs_window.start_tick() + (start_offset as u64);

    let mut regs = sim.state().registers().to_vec();
    for (j, &delay_node) in ring_delay_nodes.iter().enumerate() {
        let Some(reg_idx) = explicit_delay_register_index(sim.circuit(), delay_node) else {
            return false;
        };
        regs[reg_idx] = mu[((align_tick as usize) + (period - 1 - j)) % period];
    }
    if sim.set_registers(&regs).is_err() {
        return false;
    }

    let out_idx = usize::from(out_port);
    sim.economy_mut()
        .port_scorer_mut(out_idx)
        .reset_predictor_counts();

    let cycle_len = (w_replay - (w_replay % period)).max(period).min(w_replay);
    let num_ports = sim.economy().scorers().len();
    let mut predictions = vec![0u64; usize::from(sim.compiled().num_outputs())];
    let zeros_maint = vec![0.0; sim.circuit().node_count()];
    let seed_bank = SeedBank::new(0);

    for step_idx in 0..j_ticks {
        let frame_idx = if start_offset + step_idx < w_replay {
            start_offset + step_idx
        } else {
            (start_offset + step_idx) % cycle_len
        };
        let input = if frame_idx == 0 {
            obs_window.prior_frame()
        } else {
            &obs_window.frames()[frame_idx - 1]
        };
        let truth = &obs_window.frames()[frame_idx];
        if sim.step(input, &mut predictions).is_err() {
            return false;
        }
        let node_outputs = node_output_values(sim.circuit(), sim.state());
        if sim.economy_mut().record_activity(&node_outputs).is_err() {
            return false;
        }
        let Ok(snapshot) = counterfactual_sensitivity(
            sim.circuit(),
            sim.compiled(),
            sim.state(),
            seed_bank,
            step_idx as u64,
            RING_SIM_SENSITIVITY_SAMPLES,
        ) else {
            return false;
        };
        let Ok(ancestry) = assemble_causal_ancestry(
            sim.circuit(),
            sim.compiled(),
            sim.state(),
            &snapshot,
            &sim.economy().activity(),
            sim.economy().register_eligibility(),
            economy_config.trace_discount,
        ) else {
            return false;
        };

        let mut port_rewards = vec![0.0; num_ports];
        if ancestry.port_shares[out_idx].iter().sum::<f64>() > 0.0 {
            let Ok(r) =
                sim.economy_mut()
                    .score_port_single(out_idx, predictions[out_idx], truth[out_idx])
            else {
                return false;
            };
            port_rewards[out_idx] = r;
        } else {
            let _ = sim
                .economy_mut()
                .observe_unconnected_port(out_idx, truth[out_idx]);
        }

        if sim
            .economy_mut()
            .apply_causal_tick(&port_rewards, &ancestry, &zeros_maint)
            .is_err()
        {
            return false;
        }
    }

    for (idx, &surv) in is_survivor.iter().enumerate() {
        if !surv && sim.economy().nodes()[idx].energy <= sim_ledger.node_debts[idx] {
            return false;
        }
    }
    true
}

/// Evaluates autonomous delay-ring candidates in `min_ring_period..=max_ring_period` under channel noise (§4.5).
pub fn propose_autonomous_delay_ring(
    editable: &EditableCircuit,
    obs_window: &ObservationWindow,
    growth_config: &GrowthConfig,
    economy_config: &EconomyConfig,
    budget: &mut TickBudget,
    out_port: u16,
) -> Option<AutonomousRingProposal> {
    propose_autonomous_delay_ring_with_telemetry(
        editable,
        obs_window,
        growth_config,
        economy_config,
        budget,
        out_port,
        &mut SproutTelemetry::default(),
    )
}

enum RingPeriodOutcome {
    Accepted(AutonomousRingProposal, f64),
    Rejected,
    OutOfBudget,
}

#[allow(clippy::too_many_arguments)]
fn evaluate_ring_period(
    editable: &EditableCircuit,
    obs_window: &ObservationWindow,
    growth_config: &GrowthConfig,
    economy_config: &EconomyConfig,
    out_port: u16,
    p: usize,
    e_curr: usize,
    flux_curr: f64,
    l_curr: f64,
    j_curr: f64,
    budget: Option<&mut TickBudget>,
    telemetry: Option<&mut SproutTelemetry>,
) -> RingPeriodOutcome {
    let w_replay = obs_window.len();
    let out_idx = usize::from(out_port);
    let mut residue_counts = vec![BTreeMap::<u64, usize>::new(); p];
    for (i, frame) in obs_window.frames().iter().enumerate() {
        let global_t = obs_window.start_tick() + (i as u64);
        let r = (global_t as usize) % p;
        *residue_counts[r].entry(frame[out_idx]).or_default() += 1;
    }
    let mut mu = vec![0u64; p];
    for (r, counts) in residue_counts.iter().enumerate() {
        let mode = counts
            .iter()
            .max_by(|(s1, c1), (s2, c2)| c1.cmp(c2).then_with(|| s2.cmp(s1)))
            .map(|(&s, _)| s)
            .unwrap_or(0);
        mu[r] = mode;
    }
    if mu.iter().all(|&v| v == mu[0]) {
        return RingPeriodOutcome::Rejected;
    }

    let (staged, old_to_new, ring_delay_nodes) =
        build_autonomous_ring_circuit(editable.circuit(), p, &mu, out_port);
    if staged.nodes().len() > growth_config.max_nodes {
        if let Some(tel) = telemetry {
            tel.sprout_blocked_max_nodes += 1;
        }
        return RingPeriodOutcome::Rejected;
    }
    if let Some(b) = budget
        && !b.try_consume(w_replay as u64)
    {
        return RingPeriodOutcome::OutOfBudget;
    }

    let Ok(staged_compiled) = staged.compile() else {
        return RingPeriodOutcome::Rejected;
    };

    let mut state = ScalarState::new(&staged_compiled);
    let mut regs = state.registers().to_vec();
    if let Some((snap_epoch, snap_regs)) = obs_window.state_snapshot_at(0)
        && snap_epoch == editable.structure_epoch()
    {
        let reg_map = register_mapping(
            editable.circuit(),
            &staged,
            editable.compiled(),
            &staged_compiled,
            &old_to_new,
        );
        for (old_r, new_r) in reg_map.iter().enumerate() {
            if let Some(nr) = new_r
                && old_r < snap_regs.len()
            {
                regs[*nr] = snap_regs[old_r];
            }
        }
    }
    let start_tick = obs_window.start_tick();
    let mut reg_map_ok = true;
    for (j, &delay_node) in ring_delay_nodes.iter().enumerate() {
        let Some(reg_idx) = explicit_delay_register_index(&staged, delay_node) else {
            reg_map_ok = false;
            break;
        };
        regs[reg_idx] = mu[((start_tick as usize) + (p - 1 - j)) % p];
    }
    if !reg_map_ok || state.set_registers(&staged_compiled, &regs).is_err() {
        return RingPeriodOutcome::Rejected;
    }

    let mut scorer = PointForecastScorer::new(staged.width(), economy_config.null_model);
    let mut e_ring = 0usize;
    let mut flux_ring = 0.0;
    let mut pred = vec![0u64; usize::from(staged_compiled.num_outputs())];

    for (i, frame) in obs_window.frames().iter().enumerate() {
        let input = if i == 0 {
            obs_window.prior_frame()
        } else {
            &obs_window.frames()[i - 1]
        };
        if state.step(&staged_compiled, input, &mut pred).is_err() {
            return RingPeriodOutcome::Rejected;
        }
        if pred[out_idx] != frame[out_idx] {
            e_ring += 1;
        }
        flux_ring += score_with_reward_mode(
            &mut scorer,
            economy_config.reward_mode,
            staged.width(),
            pred[out_idx],
            frame[out_idx],
        );
    }

    if e_ring + growth_config.min_match_gain > e_curr {
        return RingPeriodOutcome::Rejected;
    }
    if flux_ring <= flux_curr {
        return RingPeriodOutcome::Rejected;
    }
    let l_ring = circuit_code_bits_with_tax(&staged, 0, economy_config.split_code_tax);
    let j_ring = l_ring + facts_code_bits(staged.width(), w_replay as u64, e_ring as u64);
    if j_ring >= j_curr {
        return RingPeriodOutcome::Rejected;
    }

    if !simulate_ring_juvenile_survival(
        editable,
        obs_window,
        &staged,
        &old_to_new,
        &ring_delay_nodes,
        p,
        &mu,
        out_port,
        l_curr,
        economy_config,
    ) {
        return RingPeriodOutcome::Rejected;
    }

    RingPeriodOutcome::Accepted(
        AutonomousRingProposal {
            candidate_circuit: staged,
            old_to_new,
            ring_delay_nodes,
            period: p,
            template: mu,
        },
        j_ring,
    )
}

/// Evaluates autonomous delay-ring candidates with bound/guardrail telemetry (§4.5).
pub fn propose_autonomous_delay_ring_with_telemetry(
    editable: &EditableCircuit,
    obs_window: &ObservationWindow,
    growth_config: &GrowthConfig,
    economy_config: &EconomyConfig,
    budget: &mut TickBudget,
    out_port: u16,
    telemetry: &mut SproutTelemetry,
) -> Option<AutonomousRingProposal> {
    if editable.circuit().wire_delay() != WireDelayMode::Combinational {
        return None;
    }
    let has_port_driver = editable
        .circuit()
        .edges()
        .iter()
        .any(|edge| edge.dst == SignalSink::BoundaryOut(out_port));
    if has_port_driver && !port_uses_boundary_in(editable.circuit(), out_port) {
        return None;
    }
    let w_diag = growth_config.diagnostic_window;
    if obs_window.len() < 2 * w_diag {
        return None;
    }

    let w_replay = obs_window.len();
    let diag_start = w_replay.saturating_sub(w_diag);
    let out_idx = usize::from(out_port);

    let mut curr_state = ScalarState::new(editable.compiled());
    if let Some((snap_epoch, snap_regs)) = obs_window.state_snapshot_at(0)
        && snap_epoch == editable.structure_epoch()
    {
        let _ = curr_state.set_registers(editable.compiled(), snap_regs);
    }
    let mut curr_pred = vec![0u64; usize::from(editable.compiled().num_outputs())];
    let mut curr_scorer =
        PointForecastScorer::new(editable.circuit().width(), economy_config.null_model);
    let mut e_curr = 0usize;
    let mut e_curr_diag = 0usize;
    let mut flux_curr = 0.0;
    for (i, frame) in obs_window.frames().iter().enumerate() {
        let input = if i == 0 {
            obs_window.prior_frame()
        } else {
            &obs_window.frames()[i - 1]
        };
        if curr_state
            .step(editable.compiled(), input, &mut curr_pred)
            .is_err()
        {
            return None;
        }
        let p_curr = if has_port_driver {
            curr_pred[out_idx]
        } else {
            0
        };
        let p_truth = frame[out_idx];
        if p_curr != p_truth {
            e_curr += 1;
            if i >= diag_start {
                e_curr_diag += 1;
            }
        }
        if has_port_driver {
            flux_curr += score_with_reward_mode(
                &mut curr_scorer,
                economy_config.reward_mode,
                editable.circuit().width(),
                p_curr,
                p_truth,
            );
        } else {
            curr_scorer.null_model_mut().observe_and_surprisal(p_truth);
        }
    }
    if e_curr_diag == 0 {
        return None;
    }

    let l_curr = circuit_code_bits_with_tax(editable.circuit(), 0, economy_config.split_code_tax);
    let j_curr =
        l_curr + facts_code_bits(editable.circuit().width(), w_replay as u64, e_curr as u64);

    let mut best_ring: Option<(AutonomousRingProposal, f64)> = None;

    for p in growth_config.min_ring_period..=growth_config.max_ring_period {
        match evaluate_ring_period(
            editable,
            obs_window,
            growth_config,
            economy_config,
            out_port,
            p,
            e_curr,
            flux_curr,
            l_curr,
            j_curr,
            Some(budget),
            Some(telemetry),
        ) {
            RingPeriodOutcome::Accepted(prop, j_ring) => {
                if best_ring
                    .as_ref()
                    .is_none_or(|(_, best_j)| j_ring < *best_j)
                {
                    best_ring = Some((prop, j_ring));
                }
            }
            RingPeriodOutcome::Rejected => {}
            RingPeriodOutcome::OutOfBudget => break,
        }
    }

    if best_ring.is_none() {
        let probe_ceiling = (w_diag / 2).max(growth_config.max_ring_period);
        for p in (2..growth_config.min_ring_period)
            .chain((growth_config.max_ring_period + 1)..=probe_ceiling)
        {
            if let RingPeriodOutcome::Accepted(..) = evaluate_ring_period(
                editable,
                obs_window,
                growth_config,
                economy_config,
                out_port,
                p,
                e_curr,
                flux_curr,
                l_curr,
                j_curr,
                None,
                None,
            ) {
                telemetry.ring_period_bound += 1;
                break;
            }
        }
    }

    best_ring.map(|(prop, _)| prop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn residual_diagnostics_find_structured_corrector_and_constant_offset() {
        let config = GrowthConfig {
            min_match_gain: 2,
            mi_threshold_bits: 0.0,
            ..GrowthConfig::default()
        };
        let diagnostics = diagnose_residual(
            Width::BITS_2,
            &[0, 1, 0, 1, 0, 1, 0, 1],
            &[ResidualFeature {
                name: "input".to_owned(),
                values: vec![0, 1, 0, 1, 0, 1, 0, 1],
            }],
            config,
        )
        .unwrap();
        assert!(diagnostics.growth_criterion_met);
        assert_eq!(diagnostics.best_feature.unwrap().feature_indices, vec![0]);

        let offset = diagnose_residual(Width::BITS_2, &[1, 1, 1, 1, 1, 1], &[], config).unwrap();
        assert_eq!(offset.constant_offset, Some((1, 6)));
        assert!(offset.growth_criterion_met);
    }

    #[test]
    fn residual_diagnostics_halt_on_zero_error_and_reject_misaligned_features() {
        let config = GrowthConfig::default();
        let zero = diagnose_residual(
            Width::BITS_1,
            &[0, 0, 0, 0],
            &[ResidualFeature {
                name: "feature".to_owned(),
                values: vec![0, 1, 0, 1],
            }],
            config,
        )
        .unwrap();
        assert!(zero.zero_error);
        assert!(!zero.growth_criterion_met);

        assert_eq!(
            diagnose_residual(
                Width::BITS_1,
                &[0, 1],
                &[ResidualFeature {
                    name: "short".to_owned(),
                    values: vec![0],
                }],
                config,
            ),
            Err(GrowthError::TraceLengthMismatch)
        );
    }

    #[test]
    fn materialize_candidate_on_unconnected_port_correctly_negates_and_scales() {
        use crate::substrate::WireDelayMode;

        let circuit = Circuit::new(Width::BITS_2, 1, 1, WireDelayMode::Combinational);

        // 1. Neg on unconnected port: -x mod 4
        let mut neg_circ = circuit.clone();
        materialize_candidate(
            &mut neg_circ,
            &CandidateMotif::Neg(SourceFeature::BoundaryIn(0)),
            0,
        );
        let compiled = neg_circ.compile().unwrap();
        let mut state = ScalarState::new(&compiled);
        let mut out = [0u64];
        // For input = 1, -1 mod 4 = 3
        state.step(&compiled, &[1], &mut out).unwrap();
        assert_eq!(out[0], 3, "Neg(1) mod 4 must be 3, got {}", out[0]);

        // 2. ScaleEqz on unconnected port: 3 * Eqz(x) mod 4
        let mut scale_circ = circuit;
        materialize_candidate(
            &mut scale_circ,
            &CandidateMotif::ScaleEqz(SourceFeature::BoundaryIn(0), 3),
            0,
        );
        let compiled2 = scale_circ.compile().unwrap();
        let mut state2 = ScalarState::new(&compiled2);
        let mut out2 = [0u64];
        // For input = 0, Eqz(0) = 1, so 3 * 1 = 3 mod 4
        state2.step(&compiled2, &[0], &mut out2).unwrap();
        assert_eq!(out2[0], 3, "ScaleEqz(0, 3) must be 3, got {}", out2[0]);
    }

    #[test]
    fn feature_pairing_includes_best_and_secondary_features() {
        let u1 = SourceFeature::BoundaryIn(0);
        let u2 = SourceFeature::LagIn { channel: 0, tau: 1 };
        let cands = generate_candidate_motifs(&u1, Some(&u2), Width::BITS_2, None, false, 1);
        assert!(cands.contains(&CandidateMotif::AddPair(u1.clone(), u2.clone())));
        assert!(cands.contains(&CandidateMotif::SubPair(u1.clone(), u2.clone())));
        assert!(cands.contains(&CandidateMotif::AddEqz(u1.clone(), u2.clone())));
    }

    #[test]
    fn two_stage_synthesis_all_four_g1b1_tasks() {
        use crate::structural::live::node_output_values;
        use crate::substrate::WireDelayMode;

        let tasks: Vec<(Width, Vec<u64>, &str)> = vec![
            (Width::BITS_1, vec![0, 1, 1], "PeriodicMotif([0,1,1])"),
            (Width::BITS_2, vec![0, 1, 3], "PeriodicMotif([0,1,3])"),
            (Width::BITS_2, vec![0, 1, 2, 3], "AffineLag1(1,1)"),
            (Width::BITS_2, vec![0, 1, 2], "CounterModN(3)"),
        ];

        for (width, period_seq, name) in tasks {
            let p_len = period_seq.len();
            let circuit = Circuit::new(width, 1, 1, WireDelayMode::Combinational);

            let economy_config = EconomyConfig {
                initial_energy: 10.0,
                juvenile_ticks: 128,
                split_code_tax: true,
                ..EconomyConfig::default()
            };
            let growth_config = GrowthConfig {
                diagnostic_window: 64,
                sprout_interval: 32,
                mi_threshold_bits: 0.05,
                min_match_gain: 4,
                max_nodes: 32,
                ..GrowthConfig::default()
            };

            let mut editable = EditableCircuit::new(circuit, economy_config).unwrap();
            let mut obs_window = ObservationWindow::new(1, 128);
            let mut diag = DiagnosticBuffer::new(64);
            let mut budget = TickBudget::new(100_000);

            // Stage 0: 128 ticks with empty circuit (warmup obs_window to 2 * W_diag)
            let mut prev_sym = 0u64;
            let mut e0 = 0usize;
            for t in 0..128 {
                let truth = period_seq[t % p_len];
                let mut pred = [0u64];
                let compiled = editable.compiled().clone();
                editable
                    .state_mut()
                    .step(&compiled, &[prev_sym], &mut pred)
                    .unwrap();
                let residual = width.sub(truth, pred[0]);
                if t >= 64 && pred[0] != truth {
                    e0 += 1;
                }
                obs_window.push_frame(&[truth]);
                diag.push_frame(&[prev_sym], &[truth], &pred, &[residual], &[], &[]);
                editable
                    .economy_mut()
                    .port_scorer_mut(0)
                    .null_model_mut()
                    .observe_and_surprisal(truth);
                prev_sym = truth;
            }

            // Propose Sprout 1 (First neuron)
            let sprout1 = propose_sprout(
                &editable,
                &obs_window,
                &diag,
                &growth_config,
                &economy_config,
                &mut budget,
                0,
            )
            .unwrap();
            assert!(
                sprout1.is_some(),
                "Task {name}: expected Sprout 1 to be found"
            );
            let staged1 = sprout1.unwrap();
            let old_to_new1: Vec<Option<NodeId>> = (0..editable.circuit().node_count())
                .map(|i| Some(NodeId(i as u32)))
                .collect();
            editable
                .recompile_preserving_state(staged1, &old_to_new1)
                .unwrap();
            diag.clear();

            // Stage 1: 128 ticks (64 settling + 64 diagnostic) with Sprout 1
            let mut e1 = 0usize;
            for t in 0..128 {
                let truth = period_seq[t % p_len];
                let mut pred = [0u64];
                let compiled = editable.compiled().clone();
                editable
                    .state_mut()
                    .step(&compiled, &[prev_sym], &mut pred)
                    .unwrap();
                let residual = width.sub(truth, pred[0]);
                if t >= 64 && pred[0] != truth {
                    e1 += 1;
                }
                obs_window.push_frame(&[truth]);
                let node_outs = node_output_values(editable.circuit(), editable.state());
                if t >= 64 {
                    diag.push_frame(
                        &[prev_sym],
                        &[truth],
                        &pred,
                        &[residual],
                        &node_outs,
                        &vec![true; node_outs.len()],
                    );
                }
                let _ = editable
                    .economy_mut()
                    .port_scorer_mut(0)
                    .observe(pred[0], truth);
                for node in editable.economy_mut().nodes_mut() {
                    if node.juvenile_ticks > 0 {
                        node.juvenile_ticks -= 1;
                    }
                }
                prev_sym = truth;
            }
            assert!(e1 < e0, "Task {name}: expected e1 ({e1}) < e0 ({e0})");

            // Propose Sprout 2 (Stacked corrector)
            let sprout2 = propose_sprout(
                &editable,
                &obs_window,
                &diag,
                &growth_config,
                &economy_config,
                &mut budget,
                0,
            )
            .unwrap();
            assert!(
                sprout2.is_some(),
                "Task {name}: expected Sprout 2 to be found"
            );
            let staged2 = sprout2.unwrap();
            let old_to_new2: Vec<Option<NodeId>> = (0..editable.circuit().node_count())
                .map(|i| Some(NodeId(i as u32)))
                .collect();
            editable
                .recompile_preserving_state(staged2, &old_to_new2)
                .unwrap();
            diag.clear();

            // Stage 2: 128 ticks (64 settling + 64 evaluation) with Sprout 2
            let mut e2 = 0usize;
            for t in 0..128 {
                let truth = period_seq[t % p_len];
                let mut pred = [0u64];
                let compiled = editable.compiled().clone();
                editable
                    .state_mut()
                    .step(&compiled, &[prev_sym], &mut pred)
                    .unwrap();
                if t >= 64 && pred[0] != truth {
                    e2 += 1;
                }
                prev_sym = truth;
            }
            assert_eq!(e2, 0, "Task {name}: expected 0 errors in Stage 2, got {e2}");
        }
    }

    #[test]
    fn k64_width_safety_diagnose_and_materialize_without_overflow() {
        let w64 = Width::new(64).unwrap();
        let config = GrowthConfig {
            min_match_gain: 2,
            mi_threshold_bits: 0.01,
            ..GrowthConfig::default()
        };
        let residuals = vec![0, u64::MAX, 0, u64::MAX, 0, u64::MAX, 0, u64::MAX];
        let features = vec![ResidualFeature {
            name: "in:0".to_owned(),
            values: vec![1, 0, 1, 0, 1, 0, 1, 0],
        }];
        let diag = diagnose_residual(w64, &residuals, &features, config).unwrap();
        assert!(diag.growth_criterion_met);
        let best = diag.best_feature.unwrap();
        assert!(best.corrected_mutual_information_bits.is_finite());
        assert!(best.corrected_mutual_information_bits > 0.0);

        let cands = generate_candidate_motifs(
            &SourceFeature::BoundaryIn(0),
            None,
            w64,
            Some((u64::MAX, 4)),
            false,
            1,
        );
        assert!(cands.contains(&CandidateMotif::ScaleEqz(
            SourceFeature::BoundaryIn(0),
            u64::MAX
        )));

        let mut circuit = Circuit::new(w64, 1, 1, WireDelayMode::Combinational);
        materialize_candidate(
            &mut circuit,
            &CandidateMotif::Identity(SourceFeature::BoundaryIn(0)),
            0,
        );
        materialize_candidate(
            &mut circuit,
            &CandidateMotif::ScaleEqz(SourceFeature::BoundaryIn(0), u64::MAX),
            0,
        );
        assert!(circuit.compile().is_ok());
    }

    #[test]
    fn search_bound_telemetry_detects_lag_horizon_candidate_cap_and_ring_period() {
        // 1. LagHorizonBound: port 1 depends on BoundaryIn(0) at lag tau = 5 (> max_reactive_lag = 3).
        let mut lag_obs = ObservationWindow::new(2, 192);
        let mut diag = DiagnosticBuffer::new(64);
        let mut hist = Vec::new();
        let mut state_rng = 0x1234_5678_9abc_def0u64;
        for t in 0..128usize {
            state_rng = state_rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let x = (state_rng >> 62) % 4;
            hist.push(x);
            let truth1 = if t >= 6 { hist[t - 6] } else { 0 };
            let frame = [x, truth1];
            let prev = if t == 0 {
                [0, 0]
            } else {
                [hist[t - 1], if t >= 7 { hist[t - 7] } else { 0 }]
            };
            lag_obs.push_frame(&frame);
            if t >= 64 {
                diag.push_frame(&prev, &frame, &[0, 0], &frame, &[], &[]);
            }
        }
        let circuit2 = Circuit::new(Width::BITS_2, 2, 2, WireDelayMode::Combinational);
        let circuit = Circuit::new(Width::BITS_2, 1, 1, WireDelayMode::Combinational);
        let econ = EconomyConfig::default();
        let growth_lag = GrowthConfig {
            diagnostic_window: 64,
            min_match_gain: 4,
            mi_threshold_bits: 0.05,
            max_reactive_lag: 3,
            ..GrowthConfig::default()
        };
        let editable_lag = EditableCircuit::new(circuit2.clone(), econ).unwrap();
        let mut budget = TickBudget::new(100_000);
        let mut tel_lag = SproutTelemetry::default();
        let prop_lag = propose_sprout_with_telemetry(
            &editable_lag,
            &lag_obs,
            &diag,
            &growth_lag,
            &econ,
            &mut budget,
            1,
            growth_lag.max_reactive_lag,
            &mut tel_lag,
        )
        .unwrap();
        assert!(prop_lag.is_none());
        assert_eq!(tel_lag.lag_horizon_bound, 1);

        // Uncorrelated noise residual must NOT emit LagHorizonBound.
        use crate::rng::StreamDomain;
        use rand_core::Rng;
        let mut rng_in = SeedBank::new(101).stream(StreamDomain::GeneratorInit);
        let mut rng_truth = SeedBank::new(202).stream(StreamDomain::ChannelNoise { channel: 0 });
        let mut noise_diag = DiagnosticBuffer::new(64);
        let mut noise_obs = ObservationWindow::new(1, 192);
        let mut prev_in = 0u64;
        for t in 0..128u64 {
            let in_sym = rng_in.next_u64() % 4;
            let truth = rng_truth.next_u64() % 4;
            noise_obs.push_frame(&[truth]);
            if t >= 64 {
                noise_diag.push_frame(&[prev_in], &[truth], &[0], &[truth], &[], &[]);
            }
            prev_in = in_sym;
        }
        let editable = EditableCircuit::new(circuit.clone(), econ).unwrap();
        let mut tel_noise = SproutTelemetry::default();
        let prop_noise = propose_sprout_with_telemetry(
            &editable,
            &noise_obs,
            &noise_diag,
            &growth_lag,
            &econ,
            &mut budget,
            0,
            growth_lag.max_reactive_lag,
            &mut tel_noise,
        )
        .unwrap();
        assert!(prop_noise.is_none());
        assert_eq!(tel_noise.lag_horizon_bound, 0);

        // CandidateListTruncated with multiple correlated inputs & small candidate_feature_cap.
        let mut cap_obs = ObservationWindow::new(2, 64);
        let mut cap_diag = DiagnosticBuffer::new(32);
        for t in 0..32u64 {
            let x = t % 4;
            cap_obs.push_frame(&[x, x]);
            cap_diag.push_frame(&[x, x], &[x, x], &[0, 0], &[x, x], &[], &[]);
        }
        let editable2 = EditableCircuit::new(circuit2, econ).unwrap();
        let growth_cap = GrowthConfig {
            diagnostic_window: 32,
            min_match_gain: 4,
            mi_threshold_bits: 0.05,
            candidate_feature_cap: 2,
            ..GrowthConfig::default()
        };
        let mut tel_cap = SproutTelemetry::default();
        let _ = propose_sprout_with_telemetry(
            &editable2,
            &cap_obs,
            &cap_diag,
            &growth_cap,
            &econ,
            &mut budget,
            0,
            growth_cap.max_reactive_lag,
            &mut tel_cap,
        )
        .unwrap();
        assert_eq!(tel_cap.candidate_list_truncated, 1);

        // RingPeriodBound: clean period-7 cycle exceeds max_ring_period = 6.
        let seq7 = [0u64, 1, 2, 3, 1, 2, 0];
        let econ_ring = EconomyConfig {
            initial_energy: 10.0,
            juvenile_ticks: 64,
            split_code_tax: true,
            ..EconomyConfig::default()
        };
        let growth_ring = GrowthConfig {
            diagnostic_window: 64,
            min_match_gain: 4,
            min_ring_period: 2,
            max_ring_period: 6,
            ..GrowthConfig::default()
        };
        let mut ring_obs = ObservationWindow::new(1, observation_horizon(&econ_ring, &growth_ring));
        for t in 0..128usize {
            let sym = seq7[t % 7];
            ring_obs.push_frame(&[sym]);
        }
        let editable_ring = EditableCircuit::new(circuit, econ_ring).unwrap();
        let mut tel_ring = SproutTelemetry::default();
        let prop_ring = propose_autonomous_delay_ring_with_telemetry(
            &editable_ring,
            &ring_obs,
            &growth_ring,
            &econ_ring,
            &mut budget,
            0,
            &mut tel_ring,
        );
        assert!(prop_ring.is_none());
        assert_eq!(tel_ring.ring_period_bound, 1);
    }

    #[test]
    fn b4_max_nodes_guardrail_counts_once_without_wasting_budget() {
        let mut obs = ObservationWindow::new(1, 32);
        let mut diag = DiagnosticBuffer::new(16);
        for t in 0..16u64 {
            let x = t % 4;
            let res = 1u64;
            obs.push_frame(&[x]);
            diag.push_frame(&[x], &[x], &[0], &[res], &[], &[]);
        }
        let econ = EconomyConfig::default();
        let mut one_node = Circuit::new(Width::BITS_2, 1, 1, WireDelayMode::Combinational);
        let tap = one_node.add_node(Primitive::Delay { init: 0 });
        one_node.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: tap, port: 0 },
        );
        one_node.connect(
            SignalSource::NodeOut { node: tap, port: 0 },
            SignalSink::BoundaryOut(0),
        );
        let editable_one = EditableCircuit::new(one_node, econ).unwrap();

        let growth = GrowthConfig {
            diagnostic_window: 16,
            min_match_gain: 4,
            mi_threshold_bits: 0.01,
            max_nodes: 1,
            ..GrowthConfig::default()
        };
        let initial_budget = 500u64;
        let mut budget = TickBudget::new(initial_budget);
        let mut tel = SproutTelemetry::default();
        let prop = propose_sprout_with_telemetry(
            &editable_one,
            &obs,
            &diag,
            &growth,
            &econ,
            &mut budget,
            0,
            growth.max_reactive_lag,
            &mut tel,
        )
        .unwrap();
        assert!(prop.is_none());
        assert_eq!(tel.sprout_blocked_max_nodes, 1);
        assert_eq!(budget.remaining(), initial_budget);

        // Ring proposal blocked by max_nodes also consumes zero replay budget and counts once per admissible mu period.
        let econ_ring = EconomyConfig {
            initial_energy: 10.0,
            juvenile_ticks: 32,
            ..EconomyConfig::default()
        };
        let growth_ring = GrowthConfig {
            diagnostic_window: 32,
            min_match_gain: 4,
            max_nodes: 2,
            min_ring_period: 2,
            max_ring_period: 4,
            ..GrowthConfig::default()
        };
        let mut ring_obs = ObservationWindow::new(1, observation_horizon(&econ_ring, &growth_ring));
        for t in 0..64u64 {
            ring_obs.push_frame(&[t % 2]);
        }
        let empty = Circuit::new(Width::BITS_2, 1, 1, WireDelayMode::Combinational);
        let editable_empty = EditableCircuit::new(empty, econ_ring).unwrap();
        let mut ring_budget = TickBudget::new(initial_budget);
        let mut tel_ring = SproutTelemetry::default();
        let ring_prop = propose_autonomous_delay_ring_with_telemetry(
            &editable_empty,
            &ring_obs,
            &growth_ring,
            &econ_ring,
            &mut ring_budget,
            0,
            &mut tel_ring,
        );
        assert!(ring_prop.is_none());
        // Periods 2, 3, 4 each yield a non-constant template and require p + 1 >= 3 nodes (> max_nodes = 2).
        assert_eq!(tel_ring.sprout_blocked_max_nodes, 3);
        assert_eq!(ring_budget.remaining(), initial_budget);
    }
}
