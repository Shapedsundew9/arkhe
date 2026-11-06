//! Transactional graph edits that preserve live register phases and node economy.

use std::collections::BTreeSet;
use std::fmt;

use crate::structural::economy::{EconomyConfig, EconomyError, EconomyState};
use crate::structural::lattice::{LatticeCircuit, LatticeError};
use crate::substrate::{
    Circuit, CompiledCircuit, EvalError, NodeId, Primitive, ScalarState, SignalSink, SignalSource,
};

/// Errors returned by a state-preserving lattice edit.
#[derive(Debug)]
pub enum SurgeryError {
    /// The edited lattice could not be built or routed.
    Lattice(LatticeError),
    /// The edited circuit violated a substrate invariant.
    Substrate(crate::substrate::InvariantError),
    /// Node or register economy state could not be remapped.
    Economy(EconomyError),
    /// The new scalar state rejected its preserved register values.
    Evaluation(EvalError),
    /// A node map was not a partial one-to-one mapping into the edited circuit.
    InvalidNodeMapping,
}

impl fmt::Display for SurgeryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lattice(error) => write!(f, "lattice surgery failed: {error}"),
            Self::Substrate(error) => write!(f, "surgery produced an invalid circuit: {error}"),
            Self::Economy(error) => write!(f, "surgery economy remap failed: {error}"),
            Self::Evaluation(error) => write!(f, "surgery register remap failed: {error}"),
            Self::InvalidNodeMapping => write!(f, "invalid old-to-new node mapping"),
        }
    }
}

impl std::error::Error for SurgeryError {}

impl From<LatticeError> for SurgeryError {
    fn from(value: LatticeError) -> Self {
        Self::Lattice(value)
    }
}

impl From<crate::substrate::InvariantError> for SurgeryError {
    fn from(value: crate::substrate::InvariantError) -> Self {
        Self::Substrate(value)
    }
}

impl From<EconomyError> for SurgeryError {
    fn from(value: EconomyError) -> Self {
        Self::Economy(value)
    }
}

impl From<EvalError> for SurgeryError {
    fn from(value: EvalError) -> Self {
        Self::Evaluation(value)
    }
}

/// A live lattice together with the compiled circuit, synchronous phase, and metabolic state.
#[derive(Clone, Debug)]
pub struct EditableLattice {
    lattice: LatticeCircuit,
    circuit: Circuit,
    compiled: CompiledCircuit,
    state: ScalarState,
    economy: EconomyState,
}

impl EditableLattice {
    /// Builds an editable circuit with fresh scalar and metabolic state.
    pub fn new(
        lattice: LatticeCircuit,
        economy_config: EconomyConfig,
    ) -> Result<Self, SurgeryError> {
        let circuit = lattice.lower()?;
        let compiled = circuit.compile()?;
        let state = ScalarState::new(&compiled);
        let economy = EconomyState::new(
            economy_config,
            circuit.width(),
            circuit.node_count(),
            usize::from(circuit.num_outputs()),
            compiled.total_registers(),
        )?;
        Ok(Self {
            lattice,
            circuit,
            compiled,
            state,
            economy,
        })
    }

    /// Current spatial graph.
    #[must_use]
    pub const fn lattice(&self) -> &LatticeCircuit {
        &self.lattice
    }

    /// Compiled execution plan for the current graph.
    #[must_use]
    pub const fn compiled(&self) -> &CompiledCircuit {
        &self.compiled
    }

    /// Live synchronous state.
    #[must_use]
    pub const fn state(&self) -> &ScalarState {
        &self.state
    }

    /// Mutable live synchronous state.
    pub fn state_mut(&mut self) -> &mut ScalarState {
        &mut self.state
    }

    /// Current node reserves, null models, and register ancestry.
    #[must_use]
    pub const fn economy(&self) -> &EconomyState {
        &self.economy
    }

    /// Mutable node economy for online tick accounting.
    pub fn economy_mut(&mut self) -> &mut EconomyState {
        &mut self.economy
    }

