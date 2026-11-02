//! Flat topological execution schedules: scalar (`1..=64` bits) and 64-lane bit-parallel ($k = 1$).
//!
//! Implements Charter §2.5 synchronous tick semantics and §9 evaluation backends with zero
//! heap allocations during stepping.

use std::fmt;

use crate::substrate::value::Width;

/// Error returned by evaluation backends when state/input dimensions or widths mismatch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvalError {
    /// Attempted to construct [`BitParallel64State`] for a circuit with `width.bits() != 1`.
    UnsupportedBitParallelWidth { actual_bits: u8 },
    /// Provided initial register slice length did not match `compiled.total_registers()`.
    RegisterCountMismatch { expected: usize, actual: usize },
    /// Provided input or output slice length did not match boundary port count.
    PortSliceMismatch {
        kind: &'static str,
        expected: usize,
        actual: usize,
    },
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedBitParallelWidth { actual_bits } => write!(
                f,
                "BitParallel64State requires width k == 1, got k == {actual_bits}"
            ),
            Self::RegisterCountMismatch { expected, actual } => write!(
                f,
                "register slice length mismatch: expected {expected}, got {actual}"
            ),
            Self::PortSliceMismatch {
                kind,
                expected,
                actual,
            } => write!(
                f,
                "{kind} port slice length mismatch: expected {expected}, got {actual}"
            ),
        }
    }
}

impl std::error::Error for EvalError {}

/// Initialisation descriptor for a synchronous state register.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RegInit {
    pub(crate) initial_value: u64,
    pub(crate) is_explicit_delay: bool,
}

/// Multi-hop synchronous pipeline register chain for [`crate::substrate::circuit::WireDelayMode::TicksPerHop`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ShiftChain {
    pub(crate) src_wire: usize,
    pub(crate) base_reg: usize,
    pub(crate) hops: usize,
}

/// Single topologically ordered combinational instruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CombOp {
    Add {
        lhs: usize,
        rhs: usize,
        out: usize,
    },
    Sub {
        lhs: usize,
        rhs: usize,
        out: usize,
    },
    Mul {
        lhs: usize,
        rhs: usize,
        out: usize,
    },
    Eqz {
        arg: usize,
        out: usize,
    },
    Split {
        arg: usize,
        out0: usize,
        out1: usize,
    },
}

/// Immutable, validated, topologically sorted execution plan for a [`crate::substrate::circuit::Circuit`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledCircuit {
    width: Width,
    num_inputs: u16,
    num_outputs: u16,
    active_boundary_inputs: u16,
    num_wires: usize,
    reg_inits: Vec<RegInit>,
    explicit_delay_count: usize,
    delay_reads: Vec<(usize, usize)>,
    comb_ops: Vec<CombOp>,
    delay_latches: Vec<(usize, usize)>,
    shift_chains: Vec<ShiftChain>,
    output_drivers: Vec<Option<usize>>,
}

impl CompiledCircuit {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        width: Width,
        num_inputs: u16,
        num_outputs: u16,
        active_boundary_inputs: u16,
        num_wires: usize,
        reg_inits: Vec<RegInit>,
        delay_reads: Vec<(usize, usize)>,
        comb_ops: Vec<CombOp>,
        delay_latches: Vec<(usize, usize)>,
        shift_chains: Vec<ShiftChain>,
        output_drivers: Vec<Option<usize>>,
    ) -> Self {
        let explicit_delay_count = reg_inits.iter().filter(|r| r.is_explicit_delay).count();
        Self {
            width,
            num_inputs,
            num_outputs,
            active_boundary_inputs,
            num_wires,
            reg_inits,
            explicit_delay_count,
            delay_reads,
            comb_ops,
            delay_latches,
            shift_chains,
            output_drivers,
        }
    }

    /// Uniform wire bit-width $k$.
    #[must_use]
    pub const fn width(&self) -> Width {
        self.width
    }

    /// Number of boundary input channels.
    #[must_use]
    pub const fn num_inputs(&self) -> u16 {
        self.num_inputs
    }

    /// Number of boundary output ports.
    #[must_use]
    pub const fn num_outputs(&self) -> u16 {
        self.num_outputs
    }

    /// Number of `BoundaryIn(c)` channels with a directed path to any output or state register.
    #[must_use]
    pub const fn active_boundary_input_count(&self) -> u16 {
        self.active_boundary_inputs
    }

    /// Returns `true` if at least one `BoundaryIn` channel reaches an output or state register.
    #[must_use]
    pub const fn uses_boundary_inputs(&self) -> bool {
        self.active_boundary_inputs > 0
    }

    /// Total number of synchronous state registers (`DELAY` primitives + wire-hop stages).
    #[must_use]
    pub fn total_registers(&self) -> usize {
        self.reg_inits.len()
    }

    /// Number of explicit `DELAY` registers (stored at indices `0..explicit_delay_count()`).
    #[must_use]
    pub const fn explicit_delay_count(&self) -> usize {
        self.explicit_delay_count
    }

    /// Number of combinational instructions executed per tick.
    #[must_use]
    pub fn comb_op_count(&self) -> usize {
        self.comb_ops.len()
    }
}

