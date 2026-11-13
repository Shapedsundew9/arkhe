//! Compute and counter scaling profiler across circuit scale $(N, K)$ (Readiness B5 / Phase 0).

use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::baselines::run_prequential_harness;
use crate::config::{ArkheM1Telemetry, BuildProvenance, CliffCountersSummary};
use crate::env::{
    DEFAULT_MAX_EXPORT_STATES, EpsilonMachineScope, FiniteStateGenerator, FiniteStateSpec,
    GeneratorConfig, NoiseModel, NoisyEnvironment, SpecExportError,
};
use crate::metrics::Scorecard;
use crate::rng::SeedBank;
use crate::structural::surgery::explicit_delay_register_index;
use crate::structural::{ArkheLearner, EconomyConfig, GrowthConfig, build_autonomous_ring};
use crate::substrate::{Circuit, Primitive, SignalSink, SignalSource, Width, WireDelayMode};
use crate::verify_m1::check_seed_range_holdout;

/// Default simulation horizon (ticks) per cell in the controlled $(N, K)$ scaling sweep.
pub const DEFAULT_PROFILE_HORIZON: u64 = 512;

/// Default simulation horizon (ticks) for endogenous online neurogenesis workloads.
pub const DEFAULT_ENDOGENOUS_HORIZON: u64 = 1_024;

/// Warmup exclusion window used when summarizing endogenous post-transient mismatches.
const ENDOGENOUS_WARMUP_TICKS: u64 = 500;

/// Per-run internal candidate evaluation tick budget ceiling for B5 profiling runs.
const PROFILE_MAX_EVAL_TICKS: u64 = 500_000;

/// Controlled circuit node scales $N$ swept during B5 Phase 0 profiling.
pub const PROFILE_NODE_SCALES: [usize; 7] = [0, 2, 4, 8, 16, 24, 32];

/// Channel counts $K$ swept during B5 Phase 0 profiling.
pub const PROFILE_CHANNEL_SCALES: [u16; 4] = [1, 2, 4, 8];

/// Substitution noise rate used in the controlled `diagnostic_noise` regime to exercise all-$K$ scans.
const PROFILE_DIAGNOSTIC_EPSILON: f64 = 0.15;

/// Substitution noise rate used for the noisy online endogenous benchmark.
const ENDOGENOUS_NOISE_EPSILON: f64 = 0.05;

/// Deterministic multi-channel periodic stream generator used for controlled & endogenous B5 runs.
#[derive(Clone, Debug)]
struct MultiChannelPeriodicGenerator {
    label: String,
    width: Width,
    motifs: Vec<Vec<u64>>,
    tick: u64,
}

impl MultiChannelPeriodicGenerator {
    fn new(label: impl Into<String>, width: Width, motifs: Vec<Vec<u64>>) -> Self {
        assert!(!motifs.is_empty(), "requires at least one channel motif");
        assert!(
            motifs.iter().all(|m| !m.is_empty()),
            "every channel motif must be non-empty"
        );
        Self {
            label: label.into(),
            width,
            motifs,
            tick: 0,
        }
    }
}

impl FiniteStateGenerator for MultiChannelPeriodicGenerator {
    fn name(&self) -> &str {
        &self.label
    }

    fn width(&self) -> Width {
        self.width
    }

    fn channels(&self) -> u16 {
        self.motifs.len() as u16
    }

    fn reset(&mut self) {
        self.tick = 0;
    }

    fn next_clean_frame(&mut self, out: &mut [u64]) {
        for (slot, motif) in out.iter_mut().zip(&self.motifs) {
            *slot = self
                .width
                .normalize(motif[(self.tick as usize) % motif.len()]);
        }
        self.tick += 1;
    }

    fn to_finite_state_spec(&self) -> Result<FiniteStateSpec, SpecExportError> {
        let mut period = 1usize;
        for m in &self.motifs {
            let g = gcd_usize(period, m.len());
            period =
                period
                    .checked_mul(m.len() / g)
                    .ok_or(SpecExportError::StateSpaceTooLarge {
                        estimated_states: u128::MAX,
                        limit: DEFAULT_MAX_EXPORT_STATES,
                    })?;
            if period > DEFAULT_MAX_EXPORT_STATES {
                return Err(SpecExportError::StateSpaceTooLarge {
                    estimated_states: period as u128,
                    limit: DEFAULT_MAX_EXPORT_STATES,
                });
            }
        }
        let mut transitions = Vec::with_capacity(period);
        for s in 0..period {
            let next_state = (s + 1) % period;
            let frame: Vec<u64> = self
                .motifs
                .iter()
                .map(|m| self.width.normalize(m[s % m.len()]))
                .collect();
            transitions.push((next_state, frame));
        }
        Ok(FiniteStateSpec {
            width: self.width,
            channels: self.channels(),
            num_states: period,
            initial_state: 0,
            transitions,
            scope: EpsilonMachineScope::StationaryProcess,
        })
    }
}

