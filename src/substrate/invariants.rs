//! Hard structural invariants (Charter §2.5) and topological schedule construction.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
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
        let num_inputs = usize::from(self.num_inputs());
        let num_outputs = usize::from(self.num_outputs());

        // Precompute contiguous wire/slot bases matching `SignalSource` and `SignalSink` Ord order.
        let mut next_wire = num_inputs;
        let mut node_out_base: Vec<usize> = Vec::with_capacity(nodes.len());
        let mut total_sink_slots = num_outputs;
        let mut node_in_base: Vec<usize> = Vec::with_capacity(nodes.len());
        for &prim in nodes {
            node_out_base.push(next_wire);
            next_wire += usize::from(prim.output_arity());
            node_in_base.push(total_sink_slots);
            total_sink_slots += usize::from(prim.input_arity());
        }
        let total_source_slots = next_wire;

        // 1. Validate port bounds & hop counts, and count source fan-out / sink drivers.
        let mut src_stack = [0usize; 64];
        let mut src_heap = Vec::new();
        let source_counts: &mut [usize] = if total_source_slots <= src_stack.len() {
            &mut src_stack[..total_source_slots]
        } else {
            src_heap.resize(total_source_slots, 0);
            &mut src_heap
        };

        let mut dst_stack = [0usize; 64];
        let mut dst_heap = Vec::new();
        let sink_drivers: &mut [usize] = if total_sink_slots <= dst_stack.len() {
            &mut dst_stack[..total_sink_slots]
        } else {
            dst_heap.resize(total_sink_slots, 0);
            &mut dst_heap
        };

        let mut sink_edge_stack = [usize::MAX; 64];
        let mut sink_edge_heap = Vec::new();
        let sink_edge_idx: &mut [usize] = if total_sink_slots <= sink_edge_stack.len() {
            &mut sink_edge_stack[..total_sink_slots]
        } else {
            sink_edge_heap.resize(total_sink_slots, usize::MAX);
            &mut sink_edge_heap
        };

        for (edge_index, edge) in edges.iter().enumerate() {
            let src_slot = match edge.src {
                SignalSource::BoundaryIn(p) => {
                    if p >= self.num_inputs() {
                        return Err(InvariantError::InvalidBoundaryInput {
                            port: p,
                            num_inputs: self.num_inputs(),
                        });
                    }
                    usize::from(p)
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
                    node_out_base[node.index()] + usize::from(port)
                }
            };

            let dst_slot = match edge.dst {
                SignalSink::BoundaryOut(p) => {
                    if p >= self.num_outputs() {
                        return Err(InvariantError::InvalidBoundaryOutput {
                            port: p,
                            num_outputs: self.num_outputs(),
                        });
                    }
                    usize::from(p)
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
                    node_in_base[node.index()] + usize::from(port)
                }
            };

            if self.wire_delay() == WireDelayMode::TicksPerHop && edge.hops == 0 {
                return Err(InvariantError::ZeroHopInDelayedMode { edge_index });
            }

            source_counts[src_slot] += 1;
            sink_drivers[dst_slot] += 1;
            sink_edge_idx[dst_slot] = edge_index;
        }

        let slot_to_sink = |target_slot: usize| -> SignalSink {
            if target_slot < num_outputs {
                SignalSink::BoundaryOut(
                    u16::try_from(target_slot).expect("boundary output port fits in u16"),
                )
            } else {
                for (idx, &prim) in nodes.iter().enumerate() {
                    let base = node_in_base[idx];
                    let arity = usize::from(prim.input_arity());
                    if target_slot < base + arity {
                        return SignalSink::NodeIn {
                            node: NodeId(u32::try_from(idx).expect("node index fits in u32")),
                            port: u8::try_from(target_slot - base).expect("port fits in u8"),
                        };
                    }
                }
                unreachable!("sink slot in bounds")
            }
        };

        let slot_to_source = |target_slot: usize| -> SignalSource {
            if target_slot < num_inputs {
                SignalSource::BoundaryIn(
                    u16::try_from(target_slot).expect("boundary input port fits in u16"),
                )
            } else {
                for (idx, &prim) in nodes.iter().enumerate() {
                    let base = node_out_base[idx];
                    let arity = usize::from(prim.output_arity());
                    if target_slot < base + arity {
                        return SignalSource::NodeOut {
                            node: NodeId(u32::try_from(idx).expect("node index fits in u32")),
                            port: u8::try_from(target_slot - base).expect("port fits in u8"),
                        };
                    }
                }
                unreachable!("source slot in bounds")
            }
        };

        // 2. Enforce single driver per sink (§2.5.4).
        for (dst_slot, &count) in sink_drivers.iter().enumerate() {
            if count > 1 {
                return Err(InvariantError::MultipleDrivers {
                    sink: slot_to_sink(dst_slot),
                    count,
                });
            }
        }

        // 3. Enforce every active node input port has a driver (§2.5.4).
        for (idx, &prim) in nodes.iter().enumerate() {
            let node = NodeId(u32::try_from(idx).expect("node index fits in u32"));
            let base = node_in_base[idx];
            for port in 0..prim.input_arity() {
                if sink_drivers[base + usize::from(port)] == 0 {
                    return Err(InvariantError::UnconnectedInput { node, port });
                }
            }
        }

        // 4. Enforce SPLIT-only fan-out (§2.5.5).
        for (src_slot, &count) in source_counts.iter().enumerate() {
            if count > 1 {
                return Err(InvariantError::IllegalFanOut {
                    source: slot_to_source(src_slot),
                    count,
                });
            }
        }

        // 5. Assign contiguous wire indices (`next_wire` already holds inputs + node outputs):
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
        let mut delay_regs: Vec<(usize, usize)> = Vec::new(); // (node_idx, reg_idx)
        for (idx, &prim) in nodes.iter().enumerate() {
            if let Primitive::Delay { init } = prim {
                let reg_idx = reg_inits.len();
                reg_inits.push(RegInit {
                    initial_value: self.width().normalize(init),
                    is_explicit_delay: true,
                });
                let out_wire = node_out_base[idx];
                delay_reads.push((reg_idx, out_wire));
                delay_regs.push((idx, reg_idx));
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

        let boundary_out_wire = |port: u16| -> Option<usize> {
            let e_idx = sink_edge_idx[usize::from(port)];
            (e_idx != usize::MAX).then(|| edge_delivered_wire[e_idx])
        };
        let node_in_wire = |node_idx: usize, port: u8| -> usize {
            let e_idx = sink_edge_idx[node_in_base[node_idx] + usize::from(port)];
            debug_assert_ne!(e_idx, usize::MAX, "node input validated connected");
            edge_delivered_wire[e_idx]
        };

        // 6. Topological sort of combinational nodes (Kahn's algorithm, §2.5.2).
        let mut indeg_stack = [0usize; 64];
        let mut indeg_heap = Vec::new();
        let comb_in_degree: &mut [usize] = if nodes.len() <= indeg_stack.len() {
            &mut indeg_stack[..nodes.len()]
        } else {
            indeg_heap.resize(nodes.len(), 0);
            &mut indeg_heap
        };

        // Flat CSR-like linked list of combinational dependents (arbitrary out-arity, 0 per-node Vecs).
        let mut head_stack = [usize::MAX; 64];
        let mut head_heap = Vec::new();
        let first_dep: &mut [usize] = if nodes.len() <= head_stack.len() {
            &mut head_stack[..nodes.len()]
        } else {
            head_heap.resize(nodes.len(), usize::MAX);
            &mut head_heap
        };
        let mut dep_next: Vec<usize> = Vec::with_capacity(nodes.len() * 2);
        let mut dep_dst: Vec<usize> = Vec::with_capacity(nodes.len() * 2);
        let mut total_comb_nodes: usize = 0;

        for (idx, &prim) in nodes.iter().enumerate() {
            if prim.is_stateful() {
                continue;
            }
            total_comb_nodes += 1;
            if self.wire_delay() == WireDelayMode::Combinational {
                let base = node_in_base[idx];
                for port in 0..prim.input_arity() {
                    let e_idx = sink_edge_idx[base + usize::from(port)];
                    if let SignalSource::NodeOut { node: src_node, .. } = edges[e_idx].src
                        && !nodes[src_node.index()].is_stateful()
                    {
                        comb_in_degree[idx] += 1;
                        let s_idx = src_node.index();
                        let entry_idx = dep_dst.len();
                        dep_dst.push(idx);
                        dep_next.push(first_dep[s_idx]);
                        first_dep[s_idx] = entry_idx;
                    }
                }
            }
        }

        let mut topo_comb_nodes: Vec<NodeId> = Vec::with_capacity(total_comb_nodes);
        if nodes.len() <= 64 {
            let mut ready_mask: u64 = 0;
            for (idx, &prim) in nodes.iter().enumerate() {
                if !prim.is_stateful() && comb_in_degree[idx] == 0 {
                    ready_mask |= 1u64 << idx;
                }
            }
            while ready_mask != 0 {
                let idx = ready_mask.trailing_zeros() as usize;
                ready_mask &= ready_mask - 1;
                topo_comb_nodes.push(NodeId(u32::try_from(idx).expect("node index fits in u32")));
                let mut cur = first_dep[idx];
                while cur != usize::MAX {
                    let d_idx = dep_dst[cur];
                    comb_in_degree[d_idx] -= 1;
                    if comb_in_degree[d_idx] == 0 {
                        ready_mask |= 1u64 << d_idx;
                    }
                    cur = dep_next[cur];
                }
            }
        } else {
            let mut ready: BinaryHeap<Reverse<usize>> = BinaryHeap::new();
            for (idx, &prim) in nodes.iter().enumerate() {
                if !prim.is_stateful() && comb_in_degree[idx] == 0 {
                    ready.push(Reverse(idx));
                }
            }
            while let Some(Reverse(idx)) = ready.pop() {
                topo_comb_nodes.push(NodeId(u32::try_from(idx).expect("node index fits in u32")));
                let mut cur = first_dep[idx];
                while cur != usize::MAX {
                    let d_idx = dep_dst[cur];
                    comb_in_degree[d_idx] -= 1;
                    if comb_in_degree[d_idx] == 0 {
                        ready.push(Reverse(d_idx));
                    }
                    cur = dep_next[cur];
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
            let in0 = node_in_wire(idx, 0);
            let op = match prim {
                Primitive::Add => CombOp::Add {
                    lhs: in0,
                    rhs: node_in_wire(idx, 1),
                    out: out_base,
                },
                Primitive::Sub => CombOp::Sub {
                    lhs: in0,
                    rhs: node_in_wire(idx, 1),
                    out: out_base,
                },
                Primitive::Mul => CombOp::Mul {
                    lhs: in0,
                    rhs: node_in_wire(idx, 1),
                    out: out_base,
                },
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
        for (node_idx, reg_idx) in delay_regs {
            delay_latches.push((reg_idx, node_in_wire(node_idx, 0)));
        }

        let node_input_wires: Vec<Vec<usize>> = nodes
            .iter()
            .enumerate()
            .map(|(idx, prim)| {
                (0..prim.input_arity())
                    .map(|port| node_in_wire(idx, port))
                    .collect()
            })
            .collect();

        // 9. Wire boundary output drivers.
        let mut output_drivers: Vec<Option<usize>> = Vec::with_capacity(num_outputs);
        for p in 0..self.num_outputs() {
            output_drivers.push(boundary_out_wire(p));
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
            node_input_wires,
            delay_latches,
            shift_chains,
            output_drivers,
        ))
    }
}
