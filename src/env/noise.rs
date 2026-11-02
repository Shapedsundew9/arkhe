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
}

impl NoiseModel {
    /// Validates noise parameters against `num_channels`.
    ///
    /// # Errors
    ///
    /// Returns [`NoiseError`] if `epsilon` is out of `[0.0, 1.0)` or any channel index is out of bounds.
    pub fn validate(&self, num_channels: u16) -> Result<(), NoiseError> {
        match self {
            Self::None => Ok(()),
            Self::IndependentSubstitution { epsilon, channels } => {
                if !epsilon.is_finite() || *epsilon < 0.0 || *epsilon >= 1.0 {
                    return Err(NoiseError::InvalidEpsilon(*epsilon));
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
        }
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
pub struct NoisyEnvironment<G> {
    generator: G,
    noise: NoiseModel,
    channel_rngs: Vec<Option<ChaCha8Rng>>,
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
        if let NoiseModel::IndependentSubstitution { epsilon, channels } = &noise
            && *epsilon > 0.0
        {
            for &ch in channels {
                channel_rngs[usize::from(ch)] =
                    Some(seeds.stream(StreamDomain::ChannelNoise { channel: ch }));
            }
        }

        Ok(Self {
            generator,
            noise,
            channel_rngs,
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

    /// Advances one tick, writing clean symbols to `clean_out` and observed symbols to `observed_out`.
    pub fn step_frame(&mut self, clean_out: &mut [u64], observed_out: &mut [u64]) {
        let n = usize::from(self.generator.channels());
        assert_eq!(clean_out.len(), n);
        assert_eq!(observed_out.len(), n);

        self.generator.next_clean_frame(clean_out);
        observed_out.copy_from_slice(clean_out);

        if let NoiseModel::IndependentSubstitution { epsilon, .. } = &self.noise
            && *epsilon > 0.0
        {
            let width = self.generator.width();
            for (ch, slot) in observed_out.iter_mut().enumerate() {
                if let Some(rng) = &mut self.channel_rngs[ch]
                    && uniform_f64_01(rng) < *epsilon
                {
                    *slot = sample_distinct_symbol(rng, width, *slot);
                }
            }
        }
    }
}