const fn gcd_usize(mut a: usize, mut b: usize) -> usize {
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a
}

/// Operating regime in the controlled $(N, K)$ sweep.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileRegime {
    /// Zero observation noise; planted circuit matches clean signal with zero structural thrash.
    SteadyClean,
    /// Substitution noise ($\varepsilon = 0.15$) on all $K$ channels, triggering diagnostic & search scans.
    DiagnosticNoise,
}

impl ProfileRegime {
    const fn as_str(self) -> &'static str {
        match self {
            Self::SteadyClean => "steady_clean",
            Self::DiagnosticNoise => "diagnostic_noise",
        }
    }
}

/// Aggregated compute and counter statistics for one $(regime, N, K)$ cell.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ControlledScaleRow {
    pub regime: ProfileRegime,
    pub target_nodes: usize,
    pub channels: u16,
    pub runs: u32,
    pub total_ticks: u64,
    pub wall_ms_total: f64,
    pub us_per_tick_mean: f64,
    pub us_per_tick_max: f64,
    pub eval_ticks_per_tick_mean: f64,
    pub eval_ticks_per_tick_max: f64,
    pub final_nodes_min: usize,
    pub final_nodes_max: usize,
    pub sprout_events: u64,
    pub ring_closure_events: u64,
    pub starvation_prune_events: u64,
    pub idle_prune_events: u64,
    pub cliff_counters: CliffCountersSummary,
}

/// Aggregated compute and counter statistics for one endogenous online workload.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EndogenousScaleRow {
    pub task: String,
    pub width_bits: u8,
    pub channels: u16,
    pub epsilon: f64,
    pub runs: u32,
    pub total_ticks: u64,
    pub wall_ms_total: f64,
    pub us_per_tick_mean: f64,
    pub us_per_tick_max: f64,
    pub eval_ticks_per_tick_mean: f64,
    pub eval_ticks_per_tick_max: f64,
    pub final_nodes_min: usize,
    pub final_nodes_max: usize,
    pub clean_mismatches_post500_total: u64,
    pub sprout_events: u64,
    pub ring_closure_events: u64,
    pub starvation_prune_events: u64,
    pub idle_prune_events: u64,
    pub cliff_counters: CliffCountersSummary,
}

/// Complete B5 compute and counter scaling report across controlled $(N, K)$ and endogenous runs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ComputeProfileReport {
    pub provenance: BuildProvenance,
    pub seed_offset: u32,
    pub seeds_tested: u32,
    pub horizon: u64,
    pub endogenous_horizon: u64,
    pub max_eval_ticks_budget: u64,
    pub growth_config: GrowthConfig,
    pub economy_config: EconomyConfig,
    pub controlled_sweep: Vec<ControlledScaleRow>,
    pub endogenous_runs: Vec<EndogenousScaleRow>,
    pub controlled_cliff_totals: CliffCountersSummary,
    pub endogenous_cliff_totals: CliffCountersSummary,
}

struct RunAccumulator {
    wall_ms_total: f64,
    us_per_tick_max: f64,
    eval_ticks_total: u64,
    eval_ticks_per_tick_max: f64,
    final_nodes_min: usize,
    final_nodes_max: usize,
    clean_mismatches_total: u64,
    sprout_events: u64,
    ring_closure_events: u64,
    starvation_prune_events: u64,
    idle_prune_events: u64,
    cliff: CliffCountersSummary,
}

impl Default for RunAccumulator {
    fn default() -> Self {
        Self {
            wall_ms_total: 0.0,
            us_per_tick_max: 0.0,
            eval_ticks_total: 0,
            eval_ticks_per_tick_max: 0.0,
            final_nodes_min: usize::MAX,
            final_nodes_max: 0,
            clean_mismatches_total: 0,
            sprout_events: 0,
            ring_closure_events: 0,
            starvation_prune_events: 0,
            idle_prune_events: 0,
            cliff: CliffCountersSummary::default(),
        }
    }
}

