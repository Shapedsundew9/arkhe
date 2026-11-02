//! Unified experiment scorecard comparing predictor performance against exact ground truth (§8).

use serde::{Deserialize, Serialize};

use crate::metrics::mdl::MdlBreakdown;

/// Declares how a baseline accounts for [`crate::baselines::TickBudget`] consumption (§8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BudgetAccountingMode {
    /// Candidate substrate circuits are replayed over buffered history windows within `TickBudget`.
    CandidateHistoryReplay,
    /// Analytical or Bayesian online baseline with zero candidate-replay tick deductions.
    AnalyticalUnmetered,
}

/// Standardised evaluation metrics emitted for every baseline or learner run (Charter §8).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scorecard {
    /// Total environment ticks stepped (`0..horizon_ticks`).
    pub horizon_ticks: u64,
    /// Initial transient warmup ticks excluded from asymptotic error averages.
    pub warmup_ticks: u64,
    /// Post-warmup ticks scored (`horizon_ticks - warmup_ticks`).
    pub evaluated_ticks: u64,
    /// Number of post-warmup symbol mismatches against observed (possibly noisy) frames.
    pub post_transient_observed_mismatches: u64,
    /// Number of post-warmup symbol mismatches against clean ground-truth frames.
    pub post_transient_clean_mismatches: u64,
    /// Fraction of post-warmup symbols matching observed emissions (`[0.0, 1.0]`).
    pub post_transient_observed_match_rate: f64,
    /// Fraction of post-warmup symbols matching clean deterministic ground truth (`[0.0, 1.0]`).
    pub post_transient_clean_match_rate: f64,
    /// Native prequential log-loss in bits/symbol (`Some` for distributional predictors such as `CTW`; `None` for point predictors).
    pub native_prob_log_loss_bits_per_symbol: Option<f64>,
    /// Sequential Krichevsky–Trofimov point-prediction residual code length in bits/symbol (computed identically for all baselines).
    pub prequential_kt_residual_bits_per_symbol: f64,
    /// `native_prob_log_loss_bits_per_symbol - h_mu_bits_per_symbol` (`None` if non-distributional or $h_\mu$ unavailable).
    pub excess_prob_bits_over_h_mu: Option<f64>,
    /// `prequential_kt_residual_bits_per_symbol - h_mu_bits_per_symbol` (`None` if $h_\mu$ unavailable).
    pub excess_residual_bits_over_h_mu: Option<f64>,
    /// Realised loss (bits/symbol) of the exact clean-state Bayes channel oracle on the identical noise trajectory (`None` if unavailable).
    pub realized_oracle_bits_per_symbol: Option<f64>,
    /// `native_prob_log_loss_bits_per_symbol - realized_oracle_bits_per_symbol` (`None` if unavailable).
    pub paired_prob_regret_bits_per_symbol: Option<f64>,
    /// `prequential_kt_residual_bits_per_symbol - realized_oracle_bits_per_symbol` (`None` if unavailable).
    pub paired_residual_regret_bits_per_symbol: Option<f64>,
    /// Primary paired regret (`paired_prob_regret_bits_per_symbol` for distributional baselines, else `paired_residual_regret_bits_per_symbol`).
    pub paired_regret_bits_per_symbol: Option<f64>,
    /// Mean circular discrepancy $\min(d, 2^k - d)$ per symbol on the post-warmup window.
    pub mean_circular_discrepancy: f64,
    /// Earliest tick $t^*$ from which every subsequent prediction matched the clean generator (`None` if never locked).
    pub ticks_to_clean_solution: Option<u64>,
    /// Internal circuit primitive count (if the predictor synthesises a [`crate::substrate::Circuit`]).
    pub node_count: Option<usize>,
    /// Explicit internal `DELAY` primitive count (if circuit-producing).
    pub delay_count: Option<usize>,
    /// Pipeline wire-hop register count (`0` in `Combinational` mode; sum of edge hops in `TicksPerHop` mode).
    pub wire_hop_register_count: Option<usize>,
    /// Number of `BoundaryIn(c)` channels with a directed path to any output or state register (harness 1-tick input lag).
    pub boundary_lag_register_count: Option<usize>,
    /// Total causal state registers (`delay_count + wire_hop_register_count + boundary_lag_register_count`).
    pub effective_state_registers: Option<usize>,
    /// Internal substrate state bits `(delay_count + wire_hop_register_count) * k` (excluding boundary input lag).
    pub internal_state_bits: Option<usize>,
    /// Effective causal state excess `(effective_state_registers * k) - C_mu` (`>= 0.0` for all valid noiseless predictors).
    pub effective_excess_state_bits_over_c_mu: Option<f64>,
    /// `node_count - exact_synthesis_min_nodes` (when both are available).
    pub excess_nodes_over_exact_synthesis: Option<isize>,
    /// Total description length breakdown (if circuit-producing).
    pub mdl: Option<MdlBreakdown>,
    /// Declares whether `candidate_replay_ticks_consumed` meters candidate replay or an unmetered analytical baseline.
    pub budget_accounting_mode: BudgetAccountingMode,
    /// Total candidate-replay evaluation ticks deducted from [`crate::baselines::TickBudget`].
    pub candidate_replay_ticks_consumed: u64,
}
