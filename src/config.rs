//! Serializable experiment configurations, ground-truth manifests, provenance, and reproducible run execution.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::baselines::{
    CgpBaseline, CrossChannelCtwBaseline, CtwBaseline, DEFAULT_CROSS_CTW_SYMBOL_DEPTH,
    DEFAULT_MAX_CERTIFY_JOINT_STEPS, DEFAULT_RNN_BPTT_LEN, DEFAULT_RNN_HIDDEN_SIZE,
    DEFAULT_RNN_LEARNING_RATE, ExhaustiveBaseline, PredictorBaseline, RandomSearchBaseline,
    ReedsSloaneBaseline, RnnBaseline, RnnError, SynthesisCertStatus,
    exact_synthesize_certified_circuit, run_prequential_harness,
};
use crate::env::{
    EpsilonMachineScope, EpsilonMachineSummary, FiniteStateGenerator, FiniteStateSpec,
    GeneratorConfig, GeneratorError, NoiseError, NoiseModel, NoisyEnvironment,
    ReedsSloaneSynthesizer, declared_noise_entropy_bits_per_symbol, solve_epsilon_machine,
};
use crate::metrics::Scorecard;
use crate::rng::{SeedBank, fnv1a_128_hex};
use crate::structural::{ArkheLearner, EconomyConfig, GrowthConfig, GrowthError};
use crate::substrate::{Circuit, Width, WireDelayMode};

/// Current configuration schema version.
pub const CONFIG_SCHEMA_VERSION: u32 = 1;

/// Maximum clean trace ticks synthesised to certify full-cycle Reeds–Sloane linear complexity.
const MAX_CERTIFIED_REEDS_SLOANE_TICKS: usize = 4_096;

/// Compile-time binary, toolchain, and source-tree provenance embedded in output manifests.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildProvenance {
    /// Crate version (`CARGO_PKG_VERSION`).
    pub package_version: String,
    /// Rust compiler version string (`rustc --version`).
    pub rustc_version: String,
    /// Target architecture triple (`TARGET`).
    pub target_triple: String,
    /// Git commit SHA when built inside a Git repository (`None` inside `cargo package` tarballs).
    pub git_commit_sha: Option<String>,
    /// Whether the working tree had uncommitted modifications at build time (`None` outside Git).
    pub git_dirty: Option<bool>,
    /// 128-bit FNV-1a digest of `Cargo.lock` (`"unavailable"` if absent).
    pub cargo_lock_fnv1a128: String,
    /// 128-bit FNV-1a digest of the canonical build inputs and Rust source tree.
    pub source_tree_fnv1a128: String,
}

impl BuildProvenance {
    /// Returns the compile-time provenance embedded into this binary by `scripts/build.rs`.
    #[must_use]
    pub fn current() -> Self {
        let raw_sha = env!("ARKHE_GIT_SHA");
        let git_commit_sha = (raw_sha != "none").then(|| raw_sha.to_owned());
        let git_dirty = match env!("ARKHE_GIT_DIRTY") {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        };
        Self {
            package_version: env!("CARGO_PKG_VERSION").to_owned(),
            rustc_version: env!("ARKHE_RUSTC_VERSION").to_owned(),
            target_triple: env!("ARKHE_TARGET").to_owned(),
            git_commit_sha,
            git_dirty,
            cargo_lock_fnv1a128: env!("ARKHE_CARGO_LOCK_FNV1A128").to_owned(),
            source_tree_fnv1a128: env!("ARKHE_SOURCE_TREE_FNV1A128").to_owned(),
        }
    }
}

/// Error returned when validating or running an [`ExperimentConfig`].
#[derive(Debug)]
pub enum ConfigRunError {
    /// Unsupported `schema_version`.
    UnsupportedSchemaVersion(u32),
    /// `transient_warmup_ticks` must be strictly less than `horizon_ticks`.
    InvalidHorizon { horizon: u64, warmup: u64 },
    /// Generator construction failed.
    Generator(GeneratorError),
    /// Noise configuration failed validation.
    Noise(NoiseError),
    /// Growth or diagnostics operation failed.
    Growth(GrowthError),
    /// RNN baseline configuration failed validation.
    Rnn(RnnError),
    /// TOML/JSON serialisation error.
    Serialization(String),
}

