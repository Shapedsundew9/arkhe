//! Charter §3.1 Structural Layer (M0 subset: 2D lattice geometry & wiring lowering).

pub mod economy;
pub mod growth;
pub mod lattice;
pub mod learner;
pub mod live;
pub mod sensitivity;
pub mod surgery;

pub use economy::{
    EconomyConfig, EconomyError, EconomyState, EconomyTick, NodeEconomy, RewardMode,
};
pub use growth::{
    FeatureScore, GrowthConfig, GrowthError, ResidualDiagnostics, ResidualFeature,
    diagnose_residual,
};
pub use lattice::{
    BoundaryAnchor, Coord, GridShape, LatticeCircuit, LatticeError, PerimeterSide, WiringTopology,
};
pub use learner::ArkheLearner;
pub use live::{
    LiveCircuit, LiveError, LiveTick, node_output_values, well_mixed_description_debts,
    well_mixed_wire_steps,
};
pub use sensitivity::{
    CausalAncestry, NodeActivity, SensitivityError, SensitivitySnapshot, assemble_causal_ancestry,
    counterfactual_sensitivity, ring_identity_input_weights, source_node_for_wire,
};
pub use surgery::{EditableLattice, MarginalDebtLedger, SurgeryError};
