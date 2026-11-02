//! Bit-decomposed Context-Tree Weighting (CTW) universal sequence predictor (Charter §8).
//!
//! Implements Willems, Shtarkov & Tjalkens (1995) in exact base-2 log-probability domain (preventing
//! floating-point underflow at arbitrary horizons), extended to general $k$-bit symbols via
//! Tjalkens–Volf–Willems bit-plane decomposition.

use crate::baselines::{PredictorBaseline, TickBudget};
use crate::metrics::BudgetAccountingMode;
use crate::substrate::{Circuit, Width};

#[inline]
fn log2_sum_exp(a: f64, b: f64) -> f64 {
    let m = a.max(b);
    if !m.is_finite() {
        return m;
    }
    m + (1.0 + 2.0f64.powf(a.min(b) - m)).log2()
}

#[derive(Clone, Debug, Default)]
struct CtwNode {
    count_zero: u64,
    count_one: u64,
    log2_pe: f64,
    log2_pw: f64,
    child_zero: Option<usize>,
    child_one: Option<usize>,
}

impl CtwNode {
    #[inline]
    fn kt_log2_step_prob(&self, bit: u8) -> f64 {
        let a = self.count_zero as f64;
        let b = self.count_one as f64;
        let num = if bit == 0 { a + 0.5 } else { b + 0.5 };
        (num / (a + b + 1.0)).log2()
    }
}

#[derive(Clone, Debug)]
struct BinaryCtwTree {
    depth: usize,
    nodes: Vec<CtwNode>,
}

impl BinaryCtwTree {
    fn new(depth: usize) -> Self {
        Self {
            depth,
            nodes: vec![CtwNode::default()],
        }
    }

    /// Computes exact predictive probability $\Pr(B = 1 \mid \text{context})$ in `(0.0, 1.0)`
    /// without mutating the arena.
    fn predict_prob_one(&self, context_recent_first: &[u8]) -> f64 {
        let log2_pw_before = self.nodes[0].log2_pw;
        let log2_pw_after_one = self.hypothetical_root_log2_pw(0, 0, context_recent_first, 1);
        (2.0f64.powf(log2_pw_after_one - log2_pw_before)).clamp(1e-15, 1.0 - 1e-15)
    }

    fn hypothetical_root_log2_pw(
        &self,
        node_idx: usize,
        level: usize,
        context_recent_first: &[u8],
        new_bit: u8,
    ) -> f64 {
        let node = &self.nodes[node_idx];
        let new_log2_pe = node.log2_pe + node.kt_log2_step_prob(new_bit);
        if level == self.depth {
            return new_log2_pe;
        }

        let ctx_bit = context_recent_first.get(level).copied().unwrap_or(0);
        let (c0_pw, c1_pw) = if ctx_bit == 0 {
            let updated_c0 = match node.child_zero {
                Some(c0) => {
                    self.hypothetical_root_log2_pw(c0, level + 1, context_recent_first, new_bit)
                }
                None => self.hypothetical_fresh_branch_log2_pw(level + 1, new_bit),
            };
            let existing_c1 = node.child_one.map_or(0.0, |c1| self.nodes[c1].log2_pw);
            (updated_c0, existing_c1)
        } else {
            let existing_c0 = node.child_zero.map_or(0.0, |c0| self.nodes[c0].log2_pw);
            let updated_c1 = match node.child_one {
                Some(c1) => {
                    self.hypothetical_root_log2_pw(c1, level + 1, context_recent_first, new_bit)
                }
                None => self.hypothetical_fresh_branch_log2_pw(level + 1, new_bit),
            };
            (existing_c0, updated_c1)
        };

        -1.0 + log2_sum_exp(new_log2_pe, c0_pw + c1_pw)
    }