/// Scalar execution state supporting any uniform width $1 \le k \le 64$.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScalarState {
    regs: Vec<u64>,
    wires: Vec<u64>,
}

impl ScalarState {
    /// Allocates and initialises a [`ScalarState`] from `compiled`'s declared register defaults.
    #[must_use]
    pub fn new(compiled: &CompiledCircuit) -> Self {
        let regs = compiled.reg_inits.iter().map(|r| r.initial_value).collect();
        let wires = vec![0u64; compiled.num_wires];
        Self { regs, wires }
    }

    /// Resets all state registers to the circuit's declared initial values.
    pub fn reset(&mut self, compiled: &CompiledCircuit) {
        assert_eq!(self.regs.len(), compiled.reg_inits.len());
        for (slot, init) in self.regs.iter_mut().zip(&compiled.reg_inits) {
            *slot = init.initial_value;
        }
        self.wires.fill(0);
    }

    /// Read-only view of all synchronous state registers (`0..total_registers()`).
    #[must_use]
    pub fn registers(&self) -> &[u64] {
        &self.regs
    }

    /// Overwrites all synchronous state registers, masking each value to `compiled.width()`.
    ///
    /// # Errors
    ///
    /// Returns [`EvalError::RegisterCountMismatch`] if `values.len() != compiled.total_registers()`.
    pub fn set_registers(
        &mut self,
        compiled: &CompiledCircuit,
        values: &[u64],
    ) -> Result<(), EvalError> {
        if values.len() != self.regs.len() {
            return Err(EvalError::RegisterCountMismatch {
                expected: self.regs.len(),
                actual: values.len(),
            });
        }
        let w = compiled.width;
        for (dst, &val) in self.regs.iter_mut().zip(values) {
            *dst = w.normalize(val);
        }
        Ok(())
    }

    /// Advances the circuit by one synchronous tick in $O(N)$ with zero heap allocations.
    ///
    /// # Errors
    ///
    /// Returns [`EvalError::PortSliceMismatch`] if `inputs` or `outputs` lengths mismatch.
    pub fn step(
        &mut self,
        compiled: &CompiledCircuit,
        inputs: &[u64],
        outputs: &mut [u64],
    ) -> Result<(), EvalError> {
        let expected_in = usize::from(compiled.num_inputs);
        let expected_out = usize::from(compiled.num_outputs);
        if inputs.len() != expected_in {
            return Err(EvalError::PortSliceMismatch {
                kind: "input",
                expected: expected_in,
                actual: inputs.len(),
            });
        }
        if outputs.len() != expected_out {
            return Err(EvalError::PortSliceMismatch {
                kind: "output",
                expected: expected_out,
                actual: outputs.len(),
            });
        }

        let w = compiled.width;

        // Phase 1: Load boundary inputs and current synchronous register outputs.
        for (i, &inp) in inputs.iter().enumerate() {
            self.wires[i] = w.normalize(inp);
        }
        for &(reg_idx, wire_idx) in &compiled.delay_reads {
            self.wires[wire_idx] = self.regs[reg_idx];
        }

        // Phase 2: Combinational topological pass.
        for &op in &compiled.comb_ops {
            match op {
                CombOp::Add { lhs, rhs, out } => {
                    self.wires[out] = w.add(self.wires[lhs], self.wires[rhs]);
                }
                CombOp::Sub { lhs, rhs, out } => {
                    self.wires[out] = w.sub(self.wires[lhs], self.wires[rhs]);
                }
                CombOp::Mul { lhs, rhs, out } => {
                    self.wires[out] = w.mul(self.wires[lhs], self.wires[rhs]);
                }
                CombOp::Eqz { arg, out } => {
                    self.wires[out] = w.eqz(self.wires[arg]);
                }
                CombOp::Split { arg, out0, out1 } => {
                    let v = self.wires[arg];
                    self.wires[out0] = v;
                    self.wires[out1] = v;
                }
            }
        }

        // Phase 3a: Emit boundary predictions.
        for (out_slot, &driver) in outputs.iter_mut().zip(&compiled.output_drivers) {
            *out_slot = driver.map_or(0, |wire_idx| self.wires[wire_idx]);
        }

        // Phase 3b: Synchronously latch all state registers in-place.
        for &(reg_idx, in_wire) in &compiled.delay_latches {
            self.regs[reg_idx] = self.wires[in_wire];
        }
        for chain in &compiled.shift_chains {
            for stage in (1..chain.hops).rev() {
                self.regs[chain.base_reg + stage] = self.regs[chain.base_reg + stage - 1];
            }
            self.regs[chain.base_reg] = self.wires[chain.src_wire];
        }

        Ok(())
    }
}

