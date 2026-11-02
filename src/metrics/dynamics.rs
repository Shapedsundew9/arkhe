//! Autonomous limit-cycle, transient, and attractor-basin measurement (Charter §2.6 & §9).

use std::collections::{BTreeMap, BTreeSet};

use rand_core::Rng;
use serde::{Deserialize, Serialize};

use crate::rng::{SeedBank, StreamDomain};
use crate::substrate::{BitParallel64State, CompiledCircuit, ScalarState};

/// Summary of an autonomous loopback trajectory ($\hat{A}(t+1) \to A(t+1)$).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CycleReport {
    /// Ticks before first entering the periodic attractor (`0` if initial state is already on the cycle).
    pub transient_ticks: usize,
    /// Exact period $P \ge 1$ of the autonomous state-register + loopback-output attractor.
    pub cycle_length: usize,
    /// Whether the cycle was closed within `max_steps`.
    pub converged: bool,
}

/// Summary of perturbed-state basin-of-attraction probing (§2.6).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BasinReport {
    /// Total perturbed initial states tested.
    pub probes: usize,
    /// Number of perturbed trajectories returning to the reference attractor cycle.
    pub converged_count: usize,
    /// Fraction `converged_count / probes` in `[0.0, 1.0]`.
    pub basin_fraction: f64,
    /// Mean ticks required for converging perturbations to re-enter the reference cycle.
    pub mean_recovery_ticks: f64,
}

/// Runs `compiled` in autonomous loopback ($\hat{y} \to x$) from its default register state
/// and extracts exact `(transient_ticks, cycle_length)`.
#[must_use]
pub fn detect_autonomous_cycle(compiled: &CompiledCircuit, max_steps: usize) -> CycleReport {
    let (report, _) = trace_autonomous_attractor(compiled, None, max_steps);
    report
}

fn trace_autonomous_attractor(
    compiled: &CompiledCircuit,
    initial_regs: Option<&[u64]>,
    max_steps: usize,
) -> (CycleReport, BTreeSet<Vec<u64>>) {
    let mut state = ScalarState::new(compiled);
    if let Some(regs) = initial_regs {
        state
            .set_registers(compiled, regs)
            .expect("valid register length");
    }

    let n_in = usize::from(compiled.num_inputs());
    let n_out = usize::from(compiled.num_outputs());
    let loopback_channels = n_in.min(n_out);
    let mut current_in = vec![0u64; n_in];
    let mut current_out = vec![0u64; n_out];

    // Full autonomous dynamical state = (current loopback input vector, internal registers).
    let make_key = |inp: &[u64], st: &ScalarState| -> Vec<u64> {
        let mut key = Vec::with_capacity(inp.len() + st.registers().len());
        key.extend_from_slice(inp);
        key.extend_from_slice(st.registers());
        key
    };

    let mut seen: BTreeMap<Vec<u64>, usize> = BTreeMap::new();
    let mut history_keys: Vec<Vec<u64>> = Vec::new();

    for step in 0..max_steps {
        let key = make_key(&current_in, &state);
        if let Some(&first_step) = seen.get(&key) {
            let cycle_set: BTreeSet<Vec<u64>> =
                history_keys[first_step..].iter().cloned().collect();
            return (
                CycleReport {
                    transient_ticks: first_step,
                    cycle_length: step - first_step,
                    converged: true,
                },
                cycle_set,
            );
        }
        seen.insert(key.clone(), step);
        history_keys.push(key);

        state
            .step(compiled, &current_in, &mut current_out)
            .expect("matching port slices");
        current_in.fill(0);
        current_in[..loopback_channels].copy_from_slice(&current_out[..loopback_channels]);
    }

    (
        CycleReport {
            transient_ticks: max_steps,
            cycle_length: 0,
            converged: false,
        },
        BTreeSet::new(),
    )
}

