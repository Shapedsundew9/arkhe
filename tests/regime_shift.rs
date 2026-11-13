//! Integration tests and exploratory latency measurements for stream regime shifts (`B6` / DR #31).

use arkhe::baselines::{PredictorBaseline, TickBudget};
use arkhe::env::{GeneratorConfig, NoiseModel, NoisyEnvironment};
use arkhe::rng::SeedBank;
use arkhe::structural::economy::EconomyConfig;
use arkhe::structural::growth::GrowthConfig;
use arkhe::structural::learner::ArkheLearner;
use arkhe::structural::observation_horizon;
use arkhe::substrate::{Width, WireDelayMode};

#[derive(Debug, Clone)]
struct RegimeShiftTrial {
    obs_horizon: u64,
    correct_pre_shift: usize,
    starve_tick: Option<u64>,
    resprout_tick: Option<u64>,
    reserve_clipped_count: u64,
    reserve_clipped_bits: f64,
    sprout_blocked_max_nodes: u64,
}

impl RegimeShiftTrial {
    fn starvation_latency(&self, shift_tick: u64) -> Option<u64> {
        self.starve_tick.map(|t| t - shift_tick)
    }

    fn replacement_latency(&self, shift_tick: u64) -> Option<u64> {
        self.resprout_tick.map(|t| t - shift_tick)
    }
}

fn run_regime_shift_trial(
    gen_a_cfg: &GeneratorConfig,
    gen_b_cfg: &GeneratorConfig,
    noise_b: &NoiseModel,
    width: Width,
    seed: u64,
    shift_tick: u64,
    total_ticks: u64,
) -> RegimeShiftTrial {
    let econ = EconomyConfig::default();
    let growth = GrowthConfig::default();
    let obs_horizon = observation_horizon(&econ, &growth) as u64;
    let seed_bank = SeedBank::new(seed);

    let gen_a = gen_a_cfg.build(width).expect("generator A");
    let gen_b = gen_b_cfg.build(width).expect("generator B");
    let mut env_a = NoisyEnvironment::new(gen_a, NoiseModel::None, seed_bank).expect("env A");
    let mut env_b = NoisyEnvironment::new(gen_b, noise_b.clone(), seed_bank).expect("env B");

    let mut learner = ArkheLearner::new_with_config(
        width,
        1,
        WireDelayMode::Combinational,
        econ,
        growth,
        seed_bank,
    )
    .expect("learner");

    let mut budget = TickBudget::new(1_000_000);
    let mut correct_pre_shift = 0usize;
    let mut clean = [0u64; 1];
    let mut obs = [0u64; 1];

    for t in 0..shift_tick {
        env_a.step_frame(&mut clean, &mut obs);
        let pred = learner.predict(&mut budget);
        if pred[0] == obs[0] && t > 200 {
            correct_pre_shift += 1;
        }
        learner.observe(&obs, &mut budget);
    }

    let pre_shift_prunes = learner.starvation_prune_events();
    let pre_shift_events = learner.sprout_events() + learner.ring_closure_events();
    let mut starve_tick = None;
    let mut resprout_tick = None;

    for t in shift_tick..total_ticks {
        env_b.step_frame(&mut clean, &mut obs);
        let _ = learner.predict(&mut budget);
        learner.observe(&obs, &mut budget);

        if starve_tick.is_none() && learner.starvation_prune_events() > pre_shift_prunes {
            starve_tick = Some(t);
        }
        if starve_tick.is_some()
            && resprout_tick.is_none()
            && (learner.sprout_events() + learner.ring_closure_events()) > pre_shift_events
        {
            resprout_tick = Some(t);
        }
    }

    let tel = learner.telemetry();
    RegimeShiftTrial {
        obs_horizon,
        correct_pre_shift,
        starve_tick,
        resprout_tick,
        reserve_clipped_count: tel.reserve_clipped_count,
        reserve_clipped_bits: tel.reserve_clipped_bits,
        sprout_blocked_max_nodes: tel.sprout_blocked_max_nodes,
    }
}

fn quantile_u64(sorted: &[u64], numerator: usize, denominator: usize) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let idx = ((sorted.len() - 1) * numerator) / denominator;
    sorted[idx] as f64
}

