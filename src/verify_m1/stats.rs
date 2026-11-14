//! Reusable statistical evaluation helpers for multi-bank milestone gates (§2.8).
//!
//! Provides exact one-sided Clopper–Pearson binomial confidence bounds and Pearson's
//! $\chi^2$ homogeneity tests for cross-bank distributional stability.

use rand_chacha::ChaCha8Rng;
use rand_core::SeedableRng;
use serde::{Deserialize, Serialize};

use crate::env::noise::uniform_u64_below;

/// Minimum expected cell count per bank required for Pearson's asymptotic $\chi^2$
/// approximation (Cochran's criterion); below this, exact conditional permutation is used.
const COCHRAN_MIN_EXPECTED_CELL_COUNT: f64 = 5.0;

/// Replicates for the deterministic conditional permutation homogeneity test when Cochran's rule
/// does not hold (`min_expected < 5.0`).
const CONDITIONAL_PERMUTATION_REPLICATES: usize = 100_000;

/// Natural logarithm of the Gamma function $\ln \Gamma(z)$ via Lanczos approximation.
pub fn ln_gamma(z: f64) -> f64 {
    const P: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_836_524,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if z < 0.5 {
        std::f64::consts::PI.ln() - (std::f64::consts::PI * z).sin().ln() - ln_gamma(1.0 - z)
    } else {
        let zm1 = z - 1.0;
        let mut x = P[0];
        for (i, &val) in P.iter().skip(1).enumerate() {
            x += val / (zm1 + (i as f64) + 1.0);
        }
        let t = zm1 + 7.5;
        0.5 * (2.0 * std::f64::consts::PI).ln() + (zm1 + 0.5) * t.ln() - t + x.ln()
    }
}

