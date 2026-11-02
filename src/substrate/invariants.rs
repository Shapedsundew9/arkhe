//! Hard structural invariants (Charter §2.5) and topological schedule construction.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::substrate::circuit::{Circuit, NodeId, SignalSink, SignalSource, WireDelayMode};
use crate::substrate::eval::{CombOp, CompiledCircuit, RegInit, ShiftChain};
use crate::substrate::primitive::Primitive;

/// Typed structural invariant violation returned when compiling an invalid [`Circuit`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InvariantError {
    /// Referenced a `BoundaryIn` index `>= num_inputs`.
    InvalidBoundaryInput { port: u16, num_inputs: u16 },
    /// Referenced a `BoundaryOut` index `>= num_outputs`.
    InvalidBoundaryOutput { port: u16, num_outputs: u16 },
    /// Referenced a `NodeId` outside `0..nodes.len()`.
    InvalidNode { node: NodeId, node_count: usize },
    /// Referenced an input slot `>= primitive.input_arity()`.
    InvalidNodeInputPort { node: NodeId, port: u8, arity: u8 },
    /// Referenced an output slot `>= primitive.output_arity()`.
    InvalidNodeOutputPort { node: NodeId, port: u8, arity: u8 },
    /// Edge had `hops == 0` under [`WireDelayMode::TicksPerHop`] (§2.5.3).
    ZeroHopInDelayedMode { edge_index: usize },
    /// More than one driver connected to the same input slot or boundary output (§2.5.4).
    MultipleDrivers { sink: SignalSink, count: usize },
    /// Active primitive input slot has no driving edge (§2.5.4).
    UnconnectedInput { node: NodeId, port: u8 },
    /// Source port fans out to more than one sink without `SPLIT` (§2.5.5).
    IllegalFanOut { source: SignalSource, count: usize },
    /// Combinational cycle without an intervening `DELAY` or delayed hop (§2.5.2).
    CombinationalLoop { participants: Vec<NodeId> },
}

impl fmt::Display for InvariantError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBoundaryInput { port, num_inputs } => {
                write!(f, "boundary input {port} out of range 0..{num_inputs}")
            }
            Self::InvalidBoundaryOutput { port, num_outputs } => {
                write!(f, "boundary output {port} out of range 0..{num_outputs}")
            }
            Self::InvalidNode { node, node_count } => {
                write!(f, "node {} out of range 0..{node_count}", node.0)
            }
            Self::InvalidNodeInputPort { node, port, arity } => {
                write!(
                    f,
                    "node {} input port {port} out of range 0..{arity}",
                    node.0
                )
            }
            Self::InvalidNodeOutputPort { node, port, arity } => {
                write!(
                    f,
                    "node {} output port {port} out of range 0..{arity}",
                    node.0
                )
            }
            Self::ZeroHopInDelayedMode { edge_index } => {
                write!(
                    f,
                    "edge {edge_index} has hops == 0 in TicksPerHop wire-delay mode"
                )
            }
            Self::MultipleDrivers { sink, count } => {
                write!(f, "sink {sink:?} driven by {count} edges (expected <= 1)")
            }
            Self::UnconnectedInput { node, port } => {
                write!(f, "node {} input port {port} is unconnected", node.0)
            }
            Self::IllegalFanOut { source, count } => {
                write!(
                    f,
                    "source {source:?} drives {count} sinks without SPLIT (expected <= 1)"
                )
            }
            Self::CombinationalLoop { participants } => {
                write!(f, "combinational cycle detected across {participants:?}")
            }
        }
    }
}

impl std::error::Error for InvariantError {}

