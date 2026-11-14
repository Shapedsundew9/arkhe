//! Counterfactual causal sensitivity and modular-identity input gating.

use std::collections::BTreeSet;
use std::fmt;

use rand::Rng;

use crate::rng::{SeedBank, StreamDomain};
use crate::substrate::eval::CombOp;
use crate::substrate::{
    Circuit, CompiledCircuit, NodeId, Primitive, ScalarState, SignalSource, Width,
};

/// Maximum bit-width $k$ for which counterfactual sensitivity enumerates all $2^k - 1$
/// alternative symbols exhaustively instead of drawing `sample_count` random alternatives.
pub const EXACT_SENSITIVITY_MAX_BITS: u8 = 4;

/// Sensitivity of each circuit node to output ports and synchronous-register latches.
#[derive(Clone, Debug, PartialEq)]
pub struct SensitivitySnapshot {
    /// `output_by_node[node][port]` is the fraction of counterfactual node values changing that output.
    pub output_by_node: Vec<Vec<f64>>,
    /// `latch_by_node[node][register]` is the fraction changing that register's next input.
    pub latch_by_node: Vec<Vec<f64>>,
}

/// Target-specific causal ancestry shares for output ports and next-tick register state.
#[derive(Clone, Debug, PartialEq)]
pub struct CausalAncestry {
    /// Node shares for each output port.
    pub port_shares: Vec<Vec<f64>>,
    /// Node ancestry latched into each synchronous register.
    pub next_register_eligibility: Vec<Vec<f64>>,
}

/// Substrate nodes defining a registered comparator `EQZ(SUB(primary, alt))`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ComparatorTarget {
    /// Difference node `SUB(primary, alt)` (`in(0)` = primary, `in(1)` = alt).
    pub sub_node: NodeId,
    /// Equality detector `EQZ(sub_node)`.
    pub eqz_node: NodeId,
}

/// Sensitivity snapshot including counterfactual sensitivities to registered comparators.
#[derive(Clone, Debug, PartialEq)]
pub struct ComparatorSensitivitySnapshot {
    /// Base boundary and latch sensitivities (`output_by_node`, `latch_by_node`).
    pub base: SensitivitySnapshot,
    /// `comparator_sub_by_node[node][m]` is the fraction of counterfactual node values changing
    /// comparator `m`'s `sub_node` output (`SUB(primary, alt)`).
    pub comparator_sub_by_node: Vec<Vec<f64>>,
    /// `comparator_eqz_by_node[node][m]` is the fraction of counterfactual node values changing
    /// comparator `m`'s `eqz_node` output (`EQZ(sub_node)`).
    pub comparator_eqz_by_node: Vec<Vec<f64>>,
}

/// Causal ancestry across output ports, next-tick registers, and registered comparators.
#[derive(Clone, Debug, PartialEq)]
pub struct ComparatorCausalAncestry {
    /// Base boundary port shares and next-tick register eligibility.
    pub base: CausalAncestry,
    /// Per-comparator causal ancestry shares (`comparator_shares[m][node]`).
    pub comparator_shares: Vec<Vec<f64>>,
}

/// Per-node emission recency history used for ring-identity gating (M1a redesign D4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeActivity {
    /// Recency weight for emitting a non-zero value (additive non-identity), in `[0.0, 1.0]`.
    pub nonzero_weight: f64,
    /// Recency weight for emitting a non-one value (multiplicative non-identity), in `[0.0, 1.0]`.
    pub non_one_weight: f64,
}

impl Default for NodeActivity {
    fn default() -> Self {
        Self {
            nonzero_weight: 0.0,
            non_one_weight: 0.0,
        }
    }
}

impl NodeActivity {
    /// Node has ever emitted a non-zero value.
    #[must_use]
    pub fn ever_nonzero(&self) -> bool {
        self.nonzero_weight > 0.0
    }

    /// Node has ever emitted a non-one value.
    #[must_use]
    pub fn ever_non_one(&self) -> bool {
        self.non_one_weight > 0.0
    }
}

/// Failure while evaluating a counterfactual sensitivity snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SensitivityError {
    /// Circuit, compiled schedule, or state dimensions are inconsistent.
    DimensionMismatch,
    /// Sample count must be non-zero for widths where sampling is used.
    EmptySample,
}

impl fmt::Display for SensitivityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DimensionMismatch => {
                write!(f, "sensitivity inputs do not match compiled circuit")
            }
            Self::EmptySample => write!(f, "sensitivity sample count must be non-zero"),
        }
    }
}

impl std::error::Error for SensitivityError {}

/// Computes whole-node counterfactual sensitivity over the current synchronous tick.
///
/// Widths through four bits enumerate every alternative value. Wider rings draw distinct
/// alternatives from the isolated `SensitivityProbe` stream for the current tick block.
/// Computes node-by-target counterfactual sensitivity across output ports and register latches.
pub fn counterfactual_sensitivity(
    circuit: &Circuit,
    compiled: &CompiledCircuit,
    state: &ScalarState,
    seed_bank: SeedBank,
    tick_block: u64,
    sample_count: usize,
) -> Result<SensitivitySnapshot, SensitivityError> {
    counterfactual_sensitivity_with_comparators(
        circuit,
        compiled,
        state,
        seed_bank,
        tick_block,
        sample_count,
        &[],
    )
    .map(|snapshot| snapshot.base)
}

