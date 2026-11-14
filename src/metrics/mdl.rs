//! Total Description Length $L = L(\text{palette}) + L(\text{circuit} \mid \text{palette}) + L(\text{facts} \mid \text{circuit})$
//! (Charter §3.4 & §8).

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::metrics::{NullModel, NullModelKind, PointForecastScorer};
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
    /// Port trace count does not match `circuit.num_outputs()`.
    PortCountMismatch { expected: usize, actual: usize },
    /// Phase sequence length does not match observed trace length.
    PhaseLengthMismatch { phases: usize, ticks: usize },
    /// Comparator targets an output port outside `0..circuit.num_outputs()`.
    ComparatorTargetOutOfRange {
        target_port: usize,
        num_outputs: usize,
    },
    /// Comparator prediction slice length does not match trace length.
    ComparatorTraceLengthMismatch {
        comparator: usize,
        expected: usize,
        actual: usize,
    },
}

impl fmt::Display for NullCalibratedMdlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LengthMismatch { facts, predictions } => write!(
                f,
                "facts and predictions length mismatch: {facts} != {predictions}"
            ),
            Self::PortCountMismatch { expected, actual } => write!(
                f,
                "output port count mismatch: expected {expected}, got {actual}"
            ),
            Self::PhaseLengthMismatch { phases, ticks } => {
                write!(f, "phase trace length mismatch: {phases} != {ticks}")
            }
            Self::ComparatorTargetOutOfRange {
                target_port,
                num_outputs,
            } => write!(
                f,
                "comparator target port {target_port} exceeds num_outputs {num_outputs}"
            ),
            Self::ComparatorTraceLengthMismatch {
                comparator,
                expected,
                actual,
            } => write!(
                f,
                "comparator {comparator} trace length mismatch: expected {expected}, got {actual}"
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
pub fn circuit_code_bits(circuit: &Circuit, extra_palette_entries: usize) -> f64 {
    circuit_code_bits_with_tax(circuit, extra_palette_entries, true)
}

/// Encodes $L(\text{circuit} \mid \text{palette})$ for a Level-0 [`Circuit`] with an optional
/// fan-out tax toggle (`split_code_tax`).
#[must_use]
pub fn circuit_code_bits_with_tax(
    circuit: &Circuit,
    extra_palette_entries: usize,
    split_code_tax: bool,
) -> f64 {
    let n = circuit.node_count();
    let m = circuit.edges().len();
    if n == 0 && m == 0 {
        return 0.0;
    }
    let header_bits = elias_gamma_bits(n) + elias_gamma_bits(m);

    let palette_card = (Primitive::PALETTE_SIZE + extra_palette_entries) as f64;
    let base_node_bits = palette_card.log2();
    let k_bits = f64::from(circuit.width().bits());

    let mut node_bits = 0.0;
    for &prim in circuit.nodes() {
        if !split_code_tax && prim == Primitive::Split {
            continue;
        }
        node_bits += base_node_bits;
        if let Primitive::Delay { init } = prim {
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

    let pairs = (src_ports.max(1) * dst_ports.max(1)) as f64;
    let bit_per_wire = pairs.log2();

    let mut wiring_bits = 0.0;
    for edge in circuit.edges() {
        let is_split_port1 = !split_code_tax
            && matches!(edge.src, SignalSource::NodeOut { node, port: 1 } if circuit.nodes()[node.index()] == Primitive::Split);
        if !is_split_port1 {
            wiring_bits += bit_per_wire;
        }
    }

    header_bits + node_bits + wiring_bits
}

/// Precomputed prefix statistics of a stripped base circuit for fast bit-exact ring MDL updates.
#[derive(Clone, Debug)]
pub(crate) struct StrippedRingCodeBase {
    k_bits: f64,
    base_n: usize,
    base_m: usize,
    base_node_bits: f64,
    palette_node_bits: f64,
    base_src_ports: usize,
    base_dst_ports: usize,
    base_counted_wires: usize,
    split_code_tax: bool,
}

impl StrippedRingCodeBase {
    #[must_use]
    pub(crate) fn new(stripped: &Circuit, split_code_tax: bool) -> Self {
        let base_n = stripped.node_count();
        let base_m = stripped.edges().len();
        let palette_card = Primitive::PALETTE_SIZE as f64;
        let palette_node_bits = palette_card.log2();
        let k_bits = f64::from(stripped.width().bits());

        let mut base_node_bits = 0.0;
        for &prim in stripped.nodes() {
            if !split_code_tax && prim == Primitive::Split {
                continue;
            }
            base_node_bits += palette_node_bits;
            if let Primitive::Delay { init } = prim {
                base_node_bits += if init == 0 { 1.0 } else { 1.0 + k_bits };
            }
        }

        let base_src_ports = usize::from(stripped.num_inputs())
            + stripped
                .nodes()
                .iter()
                .map(|p| usize::from(p.output_arity()))
                .sum::<usize>();
        let base_dst_ports = usize::from(stripped.num_outputs())
            + stripped
                .nodes()
                .iter()
                .map(|p| usize::from(p.input_arity()))
                .sum::<usize>();

        let mut base_counted_wires = 0usize;
        for edge in stripped.edges() {
            let is_split_port1 = !split_code_tax
                && matches!(edge.src, SignalSource::NodeOut { node, port: 1 } if stripped.nodes()[node.index()] == Primitive::Split);
            if !is_split_port1 {
                base_counted_wires += 1;
            }
        }

        Self {
            k_bits,
            base_n,
            base_m,
            base_node_bits,
            palette_node_bits,
            base_src_ports,
            base_dst_ports,
            base_counted_wires,
            split_code_tax,
        }
    }

    #[must_use]
    pub(crate) fn code_bits_for_ring(&self, mu: &[u64]) -> f64 {
        let p = mu.len();
        let n = self.base_n + p + 1;
        let m = self.base_m + p + 2;
        let header_bits = elias_gamma_bits(n) + elias_gamma_bits(m);

        let mut node_bits = self.base_node_bits;
        for j in 0..p {
            let init = mu[(p - 1 - j) % p];
            node_bits += self.palette_node_bits;
            node_bits += if init == 0 { 1.0 } else { 1.0 + self.k_bits };
        }
        if self.split_code_tax {
            node_bits += self.palette_node_bits;
        }

        let src_ports = self.base_src_ports + p + 2;
        let dst_ports = self.base_dst_ports + p + 1;
        let pairs = (src_ports.max(1) * dst_ports.max(1)) as f64;
        let bit_per_wire = pairs.log2();

        let total_wires = self.base_counted_wires + (p + 1) + usize::from(self.split_code_tax);
        let mut wiring_bits = 0.0;
        for _ in 0..total_wires {
            wiring_bits += bit_per_wire;
        }

        header_bits + node_bits + wiring_bits
    }
}

/// Decomposed allocation of circuit description length among nodes and unowned wires.
#[derive(Clone, Debug, PartialEq)]
pub struct CircuitDescriptionAllocation {
    /// Description debt assigned to each node index.
    pub node_debts: Vec<f64>,
    /// Bits attributable to wires or headers unowned by any node.
    pub unowned_bits: f64,
}

/// Allocates the circuit's description length among its nodes as description debt $\Delta L_i$
/// (M1a redesign D3, M1b extension D1b.7).
///
/// Returns per-node debts and unowned bits without panicking on $N = 0$ or direct boundary wires.
#[must_use]
pub fn allocate_circuit_description_bits(
    circuit: &Circuit,
    extra_palette_entries: usize,
    split_code_tax: bool,
) -> CircuitDescriptionAllocation {
    let n = circuit.node_count();
    let m = circuit.edges().len();
    if n == 0 && m == 0 {
        return CircuitDescriptionAllocation {
            node_debts: Vec::new(),
            unowned_bits: 0.0,
        };
    }

    let header_bits = elias_gamma_bits(n) + elias_gamma_bits(m);
    let palette_card = (Primitive::PALETTE_SIZE + extra_palette_entries) as f64;
    let base_node_bits = palette_card.log2();
    let k_bits = f64::from(circuit.width().bits());

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

    let pairs = (src_ports.max(1) * dst_ports.max(1)) as f64;
    let bit_per_wire = pairs.log2();

    if n == 0 {
        let mut unowned_bits = header_bits;
        for _ in circuit.edges() {
            unowned_bits += bit_per_wire;
        }
        return CircuitDescriptionAllocation {
            node_debts: Vec::new(),
            unowned_bits,
        };
    }

    let header_share = header_bits / (n as f64);
    let mut debts = Vec::with_capacity(n);
    for prim in circuit.nodes() {
        let mut node_debt = header_share;
        if split_code_tax || *prim != Primitive::Split {
            node_debt += base_node_bits;
        }
        if let Primitive::Delay { init } = *prim {
            node_debt += if init == 0 { 1.0 } else { 1.0 + k_bits };
        }
        debts.push(node_debt);
    }

    let mut unowned_bits = 0.0;
    for edge in circuit.edges() {
        let is_split_port1 = !split_code_tax
            && matches!(edge.src, SignalSource::NodeOut { node, port: 1 } if circuit.nodes()[node.index()] == Primitive::Split);
        let wire_cost = if is_split_port1 { 0.0 } else { bit_per_wire };

        match (edge.src, edge.dst) {
            (SignalSource::NodeOut { node, .. }, _) => {
                debts[node.index()] += wire_cost;
            }
            (SignalSource::BoundaryIn(_), SignalSink::NodeIn { node, .. }) => {
                debts[node.index()] += wire_cost;
            }
            (SignalSource::BoundaryIn(_), SignalSink::BoundaryOut(_)) => {
                unowned_bits += wire_cost;
            }
        }
    }

    let total_debt: f64 = debts.iter().sum::<f64>() + unowned_bits;
    let total_circuit_bits =
        circuit_code_bits_with_tax(circuit, extra_palette_entries, split_code_tax);
    debug_assert!(
        (total_debt - total_circuit_bits).abs() < 1e-12,
        "sum of debts and unowned {total_debt} does not match circuit_code_bits_with_tax {total_circuit_bits}"
    );

    CircuitDescriptionAllocation {
        node_debts: debts,
        unowned_bits,
    }
}

/// Allocates the circuit's description length $L(C)$ among its nodes as description debt $\Delta L_i$
/// (M1a redesign D3).
///
/// Each node owes:
/// - Its share of the header bits: $(\ell_\gamma(n) + \ell_\gamma(m)) / n$
/// - Its node bits: palette index $\log_2(\text{palette})$ and `Delay` init bits
/// - The bits for each wire it drives, or incoming from `BoundaryIn` for the node it feeds
///
/// Fails loudly (panics) if there are unowned bits (e.g. $n = 0$ or direct `BoundaryIn` -> `BoundaryOut` wire).
#[must_use]
pub fn circuit_node_description_debts(circuit: &Circuit, extra_palette_entries: usize) -> Vec<f64> {
    assert!(
        circuit.node_count() > 0,
        "circuit has no nodes to own description length (unallocated header/wire bits)"
    );
    let alloc = allocate_circuit_description_bits(circuit, extra_palette_entries, true);
    assert!(
        alloc.unowned_bits == 0.0,
        "circuit has direct BoundaryIn -> BoundaryOut wire with no node owner"
    );
    alloc.node_debts
}

/// Computes the full [`MdlBreakdown`] for a candidate circuit and its observed prediction counts.
#[must_use]
pub fn compute_mdl(
    circuit: &Circuit,
    palette_bits: f64,
    extra_palette_entries: usize,
    total_symbols: u64,
    mismatches: u64,
) -> MdlBreakdown {
    compute_mdl_with_tax(
        circuit,
        palette_bits,
        extra_palette_entries,
        total_symbols,
        mismatches,
        true,
    )
}

/// Computes the full [`MdlBreakdown`] for a candidate circuit and its observed prediction counts
/// with an explicit `split_code_tax` setting.
#[must_use]
pub fn compute_mdl_with_tax(
    circuit: &Circuit,
    palette_bits: f64,
    extra_palette_entries: usize,
    total_symbols: u64,
    mismatches: u64,
    split_code_tax: bool,
) -> MdlBreakdown {
    let circuit_bits = circuit_code_bits_with_tax(circuit, extra_palette_entries, split_code_tax);
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
pub fn null_calibrated_mdl_delta(
    circuit: &Circuit,
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
    Ok(palette_bits + circuit_code_bits(circuit, extra_palette_entries) - net_reward)
}

/// Execution phase on one tick for epistemic MDL accounting (Charter DR #40).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EpistemicTickPhase {
    /// Supervised wake phase: boundary outputs and comparators score against external ground truth.
    Wake,
    /// Unsupervised sleep/loopback phase: boundary rewards are zero; comparators score internal
    /// agreement against each target port's frozen wake null model.
    Sleep,
}

/// Per-comparator prediction traces used to evaluate [`epistemic_null_calibrated_mdl_delta`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EpistemicComparatorTrace<'a> {
    /// Target boundary output port $p$ supplying the calibrated wake null model $P_{\text{null}}^{(p)}$.
    pub target_port: usize,
    /// Tick index when this comparator became active (`0` if present from tick 0).
    pub registered_at_tick: usize,
    /// Source-channel bitmask of the primary derivation (`sub_node.in(0)`).
    pub primary_channel_mask: u64,
    /// Source-channel bitmask of the alternative derivation (`sub_node.in(1)`).
    pub alt_channel_mask: u64,
    /// Primary derivation outputs (`sub_node.in(0)`) at each tick `0..T`.
    pub primary_predictions: &'a [u64],
    /// Alternative derivation outputs (`sub_node.in(1)`) at each tick `0..T`.
    pub alt_predictions: &'a [u64],
}

/// Source-channel independence weight $\iota(a, b) = \frac{1}{|C_a \cap C_b| + 1}$
/// applied to positive internal agreement in sleep (Charter §1.3 & §3.1.4, DR #40).
#[must_use]
pub fn channel_independence_weight(primary_channel_mask: u64, alt_channel_mask: u64) -> f64 {
    let shared_channels = (primary_channel_mask & alt_channel_mask).count_ones();
    1.0 / (f64::from(shared_channels) + 1.0)
}

fn frozen_sleep_scorer(width: Width, frozen_null: &NullModel) -> PointForecastScorer {
    let mut scorer = PointForecastScorer::new(width, frozen_null.kind());
    *scorer.null_model_mut() = frozen_null.clone();
    scorer
}

/// Scores one Sleep tick using a frozen wake null model without mutating `frozen_null`.
fn score_sleep_step_frozen(
    sleep_scorer: &mut PointForecastScorer,
    width: Width,
    alt_val: u64,
    primary_val: u64,
    iota: f64,
) -> f64 {
    let frozen_null = sleep_scorer.null_model().clone();
    let raw_reward = sleep_scorer.observe(alt_val, primary_val);
    *sleep_scorer.null_model_mut() = frozen_null;
    if width.normalize(alt_val) == width.normalize(primary_val) {
        raw_reward * iota
    } else {
        raw_reward
    }
}

/// Computes epistemic null-calibrated MDL delta $\Delta L_{\text{null}}^{\text{epistemic}}(T)$
/// with default `split_code_tax = true` (Charter §3.4, DR #40).
///
/// With `juvenile_ticks = 0` and `ReserveCap::Uncapped`, $-\Delta L_{\text{null}}^{\text{epistemic}}(T)$
/// equals total live metabolic reserve change $\Delta E(T) = F_{\text{boundary}}(T) + F_{\text{comparators}}(T) - L(C)$
/// across both Wake and Sleep segments. Reduces bit-identically to [`null_calibrated_mdl_delta`]
/// when `comparators` is empty, `num_outputs == 1`, and every tick is [`EpistemicTickPhase::Wake`].
#[expect(
    clippy::too_many_arguments,
    reason = "Matches epistemic MDL specification inputs directly"
)]
pub fn epistemic_null_calibrated_mdl_delta(
    circuit: &Circuit,
    palette_bits: f64,
    extra_palette_entries: usize,
    port_facts: &[&[u64]],
    port_predictions: &[&[u64]],
    phases: &[EpistemicTickPhase],
    comparators: &[EpistemicComparatorTrace<'_>],
    null_model_kind: NullModelKind,
) -> Result<f64, NullCalibratedMdlError> {
    epistemic_null_calibrated_mdl_delta_with_tax(
        circuit,
        palette_bits,
        extra_palette_entries,
        port_facts,
        port_predictions,
        phases,
        comparators,
        null_model_kind,
        true,
    )
}

/// Variant of [`epistemic_null_calibrated_mdl_delta`] with configurable `split_code_tax`.
#[expect(
    clippy::too_many_arguments,
    reason = "Explicit specification of trace inputs preserves exact bit equivalence with null_calibrated_mdl_delta"
)]
pub fn epistemic_null_calibrated_mdl_delta_with_tax(
    circuit: &Circuit,
    palette_bits: f64,
    extra_palette_entries: usize,
    port_facts: &[&[u64]],
    port_predictions: &[&[u64]],
    phases: &[EpistemicTickPhase],
    comparators: &[EpistemicComparatorTrace<'_>],
    null_model_kind: NullModelKind,
    split_code_tax: bool,
) -> Result<f64, NullCalibratedMdlError> {
    let num_outputs = usize::from(circuit.num_outputs());
    if port_facts.len() != num_outputs {
        return Err(NullCalibratedMdlError::PortCountMismatch {
            expected: num_outputs,
            actual: port_facts.len(),
        });
    }
    if port_predictions.len() != num_outputs {
        return Err(NullCalibratedMdlError::PortCountMismatch {
            expected: num_outputs,
            actual: port_predictions.len(),
        });
    }

    let total_ticks = phases.len();
    for (facts, predictions) in port_facts.iter().zip(port_predictions) {
        if facts.len() != predictions.len() {
            return Err(NullCalibratedMdlError::LengthMismatch {
                facts: facts.len(),
                predictions: predictions.len(),
            });
        }
        if facts.len() != total_ticks {
            return Err(NullCalibratedMdlError::PhaseLengthMismatch {
                phases: total_ticks,
                ticks: facts.len(),
            });
        }
    }
    for (idx, comp) in comparators.iter().enumerate() {
        if comp.target_port >= num_outputs {
            return Err(NullCalibratedMdlError::ComparatorTargetOutOfRange {
                target_port: comp.target_port,
                num_outputs,
            });
        }
        if comp.primary_predictions.len() != total_ticks {
            return Err(NullCalibratedMdlError::ComparatorTraceLengthMismatch {
                comparator: idx,
                expected: total_ticks,
                actual: comp.primary_predictions.len(),
            });
        }
        if comp.alt_predictions.len() != total_ticks {
            return Err(NullCalibratedMdlError::ComparatorTraceLengthMismatch {
                comparator: idx,
                expected: total_ticks,
                actual: comp.alt_predictions.len(),
            });
        }
    }

    let width = circuit.width();
    let mut port_scorers: Vec<PointForecastScorer> = (0..num_outputs)
        .map(|_| PointForecastScorer::new(width, null_model_kind))
        .collect();
    let mut comp_wake_scorers: Vec<Option<PointForecastScorer>> = vec![None; comparators.len()];
    let mut comp_sleep_scorers: Vec<Option<PointForecastScorer>> = vec![None; comparators.len()];

    let mut net_reward = 0.0;
    let mut prev_phase: Option<EpistemicTickPhase> = None;

    for (tick, &phase) in phases.iter().enumerate() {
        let entering_sleep =
            phase == EpistemicTickPhase::Sleep && prev_phase != Some(EpistemicTickPhase::Sleep);
        let entering_wake =
            phase == EpistemicTickPhase::Wake && prev_phase == Some(EpistemicTickPhase::Sleep);

        if entering_wake {
            comp_sleep_scorers.fill(None);
        }
        if entering_sleep {
            for ((comp, wake_slot), sleep_slot) in comparators
                .iter()
                .zip(comp_wake_scorers.iter())
                .zip(comp_sleep_scorers.iter_mut())
            {
                if wake_slot.is_some() {
                    let frozen_null = port_scorers[comp.target_port].null_model();
                    *sleep_slot = Some(frozen_sleep_scorer(width, frozen_null));
                }
            }
        }

        for ((comp, wake_slot), sleep_slot) in comparators
            .iter()
            .zip(comp_wake_scorers.iter_mut())
            .zip(comp_sleep_scorers.iter_mut())
        {
            if comp.registered_at_tick == tick && wake_slot.is_none() {
                let frozen_null = port_scorers[comp.target_port].null_model();
                *wake_slot = Some(frozen_sleep_scorer(width, frozen_null));
                if phase == EpistemicTickPhase::Sleep {
                    *sleep_slot = Some(frozen_sleep_scorer(width, frozen_null));
                }
            }
        }

        match phase {
            EpistemicTickPhase::Wake => {
                let mut tick_reward = 0.0;
                for (port, scorer) in port_scorers.iter_mut().enumerate() {
                    tick_reward +=
                        scorer.observe(port_predictions[port][tick], port_facts[port][tick]);
                }
                for (comp, wake_slot) in comparators.iter().zip(comp_wake_scorers.iter_mut()) {
                    if let Some(scorer) = wake_slot.as_mut() {
                        tick_reward += scorer.observe(
                            comp.alt_predictions[tick],
                            port_facts[comp.target_port][tick],
                        );
                    }
                }
                net_reward += tick_reward;
            }
            EpistemicTickPhase::Sleep => {
                let mut tick_reward = 0.0;
                for (comp, sleep_slot) in comparators.iter().zip(comp_sleep_scorers.iter_mut()) {
                    if let Some(sleep_scorer) = sleep_slot.as_mut() {
                        let iota = channel_independence_weight(
                            comp.primary_channel_mask,
                            comp.alt_channel_mask,
                        );
                        tick_reward += score_sleep_step_frozen(
                            sleep_scorer,
                            width,
                            comp.alt_predictions[tick],
                            comp.primary_predictions[tick],
                            iota,
                        );
                    }
                }
                net_reward += tick_reward;
            }
        }

        prev_phase = Some(phase);
    }

    Ok(
        palette_bits + circuit_code_bits_with_tax(circuit, extra_palette_entries, split_code_tax)
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
    fn description_debts_sum_to_circuit_code_bits() {
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

        let debts1 = circuit_node_description_debts(&c1, 0);
        let sum1: f64 = debts1.iter().sum();
        let expected1 = circuit_code_bits(&c1, 0);
        assert!((sum1 - expected1).abs() < 1e-12);
    }

    #[test]
    #[should_panic(expected = "circuit has no nodes to own description length")]
    fn description_debts_fails_on_empty_circuit() {
        let c = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);
        let _ = circuit_node_description_debts(&c, 0);
    }

    #[test]
    #[should_panic(
        expected = "circuit has direct BoundaryIn -> BoundaryOut wire with no node owner"
    )]
    fn description_debts_fails_on_unowned_boundary_wire() {
        let mut c = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);
        let _d = c.add_node(Primitive::Delay { init: 0 });
        c.connect(SignalSource::BoundaryIn(0), SignalSink::BoundaryOut(0));
        let _ = circuit_node_description_debts(&c, 0);
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

        let debts_a = circuit_node_description_debts(&ca, 0);
        let debts_b = circuit_node_description_debts(&cb, 0);

        assert!(
            debts_b[s.index()] > debts_a[d.index()],
            "Split debt {} should exceed single-output node debt {}",
            debts_b[s.index()],
            debts_a[d.index()]
        );
    }

    #[test]
    fn allocate_circuit_description_bits_empty_and_unowned_boundary() {
        // 1. Empty circuit
        let empty = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);
        let alloc_empty = allocate_circuit_description_bits(&empty, 0, true);
        assert_eq!(alloc_empty.node_debts.len(), 0);
        assert_eq!(alloc_empty.unowned_bits, 0.0);
        assert_eq!(circuit_code_bits_with_tax(&empty, 0, true), 0.0);

        // 2. Direct BoundaryIn -> BoundaryOut with no nodes
        let mut direct = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);
        direct.connect(SignalSource::BoundaryIn(0), SignalSink::BoundaryOut(0));
        let alloc_direct = allocate_circuit_description_bits(&direct, 0, true);
        assert_eq!(alloc_direct.node_debts.len(), 0);
        let expected_direct = circuit_code_bits_with_tax(&direct, 0, true);
        assert!((alloc_direct.unowned_bits - expected_direct).abs() < 1e-12);

        // 3. Node-containing circuit with direct unowned wire alongside a node
        let mut hybrid = Circuit::new(Width::BITS_1, 2, 2, WireDelayMode::Combinational);
        let d = hybrid.add_node(Primitive::Delay { init: 0 });
        hybrid.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: d, port: 0 },
        );
        hybrid.connect(
            SignalSource::NodeOut { node: d, port: 0 },
            SignalSink::BoundaryOut(0),
        );
        hybrid.connect(SignalSource::BoundaryIn(1), SignalSink::BoundaryOut(1));
        let alloc_hybrid = allocate_circuit_description_bits(&hybrid, 0, true);
        let sum_hybrid = alloc_hybrid.node_debts.iter().sum::<f64>() + alloc_hybrid.unowned_bits;
        let expected_hybrid = circuit_code_bits_with_tax(&hybrid, 0, true);
        assert!((sum_hybrid - expected_hybrid).abs() < 1e-12);
        assert!(alloc_hybrid.unowned_bits > 0.0);

        // 4. split_code_tax toggle test
        let mut split_circ = Circuit::new(Width::BITS_1, 1, 2, WireDelayMode::Combinational);
        let s = split_circ.add_node(Primitive::Split);
        split_circ.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: s, port: 0 },
        );
        split_circ.connect(
            SignalSource::NodeOut { node: s, port: 0 },
            SignalSink::BoundaryOut(0),
        );
        split_circ.connect(
            SignalSource::NodeOut { node: s, port: 1 },
            SignalSink::BoundaryOut(1),
        );

        let alloc_taxed = allocate_circuit_description_bits(&split_circ, 0, true);
        let alloc_untaxed = allocate_circuit_description_bits(&split_circ, 0, false);
        assert!(alloc_taxed.node_debts[0] > alloc_untaxed.node_debts[0]);

        let cost_taxed = circuit_code_bits_with_tax(&split_circ, 0, true);
        let cost_untaxed = circuit_code_bits_with_tax(&split_circ, 0, false);
        assert!(cost_taxed > cost_untaxed);
        let sum_untaxed = alloc_untaxed.node_debts.iter().sum::<f64>() + alloc_untaxed.unowned_bits;
        assert!((sum_untaxed - cost_untaxed).abs() < 1e-12);
    }

    #[test]
    fn channel_independence_weight_matches_overlap_law() {
        assert!((channel_independence_weight(0b01, 0b10) - 1.0).abs() < 1e-15);
        assert!((channel_independence_weight(0b00, 0b01) - 1.0).abs() < 1e-15);
        assert!((channel_independence_weight(0b01, 0b01) - 0.5).abs() < 1e-15);
        assert!((channel_independence_weight(0b11, 0b11) - (1.0 / 3.0)).abs() < 1e-15);
    }

    #[test]
    fn epistemic_mdl_reduces_bit_identically_when_comparators_empty() {
        let mut circuit = Circuit::new(Width::BITS_2, 1, 1, WireDelayMode::Combinational);
        let d = circuit.add_node(Primitive::Delay { init: 0 });
        circuit.connect(
            SignalSource::BoundaryIn(0),
            SignalSink::NodeIn { node: d, port: 0 },
        );
        circuit.connect(
            SignalSource::NodeOut { node: d, port: 0 },
            SignalSink::BoundaryOut(0),
        );

        let facts: Vec<u64> = (0..80).map(|t| (t * 3 + 1) % 4).collect();
        let preds: Vec<u64> = (0..80)
            .map(|t| if t % 7 == 0 { 0 } else { (t * 3 + 1) % 4 })
            .collect();
        let phases = vec![EpistemicTickPhase::Wake; facts.len()];

        for kind in [NullModelKind::LaplaceFrequency, NullModelKind::RepeatLast] {
            let legacy = null_calibrated_mdl_delta(&circuit, 1.5, 2, &facts, &preds, kind).unwrap();
            let epistemic = epistemic_null_calibrated_mdl_delta(
                &circuit,
                1.5,
                2,
                &[facts.as_slice()],
                &[preds.as_slice()],
                &phases,
                &[],
                kind,
            )
            .unwrap();
            assert_eq!(
                epistemic.to_bits(),
                legacy.to_bits(),
                "epistemic MDL must be bit-identical when comparators are empty ({kind:?})"
            );
        }
    }

    #[test]
    fn epistemic_mdl_sleep_uses_iota_only_on_agreement_and_freezes_null() {
        let circuit = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);
        let wake_len = 64usize;
        let sleep_len = 32usize;
        let total = wake_len + sleep_len;
        let facts: Vec<u64> = (0..total).map(|t| (t % 2) as u64).collect();
        let boundary_preds = facts.clone();
        let pri_preds = facts.clone();
        let alt_preds = facts.clone();
        let mut phases = vec![EpistemicTickPhase::Wake; wake_len];
        phases.extend(std::iter::repeat_n(EpistemicTickPhase::Sleep, sleep_len));

        let wake_only_delta = epistemic_null_calibrated_mdl_delta(
            &circuit,
            0.0,
            0,
            &[&facts[..wake_len]],
            &[&boundary_preds[..wake_len]],
            &phases[..wake_len],
            &[],
            NullModelKind::LaplaceFrequency,
        )
        .unwrap();

        let comp_disjoint = EpistemicComparatorTrace {
            target_port: 0,
            registered_at_tick: 0,
            primary_channel_mask: 0b01,
            alt_channel_mask: 0b10,
            primary_predictions: &pri_preds,
            alt_predictions: &alt_preds,
        };
        let comp_shared = EpistemicComparatorTrace {
            primary_channel_mask: 0b01,
            alt_channel_mask: 0b01,
            ..comp_disjoint
        };

        let delta_no_comp_wake_sleep = epistemic_null_calibrated_mdl_delta(
            &circuit,
            0.0,
            0,
            &[facts.as_slice()],
            &[boundary_preds.as_slice()],
            &phases,
            &[],
            NullModelKind::LaplaceFrequency,
        )
        .unwrap();
        // Boundary rewards are disabled during Sleep, so M=0 MDL matches Wake-only.
        assert_eq!(
            delta_no_comp_wake_sleep.to_bits(),
            wake_only_delta.to_bits()
        );

        let delta_disjoint = epistemic_null_calibrated_mdl_delta(
            &circuit,
            0.0,
            0,
            &[facts.as_slice()],
            &[boundary_preds.as_slice()],
            &phases,
            &[comp_disjoint],
            NullModelKind::LaplaceFrequency,
        )
        .unwrap();
        let delta_shared = epistemic_null_calibrated_mdl_delta(
            &circuit,
            0.0,
            0,
            &[facts.as_slice()],
            &[boundary_preds.as_slice()],
            &phases,
            &[comp_shared],
            NullModelKind::LaplaceFrequency,
        )
        .unwrap();

        // Both gain extra Wake and Sleep comparator reward, and disjoint Sleep agreement pays 2x shared.
        assert!(delta_disjoint < delta_shared);
        assert!(delta_shared < delta_no_comp_wake_sleep);
    }
}