impl RunAccumulator {
    fn record(
        &mut self,
        elapsed_secs: f64,
        horizon: u64,
        scorecard: &Scorecard,
        tel: ArkheM1Telemetry,
    ) {
        let wall_ms = elapsed_secs * 1_000.0;
        let us_per_tick = (elapsed_secs * 1_000_000.0) / (horizon as f64);
        let eval_per_tick = (scorecard.candidate_replay_ticks_consumed as f64) / (horizon as f64);

        self.wall_ms_total += wall_ms;
        self.us_per_tick_max = self.us_per_tick_max.max(us_per_tick);
        self.eval_ticks_total += scorecard.candidate_replay_ticks_consumed;
        self.eval_ticks_per_tick_max = self.eval_ticks_per_tick_max.max(eval_per_tick);
        self.clean_mismatches_total += scorecard.post_transient_clean_mismatches;
        self.final_nodes_min = self.final_nodes_min.min(tel.final_node_count);
        self.final_nodes_max = self.final_nodes_max.max(tel.final_node_count);
        self.sprout_events += tel.sprout_events;
        self.ring_closure_events += tel.ring_closure_events;
        self.starvation_prune_events += tel.starvation_prune_events;
        self.idle_prune_events += tel.idle_prune_events;
        self.cliff.accumulate(&CliffCountersSummary::from(&tel));
    }

    fn us_per_tick_mean(&self, total_ticks: u64) -> f64 {
        (self.wall_ms_total * 1_000.0) / (total_ticks as f64)
    }

    fn eval_ticks_per_tick_mean(&self, total_ticks: u64) -> f64 {
        (self.eval_ticks_total as f64) / (total_ticks as f64)
    }
}

fn alternating_ring_motif(period: usize) -> Vec<u64> {
    (0..period)
        .map(|j| u64::from((j % 2 == 1) || (j == 0 && period % 2 == 1)))
        .collect()
}

/// Builds a learner whose circuit starts with exact `target_nodes` active nodes on port 0.
fn build_scaled_learner(
    channels: u16,
    target_nodes: usize,
    economy: EconomyConfig,
    growth: GrowthConfig,
    seeds: SeedBank,
) -> Result<(ArkheLearner, Vec<Vec<u64>>), String> {
    let mut learner = ArkheLearner::new_with_config(
        Width::BITS_1,
        channels,
        WireDelayMode::Combinational,
        economy,
        growth,
        seeds,
    )
    .map_err(|e| format!("ArkheLearner init failed: {e:?}"))?;

    let mut motifs = vec![vec![0u64]; usize::from(channels)];
    if target_nodes == 0 {
        return Ok((learner, motifs));
    }

    let mut circuit = Circuit::new(
        Width::BITS_1,
        channels,
        channels,
        WireDelayMode::Combinational,
    );
    let mut reg_values: Vec<(crate::substrate::NodeId, u64)> = Vec::new();

    if target_nodes == 2 {
        // Minimal 2-node autonomous self-loop: Delay(init=1) -> Split -> (Delay, BoundaryOut(0)).
        let d = circuit.add_node(Primitive::Delay { init: 1 });
        let s = circuit.add_node(Primitive::Split);
        circuit.connect(
            SignalSource::NodeOut { node: d, port: 0 },
            SignalSink::NodeIn { node: s, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: s, port: 0 },
            SignalSink::NodeIn { node: d, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: s, port: 1 },
            SignalSink::BoundaryOut(0),
        );
        reg_values.push((d, 1));
        motifs[0] = vec![1u64];
    } else {
        let period = target_nodes - 1;
        let mu = alternating_ring_motif(period);
        let (delays, _split) = build_autonomous_ring(&mut circuit, period, &mu, 0);
        for (j, &delay_node) in delays.iter().enumerate() {
            reg_values.push((delay_node, mu[(period - 1 - j) % period]));
        }
        motifs[0] = mu;
    }

    debug_assert_eq!(circuit.node_count(), target_nodes);

    learner
        .editable_mut()
        .recompile_preserving_state(circuit, &[])
        .map_err(|e| format!("recompile_preserving_state failed: {e:?}"))?;

    let mut regs = learner.editable().state().registers().to_vec();
    for (node, val) in reg_values {
        let reg_idx = explicit_delay_register_index(learner.editable().circuit(), node)
            .ok_or_else(|| format!("missing delay register for node {}", node.index()))?;
        regs[reg_idx] = val;
    }
    learner
        .editable_mut()
        .set_registers(&regs)
        .map_err(|e| format!("set_registers failed: {e:?}"))?;
    let committed_circuit = learner.editable().circuit().clone();
    learner
        .ledger_mut()
        .ensure_capacity(&committed_circuit, economy.split_code_tax);
    learner.ledger_mut().debt_paid.fill(true);
    for node in learner.editable_mut().economy_mut().nodes_mut() {
        node.juvenile_ticks = 0;
        node.debt_paid = true;
    }

    Ok((learner, motifs))
}

