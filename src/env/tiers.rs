//! Benchmark generator catalogue for Milestone 0: Tiers **T1**, **T3b**, and **T3c** (Charter §5.3).

use std::collections::{BTreeMap, VecDeque};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::env::generator::{
    DEFAULT_MAX_EXPORT_STATES, EpsilonMachineScope, FiniteStateGenerator, FiniteStateSpec,
    SpecExportError,
};
use crate::env::reeds_sloane::odd_inverse_mod_pow2;
use crate::substrate::Width;

/// Error returned when validating a [`GeneratorConfig`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GeneratorError {
    /// Periodic motif must contain at least one symbol.
    EmptyMotif,
    /// `T3b_CounterPow2` requires `1 <= log2_modulus <= width.bits()`.
    InvalidPow2Modulus { log2_modulus: u8, width_bits: u8 },
    /// `T3b_CounterModN` requires `2 <= modulus <= 2^k`.
    InvalidModulus { modulus: u64, max_allowed: u128 },
    /// `T3c_Lfsr` requires `taps.len() >= 1` and `init.len() == taps.len()`.
    InvalidLfsrLengths { taps_len: usize, init_len: usize },
    /// `T3c_LfsrCurriculum` requires at least one stage and `ticks_per_stage >= 1`.
    InvalidCurriculum,
    /// Cross-channel delayed coupling (`T2_*`) requires `lag >= 1`.
    InvalidCrossLag { lag: usize },
    /// Multi-channel composition requires at least one generator and matching symbol widths.
    InvalidChannelComposition,
    /// `T2_MultiViewCoupled` view multipliers must be odd so each view is invertible mod $2^k$.
    NonInvertibleViewMultiplier { channel: u16, mul_a: u64 },
    /// `T2_MultiViewCoupled` views on channels 0 and 1 must be distinct after width normalization.
    DuplicateViewTransform,
    /// `T2_MultiViewCoupled` target lag must be strictly greater than both view delays for causal prediction.
    NonCausalMultiViewLag {
        channel: u16,
        view_delay: usize,
        target_lag: usize,
    },
}

impl fmt::Display for GeneratorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyMotif => write!(f, "T1_PeriodicMotif requires a non-empty motif"),
            Self::InvalidPow2Modulus {
                log2_modulus,
                width_bits,
            } => write!(
                f,
                "T3b_CounterPow2 log2_modulus={log2_modulus} invalid for width k={width_bits}"
            ),
            Self::InvalidModulus {
                modulus,
                max_allowed,
            } => write!(
                f,
                "T3b_CounterModN modulus={modulus} outside 2..={max_allowed}"
            ),
            Self::InvalidLfsrLengths { taps_len, init_len } => write!(
                f,
                "T3c_Lfsr requires taps.len() >= 1 and taps.len() == init.len(), got {taps_len} vs {init_len}"
            ),
            Self::InvalidCurriculum => {
                write!(
                    f,
                    "T3c_LfsrCurriculum requires non-empty stages and ticks_per_stage >= 1"
                )
            }
            Self::InvalidCrossLag { lag } => {
                write!(f, "T2 cross-channel lag must be >= 1, got {lag}")
            }
            Self::InvalidChannelComposition => {
                write!(
                    f,
                    "multi-channel generator composition requires non-empty channels with matching widths"
                )
            }
            Self::NonInvertibleViewMultiplier { channel, mul_a } => write!(
                f,
                "T2_MultiViewCoupled view_{channel} mul_a={mul_a} must be odd to be invertible mod 2^k"
            ),
            Self::DuplicateViewTransform => write!(
                f,
                "T2_MultiViewCoupled requires distinct normalized views on ch0 and ch1"
            ),
            Self::NonCausalMultiViewLag {
                channel,
                view_delay,
                target_lag,
            } => write!(
                f,
                "T2_MultiViewCoupled target lag {target_lag} must exceed view_{channel}.delay ({view_delay})"
            ),
        }
    }
}

/// Binary linear operator combining two delayed driver channels modulo $2^k$ in [`GeneratorConfig::T2_CrossLinear`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CrossLinearOp {
    /// $A_0(t - d_0) + A_1(t - d_1) \bmod 2^k$.
    Add,
    /// $A_0(t - d_0) - A_1(t - d_1) \bmod 2^k$.
    Sub,
}

/// Binary nonlinear operator combining two delayed driver channels modulo $2^k$ in [`GeneratorConfig::T2_CrossNonlinear`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CrossNonlinearOp {
    /// $A_0(t - d_0) \cdot A_1(t - d_1) \bmod 2^k$.
    Mul,
    /// $A_0(t - d_0) \cdot \mathrm{Eqz}(A_1(t - d_1)) \bmod 2^k$.
    MulEqz,
    /// $A_0(t - d_0) + \mathrm{Eqz}(A_1(t - d_1)) \bmod 2^k$.
    AddEqz,
    /// $\mathrm{Eqz}(A_0(t - d_0)) \cdot \mathrm{Eqz}(A_1(t - d_1)) \bmod 2^k$.
    MulEqzPair,
}

impl std::error::Error for GeneratorError {}

/// Single LFSR stage inside a [`GeneratorConfig::T3c_LfsrCurriculum`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LfsrStage {
    /// Tap coefficients $[c_1, \dots, c_L]$ for $s(t) = \sum_{i=1}^L c_i s(t - i) \bmod 2^k$.
    pub taps: Vec<u64>,
    /// Initial seed symbols $[s(0), \dots, s(L - 1)]$.
    pub init: Vec<u64>,
}

/// Latent driver specification $Z(t)$ for [`GeneratorConfig::T2_MultiViewCoupled`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum MultiViewDriver {
    /// Linear feedback shift register $Z(t) = \sum_{i=1}^L c_i Z(t - i) \bmod 2^k$.
    Lfsr { stage: LfsrStage },
    /// Fixed periodic motif $Z(t) = \text{motif}[t \bmod P] \bmod 2^k$ (for autonomous loopback).
    PeriodicMotif { motif: Vec<u64> },
    /// Single-register affine recurrence $Z(t + 1) = a \cdot Z(t) + b \bmod 2^k$.
    AffineLag1 { mul_a: u64, add_b: u64, init: u64 },
}

/// Invertible affine-delayed view mapping latent driver $Z$ onto channel 0 or 1 in
/// [`GeneratorConfig::T2_MultiViewCoupled`].
///
/// Emits $\text{ch}_i(t) = a_i \cdot Z(t - v_i) + b_i \bmod 2^k$ ($Z(u) = 0$ for $u < 0$),
/// with odd $a_i$ so $Z(t - v_i)$ is uniquely recoverable from $\text{ch}_i(t)$.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultiViewTransform {
    /// Non-negative observation delay $v_i \ge 0$ relative to $Z(t)$.
    pub delay: usize,
    /// Odd invertible multiplier $a_i$ (`mul_a & 1 == 1`) modulo $2^k$.
    pub mul_a: u64,
    /// Additive offset $b_i \bmod 2^k$.
    pub add_b: u64,
}

impl MultiViewTransform {
    /// Returns a copy with `mul_a` and `add_b` masked to `width`.
    #[must_use]
    pub const fn normalized(self, width: Width) -> Self {
        Self {
            delay: self.delay,
            mul_a: width.normalize(self.mul_a),
            add_b: width.normalize(self.add_b),
        }
    }

    /// Evaluates $a_i \cdot z + b_i \bmod 2^k$.
    #[must_use]
    pub const fn eval(self, width: Width, z_delayed: u64) -> u64 {
        width.add(width.mul(self.mul_a, z_delayed), self.add_b)
    }

    /// Inverts an observed view symbol back to $z \in \mathbb{Z}_{2^k}$ if `mul_a` is odd.
    #[must_use]
    pub const fn try_invert_z(self, width: Width, view_sym: u64) -> Option<u64> {
        let a = width.normalize(self.mul_a);
        if (a & 1) == 0 {
            return None;
        }
        let inv_a = odd_inverse_mod_pow2(a, width);
        Some(width.mul(inv_a, width.sub(view_sym, self.add_b)))
    }

    /// Inverts an observed view symbol back to $z \in \mathbb{Z}_{2^k}$.
    ///
    /// # Panics
    ///
    /// Panics if `self.mul_a` is even mod $2^k$.
    #[must_use]
    pub const fn invert_z(self, width: Width, view_sym: u64) -> u64 {
        self.try_invert_z(width, view_sym)
            .expect("MultiViewTransform requires odd mul_a")
    }
}

