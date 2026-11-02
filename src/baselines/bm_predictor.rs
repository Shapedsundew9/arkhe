//! Online prequential Berlekamp–Massey / Reeds–Sloane deductive baseline over $\mathbb{Z}_{2^k}$ (§8).

use crate::baselines::{PredictorBaseline, TickBudget};
use crate::env::ReedsSloaneSynthesizer;
use crate::metrics::BudgetAccountingMode;
use crate::substrate::{Circuit, Width};

/// Per-channel online Reeds–Sloane linear recurrence predictor.
pub struct ReedsSloaneBaseline {
    synthesizers: Vec<ReedsSloaneSynthesizer>,
}

impl ReedsSloaneBaseline {
    /// Creates an online [`ReedsSloaneBaseline`] across `num_channels` of width `width`.
    #[must_use]
    pub fn new(width: Width, num_channels: u16) -> Self {
        let synthesizers = (0..num_channels)
            .map(|_| ReedsSloaneSynthesizer::new(width))
            .collect();
        Self { synthesizers }
    }

    /// Minimal recurrence lengths currently inferred on each channel.
    #[must_use]
    pub fn minimal_lengths(&self) -> Vec<usize> {
        self.synthesizers
            .iter()
            .map(ReedsSloaneSynthesizer::minimal_length)
            .collect()
    }
}

impl PredictorBaseline for ReedsSloaneBaseline {
    fn name(&self) -> &str {
        "ReedsSloane(BM)"
    }

    fn predict(&mut self, _budget: &mut TickBudget) -> Vec<u64> {
        self.synthesizers
            .iter()
            .map(ReedsSloaneSynthesizer::predict_next)
            .collect()
    }

    fn observe(&mut self, truth: &[u64], _budget: &mut TickBudget) -> Option<f64> {
        for (synth, &sym) in self.synthesizers.iter_mut().zip(truth) {
            synth.push_symbol(sym);
        }
        None
    }

    fn best_circuit(&self) -> Option<&Circuit> {
        None
    }

    fn budget_accounting_mode(&self) -> BudgetAccountingMode {
        BudgetAccountingMode::AnalyticalUnmetered
    }
}
