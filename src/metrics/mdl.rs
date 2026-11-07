//! Total Description Length $L = L(\text{palette}) + L(\text{circuit} \mid \text{palette}) + L(\text{facts} \mid \text{circuit})$
//! (Charter §3.4 & §8).

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::metrics::{NullModelKind, PointForecastScorer};
use crate::structural::{GridShape, WiringTopology};
use crate::substrate::{Circuit, Primitive, SignalSink, SignalSource, Width};

/// Decomposed Minimum Description Length (MDL) accounting in bits (§3.4).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MdlBreakdown {
    /// $L(\text{palette})$: bits required to specify derived axioms (`0.0` at Level 0).
    pub palette_bits: f64,
    /// $L(\text{circuit} \mid \text{palette})$: bits required to encode primitives, registers, and wiring.
    pub circuit_bits: f64,
    /// $L(\text{facts} \mid \text{circuit})$: sequential residual bits required to encode observed facts.
    pub facts_bits: f64,
    /// Total description length $L = \text{palette\_bits} + \text{circuit\_bits} + \text{facts\_bits}$.
    pub total_bits: f64,
}

/// Invalid input to null-calibrated MDL accounting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NullCalibratedMdlError {
    /// Observed facts and predictions have different lengths.
    LengthMismatch { facts: usize, predictions: usize },
}

impl fmt::Display for NullCalibratedMdlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LengthMismatch { facts, predictions } => write!(
                f,
                "facts and predictions length mismatch: {facts} != {predictions}"
            ),
        }
    }
}

impl std::error::Error for NullCalibratedMdlError {}

/// Elias-$\gamma$ universal code length $\ell_\gamma(n) = 2 \lfloor \log_2(n + 1) \rfloor + 1$ bits.
#[must_use]
pub fn elias_gamma_bits(n: usize) -> f64 {
    let val = (n as u128) + 1;
    let floor_log2 = 127 - val.leading_zeros();
    f64::from(2 * floor_log2 + 1)
}

/// Exact Krichevsky–Trofimov sequential binary code length (in bits) for `matches` (`0`s)
/// and `mismatches` (`1`s).
#[must_use]
pub fn kt_binary_sequence_bits(matches: u64, mismatches: u64) -> f64 {
    let total = matches + mismatches;
    if total == 0 {
        return 0.0;
    }
    // Exact summation for moderate horizons; Stirling asymptotic for very large horizons.
    if total <= 8192 {
        let mut log2_num = 0.0f64;
        for i in 0..matches {
            log2_num += ((i as f64) + 0.5).log2();
        }
        for j in 0..mismatches {
            log2_num += ((j as f64) + 0.5).log2();
        }
        let mut log2_den = 0.0f64;
        for t in 0..total {
            log2_den += ((t as f64) + 1.0).log2();
        }
        (log2_den - log2_num).max(0.0)
    } else {
        let t_f = total as f64;
        let p_err = ((mismatches as f64) + 0.5) / (t_f + 1.0);
        let h = -p_err * p_err.log2() - (1.0 - p_err) * (1.0 - p_err).log2();
        t_f * h + 0.5 * t_f.log2()
    }
}

/// Encodes $L(\text{facts} \mid \text{circuit})$ over `total_symbols` symbol predictions with `mismatches` errors.
///
/// Monotonised past the random-guessing error rate $1 - 2^{-k}$ so a predictor worse than chance
/// (e.g., 100% inverted at $k = 1$) never receives an implicit free inversion discount.
#[must_use]
pub fn facts_code_bits(width: Width, total_symbols: u64, mismatches: u64) -> f64 {
    if total_symbols == 0 {
        return 0.0;
    }
    let k = width.bits();
    let alt_bits_per_mismatch = if k == 1 {
        0.0
    } else if k == 64 {
        64.0
    } else {
        (((1u64 << k) - 1) as f64).log2()
    };

    let raw_cost = |m: u64| -> f64 {
        let matches = total_symbols.saturating_sub(m);
        kt_binary_sequence_bits(matches, m) + (m as f64) * alt_bits_per_mismatch
    };

    // Chance mismatch count floor(T * (1 - 2^{-k})).
    let chance_mismatches = if k >= 64 {
        total_symbols
    } else {
        let num = u128::from(total_symbols) * ((1u128 << k) - 1);
        (num >> k) as u64
    };

    if mismatches <= chance_mismatches {
        raw_cost(mismatches)
    } else {
        // Strictly penalise above-chance error rates without implicit bit inversion.
        raw_cost(chance_mismatches) + ((mismatches - chance_mismatches) as f64)
    }
}

