//! Planted spurious comparator derivations for Milestone M4a (`PL1`, `PL2`; Plan §2.1).
//!
//! All injections route through [`ArkheLearner::apply_external_comparator_edit`] (which delegates to
//! [`ArkheLearner::apply_external_edit`]) so `exogenous_injection_bits` and `I4` epistemic MDL
//! conservation hold identically across injection and sleep.

use std::collections::{BTreeMap, BTreeSet};

use crate::metrics::circuit_code_bits_with_tax;
use crate::structural::learner::InjectionMode;
use crate::structural::surgery::{explicit_delay_register_index, register_mapping};
use crate::structural::{
    ArkheLearner, build_autonomous_ring_source, cone_channel_mask, materialize_const_loop,
    splice_comparator_onto_port, tap_source,
};
use crate::substrate::{
    Circuit, NodeId, Primitive, ScalarState, SignalSink, SignalSource, WireDelayMode,
};

/// Receipt describing an externally injected planted spurious comparator.
#[derive(Clone, Debug, PartialEq)]
pub struct PlantedInjectionReceipt {
    /// Registered comparator index on `learner.editable().economy().comparators()`.
    pub comp_idx: usize,
    /// Injected comparator `Sub` node id.
    pub sub_node: NodeId,
    /// Injected comparator `Eqz` node id.
    pub eqz_node: NodeId,
    /// All newly injected node ids (alt cone + comparator glue + inserted tap `Split`).
    pub injected_nodes: Vec<NodeId>,
    /// Pre-injection circuit code length (bits).
    pub pre_code_bits: f64,
    /// Post-injection circuit code length (bits).
    pub post_code_bits: f64,
    /// Delta credited to `exogenous_injection_bits` (`0.0` in `InjectionMode::Juvenile`).
    pub delta_exog_bits: f64,
}

fn find_port_driver(circuit: &Circuit, target_port: u16) -> Option<SignalSource> {
    circuit
        .edges()
        .iter()
        .find(|e| e.dst == SignalSink::BoundaryOut(target_port))
        .map(|e| e.src)
}

/// Injects `PL1` (short-cycle autonomous ring alt) onto `target_port` (`§2.1`).
///
/// Constructs an autonomous ring whose period (`short_motif.len()`) divides the true generator's
/// period and agrees with facts on part of the orbit, aligned to `phase_tick` (typically
/// `learner.tick()`), and registers it via [`ArkheLearner::apply_external_comparator_edit`].
///
/// # Errors
///
/// Returns an error string if `short_motif` is empty, `target_port` is undriven, compilation fails,
/// or comparator registration fails.
pub fn inject_pl1_short_cycle_alt(
    learner: &mut ArkheLearner,
    target_port: u16,
    short_motif: &[u64],
    phase_tick: u64,
    mode: InjectionMode,
) -> Result<PlantedInjectionReceipt, String> {
    if short_motif.is_empty() {
        return Err("PL1 short_motif must be non-empty".to_owned());
    }
    let pre_circuit = learner.editable().circuit().clone();
    if target_port >= pre_circuit.num_outputs() {
        return Err(format!("PL1 target_port {target_port} out of bounds"));
    }
    let primary_src = find_port_driver(&pre_circuit, target_port)
        .ok_or_else(|| format!("PL1 target_port {target_port} is undriven"))?;
    let width = pre_circuit.width();
    let norm_motif: Vec<u64> = short_motif.iter().map(|&v| width.normalize(v)).collect();
    let period = norm_motif.len();

    let pre_n = pre_circuit.node_count();
    let old_to_new: Vec<Option<NodeId>> = (0..pre_n).map(|i| Some(NodeId(i as u32))).collect();
    let pre_compiled = learner.editable().compiled().clone();
    let pre_regs = learner.editable().state().registers().to_vec();
    let split_tax = learner.economy_config().split_code_tax;
    let pre_code_bits = circuit_code_bits_with_tax(&pre_circuit, 0, split_tax);
    let pre_exog = learner.exogenous_injection_bits();

    let mut edited = pre_circuit.clone();
    let (ring_delays, _ring_split, alt_src) =
        build_autonomous_ring_source(&mut edited, period, &norm_motif);
    let spliced = splice_comparator_onto_port(&mut edited, target_port, alt_src)
        .ok_or_else(|| format!("failed to splice PL1 comparator onto port {target_port}"))?;

    let edited_compiled = edited
        .compile()
        .map_err(|e| format!("PL1 edited compile failed: {e}"))?;
    let reg_map = register_mapping(
        &pre_circuit,
        &edited,
        &pre_compiled,
        &edited_compiled,
        &old_to_new,
    );
    let mut post_regs = ScalarState::new(&edited_compiled).registers().to_vec();
    for (old_r, mapped) in reg_map.into_iter().enumerate() {
        if let Some(new_r) = mapped
            && old_r < pre_regs.len()
            && new_r < post_regs.len()
        {
            post_regs[new_r] = pre_regs[old_r];
        }
    }
    for (j, &delay_node) in ring_delays.iter().enumerate() {
        let reg_idx = explicit_delay_register_index(&edited, delay_node)
            .ok_or_else(|| "missing register index for PL1 ring Delay".to_owned())?;
        post_regs[reg_idx] = norm_motif[((phase_tick as usize) + (period - 1 - j)) % period];
    }

    let post_code_bits = circuit_code_bits_with_tax(&edited, 0, split_tax);
    let injected: Vec<NodeId> = (pre_n..edited.node_count())
        .map(|i| NodeId(i as u32))
        .collect();
    let primary_mask = cone_channel_mask(&edited, primary_src);
    let alt_mask = 0u64;

    let comp_idx = learner
        .apply_external_comparator_edit(
            edited,
            &old_to_new,
            &injected,
            mode,
            spliced.sub_node,
            spliced.eqz_node,
            target_port,
            primary_mask,
            alt_mask,
            Some(&post_regs),
        )
        .map_err(|e| format!("PL1 apply_external_comparator_edit failed: {e}"))?;

    let delta_exog_bits = learner.exogenous_injection_bits() - pre_exog;
    Ok(PlantedInjectionReceipt {
        comp_idx,
        sub_node: spliced.sub_node,
        eqz_node: spliced.eqz_node,
        injected_nodes: injected,
        pre_code_bits,
        post_code_bits,
        delta_exog_bits,
    })
}