    /// Applies a graph edit and installs the supplied exact old-to-new node mapping.
    ///
    /// The edit is transactional: any routing, compilation, or remapping failure leaves this
    /// `EditableLattice` unchanged. Nodes omitted from the map are treated as new.
    pub fn edit<F>(&mut self, edit: F) -> Result<Vec<Option<NodeId>>, SurgeryError>
    where
        F: FnOnce(&mut LatticeCircuit) -> Result<Vec<Option<NodeId>>, LatticeError>,
    {
        let mut staged = self.lattice.clone();
        let old_to_new = edit(&mut staged)?;
        self.recompile_preserving_state(staged, &old_to_new)?;
        Ok(old_to_new)
    }

    /// Recompiles `lattice`, preserving state for each explicitly mapped surviving node and edge.
    pub fn recompile_preserving_state(
        &mut self,
        lattice: LatticeCircuit,
        old_to_new_nodes: &[Option<NodeId>],
    ) -> Result<(), SurgeryError> {
        if old_to_new_nodes.len() != self.circuit.node_count() {
            return Err(SurgeryError::InvalidNodeMapping);
        }
        let circuit = lattice.lower()?;
        let mut seen = vec![false; circuit.node_count()];
        for &mapped in old_to_new_nodes.iter().flatten() {
            let Some(slot) = seen.get_mut(mapped.index()) else {
                return Err(SurgeryError::InvalidNodeMapping);
            };
            if *slot {
                return Err(SurgeryError::InvalidNodeMapping);
            }
            *slot = true;
        }
        let compiled = circuit.compile()?;
        let register_mapping = register_mapping(
            &self.circuit,
            &circuit,
            &self.compiled,
            &compiled,
            old_to_new_nodes,
        );
        let mut registers = ScalarState::new(&compiled).registers().to_vec();
        for (old, mapped) in register_mapping.iter().enumerate() {
            if let Some(new) = mapped {
                registers[*new] = self.state.registers()[old];
            }
        }
        let mut state = ScalarState::new(&compiled);
        state.set_registers(&compiled, &registers)?;
        let node_mapping: Vec<Option<usize>> = old_to_new_nodes
            .iter()
            .map(|mapped| mapped.map(NodeId::index))
            .collect();
        let economy = self.economy.remap_structure(
            &node_mapping,
            circuit.node_count(),
            &register_mapping,
            compiled.total_registers(),
        )?;

        self.lattice = lattice;
        self.circuit = circuit;
        self.compiled = compiled;
        self.state = state;
        self.economy = economy;
        Ok(())
    }

    /// Routes a new edge, adding deterministic `Split` relays when required.
    pub fn route_connect(
        &mut self,
        src: SignalSource,
        dst: SignalSink,
        max_relay_hops: usize,
    ) -> Result<Vec<NodeId>, SurgeryError> {
        let mut relays = Vec::new();
        self.edit(|lattice| {
            let old_node_count = lattice.nodes().len();
            relays = lattice.route_connect(src, dst, max_relay_hops)?;
            Ok((0..old_node_count)
                .map(|index| {
                    Some(NodeId(
                        u32::try_from(index).expect("node count fits in u32"),
                    ))
                })
                .collect())
        })?;
        Ok(relays)
    }