/// Computes node-by-target counterfactual sensitivity across output ports, register latches, and
/// registered comparator targets (`sub_node`, `eqz_node`).
pub fn counterfactual_sensitivity_with_comparators(
    circuit: &Circuit,
    compiled: &CompiledCircuit,
    state: &ScalarState,
    seed_bank: SeedBank,
    tick_block: u64,
    sample_count: usize,
    comparators: &[ComparatorTarget],
) -> Result<ComparatorSensitivitySnapshot, SensitivityError> {
    let nodes = circuit.nodes();
    let wires = state.wire_values();
    if circuit.width() != compiled.width()
        || state.registers().len() != compiled.total_registers()
        || compiled.num_outputs() as usize != compiled.output_drivers().len()
    {
        return Err(SensitivityError::DimensionMismatch);
    }
    if wires.is_empty() && (!nodes.is_empty() || compiled.num_outputs() > 0) {
        return Err(SensitivityError::DimensionMismatch);
    }
    if compiled.width().bits() > EXACT_SENSITIVITY_MAX_BITS && sample_count == 0 {
        return Err(SensitivityError::EmptySample);
    }

    let mut node_out_base = Vec::with_capacity(nodes.len());
    let mut next_wire = usize::from(compiled.num_inputs());
    for primitive in nodes {
        node_out_base.push(next_wire);
        next_wire += usize::from(primitive.output_arity());
    }
    if next_wire > wires.len() {
        return Err(SensitivityError::DimensionMismatch);
    }

    let mut comp_wires = Vec::with_capacity(comparators.len());
    for comp in comparators {
        let sub_idx = comp.sub_node.index();
        let eqz_idx = comp.eqz_node.index();
        if sub_idx >= nodes.len()
            || eqz_idx >= nodes.len()
            || !matches!(nodes[sub_idx], Primitive::Sub)
            || !matches!(nodes[eqz_idx], Primitive::Eqz)
        {
            return Err(SensitivityError::DimensionMismatch);
        }
        comp_wires.push((node_out_base[sub_idx], node_out_base[eqz_idx]));
    }

    let output_baseline: Vec<u64> = compiled
        .output_drivers()
        .iter()
        .map(|wire| wire.map_or(0, |index| wires[index]))
        .collect();
    let latch_count = compiled.total_registers();
    let mut latch_sources = vec![None; latch_count];
    for &(register, input_wire) in compiled.delay_latches() {
        latch_sources[register] = Some(input_wire);
    }
    for chain in compiled.shift_chains() {
        latch_sources[chain.base_reg] = Some(chain.src_wire);
    }

    let mut output_by_node = vec![vec![0.0; output_baseline.len()]; nodes.len()];
    let mut latch_by_node = vec![vec![0.0; latch_count]; nodes.len()];
    let mut comparator_sub_by_node = vec![vec![0.0; comparators.len()]; nodes.len()];
    let mut comparator_eqz_by_node = vec![vec![0.0; comparators.len()]; nodes.len()];
    let mut rng = seed_bank.stream(StreamDomain::SensitivityProbe { tick_block });
    let width = compiled.width();
    let mut scratch = wires.to_vec();

    for (node_index, primitive) in nodes.iter().copied().enumerate() {
        let out_base = node_out_base[node_index];
        let out_count = usize::from(primitive.output_arity());
        let actual = wires[out_base];
        let candidates = candidate_values(width, actual, sample_count, &mut rng)?;
        for candidate in &candidates {
            scratch.copy_from_slice(wires);
            for port in 0..out_count {
                scratch[out_base + port] = *candidate;
            }
            evaluate_counterfactual_cone(compiled.comb_ops(), &mut scratch, out_base, width);
            for (port, driver) in compiled.output_drivers().iter().enumerate() {
                if driver.is_some_and(|wire| scratch[wire] != output_baseline[port]) {
                    output_by_node[node_index][port] += 1.0;
                }
            }
            for (register, input_wire) in latch_sources.iter().enumerate() {
                if input_wire.is_some_and(|wire| scratch[wire] != wires[wire]) {
                    latch_by_node[node_index][register] += 1.0;
                }
            }
            for (m, &(sub_wire, eqz_wire)) in comp_wires.iter().enumerate() {
                if scratch[sub_wire] != wires[sub_wire] {
                    comparator_sub_by_node[node_index][m] += 1.0;
                }
                if scratch[eqz_wire] != wires[eqz_wire] {
                    comparator_eqz_by_node[node_index][m] += 1.0;
                }
            }
        }
        let denominator = candidates.len() as f64;
        if denominator > 0.0 {
            for sensitivity in &mut output_by_node[node_index] {
                *sensitivity /= denominator;
            }
            for sensitivity in &mut latch_by_node[node_index] {
                *sensitivity /= denominator;
            }
            for sensitivity in &mut comparator_sub_by_node[node_index] {
                *sensitivity /= denominator;
            }
            for sensitivity in &mut comparator_eqz_by_node[node_index] {
                *sensitivity /= denominator;
            }
        }
    }

    Ok(ComparatorSensitivitySnapshot {
        base: SensitivitySnapshot {
            output_by_node,
            latch_by_node,
        },
        comparator_sub_by_node,
        comparator_eqz_by_node,
    })
}

/// Propagates counterfactual sensitivity backward through each current-tick causal cone.
pub fn assemble_causal_ancestry(
    circuit: &Circuit,
    compiled: &CompiledCircuit,
    state: &ScalarState,
    sensitivity: &SensitivitySnapshot,
    activity: &[NodeActivity],
    register_eligibility: &[Vec<f64>],
    trace_discount: f64,
) -> Result<CausalAncestry, SensitivityError> {
    let view = SensitivityView {
        output_by_node: &sensitivity.output_by_node,
        latch_by_node: &sensitivity.latch_by_node,
        comparator_sub_by_node: &[],
        comparator_eqz_by_node: &[],
    };
    assemble_causal_ancestry_core(
        circuit,
        compiled,
        state,
        view,
        activity,
        register_eligibility,
        trace_discount,
        &[],
        false,
    )
    .map(|ancestry| ancestry.base)
}

/// Propagates counterfactual sensitivity backward through output ports, next-tick registers, and
/// registered comparator cones under either Wake (alt-only isolation) or Sleep (symmetric) rules.
#[expect(
    clippy::too_many_arguments,
    reason = "Direct extension of assemble_causal_ancestry with comparator targets and phase flag"
)]
pub fn assemble_causal_ancestry_with_comparators(
    circuit: &Circuit,
    compiled: &CompiledCircuit,
    state: &ScalarState,
    sensitivity: &ComparatorSensitivitySnapshot,
    activity: &[NodeActivity],
    register_eligibility: &[Vec<f64>],
    trace_discount: f64,
    comparators: &[ComparatorTarget],
    is_sleep: bool,
) -> Result<ComparatorCausalAncestry, SensitivityError> {
    let node_count = circuit.node_count();
    if sensitivity.comparator_sub_by_node.len() != node_count
        || sensitivity.comparator_eqz_by_node.len() != node_count
        || sensitivity
            .comparator_sub_by_node
            .iter()
            .any(|row| row.len() != comparators.len())
        || sensitivity
            .comparator_eqz_by_node
            .iter()
            .any(|row| row.len() != comparators.len())
        || comparators
            .iter()
            .any(|comp| comp.sub_node.index() >= node_count || comp.eqz_node.index() >= node_count)
    {
        return Err(SensitivityError::DimensionMismatch);
    }
    let view = SensitivityView {
        output_by_node: &sensitivity.base.output_by_node,
        latch_by_node: &sensitivity.base.latch_by_node,
        comparator_sub_by_node: &sensitivity.comparator_sub_by_node,
        comparator_eqz_by_node: &sensitivity.comparator_eqz_by_node,
    };
    assemble_causal_ancestry_core(
        circuit,
        compiled,
        state,
        view,
        activity,
        register_eligibility,
        trace_discount,
        comparators,
        is_sleep,
    )
}

