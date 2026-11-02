//! Level-0 foundational axioms (`ADD`, `SUB`, `MUL`, `EQZ`, `DELAY`, `SPLIT`).
//!
//! Defines port arities, statefulness, and commutativity metadata per Charter §2.3.

use serde::{Deserialize, Serialize};

use crate::substrate::value::Width;

/// Foundational Level-0 circuit primitive (Charter §2.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Primitive {
    /// $y = a + b \bmod 2^k$ (2 inputs, 1 output).
    Add,
    /// $y = a - b \bmod 2^k$ (2 ordered inputs `in[0] - in[1]`, 1 output).
    Sub,
    /// $y = a \cdot b \bmod 2^k$ (2 inputs, 1 output).
    Mul,
    /// $y = 1$ if $x = 0$, else $0$ (1 input, 1 output).
    Eqz,
    /// Unit memory $y(t) = x(t - 1)$ with explicit initial register value `init` (1 input, 1 output).
    Delay { init: u64 },
    /// Explicit linear fan-out $y_0 = x,\; y_1 = x$ (1 input, 2 outputs).
    Split,
}

impl Primitive {
    /// Total number of Level-0 primitive kinds in the foundational palette.
    pub const PALETTE_SIZE: usize = 6;

    /// Convenience constructor for a zero-initialised [`Primitive::Delay`].
    #[must_use]
    pub const fn delay_zero() -> Self {
        Self::Delay { init: 0 }
    }

    /// Number of input slots consumed by this primitive.
    #[must_use]
    pub const fn input_arity(self) -> u8 {
        match self {
            Self::Add | Self::Sub | Self::Mul => 2,
            Self::Eqz | Self::Delay { .. } | Self::Split => 1,
        }
    }

    /// Number of distinct output slots produced by this primitive.
    #[must_use]
    pub const fn output_arity(self) -> u8 {
        match self {
            Self::Split => 2,
            Self::Add | Self::Sub | Self::Mul | Self::Eqz | Self::Delay { .. } => 1,
        }
    }

    /// Returns `true` iff this primitive holds synchronous state across ticks (`DELAY`).
    #[must_use]
    pub const fn is_stateful(self) -> bool {
        matches!(self, Self::Delay { .. })
    }

    /// Returns `true` iff swapping `in[0]` and `in[1]` preserves exact semantics for all inputs.
    #[must_use]
    pub const fn is_commutative(self) -> bool {
        matches!(self, Self::Add | Self::Mul)
    }

    /// Returns a copy with any embedded register value masked to `width`.
    #[must_use]
    pub const fn normalized(self, width: Width) -> Self {
        match self {
            Self::Delay { init } => Self::Delay {
                init: width.normalize(init),
            },
            other => other,
        }
    }

    /// Canonical tag index in `0..6` used for description-length accounting.
    #[must_use]
    pub const fn tag_index(self) -> u8 {
        match self {
            Self::Add => 0,
            Self::Sub => 1,
            Self::Mul => 2,
            Self::Eqz => 3,
            Self::Delay { .. } => 4,
            Self::Split => 5,
        }
    }
}
