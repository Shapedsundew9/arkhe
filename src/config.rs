//! Serializable experiment configurations, ground-truth manifests, provenance, and reproducible run execution.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::baselines::{
    CgpBaseline, CtwBaseline, DEFAULT_MAX_CERTIFY_JOINT_STEPS, ExhaustiveBaseline,
    PredictorBaseline, RandomSearchBaseline, ReedsSloaneBaseline, SynthesisCertStatus,
    exact_synthesize_certified_circuit, run_prequential_harness,
};
use crate::env::{
    EpsilonMachineScope, EpsilonMachineSummary, FiniteStateGenerator, FiniteStateSpec,
    GeneratorConfig, GeneratorError, NoiseError, NoiseModel, NoisyEnvironment,
    ReedsSloaneSynthesizer, declared_noise_entropy_bits_per_symbol, solve_epsilon_machine,
};
use crate::metrics::Scorecard;
use crate::rng::{SeedBank, fnv1a_128_hex};
use crate::structural::{
    ArkheLearner, EconomyConfig, GridShape, GrowthConfig, GrowthError, LatticeError, WiringTopology,
};
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
    /// Milestone 0 baseline runners only implement `WiringTopology::WellMixed` candidate search.
    UnsupportedExperimentTopology(WiringTopology),
    /// Generator construction failed.
    Generator(GeneratorError),
    /// Noise configuration failed validation.
    Noise(NoiseError),
    /// Lattice operation or coordinate mapping failed.
    Lattice(LatticeError),
    /// Growth or diagnostics operation failed.
    Growth(GrowthError),
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
            Self::UnsupportedExperimentTopology(topo) => write!(
                f,
                "unsupported wiring_topology {topo:?} in M0 baseline runner (only WellMixed is implemented in M0)"
            ),
            Self::Generator(e) => write!(f, "generator error: {e}"),
            Self::Noise(e) => write!(f, "noise error: {e}"),
            Self::Lattice(e) => write!(f, "lattice error: {e:?}"),
            Self::Growth(e) => write!(f, "growth error: {e:?}"),
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

impl From<LatticeError> for ConfigRunError {
    fn from(value: LatticeError) -> Self {
        Self::Lattice(value)
    }
}

impl From<GrowthError> for ConfigRunError {
    fn from(value: GrowthError) -> Self {
        Self::Growth(value)
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
    /// Spatial wiring locality constraint.
    #[serde(default)]
    pub wiring_topology: WiringTopology,
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
    /// Online prequential Reeds–Sloane / Berlekamp–Massey linear recurrence synthesizer.
    ReedsSloane,
    /// Canonical size-ordered small-circuit exhaustive enumerator.
    Exhaustive {
        #[serde(default = "default_exhaustive_nodes")]
        max_nodes: usize,
        #[serde(default = "default_fact_window")]
        fact_window: usize,
    },
    /// Online gradient-free structural learning from an empty spatial lattice.
    ArkheM1 {
        #[serde(default = "default_arkhe_grid_dim")]
        grid_rows: u16,
        #[serde(default = "default_arkhe_grid_dim")]
        grid_cols: u16,
        #[serde(default = "default_max_relay_hops")]
        max_relay_hops: u8,
        #[serde(default)]
        economy: EconomyConfig,
        #[serde(default)]
        growth: GrowthConfig,
    },
}

const fn default_arkhe_grid_dim() -> u16 {
    8
}
const fn default_max_relay_hops() -> u8 {
    4
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
    pub grid_rows: u16,
    pub grid_cols: u16,
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
    #[serde(default)]
    pub lag_horizon_exhausted: u64,
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
    if config.substrate.wiring_topology != WiringTopology::WellMixed
        && !matches!(config.baseline, BaselineConfig::ArkheM1 { .. })
    {
        return Err(ConfigRunError::UnsupportedExperimentTopology(
            config.substrate.wiring_topology,
        ));
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
        BaselineConfig::ArkheM1 {
            grid_rows,
            grid_cols,
            max_relay_hops,
            economy,
            growth,
        } => Box::new(ArkheLearner::new_with_spatial(
            width,
            channels,
            wire_delay,
            config.substrate.wiring_topology,
            GridShape::new(*grid_rows, *grid_cols)?,
            *max_relay_hops,
            *economy,
            *growth,
            seeds,
        )?),
    };

    let scorecard = run_prequential_harness(
        &mut noisy_env,
        predictor.as_mut(),
        config.substrate.wiring_topology,
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
