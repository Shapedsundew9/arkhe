//! Directed port-accurate circuit representation and wire-delay configuration.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::substrate::primitive::Primitive;
use crate::substrate::value::Width;

/// Dense node identifier inside a [`Circuit`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u32);

impl NodeId {
    /// Returns the zero-based slice index for this node.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// Wire propagation delay semantics (Charter §2.5 Invariant 3 & §8 ablations).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireDelayMode {
    /// Default: wires propagate combinationally within a tick; only `DELAY` holds state.
    #[default]
    Combinational,
    /// Ablation: every edge inserts `hops >= 1` synchronous pipeline ticks.
    TicksPerHop,
}

/// Signal origin port within a [`Circuit`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SignalSource {
    /// Environment input channel `0..num_inputs`.
    BoundaryIn(u16),
    /// Output slot `port` (`0..output_arity`) of `node`.
    NodeOut { node: NodeId, port: u8 },
}

/// Signal destination port within a [`Circuit`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SignalSink {
    /// Input slot `port` (`0..input_arity`) of `node`.
    NodeIn { node: NodeId, port: u8 },
    /// Boundary prediction port `0..num_outputs`.
    BoundaryOut(u16),
}

/// Directed wire linking one [`SignalSource`] to one [`SignalSink`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Edge {
    /// Driving output port.
    pub src: SignalSource,
    /// Driven input port.
    pub dst: SignalSink,
    /// Hop count (ignored as `0` in [`WireDelayMode::Combinational`]; must be `>= 1` in `TicksPerHop`).
    pub hops: u16,
}

/// Uncompiled synchronous modular-arithmetic circuit graph.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Circuit {
    width: Width,
    num_inputs: u16,
    num_outputs: u16,
    wire_delay: WireDelayMode,
    nodes: Vec<Primitive>,
    edges: Vec<Edge>,
}