#[test]
fn regime_shift_measures_starvation_and_replacement_latency() {
    let shift_tick = 350;
    let total_ticks = 800;
    let gen_a = GeneratorConfig::T1_PeriodicMotif {
        motif: vec![0, 0, 0, 0, 1],
    };
    let gen_b = GeneratorConfig::T1_Alternating { a: 0, b: 1 };

    let trial = run_regime_shift_trial(
        &gen_a,
        &gen_b,
        &NoiseModel::None,
        Width::BITS_1,
        42,
        shift_tick,
        total_ticks,
    );

    println!(
        "B6 single-seed regime shift: shift_tick={shift_tick}, W_obs={}, starve_tick={:?}, delta_starve={:?}, resprout_tick={:?}, clipped_count={}, clipped_bits={:.3}",
        trial.obs_horizon,
        trial.starve_tick,
        trial.starvation_latency(shift_tick),
        trial.resprout_tick,
        trial.reserve_clipped_count,
        trial.reserve_clipped_bits,
    );

    assert_eq!(trial.sprout_blocked_max_nodes, 0);
    assert!(
        trial.correct_pre_shift > 100,
        "Task A must be learned pre-shift, got {} correct",
        trial.correct_pre_shift
    );
    assert!(
        trial.starve_tick.is_some(),
        "Obsolete structure must starve after regime shift"
    );
    assert!(
        trial.resprout_tick.is_some(),
        "Replacement structure must sprout for Task B after obsolete structure is pruned"
    );
}

#[test]
fn regime_shift_multi_seed_latency_distribution() {
    let shift_tick = 400u64;
    let total_ticks = 1400u64;
    let seeds = 0u64..10u64;

    let scenarios = [
        (
            "Motif5->Alt(k=1,clean)",
            Width::BITS_1,
            GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 0, 0, 0, 1],
            },
            GeneratorConfig::T1_Alternating { a: 0, b: 1 },
            NoiseModel::None,
        ),
        (
            "Alt(0,1)->Constant(0)(k=1)",
            Width::BITS_1,
            GeneratorConfig::T1_Alternating { a: 0, b: 1 },
            GeneratorConfig::T1_Constant { value: 0 },
            NoiseModel::None,
        ),
        (
            "Motif013->Mod3(k=2,clean)",
            Width::BITS_2,
            GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 1, 3],
            },
            GeneratorConfig::T3b_CounterModN {
                modulus: 3,
                init: 0,
            },
            NoiseModel::None,
        ),
        (
            "Motif5->Alt(k=1,eps=0.05)",
            Width::BITS_1,
            GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 0, 0, 0, 1],
            },
            GeneratorConfig::T1_Alternating { a: 0, b: 1 },
            NoiseModel::IndependentSubstitution {
                epsilon: 0.05,
                channels: vec![0],
            },
        ),
    ];

    println!(
        "\n| Pair | Seeds | W_obs | Starved | Resprouted | Within<=W_obs | Latency min/p50/mean/p95/max | Max/W_obs | ClippedCount mean | ClippedBits mean |"
    );
    println!("| :--- | ---: | ---: | ---: | ---: | ---: | :--- | ---: | ---: | ---: |");

    for (label, width, gen_a, gen_b, noise_b) in scenarios {
        let mut latencies = Vec::new();
        let mut resprouts = 0usize;
        let mut within_w_obs = 0usize;
        let mut clipped_count_sum = 0u64;
        let mut clipped_bits_sum = 0.0f64;
        let mut obs_horizon = 0u64;

        for seed in seeds.clone() {
            let trial = run_regime_shift_trial(
                &gen_a,
                &gen_b,
                &noise_b,
                width,
                seed,
                shift_tick,
                total_ticks,
            );
            assert_eq!(
                trial.sprout_blocked_max_nodes, 0,
                "{label} seed {seed} bound max_nodes guardrail"
            );
            obs_horizon = trial.obs_horizon;
            clipped_count_sum += trial.reserve_clipped_count;
            clipped_bits_sum += trial.reserve_clipped_bits;
            if let Some(lat) = trial.starvation_latency(shift_tick) {
                latencies.push(lat);
                if lat <= trial.obs_horizon {
                    within_w_obs += 1;
                }
            }
            if trial.replacement_latency(shift_tick).is_some() {
                resprouts += 1;
            }
        }

        latencies.sort_unstable();
        let n = seeds.clone().count();
        let starved_n = latencies.len();
        let min_lat = latencies.first().copied().unwrap_or(0);
        let max_lat = latencies.last().copied().unwrap_or(0);
        let p50 = quantile_u64(&latencies, 1, 2);
        let p95 = quantile_u64(&latencies, 19, 20);
        let mean_lat = if starved_n > 0 {
            latencies.iter().sum::<u64>() as f64 / starved_n as f64
        } else {
            f64::NAN
        };
        let ratio = max_lat as f64 / obs_horizon.max(1) as f64;
        println!(
            "| {label} | {n} | {obs_horizon} | {starved_n}/{n} | {resprouts}/{n} | {within_w_obs}/{n} | {min_lat} / {p50:.1} / {mean_lat:.1} / {p95:.1} / {max_lat} | {ratio:.3}x | {:.1} | {:.1} |",
            clipped_count_sum as f64 / n as f64,
            clipped_bits_sum / n as f64,
        );
    }
}
