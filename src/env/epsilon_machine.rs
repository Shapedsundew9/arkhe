//! Exact $\varepsilon$-machine ground-truth solver and noise-entropy helpers (Charter §5.2).
//!
//! Computes statistical complexity $C_\mu$ (bits), irreducible entropy rate $h_\mu$ (bits/tick),
//! minimal causal-state register lower bound $\lceil C_\mu / k \rceil$, exact noiseless
//! synchronisation order $L_{\text{sync}}$, and bounded belief-quantised Mixed-State Presentation
//! (MSP) synchronisation estimates under declared channel noise.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::env::generator::{EpsilonMachineScope, FiniteStateSpec};
use crate::env::noise::NoiseModel;
use crate::substrate::Width;

/// Validity and approximation status of [`EpsilonMachineSummary::noisy_msp_approx_sync_ticks`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum MspSyncStatus {
    /// Process is noiseless ($\varepsilon = 0$); exact `exact_sync_order` applies instead.
    NotApplicableNoiseless,
    /// Trivial single-state attractor ($C_\mu = 0$): 0 ticks required exactly.
    ExactZeroComplexity,
    /// Bounded, belief-quantised Mixed-State Presentation estimate converged within caps.
    BoundedQuantizedEstimate {
        /// Posterior probability rounding scale (`1_000_000_000`).
        quant_grid: u64,
        /// Maximum belief-update steps explored (`256`).
        max_steps: usize,
        /// Maximum quantised belief states retained per step (`2_048`).
        max_mixed_states: usize,
    },
    /// Joint per-tick alphabet exceeded branch enumeration cap ($k \cdot K > 4$ bits).
    SkippedAlphabetCap {
        /// Actual joint bits per tick ($k \cdot K$).
        total_bits: usize,
        /// Enumeration cap (`4` bits = `16` joint symbols).
        max_bits: usize,
    },
    /// Quantised belief frontier exceeded `max_mixed_states` before reaching $< 0.01\text{ bits}$ entropy.
    ExceededMixedStateCap {
        /// Frontier cap hit (`2_048`).
        max_mixed_states: usize,
    },
    /// Expected posterior phase entropy remained $\ge 0.01\text{ bits}$ after `max_steps` ticks.
    ExceededStepCap {
        /// Step cap hit (`256`).
        max_steps: usize,
    },
}

/// Ground-truth computational-mechanics summary of an environment process (§5.2).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EpsilonMachineSummary {
    /// Whether these metrics describe the entire stationary run or only a curriculum's final stage.
    pub scope: EpsilonMachineScope,
    /// Number of recurrent causal states $|\mathcal{S}|$ on the stationary attractor.
    pub causal_state_count: usize,
    /// Transient ticks from `initial_state` until entering the recurrent attractor cycle.
    pub generator_transient_ticks: usize,
    /// Statistical complexity $C_\mu = H(\mathcal{S})$ in bits.
    pub c_mu_bits: f64,
    /// Irreducible entropy rate $h_\mu = H(X \mid \mathcal{S})$ in bits per tick (summed across channels).
    pub h_mu_bits_per_tick: f64,
    /// Entropy rate normalised per channel-symbol ($h_\mu / K$).
    pub h_mu_bits_per_symbol: f64,
    /// Universal causal-state lower bound on total accessible width-$k$ registers: $\lceil C_\mu / k \rceil$.
    pub min_delay_registers: usize,
    /// Exact worst-case observation history length to uniquely identify the causal state (`Some` when $\varepsilon = 0$).
    pub exact_sync_order: Option<usize>,
    /// Bounded, quantised MSP estimate of ticks required for expected posterior phase entropy $< 0.01\text{ bits}$.
    pub noisy_msp_approx_sync_ticks: Option<usize>,
    /// Explicit status/bounds governing `noisy_msp_approx_sync_ticks`.
    pub msp_sync_status: MspSyncStatus,
}