/// Continued fraction for regularized incomplete beta function $I_x(a, b)$.
fn betacf(a: f64, b: f64, x: f64) -> f64 {
    const MAXIT: usize = 100;
    const EPS: f64 = 3.0e-12;
    const FPMIN: f64 = 1.0e-30;

    let qab = a + b;
    let qap = a + 1.0;
    let qam = a - 1.0;
    let mut c = 1.0;
    let mut d = 1.0 - qab * x / qap;
    if d.abs() < FPMIN {
        d = FPMIN;
    }
    d = 1.0 / d;
    let mut h = d;

    for m in 1..=MAXIT {
        let m_f = m as f64;
        let m2 = 2.0 * m_f;
        // Even step
        let aa = m_f * (b - m_f) * x / ((qam + m2) * (a + m2));
        d = 1.0 + aa * d;
        if d.abs() < FPMIN {
            d = FPMIN;
        }
        c = 1.0 + aa / c;
        if c.abs() < FPMIN {
            c = FPMIN;
        }
        d = 1.0 / d;
        h *= d * c;

        // Odd step
        let aa = -(a + m_f) * (qab + m_f) * x / ((a + m2) * (qap + m2));
        d = 1.0 + aa * d;
        if d.abs() < FPMIN {
            d = FPMIN;
        }
        c = 1.0 + aa / c;
        if c.abs() < FPMIN {
            c = FPMIN;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < EPS {
            break;
        }
    }
    h
}

/// Regularized incomplete beta function $I_x(a, b)$.
pub fn inc_beta(a: f64, b: f64, x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let bt = (ln_gamma(a + b) - ln_gamma(a) - ln_gamma(b) + a * x.ln() + b * (1.0 - x).ln()).exp();
    if x < (a + 1.0) / (a + b + 2.0) {
        bt * betacf(a, b, x) / a
    } else {
        1.0 - bt * betacf(b, a, 1.0 - x) / b
    }
}

/// Exact one-sided Clopper–Pearson lower confidence bound for binomial successes.
///
/// Returns $p_L$ such that $P(\text{Bin}(n, p_L) \ge k) = 1 - \text{confidence}$.
#[must_use]
pub fn clopper_pearson_lower_bound(k: usize, n: usize, confidence: f64) -> f64 {
    if n == 0 || k == 0 {
        return 0.0;
    }
    let alpha = (1.0 - confidence).clamp(0.0, 1.0);
    if k >= n {
        return alpha.powf(1.0 / (n as f64));
    }
    let a = k as f64;
    let b = (n - k + 1) as f64;
    let mut low = 0.0;
    let mut high = 1.0;
    for _ in 0..64 {
        let mid = 0.5 * (low + high);
        if inc_beta(a, b, mid) < alpha {
            low = mid;
        } else {
            high = mid;
        }
    }
    0.5 * (low + high)
}

/// Series expansion for regularized lower incomplete gamma $P(a, x) = \gamma(a, x) / \Gamma(a)$.
fn gamma_ser(a: f64, x: f64) -> f64 {
    const MAXIT: usize = 100;
    const EPS: f64 = 3.0e-12;

    let gln = ln_gamma(a);
    let mut sum = 1.0 / a;
    let mut ap = a;
    let mut del = sum;
    for _ in 1..=MAXIT {
        ap += 1.0;
        del *= x / ap;
        sum += del;
        if del.abs() < sum.abs() * EPS {
            break;
        }
    }
    (-x + a * x.ln() - gln).exp() * sum
}

/// Continued fraction for regularized upper incomplete gamma $Q(a, x) = \Gamma(a, x) / \Gamma(a)$.
fn gamma_cf(a: f64, x: f64) -> f64 {
    const MAXIT: usize = 100;
    const EPS: f64 = 3.0e-12;
    const FPMIN: f64 = 1.0e-30;

    let mut b = x + 1.0 - a;
    let mut c = 1.0 / FPMIN;
    let mut d = 1.0 / b;
    let mut h = d;
    for i in 1..=MAXIT {
        let an = -(i as f64) * ((i as f64) - a);
        b += 2.0;
        d = an * d + b;
        if d.abs() < FPMIN {
            d = FPMIN;
        }
        c = b + an / c;
        if c.abs() < FPMIN {
            c = FPMIN;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < EPS {
            break;
        }
    }
    let gln = ln_gamma(a);
    (-x + a * x.ln() - gln).exp() * h
}

/// Computes the $p$-value for a Pearson's $\chi^2$ statistic with `df` degrees of freedom.
#[must_use]
pub fn chi_squared_p_value(df: usize, chi2: f64) -> f64 {
    if df == 0 || chi2 <= 0.0 {
        return 1.0;
    }
    let a = 0.5 * (df as f64);
    let x = 0.5 * chi2;
    if x < a + 1.0 {
        (1.0 - gamma_ser(a, x)).clamp(0.0, 1.0)
    } else {
        gamma_cf(a, x).clamp(0.0, 1.0)
    }
}

/// Counts of successes and trials for one bank of seeds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BankSample {
    /// Number of successful runs in this bank.
    pub successes: usize,
    /// Total number of runs evaluated in this bank.
    pub trials: usize,
}

impl BankSample {
    /// Creates a new bank sample count.
    #[must_use]
    pub const fn new(successes: usize, trials: usize) -> Self {
        Self { successes, trials }
    }

    /// Empirical success rate for this bank.
    #[must_use]
    pub fn rate(self) -> f64 {
        if self.trials == 0 {
            0.0
        } else {
            (self.successes as f64) / (self.trials as f64)
        }
    }
}

/// Result of pooled Clopper–Pearson confidence bound evaluation (§2.8).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PooledSuccessResult {
    /// Total trials pooled across all banks ($N$).
    pub total_trials: usize,
    /// Total successes pooled across all banks ($K$).
    pub total_successes: usize,
    /// Empirical pooled success rate $\bar{p} = K / N$.
    pub pooled_rate: f64,
    /// One-sided 99% Clopper–Pearson lower bound.
    pub lower_bound_99: f64,
    /// Target success rate required by the gate ($P_{\text{target}}$).
    pub target_rate: f64,
    /// Whether `lower_bound_99 >= target_rate`.
    pub passes_pooled: bool,
}

