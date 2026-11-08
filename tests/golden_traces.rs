//! Golden-trace regression harness for coordinate-free and parameter scaling refactoring (§3 Phase 0).
//!
//! Generates and asserts bit-identical golden execution traces across all WellMixed learner configurations.
//! Bless fixtures with: `ARKHE_BLESS=1 cargo test --test golden_traces`

use std::fs;
use std::path::PathBuf;

use arkhe::baselines::{PredictorBaseline, TickBudget};
use arkhe::env::{GeneratorConfig, NoiseModel, NoisyEnvironment};
use arkhe::rng::SeedBank;
use arkhe::structural::economy::EconomyConfig;
use arkhe::structural::growth::GrowthConfig;
use arkhe::structural::lattice::{GridShape, WiringTopology};
use arkhe::structural::learner::ArkheLearner;
use arkhe::substrate::{Width, WireDelayMode};
use serde::{Deserialize, Serialize};

const HOLDOUT_START: u64 = 10000;
const HOLDOUT_END: u64 = 10019;

fn assert_not_holdout(seed: u64) {
    assert!(
        !(HOLDOUT_START..=HOLDOUT_END).contains(&seed),
        "Fatal: seed {seed} falls within pre-registered holdout range 10000..=10019!"
    );
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeRecord {
    pub src: String,
    pub dst: String,
    pub hops: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceEvent {
    pub tick: u64,
    pub kind: String,
    pub node_count: usize,
    pub details: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GoldenTraceFixture {
    pub name: String,
    pub seed: u64,
    pub width_bits: u32,
    pub wire_delay: String,
    pub total_ticks: u64,
    pub events: Vec<TraceEvent>,
    pub trajectory_digest_fnv1a: String,
    pub final_circuit_nodes: Vec<String>,
    pub final_circuit_edges: Vec<EdgeRecord>,
}

struct Fnv1aHasher {
    hash: u128,
}

impl Fnv1aHasher {
    fn new() -> Self {
        Self {
            hash: 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d,
        }
    }

    fn write(&mut self, bytes: &[u8]) {
        const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;
        for &b in bytes {
            self.hash ^= u128::from(b);
            self.hash = self.hash.wrapping_mul(PRIME);
        }
    }

    fn finish_hex(&self) -> String {
        format!("{:032x}", self.hash)
    }
}

fn fixture_path(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests");
    p.push("fixtures");
    p.push("golden");
    p.push(format!("{name}.json"));
    p
}

struct RunConfig {
    name: &'static str,
    gen_cfg: GeneratorConfig,
    width: Width,
    wire_delay: WireDelayMode,
    seed: u64,
    horizon: u64,
}

fn execute_trace(cfg: &RunConfig) -> (GoldenTraceFixture, Vec<(u64, Vec<u64>, u64, Vec<u64>)>) {
    assert_not_holdout(cfg.seed);
    let seed_bank = SeedBank::new(cfg.seed);
    let generator = cfg.gen_cfg.build(cfg.width).expect("build generator");
    let mut env = NoisyEnvironment::new(generator, NoiseModel::None, seed_bank).expect("build env");

    let mut learner = ArkheLearner::new_with_spatial(
        cfg.width,
        1,
        cfg.wire_delay,
        WiringTopology::WellMixed,
        GridShape::new(8, 8).unwrap(),
        4,
        EconomyConfig::default(),
        GrowthConfig::default(),
        seed_bank,
    )
    .expect("build learner");

    let mut budget = TickBudget::new(100_000);
    let mut clean = [0u64; 1];
    let mut obs = [0u64; 1];

    let mut events = Vec::new();
    let mut prev_sprouts = 0;
    let mut prev_prunes = 0;
    let mut prev_rings = 0;

    let mut hasher = Fnv1aHasher::new();
    let mut tick_history = Vec::with_capacity(cfg.horizon as usize);

    for t in 0..cfg.horizon {
        env.step_frame(&mut clean, &mut obs);
        let pred = learner.predict(&mut budget);
        learner.observe(&obs, &mut budget);

        let cur_epoch = learner.editable().structure_epoch();
        let cur_reserves: Vec<u64> = learner
            .editable()
            .economy()
            .nodes()
            .iter()
            .map(|n| n.energy.to_bits())
            .collect();

        // Feed trajectory digest
        hasher.write(&t.to_le_bytes());
        for p in &pred {
            hasher.write(&p.to_le_bytes());
        }
        hasher.write(&cur_epoch.to_le_bytes());
        for r in &cur_reserves {
            hasher.write(&r.to_le_bytes());
        }

        tick_history.push((t, pred, cur_epoch, cur_reserves));

        // Detect structural events
        if learner.sprout_events() > prev_sprouts {
            prev_sprouts = learner.sprout_events();
            events.push(TraceEvent {
                tick: t,
                kind: "Sprout".to_owned(),
                node_count: learner.editable().circuit().node_count(),
                details: format!("sprout_events={}", learner.sprout_events()),
            });
        }
        if learner.starvation_prune_events() > prev_prunes {
            prev_prunes = learner.starvation_prune_events();
            events.push(TraceEvent {
                tick: t,
                kind: "StarvationPrune".to_owned(),
                node_count: learner.editable().circuit().node_count(),
                details: format!("prune_events={}", learner.starvation_prune_events()),
            });
        }
        if learner.ring_closure_events() > prev_rings {
            prev_rings = learner.ring_closure_events();
            events.push(TraceEvent {
                tick: t,
                kind: "RingClosure".to_owned(),
                node_count: learner.editable().circuit().node_count(),
                details: format!("ring_events={}", learner.ring_closure_events()),
            });
        }
    }

    let final_circuit_nodes = learner
        .editable()
        .circuit()
        .nodes()
        .iter()
        .map(|p| format!("{p:?}"))
        .collect();

    let final_circuit_edges = learner
        .editable()
        .circuit()
        .edges()
        .iter()
        .map(|e| EdgeRecord {
            src: format!("{:?}", e.src),
            dst: format!("{:?}", e.dst),
            hops: e.hops,
        })
        .collect();

    let fixture = GoldenTraceFixture {
        name: cfg.name.to_owned(),
        seed: cfg.seed,
        width_bits: u32::from(cfg.width.bits()),
        wire_delay: format!("{:?}", cfg.wire_delay),
        total_ticks: cfg.horizon,
        events,
        trajectory_digest_fnv1a: hasher.finish_hex(),
        final_circuit_nodes,
        final_circuit_edges,
    };

    (fixture, tick_history)
}

fn test_cases() -> Vec<RunConfig> {
    vec![
        RunConfig {
            name: "alt_k1_comb_s20",
            gen_cfg: GeneratorConfig::T1_Alternating { a: 0, b: 1 },
            width: Width::BITS_1,
            wire_delay: WireDelayMode::Combinational,
            seed: 20,
            horizon: 400,
        },
        RunConfig {
            name: "alt_k1_tph_s20",
            gen_cfg: GeneratorConfig::T1_Alternating { a: 0, b: 1 },
            width: Width::BITS_1,
            wire_delay: WireDelayMode::TicksPerHop,
            seed: 20,
            horizon: 400,
        },
        RunConfig {
            name: "alt_k2_comb_s21",
            gen_cfg: GeneratorConfig::T1_Alternating { a: 0, b: 3 },
            width: Width::BITS_2,
            wire_delay: WireDelayMode::Combinational,
            seed: 21,
            horizon: 400,
        },
        RunConfig {
            name: "alt_k2_tph_s21",
            gen_cfg: GeneratorConfig::T1_Alternating { a: 0, b: 3 },
            width: Width::BITS_2,
            wire_delay: WireDelayMode::TicksPerHop,
            seed: 21,
            horizon: 400,
        },
        RunConfig {
            name: "motif011_k1_comb_s22",
            gen_cfg: GeneratorConfig::T1_PeriodicMotif { motif: vec![0, 1, 1] },
            width: Width::BITS_1,
            wire_delay: WireDelayMode::Combinational,
            seed: 22,
            horizon: 500,
        },
        RunConfig {
            name: "motif013_k2_comb_s22",
            gen_cfg: GeneratorConfig::T1_PeriodicMotif { motif: vec![0, 1, 3] },
            width: Width::BITS_2,
            wire_delay: WireDelayMode::Combinational,
            seed: 22,
            horizon: 500,
        },
        RunConfig {
            name: "affine_k2_comb_s23",
            gen_cfg: GeneratorConfig::T1_AffineLag1 { mul_a: 1, add_b: 1, init: 0 },
            width: Width::BITS_2,
            wire_delay: WireDelayMode::Combinational,
            seed: 23,
            horizon: 500,
        },
        RunConfig {
            name: "affine_k2_tph_s23",
            gen_cfg: GeneratorConfig::T1_AffineLag1 { mul_a: 1, add_b: 1, init: 0 },
            width: Width::BITS_2,
            wire_delay: WireDelayMode::TicksPerHop,
            seed: 23,
            horizon: 500,
        },
    ]
}

#[test]
fn golden_traces_are_bit_identical() {
    let is_bless = std::env::var("ARKHE_BLESS").is_ok();
    let cases = test_cases();

    if is_bless {
        let fixture_dir = fixture_path("").parent().unwrap().to_path_buf();
        fs::create_dir_all(&fixture_dir).expect("create fixture dir");
    }

    for cfg in &cases {
        let path = fixture_path(cfg.name);
        let (actual_fixture, _actual_history) = execute_trace(cfg);

        if is_bless {
            let json = serde_json::to_string_pretty(&actual_fixture).expect("serialize fixture");
            fs::write(&path, json).expect("write golden fixture");
            println!("Blessed golden fixture: {}", path.display());
            continue;
        }

        assert!(
            path.exists(),
            "Golden fixture missing at {}. Run with ARKHE_BLESS=1 to generate.",
            path.display()
        );

        let golden_json = fs::read_to_string(&path).expect("read golden fixture");
        let expected_fixture: GoldenTraceFixture =
            serde_json::from_str(&golden_json).expect("deserialize fixture");

        if actual_fixture.trajectory_digest_fnv1a != expected_fixture.trajectory_digest_fnv1a {
            panic!(
                "Golden trace mismatch on case '{}'!\nExpected digest: {}\nActual digest:   {}\nEvents expected: {:?}\nEvents actual:   {:?}",
                cfg.name,
                expected_fixture.trajectory_digest_fnv1a,
                actual_fixture.trajectory_digest_fnv1a,
                expected_fixture.events,
                actual_fixture.events,
            );
        }

        assert_eq!(
            actual_fixture.final_circuit_edges, expected_fixture.final_circuit_edges,
            "Final circuit edges mismatch on case '{}'",
            cfg.name
        );
        assert_eq!(
            actual_fixture.events, expected_fixture.events,
            "Structural event sequence mismatch on case '{}'",
            cfg.name
        );
    }
}
