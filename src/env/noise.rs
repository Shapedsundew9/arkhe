//! Declared, parameterised channel observation noise models (Charter §5.1).

use std::fmt;

use rand_chacha::ChaCha8Rng;
use rand_core::Rng;
use serde::{Deserialize, Serialize};

use crate::env::generator::FiniteStateGenerator;
use crate::rng::{SeedBank, StreamDomain};
use crate::substrate::Width;

/// Error returned when validating a [`NoiseModel`].
#[derive(Clone, Debug, PartialEq)]
pub enum NoiseError {
    /// Substitution rate $\varepsilon$ must satisfy $0 \le \varepsilon < 1$ and be finite.
    InvalidEpsilon(f64),
    /// Referenced a channel index `>= generator.channels()`.
    InvalidChannel { channel: u16, num_channels: u16 },
}

impl fmt::Display for NoiseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEpsilon(eps) => {
                write!(
                    f,
                    "invalid noise rate epsilon={eps}: expected 0.0 <= eps < 1.0"
                )
            }
            Self::InvalidChannel {
                channel,
                num_channels,
            } => write!(f, "noise channel {channel} out of range 0..{num_channels}"),
        }
    }
}

impl std::error::Error for NoiseError {}

/// Declared observation noise configuration (Charter §5.1).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum NoiseModel {
    /// Deterministic, noiseless ground-truth emissions ($\varepsilon = 0$).
    #[default]
    None,
    /// Independent uniform symbol substitution at probability $\varepsilon \in [0, 1)$ on named channels.
    ///
    /// With probability $\varepsilon$, the clean symbol $x \in \mathbb{Z}_{2^k}$ is replaced uniformly
    /// by one of the $2^k - 1$ distinct symbols in $\mathbb{Z}_{2^k} \setminus \{x\}$.
    IndependentSubstitution {
        /// Substitution probability per tick on each listed channel.
        epsilon: f64,
        /// Zero-based indices of channels subject to substitution noise.
        channels: Vec<u16>,
    },
    /// Scheduled mid-run channel-fault substitution noise (Charter §5.1 & M2b `G2B.5`).
    ///
    /// For ticks `0..fault_tick`, applies `base_epsilon` on `base_channels`.
    /// Starting at `tick >= fault_tick`, channels in `fault_channels` transition to `fault_epsilon`
    /// while non-fault channels continue at `base_epsilon` (if listed in `base_channels`) or `0.0`.
    ChannelFaultSchedule {
        /// Baseline substitution probability $\varepsilon_{\text{base}} \in [0, 1)$ before `fault_tick`
        /// (and ongoing on non-fault `base_channels`).
        #[serde(default)]
        base_epsilon: f64,
        /// Zero-based indices of channels subject to `base_epsilon`.
        #[serde(default)]
        base_channels: Vec<u16>,
        /// Zero-based tick index $t_{\text{fault}}$ at which `fault_channels` transition to `fault_epsilon`.
        fault_tick: u64,
        /// Scheduled fault substitution probability $\varepsilon_{\text{fault}} \in [0, 1)$ for `t >= fault_tick`.
        fault_epsilon: f64,
        /// Zero-based indices of channels affected starting at `fault_tick`.
        fault_channels: Vec<u16>,
    },
}

fn validate_rate_and_channels(
    epsilon: f64,
    channels: &[u16],
    num_channels: u16,
) -> Result<(), NoiseError> {
    if !epsilon.is_finite() || !(0.0..1.0).contains(&epsilon) {
        return Err(NoiseError::InvalidEpsilon(epsilon));
    }
    for &ch in channels {
        if ch >= num_channels {
            return Err(NoiseError::InvalidChannel {
                channel: ch,
                num_channels,
            });
        }
    }
    Ok(())
}

impl NoiseModel {
    /// Validates noise parameters against `num_channels`.
    ///
    /// # Errors
    ///
    /// Returns [`NoiseError`] if any `epsilon` is out of `[0.0, 1.0)` or any channel index is out of bounds.
    pub fn validate(&self, num_channels: u16) -> Result<(), NoiseError> {
        match self {
            Self::None => Ok(()),
            Self::IndependentSubstitution { epsilon, channels } => {
                validate_rate_and_channels(*epsilon, channels, num_channels)
            }
            Self::ChannelFaultSchedule {
                base_epsilon,
                base_channels,
                fault_epsilon,
                fault_channels,
                ..
            } => {
                validate_rate_and_channels(*base_epsilon, base_channels, num_channels)?;
                validate_rate_and_channels(*fault_epsilon, fault_channels, num_channels)
            }
        }
    }

