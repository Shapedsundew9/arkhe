use arkhe::baselines::{PredictorBaseline, TickBudget};
use arkhe::rng::SeedBank;
use arkhe::structural::{ArkheLearner, GrowthConfig};
use arkhe::substrate::{Width, WireDelayMode};

#[test]
fn concurrent_two_channel_development_combinational() {
    let mut learner = ArkheLearner::new(
        Width::BITS_1,
        2,
        WireDelayMode::Combinational,
        GrowthConfig::default(),
        SeedBank::new(42),
    )
    .expect("learner initializes");

    let mut budget = TickBudget::new(1_000_000);
    let motif_ch0 = [0u64, 1]; // period 2
    let motif_ch1 = [0u64, 1, 1]; // period 3
    let mut ch0_correct_tail = 0;
    let mut ch1_correct_tail = 0;

    for t in 0..1000 {
        let pred = learner.predict(&mut budget);
        let truth = [motif_ch0[t % 2], motif_ch1[t % 3]];
        if t >= 600 {
            if pred[0] == truth[0] {
                ch0_correct_tail += 1;
            }
            if pred[1] == truth[1] {
                ch1_correct_tail += 1;
            }
        }
        learner.observe(&truth, &mut budget);
    }

    let tel = learner.telemetry();
    assert_eq!(
        tel.sprout_blocked_max_nodes, 0,
        "max_nodes guardrail must not bind"
    );
    assert!(
        learner.sprout_events() >= 1,
        "must have sprouted: {}",
        learner.sprout_events()
    );
    assert_eq!(
        ch0_correct_tail, 400,
        "channel 0 (alternating) must achieve 100% tail accuracy within single-channel budget"
    );
    assert_eq!(
        ch1_correct_tail, 400,
        "channel 1 (periodic motif) must achieve 100% tail accuracy within single-channel budget"
    );
}

#[test]
fn concurrent_two_channel_development_ticks_per_hop() {
    let mut learner = ArkheLearner::new(
        Width::BITS_1,
        2,
        WireDelayMode::TicksPerHop,
        GrowthConfig::default(),
        SeedBank::new(77),
    )
    .expect("learner initializes");

    let mut budget = TickBudget::new(1_000_000);
    let motif_ch0 = [0u64, 1]; // period 2
    let motif_ch1 = [0u64, 1, 1]; // period 3
    let mut ch0_correct_tail = 0;
    let mut ch1_correct_tail = 0;

    for t in 0..1000 {
        let pred = learner.predict(&mut budget);
        let truth = [motif_ch0[t % 2], motif_ch1[t % 3]];
        if t >= 600 {
            if pred[0] == truth[0] {
                ch0_correct_tail += 1;
            }
            if pred[1] == truth[1] {
                ch1_correct_tail += 1;
            }
        }
        learner.observe(&truth, &mut budget);
    }

    let tel = learner.telemetry();
    assert_eq!(
        tel.sprout_blocked_max_nodes, 0,
        "max_nodes guardrail must not bind"
    );
    assert!(
        learner.sprout_events() >= 1,
        "must have sprouted: {}",
        learner.sprout_events()
    );
    assert_eq!(
        ch0_correct_tail, 400,
        "channel 0 must achieve 100% tail accuracy in TicksPerHop mode"
    );
    assert_eq!(
        ch1_correct_tail, 400,
        "channel 1 must achieve 100% tail accuracy in TicksPerHop mode"
    );
}
