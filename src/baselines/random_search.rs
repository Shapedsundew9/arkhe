//! Budgeted constructive random circuit search baseline (Charter §8).
//!
//! Samples invariant-valid circuits directly from the linear wire-pool conservation rules so
//! every sampled candidate satisfies Charter §2.5 Invariants 1–5 without rejection waste.

use rand_chacha::ChaCha8Rng;
use rand_core::Rng;

use crate::baselines::{
    ObservationWindow, PredictorBaseline, TickBudget, evaluate_circuit_on_window,
};
use crate::env::noise::uniform_u64_below;
use crate::metrics::compute_mdl;
use crate::rng::{SeedBank, StreamDomain};
use crate::structural::WiringTopology;
use crate::substrate::{
    Circuit, CompiledCircuit, NodeId, Primitive, ScalarState, SignalSink, SignalSource, Width,
    WireDelayMode,
};

/// Constructively samples a random valid [`Circuit`] with `0..=max_nodes` primitives.
#[must_use]
pub fn sample_random_valid_circuit(
    rng: &mut ChaCha8Rng,
    width: Width,
    num_inputs: u16,
    num_outputs: u16,
    wire_delay: WireDelayMode,
    max_nodes: usize,
) -> Circuit {
    let k_in = usize::from(num_inputs);
    let k_out = usize::from(num_outputs);

    let target_nodes = if max_nodes == 0 {
        0
    } else {
        uniform_u64_below(rng, (max_nodes + 1) as u64) as usize
    };

    let mut circuit = Circuit::new(width, num_inputs, num_outputs, wire_delay);
    if k_in == 0 && target_nodes == 0 {
        return circuit;
    }

    // Choose how many of `target_nodes` are explicit `Delay` registers (`0..=target_nodes`).
    // If k_in == 0, we need at least 1 Delay so the initial wire pool is non-empty.
    let min_delays = usize::from(k_in == 0 && target_nodes > 0);
    let num_delays = if target_nodes < min_delays {
        0
    } else {
        min_delays + (uniform_u64_below(rng, (target_nodes - min_delays + 1) as u64) as usize)
    };
    let mut comb_budget = target_nodes.saturating_sub(num_delays);

    let mut pool: Vec<SignalSource> = (0..num_inputs).map(SignalSource::BoundaryIn).collect();
    let mut delay_nodes: Vec<NodeId> = Vec::with_capacity(num_delays);
    for _ in 0..num_delays {
        let init = if (rng.next_u64() & 3) == 0 {
            rng.next_u64() & width.mask()
        } else {
            0
        };
        let d_id = circuit.add_node(Primitive::Delay { init });
        delay_nodes.push(d_id);
        pool.push(SignalSource::NodeOut {
            node: d_id,
            port: 0,
        });
    }

    let draw_source = |rng: &mut ChaCha8Rng, pool: &mut Vec<SignalSource>| -> SignalSource {
        let idx = uniform_u64_below(rng, pool.len() as u64) as usize;
        pool.swap_remove(idx)
    };

    // Required sinks at end: all `num_delays` Delay inputs + up to `k_out` boundary outputs.
    // We ensure `pool.len()` always stays >= `num_delays + min_driven_outs`.
    let required_final_wires = num_delays + k_out.min(pool.len() + comb_budget);

    while comb_budget > 0 && !pool.is_empty() {
        // Compute net wire change allowed so `pool.len() + delta + (comb_budget - 1) >= required_final_wires`.
        let can_shrink = pool.len() >= 2 && (pool.len() + comb_budget - 2) >= required_final_wires;
        let must_grow = (pool.len() + comb_budget - 1) < required_final_wires;

        let prim = if must_grow {
            Primitive::Split
        } else if can_shrink {
            match uniform_u64_below(rng, 5) {
                0 => Primitive::Add,
                1 => Primitive::Sub,
                2 => Primitive::Mul,
                3 => Primitive::Eqz,
                _ => Primitive::Split,
            }
        } else if uniform_u64_below(rng, 2) == 0 {
            Primitive::Eqz
        } else {
            Primitive::Split
        };

        match prim {
            Primitive::Add | Primitive::Sub | Primitive::Mul => {
                let s0 = draw_source(rng, &mut pool);
                let s1 = draw_source(rng, &mut pool);
                let id = circuit.add_node(prim);
                circuit.connect(s0, SignalSink::NodeIn { node: id, port: 0 });
                circuit.connect(s1, SignalSink::NodeIn { node: id, port: 1 });
                pool.push(SignalSource::NodeOut { node: id, port: 0 });
            }
            Primitive::Eqz => {
                let s0 = draw_source(rng, &mut pool);
                let id = circuit.add_node(Primitive::Eqz);
                circuit.connect(s0, SignalSink::NodeIn { node: id, port: 0 });
                pool.push(SignalSource::NodeOut { node: id, port: 0 });
            }
            Primitive::Split => {
                let s0 = draw_source(rng, &mut pool);
                let id = circuit.add_node(Primitive::Split);
                circuit.connect(s0, SignalSink::NodeIn { node: id, port: 0 });
                pool.push(SignalSource::NodeOut { node: id, port: 0 });
                pool.push(SignalSource::NodeOut { node: id, port: 1 });
            }
            Primitive::Delay { .. } => unreachable!(),
        }

        comb_budget -= 1;
    }

    // If pool still lacks wires for all Delay inputs (only possible if k_in == 0 and comb_budget == 0),
    // top up with Split nodes? Note: when k_in == 0, pool started with `num_delays` wires == delay_nodes.len(),
    // so pool.len() >= num_delays is ALREADY guaranteed!
    for d_id in delay_nodes {
        let src = draw_source(rng, &mut pool);
        circuit.connect(
            src,
            SignalSink::NodeIn {
                node: d_id,
                port: 0,
            },
        );
    }

    for out_p in 0..num_outputs {
        if pool.is_empty() {
            break;
        }
        let src = draw_source(rng, &mut pool);
        circuit.connect(src, SignalSink::BoundaryOut(out_p));
    }

    circuit
}

