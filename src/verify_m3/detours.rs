//! Synthetic free-rider detour library (`D0`–`D7b`) for Milestone 3 (`H4`, Decision `A5`).
//!
//! Constructs width-aware ($k \in \{1, 2\}$) and wire-delay-aware (`Combinational` and
//! phase-aligned `TicksPerHop`) detour injections on locked primary circuits without altering
//! functional behavior (except `D2OneFire`'s single scheduled tick-0 impulse).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::structural::graph_edit::{
    cone_channel_mask, materialize_const_loop, source_has_cycle, tap_source,
};
use crate::structural::learner::{ArkheLearner, InjectionMode};
use crate::structural::surgery::{
    EditableCircuit, explicit_delay_register_index, register_mapping,
};
use crate::substrate::{
    Circuit, CompiledCircuit, NodeId, Primitive, ScalarState, SignalSink, SignalSource,
    WireDelayMode,
};

/// Nine canonical free-rider / bloat detour classes evaluated in Milestone 3 (`D0`–`D7b`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DetourClass {
    /// Stateful dead branch (`Delay <-> Split -> Eqz`) disconnected from outputs/comparators.
    D0Dead,
    /// Passive additive-identity arm `Add(x, ConstLoop(0))`.
    D1AddZero,
    /// Transient single-tick impulse `Add(x, Delay{init: 1}(ConstLoop(0)))`.
    D2OneFire,
    /// Zero-product arm `Add(x, Mul(osc, ConstLoop(0)))`.
    D3MulZero,
    /// Multiplicative-identity arm `Mul(x, ConstLoop(1))`.
    D4MulOne,
    /// Reconvergent cancelling pair `Sub(Add(x, s_0), s_1)` where `(s_0, s_1) = Split(osc)`.
    D5CancelSub,
    /// Sensitivity-complete bijection chain (`Eqz(Eqz(x))` at $k=1$; `Add(Add(x, 1), mask)` at $k \ge 2$).
    D6Involution,
    /// Unregistered fake comparator `Eqz(Sub(a, a'))` with unconsumed output.
    D7aFakeComparator,
    /// Agreement-gated clone `Mul(a, Eqz(Sub(a, a')))`.
    D7bAgreementGate,
}

impl DetourClass {
    /// All nine detour classes in canonical `D0`–`D7b` order.
    pub const ALL: [Self; 9] = [
        Self::D0Dead,
        Self::D1AddZero,
        Self::D2OneFire,
        Self::D3MulZero,
        Self::D4MulOne,
        Self::D5CancelSub,
        Self::D6Involution,
        Self::D7aFakeComparator,
        Self::D7bAgreementGate,
    ];

    /// Series hop count inserted onto the primary output path in `WireDelayMode::TicksPerHop`.
    #[must_use]
    pub const fn tph_series_hop_delta(self) -> usize {
        match self {
            Self::D0Dead | Self::D7aFakeComparator => 0,
            Self::D1AddZero | Self::D2OneFire | Self::D3MulZero | Self::D4MulOne => 1,
            Self::D5CancelSub | Self::D6Involution | Self::D7bAgreementGate => 2,
        }
    }
}

impl fmt::Display for DetourClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::D0Dead => write!(f, "D0Dead"),
            Self::D1AddZero => write!(f, "D1AddZero"),
            Self::D2OneFire => write!(f, "D2OneFire"),
            Self::D3MulZero => write!(f, "D3MulZero"),
            Self::D4MulOne => write!(f, "D4MulOne"),
            Self::D5CancelSub => write!(f, "D5CancelSub"),
            Self::D6Involution => write!(f, "D6Involution"),
            Self::D7aFakeComparator => write!(f, "D7aFakeComparator"),
            Self::D7bAgreementGate => write!(f, "D7bAgreementGate"),
        }
    }
}

/// Constructed detour edit ready for [`ArkheLearner::apply_external_edit`] and transparency certification.
#[derive(Clone, Debug)]
pub struct DetourEdit {
    pub class: DetourClass,
    pub target_port: u16,
    pub circuit: Circuit,
    pub old_to_new: Vec<Option<NodeId>>,
    pub injected: Vec<NodeId>,
    pub initial_registers: Vec<u64>,
    pub hop_delta: usize,
}

impl DetourEdit {
    /// Applies this detour edit to `learner` under `mode`, aligning `TicksPerHop` pipeline and
    /// autonomous ring registers when `wire_delay == WireDelayMode::TicksPerHop`.
    pub fn apply_to_learner(
        &self,
        learner: &mut ArkheLearner,
        mode: InjectionMode,
    ) -> Result<(), String> {
        let is_tph = self.circuit.wire_delay() == WireDelayMode::TicksPerHop;
        learner
            .apply_external_edit(self.circuit.clone(), &self.old_to_new, &self.injected, mode)
            .map_err(|e| format!("apply_external_edit failed: {e}"))?;
        if is_tph {
            learner
                .editable_mut()
                .set_registers(&self.initial_registers)
                .map_err(|e| format!("set_registers failed: {e}"))?;
        }
        Ok(())
    }
}

fn edge_register_bases(circuit: &Circuit, first_register: usize) -> Vec<usize> {
    let mut next = first_register;
    circuit
        .edges()
        .iter()
        .map(|edge| {
            let base = next;
            if circuit.wire_delay() == WireDelayMode::TicksPerHop {
                next += usize::from(edge.hops);
            }
            base
        })
        .collect()
}

fn set_edge_reg(
    circuit: &Circuit,
    edge_bases: &[usize],
    regs: &mut [u64],
    src: SignalSource,
    dst: SignalSink,
    val: u64,
) {
    if circuit.wire_delay() != WireDelayMode::TicksPerHop {
        return;
    }
    for (idx, edge) in circuit.edges().iter().enumerate() {
        if edge.src == src && edge.dst == dst {
            let base = edge_bases[idx];
            for s in 0..usize::from(edge.hops) {
                if base + s < regs.len() {
                    regs[base + s] = val;
                }
            }
        }
    }
}

fn build_oscillator(circuit: &mut Circuit) -> (NodeId, NodeId, NodeId, SignalSource) {
    let osc_d = circuit.add_node(Primitive::Delay { init: 0 });
    let osc_eqz = circuit.add_node(Primitive::Eqz);
    let osc_sp = circuit.add_node(Primitive::Split);
    circuit.connect(
        SignalSource::NodeOut {
            node: osc_d,
            port: 0,
        },
        SignalSink::NodeIn {
            node: osc_eqz,
            port: 0,
        },
    );
    circuit.connect(
        SignalSource::NodeOut {
            node: osc_eqz,
            port: 0,
        },
        SignalSink::NodeIn {
            node: osc_sp,
            port: 0,
        },
    );
    circuit.connect(
        SignalSource::NodeOut {
            node: osc_sp,
            port: 0,
        },
        SignalSink::NodeIn {
            node: osc_d,
            port: 0,
        },
    );
    (
        osc_d,
        osc_eqz,
        osc_sp,
        SignalSource::NodeOut {
            node: osc_sp,
            port: 1,
        },
    )
}

type ClonedConeInit = (SignalSource, Vec<NodeId>, Vec<(usize, u64)>);
type ClonedRegsInit = (Vec<u64>, Vec<(usize, u64)>, BTreeSet<NodeId>);

fn clone_upstream_cone(
    orig_circuit: &Circuit,
    orig_regs: &[u64],
    root_src: SignalSource,
    edited: &mut Circuit,
    is_tph_autonomous: bool,
) -> Result<ClonedConeInit, String> {
    let mut delay_reg_map = vec![None; orig_circuit.node_count()];
    let mut next_d = 0usize;
    for (idx, prim) in orig_circuit.nodes().iter().enumerate() {
        if prim.is_stateful() {
            delay_reg_map[idx] = Some(next_d);
            next_d += 1;
        }
    }
    let orig_compiled = orig_circuit
        .compile()
        .map_err(|e| format!("orig compile: {e}"))?;
    let orig_edge_bases = edge_register_bases(orig_circuit, orig_compiled.explicit_delay_count());

    let mut cone_nodes = BTreeSet::new();
    let mut stack = Vec::new();
    if let SignalSource::NodeOut { node, .. } = root_src {
        stack.push(node);
    }
    while let Some(node) = stack.pop() {
        if !cone_nodes.insert(node) {
            continue;
        }
        for edge in orig_circuit.edges() {
            if let SignalSink::NodeIn { node: dst, .. } = edge.dst
                && dst == node
                && let SignalSource::NodeOut { node: src, .. } = edge.src
            {
                stack.push(src);
            }
        }
    }

    let mut orig_to_clone: BTreeMap<NodeId, NodeId> = BTreeMap::new();
    let mut added_nodes = Vec::new();
    for &orig_node in &cone_nodes {
        let prim = orig_circuit.nodes()[orig_node.index()];
        let clone_prim = match prim {
            Primitive::Delay { init } => {
                let live_val = delay_reg_map[orig_node.index()]
                    .and_then(|r| orig_regs.get(r).copied())
                    .unwrap_or(init);
                Primitive::Delay { init: live_val }
            }
            other => other,
        };
        let cloned_id = edited.add_node(clone_prim);
        orig_to_clone.insert(orig_node, cloned_id);
        added_nodes.push(cloned_id);
    }

    let mut boundary_taps: BTreeMap<u16, SignalSource> = BTreeMap::new();
    let mut cloned_edge_inits: Vec<(usize, u64)> = Vec::new();

    for (orig_edge_idx, edge) in orig_circuit.edges().iter().enumerate() {
        let SignalSink::NodeIn { node: dst, port } = edge.dst else {
            continue;
        };
        let Some(&cloned_dst) = orig_to_clone.get(&dst) else {
            continue;
        };
        let cloned_src = match edge.src {
            SignalSource::BoundaryIn(c) => {
                if is_tph_autonomous {
                    return Err("autonomous cone unexpectedly referenced BoundaryIn".to_owned());
                }
                if let Some(&tapped) = boundary_taps.get(&c) {
                    tap_source(edited, tapped)
                } else {
                    let pre_n = edited.node_count();
                    let tapped = tap_source(edited, SignalSource::BoundaryIn(c));
                    for idx in pre_n..edited.node_count() {
                        added_nodes.push(NodeId(u32::try_from(idx).expect("fits in u32")));
                    }
                    boundary_taps.insert(c, tapped);
                    tapped
                }
            }
            SignalSource::NodeOut {
                node: src,
                port: sp,
            } => {
                let Some(&cloned_src_node) = orig_to_clone.get(&src) else {
                    return Err(format!("missing cloned source node {src:?}"));
                };
                SignalSource::NodeOut {
                    node: cloned_src_node,
                    port: sp,
                }
            }
        };
        let new_edge_idx = edited.edges().len();
        edited.connect_hops(
            cloned_src,
            SignalSink::NodeIn {
                node: cloned_dst,
                port,
            },
            edge.hops,
        );
        if orig_circuit.wire_delay() == WireDelayMode::TicksPerHop {
            let base = orig_edge_bases[orig_edge_idx];
            if base < orig_regs.len() {
                cloned_edge_inits.push((new_edge_idx, orig_regs[base]));
            }
        }
    }

    // Also include any Splits inserted by `tap_source` if more than 2 consumers used BoundaryIn(c).
    let cloned_root = match root_src {
        SignalSource::BoundaryIn(c) => {
            let pre_n = edited.node_count();
            let tapped = tap_source(edited, SignalSource::BoundaryIn(c));
            for idx in pre_n..edited.node_count() {
                added_nodes.push(NodeId(u32::try_from(idx).expect("fits in u32")));
            }
            tapped
        }
        SignalSource::NodeOut { node, port } => SignalSource::NodeOut {
            node: *orig_to_clone
                .get(&node)
                .ok_or_else(|| "missing cloned root".to_owned())?,
            port,
        },
    };

    Ok((cloned_root, added_nodes, cloned_edge_inits))
}