/// Target coupling on channel 2 from delayed latent driver $Z$ in [`GeneratorConfig::T2_MultiViewCoupled`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum MultiViewTarget {
    /// $a \cdot Z(t - d) + b \bmod 2^k$.
    Affine { lag: usize, mul_a: u64, add_b: u64 },
    /// $\mathrm{Eqz}(Z(t - d) + b) \bmod 2^k$.
    Eqz {
        lag: usize,
        #[serde(default)]
        add_b: u64,
    },
    /// $Z(t - d_a) + \mathrm{Eqz}(Z(t - d_b)) \bmod 2^k$.
    AddEqz { lag_a: usize, lag_b: usize },
    /// $Z(t - d_a) \cdot \mathrm{Eqz}(Z(t - d_b)) \bmod 2^k$.
    MulEqz { lag_a: usize, lag_b: usize },
    /// $\mathrm{Eqz}(Z(t - d_a)) \cdot \mathrm{Eqz}(Z(t - d_b)) \bmod 2^k$.
    MulEqzPair { lag_a: usize, lag_b: usize },
}

impl MultiViewTarget {
    /// Minimum driver lag referenced by this target rule.
    #[must_use]
    pub const fn min_lag(self) -> usize {
        match self {
            Self::Affine { lag, .. } | Self::Eqz { lag, .. } => lag,
            Self::AddEqz { lag_a, lag_b }
            | Self::MulEqz { lag_a, lag_b }
            | Self::MulEqzPair { lag_a, lag_b } => {
                if lag_a < lag_b {
                    lag_a
                } else {
                    lag_b
                }
            }
        }
    }

    /// Maximum driver lag referenced by this target rule.
    #[must_use]
    pub const fn max_lag(self) -> usize {
        match self {
            Self::Affine { lag, .. } | Self::Eqz { lag, .. } => lag,
            Self::AddEqz { lag_a, lag_b }
            | Self::MulEqz { lag_a, lag_b }
            | Self::MulEqzPair { lag_a, lag_b } => {
                if lag_a > lag_b {
                    lag_a
                } else {
                    lag_b
                }
            }
        }
    }

    /// Returns a copy with constant operands masked to `width`.
    #[must_use]
    pub const fn normalized(self, width: Width) -> Self {
        match self {
            Self::Affine { lag, mul_a, add_b } => Self::Affine {
                lag,
                mul_a: width.normalize(mul_a),
                add_b: width.normalize(add_b),
            },
            Self::Eqz { lag, add_b } => Self::Eqz {
                lag,
                add_b: width.normalize(add_b),
            },
            other => other,
        }
    }

    /// Evaluates the target symbol from delayed latent samples `(z_a, z_b)`.
    ///
    /// Unary variants (`Affine`, `Eqz`) use `z_a` and ignore `z_b`.
    #[must_use]
    pub const fn eval(self, width: Width, z_a: u64, z_b: u64) -> u64 {
        match self {
            Self::Affine { mul_a, add_b, .. } => width.add(width.mul(mul_a, z_a), add_b),
            Self::Eqz { add_b, .. } => width.eqz(width.add(z_a, add_b)),
            Self::AddEqz { .. } => width.add(z_a, width.eqz(z_b)),
            Self::MulEqz { .. } => width.mul(z_a, width.eqz(z_b)),
            Self::MulEqzPair { .. } => width.mul(width.eqz(z_a), width.eqz(z_b)),
        }
    }

    /// Predicts channel 2 strictly causally from a single view channel's past history `channel_history[0..t]`.
    #[must_use]
    pub fn predict_from_view_history(
        self,
        width: Width,
        view: MultiViewTransform,
        channel_history: &[u64],
    ) -> u64 {
        let recover = |lag: usize| -> u64 {
            let Some(tau) = lag.checked_sub(view.delay) else {
                return 0;
            };
            if tau == 0 || channel_history.len() < tau {
                0
            } else {
                view.invert_z(width, channel_history[channel_history.len() - tau])
            }
        };
        match self {
            Self::Affine { lag, .. } | Self::Eqz { lag, .. } => self.eval(width, recover(lag), 0),
            Self::AddEqz { lag_a, lag_b }
            | Self::MulEqz { lag_a, lag_b }
            | Self::MulEqzPair { lag_a, lag_b } => self.eval(width, recover(lag_a), recover(lag_b)),
        }
    }
}

/// Serializable specification for all M0 benchmark generators (Tiers T1, T3b, T3c).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
#[allow(non_camel_case_types)]
pub enum GeneratorConfig {
    /// T1: Constant stream $s(t) = c \bmod 2^k$ ($C_\mu = 0, h_\mu = 0$).
    T1_Constant { value: u64 },
    /// T1: Period-2 alternation $a, b, a, b, \dots \bmod 2^k$.
    T1_Alternating { a: u64, b: u64 },
    /// T1: Arbitrary fixed periodic rhythm `motif[t mod P]`.
    T1_PeriodicMotif { motif: Vec<u64> },
    /// T1: Single-register affine recurrence $s(t+1) = a \cdot s(t) + b \bmod 2^k$.
    T1_AffineLag1 { mul_a: u64, add_b: u64, init: u64 },
    /// T3b: Native ring counter modulo $2^j$ ($1 \le j \le k$).
    T3b_CounterPow2 {
        log2_modulus: u8,
        #[serde(default)]
        init: u64,
    },
    /// T3b: Non-power-of-two counter modulo $n$ ($2 \le n \le 2^k$; tests `EQZ` completeness).
    T3b_CounterModN {
        modulus: u64,
        #[serde(default)]
        init: u64,
    },
    /// T3c: Linear feedback recurrence $s(t) = \sum_{i=1}^L c_i s(t - i) \bmod 2^k$.
    T3c_Lfsr { taps: Vec<u64>, init: Vec<u64> },
    /// T3c: Multi-stage LFSR curriculum stepping through sub-parities before full composition.
    T3c_LfsrCurriculum {
        stages: Vec<LfsrStage>,
        ticks_per_stage: u64,
    },
    /// T2 ($K = 2$): Channel 0 is LFSR $A_0(t)$; Channel 1 is $a \cdot A_0(t - d) + b \bmod 2^k$.
    T2_DelayedAffine {
        driver: LfsrStage,
        lag: usize,
        mul_a: u64,
        add_b: u64,
    },
    /// T2 ($K = 3$): Channels 0, 1 are LFSRs $A_0(t), A_1(t)$; Channel 2 is $A_0(t - d_0) \pm A_1(t - d_1) \bmod 2^k$.
    T2_CrossLinear {
        driver_0: LfsrStage,
        driver_1: LfsrStage,
        lag_0: usize,
        lag_1: usize,
        op: CrossLinearOp,
    },
    /// T2 ($K = 3$): Channels 0, 1 are LFSRs $A_0(t), A_1(t)$; Channel 2 applies a nonlinear operator over $(A_0(t - d_0), A_1(t - d_1))$.
    T2_CrossNonlinear {
        driver_0: LfsrStage,
        driver_1: LfsrStage,
        lag_0: usize,
        lag_1: usize,
        op: CrossNonlinearOp,
    },
    /// T2 ($K = 3$): Multi-view coupled system where latent driver $Z(t)$ feeds two distinct
    /// invertible affine-delayed views on channels 0 and 1, and a causal target coupling on
    /// channel 2 so channel 2 is an exact causal deterministic target of channel 0 alone and
    /// of channel 1 alone.
    T2_MultiViewCoupled {
        /// Latent driver process $Z(t)$.
        driver: MultiViewDriver,
        /// Invertible view mapping $Z$ onto channel 0.
        view_0: MultiViewTransform,
        /// Distinct invertible view mapping $Z$ onto channel 1.
        view_1: MultiViewTransform,
        /// Causal target rule emitted on channel 2.
        target: MultiViewTarget,
    },
}