/// Equal-compute random valid-circuit search baseline.
pub struct RandomSearchBaseline {
    width: Width,
    channels: u16,
    wire_delay: WireDelayMode,
    max_nodes: usize,
    samples_per_error: usize,
    rng: ChaCha8Rng,
    window: ObservationWindow,
    last_observed: Vec<u64>,
    best_circuit: Circuit,
    best_compiled: CompiledCircuit,
    live_state: ScalarState,
}

impl RandomSearchBaseline {
    /// Creates a new [`RandomSearchBaseline`] seeded from `seeds`.
    #[must_use]
    pub fn new(
        width: Width,
        channels: u16,
        wire_delay: WireDelayMode,
        max_nodes: usize,
        fact_window: usize,
        samples_per_error: usize,
        seeds: SeedBank,
    ) -> Self {
        let rng = seeds.stream(StreamDomain::BaselineSearch { trial: 0 });
        let best_circuit = Circuit::new(width, channels, channels, wire_delay);
        let best_compiled = best_circuit.compile().expect("empty circuit is valid");
        let live_state = ScalarState::new(&best_compiled);
        Self {
            width,
            channels,
            wire_delay,
            max_nodes,
            samples_per_error: samples_per_error.max(1),
            rng,
            window: ObservationWindow::new(channels, fact_window.max(24)),
            last_observed: vec![0u64; usize::from(channels)],
            best_circuit,
            best_compiled,
            live_state,
        }
    }
}

impl PredictorBaseline for RandomSearchBaseline {
    fn name(&self) -> &str {
        "RandomSearch"
    }

    fn predict(&mut self, _budget: &mut TickBudget) -> Vec<u64> {
        let mut pred = vec![0u64; usize::from(self.channels)];
        self.live_state
            .step(&self.best_compiled, &self.last_observed, &mut pred)
            .expect("valid port lengths");
        pred
    }

    fn observe(&mut self, truth: &[u64], budget: &mut TickBudget) -> Option<f64> {
        self.last_observed.copy_from_slice(truth);
        self.window.push_frame(truth);

        if self.window.len() < 2 {
            return None;
        }

        let (inc_mismatches, inc_total, inc_state) =
            evaluate_circuit_on_window(&self.best_compiled, &self.window, budget)?;
        let mut best_mdl = compute_mdl(
            &self.best_circuit,
            WiringTopology::WellMixed,
            None,
            0.0,
            0,
            inc_total,
            inc_mismatches,
        )
        .total_bits;
        self.live_state = inc_state;

        // Only burn search budget when incumbent still makes errors on the window (or occasionally to compress).
        if inc_mismatches == 0 && !self.window.len().is_multiple_of(32) {
            return None;
        }

        for _ in 0..self.samples_per_error {
            let cand = sample_random_valid_circuit(
                &mut self.rng,
                self.width,
                self.channels,
                self.channels,
                self.wire_delay,
                self.max_nodes,
            );
            let compiled = cand
                .compile()
                .expect("constructive sampler satisfies §2.5 invariants");
            let Some((mismatches, total, end_state)) =
                evaluate_circuit_on_window(&compiled, &self.window, budget)
            else {
                break;
            };
            let mdl = compute_mdl(
                &cand,
                WiringTopology::WellMixed,
                None,
                0.0,
                0,
                total,
                mismatches,
            )
            .total_bits;
            if mdl < best_mdl {
                best_mdl = mdl;
                self.best_circuit = cand;
                self.best_compiled = compiled;
                self.live_state = end_state;
                if mismatches == 0 {
                    break;
                }
            }
        }

        None
    }

    fn best_circuit(&self) -> Option<&Circuit> {
        Some(&self.best_circuit)
    }
}
