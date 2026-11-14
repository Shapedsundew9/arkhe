//! Online 1-layer `tanh` Elman RNN trained with truncated BPTT (M2a §6.2).
//!
//! Serves as an honest gradient-based cross-channel baseline operating strictly on
//! concatenated one-hot boundary symbols with zero-initialized biases, unclipped gradients,
//! and explicit [`BudgetAccountingMode::BpttUnrollSteps`] tick metering.

use std::collections::VecDeque;
use std::fmt;

use rand_core::Rng;

use crate::baselines::{PredictorBaseline, TickBudget};
use crate::metrics::BudgetAccountingMode;
use crate::rng::{SeedBank, StreamDomain};
use crate::substrate::{Circuit, Width};

/// Default hidden layer width for [`RnnBaseline`].
pub const DEFAULT_RNN_HIDDEN_SIZE: usize = 16;

/// Default truncated BPTT unroll horizon for [`RnnBaseline`].
pub const DEFAULT_RNN_BPTT_LEN: usize = 16;

/// Default unclipped SGD learning rate for [`RnnBaseline`].
pub const DEFAULT_RNN_LEARNING_RATE: f64 = 0.05;

/// Maximum symbol bit-width supported by one-hot input/softmax embeddings (`2^8 = 256`).
pub const MAX_RNN_ONE_HOT_WIDTH_BITS: u8 = 8;

/// Validation error returned when constructing [`RnnBaseline`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RnnError {
    /// Width exceeds [`MAX_RNN_ONE_HOT_WIDTH_BITS`].
    UnsupportedWidth { bits: u8, max_bits: u8 },
    /// Zero output/input channels specified.
    InvalidChannels(u16),
    /// Invalid hyperparameter (zero size/length or non-positive/non-finite learning rate).
    InvalidHyperparameter(&'static str),
}

impl fmt::Display for RnnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedWidth { bits, max_bits } => write!(
                f,
                "RnnBaseline supports width <= {max_bits} bits for one-hot encoding, got {bits}"
            ),
            Self::InvalidChannels(c) => write!(f, "RnnBaseline requires channels >= 1, got {c}"),
            Self::InvalidHyperparameter(msg) => write!(f, "invalid RnnBaseline config: {msg}"),
        }
    }
}

impl std::error::Error for RnnError {}

/// Single-layer `tanh` Elman RNN trained online via truncated BPTT.
#[derive(Clone, Debug)]
pub struct RnnBaseline {
    width: Width,
    channels: usize,
    vocab_size: usize,
    input_dim: usize,
    hidden_size: usize,
    bptt_len: usize,
    learning_rate: f64,
    w_xh: Vec<f64>,
    w_hh: Vec<f64>,
    b_h: Vec<f64>,
    w_hy: Vec<f64>,
    b_y: Vec<f64>,
    h_curr: Vec<f64>,
    h_window_start: Vec<f64>,
    prev_observed: Vec<u64>,
    pending_logits: Option<Vec<f64>>,
    pending_h_next: Option<Vec<f64>>,
    history: VecDeque<(Vec<u64>, Vec<u64>)>,
}

fn u64_to_unit_f64(bits: u64) -> f64 {
    ((bits >> 11) as f64) * (1.0 / ((1u64 << 53) as f64))
}

fn glorot_uniform_vec(rng: &mut impl Rng, len: usize, fan_in: usize, fan_out: usize) -> Vec<f64> {
    let limit = (6.0 / ((fan_in + fan_out) as f64)).sqrt();
    let mut out = Vec::with_capacity(len);
    for _ in 0..len {
        let u = u64_to_unit_f64(rng.next_u64());
        out.push((u * 2.0 - 1.0) * limit);
    }
    out
}

fn softmax_probs_and_loss_bits(logits: &[f64], target_sym: usize) -> (Vec<f64>, f64) {
    let max_z = logits.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let mut probs = Vec::with_capacity(logits.len());
    let mut sum_exp = 0.0f64;
    for &z in logits {
        let e = (z - max_z).exp();
        sum_exp += e;
        probs.push(e);
    }
    let inv = 1.0 / sum_exp;
    for p in &mut probs {
        *p *= inv;
    }
    let log_sum_exp = max_z + sum_exp.ln();
    let loss_nats = (log_sum_exp - logits[target_sym]).max(0.0);
    (probs, loss_nats / std::f64::consts::LN_2)
}

