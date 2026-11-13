//! Arkhe: Deterministic variable-width modular circuit simulator and yardstick.
//!
//! Implements Milestone 0 (`M0 — Simulator and yardstick`) of the Foundational Research Charter v2:
//! - [`rng`]: Domain-separated portable `ChaCha8Rng` streams and deterministic fingerprinting.
//! - [`substrate`]: Exact $\mathbb{Z}_{2^k}$ arithmetic, Level-0 primitives, §2.5 invariants, and evaluators.
//! - [`structural`]: Coordinate-free graph morphogenesis, metabolic economy, sensitivity credit, and online learner.
//! - [`env`]: Finite-state benchmark tiers (`T1`, `T3b`, `T3c`), noise, exact $\varepsilon$-machines, and Reeds–Sloane synthesis.
//! - [`metrics`]: Surprisal null models, Minimum Description Length ($L$), limit-cycle/basin inspection, and scorecards.
//! - [`baselines`]: Compute-matched prequential baseline harness (`RandomSearch`, `CGP`, `CTW`, `ReedsSloane`, `Exhaustive`).
//! - [`config`]: Serializable TOML/JSON `ExperimentConfig` and `RunReport` execution.
//! - [`verify`]: Pre-registered Milestone 0 acceptance verification battery (Gates G1–G4).

pub mod baselines;
pub mod config;
pub mod env;
pub mod metrics;
pub mod rng;
pub mod structural;
pub mod substrate;
pub mod verify;
pub mod verify_m1;

use std::fs;
use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::config::{ExperimentConfig, compute_ground_truth_yardstick, execute_experiment};
use crate::env::{FiniteStateGenerator, GeneratorConfig, NoiseModel};
use crate::substrate::{Width, WireDelayMode};
use crate::verify::run_m0_verification_suite_with_offset;

