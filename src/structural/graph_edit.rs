//! Coordinate-free structural graph edit policy helpers (§1.3).
//!
//! Encodes neurogenesis and growth policies over [`Circuit`] multigraphs without spatial coordinate grids.

use std::collections::BTreeSet;

use crate::structural::surgery::{port_upstream_cone_and_edges, register_mapping};
use crate::substrate::{
    Circuit, CompiledCircuit, NodeId, Primitive, ScalarState, SignalSink, SignalSource,
    WireDelayMode,
};

/// Taps a signal source, automatically inserting a [`Primitive::Split`] when required.
///
/// Under coordinate-free WellMixed wiring:
/// 1. If `source` has no consumers, it is returned directly.
/// 2. If `source` is an existing `Split` with a free output port, that port is returned.
/// 3. If an existing consumer of `source` is a `Split` at input 0 with a free port 1, port 1 is returned.
/// 4. Otherwise, a new `Split` is appended, all existing consumers of `source` are rewired
///    to `split.port(0)`, `source` drives `split.in(0)`, and `split.port(1)` is returned.
pub fn tap_source(circuit: &mut Circuit, source: SignalSource) -> SignalSource {
    let existing_sinks: Vec<SignalSink> = circuit
        .edges()
        .iter()
        .filter_map(|e| (e.src == source).then_some(e.dst))
        .collect();

    if existing_sinks.is_empty() {
        return source;
    }

    if let SignalSource::NodeOut { node, .. } = source
        && circuit.nodes().get(node.index()) == Some(&Primitive::Split)
    {
        let port0_used = circuit
            .edges()
            .iter()
            .any(|e| e.src == SignalSource::NodeOut { node, port: 0 });
        let port1_used = circuit
            .edges()
            .iter()
            .any(|e| e.src == SignalSource::NodeOut { node, port: 1 });
        if !port0_used {
            return SignalSource::NodeOut { node, port: 0 };
        }
        if !port1_used {
            return SignalSource::NodeOut { node, port: 1 };
        }
    }

    for &sink in &existing_sinks {
        if let SignalSink::NodeIn {
            node: sink_node,
            port: 0,
        } = sink
            && circuit.nodes().get(sink_node.index()) == Some(&Primitive::Split)
        {
            let port1_used = circuit.edges().iter().any(|e| {
                e.src
                    == SignalSource::NodeOut {
                        node: sink_node,
                        port: 1,
                    }
            });
            if !port1_used {
                return SignalSource::NodeOut {
                    node: sink_node,
                    port: 1,
                };
            }
        }
    }

    let split_node = circuit.add_node(Primitive::Split);
    for &sink in &existing_sinks {
        circuit.disconnect(source, sink);
        circuit.connect(
            SignalSource::NodeOut {
                node: split_node,
                port: 0,
            },
            sink,
        );
    }
    circuit.connect(
        source,
        SignalSink::NodeIn {
            node: split_node,
            port: 0,
        },
    );
    SignalSource::NodeOut {
        node: split_node,
        port: 1,
    }
}

/// Stacks a binary operator (such as `Add` or `Sub`) at `BoundaryOut(out_port)`.
pub fn stack_output_binary_op(
    circuit: &mut Circuit,
    op: Primitive,
    out_port: u16,
    corrector_src: SignalSource,
) -> NodeId {
    let out_sink = SignalSink::BoundaryOut(out_port);
    let existing_driver = circuit
        .edges()
        .iter()
        .find_map(|e| (e.dst == out_sink).then_some(e.src));

    match existing_driver {
        None => match corrector_src {
            SignalSource::BoundaryIn(_) => {
                let tapped = tap_source(circuit, corrector_src);
                let split = circuit.add_node(Primitive::Split);
                circuit.connect(
                    tapped,
                    SignalSink::NodeIn {
                        node: split,
                        port: 0,
                    },
                );
                circuit.connect(
                    SignalSource::NodeOut {
                        node: split,
                        port: 0,
                    },
                    out_sink,
                );
                split
            }
            SignalSource::NodeOut { node, .. } => {
                circuit.connect(corrector_src, out_sink);
                node
            }
        },
        Some(driver) => {
            circuit.disconnect(driver, out_sink);
            let y_tap = tap_source(circuit, driver);
            let op_node = circuit.add_node(op);
            circuit.connect(
                y_tap,
                SignalSink::NodeIn {
                    node: op_node,
                    port: 0,
                },
            );
            let r_tap = tap_source(circuit, corrector_src);
            circuit.connect(
                r_tap,
                SignalSink::NodeIn {
                    node: op_node,
                    port: 1,
                },
            );
            circuit.connect(
                SignalSource::NodeOut {
                    node: op_node,
                    port: 0,
                },
                out_sink,
            );
            op_node
        }
    }
}

fn materialize_zero_loop_tph(circuit: &mut Circuit) -> SignalSource {
    let split = circuit.add_node(Primitive::Split);
    circuit.connect(
        SignalSource::NodeOut {
            node: split,
            port: 0,
        },
        SignalSink::NodeIn {
            node: split,
            port: 0,
        },
    );
    SignalSource::NodeOut {
        node: split,
        port: 1,
    }
}

fn materialize_one_tph(circuit: &mut Circuit) -> SignalSource {
    let zero = materialize_zero_loop_tph(circuit);
    let eqz = circuit.add_node(Primitive::Eqz);
    circuit.connect(zero, SignalSink::NodeIn { node: eqz, port: 0 });
    SignalSource::NodeOut { node: eqz, port: 0 }
}

