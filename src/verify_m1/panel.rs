//! Panel task and circuit definitions for pre-growth M1a gates.

use serde::{Deserialize, Serialize};

use crate::config::CliffCountersSummary;
use crate::env::{FiniteStateGenerator, GeneratorConfig, NoiseModel, NoisyEnvironment};
use crate::metrics::{NullModelKind, circuit_code_bits, null_calibrated_mdl_delta};
use crate::rng::SeedBank;
use crate::structural::economy::{EconomyConfig, ReserveCap};
use crate::structural::live::{LiveCircuit, well_mixed_description_debts};
use crate::substrate::{
    Circuit, NodeId, Primitive, SignalSink, SignalSource, Width, WireDelayMode,
};

/// Ground-truth taxonomic class of a panel circuit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CircuitClass {
    /// Minimal exact predictor.
    Opt,
    /// Informative partial predictor (report-only in separation checks).
    Partial,
    /// Blind or chance guessing circuit.
    Chance,
    /// Anti-correlated predictor.
    Wrong,
    /// Unchanging stream predictor on constant signals.
    Silence,
    /// Functionally correct circuit with dead or redundant detour nodes.
    Detour,
}

/// Tolerance for empirical accuracy self-checks at ε = 0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AccuracyCheck {
    /// Strict floating-point match within 1e-12.
    Exact(f64),
    /// Bounded accuracy interval [min, max].
    Range(f64, f64),
    /// No strict accuracy requirement (report only).
    None,
}

/// Benchmark task specification on the panel.
#[derive(Clone, Debug)]
pub struct PanelTask {
    /// Task identifier (e.g. "alt", "motif3").
    pub id: &'static str,
    /// Substrate bit width.
    pub width: Width,
    /// Stream generator configuration.
    pub generator: GeneratorConfig,
    /// Whether the task is a degenerate constant stream.
    pub constant: bool,
}

/// Hand-built circuit instance evaluated against a panel task.
#[derive(Clone, Debug)]
pub struct PanelCircuit {
    /// Circuit identifier (e.g. "osc2", "addzero").
    pub id: &'static str,
    /// Diagnostic classification.
    pub class: CircuitClass,
    /// Abstract circuit wiring.
    pub circuit: Circuit,
    /// Detour nodes tracked for free-rider credit checks.
    pub detour_nodes: Vec<NodeId>,
    /// Identifier of the reference `C_opt` circuit for detour comparisons.
    pub reference: Option<&'static str>,
    /// Expected accuracy on uncorrupted streams.
    pub expected_accuracy: AccuracyCheck,
}

/// Pre-registered benchmark evaluation horizon in ticks.
pub const DEFAULT_HORIZON: u64 = 1024;

/// Outcome of one live circuit evaluation across the benchmark horizon.
#[derive(Clone, Debug, PartialEq)]
pub struct PanelRun {
    /// Benchmark task identifier.
    pub task: &'static str,
    /// Bit width.
    pub k: u8,
    /// Circuit identifier.
    pub circuit: &'static str,
    /// Diagnostic classification.
    pub class: CircuitClass,
    /// Substitution noise rate.
    pub epsilon: f64,
    /// Random seed.
    pub seed: u64,
    /// Total nodes in the circuit.
    pub node_count: usize,
    /// Empirical accuracy over post-transient ticks 16..horizon against observed facts.
    pub accuracy: f64,
    /// Empirical accuracy over post-transient ticks 16..horizon against clean ground truth.
    pub clean_accuracy: f64,
    /// Net energy delta Σ_i (E_i(T) - E_init) over all ticks.
    pub delta_e: f64,
    /// Minimum node energy min_i E_i(T) at horizon end.
    pub min_node_energy: f64,
    /// Null-calibrated description length change ΔL_null over all ticks.
    pub delta_l_null: f64,
    /// Cumulative credit paid to detour nodes after tick 0.
    pub detour_credit_after_tick0: f64,
    /// Maximum energy among detour nodes at horizon end, if any.
    pub max_detour_energy: Option<f64>,
    /// Maximum conservation discrepancy |Σ node_delta - (flux - maint)| across all ticks.
    pub max_conservation_error: f64,
    /// Cumulative prediction reward flux Σ_t flux(t) across all ticks.
    pub flux: f64,
    /// Circuit description length L(C).
    pub code_bits: f64,
    /// Minimum node credit margin min_i (E_i(T) - E_init) at horizon end.
    pub min_node_margin: f64,
    /// Maximum node credit margin max_i (E_i(T) - E_init) at horizon end.
    pub max_node_margin: f64,
    /// Cliff, bound, and guardrail telemetry counters observed on this panel run (§B3).
    pub cliff_counters: CliffCountersSummary,
}

