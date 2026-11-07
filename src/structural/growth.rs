//! Finite-ring residual diagnostics for bounded corrector growth.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::baselines::{ObservationWindow, TickBudget};
use crate::metrics::{circuit_code_bits_with_tax, facts_code_bits};
use crate::structural::economy::{EconomyConfig, EconomyState};
use crate::structural::lattice::{Coord, LatticeCircuit, LatticeError, WiringTopology};
use crate::structural::surgery::EditableLattice;
use crate::substrate::{NodeId, Primitive, ScalarState, SignalSink, SignalSource, Width};

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
    /// Maximum number of live substrate nodes.
    pub max_nodes: usize,
}

impl Default for GrowthConfig {
    fn default() -> Self {
        Self {
            diagnostic_window: 64,
            sprout_interval: 32,
            mi_threshold_bits: 0.05,
            min_match_gain: 4,
            max_nodes: 32,
        }
    }
}

impl GrowthConfig {
    /// Returns whether all growth limits are usable.
    #[must_use]
    pub fn is_valid(self) -> bool {
        self.diagnostic_window >= 2
            && self.sprout_interval > 0
            && self.mi_threshold_bits.is_finite()
            && self.mi_threshold_bits >= 0.0
            && self.min_match_gain > 0
            && self.max_nodes > 0
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
    /// Mutual information between the residual and its lags 1 through 3.
    pub lagged_mutual_information_bits: Vec<f64>,
    /// Best singleton or pair candidate, selected deterministically.
    pub best_feature: Option<FeatureScore>,
    /// Secondary candidate (second index of pair, or highest-scoring distinct singleton).
    pub secondary_feature: Option<FeatureScore>,
    /// Top candidate feature groups clearing the growth guards.
    pub candidate_features: Vec<FeatureScore>,
    /// Whether a non-zero constant offset or feature clears the growth guards.
    pub growth_criterion_met: bool,
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

    let mut lagged_mutual_information_bits = Vec::with_capacity(3);
    for lag in 1..=3 {
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
    let candidate_features: Vec<FeatureScore> = scores
        .iter()
        .filter(|s| {
            s.corrected_mutual_information_bits > config.mi_threshold_bits
                && s.match_gain >= config.min_match_gain
        })
        .take(6)
        .cloned()
        .collect();

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
    let residual_alphabet_size = 2.0f64.powi(i32::from(width.bits()));
    let correction = (alphabet_size.saturating_sub(1) as f64) * (residual_alphabet_size - 1.0)
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
    pub fn build_signal_pool(
        &self,
        lattice: &LatticeCircuit,
        obs_window: &ObservationWindow,
        max_relay_hops: u8,
        out_port: u16,
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

        // 2. Live internal node outputs within spatial routing reach
        let out_cell = lattice
            .output_anchors()
            .get(usize::from(out_port))
            .and_then(|a| a.adjacent_border_cell(lattice.shape()).ok());

        for (node_idx, &(_, primitive)) in lattice.nodes().iter().enumerate() {
            let node = NodeId(u32::try_from(node_idx).expect("fits in u32"));
            if primitive == Primitive::Split {
                let consumers = lattice
                    .edges()
                    .iter()
                    .filter(
                        |&(s, _)| matches!(s, SignalSource::NodeOut { node: n, .. } if *n == node),
                    )
                    .count();
                if consumers <= 1 {
                    continue;
                }
            }
            if let Some(out_cell) = out_cell
                && let Some(coord) = lattice.node_coord(node)
            {
                let in_reach = match lattice.topology() {
                    WiringTopology::LocalVonNeumann => {
                        usize::from(coord.manhattan_distance(out_cell))
                            <= usize::from(max_relay_hops) + 1
                    }
                    WiringTopology::LocalMoore => {
                        usize::from(coord.chebyshev_distance(out_cell))
                            <= usize::from(max_relay_hops) + 1
                    }
                    WiringTopology::WellMixed => true,
                };
                if !in_reach {
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

        // 3. Virtual causal z^-tau delay taps (tau in 1..=3)
        let window_len = obs_window.len();
        let start_in_window = window_len.saturating_sub(len);

        for tau in 1..=3u8 {
            for c in 0..num_channels {
                let mut values = Vec::with_capacity(len);
                for t in 0..len {
                    let win_idx = start_in_window + t;
                    let lag_t = (win_idx as isize) - 1 - (tau as isize);
                    let val = if lag_t >= 0 {
                        obs_window.frames().get(lag_t as usize).map_or(0, |f| f[c])
                    } else if lag_t == -1 {
                        obs_window.prior_frame().get(c).copied().unwrap_or(0)
                    } else {
                        0
                    };
                    values.push(val);
                }
                features.push(ResidualFeature {
                    name: SourceFeature::LagIn {
                        channel: c as u16,
                        tau,
                    }
                    .name(),
                    values,
                });
            }

            let is_driven = lattice
                .edges()
                .iter()
                .any(|&(_, dst)| dst == SignalSink::BoundaryOut(out_port));

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

/// Generates candidate motifs in canonical order from primary/secondary features (§4.3).
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
    let max_c = (1u64 << width.bits()).saturating_sub(1).min(3);
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

fn source_coord(lattice: &LatticeCircuit, src: SignalSource) -> Result<Coord, LatticeError> {
    match src {
        SignalSource::BoundaryIn(c) => {
            if let Some(anchor) = lattice.input_anchors().get(usize::from(c)) {
                anchor.adjacent_border_cell(lattice.shape())
            } else {
                Ok(Coord { row: 0, col: 0 })
            }
        }
        SignalSource::NodeOut { node, .. } => lattice
            .node_coord(node)
            .ok_or(LatticeError::UnknownEndpoint),
    }
}

fn target_out_cell(lattice: &LatticeCircuit, out_port: u16) -> Option<Coord> {
    if lattice.topology() == WiringTopology::WellMixed {
        None
    } else {
        lattice
            .output_anchors()
            .get(usize::from(out_port))
            .and_then(|a| a.adjacent_border_cell(lattice.shape()).ok())
    }
}

fn materialize_source(
    lattice: &mut LatticeCircuit,
    source: &SourceFeature,
    target_cell: Option<Coord>,
    out_port: u16,
    max_relay_hops: u8,
) -> Result<SignalSource, LatticeError> {
    let out_cell = target_cell.or_else(|| target_out_cell(lattice, out_port));
    match source {
        SourceFeature::BoundaryIn(c) => Ok(SignalSource::BoundaryIn(*c)),
        SourceFeature::NodeOut { node, port } => Ok(SignalSource::NodeOut {
            node: *node,
            port: *port,
        }),
        SourceFeature::LagIn { channel, tau } => {
            let mut curr =
                lattice.tap_source(SignalSource::BoundaryIn(*channel), max_relay_hops)?;
            for _ in 0..*tau {
                let coord = source_coord(lattice, curr)?;
                let cell = lattice
                    .best_empty_cell_between(coord, out_cell, max_relay_hops)
                    .ok_or(LatticeError::UnknownEndpoint)?;
                let delay = lattice.place_node(cell, Primitive::Delay { init: 0 })?;
                lattice.route_connect(
                    curr,
                    SignalSink::NodeIn {
                        node: delay,
                        port: 0,
                    },
                    usize::from(max_relay_hops),
                )?;
                curr = SignalSource::NodeOut {
                    node: delay,
                    port: 0,
                };
            }
            Ok(curr)
        }
        SourceFeature::LagYhat { port, tau } => {
            let old_edge = lattice
                .edges()
                .iter()
                .find(|&(_, dst)| *dst == SignalSink::BoundaryOut(*port))
                .ok_or(LatticeError::UnknownEndpoint)?;
            let mut driver = old_edge.0;
            if let SignalSource::NodeOut { node, .. } = driver
                && lattice.node_primitive(node) == Some(Primitive::Split)
            {
                let consumers = lattice
                    .edges()
                    .iter()
                    .filter(
                        |&(s, _)| matches!(s, SignalSource::NodeOut { node: n, .. } if *n == node),
                    )
                    .count();
                if consumers == 1
                    && let Some(in_edge) = lattice
                        .edges()
                        .iter()
                        .find(|&(_, dst)| *dst == SignalSink::NodeIn { node, port: 0 })
                {
                    driver = in_edge.0;
                }
            }
            let mut curr = lattice.tap_source(driver, max_relay_hops)?;
            for _ in 0..*tau {
                let coord = source_coord(lattice, curr)?;
                let cell = lattice
                    .best_empty_cell_between(coord, out_cell, max_relay_hops)
                    .ok_or(LatticeError::UnknownEndpoint)?;
                let delay = lattice.place_node(cell, Primitive::Delay { init: 0 })?;
                lattice.route_connect(
                    curr,
                    SignalSink::NodeIn {
                        node: delay,
                        port: 0,
                    },
                    usize::from(max_relay_hops),
                )?;
                curr = SignalSource::NodeOut {
                    node: delay,
                    port: 0,
                };
            }
            Ok(curr)
        }
        SourceFeature::LagRes { port, tau } => {
            let s_tap = lattice.tap_source(SignalSource::BoundaryIn(*port), max_relay_hops)?;
            let old_edge = lattice
                .edges()
                .iter()
                .find(|&(_, dst)| *dst == SignalSink::BoundaryOut(*port))
                .ok_or(LatticeError::UnknownEndpoint)?;
            let mut driver = old_edge.0;
            if let SignalSource::NodeOut { node, .. } = driver
                && lattice.node_primitive(node) == Some(Primitive::Split)
            {
                let consumers = lattice
                    .edges()
                    .iter()
                    .filter(
                        |&(s, _)| matches!(s, SignalSource::NodeOut { node: n, .. } if *n == node),
                    )
                    .count();
                if consumers == 1
                    && let Some(in_edge) = lattice
                        .edges()
                        .iter()
                        .find(|&(_, dst)| *dst == SignalSink::NodeIn { node, port: 0 })
                {
                    driver = in_edge.0;
                }
            }
            let y_tap = lattice.tap_source(driver, max_relay_hops)?;
            let coord = source_coord(lattice, y_tap)?;
            let d_cell = lattice
                .best_empty_cell_between(coord, out_cell, max_relay_hops)
                .ok_or(LatticeError::UnknownEndpoint)?;
            let d_node = lattice.place_node(d_cell, Primitive::Delay { init: 0 })?;
            lattice.route_connect(
                y_tap,
                SignalSink::NodeIn {
                    node: d_node,
                    port: 0,
                },
                usize::from(max_relay_hops),
            )?;
            let y_lag1 = SignalSource::NodeOut {
                node: d_node,
                port: 0,
            };

            let sub_cell = lattice
                .best_empty_cell_between(d_cell, out_cell, max_relay_hops)
                .ok_or(LatticeError::UnknownEndpoint)?;
            let sub_node = lattice.place_node(sub_cell, Primitive::Sub)?;
            let s_tap2 = lattice.tap_source(s_tap, max_relay_hops)?;
            lattice.route_connect(
                s_tap2,
                SignalSink::NodeIn {
                    node: sub_node,
                    port: 0,
                },
                usize::from(max_relay_hops),
            )?;
            lattice.route_connect(
                y_lag1,
                SignalSink::NodeIn {
                    node: sub_node,
                    port: 1,
                },
                usize::from(max_relay_hops),
            )?;
            let mut curr = SignalSource::NodeOut {
                node: sub_node,
                port: 0,
            };

            for _ in 1..*tau {
                let coord = source_coord(lattice, curr)?;
                let cell = lattice
                    .best_empty_cell_between(coord, out_cell, max_relay_hops)
                    .ok_or(LatticeError::UnknownEndpoint)?;
                let delay = lattice.place_node(cell, Primitive::Delay { init: 0 })?;
                lattice.route_connect(
                    curr,
                    SignalSink::NodeIn {
                        node: delay,
                        port: 0,
                    },
                    usize::from(max_relay_hops),
                )?;
                curr = SignalSource::NodeOut {
                    node: delay,
                    port: 0,
                };
            }
            Ok(curr)
        }
    }
}

fn materialize_const_loop(
    lattice: &mut LatticeCircuit,
    c: u64,
    near: Option<Coord>,
    out_cell: Option<Coord>,
    max_relay_hops: u8,
) -> Result<SignalSource, LatticeError> {
    let forbidden = out_cell
        .map(|g| {
            let mut set: BTreeSet<Coord> = lattice.neighbors(g).into_iter().collect();
            set.insert(g);
            set
        })
        .unwrap_or_default();

    let start = near.or(out_cell).unwrap_or(Coord { row: 0, col: 0 });
    let mut queue = VecDeque::new();
    let mut visited = BTreeSet::new();
    visited.insert(start);
    queue.push_back(start);
    let mut cell0 = None;
    while let Some(current) = queue.pop_front() {
        if lattice.cell_node(current).is_none() && !forbidden.contains(&current) {
            cell0 = Some(current);
            break;
        }
        for next in lattice.neighbors(current) {
            if visited.insert(next) {
                queue.push_back(next);
            }
        }
    }
    let cell0 = cell0.ok_or(LatticeError::UnknownEndpoint)?;
    let split = lattice.place_node(cell0, Primitive::Split)?;
    let cell1 = lattice
        .nearest_empty_cell(cell0)
        .ok_or(LatticeError::UnknownEndpoint)?;
    let delay = lattice.place_node(cell1, Primitive::Delay { init: c })?;
    lattice.route_connect(
        SignalSource::NodeOut {
            node: delay,
            port: 0,
        },
        SignalSink::NodeIn {
            node: split,
            port: 0,
        },
        usize::from(max_relay_hops),
    )?;
    lattice.route_connect(
        SignalSource::NodeOut {
            node: split,
            port: 0,
        },
        SignalSink::NodeIn {
            node: delay,
            port: 0,
        },
        usize::from(max_relay_hops),
    )?;
    Ok(SignalSource::NodeOut {
        node: split,
        port: 1,
    })
}

fn feature_coord(lattice: &LatticeCircuit, u: &SourceFeature) -> Option<Coord> {
    match u {
        SourceFeature::BoundaryIn(c) => lattice
            .input_anchors()
            .get(usize::from(*c))
            .and_then(|a| a.adjacent_border_cell(lattice.shape()).ok()),
        SourceFeature::NodeOut { node, .. } => lattice.node_coord(*node),
        SourceFeature::LagIn { channel, .. } => lattice
            .input_anchors()
            .get(usize::from(*channel))
            .and_then(|a| a.adjacent_border_cell(lattice.shape()).ok()),
        SourceFeature::LagYhat { port, .. } | SourceFeature::LagRes { port, .. } => lattice
            .output_anchors()
            .get(usize::from(*port))
            .and_then(|a| a.adjacent_border_cell(lattice.shape()).ok()),
    }
}

fn motif_start_coord(lattice: &LatticeCircuit, motif: &CandidateMotif, fallback: Coord) -> Coord {
    match motif {
        CandidateMotif::Identity(u)
        | CandidateMotif::Eqz(u)
        | CandidateMotif::AddSelf(u)
        | CandidateMotif::AddConst(u, _)
        | CandidateMotif::EqzAddConst(u, _)
        | CandidateMotif::Neg(u)
        | CandidateMotif::ScaleEqz(u, _) => feature_coord(lattice, u).unwrap_or(fallback),
        CandidateMotif::AddPair(u1, u2)
        | CandidateMotif::SubPair(u1, u2)
        | CandidateMotif::MulPair(u1, u2)
        | CandidateMotif::AddEqz(u1, u2)
        | CandidateMotif::MulEqz(u1, u2)
        | CandidateMotif::MulEqzPair(u1, u2) => feature_coord(lattice, u1)
            .or_else(|| feature_coord(lattice, u2))
            .unwrap_or(fallback),
        CandidateMotif::ConstLoop(_) => fallback,
    }
}

fn materialize_motif(
    lattice: &mut LatticeCircuit,
    motif: &CandidateMotif,
    out_port: u16,
    max_relay_hops: u8,
) -> Result<SignalSource, LatticeError> {
    let out_cell = target_out_cell(lattice, out_port);
    let fallback_start = out_cell.unwrap_or(Coord { row: 0, col: 0 });
    let start_coord = motif_start_coord(lattice, motif, fallback_start);
    match motif {
        CandidateMotif::Identity(u) => {
            let src = materialize_source(lattice, u, out_cell, out_port, max_relay_hops)?;
            lattice.tap_source(src, max_relay_hops)
        }
        CandidateMotif::Eqz(u) => {
            let cell = lattice
                .best_empty_cell_between(start_coord, out_cell, max_relay_hops)
                .ok_or(LatticeError::UnknownEndpoint)?;
            let eqz = lattice.place_node(cell, Primitive::Eqz)?;
            let src = materialize_source(lattice, u, Some(cell), out_port, max_relay_hops)?;
            let tapped = lattice.tap_source(src, max_relay_hops)?;
            lattice.route_connect(
                tapped,
                SignalSink::NodeIn { node: eqz, port: 0 },
                usize::from(max_relay_hops),
            )?;
            Ok(SignalSource::NodeOut { node: eqz, port: 0 })
        }
        CandidateMotif::AddSelf(u) => {
            let add_cell = lattice
                .best_empty_cell_between(start_coord, out_cell, max_relay_hops)
                .ok_or(LatticeError::UnknownEndpoint)?;
            let add = lattice.place_node(add_cell, Primitive::Add)?;
            let split_cell = lattice
                .best_empty_cell_between(start_coord, Some(add_cell), max_relay_hops)
                .ok_or(LatticeError::UnknownEndpoint)?;
            let split = lattice.place_node(split_cell, Primitive::Split)?;
            let src = materialize_source(lattice, u, Some(split_cell), out_port, max_relay_hops)?;
            let tapped = lattice.tap_source(src, max_relay_hops)?;
            lattice.route_connect(
                tapped,
                SignalSink::NodeIn {
                    node: split,
                    port: 0,
                },
                usize::from(max_relay_hops),
            )?;
            lattice.route_connect(
                SignalSource::NodeOut {
                    node: split,
                    port: 0,
                },
                SignalSink::NodeIn { node: add, port: 0 },
                usize::from(max_relay_hops),
            )?;
            lattice.route_connect(
                SignalSource::NodeOut {
                    node: split,
                    port: 1,
                },
                SignalSink::NodeIn { node: add, port: 1 },
                usize::from(max_relay_hops),
            )?;
            Ok(SignalSource::NodeOut { node: add, port: 0 })
        }
        CandidateMotif::AddPair(u1, u2) => {
            let add_cell = lattice
                .best_empty_cell_between(start_coord, out_cell, max_relay_hops)
                .ok_or(LatticeError::UnknownEndpoint)?;
            let add = lattice.place_node(add_cell, Primitive::Add)?;
            let s1 = materialize_source(lattice, u1, Some(add_cell), out_port, max_relay_hops)?;
            let t1 = lattice.tap_source(s1, max_relay_hops)?;
            lattice.route_connect(
                t1,
                SignalSink::NodeIn { node: add, port: 0 },
                usize::from(max_relay_hops),
            )?;
            let s2 = materialize_source(lattice, u2, Some(add_cell), out_port, max_relay_hops)?;
            let t2 = lattice.tap_source(s2, max_relay_hops)?;
            lattice.route_connect(
                t2,
                SignalSink::NodeIn { node: add, port: 1 },
                usize::from(max_relay_hops),
            )?;
            Ok(SignalSource::NodeOut { node: add, port: 0 })
        }
        CandidateMotif::SubPair(u1, u2) => {
            let sub_cell = lattice
                .best_empty_cell_between(start_coord, out_cell, max_relay_hops)
                .ok_or(LatticeError::UnknownEndpoint)?;
            let sub = lattice.place_node(sub_cell, Primitive::Sub)?;
            let s1 = materialize_source(lattice, u1, Some(sub_cell), out_port, max_relay_hops)?;
            let t1 = lattice.tap_source(s1, max_relay_hops)?;
            lattice.route_connect(
                t1,
                SignalSink::NodeIn { node: sub, port: 0 },
                usize::from(max_relay_hops),
            )?;
            let s2 = materialize_source(lattice, u2, Some(sub_cell), out_port, max_relay_hops)?;
            let t2 = lattice.tap_source(s2, max_relay_hops)?;
            lattice.route_connect(
                t2,
                SignalSink::NodeIn { node: sub, port: 1 },
                usize::from(max_relay_hops),
            )?;
            Ok(SignalSource::NodeOut { node: sub, port: 0 })
        }
        CandidateMotif::MulPair(u1, u2) => {
            let mul_cell = lattice
                .best_empty_cell_between(start_coord, out_cell, max_relay_hops)
                .ok_or(LatticeError::UnknownEndpoint)?;
            let mul = lattice.place_node(mul_cell, Primitive::Mul)?;
            let s1 = materialize_source(lattice, u1, Some(mul_cell), out_port, max_relay_hops)?;
            let t1 = lattice.tap_source(s1, max_relay_hops)?;
            lattice.route_connect(
                t1,
                SignalSink::NodeIn { node: mul, port: 0 },
                usize::from(max_relay_hops),
            )?;
            let s2 = materialize_source(lattice, u2, Some(mul_cell), out_port, max_relay_hops)?;
            let t2 = lattice.tap_source(s2, max_relay_hops)?;
            lattice.route_connect(
                t2,
                SignalSink::NodeIn { node: mul, port: 1 },
                usize::from(max_relay_hops),
            )?;
            Ok(SignalSource::NodeOut { node: mul, port: 0 })
        }
        CandidateMotif::ConstLoop(c) => {
            materialize_const_loop(lattice, *c, out_cell, out_cell, max_relay_hops)
        }
        CandidateMotif::AddConst(u, c) => {
            let add_cell = lattice
                .best_empty_cell_between(start_coord, out_cell, max_relay_hops)
                .ok_or(LatticeError::UnknownEndpoint)?;
            let add = lattice.place_node(add_cell, Primitive::Add)?;
            let u_src = materialize_source(lattice, u, Some(add_cell), out_port, max_relay_hops)?;
            let u_tap = lattice.tap_source(u_src, max_relay_hops)?;
            lattice.route_connect(
                u_tap,
                SignalSink::NodeIn { node: add, port: 0 },
                usize::from(max_relay_hops),
            )?;
            let c_src =
                materialize_const_loop(lattice, *c, Some(add_cell), out_cell, max_relay_hops)?;
            lattice.route_connect(
                c_src,
                SignalSink::NodeIn { node: add, port: 1 },
                usize::from(max_relay_hops),
            )?;
            Ok(SignalSource::NodeOut { node: add, port: 0 })
        }
        CandidateMotif::EqzAddConst(u, c) => {
            let eqz_cell = lattice
                .best_empty_cell_between(start_coord, out_cell, max_relay_hops)
                .ok_or(LatticeError::UnknownEndpoint)?;
            let eqz = lattice.place_node(eqz_cell, Primitive::Eqz)?;
            let add_src = materialize_motif(
                lattice,
                &CandidateMotif::AddConst(u.clone(), *c),
                out_port,
                max_relay_hops,
            )?;
            lattice.route_connect(
                add_src,
                SignalSink::NodeIn { node: eqz, port: 0 },
                usize::from(max_relay_hops),
            )?;
            Ok(SignalSource::NodeOut { node: eqz, port: 0 })
        }
        CandidateMotif::Neg(u) => {
            if lattice.width().bits() > 1 {
                let mult = (1u64 << lattice.width().bits()) - 1;
                let mul_cell = lattice
                    .best_empty_cell_between(start_coord, out_cell, max_relay_hops)
                    .ok_or(LatticeError::UnknownEndpoint)?;
                let mul = lattice.place_node(mul_cell, Primitive::Mul)?;
                let u_src =
                    materialize_source(lattice, u, Some(mul_cell), out_port, max_relay_hops)?;
                let u_tap = lattice.tap_source(u_src, max_relay_hops)?;
                lattice.route_connect(
                    u_tap,
                    SignalSink::NodeIn { node: mul, port: 0 },
                    usize::from(max_relay_hops),
                )?;
                let c_src = materialize_const_loop(
                    lattice,
                    mult,
                    Some(mul_cell),
                    out_cell,
                    max_relay_hops,
                )?;
                lattice.route_connect(
                    c_src,
                    SignalSink::NodeIn { node: mul, port: 1 },
                    usize::from(max_relay_hops),
                )?;
                Ok(SignalSource::NodeOut { node: mul, port: 0 })
            } else {
                let sub_cell = lattice
                    .best_empty_cell_between(start_coord, out_cell, max_relay_hops)
                    .ok_or(LatticeError::UnknownEndpoint)?;
                let sub = lattice.place_node(sub_cell, Primitive::Sub)?;
                let u_src =
                    materialize_source(lattice, u, Some(sub_cell), out_port, max_relay_hops)?;
                let u_tap = lattice.tap_source(u_src, max_relay_hops)?;
                lattice.route_connect(
                    u_tap,
                    SignalSink::NodeIn { node: sub, port: 1 },
                    usize::from(max_relay_hops),
                )?;
                let zero_src =
                    materialize_const_loop(lattice, 0, Some(sub_cell), out_cell, max_relay_hops)?;
                lattice.route_connect(
                    zero_src,
                    SignalSink::NodeIn { node: sub, port: 0 },
                    usize::from(max_relay_hops),
                )?;
                Ok(SignalSource::NodeOut { node: sub, port: 0 })
            }
        }
        CandidateMotif::ScaleEqz(u, c) => {
            let mul_cell = lattice
                .best_empty_cell_between(start_coord, out_cell, max_relay_hops)
                .ok_or(LatticeError::UnknownEndpoint)?;
            let mul = lattice.place_node(mul_cell, Primitive::Mul)?;
            let eqz_cell = lattice
                .best_empty_cell_between(start_coord, Some(mul_cell), max_relay_hops)
                .ok_or(LatticeError::UnknownEndpoint)?;
            let eqz = lattice.place_node(eqz_cell, Primitive::Eqz)?;
            let u_src = materialize_source(lattice, u, Some(eqz_cell), out_port, max_relay_hops)?;
            let u_tap = lattice.tap_source(u_src, max_relay_hops)?;
            lattice.route_connect(
                u_tap,
                SignalSink::NodeIn { node: eqz, port: 0 },
                usize::from(max_relay_hops),
            )?;
            lattice.route_connect(
                SignalSource::NodeOut { node: eqz, port: 0 },
                SignalSink::NodeIn { node: mul, port: 0 },
                usize::from(max_relay_hops),
            )?;
            let c_src =
                materialize_const_loop(lattice, *c, Some(mul_cell), out_cell, max_relay_hops)?;
            lattice.route_connect(
                c_src,
                SignalSink::NodeIn { node: mul, port: 1 },
                usize::from(max_relay_hops),
            )?;
            Ok(SignalSource::NodeOut { node: mul, port: 0 })
        }
        CandidateMotif::AddEqz(u1, u2) => {
            let add_cell = lattice
                .best_empty_cell_between(start_coord, out_cell, max_relay_hops)
                .ok_or(LatticeError::UnknownEndpoint)?;
            let add = lattice.place_node(add_cell, Primitive::Add)?;
            let s1 = materialize_source(lattice, u1, Some(add_cell), out_port, max_relay_hops)?;
            let t1 = lattice.tap_source(s1, max_relay_hops)?;
            lattice.route_connect(
                t1,
                SignalSink::NodeIn { node: add, port: 0 },
                usize::from(max_relay_hops),
            )?;
            let eqz_cell = lattice
                .best_empty_cell_between(start_coord, Some(add_cell), max_relay_hops)
                .ok_or(LatticeError::UnknownEndpoint)?;
            let eqz = lattice.place_node(eqz_cell, Primitive::Eqz)?;
            let s2 = materialize_source(lattice, u2, Some(eqz_cell), out_port, max_relay_hops)?;
            let t2 = lattice.tap_source(s2, max_relay_hops)?;
            lattice.route_connect(
                t2,
                SignalSink::NodeIn { node: eqz, port: 0 },
                usize::from(max_relay_hops),
            )?;
            lattice.route_connect(
                SignalSource::NodeOut { node: eqz, port: 0 },
                SignalSink::NodeIn { node: add, port: 1 },
                usize::from(max_relay_hops),
            )?;
            Ok(SignalSource::NodeOut { node: add, port: 0 })
        }
        CandidateMotif::MulEqz(u1, u2) => {
            let mul_cell = lattice
                .best_empty_cell_between(start_coord, out_cell, max_relay_hops)
                .ok_or(LatticeError::UnknownEndpoint)?;
            let mul = lattice.place_node(mul_cell, Primitive::Mul)?;
            let s1 = materialize_source(lattice, u1, Some(mul_cell), out_port, max_relay_hops)?;
            let t1 = lattice.tap_source(s1, max_relay_hops)?;
            lattice.route_connect(
                t1,
                SignalSink::NodeIn { node: mul, port: 0 },
                usize::from(max_relay_hops),
            )?;
            let eqz_cell = lattice
                .best_empty_cell_between(start_coord, Some(mul_cell), max_relay_hops)
                .ok_or(LatticeError::UnknownEndpoint)?;
            let eqz = lattice.place_node(eqz_cell, Primitive::Eqz)?;
            let s2 = materialize_source(lattice, u2, Some(eqz_cell), out_port, max_relay_hops)?;
            let t2 = lattice.tap_source(s2, max_relay_hops)?;
            lattice.route_connect(
                t2,
                SignalSink::NodeIn { node: eqz, port: 0 },
                usize::from(max_relay_hops),
            )?;
            lattice.route_connect(
                SignalSource::NodeOut { node: eqz, port: 0 },
                SignalSink::NodeIn { node: mul, port: 1 },
                usize::from(max_relay_hops),
            )?;
            Ok(SignalSource::NodeOut { node: mul, port: 0 })
        }
        CandidateMotif::MulEqzPair(u1, u2) => {
            let mul_cell = lattice
                .best_empty_cell_between(start_coord, out_cell, max_relay_hops)
                .ok_or(LatticeError::UnknownEndpoint)?;
            let mul = lattice.place_node(mul_cell, Primitive::Mul)?;
            let eqz1_cell = lattice
                .best_empty_cell_between(start_coord, Some(mul_cell), max_relay_hops)
                .ok_or(LatticeError::UnknownEndpoint)?;
            let eqz1 = lattice.place_node(eqz1_cell, Primitive::Eqz)?;
            let s1 = materialize_source(lattice, u1, Some(eqz1_cell), out_port, max_relay_hops)?;
            let t1 = lattice.tap_source(s1, max_relay_hops)?;
            lattice.route_connect(
                t1,
                SignalSink::NodeIn {
                    node: eqz1,
                    port: 0,
                },
                usize::from(max_relay_hops),
            )?;
            lattice.route_connect(
                SignalSource::NodeOut {
                    node: eqz1,
                    port: 0,
                },
                SignalSink::NodeIn { node: mul, port: 0 },
                usize::from(max_relay_hops),
            )?;
            let eqz2_cell = lattice
                .best_empty_cell_between(start_coord, Some(mul_cell), max_relay_hops)
                .ok_or(LatticeError::UnknownEndpoint)?;
            let eqz2 = lattice.place_node(eqz2_cell, Primitive::Eqz)?;
            let s2 = materialize_source(lattice, u2, Some(eqz2_cell), out_port, max_relay_hops)?;
            let t2 = lattice.tap_source(s2, max_relay_hops)?;
            lattice.route_connect(
                t2,
                SignalSink::NodeIn {
                    node: eqz2,
                    port: 0,
                },
                usize::from(max_relay_hops),
            )?;
            lattice.route_connect(
                SignalSource::NodeOut {
                    node: eqz2,
                    port: 0,
                },
                SignalSink::NodeIn { node: mul, port: 1 },
                usize::from(max_relay_hops),
            )?;
            Ok(SignalSource::NodeOut { node: mul, port: 0 })
        }
    }
}

/// Materializes a candidate motif onto a staged lattice and stacks it onto `out_port`.
pub fn materialize_candidate(
    lattice: &mut LatticeCircuit,
    motif: &CandidateMotif,
    out_port: u16,
    max_relay_hops: u8,
) -> Result<NodeId, LatticeError> {
    let is_driven = lattice
        .edges()
        .iter()
        .any(|&(_, dst)| dst == SignalSink::BoundaryOut(out_port));
    match motif {
        CandidateMotif::Neg(u) if is_driven => {
            let out_cell = target_out_cell(lattice, out_port);
            let u_src = materialize_source(lattice, u, out_cell, out_port, max_relay_hops)?;
            lattice.stack_output_subtractor(out_port, u_src, max_relay_hops)
        }
        CandidateMotif::ScaleEqz(u, c)
            if is_driven && *c == (1u64 << lattice.width().bits()) - 1 =>
        {
            let out_cell = target_out_cell(lattice, out_port);
            let fallback_start = out_cell.unwrap_or(Coord { row: 0, col: 0 });
            let start_coord = motif_start_coord(lattice, motif, fallback_start);
            let eqz_cell = lattice
                .best_empty_cell_between(start_coord, out_cell, max_relay_hops)
                .ok_or(LatticeError::UnknownEndpoint)?;
            let eqz = lattice.place_node(eqz_cell, Primitive::Eqz)?;
            let u_src = materialize_source(lattice, u, Some(eqz_cell), out_port, max_relay_hops)?;
            let u_tap = lattice.tap_source(u_src, max_relay_hops)?;
            lattice.route_connect(
                u_tap,
                SignalSink::NodeIn { node: eqz, port: 0 },
                usize::from(max_relay_hops),
            )?;
            lattice.stack_output_subtractor(
                out_port,
                SignalSource::NodeOut { node: eqz, port: 0 },
                max_relay_hops,
            )
        }
        _ => {
            let motif_src = materialize_motif(lattice, motif, out_port, max_relay_hops)?;
            lattice.stack_output_adder(out_port, motif_src, max_relay_hops)
        }
    }
}

/// Evaluates candidates from $\mathcal{K}(u_1, u_2)$ on `obs_window` and returns the best staged lattice if admissible (§4.3).
#[allow(clippy::too_many_arguments)]
pub fn propose_sprout(
    editable: &EditableLattice,
    obs_window: &ObservationWindow,
    diag: &DiagnosticBuffer,
    growth_config: &GrowthConfig,
    economy_config: &EconomyConfig,
    budget: &mut TickBudget,
    out_port: u16,
    max_relay_hops: u8,
) -> Result<Option<LatticeCircuit>, GrowthError> {
    let len = diag.len();
    if len < growth_config.diagnostic_window {
        return Ok(None);
    }
    let residuals: Vec<u64> = diag
        .residuals
        .iter()
        .map(|f| f[usize::from(out_port)])
        .collect();
    let features = diag.build_signal_pool(editable.lattice(), obs_window, max_relay_hops, out_port);

    let is_epoch_1 = !editable
        .lattice()
        .edges()
        .iter()
        .any(|&(_, dst)| dst == SignalSink::BoundaryOut(out_port));

    let diagnostics = diagnose_residual(
        editable.lattice().width(),
        &residuals,
        &features,
        *growth_config,
    )?;
    if diagnostics.zero_error || !diagnostics.growth_criterion_met {
        return Ok(None);
    }

    let mut feature_pairs: Vec<(SourceFeature, Option<SourceFeature>)> = Vec::new();
    if let Some(ref best) = diagnostics.best_feature {
        let u1_name = &features[best.feature_indices[0]].name;
        if let Some(u1) = SourceFeature::parse(u1_name) {
            let u2 = if best.feature_indices.len() > 1 {
                let u2_name = &features[best.feature_indices[1]].name;
                SourceFeature::parse(u2_name)
            } else {
                diagnostics.secondary_feature.as_ref().and_then(|sec| {
                    let u2_name = &features[sec.feature_indices[0]].name;
                    SourceFeature::parse(u2_name)
                })
            };
            feature_pairs.push((u1, u2));
        }
    } else if let Some((_, _)) = diagnostics.constant_offset {
        feature_pairs.push((SourceFeature::BoundaryIn(0), None));
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

    if feature_pairs.is_empty() {
        return Ok(None);
    }

    let num_channels = editable.circuit().num_inputs();
    let mut candidates = Vec::new();
    for (u1_feat, u2_feat) in &feature_pairs {
        let cands = generate_candidate_motifs(
            u1_feat,
            u2_feat.as_ref(),
            editable.lattice().width(),
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

    let w_diag = growth_config.diagnostic_window;
    let w_replay = obs_window.len().min(2 * w_diag);
    let start_idx = obs_window.len() - w_replay;
    let eval_start_idx = obs_window.len() - w_diag;

    let mut curr_state = ScalarState::new(editable.compiled());
    let mut curr_pred = vec![0u64; usize::from(editable.compiled().num_outputs())];
    let mut curr_scorer = editable.economy().scorers()[usize::from(out_port)].clone();
    curr_scorer.reset_predictor_counts();

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
        if idx >= eval_start_idx {
            let p_curr = if is_epoch_1 {
                0
            } else {
                curr_pred[usize::from(out_port)]
            };
            let p_truth = truth[usize::from(out_port)];
            if p_curr != p_truth {
                e_curr += 1;
            }
            if !is_epoch_1 {
                flux_curr += curr_scorer.observe(p_curr, p_truth);
            } else {
                curr_scorer.null_model_mut().observe_and_surprisal(p_truth);
            }
        }
    }

    let shape = if editable.lattice().topology() == WiringTopology::WellMixed {
        None
    } else {
        Some(editable.lattice().shape())
    };
    let l_curr = circuit_code_bits_with_tax(
        editable.circuit(),
        editable.lattice().topology(),
        shape,
        0,
        economy_config.split_code_tax,
    );
    let j_curr = l_curr + facts_code_bits(editable.lattice().width(), w_diag as u64, e_curr as u64);

    let mut best_proposal: Option<(LatticeCircuit, usize, f64, f64, bool)> = None;

    let is_constant_stream = !obs_window.frames().is_empty()
        && obs_window
            .frames()
            .iter()
            .all(|f| f[usize::from(out_port)] == obs_window.frames()[0][usize::from(out_port)]);

    for motif in &candidates {
        if !budget.try_consume(w_replay as u64) {
            break;
        }
        let mut staged = editable.lattice().clone();
        if staged.nodes().len() >= growth_config.max_nodes {
            continue;
        }
        if materialize_candidate(&mut staged, motif, out_port, max_relay_hops).is_err() {
            continue;
        }
        if staged.nodes().len() > growth_config.max_nodes {
            continue;
        }
        let Ok(staged_circuit) = staged.lower() else {
            continue;
        };
        let Ok(staged_compiled) = staged_circuit.compile() else {
            continue;
        };

        let mut state = ScalarState::new(&staged_compiled);
        let mut pred = vec![0u64; usize::from(staged_compiled.num_outputs())];
        let mut scorer = editable.economy().scorers()[usize::from(out_port)].clone();
        scorer.reset_predictor_counts();

        let mut e_cand = 0usize;
        let mut flux_cand = 0.0;
        let mut replay_ok = true;

        for i in 0..w_replay {
            let idx = start_idx + i;
            let input = if idx == 0 {
                obs_window.prior_frame()
            } else {
                &obs_window.frames()[idx - 1]
            };
            let truth = &obs_window.frames()[idx];
            if state.step(&staged_compiled, input, &mut pred).is_err() {
                replay_ok = false;
                break;
            }
            let p_cand = pred[usize::from(out_port)];
            let p_truth = truth[usize::from(out_port)];
            let reward = scorer.observe(p_cand, p_truth);
            if idx >= eval_start_idx {
                if p_cand != p_truth {
                    e_cand += 1;
                }
                flux_cand += reward;
            }
        }
        if !replay_ok {
            continue;
        }

        if e_cand + growth_config.min_match_gain > e_curr {
            continue;
        }
        let delta_flux = flux_cand - flux_curr;
        let staged_shape = if staged.topology() == WiringTopology::WellMixed {
            None
        } else {
            Some(staged.shape())
        };
        let l_cand = circuit_code_bits_with_tax(
            &staged_circuit,
            staged.topology(),
            staged_shape,
            0,
            economy_config.split_code_tax,
        );
        let delta_l_sprout = (l_cand - l_curr).max(0.0);
        let is_1_op = is_epoch_1
            && matches!(
                motif,
                CandidateMotif::Identity(SourceFeature::BoundaryIn(_))
                    | CandidateMotif::Eqz(SourceFeature::BoundaryIn(_))
            );

        if delta_flux <= 0.0 && !(is_1_op && !is_constant_stream) {
            continue;
        }

        if !is_1_op && economy_config.juvenile_ticks > 0 && delta_l_sprout > 0.0 {
            let projected_survival = economy_config.initial_energy
                + (economy_config.juvenile_ticks as f64 / w_diag as f64) * flux_cand;
            if projected_survival <= delta_l_sprout {
                continue;
            }
        }

        let j_cand = l_cand + facts_code_bits(staged.width(), w_diag as u64, e_cand as u64);

        if !is_epoch_1 && j_cand >= j_curr && e_cand > 0 {
            continue;
        }

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
                            || ((j_cand - *prev_j).abs() < 1e-9 && delta_flux > *prev_flux)
                    }
                } else {
                    j_cand < *prev_j || ((j_cand - *prev_j).abs() < 1e-9 && delta_flux > *prev_flux)
                }
            }
        };

        if is_better {
            best_proposal = Some((staged, e_cand, j_cand, delta_flux, is_1_op));
        }
    }

    Ok(best_proposal.map(|(lat, _, _, _, _)| lat))
}

/// Autonomous delay ring proposal under channel noise (§4.5).
#[derive(Clone, Debug, PartialEq)]
pub struct AutonomousRingProposal {
    /// Staged lattice circuit containing the autonomous delay ring.
    pub lattice: LatticeCircuit,
    /// Period of the delay ring.
    pub period: usize,
    /// Learned clean symbol template for the cycle.
    pub template: Vec<u64>,
}

fn build_autonomous_ring_lattice(
    base: &LatticeCircuit,
    period: usize,
    mu: &[u64],
    out_port: u16,
    max_relay_hops: u8,
) -> Result<LatticeCircuit, LatticeError> {
    let mut staged = LatticeCircuit::new(
        base.shape(),
        base.width(),
        base.wire_delay(),
        base.topology(),
        base.input_anchors().to_vec(),
        base.output_anchors().to_vec(),
    )?;

    if staged.topology() == WiringTopology::WellMixed {
        let mut delays = Vec::with_capacity(period);
        for j in 0..period {
            let init_val = mu[(period - 1 - j) % period];
            let cell = staged
                .nearest_empty_cell(Coord { row: 0, col: 0 })
                .ok_or(LatticeError::UnknownEndpoint)?;
            let d = staged.place_node(cell, Primitive::Delay { init: init_val })?;
            delays.push(d);
        }
        let s_cell = staged
            .nearest_empty_cell(Coord { row: 0, col: 0 })
            .ok_or(LatticeError::UnknownEndpoint)?;
        let split = staged.place_node(s_cell, Primitive::Split)?;

        for i in 0..(period - 1) {
            staged.connect(
                SignalSource::NodeOut {
                    node: delays[i],
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: delays[i + 1],
                    port: 0,
                },
            )?;
        }
        staged.connect(
            SignalSource::NodeOut {
                node: delays[period - 1],
                port: 0,
            },
            SignalSink::NodeIn {
                node: split,
                port: 0,
            },
        )?;
        staged.connect(
            SignalSource::NodeOut {
                node: split,
                port: 0,
            },
            SignalSink::NodeIn {
                node: delays[0],
                port: 0,
            },
        )?;
        staged.connect(
            SignalSource::NodeOut {
                node: split,
                port: 1,
            },
            SignalSink::BoundaryOut(out_port),
        )?;
        return Ok(staged);
    }

    let anchor = *staged
        .output_anchors()
        .get(usize::from(out_port))
        .ok_or(LatticeError::UnknownEndpoint)?;
    let out_cell = anchor.adjacent_border_cell(staged.shape())?;

    let split = staged.place_node(out_cell, Primitive::Split)?;
    staged.route_connect(
        SignalSource::NodeOut {
            node: split,
            port: 1,
        },
        SignalSink::BoundaryOut(out_port),
        usize::from(max_relay_hops),
    )?;

    let mut delays = Vec::with_capacity(period);
    let mut prev_cell = out_cell;
    for j in 0..period {
        let init_val = mu[(period - 1 - j) % period];
        let cell = staged
            .nearest_empty_cell(prev_cell)
            .ok_or(LatticeError::UnknownEndpoint)?;
        let d = staged.place_node(cell, Primitive::Delay { init: init_val })?;
        delays.push(d);
        prev_cell = cell;
    }
    for i in 0..(period - 1) {
        staged.route_connect(
            SignalSource::NodeOut {
                node: delays[i],
                port: 0,
            },
            SignalSink::NodeIn {
                node: delays[i + 1],
                port: 0,
            },
            usize::from(max_relay_hops),
        )?;
    }
    staged.route_connect(
        SignalSource::NodeOut {
            node: delays[period - 1],
            port: 0,
        },
        SignalSink::NodeIn {
            node: split,
            port: 0,
        },
        usize::from(max_relay_hops),
    )?;
    staged.route_connect(
        SignalSource::NodeOut {
            node: split,
            port: 0,
        },
        SignalSink::NodeIn {
            node: delays[0],
            port: 0,
        },
        usize::from(max_relay_hops),
    )?;

    Ok(staged)
}

/// Evaluates autonomous delay-ring candidates of period $p \in 2..=6$ under channel noise (§4.5).
pub fn propose_autonomous_delay_ring(
    editable: &EditableLattice,
    obs_window: &ObservationWindow,
    growth_config: &GrowthConfig,
    economy_config: &EconomyConfig,
    budget: &mut TickBudget,
    out_port: u16,
    max_relay_hops: u8,
) -> Option<AutonomousRingProposal> {
    if !editable.compiled().uses_boundary_inputs() {
        return None;
    }
    let w_diag = growth_config.diagnostic_window;
    if obs_window.len() < 2 * w_diag {
        return None;
    }

    let w_replay = obs_window.len();
    let out_idx = usize::from(out_port);

    let mut curr_state = ScalarState::new(editable.compiled());
    let mut curr_pred = vec![0u64; usize::from(editable.compiled().num_outputs())];
    let mut curr_scorer = editable.economy().scorers()[out_idx].clone();
    curr_scorer.reset_predictor_counts();
    let mut e_curr = 0usize;
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
        if curr_pred[out_idx] != frame[out_idx] {
            e_curr += 1;
        }
        flux_curr += curr_scorer.observe(curr_pred[out_idx], frame[out_idx]);
    }
    let shape = if editable.lattice().topology() == WiringTopology::WellMixed {
        None
    } else {
        Some(editable.lattice().shape())
    };
    let l_curr = circuit_code_bits_with_tax(
        editable.circuit(),
        editable.lattice().topology(),
        shape,
        0,
        economy_config.split_code_tax,
    );
    let j_curr =
        l_curr + facts_code_bits(editable.lattice().width(), w_replay as u64, e_curr as u64);

    let mut best_ring: Option<(AutonomousRingProposal, f64)> = None;

    for p in 2..=6usize {
        if p + 1 > growth_config.max_nodes {
            continue;
        }
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
            continue;
        }

        if !budget.try_consume(w_replay as u64) {
            break;
        }

        let Ok(staged) =
            build_autonomous_ring_lattice(editable.lattice(), p, &mu, out_port, max_relay_hops)
        else {
            continue;
        };
        let Ok(staged_circuit) = staged.lower() else {
            continue;
        };
        let Ok(staged_compiled) = staged_circuit.compile() else {
            continue;
        };

        let mut state = ScalarState::new(&staged_compiled);
        let start_tick = obs_window.start_tick();
        let total_regs = staged_compiled.total_registers();
        if total_regs != p {
            continue;
        }
        let mut regs = state.registers().to_vec();
        for j in 0..p {
            let val = mu[((start_tick as usize) + (p - 1 - j)) % p];
            regs[j] = val;
        }
        if state.set_registers(&staged_compiled, &regs).is_err() {
            continue;
        }

        let mut scorer = editable.economy().scorers()[out_idx].clone();
        scorer.reset_predictor_counts();
        let mut e_ring = 0usize;
        let mut flux_ring = 0.0;
        let mut pred = vec![0u64; usize::from(staged_compiled.num_outputs())];
        let zeros = vec![0u64; usize::from(staged_compiled.num_inputs())];

        let mut replay_ok = true;
        for frame in obs_window.frames() {
            if state.step(&staged_compiled, &zeros, &mut pred).is_err() {
                replay_ok = false;
                break;
            }
            if pred[out_idx] != frame[out_idx] {
                e_ring += 1;
            }
            flux_ring += scorer.observe(pred[out_idx], frame[out_idx]);
        }
        if !replay_ok {
            continue;
        }

        if e_ring + growth_config.min_match_gain > e_curr {
            continue;
        }
        if flux_ring <= flux_curr {
            continue;
        }
        let ring_shape = if staged.topology() == WiringTopology::WellMixed {
            None
        } else {
            Some(staged.shape())
        };
        let l_ring = circuit_code_bits_with_tax(
            &staged_circuit,
            staged.topology(),
            ring_shape,
            0,
            economy_config.split_code_tax,
        );
        let j_ring = l_ring + facts_code_bits(staged.width(), w_replay as u64, e_ring as u64);
        if j_ring >= j_curr {
            continue;
        }

        if best_ring
            .as_ref()
            .is_none_or(|(_, best_j)| j_ring < *best_j)
        {
            best_ring = Some((
                AutonomousRingProposal {
                    lattice: staged,
                    period: p,
                    template: mu,
                },
                j_ring,
            ));
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
        use crate::structural::lattice::{BoundaryAnchor, GridShape, PerimeterSide};
        use crate::substrate::WireDelayMode;

        let shape = GridShape::new(8, 8).unwrap();
        let lattice = LatticeCircuit::new(
            shape,
            Width::BITS_2,
            WireDelayMode::Combinational,
            WiringTopology::WellMixed,
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

        // 1. Neg on unconnected port: -x mod 4
        let mut neg_lat = lattice.clone();
        materialize_candidate(
            &mut neg_lat,
            &CandidateMotif::Neg(SourceFeature::BoundaryIn(0)),
            0,
            4,
        )
        .unwrap();
        let circuit = neg_lat.lower().unwrap();
        let compiled = circuit.compile().unwrap();
        let mut state = ScalarState::new(&compiled);
        let mut out = [0u64];
        // For input = 1, -1 mod 4 = 3
        state.step(&compiled, &[1], &mut out).unwrap();
        assert_eq!(out[0], 3, "Neg(1) mod 4 must be 3, got {}", out[0]);

        // 2. ScaleEqz on unconnected port: 3 * Eqz(x) mod 4
        let mut scale_lat = lattice;
        materialize_candidate(
            &mut scale_lat,
            &CandidateMotif::ScaleEqz(SourceFeature::BoundaryIn(0), 3),
            0,
            4,
        )
        .unwrap();
        let circuit2 = scale_lat.lower().unwrap();
        let compiled2 = circuit2.compile().unwrap();
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
        use crate::structural::lattice::{BoundaryAnchor, GridShape, PerimeterSide};
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
            let shape = GridShape::new(8, 8).unwrap();
            let lattice = LatticeCircuit::new(
                shape,
                width,
                WireDelayMode::Combinational,
                WiringTopology::WellMixed,
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
            };

            let mut editable = EditableLattice::new(lattice, economy_config).unwrap();
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
                4,
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
                4,
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
}
