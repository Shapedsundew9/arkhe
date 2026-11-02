//! $(1 + \lambda)$ Cartesian Genetic Programming baseline with `SPLIT`-explicit lowering (Charter §8).
//!
//! Evolves both input-driven circuits and noise-resistant autonomous `DELAY` oscillators while
//! guaranteeing combinational acyclicity by construction, applying Miller's Single-Active Mutation
//! (SAM), preserving periodic phase alignment across history trims, and inserting explicit
//! `Primitive::Split` trees for all multi-consumer active wires before compilation and MDL scoring.

use std::collections::BTreeMap;

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

#[derive(Clone, Copy, Debug)]
struct Gene {
    op: Primitive,
    /// Source index in `0..(num_inputs + max_genes)`.
    in0: u16,
    /// Second source index (used only when `op.input_arity() == 2`).
    in1: u16,
}

#[derive(Clone, Debug)]
struct Genotype {
    genes: Vec<Gene>,
    outputs: Vec<u16>,
}

impl Genotype {
    fn initial_passthrough(num_inputs: u16, num_outputs: u16, max_genes: usize) -> Self {
        let mut genes = Vec::with_capacity(max_genes);
        for i in 0..max_genes {
            let src = if num_inputs > 0 && i == 0 {
                0
            } else if i > 0 {
                num_inputs + (i as u16) - 1
            } else {
                0
            };
            genes.push(Gene {
                op: Primitive::delay_zero(),
                in0: src,
                in1: src,
            });
        }
        let outputs = (0..num_outputs)
            .map(|p| if num_inputs > 0 { p % num_inputs } else { 0 })
            .collect();
        Self { genes, outputs }
    }

    fn active_gene_flags(&self, num_inputs: u16) -> Vec<bool> {
        let mut active = vec![false; self.genes.len()];
        let mut stack: Vec<usize> = Vec::new();
        for &out_src in &self.outputs {
            if out_src >= num_inputs {
                let g = usize::from(out_src - num_inputs);
                if !active[g] {
                    active[g] = true;
                    stack.push(g);
                }
            }
        }
        while let Some(g) = stack.pop() {
            let gene = self.genes[g];
            for port in 0..gene.op.input_arity() {
                let src = if port == 0 { gene.in0 } else { gene.in1 };
                if src >= num_inputs {
                    let sg = usize::from(src - num_inputs);
                    if !active[sg] {
                        active[sg] = true;
                        stack.push(sg);
                    }
                }
            }
        }
        active
    }

    /// Valid source indices for gene `g_idx` given `op`:
    /// - Stateful `Delay` genes can read from ANY boundary input or ANY gene (enabling feedback loops!).
    /// - Combinational genes can read from boundary inputs, earlier genes `< g_idx`, or ANY `Delay` gene.
    fn allowed_sources(&self, g_idx: usize, op: Primitive, num_inputs: u16) -> Vec<u16> {
        let total = usize::from(num_inputs) + self.genes.len();
        if op.is_stateful() {
            return (0..(total as u16)).collect();
        }
        let mut allowed: Vec<u16> = (0..num_inputs).collect();
        for (j, gene) in self.genes.iter().enumerate() {
            if j < g_idx || gene.op.is_stateful() {
                allowed.push(num_inputs + (j as u16));
            }
        }
        if allowed.is_empty() {
            allowed.push(0);
        }
        allowed
    }

    /// Repairs any combinational gene input that points to a non-`Delay` gene `>= g_idx`.
    fn repair(&mut self, rng: &mut ChaCha8Rng, num_inputs: u16) {
        for g_idx in 0..self.genes.len() {
            let op = self.genes[g_idx].op;
            if op.is_stateful() {
                continue;
            }
            let allowed = self.allowed_sources(g_idx, op, num_inputs);
            if !allowed.contains(&self.genes[g_idx].in0) {
                let pick = uniform_u64_below(rng, allowed.len() as u64) as usize;
                self.genes[g_idx].in0 = allowed[pick];
            }
            if !allowed.contains(&self.genes[g_idx].in1) {
                let pick = uniform_u64_below(rng, allowed.len() as u64) as usize;
                self.genes[g_idx].in1 = allowed[pick];
            }
        }
    }

    /// Mutates a single inactive gene silently (neutral drift at zero evaluation cost).
    fn drift_inactive(&mut self, rng: &mut ChaCha8Rng, width: Width, num_inputs: u16) {
        let active = self.active_gene_flags(num_inputs);
        let inactive_indices: Vec<usize> = active
            .iter()
            .enumerate()
            .filter_map(|(idx, &is_act)| (!is_act).then_some(idx))
            .collect();
        if inactive_indices.is_empty() {
            return;
        }
        let g_idx =
            inactive_indices[uniform_u64_below(rng, inactive_indices.len() as u64) as usize];
        let op = sample_primitive(rng, width);
        let allowed = self.allowed_sources(g_idx, op, num_inputs);
        let in0 = allowed[uniform_u64_below(rng, allowed.len() as u64) as usize];
        let in1 = allowed[uniform_u64_below(rng, allowed.len() as u64) as usize];
        self.genes[g_idx] = Gene { op, in0, in1 };
    }