/// Helper building circuits from canonical Level-0 motifs.
pub struct Builder {
    circuit: Circuit,
}

impl Builder {
    /// Creates an empty single-input, single-output circuit builder.
    #[must_use]
    pub fn new(width: Width) -> Self {
        Self {
            circuit: Circuit::new(width, 1, 1, WireDelayMode::Combinational),
        }
    }

    /// Boundary input port 0 (observed previous symbol s(t-1)).
    #[must_use]
    pub const fn x(&self) -> SignalSource {
        SignalSource::BoundaryIn(0)
    }

    /// Returns the current number of placed nodes.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.circuit.node_count()
    }

    /// Collects node IDs placed since `mark`.
    #[must_use]
    pub fn nodes_since(&self, mark: usize) -> Vec<NodeId> {
        (mark..self.circuit.node_count())
            .map(|i| NodeId(u32::try_from(i).expect("node index fits in u32")))
            .collect()
    }

    /// ConstLoop(c): D = Delay(c), S = Split; D.out -> S.in, S.out0 -> D.in, S.out1 -> consumer
    pub fn const_loop(&mut self, c: u64) -> SignalSource {
        let d = self.circuit.add_node(Primitive::Delay { init: c });
        let s = self.circuit.add_node(Primitive::Split);
        self.circuit.connect(
            SignalSource::NodeOut { node: d, port: 0 },
            SignalSink::NodeIn { node: s, port: 0 },
        );
        self.circuit.connect(
            SignalSource::NodeOut { node: s, port: 0 },
            SignalSink::NodeIn { node: d, port: 0 },
        );
        SignalSource::NodeOut { node: s, port: 1 }
    }

    /// Osc2(a, b): D1 = Delay(b), D2 = Delay(a), S = Split; D1 -> D2 -> S.in, S.out0 -> D1.in, S.out1 -> consumer
    pub fn osc2(&mut self, a: u64, b: u64) -> SignalSource {
        let d1 = self.circuit.add_node(Primitive::Delay { init: b });
        let d2 = self.circuit.add_node(Primitive::Delay { init: a });
        let s = self.circuit.add_node(Primitive::Split);
        self.circuit.connect(
            SignalSource::NodeOut { node: d1, port: 0 },
            SignalSink::NodeIn { node: d2, port: 0 },
        );
        self.circuit.connect(
            SignalSource::NodeOut { node: d2, port: 0 },
            SignalSink::NodeIn { node: s, port: 0 },
        );
        self.circuit.connect(
            SignalSource::NodeOut { node: s, port: 0 },
            SignalSink::NodeIn { node: d1, port: 0 },
        );
        SignalSource::NodeOut { node: s, port: 1 }
    }

    /// Osc3(a, b, a): D1 = Delay(a), D2 = Delay(b), D3 = Delay(a), S; D1 -> D2 -> D3 -> S, S.out0 -> D1, S.out1 -> consumer
    pub fn osc3(&mut self, a: u64, b: u64, c: u64) -> SignalSource {
        let d1 = self.circuit.add_node(Primitive::Delay { init: a });
        let d2 = self.circuit.add_node(Primitive::Delay { init: b });
        let d3 = self.circuit.add_node(Primitive::Delay { init: c });
        let s = self.circuit.add_node(Primitive::Split);
        self.circuit.connect(
            SignalSource::NodeOut { node: d1, port: 0 },
            SignalSink::NodeIn { node: d2, port: 0 },
        );
        self.circuit.connect(
            SignalSource::NodeOut { node: d2, port: 0 },
            SignalSink::NodeIn { node: d3, port: 0 },
        );
        self.circuit.connect(
            SignalSource::NodeOut { node: d3, port: 0 },
            SignalSink::NodeIn { node: s, port: 0 },
        );
        self.circuit.connect(
            SignalSource::NodeOut { node: s, port: 0 },
            SignalSink::NodeIn { node: d1, port: 0 },
        );
        SignalSource::NodeOut { node: s, port: 1 }
    }

    /// Loop4(a, b): D1..D4 with inits (b, a, b, a), chain D1 -> D2 -> D3 -> D4 -> S, S.out0 -> D1, S.out1 -> consumer
    pub fn loop4(&mut self, a: u64, b: u64) -> SignalSource {
        let d1 = self.circuit.add_node(Primitive::Delay { init: b });
        let d2 = self.circuit.add_node(Primitive::Delay { init: a });
        let d3 = self.circuit.add_node(Primitive::Delay { init: b });
        let d4 = self.circuit.add_node(Primitive::Delay { init: a });
        let s = self.circuit.add_node(Primitive::Split);
        self.circuit.connect(
            SignalSource::NodeOut { node: d1, port: 0 },
            SignalSink::NodeIn { node: d2, port: 0 },
        );
        self.circuit.connect(
            SignalSource::NodeOut { node: d2, port: 0 },
            SignalSink::NodeIn { node: d3, port: 0 },
        );
        self.circuit.connect(
            SignalSource::NodeOut { node: d3, port: 0 },
            SignalSink::NodeIn { node: d4, port: 0 },
        );
        self.circuit.connect(
            SignalSource::NodeOut { node: d4, port: 0 },
            SignalSink::NodeIn { node: s, port: 0 },
        );
        self.circuit.connect(
            SignalSource::NodeOut { node: s, port: 0 },
            SignalSink::NodeIn { node: d1, port: 0 },
        );
        SignalSource::NodeOut { node: s, port: 1 }
    }

    /// Shift(n): src -> D(0) -> ... -> D(0) (n delays)
    pub fn shift(&mut self, n: usize, src: SignalSource) -> SignalSource {
        let mut prev = src;
        for _ in 0..n {
            let d = self.circuit.add_node(Primitive::Delay { init: 0 });
            self.circuit
                .connect(prev, SignalSink::NodeIn { node: d, port: 0 });
            prev = SignalSource::NodeOut { node: d, port: 0 };
        }
        prev
    }

    /// Eqz(src)
    pub fn eqz(&mut self, src: SignalSource) -> SignalSource {
        let e = self.circuit.add_node(Primitive::Eqz);
        self.circuit
            .connect(src, SignalSink::NodeIn { node: e, port: 0 });
        SignalSource::NodeOut { node: e, port: 0 }
    }

    /// Add(a, b)
    pub fn add(&mut self, a: SignalSource, b: SignalSource) -> SignalSource {
        let node = self.circuit.add_node(Primitive::Add);
        self.circuit
            .connect(a, SignalSink::NodeIn { node, port: 0 });
        self.circuit
            .connect(b, SignalSink::NodeIn { node, port: 1 });
        SignalSource::NodeOut { node, port: 0 }
    }

    /// Sub(a, b)
    pub fn sub(&mut self, a: SignalSource, b: SignalSource) -> SignalSource {
        let node = self.circuit.add_node(Primitive::Sub);
        self.circuit
            .connect(a, SignalSink::NodeIn { node, port: 0 });
        self.circuit
            .connect(b, SignalSink::NodeIn { node, port: 1 });
        SignalSource::NodeOut { node, port: 0 }
    }

    /// Mul(a, b)
    pub fn mul(&mut self, a: SignalSource, b: SignalSource) -> SignalSource {
        let node = self.circuit.add_node(Primitive::Mul);
        self.circuit
            .connect(a, SignalSink::NodeIn { node, port: 0 });
        self.circuit
            .connect(b, SignalSink::NodeIn { node, port: 1 });
        SignalSource::NodeOut { node, port: 0 }
    }

    /// Split(src) -> (out0, out1)
    pub fn split(&mut self, src: SignalSource) -> (SignalSource, SignalSource) {
        let node = self.circuit.add_node(Primitive::Split);
        self.circuit
            .connect(src, SignalSink::NodeIn { node, port: 0 });
        (
            SignalSource::NodeOut { node, port: 0 },
            SignalSource::NodeOut { node, port: 1 },
        )
    }

    /// AddConst1: Add(x, ConstLoop(1))
    pub fn add_const_1(&mut self) -> SignalSource {
        let c = self.const_loop(1);
        let x = self.x();
        self.add(x, c)
    }

    /// Counter3Opt wiring (Section 1.3):
    /// Dc -> Sc.in; Sc.out0 -> Dc.in; Sc.out1 -> Sc2.in; x -> A1.0; Sc2.out0 -> A1.1;
    /// A1 -> Sy.in; Sy.out0 -> AF.0; Sy.out1 -> A2.0; Sc2.out1 -> A2.1; A2 -> E; E -> AF.1; AF -> Out.
    pub fn counter3_opt(&mut self) -> SignalSource {
        let dc = self.circuit.add_node(Primitive::Delay { init: 1 });
        let (sc_out0, sc_out1) = self.split(SignalSource::NodeOut { node: dc, port: 0 });
        self.circuit
            .connect(sc_out0, SignalSink::NodeIn { node: dc, port: 0 });
        let (sc2_out0, sc2_out1) = self.split(sc_out1);
        let x = self.x();
        let a1 = self.add(x, sc2_out0);
        let (sy_out0, sy_out1) = self.split(a1);
        let a2 = self.add(sy_out1, sc2_out1);
        let e = self.eqz(a2);
        self.add(sy_out0, e)
    }

    /// Connects `out_src` to BoundaryOut(0) and returns the completed circuit.
    #[must_use]
    pub fn finish(mut self, out_src: SignalSource) -> Circuit {
        self.circuit.connect(out_src, SignalSink::BoundaryOut(0));
        self.circuit
    }
}

