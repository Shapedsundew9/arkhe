//! Equal-compute prequential baseline harness (Charter §7 & §8).
//!
//! Includes the five Milestone 0 discrete/symbolic baselines:
//! - [`RandomSearchBaseline`] (`random_search`)
//! - [`CgpBaseline`] (`cgp`)
//! - [`CtwBaseline`] (`ctw`)
//! - [`ReedsSloaneBaseline`] (`bm_predictor`)
//! - [`ExhaustiveBaseline`] (`exhaustive`)
//!
//! Note: The small `RNN` / `LSTM` BPTT baseline is scheduled for Milestone M1b/M2 and slots
//! directly into [`PredictorBaseline`] without harness changes.

pub mod bm_predictor;
pub mod cgp;
pub mod ctw;
pub mod exhaustive;
pub mod random_search;

use std::collections::HashMap;

pub use bm_predictor::ReedsSloaneBaseline;
pub use cgp::CgpBaseline;
pub use ctw::CtwBaseline;
pub use exhaustive::{
    DEFAULT_MAX_CERTIFY_JOINT_STEPS, ExhaustiveBaseline, SynthesisCertStatus,
    build_z4_motif_013_reactive_one_reg_witness, certify_circuit_against_spec,
    enumerate_canonical_circuits, exact_synthesize_certified_circuit,
    exact_synthesize_clean_circuit,
};
pub use random_search::{RandomSearchBaseline, sample_random_valid_circuit};

use crate::env::{
    EpsilonMachineSummary, FiniteStateGenerator, NoisyEnvironment,
    declared_noise_entropy_bits_per_symbol, realized_oracle_step_bits,
};
use crate::metrics::{BudgetAccountingMode, Scorecard, compute_mdl, facts_code_bits};
use crate::structural::WiringTopology;
use crate::substrate::{Circuit, CompiledCircuit, ScalarState};

const WINDOW_EVICT_CHUNK: usize = 16;
const MAX_CYCLE_DETECT_STEPS: u64 = 512;

/// Shared tick counter enforcing Charter §8 equal-compute candidate-evaluation budgets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TickBudget {
    remaining: u64,
    consumed: u64,
}

impl TickBudget {
    /// Creates a new [`TickBudget`] allowing up to `max_ticks` internal candidate evaluations.
    #[must_use]
    pub const fn new(max_ticks: u64) -> Self {
        Self {
            remaining: max_ticks,
            consumed: 0,
        }
    }

    /// Remaining evaluation ticks available.
    #[must_use]
    pub const fn remaining(&self) -> u64 {
        self.remaining
    }

    /// Total evaluation ticks spent so far.
    #[must_use]
    pub const fn consumed(&self) -> u64 {
        self.consumed
    }

    /// Attempts to deduct `ticks`; returns `true` on success, or `false` if insufficient budget remains.
    pub fn try_consume(&mut self, ticks: u64) -> bool {
        if self.remaining >= ticks {
            self.remaining -= ticks;
            self.consumed += ticks;
            true
        } else {
            false
        }
    }
}

/// Phase-exact sliding observation window for candidate substrate circuit replay.
///
/// Tracks `start_tick` (the global 0-based tick of `frames[0]`) and `prior_frame` (the observed
/// frame at `start_tick - 1`, or `[0; channels]` when `start_tick == 0`) so both reactive
/// `BoundaryIn` circuits and purely autonomous `Delay` oscillators preserve exact phase alignment
/// across window evictions for arbitrary generator periods (including primes such as `5`, `7`, `257`).
#[derive(Clone, Debug)]
pub struct ObservationWindow {
    start_tick: u64,
    prior_frame: Vec<u64>,
    frames: Vec<Vec<u64>>,
    capacity: usize,
}

impl ObservationWindow {
    /// Creates an empty [`ObservationWindow`] with target steady-state `capacity`.
    #[must_use]
    pub fn new(channels: u16, capacity: usize) -> Self {
        let cap = capacity.max(16);
        Self {
            start_tick: 0,
            prior_frame: vec![0u64; usize::from(channels)],
            frames: Vec::with_capacity(cap + WINDOW_EVICT_CHUNK),
            capacity: cap,
        }
    }

