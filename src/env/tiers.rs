//! Benchmark generator catalogue for Milestone 0: Tiers **T1**, **T3b**, and **T3c** (Charter §5.3).

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::env::generator::{
    DEFAULT_MAX_EXPORT_STATES, EpsilonMachineScope, FiniteStateGenerator, FiniteStateSpec,
    SpecExportError,
};
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
        }
    }
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
        };

        Ok(TierGenerator {
            label: self.default_label(),
            width,
            kind,
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
}

impl FiniteStateGenerator for TierGenerator {
    fn name(&self) -> &str {
        &self.label
    }

    fn width(&self) -> Width {
        self.width
    }

    fn channels(&self) -> u16 {
        1
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
        }
    }

    fn next_clean_frame(&mut self, out: &mut [u64]) {
        assert_eq!(out.len(), 1);
        let w = self.width;
        out[0] = match &mut self.kind {
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
            RuntimeKind::Lfsr { taps, window, .. } => Self::step_lfsr_window(w, taps, window),
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
        };
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
        }
    }
}