impl GeneratorConfig {
    /// Builds a validated runtime [`TierGenerator`] at uniform bit-width `width`.
    ///
    /// # Errors
    ///
    /// Returns [`GeneratorError`] if any parameter is invalid for `width`.
    pub fn build(&self, width: Width) -> Result<TierGenerator, GeneratorError> {
        let kind = match self {
            Self::T1_Constant { value } => RuntimeKind::Constant {
                val: width.normalize(*value),
            },
            Self::T1_Alternating { a, b } => RuntimeKind::Alternating {
                a: width.normalize(*a),
                b: width.normalize(*b),
                phase: false,
            },
            Self::T1_PeriodicMotif { motif } => {
                if motif.is_empty() {
                    return Err(GeneratorError::EmptyMotif);
                }
                let norm: Vec<u64> = motif.iter().map(|&x| width.normalize(x)).collect();
                RuntimeKind::PeriodicMotif {
                    motif: norm,
                    idx: 0,
                }
            }
            Self::T1_AffineLag1 { mul_a, add_b, init } => {
                let s0 = width.normalize(*init);
                RuntimeKind::AffineLag1 {
                    mul_a: width.normalize(*mul_a),
                    add_b: width.normalize(*add_b),
                    init: s0,
                    state: s0,
                }
            }
            Self::T3b_CounterPow2 { log2_modulus, init } => {
                if *log2_modulus == 0 || *log2_modulus > width.bits() {
                    return Err(GeneratorError::InvalidPow2Modulus {
                        log2_modulus: *log2_modulus,
                        width_bits: width.bits(),
                    });
                }
                let mask = if *log2_modulus == 64 {
                    u64::MAX
                } else {
                    (1u64 << *log2_modulus) - 1
                };
                let s0 = *init & mask;
                RuntimeKind::CounterPow2 {
                    log2_modulus: *log2_modulus,
                    mod_mask: mask,
                    init: s0,
                    state: s0,
                }
            }
            Self::T3b_CounterModN { modulus, init } => {
                let max_allowed = width.cardinality();
                if *modulus < 2 || u128::from(*modulus) > max_allowed {
                    return Err(GeneratorError::InvalidModulus {
                        modulus: *modulus,
                        max_allowed,
                    });
                }
                let s0 = *init % *modulus;
                RuntimeKind::CounterModN {
                    modulus: *modulus,
                    init: s0,
                    state: s0,
                }
            }
            Self::T3c_Lfsr { taps, init } => {
                if taps.is_empty() || taps.len() != init.len() {
                    return Err(GeneratorError::InvalidLfsrLengths {
                        taps_len: taps.len(),
                        init_len: init.len(),
                    });
                }
                let norm_taps: Vec<u64> = taps.iter().map(|&c| width.normalize(c)).collect();
                let norm_init: Vec<u64> = init.iter().map(|&x| width.normalize(x)).collect();
                RuntimeKind::Lfsr {
                    taps: norm_taps,
                    init: norm_init.clone(),
                    window: norm_init,
                }
            }
            Self::T3c_LfsrCurriculum {
                stages,
                ticks_per_stage,
            } => {
                if stages.is_empty() || *ticks_per_stage == 0 {
                    return Err(GeneratorError::InvalidCurriculum);
                }
                let mut norm_stages = Vec::with_capacity(stages.len());
                for st in stages {
                    if st.taps.is_empty() || st.taps.len() != st.init.len() {
                        return Err(GeneratorError::InvalidLfsrLengths {
                            taps_len: st.taps.len(),
                            init_len: st.init.len(),
                        });
                    }
                    norm_stages.push(LfsrStage {
                        taps: st.taps.iter().map(|&c| width.normalize(c)).collect(),
                        init: st.init.iter().map(|&x| width.normalize(x)).collect(),
                    });
                }
                let first_window = norm_stages[0].init.clone();
                RuntimeKind::LfsrCurriculum {
                    stages: norm_stages,
                    ticks_per_stage: *ticks_per_stage,
                    stage_idx: 0,
                    stage_tick: 0,
                    window: first_window,
                }
            }
            Self::T2_DelayedAffine {
                driver,
                lag,
                mul_a,
                add_b,
            } => {
                if *lag == 0 {
                    return Err(GeneratorError::InvalidCrossLag { lag: *lag });
                }
                if driver.taps.is_empty() || driver.taps.len() != driver.init.len() {
                    return Err(GeneratorError::InvalidLfsrLengths {
                        taps_len: driver.taps.len(),
                        init_len: driver.init.len(),
                    });
                }
                let norm_taps: Vec<u64> = driver.taps.iter().map(|&c| width.normalize(c)).collect();
                let norm_init: Vec<u64> = driver.init.iter().map(|&x| width.normalize(x)).collect();
                RuntimeKind::T2DelayedAffine {
                    taps: norm_taps,
                    init: norm_init.clone(),
                    window: norm_init,
                    lag: *lag,
                    hist: VecDeque::from(vec![0u64; *lag]),
                    mul_a: width.normalize(*mul_a),
                    add_b: width.normalize(*add_b),
                }
            }
            Self::T2_CrossLinear {
                driver_0,
                driver_1,
                lag_0,
                lag_1,
                op,
            } => Self::build_t2_dual_driver(
                width,
                driver_0,
                driver_1,
                *lag_0,
                *lag_1,
                T2DualOp::Linear(*op),
            )?,
            Self::T2_CrossNonlinear {
                driver_0,
                driver_1,
                lag_0,
                lag_1,
                op,
            } => Self::build_t2_dual_driver(
                width,
                driver_0,
                driver_1,
                *lag_0,
                *lag_1,
                T2DualOp::Nonlinear(*op),
            )?,
            Self::T2_MultiViewCoupled {
                driver,
                view_0,
                view_1,
                target,
            } => Self::build_t2_multiview(width, driver, *view_0, *view_1, *target)?,
        };

        Ok(TierGenerator {
            label: self.default_label(),
            width,
            kind,
        })
    }

    fn build_t2_dual_driver(
        width: Width,
        driver_0: &LfsrStage,
        driver_1: &LfsrStage,
        lag_0: usize,
        lag_1: usize,
        op: T2DualOp,
    ) -> Result<RuntimeKind, GeneratorError> {
        if lag_0 == 0 {
            return Err(GeneratorError::InvalidCrossLag { lag: lag_0 });
        }
        if lag_1 == 0 {
            return Err(GeneratorError::InvalidCrossLag { lag: lag_1 });
        }
        for d in [driver_0, driver_1] {
            if d.taps.is_empty() || d.taps.len() != d.init.len() {
                return Err(GeneratorError::InvalidLfsrLengths {
                    taps_len: d.taps.len(),
                    init_len: d.init.len(),
                });
            }
        }
        let taps_0: Vec<u64> = driver_0.taps.iter().map(|&c| width.normalize(c)).collect();
        let init_0: Vec<u64> = driver_0.init.iter().map(|&x| width.normalize(x)).collect();
        let taps_1: Vec<u64> = driver_1.taps.iter().map(|&c| width.normalize(c)).collect();
        let init_1: Vec<u64> = driver_1.init.iter().map(|&x| width.normalize(x)).collect();
        Ok(RuntimeKind::T2DualDriver {
            taps_0,
            window_0: init_0.clone(),
            init_0,
            taps_1,
            window_1: init_1.clone(),
            init_1,
            lag_0,
            hist_0: VecDeque::from(vec![0u64; lag_0]),
            lag_1,
            hist_1: VecDeque::from(vec![0u64; lag_1]),
            op,
        })
    }

    fn build_t2_multiview(
        width: Width,
        driver: &MultiViewDriver,
        view_0: MultiViewTransform,
        view_1: MultiViewTransform,
        target: MultiViewTarget,
    ) -> Result<RuntimeKind, GeneratorError> {
        let runtime_driver = MultiViewRuntimeDriver::from_config(width, driver)?;
        let v0 = view_0.normalized(width);
        let v1 = view_1.normalized(width);
        for (channel, view) in [(0u16, v0), (1u16, v1)] {
            if (view.mul_a & 1) == 0 {
                return Err(GeneratorError::NonInvertibleViewMultiplier {
                    channel,
                    mul_a: view.mul_a,
                });
            }
        }
        if v0 == v1 {
            return Err(GeneratorError::DuplicateViewTransform);
        }
        let target_norm = target.normalized(width);
        let min_lag = target_norm.min_lag();
        for (channel, view) in [(0u16, v0), (1u16, v1)] {
            if min_lag <= view.delay {
                return Err(GeneratorError::NonCausalMultiViewLag {
                    channel,
                    view_delay: view.delay,
                    target_lag: min_lag,
                });
            }
        }
        let max_delay = target_norm.max_lag().max(v0.delay).max(v1.delay);
        debug_assert!(max_delay >= 1);
        Ok(RuntimeKind::T2MultiViewCoupled {
            driver: runtime_driver,
            view_0: v0,
            view_1: v1,
            target: target_norm,
            max_delay,
            hist: VecDeque::from(vec![0u64; max_delay]),
        })
    }

    fn default_label(&self) -> String {
        match self {
            Self::T1_Constant { value } => format!("T1_Constant({value})"),
            Self::T1_Alternating { a, b } => format!("T1_Alternating({a},{b})"),
            Self::T1_PeriodicMotif { motif } => format!("T1_PeriodicMotif(len={})", motif.len()),
            Self::T1_AffineLag1 { mul_a, add_b, .. } => format!("T1_AffineLag1({mul_a}x+{add_b})"),
            Self::T3b_CounterPow2 { log2_modulus, .. } => {
                format!("T3b_CounterPow2(2^{log2_modulus})")
            }
            Self::T3b_CounterModN { modulus, .. } => format!("T3b_CounterModN(mod {modulus})"),
            Self::T3c_Lfsr { taps, .. } => format!("T3c_Lfsr(L={})", taps.len()),
            Self::T3c_LfsrCurriculum { stages, .. } => {
                format!("T3c_LfsrCurriculum(stages={})", stages.len())
            }
            Self::T2_DelayedAffine {
                lag, mul_a, add_b, ..
            } => format!("T2_DelayedAffine(d={lag},{mul_a}x+{add_b})"),
            Self::T2_CrossLinear {
                lag_0, lag_1, op, ..
            } => format!("T2_CrossLinear({op:?},d0={lag_0},d1={lag_1})"),
            Self::T2_CrossNonlinear {
                lag_0, lag_1, op, ..
            } => format!("T2_CrossNonlinear({op:?},d0={lag_0},d1={lag_1})"),
            Self::T2_MultiViewCoupled {
                driver,
                view_0,
                view_1,
                target,
            } => {
                let d_tag = match driver {
                    MultiViewDriver::Lfsr { stage } => format!("Lfsr(L={})", stage.taps.len()),
                    MultiViewDriver::PeriodicMotif { motif } => {
                        format!("Periodic(P={})", motif.len())
                    }
                    MultiViewDriver::AffineLag1 { mul_a, add_b, .. } => {
                        format!("Affine1({mul_a}x+{add_b})")
                    }
                };
                format!(
                    "T2_MultiViewCoupled({d_tag},v0=({},{}x+{}),v1=({},{}x+{}),tgt={target:?})",
                    view_0.delay,
                    view_0.mul_a,
                    view_0.add_b,
                    view_1.delay,
                    view_1.mul_a,
                    view_1.add_b
                )
            }
        }
    }

