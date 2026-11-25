//! External oracle instrumentation for Milestone M4a (`R8`, Principle 5).
//!
//! Provides:
//! - [`PortLanguageOracle`]: exact $(m + 1)$-word port language and forbidden-word checks derived
//!   strictly from a clean [`FiniteStateSpec`].
//! - [`detect_loopback_attractor`]: free-run loopback state-recurrence and spurious-cycle detector.
//! - [`extract_derivation_circuit`] and [`certify_derivation_against_spec`]: standalone cone
//!   extraction and exact joint-state certification against the clean generator specification.
//! - [`label_circuit_oracle`], [`label_learner_oracle`], and [`audit_sleep_segment_oracle`]:
//!   node- and port-level oracle labels and wrongful-excision auditing across sleep segments.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::baselines::DEFAULT_MAX_CERTIFY_JOINT_STEPS;
use crate::baselines::exhaustive::certify_circuit_port_against_spec;
use crate::env::{FiniteStateSpec, NoiseModel, solve_epsilon_machine};
use crate::structural::surgery::register_mapping;
use crate::structural::{
    ArkheLearner, ComparatorSite, cone_channel_mask, excise_comparator_branch,
};
use crate::substrate::{
    Circuit, CompiledCircuit, EvalError, NodeId, Primitive, ScalarState, SignalSink, SignalSource,
    Width, WireDelayMode,
};

/// Default upper bound on free-run loopback steps before reporting non-closure.
pub const DEFAULT_MAX_LOOPBACK_ATTRACTOR_STEPS: usize = 65_536;

/// Exact clean-spec $(m + 1)$-word port language oracle (`R8`).
///
/// Built strictly from a clean [`FiniteStateSpec`] (noisy tasks must supply their underlying clean
/// spec). Here `m` is the clean $\varepsilon$-machine's exact synchronisation order
/// (`exact_sync_order`, handling `m == 0` as `word_len = 1`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortLanguageOracle {
    width: Width,
    channels: u16,
    sync_order: usize,
    word_len: usize,
    recurrent_period: usize,
    allowed_words_per_port: Vec<BTreeSet<Vec<u64>>>,
}

impl PortLanguageOracle {
    /// Constructs a [`PortLanguageOracle`] from a clean deterministic [`FiniteStateSpec`].
    ///
    /// # Panics
    ///
    /// Panics if `spec.num_states == 0`, `spec.initial_state >= spec.num_states`, or `spec`
    /// transitions contain out-of-bounds states.
    #[must_use]
    pub fn from_clean_spec(spec: &FiniteStateSpec) -> Self {
        assert!(spec.num_states >= 1, "FiniteStateSpec requires >= 1 state");
        assert!(
            spec.initial_state < spec.num_states,
            "FiniteStateSpec initial_state out of range"
        );
        let summary = solve_epsilon_machine(spec, &NoiseModel::None);
        let sync_order = summary
            .exact_sync_order
            .expect("clean FiniteStateSpec must yield exact_sync_order");
        let word_len = sync_order + 1;

        let mut visited_at: BTreeMap<usize, usize> = BTreeMap::new();
        let mut trajectory: Vec<usize> = Vec::new();
        let mut curr = spec.initial_state;
        let transient_ticks = loop {
            if let Some(&first_idx) = visited_at.get(&curr) {
                break first_idx;
            }
            visited_at.insert(curr, trajectory.len());
            trajectory.push(curr);
            curr = spec.transitions[curr].0;
        };
        let cycle_states = &trajectory[transient_ticks..];
        let cycle_len = cycle_states.len();
        let channels = usize::from(spec.channels);
        let width = spec.width;

        let cycle_emissions: Vec<Vec<u64>> = cycle_states
            .iter()
            .map(|&s| {
                spec.transitions[s]
                    .1
                    .iter()
                    .map(|&v| width.normalize(v))
                    .collect()
            })
            .collect();

        let mut allowed_words_per_port = vec![BTreeSet::new(); channels];
        if cycle_len > 0 {
            for (p, allowed_set) in allowed_words_per_port.iter_mut().enumerate() {
                for start in 0..cycle_len {
                    let word: Vec<u64> = (0..word_len)
                        .map(|off| cycle_emissions[(start + off) % cycle_len][p])
                        .collect();
                    allowed_set.insert(word);
                }
            }
        }

        Self {
            width,
            channels: spec.channels,
            sync_order,
            word_len,
            recurrent_period: summary.causal_state_count,
            allowed_words_per_port,
        }
    }