/// Materializes a constant generator loop (`Delay` <-> `Split` feedback cycle in
/// `Combinational` mode, or settled `zero_loop`/`Eqz`/`Sub`/`Add` constant in `TicksPerHop`).
pub fn materialize_const_loop(circuit: &mut Circuit, c: u64) -> SignalSource {
    use crate::substrate::WireDelayMode;

    if circuit.wire_delay() == WireDelayMode::TicksPerHop {
        let c_norm = circuit.width().normalize(c);
        if c_norm == 0 {
            return materialize_zero_loop_tph(circuit);
        }
        if c_norm == 1 {
            return materialize_one_tph(circuit);
        }
        if c_norm == circuit.width().mask() {
            let sub = circuit.add_node(Primitive::Sub);
            let zero = materialize_zero_loop_tph(circuit);
            let one = materialize_one_tph(circuit);
            circuit.connect(zero, SignalSink::NodeIn { node: sub, port: 0 });
            circuit.connect(one, SignalSink::NodeIn { node: sub, port: 1 });
            return SignalSource::NodeOut { node: sub, port: 0 };
        }
        let add = circuit.add_node(Primitive::Add);
        let one = materialize_one_tph(circuit);
        let split = circuit.add_node(Primitive::Split);
        circuit.connect(
            one,
            SignalSink::NodeIn {
                node: split,
                port: 0,
            },
        );
        circuit.connect(
            SignalSource::NodeOut {
                node: split,
                port: 0,
            },
            SignalSink::NodeIn { node: add, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut {
                node: split,
                port: 1,
            },
            SignalSink::NodeIn { node: add, port: 1 },
        );
        let mut acc = SignalSource::NodeOut { node: add, port: 0 };
        for _ in 2..c_norm {
            let next_add = circuit.add_node(Primitive::Add);
            let extra_one = materialize_one_tph(circuit);
            circuit.connect(
                acc,
                SignalSink::NodeIn {
                    node: next_add,
                    port: 0,
                },
            );
            circuit.connect(
                extra_one,
                SignalSink::NodeIn {
                    node: next_add,
                    port: 1,
                },
            );
            acc = SignalSource::NodeOut {
                node: next_add,
                port: 0,
            };
        }
        return acc;
    }

    let split = circuit.add_node(Primitive::Split);
    let delay = circuit.add_node(Primitive::Delay { init: c });
    circuit.connect(
        SignalSource::NodeOut {
            node: delay,
            port: 0,
        },
        SignalSink::NodeIn {
            node: split,
            port: 0,
        },
    );
    circuit.connect(
        SignalSource::NodeOut {
            node: split,
            port: 0,
        },
        SignalSink::NodeIn {
            node: delay,
            port: 0,
        },
    );
    SignalSource::NodeOut {
        node: split,
        port: 1,
    }
}

/// Builds an autonomous delay ring generator and returns `(delay_nodes, split_node, output_source)`
/// without connecting `output_source` to any sink.
pub fn build_autonomous_ring_source(
    circuit: &mut Circuit,
    period: usize,
    mu: &[u64],
) -> (Vec<NodeId>, NodeId, SignalSource) {
    let mut delays = Vec::with_capacity(period);
    for j in 0..period {
        let init_val = mu[(period - 1 - j) % period];
        let d = circuit.add_node(Primitive::Delay { init: init_val });
        delays.push(d);
    }
    let split = circuit.add_node(Primitive::Split);

    for i in 0..(period - 1) {
        circuit.connect(
            SignalSource::NodeOut {
                node: delays[i],
                port: 0,
            },
            SignalSink::NodeIn {
                node: delays[i + 1],
                port: 0,
            },
        );
    }
    circuit.connect(
        SignalSource::NodeOut {
            node: delays[period - 1],
            port: 0,
        },
        SignalSink::NodeIn {
            node: split,
            port: 0,
        },
    );
    circuit.connect(
        SignalSource::NodeOut {
            node: split,
            port: 0,
        },
        SignalSink::NodeIn {
            node: delays[0],
            port: 0,
        },
    );
    (
        delays,
        split,
        SignalSource::NodeOut {
            node: split,
            port: 1,
        },
    )
}

/// Builds an autonomous delay ring generator driving `out_port`.
pub fn build_autonomous_ring(
    circuit: &mut Circuit,
    period: usize,
    mu: &[u64],
    out_port: u16,
) -> (Vec<NodeId>, NodeId) {
    let (delays, split, out_src) = build_autonomous_ring_source(circuit, period, mu);
    circuit.connect(out_src, SignalSink::BoundaryOut(out_port));
    (delays, split)
}

/// Returns `true` if `node` is a [`Primitive::Split`] whose entire upstream ancestry is strictly
/// a tree of [`Primitive::Split`] nodes driven directly by a [`SignalSource::BoundaryIn`].
#[must_use]
pub fn is_pure_boundary_split(circuit: &Circuit, node: NodeId) -> bool {
    let mut curr = node;
    let mut visited = BTreeSet::new();
    loop {
        if !visited.insert(curr) {
            return false;
        }
        if circuit.nodes().get(curr.index()) != Some(&Primitive::Split) {
            return false;
        }
        let Some(edge) = circuit.edges().iter().find(|e| {
            e.dst
                == SignalSink::NodeIn {
                    node: curr,
                    port: 0,
                }
        }) else {
            return false;
        };
        match edge.src {
            SignalSource::BoundaryIn(_) => return true,
            SignalSource::NodeOut { node: parent, .. } => curr = parent,
        }
    }
}

/// Computes the bitmask of boundary input channels (`1u64 << c` for `c < 64`) reachable in
/// `root`'s upstream causal cone (`0` for purely autonomous cycles / constants).
#[must_use]
pub fn cone_channel_mask(circuit: &Circuit, root: SignalSource) -> u64 {
    let mut mask = 0u64;
    let mut visited = BTreeSet::new();
    let mut stack = vec![root];
    while let Some(src) = stack.pop() {
        match src {
            SignalSource::BoundaryIn(c) => {
                if usize::from(c) < 64 {
                    mask |= 1u64 << c;
                }
            }
            SignalSource::NodeOut { node, .. } => {
                if !visited.insert(node) {
                    continue;
                }
                for edge in circuit.edges() {
                    if let SignalSink::NodeIn { node: dst, .. } = edge.dst
                        && dst == node
                    {
                        stack.push(edge.src);
                    }
                }
            }
        }
    }
    mask
}

/// Computes the maximum register depth along any simple path upstream of `source` (counting
/// [`Primitive::Delay`] nodes plus edge `hops` in [`WireDelayMode::TicksPerHop`]).
#[must_use]
pub fn source_register_depth(circuit: &Circuit, source: SignalSource) -> usize {
    fn rec(circuit: &Circuit, node: NodeId, on_path: &mut [bool]) -> usize {
        if on_path[node.index()] {
            return 0;
        }
        on_path[node.index()] = true;
        let self_reg = usize::from(matches!(
            circuit.nodes().get(node.index()),
            Some(Primitive::Delay { .. })
        ));
        let is_tph = circuit.wire_delay() == WireDelayMode::TicksPerHop;
        let mut max_up = 0usize;
        for edge in circuit.edges() {
            if let SignalSink::NodeIn { node: dst, .. } = edge.dst
                && dst == node
            {
                let hop_regs = if is_tph { usize::from(edge.hops) } else { 0 };
                let up = match edge.src {
                    SignalSource::BoundaryIn(_) => hop_regs,
                    SignalSource::NodeOut { node: src_node, .. } => {
                        if on_path[src_node.index()] {
                            0
                        } else {
                            hop_regs + rec(circuit, src_node, on_path)
                        }
                    }
                };
                max_up = max_up.max(up);
            }
        }
        on_path[node.index()] = false;
        self_reg + max_up
    }

    match source {
        SignalSource::BoundaryIn(_) => 0,
        SignalSource::NodeOut { node, .. } => {
            let mut on_path = vec![false; circuit.node_count()];
            rec(circuit, node, &mut on_path)
        }
    }
}

/// Causal support summary of existing derivations for one output port (`M2B-D2`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DerivationConeInfo {
    /// Excluded interior computation/state nodes $\mathcal{V}_{\text{excl}}$ (excluding pure
    /// fan-out `Split` trees directly on `BoundaryIn(c)`).
    pub excl_nodes: BTreeSet<NodeId>,
    /// Excluded elementary reactive leaves $\mathcal{L}_{\text{excl}}$ as `(channel, effective_lag)`.
    pub excl_reactive_leaves: BTreeSet<(u16, usize)>,
    /// Whether any existing derivation on the target is an autonomous cycle (`mask == 0` with cycle).
    pub has_autonomous_cycle: bool,
}

