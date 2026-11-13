//! Charter §3.1 Structural Layer (Coordinate-free dynamic graph development).

pub mod economy;
pub mod graph_edit;
pub mod growth;
pub mod learner;
pub mod live;
pub mod sensitivity;
pub mod surgery;

pub use economy::{
    EconomyConfig, EconomyError, EconomyState, EconomyTick, NodeEconomy,
    RESERVE_MISMATCH_DENOM_FLOOR, ReserveCap, RewardMode,
};
pub use graph_edit::{
    build_autonomous_ring, materialize_const_loop, stack_output_binary_op, tap_source,
};
pub use growth::{
    AutonomousRingProposal, FeatureScore, GrowthConfig, GrowthError, MDL_TIE_BREAK_EPS,
    RING_SIM_SENSITIVITY_SAMPLES, ResidualDiagnostics, ResidualFeature, SMALL_CONST_CANDIDATE_CAP,
    SproutTelemetry, diagnose_residual, observation_horizon,
    propose_autonomous_delay_ring_with_telemetry,
};
pub use learner::{ArkheLearner, GrowthEpochSnapshot};
pub use live::{
    LiveCircuit, LiveError, LiveTick, node_output_values, well_mixed_description_debts,
    well_mixed_wire_steps,
};
pub use sensitivity::{
    CausalAncestry, EXACT_SENSITIVITY_MAX_BITS, NodeActivity, SensitivityError,
    SensitivitySnapshot, assemble_causal_ancestry, counterfactual_sensitivity,
    ring_identity_input_weights, source_node_for_wire,
};
pub use surgery::{EditableCircuit, MarginalDebtLedger, SurgeryError};
