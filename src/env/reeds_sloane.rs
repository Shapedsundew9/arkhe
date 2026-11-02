//! Reeds–Sloane shift-register synthesis over $\mathbb{Z}_{2^k}$ (Charter §1.4, §5.2, §8).
//!
//! Implements Reeds & Sloane (1985), *"Shift-register synthesis (modulo $m$)"*, *SIAM J. Comput.*
//! 14(3), 505–513, specialised to $m = 2^k$.
//!
//! - At $k = 1$, reduces identically to classical $GF(2)$ Berlekamp–Massey, recovering the minimal
//!   LFSR of length $L$ within $2L$ symbols.
//! - At $k \ge 2$, handles zero divisors via prime-power valuation $v_2(\theta)$ and odd-unit
//!   Newton–Hensel inversion across $k$ ideal tracks.

use crate::substrate::Width;

/// Multiplicative inverse of an odd integer `u` modulo $2^{64}$ via 6 Newton–Hensel steps,
/// masked to `width`.
#[must_use]
pub const fn odd_inverse_mod_pow2(u: u64, width: Width) -> u64 {
    debug_assert!(u & 1 == 1);
    let mut x = 1u64;
    // Quadratic convergence: 1 -> 2 -> 4 -> 8 -> 16 -> 32 -> 64 bits exact!
    let mut i = 0;
    while i < 6 {
        x = x.wrapping_mul(2u64.wrapping_sub(u.wrapping_mul(x)));
        i += 1;
    }
    x & width.mask()
}

/// Decomposes a non-zero $d \in \mathbb{Z}_{2^k}$ into `(odd_unit, valuation_g)` such that
/// $d \equiv u \cdot 2^g \pmod{2^k}$ with $u$ odd and $0 \le g < k$.
#[inline]
const fn factor_unit_and_valuation(d: u64, width: Width) -> (u64, usize) {
    debug_assert!(d != 0);
    let g = d.trailing_zeros() as usize;
    let u = (d >> g) & width.mask();
    (u, g)
}

#[derive(Clone, Debug)]
struct TrackPoly {
    /// Connection polynomial $a(D) = \sum_{i=0}^L a_i D^i$ with $a_0 = 2^\eta \bmod 2^k$.
    a: Vec<u64>,
    /// Initial-condition numerator $b(D) = \sum_{i=0}^{L-1} b_i D^i$.
    b: Vec<u64>,
    /// Linear register length $L$.
    len: usize,
}

#[derive(Clone, Debug)]
struct OldRecord {
    a: Vec<u64>,
    b: Vec<u64>,
    len: usize,
    odd_unit: u64,
    step_r: usize,
}

impl OldRecord {
    /// Score $r - L$: larger means fewer added taps $n - r + L = n - (r - L)$ when used at step $n$.
    const fn score(&self) -> isize {
        (self.step_r as isize) - (self.len as isize)
    }
}

/// Incremental Reeds–Sloane / Berlekamp–Massey minimal linear recurrence synthesizer over $\mathbb{Z}_{2^k}$.
#[derive(Clone, Debug)]
pub struct ReedsSloaneSynthesizer {
    width: Width,
    history: Vec<u64>,
    tracks: Vec<TrackPoly>,
    old_by_valuation: Vec<Option<OldRecord>>,
}

impl ReedsSloaneSynthesizer {
    /// Creates an empty synthesizer for $\mathbb{Z}_{2^k}$.
    #[must_use]
    pub fn new(width: Width) -> Self {
        let k = usize::from(width.bits());
        let tracks = (0..k)
            .map(|eta| {
                let a0 = if eta == 64 {
                    0
                } else {
                    (1u64 << eta) & width.mask()
                };
                TrackPoly {
                    a: vec![a0],
                    b: Vec::new(),
                    len: 0,
                }
            })
            .collect();

        Self {
            width,
            history: Vec::new(),
            tracks,
            old_by_valuation: vec![None; k],
        }
    }

    /// Resets the synthesizer to zero observed symbols.
    pub fn reset(&mut self) {
        *self = Self::new(self.width);
    }

    /// Uniform bit-width $k$.
    #[must_use]
    pub const fn width(&self) -> Width {
        self.width
    }

    /// Number of symbols observed so far.
    #[must_use]
    pub fn observed_len(&self) -> usize {
        self.history.len()
    }

    /// Minimal monic linear register length $L$ explaining all symbols observed so far.
    #[must_use]
    pub fn minimal_length(&self) -> usize {
        self.tracks[0].len
    }