/// Estimates the fraction of perturbed register states that converge back to `compiled`'s default
/// autonomous attractor cycle, using 64-lane `BitParallel64State` acceleration when $k = 1$ (§9).
#[must_use]
pub fn estimate_attractor_basin(
    compiled: &CompiledCircuit,
    num_probes: usize,
    max_steps: usize,
    seeds: SeedBank,
) -> BasinReport {
    if num_probes == 0 {
        return BasinReport {
            probes: 0,
            converged_count: 0,
            basin_fraction: 0.0,
            mean_recovery_ticks: 0.0,
        };
    }

    let (ref_report, cycle_keys) = trace_autonomous_attractor(compiled, None, max_steps);
    if !ref_report.converged || compiled.total_registers() == 0 {
        let all = usize::from(ref_report.converged);
        return BasinReport {
            probes: num_probes,
            converged_count: all * num_probes,
            basin_fraction: all as f64,
            mean_recovery_ticks: 0.0,
        };
    }

    let num_regs = compiled.total_registers();
    let mut rng = seeds.stream(StreamDomain::BasinPerturbation { probe: 0 });

    if compiled.width().bits() == 1 {
        // Fast 64-lane bit-parallel basin sweep at k = 1!
        let mut converged_count = 0usize;
        let mut total_recovery = 0usize;
        let n_in = usize::from(compiled.num_inputs());
        let n_out = usize::from(compiled.num_outputs());
        let loopback = n_in.min(n_out);

        let mut done = 0usize;
        while done < num_probes {
            let batch = (num_probes - done).min(64);
            let mut packed_regs = vec![0u64; num_regs];
            for r in &mut packed_regs {
                *r = rng.next_u64();
            }
            let mut bp = BitParallel64State::new(compiled).expect("k == 1");
            bp.set_packed_registers(&packed_regs).unwrap();

            let mut p_in = vec![0u64; n_in];
            let mut p_out = vec![0u64; n_out];
            let mut lane_hit_tick: Vec<Option<usize>> = vec![None; batch];
            let mut remaining_lanes = batch;

            for step in 0..max_steps {
                for (lane, hit) in lane_hit_tick.iter_mut().enumerate() {
                    if hit.is_some() {
                        continue;
                    }
                    let mut key = Vec::with_capacity(n_in + num_regs);
                    for &w in &p_in {
                        key.push((w >> lane) & 1);
                    }
                    for &r in bp.registers() {
                        key.push((r >> lane) & 1);
                    }
                    if cycle_keys.contains(&key) {
                        *hit = Some(step);
                        remaining_lanes -= 1;
                    }
                }
                if remaining_lanes == 0 {
                    break;
                }
                bp.step(compiled, &p_in, &mut p_out).unwrap();
                p_in.fill(0);
                p_in[..loopback].copy_from_slice(&p_out[..loopback]);
            }

            for tick in lane_hit_tick.into_iter().flatten() {
                converged_count += 1;
                total_recovery += tick;
            }
            done += batch;
        }

        let mean_rec = if converged_count == 0 {
            0.0
        } else {
            (total_recovery as f64) / (converged_count as f64)
        };
        return BasinReport {
            probes: num_probes,
            converged_count,
            basin_fraction: (converged_count as f64) / (num_probes as f64),
            mean_recovery_ticks: mean_rec,
        };
    }

    // General k > 1 scalar perturbation sweep.
    let mask = compiled.width().mask();
    let n_in = usize::from(compiled.num_inputs());
    let n_out = usize::from(compiled.num_outputs());
    let loopback = n_in.min(n_out);
    let mut converged_count = 0usize;
    let mut total_recovery = 0usize;
    let mut st = ScalarState::new(compiled);

    for _ in 0..num_probes {
        let init_regs: Vec<u64> = (0..num_regs).map(|_| rng.next_u64() & mask).collect();
        st.set_registers(compiled, &init_regs).unwrap();
        let mut cur_in = vec![0u64; n_in];
        let mut cur_out = vec![0u64; n_out];

        for step in 0..max_steps {
            let mut key = Vec::with_capacity(n_in + num_regs);
            key.extend_from_slice(&cur_in);
            key.extend_from_slice(st.registers());
            if cycle_keys.contains(&key) {
                converged_count += 1;
                total_recovery += step;
                break;
            }
            st.step(compiled, &cur_in, &mut cur_out).unwrap();
            cur_in.fill(0);
            cur_in[..loopback].copy_from_slice(&cur_out[..loopback]);
        }
    }

    let mean_rec = if converged_count == 0 {
        0.0
    } else {
        (total_recovery as f64) / (converged_count as f64)
    };
    BasinReport {
        probes: num_probes,
        converged_count,
        basin_fraction: (converged_count as f64) / (num_probes as f64),
        mean_recovery_ticks: mean_rec,
    }
}