    /// Returns the active substitution rate $\varepsilon_c(t)$ for channel `ch` at `tick`.
    #[must_use]
    pub fn channel_epsilon_at(&self, ch: u16, tick: u64) -> f64 {
        match self {
            Self::None => 0.0,
            Self::IndependentSubstitution { epsilon, channels } => {
                if channels.contains(&ch) {
                    *epsilon
                } else {
                    0.0
                }
            }
            Self::ChannelFaultSchedule {
                base_epsilon,
                base_channels,
                fault_tick,
                fault_epsilon,
                fault_channels,
            } => {
                if tick >= *fault_tick && fault_channels.contains(&ch) {
                    *fault_epsilon
                } else if base_channels.contains(&ch) {
                    *base_epsilon
                } else {
                    0.0
                }
            }
        }
    }

    /// Resolves active per-channel substitution rates across `0..num_channels` at `tick`.
    #[must_use]
    pub fn channel_epsilons_at(&self, num_channels: usize, tick: u64) -> Vec<f64> {
        let mut channel_eps = vec![0.0f64; num_channels];
        match self {
            Self::None => {}
            Self::IndependentSubstitution { epsilon, channels } => {
                for &ch in channels {
                    if usize::from(ch) < num_channels {
                        channel_eps[usize::from(ch)] = *epsilon;
                    }
                }
            }
            Self::ChannelFaultSchedule {
                base_epsilon,
                base_channels,
                fault_tick,
                fault_epsilon,
                fault_channels,
            } => {
                for &ch in base_channels {
                    if usize::from(ch) < num_channels {
                        channel_eps[usize::from(ch)] = *base_epsilon;
                    }
                }
                if tick >= *fault_tick {
                    for &ch in fault_channels {
                        if usize::from(ch) < num_channels {
                            channel_eps[usize::from(ch)] = *fault_epsilon;
                        }
                    }
                }
            }
        }
        channel_eps
    }
}

/// Samples a uniform `f64` in `[0.0, 1.0)` using 53 high bits of `next_u64()` (portable across platforms).
#[inline]
fn uniform_f64_01(rng: &mut ChaCha8Rng) -> f64 {
    const SCALE: f64 = 1.0 / ((1u64 << 53) as f64);
    ((rng.next_u64() >> 11) as f64) * SCALE
}

/// Samples an unbiased integer uniformly in `0..bound` (`bound >= 1`) via Lemire's rejection method.
#[inline]
pub(crate) fn uniform_u64_below(rng: &mut ChaCha8Rng, bound: u64) -> u64 {
    debug_assert!(bound >= 1);
    if bound == 1 {
        return 0;
    }
    let threshold = bound.wrapping_neg() % bound;
    loop {
        let x = rng.next_u64();
        let prod = u128::from(x) * u128::from(bound);
        let low = prod as u64;
        if low >= threshold {
            return (prod >> 64) as u64;
        }
    }
}

/// Samples a symbol uniformly from $\mathbb{Z}_{2^k} \setminus \{\text{clean}\}$.
fn sample_distinct_symbol(rng: &mut ChaCha8Rng, width: Width, clean: u64) -> u64 {
    match width.bits() {
        1 => (clean & 1) ^ 1,
        64 => loop {
            let delta = rng.next_u64();
            if delta != 0 {
                return clean.wrapping_add(delta);
            }
        },
        k => {
            let num_alternatives = (1u64 << k) - 1;
            let delta = 1 + uniform_u64_below(rng, num_alternatives);
            width.add(clean, delta)
        }
    }
}

/// Paired environment runner emitting both clean ground-truth and noisy observed frames per tick.
#[derive(Clone, Debug)]
pub struct NoisyEnvironment<G> {
    generator: G,
    noise: NoiseModel,
    channel_rngs: Vec<Option<ChaCha8Rng>>,
    tick: u64,
}