impl fmt::Display for ConfigRunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchemaVersion(v) => {
                write!(
                    f,
                    "unsupported schema_version {v} (expected {CONFIG_SCHEMA_VERSION})"
                )
            }
            Self::InvalidHorizon { horizon, warmup } => write!(
                f,
                "invalid horizon_ticks={horizon} with transient_warmup_ticks={warmup} (expected warmup < horizon)"
            ),
            Self::Generator(e) => write!(f, "generator error: {e}"),
            Self::Noise(e) => write!(f, "noise error: {e}"),
            Self::Growth(e) => write!(f, "growth error: {e:?}"),
            Self::Rnn(e) => write!(f, "rnn error: {e}"),
            Self::Serialization(e) => write!(f, "serialization error: {e}"),
        }
    }
}

impl std::error::Error for ConfigRunError {}

impl From<GeneratorError> for ConfigRunError {
    fn from(value: GeneratorError) -> Self {
        Self::Generator(value)
    }
}

impl From<NoiseError> for ConfigRunError {
    fn from(value: NoiseError) -> Self {
        Self::Noise(value)
    }
}

impl From<GrowthError> for ConfigRunError {
    fn from(value: GrowthError) -> Self {
        Self::Growth(value)
    }
}

impl From<RnnError> for ConfigRunError {
    fn from(value: RnnError) -> Self {
        Self::Rnn(value)
    }
}

/// Substrate configuration block in [`ExperimentConfig`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubstrateConfig {
    /// Uniform bit-width $k \in 1..=64$.
    pub width_bits: Width,
    /// Combinational vs. hop-delayed wire mode.
    #[serde(default)]
    pub wire_delay: WireDelayMode,
}

/// Environment configuration block in [`ExperimentConfig`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EnvironmentConfig {
    /// Total ticks simulated (`0..horizon_ticks`).
    pub horizon_ticks: u64,
    /// Initial transient ticks excluded from post-transient loss averages.
    pub transient_warmup_ticks: u64,
    /// Deterministic benchmark generator specification.
    pub generator: GeneratorConfig,
    /// Optional declared channel substitution noise.
    #[serde(default)]
    pub noise: NoiseModel,
}

/// Equal-compute tick budget block in [`ExperimentConfig`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetConfig {
    /// Maximum internal candidate-evaluation ticks allowed across the run.
    pub max_eval_ticks: u64,
}

/// Baseline selector and hyperparameters in [`ExperimentConfig`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum BaselineConfig {
    /// Constructive random valid-circuit search.
    RandomSearch {
        max_nodes: usize,
        #[serde(default = "default_fact_window")]
        fact_window: usize,
        #[serde(default = "default_samples_per_error")]
        samples_per_error: usize,
    },
    /// $(1 + \lambda)$ Cartesian Genetic Programming with `SPLIT`-explicit lowering.
    Cgp {
        #[serde(default = "default_cgp_lambda")]
        lambda: usize,
        #[serde(default = "default_cgp_genes")]
        max_genes: usize,
        #[serde(default = "default_fact_window")]
        fact_window: usize,
    },
    /// Bit-decomposed Context-Tree Weighting universal predictor.
    Ctw {
        #[serde(default = "default_ctw_depth")]
        context_depth: usize,
    },
    /// Joint cross-channel interleaved bit-decomposed Context-Tree Weighting predictor.
    CrossChannelCtw {
        #[serde(default = "default_cross_ctw_symbol_depth")]
        symbol_context_depth: usize,
    },
    /// 1-layer `tanh` Elman RNN trained online with truncated BPTT.
    RnnBptt {
        #[serde(default = "default_rnn_hidden_size")]
        hidden_size: usize,
        #[serde(default = "default_rnn_bptt_len")]
        bptt_len: usize,
        #[serde(default = "default_rnn_learning_rate")]
        learning_rate: f64,
    },
    /// Online prequential Reeds–Sloane / Berlekamp–Massey linear recurrence synthesizer.
    ReedsSloane,
    /// Canonical size-ordered small-circuit exhaustive enumerator.
    Exhaustive {
        #[serde(default = "default_exhaustive_nodes")]
        max_nodes: usize,
        #[serde(default = "default_fact_window")]
        fact_window: usize,
    },
    /// Online gradient-free structural learning from an empty circuit.
    ArkheM1 {
        #[serde(default)]
        economy: EconomyConfig,
        #[serde(default)]
        growth: GrowthConfig,
    },
}

