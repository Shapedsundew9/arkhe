//! Wake–sleep scheduler, bounded fact buffer, two-sample KT match test, compute ledger,
//! and epistemic conservation audit (Milestone M4a, DR #49).

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::baselines::{PredictorBaseline, TickBudget};
use crate::env::{FiniteStateGenerator, NoisyEnvironment};
use crate::metrics::{PointForecastScorer, circuit_code_bits_with_tax};
use crate::structural::economy::{
    EconomyConfig, RewardMode, channel_independence_weight, score_with_reward_mode,
};
use crate::structural::growth::{GrowthConfig, observation_horizon};
use crate::structural::learner::ArkheLearner;
use crate::substrate::{NodeId, Width};

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

/// Bounded ring storing the trailing `W_obs / 2` wake frames paired with pre-step `(epoch, registers)`
/// snapshots and `prior_frame` (`R6`, matching [`crate::baselines::ObservationWindow::push_frame_with_state`] semantics).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FactBuffer {
    capacity: usize,
    start_tick: u64,
    evictions: u64,
    prior_frame: Vec<u64>,
    frames: VecDeque<Vec<u64>>,
    state_snapshots: VecDeque<(u64, Vec<u64>)>,
}

impl FactBuffer {
    /// Creates an empty [`FactBuffer`] of exact capacity `capacity` (`W_obs / 2`).
    #[must_use]
    pub fn new(channels: u16, capacity: usize) -> Self {
        Self {
            capacity,
            start_tick: 0,
            evictions: 0,
            prior_frame: vec![0u64; usize::from(channels)],
            frames: VecDeque::with_capacity(capacity),
            state_snapshots: VecDeque::with_capacity(capacity),
        }
    }

    /// Appends a newly observed wake frame paired with the pre-step `(structure_epoch, registers)`
    /// snapshot, evicting the oldest entry into `prior_frame` when at capacity.
    ///
    /// Returns `true` iff an eviction occurred.
    pub fn push_frame_with_state(
        &mut self,
        frame: &[u64],
        structure_epoch: u64,
        pre_step_registers: &[u64],
    ) -> bool {
        if self.capacity == 0 {
            return false;
        }
        let mut evicted = false;
        if self.frames.len() >= self.capacity {
            if let Some(old_frame) = self.frames.pop_front() {
                self.prior_frame.clear();
                self.prior_frame.extend_from_slice(&old_frame);
            }
            let _ = self.state_snapshots.pop_front();
            self.start_tick = self.start_tick.saturating_add(1);
            self.evictions = self.evictions.saturating_add(1);
            evicted = true;
        }
        self.frames.push_back(frame.to_vec());
        self.state_snapshots
            .push_back((structure_epoch, pre_step_registers.to_vec()));
        evicted
    }

    /// Configured exact capacity ($W_{\text{obs}} / 2$).
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Number of wake frames currently buffered.
    #[must_use]
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Returns `true` when no frames have been buffered yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Global 0-based wake tick corresponding to index `0` in the buffer.
    #[must_use]
    pub const fn start_tick(&self) -> u64 {
        self.start_tick
    }

    /// Total number of frames evicted from the buffer (`FactBufferEvictions`).
    #[must_use]
    pub const fn evictions(&self) -> u64 {
        self.evictions
    }

    /// Observed frame immediately preceding `frame_at(0)` (`[0; channels]` before first eviction).
    #[must_use]
    pub fn prior_frame(&self) -> &[u64] {
        &self.prior_frame
    }

    /// Buffered frame at relative index `idx` (`0..self.len()`).
    #[must_use]
    pub fn frame_at(&self, idx: usize) -> Option<&[u64]> {
        self.frames.get(idx).map(Vec::as_slice)
    }

    /// Boundary input frame fed on the tick whose observed truth was `frame_at(idx)`.
    #[must_use]
    pub fn input_for_frame(&self, idx: usize) -> Option<&[u64]> {
        if idx >= self.frames.len() {
            return None;
        }
        if idx == 0 {
            Some(&self.prior_frame)
        } else {
            self.frame_at(idx - 1)
        }
    }

    /// Pre-step `(structure_epoch, registers)` snapshot at relative index `idx`.
    #[must_use]
    pub fn state_snapshot_at(&self, idx: usize) -> Option<(u64, &[u64])> {
        self.state_snapshots
            .get(idx)
            .map(|(epoch, regs)| (*epoch, regs.as_slice()))
    }
}

