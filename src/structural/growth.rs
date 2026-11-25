//! Finite-ring residual diagnostics for bounded corrector growth.

use std::collections::{BTreeMap, BTreeSet};

use crate::baselines::{ObservationWindow, TickBudget};
use crate::metrics::mdl::StrippedRingCodeBase;
use crate::metrics::{PointForecastScorer, circuit_code_bits_with_tax, facts_code_bits};
use crate::rng::SeedBank;
use crate::structural::economy::{
    ComparatorSite, EconomyConfig, EconomyState, score_with_reward_mode,
};
use crate::structural::graph_edit::{
    build_autonomous_ring, build_autonomous_ring_source, cone_channel_mask, derivation_cone_info,
    is_pure_boundary_split, materialize_const_loop, source_has_cycle, source_register_depth,
    splice_comparator_onto_port, stack_output_binary_op, swap_comparator_driver, tap_source,
};
use crate::structural::live::node_output_values;
use crate::structural::sensitivity::{
    ComparatorTarget, assemble_causal_ancestry, assemble_causal_ancestry_with_comparators,
    counterfactual_sensitivity_audited, counterfactual_sensitivity_with_comparators_audited,
    exact_wake_sensitivity_node_evals,
};
use crate::structural::sleep::SleepSchedule;
use crate::structural::surgery::{
    EditableCircuit, MarginalDebtLedger, explicit_delay_register_index, port_cone_changed,
    port_upstream_cone_and_edges, port_uses_boundary_in, register_mapping,
};
use crate::substrate::{
    Circuit, CompiledCircuit, Edge, NodeId, Primitive, ScalarState, SignalSink, SignalSource,
    Width, WireDelayMode,
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

/// Derived causal-depth-scaled statistical maturation floor (`d2-A`, §4.3; `M2B-B1`).
///
/// Reduces identically to `economy_config.juvenile_ticks` ($t_{\text{juv,min}}$; `0` in
/// conservation tests) when `juvenile_ticks == 0` or `d_cone <= max_reactive_lag`. For deeper
/// register chains, compensates for `d_cone` ticks of pipeline fill, retrograde discount
/// $\gamma^{-(d_{\text{cone}} - \tau_{\max})}$, and chain-sum normalization
/// $\frac{1 - \gamma^{d_{\text{cone}}}}{1 - \gamma^{\tau_{\max}}}$. Stage-2 and Stage-3 sprouts
/// extend above this floor whenever per-node debt amortization $\lceil \Delta L_v / \hat{r}_v^+ \rceil$
/// requires additional ticks within $2 W_{\text{obs}}$ (`M2B-B1`).
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

/// Number of unit-sensitivity combinational primitives (`Sub`, `Eqz`) in a comparator site.
pub const COMPARATOR_PRIMITIVE_COUNT: usize = 2;

/// Zero-free-parameter comparator maturation scaling law parameterized by comparator primitive count.
///
/// Reduces identically to [`effective_juvenile_ticks`] when `comp_primitive_count == 0` or `z_alt <= 0.0`.
#[must_use]
pub fn effective_comparator_juvenile_ticks_with_primitives(
    d_alt: usize,
    z_alt: f64,
    comp_primitive_count: usize,
    economy_config: &EconomyConfig,
    growth_config: &GrowthConfig,
) -> u32 {
    let base = effective_juvenile_ticks(d_alt, economy_config, growth_config);
    if base == 0 || comp_primitive_count == 0 || z_alt <= 0.0 || !z_alt.is_finite() {
        return base;
    }
    let scaled = (f64::from(base) * ((z_alt + comp_primitive_count as f64) / z_alt)).ceil();
    scaled.min(f64::from(u32::MAX)) as u32
}

/// Zero-free-parameter comparator maturation grace scaling law (Charter DR #40, `M2B-D1`).
///
/// Scales [`effective_juvenile_ticks`] by $\lceil \cdot (Z_{\text{alt}} + 2) / Z_{\text{alt}} \rceil$
/// to compensate exact share dilution caused by `Sub` and `Eqz` (`COMPARATOR_PRIMITIVE_COUNT = 2`).
#[must_use]
pub fn effective_comparator_juvenile_ticks(
    d_alt: usize,
    z_alt: f64,
    economy_config: &EconomyConfig,
    growth_config: &GrowthConfig,
) -> u32 {
    effective_comparator_juvenile_ticks_with_primitives(
        d_alt,
        z_alt,
        COMPARATOR_PRIMITIVE_COUNT,
        economy_config,
        growth_config,
    )
}

/// Derives the coordinate-free per-circuit node guardrail scaled linearly by channel count.
///
/// Reduces identically to `growth_config.max_nodes` when `num_channels <= 1`. For multi-channel
/// circuits (`num_channels > 1`), guarantees `(2 * deepening_max_lag + 8)` nodes per channel for
/// Stage-2 lag-deepened binary pairings, plus `(deepening_max_lag + COMPARATOR_PRIMITIVE_COUNT + 2)`
/// nodes per channel when `redundancy_sprouting` is enabled so Stage 3 can evaluate and maintain a
/// causal-disjoint backup derivation and comparator (`Sub`, `Eqz`, fan-out `Split`s) without
/// starving siblings.
#[must_use]
pub const fn effective_max_nodes(num_channels: u16, growth_config: &GrowthConfig) -> usize {
    if num_channels <= 1 {
        return growth_config.max_nodes;
    }
    let tau_deep = growth_config.deepening_max_lag as usize;
    let stage3_allowance = if growth_config.redundancy_sprouting {
        tau_deep + COMPARATOR_PRIMITIVE_COUNT + 2
    } else {
        0
    };
    let floor = (2 * tau_deep + 8) + stage3_allowance;
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

/// Wake structural excision mode (`M3-E1` + `M3-E2` vs. unamended `M2b` baseline ablation).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ExcisionMode {
    /// Unified zero-free-parameter wake excision (`M3-E1` target-anchored reachability &
    /// `W_diag` ring-identity bypass + `M3-E2` verified local pass-through excision).
    #[default]
    VerifiedLocalExcision,
    /// Unamended `M2b` D1b.1 single-node zero-sensitivity and `Delay{init:0}` `!ever_nonzero`
    /// bypass (retained solely for `G3.5(a)` paired comparative ablation).
    BaselineD1b1,
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
    /// Enable Stage 3 causal-disjoint redundancy sprouting and comparator formation (`M2b`).
    #[serde(default = "default_redundancy_sprouting")]
    pub redundancy_sprouting: bool,
    /// Wake structural excision policy (`M3-E1` + `M3-E2` vs. `BaselineD1b1` ablation).
    #[serde(default)]
    pub excision_mode: ExcisionMode,
    /// Wake–sleep schedule (`SleepSchedule::None` by default for backward-identical `I1` runs).
    #[serde(default)]
    pub sleep_schedule: SleepSchedule,
    /// Optional M4a-0 descriptive sensitivity override for $W_{\text{sleep}}$ (`None` uses $W_{\text{obs}}$).
    #[serde(default)]
    pub sleep_window_override: Option<u64>,
}

const fn default_redundancy_sprouting() -> bool {
    true
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
            redundancy_sprouting: true,
            excision_mode: ExcisionMode::VerifiedLocalExcision,
            sleep_schedule: SleepSchedule::None,
            sleep_window_override: None,
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
            && self.sleep_window_override.is_none_or(|w| w > 0)
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

/// Audited node-evaluation counters recorded during candidate growth replay and survival checks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct GrowthEvalAudit {
    pub candidate_replay_node_evals: u64,
    pub sensitivity_node_evals: u64,
    pub max_additional_node_evals: Option<u64>,
    pub budget_cap_hit: bool,
}

impl GrowthEvalAudit {
    #[must_use]
    pub fn total_evals(&self) -> u64 {
        self.candidate_replay_node_evals
            .saturating_add(self.sensitivity_node_evals)
    }

    #[must_use]
    pub fn can_afford(&self, additional: u64) -> bool {
        self.max_additional_node_evals
            .is_none_or(|cap| self.total_evals().saturating_add(additional) <= cap)
    }

    pub fn try_reserve_replay(&mut self, additional: u64) -> bool {
        if !self.can_afford(additional) {
            self.budget_cap_hit = true;
            return false;
        }
        self.candidate_replay_node_evals =
            self.candidate_replay_node_evals.saturating_add(additional);
        true
    }
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
    pub pre_step_snapshots: Vec<Option<(u64, Vec<u64>)>>,
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
            pre_step_snapshots: Vec::with_capacity(capacity),
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
        self.push_frame_inner(
            input, truth, prediction, residual, node_outs, sensitive, None,
        );
    }

    /// Appends a newly observed frame together with the pre-step `(structure_epoch, registers)`.
    #[allow(clippy::too_many_arguments)]
    pub fn push_frame_with_snapshot(
        &mut self,
        input: &[u64],
        truth: &[u64],
        prediction: &[u64],
        residual: &[u64],
        node_outs: &[u64],
        sensitive: &[bool],
        pre_epoch: u64,
        pre_registers: &[u64],
    ) {
        self.push_frame_inner(
            input,
            truth,
            prediction,
            residual,
            node_outs,
            sensitive,
            Some((pre_epoch, pre_registers)),
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn push_frame_inner(
        &mut self,
        input: &[u64],
        truth: &[u64],
        prediction: &[u64],
        residual: &[u64],
        node_outs: &[u64],
        sensitive: &[bool],
        snapshot: Option<(u64, &[u64])>,
    ) {
        #[inline]
        fn rotate_and_overwrite<T: Copy>(col: &mut [Vec<T>], data: &[T]) {
            col.rotate_left(1);
            if let Some(last) = col.last_mut() {
                last.clear();
                last.extend_from_slice(data);
            }
        }

        if self.capacity > 0 && self.inputs.len() >= self.capacity {
            rotate_and_overwrite(&mut self.inputs, input);
            rotate_and_overwrite(&mut self.truth, truth);
            rotate_and_overwrite(&mut self.predictions, prediction);
            rotate_and_overwrite(&mut self.residuals, residual);
            rotate_and_overwrite(&mut self.node_outputs, node_outs);
            rotate_and_overwrite(&mut self.counterfactual_sensitivity, sensitive);
            if !self.pre_step_snapshots.is_empty() {
                self.pre_step_snapshots.rotate_left(1);
                if let Some(last_slot) = self.pre_step_snapshots.last_mut() {
                    match (last_slot.as_mut(), snapshot) {
                        (Some((ep, buf)), Some((new_ep, new_regs))) => {
                            *ep = new_ep;
                            buf.clear();
                            buf.extend_from_slice(new_regs);
                        }
                        (_, Some((new_ep, new_regs))) => {
                            *last_slot = Some((new_ep, new_regs.to_vec()));
                        }
                        (_, None) => {
                            *last_slot = None;
                        }
                    }
                }
            } else {
                self.pre_step_snapshots
                    .push(snapshot.map(|(ep, regs)| (ep, regs.to_vec())));
            }
            return;
        }
        if self.capacity == 0 && !self.inputs.is_empty() {
            self.inputs.remove(0);
            self.truth.remove(0);
            self.predictions.remove(0);
            self.residuals.remove(0);
            self.node_outputs.remove(0);
            self.counterfactual_sensitivity.remove(0);
            if !self.pre_step_snapshots.is_empty() {
                self.pre_step_snapshots.remove(0);
            }
        }
        self.inputs.push(input.to_vec());
        self.truth.push(truth.to_vec());
        self.predictions.push(prediction.to_vec());
        self.residuals.push(residual.to_vec());
        self.node_outputs.push(node_outs.to_vec());
        self.counterfactual_sensitivity.push(sensitive.to_vec());
        self.pre_step_snapshots
            .push(snapshot.map(|(ep, regs)| (ep, regs.to_vec())));
    }

    /// Returns the pre-step `(structure_epoch, registers)` captured at frame 0, if present.
    #[must_use]
    pub fn first_frame_snapshot(&self) -> Option<(u64, &[u64])> {
        self.pre_step_snapshots
            .first()?
            .as_ref()
            .map(|(epoch, regs)| (*epoch, regs.as_slice()))
    }

    /// Remaps buffered node-indexed columns across a structural prune `old_to_new`.
    pub fn remap_nodes(&mut self, old_to_new: &[Option<NodeId>], new_node_count: usize) {
        for frame in &mut self.node_outputs {
            let mut remapped = vec![0u64; new_node_count];
            for (old_idx, &new_opt) in old_to_new.iter().enumerate() {
                if let Some(new_id) = new_opt
                    && let Some(&val) = frame.get(old_idx)
                    && new_id.index() < new_node_count
                {
                    remapped[new_id.index()] = val;
                }
            }
            *frame = remapped;
        }
        for frame in &mut self.counterfactual_sensitivity {
            let mut remapped = vec![false; new_node_count];
            for (old_idx, &new_opt) in old_to_new.iter().enumerate() {
                if let Some(new_id) = new_opt
                    && let Some(&val) = frame.get(old_idx)
                    && new_id.index() < new_node_count
                {
                    remapped[new_id.index()] = val;
                }
            }
            *frame = remapped;
        }
        if old_to_new
            .iter()
            .enumerate()
            .any(|(idx, mapped)| mapped.map(NodeId::index) != Some(idx))
        {
            self.pre_step_snapshots.fill(None);
        }
    }

    /// Clears all buffered frames.
    pub fn clear(&mut self) {
        self.inputs.clear();
        self.truth.clear();
        self.predictions.clear();
        self.residuals.clear();
        self.node_outputs.clear();
        self.counterfactual_sensitivity.clear();
        self.pre_step_snapshots.clear();
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

    /// Returns mature nodes eligible for `M3-E1` idle/ring-identity pruning, including any
    /// mature nodes emitting constant ring-identity values (`0` for `Add`/`Sub.in(1)`/`Mul` or
    /// `1` for `Mul`) across all `W_diag` frames together with their exclusive upstream cones.
    #[must_use]
    pub fn idle_mature_nodes_for_circuit(
        &self,
        circuit: &Circuit,
        economy: &EconomyState,
    ) -> BTreeSet<NodeId> {
        let mut seed_idle = self.idle_mature_nodes(economy);
        if !self.is_full() {
            return seed_idle;
        }
        let node_count = circuit.node_count();
        if node_count == 0 {
            return seed_idle;
        }

        let protected: BTreeSet<NodeId> = economy
            .comparators()
            .iter()
            .flat_map(|c| [c.sub_node, c.eqz_node])
            .collect();
        // Never mark registered comparator nodes as idle seeds.
        seed_idle.retain(|n| !protected.contains(n));

        let width = circuit.width();
        let w_diag = self.len();
        let gamma = economy.config().trace_discount;
        let decay_floor = gamma.powi(w_diag as i32) * (1.0 + 1e-9);

        let mut out_edges: Vec<Vec<SignalSink>> = vec![Vec::new(); node_count];
        let mut in_sources: Vec<Vec<NodeId>> = vec![Vec::new(); node_count];
        for edge in circuit.edges() {
            if let SignalSource::NodeOut { node: src, .. } = edge.src
                && src.index() < node_count
            {
                out_edges[src.index()].push(edge.dst);
                if let SignalSink::NodeIn { node: dst, .. } = edge.dst
                    && dst.index() < node_count
                {
                    in_sources[dst.index()].push(src);
                }
            }
        }

        let mut valid_zero = BTreeSet::new();
        let mut valid_one = BTreeSet::new();
        for (node_idx, node_econ) in economy.nodes().iter().enumerate().take(node_count) {
            let node = NodeId(u32::try_from(node_idx).expect("fits in u32"));
            if node_econ.juvenile_ticks > 0 || protected.contains(&node) {
                continue;
            }
            let all_zero = self.node_outputs.iter().all(|frame| {
                frame
                    .get(node_idx)
                    .is_some_and(|&v| width.normalize(v) == 0)
            });
            let all_one = self.node_outputs.iter().all(|frame| {
                frame
                    .get(node_idx)
                    .is_some_and(|&v| width.normalize(v) == 1)
            });
            if all_zero && (node_econ.activity.nonzero_weight <= decay_floor || all_zero) {
                valid_zero.insert(node);
            }
            if all_one && (node_econ.activity.non_one_weight <= decay_floor || all_one) {
                valid_one.insert(node);
            }
        }

        loop {
            let mut removed_any = false;
            let current: Vec<NodeId> = valid_zero.iter().copied().collect();
            for node in current {
                let sinks = &out_edges[node.index()];
                let ok = !sinks.is_empty()
                    && sinks.iter().all(|&dst| match dst {
                        SignalSink::NodeIn { node: c, port } => {
                            if protected.contains(&c) {
                                return false;
                            }
                            match circuit.nodes().get(c.index()) {
                                Some(Primitive::Add | Primitive::Mul) => true,
                                Some(Primitive::Sub) => port == 1,
                                Some(Primitive::Split | Primitive::Delay { .. }) => {
                                    port == 0 && valid_zero.contains(&c)
                                }
                                _ => false,
                            }
                        }
                        SignalSink::BoundaryOut(_) => false,
                    });
                if !ok {
                    valid_zero.remove(&node);
                    removed_any = true;
                }
            }
            if !removed_any {
                break;
            }
        }

        loop {
            let mut removed_any = false;
            let current: Vec<NodeId> = valid_one.iter().copied().collect();
            for node in current {
                let sinks = &out_edges[node.index()];
                let ok = !sinks.is_empty()
                    && sinks.iter().all(|&dst| match dst {
                        SignalSink::NodeIn { node: c, port } => {
                            if protected.contains(&c) {
                                return false;
                            }
                            match circuit.nodes().get(c.index()) {
                                Some(Primitive::Mul) => true,
                                Some(Primitive::Split | Primitive::Delay { .. }) => {
                                    port == 0 && valid_one.contains(&c)
                                }
                                _ => false,
                            }
                        }
                        SignalSink::BoundaryOut(_) => false,
                    });
                if !ok {
                    valid_one.remove(&node);
                    removed_any = true;
                }
            }
            if !removed_any {
                break;
            }
        }

        seed_idle.extend(valid_zero);
        seed_idle.extend(valid_one);

        if seed_idle.is_empty() {
            return seed_idle;
        }

        // Expand `seed_idle` to include mature nodes in its exclusive upstream cone
        // (nodes upstream of `seed_idle` that cannot reach any BoundaryOut or ComparatorSite
        // without passing through `seed_idle`).
        let mut live_without_seed = vec![false; node_count];
        let mut stack: Vec<NodeId> = Vec::new();
        for edge in circuit.edges() {
            if let SignalSink::BoundaryOut(_) = edge.dst
                && let SignalSource::NodeOut { node, .. } = edge.src
                && node.index() < node_count
                && !seed_idle.contains(&node)
                && !live_without_seed[node.index()]
            {
                live_without_seed[node.index()] = true;
                stack.push(node);
            }
        }
        for &p_node in &protected {
            if p_node.index() < node_count
                && !seed_idle.contains(&p_node)
                && !live_without_seed[p_node.index()]
            {
                live_without_seed[p_node.index()] = true;
                stack.push(p_node);
            }
        }
        while let Some(node) = stack.pop() {
            for &pred in &in_sources[node.index()] {
                if !seed_idle.contains(&pred) && !live_without_seed[pred.index()] {
                    live_without_seed[pred.index()] = true;
                    stack.push(pred);
                }
            }
        }

        let mut upstream_of_seed = vec![false; node_count];
        let mut up_stack: Vec<NodeId> = seed_idle.iter().copied().collect();
        for &n in &up_stack {
            if n.index() < node_count {
                upstream_of_seed[n.index()] = true;
            }
        }
        while let Some(node) = up_stack.pop() {
            for &pred in &in_sources[node.index()] {
                if !upstream_of_seed[pred.index()] {
                    upstream_of_seed[pred.index()] = true;
                    up_stack.push(pred);
                }
            }
        }

        for (node_idx, node_econ) in economy.nodes().iter().enumerate().take(node_count) {
            let node = NodeId(u32::try_from(node_idx).expect("fits in u32"));
            if upstream_of_seed[node_idx]
                && !live_without_seed[node_idx]
                && !protected.contains(&node)
                && node_econ.juvenile_ticks == 0
            {
                seed_idle.insert(node);
            }
        }

        seed_idle
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

        // 2. Live internal node outputs (excluding juvenile nodes, §2.4, and comparator internals)
        for (node_idx, &primitive) in circuit.nodes().iter().enumerate() {
            let node = NodeId(u32::try_from(node_idx).expect("fits in u32"));
            if let Some(econ) = economy
                && (econ
                    .nodes()
                    .get(node_idx)
                    .is_some_and(|n| n.juvenile_ticks > 0)
                    || econ
                        .comparators()
                        .iter()
                        .any(|c| c.sub_node == node || c.eqz_node == node))
            {
                continue;
            }
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
    e_comp_curr: Vec<usize>,
    comp_cone_edges: Vec<Edge>,
    start_idx: usize,
    eval_start_idx: usize,
    eval_offset: usize,
    w_replay: usize,
    num_inputs: usize,
    can_fast_comb: bool,
    norm_inputs: Vec<u64>,
    out_pred_trace: Vec<u64>,
    out_truth_trace: Vec<u64>,
    survivor_map: Vec<Option<NodeId>>,
}

/// Returns all upstream edges feeding registered comparator cones in `TicksPerHop` mode.
///
/// Inserting a `Split` in series along any of these edges shifts register hop depth on an active
/// comparator branch. Empty when no comparators exist or wire delay is `Combinational`.
fn comparator_cone_edges(circuit: &Circuit, comparators: &[ComparatorSite]) -> Vec<Edge> {
    if comparators.is_empty() || circuit.wire_delay() != WireDelayMode::TicksPerHop {
        return Vec::new();
    }
    let mut visited = BTreeSet::new();
    let mut stack = Vec::new();
    for comp in comparators {
        for root in [comp.sub_node, comp.eqz_node] {
            if visited.insert(root) {
                stack.push(root);
            }
        }
    }
    let mut cone_edges = Vec::new();
    while let Some(node) = stack.pop() {
        for &edge in circuit.edges() {
            if let SignalSink::NodeIn { node: dst_node, .. } = edge.dst
                && dst_node == node
            {
                cone_edges.push(edge);
                if let SignalSource::NodeOut { node: src_node, .. } = edge.src
                    && visited.insert(src_node)
                {
                    stack.push(src_node);
                }
            }
        }
    }
    cone_edges.sort();
    cone_edges.dedup();
    cone_edges
}

#[allow(clippy::too_many_arguments)]
fn replay_current_baseline(
    editable: &EditableCircuit,
    obs_window: &ObservationWindow,
    growth_config: &GrowthConfig,
    economy_config: &EconomyConfig,
    out_port: u16,
    is_epoch_1: bool,
    audit: &mut GrowthEvalAudit,
) -> BaselineReplayMetrics {
    let w_diag = growth_config.diagnostic_window;
    let max_w_replay = obs_window.len().min(2 * w_diag);
    let raw_start_idx = obs_window.len() - max_w_replay;
    let eval_start_idx = obs_window.len() - w_diag;
    let cur_epoch = editable.structure_epoch();
    let start_idx = (raw_start_idx..=eval_start_idx)
        .find(|&idx| {
            obs_window
                .state_snapshot_at(idx)
                .is_some_and(|(snap_epoch, _)| snap_epoch == cur_epoch)
        })
        .unwrap_or(raw_start_idx);
    let w_replay = obs_window.len() - start_idx;
    let width = editable.circuit().width();

    let max_sym_count = frequencies(
        obs_window.frames()[eval_start_idx..]
            .iter()
            .map(|f| width.normalize(f[usize::from(out_port)])),
    )
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
    let num_inputs = usize::from(editable.compiled().num_inputs());
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
    let comparators = editable.economy().comparators();
    let mut e_comp_curr = vec![0usize; comparators.len()];
    let comp_cone_edges = comparator_cone_edges(editable.circuit(), comparators);
    let mut curr_scorer = PointForecastScorer::new(width, economy_config.null_model);

    let can_fast_comb = comparators.is_empty()
        && editable.circuit().wire_delay() == WireDelayMode::Combinational
        && is_epoch_1
            == !driven_ports
                .get(usize::from(out_port))
                .copied()
                .unwrap_or(false);
    let mut norm_inputs = if can_fast_comb {
        Vec::with_capacity(w_replay * num_inputs)
    } else {
        Vec::new()
    };
    let mut out_pred_trace = if can_fast_comb {
        Vec::with_capacity(w_replay)
    } else {
        Vec::new()
    };
    let mut out_truth_trace = if can_fast_comb {
        Vec::with_capacity(w_replay)
    } else {
        Vec::new()
    };

    let mut e_curr = 0usize;
    let mut flux_curr = 0.0;
    let curr_node_count = editable.circuit().node_count() as u64;
    for i in 0..w_replay {
        let idx = start_idx + i;
        let input = if idx == 0 {
            obs_window.prior_frame()
        } else {
            &obs_window.frames()[idx - 1]
        };
        let truth = &obs_window.frames()[idx];
        if !audit.try_reserve_replay(curr_node_count) {
            break;
        }
        let _ = curr_state.step(editable.compiled(), input, &mut curr_pred);
        let p_curr = if is_epoch_1 {
            0
        } else {
            curr_pred[usize::from(out_port)]
        };
        let p_truth = truth[usize::from(out_port)];
        if can_fast_comb {
            for &inp in input.iter().take(num_inputs) {
                norm_inputs.push(width.normalize(inp));
            }
            out_pred_trace.push(p_curr);
            out_truth_trace.push(p_truth);
        }
        let reward_curr = if is_epoch_1 {
            curr_scorer.null_model_mut().observe_and_surprisal(p_truth);
            0.0
        } else {
            score_with_reward_mode(
                &mut curr_scorer,
                economy_config.reward_mode,
                width,
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
            if !comparators.is_empty() {
                let wires = curr_state.wire_values();
                for (m, comp) in comparators.iter().enumerate() {
                    let sub_in = &editable.compiled().node_input_wires()[comp.sub_node.index()];
                    if sub_in.len() >= 2
                        && width.normalize(wires[sub_in[0]]) != width.normalize(wires[sub_in[1]])
                    {
                        e_comp_curr[m] += 1;
                    }
                }
            }
            if p_curr != p_truth {
                e_curr += 1;
            }
            flux_curr += reward_curr;
        }
    }

    let l_curr = circuit_code_bits_with_tax(editable.circuit(), 0, economy_config.split_code_tax);
    let facts_curr = facts_code_bits(width, w_diag as u64, e_curr as u64);
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
        e_comp_curr,
        comp_cone_edges,
        start_idx,
        eval_start_idx,
        eval_offset: eval_start_idx - start_idx,
        w_replay,
        num_inputs,
        can_fast_comb,
        norm_inputs,
        out_pred_trace,
        out_truth_trace,
        survivor_map,
    }
}

fn compute_cyclic_nodes(staged: &Circuit) -> Vec<bool> {
    let n = staged.node_count();
    let mut adj = vec![Vec::new(); n];
    for edge in staged.edges() {
        if let (SignalSource::NodeOut { node: u, .. }, SignalSink::NodeIn { node: v, .. }) =
            (edge.src, edge.dst)
            && u.index() < n
            && v.index() < n
        {
            adj[u.index()].push(v.index());
        }
    }
    let mut is_cyclic = vec![false; n];
    let mut visited = vec![false; n];
    let mut stack = Vec::new();
    for start in 0..n {
        visited.fill(false);
        stack.clear();
        stack.extend_from_slice(&adj[start]);
        while let Some(curr) = stack.pop() {
            if curr == start {
                is_cyclic[start] = true;
                break;
            }
            if !visited[curr] {
                visited[curr] = true;
                stack.extend_from_slice(&adj[curr]);
            }
        }
    }
    is_cyclic
}

fn acyclic_register_depth(
    staged: &Circuit,
    is_surviving_node: &[bool],
    is_cyclic_node: &[bool],
    stop_at_surviving: bool,
) -> usize {
    fn rec(
        circuit: &Circuit,
        node: NodeId,
        is_surviving_node: &[bool],
        is_cyclic_node: &[bool],
        stop_at_surviving: bool,
        memo: &mut [Option<usize>],
        on_path: &mut [bool],
    ) -> usize {
        let idx = node.index();
        if (stop_at_surviving && is_surviving_node.get(idx).copied().unwrap_or(false))
            || is_cyclic_node.get(idx).copied().unwrap_or(false)
            || on_path[idx]
        {
            return 0;
        }
        if let Some(cached) = memo[idx] {
            return cached;
        }
        on_path[idx] = true;
        let self_reg = usize::from(circuit.nodes()[idx].is_stateful());
        let is_tph = circuit.wire_delay() == WireDelayMode::TicksPerHop;
        let mut max_up = 0usize;
        for edge in circuit.edges() {
            if let SignalSink::NodeIn { node: dst, .. } = edge.dst
                && dst == node
            {
                let hop_regs = if is_tph { usize::from(edge.hops) } else { 0 };
                let up = match edge.src {
                    SignalSource::BoundaryIn(_) => hop_regs,
                    SignalSource::NodeOut { node: src, .. } => {
                        hop_regs
                            + rec(
                                circuit,
                                src,
                                is_surviving_node,
                                is_cyclic_node,
                                stop_at_surviving,
                                memo,
                                on_path,
                            )
                    }
                };
                max_up = max_up.max(up);
            }
        }
        on_path[idx] = false;
        let depth = self_reg + max_up;
        memo[idx] = Some(depth);
        depth
    }

    let n = staged.node_count();
    let mut memo = vec![None; n];
    let mut on_path = vec![false; n];
    let mut max_depth = 0usize;
    for idx in 0..n {
        if !is_surviving_node[idx] && !is_cyclic_node[idx] {
            max_depth = max_depth.max(rec(
                staged,
                NodeId(u32::try_from(idx).expect("node index fits in u32")),
                is_surviving_node,
                is_cyclic_node,
                stop_at_surviving,
                &mut memo,
                &mut on_path,
            ));
        }
    }
    if staged.wire_delay() == WireDelayMode::TicksPerHop {
        for edge in staged.edges() {
            if edge.hops > 0
                && matches!(edge.dst, SignalSink::BoundaryOut(_))
                && let SignalSource::NodeOut { node: src, .. } = edge.src
                && !is_surviving_node.get(src.index()).copied().unwrap_or(false)
                && !is_cyclic_node.get(src.index()).copied().unwrap_or(false)
            {
                let d = memo[src.index()].unwrap_or(0) + usize::from(edge.hops);
                max_depth = max_depth.max(d);
            }
        }
    }
    max_depth
}

fn non_surviving_depends_on_surviving_cyclic(
    staged: &Circuit,
    is_surviving_node: &[bool],
    is_cyclic_node: &[bool],
    protected_new_node: Option<NodeId>,
) -> bool {
    let n = staged.node_count();
    let mut incoming = vec![Vec::new(); n];
    for edge in staged.edges() {
        if let (SignalSource::NodeOut { node: src, .. }, SignalSink::NodeIn { node: dst, .. }) =
            (edge.src, edge.dst)
            && src.index() < n
            && dst.index() < n
        {
            incoming[dst.index()].push(src.index());
        }
    }

    let mut visited = vec![false; n];
    let mut stack = Vec::new();
    for (idx, seen) in visited.iter_mut().enumerate() {
        if !is_surviving_node.get(idx).copied().unwrap_or(false)
            && !is_cyclic_node.get(idx).copied().unwrap_or(false)
            && staged.nodes()[idx].is_stateful()
        {
            *seen = true;
            stack.push(idx);
        }
    }
    if staged.wire_delay() == WireDelayMode::TicksPerHop {
        for edge in staged.edges() {
            if edge.hops == 0 {
                continue;
            }
            let incident_to_protected = protected_new_node.is_some_and(|p_node| {
                matches!(edge.src, SignalSource::NodeOut { node, .. } if node == p_node)
                    || matches!(edge.dst, SignalSink::NodeIn { node, .. } if node == p_node)
            });
            let cyclic_internal = match (edge.src, edge.dst) {
                (SignalSource::NodeOut { node: s, .. }, SignalSink::NodeIn { node: d, .. }) => {
                    is_cyclic_node.get(s.index()).copied().unwrap_or(false)
                        && is_cyclic_node.get(d.index()).copied().unwrap_or(false)
                }
                _ => false,
            };
            if incident_to_protected || cyclic_internal {
                continue;
            }
            let feeds_non_surviving_acyclic = match edge.dst {
                SignalSink::NodeIn { node: dst, .. } => {
                    let dst_idx = dst.index();
                    dst_idx < n
                        && !is_surviving_node.get(dst_idx).copied().unwrap_or(false)
                        && !is_cyclic_node.get(dst_idx).copied().unwrap_or(false)
                }
                SignalSink::BoundaryOut(_) => matches!(
                    edge.src,
                    SignalSource::NodeOut { node: src, .. }
                        if src.index() < n
                            && !is_surviving_node.get(src.index()).copied().unwrap_or(false)
                            && !is_cyclic_node.get(src.index()).copied().unwrap_or(false)
                ),
            };
            if feeds_non_surviving_acyclic
                && let SignalSource::NodeOut { node: src, .. } = edge.src
                && src.index() < n
            {
                let src_idx = src.index();
                if is_surviving_node.get(src_idx).copied().unwrap_or(false)
                    && is_cyclic_node.get(src_idx).copied().unwrap_or(false)
                {
                    return true;
                }
                if !is_cyclic_node.get(src_idx).copied().unwrap_or(false) && !visited[src_idx] {
                    visited[src_idx] = true;
                    stack.push(src_idx);
                }
            }
        }
    }

    while let Some(curr) = stack.pop() {
        for &src in &incoming[curr] {
            let src_surviving = is_surviving_node.get(src).copied().unwrap_or(false);
            let src_cyclic = is_cyclic_node.get(src).copied().unwrap_or(false);
            if src_surviving && src_cyclic {
                return true;
            }
            if !src_cyclic && !visited[src] {
                visited[src] = true;
                stack.push(src);
            }
        }
    }
    false
}

struct PrimingPlan {
    is_surviving_node: Vec<bool>,
    is_cyclic_node: Vec<bool>,
    prime_start: usize,
    prime_steps: usize,
}

enum PrimingPlanOutcome {
    NotNeeded,
    Ready(PrimingPlan),
    Unavailable,
}

fn plan_non_surviving_register_priming(
    staged: &Circuit,
    old_to_new: &[Option<NodeId>],
    protected_new_node: Option<NodeId>,
    obs_window: &ObservationWindow,
    structure_epoch: u64,
    start_idx: usize,
    eval_start_idx: usize,
) -> PrimingPlanOutcome {
    if start_idx == 0 {
        return PrimingPlanOutcome::NotNeeded;
    }
    let n = staged.node_count();
    let mut is_surviving_node = vec![false; n];
    for mapped in old_to_new.iter().flatten() {
        if mapped.index() < n {
            is_surviving_node[mapped.index()] = true;
        }
    }
    if let Some(p_node) = protected_new_node
        && p_node.index() < n
    {
        is_surviving_node[p_node.index()] = true;
    }
    let is_cyclic_node = compute_cyclic_nodes(staged);
    let d_new = acyclic_register_depth(staged, &is_surviving_node, &is_cyclic_node, true);
    if eval_start_idx.saturating_sub(start_idx) >= d_new {
        return PrimingPlanOutcome::NotNeeded;
    }

    let prime_start_new = start_idx.saturating_sub(d_new);
    let has_new_snapshot = obs_window
        .state_snapshot_at(prime_start_new)
        .is_some_and(|(snap_epoch, _)| snap_epoch == structure_epoch);
    let prime_start = if has_new_snapshot {
        prime_start_new
    } else {
        if non_surviving_depends_on_surviving_cyclic(
            staged,
            &is_surviving_node,
            &is_cyclic_node,
            protected_new_node,
        ) {
            return PrimingPlanOutcome::Unavailable;
        }
        let d_full = acyclic_register_depth(staged, &is_surviving_node, &is_cyclic_node, false);
        start_idx.saturating_sub(d_full)
    };
    let prime_steps = start_idx.saturating_sub(prime_start);
    if prime_steps == 0 {
        return PrimingPlanOutcome::NotNeeded;
    }

    PrimingPlanOutcome::Ready(PrimingPlan {
        is_surviving_node,
        is_cyclic_node,
        prime_start,
        prime_steps,
    })
}

fn map_snapshot_regs_into(reg_map: &[Option<usize>], snap_regs: &[u64], dst: &mut [u64]) {
    for (old_r, new_r) in reg_map.iter().enumerate() {
        if let Some(nr) = new_r
            && old_r < snap_regs.len()
            && *nr < dst.len()
        {
            dst[*nr] = snap_regs[old_r];
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn prime_non_surviving_registers_with_plan(
    plan: &PrimingPlan,
    staged: &Circuit,
    staged_compiled: &CompiledCircuit,
    reg_map: &[Option<usize>],
    protected_new_node: Option<NodeId>,
    obs_window: &ObservationWindow,
    start_idx: usize,
    regs: &mut [u64],
    audit: &mut GrowthEvalAudit,
    seed_shadow_at: impl FnOnce(usize, &mut ScalarState) -> bool,
) -> bool {
    let mut is_untouched_reg = vec![false; regs.len()];
    for nr in reg_map.iter().flatten().copied() {
        if nr < is_untouched_reg.len() {
            is_untouched_reg[nr] = true;
        }
    }
    let mut exp_reg = 0usize;
    for (idx, prim) in staged.nodes().iter().enumerate() {
        if prim.is_stateful() {
            if exp_reg < is_untouched_reg.len()
                && (plan.is_surviving_node[idx] || plan.is_cyclic_node[idx])
            {
                is_untouched_reg[exp_reg] = true;
            }
            exp_reg += 1;
        }
    }
    if staged.wire_delay() == WireDelayMode::TicksPerHop {
        let mut base = staged_compiled.explicit_delay_count();
        for edge in staged.edges() {
            let hops = usize::from(edge.hops);
            let incident_to_protected = protected_new_node.is_some_and(|p_node| {
                matches!(edge.src, SignalSource::NodeOut { node, .. } if node == p_node)
                    || matches!(edge.dst, SignalSink::NodeIn { node, .. } if node == p_node)
            });
            let cyclic_internal = match (edge.src, edge.dst) {
                (SignalSource::NodeOut { node: s, .. }, SignalSink::NodeIn { node: d, .. }) => {
                    plan.is_cyclic_node.get(s.index()).copied().unwrap_or(false)
                        && plan.is_cyclic_node.get(d.index()).copied().unwrap_or(false)
                }
                _ => false,
            };
            if incident_to_protected || cyclic_internal {
                for s in 0..hops {
                    if base + s < is_untouched_reg.len() {
                        is_untouched_reg[base + s] = true;
                    }
                }
            }
            base += hops;
        }
    }

    let mut shadow = ScalarState::new(staged_compiled);
    if !seed_shadow_at(plan.prime_start, &mut shadow) {
        return false;
    }
    let mut dummy_pred = vec![0u64; usize::from(staged_compiled.num_outputs())];
    let staged_nodes = staged.node_count() as u64;
    for idx in plan.prime_start..start_idx {
        let input = if idx == 0 {
            obs_window.prior_frame()
        } else {
            &obs_window.frames()[idx - 1]
        };
        if !audit.try_reserve_replay(staged_nodes) {
            return false;
        }
        if shadow
            .step(staged_compiled, input, &mut dummy_pred)
            .is_err()
        {
            return false;
        }
    }
    let shadow_regs = shadow.registers();
    for (r, untouched) in is_untouched_reg.iter().enumerate() {
        if !*untouched && r < regs.len() && r < shadow_regs.len() {
            regs[r] = shadow_regs[r];
        }
    }
    true
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrimingOutcome {
    NotNeeded,
    Primed,
    Unavailable,
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn prime_non_surviving_registers(
    staged: &Circuit,
    staged_compiled: &CompiledCircuit,
    old_to_new: &[Option<NodeId>],
    reg_map: &[Option<usize>],
    protected_new_node: Option<NodeId>,
    obs_window: &ObservationWindow,
    structure_epoch: u64,
    start_idx: usize,
    eval_start_idx: usize,
    regs: &mut [u64],
) -> PrimingOutcome {
    let plan = match plan_non_surviving_register_priming(
        staged,
        old_to_new,
        protected_new_node,
        obs_window,
        structure_epoch,
        start_idx,
        eval_start_idx,
    ) {
        PrimingPlanOutcome::NotNeeded => return PrimingOutcome::NotNeeded,
        PrimingPlanOutcome::Unavailable => return PrimingOutcome::Unavailable,
        PrimingPlanOutcome::Ready(plan) => plan,
    };
    let mut dummy_audit = GrowthEvalAudit::default();
    if prime_non_surviving_registers_with_plan(
        &plan,
        staged,
        staged_compiled,
        reg_map,
        protected_new_node,
        obs_window,
        start_idx,
        regs,
        &mut dummy_audit,
        |prime_idx, shadow| {
            if let Some((snap_epoch, snap_regs)) = obs_window.state_snapshot_at(prime_idx)
                && snap_epoch == structure_epoch
            {
                let mut shadow_regs = shadow.registers().to_vec();
                map_snapshot_regs_into(reg_map, snap_regs, &mut shadow_regs);
                let _ = shadow.set_registers(staged_compiled, &shadow_regs);
            }
            true
        },
    ) {
        PrimingOutcome::Primed
    } else {
        PrimingOutcome::Unavailable
    }
}

enum SproutMotifOutcome {
    Accepted {
        circuit: Circuit,
        e_cand: usize,
        j_cand: f64,
        delta_flux: f64,
        is_1_op: bool,
        effective_juvenile_ticks: u32,
    },
    BlockedMaxNodes,
    Rejected,
    OutOfBudget,
}

#[derive(Clone, Copy)]
enum CombSourceSpec {
    BoundaryIn(usize),
    LagIn { channel: usize, tau: usize },
    LagYhat { tau: usize },
    LagRes { port: usize, tau_m1: usize },
}

impl CombSourceSpec {
    fn from_feature(
        sf: &SourceFeature,
        num_inputs: usize,
        out_port: u16,
        is_epoch_1: bool,
    ) -> Option<Self> {
        match sf {
            SourceFeature::BoundaryIn(c) if usize::from(*c) < num_inputs => {
                Some(Self::BoundaryIn(usize::from(*c)))
            }
            SourceFeature::LagIn { channel, tau } if usize::from(*channel) < num_inputs => {
                Some(Self::LagIn {
                    channel: usize::from(*channel),
                    tau: usize::from(*tau),
                })
            }
            SourceFeature::LagYhat { port, tau } if *port == out_port && !is_epoch_1 => {
                Some(Self::LagYhat {
                    tau: usize::from(*tau),
                })
            }
            SourceFeature::LagRes { port, tau }
                if *port == out_port && !is_epoch_1 && *tau >= 1 =>
            {
                Some(Self::LagRes {
                    port: usize::from(*port),
                    tau_m1: usize::from(*tau - 1),
                })
            }
            _ => None,
        }
    }

    const fn max_added_nodes(self) -> usize {
        match self {
            Self::BoundaryIn(_) => 1,
            Self::LagIn { tau, .. } | Self::LagYhat { tau } => tau + 1,
            Self::LagRes { tau_m1, .. } => tau_m1 + 5,
        }
    }

    const fn register_depth(self) -> usize {
        match self {
            Self::BoundaryIn(_) => 0,
            Self::LagIn { tau, .. } | Self::LagYhat { tau } => tau,
            Self::LagRes { tau_m1, .. } => tau_m1 + 1,
        }
    }

    #[inline(always)]
    fn eval_at(self, i: usize, width: Width, baseline: &BaselineReplayMetrics) -> u64 {
        let num_inputs = baseline.num_inputs;
        match self {
            Self::BoundaryIn(c) => baseline.norm_inputs[i * num_inputs + c],
            Self::LagIn { channel, tau } => {
                if i < tau {
                    0
                } else {
                    baseline.norm_inputs[(i - tau) * num_inputs + channel]
                }
            }
            Self::LagYhat { tau } => {
                if i < tau {
                    0
                } else {
                    baseline.out_pred_trace[i - tau]
                }
            }
            Self::LagRes { port, tau_m1 } => {
                if i < tau_m1 {
                    0
                } else {
                    let j = i - tau_m1;
                    let y_lag1 = if j == 0 {
                        0
                    } else {
                        baseline.out_pred_trace[j - 1]
                    };
                    width.sub(baseline.norm_inputs[j * num_inputs + port], y_lag1)
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
enum FastCombKind {
    ConstLoop(u64),
    Identity(CombSourceSpec),
    Eqz(CombSourceSpec),
    AddSelf(CombSourceSpec),
    AddConst(CombSourceSpec, u64),
    EqzAddConst(CombSourceSpec, u64),
    NegAddConst(CombSourceSpec, u64),
    Neg(CombSourceSpec),
    ScaleEqz(CombSourceSpec, u64),
    AddPair(CombSourceSpec, CombSourceSpec),
    SubPair(CombSourceSpec, CombSourceSpec),
    MulPair(CombSourceSpec, CombSourceSpec),
    AddEqz(CombSourceSpec, CombSourceSpec),
    MulEqz(CombSourceSpec, CombSourceSpec),
    MulEqzPair(CombSourceSpec, CombSourceSpec),
}

impl FastCombKind {
    const fn register_depth(self) -> usize {
        match self {
            Self::ConstLoop(_) => 0,
            Self::Identity(s)
            | Self::Eqz(s)
            | Self::AddSelf(s)
            | Self::AddConst(s, _)
            | Self::EqzAddConst(s, _)
            | Self::NegAddConst(s, _)
            | Self::Neg(s)
            | Self::ScaleEqz(s, _) => s.register_depth(),
            Self::AddPair(s1, s2)
            | Self::SubPair(s1, s2)
            | Self::MulPair(s1, s2)
            | Self::AddEqz(s1, s2)
            | Self::MulEqz(s1, s2)
            | Self::MulEqzPair(s1, s2) => {
                let d1 = s1.register_depth();
                let d2 = s2.register_depth();
                if d1 > d2 { d1 } else { d2 }
            }
        }
    }
}

struct FastCombPlan {
    kind: FastCombKind,
    is_epoch_1: bool,
    min_added_nodes: usize,
}

enum FastCombPrep {
    BlockedMaxNodes,
    Plan(FastCombPlan),
    Fallback,
}

const fn comb_source_min_nodes(sf: &SourceFeature) -> usize {
    match sf {
        SourceFeature::BoundaryIn(_) | SourceFeature::NodeOut { .. } => 0,
        SourceFeature::LagIn { tau, .. } | SourceFeature::LagYhat { tau, .. } => *tau as usize,
        SourceFeature::LagRes { tau, .. } => (*tau as usize).saturating_add(1),
    }
}

fn comb_motif_min_added_nodes(width: Width, motif: &CandidateMotif, is_epoch_1: bool) -> usize {
    let stack = usize::from(!is_epoch_1);
    match motif {
        CandidateMotif::ConstLoop(_) => 2 + stack,
        CandidateMotif::Identity(u) => comb_source_min_nodes(u) + stack,
        CandidateMotif::Eqz(u) => comb_source_min_nodes(u) + 1 + stack,
        CandidateMotif::AddSelf(u) => comb_source_min_nodes(u) + 2 + stack,
        CandidateMotif::AddConst(u, _) | CandidateMotif::NegAddConst(u, _) => {
            comb_source_min_nodes(u) + 3 + stack
        }
        CandidateMotif::EqzAddConst(u, _) => comb_source_min_nodes(u) + 4 + stack,
        CandidateMotif::Neg(u) => comb_source_min_nodes(u) + if !is_epoch_1 { 1 } else { 3 },
        CandidateMotif::ScaleEqz(u, c) => {
            if !is_epoch_1 && *c == width.mask() {
                comb_source_min_nodes(u) + 2
            } else {
                comb_source_min_nodes(u) + 4 + stack
            }
        }
        CandidateMotif::AddPair(u1, u2)
        | CandidateMotif::SubPair(u1, u2)
        | CandidateMotif::MulPair(u1, u2) => {
            comb_source_min_nodes(u1) + comb_source_min_nodes(u2) + 1 + stack
        }
        CandidateMotif::AddEqz(u1, u2) | CandidateMotif::MulEqz(u1, u2) => {
            comb_source_min_nodes(u1) + comb_source_min_nodes(u2) + 2 + stack
        }
        CandidateMotif::MulEqzPair(u1, u2) => {
            comb_source_min_nodes(u1) + comb_source_min_nodes(u2) + 3 + stack
        }
    }
}

impl FastCombPlan {
    fn prepare(
        circuit: &Circuit,
        motif: &CandidateMotif,
        baseline: &BaselineReplayMetrics,
        out_port: u16,
        is_epoch_1: bool,
        max_nodes: usize,
    ) -> FastCombPrep {
        if !baseline.can_fast_comb {
            return FastCombPrep::Fallback;
        }
        let width = circuit.width();
        let base_nodes = circuit.nodes().len();
        let min_added = comb_motif_min_added_nodes(width, motif, is_epoch_1);
        if base_nodes.saturating_add(min_added) > max_nodes {
            return FastCombPrep::BlockedMaxNodes;
        }
        let n_in = baseline.num_inputs;
        let src = |sf: &SourceFeature| CombSourceSpec::from_feature(sf, n_in, out_port, is_epoch_1);
        let stack_max = if is_epoch_1 { 2 } else { 3 };
        let Some((kind, max_added)) = (match motif {
            CandidateMotif::ConstLoop(c) => {
                Some((FastCombKind::ConstLoop(width.normalize(*c)), 2 + stack_max))
            }
            CandidateMotif::Identity(u) => src(u).map(|s| {
                let extra = if is_epoch_1 { 2 } else { 3 };
                (FastCombKind::Identity(s), s.max_added_nodes() + extra)
            }),
            CandidateMotif::Eqz(u) => {
                src(u).map(|s| (FastCombKind::Eqz(s), s.max_added_nodes() + 2 + stack_max))
            }
            CandidateMotif::AddSelf(u) => src(u).map(|s| {
                (
                    FastCombKind::AddSelf(s),
                    s.max_added_nodes() + 3 + stack_max,
                )
            }),
            CandidateMotif::AddConst(u, c) => src(u).map(|s| {
                (
                    FastCombKind::AddConst(s, width.normalize(*c)),
                    s.max_added_nodes() + 4 + stack_max,
                )
            }),
            CandidateMotif::EqzAddConst(u, c) => src(u).map(|s| {
                (
                    FastCombKind::EqzAddConst(s, width.normalize(*c)),
                    s.max_added_nodes() + 5 + stack_max,
                )
            }),
            CandidateMotif::NegAddConst(u, c) => src(u).map(|s| {
                (
                    FastCombKind::NegAddConst(s, width.normalize(*c)),
                    s.max_added_nodes() + 4 + stack_max,
                )
            }),
            CandidateMotif::Neg(u) => {
                src(u).map(|s| (FastCombKind::Neg(s), s.max_added_nodes() + 4 + stack_max))
            }
            CandidateMotif::ScaleEqz(u, c) => src(u).map(|s| {
                (
                    FastCombKind::ScaleEqz(s, width.normalize(*c)),
                    s.max_added_nodes() + 5 + stack_max,
                )
            }),
            CandidateMotif::AddPair(u1, u2) => src(u1).zip(src(u2)).map(|(s1, s2)| {
                (
                    FastCombKind::AddPair(s1, s2),
                    s1.max_added_nodes() + s2.max_added_nodes() + 3 + stack_max,
                )
            }),
            CandidateMotif::SubPair(u1, u2) => src(u1).zip(src(u2)).map(|(s1, s2)| {
                (
                    FastCombKind::SubPair(s1, s2),
                    s1.max_added_nodes() + s2.max_added_nodes() + 3 + stack_max,
                )
            }),
            CandidateMotif::MulPair(u1, u2) => src(u1).zip(src(u2)).map(|(s1, s2)| {
                (
                    FastCombKind::MulPair(s1, s2),
                    s1.max_added_nodes() + s2.max_added_nodes() + 3 + stack_max,
                )
            }),
            CandidateMotif::AddEqz(u1, u2) => src(u1).zip(src(u2)).map(|(s1, s2)| {
                (
                    FastCombKind::AddEqz(s1, s2),
                    s1.max_added_nodes() + s2.max_added_nodes() + 4 + stack_max,
                )
            }),
            CandidateMotif::MulEqz(u1, u2) => src(u1).zip(src(u2)).map(|(s1, s2)| {
                (
                    FastCombKind::MulEqz(s1, s2),
                    s1.max_added_nodes() + s2.max_added_nodes() + 4 + stack_max,
                )
            }),
            CandidateMotif::MulEqzPair(u1, u2) => src(u1).zip(src(u2)).map(|(s1, s2)| {
                (
                    FastCombKind::MulEqzPair(s1, s2),
                    s1.max_added_nodes() + s2.max_added_nodes() + 5 + stack_max,
                )
            }),
        }) else {
            return FastCombPrep::Fallback;
        };
        let _ = min_added;
        if base_nodes.saturating_add(max_added) > max_nodes {
            return FastCombPrep::Fallback;
        }
        if baseline.start_idx > 0 && kind.register_depth() > baseline.eval_offset {
            return FastCombPrep::Fallback;
        }
        FastCombPrep::Plan(Self {
            kind,
            is_epoch_1,
            min_added_nodes: min_added,
        })
    }

    fn exceeds_error_upper_audited(
        &self,
        baseline: &BaselineReplayMetrics,
        width: Width,
        e_cand_upper: isize,
        audit: &mut GrowthEvalAudit,
    ) -> Result<bool, ()> {
        let mut e_cand = 0usize;
        let per_step_evals = self.min_added_nodes.max(1) as u64;
        for i in baseline.eval_offset..baseline.w_replay {
            if !audit.try_reserve_replay(per_step_evals) {
                return Err(());
            }
            let p_cand = match self.kind {
                FastCombKind::ConstLoop(c) => {
                    if self.is_epoch_1 {
                        c
                    } else {
                        width.add(baseline.out_pred_trace[i], c)
                    }
                }
                FastCombKind::Identity(u) => {
                    let v = u.eval_at(i, width, baseline);
                    if self.is_epoch_1 {
                        v
                    } else {
                        width.add(baseline.out_pred_trace[i], v)
                    }
                }
                FastCombKind::Eqz(u) => {
                    let v = width.eqz(u.eval_at(i, width, baseline));
                    if self.is_epoch_1 {
                        v
                    } else {
                        width.add(baseline.out_pred_trace[i], v)
                    }
                }
                FastCombKind::AddSelf(u) => {
                    let uv = u.eval_at(i, width, baseline);
                    let v = width.add(uv, uv);
                    if self.is_epoch_1 {
                        v
                    } else {
                        width.add(baseline.out_pred_trace[i], v)
                    }
                }
                FastCombKind::AddConst(u, c) => {
                    let v = width.add(u.eval_at(i, width, baseline), c);
                    if self.is_epoch_1 {
                        v
                    } else {
                        width.add(baseline.out_pred_trace[i], v)
                    }
                }
                FastCombKind::EqzAddConst(u, c) => {
                    let v = width.eqz(width.add(u.eval_at(i, width, baseline), c));
                    if self.is_epoch_1 {
                        v
                    } else {
                        width.add(baseline.out_pred_trace[i], v)
                    }
                }
                FastCombKind::NegAddConst(u, c) => {
                    let v = width.sub(c, u.eval_at(i, width, baseline));
                    if self.is_epoch_1 {
                        v
                    } else {
                        width.add(baseline.out_pred_trace[i], v)
                    }
                }
                FastCombKind::Neg(u) => {
                    let uv = u.eval_at(i, width, baseline);
                    if self.is_epoch_1 {
                        width.sub(0, uv)
                    } else {
                        width.sub(baseline.out_pred_trace[i], uv)
                    }
                }
                FastCombKind::ScaleEqz(u, c) => {
                    let eq = width.eqz(u.eval_at(i, width, baseline));
                    if self.is_epoch_1 {
                        width.mul(eq, c)
                    } else {
                        width.add(baseline.out_pred_trace[i], width.mul(eq, c))
                    }
                }
                FastCombKind::AddPair(u1, u2) => {
                    let v = width.add(
                        u1.eval_at(i, width, baseline),
                        u2.eval_at(i, width, baseline),
                    );
                    if self.is_epoch_1 {
                        v
                    } else {
                        width.add(baseline.out_pred_trace[i], v)
                    }
                }
                FastCombKind::SubPair(u1, u2) => {
                    let v = width.sub(
                        u1.eval_at(i, width, baseline),
                        u2.eval_at(i, width, baseline),
                    );
                    if self.is_epoch_1 {
                        v
                    } else {
                        width.add(baseline.out_pred_trace[i], v)
                    }
                }
                FastCombKind::MulPair(u1, u2) => {
                    let v = width.mul(
                        u1.eval_at(i, width, baseline),
                        u2.eval_at(i, width, baseline),
                    );
                    if self.is_epoch_1 {
                        v
                    } else {
                        width.add(baseline.out_pred_trace[i], v)
                    }
                }
                FastCombKind::AddEqz(u1, u2) => {
                    let v = width.add(
                        u1.eval_at(i, width, baseline),
                        width.eqz(u2.eval_at(i, width, baseline)),
                    );
                    if self.is_epoch_1 {
                        v
                    } else {
                        width.add(baseline.out_pred_trace[i], v)
                    }
                }
                FastCombKind::MulEqz(u1, u2) => {
                    let v = width.mul(
                        u1.eval_at(i, width, baseline),
                        width.eqz(u2.eval_at(i, width, baseline)),
                    );
                    if self.is_epoch_1 {
                        v
                    } else {
                        width.add(baseline.out_pred_trace[i], v)
                    }
                }
                FastCombKind::MulEqzPair(u1, u2) => {
                    let v = width.mul(
                        width.eqz(u1.eval_at(i, width, baseline)),
                        width.eqz(u2.eval_at(i, width, baseline)),
                    );
                    if self.is_epoch_1 {
                        v
                    } else {
                        width.add(baseline.out_pred_trace[i], v)
                    }
                }
            };
            if p_cand != baseline.out_truth_trace[i] {
                e_cand += 1;
                if (e_cand as isize) > e_cand_upper {
                    return Ok(true);
                }
            }
        }
        Ok((e_cand as isize) > e_cand_upper)
    }
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
    mut budget: Option<&mut TickBudget>,
    mut telemetry: Option<&mut SproutTelemetry>,
    audit: &mut GrowthEvalAudit,
) -> SproutMotifOutcome {
    let w_diag = growth_config.diagnostic_window;
    let width = editable.circuit().width();
    let max_nodes = effective_max_nodes(editable.circuit().num_inputs(), growth_config);
    let is_epoch1_non_const = is_epoch_1 && !matches!(motif, CandidateMotif::ConstLoop(_));
    let is_binary_eqz_scaffold = is_epoch1_non_const
        && width == Width::BITS_1
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

    let mut budget_consumed = false;
    match FastCombPlan::prepare(
        editable.circuit(),
        motif,
        baseline,
        out_port,
        is_epoch_1,
        max_nodes,
    ) {
        FastCombPrep::BlockedMaxNodes => return SproutMotifOutcome::BlockedMaxNodes,
        FastCombPrep::Plan(plan) => {
            if let Some(b) = budget.as_deref_mut()
                && !b.try_consume(baseline.w_replay as u64)
            {
                if let Some(tel) = telemetry {
                    tel.sprout_budget_exhausted += 1;
                }
                return SproutMotifOutcome::OutOfBudget;
            }
            budget_consumed = true;
            match plan.exceeds_error_upper_audited(baseline, width, e_cand_upper, audit) {
                Ok(true) => return SproutMotifOutcome::Rejected,
                Ok(false) => {}
                Err(()) => {
                    if let Some(tel) = telemetry {
                        tel.sprout_budget_exhausted += 1;
                    }
                    return SproutMotifOutcome::OutOfBudget;
                }
            }
        }
        FastCombPrep::Fallback => {}
    }

    let mut staged = editable.circuit().clone();
    materialize_candidate(&mut staged, motif, out_port);
    if staged.nodes().len() > max_nodes {
        return SproutMotifOutcome::BlockedMaxNodes;
    }
    if !baseline.comp_cone_edges.is_empty()
        && baseline
            .comp_cone_edges
            .iter()
            .any(|edge| !staged.edges().contains(edge))
    {
        return SproutMotifOutcome::Rejected;
    }
    let start_snap = obs_window
        .state_snapshot_at(baseline.start_idx)
        .filter(|&(snap_epoch, _)| snap_epoch == editable.structure_epoch());
    let priming_plan = if start_snap.is_some() {
        match plan_non_surviving_register_priming(
            &staged,
            &baseline.survivor_map,
            None,
            obs_window,
            editable.structure_epoch(),
            baseline.start_idx,
            baseline.eval_start_idx,
        ) {
            PrimingPlanOutcome::NotNeeded => None,
            PrimingPlanOutcome::Ready(plan) => Some(plan),
            PrimingPlanOutcome::Unavailable => {
                if let Some(tel) = telemetry {
                    tel.candidate_undecidable += 1;
                }
                return SproutMotifOutcome::Rejected;
            }
        }
    } else {
        None
    };
    let prime_steps = priming_plan.as_ref().map_or(0, |p| p.prime_steps);
    let prime_eval_cost = (prime_steps as u64).saturating_mul(staged.node_count() as u64);
    if !audit.can_afford(prime_eval_cost) {
        audit.budget_cap_hit = true;
        if let Some(tel) = telemetry {
            tel.sprout_budget_exhausted += 1;
        }
        return SproutMotifOutcome::OutOfBudget;
    }
    let replay_ticks = if budget_consumed {
        0
    } else {
        baseline.w_replay
    };
    let needed_ticks = (replay_ticks + prime_steps) as u64;
    if needed_ticks > 0
        && let Some(b) = budget
        && !b.try_consume(needed_ticks)
    {
        if let Some(tel) = telemetry {
            tel.sprout_budget_exhausted += 1;
        }
        return SproutMotifOutcome::OutOfBudget;
    }
    let Ok(staged_compiled) = staged.compile() else {
        return SproutMotifOutcome::Rejected;
    };

    let mut state = ScalarState::new(&staged_compiled);
    if let Some((_, snap_regs)) = start_snap {
        let reg_map = register_mapping(
            editable.circuit(),
            &staged,
            editable.compiled(),
            &staged_compiled,
            &baseline.survivor_map,
        );
        let mut staged_regs = state.registers().to_vec();
        map_snapshot_regs_into(&reg_map, snap_regs, &mut staged_regs);
        if let Some(ref plan) = priming_plan {
            let structure_epoch = editable.structure_epoch();
            if !prime_non_surviving_registers_with_plan(
                plan,
                &staged,
                &staged_compiled,
                &reg_map,
                None,
                obs_window,
                baseline.start_idx,
                &mut staged_regs,
                audit,
                |prime_idx, shadow| {
                    if let Some((prime_epoch, prime_snap_regs)) =
                        obs_window.state_snapshot_at(prime_idx)
                        && prime_epoch == structure_epoch
                    {
                        let mut shadow_regs = shadow.registers().to_vec();
                        map_snapshot_regs_into(&reg_map, prime_snap_regs, &mut shadow_regs);
                        let _ = shadow.set_registers(&staged_compiled, &shadow_regs);
                    }
                    true
                },
            ) {
                if audit.budget_cap_hit
                    && let Some(tel) = telemetry
                {
                    tel.sprout_budget_exhausted += 1;
                }
                return if audit.budget_cap_hit {
                    SproutMotifOutcome::OutOfBudget
                } else {
                    SproutMotifOutcome::Rejected
                };
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

    let out_idx = usize::from(out_port);
    let mut e_cand = 0usize;
    let comparators = editable.economy().comparators();
    let mut e_comp_cand = if comparators.is_empty() {
        Vec::new()
    } else {
        vec![0usize; comparators.len()]
    };

    let staged_nodes = staged.node_count() as u64;
    for (i, trace_slot) in p_cand_trace.iter_mut().enumerate() {
        let idx = baseline.start_idx + i;
        let input = if idx == 0 {
            obs_window.prior_frame()
        } else {
            &obs_window.frames()[idx - 1]
        };
        let truth = &obs_window.frames()[idx];
        if !audit.try_reserve_replay(staged_nodes) {
            if let Some(tel) = telemetry {
                tel.sprout_budget_exhausted += 1;
            }
            return SproutMotifOutcome::OutOfBudget;
        }
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
            if !comparators.is_empty() {
                let wires = state.wire_values();
                for (m, comp) in comparators.iter().enumerate() {
                    let sub_in = &staged_compiled.node_input_wires()[comp.sub_node.index()];
                    if sub_in.len() >= 2
                        && width.normalize(wires[sub_in[0]]) != width.normalize(wires[sub_in[1]])
                    {
                        e_comp_cand[m] += 1;
                        if e_comp_cand[m] > baseline.e_comp_curr[m] {
                            return SproutMotifOutcome::Rejected;
                        }
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
    let eff_juv_base = effective_juvenile_ticks(d_cone, economy_config, growth_config);
    let old_n = editable.circuit().node_count();
    let new_nodes = staged.node_count().saturating_sub(old_n);

    if economy_config.juvenile_ticks > 0 && delta_l_sprout > 0.0 {
        let endowment = (new_nodes.max(1) as f64) * economy_config.initial_energy;
        let scaled_flux = (f64::from(eff_juv_base) / w_diag as f64) * pos_flux_cand;
        let projected_survival = endowment + scaled_flux;
        let horizon_limit = obs_window
            .capacity()
            .max(observation_horizon(economy_config, growth_config))
            .max((eff_juv_base as usize).saturating_add(2 * w_diag));
        if projected_survival <= delta_l_sprout {
            let delta_i_hat = pos_flux_cand / (w_diag as f64);
            if delta_i_hat > 0.0 {
                let w_star = delta_l_sprout / delta_i_hat;
                if w_star > (horizon_limit as f64)
                    && let Some(tel) = telemetry.as_deref_mut()
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

    let effective_juvenile_ticks = if economy_config.juvenile_ticks > 0 && new_nodes > 0 {
        let Some(t_juv) = verify_stage2_debt_amortization_and_survival(
            editable,
            obs_window,
            growth_config,
            economy_config,
            &staged,
            &staged_compiled,
            baseline,
            out_port,
            d_cone,
            eff_juv_base,
            telemetry.as_deref_mut(),
            audit,
        ) else {
            if audit.budget_cap_hit {
                if let Some(tel) = telemetry {
                    tel.sprout_budget_exhausted += 1;
                }
                return SproutMotifOutcome::OutOfBudget;
            }
            return SproutMotifOutcome::Rejected;
        };
        t_juv
    } else {
        eff_juv_base
    };

    SproutMotifOutcome::Accepted {
        circuit: staged,
        e_cand,
        j_cand,
        delta_flux,
        is_1_op,
        effective_juvenile_ticks,
    }
}

#[allow(clippy::too_many_arguments)]
fn replay_causal_tick(
    sim: &mut EditableCircuit,
    input: &[u64],
    truth: &[u64],
    predictions: &mut [u64],
    zeros_maint: &[f64],
    targets: &[ComparatorTarget],
    sample_count: usize,
    economy_config: &EconomyConfig,
    seed_bank: SeedBank,
    step_seed: u64,
    score_tick: bool,
    compute_ancestry: bool,
    comp_capture_idx: Option<usize>,
    sink_port: Option<usize>,
    audit: &mut GrowthEvalAudit,
) -> Option<(Vec<f64>, Option<f64>, Option<f64>)> {
    let num_outputs = usize::from(sim.compiled().num_outputs());
    if !audit.try_reserve_replay(sim.circuit().node_count() as u64) {
        return None;
    }
    sim.step(input, predictions).ok()?;
    let node_outs = node_output_values(sim.circuit(), sim.state());
    sim.economy_mut().record_activity(&node_outs).ok()?;
    if !compute_ancestry {
        return Some((Vec::new(), None, None));
    }
    let eff_samples = sample_count.min(RING_SIM_SENSITIVITY_SAMPLES);
    let est_sens =
        exact_wake_sensitivity_node_evals(sim.circuit(), sim.compiled(), eff_samples, targets);
    if !audit.can_afford(est_sens) {
        audit.budget_cap_hit = true;
        return None;
    }
    if targets.is_empty() {
        let snapshot = counterfactual_sensitivity_audited(
            sim.circuit(),
            sim.compiled(),
            sim.state(),
            seed_bank,
            step_seed,
            eff_samples,
            &mut audit.sensitivity_node_evals,
        )
        .ok()?;
        let ancestry = assemble_causal_ancestry(
            sim.circuit(),
            sim.compiled(),
            sim.state(),
            &snapshot,
            &sim.economy().activity(),
            sim.economy().register_eligibility(),
            economy_config.trace_discount,
        )
        .ok()?;
        if !score_tick {
            for (reg, trace) in ancestry.next_register_eligibility.iter().enumerate() {
                sim.economy_mut()
                    .set_register_eligibility(reg, trace)
                    .ok()?;
            }
            return Some((Vec::new(), None, None));
        }
        let z_sink =
            sink_port.and_then(|q| ancestry.port_shares.get(q).map(|s| s.iter().sum::<f64>()));
        let mut port_rewards = vec![0.0; num_outputs];
        for q in 0..num_outputs {
            if q < truth.len() && ancestry.port_shares[q].iter().sum::<f64>() > 0.0 {
                port_rewards[q] = sim
                    .economy_mut()
                    .score_port_single(q, predictions[q], truth[q])
                    .ok()?;
            } else if q < truth.len() {
                let _ = sim.economy_mut().observe_unconnected_port(q, truth[q]);
            }
        }
        let econ_tick = sim
            .economy_mut()
            .apply_causal_tick(&port_rewards, &ancestry, zeros_maint)
            .ok()?;
        Some((econ_tick.node_credit, None, z_sink))
    } else {
        let wires = if score_tick {
            Some(sim.state().wire_values().to_vec())
        } else {
            None
        };
        let snapshot = counterfactual_sensitivity_with_comparators_audited(
            sim.circuit(),
            sim.compiled(),
            sim.state(),
            seed_bank,
            step_seed,
            eff_samples,
            targets,
            &mut audit.sensitivity_node_evals,
        )
        .ok()?;
        let ancestry = assemble_causal_ancestry_with_comparators(
            sim.circuit(),
            sim.compiled(),
            sim.state(),
            &snapshot,
            &sim.economy().activity(),
            sim.economy().register_eligibility(),
            economy_config.trace_discount,
            targets,
            false,
        )
        .ok()?;
        let z_tick = comp_capture_idx.map(|m| {
            let comp = &sim.economy().comparators()[m];
            ancestry.comparator_shares[m]
                .iter()
                .enumerate()
                .filter(|&(u, _)| u != comp.sub_node.index() && u != comp.eqz_node.index())
                .map(|(_, &s)| s)
                .sum()
        });
        if !score_tick {
            for (reg, trace) in ancestry.base.next_register_eligibility.iter().enumerate() {
                sim.economy_mut()
                    .set_register_eligibility(reg, trace)
                    .ok()?;
            }
            return Some((Vec::new(), z_tick, None));
        }
        let z_sink = sink_port.and_then(|q| {
            ancestry
                .base
                .port_shares
                .get(q)
                .map(|s| s.iter().sum::<f64>())
        });
        let mut port_rewards = vec![0.0; num_outputs];
        for q in 0..num_outputs {
            if q < truth.len() && ancestry.base.port_shares[q].iter().sum::<f64>() > 0.0 {
                port_rewards[q] = sim
                    .economy_mut()
                    .score_port_single(q, predictions[q], truth[q])
                    .ok()?;
            } else if q < truth.len() {
                let _ = sim.economy_mut().observe_unconnected_port(q, truth[q]);
            }
        }
        let wires = wires.as_deref()?;
        let mut pri_vals = Vec::with_capacity(targets.len());
        let mut alt_vals = Vec::with_capacity(targets.len());
        let mut eqz_vals = Vec::with_capacity(targets.len());
        for comp in sim.economy().comparators() {
            let sub_in = &sim.compiled().node_input_wires()[comp.sub_node.index()];
            pri_vals.push(wires[sub_in[0]]);
            alt_vals.push(wires[sub_in[1]]);
            eqz_vals.push(node_outs[comp.eqz_node.index()]);
        }
        let fact_slice = truth.get(..num_outputs).unwrap_or(truth);
        let comp_obs = sim
            .economy_mut()
            .observe_comparators(&pri_vals, &alt_vals, &eqz_vals, Some(fact_slice))
            .ok()?;
        let econ_tick = sim
            .economy_mut()
            .apply_causal_tick_with_comparators(
                &port_rewards,
                &comp_obs.rewards,
                comp_obs.agreements,
                comp_obs.disagreements,
                &ancestry,
                zeros_maint,
            )
            .ok()?;
        Some((econ_tick.node_credit, z_tick, z_sink))
    }
}

/// Unified Stage-2 / Stage-3 decidability law for per-node debt amortization (`M2B-B1` + `M2B-C1`).
///
/// For the new-node set `V_new` (`!is_survivor`), with per-node marginal debt `ΔL_v`, measured positive
/// credit rate `r̂_v⁺` and measured mean sink causal ancestry `z_tot >= 1`:
///
/// * free-rider rejection: any `r̂_v⁺ <= 0` (or non-finite) rejects immediately;
/// * Clause (i) (subgraph epistemic crossover, anti-noise):
///   `t_cone = d_cone + ceil(Σ ΔL_v / Σ r̂_v⁺) <= 2 W_obs`;
/// * Clause (ii) (per-node causal-dilution ceiling, anti-parasite):
///   `t_juv = max(t_base, max_v (d_cone + ceil(ΔL_v / r̂_v⁺))) <= ceil(2 W_obs · max(t_base / t_juv,min, z_tot))`.
///
/// Either bound binding emits `candidate_undecidable += 1` and rejects.
#[allow(clippy::too_many_arguments)]
fn compute_debt_amortization_juvenile_ticks(
    is_survivor: &[bool],
    node_debts: &[f64],
    pos_credit_sums: &[f64],
    eval_ticks: usize,
    d_cone: usize,
    base_eff_juv: u32,
    z_tot: f64,
    economy_config: &EconomyConfig,
    growth_config: &GrowthConfig,
    telemetry: Option<&mut SproutTelemetry>,
) -> Option<u32> {
    if eval_ticks == 0 {
        return None;
    }
    let w_obs = observation_horizon(economy_config, growth_config);
    let ceiling = w_obs.saturating_mul(2);
    let eval_ticks_f64 = eval_ticks as f64;
    let mut max_amort_ticks = 0usize;
    let mut sum_debt = 0.0f64;
    let mut sum_rate = 0.0f64;
    let mut undecidable = false;
    for (idx, &surv) in is_survivor.iter().enumerate() {
        if surv {
            continue;
        }
        let r_hat_plus = pos_credit_sums.get(idx).copied().unwrap_or(0.0) / eval_ticks_f64;
        if !r_hat_plus.is_finite() || r_hat_plus <= 0.0 {
            return None;
        }
        let debt_v = node_debts.get(idx).copied().unwrap_or(0.0).max(0.0);
        sum_debt += debt_v;
        sum_rate += r_hat_plus;
        if debt_v > 0.0 {
            let ceil_ticks = (debt_v / r_hat_plus).ceil();
            if !ceil_ticks.is_finite() {
                undecidable = true;
            } else {
                max_amort_ticks = max_amort_ticks.max(ceil_ticks as usize);
            }
        }
    }
    let cone_amort = if sum_debt > 0.0 {
        (sum_debt / sum_rate).ceil()
    } else {
        0.0
    };
    let t_cone_ok = cone_amort.is_finite() && (d_cone as f64) + cone_amort <= ceiling as f64;

    let t_base = f64::from(base_eff_juv);
    let t_min = f64::from(economy_config.juvenile_ticks);
    let base_ratio = if t_min > 0.0 { t_base / t_min } else { 1.0 };
    let scale = base_ratio.max(z_tot).max(1.0);
    let h_node = ((ceiling as f64) * scale).ceil();
    let t_debt = d_cone.saturating_add(max_amort_ticks);
    let t_juv = (base_eff_juv as usize).max(t_debt);
    let t_node_ok = h_node.is_finite() && (t_debt as f64) <= h_node && (t_juv as f64) <= h_node;

    if undecidable || !t_cone_ok || !t_node_ok {
        if let Some(tel) = telemetry {
            tel.candidate_undecidable += 1;
        }
        return None;
    }
    u32::try_from(t_juv).ok()
}

#[allow(clippy::too_many_arguments)]
fn verify_stage2_debt_amortization_and_survival(
    editable: &EditableCircuit,
    obs_window: &ObservationWindow,
    growth_config: &GrowthConfig,
    economy_config: &EconomyConfig,
    staged: &Circuit,
    staged_compiled: &crate::substrate::CompiledCircuit,
    baseline: &BaselineReplayMetrics,
    out_port: u16,
    d_cone: usize,
    base_eff_juv: u32,
    telemetry: Option<&mut SproutTelemetry>,
    audit: &mut GrowthEvalAudit,
) -> Option<u32> {
    let old_n = editable.circuit().node_count();
    let old_to_new: Vec<Option<NodeId>> = (0..old_n).map(|i| Some(NodeId(i as u32))).collect();
    let mut sim_template = editable.clone();
    sim_template
        .recompile_preserving_state(staged.clone(), &old_to_new)
        .ok()?;
    let mut sim_ledger = MarginalDebtLedger {
        last_committed_code_bits: baseline.l_curr,
        ..MarginalDebtLedger::default()
    };
    sim_ledger.on_sprout(
        sim_template.circuit(),
        economy_config.split_code_tax,
        &old_to_new,
    );
    sim_template
        .economy_mut()
        .assign_sprout_debts(&sim_ledger.node_debts);

    let num_outputs = usize::from(staged_compiled.num_outputs());
    for q in 0..num_outputs {
        if port_cone_changed(
            editable.circuit(),
            sim_template.circuit(),
            &old_to_new,
            q as u16,
        ) {
            sim_template
                .economy_mut()
                .port_scorer_mut(q)
                .reset_predictor_counts();
        }
    }

    let node_count = sim_template.circuit().node_count();
    let is_survivor: Vec<bool> = (0..node_count).map(|idx| idx < old_n).collect();
    let targets = sim_template.comparator_targets();
    let zeros_maint = vec![0.0; node_count];
    let mut predictions = vec![0u64; num_outputs];
    let seed_bank = SeedBank::new(0);
    let sample_count = growth_config.sensitivity_sample_count;
    let w_obs = obs_window.len();
    let replay_start = baseline.start_idx;
    let eval_start = baseline.eval_start_idx;

    let seed_stage2_regs = |sim_mut: &mut EditableCircuit, align_idx: usize| {
        let fresh = ScalarState::new(staged_compiled);
        let mut staged_regs = fresh.registers().to_vec();
        if let Some((snap_epoch, snap_regs)) = obs_window.state_snapshot_at(align_idx)
            && snap_epoch == editable.structure_epoch()
        {
            let reg_map = register_mapping(
                editable.circuit(),
                staged,
                editable.compiled(),
                staged_compiled,
                &old_to_new,
            );
            for (old_r, new_r) in reg_map.iter().enumerate() {
                if let Some(nr) = new_r
                    && old_r < snap_regs.len()
                    && *nr < staged_regs.len()
                {
                    staged_regs[*nr] = snap_regs[old_r];
                }
            }
        }
        let _ = sim_mut.set_registers(&staged_regs);
    };

    // Pass 1: Replay `staged` over `obs_window` to measure per-node positive credit rate `r_hat_v^+`.
    let mut r_sim = sim_template.clone();
    for (idx, &surv) in is_survivor.iter().enumerate() {
        if !surv && let Some(node_econ) = r_sim.economy_mut().node_mut(idx) {
            node_econ.juvenile_ticks = (w_obs as u32).saturating_add(1);
        }
    }
    seed_stage2_regs(&mut r_sim, replay_start);
    let mut pos_credit_sums = vec![0.0; node_count];
    let mut z_tot_sum = 0.0f64;
    let mut eval_ticks = 0usize;
    for idx in replay_start..w_obs {
        let input = if idx == 0 {
            obs_window.prior_frame()
        } else {
            &obs_window.frames()[idx - 1]
        };
        let truth = &obs_window.frames()[idx];
        let score_tick = idx >= eval_start;
        let compute_ancestry = d_cone > 0 || score_tick;
        let (node_credit, _, z_sink) = replay_causal_tick(
            &mut r_sim,
            input,
            truth,
            &mut predictions,
            &zeros_maint,
            &targets,
            sample_count,
            economy_config,
            seed_bank,
            idx as u64,
            score_tick,
            compute_ancestry,
            None,
            Some(usize::from(out_port)),
            audit,
        )?;
        if score_tick {
            for (v, &c) in node_credit.iter().enumerate() {
                pos_credit_sums[v] += c.max(0.0);
            }
            z_tot_sum += z_sink.unwrap_or(0.0);
            eval_ticks += 1;
        }
    }
    let z_tot = if eval_ticks > 0 {
        (z_tot_sum / eval_ticks as f64).max(1.0)
    } else {
        1.0
    };

    let t_juv = compute_debt_amortization_juvenile_ticks(
        &is_survivor,
        &sim_ledger.node_debts,
        &pos_credit_sums,
        eval_ticks,
        d_cone,
        base_eff_juv,
        z_tot,
        economy_config,
        growth_config,
        telemetry,
    )?;

    // Pass 2: Simulate `t_juv` replay ticks with `juvenile_ticks = t_juv` on `V_new` and verify
    // `energy > sim_ledger.node_debts[idx]` for every new node prior to commit.
    let mut grad_sim = sim_template;
    let sim_circuit = grad_sim.circuit().clone();
    assign_stage3_juvenile_ticks(
        &sim_circuit,
        grad_sim.economy_mut(),
        &is_survivor,
        t_juv,
        economy_config,
        growth_config,
    );
    seed_stage2_regs(&mut grad_sim, replay_start);

    let j_ticks = t_juv as usize;
    let w_span = w_obs.saturating_sub(replay_start).max(1);
    let reactive_warmup = d_cone.min(w_span / 2);
    let settled_chunk = w_span.saturating_sub(reactive_warmup).max(1);
    let sim_node_count = sim_circuit.node_count() as u64;
    for step_idx in 0..j_ticks {
        let frame_idx = if step_idx < w_span {
            replay_start + step_idx
        } else {
            let offset = (step_idx - w_span) % settled_chunk;
            if offset == 0 {
                seed_stage2_regs(&mut grad_sim, replay_start);
                for w in 0..reactive_warmup {
                    let w_idx = replay_start + w;
                    let w_in = if w_idx == 0 {
                        obs_window.prior_frame()
                    } else {
                        &obs_window.frames()[w_idx - 1]
                    };
                    if !audit.try_reserve_replay(sim_node_count) {
                        return None;
                    }
                    grad_sim.step(w_in, &mut predictions).ok()?;
                }
            }
            replay_start + reactive_warmup + offset
        };
        let input = if frame_idx == 0 {
            obs_window.prior_frame()
        } else {
            &obs_window.frames()[frame_idx - 1]
        };
        let truth = &obs_window.frames()[frame_idx];
        replay_causal_tick(
            &mut grad_sim,
            input,
            truth,
            &mut predictions,
            &zeros_maint,
            &targets,
            sample_count,
            economy_config,
            seed_bank,
            step_idx as u64,
            true,
            true,
            None,
            None,
            audit,
        )?;
    }

    let survived = is_survivor
        .iter()
        .enumerate()
        .filter(|&(_, &surv)| !surv)
        .all(|(idx, _)| grad_sim.economy().nodes()[idx].energy > sim_ledger.node_debts[idx]);
    survived.then_some(t_juv)
}

fn for_each_u1_deeper_motif(
    u1: &SourceFeature,
    width: Width,
    mut f: impl FnMut(CandidateMotif) -> bool,
) -> bool {
    if f(CandidateMotif::Identity(u1.clone())) || f(CandidateMotif::Eqz(u1.clone())) {
        return true;
    }
    if width.bits() > 1 && f(CandidateMotif::AddSelf(u1.clone())) {
        return true;
    }
    let max_c = width.mask().min(SMALL_CONST_CANDIDATE_CAP);
    for k in 1..=max_c {
        if f(CandidateMotif::AddConst(u1.clone(), k)) {
            return true;
        }
        if width.bits() > 1
            && (f(CandidateMotif::EqzAddConst(u1.clone(), k))
                || f(CandidateMotif::NegAddConst(u1.clone(), k)))
        {
            return true;
        }
        if width.bits() > 1 && k > 1 && f(CandidateMotif::ScaleEqz(u1.clone(), k)) {
            return true;
        }
    }
    f(CandidateMotif::Neg(u1.clone()))
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
    audit: &mut GrowthEvalAudit,
) -> bool {
    if audit.budget_cap_hit {
        return false;
    }
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
        if audit.budget_cap_hit {
            return false;
        }
        for c in 0..usize::from(num_channels) {
            if audit.budget_cap_hit {
                return false;
            }
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
                if for_each_u1_deeper_motif(&u1, width, |motif| {
                    if audit.budget_cap_hit {
                        return false;
                    }
                    matches!(
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
                            audit,
                        ),
                        SproutMotifOutcome::Accepted { .. }
                    )
                }) {
                    return true;
                }
                if audit.budget_cap_hit {
                    return false;
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
    /// Unified per-node debt-amortization juvenile maturation horizon (`M2B-B1`).
    pub effective_juvenile_ticks: u32,
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
    propose_sprout_band_scored_internal_audited(
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
        compute_lag_bound,
        &mut GrowthEvalAudit::default(),
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn propose_sprout_band_scored_internal_audited(
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
    audit: &mut GrowthEvalAudit,
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
        audit,
    );
    if audit.budget_cap_hit {
        telemetry.sprout_budget_exhausted += 1;
        return Ok((None, false));
    }

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
            audit,
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

    let mut best_same: Option<(Circuit, usize, f64, f64, bool, u32)> = None;
    let mut best_cross: Option<(Circuit, usize, f64, f64, u32)> = None;
    let mut best_other: Option<(Circuit, usize, f64, f64, u32)> = None;
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
            audit,
        ) {
            SproutMotifOutcome::Accepted {
                circuit: staged,
                e_cand,
                j_cand,
                delta_flux,
                is_1_op,
                effective_juvenile_ticks,
            } => {
                if is_epoch_1 {
                    if motif_is_cross_channel(motif, out_port) {
                        let is_better = match &best_cross {
                            None => true,
                            Some((_, _, prev_j, prev_flux, _)) => {
                                is_better_mdl(j_cand, delta_flux, *prev_j, *prev_flux)
                            }
                        };
                        if is_better {
                            best_cross = Some((
                                staged,
                                e_cand,
                                j_cand,
                                delta_flux,
                                effective_juvenile_ticks,
                            ));
                        }
                    } else {
                        let is_better = match &best_same {
                            None => true,
                            Some((_, _, prev_j, prev_flux, prev_1_op, _)) => {
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
                            best_same = Some((
                                staged,
                                e_cand,
                                j_cand,
                                delta_flux,
                                is_1_op,
                                effective_juvenile_ticks,
                            ));
                        }
                    }
                } else {
                    let is_better = match &best_other {
                        None => true,
                        Some((_, _, prev_j, prev_flux, _)) => {
                            is_better_mdl(j_cand, delta_flux, *prev_j, *prev_flux)
                        }
                    };
                    if is_better {
                        best_other =
                            Some((staged, e_cand, j_cand, delta_flux, effective_juvenile_ticks));
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
                effective_juvenile_ticks: same.5,
            }),
            (None, Some(cross)) => Some(ScoredSproutProposal {
                circuit: cross.0,
                e_cand: cross.1,
                j_cand: cross.2,
                effective_juvenile_ticks: cross.4,
            }),
            (Some(same), Some(cross)) => {
                if is_better_mdl(cross.2, cross.3, same.2, same.3) {
                    Some(ScoredSproutProposal {
                        circuit: cross.0,
                        e_cand: cross.1,
                        j_cand: cross.2,
                        effective_juvenile_ticks: cross.4,
                    })
                } else {
                    Some(ScoredSproutProposal {
                        circuit: same.0,
                        e_cand: same.1,
                        j_cand: same.2,
                        effective_juvenile_ticks: same.5,
                    })
                }
            }
        }
    } else {
        best_other.map(|(circuit, e_cand, j_cand, _, effective_juvenile_ticks)| {
            ScoredSproutProposal {
                circuit,
                e_cand,
                j_cand,
                effective_juvenile_ticks,
            }
        })
    };

    let mut pending_deeper_check = false;
    if best_proposal.is_none() {
        if blocked_by_max_nodes > 0 {
            telemetry.sprout_blocked_max_nodes += 1;
        } else if audit.budget_cap_hit {
            // Wake-envelope node eval cap already tripped during search; skip diagnostic deeper-lag probe.
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
            audit,
        ) {
            telemetry.lag_horizon_bound += 1;
        }
    }

    Ok((best_proposal, pending_deeper_check))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn check_deeper_lag_for_port_audited(
    editable: &EditableCircuit,
    obs_window: &ObservationWindow,
    diag: &DiagnosticBuffer,
    growth_config: &GrowthConfig,
    economy_config: &EconomyConfig,
    out_port: u16,
    max_tau: u8,
    audit: &mut GrowthEvalAudit,
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
        audit,
    );
    if audit.budget_cap_hit {
        return false;
    }
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
        audit,
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

fn strip_circuit_for_ring(
    base: &Circuit,
    out_port: u16,
    comparators: &[ComparatorSite],
) -> (Circuit, Vec<Option<NodeId>>, Vec<NodeId>) {
    let (target_cone, _) = port_upstream_cone_and_edges(base, out_port);
    let mut protected_nodes = BTreeSet::new();
    for q in 0..base.num_outputs() {
        if q != out_port {
            let (other_cone, _) = port_upstream_cone_and_edges(base, q);
            protected_nodes.extend(other_cone);
        }
    }

    let collect_upstream = |root: SignalSource, out: &mut BTreeSet<NodeId>| {
        let mut stack = Vec::new();
        if let SignalSource::NodeOut { node, .. } = root
            && out.insert(node)
        {
            stack.push(node);
        }
        while let Some(node) = stack.pop() {
            for edge in base.edges() {
                if let SignalSink::NodeIn { node: dst, .. } = edge.dst
                    && dst == node
                    && let SignalSource::NodeOut { node: src, .. } = edge.src
                    && out.insert(src)
                {
                    stack.push(src);
                }
            }
        }
    };

    let mut detached_subs = Vec::new();
    let mut sub0_edges_to_detach = Vec::new();
    for comp in comparators {
        protected_nodes.insert(comp.sub_node);
        protected_nodes.insert(comp.eqz_node);
        if comp.target_port == out_port {
            if let Some(alt_edge) = base.edges().iter().find(|e| {
                e.dst
                    == SignalSink::NodeIn {
                        node: comp.sub_node,
                        port: 1,
                    }
            }) {
                collect_upstream(alt_edge.src, &mut protected_nodes);
            }
            if let Some(pri_edge) = base.edges().iter().find(|e| {
                e.dst
                    == SignalSink::NodeIn {
                        node: comp.sub_node,
                        port: 0,
                    }
            }) {
                detached_subs.push(comp.sub_node);
                sub0_edges_to_detach.push((pri_edge.src, pri_edge.dst));
            }
        } else {
            for port in [0u8, 1u8] {
                if let Some(edge) = base.edges().iter().find(|e| {
                    e.dst
                        == SignalSink::NodeIn {
                            node: comp.sub_node,
                            port,
                        }
                }) {
                    collect_upstream(edge.src, &mut protected_nodes);
                }
            }
        }
    }

    let removed: BTreeSet<NodeId> = target_cone
        .into_iter()
        .filter(|node| !protected_nodes.contains(node))
        .collect();

    let mut stripped = base.clone();
    let old_out_edges: Vec<(SignalSource, SignalSink)> = stripped
        .edges()
        .iter()
        .filter_map(|e| (e.dst == SignalSink::BoundaryOut(out_port)).then_some((e.src, e.dst)))
        .collect();
    for (src, dst) in old_out_edges {
        stripped.disconnect(src, dst);
    }
    for (src, dst) in sub0_edges_to_detach {
        stripped.disconnect(src, dst);
    }
    let old_to_new = stripped.remove_nodes(&removed);
    let reattach_subs: Vec<NodeId> = detached_subs
        .into_iter()
        .filter_map(|sub| old_to_new.get(sub.index()).copied().flatten())
        .collect();
    (stripped, old_to_new, reattach_subs)
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
    audit: &mut GrowthEvalAudit,
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

    let sim_node_count = sim.circuit().node_count() as u64;
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
        if !audit.try_reserve_replay(sim_node_count) {
            return false;
        }
        if sim.step(input, &mut predictions).is_err() {
            return false;
        }
        let node_outputs = node_output_values(sim.circuit(), sim.state());
        if sim.economy_mut().record_activity(&node_outputs).is_err() {
            return false;
        }
        let est_sens = exact_wake_sensitivity_node_evals(
            sim.circuit(),
            sim.compiled(),
            RING_SIM_SENSITIVITY_SAMPLES,
            &[],
        );
        if !audit.can_afford(est_sens) {
            audit.budget_cap_hit = true;
            return false;
        }
        let Ok(snapshot) = counterfactual_sensitivity_audited(
            sim.circuit(),
            sim.compiled(),
            sim.state(),
            seed_bank,
            step_idx as u64,
            RING_SIM_SENSITIVITY_SAMPLES,
            &mut audit.sensitivity_node_evals,
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

struct RingEvalPrecomp<'a> {
    stripped: &'a Circuit,
    old_to_new: &'a [Option<NodeId>],
    reattach_subs: &'a [NodeId],
    obs_out: &'a [u64],
    ring_mdl: &'a StrippedRingCodeBase,
}

#[allow(clippy::too_many_arguments)]
fn evaluate_ring_period(
    editable: &EditableCircuit,
    pre: &RingEvalPrecomp<'_>,
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
    audit: &mut GrowthEvalAudit,
) -> RingPeriodOutcome {
    let w_replay = obs_window.len();
    let out_idx = usize::from(out_port);
    let width = editable.circuit().width();
    let bits = width.bits();
    let start_tick = obs_window.start_tick();
    let start_mod = (start_tick as usize) % p;
    let mut mu = vec![0u64; p];
    let mut matched = 0usize;
    let mut exact_fast_counts = false;
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
        let mut r = start_mod;
        for &sym in pre.obs_out {
            counts[r * card + (sym as usize)] += 1;
            r += 1;
            if r == p {
                r = 0;
            }
        }
        for (r, mu_slot) in mu.iter_mut().enumerate() {
            let row = &counts[r * card..(r + 1) * card];
            let mut best_sym = 0u64;
            let mut best_cnt = row[0];
            for (sym, &cnt) in row.iter().enumerate().skip(1) {
                if cnt > best_cnt {
                    best_cnt = cnt;
                    best_sym = sym as u64;
                }
            }
            *mu_slot = best_sym;
            matched += best_cnt;
        }
        exact_fast_counts = true;
    } else {
        let mut residue_counts = vec![BTreeMap::<u64, usize>::new(); p];
        let mut r = start_mod;
        for &sym in pre.obs_out {
            *residue_counts[r].entry(sym).or_default() += 1;
            r += 1;
            if r == p {
                r = 0;
            }
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

    let max_nodes = effective_max_nodes(editable.circuit().num_inputs(), growth_config);
    if pre.stripped.nodes().len() + p + 1 + pre.reattach_subs.len() > max_nodes {
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

    let e_ring_upper = (e_curr as isize) - (growth_config.min_match_gain as isize);
    if exact_fast_counts && pre.reattach_subs.is_empty() {
        let e_ring_est = w_replay - matched;
        if (e_ring_est as isize) > e_ring_upper {
            return RingPeriodOutcome::Rejected;
        }
        let l_ring_est = pre.ring_mdl.code_bits_for_ring(&mu);
        let j_ring_est = l_ring_est + facts_code_bits(width, w_replay as u64, e_ring_est as u64);
        if j_ring_est >= j_curr {
            return RingPeriodOutcome::Rejected;
        }
        let mut scorer = PointForecastScorer::new(width, economy_config.null_model);
        let mut flux_ring = 0.0;
        let mut r = start_mod;
        for &truth_sym in pre.obs_out {
            flux_ring += score_with_reward_mode(
                &mut scorer,
                economy_config.reward_mode,
                width,
                mu[r],
                truth_sym,
            );
            r += 1;
            if r == p {
                r = 0;
            }
        }
        if flux_ring <= flux_curr {
            return RingPeriodOutcome::Rejected;
        }
    }

    let mut staged = pre.stripped.clone();
    let (ring_delay_nodes, _split) = build_autonomous_ring(&mut staged, p, &mu, out_port);
    for &sub_node in pre.reattach_subs {
        let Some(cur_out_src) = staged
            .edges()
            .iter()
            .find(|e| e.dst == SignalSink::BoundaryOut(out_port))
            .map(|e| e.src)
        else {
            return RingPeriodOutcome::Rejected;
        };
        let tapped = tap_source(&mut staged, cur_out_src);
        staged.connect(
            tapped,
            SignalSink::NodeIn {
                node: sub_node,
                port: 0,
            },
        );
    }
    if staged.nodes().len() > max_nodes {
        if let Some(tel) = telemetry {
            tel.sprout_blocked_max_nodes += 1;
        }
        return RingPeriodOutcome::Rejected;
    }
    let old_to_new = pre.old_to_new.to_vec();

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

    let staged_nodes = staged.node_count() as u64;
    for (i, frame) in obs_window.frames().iter().enumerate() {
        let input = if i == 0 {
            obs_window.prior_frame()
        } else {
            &obs_window.frames()[i - 1]
        };
        if !audit.try_reserve_replay(staged_nodes) {
            if let Some(tel) = telemetry {
                tel.sprout_budget_exhausted += 1;
            }
            return RingPeriodOutcome::OutOfBudget;
        }
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
        audit,
    ) {
        if audit.budget_cap_hit {
            if let Some(tel) = telemetry {
                tel.sprout_budget_exhausted += 1;
            }
            return RingPeriodOutcome::OutOfBudget;
        }
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
    propose_autonomous_delay_ring_scored_audited(
        editable,
        obs_window,
        growth_config,
        economy_config,
        budget,
        out_port,
        telemetry,
        &mut GrowthEvalAudit::default(),
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn propose_autonomous_delay_ring_scored_audited(
    editable: &EditableCircuit,
    obs_window: &ObservationWindow,
    growth_config: &GrowthConfig,
    economy_config: &EconomyConfig,
    budget: &mut TickBudget,
    out_port: u16,
    telemetry: &mut SproutTelemetry,
    audit: &mut GrowthEvalAudit,
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
    let curr_nodes = editable.circuit().node_count() as u64;
    for (i, frame) in obs_window.frames().iter().enumerate() {
        let input = if i == 0 {
            obs_window.prior_frame()
        } else {
            &obs_window.frames()[i - 1]
        };
        if !audit.try_reserve_replay(curr_nodes) {
            telemetry.sprout_budget_exhausted += 1;
            return None;
        }
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

    let (stripped_base, old_to_new_base, reattach_subs) = strip_circuit_for_ring(
        editable.circuit(),
        out_port,
        editable.economy().comparators(),
    );
    let ring_mdl = StrippedRingCodeBase::new(&stripped_base, economy_config.split_code_tax);
    let obs_out: Vec<u64> = obs_window.frames().iter().map(|f| f[out_idx]).collect();
    let ring_pre = RingEvalPrecomp {
        stripped: &stripped_base,
        old_to_new: &old_to_new_base,
        reattach_subs: &reattach_subs,
        obs_out: &obs_out,
        ring_mdl: &ring_mdl,
    };

    let mut best_ring: Option<(AutonomousRingProposal, f64, usize, f64)> = None;

    for p in growth_config.min_ring_period..=growth_config.max_ring_period {
        match evaluate_ring_period(
            editable,
            &ring_pre,
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
            audit,
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

    if best_ring.is_none() && !audit.budget_cap_hit {
        let probe_ceiling = (w_diag / 2).max(growth_config.max_ring_period);
        for p in (2..growth_config.min_ring_period)
            .chain((growth_config.max_ring_period + 1)..=probe_ceiling)
        {
            match evaluate_ring_period(
                editable,
                &ring_pre,
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
                audit,
            ) {
                RingPeriodOutcome::Accepted { .. } => {
                    telemetry.ring_period_bound += 1;
                    break;
                }
                RingPeriodOutcome::OutOfBudget => break,
                RingPeriodOutcome::Rejected => {}
            }
        }
    }

    best_ring.map(|(prop, _, e_diag, j_diag)| (prop, e_diag, j_diag))
}

/// Admissible Stage 3 causal-disjoint redundancy proposal (`M2B-D2`).
#[derive(Clone, Debug)]
pub struct RedundancySproutProposal {
    /// Complete staged circuit containing `alt`, `SUB`, `EQZ`, and comparator fan-out `Split`.
    pub circuit: Circuit,
    /// Node identity mapping from pre-sprout circuit (`0..old_node_count`) to `circuit`.
    pub old_to_new: Vec<Option<NodeId>>,
    /// Newly added `Primitive::Sub` node (`in(0) = primary`, `in(1) = alt`).
    pub sub_node: NodeId,
    /// Newly added `Primitive::Eqz` node (`in(0) = sub_node`).
    pub eqz_node: NodeId,
    /// Non-comparator nodes newly added for `alt` (plus any inserted `Split`).
    pub new_nodes: Vec<NodeId>,
    /// Register depth $d_{\text{alt}}$ of the `alt` cone feeding `sub_node.in(1)`.
    pub d_alt: usize,
    /// Mean Wake causal share sum $Z_{\text{alt}}$ over settled diagnostic ticks.
    pub z_alt: f64,
    /// Derived comparator juvenile maturation ticks from [`effective_comparator_juvenile_ticks`].
    pub effective_juvenile_ticks: u32,
    /// Channel bitmask of the primary derivation feeding `BoundaryOut(target_port)`.
    pub primary_channel_mask: u64,
    /// Channel bitmask of the newly proposed `alt` derivation (`0` if autonomous).
    pub alt_channel_mask: u64,
    /// Non-comparator diagnostic mismatches of `alt` over `W_diag`.
    pub e_alt_diag: usize,
    /// Objective MDL score $J_{\text{cand}}$ of `alt` over the replay window.
    pub j_cand: f64,
    /// Tier (`1` = channel-disjoint $C_{\text{alt}} \cap C_{\text{primary}} = \emptyset$, `2` = disjoint lag/state).
    pub tier: u8,
    /// Populated when `alt` is an autonomous delay-ring backup: `(period, template, delay_nodes)`.
    pub autonomous_ring: Option<(usize, Vec<u64>, Vec<NodeId>)>,
    /// Populated when splicing a comparator in `TicksPerHop` inserted a `Split` on an autonomous primary cycle.
    pub primary_tph_split: Option<NodeId>,
}

struct EvaluatedStage3Pre {
    staged: Circuit,
    old_to_new: Vec<Option<NodeId>>,
    sub_node: NodeId,
    eqz_node: NodeId,
    alt_src: SignalSource,
    alt_channel_mask: u64,
    e_alt_diag: usize,
    j_cand: f64,
    tier: u8,
    autonomous_ring: Option<(usize, Vec<u64>, Vec<NodeId>)>,
    primary_tph_split: Option<NodeId>,
}

fn port_comparator_root_sinks(
    _circuit: &Circuit,
    target_port: u16,
    comparator_sub_nodes: &[NodeId],
) -> Vec<SignalSink> {
    let mut sinks = vec![SignalSink::BoundaryOut(target_port)];
    for &sub_node in comparator_sub_nodes {
        sinks.push(SignalSink::NodeIn {
            node: sub_node,
            port: 1,
        });
    }
    sinks
}

fn feature_channel_mask(sf: &SourceFeature, circuit: &Circuit) -> Option<u64> {
    match sf {
        SourceFeature::BoundaryIn(c) | SourceFeature::LagIn { channel: c, .. } => {
            if usize::from(*c) < 64 {
                Some(1u64 << c)
            } else {
                Some(0)
            }
        }
        SourceFeature::NodeOut { node, port } => Some(cone_channel_mask(
            circuit,
            SignalSource::NodeOut {
                node: *node,
                port: *port,
            },
        )),
        SourceFeature::LagYhat { .. } | SourceFeature::LagRes { .. } => None,
    }
}

fn build_masked_stage3_pool(
    editable: &EditableCircuit,
    obs_window: &ObservationWindow,
    diag: &DiagnosticBuffer,
    out_port: u16,
    max_tau: u8,
    excl: &crate::structural::graph_edit::DerivationConeInfo,
    comparator_nodes: &BTreeSet<NodeId>,
) -> Vec<ResidualFeature> {
    let circuit = editable.circuit();
    let is_tph = circuit.wire_delay() == WireDelayMode::TicksPerHop;
    let raw = diag.build_signal_pool(
        circuit,
        obs_window,
        out_port,
        max_tau,
        Some(editable.economy()),
    );
    raw.into_iter()
        .filter(|feat| {
            let Some(sf) = SourceFeature::parse(&feat.name) else {
                return false;
            };
            match sf {
                SourceFeature::LagYhat { .. } | SourceFeature::LagRes { .. } => false,
                SourceFeature::NodeOut { node, port } => {
                    if excl.excl_nodes.contains(&node) || comparator_nodes.contains(&node) {
                        return false;
                    }
                    // Also reject if any reactive leaf in `node`'s cone collides with `L_excl`.
                    let mut dummy_sink = Vec::new();
                    for edge in circuit.edges() {
                        if edge.src == (SignalSource::NodeOut { node, port }) {
                            dummy_sink.push(edge.dst);
                        }
                    }
                    let node_info = if let Some(&dst) = dummy_sink.first() {
                        derivation_cone_info(circuit, &[dst])
                    } else {
                        derivation_cone_info(circuit, &[])
                    };
                    node_info
                        .excl_reactive_leaves
                        .is_disjoint(&excl.excl_reactive_leaves)
                }
                SourceFeature::BoundaryIn(c) => {
                    is_tph || !excl.excl_reactive_leaves.contains(&(c, 0))
                }
                SourceFeature::LagIn { channel, tau } => {
                    is_tph
                        || !excl
                            .excl_reactive_leaves
                            .contains(&(channel, usize::from(tau)))
                }
            }
        })
        .collect()
}

fn diagnose_stage3_values(
    width: Width,
    truth: &[u64],
    features: &[ResidualFeature],
    config: GrowthConfig,
    min_tau: u8,
    max_tau: u8,
) -> Result<ResidualDiagnostics, GrowthError> {
    if truth.is_empty() {
        return Err(GrowthError::EmptyTrace);
    }
    let truth_norm: Vec<u64> = truth.iter().map(|&v| width.normalize(v)).collect();
    let zero_error = truth_norm.iter().all(|&v| v == 0);
    let res_ctx = ResidualContext::new(&truth_norm, width, config.min_match_gain);
    let len = truth_norm.len();

    let filter_band = min_tau > 1;
    let mut in_band_flags = vec![false; features.len()];
    if filter_band {
        for (idx, feat) in features.iter().enumerate() {
            let lag = feature_lag(feat);
            in_band_flags[idx] = lag >= min_tau && lag <= max_tau;
        }
    }

    let mut norm_pool = vec![0u64; features.len() * len];
    for (idx, feat) in features.iter().enumerate() {
        let dst = &mut norm_pool[idx * len..(idx + 1) * len];
        for (slot, &val) in dst.iter_mut().zip(&feat.values) {
            *slot = width.normalize(val);
        }
    }

    let mut scratch_1: Vec<(u64, u64)> = Vec::new();
    let mut scratch_2: Vec<(u64, u64, u64)> = Vec::new();
    let mut scores = Vec::with_capacity(features.len() * (features.len() + 1) / 2);
    for first in 0..features.len() {
        let first_slice = &norm_pool[first * len..(first + 1) * len];
        let first_in_band = in_band_flags[first];
        if !filter_band || first_in_band {
            scores.push(score_singleton_normalized(
                &res_ctx,
                &truth_norm,
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
                    &truth_norm,
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
            .then_with(|| left.feature_indices.cmp(&right.feature_indices))
    });

    let passing_count = scores
        .iter()
        .filter(|s| {
            s.corrected_mutual_information_bits > config.mi_threshold_bits
                && s.match_gain >= config.min_match_gain
        })
        .count();
    let cap = config.candidate_feature_cap.max(1);
    let candidate_list_truncated = passing_count > cap;
    let candidate_features: Vec<FeatureScore> = scores
        .into_iter()
        .filter(|s| {
            s.corrected_mutual_information_bits > config.mi_threshold_bits
                && s.match_gain >= config.min_match_gain
        })
        .take(cap)
        .collect();
    let best_feature = candidate_features.first().cloned();
    let secondary_feature = candidate_features.get(1).cloned();
    let growth_criterion_met = !zero_error && best_feature.is_some();

    Ok(ResidualDiagnostics {
        sample_count: len,
        residual_entropy_bits: res_ctx.entropy_bits,
        constant_offset: res_ctx.constant_offset,
        zero_error,
        lagged_mutual_information_bits: Vec::new(),
        best_feature,
        secondary_feature,
        candidate_features,
        growth_criterion_met,
        candidate_list_truncated,
    })
}

/// Returns `true` if every non-boundary-`Split` node in `out_port`'s upstream cone is mature.
pub(crate) fn port_primary_cone_is_mature(
    circuit: &Circuit,
    economy: &EconomyState,
    out_port: u16,
) -> bool {
    let (cone_nodes, _) = port_upstream_cone_and_edges(circuit, out_port);
    cone_nodes.iter().all(|&node| {
        is_pure_boundary_split(circuit, node)
            || economy
                .nodes()
                .get(node.index())
                .is_none_or(|n| n.juvenile_ticks == 0)
    })
}

/// Assigns Stage 3 juvenile maturation ticks to every new node (`!is_survivor`) matching the
/// checked graduation horizon `eff_juv`.
pub(crate) fn assign_stage3_juvenile_ticks(
    _circuit: &Circuit,
    economy: &mut EconomyState,
    is_survivor: &[bool],
    eff_juv: u32,
    _economy_config: &EconomyConfig,
    _growth_config: &GrowthConfig,
) {
    for (idx, &surv) in is_survivor.iter().enumerate() {
        if surv {
            continue;
        }
        if let Some(node_econ) = economy.node_mut(idx) {
            node_econ.juvenile_ticks = eff_juv;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn stage3_measure_z_alt_and_survive(
    editable: &EditableCircuit,
    obs_window: &ObservationWindow,
    growth_config: &GrowthConfig,
    economy_config: &EconomyConfig,
    out_port: u16,
    primary_channel_mask: u64,
    pre: &EvaluatedStage3Pre,
    telemetry: &mut SproutTelemetry,
    audit: &mut GrowthEvalAudit,
) -> Option<(usize, f64, u32)> {
    let staged = &pre.staged;
    let is_tph = staged.wire_delay() == WireDelayMode::TicksPerHop;
    let d_alt = source_register_depth(staged, pre.alt_src) + usize::from(is_tph);
    let num_outputs = usize::from(staged.num_outputs());
    let w_replay = obs_window.len();
    let w_diag = growth_config.diagnostic_window.max(1);
    let eval_start = w_replay.saturating_sub(w_diag);

    let mut sim = editable.clone();
    sim.recompile_preserving_state(staged.clone(), &pre.old_to_new)
        .ok()?;
    let l_curr = circuit_code_bits_with_tax(editable.circuit(), 0, economy_config.split_code_tax);
    let mut sim_ledger = MarginalDebtLedger {
        last_committed_code_bits: l_curr,
        ..MarginalDebtLedger::default()
    };
    sim_ledger.on_sprout(
        sim.circuit(),
        economy_config.split_code_tax,
        &pre.old_to_new,
    );
    sim.economy_mut()
        .assign_sprout_debts(&sim_ledger.node_debts);

    let mut is_survivor = vec![false; sim.circuit().node_count()];
    for &mapped in pre.old_to_new.iter().flatten() {
        if mapped.index() < is_survivor.len() {
            is_survivor[mapped.index()] = true;
        }
    }

    let comp_idx = sim
        .register_comparator(
            pre.sub_node,
            pre.eqz_node,
            out_port,
            primary_channel_mask,
            pre.alt_channel_mask,
        )
        .ok()?;

    let seed_regs = |sim_mut: &mut EditableCircuit, align_tick: u64| -> bool {
        let mut regs = sim_mut.state().registers().to_vec();
        if let Some((snap_epoch, snap_regs)) = obs_window.state_snapshot_at(0)
            && snap_epoch == editable.structure_epoch()
        {
            if let Some(split_node) = pre.primary_tph_split
                && let Some(adj_regs) =
                    crate::structural::graph_edit::advance_tph_autonomous_ring_phase(
                        editable.circuit(),
                        editable.compiled(),
                        snap_regs,
                        sim_mut.circuit(),
                        sim_mut.compiled(),
                        &pre.old_to_new,
                        out_port,
                        split_node,
                        pre.sub_node,
                    )
            {
                regs = adj_regs;
            } else {
                let reg_map = register_mapping(
                    editable.circuit(),
                    sim_mut.circuit(),
                    editable.compiled(),
                    sim_mut.compiled(),
                    &pre.old_to_new,
                );
                for (old_r, new_r) in reg_map.iter().enumerate() {
                    if let Some(nr) = new_r
                        && old_r < snap_regs.len()
                    {
                        regs[*nr] = snap_regs[old_r];
                    }
                }
            }
        }
        if let Some((period, ref mu, ref delay_nodes)) = pre.autonomous_ring {
            for (j, &delay_node) in delay_nodes.iter().enumerate() {
                let Some(reg_idx) = explicit_delay_register_index(sim_mut.circuit(), delay_node)
                else {
                    return false;
                };
                regs[reg_idx] = mu[((align_tick as usize) + (period - 1 - j)) % period];
            }
        }
        sim_mut.set_registers(&regs).is_ok()
    };

    // Pass 1: Measure empirical steady-state `Z_alt` and per-node positive credit rates `r_hat_v^+`
    // over `eval_start..w_replay` (`M2B-D1` & `M2B-B1`).
    let mut z_sim = sim.clone();
    for (idx, &surv) in is_survivor.iter().enumerate() {
        if !surv && let Some(node_econ) = z_sim.economy_mut().node_mut(idx) {
            node_econ.juvenile_ticks = (w_replay as u32).saturating_add(1);
        }
    }
    if !seed_regs(&mut z_sim, obs_window.start_tick()) {
        return None;
    }
    let targets = z_sim.comparator_targets();
    let seed_bank = SeedBank::new(0);
    let sample_count = growth_config.sensitivity_sample_count;
    let node_count = sim.circuit().node_count();
    let zeros_maint = vec![0.0; node_count];
    let mut predictions = vec![0u64; num_outputs];
    let mut z_sum = 0.0f64;
    let mut z_ticks = 0usize;
    let mut pos_credit_sums = vec![0.0; node_count];
    let mut eval_ticks = 0usize;
    for (idx, frame) in obs_window.frames().iter().enumerate() {
        let input = if idx == 0 {
            obs_window.prior_frame()
        } else {
            &obs_window.frames()[idx - 1]
        };
        let score_tick = idx >= eval_start;
        let compute_ancestry = d_alt > 0 || score_tick;
        let (node_credit, z_tick, _) = replay_causal_tick(
            &mut z_sim,
            input,
            frame,
            &mut predictions,
            &zeros_maint,
            &targets,
            sample_count,
            economy_config,
            seed_bank,
            idx as u64,
            score_tick,
            compute_ancestry,
            Some(comp_idx),
            None,
            audit,
        )?;
        if score_tick {
            if let Some(zt) = z_tick {
                z_sum += zt;
                z_ticks += 1;
            }
            for (v, &c) in node_credit.iter().enumerate() {
                pos_credit_sums[v] += c.max(0.0);
            }
            eval_ticks += 1;
        }
    }
    let z_alt = if z_ticks > 0 {
        z_sum / (z_ticks as f64)
    } else {
        0.0
    };
    let base_eff_juv =
        effective_comparator_juvenile_ticks(d_alt, z_alt, economy_config, growth_config);
    let eff_juv = if economy_config.juvenile_ticks > 0 && is_survivor.iter().any(|&s| !s) {
        compute_debt_amortization_juvenile_ticks(
            &is_survivor,
            &sim_ledger.node_debts,
            &pos_credit_sums,
            eval_ticks,
            d_alt,
            base_eff_juv,
            (z_alt + COMPARATOR_PRIMITIVE_COUNT as f64).max(1.0),
            economy_config,
            growth_config,
            Some(telemetry),
        )?
    } else {
        base_eff_juv
    };

    // Pass 2: Simulate buffered graduation survival across `eff_juv` ticks.
    let sim_circuit = sim.circuit().clone();
    assign_stage3_juvenile_ticks(
        &sim_circuit,
        sim.economy_mut(),
        &is_survivor,
        eff_juv,
        economy_config,
        growth_config,
    );
    if eff_juv == 0 {
        let ok = is_survivor
            .iter()
            .enumerate()
            .filter(|&(_, &surv)| !surv)
            .all(|(idx, _)| sim.economy().nodes()[idx].energy > sim_ledger.node_debts[idx]);
        return ok.then_some((d_alt, z_alt, eff_juv));
    }

    if !seed_regs(&mut sim, obs_window.start_tick()) {
        return None;
    }
    let j_ticks = eff_juv as usize;
    let ring_period = pre.autonomous_ring.as_ref().map(|(p, _, _)| *p);
    let cycle_len = ring_period
        .map(|p| (w_replay - (w_replay % p)).max(p).min(w_replay))
        .unwrap_or(w_replay);
    let reactive_warmup = d_alt.min(w_replay / 2);
    let settled_chunk = w_replay.saturating_sub(reactive_warmup).max(1);

    let sim_node_count = sim_circuit.node_count() as u64;
    for step_idx in 0..j_ticks {
        let frame_idx = if ring_period.is_some() {
            step_idx % cycle_len
        } else if step_idx < w_replay {
            step_idx
        } else {
            let offset = (step_idx - w_replay) % settled_chunk;
            if offset == 0 {
                // Re-prime reactive delay registers cleanly on a shadow pass to avoid wrap discontinuity.
                if !seed_regs(&mut sim, obs_window.start_tick()) {
                    return None;
                }
                for w in 0..reactive_warmup {
                    let w_in = if w == 0 {
                        obs_window.prior_frame()
                    } else {
                        &obs_window.frames()[w - 1]
                    };
                    if !audit.try_reserve_replay(sim_node_count) {
                        return None;
                    }
                    sim.step(w_in, &mut predictions).ok()?;
                }
            }
            reactive_warmup + offset
        };

        let input = if frame_idx == 0 {
            obs_window.prior_frame()
        } else {
            &obs_window.frames()[frame_idx - 1]
        };
        let truth = &obs_window.frames()[frame_idx];
        replay_causal_tick(
            &mut sim,
            input,
            truth,
            &mut predictions,
            &zeros_maint,
            &targets,
            sample_count,
            economy_config,
            seed_bank,
            step_idx as u64,
            true,
            true,
            None,
            None,
            audit,
        )?;
    }

    let survived = is_survivor
        .iter()
        .enumerate()
        .filter(|&(_, &surv)| !surv)
        .all(|(idx, _)| sim.economy().nodes()[idx].energy > sim_ledger.node_debts[idx]);
    survived.then_some((d_alt, z_alt, eff_juv))
}

#[allow(clippy::too_many_arguments)]
fn evaluate_stage3_candidate_pre(
    editable: &EditableCircuit,
    obs_window: &ObservationWindow,
    growth_config: &GrowthConfig,
    economy_config: &EconomyConfig,
    budget: &mut TickBudget,
    out_port: u16,
    primary_channel_mask: u64,
    excl: &crate::structural::graph_edit::DerivationConeInfo,
    baseline: &BaselineReplayMetrics,
    e_null_diag: usize,
    staged: Circuit,
    spliced: crate::structural::graph_edit::SplicedComparator,
    alt_src: SignalSource,
    autonomous_ring: Option<(usize, Vec<u64>, Vec<NodeId>)>,
    blocked_by_max_nodes: &mut usize,
    telemetry: &mut SproutTelemetry,
    audit: &mut GrowthEvalAudit,
) -> Option<Result<EvaluatedStage3Pre, ()>> {
    let max_nodes = effective_max_nodes(staged.num_inputs(), growth_config);
    if staged.node_count() > max_nodes {
        *blocked_by_max_nodes += 1;
        return None;
    }

    // TicksPerHop structural phase safety: disallow inserting an uncompensated `Split` on any
    // pre-existing active source edge (which adds +1 register hop and causes live flush errors).
    if staged.wire_delay() == WireDelayMode::TicksPerHop {
        let pre_circuit = editable.circuit();
        let pre_count = pre_circuit.node_count();
        let allowed_ring_split = spliced
            .inserted_split
            .filter(|_| spliced.primary_autonomous_shifted);
        for idx in pre_count..staged.node_count() {
            if staged.nodes()[idx] != Primitive::Split {
                continue;
            }
            let new_split = NodeId(u32::try_from(idx).expect("fits in u32"));
            if Some(new_split) == allowed_ring_split {
                continue;
            }
            let Some(in_edge) = staged.edges().iter().find(|e| {
                e.dst
                    == SignalSink::NodeIn {
                        node: new_split,
                        port: 0,
                    }
            }) else {
                continue;
            };
            if pre_circuit.edges().iter().any(|e| e.src == in_edge.src) {
                return None;
            }
        }
    }

    // Layer 2 exact post-materialization causal-support disjointness audit.
    let alt_cone = derivation_cone_info(
        &staged,
        &[SignalSink::NodeIn {
            node: spliced.sub_node,
            port: 1,
        }],
    );
    if !alt_cone
        .excl_reactive_leaves
        .is_disjoint(&excl.excl_reactive_leaves)
    {
        return None;
    }
    if alt_cone
        .excl_nodes
        .iter()
        .any(|n| excl.excl_nodes.contains(n))
    {
        return None;
    }

    let old_to_new: Vec<Option<NodeId>> = (0..editable.circuit().node_count())
        .map(|i| Some(NodeId(i as u32)))
        .collect();
    let protected_new_node = spliced
        .inserted_split
        .filter(|_| spliced.primary_autonomous_shifted);
    let start_snap = obs_window
        .state_snapshot_at(baseline.start_idx)
        .filter(|&(snap_epoch, _)| snap_epoch == editable.structure_epoch());
    let priming_plan = if start_snap.is_some() {
        match plan_non_surviving_register_priming(
            &staged,
            &old_to_new,
            protected_new_node,
            obs_window,
            editable.structure_epoch(),
            baseline.start_idx,
            baseline.eval_start_idx,
        ) {
            PrimingPlanOutcome::NotNeeded => None,
            PrimingPlanOutcome::Ready(plan) => Some(plan),
            PrimingPlanOutcome::Unavailable => {
                telemetry.candidate_undecidable += 1;
                return None;
            }
        }
    } else {
        None
    };
    let prime_steps = priming_plan.as_ref().map_or(0, |p| p.prime_steps);
    let prime_eval_cost = (prime_steps as u64).saturating_mul(staged.node_count() as u64);
    if !audit.can_afford(prime_eval_cost) {
        audit.budget_cap_hit = true;
        telemetry.sprout_budget_exhausted += 1;
        return Some(Err(()));
    }
    if !budget.try_consume((baseline.w_replay + prime_steps) as u64) {
        telemetry.sprout_budget_exhausted += 1;
        return Some(Err(()));
    }

    let Ok(staged_compiled) = staged.compile() else {
        return None;
    };
    let mut state = ScalarState::new(&staged_compiled);
    let seed_autonomous_ring_at = |idx: usize, regs: &mut [u64]| -> bool {
        if let Some((period, ref mu, ref delay_nodes)) = autonomous_ring {
            let align_tick = (obs_window.start_tick() as usize) + idx;
            for (j, &delay_node) in delay_nodes.iter().enumerate() {
                let Some(reg_idx) = explicit_delay_register_index(&staged, delay_node) else {
                    return false;
                };
                regs[reg_idx] = mu[(align_tick + (period - 1 - j)) % period];
            }
        }
        true
    };
    if let Some((_, snap_regs)) = start_snap {
        let reg_map = register_mapping(
            editable.circuit(),
            &staged,
            editable.compiled(),
            &staged_compiled,
            &old_to_new,
        );
        let seed_snapshot_regs = |snap: &[u64], regs: &mut Vec<u64>| {
            if let Some(split_node) = protected_new_node
                && let Some(adj_regs) =
                    crate::structural::graph_edit::advance_tph_autonomous_ring_phase(
                        editable.circuit(),
                        editable.compiled(),
                        snap,
                        &staged,
                        &staged_compiled,
                        &old_to_new,
                        out_port,
                        split_node,
                        spliced.sub_node,
                    )
            {
                *regs = adj_regs;
            } else {
                map_snapshot_regs_into(&reg_map, snap, regs);
            }
        };

        let mut regs = state.registers().to_vec();
        seed_snapshot_regs(snap_regs, &mut regs);
        if let Some(ref plan) = priming_plan {
            let structure_epoch = editable.structure_epoch();
            let mut ring_ok = true;
            if !prime_non_surviving_registers_with_plan(
                plan,
                &staged,
                &staged_compiled,
                &reg_map,
                protected_new_node,
                obs_window,
                baseline.start_idx,
                &mut regs,
                audit,
                |prime_idx, shadow| {
                    let mut shadow_regs = shadow.registers().to_vec();
                    if let Some((prime_epoch, prime_snap_regs)) =
                        obs_window.state_snapshot_at(prime_idx)
                        && prime_epoch == structure_epoch
                    {
                        seed_snapshot_regs(prime_snap_regs, &mut shadow_regs);
                    }
                    if !seed_autonomous_ring_at(prime_idx, &mut shadow_regs) {
                        ring_ok = false;
                        return false;
                    }
                    let _ = shadow.set_registers(&staged_compiled, &shadow_regs);
                    true
                },
            ) || !ring_ok
            {
                return None;
            }
        }
        if !seed_autonomous_ring_at(baseline.start_idx, &mut regs) {
            return None;
        }
        let _ = state.set_registers(&staged_compiled, &regs);
    } else if autonomous_ring.is_some() {
        let mut regs = state.registers().to_vec();
        if !seed_autonomous_ring_at(baseline.start_idx, &mut regs) {
            return None;
        }
        let _ = state.set_registers(&staged_compiled, &regs);
    }

    let num_outputs = usize::from(staged_compiled.num_outputs());
    let out_idx = usize::from(out_port);
    let mut pred = vec![0u64; num_outputs];
    let mut e_cand_by_port = vec![0usize; num_outputs];
    let mut e_alt_diag = 0usize;
    let mut disagree_diag = 0usize;
    let mut flux_alt = 0.0f64;
    let sub_alt_wire = staged_compiled.node_input_wires()[spliced.sub_node.index()][1];
    let mut alt_scorer = PointForecastScorer::new(
        staged.width(),
        editable.economy().scorers()[out_idx].null_model().kind(),
    );
    *alt_scorer.null_model_mut() = editable.economy().scorers()[out_idx].null_model().clone();

    let staged_nodes = staged.node_count() as u64;
    for i in 0..baseline.w_replay {
        let idx = baseline.start_idx + i;
        let input = if idx == 0 {
            obs_window.prior_frame()
        } else {
            &obs_window.frames()[idx - 1]
        };
        let truth = &obs_window.frames()[idx];
        if !audit.try_reserve_replay(staged_nodes) {
            telemetry.sprout_budget_exhausted += 1;
            return Some(Err(()));
        }
        if state.step(&staged_compiled, input, &mut pred).is_err() {
            return None;
        }
        if idx >= baseline.eval_start_idx {
            for q in 0..num_outputs {
                if baseline.driven_ports.get(q).copied().unwrap_or(false)
                    && q < truth.len()
                    && pred[q] != truth[q]
                {
                    e_cand_by_port[q] += 1;
                }
            }
            let alt_val = state.wire_values()[sub_alt_wire];
            if alt_val != truth[out_idx] {
                e_alt_diag += 1;
            }
            if staged.width().normalize(alt_val) != staged.width().normalize(pred[out_idx]) {
                disagree_diag += 1;
            }
            flux_alt += score_with_reward_mode(
                &mut alt_scorer,
                economy_config.reward_mode,
                staged.width(),
                alt_val,
                truth[out_idx],
            );
        }
    }

    // All-Port Non-Regression Guard (∀ q in 0..K including q == target_port).
    if (0..num_outputs).any(|q| e_cand_by_port[q] > baseline.e_curr_by_port[q]) {
        return None;
    }
    if baseline.e_curr_by_port[out_idx] == 0 && e_alt_diag > 0 {
        return None;
    }
    if flux_alt <= 0.0 || e_alt_diag + growth_config.min_match_gain > e_null_diag {
        return None;
    }

    let l_cand = circuit_code_bits_with_tax(&staged, 0, economy_config.split_code_tax);
    let eval_len_usize = baseline
        .w_replay
        .saturating_sub(baseline.eval_start_idx.saturating_sub(baseline.start_idx));
    if e_alt_diag > baseline.e_curr_by_port[out_idx]
        && disagree_diag.saturating_mul(2) >= eval_len_usize
    {
        return None;
    }
    let eval_len = eval_len_usize as u64;
    let j_cand = l_cand + facts_code_bits(staged.width(), eval_len, e_alt_diag as u64);
    let alt_channel_mask = if autonomous_ring.is_some() {
        0
    } else {
        cone_channel_mask(&staged, alt_src)
    };
    let tier = if (alt_channel_mask & primary_channel_mask) == 0 {
        1
    } else {
        2
    };
    if e_alt_diag > 0 && tier != 1 {
        return None;
    }

    Some(Ok(EvaluatedStage3Pre {
        staged,
        old_to_new,
        sub_node: spliced.sub_node,
        eqz_node: spliced.eqz_node,
        alt_src,
        alt_channel_mask,
        e_alt_diag,
        j_cand,
        tier,
        autonomous_ring,
        primary_tph_split: spliced
            .primary_autonomous_shifted
            .then_some(spliced.inserted_split)
            .flatten(),
    }))
}

fn rank_stage3_candidates(candidates: &mut Vec<EvaluatedStage3Pre>) {
    // Total strict MDL ordering within each tier: lower J (`L(C) + facts_code_bits`), then error count, alt mask, nodes.
    let cmp_within_tier = |a: &EvaluatedStage3Pre, b: &EvaluatedStage3Pre| {
        a.j_cand
            .total_cmp(&b.j_cand)
            .then_with(|| a.e_alt_diag.cmp(&b.e_alt_diag))
            .then_with(|| a.alt_channel_mask.cmp(&b.alt_channel_mask))
            .then_with(|| a.staged.node_count().cmp(&b.staged.node_count()))
    };
    let mut exact_tier1 = Vec::new();
    let mut inexact_tier1 = Vec::new();
    let mut tier2 = Vec::new();
    for cand in candidates.drain(..) {
        if cand.tier == 1 && cand.e_alt_diag == 0 {
            exact_tier1.push(cand);
        } else if cand.tier == 1 {
            inexact_tier1.push(cand);
        } else {
            tier2.push(cand);
        }
    }
    exact_tier1.sort_by(cmp_within_tier);
    inexact_tier1.sort_by(cmp_within_tier);
    tier2.sort_by(cmp_within_tier);

    // Tier 1 exact (`e_alt == 0`) always has strictly highest priority; otherwise compare best
    // remaining Tier 1 vs best Tier 2 using `j_cand` (with MDL tie-break favoring Tier 1).
    candidates.extend(exact_tier1);
    let tier1_head_beats_tier2 = match (inexact_tier1.first(), tier2.first()) {
        (Some(t1), Some(t2)) => t1.j_cand <= t2.j_cand + MDL_TIE_BREAK_EPS,
        (Some(_), None) => true,
        _ => false,
    };
    if tier1_head_beats_tier2 {
        if let Some(head) = (!inexact_tier1.is_empty()).then(|| inexact_tier1.remove(0)) {
            candidates.push(head);
        }
    } else if !tier2.is_empty() {
        candidates.push(tier2.remove(0));
    }
    let mut rest: Vec<EvaluatedStage3Pre> = inexact_tier1.into_iter().chain(tier2).collect();
    rest.sort_by(|a, b| cmp_within_tier(a, b).then_with(|| a.tier.cmp(&b.tier)));
    candidates.extend(rest);
}

/// Checks whether an existing autonomous delay-ring comparator branch (`comp.alt_channel_mask == 0`
/// with cyclic support) strictly beats a noisy reactive primary driver on `out_port` over the
/// current structure epoch's replay span, allowing Stage 2 to promote the ring via handoff without
/// duplicating autonomous cycles or breaking existing comparators.
#[allow(clippy::too_many_arguments)]
pub(crate) fn evaluate_comparator_ring_promotion_audited(
    editable: &EditableCircuit,
    obs_window: &ObservationWindow,
    growth_config: &GrowthConfig,
    economy_config: &EconomyConfig,
    budget: &mut TickBudget,
    out_port: u16,
    telemetry: &mut SproutTelemetry,
    audit: &mut GrowthEvalAudit,
) -> Option<(usize, usize, f64)> {
    let circuit = editable.circuit();
    let has_port_driver = circuit
        .edges()
        .iter()
        .any(|edge| edge.dst == SignalSink::BoundaryOut(out_port));
    if !has_port_driver || !port_uses_boundary_in(circuit, out_port) {
        return None;
    }

    let comparators = editable.economy().comparators();
    let candidate_indices: Vec<usize> = comparators
        .iter()
        .enumerate()
        .filter_map(|(idx, comp)| {
            if comp.target_port != out_port || comp.alt_channel_mask != 0 {
                return None;
            }
            let alt_src = circuit
                .edges()
                .iter()
                .find(|e| {
                    e.dst
                        == SignalSink::NodeIn {
                            node: comp.sub_node,
                            port: 1,
                        }
                })
                .map(|e| e.src)?;
            source_has_cycle(circuit, alt_src).then_some(idx)
        })
        .collect();
    if candidate_indices.is_empty() {
        return None;
    }

    let w_diag = growth_config.diagnostic_window.max(1);
    let epoch = editable.structure_epoch();
    let start_idx = (0..obs_window.len()).find(|&idx| {
        obs_window
            .state_snapshot_at(idx)
            .is_some_and(|(snap_epoch, _)| snap_epoch == epoch)
    })?;
    let w_replay = obs_window.len().saturating_sub(start_idx);
    if w_replay < w_diag {
        return None;
    }
    let (_, snap_regs) = obs_window.state_snapshot_at(start_idx)?;
    let eval_diag_start = obs_window.len() - w_diag;

    if !budget.try_consume(w_replay as u64) {
        telemetry.sprout_budget_exhausted += 1;
        return None;
    }

    let width = circuit.width();
    let num_outputs = usize::from(editable.compiled().num_outputs());
    let out_idx = usize::from(out_port);
    let driven_ports: Vec<bool> = (0..num_outputs)
        .map(|q| {
            let port_u16 = u16::try_from(q).expect("output port fits in u16");
            circuit
                .edges()
                .iter()
                .any(|edge| edge.dst == SignalSink::BoundaryOut(port_u16))
        })
        .collect();

    let mut curr_state = ScalarState::new(editable.compiled());
    if curr_state
        .set_registers(editable.compiled(), snap_regs)
        .is_err()
    {
        return None;
    }
    let mut curr_pred = vec![0u64; num_outputs];
    let mut e_curr = 0usize;
    let mut e_curr_diag = 0usize;
    let mut e_curr_by_port = vec![0usize; num_outputs];
    let mut e_comp_curr = vec![0usize; comparators.len()];
    let mut curr_scorer = PointForecastScorer::new(width, economy_config.null_model);
    let mut flux_curr = 0.0;

    let curr_nodes = circuit.node_count() as u64;
    for (i, frame) in obs_window.frames().iter().enumerate().skip(start_idx) {
        let input = if i == 0 {
            obs_window.prior_frame()
        } else {
            &obs_window.frames()[i - 1]
        };
        if !audit.try_reserve_replay(curr_nodes) {
            telemetry.sprout_budget_exhausted += 1;
            return None;
        }
        if curr_state
            .step(editable.compiled(), input, &mut curr_pred)
            .is_err()
        {
            return None;
        }
        let p_curr = curr_pred[out_idx];
        let p_truth = frame[out_idx];
        if p_curr != p_truth {
            e_curr += 1;
            if i >= eval_diag_start {
                e_curr_diag += 1;
            }
        }
        for (q, e_port) in e_curr_by_port.iter_mut().enumerate() {
            if driven_ports[q] && curr_pred[q] != frame[q] {
                *e_port += 1;
            }
        }
        let wires = curr_state.wire_values();
        for (m, comp) in comparators.iter().enumerate() {
            let sub_in = &editable.compiled().node_input_wires()[comp.sub_node.index()];
            if sub_in.len() >= 2
                && width.normalize(wires[sub_in[0]]) != width.normalize(wires[sub_in[1]])
            {
                e_comp_curr[m] += 1;
            }
        }
        flux_curr += score_with_reward_mode(
            &mut curr_scorer,
            economy_config.reward_mode,
            width,
            p_curr,
            p_truth,
        );
    }

    if e_curr_diag == 0 {
        return None;
    }

    let l_curr = circuit_code_bits_with_tax(circuit, 0, economy_config.split_code_tax);
    let j_curr = l_curr + facts_code_bits(width, w_replay as u64, e_curr as u64);
    let max_nodes = effective_max_nodes(circuit.num_inputs(), growth_config);
    let target_port_subs: Vec<NodeId> = comparators
        .iter()
        .filter(|c| c.target_port == out_port)
        .map(|c| c.sub_node)
        .collect();
    let pre_n = circuit.node_count();
    let identity_map: Vec<Option<NodeId>> = (0..pre_n).map(|i| Some(NodeId(i as u32))).collect();

    let mut best: Option<(usize, usize, f64, f64)> = None;
    for comp_idx in candidate_indices {
        let comp = &comparators[comp_idx];
        let mut swapped = circuit.clone();
        if !swap_comparator_driver(&mut swapped, out_port, comp.sub_node, &target_port_subs) {
            continue;
        }
        if swapped.node_count() > max_nodes {
            telemetry.sprout_blocked_max_nodes += 1;
            continue;
        }
        let Ok(swapped_compiled) = swapped.compile() else {
            continue;
        };
        if !budget.try_consume(w_replay as u64) {
            telemetry.sprout_budget_exhausted += 1;
            break;
        }

        let reg_map = register_mapping(
            circuit,
            &swapped,
            editable.compiled(),
            &swapped_compiled,
            &identity_map,
        );
        let mut cand_state = ScalarState::new(&swapped_compiled);
        let mut cand_regs = cand_state.registers().to_vec();
        for (old_r, new_r) in reg_map.iter().enumerate() {
            if let Some(nr) = new_r
                && old_r < snap_regs.len()
            {
                cand_regs[*nr] = snap_regs[old_r];
            }
        }
        if cand_state
            .set_registers(&swapped_compiled, &cand_regs)
            .is_err()
        {
            continue;
        }

        let mut cand_pred = vec![0u64; num_outputs];
        let mut e_cand = 0usize;
        let mut e_cand_diag = 0usize;
        let mut e_cand_by_port = vec![0usize; num_outputs];
        let mut e_comp_cand = vec![0usize; comparators.len()];
        let mut cand_scorer = PointForecastScorer::new(width, economy_config.null_model);
        let mut flux_cand = 0.0;
        let mut ok = true;

        let swapped_nodes = swapped.node_count() as u64;
        for (i, frame) in obs_window.frames().iter().enumerate().skip(start_idx) {
            let input = if i == 0 {
                obs_window.prior_frame()
            } else {
                &obs_window.frames()[i - 1]
            };
            if !audit.try_reserve_replay(swapped_nodes) {
                telemetry.sprout_budget_exhausted += 1;
                ok = false;
                break;
            }
            if cand_state
                .step(&swapped_compiled, input, &mut cand_pred)
                .is_err()
            {
                ok = false;
                break;
            }
            let p_cand = cand_pred[out_idx];
            if p_cand != frame[out_idx] {
                e_cand += 1;
                if i >= eval_diag_start {
                    e_cand_diag += 1;
                }
            }
            for (q, e_port) in e_cand_by_port.iter_mut().enumerate() {
                if driven_ports[q] && cand_pred[q] != frame[q] {
                    *e_port += 1;
                    if q != out_idx && *e_port > e_curr_by_port[q] {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                break;
            }
            let wires = cand_state.wire_values();
            for (m, c) in comparators.iter().enumerate() {
                let sub_in = &swapped_compiled.node_input_wires()[c.sub_node.index()];
                if sub_in.len() >= 2
                    && width.normalize(wires[sub_in[0]]) != width.normalize(wires[sub_in[1]])
                {
                    e_comp_cand[m] += 1;
                    if e_comp_cand[m] > e_comp_curr[m] {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                break;
            }
            flux_cand += score_with_reward_mode(
                &mut cand_scorer,
                economy_config.reward_mode,
                width,
                p_cand,
                frame[out_idx],
            );
        }

        if !ok || e_cand >= e_curr || flux_cand <= flux_curr {
            continue;
        }
        if swapped.node_count() > pre_n && e_cand + growth_config.min_match_gain > e_curr {
            continue;
        }
        let l_cand = circuit_code_bits_with_tax(&swapped, 0, economy_config.split_code_tax);
        let j_cand = l_cand + facts_code_bits(width, w_replay as u64, e_cand as u64);
        if j_cand >= j_curr {
            continue;
        }

        let delta_flux = flux_cand - flux_curr;
        let rank_scale = sprout_rank_scale(swapped.num_inputs(), w_diag, economy_config);
        let j_cand_diag =
            l_cand + rank_scale * facts_code_bits(width, w_diag as u64, e_cand_diag as u64);

        let is_better = match best {
            None => true,
            Some((_, _, prev_j, prev_flux)) => {
                is_better_mdl(j_cand_diag, delta_flux, prev_j, prev_flux)
            }
        };
        if is_better {
            best = Some((comp_idx, e_cand_diag, j_cand_diag, delta_flux));
        }
    }

    best.map(|(idx, e_diag, j_diag, _)| (idx, e_diag, j_diag))
}

/// Searches for a causal-disjoint Stage 3 backup derivation and comparator (`M2B-D2`).
#[allow(clippy::too_many_arguments)]
pub fn propose_redundancy_sprout(
    editable: &EditableCircuit,
    obs_window: &ObservationWindow,
    diag: &DiagnosticBuffer,
    growth_config: &GrowthConfig,
    economy_config: &EconomyConfig,
    budget: &mut TickBudget,
    out_port: u16,
    telemetry: &mut SproutTelemetry,
) -> Result<Option<RedundancySproutProposal>, GrowthError> {
    propose_redundancy_sprout_audited(
        editable,
        obs_window,
        diag,
        growth_config,
        economy_config,
        budget,
        out_port,
        telemetry,
        &mut GrowthEvalAudit::default(),
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn propose_redundancy_sprout_audited(
    editable: &EditableCircuit,
    obs_window: &ObservationWindow,
    diag: &DiagnosticBuffer,
    growth_config: &GrowthConfig,
    economy_config: &EconomyConfig,
    budget: &mut TickBudget,
    out_port: u16,
    telemetry: &mut SproutTelemetry,
    audit: &mut GrowthEvalAudit,
) -> Result<Option<RedundancySproutProposal>, GrowthError> {
    if !growth_config.redundancy_sprouting || !growth_config.is_valid() {
        return Ok(None);
    }
    let circuit = editable.circuit();
    let out_idx = usize::from(out_port);
    let Some(prim_edge) = circuit
        .edges()
        .iter()
        .find(|e| e.dst == SignalSink::BoundaryOut(out_port))
    else {
        return Ok(None);
    };
    if diag.len() < growth_config.diagnostic_window {
        return Ok(None);
    }

    let comparator_subs: Vec<NodeId> = editable
        .economy()
        .comparators()
        .iter()
        .filter(|c| c.target_port == out_port)
        .map(|c| c.sub_node)
        .collect();
    let mut comparator_all_nodes = BTreeSet::new();
    for c in editable.economy().comparators() {
        comparator_all_nodes.insert(c.sub_node);
        comparator_all_nodes.insert(c.eqz_node);
    }
    let root_sinks = port_comparator_root_sinks(circuit, out_port, &comparator_subs);
    let excl = derivation_cone_info(circuit, &root_sinks);
    let primary_channel_mask = cone_channel_mask(circuit, prim_edge.src);

    let baseline = replay_current_baseline(
        editable,
        obs_window,
        growth_config,
        economy_config,
        out_port,
        false,
        audit,
    );
    if audit.budget_cap_hit {
        telemetry.sprout_budget_exhausted += 1;
        return Ok(None);
    }
    let truth_slice: Vec<u64> = diag.truth.iter().map(|f| f[out_idx]).collect();
    let modal_counts = {
        let mut counts = BTreeMap::<u64, usize>::new();
        for &v in &truth_slice {
            *counts.entry(circuit.width().normalize(v)).or_insert(0) += 1;
        }
        counts.values().copied().max().unwrap_or(0)
    };
    let e_null_diag = truth_slice.len().saturating_sub(modal_counts);

    let mut candidates: Vec<EvaluatedStage3Pre> = Vec::new();
    let mut budget_hit = false;
    let mut blocked_by_max_nodes = 0usize;

    // 1. Autonomous delay-ring backup iff `!HasAutonomousCycle(p)` (Tier 1 vs reactive primary).
    if !excl.has_autonomous_cycle && circuit.wire_delay() == WireDelayMode::Combinational {
        let w_replay = obs_window.len();
        let w_diag = growth_config.diagnostic_window.max(1);
        let eval_start = w_replay.saturating_sub(w_diag);
        let start_tick = obs_window.start_tick() as usize;
        for p in growth_config.min_ring_period..=growth_config.max_ring_period {
            let mut phase_counts = vec![BTreeMap::<u64, usize>::new(); p];
            for (t, frame) in obs_window.frames().iter().enumerate() {
                let sym = circuit.width().normalize(frame[out_idx]);
                let phase = (start_tick + t) % p;
                *phase_counts[phase].entry(sym).or_insert(0) += 1;
            }
            let mut mu = Vec::with_capacity(p);
            let mut ring_e = 0usize;
            for pc in &phase_counts {
                let (best_sym, _) = pc
                    .iter()
                    .max_by(|(s_a, c_a), (s_b, c_b)| c_a.cmp(c_b).then_with(|| s_b.cmp(s_a)))
                    .map(|(&s, &c)| (s, c))
                    .unwrap_or((0, 0));
                mu.push(best_sym);
            }
            for (t, frame) in obs_window.frames().iter().enumerate().skip(eval_start) {
                let phase = (start_tick + t) % p;
                if circuit.width().normalize(frame[out_idx]) != mu[phase] {
                    ring_e += 1;
                }
            }
            if ring_e + growth_config.min_match_gain > e_null_diag {
                continue;
            }
            let mut staged = circuit.clone();
            let (delays, _split, ring_src) = build_autonomous_ring_source(&mut staged, p, &mu);
            if let Some(spliced) = splice_comparator_onto_port(&mut staged, out_port, ring_src) {
                match evaluate_stage3_candidate_pre(
                    editable,
                    obs_window,
                    growth_config,
                    economy_config,
                    budget,
                    out_port,
                    primary_channel_mask,
                    &excl,
                    &baseline,
                    e_null_diag,
                    staged,
                    spliced,
                    ring_src,
                    Some((p, mu, delays)),
                    &mut blocked_by_max_nodes,
                    telemetry,
                    audit,
                ) {
                    Some(Ok(eval)) => candidates.push(eval),
                    Some(Err(())) => {
                        budget_hit = true;
                        break;
                    }
                    None => {}
                }
            }
        }
    }

    // 2. Reactive causal-disjoint search over progressive lag bands (multi-channel deepening mirrors Stage 2).
    if !budget_hit && budget.remaining() > 0 {
        let mut bands = Vec::new();
        let max_lag = growth_config.max_reactive_lag;
        bands.push((1u8, max_lag));
        let deepen_max = growth_config.deepening_max_lag.max(max_lag);
        if circuit.num_outputs() > 1 && deepen_max > max_lag {
            let mut prev = max_lag;
            for &cap in &[6u8, 12u8, deepen_max] {
                let c_cap = cap.min(deepen_max);
                if c_cap > prev {
                    bands.push((prev + 1, c_cap));
                    prev = c_cap;
                }
            }
        }

        'band_loop: for (min_tau, max_tau) in bands {
            if budget.remaining() == 0 {
                break;
            }
            let masked_pool = build_masked_stage3_pool(
                editable,
                obs_window,
                diag,
                out_port,
                max_tau,
                &excl,
                &comparator_all_nodes,
            );
            if masked_pool.is_empty() {
                continue;
            }
            let tier1_pool: Vec<ResidualFeature> = masked_pool
                .iter()
                .filter(|f| {
                    SourceFeature::parse(&f.name)
                        .and_then(|sf| feature_channel_mask(&sf, circuit))
                        .is_some_and(|m| (m & primary_channel_mask) == 0)
                })
                .cloned()
                .collect();

            let mut pools: Vec<&[ResidualFeature]> = Vec::new();
            if !tier1_pool.is_empty() && tier1_pool.len() < masked_pool.len() {
                pools.push(&tier1_pool);
            }
            pools.push(&masked_pool);

            let mut evaluated_motifs = BTreeSet::<String>::new();
            for pool in pools {
                let Ok(diag_vals) = diagnose_stage3_values(
                    circuit.width(),
                    &truth_slice,
                    pool,
                    *growth_config,
                    min_tau,
                    max_tau,
                ) else {
                    continue;
                };
                if diag_vals.candidate_list_truncated {
                    telemetry.candidate_list_truncated += 1;
                }
                for feat_score in &diag_vals.candidate_features {
                    let Some(u1) = SourceFeature::parse(&pool[feat_score.feature_indices[0]].name)
                    else {
                        continue;
                    };
                    let u2 = feat_score
                        .feature_indices
                        .get(1)
                        .and_then(|&idx| SourceFeature::parse(&pool[idx].name));
                    let motifs = generate_candidate_motifs(
                        &u1,
                        u2.as_ref(),
                        circuit.width(),
                        diag_vals.constant_offset,
                        true,
                        circuit.num_inputs(),
                    );
                    for motif in motifs {
                        if matches!(motif, CandidateMotif::ConstLoop(_)) {
                            continue;
                        }
                        let key = format!("{motif:?}");
                        if !evaluated_motifs.insert(key) {
                            continue;
                        }
                        let mut staged = circuit.clone();
                        let alt_src =
                            materialize_motif_with_stack(&mut staged, &motif, out_port, false);
                        let Some(spliced) =
                            splice_comparator_onto_port(&mut staged, out_port, alt_src)
                        else {
                            continue;
                        };
                        match evaluate_stage3_candidate_pre(
                            editable,
                            obs_window,
                            growth_config,
                            economy_config,
                            budget,
                            out_port,
                            primary_channel_mask,
                            &excl,
                            &baseline,
                            e_null_diag,
                            staged,
                            spliced,
                            alt_src,
                            None,
                            &mut blocked_by_max_nodes,
                            telemetry,
                            audit,
                        ) {
                            Some(Ok(eval)) => candidates.push(eval),
                            Some(Err(())) => {
                                break 'band_loop;
                            }
                            None => {}
                        }
                    }
                }
                if candidates.iter().any(|c| c.tier == 1 && c.e_alt_diag == 0) {
                    break;
                }
            }

            if candidates.iter().any(|c| c.tier == 1 && c.e_alt_diag == 0) {
                break;
            }
        }
    }

    if candidates.is_empty() {
        if blocked_by_max_nodes > 0 {
            telemetry.sprout_blocked_max_nodes += 1;
        }
        return Ok(None);
    }

    // Rank candidates using Tier 1 > Tier 2 + error/J priority with a strict total order.
    rank_stage3_candidates(&mut candidates);

    for cand in candidates {
        if audit.budget_cap_hit {
            break;
        }
        if let Some((d_alt, z_alt, eff_juv)) = stage3_measure_z_alt_and_survive(
            editable,
            obs_window,
            growth_config,
            economy_config,
            out_port,
            primary_channel_mask,
            &cand,
            telemetry,
            audit,
        ) {
            let old_nodes = cand.old_to_new.len();
            let new_nodes = (old_nodes..cand.staged.node_count())
                .map(|i| NodeId(u32::try_from(i).expect("fits in u32")))
                .collect();
            return Ok(Some(RedundancySproutProposal {
                circuit: cand.staged,
                old_to_new: cand.old_to_new,
                sub_node: cand.sub_node,
                eqz_node: cand.eqz_node,
                new_nodes,
                d_alt,
                z_alt,
                effective_juvenile_ticks: eff_juv,
                primary_channel_mask,
                alt_channel_mask: cand.alt_channel_mask,
                e_alt_diag: cand.e_alt_diag,
                j_cand: cand.j_cand,
                tier: cand.tier,
                autonomous_ring: cand.autonomous_ring,
                primary_tph_split: cand.primary_tph_split,
            }));
        }
        if audit.budget_cap_hit {
            telemetry.sprout_budget_exhausted += 1;
            break;
        }
    }

    Ok(None)
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
        assert_eq!(effective_max_nodes(2, &growth), 96);
        assert_eq!(effective_max_nodes(3, &growth), 144);
        assert_eq!(effective_max_nodes(4, &growth), 192);

        let no_stage3 = GrowthConfig {
            redundancy_sprouting: false,
            ..GrowthConfig::default()
        };
        assert_eq!(effective_max_nodes(0, &no_stage3), no_stage3.max_nodes);
        assert_eq!(effective_max_nodes(1, &no_stage3), no_stage3.max_nodes);
        assert_eq!(effective_max_nodes(2, &no_stage3), 64);
        assert_eq!(effective_max_nodes(3, &no_stage3), 96);
        assert_eq!(effective_max_nodes(4, &no_stage3), 128);

        let small_growth = GrowthConfig {
            max_nodes: 8,
            deepening_max_lag: 12,
            redundancy_sprouting: false,
            ..GrowthConfig::default()
        };
        assert_eq!(effective_max_nodes(1, &small_growth), 8);
        // floor = 2 * 12 + 8 = 32 > max_nodes (8) => K * 32.
        assert_eq!(effective_max_nodes(3, &small_growth), 96);

        let small_growth_stage3 = GrowthConfig {
            max_nodes: 8,
            deepening_max_lag: 12,
            redundancy_sprouting: true,
            ..GrowthConfig::default()
        };
        assert_eq!(effective_max_nodes(1, &small_growth_stage3), 8);
        // floor = (2 * 12 + 8) + (12 + 2 + 2) = 48 > max_nodes (8) => K * 48.
        assert_eq!(effective_max_nodes(3, &small_growth_stage3), 144);

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
        let baseline = replay_current_baseline(
            &editable,
            &obs,
            &growth,
            &econ,
            0,
            true,
            &mut GrowthEvalAudit::default(),
        );
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
            &mut GrowthEvalAudit::default(),
        );
        assert!(matches!(outcome, SproutMotifOutcome::Rejected));
    }

    #[test]
    fn effective_comparator_juvenile_ticks_scales_and_backward_reduces() {
        let econ = EconomyConfig::default();
        let growth = GrowthConfig::default();
        assert_eq!(COMPARATOR_PRIMITIVE_COUNT, 2);
        for d in 0..=4usize {
            let base = effective_juvenile_ticks(d, &econ, &growth);
            assert_eq!(
                effective_comparator_juvenile_ticks_with_primitives(d, 0.5, 0, &econ, &growth),
                base
            );
            assert_eq!(
                effective_comparator_juvenile_ticks(d, 0.0, &econ, &growth),
                base
            );
            let scaled = effective_comparator_juvenile_ticks(d, 0.90, &econ, &growth);
            let expected = ((f64::from(base) * ((0.90 + 2.0) / 0.90)).ceil()) as u32;
            assert_eq!(scaled, expected);
            assert!(scaled > base);
        }
    }

    #[test]
    fn growth_config_redundancy_sprouting_defaults_true_in_json() {
        let legacy: GrowthConfig = serde_json::from_str("{}").expect("legacy json deserializes");
        assert!(legacy.redundancy_sprouting);
        let disabled: GrowthConfig =
            serde_json::from_str(r#"{"redundancy_sprouting": false}"#).expect("disable flag");
        assert!(!disabled.redundancy_sprouting);
    }

    #[test]
    fn propose_redundancy_sprout_prefers_tier1_disjoint_channel_and_blocks_duplicate() {
        let mut circuit = Circuit::new(Width::BITS_1, 3, 1, WireDelayMode::Combinational);
        let eq0 = circuit.add_node(Primitive::Eqz);
        circuit.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: eq0, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: eq0, port: 0 },
            SignalSink::BoundaryOut(0),
        );

        let econ = EconomyConfig::default();
        let growth = GrowthConfig::default();
        let mut editable = EditableCircuit::new(circuit, econ).expect("editable");
        let ledger = MarginalDebtLedger::new();
        let mut ledger = ledger;
        ledger.ensure_capacity(editable.circuit(), econ.split_code_tax);
        editable
            .economy_mut()
            .assign_sprout_debts(&ledger.node_debts);
        // Prime and mature primary node so Stage 3 sees live causal cone.
        let mut preds = vec![0u64; 1];
        for _ in 0..growth.diagnostic_window.max(32) {
            let _ = editable.step(&[0, 0, 0], &mut preds);
        }

        let mut obs = ObservationWindow::new(3, 256);
        for t in 0..160usize {
            let b0 = ((t + 1) % 2) as u64;
            let b1 = b0; // Channel 1 mirrors channel 0 on this window (redundant view).
            let b2 = ((t / 2) % 2) as u64;
            let frame = [b0, b1, b2];
            obs.push_frame_with_state(&frame, 0, &[]);
        }

        let mut diag_buf = DiagnosticBuffer::new(growth.diagnostic_window);
        let eval_start = obs.len() - growth.diagnostic_window;
        for t in eval_start..obs.len() {
            let prev = &obs.frames()[t - 1];
            let truth_frame = &obs.frames()[t];
            let pred = if prev[0] == 0 { 1 } else { 0 };
            diag_buf.push_frame(prev, truth_frame, &[pred], &[0], &[pred], &[true]);
        }

        let mut budget = TickBudget::new(10_000);
        let mut telemetry = SproutTelemetry::default();
        let proposal = propose_redundancy_sprout(
            &editable,
            &obs,
            &diag_buf,
            &growth,
            &econ,
            &mut budget,
            0,
            &mut telemetry,
        )
        .expect("stage 3 result")
        .expect("Stage 3 proposes cross-channel comparator");

        assert_eq!(proposal.tier, 1);
        assert_eq!(proposal.alt_channel_mask & 1u64, 0);
        assert_ne!(proposal.alt_channel_mask & (1u64 << 1), 0);
        assert_eq!(proposal.e_alt_diag, 0);
        assert!(proposal.effective_juvenile_ticks >= effective_juvenile_ticks(0, &econ, &growth));

        // Now repeat with channel 1 orthogonal to truth[0]: Stage 3 may only pick a causal-disjoint
        // non-ch0 route (never duplicating ch0).
        let mut obs_orth = ObservationWindow::new(3, 256);
        for t in 0..160usize {
            let b0 = ((t + 1) % 2) as u64;
            let b1 = ((t / 2) % 2) as u64;
            let b2 = ((t / 3) % 2) as u64;
            obs_orth.push_frame_with_state(&[b0, b1, b2], 0, &[]);
        }
        let mut diag_buf_orth = DiagnosticBuffer::new(growth.diagnostic_window);
        for t in eval_start..obs_orth.len() {
            let prev = &obs_orth.frames()[t - 1];
            let truth_frame = &obs_orth.frames()[t];
            let pred = if prev[0] == 0 { 1 } else { 0 };
            diag_buf_orth.push_frame(prev, truth_frame, &[pred], &[0], &[pred], &[true]);
        }
        let mut budget2 = TickBudget::new(10_000);
        if let Some(prop) = propose_redundancy_sprout(
            &editable,
            &obs_orth,
            &diag_buf_orth,
            &growth,
            &econ,
            &mut budget2,
            0,
            &mut telemetry,
        )
        .expect("stage 3 result")
        {
            assert_eq!(prop.alt_channel_mask & 1u64, 0);
        }
    }

    #[test]
    fn m2b_b1_debt_amortization_law_covers_free_rider_undecidable_and_backward_reduction() {
        let econ = EconomyConfig::default();
        let growth = GrowthConfig::default();

        // 1. d <= 3 backward reduction (z_tot = 1): fast amortization keeps t_juv at the 128 floor.
        for d in 0..=3usize {
            let base = effective_juvenile_ticks(d, &econ, &growth);
            assert_eq!(base, econ.juvenile_ticks);
            let mut tel = SproutTelemetry::default();
            let got = compute_debt_amortization_juvenile_ticks(
                &[true, false, false],
                &[0.0, 5.0, 6.0],
                &[0.0, 64.0, 64.0],
                128,
                d,
                base,
                1.0,
                &econ,
                &growth,
                Some(&mut tel),
            );
            assert_eq!(got, Some(128));
            assert_eq!(tel.candidate_undecidable, 0);
        }

        // 2. Zero-share free-rider rejection: zero positive credit on one new node rejects cleanly.
        let mut tel_fr = SproutTelemetry::default();
        assert_eq!(
            compute_debt_amortization_juvenile_ticks(
                &[true, false, false],
                &[0.0, 5.0, 6.0],
                &[0.0, 64.0, 0.0],
                128,
                1,
                effective_juvenile_ticks(1, &econ, &growth),
                1.0,
                &econ,
                &growth,
                Some(&mut tel_fr),
            ),
            None
        );
        assert_eq!(tel_fr.candidate_undecidable, 0);

        // 3. Stage-2 multi-node weak credit amortization: slowest new node determines t_debt > 128.
        let base2 = effective_juvenile_ticks(2, &econ, &growth);
        assert_eq!(
            compute_debt_amortization_juvenile_ticks(
                &[false, false],
                &[5.0, 12.0],
                &[64.0, 6.4],
                128,
                2,
                base2,
                1.0,
                &econ,
                &growth,
                None,
            ),
            Some(242)
        );

        // 4a. Clause (ii) (anti-parasite): a near-zero-share leaf (t_debt ~ 602) exceeds the undiluted
        //     ceiling 2 * W_obs = 512 when z_tot = 1, but survives once measured causal dilution
        //     z_tot = 4 scales the ceiling to 2048.
        let mut tel_undec = SproutTelemetry::default();
        assert_eq!(
            compute_debt_amortization_juvenile_ticks(
                &[false, false],
                &[5.0, 12.0],
                &[64.0, 2.56],
                128,
                2,
                base2,
                1.0,
                &econ,
                &growth,
                Some(&mut tel_undec),
            ),
            None
        );
        assert_eq!(tel_undec.candidate_undecidable, 1);
        let diluted = compute_debt_amortization_juvenile_ticks(
            &[false, false],
            &[5.0, 12.0],
            &[64.0, 2.56],
            128,
            2,
            base2,
            4.0,
            &econ,
            &growth,
            None,
        )
        .expect("causal dilution z_tot = 4 admits the weak leaf");
        assert!((602..=603).contains(&diluted));

        // 4b. Clause (i) (anti-noise): uniformly weak correlations (cone crossover 2 + 550 > 512) are
        //     rejected even when the per-node ceiling is relaxed by z_tot = 4.
        let mut tel_noise = SproutTelemetry::default();
        assert_eq!(
            compute_debt_amortization_juvenile_ticks(
                &[false, false],
                &[5.0, 6.0],
                &[1.28, 1.28],
                128,
                2,
                base2,
                4.0,
                &econ,
                &growth,
                Some(&mut tel_noise),
            ),
            None
        );
        assert_eq!(tel_noise.candidate_undecidable, 1);

        // 4c. M2a d = 14 chain (c2a): gamma^13-attenuated leaves (t_debt = 1144 <= t_base) pass with the
        //     single code path; without base/z scaling the same cone is rejected at 512.
        let base14 = effective_juvenile_ticks(14, &econ, &growth);
        assert!(base14 > 512);
        let deep_credits = [128.0, 0.02729 * 128.0, 0.01941 * 128.0];
        let deep_debts = [12.0, 20.728, 21.931];
        let deep = compute_debt_amortization_juvenile_ticks(
            &[false, false, false],
            &deep_debts,
            &deep_credits,
            128,
            14,
            base14,
            7.71,
            &econ,
            &growth,
            None,
        );
        assert_eq!(deep, Some(base14));
        let mut tel_deep = SproutTelemetry::default();
        assert_eq!(
            compute_debt_amortization_juvenile_ticks(
                &[false, false, false],
                &deep_debts,
                &deep_credits,
                128,
                14,
                econ.juvenile_ticks,
                1.0,
                &econ,
                &growth,
                Some(&mut tel_deep),
            ),
            None
        );
        assert_eq!(tel_deep.candidate_undecidable, 1);

        // 4d. Finite guards: non-finite z_tot and juvenile_ticks == 0 never loosen the law.
        let econ0 = EconomyConfig {
            juvenile_ticks: 0,
            ..EconomyConfig::default()
        };
        assert_eq!(
            compute_debt_amortization_juvenile_ticks(
                &[false],
                &[5.0],
                &[64.0],
                128,
                1,
                0,
                f64::NAN,
                &econ0,
                &growth,
                None,
            ),
            Some(11)
        );

        // 5. Circuit-level free-rider rejection via verify_stage2_debt_amortization_and_survival:
        //    useful node drives alternating BoundaryOut(0) cleanly; parasitic second node gets 0 share.
        let base_c = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);
        let editable = EditableCircuit::new(base_c, econ).unwrap();
        let mut obs = ObservationWindow::new(1, 256);
        for t in 0..256usize {
            obs.push_frame(&[(t % 2) as u64]);
        }
        let baseline = replay_current_baseline(
            &editable,
            &obs,
            &growth,
            &econ,
            0,
            true,
            &mut GrowthEvalAudit::default(),
        );
        let mut staged = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);
        let split = staged.add_node(Primitive::Split);
        let useful = staged.add_node(Primitive::Eqz);
        let parasite = staged.add_node(Primitive::Eqz);
        staged.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn {
                node: split,
                port: 0,
            },
        );
        staged.connect(
            SignalSource::NodeOut {
                node: split,
                port: 0,
            },
            SignalSink::NodeIn {
                node: useful,
                port: 0,
            },
        );
        staged.connect(
            SignalSource::NodeOut {
                node: split,
                port: 1,
            },
            SignalSink::NodeIn {
                node: parasite,
                port: 0,
            },
        );
        staged.connect(
            SignalSource::NodeOut {
                node: useful,
                port: 0,
            },
            SignalSink::BoundaryOut(0),
        );
        let staged_compiled = staged.compile().unwrap();
        assert_eq!(
            verify_stage2_debt_amortization_and_survival(
                &editable,
                &obs,
                &growth,
                &econ,
                &staged,
                &staged_compiled,
                &baseline,
                0,
                0,
                effective_juvenile_ticks(0, &econ, &growth),
                None,
                &mut GrowthEvalAudit::default(),
            ),
            None
        );
    }

    #[test]
    fn r1_repro_surviving_delay_feeds_new_delay_with_and_without_snapshot() {
        let mut old = Circuit::new(Width::BITS_2, 1, 1, WireDelayMode::Combinational);
        let d0 = old.add_node(Primitive::Delay { init: 0 });
        old.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: d0, port: 0 },
        );
        old.connect(
            SignalSource::NodeOut { node: d0, port: 0 },
            SignalSink::BoundaryOut(0),
        );
        let old_compiled = old.compile().unwrap();

        let mut staged = Circuit::new(Width::BITS_2, 1, 1, WireDelayMode::Combinational);
        let s_d0 = staged.add_node(Primitive::Delay { init: 0 });
        let s_d1 = staged.add_node(Primitive::Delay { init: 0 });
        staged.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn {
                node: s_d0,
                port: 0,
            },
        );
        staged.connect(
            SignalSource::NodeOut {
                node: s_d0,
                port: 0,
            },
            SignalSink::NodeIn {
                node: s_d1,
                port: 0,
            },
        );
        staged.connect(
            SignalSource::NodeOut {
                node: s_d1,
                port: 0,
            },
            SignalSink::BoundaryOut(0),
        );
        let staged_compiled = staged.compile().unwrap();

        let old_to_new = vec![Some(s_d0)];
        let reg_map = register_mapping(&old, &staged, &old_compiled, &staged_compiled, &old_to_new);
        assert_eq!(reg_map, vec![Some(0)]);

        // (a) Same-epoch snapshot `[3]` at `idx = 2` (`prime_start = start_idx - d_new = 2`).
        let mut obs_a = ObservationWindow::new(1, 16);
        obs_a.push_frame_with_state(&[3], 1, &[0]);
        obs_a.push_frame_with_state(&[2], 1, &[0]);
        obs_a.push_frame_with_state(&[1], 1, &[3]);
        let mut regs_a = [2u64, 0u64];
        assert_eq!(
            prime_non_surviving_registers(
                &staged,
                &staged_compiled,
                &old_to_new,
                &reg_map,
                None,
                &obs_a,
                1,
                3,
                3,
                &mut regs_a,
            ),
            PrimingOutcome::Primed
        );
        assert_eq!(regs_a, [2, 3]);

        // (b) NO same-epoch snapshot at `idx = 2` (only `[2]` at `idx = 3`); falls back to `d_full = 2`.
        let mut obs_b = ObservationWindow::new(1, 16);
        obs_b.push_frame(&[3]);
        obs_b.push_frame(&[2]);
        obs_b.push_frame_with_state(&[1], 0, &[3]);
        obs_b.push_frame_with_state(&[1], 1, &[2]);
        let mut regs_b = [2u64, 0u64];
        assert_eq!(
            prime_non_surviving_registers(
                &staged,
                &staged_compiled,
                &old_to_new,
                &reg_map,
                None,
                &obs_b,
                1,
                3,
                3,
                &mut regs_b,
            ),
            PrimingOutcome::Primed
        );
        assert_eq!(regs_b, [2, 3]);

        // End-to-end check through `evaluate_sprout_motif` for case (b) where edit happened at `idx = 3`:
        // At `idx = 3`, `d0 = 2`, primed `d1 = 3` -> `LagYhat(0, 1)` predicts `(2 + 3) mod 4 = 1 == truth[3]`.
        // At `idx = 4`, `d0 = 0`, `d1 = 2` -> `LagYhat(0, 1)` predicts `(0 + 2) mod 4 = 2 == truth[4]`.
        let mut obs_e2e = ObservationWindow::new(1, 16);
        obs_e2e.push_frame(&[3]);
        obs_e2e.push_frame(&[2]);
        obs_e2e.push_frame_with_state(&[0], 0, &[3]);
        obs_e2e.push_frame_with_state(&[1], 1, &[2]);
        obs_e2e.push_frame_with_state(&[2], 1, &[0]);
        let econ = EconomyConfig {
            juvenile_ticks: 0,
            ..EconomyConfig::default()
        };
        let growth = GrowthConfig {
            diagnostic_window: 2,
            min_match_gain: 1,
            ..GrowthConfig::default()
        };
        let mut editable = EditableCircuit::new(old.clone(), econ).unwrap();
        editable
            .recompile_preserving_state(old, &[Some(d0)])
            .unwrap();
        assert_eq!(editable.structure_epoch(), 1);
        let baseline = replay_current_baseline(
            &editable,
            &obs_e2e,
            &growth,
            &econ,
            0,
            false,
            &mut GrowthEvalAudit::default(),
        );
        assert_eq!(baseline.start_idx, 3);
        assert_eq!(baseline.eval_start_idx, 3);
        assert_eq!(baseline.w_replay, 2);
        assert_eq!(baseline.e_curr, 2);

        let mut budget = TickBudget::new(100);
        let mut tel = SproutTelemetry::default();
        let outcome = evaluate_sprout_motif(
            &editable,
            &obs_e2e,
            &growth,
            &econ,
            0,
            &CandidateMotif::Identity(SourceFeature::LagYhat { port: 0, tau: 1 }),
            false,
            false,
            &baseline,
            Some(&mut budget),
            Some(&mut tel),
            &mut GrowthEvalAudit::default(),
        );
        match outcome {
            SproutMotifOutcome::Accepted { e_cand, .. } => assert_eq!(e_cand, 0),
            _ => panic!("expected LagYhat(0, 1) to accept once primed to [2, 3]"),
        }
        assert_eq!(budget.consumed(), 4); // w_replay (2) + prime_steps (2)
        assert_eq!(tel.sprout_budget_exhausted, 0);
    }

    #[test]
    fn r1_priming_surviving_cyclic_ring_feeds_new_delay() {
        // Surviving period-3 autonomous ring [1, 2, 3] feeding a newly added Delay { init: 0 }.
        let mut old = Circuit::new(Width::BITS_2, 1, 1, WireDelayMode::Combinational);
        let (ring_delays, ring_split) = build_autonomous_ring(&mut old, 3, &[1, 2, 3], 0);
        let old_compiled = old.compile().unwrap();

        let mut sim = ScalarState::new(&old_compiled);
        let mut obs = ObservationWindow::new(1, 16);
        let mut pred = [0u64];
        for _ in 0..4 {
            let pre_regs = sim.registers().to_vec();
            sim.step(&old_compiled, &[0], &mut pred).unwrap();
            obs.push_frame_with_state(&pred, 0, &pre_regs);
        }
        // Snapshot at `idx = 2` holds [2, 1, 3] (r2 = 3); snapshot at `idx = 3` holds [3, 2, 1] (r2 = 1).
        assert_eq!(obs.state_snapshot_at(2).unwrap().1, &[2, 1, 3]);
        assert_eq!(obs.state_snapshot_at(3).unwrap().1, &[3, 2, 1]);

        let mut staged = old.clone();
        let d_new = staged.add_node(Primitive::Delay { init: 0 });
        let tapped = tap_source(
            &mut staged,
            SignalSource::NodeOut {
                node: ring_split,
                port: 1,
            },
        );
        staged.connect(
            tapped,
            SignalSink::NodeIn {
                node: d_new,
                port: 0,
            },
        );
        let staged_compiled = staged.compile().unwrap();

        let mut old_to_new: Vec<Option<NodeId>> = (0..old.node_count())
            .map(|i| Some(NodeId(i as u32)))
            .collect();
        let reg_map = register_mapping(&old, &staged, &old_compiled, &staged_compiled, &old_to_new);
        let d_new_reg = explicit_delay_register_index(&staged, d_new).unwrap();
        assert_eq!(ring_delays.len(), 3);

        let mut regs = vec![0u64; staged_compiled.total_registers()];
        map_snapshot_regs_into(&reg_map, obs.state_snapshot_at(3).unwrap().1, &mut regs);
        assert_eq!(regs[d_new_reg], 0);

        assert_eq!(
            prime_non_surviving_registers(
                &staged,
                &staged_compiled,
                &old_to_new,
                &reg_map,
                None,
                &obs,
                0,
                3,
                3,
                &mut regs,
            ),
            PrimingOutcome::Primed
        );
        // Surviving ring registers at idx = 3 stay [3, 2, 1]; new Delay primed from idx = 2 ring output (3).
        assert_eq!(regs[..3], [3, 2, 1]);
        assert_eq!(regs[d_new_reg], 3);

        // Second check: TicksPerHop Stage 3 `primary_autonomous_shifted` with reactive `alt_src` Delay.
        let mut tph_old = Circuit::new(Width::BITS_2, 2, 1, WireDelayMode::TicksPerHop);
        let tph_d0 = tph_old.add_node(Primitive::Delay { init: 1 });
        let tph_d1 = tph_old.add_node(Primitive::Delay { init: 2 });
        let tph_split = tph_old.add_node(Primitive::Split);
        tph_old.connect(
            SignalSource::NodeOut {
                node: tph_d0,
                port: 0,
            },
            SignalSink::NodeIn {
                node: tph_d1,
                port: 0,
            },
        );
        tph_old.connect(
            SignalSource::NodeOut {
                node: tph_d1,
                port: 0,
            },
            SignalSink::NodeIn {
                node: tph_split,
                port: 0,
            },
        );
        tph_old.connect(
            SignalSource::NodeOut {
                node: tph_split,
                port: 0,
            },
            SignalSink::NodeIn {
                node: tph_d0,
                port: 0,
            },
        );
        tph_old.connect(
            SignalSource::NodeOut {
                node: tph_split,
                port: 1,
            },
            SignalSink::BoundaryOut(0),
        );
        let econ = EconomyConfig {
            juvenile_ticks: 0,
            ..EconomyConfig::default()
        };
        let growth = GrowthConfig {
            diagnostic_window: 1,
            min_match_gain: 1,
            ..GrowthConfig::default()
        };
        let tph_editable = EditableCircuit::new(tph_old, econ).unwrap();
        let mut tph_sim = ScalarState::new(tph_editable.compiled());
        let mut tph_obs = ObservationWindow::new(2, 16);
        let mut tph_pred = [0u64];
        let mut ch1_hist = Vec::new();
        for t in 0..8usize {
            let pre_regs = tph_sim.registers().to_vec();
            let ch1 = ((t % 3) + 1) as u64;
            ch1_hist.push(ch1);
            let prev_ch1 = if t == 0 { 0 } else { ch1_hist[t - 1] };
            tph_sim
                .step(tph_editable.compiled(), &[0, prev_ch1], &mut tph_pred)
                .unwrap();
            // Frame carries pre-ring output on port 0 shifted by 1 tick vs post-splice, wait:
            // `advance_tph_autonomous_ring_phase` keeps `BoundaryOut(0)` phase-aligned to pre-splice!
            tph_obs.push_frame_with_state(&[tph_pred[0], ch1], 0, &pre_regs);
        }
        // Check with `start_idx = 6, eval_start_idx = 6` (`BaselineReplayMetrics` manually aligned or via replay):
        let tph_baseline = BaselineReplayMetrics {
            e_curr: 1,
            e_const_mode: 1,
            flux_curr: 0.0,
            l_curr: 0.0,
            facts_curr: 0.0,
            driven_ports: vec![true],
            e_curr_by_port: vec![1],
            e_comp_curr: Vec::new(),
            comp_cone_edges: Vec::new(),
            start_idx: 6,
            eval_start_idx: 6,
            eval_offset: 0,
            w_replay: 2,
            num_inputs: 2,
            can_fast_comb: false,
            norm_inputs: Vec::new(),
            out_pred_trace: Vec::new(),
            out_truth_trace: Vec::new(),
            survivor_map: (0..tph_editable.circuit().node_count())
                .map(|i| Some(NodeId(i as u32)))
                .collect(),
        };
        let mut tph_staged = tph_editable.circuit().clone();
        let alt_delay = tph_staged.add_node(Primitive::Delay { init: 0 });
        tph_staged.connect(
            SignalSource::BoundaryIn(1),
            SignalSink::NodeIn {
                node: alt_delay,
                port: 0,
            },
        );
        let alt_src = SignalSource::NodeOut {
            node: alt_delay,
            port: 0,
        };
        let spliced = splice_comparator_onto_port(&mut tph_staged, 0, alt_src).unwrap();
        assert!(spliced.primary_autonomous_shifted);
        // Verify snapshot-loader closure primes `alt_delay` + `alt_delay -> sub.in(1)` without
        // disturbing the shifted primary autonomous ring phase.
        old_to_new = (0..tph_editable.circuit().node_count())
            .map(|i| Some(NodeId(i as u32)))
            .collect();
        let excl = derivation_cone_info(tph_editable.circuit(), &[SignalSink::BoundaryOut(0)]);
        let PrimingPlanOutcome::Ready(plan) = plan_non_surviving_register_priming(
            &tph_staged,
            &old_to_new,
            spliced.inserted_split,
            &tph_obs,
            0,
            tph_baseline.start_idx,
            tph_baseline.eval_start_idx,
        ) else {
            panic!("priming plan needed for TPH alt Delay");
        };
        assert_eq!(plan.prime_steps, 4); // BoundaryIn(1) -[1]-> alt_delay(1) -[1]-> sub_node -[1]-> eqz_node
        let mut tph_budget = TickBudget::new((tph_baseline.w_replay + plan.prime_steps) as u64);
        let mut tph_tel = SproutTelemetry::default();
        let mut blocked = 0usize;
        let _ = evaluate_stage3_candidate_pre(
            &tph_editable,
            &tph_obs,
            &growth,
            &econ,
            &mut tph_budget,
            0,
            0,
            &excl,
            &tph_baseline,
            2,
            tph_staged,
            spliced,
            alt_src,
            None,
            &mut blocked,
            &mut tph_tel,
            &mut GrowthEvalAudit::default(),
        );
        assert_eq!(
            tph_budget.consumed(),
            (tph_baseline.w_replay + plan.prime_steps) as u64
        );
        assert_eq!(tph_tel.sprout_budget_exhausted, 0);
    }

    #[test]
    fn r5_priming_steps_metered_in_stage2_and_stage3_budget() {
        let econ = EconomyConfig {
            juvenile_ticks: 0,
            ..EconomyConfig::default()
        };
        let growth = GrowthConfig {
            diagnostic_window: 2,
            min_match_gain: 1,
            max_reactive_lag: 4,
            ..GrowthConfig::default()
        };

        // 1. Stage 2 (`evaluate_sprout_motif`): `obs.len() = 8`, `w_diag = 2` => `start_idx = 4`,
        //    `eval_start_idx = 6`, `w_replay = 4`, `eval_offset = 2`. Motif `LagIn(0, 3)` has
        //    `d_new = 3 > eval_offset = 2`, so `prime_start = 4 - 3 = 1` and `prime_steps = 3`.
        let mut circ1 = Circuit::new(Width::BITS_2, 1, 1, WireDelayMode::Combinational);
        let c1_d0 = circ1.add_node(Primitive::Delay { init: 0 });
        circ1.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn {
                node: c1_d0,
                port: 0,
            },
        );
        circ1.connect(
            SignalSource::NodeOut {
                node: c1_d0,
                port: 0,
            },
            SignalSink::BoundaryOut(0),
        );
        let editable1 = EditableCircuit::new(circ1, econ).unwrap();
        let mut obs1_p4 = ObservationWindow::new(1, 16);
        let mut sim1_p4 = ScalarState::new(editable1.compiled());
        let mut pred1 = [0u64];
        let p4 = [0u64, 1, 2, 3];
        for t in 0..8usize {
            let pre_regs = sim1_p4.registers().to_vec();
            let prev = if t == 0 { 0 } else { p4[(t - 1) % 4] };
            sim1_p4
                .step(editable1.compiled(), &[prev], &mut pred1)
                .unwrap();
            obs1_p4.push_frame_with_state(&[p4[t % 4]], 0, &pre_regs);
        }
        let baseline1 = replay_current_baseline(
            &editable1,
            &obs1_p4,
            &growth,
            &econ,
            0,
            false,
            &mut GrowthEvalAudit::default(),
        );
        assert_eq!(baseline1.start_idx, 4);
        assert_eq!(baseline1.eval_start_idx, 6);
        assert_eq!(baseline1.w_replay, 4);

        let motif = CandidateMotif::Identity(SourceFeature::LagIn { channel: 0, tau: 3 });
        let mut budget_short = TickBudget::new(baseline1.w_replay as u64);
        let mut tel_short = SproutTelemetry::default();
        let out_short = evaluate_sprout_motif(
            &editable1,
            &obs1_p4,
            &growth,
            &econ,
            0,
            &motif,
            false,
            false,
            &baseline1,
            Some(&mut budget_short),
            Some(&mut tel_short),
            &mut GrowthEvalAudit::default(),
        );
        assert!(matches!(out_short, SproutMotifOutcome::OutOfBudget));
        assert_eq!(tel_short.sprout_budget_exhausted, 1);

        let expected_prime_steps1 = 3usize;
        let mut budget_exact = TickBudget::new((baseline1.w_replay + expected_prime_steps1) as u64);
        let mut tel_exact = SproutTelemetry::default();
        let out_exact = evaluate_sprout_motif(
            &editable1,
            &obs1_p4,
            &growth,
            &econ,
            0,
            &motif,
            false,
            false,
            &baseline1,
            Some(&mut budget_exact),
            Some(&mut tel_exact),
            &mut GrowthEvalAudit::default(),
        );
        assert!(!matches!(out_exact, SproutMotifOutcome::OutOfBudget));
        assert_eq!(tel_exact.sprout_budget_exhausted, 0);
        assert_eq!(
            budget_exact.consumed(),
            (baseline1.w_replay + expected_prime_steps1) as u64
        );
        assert_eq!(budget_exact.remaining(), 0);

        // 2. Stage 3 (`evaluate_stage3_candidate_pre`): primary on ch0, disjoint 3-Delay alt on ch1.
        let mut circ3 = Circuit::new(Width::BITS_2, 2, 1, WireDelayMode::Combinational);
        let c3_d0 = circ3.add_node(Primitive::Delay { init: 0 });
        circ3.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn {
                node: c3_d0,
                port: 0,
            },
        );
        circ3.connect(
            SignalSource::NodeOut {
                node: c3_d0,
                port: 0,
            },
            SignalSink::BoundaryOut(0),
        );
        let editable3 = EditableCircuit::new(circ3, econ).unwrap();
        let mut obs3 = ObservationWindow::new(2, 16);
        let mut sim3 = ScalarState::new(editable3.compiled());
        let mut pred3 = [0u64];
        for t in 0..8usize {
            let pre_regs = sim3.registers().to_vec();
            let prev0 = if t == 0 { 0 } else { p4[(t - 1) % 4] };
            let prev1 = if t == 0 { 0 } else { p4[(t - 1) % 4] };
            sim3.step(editable3.compiled(), &[prev0, prev1], &mut pred3)
                .unwrap();
            // Truth matches channel 1 delayed by 4 (3 delays + input boundary step), while ch0 lags by 1.
            let y_alt = p4[(t + 1) % 4];
            let ch1 = p4[(t + 1) % 4];
            obs3.push_frame_with_state(&[y_alt, ch1], 0, &pre_regs);
        }
        let baseline3 = replay_current_baseline(
            &editable3,
            &obs3,
            &growth,
            &econ,
            0,
            false,
            &mut GrowthEvalAudit::default(),
        );
        assert_eq!(baseline3.start_idx, 4);
        assert_eq!(baseline3.eval_start_idx, 6);
        assert_eq!(baseline3.w_replay, 4);

        let mut staged3 = editable3.circuit().clone();
        let a0 = staged3.add_node(Primitive::Delay { init: 0 });
        let a1 = staged3.add_node(Primitive::Delay { init: 0 });
        let a2 = staged3.add_node(Primitive::Delay { init: 0 });
        staged3.connect(
            SignalSource::BoundaryIn(1),
            SignalSink::NodeIn { node: a0, port: 0 },
        );
        staged3.connect(
            SignalSource::NodeOut { node: a0, port: 0 },
            SignalSink::NodeIn { node: a1, port: 0 },
        );
        staged3.connect(
            SignalSource::NodeOut { node: a1, port: 0 },
            SignalSink::NodeIn { node: a2, port: 0 },
        );
        let alt_src = SignalSource::NodeOut { node: a2, port: 0 };
        let spliced3 = splice_comparator_onto_port(&mut staged3, 0, alt_src).unwrap();
        let excl3 = derivation_cone_info(editable3.circuit(), &[SignalSink::BoundaryOut(0)]);
        let primary_mask3 = cone_channel_mask(
            editable3.circuit(),
            SignalSource::NodeOut {
                node: c3_d0,
                port: 0,
            },
        );

        let mut b3_short = TickBudget::new(baseline3.w_replay as u64);
        let mut t3_short = SproutTelemetry::default();
        let mut blocked3 = 0usize;
        let res_short = evaluate_stage3_candidate_pre(
            &editable3,
            &obs3,
            &growth,
            &econ,
            &mut b3_short,
            0,
            primary_mask3,
            &excl3,
            &baseline3,
            2,
            staged3.clone(),
            spliced3,
            alt_src,
            None,
            &mut blocked3,
            &mut t3_short,
            &mut GrowthEvalAudit::default(),
        );
        assert!(matches!(res_short, Some(Err(()))));
        assert_eq!(t3_short.sprout_budget_exhausted, 1);

        let expected_prime_steps3 = 3usize;
        let mut b3_exact = TickBudget::new((baseline3.w_replay + expected_prime_steps3) as u64);
        let mut t3_exact = SproutTelemetry::default();
        let res_exact = evaluate_stage3_candidate_pre(
            &editable3,
            &obs3,
            &growth,
            &econ,
            &mut b3_exact,
            0,
            primary_mask3,
            &excl3,
            &baseline3,
            2,
            staged3,
            spliced3,
            alt_src,
            None,
            &mut blocked3,
            &mut t3_exact,
            &mut GrowthEvalAudit::default(),
        );
        assert!(!matches!(res_exact, Some(Err(()))));
        assert_eq!(t3_exact.sprout_budget_exhausted, 0);
        assert_eq!(
            b3_exact.consumed(),
            (baseline3.w_replay + expected_prime_steps3) as u64
        );
        assert_eq!(b3_exact.remaining(), 0);
    }

    #[test]
    fn r1_residual_surviving_cyclic_ring_missing_or_mismatched_snapshot_unavailable() {
        // Exact R1 Residual reproducer: surviving period-3 cyclic ring [1, 2, 3] feeding a new
        // Delay { init: 0 }. At `start_idx = eval_start_idx = 3`, live registers are `[3, 2, 1]`.
        let mut old = Circuit::new(Width::BITS_2, 1, 1, WireDelayMode::Combinational);
        let (ring_delays, ring_split) = build_autonomous_ring(&mut old, 3, &[1, 2, 3], 0);
        assert_eq!(ring_delays.len(), 3);
        let old_compiled = old.compile().unwrap();

        let mut sim = ScalarState::new(&old_compiled);
        let mut obs_missing = ObservationWindow::new(1, 16);
        let mut obs_mismatch = ObservationWindow::new(1, 16);
        let mut pred = [0u64];
        let structure_epoch = 1u64;
        for t in 0..4usize {
            let pre_regs = sim.registers().to_vec();
            sim.step(&old_compiled, &[0], &mut pred).unwrap();
            if t < 3 {
                obs_missing.push_frame(&pred);
                obs_mismatch.push_frame_with_state(&pred, structure_epoch - 1, &pre_regs);
            } else {
                obs_missing.push_frame_with_state(&pred, structure_epoch, &pre_regs);
                obs_mismatch.push_frame_with_state(&pred, structure_epoch, &pre_regs);
            }
        }
        assert!(obs_missing.state_snapshot_at(2).is_none());
        assert_eq!(
            obs_missing.state_snapshot_at(3).unwrap(),
            (1, [3, 2, 1].as_slice())
        );
        assert_eq!(
            obs_mismatch.state_snapshot_at(2).unwrap(),
            (0, [2, 1, 3].as_slice())
        );
        assert_eq!(
            obs_mismatch.state_snapshot_at(3).unwrap(),
            (1, [3, 2, 1].as_slice())
        );

        let mut staged = old.clone();
        let d_new = staged.add_node(Primitive::Delay { init: 0 });
        let tapped = tap_source(
            &mut staged,
            SignalSource::NodeOut {
                node: ring_split,
                port: 1,
            },
        );
        staged.connect(
            tapped,
            SignalSink::NodeIn {
                node: d_new,
                port: 0,
            },
        );
        let staged_compiled = staged.compile().unwrap();

        let old_to_new: Vec<Option<NodeId>> = (0..old.node_count())
            .map(|i| Some(NodeId(i as u32)))
            .collect();
        let reg_map = register_mapping(&old, &staged, &old_compiled, &staged_compiled, &old_to_new);
        let d_new_reg = explicit_delay_register_index(&staged, d_new).unwrap();

        // (a) Missing earlier snapshots (`None` at idx < 3): must return Unavailable and keep regs unchanged.
        let mut regs_missing = vec![0u64; staged_compiled.total_registers()];
        map_snapshot_regs_into(
            &reg_map,
            obs_missing.state_snapshot_at(3).unwrap().1,
            &mut regs_missing,
        );
        let expected_unchanged = regs_missing.clone();
        assert_eq!(expected_unchanged[..3], [3, 2, 1]);
        assert_eq!(expected_unchanged[d_new_reg], 0);
        assert_eq!(
            prime_non_surviving_registers(
                &staged,
                &staged_compiled,
                &old_to_new,
                &reg_map,
                None,
                &obs_missing,
                structure_epoch,
                3,
                3,
                &mut regs_missing,
            ),
            PrimingOutcome::Unavailable
        );
        assert_eq!(regs_missing, expected_unchanged);
        assert_ne!(regs_missing[d_new_reg], 1);

        // (b) Mismatched-epoch earlier snapshot (`structure_epoch - 1` at idx = 2): must return Unavailable and keep regs unchanged.
        let mut regs_mismatch = expected_unchanged.clone();
        assert_eq!(
            prime_non_surviving_registers(
                &staged,
                &staged_compiled,
                &old_to_new,
                &reg_map,
                None,
                &obs_mismatch,
                structure_epoch,
                3,
                3,
                &mut regs_mismatch,
            ),
            PrimingOutcome::Unavailable
        );
        assert_eq!(regs_mismatch, expected_unchanged);
        assert_ne!(regs_mismatch[d_new_reg], 1);
    }

    #[test]
    fn r1_residual_stage2_and_stage3_reject_cyclic_source_without_matching_snapshot() {
        let econ = EconomyConfig {
            juvenile_ticks: 0,
            ..EconomyConfig::default()
        };
        let growth = GrowthConfig {
            diagnostic_window: 2,
            min_match_gain: 1,
            ..GrowthConfig::default()
        };

        // Surviving period-3 cyclic ring [1, 2, 3] on port 0 in a 2-input, 1-output circuit.
        let mut old = Circuit::new(Width::BITS_2, 2, 1, WireDelayMode::Combinational);
        build_autonomous_ring(&mut old, 3, &[1, 2, 3], 0);
        let mut editable = EditableCircuit::new(old.clone(), econ).unwrap();
        let old_to_new: Vec<Option<NodeId>> = (0..old.node_count())
            .map(|i| Some(NodeId(i as u32)))
            .collect();
        editable
            .recompile_preserving_state(old, &old_to_new)
            .unwrap();
        assert_eq!(editable.structure_epoch(), 1);

        let mut sim = ScalarState::new(editable.compiled());
        let mut obs_missing = ObservationWindow::new(2, 16);
        let mut obs_mismatch = ObservationWindow::new(2, 16);
        let mut pred = [0u64];
        for t in 0..5usize {
            let pre_regs = sim.registers().to_vec();
            let ch1 = ((t % 3) + 1) as u64;
            sim.step(editable.compiled(), &[0, ch1], &mut pred).unwrap();
            let frame = [pred[0], ch1];
            if t < 3 {
                obs_missing.push_frame(&frame);
                obs_mismatch.push_frame_with_state(&frame, 0, &pre_regs);
            } else {
                obs_missing.push_frame_with_state(&frame, 1, &pre_regs);
                obs_mismatch.push_frame_with_state(&frame, 1, &pre_regs);
            }
        }

        for (label, obs) in [("missing", &obs_missing), ("mismatched", &obs_mismatch)] {
            let baseline = replay_current_baseline(
                &editable,
                obs,
                &growth,
                &econ,
                0,
                false,
                &mut GrowthEvalAudit::default(),
            );
            assert_eq!(baseline.start_idx, 3, "{label}: start_idx");
            assert_eq!(baseline.eval_start_idx, 3, "{label}: eval_start_idx");
            assert_eq!(baseline.w_replay, 2, "{label}: w_replay");

            // 1. Stage 2 (`evaluate_sprout_motif`): `LagYhat(0, 1)` creates a Delay fed by the cyclic ring.
            let mut s2_budget = TickBudget::new(100);
            let mut s2_tel = SproutTelemetry::default();
            let s2_out = evaluate_sprout_motif(
                &editable,
                obs,
                &growth,
                &econ,
                0,
                &CandidateMotif::Identity(SourceFeature::LagYhat { port: 0, tau: 1 }),
                false,
                false,
                &baseline,
                Some(&mut s2_budget),
                Some(&mut s2_tel),
                &mut GrowthEvalAudit::default(),
            );
            assert!(
                matches!(s2_out, SproutMotifOutcome::Rejected),
                "{label}: expected Stage 2 rejection"
            );
            assert_eq!(
                s2_tel.candidate_undecidable, 1,
                "{label}: Stage 2 undecidable"
            );
            assert_eq!(
                s2_tel.sprout_budget_exhausted, 0,
                "{label}: Stage 2 budget exhausted"
            );
            assert_eq!(
                s2_budget.consumed(),
                0,
                "{label}: Stage 2 zero unmetered budget consumed"
            );
            assert_eq!(s2_budget.remaining(), 100);

            // 2. Stage 3 (`evaluate_stage3_candidate_pre`):
            // 2a. 2-input, 2-output Combinational circuit: reactive primary on port 0, surviving
            //     period-3 cyclic ring on port 1 (`ring_out`), comparator spliced onto port 0
            //     whose `alt_delay` reads `ring_out` (`ring_out -> alt_delay -> Sub.in(1)`).
            let mut comb_old = Circuit::new(Width::BITS_2, 2, 2, WireDelayMode::Combinational);
            comb_old.connect(SignalSource::BoundaryIn(0), SignalSink::BoundaryOut(0));
            let (_comb_ring_delays, comb_ring_split) =
                build_autonomous_ring(&mut comb_old, 3, &[1, 2, 3], 1);
            let mut comb_editable = EditableCircuit::new(comb_old.clone(), econ).unwrap();
            let comb_old_to_new: Vec<Option<NodeId>> = (0..comb_old.node_count())
                .map(|i| Some(NodeId(i as u32)))
                .collect();
            comb_editable
                .recompile_preserving_state(comb_old, &comb_old_to_new)
                .unwrap();
            assert_eq!(comb_editable.structure_epoch(), 1);

            let mut comb_sim = ScalarState::new(comb_editable.compiled());
            let mut comb_obs_missing = ObservationWindow::new(2, 16);
            let mut comb_obs_mismatch = ObservationWindow::new(2, 16);
            let mut comb_pred = [0u64; 2];
            for t in 0..5usize {
                let pre_regs = comb_sim.registers().to_vec();
                let ch0 = ((t + 1) % 4) as u64;
                comb_sim
                    .step(comb_editable.compiled(), &[ch0, 0], &mut comb_pred)
                    .unwrap();
                let frame = [comb_pred[0], comb_pred[1]];
                if t < 3 {
                    comb_obs_missing.push_frame(&frame);
                    comb_obs_mismatch.push_frame_with_state(&frame, 0, &pre_regs);
                } else {
                    comb_obs_missing.push_frame_with_state(&frame, 1, &pre_regs);
                    comb_obs_mismatch.push_frame_with_state(&frame, 1, &pre_regs);
                }
            }
            let comb_obs = match label {
                "missing" => &comb_obs_missing,
                "mismatched" => &comb_obs_mismatch,
                _ => unreachable!(),
            };
            let comb_baseline = replay_current_baseline(
                &comb_editable,
                comb_obs,
                &growth,
                &econ,
                0,
                false,
                &mut GrowthEvalAudit::default(),
            );
            assert_eq!(comb_baseline.start_idx, 3, "{label}: comb start_idx");
            assert_eq!(
                comb_baseline.eval_start_idx, 3,
                "{label}: comb eval_start_idx"
            );
            assert_eq!(comb_baseline.w_replay, 2, "{label}: comb w_replay");

            let mut staged3_comb = comb_editable.circuit().clone();
            let comb_alt_delay = staged3_comb.add_node(Primitive::Delay { init: 0 });
            let ring_tap = tap_source(
                &mut staged3_comb,
                SignalSource::NodeOut {
                    node: comb_ring_split,
                    port: 1,
                },
            );
            staged3_comb.connect(
                ring_tap,
                SignalSink::NodeIn {
                    node: comb_alt_delay,
                    port: 0,
                },
            );
            let comb_alt_src = SignalSource::NodeOut {
                node: comb_alt_delay,
                port: 0,
            };
            let spliced3_comb =
                splice_comparator_onto_port(&mut staged3_comb, 0, comb_alt_src).unwrap();
            let excl3_comb =
                derivation_cone_info(comb_editable.circuit(), &[SignalSink::BoundaryOut(0)]);
            let mut s3_comb_budget = TickBudget::new(100);
            let mut s3_comb_tel = SproutTelemetry::default();
            let mut blocked3_comb = 0usize;
            let s3_comb_res = evaluate_stage3_candidate_pre(
                &comb_editable,
                comb_obs,
                &growth,
                &econ,
                &mut s3_comb_budget,
                0,
                0,
                &excl3_comb,
                &comb_baseline,
                2,
                staged3_comb,
                spliced3_comb,
                comb_alt_src,
                None,
                &mut blocked3_comb,
                &mut s3_comb_tel,
                &mut GrowthEvalAudit::default(),
            );
            assert!(
                s3_comb_res.is_none(),
                "{label}: expected Combinational Stage 3 rejection"
            );
            assert_eq!(blocked3_comb, 0, "{label}: Comb Stage 3 max_nodes");
            assert_eq!(
                s3_comb_tel.candidate_undecidable, 1,
                "{label}: Comb Stage 3 undecidable"
            );
            assert_eq!(
                s3_comb_tel.sprout_budget_exhausted, 0,
                "{label}: Comb Stage 3 budget exhausted"
            );
            assert_eq!(
                s3_comb_budget.consumed(),
                0,
                "{label}: Comb Stage 3 zero unmetered budget consumed"
            );
            assert_eq!(s3_comb_budget.remaining(), 100);

            // 2b. TicksPerHop circuit with a surviving autonomous ring on port 0
            //     (`tph_d0 -> tph_d1 -> tph_split -> BoundaryOut(0)`) and Stage 3 comparator
            //     (`BoundaryIn(1) -> alt_delay`, creating non-surviving hop-delayed edge
            //     `sub_node -[1]-> eqz_node`) with missing / mismatched-epoch earlier snapshots.
            let mut tph_old = Circuit::new(Width::BITS_2, 2, 1, WireDelayMode::TicksPerHop);
            let tph_d0 = tph_old.add_node(Primitive::Delay { init: 1 });
            let tph_d1 = tph_old.add_node(Primitive::Delay { init: 2 });
            let tph_split = tph_old.add_node(Primitive::Split);
            tph_old.connect(
                SignalSource::NodeOut {
                    node: tph_d0,
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: tph_d1,
                    port: 0,
                },
            );
            tph_old.connect(
                SignalSource::NodeOut {
                    node: tph_d1,
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: tph_split,
                    port: 0,
                },
            );
            tph_old.connect(
                SignalSource::NodeOut {
                    node: tph_split,
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: tph_d0,
                    port: 0,
                },
            );
            tph_old.connect(
                SignalSource::NodeOut {
                    node: tph_split,
                    port: 1,
                },
                SignalSink::BoundaryOut(0),
            );
            let mut tph_editable = EditableCircuit::new(tph_old.clone(), econ).unwrap();
            let tph_old_to_new: Vec<Option<NodeId>> = (0..tph_old.node_count())
                .map(|i| Some(NodeId(i as u32)))
                .collect();
            tph_editable
                .recompile_preserving_state(tph_old, &tph_old_to_new)
                .unwrap();
            assert_eq!(tph_editable.structure_epoch(), 1);

            let mut tph_sim = ScalarState::new(tph_editable.compiled());
            let mut tph_obs_missing = ObservationWindow::new(2, 16);
            let mut tph_obs_mismatch = ObservationWindow::new(2, 16);
            let mut tph_pred = [0u64];
            for t in 0..5usize {
                let pre_regs = tph_sim.registers().to_vec();
                let ch1 = ((t % 3) + 1) as u64;
                tph_sim
                    .step(tph_editable.compiled(), &[0, ch1], &mut tph_pred)
                    .unwrap();
                let frame = [tph_pred[0], ch1];
                if t < 3 {
                    tph_obs_missing.push_frame(&frame);
                    tph_obs_mismatch.push_frame_with_state(&frame, 0, &pre_regs);
                } else {
                    tph_obs_missing.push_frame_with_state(&frame, 1, &pre_regs);
                    tph_obs_mismatch.push_frame_with_state(&frame, 1, &pre_regs);
                }
            }
            let tph_obs = match label {
                "missing" => &tph_obs_missing,
                "mismatched" => &tph_obs_mismatch,
                _ => unreachable!(),
            };
            let tph_baseline = replay_current_baseline(
                &tph_editable,
                tph_obs,
                &growth,
                &econ,
                0,
                false,
                &mut GrowthEvalAudit::default(),
            );
            assert_eq!(tph_baseline.start_idx, 3, "{label}: tph start_idx");
            assert_eq!(
                tph_baseline.eval_start_idx, 3,
                "{label}: tph eval_start_idx"
            );
            assert_eq!(tph_baseline.w_replay, 2, "{label}: tph w_replay");

            let mut staged3_tph = tph_editable.circuit().clone();
            let tph_alt_delay = staged3_tph.add_node(Primitive::Delay { init: 0 });
            staged3_tph.connect(
                SignalSource::BoundaryIn(1),
                SignalSink::NodeIn {
                    node: tph_alt_delay,
                    port: 0,
                },
            );
            let tph_alt_src = SignalSource::NodeOut {
                node: tph_alt_delay,
                port: 0,
            };
            let spliced3_tph =
                splice_comparator_onto_port(&mut staged3_tph, 0, tph_alt_src).unwrap();
            assert!(spliced3_tph.primary_autonomous_shifted);
            let excl3_tph =
                derivation_cone_info(tph_editable.circuit(), &[SignalSink::BoundaryOut(0)]);
            let mut s3_tph_budget = TickBudget::new(100);
            let mut s3_tph_tel = SproutTelemetry::default();
            let mut blocked3_tph = 0usize;
            let s3_tph_res = evaluate_stage3_candidate_pre(
                &tph_editable,
                tph_obs,
                &growth,
                &econ,
                &mut s3_tph_budget,
                0,
                0,
                &excl3_tph,
                &tph_baseline,
                2,
                staged3_tph,
                spliced3_tph,
                tph_alt_src,
                None,
                &mut blocked3_tph,
                &mut s3_tph_tel,
                &mut GrowthEvalAudit::default(),
            );
            assert!(
                s3_tph_res.is_none(),
                "{label}: expected TicksPerHop Stage 3 rejection"
            );
            assert_eq!(blocked3_tph, 0, "{label}: TPH Stage 3 max_nodes");
            assert_eq!(
                s3_tph_tel.candidate_undecidable, 1,
                "{label}: TPH Stage 3 undecidable"
            );
            assert_eq!(
                s3_tph_tel.sprout_budget_exhausted, 0,
                "{label}: TPH Stage 3 budget exhausted"
            );
            assert_eq!(
                s3_tph_budget.consumed(),
                0,
                "{label}: TPH Stage 3 zero unmetered budget consumed"
            );
            assert_eq!(s3_tph_budget.remaining(), 100);
        }
    }

    #[test]
    fn r1_residual_combinational_join_with_cyclic_primary_and_reactive_delay_still_primes() {
        // Surviving period-3 cyclic ring [1, 2, 3] on port 0 in a 2-input, 1-output Combinational circuit.
        let mut old = Circuit::new(Width::BITS_2, 2, 1, WireDelayMode::Combinational);
        let (ring_delays, _ring_split) = build_autonomous_ring(&mut old, 3, &[1, 2, 3], 0);
        assert_eq!(ring_delays.len(), 3);
        let old_compiled = old.compile().unwrap();

        let mut sim = ScalarState::new(&old_compiled);
        let mut obs_missing = ObservationWindow::new(2, 16);
        let mut obs_mismatch = ObservationWindow::new(2, 16);
        let mut pred = [0u64];
        let structure_epoch = 1u64;
        for t in 0..4usize {
            let pre_regs = sim.registers().to_vec();
            let ch1 = ((t % 3) + 1) as u64;
            sim.step(&old_compiled, &[0, ch1], &mut pred).unwrap();
            let frame = [pred[0], ch1];
            if t < 3 {
                obs_missing.push_frame(&frame);
                obs_mismatch.push_frame_with_state(&frame, structure_epoch - 1, &pre_regs);
            } else {
                obs_missing.push_frame_with_state(&frame, structure_epoch, &pre_regs);
                obs_mismatch.push_frame_with_state(&frame, structure_epoch, &pre_regs);
            }
        }
        assert!(obs_missing.state_snapshot_at(2).is_none());
        assert_eq!(
            obs_missing.state_snapshot_at(3).unwrap(),
            (1, [3, 2, 1].as_slice())
        );
        assert_eq!(
            obs_mismatch.state_snapshot_at(2).unwrap(),
            (0, [2, 1, 3].as_slice())
        );
        assert_eq!(
            obs_mismatch.state_snapshot_at(3).unwrap(),
            (1, [3, 2, 1].as_slice())
        );

        let old_to_new: Vec<Option<NodeId>> = (0..old.node_count())
            .map(|i| Some(NodeId(i as u32)))
            .collect();
        let start_idx = 3usize;
        // At step `idx = start_idx - 1 = 2`, the input fed to `step` is `frames[start_idx - 2][1] = frames[1][1] = 2`.
        let expected_reactive_ch1 = obs_missing.frames()[start_idx - 2][1];
        assert_eq!(expected_reactive_ch1, 2);

        // (a) Stage 3 comparator `Sub(ring_out, alt_delay)` where `alt_delay` reads `BoundaryIn(1)`.
        let mut staged3 = old.clone();
        let alt_delay = staged3.add_node(Primitive::Delay { init: 0 });
        staged3.connect(
            SignalSource::BoundaryIn(1),
            SignalSink::NodeIn {
                node: alt_delay,
                port: 0,
            },
        );
        let alt_src = SignalSource::NodeOut {
            node: alt_delay,
            port: 0,
        };
        let spliced3 = splice_comparator_onto_port(&mut staged3, 0, alt_src).unwrap();
        let staged3_compiled = staged3.compile().unwrap();
        let reg_map3 = register_mapping(
            &old,
            &staged3,
            &old_compiled,
            &staged3_compiled,
            &old_to_new,
        );
        let alt_delay_reg = explicit_delay_register_index(&staged3, alt_delay).unwrap();
        let protected3 = spliced3
            .inserted_split
            .filter(|_| spliced3.primary_autonomous_shifted);

        // (b) Stage 2 combinational `Add(ring_out, Delay(BoundaryIn(1)))`.
        let mut staged2 = old.clone();
        let stage2_delay = staged2.add_node(Primitive::Delay { init: 0 });
        staged2.connect(
            SignalSource::BoundaryIn(1),
            SignalSink::NodeIn {
                node: stage2_delay,
                port: 0,
            },
        );
        let _stage2_add = stack_output_binary_op(
            &mut staged2,
            Primitive::Add,
            0,
            SignalSource::NodeOut {
                node: stage2_delay,
                port: 0,
            },
        );
        let staged2_compiled = staged2.compile().unwrap();
        let reg_map2 = register_mapping(
            &old,
            &staged2,
            &old_compiled,
            &staged2_compiled,
            &old_to_new,
        );
        let stage2_delay_reg = explicit_delay_register_index(&staged2, stage2_delay).unwrap();

        for (label, obs) in [("missing", &obs_missing), ("mismatched", &obs_mismatch)] {
            let mut regs3 = vec![0u64; staged3_compiled.total_registers()];
            map_snapshot_regs_into(
                &reg_map3,
                obs.state_snapshot_at(start_idx).unwrap().1,
                &mut regs3,
            );
            assert_eq!(regs3[..3], [3, 2, 1], "{label}: Stage 3 initial ring regs");
            assert_eq!(
                regs3[alt_delay_reg], 0,
                "{label}: Stage 3 initial alt_delay"
            );
            assert_eq!(
                prime_non_surviving_registers(
                    &staged3,
                    &staged3_compiled,
                    &old_to_new,
                    &reg_map3,
                    protected3,
                    obs,
                    structure_epoch,
                    start_idx,
                    start_idx,
                    &mut regs3,
                ),
                PrimingOutcome::Primed,
                "{label}: Stage 3 must prime"
            );
            assert_eq!(
                regs3[..3],
                [3, 2, 1],
                "{label}: Stage 3 preserves surviving ring regs"
            );
            assert_eq!(
                regs3[alt_delay_reg], expected_reactive_ch1,
                "{label}: Stage 3 primes alt_delay to exact BoundaryIn(1) at start_idx - 1"
            );

            let mut regs2 = vec![0u64; staged2_compiled.total_registers()];
            map_snapshot_regs_into(
                &reg_map2,
                obs.state_snapshot_at(start_idx).unwrap().1,
                &mut regs2,
            );
            assert_eq!(regs2[..3], [3, 2, 1], "{label}: Stage 2 initial ring regs");
            assert_eq!(
                regs2[stage2_delay_reg], 0,
                "{label}: Stage 2 initial new Delay"
            );
            assert_eq!(
                prime_non_surviving_registers(
                    &staged2,
                    &staged2_compiled,
                    &old_to_new,
                    &reg_map2,
                    None,
                    obs,
                    structure_epoch,
                    start_idx,
                    start_idx,
                    &mut regs2,
                ),
                PrimingOutcome::Primed,
                "{label}: Stage 2 must prime"
            );
            assert_eq!(
                regs2[..3],
                [3, 2, 1],
                "{label}: Stage 2 preserves surviving ring regs"
            );
            assert_eq!(
                regs2[stage2_delay_reg], expected_reactive_ch1,
                "{label}: Stage 2 primes new Delay to exact BoundaryIn(1) at start_idx - 1"
            );
        }
    }
}
