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
    counterfactual_sensitivity_audited(
        circuit,
        compiled,
        state,
        seed_bank,
        tick_block,
        sample_count,
        &mut 0,
    )
}

/// Computes whole-node counterfactual sensitivity while recording audited counterfactual node
/// evaluations into `node_evals` without altering RNG consumption or outputs.
pub fn counterfactual_sensitivity_audited(
    circuit: &Circuit,
    compiled: &CompiledCircuit,
    state: &ScalarState,
    seed_bank: SeedBank,
    tick_block: u64,
    sample_count: usize,
    node_evals: &mut u64,
) -> Result<SensitivitySnapshot, SensitivityError> {
    counterfactual_sensitivity_with_comparators_audited(
        circuit,
        compiled,
        state,
        seed_bank,
        tick_block,
        sample_count,
        &[],
        node_evals,
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
    counterfactual_sensitivity_with_comparators_audited(
        circuit,
        compiled,
        state,
        seed_bank,
        tick_block,
        sample_count,
        comparators,
        &mut 0,
    )
}

/// Target mask controlling which outputs, register latches, and comparators are evaluated during
/// zero-flux sleep sensitivity checks (`R2`).
#[derive(Clone, Copy, Debug)]
pub struct SleepSensitivityMask<'a> {
    /// Per-output-port flags (`true` only when `R5(a)` needs primary ancestry on that port).
    pub include_outputs: &'a [bool],
    /// Per-register latch flags (`true` only for registers in active cones).
    pub include_latches: &'a [bool],
    /// Per-comparator flags (`true` only for active non-suspended comparators).
    pub include_comparators: &'a [bool],
}

/// Computes node-by-target counterfactual sensitivity while recording audited counterfactual node
/// evaluations into `node_evals` without altering RNG consumption or outputs.
#[allow(clippy::too_many_arguments)]
pub fn counterfactual_sensitivity_with_comparators_audited(
    circuit: &Circuit,
    compiled: &CompiledCircuit,
    state: &ScalarState,
    seed_bank: SeedBank,
    tick_block: u64,
    sample_count: usize,
    comparators: &[ComparatorTarget],
    node_evals: &mut u64,
) -> Result<ComparatorSensitivitySnapshot, SensitivityError> {
    counterfactual_sensitivity_core(
        circuit,
        compiled,
        state,
        seed_bank,
        tick_block,
        sample_count,
        comparators,
        None,
        node_evals,
    )
}

/// Computes masked sleep-segment counterfactual sensitivity (`R2`), skipping zero-flux boundary
/// output ports, suspended comparators (`I7`), unused `SUB` sensitivities, and unrelated latches.
#[allow(clippy::too_many_arguments)]
pub fn counterfactual_sensitivity_sleep_masked_audited(
    circuit: &Circuit,
    compiled: &CompiledCircuit,
    state: &ScalarState,
    seed_bank: SeedBank,
    tick_block: u64,
    sample_count: usize,
    comparators: &[ComparatorTarget],
    mask: SleepSensitivityMask<'_>,
    node_evals: &mut u64,
) -> Result<ComparatorSensitivitySnapshot, SensitivityError> {
    counterfactual_sensitivity_core(
        circuit,
        compiled,
        state,
        seed_bank,
        tick_block,
        sample_count,
        comparators,
        Some(mask),
        node_evals,
    )
}