    /// Symbol width $k$.
    #[must_use]
    pub const fn width(&self) -> Width {
        self.width
    }

    /// Number of boundary channels $K$.
    #[must_use]
    pub const fn channels(&self) -> u16 {
        self.channels
    }

    /// Exact clean $\varepsilon$-machine synchronisation order $m$.
    #[must_use]
    pub const fn sync_order(&self) -> usize {
        self.sync_order
    }

    /// Forbidden-word length $m + 1$.
    #[must_use]
    pub const fn word_len(&self) -> usize {
        self.word_len
    }

    /// Minimal causal period $p_{\min}$ on the clean stationary attractor.
    #[must_use]
    pub const fn recurrent_period(&self) -> usize {
        self.recurrent_period
    }

    /// Set of allowed $(m + 1)$-length words on `port`, if `port < self.channels()`.
    #[must_use]
    pub fn allowed_words(&self, port: u16) -> Option<&BTreeSet<Vec<u64>>> {
        self.allowed_words_per_port.get(usize::from(port))
    }

    /// Returns `true` iff `word` is forbidden on `port` (zero probability in the clean generator's
    /// stationary language). Out-of-range ports or words with length `!= self.word_len()` return `true`.
    #[must_use]
    pub fn is_word_forbidden(&self, port: u16, word: &[u64]) -> bool {
        let Some(allowed) = self.allowed_words_per_port.get(usize::from(port)) else {
            return true;
        };
        if word.len() != self.word_len {
            return true;
        }
        let norm: Vec<u64> = word.iter().map(|&sym| self.width.normalize(sym)).collect();
        !allowed.contains(&norm)
    }

    /// Checks cyclic wrap-around $(m + 1)$-words along `orbit_symbols` on `port`, returning the
    /// first forbidden word encountered if any.
    #[must_use]
    pub fn first_forbidden_word_in_orbit(
        &self,
        port: u16,
        orbit_symbols: &[u64],
    ) -> Option<Vec<u64>> {
        if usize::from(port) >= usize::from(self.channels) || orbit_symbols.is_empty() {
            return None;
        }
        let orbit_len = orbit_symbols.len();
        for start in 0..orbit_len {
            let word: Vec<u64> = (0..self.word_len)
                .map(|off| {
                    self.width
                        .normalize(orbit_symbols[(start + off) % orbit_len])
                })
                .collect();
            if self.is_word_forbidden(port, &word) {
                return Some(word);
            }
        }
        None
    }
}

/// Result of free-run loopback attractor detection against [`PortLanguageOracle`] (`R8`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoopbackAttractorReport {
    /// Whether exact `(registers, grounded_inputs)` recurrence occurred within `max_steps`.
    pub closed: bool,
    /// Number of transient loopback steps preceding entry into the recurrent orbit.
    pub transient_steps: usize,
    /// Length of the recurrent loopback orbit (`0` if `!closed`).
    pub orbit_period: usize,
    /// Per-output-port symbol sequence emitted along the closed attractor orbit.
    pub orbit_outputs_per_port: Vec<Vec<u64>>,
    /// Grounded ports whose attractor orbit contains at least one forbidden $(m + 1)$-word.
    pub spurious_ports: Vec<u16>,
    /// First forbidden $(m + 1)$-word detected per spurious grounded port.
    pub first_forbidden_words: Vec<(u16, Vec<u64>)>,
    /// `true` iff `closed` and at least one grounded port exhibits a forbidden word (`!spurious_ports.is_empty()`).
    pub is_spurious_cycle: bool,
}

#[inline]
fn is_channel_grounded(grounded_mask: u64, ch: usize) -> bool {
    ch < 64 && ((grounded_mask & (1u64 << ch)) != 0)
}

