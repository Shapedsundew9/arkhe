//! Charter §2.6, §3.1.4, §3.4 & §8 Yardsticks: surprisal null models, MDL $L$, dynamics, and scorecards.

pub mod dynamics;
pub mod mdl;
pub mod null_model;
pub mod point_forecast;
pub mod scorecard;

pub use dynamics::{BasinReport, CycleReport, detect_autonomous_cycle, estimate_attractor_basin};
pub use mdl::{
    CircuitDescriptionAllocation, MdlBreakdown, NullCalibratedMdlError,
    allocate_circuit_description_bits, circuit_code_bits, circuit_code_bits_with_tax,
    circuit_node_description_debts, compute_mdl, compute_mdl_with_tax, elias_gamma_bits,
    facts_code_bits, kt_binary_sequence_bits, null_calibrated_mdl_delta,
};
pub use null_model::{NullModel, NullModelKind};
pub use point_forecast::PointForecastScorer;
pub use scorecard::{BudgetAccountingMode, Scorecard};
