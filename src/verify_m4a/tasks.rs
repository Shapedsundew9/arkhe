//! M4a-0 task registry and controls across `k in {1, 2}` and `Clean` / `Noisy` (`§2.1`).
//!
//! Defines the three task families:
//! - `SP` (spontaneous `H5`/`H6` tasks): `G2B.2`-class multi-view `T2` (periodic grounded and LFSR
//!   cross-channel), `K = 1` period-6 lag redundancy (Charter §1.3), and `G2B.5`-style channel fault.
//! - `PL` (planted spurious-cycle rejection): `PL1` short-cycle alt and `PL2` rare-context alt.
//! - `CT` (preservation & non-inferiority controls): `CT1` exogenous LFSR cross-channel (`R4`),
//!   `CT2` rare-event-dependent oracle-correct structure, and `CT3` no-comparator task (`I2`).

use crate::env::{
    CrossNonlinearOp, FiniteStateGenerator, FiniteStateSpec, GeneratorConfig, GeneratorError,
    LfsrStage, MultiViewDriver, MultiViewTarget, MultiViewTransform, NoiseError, NoiseModel,
    NoisyEnvironment, SpecExportError, TierGenerator,
};
use crate::rng::SeedBank;
use crate::substrate::{Width, WireDelayMode};
use crate::verify_m4a::oracle::PortLanguageOracle;

/// Default spontaneous evaluation horizon $T_{\text{total}}$ (`§2.1`, inheriting M3 $T = 8000$).
pub const DEFAULT_M4A_HORIZON: u64 = 8_000;
/// Default horizon for planted and channel-fault tasks (`§2.1`, inheriting M2b $T = 2000$).
pub const DEFAULT_M4A_PL_HORIZON: u64 = 2_000;
/// Default stationary substitution noise rate $\varepsilon = 0.05$ (`§2.1`, matching `G2A.4` / `G2B.4`).
pub const DEFAULT_M4A_NOISE_EPSILON: f64 = 0.05;
/// Default mid-run channel fault onset tick (`G2B.5`).
pub const DEFAULT_M4A_FAULT_TICK: u64 = 1_000;

/// Benchmark family in M4a-0 (`§2.1`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum M4aTaskFamily {
    /// Spontaneous tasks for `H5` / `H6` characterisation and gating (`R9`).
    Sp,
    /// Planted spurious-cycle / rare-context injection tasks (`PL1`, `PL2`).
    Pl,
    /// Controls verifying preservation and non-inferiority (`CT1`, `CT2`, `CT3`).
    Ct,
}

/// Noise stratum (`Clean` vs `Noisy`, never pooled under `StratifiedGateAudit`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum M4aRegime {
    /// Noiseless ground truth ($\varepsilon = 0.0$).
    Clean,
    /// Declared substitution noise ($\varepsilon = 0.05$).
    Noisy,
}

/// Canonical task kind within the M4a-0 registry (`§2.1`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum M4aTaskKind {
    /// `G2B.2`-class multi-view `T2` with grounded periodic driver.
    SpMultiViewPeriodic,
    /// `G2B.2`-class multi-view `T2` with LFSR driver.
    SpMultiViewLfsr,
    /// `K = 1` shallow-vs-deep lag redundancy (Charter §1.3 period-6).
    SpPeriod6LagRedundancy,
    /// `G2B.5`-style mid-run channel fault on channel 0.
    SpChannelFault,
    /// `PL1`: planted short-cycle autonomous ring alt dividing the true period.
    Pl1ShortCycle,
    /// `PL2`: planted rare-context alt agreeing everywhere except a low-probability context.
    Pl2RareContext,
    /// `CT1`: exogenous LFSR-driver cross-channel control (`R4`).
    Ct1ExogenousLfsr,
    /// `CT2`: rare-event-dependent oracle-correct structure control.
    Ct2RareEventCorrect,
    /// `CT3`: non-redundant multi-channel task where no comparator forms (`I2`).
    Ct3NoComparator,
}

/// Planted injection specification for `PL1` / `PL2` tasks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlantedSpec {
    /// Autonomous short-cycle motif dividing the true generator period.
    Pl1 { short_motif: Vec<u64> },
    /// Conjunctive rare-context clauses `(channel, extra_delay, target_symbol)`.
    Pl2 { clauses: Vec<(u16, usize, u64)> },
}