fn settle_subcircuit_nodes(
    edited: &Circuit,
    compiled: &CompiledCircuit,
    regs: &mut [u64],
    nodes_to_settle: &BTreeSet<NodeId>,
) {
    if nodes_to_settle.is_empty() || edited.wire_delay() != WireDelayMode::TicksPerHop {
        return;
    }
    let mut probe = ScalarState::new(compiled);
    if probe.set_registers(compiled, regs).is_err() {
        return;
    }
    let zeros = vec![0u64; usize::from(compiled.num_inputs())];
    let mut out = vec![0u64; usize::from(compiled.num_outputs())];
    for _ in 0..(edited.node_count() + 4) {
        if probe.step(compiled, &zeros, &mut out).is_err() {
            return;
        }
    }
    let mut explicit_reg = 0usize;
    for (idx, prim) in edited.nodes().iter().enumerate() {
        if prim.is_stateful() {
            let nid = NodeId(u32::try_from(idx).expect("fits"));
            if nodes_to_settle.contains(&nid) && explicit_reg < regs.len() {
                regs[explicit_reg] = probe.registers()[explicit_reg];
            }
            explicit_reg += 1;
        }
    }
    let bases = edge_register_bases(edited, compiled.explicit_delay_count());
    for (e_idx, edge) in edited.edges().iter().enumerate() {
        if let SignalSource::NodeOut { node, .. } = edge.src
            && nodes_to_settle.contains(&node)
        {
            let base = bases[e_idx];
            for s in 0..usize::from(edge.hops) {
                if base + s < regs.len() {
                    regs[base + s] = probe.registers()[base + s];
                }
            }
        }
    }
}

