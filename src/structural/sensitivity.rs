//! Counterfactual causal sensitivity and modular-identity input gating.

use std::collections::BTreeSet;
use std::fmt;

use rand::Rng;

use crate::rng::{SeedBank, StreamDomain};
use crate::substrate::eval::CombOp;
use crate::substrate::{
    Circuit, CompiledCircuit, NodeId, Primitive, ScalarState, SignalSource, Width,
};

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

/// Monotonic per-node emission history used for ring-identity gating.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NodeActivity {
    /// Node has emitted a value other than 0 (additive identity).
    pub ever_nonzero: bool,
    /// Node has emitted a value other than 1 (multiplicative identity).
    pub ever_non_one: bool,
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
pub fn counterfactual_sensitivity(
    circuit: &Circuit,
    compiled: &CompiledCircuit,
    state: &ScalarState,
    seed_bank: SeedBank,
    tick_block: u64,
    sample_count: usize,
) -> Result<SensitivitySnapshot, SensitivityError> {
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
    if compiled.width().bits() > 4 && sample_count == 0 {
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
        }
        let denominator = candidates.len() as f64;
        if denominator > 0.0 {
            for sensitivity in &mut output_by_node[node_index] {
                *sensitivity /= denominator;
            }
            for sensitivity in &mut latch_by_node[node_index] {
                *sensitivity /= denominator;
            }
        }
    }

    Ok(SensitivitySnapshot {
        output_by_node,
        latch_by_node,
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

    let mut port_shares = vec![vec![0.0; node_count]; compiled.output_drivers().len()];
    for (port, shares) in port_shares.iter_mut().enumerate() {
        let wire_ancestry = ancestry_for_target(
            circuit,
            compiled,
            state,
            sensitivity,
            activity,
            register_eligibility,
            trace_discount,
            &node_out_base,
            &delay_register_of_node,
            Target::Output(port),
        )?;
        if let Some(driver) = compiled.output_drivers()[port] {
            shares.copy_from_slice(&wire_ancestry[driver]);
        }
    }

    let mut next_register_eligibility = vec![vec![0.0; node_count]; register_count];
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
    for register in 0..register_count {
        let Some(input_wire) = register_input_wires[register] else {
            continue;
        };
        let wire_ancestry = ancestry_for_target(
            circuit,
            compiled,
            state,
            sensitivity,
            activity,
            register_eligibility,
            trace_discount,
            &node_out_base,
            &delay_register_of_node,
            Target::Register(register),
        )?;
        next_register_eligibility[register].copy_from_slice(&wire_ancestry[input_wire]);
    }

    Ok(CausalAncestry {
        port_shares,
        next_register_eligibility,
    })
}

#[derive(Clone, Copy)]
enum Target {
    Output(usize),
    Register(usize),
}

#[allow(clippy::too_many_arguments)]
fn ancestry_for_target(
    circuit: &Circuit,
    compiled: &CompiledCircuit,
    state: &ScalarState,
    sensitivity: &SensitivitySnapshot,
    activity: &[NodeActivity],
    register_eligibility: &[Vec<f64>],
    trace_discount: f64,
    node_out_base: &[usize],
    delay_register_of_node: &[Option<usize>],
    target: Target,
) -> Result<Vec<Vec<f64>>, SensitivityError> {
    let node_count = circuit.node_count();
    let mut wire_ancestry = vec![vec![0.0; node_count]; state.wire_values().len()];
    for &(register, wire) in compiled.delay_reads() {
        if let Some(trace) = register_eligibility.get(register) {
            for (dst, &value) in wire_ancestry[wire].iter_mut().zip(trace) {
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
        let coefficient = target_sensitivity(sensitivity, node, target)?;
        let wire = node_out_base[node];
        for ancestor in 0..node_count {
            let inherited = trace_discount * register_eligibility[register][ancestor];
            let raw = (if ancestor == node { 1.0 } else { 0.0 }) + inherited;
            wire_ancestry[wire][ancestor] = coefficient * raw.min(1.0);
        }
    }

    for operation in compiled.comb_ops() {
        let (node, output_wires) = match *operation {
            CombOp::Add { out, .. }
            | CombOp::Sub { out, .. }
            | CombOp::Mul { out, .. }
            | CombOp::Eqz { out, .. } => {
                let node = node_out_base
                    .iter()
                    .position(|&base| base == out)
                    .ok_or(SensitivityError::DimensionMismatch)?;
                (node, vec![out])
            }
            CombOp::Split { out0, out1, .. } => {
                let node = node_out_base
                    .iter()
                    .position(|&base| base == out0)
                    .ok_or(SensitivityError::DimensionMismatch)?;
                (node, vec![out0, out1])
            }
        };
        let coefficient = target_sensitivity(sensitivity, node, target)?;
        let input_wires = &compiled.node_input_wires()[node];
        let input_values: Vec<u64> = input_wires
            .iter()
            .map(|&wire| state.wire_values()[wire])
            .collect();
        let is_mul_consumer = matches!(circuit.nodes()[node], Primitive::Mul);
        let input_active: Vec<bool> = input_wires
            .iter()
            .map(|&wire| {
                source_node_for_wire(circuit, wire).is_some_and(|source| {
                    let act = activity[source.index()];
                    if is_mul_consumer {
                        act.ever_non_one
                    } else {
                        act.ever_nonzero
                    }
                })
            })
            .collect();
        let weights = ring_identity_input_weights(
            circuit.nodes()[node],
            &input_values,
            &input_active,
            compiled.width(),
        );
        let mut node_ancestry = vec![0.0; node_count];
        for (ancestor, value) in node_ancestry.iter_mut().enumerate() {
            let inherited: f64 = input_wires
                .iter()
                .zip(&weights)
                .map(|(&wire, &weight)| weight * wire_ancestry[wire][ancestor])
                .sum();
            let raw = (if ancestor == node { 1.0 } else { 0.0 }) + inherited;
            *value = coefficient * raw.min(1.0);
        }
        for wire in output_wires {
            wire_ancestry[wire].copy_from_slice(&node_ancestry);
        }
    }
    Ok(wire_ancestry)
}

fn target_sensitivity(
    sensitivity: &SensitivitySnapshot,
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
    if width.bits() <= 4 {
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

/// Returns normalised ring-identity-aware input weights for one primitive.
///
/// `input_active[j]` is the monotonic `ever_active` flag for the source of input `j`.
#[must_use]
pub fn ring_identity_input_weights(
    primitive: Primitive,
    inputs: &[u64],
    input_active: &[bool],
    width: Width,
) -> Vec<f64> {
    let arity = usize::from(primitive.input_arity());
    if inputs.len() != arity || input_active.len() != arity {
        return vec![0.0; arity];
    }
    if arity == 1 {
        return vec![1.0];
    }
    let values = [width.normalize(inputs[0]), width.normalize(inputs[1])];
    let mut raw = [1.0, 1.0];
    match primitive {
        Primitive::Add => {
            if values[0] == 0 && values[1] != 0 {
                raw[0] = 0.0;
            } else if values[1] == 0 && values[0] != 0 {
                raw[1] = 0.0;
            } else if values[0] == 0 && values[1] == 0 {
                if !input_active[0] {
                    raw[0] = 0.0;
                }
                if !input_active[1] {
                    raw[1] = 0.0;
                }
            }
        }
        Primitive::Sub => {
            if values[1] == 0 && (values[0] != 0 || !input_active[1]) {
                raw[1] = 0.0;
            }
        }
        Primitive::Mul => {
            let cardinality = 2.0f64.powi(i32::from(width.bits()));
            for input in 0..2 {
                let other = 1 - input;
                if values[input] == 1 && !input_active[input] {
                    raw[input] = 0.0;
                    continue;
                }
                let factor = values[other];
                let divisor = if factor == 0 {
                    cardinality
                } else {
                    2.0f64.powi(
                        i32::try_from(factor.trailing_zeros().min(u32::from(width.bits())))
                            .expect("trailing-zero count fits i32"),
                    )
                };
                raw[input] = cardinality - divisor;
            }
        }
        _ => {}
    }
    let total = raw[0] + raw[1];
    if total == 0.0 {
        vec![0.0, 0.0]
    } else {
        vec![raw[0] / total, raw[1] / total]
    }
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
            ring_identity_input_weights(Primitive::Add, &[3, 0], &[true, false], Width::BITS_2),
            vec![1.0, 0.0]
        );
        assert_eq!(
            ring_identity_input_weights(Primitive::Sub, &[1, 0], &[true, false], Width::BITS_2),
            vec![1.0, 0.0]
        );
        assert_eq!(
            ring_identity_input_weights(Primitive::Add, &[0, 0], &[false, true], Width::BITS_2),
            vec![0.0, 1.0]
        );
    }

    #[test]
    fn multiplication_weights_equal_exact_local_perturbation_counts() {
        let weights =
            ring_identity_input_weights(Primitive::Mul, &[2, 1], &[true, true], Width::BITS_2);
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
                ever_nonzero: true,
                ever_non_one: false,
            },
            NodeActivity {
                ever_nonzero: true,
                ever_non_one: false,
            },
            NodeActivity {
                ever_nonzero: false,
                ever_non_one: true,
            },
            NodeActivity {
                ever_nonzero: false,
                ever_non_one: true,
            },
            NodeActivity {
                ever_nonzero: false,
                ever_non_one: true,
            },
            NodeActivity {
                ever_nonzero: false,
                ever_non_one: false,
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
                ever_nonzero: true,
                ever_non_one: false,
            },
            NodeActivity {
                ever_nonzero: true,
                ever_non_one: false,
            },
            NodeActivity {
                ever_nonzero: true,
                ever_non_one: false,
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
                ever_nonzero: true,
                ever_non_one: false,
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
}
