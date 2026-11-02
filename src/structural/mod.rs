//! Charter §3.1 Structural Layer (M0 subset: 2D lattice geometry & wiring lowering).

pub mod lattice;

pub use lattice::{
    BoundaryAnchor, Coord, GridShape, LatticeCircuit, LatticeError, PerimeterSide, WiringTopology,
};
