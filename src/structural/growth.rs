//! Finite-ring residual diagnostics for bounded corrector growth.

use std::collections::BTreeMap;

use crate::substrate::Width;

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
            .then_with(|| left.feature_indices.cmp(&right.feature_indices))
    });
    let best_feature = scores.into_iter().next();

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
    FeatureScore {
        feature_indices: indices.to_vec(),
        mutual_information_bits,
        corrected_mutual_information_bits,
        match_gain,
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
}