    /// Wraps a static prefix trace starting at tick `0`.
    #[must_use]
    pub fn from_prefix(channels: u16, prefix_frames: &[Vec<u64>]) -> Self {
        Self {
            start_tick: 0,
            prior_frame: vec![0u64; usize::from(channels)],
            frames: prefix_frames.to_vec(),
            capacity: prefix_frames.len().max(16),
        }
    }

    /// Appends a newly observed frame and evicts oldest frames in chunks once capacity is exceeded,
    /// updating `start_tick` and `prior_frame` identically.
    pub fn push_frame(&mut self, frame: &[u64]) {
        self.frames.push(frame.to_vec());
        if self.frames.len() >= self.capacity + WINDOW_EVICT_CHUNK {
            let evict = WINDOW_EVICT_CHUNK;
            self.prior_frame.copy_from_slice(&self.frames[evict - 1]);
            self.frames.drain(0..evict);
            self.start_tick += evict as u64;
        }
    }

    /// Global 0-based tick index corresponding to `self.frames()[0]`.
    #[must_use]
    pub const fn start_tick(&self) -> u64 {
        self.start_tick
    }

    /// Observed frame immediately preceding `frames[0]` (`[0; channels]` when `start_tick == 0`).
    #[must_use]
    pub fn prior_frame(&self) -> &[u64] {
        &self.prior_frame
    }

    /// Buffered observation frames `y(start_tick .. start_tick + len)`.
    #[must_use]
    pub fn frames(&self) -> &[Vec<u64>] {
        &self.frames
    }

    /// Number of buffered frames currently in the window.
    #[must_use]
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Returns `true` if no frames have been pushed yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
}

/// Unified prequential predictor interface for baselines and future learners.
pub trait PredictorBaseline {
    /// Baseline display name.
    fn name(&self) -> &str;

    /// Step 1 (causal): commit to next-tick symbol predictions $\hat{y}(t)$ prior to seeing $y(t)$.
    fn predict(&mut self, budget: &mut TickBudget) -> Vec<u64>;

    /// Step 2: observe ground-truth frame $y(t)$, optionally returning exact prequential log-loss
    /// (in bits across all channels for tick $t$) before updating internal parameters/circuits.
    fn observe(&mut self, truth: &[u64], budget: &mut TickBudget) -> Option<f64>;

    /// Returns the best discovered substrate [`Circuit`], if this predictor synthesises circuits.
    fn best_circuit(&self) -> Option<&Circuit>;

    /// Declares whether internal candidate evaluations are metered against [`TickBudget`].
    fn budget_accounting_mode(&self) -> BudgetAccountingMode {
        BudgetAccountingMode::CandidateHistoryReplay
    }
}

fn align_autonomous_state_to_tick(
    state: &mut ScalarState,
    compiled: &CompiledCircuit,
    target_tick: u64,
    mut budget: Option<&mut TickBudget>,
) -> bool {
    if target_tick == 0 || compiled.uses_boundary_inputs() || compiled.total_registers() == 0 {
        return true;
    }

    let zeros = vec![0u64; usize::from(compiled.num_inputs())];
    let mut dummy_out = vec![0u64; usize::from(compiled.num_outputs())];

    if target_tick <= 32 {
        for _ in 0..target_tick {
            if let Some(b) = budget.as_deref_mut()
                && !b.try_consume(1)
            {
                return false;
            }
            state
                .step(compiled, &zeros, &mut dummy_out)
                .expect("matching port counts");
        }
        return true;
    }

    let detect_cap = MAX_CYCLE_DETECT_STEPS.min(target_tick);
    let mut reg_history: Vec<Vec<u64>> = Vec::with_capacity((detect_cap as usize) + 1);
    let mut seen: HashMap<Vec<u64>, u64> = HashMap::with_capacity((detect_cap as usize) + 1);
    let init_regs = state.registers().to_vec();
    seen.insert(init_regs.clone(), 0);
    reg_history.push(init_regs);

    for step_idx in 1..=detect_cap {
        if let Some(b) = budget.as_deref_mut()
            && !b.try_consume(1)
        {
            return false;
        }
        state
            .step(compiled, &zeros, &mut dummy_out)
            .expect("matching port counts");
        if step_idx == target_tick {
            return true;
        }
        let regs = state.registers().to_vec();
        if let Some(&cycle_start) = seen.get(&regs) {
            let cycle_len = step_idx - cycle_start;
            let offset = (target_tick - cycle_start) % cycle_len;
            let aligned_idx = (cycle_start + offset) as usize;
            state
                .set_registers(compiled, &reg_history[aligned_idx])
                .expect("matching register count");
            return true;
        }
        seen.insert(regs.clone(), step_idx);
        reg_history.push(regs);
    }

    let rem = target_tick - detect_cap;
    if let Some(b) = budget
        && !b.try_consume(rem)
    {
        return false;
    }
    for _ in 0..rem {
        state
            .step(compiled, &zeros, &mut dummy_out)
            .expect("matching port counts");
    }
    true
}