/// Builds a width- and wire-delay-aware free-rider detour edit (`D0`–`D7b`) targeting `target_port`.
///
/// # Errors
///
/// Returns an error string if `target_port` is undriven or a `TicksPerHop` series detour is
/// requested on a non-autonomous reactive cone (Decision `A5`).
pub fn build_detour_edit(
    editable: &EditableCircuit,
    target_port: u16,
    class: DetourClass,
) -> Result<DetourEdit, String> {
    let orig_circuit = editable.circuit();
    let orig_compiled = editable.compiled();
    let orig_regs = editable.state().registers();
    let width = orig_circuit.width();
    let is_tph = orig_circuit.wire_delay() == WireDelayMode::TicksPerHop;
    let out_sink = SignalSink::BoundaryOut(target_port);
    let orig_driver = orig_circuit
        .edges()
        .iter()
        .find(|e| e.dst == out_sink)
        .map(|e| e.src)
        .ok_or_else(|| format!("target_port {target_port} is undriven"))?;

    let is_autonomous = cone_channel_mask(orig_circuit, orig_driver) == 0
        && source_has_cycle(orig_circuit, orig_driver);
    let hop_delta = class.tph_series_hop_delta();
    if is_tph && hop_delta > 0 && !is_autonomous {
        return Err(format!(
            "TicksPerHop series detour {class} requires an autonomous cycle driver (Decision A5)"
        ));
    }

    let pre_n = orig_circuit.node_count();
    let mut edited = orig_circuit.clone();
    let old_to_new: Vec<Option<NodeId>> = (0..pre_n)
        .map(|i| Some(NodeId(u32::try_from(i).expect("fits in u32"))))
        .collect();

    // Check if `orig_driver` is a comparator tap `Split` in TicksPerHop autonomous mode: if so,
    // splice upstream of that `Split` so `BoundaryOut` and `ComparatorSite::sub_node.in(0)` stay aligned.
    let (splice_src, splice_dst) = if is_tph
        && hop_delta > 0
        && let SignalSource::NodeOut { node: sp_node, .. } = orig_driver
        && orig_circuit.nodes().get(sp_node.index()) == Some(&Primitive::Split)
        && orig_circuit.edges().iter().any(|e| {
            matches!(e.src, SignalSource::NodeOut { node, .. } if node == sp_node)
                && matches!(e.dst, SignalSink::NodeIn { node: c, port: 0 }
                    if editable.economy().comparators().iter().any(|comp| comp.sub_node == c))
        }) {
        let in_src = orig_circuit
            .edges()
            .iter()
            .find(|e| {
                e.dst
                    == SignalSink::NodeIn {
                        node: sp_node,
                        port: 0,
                    }
            })
            .map(|e| e.src)
            .ok_or_else(|| "missing Split input".to_owned())?;
        (
            in_src,
            SignalSink::NodeIn {
                node: sp_node,
                port: 0,
            },
        )
    } else {
        (orig_driver, out_sink)
    };

    // Pre-step pre-injection output sequence on `target_port` when `is_tph && is_autonomous`.
    let mut pre_y = Vec::new();
    if is_tph && is_autonomous {
        let mut probe = ScalarState::new(orig_compiled);
        probe
            .set_registers(orig_compiled, orig_regs)
            .map_err(|e| format!("probe set_registers: {e}"))?;
        let zeros = vec![0u64; usize::from(orig_compiled.num_inputs())];
        let mut out_buf = vec![0u64; usize::from(orig_compiled.num_outputs())];
        for _ in 0..16 {
            probe
                .step(orig_compiled, &zeros, &mut out_buf)
                .map_err(|e| format!("probe step: {e}"))?;
            pre_y.push(out_buf[usize::from(target_port)]);
        }
    }

    let mut extra_injected = Vec::new();
    let mut d5_osc_nodes: Option<(NodeId, NodeId, NodeId, NodeId, NodeId)> = None;
    let mut d7b_clone_regs: Option<ClonedRegsInit> = None;
    let mut const_nodes_to_settle: BTreeSet<NodeId> = BTreeSet::new();
    let mut detour_edges: Vec<(SignalSource, SignalSink, u64)> = Vec::new();
    let mut onefire_node: Option<NodeId> = None;

    match class {
        DetourClass::D0Dead => {
            if is_tph {
                // Hop-transparent parallel stateful dead loop in TicksPerHop (both autonomous & reactive).
                let (_, _, osc_sp, osc_out) = build_oscillator(&mut edited);
                let dead_eqz = edited.add_node(Primitive::Eqz);
                edited.connect(
                    osc_out,
                    SignalSink::NodeIn {
                        node: dead_eqz,
                        port: 0,
                    },
                );
                let _ = osc_sp;
            } else {
                // Tap splice_src (or orig_driver) into a stateful dead feedback loop.
                let tapped = tap_source(&mut edited, orig_driver);
                let dead_d = edited.add_node(Primitive::Delay { init: 0 });
                let dead_sp = edited.add_node(Primitive::Split);
                let dead_add = edited.add_node(Primitive::Add);
                let dead_eqz = edited.add_node(Primitive::Eqz);
                edited.connect(
                    tapped,
                    SignalSink::NodeIn {
                        node: dead_add,
                        port: 0,
                    },
                );
                edited.connect(
                    SignalSource::NodeOut {
                        node: dead_sp,
                        port: 0,
                    },
                    SignalSink::NodeIn {
                        node: dead_add,
                        port: 1,
                    },
                );
                edited.connect(
                    SignalSource::NodeOut {
                        node: dead_add,
                        port: 0,
                    },
                    SignalSink::NodeIn {
                        node: dead_d,
                        port: 0,
                    },
                );
                edited.connect(
                    SignalSource::NodeOut {
                        node: dead_d,
                        port: 0,
                    },
                    SignalSink::NodeIn {
                        node: dead_sp,
                        port: 0,
                    },
                );
                edited.connect(
                    SignalSource::NodeOut {
                        node: dead_sp,
                        port: 1,
                    },
                    SignalSink::NodeIn {
                        node: dead_eqz,
                        port: 0,
                    },
                );
            }
        }
        DetourClass::D1AddZero => {
            edited.disconnect(splice_src, splice_dst);
            let before_c = edited.node_count();
            let zero_src = materialize_const_loop(&mut edited, 0);
            for idx in before_c..edited.node_count() {
                const_nodes_to_settle.insert(NodeId(u32::try_from(idx).expect("fits")));
            }
            let add = edited.add_node(Primitive::Add);
            edited.connect(splice_src, SignalSink::NodeIn { node: add, port: 0 });
            edited.connect(zero_src, SignalSink::NodeIn { node: add, port: 1 });
            edited.connect(SignalSource::NodeOut { node: add, port: 0 }, splice_dst);
            if is_tph {
                detour_edges.push((
                    splice_src,
                    SignalSink::NodeIn { node: add, port: 0 },
                    pre_y[1],
                ));
                detour_edges.push((zero_src, SignalSink::NodeIn { node: add, port: 1 }, 0));
                detour_edges.push((
                    SignalSource::NodeOut { node: add, port: 0 },
                    splice_dst,
                    pre_y[0],
                ));
            }
        }
        DetourClass::D2OneFire => {
            edited.disconnect(splice_src, splice_dst);
            let before_c = edited.node_count();
            let zero_src = materialize_const_loop(&mut edited, 0);
            for idx in before_c..edited.node_count() {
                const_nodes_to_settle.insert(NodeId(u32::try_from(idx).expect("fits")));
            }
            let onefire = edited.add_node(Primitive::Delay { init: 1 });
            onefire_node = Some(onefire);
            let add = edited.add_node(Primitive::Add);
            edited.connect(
                zero_src,
                SignalSink::NodeIn {
                    node: onefire,
                    port: 0,
                },
            );
            edited.connect(splice_src, SignalSink::NodeIn { node: add, port: 0 });
            edited.connect(
                SignalSource::NodeOut {
                    node: onefire,
                    port: 0,
                },
                SignalSink::NodeIn { node: add, port: 1 },
            );
            edited.connect(SignalSource::NodeOut { node: add, port: 0 }, splice_dst);
            if is_tph {
                detour_edges.push((
                    splice_src,
                    SignalSink::NodeIn { node: add, port: 0 },
                    pre_y[1],
                ));
                detour_edges.push((
                    zero_src,
                    SignalSink::NodeIn {
                        node: onefire,
                        port: 0,
                    },
                    0,
                ));
                detour_edges.push((
                    SignalSource::NodeOut {
                        node: onefire,
                        port: 0,
                    },
                    SignalSink::NodeIn { node: add, port: 1 },
                    0,
                ));
                detour_edges.push((
                    SignalSource::NodeOut { node: add, port: 0 },
                    splice_dst,
                    width.add(pre_y[0], 1),
                ));
            }
        }
        DetourClass::D3MulZero => {
            edited.disconnect(splice_src, splice_dst);
            let before_c = edited.node_count();
            let zero_src = materialize_const_loop(&mut edited, 0);
            for idx in before_c..edited.node_count() {
                const_nodes_to_settle.insert(NodeId(u32::try_from(idx).expect("fits")));
            }
            let (_, _, _, osc_out) = build_oscillator(&mut edited);
            let mul = edited.add_node(Primitive::Mul);
            let add = edited.add_node(Primitive::Add);
            edited.connect(osc_out, SignalSink::NodeIn { node: mul, port: 0 });
            edited.connect(zero_src, SignalSink::NodeIn { node: mul, port: 1 });
            edited.connect(splice_src, SignalSink::NodeIn { node: add, port: 0 });
            edited.connect(
                SignalSource::NodeOut { node: mul, port: 0 },
                SignalSink::NodeIn { node: add, port: 1 },
            );
            edited.connect(SignalSource::NodeOut { node: add, port: 0 }, splice_dst);
            if is_tph {
                detour_edges.push((
                    splice_src,
                    SignalSink::NodeIn { node: add, port: 0 },
                    pre_y[1],
                ));
                detour_edges.push((
                    SignalSource::NodeOut { node: mul, port: 0 },
                    SignalSink::NodeIn { node: add, port: 1 },
                    0,
                ));
                detour_edges.push((zero_src, SignalSink::NodeIn { node: mul, port: 1 }, 0));
                detour_edges.push((
                    SignalSource::NodeOut { node: add, port: 0 },
                    splice_dst,
                    pre_y[0],
                ));
            }
        }
        DetourClass::D4MulOne => {
            edited.disconnect(splice_src, splice_dst);
            let before_c = edited.node_count();
            let one_src = materialize_const_loop(&mut edited, 1);
            for idx in before_c..edited.node_count() {
                const_nodes_to_settle.insert(NodeId(u32::try_from(idx).expect("fits")));
            }
            let mul = edited.add_node(Primitive::Mul);
            edited.connect(splice_src, SignalSink::NodeIn { node: mul, port: 0 });
            edited.connect(one_src, SignalSink::NodeIn { node: mul, port: 1 });
            edited.connect(SignalSource::NodeOut { node: mul, port: 0 }, splice_dst);
            if is_tph {
                detour_edges.push((
                    splice_src,
                    SignalSink::NodeIn { node: mul, port: 0 },
                    pre_y[1],
                ));
                detour_edges.push((one_src, SignalSink::NodeIn { node: mul, port: 1 }, 1));
                detour_edges.push((
                    SignalSource::NodeOut { node: mul, port: 0 },
                    splice_dst,
                    pre_y[0],
                ));
            }
        }
        DetourClass::D5CancelSub => {
            edited.disconnect(splice_src, splice_dst);
            let (osc_d, osc_eqz, osc_sp, osc_out) = build_oscillator(&mut edited);
            let fan_sp = edited.add_node(Primitive::Split);
            edited.connect(
                osc_out,
                SignalSink::NodeIn {
                    node: fan_sp,
                    port: 0,
                },
            );
            let add = edited.add_node(Primitive::Add);
            let sub = edited.add_node(Primitive::Sub);
            edited.connect(splice_src, SignalSink::NodeIn { node: add, port: 0 });
            edited.connect(
                SignalSource::NodeOut {
                    node: fan_sp,
                    port: 0,
                },
                SignalSink::NodeIn { node: add, port: 1 },
            );
            edited.connect(
                SignalSource::NodeOut { node: add, port: 0 },
                SignalSink::NodeIn { node: sub, port: 0 },
            );
            if is_tph {
                let s1_buf = edited.add_node(Primitive::Split);
                edited.connect(
                    SignalSource::NodeOut {
                        node: fan_sp,
                        port: 1,
                    },
                    SignalSink::NodeIn {
                        node: s1_buf,
                        port: 0,
                    },
                );
                edited.connect(
                    SignalSource::NodeOut {
                        node: s1_buf,
                        port: 0,
                    },
                    SignalSink::NodeIn { node: sub, port: 1 },
                );
                d5_osc_nodes = Some((osc_d, osc_eqz, osc_sp, fan_sp, s1_buf));
                detour_edges.push((
                    splice_src,
                    SignalSink::NodeIn { node: add, port: 0 },
                    pre_y[2],
                ));
                detour_edges.push((
                    SignalSource::NodeOut { node: sub, port: 0 },
                    splice_dst,
                    pre_y[0],
                ));
            } else {
                edited.connect(
                    SignalSource::NodeOut {
                        node: fan_sp,
                        port: 1,
                    },
                    SignalSink::NodeIn { node: sub, port: 1 },
                );
            }
            edited.connect(SignalSource::NodeOut { node: sub, port: 0 }, splice_dst);
        }
        DetourClass::D6Involution => {
            edited.disconnect(splice_src, splice_dst);
            if width.bits() == 1 {
                let eqz1 = edited.add_node(Primitive::Eqz);
                let eqz2 = edited.add_node(Primitive::Eqz);
                edited.connect(
                    splice_src,
                    SignalSink::NodeIn {
                        node: eqz1,
                        port: 0,
                    },
                );
                edited.connect(
                    SignalSource::NodeOut {
                        node: eqz1,
                        port: 0,
                    },
                    SignalSink::NodeIn {
                        node: eqz2,
                        port: 0,
                    },
                );
                edited.connect(
                    SignalSource::NodeOut {
                        node: eqz2,
                        port: 0,
                    },
                    splice_dst,
                );
                if is_tph {
                    detour_edges.push((
                        splice_src,
                        SignalSink::NodeIn {
                            node: eqz1,
                            port: 0,
                        },
                        pre_y[2],
                    ));
                    detour_edges.push((
                        SignalSource::NodeOut {
                            node: eqz1,
                            port: 0,
                        },
                        SignalSink::NodeIn {
                            node: eqz2,
                            port: 0,
                        },
                        width.eqz(pre_y[1]),
                    ));
                    detour_edges.push((
                        SignalSource::NodeOut {
                            node: eqz2,
                            port: 0,
                        },
                        splice_dst,
                        pre_y[0],
                    ));
                }
            } else {
                let before_c1 = edited.node_count();
                let c1 = materialize_const_loop(&mut edited, 1);
                let c_mask = materialize_const_loop(&mut edited, width.mask());
                for idx in before_c1..edited.node_count() {
                    const_nodes_to_settle.insert(NodeId(u32::try_from(idx).expect("fits")));
                }
                let add1 = edited.add_node(Primitive::Add);
                let add2 = edited.add_node(Primitive::Add);
                edited.connect(
                    splice_src,
                    SignalSink::NodeIn {
                        node: add1,
                        port: 0,
                    },
                );
                edited.connect(
                    c1,
                    SignalSink::NodeIn {
                        node: add1,
                        port: 1,
                    },
                );
                edited.connect(
                    SignalSource::NodeOut {
                        node: add1,
                        port: 0,
                    },
                    SignalSink::NodeIn {
                        node: add2,
                        port: 0,
                    },
                );
                edited.connect(
                    c_mask,
                    SignalSink::NodeIn {
                        node: add2,
                        port: 1,
                    },
                );
                edited.connect(
                    SignalSource::NodeOut {
                        node: add2,
                        port: 0,
                    },
                    splice_dst,
                );
                if is_tph {
                    detour_edges.push((
                        splice_src,
                        SignalSink::NodeIn {
                            node: add1,
                            port: 0,
                        },
                        pre_y[2],
                    ));
                    detour_edges.push((
                        c1,
                        SignalSink::NodeIn {
                            node: add1,
                            port: 1,
                        },
                        1,
                    ));
                    detour_edges.push((
                        SignalSource::NodeOut {
                            node: add1,
                            port: 0,
                        },
                        SignalSink::NodeIn {
                            node: add2,
                            port: 0,
                        },
                        width.add(pre_y[1], 1),
                    ));
                    detour_edges.push((
                        c_mask,
                        SignalSink::NodeIn {
                            node: add2,
                            port: 1,
                        },
                        width.mask(),
                    ));
                    detour_edges.push((
                        SignalSource::NodeOut {
                            node: add2,
                            port: 0,
                        },
                        splice_dst,
                        pre_y[0],
                    ));
                }
            }
        }
        DetourClass::D7aFakeComparator => {
            if is_tph {
                // Parallel hop-transparent unregistered fake comparator in TicksPerHop.
                let (_, _, _, osc1_out) = build_oscillator(&mut edited);
                let (_, _, _, osc2_out) = build_oscillator(&mut edited);
                let fake_sub = edited.add_node(Primitive::Sub);
                let fake_eqz = edited.add_node(Primitive::Eqz);
                edited.connect(
                    osc1_out,
                    SignalSink::NodeIn {
                        node: fake_sub,
                        port: 0,
                    },
                );
                edited.connect(
                    osc2_out,
                    SignalSink::NodeIn {
                        node: fake_sub,
                        port: 1,
                    },
                );
                edited.connect(
                    SignalSource::NodeOut {
                        node: fake_sub,
                        port: 0,
                    },
                    SignalSink::NodeIn {
                        node: fake_eqz,
                        port: 0,
                    },
                );
            } else {
                let (cloned_src, clone_nodes, _) = clone_upstream_cone(
                    orig_circuit,
                    orig_regs,
                    orig_driver,
                    &mut edited,
                    is_tph_autonomous_flag(is_tph, is_autonomous),
                )?;
                extra_injected.extend(clone_nodes);
                let tapped = tap_source(&mut edited, orig_driver);
                let fake_sub = edited.add_node(Primitive::Sub);
                let fake_eqz = edited.add_node(Primitive::Eqz);
                edited.connect(
                    tapped,
                    SignalSink::NodeIn {
                        node: fake_sub,
                        port: 0,
                    },
                );
                edited.connect(
                    cloned_src,
                    SignalSink::NodeIn {
                        node: fake_sub,
                        port: 1,
                    },
                );
                edited.connect(
                    SignalSource::NodeOut {
                        node: fake_sub,
                        port: 0,
                    },
                    SignalSink::NodeIn {
                        node: fake_eqz,
                        port: 0,
                    },
                );
            }
        }
        DetourClass::D7bAgreementGate => {
            edited.disconnect(splice_src, splice_dst);
            // Clone `orig_driver` (note: in TicksPerHop autonomous mode, advance clone regs by +1 tick
            // so `cloned_src -> sub.in(1)` aligns with `split_gate.1 -> sub.in(0)`!).
            let clone_base_regs: Vec<u64> = if is_tph && is_autonomous {
                let mut probe = ScalarState::new(orig_compiled);
                probe
                    .set_registers(orig_compiled, orig_regs)
                    .map_err(|e| format!("d7b clone step: {e}"))?;
                let zeros = vec![0u64; usize::from(orig_compiled.num_inputs())];
                let mut dummy = vec![0u64; usize::from(orig_compiled.num_outputs())];
                probe
                    .step(orig_compiled, &zeros, &mut dummy)
                    .map_err(|e| format!("d7b clone step: {e}"))?;
                probe.registers().to_vec()
            } else {
                orig_regs.to_vec()
            };
            let (cloned_src, clone_nodes, cloned_edge_inits) = clone_upstream_cone(
                orig_circuit,
                &clone_base_regs,
                splice_src,
                &mut edited,
                is_tph && is_autonomous,
            )?;
            let clone_set: BTreeSet<NodeId> = clone_nodes.iter().copied().collect();
            extra_injected.extend(clone_nodes);

            let split_gate = edited.add_node(Primitive::Split);
            let sub = edited.add_node(Primitive::Sub);
            let eqz = edited.add_node(Primitive::Eqz);
            let mul = edited.add_node(Primitive::Mul);

            edited.connect(
                splice_src,
                SignalSink::NodeIn {
                    node: split_gate,
                    port: 0,
                },
            );
            edited.connect(
                SignalSource::NodeOut {
                    node: split_gate,
                    port: 0,
                },
                SignalSink::NodeIn { node: mul, port: 0 },
            );
            edited.connect(
                SignalSource::NodeOut {
                    node: split_gate,
                    port: 1,
                },
                SignalSink::NodeIn { node: sub, port: 0 },
            );
            edited.connect(cloned_src, SignalSink::NodeIn { node: sub, port: 1 });
            edited.connect(
                SignalSource::NodeOut { node: sub, port: 0 },
                SignalSink::NodeIn { node: eqz, port: 0 },
            );
            edited.connect(
                SignalSource::NodeOut { node: eqz, port: 0 },
                SignalSink::NodeIn { node: mul, port: 1 },
            );
            edited.connect(SignalSource::NodeOut { node: mul, port: 0 }, splice_dst);
            if is_tph {
                d7b_clone_regs = Some((clone_base_regs, cloned_edge_inits, clone_set));
                detour_edges.push((
                    splice_src,
                    SignalSink::NodeIn {
                        node: split_gate,
                        port: 0,
                    },
                    pre_y[2],
                ));
                detour_edges.push((
                    SignalSource::NodeOut {
                        node: split_gate,
                        port: 0,
                    },
                    SignalSink::NodeIn { node: mul, port: 0 },
                    pre_y[1],
                ));
                detour_edges.push((
                    SignalSource::NodeOut {
                        node: split_gate,
                        port: 1,
                    },
                    SignalSink::NodeIn { node: sub, port: 0 },
                    pre_y[1],
                ));
                detour_edges.push((
                    cloned_src,
                    SignalSink::NodeIn { node: sub, port: 1 },
                    pre_y[1],
                ));
                detour_edges.push((
                    SignalSource::NodeOut { node: sub, port: 0 },
                    SignalSink::NodeIn { node: eqz, port: 0 },
                    0,
                ));
                detour_edges.push((
                    SignalSource::NodeOut { node: eqz, port: 0 },
                    SignalSink::NodeIn { node: mul, port: 1 },
                    1,
                ));
                detour_edges.push((
                    SignalSource::NodeOut { node: mul, port: 0 },
                    splice_dst,
                    pre_y[0],
                ));
            }
        }
    }

    let _ = extra_injected;
    let injected: Vec<NodeId> = (pre_n..edited.node_count())
        .map(|i| NodeId(u32::try_from(i).expect("fits in u32")))
        .collect();
    let edited_compiled = edited
        .compile()
        .map_err(|e| format!("edited compile failed ({class}): {e}"))?;

    // Compute exact phase-aligned initial register vector for `edited`.
    let mut initial_registers = ScalarState::new(&edited_compiled).registers().to_vec();
    let reg_map = register_mapping(
        orig_circuit,
        &edited,
        orig_compiled,
        &edited_compiled,
        &old_to_new,
    );
    let base_for_survivors: Vec<u64> = if is_tph && is_autonomous && hop_delta > 0 {
        let mut probe = ScalarState::new(orig_compiled);
        probe
            .set_registers(orig_compiled, orig_regs)
            .map_err(|e| format!("tph shift set_registers: {e}"))?;
        let zeros = vec![0u64; usize::from(orig_compiled.num_inputs())];
        let mut dummy = vec![0u64; usize::from(orig_compiled.num_outputs())];
        for _ in 0..hop_delta {
            probe
                .step(orig_compiled, &zeros, &mut dummy)
                .map_err(|e| format!("tph shift step: {e}"))?;
        }
        probe.registers().to_vec()
    } else {
        orig_regs.to_vec()
    };
    for (old_r, new_r) in reg_map.iter().enumerate() {
        if let Some(nr) = new_r
            && old_r < base_for_survivors.len()
            && *nr < initial_registers.len()
        {
            initial_registers[*nr] = base_for_survivors[old_r];
        }
    }

    if is_tph {
        let edited_edge_bases =
            edge_register_bases(&edited, edited_compiled.explicit_delay_count());
        // Apply `D7b` cloned cone edge registers (which are +1 tick relative to `orig_regs`, not +2).
        if let Some((_, cloned_edge_inits, _)) = d7b_clone_regs {
            for (edge_idx, val) in cloned_edge_inits {
                let base = edited_edge_bases[edge_idx];
                if base < initial_registers.len() {
                    initial_registers[base] = val;
                }
            }
        }
        // Settle newly added constant subgraphs (`ConstLoop(1)`/`ConstLoop(3)`).
        settle_subcircuit_nodes(
            &edited,
            &edited_compiled,
            &mut initial_registers,
            &const_nodes_to_settle,
        );
        // Settle `D5` oscillator in `TicksPerHop` and align `add -> sub.in(0)`.
        if let Some((osc_d, osc_eqz, osc_sp, fan_sp, s1_buf)) = d5_osc_nodes {
            let osc_set: BTreeSet<NodeId> = [osc_d, osc_eqz, osc_sp, fan_sp, s1_buf]
                .into_iter()
                .collect();
            settle_subcircuit_nodes(&edited, &edited_compiled, &mut initial_registers, &osc_set);
            let mut s1_reg_val = 0u64;
            for (e_idx, edge) in edited.edges().iter().enumerate() {
                if edge.src
                    == (SignalSource::NodeOut {
                        node: s1_buf,
                        port: 0,
                    })
                    && edge.dst
                        == (SignalSink::NodeIn {
                            node: d5_sub_node(&edited, splice_dst),
                            port: 1,
                        })
                {
                    let base = edited_edge_bases[e_idx];
                    if base < initial_registers.len() {
                        s1_reg_val = initial_registers[base];
                    }
                }
            }
            let sub_n = d5_sub_node(&edited, splice_dst);
            let add_n = d5_add_node(&edited, sub_n);
            set_edge_reg(
                &edited,
                &edited_edge_bases,
                &mut initial_registers,
                SignalSource::NodeOut {
                    node: add_n,
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: sub_n,
                    port: 0,
                },
                width.add(pre_y[1], s1_reg_val),
            );
        }
        // Explicitly set detour pipeline edge registers.
        for (src, dst, val) in detour_edges {
            set_edge_reg(
                &edited,
                &edited_edge_bases,
                &mut initial_registers,
                src,
                dst,
                val,
            );
        }
        if let Some(onefire) = onefire_node
            && let Some(r) = explicit_delay_register_index(&edited, onefire)
            && r < initial_registers.len()
        {
            initial_registers[r] = 0;
        }
        // Note: if `splice_dst` was `Split_comp.in(0)`, ensure `Split_comp -> Out` and `Split_comp -> Sub`
        // keep their pre-step `pre_y[0]` value from `orig_regs` rather than `base_for_survivors`!
        if splice_dst != out_sink {
            let orig_edge_bases =
                edge_register_bases(orig_circuit, orig_compiled.explicit_delay_count());
            for (e_idx, edge) in orig_circuit.edges().iter().enumerate() {
                if edge.dst == out_sink
                    || matches!(edge.dst, SignalSink::NodeIn { node, port: 0 }
                        if editable.economy().comparators().iter().any(|c| c.sub_node == node))
                {
                    let orig_base = orig_edge_bases[e_idx];
                    if orig_base < orig_regs.len() {
                        set_edge_reg(
                            &edited,
                            &edited_edge_bases,
                            &mut initial_registers,
                            edge.src,
                            edge.dst,
                            orig_regs[orig_base],
                        );
                    }
                }
            }
        }
    }

    Ok(DetourEdit {
        class,
        target_port,
        circuit: edited,
        old_to_new,
        injected,
        initial_registers,
        hop_delta,
    })
}