/// Command-line interface for the Arkhe Milestone 0 simulator and yardstick.
#[derive(Debug, Parser)]
#[command(
    name = "arkhe",
    version,
    about = "Deterministic variable-width modular circuit simulator and yardstick"
)]
pub struct Cli {
    /// Subcommand to execute (defaults to printing banner when omitted).
    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Supported CLI subcommands.
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Execute a single reproducible experiment from a TOML configuration file.
    Run {
        /// Path to the input `ExperimentConfig` TOML file.
        #[arg(long)]
        config: PathBuf,
        /// Optional path to write the JSON `RunReport` (prints to stdout when omitted).
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Compute and print exact analytical ground-truth yardstick metrics for a generator.
    Yardstick {
        /// Inline TOML or JSON `GeneratorConfig` table (e.g. `kind = "T1_PeriodicMotif", motif = [0, 1, 3]`).
        #[arg(long)]
        generator: String,
        /// Uniform bit-width $k \in 1..=64$.
        #[arg(long, default_value_t = 1)]
        width: u8,
        /// Optional independent symbol substitution noise rate $\varepsilon \in [0, 1)$ across all channels.
        #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
        epsilon: f64,
    },
    /// Run the pre-registered Milestone 0 acceptance test suite (Gates G1–G4) across $k = 1$ and $k = 2$.
    VerifyM0 {
        /// Starting master seed offset (`seed_offset .. seed_offset + seeds`).
        #[arg(long, default_value_t = 0)]
        seed_offset: u32,
        /// Number of master seeds evaluated on stochastic conditions.
        #[arg(long, default_value_t = 20)]
        seeds: u32,
    },
    /// Run the fixed-structure Milestone 1a metabolic-economy gates.
    VerifyM1a {
        /// Starting master seed offset (`seed_offset .. seed_offset + seeds`).
        #[arg(long, default_value_t = 0)]
        seed_offset: u32,
        /// Number of master seeds evaluated on noisy conditions (per bank in bank mode).
        #[arg(long, default_value_t = 20)]
        seeds: u32,
        /// Simulation horizon T in ticks.
        #[arg(long, default_value_t = verify_m1::DEFAULT_HORIZON)]
        horizon: u64,
        /// Number of consecutive seed banks to score (when set, activates bank mode).
        #[arg(long)]
        banks: Option<u32>,
        /// Explicit authorization to run against pre-registered holdout seeds 10000..=10019.
        #[arg(long, alias = "allow-holdout", default_value_t = false)]
        holdout: bool,
    },
    /// Run the online neurogenesis Milestone 1b gates (G1B.1–G1B.4).
    VerifyM1b {
        /// Starting master seed offset (`seed_offset .. seed_offset + seeds`).
        #[arg(long, default_value_t = 0)]
        seed_offset: u32,
        /// Number of master seeds evaluated (per bank when `--banks` is set).
        #[arg(long, default_value_t = 20)]
        seeds: u32,
        /// Simulation horizon T in ticks.
        #[arg(long, default_value_t = verify_m1::DEFAULT_M1B_HORIZON)]
        horizon: u64,
        /// Number of consecutive seed banks to score (when set, activates bank mode).
        #[arg(long)]
        banks: Option<u32>,
        /// Explicit authorization to run against pre-registered holdout seeds 10000..=10019.
        #[arg(long, alias = "allow-holdout", default_value_t = false)]
        holdout: bool,
    },
    /// Run combined Milestone 1 verification (hard M1a pre-growth stage gate -> M1b neurogenesis).
    VerifyM1 {
        /// Starting master seed offset (`seed_offset .. seed_offset + seeds`).
        #[arg(long, default_value_t = 0)]
        seed_offset: u32,
        /// Number of master seeds evaluated (per bank when `--banks` is set).
        #[arg(long, default_value_t = 20)]
        seeds: u32,
        /// Simulation horizon T for M1a in ticks.
        #[arg(long, default_value_t = verify_m1::DEFAULT_HORIZON)]
        m1a_horizon: u64,
        /// Simulation horizon T for M1b in ticks.
        #[arg(long, default_value_t = verify_m1::DEFAULT_M1B_HORIZON)]
        m1b_horizon: u64,
        /// Number of consecutive seed banks to score (when set, activates bank mode).
        #[arg(long)]
        banks: Option<u32>,
        /// Explicit authorization to run against pre-registered holdout seeds 10000..=10019.
        #[arg(long, alias = "allow-holdout", default_value_t = false)]
        holdout: bool,
    },
}

/// Returns the crate banner string.
#[must_use]
pub fn greeting() -> &'static str {
    "arkhe m0"
}

/// Parses CLI arguments and executes the requested command.
///
/// # Errors
///
/// Returns an error if file I/O, configuration parsing, or verification gates fail.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    run_with_args(std::env::args_os())
}