    /// Excises mature depleted nodes, collapses single-use `Split` nodes, and bypasses inactive
    /// zero-valued correctors. Returns the old-to-new mapping for the pre-prune nodes.
    pub fn prune_starved_nodes(&mut self) -> Result<Vec<Option<NodeId>>, SurgeryError> {
        let mut removed: BTreeSet<NodeId> = self
            .economy
            .nodes()
            .iter()
            .enumerate()
            .filter(|(_, node)| node.juvenile_ticks == 0 && node.energy <= 0.0)
            .map(|(index, _)| NodeId(u32::try_from(index).expect("node count fits in u32")))
            .collect();
        if removed.is_empty() {
            return Ok((0..self.circuit.node_count())
                .map(|index| {
                    Some(NodeId(
                        u32::try_from(index).expect("node count fits in u32"),
                    ))
                })
                .collect());
        }

        let mut logical_edges = self.lattice.edges().to_vec();
        let mut bypasses = Vec::new();
        let initially_starved: Vec<NodeId> = removed.iter().copied().collect();
        let mut inactive_zero_nodes: BTreeSet<NodeId> = initially_starved
            .iter()
            .copied()
            .filter(|&node| {
                matches!(
                    self.lattice.node_primitive(node),
                    Some(Primitive::Delay { init: 0 })
                ) && !self.economy.nodes()[node.index()].activity.ever_nonzero
            })
            .collect();
        loop {
            let mut propagated = false;
            for (index, &(_, primitive)) in self.lattice.nodes().iter().enumerate() {
                let node = NodeId(u32::try_from(index).expect("node count fits in u32"));
                if primitive == Primitive::Split
                    && input_source(&logical_edges, node, 0)
                        .and_then(source_node)
                        .is_some_and(|source| inactive_zero_nodes.contains(&source))
                {
                    propagated |= inactive_zero_nodes.insert(node);
                }
            }
            if !propagated {
                break;
            }
        }
        for zero_node in inactive_zero_nodes.iter().copied() {
            let consumers: Vec<(NodeId, u8)> = logical_edges
                .iter()
                .filter_map(|(source, sink)| {
                    if let (
                        SignalSource::NodeOut { node, .. },
                        SignalSink::NodeIn {
                            node: consumer,
                            port,
                        },
                    ) = (*source, *sink)
                        && node == zero_node
                    {
                        Some((consumer, port))
                    } else {
                        None
                    }
                })
                .collect();
            for (consumer, port) in consumers {
                if removed.contains(&consumer) {
                    continue;
                }
                let Some(primitive) = self.lattice.node_primitive(consumer) else {
                    continue;
                };
                let other_port = match (primitive, port) {
                    (Primitive::Add, 0) => Some(1),
                    (Primitive::Add, 1) | (Primitive::Sub, 1) => Some(0),
                    _ => None,
                };
                let Some(other_port) = other_port else {
                    continue;
                };
                let Some(other_source) = input_source(&logical_edges, consumer, other_port) else {
                    continue;
                };
                if source_node(other_source).is_some_and(|node| {
                    removed.contains(&node) || inactive_zero_nodes.contains(&node)
                }) {
                    continue;
                }
                let outputs = output_sinks(&logical_edges, consumer);
                if outputs.len() != 1 {
                    continue;
                }
                let output = outputs[0];
                removed.insert(consumer);
                logical_edges
                    .retain(|(source, sink)| !edge_touches_nodes(*source, *sink, &removed));
                logical_edges.push((other_source, output));
                bypasses.push((other_source, output));
            }
        }

        loop {
            logical_edges.retain(|(source, sink)| !edge_touches_nodes(*source, *sink, &removed));
            let mut changed = false;
            for (index, &(_, primitive)) in self.lattice.nodes().iter().enumerate() {
                let node = NodeId(u32::try_from(index).expect("node count fits in u32"));
                if removed.contains(&node) {
                    continue;
                }
                if (0..primitive.input_arity())
                    .any(|port| input_source(&logical_edges, node, port).is_none())
                {
                    removed.insert(node);
                    changed = true;
                }
            }
            if changed {
                continue;
            }
            for (index, &(_, primitive)) in self.lattice.nodes().iter().enumerate() {
                let node = NodeId(u32::try_from(index).expect("node count fits in u32"));
                if removed.contains(&node) || primitive != Primitive::Split {
                    continue;
                }
                let outputs = output_sinks(&logical_edges, node);
                if outputs.len() > 1 {
                    continue;
                }
                if let Some(output) = outputs.first().copied()
                    && let Some(input) = input_source(&logical_edges, node, 0)
                    && !source_node(input).is_some_and(|source| removed.contains(&source))
                {
                    logical_edges.retain(|(source, sink)| {
                        !matches!(source, SignalSource::NodeOut { node: current, .. } if *current == node)
                            && !matches!(sink, SignalSink::NodeIn { node: current, .. } if *current == node)
                    });
                    logical_edges.push((input, output));
                    bypasses.push((input, output));
                }
                removed.insert(node);
                changed = true;
                break;
            }
            if !changed {
                break;
            }
        }

        let mut staged = self.lattice.clone();
        let old_to_new = staged.remove_nodes(&removed);
        for (source, sink) in bypasses {
            let Some(source) = remap_source(source, &old_to_new) else {
                continue;
            };
            let Some(sink) = remap_sink(sink, &old_to_new) else {
                continue;
            };
            staged.route_connect(source, sink, 4)?;
        }
        self.recompile_preserving_state(staged, &old_to_new)?;
        Ok(old_to_new)
    }
}