/// Returns the pre-registered list of M1a panel tasks across k ∈ {1, 2}.
#[must_use]
pub fn panel_tasks() -> Vec<PanelTask> {
    vec![
        // alt: k=1, k=2
        PanelTask {
            id: "alt",
            width: Width::BITS_1,
            generator: GeneratorConfig::T1_Alternating { a: 0, b: 1 },
            constant: false,
        },
        PanelTask {
            id: "alt",
            width: Width::BITS_2,
            generator: GeneratorConfig::T1_Alternating { a: 0, b: 3 },
            constant: false,
        },
        // motif3: k=1, k=2
        PanelTask {
            id: "motif3",
            width: Width::BITS_1,
            generator: GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 1, 1],
            },
            constant: false,
        },
        PanelTask {
            id: "motif3",
            width: Width::BITS_2,
            generator: GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 1, 3],
            },
            constant: false,
        },
        // sparse6: k=1, k=2
        PanelTask {
            id: "sparse6",
            width: Width::BITS_1,
            generator: GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 0, 0, 0, 0, 1],
            },
            constant: false,
        },
        PanelTask {
            id: "sparse6",
            width: Width::BITS_2,
            generator: GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 0, 0, 0, 0, 1],
            },
            constant: false,
        },
        // affine: k=1, k=2
        PanelTask {
            id: "affine",
            width: Width::BITS_1,
            generator: GeneratorConfig::T1_AffineLag1 {
                mul_a: 1,
                add_b: 1,
                init: 0,
            },
            constant: false,
        },
        PanelTask {
            id: "affine",
            width: Width::BITS_2,
            generator: GeneratorConfig::T1_AffineLag1 {
                mul_a: 1,
                add_b: 1,
                init: 0,
            },
            constant: false,
        },
        // counter3: k=2 only
        PanelTask {
            id: "counter3",
            width: Width::BITS_2,
            generator: GeneratorConfig::T3b_CounterModN {
                modulus: 3,
                init: 0,
            },
            constant: false,
        },
        // const0: k=1, k=2
        PanelTask {
            id: "const0",
            width: Width::BITS_1,
            generator: GeneratorConfig::T1_Constant { value: 0 },
            constant: true,
        },
        PanelTask {
            id: "const0",
            width: Width::BITS_2,
            generator: GeneratorConfig::T1_Constant { value: 0 },
            constant: true,
        },
        // consthi: k=1, k=2
        PanelTask {
            id: "consthi",
            width: Width::BITS_1,
            generator: GeneratorConfig::T1_Constant { value: 1 },
            constant: true,
        },
        PanelTask {
            id: "consthi",
            width: Width::BITS_2,
            generator: GeneratorConfig::T1_Constant { value: 3 },
            constant: true,
        },
    ]
}