/// Summary of bank success rate distribution and homogeneity test (§2.8).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BankDistributionSummary {
    /// Number of banks evaluated ($B$).
    pub bank_count: usize,
    /// Unweighted mean of bank success rates.
    pub mean: f64,
    /// Sample standard deviation across bank success rates.
    pub std_dev: f64,
    /// 5th percentile of bank success rates.
    pub percentile_5th: f64,
    /// Pearson's $\chi^2$ statistic across banks.
    pub chi_squared: f64,
    /// Degrees of freedom ($B - 1$).
    pub df: usize,
    /// $p$-value for equal success rates across banks.
    pub p_value: f64,
    /// Whether homogeneity holds at the 1% significance level ($p \ge 0.01$).
    pub passes_homogeneity: bool,
}

/// Outcome of the multi-bank statistical acceptance gate (§2.8).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MultiBankGateResult {
    /// Pooled confidence bound evaluation.
    pub pooled: PooledSuccessResult,
    /// Distributional stability and homogeneity evaluation.
    pub distribution: BankDistributionSummary,
    /// Overall pass: requires both pooled confidence and homogeneity.
    pub passed: bool,
}

/// Evaluates multi-bank results against a target success rate (§2.8).
///
/// 1. Computes the one-sided 99% Clopper–Pearson lower bound on pooled success rate.
/// 2. Performs a $\chi^2$ homogeneity test across banks at $\alpha = 0.01$.
/// 3. Computes the bank mean, standard deviation, and 5th percentile.
/// 4. Rejects if either the pooled lower bound falls below `target_rate` or failures cluster
///    significantly across banks (rejecting homogeneity).
#[must_use]
pub fn evaluate_multi_bank_gate(banks: &[BankSample], target_rate: f64) -> MultiBankGateResult {
    let bank_count = banks.len();
    let total_trials: usize = banks.iter().map(|b| b.trials).sum();
    let total_successes: usize = banks.iter().map(|b| b.successes).sum();

    let pooled_rate = if total_trials == 0 {
        0.0
    } else {
        (total_successes as f64) / (total_trials as f64)
    };

    let lower_bound_99 = clopper_pearson_lower_bound(total_successes, total_trials, 0.99);
    let passes_pooled = lower_bound_99 >= target_rate;

    let pooled = PooledSuccessResult {
        total_trials,
        total_successes,
        pooled_rate,
        lower_bound_99,
        target_rate,
        passes_pooled,
    };

    if bank_count <= 1 {
        let rate = if bank_count == 1 {
            banks[0].rate()
        } else {
            0.0
        };
        let distribution = BankDistributionSummary {
            bank_count,
            mean: rate,
            std_dev: 0.0,
            percentile_5th: rate,
            chi_squared: 0.0,
            df: 0,
            p_value: 1.0,
            passes_homogeneity: true,
        };
        return MultiBankGateResult {
            pooled,
            distribution,
            passed: passes_pooled,
        };
    }

    let mut rates: Vec<f64> = banks.iter().map(|b| b.rate()).collect();
    rates.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let mean: f64 = rates.iter().sum::<f64>() / (bank_count as f64);
    let variance: f64 =
        rates.iter().map(|&r| (r - mean) * (r - mean)).sum::<f64>() / ((bank_count - 1) as f64);
    let std_dev = variance.sqrt();

    // 5th percentile via linear rank interpolation
    let p05_rank = 0.05 * ((bank_count - 1) as f64);
    let p05_idx = p05_rank.floor() as usize;
    let p05_frac = p05_rank - (p05_idx as f64);
    let percentile_5th = if p05_idx + 1 < bank_count {
        (1.0 - p05_frac) * rates[p05_idx] + p05_frac * rates[p05_idx + 1]
    } else {
        rates[p05_idx]
    };

    // Chi-squared homogeneity test (with exact conditional permutation fallback when Cochran's
    // cell-expectation criterion `E >= 5` fails, e.g. rare failures across 20-seed banks).
    let df = bank_count - 1;
    let (chi_squared, p_value, passes_homogeneity) = if pooled_rate <= 0.0 || pooled_rate >= 1.0 {
        // All 0% or all 100%: perfect homogeneity
        (0.0, 1.0, true)
    } else {
        let var_denom = pooled_rate * (1.0 - pooled_rate);
        let chi2: f64 = banks
            .iter()
            .filter(|b| b.trials > 0)
            .map(|b| {
                let n = b.trials as f64;
                let k = b.successes as f64;
                let expected_k = n * pooled_rate;
                let diff = k - expected_k;
                (diff * diff) / (n * var_denom)
            })
            .sum();
        let min_expected = banks
            .iter()
            .filter(|b| b.trials > 0)
            .map(|b| {
                let n = b.trials as f64;
                (n * pooled_rate).min(n * (1.0 - pooled_rate))
            })
            .fold(f64::INFINITY, f64::min);
        let p = if min_expected >= COCHRAN_MIN_EXPECTED_CELL_COUNT {
            chi_squared_p_value(df, chi2)
        } else {
            conditional_permutation_chi2_p_value(
                banks,
                total_trials,
                total_successes,
                pooled_rate,
                var_denom,
                chi2,
            )
        };
        let passes = p >= 0.01;
        (chi2, p, passes)
    };

    let distribution = BankDistributionSummary {
        bank_count,
        mean,
        std_dev,
        percentile_5th,
        chi_squared,
        df,
        p_value,
        passes_homogeneity,
    };

    let passed = passes_pooled && passes_homogeneity;
    MultiBankGateResult {
        pooled,
        distribution,
        passed,
    }
}