/// Steps `compiled` in free-run loopback starting from `initial_registers` and
/// `initial_input_frame`, feeding back grounded output ports `p` where `(grounded_mask >> p) & 1 == 1`
/// until exact `(registers, grounded_inputs)` recurrence (`R8`).
///
/// # Errors
///
/// Returns [`EvalError`] if `initial_registers` mismatches `compiled.total_registers()`.
pub fn detect_loopback_attractor(
    compiled: &CompiledCircuit,
    oracle: &PortLanguageOracle,
    initial_registers: &[u64],
    initial_input_frame: &[u64],
    grounded_mask: u64,
    max_steps: Option<usize>,
) -> Result<LoopbackAttractorReport, EvalError> {
    let width = compiled.width();
    let num_in = usize::from(compiled.num_inputs());
    let num_out = usize::from(compiled.num_outputs());
    let mut state = ScalarState::new(compiled);
    state.set_registers(compiled, initial_registers)?;

    let mut curr_input = vec![0u64; num_in];
    for (dst, &val) in curr_input.iter_mut().zip(initial_input_frame.iter()) {
        *dst = width.normalize(val);
    }

    let step_cap = max_steps
        .unwrap_or(DEFAULT_MAX_LOOPBACK_ATTRACTOR_STEPS)
        .max(1);
    let mut visited: HashMap<(Vec<u64>, Vec<u64>), usize> = HashMap::new();
    let mut history_outputs: Vec<Vec<u64>> = Vec::new();
    let mut out_buf = vec![0u64; num_out];

    for step in 0..step_cap {
        let grounded_inputs: Vec<u64> = (0..num_in)
            .filter(|&c| is_channel_grounded(grounded_mask, c))
            .map(|c| curr_input[c])
            .collect();
        let key = (state.registers().to_vec(), grounded_inputs);
        if let Some(&cycle_start) = visited.get(&key) {
            let orbit_period = step - cycle_start;
            let mut orbit_outputs_per_port = vec![Vec::with_capacity(orbit_period); num_out];
            for frame in &history_outputs[cycle_start..step] {
                for (p, &sym) in frame.iter().enumerate() {
                    orbit_outputs_per_port[p].push(sym);
                }
            }
            let mut spurious_ports = Vec::new();
            let mut first_forbidden_words = Vec::new();
            for (p, orbit) in orbit_outputs_per_port.iter().enumerate() {
                if !is_channel_grounded(grounded_mask, p) {
                    continue;
                }
                let Ok(port_u16) = u16::try_from(p) else {
                    continue;
                };
                if port_u16 >= oracle.channels() {
                    continue;
                }
                if let Some(bad) = oracle.first_forbidden_word_in_orbit(port_u16, orbit) {
                    spurious_ports.push(port_u16);
                    first_forbidden_words.push((port_u16, bad));
                }
            }
            let is_spurious_cycle = !spurious_ports.is_empty();
            return Ok(LoopbackAttractorReport {
                closed: true,
                transient_steps: cycle_start,
                orbit_period,
                orbit_outputs_per_port,
                spurious_ports,
                first_forbidden_words,
                is_spurious_cycle,
            });
        }
        visited.insert(key, step);

        state.step(compiled, &curr_input, &mut out_buf)?;
        for sym in &mut out_buf {
            *sym = width.normalize(*sym);
        }
        history_outputs.push(out_buf.clone());

        for c in 0..num_in.min(num_out) {
            if is_channel_grounded(grounded_mask, c) {
                curr_input[c] = out_buf[c];
            }
        }
    }

    Ok(LoopbackAttractorReport {
        closed: false,
        transient_steps: step_cap,
        orbit_period: 0,
        orbit_outputs_per_port: vec![Vec::new(); num_out],
        spurious_ports: Vec::new(),
        first_forbidden_words: Vec::new(),
        is_spurious_cycle: false,
    })
}

fn collect_backward_cone_nodes(circuit: &Circuit, root_src: SignalSource) -> BTreeSet<NodeId> {
    let mut cone_nodes = BTreeSet::new();
    let mut stack = Vec::new();
    if let SignalSource::NodeOut { node, .. } = root_src {
        stack.push(node);
    }
    while let Some(node) = stack.pop() {
        if !cone_nodes.insert(node) {
            continue;
        }
        for edge in circuit.edges() {
            if let SignalSink::NodeIn { node: dst, .. } = edge.dst
                && dst == node
                && let SignalSource::NodeOut { node: src, .. } = edge.src
            {
                stack.push(src);
            }
        }
    }
    cone_nodes
}

