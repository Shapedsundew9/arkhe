//! Online, budgeted finite-ring search with residual-structure diagnostics.

use crate::baselines::{PredictorBaseline, RandomSearchBaseline, TickBudget};
use crate::rng::SeedBank;
use crate::structural::growth::{
    GrowthConfig, GrowthError, ResidualDiagnostics, ResidualFeature, diagnose_residual,
};
use crate::substrate::{Circuit, Width, WireDelayMode};

/// Online point predictor that couples budgeted circuit search with residual diagnostics.
pub struct ArkheLearner {
    search: RandomSearchBaseline,
    width: Width,
    growth: GrowthConfig,
    last_prediction: Vec<u64>,
    previous_frame: Vec<u64>,
    residual_history: Vec<u64>,
    input_history: Vec<u64>,
    diagnostics: Option<ResidualDiagnostics>,
}

impl ArkheLearner {
    /// Creates a deterministic learner for the configured stream and node budget.
    pub fn new(
        width: Width,
        channels: u16,
        wire_delay: WireDelayMode,
        growth: GrowthConfig,
        seeds: SeedBank,
    ) -> Result<Self, GrowthError> {
        if !growth.is_valid() {
            return Err(GrowthError::InvalidConfig);
        }
        let channels_usize = usize::from(channels);
        Ok(Self {
            search: RandomSearchBaseline::new(
                width,
                channels,
                wire_delay,
                growth.max_nodes,
                growth.diagnostic_window.max(24),
                32,
                seeds,
            ),
            width,
            growth,
            last_prediction: vec![0; channels_usize],
            previous_frame: vec![0; channels_usize],
            residual_history: Vec::new(),
            input_history: Vec::new(),
            diagnostics: None,
        })
    }

    /// Most recent residual diagnosis, if enough observations have been collected.
    #[must_use]
    pub fn diagnostics(&self) -> Option<&ResidualDiagnostics> {
        self.diagnostics.as_ref()
    }

    fn update_diagnostics(&mut self) {
        let window = self.growth.diagnostic_window;
        if self.residual_history.len() < 2 {
            return;
        }
        let start = self.residual_history.len().saturating_sub(window);
        let residuals = &self.residual_history[start..];
        let feature = ResidualFeature {
            name: "previous-observation".to_owned(),
            values: self.input_history[start..].to_vec(),
        };
        self.diagnostics = Some(
            diagnose_residual(self.width, residuals, &[feature], self.growth)
                .expect("validated learner configuration and aligned residual features"),
        );
    }
}

impl PredictorBaseline for ArkheLearner {
    fn name(&self) -> &str {
        "ArkheM1"
    }

    fn predict(&mut self, budget: &mut TickBudget) -> Vec<u64> {
        let predictions = self.search.predict(budget);
        self.last_prediction.clone_from(&predictions);
        predictions
    }

    fn observe(&mut self, truth: &[u64], budget: &mut TickBudget) -> Option<f64> {
        if let (Some(&fact), Some(&prediction), Some(&previous)) = (
            truth.first(),
            self.last_prediction.first(),
            self.previous_frame.first(),
        ) {
            self.residual_history.push(self.width.sub(fact, prediction));
            self.input_history.push(previous);
            if self.residual_history.len() > self.growth.diagnostic_window {
                self.residual_history.remove(0);
                self.input_history.remove(0);
            }
        }
        for (previous, &value) in self.previous_frame.iter_mut().zip(truth) {
            *previous = self.width.normalize(value);
        }
        self.update_diagnostics();

        self.search.observe(truth, budget)
    }

    fn best_circuit(&self) -> Option<&Circuit> {
        self.search.best_circuit()
    }
}
