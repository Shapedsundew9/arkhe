//! Integration tests and exploratory multi-channel lock-time measurements (`B7` / DR #34).

use arkhe::baselines::{PredictorBaseline, TickBudget};
use arkhe::config::CliffCountersSummary;
use arkhe::rng::SeedBank;
use arkhe::structural::{ArkheLearner, GrowthConfig};
use arkhe::substrate::{Width, WireDelayMode};

fn measure_channel_lock_ticks(
    motifs: &[&[u64]],
    width: Width,
    wire_delay: WireDelayMode,
    seed: u64,
    horizon: usize,
    min_tail_streak: usize,
) -> (
    Vec<Option<usize>>,
    Vec<usize>,
    CliffCountersSummary,
    Vec<usize>,
) {
    let growth = GrowthConfig::default();
    let num_channels = motifs.len();
    let num_channels_u16 = u16::try_from(num_channels).expect("channel count fits in u16");
    let mut learner = ArkheLearner::new(
        width,
        num_channels_u16,
        wire_delay,
        growth,
        SeedBank::new(seed),
    )
    .expect("learner initializes");

    let mut budget = TickBudget::new(2_000_000);
    let mut active_streak_start: Vec<Option<usize>> = vec![None; num_channels];
    let mut truth = vec![0u64; num_channels];
    let mut tail_correct = vec![0usize; num_channels];

    for t in 0..horizon {
        let pred = learner.predict(&mut budget);
        for c in 0..num_channels {
            let motif = motifs[c];
            truth[c] = motif[t % motif.len()];
            if pred[c] == truth[c] {
                if active_streak_start[c].is_none() {
                    active_streak_start[c] = Some(t);
                }
                if t >= 600 {
                    tail_correct[c] += 1;
                }
            } else {
                active_streak_start[c] = None;
            }
        }
        learner.observe(&truth, &mut budget);
    }

    let lock_ticks: Vec<Option<usize>> = active_streak_start
        .into_iter()
        .map(|start| start.filter(|&s| horizon.saturating_sub(s) >= min_tail_streak))
        .collect();
    let snapshot_counts: Vec<usize> = (0..num_channels)
        .map(|c| {
            learner
                .port_epoch_snapshots(c as u16)
                .expect("valid port index")
                .len()
        })
        .collect();
    assert!(
        learner.port_epoch_snapshots(num_channels as u16).is_none(),
        "out-of-range port snapshot must be None"
    );

    let tel = learner.telemetry();
    assert_eq!(
        tel.sprout_blocked_max_nodes, 0,
        "max_nodes guardrail must not bind"
    );
    assert!(
        learner.sprout_events() + learner.ring_closure_events() >= 1,
        "must synthesize structure"
    );
    let summary = CliffCountersSummary::from(&tel);
    (lock_ticks, tail_correct, summary, snapshot_counts)
}

fn verify_concurrent_two_channel_tail(wire_delay: WireDelayMode, seed: u64) {
    let motifs: [&[u64]; 2] = [&[0, 1], &[0, 1, 1]];
    let (_, tail_correct, summary, _) =
        measure_channel_lock_ticks(&motifs, Width::BITS_1, wire_delay, seed, 1000, 64);

    assert_eq!(summary.sprout_blocked_max_nodes, 0);
    assert_eq!(
        tail_correct[0], 400,
        "channel 0 (alternating) must achieve 100% tail accuracy in {wire_delay:?} mode"
    );
    assert_eq!(
        tail_correct[1], 400,
        "channel 1 (periodic motif) must achieve 100% tail accuracy in {wire_delay:?} mode"
    );
}

#[test]
fn concurrent_two_channel_development_combinational() {
    verify_concurrent_two_channel_tail(WireDelayMode::Combinational, 42);
}

#[test]
fn concurrent_two_channel_development_ticks_per_hop() {
    verify_concurrent_two_channel_tail(WireDelayMode::TicksPerHop, 77);
}

fn f64_quantile(sorted: &[f64], numerator: usize, denominator: usize) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let idx = ((sorted.len() - 1) * numerator) / denominator;
    sorted[idx]
}

