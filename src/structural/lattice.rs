//! 2D discrete spatial lattice geometry and wiring-topology lowering (Charter §3.1.1 & §8).
//!
//! Keeps grid placement, perimeter channel anchors, and local vs. well-mixed wiring rules cleanly
//! separated from `substrate` execution and from future M1a+ metabolic/growth logic.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::substrate::{
    Circuit, InvariantError, NodeId, Primitive, SignalSink, SignalSource, Width, WireDelayMode,
};

/// Dimensions of a 2D spatial lattice (`rows >= 1`, `cols >= 1`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GridShape {
    /// Number of grid rows.
    pub rows: u16,
    /// Number of grid columns.
    pub cols: u16,
}

impl GridShape {
    /// Constructs a validated non-empty [`GridShape`].
    ///
    /// # Errors
    ///
    /// Returns [`LatticeError::EmptyGrid`] if `rows == 0` or `cols == 0`.
    pub const fn new(rows: u16, cols: u16) -> Result<Self, LatticeError> {
        if rows == 0 || cols == 0 {
            Err(LatticeError::EmptyGrid { rows, cols })
        } else {
            Ok(Self { rows, cols })
        }
    }

    /// Returns `true` iff `coord` lies inside `0..rows` × `0..cols`.
    #[must_use]
    pub const fn contains(self, coord: Coord) -> bool {
        coord.row < self.rows && coord.col < self.cols
    }
}

/// Zero-indexed 2D cell coordinate on a [`GridShape`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Coord {
    /// Row index (`0..rows`).
    pub row: u16,
    /// Column index (`0..cols`).
    pub col: u16,
}

impl Coord {
    /// Manhattan ($L_1$) distance $\lvert r_1 - r_2 \rvert + \lvert c_1 - c_2 \rvert$.
    #[must_use]
    pub const fn manhattan_distance(self, other: Self) -> u16 {
        self.row
            .abs_diff(other.row)
            .saturating_add(self.col.abs_diff(other.col))
    }

    /// Chebyshev ($L_\infty$) distance $\max(\lvert r_1 - r_2 \rvert, \lvert c_1 - c_2 \rvert)$.
    #[must_use]
    pub const fn chebyshev_distance(self, other: Self) -> u16 {
        let dr = self.row.abs_diff(other.row);
        let dc = self.col.abs_diff(other.col);
        if dr > dc { dr } else { dc }
    }
}

/// Cardinal perimeter side where an environment input or prediction port enters/exits (§3.1.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PerimeterSide {
    /// Top boundary adjacent to `row == 0` (`offset` in `0..cols`).
    North,
    /// Bottom boundary adjacent to `row == rows - 1` (`offset` in `0..cols`).
    South,
    /// Left boundary adjacent to `col == 0` (`offset` in `0..rows`).
    West,
    /// Right boundary adjacent to `col == cols - 1` (`offset` in `0..rows`).
    East,
}

/// Perimeter attachment site for a boundary input or output port.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BoundaryAnchor {
    /// Perimeter edge.
    pub side: PerimeterSide,
    /// Position along `side` (`0..cols` for North/South, `0..rows` for East/West).
    pub offset: u16,
}

impl BoundaryAnchor {
    /// Validates the anchor against `shape` and returns the immediately adjacent border cell.
    ///
    /// A boundary port sits one step outside this border cell, so connecting a boundary port
    /// to its adjacent border cell has step distance `1`.
    fn adjacent_border_cell(self, shape: GridShape) -> Result<Coord, LatticeError> {
        match self.side {
            PerimeterSide::North => {
                if self.offset >= shape.cols {
                    return Err(LatticeError::InvalidBoundaryAnchor {
                        anchor: self,
                        shape,
                    });
                }
                Ok(Coord {
                    row: 0,
                    col: self.offset,
                })
            }
            PerimeterSide::South => {
                if self.offset >= shape.cols {
                    return Err(LatticeError::InvalidBoundaryAnchor {
                        anchor: self,
                        shape,
                    });
                }
                Ok(Coord {
                    row: shape.rows - 1,
                    col: self.offset,
                })
            }
            PerimeterSide::West => {
                if self.offset >= shape.rows {
                    return Err(LatticeError::InvalidBoundaryAnchor {
                        anchor: self,
                        shape,
                    });
                }
                Ok(Coord {
                    row: self.offset,
                    col: 0,
                })
            }
            PerimeterSide::East => {
                if self.offset >= shape.rows {
                    return Err(LatticeError::InvalidBoundaryAnchor {
                        anchor: self,
                        shape,
                    });
                }
                Ok(Coord {
                    row: self.offset,
                    col: shape.cols - 1,
                })
            }
        }
    }
}