/// Extracts a standalone derivation [`Circuit`] from `root_src` driving `BoundaryOut(target_port)`,
/// preserving `BoundaryIn` ports, `Width`, and `WireDelayMode`.
///
/// Returns `Some((extracted_circuit, orig_to_extracted))` when `target_port < circuit.num_outputs()`
/// and the extracted circuit passes §2.5 invariant compilation, or `None` otherwise.
#[must_use]
pub fn extract_derivation_circuit(
    circuit: &Circuit,
    root_src: SignalSource,
    target_port: u16,
    output_edge_hops: Option<u16>,
) -> Option<(Circuit, Vec<Option<NodeId>>)> {
    if target_port >= circuit.num_outputs() {
        return None;
    }
    let cone_nodes = collect_backward_cone_nodes(circuit, root_src);
    let mut extracted = Circuit::new(
        circuit.width(),
        circuit.num_inputs(),
        circuit.num_outputs(),
        circuit.wire_delay(),
    );
    let mut old_to_new = vec![None; circuit.node_count()];
    for &orig_node in &cone_nodes {
        let prim = circuit.nodes().get(orig_node.index()).copied()?;
        let new_id = extracted.add_node(prim);
        old_to_new[orig_node.index()] = Some(new_id);
    }

    let remap_src = |src: SignalSource| -> Option<SignalSource> {
        match src {
            SignalSource::BoundaryIn(c) => Some(SignalSource::BoundaryIn(c)),
            SignalSource::NodeOut { node, port } => Some(SignalSource::NodeOut {
                node: old_to_new.get(node.index()).copied().flatten()?,
                port,
            }),
        }
    };

    for edge in circuit.edges() {
        if let SignalSink::NodeIn { node: dst, port } = edge.dst
            && cone_nodes.contains(&dst)
        {
            let mapped_dst = SignalSink::NodeIn {
                node: old_to_new[dst.index()]?,
                port,
            };
            let mapped_src = remap_src(edge.src)?;
            extracted.connect_hops(mapped_src, mapped_dst, edge.hops);
        }
    }

    let mapped_root = remap_src(root_src)?;
    let default_hops = match circuit.wire_delay() {
        WireDelayMode::Combinational => 0,
        WireDelayMode::TicksPerHop => 1,
    };
    let out_hops = output_edge_hops.unwrap_or(default_hops);
    extracted.connect_hops(mapped_root, SignalSink::BoundaryOut(target_port), out_hops);
    extracted.compile().ok()?;
    Some((extracted, old_to_new))
}

/// Extracts the standalone derivation cone rooted at `root_src` for `target_port` and certifies it
/// against `spec` using [`certify_circuit_port_against_spec`] (`R8`).
#[must_use]
pub fn certify_derivation_against_spec(
    circuit: &Circuit,
    root_src: SignalSource,
    target_port: u16,
    output_edge_hops: Option<u16>,
    spec: &FiniteStateSpec,
    max_joint_steps: Option<usize>,
) -> Option<usize> {
    let (extracted, _) =
        extract_derivation_circuit(circuit, root_src, target_port, output_edge_hops)?;
    let compiled = extracted.compile().ok()?;
    let steps = max_joint_steps.unwrap_or_else(|| {
        spec.num_states
            .saturating_mul(4)
            .max(DEFAULT_MAX_CERTIFY_JOINT_STEPS)
    });
    certify_circuit_port_against_spec(&compiled, spec, target_port, steps)
}

/// Per-comparator oracle certification and spurious-cycle labels (`R8`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComparatorOracleLabel {
    /// Target output port monitored by this comparator.
    pub target_port: u16,
    /// Comparator `Sub` node id in the evaluated circuit.
    pub sub_node: NodeId,
    /// Comparator `Eqz` node id in the evaluated circuit.
    pub eqz_node: NodeId,
    /// Whether the comparator's `primary` derivation (`sub_node.in(0)`) is certified oracle-correct.
    pub primary_oracle_correct: bool,
    /// Whether the comparator's `alt` derivation (`sub_node.in(1)`) is certified oracle-correct.
    pub alt_oracle_correct: bool,
    /// Whether `alt` exhibits a loopback spurious cycle (`R8`) when all its causal leaves are grounded.
    pub alt_is_spurious_cycle: bool,
}

/// Oracle correctness labels over nodes, ports, and comparators of a circuit (`R8`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CircuitOracleLabels {
    /// Per-node oracle correctness (`true` for nodes in any oracle-correct primary or alt cone;
    /// `false` for comparator glue `(sub_node, eqz_node)` and collapsible comparator fan-out `Split`s).
    pub oracle_correct_nodes: Vec<bool>,
    /// Per-port oracle correctness (`true` iff the port is driven and its primary cone is oracle-correct).
    pub oracle_correct_ports: Vec<bool>,
    /// Per-comparator primary/alt certification and spurious-cycle labels.
    pub comparators: Vec<ComparatorOracleLabel>,
}