/// Computes the stationary $\varepsilon$-machine metrics for `spec` under `noise`.
#[must_use]
pub fn solve_epsilon_machine(spec: &FiniteStateSpec, noise: &NoiseModel) -> EpsilonMachineSummary {
    assert!(spec.num_states >= 1);
    assert!(spec.initial_state < spec.num_states);

    // 1. Trace from initial_state to extract transient length `t0` and recurrent cycle `C`.
    let mut visited_at: BTreeMap<usize, usize> = BTreeMap::new();
    let mut trajectory: Vec<usize> = Vec::new();
    let mut curr = spec.initial_state;
    let transient_ticks = loop {
        if let Some(&first_idx) = visited_at.get(&curr) {
            break first_idx;
        }
        visited_at.insert(curr, trajectory.len());
        trajectory.push(curr);
        curr = spec.transitions[curr].0;
    };

    let cycle_states = &trajectory[transient_ticks..];
    let raw_period = cycle_states.len();

    // 2. Identify active noisy channels and check whether noise completely destroys information
    //    (at the uniform white-noise point 1 - eps == eps / (2^k - 1), i.e., eps == 1 - 2^{-k}).
    let num_channels = usize::from(spec.channels);
    let k_bits = spec.width.bits();
    let uniform_eps = if k_bits == 64 {
        1.0
    } else {
        1.0 - 1.0 / ((1u128 << k_bits) as f64)
    };

    let channel_eps = per_channel_epsilon(noise, num_channels);

    // Build effective observable symbol sequence along the attractor cycle, masking out any channel
    // that is operating at the exact uniform memoryless white-noise point `epsilon == 1 - 2^{-k}`.
    let cycle_emissions: Vec<Vec<u64>> = cycle_states
        .iter()
        .map(|&s| {
            spec.transitions[s]
                .1
                .iter()
                .enumerate()
                .map(|(ch, &val)| {
                    if (channel_eps[ch] - uniform_eps).abs() < 1e-15 {
                        0
                    } else {
                        val
                    }
                })
                .collect()
        })
        .collect();

    // 3. Causal-state minimisation on the periodic recurrent emission sequence:
    //    Two phases j1, j2 belong to the same causal state iff their infinite future emission
    //    morphs are identical, which reduces raw_period to its minimal period `p_min`.
    let p_min = minimal_period(&cycle_emissions);
    debug_assert_eq!(raw_period % p_min, 0);

    let c_mu_bits = if p_min <= 1 {
        0.0
    } else {
        (p_min as f64).log2()
    };

    let min_delay_registers = if p_min <= 1 {
        0
    } else {
        // Exact integer ceiling of log2(p_min) / k: smallest d such that (2^k)^d >= p_min.
        let mut d = 0usize;
        let mut cap = 1u128;
        let base = spec.width.cardinality();
        while cap < (p_min as u128) {
            d += 1;
            cap = cap.saturating_mul(base);
        }
        d
    };

    // 4. Exact stationary entropy rate h_mu across all channels.
    let mut h_mu_bits_per_tick = 0.0f64;
    for &eps in &channel_eps {
        h_mu_bits_per_tick += channel_substitution_entropy_bits(eps, k_bits);
    }
    let h_mu_bits_per_symbol = if num_channels == 0 {
        0.0
    } else {
        h_mu_bits_per_tick / (num_channels as f64)
    };

    let any_noise = channel_eps.iter().any(|&e| e > 0.0);

    // 5. Synchronisation analysis:
    //    - When eps == 0: exact minimal word length L in 0..=p_min that uniquely identifies phase.
    //    - When eps > 0: bounded, belief-quantised MSP estimate on the p_min causal states.
    let (exact_sync_order, noisy_msp_approx_sync_ticks, msp_sync_status) = if !any_noise {
        let sync = exact_noiseless_sync_order(&cycle_emissions[..p_min]);
        (Some(sync), None, MspSyncStatus::NotApplicableNoiseless)
    } else if p_min == 1 {
        (Some(0), Some(0), MspSyncStatus::ExactZeroComplexity)
    } else {
        let (msp_ticks, status) =
            estimate_msp_sync_ticks(&cycle_emissions[..p_min], &channel_eps, k_bits);
        (None, msp_ticks, status)
    };

    EpsilonMachineSummary {
        scope: spec.scope,
        causal_state_count: p_min,
        generator_transient_ticks: transient_ticks,
        c_mu_bits,
        h_mu_bits_per_tick,
        h_mu_bits_per_symbol,
        min_delay_registers,
        exact_sync_order,
        noisy_msp_approx_sync_ticks,
        msp_sync_status,
    }
}

