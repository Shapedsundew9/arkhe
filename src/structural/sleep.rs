//! Wake–sleep scheduler, compute ledger, and live-state sleep execution (Milestone M4a, DR #49).

use serde::{Deserialize, Serialize};

use crate::baselines::{PredictorBaseline, TickBudget};
use crate::env::{FiniteStateGenerator, NoisyEnvironment};
use crate::structural::economy::EconomyConfig;
use crate::structural::growth::{GrowthConfig, observation_horizon};
use crate::structural::learner::ArkheLearner;
use crate::substrate::NodeId;

/// Wake–sleep schedule arm selected on [`GrowthConfig`] (`R7`, `I1`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SleepSchedule {
    /// Default wake-only execution (bit-identical to pre-M4a learner, `I1`).
    #[default]
    None,
    /// `S-A`: consistency-only offline loopback sleep (`R4`, `R5`, `R7`).
    ConsistencyOnly,
    /// `S-B`: consistency plus fact-buffer replay sleep (`R4`, `R5`, `R6`, `R7`).
    ConsistencyReplay,
}

/// Telemetry counters describing scheduled wake–sleep execution and onset/excision events.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SleepTelemetry {
    /// Number of sleep segments executed after passing the `R7` onset rule.
    pub sleep_segments_executed: u64,
    /// Number of `W_wake` boundaries skipped because no mature, grounded, scoreable comparator existed (`R7`, `I2`).
    pub sleep_segment_skipped_no_scoreable_comparator: u64,
    /// Total structural excisions committed during sleep segments (3-route majority or starvation).
    pub sleep_excisions: u64,
    /// Minority faulty routes pruned by 3-route majority consensus during sleep.
    pub sleep_minority_routes_pruned: u64,
    /// Starvation / depleted-node or comparator-branch excisions committed during sleep.
    pub sleep_starvation_excisions: u64,
    /// Comparator-ticks suspended in `S-A` because causal-support leaves touch an ungrounded channel (`R4`, `I7`).
    pub sleep_comparator_suspended_ungrounded: u64,
    /// Wake substrate tick at which the first sleep segment began, if onset occurred.
    pub first_onset_wake_tick: Option<u64>,
    /// Wake environment facts forgone while the environment was paused during sleep (`R2`).
    pub facts_forgone: u64,
}

/// Equal-compute substrate tick ledger and one-sided node-evaluation audit ledger (`R2`, `I5`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ComputeLedger {
    /// Substrate ticks executed during supervised wake (`predict` / `observe`).
    pub wake_substrate_ticks: u64,
    /// Substrate ticks executed during sleep loopback.
    pub sleep_loopback_ticks: u64,
    /// Substrate ticks executed during `S-B` fact-buffer replay.
    pub sleep_replay_ticks: u64,
    /// Audited node evaluations from live circuit steps across wake, loopback, and replay.
    pub live_step_node_evals: u64,
    /// Audited node evaluations from candidate replays and structural verification.
    pub candidate_replay_node_evals: u64,
    /// Audited node evaluations from counterfactual sensitivity probes (wake and sleep).
    pub sensitivity_node_evals: u64,
}

impl ComputeLedger {
    /// Total substrate ticks across wake, sleep loopback, and `S-B` sleep replay (`I5`).
    #[must_use]
    pub const fn total_substrate_ticks(&self) -> u64 {
        self.wake_substrate_ticks
            .saturating_add(self.sleep_loopback_ticks)
            .saturating_add(self.sleep_replay_ticks)
    }

    /// Total audited node evaluations across live steps, candidate replays, and sensitivity probes (`R2`).
    #[must_use]
    pub const fn total_node_evals(&self) -> u64 {
        self.live_step_node_evals
            .saturating_add(self.candidate_replay_node_evals)
            .saturating_add(self.sensitivity_node_evals)
    }
}