/// Complete specification of one M4a-0 benchmark task.
#[derive(Clone, Debug, PartialEq)]
pub struct M4aTaskSpec {
    /// Stable task identifier (`<FAMILY>:<KIND>:k<1|2>:<clean|noisy>`).
    pub id: String,
    /// Task family (`SP`, `PL`, `CT`).
    pub family: M4aTaskFamily,
    /// Task kind.
    pub kind: M4aTaskKind,
    /// Uniform bit-width ($k \in \{1, 2\}$).
    pub width: Width,
    /// Wire-delay mode (`Combinational`).
    pub wire_delay: WireDelayMode,
    /// Noise regime (`Clean` or `Noisy`).
    pub regime: M4aRegime,
    /// Baseline substitution noise rate (`0.0` or `0.05`).
    pub epsilon: f64,
    /// Clean underlying generator configuration.
    pub generator: GeneratorConfig,
    /// Declared environment noise model.
    pub noise: NoiseModel,
    /// Primary prediction/evaluation target port.
    pub target_port: u16,
    /// Default episode horizon $T_{\text{total}}$.
    pub horizon: u64,
    /// Optional planted injection payload for `PL` tasks.
    pub planted: Option<PlantedSpec>,
}

impl M4aTaskSpec {
    /// Builds the clean underlying [`TierGenerator`].
    ///
    /// # Errors
    ///
    /// Returns [`GeneratorError`] if the generator config is invalid for `self.width`.
    pub fn build_clean_generator(&self) -> Result<TierGenerator, GeneratorError> {
        self.generator.build(self.width)
    }

    /// Exports the clean underlying [`FiniteStateSpec`] (used for oracle labeling even on `Noisy` tasks, `R8`).
    ///
    /// # Errors
    ///
    /// Returns [`SpecExportError`] if the generator state space exceeds `DEFAULT_MAX_EXPORT_STATES`.
    pub fn to_clean_finite_state_spec(&self) -> Result<FiniteStateSpec, SpecExportError> {
        self.build_clean_generator()
            .expect("registered M4a-0 task generator config must be valid")
            .to_finite_state_spec()
    }

    /// Builds a [`NoisyEnvironment`] combining the clean generator, declared `self.noise`, and `seed`.
    ///
    /// # Errors
    ///
    /// Returns [`NoiseError`] if `self.noise` is invalid for the generator channel count.
    pub fn build_environment(
        &self,
        seed: SeedBank,
    ) -> Result<NoisyEnvironment<TierGenerator>, NoiseError> {
        let generator = self
            .build_clean_generator()
            .expect("registered M4a-0 task generator config must be valid");
        NoisyEnvironment::new(generator, self.noise.clone(), seed)
    }

    /// Constructs the clean [`PortLanguageOracle`] for this task (`R8`).
    ///
    /// # Errors
    ///
    /// Returns [`SpecExportError`] if the underlying clean spec cannot be exported.
    pub fn make_oracle(&self) -> Result<PortLanguageOracle, SpecExportError> {
        let spec = self.to_clean_finite_state_spec()?;
        Ok(PortLanguageOracle::from_clean_spec(&spec))
    }
}

fn lfsr_deg5() -> LfsrStage {
    LfsrStage {
        taps: vec![0, 1, 0, 0, 1],
        init: vec![1, 1, 0, 1, 0],
    }
}

fn lfsr_deg7() -> LfsrStage {
    LfsrStage {
        taps: vec![1, 0, 0, 0, 0, 0, 1],
        init: vec![1, 0, 1, 1, 0, 1, 0],
    }
}

fn lfsr_deg11() -> LfsrStage {
    let mut taps = vec![0u64; 11];
    taps[8] = 1;
    taps[10] = 1;
    let init = vec![1, 0, 1, 1, 0, 1, 0, 1, 1, 0, 1];
    LfsrStage { taps, init }
}

