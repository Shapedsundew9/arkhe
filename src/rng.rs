//! Deterministic, domain-separated random number generation (`ChaCha8Rng`) and hashing.
//!
//! Enforces Charter §7, §8, and §9 reproducibility: every stochastic subsystem derives an
//! independent `ChaCha8Rng` stream from a 64-bit master seed and a typed [`StreamDomain`],
//! ensuring subsystem configuration changes never cross-contaminate other streams.

use rand_chacha::ChaCha8Rng;
use rand_core::SeedableRng;
use serde::{Deserialize, Serialize};

/// Protocol version tag mixed into child RNG seeds so stream layouts are explicit.
const SEED_PROTOCOL_VERSION: u32 = 1;

/// Domain tag isolating independent random streams derived from a single [`SeedBank`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum StreamDomain {
    /// Initial state selection for environment generators when randomised.
    GeneratorInit,
    /// Independent observation noise stream for a specific boundary channel.
    ChannelNoise { channel: u16 },
    /// Candidate sampling and mutation stream for a baseline trial.
    BaselineSearch { trial: u32 },
    /// Initial register perturbation sampling for autonomous basin estimation.
    BasinPerturbation { probe: u32 },
}

impl StreamDomain {
    const fn tag_and_index(self) -> (u32, u64) {
        match self {
            Self::GeneratorInit => (1, 0),
            Self::ChannelNoise { channel } => (2, channel as u64),
            Self::BaselineSearch { trial } => (3, trial as u64),
            Self::BasinPerturbation { probe } => (4, probe as u64),
        }
    }
}

/// Deterministic factory deriving orthogonal `ChaCha8Rng` streams from a master seed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeedBank {
    master_seed: u64,
}

impl SeedBank {
    /// Creates a new [`SeedBank`] rooted at `master_seed`.
    #[must_use]
    pub const fn new(master_seed: u64) -> Self {
        Self { master_seed }
    }

    /// Returns the 64-bit master seed.
    #[must_use]
    pub const fn master_seed(self) -> u64 {
        self.master_seed
    }

    /// Expands `(master_seed, domain)` into a canonical little-endian 32-byte ChaCha seed.
    #[must_use]
    pub fn expand_seed(self, domain: StreamDomain) -> [u8; 32] {
        let (domain_tag, domain_index) = domain.tag_and_index();
        let mut seed = [0u8; 32];
        seed[0..8].copy_from_slice(&self.master_seed.to_le_bytes());
        seed[8..12].copy_from_slice(&domain_tag.to_le_bytes());
        seed[12..16].copy_from_slice(&SEED_PROTOCOL_VERSION.to_le_bytes());
        seed[16..24].copy_from_slice(&domain_index.to_le_bytes());
        // Mix fixed ASCII domain separator "ARKHEM0!" in bytes 24..32.
        seed[24..32].copy_from_slice(b"ARKHEM0!");
        seed
    }

    /// Constructs a portable, deterministic `ChaCha8Rng` for `domain`.
    #[must_use]
    pub fn stream(self, domain: StreamDomain) -> ChaCha8Rng {
        ChaCha8Rng::from_seed(self.expand_seed(domain))
    }
}

/// Computes a deterministic 128-bit FNV-1a hex digest for configuration/manifest fingerprinting.
#[must_use]
pub fn fnv1a_128_hex(bytes: &[u8]) -> String {
    const OFFSET_BASIS: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;
    let mut hash = OFFSET_BASIS;
    for &b in bytes {
        hash ^= u128::from(b);
        hash = hash.wrapping_mul(PRIME);
    }
    format!("{hash:032x}")
}