const fn default_fact_window() -> usize {
    128
}
const fn default_samples_per_error() -> usize {
    32
}
const fn default_cgp_lambda() -> usize {
    4
}
const fn default_cgp_genes() -> usize {
    8
}
const fn default_ctw_depth() -> usize {
    12
}
const fn default_cross_ctw_symbol_depth() -> usize {
    DEFAULT_CROSS_CTW_SYMBOL_DEPTH
}
const fn default_rnn_hidden_size() -> usize {
    DEFAULT_RNN_HIDDEN_SIZE
}
const fn default_rnn_bptt_len() -> usize {
    DEFAULT_RNN_BPTT_LEN
}
const fn default_rnn_learning_rate() -> f64 {
    DEFAULT_RNN_LEARNING_RATE
}
const fn default_exhaustive_nodes() -> usize {
    3
}

/// Top-level reproducible experiment configuration (TOML / JSON).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExperimentConfig {
    /// Schema version (`1`).
    pub schema_version: u32,
    /// Master 64-bit RNG seed governing all deterministic sub-streams.
    pub master_seed: u64,
    /// Substrate width, wire-delay, and topology settings.
    pub substrate: SubstrateConfig,
    /// Environment generator, horizon, and noise settings.
    pub environment: EnvironmentConfig,
    /// Equal-compute candidate-evaluation budget.
    pub budget: BudgetConfig,
    /// Baseline predictor and hyperparameters.
    pub baseline: BaselineConfig,
}

/// Analytical ground-truth yardstick summary for an environment configuration (§5.2).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroundTruthReport {
    /// Exact $\varepsilon$-machine summary when $|S| \le \text{DEFAULT\_MAX\_EXPORT\_STATES}$.
    pub epsilon_machine: Option<EpsilonMachineSummary>,
    /// Explicit reason when `epsilon_machine` could not be exported (`None` when exported cleanly).
    pub epsilon_machine_error: Option<String>,
    /// Analytical per-symbol substitution channel entropy rate $H_{\text{channel}}$ (available even when $|S|$ exceeds export bounds).
    pub declared_noise_entropy_bits_per_symbol: f64,
    /// Minimal monic linear recurrence length $L$ over $\mathbb{Z}_{2^k}$ per channel on the evaluated clean trace.
    pub reeds_sloane_min_len: Vec<usize>,
    /// Number of clean ticks fed into Reeds–Sloane synthesis.
    pub reeds_sloane_prefix_ticks: usize,
    /// `true` iff `reeds_sloane_prefix_ticks` covered at least $2(t_0 + P)$ ticks of a stationary process.
    pub reeds_sloane_certified_full_cycle: bool,
    /// Minimum Level-0 node count (up to `exact_synth_cap`) achieving 0 clean post-sync errors.
    pub exact_synthesis_min_nodes: Option<usize>,
    /// Certification status distinguishing full-cycle proof from finite-prefix fallback or cap exhaustion.
    pub exact_synthesis_status: SynthesisCertStatus,
}