fn time_learner_harness<G: FiniteStateGenerator>(
    generator: G,
    noise_model: NoiseModel,
    seed_bank: SeedBank,
    learner: &mut ArkheLearner,
    horizon: u64,
    warmup: u64,
) -> Result<(f64, Scorecard, ArkheM1Telemetry), String> {
    let mut env = NoisyEnvironment::new(generator, noise_model, seed_bank)
        .map_err(|e| format!("NoisyEnvironment failed: {e}"))?;
    let started = Instant::now();
    let scorecard = run_prequential_harness(
        &mut env,
        learner,
        horizon,
        warmup,
        PROFILE_MAX_EVAL_TICKS,
        None,
        None,
    );
    let elapsed_secs = started.elapsed().as_secs_f64();
    Ok((elapsed_secs, scorecard, learner.telemetry()))
}

#[derive(Clone, Copy)]
struct ProfileRunParams {
    seed_offset: u32,
    seeds: u32,
    horizon: u64,
    economy: EconomyConfig,
    growth: GrowthConfig,
}

fn run_controlled_cell(
    regime: ProfileRegime,
    target_nodes: usize,
    channels: u16,
    params: ProfileRunParams,
) -> Result<ControlledScaleRow, String> {
    let mut acc = RunAccumulator::default();
    let noise_model = match regime {
        ProfileRegime::SteadyClean => NoiseModel::None,
        ProfileRegime::DiagnosticNoise => NoiseModel::IndependentSubstitution {
            epsilon: PROFILE_DIAGNOSTIC_EPSILON,
            channels: (0..channels).collect(),
        },
    };

    for seed_idx in 0..params.seeds {
        let master_seed = u64::from(params.seed_offset + seed_idx);
        let seed_bank = SeedBank::new(master_seed);
        let (mut learner, motifs) = build_scaled_learner(
            channels,
            target_nodes,
            params.economy,
            params.growth,
            seed_bank,
        )?;
        let generator = MultiChannelPeriodicGenerator::new(
            format!("{}:N={target_nodes}:K={channels}", regime.as_str()),
            Width::BITS_1,
            motifs,
        );
        let (elapsed_secs, scorecard, tel) = time_learner_harness(
            generator,
            noise_model.clone(),
            seed_bank,
            &mut learner,
            params.horizon,
            0,
        )?;
        acc.record(elapsed_secs, params.horizon, &scorecard, tel);
    }

    let total_ticks = u64::from(params.seeds) * params.horizon;
    Ok(ControlledScaleRow {
        regime,
        target_nodes,
        channels,
        runs: params.seeds,
        total_ticks,
        wall_ms_total: acc.wall_ms_total,
        us_per_tick_mean: acc.us_per_tick_mean(total_ticks),
        us_per_tick_max: acc.us_per_tick_max,
        eval_ticks_per_tick_mean: acc.eval_ticks_per_tick_mean(total_ticks),
        eval_ticks_per_tick_max: acc.eval_ticks_per_tick_max,
        final_nodes_min: acc.final_nodes_min,
        final_nodes_max: acc.final_nodes_max,
        sprout_events: acc.sprout_events,
        ring_closure_events: acc.ring_closure_events,
        starvation_prune_events: acc.starvation_prune_events,
        idle_prune_events: acc.idle_prune_events,
        cliff_counters: acc.cliff,
    })
}

struct EndogenousSpec {
    label: &'static str,
    width: Width,
    channels: u16,
    motifs: Option<Vec<Vec<u64>>>,
    single_channel_cfg: Option<GeneratorConfig>,
    epsilon: f64,
}

