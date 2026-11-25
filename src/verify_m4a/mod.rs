//! Milestone M4a verification, oracle instrumentation (`R8`), planted spurious injections
//! (`PL1`, `PL2`), and M4a-0 exploratory task registry (`SP`, `PL`, `CT`).
//!
//! Strictly enforces Principle 5 / `R8`: oracle labels in [`oracle`] are external instrumentation
//! only and are never imported or read by `crate::structural`.

pub mod explore;
pub mod oracle;
pub mod planted;
pub mod tasks;

pub use explore::{
    ArmDistributionSummary, ExploreM4aReport, GuardrailAggregateSummary, H6PairedContrast,
    InvariantAggregateSummary, K2OpportunitySummary, PairedDeltaStats, PerturbedDreamLaneSummary,
    RuntimeExtrapolation, ScalarStats, SleepArmSummary, SleepTelemetryTotals,
    SleepWindowSensitivityRow, SpOpportunityRow, check_explore_m4a_seed_range_holdout,
    compute_recoverable_comparator_bits, run_explore_m4a,
};
pub use oracle::{
    CircuitOracleLabels, ComparatorOracleLabel, DEFAULT_MAX_LOOPBACK_ATTRACTOR_STEPS,
    LoopbackAttractorReport, PortLanguageOracle, SleepOracleAudit, audit_sleep_segment_oracle,
    certify_derivation_against_spec, detect_loopback_attractor, extract_derivation_circuit,
    label_circuit_oracle, label_learner_oracle,
};
pub use planted::{
    PlantedInjectionReceipt, inject_pl1_short_cycle_alt, inject_pl2_rare_context_alt,
};
pub use tasks::{
    DEFAULT_M4A_FAULT_TICK, DEFAULT_M4A_HORIZON, DEFAULT_M4A_NOISE_EPSILON, DEFAULT_M4A_PL_HORIZON,
    M4aRegime, M4aTaskFamily, M4aTaskKind, M4aTaskSpec, PlantedSpec, m4a0_task_registry,
};