pub(crate) fn source_has_cycle(circuit: &Circuit, root: SignalSource) -> bool {
    let mut visited = BTreeSet::new();
    let mut on_path = BTreeSet::new();
    let mut stack: Vec<(NodeId, bool)> = Vec::new();
    if let SignalSource::NodeOut { node, .. } = root {
        stack.push((node, false));
    }
    while let Some((node, leaving)) = stack.pop() {
        if leaving {
            on_path.remove(&node);
            continue;
        }
        if on_path.contains(&node) {
            return true;
        }
        if !visited.insert(node) {
            continue;
        }
        on_path.insert(node);
        stack.push((node, true));
        for edge in circuit.edges() {
            if let SignalSink::NodeIn { node: dst, .. } = edge.dst
                && dst == node
                && let SignalSource::NodeOut { node: src, .. } = edge.src
            {
                if on_path.contains(&src) {
                    return true;
                }
                if !visited.contains(&src) {
                    stack.push((src, false));
                }
            }
        }
    }
    false
}

/// Collects $\mathcal{V}_{\text{excl}}$, $\mathcal{L}_{\text{excl}}$, and autonomous cycle status
/// across `roots` (each paired with initial edge hops `0` or `edge.hops` in `TicksPerHop`).
#[must_use]
pub fn derivation_cone_info(circuit: &Circuit, root_sinks: &[SignalSink]) -> DerivationConeInfo {
    let is_tph = circuit.wire_delay() == WireDelayMode::TicksPerHop;
    let mut info = DerivationConeInfo::default();
    let mut visited_for_nodes = BTreeSet::new();
    let mut on_path = vec![false; circuit.node_count()];

    #[allow(clippy::too_many_arguments)]
    fn walk_leaves(
        circuit: &Circuit,
        is_tph: bool,
        src: SignalSource,
        acc_lag: usize,
        on_path: &mut [bool],
        seen_state: &mut BTreeSet<(usize, usize)>,
        leaves: &mut BTreeSet<(u16, usize)>,
    ) {
        match src {
            SignalSource::BoundaryIn(c) => {
                leaves.insert((c, acc_lag));
            }
            SignalSource::NodeOut { node, .. } => {
                if on_path[node.index()] {
                    return;
                }
                if !seen_state.insert((node.index(), acc_lag)) {
                    return;
                }
                on_path[node.index()] = true;
                let self_reg = usize::from(matches!(
                    circuit.nodes().get(node.index()),
                    Some(Primitive::Delay { .. })
                ));
                let next_lag = acc_lag + self_reg;
                for edge in circuit.edges() {
                    if let SignalSink::NodeIn { node: dst, .. } = edge.dst
                        && dst == node
                    {
                        let hop = if is_tph { usize::from(edge.hops) } else { 0 };
                        walk_leaves(
                            circuit,
                            is_tph,
                            edge.src,
                            next_lag + hop,
                            on_path,
                            seen_state,
                            leaves,
                        );
                    }
                }
                on_path[node.index()] = false;
            }
        }
    }

    let mut seen_state = BTreeSet::new();
    for &sink in root_sinks {
        let Some(root_edge) = circuit.edges().iter().find(|e| e.dst == sink) else {
            continue;
        };
        if cone_channel_mask(circuit, root_edge.src) == 0
            && source_has_cycle(circuit, root_edge.src)
        {
            info.has_autonomous_cycle = true;
        }
        let init_lag = if is_tph {
            usize::from(root_edge.hops)
        } else {
            0
        };
        walk_leaves(
            circuit,
            is_tph,
            root_edge.src,
            init_lag,
            &mut on_path,
            &mut seen_state,
            &mut info.excl_reactive_leaves,
        );

        let mut stack = vec![root_edge.src];
        while let Some(src) = stack.pop() {
            if let SignalSource::NodeOut { node, .. } = src {
                if !visited_for_nodes.insert(node) {
                    continue;
                }
                if !is_pure_boundary_split(circuit, node) {
                    info.excl_nodes.insert(node);
                }
                for edge in circuit.edges() {
                    if let SignalSink::NodeIn { node: dst, .. } = edge.dst
                        && dst == node
                    {
                        stack.push(edge.src);
                    }
                }
            }
        }
    }

    info
}

/// Result of splicing an `alt` source and `EQZ(SUB(primary, alt))` comparator onto a driven port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SplicedComparator {
    pub sub_node: NodeId,
    pub eqz_node: NodeId,
    pub inserted_split: Option<NodeId>,
    pub primary_autonomous_shifted: bool,
}

/// Splices a newly constructed `alt_src` predictor onto `target_port` via `EQZ(SUB(primary, alt))`.
///
/// Reuses any free `Split` port driving `BoundaryOut(target_port)` to avoid adding wire hops in
/// `TicksPerHop`. Returns `None` if `target_port` is unconnected.
pub fn splice_comparator_onto_port(
    circuit: &mut Circuit,
    target_port: u16,
    alt_src: SignalSource,
) -> Option<SplicedComparator> {
    let old_out_edge = circuit
        .edges()
        .iter()
        .find(|e| e.dst == SignalSink::BoundaryOut(target_port))?
        .src;
    let pre_node_count = circuit.node_count();
    let is_autonomous =
        cone_channel_mask(circuit, old_out_edge) == 0 && source_has_cycle(circuit, old_out_edge);
    let tapped_primary = tap_source(circuit, old_out_edge);
    let inserted_split = if circuit.node_count() > pre_node_count {
        Some(NodeId(
            u32::try_from(pre_node_count).expect("node index fits in u32"),
        ))
    } else {
        None
    };
    let primary_autonomous_shifted = circuit.wire_delay() == WireDelayMode::TicksPerHop
        && inserted_split.is_some()
        && is_autonomous;

    let sub_node = circuit.add_node(Primitive::Sub);
    let eqz_node = circuit.add_node(Primitive::Eqz);
    circuit.connect(
        tapped_primary,
        SignalSink::NodeIn {
            node: sub_node,
            port: 0,
        },
    );
    circuit.connect(
        alt_src,
        SignalSink::NodeIn {
            node: sub_node,
            port: 1,
        },
    );
    circuit.connect(
        SignalSource::NodeOut {
            node: sub_node,
            port: 0,
        },
        SignalSink::NodeIn {
            node: eqz_node,
            port: 0,
        },
    );

    Some(SplicedComparator {
        sub_node,
        eqz_node,
        inserted_split,
        primary_autonomous_shifted,
    })
}