/// Detailed online telemetry from [`crate::structural::learner::ArkheLearner`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArkheM1Telemetry {
    pub final_node_count: usize,
    pub final_split_count: usize,
    pub final_delay_count: usize,
    pub final_total_energy: f64,
    pub min_mature_node_energy: Option<f64>,
    pub cumulative_net_flux: f64,
    pub cumulative_maturity_debt_paid: f64,
    pub null_calibrated_mdl_delta_bits: f64,
    pub sprout_events: u64,
    pub ring_closure_events: u64,
    pub starvation_prune_events: u64,
    pub idle_prune_events: u64,
    #[serde(default)]
    pub reserve_clipped_count: u64,
    #[serde(default)]
    pub reserve_clipped_bits: f64,
    #[serde(default)]
    pub sprout_blocked_max_nodes: u64,
    #[serde(default)]
    pub candidate_undecidable: u64,
    #[serde(default)]
    pub candidate_list_truncated: u64,
    #[serde(default, alias = "lag_horizon_exhausted")]
    pub lag_horizon_bound: u64,
    #[serde(default)]
    pub ring_period_bound: u64,
    #[serde(default)]
    pub reserve_mismatch_floored: u64,
    #[serde(default)]
    pub lag_deepening_steps: u64,
    #[serde(default)]
    pub sprout_budget_exhausted: u64,
    #[serde(default)]
    pub redundancy_sprout_events: u64,
    #[serde(default)]
    pub active_comparator_count: usize,
    #[serde(default)]
    pub comparator_agreement_ticks: u64,
    #[serde(default)]
    pub comparator_disagreement_ticks: u64,
    #[serde(default)]
    pub cumulative_comparator_flux: f64,
    #[serde(default)]
    pub driver_handoff_events: u64,
    #[serde(default)]
    pub disagreement_prune_events: u64,
    #[serde(default)]
    pub local_excision_events: usize,
    #[serde(default)]
    pub exogenous_injection_bits: f64,
}

/// Aggregated cliff, bound, and guardrail telemetry across one or more learner runs (§B3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CliffCountersSummary {
    pub reserve_clipped_count: u64,
    pub reserve_clipped_bits: f64,
    pub reserve_mismatch_floored: u64,
    pub sprout_blocked_max_nodes: u64,
    pub candidate_undecidable: u64,
    pub candidate_list_truncated: u64,
    pub lag_horizon_bound: u64,
    pub ring_period_bound: u64,
    #[serde(default)]
    pub lag_deepening_steps: u64,
    #[serde(default)]
    pub sprout_budget_exhausted: u64,
}

impl From<&ArkheM1Telemetry> for CliffCountersSummary {
    fn from(telemetry: &ArkheM1Telemetry) -> Self {
        Self {
            reserve_clipped_count: telemetry.reserve_clipped_count,
            reserve_clipped_bits: telemetry.reserve_clipped_bits,
            reserve_mismatch_floored: telemetry.reserve_mismatch_floored,
            sprout_blocked_max_nodes: telemetry.sprout_blocked_max_nodes,
            candidate_undecidable: telemetry.candidate_undecidable,
            candidate_list_truncated: telemetry.candidate_list_truncated,
            lag_horizon_bound: telemetry.lag_horizon_bound,
            ring_period_bound: telemetry.ring_period_bound,
            lag_deepening_steps: telemetry.lag_deepening_steps,
            sprout_budget_exhausted: telemetry.sprout_budget_exhausted,
        }
    }
}

impl CliffCountersSummary {
    /// Adds another summary's counters into `self`.
    pub fn accumulate(&mut self, other: &Self) {
        self.reserve_clipped_count += other.reserve_clipped_count;
        self.reserve_clipped_bits += other.reserve_clipped_bits;
        self.reserve_mismatch_floored += other.reserve_mismatch_floored;
        self.sprout_blocked_max_nodes += other.sprout_blocked_max_nodes;
        self.candidate_undecidable += other.candidate_undecidable;
        self.candidate_list_truncated += other.candidate_list_truncated;
        self.lag_horizon_bound += other.lag_horizon_bound;
        self.ring_period_bound += other.ring_period_bound;
        self.lag_deepening_steps += other.lag_deepening_steps;
        self.sprout_budget_exhausted += other.sprout_budget_exhausted;
    }

    /// Returns whether the hard compute guardrail (`max_nodes`) bound during execution.
    #[must_use]
    pub const fn guardrail_bound(&self) -> bool {
        self.sprout_blocked_max_nodes > 0
    }
}

