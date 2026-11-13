//! Canonical small-circuit exhaustive enumerator and certified exact-synthesis oracle (Charter §5.2 & §8).
//!
//! Enumerates §2.5-valid minimal topologies in strict size order $N = 0, 1, \dots, N_{\max}$,
//! pruning dead internal wires via linear port conservation, materialising non-zero `Delay { init }`
//! assignments on cycle-reaching registers, and certifying candidates over the closed joint
//! `(generator, circuit)` state graph so $t \to \infty$ optimality is mathematically exact.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::baselines::{
    ObservationWindow, PredictorBaseline, TickBudget, evaluate_circuit_on_window,
    evaluate_circuit_on_window_unmetered,
};
use crate::env::FiniteStateSpec;
use crate::metrics::{circuit_code_bits, compute_mdl};
use crate::substrate::{
    Circuit, CompiledCircuit, NodeId, Primitive, ScalarState, SignalSink, SignalSource, Width,
    WireDelayMode,
};

/// Maximum number of recurrent register-initialisation combinations expanded per canonical topology.
const MAX_DELAY_INIT_COMBOS: u64 = 256;

/// Default upper bound on joint `(generator, circuit)` state transitions before declaring step cap exceeded.
pub const DEFAULT_MAX_CERTIFY_JOINT_STEPS: usize = 4_096;