fn input_source(
    edges: &[(SignalSource, SignalSink)],
    node: NodeId,
    port: u8,
) -> Option<SignalSource> {
    edges
        .iter()
        .find_map(|&(source, sink)| (sink == SignalSink::NodeIn { node, port }).then_some(source))
}

fn output_sinks(edges: &[(SignalSource, SignalSink)], node: NodeId) -> Vec<SignalSink> {
    edges
        .iter()
        .filter_map(|&(source, sink)| {
            matches!(source, SignalSource::NodeOut { node: source_node, .. } if source_node == node)
                .then_some(sink)
        })
        .collect()
}

fn source_node(source: SignalSource) -> Option<NodeId> {
    match source {
        SignalSource::BoundaryIn(_) => None,
        SignalSource::NodeOut { node, .. } => Some(node),
    }
}

fn edge_touches_nodes(source: SignalSource, sink: SignalSink, removed: &BTreeSet<NodeId>) -> bool {
    source_node(source).is_some_and(|node| removed.contains(&node))
        || matches!(sink, SignalSink::NodeIn { node, .. } if removed.contains(&node))
}

fn remap_source(source: SignalSource, mapping: &[Option<NodeId>]) -> Option<SignalSource> {
    match source {
        SignalSource::BoundaryIn(_) => Some(source),
        SignalSource::NodeOut { node, port } => Some(SignalSource::NodeOut {
            node: mapping.get(node.index()).copied().flatten()?,
            port,
        }),
    }
}

fn remap_sink(sink: SignalSink, mapping: &[Option<NodeId>]) -> Option<SignalSink> {
    match sink {
        SignalSink::BoundaryOut(_) => Some(sink),
        SignalSink::NodeIn { node, port } => Some(SignalSink::NodeIn {
            node: mapping.get(node.index()).copied().flatten()?,
            port,
        }),
    }
}

fn register_mapping(
    old: &Circuit,
    new: &Circuit,
    old_compiled: &CompiledCircuit,
    new_compiled: &CompiledCircuit,
    old_to_new_nodes: &[Option<NodeId>],
) -> Vec<Option<usize>> {
    let mut mapping = vec![None; old_compiled.total_registers()];
    let mut old_delay_register = 0usize;
    let mut new_delay_registers = vec![None; new.node_count()];
    let mut next_new_delay_register = 0usize;
    for (index, primitive) in new.nodes().iter().enumerate() {
        if primitive.is_stateful() {
            new_delay_registers[index] = Some(next_new_delay_register);
            next_new_delay_register += 1;
        }
    }
    for (old_index, primitive) in old.nodes().iter().enumerate() {
        if !primitive.is_stateful() {
            continue;
        }
        if let Some(new_node) = old_to_new_nodes[old_index]
            && new
                .nodes()
                .get(new_node.index())
                .is_some_and(|primitive| primitive.is_stateful())
        {
            mapping[old_delay_register] = new_delay_registers[new_node.index()];
        }
        old_delay_register += 1;
    }

    if old.wire_delay() != crate::substrate::WireDelayMode::TicksPerHop {
        return mapping;
    }
    let old_bases = edge_register_bases(old, old_compiled.explicit_delay_count());
    let new_bases = edge_register_bases(new, new_compiled.explicit_delay_count());
    for (old_index, edge) in old.edges().iter().enumerate() {
        let Some(source) = remap_source(edge.src, old_to_new_nodes) else {
            continue;
        };
        let Some(sink) = remap_sink(edge.dst, old_to_new_nodes) else {
            continue;
        };
        let Some(new_index) = new.edges().iter().position(|candidate| {
            candidate.src == source && candidate.dst == sink && candidate.hops == edge.hops
        }) else {
            continue;
        };
        let old_base = old_bases[old_index];
        let new_base = new_bases[new_index];
        for stage in 0..usize::from(edge.hops) {
            mapping[old_base + stage] = Some(new_base + stage);
        }
    }
    mapping
}