/// Result of the exact order-`d` two-sample sequential KT code-length match test (`R5(b)`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TwoSampleKtResult {
    /// Clamped Markov context order used for the comparison.
    pub order: usize,
    /// Sequential KT code length of sample 1 in bits.
    pub l_s1_bits: f64,
    /// Sequential KT code length of sample 2 in bits.
    pub l_s2_bits: f64,
    /// Separate code length `l_s1_bits + l_s2_bits`.
    pub l_sep_bits: f64,
    /// Symmetric pooled joint code length $L_{\text{KT}}(S_1 \cup S_2 \mid d)$ in bits.
    pub l_joint_bits: f64,
    /// `l_joint_bits - l_sep_bits` computed symmetrically over shared contexts (`> 0.0` => fail).
    pub delta_bits: f64,
    /// `true` iff `delta_bits <= 0.0` (joint coding does not lose to separate coding).
    pub passed: bool,
}

fn kt_multinomial_context_bits(counts: &BTreeMap<u64, u64>, width: Width) -> f64 {
    let total: u64 = counts.values().sum();
    if total == 0 {
        return 0.0;
    }
    let k = width.bits();
    let half_alpha = if k == 64 {
        2.0f64.powi(63)
    } else {
        0.5 * 2.0f64.powi(i32::from(k))
    };
    let mut log2_den = 0.0f64;
    for t in 0..total {
        log2_den += ((t as f64) + half_alpha).log2();
    }
    let mut log2_num = 0.0f64;
    for &cnt in counts.values() {
        for j in 0..cnt {
            log2_num += ((j as f64) + 0.5).log2();
        }
    }
    (log2_den - log2_num).max(0.0)
}

fn collect_order_d_context_counts(
    seq: &[u64],
    order: usize,
    width: Width,
) -> BTreeMap<Vec<u64>, BTreeMap<u64, u64>> {
    let norm: Vec<u64> = seq.iter().map(|&x| width.normalize(x)).collect();
    let mut map: BTreeMap<Vec<u64>, BTreeMap<u64, u64>> = BTreeMap::new();
    for t in 0..norm.len() {
        let ctx_start = t.saturating_sub(order);
        let ctx = norm[ctx_start..t].to_vec();
        *map.entry(ctx).or_default().entry(norm[t]).or_insert(0) += 1;
    }
    map
}

