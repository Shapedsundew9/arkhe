//! Calibrated sequential point-forecast reward scoring over a prequential null model (D2).
//!
//! A deterministic point prediction $\hat{s} \in \mathbb{Z}_{2^k}$ is treated as a probability forecast
//! placing probability mass $q$ on $\hat{s}$, with remaining mass $1 - q$ distributed across alternative
//! symbols in proportion to the running null model $p_{\text{null}}$.
//!
//! With KT prior $\hat{q} = \frac{M + 1/2}{t + 1/2 \cdot 2^k}$ and non-inversion floor $q = \max(\hat{q}, p)$,
//! the per-tick information gain is:
//!
//! $$r = \begin{cases} \log_2 \frac{q}{p} & \text{if } \hat{s} = s, \\ \log_2 \frac{1 - q}{1 - p} & \text{otherwise.} \end{cases}$$

use crate::metrics::null_model::{NullModel, NullModelKind};
use crate::substrate::Width;

/// Online sequential point-forecast scorer computing bits saved over a prequential null model.
#[derive(Clone, Debug)]
pub struct PointForecastScorer {
    width: Width,
    null_model: NullModel,
    t: u64,
    matches: u64,
    rep_t: u64,
    rep_matches: u64,
    chg_t: u64,
    chg_matches: u64,
}

impl PointForecastScorer {
    /// Creates a fresh [`PointForecastScorer`] for symbols of `width` using `null_model_kind`.
    #[must_use]
    pub const fn new(width: Width, null_model_kind: NullModelKind) -> Self {
        Self {
            width,
            null_model: NullModel::new(width, null_model_kind),
            t: 0,
            matches: 0,
            rep_t: 0,
            rep_matches: 0,
            chg_t: 0,
            chg_matches: 0,
        }
    }

    /// Returns the uniform symbol bit-width.
    #[must_use]
    pub const fn width(&self) -> Width {
        self.width
    }

    /// Returns a reference to the underlying [`NullModel`].
    #[must_use]
    pub const fn null_model(&self) -> &NullModel {
        &self.null_model
    }

    /// Returns a mutable reference to the underlying [`NullModel`].
    #[must_use]
    pub fn null_model_mut(&mut self) -> &mut NullModel {
        &mut self.null_model
    }

    /// Number of ticks evaluated so far.
    #[must_use]
    pub const fn ticks(&self) -> u64 {
        self.t
    }

    /// Number of earlier ticks on which the prediction was exactly right ($M$).
    #[must_use]
    pub const fn matches(&self) -> u64 {
        self.matches
    }

    /// Resets running statistics and the underlying null model.
    pub fn reset(&mut self) {
        self.null_model.reset();
        self.reset_predictor_counts();
    }

    /// Resets running predictor accuracy statistics while preserving the underlying null model.
    pub fn reset_predictor_counts(&mut self) {
        self.t = 0;
        self.matches = 0;
        self.rep_t = 0;
        self.rep_matches = 0;
        self.chg_t = 0;
        self.chg_matches = 0;
    }

    /// Prequential forecast probability $\hat{q} = \frac{M + 1/2}{t + 1/2 \cdot 2^k}$ before observing $s$.
    #[must_use]
    pub fn q_hat(&self) -> f64 {
        let k = self.width.bits();
        let card_f64 = 2.0f64.powi(i32::from(k));
        ((self.matches as f64) + 0.5) / ((self.t as f64) + 0.5 * card_f64)
    }

    /// Prequential forecast probability $\hat{q}$ for candidate prediction `raw_prediction`.
    #[must_use]
    pub fn q_hat_for(&self, raw_prediction: u64) -> f64 {
        let s_hat = self.width.normalize(raw_prediction);
        let k = self.width.bits();
        let card_f64 = 2.0f64.powi(i32::from(k));
        match self.null_model.kind() {
            NullModelKind::LaplaceFrequency => self.q_hat(),
            NullModelKind::RepeatLast => match self.null_model.last_symbol() {
                None => 1.0 / card_f64,
                Some(prev) => {
                    if s_hat == prev {
                        ((self.rep_matches as f64) + 0.5) / ((self.rep_t as f64) + 1.0)
                    } else {
                        let alternatives = if k == 64 {
                            card_f64
                        } else {
                            ((1u64 << k) - 1).max(1) as f64
                        };
                        ((self.chg_matches as f64) + 0.5 / alternatives)
                            / ((self.chg_t as f64) + 1.0)
                    }
                }
            },
        }
    }