fn clone_primary_cone(
    orig_circuit: &Circuit,
    root_src: SignalSource,
    edited: &mut Circuit,
) -> Result<(BTreeMap<NodeId, NodeId>, SignalSource), String> {
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

    let mut orig_to_clone = BTreeMap::new();
    for &orig_node in &cone_nodes {
        let prim = orig_circuit
            .nodes()
            .get(orig_node.index())
            .copied()
            .ok_or_else(|| "invalid orig node in primary cone".to_owned())?;
        let new_id = edited.add_node(prim);
        orig_to_clone.insert(orig_node, new_id);
    }

    for edge in orig_circuit.edges() {
        if let SignalSink::NodeIn { node: dst, port } = edge.dst
            && cone_nodes.contains(&dst)
        {
            let cloned_dst = SignalSink::NodeIn {
                node: orig_to_clone[&dst],
                port,
            };
            let cloned_src = match edge.src {
                SignalSource::BoundaryIn(c) => tap_source(edited, SignalSource::BoundaryIn(c)),
                SignalSource::NodeOut { node, port: p } => SignalSource::NodeOut {
                    node: orig_to_clone[&node],
                    port: p,
                },
            };
            edited.connect_hops(cloned_src, cloned_dst, edge.hops);
        }
    }

    let cloned_root = match root_src {
        SignalSource::BoundaryIn(c) => tap_source(edited, SignalSource::BoundaryIn(c)),
        SignalSource::NodeOut { node, port } => SignalSource::NodeOut {
            node: orig_to_clone[&node],
            port,
        },
    };
    Ok((orig_to_clone, cloned_root))
}