#[derive(Clone, Copy)]
struct SensitivityView<'a> {
    output_by_node: &'a [Vec<f64>],
    latch_by_node: &'a [Vec<f64>],
    comparator_sub_by_node: &'a [Vec<f64>],
    comparator_eqz_by_node: &'a [Vec<f64>],
}

#[expect(
    clippy::too_many_arguments,
    reason = "Shared core for base and comparator causal ancestry propagation"
)]
fn assemble_causal_ancestry_core(
    circuit: &Circuit,
    compiled: &CompiledCircuit,
    state: &ScalarState,
    sensitivity: SensitivityView<'_>,
    activity: &[NodeActivity],
    register_eligibility: &[Vec<f64>],
    trace_discount: f64,
    comparators: &[ComparatorTarget],
    is_sleep: bool,
) -> Result<ComparatorCausalAncestry, SensitivityError> {
    let node_count = circuit.node_count();
    let register_count = compiled.total_registers();
    if activity.len() != node_count
        || register_eligibility.len() != register_count
        || register_eligibility
            .iter()
            .any(|trace| trace.len() != node_count)
        || sensitivity.output_by_node.len() != node_count
        || sensitivity.latch_by_node.len() != node_count
        || !trace_discount.is_finite()
        || !(0.0..1.0).contains(&trace_discount)
    {
        return Err(SensitivityError::DimensionMismatch);
    }

    let wire_count = state.wire_values().len();
    let mut node_out_base = Vec::with_capacity(node_count);
    let mut next_wire = usize::from(compiled.num_inputs());
    for primitive in circuit.nodes() {
        node_out_base.push(next_wire);
        next_wire += usize::from(primitive.output_arity());
    }
    if next_wire > wire_count
        || compiled.node_input_wires().len() != node_count
        || sensitivity
            .output_by_node
            .iter()
            .any(|values| values.len() != compiled.output_drivers().len())
        || sensitivity
            .latch_by_node
            .iter()
            .any(|values| values.len() != register_count)
    {
        return Err(SensitivityError::DimensionMismatch);
    }

    let mut delay_register_of_node = vec![None; node_count];
    let mut next_explicit_register = 0usize;
    for (node, primitive) in circuit.nodes().iter().enumerate() {
        if matches!(primitive, Primitive::Delay { .. }) {
            delay_register_of_node[node] = Some(next_explicit_register);
            next_explicit_register += 1;
        }
    }

    let num_outputs = compiled.output_drivers().len();
    let mut port_shares = vec![vec![0.0; node_count]; num_outputs];
    let mut next_register_eligibility = vec![vec![0.0; node_count]; register_count];
    let mut comparator_shares = vec![vec![0.0; node_count]; comparators.len()];
    if node_count == 0 {
        return Ok(ComparatorCausalAncestry {
            base: CausalAncestry {
                port_shares,
                next_register_eligibility,
            },
            comparator_shares,
        });
    }

    let mut out_wire_to_node = vec![usize::MAX; wire_count];
    let mut wire_source_nodes = vec![None; wire_count];
    for (idx, primitive) in circuit.nodes().iter().enumerate() {
        let base = node_out_base[idx];
        let node_id = NodeId(u32::try_from(idx).expect("node index fits in u32"));
        for port in 0..usize::from(primitive.output_arity()) {
            out_wire_to_node[base + port] = idx;
            wire_source_nodes[base + port] = Some(node_id);
        }
    }
    if circuit.wire_delay() == crate::substrate::WireDelayMode::TicksPerHop {
        for (edge_idx, edge) in circuit.edges().iter().enumerate() {
            if let Some(slot) = wire_source_nodes.get_mut(next_wire + edge_idx)
                && let SignalSource::NodeOut { node, .. } = edge.src
            {
                *slot = Some(node);
            }
        }
    }

    let mut wire_ancestry = vec![0.0; wire_count * node_count];
    let mut node_ancestry = vec![0.0; node_count];
    let wire_values = state.wire_values();

    for (port, shares) in port_shares.iter_mut().enumerate() {
        ancestry_for_target(
            circuit,
            compiled,
            wire_values,
            sensitivity,
            comparators,
            activity,
            register_eligibility,
            trace_discount,
            &node_out_base,
            &out_wire_to_node,
            &wire_source_nodes,
            &delay_register_of_node,
            wire_count,
            Target::Output(port),
            &mut wire_ancestry,
            &mut node_ancestry,
        )?;
        if let Some(driver) = compiled.output_drivers()[port] {
            shares.copy_from_slice(&wire_ancestry[driver * node_count..(driver + 1) * node_count]);
        }
    }

    let mut register_input_wires = vec![None; register_count];
    for &(register, input_wire) in compiled.delay_latches() {
        register_input_wires[register] = Some(input_wire);
    }
    for chain in compiled.shift_chains() {
        register_input_wires[chain.base_reg] = Some(chain.src_wire);
        for stage in 1..chain.hops {
            let current = chain.base_reg + stage;
            let previous = chain.base_reg + stage - 1;
            for node in 0..node_count {
                next_register_eligibility[current][node] =
                    trace_discount * register_eligibility[previous][node];
            }
        }
    }
    for (register, input_wire_opt) in register_input_wires.iter().enumerate() {
        let Some(input_wire) = *input_wire_opt else {
            continue;
        };
        ancestry_for_target(
            circuit,
            compiled,
            wire_values,
            sensitivity,
            comparators,
            activity,
            register_eligibility,
            trace_discount,
            &node_out_base,
            &out_wire_to_node,
            &wire_source_nodes,
            &delay_register_of_node,
            wire_count,
            Target::Register(register),
            &mut wire_ancestry,
            &mut node_ancestry,
        )?;
        next_register_eligibility[register].copy_from_slice(
            &wire_ancestry[input_wire * node_count..(input_wire + 1) * node_count],
        );
    }

    for (m, comp) in comparators.iter().enumerate() {
        let target = if is_sleep {
            Target::ComparatorSleep(m)
        } else {
            Target::ComparatorWake(m)
        };
        ancestry_for_target(
            circuit,
            compiled,
            wire_values,
            sensitivity,
            comparators,
            activity,
            register_eligibility,
            trace_discount,
            &node_out_base,
            &out_wire_to_node,
            &wire_source_nodes,
            &delay_register_of_node,
            wire_count,
            target,
            &mut wire_ancestry,
            &mut node_ancestry,
        )?;
        let eqz_wire = node_out_base[comp.eqz_node.index()];
        comparator_shares[m]
            .copy_from_slice(&wire_ancestry[eqz_wire * node_count..(eqz_wire + 1) * node_count]);
    }

    Ok(ComparatorCausalAncestry {
        base: CausalAncestry {
            port_shares,
            next_register_eligibility,
        },
        comparator_shares,
    })
}