/// Evaluates the zero-parameter order-`d` two-sample sequential KT code-length test (`R5(b)`).
///
/// Compares `s1` and `s2` over alphabet $2^k$ using progressive contexts up to order
/// `order.min(min(s1.len(), s2.len()).saturating_sub(1))` and exact symmetric pooled context counts.
/// Fails (`passed == false`) iff `L_joint - L_sep > 0.0`.
#[must_use]
pub fn two_sample_kt_match_test(
    s1: &[u64],
    s2: &[u64],
    order: usize,
    width: Width,
) -> TwoSampleKtResult {
    if s1.is_empty() || s2.is_empty() {
        let map1 = collect_order_d_context_counts(s1, 0, width);
        let map2 = collect_order_d_context_counts(s2, 0, width);
        let l_s1_bits: f64 = map1
            .values()
            .map(|c| kt_multinomial_context_bits(c, width))
            .sum();
        let l_s2_bits: f64 = map2
            .values()
            .map(|c| kt_multinomial_context_bits(c, width))
            .sum();
        let l_sep_bits = l_s1_bits + l_s2_bits;
        return TwoSampleKtResult {
            order: 0,
            l_s1_bits,
            l_s2_bits,
            l_sep_bits,
            l_joint_bits: l_sep_bits,
            delta_bits: 0.0,
            passed: true,
        };
    }

    let d_eff = order.min(s1.len().min(s2.len()).saturating_sub(1));
    let map1 = collect_order_d_context_counts(s1, d_eff, width);
    let map2 = collect_order_d_context_counts(s2, d_eff, width);

    let l_s1_bits: f64 = map1
        .values()
        .map(|c| kt_multinomial_context_bits(c, width))
        .sum();
    let l_s2_bits: f64 = map2
        .values()
        .map(|c| kt_multinomial_context_bits(c, width))
        .sum();
    let l_sep_bits = l_s1_bits + l_s2_bits;

    let mut l_joint_bits = 0.0f64;
    let mut delta_bits = 0.0f64;
    let all_contexts: BTreeSet<&Vec<u64>> = map1.keys().chain(map2.keys()).collect();
    for ctx in all_contexts {
        match (map1.get(ctx), map2.get(ctx)) {
            (Some(c1), Some(c2)) => {
                let mut merged = c1.clone();
                for (&sym, &cnt) in c2 {
                    *merged.entry(sym).or_insert(0) += cnt;
                }
                let lj = kt_multinomial_context_bits(&merged, width);
                let l1 = kt_multinomial_context_bits(c1, width);
                let l2 = kt_multinomial_context_bits(c2, width);
                l_joint_bits += lj;
                delta_bits += lj - (l1 + l2);
            }
            (Some(c), None) | (None, Some(c)) => {
                l_joint_bits += kt_multinomial_context_bits(c, width);
            }
            (None, None) => {}
        }
    }

    TwoSampleKtResult {
        order: d_eff,
        l_s1_bits,
        l_s2_bits,
        l_sep_bits,
        l_joint_bits,
        delta_bits,
        passed: delta_bits <= 0.0,
    }
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
    /// Comparator-ticks suspended because causal-support leaves or target touch an ungrounded channel (`R4`, `I7`).
    pub sleep_comparator_suspended_ungrounded: u64,
    /// Number of grounded-port `R5(b)` two-sample KT statistics-match failures across `S-B` segments (`SleepMatchFailures`).
    pub sleep_match_failures: u64,
    /// Number of wake frames evicted from the bounded `FactBuffer` (`FactBufferEvictions`, `R6`).
    pub fact_buffer_evictions: u64,
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
    /// Comparator-ticks suspended in this segment due to ungrounded channels (`R4`, `I7`).
    pub suspended_comparator_ticks: u64,
    /// Number of grounded-port `R5(b)` match failures in this segment (`S-B`).
    pub match_failures: u64,
    /// Total positive agreement flux (in bits) voided by `R5(b)` match failures in this segment.
    pub voided_agreement_flux_bits: f64,
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

/// Per-comparator wake observation recorded for independent `I4` epistemic MDL verification.
#[derive(Clone, Debug)]
pub(crate) struct WakeCompAudit {
    pub comp_id: u64,
    pub target_port: u16,
    pub alt_val: u64,
}

/// Per-comparator sleep observation recorded for independent `I4` epistemic MDL verification.
#[derive(Clone, Debug)]
pub(crate) struct SleepCompAudit {
    pub comp_id: u64,
    pub target_port: u16,
    pub primary_channel_mask: u64,
    pub alt_channel_mask: u64,
    pub pri_val: u64,
    pub alt_val: u64,
    pub suspended: bool,
    pub escrow_id: Option<u64>,
}

/// Trace events emitted when epistemic conservation auditing is explicitly enabled.
#[derive(Clone, Debug)]
pub(crate) enum EpistemicAuditEvent {
    WakeTick {
        preds: Vec<u64>,
        facts: Vec<u64>,
        port_scored: Vec<bool>,
        comp_evals: Vec<WakeCompAudit>,
    },
    SleepSegmentStart {
        schedule: SleepSchedule,
    },
    SleepTick {
        comp_evals: Vec<SleepCompAudit>,
    },
    SleepSegmentEnd {
        voided_ports: Vec<bool>,
        dropped_escrow_ids: Vec<u64>,
    },
    ResetPortPredictor {
        port: usize,
    },
    RegisterComparator {
        comp_id: u64,
        target_port: u16,
    },
    SwapPortAndComparator {
        port: usize,
        comp_id: u64,
    },
}

/// Opt-in trace collector used by [`run_wake_sleep_episode_audited`] and [`verify_epistemic_mdl_conservation`].
#[derive(Clone, Debug)]
pub struct EpistemicTraceCollector {
    pub(crate) width: Width,
    pub(crate) reward_mode: RewardMode,
    pub(crate) split_code_tax: bool,
    pub(crate) initial_port_scorers: Vec<PointForecastScorer>,
    pub(crate) initial_comparators: Vec<(u64, u16, PointForecastScorer)>,
    pub(crate) initial_total_energy: f64,
    pub(crate) initial_code_bits: f64,
    pub(crate) initial_cumulative_net_flux: f64,
    pub(crate) initial_exogenous_bits: f64,
    pub(crate) initial_debt_paid: f64,
    pub(crate) initial_clipped_bits: f64,
    pub(crate) initial_energy_endowed: f64,
    pub(crate) initial_energy_pruned: f64,
    pub(crate) initial_juvenile_shielded: f64,
    pub(crate) events: Vec<EpistemicAuditEvent>,
}

impl EpistemicTraceCollector {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_from_snapshot(
        width: Width,
        reward_mode: RewardMode,
        split_code_tax: bool,
        initial_port_scorers: Vec<PointForecastScorer>,
        initial_comparators: Vec<(u64, u16, PointForecastScorer)>,
        initial_total_energy: f64,
        initial_code_bits: f64,
        initial_cumulative_net_flux: f64,
        initial_exogenous_bits: f64,
        initial_debt_paid: f64,
        initial_clipped_bits: f64,
        initial_energy_endowed: f64,
        initial_energy_pruned: f64,
        initial_juvenile_shielded: f64,
    ) -> Self {
        Self {
            width,
            reward_mode,
            split_code_tax,
            initial_port_scorers,
            initial_comparators,
            initial_total_energy,
            initial_code_bits,
            initial_cumulative_net_flux,
            initial_exogenous_bits,
            initial_debt_paid,
            initial_clipped_bits,
            initial_energy_endowed,
            initial_energy_pruned,
            initial_juvenile_shielded,
            events: Vec::new(),
        }
    }

    pub(crate) fn push(&mut self, event: EpistemicAuditEvent) {
        self.events.push(event);
    }
}

/// Verification summary produced by [`verify_epistemic_mdl_conservation`] (`I4`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EpistemicConservationReport {
    /// Total substrate ticks evaluated (`T`).
    pub total_ticks: u64,
    /// Reference net reward flux computed independently from raw observations and sleep rules.
    pub reference_net_flux_bits: f64,
    /// Learner net flux accumulated over the audited span.
    pub learner_net_flux_bits: f64,
    /// Reference epistemic MDL delta $-\Delta L_{\text{null}}^{\text{epistemic}}(T)$.
    pub reference_neg_delta_l_epistemic_bits: f64,
    /// Adjusted live reserve energy delta accounting for endowments, excisions, debts, clipping, and juvenile shielding.
    pub adjusted_delta_energy_bits: f64,
    /// Exogenous injection bits credited during the audited span ($E_{\text{exog}}$).
    pub delta_exog_bits: f64,
    /// Per-symbol error `|learner_net_flux - reference_net_flux| / T` (`<= 1e-11` required).
    pub flux_mdl_err_bits_per_symbol: f64,
    /// Per-symbol error `|adjusted_delta_energy - reference_net_flux| / T` (`<= 1e-11` required).
    pub energy_flux_err_bits_per_symbol: f64,
    /// Direct `|ΔE - (-ΔL_null^epistemic + E_exog)| / T` when no pruned/debt/clip/juvenile adjustments occurred.
    pub direct_i4_err_bits_per_symbol: Option<f64>,
}