/// Advances an autonomous cycle's registers by `+1` tick in `TicksPerHop` mode when `tap_source`
/// inserted a new `Split` hop between the cycle's driver and `BoundaryOut(target_port)`.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn advance_tph_autonomous_ring_phase(
    pre_circuit: &Circuit,
    pre_compiled: &CompiledCircuit,
    pre_regs: &[u64],
    staged_circuit: &Circuit,
    staged_compiled: &CompiledCircuit,
    old_to_new: &[Option<NodeId>],
    target_port: u16,
    inserted_split: NodeId,
    sub_node: NodeId,
) -> Option<Vec<u64>> {
    if staged_circuit.wire_delay() != WireDelayMode::TicksPerHop {
        return None;
    }
    let mut pre_state = ScalarState::new(pre_compiled);
    pre_state.set_registers(pre_compiled, pre_regs).ok()?;
    let zeros_in = vec![0u64; usize::from(pre_compiled.num_inputs())];
    let mut pre_out_before = vec![0u64; usize::from(pre_compiled.num_outputs())];
    pre_state
        .step(pre_compiled, &zeros_in, &mut pre_out_before)
        .ok()?;
    let mut pre_out_after = vec![0u64; usize::from(pre_compiled.num_outputs())];
    let post_first_regs = pre_state.registers().to_vec();
    // Advance one extra tick for internal cycle registers while holding current tick's `out_before`
    // for the new downstream `split -> BoundaryOut` / `split -> sub.in(0)` hop.
    let reg_map = register_mapping(
        pre_circuit,
        staged_circuit,
        pre_compiled,
        staged_compiled,
        old_to_new,
    );
    let mut staged_regs = vec![0u64; staged_compiled.total_registers()];
    let (cone_nodes, _) = port_upstream_cone_and_edges(pre_circuit, target_port);
    let mut pre_edge_bases = Vec::with_capacity(pre_circuit.edges().len());
    let mut cur_reg = pre_compiled.explicit_delay_count();
    for edge in pre_circuit.edges() {
        pre_edge_bases.push(cur_reg);
        cur_reg += usize::from(edge.hops);
    }
    let mut staged_edge_bases = Vec::with_capacity(staged_circuit.edges().len());
    let mut cur_staged = staged_compiled.explicit_delay_count();
    for edge in staged_circuit.edges() {
        staged_edge_bases.push(cur_staged);
        cur_staged += usize::from(edge.hops);
    }

    // Map every register in the cone using `post_first_regs` (+1 phase step).
    for (old_r, new_r) in reg_map.iter().enumerate() {
        if let Some(nr) = new_r
            && old_r < pre_regs.len()
        {
            staged_regs[*nr] = pre_regs[old_r];
        }
    }
    // Overwrite registers inside `cone_nodes` with `+1` stepped `post_first_regs`.
    let mut pre_delay_idx = 0usize;
    for (idx, prim) in pre_circuit.nodes().iter().enumerate() {
        if prim.is_stateful() {
            let old_node = NodeId(u32::try_from(idx).expect("fits"));
            if cone_nodes.contains(&old_node)
                && let Some(nr) = reg_map.get(pre_delay_idx).copied().flatten()
            {
                staged_regs[nr] = post_first_regs[pre_delay_idx];
            }
            pre_delay_idx += 1;
        }
    }
    for (edge_idx, edge) in pre_circuit.edges().iter().enumerate() {
        let in_cone = match edge.src {
            SignalSource::NodeOut { node, .. } => cone_nodes.contains(&node),
            SignalSource::BoundaryIn(_) => false,
        };
        if in_cone && edge.dst != SignalSink::BoundaryOut(target_port) {
            let base = pre_edge_bases[edge_idx];
            for s in 0..usize::from(edge.hops) {
                if let Some(nr) = reg_map.get(base + s).copied().flatten() {
                    staged_regs[nr] = post_first_regs[base + s];
                }
            }
        }
    }
    // `src -> split.in(0)` gets what `src -> Out` held after 1 tick (`pre_out_next`),
    // while `split.0 -> BoundaryOut` and `split.1 -> sub.in(0)` get `pre_out_before`.
    let mut pre_state2 = ScalarState::new(pre_compiled);
    pre_state2
        .set_registers(pre_compiled, &post_first_regs)
        .ok()?;
    pre_state2
        .step(pre_compiled, &zeros_in, &mut pre_out_after)
        .ok()?;
    let val_before = pre_out_before[usize::from(target_port)];
    let val_next = pre_out_after[usize::from(target_port)];
    for (edge_idx, edge) in staged_circuit.edges().iter().enumerate() {
        let base = staged_edge_bases[edge_idx];
        if edge.dst
            == (SignalSink::NodeIn {
                node: inserted_split,
                port: 0,
            })
        {
            staged_regs[base] = val_next;
        } else if edge.dst == SignalSink::BoundaryOut(target_port)
            || edge.dst
                == (SignalSink::NodeIn {
                    node: sub_node,
                    port: 0,
                })
        {
            staged_regs[base] = val_before;
        }
    }
    Some(staged_regs)
}

/// Returns `(out_src, sub_in0_src, alt_src)` and optional shared `Split` root exclusively feeding
/// `BoundaryOut(target_port)` and comparator `port 0` inputs for `target_port`.
fn find_exclusive_fanout_root(
    circuit: &Circuit,
    target_port: u16,
    sub_node: NodeId,
    target_port_subs: &[NodeId],
) -> Option<(NodeId, SignalSource)> {
    let sub_set: BTreeSet<NodeId> = target_port_subs.iter().copied().collect();
    let out_src = circuit
        .edges()
        .iter()
        .find(|e| e.dst == SignalSink::BoundaryOut(target_port))?
        .src;
    let sub0_src = circuit
        .edges()
        .iter()
        .find(|e| {
            e.dst
                == SignalSink::NodeIn {
                    node: sub_node,
                    port: 0,
                }
        })?
        .src;

    let collect_split_ancestors = |start: SignalSource| -> Vec<NodeId> {
        let mut chain = Vec::new();
        let mut curr = start;
        let mut seen = BTreeSet::new();
        while let SignalSource::NodeOut { node, .. } = curr {
            if !seen.insert(node) || circuit.nodes().get(node.index()) != Some(&Primitive::Split) {
                break;
            }
            chain.push(node);
            if let Some(in_e) = circuit
                .edges()
                .iter()
                .find(|e| e.dst == SignalSink::NodeIn { node, port: 0 })
            {
                curr = in_e.src;
            } else {
                break;
            }
        }
        chain
    };

    let c1 = collect_split_ancestors(out_src);
    let c2: BTreeSet<NodeId> = collect_split_ancestors(sub0_src).into_iter().collect();
    let is_exclusive_root = |root: NodeId| -> bool {
        let mut stack = vec![root];
        let mut seen = BTreeSet::new();
        let mut reaches_out = false;
        let mut reaches_sub = false;
        while let Some(s) = stack.pop() {
            if !seen.insert(s) {
                continue;
            }
            for edge in circuit.edges() {
                if let SignalSource::NodeOut { node, .. } = edge.src
                    && node == s
                {
                    match edge.dst {
                        SignalSink::BoundaryOut(q) if q == target_port => {
                            reaches_out = true;
                        }
                        SignalSink::NodeIn { node: dst, port: 0 }
                            if circuit.nodes().get(dst.index()) == Some(&Primitive::Split) =>
                        {
                            stack.push(dst);
                        }
                        SignalSink::NodeIn { node: dst, port: 0 }
                            if dst == sub_node || sub_set.contains(&dst) =>
                        {
                            reaches_sub = true;
                        }
                        _ => return false,
                    }
                }
            }
        }
        reaches_out && reaches_sub
    };

    let mut highest_exclusive: Option<NodeId> = None;
    for &cand in &c1 {
        if c2.contains(&cand) && is_exclusive_root(cand) {
            highest_exclusive = Some(cand);
        }
    }
    let root = highest_exclusive?;
    let in_src = circuit
        .edges()
        .iter()
        .find(|e| {
            e.dst
                == SignalSink::NodeIn {
                    node: root,
                    port: 0,
                }
        })?
        .src;
    Some((root, in_src))
}