    /// Current minimal monic connection polynomial $[1, a_1, \dots, a_L]$ where
    /// $s_t + \sum_{i=1}^L a_i s_{t-i} \equiv 0 \pmod{2^k}$ for all $L \le t < n$.
    #[must_use]
    pub fn connection_poly(&self) -> &[u64] {
        &self.tracks[0].a
    }

    /// Feedback tap vector $[c_1, \dots, c_L]$ where $\hat{s}_t = \sum_{i=1}^L c_i s_{t-i} \bmod 2^k$
    /// ($c_i = -a_i \bmod 2^k$).
    #[must_use]
    pub fn feedback_taps(&self) -> Vec<u64> {
        let w = self.width;
        let len = self.tracks[0].len;
        (1..=len)
            .map(|i| {
                let ai = self.tracks[0].a.get(i).copied().unwrap_or(0);
                w.sub(0, ai)
            })
            .collect()
    }

    /// Predicts the next symbol $\hat{s}_n \in \mathbb{Z}_{2^k}$ from the current minimal monic register
    /// without mutating state.
    #[must_use]
    pub fn predict_next(&self) -> u64 {
        let n = self.history.len();
        let w = self.width;
        let poly = &self.tracks[0];
        // From a(D) S(D) - b(D) == 0 at degree n:
        // a_0 s_n + sum_{i=1..len} a_i s_{n - i} - b_n == 0 (mod 2^k), with a_0 == 1 and deg(b) < len.
        let mut sum = 0u64;
        let max_i = poly.a.len().min(n + 1);
        for i in 1..max_i {
            sum = w.add(sum, w.mul(poly.a[i], self.history[n - i]));
        }
        let b_n = poly.b.get(n).copied().unwrap_or(0);
        w.sub(b_n, sum)
    }

    /// Appends one observed symbol $s_n \in \mathbb{Z}_{2^k}$ and updates all $k$ ideal tracks.
    pub fn push_symbol(&mut self, raw_symbol: u64) {
        let w = self.width;
        let sym = w.normalize(raw_symbol);
        let n = self.history.len();
        self.history.push(sym);

        let k = self.tracks.len();
        let mut candidates_for_old: Vec<Option<OldRecord>> = vec![None; k];

        for eta in 0..k {
            let track = &self.tracks[eta];

            // Compute discrepancy theta = [D^n](a(D) S(D) - b(D)) mod 2^k.
            let mut conv = 0u64;
            let max_i = track.a.len().min(n + 1);
            for i in 0..max_i {
                conv = w.add(conv, w.mul(track.a[i], self.history[n - i]));
            }
            let b_n = track.b.get(n).copied().unwrap_or(0);
            let theta = w.sub(conv, b_n);
            if theta == 0 {
                continue;
            }

            let (u_eta, g) = factor_unit_and_valuation(theta, w);
            let pre_update = OldRecord {
                a: track.a.clone(),
                b: track.b.clone(),
                len: track.len,
                odd_unit: u_eta,
                step_r: n,
            };

            let updated = if let Some(old) = &self.old_by_valuation[g] {
                let shift = n - old.step_r;
                debug_assert!(shift >= 1);
                let inv_u_old = odd_inverse_mod_pow2(old.odd_unit, w);
                let factor = w.mul(u_eta, inv_u_old);
                let new_len = track.len.max(shift + old.len);

                let mut new_a = track.a.clone();
                if new_a.len() < shift + old.a.len() {
                    new_a.resize(shift + old.a.len(), 0);
                }
                for (i, &oa) in old.a.iter().enumerate() {
                    let sub_term = w.mul(factor, oa);
                    new_a[shift + i] = w.sub(new_a[shift + i], sub_term);
                }

                let mut new_b = track.b.clone();
                if new_b.len() < shift + old.b.len() {
                    new_b.resize(shift + old.b.len(), 0);
                }
                for (i, &ob) in old.b.iter().enumerate() {
                    let sub_term = w.mul(factor, ob);
                    new_b[shift + i] = w.sub(new_b[shift + i], sub_term);
                }

                TrackPoly {
                    a: new_a,
                    b: new_b,
                    len: new_len,
                }
            } else {
                // Virtual r = -1 fallback: add (0, theta * D^n), raising length to max(len, n + 1).
                let mut new_b = track.b.clone();
                if new_b.len() <= n {
                    new_b.resize(n + 1, 0);
                }
                new_b[n] = w.add(new_b[n], theta);
                TrackPoly {
                    a: track.a.clone(),
                    b: new_b,
                    len: track.len.max(n + 1),
                }
            };

            // Record pre_update if it strictly improves valuation slot `g` (equivalently, new_len > old track.len).
            let current_best_score = match (&candidates_for_old[g], &self.old_by_valuation[g]) {
                (Some(cand), _) => cand.score(),
                (None, Some(existing)) => existing.score(),
                (None, None) => -1,
            };
            if pre_update.score() > current_best_score {
                candidates_for_old[g] = Some(pre_update);
            }

            self.tracks[eta] = updated;
        }

        for (g, cand) in candidates_for_old.into_iter().enumerate() {
            if let Some(rec) = cand {
                self.old_by_valuation[g] = Some(rec);
            }
        }
    }