fn per_channel_epsilon(noise: &NoiseModel, num_channels: usize) -> Vec<f64> {
    let mut channel_eps = vec![0.0f64; num_channels];
    if let NoiseModel::IndependentSubstitution { epsilon, channels } = noise {
        for &ch in channels {
            if usize::from(ch) < num_channels {
                channel_eps[usize::from(ch)] = *epsilon;
            }
        }
    }
    channel_eps
}

fn log2_alternatives(k_bits: u8) -> f64 {
    if k_bits == 1 {
        0.0
    } else if k_bits == 64 {
        64.0
    } else {
        (((1u64 << k_bits) - 1) as f64).log2()
    }
}

/// Minimal period $p_{\min} \ge 1$ of a non-empty periodic slice `seq`.
fn minimal_period(seq: &[Vec<u64>]) -> usize {
    let n = seq.len();
    for p in 1..n {
        if n.is_multiple_of(p) && (0..n).all(|i| seq[i] == seq[i % p]) {
            return p;
        }
    }
    n
}

/// Exact conditional Shannon entropy (in bits) of a $k$-bit symbol corrupted with rate $\varepsilon$.
#[must_use]
pub fn channel_substitution_entropy_bits(eps: f64, k_bits: u8) -> f64 {
    if eps <= 0.0 {
        return 0.0;
    }
    let p_keep = 1.0 - eps;
    let h_binary = -p_keep * p_keep.log2() - eps * eps.log2();
    h_binary + eps * log2_alternatives(k_bits)
}

/// Analytical conditional channel-noise entropy per symbol declared by `noise` (even when $|S|$ exceeds export caps).
#[must_use]
pub fn declared_noise_entropy_bits_per_symbol(
    noise: &NoiseModel,
    num_channels: u16,
    width: Width,
) -> f64 {
    let n_ch = usize::from(num_channels);
    if n_ch == 0 {
        return 0.0;
    }
    let channel_eps = per_channel_epsilon(noise, n_ch);
    let total: f64 = channel_eps
        .iter()
        .map(|&eps| channel_substitution_entropy_bits(eps, width.bits()))
        .sum();
    total / (n_ch as f64)
}

/// Exact single-tick conditional log-loss $-\log_2 \Pr(Y_t = \text{observed} \mid X_t = \text{clean})$
/// incurred by the clean-phase Bayes-optimal oracle on a realized `(clean_frame, observed_frame)` pair.
///
/// Used to compute paired oracle regret on finite noisy trajectories so $O(T^{-1/2})$ channel-draw
/// luck cancels out identically across predictors.
#[must_use]
pub fn realized_oracle_step_bits(
    noise: &NoiseModel,
    width: Width,
    clean_frame: &[u64],
    observed_frame: &[u64],
) -> Option<f64> {
    if clean_frame.len() != observed_frame.len() {
        return None;
    }
    let n_ch = clean_frame.len();
    let k_bits = width.bits();
    let channel_eps = per_channel_epsilon(noise, n_ch);
    let alt_bits = log2_alternatives(k_bits);

    let mut bits = 0.0f64;
    for ch in 0..n_ch {
        let eps = channel_eps[ch];
        let matched = width.normalize(clean_frame[ch]) == width.normalize(observed_frame[ch]);
        if eps <= 0.0 {
            if !matched {
                return None;
            }
        } else if eps < 1.0 {
            if matched {
                bits += -(1.0 - eps).log2();
            } else {
                bits += -eps.log2() + alt_bits;
            }
        } else {
            return None;
        }
    }
    Some(bits)
}

/// Shortest observation window length $L \in 0..=p_{\min}$ that uniquely determines causal phase.
fn exact_noiseless_sync_order(minimal_cycle: &[Vec<u64>]) -> usize {
    let p = minimal_cycle.len();
    if p <= 1 {
        return 0;
    }
    for len in 1..=p {
        let mut seen: BTreeSet<Vec<&[u64]>> = BTreeSet::new();
        let mut all_unique = true;
        for start in 0..p {
            let word: Vec<&[u64]> = (0..len)
                .map(|offset| minimal_cycle[(start + offset) % p].as_slice())
                .collect();
            if !seen.insert(word) {
                all_unique = false;
                break;
            }
        }
        if all_unique {
            return len;
        }
    }
    p
}