/// Spatial wiring rule (Charter §3.1.1 & §8 *"lattice vs. well-mixed wiring"* ablation).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WiringTopology {
    /// Orthogonal 4-neighbour locality ($L_1 = 1$ in combinational mode; $L_1$ hops in delayed mode).
    LocalVonNeumann,
    /// 8-neighbour locality ($L_\infty = 1$ in combinational mode; $L_\infty$ hops in delayed mode).
    LocalMoore,
    /// Arbitrary non-local wiring permitted across any coordinates (baseline/ablation default).
    #[default]
    WellMixed,
}

/// Geometric or structural error encountered while building or lowering a [`LatticeCircuit`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LatticeError {
    /// Grid had zero rows or zero columns.
    EmptyGrid { rows: u16, cols: u16 },
    /// Cell coordinate lay outside `GridShape`.
    CoordOutOfBounds { coord: Coord, shape: GridShape },
    /// Attempted to place two primitives in the same grid cell.
    CellOccupied { coord: Coord, existing: NodeId },
    /// Boundary anchor offset exceeded the length of its perimeter side.
    InvalidBoundaryAnchor {
        anchor: BoundaryAnchor,
        shape: GridShape,
    },
    /// Referenced an unplaced node or out-of-range boundary port.
    UnknownEndpoint,
    /// Edge exceeded neighbourhood distance allowed by active [`WiringTopology`] + `Combinational` mode.
    NonLocalEdge {
        src: SignalSource,
        dst: SignalSink,
        distance: u16,
        topology: WiringTopology,
    },
    /// Underlying substrate structural invariant failed during compilation.
    Substrate(InvariantError),
}

impl fmt::Display for LatticeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyGrid { rows, cols } => {
                write!(f, "invalid empty grid shape {rows}x{cols}")
            }
            Self::CoordOutOfBounds { coord, shape } => write!(
                f,
                "coordinate ({}, {}) out of bounds for {}x{} grid",
                coord.row, coord.col, shape.rows, shape.cols
            ),
            Self::CellOccupied { coord, existing } => write!(
                f,
                "grid cell ({}, {}) already occupied by node {}",
                coord.row, coord.col, existing.0
            ),
            Self::InvalidBoundaryAnchor { anchor, shape } => write!(
                f,
                "boundary anchor {anchor:?} out of bounds for {}x{} grid",
                shape.rows, shape.cols
            ),
            Self::UnknownEndpoint => write!(f, "edge references unknown lattice endpoint"),
            Self::NonLocalEdge {
                src,
                dst,
                distance,
                topology,
            } => write!(
                f,
                "non-local edge {src:?} -> {dst:?} has distance {distance} under {topology:?}"
            ),
            Self::Substrate(err) => write!(f, "substrate invariant error: {err}"),
        }
    }
}

impl std::error::Error for LatticeError {}

impl From<InvariantError> for LatticeError {
    fn from(value: InvariantError) -> Self {
        Self::Substrate(value)
    }
}

/// Spatially embedded circuit on a 2D discrete lattice.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatticeCircuit {
    shape: GridShape,
    width: Width,
    wire_delay: WireDelayMode,
    topology: WiringTopology,
    input_anchors: Vec<BoundaryAnchor>,
    output_anchors: Vec<BoundaryAnchor>,
    nodes: Vec<(Coord, Primitive)>,
    cell_map: BTreeMap<Coord, NodeId>,
    edges: Vec<(SignalSource, SignalSink)>,
}

impl LatticeCircuit {
    /// Creates an empty [`LatticeCircuit`] with validated perimeter input/output anchors.
    ///
    /// # Errors
    ///
    /// Returns [`LatticeError`] if any boundary anchor is out of range for `shape`.
    pub fn new(
        shape: GridShape,
        width: Width,
        wire_delay: WireDelayMode,
        topology: WiringTopology,
        input_anchors: Vec<BoundaryAnchor>,
        output_anchors: Vec<BoundaryAnchor>,
    ) -> Result<Self, LatticeError> {
        for &anchor in &input_anchors {
            anchor.adjacent_border_cell(shape)?;
        }
        for &anchor in &output_anchors {
            anchor.adjacent_border_cell(shape)?;
        }
        Ok(Self {
            shape,
            width,
            wire_delay,
            topology,
            input_anchors,
            output_anchors,
            nodes: Vec::new(),
            cell_map: BTreeMap::new(),
            edges: Vec::new(),
        })
    }