/// Encodes $L(\text{circuit} \mid \text{palette})$ for a Level-0 [`Circuit`].
#[must_use]
pub fn circuit_code_bits(
    circuit: &Circuit,
    topology: WiringTopology,
    grid_shape: Option<GridShape>,
    extra_palette_entries: usize,
) -> f64 {
    let n = circuit.node_count();
    let m = circuit.edges().len();
    let header_bits = elias_gamma_bits(n) + elias_gamma_bits(m);

    let palette_card = (Primitive::PALETTE_SIZE + extra_palette_entries) as f64;
    let mut node_bits = (n as f64) * palette_card.log2();
    let k_bits = f64::from(circuit.width().bits());
    for &prim in circuit.nodes() {
        if let Primitive::Delay { init } = prim {
            // 1-bit flag for default zero vs. explicit k-bit seed register.
            node_bits += if init == 0 { 1.0 } else { 1.0 + k_bits };
        }
    }

    let src_ports = usize::from(circuit.num_inputs())
        + circuit
            .nodes()
            .iter()
            .map(|p| usize::from(p.output_arity()))
            .sum::<usize>();
    let dst_ports = usize::from(circuit.num_outputs())
        + circuit
            .nodes()
            .iter()
            .map(|p| usize::from(p.input_arity()))
            .sum::<usize>();

    let wiring_bits = match (topology, grid_shape) {
        (WiringTopology::LocalVonNeumann, Some(shape)) => {
            let cells = (usize::from(shape.rows) * usize::from(shape.cols)).max(1) as f64;
            // Cell placement + 2-bit cardinal step + port slot bit.
            (n as f64) * cells.log2() + (m as f64) * 3.0
        }
        (WiringTopology::LocalMoore, Some(shape)) => {
            let cells = (usize::from(shape.rows) * usize::from(shape.cols)).max(1) as f64;
            (n as f64) * cells.log2() + (m as f64) * 4.0
        }
        _ => {
            let pairs = (src_ports.max(1) * dst_ports.max(1)) as f64;
            (m as f64) * pairs.log2()
        }
    };

    header_bits + node_bits + wiring_bits
}

/// Allocates the circuit's description length $L(C)$ among its nodes as description debt $\Delta L_i$
/// (M1a redesign D3).
///
/// Each node owes:
/// - Its share of the header bits: $(\ell_\gamma(n) + \ell_\gamma(m)) / n$
/// - Its node bits: palette index $\log_2(\text{palette})$, `Delay` init bits, and lattice placement bits $\log_2(\text{cells})$
/// - The bits for each wire it drives, or incoming from `BoundaryIn` for the node it feeds
///
/// Fails loudly (panics) if there are unowned bits (e.g. $n = 0$ or direct `BoundaryIn` -> `BoundaryOut` wire).
#[must_use]
pub fn circuit_node_description_debts(
    circuit: &Circuit,
    topology: WiringTopology,
    grid_shape: Option<GridShape>,
    extra_palette_entries: usize,
) -> Vec<f64> {
    let n = circuit.node_count();
    let m = circuit.edges().len();
    assert!(
        n > 0,
        "circuit has no nodes to own description length (unallocated header/wire bits)"
    );

    let header_bits = elias_gamma_bits(n) + elias_gamma_bits(m);
    let header_share = header_bits / (n as f64);

    let palette_card = (Primitive::PALETTE_SIZE + extra_palette_entries) as f64;
    let base_node_bits = palette_card.log2();
    let k_bits = f64::from(circuit.width().bits());

    let placement_bits = match (topology, grid_shape) {
        (WiringTopology::LocalVonNeumann | WiringTopology::LocalMoore, Some(shape)) => {
            let cells = (usize::from(shape.rows) * usize::from(shape.cols)).max(1) as f64;
            cells.log2()
        }
        _ => 0.0,
    };

    let src_ports = usize::from(circuit.num_inputs())
        + circuit
            .nodes()
            .iter()
            .map(|p| usize::from(p.output_arity()))
            .sum::<usize>();
    let dst_ports = usize::from(circuit.num_outputs())
        + circuit
            .nodes()
            .iter()
            .map(|p| usize::from(p.input_arity()))
            .sum::<usize>();

    let bit_per_wire = match (topology, grid_shape) {
        (WiringTopology::LocalVonNeumann, Some(_)) => 3.0,
        (WiringTopology::LocalMoore, Some(_)) => 4.0,
        _ => {
            let pairs = (src_ports.max(1) * dst_ports.max(1)) as f64;
            pairs.log2()
        }
    };

    let mut debts = Vec::with_capacity(n);
    for prim in circuit.nodes() {
        let mut node_debt = header_share + base_node_bits + placement_bits;
        if let Primitive::Delay { init } = *prim {
            node_debt += if init == 0 { 1.0 } else { 1.0 + k_bits };
        }
        debts.push(node_debt);
    }

    for edge in circuit.edges() {
        match (edge.src, edge.dst) {
            (SignalSource::NodeOut { node, .. }, _) => {
                debts[node.index()] += bit_per_wire;
            }
            (SignalSource::BoundaryIn(_), SignalSink::NodeIn { node, .. }) => {
                debts[node.index()] += bit_per_wire;
            }
            (SignalSource::BoundaryIn(_), SignalSink::BoundaryOut(_)) => {
                panic!("circuit has direct BoundaryIn -> BoundaryOut wire with no node owner");
            }
        }
    }

    let total_debt: f64 = debts.iter().sum();
    let total_circuit_bits =
        circuit_code_bits(circuit, topology, grid_shape, extra_palette_entries);
    debug_assert!(
        (total_debt - total_circuit_bits).abs() < 1e-12,
        "sum of node description debts {total_debt} does not match circuit_code_bits {total_circuit_bits}"
    );

    debts
}