fn replay_aligned_window(
    mut state: ScalarState,
    compiled: &CompiledCircuit,
    window: &ObservationWindow,
) -> (u64, u64, ScalarState) {
    let n_in = usize::from(compiled.num_inputs());
    let mut prev_obs = vec![0u64; n_in];
    let copy_len = n_in.min(window.prior_frame().len());
    prev_obs[..copy_len].copy_from_slice(&window.prior_frame()[..copy_len]);
    let mut pred = vec![0u64; usize::from(compiled.num_outputs())];

    let raw_skip = if compiled.uses_boundary_inputs() {
        if window.start_tick() == 0 {
            1 + compiled.total_registers()
        } else {
            compiled.total_registers()
        }
    } else {
        (1 + compiled.total_registers())
            .saturating_sub(usize::try_from(window.start_tick()).unwrap_or(usize::MAX))
    };
    let sync_skip = raw_skip.min(window.len() / 3);

    let mut mismatches = 0u64;
    let mut scored_symbols = 0u64;

    for (t, frame) in window.frames().iter().enumerate() {
        state
            .step(compiled, &prev_obs, &mut pred)
            .expect("matching port counts");
        if t >= sync_skip {
            for (&p, &actual) in pred.iter().zip(frame) {
                scored_symbols += 1;
                if p != actual {
                    mismatches += 1;
                }
            }
        }
        let obs_copy = n_in.min(frame.len());
        prev_obs[..obs_copy].copy_from_slice(&frame[..obs_copy]);
    }

    (mismatches, scored_symbols, state)
}

/// Evaluates `compiled` causally across `window` within `budget`, aligning initial `BoundaryIn`
/// lag from `window.prior_frame()` and advancing autonomous register states to `window.start_tick()`
/// via cycle-modular jump.
///
/// Returns `None` if `budget` has insufficient ticks remaining.
pub fn evaluate_circuit_on_window(
    compiled: &CompiledCircuit,
    window: &ObservationWindow,
    budget: &mut TickBudget,
) -> Option<(u64, u64, ScalarState)> {
    let window_ticks = window.len() as u64;
    if budget.remaining() < window_ticks {
        return None;
    }
    let mut state = ScalarState::new(compiled);
    if !align_autonomous_state_to_tick(&mut state, compiled, window.start_tick(), Some(budget)) {
        return None;
    }
    if !budget.try_consume(window_ticks) {
        return None;
    }
    Some(replay_aligned_window(state, compiled, window))
}

/// Unmetered offline helper for exact-synthesis prefix fallback and unit testing.
#[must_use]
pub fn evaluate_circuit_on_window_unmetered(
    compiled: &CompiledCircuit,
    window: &ObservationWindow,
) -> (u64, u64, ScalarState) {
    let mut state = ScalarState::new(compiled);
    let _ = align_autonomous_state_to_tick(&mut state, compiled, window.start_tick(), None);
    replay_aligned_window(state, compiled, window)
}