fn build_base_source(
    task_id: &str,
    builder: &mut Builder,
    hi: u64,
) -> (&'static str, SignalSource) {
    match task_id {
        "alt" => ("osc2", builder.osc2(0, hi)),
        "motif3" => {
            let x = builder.x();
            ("shift2", builder.shift(2, x))
        }
        "sparse6" => {
            let x = builder.x();
            ("shift5", builder.shift(5, x))
        }
        "affine" => ("addc1", builder.add_const_1()),
        "counter3" => ("counter3", builder.counter3_opt()),
        _ => panic!("unknown task id for Base: {task_id}"),
    }
}

/// Returns the panel circuits for `task` matching §1.4.
#[must_use]
pub fn panel_circuits(task: &PanelTask) -> Vec<PanelCircuit> {
    let k = task.width.bits();
    let hi = (1u64 << k) - 1;

    if task.constant {
        let value = match task.generator {
            GeneratorConfig::T1_Constant { value } => value,
            _ => 0,
        };
        let mut b = Builder::new(task.width);
        let src = b.const_loop(value);
        let circuit = b.finish(src);
        return vec![PanelCircuit {
            id: "const_match",
            class: CircuitClass::Silence,
            circuit,
            detour_nodes: Vec::new(),
            reference: None,
            expected_accuracy: AccuracyCheck::Exact(1.0),
        }];
    }

    let mut circuits = Vec::new();

    // 1. C_opt circuits
    match task.id {
        "alt" => {
            let mut b = Builder::new(task.width);
            let src = b.osc2(0, hi);
            circuits.push(PanelCircuit {
                id: "osc2",
                class: CircuitClass::Opt,
                circuit: b.finish(src),
                detour_nodes: Vec::new(),
                reference: None,
                expected_accuracy: AccuracyCheck::Exact(1.0),
            });
            if k == 1 {
                let mut b = Builder::new(task.width);
                let x = b.x();
                let src = b.eqz(x);
                circuits.push(PanelCircuit {
                    id: "eqz",
                    class: CircuitClass::Opt,
                    circuit: b.finish(src),
                    detour_nodes: Vec::new(),
                    reference: None,
                    expected_accuracy: AccuracyCheck::Exact(1.0),
                });
            }
        }
        "motif3" => {
            let mut b = Builder::new(task.width);
            let x = b.x();
            let src = b.shift(2, x);
            circuits.push(PanelCircuit {
                id: "shift2",
                class: CircuitClass::Opt,
                circuit: b.finish(src),
                detour_nodes: Vec::new(),
                reference: None,
                expected_accuracy: AccuracyCheck::Exact(1.0),
            });
        }
        "sparse6" => {
            let mut b = Builder::new(task.width);
            let x = b.x();
            let src = b.shift(5, x);
            circuits.push(PanelCircuit {
                id: "shift5",
                class: CircuitClass::Opt,
                circuit: b.finish(src),
                detour_nodes: Vec::new(),
                reference: None,
                expected_accuracy: AccuracyCheck::Exact(1.0),
            });
        }
        "affine" => {
            let mut b = Builder::new(task.width);
            let src = b.add_const_1();
            circuits.push(PanelCircuit {
                id: "addc1",
                class: CircuitClass::Opt,
                circuit: b.finish(src),
                detour_nodes: Vec::new(),
                reference: None,
                expected_accuracy: AccuracyCheck::Exact(1.0),
            });
        }
        "counter3" => {
            let mut b = Builder::new(task.width);
            let src = b.counter3_opt();
            circuits.push(PanelCircuit {
                id: "counter3",
                class: CircuitClass::Opt,
                circuit: b.finish(src),
                detour_nodes: Vec::new(),
                reference: None,
                expected_accuracy: AccuracyCheck::Exact(1.0),
            });
        }
        _ => {}
    }

    // 2. C_partial circuits
    if task.id == "motif3" && k == 1 {
        let mut b = Builder::new(task.width);
        let x = b.x();
        let d = b.shift(1, x);
        let src = b.eqz(d);
        circuits.push(PanelCircuit {
            id: "eqz_lag2",
            class: CircuitClass::Partial,
            circuit: b.finish(src),
            detour_nodes: Vec::new(),
            reference: None,
            expected_accuracy: AccuracyCheck::Range(0.66, 0.67),
        });
    }
    if task.id == "counter3" {
        let mut b = Builder::new(task.width);
        let src = b.add_const_1();
        circuits.push(PanelCircuit {
            id: "inc1",
            class: CircuitClass::Partial,
            circuit: b.finish(src),
            detour_nodes: Vec::new(),
            reference: None,
            expected_accuracy: AccuracyCheck::Range(0.66, 0.67),
        });
    }

    // 3. C_chance circuits (every non-constant task)
    {
        let mut b = Builder::new(task.width);
        let src = b.const_loop(0);
        circuits.push(PanelCircuit {
            id: "const0",
            class: CircuitClass::Chance,
            circuit: b.finish(src),
            detour_nodes: Vec::new(),
            reference: None,
            expected_accuracy: AccuracyCheck::None,
        });
    }
    {
        let mut b = Builder::new(task.width);
        let src = b.const_loop(1);
        circuits.push(PanelCircuit {
            id: "const1",
            class: CircuitClass::Chance,
            circuit: b.finish(src),
            detour_nodes: Vec::new(),
            reference: None,
            expected_accuracy: AccuracyCheck::None,
        });
    }
    if task.id == "alt" {
        let mut b = Builder::new(task.width);
        let src = b.osc3(0, hi, 0);
        circuits.push(PanelCircuit {
            id: "osc3",
            class: CircuitClass::Chance,
            circuit: b.finish(src),
            detour_nodes: Vec::new(),
            reference: None,
            expected_accuracy: AccuracyCheck::None,
        });
    }

    // 4. C_wrong circuits
    if task.id == "alt" {
        let mut b = Builder::new(task.width);
        let src = b.osc2(hi, 0);
        circuits.push(PanelCircuit {
            id: "osc2_anti",
            class: CircuitClass::Wrong,
            circuit: b.finish(src),
            detour_nodes: Vec::new(),
            reference: None,
            expected_accuracy: AccuracyCheck::Exact(0.0),
        });
    }
    if task.id == "motif3" && k == 2 {
        let mut b = Builder::new(task.width);
        let x = b.x();
        let src = b.shift(1, x);
        circuits.push(PanelCircuit {
            id: "shift1",
            class: CircuitClass::Wrong,
            circuit: b.finish(src),
            detour_nodes: Vec::new(),
            reference: None,
            expected_accuracy: AccuracyCheck::Exact(0.0),
        });
    }

    // 5. C_detour circuits
    // Base is the first listed C_opt of this task
    let (base_id, _) = {
        let mut dummy = Builder::new(task.width);
        build_base_source(task.id, &mut dummy, hi)
    };

    // addzero: Add(Base, ConstLoop(0)) -> Out
    {
        let mut b = Builder::new(task.width);
        let (_, base_src) = build_base_source(task.id, &mut b, hi);
        let mark = b.node_count();
        let c0 = b.const_loop(0);
        let detour_nodes = b.nodes_since(mark);
        let out = b.add(base_src, c0);
        circuits.push(PanelCircuit {
            id: "addzero",
            class: CircuitClass::Detour,
            circuit: b.finish(out),
            detour_nodes,
            reference: Some(base_id),
            expected_accuracy: AccuracyCheck::Exact(1.0),
        });
    }

    // onefire: Add(Base, Delay{init:1}(ConstLoop(0))) -> Out (on every non-constant task)
    if task.id != "const0" && task.id != "const1" {
        let mut b = Builder::new(task.width);
        let (_, base_src) = build_base_source(task.id, &mut b, hi);
        let mark = b.node_count();
        let c0 = b.const_loop(0);
        let d_one = b.circuit.add_node(Primitive::Delay { init: 1 });
        b.circuit.connect(
            c0,
            SignalSink::NodeIn {
                node: d_one,
                port: 0,
            },
        );
        let detour_nodes = b.nodes_since(mark);
        let out = b.add(
            base_src,
            SignalSource::NodeOut {
                node: d_one,
                port: 0,
            },
        );
        circuits.push(PanelCircuit {
            id: "onefire",
            class: CircuitClass::Detour,
            circuit: b.finish(out),
            detour_nodes,
            reference: Some(base_id),
            expected_accuracy: AccuracyCheck::Exact(1.0),
        });
    }

    // mulzero: Add(Base, Mul(Bosc, ConstLoop(0))) -> Out where Bosc = Osc2(1, 0)
    {
        let mut b = Builder::new(task.width);
        let (_, base_src) = build_base_source(task.id, &mut b, hi);
        let mark = b.node_count();
        let bosc = b.osc2(1, 0);
        let c0 = b.const_loop(0);
        let mul = b.mul(bosc, c0);
        let detour_nodes = b.nodes_since(mark);
        let out = b.add(base_src, mul);
        circuits.push(PanelCircuit {
            id: "mulzero",
            class: CircuitClass::Detour,
            circuit: b.finish(out),
            detour_nodes,
            reference: Some(base_id),
            expected_accuracy: AccuracyCheck::Exact(1.0),
        });
    }

    // cancelsub: Sb = Split(Bosc); Sub(Add(Base, Sb.out0), Sb.out1) -> Out
    {
        let mut b = Builder::new(task.width);
        let (_, base_src) = build_base_source(task.id, &mut b, hi);
        let mark = b.node_count();
        let bosc = b.osc2(1, 0);
        let (sb0, sb1) = b.split(bosc);
        let detour_nodes = b.nodes_since(mark);
        let sum = b.add(base_src, sb0);
        let out = b.sub(sum, sb1);
        circuits.push(PanelCircuit {
            id: "cancelsub",
            class: CircuitClass::Detour,
            circuit: b.finish(out),
            detour_nodes,
            reference: Some(base_id),
            expected_accuracy: AccuracyCheck::Exact(1.0),
        });
    }

    // serial_not: Eqz(Eqz(Eqz(x))) -> Out (alt at k=1)
    if task.id == "alt" && k == 1 {
        let mut b = Builder::new(task.width);
        let x = b.x();
        let not1 = b.eqz(x);
        let not2 = b.eqz(not1);
        let not3 = b.eqz(not2);
        circuits.push(PanelCircuit {
            id: "serial_not",
            class: CircuitClass::Detour,
            circuit: b.finish(not3),
            detour_nodes: Vec::new(),
            reference: Some("eqz"),
            expected_accuracy: AccuracyCheck::Exact(1.0),
        });
    }

    // loop4: Loop4(0, hi) -> Out (alt)
    if task.id == "alt" {
        let mut b = Builder::new(task.width);
        let src = b.loop4(0, hi);
        circuits.push(PanelCircuit {
            id: "loop4",
            class: CircuitClass::Detour,
            circuit: b.finish(src),
            detour_nodes: Vec::new(),
            reference: Some("osc2"),
            expected_accuracy: AccuracyCheck::Exact(1.0),
        });
    }

    circuits
}