impl Circuit {
    /// Creates an empty [`Circuit`] with the given boundary arities and wire-delay mode.
    #[must_use]
    pub const fn new(
        width: Width,
        num_inputs: u16,
        num_outputs: u16,
        wire_delay: WireDelayMode,
    ) -> Self {
        Self {
            width,
            num_inputs,
            num_outputs,
            wire_delay,
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }

    /// Returns the uniform bit-width $k$ of all wires in this circuit.
    #[must_use]
    pub const fn width(&self) -> Width {
        self.width
    }

    /// Number of boundary input channels.
    #[must_use]
    pub const fn num_inputs(&self) -> u16 {
        self.num_inputs
    }

    /// Number of boundary prediction output ports.
    #[must_use]
    pub const fn num_outputs(&self) -> u16 {
        self.num_outputs
    }

    /// Active wire-delay mode.
    #[must_use]
    pub const fn wire_delay(&self) -> WireDelayMode {
        self.wire_delay
    }

    /// Slice of placed primitives indexed by [`NodeId`].
    #[must_use]
    pub fn nodes(&self) -> &[Primitive] {
        &self.nodes
    }

    /// Slice of directed port edges.
    #[must_use]
    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    /// Total number of primitives in the circuit.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Number of explicit `DELAY` primitives in the circuit.
    #[must_use]
    pub fn delay_count(&self) -> usize {
        self.nodes.iter().filter(|p| p.is_stateful()).count()
    }

    /// Number of `SPLIT` primitives in the circuit.
    #[must_use]
    pub fn split_count(&self) -> usize {
        self.nodes
            .iter()
            .filter(|p| matches!(p, Primitive::Split))
            .count()
    }

    /// Appends a primitive (normalising any `Delay` initial state to `width`) and returns its [`NodeId`].
    pub fn add_node(&mut self, primitive: Primitive) -> NodeId {
        let id = NodeId(u32::try_from(self.nodes.len()).expect("node count fits in u32"));
        self.nodes.push(primitive.normalized(self.width));
        id
    }

    /// Appends an edge using the default hop count for the circuit's [`WireDelayMode`] (`0` or `1`).
    pub fn connect(&mut self, src: SignalSource, dst: SignalSink) -> &mut Self {
        let default_hops = match self.wire_delay {
            WireDelayMode::Combinational => 0,
            WireDelayMode::TicksPerHop => 1,
        };
        self.connect_hops(src, dst, default_hops)
    }

    /// Appends an edge with an explicit hop count.
    pub fn connect_hops(&mut self, src: SignalSource, dst: SignalSink, hops: u16) -> &mut Self {
        self.edges.push(Edge { src, dst, hops });
        self
    }

    /// Updates the initial state of an existing `Primitive::Delay` node (masked to `self.width()`).
    ///
    /// Returns `true` if `node` exists and is a `Delay` primitive, or `false` otherwise.
    pub fn set_delay_init(&mut self, node: NodeId, init: u64) -> bool {
        if let Some(Primitive::Delay { init: slot }) = self.nodes.get_mut(node.index()) {
            *slot = self.width.normalize(init);
            true
        } else {
            false
        }
    }

    /// Counts distinct `BoundaryIn(c)` channels with a directed path to any `BoundaryOut` or
    /// synchronous state register (`Delay` node or `TicksPerHop` pipeline edge).
    #[must_use]
    pub fn active_boundary_input_count(&self) -> usize {
        if self.num_inputs == 0 || self.edges.is_empty() {
            return 0;
        }
        let n_in = usize::from(self.num_inputs);
        if self.wire_delay == WireDelayMode::TicksPerHop {
            let mut active = vec![false; n_in];
            for edge in &self.edges {
                if let SignalSource::BoundaryIn(p) = edge.src
                    && usize::from(p) < n_in
                {
                    active[usize::from(p)] = true;
                }
            }
            return active.into_iter().filter(|&b| b).count();
        }

        let n_nodes = self.nodes.len();
        let mut reaches_sink_or_state: Vec<bool> =
            self.nodes.iter().map(|p| p.is_stateful()).collect();
        for edge in &self.edges {
            if matches!(edge.dst, SignalSink::BoundaryOut(_))
                && let SignalSource::NodeOut { node, .. } = edge.src
                && node.index() < n_nodes
            {
                reaches_sink_or_state[node.index()] = true;
            }
        }

        let mut changed = true;
        while changed {
            changed = false;
            for edge in &self.edges {
                if let SignalSink::NodeIn { node: dst, .. } = edge.dst
                    && dst.index() < n_nodes
                    && reaches_sink_or_state[dst.index()]
                    && let SignalSource::NodeOut { node: src, .. } = edge.src
                    && src.index() < n_nodes
                    && !reaches_sink_or_state[src.index()]
                {
                    reaches_sink_or_state[src.index()] = true;
                    changed = true;
                }
            }
        }

        let mut active_in = vec![false; n_in];
        for edge in &self.edges {
            if let SignalSource::BoundaryIn(p) = edge.src
                && usize::from(p) < n_in
            {
                let reaches = match edge.dst {
                    SignalSink::BoundaryOut(_) => true,
                    SignalSink::NodeIn { node, .. } => {
                        node.index() < n_nodes && reaches_sink_or_state[node.index()]
                    }
                };
                if reaches {
                    active_in[usize::from(p)] = true;
                }
            }
        }
        active_in.into_iter().filter(|&b| b).count()
    }

    /// Returns all `Delay` [`NodeId`]s (in ascending index order) that have a directed path in the
    /// full circuit graph to at least one directed cycle.
    ///
    /// Only cycle-reaching `Delay` registers can influence post-transient ($t \ge N$) predictions;
    /// purely feed-forward `Delay` lines flush their initial values within $\le N$ ticks.
    #[must_use]
    pub fn cycle_reaching_delays(&self) -> Vec<NodeId> {
        let n = self.nodes.len();
        if n == 0 {
            return Vec::new();
        }
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
        for edge in &self.edges {
            if let (SignalSource::NodeOut { node: u, .. }, SignalSink::NodeIn { node: v, .. }) =
                (edge.src, edge.dst)
                && u.index() < n
                && v.index() < n
            {
                adj[u.index()].push(v.index());
            }
        }

        // Compute transitive reachability `reach[u][v]` (u reaches v in >= 1 directed edges).
        let mut reach = vec![vec![false; n]; n];
        for start in 0..n {
            let mut stack: Vec<usize> = adj[start].clone();
            while let Some(curr) = stack.pop() {
                if !reach[start][curr] {
                    reach[start][curr] = true;
                    for &nxt in &adj[curr] {
                        if !reach[start][nxt] {
                            stack.push(nxt);
                        }
                    }
                }
            }
        }

        let on_cycle: Vec<bool> = (0..n).map(|u| reach[u][u]).collect();
        let mut result = Vec::new();
        for (idx, prim) in self.nodes.iter().enumerate() {
            if prim.is_stateful() && (on_cycle[idx] || (0..n).any(|u| reach[idx][u] && on_cycle[u]))
            {
                result.push(NodeId(u32::try_from(idx).expect("node index fits in u32")));
            }
        }
        result
    }

    /// Removes all directed edges exactly matching `(src, dst)`. Returns `true` if any edges were removed.
    pub fn disconnect(&mut self, src: SignalSource, dst: SignalSink) -> bool {
        let original_len = self.edges.len();
        self.edges.retain(|e| e.src != src || e.dst != dst);
        self.edges.len() != original_len
    }

    /// Replaces the source endpoint of the edge driving `dst`. Returns `true` if found and replaced.
    pub fn replace_edge_source(&mut self, dst: SignalSink, src: SignalSource) -> bool {
        if let Some(edge) = self.edges.iter_mut().find(|e| e.dst == dst) {
            edge.src = src;
            true
        } else {
            false
        }
    }

    /// Compacts nodes after deletion and returns `old_node -> Some(new_node)` for survivors.
    ///
    /// Preserves existing relative order of surviving nodes and incident edges.
    pub fn remove_nodes(&mut self, removed: &BTreeSet<NodeId>) -> Vec<Option<NodeId>> {
        let mut mapping = vec![None; self.nodes.len()];
        let mut nodes = Vec::with_capacity(self.nodes.len().saturating_sub(removed.len()));
        for (old_index, &primitive) in self.nodes.iter().enumerate() {
            let old = NodeId(u32::try_from(old_index).expect("node index fits in u32"));
            if removed.contains(&old) {
                continue;
            }
            let new = NodeId(u32::try_from(nodes.len()).expect("node count fits in u32"));
            mapping[old_index] = Some(new);
            nodes.push(primitive);
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
            .filter_map(|&edge| {
                Some(Edge {
                    src: remap_source(edge.src)?,
                    dst: remap_sink(edge.dst)?,
                    hops: edge.hops,
                })
            })
            .collect();
        self.nodes = nodes;
        mapping
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circuit_graph_mutation_primitives() {
        let mut c = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);
        let n0 = c.add_node(Primitive::Eqz);
        let n1 = c.add_node(Primitive::Split);
        let n2 = c.add_node(Primitive::Delay { init: 0 });

        c.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: n0, port: 0 },
        );
        c.connect(
            SignalSource::NodeOut { node: n0, port: 0 },
            SignalSink::NodeIn { node: n1, port: 0 },
        );
        c.connect(
            SignalSource::NodeOut { node: n1, port: 0 },
            SignalSink::NodeIn { node: n2, port: 0 },
        );
        c.connect(
            SignalSource::NodeOut { node: n2, port: 0 },
            SignalSink::BoundaryOut(0),
        );

        assert_eq!(c.node_count(), 3);
        assert_eq!(c.edges().len(), 4);

        // Test disconnect
        assert!(c.disconnect(
            SignalSource::NodeOut { node: n1, port: 0 },
            SignalSink::NodeIn { node: n2, port: 0 }
        ));
        assert_eq!(c.edges().len(), 3);

        // Test replace_edge_source
        assert!(c.replace_edge_source(
            SignalSink::BoundaryOut(0),
            SignalSource::NodeOut { node: n1, port: 1 }
        ));
        assert!(c.edges().contains(&Edge {
            src: SignalSource::NodeOut { node: n1, port: 1 },
            dst: SignalSink::BoundaryOut(0),
            hops: 0,
        }));

        // Test remove_nodes
        let mut removed = BTreeSet::new();
        removed.insert(n2);
        let mapping = c.remove_nodes(&removed);

        assert_eq!(c.node_count(), 2);
        assert_eq!(mapping[n0.index()], Some(NodeId(0)));
        assert_eq!(mapping[n1.index()], Some(NodeId(1)));
        assert_eq!(mapping[n2.index()], None);

        // Surviving nodes order preserved
        assert_eq!(c.nodes(), &[Primitive::Eqz, Primitive::Split]);
    }
}