/// Independently replays wake + sleep scorer streams to verify `I4` (`ΔE == -ΔL_null^epistemic + E_exog`)
/// across all `SleepSchedule` arms including `R4` suspensions, `R5(b)` escrow/voiding, and `R5(a)` excisions.
#[must_use]
pub fn verify_epistemic_mdl_conservation(
    audit: &EpistemicTraceCollector,
    learner: &ArkheLearner,
    total_ticks: u64,
) -> EpistemicConservationReport {
    let width = audit.width;
    let mode = audit.reward_mode;
    let mut port_scorers = audit.initial_port_scorers.clone();
    let mut comp_wake_scorers: BTreeMap<u64, PointForecastScorer> = audit
        .initial_comparators
        .iter()
        .map(|(id, _, scorer)| (*id, scorer.clone()))
        .collect();
    let mut comp_sleep_scorers: BTreeMap<u64, PointForecastScorer> = BTreeMap::new();
    let mut current_schedule = SleepSchedule::None;
    let mut pending_escrow: Vec<(u64, usize, f64)> = Vec::new();
    let mut ref_net_flux = 0.0f64;

    for event in &audit.events {
        match event {
            EpistemicAuditEvent::RegisterComparator {
                comp_id,
                target_port,
            } => {
                let wake_null = port_scorers[usize::from(*target_port)].null_model().clone();
                let mut scorer = PointForecastScorer::new(width, wake_null.kind());
                *scorer.null_model_mut() = wake_null;
                comp_wake_scorers.insert(*comp_id, scorer);
            }
            EpistemicAuditEvent::ResetPortPredictor { port } => {
                if let Some(scorer) = port_scorers.get_mut(*port) {
                    scorer.reset_predictor_counts();
                }
            }
            EpistemicAuditEvent::SwapPortAndComparator { port, comp_id } => {
                if let (Some(p_scorer), Some(c_scorer)) = (
                    port_scorers.get_mut(*port),
                    comp_wake_scorers.get_mut(comp_id),
                ) {
                    std::mem::swap(p_scorer, c_scorer);
                }
            }
            EpistemicAuditEvent::WakeTick {
                preds,
                facts,
                port_scored,
                comp_evals,
            } => {
                for (p, (&scored, (&pred, &fact))) in port_scored
                    .iter()
                    .zip(preds.iter().zip(facts.iter()))
                    .enumerate()
                {
                    if scored {
                        ref_net_flux +=
                            score_with_reward_mode(&mut port_scorers[p], mode, width, pred, fact);
                    } else {
                        port_scorers[p].null_model_mut().observe_and_surprisal(fact);
                    }
                }
                for ce in comp_evals {
                    if let Some(scorer) = comp_wake_scorers.get_mut(&ce.comp_id) {
                        let fact = facts[usize::from(ce.target_port)];
                        ref_net_flux +=
                            score_with_reward_mode(scorer, mode, width, ce.alt_val, fact);
                    }
                }
            }
            EpistemicAuditEvent::SleepSegmentStart { schedule } => {
                current_schedule = *schedule;
                comp_sleep_scorers.clear();
                pending_escrow.clear();
            }
            EpistemicAuditEvent::SleepTick { comp_evals } => {
                for ce in comp_evals {
                    if ce.suspended {
                        continue;
                    }
                    let tp = usize::from(ce.target_port);
                    let sleep_scorer = comp_sleep_scorers.entry(ce.comp_id).or_insert_with(|| {
                        let frozen_null = port_scorers[tp].null_model().clone();
                        let mut fresh = PointForecastScorer::new(width, frozen_null.kind());
                        *fresh.null_model_mut() = frozen_null;
                        fresh
                    });
                    let frozen_null = sleep_scorer.null_model().clone();
                    let raw_r =
                        score_with_reward_mode(sleep_scorer, mode, width, ce.alt_val, ce.pri_val);
                    *sleep_scorer.null_model_mut() = frozen_null;
                    let iota =
                        channel_independence_weight(ce.primary_channel_mask, ce.alt_channel_mask);
                    let reward = if width.normalize(ce.alt_val) == width.normalize(ce.pri_val) {
                        raw_r * iota
                    } else {
                        raw_r
                    };
                    if current_schedule == SleepSchedule::ConsistencyReplay && reward > 0.0 {
                        if let Some(eid) = ce.escrow_id {
                            pending_escrow.push((eid, tp, reward));
                        }
                    } else {
                        ref_net_flux += reward;
                    }
                }
            }
            EpistemicAuditEvent::SleepSegmentEnd {
                voided_ports,
                dropped_escrow_ids,
            } => {
                for &(eid, tp, reward) in &pending_escrow {
                    let is_voided = voided_ports.get(tp).copied().unwrap_or(false);
                    let is_dropped = dropped_escrow_ids.contains(&eid);
                    if !is_voided && !is_dropped {
                        ref_net_flux += reward;
                    }
                }
                pending_escrow.clear();
                comp_sleep_scorers.clear();
            }
        }
    }

    let final_total_energy: f64 = learner
        .editable()
        .economy()
        .nodes()
        .iter()
        .map(|n| n.energy)
        .sum();
    let final_code_bits =
        circuit_code_bits_with_tax(learner.editable().circuit(), 0, audit.split_code_tax);
    let net_new_endowments = learner.cumulative_energy_endowed() - audit.initial_energy_endowed;
    let delta_pruned = learner.cumulative_energy_pruned() - audit.initial_energy_pruned;
    let delta_debt = learner.cumulative_debt_paid() - audit.initial_debt_paid;
    let delta_clipped = learner.reserve_clipped_bits() - audit.initial_clipped_bits;
    let delta_shielded = learner.cumulative_juvenile_shielded() - audit.initial_juvenile_shielded;
    let delta_exog = learner.exogenous_injection_bits() - audit.initial_exogenous_bits;

    let raw_delta_e = final_total_energy - audit.initial_total_energy - net_new_endowments;
    let adjusted_delta_e = raw_delta_e + delta_pruned + delta_debt + delta_clipped - delta_shielded;
    let learner_net_flux = learner.cumulative_net_flux() - audit.initial_cumulative_net_flux;
    let ref_delta_l_epistemic = (final_code_bits - audit.initial_code_bits) - ref_net_flux;
    let ref_neg_delta_l_epistemic = -ref_delta_l_epistemic;

    let denom = total_ticks.max(1) as f64;
    let flux_mdl_err_bits_per_symbol = (learner_net_flux - ref_net_flux).abs() / denom;
    let energy_flux_err_bits_per_symbol = (adjusted_delta_e - ref_net_flux).abs() / denom;
    let direct_i4_err_bits_per_symbol =
        (delta_pruned == 0.0 && delta_debt == 0.0 && delta_clipped == 0.0 && delta_shielded == 0.0)
            .then(|| (raw_delta_e - (ref_neg_delta_l_epistemic + delta_exog)).abs() / denom);

    EpistemicConservationReport {
        total_ticks,
        reference_net_flux_bits: ref_net_flux,
        learner_net_flux_bits: learner_net_flux,
        reference_neg_delta_l_epistemic_bits: ref_neg_delta_l_epistemic,
        adjusted_delta_energy_bits: adjusted_delta_e,
        delta_exog_bits: delta_exog,
        flux_mdl_err_bits_per_symbol,
        energy_flux_err_bits_per_symbol,
        direct_i4_err_bits_per_symbol,
    }
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
    run_wake_sleep_episode_core(
        learner,
        env,
        total_substrate_ticks,
        warmup_ticks,
        max_eval_ticks,
        false,
    )
    .0
}