/// Summary report returned for a single executed sleep segment.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SleepSegmentReport {
    /// Total substrate ticks executed in this sleep segment (`replay_ticks + loopback_ticks`).
    pub ticks: u64,
    /// Sleep ticks spent in `S-B` replay (`0` for `S-A`).
    pub replay_ticks: u64,
    /// Sleep ticks spent in loopback.
    pub loopback_ticks: u64,
    /// Comparator agreement ticks observed during this segment's loopback.
    pub comparator_agreement_ticks: u64,
    /// Comparator disagreement ticks observed during this segment's loopback.
    pub comparator_disagreement_ticks: u64,
    /// Cumulative net comparator flux (in bits) during this segment.
    pub sleep_comparator_flux: f64,
    /// Maximum circular discrepancy seen on any comparator during this segment.
    pub max_circular_discrepancy: u64,
    /// Number of minority faulty routes pruned by 3-route majority consensus.
    pub minority_routes_pruned: u64,
    /// Number of starvation / depleted-node excisions committed during this segment.
    pub starvation_excisions: u64,
    /// Total structural excisions committed during this segment (`minority_routes_pruned + starvation_excisions`).
    pub sleep_excisions: u64,
    /// Whether any comparator disagreement occurred during this segment.
    pub contradiction_detected: bool,
    /// Live register values at sleep entry (`I3` verification).
    pub entry_registers: Vec<u64>,
    /// Predictions emitted on the first loopback step, if `loopback_ticks > 0` (`I3` verification).
    pub first_loopback_predictions: Option<Vec<u64>>,
}

/// Summary report returned by [`run_wake_sleep_episode`] (`R2`, `R7`, `I5`).
#[derive(Clone, Debug, PartialEq)]
pub struct WakeSleepEpisodeReport {
    /// Total substrate ticks executed in this episode (`== ledger.total_substrate_ticks()` for fresh learners).
    pub total_substrate_ticks: u64,
    /// Number of environment frames stepped (`== wake_observations.len() == wake_substrate_ticks` for fresh learners).
    pub env_steps: u64,
    /// Predictions emitted on each wake tick (`env_steps` entries).
    pub wake_predictions: Vec<Vec<u64>>,
    /// Observed frames stepped from the environment on each wake tick (`env_steps` entries).
    pub wake_observations: Vec<Vec<u64>>,
    /// Clean generator frames stepped from the environment on each wake tick (`env_steps` entries).
    pub wake_clean_frames: Vec<Vec<u64>>,
    /// Snapshot of learner [`SleepTelemetry`] at episode end.
    pub sleep_telemetry: SleepTelemetry,
    /// Snapshot of learner [`ComputeLedger`] at episode end.
    pub compute_ledger: ComputeLedger,
    /// Candidate replay `TickBudget` consumed during wake ticks.
    pub budget_consumed: u64,
    /// Remaining candidate replay `TickBudget` at episode end.
    pub budget_remaining: u64,
    /// Post-warmup observed symbol mismatches across all wake ticks at `wake_tick >= warmup_ticks`.
    pub post_warmup_obs_mismatches: u64,
    /// Post-warmup clean symbol mismatches across all wake ticks at `wake_tick >= warmup_ticks`.
    pub post_warmup_clean_mismatches: u64,
    /// Reports for every sleep segment executed during the episode.
    pub segment_reports: Vec<SleepSegmentReport>,
}

/// Derives `(w_wake, w_sleep)` from `observation_horizon(economy_config, growth_config)` (`R7`).
///
/// `w_wake` is always `W_obs`. `w_sleep` defaults to `W_obs` and honors `growth_config.sleep_window_override`
/// solely for M4a-0 descriptive sensitivity reporting (`R7`).
#[must_use]
pub fn derived_wake_sleep_windows(
    economy_config: &EconomyConfig,
    growth_config: &GrowthConfig,
) -> (u64, u64) {
    let w_obs = observation_horizon(economy_config, growth_config) as u64;
    let w_wake = w_obs;
    let w_sleep = growth_config.sleep_window_override.unwrap_or(w_obs);
    (w_wake, w_sleep)
}

/// Composes an incremental `step` node map onto an accumulated pre-sleep node map `acc`.
pub(crate) fn compose_old_to_new(acc: &mut [Option<NodeId>], step: &[Option<NodeId>]) {
    for slot in acc.iter_mut() {
        *slot = slot.and_then(|mid| step.get(mid.index()).copied().flatten());
    }
}