#[test]
fn multi_channel_lock_time_ratio_measurement() {
    let horizon = 1200usize;
    let tail_streak = GrowthConfig::default().diagnostic_window;
    let seeds = 0u64..10u64;

    let pairs: [(&str, &[&[u64]], Width); 2] = [
        ("K=2:(Alt,Motif011)", &[&[0, 1], &[0, 1, 1]], Width::BITS_1),
        (
            "K=3:(Alt,Motif011,Motif001)",
            &[&[0, 1], &[0, 1, 1], &[0, 0, 1]],
            Width::BITS_1,
        ),
    ];

    println!(
        "\n| Setting | Channel | Locked | Solo p50/max | Multi p50/max | Ratio min/p50/mean/p95/max | MaxSnapshots |"
    );
    println!("| :--- | ---: | ---: | :--- | :--- | :--- | ---: |");

    for (pair_label, motifs, width) in pairs {
        for wire_delay in [WireDelayMode::Combinational, WireDelayMode::TicksPerHop] {
            let num_channels = motifs.len();
            let mut solo_by_ch = vec![Vec::<usize>::new(); num_channels];
            let mut multi_by_ch = vec![Vec::<usize>::new(); num_channels];
            let mut ratios_by_ch = vec![Vec::<f64>::new(); num_channels];
            let mut max_snaps = vec![0usize; num_channels];

            for seed in seeds.clone() {
                let mut solo_locks = Vec::with_capacity(num_channels);
                for ch_motif in motifs {
                    let (locks, tail, summary, _) = measure_channel_lock_ticks(
                        &[*ch_motif],
                        width,
                        wire_delay,
                        seed,
                        horizon,
                        tail_streak,
                    );
                    assert_eq!(summary.sprout_blocked_max_nodes, 0);
                    assert_eq!(
                        tail[0],
                        horizon - 600,
                        "solo channel failed tail accuracy on {pair_label} {wire_delay:?} seed {seed}"
                    );
                    solo_locks.push(locks[0]);
                }

                let (multi_locks, multi_tail, summary, snap_counts) = measure_channel_lock_ticks(
                    motifs,
                    width,
                    wire_delay,
                    seed,
                    horizon,
                    tail_streak,
                );
                assert_eq!(summary.sprout_blocked_max_nodes, 0);

                for c in 0..num_channels {
                    assert_eq!(
                        multi_tail[c],
                        horizon - 600,
                        "multi channel {c} failed tail accuracy on {pair_label} {wire_delay:?} seed {seed}"
                    );
                    max_snaps[c] = max_snaps[c].max(snap_counts[c]);
                    if let (Some(solo_t), Some(multi_t)) = (solo_locks[c], multi_locks[c]) {
                        solo_by_ch[c].push(solo_t);
                        multi_by_ch[c].push(multi_t);
                        ratios_by_ch[c].push((multi_t as f64) / (solo_t.max(1) as f64));
                    }
                }
            }

            let total_seeds = seeds.clone().count();
            for c in 0..num_channels {
                let mut solo_f: Vec<f64> = solo_by_ch[c].iter().map(|&v| v as f64).collect();
                let mut multi_f: Vec<f64> = multi_by_ch[c].iter().map(|&v| v as f64).collect();
                let mut ratios = ratios_by_ch[c].clone();
                solo_f.sort_by(f64::total_cmp);
                multi_f.sort_by(f64::total_cmp);
                ratios.sort_by(f64::total_cmp);

                let n = ratios.len();
                let mean_r = ratios.iter().sum::<f64>() / n.max(1) as f64;
                println!(
                    "| {pair_label} {wire_delay:?} | ch{c} | {n}/{total_seeds} | {:.0}/{:.0} | {:.0}/{:.0} | {:.2}/{:.2}/{:.2}/{:.2}/{:.2}x | {} |",
                    f64_quantile(&solo_f, 1, 2),
                    solo_f.last().copied().unwrap_or(f64::NAN),
                    f64_quantile(&multi_f, 1, 2),
                    multi_f.last().copied().unwrap_or(f64::NAN),
                    ratios.first().copied().unwrap_or(f64::NAN),
                    f64_quantile(&ratios, 1, 2),
                    mean_r,
                    f64_quantile(&ratios, 19, 20),
                    ratios.last().copied().unwrap_or(f64::NAN),
                    max_snaps[c],
                );
            }
        }
    }
}
