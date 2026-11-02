//! Charter §5 Environment: finite-state generators, declared noise, $\varepsilon$-machine solver, and Reeds–Sloane synthesis.

pub mod epsilon_machine;
pub mod generator;
pub mod noise;
pub mod reeds_sloane;
pub mod tiers;

pub use epsilon_machine::{
    EpsilonMachineSummary, MspSyncStatus, channel_substitution_entropy_bits,
    declared_noise_entropy_bits_per_symbol, realized_oracle_step_bits, solve_epsilon_machine,
};
pub use generator::{
    DEFAULT_MAX_EXPORT_STATES, EpsilonMachineScope, FiniteStateGenerator, FiniteStateSpec,
    SpecExportError,
};
pub use noise::{NoiseError, NoiseModel, NoisyEnvironment};
pub use reeds_sloane::{ReedsSloaneSynthesizer, odd_inverse_mod_pow2};
pub use tiers::{GeneratorConfig, GeneratorError, LfsrStage, TierGenerator};

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
}