impl Circuit {
    /// Validates all Charter §2.5 structural invariants and compiles into a flat schedule.
    ///
    /// # Errors
    ///
    /// Returns [`InvariantError`] if any port bound, single-driver, `SPLIT`-only fan-out,
    /// hop-delay, or combinational acyclicity invariant is violated.
    pub fn compile(&self) -> Result<CompiledCircuit, InvariantError> {
        let nodes = self.nodes();
        let edges = self.edges();

        // 1. Validate port bounds & hop counts, and count source fan-out / sink drivers.
        let mut source_counts: BTreeMap<SignalSource, usize> = BTreeMap::new();
        let mut sink_drivers: BTreeMap<SignalSink, usize> = BTreeMap::new();
        let mut sink_edge_idx: BTreeMap<SignalSink, usize> = BTreeMap::new();

        for (edge_index, edge) in edges.iter().enumerate() {
            match edge.src {
                SignalSource::BoundaryIn(p) => {
                    if p >= self.num_inputs() {
                        return Err(InvariantError::InvalidBoundaryInput {
                            port: p,
                            num_inputs: self.num_inputs(),
                        });
                    }
                }
                SignalSource::NodeOut { node, port } => {
                    let Some(&prim) = nodes.get(node.index()) else {
                        return Err(InvariantError::InvalidNode {
                            node,
                            node_count: nodes.len(),
                        });
                    };
                    let arity = prim.output_arity();
                    if port >= arity {
                        return Err(InvariantError::InvalidNodeOutputPort { node, port, arity });
                    }
                }
            }

            match edge.dst {
                SignalSink::BoundaryOut(p) => {
                    if p >= self.num_outputs() {
                        return Err(InvariantError::InvalidBoundaryOutput {
                            port: p,
                            num_outputs: self.num_outputs(),
                        });
                    }
                }
                SignalSink::NodeIn { node, port } => {
                    let Some(&prim) = nodes.get(node.index()) else {
                        return Err(InvariantError::InvalidNode {
                            node,
                            node_count: nodes.len(),
                        });
                    };
                    let arity = prim.input_arity();
                    if port >= arity {
                        return Err(InvariantError::InvalidNodeInputPort { node, port, arity });
                    }
                }
            }

            if self.wire_delay() == WireDelayMode::TicksPerHop && edge.hops == 0 {
                return Err(InvariantError::ZeroHopInDelayedMode { edge_index });
            }

            *source_counts.entry(edge.src).or_insert(0) += 1;
            *sink_drivers.entry(edge.dst).or_insert(0) += 1;
            sink_edge_idx.insert(edge.dst, edge_index);
        }

        // 2. Enforce single driver per sink (§2.5.4).
        for (&sink, &count) in &sink_drivers {
            if count > 1 {
                return Err(InvariantError::MultipleDrivers { sink, count });
            }
        }

        // 3. Enforce every active node input port has a driver (§2.5.4).
        for (idx, &prim) in nodes.iter().enumerate() {
            let node = NodeId(u32::try_from(idx).expect("node index fits in u32"));
            for port in 0..prim.input_arity() {
                if !sink_drivers.contains_key(&SignalSink::NodeIn { node, port }) {
                    return Err(InvariantError::UnconnectedInput { node, port });
                }
            }
        }

        // 4. Enforce SPLIT-only fan-out (§2.5.5).
        for (&source, &count) in &source_counts {
            if count > 1 {
                return Err(InvariantError::IllegalFanOut { source, count });
            }
        }

        // 5. Assign contiguous wire indices:
        //    - [0..num_inputs]: BoundaryIn(0..num_inputs)
        //    - NodeOut slots for every node
        //    - Delayed-edge output wires (in TicksPerHop mode)
        let mut next_wire: usize = usize::from(self.num_inputs());
        let mut node_out_base: Vec<usize> = Vec::with_capacity(nodes.len());
        for &prim in nodes {
            node_out_base.push(next_wire);
            next_wire += usize::from(prim.output_arity());
        }

        let source_wire = |src: SignalSource| -> usize {
            match src {
                SignalSource::BoundaryIn(p) => usize::from(p),
                SignalSource::NodeOut { node, port } => {
                    node_out_base[node.index()] + usize::from(port)
                }
            }
        };

        let mut reg_inits: Vec<RegInit> = Vec::new();
        let mut delay_reads: Vec<(usize, usize)> = Vec::new(); // (reg_idx, wire_idx)
        let mut delay_latches: Vec<(usize, usize)> = Vec::new(); // (reg_idx, src_wire_idx)
        let mut shift_chains: Vec<ShiftChain> = Vec::new();
        let mut edge_delivered_wire: Vec<usize> = Vec::with_capacity(edges.len());

        // Allocate explicit Delay node registers first (in deterministic NodeId order).
        let mut delay_reg_of_node: BTreeMap<NodeId, usize> = BTreeMap::new();
        for (idx, &prim) in nodes.iter().enumerate() {
            if let Primitive::Delay { init } = prim {
                let node = NodeId(u32::try_from(idx).expect("node index fits in u32"));
                let reg_idx = reg_inits.len();
                reg_inits.push(RegInit {
                    initial_value: self.width().normalize(init),
                    is_explicit_delay: true,
                });
                let out_wire = node_out_base[idx];
                delay_reads.push((reg_idx, out_wire));
                delay_reg_of_node.insert(node, reg_idx);
            }
        }

        // Allocate TicksPerHop wire pipeline registers if enabled.
        for edge in edges {
            if self.wire_delay() == WireDelayMode::TicksPerHop {
                let hops = usize::from(edge.hops);
                let base_reg = reg_inits.len();
                for _ in 0..hops {
                    reg_inits.push(RegInit {
                        initial_value: 0,
                        is_explicit_delay: false,
                    });
                }
                let delivered_wire = next_wire;
                next_wire += 1;
                let tail_reg = base_reg + hops - 1;
                delay_reads.push((tail_reg, delivered_wire));
                shift_chains.push(ShiftChain {
                    src_wire: source_wire(edge.src),
                    base_reg,
                    hops,
                });
                edge_delivered_wire.push(delivered_wire);
            } else {
                edge_delivered_wire.push(source_wire(edge.src));
            }
        }

        let sink_wire = |sink: SignalSink| -> Option<usize> {
            sink_edge_idx
                .get(&sink)
                .map(|&e_idx| edge_delivered_wire[e_idx])
        };

        // 6. Topological sort of combinational nodes (Kahn's algorithm, §2.5.2).
        let mut comb_in_degree: Vec<usize> = vec![0; nodes.len()];
        let mut comb_dependents: Vec<Vec<NodeId>> = vec![Vec::new(); nodes.len()];
        let mut total_comb_nodes: usize = 0;

        for (idx, &prim) in nodes.iter().enumerate() {
            if prim.is_stateful() {
                continue;
            }
            total_comb_nodes += 1;
            let dst_node = NodeId(u32::try_from(idx).expect("node index fits in u32"));

            if self.wire_delay() == WireDelayMode::Combinational {
                for port in 0..prim.input_arity() {
                    let e_idx = sink_edge_idx[&SignalSink::NodeIn {
                        node: dst_node,
                        port,
                    }];
                    if let SignalSource::NodeOut { node: src_node, .. } = edges[e_idx].src
                        && !nodes[src_node.index()].is_stateful()
                    {
                        comb_in_degree[idx] += 1;
                        comb_dependents[src_node.index()].push(dst_node);
                    }
                }
            }
        }

        let mut ready: BTreeSet<NodeId> = BTreeSet::new();
        for (idx, &prim) in nodes.iter().enumerate() {
            if !prim.is_stateful() && comb_in_degree[idx] == 0 {
                ready.insert(NodeId(u32::try_from(idx).expect("node index fits in u32")));
            }
        }

        let mut topo_comb_nodes: Vec<NodeId> = Vec::with_capacity(total_comb_nodes);
        while let Some(node) = ready.pop_first() {
            topo_comb_nodes.push(node);
            for &dep in &comb_dependents[node.index()] {
                let d_idx = dep.index();
                comb_in_degree[d_idx] -= 1;
                if comb_in_degree[d_idx] == 0 {
                    ready.insert(dep);
                }
            }
        }

        if topo_comb_nodes.len() != total_comb_nodes {
            let participants: Vec<NodeId> = (0..nodes.len())
                .filter(|&i| !nodes[i].is_stateful() && comb_in_degree[i] > 0)
                .map(|i| NodeId(u32::try_from(i).expect("node index fits in u32")))
                .collect();
            return Err(InvariantError::CombinationalLoop { participants });
        }

        // 7. Emit flat CombOp instructions in topological order.
        let mut comb_ops: Vec<CombOp> = Vec::with_capacity(total_comb_nodes);
        for node in topo_comb_nodes {
            let idx = node.index();
            let prim = nodes[idx];
            let out_base = node_out_base[idx];
            let in0 = sink_wire(SignalSink::NodeIn { node, port: 0 }).expect("validated connected");
            let op = match prim {
                Primitive::Add => {
                    let in1 = sink_wire(SignalSink::NodeIn { node, port: 1 })
                        .expect("validated connected");
                    CombOp::Add {
                        lhs: in0,
                        rhs: in1,
                        out: out_base,
                    }
                }
                Primitive::Sub => {
                    let in1 = sink_wire(SignalSink::NodeIn { node, port: 1 })
                        .expect("validated connected");
                    CombOp::Sub {
                        lhs: in0,
                        rhs: in1,
                        out: out_base,
                    }
                }
                Primitive::Mul => {
                    let in1 = sink_wire(SignalSink::NodeIn { node, port: 1 })
                        .expect("validated connected");
                    CombOp::Mul {
                        lhs: in0,
                        rhs: in1,
                        out: out_base,
                    }
                }
                Primitive::Eqz => CombOp::Eqz {
                    arg: in0,
                    out: out_base,
                },
                Primitive::Split => CombOp::Split {
                    arg: in0,
                    out0: out_base,
                    out1: out_base + 1,
                },
                Primitive::Delay { .. } => unreachable!("stateful nodes filtered out"),
            };
            comb_ops.push(op);
        }

        // 8. Wire explicit Delay register latches.
        for (&node, &reg_idx) in &delay_reg_of_node {
            let in_wire =
                sink_wire(SignalSink::NodeIn { node, port: 0 }).expect("validated connected");
            delay_latches.push((reg_idx, in_wire));
        }

        // 9. Wire boundary output drivers.
        let mut output_drivers: Vec<Option<usize>> =
            Vec::with_capacity(usize::from(self.num_outputs()));
        for p in 0..self.num_outputs() {
            output_drivers.push(sink_wire(SignalSink::BoundaryOut(p)));
        }

        let active_boundary_inputs =
            u16::try_from(self.active_boundary_input_count()).expect("port count fits in u16");

        Ok(CompiledCircuit::new(
            self.width(),
            self.num_inputs(),
            self.num_outputs(),
            active_boundary_inputs,
            next_wire,
            reg_inits,
            delay_reads,
            comb_ops,
            delay_latches,
            shift_chains,
            output_drivers,
        ))
    }
}
