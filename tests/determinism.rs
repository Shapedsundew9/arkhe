//! Cross-run determinism, golden RNG vectors, and stream-isolation integration tests (Gate G4).

use arkhe::config::{
    BaselineConfig, BudgetConfig, CONFIG_SCHEMA_VERSION, EnvironmentConfig, ExperimentConfig,
    SubstrateConfig, execute_experiment,
};
use arkhe::env::{GeneratorConfig, NoiseModel};
use arkhe::rng::{SeedBank, StreamDomain, fnv1a_128_hex};
use arkhe::structural::WiringTopology;
use arkhe::substrate::{Width, WireDelayMode};
use rand_core::Rng;

#[test]
fn seed_bank_golden_vectors_are_portable_and_stable() {
    let bank0 = SeedBank::new(0);
    assert_eq!(bank0.master_seed(), 0);

    let expected_seed_bytes: [u8; 32] = [
        0, 0, 0, 0, 0, 0, 0, 0, // master_seed = 0 (LE)
        2, 0, 0, 0, // ChannelNoise tag = 2 (LE)
        1, 0, 0, 0, // SEED_PROTOCOL_VERSION = 1 (LE)
        3, 0, 0, 0, 0, 0, 0, 0, // channel = 3 (LE)
        b'A', b'R', b'K', b'H', b'E', b'M', b'0', b'!',
    ];
    assert_eq!(
        bank0.expand_seed(StreamDomain::ChannelNoise { channel: 3 }),
        expected_seed_bytes
    );

    // Golden ChaCha8Rng words for SeedBank::new(0) across all four StreamDomain variants.
    let mut g_init = bank0.stream(StreamDomain::GeneratorInit);
    let mut c_noise = bank0.stream(StreamDomain::ChannelNoise { channel: 0 });
    let mut b_search = bank0.stream(StreamDomain::BaselineSearch { trial: 1 });
    let mut b_basin = bank0.stream(StreamDomain::BasinPerturbation { probe: 0 });

    let draws = [
        g_init.next_u64(),
        c_noise.next_u64(),
        b_search.next_u64(),
        b_basin.next_u64(),
    ];
    // All four domains must produce pairwise distinct streams from the same master seed.
    for i in 0..draws.len() {
        for j in (i + 1)..draws.len() {
            assert_ne!(draws[i], draws[j], "domain collision between {i} and {j}");
        }
    }

    // Lock exact portable ChaCha8 outputs so cross-platform endianness/RNG upgrades cannot silently drift.
    let bank_arkhe = SeedBank::new(0xA4_4B_48_45);
    let mut s1 = bank_arkhe.stream(StreamDomain::BaselineSearch { trial: 0 });
    let v0 = s1.next_u64();
    let v1 = s1.next_u64();

    let mut s1_repeat =
        SeedBank::new(0xA4_4B_48_45).stream(StreamDomain::BaselineSearch { trial: 0 });
    assert_eq!(s1_repeat.next_u64(), v0);
    assert_eq!(s1_repeat.next_u64(), v1);
    assert_eq!(v0, 10_570_056_588_342_067_228);
    assert_eq!(v1, 10_419_271_532_414_833_232);
}

#[test]
fn fnv1a_128_digest_matches_known_vectors() {
    // Standard FNV-1a 128-bit offset basis on empty input.
    assert_eq!(fnv1a_128_hex(b""), "6c62272e07bb014262b821756295c58d");
    assert_ne!(fnv1a_128_hex(b"arkhe-a"), fnv1a_128_hex(b"arkhe-b"));
}

#[test]
fn stream_domains_are_strictly_isolated() {
    let bank = SeedBank::new(2026);
    let mut noise_clean = bank.stream(StreamDomain::ChannelNoise { channel: 0 });

    // Heavily advance BaselineSearch and BasinPerturbation streams from the same SeedBank.
    let mut search = bank.stream(StreamDomain::BaselineSearch { trial: 1 });
    let mut basin = bank.stream(StreamDomain::BasinPerturbation { probe: 7 });
    for _ in 0..10_000 {
        let _ = search.next_u64();
        let _ = basin.next_u64();
    }

    let mut noise_after = bank.stream(StreamDomain::ChannelNoise { channel: 0 });
    for _ in 0..256 {
        assert_eq!(noise_clean.next_u64(), noise_after.next_u64());
    }
}

#[test]
fn execute_experiment_is_byte_identical_across_runs_and_distinct_across_seeds() {
    let make_cfg = |seed: u64| ExperimentConfig {
        schema_version: CONFIG_SCHEMA_VERSION,
        master_seed: seed,
        substrate: SubstrateConfig {
            width_bits: Width::BITS_2,
            wire_delay: WireDelayMode::Combinational,
            wiring_topology: WiringTopology::WellMixed,
        },
        environment: EnvironmentConfig {
            horizon_ticks: 320,
            transient_warmup_ticks: 60,
            generator: GeneratorConfig::T1_Alternating { a: 0, b: 3 },
            noise: NoiseModel::IndependentSubstitution {
                epsilon: 0.05,
                channels: vec![0],
            },
        },
        budget: BudgetConfig {
            max_eval_ticks: 30_000,
        },
        baseline: BaselineConfig::Cgp {
            lambda: 4,
            max_genes: 6,
            fact_window: 96,
        },
    };

    let cfg_a = make_cfg(77);
    let rep_a1 = execute_experiment(&cfg_a).expect("experiment A1 succeeds");
    let rep_a2 = execute_experiment(&cfg_a).expect("experiment A2 succeeds");

    let json_a1 = serde_json::to_vec_pretty(&rep_a1).expect("serializes A1");
    let json_a2 = serde_json::to_vec_pretty(&rep_a2).expect("serializes A2");
    assert_eq!(
        json_a1, json_a2,
        "identical (seed, config) must be byte-identical"
    );

    let cfg_b = make_cfg(78);
    let rep_b = execute_experiment(&cfg_b).expect("experiment B succeeds");
    assert_ne!(rep_a1.config_digest_fnv1a128, rep_b.config_digest_fnv1a128);
    assert_ne!(
        serde_json::to_vec_pretty(&rep_b).expect("serializes B"),
        json_a1
    );
}