    /// On an unvisited branch (`count_zero == 0, count_one == 0`), a single observed bit has
    /// KT probability `0.5 / 1.0 == 0.5` (`log2_pe == -1.0`) at every depth down to the leaf,
    /// and by induction `-1 + log2(2^-1 + 2^-1 * 2^0) = -1.0` at every ancestor too!
    #[inline]
    fn hypothetical_fresh_branch_log2_pw(&self, _level: usize, _new_bit: u8) -> f64 {
        -1.0
    }

    /// Updates counts and weighted log-probabilities along `context_recent_first` with `actual_bit`.
    fn update(&mut self, context_recent_first: &[u8], actual_bit: u8) {
        self.update_rec(0, 0, context_recent_first, actual_bit);
    }

    fn update_rec(
        &mut self,
        node_idx: usize,
        level: usize,
        context_recent_first: &[u8],
        actual_bit: u8,
    ) -> f64 {
        let step_log2 = self.nodes[node_idx].kt_log2_step_prob(actual_bit);
        self.nodes[node_idx].log2_pe += step_log2;
        if actual_bit == 0 {
            self.nodes[node_idx].count_zero += 1;
        } else {
            self.nodes[node_idx].count_one += 1;
        }

        if level == self.depth {
            let pe = self.nodes[node_idx].log2_pe;
            self.nodes[node_idx].log2_pw = pe;
            return pe;
        }

        let ctx_bit = context_recent_first.get(level).copied().unwrap_or(0);
        let child_idx = if ctx_bit == 0 {
            if let Some(c0) = self.nodes[node_idx].child_zero {
                c0
            } else {
                let fresh = self.nodes.len();
                self.nodes.push(CtwNode::default());
                self.nodes[node_idx].child_zero = Some(fresh);
                fresh
            }
        } else if let Some(c1) = self.nodes[node_idx].child_one {
            c1
        } else {
            let fresh = self.nodes.len();
            self.nodes.push(CtwNode::default());
            self.nodes[node_idx].child_one = Some(fresh);
            fresh
        };

        self.update_rec(child_idx, level + 1, context_recent_first, actual_bit);

        let c0_pw = self.nodes[node_idx]
            .child_zero
            .map_or(0.0, |c| self.nodes[c].log2_pw);
        let c1_pw = self.nodes[node_idx]
            .child_one
            .map_or(0.0, |c| self.nodes[c].log2_pw);
        let pe = self.nodes[node_idx].log2_pe;
        let pw = -1.0 + log2_sum_exp(pe, c0_pw + c1_pw);
        self.nodes[node_idx].log2_pw = pw;
        pw
    }
}

#[derive(Clone, Debug)]
struct ChannelCtw {
    width: Width,
    bit_depth: usize,
    bit_trees: Vec<BinaryCtwTree>,
    /// Most-recent-bit-first history buffer (`bit_history[0]` is the latest observed bit).
    bit_history: Vec<u8>,
}

impl ChannelCtw {
    fn new(width: Width, symbol_context_depth: usize) -> Self {
        let k = usize::from(width.bits());
        let bit_depth = symbol_context_depth.saturating_mul(k).clamp(4, 64);
        let bit_trees = (0..k).map(|_| BinaryCtwTree::new(bit_depth)).collect();
        Self {
            width,
            bit_depth,
            bit_trees,
            bit_history: Vec::with_capacity(bit_depth + k),
        }
    }

    fn make_context(&self, prefix_bits_msb_first: &[u8]) -> Vec<u8> {
        let mut ctx = Vec::with_capacity(self.bit_depth);
        for &b in prefix_bits_msb_first.iter().rev() {
            if ctx.len() == self.bit_depth {
                break;
            }
            ctx.push(b);
        }
        for &b in &self.bit_history {
            if ctx.len() == self.bit_depth {
                break;
            }
            ctx.push(b);
        }
        ctx
    }