/// Executes a canonical wake–sleep episode of `total_substrate_ticks` total substrate ticks (`R2`, `R7`, `I5`).
///
/// The environment `env` is stepped ONLY on wake ticks and paused throughout sleep segments.
/// Final partial sleep segments are clamped to `remaining = total_substrate_ticks - elapsed_substrate_ticks`.
/// The same per-wake-tick `TickBudget::new(max_eval_ticks)` policy is applied across all arms (`R2`).
pub fn run_wake_sleep_episode<G: FiniteStateGenerator>(
    learner: &mut ArkheLearner,
    env: &mut NoisyEnvironment<G>,
    total_substrate_ticks: u64,
    warmup_ticks: u64,
    max_eval_ticks: u64,
) -> WakeSleepEpisodeReport {
    let width = env.generator().width();
    let channels = usize::from(env.generator().channels());
    let mut budget = TickBudget::new(max_eval_ticks);
    let mut clean = vec![0u64; channels];
    let mut obs = vec![0u64; channels];

    let (w_wake, w_sleep) =
        derived_wake_sleep_windows(learner.economy_config(), learner.growth_config());
    let schedule = learner.growth_config().sleep_schedule;
    let start_ticks = learner.compute_ledger().total_substrate_ticks();

    let mut env_steps = 0u64;
    let mut wake_in_segment = 0u64;
    let mut post_warmup_obs_mismatches = 0u64;
    let mut post_warmup_clean_mismatches = 0u64;
    let mut wake_predictions = Vec::new();
    let mut wake_observations = Vec::new();
    let mut wake_clean_frames = Vec::new();
    let mut segment_reports = Vec::new();

    while learner
        .compute_ledger()
        .total_substrate_ticks()
        .saturating_sub(start_ticks)
        < total_substrate_ticks
    {
        let pred = learner.predict(&mut budget);
        env.step_frame(&mut clean, &mut obs);
        let _ = learner.observe(&obs, &mut budget);

        for ch in 0..channels {
            let p = width.normalize(pred[ch]);
            if env_steps >= warmup_ticks {
                if p != obs[ch] {
                    post_warmup_obs_mismatches += 1;
                }
                if p != clean[ch] {
                    post_warmup_clean_mismatches += 1;
                }
            }
        }
        wake_predictions.push(pred);
        wake_observations.push(obs.clone());
        wake_clean_frames.push(clean.clone());
        env_steps += 1;
        wake_in_segment += 1;

        let elapsed = learner
            .compute_ledger()
            .total_substrate_ticks()
            .saturating_sub(start_ticks);
        let remaining = total_substrate_ticks.saturating_sub(elapsed);
        if remaining == 0 {
            break;
        }

        if schedule != SleepSchedule::None && wake_in_segment >= w_wake {
            wake_in_segment = 0;
            if learner.has_scoreable_comparator_for_sleep() {
                let seg_ticks = w_sleep.min(remaining);
                let seg_report = learner.run_sleep_segment(seg_ticks);
                segment_reports.push(seg_report);
            } else {
                learner.record_skipped_sleep_segment();
            }
        }
    }

    let elapsed = learner
        .compute_ledger()
        .total_substrate_ticks()
        .saturating_sub(start_ticks);

    WakeSleepEpisodeReport {
        total_substrate_ticks: elapsed,
        env_steps,
        wake_predictions,
        wake_observations,
        wake_clean_frames,
        sleep_telemetry: learner.sleep_telemetry(),
        compute_ledger: learner.compute_ledger(),
        budget_consumed: budget.consumed(),
        budget_remaining: budget.remaining(),
        post_warmup_obs_mismatches,
        post_warmup_clean_mismatches,
        segment_reports,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_windows_follow_observation_horizon_and_override() {
        let econ = EconomyConfig::default();
        let growth = GrowthConfig::default();
        assert_eq!(derived_wake_sleep_windows(&econ, &growth), (256, 256));

        let override_growth = GrowthConfig {
            sleep_window_override: Some(64),
            ..growth
        };
        assert_eq!(
            derived_wake_sleep_windows(&econ, &override_growth),
            (256, 64)
        );
    }

    #[test]
    fn compose_old_to_new_chains_multiple_excisions() {
        let mut acc = vec![
            Some(NodeId(0)),
            Some(NodeId(1)),
            Some(NodeId(2)),
            Some(NodeId(3)),
        ];
        // First edit drops node 1 -> surviving 0, 2, 3 map to 0, 1, 2
        compose_old_to_new(
            &mut acc,
            &[Some(NodeId(0)), None, Some(NodeId(1)), Some(NodeId(2))],
        );
        // Second edit drops new node 1 (originally 2) -> surviving 0, 2 map to 0, 1
        compose_old_to_new(&mut acc, &[Some(NodeId(0)), None, Some(NodeId(1))]);
        assert_eq!(acc, vec![Some(NodeId(0)), None, None, Some(NodeId(1))]);
    }

    #[test]
    fn compute_ledger_totals_are_exact_sums() {
        let ledger = ComputeLedger {
            wake_substrate_ticks: 256,
            sleep_loopback_ticks: 128,
            sleep_replay_ticks: 128,
            live_step_node_evals: 1_000,
            candidate_replay_node_evals: 2_500,
            sensitivity_node_evals: 3_750,
        };
        assert_eq!(ledger.total_substrate_ticks(), 512);
        assert_eq!(ledger.total_node_evals(), 7_250);
    }
}