fn find_port_driver_edge(circuit: &Circuit, target_port: u16) -> Option<(SignalSource, u16)> {
    circuit
        .edges()
        .iter()
        .find(|e| e.dst == SignalSink::BoundaryOut(target_port))
        .map(|e| (e.src, e.hops))
}

fn find_node_input_edge(circuit: &Circuit, node: NodeId, port: u8) -> Option<(SignalSource, u16)> {
    circuit
        .edges()
        .iter()
        .find(|e| e.dst == SignalSink::NodeIn { node, port })
        .map(|e| (e.src, e.hops))
}

fn build_alt_loopback_test_circuit(
    circuit: &Circuit,
    target_port: u16,
    alt_src: SignalSource,
    alt_hops: u16,
    grounded_mask: u64,
) -> Option<(Circuit, Vec<Option<NodeId>>, u64)> {
    let alt_mask = cone_channel_mask(circuit, alt_src);
    if alt_mask == 0 {
        let (extracted, old_to_new) =
            extract_derivation_circuit(circuit, alt_src, target_port, Some(alt_hops))?;
        let mask = if target_port < 64 {
            1u64 << target_port
        } else {
            0
        };
        return Some((extracted, old_to_new, mask));
    }

    // Reactive grounded alt: retain alt cone for `target_port` plus grounded primary drivers on
    // other ports so loopback feedback matches closed-loop grounded execution.
    let mut keep_nodes = collect_backward_cone_nodes(circuit, alt_src);
    let mut port_drivers: Vec<(u16, SignalSource, u16)> = Vec::new();
    port_drivers.push((target_port, alt_src, alt_hops));

    for p in 0..usize::from(circuit.num_outputs()) {
        let p_u16 = p as u16;
        if p_u16 == target_port || !is_channel_grounded(grounded_mask, p) {
            continue;
        }
        if let Some((drv_src, drv_hops)) = find_port_driver_edge(circuit, p_u16) {
            keep_nodes.extend(collect_backward_cone_nodes(circuit, drv_src));
            port_drivers.push((p_u16, drv_src, drv_hops));
        }
    }

    let mut test_circ = Circuit::new(
        circuit.width(),
        circuit.num_inputs(),
        circuit.num_outputs(),
        circuit.wire_delay(),
    );
    let mut old_to_new = vec![None; circuit.node_count()];
    for &orig in &keep_nodes {
        let prim = circuit.nodes().get(orig.index()).copied()?;
        let new_id = test_circ.add_node(prim);
        old_to_new[orig.index()] = Some(new_id);
    }
    let remap_src = |src: SignalSource| -> Option<SignalSource> {
        match src {
            SignalSource::BoundaryIn(c) => Some(SignalSource::BoundaryIn(c)),
            SignalSource::NodeOut { node, port } => Some(SignalSource::NodeOut {
                node: old_to_new.get(node.index()).copied().flatten()?,
                port,
            }),
        }
    };
    for edge in circuit.edges() {
        if let SignalSink::NodeIn { node: dst, port } = edge.dst
            && keep_nodes.contains(&dst)
        {
            let mapped_dst = SignalSink::NodeIn {
                node: old_to_new[dst.index()]?,
                port,
            };
            let mapped_src = remap_src(edge.src)?;
            test_circ.connect_hops(mapped_src, mapped_dst, edge.hops);
        }
    }
    for (p_u16, drv_src, drv_hops) in port_drivers {
        let mapped_src = remap_src(drv_src)?;
        test_circ.connect_hops(mapped_src, SignalSink::BoundaryOut(p_u16), drv_hops);
    }
    test_circ.compile().ok()?;
    Some((test_circ, old_to_new, grounded_mask))
}

#[allow(clippy::too_many_arguments)]
fn check_alt_spurious_cycle(
    circuit: &Circuit,
    compiled: &CompiledCircuit,
    live_registers: &[u64],
    initial_input_frame: &[u64],
    grounded_mask: u64,
    target_port: u16,
    alt_src: SignalSource,
    alt_hops: u16,
    oracle: &PortLanguageOracle,
) -> bool {
    if usize::from(target_port) >= usize::from(oracle.channels())
        || !is_channel_grounded(grounded_mask, usize::from(target_port))
    {
        return false;
    }
    let alt_mask = cone_channel_mask(circuit, alt_src);
    if (alt_mask & !grounded_mask) != 0 {
        return false;
    }
    let Some((test_circ, old_to_new, test_mask)) =
        build_alt_loopback_test_circuit(circuit, target_port, alt_src, alt_hops, grounded_mask)
    else {
        return false;
    };
    let Ok(test_compiled) = test_circ.compile() else {
        return false;
    };
    let reg_map = register_mapping(circuit, &test_circ, compiled, &test_compiled, &old_to_new);
    let mut test_regs = ScalarState::new(&test_compiled).registers().to_vec();
    for (old_r, mapped_opt) in reg_map.into_iter().enumerate() {
        if let Some(new_r) = mapped_opt
            && old_r < live_registers.len()
            && new_r < test_regs.len()
        {
            test_regs[new_r] = live_registers[old_r];
        }
    }
    match detect_loopback_attractor(
        &test_compiled,
        oracle,
        &test_regs,
        initial_input_frame,
        test_mask,
        None,
    ) {
        Ok(rep) => rep.closed && rep.spurious_ports.contains(&target_port),
        Err(_) => false,
    }
}

