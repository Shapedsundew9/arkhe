//! Core finite-state environment generator traits and state-table export (Charter §5.1–§5.2).

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::substrate::Width;

/// Default upper bound on exported explicit state tables for exact $\varepsilon$-machine analysis.
pub const DEFAULT_MAX_EXPORT_STATES: usize = 65_536;

/// Scope of an exported [`FiniteStateSpec`] and its derived [`crate::env::EpsilonMachineSummary`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum EpsilonMachineScope {
    /// Stationary finite-state generator whose rules stay fixed across the entire run.
    #[default]
    StationaryProcess,
    /// Stationary analysis of only the final target stage of a non-stationary curriculum schedule.
    CurriculumFinalStage {
        /// Zero-based index of the exported final stage.
        stage_index: usize,
        /// Total number of stages in the curriculum schedule.
        total_stages: usize,
        /// Configured ticks spent in each earlier stage before advancing.
        ticks_per_stage: u64,
    },
}

/// Error returned when a generator's state space exceeds the explicit table export bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpecExportError {
    /// Generator has `estimated_states > limit` (e.g., wide `k` LFSR where BM/Reeds–Sloane applies instead).
    StateSpaceTooLarge {
        estimated_states: u128,
        limit: usize,
    },
}

impl fmt::Display for SpecExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StateSpaceTooLarge {
                estimated_states,
                limit,
            } => write!(
                f,
                "generator state space ({estimated_states}) exceeds explicit export limit ({limit})"
            ),
        }
    }
}

impl std::error::Error for SpecExportError {}

/// Explicit deterministic finite-state machine table exported for exact $\varepsilon$-machine ground truth.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FiniteStateSpec {
    /// Uniform symbol bit-width $k$.
    pub width: Width,
    /// Number of parallel output channels $K$.
    pub channels: u16,
    /// Number of internal states $|S|$.
    pub num_states: usize,
    /// Starting internal state $s_0 \in 0..\text{num\_states}$.
    pub initial_state: usize,
    /// For each state `s`, `(next_state, emitted_frame[0..channels])`.
    pub transitions: Vec<(usize, Vec<u64>)>,
    /// Whether this table describes the entire stationary process or a curriculum's final stage.
    pub scope: EpsilonMachineScope,
}

/// Deterministic multi-channel environment generator over $\mathbb{Z}_{2^k}$.
pub trait FiniteStateGenerator {
    /// Human-readable identifier for reports and scorecards.
    fn name(&self) -> &str;

    /// Symbol width $k$.
    fn width(&self) -> Width;

    /// Number of parallel boundary channels $K$.
    fn channels(&self) -> u16;

    /// Resets internal state back to the deterministic starting configuration.
    fn reset(&mut self);

    /// Emits the clean ground-truth frame for the current tick into `out` and advances one tick.
    fn next_clean_frame(&mut self, out: &mut [u64]);

    /// Exports an explicit finite-state transition table when $|S| \le \text{DEFAULT\_MAX\_EXPORT\_STATES}$.
    ///
    /// # Errors
    ///
    /// Returns [`SpecExportError::StateSpaceTooLarge`] when $|S|$ exceeds the table limit.
    fn to_finite_state_spec(&self) -> Result<FiniteStateSpec, SpecExportError>;
}