    /// Standard Milestone 0 Tier-1 benchmark suite used by Gate G3 (`verify-m0`).
    #[must_use]
    pub fn m0_t1_suite(width: Width) -> Vec<Self> {
        let high = if width.bits() == 1 { 1 } else { 3 };
        vec![
            Self::T1_Constant { value: 1 },
            Self::T1_Alternating { a: 0, b: high },
            Self::T1_PeriodicMotif {
                motif: vec![0, 1, high],
            },
            Self::T1_AffineLag1 {
                mul_a: 1,
                add_b: 1,
                init: 0,
            },
        ]
    }
}

#[derive(Clone, Copy, Debug)]
enum T2DualOp {
    Linear(CrossLinearOp),
    Nonlinear(CrossNonlinearOp),
}

impl T2DualOp {
    fn eval(self, width: Width, a0: u64, a1: u64) -> u64 {
        match self {
            Self::Linear(CrossLinearOp::Add) => width.add(a0, a1),
            Self::Linear(CrossLinearOp::Sub) => width.sub(a0, a1),
            Self::Nonlinear(CrossNonlinearOp::Mul) => width.mul(a0, a1),
            Self::Nonlinear(CrossNonlinearOp::MulEqz) => width.mul(a0, width.eqz(a1)),
            Self::Nonlinear(CrossNonlinearOp::AddEqz) => width.add(a0, width.eqz(a1)),
            Self::Nonlinear(CrossNonlinearOp::MulEqzPair) => {
                width.mul(width.eqz(a0), width.eqz(a1))
            }
        }
    }
}

#[derive(Clone, Debug)]
enum MultiViewRuntimeDriver {
    Lfsr {
        taps: Vec<u64>,
        init: Vec<u64>,
        window: Vec<u64>,
    },
    PeriodicMotif {
        motif: Vec<u64>,
        idx: usize,
    },
    AffineLag1 {
        mul_a: u64,
        add_b: u64,
        init: u64,
        state: u64,
    },
}

impl MultiViewRuntimeDriver {
    fn from_config(width: Width, driver: &MultiViewDriver) -> Result<Self, GeneratorError> {
        match driver {
            MultiViewDriver::Lfsr { stage } => {
                if stage.taps.is_empty() || stage.taps.len() != stage.init.len() {
                    return Err(GeneratorError::InvalidLfsrLengths {
                        taps_len: stage.taps.len(),
                        init_len: stage.init.len(),
                    });
                }
                let taps: Vec<u64> = stage.taps.iter().map(|&c| width.normalize(c)).collect();
                let init: Vec<u64> = stage.init.iter().map(|&x| width.normalize(x)).collect();
                Ok(Self::Lfsr {
                    taps,
                    window: init.clone(),
                    init,
                })
            }
            MultiViewDriver::PeriodicMotif { motif } => {
                if motif.is_empty() {
                    return Err(GeneratorError::EmptyMotif);
                }
                let norm: Vec<u64> = motif.iter().map(|&x| width.normalize(x)).collect();
                Ok(Self::PeriodicMotif {
                    motif: norm,
                    idx: 0,
                })
            }
            MultiViewDriver::AffineLag1 { mul_a, add_b, init } => {
                let init_norm = width.normalize(*init);
                Ok(Self::AffineLag1 {
                    mul_a: width.normalize(*mul_a),
                    add_b: width.normalize(*add_b),
                    init: init_norm,
                    state: init_norm,
                })
            }
        }
    }

    fn state_key_vec(&self) -> Vec<u64> {
        match self {
            Self::Lfsr { window, .. } => window.clone(),
            Self::PeriodicMotif { idx, .. } => vec![*idx as u64],
            Self::AffineLag1 { state, .. } => vec![*state],
        }
    }

    fn reset(&mut self) {
        match self {
            Self::Lfsr { init, window, .. } => window.copy_from_slice(init),
            Self::PeriodicMotif { idx, .. } => *idx = 0,
            Self::AffineLag1 { init, state, .. } => *state = *init,
        }
    }