/// Runs one panel circuit through the live economy harness across `horizon` ticks.
pub fn run_panel_case(
    task: &PanelTask,
    pc: &PanelCircuit,
    epsilon: f64,
    seed: u64,
    horizon: u64,
) -> Result<PanelRun, String> {
    let generator = task
        .generator
        .build(task.width)
        .map_err(|e| format!("failed to build generator for {}: {e}", task.id))?;
    let channels = usize::from(generator.channels());
    let noise = if epsilon == 0.0 {
        NoiseModel::None
    } else {
        NoiseModel::IndependentSubstitution {
            epsilon,
            channels: vec![0],
        }
    };
    let mut env = NoisyEnvironment::new(generator, noise, SeedBank::new(seed))
        .map_err(|e| format!("failed to create environment: {e}"))?;

    let debts = well_mixed_description_debts(&pc.circuit);
    let config = EconomyConfig {
        juvenile_ticks: 0,
        reserve_cap: ReserveCap::Uncapped,
        ..EconomyConfig::default()
    };
    let mut live = LiveCircuit::new(pc.circuit.clone(), &debts, config, SeedBank::new(seed), 8)
        .map_err(|e| format!("failed to init LiveCircuit: {e}"))?;

    let mut clean = vec![0u64; channels];
    let mut observed = vec![0u64; channels];
    let mut prev = vec![0u64; 1];

    let h_usize = usize::try_from(horizon).unwrap_or(usize::MAX);
    let mut predictions = Vec::with_capacity(h_usize);
    let mut observed_facts = Vec::with_capacity(h_usize);
    let mut clean_facts = Vec::with_capacity(h_usize);

    let mut matches_observed = 0usize;
    let mut matches_clean = 0usize;
    let mut max_conservation_error = 0.0f64;
    let mut detour_credit_after_tick0 = 0.0f64;
    let mut cumulative_flux = 0.0f64;
    let mut cliff_counters = CliffCountersSummary::default();

    for t in 0..horizon {
        env.step_frame(&mut clean, &mut observed);
        let pred = live
            .predict(&prev)
            .map_err(|e| format!("predict failed at tick {t}: {e}"))?;
        let tick = live
            .settle(&observed)
            .map_err(|e| format!("settle failed at tick {t}: {e}"))?;

        let p0 = pred[0];
        let obs0 = observed[0];
        let cl0 = clean[0];

        predictions.push(p0);
        observed_facts.push(obs0);
        clean_facts.push(cl0);

        if t >= 16 {
            if p0 == obs0 {
                matches_observed += 1;
            }
            if p0 == cl0 {
                matches_clean += 1;
            }
        }

        let sum_delta: f64 = tick.economy.node_delta.iter().sum();
        let expected_change = tick.economy.net_flux - tick.economy.maintenance;
        let conservation_error = (sum_delta - expected_change).abs();
        if conservation_error > max_conservation_error {
            max_conservation_error = conservation_error;
        }

        cumulative_flux += tick.economy.net_flux;
        cliff_counters.reserve_clipped_count += tick.economy.clipped_nodes as u64;
        cliff_counters.reserve_clipped_bits += tick.economy.clipped_bits;
        cliff_counters.reserve_mismatch_floored += tick.economy.mismatch_floored_nodes as u64;

        if t >= 1 {
            for &node in &pc.detour_nodes {
                detour_credit_after_tick0 += tick.economy.node_credit[node.index()];
            }
        }

        prev[0] = obs0;
    }

    let window_len = (horizon.saturating_sub(16)).max(1) as f64;
    let accuracy = (matches_observed as f64) / window_len;
    let clean_accuracy = (matches_clean as f64) / window_len;

    let delta_e = live
        .economy()
        .nodes()
        .iter()
        .map(|n| n.energy - config.initial_energy)
        .sum();
    let min_node_energy = live
        .economy()
        .nodes()
        .iter()
        .map(|n| n.energy)
        .fold(f64::INFINITY, f64::min);
    let max_node_energy = live
        .economy()
        .nodes()
        .iter()
        .map(|n| n.energy)
        .fold(f64::NEG_INFINITY, f64::max);

    let min_node_margin = if live.economy().nodes().is_empty() {
        0.0
    } else {
        min_node_energy - config.initial_energy
    };
    let max_node_margin = if live.economy().nodes().is_empty() {
        0.0
    } else {
        max_node_energy - config.initial_energy
    };

    let max_detour_energy = if pc.detour_nodes.is_empty() {
        None
    } else {
        Some(
            pc.detour_nodes
                .iter()
                .map(|&n| live.economy().nodes()[n.index()].energy)
                .fold(f64::NEG_INFINITY, f64::max),
        )
    };

    let code_bits = circuit_code_bits(&pc.circuit, 0);

    let delta_l_null = null_calibrated_mdl_delta(
        &pc.circuit,
        0.0,
        0,
        &observed_facts,
        &predictions,
        NullModelKind::LaplaceFrequency,
    )
    .map_err(|e| format!("MDL delta failed: {e}"))?;

    Ok(PanelRun {
        task: task.id,
        k: task.width.bits(),
        circuit: pc.id,
        class: pc.class,
        epsilon,
        seed,
        node_count: pc.circuit.node_count(),
        accuracy,
        clean_accuracy,
        delta_e,
        min_node_energy,
        delta_l_null,
        detour_credit_after_tick0,
        max_detour_energy,
        max_conservation_error,
        flux: cumulative_flux,
        code_bits,
        min_node_margin,
        max_node_margin,
        cliff_counters,
    })
}