/// Computes oracle-correctness labels (`R8`) for `circuit` against clean `spec` and `oracle`.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn label_circuit_oracle(
    circuit: &Circuit,
    compiled: &CompiledCircuit,
    live_registers: &[u64],
    initial_input_frame: &[u64],
    grounded_mask: u64,
    comps: &[ComparatorSite],
    spec: &FiniteStateSpec,
    oracle: &PortLanguageOracle,
) -> CircuitOracleLabels {
    let num_nodes = circuit.node_count();
    let num_out = usize::from(circuit.num_outputs());
    let mut oracle_correct_ports = vec![false; num_out];
    let mut port_drivers: Vec<Option<(SignalSource, u16)>> = vec![None; num_out];

    for p in 0..num_out {
        let p_u16 = p as u16;
        if let Some((drv_src, drv_hops)) = find_port_driver_edge(circuit, p_u16) {
            port_drivers[p] = Some((drv_src, drv_hops));
            oracle_correct_ports[p] = certify_derivation_against_spec(
                circuit,
                drv_src,
                p_u16,
                Some(drv_hops),
                spec,
                None,
            )
            .is_some();
        }
    }

    let mut comp_labels = Vec::with_capacity(comps.len());
    let mut comp_sources: Vec<(Option<SignalSource>, Option<SignalSource>)> =
        Vec::with_capacity(comps.len());

    for comp in comps {
        let out_hops = port_drivers
            .get(usize::from(comp.target_port))
            .and_then(|opt| opt.map(|(_, h)| h));
        let sub0 = find_node_input_edge(circuit, comp.sub_node, 0);
        let sub1 = find_node_input_edge(circuit, comp.sub_node, 1);

        let primary_oracle_correct = sub0.is_some_and(|(src, hops)| {
            certify_derivation_against_spec(
                circuit,
                src,
                comp.target_port,
                out_hops.or(Some(hops)),
                spec,
                None,
            )
            .is_some()
        });
        let alt_oracle_correct = sub1.is_some_and(|(src, hops)| {
            certify_derivation_against_spec(
                circuit,
                src,
                comp.target_port,
                out_hops.or(Some(hops)),
                spec,
                None,
            )
            .is_some()
        });
        let alt_is_spurious_cycle = sub1.is_some_and(|(src, hops)| {
            check_alt_spurious_cycle(
                circuit,
                compiled,
                live_registers,
                initial_input_frame,
                grounded_mask,
                comp.target_port,
                src,
                out_hops.unwrap_or(hops),
                oracle,
            )
        });

        comp_labels.push(ComparatorOracleLabel {
            target_port: comp.target_port,
            sub_node: comp.sub_node,
            eqz_node: comp.eqz_node,
            primary_oracle_correct,
            alt_oracle_correct,
            alt_is_spurious_cycle,
        });
        comp_sources.push((sub0.map(|(s, _)| s), sub1.map(|(s, _)| s)));
    }

    let mut oracle_correct_nodes = vec![false; num_nodes];
    for (p, &correct) in oracle_correct_ports.iter().enumerate() {
        if correct && let Some((drv_src, _)) = port_drivers[p] {
            for node in collect_backward_cone_nodes(circuit, drv_src) {
                if node.index() < num_nodes {
                    oracle_correct_nodes[node.index()] = true;
                }
            }
        }
    }
    for (label, (pri_src_opt, alt_src_opt)) in comp_labels.iter().zip(comp_sources.iter()) {
        if label.primary_oracle_correct
            && let Some(pri_src) = pri_src_opt
        {
            for node in collect_backward_cone_nodes(circuit, *pri_src) {
                if node.index() < num_nodes {
                    oracle_correct_nodes[node.index()] = true;
                }
            }
        }
        if label.alt_oracle_correct
            && let Some(alt_src) = alt_src_opt
        {
            for node in collect_backward_cone_nodes(circuit, *alt_src) {
                if node.index() < num_nodes {
                    oracle_correct_nodes[node.index()] = true;
                }
            }
        }
    }

    // Comparator glue nodes (sub_node, eqz_node) are never marked oracle-correct.
    for comp in comps {
        if comp.sub_node.index() < num_nodes {
            oracle_correct_nodes[comp.sub_node.index()] = false;
        }
        if comp.eqz_node.index() < num_nodes {
            oracle_correct_nodes[comp.eqz_node.index()] = false;
        }
    }

    // Fan-out Split glue that `excise_comparator_branch` collapses when excising an oracle-incorrect
    // arm is comparator/branch plumbing rather than essential computational structure.
    for (comp_idx, (comp, label)) in comps.iter().zip(comp_labels.iter()).enumerate() {
        let target_port_subs: Vec<NodeId> = comps
            .iter()
            .filter(|c| c.target_port == comp.target_port)
            .map(|c| c.sub_node)
            .collect();
        let surviving_comp_nodes: Vec<(NodeId, NodeId)> = comps
            .iter()
            .enumerate()
            .filter_map(|(i, c)| (i != comp_idx).then_some((c.sub_node, c.eqz_node)))
            .collect();
        let prune_primary_opt = if !label.alt_oracle_correct {
            Some(false)
        } else if !label.primary_oracle_correct && label.alt_oracle_correct {
            Some(true)
        } else {
            None
        };
        if let Some(prune_primary) = prune_primary_opt
            && let Some((_, old_to_new)) = excise_comparator_branch(
                circuit,
                comp.sub_node,
                comp.eqz_node,
                comp.target_port,
                prune_primary,
                &target_port_subs,
                &surviving_comp_nodes,
            )
        {
            for (v, mapped) in old_to_new.iter().enumerate() {
                if mapped.is_none()
                    && v < num_nodes
                    && circuit.nodes().get(v) == Some(&Primitive::Split)
                {
                    oracle_correct_nodes[v] = false;
                }
            }
        }
    }

    CircuitOracleLabels {
        oracle_correct_nodes,
        oracle_correct_ports,
        comparators: comp_labels,
    }
}