fn edge_register_bases(circuit: &Circuit, first_register: usize) -> Vec<usize> {
    let mut next = first_register;
    circuit
        .edges()
        .iter()
        .map(|edge| {
            let base = next;
            if circuit.wire_delay() == crate::substrate::WireDelayMode::TicksPerHop {
                next += usize::from(edge.hops);
            }
            base
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structural::lattice::{Coord, GridShape, WiringTopology};
    use crate::substrate::{Width, WireDelayMode};

    fn oscillator_lattice() -> (LatticeCircuit, [NodeId; 5]) {
        let mut lattice = LatticeCircuit::new(
            GridShape::new(1, 5).unwrap(),
            Width::BITS_2,
            WireDelayMode::TicksPerHop,
            WiringTopology::WellMixed,
            vec![],
            vec![
                crate::structural::lattice::BoundaryAnchor {
                    side: crate::structural::lattice::PerimeterSide::South,
                    offset: 0,
                },
                crate::structural::lattice::BoundaryAnchor {
                    side: crate::structural::lattice::PerimeterSide::South,
                    offset: 4,
                },
            ],
        )
        .unwrap();
        let delay0 = lattice
            .place_node(Coord { row: 0, col: 0 }, Primitive::delay_zero())
            .unwrap();
        let split0 = lattice
            .place_node(Coord { row: 0, col: 1 }, Primitive::Split)
            .unwrap();
        let removable = lattice
            .place_node(Coord { row: 0, col: 2 }, Primitive::Eqz)
            .unwrap();
        let delay1 = lattice
            .place_node(Coord { row: 0, col: 3 }, Primitive::delay_zero())
            .unwrap();
        let split1 = lattice
            .place_node(Coord { row: 0, col: 4 }, Primitive::Split)
            .unwrap();
        for (source, sink) in [
            (
                SignalSource::NodeOut {
                    node: delay0,
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: split0,
                    port: 0,
                },
            ),
            (
                SignalSource::NodeOut {
                    node: split0,
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: delay0,
                    port: 0,
                },
            ),
            (
                SignalSource::NodeOut {
                    node: split0,
                    port: 1,
                },
                SignalSink::BoundaryOut(0),
            ),
            (
                SignalSource::NodeOut {
                    node: delay1,
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: removable,
                    port: 0,
                },
            ),
            (
                SignalSource::NodeOut {
                    node: removable,
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: split1,
                    port: 0,
                },
            ),
            (
                SignalSource::NodeOut {
                    node: split1,
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: delay1,
                    port: 0,
                },
            ),
            (
                SignalSource::NodeOut {
                    node: split1,
                    port: 1,
                },
                SignalSink::BoundaryOut(1),
            ),
        ] {
            lattice.connect(source, sink).unwrap();
        }
        (lattice, [delay0, split0, removable, delay1, split1])
    }

    #[test]
    fn recompilation_preserves_delay_phase_and_unchanged_hop_registers() {
        let (lattice, nodes) = oscillator_lattice();
        let mut editable = EditableLattice::new(lattice, EconomyConfig::default()).unwrap();
        let compiled = editable.compiled().clone();
        editable
            .state_mut()
            .set_registers(&compiled, &[1, 2, 3, 0, 1, 2, 3, 0, 1])
            .unwrap();

        let mapping = editable
            .edit(|lattice| {
                let mapping = lattice.remove_nodes(&BTreeSet::from([nodes[2]]));
                let source = mapping[nodes[3].index()].unwrap();
                let sink = mapping[nodes[4].index()].unwrap();
                lattice.route_connect(
                    SignalSource::NodeOut {
                        node: source,
                        port: 0,
                    },
                    SignalSink::NodeIn {
                        node: sink,
                        port: 0,
                    },
                    4,
                )?;
                Ok(mapping)
            })
            .unwrap();

        assert_eq!(mapping[nodes[2].index()], None);
        assert_eq!(editable.state().registers(), &[1, 2, 3, 0, 1, 0, 1, 0]);
        assert!(editable.lattice().lower().is_ok());
    }

    #[test]
    fn pruning_zero_corrector_bypasses_add_and_preserves_surviving_state() {
        let mut lattice = LatticeCircuit::new(
            GridShape::new(1, 5).unwrap(),
            Width::BITS_1,
            WireDelayMode::Combinational,
            WiringTopology::WellMixed,
            vec![],
            vec![crate::structural::lattice::BoundaryAnchor {
                side: crate::structural::lattice::PerimeterSide::South,
                offset: 2,
            }],
        )
        .unwrap();
        let base_delay = lattice
            .place_node(Coord { row: 0, col: 0 }, Primitive::delay_zero())
            .unwrap();
        let base_split = lattice
            .place_node(Coord { row: 0, col: 1 }, Primitive::Split)
            .unwrap();
        let zero_delay = lattice
            .place_node(Coord { row: 0, col: 2 }, Primitive::delay_zero())
            .unwrap();
        let zero_split = lattice
            .place_node(Coord { row: 0, col: 3 }, Primitive::Split)
            .unwrap();
        let corrector_add = lattice
            .place_node(Coord { row: 0, col: 4 }, Primitive::Add)
            .unwrap();
        for (source, sink) in [
            (
                SignalSource::NodeOut {
                    node: base_delay,
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: base_split,
                    port: 0,
                },
            ),
            (
                SignalSource::NodeOut {
                    node: base_split,
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: base_delay,
                    port: 0,
                },
            ),
            (
                SignalSource::NodeOut {
                    node: zero_delay,
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: zero_split,
                    port: 0,
                },
            ),
            (
                SignalSource::NodeOut {
                    node: zero_split,
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: zero_delay,
                    port: 0,
                },
            ),
            (
                SignalSource::NodeOut {
                    node: base_split,
                    port: 1,
                },
                SignalSink::NodeIn {
                    node: corrector_add,
                    port: 0,
                },
            ),
            (
                SignalSource::NodeOut {
                    node: zero_split,
                    port: 1,
                },
                SignalSink::NodeIn {
                    node: corrector_add,
                    port: 1,
                },
            ),
            (
                SignalSource::NodeOut {
                    node: corrector_add,
                    port: 0,
                },
                SignalSink::BoundaryOut(0),
            ),
        ] {
            lattice.connect(source, sink).unwrap();
        }
        let economy_config = EconomyConfig {
            initial_energy: 0.1,
            juvenile_ticks: 0,
            ..EconomyConfig::default()
        };
        let mut editable = EditableLattice::new(lattice, economy_config).unwrap();
        let compiled = editable.compiled().clone();
        editable
            .state_mut()
            .set_registers(&compiled, &[1, 0])
            .unwrap();
        editable
            .economy_mut()
            .apply_tick(&[0.0], &[vec![0.0; 5]], &[0.0, 0.0, 0.2, 0.0, 0.0])
            .unwrap();

        let mapping = editable.prune_starved_nodes().unwrap();

        assert_eq!(mapping[zero_delay.index()], None);
        assert_eq!(mapping[zero_split.index()], None);
        assert_eq!(mapping[corrector_add.index()], None);
        assert_eq!(editable.state().registers(), &[1]);
        let mut output = [0];
        let compiled = editable.compiled().clone();
        editable
            .state_mut()
            .step(&compiled, &[], &mut output)
            .unwrap();
        assert_eq!(output, [1], "edges: {:?}", editable.lattice().edges());
    }
}