/// Bounded, belief-quantised Mixed-State Presentation (MSP) belief propagation for small noisy alphabets,
/// estimating the smallest step $t \le \text{max\_steps}$ where expected posterior phase entropy $< 0.01\text{ bits}$.
fn estimate_msp_sync_ticks(
    minimal_cycle: &[Vec<u64>],
    channel_eps: &[f64],
    k_bits: u8,
) -> (Option<usize>, MspSyncStatus) {
    const QUANT_GRID: u64 = 1_000_000_000;
    const MAX_STEPS: usize = 256;
    const MAX_MIXED_STATES: usize = 2_048;
    const MAX_TOTAL_BITS: usize = 4;

    let p = minimal_cycle.len();
    let num_channels = channel_eps.len();
    let total_bits = usize::from(k_bits).saturating_mul(num_channels);
    if total_bits > MAX_TOTAL_BITS {
        return (
            None,
            MspSyncStatus::SkippedAlphabetCap {
                total_bits,
                max_bits: MAX_TOTAL_BITS,
            },
        );
    }
    let alpha_card = 1usize << total_bits;
    let sym_card = 1usize << k_bits;

    let uniform = vec![1.0 / (p as f64); p];
    let mut frontier: Vec<(f64, Vec<f64>)> = vec![(1.0, uniform)];

    for step in 1..=MAX_STEPS {
        let mut merged: BTreeMap<Vec<u64>, (f64, Vec<f64>)> = BTreeMap::new();
        for (state_weight, belief) in &frontier {
            for joint_obs in 0..alpha_card {
                let mut unnorm = vec![0.0f64; p];
                let mut obs_prob = 0.0f64;
                for (phase, &prior) in belief.iter().enumerate() {
                    if prior == 0.0 {
                        continue;
                    }
                    let mut like = 1.0f64;
                    let mut tmp = joint_obs;
                    for (ch, &clean) in minimal_cycle[phase].iter().enumerate() {
                        let obs_sym = (tmp & (sym_card - 1)) as u64;
                        tmp >>= k_bits;
                        let eps = channel_eps[ch];
                        like *= if obs_sym == clean {
                            1.0 - eps
                        } else {
                            eps / ((sym_card - 1) as f64)
                        };
                    }
                    let next_phase = (phase + 1) % p;
                    let contrib = prior * like;
                    unnorm[next_phase] += contrib;
                    obs_prob += contrib;
                }
                if obs_prob <= 0.0 {
                    continue;
                }
                for v in &mut unnorm {
                    *v /= obs_prob;
                }
                let key: Vec<u64> = unnorm
                    .iter()
                    .map(|&x| (x * (QUANT_GRID as f64)).round() as u64)
                    .collect();
                let branch_weight = state_weight * obs_prob;
                merged
                    .entry(key)
                    .and_modify(|(w_acc, b_acc)| {
                        let new_w = *w_acc + branch_weight;
                        for (acc, &val) in b_acc.iter_mut().zip(&unnorm) {
                            *acc = (*acc * *w_acc + val * branch_weight) / new_w;
                        }
                        *w_acc = new_w;
                    })
                    .or_insert((branch_weight, unnorm));
            }
        }

        if merged.len() > MAX_MIXED_STATES {
            return (
                None,
                MspSyncStatus::ExceededMixedStateCap {
                    max_mixed_states: MAX_MIXED_STATES,
                },
            );
        }
        frontier = merged.into_values().collect();

        let expected_entropy: f64 = frontier
            .iter()
            .map(|(w, b)| {
                let h: f64 = b.iter().filter(|&&q| q > 0.0).map(|&q| -q * q.log2()).sum();
                w * h
            })
            .sum();

        if expected_entropy < 0.01 {
            return (
                Some(step),
                MspSyncStatus::BoundedQuantizedEstimate {
                    quant_grid: QUANT_GRID,
                    max_steps: MAX_STEPS,
                    max_mixed_states: MAX_MIXED_STATES,
                },
            );
        }
    }

    (
        None,
        MspSyncStatus::ExceededStepCap {
            max_steps: MAX_STEPS,
        },
    )
}