#[derive(Clone, Copy)]
enum Target {
    Output(usize),
    Register(usize),
    ComparatorWake(usize),
    ComparatorSleep(usize),
}

#[allow(clippy::too_many_arguments)]
fn ancestry_for_target(
    circuit: &Circuit,
    compiled: &CompiledCircuit,
    wire_values: &[u64],
    sensitivity: SensitivityView<'_>,
    comparators: &[ComparatorTarget],
    activity: &[NodeActivity],
    register_eligibility: &[Vec<f64>],
    trace_discount: f64,
    node_out_base: &[usize],
    out_wire_to_node: &[usize],
    wire_source_nodes: &[Option<NodeId>],
    delay_register_of_node: &[Option<usize>],
    wire_count: usize,
    target: Target,
    wire_ancestry: &mut [f64],
    node_ancestry: &mut [f64],
) -> Result<(), SensitivityError> {
    let node_count = circuit.node_count();
    wire_ancestry.fill(0.0);
    for &(register, wire) in compiled.delay_reads() {
        if let Some(trace) = register_eligibility.get(register) {
            let dst_slice = &mut wire_ancestry[wire * node_count..(wire + 1) * node_count];
            for (dst, &value) in dst_slice.iter_mut().zip(trace) {
                *dst = trace_discount * value;
            }
        }
    }

    for (node, primitive) in circuit.nodes().iter().copied().enumerate() {
        if !matches!(primitive, Primitive::Delay { .. }) {
            continue;
        }
        let Some(register) = delay_register_of_node[node] else {
            return Err(SensitivityError::DimensionMismatch);
        };
        let coefficient = target_sensitivity(sensitivity, comparators, node, target)?;
        let wire = node_out_base[node];
        let dst_slice = &mut wire_ancestry[wire * node_count..(wire + 1) * node_count];
        let reg_elig = &register_eligibility[register];
        for (ancestor, (dst, &elig)) in dst_slice.iter_mut().zip(reg_elig).enumerate() {
            let inherited = trace_discount * elig;
            let raw = (if ancestor == node { 1.0 } else { 0.0 }) + inherited;
            *dst = coefficient * raw.min(1.0);
        }
    }

    debug_assert_eq!(wire_ancestry.len(), wire_count * node_count);
    for operation in compiled.comb_ops() {
        let (first_out, out_count, out_pair) = match *operation {
            CombOp::Add { out, .. }
            | CombOp::Sub { out, .. }
            | CombOp::Mul { out, .. }
            | CombOp::Eqz { out, .. } => (out, 1usize, [out, 0]),
            CombOp::Split { out0, out1, .. } => (out0, 2usize, [out0, out1]),
        };
        let node = *out_wire_to_node
            .get(first_out)
            .filter(|&&idx| idx != usize::MAX)
            .ok_or(SensitivityError::DimensionMismatch)?;
        let coefficient = target_sensitivity(sensitivity, comparators, node, target)?;
        let input_wires = &compiled.node_input_wires()[node];
        let arity = input_wires.len();
        let prim = circuit.nodes()[node];
        let is_mul_consumer = matches!(prim, Primitive::Mul);

        let mut in_vals_stack = [0u64; 2];
        let mut in_act_stack = [0.0f64; 2];
        let mut in_vals_heap = Vec::new();
        let mut in_act_heap = Vec::new();
        let (in_vals, in_act): (&mut [u64], &mut [f64]) = if arity <= 2 {
            (&mut in_vals_stack[..arity], &mut in_act_stack[..arity])
        } else {
            in_vals_heap.resize(arity, 0);
            in_act_heap.resize(arity, 0.0);
            (&mut in_vals_heap, &mut in_act_heap)
        };
        for (i, &wire) in input_wires.iter().enumerate() {
            in_vals[i] = wire_values[wire];
            in_act[i] = wire_source_nodes[wire].map_or(0.0, |source| {
                let act = activity[source.index()];
                if is_mul_consumer {
                    act.non_one_weight
                } else {
                    act.nonzero_weight
                }
            });
        }
        let w_stack = match target {
            Target::ComparatorWake(m) if node == comparators[m].sub_node.index() => [0.0, 1.0],
            Target::ComparatorSleep(m) if node == comparators[m].sub_node.index() => [0.5, 0.5],
            _ if arity == 1 => [1.0, 0.0],
            _ => ring_identity_input_weights_pair(
                prim,
                [in_vals[0], in_vals[1]],
                [in_act[0], in_act[1]],
                compiled.width(),
            ),
        };
        let weights = &w_stack[..arity];
        for (ancestor, value) in node_ancestry.iter_mut().enumerate() {
            let mut inherited = 0.0;
            for (&wire, &weight) in input_wires.iter().zip(weights) {
                inherited += weight * wire_ancestry[wire * node_count + ancestor];
            }
            let raw = (if ancestor == node { 1.0 } else { 0.0 }) + inherited;
            *value = coefficient * raw.min(1.0);
        }
        for &wire in &out_pair[..out_count] {
            wire_ancestry[wire * node_count..(wire + 1) * node_count]
                .copy_from_slice(node_ancestry);
        }
    }
    Ok(())
}