/// 64-lane bit-parallel execution state for $k = 1$ circuits (Charter §9).
///
/// Bit $b \in \{0, \dots, 63\}$ of each `u64` word represents an independent Boolean trajectory
/// of the same fixed [`CompiledCircuit`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BitParallel64State {
    regs: Vec<u64>,
    wires: Vec<u64>,
}

impl BitParallel64State {
    /// Allocates and initialises all 64 lanes to `compiled`'s declared register defaults.
    ///
    /// # Errors
    ///
    /// Returns [`EvalError::UnsupportedBitParallelWidth`] if `compiled.width().bits() != 1`.
    pub fn new(compiled: &CompiledCircuit) -> Result<Self, EvalError> {
        if compiled.width.bits() != 1 {
            return Err(EvalError::UnsupportedBitParallelWidth {
                actual_bits: compiled.width.bits(),
            });
        }
        let regs = compiled
            .reg_inits
            .iter()
            .map(|r| if r.initial_value == 0 { 0u64 } else { u64::MAX })
            .collect();
        let wires = vec![0u64; compiled.num_wires];
        Ok(Self { regs, wires })
    }

    /// Resets all 64 lanes to `compiled`'s declared register defaults.
    pub fn reset(&mut self, compiled: &CompiledCircuit) {
        assert_eq!(self.regs.len(), compiled.reg_inits.len());
        for (slot, init) in self.regs.iter_mut().zip(&compiled.reg_inits) {
            *slot = if init.initial_value == 0 {
                0u64
            } else {
                u64::MAX
            };
        }
        self.wires.fill(0);
    }

    /// Read-only view of packed 64-lane register words.
    #[must_use]
    pub fn registers(&self) -> &[u64] {
        &self.regs
    }

    /// Sets packed 64-lane register words directly (bit $b$ = initial state of lane $b$).
    ///
    /// # Errors
    ///
    /// Returns [`EvalError::RegisterCountMismatch`] if `packed_regs.len() != total_registers()`.
    pub fn set_packed_registers(&mut self, packed_regs: &[u64]) -> Result<(), EvalError> {
        if packed_regs.len() != self.regs.len() {
            return Err(EvalError::RegisterCountMismatch {
                expected: self.regs.len(),
                actual: packed_regs.len(),
            });
        }
        self.regs.copy_from_slice(packed_regs);
        Ok(())
    }

    /// Advances all 64 independent Boolean trajectories simultaneously by one tick.
    ///
    /// # Errors
    ///
    /// Returns [`EvalError::PortSliceMismatch`] if `packed_inputs` or `packed_outputs` mismatch.
    pub fn step(
        &mut self,
        compiled: &CompiledCircuit,
        packed_inputs: &[u64],
        packed_outputs: &mut [u64],
    ) -> Result<(), EvalError> {
        let expected_in = usize::from(compiled.num_inputs);
        let expected_out = usize::from(compiled.num_outputs);
        if packed_inputs.len() != expected_in {
            return Err(EvalError::PortSliceMismatch {
                kind: "input",
                expected: expected_in,
                actual: packed_inputs.len(),
            });
        }
        if packed_outputs.len() != expected_out {
            return Err(EvalError::PortSliceMismatch {
                kind: "output",
                expected: expected_out,
                actual: packed_outputs.len(),
            });
        }

        // Phase 1: Load 64-lane packed inputs and synchronous registers.
        self.wires[..expected_in].copy_from_slice(packed_inputs);
        for &(reg_idx, wire_idx) in &compiled.delay_reads {
            self.wires[wire_idx] = self.regs[reg_idx];
        }

        // Phase 2: Bitwise parallel combinational pass over GF(2).
        for &op in &compiled.comb_ops {
            match op {
                CombOp::Add { lhs, rhs, out } | CombOp::Sub { lhs, rhs, out } => {
                    self.wires[out] = self.wires[lhs] ^ self.wires[rhs];
                }
                CombOp::Mul { lhs, rhs, out } => {
                    self.wires[out] = self.wires[lhs] & self.wires[rhs];
                }
                CombOp::Eqz { arg, out } => {
                    self.wires[out] = !self.wires[arg];
                }
                CombOp::Split { arg, out0, out1 } => {
                    let v = self.wires[arg];
                    self.wires[out0] = v;
                    self.wires[out1] = v;
                }
            }
        }

        // Phase 3a: Emit packed 64-lane boundary predictions.
        for (out_slot, &driver) in packed_outputs.iter_mut().zip(&compiled.output_drivers) {
            *out_slot = driver.map_or(0u64, |wire_idx| self.wires[wire_idx]);
        }

        // Phase 3b: Synchronously latch all 64-lane registers.
        for &(reg_idx, in_wire) in &compiled.delay_latches {
            self.regs[reg_idx] = self.wires[in_wire];
        }
        for chain in &compiled.shift_chains {
            for stage in (1..chain.hops).rev() {
                self.regs[chain.base_reg + stage] = self.regs[chain.base_reg + stage - 1];
            }
            self.regs[chain.base_reg] = self.wires[chain.src_wire];
        }

        Ok(())
    }
}