fn endogenous_workloads() -> Vec<EndogenousSpec> {
    vec![
        EndogenousSpec {
            label: "K=1:Motif011(k=1,clean)",
            width: Width::BITS_1,
            channels: 1,
            motifs: None,
            single_channel_cfg: Some(GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 1, 1],
            }),
            epsilon: 0.0,
        },
        EndogenousSpec {
            label: "K=1:Motif013(k=2,clean)",
            width: Width::BITS_2,
            channels: 1,
            motifs: None,
            single_channel_cfg: Some(GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 1, 3],
            }),
            epsilon: 0.0,
        },
        EndogenousSpec {
            label: "K=1:Motif013(k=2,eps=0.05)",
            width: Width::BITS_2,
            channels: 1,
            motifs: None,
            single_channel_cfg: Some(GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 1, 3],
            }),
            epsilon: ENDOGENOUS_NOISE_EPSILON,
        },
        EndogenousSpec {
            label: "K=2:(Alt,Motif011)(k=1,clean)",
            width: Width::BITS_1,
            channels: 2,
            motifs: Some(vec![vec![0, 1], vec![0, 1, 1]]),
            single_channel_cfg: None,
            epsilon: 0.0,
        },
        EndogenousSpec {
            label: "K=4:(4 motifs)(k=1,clean)",
            width: Width::BITS_1,
            channels: 4,
            motifs: Some(vec![
                vec![0, 1],
                vec![0, 1, 1],
                vec![0, 0, 1],
                vec![1, 0, 0, 1],
            ]),
            single_channel_cfg: None,
            epsilon: 0.0,
        },
        EndogenousSpec {
            label: "K=8:(8 motifs)(k=1,clean)",
            width: Width::BITS_1,
            channels: 8,
            motifs: Some(vec![
                vec![0, 1],
                vec![0, 1, 1],
                vec![0, 0, 1],
                vec![1, 0, 0, 1],
                vec![0, 1, 0, 1, 1],
                vec![1, 1, 0],
                vec![1, 0, 1],
                vec![0, 0, 0, 1],
            ]),
            single_channel_cfg: None,
            epsilon: 0.0,
        },
    ]
}

fn run_endogenous_spec(
    spec: &EndogenousSpec,
    params: ProfileRunParams,
) -> Result<EndogenousScaleRow, String> {
    let mut acc = RunAccumulator::default();
    let noise = if spec.epsilon > 0.0 {
        NoiseModel::IndependentSubstitution {
            epsilon: spec.epsilon,
            channels: (0..spec.channels).collect(),
        }
    } else {
        NoiseModel::None
    };

    let warmup = ENDOGENOUS_WARMUP_TICKS.min(params.horizon.saturating_sub(1));

    for seed_idx in 0..params.seeds {
        let master_seed = u64::from(params.seed_offset + seed_idx);
        let seed_bank = SeedBank::new(master_seed);
        let mut learner = ArkheLearner::new_with_config(
            spec.width,
            spec.channels,
            WireDelayMode::Combinational,
            params.economy,
            params.growth,
            seed_bank,
        )
        .map_err(|e| format!("ArkheLearner init failed: {e:?}"))?;

        let (elapsed_secs, scorecard, tel) = if let Some(cfg) = &spec.single_channel_cfg {
            let generator = cfg
                .build(spec.width)
                .map_err(|e| format!("Generator build failed: {e}"))?;
            time_learner_harness(
                generator,
                noise.clone(),
                seed_bank,
                &mut learner,
                params.horizon,
                warmup,
            )?
        } else {
            let motifs = spec.motifs.clone().expect("multi-channel motifs");
            let generator = MultiChannelPeriodicGenerator::new(spec.label, spec.width, motifs);
            time_learner_harness(
                generator,
                noise.clone(),
                seed_bank,
                &mut learner,
                params.horizon,
                warmup,
            )?
        };
        acc.record(elapsed_secs, params.horizon, &scorecard, tel);
    }

    let total_ticks = u64::from(params.seeds) * params.horizon;
    Ok(EndogenousScaleRow {
        task: spec.label.to_owned(),
        width_bits: spec.width.bits(),
        channels: spec.channels,
        epsilon: spec.epsilon,
        runs: params.seeds,
        total_ticks,
        wall_ms_total: acc.wall_ms_total,
        us_per_tick_mean: acc.us_per_tick_mean(total_ticks),
        us_per_tick_max: acc.us_per_tick_max,
        eval_ticks_per_tick_mean: acc.eval_ticks_per_tick_mean(total_ticks),
        eval_ticks_per_tick_max: acc.eval_ticks_per_tick_max,
        final_nodes_min: acc.final_nodes_min,
        final_nodes_max: acc.final_nodes_max,
        clean_mismatches_post500_total: acc.clean_mismatches_total,
        sprout_events: acc.sprout_events,
        ring_closure_events: acc.ring_closure_events,
        starvation_prune_events: acc.starvation_prune_events,
        idle_prune_events: acc.idle_prune_events,
        cliff_counters: acc.cliff,
    })
}