impl RnnBaseline {
    /// Constructs a Glorot-uniform Elman RNN baseline from `seed_bank`.
    pub fn new(
        width: Width,
        channels: u16,
        hidden_size: usize,
        bptt_len: usize,
        learning_rate: f64,
        seed_bank: SeedBank,
    ) -> Result<Self, RnnError> {
        let bits = width.bits();
        if bits > MAX_RNN_ONE_HOT_WIDTH_BITS {
            return Err(RnnError::UnsupportedWidth {
                bits,
                max_bits: MAX_RNN_ONE_HOT_WIDTH_BITS,
            });
        }
        if channels == 0 {
            return Err(RnnError::InvalidChannels(channels));
        }
        if hidden_size == 0 {
            return Err(RnnError::InvalidHyperparameter("hidden_size must be >= 1"));
        }
        if bptt_len == 0 {
            return Err(RnnError::InvalidHyperparameter("bptt_len must be >= 1"));
        }
        if !learning_rate.is_finite() || learning_rate <= 0.0 {
            return Err(RnnError::InvalidHyperparameter(
                "learning_rate must be finite and > 0",
            ));
        }

        let k = usize::from(channels);
        let vocab_size = 1usize << bits;
        let input_dim = k * vocab_size;
        let output_dim = k * vocab_size;
        let mut rng = seed_bank.stream(StreamDomain::BaselineSearch { trial: 0 });

        let w_xh = glorot_uniform_vec(&mut rng, hidden_size * input_dim, input_dim, hidden_size);
        let w_hh = glorot_uniform_vec(
            &mut rng,
            hidden_size * hidden_size,
            hidden_size,
            hidden_size,
        );
        let b_h = vec![0.0; hidden_size];
        let w_hy = glorot_uniform_vec(&mut rng, output_dim * hidden_size, hidden_size, vocab_size);
        let b_y = vec![0.0; output_dim];

        Ok(Self {
            width,
            channels: k,
            vocab_size,
            input_dim,
            hidden_size,
            bptt_len,
            learning_rate,
            w_xh,
            w_hh,
            b_h,
            w_hy,
            b_y,
            h_curr: vec![0.0; hidden_size],
            h_window_start: vec![0.0; hidden_size],
            prev_observed: vec![0; k],
            pending_logits: None,
            pending_h_next: None,
            history: VecDeque::with_capacity(bptt_len),
        })
    }

    /// Constructs an [`RnnBaseline`] with default hyperparameters (`H=16`, `bptt_len=16`, `lr=0.05`).
    pub fn with_defaults(
        width: Width,
        channels: u16,
        seed_bank: SeedBank,
    ) -> Result<Self, RnnError> {
        Self::new(
            width,
            channels,
            DEFAULT_RNN_HIDDEN_SIZE,
            DEFAULT_RNN_BPTT_LEN,
            DEFAULT_RNN_LEARNING_RATE,
            seed_bank,
        )
    }

    /// Hidden layer width $H$.
    #[must_use]
    pub const fn hidden_size(&self) -> usize {
        self.hidden_size
    }

    /// Truncated BPTT window length.
    #[must_use]
    pub const fn bptt_len(&self) -> usize {
        self.bptt_len
    }

    /// Online SGD learning rate.
    #[must_use]
    pub const fn learning_rate(&self) -> f64 {
        self.learning_rate
    }

    fn forward_hidden(&self, h_prev: &[f64], input_syms: &[u64]) -> Vec<f64> {
        let h_dim = self.hidden_size;
        let mut h_next = vec![0.0; h_dim];
        for (j, out) in h_next.iter_mut().enumerate() {
            let mut pre = self.b_h[j];
            let xh_row = j * self.input_dim;
            for (c, &sym) in input_syms.iter().enumerate() {
                let s = (sym as usize) & (self.vocab_size - 1);
                pre += self.w_xh[xh_row + c * self.vocab_size + s];
            }
            let hh_row = j * h_dim;
            for (i, &hp) in h_prev.iter().enumerate() {
                pre += self.w_hh[hh_row + i] * hp;
            }
            *out = pre.tanh();
        }
        h_next
    }

    fn forward_logits(&self, h: &[f64]) -> Vec<f64> {
        let h_dim = self.hidden_size;
        let out_dim = self.channels * self.vocab_size;
        let mut logits = Vec::with_capacity(out_dim);
        for row in 0..out_dim {
            let w_row = row * h_dim;
            let mut z = self.b_y[row];
            for (j, &hj) in h.iter().enumerate() {
                z += self.w_hy[w_row + j] * hj;
            }
            logits.push(z);
        }
        logits
    }

