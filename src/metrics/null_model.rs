//! Cheap local null models and surprisal $-\log_2 p_{\text{null}}(s)$ (Charter §1.2 & §3.1.4).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::substrate::Width;

/// Selector for local null models used in surprisal weighting (§3.1.4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NullModelKind {
    /// Krichevsky–Trofimov ($\alpha = 1/2$) running symbol frequency over $\mathbb{Z}_{2^k}$.
    #[default]
    LaplaceFrequency,
    /// Smoothed first-order persistence model ("repeat the last symbol").
    RepeatLast,
}

/// Online prequential null model computing surprisal in bits before updating counts.
#[derive(Clone, Debug)]
pub struct NullModel {
    width: Width,
    kind: NullModelKind,
    steps: u64,
    counts: BTreeMap<u64, u64>,
    last_symbol: Option<u64>,
    repeat_matches: u64,
}

impl NullModel {
    /// Creates a fresh [`NullModel`] of `kind` for symbols of `width`.
    #[must_use]
    pub const fn new(width: Width, kind: NullModelKind) -> Self {
        Self {
            width,
            kind,
            steps: 0,
            counts: BTreeMap::new(),
            last_symbol: None,
            repeat_matches: 0,
        }
    }

    /// Resets running statistics.
    pub fn reset(&mut self) {
        self.steps = 0;
        self.counts.clear();
        self.last_symbol = None;
        self.repeat_matches = 0;
    }

    /// Model kind used by this estimator.
    #[must_use]
    pub const fn kind(&self) -> NullModelKind {
        self.kind
    }

    /// Last observed symbol, if any.
    #[must_use]
    pub const fn last_symbol(&self) -> Option<u64> {
        self.last_symbol
    }

    /// Predictive null probability $p_{\text{null}}(s)$ prior to observing `raw_symbol`.
    #[must_use]
    pub fn probability(&self, raw_symbol: u64) -> f64 {
        let sym = self.width.normalize(raw_symbol);
        let k = self.width.bits();
        let card_f64 = 2.0f64.powi(i32::from(k));

        match self.kind {
            NullModelKind::LaplaceFrequency => {
                let count = self.counts.get(&sym).copied().unwrap_or(0) as f64;
                // KT prior: (N_x + 0.5) / (t + 0.5 * 2^k).
                let denom = (self.steps as f64) + 0.5 * card_f64;
                (count + 0.5) / denom
            }
            NullModelKind::RepeatLast => {
                let Some(prev) = self.last_symbol else {
                    return 1.0 / card_f64;
                };
                // steps >= 1 transitions observed after tick 0; transitions = steps - 1.
                let transitions = (self.steps - 1) as f64;
                let p_repeat = ((self.repeat_matches as f64) + 0.5) / (transitions + 1.0);
                if sym == prev {
                    p_repeat
                } else {
                    let alternatives = if k == 64 {
                        card_f64
                    } else {
                        ((1u64 << k) - 1) as f64
                    };
                    (1.0 - p_repeat) / alternatives
                }
            }
        }
    }

    /// Prequential surprisal $-\log_2 p_{\text{null}}(s)$ (in bits), followed by updating statistics with `s`.
    pub fn observe_and_surprisal(&mut self, raw_symbol: u64) -> f64 {
        let sym = self.width.normalize(raw_symbol);
        let p = self.probability(sym);
        let surprisal = -p.log2();

        if let Some(prev) = self.last_symbol
            && sym == prev
        {
            self.repeat_matches += 1;
        }
        self.last_symbol = Some(sym);
        *self.counts.entry(sym).or_insert(0) += 1;
        self.steps += 1;

        surprisal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_and_constant_streams_drive_surprisal_toward_zero() {
        let mut freq_null = NullModel::new(Width::BITS_2, NullModelKind::LaplaceFrequency);
        let mut rep_null = NullModel::new(Width::BITS_2, NullModelKind::RepeatLast);

        let mut last_freq = 0.0;
        let mut last_rep = 0.0;
        for _ in 0..500 {
            last_freq = freq_null.observe_and_surprisal(0);
            last_rep = rep_null.observe_and_surprisal(0);
        }
        assert!(last_freq < 0.01, "expected ~0 surprisal, got {last_freq}");
        assert!(last_rep < 0.01, "expected ~0 surprisal, got {last_rep}");
    }
}