/// Executes [`run_wake_sleep_episode`] with opt-in `I4` epistemic conservation auditing enabled.
pub fn run_wake_sleep_episode_audited<G: FiniteStateGenerator>(
    learner: &mut ArkheLearner,
    env: &mut NoisyEnvironment<G>,
    total_substrate_ticks: u64,
    warmup_ticks: u64,
    max_eval_ticks: u64,
) -> (WakeSleepEpisodeReport, EpistemicConservationReport) {
    let (report, audit_opt) = run_wake_sleep_episode_core(
        learner,
        env,
        total_substrate_ticks,
        warmup_ticks,
        max_eval_ticks,
        true,
    );
    let audit = audit_opt.expect("audited episode must return collector");
    let conservation =
        verify_epistemic_mdl_conservation(&audit, learner, report.total_substrate_ticks);
    (report, conservation)
}

fn run_wake_sleep_episode_core<G: FiniteStateGenerator>(
    learner: &mut ArkheLearner,
    env: &mut NoisyEnvironment<G>,
    total_substrate_ticks: u64,
    warmup_ticks: u64,
    max_eval_ticks: u64,
    enable_audit: bool,
) -> (WakeSleepEpisodeReport, Option<EpistemicTraceCollector>) {
    if enable_audit {
        learner.enable_epistemic_audit();
    }

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

    let audit = if enable_audit {
        learner.take_epistemic_audit()
    } else {
        None
    };

    let report = WakeSleepEpisodeReport {
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
    };
    (report, audit)
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

    #[test]
    fn fact_buffer_enforces_exact_half_horizon_capacity_and_prior_frame_eviction() {
        let cap = observation_horizon(&EconomyConfig::default(), &GrowthConfig::default()) / 2;
        assert_eq!(cap, 128);
        let mut fb = FactBuffer::new(2, cap);
        assert_eq!(fb.capacity(), 128);
        assert!(fb.is_empty());
        assert_eq!(fb.prior_frame(), &[0, 0]);

        for t in 0..128u64 {
            let evicted = fb.push_frame_with_state(&[t, t + 1000], t / 10, &[t]);
            assert!(!evicted);
        }
        assert_eq!(fb.len(), 128);
        assert_eq!(fb.evictions(), 0);
        assert_eq!(fb.start_tick(), 0);
        assert_eq!(fb.prior_frame(), &[0, 0]);
        assert_eq!(fb.frame_at(0), Some(&[0u64, 1000u64][..]));
        assert_eq!(fb.input_for_frame(0), Some(&[0u64, 0u64][..]));
        assert_eq!(fb.state_snapshot_at(0), Some((0, &[0u64][..])));

        // Push 129th frame -> evicts tick 0 into prior_frame.
        assert!(fb.push_frame_with_state(&[128, 1128], 99, &[128]));
        assert_eq!(fb.len(), 128);
        assert_eq!(fb.evictions(), 1);
        assert_eq!(fb.start_tick(), 1);
        assert_eq!(fb.prior_frame(), &[0, 1000]);
        assert_eq!(fb.frame_at(0), Some(&[1u64, 1001u64][..]));
        assert_eq!(fb.input_for_frame(0), Some(&[0u64, 1000u64][..]));
        assert_eq!(fb.state_snapshot_at(0), Some((0, &[1u64][..])));
        assert_eq!(fb.frame_at(127), Some(&[128u64, 1128u64][..]));
        assert_eq!(fb.state_snapshot_at(127), Some((99, &[128u64][..])));
    }

    #[test]
    fn two_sample_kt_test_distinguishes_order_d_cycles_missed_by_order_zero() {
        let s_0101 = [0, 1, 0, 1];
        let s_0011 = [0, 0, 1, 1];

        // 1. Identical sequences pass at all orders d >= 0.
        for d in 0..=4 {
            let res = two_sample_kt_match_test(&s_0101, &s_0101, d, Width::BITS_1);
            assert!(
                res.passed && res.delta_bits < 0.0,
                "0101 vs 0101 must pass at d={d}: {res:?}"
            );
        }

        // 2. Order 0 misses 0101 vs 0011 (identical 0/1 marginals), while d >= 1 catches it.
        let r0 = two_sample_kt_match_test(&s_0101, &s_0011, 0, Width::BITS_1);
        assert!(
            r0.passed && r0.delta_bits < 0.0,
            "order 0 must miss 0101 vs 0011: {r0:?}"
        );
        for d in [1usize, 2, 3, 5] {
            let rd = two_sample_kt_match_test(&s_0101, &s_0011, d, Width::BITS_1);
            assert!(
                !rd.passed && rd.delta_bits > 0.0,
                "order d={d} must reject 0101 vs 0011: {rd:?}"
            );
            // Clamping check when d > len - 1
            assert_eq!(rd.order, d.min(3));
            // Exact symmetry check
            let rev = two_sample_kt_match_test(&s_0011, &s_0101, d, Width::BITS_1);
            assert_eq!(rd.l_joint_bits.to_bits(), rev.l_joint_bits.to_bits());
            assert_eq!(rd.l_sep_bits.to_bits(), rev.l_sep_bits.to_bits());
            assert_eq!(rd.delta_bits.to_bits(), rev.delta_bits.to_bits());
        }

        // 3. Disjoint constant sequences fail at d >= 1 as well as d = 0.
        let zeros = vec![0u64; 32];
        let ones = vec![1u64; 32];
        for d in [0usize, 1, 2, 4] {
            let rc = two_sample_kt_match_test(&zeros, &ones, d, Width::BITS_1);
            assert!(
                !rc.passed && rc.delta_bits > 0.0,
                "0..0 vs 1..1 must fail at d={d}: {rc:?}"
            );
        }

        // 4. Phase-shifted periodic sequences over k = 2 pass at d >= 1.
        let p1: Vec<u64> = (0..64).map(|t| (t % 4) as u64).collect();
        let p2: Vec<u64> = (1..65).map(|t| (t % 4) as u64).collect();
        let rp = two_sample_kt_match_test(&p1, &p2, 2, Width::BITS_2);
        assert!(
            rp.passed && rp.delta_bits < 0.0,
            "phase-shifted period-4 must pass at k=2, d=2: {rp:?}"
        );
    }
}