/// Computes the full [`MdlBreakdown`] for a candidate circuit and its observed prediction counts.
#[must_use]
pub fn compute_mdl(
    circuit: &Circuit,
    topology: WiringTopology,
    grid_shape: Option<GridShape>,
    palette_bits: f64,
    extra_palette_entries: usize,
    total_symbols: u64,
    mismatches: u64,
) -> MdlBreakdown {
    let circuit_bits = circuit_code_bits(circuit, topology, grid_shape, extra_palette_entries);
    let facts_bits = facts_code_bits(circuit.width(), total_symbols, mismatches);
    let total_bits = palette_bits + circuit_bits + facts_bits;
    MdlBreakdown {
        palette_bits,
        circuit_bits,
        facts_bits,
        total_bits,
    }
}

/// Computes circuit description length relative to a prequential null model.
///
/// The centered exact-match reward is the reduction in prequential residual code length relative
/// to the null stream; the circuit description is then added once, as required by the MDL balance.
#[allow(clippy::too_many_arguments)]
pub fn null_calibrated_mdl_delta(
    circuit: &Circuit,
    topology: WiringTopology,
    grid_shape: Option<GridShape>,
    palette_bits: f64,
    extra_palette_entries: usize,
    facts: &[u64],
    predictions: &[u64],
    null_model_kind: NullModelKind,
) -> Result<f64, NullCalibratedMdlError> {
    if facts.len() != predictions.len() {
        return Err(NullCalibratedMdlError::LengthMismatch {
            facts: facts.len(),
            predictions: predictions.len(),
        });
    }
    let width = circuit.width();
    let mut scorer = PointForecastScorer::new(width, null_model_kind);
    let mut net_reward = 0.0;
    for (&fact, &prediction) in facts.iter().zip(predictions) {
        net_reward += scorer.observe(prediction, fact);
    }
    Ok(
        palette_bits + circuit_code_bits(circuit, topology, grid_shape, extra_palette_entries)
            - net_reward,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::NullModelKind;
    use crate::substrate::{SignalSink, SignalSource, WireDelayMode};

    #[test]
    fn null_calibrated_mdl_rewards_an_exact_predictor_over_the_null_model() {
        let mut circuit = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);
        circuit.connect(SignalSource::BoundaryIn(0), SignalSink::BoundaryOut(0));
        let facts: Vec<u64> = (0..64).map(|tick| tick % 2).collect();
        let delta = null_calibrated_mdl_delta(
            &circuit,
            WiringTopology::WellMixed,
            None,
            0.0,
            0,
            &facts,
            &facts,
            NullModelKind::LaplaceFrequency,
        )
        .unwrap();
        assert!(delta < 0.0, "exact predictor should beat null, got {delta}");
    }

    #[test]
    fn null_calibrated_mdl_rejects_mismatched_traces() {
        let circuit = Circuit::new(Width::BITS_1, 0, 1, WireDelayMode::Combinational);
        let error = null_calibrated_mdl_delta(
            &circuit,
            WiringTopology::WellMixed,
            None,
            0.0,
            0,
            &[0, 1],
            &[0],
            NullModelKind::LaplaceFrequency,
        )
        .unwrap_err();
        assert_eq!(
            error,
            NullCalibratedMdlError::LengthMismatch {
                facts: 2,
                predictions: 1
            }
        );
    }

    #[test]
    fn description_debts_sum_to_circuit_code_bits_for_well_mixed_and_lattice() {
        let mut c1 = Circuit::new(Width::BITS_2, 1, 1, WireDelayMode::Combinational);
        let d = c1.add_node(Primitive::Delay { init: 1 });
        let s = c1.add_node(Primitive::Split);
        c1.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: d, port: 0 },
        );
        c1.connect(
            SignalSource::NodeOut { node: d, port: 0 },
            SignalSink::NodeIn { node: s, port: 0 },
        );
        c1.connect(
            SignalSource::NodeOut { node: s, port: 0 },
            SignalSink::BoundaryOut(0),
        );
        c1.connect(
            SignalSource::NodeOut { node: s, port: 1 },
            SignalSink::NodeIn { node: d, port: 0 },
        );

        // Well-mixed
        let debts1 = circuit_node_description_debts(&c1, WiringTopology::WellMixed, None, 0);
        let sum1: f64 = debts1.iter().sum();
        let expected1 = circuit_code_bits(&c1, WiringTopology::WellMixed, None, 0);
        assert!((sum1 - expected1).abs() < 1e-12);

        // Lattice (Von Neumann)
        let shape = GridShape::new(3, 3).unwrap();
        let debts2 =
            circuit_node_description_debts(&c1, WiringTopology::LocalVonNeumann, Some(shape), 0);
        let sum2: f64 = debts2.iter().sum();
        let expected2 = circuit_code_bits(&c1, WiringTopology::LocalVonNeumann, Some(shape), 0);
        assert!((sum2 - expected2).abs() < 1e-12);

        // Lattice (Moore)
        let debts3 =
            circuit_node_description_debts(&c1, WiringTopology::LocalMoore, Some(shape), 0);
        let sum3: f64 = debts3.iter().sum();
        let expected3 = circuit_code_bits(&c1, WiringTopology::LocalMoore, Some(shape), 0);
        assert!((sum3 - expected3).abs() < 1e-12);
    }

    #[test]
    #[should_panic(expected = "circuit has no nodes to own description length")]
    fn description_debts_fails_on_empty_circuit() {
        let c = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);
        let _ = circuit_node_description_debts(&c, WiringTopology::WellMixed, None, 0);
    }

    #[test]
    #[should_panic(
        expected = "circuit has direct BoundaryIn -> BoundaryOut wire with no node owner"
    )]
    fn description_debts_fails_on_unowned_boundary_wire() {
        let mut c = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);
        let _d = c.add_node(Primitive::Delay { init: 0 });
        c.connect(SignalSource::BoundaryIn(0), SignalSink::BoundaryOut(0));
        let _ = circuit_node_description_debts(&c, WiringTopology::WellMixed, None, 0);
    }

    #[test]
    fn split_owes_more_description_debt_than_single_output_node() {
        let mut ca = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);
        let d = ca.add_node(Primitive::Delay { init: 0 });
        ca.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: d, port: 0 },
        );
        ca.connect(
            SignalSource::NodeOut { node: d, port: 0 },
            SignalSink::BoundaryOut(0),
        );

        let mut cb = Circuit::new(Width::BITS_1, 1, 2, WireDelayMode::Combinational);
        let s = cb.add_node(Primitive::Split);
        cb.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: s, port: 0 },
        );
        cb.connect(
            SignalSource::NodeOut { node: s, port: 0 },
            SignalSink::BoundaryOut(0),
        );
        cb.connect(
            SignalSource::NodeOut { node: s, port: 1 },
            SignalSink::BoundaryOut(1),
        );

        let debts_a = circuit_node_description_debts(&ca, WiringTopology::WellMixed, None, 0);
        let debts_b = circuit_node_description_debts(&cb, WiringTopology::WellMixed, None, 0);

        assert!(
            debts_b[s.index()] > debts_a[d.index()],
            "Split debt {} should exceed single-output node debt {}",
            debts_b[s.index()],
            debts_a[d.index()]
        );
    }
}