/// Injects `PL2` (rare-context wrong alt) onto `target_port` (`§2.1`).
///
/// Clones the port's primary derivation cone and adds a conjunctive indicator
/// $\prod_{(c, d, v) \in \text{clauses}} \mathrm{Eqz}(x_c(t - 1 - d) - v) \in \{0, 1\}$,
/// producing an `alt` derivation that agrees identically with `primary` everywhere except on ticks
/// matching all rare-context clauses (where it differs by `+1 mod 2^k` at both $k = 1$ and $k = 2$).
///
/// # Errors
///
/// Returns an error string if `clauses` is empty, any channel is out of range, `target_port` is
/// undriven, `wire_delay == WireDelayMode::TicksPerHop`, compilation fails, or registration fails.
pub fn inject_pl2_rare_context_alt(
    learner: &mut ArkheLearner,
    target_port: u16,
    clauses: &[(u16, usize, u64)],
    mode: InjectionMode,
) -> Result<PlantedInjectionReceipt, String> {
    if clauses.is_empty() {
        return Err("PL2 requires at least one rare-context clause".to_owned());
    }
    let pre_circuit = learner.editable().circuit().clone();
    if pre_circuit.wire_delay() == WireDelayMode::TicksPerHop {
        return Err("PL2 injection currently requires Combinational wire delay mode".to_owned());
    }
    if target_port >= pre_circuit.num_outputs() {
        return Err(format!("PL2 target_port {target_port} out of bounds"));
    }
    for &(ch, _, _) in clauses {
        if ch >= pre_circuit.num_inputs() {
            return Err(format!(
                "PL2 clause channel {ch} out of range 0..{}",
                pre_circuit.num_inputs()
            ));
        }
    }
    let primary_src = find_port_driver(&pre_circuit, target_port)
        .ok_or_else(|| format!("PL2 target_port {target_port} is undriven"))?;
    let width = pre_circuit.width();

    let pre_n = pre_circuit.node_count();
    let old_to_new: Vec<Option<NodeId>> = (0..pre_n).map(|i| Some(NodeId(i as u32))).collect();
    let pre_compiled = learner.editable().compiled().clone();
    let pre_regs = learner.editable().state().registers().to_vec();
    let split_tax = learner.economy_config().split_code_tax;
    let pre_code_bits = circuit_code_bits_with_tax(&pre_circuit, 0, split_tax);
    let pre_exog = learner.exogenous_injection_bits();

    let mut edited = pre_circuit.clone();
    let (orig_to_clone, cloned_primary) =
        clone_primary_cone(&pre_circuit, primary_src, &mut edited)?;

    // Build shared delay chains per referenced boundary input channel.
    let mut max_delay_by_ch: BTreeMap<u16, usize> = BTreeMap::new();
    for &(ch, d, _) in clauses {
        let slot = max_delay_by_ch.entry(ch).or_insert(0);
        *slot = (*slot).max(d);
    }

    let mut chain_nodes_by_ch: BTreeMap<u16, Vec<NodeId>> = BTreeMap::new();
    for (&ch, &max_d) in &max_delay_by_ch {
        let mut chain = Vec::with_capacity(max_d);
        let mut cur_src = SignalSource::BoundaryIn(ch);
        for _ in 0..max_d {
            let tapped = tap_source(&mut edited, cur_src);
            let d_node = edited.add_node(Primitive::Delay { init: 0 });
            edited.connect(
                tapped,
                SignalSink::NodeIn {
                    node: d_node,
                    port: 0,
                },
            );
            cur_src = SignalSource::NodeOut {
                node: d_node,
                port: 0,
            };
            chain.push(d_node);
        }
        chain_nodes_by_ch.insert(ch, chain);
    }

    let mut clause_indicators = Vec::with_capacity(clauses.len());
    for &(ch, d, sym) in clauses {
        let raw_sig = if d == 0 {
            SignalSource::BoundaryIn(ch)
        } else {
            SignalSource::NodeOut {
                node: chain_nodes_by_ch[&ch][d - 1],
                port: 0,
            }
        };
        let sig = tap_source(&mut edited, raw_sig);
        let norm_sym = width.normalize(sym);
        let ind = if norm_sym == 0 {
            let eqz = edited.add_node(Primitive::Eqz);
            edited.connect(sig, SignalSink::NodeIn { node: eqz, port: 0 });
            SignalSource::NodeOut { node: eqz, port: 0 }
        } else {
            let c_src = materialize_const_loop(&mut edited, norm_sym);
            let sub = edited.add_node(Primitive::Sub);
            let eqz = edited.add_node(Primitive::Eqz);
            edited.connect(sig, SignalSink::NodeIn { node: sub, port: 0 });
            edited.connect(c_src, SignalSink::NodeIn { node: sub, port: 1 });
            edited.connect(
                SignalSource::NodeOut { node: sub, port: 0 },
                SignalSink::NodeIn { node: eqz, port: 0 },
            );
            SignalSource::NodeOut { node: eqz, port: 0 }
        };
        clause_indicators.push(ind);
    }

    let mut rare_ind = clause_indicators[0];
    for &ind in &clause_indicators[1..] {
        let mul = edited.add_node(Primitive::Mul);
        edited.connect(rare_ind, SignalSink::NodeIn { node: mul, port: 0 });
        edited.connect(ind, SignalSink::NodeIn { node: mul, port: 1 });
        rare_ind = SignalSource::NodeOut { node: mul, port: 0 };
    }

    let add_out = edited.add_node(Primitive::Add);
    let base_tap = tap_source(&mut edited, cloned_primary);
    edited.connect(
        base_tap,
        SignalSink::NodeIn {
            node: add_out,
            port: 0,
        },
    );
    edited.connect(
        rare_ind,
        SignalSink::NodeIn {
            node: add_out,
            port: 1,
        },
    );
    let alt_src = SignalSource::NodeOut {
        node: add_out,
        port: 0,
    };

    let spliced = splice_comparator_onto_port(&mut edited, target_port, alt_src)
        .ok_or_else(|| format!("failed to splice PL2 comparator onto port {target_port}"))?;

    let edited_compiled = edited
        .compile()
        .map_err(|e| format!("PL2 edited compile failed: {e}"))?;
    let reg_map = register_mapping(
        &pre_circuit,
        &edited,
        &pre_compiled,
        &edited_compiled,
        &old_to_new,
    );
    let mut post_regs = ScalarState::new(&edited_compiled).registers().to_vec();
    for (old_r, mapped) in reg_map.into_iter().enumerate() {
        if let Some(new_r) = mapped
            && old_r < pre_regs.len()
            && new_r < post_regs.len()
        {
            post_regs[new_r] = pre_regs[old_r];
        }
    }
    // Synchronize cloned primary Delay registers to live primary register values.
    for (&orig_node, &clone_node) in &orig_to_clone {
        if let (Some(orig_r), Some(clone_r)) = (
            explicit_delay_register_index(&pre_circuit, orig_node),
            explicit_delay_register_index(&edited, clone_node),
        ) && orig_r < pre_regs.len()
            && clone_r < post_regs.len()
        {
            post_regs[clone_r] = pre_regs[orig_r];
        }
    }
    // Synchronize rare-context Delay chains from trailing wake frames in FactBuffer.
    let fb = learner.fact_buffer();
    let fb_len = fb.len();
    if fb_len > 0 {
        for (&ch, chain) in &chain_nodes_by_ch {
            let ch_idx = usize::from(ch);
            for (d_idx, &d_node) in chain.iter().enumerate() {
                let lag_from_prev = d_idx + 1;
                let sym = if fb_len > lag_from_prev {
                    fb.frame_at(fb_len - 1 - lag_from_prev)
                        .and_then(|f| f.get(ch_idx).copied())
                        .unwrap_or(0)
                } else if fb_len == lag_from_prev {
                    fb.prior_frame().get(ch_idx).copied().unwrap_or(0)
                } else {
                    0
                };
                if let Some(r_idx) = explicit_delay_register_index(&edited, d_node)
                    && r_idx < post_regs.len()
                {
                    post_regs[r_idx] = width.normalize(sym);
                }
            }
        }
    }

    let post_code_bits = circuit_code_bits_with_tax(&edited, 0, split_tax);
    let injected: Vec<NodeId> = (pre_n..edited.node_count())
        .map(|i| NodeId(i as u32))
        .collect();
    let primary_mask = cone_channel_mask(&edited, primary_src);
    let alt_mask = cone_channel_mask(&edited, alt_src);

    let comp_idx = learner
        .apply_external_comparator_edit(
            edited,
            &old_to_new,
            &injected,
            mode,
            spliced.sub_node,
            spliced.eqz_node,
            target_port,
            primary_mask,
            alt_mask,
            Some(&post_regs),
        )
        .map_err(|e| format!("PL2 apply_external_comparator_edit failed: {e}"))?;

    let delta_exog_bits = learner.exogenous_injection_bits() - pre_exog;
    Ok(PlantedInjectionReceipt {
        comp_idx,
        sub_node: spliced.sub_node,
        eqz_node: spliced.eqz_node,
        injected_nodes: injected,
        pre_code_bits,
        post_code_bits,
        delta_exog_bits,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::SeedBank;
    use crate::structural::{EconomyConfig, GrowthConfig};
    use crate::substrate::Width;

    #[test]
    fn planted_injection_validation_rejects_empty_and_undriven_inputs() {
        let mut learner = ArkheLearner::new_with_config(
            Width::BITS_1,
            2,
            WireDelayMode::Combinational,
            EconomyConfig::default(),
            GrowthConfig::default(),
            SeedBank::new(1),
        )
        .unwrap();

        assert!(
            inject_pl1_short_cycle_alt(&mut learner, 0, &[], 0, InjectionMode::Established)
                .is_err()
        );
        assert!(
            inject_pl1_short_cycle_alt(&mut learner, 0, &[0, 1], 0, InjectionMode::Established)
                .is_err()
        );
        assert!(
            inject_pl2_rare_context_alt(&mut learner, 0, &[], InjectionMode::Established).is_err()
        );
        assert!(
            inject_pl2_rare_context_alt(&mut learner, 0, &[(5, 0, 1)], InjectionMode::Established)
                .is_err()
        );
    }
}