/// Executes the B5 compute and counter scaling profile across controlled $(N, K)$ and endogenous runs.
///
/// # Errors
///
/// Returns an error if the seed range overlaps `10000..=10019` without `allow_holdout` or if setup fails.
pub fn run_compute_profile(
    seed_offset: u32,
    seeds: u32,
    horizon: u64,
    endogenous_horizon: u64,
    allow_holdout: bool,
) -> Result<ComputeProfileReport, String> {
    let seeds = seeds.max(1);
    let horizon = horizon.max(64);
    let endogenous_horizon = endogenous_horizon.max(64);
    check_seed_range_holdout(seed_offset, seeds, allow_holdout)?;

    let economy = EconomyConfig::default();
    let growth = GrowthConfig::default();

    let mut controlled_sweep =
        Vec::with_capacity(2 * PROFILE_NODE_SCALES.len() * PROFILE_CHANNEL_SCALES.len());
    let mut controlled_cliff_totals = CliffCountersSummary::default();

    let controlled_params = ProfileRunParams {
        seed_offset,
        seeds,
        horizon,
        economy,
        growth,
    };
    for regime in [ProfileRegime::SteadyClean, ProfileRegime::DiagnosticNoise] {
        for &target_nodes in &PROFILE_NODE_SCALES {
            for &channels in &PROFILE_CHANNEL_SCALES {
                let row = run_controlled_cell(regime, target_nodes, channels, controlled_params)?;
                controlled_cliff_totals.accumulate(&row.cliff_counters);
                controlled_sweep.push(row);
            }
        }
    }

    let specs = endogenous_workloads();
    let mut endogenous_runs = Vec::with_capacity(specs.len());
    let mut endogenous_cliff_totals = CliffCountersSummary::default();
    let endogenous_params = ProfileRunParams {
        seed_offset,
        seeds,
        horizon: endogenous_horizon,
        economy,
        growth,
    };
    for spec in &specs {
        let row = run_endogenous_spec(spec, endogenous_params)?;
        endogenous_cliff_totals.accumulate(&row.cliff_counters);
        endogenous_runs.push(row);
    }

    Ok(ComputeProfileReport {
        provenance: BuildProvenance::current(),
        seed_offset,
        seeds_tested: seeds,
        horizon,
        endogenous_horizon,
        max_eval_ticks_budget: PROFILE_MAX_EVAL_TICKS,
        growth_config: growth,
        economy_config: economy,
        controlled_sweep,
        endogenous_runs,
        controlled_cliff_totals,
        endogenous_cliff_totals,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controlled_scale_learner_maintains_exact_node_counts_without_starvation() {
        let params = ProfileRunParams {
            seed_offset: 0,
            seeds: 1,
            horizon: 256,
            economy: EconomyConfig::default(),
            growth: GrowthConfig::default(),
        };
        for &target_nodes in &PROFILE_NODE_SCALES {
            for &channels in &[1u16, 4u16] {
                let row =
                    run_controlled_cell(ProfileRegime::SteadyClean, target_nodes, channels, params)
                        .unwrap();
                assert_eq!(
                    row.final_nodes_min, target_nodes,
                    "node count mismatch for N={target_nodes}, K={channels}: {row:?}"
                );
                assert_eq!(
                    row.final_nodes_max, target_nodes,
                    "node count mismatch for N={target_nodes}, K={channels}: {row:?}"
                );
                assert_eq!(row.starvation_prune_events, 0);
                assert_eq!(row.sprout_events, 0);
            }
        }
    }

    #[test]
    fn compute_profile_enforces_holdout_guard_and_covers_full_grid() {
        assert!(run_compute_profile(10_000, 1, 64, 64, false).is_err());
        let report = run_compute_profile(0, 1, 64, 64, false).unwrap();
        assert_eq!(
            report.controlled_sweep.len(),
            2 * PROFILE_NODE_SCALES.len() * PROFILE_CHANNEL_SCALES.len()
        );
        assert_eq!(report.endogenous_runs.len(), 6);
    }
}