/// Computes the exact topological wake counterfactual sensitivity node evaluations per tick for
/// `circuit` without performing counterfactual substitutions (`R2` wake-envelope reservation).
#[must_use]
pub fn exact_wake_sensitivity_node_evals(
    circuit: &Circuit,
    compiled: &CompiledCircuit,
    sample_count: usize,
    comparators: &[ComparatorTarget],
) -> u64 {
    let dummy_state = ScalarState::new(compiled);
    let mut node_evals = 0u64;
    let Ok((wire_op_offsets, wire_op_list, node_out_base, comp_wires, latch_sources)) =
        build_sensitivity_wiring(circuit, compiled, &dummy_state, comparators)
    else {
        return 0;
    };
    let nodes = circuit.nodes();
    let wires = dummy_state.wire_values();
    let wire_count = wires.len();
    let ops = compiled.comb_ops();
    let width = compiled.width();
    let cand_len = if width.bits() <= EXACT_SENSITIVITY_MAX_BITS {
        ((1u64 << width.bits()) - 1) as usize
    } else {
        let max_distinct = if width.bits() == 64 {
            usize::MAX
        } else {
            ((1u64 << width.bits()) - 1) as usize
        };
        sample_count.min(max_distinct)
    };
    if cand_len == 0 {
        return 0;
    }

    let mut visited_wire = vec![0u32; wire_count];
    let mut visited_op = vec![0u32; ops.len()];
    let mut vis_epoch = 0u32;
    let mut dirty_wires = Vec::with_capacity(wire_count);

    for (node_index, primitive) in nodes.iter().copied().enumerate() {
        let out_base = node_out_base[node_index];
        let out_count = usize::from(primitive.output_arity());
        vis_epoch += 1;
        dirty_wires.clear();
        let mut active_ops_len = 0u64;
        for port in 0..out_count {
            let w = out_base + port;
            visited_wire[w] = vis_epoch;
            dirty_wires.push(w);
        }
        let mut qi = 0usize;
        while qi < dirty_wires.len() {
            let w = dirty_wires[qi];
            qi += 1;
            let start = wire_op_offsets[w];
            let end = wire_op_offsets[w + 1];
            for &op_idx in &wire_op_list[start..end] {
                if visited_op[op_idx] == vis_epoch {
                    continue;
                }
                let op = ops[op_idx];
                if comb_op_output_base(op) == out_base {
                    continue;
                }
                visited_op[op_idx] = vis_epoch;
                active_ops_len += 1;
                match op {
                    CombOp::Add { out, .. }
                    | CombOp::Sub { out, .. }
                    | CombOp::Mul { out, .. }
                    | CombOp::Eqz { out, .. } => {
                        if visited_wire[out] != vis_epoch {
                            visited_wire[out] = vis_epoch;
                            dirty_wires.push(out);
                        }
                    }
                    CombOp::Split { out0, out1, .. } => {
                        if visited_wire[out0] != vis_epoch {
                            visited_wire[out0] = vis_epoch;
                            dirty_wires.push(out0);
                        }
                        if visited_wire[out1] != vis_epoch {
                            visited_wire[out1] = vis_epoch;
                            dirty_wires.push(out1);
                        }
                    }
                }
            }
        }
        let reaches_target = compiled
            .output_drivers()
            .iter()
            .any(|driver| driver.is_some_and(|w| visited_wire[w] == vis_epoch))
            || latch_sources
                .iter()
                .any(|driver| driver.is_some_and(|w| visited_wire[w] == vis_epoch))
            || comp_wires
                .iter()
                .any(|&(sw, ew)| visited_wire[sw] == vis_epoch || visited_wire[ew] == vis_epoch);
        if reaches_target {
            node_evals = node_evals
                .saturating_add((cand_len as u64).saturating_mul(active_ops_len.saturating_add(1)));
        }
    }
    node_evals
}

#[allow(clippy::type_complexity)]
fn build_sensitivity_wiring(
    circuit: &Circuit,
    compiled: &CompiledCircuit,
    state: &ScalarState,
    comparators: &[ComparatorTarget],
) -> Result<
    (
        Vec<usize>,
        Vec<usize>,
        Vec<usize>,
        Vec<(usize, usize)>,
        Vec<Option<usize>>,
    ),
    SensitivityError,
> {
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

    let latch_count = compiled.total_registers();
    let mut latch_sources = vec![None; latch_count];
    for &(register, input_wire) in compiled.delay_latches() {
        latch_sources[register] = Some(input_wire);
    }
    for chain in compiled.shift_chains() {
        latch_sources[chain.base_reg] = Some(chain.src_wire);
    }

    let wire_count = wires.len();
    let ops = compiled.comb_ops();
    let mut op_count_per_wire = vec![0usize; wire_count];
    for op in ops {
        match *op {
            CombOp::Add { lhs, rhs, .. }
            | CombOp::Sub { lhs, rhs, .. }
            | CombOp::Mul { lhs, rhs, .. } => {
                op_count_per_wire[lhs] += 1;
                if rhs != lhs {
                    op_count_per_wire[rhs] += 1;
                }
            }
            CombOp::Eqz { arg, .. } | CombOp::Split { arg, .. } => {
                op_count_per_wire[arg] += 1;
            }
        }
    }
    let mut wire_op_offsets = Vec::with_capacity(wire_count + 1);
    let mut running = 0usize;
    for &cnt in &op_count_per_wire {
        wire_op_offsets.push(running);
        running += cnt;
    }
    wire_op_offsets.push(running);
    let mut wire_op_cursor = wire_op_offsets[..wire_count].to_vec();
    let mut wire_op_list = vec![0usize; running];
    for (op_idx, op) in ops.iter().enumerate() {
        let push_wire = |w: usize, cursor: &mut [usize], list: &mut [usize]| {
            let pos = cursor[w];
            list[pos] = op_idx;
            cursor[w] = pos + 1;
        };
        match *op {
            CombOp::Add { lhs, rhs, .. }
            | CombOp::Sub { lhs, rhs, .. }
            | CombOp::Mul { lhs, rhs, .. } => {
                push_wire(lhs, &mut wire_op_cursor, &mut wire_op_list);
                if rhs != lhs {
                    push_wire(rhs, &mut wire_op_cursor, &mut wire_op_list);
                }
            }
            CombOp::Eqz { arg, .. } | CombOp::Split { arg, .. } => {
                push_wire(arg, &mut wire_op_cursor, &mut wire_op_list);
            }
        }
    }
    Ok((
        wire_op_offsets,
        wire_op_list,
        node_out_base,
        comp_wires,
        latch_sources,
    ))
}

