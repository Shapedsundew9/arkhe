//! Charter §2.6, §3.1.4, §3.4 & §8 Yardsticks: surprisal null models, MDL $L$, dynamics, and scorecards.

pub mod dynamics;
pub mod mdl;
pub mod null_model;
pub mod scorecard;

pub use dynamics::{BasinReport, CycleReport, detect_autonomous_cycle, estimate_attractor_basin};
pub use mdl::{
    MdlBreakdown, circuit_code_bits, compute_mdl, elias_gamma_bits, facts_code_bits,
    kt_binary_sequence_bits,
};
pub use null_model::{NullModel, NullModelKind};
pub use scorecard::{BudgetAccountingMode, Scorecard};
