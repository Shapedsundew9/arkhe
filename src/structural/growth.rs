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

/// Derived causal-depth-scaled juvenile maturation horizon (`d2-A`, §4.3).
///
/// Reduces identically to `economy_config.juvenile_ticks` when `juvenile_ticks == 0` or
/// `d_cone <= max_reactive_lag`. For deeper register chains, compensates for `d_cone` ticks of
/// pipeline fill, retrograde discount $\gamma^{-(d_{\text{cone}} - \tau_{\max})}$, and chain-sum
/// normalization $\frac{1 - \gamma^{d_{\text{cone}}}}{1 - \gamma^{\tau_{\max}}}$.
#[must_use]
pub fn effective_juvenile_ticks(
    d_cone: usize,
    economy_config: &EconomyConfig,
    growth_config: &GrowthConfig,
) -> u32 {
    let tau_max = usize::from(growth_config.max_reactive_lag);
    if economy_config.juvenile_ticks == 0 || d_cone <= tau_max {
        return economy_config.juvenile_ticks;
    }
    let gamma = economy_config.trace_discount;
    let discount_factor = gamma.powi(-((d_cone - tau_max) as i32));
    let chain_sum_ratio = (1.0 - gamma.powi(d_cone as i32)) / (1.0 - gamma.powi(tau_max as i32));
    let scaled = (d_cone as f64)
        + (f64::from(economy_config.juvenile_ticks) * discount_factor * chain_sum_ratio).ceil();
    scaled.min(f64::from(u32::MAX)) as u32
}

/// Derives the coordinate-free per-circuit node guardrail scaled linearly by channel count.
///
/// Reduces identically to `growth_config.max_nodes` when `num_channels <= 1`. For multi-channel
/// circuits (`num_channels > 1`), guarantees at least `2 * deepening_max_lag + 8` nodes per
/// channel so concurrent ports can evaluate lag-deepened binary pairings without starving siblings.
#[must_use]
pub const fn effective_max_nodes(num_channels: u16, growth_config: &GrowthConfig) -> usize {
    if num_channels <= 1 {
        return growth_config.max_nodes;
    }
    let tau_deep = growth_config.deepening_max_lag as usize;
    let floor = 2 * tau_deep + 8;
    let per_channel = if growth_config.max_nodes > floor {
        growth_config.max_nodes
    } else {
        floor
    };
    (num_channels as usize).saturating_mul(per_channel)
}

fn port_cone_register_depth_rec(circuit: &Circuit, node: NodeId, on_path: &mut [bool]) -> usize {
    if on_path[node.index()] {
        return 0;
    }
    on_path[node.index()] = true;
    let self_reg = usize::from(matches!(
        circuit.nodes()[node.index()],
        Primitive::Delay { .. }
    ));
    let is_tph = circuit.wire_delay() == WireDelayMode::TicksPerHop;
    let mut max_upstream = 0usize;
    for edge in circuit.edges() {
        if let SignalSink::NodeIn { node: dst_node, .. } = edge.dst
            && dst_node == node
        {
            let hop_regs = if is_tph { usize::from(edge.hops) } else { 0 };
            let upstream = match edge.src {
                SignalSource::BoundaryIn(_) => hop_regs,
                SignalSource::NodeOut { node: src_node, .. } => {
                    if on_path[src_node.index()] {
                        0
                    } else {
                        hop_regs + port_cone_register_depth_rec(circuit, src_node, on_path)
                    }
                }
            };
            max_upstream = max_upstream.max(upstream);
        }
    }
    on_path[node.index()] = false;
    self_reg + max_upstream
}