impl<G: FiniteStateGenerator> NoisyEnvironment<G> {
    /// Wraps `generator` with `noise`, seeding isolated per-channel `ChaCha8Rng` streams from `seeds`.
    ///
    /// # Errors
    ///
    /// Returns [`NoiseError`] if `noise` fails validation against `generator.channels()`.
    pub fn new(generator: G, noise: NoiseModel, seeds: SeedBank) -> Result<Self, NoiseError> {
        let num_channels = generator.channels();
        noise.validate(num_channels)?;

        let mut channel_rngs = vec![None; usize::from(num_channels)];
        let init_rng = |channel_rngs: &mut [Option<ChaCha8Rng>], ch: u16| {
            let idx = usize::from(ch);
            if channel_rngs[idx].is_none() {
                channel_rngs[idx] = Some(seeds.stream(StreamDomain::ChannelNoise { channel: ch }));
            }
        };
        match &noise {
            NoiseModel::None => {}
            NoiseModel::IndependentSubstitution { epsilon, channels } => {
                if *epsilon > 0.0 {
                    for &ch in channels {
                        init_rng(&mut channel_rngs, ch);
                    }
                }
            }
            NoiseModel::ChannelFaultSchedule {
                base_epsilon,
                base_channels,
                fault_epsilon,
                fault_channels,
                ..
            } => {
                if *base_epsilon > 0.0 {
                    for &ch in base_channels {
                        init_rng(&mut channel_rngs, ch);
                    }
                }
                if *fault_epsilon > 0.0 {
                    for &ch in fault_channels {
                        init_rng(&mut channel_rngs, ch);
                    }
                }
            }
        }

        Ok(Self {
            generator,
            noise,
            channel_rngs,
            tick: 0,
        })
    }

    /// Reference to the underlying clean generator.
    #[must_use]
    pub const fn generator(&self) -> &G {
        &self.generator
    }

    /// Active [`NoiseModel`].
    #[must_use]
    pub const fn noise(&self) -> &NoiseModel {
        &self.noise
    }

    /// Zero-based tick counter incremented after each [`Self::step_frame`] call.
    #[must_use]
    pub const fn tick(&self) -> u64 {
        self.tick
    }