    fn apply_bptt_step(&mut self) {
        let steps = self.history.len();
        if steps == 0 {
            return;
        }
        let h_dim = self.hidden_size;
        let out_dim = self.channels * self.vocab_size;

        // Forward unroll over the truncated window [start .. start + steps).
        let mut h_states = Vec::with_capacity(steps + 1);
        h_states.push(self.h_window_start.clone());
        for (in_syms, _) in &self.history {
            let next_h = self.forward_hidden(h_states.last().expect("non-empty"), in_syms);
            h_states.push(next_h);
        }

        // Exact cross-entropy gradient wrt final tick t = steps - 1 across all K softmax heads.
        let last_h = &h_states[steps];
        let logits_last = self.forward_logits(last_h);
        let (_, target_last) = self.history.back().expect("steps > 0");

        let mut dz = vec![0.0; out_dim];
        for (c, &tgt) in target_last.iter().enumerate() {
            let offset = c * self.vocab_size;
            let s = (tgt as usize) & (self.vocab_size - 1);
            let (probs, _) =
                softmax_probs_and_loss_bits(&logits_last[offset..offset + self.vocab_size], s);
            for (v, p) in probs.into_iter().enumerate() {
                dz[offset + v] = if v == s { p - 1.0 } else { p };
            }
        }

        let mut dw_hy = vec![0.0; out_dim * h_dim];
        let db_y = dz.clone();
        let mut dh = vec![0.0; h_dim];
        for (row, &g_z) in dz.iter().enumerate() {
            let w_row = row * h_dim;
            for j in 0..h_dim {
                dw_hy[w_row + j] = g_z * last_h[j];
                dh[j] += self.w_hy[w_row + j] * g_z;
            }
        }

        let mut dw_xh = vec![0.0; self.w_xh.len()];
        let mut dw_hh = vec![0.0; self.w_hh.len()];
        let mut db_h = vec![0.0; h_dim];

        // Backpropagate unclipped gradients backward through time steps (steps - 1 ..= 0).
        let frames: Vec<&(Vec<u64>, Vec<u64>)> = self.history.iter().collect();
        for u in (0..steps).rev() {
            let h_prev = &h_states[u];
            let h_next = &h_states[u + 1];
            let (in_syms, _) = frames[u];

            let mut dpre = vec![0.0; h_dim];
            for j in 0..h_dim {
                let dtanh = 1.0 - h_next[j] * h_next[j];
                let g = dh[j] * dtanh;
                dpre[j] = g;
                db_h[j] += g;
            }

            let mut dh_prev = vec![0.0; h_dim];
            for (j, &g) in dpre.iter().enumerate() {
                let xh_row = j * self.input_dim;
                for (c, &sym) in in_syms.iter().enumerate() {
                    let s = (sym as usize) & (self.vocab_size - 1);
                    dw_xh[xh_row + c * self.vocab_size + s] += g;
                }
                let hh_row = j * h_dim;
                for i in 0..h_dim {
                    dw_hh[hh_row + i] += g * h_prev[i];
                    dh_prev[i] += self.w_hh[hh_row + i] * g;
                }
            }
            dh = dh_prev;
        }

        let lr = self.learning_rate;
        for (w, g) in self.w_hy.iter_mut().zip(&dw_hy) {
            *w -= lr * g;
        }
        for (b, g) in self.b_y.iter_mut().zip(&db_y) {
            *b -= lr * g;
        }
        for (w, g) in self.w_xh.iter_mut().zip(&dw_xh) {
            *w -= lr * g;
        }
        for (w, g) in self.w_hh.iter_mut().zip(&dw_hh) {
            *w -= lr * g;
        }
        for (b, g) in self.b_h.iter_mut().zip(&db_h) {
            *b -= lr * g;
        }
    }
}

impl PredictorBaseline for RnnBaseline {
    fn name(&self) -> &str {
        "RnnBPTT"
    }

    fn predict(&mut self, _budget: &mut TickBudget) -> Vec<u64> {
        let h_next = self.forward_hidden(&self.h_curr, &self.prev_observed);
        let logits = self.forward_logits(&h_next);
        let mut preds = Vec::with_capacity(self.channels);
        for c in 0..self.channels {
            let offset = c * self.vocab_size;
            let head = &logits[offset..offset + self.vocab_size];
            let mut best_v = 0usize;
            let mut best_z = head[0];
            for (v, &z) in head.iter().enumerate().skip(1) {
                if z > best_z {
                    best_z = z;
                    best_v = v;
                }
            }
            preds.push(best_v as u64);
        }
        self.pending_h_next = Some(h_next);
        self.pending_logits = Some(logits);
        preds
    }