    /// Exact probability of `candidate_sym` under the $k$ chained bit-plane CTW trees.
    fn symbol_prob(&self, candidate_sym: u64) -> f64 {
        let k = usize::from(self.width.bits());
        let mut prefix = Vec::with_capacity(k);
        let mut prob = 1.0f64;
        for plane in 0..k {
            let shift = k - 1 - plane;
            let bit = ((candidate_sym >> shift) & 1) as u8;
            let ctx = self.make_context(&prefix);
            let p1 = self.bit_trees[plane].predict_prob_one(&ctx);
            prob *= if bit == 1 { p1 } else { 1.0 - p1 };
            prefix.push(bit);
        }
        prob
    }

    /// Predicts the MAP symbol $\arg\max_x q_t(x)$ (exact over all $2^k$ symbols for $k \le 8$,
    /// bit-greedy MSB-to-LSB for $k > 8$).
    fn predict_modal_symbol(&self) -> u64 {
        let k = self.width.bits();
        if k <= 8 {
            let card = 1u64 << k;
            let mut best_sym = 0u64;
            let mut best_p = -1.0f64;
            for sym in 0..card {
                let p = self.symbol_prob(sym);
                if p > best_p {
                    best_p = p;
                    best_sym = sym;
                }
            }
            best_sym
        } else {
            let k_usize = usize::from(k);
            let mut prefix = Vec::with_capacity(k_usize);
            let mut sym = 0u64;
            for plane in 0..k_usize {
                let ctx = self.make_context(&prefix);
                let p1 = self.bit_trees[plane].predict_prob_one(&ctx);
                let bit = u8::from(p1 >= 0.5);
                sym = (sym << 1) | u64::from(bit);
                prefix.push(bit);
            }
            sym & self.width.mask()
        }
    }

    /// Computes exact prequential log-loss $-\log_2 q_t(\text{truth})$ in bits and updates all $k$ bit-trees.
    fn observe_and_loss(&mut self, raw_truth: u64) -> f64 {
        let sym = self.width.normalize(raw_truth);
        let k = usize::from(self.width.bits());
        let mut prefix = Vec::with_capacity(k);
        let mut loss_bits = 0.0f64;

        for plane in 0..k {
            let shift = k - 1 - plane;
            let bit = ((sym >> shift) & 1) as u8;
            let ctx = self.make_context(&prefix);
            let p1 = self.bit_trees[plane].predict_prob_one(&ctx);
            let p_actual = if bit == 1 { p1 } else { 1.0 - p1 };
            loss_bits += -p_actual.log2();
            self.bit_trees[plane].update(&ctx, bit);
            prefix.push(bit);
        }

        for bit in prefix {
            self.bit_history.insert(0, bit);
            if self.bit_history.len() > self.bit_depth {
                self.bit_history.pop();
            }
        }

        loss_bits
    }
}

/// Multi-channel Context-Tree Weighting universal reference predictor.
pub struct CtwBaseline {
    channels: Vec<ChannelCtw>,
}

impl CtwBaseline {
    /// Creates a [`CtwBaseline`] with Markov symbol context depth `context_depth` per channel.
    #[must_use]
    pub fn new(width: Width, num_channels: u16, context_depth: usize) -> Self {
        let channels = (0..num_channels)
            .map(|_| ChannelCtw::new(width, context_depth))
            .collect();
        Self { channels }
    }
}

impl PredictorBaseline for CtwBaseline {
    fn name(&self) -> &str {
        "CTW"
    }

    fn predict(&mut self, _budget: &mut TickBudget) -> Vec<u64> {
        self.channels
            .iter()
            .map(ChannelCtw::predict_modal_symbol)
            .collect()
    }

    fn observe(&mut self, truth: &[u64], _budget: &mut TickBudget) -> Option<f64> {
        let mut total_loss_bits = 0.0f64;
        for (ch_ctw, &sym) in self.channels.iter_mut().zip(truth) {
            total_loss_bits += ch_ctw.observe_and_loss(sym);
        }
        Some(total_loss_bits)
    }

    fn best_circuit(&self) -> Option<&Circuit> {
        None
    }

    fn budget_accounting_mode(&self) -> BudgetAccountingMode {
        BudgetAccountingMode::AnalyticalUnmetered
    }
}