    /// Recruits an inactive gene into the active circuit by sampling its `(op, in0, in1)` from
    /// live sources (boundary inputs, active genes, and self-loop if `Delay`) and wiring one
    /// valid active consumer to read from it.
    fn mutate_recruit_gene(&self, rng: &mut ChaCha8Rng, width: Width, num_inputs: u16) -> Self {
        let active = self.active_gene_flags(num_inputs);
        let inactive_indices: Vec<usize> = active
            .iter()
            .enumerate()
            .filter_map(|(idx, &is_act)| (!is_act).then_some(idx))
            .collect();
        if inactive_indices.is_empty() {
            return self.mutate(rng, width, num_inputs, 2, true);
        }

        let mut child = self.clone();
        let g_new =
            inactive_indices[uniform_u64_below(rng, inactive_indices.len() as u64) as usize];
        let src_new = num_inputs + (g_new as u16);
        let new_op = sample_primitive(rng, width);

        let allowed = child.allowed_sources(g_new, new_op, num_inputs);
        let live_allowed: Vec<u16> = allowed
            .iter()
            .copied()
            .filter(|&s| {
                s < num_inputs
                    || (s == src_new && new_op.is_stateful())
                    || active[usize::from(s - num_inputs)]
            })
            .collect();
        let pool = if live_allowed.is_empty() {
            &allowed
        } else {
            &live_allowed
        };
        let in0 = pool[uniform_u64_below(rng, pool.len() as u64) as usize];
        let in1 = pool[uniform_u64_below(rng, pool.len() as u64) as usize];
        child.genes[g_new] = Gene {
            op: new_op,
            in0,
            in1,
        };

        // Collect every active sink that can read `src_new` without forming a combinational cycle.
        #[derive(Clone, Copy)]
        enum TargetSink {
            GenePort { idx: usize, port: u8 },
            Output { idx: usize },
        }
        let mut valid_sinks = Vec::new();
        for out_idx in 0..child.outputs.len() {
            valid_sinks.push(TargetSink::Output { idx: out_idx });
        }
        for (g_act, &is_act) in active.iter().enumerate() {
            if !is_act {
                continue;
            }
            let act_op = child.genes[g_act].op;
            if act_op.is_stateful() || new_op.is_stateful() || g_new < g_act {
                valid_sinks.push(TargetSink::GenePort {
                    idx: g_act,
                    port: 0,
                });
                if act_op.input_arity() == 2 {
                    valid_sinks.push(TargetSink::GenePort {
                        idx: g_act,
                        port: 1,
                    });
                }
            }
        }

        if !valid_sinks.is_empty() {
            let pick = uniform_u64_below(rng, valid_sinks.len() as u64) as usize;
            match valid_sinks[pick] {
                TargetSink::Output { idx } => child.outputs[idx] = src_new,
                TargetSink::GenePort { idx, port: 0 } => child.genes[idx].in0 = src_new,
                TargetSink::GenePort { idx, port: _ } => child.genes[idx].in1 = src_new,
            }
        }

        child.repair(rng, num_inputs);
        child
    }