    /// Advances one tick, writing clean symbols to `clean_out` and observed symbols to `observed_out`.
    pub fn step_frame(&mut self, clean_out: &mut [u64], observed_out: &mut [u64]) {
        let n = usize::from(self.generator.channels());
        assert_eq!(clean_out.len(), n);
        assert_eq!(observed_out.len(), n);

        self.generator.next_clean_frame(clean_out);
        observed_out.copy_from_slice(clean_out);

        let tick = self.tick;
        self.tick = self.tick.saturating_add(1);

        let width = self.generator.width();
        match &self.noise {
            NoiseModel::None => {}
            NoiseModel::IndependentSubstitution { epsilon, .. } => {
                if *epsilon > 0.0 {
                    for (ch, slot) in observed_out.iter_mut().enumerate() {
                        if let Some(rng) = &mut self.channel_rngs[ch]
                            && uniform_f64_01(rng) < *epsilon
                        {
                            *slot = sample_distinct_symbol(rng, width, *slot);
                        }
                    }
                }
            }
            NoiseModel::ChannelFaultSchedule { .. } => {
                for (ch, slot) in observed_out.iter_mut().enumerate() {
                    let ch_u16 = u16::try_from(ch).expect("channel index fits in u16");
                    let eps = self.noise.channel_epsilon_at(ch_u16, tick);
                    if eps > 0.0
                        && let Some(rng) = &mut self.channel_rngs[ch]
                        && uniform_f64_01(rng) < eps
                    {
                        *slot = sample_distinct_symbol(rng, width, *slot);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::tiers::{GeneratorConfig, LfsrStage};

    #[test]
    fn channel_fault_schedule_validation_and_rates() {
        let valid = NoiseModel::ChannelFaultSchedule {
            base_epsilon: 0.0,
            base_channels: Vec::new(),
            fault_tick: 1000,
            fault_epsilon: 0.75,
            fault_channels: vec![0],
        };
        assert_eq!(valid.validate(3), Ok(()));
        assert_eq!(valid.channel_epsilons_at(3, 999), vec![0.0, 0.0, 0.0]);
        assert_eq!(valid.channel_epsilons_at(3, 1000), vec![0.75, 0.0, 0.0]);

        let bad_base_eps = NoiseModel::ChannelFaultSchedule {
            base_epsilon: 1.0,
            base_channels: vec![1],
            fault_tick: 10,
            fault_epsilon: 0.5,
            fault_channels: vec![0],
        };
        assert_eq!(
            bad_base_eps.validate(3),
            Err(NoiseError::InvalidEpsilon(1.0))
        );

        let bad_fault_eps = NoiseModel::ChannelFaultSchedule {
            base_epsilon: 0.0,
            base_channels: vec![],
            fault_tick: 10,
            fault_epsilon: -0.1,
            fault_channels: vec![0],
        };
        assert_eq!(
            bad_fault_eps.validate(3),
            Err(NoiseError::InvalidEpsilon(-0.1))
        );

        let bad_ch = NoiseModel::ChannelFaultSchedule {
            base_epsilon: 0.0,
            base_channels: vec![],
            fault_tick: 10,
            fault_epsilon: 0.5,
            fault_channels: vec![3],
        };
        assert_eq!(
            bad_ch.validate(3),
            Err(NoiseError::InvalidChannel {
                channel: 3,
                num_channels: 3
            })
        );
    }

    #[test]
    fn channel_fault_schedule_deterministic_transition_and_sibling_isolation() {
        let gen_cfg = GeneratorConfig::T2_CrossLinear {
            driver_0: LfsrStage {
                taps: vec![0, 1, 0, 0, 1],
                init: vec![1, 1, 0, 1, 0],
            },
            driver_1: LfsrStage {
                taps: vec![1, 0, 0, 1],
                init: vec![1, 0, 1, 1],
            },
            lag_0: 1,
            lag_1: 2,
            op: crate::env::tiers::CrossLinearOp::Add,
        };

        for (width, fault_eps) in [(Width::BITS_1, 0.5), (Width::BITS_2, 0.75)] {
            let noise = NoiseModel::ChannelFaultSchedule {
                base_epsilon: 0.0,
                base_channels: Vec::new(),
                fault_tick: 1000,
                fault_epsilon: fault_eps,
                fault_channels: vec![0],
            };
            let mut env_a = NoisyEnvironment::new(
                gen_cfg.build(width).unwrap(),
                noise.clone(),
                SeedBank::new(42),
            )
            .unwrap();
            let mut env_b =
                NoisyEnvironment::new(gen_cfg.build(width).unwrap(), noise, SeedBank::new(42))
                    .unwrap();

            let mut clean_a = [0u64; 3];
            let mut obs_a = [0u64; 3];
            let mut clean_b = [0u64; 3];
            let mut obs_b = [0u64; 3];
            let mut post_fault_flips = 0usize;

            for t in 0..3000u64 {
                env_a.step_frame(&mut clean_a, &mut obs_a);
                env_b.step_frame(&mut clean_b, &mut obs_b);
                assert_eq!(env_a.tick(), t + 1);
                assert_eq!(clean_a, clean_b);
                assert_eq!(obs_a, obs_b);
                // Channels 1 and 2 are not in base_channels or fault_channels -> strictly clean.
                assert_eq!(obs_a[1], clean_a[1]);
                assert_eq!(obs_a[2], clean_a[2]);

                if t < 1000 {
                    assert_eq!(obs_a[0], clean_a[0]);
                } else if obs_a[0] != clean_a[0] {
                    post_fault_flips += 1;
                }
            }

            let empirical_rate = post_fault_flips as f64 / 2000.0;
            assert!(
                (empirical_rate - fault_eps).abs() < 0.05,
                "k={}: expected fault rate ~{fault_eps}, got {empirical_rate}",
                width.bits()
            );
        }

        // Verify base_epsilon applies before fault_tick and stays unchanged on non-fault channels.
        let base_and_fault = NoiseModel::ChannelFaultSchedule {
            base_epsilon: 0.05,
            base_channels: vec![0, 1],
            fault_tick: 1000,
            fault_epsilon: 0.75,
            fault_channels: vec![0],
        };
        let mut env = NoisyEnvironment::new(
            gen_cfg.build(Width::BITS_2).unwrap(),
            base_and_fault,
            SeedBank::new(7),
        )
        .unwrap();
        let mut clean = [0u64; 3];
        let mut obs = [0u64; 3];
        let mut pre_flips_0 = 0usize;
        let mut pre_flips_1 = 0usize;
        let mut post_flips_0 = 0usize;
        let mut post_flips_1 = 0usize;
        for t in 0..3000u64 {
            env.step_frame(&mut clean, &mut obs);
            assert_eq!(obs[2], clean[2]);
            if t < 1000 {
                pre_flips_0 += usize::from(obs[0] != clean[0]);
                pre_flips_1 += usize::from(obs[1] != clean[1]);
            } else {
                post_flips_0 += usize::from(obs[0] != clean[0]);
                post_flips_1 += usize::from(obs[1] != clean[1]);
            }
        }
        assert!(((pre_flips_0 as f64 / 1000.0) - 0.05).abs() < 0.03);
        assert!(((pre_flips_1 as f64 / 1000.0) - 0.05).abs() < 0.03);
        assert!(((post_flips_0 as f64 / 2000.0) - 0.75).abs() < 0.05);
        assert!(((post_flips_1 as f64 / 2000.0) - 0.05).abs() < 0.03);
    }
}