fn target_sensitivity(
    sensitivity: SensitivityView<'_>,
    comparators: &[ComparatorTarget],
    node: usize,
    target: Target,
) -> Result<f64, SensitivityError> {
    match target {
        Target::Output(port) => sensitivity
            .output_by_node
            .get(node)
            .and_then(|values| values.get(port))
            .copied(),
        Target::Register(register) => sensitivity
            .latch_by_node
            .get(node)
            .and_then(|values| values.get(register))
            .copied(),
        Target::ComparatorWake(m) => {
            let comp = comparators
                .get(m)
                .ok_or(SensitivityError::DimensionMismatch)?;
            if node == comp.eqz_node.index() {
                Some(1.0)
            } else {
                sensitivity
                    .comparator_sub_by_node
                    .get(node)
                    .and_then(|values| values.get(m))
                    .copied()
            }
        }
        Target::ComparatorSleep(m) => sensitivity
            .comparator_eqz_by_node
            .get(node)
            .and_then(|values| values.get(m))
            .copied(),
    }
    .ok_or(SensitivityError::DimensionMismatch)
}

fn candidate_values(
    width: Width,
    actual: u64,
    sample_count: usize,
    rng: &mut impl Rng,
) -> Result<Vec<u64>, SensitivityError> {
    let actual = width.normalize(actual);
    if width.bits() <= EXACT_SENSITIVITY_MAX_BITS {
        let cardinality = 1u64 << width.bits();
        return Ok((0..cardinality).filter(|&value| value != actual).collect());
    }
    if sample_count == 0 {
        return Err(SensitivityError::EmptySample);
    }

    let max_distinct = if width.bits() == 64 {
        usize::MAX
    } else {
        ((1u64 << width.bits()) - 1) as usize
    };
    let target = sample_count.min(max_distinct);
    let mut selected = BTreeSet::new();
    while selected.len() < target {
        let candidate = width.normalize(rng.next_u64());
        if candidate != actual {
            selected.insert(candidate);
        }
    }
    Ok(selected.into_iter().collect())
}

fn evaluate_counterfactual_cone(
    operations: &[CombOp],
    wires: &mut [u64],
    source_base: usize,
    width: Width,
) {
    for operation in operations {
        let output_base = match *operation {
            CombOp::Add { out, .. }
            | CombOp::Sub { out, .. }
            | CombOp::Mul { out, .. }
            | CombOp::Eqz { out, .. } => out,
            CombOp::Split { out0, .. } => out0,
        };
        if output_base == source_base {
            continue;
        }
        match *operation {
            CombOp::Add { lhs, rhs, out } => wires[out] = width.add(wires[lhs], wires[rhs]),
            CombOp::Sub { lhs, rhs, out } => wires[out] = width.sub(wires[lhs], wires[rhs]),
            CombOp::Mul { lhs, rhs, out } => wires[out] = width.mul(wires[lhs], wires[rhs]),
            CombOp::Eqz { arg, out } => wires[out] = width.eqz(wires[arg]),
            CombOp::Split { arg, out0, out1 } => {
                wires[out0] = wires[arg];
                wires[out1] = wires[arg];
            }
        }
    }
}

fn ring_identity_input_weights_pair(
    primitive: Primitive,
    inputs: [u64; 2],
    input_active: [f64; 2],
    width: Width,
) -> [f64; 2] {
    let values = [width.normalize(inputs[0]), width.normalize(inputs[1])];
    let mut raw = [1.0, 1.0];
    match primitive {
        Primitive::Add => {
            for j in 0..2 {
                if values[j] == 0 {
                    raw[j] = input_active[j];
                }
            }
        }
        Primitive::Sub => {
            if values[1] == 0 {
                raw[1] = input_active[1];
                if values[0] == 0 {
                    raw[0] = input_active[0];
                }
            }
        }
        Primitive::Mul => {
            let cardinality = 2.0f64.powi(i32::from(width.bits()));
            for input in 0..2 {
                let other = 1 - input;
                let factor = values[other];
                let divisor = if factor == 0 {
                    cardinality
                } else {
                    2.0f64.powi(
                        i32::try_from(factor.trailing_zeros().min(u32::from(width.bits())))
                            .expect("trailing-zero count fits i32"),
                    )
                };
                let base_weight = cardinality - divisor;
                if values[input] == 1 {
                    raw[input] = base_weight * input_active[input];
                } else {
                    raw[input] = base_weight;
                }
            }
        }
        _ => {}
    }
    let total = raw[0] + raw[1];
    if total == 0.0 {
        [0.0, 0.0]
    } else {
        [raw[0] / total, raw[1] / total]
    }
}

/// Returns normalised ring-identity-aware input weights for one primitive.
///
/// `input_active[j]` is the recency weight in `[0.0, 1.0]` for the source of input `j` (M1a redesign D4).
#[must_use]
pub fn ring_identity_input_weights(
    primitive: Primitive,
    inputs: &[u64],
    input_active: &[f64],
    width: Width,
) -> Vec<f64> {
    let arity = usize::from(primitive.input_arity());
    if inputs.len() != arity || input_active.len() != arity {
        return vec![0.0; arity];
    }
    if arity == 1 {
        return vec![1.0];
    }
    ring_identity_input_weights_pair(
        primitive,
        [inputs[0], inputs[1]],
        [input_active[0], input_active[1]],
        width,
    )
    .to_vec()
}

/// Returns the source node for a compiled wire, where applicable.
#[must_use]
pub fn source_node_for_wire(circuit: &Circuit, wire: usize) -> Option<NodeId> {
    let mut next_wire = usize::from(circuit.num_inputs());
    for (index, primitive) in circuit.nodes().iter().enumerate() {
        let end = next_wire + usize::from(primitive.output_arity());
        if (next_wire..end).contains(&wire) {
            return Some(NodeId(
                u32::try_from(index).expect("node index fits in u32"),
            ));
        }
        next_wire = end;
    }
    if circuit.wire_delay() == crate::substrate::WireDelayMode::TicksPerHop {
        let edge_idx = wire - next_wire;
        if let Some(edge) = circuit.edges().get(edge_idx)
            && let SignalSource::NodeOut { node, .. } = edge.src
        {
            return Some(node);
        }
    }
    None
}