    /// Synthesizes the minimal linear recurrence over an entire slice in one call.
    #[must_use]
    pub fn from_slice(width: Width, symbols: &[u64]) -> Self {
        let mut synth = Self::new(width);
        for &s in symbols {
            synth.push_symbol(s);
        }
        synth
    }
}

#[cfg(test)]
mod tests {
    use rand_core::Rng;

    use super::*;
    use crate::rng::{SeedBank, StreamDomain};

    /// Brute-force oracle finding the true minimum $L$ for which a monic recurrence over $\mathbb{Z}_{2^k}$
    /// explains `seq`.
    fn brute_force_minimal_len_z4(seq: &[u64]) -> usize {
        let w = Width::BITS_2;
        for len in 0..=seq.len() {
            if len == 0 {
                if seq.iter().all(|&x| x == 0) {
                    return 0;
                }
                continue;
            }
            let num_combos = 1usize << (2 * len);
            for mask in 0..num_combos {
                let mut taps = vec![0u64; len];
                let mut tmp = mask;
                for slot in &mut taps {
                    *slot = (tmp & 3) as u64;
                    tmp >>= 2;
                }
                let ok = (len..seq.len()).all(|t| {
                    let mut pred = 0u64;
                    for i in 1..=len {
                        pred = w.add(pred, w.mul(taps[i - 1], seq[t - i]));
                    }
                    pred == seq[t]
                });
                if ok {
                    return len;
                }
            }
        }
        seq.len()
    }

    #[test]
    fn reeds_sloane_matches_brute_force_oracle_on_500_random_z4_streams() {
        let mut rng = SeedBank::new(2026).stream(StreamDomain::GeneratorInit);
        let w = Width::BITS_2;

        // Adversarial zero-divisor sequences first:
        for seq in [
            vec![2, 1, 2, 0],
            vec![2, 1, 0, 0],
            vec![2, 2, 2, 2],
            vec![1, 2, 0, 0],
            vec![0, 2, 1, 2, 3],
        ] {
            let rs = ReedsSloaneSynthesizer::from_slice(w, &seq);
            let exact = brute_force_minimal_len_z4(&seq);
            assert_eq!(
                rs.minimal_length(),
                exact,
                "mismatch on adversarial seq {seq:?}"
            );
            let taps = rs.feedback_taps();
            for t in rs.minimal_length()..seq.len() {
                let mut pred = 0u64;
                for i in 1..=taps.len() {
                    pred = w.add(pred, w.mul(taps[i - 1], seq[t - i]));
                }
                assert_eq!(pred, seq[t]);
            }
        }

        // 500 random Z_4 sequences of length 6:
        for trial in 0..500 {
            let seq: Vec<u64> = (0..6).map(|_| rng.next_u64() & 3).collect();
            let rs = ReedsSloaneSynthesizer::from_slice(w, &seq);
            let exact = brute_force_minimal_len_z4(&seq);
            assert_eq!(
                rs.minimal_length(),
                exact,
                "trial {trial} mismatch on seq {seq:?}"
            );
        }
    }

    #[test]
    fn k1_recovers_maximal_length_lfsr_in_2l_symbols() {
        // Primitive polynomial x^5 + x^2 + 1 over GF(2) (period 31, L = 5):
        // s(t) = s(t - 2) + s(t - 5) mod 2 => taps = [0, 1, 0, 0, 1].
        let w = Width::BITS_1;
        let taps = [0u64, 1, 0, 0, 1];
        let mut seq = vec![1u64, 0, 0, 0, 0];
        for t in 5..40 {
            let next = (seq[t - 2] ^ seq[t - 5]) & 1;
            seq.push(next);
        }

        let mut synth = ReedsSloaneSynthesizer::new(w);
        for (t, &sym) in seq.iter().enumerate() {
            if t >= 10 {
                // 2L = 10 symbols suffice for exact prediction on all subsequent ticks!
                assert_eq!(synth.predict_next(), sym, "failed at tick {t}");
            }
            synth.push_symbol(sym);
        }
        assert_eq!(synth.minimal_length(), 5);
        assert_eq!(synth.feedback_taps(), taps);
    }
}
