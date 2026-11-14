//! Coordinate-free structural graph edit policy helpers (§1.3).
//!
//! Encodes neurogenesis and growth policies over [`Circuit`] multigraphs without spatial coordinate grids.

use crate::substrate::{Circuit, NodeId, Primitive, SignalSink, SignalSource};

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

/// Builds an autonomous delay ring generator driving `out_port`.
pub fn build_autonomous_ring(
    circuit: &mut Circuit,
    period: usize,
    mu: &[u64],
    out_port: u16,
) -> (Vec<NodeId>, NodeId) {
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
    circuit.connect(
        SignalSource::NodeOut {
            node: split,
            port: 1,
        },
        SignalSink::BoundaryOut(out_port),
    );
    (delays, split)
}