    fn step(&mut self, width: Width) -> u64 {
        match self {
            Self::Lfsr { taps, window, .. } => TierGenerator::step_lfsr_window(width, taps, window),
            Self::PeriodicMotif { motif, idx } => {
                let sym = motif[*idx];
                *idx = (*idx + 1) % motif.len();
                sym
            }
            Self::AffineLag1 {
                mul_a,
                add_b,
                state,
                ..
            } => {
                let sym = *state;
                *state = width.add(width.mul(*mul_a, *state), *add_b);
                sym
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct T2StateKey {
    window_0: Vec<u64>,
    window_1: Vec<u64>,
    hist_0: VecDeque<u64>,
    hist_1: VecDeque<u64>,
}

#[derive(Clone, Debug)]
enum RuntimeKind {
    Constant {
        val: u64,
    },
    Alternating {
        a: u64,
        b: u64,
        phase: bool,
    },
    PeriodicMotif {
        motif: Vec<u64>,
        idx: usize,
    },
    AffineLag1 {
        mul_a: u64,
        add_b: u64,
        init: u64,
        state: u64,
    },
    CounterPow2 {
        log2_modulus: u8,
        mod_mask: u64,
        init: u64,
        state: u64,
    },
    CounterModN {
        modulus: u64,
        init: u64,
        state: u64,
    },
    Lfsr {
        taps: Vec<u64>,
        init: Vec<u64>,
        /// `[s(t), s(t+1), ..., s(t+L-1)]`.
        window: Vec<u64>,
    },
    LfsrCurriculum {
        stages: Vec<LfsrStage>,
        ticks_per_stage: u64,
        stage_idx: usize,
        stage_tick: u64,
        window: Vec<u64>,
    },
    T2DelayedAffine {
        taps: Vec<u64>,
        init: Vec<u64>,
        window: Vec<u64>,
        lag: usize,
        hist: VecDeque<u64>,
        mul_a: u64,
        add_b: u64,
    },
    T2DualDriver {
        taps_0: Vec<u64>,
        init_0: Vec<u64>,
        window_0: Vec<u64>,
        taps_1: Vec<u64>,
        init_1: Vec<u64>,
        window_1: Vec<u64>,
        lag_0: usize,
        hist_0: VecDeque<u64>,
        lag_1: usize,
        hist_1: VecDeque<u64>,
        op: T2DualOp,
    },
    T2MultiViewCoupled {
        driver: MultiViewRuntimeDriver,
        view_0: MultiViewTransform,
        view_1: MultiViewTransform,
        target: MultiViewTarget,
        max_delay: usize,
        hist: VecDeque<u64>,
    },
}

/// Concrete single-channel runtime instance of a [`GeneratorConfig`].
#[derive(Clone, Debug)]
pub struct TierGenerator {
    label: String,
    width: Width,
    kind: RuntimeKind,
}

impl TierGenerator {
    fn step_lfsr_window(width: Width, taps: &[u64], window: &mut [u64]) -> u64 {
        let emitted = window[0];
        let len = taps.len();
        let mut feedback = 0u64;
        for (i, &c) in taps.iter().enumerate() {
            // c = taps[i] corresponds to c_{i+1} multiplying s(t + L - (i + 1)) = window[len - 1 - i].
            let term = width.mul(c, window[len - 1 - i]);
            feedback = width.add(feedback, term);
        }
        window.copy_within(1..len, 0);
        window[len - 1] = feedback;
        emitted
    }

    fn t2_state_key(&self) -> Option<T2StateKey> {
        match &self.kind {
            RuntimeKind::T2DelayedAffine { window, hist, .. } => Some(T2StateKey {
                window_0: window.clone(),
                window_1: Vec::new(),
                hist_0: hist.clone(),
                hist_1: VecDeque::new(),
            }),
            RuntimeKind::T2DualDriver {
                window_0,
                window_1,
                hist_0,
                hist_1,
                ..
            } => Some(T2StateKey {
                window_0: window_0.clone(),
                window_1: window_1.clone(),
                hist_0: hist_0.clone(),
                hist_1: hist_1.clone(),
            }),
            RuntimeKind::T2MultiViewCoupled { driver, hist, .. } => Some(T2StateKey {
                window_0: driver.state_key_vec(),
                window_1: Vec::new(),
                hist_0: hist.clone(),
                hist_1: VecDeque::new(),
            }),
            _ => None,
        }
    }

    fn export_t2_finite_state_spec(&self) -> Result<FiniteStateSpec, SpecExportError> {
        let channels = self.channels();
        let mut clone = self.clone();
        clone.reset();

        let mut state_to_idx: BTreeMap<T2StateKey, usize> = BTreeMap::new();
        let init_key = clone
            .t2_state_key()
            .expect("export_t2_finite_state_spec only called on T2 generators");
        state_to_idx.insert(init_key, 0);

        let mut transitions: Vec<(usize, Vec<u64>)> = Vec::new();
        let mut frame = vec![0u64; usize::from(channels)];

        loop {
            clone.next_clean_frame(&mut frame);
            let next_key = clone
                .t2_state_key()
                .expect("export_t2_finite_state_spec only called on T2 generators");
            if let Some(&existing) = state_to_idx.get(&next_key) {
                transitions.push((existing, frame.clone()));
                break;
            }
            if state_to_idx.len() >= DEFAULT_MAX_EXPORT_STATES {
                return Err(SpecExportError::StateSpaceTooLarge {
                    estimated_states: (DEFAULT_MAX_EXPORT_STATES + 1) as u128,
                    limit: DEFAULT_MAX_EXPORT_STATES,
                });
            }
            let new_idx = state_to_idx.len();
            state_to_idx.insert(next_key, new_idx);
            transitions.push((new_idx, frame.clone()));
        }

        Ok(FiniteStateSpec {
            width: self.width,
            channels,
            num_states: transitions.len(),
            initial_state: 0,
            transitions,
            scope: EpsilonMachineScope::StationaryProcess,
        })
    }
}

impl FiniteStateGenerator for TierGenerator {
    fn name(&self) -> &str {
        &self.label
    }

    fn width(&self) -> Width {
        self.width
    }

    fn channels(&self) -> u16 {
        match &self.kind {
            RuntimeKind::T2DelayedAffine { .. } => 2,
            RuntimeKind::T2DualDriver { .. } | RuntimeKind::T2MultiViewCoupled { .. } => 3,
            _ => 1,
        }
    }

    fn reset(&mut self) {
        match &mut self.kind {
            RuntimeKind::Constant { .. } => {}
            RuntimeKind::Alternating { phase, .. } => *phase = false,
            RuntimeKind::PeriodicMotif { idx, .. } => *idx = 0,
            RuntimeKind::AffineLag1 { init, state, .. } => *state = *init,
            RuntimeKind::CounterPow2 { init, state, .. } => *state = *init,
            RuntimeKind::CounterModN { init, state, .. } => *state = *init,
            RuntimeKind::Lfsr { init, window, .. } => window.clone_from(init),
            RuntimeKind::LfsrCurriculum {
                stages,
                stage_idx,
                stage_tick,
                window,
                ..
            } => {
                *stage_idx = 0;
                *stage_tick = 0;
                window.clone_from(&stages[0].init);
            }
            RuntimeKind::T2DelayedAffine {
                init,
                window,
                lag,
                hist,
                ..
            } => {
                window.clone_from(init);
                hist.clear();
                hist.resize(*lag, 0);
            }
            RuntimeKind::T2DualDriver {
                init_0,
                window_0,
                init_1,
                window_1,
                lag_0,
                hist_0,
                lag_1,
                hist_1,
                ..
            } => {
                window_0.clone_from(init_0);
                window_1.clone_from(init_1);
                hist_0.clear();
                hist_0.resize(*lag_0, 0);
                hist_1.clear();
                hist_1.resize(*lag_1, 0);
            }
            RuntimeKind::T2MultiViewCoupled {
                driver,
                max_delay,
                hist,
                ..
            } => {
                driver.reset();
                hist.clear();
                hist.resize(*max_delay, 0);
            }
        }
    }

    fn next_clean_frame(&mut self, out: &mut [u64]) {
        assert_eq!(out.len(), usize::from(self.channels()));
        let w = self.width;
        match &mut self.kind {
            RuntimeKind::T2DelayedAffine {
                taps,
                window,
                hist,
                mul_a,
                add_b,
                ..
            } => {
                let a0 = Self::step_lfsr_window(w, taps, window);
                let a_delayed = hist.pop_front().expect("T2 lag validated >= 1");
                hist.push_back(a0);
                out[0] = a0;
                out[1] = w.add(w.mul(*mul_a, a_delayed), *add_b);
            }
            RuntimeKind::T2DualDriver {
                taps_0,
                window_0,
                taps_1,
                window_1,
                hist_0,
                hist_1,
                op,
                ..
            } => {
                let a0 = Self::step_lfsr_window(w, taps_0, window_0);
                let a1 = Self::step_lfsr_window(w, taps_1, window_1);
                let d0 = hist_0.pop_front().expect("T2 lag_0 validated >= 1");
                hist_0.push_back(a0);
                let d1 = hist_1.pop_front().expect("T2 lag_1 validated >= 1");
                hist_1.push_back(a1);
                out[0] = a0;
                out[1] = a1;
                out[2] = op.eval(w, d0, d1);
            }
            RuntimeKind::T2MultiViewCoupled {
                driver,
                view_0,
                view_1,
                target,
                max_delay,
                hist,
            } => {
                let z_now = driver.step(w);
                let z_at = |lag: usize| -> u64 {
                    if lag == 0 {
                        z_now
                    } else {
                        hist[*max_delay - lag]
                    }
                };
                out[0] = view_0.eval(w, z_at(view_0.delay));
                out[1] = view_1.eval(w, z_at(view_1.delay));
                out[2] = match *target {
                    MultiViewTarget::Affine { lag, .. } | MultiViewTarget::Eqz { lag, .. } => {
                        target.eval(w, z_at(lag), 0)
                    }
                    MultiViewTarget::AddEqz { lag_a, lag_b }
                    | MultiViewTarget::MulEqz { lag_a, lag_b }
                    | MultiViewTarget::MulEqzPair { lag_a, lag_b } => {
                        target.eval(w, z_at(lag_a), z_at(lag_b))
                    }
                };
                if *max_delay > 0 {
                    hist.pop_front();
                    hist.push_back(z_now);
                }
            }
            other => {
                out[0] = match other {
                    RuntimeKind::Constant { val } => *val,
                    RuntimeKind::Alternating { a, b, phase } => {
                        let sym = if *phase { *b } else { *a };
                        *phase = !*phase;
                        sym
                    }
                    RuntimeKind::PeriodicMotif { motif, idx } => {
                        let sym = motif[*idx];
                        *idx = (*idx + 1) % motif.len();
                        sym
                    }
                    RuntimeKind::AffineLag1 {
                        mul_a,
                        add_b,
                        state,
                        ..
                    } => {
                        let sym = *state;
                        *state = w.add(w.mul(*mul_a, *state), *add_b);
                        sym
                    }
                    RuntimeKind::CounterPow2 {
                        mod_mask, state, ..
                    } => {
                        let sym = *state;
                        *state = state.wrapping_add(1) & *mod_mask;
                        sym
                    }
                    RuntimeKind::CounterModN { modulus, state, .. } => {
                        let sym = *state;
                        *state = (*state + 1) % *modulus;
                        sym
                    }
                    RuntimeKind::Lfsr { taps, window, .. } => {
                        Self::step_lfsr_window(w, taps, window)
                    }
                    RuntimeKind::LfsrCurriculum {
                        stages,
                        ticks_per_stage,
                        stage_idx,
                        stage_tick,
                        window,
                    } => {
                        let sym = Self::step_lfsr_window(w, &stages[*stage_idx].taps, window);
                        *stage_tick += 1;
                        if *stage_tick >= *ticks_per_stage && *stage_idx + 1 < stages.len() {
                            *stage_idx += 1;
                            *stage_tick = 0;
                            window.clone_from(&stages[*stage_idx].init);
                        }
                        sym
                    }
                    RuntimeKind::T2DelayedAffine { .. }
                    | RuntimeKind::T2DualDriver { .. }
                    | RuntimeKind::T2MultiViewCoupled { .. } => {
                        unreachable!("handled above")
                    }
                };
            }
        }
    }

    fn to_finite_state_spec(&self) -> Result<FiniteStateSpec, SpecExportError> {
        let w = self.width;
        match &self.kind {
            RuntimeKind::Constant { val } => Ok(FiniteStateSpec {
                width: w,
                channels: 1,
                num_states: 1,
                initial_state: 0,
                transitions: vec![(0, vec![*val])],
                scope: EpsilonMachineScope::StationaryProcess,
            }),
            RuntimeKind::Alternating { a, b, .. } => Ok(FiniteStateSpec {
                width: w,
                channels: 1,
                num_states: 2,
                initial_state: 0,
                transitions: vec![(1, vec![*a]), (0, vec![*b])],
                scope: EpsilonMachineScope::StationaryProcess,
            }),
            RuntimeKind::PeriodicMotif { motif, .. } => {
                let p = motif.len();
                if p > DEFAULT_MAX_EXPORT_STATES {
                    return Err(SpecExportError::StateSpaceTooLarge {
                        estimated_states: p as u128,
                        limit: DEFAULT_MAX_EXPORT_STATES,
                    });
                }
                let transitions = (0..p).map(|i| ((i + 1) % p, vec![motif[i]])).collect();
                Ok(FiniteStateSpec {
                    width: w,
                    channels: 1,
                    num_states: p,
                    initial_state: 0,
                    transitions,
                    scope: EpsilonMachineScope::StationaryProcess,
                })
            }
            RuntimeKind::AffineLag1 {
                mul_a, add_b, init, ..
            } => {
                let card = w.cardinality();
                if card > DEFAULT_MAX_EXPORT_STATES as u128 {
                    return Err(SpecExportError::StateSpaceTooLarge {
                        estimated_states: card,
                        limit: DEFAULT_MAX_EXPORT_STATES,
                    });
                }
                let n = card as usize;
                let transitions = (0..n)
                    .map(|s| {
                        let next = w.add(w.mul(*mul_a, s as u64), *add_b) as usize;
                        (next, vec![s as u64])
                    })
                    .collect();
                Ok(FiniteStateSpec {
                    width: w,
                    channels: 1,
                    num_states: n,
                    initial_state: *init as usize,
                    transitions,
                    scope: EpsilonMachineScope::StationaryProcess,
                })
            }
            RuntimeKind::CounterPow2 {
                log2_modulus,
                mod_mask,
                init,
                ..
            } => {
                let card = 1u128 << *log2_modulus;
                if card > DEFAULT_MAX_EXPORT_STATES as u128 {
                    return Err(SpecExportError::StateSpaceTooLarge {
                        estimated_states: card,
                        limit: DEFAULT_MAX_EXPORT_STATES,
                    });
                }
                let n = card as usize;
                let transitions = (0..n)
                    .map(|s| {
                        (
                            ((s as u64).wrapping_add(1) & *mod_mask) as usize,
                            vec![s as u64],
                        )
                    })
                    .collect();
                Ok(FiniteStateSpec {
                    width: w,
                    channels: 1,
                    num_states: n,
                    initial_state: *init as usize,
                    transitions,
                    scope: EpsilonMachineScope::StationaryProcess,
                })
            }
            RuntimeKind::CounterModN { modulus, init, .. } => {
                if u128::from(*modulus) > DEFAULT_MAX_EXPORT_STATES as u128 {
                    return Err(SpecExportError::StateSpaceTooLarge {
                        estimated_states: u128::from(*modulus),
                        limit: DEFAULT_MAX_EXPORT_STATES,
                    });
                }
                let n = *modulus as usize;
                let transitions = (0..n).map(|s| ((s + 1) % n, vec![s as u64])).collect();
                Ok(FiniteStateSpec {
                    width: w,
                    channels: 1,
                    num_states: n,
                    initial_state: *init as usize,
                    transitions,
                    scope: EpsilonMachineScope::StationaryProcess,
                })
            }
            RuntimeKind::Lfsr { taps, init, .. } => {
                let len = taps.len();
                let total_bits = u32::from(w.bits()).saturating_mul(len as u32);
                if total_bits >= 128 {
                    return Err(SpecExportError::StateSpaceTooLarge {
                        estimated_states: u128::MAX,
                        limit: DEFAULT_MAX_EXPORT_STATES,
                    });
                }
                let total_states = 1u128 << total_bits;
                if total_states > DEFAULT_MAX_EXPORT_STATES as u128 {
                    return Err(SpecExportError::StateSpaceTooLarge {
                        estimated_states: total_states,
                        limit: DEFAULT_MAX_EXPORT_STATES,
                    });
                }
                let n = total_states as usize;
                let k = usize::from(w.bits());
                let mask = w.mask();

                let pack = |win: &[u64]| -> usize {
                    let mut idx = 0usize;
                    for &sym in win {
                        idx = (idx << k) | ((sym & mask) as usize);
                    }
                    idx
                };

                let mut win = vec![0u64; len];
                let mut transitions = Vec::with_capacity(n);
                for state_idx in 0..n {
                    let mut tmp = state_idx;
                    for slot in win.iter_mut().rev() {
                        *slot = (tmp as u64) & mask;
                        tmp >>= k;
                    }
                    let emitted = Self::step_lfsr_window(w, taps, &mut win);
                    let next_idx = pack(&win);
                    transitions.push((next_idx, vec![emitted]));
                }

                Ok(FiniteStateSpec {
                    width: w,
                    channels: 1,
                    num_states: n,
                    initial_state: pack(init),
                    transitions,
                    scope: EpsilonMachineScope::StationaryProcess,
                })
            }
            RuntimeKind::LfsrCurriculum {
                stages,
                ticks_per_stage,
                ..
            } => {
                // Explicitly tag that only the final target LFSR stage is stationary.
                let last = stages.last().expect("validated non-empty");
                let mut spec = GeneratorConfig::T3c_Lfsr {
                    taps: last.taps.clone(),
                    init: last.init.clone(),
                }
                .build(w)
                .expect("already validated")
                .to_finite_state_spec()?;
                spec.scope = EpsilonMachineScope::CurriculumFinalStage {
                    stage_index: stages.len() - 1,
                    total_stages: stages.len(),
                    ticks_per_stage: *ticks_per_stage,
                };
                Ok(spec)
            }
            RuntimeKind::T2DelayedAffine { .. }
            | RuntimeKind::T2DualDriver { .. }
            | RuntimeKind::T2MultiViewCoupled { .. } => self.export_t2_finite_state_spec(),
        }
    }
}

/// Projects a multi-channel [`FiniteStateGenerator`] onto a single target channel ($K = 1$).
///
/// Defaults to the final channel (`inner.channels() - 1`) for G2A.1(iii) marginal controls.
#[derive(Clone, Debug)]
pub struct TargetChannelGenerator<G> {
    inner: G,
    channel: u16,
    label: String,
    scratch: Vec<u64>,
}

impl<G: FiniteStateGenerator> TargetChannelGenerator<G> {
    /// Projects `inner` onto its final channel (`inner.channels() - 1`).
    ///
    /// # Panics
    ///
    /// Panics if `inner.channels() == 0`.
    #[must_use]
    pub fn new(inner: G) -> Self {
        let n_ch = inner.channels();
        assert!(
            n_ch >= 1,
            "TargetChannelGenerator requires inner.channels() >= 1"
        );
        Self::with_channel(inner, n_ch - 1)
    }

    /// Projects `inner` onto a specific 0-based `channel`.
    ///
    /// # Panics
    ///
    /// Panics if `channel >= inner.channels()`.
    #[must_use]
    pub fn with_channel(inner: G, channel: u16) -> Self {
        let n_ch = inner.channels();
        assert!(
            channel < n_ch,
            "TargetChannelGenerator channel {channel} out of range 0..{n_ch}"
        );
        let label = format!("{}.ch{channel}", inner.name());
        Self {
            inner,
            channel,
            label,
            scratch: vec![0u64; usize::from(n_ch)],
        }
    }

    /// Shared reference to the underlying multi-channel generator.
    #[must_use]
    pub fn inner(&self) -> &G {
        &self.inner
    }
}

impl<G: FiniteStateGenerator> FiniteStateGenerator for TargetChannelGenerator<G> {
    fn name(&self) -> &str {
        &self.label
    }

    fn width(&self) -> Width {
        self.inner.width()
    }

    fn channels(&self) -> u16 {
        1
    }

    fn reset(&mut self) {
        self.inner.reset();
    }

    fn next_clean_frame(&mut self, out: &mut [u64]) {
        assert_eq!(out.len(), 1);
        self.inner.next_clean_frame(&mut self.scratch);
        out[0] = self.scratch[usize::from(self.channel)];
    }

    fn to_finite_state_spec(&self) -> Result<FiniteStateSpec, SpecExportError> {
        let mut spec = self.inner.to_finite_state_spec()?;
        let ch_idx = usize::from(self.channel);
        for (_, emission) in &mut spec.transitions {
            let sym = emission[ch_idx];
            *emission = vec![sym];
        }
        spec.channels = 1;
        Ok(spec)
    }
}

/// Composes independent finite-state generators of matching symbol width into a multi-channel frame generator.
#[derive(Clone, Debug)]
pub struct MultiChannelProductGenerator<G = TierGenerator> {
    generators: Vec<G>,
    width: Width,
    channels: u16,
    label: String,
}

impl<G: FiniteStateGenerator> MultiChannelProductGenerator<G> {
    /// Composes `generators` in channel order (flattening each generator's channels).
    ///
    /// # Errors
    ///
    /// Returns [`GeneratorError::InvalidChannelComposition`] if `generators` is empty,
    /// any generator has zero channels, widths differ, or total channels exceeds `u16::MAX`.
    pub fn new(generators: Vec<G>) -> Result<Self, GeneratorError> {
        let Some(first) = generators.first() else {
            return Err(GeneratorError::InvalidChannelComposition);
        };
        let width = first.width();
        let mut total_channels = 0u16;
        let mut names = Vec::with_capacity(generators.len());
        for g in &generators {
            if g.channels() == 0 || g.width() != width {
                return Err(GeneratorError::InvalidChannelComposition);
            }
            total_channels = total_channels
                .checked_add(g.channels())
                .ok_or(GeneratorError::InvalidChannelComposition)?;
            names.push(g.name().to_owned());
        }
        let label = format!("Product({})", names.join("+"));
        Ok(Self {
            generators,
            width,
            channels: total_channels,
            label,
        })
    }
}

impl<G: FiniteStateGenerator> FiniteStateGenerator for MultiChannelProductGenerator<G> {
    fn name(&self) -> &str {
        &self.label
    }

    fn width(&self) -> Width {
        self.width
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn reset(&mut self) {
        for g in &mut self.generators {
            g.reset();
        }
    }

    fn next_clean_frame(&mut self, out: &mut [u64]) {
        assert_eq!(out.len(), usize::from(self.channels));
        let mut offset = 0usize;
        for g in &mut self.generators {
            let n = usize::from(g.channels());
            g.next_clean_frame(&mut out[offset..offset + n]);
            offset += n;
        }
    }

    fn to_finite_state_spec(&self) -> Result<FiniteStateSpec, SpecExportError> {
        let specs: Vec<FiniteStateSpec> = self
            .generators
            .iter()
            .map(FiniteStateGenerator::to_finite_state_spec)
            .collect::<Result<Vec<_>, _>>()?;
        let estimated_states = specs
            .iter()
            .fold(1u128, |acc, s| acc.saturating_mul(s.num_states as u128));

        let mut state_to_idx: BTreeMap<Vec<usize>, usize> = BTreeMap::new();
        let mut cur_state: Vec<usize> = specs.iter().map(|s| s.initial_state).collect();
        state_to_idx.insert(cur_state.clone(), 0);
        let mut transitions: Vec<(usize, Vec<u64>)> = Vec::new();

        loop {
            let mut next_state = Vec::with_capacity(specs.len());
            let mut emission = Vec::with_capacity(usize::from(self.channels));
            for (spec, &s_idx) in specs.iter().zip(cur_state.iter()) {
                let (ns, sym_slice) = &spec.transitions[s_idx];
                next_state.push(*ns);
                emission.extend_from_slice(sym_slice);
            }
            if let Some(&existing) = state_to_idx.get(&next_state) {
                transitions.push((existing, emission));
                break;
            }
            if state_to_idx.len() >= DEFAULT_MAX_EXPORT_STATES {
                return Err(SpecExportError::StateSpaceTooLarge {
                    estimated_states: estimated_states.max((DEFAULT_MAX_EXPORT_STATES + 1) as u128),
                    limit: DEFAULT_MAX_EXPORT_STATES,
                });
            }
            let new_idx = state_to_idx.len();
            state_to_idx.insert(next_state.clone(), new_idx);
            transitions.push((new_idx, emission));
            cur_state = next_state;
        }

        let scope = if specs
            .iter()
            .all(|s| s.scope == EpsilonMachineScope::StationaryProcess)
        {
            EpsilonMachineScope::StationaryProcess
        } else {
            specs[0].scope
        };

        Ok(FiniteStateSpec {
            width: self.width,
            channels: self.channels,
            num_states: transitions.len(),
            initial_state: 0,
            transitions,
            scope,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::{NoiseModel, NoisyEnvironment, solve_epsilon_machine};
    use crate::rng::SeedBank;

    fn assert_spec_and_single_view_parity(
        cfg: &GeneratorConfig,
        width: Width,
        ticks: usize,
        verify_spec: bool,
    ) {
        let mut generator = cfg.build(width).expect("valid T2_MultiViewCoupled");
        assert_eq!(generator.channels(), 3);
        let (view_0, view_1, target) = match cfg {
            GeneratorConfig::T2_MultiViewCoupled {
                view_0,
                view_1,
                target,
                ..
            } => (
                view_0.normalized(width),
                view_1.normalized(width),
                target.normalized(width),
            ),
            other => panic!("expected T2_MultiViewCoupled, got {other:?}"),
        };

        let mut ch0_hist = Vec::with_capacity(ticks);
        let mut ch1_hist = Vec::with_capacity(ticks);
        let mut observed = Vec::with_capacity(ticks);
        let mut frame = [0u64; 3];

        for t in 0..ticks {
            generator.next_clean_frame(&mut frame);
            let pred_from_ch0 = target.predict_from_view_history(width, view_0, &ch0_hist);
            let pred_from_ch1 = target.predict_from_view_history(width, view_1, &ch1_hist);
            assert_eq!(
                pred_from_ch0, frame[2],
                "tick {t}: ch0 single-view prediction mismatch"
            );
            assert_eq!(
                pred_from_ch1, frame[2],
                "tick {t}: ch1 single-view prediction mismatch"
            );
            ch0_hist.push(frame[0]);
            ch1_hist.push(frame[1]);
            observed.push(frame.to_vec());
        }

        // Verify reset() determinism.
        generator.reset();
        for (t, expected) in observed.iter().enumerate() {
            generator.next_clean_frame(&mut frame);
            assert_eq!(&frame[..], &expected[..], "reset mismatch at tick {t}");
        }

        if verify_spec {
            let spec = generator.to_finite_state_spec().expect("finite state spec");
            assert_eq!(spec.channels, 3);
            assert_eq!(spec.width, width);
            let mut state = spec.initial_state;
            for (t, expected) in observed.iter().enumerate() {
                let (next_state, emission) = &spec.transitions[state];
                assert_eq!(
                    emission, expected,
                    "FiniteStateSpec emission mismatch at tick {t}"
                );
                state = *next_state;
            }
            let clean_summary = solve_epsilon_machine(&spec, &NoiseModel::None);
            assert_eq!(clean_summary.h_mu_bits_per_tick, 0.0);
            assert!(clean_summary.causal_state_count >= 1);
        }
    }

    #[test]
    fn multiview_validation_rejects_non_invertible_duplicate_and_non_causal_configs() {
        let w1 = Width::new(1).expect("w1");
        let w2 = Width::new(2).expect("w2");
        let lfsr = MultiViewDriver::Lfsr {
            stage: LfsrStage {
                taps: vec![1, 1],
                init: vec![0, 1],
            },
        };
        let v0 = MultiViewTransform {
            delay: 0,
            mul_a: 1,
            add_b: 0,
        };
        let v1 = MultiViewTransform {
            delay: 1,
            mul_a: 3,
            add_b: 1,
        };
        let tgt = MultiViewTarget::Affine {
            lag: 2,
            mul_a: 1,
            add_b: 0,
        };

        let even_ch0 = GeneratorConfig::T2_MultiViewCoupled {
            driver: lfsr.clone(),
            view_0: MultiViewTransform {
                delay: 0,
                mul_a: 2,
                add_b: 0,
            },
            view_1: v1,
            target: tgt,
        };
        assert_eq!(
            even_ch0.build(w2).unwrap_err(),
            GeneratorError::NonInvertibleViewMultiplier {
                channel: 0,
                mul_a: 2,
            }
        );

        let even_ch1 = GeneratorConfig::T2_MultiViewCoupled {
            driver: lfsr.clone(),
            view_0: v0,
            view_1: MultiViewTransform {
                delay: 1,
                mul_a: 0,
                add_b: 1,
            },
            target: tgt,
        };
        assert_eq!(
            even_ch1.build(w1).unwrap_err(),
            GeneratorError::NonInvertibleViewMultiplier {
                channel: 1,
                mul_a: 0,
            }
        );

        // Distinct raw add_b that collide modulo 2^k must be rejected as DuplicateViewTransform.
        let dup_views = GeneratorConfig::T2_MultiViewCoupled {
            driver: lfsr.clone(),
            view_0: v0,
            view_1: MultiViewTransform {
                delay: 0,
                mul_a: 5,
                add_b: 4,
            },
            target: tgt,
        };
        assert_eq!(
            dup_views.build(w2).unwrap_err(),
            GeneratorError::DuplicateViewTransform
        );

        // Non-causal lag (lag == view_1.delay and lag_b <= view_1.delay).
        let non_causal_equal = GeneratorConfig::T2_MultiViewCoupled {
            driver: lfsr.clone(),
            view_0: v0,
            view_1: MultiViewTransform {
                delay: 2,
                mul_a: 1,
                add_b: 1,
            },
            target: MultiViewTarget::Affine {
                lag: 2,
                mul_a: 1,
                add_b: 0,
            },
        };
        assert_eq!(
            non_causal_equal.build(w2).unwrap_err(),
            GeneratorError::NonCausalMultiViewLag {
                channel: 1,
                view_delay: 2,
                target_lag: 2,
            }
        );

        let non_causal_binary = GeneratorConfig::T2_MultiViewCoupled {
            driver: lfsr,
            view_0: v0,
            view_1: MultiViewTransform {
                delay: 2,
                mul_a: 1,
                add_b: 1,
            },
            target: MultiViewTarget::AddEqz { lag_a: 5, lag_b: 1 },
        };
        assert_eq!(
            non_causal_binary.build(w2).unwrap_err(),
            GeneratorError::NonCausalMultiViewLag {
                channel: 1,
                view_delay: 2,
                target_lag: 1,
            }
        );

        let empty_motif = GeneratorConfig::T2_MultiViewCoupled {
            driver: MultiViewDriver::PeriodicMotif { motif: vec![] },
            view_0: v0,
            view_1: v1,
            target: tgt,
        };
        assert_eq!(
            empty_motif.build(w1).unwrap_err(),
            GeneratorError::EmptyMotif
        );

        let bad_lfsr = GeneratorConfig::T2_MultiViewCoupled {
            driver: MultiViewDriver::Lfsr {
                stage: LfsrStage {
                    taps: vec![1, 1],
                    init: vec![1],
                },
            },
            view_0: v0,
            view_1: v1,
            target: tgt,
        };
        assert_eq!(
            bad_lfsr.build(w1).unwrap_err(),
            GeneratorError::InvalidLfsrLengths {
                taps_len: 2,
                init_len: 1,
            }
        );
    }

    #[test]
    fn multiview_linear_and_nonlinear_targets_match_spec_and_single_view_across_widths() {
        let small_lfsr = MultiViewDriver::Lfsr {
            stage: LfsrStage {
                taps: vec![1, 0, 1],
                init: vec![1, 0, 1],
            },
        };
        let periodic = MultiViewDriver::PeriodicMotif {
            motif: vec![0, 1, 3, 2, 1],
        };
        let affine_lag1 = MultiViewDriver::AffineLag1 {
            mul_a: 1,
            add_b: 1,
            init: 0,
        };

        let targets = [
            MultiViewTarget::Affine {
                lag: 3,
                mul_a: 3,
                add_b: 1,
            },
            MultiViewTarget::Eqz { lag: 3, add_b: 2 },
            MultiViewTarget::AddEqz { lag_a: 3, lag_b: 4 },
            MultiViewTarget::MulEqz { lag_a: 4, lag_b: 3 },
            MultiViewTarget::MulEqzPair { lag_a: 3, lag_b: 4 },
        ];

        let view_0 = MultiViewTransform {
            delay: 0,
            mul_a: 1,
            add_b: 0,
        };
        let view_1 = MultiViewTransform {
            delay: 2,
            mul_a: 3,
            add_b: 1,
        };

        for k in [1u8, 2u8] {
            let width = Width::new(k).expect("valid width");
            for driver in [&small_lfsr, &periodic, &affine_lag1] {
                for &target in &targets {
                    let cfg = GeneratorConfig::T2_MultiViewCoupled {
                        driver: driver.clone(),
                        view_0,
                        view_1,
                        target,
                    };
                    assert_spec_and_single_view_parity(&cfg, width, 40, true);
                }
            }
        }
    }

    #[test]
    fn multiview_high_degree_lfsr_and_serde_and_spec_bound() {
        let w1 = Width::new(1).expect("w1");
        let w2 = Width::new(2).expect("w2");
        // Degree-11 primitive LFSR driver (matches G2B.2/G2B.5 high-degree driver scale).
        let mut taps11 = vec![0u64; 11];
        taps11[1] = 1;
        taps11[10] = 1;
        let mut init11 = vec![0u64; 11];
        init11[0] = 1;
        init11[5] = 1;
        let cfg11 = GeneratorConfig::T2_MultiViewCoupled {
            driver: MultiViewDriver::Lfsr {
                stage: LfsrStage {
                    taps: taps11,
                    init: init11,
                },
            },
            view_0: MultiViewTransform {
                delay: 0,
                mul_a: 1,
                add_b: 0,
            },
            view_1: MultiViewTransform {
                delay: 1,
                mul_a: 3,
                add_b: 2,
            },
            target: MultiViewTarget::AddEqz { lag_a: 5, lag_b: 6 },
        };

        // Serde roundtrip + label sanity.
        let json = serde_json::to_string(&cfg11).expect("serialize T2_MultiViewCoupled");
        let parsed: GeneratorConfig =
            serde_json::from_str(&json).expect("deserialize T2_MultiViewCoupled");
        assert_eq!(parsed, cfg11);
        assert!(
            cfg11
                .build(w2)
                .unwrap()
                .name()
                .starts_with("T2_MultiViewCoupled(")
        );

        // Verify single-view causal predictability on high-degree driver for k=1 and k=2.
        assert_spec_and_single_view_parity(&cfg11, w1, 64, true);
        assert_spec_and_single_view_parity(&cfg11, w2, 64, false);

        // Exceeding DEFAULT_MAX_EXPORT_STATES returns StateSpaceTooLarge cleanly:
        // x^17 + x^3 + 1 primitive LFSR over F_2 has period 2^17 - 1 = 131_071 > 65_536.
        let mut taps17 = vec![0u64; 17];
        taps17[2] = 1;
        taps17[16] = 1;
        let mut init17 = vec![0u64; 17];
        init17[0] = 1;
        let cfg17 = GeneratorConfig::T2_MultiViewCoupled {
            driver: MultiViewDriver::Lfsr {
                stage: LfsrStage {
                    taps: taps17,
                    init: init17,
                },
            },
            view_0: MultiViewTransform {
                delay: 0,
                mul_a: 1,
                add_b: 0,
            },
            view_1: MultiViewTransform {
                delay: 0,
                mul_a: 1,
                add_b: 1,
            },
            target: MultiViewTarget::Affine {
                lag: 2,
                mul_a: 1,
                add_b: 0,
            },
        };
        let gen17 = cfg17.build(w1).expect("valid deg-17 multiview");
        assert!(matches!(
            gen17.to_finite_state_spec(),
            Err(SpecExportError::StateSpaceTooLarge { .. })
        ));
    }

    #[test]
    fn multiview_with_channel_fault_schedule_recovers_cleanly_from_surviving_view() {
        let width = Width::new(2).expect("w2");
        let cfg = GeneratorConfig::T2_MultiViewCoupled {
            driver: MultiViewDriver::PeriodicMotif {
                motif: vec![0, 3, 2, 1],
            },
            view_0: MultiViewTransform {
                delay: 0,
                mul_a: 1,
                add_b: 0,
            },
            view_1: MultiViewTransform {
                delay: 0,
                mul_a: 3,
                add_b: 1,
            },
            target: MultiViewTarget::Affine {
                lag: 3,
                mul_a: 1,
                add_b: 0,
            },
        };
        let view_1 = MultiViewTransform {
            delay: 0,
            mul_a: 3,
            add_b: 1,
        };
        let target = MultiViewTarget::Affine {
            lag: 3,
            mul_a: 1,
            add_b: 0,
        };

        let fault_tick = 100u64;
        let noise = NoiseModel::ChannelFaultSchedule {
            base_epsilon: 0.0,
            base_channels: vec![],
            fault_tick,
            fault_epsilon: 0.75,
            fault_channels: vec![0],
        };
        let mut env = NoisyEnvironment::new(
            cfg.build(width).expect("valid config"),
            noise,
            SeedBank::new(42),
        )
        .expect("valid fault schedule");

        let mut ch1_hist = Vec::new();
        let mut clean = [0u64; 3];
        let mut obs = [0u64; 3];
        let mut ch0_mismatches_post_fault = 0usize;

        for t in 0..300u64 {
            env.step_frame(&mut clean, &mut obs);
            if t < fault_tick {
                assert_eq!(obs, clean, "pre-fault frame must be clean at t={t}");
            } else {
                assert_eq!(obs[1], clean[1], "ch1 stays clean post-fault at t={t}");
                assert_eq!(obs[2], clean[2], "ch2 stays clean post-fault at t={t}");
                if obs[0] != clean[0] {
                    ch0_mismatches_post_fault += 1;
                }
            }
            let pred = target.predict_from_view_history(width, view_1, &ch1_hist);
            assert_eq!(
                pred, clean[2],
                "ch1-only prediction must stay exact at t={t}"
            );
            ch1_hist.push(obs[1]);
        }
        assert!(ch0_mismatches_post_fault > 50);
    }
}