    /// Applies Miller's Single-Active Mutation (`active_target` active hits), or `2..=4`
    /// unrestricted point mutations when `burst` is true to cross multi-edge feedback valleys.
    fn mutate(
        &self,
        rng: &mut ChaCha8Rng,
        width: Width,
        num_inputs: u16,
        active_target: usize,
        burst: bool,
    ) -> Self {
        let mut child = self.clone();
        let num_genes = child.genes.len();
        let total_loci = num_genes + child.outputs.len();
        if total_loci == 0 {
            return child;
        }

        let active = self.active_gene_flags(num_inputs);
        let target_hits = if burst {
            2 + (uniform_u64_below(rng, 3) as usize)
        } else {
            active_target.max(1)
        };
        let mut hits = 0usize;
        let mut steps = 0usize;

        while hits < target_hits && steps < 32 {
            steps += 1;
            let locus = uniform_u64_below(rng, total_loci as u64) as usize;
            let counts_toward_target = burst || locus >= num_genes || active[locus];

            if locus < num_genes {
                let g_idx = locus;
                match uniform_u64_below(rng, 3) {
                    0 => {
                        child.genes[g_idx].op = sample_primitive(rng, width);
                    }
                    1 => {
                        let op = child.genes[g_idx].op;
                        let allowed = child.allowed_sources(g_idx, op, num_inputs);
                        let pick = uniform_u64_below(rng, allowed.len() as u64) as usize;
                        child.genes[g_idx].in0 = allowed[pick];
                    }
                    _ => {
                        let op = child.genes[g_idx].op;
                        if let Primitive::Delay { .. } = op {
                            let init = if (rng.next_u64() & 3) == 0 {
                                0
                            } else {
                                rng.next_u64() & width.mask()
                            };
                            child.genes[g_idx].op = Primitive::Delay { init };
                        } else {
                            let allowed = child.allowed_sources(g_idx, op, num_inputs);
                            let pick = uniform_u64_below(rng, allowed.len() as u64) as usize;
                            child.genes[g_idx].in1 = allowed[pick];
                        }
                    }
                }
            } else {
                let out_idx = locus - num_genes;
                let total_sources = usize::from(num_inputs) + num_genes;
                if total_sources > 0 {
                    child.outputs[out_idx] = uniform_u64_below(rng, total_sources as u64) as u16;
                }
            }

            if counts_toward_target {
                hits += 1;
            }
        }

        child.repair(rng, num_inputs);
        child
    }

    /// Decodes active genes reachable from `outputs` and expands multi-consumer fan-out into
    /// explicit binary trees of `Primitive::Split` nodes so Charter §2.5.5 holds identically.
    fn lower_to_circuit(
        &self,
        width: Width,
        num_inputs: u16,
        wire_delay: WireDelayMode,
    ) -> Circuit {
        let num_outputs = u16::try_from(self.outputs.len()).expect("port count fits in u16");
        let active_genes = self.active_gene_flags(num_inputs);

        let mut circuit = Circuit::new(width, num_inputs, num_outputs, wire_delay);
        let mut gene_to_node: BTreeMap<usize, NodeId> = BTreeMap::new();
        for (g, &is_active) in active_genes.iter().enumerate() {
            if is_active {
                let id = circuit.add_node(self.genes[g].op);
                gene_to_node.insert(g, id);
            }
        }

        let mut consumers_by_src: BTreeMap<u16, Vec<SignalSink>> = BTreeMap::new();
        for (&g, &node) in &gene_to_node {
            let gene = self.genes[g];
            consumers_by_src
                .entry(gene.in0)
                .or_default()
                .push(SignalSink::NodeIn { node, port: 0 });
            if gene.op.input_arity() == 2 {
                consumers_by_src
                    .entry(gene.in1)
                    .or_default()
                    .push(SignalSink::NodeIn { node, port: 1 });
            }
        }
        for (p, &out_src) in self.outputs.iter().enumerate() {
            consumers_by_src
                .entry(out_src)
                .or_default()
                .push(SignalSink::BoundaryOut(p as u16));
        }

        for (raw_src, sinks) in consumers_by_src {
            let root = if raw_src < num_inputs {
                SignalSource::BoundaryIn(raw_src)
            } else {
                let g = usize::from(raw_src - num_inputs);
                SignalSource::NodeOut {
                    node: gene_to_node[&g],
                    port: 0,
                }
            };
            route_with_splits(&mut circuit, root, &sinks);
        }

        circuit
    }
}

fn sample_primitive(rng: &mut ChaCha8Rng, width: Width) -> Primitive {
    match uniform_u64_below(rng, 7) {
        0 => Primitive::Add,
        1 => Primitive::Sub,
        2 => Primitive::Mul,
        3 => Primitive::Eqz,
        4 => Primitive::delay_zero(),
        _ => Primitive::Delay {
            init: rng.next_u64() & width.mask(),
        },
    }
}

fn route_with_splits(circuit: &mut Circuit, src: SignalSource, sinks: &[SignalSink]) {
    match sinks {
        [] => {}
        [single] => {
            circuit.connect(src, *single);
        }
        many => {
            let mid = many.len() / 2;
            let split_id = circuit.add_node(Primitive::Split);
            circuit.connect(
                src,
                SignalSink::NodeIn {
                    node: split_id,
                    port: 0,
                },
            );
            route_with_splits(
                circuit,
                SignalSource::NodeOut {
                    node: split_id,
                    port: 0,
                },
                &many[..mid],
            );
            route_with_splits(
                circuit,
                SignalSource::NodeOut {
                    node: split_id,
                    port: 1,
                },
                &many[mid..],
            );
        }
    }
}

/// Equal-compute $(1 + \lambda)$ Cartesian Genetic Programming baseline.
pub struct CgpBaseline {
    width: Width,
    channels: u16,
    wire_delay: WireDelayMode,
    lambda: usize,
    rng: ChaCha8Rng,
    window: ObservationWindow,
    ticks_seen: u64,
    last_observed: Vec<u64>,
    parent_geno: Genotype,
    best_circuit: Circuit,
    best_compiled: CompiledCircuit,
    live_state: ScalarState,
}