    /// Grid dimensions.
    #[must_use]
    pub const fn shape(&self) -> GridShape {
        self.shape
    }

    /// Active spatial wiring rule.
    #[must_use]
    pub const fn topology(&self) -> WiringTopology {
        self.topology
    }

    /// Looks up the [`NodeId`] occupying `coord`, if any.
    #[must_use]
    pub fn cell_node(&self, coord: Coord) -> Option<NodeId> {
        self.cell_map.get(&coord).copied()
    }

    /// Looks up the [`Coord`] of `node`, if valid.
    #[must_use]
    pub fn node_coord(&self, node: NodeId) -> Option<Coord> {
        self.nodes.get(node.index()).map(|&(c, _)| c)
    }

    /// Places `primitive` at `coord` and returns its [`NodeId`].
    ///
    /// # Errors
    ///
    /// Returns [`LatticeError::CoordOutOfBounds`] or [`LatticeError::CellOccupied`] on conflict.
    pub fn place_node(
        &mut self,
        coord: Coord,
        primitive: Primitive,
    ) -> Result<NodeId, LatticeError> {
        if !self.shape.contains(coord) {
            return Err(LatticeError::CoordOutOfBounds {
                coord,
                shape: self.shape,
            });
        }
        if let Some(&existing) = self.cell_map.get(&coord) {
            return Err(LatticeError::CellOccupied { coord, existing });
        }
        let id = NodeId(u32::try_from(self.nodes.len()).expect("node count fits in u32"));
        self.nodes.push((coord, primitive.normalized(self.width)));
        self.cell_map.insert(coord, id);
        Ok(id)
    }

    /// Connects `src` to `dst`, validating geometric locality and hop distances.
    ///
    /// # Errors
    ///
    /// Returns [`LatticeError`] if endpoints are unknown or violate [`WiringTopology`] locality.
    pub fn connect(&mut self, src: SignalSource, dst: SignalSink) -> Result<(), LatticeError> {
        let distance = self.step_distance(src, dst)?;
        if self.wire_delay == WireDelayMode::Combinational {
            match self.topology {
                WiringTopology::LocalVonNeumann | WiringTopology::LocalMoore if distance > 1 => {
                    return Err(LatticeError::NonLocalEdge {
                        src,
                        dst,
                        distance,
                        topology: self.topology,
                    });
                }
                _ => {}
            }
        }
        self.edges.push((src, dst));
        Ok(())
    }

    fn step_distance(&self, src: SignalSource, dst: SignalSink) -> Result<u16, LatticeError> {
        let dist_fn = |a: Coord, b: Coord| -> u16 {
            match self.topology {
                WiringTopology::LocalVonNeumann | WiringTopology::WellMixed => {
                    a.manhattan_distance(b)
                }
                WiringTopology::LocalMoore => a.chebyshev_distance(b),
            }
        };

        match (src, dst) {
            (SignalSource::NodeOut { node: u, .. }, SignalSink::NodeIn { node: v, .. }) => {
                let cu = self.node_coord(u).ok_or(LatticeError::UnknownEndpoint)?;
                let cv = self.node_coord(v).ok_or(LatticeError::UnknownEndpoint)?;
                Ok(dist_fn(cu, cv).max(1))
            }
            (SignalSource::BoundaryIn(p), SignalSink::NodeIn { node: v, .. }) => {
                let anchor = *self
                    .input_anchors
                    .get(usize::from(p))
                    .ok_or(LatticeError::UnknownEndpoint)?;
                let border = anchor.adjacent_border_cell(self.shape)?;
                let cv = self.node_coord(v).ok_or(LatticeError::UnknownEndpoint)?;
                Ok(dist_fn(border, cv).saturating_add(1))
            }
            (SignalSource::NodeOut { node: u, .. }, SignalSink::BoundaryOut(p)) => {
                let cu = self.node_coord(u).ok_or(LatticeError::UnknownEndpoint)?;
                let anchor = *self
                    .output_anchors
                    .get(usize::from(p))
                    .ok_or(LatticeError::UnknownEndpoint)?;
                let border = anchor.adjacent_border_cell(self.shape)?;
                Ok(dist_fn(cu, border).saturating_add(1))
            }
            (SignalSource::BoundaryIn(p_in), SignalSink::BoundaryOut(p_out)) => {
                let a_in = *self
                    .input_anchors
                    .get(usize::from(p_in))
                    .ok_or(LatticeError::UnknownEndpoint)?;
                let a_out = *self
                    .output_anchors
                    .get(usize::from(p_out))
                    .ok_or(LatticeError::UnknownEndpoint)?;
                let b_in = a_in.adjacent_border_cell(self.shape)?;
                let b_out = a_out.adjacent_border_cell(self.shape)?;
                Ok(dist_fn(b_in, b_out).saturating_add(2))
            }
        }
    }