/// Evaluates the exact conditional permutation upper-tail $p$-value for Pearson's $\chi^2$
/// fixing bank trial counts `[b.trials]` and total successes `total_successes`.
fn conditional_permutation_chi2_p_value(
    banks: &[BankSample],
    total_trials: usize,
    total_successes: usize,
    pooled_rate: f64,
    var_denom: f64,
    observed_chi2: f64,
) -> f64 {
    let total_failures = total_trials.saturating_sub(total_successes);
    let count_failures = total_failures <= total_successes;
    let minority_count = if count_failures {
        total_failures
    } else {
        total_successes
    };
    if total_trials == 0 || minority_count == 0 {
        return 1.0;
    }

    let mut bank_n = Vec::with_capacity(banks.len());
    let mut bank_expected_k = Vec::with_capacity(banks.len());
    let mut bank_denom = Vec::with_capacity(banks.len());
    let mut slot_bank = Vec::with_capacity(total_trials);

    for b in banks {
        if b.trials == 0 {
            continue;
        }
        let active_idx = bank_n.len();
        let n_f = b.trials as f64;
        bank_n.push(n_f);
        bank_expected_k.push(n_f * pooled_rate);
        bank_denom.push(n_f * var_denom);
        slot_bank.resize(slot_bank.len() + b.trials, active_idx);
    }

    let num_active = bank_n.len();
    if num_active <= 1 {
        return 1.0;
    }

    let mut rng = ChaCha8Rng::seed_from_u64(0);
    let mut minority_counts = vec![0usize; num_active];
    let mut ge_count = 0usize;

    for _ in 0..CONDITIONAL_PERMUTATION_REPLICATES {
        minority_counts.fill(0);
        // Partial Fisher-Yates draws an exact uniform subset of size `minority_count`
        // without replacement from the current multiset permutation in `slot_bank`.
        for i in 0..minority_count {
            let j = i + (uniform_u64_below(&mut rng, (total_trials - i) as u64) as usize);
            slot_bank.swap(i, j);
            minority_counts[slot_bank[i]] += 1;
        }

        let mut perm_chi2 = 0.0f64;
        for b_idx in 0..num_active {
            let m = minority_counts[b_idx] as f64;
            let k = if count_failures { bank_n[b_idx] - m } else { m };
            let diff = k - bank_expected_k[b_idx];
            perm_chi2 += (diff * diff) / bank_denom[b_idx];
        }

        if perm_chi2 + 1e-12 >= observed_chi2 {
            ge_count += 1;
        }
    }

    (ge_count as f64) / (CONDITIONAL_PERMUTATION_REPLICATES as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clopper_pearson_exact_boundary_cases() {
        assert_eq!(clopper_pearson_lower_bound(0, 100, 0.99), 0.0);
        let all_pass = clopper_pearson_lower_bound(20, 20, 0.99);
        let expected = 0.01f64.powf(1.0 / 20.0);
        assert!((all_pass - expected).abs() < 1e-9);
    }

    #[test]
    fn clopper_pearson_known_intermediate_quantiles() {
        // For n=100, k=95, 99% one-sided lower bound should be ≈ 0.8697
        let p_l = clopper_pearson_lower_bound(95, 100, 0.99);
        assert!((p_l - 0.8697).abs() < 0.005, "got {p_l}");

        // For n=1000, k=950, 99% one-sided lower bound should be ≈ 0.9318
        let p_l_1000 = clopper_pearson_lower_bound(950, 1000, 0.99);
        assert!((p_l_1000 - 0.9318).abs() < 0.005, "got {p_l_1000}");
    }

    #[test]
    fn chi_squared_known_critical_values() {
        // df = 1: critical value for p = 0.05 is 3.841
        let p1 = chi_squared_p_value(1, 3.84146);
        assert!((p1 - 0.05).abs() < 0.001, "got {p1}");

        // df = 4: critical value for p = 0.05 is 9.488
        let p4 = chi_squared_p_value(4, 9.48773);
        assert!((p4 - 0.05).abs() < 0.001, "got {p4}");

        // df = 9: critical value for p = 0.01 is 21.666
        let p9 = chi_squared_p_value(9, 21.666);
        assert!((p9 - 0.01).abs() < 0.001, "got {p9}");
    }

    #[test]
    fn multi_bank_evaluation_homogeneous_vs_clustered() {
        // 10 banks of 20 seeds, 19/20 in each bank (total 190/200)
        let homogeneous: Vec<BankSample> = (0..10).map(|_| BankSample::new(19, 20)).collect();
        let res_hom = evaluate_multi_bank_gate(&homogeneous, 0.85);
        assert!(res_hom.passed);
        assert!(res_hom.pooled.passes_pooled);
        assert!(res_hom.distribution.passes_homogeneity);
        assert!(res_hom.distribution.p_value > 0.99);

        // Clustered failure: 9 banks at 20/20, 1 bank at 10/20 (total 190/200)
        // Pooled count is identical (190/200), but distribution is severely clustered!
        let mut clustered: Vec<BankSample> = (0..9).map(|_| BankSample::new(20, 20)).collect();
        clustered.push(BankSample::new(10, 20));
        let res_clustered = evaluate_multi_bank_gate(&clustered, 0.85);
        assert!(res_clustered.pooled.passes_pooled);
        assert!(
            !res_clustered.distribution.passes_homogeneity,
            "clustered failures should be rejected by homogeneity test"
        );
        assert!(
            !res_clustered.passed,
            "overall gate must fail when homogeneity is rejected"
        );
    }

    #[test]
    fn cochran_rare_failure_permutation_fallback_handles_sparse_and_clustered_banks() {
        // Reproduces 983/1000 across 50 banks of 20 seeds (38x20, 9x19, 1x18, 2x17):
        // E_fail = 0.34 < 5.0 triggers conditional permutation (exact tail p >= 0.01).
        let mut banks = Vec::with_capacity(50);
        banks.extend((0..38).map(|_| BankSample::new(20, 20)));
        banks.extend((0..9).map(|_| BankSample::new(19, 20)));
        banks.push(BankSample::new(18, 20));
        banks.extend((0..2).map(|_| BankSample::new(17, 20)));
        let res_rare = evaluate_multi_bank_gate(&banks, 0.85);
        assert!(res_rare.pooled.passes_pooled);
        assert!(
            res_rare.distribution.passes_homogeneity,
            "expected conditional permutation p >= 0.01, got chi2={} p={}",
            res_rare.distribution.chi_squared, res_rare.distribution.p_value
        );
        assert!(res_rare.passed);

        // Truly clustered rare failures: 48x(20/20) + 1x(19/20) + 1x(4/20) = 983/1000.
        let mut clustered_rare = Vec::with_capacity(50);
        clustered_rare.extend((0..48).map(|_| BankSample::new(20, 20)));
        clustered_rare.push(BankSample::new(19, 20));
        clustered_rare.push(BankSample::new(4, 20));
        let res_clustered_rare = evaluate_multi_bank_gate(&clustered_rare, 0.85);
        assert!(
            !res_clustered_rare.distribution.passes_homogeneity,
            "severely clustered rare failures must still fail homogeneity (p={})",
            res_clustered_rare.distribution.p_value
        );
        assert!(!res_clustered_rare.passed);
    }
}