/// Executes a prequential evaluation run of `baseline` on `env` over `horizon_ticks` and builds a [`Scorecard`].
#[allow(clippy::too_many_arguments)]
pub fn run_prequential_harness<G: FiniteStateGenerator>(
    env: &mut NoisyEnvironment<G>,
    baseline: &mut dyn PredictorBaseline,
    wiring_topology: WiringTopology,
    horizon_ticks: u64,
    warmup_ticks: u64,
    max_eval_ticks: u64,
    em_ground_truth: Option<&EpsilonMachineSummary>,
    exact_synthesis_min_nodes: Option<usize>,
) -> Scorecard {
    assert!(warmup_ticks < horizon_ticks);
    let width = env.generator().width();
    let num_channels_u16 = env.generator().channels();
    let channels = usize::from(num_channels_u16);

    let mut budget = TickBudget::new(max_eval_ticks);
    let mut clean_frame = vec![0u64; channels];
    let mut observed_frame = vec![0u64; channels];

    let mut post_obs_mismatches = 0u64;
    let mut post_clean_mismatches = 0u64;
    let mut total_obs_mismatches = 0u64;
    let mut post_circular_sum = 0u128;
    let mut post_prob_loss_bits = 0.0f64;
    let mut has_prob_loss = true;
    let mut post_oracle_bits = 0.0f64;
    let mut has_oracle_bits = true;
    let mut last_clean_mismatch_tick: Option<u64> = None;

    for tick in 0..horizon_ticks {
        let pred = baseline.predict(&mut budget);
        env.step_frame(&mut clean_frame, &mut observed_frame);
        let step_loss = baseline.observe(&observed_frame, &mut budget);

        let mut clean_ok = true;
        for ch in 0..channels {
            let p = width.normalize(pred[ch]);
            if p != observed_frame[ch] {
                total_obs_mismatches += 1;
                if tick >= warmup_ticks {
                    post_obs_mismatches += 1;
                }
            }
            if p != clean_frame[ch] {
                clean_ok = false;
                if tick >= warmup_ticks {
                    post_clean_mismatches += 1;
                }
            }
            if tick >= warmup_ticks {
                post_circular_sum += u128::from(width.circular_distance(p, observed_frame[ch]));
            }
        }

        if !clean_ok {
            last_clean_mismatch_tick = Some(tick);
        }

        if tick >= warmup_ticks {
            if let Some(ob) =
                realized_oracle_step_bits(env.noise(), width, &clean_frame, &observed_frame)
            {
                post_oracle_bits += ob;
            } else {
                has_oracle_bits = false;
            }
            if let Some(bits) = step_loss {
                post_prob_loss_bits += bits;
            } else {
                has_prob_loss = false;
            }
        }
    }

    let evaluated_ticks = horizon_ticks - warmup_ticks;
    let evaluated_symbols = evaluated_ticks * (channels as u64);
    let total_symbols = horizon_ticks * (channels as u64);

    let post_transient_observed_match_rate =
        ((evaluated_symbols - post_obs_mismatches) as f64) / (evaluated_symbols as f64);
    let post_transient_clean_match_rate =
        ((evaluated_symbols - post_clean_mismatches) as f64) / (evaluated_symbols as f64);

    let native_prob_log_loss_bits_per_symbol =
        has_prob_loss.then_some(post_prob_loss_bits / (evaluated_symbols as f64));
    let prequential_kt_residual_bits_per_symbol =
        facts_code_bits(width, evaluated_symbols, post_obs_mismatches) / (evaluated_symbols as f64);

    let excess_prob_bits_over_h_mu = match (native_prob_log_loss_bits_per_symbol, em_ground_truth) {
        (Some(loss), Some(em)) => Some(loss - em.h_mu_bits_per_symbol),
        _ => None,
    };
    let excess_residual_bits_over_h_mu =
        em_ground_truth.map(|em| prequential_kt_residual_bits_per_symbol - em.h_mu_bits_per_symbol);

    let declared_noise_h =
        declared_noise_entropy_bits_per_symbol(env.noise(), num_channels_u16, width);
    let realized_oracle_bits_per_symbol = em_ground_truth.and_then(|em| {
        (has_oracle_bits && (em.h_mu_bits_per_symbol - declared_noise_h).abs() < 1e-6)
            .then_some(post_oracle_bits / (evaluated_symbols as f64))
    });
    let paired_prob_regret_bits_per_symbol = match (
        native_prob_log_loss_bits_per_symbol,
        realized_oracle_bits_per_symbol,
    ) {
        (Some(loss), Some(oracle)) => Some(loss - oracle),
        _ => None,
    };
    let paired_residual_regret_bits_per_symbol = realized_oracle_bits_per_symbol
        .map(|oracle| prequential_kt_residual_bits_per_symbol - oracle);
    let paired_regret_bits_per_symbol =
        paired_prob_regret_bits_per_symbol.or(paired_residual_regret_bits_per_symbol);

    let mean_circular_discrepancy = (post_circular_sum as f64) / (evaluated_symbols as f64);

    let ticks_to_clean_solution = match last_clean_mismatch_tick {
        None => Some(0),
        Some(last_bad) if last_bad + 1 < horizon_ticks => Some(last_bad + 1),
        Some(_) => None,
    };

    let best_circ = baseline.best_circuit();
    let node_count = best_circ.map(Circuit::node_count);
    let (
        delay_count,
        wire_hop_register_count,
        boundary_lag_register_count,
        effective_state_registers,
        internal_state_bits,
        effective_excess_state_bits_over_c_mu,
    ) = if let Some(c) = best_circ {
        let comp = c
            .compile()
            .expect("baseline best_circuit satisfies §2.5 invariants");
        let d_cnt = comp.explicit_delay_count();
        let hop_cnt = comp.total_registers() - d_cnt;
        let lag_cnt = usize::from(comp.active_boundary_input_count());
        let eff_regs = d_cnt + hop_cnt + lag_cnt;
        let k_usize = usize::from(width.bits());
        let int_bits = (d_cnt + hop_cnt) * k_usize;
        let eff_bits = (eff_regs * k_usize) as f64;
        let eff_excess = em_ground_truth.map(|em| eff_bits - em.c_mu_bits);
        (
            Some(d_cnt),
            Some(hop_cnt),
            Some(lag_cnt),
            Some(eff_regs),
            Some(int_bits),
            eff_excess,
        )
    } else {
        (None, None, None, None, None, None)
    };

    let excess_nodes_over_exact_synthesis = match (node_count, exact_synthesis_min_nodes) {
        (Some(n), Some(opt)) => Some((n as isize) - (opt as isize)),
        _ => None,
    };
    let mdl = best_circ.map(|c| {
        compute_mdl(
            c,
            wiring_topology,
            None,
            0.0,
            0,
            total_symbols,
            total_obs_mismatches,
        )
    });

    Scorecard {
        horizon_ticks,
        warmup_ticks,
        evaluated_ticks,
        post_transient_observed_mismatches: post_obs_mismatches,
        post_transient_clean_mismatches: post_clean_mismatches,
        post_transient_observed_match_rate,
        post_transient_clean_match_rate,
        native_prob_log_loss_bits_per_symbol,
        prequential_kt_residual_bits_per_symbol,
        excess_prob_bits_over_h_mu,
        excess_residual_bits_over_h_mu,
        realized_oracle_bits_per_symbol,
        paired_prob_regret_bits_per_symbol,
        paired_residual_regret_bits_per_symbol,
        paired_regret_bits_per_symbol,
        mean_circular_discrepancy,
        ticks_to_clean_solution,
        node_count,
        delay_count,
        wire_hop_register_count,
        boundary_lag_register_count,
        effective_state_registers,
        internal_state_bits,
        effective_excess_state_bits_over_c_mu,
        excess_nodes_over_exact_synthesis,
        mdl,
        budget_accounting_mode: baseline.budget_accounting_mode(),
        candidate_replay_ticks_consumed: budget.consumed(),
    }
}