    /// Lowers the spatially placed graph into a [`Circuit`] and verifies all §2.5 invariants.
    ///
    /// # Errors
    ///
    /// Returns [`LatticeError`] if geometric or substrate invariants fail.
    pub fn lower(&self) -> Result<Circuit, LatticeError> {
        let num_in = u16::try_from(self.input_anchors.len()).expect("port count fits in u16");
        let num_out = u16::try_from(self.output_anchors.len()).expect("port count fits in u16");
        let mut circuit = Circuit::new(self.width, num_in, num_out, self.wire_delay);
        for &(_, prim) in &self.nodes {
            circuit.add_node(prim);
        }
        for &(src, dst) in &self.edges {
            let hops = match self.wire_delay {
                WireDelayMode::Combinational => 0,
                WireDelayMode::TicksPerHop => match self.topology {
                    WiringTopology::WellMixed => 1,
                    WiringTopology::LocalVonNeumann | WiringTopology::LocalMoore => {
                        self.step_distance(src, dst)?
                    }
                },
            };
            circuit.connect_hops(src, dst, hops);
        }
        circuit.compile()?;
        Ok(circuit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::substrate::ScalarState;

    #[test]
    fn local_von_neumann_enforces_adjacency_in_combinational_mode() {
        let shape = GridShape::new(4, 4).unwrap();
        let mut lat = LatticeCircuit::new(
            shape,
            Width::BITS_1,
            WireDelayMode::Combinational,
            WiringTopology::LocalVonNeumann,
            vec![BoundaryAnchor {
                side: PerimeterSide::West,
                offset: 0,
            }],
            vec![BoundaryAnchor {
                side: PerimeterSide::East,
                offset: 0,
            }],
        )
        .unwrap();

        let n0 = lat
            .place_node(Coord { row: 0, col: 0 }, Primitive::Eqz)
            .unwrap();
        let n_far = lat
            .place_node(Coord { row: 0, col: 2 }, Primitive::Eqz)
            .unwrap();

        // Adjacent border cell (0,0) from West(0) has distance 1 -> allowed!
        lat.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: n0, port: 0 },
        )
        .unwrap();

        // Jump from (0,0) to (0,2) has Manhattan distance 2 -> rejected in LocalVonNeumann Combinational!
        assert!(matches!(
            lat.connect(
                SignalSource::NodeOut { node: n0, port: 0 },
                SignalSink::NodeIn {
                    node: n_far,
                    port: 0
                }
            ),
            Err(LatticeError::NonLocalEdge { distance: 2, .. })
        ));
    }

    #[test]
    fn ticks_per_hop_converts_manhattan_distance_into_pipeline_latency() {
        let shape = GridShape::new(3, 3).unwrap();
        let mut lat = LatticeCircuit::new(
            shape,
            Width::BITS_2,
            WireDelayMode::TicksPerHop,
            WiringTopology::LocalVonNeumann,
            vec![BoundaryAnchor {
                side: PerimeterSide::West,
                offset: 0,
            }],
            vec![BoundaryAnchor {
                side: PerimeterSide::East,
                offset: 0,
            }],
        )
        .unwrap();

        let n = lat
            .place_node(Coord { row: 0, col: 0 }, Primitive::Split)
            .unwrap();
        // West(0) -> (0,0) = 1 hop; (0,0) -> East(0) at border (0,2) = 2 + 1 = 3 hops. Total = 4 ticks!
        lat.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: n, port: 0 },
        )
        .unwrap();
        lat.connect(
            SignalSource::NodeOut { node: n, port: 0 },
            SignalSink::BoundaryOut(0),
        )
        .unwrap();

        let circuit = lat.lower().unwrap();
        let compiled = circuit.compile().unwrap();
        assert_eq!(compiled.total_registers(), 4);

        let mut st = ScalarState::new(&compiled);
        let mut out = [0u64];
        // Feed [3, 2, 0, 0, 0, 0]; with 4 pipeline hops initialised to 0, [3, 2] arrives at t = 4, 5.
        let inputs = [3u64, 2, 0, 0, 0, 0];
        let mut trace = Vec::new();
        for &inp in &inputs {
            st.step(&compiled, &[inp], &mut out).unwrap();
            trace.push(out[0]);
        }
        assert_eq!(trace, vec![0, 0, 0, 0, 3, 2]);
    }
}