/// Swaps `BoundaryOut(target_port)`'s primary driver with comparator `sub_node.in(1)` (`alt`).
///
/// Updates `BoundaryOut(target_port)` and all comparator `in(0)` feeds on `target_port` so the
/// new primary becomes active while moving the previous primary to `sub_node.in(1)`.
pub fn swap_comparator_driver(
    circuit: &mut Circuit,
    target_port: u16,
    sub_node: NodeId,
    target_port_subs: &[NodeId],
) -> bool {
    let Some(alt_src) = circuit
        .edges()
        .iter()
        .find(|e| {
            e.dst
                == SignalSink::NodeIn {
                    node: sub_node,
                    port: 1,
                }
        })
        .map(|e| e.src)
    else {
        return false;
    };

    if let Some((root_split, primary_src)) =
        find_exclusive_fanout_root(circuit, target_port, sub_node, target_port_subs)
    {
        circuit.disconnect(
            primary_src,
            SignalSink::NodeIn {
                node: root_split,
                port: 0,
            },
        );
        circuit.disconnect(
            alt_src,
            SignalSink::NodeIn {
                node: sub_node,
                port: 1,
            },
        );
        circuit.connect(
            alt_src,
            SignalSink::NodeIn {
                node: root_split,
                port: 0,
            },
        );
        circuit.connect(
            primary_src,
            SignalSink::NodeIn {
                node: sub_node,
                port: 1,
            },
        );
        return true;
    }

    let Some(out_src) = circuit
        .edges()
        .iter()
        .find(|e| e.dst == SignalSink::BoundaryOut(target_port))
        .map(|e| e.src)
    else {
        return false;
    };
    let Some(sub0_src) = circuit
        .edges()
        .iter()
        .find(|e| {
            e.dst
                == SignalSink::NodeIn {
                    node: sub_node,
                    port: 0,
                }
        })
        .map(|e| e.src)
    else {
        return false;
    };

    circuit.disconnect(out_src, SignalSink::BoundaryOut(target_port));
    circuit.disconnect(
        sub0_src,
        SignalSink::NodeIn {
            node: sub_node,
            port: 0,
        },
    );
    circuit.disconnect(
        alt_src,
        SignalSink::NodeIn {
            node: sub_node,
            port: 1,
        },
    );
    let split = circuit.add_node(Primitive::Split);
    circuit.connect(
        alt_src,
        SignalSink::NodeIn {
            node: split,
            port: 0,
        },
    );
    circuit.connect(
        SignalSource::NodeOut {
            node: split,
            port: 0,
        },
        SignalSink::BoundaryOut(target_port),
    );
    circuit.connect(
        SignalSource::NodeOut {
            node: split,
            port: 1,
        },
        SignalSink::NodeIn {
            node: sub_node,
            port: 0,
        },
    );
    circuit.connect(
        out_src,
        SignalSink::NodeIn {
            node: sub_node,
            port: 1,
        },
    );
    true
}

fn collect_comparator_live_nodes(
    work: &Circuit,
    sub_node: NodeId,
    eqz_node: NodeId,
    surviving_comparator_nodes: &[(NodeId, NodeId)],
) -> BTreeSet<NodeId> {
    let mut live_nodes = BTreeSet::new();
    let mut live_stack: Vec<NodeId> = Vec::new();
    for edge in work.edges() {
        if let SignalSink::BoundaryOut(_) = edge.dst
            && let SignalSource::NodeOut { node, .. } = edge.src
        {
            live_stack.push(node);
        }
    }
    for &(other_sub, other_eqz) in surviving_comparator_nodes {
        if other_sub != sub_node && other_eqz != eqz_node {
            live_stack.push(other_sub);
            live_stack.push(other_eqz);
        }
    }
    while let Some(node) = live_stack.pop() {
        if !live_nodes.insert(node) {
            continue;
        }
        for edge in work.edges() {
            if let SignalSink::NodeIn { node: dst, .. } = edge.dst
                && dst == node
                && let SignalSource::NodeOut { node: src, .. } = edge.src
            {
                live_stack.push(src);
            }
        }
    }
    live_nodes
}