    fn observe(&mut self, truth: &[u64], budget: &mut TickBudget) -> Option<f64> {
        assert_eq!(
            truth.len(),
            self.channels,
            "RnnBaseline::observe expected {} channels, got {}",
            self.channels,
            truth.len()
        );
        let norm_truth: Vec<u64> = truth.iter().map(|&v| self.width.normalize(v)).collect();

        let h_next = self
            .pending_h_next
            .take()
            .unwrap_or_else(|| self.forward_hidden(&self.h_curr, &self.prev_observed));
        let logits = self
            .pending_logits
            .take()
            .unwrap_or_else(|| self.forward_logits(&h_next));

        let mut total_loss_bits = 0.0f64;
        for (c, &tgt) in norm_truth.iter().enumerate() {
            let offset = c * self.vocab_size;
            let s = (tgt as usize) & (self.vocab_size - 1);
            let (_, ch_loss) =
                softmax_probs_and_loss_bits(&logits[offset..offset + self.vocab_size], s);
            total_loss_bits += ch_loss;
        }

        if self.history.len() >= self.bptt_len
            && let Some((old_in, _)) = self.history.pop_front()
        {
            self.h_window_start = self.forward_hidden(&self.h_window_start, &old_in);
        }
        self.history
            .push_back((self.prev_observed.clone(), norm_truth.clone()));

        let steps = self.history.len().min(self.bptt_len);
        if budget.try_consume(steps as u64) {
            self.apply_bptt_step();
        }

        self.prev_observed = norm_truth;
        self.h_curr = h_next;

        Some(total_loss_bits)
    }

    fn best_circuit(&self) -> Option<&Circuit> {
        None
    }