    /// Scores prediction `raw_prediction` ($\hat{s}$) against fact `raw_fact` ($s$) in bits,
    /// advancing internal state ($t$, $M$, and the null model).
    pub fn observe(&mut self, raw_prediction: u64, raw_fact: u64) -> f64 {
        let s_hat = self.width.normalize(raw_prediction);
        let s = self.width.normalize(raw_fact);

        let p = self.null_model.probability(s_hat);
        let q_hat = self.q_hat_for(s_hat);
        let q = q_hat.max(p);

        let r = if q <= p {
            0.0
        } else if s_hat == s {
            (q / p).log2()
        } else {
            ((1.0 - q) / (1.0 - p)).log2()
        };

        if let NullModelKind::RepeatLast = self.null_model.kind()
            && let Some(prev) = self.null_model.last_symbol()
        {
            if s_hat == prev {
                self.rep_t += 1;
                if s_hat == s {
                    self.rep_matches += 1;
                }
            } else {
                self.chg_t += 1;
                if s_hat == s {
                    self.chg_matches += 1;
                }
            }
        }

        if s_hat == s {
            self.matches += 1;
        }
        self.t += 1;
        self.null_model.observe_and_surprisal(raw_fact);

        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_chacha::ChaCha8Rng;
    use rand_core::{Rng, SeedableRng};

    #[test]
    fn constant_predictor_scores_exact_zero_on_random_streams() {
        for k in [1u8, 2, 4, 8] {
            let width = Width::new(k).unwrap();
            let mut rng = ChaCha8Rng::seed_from_u64(42 + u64::from(k));
            let mask = (1u64 << k) - 1;

            for constant in [0u64, 1u64, mask] {
                let constant = width.normalize(constant);
                let mut scorer = PointForecastScorer::new(width, NullModelKind::LaplaceFrequency);
                let mut cumulative_flux = 0.0f64;

                for _ in 0..500 {
                    let fact = rng.next_u64() & mask;
                    let r = scorer.observe(constant, fact);
                    assert_eq!(
                        r, 0.0,
                        "k={k}, constant={constant}: r must be exactly 0.0, got {r}"
                    );
                    cumulative_flux += r;
                }
                assert_eq!(
                    cumulative_flux, 0.0,
                    "k={k}, constant={constant}: cumulative flux must be exactly 0.0"
                );
            }
        }
    }

    #[test]
    fn at_k1_q_hat_equals_kt_binary_estimator() {
        let width = Width::BITS_1;
        let mut scorer = PointForecastScorer::new(width, NullModelKind::LaplaceFrequency);
        let facts = [1, 1, 0, 1, 0, 0, 1, 1];
        let preds = [1, 0, 0, 1, 1, 0, 1, 0];

        let mut expected_matches = 0u64;
        for (t, (&p, &f)) in preds.iter().zip(&facts).enumerate() {
            let t_u64 = t as u64;
            let expected_q_hat = ((expected_matches as f64) + 0.5) / ((t_u64 as f64) + 1.0);
            assert!(
                (scorer.q_hat() - expected_q_hat).abs() < 1e-12,
                "tick {t}: q_hat {} != expected {}",
                scorer.q_hat(),
                expected_q_hat
            );
            if p == f {
                expected_matches += 1;
            }
            let _ = scorer.observe(p, f);
        }
    }

    #[test]
    fn predictor_never_better_than_null_scores_exact_zero() {
        let width = Width::BITS_1;
        let mut scorer = PointForecastScorer::new(width, NullModelKind::LaplaceFrequency);
        // An anti-correlated predictor on an alternating stream 0, 1, 0, 1...
        // Stream: 0, 1, 0, 1, 0, 1...
        // Predictor: 1, 0, 1, 0, 1, 0... (100% wrong!)
        for t in 0..200 {
            let fact = (t % 2) as u64;
            let pred = (1 - (t % 2)) as u64;
            let r = scorer.observe(pred, fact);
            assert_eq!(
                r, 0.0,
                "tick {t}: inverted predictor must earn exactly 0.0, got {r}"
            );
        }
        assert_eq!(scorer.matches(), 0);
    }

    #[test]
    fn repeat_last_matched_estimator_scores_zero_for_repeat_and_constant_predictors() {
        for k in [1u8, 2, 4] {
            let width = Width::new(k).unwrap();
            let mut rng = ChaCha8Rng::seed_from_u64(1337 + u64::from(k));
            let mask = (1u64 << k) - 1;

            // 1. Repeat-last predictor on random stream earns exactly 0.0 every tick
            let mut repeat_scorer = PointForecastScorer::new(width, NullModelKind::RepeatLast);
            let mut last_sym = 0u64;
            for t in 0..500 {
                let fact = rng.next_u64() & mask;
                let pred = if t == 0 { 0 } else { last_sym };
                let r = repeat_scorer.observe(pred, fact);
                assert_eq!(
                    r, 0.0,
                    "k={k}, t={t}: repeat-last predictor must score 0.0, got {r}"
                );
                last_sym = fact;
            }

            // 2. Any constant predictor on any constant stream earns exactly 0.0 every tick
            for stream_const in [0u64, 1, mask] {
                for pred_const in [0u64, 1, mask] {
                    let mut const_scorer =
                        PointForecastScorer::new(width, NullModelKind::RepeatLast);
                    for t in 0..200 {
                        let r = const_scorer.observe(pred_const, stream_const);
                        assert_eq!(
                            r, 0.0,
                            "k={k}, t={t}, stream={stream_const}, pred={pred_const}: const predictor must score 0.0, got {r}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn martingale_property_exact_expectation_under_null_model() {
        for k in [1u8, 2] {
            let width = Width::new(k).unwrap();
            let card = 1usize << k;
            for kind in [NullModelKind::LaplaceFrequency, NullModelKind::RepeatLast] {
                let mut scorer = PointForecastScorer::new(width, kind);
                // Warm up with a few observations
                scorer.observe(0, 0);
                scorer.observe(1, 1);
                scorer.observe(0, (card - 1) as u64);

                // For any candidate prediction, check that sum_s p(s) * 2^r(s) == 1.0
                for pred in 0..card as u64 {
                    let mut expected_e = 0.0;
                    for fact in 0..card as u64 {
                        let mut test_scorer = scorer.clone();
                        let p = test_scorer.null_model().probability(fact);
                        let r = test_scorer.observe(pred, fact);
                        expected_e += p * 2.0f64.powf(r);
                    }
                    assert!(
                        (expected_e - 1.0).abs() < 1e-12,
                        "k={k}, kind={kind:?}, pred={pred}: expected 1.0, got {expected_e}"
                    );
                }
            }
        }
    }
}