/// Complete, deterministic output manifest produced by [`execute_experiment`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunReport {
    /// Schema version (`1`).
    pub schema_version: u32,
    /// Compile-time binary, toolchain, and source provenance.
    pub provenance: BuildProvenance,
    /// Deterministic 128-bit FNV-1a hex digest of canonical JSON-encoded `config`.
    pub config_digest_fnv1a128: String,
    /// Full input configuration that generated this report.
    pub config: ExperimentConfig,
    /// Predictor display name.
    pub baseline_name: String,
    /// Exact ground-truth yardstick metrics for the configured task.
    pub ground_truth: GroundTruthReport,
    /// Standardised performance scorecard.
    pub scorecard: Scorecard,
    /// Winning substrate circuit, if the baseline produces circuits.
    pub discovered_circuit: Option<Circuit>,
    /// Detailed M1 learner telemetry, present only for ArkheM1 runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub m1_telemetry: Option<ArkheM1Telemetry>,
}

fn spec_transient_and_period(spec: &FiniteStateSpec) -> Option<(usize, usize)> {
    if spec.num_states == 0 || spec.initial_state >= spec.num_states {
        return None;
    }
    let mut visited = vec![None; spec.num_states];
    let mut curr = spec.initial_state;
    let mut step = 0usize;
    loop {
        if let Some(first_seen) = visited[curr] {
            return Some((first_seen, step - first_seen));
        }
        visited[curr] = Some(step);
        curr = spec.transitions[curr].0;
        step += 1;
    }
}

/// Computes exact ground-truth yardstick quantities for `(generator_cfg, width, wire_delay, noise)`.
///
/// # Errors
///
/// Returns [`ConfigRunError`] if `generator_cfg` or `noise` fails validation.
pub fn compute_ground_truth_yardstick(
    generator_cfg: &GeneratorConfig,
    width: Width,
    wire_delay: WireDelayMode,
    noise: &NoiseModel,
    reference_ticks: usize,
    exact_synth_max_nodes: Option<usize>,
) -> Result<GroundTruthReport, ConfigRunError> {
    let mut clean_gen = generator_cfg.build(width)?;
    let channels = clean_gen.channels();
    noise.validate(channels)?;

    let declared_noise_entropy = declared_noise_entropy_bits_per_symbol(noise, channels, width);

    let (spec_opt, epsilon_machine, epsilon_machine_error) = match clean_gen.to_finite_state_spec()
    {
        Ok(spec) => {
            let em = solve_epsilon_machine(&spec, noise);
            (Some(spec), Some(em), None)
        }
        Err(err) => (None, None, Some(err.to_string())),
    };

    let base_ticks = reference_ticks.max(64);
    let (reeds_sloane_prefix_ticks, reeds_sloane_certified_full_cycle) = if let Some(ref spec) =
        spec_opt
        && spec.scope == EpsilonMachineScope::StationaryProcess
        && let Some((t0, period)) = spec_transient_and_period(spec)
    {
        let full_cycle_bound = 2usize.saturating_mul(t0.saturating_add(period));
        if full_cycle_bound <= MAX_CERTIFIED_REEDS_SLOANE_TICKS {
            (base_ticks.max(full_cycle_bound), true)
        } else {
            (base_ticks, base_ticks >= full_cycle_bound)
        }
    } else {
        (base_ticks, false)
    };

    let mut clean_trace = Vec::with_capacity(reeds_sloane_prefix_ticks);
    let mut frame = vec![0u64; usize::from(channels)];
    let mut synths: Vec<ReedsSloaneSynthesizer> = (0..channels)
        .map(|_| ReedsSloaneSynthesizer::new(width))
        .collect();

    for _ in 0..reeds_sloane_prefix_ticks {
        clean_gen.next_clean_frame(&mut frame);
        for (ch, &sym) in frame.iter().enumerate() {
            synths[ch].push_symbol(sym);
        }
        clean_trace.push(frame.clone());
    }

    let reeds_sloane_min_len = synths
        .iter()
        .map(ReedsSloaneSynthesizer::minimal_length)
        .collect();

    let stationary_spec = spec_opt
        .as_ref()
        .filter(|s| s.scope == EpsilonMachineScope::StationaryProcess);
    let (exact_synthesis_min_nodes, exact_synthesis_status) = match exact_synth_max_nodes {
        Some(cap) => {
            let (circ_opt, status) = exact_synthesize_certified_circuit(
                width,
                channels,
                wire_delay,
                stationary_spec,
                &clean_trace,
                cap,
                DEFAULT_MAX_CERTIFY_JOINT_STEPS,
            );
            (circ_opt.map(|c| c.node_count()), status)
        }
        None => (
            None,
            SynthesisCertStatus::SkippedBoundsExceeded {
                reason: "exact_synth_max_nodes disabled".to_owned(),
            },
        ),
    };

    Ok(GroundTruthReport {
        epsilon_machine,
        epsilon_machine_error,
        declared_noise_entropy_bits_per_symbol: declared_noise_entropy,
        reeds_sloane_min_len,
        reeds_sloane_prefix_ticks,
        reeds_sloane_certified_full_cycle,
        exact_synthesis_min_nodes,
        exact_synthesis_status,
    })
}

