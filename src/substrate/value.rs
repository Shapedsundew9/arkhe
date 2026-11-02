//! Exact variable-width modular arithmetic over $\mathbb{Z}_{2^k}$ ($1 \le k \le 64$).
//!
//! Implements Charter §2.1–§2.4 and §9: unsigned machine integer values masked to $k$ bits,
//! wrapping ring operations (`ADD`, `SUB`, `MUL`), exact zero-test (`EQZ`), derived equality
//! comparator (`EQZ(SUB(a, b))`), and graded circular discrepancy magnitude.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Error returned when constructing an out-of-range bit [`Width`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WidthError(pub u8);

impl fmt::Display for WidthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid bit width {}: expected 1..=64", self.0)
    }
}

impl std::error::Error for WidthError {}

/// Uniform substrate bit-width $k \in \{1, 2, \dots, 64\}$ (Phase W1, Charter §2.2 & §2.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct Width(u8);

impl Width {
    /// Boolean width $k = 1$ ($GF(2)$, exact v1 special case).
    pub const BITS_1: Self = Self(1);
    /// 2-bit ring width $k = 2$ ($\mathbb{Z}_4$, mandatory M1a+ graded test width).
    pub const BITS_2: Self = Self(2);

    /// Validates and constructs a [`Width`] in `1..=64`.
    ///
    /// # Errors
    ///
    /// Returns [`WidthError`] if `bits == 0` or `bits > 64`.
    pub const fn new(bits: u8) -> Result<Self, WidthError> {
        if bits >= 1 && bits <= 64 {
            Ok(Self(bits))
        } else {
            Err(WidthError(bits))
        }
    }

    /// Returns the width $k$ in bits.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Bitmask retaining the lowest $k$ bits (`u64::MAX` when $k = 64$, overflow-free).
    #[must_use]
    pub const fn mask(self) -> u64 {
        if self.0 == 64 {
            u64::MAX
        } else {
            (1u64 << self.0) - 1
        }
    }

    /// Cardinality $2^k$ of $\mathbb{Z}_{2^k}$ as a `u128` (exact for all $1 \le k \le 64$).
    #[must_use]
    pub const fn cardinality(self) -> u128 {
        1u128 << self.0
    }

    /// Masks `raw` into $\mathbb{Z}_{2^k}$.
    #[must_use]
    pub const fn normalize(self, raw: u64) -> u64 {
        raw & self.mask()
    }

    /// `ADD`: $a + b \bmod 2^k$. At $k = 1$, equals `XOR`.
    #[must_use]
    pub const fn add(self, a: u64, b: u64) -> u64 {
        a.wrapping_add(b) & self.mask()
    }

    /// `SUB`: $a - b \bmod 2^k$. Zero iff $a \equiv b \pmod{2^k}$. At $k = 1$, equals `XOR`.
    #[must_use]
    pub const fn sub(self, a: u64, b: u64) -> u64 {
        a.wrapping_sub(b) & self.mask()
    }

    /// `MUL`: $a \cdot b \bmod 2^k$. At $k = 1$, equals `AND`.
    #[must_use]
    pub const fn mul(self, a: u64, b: u64) -> u64 {
        a.wrapping_mul(b) & self.mask()
    }

    /// `EQZ`: `1` if $x \equiv 0 \pmod{2^k}$, else `0`. At $k = 1$, equals `NOT`.
    #[must_use]
    pub const fn eqz(self, x: u64) -> u64 {
        ((x & self.mask()) == 0) as u64
    }

    /// Derived exact comparator `EQZ(SUB(a, b))` (§2.3). At $k = 1$, equals `XNOR`.
    #[must_use]
    pub const fn comparator(self, a: u64, b: u64) -> u64 {
        self.eqz(self.sub(a, b))
    }

    /// Signed-free circular residual magnitude $\min(d, 2^k - d)$ where $d = a - b \bmod 2^k$ (§2.3).
    #[must_use]
    pub const fn circular_distance(self, a: u64, b: u64) -> u64 {
        let d = self.sub(a, b);
        let neg_d = d.wrapping_neg() & self.mask();
        if d < neg_d { d } else { neg_d }
    }
}

impl TryFrom<u8> for Width {
    type Error = WidthError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Width> for u8 {
    fn from(w: Width) -> Self {
        w.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_bounds_and_masks() {
        assert!(Width::new(0).is_err());
        assert!(Width::new(65).is_err());
        for k in [1u8, 2, 4, 8, 16, 32, 63] {
            let w = Width::new(k).unwrap();
            assert_eq!(w.mask(), (1u64 << k) - 1);
            assert_eq!(w.cardinality(), 1u128 << k);
        }
        let w64 = Width::new(64).unwrap();
        assert_eq!(w64.mask(), u64::MAX);
        assert_eq!(w64.cardinality(), 1u128 << 64);
    }

    #[test]
    fn boolean_k1_reduces_to_gf2_primitives() {
        let w = Width::BITS_1;
        for a in 0..=1 {
            assert_eq!(w.eqz(a), a ^ 1);
            for b in 0..=1 {
                assert_eq!(w.add(a, b), a ^ b);
                assert_eq!(w.sub(a, b), a ^ b);
                assert_eq!(w.mul(a, b), a & b);
                assert_eq!(w.comparator(a, b), (a ^ b) ^ 1);
                assert_eq!(w.circular_distance(a, b), a ^ b);
            }
        }
    }

    #[test]
    fn graded_ring_ops_and_k64_extremes() {
        let w2 = Width::BITS_2;
        assert_eq!(w2.add(3, 2), 1);
        assert_eq!(w2.sub(1, 3), 2);
        assert_eq!(w2.mul(2, 2), 0); // Zero divisor in Z_4
        assert_eq!(w2.circular_distance(0, 3), 1);
        assert_eq!(w2.circular_distance(1, 3), 2);
        assert_eq!(w2.comparator(3, 3), 1);
        assert_eq!(w2.comparator(3, 1), 0);

        let w64 = Width::new(64).unwrap();
        assert_eq!(w64.add(u64::MAX, 1), 0);
        assert_eq!(w64.sub(0, 1), u64::MAX);
        assert_eq!(w64.circular_distance(0, u64::MAX), 1);
        assert_eq!(w64.circular_distance(0, 0), 0);
        assert_eq!(w64.eqz(0), 1);
        assert_eq!(w64.eqz(u64::MAX), 0);
    }
}