/// Convenience wrapper computing [`CircuitOracleLabels`] directly from an [`ArkheLearner`]
/// using its live `channel_is_grounded` mask, registers, and `prev_observed_frame`.
#[must_use]
pub fn label_learner_oracle(
    learner: &ArkheLearner,
    spec: &FiniteStateSpec,
    oracle: &PortLanguageOracle,
) -> CircuitOracleLabels {
    let circuit = learner.editable().circuit();
    let compiled = learner.editable().compiled();
    let num_channels = usize::from(circuit.num_inputs().max(circuit.num_outputs())).min(64);
    let mut grounded_mask = 0u64;
    for ch in 0..num_channels {
        if learner.channel_is_grounded(ch) {
            grounded_mask |= 1u64 << ch;
        }
    }
    label_circuit_oracle(
        circuit,
        compiled,
        learner.editable().state().registers(),
        learner.prev_observed_frame(),
        grounded_mask,
        learner.editable().economy().comparators(),
        spec,
        oracle,
    )
}

/// Oracle audit summary across a sleep segment (`R8`, `G4A.6`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SleepOracleAudit {
    /// Count of pre-sleep oracle-correct nodes excised during the segment (`composed_old_to_new[v].is_none()`).
    pub wrongful_excisions: usize,
    /// Pre-sleep `NodeId`s that suffered wrongful excision.
    pub wrongful_excised_nodes: Vec<NodeId>,
    /// Count of ports that were oracle-correct before sleep and lost oracle correctness after sleep.
    pub oracle_correct_port_locks_lost: usize,
    /// Port indices that lost oracle correctness across sleep.
    pub lost_ports: Vec<u16>,
}