impl CgpBaseline {
    /// Creates a new [`CgpBaseline`] with `max_genes` internal CGP loci and offspring count `lambda`.
    #[must_use]
    pub fn new(
        width: Width,
        channels: u16,
        wire_delay: WireDelayMode,
        lambda: usize,
        max_genes: usize,
        fact_window: usize,
        seeds: SeedBank,
    ) -> Self {
        let rng = seeds.stream(StreamDomain::BaselineSearch { trial: 1 });
        let parent_geno = Genotype::initial_passthrough(channels, channels, max_genes.max(4));
        let best_circuit = parent_geno.lower_to_circuit(width, channels, wire_delay);
        let best_compiled = best_circuit
            .compile()
            .expect("CGP lowered circuit satisfies §2.5 invariants");
        let live_state = ScalarState::new(&best_compiled);

        Self {
            width,
            channels,
            wire_delay,
            lambda: lambda.max(1),
            rng,
            window: ObservationWindow::new(channels, fact_window.max(48)),
            ticks_seen: 0,
            last_observed: vec![0u64; usize::from(channels)],
            parent_geno,
            best_circuit,
            best_compiled,
            live_state,
        }
    }
}

impl PredictorBaseline for CgpBaseline {
    fn name(&self) -> &str {
        "CGP(1+lambda)"
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
        self.ticks_seen += 1;

        if self.window.len() < 4 {
            return None;
        }

        // Budget-proportional pacing: search intensively during early warmup (up to 60 ticks and
        // at most 50% of budget) so noiseless tasks lock on rapidly, then throttle to every 6 ticks
        // so long noisy runs preserve budget past the MDL sample-size crossover.
        let total_budget = budget.consumed() + budget.remaining();
        let early_fast = self.ticks_seen <= 60 && budget.remaining() * 2 >= total_budget;
        let should_search = early_fast || self.ticks_seen.is_multiple_of(6);
        if !should_search {
            return None;
        }

        let (mut parent_err, parent_total, parent_end_state) =
            evaluate_circuit_on_window(&self.best_compiled, &self.window, budget)?;
        let mut parent_mdl = compute_mdl(
            &self.best_circuit,
            WiringTopology::WellMixed,
            None,
            0.0,
            0,
            parent_total,
            parent_err,
        )
        .total_bits;
        self.live_state = parent_end_state;

        let gens = if parent_err == 0 {
            1
        } else if early_fast {
            8
        } else {
            4
        };

        for _ in 0..gens {
            self.parent_geno
                .drift_inactive(&mut self.rng, self.width, self.channels);
            let mut gen_improved = false;
            for offspring_idx in 0..self.lambda {
                let child_geno = if offspring_idx % 3 == 2 {
                    self.parent_geno
                        .mutate_recruit_gene(&mut self.rng, self.width, self.channels)
                } else {
                    let burst = offspring_idx + 1 == self.lambda && self.lambda >= 2;
                    let active_target = if offspring_idx == 1 { 2 } else { 1 };
                    self.parent_geno.mutate(
                        &mut self.rng,
                        self.width,
                        self.channels,
                        active_target,
                        burst,
                    )
                };
                let child_circ =
                    child_geno.lower_to_circuit(self.width, self.channels, self.wire_delay);
                // If phenotype is unchanged, accept neutral genotype drift without re-running identical ticks.
                if child_circ == self.best_circuit {
                    self.parent_geno = child_geno;
                    continue;
                }
                let child_comp = child_circ
                    .compile()
                    .expect("CGP lowered circuit satisfies §2.5 invariants");
                let (c_err, c_total, c_state) =
                    evaluate_circuit_on_window(&child_comp, &self.window, budget)?;
                let c_mdl = compute_mdl(
                    &child_circ,
                    WiringTopology::WellMixed,
                    None,
                    0.0,
                    0,
                    c_total,
                    c_err,
                )
                .total_bits;

                if c_mdl <= parent_mdl + 1e-12 {
                    if c_mdl < parent_mdl - 1e-12 {
                        gen_improved = true;
                    }
                    parent_mdl = c_mdl;
                    parent_err = c_err;
                    self.parent_geno = child_geno;
                    self.best_circuit = child_circ;
                    self.best_compiled = child_comp;
                    self.live_state = c_state;
                }
            }
            if parent_err == 0 && gen_improved {
                break;
            }
        }

        None
    }

    fn best_circuit(&self) -> Option<&Circuit> {
        Some(&self.best_circuit)
    }
}