/// Executes a single reproducible experiment from `config` and returns its [`RunReport`].
///
/// # Errors
///
/// Returns [`ConfigRunError`] if configuration validation fails.
pub fn execute_experiment(config: &ExperimentConfig) -> Result<RunReport, ConfigRunError> {
    if config.schema_version != CONFIG_SCHEMA_VERSION {
        return Err(ConfigRunError::UnsupportedSchemaVersion(
            config.schema_version,
        ));
    }
    if config.environment.transient_warmup_ticks >= config.environment.horizon_ticks {
        return Err(ConfigRunError::InvalidHorizon {
            horizon: config.environment.horizon_ticks,
            warmup: config.environment.transient_warmup_ticks,
        });
    }

    let canonical_json =
        serde_json::to_vec(config).map_err(|e| ConfigRunError::Serialization(e.to_string()))?;
    let config_digest_fnv1a128 = fnv1a_128_hex(&canonical_json);

    let width = config.substrate.width_bits;
    let wire_delay = config.substrate.wire_delay;

    // Compute exact ground-truth yardstick (including up to 3-node certified synthesis).
    let synth_cap = Some(3);
    let ground_truth = compute_ground_truth_yardstick(
        &config.environment.generator,
        width,
        wire_delay,
        &config.environment.noise,
        usize::try_from(config.environment.horizon_ticks)
            .unwrap_or(256)
            .min(512),
        synth_cap,
    )?;

    let seeds = SeedBank::new(config.master_seed);
    let generator = config.environment.generator.build(width)?;
    let channels = generator.channels();
    let mut noisy_env = NoisyEnvironment::new(generator, config.environment.noise.clone(), seeds)?;

    let mut predictor: Box<dyn PredictorBaseline> = match &config.baseline {
        BaselineConfig::RandomSearch {
            max_nodes,
            fact_window,
            samples_per_error,
        } => Box::new(RandomSearchBaseline::new(
            width,
            channels,
            wire_delay,
            *max_nodes,
            *fact_window,
            *samples_per_error,
            seeds,
        )),
        BaselineConfig::Cgp {
            lambda,
            max_genes,
            fact_window,
        } => Box::new(CgpBaseline::new(
            width,
            channels,
            wire_delay,
            *lambda,
            *max_genes,
            *fact_window,
            seeds,
        )),
        BaselineConfig::Ctw { context_depth } => {
            Box::new(CtwBaseline::new(width, channels, *context_depth))
        }
        BaselineConfig::CrossChannelCtw {
            symbol_context_depth,
        } => Box::new(CrossChannelCtwBaseline::new(
            width,
            channels,
            *symbol_context_depth,
        )),
        BaselineConfig::RnnBptt {
            hidden_size,
            bptt_len,
            learning_rate,
        } => Box::new(RnnBaseline::new(
            width,
            channels,
            *hidden_size,
            *bptt_len,
            *learning_rate,
            seeds,
        )?),
        BaselineConfig::ReedsSloane => Box::new(ReedsSloaneBaseline::new(width, channels)),
        BaselineConfig::Exhaustive {
            max_nodes,
            fact_window,
        } => Box::new(ExhaustiveBaseline::new(
            width,
            channels,
            wire_delay,
            *max_nodes,
            *fact_window,
        )),
        BaselineConfig::ArkheM1 { economy, growth } => Box::new(ArkheLearner::new_with_config(
            width, channels, wire_delay, *economy, *growth, seeds,
        )?),
    };

    let scorecard = run_prequential_harness(
        &mut noisy_env,
        predictor.as_mut(),
        config.environment.horizon_ticks,
        config.environment.transient_warmup_ticks,
        config.budget.max_eval_ticks,
        ground_truth.epsilon_machine.as_ref(),
        ground_truth.exact_synthesis_min_nodes,
    );

    let baseline_name = predictor.name().to_owned();
    let discovered_circuit = predictor.best_circuit().cloned();
    let m1_telemetry = predictor.m1_telemetry();

    Ok(RunReport {
        schema_version: CONFIG_SCHEMA_VERSION,
        provenance: BuildProvenance::current(),
        config_digest_fnv1a128,
        config: config.clone(),
        baseline_name,
        ground_truth,
        scorecard,
        discovered_circuit,
        m1_telemetry,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::baselines::{MAX_RNN_ONE_HOT_WIDTH_BITS, RnnError};
    use crate::env::{GeneratorConfig, LfsrStage, NoiseModel};
    use crate::metrics::BudgetAccountingMode;
    use crate::substrate::{Width, WireDelayMode};

    fn multi_channel_experiment_config(
        width_bits: u8,
        baseline: BaselineConfig,
    ) -> ExperimentConfig {
        ExperimentConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            master_seed: 42,
            substrate: SubstrateConfig {
                width_bits: Width::new(width_bits).expect("valid width"),
                wire_delay: WireDelayMode::Combinational,
            },
            environment: EnvironmentConfig {
                generator: GeneratorConfig::T2_DelayedAffine {
                    driver: LfsrStage {
                        taps: vec![1, 0, 0, 1],
                        init: vec![0, 0, 0, 1],
                    },
                    lag: 2,
                    mul_a: 1,
                    add_b: 1,
                },
                horizon_ticks: 80,
                transient_warmup_ticks: 16,
                noise: NoiseModel::None,
            },
            budget: BudgetConfig {
                max_eval_ticks: 1_000_000,
            },
            baseline,
        }
    }

    #[test]
    fn cross_channel_ctw_and_rnn_execute_experiment_determinism_and_budget_modes() {
        let cfg_ctw = multi_channel_experiment_config(
            1,
            BaselineConfig::CrossChannelCtw {
                symbol_context_depth: DEFAULT_CROSS_CTW_SYMBOL_DEPTH,
            },
        );
        let rep_ctw_a = execute_experiment(&cfg_ctw).expect("cross_ctw run A succeeds");
        let rep_ctw_b = execute_experiment(&cfg_ctw).expect("cross_ctw run B succeeds");
        assert_eq!(rep_ctw_a.scorecard, rep_ctw_b.scorecard);
        assert_eq!(
            rep_ctw_a.scorecard.budget_accounting_mode,
            BudgetAccountingMode::ContextTreeDepthSteps
        );
        // 80 steps * bit_depth (9 * 2 * 1 = 18) = 1440 ticks
        assert_eq!(rep_ctw_a.scorecard.candidate_replay_ticks_consumed, 80 * 18);

        let cfg_rnn = multi_channel_experiment_config(
            2,
            BaselineConfig::RnnBptt {
                hidden_size: DEFAULT_RNN_HIDDEN_SIZE,
                bptt_len: DEFAULT_RNN_BPTT_LEN,
                learning_rate: DEFAULT_RNN_LEARNING_RATE,
            },
        );
        let rep_rnn_a = execute_experiment(&cfg_rnn).expect("rnn run A succeeds");
        let rep_rnn_b = execute_experiment(&cfg_rnn).expect("rnn run B succeeds");
        assert_eq!(rep_rnn_a.scorecard, rep_rnn_b.scorecard);
        assert_eq!(
            rep_rnn_a.scorecard.budget_accounting_mode,
            BudgetAccountingMode::BpttUnrollSteps
        );
        assert!(rep_rnn_a.scorecard.candidate_replay_ticks_consumed > 0);
    }

    #[test]
    fn rnn_bptt_rejects_width_exceeding_one_hot_guard() {
        let cfg = multi_channel_experiment_config(
            16,
            BaselineConfig::RnnBptt {
                hidden_size: DEFAULT_RNN_HIDDEN_SIZE,
                bptt_len: DEFAULT_RNN_BPTT_LEN,
                learning_rate: DEFAULT_RNN_LEARNING_RATE,
            },
        );
        let err = execute_experiment(&cfg).expect_err("k=16 should fail width guard");
        match err {
            ConfigRunError::Rnn(RnnError::UnsupportedWidth { bits, max_bits }) => {
                assert_eq!(bits, 16);
                assert_eq!(max_bits, MAX_RNN_ONE_HOT_WIDTH_BITS);
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn baseline_config_toml_defaults_deserialize() {
        let ctw_cfg: BaselineConfig =
            toml::from_str("kind = \"CrossChannelCtw\"\n").expect("CrossChannelCtw TOML");
        assert_eq!(
            ctw_cfg,
            BaselineConfig::CrossChannelCtw {
                symbol_context_depth: DEFAULT_CROSS_CTW_SYMBOL_DEPTH,
            }
        );

        let rnn_cfg: BaselineConfig = toml::from_str("kind = \"RnnBptt\"\n").expect("RnnBptt TOML");
        assert_eq!(
            rnn_cfg,
            BaselineConfig::RnnBptt {
                hidden_size: DEFAULT_RNN_HIDDEN_SIZE,
                bptt_len: DEFAULT_RNN_BPTT_LEN,
                learning_rate: DEFAULT_RNN_LEARNING_RATE,
            }
        );
    }

    #[test]
    fn arkhe_m1_telemetry_serde_roundtrip_and_legacy_defaults() {
        let legacy_json = r#"{
            "final_node_count": 4,
            "final_split_count": 1,
            "final_delay_count": 2,
            "final_total_energy": 12.5,
            "min_mature_node_energy": 3.0,
            "cumulative_net_flux": 40.0,
            "cumulative_maturity_debt_paid": 8.0,
            "null_calibrated_mdl_delta_bits": -20.0,
            "sprout_events": 1,
            "ring_closure_events": 0,
            "starvation_prune_events": 0,
            "idle_prune_events": 0
        }"#;
        let parsed: ArkheM1Telemetry =
            serde_json::from_str(legacy_json).expect("legacy ArkheM1Telemetry JSON deserializes");
        assert_eq!(parsed.final_node_count, 4);
        assert_eq!(parsed.redundancy_sprout_events, 0);
        assert_eq!(parsed.active_comparator_count, 0);
        assert_eq!(parsed.comparator_agreement_ticks, 0);
        assert_eq!(parsed.comparator_disagreement_ticks, 0);
        assert_eq!(parsed.cumulative_comparator_flux, 0.0);
        assert_eq!(parsed.driver_handoff_events, 0);
        assert_eq!(parsed.disagreement_prune_events, 0);
        assert_eq!(parsed.local_excision_events, 0);
        assert_eq!(parsed.exogenous_injection_bits, 0.0);

        let full = ArkheM1Telemetry {
            redundancy_sprout_events: 2,
            active_comparator_count: 1,
            comparator_agreement_ticks: 120,
            comparator_disagreement_ticks: 3,
            cumulative_comparator_flux: 18.75,
            driver_handoff_events: 1,
            disagreement_prune_events: 1,
            local_excision_events: 3,
            exogenous_injection_bits: 14.5,
            ..parsed
        };
        let encoded = serde_json::to_string(&full).expect("serialize ArkheM1Telemetry");
        let roundtrip: ArkheM1Telemetry =
            serde_json::from_str(&encoded).expect("deserialize ArkheM1Telemetry");
        assert_eq!(roundtrip, full);
    }
}
