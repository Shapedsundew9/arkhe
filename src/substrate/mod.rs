//! Charter §2 Substrate: exact $\mathbb{Z}_{2^k}$ values, Level-0 primitives, invariants, and evaluators.

pub mod circuit;
pub mod eval;
pub mod invariants;
pub mod primitive;
pub mod value;

pub use circuit::{Circuit, Edge, NodeId, SignalSink, SignalSource, WireDelayMode};
pub use eval::{BitParallel64State, CompiledCircuit, EvalError, ScalarState};
pub use invariants::InvariantError;
pub use primitive::Primitive;
pub use value::{Width, WidthError};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invariant_rejections_cover_all_charter_2_5_rules() {
        // 1. Combinational loop without DELAY is rejected.
        let mut loop_circ = Circuit::new(Width::BITS_1, 0, 0, WireDelayMode::Combinational);
        let n0 = loop_circ.add_node(Primitive::Eqz);
        let n1 = loop_circ.add_node(Primitive::Eqz);
        loop_circ.connect(
            SignalSource::NodeOut { node: n0, port: 0 },
            SignalSink::NodeIn { node: n1, port: 0 },
        );
        loop_circ.connect(
            SignalSource::NodeOut { node: n1, port: 0 },
            SignalSink::NodeIn { node: n0, port: 0 },
        );
        assert!(matches!(
            loop_circ.compile(),
            Err(InvariantError::CombinationalLoop { .. })
        ));

        // 2. Adding a DELAY inside the cycle makes it valid!
        let mut osc = Circuit::new(Width::BITS_1, 0, 1, WireDelayMode::Combinational);
        let d = osc.add_node(Primitive::delay_zero());
        let s = osc.add_node(Primitive::Split);
        let inv = osc.add_node(Primitive::Eqz);
        osc.connect(
            SignalSource::NodeOut { node: d, port: 0 },
            SignalSink::NodeIn { node: s, port: 0 },
        );
        osc.connect(
            SignalSource::NodeOut { node: s, port: 0 },
            SignalSink::NodeIn { node: inv, port: 0 },
        );
        osc.connect(
            SignalSource::NodeOut { node: inv, port: 0 },
            SignalSink::NodeIn { node: d, port: 0 },
        );
        osc.connect(
            SignalSource::NodeOut { node: s, port: 1 },
            SignalSink::BoundaryOut(0),
        );
        let compiled = osc.compile().expect("DELAY breaks cycle");
        let mut st = ScalarState::new(&compiled);
        let mut out = [0u64];
        let mut trace = Vec::new();
        for _ in 0..6 {
            st.step(&compiled, &[], &mut out).unwrap();
            trace.push(out[0]);
        }
        assert_eq!(trace, vec![0, 1, 0, 1, 0, 1]);

        // 3. Fan-out without SPLIT is rejected.
        let mut bad_fan = Circuit::new(Width::BITS_2, 1, 2, WireDelayMode::Combinational);
        bad_fan.connect(SignalSource::BoundaryIn(0), SignalSink::BoundaryOut(0));
        bad_fan.connect(SignalSource::BoundaryIn(0), SignalSink::BoundaryOut(1));
        assert!(matches!(
            bad_fan.compile(),
            Err(InvariantError::IllegalFanOut { .. })
        ));

        // 4. Multiple drivers to one boundary output port is rejected.
        let mut bad_drv = Circuit::new(Width::BITS_2, 2, 1, WireDelayMode::Combinational);
        bad_drv.connect(SignalSource::BoundaryIn(0), SignalSink::BoundaryOut(0));
        bad_drv.connect(SignalSource::BoundaryIn(1), SignalSink::BoundaryOut(0));
        assert!(matches!(
            bad_drv.compile(),
            Err(InvariantError::MultipleDrivers { .. })
        ));

        // 5. Unconnected active primitive input is rejected.
        let mut unconn = Circuit::new(Width::BITS_2, 1, 1, WireDelayMode::Combinational);
        let add = unconn.add_node(Primitive::Add);
        unconn.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: add, port: 0 },
        );
        unconn.connect(
            SignalSource::NodeOut { node: add, port: 0 },
            SignalSink::BoundaryOut(0),
        );
        assert!(matches!(
            unconn.compile(),
            Err(InvariantError::UnconnectedInput { node, port: 1 }) if node == add
        ));

        // 6. Zero hops in TicksPerHop mode is rejected; >=1 hops breaks loops cleanly.
        let mut zero_hop = Circuit::new(Width::BITS_2, 1, 1, WireDelayMode::TicksPerHop);
        zero_hop.connect_hops(SignalSource::BoundaryIn(0), SignalSink::BoundaryOut(0), 0);
        assert!(matches!(
            zero_hop.compile(),
            Err(InvariantError::ZeroHopInDelayedMode { edge_index: 0 })
        ));
    }

    #[test]
    fn ticks_per_hop_pipeline_matches_exact_hop_latency() {
        let mut pipe = Circuit::new(Width::BITS_2, 1, 1, WireDelayMode::TicksPerHop);
        pipe.connect_hops(SignalSource::BoundaryIn(0), SignalSink::BoundaryOut(0), 3);
        let compiled = pipe.compile().unwrap();
        let mut st = ScalarState::new(&compiled);
        let mut out = [0u64];
        let inputs = [1u64, 2, 3, 1, 0, 2];
        let mut outputs = Vec::new();
        for &x in &inputs {
            st.step(&compiled, &[x], &mut out).unwrap();
            outputs.push(out[0]);
        }
        assert_eq!(outputs, vec![0, 0, 0, 1, 2, 3]);
    }
}