/// 4-tap primitive degree-13 LFSR (`x^13 + x^4 + x^3 + x + 1`, Mersenne prime period `8191` at `k=1`):
/// 4 taps and degree `13 > deepening_max_lag (12)` prevent single-channel Stage 2 grounding (`CT1`/`R4`).
fn lfsr_deg13_4tap() -> LfsrStage {
    let mut taps = vec![0u64; 13];
    taps[8] = 1;
    taps[9] = 1;
    taps[11] = 1;
    taps[12] = 1;
    let init = vec![1, 0, 1, 1, 0, 1, 0, 1, 1, 0, 1, 0, 1];
    LfsrStage { taps, init }
}

fn multiview_cfg(
    driver: MultiViewDriver,
    width: Width,
    target: MultiViewTarget,
) -> GeneratorConfig {
    let (view_0, view_1) = match width.bits() {
        1 => (
            MultiViewTransform {
                delay: 1,
                mul_a: 1,
                add_b: 0,
            },
            MultiViewTransform {
                delay: 0,
                mul_a: 1,
                add_b: 1,
            },
        ),
        _ => (
            MultiViewTransform {
                delay: 1,
                mul_a: 1,
                add_b: 0,
            },
            MultiViewTransform {
                delay: 0,
                mul_a: 3,
                add_b: 2,
            },
        ),
    };
    GeneratorConfig::T2_MultiViewCoupled {
        driver,
        view_0,
        view_1,
        target,
    }
}

fn simultaneous_ungrounded_multiview_cfg(
    driver: MultiViewDriver,
    width: Width,
    target: MultiViewTarget,
) -> GeneratorConfig {
    let (view_0, view_1) = match width.bits() {
        1 => (
            MultiViewTransform {
                delay: 0,
                mul_a: 1,
                add_b: 0,
            },
            MultiViewTransform {
                delay: 0,
                mul_a: 1,
                add_b: 1,
            },
        ),
        _ => (
            MultiViewTransform {
                delay: 0,
                mul_a: 1,
                add_b: 0,
            },
            MultiViewTransform {
                delay: 0,
                mul_a: 3,
                add_b: 2,
            },
        ),
    };
    GeneratorConfig::T2_MultiViewCoupled {
        driver,
        view_0,
        view_1,
        target,
    }
}

fn identity_shifted_multiview_cfg(
    driver: MultiViewDriver,
    target: MultiViewTarget,
) -> GeneratorConfig {
    GeneratorConfig::T2_MultiViewCoupled {
        driver,
        view_0: MultiViewTransform {
            delay: 0,
            mul_a: 1,
            add_b: 0,
        },
        view_1: MultiViewTransform {
            delay: 1,
            mul_a: 1,
            add_b: 0,
        },
        target,
    }
}

fn stationary_noise(regime: M4aRegime, num_channels: u16) -> (f64, NoiseModel) {
    match regime {
        M4aRegime::Clean => (0.0, NoiseModel::None),
        M4aRegime::Noisy => (
            DEFAULT_M4A_NOISE_EPSILON,
            NoiseModel::IndependentSubstitution {
                epsilon: DEFAULT_M4A_NOISE_EPSILON,
                channels: (0..num_channels).collect(),
            },
        ),
    }
}