    fn budget_accounting_mode(&self) -> BudgetAccountingMode {
        BudgetAccountingMode::BpttUnrollSteps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::{FiniteStateGenerator, GeneratorConfig, LfsrStage};

    #[test]
    fn width_guard_zero_biases_and_budget_accounting_mode() {
        let err =
            RnnBaseline::with_defaults(Width::new(16).unwrap(), 2, SeedBank::new(1)).unwrap_err();
        assert_eq!(
            err,
            RnnError::UnsupportedWidth {
                bits: 16,
                max_bits: MAX_RNN_ONE_HOT_WIDTH_BITS,
            }
        );

        let rnn = RnnBaseline::with_defaults(Width::BITS_2, 2, SeedBank::new(1)).unwrap();
        assert_eq!(
            rnn.budget_accounting_mode(),
            BudgetAccountingMode::BpttUnrollSteps
        );
        assert!(rnn.b_h.iter().all(|&b| b == 0.0));
        assert!(rnn.b_y.iter().all(|&b| b == 0.0));
        assert!(rnn.h_curr.iter().all(|&h| h == 0.0));
        assert!(rnn.prev_observed.iter().all(|&y| y == 0));

        // Verify per-tick BPTT unroll consumption: 1, 2, ..., bptt_len.
        let mut metered = RnnBaseline::new(Width::BITS_1, 1, 8, 4, 0.05, SeedBank::new(7)).unwrap();
        let mut budget = TickBudget::new(100);
        let mut expected_total = 0u64;
        for t in 1..=6u64 {
            let _ = metered.predict(&mut budget);
            assert_eq!(
                budget.consumed(),
                expected_total,
                "predict() must consume zero ticks"
            );
            let loss = metered.observe(&[t % 2], &mut budget).unwrap();
            assert!(loss.is_finite());
            expected_total += t.min(4);
            assert_eq!(budget.consumed(), expected_total);
        }
    }

    #[test]
    fn deterministic_initialization_and_solves_alternating_and_delayed_tasks() {
        // Two models with identical SeedBank make identical predictions and losses.
        let mut r1 = RnnBaseline::with_defaults(Width::BITS_2, 2, SeedBank::new(42)).unwrap();
        let mut r2 = RnnBaseline::with_defaults(Width::BITS_2, 2, SeedBank::new(42)).unwrap();
        let mut b1 = TickBudget::new(10_000);
        let mut b2 = TickBudget::new(10_000);
        for t in 0..20u64 {
            assert_eq!(r1.predict(&mut b1), r2.predict(&mut b2));
            let l1 = r1.observe(&[t % 4, (t + 1) % 4], &mut b1).unwrap();
            let l2 = r2.observe(&[t % 4, (t + 1) % 4], &mut b2).unwrap();
            assert_eq!(l1.to_bits(), l2.to_bits());
        }

        // Learns period-2 alternating sequence cleanly.
        let mut alt_gen = GeneratorConfig::T1_Alternating { a: 1, b: 3 }
            .build(Width::BITS_2)
            .unwrap();
        let mut rnn_alt = RnnBaseline::with_defaults(Width::BITS_2, 1, SeedBank::new(9)).unwrap();
        let mut b_alt = TickBudget::new(100_000);
        let mut truth = [0u64];
        let mut alt_tail = 0usize;
        for t in 0..600 {
            let pred = rnn_alt.predict(&mut b_alt);
            alt_gen.next_clean_frame(&mut truth);
            if t >= 400 && pred[0] == truth[0] {
                alt_tail += 1;
            }
            let loss = rnn_alt.observe(&truth, &mut b_alt).unwrap();
            assert!(loss.is_finite());
        }
        assert_eq!(alt_tail, 200);

        // Learns multi-step delayed cross-channel copy (lag = 2) through BPTT hidden state.
        let driver = LfsrStage {
            taps: vec![0, 1, 0, 0, 1],
            init: vec![1, 0, 1, 1, 0],
        };
        let mut gen_del = GeneratorConfig::T2_DelayedAffine {
            driver,
            lag: 2,
            mul_a: 1,
            add_b: 1,
        }
        .build(Width::BITS_2)
        .unwrap();
        let mut rnn_del = RnnBaseline::with_defaults(Width::BITS_2, 2, SeedBank::new(11)).unwrap();
        let mut b_del = TickBudget::new(200_000);
        let mut f_del = [0u64; 2];
        let mut del_tail = 0usize;
        for t in 0..1500 {
            let pred = rnn_del.predict(&mut b_del);
            gen_del.next_clean_frame(&mut f_del);
            if t >= 1200 && pred[1] == f_del[1] {
                del_tail += 1;
            }
            let loss = rnn_del.observe(&f_del, &mut b_del).unwrap();
            assert!(loss.is_finite());
        }
        assert_eq!(
            del_tail, 300,
            "RNN BPTT must solve T2_DelayedAffine(lag=2): got {del_tail}/300"
        );
    }

    #[test]
    fn finite_differences_match_analytical_bptt_gradient_on_step_and_budget_freeze() {
        // Verify exact gradient numerically on 2-step sequence (input -> h1 -> h2 -> loss).
        let mut rnn = RnnBaseline::new(Width::BITS_1, 1, 3, 4, 1.0, SeedBank::new(5)).unwrap();
        let mut dummy_budget = TickBudget::new(100);
        let _ = rnn.predict(&mut dummy_budget);
        let _ = rnn.observe(&[1], &mut dummy_budget);
        // Freeze model copy before t=1 update for analytical vs finite-diff comparison.
        let mut rnn_ana = rnn.clone();
        let _ = rnn_ana.predict(&mut dummy_budget);
        // Capture weights before t=1 update, run 1 step with lr=1.0, check gradient step on w_hy[0].
        let w_hy0_before = rnn_ana.w_hy[0];
        let mut b_step = TickBudget::new(100);
        let _ = rnn_ana.observe(&[0], &mut b_step);
        let w_hy0_after = rnn_ana.w_hy[0];
        let ana_grad_hy0 = w_hy0_before - w_hy0_after; // since lr = 1.0

        // Finite difference check on w_hy[0].
        let eps = 1e-6;
        let eval_loss_at = |delta: f64| -> f64 {
            let mut probe = rnn.clone();
            probe.w_hy[0] += delta;
            let h1 = probe.forward_hidden(&probe.h_window_start, &[0]);
            let h2 = probe.forward_hidden(&h1, &[1]);
            let logits = probe.forward_logits(&h2);
            let (_, loss_bits) = softmax_probs_and_loss_bits(&logits, 0);
            loss_bits * std::f64::consts::LN_2 // analytical grad is in nats
        };
        let num_grad_hy0 = (eval_loss_at(eps) - eval_loss_at(-eps)) / (2.0 * eps);
        assert!(
            (ana_grad_hy0 - num_grad_hy0).abs() < 1e-6,
            "analytical grad {ana_grad_hy0} != finite diff {num_grad_hy0}"
        );

        // Exhausting budget prevents weight updates.
        let mut frozen = RnnBaseline::new(Width::BITS_1, 1, 4, 4, 0.05, SeedBank::new(3)).unwrap();
        let init_whh = frozen.w_hh.clone();
        let init_why = frozen.w_hy.clone();
        let mut zero_budget = TickBudget::new(0);
        let _ = frozen.predict(&mut zero_budget);
        let loss = frozen.observe(&[1], &mut zero_budget).unwrap();
        assert!(loss.is_finite());
        assert_eq!(frozen.w_hh, init_whh);
        assert_eq!(frozen.w_hy, init_why);
        assert_eq!(zero_budget.consumed(), 0);
    }
}