/// Runs the panel matrix across tasks × circuits × ε for the specified `horizon`.
pub fn run_panel_matrix(seed_offset: u32, seeds: u32, horizon: u64) -> Vec<PanelRun> {
    let mut runs = Vec::new();
    let epsilons = [0.0, 0.05];
    let seeds_tested = seeds.max(1);
    for task in &panel_tasks() {
        for pc in &panel_circuits(task) {
            for &epsilon in &epsilons {
                if epsilon == 0.0 {
                    let seed = u64::from(seed_offset);
                    let run = run_panel_case(task, pc, epsilon, seed, horizon)
                        .unwrap_or_else(|err| panic!("panel case failed: {err}"));
                    runs.push(run);
                } else {
                    for s in 0..seeds_tested {
                        let seed = u64::from(seed_offset + s);
                        let run = run_panel_case(task, pc, epsilon, seed, horizon)
                            .unwrap_or_else(|err| panic!("panel case failed: {err}"));
                        runs.push(run);
                    }
                }
            }
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_panel_circuit_compiles() {
        for task in panel_tasks() {
            let circuits = panel_circuits(&task);
            assert!(!circuits.is_empty(), "task {} has no circuits", task.id);
            for pc in &circuits {
                assert!(
                    pc.circuit.compile().is_ok(),
                    "circuit {} on task {} (k={}) failed to compile: {:?}",
                    pc.id,
                    task.id,
                    task.width.bits(),
                    pc.circuit.compile().err()
                );
            }
        }
    }

    #[test]
    fn panel_construction_self_checks_pass_at_epsilon_zero() {
        for task in panel_tasks() {
            for pc in panel_circuits(&task) {
                let run = run_panel_case(&task, &pc, 0.0, 0, DEFAULT_HORIZON).unwrap();
                match pc.expected_accuracy {
                    AccuracyCheck::Exact(expected) => {
                        assert!(
                            (run.accuracy - expected).abs() < 1e-12,
                            "panel-construction: task {} (k={}) circuit {} accuracy {} != expected {}",
                            task.id,
                            task.width.bits(),
                            pc.id,
                            run.accuracy,
                            expected
                        );
                    }
                    AccuracyCheck::Range(min, max) => {
                        assert!(
                            run.accuracy >= min && run.accuracy <= max,
                            "panel-construction: task {} (k={}) circuit {} accuracy {} not in [{}, {}]",
                            task.id,
                            task.width.bits(),
                            pc.id,
                            run.accuracy,
                            min,
                            max
                        );
                    }
                    AccuracyCheck::None => {}
                }
            }
        }
    }

    #[test]
    fn no_panel_circuit_has_unowned_description_bits() {
        for task in panel_tasks() {
            for pc in panel_circuits(&task) {
                let debts = well_mixed_description_debts(&pc.circuit);
                assert_eq!(debts.len(), pc.circuit.node_count());
                let sum: f64 = debts.iter().sum();
                let total = circuit_code_bits(&pc.circuit, 0);
                assert!(
                    (sum - total).abs() < 1e-12,
                    "task {} circuit {} debts sum {sum} != total {total}",
                    task.id,
                    pc.id
                );
            }
        }
    }
}