/// Checks whether a compiled node output is driven by an environmental input.
#[must_use]
pub fn node_has_boundary_input(circuit: &Circuit, node: NodeId) -> bool {
    circuit.edges().iter().any(|edge| {
        edge.dst == crate::substrate::SignalSink::NodeIn { node, port: 0 }
            && matches!(edge.src, SignalSource::BoundaryIn(_))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::substrate::{ScalarState, SignalSink, WireDelayMode};

    #[test]
    fn whole_node_counterfactual_rejects_reconvergent_cancellation() {
        let mut circuit = Circuit::new(Width::BITS_2, 2, 1, WireDelayMode::Combinational);
        let branch = circuit.add_node(Primitive::Split);
        let add = circuit.add_node(Primitive::Add);
        let sub = circuit.add_node(Primitive::Sub);
        circuit.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn {
                node: branch,
                port: 0,
            },
        );
        circuit.connect(
            SignalSource::BoundaryIn(1),
            SignalSink::NodeIn { node: add, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut {
                node: branch,
                port: 0,
            },
            SignalSink::NodeIn { node: add, port: 1 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: add, port: 0 },
            SignalSink::NodeIn { node: sub, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut {
                node: branch,
                port: 1,
            },
            SignalSink::NodeIn { node: sub, port: 1 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: sub, port: 0 },
            SignalSink::BoundaryOut(0),
        );

        let compiled = circuit.compile().unwrap();
        let mut state = ScalarState::new(&compiled);
        state.step(&compiled, &[2, 1], &mut [0]).unwrap();
        let sensitivity =
            counterfactual_sensitivity(&circuit, &compiled, &state, SeedBank::new(7), 0, 8)
                .unwrap();
        assert_eq!(sensitivity.output_by_node[branch.index()][0], 0.0);
        assert_eq!(sensitivity.output_by_node[add.index()][0], 1.0);
    }

    #[test]
    fn modular_identity_gates_remove_passive_inputs() {
        assert_eq!(
            ring_identity_input_weights(Primitive::Add, &[3, 0], &[1.0, 0.0], Width::BITS_2),
            vec![1.0, 0.0]
        );
        assert_eq!(
            ring_identity_input_weights(Primitive::Sub, &[1, 0], &[1.0, 0.0], Width::BITS_2),
            vec![1.0, 0.0]
        );
        assert_eq!(
            ring_identity_input_weights(Primitive::Add, &[0, 0], &[0.0, 1.0], Width::BITS_2),
            vec![0.0, 1.0]
        );
    }

    #[test]
    fn ring_identity_gates_preserve_credit_for_active_zero_inputs() {
        assert_eq!(
            ring_identity_input_weights(Primitive::Add, &[3, 0], &[1.0, 1.0], Width::BITS_2),
            vec![0.5, 0.5]
        );
        assert_eq!(
            ring_identity_input_weights(Primitive::Add, &[0, 3], &[1.0, 1.0], Width::BITS_2),
            vec![0.5, 0.5]
        );
        assert_eq!(
            ring_identity_input_weights(Primitive::Sub, &[1, 0], &[1.0, 1.0], Width::BITS_2),
            vec![0.5, 0.5]
        );
        assert_eq!(
            ring_identity_input_weights(Primitive::Sub, &[0, 0], &[1.0, 1.0], Width::BITS_2),
            vec![0.5, 0.5]
        );
        // Partially inactive zero input preserves recency weighting.
        let weighted =
            ring_identity_input_weights(Primitive::Add, &[3, 0], &[1.0, 0.25], Width::BITS_2);
        assert!((weighted[0] - 0.8).abs() < 1e-12);
        assert!((weighted[1] - 0.2).abs() < 1e-12);
    }

    #[test]
    fn multiplication_weights_equal_exact_local_perturbation_counts() {
        let weights =
            ring_identity_input_weights(Primitive::Mul, &[2, 1], &[1.0, 1.0], Width::BITS_2);
        assert!((weights[0] - 0.6).abs() < 1e-12);
        assert!((weights[1] - 0.4).abs() < 1e-12);
    }

    #[test]
    fn discounted_register_ancestry_preserves_credit_through_delay_chains() {
        let mut circuit = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);
        let delays: Vec<NodeId> = (0..5)
            .map(|_| circuit.add_node(Primitive::delay_zero()))
            .collect();
        circuit.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn {
                node: delays[0],
                port: 0,
            },
        );
        for pair in delays.windows(2) {
            circuit.connect(
                SignalSource::NodeOut {
                    node: pair[0],
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: pair[1],
                    port: 0,
                },
            );
        }
        circuit.connect(
            SignalSource::NodeOut {
                node: delays[4],
                port: 0,
            },
            SignalSink::BoundaryOut(0),
        );

        let compiled = circuit.compile().unwrap();
        let mut state = ScalarState::new(&compiled);
        let mut eligibility = vec![vec![0.0; delays.len()]; compiled.total_registers()];
        let active = vec![NodeActivity::default(); delays.len()];
        let mut output_shares = Vec::new();
        for _ in 0..8 {
            state.step(&compiled, &[1], &mut [0]).unwrap();
            let sensitivity =
                counterfactual_sensitivity(&circuit, &compiled, &state, SeedBank::new(1), 0, 8)
                    .unwrap();
            let ancestry = assemble_causal_ancestry(
                &circuit,
                &compiled,
                &state,
                &sensitivity,
                &active,
                &eligibility,
                0.9,
            )
            .unwrap();
            eligibility = ancestry.next_register_eligibility;
            output_shares = ancestry.port_shares[0].clone();
        }
        let total: f64 = output_shares.iter().sum();
        let expected = [0.9f64.powi(4), 0.9f64.powi(3), 0.9f64.powi(2), 0.9, 1.0];
        let normalizer: f64 = expected.iter().sum();
        for (share, expected) in output_shares.iter().zip(expected) {
            assert!((share / total - expected / normalizer).abs() < 1e-12);
        }
    }

    #[test]
    fn mul_zero_detour_receives_no_additive_credit() {
        let mut circuit = Circuit::new(Width::BITS_2, 1, 1, WireDelayMode::Combinational);
        // ConstLoop(1) for b: db -> sb -> (db, mul.in0)
        let db = circuit.add_node(Primitive::Delay { init: 1 });
        let sb = circuit.add_node(Primitive::Split);
        // ConstLoop(0) for z: dz -> sz -> (dz, mul.in1)
        let dz = circuit.add_node(Primitive::Delay { init: 0 });
        let sz = circuit.add_node(Primitive::Split);
        let mul = circuit.add_node(Primitive::Mul);
        let add = circuit.add_node(Primitive::Add);

        // Wire b loop
        circuit.connect(
            SignalSource::NodeOut { node: db, port: 0 },
            crate::substrate::SignalSink::NodeIn { node: sb, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: sb, port: 0 },
            crate::substrate::SignalSink::NodeIn { node: db, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: sb, port: 1 },
            crate::substrate::SignalSink::NodeIn { node: mul, port: 0 },
        );

        // Wire z loop
        circuit.connect(
            SignalSource::NodeOut { node: dz, port: 0 },
            crate::substrate::SignalSink::NodeIn { node: sz, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: sz, port: 0 },
            crate::substrate::SignalSink::NodeIn { node: dz, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: sz, port: 1 },
            crate::substrate::SignalSink::NodeIn { node: mul, port: 1 },
        );

        // Wire add and output
        circuit.connect(
            SignalSource::BoundaryIn(0),
            crate::substrate::SignalSink::NodeIn { node: add, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: mul, port: 0 },
            crate::substrate::SignalSink::NodeIn { node: add, port: 1 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: add, port: 0 },
            crate::substrate::SignalSink::BoundaryOut(0),
        );

        let compiled = circuit.compile().unwrap();
        let mut state = ScalarState::new(&compiled);
        let mut outputs = [0u64];
        state.step(&compiled, &[0], &mut outputs).unwrap();

        let sensitivity =
            counterfactual_sensitivity(&circuit, &compiled, &state, SeedBank::new(1), 0, 8)
                .unwrap();
        let activity = vec![
            NodeActivity {
                nonzero_weight: 1.0,
                non_one_weight: 0.0,
            },
            NodeActivity {
                nonzero_weight: 1.0,
                non_one_weight: 0.0,
            },
            NodeActivity {
                nonzero_weight: 0.0,
                non_one_weight: 1.0,
            },
            NodeActivity {
                nonzero_weight: 0.0,
                non_one_weight: 1.0,
            },
            NodeActivity {
                nonzero_weight: 0.0,
                non_one_weight: 1.0,
            },
            NodeActivity {
                nonzero_weight: 0.0,
                non_one_weight: 0.0,
            },
        ];
        let eligibility = vec![vec![0.0; circuit.node_count()]; compiled.total_registers()];
        let ancestry = assemble_causal_ancestry(
            &circuit,
            &compiled,
            &state,
            &sensitivity,
            &activity,
            &eligibility,
            0.9,
        )
        .unwrap();

        assert_eq!(ancestry.port_shares[0][mul.index()], 0.0);
        assert_eq!(ancestry.port_shares[0][dz.index()], 0.0);
        assert_eq!(ancestry.port_shares[0][sz.index()], 0.0);
    }

    #[test]
    fn nonzero_constant_keeps_credit() {
        let mut circuit = Circuit::new(Width::BITS_2, 1, 1, WireDelayMode::Combinational);
        let d = circuit.add_node(Primitive::Delay { init: 1 });
        let s = circuit.add_node(Primitive::Split);
        let add = circuit.add_node(Primitive::Add);

        circuit.connect(
            SignalSource::NodeOut { node: d, port: 0 },
            crate::substrate::SignalSink::NodeIn { node: s, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: s, port: 0 },
            crate::substrate::SignalSink::NodeIn { node: d, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: s, port: 1 },
            crate::substrate::SignalSink::NodeIn { node: add, port: 1 },
        );
        circuit.connect(
            SignalSource::BoundaryIn(0),
            crate::substrate::SignalSink::NodeIn { node: add, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: add, port: 0 },
            crate::substrate::SignalSink::BoundaryOut(0),
        );

        let compiled = circuit.compile().unwrap();
        let mut state = ScalarState::new(&compiled);
        let mut outputs = [0u64];
        state.step(&compiled, &[0], &mut outputs).unwrap();

        let sensitivity =
            counterfactual_sensitivity(&circuit, &compiled, &state, SeedBank::new(1), 0, 8)
                .unwrap();
        let activity = vec![
            NodeActivity {
                nonzero_weight: 1.0,
                non_one_weight: 0.0,
            },
            NodeActivity {
                nonzero_weight: 1.0,
                non_one_weight: 0.0,
            },
            NodeActivity {
                nonzero_weight: 1.0,
                non_one_weight: 0.0,
            },
        ];
        let eligibility = vec![vec![0.0; circuit.node_count()]; compiled.total_registers()];
        let ancestry = assemble_causal_ancestry(
            &circuit,
            &compiled,
            &state,
            &sensitivity,
            &activity,
            &eligibility,
            0.9,
        )
        .unwrap();

        assert!(ancestry.port_shares[0][d.index()] > 0.0);
        assert!(ancestry.port_shares[0][s.index()] > 0.0);
    }

    #[test]
    fn recurrent_loop_ancestry_entries_are_clamped_at_most_one() {
        let mut circuit = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);
        let d = circuit.add_node(Primitive::Delay { init: 1 });
        let s = circuit.add_node(Primitive::Split);
        let add = circuit.add_node(Primitive::Add);
        circuit.connect(
            SignalSource::NodeOut { node: d, port: 0 },
            SignalSink::NodeIn { node: s, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: s, port: 0 },
            SignalSink::NodeIn { node: d, port: 0 },
        );
        circuit.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: add, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: s, port: 1 },
            SignalSink::NodeIn { node: add, port: 1 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: add, port: 0 },
            SignalSink::BoundaryOut(0),
        );

        let compiled = circuit.compile().unwrap();
        let mut state = ScalarState::new(&compiled);
        let mut eligibility = vec![vec![0.0; circuit.node_count()]; compiled.total_registers()];
        let activity = vec![
            NodeActivity {
                nonzero_weight: 1.0,
                non_one_weight: 0.0,
            };
            circuit.node_count()
        ];
        let mut outputs = [0u64];

        for t in 0..100 {
            let input = t % 2;
            state.step(&compiled, &[input], &mut outputs).unwrap();
            let sensitivity =
                counterfactual_sensitivity(&circuit, &compiled, &state, SeedBank::new(1), t, 8)
                    .unwrap();
            let ancestry = assemble_causal_ancestry(
                &circuit,
                &compiled,
                &state,
                &sensitivity,
                &activity,
                &eligibility,
                0.9,
            )
            .unwrap();

            for &share in &ancestry.port_shares[0] {
                assert!(
                    share <= 1.0 + 1e-12,
                    "tick {t}: port share {share} exceeded 1.0"
                );
            }
            for trace in &ancestry.next_register_eligibility {
                for &val in trace {
                    assert!(
                        val <= 1.0 + 1e-12,
                        "tick {t}: register eligibility {val} exceeded 1.0"
                    );
                }
            }
            eligibility = ancestry.next_register_eligibility;
        }
    }

    #[test]
    fn comparator_wake_isolation_and_sleep_symmetry() {
        use crate::substrate::{SignalSink, WireDelayMode};

        // Circuit: primary Delay(p_d), alt Delay(a_d), sub = SUB(p_d, a_d), eqz = EQZ(sub)
        let mut circuit = Circuit::new(Width::BITS_1, 2, 1, WireDelayMode::Combinational);
        let p_d = circuit.add_node(Primitive::Delay { init: 0 });
        let a_d = circuit.add_node(Primitive::Delay { init: 0 });
        let split_p = circuit.add_node(Primitive::Split);
        let sub = circuit.add_node(Primitive::Sub);
        let eqz = circuit.add_node(Primitive::Eqz);

        circuit.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: p_d, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: p_d, port: 0 },
            SignalSink::NodeIn {
                node: split_p,
                port: 0,
            },
        );
        circuit.connect(
            SignalSource::NodeOut {
                node: split_p,
                port: 0,
            },
            SignalSink::BoundaryOut(0),
        );
        circuit.connect(
            SignalSource::NodeOut {
                node: split_p,
                port: 1,
            },
            SignalSink::NodeIn { node: sub, port: 0 },
        );
        circuit.connect(
            SignalSource::BoundaryIn(1),
            SignalSink::NodeIn { node: a_d, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: a_d, port: 0 },
            SignalSink::NodeIn { node: sub, port: 1 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: sub, port: 0 },
            SignalSink::NodeIn { node: eqz, port: 0 },
        );

        let compiled = circuit.compile().unwrap();
        let mut state = ScalarState::new(&compiled);
        let mut outputs = [0u64];
        state.step(&compiled, &[1, 1], &mut outputs).unwrap();

        let target = ComparatorTarget {
            sub_node: sub,
            eqz_node: eqz,
        };
        let legacy =
            counterfactual_sensitivity(&circuit, &compiled, &state, SeedBank::new(3), 0, 8)
                .unwrap();
        let snap = counterfactual_sensitivity_with_comparators(
            &circuit,
            &compiled,
            &state,
            SeedBank::new(3),
            0,
            8,
            &[target],
        )
        .unwrap();
        assert_eq!(snap.base, legacy);

        // Both Delay nodes and sub/eqz change eqz_node when perturbed on k=1.
        assert!((snap.comparator_eqz_by_node[p_d.index()][0] - 1.0).abs() < 1e-12);
        assert!((snap.comparator_eqz_by_node[a_d.index()][0] - 1.0).abs() < 1e-12);
        assert!((snap.comparator_eqz_by_node[sub.index()][0] - 1.0).abs() < 1e-12);
        assert!((snap.comparator_eqz_by_node[eqz.index()][0] - 1.0).abs() < 1e-12);
        // Boundary output sensitivity ignores alt/sub/eqz.
        assert_eq!(snap.base.output_by_node[a_d.index()][0], 0.0);
        assert_eq!(snap.base.output_by_node[sub.index()][0], 0.0);
        assert_eq!(snap.base.output_by_node[eqz.index()][0], 0.0);

        let eligibility = vec![vec![0.0; circuit.node_count()]; compiled.total_registers()];
        let activity = vec![
            NodeActivity {
                nonzero_weight: 0.0,
                non_one_weight: 0.0,
            };
            circuit.node_count()
        ];

        // Wake: primary cone receives 0.0 from comparator even though p_d == 0 and a_d == 0.
        let wake_ancestry = assemble_causal_ancestry_with_comparators(
            &circuit,
            &compiled,
            &state,
            &snap,
            &activity,
            &eligibility,
            0.9,
            &[target],
            false,
        )
        .unwrap();
        let wake_shares = &wake_ancestry.comparator_shares[0];
        assert_eq!(wake_shares[p_d.index()], 0.0);
        assert_eq!(wake_shares[split_p.index()], 0.0);
        assert!((wake_shares[a_d.index()] - 1.0).abs() < 1e-12);
        assert!((wake_shares[sub.index()] - 1.0).abs() < 1e-12);
        assert!((wake_shares[eqz.index()] - 1.0).abs() < 1e-12);

        // Sleep: symmetric 0.5 weights at sub_node share credit across primary and alt equally.
        let sleep_ancestry = assemble_causal_ancestry_with_comparators(
            &circuit,
            &compiled,
            &state,
            &snap,
            &activity,
            &eligibility,
            0.9,
            &[target],
            true,
        )
        .unwrap();
        let sleep_shares = &sleep_ancestry.comparator_shares[0];
        assert!((sleep_shares[p_d.index()] - 0.5).abs() < 1e-12);
        assert!((sleep_shares[split_p.index()] - 0.5).abs() < 1e-12);
        assert!((sleep_shares[a_d.index()] - 0.5).abs() < 1e-12);
        assert!((sleep_shares[sub.index()] - 1.0).abs() < 1e-12);
        assert!((sleep_shares[eqz.index()] - 1.0).abs() < 1e-12);
    }
}