/// Audits pre-sleep and post-sleep [`CircuitOracleLabels`] against
/// `SleepSegmentReport::composed_old_to_new` (`R8`, `G4A.6`).
#[must_use]
pub fn audit_sleep_segment_oracle(
    pre: &CircuitOracleLabels,
    post: &CircuitOracleLabels,
    composed_old_to_new: &[Option<NodeId>],
) -> SleepOracleAudit {
    let mut wrongful_excised_nodes = Vec::new();
    if composed_old_to_new.len() == pre.oracle_correct_nodes.len() {
        for (v, (&was_correct, mapped)) in pre
            .oracle_correct_nodes
            .iter()
            .zip(composed_old_to_new.iter())
            .enumerate()
        {
            if was_correct && mapped.is_none() {
                wrongful_excised_nodes.push(NodeId(v as u32));
            }
        }
    }

    let mut lost_ports = Vec::new();
    for (p, &was_locked) in pre.oracle_correct_ports.iter().enumerate() {
        let still_locked = post.oracle_correct_ports.get(p).copied().unwrap_or(false);
        if was_locked && !still_locked {
            lost_ports.push(p as u16);
        }
    }

    SleepOracleAudit {
        wrongful_excisions: wrongful_excised_nodes.len(),
        wrongful_excised_nodes,
        oracle_correct_port_locks_lost: lost_ports.len(),
        lost_ports,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::{FiniteStateGenerator, GeneratorConfig};
    use crate::substrate::Primitive;

    #[test]
    fn port_language_oracle_rejects_wrong_length_and_out_of_bounds_port() {
        let spec = GeneratorConfig::T1_Alternating { a: 0, b: 1 }
            .build(Width::BITS_1)
            .unwrap()
            .to_finite_state_spec()
            .unwrap();
        let oracle = PortLanguageOracle::from_clean_spec(&spec);
        assert_eq!(oracle.word_len(), 2);
        assert!(oracle.is_word_forbidden(0, &[0]));
        assert!(oracle.is_word_forbidden(0, &[0, 1, 0]));
        assert!(oracle.is_word_forbidden(1, &[0, 1]));
        assert_eq!(oracle.first_forbidden_word_in_orbit(0, &[]), None);
        assert_eq!(oracle.first_forbidden_word_in_orbit(1, &[0, 1]), None);
    }

    #[test]
    fn detect_loopback_attractor_reports_non_closure_on_tiny_step_bound_and_mismatch_error() {
        let spec = GeneratorConfig::T1_PeriodicMotif {
            motif: vec![0, 1, 0, 1, 0, 0],
        }
        .build(Width::BITS_1)
        .unwrap()
        .to_finite_state_spec()
        .unwrap();
        let oracle = PortLanguageOracle::from_clean_spec(&spec);

        let mut c = Circuit::new(Width::BITS_1, 1, 1, WireDelayMode::Combinational);
        let (_, _, src) =
            crate::structural::build_autonomous_ring_source(&mut c, 6, &[0, 1, 0, 1, 0, 0]);
        c.connect(src, SignalSink::BoundaryOut(0));
        let compiled = c.compile().unwrap();
        let regs = ScalarState::new(&compiled).registers().to_vec();

        let err = detect_loopback_attractor(&compiled, &oracle, &[], &[0], 0b1, None).unwrap_err();
        assert!(matches!(err, EvalError::RegisterCountMismatch { .. }));

        let rep = detect_loopback_attractor(&compiled, &oracle, &regs, &[0], 0b1, Some(3)).unwrap();
        assert!(!rep.closed);
        assert!(!rep.is_spurious_cycle);
    }

    #[test]
    fn extract_derivation_circuit_rejects_invalid_port_and_preserves_width_and_mode() {
        let mut c = Circuit::new(Width::BITS_2, 2, 2, WireDelayMode::TicksPerHop);
        let d = c.add_node(Primitive::Delay { init: 1 });
        c.connect(
            SignalSource::BoundaryIn(1),
            SignalSink::NodeIn { node: d, port: 0 },
        );
        c.connect(
            SignalSource::NodeOut { node: d, port: 0 },
            SignalSink::BoundaryOut(1),
        );

        assert!(
            extract_derivation_circuit(&c, SignalSource::NodeOut { node: d, port: 0 }, 2, None)
                .is_none()
        );
        let (ext, map) =
            extract_derivation_circuit(&c, SignalSource::NodeOut { node: d, port: 0 }, 1, Some(1))
                .unwrap();
        assert_eq!(ext.width(), Width::BITS_2);
        assert_eq!(ext.wire_delay(), WireDelayMode::TicksPerHop);
        assert_eq!(ext.num_inputs(), 2);
        assert_eq!(ext.num_outputs(), 2);
        assert_eq!(map.len(), 1);
        assert!(map[0].is_some());
    }
}