#[allow(clippy::too_many_arguments)]
fn counterfactual_sensitivity_core(
    circuit: &Circuit,
    compiled: &CompiledCircuit,
    state: &ScalarState,
    seed_bank: SeedBank,
    tick_block: u64,
    sample_count: usize,
    comparators: &[ComparatorTarget],
    sleep_mask: Option<SleepSensitivityMask<'_>>,
    node_evals: &mut u64,
) -> Result<ComparatorSensitivitySnapshot, SensitivityError> {
    let (wire_op_offsets, wire_op_list, node_out_base, comp_wires, latch_sources) =
        build_sensitivity_wiring(circuit, compiled, state, comparators)?;
    let nodes = circuit.nodes();
    let wires = state.wire_values();
    if compiled.width().bits() > EXACT_SENSITIVITY_MAX_BITS && sample_count == 0 {
        return Err(SensitivityError::EmptySample);
    }

    let output_baseline: Vec<u64> = compiled
        .output_drivers()
        .iter()
        .map(|wire| wire.map_or(0, |index| wires[index]))
        .collect();
    let latch_count = compiled.total_registers();
    if let Some(mask) = sleep_mask
        && (mask.include_outputs.len() != output_baseline.len()
            || mask.include_latches.len() != latch_count
            || mask.include_comparators.len() != comparators.len())
    {
        return Err(SensitivityError::DimensionMismatch);
    }

    let wire_count = wires.len();
    let ops = compiled.comb_ops();

    let mut output_by_node = vec![vec![0.0; output_baseline.len()]; nodes.len()];
    let mut latch_by_node = vec![vec![0.0; latch_count]; nodes.len()];
    let mut comparator_sub_by_node = vec![vec![0.0; comparators.len()]; nodes.len()];
    let mut comparator_eqz_by_node = vec![vec![0.0; comparators.len()]; nodes.len()];
    let mut rng = seed_bank.stream(StreamDomain::SensitivityProbe { tick_block });
    let width = compiled.width();
    let mut scratch = wires.to_vec();
    let mut visited_wire = vec![0u32; wire_count];
    let mut visited_op = vec![0u32; ops.len()];
    let mut vis_epoch = 0u32;
    let mut dirty_wires: Vec<usize> = Vec::with_capacity(wire_count);
    let mut active_ops: Vec<usize> = Vec::with_capacity(ops.len());
    let mut affected_ports: Vec<(usize, usize)> = Vec::with_capacity(output_baseline.len());
    let mut affected_latches: Vec<(usize, usize)> = Vec::with_capacity(latch_count);
    let mut affected_comp_subs: Vec<(usize, usize)> = Vec::with_capacity(comparators.len());
    let mut affected_comp_eqzs: Vec<(usize, usize)> = Vec::with_capacity(comparators.len());
    let mut stack_cands = [0u64; 16];
    let mut heap_cands = Vec::new();

    for (node_index, primitive) in nodes.iter().copied().enumerate() {
        let out_base = node_out_base[node_index];
        let out_count = usize::from(primitive.output_arity());
        let actual = wires[out_base];
        let cand_len = if width.bits() <= EXACT_SENSITIVITY_MAX_BITS {
            fill_exact_candidate_values(width, actual, &mut stack_cands)
        } else {
            heap_cands = candidate_values(width, actual, sample_count, &mut rng)?;
            heap_cands.len()
        };
        let candidates: &[u64] = if width.bits() <= EXACT_SENSITIVITY_MAX_BITS {
            &stack_cands[..cand_len]
        } else {
            &heap_cands
        };

        vis_epoch += 1;
        dirty_wires.clear();
        active_ops.clear();
        for port in 0..out_count {
            let w = out_base + port;
            visited_wire[w] = vis_epoch;
            dirty_wires.push(w);
        }
        let mut qi = 0usize;
        while qi < dirty_wires.len() {
            let w = dirty_wires[qi];
            qi += 1;
            let start = wire_op_offsets[w];
            let end = wire_op_offsets[w + 1];
            for &op_idx in &wire_op_list[start..end] {
                if visited_op[op_idx] == vis_epoch {
                    continue;
                }
                let op = ops[op_idx];
                let op_out_base = comb_op_output_base(op);
                if op_out_base == out_base {
                    continue;
                }
                visited_op[op_idx] = vis_epoch;
                active_ops.push(op_idx);
                match op {
                    CombOp::Add { out, .. }
                    | CombOp::Sub { out, .. }
                    | CombOp::Mul { out, .. }
                    | CombOp::Eqz { out, .. } => {
                        if visited_wire[out] != vis_epoch {
                            visited_wire[out] = vis_epoch;
                            dirty_wires.push(out);
                        }
                    }
                    CombOp::Split { out0, out1, .. } => {
                        if visited_wire[out0] != vis_epoch {
                            visited_wire[out0] = vis_epoch;
                            dirty_wires.push(out0);
                        }
                        if visited_wire[out1] != vis_epoch {
                            visited_wire[out1] = vis_epoch;
                            dirty_wires.push(out1);
                        }
                    }
                }
            }
        }
        if active_ops.len() > 1 {
            active_ops.sort_unstable();
        }

        affected_ports.clear();
        for (port, driver) in compiled.output_drivers().iter().enumerate() {
            if sleep_mask.is_some_and(|m| !m.include_outputs[port]) {
                continue;
            }
            if let Some(wire) = *driver
                && visited_wire[wire] == vis_epoch
            {
                affected_ports.push((port, wire));
            }
        }
        affected_latches.clear();
        for (register, input_wire) in latch_sources.iter().enumerate() {
            if sleep_mask.is_some_and(|m| !m.include_latches[register]) {
                continue;
            }
            if let Some(wire) = *input_wire
                && visited_wire[wire] == vis_epoch
            {
                affected_latches.push((register, wire));
            }
        }
        affected_comp_subs.clear();
        affected_comp_eqzs.clear();
        for (m, &(sub_wire, eqz_wire)) in comp_wires.iter().enumerate() {
            if let Some(mask) = sleep_mask {
                if mask.include_comparators[m] && visited_wire[eqz_wire] == vis_epoch {
                    affected_comp_eqzs.push((m, eqz_wire));
                }
            } else {
                if visited_wire[sub_wire] == vis_epoch {
                    affected_comp_subs.push((m, sub_wire));
                }
                if visited_wire[eqz_wire] == vis_epoch {
                    affected_comp_eqzs.push((m, eqz_wire));
                }
            }
        }

        if affected_ports.is_empty()
            && affected_latches.is_empty()
            && affected_comp_subs.is_empty()
            && affected_comp_eqzs.is_empty()
        {
            continue;
        }

        *node_evals = node_evals.saturating_add(
            (candidates.len() as u64).saturating_mul((active_ops.len() as u64).saturating_add(1)),
        );

        for &candidate in candidates {
            for port in 0..out_count {
                scratch[out_base + port] = candidate;
            }
            for &op_idx in &active_ops {
                eval_comb_op(ops[op_idx], &mut scratch, width);
            }
            for &(port, wire) in &affected_ports {
                if scratch[wire] != output_baseline[port] {
                    output_by_node[node_index][port] += 1.0;
                }
            }
            for &(register, wire) in &affected_latches {
                if scratch[wire] != wires[wire] {
                    latch_by_node[node_index][register] += 1.0;
                }
            }
            for &(m, sub_wire) in &affected_comp_subs {
                if scratch[sub_wire] != wires[sub_wire] {
                    comparator_sub_by_node[node_index][m] += 1.0;
                }
            }
            for &(m, eqz_wire) in &affected_comp_eqzs {
                if scratch[eqz_wire] != wires[eqz_wire] {
                    comparator_eqz_by_node[node_index][m] += 1.0;
                }
            }
            for &w in &dirty_wires {
                scratch[w] = wires[w];
            }
        }
        let denominator = candidates.len() as f64;
        if denominator > 0.0 {
            for &(port, _) in &affected_ports {
                output_by_node[node_index][port] /= denominator;
            }
            for &(register, _) in &affected_latches {
                latch_by_node[node_index][register] /= denominator;
            }
            for &(m, _) in &affected_comp_subs {
                comparator_sub_by_node[node_index][m] /= denominator;
            }
            for &(m, _) in &affected_comp_eqzs {
                comparator_eqz_by_node[node_index][m] /= denominator;
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
        None,
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
    assemble_causal_ancestry_with_comparators_masked(
        circuit,
        compiled,
        state,
        sensitivity,
        activity,
        register_eligibility,
        trace_discount,
        comparators,
        is_sleep,
        None,
    )
}

/// Propagates masked sleep counterfactual sensitivity backward across only active outputs, active
/// register latches, and non-suspended comparators (`R2`, `I7`).
#[expect(
    clippy::too_many_arguments,
    reason = "Masked sleep causal ancestry shares core validation and propagation with unmasked path"
)]
pub fn assemble_causal_ancestry_sleep_masked(
    circuit: &Circuit,
    compiled: &CompiledCircuit,
    state: &ScalarState,
    sensitivity: &ComparatorSensitivitySnapshot,
    activity: &[NodeActivity],
    register_eligibility: &[Vec<f64>],
    trace_discount: f64,
    comparators: &[ComparatorTarget],
    mask: SleepSensitivityMask<'_>,
) -> Result<ComparatorCausalAncestry, SensitivityError> {
    assemble_causal_ancestry_with_comparators_masked(
        circuit,
        compiled,
        state,
        sensitivity,
        activity,
        register_eligibility,
        trace_discount,
        comparators,
        true,
        Some(mask),
    )
}

#[allow(clippy::too_many_arguments)]
fn assemble_causal_ancestry_with_comparators_masked(
    circuit: &Circuit,
    compiled: &CompiledCircuit,
    state: &ScalarState,
    sensitivity: &ComparatorSensitivitySnapshot,
    activity: &[NodeActivity],
    register_eligibility: &[Vec<f64>],
    trace_discount: f64,
    comparators: &[ComparatorTarget],
    is_sleep: bool,
    sleep_mask: Option<SleepSensitivityMask<'_>>,
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
        sleep_mask,
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
    sleep_mask: Option<SleepSensitivityMask<'_>>,
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
    if let Some(mask) = sleep_mask
        && (mask.include_outputs.len() != compiled.output_drivers().len()
            || mask.include_latches.len() != register_count
            || mask.include_comparators.len() != comparators.len())
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

    let mut base_wire_ancestry = vec![0.0; wire_count * node_count];
    let mut base_nonzero = vec![false; wire_count];
    for &(register, wire) in compiled.delay_reads() {
        if let Some(trace) = register_eligibility.get(register) {
            let dst_slice = &mut base_wire_ancestry[wire * node_count..(wire + 1) * node_count];
            let mut any_nz = false;
            for (dst, &value) in dst_slice.iter_mut().zip(trace) {
                let v = trace_discount * value;
                *dst = v;
                if v != 0.0 {
                    any_nz = true;
                }
            }
            if any_nz {
                base_nonzero[wire] = true;
            }
        }
    }

    let mut delay_nodes = Vec::new();
    for (node, primitive) in circuit.nodes().iter().copied().enumerate() {
        if matches!(primitive, Primitive::Delay { .. }) {
            let Some(register) = delay_register_of_node[node] else {
                return Err(SensitivityError::DimensionMismatch);
            };
            delay_nodes.push((node, register, node_out_base[node]));
        }
    }

    let wire_values = state.wire_values();
    let comb_meta = build_comb_op_ancestry_meta(
        circuit,
        compiled,
        wire_values,
        activity,
        &out_wire_to_node,
        &wire_source_nodes,
    )?;

    let mut wire_ancestry = vec![0.0; wire_count * node_count];
    let mut wire_epoch = vec![0u32; wire_count];
    let mut wire_zero_epoch = vec![0u32; wire_count];
    let mut cur_epoch = 0u32;

    for (port, shares) in port_shares.iter_mut().enumerate() {
        if sleep_mask.is_some_and(|m| !m.include_outputs[port]) {
            continue;
        }
        let Some(driver) = compiled.output_drivers()[port] else {
            continue;
        };
        cur_epoch += 1;
        ancestry_for_target_fast(
            node_count,
            sensitivity,
            comparators,
            register_eligibility,
            trace_discount,
            &delay_nodes,
            &comb_meta,
            &base_wire_ancestry,
            &base_nonzero,
            Target::Output(port),
            cur_epoch,
            &mut wire_ancestry,
            &mut wire_epoch,
            &mut wire_zero_epoch,
        )?;
        copy_wire_ancestry_out(
            driver,
            node_count,
            cur_epoch,
            &wire_ancestry,
            &wire_epoch,
            &wire_zero_epoch,
            &base_wire_ancestry,
            &base_nonzero,
            shares,
        );
    }

    let mut register_input_wires = vec![None; register_count];
    for &(register, input_wire) in compiled.delay_latches() {
        register_input_wires[register] = Some(input_wire);
    }
    for chain in compiled.shift_chains() {
        register_input_wires[chain.base_reg] = Some(chain.src_wire);
        for stage in 1..chain.hops {
            let current = chain.base_reg + stage;
            if sleep_mask.is_some_and(|m| !m.include_latches[current]) {
                continue;
            }
            let previous = chain.base_reg + stage - 1;
            for node in 0..node_count {
                next_register_eligibility[current][node] =
                    trace_discount * register_eligibility[previous][node];
            }
        }
    }
    for (register, input_wire_opt) in register_input_wires.iter().enumerate() {
        if sleep_mask.is_some_and(|m| !m.include_latches[register]) {
            continue;
        }
        let Some(input_wire) = *input_wire_opt else {
            continue;
        };
        cur_epoch += 1;
        ancestry_for_target_fast(
            node_count,
            sensitivity,
            comparators,
            register_eligibility,
            trace_discount,
            &delay_nodes,
            &comb_meta,
            &base_wire_ancestry,
            &base_nonzero,
            Target::Register(register),
            cur_epoch,
            &mut wire_ancestry,
            &mut wire_epoch,
            &mut wire_zero_epoch,
        )?;
        copy_wire_ancestry_out(
            input_wire,
            node_count,
            cur_epoch,
            &wire_ancestry,
            &wire_epoch,
            &wire_zero_epoch,
            &base_wire_ancestry,
            &base_nonzero,
            &mut next_register_eligibility[register],
        );
    }

    for (m, comp) in comparators.iter().enumerate() {
        if sleep_mask.is_some_and(|mask| !mask.include_comparators[m]) {
            continue;
        }
        let target = if is_sleep {
            Target::ComparatorSleep(m)
        } else {
            Target::ComparatorWake(m)
        };
        cur_epoch += 1;
        ancestry_for_target_fast(
            node_count,
            sensitivity,
            comparators,
            register_eligibility,
            trace_discount,
            &delay_nodes,
            &comb_meta,
            &base_wire_ancestry,
            &base_nonzero,
            target,
            cur_epoch,
            &mut wire_ancestry,
            &mut wire_epoch,
            &mut wire_zero_epoch,
        )?;
        let eqz_wire = node_out_base[comp.eqz_node.index()];
        copy_wire_ancestry_out(
            eqz_wire,
            node_count,
            cur_epoch,
            &wire_ancestry,
            &wire_epoch,
            &wire_zero_epoch,
            &base_wire_ancestry,
            &base_nonzero,
            &mut comparator_shares[m],
        );
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

struct CombOpAncestryMeta {
    node: usize,
    out0: usize,
    out1: Option<usize>,
    arity: usize,
    in_wires: [usize; 2],
    extra_in_wires: Vec<usize>,
    default_weights: [f64; 2],
}

fn build_comb_op_ancestry_meta(
    circuit: &Circuit,
    compiled: &CompiledCircuit,
    wire_values: &[u64],
    activity: &[NodeActivity],
    out_wire_to_node: &[usize],
    wire_source_nodes: &[Option<NodeId>],
) -> Result<Vec<CombOpAncestryMeta>, SensitivityError> {
    let mut meta = Vec::with_capacity(compiled.comb_ops().len());
    let mut in_vals_heap = Vec::new();
    let mut in_act_heap = Vec::new();
    for operation in compiled.comb_ops() {
        let (first_out, out1) = match *operation {
            CombOp::Add { out, .. }
            | CombOp::Sub { out, .. }
            | CombOp::Mul { out, .. }
            | CombOp::Eqz { out, .. } => (out, None),
            CombOp::Split { out0, out1, .. } => (out0, Some(out1)),
        };
        let node = *out_wire_to_node
            .get(first_out)
            .filter(|&&idx| idx != usize::MAX)
            .ok_or(SensitivityError::DimensionMismatch)?;
        let input_wires = &compiled.node_input_wires()[node];
        let arity = input_wires.len();
        let prim = circuit.nodes()[node];
        let is_mul_consumer = matches!(prim, Primitive::Mul);

        let mut in_vals_stack = [0u64; 2];
        let mut in_act_stack = [0.0f64; 2];
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
        let default_weights = if arity == 1 {
            [1.0, 0.0]
        } else {
            ring_identity_input_weights_pair(
                prim,
                [in_vals[0], in_vals[1]],
                [in_act[0], in_act[1]],
                compiled.width(),
            )
        };
        let in_wires = [
            input_wires.first().copied().unwrap_or(0),
            input_wires.get(1).copied().unwrap_or(0),
        ];
        let extra_in_wires = if arity > 2 {
            input_wires[2..].to_vec()
        } else {
            Vec::new()
        };
        meta.push(CombOpAncestryMeta {
            node,
            out0: first_out,
            out1,
            arity,
            in_wires,
            extra_in_wires,
            default_weights,
        });
    }
    Ok(meta)
}

#[allow(clippy::too_many_arguments)]
fn copy_wire_ancestry_out(
    wire: usize,
    node_count: usize,
    cur_epoch: u32,
    wire_ancestry: &[f64],
    wire_epoch: &[u32],
    wire_zero_epoch: &[u32],
    base_wire_ancestry: &[f64],
    base_nonzero: &[bool],
    dst: &mut [f64],
) {
    if wire_zero_epoch[wire] == cur_epoch {
        dst.fill(0.0);
    } else if wire_epoch[wire] == cur_epoch {
        dst.copy_from_slice(&wire_ancestry[wire * node_count..(wire + 1) * node_count]);
    } else if base_nonzero[wire] {
        dst.copy_from_slice(&base_wire_ancestry[wire * node_count..(wire + 1) * node_count]);
    } else {
        dst.fill(0.0);
    }
}

#[derive(Clone, Copy)]
enum WireSliceKind {
    Zero,
    Curr(usize),
    Base(usize),
}

#[inline]
fn classify_wire(
    wire: usize,
    cur_epoch: u32,
    wire_epoch: &[u32],
    wire_zero_epoch: &[u32],
    base_nonzero: &[bool],
) -> WireSliceKind {
    if wire_zero_epoch[wire] == cur_epoch {
        WireSliceKind::Zero
    } else if wire_epoch[wire] == cur_epoch {
        WireSliceKind::Curr(wire)
    } else if base_nonzero[wire] {
        WireSliceKind::Base(wire)
    } else {
        WireSliceKind::Zero
    }
}

#[allow(clippy::too_many_arguments)]
fn ancestry_for_target_fast(
    node_count: usize,
    sensitivity: SensitivityView<'_>,
    comparators: &[ComparatorTarget],
    register_eligibility: &[Vec<f64>],
    trace_discount: f64,
    delay_nodes: &[(usize, usize, usize)],
    comb_meta: &[CombOpAncestryMeta],
    base_wire_ancestry: &[f64],
    base_nonzero: &[bool],
    target: Target,
    cur_epoch: u32,
    wire_ancestry: &mut [f64],
    wire_epoch: &mut [u32],
    wire_zero_epoch: &mut [u32],
) -> Result<(), SensitivityError> {
    for &(node, register, wire) in delay_nodes {
        let coefficient = target_sensitivity(sensitivity, comparators, node, target)?;
        if coefficient == 0.0 {
            wire_zero_epoch[wire] = cur_epoch;
            continue;
        }
        wire_epoch[wire] = cur_epoch;
        let dst_slice = &mut wire_ancestry[wire * node_count..(wire + 1) * node_count];
        let reg_elig = &register_eligibility[register];
        for (ancestor, (dst, &elig)) in dst_slice.iter_mut().zip(reg_elig).enumerate() {
            let inherited = trace_discount * elig;
            let raw = (if ancestor == node { 1.0 } else { 0.0 }) + inherited;
            *dst = coefficient * raw.min(1.0);
        }
    }

    for op in comb_meta {
        let node = op.node;
        let coefficient = target_sensitivity(sensitivity, comparators, node, target)?;
        if coefficient == 0.0 {
            wire_zero_epoch[op.out0] = cur_epoch;
            if let Some(out1) = op.out1 {
                wire_zero_epoch[out1] = cur_epoch;
            }
            continue;
        }

        let weights = match target {
            Target::ComparatorWake(m) if node == comparators[m].sub_node.index() => [0.0, 1.0],
            Target::ComparatorSleep(m) if node == comparators[m].sub_node.index() => [0.5, 0.5],
            _ => op.default_weights,
        };

        let out_start = op.out0 * node_count;
        let out_end = out_start + node_count;
        wire_epoch[op.out0] = cur_epoch;

        if op.arity == 0 || !op.extra_in_wires.is_empty() {
            let dst = &mut wire_ancestry[out_start..out_end];
            dst.fill(0.0);
            dst[node] = coefficient;
        } else if op.arity == 1 {
            let k0 = classify_wire(
                op.in_wires[0],
                cur_epoch,
                wire_epoch,
                wire_zero_epoch,
                base_nonzero,
            );
            let w0 = weights[0];
            match k0 {
                WireSliceKind::Zero => {
                    let dst = &mut wire_ancestry[out_start..out_end];
                    dst.fill(0.0);
                    dst[node] = coefficient;
                }
                WireSliceKind::Curr(src_w) => {
                    let src_start = src_w * node_count;
                    let (dst, src) =
                        borrow_two_wire_slices_mut(wire_ancestry, out_start, src_start, node_count);
                    for (ancestor, (d, &s)) in dst.iter_mut().zip(src).enumerate() {
                        let inherited = w0 * s;
                        let raw = (if ancestor == node { 1.0 } else { 0.0 }) + inherited;
                        *d = coefficient * raw.min(1.0);
                    }
                }
                WireSliceKind::Base(src_w) => {
                    let src = &base_wire_ancestry[src_w * node_count..(src_w + 1) * node_count];
                    let dst = &mut wire_ancestry[out_start..out_end];
                    for (ancestor, (d, &s)) in dst.iter_mut().zip(src).enumerate() {
                        let inherited = w0 * s;
                        let raw = (if ancestor == node { 1.0 } else { 0.0 }) + inherited;
                        *d = coefficient * raw.min(1.0);
                    }
                }
            }
        } else {
            let k0 = classify_wire(
                op.in_wires[0],
                cur_epoch,
                wire_epoch,
                wire_zero_epoch,
                base_nonzero,
            );
            let k1 = classify_wire(
                op.in_wires[1],
                cur_epoch,
                wire_epoch,
                wire_zero_epoch,
                base_nonzero,
            );
            let w0 = weights[0];
            let w1 = weights[1];
            match (k0, k1) {
                (WireSliceKind::Zero, WireSliceKind::Zero) => {
                    let dst = &mut wire_ancestry[out_start..out_end];
                    dst.fill(0.0);
                    dst[node] = coefficient;
                }
                (WireSliceKind::Curr(s0), WireSliceKind::Zero) => {
                    let (dst, src0) = borrow_two_wire_slices_mut(
                        wire_ancestry,
                        out_start,
                        s0 * node_count,
                        node_count,
                    );
                    for (ancestor, (d, &v0)) in dst.iter_mut().zip(src0).enumerate() {
                        let inherited = w0 * v0;
                        let raw = (if ancestor == node { 1.0 } else { 0.0 }) + inherited;
                        *d = coefficient * raw.min(1.0);
                    }
                }
                (WireSliceKind::Base(s0), WireSliceKind::Zero) => {
                    let src0 = &base_wire_ancestry[s0 * node_count..(s0 + 1) * node_count];
                    let dst = &mut wire_ancestry[out_start..out_end];
                    for (ancestor, (d, &v0)) in dst.iter_mut().zip(src0).enumerate() {
                        let inherited = w0 * v0;
                        let raw = (if ancestor == node { 1.0 } else { 0.0 }) + inherited;
                        *d = coefficient * raw.min(1.0);
                    }
                }
                (WireSliceKind::Zero, WireSliceKind::Curr(s1)) => {
                    let (dst, src1) = borrow_two_wire_slices_mut(
                        wire_ancestry,
                        out_start,
                        s1 * node_count,
                        node_count,
                    );
                    for (ancestor, (d, &v1)) in dst.iter_mut().zip(src1).enumerate() {
                        let inherited = w1 * v1;
                        let raw = (if ancestor == node { 1.0 } else { 0.0 }) + inherited;
                        *d = coefficient * raw.min(1.0);
                    }
                }
                (WireSliceKind::Zero, WireSliceKind::Base(s1)) => {
                    let src1 = &base_wire_ancestry[s1 * node_count..(s1 + 1) * node_count];
                    let dst = &mut wire_ancestry[out_start..out_end];
                    for (ancestor, (d, &v1)) in dst.iter_mut().zip(src1).enumerate() {
                        let inherited = w1 * v1;
                        let raw = (if ancestor == node { 1.0 } else { 0.0 }) + inherited;
                        *d = coefficient * raw.min(1.0);
                    }
                }
                (k0, k1) => {
                    for ancestor in 0..node_count {
                        let mut inherited = 0.0;
                        let v0 = match k0 {
                            WireSliceKind::Zero => 0.0,
                            WireSliceKind::Curr(s0) => wire_ancestry[s0 * node_count + ancestor],
                            WireSliceKind::Base(s0) => {
                                base_wire_ancestry[s0 * node_count + ancestor]
                            }
                        };
                        let v1 = match k1 {
                            WireSliceKind::Zero => 0.0,
                            WireSliceKind::Curr(s1) => wire_ancestry[s1 * node_count + ancestor],
                            WireSliceKind::Base(s1) => {
                                base_wire_ancestry[s1 * node_count + ancestor]
                            }
                        };
                        inherited += w0 * v0;
                        inherited += w1 * v1;
                        let raw = (if ancestor == node { 1.0 } else { 0.0 }) + inherited;
                        wire_ancestry[out_start + ancestor] = coefficient * raw.min(1.0);
                    }
                }
            }
        }

        if let Some(out1) = op.out1 {
            wire_epoch[out1] = cur_epoch;
            let o1_start = out1 * node_count;
            let (dst1, src0) =
                borrow_two_wire_slices_mut(wire_ancestry, o1_start, out_start, node_count);
            dst1.copy_from_slice(src0);
        }
    }
    Ok(())
}

#[inline]
fn borrow_two_wire_slices_mut(
    buf: &mut [f64],
    dst_start: usize,
    src_start: usize,
    len: usize,
) -> (&mut [f64], &[f64]) {
    if dst_start < src_start {
        let (left, right) = buf.split_at_mut(src_start);
        (&mut left[dst_start..dst_start + len], &right[..len])
    } else {
        let (left, right) = buf.split_at_mut(dst_start);
        (&mut right[..len], &left[src_start..src_start + len])
    }
}

#[inline]
fn fill_exact_candidate_values(width: Width, actual: u64, out: &mut [u64; 16]) -> usize {
    let actual = width.normalize(actual);
    let cardinality = 1u64 << width.bits();
    let mut len = 0usize;
    for value in 0..cardinality {
        if value != actual {
            out[len] = value;
            len += 1;
        }
    }
    len
}

#[inline]
const fn comb_op_output_base(op: CombOp) -> usize {
    match op {
        CombOp::Add { out, .. }
        | CombOp::Sub { out, .. }
        | CombOp::Mul { out, .. }
        | CombOp::Eqz { out, .. } => out,
        CombOp::Split { out0, .. } => out0,
    }
}

#[inline]
fn eval_comb_op(op: CombOp, wires: &mut [u64], width: Width) {
    match op {
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