/// Certification status for small-circuit exact synthesis (§5.2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum SynthesisCertStatus {
    /// Candidate verified to make 0 post-transient errors around the full closed `(generator, circuit)`
    /// state cycle, and every smaller-node candidate was exhaustively refuted.
    CertifiedExact {
        max_nodes_searched: usize,
        min_nodes: usize,
    },
    /// Exhaustively searched `0..=max_nodes_searched` and proved no §2.5-valid circuit achieves 0 errors.
    NoCircuitWithinCap { max_nodes_searched: usize },
    /// `FiniteStateSpec` was unavailable (or exceeded joint step cap), so synthesis used finite prefix replay.
    PrefixOnlyUncertified {
        prefix_ticks: usize,
        min_nodes: Option<usize>,
    },
    /// Exact synthesis was skipped due to `max_nodes` or width/channel enumeration bounds.
    SkippedBoundsExceeded { reason: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CertifyOutcome {
    Certified { lock_tick: usize },
    Rejected,
    StepCapExceeded,
}

/// Generates all canonical minimal-connected §2.5-valid [`Circuit`]s with `0..=max_nodes`
/// primitives in non-decreasing node-count order, expanding non-zero `Delay { init }` assignments
/// on cycle-reaching registers whenever $(2^k)^{|D_{\text{rec}}|} \le 256$.
#[must_use]
pub fn enumerate_canonical_circuits(
    width: Width,
    num_inputs: u16,
    num_outputs: u16,
    wire_delay: WireDelayMode,
    max_nodes: usize,
) -> Vec<Circuit> {
    let mut results = Vec::new();
    for target_nodes in 0..=max_nodes {
        enumerate_exact_size(
            width,
            num_inputs,
            num_outputs,
            wire_delay,
            target_nodes,
            &mut results,
        );
    }
    results
}

fn enumerate_exact_size(
    width: Width,
    num_inputs: u16,
    num_outputs: u16,
    wire_delay: WireDelayMode,
    target_nodes: usize,
    out: &mut Vec<Circuit>,
) {
    let k_in = usize::from(num_inputs);
    let k_out = usize::from(num_outputs);
    let mut size_bucket = Vec::new();

    for num_delays in 0..=target_nodes {
        if k_in == 0 && target_nodes > 0 && num_delays == 0 {
            continue;
        }
        let comb_nodes = target_nodes - num_delays;

        let mut base = Circuit::new(width, num_inputs, num_outputs, wire_delay);
        let mut pool: Vec<(SignalSource, bool)> = (0..num_inputs)
            .map(|p| (SignalSource::BoundaryIn(p), false))
            .collect();
        let mut delay_ids = Vec::with_capacity(num_delays);
        for _ in 0..num_delays {
            let d_id = base.add_node(Primitive::delay_zero());
            delay_ids.push(d_id);
            pool.push((
                SignalSource::NodeOut {
                    node: d_id,
                    port: 0,
                },
                true, // internal wire: must be consumed so no dead node exists!
            ));
        }

        let mut zero_init_topos = Vec::new();
        rec_place_comb(
            &mut base,
            &pool,
            &delay_ids,
            comb_nodes,
            k_out,
            &mut zero_init_topos,
        );

        for topo in zero_init_topos {
            expand_recurrent_delay_inits(width, topo, &mut size_bucket);
        }
    }

    size_bucket.sort_by(|a, b| {
        let mdl_a = circuit_code_bits(a, 0);
        let mdl_b = circuit_code_bits(b, 0);
        let eff_a = a.delay_count() + a.active_boundary_input_count();
        let eff_b = b.delay_count() + b.active_boundary_input_count();
        mdl_a.total_cmp(&mdl_b).then_with(|| eff_a.cmp(&eff_b))
    });
    out.extend(size_bucket);
}

fn expand_recurrent_delay_inits(width: Width, zero_topo: Circuit, out: &mut Vec<Circuit>) {
    let rec_delays = zero_topo.cycle_reaching_delays();
    out.push(zero_topo.clone());
    if rec_delays.is_empty() || width.bits() > 8 {
        return;
    }

    let card = 1u64 << width.bits();
    let Ok(exp) = u32::try_from(rec_delays.len()) else {
        return;
    };
    let Some(total_combos) = card.checked_pow(exp) else {
        return;
    };
    if total_combos > MAX_DELAY_INIT_COMBOS {
        return;
    }

    for combo in 1..total_combos {
        let mut variant = zero_topo.clone();
        let mut code = combo;
        for &d_node in &rec_delays {
            let init_val = code % card;
            code /= card;
            variant.set_delay_init(d_node, init_val);
        }
        out.push(variant);
    }
}

fn rec_place_comb(
    circuit: &mut Circuit,
    pool: &[(SignalSource, bool)],
    delay_ids: &[NodeId],
    comb_rem: usize,
    k_out: usize,
    out: &mut Vec<Circuit>,
) {
    let required_node_sinks = delay_ids.len();
    let max_sinks = required_node_sinks + k_out;

    // Even if every remaining combinational node is `Split` (+1 wire), all `Delay` inputs must be fed.
    if pool.len() + comb_rem < required_node_sinks {
        return;
    }
    // Every internal wire (`is_internal == true`) must eventually be consumed.
    let internal_count = pool.iter().filter(|&&(_, is_int)| is_int).count();
    if internal_count > comb_rem + max_sinks {
        return;
    }

    if comb_rem == 0 {
        if internal_count > max_sinks || pool.len() < required_node_sinks {
            return;
        }
        let mut sinks: Vec<SignalSink> = delay_ids
            .iter()
            .map(|&d| SignalSink::NodeIn { node: d, port: 0 })
            .chain((0..k_out).map(|p| SignalSink::BoundaryOut(p as u16)))
            .collect();
        rec_wire_final_sinks(circuit, pool, &mut sinks, 0, out);
        return;
    }

    if pool.is_empty() {
        return;
    }

    // 1. Unary combinational ops (`Eqz`, `Split`): pick 1 wire `i` from `pool`.
    for i in 0..pool.len() {
        let (src_i, _) = pool[i];
        let mut base_pool = pool.to_vec();
        base_pool.remove(i);

        // 1a. `Eqz`
        {
            let mut next_circ = circuit.clone();
            let id = next_circ.add_node(Primitive::Eqz);
            next_circ.connect(src_i, SignalSink::NodeIn { node: id, port: 0 });
            let mut next_pool = base_pool.clone();
            next_pool.push((SignalSource::NodeOut { node: id, port: 0 }, true));
            rec_place_comb(
                &mut next_circ,
                &next_pool,
                delay_ids,
                comb_rem - 1,
                k_out,
                out,
            );
        }

        // 1b. `Split`
        {
            let mut next_circ = circuit.clone();
            let id = next_circ.add_node(Primitive::Split);
            next_circ.connect(src_i, SignalSink::NodeIn { node: id, port: 0 });
            let mut next_pool = base_pool;
            next_pool.push((SignalSource::NodeOut { node: id, port: 0 }, true));
            next_pool.push((SignalSource::NodeOut { node: id, port: 1 }, true));
            rec_place_comb(
                &mut next_circ,
                &next_pool,
                delay_ids,
                comb_rem - 1,
                k_out,
                out,
            );
        }
    }

    // 2. Binary combinational ops (`Add`, `Sub`, `Mul`): pick 2 distinct wires from `pool`.
    if pool.len() >= 2 {
        for i in 0..pool.len() {
            for j in (i + 1)..pool.len() {
                let (src_i, _) = pool[i];
                let (src_j, _) = pool[j];
                let mut base_pool = pool.to_vec();
                base_pool.remove(j);
                base_pool.remove(i);

                // Commutative ops (`Add`, `Mul`): canonical order `(src_i, src_j)` only!
                for prim in [Primitive::Add, Primitive::Mul] {
                    let mut next_circ = circuit.clone();
                    let id = next_circ.add_node(prim);
                    next_circ.connect(src_i, SignalSink::NodeIn { node: id, port: 0 });
                    next_circ.connect(src_j, SignalSink::NodeIn { node: id, port: 1 });
                    let mut next_pool = base_pool.clone();
                    next_pool.push((SignalSource::NodeOut { node: id, port: 0 }, true));
                    rec_place_comb(
                        &mut next_circ,
                        &next_pool,
                        delay_ids,
                        comb_rem - 1,
                        k_out,
                        out,
                    );
                }

                // Non-commutative `Sub`: omit at k = 1 where `(a - b) mod 2 == (a + b) mod 2`.
                if circuit.width() != Width::BITS_1 {
                    for (lhs, rhs) in [(src_i, src_j), (src_j, src_i)] {
                        let mut next_circ = circuit.clone();
                        let id = next_circ.add_node(Primitive::Sub);
                        next_circ.connect(lhs, SignalSink::NodeIn { node: id, port: 0 });
                        next_circ.connect(rhs, SignalSink::NodeIn { node: id, port: 1 });
                        let mut next_pool = base_pool.clone();
                        next_pool.push((SignalSource::NodeOut { node: id, port: 0 }, true));
                        rec_place_comb(
                            &mut next_circ,
                            &next_pool,
                            delay_ids,
                            comb_rem - 1,
                            k_out,
                            out,
                        );
                    }
                }
            }
        }
    }
}

fn rec_wire_final_sinks(
    circuit: &Circuit,
    pool: &[(SignalSource, bool)],
    sinks: &mut [SignalSink],
    sink_idx: usize,
    out: &mut Vec<Circuit>,
) {
    if sink_idx == sinks.len() {
        // Accept iff no internal wire remains unconsumed (eliminates dead-code bloat).
        if pool.iter().all(|&(_, is_internal)| !is_internal) {
            out.push(circuit.clone());
        }
        return;
    }

    let rem_sinks = sinks.len() - sink_idx;
    let rem_internal = pool.iter().filter(|&&(_, is_int)| is_int).count();
    if rem_internal > rem_sinks {
        return;
    }

    let dst = sinks[sink_idx];

    // Charter §2.5.4 permits `BoundaryOut` ports to remain unconnected (emitting `0` with 0 edges).
    if matches!(dst, SignalSink::BoundaryOut(_)) && rem_internal < rem_sinks {
        rec_wire_final_sinks(circuit, pool, sinks, sink_idx + 1, out);
    }

    for i in 0..pool.len() {
        let (src, _) = pool[i];
        let mut next_pool = pool.to_vec();
        next_pool.remove(i);
        let mut next_circ = circuit.clone();
        next_circ.connect(src, dst);
        rec_wire_final_sinks(&next_circ, &next_pool, sinks, sink_idx + 1, out);
    }
}

fn certify_against_spec_outcome(
    compiled: &CompiledCircuit,
    spec: &FiniteStateSpec,
    max_joint_steps: usize,
) -> CertifyOutcome {
    let k_out = usize::from(compiled.num_outputs());
    let k_in = usize::from(compiled.num_inputs());
    if k_out != usize::from(spec.channels) || (k_in != 0 && k_in != usize::from(spec.channels)) {
        return CertifyOutcome::Rejected;
    }
    if spec.num_states == 0 || spec.initial_state >= spec.num_states {
        return CertifyOutcome::Rejected;
    }

    let uses_in = compiled.uses_boundary_inputs();
    let mut gen_state = spec.initial_state;
    let mut prev_obs = vec![0u64; k_in];
    let mut circuit_state = ScalarState::new(compiled);
    let mut pred = vec![0u64; k_out];

    let mut visited: HashMap<(usize, Vec<u64>, Vec<u64>), usize> = HashMap::new();
    let mut had_mismatch: Vec<bool> = Vec::new();

    for step in 0..max_joint_steps {
        let obs_key = if uses_in {
            prev_obs.clone()
        } else {
            Vec::new()
        };
        let key = (gen_state, obs_key, circuit_state.registers().to_vec());
        if let Some(&cycle_start) = visited.get(&key) {
            if had_mismatch[cycle_start..step].iter().all(|&bad| !bad) {
                let lock_tick = had_mismatch[..cycle_start]
                    .iter()
                    .rposition(|&bad| bad)
                    .map_or(0, |idx| idx + 1);
                return CertifyOutcome::Certified { lock_tick };
            }
            return CertifyOutcome::Rejected;
        }
        visited.insert(key, step);

        let (next_gen_state, ref clean_frame) = spec.transitions[gen_state];
        circuit_state
            .step(compiled, &prev_obs, &mut pred)
            .expect("matching port counts");
        let mismatch = pred
            .iter()
            .zip(clean_frame)
            .any(|(&p, &c)| compiled.width().normalize(p) != compiled.width().normalize(c));
        had_mismatch.push(mismatch);

        if k_in > 0 {
            prev_obs.copy_from_slice(clean_frame);
        }
        gen_state = next_gen_state;
    }

    CertifyOutcome::StepCapExceeded
}

/// Certifies whether `compiled` achieves **zero** prediction errors for all $t \ge t^*$ around the
/// closed joint `(generator, prev_obs, circuit)` state cycle of `spec`.
///
/// Returns `Some(lock_tick)` ($t^*$) when cycle closure occurs within `max_joint_steps` with zero
/// mismatches on the closed cycle, or `None` otherwise.
#[must_use]
pub fn certify_circuit_against_spec(
    compiled: &CompiledCircuit,
    spec: &FiniteStateSpec,
    max_joint_steps: usize,
) -> Option<usize> {
    match certify_against_spec_outcome(compiled, spec, max_joint_steps) {
        CertifyOutcome::Certified { lock_tick } => Some(lock_tick),
        CertifyOutcome::Rejected | CertifyOutcome::StepCapExceeded => None,
    }
}

/// Certified small-circuit exact synthesis (§5.2): searches `0..=max_nodes` in strict node-count
/// order (zero-init before non-zero `init`) and returns the minimal-node [`Circuit`] together with
/// its [`SynthesisCertStatus`].
#[must_use]
pub fn exact_synthesize_certified_circuit(
    width: Width,
    num_channels: u16,
    wire_delay: WireDelayMode,
    spec: Option<&FiniteStateSpec>,
    clean_trace_fallback: &[Vec<u64>],
    max_nodes: usize,
    max_joint_steps: usize,
) -> (Option<Circuit>, SynthesisCertStatus) {
    if num_channels > 2 || max_nodes > 4 || (width.bits() > 8 && max_nodes > 3) {
        return (
            None,
            SynthesisCertStatus::SkippedBoundsExceeded {
                reason: format!(
                    "channels={num_channels}, width={}, max_nodes={max_nodes} exceeds M0 exact-synthesis bounds",
                    width.bits()
                ),
            },
        );
    }

    let candidates =
        enumerate_canonical_circuits(width, num_channels, num_channels, wire_delay, max_nodes);

    if let Some(spec) = spec {
        let mut any_step_cap_exceeded = false;
        for cand in &candidates {
            let compiled = cand
                .compile()
                .expect("canonical enumerator emits §2.5-valid circuits");
            match certify_against_spec_outcome(&compiled, spec, max_joint_steps) {
                CertifyOutcome::Certified { .. } => {
                    if !any_step_cap_exceeded {
                        return (
                            Some(cand.clone()),
                            SynthesisCertStatus::CertifiedExact {
                                max_nodes_searched: max_nodes,
                                min_nodes: cand.node_count(),
                            },
                        );
                    }
                    break;
                }
                CertifyOutcome::Rejected => {}
                CertifyOutcome::StepCapExceeded => {
                    any_step_cap_exceeded = true;
                }
            }
        }

        if !any_step_cap_exceeded {
            return (
                None,
                SynthesisCertStatus::NoCircuitWithinCap {
                    max_nodes_searched: max_nodes,
                },
            );
        }
    }

    let window = ObservationWindow::from_prefix(num_channels, clean_trace_fallback);
    for cand in candidates {
        let compiled = cand
            .compile()
            .expect("canonical enumerator emits §2.5-valid circuits");
        let (mismatches, total, _) = evaluate_circuit_on_window_unmetered(&compiled, &window);
        if total > 0 && mismatches == 0 {
            let n = cand.node_count();
            return (
                Some(cand),
                SynthesisCertStatus::PrefixOnlyUncertified {
                    prefix_ticks: clean_trace_fallback.len(),
                    min_nodes: Some(n),
                },
            );
        }
    }

    (
        None,
        SynthesisCertStatus::PrefixOnlyUncertified {
            prefix_ticks: clean_trace_fallback.len(),
            min_nodes: None,
        },
    )
}

/// Prefix-only synthesis helper retained for callers supplying only a finite `clean_trace`.
#[must_use]
pub fn exact_synthesize_clean_circuit(
    width: Width,
    num_channels: u16,
    wire_delay: WireDelayMode,
    clean_trace: &[Vec<u64>],
    max_nodes: usize,
) -> Option<Circuit> {
    exact_synthesize_certified_circuit(
        width,
        num_channels,
        wire_delay,
        None,
        clean_trace,
        max_nodes,
        DEFAULT_MAX_CERTIFY_JOINT_STEPS,
    )
    .0
}

/// Constructs the explicit 1-effective-register (`0` `Delay`, `1` `BoundaryIn`) nonlinear `Eqz`
/// witness circuit for `T1_PeriodicMotif([0, 1, 3])` over $\mathbb{Z}_4$ ($k = 2$), implementing
/// $f(x) \equiv \text{Eqz}(x) - x - \text{Eqz}(x + 1) \pmod 4$.
///
/// Proves that the universal causal-state bound $\lceil \log_2(3) / 2 \rceil = 1$ register is
/// attainable in the 6-primitive basis while requiring 11 nodes (vs. 2 nodes / 3 effective registers
/// for the node-minimal pure delay line $y_t = y_{t-3}$).
#[must_use]
pub fn build_z4_motif_013_reactive_one_reg_witness() -> Circuit {
    let mut circ = Circuit::new(Width::BITS_2, 1, 1, WireDelayMode::Combinational);

    // Fan-out BoundaryIn(0) into 5 copies via 4 explicit Split nodes.
    let s0 = circ.add_node(Primitive::Split);
    let s1 = circ.add_node(Primitive::Split);
    let s2 = circ.add_node(Primitive::Split);
    let s3 = circ.add_node(Primitive::Split);

    let zero = circ.add_node(Primitive::Sub);
    let one = circ.add_node(Primitive::Eqz);
    let eqz_x = circ.add_node(Primitive::Eqz);
    let x_plus_1 = circ.add_node(Primitive::Add);
    let eqz_xp1 = circ.add_node(Primitive::Eqz);
    let diff1 = circ.add_node(Primitive::Sub);
    let out_val = circ.add_node(Primitive::Sub);

    circ.connect(
        SignalSource::BoundaryIn(0),
        SignalSink::NodeIn { node: s0, port: 0 },
    );
    circ.connect(
        SignalSource::NodeOut { node: s0, port: 0 },
        SignalSink::NodeIn { node: s1, port: 0 },
    );
    circ.connect(
        SignalSource::NodeOut { node: s0, port: 1 },
        SignalSink::NodeIn { node: s2, port: 0 },
    );
    circ.connect(
        SignalSource::NodeOut { node: s2, port: 1 },
        SignalSink::NodeIn { node: s3, port: 0 },
    );

    // zero = x - x = 0; one = Eqz(zero) = 1.
    circ.connect(
        SignalSource::NodeOut { node: s1, port: 0 },
        SignalSink::NodeIn {
            node: zero,
            port: 0,
        },
    );
    circ.connect(
        SignalSource::NodeOut { node: s1, port: 1 },
        SignalSink::NodeIn {
            node: zero,
            port: 1,
        },
    );
    circ.connect(
        SignalSource::NodeOut {
            node: zero,
            port: 0,
        },
        SignalSink::NodeIn { node: one, port: 0 },
    );

    // eqz_x = Eqz(x); x_plus_1 = x + 1; eqz_xp1 = Eqz(x + 1).
    circ.connect(
        SignalSource::NodeOut { node: s2, port: 0 },
        SignalSink::NodeIn {
            node: eqz_x,
            port: 0,
        },
    );
    circ.connect(
        SignalSource::NodeOut { node: s3, port: 0 },
        SignalSink::NodeIn {
            node: x_plus_1,
            port: 0,
        },
    );
    circ.connect(
        SignalSource::NodeOut { node: one, port: 0 },
        SignalSink::NodeIn {
            node: x_plus_1,
            port: 1,
        },
    );
    circ.connect(
        SignalSource::NodeOut {
            node: x_plus_1,
            port: 0,
        },
        SignalSink::NodeIn {
            node: eqz_xp1,
            port: 0,
        },
    );

    // out_val = (Eqz(x) - x) - Eqz(x + 1).
    circ.connect(
        SignalSource::NodeOut {
            node: eqz_x,
            port: 0,
        },
        SignalSink::NodeIn {
            node: diff1,
            port: 0,
        },
    );
    circ.connect(
        SignalSource::NodeOut { node: s3, port: 1 },
        SignalSink::NodeIn {
            node: diff1,
            port: 1,
        },
    );
    circ.connect(
        SignalSource::NodeOut {
            node: diff1,
            port: 0,
        },
        SignalSink::NodeIn {
            node: out_val,
            port: 0,
        },
    );
    circ.connect(
        SignalSource::NodeOut {
            node: eqz_xp1,
            port: 0,
        },
        SignalSink::NodeIn {
            node: out_val,
            port: 1,
        },
    );
    circ.connect(
        SignalSource::NodeOut {
            node: out_val,
            port: 0,
        },
        SignalSink::BoundaryOut(0),
    );

    circ
}

/// Equal-compute canonical small-circuit exhaustive enumeration baseline.
pub struct ExhaustiveBaseline {
    candidates: Vec<(Circuit, CompiledCircuit, f64)>,
    window: ObservationWindow,
    channels: u16,
    last_observed: Vec<u64>,
    best_idx: usize,
    live_state: ScalarState,
    ticks_seen: u64,
}

impl ExhaustiveBaseline {
    /// Pre-enumerates canonical valid circuits up to `max_nodes` and evaluates them online within `TickBudget`.
    #[must_use]
    pub fn new(
        width: Width,
        channels: u16,
        wire_delay: WireDelayMode,
        max_nodes: usize,
        fact_window: usize,
    ) -> Self {
        let raw = enumerate_canonical_circuits(width, channels, channels, wire_delay, max_nodes);
        let mut candidates: Vec<(Circuit, CompiledCircuit, f64)> = raw
            .into_iter()
            .map(|c| {
                let c_bits = circuit_code_bits(&c, 0);
                let comp = c
                    .compile()
                    .expect("canonical enumerator emits §2.5-valid circuits");
                (c, comp, c_bits)
            })
            .collect();
        if candidates.is_empty() {
            let empty = Circuit::new(width, channels, channels, wire_delay);
            let c_bits = circuit_code_bits(&empty, 0);
            let comp = empty.compile().unwrap();
            candidates.push((empty, comp, c_bits));
        }
        let live_state = ScalarState::new(&candidates[0].1);

        Self {
            candidates,
            window: ObservationWindow::new(channels, fact_window.max(24)),
            channels,
            last_observed: vec![0u64; usize::from(channels)],
            best_idx: 0,
            live_state,
            ticks_seen: 0,
        }
    }

    /// Number of canonical circuits in the enumeration bank.
    #[must_use]
    pub fn candidate_count(&self) -> usize {
        self.candidates.len()
    }
}

impl PredictorBaseline for ExhaustiveBaseline {
    fn name(&self) -> &str {
        "Exhaustive"
    }

    fn predict(&mut self, _budget: &mut TickBudget) -> Vec<u64> {
        let mut pred = vec![0u64; usize::from(self.channels)];
        let compiled = &self.candidates[self.best_idx].1;
        self.live_state
            .step(compiled, &self.last_observed, &mut pred)
            .expect("matching port lengths");
        pred
    }

    fn observe(&mut self, truth: &[u64], budget: &mut TickBudget) -> Option<f64> {
        self.ticks_seen += 1;
        self.last_observed.copy_from_slice(truth);
        self.window.push_frame(truth);

        if self.window.len() < 4 {
            return None;
        }

        let (inc_err, _, inc_state) =
            evaluate_circuit_on_window(&self.candidates[self.best_idx].1, &self.window, budget)?;
        self.live_state = inc_state;

        // If current minimal-size champion already makes 0 errors on the window, no search needed!
        if inc_err == 0 {
            return None;
        }

        // Pace full candidate-bank sweeps so early tiny windows (before facts MDL crosses circuit
        // description cost) do not exhaust `TickBudget` prior to informative window horizons.
        let should_sweep = self.ticks_seen == 28
            || (self.ticks_seen >= 64 && (self.ticks_seen - 64).is_multiple_of(32));
        if !should_sweep {
            return None;
        }

        let mut best_mdl = f64::INFINITY;
        for (idx, (circ, comp, circ_bits)) in self.candidates.iter().enumerate() {
            // Exact MDL lower bound: since `facts_bits >= 0`, any candidate whose structural
            // description length alone meets or exceeds `best_mdl` cannot improve `best_mdl`.
            if *circ_bits >= best_mdl {
                continue;
            }
            let Some((err, total, end_state)) =
                evaluate_circuit_on_window(comp, &self.window, budget)
            else {
                break;
            };
            let mdl = compute_mdl(circ, 0.0, 0, total, err).total_bits;
            if mdl < best_mdl {
                best_mdl = mdl;
                self.best_idx = idx;
                self.live_state = end_state;
                if err == 0 {
                    // Since candidates are ordered in strictly non-decreasing node count,
                    // the first 0-error circuit found is minimal in node count!
                    break;
                }
            }
        }

        None
    }

    fn best_circuit(&self) -> Option<&Circuit> {
        Some(&self.candidates[self.best_idx].0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::{FiniteStateGenerator, GeneratorConfig, NoiseModel, solve_epsilon_machine};

    /// Completely independent raw-port-graph enumerator (for `0..=max_nodes`, `max_nodes <= 2`)
    /// that enumerates raw primitive tuples and injective sink-to-source port assignments directly,
    /// relying exclusively on `Circuit::compile()` (§2.5 checker) and `certify_circuit_against_spec`
    /// without sharing any pruning or pool logic with `enumerate_canonical_circuits`.
    fn brute_force_raw_port_min_nodes(
        width: Width,
        num_inputs: u16,
        num_outputs: u16,
        spec: &FiniteStateSpec,
        max_nodes: usize,
    ) -> Option<usize> {
        let mut palette = vec![
            Primitive::Add,
            Primitive::Sub,
            Primitive::Mul,
            Primitive::Eqz,
            Primitive::Split,
        ];
        for init in 0..(1u64 << width.bits()) {
            palette.push(Primitive::Delay { init });
        }

        for n_nodes in 0..=max_nodes {
            let total_tuples = palette.len().pow(n_nodes as u32);
            for mut tuple_code in 0..total_tuples {
                let mut prims = Vec::with_capacity(n_nodes);
                for _ in 0..n_nodes {
                    prims.push(palette[tuple_code % palette.len()]);
                    tuple_code /= palette.len();
                }

                let mut base =
                    Circuit::new(width, num_inputs, num_outputs, WireDelayMode::Combinational);
                let mut srcs: Vec<SignalSource> =
                    (0..num_inputs).map(SignalSource::BoundaryIn).collect();
                let mut req_sinks: Vec<SignalSink> = Vec::new();
                for &p in &prims {
                    let id = base.add_node(p);
                    for port in 0..p.output_arity() {
                        srcs.push(SignalSource::NodeOut { node: id, port });
                    }
                    for port in 0..p.input_arity() {
                        req_sinks.push(SignalSink::NodeIn { node: id, port });
                    }
                }
                if req_sinks.len() > srcs.len() {
                    continue;
                }
                let opt_sinks: Vec<SignalSink> =
                    (0..num_outputs).map(SignalSink::BoundaryOut).collect();

                if search_raw_injective_wirings(
                    &base,
                    &srcs,
                    &mut vec![false; srcs.len()],
                    &req_sinks,
                    &opt_sinks,
                    0,
                    spec,
                ) {
                    return Some(n_nodes);
                }
            }
        }
        None
    }

    fn search_raw_injective_wirings(
        circ: &Circuit,
        srcs: &[SignalSource],
        used_src: &mut [bool],
        req_sinks: &[SignalSink],
        opt_sinks: &[SignalSink],
        step: usize,
        spec: &FiniteStateSpec,
    ) -> bool {
        let total_sinks = req_sinks.len() + opt_sinks.len();
        if step == total_sinks {
            if let Ok(compiled) = circ.compile()
                && certify_circuit_against_spec(&compiled, spec, 1024).is_some()
            {
                return true;
            }
            return false;
        }

        if step < req_sinks.len() {
            let dst = req_sinks[step];
            for i in 0..srcs.len() {
                if !used_src[i] {
                    used_src[i] = true;
                    let mut next = circ.clone();
                    next.connect(srcs[i], dst);
                    if search_raw_injective_wirings(
                        &next,
                        srcs,
                        used_src,
                        req_sinks,
                        opt_sinks,
                        step + 1,
                        spec,
                    ) {
                        return true;
                    }
                    used_src[i] = false;
                }
            }
            false
        } else {
            let dst = opt_sinks[step - req_sinks.len()];
            // Option 1: leave `BoundaryOut` unconnected.
            if search_raw_injective_wirings(
                circ,
                srcs,
                used_src,
                req_sinks,
                opt_sinks,
                step + 1,
                spec,
            ) {
                return true;
            }
            // Option 2: connect one unused source port.
            for i in 0..srcs.len() {
                if !used_src[i] {
                    used_src[i] = true;
                    let mut next = circ.clone();
                    next.connect(srcs[i], dst);
                    if search_raw_injective_wirings(
                        &next,
                        srcs,
                        used_src,
                        req_sinks,
                        opt_sinks,
                        step + 1,
                        spec,
                    ) {
                        return true;
                    }
                    used_src[i] = false;
                }
            }
            false
        }
    }

    #[test]
    fn independent_raw_port_witness_matches_certified_synthesis_across_t1_suite() {
        for width in [Width::BITS_1, Width::BITS_2] {
            let mut configs = GeneratorConfig::m0_t1_suite(width);
            configs.push(GeneratorConfig::T1_Constant { value: 0 });
            for cfg in configs {
                let generator = cfg.build(width).unwrap();
                let spec = generator.to_finite_state_spec().unwrap();
                let raw_min = brute_force_raw_port_min_nodes(width, 1, 1, &spec, 2);
                let (synth_circ, status) = exact_synthesize_certified_circuit(
                    width,
                    1,
                    WireDelayMode::Combinational,
                    Some(&spec),
                    &[],
                    2,
                    1024,
                );
                assert_eq!(
                    synth_circ.as_ref().map(Circuit::node_count),
                    raw_min,
                    "mismatch between canonical synthesis and raw-port witness on {cfg:?} at k={}",
                    width.bits()
                );
                match raw_min {
                    Some(n) => assert_eq!(
                        status,
                        SynthesisCertStatus::CertifiedExact {
                            max_nodes_searched: 2,
                            min_nodes: n,
                        }
                    ),
                    None => assert_eq!(
                        status,
                        SynthesisCertStatus::NoCircuitWithinCap {
                            max_nodes_searched: 2,
                        }
                    ),
                }
            }
        }
    }

    #[test]
    fn autonomous_nonzero_delay_init_synthesizes_constant_three() {
        let generator = GeneratorConfig::T1_Constant { value: 3 }
            .build(Width::BITS_2)
            .unwrap();
        let spec = generator.to_finite_state_spec().unwrap();

        // With `num_inputs = 0` (autonomous), `Delay { init: 3 } <-> Split -> BoundaryOut(0)` (2 nodes) is required!
        let cands =
            enumerate_canonical_circuits(Width::BITS_2, 0, 1, WireDelayMode::Combinational, 2);
        let found = cands
            .into_iter()
            .find(|c| {
                let comp = c.compile().unwrap();
                certify_circuit_against_spec(&comp, &spec, 256).is_some()
            })
            .expect("must synthesize Delay { init: 3 } <-> Split oscillator");
        assert_eq!(found.node_count(), 2);
        assert_eq!(found.delay_count(), 1);
        assert!(found.nodes().contains(&Primitive::Delay { init: 3 }));
    }

    #[test]
    fn certified_synthesis_rejects_zero_node_echo_on_period_257_motif() {
        let mut motif = vec![0u64; 256];
        motif.push(1);
        let generator = GeneratorConfig::T1_PeriodicMotif { motif }
            .build(Width::BITS_1)
            .unwrap();
        let spec = generator.to_finite_state_spec().unwrap();

        let (circ, status) = exact_synthesize_certified_circuit(
            Width::BITS_1,
            1,
            WireDelayMode::Combinational,
            Some(&spec),
            &[],
            3,
            DEFAULT_MAX_CERTIFY_JOINT_STEPS,
        );
        assert!(circ.is_none());
        assert_eq!(
            status,
            SynthesisCertStatus::NoCircuitWithinCap {
                max_nodes_searched: 3,
            }
        );
    }

    #[test]
    fn z4_motif_013_separates_node_minimal_and_state_minimal_witnesses() {
        let generator = GeneratorConfig::T1_PeriodicMotif {
            motif: vec![0, 1, 3],
        }
        .build(Width::BITS_2)
        .unwrap();
        let spec = generator.to_finite_state_spec().unwrap();
        let em = solve_epsilon_machine(&spec, &NoiseModel::None);
        assert_eq!(em.min_delay_registers, 1);

        // 1. Node-minimal circuit: 2 nodes (`Delay -> Delay`), 3 effective registers (`2 Delay + 1 BoundaryIn`).
        let (node_min, status) = exact_synthesize_certified_circuit(
            Width::BITS_2,
            1,
            WireDelayMode::Combinational,
            Some(&spec),
            &[],
            3,
            1024,
        );
        let node_min = node_min.expect("node-minimal circuit exists");
        assert_eq!(
            status,
            SynthesisCertStatus::CertifiedExact {
                max_nodes_searched: 3,
                min_nodes: 2,
            }
        );
        let node_min_comp = node_min.compile().unwrap();
        let node_min_eff_regs = node_min_comp.total_registers()
            + usize::from(node_min_comp.active_boundary_input_count());
        assert_eq!(node_min.node_count(), 2);
        assert_eq!(node_min_eff_regs, 3);

        // 2. State-minimal witness: 11 nodes (`Eqz` nonlinear map), 1 effective register (`0 Delay + 1 BoundaryIn`)!
        let state_min = build_z4_motif_013_reactive_one_reg_witness();
        let state_min_comp = state_min
            .compile()
            .expect("1-register nonlinear Eqz witness satisfies all §2.5 invariants");
        assert_eq!(state_min_comp.total_registers(), 0);
        assert_eq!(state_min_comp.active_boundary_input_count(), 1);
        let eff_regs = state_min_comp.total_registers()
            + usize::from(state_min_comp.active_boundary_input_count());
        assert_eq!(eff_regs, em.min_delay_registers);
        assert_eq!(
            certify_circuit_against_spec(&state_min_comp, &spec, 1024),
            Some(1),
            "1-register Eqz witness must lock at tick 1 and hold 0 errors around the full cycle"
        );
    }
}
