//! Charter §5 Environment: finite-state generators, declared noise, $\varepsilon$-machine solver, and Reeds–Sloane synthesis.

pub mod epsilon_machine;
pub mod generator;
pub mod noise;
pub mod reeds_sloane;
pub mod tiers;

pub use epsilon_machine::{
    EpsilonMachineSummary, MspSyncStatus, channel_substitution_entropy_bits,
    compound_reactive_entropy_bits, compound_reactive_error_rate,
    declared_noise_entropy_bits_per_symbol, per_channel_epsilon, realized_oracle_channel_step_bits,
    realized_oracle_step_bits, solve_epsilon_machine,
};
pub use generator::{
    DEFAULT_MAX_EXPORT_STATES, EpsilonMachineScope, FiniteStateGenerator, FiniteStateSpec,
    SpecExportError,
};
pub use noise::{NoiseError, NoiseModel, NoisyEnvironment};
pub use reeds_sloane::{ReedsSloaneSynthesizer, odd_inverse_mod_pow2};
pub use tiers::{
    CrossLinearOp, CrossNonlinearOp, GeneratorConfig, GeneratorError, LfsrStage,
    MultiChannelProductGenerator, TargetChannelGenerator, TierGenerator,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::substrate::Width;

    #[test]
    fn epsilon_machine_matches_closed_form_analytic_values() {
        // 1. Constant stream: C_mu = 0, h_mu = 0, sync = 0.
        let g_const = GeneratorConfig::T1_Constant { value: 1 }
            .build(Width::BITS_1)
            .unwrap();
        let em_const =
            solve_epsilon_machine(&g_const.to_finite_state_spec().unwrap(), &NoiseModel::None);
        assert_eq!(em_const.causal_state_count, 1);
        assert!(em_const.c_mu_bits.abs() < 1e-12);
        assert!(em_const.h_mu_bits_per_symbol.abs() < 1e-12);
        assert_eq!(em_const.exact_sync_order, Some(0));
        assert_eq!(em_const.min_delay_registers, 0);

        // 2. Motif [1, 1, 0] at k = 1: C_mu = log2(3), h_mu = 0, sync = 2, min_delay = 2.
        let g_motif = GeneratorConfig::T1_PeriodicMotif {
            motif: vec![1, 1, 0],
        }
        .build(Width::BITS_1)
        .unwrap();
        let em_motif =
            solve_epsilon_machine(&g_motif.to_finite_state_spec().unwrap(), &NoiseModel::None);
        assert_eq!(em_motif.causal_state_count, 3);
        assert!((em_motif.c_mu_bits - 3.0f64.log2()).abs() < 1e-12);
        assert_eq!(em_motif.exact_sync_order, Some(2));
        assert_eq!(em_motif.min_delay_registers, 2);

        // 3. Same period-3 motif at k = 2: 2^2 = 4 >= 3 states fit in 1 DELAY register!
        let g_motif_k2 = GeneratorConfig::T1_PeriodicMotif {
            motif: vec![0, 1, 3],
        }
        .build(Width::BITS_2)
        .unwrap();
        let em_motif_k2 = solve_epsilon_machine(
            &g_motif_k2.to_finite_state_spec().unwrap(),
            &NoiseModel::None,
        );
        assert_eq!(em_motif_k2.min_delay_registers, 1);
        assert_eq!(em_motif_k2.exact_sync_order, Some(1));

        // 4. Redundant motif [0, 1, 0, 1] collapses to p_min = 2 (C_mu = 1 bit)!
        let g_red = GeneratorConfig::T1_PeriodicMotif {
            motif: vec![0, 1, 0, 1],
        }
        .build(Width::BITS_1)
        .unwrap();
        let em_red =
            solve_epsilon_machine(&g_red.to_finite_state_spec().unwrap(), &NoiseModel::None);
        assert_eq!(em_red.causal_state_count, 2);
        assert!((em_red.c_mu_bits - 1.0).abs() < 1e-12);

        // 5. Noisy T1_Alternating at k = 2 with eps = 0.05:
        //    h_mu = H_2(0.05) + 0.05 * log2(3), and MSP converges in finite ticks.
        let g_alt = GeneratorConfig::T1_Alternating { a: 0, b: 3 }
            .build(Width::BITS_2)
            .unwrap();
        let noise = NoiseModel::IndependentSubstitution {
            epsilon: 0.05,
            channels: vec![0],
        };
        let em_noisy = solve_epsilon_machine(&g_alt.to_finite_state_spec().unwrap(), &noise);
        let expected_h = -0.95 * 0.95f64.log2() - 0.05 * 0.05f64.log2() + 0.05 * 3.0f64.log2();
        assert!((em_noisy.c_mu_bits - 1.0).abs() < 1e-12);
        assert!((em_noisy.h_mu_bits_per_symbol - expected_h).abs() < 1e-12);
        assert!(em_noisy.noisy_msp_approx_sync_ticks.unwrap() <= 10);
        assert!(matches!(
            em_noisy.msp_sync_status,
            MspSyncStatus::BoundedQuantizedEstimate { .. }
        ));
        assert_eq!(em_noisy.scope, EpsilonMachineScope::StationaryProcess);

        // 6. Curriculum explicitly marks its exported spec as CurriculumFinalStage.
        let curr = GeneratorConfig::T3c_LfsrCurriculum {
            stages: vec![
                LfsrStage {
                    taps: vec![1, 0],
                    init: vec![1, 0],
                },
                LfsrStage {
                    taps: vec![1, 1],
                    init: vec![0, 1],
                },
            ],
            ticks_per_stage: 25,
        }
        .build(Width::BITS_1)
        .unwrap();
        let em_curr =
            solve_epsilon_machine(&curr.to_finite_state_spec().unwrap(), &NoiseModel::None);
        assert_eq!(
            em_curr.scope,
            EpsilonMachineScope::CurriculumFinalStage {
                stage_index: 1,
                total_stages: 2,
                ticks_per_stage: 25,
            }
        );
    }

    #[test]
    fn t2_generators_and_compound_entropy_match_closed_form() {
        // Degree-5 (x^5 + x^2 + 1 -> period 31) x Degree-4 (x^4 + x + 1 -> period 15) => period 465.
        let deg5 = LfsrStage {
            taps: vec![0, 1, 0, 0, 1],
            init: vec![0, 0, 0, 0, 1],
        };
        let deg4 = LfsrStage {
            taps: vec![1, 0, 0, 1],
            init: vec![0, 0, 0, 1],
        };

        let mut g_lin = GeneratorConfig::T2_CrossLinear {
            driver_0: deg5.clone(),
            driver_1: deg4.clone(),
            lag_0: 1,
            lag_1: 2,
            op: CrossLinearOp::Add,
        }
        .build(Width::BITS_1)
        .unwrap();
        assert_eq!(g_lin.channels(), 3);

        let spec = g_lin.to_finite_state_spec().unwrap();
        let em = solve_epsilon_machine(&spec, &NoiseModel::None);
        assert_eq!(em.causal_state_count, 465);
        assert!((em.c_mu_bits - (465.0f64).log2()).abs() < 1e-12);
        assert!((em.c_mu_bits - 8.861_086_905_995_395).abs() < 1e-9);
        assert!(em.h_mu_bits_per_symbol.abs() < 1e-12);

        // Verify exact delayed relation B(t) = A0(t - 1) + A1(t - 2) mod 2.
        let mut a0_hist = Vec::new();
        let mut a1_hist = Vec::new();
        let mut frame = [0u64; 3];
        for t in 0..40 {
            g_lin.next_clean_frame(&mut frame);
            a0_hist.push(frame[0]);
            a1_hist.push(frame[1]);
            let prev0 = if t >= 1 { a0_hist[t - 1] } else { 0 };
            let prev1 = if t >= 2 { a1_hist[t - 2] } else { 0 };
            assert_eq!(frame[2], (prev0 + prev1) & 1);
        }

        // Delayed affine + TargetChannelGenerator marginal projection.
        let g_aff = GeneratorConfig::T2_DelayedAffine {
            driver: deg4.clone(),
            lag: 2,
            mul_a: 3,
            add_b: 1,
        }
        .build(Width::BITS_2)
        .unwrap();
        let mut proj = TargetChannelGenerator::new(g_aff);
        assert_eq!(proj.channels(), 1);
        let mut p_frame = [0u64; 1];
        proj.next_clean_frame(&mut p_frame);
        // At t = 0 (< lag), A0(0 - 2) = 0, so B(0) = 3 * 0 + 1 = 1 mod 4.
        assert_eq!(p_frame[0], 1);

        // MultiChannelProductGenerator composes independent generators.
        let g_c0 = GeneratorConfig::T1_Alternating { a: 0, b: 1 }
            .build(Width::BITS_1)
            .unwrap();
        let g_c1 = GeneratorConfig::T1_PeriodicMotif {
            motif: vec![0, 1, 1],
        }
        .build(Width::BITS_1)
        .unwrap();
        let prod = MultiChannelProductGenerator::new(vec![g_c0, g_c1]).unwrap();
        assert_eq!(prod.channels(), 2);
        let prod_em =
            solve_epsilon_machine(&prod.to_finite_state_spec().unwrap(), &NoiseModel::None);
        assert_eq!(prod_em.causal_state_count, 6);

        // Zero lag is rejected.
        assert_eq!(
            GeneratorConfig::T2_DelayedAffine {
                driver: deg4,
                lag: 0,
                mul_a: 1,
                add_b: 0,
            }
            .build(Width::BITS_1)
            .unwrap_err(),
            GeneratorError::InvalidCrossLag { lag: 0 }
        );

        // Compound reactive substitution error & entropy checks.
        assert_eq!(compound_reactive_error_rate(0.0, 2, 2), 0.0);
        assert_eq!(compound_reactive_entropy_bits(0.0, 2, 2), 0.0);
        // k = 1, m = 1, eps = 0.05 => 0.5 * (1 - 0.9^2) = 0.095.
        assert!((compound_reactive_error_rate(0.05, 1, 1) - 0.095).abs() < 1e-12);
        assert!(
            (compound_reactive_entropy_bits(0.05, 1, 1)
                - channel_substitution_entropy_bits(0.095, 1))
            .abs()
                < 1e-12
        );
    }
}
