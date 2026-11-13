//! Integration test exercising stream regime shifts and obsolete structure starvation latency (§2.2).

use arkhe::baselines::{PredictorBaseline, TickBudget};
use arkhe::env::{GeneratorConfig, NoiseModel, NoisyEnvironment};
use arkhe::rng::SeedBank;
use arkhe::structural::economy::EconomyConfig;
use arkhe::structural::growth::GrowthConfig;
use arkhe::structural::learner::ArkheLearner;
use arkhe::substrate::{Width, WireDelayMode};

#[test]
fn regime_shift_measures_starvation_and_replacement_latency() {
    let width = Width::BITS_1;
    let seed = 42u64;
    let seed_bank = SeedBank::new(seed);

    let gen_a = GeneratorConfig::T1_PeriodicMotif {
        motif: vec![0, 0, 0, 0, 1],
    }
    .build(width)
    .expect("generator A");
    let gen_b = GeneratorConfig::T1_Alternating { a: 0, b: 1 }
        .build(width)
        .expect("generator B");

    let mut env_a = NoisyEnvironment::new(gen_a, NoiseModel::None, seed_bank).expect("env A");
    let mut env_b = NoisyEnvironment::new(gen_b, NoiseModel::None, seed_bank).expect("env B");

    let mut learner = ArkheLearner::new_with_config(
        width,
        1,
        WireDelayMode::Combinational,
        EconomyConfig::default(),
        GrowthConfig::default(),
        seed_bank,
    )
    .expect("learner");

    let mut budget = TickBudget::new(100_000);
    let mut correct_pre_shift = 0;
    let shift_tick = 350;
    let total_ticks = 800;

    let mut starved_after_shift = false;
    let mut starve_tick = None;
    let mut resprout_tick = None;

    let mut clean = [0u64; 1];
    let mut obs = [0u64; 1];

    // Phase 1: Stream A
    for t in 0..shift_tick {
        env_a.step_frame(&mut clean, &mut obs);
        let pred = learner.predict(&mut budget);
        if pred[0] == obs[0] && t > 200 {
            correct_pre_shift += 1;
        }
        learner.observe(&obs, &mut budget);
    }

    assert!(
        correct_pre_shift > 100,
        "Circuit should learn task A before regime shift, got {correct_pre_shift} correct"
    );
    let initial_events = learner.sprout_events() + learner.ring_closure_events();
    assert!(initial_events > 0, "Learner must grow on task A");

    // Phase 2: Stream B (regime shift)
    for t in shift_tick..total_ticks {
        env_b.step_frame(&mut clean, &mut obs);
        let _ = learner.predict(&mut budget);
        learner.observe(&obs, &mut budget);

        if !starved_after_shift && learner.starvation_prune_events() > 0 {
            starved_after_shift = true;
            starve_tick = Some(t);
        }
        if starve_tick.is_some()
            && resprout_tick.is_none()
            && (learner.sprout_events() + learner.ring_closure_events()) > initial_events
        {
            resprout_tick = Some(t);
        }
    }

    let tel = learner.telemetry();
    println!(
        "Phase 0 regime shift baseline: shift_tick={}, starve_tick={:?}, resprout_tick={:?}, prunes={}, sprouts={}, clipped_count={}, clipped_bits={}",
        shift_tick,
        starve_tick,
        resprout_tick,
        learner.starvation_prune_events(),
        learner.sprout_events(),
        tel.reserve_clipped_count,
        tel.reserve_clipped_bits
    );

    // In Phase 3, verify that obsolete structure starves within W_obs and replacement sprouts (§2.2, Option B):
    assert!(correct_pre_shift > 100, "Task A must be learned pre-shift");
    assert!(
        starve_tick.is_some(),
        "Obsolete structure must starve after regime shift within observation horizon"
    );
    let starve_t = starve_tick.expect("starve tick recorded");
    assert!(
        starve_t - shift_tick <= 256,
        "Starvation must occur within W_obs (256 ticks), occurred at tick {starve_t} (delta {})",
        starve_t - shift_tick
    );
    assert!(
        resprout_tick.is_some(),
        "New structure must sprout for Task B after obsolete structure is pruned"
    );
}
