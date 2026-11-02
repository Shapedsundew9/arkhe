//! 2D discrete spatial lattice geometry and wiring-topology lowering (Charter §3.1.1 & §8).
//!
//! Keeps grid placement, perimeter channel anchors, and local vs. well-mixed wiring rules cleanly
//! separated from `substrate` execution and from future M1a+ metabolic/growth logic.

use std::collections::BTreeMap;
use std::collections::{BTreeSet, VecDeque};
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
    /// No path with enough unoccupied relay cells existed within the configured limit.
    RouteUnavailable {
        src: SignalSource,
        dst: SignalSink,
        max_relay_hops: usize,
    },
    /// Underlying substrate structural invariant failed during compilation.
    Substrate(InvariantError),
}

#[derive(Clone, Copy)]
struct RouteRequest {
    src: SignalSource,
    dst: SignalSink,
    max_relay_hops: usize,
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
            Self::RouteUnavailable {
                src,
                dst,
                max_relay_hops,
            } => write!(
                f,
                "no local route for {src:?} -> {dst:?} within {max_relay_hops} relay nodes"
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

    /// Circuit wire width.
    #[must_use]
    pub const fn width(&self) -> Width {
        self.width
    }

    /// Circuit propagation mode.
    #[must_use]
    pub const fn wire_delay(&self) -> WireDelayMode {
        self.wire_delay
    }

    /// Active spatial wiring rule.
    #[must_use]
    pub const fn topology(&self) -> WiringTopology {
        self.topology
    }

    /// Perimeter input anchors in channel order.
    #[must_use]
    pub fn input_anchors(&self) -> &[BoundaryAnchor] {
        &self.input_anchors
    }

    /// Perimeter output anchors in port order.
    #[must_use]
    pub fn output_anchors(&self) -> &[BoundaryAnchor] {
        &self.output_anchors
    }

    /// Placed nodes in stable creation order.
    #[must_use]
    pub fn nodes(&self) -> &[(Coord, Primitive)] {
        &self.nodes
    }

    /// Directed lattice edges.
    #[must_use]
    pub fn edges(&self) -> &[(SignalSource, SignalSink)] {
        &self.edges
    }

    /// Primitive at `node`, if it is currently placed.
    #[must_use]
    pub fn node_primitive(&self, node: NodeId) -> Option<Primitive> {
        self.nodes
            .get(node.index())
            .map(|(_, primitive)| *primitive)
    }

    /// Looks up the [`NodeId`] occupying `coord`, if any.
    #[must_use]
    pub fn cell_node(&self, coord: Coord) -> Option<NodeId> {
        self.cell_map.get(&coord).copied()
    }

    /// Returns per-node wire-hop totals used by metabolic maintenance pricing.
    ///
    /// Incoming node edges and outgoing boundary-output edges are charged to the node. In
    /// `WellMixed` mode each edge costs one hop, independent of coordinates.
    pub fn wire_steps_per_node(&self) -> Result<Vec<u32>, LatticeError> {
        let mut steps = vec![0u32; self.nodes.len()];
        for &(src, dst) in &self.edges {
            let charged_node = match (src, dst) {
                (_, SignalSink::NodeIn { node, .. }) => Some(node),
                (SignalSource::NodeOut { node, .. }, SignalSink::BoundaryOut(_)) => Some(node),
                _ => None,
            };
            if let Some(node) = charged_node {
                let distance = if self.topology == WiringTopology::WellMixed {
                    1
                } else {
                    u32::from(self.step_distance(src, dst)?)
                };
                let Some(total) = steps.get_mut(node.index()) else {
                    return Err(LatticeError::UnknownEndpoint);
                };
                *total = total.saturating_add(distance);
            }
        }
        Ok(steps)
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

    /// Connects endpoints through the shortest deterministic local route.
    ///
    /// Empty intermediate cells are filled with `SPLIT` relay nodes whose second output remains
    /// available for future fan-out. The returned list is ordered from source to destination.
    pub fn route_connect(
        &mut self,
        src: SignalSource,
        dst: SignalSink,
        max_relay_hops: usize,
    ) -> Result<Vec<NodeId>, LatticeError> {
        let mut staged = self.clone();
        let relays = staged.route_connect_in_place(src, dst, max_relay_hops)?;
        *self = staged;
        Ok(relays)
    }

    fn route_connect_in_place(
        &mut self,
        src: SignalSource,
        dst: SignalSink,
        max_relay_hops: usize,
    ) -> Result<Vec<NodeId>, LatticeError> {
        if self.topology == WiringTopology::WellMixed {
            self.connect(src, dst)?;
            return Ok(Vec::new());
        }
        let (start, source_is_boundary) = self.source_route_cell(src)?;
        let (goal, sink_is_boundary) = self.sink_route_cell(dst)?;
        let request = RouteRequest {
            src,
            dst,
            max_relay_hops,
        };
        let path = self.shortest_empty_cell_path(
            start,
            goal,
            max_relay_hops.saturating_add(1),
            !source_is_boundary,
            !sink_is_boundary,
            request,
        )?;
        let relay_coords: Vec<Coord> = path
            .iter()
            .enumerate()
            .filter_map(|(index, &coord)| {
                let is_source_node = index == 0 && !source_is_boundary;
                let is_sink_node = index + 1 == path.len() && !sink_is_boundary;
                (!is_source_node && !is_sink_node).then_some(coord)
            })
            .collect();
        if relay_coords.len() > max_relay_hops {
            return Err(LatticeError::RouteUnavailable {
                src,
                dst,
                max_relay_hops,
            });
        }

        let mut relays = Vec::with_capacity(relay_coords.len());
        for coord in relay_coords {
            relays.push(self.place_node(coord, Primitive::Split)?);
        }
        if relays.is_empty() {
            self.connect(src, dst)?;
            return Ok(relays);
        }
        self.connect(
            src,
            SignalSink::NodeIn {
                node: relays[0],
                port: 0,
            },
        )?;
        for pair in relays.windows(2) {
            self.connect(
                SignalSource::NodeOut {
                    node: pair[0],
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: pair[1],
                    port: 0,
                },
            )?;
        }
        self.connect(
            SignalSource::NodeOut {
                node: *relays.last().expect("non-empty relay route"),
                port: 0,
            },
            dst,
        )?;
        Ok(relays)
    }

    /// Removes edges exactly matching `(src, dst)`.
    pub fn disconnect(&mut self, src: SignalSource, dst: SignalSink) -> bool {
        let original_len = self.edges.len();
        self.edges
            .retain(|&(edge_src, edge_dst)| edge_src != src || edge_dst != dst);
        self.edges.len() != original_len
    }

    /// Changes the source endpoint of an edge with destination `dst`.
    pub fn replace_edge_source(
        &mut self,
        dst: SignalSink,
        src: SignalSource,
    ) -> Result<(), LatticeError> {
        let Some((old_src, _)) = self.edges.iter_mut().find(|(_, edge_dst)| *edge_dst == dst)
        else {
            return Err(LatticeError::UnknownEndpoint);
        };
        *old_src = src;
        Ok(())
    }

    /// Compacts nodes after deletion and returns `old_node -> Some(new_node)` for survivors.
    pub fn remove_nodes(&mut self, removed: &BTreeSet<NodeId>) -> Vec<Option<NodeId>> {
        let mut mapping = vec![None; self.nodes.len()];
        let mut nodes = Vec::with_capacity(self.nodes.len().saturating_sub(removed.len()));
        let mut cell_map = BTreeMap::new();
        for (old_index, &(coord, primitive)) in self.nodes.iter().enumerate() {
            let old = NodeId(u32::try_from(old_index).expect("node index fits in u32"));
            if removed.contains(&old) {
                continue;
            }
            let new = NodeId(u32::try_from(nodes.len()).expect("node count fits in u32"));
            mapping[old_index] = Some(new);
            nodes.push((coord, primitive));
            cell_map.insert(coord, new);
        }
        let remap_source = |source: SignalSource| -> Option<SignalSource> {
            match source {
                SignalSource::BoundaryIn(_) => Some(source),
                SignalSource::NodeOut { node, port } => Some(SignalSource::NodeOut {
                    node: mapping.get(node.index()).copied().flatten()?,
                    port,
                }),
            }
        };
        let remap_sink = |sink: SignalSink| -> Option<SignalSink> {
            match sink {
                SignalSink::BoundaryOut(_) => Some(sink),
                SignalSink::NodeIn { node, port } => Some(SignalSink::NodeIn {
                    node: mapping.get(node.index()).copied().flatten()?,
                    port,
                }),
            }
        };
        self.edges = self
            .edges
            .iter()
            .filter_map(|&(src, dst)| Some((remap_source(src)?, remap_sink(dst)?)))
            .collect();
        self.nodes = nodes;
        self.cell_map = cell_map;
        mapping
    }

    fn source_route_cell(&self, source: SignalSource) -> Result<(Coord, bool), LatticeError> {
        match source {
            SignalSource::BoundaryIn(port) => self
                .input_anchors
                .get(usize::from(port))
                .copied()
                .ok_or(LatticeError::UnknownEndpoint)?
                .adjacent_border_cell(self.shape)
                .map(|coord| (coord, true)),
            SignalSource::NodeOut { node, .. } => self
                .node_coord(node)
                .map(|coord| (coord, false))
                .ok_or(LatticeError::UnknownEndpoint),
        }
    }

    fn sink_route_cell(&self, sink: SignalSink) -> Result<(Coord, bool), LatticeError> {
        match sink {
            SignalSink::BoundaryOut(port) => self
                .output_anchors
                .get(usize::from(port))
                .copied()
                .ok_or(LatticeError::UnknownEndpoint)?
                .adjacent_border_cell(self.shape)
                .map(|coord| (coord, true)),
            SignalSink::NodeIn { node, .. } => self
                .node_coord(node)
                .map(|coord| (coord, false))
                .ok_or(LatticeError::UnknownEndpoint),
        }
    }

    fn shortest_empty_cell_path(
        &self,
        start: Coord,
        goal: Coord,
        max_steps: usize,
        allow_occupied_start: bool,
        allow_occupied_goal: bool,
        request: RouteRequest,
    ) -> Result<Vec<Coord>, LatticeError> {
        let start_is_blocked = self.cell_map.contains_key(&start)
            && !allow_occupied_start
            && !(start == goal && allow_occupied_goal);
        let goal_is_blocked = self.cell_map.contains_key(&goal)
            && !allow_occupied_goal
            && !(start == goal && allow_occupied_start);
        if start_is_blocked || goal_is_blocked {
            return Err(LatticeError::RouteUnavailable {
                src: request.src,
                dst: request.dst,
                max_relay_hops: request.max_relay_hops,
            });
        }
        let mut parents = BTreeMap::new();
        let mut queue = VecDeque::new();
        parents.insert(start, None);
        queue.push_back((start, 0usize));
        while let Some((current, depth)) = queue.pop_front() {
            if current == goal {
                let mut path = vec![goal];
                let mut cursor = goal;
                while let Some(Some(parent)) = parents.get(&cursor) {
                    path.push(*parent);
                    cursor = *parent;
                }
                path.reverse();
                return Ok(path);
            }
            if depth >= max_steps {
                continue;
            }
            for next in self.neighbors(current) {
                if parents.contains_key(&next)
                    || (self.cell_map.contains_key(&next)
                        && !(next == goal && allow_occupied_goal)
                        && !(next == start && allow_occupied_start))
                {
                    continue;
                }
                parents.insert(next, Some(current));
                queue.push_back((next, depth + 1));
            }
        }
        Err(LatticeError::RouteUnavailable {
            src: request.src,
            dst: request.dst,
            max_relay_hops: request.max_relay_hops,
        })
    }

    fn neighbors(&self, coord: Coord) -> Vec<Coord> {
        let mut neighbors = Vec::with_capacity(8);
        for dr in -1i32..=1 {
            for dc in -1i32..=1 {
                if dr == 0 && dc == 0 {
                    continue;
                }
                if self.topology == WiringTopology::LocalVonNeumann && dr != 0 && dc != 0 {
                    continue;
                }
                let row = i32::from(coord.row) + dr;
                let col = i32::from(coord.col) + dc;
                if row >= 0
                    && col >= 0
                    && row < i32::from(self.shape.rows)
                    && col < i32::from(self.shape.cols)
                {
                    neighbors.push(Coord {
                        row: u16::try_from(row).expect("row is in bounds"),
                        col: u16::try_from(col).expect("column is in bounds"),
                    });
                }
            }
        }
        neighbors.sort_unstable();
        neighbors
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

    #[test]
    fn route_connect_uses_deterministic_local_paths_for_both_topologies() {
        let shape = GridShape::new(2, 2).unwrap();
        let input = BoundaryAnchor {
            side: PerimeterSide::West,
            offset: 0,
        };
        let output = BoundaryAnchor {
            side: PerimeterSide::East,
            offset: 1,
        };
        for (topology, expected_relays) in [
            (WiringTopology::LocalVonNeumann, 3),
            (WiringTopology::LocalMoore, 2),
        ] {
            let mut lattice = LatticeCircuit::new(
                shape,
                Width::BITS_1,
                WireDelayMode::Combinational,
                topology,
                vec![input],
                vec![output],
            )
            .unwrap();
            let src = SignalSource::BoundaryIn(0);
            let dst = SignalSink::BoundaryOut(0);
            let original = lattice.clone();
            let relays = lattice.route_connect(src, dst, 4).unwrap();
            let mut repeated = original;
            let repeated_relays = repeated.route_connect(src, dst, 4).unwrap();

            assert_eq!(relays.len(), expected_relays);
            assert_eq!(relays, repeated_relays);
            for &(edge_src, edge_dst) in lattice.edges() {
                assert_eq!(lattice.step_distance(edge_src, edge_dst).unwrap(), 1);
            }
            assert!(lattice.lower().is_ok());
        }
    }

    #[test]
    fn route_connect_respects_relay_limit_and_leaves_lattice_unchanged() {
        let mut lattice = LatticeCircuit::new(
            GridShape::new(3, 3).unwrap(),
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
        let before = lattice.clone();

        assert!(matches!(
            lattice.route_connect(SignalSource::BoundaryIn(0), SignalSink::BoundaryOut(0), 0),
            Err(LatticeError::RouteUnavailable {
                src: SignalSource::BoundaryIn(0),
                dst: SignalSink::BoundaryOut(0),
                max_relay_hops: 0,
            })
        ));
        assert_eq!(lattice, before);
    }

    #[test]
    fn route_connect_reports_blocked_boundary_cells_without_partial_mutation() {
        let mut lattice = LatticeCircuit::new(
            GridShape::new(1, 1).unwrap(),
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
        let blocker = lattice
            .place_node(Coord { row: 0, col: 0 }, Primitive::Eqz)
            .unwrap();
        let before = lattice.clone();

        assert!(matches!(
            lattice.route_connect(SignalSource::BoundaryIn(0), SignalSink::BoundaryOut(0), 4),
            Err(LatticeError::RouteUnavailable {
                src: SignalSource::BoundaryIn(0),
                dst: SignalSink::BoundaryOut(0),
                ..
            })
        ));
        assert_eq!(lattice, before);
        assert_eq!(lattice.cell_node(Coord { row: 0, col: 0 }), Some(blocker));
    }

    #[test]
    fn route_connect_routes_around_occupied_cells_and_preserves_wire_hop_latency() {
        let mut lattice = LatticeCircuit::new(
            GridShape::new(3, 3).unwrap(),
            Width::BITS_1,
            WireDelayMode::TicksPerHop,
            WiringTopology::LocalVonNeumann,
            vec![
                BoundaryAnchor {
                    side: PerimeterSide::West,
                    offset: 1,
                },
                BoundaryAnchor {
                    side: PerimeterSide::North,
                    offset: 1,
                },
            ],
            vec![],
        )
        .unwrap();
        let source = lattice
            .place_node(Coord { row: 1, col: 0 }, Primitive::Split)
            .unwrap();
        let blocker = lattice
            .place_node(Coord { row: 1, col: 1 }, Primitive::Eqz)
            .unwrap();
        let target = lattice
            .place_node(Coord { row: 1, col: 2 }, Primitive::Eqz)
            .unwrap();
        lattice
            .connect(
                SignalSource::BoundaryIn(0),
                SignalSink::NodeIn {
                    node: source,
                    port: 0,
                },
            )
            .unwrap();
        lattice
            .connect(
                SignalSource::BoundaryIn(1),
                SignalSink::NodeIn {
                    node: blocker,
                    port: 0,
                },
            )
            .unwrap();
        let relays = lattice
            .route_connect(
                SignalSource::NodeOut {
                    node: source,
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: target,
                    port: 0,
                },
                3,
            )
            .unwrap();

        assert_eq!(relays.len(), 3);
        assert!(!relays.contains(&blocker));
        let compiled = lattice.lower().unwrap().compile().unwrap();
        assert_eq!(compiled.total_registers(), 7);
    }
}