/// Testable entry point accepting an explicit iterator of OS arguments.
///
/// # Errors
///
/// Returns an error if CLI parsing, configuration execution, or verification gates fail.
pub fn run_with_args<I, T>(args: I) -> Result<(), Box<dyn std::error::Error>>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::try_parse_from(args)?;
    match cli.command {
        None => {
            println!("{}", greeting());
            Ok(())
        }
        Some(Commands::Run { config, output }) => {
            let text = fs::read_to_string(&config)?;
            let cfg: ExperimentConfig = toml::from_str(&text)?;
            let report = execute_experiment(&cfg)?;
            let json = serde_json::to_string_pretty(&report)?;
            if let Some(out_path) = output {
                fs::write(out_path, json)?;
            } else {
                println!("{json}");
            }
            Ok(())
        }
        Some(Commands::Yardstick {
            generator,
            width,
            epsilon,
        }) => {
            let w = Width::new(width)?;
            let gen_cfg: GeneratorConfig = if generator.trim_start().starts_with('{') {
                serde_json::from_str(&generator)?
            } else {
                toml::from_str(&generator)?
            };
            let clean_preview = gen_cfg.build(w)?;
            let num_ch = clean_preview.channels();
            let noise = if epsilon == 0.0 {
                NoiseModel::None
            } else {
                NoiseModel::IndependentSubstitution {
                    epsilon,
                    channels: (0..num_ch).collect(),
                }
            };
            let gt = compute_ground_truth_yardstick(
                &gen_cfg,
                w,
                WireDelayMode::Combinational,
                &noise,
                256,
                Some(3),
            )?;
            println!("{}", serde_json::to_string_pretty(&gt)?);
            Ok(())
        }
        Some(Commands::VerifyM0 { seed_offset, seeds }) => {
            let summary = run_m0_verification_suite_with_offset(seed_offset, seeds);
            println!("{}", serde_json::to_string_pretty(&summary)?);
            if summary.all_passed() {
                Ok(())
            } else {
                Err("Milestone 0 verification failed one or more acceptance gates".into())
            }
        }
        Some(Commands::VerifyM1a {
            seed_offset,
            seeds,
            horizon,
            banks,
            holdout,
        }) => {
            if let Some(num_banks) = banks {
                let report = verify_m1::run_m1a_bank_evaluation(
                    seed_offset,
                    seeds,
                    num_banks,
                    horizon,
                    holdout,
                )?;
                println!("{}", serde_json::to_string_pretty(&report)?);
                if report.all_passed() {
                    Ok(())
                } else {
                    Err("Milestone 1a bank evaluation failed one or more gates".into())
                }
            } else {
                let summary = verify_m1::run_m1a_verification_suite_with_horizon_and_holdout(
                    seed_offset,
                    seeds,
                    horizon,
                    holdout,
                )?;
                println!("{}", serde_json::to_string_pretty(&summary)?);
                if summary.all_passed {
                    Ok(())
                } else {
                    Err("Milestone 1a verification failed one or more acceptance gates".into())
                }
            }
        }
        Some(Commands::VerifyM1b {
            seed_offset,
            seeds,
            horizon,
            banks,
            holdout,
        }) => {
            if let Some(num_banks) = banks {
                let report = verify_m1::run_m1b_bank_evaluation(
                    seed_offset,
                    seeds,
                    num_banks,
                    horizon,
                    holdout,
                )?;
                println!("{}", serde_json::to_string_pretty(&report)?);
                if report.all_passed() {
                    Ok(())
                } else {
                    Err("Milestone 1b bank evaluation failed one or more gates".into())
                }
            } else {
                verify_m1::check_seed_range_holdout(seed_offset, seeds, holdout)?;
                let summary = verify_m1::run_m1b_verification_suite(seed_offset, seeds, horizon);
                println!("{}", serde_json::to_string_pretty(&summary)?);
                if summary.all_passed {
                    Ok(())
                } else {
                    Err("Milestone 1b verification failed one or more acceptance gates".into())
                }
            }
        }
        Some(Commands::VerifyM1 {
            seed_offset,
            seeds,
            m1a_horizon,
            m1b_horizon,
            banks,
            holdout,
        }) => {
            if let Some(num_banks) = banks {
                let report = verify_m1::run_m1_combined_bank_evaluation(
                    seed_offset,
                    seeds,
                    num_banks,
                    m1a_horizon,
                    m1b_horizon,
                    holdout,
                )?;
                println!("{}", serde_json::to_string_pretty(&report)?);
                if report.stage_gate_aborted {
                    Err("Milestone 1 stage gate aborted: M1a bank evaluation failed".into())
                } else if report.all_passed {
                    Ok(())
                } else {
                    Err("Milestone 1 combined bank evaluation failed one or more gates".into())
                }
            } else {
                let combined = verify_m1::run_m1_combined_verification(
                    seed_offset,
                    seeds,
                    m1a_horizon,
                    m1b_horizon,
                    holdout,
                )?;
                println!("{}", serde_json::to_string_pretty(&combined)?);
                if combined.stage_gate_aborted {
                    Err("Milestone 1 stage gate aborted: M1a failed one or more gates".into())
                } else if combined.all_passed {
                    Ok(())
                } else {
                    Err("Milestone 1 combined verification failed one or more gates".into())
                }
            }
        }
    }
}