/// Computes the maximum number of synchronous registers (`Primitive::Delay`, plus `edge.hops` in
/// `WireDelayMode::TicksPerHop`) along any simple path in `out_port`'s upstream cone.
#[must_use]
pub fn port_cone_register_depth(circuit: &Circuit, out_port: u16) -> usize {
    let is_tph = circuit.wire_delay() == WireDelayMode::TicksPerHop;
    let mut on_path = vec![false; circuit.node_count()];
    let mut max_depth = 0usize;
    for edge in circuit.edges() {
        if edge.dst == SignalSink::BoundaryOut(out_port) {
            let hop_regs = if is_tph { usize::from(edge.hops) } else { 0 };
            let depth = match edge.src {
                SignalSource::BoundaryIn(_) => hop_regs,
                SignalSource::NodeOut { node, .. } => {
                    hop_regs + port_cone_register_depth_rec(circuit, node, &mut on_path)
                }
            };
            max_depth = max_depth.max(depth);
        }
    }
    max_depth
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
    /// Pre-registered upper bound on progressive deepening lag search (default `12`).
    pub deepening_max_lag: u8,
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
            deepening_max_lag: 12,
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
            && self.deepening_max_lag >= self.max_reactive_lag
            && usize::from(self.deepening_max_lag) <= self.diagnostic_window / 2
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
    /// Count of progressive lag-deepening band evaluations entered.
    pub lag_deepening_steps: u64,
    /// Count of replay evaluation proposals stopped because `TickBudget` ran out.
    pub sprout_budget_exhausted: u64,
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

fn feature_lag(feature: &ResidualFeature) -> u8 {
    match SourceFeature::parse(&feature.name) {
        Some(
            SourceFeature::LagIn { tau, .. }
            | SourceFeature::LagYhat { tau, .. }
            | SourceFeature::LagRes { tau, .. },
        ) => tau,
        _ => 0,
    }
}

/// Computes plug-in information, Miller–Madow bias correction, and match gain for residuals.
pub fn diagnose_residual(
    width: Width,
    residuals: &[u64],
    features: &[ResidualFeature],
    config: GrowthConfig,
) -> Result<ResidualDiagnostics, GrowthError> {
    diagnose_residual_with_band(
        width,
        residuals,
        features,
        config,
        1,
        config.max_reactive_lag,
        0,
    )
}

fn diagnose_residual_with_band(
    width: Width,
    residuals: &[u64],
    features: &[ResidualFeature],
    config: GrowthConfig,
    min_tau: u8,
    max_tau: u8,
    out_port: u16,
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
    let res_ctx = ResidualContext::new(&residuals, width, config.min_match_gain);
    let residual_entropy_bits = res_ctx.entropy_bits;
    let constant_offset = res_ctx.constant_offset;

    let filter_band = min_tau > 1;
    let mut in_band_flags = vec![false; features.len()];
    if filter_band {
        for (idx, feat) in features.iter().enumerate() {
            let lag = feature_lag(feat);
            in_band_flags[idx] = lag >= min_tau
                && lag <= max_tau
                && SourceFeature::parse(&feat.name)
                    .is_some_and(|sf| source_is_cross_channel(&sf, out_port));
        }
    }

    let len = residuals.len();
    let mut norm_pool = vec![0u64; features.len() * len];
    for (idx, feat) in features.iter().enumerate() {
        let dst = &mut norm_pool[idx * len..(idx + 1) * len];
        for (slot, &val) in dst.iter_mut().zip(&feat.values) {
            *slot = width.normalize(val);
        }
    }

    let mut scratch_1: Vec<(u64, u64)> = Vec::new();
    let mut scratch_2: Vec<(u64, u64, u64)> = Vec::new();
    let mut scores = Vec::with_capacity(if filter_band {
        features.len() * 8
    } else {
        features.len() * (features.len() + 1) / 2
    });
    for first in 0..features.len() {
        let first_slice = &norm_pool[first * len..(first + 1) * len];
        let first_in_band = in_band_flags[first];
        if !filter_band || first_in_band {
            scores.push(score_singleton_normalized(
                &res_ctx,
                &residuals,
                first,
                first_slice,
                &mut scratch_1,
            ));
        }
        for second in (first + 1)..features.len() {
            if !filter_band || first_in_band || in_band_flags[second] {
                let second_slice = &norm_pool[second * len..(second + 1) * len];
                scores.push(score_pair_normalized(
                    &res_ctx,
                    &residuals,
                    first,
                    second,
                    first_slice,
                    second_slice,
                    &mut scratch_2,
                ));
            }
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
            let sub_ctx = ResidualContext::new(&residuals[lag..], width, config.min_match_gain);
            let sub = score_singleton_normalized(
                &sub_ctx,
                &residuals[lag..],
                0,
                &residuals[..count],
                &mut scratch_1,
            );
            lagged_mutual_information_bits.push(sub.mutual_information_bits);
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
        && ((min_tau <= 1 && constant_offset.is_some())
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

struct ResidualContext {
    width: Width,
    len: usize,
    n_f64: f64,
    alphabet_size: usize,
    entropy_bits: f64,
    constant_offset: Option<(u64, usize)>,
    small_p_res: [f64; 16],
    sorted_counts: Vec<(u64, usize)>,
}

impl ResidualContext {
    fn new(residuals: &[u64], width: Width, min_match_gain: usize) -> Self {
        let len = residuals.len();
        let n_f64 = len as f64;
        let bits = width.bits();
        let mut small_p_res = [0.0f64; 16];
        let mut sorted_counts: Vec<(u64, usize)>;
        let mut entropy_bits = 0.0f64;
        let mut alphabet_size = 0usize;
        let zero_count;
        let mut best_const: Option<(u64, usize)> = None;

        if bits <= 4 {
            let mut counts = [0usize; 16];
            let card = 1usize << bits;
            for &r in residuals {
                counts[r as usize] += 1;
            }
            zero_count = counts[0];
            sorted_counts = Vec::new();
            for (sym, &count) in counts.iter().enumerate().take(card) {
                if count > 0 {
                    alphabet_size += 1;
                    let prob = count as f64 / n_f64;
                    small_p_res[sym] = prob;
                    entropy_bits += -prob * prob.log2();
                    if sym != 0 && best_const.is_none_or(|(_, prev_cnt)| count > prev_cnt) {
                        best_const = Some((sym as u64, count));
                    }
                }
            }
        } else {
            let mut tmp = residuals.to_vec();
            tmp.sort_unstable();
            sorted_counts = Vec::with_capacity(tmp.len().min(64));
            let mut i = 0;
            let mut zc = 0usize;
            while i < tmp.len() {
                let sym = tmp[i];
                let start = i;
                while i < tmp.len() && tmp[i] == sym {
                    i += 1;
                }
                let count = i - start;
                sorted_counts.push((sym, count));
                alphabet_size += 1;
                let prob = count as f64 / n_f64;
                entropy_bits += -prob * prob.log2();
                if sym == 0 {
                    zc = count;
                } else if best_const.is_none_or(|(_, prev_cnt)| count > prev_cnt) {
                    best_const = Some((sym, count));
                }
            }
            zero_count = zc;
        }

        let constant_offset = best_const.and_then(|(symbol, count)| {
            let gain = count.saturating_sub(zero_count);
            (gain >= min_match_gain).then_some((symbol, gain))
        });

        Self {
            width,
            len,
            n_f64,
            alphabet_size,
            entropy_bits,
            constant_offset,
            small_p_res,
            sorted_counts,
        }
    }

    #[inline]
    fn res_p(&self, r: u64) -> f64 {
        if self.width.bits() <= 4 {
            self.small_p_res[r as usize]
        } else {
            let idx = self
                .sorted_counts
                .binary_search_by_key(&r, |(s, _)| *s)
                .expect("residual present in context");
            self.sorted_counts[idx].1 as f64 / self.n_f64
        }
    }
}

fn score_singleton_normalized(
    res_ctx: &ResidualContext,
    residuals: &[u64],
    feature_idx: usize,
    f0: &[u64],
    scratch: &mut Vec<(u64, u64)>,
) -> FeatureScore {
    let bits = res_ctx.width.bits();
    let n = res_ctx.n_f64;
    let mut information = 0.0f64;
    let mut alphabet_size = 0usize;
    let mut gain = 0usize;
    let mut direct_matches = 0usize;

    if bits <= 3 {
        let m = 1usize << bits;
        let mut feat = [0u32; 8];
        let mut joint = [0u32; 64];
        for (&v0, &r) in f0.iter().zip(residuals) {
            let vi = v0 as usize;
            let ri = r as usize;
            feat[vi] += 1;
            joint[(vi << bits) | ri] += 1;
            direct_matches += usize::from(v0 == r);
        }
        for (fv, &f_cnt) in feat.iter().enumerate().take(m) {
            if f_cnt == 0 {
                continue;
            }
            alphabet_size += 1;
            let p_feature = f64::from(f_cnt) / n;
            let base = fv << bits;
            let zero_cnt = joint[base];
            let mut max_sym = 0u32;
            for (rv, &p_res) in res_ctx.small_p_res.iter().enumerate().take(m) {
                let cnt = joint[base | rv];
                if cnt > 0 {
                    if cnt > max_sym {
                        max_sym = cnt;
                    }
                    let p_joint = f64::from(cnt) / n;
                    information += p_joint * (p_joint / (p_feature * p_res)).log2();
                }
            }
            gain += max_sym.saturating_sub(zero_cnt) as usize;
        }
    } else {
        scratch.clear();
        scratch.reserve(res_ctx.len.saturating_sub(scratch.capacity()));
        for (&v0, &r) in f0.iter().zip(residuals) {
            direct_matches += usize::from(v0 == r);
            scratch.push((v0, r));
        }
        scratch.sort_unstable();
        let len = scratch.len();
        let mut i = 0;
        while i < len {
            let fv = scratch[i].0;
            let mut f_end = i + 1;
            while f_end < len && scratch[f_end].0 == fv {
                f_end += 1;
            }
            let f_cnt = f_end - i;
            alphabet_size += 1;
            let p_feature = f_cnt as f64 / n;
            let mut max_sym = 0usize;
            let mut zero_cnt = 0usize;
            let mut j = i;
            while j < f_end {
                let rv = scratch[j].1;
                let r_start = j;
                while j < f_end && scratch[j].1 == rv {
                    j += 1;
                }
                let cnt = j - r_start;
                if cnt > max_sym {
                    max_sym = cnt;
                }
                if rv == 0 {
                    zero_cnt = cnt;
                }
                let p_joint = cnt as f64 / n;
                let p_res = res_ctx.res_p(rv);
                information += p_joint * (p_joint / (p_feature * p_res)).log2();
            }
            gain += max_sym.saturating_sub(zero_cnt);
            i = f_end;
        }
    }

    finalize_feature_score(
        res_ctx,
        vec![feature_idx],
        information,
        alphabet_size,
        gain,
        direct_matches,
    )
}

fn score_pair_normalized(
    res_ctx: &ResidualContext,
    residuals: &[u64],
    first: usize,
    second: usize,
    f0: &[u64],
    f1: &[u64],
    scratch: &mut Vec<(u64, u64, u64)>,
) -> FeatureScore {
    let bits = res_ctx.width.bits();
    let n = res_ctx.n_f64;
    let mut information = 0.0f64;
    let mut alphabet_size = 0usize;
    let mut gain = 0usize;

    if bits == 1 {
        let mut feat = [0u32; 4];
        let mut joint = [0u32; 8];
        for ((&v0, &v1), &r) in f0.iter().zip(f1).zip(residuals) {
            let fk = ((v0 as usize) << 1) | (v1 as usize);
            feat[fk] += 1;
            joint[(fk << 1) | (r as usize)] += 1;
        }
        for (fk, &f_cnt) in feat.iter().enumerate() {
            if f_cnt == 0 {
                continue;
            }
            alphabet_size += 1;
            let p_feature = f64::from(f_cnt) / n;
            let base = fk << 1;
            let c0 = joint[base];
            let c1 = joint[base | 1];
            if c0 > 0 {
                let p_joint = f64::from(c0) / n;
                information += p_joint * (p_joint / (p_feature * res_ctx.small_p_res[0])).log2();
            }
            if c1 > 0 {
                let p_joint = f64::from(c1) / n;
                information += p_joint * (p_joint / (p_feature * res_ctx.small_p_res[1])).log2();
            }
            gain += c0.max(c1).saturating_sub(c0) as usize;
        }
    } else if bits == 2 {
        let mut feat = [0u32; 16];
        let mut joint = [0u32; 64];
        for ((&v0, &v1), &r) in f0.iter().zip(f1).zip(residuals) {
            let fk = ((v0 as usize) << 2) | (v1 as usize);
            feat[fk] += 1;
            joint[(fk << 2) | (r as usize)] += 1;
        }
        for (fk, &f_cnt) in feat.iter().enumerate() {
            if f_cnt == 0 {
                continue;
            }
            alphabet_size += 1;
            let p_feature = f64::from(f_cnt) / n;
            let base = fk << 2;
            let zero_cnt = joint[base];
            let mut max_sym = 0u32;
            for (rv, &p_res) in res_ctx.small_p_res.iter().enumerate().take(4) {
                let cnt = joint[base | rv];
                if cnt > 0 {
                    if cnt > max_sym {
                        max_sym = cnt;
                    }
                    let p_joint = f64::from(cnt) / n;
                    information += p_joint * (p_joint / (p_feature * p_res)).log2();
                }
            }
            gain += max_sym.saturating_sub(zero_cnt) as usize;
        }
    } else {
        scratch.clear();
        scratch.reserve(res_ctx.len.saturating_sub(scratch.capacity()));
        for ((&v0, &v1), &r) in f0.iter().zip(f1).zip(residuals) {
            scratch.push((v0, v1, r));
        }
        scratch.sort_unstable();
        let len = scratch.len();
        let mut i = 0;
        while i < len {
            let (v0, v1, _) = scratch[i];
            let mut f_end = i + 1;
            while f_end < len && scratch[f_end].0 == v0 && scratch[f_end].1 == v1 {
                f_end += 1;
            }
            let f_cnt = f_end - i;
            alphabet_size += 1;
            let p_feature = f_cnt as f64 / n;
            let mut max_sym = 0usize;
            let mut zero_cnt = 0usize;
            let mut j = i;
            while j < f_end {
                let rv = scratch[j].2;
                let r_start = j;
                while j < f_end && scratch[j].2 == rv {
                    j += 1;
                }
                let cnt = j - r_start;
                if cnt > max_sym {
                    max_sym = cnt;
                }
                if rv == 0 {
                    zero_cnt = cnt;
                }
                let p_joint = cnt as f64 / n;
                let p_res = res_ctx.res_p(rv);
                information += p_joint * (p_joint / (p_feature * p_res)).log2();
            }
            gain += max_sym.saturating_sub(zero_cnt);
            i = f_end;
        }
    }

    finalize_feature_score(
        res_ctx,
        vec![first, second],
        information,
        alphabet_size,
        gain,
        0,
    )
}

fn finalize_feature_score(
    res_ctx: &ResidualContext,
    feature_indices: Vec<usize>,
    information: f64,
    alphabet_size: usize,
    match_gain: usize,
    direct_matches: usize,
) -> FeatureScore {
    let mutual_information_bits = information.max(0.0);
    let correction = (alphabet_size.saturating_sub(1) as f64)
        * (res_ctx.alphabet_size.saturating_sub(1) as f64)
        / (2.0 * std::f64::consts::LN_2 * res_ctx.n_f64);
    FeatureScore {
        feature_indices,
        mutual_information_bits,
        corrected_mutual_information_bits: mutual_information_bits - correction,
        match_gain,
        direct_matches,
    }
}

fn frequencies(values: impl IntoIterator<Item = u64>) -> BTreeMap<u64, usize> {
    let mut counts = BTreeMap::new();
    for value in values {
        *counts.entry(value).or_default() += 1;
    }
    counts
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
    NegAddConst(SourceFeature, u64),
    Neg(SourceFeature),
    ScaleEqz(SourceFeature, u64),
    // 4. Nonlinear Interaction Motifs
    AddEqz(SourceFeature, SourceFeature),
    MulEqz(SourceFeature, SourceFeature),
    MulEqzPair(SourceFeature, SourceFeature),
}

fn source_is_cross_channel(source: &SourceFeature, out_port: u16) -> bool {
    match source {
        SourceFeature::BoundaryIn(c) | SourceFeature::LagIn { channel: c, .. } => *c != out_port,
        SourceFeature::LagYhat { port, .. } | SourceFeature::LagRes { port, .. } => {
            *port != out_port
        }
        SourceFeature::NodeOut { .. } => false,
    }
}

fn source_feature_lag(source: &SourceFeature) -> u8 {
    match source {
        SourceFeature::LagIn { tau, .. }
        | SourceFeature::LagYhat { tau, .. }
        | SourceFeature::LagRes { tau, .. } => *tau,
        SourceFeature::BoundaryIn(_) | SourceFeature::NodeOut { .. } => 0,
    }
}

fn motif_any_source(motif: &CandidateMotif, mut pred: impl FnMut(&SourceFeature) -> bool) -> bool {
    match motif {
        CandidateMotif::ConstLoop(_) => false,
        CandidateMotif::Identity(u)
        | CandidateMotif::Eqz(u)
        | CandidateMotif::AddSelf(u)
        | CandidateMotif::AddConst(u, _)
        | CandidateMotif::EqzAddConst(u, _)
        | CandidateMotif::NegAddConst(u, _)
        | CandidateMotif::Neg(u)
        | CandidateMotif::ScaleEqz(u, _) => pred(u),
        CandidateMotif::AddPair(u1, u2)
        | CandidateMotif::SubPair(u1, u2)
        | CandidateMotif::MulPair(u1, u2)
        | CandidateMotif::AddEqz(u1, u2)
        | CandidateMotif::MulEqz(u1, u2)
        | CandidateMotif::MulEqzPair(u1, u2) => pred(u1) || pred(u2),
    }
}

/// Returns `true` iff any [`SourceFeature`] referenced by `motif` belongs to a channel `c != out_port`.
#[must_use]
pub fn motif_is_cross_channel(motif: &CandidateMotif, out_port: u16) -> bool {
    motif_any_source(motif, |u| source_is_cross_channel(u, out_port))
}

fn motif_has_band_cross_feature(
    motif: &CandidateMotif,
    out_port: u16,
    min_tau: u8,
    max_tau: u8,
) -> bool {
    motif_any_source(motif, |u| {
        let lag = source_feature_lag(u);
        source_is_cross_channel(u, out_port) && lag >= min_tau && lag <= max_tau
    })
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

fn fill_lagged_boundary_values(
    obs_window: &ObservationWindow,
    start_in_window: usize,
    len: usize,
    channel: usize,
    tau: usize,
    values: &mut Vec<u64>,
) {
    values.clear();
    values.reserve(len.saturating_sub(values.capacity()));
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
}

fn lagged_boundary_values(
    obs_window: &ObservationWindow,
    start_in_window: usize,
    len: usize,
    channel: usize,
    tau: usize,
) -> Vec<u64> {
    let mut values = Vec::with_capacity(len);
    fill_lagged_boundary_values(obs_window, start_in_window, len, channel, tau, &mut values);
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
            candidates.push(CandidateMotif::NegAddConst(u1.clone(), c));
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

fn lag_in_delay_count(
    wire_delay: WireDelayMode,
    tau: u8,
    h_arm: usize,
    is_unconnected_identity: bool,
) -> usize {
    let tau_usize = usize::from(tau);
    match wire_delay {
        WireDelayMode::Combinational => tau_usize,
        WireDelayMode::TicksPerHop => {
            if is_unconnected_identity {
                if tau == 2 {
                    0
                } else if tau == 1 {
                    1
                } else {
                    tau_usize.saturating_sub(1) / 2
                }
            } else {
                tau_usize.saturating_sub(h_arm) / 2
            }
        }
    }
}

fn materialize_source_aligned(
    circuit: &mut Circuit,
    source: &SourceFeature,
    _out_port: u16,
    h_arm: usize,
    is_unconnected_identity: bool,
) -> SignalSource {
    match source {
        SourceFeature::BoundaryIn(c) => SignalSource::BoundaryIn(*c),
        SourceFeature::NodeOut { node, port } => SignalSource::NodeOut {
            node: *node,
            port: *port,
        },
        SourceFeature::LagIn { channel, tau } => {
            let n_delay =
                lag_in_delay_count(circuit.wire_delay(), *tau, h_arm, is_unconnected_identity);
            let mut curr = tap_source(circuit, SignalSource::BoundaryIn(*channel));
            for _ in 0..n_delay {
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

fn materialize_eqz_tap(
    circuit: &mut Circuit,
    source: &SourceFeature,
    out_port: u16,
    h_arm: usize,
) -> SignalSource {
    let eqz = circuit.add_node(Primitive::Eqz);
    let src = materialize_source_aligned(circuit, source, out_port, h_arm, false);
    let tapped = tap_source(circuit, src);
    circuit.connect(tapped, SignalSink::NodeIn { node: eqz, port: 0 });
    SignalSource::NodeOut { node: eqz, port: 0 }
}

fn materialize_add_const_with_hops(
    circuit: &mut Circuit,
    source: &SourceFeature,
    c: u64,
    out_port: u16,
    h_arm: usize,
) -> SignalSource {
    let add = circuit.add_node(Primitive::Add);
    let u_src = materialize_source_aligned(circuit, source, out_port, h_arm, false);
    let u_tap = tap_source(circuit, u_src);
    circuit.connect(u_tap, SignalSink::NodeIn { node: add, port: 0 });
    let c_src = materialize_const_loop(circuit, c);
    circuit.connect(c_src, SignalSink::NodeIn { node: add, port: 1 });
    SignalSource::NodeOut { node: add, port: 0 }
}

fn materialize_motif_with_stack(
    circuit: &mut Circuit,
    motif: &CandidateMotif,
    out_port: u16,
    is_driven: bool,
) -> SignalSource {
    let stack_offset = usize::from(is_driven);
    let arm_hops = |motif_hops: usize| -> usize { motif_hops + 1 + stack_offset };
    match motif {
        CandidateMotif::Identity(u) => {
            let src = if !is_driven {
                materialize_source_aligned(circuit, u, out_port, 0, true)
            } else {
                materialize_source_aligned(circuit, u, out_port, arm_hops(0), false)
            };
            tap_source(circuit, src)
        }
        CandidateMotif::Eqz(u) => materialize_eqz_tap(circuit, u, out_port, arm_hops(1)),
        CandidateMotif::AddSelf(u) => {
            let add = circuit.add_node(Primitive::Add);
            let split = circuit.add_node(Primitive::Split);
            let src = materialize_source_aligned(circuit, u, out_port, arm_hops(2), false);
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
        CandidateMotif::AddPair(u1, u2)
        | CandidateMotif::SubPair(u1, u2)
        | CandidateMotif::MulPair(u1, u2) => {
            let prim = match motif {
                CandidateMotif::AddPair(_, _) => Primitive::Add,
                CandidateMotif::SubPair(_, _) => Primitive::Sub,
                CandidateMotif::MulPair(_, _) => Primitive::Mul,
                _ => unreachable!(),
            };
            let op = circuit.add_node(prim);
            let h = arm_hops(1);
            let s1 = materialize_source_aligned(circuit, u1, out_port, h, false);
            let t1 = tap_source(circuit, s1);
            circuit.connect(t1, SignalSink::NodeIn { node: op, port: 0 });
            let s2 = materialize_source_aligned(circuit, u2, out_port, h, false);
            let t2 = tap_source(circuit, s2);
            circuit.connect(t2, SignalSink::NodeIn { node: op, port: 1 });
            SignalSource::NodeOut { node: op, port: 0 }
        }
        CandidateMotif::ConstLoop(c) => materialize_const_loop(circuit, *c),
        CandidateMotif::AddConst(u, c) => {
            materialize_add_const_with_hops(circuit, u, *c, out_port, arm_hops(1))
        }
        CandidateMotif::EqzAddConst(u, c) => {
            let eqz = circuit.add_node(Primitive::Eqz);
            let add_src = materialize_add_const_with_hops(circuit, u, *c, out_port, arm_hops(2));
            circuit.connect(add_src, SignalSink::NodeIn { node: eqz, port: 0 });
            SignalSource::NodeOut { node: eqz, port: 0 }
        }
        CandidateMotif::NegAddConst(u, c) => {
            let sub = circuit.add_node(Primitive::Sub);
            let c_src = materialize_const_loop(circuit, *c);
            circuit.connect(c_src, SignalSink::NodeIn { node: sub, port: 0 });
            let u_src = materialize_source_aligned(circuit, u, out_port, arm_hops(1), false);
            let u_tap = tap_source(circuit, u_src);
            circuit.connect(u_tap, SignalSink::NodeIn { node: sub, port: 1 });
            SignalSource::NodeOut { node: sub, port: 0 }
        }
        CandidateMotif::Neg(u) => {
            let h = arm_hops(1);
            if circuit.width().bits() > 1 {
                let mult = (1u64 << circuit.width().bits()) - 1;
                let mul = circuit.add_node(Primitive::Mul);
                let u_src = materialize_source_aligned(circuit, u, out_port, h, false);
                let u_tap = tap_source(circuit, u_src);
                circuit.connect(u_tap, SignalSink::NodeIn { node: mul, port: 0 });
                let c_src = materialize_const_loop(circuit, mult);
                circuit.connect(c_src, SignalSink::NodeIn { node: mul, port: 1 });
                SignalSource::NodeOut { node: mul, port: 0 }
            } else {
                let sub = circuit.add_node(Primitive::Sub);
                let u_src = materialize_source_aligned(circuit, u, out_port, h, false);
                let u_tap = tap_source(circuit, u_src);
                circuit.connect(u_tap, SignalSink::NodeIn { node: sub, port: 1 });
                let zero_src = materialize_const_loop(circuit, 0);
                circuit.connect(zero_src, SignalSink::NodeIn { node: sub, port: 0 });
                SignalSource::NodeOut { node: sub, port: 0 }
            }
        }
        CandidateMotif::ScaleEqz(u, c) => {
            let mul = circuit.add_node(Primitive::Mul);
            let eqz_src = materialize_eqz_tap(circuit, u, out_port, arm_hops(2));
            circuit.connect(eqz_src, SignalSink::NodeIn { node: mul, port: 0 });
            let c_src = materialize_const_loop(circuit, *c);
            circuit.connect(c_src, SignalSink::NodeIn { node: mul, port: 1 });
            SignalSource::NodeOut { node: mul, port: 0 }
        }
        CandidateMotif::AddEqz(u1, u2) | CandidateMotif::MulEqz(u1, u2) => {
            let prim = match motif {
                CandidateMotif::AddEqz(_, _) => Primitive::Add,
                CandidateMotif::MulEqz(_, _) => Primitive::Mul,
                _ => unreachable!(),
            };
            let op = circuit.add_node(prim);
            let s1 = materialize_source_aligned(circuit, u1, out_port, arm_hops(1), false);
            let t1 = tap_source(circuit, s1);
            circuit.connect(t1, SignalSink::NodeIn { node: op, port: 0 });
            let eqz_src = materialize_eqz_tap(circuit, u2, out_port, arm_hops(2));
            circuit.connect(eqz_src, SignalSink::NodeIn { node: op, port: 1 });
            SignalSource::NodeOut { node: op, port: 0 }
        }
        CandidateMotif::MulEqzPair(u1, u2) => {
            let mul = circuit.add_node(Primitive::Mul);
            let h = arm_hops(2);
            let eqz1_src = materialize_eqz_tap(circuit, u1, out_port, h);
            circuit.connect(eqz1_src, SignalSink::NodeIn { node: mul, port: 0 });
            let eqz2_src = materialize_eqz_tap(circuit, u2, out_port, h);
            circuit.connect(eqz2_src, SignalSink::NodeIn { node: mul, port: 1 });
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
            let u_src = materialize_source_aligned(circuit, u, out_port, 2, false);
            stack_output_binary_op(circuit, Primitive::Sub, out_port, u_src)
        }
        CandidateMotif::ScaleEqz(u, c) if is_driven && *c == circuit.width().mask() => {
            let eqz_src = materialize_eqz_tap(circuit, u, out_port, 3);
            stack_output_binary_op(circuit, Primitive::Sub, out_port, eqz_src)
        }
        _ => {
            let motif_src = materialize_motif_with_stack(circuit, motif, out_port, is_driven);
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
    facts_curr: f64,
    driven_ports: Vec<bool>,
    e_curr_by_port: Vec<usize>,
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
    let num_outputs = usize::from(editable.compiled().num_outputs());
    let driven_ports: Vec<bool> = (0..num_outputs)
        .map(|q| {
            let port_u16 = u16::try_from(q).expect("output port fits in u16");
            editable
                .circuit()
                .edges()
                .iter()
                .any(|edge| edge.dst == SignalSink::BoundaryOut(port_u16))
        })
        .collect();
    let mut curr_pred = vec![0u64; num_outputs];
    let mut e_curr_by_port = vec![0usize; num_outputs];
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
            for (q, e_port) in e_curr_by_port.iter_mut().enumerate() {
                if driven_ports[q] && q < truth.len() && curr_pred[q] != truth[q] {
                    *e_port += 1;
                }
            }
            if p_curr != p_truth {
                e_curr += 1;
            }
            flux_curr += reward_curr;
        }
    }

    let l_curr = circuit_code_bits_with_tax(editable.circuit(), 0, economy_config.split_code_tax);
    let facts_curr = facts_code_bits(editable.circuit().width(), w_diag as u64, e_curr as u64);
    let survivor_map: Vec<Option<NodeId>> = (0..editable.circuit().node_count())
        .map(|i| Some(NodeId(i as u32)))
        .collect();

    BaselineReplayMetrics {
        e_curr,
        e_const_mode,
        flux_curr,
        l_curr,
        facts_curr,
        driven_ports,
        e_curr_by_port,
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
    allow_epoch1_1op_bypass: bool,
    baseline: &BaselineReplayMetrics,
    budget: Option<&mut TickBudget>,
    mut telemetry: Option<&mut SproutTelemetry>,
) -> SproutMotifOutcome {
    let w_diag = growth_config.diagnostic_window;
    let mut staged = editable.circuit().clone();
    materialize_candidate(&mut staged, motif, out_port);
    let max_nodes = effective_max_nodes(editable.circuit().num_inputs(), growth_config);
    if staged.nodes().len() > max_nodes {
        return SproutMotifOutcome::BlockedMaxNodes;
    }
    if let Some(b) = budget
        && !b.try_consume(baseline.w_replay as u64)
    {
        if let Some(tel) = telemetry.as_deref_mut() {
            tel.sprout_budget_exhausted += 1;
        }
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
    let num_outputs = usize::from(staged_compiled.num_outputs());
    let mut pred_stack = [0u64; 16];
    let mut pred_heap = Vec::new();
    let pred: &mut [u64] = if num_outputs <= pred_stack.len() {
        &mut pred_stack[..num_outputs]
    } else {
        pred_heap.resize(num_outputs, 0);
        &mut pred_heap
    };

    let mut e_port_stack = [0usize; 16];
    let mut e_port_heap = Vec::new();
    let e_cand_by_port: &mut [usize] = if num_outputs <= e_port_stack.len() {
        &mut e_port_stack[..num_outputs]
    } else {
        e_port_heap.resize(num_outputs, 0);
        &mut e_port_heap
    };

    let mut trace_stack = [0u64; 256];
    let mut trace_heap = Vec::new();
    let p_cand_trace: &mut [u64] = if baseline.w_replay <= trace_stack.len() {
        &mut trace_stack[..baseline.w_replay]
    } else {
        trace_heap.resize(baseline.w_replay, 0);
        &mut trace_heap
    };

    let is_epoch1_non_const = is_epoch_1 && !matches!(motif, CandidateMotif::ConstLoop(_));
    let is_binary_eqz_scaffold = is_epoch1_non_const
        && staged.width() == Width::BITS_1
        && matches!(motif, CandidateMotif::Eqz(SourceFeature::BoundaryIn(c)) if *c == out_port);
    let e_cand_upper: isize = {
        let base = (baseline.e_curr as isize) - (growth_config.min_match_gain as isize);
        if is_epoch1_non_const {
            let const_lim = if is_binary_eqz_scaffold {
                baseline.e_const_mode.saturating_add(1) as isize
            } else {
                (baseline.e_const_mode as isize) - (growth_config.min_match_gain as isize)
            };
            base.min(const_lim)
        } else {
            base
        }
    };

    let out_idx = usize::from(out_port);
    let mut e_cand = 0usize;

    for (i, trace_slot) in p_cand_trace.iter_mut().enumerate() {
        let idx = baseline.start_idx + i;
        let input = if idx == 0 {
            obs_window.prior_frame()
        } else {
            &obs_window.frames()[idx - 1]
        };
        let truth = &obs_window.frames()[idx];
        if state.step(&staged_compiled, input, pred).is_err() {
            return SproutMotifOutcome::Rejected;
        }
        let p_cand = pred[out_idx];
        *trace_slot = p_cand;
        if idx >= baseline.eval_start_idx {
            for (q, e_port) in e_cand_by_port.iter_mut().enumerate() {
                if q < truth.len() && pred[q] != truth[q] {
                    *e_port += 1;
                    if q != out_idx
                        && baseline.driven_ports.get(q).copied().unwrap_or(false)
                        && *e_port > baseline.e_curr_by_port[q]
                    {
                        return SproutMotifOutcome::Rejected;
                    }
                }
            }
            if p_cand != truth[out_idx] {
                e_cand += 1;
            }
            if (e_cand as isize) > e_cand_upper {
                return SproutMotifOutcome::Rejected;
            }
        }
    }

    let mut scorer = PointForecastScorer::new(staged.width(), economy_config.null_model);
    let mut flux_cand = 0.0;
    let mut pos_flux_cand = 0.0;
    for (i, &p_cand) in p_cand_trace.iter().enumerate() {
        let idx = baseline.start_idx + i;
        let p_truth = obs_window.frames()[idx][out_idx];
        let reward = score_with_reward_mode(
            &mut scorer,
            economy_config.reward_mode,
            staged.width(),
            p_cand,
            p_truth,
        );
        if idx >= baseline.eval_start_idx {
            flux_cand += reward;
            pos_flux_cand += reward.max(0.0);
        }
    }

    let delta_flux = flux_cand - baseline.flux_curr;
    let l_cand = circuit_code_bits_with_tax(&staged, 0, economy_config.split_code_tax);
    let delta_l_sprout = (l_cand - baseline.l_curr).max(0.0);
    let is_1_op = is_epoch_1
        && matches!(
            motif,
            CandidateMotif::Identity(SourceFeature::BoundaryIn(c))
                | CandidateMotif::Eqz(SourceFeature::BoundaryIn(c))
                if *c == out_port
        );

    let d_cone = port_cone_register_depth(&staged, out_port);
    let eff_juv = effective_juvenile_ticks(d_cone, economy_config, growth_config);

    if economy_config.juvenile_ticks > 0 && delta_l_sprout > 0.0 {
        let (projected_survival, horizon_limit) = if d_cone
            <= usize::from(growth_config.max_reactive_lag)
        {
            (
                economy_config.initial_energy
                    + (f64::from(economy_config.juvenile_ticks) / w_diag as f64) * pos_flux_cand,
                obs_window.capacity(),
            )
        } else {
            let new_nodes = staged
                .node_count()
                .saturating_sub(editable.circuit().node_count())
                .max(1);
            let endowment = (new_nodes as f64) * economy_config.initial_energy;
            let scaled_flux = (f64::from(eff_juv) / w_diag as f64) * pos_flux_cand;
            (
                endowment + scaled_flux,
                observation_horizon(economy_config, growth_config)
                    .max((eff_juv as usize).saturating_add(2 * w_diag)),
            )
        };
        if projected_survival <= delta_l_sprout {
            let delta_i_hat = pos_flux_cand / (w_diag as f64);
            if delta_i_hat > 0.0 {
                let w_star = delta_l_sprout / delta_i_hat;
                if w_star > (horizon_limit as f64)
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
        && matches!(motif, CandidateMotif::Eqz(SourceFeature::BoundaryIn(c)) if *c == out_port)
        && pos_flux_cand > delta_l_sprout;
    if delta_flux <= 0.0 && !allow_eqz_scaffold_zero_flux {
        return SproutMotifOutcome::Rejected;
    }

    let num_channels = editable.circuit().num_inputs();
    let facts_cand = facts_code_bits(staged.width(), w_diag as u64, e_cand as u64);
    let rank_scale = sprout_rank_scale(num_channels, w_diag, economy_config);
    let j_cand = l_cand + rank_scale * facts_cand;

    let epoch1_scaffold_bypass = is_epoch_1
        && !motif_is_cross_channel(motif, out_port)
        && (num_channels == 1 || (is_1_op && allow_epoch1_1op_bypass));
    if !epoch1_scaffold_bypass && e_cand > 0 {
        let amort_scale = if economy_config.juvenile_ticks > 0
            && (num_channels > 1 || d_cone > usize::from(growth_config.max_reactive_lag))
        {
            (f64::from(economy_config.juvenile_ticks) / (w_diag as f64)).max(1.0)
        } else {
            1.0
        };
        let j_mat_cand = l_cand + amort_scale * facts_cand;
        let j_mat_curr = baseline.l_curr + amort_scale * baseline.facts_curr;
        if j_mat_cand >= j_mat_curr {
            return SproutMotifOutcome::Rejected;
        }
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
    let res_ctx = ResidualContext::new(&norm_residuals, width, growth_config.min_match_gain);
    let mut val_buf = Vec::with_capacity(norm_residuals.len());
    let mut scratch_1: Vec<(u64, u64)> = Vec::new();
    let len = norm_residuals.len();
    let start_in_window = obs_window.len().saturating_sub(len);
    let num_channels = editable.circuit().num_inputs();
    let cross_only = max_tau > growth_config.max_reactive_lag;
    for tau in start_tau..=probe_max {
        for c in 0..usize::from(num_channels) {
            if cross_only && c == usize::from(out_port) {
                continue;
            }
            fill_lagged_boundary_values(obs_window, start_in_window, len, c, tau, &mut val_buf);
            for slot in &mut val_buf {
                *slot = width.normalize(*slot);
            }
            let score =
                score_singleton_normalized(&res_ctx, &norm_residuals, 0, &val_buf, &mut scratch_1);
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
                    let uses_deeper_lag = motif_any_source(&motif, |sf| {
                        source_feature_lag(sf) > max_tau
                            && (!cross_only || source_is_cross_channel(sf, out_port))
                    });
                    if !uses_deeper_lag {
                        continue;
                    }
                    if matches!(
                        evaluate_sprout_motif(
                            editable,
                            obs_window,
                            growth_config,
                            economy_config,
                            out_port,
                            &motif,
                            is_epoch_1,
                            num_channels == 1,
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

fn sprout_rank_scale(num_channels: u16, w_diag: usize, economy_config: &EconomyConfig) -> f64 {
    if economy_config.juvenile_ticks > 0 && num_channels > 1 {
        (f64::from(economy_config.juvenile_ticks) / (w_diag as f64)).max(1.0)
    } else {
        1.0
    }
}

/// Accepted candidate circuit paired with its diagnostic-window mismatch count and ranking score.
#[derive(Clone, Debug, PartialEq)]
pub struct ScoredSproutProposal {
    /// Staged circuit containing the proposed motif.
    pub circuit: Circuit,
    /// Replay mismatch count on `out_port` over the diagnostic window.
    pub e_cand: usize,
    /// Uniform horizon-adjusted MDL ranking score (`l_cand + rank_scale * facts_cand`).
    pub j_cand: f64,
}

fn is_better_mdl(j_cand: f64, delta_flux: f64, prev_j: f64, prev_flux: f64) -> bool {
    j_cand < prev_j || ((j_cand - prev_j).abs() < MDL_TIE_BREAK_EPS && delta_flux > prev_flux)
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
    propose_sprout_scored_with_telemetry(
        editable,
        obs_window,
        diag,
        growth_config,
        economy_config,
        budget,
        out_port,
        max_tau,
        telemetry,
    )
    .map(|opt| opt.map(|p| p.circuit))
}

/// Evaluates candidates up to `max_tau` and returns the winning proposal with `(e_cand, j_cand)`.
#[allow(clippy::too_many_arguments)]
pub fn propose_sprout_scored_with_telemetry(
    editable: &EditableCircuit,
    obs_window: &ObservationWindow,
    diag: &DiagnosticBuffer,
    growth_config: &GrowthConfig,
    economy_config: &EconomyConfig,
    budget: &mut TickBudget,
    out_port: u16,
    max_tau: u8,
    telemetry: &mut SproutTelemetry,
) -> Result<Option<ScoredSproutProposal>, GrowthError> {
    propose_sprout_band_scored_with_telemetry(
        editable,
        obs_window,
        diag,
        growth_config,
        economy_config,
        budget,
        out_port,
        1,
        max_tau,
        telemetry,
    )
}

/// Evaluates candidates whose lag features lie in incremental band `min_tau..=max_tau` (C2-a).
///
/// When `min_tau <= 1`, evaluates all reactive features `1..=max_tau` identically to
/// [`propose_sprout_with_telemetry`]. When `min_tau > 1`, builds the signal pool up to `max_tau`
/// and scores only singletons inside `min_tau..=max_tau` and pairs with at least one member in `min_tau..=max_tau`.
#[allow(clippy::too_many_arguments)]
pub fn propose_sprout_band_with_telemetry(
    editable: &EditableCircuit,
    obs_window: &ObservationWindow,
    diag: &DiagnosticBuffer,
    growth_config: &GrowthConfig,
    economy_config: &EconomyConfig,
    budget: &mut TickBudget,
    out_port: u16,
    min_tau: u8,
    max_tau: u8,
    telemetry: &mut SproutTelemetry,
) -> Result<Option<Circuit>, GrowthError> {
    propose_sprout_band_scored_with_telemetry(
        editable,
        obs_window,
        diag,
        growth_config,
        economy_config,
        budget,
        out_port,
        min_tau,
        max_tau,
        telemetry,
    )
    .map(|opt| opt.map(|p| p.circuit))
}

/// Evaluates candidates in incremental band `min_tau..=max_tau` and returns `(circuit, e_cand, j_cand)`.
#[allow(clippy::too_many_arguments)]
pub fn propose_sprout_band_scored_with_telemetry(
    editable: &EditableCircuit,
    obs_window: &ObservationWindow,
    diag: &DiagnosticBuffer,
    growth_config: &GrowthConfig,
    economy_config: &EconomyConfig,
    budget: &mut TickBudget,
    out_port: u16,
    min_tau: u8,
    max_tau: u8,
    telemetry: &mut SproutTelemetry,
) -> Result<Option<ScoredSproutProposal>, GrowthError> {
    propose_sprout_band_scored_internal(
        editable,
        obs_window,
        diag,
        growth_config,
        economy_config,
        budget,
        out_port,
        min_tau,
        max_tau,
        telemetry,
        true,
    )
    .map(|(proposal, _)| proposal)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn propose_sprout_band_scored_internal(
    editable: &EditableCircuit,
    obs_window: &ObservationWindow,
    diag: &DiagnosticBuffer,
    growth_config: &GrowthConfig,
    economy_config: &EconomyConfig,
    budget: &mut TickBudget,
    out_port: u16,
    min_tau: u8,
    max_tau: u8,
    telemetry: &mut SproutTelemetry,
    compute_lag_bound: bool,
) -> Result<(Option<ScoredSproutProposal>, bool), GrowthError> {
    let len = diag.len();
    if len < growth_config.diagnostic_window {
        return Ok((None, false));
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

    let diagnostics = diagnose_residual_with_band(
        editable.circuit().width(),
        &residuals,
        &features,
        *growth_config,
        min_tau,
        max_tau,
        out_port,
    )?;
    if diagnostics.candidate_list_truncated {
        telemetry.candidate_list_truncated += 1;
    }
    if diagnostics.zero_error {
        return Ok((None, false));
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
        if !compute_lag_bound {
            return Ok((None, true));
        }
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
        return Ok((None, false));
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
    let boundary_in_diagnosed = feature_pairs.iter().any(|(u1, u2)| {
        *u1 == SourceFeature::BoundaryIn(out_port)
            || u2.as_ref() == Some(&SourceFeature::BoundaryIn(out_port))
    });
    let allow_epoch1_1op_bypass = num_channels == 1 || boundary_in_diagnosed;
    let mut candidates = Vec::new();
    if feature_pairs.is_empty() {
        if min_tau <= 1
            && let Some((c_off, _)) = diagnostics.constant_offset
        {
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
                if min_tau > 1 && !motif_has_band_cross_feature(&c, out_port, min_tau, max_tau) {
                    continue;
                }
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
        return Ok((None, false));
    }

    let mut best_same: Option<(Circuit, usize, f64, f64, bool)> = None;
    let mut best_cross: Option<(Circuit, usize, f64, f64)> = None;
    let mut best_other: Option<(Circuit, usize, f64, f64)> = None;
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
            allow_epoch1_1op_bypass,
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
                if is_epoch_1 {
                    if motif_is_cross_channel(motif, out_port) {
                        let is_better = match &best_cross {
                            None => true,
                            Some((_, _, prev_j, prev_flux)) => {
                                is_better_mdl(j_cand, delta_flux, *prev_j, *prev_flux)
                            }
                        };
                        if is_better {
                            best_cross = Some((staged, e_cand, j_cand, delta_flux));
                        }
                    } else {
                        let is_better = match &best_same {
                            None => true,
                            Some((_, _, prev_j, prev_flux, prev_1_op)) => {
                                if is_1_op && !prev_1_op {
                                    true
                                } else if !is_1_op && *prev_1_op {
                                    false
                                } else {
                                    is_better_mdl(j_cand, delta_flux, *prev_j, *prev_flux)
                                }
                            }
                        };
                        if is_better {
                            best_same = Some((staged, e_cand, j_cand, delta_flux, is_1_op));
                        }
                    }
                } else {
                    let is_better = match &best_other {
                        None => true,
                        Some((_, _, prev_j, prev_flux)) => {
                            is_better_mdl(j_cand, delta_flux, *prev_j, *prev_flux)
                        }
                    };
                    if is_better {
                        best_other = Some((staged, e_cand, j_cand, delta_flux));
                    }
                }
            }
            SproutMotifOutcome::BlockedMaxNodes => {
                blocked_by_max_nodes += 1;
            }
            SproutMotifOutcome::Rejected => {}
            SproutMotifOutcome::OutOfBudget => break,
        }
    }

    let best_proposal = if is_epoch_1 {
        match (best_same, best_cross) {
            (None, None) => None,
            (Some(same), None) => Some(ScoredSproutProposal {
                circuit: same.0,
                e_cand: same.1,
                j_cand: same.2,
            }),
            (None, Some(cross)) => Some(ScoredSproutProposal {
                circuit: cross.0,
                e_cand: cross.1,
                j_cand: cross.2,
            }),
            (Some(same), Some(cross)) => {
                if is_better_mdl(cross.2, cross.3, same.2, same.3) {
                    Some(ScoredSproutProposal {
                        circuit: cross.0,
                        e_cand: cross.1,
                        j_cand: cross.2,
                    })
                } else {
                    Some(ScoredSproutProposal {
                        circuit: same.0,
                        e_cand: same.1,
                        j_cand: same.2,
                    })
                }
            }
        }
    } else {
        best_other.map(|(circuit, e_cand, j_cand, _)| ScoredSproutProposal {
            circuit,
            e_cand,
            j_cand,
        })
    };

    let mut pending_deeper_check = false;
    if best_proposal.is_none() {
        if blocked_by_max_nodes > 0 {
            telemetry.sprout_blocked_max_nodes += 1;
        } else if !compute_lag_bound {
            pending_deeper_check = true;
        } else if has_admissible_deeper_lag(
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
    }

    Ok((best_proposal, pending_deeper_check))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn check_deeper_lag_for_port(
    editable: &EditableCircuit,
    obs_window: &ObservationWindow,
    diag: &DiagnosticBuffer,
    growth_config: &GrowthConfig,
    economy_config: &EconomyConfig,
    out_port: u16,
    max_tau: u8,
) -> bool {
    let residuals: Vec<u64> = diag
        .residuals
        .iter()
        .map(|f| f[usize::from(out_port)])
        .collect();
    let is_epoch_1 = !editable
        .circuit()
        .edges()
        .iter()
        .any(|edge| edge.dst == SignalSink::BoundaryOut(out_port));
    let baseline = replay_current_baseline(
        editable,
        obs_window,
        growth_config,
        economy_config,
        out_port,
        is_epoch_1,
    );
    has_admissible_deeper_lag(
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
    Accepted {
        proposal: AutonomousRingProposal,
        j_ring: f64,
        e_ring_diag: usize,
        j_ring_diag: f64,
    },
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
    let bits = editable.circuit().width().bits();
    let mut mu = vec![0u64; p];
    if bits <= 4 {
        let card = 1usize << bits;
        let needed = p * card;
        let mut stack_counts = [0usize; 32 * 16];
        let mut heap_counts = Vec::new();
        let counts: &mut [usize] = if needed <= stack_counts.len() {
            &mut stack_counts[..needed]
        } else {
            heap_counts.resize(needed, 0);
            &mut heap_counts
        };
        for (i, frame) in obs_window.frames().iter().enumerate() {
            let global_t = obs_window.start_tick() + (i as u64);
            let r = (global_t as usize) % p;
            counts[r * card + (frame[out_idx] as usize)] += 1;
        }
        for r in 0..p {
            let row = &counts[r * card..(r + 1) * card];
            let mut best_sym = 0u64;
            let mut best_cnt = row[0];
            for (sym, &cnt) in row.iter().enumerate().skip(1) {
                if cnt > best_cnt {
                    best_cnt = cnt;
                    best_sym = sym as u64;
                }
            }
            mu[r] = best_sym;
        }
    } else {
        let mut residue_counts = vec![BTreeMap::<u64, usize>::new(); p];
        for (i, frame) in obs_window.frames().iter().enumerate() {
            let global_t = obs_window.start_tick() + (i as u64);
            let r = (global_t as usize) % p;
            *residue_counts[r].entry(frame[out_idx]).or_default() += 1;
        }
        for (r, counts) in residue_counts.iter().enumerate() {
            let mode = counts
                .iter()
                .max_by(|(s1, c1), (s2, c2)| c1.cmp(c2).then_with(|| s2.cmp(s1)))
                .map(|(&s, _)| s)
                .unwrap_or(0);
            mu[r] = mode;
        }
    }
    if mu.iter().all(|&v| v == mu[0]) {
        return RingPeriodOutcome::Rejected;
    }

    let (staged, old_to_new, ring_delay_nodes) =
        build_autonomous_ring_circuit(editable.circuit(), p, &mu, out_port);
    let max_nodes = effective_max_nodes(editable.circuit().num_inputs(), growth_config);
    if staged.nodes().len() > max_nodes {
        if let Some(tel) = telemetry {
            tel.sprout_blocked_max_nodes += 1;
        }
        return RingPeriodOutcome::Rejected;
    }
    if let Some(b) = budget
        && !b.try_consume(w_replay as u64)
    {
        if let Some(tel) = telemetry {
            tel.sprout_budget_exhausted += 1;
        }
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

    let mut e_ring = 0usize;
    let mut e_ring_diag = 0usize;
    let w_diag = growth_config.diagnostic_window.max(1);
    let diag_start = w_replay.saturating_sub(w_diag);
    let e_ring_upper = (e_curr as isize) - (growth_config.min_match_gain as isize);

    let num_outputs = usize::from(staged_compiled.num_outputs());
    let mut pred_stack = [0u64; 16];
    let mut pred_heap = Vec::new();
    let pred: &mut [u64] = if num_outputs <= pred_stack.len() {
        &mut pred_stack[..num_outputs]
    } else {
        pred_heap.resize(num_outputs, 0);
        &mut pred_heap
    };

    let mut trace_stack = [0u64; 256];
    let mut trace_heap = Vec::new();
    let p_ring_trace: &mut [u64] = if w_replay <= trace_stack.len() {
        &mut trace_stack[..w_replay]
    } else {
        trace_heap.resize(w_replay, 0);
        &mut trace_heap
    };

    for (i, frame) in obs_window.frames().iter().enumerate() {
        let input = if i == 0 {
            obs_window.prior_frame()
        } else {
            &obs_window.frames()[i - 1]
        };
        if state.step(&staged_compiled, input, pred).is_err() {
            return RingPeriodOutcome::Rejected;
        }
        let p_pred = pred[out_idx];
        p_ring_trace[i] = p_pred;
        if p_pred != frame[out_idx] {
            e_ring += 1;
            if i >= diag_start {
                e_ring_diag += 1;
            }
            if (e_ring as isize) > e_ring_upper {
                return RingPeriodOutcome::Rejected;
            }
        }
    }
    if (e_ring as isize) > e_ring_upper {
        return RingPeriodOutcome::Rejected;
    }

    let mut scorer = PointForecastScorer::new(staged.width(), economy_config.null_model);
    let mut flux_ring = 0.0;
    for (i, &p_pred) in p_ring_trace.iter().enumerate() {
        flux_ring += score_with_reward_mode(
            &mut scorer,
            economy_config.reward_mode,
            staged.width(),
            p_pred,
            obs_window.frames()[i][out_idx],
        );
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

    let diag_eval = w_replay.min(w_diag);
    let rank_scale = sprout_rank_scale(staged.num_inputs(), diag_eval, economy_config);
    let j_ring_diag =
        l_ring + rank_scale * facts_code_bits(staged.width(), diag_eval as u64, e_ring_diag as u64);

    RingPeriodOutcome::Accepted {
        proposal: AutonomousRingProposal {
            candidate_circuit: staged,
            old_to_new,
            ring_delay_nodes,
            period: p,
            template: mu,
        },
        j_ring,
        e_ring_diag,
        j_ring_diag,
    }
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
    propose_autonomous_delay_ring_scored_with_telemetry(
        editable,
        obs_window,
        growth_config,
        economy_config,
        budget,
        out_port,
        telemetry,
    )
    .map(|(prop, _, _)| prop)
}

/// Evaluates autonomous delay-ring candidates and returns `(proposal, e_ring_diag, j_ring_diag)`.
pub fn propose_autonomous_delay_ring_scored_with_telemetry(
    editable: &EditableCircuit,
    obs_window: &ObservationWindow,
    growth_config: &GrowthConfig,
    economy_config: &EconomyConfig,
    budget: &mut TickBudget,
    out_port: u16,
    telemetry: &mut SproutTelemetry,
) -> Option<(AutonomousRingProposal, usize, f64)> {
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

    let mut best_ring: Option<(AutonomousRingProposal, f64, usize, f64)> = None;

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
            RingPeriodOutcome::Accepted {
                proposal,
                j_ring,
                e_ring_diag,
                j_ring_diag,
            } => {
                if best_ring
                    .as_ref()
                    .is_none_or(|(_, best_j, _, _)| j_ring < *best_j)
                {
                    best_ring = Some((proposal, j_ring, e_ring_diag, j_ring_diag));
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
            if let RingPeriodOutcome::Accepted { .. } = evaluate_ring_period(
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

    best_ring.map(|(prop, _, e_diag, j_diag)| (prop, e_diag, j_diag))
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
            deepening_max_lag: 8,
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

    #[test]
    fn s3a_neg_add_const_synthesizes_and_detects_cross_channel() {
        use crate::substrate::WireDelayMode;

        // S3-A: NegAddConst(u, 1) computes (1 - u) mod 4 == 3*u + 1 mod 4 at Width::BITS_2.
        let mut circuit = Circuit::new(Width::BITS_2, 2, 2, WireDelayMode::Combinational);
        let motif = CandidateMotif::NegAddConst(SourceFeature::BoundaryIn(0), 1);
        materialize_candidate(&mut circuit, &motif, 1);
        let compiled = circuit.compile().unwrap();
        let mut state = ScalarState::new(&compiled);
        let mut out = [0u64, 0u64];
        for x in 0..4u64 {
            state.step(&compiled, &[x, 0], &mut out).unwrap();
            let expected = (3 * x + 1) % 4;
            assert_eq!(out[1], expected, "1 - {x} mod 4 must equal {expected}");
        }

        // Cross-channel motif helper detects channel mismatch and matches same-channel motifs.
        assert!(motif_is_cross_channel(&motif, 1));
        assert!(!motif_is_cross_channel(&motif, 0));
        assert!(motif_is_cross_channel(
            &CandidateMotif::AddEqz(
                SourceFeature::BoundaryIn(1),
                SourceFeature::LagIn { channel: 0, tau: 2 },
            ),
            1,
        ));
        assert!(!motif_is_cross_channel(
            &CandidateMotif::AddEqz(
                SourceFeature::BoundaryIn(1),
                SourceFeature::LagYhat { port: 1, tau: 1 },
            ),
            1,
        ));
    }

    #[test]
    fn d2a_effective_juvenile_ticks_and_port_cone_register_depth() {
        use crate::substrate::WireDelayMode;

        let growth = GrowthConfig::default();
        let econ_zero = EconomyConfig {
            juvenile_ticks: 0,
            ..EconomyConfig::default()
        };
        assert_eq!(effective_juvenile_ticks(8, &econ_zero, &growth), 0);

        let econ = EconomyConfig::default();
        for d in 0..=3 {
            assert_eq!(
                effective_juvenile_ticks(d, &econ, &growth),
                econ.juvenile_ticks
            );
        }
        let d4 = effective_juvenile_ticks(4, &econ, &growth);
        let d5 = effective_juvenile_ticks(5, &econ, &growth);
        let d8 = effective_juvenile_ticks(8, &econ, &growth);
        assert_eq!(d4, 185);
        assert_eq!(d5, 244);
        assert!(d8 > d5 && d5 > d4 && d4 > econ.juvenile_ticks);

        // Unconnected port -> depth 0; 5-delay chain in Combinational -> depth 5.
        let mut circ_comb = Circuit::new(Width::BITS_1, 2, 2, WireDelayMode::Combinational);
        assert_eq!(port_cone_register_depth(&circ_comb, 1), 0);
        materialize_candidate(
            &mut circ_comb,
            &CandidateMotif::Identity(SourceFeature::LagIn { channel: 0, tau: 5 }),
            1,
        );
        assert_eq!(port_cone_register_depth(&circ_comb, 1), 5);

        // AddConst with ConstLoop cycle does not recurse endlessly and measures the delay arm.
        let mut circ_const = Circuit::new(Width::BITS_2, 2, 2, WireDelayMode::Combinational);
        materialize_candidate(
            &mut circ_const,
            &CandidateMotif::AddConst(SourceFeature::LagIn { channel: 0, tau: 2 }, 1),
            1,
        );
        assert_eq!(port_cone_register_depth(&circ_const, 1), 2);

        // TicksPerHop AddPair(LagIn(0, 3), LagIn(1, 5)):
        // arm 1 has BoundaryIn(1) -[1 hop]-> Delay(1 reg) -[1 hop]-> Add -[1 hop]-> Out(2) => depth 4.
        let mut circ_tph = Circuit::new(Width::BITS_2, 3, 3, WireDelayMode::TicksPerHop);
        materialize_candidate(
            &mut circ_tph,
            &CandidateMotif::AddPair(
                SourceFeature::LagIn { channel: 0, tau: 3 },
                SourceFeature::LagIn { channel: 1, tau: 5 },
            ),
            2,
        );
        assert_eq!(port_cone_register_depth(&circ_tph, 2), 4);
    }

    #[test]
    fn effective_max_nodes_and_mixed_candidate_loop_telemetry() {
        let growth = GrowthConfig::default();
        assert_eq!(effective_max_nodes(0, &growth), growth.max_nodes);
        assert_eq!(effective_max_nodes(1, &growth), growth.max_nodes);
        assert_eq!(effective_max_nodes(2, &growth), 64);
        assert_eq!(effective_max_nodes(4, &growth), 128);

        let small_growth = GrowthConfig {
            max_nodes: 8,
            deepening_max_lag: 12,
            ..GrowthConfig::default()
        };
        assert_eq!(effective_max_nodes(1, &small_growth), 8);
        // floor = 2 * 12 + 8 = 32 > max_nodes (8) => K * 32.
        assert_eq!(effective_max_nodes(3, &small_growth), 96);

        // Mixed loop: Eqz(BoundaryIn(0)) / Identity(LagIn(0, 2)) fit within max_nodes = 1 while
        // Eqz(LagIn(0, 2)) requires 2 nodes (BlockedMaxNodes). Proposal-level counter stays 0.
        let mixed_growth = GrowthConfig {
            diagnostic_window: 16,
            min_match_gain: 4,
            mi_threshold_bits: 0.01,
            max_nodes: 1,
            max_reactive_lag: 3,
            deepening_max_lag: 3,
            ..GrowthConfig::default()
        };
        let econ = EconomyConfig::default();
        let mut obs = ObservationWindow::new(1, 32);
        let mut diag = DiagnosticBuffer::new(16);
        for t in 0..32u64 {
            let x = t & 1;
            let prev_x = (t.wrapping_sub(1)) & 1;
            obs.push_frame(&[x]);
            if t >= 16 {
                diag.push_frame(&[prev_x], &[x], &[0], &[x], &[], &[]);
            }
        }
        let empty = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);
        let editable = EditableCircuit::new(empty, econ).unwrap();
        let mut budget = TickBudget::new(50_000);
        let mut tel = SproutTelemetry::default();
        let prop = propose_sprout_with_telemetry(
            &editable,
            &obs,
            &diag,
            &mixed_growth,
            &econ,
            &mut budget,
            0,
            mixed_growth.max_reactive_lag,
            &mut tel,
        )
        .unwrap();
        assert!(prop.is_some());
        assert_eq!(tel.sprout_blocked_max_nodes, 0);
    }

    #[test]
    fn shared_wire_tph_sibling_regression_is_rejected() {
        use crate::substrate::WireDelayMode;

        // ch1 cleanly learns d = 3 delay of ch0: BoundaryIn(0) -[1]-> Delay -[1]-> Out(1).
        let econ = EconomyConfig::default();
        let growth = GrowthConfig {
            diagnostic_window: 32,
            min_match_gain: 4,
            mi_threshold_bits: 0.01,
            ..GrowthConfig::default()
        };
        let mut base = Circuit::new(Width::BITS_1, 2, 2, WireDelayMode::TicksPerHop);
        let d = base.add_node(Primitive::delay_zero());
        base.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: d, port: 0 },
        );
        base.connect(
            SignalSource::NodeOut { node: d, port: 0 },
            SignalSink::BoundaryOut(1),
        );
        let editable = EditableCircuit::new(base, econ).unwrap();

        let mut obs = ObservationWindow::new(2, 128);
        let mut diag = DiagnosticBuffer::new(32);
        let mut lfsr = 0x15u64;
        let mut seq = Vec::new();
        for _ in 0..96 {
            let bit = lfsr & 1;
            let fb = (lfsr ^ (lfsr >> 2)) & 1;
            lfsr = (lfsr >> 1) | (fb << 4);
            seq.push(bit);
        }
        for t in 4..96usize {
            let cur = [seq[t], seq[t - 4]];
            obs.push_frame(&cur);
            if t >= 64 {
                let prev = [seq[t - 1], seq[t - 5]];
                let pred = [0, seq[t - 4]];
                let res = [cur[0], 0];
                diag.push_frame(&prev, &cur, &pred, &res, &[], &[]);
            }
        }
        let baseline = replay_current_baseline(&editable, &obs, &growth, &econ, 0, true);
        assert_eq!(baseline.driven_ports, vec![false, true]);
        assert_eq!(baseline.e_curr_by_port[1], 0);

        // Splicing BoundaryIn(0) for port 0 via tap_source adds a Split hop and shifts port 1 from d=3 to d=4.
        let outcome = evaluate_sprout_motif(
            &editable,
            &obs,
            &growth,
            &econ,
            0,
            &CandidateMotif::Eqz(SourceFeature::BoundaryIn(0)),
            true,
            true,
            &baseline,
            None,
            None,
        );
        assert!(matches!(outcome, SproutMotifOutcome::Rejected));
    }
}