#[allow(clippy::too_many_arguments)]
fn prepare_comparator_arm_excision(
    circuit: &Circuit,
    sub_node: NodeId,
    eqz_node: NodeId,
    target_port: u16,
    prune_primary: bool,
    target_port_subs: &[NodeId],
    surviving_comparator_nodes: &[(NodeId, NodeId)],
    collapse_splits: bool,
) -> Option<(Circuit, BTreeSet<NodeId>)> {
    let mut work = circuit.clone();
    if prune_primary && !swap_comparator_driver(&mut work, target_port, sub_node, target_port_subs)
    {
        return None;
    }

    let alt_src = work
        .edges()
        .iter()
        .find(|e| {
            e.dst
                == SignalSink::NodeIn {
                    node: sub_node,
                    port: 1,
                }
        })?
        .src;
    let sub0_src = work
        .edges()
        .iter()
        .find(|e| {
            e.dst
                == SignalSink::NodeIn {
                    node: sub_node,
                    port: 0,
                }
        })?
        .src;
    let eqz_in_src = work
        .edges()
        .iter()
        .find(|e| {
            e.dst
                == SignalSink::NodeIn {
                    node: eqz_node,
                    port: 0,
                }
        })?
        .src;

    work.disconnect(
        sub0_src,
        SignalSink::NodeIn {
            node: sub_node,
            port: 0,
        },
    );
    work.disconnect(
        alt_src,
        SignalSink::NodeIn {
            node: sub_node,
            port: 1,
        },
    );
    work.disconnect(
        eqz_in_src,
        SignalSink::NodeIn {
            node: eqz_node,
            port: 0,
        },
    );

    let mut excised_candidates: BTreeSet<NodeId> = [sub_node, eqz_node].into_iter().collect();
    // Collect nodes strictly upstream of `alt_src` BEFORE collapsing Splits.
    let mut stack = vec![alt_src];
    while let Some(src) = stack.pop() {
        if let SignalSource::NodeOut { node, .. } = src {
            if !excised_candidates.insert(node) {
                continue;
            }
            for edge in work.edges() {
                if let SignalSink::NodeIn { node: dst, .. } = edge.dst
                    && dst == node
                {
                    stack.push(edge.src);
                }
            }
        }
    }

    // In Combinational mode, collapse 1-consumer degenerate `Split`s left on `sub0_src` / `alt_src`.
    let mut collapsed_splits = BTreeSet::new();
    if collapse_splits && work.wire_delay() == WireDelayMode::Combinational {
        let mut seed_splits = BTreeSet::new();
        if let SignalSource::NodeOut { node, .. } = sub0_src {
            seed_splits.insert(node);
        }
        if let SignalSource::NodeOut { node, .. } = alt_src {
            seed_splits.insert(node);
        }
        for &cand in &excised_candidates {
            for edge in work.edges() {
                if let SignalSink::NodeIn { node: dst, .. } = edge.dst
                    && dst == cand
                    && let SignalSource::NodeOut { node: parent, .. } = edge.src
                    && work.nodes().get(parent.index()) == Some(&Primitive::Split)
                {
                    seed_splits.insert(parent);
                }
            }
        }
        loop {
            let mut changed = false;
            let current_seeds: Vec<NodeId> = seed_splits.iter().copied().collect();
            for s_node in current_seeds {
                if collapsed_splits.contains(&s_node)
                    || work.nodes().get(s_node.index()) != Some(&Primitive::Split)
                {
                    continue;
                }
                let out_edges: Vec<(SignalSource, SignalSink)> = work
                    .edges()
                    .iter()
                    .filter_map(|e| match e.src {
                        SignalSource::NodeOut { node, .. } if node == s_node => {
                            Some((e.src, e.dst))
                        }
                        _ => None,
                    })
                    .collect();
                let in_edge = work
                    .edges()
                    .iter()
                    .find(|e| {
                        e.dst
                            == SignalSink::NodeIn {
                                node: s_node,
                                port: 0,
                            }
                    })
                    .map(|e| e.src);
                if out_edges.len() == 1
                    && let Some(parent_src) = in_edge
                {
                    let (old_src, sole_dst) = out_edges[0];
                    work.disconnect(
                        parent_src,
                        SignalSink::NodeIn {
                            node: s_node,
                            port: 0,
                        },
                    );
                    work.disconnect(old_src, sole_dst);
                    work.connect(parent_src, sole_dst);
                    collapsed_splits.insert(s_node);
                    excised_candidates.insert(s_node);
                    if let SignalSource::NodeOut { node: p_node, .. } = parent_src {
                        seed_splits.insert(p_node);
                    }
                    changed = true;
                } else if out_edges.is_empty() {
                    if let Some(parent_src) = in_edge {
                        work.disconnect(
                            parent_src,
                            SignalSink::NodeIn {
                                node: s_node,
                                port: 0,
                            },
                        );
                        if let SignalSource::NodeOut { node: p_node, .. } = parent_src {
                            seed_splits.insert(p_node);
                        }
                    }
                    collapsed_splits.insert(s_node);
                    excised_candidates.insert(s_node);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    // Compute all surviving live nodes reachable from BoundaryOut(0..K) and surviving comparators.
    let live_nodes =
        collect_comparator_live_nodes(&work, sub_node, eqz_node, surviving_comparator_nodes);

    let mut removed: BTreeSet<NodeId> = excised_candidates
        .into_iter()
        .filter(|n| !live_nodes.contains(n))
        .collect();
    removed.insert(sub_node);
    removed.insert(eqz_node);
    for s in collapsed_splits {
        removed.insert(s);
    }
    Some((work, removed))
}

/// Returns the set of existing nodes in `circuit` that belong exclusively to a comparator's
/// `primary` (`prune_primary = true`) or `alt` (`prune_primary = false`) branch, excluding the
/// comparator nodes (`sub_node`, `eqz_node`) themselves and any node reachable from surviving
/// boundary outputs or surviving comparators (`M2B-B2`).
#[must_use]
pub fn comparator_exclusive_arm_nodes(
    circuit: &Circuit,
    sub_node: NodeId,
    eqz_node: NodeId,
    target_port: u16,
    prune_primary: bool,
    target_port_subs: &[NodeId],
    surviving_comparator_nodes: &[(NodeId, NodeId)],
) -> Option<BTreeSet<NodeId>> {
    let (_, removed) = prepare_comparator_arm_excision(
        circuit,
        sub_node,
        eqz_node,
        target_port,
        prune_primary,
        target_port_subs,
        surviving_comparator_nodes,
        false,
    )?;
    let orig_count = circuit.node_count();
    Some(
        removed
            .into_iter()
            .filter(|&n| n != sub_node && n != eqz_node && n.index() < orig_count)
            .collect(),
    )
}

/// Excises a comparator (`sub_node`, `eqz_node`) and either its `alt` branch (`prune_primary = false`)
/// or its `primary` branch (`prune_primary = true`, promoting `alt` to `BoundaryOut(target_port)` first).
///
/// Only removes nodes inside the excised branch whose outputs are not reachable from any surviving
/// boundary output or any node in `surviving_comparator_nodes`.
#[must_use]
pub fn excise_comparator_branch(
    circuit: &Circuit,
    sub_node: NodeId,
    eqz_node: NodeId,
    target_port: u16,
    prune_primary: bool,
    target_port_subs: &[NodeId],
    surviving_comparator_nodes: &[(NodeId, NodeId)],
) -> Option<(Circuit, Vec<Option<NodeId>>)> {
    let (mut work, removed) = prepare_comparator_arm_excision(
        circuit,
        sub_node,
        eqz_node,
        target_port,
        prune_primary,
        target_port_subs,
        surviving_comparator_nodes,
        true,
    )?;
    let old_to_new = work.remove_nodes(&removed);
    work.compile().ok()?;
    Some((work, old_to_new))
}

/// Prunes `starved` nodes while bypassing acyclic `Primitive::Split` nodes whose surviving branch
/// feeds exclusively non-starved consumers while the sibling branch contains starved or juvenile work.
///
/// Returns `None` when no starved `Split` qualifies for branch-preserving bypass, allowing the
/// caller to fall back directly to standard `EditableCircuit::prune_starved_nodes`.
#[must_use]
pub fn prune_starved_preserving_healthy_split_branches(
    circuit: &Circuit,
    starved: &BTreeSet<NodeId>,
    juvenile_nodes: &BTreeSet<NodeId>,
) -> Option<(Circuit, Vec<Option<NodeId>>)> {
    if starved.is_empty() {
        return None;
    }

    let mut work = circuit.clone();
    let mut bypassed_splits: BTreeSet<NodeId> = BTreeSet::new();
    let mut dead_seed_nodes: BTreeSet<NodeId> = BTreeSet::new();

    let forward_branch_info =
        |circ: &Circuit, split: NodeId, port: u8| -> (Vec<SignalSink>, BTreeSet<NodeId>, bool) {
            let out_src = SignalSource::NodeOut { node: split, port };
            let sinks: Vec<SignalSink> = circ
                .edges()
                .iter()
                .filter_map(|e| (e.src == out_src).then_some(e.dst))
                .collect();
            let mut visited = BTreeSet::new();
            let mut stack: Vec<NodeId> = sinks
                .iter()
                .filter_map(|dst| match dst {
                    SignalSink::NodeIn { node, .. } => Some(*node),
                    SignalSink::BoundaryOut(_) => None,
                })
                .collect();
            let mut reaches_self = false;
            while let Some(node) = stack.pop() {
                if node == split {
                    reaches_self = true;
                }
                if !visited.insert(node) {
                    continue;
                }
                for edge in circ.edges() {
                    if let SignalSource::NodeOut { node: src, .. } = edge.src
                        && src == node
                        && let SignalSink::NodeIn { node: dst, .. } = edge.dst
                    {
                        stack.push(dst);
                    }
                }
            }
            (sinks, visited, reaches_self)
        };

    loop {
        let mut bypassed_any = false;
        let candidates: Vec<NodeId> = starved
            .iter()
            .copied()
            .filter(|s| !bypassed_splits.contains(s))
            .collect();

        for split in candidates {
            if work.nodes().get(split.index()).copied() != Some(Primitive::Split) {
                continue;
            }
            let Some(in_src) = work
                .edges()
                .iter()
                .find(|e| {
                    e.dst
                        == SignalSink::NodeIn {
                            node: split,
                            port: 0,
                        }
                })
                .map(|e| e.src)
            else {
                continue;
            };
            if let SignalSource::NodeOut { node: src_node, .. } = in_src
                && (starved.contains(&src_node) || bypassed_splits.contains(&src_node))
            {
                continue;
            }

            let (sinks_0, fwd_0, cyc_0) = forward_branch_info(&work, split, 0);
            let (sinks_1, fwd_1, cyc_1) = forward_branch_info(&work, split, 1);
            if cyc_0 || cyc_1 || !fwd_0.is_disjoint(&fwd_1) {
                continue;
            }

            let starved_0 = fwd_0.iter().any(|n| starved.contains(n));
            let starved_1 = fwd_1.iter().any(|n| starved.contains(n));
            let juv_0 = fwd_0.iter().any(|n| juvenile_nodes.contains(n));
            let juv_1 = fwd_1.iter().any(|n| juvenile_nodes.contains(n));

            let selection = if sinks_0.len() == 1 && !starved_0 && starved_1 {
                Some((0u8, 1u8, sinks_0, sinks_1))
            } else if sinks_1.len() == 1 && !starved_1 && starved_0 {
                Some((1u8, 0u8, sinks_1, sinks_0))
            } else if sinks_0.len() == 1
                && sinks_1.len() == 1
                && !starved_0
                && !starved_1
                && !juv_0
                && juv_1
            {
                Some((0u8, 1u8, sinks_0, sinks_1))
            } else if sinks_0.len() == 1
                && sinks_1.len() == 1
                && !starved_0
                && !starved_1
                && juv_0
                && !juv_1
            {
                Some((1u8, 0u8, sinks_1, sinks_0))
            } else {
                None
            };

            let Some((keep_port, dead_port, keep_sinks, dead_sinks)) = selection else {
                continue;
            };

            work.disconnect(
                in_src,
                SignalSink::NodeIn {
                    node: split,
                    port: 0,
                },
            );
            let keep_src = SignalSource::NodeOut {
                node: split,
                port: keep_port,
            };
            for sink in keep_sinks {
                work.disconnect(keep_src, sink);
                work.connect(in_src, sink);
            }
            let dead_src = SignalSource::NodeOut {
                node: split,
                port: dead_port,
            };
            for sink in dead_sinks {
                match sink {
                    SignalSink::BoundaryOut(_) => {
                        work.disconnect(dead_src, sink);
                    }
                    SignalSink::NodeIn { node, .. } => {
                        dead_seed_nodes.insert(node);
                    }
                }
            }
            bypassed_splits.insert(split);
            bypassed_any = true;
        }

        if !bypassed_any {
            break;
        }
    }

    if bypassed_splits.is_empty() {
        return None;
    }

    let mut removed: BTreeSet<NodeId> = bypassed_splits
        .union(starved)
        .copied()
        .collect::<BTreeSet<_>>()
        .union(&dead_seed_nodes)
        .copied()
        .collect();

    loop {
        let mut added = false;
        let snapshot: Vec<(SignalSource, SignalSink)> =
            work.edges().iter().map(|e| (e.src, e.dst)).collect();
        for (src, dst) in snapshot {
            if let SignalSource::NodeOut { node: src_node, .. } = src
                && removed.contains(&src_node)
                && let SignalSink::NodeIn { node: dst_node, .. } = dst
                && removed.insert(dst_node)
            {
                added = true;
            }
        }
        if !added {
            break;
        }
    }

    let boundary_edges_to_drop: Vec<(SignalSource, SignalSink)> = work
        .edges()
        .iter()
        .filter_map(|e| {
            if let SignalSink::BoundaryOut(_) = e.dst
                && let SignalSource::NodeOut { node: src_node, .. } = e.src
                && removed.contains(&src_node)
            {
                Some((e.src, e.dst))
            } else {
                None
            }
        })
        .collect();
    for (src, dst) in boundary_edges_to_drop {
        work.disconnect(src, dst);
    }

    // Collapse any remaining healthy `Split` nodes left single-fanout after removing `dead_seed_nodes`.
    loop {
        let mut collapsed = false;
        for idx in 0..work.node_count() {
            let node = NodeId(idx as u32);
            if removed.contains(&node) || work.nodes()[idx] != Primitive::Split {
                continue;
            }
            let Some(in_src) = work
                .edges()
                .iter()
                .find(|e| e.dst == SignalSink::NodeIn { node, port: 0 })
                .map(|e| e.src)
            else {
                continue;
            };
            let surviving_sinks: Vec<(SignalSource, SignalSink)> = work
                .edges()
                .iter()
                .filter_map(|e| {
                    if let SignalSource::NodeOut { node: src_node, .. } = e.src
                        && src_node == node
                    {
                        let dst_alive = match e.dst {
                            SignalSink::BoundaryOut(_) => true,
                            SignalSink::NodeIn { node: dst_node, .. } => {
                                !removed.contains(&dst_node)
                            }
                        };
                        dst_alive.then_some((e.src, e.dst))
                    } else {
                        None
                    }
                })
                .collect();
            if surviving_sinks.len() <= 1 {
                if surviving_sinks.len() == 1 {
                    let (old_src, dst) = surviving_sinks[0];
                    work.disconnect(old_src, dst);
                    work.connect(in_src, dst);
                }
                work.disconnect(in_src, SignalSink::NodeIn { node, port: 0 });
                removed.insert(node);
                collapsed = true;
            }
        }
        if !collapsed {
            break;
        }
    }

    let old_to_new = work.remove_nodes(&removed);
    work.compile().ok()?;
    Some((work, old_to_new))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::substrate::{ScalarState, Width};

    #[test]
    fn splice_swap_and_excise_preserve_ports_and_cleanup_cleanly() {
        // Primary: Eqz(in:0) -> Out(0); Alt: Delay(in:1) -> Sub/Eqz comparator.
        let mut circ = Circuit::new(Width::BITS_1, 2, 1, WireDelayMode::Combinational);
        let prim_eqz = circ.add_node(Primitive::Eqz);
        circ.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn {
                node: prim_eqz,
                port: 0,
            },
        );
        circ.connect(
            SignalSource::NodeOut {
                node: prim_eqz,
                port: 0,
            },
            SignalSink::BoundaryOut(0),
        );

        let alt_delay = circ.add_node(Primitive::delay_zero());
        circ.connect(
            SignalSource::BoundaryIn(1),
            SignalSink::NodeIn {
                node: alt_delay,
                port: 0,
            },
        );
        let spliced = splice_comparator_onto_port(
            &mut circ,
            0,
            SignalSource::NodeOut {
                node: alt_delay,
                port: 0,
            },
        )
        .expect("splice succeeds");

        let info = derivation_cone_info(&circ, &[SignalSink::BoundaryOut(0)]);
        assert!(info.excl_nodes.contains(&prim_eqz));
        assert!(info.excl_reactive_leaves.contains(&(0, 0)));
        assert!(!info.has_autonomous_cycle);

        let compiled = circ.compile().unwrap();
        let mut state = ScalarState::new(&compiled);
        let mut out = [0u64];
        state.step(&compiled, &[0, 1], &mut out).unwrap();
        assert_eq!(out[0], 1); // Eqz(0) = 1

        // Swap primary and alt: BoundaryOut(0) now outputs Delay(in:1).
        assert!(swap_comparator_driver(&mut circ, 0, spliced.sub_node, &[]));
        let swapped_compiled = circ.compile().unwrap();
        let mut swapped_state = ScalarState::new(&swapped_compiled);
        swapped_state
            .step(&swapped_compiled, &[0, 1], &mut out)
            .unwrap();
        assert_eq!(out[0], 0); // initial Delay(0) = 0
        swapped_state
            .step(&swapped_compiled, &[0, 0], &mut out)
            .unwrap();
        assert_eq!(out[0], 1); // previous tick in:1 = 1

        // Excise old primary (now in comp.alt = Eqz(in:0)) + comparator cleanly.
        let (pruned, mapping) = excise_comparator_branch(
            &circ,
            spliced.sub_node,
            spliced.eqz_node,
            0,
            false,
            &[],
            &[],
        )
        .expect("excise succeeds");
        assert!(mapping[prim_eqz.index()].is_none());
        assert!(mapping[spliced.sub_node.index()].is_none());
        assert!(mapping[spliced.eqz_node.index()].is_none());
        assert!(mapping[alt_delay.index()].is_some());
        assert_eq!(pruned.node_count(), 1);
    }

    #[test]
    fn tph_autonomous_ring_plus_one_phase_adjustment_matches_pre_splice_sequence() {
        let mut pre = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::TicksPerHop);
        let (_, _) = build_autonomous_ring(&mut pre, 3, &[0, 1, 1], 0);
        let pre_compiled = pre.compile().unwrap();
        let mut pre_state = ScalarState::new(&pre_compiled);
        let mut warmup_out = [0u64];
        for _ in 0..7 {
            pre_state
                .step(&pre_compiled, &[0], &mut warmup_out)
                .unwrap();
        }
        let mut ref_state = pre_state.clone();
        let mut expected = Vec::new();
        for _ in 0..24 {
            let mut o = [0u64];
            ref_state.step(&pre_compiled, &[0], &mut o).unwrap();
            expected.push(o[0]);
        }

        let mut staged = pre.clone();
        let alt_ring = build_autonomous_ring_source(&mut staged, 3, &[0, 1, 1]);
        let spliced = splice_comparator_onto_port(&mut staged, 0, alt_ring.2).unwrap();
        assert!(spliced.primary_autonomous_shifted);
        let inserted_split = spliced.inserted_split.unwrap();
        let old_to_new: Vec<Option<NodeId>> = (0..pre.node_count())
            .map(|i| Some(NodeId(i as u32)))
            .collect();
        let staged_compiled = staged.compile().unwrap();
        let adj_regs = advance_tph_autonomous_ring_phase(
            &pre,
            &pre_compiled,
            pre_state.registers(),
            &staged,
            &staged_compiled,
            &old_to_new,
            0,
            inserted_split,
            spliced.sub_node,
        )
        .expect("phase shift returns valid registers");

        let mut staged_state = ScalarState::new(&staged_compiled);
        staged_state
            .set_registers(&staged_compiled, &adj_regs)
            .unwrap();
        let mut actual = Vec::new();
        for _ in 0..24 {
            let mut o = [0u64];
            staged_state.step(&staged_compiled, &[0], &mut o).unwrap();
            actual.push(o[0]);
        }
        assert_eq!(actual, expected);
    }

    #[test]
    fn prune_starved_split_bypass_rescues_healthy_branch_and_rejects_cycles() {
        let width = Width::BITS_1;
        let mut circ = Circuit::new(width, 2, 2, WireDelayMode::Combinational);
        let d1 = circ.add_node(Primitive::Delay { init: 0 });
        let eq0 = circ.add_node(Primitive::Eqz);
        let starved_split = circ.add_node(Primitive::Split);
        let healthy_split = circ.add_node(Primitive::Split);
        let alt_d = circ.add_node(Primitive::Delay { init: 0 });
        let comp_sub = circ.add_node(Primitive::Sub);
        let comp_eqz = circ.add_node(Primitive::Eqz);
        let dead_add = circ.add_node(Primitive::Add);
        let dead_eqz = circ.add_node(Primitive::Eqz);

        let in0_s = tap_source(&mut circ, SignalSource::BoundaryIn(0));
        circ.connect(in0_s, SignalSink::NodeIn { node: d1, port: 0 });
        circ.connect(
            SignalSource::NodeOut { node: d1, port: 0 },
            SignalSink::NodeIn { node: eq0, port: 0 },
        );
        circ.connect(
            SignalSource::NodeOut { node: eq0, port: 0 },
            SignalSink::NodeIn {
                node: starved_split,
                port: 0,
            },
        );
        circ.connect(
            SignalSource::NodeOut {
                node: starved_split,
                port: 0,
            },
            SignalSink::NodeIn {
                node: healthy_split,
                port: 0,
            },
        );
        circ.connect(
            SignalSource::NodeOut {
                node: healthy_split,
                port: 0,
            },
            SignalSink::BoundaryOut(1),
        );
        circ.connect(
            SignalSource::NodeOut {
                node: healthy_split,
                port: 1,
            },
            SignalSink::NodeIn {
                node: comp_sub,
                port: 0,
            },
        );
        let in0_alt = tap_source(&mut circ, SignalSource::BoundaryIn(0));
        circ.connect(
            in0_alt,
            SignalSink::NodeIn {
                node: alt_d,
                port: 0,
            },
        );
        circ.connect(
            SignalSource::NodeOut {
                node: alt_d,
                port: 0,
            },
            SignalSink::NodeIn {
                node: comp_sub,
                port: 1,
            },
        );
        circ.connect(
            SignalSource::NodeOut {
                node: comp_sub,
                port: 0,
            },
            SignalSink::NodeIn {
                node: comp_eqz,
                port: 0,
            },
        );
        circ.connect(
            SignalSource::NodeOut {
                node: starved_split,
                port: 1,
            },
            SignalSink::NodeIn {
                node: dead_add,
                port: 0,
            },
        );
        circ.connect(
            SignalSource::BoundaryIn(1),
            SignalSink::NodeIn {
                node: dead_add,
                port: 1,
            },
        );
        circ.connect(
            SignalSource::NodeOut {
                node: dead_add,
                port: 0,
            },
            SignalSink::NodeIn {
                node: dead_eqz,
                port: 0,
            },
        );
        circ.connect(
            SignalSource::NodeOut {
                node: dead_eqz,
                port: 0,
            },
            SignalSink::BoundaryOut(0),
        );
        circ.compile().unwrap();

        let starved = BTreeSet::from([starved_split, dead_eqz]);
        let (pruned, mapping) =
            prune_starved_preserving_healthy_split_branches(&circ, &starved, &BTreeSet::new())
                .expect("should bypass starved split onto healthy branch");
        assert!(mapping[starved_split.index()].is_none());
        assert!(mapping[dead_add.index()].is_none());
        assert!(mapping[dead_eqz.index()].is_none());
        let new_eq0 = mapping[eq0.index()].unwrap();
        let new_healthy_split = mapping[healthy_split.index()].unwrap();
        assert!(mapping[comp_sub.index()].is_some());
        assert!(mapping[comp_eqz.index()].is_some());
        assert!(pruned.edges().iter().any(|e| {
            e.src
                == SignalSource::NodeOut {
                    node: new_eq0,
                    port: 0,
                }
                && e.dst
                    == SignalSink::NodeIn {
                        node: new_healthy_split,
                        port: 0,
                    }
        }));
        assert!(
            pruned
                .edges()
                .iter()
                .any(|e| e.dst == SignalSink::BoundaryOut(1))
        );
        assert!(
            !pruned
                .edges()
                .iter()
                .any(|e| e.dst == SignalSink::BoundaryOut(0))
        );

        // Must return None when the starved Split is part of an autonomous cycle.
        let mut ring_circ = Circuit::new(width, 1, 1, WireDelayMode::Combinational);
        let (_, ring_split) = build_autonomous_ring(&mut ring_circ, 2, &[0, 1], 0);
        assert!(
            prune_starved_preserving_healthy_split_branches(
                &ring_circ,
                &BTreeSet::from([ring_split]),
                &BTreeSet::new()
            )
            .is_none()
        );
    }
}