const fn is_tph_autonomous_flag(is_tph: bool, is_autonomous: bool) -> bool {
    is_tph && is_autonomous
}

fn d5_sub_node(circuit: &Circuit, splice_dst: SignalSink) -> NodeId {
    for edge in circuit.edges() {
        if edge.dst == splice_dst
            && let SignalSource::NodeOut { node, .. } = edge.src
        {
            return node;
        }
    }
    NodeId(0)
}

fn d5_add_node(circuit: &Circuit, sub_node: NodeId) -> NodeId {
    for edge in circuit.edges() {
        if edge.dst
            == (SignalSink::NodeIn {
                node: sub_node,
                port: 0,
            })
            && let SignalSource::NodeOut { node, .. } = edge.src
        {
            return node;
        }
    }
    NodeId(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::baselines::{PredictorBaseline, TickBudget, certify_circuit_against_spec};
    use crate::env::{FiniteStateGenerator, GeneratorConfig, NoiseModel, NoisyEnvironment};
    use crate::metrics::circuit_code_bits_with_tax;
    use crate::metrics::mdl::{
        EpistemicComparatorTrace, EpistemicTickPhase, epistemic_null_calibrated_mdl_delta_with_tax,
    };
    use crate::rng::SeedBank;
    use crate::structural::economy::{EconomyConfig, ReserveCap};
    use crate::structural::graph_edit::{build_autonomous_ring, splice_comparator_onto_port};
    use crate::structural::growth::{ExcisionMode, GrowthConfig};
    use crate::structural::live::node_output_values;
    use crate::structural::surgery::primary_cone_nodes;
    use crate::substrate::Width;

    const MDL_TOL_BITS_PER_SYM: f64 = 1e-11;

    fn test_economy_uncapped(juvenile_ticks: u32) -> EconomyConfig {
        EconomyConfig {
            juvenile_ticks,
            reserve_cap: ReserveCap::Uncapped,
            ..EconomyConfig::default()
        }
    }

    fn settle_mature_base_editable(ed: &mut EditableCircuit) {
        for node_econ in ed.economy_mut().nodes_mut() {
            node_econ.juvenile_ticks = 0;
            node_econ.debt_paid = true;
        }
    }

    /// Builds a simple 1-channel reactive `In(0) -> Delay{0} -> Out(0)` editable circuit.
    fn build_reactive_delay_editable(
        width: Width,
        wire_delay: WireDelayMode,
        economy: EconomyConfig,
        growth: GrowthConfig,
    ) -> EditableCircuit {
        let mut c = Circuit::new(width, 1, 1, wire_delay);
        let d = c.add_node(Primitive::Delay { init: 0 });
        c.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: d, port: 0 },
        );
        c.connect(
            SignalSource::NodeOut { node: d, port: 0 },
            SignalSink::BoundaryOut(0),
        );
        let mut ed =
            EditableCircuit::new_with_growth(c, economy, &growth).expect("valid reactive circuit");
        settle_mature_base_editable(&mut ed);
        ed
    }

    /// Builds a canonical autonomous periodic delay ring (`cone_channel_mask == 0 && source_has_cycle`),
    /// warming `TicksPerHop` edge pipelines for 24 ticks so registers lie cleanly on the periodic orbit.
    fn build_autonomous_ring_editable(
        width: Width,
        wire_delay: WireDelayMode,
        motif: &[u64],
        economy: EconomyConfig,
        growth: GrowthConfig,
    ) -> EditableCircuit {
        let mut c = Circuit::new(width, 1, 1, wire_delay);
        build_autonomous_ring(&mut c, motif.len(), motif, 0);
        let mut ed =
            EditableCircuit::new_with_growth(c, economy, &growth).expect("valid autonomous ring");
        settle_mature_base_editable(&mut ed);
        if wire_delay == WireDelayMode::TicksPerHop {
            let mut out = vec![0u64; 1];
            for _ in 0..24 {
                ed.step(&[0], &mut out).expect("tph warmup");
            }
        }
        ed
    }

    /// Builds a 2-channel comparator-bearing reactive editable circuit (`ch0 -> Delay -> Split_pri`,
    /// `ch1 -> Delay -> Add(ConstLoop(alt_offset))`, spliced comparator onto port 0).
    fn build_comparator_reactive_editable(
        width: Width,
        alt_offset: u64,
        economy: EconomyConfig,
        growth: GrowthConfig,
    ) -> EditableCircuit {
        let mut c = Circuit::new(width, 2, 1, WireDelayMode::Combinational);
        let d_pri = c.add_node(Primitive::Delay { init: 0 });
        c.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn {
                node: d_pri,
                port: 0,
            },
        );
        c.connect(
            SignalSource::NodeOut {
                node: d_pri,
                port: 0,
            },
            SignalSink::BoundaryOut(0),
        );

        let d_alt = c.add_node(Primitive::Delay { init: 0 });
        c.connect(
            SignalSource::BoundaryIn(1),
            SignalSink::NodeIn {
                node: d_alt,
                port: 0,
            },
        );
        let alt_src = if width.normalize(alt_offset) == 0 {
            SignalSource::NodeOut {
                node: d_alt,
                port: 0,
            }
        } else {
            let c_off = materialize_const_loop(&mut c, width.normalize(alt_offset));
            let add_alt = c.add_node(Primitive::Add);
            c.connect(
                SignalSource::NodeOut {
                    node: d_alt,
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: add_alt,
                    port: 0,
                },
            );
            c.connect(
                c_off,
                SignalSink::NodeIn {
                    node: add_alt,
                    port: 1,
                },
            );
            SignalSource::NodeOut {
                node: add_alt,
                port: 0,
            }
        };
        let comp = splice_comparator_onto_port(&mut c, 0, alt_src).expect("splice comparator");
        let mut ed =
            EditableCircuit::new_with_growth(c, economy, &growth).expect("valid comp circuit");
        ed.register_comparator(comp.sub_node, comp.eqz_node, 0, 1 << 0, 1 << 1)
            .expect("register comparator");
        settle_mature_base_editable(&mut ed);
        ed
    }

    #[test]
    fn detour_functional_transparency_all_classes_k1_k2_and_tph() {
        let growth = GrowthConfig::default();
        let econ = EconomyConfig::default();

        for width in [Width::BITS_1, Width::BITS_2] {
            let motif = if width == Width::BITS_1 {
                vec![0, 0, 0, 0, 1]
            } else {
                vec![0, 1, 3, 2, 1]
            };

            // 1. Reactive Combinational (with & without registered comparator): all 9 classes.
            for with_comp in [false, true] {
                let mut ed = if with_comp {
                    build_comparator_reactive_editable(width, 0, econ, growth)
                } else {
                    build_reactive_delay_editable(width, WireDelayMode::Combinational, econ, growth)
                };
                let card = 1u64 << width.bits();
                let mut out_warm = vec![0u64; 1];
                for t in 0..8u64 {
                    let sym = (t * 3 + 1) % card;
                    let inp = if with_comp { vec![sym, sym] } else { vec![sym] };
                    ed.step(&inp, &mut out_warm).expect("warmup step");
                }
                let pre_regs = ed.state().registers().to_vec();
                let orig_compiled = ed.compiled().clone();

                for &class in &DetourClass::ALL {
                    let edit = build_detour_edit(&ed, 0, class).expect("build detour");
                    assert_eq!(edit.circuit.width(), width);

                    let mut orig_state = ScalarState::new(&orig_compiled);
                    orig_state.set_registers(&orig_compiled, &pre_regs).unwrap();
                    let edit_compiled = edit.circuit.compile().expect("compile edit");
                    let mut edit_state = ScalarState::new(&edit_compiled);
                    edit_state
                        .set_registers(&edit_compiled, &edit.initial_registers)
                        .unwrap();

                    let mut o1 = vec![0u64; 1];
                    let mut o2 = vec![0u64; 1];
                    for t in 0..64u64 {
                        let sym = (t * 5 + 2) % card;
                        let inp = if with_comp { vec![sym, sym] } else { vec![sym] };
                        orig_state.step(&orig_compiled, &inp, &mut o1).unwrap();
                        edit_state.step(&edit_compiled, &inp, &mut o2).unwrap();
                        if class == DetourClass::D2OneFire && t == 0 {
                            assert_eq!(
                                o2[0],
                                width.add(o1[0], 1),
                                "D2 tick-0 impulse check ({width:?}, comp={with_comp})"
                            );
                        } else {
                            assert_eq!(
                                o2[0], o1[0],
                                "transparency mismatch: class={class} ({width:?}, comp={with_comp}) tick={t}"
                            );
                        }
                    }
                }
            }

            // 2. Autonomous periodic ring (Combinational & TicksPerHop): all 9 classes + Combinational spec cert.
            for wire_delay in [WireDelayMode::Combinational, WireDelayMode::TicksPerHop] {
                let ed = build_autonomous_ring_editable(width, wire_delay, &motif, econ, growth);
                let pre_regs = ed.state().registers().to_vec();
                let orig_compiled = ed.compiled().clone();
                let spec_gen = GeneratorConfig::T1_PeriodicMotif {
                    motif: motif.clone(),
                }
                .build(width)
                .unwrap();
                let spec = spec_gen.to_finite_state_spec().expect("spec");

                for &class in &DetourClass::ALL {
                    let edit = build_detour_edit(&ed, 0, class).expect("build T1 detour");
                    let mut orig_state = ScalarState::new(&orig_compiled);
                    orig_state.set_registers(&orig_compiled, &pre_regs).unwrap();
                    let edit_compiled = edit.circuit.compile().expect("compile edit");
                    let mut edit_state = ScalarState::new(&edit_compiled);
                    edit_state
                        .set_registers(&edit_compiled, &edit.initial_registers)
                        .unwrap();

                    let mut o1 = vec![0u64; 1];
                    let mut o2 = vec![0u64; 1];
                    for t in 0..64u64 {
                        orig_state.step(&orig_compiled, &[0], &mut o1).unwrap();
                        edit_state.step(&edit_compiled, &[0], &mut o2).unwrap();
                        if class == DetourClass::D2OneFire && t == 0 {
                            assert_eq!(
                                o2[0],
                                width.add(o1[0], 1),
                                "D2 T1 tick-0 impulse ({width:?}, {wire_delay:?})"
                            );
                        } else {
                            assert_eq!(
                                o2[0], o1[0],
                                "T1 transparency mismatch: class={class} ({width:?}, {wire_delay:?}) tick={t}"
                            );
                        }
                    }

                    if wire_delay == WireDelayMode::Combinational && class != DetourClass::D2OneFire
                    {
                        assert!(
                            certify_circuit_against_spec(&edit_compiled, &spec, 4_096).is_some(),
                            "expected formal spec certification for {class} ({width:?})"
                        );
                    }
                }
            }

            // 3. Reactive TicksPerHop: hop_delta == 0 parallel detours (`D0Dead`, `D7aFakeComparator`).
            let mut ed_tph =
                build_reactive_delay_editable(width, WireDelayMode::TicksPerHop, econ, growth);
            let card = 1u64 << width.bits();
            let mut out_w = vec![0u64; 1];
            for t in 0..6u64 {
                ed_tph.step(&[(t + 1) % card], &mut out_w).unwrap();
            }
            let pre_regs = ed_tph.state().registers().to_vec();
            let orig_compiled = ed_tph.compiled().clone();
            for class in [DetourClass::D0Dead, DetourClass::D7aFakeComparator] {
                let edit = build_detour_edit(&ed_tph, 0, class).expect("build reactive tph");
                let mut orig_state = ScalarState::new(&orig_compiled);
                orig_state.set_registers(&orig_compiled, &pre_regs).unwrap();
                let edit_compiled = edit.circuit.compile().expect("compile reactive tph");
                let mut edit_state = ScalarState::new(&edit_compiled);
                edit_state
                    .set_registers(&edit_compiled, &edit.initial_registers)
                    .unwrap();
                let mut o1 = vec![0u64; 1];
                let mut o2 = vec![0u64; 1];
                for t in 0..48u64 {
                    let inp = [(t * 3 + 1) % card];
                    orig_state.step(&orig_compiled, &inp, &mut o1).unwrap();
                    edit_state.step(&edit_compiled, &inp, &mut o2).unwrap();
                    assert_eq!(
                        o2[0], o1[0],
                        "reactive TicksPerHop mismatch for {class} ({width:?}) tick={t}"
                    );
                }
            }
        }
    }

    #[test]
    fn detour_epistemic_mdl_conservation_established_and_juvenile() {
        let econ_cfg = test_economy_uncapped(EconomyConfig::default().juvenile_ticks);
        let growth_cfg = GrowthConfig::default();

        // Part A: Exact epistemic MDL conservation (`<= 1e-11` b/sym) across injection + excision
        // on autonomous periodic rings (`k in {1, 2}`, clean & `eps=0.05`, Established & Juvenile, all 9 classes).
        for width in [Width::BITS_1, Width::BITS_2] {
            let motif = if width == Width::BITS_1 {
                vec![0, 0, 0, 0, 1]
            } else {
                vec![0, 1, 3, 2, 1]
            };
            let base_ed = build_autonomous_ring_editable(
                width,
                WireDelayMode::Combinational,
                &motif,
                econ_cfg,
                growth_cfg,
            );
            for &noisy_eps in &[0.0f64, 0.05f64] {
                for &mode in &[InjectionMode::Established, InjectionMode::Juvenile] {
                    let classes: &[DetourClass] = if noisy_eps == 0.0 {
                        &DetourClass::ALL
                    } else {
                        &[
                            DetourClass::D0Dead,
                            DetourClass::D2OneFire,
                            DetourClass::D3MulZero,
                            DetourClass::D5CancelSub,
                            DetourClass::D6Involution,
                            DetourClass::D7bAgreementGate,
                        ]
                    };
                    for &class in classes {
                        let generator = GeneratorConfig::T1_PeriodicMotif {
                            motif: motif.clone(),
                        }
                        .build(width)
                        .unwrap();
                        let noise = if noisy_eps > 0.0 {
                            NoiseModel::IndependentSubstitution {
                                epsilon: noisy_eps,
                                channels: vec![0],
                            }
                        } else {
                            NoiseModel::None
                        };
                        let mut env = NoisyEnvironment::new(generator, noise, SeedBank::new(77))
                            .expect("env");
                        let mut learner = ArkheLearner::from_editable(
                            base_ed.clone(),
                            growth_cfg,
                            SeedBank::new(19),
                        )
                        .expect("learner");
                        let mut budget = TickBudget::new(25_000_000);

                        let pre_ticks = 32usize;
                        let post_ticks = match mode {
                            InjectionMode::Established => 256usize,
                            InjectionMode::Juvenile => 512usize,
                        };
                        let total_ticks = pre_ticks + post_ticks;
                        let mut facts = Vec::with_capacity(total_ticks);
                        let mut preds = Vec::with_capacity(total_ticks);
                        let mut phases = Vec::with_capacity(total_ticks);
                        let mut comp_pri = Vec::with_capacity(total_ticks);
                        let mut comp_alt = Vec::with_capacity(total_ticks);
                        let mut comp_reg_tick: Option<usize> = None;
                        let mut comp_masks: Option<(u64, u64)> = None;
                        let mut clean = vec![0u64; 1];
                        let mut obs = vec![0u64; 1];

                        for tick_idx in 0..pre_ticks {
                            env.step_frame(&mut clean, &mut obs);
                            let p = learner.predict(&mut budget);
                            preds.push(p[0]);
                            facts.push(obs[0]);
                            phases.push(EpistemicTickPhase::Wake);
                            if let Some(comp) = learner.editable().economy().comparators().first() {
                                let wires = learner.editable().state().wire_values();
                                let sub_in = &learner.editable().compiled().node_input_wires()
                                    [comp.sub_node.index()];
                                comp_pri.push(width.normalize(wires[sub_in[0]]));
                                comp_alt.push(width.normalize(wires[sub_in[1]]));
                            } else {
                                comp_pri.push(0);
                                comp_alt.push(0);
                            }
                            learner.observe(&obs, &mut budget);
                            if comp_reg_tick.is_none()
                                && let Some(comp) =
                                    learner.editable().economy().comparators().first()
                            {
                                comp_reg_tick = Some(tick_idx + 1);
                                comp_masks =
                                    Some((comp.primary_channel_mask, comp.alt_channel_mask));
                            }
                        }

                        let pre_inj_bits = circuit_code_bits_with_tax(
                            learner.editable().circuit(),
                            0,
                            econ_cfg.split_code_tax,
                        );
                        let debt_before_inj = learner.telemetry().cumulative_maturity_debt_paid;
                        let edit =
                            build_detour_edit(learner.editable(), 0, class).expect("detour edit");
                        let post_inj_bits =
                            circuit_code_bits_with_tax(&edit.circuit, 0, econ_cfg.split_code_tax);
                        edit.apply_to_learner(&mut learner, mode)
                            .expect("apply edit");

                        let expected_exog = match mode {
                            InjectionMode::Established => post_inj_bits - pre_inj_bits,
                            InjectionMode::Juvenile => 0.0,
                        };
                        assert!(
                            (learner.telemetry().exogenous_injection_bits - expected_exog).abs()
                                <= 1e-11,
                            "exogenous_injection_bits mismatch ({class}, {mode:?}): got {}, exp {}",
                            learner.telemetry().exogenous_injection_bits,
                            expected_exog
                        );
                        for &inj in &edit.injected {
                            let econ_node = &learner.editable().economy().nodes()[inj.index()];
                            match mode {
                                InjectionMode::Established => {
                                    assert_eq!(econ_node.juvenile_ticks, 0);
                                    assert!(econ_node.debt_paid);
                                }
                                InjectionMode::Juvenile => {
                                    assert!(econ_node.juvenile_ticks > 0);
                                    assert!(!econ_node.debt_paid);
                                }
                            }
                        }

                        for offset_t in 0..post_ticks {
                            let tick_idx = pre_ticks + offset_t;
                            env.step_frame(&mut clean, &mut obs);
                            let p = learner.predict(&mut budget);
                            preds.push(p[0]);
                            facts.push(obs[0]);
                            phases.push(EpistemicTickPhase::Wake);
                            if let Some(comp) = learner.editable().economy().comparators().first() {
                                let wires = learner.editable().state().wire_values();
                                let sub_in = &learner.editable().compiled().node_input_wires()
                                    [comp.sub_node.index()];
                                comp_pri.push(width.normalize(wires[sub_in[0]]));
                                comp_alt.push(width.normalize(wires[sub_in[1]]));
                            } else {
                                comp_pri.push(0);
                                comp_alt.push(0);
                            }
                            learner.observe(&obs, &mut budget);
                            if comp_reg_tick.is_none()
                                && let Some(comp) =
                                    learner.editable().economy().comparators().first()
                            {
                                comp_reg_tick = Some(tick_idx + 1);
                                comp_masks =
                                    Some((comp.primary_channel_mask, comp.alt_channel_mask));
                            }
                        }

                        let telem = learner.telemetry();
                        let comp_traces: Vec<EpistemicComparatorTrace<'_>> =
                            if let (Some(reg_tick), Some((pm, am))) = (comp_reg_tick, comp_masks) {
                                vec![EpistemicComparatorTrace {
                                    target_port: 0,
                                    registered_at_tick: reg_tick,
                                    primary_channel_mask: pm,
                                    alt_channel_mask: am,
                                    primary_predictions: &comp_pri,
                                    alt_predictions: &comp_alt,
                                }]
                            } else {
                                Vec::new()
                            };
                        let ref_mdl_delta = epistemic_null_calibrated_mdl_delta_with_tax(
                            learner.editable().circuit(),
                            0.0,
                            0,
                            &[&facts],
                            &[&preds],
                            &phases,
                            &comp_traces,
                            econ_cfg.null_model,
                            econ_cfg.split_code_tax,
                        )
                        .expect("ref epistemic mdl");
                        let err_b_per_sym = (telem.null_calibrated_mdl_delta_bits - ref_mdl_delta)
                            .abs()
                            / (total_ticks as f64);
                        assert!(
                            err_b_per_sym <= MDL_TOL_BITS_PER_SYM,
                            "epistemic MDL conservation err {err_b_per_sym:.3e} > 1e-11 ({class}, {width:?}, eps={noisy_eps}, {mode:?}) idle={} starv={} local={} sprout={} red={} comp={} comp_flux={} n={}",
                            telem.idle_prune_events,
                            telem.starvation_prune_events,
                            telem.local_excision_events,
                            telem.sprout_events,
                            telem.redundancy_sprout_events,
                            telem.active_comparator_count,
                            telem.cumulative_comparator_flux,
                            telem.final_node_count,
                        );
                        if mode == InjectionMode::Established && noisy_eps == 0.0 {
                            assert!(
                                (telem.cumulative_maturity_debt_paid - debt_before_inj).abs()
                                    <= 1e-11,
                                "Established detour double-billed maturity debt ({class}): before={debt_before_inj}, after={}",
                                telem.cumulative_maturity_debt_paid
                            );
                        }
                    }
                }
            }
        }

        // Part B: Endogenous N=0 growth -> Established & Juvenile detour injection (verifies exogenous credit & debt tracking)
        // AND Comparator-bearing multi-view reactive circuit (`k=2`, clean & eps=0.05) exact epistemic + reserve conservation.
        {
            let mut generator = GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 0, 0, 0, 1],
            }
            .build(Width::BITS_1)
            .unwrap();
            let mut learner = ArkheLearner::new_with_config(
                Width::BITS_1,
                1,
                WireDelayMode::Combinational,
                econ_cfg,
                growth_cfg,
                SeedBank::new(91),
            )
            .expect("N=0 learner");
            let mut budget = TickBudget::new(25_000_000);
            let mut frame = vec![0u64; 1];
            for _ in 0..320 {
                generator.next_clean_frame(&mut frame);
                let _ = learner.predict(&mut budget);
                learner.observe(&frame, &mut budget);
            }
            let pre_bits = circuit_code_bits_with_tax(
                learner.editable().circuit(),
                0,
                econ_cfg.split_code_tax,
            );
            let edit = build_detour_edit(learner.editable(), 0, DetourClass::D5CancelSub)
                .expect("N=0 D5 edit");
            let post_bits = circuit_code_bits_with_tax(&edit.circuit, 0, econ_cfg.split_code_tax);
            let debt_pre = learner.telemetry().cumulative_maturity_debt_paid;
            edit.apply_to_learner(&mut learner, InjectionMode::Established)
                .expect("apply N=0 edit");
            assert!((learner.exogenous_injection_bits() - (post_bits - pre_bits)).abs() <= 1e-11);
            for _ in 0..256 {
                generator.next_clean_frame(&mut frame);
                let _ = learner.predict(&mut budget);
                learner.observe(&frame, &mut budget);
            }
            assert!((learner.telemetry().cumulative_maturity_debt_paid - debt_pre).abs() <= 1e-11);
            assert!(
                circuit_code_bits_with_tax(
                    learner.editable().circuit(),
                    0,
                    econ_cfg.split_code_tax,
                ) <= pre_bits + 1e-11
            );
        }

        for &noisy in &[false, true] {
            for &mode in &[InjectionMode::Established, InjectionMode::Juvenile] {
                for class in [
                    DetourClass::D1AddZero,
                    DetourClass::D4MulOne,
                    DetourClass::D5CancelSub,
                    DetourClass::D7bAgreementGate,
                ] {
                    let ed = build_comparator_reactive_editable(
                        Width::BITS_2,
                        1,
                        test_economy_uncapped(econ_cfg.juvenile_ticks),
                        growth_cfg,
                    );
                    let init_e_sum: f64 = ed.economy().nodes().iter().map(|n| n.energy).sum();
                    let mut learner =
                        ArkheLearner::from_editable(ed, growth_cfg, SeedBank::new(41))
                            .expect("from_editable");
                    let mut budget = TickBudget::new(5_000_000);

                    let total_ticks = 96usize;
                    let inj_tick = 24usize;
                    let pre_excise_check_tick = inj_tick + 32;
                    let mut facts = Vec::with_capacity(total_ticks);
                    let mut preds = Vec::with_capacity(total_ticks);
                    let mut phases = Vec::with_capacity(total_ticks);
                    let mut comp_pri = Vec::with_capacity(total_ticks);
                    let mut comp_alt = Vec::with_capacity(total_ticks);
                    let mut total_endowments = init_e_sum;

                    for t in 0..total_ticks {
                        if t == inj_tick {
                            let pre_n = learner.editable().circuit().node_count();
                            let edit = build_detour_edit(learner.editable(), 0, class)
                                .expect("comp detour");
                            let post_n = edit.circuit.node_count();
                            total_endowments += ((post_n - pre_n) as f64) * econ_cfg.initial_energy;
                            edit.apply_to_learner(&mut learner, mode)
                                .expect("apply comp detour");
                        }
                        let base_ch0 = if t.is_multiple_of(2) { 1u64 } else { 3u64 };
                        let ch0 = if noisy && (t % 20 == 7) {
                            (base_ch0 + 1) & 3
                        } else {
                            base_ch0
                        };
                        let ch1 = Width::BITS_2.sub(ch0, 1);

                        let p = learner.predict(&mut budget);
                        preds.push(p[0]);
                        let wires = learner.editable().state().wire_values().to_vec();
                        let comp = &learner.editable().economy().comparators()[0];
                        let sub_in = &learner.editable().compiled().node_input_wires()
                            [comp.sub_node.index()];
                        comp_pri.push(Width::BITS_2.normalize(wires[sub_in[0]]));
                        comp_alt.push(Width::BITS_2.normalize(wires[sub_in[1]]));

                        let obs_frame = [ch0, ch1];
                        facts.push(obs_frame[0]);
                        phases.push(EpistemicTickPhase::Wake);
                        learner.observe(&obs_frame, &mut budget);

                        if t == pre_excise_check_tick {
                            let mid_telem = learner.telemetry();
                            if mid_telem.idle_prune_events == 0
                                && mid_telem.starvation_prune_events == 0
                                && !(noisy && mode == InjectionMode::Juvenile)
                            {
                                let e_err = ((mid_telem.final_total_energy
                                    + mid_telem.cumulative_maturity_debt_paid
                                    - total_endowments)
                                    - mid_telem.cumulative_net_flux)
                                    .abs()
                                    / ((t + 1) as f64);
                                assert!(
                                    e_err <= MDL_TOL_BITS_PER_SYM,
                                    "pre-excision reserve conservation err {e_err:.3e} > 1e-11 ({class}, noisy={noisy}, {mode:?})"
                                );
                            }
                        }
                    }

                    let telem = learner.telemetry();
                    let comp_trace = [EpistemicComparatorTrace {
                        target_port: 0,
                        registered_at_tick: 0,
                        primary_channel_mask: 1 << 0,
                        alt_channel_mask: 1 << 1,
                        primary_predictions: &comp_pri,
                        alt_predictions: &comp_alt,
                    }];
                    let ref_mdl_delta = epistemic_null_calibrated_mdl_delta_with_tax(
                        learner.editable().circuit(),
                        0.0,
                        0,
                        &[&facts],
                        &[&preds],
                        &phases,
                        &comp_trace,
                        econ_cfg.null_model,
                        econ_cfg.split_code_tax,
                    )
                    .expect("comp ref mdl");
                    let err_b_per_sym = (telem.null_calibrated_mdl_delta_bits - ref_mdl_delta)
                        .abs()
                        / (total_ticks as f64);
                    assert!(
                        err_b_per_sym <= MDL_TOL_BITS_PER_SYM,
                        "comparator MDL err {err_b_per_sym:.3e} > 1e-11 ({class}, noisy={noisy}, {mode:?})"
                    );
                    let _ = node_output_values(
                        learner.editable().circuit(),
                        learner.editable().state(),
                    );
                }
            }
        }
    }

    #[test]
    fn verified_local_excision_cleans_all_classes_without_collapse() {
        let growth = GrowthConfig::default();

        // 1. Autonomous periodic ring (`Combinational` & `TicksPerHop`, `k in {1, 2}`) + noisy `eps = 0.05`:
        //    all 9 classes excised cleanly in both Established (`<= 256` ticks) and Juvenile (`<= 768` ticks).
        for width in [Width::BITS_1, Width::BITS_2] {
            let motif = if width == Width::BITS_1 {
                vec![0, 0, 0, 0, 1]
            } else {
                vec![0, 1, 3, 2, 1]
            };
            for wire_delay in [WireDelayMode::Combinational, WireDelayMode::TicksPerHop] {
                let base_ed = build_autonomous_ring_editable(
                    width,
                    wire_delay,
                    &motif,
                    EconomyConfig::default(),
                    growth,
                );
                let pre_node_count = base_ed.circuit().node_count();
                let pre_code_bits = circuit_code_bits_with_tax(
                    base_ed.circuit(),
                    0,
                    EconomyConfig::default().split_code_tax,
                );
                let pre_pri_len = primary_cone_nodes(base_ed.circuit(), base_ed.economy(), 0).len();

                for &mode in &[InjectionMode::Established, InjectionMode::Juvenile] {
                    let horizon = match mode {
                        InjectionMode::Established => 256usize,
                        InjectionMode::Juvenile => 768usize,
                    };
                    for &class in &DetourClass::ALL {
                        let noisy = wire_delay == WireDelayMode::Combinational
                            && matches!(
                                class,
                                DetourClass::D0Dead
                                    | DetourClass::D5CancelSub
                                    | DetourClass::D6Involution
                                    | DetourClass::D7bAgreementGate
                            )
                            && mode == InjectionMode::Established;
                        let case_growth = GrowthConfig {
                            redundancy_sprouting: !noisy && growth.redundancy_sprouting,
                            ..growth
                        };
                        let mut learner = ArkheLearner::from_editable(
                            base_ed.clone(),
                            case_growth,
                            SeedBank::new(31),
                        )
                        .expect("clone from editable");
                        let mut budget = TickBudget::new(25_000_000);
                        let clean_probe = base_ed.compiled().clone();
                        let mut clean_state = ScalarState::new(&clean_probe);
                        clean_state
                            .set_registers(&clean_probe, base_ed.state().registers())
                            .unwrap();
                        let mut clean_out = vec![0u64; 1];
                        for _ in 0..32 {
                            clean_state
                                .step(&clean_probe, &[0], &mut clean_out)
                                .unwrap();
                            let _ = learner.predict(&mut budget);
                            learner.observe(&[clean_out[0]], &mut budget);
                        }

                        let edit =
                            build_detour_edit(learner.editable(), 0, class).expect("T1 detour");
                        edit.apply_to_learner(&mut learner, mode)
                            .expect("apply detour");

                        for t in 0..horizon {
                            clean_state
                                .step(&clean_probe, &[0], &mut clean_out)
                                .unwrap();
                            let obs_val = if noisy && (t * 17 + 3).is_multiple_of(20) {
                                width.add(clean_out[0], 1)
                            } else {
                                clean_out[0]
                            };
                            let pred = learner.predict(&mut budget);
                            if t > horizon - 32 && !noisy {
                                assert_eq!(
                                    pred[0], clean_out[0],
                                    "post-excision prediction mismatch ({class}, {width:?}, {wire_delay:?}, {mode:?}) t={t}"
                                );
                            }
                            learner.observe(&[obs_val], &mut budget);
                        }

                        let post_n = learner.editable().circuit().node_count();
                        let post_bits = circuit_code_bits_with_tax(
                            learner.editable().circuit(),
                            0,
                            EconomyConfig::default().split_code_tax,
                        );
                        let post_pri_len = primary_cone_nodes(
                            learner.editable().circuit(),
                            learner.editable().economy(),
                            0,
                        )
                        .len();
                        assert!(
                            post_n <= pre_node_count && post_bits <= pre_code_bits + 1e-11,
                            "detour not excised: {class} ({width:?}, {wire_delay:?}, {mode:?}) pre=({pre_node_count},{pre_code_bits}) post=({post_n},{post_bits}) idle={} starv={} local={} sprout={} red={} comp={} nodes={:?}",
                            learner.idle_prune_events(),
                            learner.starvation_prune_events(),
                            learner.local_excision_events(),
                            learner.sprout_events(),
                            learner.redundancy_sprout_events(),
                            learner.active_comparator_count(),
                            learner.editable().circuit().nodes(),
                        );
                        assert!(
                            post_pri_len > 0 && post_pri_len <= pre_pri_len,
                            "primary cone abnormal ({class}, {width:?}, {wire_delay:?}, {mode:?}): pre={pre_pri_len}, post={post_pri_len}"
                        );
                        assert!(
                            learner.idle_prune_events() > 0
                                || learner.starvation_prune_events() > 0,
                            "expected excision event for {class} ({mode:?})"
                        );
                        if matches!(class, DetourClass::D5CancelSub | DetourClass::D6Involution) {
                            assert!(
                                learner.local_excision_events() > 0,
                                "expected M3-E2 local excision for {class} ({width:?}, {wire_delay:?}, {mode:?}): local={}, idle={}, starv={}",
                                learner.local_excision_events(),
                                learner.idle_prune_events(),
                                learner.starvation_prune_events(),
                            );
                        }
                    }
                }
            }
        }

        // 1b. Direct M3-E2 `excise_verified_passthrough_subgraphs` check on `D7bAgreementGate`
        //     (`k in {1, 2}`, `Combinational` & `TicksPerHop`) verifying Step 1–3 cloned-cone certification.
        for width in [Width::BITS_1, Width::BITS_2] {
            let motif = if width == Width::BITS_1 {
                vec![0, 0, 0, 0, 1]
            } else {
                vec![0, 1, 3, 2, 1]
            };
            for wire_delay in [WireDelayMode::Combinational, WireDelayMode::TicksPerHop] {
                let mut ed = build_autonomous_ring_editable(
                    width,
                    wire_delay,
                    &motif,
                    EconomyConfig::default(),
                    growth,
                );
                let pre_n = ed.circuit().node_count();
                let pre_bits = circuit_code_bits_with_tax(
                    ed.circuit(),
                    0,
                    EconomyConfig::default().split_code_tax,
                );
                let edit =
                    build_detour_edit(&ed, 0, DetourClass::D7bAgreementGate).expect("D7b detour");
                ed.recompile_preserving_state(edit.circuit, &edit.old_to_new)
                    .expect("apply D7b");
                if wire_delay == WireDelayMode::TicksPerHop {
                    ed.set_registers(&edit.initial_registers).expect("regs");
                }
                settle_mature_base_editable(&mut ed);
                let init_regs = ed.state().registers().to_vec();
                let w_diag = growth.diagnostic_window;
                let mut inputs = Vec::with_capacity(w_diag);
                let mut preds = Vec::with_capacity(w_diag);
                let mut node_outs = Vec::with_capacity(w_diag);
                for _ in 0..w_diag {
                    let mut out = vec![0u64; 1];
                    ed.step(&[0], &mut out).unwrap();
                    inputs.push(vec![0u64]);
                    preds.push(out);
                    node_outs.push(node_output_values(ed.circuit(), ed.state()));
                }
                let excised = ed
                    .excise_verified_passthrough_subgraphs(
                        0,
                        &inputs,
                        &preds,
                        &node_outs,
                        Some(&init_regs),
                        EconomyConfig::default().split_code_tax,
                    )
                    .expect("M3-E2 D7b check");
                assert!(
                    excised.is_some(),
                    "expected M3-E2 to directly certify and excise D7bAgreementGate ({width:?}, {wire_delay:?})"
                );
                assert!(
                    ed.circuit().node_count() <= pre_n
                        && circuit_code_bits_with_tax(
                            ed.circuit(),
                            0,
                            EconomyConfig::default().split_code_tax,
                        ) <= pre_bits + 1e-11
                );
            }
        }

        // 2. Comparator-bearing `Combinational` reactive circuit (`k=2`): all 9 classes cleanly excised
        //    WITHOUT dropping the registered comparator or harming the primary cone!
        let ed_comp =
            build_comparator_reactive_editable(Width::BITS_2, 0, EconomyConfig::default(), growth);
        let pre_n = ed_comp.circuit().node_count();
        let pre_pri_len = primary_cone_nodes(ed_comp.circuit(), ed_comp.economy(), 0).len();
        for &mode in &[InjectionMode::Established, InjectionMode::Juvenile] {
            let horizon = match mode {
                InjectionMode::Established => 256usize,
                InjectionMode::Juvenile => 768usize,
            };
            for &class in &DetourClass::ALL {
                let mut learner =
                    ArkheLearner::from_editable(ed_comp.clone(), growth, SeedBank::new(67))
                        .expect("learner from comp");
                let mut budget = TickBudget::new(10_000_000);
                for t in 0..8u64 {
                    let s = if t.is_multiple_of(2) { 1u64 } else { 3u64 };
                    let _ = learner.predict(&mut budget);
                    learner.observe(&[s, s], &mut budget);
                }
                let edit = build_detour_edit(learner.editable(), 0, class).expect("comp detour");
                edit.apply_to_learner(&mut learner, mode)
                    .expect("apply comp detour");

                for t in 0..horizon {
                    let abs_t = (t as u64) + 8;
                    let s = if abs_t.is_multiple_of(2) { 1u64 } else { 3u64 };
                    let p = learner.predict(&mut budget);
                    if t > horizon - 32 {
                        let expected = s;
                        assert_eq!(p[0], expected, "comp post-excision err ({class}, {mode:?})");
                    }
                    learner.observe(&[s, s], &mut budget);
                }

                assert_eq!(
                    learner.active_comparator_count(),
                    1,
                    "comparator destroyed by {class} ({mode:?})"
                );
                assert_eq!(
                    learner.disagreement_prune_events(),
                    0,
                    "comparator wrongly refuted on {class} ({mode:?})"
                );
                assert!(
                    learner.editable().circuit().node_count() <= pre_n,
                    "comp circuit did not return to pre-injection size on {class} ({mode:?}): post={}, pre={pre_n}",
                    learner.editable().circuit().node_count()
                );
                let post_pri_len = primary_cone_nodes(
                    learner.editable().circuit(),
                    learner.editable().economy(),
                    0,
                )
                .len();
                assert_eq!(
                    post_pri_len, pre_pri_len,
                    "primary cone altered on comp circuit for {class} ({mode:?})"
                );
            }
        }
    }

    #[test]
    fn baseline_d1b1_matches_predicted_survival_and_collapse() {
        let base_growth = GrowthConfig {
            excision_mode: ExcisionMode::BaselineD1b1,
            ..GrowthConfig::default()
        };
        let base_ed = build_autonomous_ring_editable(
            Width::BITS_2,
            WireDelayMode::Combinational,
            &[0, 1, 3, 2, 1],
            EconomyConfig::default(),
            base_growth,
        );
        let pre_n = base_ed.circuit().node_count();
        let pre_bits = circuit_code_bits_with_tax(
            base_ed.circuit(),
            0,
            EconomyConfig::default().split_code_tax,
        );

        let run_baseline_case = |class: DetourClass, mode: InjectionMode, ticks: usize| {
            let mut learner =
                ArkheLearner::from_editable(base_ed.clone(), base_growth, SeedBank::new(59))
                    .expect("clone baseline learner");
            let mut budget = TickBudget::new(25_000_000);
            let clean_probe = base_ed.compiled().clone();
            let mut clean_state = ScalarState::new(&clean_probe);
            clean_state
                .set_registers(&clean_probe, base_ed.state().registers())
                .unwrap();
            let mut clean_out = vec![0u64; 1];
            for _ in 0..32 {
                clean_state
                    .step(&clean_probe, &[0], &mut clean_out)
                    .unwrap();
                let _ = learner.predict(&mut budget);
                learner.observe(&clean_out, &mut budget);
            }
            let edit = build_detour_edit(learner.editable(), 0, class).expect("detour");
            edit.apply_to_learner(&mut learner, mode).expect("apply");
            let mut tail_mismatches = 0usize;
            let mut min_n = learner.editable().circuit().node_count();
            for t in 0..ticks {
                clean_state
                    .step(&clean_probe, &[0], &mut clean_out)
                    .unwrap();
                let pred = learner.predict(&mut budget);
                if t + 32 >= ticks && pred[0] != clean_out[0] {
                    tail_mismatches += 1;
                }
                learner.observe(&clean_out, &mut budget);
                min_n = min_n.min(learner.editable().circuit().node_count());
            }
            (
                learner.editable().circuit().node_count(),
                circuit_code_bits_with_tax(
                    learner.editable().circuit(),
                    0,
                    EconomyConfig::default().split_code_tax,
                ),
                tail_mismatches,
                min_n,
                learner.starvation_prune_events(),
            )
        };

        // 1. Established mode (`W_obs = 256` ticks): D0 (stateful), D1..D4, D6, D7a (cloned stateful),
        //    and D7b all survive under unamended BaselineD1b1 (`local_excision_events == 0`).
        for class in [
            DetourClass::D0Dead,
            DetourClass::D1AddZero,
            DetourClass::D2OneFire,
            DetourClass::D3MulZero,
            DetourClass::D4MulOne,
            DetourClass::D6Involution,
            DetourClass::D7aFakeComparator,
            DetourClass::D7bAgreementGate,
        ] {
            let (post_n, post_bits, _, _, _) =
                run_baseline_case(class, InjectionMode::Established, 256);
            assert!(
                post_n > pre_n && post_bits > pre_bits,
                "expected BaselineD1b1 Established survival for {class}: post=({post_n},{post_bits}), pre=({pre_n},{pre_bits})"
            );
        }

        // 2. Juvenile mode (`3 * W_obs = 768` ticks):
        //    - D6Involution (`k=1` Eqz->Eqz) stays directly on primary cone, amortizes debt, and survives bloated.
        let base_k1_ed = build_autonomous_ring_editable(
            Width::BITS_1,
            WireDelayMode::Combinational,
            &[0, 1],
            EconomyConfig::default(),
            base_growth,
        );
        let pre_k1_n = base_k1_ed.circuit().node_count();
        let pre_k1_bits = circuit_code_bits_with_tax(
            base_k1_ed.circuit(),
            0,
            EconomyConfig::default().split_code_tax,
        );
        let mut d6_learner =
            ArkheLearner::from_editable(base_k1_ed.clone(), base_growth, SeedBank::new(61))
                .expect("k1 baseline learner");
        let mut d6_budget = TickBudget::new(25_000_000);
        let d6_probe = base_k1_ed.compiled().clone();
        let mut d6_state = ScalarState::new(&d6_probe);
        d6_state
            .set_registers(&d6_probe, base_k1_ed.state().registers())
            .unwrap();
        let mut d6_out = vec![0u64; 1];
        for _ in 0..32 {
            d6_state.step(&d6_probe, &[0], &mut d6_out).unwrap();
            let _ = d6_learner.predict(&mut d6_budget);
            d6_learner.observe(&d6_out, &mut d6_budget);
        }
        let d6_edit =
            build_detour_edit(d6_learner.editable(), 0, DetourClass::D6Involution).expect("k1 D6");
        d6_edit
            .apply_to_learner(&mut d6_learner, InjectionMode::Juvenile)
            .expect("apply k1 D6");
        let mut d6_mis = 0usize;
        let mut d6_min_n = d6_learner.editable().circuit().node_count();
        for t in 0..768usize {
            d6_state.step(&d6_probe, &[0], &mut d6_out).unwrap();
            let pred = d6_learner.predict(&mut d6_budget);
            if t + 32 >= 768 && pred[0] != d6_out[0] {
                d6_mis += 1;
            }
            d6_learner.observe(&d6_out, &mut d6_budget);
            d6_min_n = d6_min_n.min(d6_learner.editable().circuit().node_count());
        }
        let d6_n = d6_learner.editable().circuit().node_count();
        let d6_bits = circuit_code_bits_with_tax(
            d6_learner.editable().circuit(),
            0,
            EconomyConfig::default().split_code_tax,
        );
        let d6_starv = d6_learner.starvation_prune_events();
        assert!(
            d6_n > pre_k1_n
                && d6_bits > pre_k1_bits
                && d6_mis == 0
                && d6_min_n > pre_k1_n
                && d6_starv == 0,
            "expected BaselineD1b1 Juvenile D6 survival: ({d6_n}, {d6_bits}, mis={d6_mis}, min_n={d6_min_n}, starv={d6_starv})"
        );
        //    - D2OneFire, D3MulZero, D4MulOne, D7bAgreementGate fail to bypass under M2b D1b.1
        //      and collapse the downstream primary cone (`min_n < pre_n` / `starv > 0` or `tail_mismatches > 0`).
        for class in [
            DetourClass::D2OneFire,
            DetourClass::D3MulZero,
            DetourClass::D4MulOne,
            DetourClass::D7bAgreementGate,
        ] {
            let (post_n, _, tail_mis, min_n, starv) =
                run_baseline_case(class, InjectionMode::Juvenile, 768);
            assert!(
                min_n < pre_n || post_n < pre_n || tail_mis > 0 || starv > 0,
                "expected BaselineD1b1 Juvenile primary-cone collapse on {class}: post_n={post_n}, min_n={min_n} (pre={pre_n}), starv={starv}, tail_mis={tail_mis}"
            );
        }
    }
}