/// Returns the complete Milestone M4a-0 task registry across `k in {1, 2}` and `Clean` / `Noisy` (`§2.1`).
#[must_use]
pub fn m4a0_task_registry() -> Vec<M4aTaskSpec> {
    let mut tasks = Vec::with_capacity(36);
    let widths = [Width::BITS_1, Width::BITS_2];
    let regimes = [M4aRegime::Clean, M4aRegime::Noisy];

    for width in widths {
        let k = width.bits();
        let period6_motif = if k == 1 {
            vec![0, 1, 0, 1, 0, 0]
        } else {
            vec![0, 1, 3, 0, 1, 2]
        };
        let short_motif = if k == 1 { vec![0, 1] } else { vec![0, 1, 3] };

        let mv_periodic_driver = MultiViewDriver::PeriodicMotif {
            motif: period6_motif.clone(),
        };
        let mv_d11_driver = MultiViewDriver::Lfsr {
            stage: lfsr_deg11(),
        };
        let mv_d13_driver = MultiViewDriver::Lfsr {
            stage: lfsr_deg13_4tap(),
        };
        let mv_d5_driver = MultiViewDriver::Lfsr { stage: lfsr_deg5() };

        let affine_tgt = if k == 1 {
            MultiViewTarget::Affine {
                lag: 2,
                mul_a: 1,
                add_b: 0,
            }
        } else {
            MultiViewTarget::Affine {
                lag: 2,
                mul_a: 3,
                add_b: 1,
            }
        };
        let rare_event_tgt = MultiViewTarget::MulEqz { lag_a: 2, lag_b: 3 };

        // PL2 clauses match `lfsr_deg5()` initial 5-window `[1, 1, 0, 1, 0]` (`ch0(t) = z(t)`),
        // which occurs once per LFSR period (p <= 1/31 at k=1, <= 1/31 at k=2).
        let pl2_clauses = vec![(0, 0, 0), (0, 1, 1), (0, 2, 0), (0, 3, 1), (0, 4, 1)];

        for regime in regimes {
            let r_tag = match regime {
                M4aRegime::Clean => "clean",
                M4aRegime::Noisy => "noisy",
            };
            let (eps, noise_3ch) = stationary_noise(regime, 3);
            let (_, noise_1ch) = stationary_noise(regime, 1);

            // 1. SP: Multi-view T2 with periodic grounded driver.
            tasks.push(M4aTaskSpec {
                id: format!("SP:MV_Periodic:k{k}:{r_tag}"),
                family: M4aTaskFamily::Sp,
                kind: M4aTaskKind::SpMultiViewPeriodic,
                width,
                wire_delay: WireDelayMode::Combinational,
                regime,
                epsilon: eps,
                generator: multiview_cfg(mv_periodic_driver.clone(), width, affine_tgt),
                noise: noise_3ch.clone(),
                target_port: 2,
                horizon: DEFAULT_M4A_HORIZON,
                planted: None,
            });

            // 2. SP: Multi-view T2 with LFSR driver (cross-channel).
            tasks.push(M4aTaskSpec {
                id: format!("SP:MV_Lfsr:k{k}:{r_tag}"),
                family: M4aTaskFamily::Sp,
                kind: M4aTaskKind::SpMultiViewLfsr,
                width,
                wire_delay: WireDelayMode::Combinational,
                regime,
                epsilon: eps,
                generator: multiview_cfg(mv_d11_driver.clone(), width, affine_tgt),
                noise: noise_3ch.clone(),
                target_port: 2,
                horizon: DEFAULT_M4A_HORIZON,
                planted: None,
            });

            // 3. SP: K=1 shallow-vs-deep lag redundancy (Charter §1.3 period-6).
            tasks.push(M4aTaskSpec {
                id: format!("SP:Period6_LagRedundancy:k{k}:{r_tag}"),
                family: M4aTaskFamily::Sp,
                kind: M4aTaskKind::SpPeriod6LagRedundancy,
                width,
                wire_delay: WireDelayMode::Combinational,
                regime,
                epsilon: eps,
                generator: GeneratorConfig::T1_PeriodicMotif {
                    motif: period6_motif.clone(),
                },
                noise: noise_1ch.clone(),
                target_port: 0,
                horizon: DEFAULT_M4A_HORIZON,
                planted: None,
            });

            // 4. SP: G2B.5-style channel-fault task.
            let fault_eps = 1.0 - 1.0 / 2.0f64.powi(i32::from(k));
            let fault_noise = NoiseModel::ChannelFaultSchedule {
                base_epsilon: eps,
                base_channels: if eps > 0.0 { vec![0, 1, 2] } else { Vec::new() },
                fault_tick: DEFAULT_M4A_FAULT_TICK,
                fault_epsilon: fault_eps,
                fault_channels: vec![0],
            };
            tasks.push(M4aTaskSpec {
                id: format!("SP:ChannelFault:k{k}:{r_tag}"),
                family: M4aTaskFamily::Sp,
                kind: M4aTaskKind::SpChannelFault,
                width,
                wire_delay: WireDelayMode::Combinational,
                regime,
                epsilon: eps,
                generator: multiview_cfg(mv_d11_driver.clone(), width, affine_tgt),
                noise: fault_noise,
                target_port: 2,
                horizon: DEFAULT_M4A_PL_HORIZON,
                planted: None,
            });

            // 5. PL1: Planted short-cycle alt dividing period 6.
            tasks.push(M4aTaskSpec {
                id: format!("PL:PL1_ShortCycle:k{k}:{r_tag}"),
                family: M4aTaskFamily::Pl,
                kind: M4aTaskKind::Pl1ShortCycle,
                width,
                wire_delay: WireDelayMode::Combinational,
                regime,
                epsilon: eps,
                generator: GeneratorConfig::T1_PeriodicMotif {
                    motif: period6_motif.clone(),
                },
                noise: noise_1ch.clone(),
                target_port: 0,
                horizon: DEFAULT_M4A_PL_HORIZON,
                planted: Some(PlantedSpec::Pl1 {
                    short_motif: short_motif.clone(),
                }),
            });

            // 6. PL2: Planted rare-context alt on LFSR multi-view T2.
            tasks.push(M4aTaskSpec {
                id: format!("PL:PL2_RareContext:k{k}:{r_tag}"),
                family: M4aTaskFamily::Pl,
                kind: M4aTaskKind::Pl2RareContext,
                width,
                wire_delay: WireDelayMode::Combinational,
                regime,
                epsilon: eps,
                generator: simultaneous_ungrounded_multiview_cfg(
                    mv_d5_driver.clone(),
                    width,
                    affine_tgt,
                ),
                noise: noise_3ch.clone(),
                target_port: 2,
                horizon: DEFAULT_M4A_PL_HORIZON,
                planted: Some(PlantedSpec::Pl2 {
                    clauses: pl2_clauses.clone(),
                }),
            });

            // 7. CT1: Exogenous LFSR-driver cross-channel task (R4 protection).
            tasks.push(M4aTaskSpec {
                id: format!("CT:CT1_ExogenousLfsr:k{k}:{r_tag}"),
                family: M4aTaskFamily::Ct,
                kind: M4aTaskKind::Ct1ExogenousLfsr,
                width,
                wire_delay: WireDelayMode::Combinational,
                regime,
                epsilon: eps,
                generator: simultaneous_ungrounded_multiview_cfg(
                    mv_d13_driver.clone(),
                    width,
                    affine_tgt,
                ),
                noise: noise_3ch.clone(),
                target_port: 2,
                horizon: DEFAULT_M4A_HORIZON,
                planted: None,
            });

            // 8. CT2: Rare-event-dependent oracle-correct structure control.
            tasks.push(M4aTaskSpec {
                id: format!("CT:CT2_RareEventCorrect:k{k}:{r_tag}"),
                family: M4aTaskFamily::Ct,
                kind: M4aTaskKind::Ct2RareEventCorrect,
                width,
                wire_delay: WireDelayMode::Combinational,
                regime,
                epsilon: eps,
                generator: identity_shifted_multiview_cfg(mv_d5_driver.clone(), rare_event_tgt),
                noise: noise_3ch.clone(),
                target_port: 2,
                horizon: DEFAULT_M4A_HORIZON,
                planted: None,
            });

            // 9. CT3: Non-redundant dual-LFSR task where no comparator forms (I2).
            tasks.push(M4aTaskSpec {
                id: format!("CT:CT3_NoComparator:k{k}:{r_tag}"),
                family: M4aTaskFamily::Ct,
                kind: M4aTaskKind::Ct3NoComparator,
                width,
                wire_delay: WireDelayMode::Combinational,
                regime,
                epsilon: eps,
                generator: GeneratorConfig::T2_CrossNonlinear {
                    driver_0: lfsr_deg7(),
                    driver_1: lfsr_deg5(),
                    lag_0: 4,
                    lag_1: 4,
                    op: CrossNonlinearOp::MulEqzPair,
                },
                noise: noise_3ch,
                target_port: 2,
                horizon: DEFAULT_M4A_HORIZON,
                planted: None,
            });
        }
    }

    tasks
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn m4a0_task_registry_has_unique_ids_and_valid_specs() {
        let tasks = m4a0_task_registry();
        let ids: BTreeSet<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids.len(), tasks.len());
        for task in &tasks {
            let spec = task.to_clean_finite_state_spec().unwrap();
            assert!(spec.num_states >= 1);
            assert!(usize::from(task.target_port) < usize::from(spec.channels));
        }
    }
}
