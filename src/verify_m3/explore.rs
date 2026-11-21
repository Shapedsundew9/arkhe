//! Milestone 3 (`H4`) exploratory and paired-ablation harness (`arkhe explore-m3`).
//!
//! Per Rule 8 (`docs/instructions/repo-common.md`), `explore-m3` is strictly exploratory:
//! it measures and reports JSON manifests and empirical distributions without pass/fail gating,
//! and unconditionally rejects any overlap with pre-registered milestone holdout seed banks.

use std::collections::{BTreeSet, VecDeque};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::{Deserialize, Serialize};

use crate::baselines::{
    DEFAULT_MAX_CERTIFY_JOINT_STEPS, PredictorBaseline, TickBudget,
    exact_synthesize_certified_circuit,
};
use crate::config::{BuildProvenance, CliffCountersSummary};
use crate::env::{
    CrossLinearOp, CrossNonlinearOp, EpsilonMachineScope, FiniteStateGenerator, GeneratorConfig,
    LfsrStage, MultiViewDriver, MultiViewTarget, MultiViewTransform, NoiseModel, NoisyEnvironment,
    declared_noise_entropy_bits_per_symbol,
};
use crate::metrics::mdl::{
    EpistemicComparatorTrace, EpistemicTickPhase, epistemic_null_calibrated_mdl_delta_with_tax,
};
use crate::metrics::{circuit_code_bits_with_tax, facts_code_bits};
use crate::rng::SeedBank;
use crate::structural::graph_edit::source_has_cycle;
use crate::structural::growth::ExcisionMode;
use crate::structural::learner::InjectionMode;
use crate::structural::surgery::primary_cone_nodes;
use crate::structural::{
    ArkheLearner, EconomyConfig, EconomyState, EditableCircuit, GrowthConfig, ReserveCap,
    build_autonomous_ring, cone_channel_mask, materialize_const_loop, observation_horizon,
    splice_comparator_onto_port,
};
use crate::substrate::{
    Circuit, NodeId, Primitive, ScalarState, SignalSink, SignalSource, Width, WireDelayMode,
};
use crate::verify_m3::detours::{DetourClass, build_detour_edit};

/// Pre-registered M1 holdout range start seed (unconditionally rejected).
pub const M1_HOLDOUT_SEED_START: u64 = 10_000;
/// Pre-registered M1 holdout range end seed (inclusive).
pub const M1_HOLDOUT_SEED_END: u64 = 10_019;
/// Pre-registered M2a holdout range start seed (unconditionally rejected).
pub const M2A_HOLDOUT_SEED_START: u64 = 30_000;
/// Pre-registered M2a holdout range end seed (inclusive).
pub const M2A_HOLDOUT_SEED_END: u64 = 30_019;
/// Pre-registered M2b holdout range start seed (unconditionally rejected).
pub const M2B_HOLDOUT_SEED_START: u64 = 40_000;
/// Pre-registered M2b holdout range end seed (inclusive).
pub const M2B_HOLDOUT_SEED_END: u64 = 40_019;
/// Pre-registered M3 holdout range start seed (unconditionally rejected in `explore-m3`).
pub const M3_HOLDOUT_SEED_START: u64 = 50_000;
/// Pre-registered M3 holdout range end seed (inclusive).
pub const M3_HOLDOUT_SEED_END: u64 = 50_019;
/// Pre-registered M4a reserved holdout range start seed (unconditionally rejected).
pub const M4A_HOLDOUT_SEED_START: u64 = 60_000;
/// Pre-registered M4a reserved holdout range end seed (inclusive).
pub const M4A_HOLDOUT_SEED_END: u64 = 60_019;

const MAX_SETTLE_TICKS: u64 = 2_000;
const INJECTION_TICK_BUDGET: u64 = 200_000;
const SPONTANEOUS_T2000: u64 = 2_000;
const SPONTANEOUS_T8000: u64 = 8_000;
const SPONTANEOUS_BUDGET_T8000: u64 = 800_000;
const SPONTANEOUS_WARMUP: u64 = 500;
const CONSERVATION_TOL: f64 = 1e-11;

/// Unconditionally rejects any seed range `[seed_offset, seed_offset + seeds)` overlapping
/// pre-registered milestone holdout banks (`M1`, `M2a`, `M2b`, `M3`, `M4a`).
///
/// # Errors
///
/// Returns a descriptive error if the requested range overlaps any holdout bank.
pub fn check_explore_m3_seed_range_holdout(seed_offset: u64, seeds: usize) -> Result<(), String> {
    let start = u128::from(seed_offset);
    let span = u128::try_from(seeds.max(1)).unwrap_or(u128::MAX);
    let end = start.saturating_add(span);
    let banks = [
        ("M1", M1_HOLDOUT_SEED_START, M1_HOLDOUT_SEED_END),
        ("M2a", M2A_HOLDOUT_SEED_START, M2A_HOLDOUT_SEED_END),
        ("M2b", M2B_HOLDOUT_SEED_START, M2B_HOLDOUT_SEED_END),
        ("M3", M3_HOLDOUT_SEED_START, M3_HOLDOUT_SEED_END),
        ("M4a", M4A_HOLDOUT_SEED_START, M4A_HOLDOUT_SEED_END),
    ];
    for (label, h_start, h_end) in banks {
        let hs = u128::from(h_start);
        let he = u128::from(h_end);
        if start <= he && end > hs {
            return Err(format!(
                "refusing explore-m3 seed range {start}..{end} which overlaps pre-registered {label} holdout range {h_start}..={h_end} (unconditionally forbidden)"
            ));
        }
    }
    Ok(())
}

/// Prune mechanism observed after detour injection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PruneMechanism {
    IdleD1b1,
    LocalExcision,
    StarvationBypass,
    CollapseRegrow,
    SurvivedBloated,
    NotSettled,
    SkippedNonAutonomousTph,
}

/// Counts of prune mechanisms across seeds in one exploratory cell.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PruneMechanismCounts {
    pub idle_d1b1: usize,
    pub local_excision: usize,
    pub starvation_bypass: usize,
    pub collapse_regrow: usize,
    pub survived_bloated: usize,
    pub not_settled: usize,
    pub skipped_non_autonomous_tph: usize,
}

impl PruneMechanismCounts {
    fn record(&mut self, mech: PruneMechanism) {
        match mech {
            PruneMechanism::IdleD1b1 => self.idle_d1b1 += 1,
            PruneMechanism::LocalExcision => self.local_excision += 1,
            PruneMechanism::StarvationBypass => self.starvation_bypass += 1,
            PruneMechanism::CollapseRegrow => self.collapse_regrow += 1,
            PruneMechanism::SurvivedBloated => self.survived_bloated += 1,
            PruneMechanism::NotSettled => self.not_settled += 1,
            PruneMechanism::SkippedNonAutonomousTph => self.skipped_non_autonomous_tph += 1,
        }
    }
}

/// Aggregated statistics for one injection experimental cell across `seeds`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InjectionCellSummary {
    pub sweep_tag: String,
    pub task: String,
    pub tier: String,
    pub width_bits: u8,
    pub wire_delay: WireDelayMode,
    pub epsilon: f64,
    pub split_code_tax: bool,
    pub excision_mode: ExcisionMode,
    pub injection_mode: InjectionMode,
    pub detour_class: DetourClass,
    pub total_runs: usize,
    pub settled_runs: usize,
    pub applicable_runs: usize,
    pub excised_within_w_obs: usize,
    pub excised_within_3w_obs: usize,
    pub excised_by_end: usize,
    pub collapsed_runs: usize,
    pub survived_bloated_runs: usize,
    pub mechanism_counts: PruneMechanismCounts,
    pub mean_t_inj: f64,
    pub min_excise_dt: Option<u64>,
    pub mean_excise_dt: Option<f64>,
    pub max_excise_dt: Option<u64>,
    pub mean_pre_inj_primary_nodes: f64,
    pub mean_final_primary_nodes: f64,
    pub max_final_primary_nodes: usize,
    pub mean_pre_inj_comp_nodes: f64,
    pub mean_final_comp_nodes: f64,
    pub min_final_active_comparators: usize,
    pub comparator_bearing_runs: usize,
    pub comparator_preserved_runs: usize,
    pub mean_pre_inj_primary_code_bits: f64,
    pub mean_final_primary_code_bits: f64,
    pub primary_code_bits_restored_runs: usize,
    pub mean_post_inj_clean_mismatches: f64,
    pub max_post_inj_clean_mismatches: u64,
    pub mean_post_inj_kt_excess_b_per_sym: f64,
    pub max_post_inj_kt_excess_b_per_sym: f64,
    pub mean_detour_peak_siphoned_energy: f64,
    pub max_detour_peak_siphoned_energy: f64,
    pub mean_detour_net_siphoned_energy: f64,
    pub max_exogenous_bits_err: f64,
    pub cliff_counters: CliffCountersSummary,
}

/// Paired `BaselineD1b1` vs `VerifiedLocalExcision` ablation row.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PairedExcisionAblationRow {
    pub task: String,
    pub width_bits: u8,
    pub injection_mode: InjectionMode,
    pub detour_class: DetourClass,
    pub paired_runs: usize,
    pub vle_excised_count: usize,
    pub base_excised_count: usize,
    pub base_collapsed_count: usize,
    pub base_survived_count: usize,
    pub vle_strictly_lower_mdl_count: usize,
    pub mean_mdl_delta_base_minus_vle: f64,
    pub vle_mean_excise_dt: Option<f64>,
    pub base_mean_excise_dt: Option<f64>,
}

/// Paired `split_code_tax = true` vs `false` ablation row on injection runs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PairedSplitTaxInjectionRow {
    pub task: String,
    pub width_bits: u8,
    pub injection_mode: InjectionMode,
    pub detour_class: DetourClass,
    pub paired_runs: usize,
    pub mean_l_pre_tax_true: f64,
    pub mean_l_pre_tax_false: f64,
    pub mean_delta_l_inj_tax_true: f64,
    pub mean_delta_l_inj_tax_false: f64,
    pub excised_tax_true: usize,
    pub excised_tax_false: usize,
    pub mean_excise_dt_tax_true: Option<f64>,
    pub mean_excise_dt_tax_false: Option<f64>,
}

/// Aggregated spontaneous long-horizon (`T in {2000, 8000}`) cell summary.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpontaneousCellSummary {
    pub task: String,
    pub tier: String,
    pub width_bits: u8,
    pub wire_delay: WireDelayMode,
    pub epsilon: f64,
    pub split_code_tax: bool,
    pub n_min: Option<usize>,
    pub exact_synthesis_status: String,
    pub total_runs: usize,
    pub locked_runs: usize,
    pub mean_t_lock: Option<f64>,
    pub mean_n_primary_at_lock: Option<f64>,
    pub mean_n_primary_2000: f64,
    pub max_n_primary_2000: usize,
    pub mean_n_primary_8000: f64,
    pub max_n_primary_8000: usize,
    pub mean_post_lock_slope: Option<f64>,
    pub max_post_lock_slope: Option<f64>,
    pub non_increasing_post_lock_runs: usize,
    pub mean_active_comp_2000: f64,
    pub max_active_comp_2000: usize,
    pub mean_active_comp_8000: f64,
    pub max_active_comp_8000: usize,
    pub mean_final_split_count: f64,
    pub mean_final_code_bits: f64,
    pub mean_n_primary_minus_n_min: Option<f64>,
    pub max_n_primary_minus_n_min: Option<isize>,
    pub mean_n_primary_over_n_min: Option<f64>,
    pub max_n_primary_over_n_min: Option<f64>,
    pub mean_post_warmup_kt_excess_8000: f64,
    pub clean_locked_8000_runs: usize,
    pub cliff_counters: CliffCountersSummary,
}

/// Paired `split_code_tax = true` vs `false` ablation row on spontaneous `T = 8000` runs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PairedSplitTaxSpontaneousRow {
    pub task: String,
    pub width_bits: u8,
    pub wire_delay: WireDelayMode,
    pub epsilon: f64,
    pub paired_runs: usize,
    pub mean_code_bits_tax_true: f64,
    pub mean_code_bits_tax_false: f64,
    pub mean_mdl_tax_delta: f64,
    pub mean_splits_tax_true: f64,
    pub mean_splits_tax_false: f64,
    pub mean_n_primary_tax_true: f64,
    pub mean_n_primary_tax_false: f64,
    pub clean_locked_tax_true: usize,
    pub clean_locked_tax_false: usize,
}

/// Summary of dedicated `ReserveCap::Uncapped` conservation probes across `T1`, `T2`, and `T2_MultiViewCoupled`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConservationProbeSummary {
    pub total_probes: usize,
    pub passed_probes: usize,
    pub max_epistemic_mdl_residual_b_per_sym: f64,
    pub max_pre_excision_delta_e_residual_b_per_sym: f64,
    pub max_exogenous_bits_err: f64,
}

/// Full JSON manifest returned by `arkhe explore-m3`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExploreM3Report {
    pub provenance: BuildProvenance,
    pub seed_offset: u64,
    pub seeds_tested: usize,
    pub injection_cells: Vec<InjectionCellSummary>,
    pub paired_excision_ablation: Vec<PairedExcisionAblationRow>,
    pub paired_split_tax_injection_ablation: Vec<PairedSplitTaxInjectionRow>,
    pub spontaneous_cells: Vec<SpontaneousCellSummary>,
    pub paired_split_tax_spontaneous_ablation: Vec<PairedSplitTaxSpontaneousRow>,
    pub conservation_summary: ConservationProbeSummary,
    pub total_cliff_counters: CliffCountersSummary,
}

#[derive(Clone)]
struct TaskSpec {
    label: &'static str,
    tier: &'static str,
    width: Width,
    target_port: u16,
    is_multiview: bool,
    is_autonomous_t1: bool,
    tph_ring_motif: Option<&'static [u64]>,
    comb_cfg: GeneratorConfig,
    tph_cfg: Option<GeneratorConfig>,
    noise_channels: Vec<u16>,
    split_tax_ablate_inj: bool,
}

fn d0_lfsr() -> LfsrStage {
    LfsrStage {
        taps: vec![0, 1, 0, 0, 1],
        init: vec![1, 1, 0, 1, 0],
    }
}

fn d1_lfsr() -> LfsrStage {
    LfsrStage {
        taps: vec![1, 0, 0, 1],
        init: vec![1, 0, 1, 1],
    }
}

fn d11_lfsr() -> LfsrStage {
    let mut taps = vec![0u64; 11];
    taps[8] = 1;
    taps[10] = 1;
    LfsrStage {
        taps,
        init: vec![1, 0, 1, 1, 0, 1, 0, 1, 1, 0, 1],
    }
}

fn injection_task_panel() -> Vec<TaskSpec> {
    let d0 = d0_lfsr();
    let d1 = d1_lfsr();
    let d11 = d11_lfsr();
    vec![
        TaskSpec {
            label: "T1_Alternating(k=1)",
            tier: "T1",
            width: Width::BITS_1,
            target_port: 0,
            is_multiview: false,
            is_autonomous_t1: true,
            tph_ring_motif: Some(&[0, 1]),
            comb_cfg: GeneratorConfig::T1_Alternating { a: 0, b: 1 },
            tph_cfg: Some(GeneratorConfig::T1_Alternating { a: 0, b: 1 }),
            noise_channels: vec![0],
            split_tax_ablate_inj: true,
        },
        TaskSpec {
            label: "T1_Alternating(k=2)",
            tier: "T1",
            width: Width::BITS_2,
            target_port: 0,
            is_multiview: false,
            is_autonomous_t1: true,
            tph_ring_motif: Some(&[0, 3]),
            comb_cfg: GeneratorConfig::T1_Alternating { a: 0, b: 3 },
            tph_cfg: Some(GeneratorConfig::T1_Alternating { a: 0, b: 3 }),
            noise_channels: vec![0],
            split_tax_ablate_inj: true,
        },
        TaskSpec {
            label: "T1_PeriodicMotif([0,1,1],k=1)",
            tier: "T1",
            width: Width::BITS_1,
            target_port: 0,
            is_multiview: false,
            is_autonomous_t1: true,
            tph_ring_motif: Some(&[0, 1, 1]),
            comb_cfg: GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 1, 1],
            },
            tph_cfg: Some(GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 1, 1],
            }),
            noise_channels: vec![0],
            split_tax_ablate_inj: true,
        },
        TaskSpec {
            label: "T1_PeriodicMotif([0,1,3],k=2)",
            tier: "T1",
            width: Width::BITS_2,
            target_port: 0,
            is_multiview: false,
            is_autonomous_t1: true,
            tph_ring_motif: Some(&[0, 1, 3]),
            comb_cfg: GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 1, 3],
            },
            tph_cfg: Some(GeneratorConfig::T1_PeriodicMotif {
                motif: vec![0, 1, 3],
            }),
            noise_channels: vec![0],
            split_tax_ablate_inj: true,
        },
        TaskSpec {
            label: "T3b_CounterModN(3,k=2)",
            tier: "T1",
            width: Width::BITS_2,
            target_port: 0,
            is_multiview: false,
            is_autonomous_t1: true,
            tph_ring_motif: Some(&[0, 1, 2]),
            comb_cfg: GeneratorConfig::T3b_CounterModN {
                modulus: 3,
                init: 0,
            },
            tph_cfg: Some(GeneratorConfig::T3b_CounterModN {
                modulus: 3,
                init: 0,
            }),
            noise_channels: vec![0],
            split_tax_ablate_inj: false,
        },
        TaskSpec {
            label: "T1_AffineLag1(1,1,0,k=2)",
            tier: "T1",
            width: Width::BITS_2,
            target_port: 0,
            is_multiview: false,
            is_autonomous_t1: true,
            tph_ring_motif: Some(&[0, 1, 2, 3]),
            comb_cfg: GeneratorConfig::T1_AffineLag1 {
                mul_a: 1,
                add_b: 1,
                init: 0,
            },
            tph_cfg: Some(GeneratorConfig::T1_AffineLag1 {
                mul_a: 1,
                add_b: 1,
                init: 0,
            }),
            noise_channels: vec![0],
            split_tax_ablate_inj: false,
        },
        TaskSpec {
            label: "T2_CrossSum(k=1)",
            tier: "T2",
            width: Width::BITS_1,
            target_port: 2,
            is_multiview: false,
            is_autonomous_t1: false,
            tph_ring_motif: None,
            comb_cfg: GeneratorConfig::T2_CrossLinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossLinearOp::Add,
            },
            tph_cfg: Some(GeneratorConfig::T2_CrossLinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 3,
                lag_1: 3,
                op: CrossLinearOp::Add,
            }),
            noise_channels: vec![0, 1, 2],
            split_tax_ablate_inj: false,
        },
        TaskSpec {
            label: "T2_CrossSum(k=2)",
            tier: "T2",
            width: Width::BITS_2,
            target_port: 2,
            is_multiview: false,
            is_autonomous_t1: false,
            tph_ring_motif: None,
            comb_cfg: GeneratorConfig::T2_CrossLinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossLinearOp::Add,
            },
            tph_cfg: Some(GeneratorConfig::T2_CrossLinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 3,
                lag_1: 3,
                op: CrossLinearOp::Add,
            }),
            noise_channels: vec![0, 1, 2],
            split_tax_ablate_inj: true,
        },
        TaskSpec {
            label: "T2_CrossMulEqz(k=2)",
            tier: "T2",
            width: Width::BITS_2,
            target_port: 2,
            is_multiview: false,
            is_autonomous_t1: false,
            tph_ring_motif: None,
            comb_cfg: GeneratorConfig::T2_CrossNonlinear {
                driver_0: d0.clone(),
                driver_1: d1.clone(),
                lag_0: 1,
                lag_1: 2,
                op: CrossNonlinearOp::MulEqz,
            },
            tph_cfg: Some(GeneratorConfig::T2_CrossNonlinear {
                driver_0: d0,
                driver_1: d1,
                lag_0: 3,
                lag_1: 4,
                op: CrossNonlinearOp::MulEqz,
            }),
            noise_channels: vec![2],
            split_tax_ablate_inj: true,
        },
        TaskSpec {
            label: "T2_MultiViewCoupled(k=1)",
            tier: "T2_MultiViewCoupled",
            width: Width::BITS_1,
            target_port: 2,
            is_multiview: true,
            is_autonomous_t1: false,
            tph_ring_motif: None,
            comb_cfg: GeneratorConfig::T2_MultiViewCoupled {
                driver: MultiViewDriver::Lfsr { stage: d11.clone() },
                view_0: MultiViewTransform {
                    delay: 1,
                    mul_a: 1,
                    add_b: 0,
                },
                view_1: MultiViewTransform {
                    delay: 0,
                    mul_a: 1,
                    add_b: 1,
                },
                target: MultiViewTarget::Affine {
                    lag: 2,
                    mul_a: 1,
                    add_b: 0,
                },
            },
            tph_cfg: Some(GeneratorConfig::T2_MultiViewCoupled {
                driver: MultiViewDriver::Lfsr { stage: d11.clone() },
                view_0: MultiViewTransform {
                    delay: 1,
                    mul_a: 1,
                    add_b: 0,
                },
                view_1: MultiViewTransform {
                    delay: 1,
                    mul_a: 1,
                    add_b: 1,
                },
                target: MultiViewTarget::Affine {
                    lag: 4,
                    mul_a: 1,
                    add_b: 0,
                },
            }),
            noise_channels: vec![0, 1, 2],
            split_tax_ablate_inj: true,
        },
        TaskSpec {
            label: "T2_MultiViewCoupled(k=2)",
            tier: "T2_MultiViewCoupled",
            width: Width::BITS_2,
            target_port: 2,
            is_multiview: true,
            is_autonomous_t1: false,
            tph_ring_motif: None,
            comb_cfg: GeneratorConfig::T2_MultiViewCoupled {
                driver: MultiViewDriver::Lfsr { stage: d11.clone() },
                view_0: MultiViewTransform {
                    delay: 1,
                    mul_a: 1,
                    add_b: 0,
                },
                view_1: MultiViewTransform {
                    delay: 0,
                    mul_a: 3,
                    add_b: 2,
                },
                target: MultiViewTarget::Affine {
                    lag: 2,
                    mul_a: 3,
                    add_b: 1,
                },
            },
            tph_cfg: Some(GeneratorConfig::T2_MultiViewCoupled {
                driver: MultiViewDriver::Lfsr { stage: d11 },
                view_0: MultiViewTransform {
                    delay: 1,
                    mul_a: 1,
                    add_b: 0,
                },
                view_1: MultiViewTransform {
                    delay: 1,
                    mul_a: 3,
                    add_b: 2,
                },
                target: MultiViewTarget::Affine {
                    lag: 4,
                    mul_a: 1,
                    add_b: 0,
                },
            }),
            noise_channels: vec![0, 1, 2],
            split_tax_ablate_inj: true,
        },
    ]
}

#[derive(Clone)]
struct SpontaneousSpec {
    label: &'static str,
    tier: &'static str,
    width: Width,
    target_port: u16,
    wire_delay: WireDelayMode,
    epsilon: f64,
    noise_channels: Vec<u16>,
    gen_cfg: GeneratorConfig,
}

fn spontaneous_panel() -> Vec<SpontaneousSpec> {
    let d0 = d0_lfsr();
    let d1 = d1_lfsr();
    let d11 = d11_lfsr();
    let mut specs = Vec::new();

    for width in [Width::BITS_1, Width::BITS_2] {
        specs.push(SpontaneousSpec {
            label: if width == Width::BITS_1 {
                "T1_Constant(0,k=1)"
            } else {
                "T1_Constant(0,k=2)"
            },
            tier: "T1",
            width,
            target_port: 0,
            wire_delay: WireDelayMode::Combinational,
            epsilon: 0.0,
            noise_channels: Vec::new(),
            gen_cfg: GeneratorConfig::T1_Constant { value: 0 },
        });
        specs.push(SpontaneousSpec {
            label: if width == Width::BITS_1 {
                "T1_NoisyConstant(0,k=1)"
            } else {
                "T1_NoisyConstant(0,k=2)"
            },
            tier: "T1",
            width,
            target_port: 0,
            wire_delay: WireDelayMode::Combinational,
            epsilon: 0.05,
            noise_channels: vec![0],
            gen_cfg: GeneratorConfig::T1_Constant { value: 0 },
        });
        specs.push(SpontaneousSpec {
            label: if width == Width::BITS_1 {
                "T1_NoisyConstant(max,k=1)"
            } else {
                "T1_NoisyConstant(max,k=2)"
            },
            tier: "T1",
            width,
            target_port: 0,
            wire_delay: WireDelayMode::Combinational,
            epsilon: 0.05,
            noise_channels: vec![0],
            gen_cfg: GeneratorConfig::T1_Constant {
                value: width.mask(),
            },
        });
    }

    for task in injection_task_panel() {
        for &wire_delay in &[WireDelayMode::Combinational, WireDelayMode::TicksPerHop] {
            let cfg = match wire_delay {
                WireDelayMode::Combinational => task.comb_cfg.clone(),
                WireDelayMode::TicksPerHop => {
                    let Some(c) = task.tph_cfg.clone() else {
                        continue;
                    };
                    c
                }
            };
            specs.push(SpontaneousSpec {
                label: task.label,
                tier: task.tier,
                width: task.width,
                target_port: task.target_port,
                wire_delay,
                epsilon: 0.0,
                noise_channels: Vec::new(),
                gen_cfg: cfg,
            });
        }
        if !task.is_autonomous_t1 || task.label.starts_with("T1_Alternating") {
            specs.push(SpontaneousSpec {
                label: task.label,
                tier: task.tier,
                width: task.width,
                target_port: task.target_port,
                wire_delay: WireDelayMode::Combinational,
                epsilon: 0.05,
                noise_channels: task.noise_channels.clone(),
                gen_cfg: task.comb_cfg.clone(),
            });
        }
    }

    // Add T2_DelayedAffine in Combinational and TicksPerHop (k=2).
    specs.push(SpontaneousSpec {
        label: "T2_DelayedAffine(d=2,k=2)",
        tier: "T2",
        width: Width::BITS_2,
        target_port: 1,
        wire_delay: WireDelayMode::Combinational,
        epsilon: 0.0,
        noise_channels: Vec::new(),
        gen_cfg: GeneratorConfig::T2_DelayedAffine {
            driver: d0.clone(),
            lag: 2,
            mul_a: 3,
            add_b: 1,
        },
    });
    specs.push(SpontaneousSpec {
        label: "T2_DelayedAffine(d=3,k=2)",
        tier: "T2",
        width: Width::BITS_2,
        target_port: 1,
        wire_delay: WireDelayMode::TicksPerHop,
        epsilon: 0.0,
        noise_channels: Vec::new(),
        gen_cfg: GeneratorConfig::T2_DelayedAffine {
            driver: d0,
            lag: 3,
            mul_a: 3,
            add_b: 1,
        },
    });
    let _ = (d1, d11);
    specs
}

fn build_causal_split_tph_editable(
    width: Width,
    econ: EconomyConfig,
    growth: &GrowthConfig,
) -> EditableCircuit {
    let mut c = Circuit::new(width, 3, 3, WireDelayMode::TicksPerHop);
    let split = c.add_node(Primitive::Split);
    c.connect(
        SignalSource::BoundaryIn(0),
        SignalSink::NodeIn {
            node: split,
            port: 0,
        },
    );
    c.connect(
        SignalSource::NodeOut {
            node: split,
            port: 0,
        },
        SignalSink::BoundaryOut(2),
    );
    let mut editable =
        EditableCircuit::new_with_growth(c, econ, growth).expect("valid split-primary editable");
    if let Some(ne) = editable.economy_mut().node_mut(split.index()) {
        ne.juvenile_ticks = econ.juvenile_ticks;
    }
    editable
}

fn build_autonomous_ring_tph_editable(
    width: Width,
    motif: &[u64],
    econ: EconomyConfig,
    growth: &GrowthConfig,
) -> EditableCircuit {
    let mut c = Circuit::new(width, 1, 1, WireDelayMode::TicksPerHop);
    build_autonomous_ring(&mut c, motif.len(), motif, 0);
    let mut ed = EditableCircuit::new_with_growth(c, econ, growth).expect("valid autonomous ring");
    for node_econ in ed.economy_mut().nodes_mut() {
        node_econ.juvenile_ticks = 0;
        node_econ.debt_paid = true;
    }
    let mut out = vec![0u64; 1];
    for _ in 0..24 {
        ed.step(&[0], &mut out).expect("tph warmup");
    }
    ed
}

fn target_active_comparator_count(economy: &EconomyState, target_port: u16) -> usize {
    economy
        .comparators()
        .iter()
        .filter(|c| c.target_port == target_port)
        .count()
}

fn target_cone_signature(
    circuit: &Circuit,
    economy: &EconomyState,
    target_port: u16,
) -> (Vec<(NodeId, Primitive)>, usize) {
    let pri = primary_cone_nodes(circuit, economy, target_port);
    let comp = comparator_cone_set(circuit, economy, Some(target_port));
    let mut nodes: Vec<(NodeId, Primitive)> = pri
        .union(&comp)
        .filter_map(|&id| circuit.nodes().get(id.index()).map(|&p| (id, p)))
        .collect();
    nodes.sort_by_key(|(id, _)| id.index());
    (nodes, target_active_comparator_count(economy, target_port))
}

fn target_cone_all_mature(circuit: &Circuit, economy: &EconomyState, target_port: u16) -> bool {
    let pri = primary_cone_nodes(circuit, economy, target_port);
    let comp = comparator_cone_set(circuit, economy, Some(target_port));
    let econ_nodes = economy.nodes();
    pri.union(&comp).all(|id| {
        econ_nodes
            .get(id.index())
            .is_none_or(|n| n.juvenile_ticks == 0)
    })
}

fn port_is_driven(circuit: &Circuit, port: u16) -> bool {
    circuit
        .edges()
        .iter()
        .any(|e| e.dst == SignalSink::BoundaryOut(port))
}

fn comparator_cone_set(
    circuit: &Circuit,
    economy: &EconomyState,
    target_port: Option<u16>,
) -> BTreeSet<NodeId> {
    let comps = economy.comparators();
    if comps.is_empty() {
        return BTreeSet::new();
    }
    let mut boundary_live = BTreeSet::new();
    let mut stack = Vec::new();
    for edge in circuit.edges() {
        if let SignalSink::BoundaryOut(_) = edge.dst
            && let SignalSource::NodeOut { node, .. } = edge.src
            && boundary_live.insert(node)
        {
            stack.push(node);
        }
    }
    while let Some(node) = stack.pop() {
        for edge in circuit.edges() {
            if let SignalSink::NodeIn { node: dst, .. } = edge.dst
                && dst == node
                && let SignalSource::NodeOut { node: src, .. } = edge.src
                && boundary_live.insert(src)
            {
                stack.push(src);
            }
        }
    }
    let mut comp_nodes = BTreeSet::new();
    let mut c_stack: Vec<NodeId> = comps
        .iter()
        .filter(|c| target_port.is_none_or(|tp| c.target_port == tp))
        .flat_map(|c| [c.sub_node, c.eqz_node])
        .collect();
    for &n in &c_stack {
        comp_nodes.insert(n);
    }
    while let Some(node) = c_stack.pop() {
        for edge in circuit.edges() {
            if let SignalSink::NodeIn { node: dst, .. } = edge.dst
                && dst == node
                && let SignalSource::NodeOut { node: src, .. } = edge.src
                && !boundary_live.contains(&src)
                && comp_nodes.insert(src)
            {
                c_stack.push(src);
            }
        }
    }
    comp_nodes
}

fn dead_or_unregistered_nodes(circuit: &Circuit, economy: &EconomyState) -> BTreeSet<NodeId> {
    let comp_set = comparator_cone_set(circuit, economy, None);
    let mut live_boundary = BTreeSet::new();
    let mut stack = Vec::new();
    for edge in circuit.edges() {
        if let SignalSink::BoundaryOut(_) = edge.dst
            && let SignalSource::NodeOut { node, .. } = edge.src
            && live_boundary.insert(node)
        {
            stack.push(node);
        }
    }
    while let Some(node) = stack.pop() {
        for edge in circuit.edges() {
            if let SignalSink::NodeIn { node: dst, .. } = edge.dst
                && dst == node
                && let SignalSource::NodeOut { node: src, .. } = edge.src
                && live_boundary.insert(src)
            {
                stack.push(src);
            }
        }
    }
    (0..circuit.node_count())
        .map(|i| NodeId(u32::try_from(i).expect("fits")))
        .filter(|n| !live_boundary.contains(n) && !comp_set.contains(n))
        .collect()
}

fn target_primary_and_dead_counts(
    circuit: &Circuit,
    economy: &EconomyState,
    target_port: u16,
    split_code_tax: bool,
) -> (usize, usize, f64) {
    let pri = primary_cone_nodes(circuit, economy, target_port);
    let dead = dead_or_unregistered_nodes(circuit, economy);
    let keep: BTreeSet<NodeId> = pri.union(&dead).copied().collect();
    let to_remove: BTreeSet<NodeId> = (0..circuit.node_count())
        .map(|i| NodeId(u32::try_from(i).expect("fits")))
        .filter(|n| !keep.contains(n))
        .collect();
    let bits = if to_remove.is_empty() {
        circuit_code_bits_with_tax(circuit, 0, split_code_tax)
    } else {
        let mut stripped = circuit.clone();
        stripped.remove_nodes(&to_remove);
        circuit_code_bits_with_tax(&stripped, 0, split_code_tax)
    };
    (pri.len(), dead.len(), bits)
}

fn build_noise(epsilon: f64, channels: &[u16]) -> NoiseModel {
    if epsilon <= 0.0 || channels.is_empty() {
        NoiseModel::None
    } else {
        NoiseModel::IndependentSubstitution {
            epsilon,
            channels: channels.to_vec(),
        }
    }
}

#[derive(Clone, Debug)]
struct InjectionRunOutcome {
    settled: bool,
    tph_non_autonomous_skipped: bool,
    t_inj: u64,
    excised_within_w_obs: bool,
    excised_within_3w_obs: bool,
    excised_by_end: bool,
    collapsed: bool,
    survived_bloated: bool,
    mechanism: PruneMechanism,
    excise_dt: Option<u64>,
    pre_inj_primary_nodes: usize,
    final_primary_nodes: usize,
    pre_inj_comp_nodes: usize,
    final_comp_nodes: usize,
    final_active_comparators: usize,
    comparator_bearing: bool,
    comparator_preserved: bool,
    pre_inj_primary_code_bits: f64,
    final_primary_code_bits: f64,
    final_total_code_bits: f64,
    primary_code_bits_restored: bool,
    post_inj_clean_mismatches: u64,
    post_inj_kt_excess_b_per_sym: f64,
    detour_peak_siphoned_energy: f64,
    detour_net_siphoned_energy: f64,
    exogenous_bits_err: f64,
    cliff: CliffCountersSummary,
}

#[allow(clippy::too_many_arguments)]
fn run_injection_case(
    task: &TaskSpec,
    wire_delay: WireDelayMode,
    epsilon: f64,
    split_code_tax: bool,
    excision_mode: ExcisionMode,
    injection_mode: InjectionMode,
    detour_class: DetourClass,
    seed: u64,
) -> InjectionRunOutcome {
    let econ_cfg = EconomyConfig {
        split_code_tax,
        ..EconomyConfig::default()
    };
    let growth_cfg = GrowthConfig {
        excision_mode,
        ..GrowthConfig::default()
    };
    let w_obs = observation_horizon(&econ_cfg, &growth_cfg) as u64;
    let gen_cfg = match wire_delay {
        WireDelayMode::Combinational => &task.comb_cfg,
        WireDelayMode::TicksPerHop => task.tph_cfg.as_ref().unwrap_or(&task.comb_cfg),
    };
    let noise = build_noise(epsilon, &task.noise_channels);
    let sb = SeedBank::new(seed);
    let generator = gen_cfg.build(task.width).expect("valid explore task");
    let channels = usize::from(generator.channels());
    let target_ch = usize::from(task.target_port);
    let declared_h = declared_noise_entropy_bits_per_symbol(&noise, channels as u16, task.width);
    let target_has_noise = epsilon > 0.0 && task.noise_channels.contains(&task.target_port);
    let target_h = if target_has_noise {
        crate::env::channel_substitution_entropy_bits(epsilon, task.width.bits())
    } else {
        let _ = declared_h;
        0.0
    };

    let mut env = NoisyEnvironment::new(generator, noise, sb).expect("valid noisy env");
    let mut tph_auto_probe: Option<(crate::substrate::CompiledCircuit, ScalarState)> = None;
    let mut learner = if wire_delay == WireDelayMode::TicksPerHop && task.is_autonomous_t1 {
        let motif = task
            .tph_ring_motif
            .expect("autonomous T1 task must define tph_ring_motif");
        let base_ed = build_autonomous_ring_tph_editable(task.width, motif, econ_cfg, &growth_cfg);
        let probe_compiled = base_ed.compiled().clone();
        let mut probe_state = ScalarState::new(&probe_compiled);
        probe_state
            .set_registers(&probe_compiled, base_ed.state().registers())
            .expect("set probe registers");
        tph_auto_probe = Some((probe_compiled, probe_state));
        ArkheLearner::from_editable(base_ed, growth_cfg, sb).expect("valid tph ring learner")
    } else if task.is_multiview && wire_delay == WireDelayMode::TicksPerHop {
        let base_ed = build_causal_split_tph_editable(task.width, econ_cfg, &growth_cfg);
        ArkheLearner::from_editable(base_ed, growth_cfg, sb).expect("valid tph split learner")
    } else {
        ArkheLearner::new_with_config(
            task.width,
            channels as u16,
            wire_delay,
            econ_cfg,
            growth_cfg,
            sb,
        )
        .expect("valid learner")
    };
    let mut budget = TickBudget::new(INJECTION_TICK_BUDGET);
    let mut clean = vec![0u64; channels];
    let mut obs = vec![0u64; channels];

    let step_pair = |env: &mut NoisyEnvironment<crate::env::TierGenerator>,
                     tph_probe: &mut Option<(crate::substrate::CompiledCircuit, ScalarState)>,
                     clean_buf: &mut [u64],
                     obs_buf: &mut [u64]| {
        if let Some((compiled, state)) = tph_probe.as_mut() {
            state
                .step(compiled, &[0], clean_buf)
                .expect("tph clean step");
            obs_buf[0] = clean_buf[0];
        } else {
            env.step_frame(clean_buf, obs_buf);
        }
    };

    let mut settled_at: Option<u64> = None;
    if tph_auto_probe.is_some() {
        for _ in 0..w_obs {
            step_pair(&mut env, &mut tph_auto_probe, &mut clean, &mut obs);
            let _ = learner.predict(&mut budget);
            learner.observe(&obs, &mut budget);
        }
        settled_at = Some(w_obs);
    } else if epsilon == 0.0 {
        let mut last_sig = target_cone_signature(
            learner.editable().circuit(),
            learner.editable().economy(),
            task.target_port,
        );
        let mut last_sig_change_tick = 0u64;
        let mut clean_window: VecDeque<bool> = VecDeque::with_capacity(w_obs as usize);
        for t in 0..MAX_SETTLE_TICKS {
            step_pair(&mut env, &mut tph_auto_probe, &mut clean, &mut obs);
            let pred = learner.predict(&mut budget);
            let is_clean_match = task.width.normalize(pred[target_ch]) == clean[target_ch];
            if clean_window.len() == w_obs as usize {
                clean_window.pop_front();
            }
            clean_window.push_back(is_clean_match);
            learner.observe(&obs, &mut budget);

            let curr_sig = target_cone_signature(
                learner.editable().circuit(),
                learner.editable().economy(),
                task.target_port,
            );
            if curr_sig != last_sig {
                last_sig = curr_sig;
                last_sig_change_tick = t + 1;
            }
            let stable_ticks = (t + 1).saturating_sub(last_sig_change_tick);
            let all_mature = target_cone_all_mature(
                learner.editable().circuit(),
                learner.editable().economy(),
                task.target_port,
            );
            let driven = port_is_driven(learner.editable().circuit(), task.target_port);
            let comp_ok = !task.is_multiview || learner.active_comparator_count() >= 1;
            let window_matches = clean_window.iter().filter(|&&ok| ok).count();
            if t + 1 >= w_obs
                && stable_ticks >= w_obs
                && all_mature
                && driven
                && comp_ok
                && clean_window.len() == w_obs as usize
                && window_matches == w_obs as usize
            {
                settled_at = Some(t + 1);
                break;
            }
        }
    } else {
        // For noisy injection cells (G3.2/G3.3): reach clean lock first, then run W_obs pre-injection
        // ticks under active eps=0.05 noise before injecting the detour.
        let mut last_sig = target_cone_signature(
            learner.editable().circuit(),
            learner.editable().economy(),
            task.target_port,
        );
        let mut last_sig_change_tick = 0u64;
        let mut clean_window: VecDeque<bool> = VecDeque::with_capacity(w_obs as usize);
        let mut locked_tick: Option<u64> = None;
        for t in 0..MAX_SETTLE_TICKS {
            step_pair(&mut env, &mut tph_auto_probe, &mut clean, &mut obs);
            let pred = learner.predict(&mut budget);
            let is_clean_match = task.width.normalize(pred[target_ch]) == clean[target_ch];
            if clean_window.len() == w_obs as usize {
                clean_window.pop_front();
            }
            clean_window.push_back(is_clean_match);
            learner.observe(&clean, &mut budget);

            let curr_sig = target_cone_signature(
                learner.editable().circuit(),
                learner.editable().economy(),
                task.target_port,
            );
            if curr_sig != last_sig {
                last_sig = curr_sig;
                last_sig_change_tick = t + 1;
            }
            let stable_ticks = (t + 1).saturating_sub(last_sig_change_tick);
            let all_mature = target_cone_all_mature(
                learner.editable().circuit(),
                learner.editable().economy(),
                task.target_port,
            );
            let driven = port_is_driven(learner.editable().circuit(), task.target_port);
            let comp_ok = !task.is_multiview || learner.active_comparator_count() >= 1;
            let window_matches = clean_window.iter().filter(|&&ok| ok).count();
            if t + 1 >= w_obs
                && stable_ticks >= w_obs
                && all_mature
                && driven
                && comp_ok
                && clean_window.len() == w_obs as usize
                && window_matches == w_obs as usize
            {
                locked_tick = Some(t + 1);
                break;
            }
        }
        if let Some(t_lock) = locked_tick {
            for _ in 0..w_obs {
                step_pair(&mut env, &mut tph_auto_probe, &mut clean, &mut obs);
                let _ = learner.predict(&mut budget);
                learner.observe(&obs, &mut budget);
            }
            settled_at = Some(t_lock + w_obs);
        }
    }

    let Some(t_inj) = settled_at else {
        let tel = learner.telemetry();
        let (final_pri, _, final_pri_bits) = target_primary_and_dead_counts(
            learner.editable().circuit(),
            learner.editable().economy(),
            task.target_port,
            split_code_tax,
        );
        let final_comp = comparator_cone_set(
            learner.editable().circuit(),
            learner.editable().economy(),
            Some(task.target_port),
        )
        .len();
        return InjectionRunOutcome {
            settled: false,
            tph_non_autonomous_skipped: false,
            t_inj: MAX_SETTLE_TICKS,
            excised_within_w_obs: false,
            excised_within_3w_obs: false,
            excised_by_end: false,
            collapsed: false,
            survived_bloated: false,
            mechanism: PruneMechanism::NotSettled,
            excise_dt: None,
            pre_inj_primary_nodes: final_pri,
            final_primary_nodes: final_pri,
            pre_inj_comp_nodes: final_comp,
            final_comp_nodes: final_comp,
            final_active_comparators: tel.active_comparator_count,
            comparator_bearing: task.is_multiview,
            comparator_preserved: false,
            pre_inj_primary_code_bits: final_pri_bits,
            final_primary_code_bits: final_pri_bits,
            final_total_code_bits: circuit_code_bits_with_tax(
                learner.editable().circuit(),
                0,
                split_code_tax,
            ),
            primary_code_bits_restored: false,
            post_inj_clean_mismatches: 0,
            post_inj_kt_excess_b_per_sym: 0.0,
            detour_peak_siphoned_energy: 0.0,
            detour_net_siphoned_energy: 0.0,
            exogenous_bits_err: 0.0,
            cliff: CliffCountersSummary::from(&tel),
        };
    };

    let pre_circuit = learner.editable().circuit().clone();
    let pre_econ = learner.editable().economy();
    let (pre_inj_primary_nodes, pre_inj_dead_nodes, pre_inj_primary_code_bits) =
        target_primary_and_dead_counts(&pre_circuit, pre_econ, task.target_port, split_code_tax);
    let pre_inj_comp_nodes =
        comparator_cone_set(&pre_circuit, pre_econ, Some(task.target_port)).len();
    let pre_inj_total_code_bits = circuit_code_bits_with_tax(&pre_circuit, 0, split_code_tax);
    let pre_inj_comp_count = learner.active_comparator_count();
    let pre_inj_disagree_prunes = learner.disagreement_prune_events();
    let pre_inj_handoffs = learner.driver_handoff_events();
    let pre_inj_rings = learner.ring_closure_events();
    let pre_inj_idle = learner.idle_prune_events();
    let pre_inj_starv = learner.starvation_prune_events();
    let pre_inj_local = learner.local_excision_events();
    let pre_driven = port_is_driven(&pre_circuit, task.target_port);

    if wire_delay == WireDelayMode::TicksPerHop && detour_class.tph_series_hop_delta() > 0 {
        let orig_driver = pre_circuit
            .edges()
            .iter()
            .find(|e| e.dst == SignalSink::BoundaryOut(task.target_port))
            .map(|e| e.src);
        let is_auto = orig_driver.is_some_and(|src| {
            cone_channel_mask(&pre_circuit, src) == 0 && source_has_cycle(&pre_circuit, src)
        });
        if !is_auto {
            let tel = learner.telemetry();
            return InjectionRunOutcome {
                settled: true,
                tph_non_autonomous_skipped: true,
                t_inj,
                excised_within_w_obs: false,
                excised_within_3w_obs: false,
                excised_by_end: false,
                collapsed: false,
                survived_bloated: false,
                mechanism: PruneMechanism::SkippedNonAutonomousTph,
                excise_dt: None,
                pre_inj_primary_nodes,
                final_primary_nodes: pre_inj_primary_nodes,
                pre_inj_comp_nodes,
                final_comp_nodes: pre_inj_comp_nodes,
                final_active_comparators: pre_inj_comp_count,
                comparator_bearing: pre_inj_comp_count > 0,
                comparator_preserved: true,
                pre_inj_primary_code_bits,
                final_primary_code_bits: pre_inj_primary_code_bits,
                final_total_code_bits: pre_inj_total_code_bits,
                primary_code_bits_restored: true,
                post_inj_clean_mismatches: 0,
                post_inj_kt_excess_b_per_sym: 0.0,
                detour_peak_siphoned_energy: 0.0,
                detour_net_siphoned_energy: 0.0,
                exogenous_bits_err: 0.0,
                cliff: CliffCountersSummary::from(&tel),
            };
        }
    }

    let edit = build_detour_edit(learner.editable(), task.target_port, detour_class)
        .expect("build_detour_edit succeeds on settled circuit");
    let post_inj_total_code_bits = circuit_code_bits_with_tax(&edit.circuit, 0, split_code_tax);
    let expected_exog = match injection_mode {
        InjectionMode::Established => post_inj_total_code_bits - pre_inj_total_code_bits,
        InjectionMode::Juvenile => 0.0,
    };
    let exog_before = learner.exogenous_injection_bits();
    edit.apply_to_learner(&mut learner, injection_mode)
        .expect("apply detour edit");
    let exogenous_bits_err =
        ((learner.exogenous_injection_bits() - exog_before) - expected_exog).abs();

    let post_inj_epoch = learner.editable().structure_epoch();
    let initial_detour_energy = (edit.injected.len() as f64) * econ_cfg.initial_energy;
    let mut detour_peak_siphoned_energy = 0.0f64;
    let mut detour_net_siphoned_energy = 0.0f64;

    let post_horizon = 4 * w_obs;
    let mut post_inj_clean_mismatches = 0u64;
    let mut post_inj_obs_mismatches = 0u64;
    let mut collapsed = false;
    let mut excise_dt: Option<u64> = None;

    for dt in 1..=post_horizon {
        step_pair(&mut env, &mut tph_auto_probe, &mut clean, &mut obs);
        let pred = learner.predict(&mut budget);
        let p_norm = task.width.normalize(pred[target_ch]);
        if p_norm != clean[target_ch] {
            post_inj_clean_mismatches += 1;
        }
        if p_norm != obs[target_ch] {
            post_inj_obs_mismatches += 1;
        }
        learner.observe(&obs, &mut budget);

        if learner.editable().structure_epoch() == post_inj_epoch {
            let econ_nodes = learner.editable().economy().nodes();
            let detour_e: f64 = edit
                .injected
                .iter()
                .filter_map(|id| econ_nodes.get(id.index()))
                .map(|n| n.energy)
                .sum();
            let siphoned = detour_e - initial_detour_energy;
            detour_peak_siphoned_energy = detour_peak_siphoned_energy.max(siphoned);
            detour_net_siphoned_energy = siphoned;
        }

        let c_now = learner.editable().circuit();
        let e_now = learner.editable().economy();
        let curr_driven = port_is_driven(c_now, task.target_port);
        let (curr_pri, curr_dead, curr_pri_bits) =
            target_primary_and_dead_counts(c_now, e_now, task.target_port, split_code_tax);
        let endogenous_primary_swap = learner.driver_handoff_events() > pre_inj_handoffs
            || learner.ring_closure_events() > pre_inj_rings;
        let size_and_mdl_restored =
            curr_pri <= pre_inj_primary_nodes && curr_pri_bits <= pre_inj_primary_code_bits + 1e-9;
        let iso_mdl_improved =
            curr_pri <= pre_inj_primary_nodes && curr_pri_bits + 1e-9 < pre_inj_primary_code_bits;
        let restored_now = curr_dead <= pre_inj_dead_nodes
            && ((learner.surviving_injected_detour_nodes() == 0
                && (size_and_mdl_restored || endogenous_primary_swap))
                || iso_mdl_improved);

        if (pre_driven && !curr_driven) || (pre_inj_primary_nodes > 0 && curr_pri == 0) {
            collapsed = true;
        }
        if !collapsed
            && excise_dt.is_none()
            && restored_now
            && (!pre_driven || (curr_driven && (pre_inj_primary_nodes == 0 || curr_pri > 0)))
        {
            excise_dt = Some(dt);
        }
    }

    let final_circuit = learner.editable().circuit();
    let final_econ = learner.editable().economy();
    let (final_primary_nodes, final_dead_nodes, final_primary_code_bits) =
        target_primary_and_dead_counts(final_circuit, final_econ, task.target_port, split_code_tax);
    let final_comp_nodes =
        comparator_cone_set(final_circuit, final_econ, Some(task.target_port)).len();
    let final_total_code_bits = circuit_code_bits_with_tax(final_circuit, 0, split_code_tax);
    let endogenous_primary_swap = learner.driver_handoff_events() > pre_inj_handoffs
        || learner.ring_closure_events() > pre_inj_rings;
    let final_size_and_mdl_restored = final_primary_nodes <= pre_inj_primary_nodes
        && final_primary_code_bits <= pre_inj_primary_code_bits + 1e-9;
    let final_iso_mdl_improved = final_primary_nodes <= pre_inj_primary_nodes
        && final_primary_code_bits + 1e-9 < pre_inj_primary_code_bits;
    let primary_code_bits_restored = final_dead_nodes <= pre_inj_dead_nodes
        && ((learner.surviving_injected_detour_nodes() == 0
            && (final_size_and_mdl_restored || endogenous_primary_swap))
            || final_iso_mdl_improved);

    let delta_idle = learner.idle_prune_events() - pre_inj_idle;
    let delta_starv = learner.starvation_prune_events() - pre_inj_starv;
    let delta_local = learner.local_excision_events() - pre_inj_local;
    let survived_bloated = !collapsed && !primary_code_bits_restored;

    let mechanism = if collapsed {
        PruneMechanism::CollapseRegrow
    } else if survived_bloated {
        PruneMechanism::SurvivedBloated
    } else if delta_local > 0 {
        PruneMechanism::LocalExcision
    } else if delta_starv > 0 {
        PruneMechanism::StarvationBypass
    } else if delta_idle > 0 {
        PruneMechanism::IdleD1b1
    } else {
        PruneMechanism::SurvivedBloated
    };

    let excised_by_end = !collapsed && primary_code_bits_restored && excise_dt.is_some();
    let excised_within_w_obs = excised_by_end && excise_dt.is_some_and(|dt| dt <= w_obs);
    let excised_within_3w_obs = excised_by_end && excise_dt.is_some_and(|dt| dt <= 3 * w_obs);

    let post_inj_kt =
        facts_code_bits(task.width, post_horizon, post_inj_obs_mismatches) / (post_horizon as f64);
    let post_inj_kt_excess_b_per_sym = post_inj_kt - target_h;
    let tel = learner.telemetry();
    let comparator_bearing = pre_inj_comp_count > 0 || task.is_multiview;
    let comparator_preserved = !comparator_bearing
        || (tel.active_comparator_count >= 1
            && tel.disagreement_prune_events == pre_inj_disagree_prunes);

    InjectionRunOutcome {
        settled: true,
        tph_non_autonomous_skipped: false,
        t_inj,
        excised_within_w_obs,
        excised_within_3w_obs,
        excised_by_end,
        collapsed,
        survived_bloated,
        mechanism,
        excise_dt,
        pre_inj_primary_nodes,
        final_primary_nodes,
        pre_inj_comp_nodes,
        final_comp_nodes,
        final_active_comparators: tel.active_comparator_count,
        comparator_bearing,
        comparator_preserved,
        pre_inj_primary_code_bits,
        final_primary_code_bits,
        final_total_code_bits,
        primary_code_bits_restored,
        post_inj_clean_mismatches,
        post_inj_kt_excess_b_per_sym,
        detour_peak_siphoned_energy,
        detour_net_siphoned_energy,
        exogenous_bits_err,
        cliff: CliffCountersSummary::from(&tel),
    }
}

#[allow(clippy::too_many_arguments)]
fn summarize_injection_cell(
    sweep_tag: &str,
    task: &TaskSpec,
    wire_delay: WireDelayMode,
    epsilon: f64,
    split_code_tax: bool,
    excision_mode: ExcisionMode,
    injection_mode: InjectionMode,
    detour_class: DetourClass,
    outcomes: &[InjectionRunOutcome],
) -> InjectionCellSummary {
    let total_runs = outcomes.len();
    let mut settled_runs = 0usize;
    let mut applicable_runs = 0usize;
    let mut excised_within_w_obs = 0usize;
    let mut excised_within_3w_obs = 0usize;
    let mut excised_by_end = 0usize;
    let mut collapsed_runs = 0usize;
    let mut survived_bloated_runs = 0usize;
    let mut mechanism_counts = PruneMechanismCounts::default();
    let mut t_inj_sum = 0.0f64;
    let mut excise_dts = Vec::new();
    let mut pre_pri_sum = 0.0f64;
    let mut final_pri_sum = 0.0f64;
    let mut max_final_primary_nodes = 0usize;
    let mut pre_comp_sum = 0.0f64;
    let mut final_comp_sum = 0.0f64;
    let mut min_final_active_comparators = usize::MAX;
    let mut comparator_bearing_runs = 0usize;
    let mut comparator_preserved_runs = 0usize;
    let mut pre_bits_sum = 0.0f64;
    let mut final_bits_sum = 0.0f64;
    let mut primary_code_bits_restored_runs = 0usize;
    let mut clean_mis_sum = 0.0f64;
    let mut max_post_inj_clean_mismatches = 0u64;
    let mut kt_excess_sum = 0.0f64;
    let mut max_post_inj_kt_excess_b_per_sym = f64::NEG_INFINITY;
    let mut peak_siphoned_sum = 0.0f64;
    let mut max_detour_peak_siphoned_energy = 0.0f64;
    let mut net_siphoned_sum = 0.0f64;
    let mut max_exogenous_bits_err = 0.0f64;
    let mut cliff_counters = CliffCountersSummary::default();

    for r in outcomes {
        cliff_counters.accumulate(&r.cliff);
        mechanism_counts.record(r.mechanism);
        if r.settled {
            settled_runs += 1;
        }
        if r.settled && !r.tph_non_autonomous_skipped {
            applicable_runs += 1;
            t_inj_sum += r.t_inj as f64;
            if r.excised_within_w_obs {
                excised_within_w_obs += 1;
            }
            if r.excised_within_3w_obs {
                excised_within_3w_obs += 1;
            }
            if r.excised_by_end {
                excised_by_end += 1;
            }
            if r.collapsed {
                collapsed_runs += 1;
            }
            if r.survived_bloated {
                survived_bloated_runs += 1;
            }
            if let Some(dt) = r.excise_dt {
                excise_dts.push(dt);
            }
            pre_pri_sum += r.pre_inj_primary_nodes as f64;
            final_pri_sum += r.final_primary_nodes as f64;
            max_final_primary_nodes = max_final_primary_nodes.max(r.final_primary_nodes);
            pre_comp_sum += r.pre_inj_comp_nodes as f64;
            final_comp_sum += r.final_comp_nodes as f64;
            min_final_active_comparators =
                min_final_active_comparators.min(r.final_active_comparators);
            if r.comparator_bearing {
                comparator_bearing_runs += 1;
                if r.comparator_preserved {
                    comparator_preserved_runs += 1;
                }
            }
            pre_bits_sum += r.pre_inj_primary_code_bits;
            final_bits_sum += r.final_primary_code_bits;
            if r.primary_code_bits_restored {
                primary_code_bits_restored_runs += 1;
            }
            clean_mis_sum += r.post_inj_clean_mismatches as f64;
            max_post_inj_clean_mismatches =
                max_post_inj_clean_mismatches.max(r.post_inj_clean_mismatches);
            kt_excess_sum += r.post_inj_kt_excess_b_per_sym;
            max_post_inj_kt_excess_b_per_sym =
                max_post_inj_kt_excess_b_per_sym.max(r.post_inj_kt_excess_b_per_sym);
            peak_siphoned_sum += r.detour_peak_siphoned_energy;
            max_detour_peak_siphoned_energy =
                max_detour_peak_siphoned_energy.max(r.detour_peak_siphoned_energy);
            net_siphoned_sum += r.detour_net_siphoned_energy;
            max_exogenous_bits_err = max_exogenous_bits_err.max(r.exogenous_bits_err);
        }
    }

    let denom = applicable_runs.max(1) as f64;
    let (min_excise_dt, mean_excise_dt, max_excise_dt) = if excise_dts.is_empty() {
        (None, None, None)
    } else {
        let min_v = excise_dts.iter().copied().min();
        let max_v = excise_dts.iter().copied().max();
        let sum_v: u64 = excise_dts.iter().sum();
        (
            min_v,
            Some((sum_v as f64) / (excise_dts.len() as f64)),
            max_v,
        )
    };
    if min_final_active_comparators == usize::MAX {
        min_final_active_comparators = 0;
    }
    if max_post_inj_kt_excess_b_per_sym == f64::NEG_INFINITY {
        max_post_inj_kt_excess_b_per_sym = 0.0;
    }

    InjectionCellSummary {
        sweep_tag: sweep_tag.to_owned(),
        task: task.label.to_owned(),
        tier: task.tier.to_owned(),
        width_bits: task.width.bits(),
        wire_delay,
        epsilon,
        split_code_tax,
        excision_mode,
        injection_mode,
        detour_class,
        total_runs,
        settled_runs,
        applicable_runs,
        excised_within_w_obs,
        excised_within_3w_obs,
        excised_by_end,
        collapsed_runs,
        survived_bloated_runs,
        mechanism_counts,
        mean_t_inj: t_inj_sum / denom,
        min_excise_dt,
        mean_excise_dt,
        max_excise_dt,
        mean_pre_inj_primary_nodes: pre_pri_sum / denom,
        mean_final_primary_nodes: final_pri_sum / denom,
        max_final_primary_nodes,
        mean_pre_inj_comp_nodes: pre_comp_sum / denom,
        mean_final_comp_nodes: final_comp_sum / denom,
        min_final_active_comparators,
        comparator_bearing_runs,
        comparator_preserved_runs,
        mean_pre_inj_primary_code_bits: pre_bits_sum / denom,
        mean_final_primary_code_bits: final_bits_sum / denom,
        primary_code_bits_restored_runs,
        mean_post_inj_clean_mismatches: clean_mis_sum / denom,
        max_post_inj_clean_mismatches,
        mean_post_inj_kt_excess_b_per_sym: kt_excess_sum / denom,
        max_post_inj_kt_excess_b_per_sym,
        mean_detour_peak_siphoned_energy: peak_siphoned_sum / denom,
        max_detour_peak_siphoned_energy,
        mean_detour_net_siphoned_energy: net_siphoned_sum / denom,
        max_exogenous_bits_err,
        cliff_counters,
    }
}

#[derive(Clone, Debug)]
struct SpontaneousRunOutcome {
    locked: bool,
    t_lock: Option<u64>,
    n_primary_at_lock: Option<usize>,
    n_primary_2000: usize,
    n_primary_8000: usize,
    post_lock_slope: Option<f64>,
    non_increasing_post_lock: bool,
    active_comp_2000: usize,
    active_comp_8000: usize,
    final_split_count: usize,
    final_code_bits: f64,
    post_warmup_kt_excess_8000: f64,
    clean_locked_8000: bool,
    cliff: CliffCountersSummary,
}

fn run_spontaneous_case(
    spec: &SpontaneousSpec,
    split_code_tax: bool,
    seed: u64,
) -> SpontaneousRunOutcome {
    let econ_cfg = EconomyConfig {
        split_code_tax,
        ..EconomyConfig::default()
    };
    let growth_cfg = GrowthConfig::default();
    let w_obs = observation_horizon(&econ_cfg, &growth_cfg) as u64;
    let noise = build_noise(spec.epsilon, &spec.noise_channels);
    let sb = SeedBank::new(seed);
    let generator = spec
        .gen_cfg
        .build(spec.width)
        .expect("valid spontaneous generator");
    let channels = usize::from(generator.channels());
    let target_ch = usize::from(spec.target_port);
    let target_has_noise = spec.epsilon > 0.0 && spec.noise_channels.contains(&spec.target_port);
    let target_h = if target_has_noise {
        crate::env::channel_substitution_entropy_bits(spec.epsilon, spec.width.bits())
    } else {
        0.0
    };

    let mut env = NoisyEnvironment::new(generator, noise, sb).expect("valid env");
    let mut learner =
        if spec.wire_delay == WireDelayMode::TicksPerHop && spec.tier == "T2_MultiViewCoupled" {
            let base_ed = build_causal_split_tph_editable(spec.width, econ_cfg, &growth_cfg);
            ArkheLearner::from_editable(base_ed, growth_cfg, sb).expect("valid tph split learner")
        } else {
            ArkheLearner::new_with_config(
                spec.width,
                channels as u16,
                spec.wire_delay,
                econ_cfg,
                growth_cfg,
                sb,
            )
            .expect("valid learner")
        };
    let mut budget = TickBudget::new(SPONTANEOUS_BUDGET_T8000);
    let mut clean = vec![0u64; channels];
    let mut obs = vec![0u64; channels];

    let is_noisy_constant =
        matches!(spec.gen_cfg, GeneratorConfig::T1_Constant { .. }) && spec.epsilon > 0.0;
    let mut clean_window: VecDeque<bool> = VecDeque::with_capacity(w_obs as usize);
    let mut last_sig = target_cone_signature(
        learner.editable().circuit(),
        learner.editable().economy(),
        spec.target_port,
    );
    let mut last_sig_change_tick = 0u64;
    let mut t_lock: Option<u64> = None;
    let mut n_primary_at_lock: Option<usize> = None;

    let mut n_primary_2000 = 0usize;
    let mut active_comp_2000 = 0usize;
    let mut post_warmup_obs_mis_8000 = 0u64;
    let mut tail_clean_mis_8000 = 0u64;

    for t in 0..SPONTANEOUS_T8000 {
        env.step_frame(&mut clean, &mut obs);
        let pred = learner.predict(&mut budget);
        let p_norm = spec.width.normalize(pred[target_ch]);
        let clean_ok = p_norm == clean[target_ch];
        if clean_window.len() == w_obs as usize {
            clean_window.pop_front();
        }
        clean_window.push_back(clean_ok);
        if t >= SPONTANEOUS_WARMUP && p_norm != obs[target_ch] {
            post_warmup_obs_mis_8000 += 1;
        }
        if t + w_obs >= SPONTANEOUS_T8000 && !clean_ok {
            tail_clean_mis_8000 += 1;
        }

        learner.observe(&obs, &mut budget);

        let curr_sig = target_cone_signature(
            learner.editable().circuit(),
            learner.editable().economy(),
            spec.target_port,
        );
        if curr_sig != last_sig {
            last_sig = curr_sig;
            last_sig_change_tick = t + 1;
        }
        if t_lock.is_none() && t + 1 >= w_obs {
            let all_mature = target_cone_all_mature(
                learner.editable().circuit(),
                learner.editable().economy(),
                spec.target_port,
            );
            let stable_ticks = (t + 1).saturating_sub(last_sig_change_tick);
            let matches = clean_window.iter().filter(|&&ok| ok).count();
            let is_locked = if is_noisy_constant {
                stable_ticks >= w_obs
                    && all_mature
                    && learner.editable().circuit().node_count() == 0
            } else if spec.epsilon == 0.0 {
                clean_window.len() == w_obs as usize && matches == w_obs as usize && all_mature
            } else {
                stable_ticks >= w_obs && all_mature && (matches as f64) / (w_obs as f64) >= 0.80
            };
            if is_locked {
                t_lock = Some(t + 1);
                n_primary_at_lock = Some(
                    primary_cone_nodes(
                        learner.editable().circuit(),
                        learner.editable().economy(),
                        spec.target_port,
                    )
                    .len(),
                );
            }
        }

        if t + 1 == SPONTANEOUS_T2000 {
            n_primary_2000 = primary_cone_nodes(
                learner.editable().circuit(),
                learner.editable().economy(),
                spec.target_port,
            )
            .len();
            active_comp_2000 = learner.active_comparator_count();
        }
    }

    let n_primary_8000 = primary_cone_nodes(
        learner.editable().circuit(),
        learner.editable().economy(),
        spec.target_port,
    )
    .len();
    let active_comp_8000 = learner.active_comparator_count();
    let tel = learner.telemetry();
    let final_code_bits =
        circuit_code_bits_with_tax(learner.editable().circuit(), 0, split_code_tax);
    let eval_ticks = SPONTANEOUS_T8000 - SPONTANEOUS_WARMUP;
    let kt_8000 =
        facts_code_bits(spec.width, eval_ticks, post_warmup_obs_mis_8000) / (eval_ticks as f64);
    let post_warmup_kt_excess_8000 = kt_8000 - target_h;

    let (post_lock_slope, non_increasing_post_lock) =
        if let (Some(tl), Some(nl)) = (t_lock, n_primary_at_lock) {
            let dt = SPONTANEOUS_T8000.saturating_sub(tl).max(1) as f64;
            let slope = ((n_primary_8000 as f64) - (nl as f64)) / dt;
            (
                Some(slope),
                n_primary_8000 <= n_primary_2000 && n_primary_2000 <= nl,
            )
        } else {
            (None, false)
        };

    let clean_locked_8000 = if is_noisy_constant {
        n_primary_8000 == 0
    } else if spec.epsilon == 0.0 {
        tail_clean_mis_8000 == 0
    } else {
        t_lock.is_some()
    };

    SpontaneousRunOutcome {
        locked: t_lock.is_some(),
        t_lock,
        n_primary_at_lock,
        n_primary_2000,
        n_primary_8000,
        post_lock_slope,
        non_increasing_post_lock,
        active_comp_2000,
        active_comp_8000,
        final_split_count: tel.final_split_count,
        final_code_bits,
        post_warmup_kt_excess_8000,
        clean_locked_8000,
        cliff: CliffCountersSummary::from(&tel),
    }
}

fn compute_n_min_for_spontaneous(spec: &SpontaneousSpec) -> (Option<usize>, String) {
    let Ok(mut generator) = spec.gen_cfg.build(spec.width) else {
        return (None, "generator build error".to_owned());
    };
    let channels = generator.channels();
    if channels > 2 {
        return (None, format!("SkippedBoundsExceeded(K={channels}>2)"));
    }
    let spec_opt = generator.to_finite_state_spec().ok();
    let stationary = spec_opt
        .as_ref()
        .filter(|s| s.scope == EpsilonMachineScope::StationaryProcess);
    let mut clean_trace = Vec::with_capacity(256);
    let mut frame = vec![0u64; usize::from(channels)];
    for _ in 0..256 {
        generator.next_clean_frame(&mut frame);
        clean_trace.push(frame.clone());
    }
    let max_nodes = if channels == 1 { 4 } else { 3 };
    let (circ_opt, status) = exact_synthesize_certified_circuit(
        spec.width,
        channels,
        spec.wire_delay,
        stationary,
        &clean_trace,
        max_nodes,
        DEFAULT_MAX_CERTIFY_JOINT_STEPS,
    );
    (circ_opt.map(|c| c.node_count()), format!("{status:?}"))
}

fn summarize_spontaneous_cell(
    spec: &SpontaneousSpec,
    split_code_tax: bool,
    n_min: Option<usize>,
    exact_status: &str,
    outcomes: &[SpontaneousRunOutcome],
) -> SpontaneousCellSummary {
    let total_runs = outcomes.len();
    let denom = total_runs.max(1) as f64;
    let mut locked_runs = 0usize;
    let mut t_lock_sum = 0.0f64;
    let mut n_lock_sum = 0.0f64;
    let mut n2000_sum = 0.0f64;
    let mut max_n_primary_2000 = 0usize;
    let mut n8000_sum = 0.0f64;
    let mut max_n_primary_8000 = 0usize;
    let mut slope_sum = 0.0f64;
    let mut slope_count = 0usize;
    let mut max_post_lock_slope: Option<f64> = None;
    let mut non_increasing_post_lock_runs = 0usize;
    let mut c2000_sum = 0.0f64;
    let mut max_active_comp_2000 = 0usize;
    let mut c8000_sum = 0.0f64;
    let mut max_active_comp_8000 = 0usize;
    let mut split_sum = 0.0f64;
    let mut bits_sum = 0.0f64;
    let mut kt_sum = 0.0f64;
    let mut clean_locked_8000_runs = 0usize;
    let mut cliff_counters = CliffCountersSummary::default();

    let mut diff_sum = 0.0f64;
    let mut max_diff: Option<isize> = None;
    let mut ratio_sum = 0.0f64;
    let mut max_ratio: Option<f64> = None;

    for r in outcomes {
        cliff_counters.accumulate(&r.cliff);
        if r.locked {
            locked_runs += 1;
            if let Some(tl) = r.t_lock {
                t_lock_sum += tl as f64;
            }
            if let Some(nl) = r.n_primary_at_lock {
                n_lock_sum += nl as f64;
            }
        }
        if let Some(slope) = r.post_lock_slope {
            slope_sum += slope;
            slope_count += 1;
            max_post_lock_slope = Some(max_post_lock_slope.map_or(slope, |m| m.max(slope)));
        }
        if r.non_increasing_post_lock {
            non_increasing_post_lock_runs += 1;
        }
        n2000_sum += r.n_primary_2000 as f64;
        max_n_primary_2000 = max_n_primary_2000.max(r.n_primary_2000);
        n8000_sum += r.n_primary_8000 as f64;
        max_n_primary_8000 = max_n_primary_8000.max(r.n_primary_8000);
        c2000_sum += r.active_comp_2000 as f64;
        max_active_comp_2000 = max_active_comp_2000.max(r.active_comp_2000);
        c8000_sum += r.active_comp_8000 as f64;
        max_active_comp_8000 = max_active_comp_8000.max(r.active_comp_8000);
        split_sum += r.final_split_count as f64;
        bits_sum += r.final_code_bits;
        kt_sum += r.post_warmup_kt_excess_8000;
        if r.clean_locked_8000 {
            clean_locked_8000_runs += 1;
        }
        if let Some(nm) = n_min {
            let diff = (r.n_primary_8000 as isize) - (nm as isize);
            diff_sum += diff as f64;
            max_diff = Some(max_diff.map_or(diff, |m| m.max(diff)));
            let ratio = (r.n_primary_8000 as f64) / (nm.max(1) as f64);
            ratio_sum += ratio;
            max_ratio = Some(max_ratio.map_or(ratio, |m| m.max(ratio)));
        }
    }

    SpontaneousCellSummary {
        task: spec.label.to_owned(),
        tier: spec.tier.to_owned(),
        width_bits: spec.width.bits(),
        wire_delay: spec.wire_delay,
        epsilon: spec.epsilon,
        split_code_tax,
        n_min,
        exact_synthesis_status: exact_status.to_owned(),
        total_runs,
        locked_runs,
        mean_t_lock: (locked_runs > 0).then(|| t_lock_sum / (locked_runs as f64)),
        mean_n_primary_at_lock: (locked_runs > 0).then(|| n_lock_sum / (locked_runs as f64)),
        mean_n_primary_2000: n2000_sum / denom,
        max_n_primary_2000,
        mean_n_primary_8000: n8000_sum / denom,
        max_n_primary_8000,
        mean_post_lock_slope: (slope_count > 0).then(|| slope_sum / (slope_count as f64)),
        max_post_lock_slope,
        non_increasing_post_lock_runs,
        mean_active_comp_2000: c2000_sum / denom,
        max_active_comp_2000,
        mean_active_comp_8000: c8000_sum / denom,
        max_active_comp_8000,
        mean_final_split_count: split_sum / denom,
        mean_final_code_bits: bits_sum / denom,
        mean_n_primary_minus_n_min: n_min.map(|_| diff_sum / denom),
        max_n_primary_minus_n_min: max_diff,
        mean_n_primary_over_n_min: n_min.map(|_| ratio_sum / denom),
        max_n_primary_over_n_min: max_ratio,
        mean_post_warmup_kt_excess_8000: kt_sum / denom,
        clean_locked_8000_runs,
        cliff_counters,
    }
}

fn build_conservation_base_editable(
    family: &str,
    width: Width,
    econ: EconomyConfig,
    growth: GrowthConfig,
    init_pri: u64,
    init_alt: u64,
) -> (EditableCircuit, u16) {
    let (mut ed, target_port) = match family {
        "T1" => {
            let mut c = Circuit::new(width, 1, 1, WireDelayMode::Combinational);
            let motif = if width == Width::BITS_1 {
                vec![0, 0, 0, 0, 1]
            } else {
                vec![0, 1, 3, 2, 1]
            };
            build_autonomous_ring(&mut c, motif.len(), &motif, 0);
            (
                EditableCircuit::new_with_growth(c, econ, &growth).expect("T1 ring"),
                0,
            )
        }
        "T2" => {
            let mut c = Circuit::new(width, 2, 2, WireDelayMode::Combinational);
            let d = c.add_node(Primitive::Delay {
                init: width.normalize(init_pri),
            });
            c.connect(
                SignalSource::BoundaryIn(0),
                SignalSink::NodeIn { node: d, port: 0 },
            );
            c.connect(
                SignalSource::NodeOut { node: d, port: 0 },
                SignalSink::BoundaryOut(1),
            );
            (
                EditableCircuit::new_with_growth(c, econ, &growth).expect("T2 circuit"),
                1,
            )
        }
        _ => {
            let mut c = Circuit::new(width, 3, 3, WireDelayMode::Combinational);
            let d_pri = c.add_node(Primitive::Delay {
                init: width.normalize(init_pri),
            });
            c.connect(
                SignalSource::BoundaryIn(0),
                SignalSink::NodeIn {
                    node: d_pri,
                    port: 0,
                },
            );
            c.connect(
                SignalSource::NodeOut {
                    node: d_pri,
                    port: 0,
                },
                SignalSink::BoundaryOut(2),
            );
            let d_alt = c.add_node(Primitive::Delay {
                init: width.normalize(init_alt),
            });
            c.connect(
                SignalSource::BoundaryIn(1),
                SignalSink::NodeIn {
                    node: d_alt,
                    port: 0,
                },
            );
            let c1 = materialize_const_loop(&mut c, 1);
            let add_alt = c.add_node(Primitive::Add);
            c.connect(
                SignalSource::NodeOut {
                    node: d_alt,
                    port: 0,
                },
                SignalSink::NodeIn {
                    node: add_alt,
                    port: 0,
                },
            );
            c.connect(
                c1,
                SignalSink::NodeIn {
                    node: add_alt,
                    port: 1,
                },
            );
            let alt_src = SignalSource::NodeOut {
                node: add_alt,
                port: 0,
            };
            let comp = splice_comparator_onto_port(&mut c, 2, alt_src).expect("comp splice");
            let mut ed =
                EditableCircuit::new_with_growth(c, econ, &growth).expect("MV comp circuit");
            ed.register_comparator(comp.sub_node, comp.eqz_node, 2, 1 << 0, 1 << 1)
                .expect("register comp");
            (ed, 2)
        }
    };
    for node_econ in ed.economy_mut().nodes_mut() {
        node_econ.juvenile_ticks = 0;
        node_econ.debt_paid = true;
    }
    (ed, target_port)
}

fn run_conservation_probe_seed(seed: u64) -> ConservationProbeSummary {
    let growth = GrowthConfig {
        redundancy_sprouting: false,
        ..GrowthConfig::default()
    };
    let mut total_probes = 0usize;
    let mut passed_probes = 0usize;
    let mut max_mdl_err = 0.0f64;
    let mut max_de_err = 0.0f64;
    let mut max_exog_err = 0.0f64;

    let d0 = d0_lfsr();
    let d11 = d11_lfsr();

    for family in ["T1", "T2", "T2_MultiViewCoupled"] {
        for width in [Width::BITS_1, Width::BITS_2] {
            for &eps in &[0.0f64, 0.05f64] {
                for &mode in &[InjectionMode::Established, InjectionMode::Juvenile] {
                    let juv_ticks = match mode {
                        InjectionMode::Established => 0,
                        InjectionMode::Juvenile => EconomyConfig::default().juvenile_ticks,
                    };
                    let econ = EconomyConfig {
                        juvenile_ticks: juv_ticks,
                        reserve_cap: ReserveCap::Uncapped,
                        ..EconomyConfig::default()
                    };
                    let gen_cfg = match family {
                        "T1" => GeneratorConfig::T1_PeriodicMotif {
                            motif: if width == Width::BITS_1 {
                                vec![0, 0, 0, 0, 1]
                            } else {
                                vec![0, 1, 3, 2, 1]
                            },
                        },
                        "T2" => GeneratorConfig::T2_DelayedAffine {
                            driver: d0.clone(),
                            lag: 2,
                            mul_a: 1,
                            add_b: 0,
                        },
                        _ => GeneratorConfig::T2_MultiViewCoupled {
                            driver: MultiViewDriver::Lfsr { stage: d11.clone() },
                            view_0: MultiViewTransform {
                                delay: 0,
                                mul_a: 1,
                                add_b: 0,
                            },
                            view_1: MultiViewTransform {
                                delay: 0,
                                mul_a: 1,
                                add_b: width.mask(),
                            },
                            target: MultiViewTarget::Affine {
                                lag: 2,
                                mul_a: 1,
                                add_b: 0,
                            },
                        },
                    };
                    let target_port_hint: u16 = match family {
                        "T1" => 0,
                        "T2" => 1,
                        _ => 2,
                    };
                    for &class in &DetourClass::ALL {
                        total_probes += 1;
                        let sb = SeedBank::new(seed);
                        let generator = gen_cfg.build(width).expect("valid conservation gen");
                        let channels = usize::from(generator.channels());
                        let noise = if eps > 0.0 {
                            NoiseModel::IndependentSubstitution {
                                epsilon: eps,
                                channels: vec![target_port_hint],
                            }
                        } else {
                            NoiseModel::None
                        };
                        let mut env = NoisyEnvironment::new(generator, noise, sb).expect("env");
                        let mut warm0 = vec![0u64; channels];
                        let mut warm1 = vec![0u64; channels];
                        let mut obs = vec![0u64; channels];
                        if family != "T1" {
                            env.step_frame(&mut warm0, &mut obs);
                            env.step_frame(&mut warm1, &mut obs);
                        }
                        let (base_ed, target_port) = build_conservation_base_editable(
                            family,
                            width,
                            econ,
                            growth,
                            warm0[0],
                            *warm0.get(1).unwrap_or(&0),
                        );
                        let mut probe_growth = growth;
                        probe_growth.sprout_interval = 1_000_000;
                        let init_e_sum: f64 =
                            base_ed.economy().nodes().iter().map(|n| n.energy).sum();
                        let init_code_bits =
                            circuit_code_bits_with_tax(base_ed.circuit(), 0, econ.split_code_tax);
                        let mut learner =
                            ArkheLearner::from_editable(base_ed.clone(), probe_growth, sb)
                                .expect("lrn");
                        if family != "T1" {
                            learner.set_prev_observed_frame(&warm1);
                        }
                        let mut budget = TickBudget::new(100_000);
                        let mut clean = vec![0u64; channels];
                        let num_out = usize::from(learner.editable().circuit().num_outputs());

                        let pre_ticks = 24usize;
                        let post_ticks = 160usize;
                        let total_ticks = pre_ticks + post_ticks;
                        let mut facts = vec![Vec::with_capacity(total_ticks); num_out];
                        let mut preds = vec![Vec::with_capacity(total_ticks); num_out];
                        let mut phases = Vec::with_capacity(total_ticks);
                        let has_comp = !learner.editable().economy().comparators().is_empty();
                        let mut comp_pri = Vec::with_capacity(total_ticks);
                        let mut comp_alt = Vec::with_capacity(total_ticks);
                        let comp_masks =
                            learner.editable().economy().comparators().first().map(|c| {
                                (c.target_port, c.primary_channel_mask, c.alt_channel_mask)
                            });

                        let mut total_endowments = init_e_sum;
                        let mut exog_err = 0.0f64;
                        let mut pre_excision_de_err = 0.0f64;
                        let mut checked_pre_excision = false;

                        for t in 0..total_ticks {
                            if t == pre_ticks {
                                let pre_n = learner.editable().circuit().node_count();
                                let pre_bits = circuit_code_bits_with_tax(
                                    learner.editable().circuit(),
                                    0,
                                    econ.split_code_tax,
                                );
                                let edit =
                                    build_detour_edit(learner.editable(), target_port, class)
                                        .expect("cons edit");
                                let post_n = edit.circuit.node_count();
                                let post_bits = circuit_code_bits_with_tax(
                                    &edit.circuit,
                                    0,
                                    econ.split_code_tax,
                                );
                                total_endowments += ((post_n - pre_n) as f64) * econ.initial_energy;
                                let expected_exog = match mode {
                                    InjectionMode::Established => post_bits - pre_bits,
                                    InjectionMode::Juvenile => 0.0,
                                };
                                edit.apply_to_learner(&mut learner, mode).expect("apply");
                                exog_err =
                                    (learner.exogenous_injection_bits() - expected_exog).abs();
                            }

                            env.step_frame(&mut clean, &mut obs);
                            let p = learner.predict(&mut budget);
                            for ch in 0..num_out {
                                if ch == usize::from(target_port) {
                                    preds[ch].push(p[ch]);
                                    facts[ch].push(obs[ch]);
                                } else {
                                    preds[ch].push(0);
                                    facts[ch].push(0);
                                }
                            }
                            phases.push(EpistemicTickPhase::Wake);
                            if has_comp
                                && let Some(comp) =
                                    learner.editable().economy().comparators().first()
                            {
                                let wires = learner.editable().state().wire_values();
                                let sub_in = &learner.editable().compiled().node_input_wires()
                                    [comp.sub_node.index()];
                                comp_pri.push(width.normalize(wires[sub_in[0]]));
                                comp_alt.push(width.normalize(wires[sub_in[1]]));
                            } else {
                                comp_pri.push(0);
                                comp_alt.push(0);
                            }
                            learner.observe(&obs, &mut budget);

                            if t == pre_ticks + 16 {
                                let tel = learner.telemetry();
                                // Charter §5: juveniles are exempt from negative mismatch losses on live
                                // shares (`economy.rs`), so pre-excision ΔE == net_flux holds exactly for
                                // all `Established` probes and all mismatch-free `Juvenile` probes.
                                let juv_mismatch_exempt = mode == InjectionMode::Juvenile
                                    && (eps > 0.0 || class == DetourClass::D2OneFire);
                                if tel.idle_prune_events == 0
                                    && tel.starvation_prune_events == 0
                                    && !juv_mismatch_exempt
                                {
                                    checked_pre_excision = true;
                                    let delta_e = tel.final_total_energy - total_endowments;
                                    if mode == InjectionMode::Established {
                                        let post_inj_bits = circuit_code_bits_with_tax(
                                            learner.editable().circuit(),
                                            0,
                                            econ.split_code_tax,
                                        );
                                        let delta_l_null = post_inj_bits - tel.cumulative_net_flux;
                                        let _ = (init_code_bits, delta_l_null);
                                        // Identity: (Delta E - exogenous_injection_bits) + Delta L_null == init_code_bits (pre-existing paid debt)
                                        // Equivalently, since base circuit paid debt = 0 during this probe and init_code_bits was prepaid:
                                        // (Delta E - exog) == net_flux - (post_inj_bits - init_code_bits)
                                        // => |(Delta E - exog) + (Delta L_null - init_code_bits)| / T == 0.
                                        pre_excision_de_err = ((delta_e
                                            - tel.exogenous_injection_bits)
                                            + (delta_l_null - init_code_bits))
                                            .abs()
                                            / ((t + 1) as f64);
                                    } else {
                                        pre_excision_de_err = ((delta_e
                                            + tel.cumulative_maturity_debt_paid)
                                            - tel.cumulative_net_flux)
                                            .abs()
                                            / ((t + 1) as f64);
                                    }
                                }
                            }
                        }

                        let tel = learner.telemetry();
                        let fact_slices: Vec<&[u64]> = facts.iter().map(Vec::as_slice).collect();
                        let pred_slices: Vec<&[u64]> = preds.iter().map(Vec::as_slice).collect();
                        let comp_traces: Vec<EpistemicComparatorTrace<'_>> =
                            if let Some((tp, pm, am)) = comp_masks {
                                if learner.active_comparator_count() > 0 {
                                    vec![EpistemicComparatorTrace {
                                        target_port: usize::from(tp),
                                        registered_at_tick: 0,
                                        primary_channel_mask: pm,
                                        alt_channel_mask: am,
                                        primary_predictions: &comp_pri,
                                        alt_predictions: &comp_alt,
                                    }]
                                } else {
                                    Vec::new()
                                }
                            } else {
                                Vec::new()
                            };
                        let ref_mdl = epistemic_null_calibrated_mdl_delta_with_tax(
                            learner.editable().circuit(),
                            0.0,
                            0,
                            &fact_slices,
                            &pred_slices,
                            &phases,
                            &comp_traces,
                            econ.null_model,
                            econ.split_code_tax,
                        )
                        .expect("ref epistemic mdl");
                        let mdl_err = (tel.null_calibrated_mdl_delta_bits - ref_mdl).abs()
                            / (total_ticks as f64);
                        max_mdl_err = max_mdl_err.max(mdl_err);
                        if checked_pre_excision {
                            max_de_err = max_de_err.max(pre_excision_de_err);
                        }
                        max_exog_err = max_exog_err.max(exog_err);
                        if mdl_err <= CONSERVATION_TOL
                            && (!checked_pre_excision || pre_excision_de_err <= CONSERVATION_TOL)
                            && exog_err <= CONSERVATION_TOL
                        {
                            passed_probes += 1;
                        }
                    }
                }
            }
        }
    }

    ConservationProbeSummary {
        total_probes,
        passed_probes,
        max_epistemic_mdl_residual_b_per_sym: max_mdl_err,
        max_pre_excision_delta_e_residual_b_per_sym: max_de_err,
        max_exogenous_bits_err: max_exog_err,
    }
}

#[derive(Clone, Copy)]
struct InjJobMeta<'a> {
    sweep_tag: &'a str,
    task_idx: usize,
    wire_delay: WireDelayMode,
    epsilon: f64,
    split_code_tax: bool,
    excision_mode: ExcisionMode,
    injection_mode: InjectionMode,
    detour_class: DetourClass,
}

fn build_comb_main_jobs(tasks: &[TaskSpec]) -> Vec<InjJobMeta<'_>> {
    let mut jobs = Vec::new();
    for (ti, _) in tasks.iter().enumerate() {
        for &eps in &[0.0, 0.05] {
            for &inj_mode in &[InjectionMode::Established, InjectionMode::Juvenile] {
                for &class in &DetourClass::ALL {
                    jobs.push(InjJobMeta {
                        sweep_tag: "comb_vle",
                        task_idx: ti,
                        wire_delay: WireDelayMode::Combinational,
                        epsilon: eps,
                        split_code_tax: true,
                        excision_mode: ExcisionMode::VerifiedLocalExcision,
                        injection_mode: inj_mode,
                        detour_class: class,
                    });
                }
            }
        }
    }
    jobs
}

fn build_comb_baseline_jobs(tasks: &[TaskSpec]) -> Vec<InjJobMeta<'_>> {
    let mut jobs = Vec::new();
    for (ti, _) in tasks.iter().enumerate() {
        for &inj_mode in &[InjectionMode::Established, InjectionMode::Juvenile] {
            for &class in &DetourClass::ALL {
                jobs.push(InjJobMeta {
                    sweep_tag: "comb_baseline_d1b1",
                    task_idx: ti,
                    wire_delay: WireDelayMode::Combinational,
                    epsilon: 0.0,
                    split_code_tax: true,
                    excision_mode: ExcisionMode::BaselineD1b1,
                    injection_mode: inj_mode,
                    detour_class: class,
                });
            }
        }
    }
    jobs
}

fn build_tph_jobs(tasks: &[TaskSpec]) -> Vec<InjJobMeta<'_>> {
    let mut jobs = Vec::new();
    for (ti, task) in tasks.iter().enumerate() {
        if task.tph_cfg.is_none() {
            continue;
        }
        let classes: &[DetourClass] = if task.is_autonomous_t1 {
            &DetourClass::ALL
        } else {
            &[DetourClass::D0Dead, DetourClass::D7aFakeComparator]
        };
        for &inj_mode in &[InjectionMode::Established, InjectionMode::Juvenile] {
            for &class in classes {
                jobs.push(InjJobMeta {
                    sweep_tag: "tph_vle",
                    task_idx: ti,
                    wire_delay: WireDelayMode::TicksPerHop,
                    epsilon: 0.0,
                    split_code_tax: true,
                    excision_mode: ExcisionMode::VerifiedLocalExcision,
                    injection_mode: inj_mode,
                    detour_class: class,
                });
            }
        }
    }
    jobs
}

fn build_split_false_inj_jobs(tasks: &[TaskSpec]) -> Vec<InjJobMeta<'_>> {
    let split_classes = [
        DetourClass::D0Dead,
        DetourClass::D1AddZero,
        DetourClass::D4MulOne,
        DetourClass::D5CancelSub,
        DetourClass::D7bAgreementGate,
    ];
    let mut jobs = Vec::new();
    for (ti, task) in tasks.iter().enumerate() {
        if !task.split_tax_ablate_inj {
            continue;
        }
        for &inj_mode in &[InjectionMode::Established, InjectionMode::Juvenile] {
            for &class in &split_classes {
                jobs.push(InjJobMeta {
                    sweep_tag: "comb_split_tax_false",
                    task_idx: ti,
                    wire_delay: WireDelayMode::Combinational,
                    epsilon: 0.0,
                    split_code_tax: false,
                    excision_mode: ExcisionMode::VerifiedLocalExcision,
                    injection_mode: inj_mode,
                    detour_class: class,
                });
            }
        }
    }
    jobs
}

/// Executes the Milestone 3 exploratory and paired-ablation sweep on `[seed_offset, seed_offset + seeds)`.
///
/// # Errors
///
/// Returns an error if the requested seed range overlaps any milestone holdout bank.
pub fn run_explore_m3(seed_offset: u64, seeds: usize) -> Result<ExploreM3Report, String> {
    check_explore_m3_seed_range_holdout(seed_offset, seeds)?;

    let seeds_tested = seeds.max(1);
    let tasks = injection_task_panel();
    let spont_specs = spontaneous_panel();
    let comb_main_jobs = build_comb_main_jobs(&tasks);
    let comb_base_jobs = build_comb_baseline_jobs(&tasks);
    let tph_jobs = build_tph_jobs(&tasks);
    let split_false_jobs = build_split_false_inj_jobs(&tasks);

    // Precompute certified exact-synthesis minima once per unique `(gen_cfg, width, wire_delay)`.
    let mut n_min_cache: std::collections::BTreeMap<(String, u8, u8), usize> =
        std::collections::BTreeMap::new();
    let mut unique_specs: Vec<(usize, &SpontaneousSpec)> = Vec::new();
    let mut spec_to_unique = Vec::with_capacity(spont_specs.len());
    for spec in &spont_specs {
        let wd_tag = u8::from(spec.wire_delay == WireDelayMode::TicksPerHop);
        let key = (format!("{:?}", spec.gen_cfg), spec.width.bits(), wd_tag);
        let u_idx = if let Some(&existing) = n_min_cache.get(&key) {
            existing
        } else {
            let next = unique_specs.len();
            unique_specs.push((next, spec));
            n_min_cache.insert(key, next);
            next
        };
        spec_to_unique.push(u_idx);
    }

    let max_threads = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1)
        .max(1);

    type ExactMinEntry = (Option<usize>, String);
    let unique_n_mins: Vec<Mutex<Option<ExactMinEntry>>> =
        (0..unique_specs.len()).map(|_| Mutex::new(None)).collect();
    let next_nmin = AtomicUsize::new(0);
    std::thread::scope(|s| {
        let t_count = max_threads.min(unique_specs.len()).max(1);
        for _ in 0..t_count {
            let next_ref = &next_nmin;
            let unique_ref = &unique_specs;
            let out_ref = &unique_n_mins;
            s.spawn(move || {
                loop {
                    let i = next_ref.fetch_add(1, Ordering::Relaxed);
                    if i >= unique_ref.len() {
                        break;
                    }
                    let res = compute_n_min_for_spontaneous(unique_ref[i].1);
                    *out_ref[i].lock().expect("nmin lock") = Some(res);
                }
            });
        }
    });
    let unique_n_mins_done: Vec<(Option<usize>, String)> = unique_n_mins
        .into_iter()
        .map(|m| m.into_inner().expect("nmin lock").expect("nmin computed"))
        .collect();
    let n_mins: Vec<(Option<usize>, String)> = spec_to_unique
        .into_iter()
        .map(|u| unique_n_mins_done[u].clone())
        .collect();

    #[derive(Clone, Copy)]
    enum WorkItem {
        CombMain {
            seed_idx: usize,
            job_idx: usize,
        },
        CombBase {
            seed_idx: usize,
            job_idx: usize,
        },
        Tph {
            seed_idx: usize,
            job_idx: usize,
        },
        SplitFalse {
            seed_idx: usize,
            job_idx: usize,
        },
        Spont {
            seed_idx: usize,
            spec_idx: usize,
            tax_idx: usize,
        },
        Conservation {
            seed_idx: usize,
        },
    }

    let mut work_items = Vec::new();
    for seed_idx in 0..seeds_tested {
        for job_idx in 0..comb_main_jobs.len() {
            work_items.push(WorkItem::CombMain { seed_idx, job_idx });
        }
        for job_idx in 0..comb_base_jobs.len() {
            work_items.push(WorkItem::CombBase { seed_idx, job_idx });
        }
        for job_idx in 0..tph_jobs.len() {
            work_items.push(WorkItem::Tph { seed_idx, job_idx });
        }
        for job_idx in 0..split_false_jobs.len() {
            work_items.push(WorkItem::SplitFalse { seed_idx, job_idx });
        }
        for spec_idx in 0..spont_specs.len() {
            for tax_idx in 0..2 {
                work_items.push(WorkItem::Spont {
                    seed_idx,
                    spec_idx,
                    tax_idx,
                });
            }
        }
        work_items.push(WorkItem::Conservation { seed_idx });
    }

    let comb_main_slots: Vec<Vec<Mutex<Option<InjectionRunOutcome>>>> = (0..comb_main_jobs.len())
        .map(|_| (0..seeds_tested).map(|_| Mutex::new(None)).collect())
        .collect();
    let comb_base_slots: Vec<Vec<Mutex<Option<InjectionRunOutcome>>>> = (0..comb_base_jobs.len())
        .map(|_| (0..seeds_tested).map(|_| Mutex::new(None)).collect())
        .collect();
    let tph_slots: Vec<Vec<Mutex<Option<InjectionRunOutcome>>>> = (0..tph_jobs.len())
        .map(|_| (0..seeds_tested).map(|_| Mutex::new(None)).collect())
        .collect();
    let split_false_slots: Vec<Vec<Mutex<Option<InjectionRunOutcome>>>> = (0..split_false_jobs
        .len())
        .map(|_| (0..seeds_tested).map(|_| Mutex::new(None)).collect())
        .collect();
    let spont_slots: Vec<Vec<Mutex<Option<SpontaneousRunOutcome>>>> = (0..spont_specs.len() * 2)
        .map(|_| (0..seeds_tested).map(|_| Mutex::new(None)).collect())
        .collect();
    let cons_slots: Vec<Mutex<Option<ConservationProbeSummary>>> =
        (0..seeds_tested).map(|_| Mutex::new(None)).collect();

    let next_item = AtomicUsize::new(0);
    let num_threads = max_threads.min(work_items.len()).max(1);
    std::thread::scope(|s| {
        for _ in 0..num_threads {
            let next_ref = &next_item;
            let items_ref = &work_items;
            let tasks_ref = &tasks;
            let comb_main_ref = &comb_main_jobs;
            let comb_base_ref = &comb_base_jobs;
            let tph_ref = &tph_jobs;
            let split_false_ref = &split_false_jobs;
            let spont_ref = &spont_specs;
            let cm_slots_ref = &comb_main_slots;
            let cb_slots_ref = &comb_base_slots;
            let tph_slots_ref = &tph_slots;
            let sf_slots_ref = &split_false_slots;
            let sp_slots_ref = &spont_slots;
            let cs_slots_ref = &cons_slots;
            s.spawn(move || {
                loop {
                    let idx = next_ref.fetch_add(1, Ordering::Relaxed);
                    if idx >= items_ref.len() {
                        break;
                    }
                    match items_ref[idx] {
                        WorkItem::CombMain { seed_idx, job_idx } => {
                            let j = &comb_main_ref[job_idx];
                            let seed = seed_offset.saturating_add(seed_idx as u64);
                            let out = run_injection_case(
                                &tasks_ref[j.task_idx],
                                j.wire_delay,
                                j.epsilon,
                                j.split_code_tax,
                                j.excision_mode,
                                j.injection_mode,
                                j.detour_class,
                                seed,
                            );
                            *cm_slots_ref[job_idx][seed_idx].lock().expect("cm lock") = Some(out);
                        }
                        WorkItem::CombBase { seed_idx, job_idx } => {
                            let j = &comb_base_ref[job_idx];
                            let seed = seed_offset.saturating_add(seed_idx as u64);
                            let out = run_injection_case(
                                &tasks_ref[j.task_idx],
                                j.wire_delay,
                                j.epsilon,
                                j.split_code_tax,
                                j.excision_mode,
                                j.injection_mode,
                                j.detour_class,
                                seed,
                            );
                            *cb_slots_ref[job_idx][seed_idx].lock().expect("cb lock") = Some(out);
                        }
                        WorkItem::Tph { seed_idx, job_idx } => {
                            let j = &tph_ref[job_idx];
                            let seed = seed_offset.saturating_add(seed_idx as u64);
                            let out = run_injection_case(
                                &tasks_ref[j.task_idx],
                                j.wire_delay,
                                j.epsilon,
                                j.split_code_tax,
                                j.excision_mode,
                                j.injection_mode,
                                j.detour_class,
                                seed,
                            );
                            *tph_slots_ref[job_idx][seed_idx].lock().expect("tph lock") = Some(out);
                        }
                        WorkItem::SplitFalse { seed_idx, job_idx } => {
                            let j = &split_false_ref[job_idx];
                            let seed = seed_offset.saturating_add(seed_idx as u64);
                            let out = run_injection_case(
                                &tasks_ref[j.task_idx],
                                j.wire_delay,
                                j.epsilon,
                                j.split_code_tax,
                                j.excision_mode,
                                j.injection_mode,
                                j.detour_class,
                                seed,
                            );
                            *sf_slots_ref[job_idx][seed_idx].lock().expect("sf lock") = Some(out);
                        }
                        WorkItem::Spont {
                            seed_idx,
                            spec_idx,
                            tax_idx,
                        } => {
                            let seed = seed_offset.saturating_add(seed_idx as u64);
                            let split_tax = tax_idx == 0;
                            let out = run_spontaneous_case(&spont_ref[spec_idx], split_tax, seed);
                            *sp_slots_ref[spec_idx * 2 + tax_idx][seed_idx]
                                .lock()
                                .expect("sp lock") = Some(out);
                        }
                        WorkItem::Conservation { seed_idx } => {
                            let seed = seed_offset.saturating_add(seed_idx as u64);
                            let out = run_conservation_probe_seed(seed);
                            *cs_slots_ref[seed_idx].lock().expect("cs lock") = Some(out);
                        }
                    }
                }
            });
        }
    });

    let unwrap_inj =
        |slots: Vec<Vec<Mutex<Option<InjectionRunOutcome>>>>| -> Vec<Vec<InjectionRunOutcome>> {
            slots
                .into_iter()
                .map(|row| {
                    row.into_iter()
                        .map(|m| m.into_inner().expect("inj lock").expect("inj computed"))
                        .collect()
                })
                .collect()
        };
    let comb_main_by_job = unwrap_inj(comb_main_slots);
    let comb_base_by_job = unwrap_inj(comb_base_slots);
    let tph_by_job = unwrap_inj(tph_slots);
    let split_false_by_job = unwrap_inj(split_false_slots);
    let spont_by_job: Vec<Vec<SpontaneousRunOutcome>> = spont_slots
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|m| m.into_inner().expect("sp lock").expect("sp computed"))
                .collect()
        })
        .collect();

    let mut total_cons_probes = 0usize;
    let mut passed_cons_probes = 0usize;
    let mut max_mdl_res = 0.0f64;
    let mut max_de_res = 0.0f64;
    let mut max_exog_err = 0.0f64;

    for slot in cons_slots {
        let c = slot
            .into_inner()
            .expect("cs lock")
            .expect("conservation computed");
        total_cons_probes += c.total_probes;
        passed_cons_probes += c.passed_probes;
        max_mdl_res = max_mdl_res.max(c.max_epistemic_mdl_residual_b_per_sym);
        max_de_res = max_de_res.max(c.max_pre_excision_delta_e_residual_b_per_sym);
        max_exog_err = max_exog_err.max(c.max_exogenous_bits_err);
    }

    let mut injection_cells = Vec::new();
    let mut total_cliff_counters = CliffCountersSummary::default();

    let summarize_group = |jobs: &[InjJobMeta<'_>],
                           outcomes_by_job: &[Vec<InjectionRunOutcome>],
                           out_cells: &mut Vec<InjectionCellSummary>,
                           cliff_acc: &mut CliffCountersSummary| {
        for (job, outcomes) in jobs.iter().zip(outcomes_by_job.iter()) {
            let summary = summarize_injection_cell(
                job.sweep_tag,
                &tasks[job.task_idx],
                job.wire_delay,
                job.epsilon,
                job.split_code_tax,
                job.excision_mode,
                job.injection_mode,
                job.detour_class,
                outcomes,
            );
            cliff_acc.accumulate(&summary.cliff_counters);
            out_cells.push(summary);
        }
    };

    summarize_group(
        &comb_main_jobs,
        &comb_main_by_job,
        &mut injection_cells,
        &mut total_cliff_counters,
    );
    summarize_group(
        &comb_base_jobs,
        &comb_base_by_job,
        &mut injection_cells,
        &mut total_cliff_counters,
    );
    summarize_group(
        &tph_jobs,
        &tph_by_job,
        &mut injection_cells,
        &mut total_cliff_counters,
    );
    summarize_group(
        &split_false_jobs,
        &split_false_by_job,
        &mut injection_cells,
        &mut total_cliff_counters,
    );

    // Paired BaselineD1b1 vs VerifiedLocalExcision ablation table (clean Combinational, split_code_tax=true).
    let mut paired_excision_ablation = Vec::new();
    for (base_idx, base_job) in comb_base_jobs.iter().enumerate() {
        let Some(vle_idx) = comb_main_jobs.iter().position(|j| {
            j.task_idx == base_job.task_idx
                && j.epsilon == 0.0
                && j.injection_mode == base_job.injection_mode
                && j.detour_class == base_job.detour_class
        }) else {
            continue;
        };
        let vle_outs = &comb_main_by_job[vle_idx];
        let base_outs = &comb_base_by_job[base_idx];
        let mut vle_excised = 0usize;
        let mut base_excised = 0usize;
        let mut base_collapsed = 0usize;
        let mut base_survived = 0usize;
        let mut vle_lower_mdl = 0usize;
        let mut mdl_diff_sum = 0.0f64;
        let mut vle_dts = Vec::new();
        let mut base_dts = Vec::new();
        let mut paired_runs = 0usize;

        for (v, b) in vle_outs.iter().zip(base_outs.iter()) {
            if !v.settled || !b.settled {
                continue;
            }
            paired_runs += 1;
            if v.excised_by_end {
                vle_excised += 1;
            }
            if b.excised_by_end {
                base_excised += 1;
            }
            if b.collapsed {
                base_collapsed += 1;
            }
            if b.survived_bloated {
                base_survived += 1;
            }
            if v.final_total_code_bits + 1e-9 < b.final_total_code_bits {
                vle_lower_mdl += 1;
            }
            mdl_diff_sum += b.final_total_code_bits - v.final_total_code_bits;
            if let Some(dt) = v.excise_dt {
                vle_dts.push(dt);
            }
            if let Some(dt) = b.excise_dt {
                base_dts.push(dt);
            }
        }
        let task = &tasks[base_job.task_idx];
        paired_excision_ablation.push(PairedExcisionAblationRow {
            task: task.label.to_owned(),
            width_bits: task.width.bits(),
            injection_mode: base_job.injection_mode,
            detour_class: base_job.detour_class,
            paired_runs,
            vle_excised_count: vle_excised,
            base_excised_count: base_excised,
            base_collapsed_count: base_collapsed,
            base_survived_count: base_survived,
            vle_strictly_lower_mdl_count: vle_lower_mdl,
            mean_mdl_delta_base_minus_vle: mdl_diff_sum / (paired_runs.max(1) as f64),
            vle_mean_excise_dt: (!vle_dts.is_empty())
                .then(|| (vle_dts.iter().sum::<u64>() as f64) / (vle_dts.len() as f64)),
            base_mean_excise_dt: (!base_dts.is_empty())
                .then(|| (base_dts.iter().sum::<u64>() as f64) / (base_dts.len() as f64)),
        });
    }

    // Paired split_code_tax injection ablation table.
    let mut paired_split_tax_injection_ablation = Vec::new();
    for (sf_idx, sf_job) in split_false_jobs.iter().enumerate() {
        let Some(st_idx) = comb_main_jobs.iter().position(|j| {
            j.task_idx == sf_job.task_idx
                && j.epsilon == 0.0
                && j.injection_mode == sf_job.injection_mode
                && j.detour_class == sf_job.detour_class
        }) else {
            continue;
        };
        let st_outs = &comb_main_by_job[st_idx];
        let sf_outs = &split_false_by_job[sf_idx];
        let mut paired_runs = 0usize;
        let mut l_pre_t = 0.0f64;
        let mut l_pre_f = 0.0f64;
        let mut dl_t = 0.0f64;
        let mut dl_f = 0.0f64;
        let mut exc_t = 0usize;
        let mut exc_f = 0usize;
        let mut dts_t = Vec::new();
        let mut dts_f = Vec::new();
        for (st, sf) in st_outs.iter().zip(sf_outs.iter()) {
            if !st.settled || !sf.settled {
                continue;
            }
            paired_runs += 1;
            l_pre_t += st.pre_inj_primary_code_bits;
            l_pre_f += sf.pre_inj_primary_code_bits;
            dl_t += st.final_primary_code_bits - st.pre_inj_primary_code_bits;
            dl_f += sf.final_primary_code_bits - sf.pre_inj_primary_code_bits;
            if st.excised_by_end {
                exc_t += 1;
            }
            if sf.excised_by_end {
                exc_f += 1;
            }
            if let Some(dt) = st.excise_dt {
                dts_t.push(dt);
            }
            if let Some(dt) = sf.excise_dt {
                dts_f.push(dt);
            }
        }
        let denom = paired_runs.max(1) as f64;
        let task = &tasks[sf_job.task_idx];
        paired_split_tax_injection_ablation.push(PairedSplitTaxInjectionRow {
            task: task.label.to_owned(),
            width_bits: task.width.bits(),
            injection_mode: sf_job.injection_mode,
            detour_class: sf_job.detour_class,
            paired_runs,
            mean_l_pre_tax_true: l_pre_t / denom,
            mean_l_pre_tax_false: l_pre_f / denom,
            mean_delta_l_inj_tax_true: dl_t / denom,
            mean_delta_l_inj_tax_false: dl_f / denom,
            excised_tax_true: exc_t,
            excised_tax_false: exc_f,
            mean_excise_dt_tax_true: (!dts_t.is_empty())
                .then(|| (dts_t.iter().sum::<u64>() as f64) / (dts_t.len() as f64)),
            mean_excise_dt_tax_false: (!dts_f.is_empty())
                .then(|| (dts_f.iter().sum::<u64>() as f64) / (dts_f.len() as f64)),
        });
    }

    // Spontaneous summaries + paired split_code_tax spontaneous ablation.
    let mut spontaneous_cells = Vec::with_capacity(spont_specs.len() * 2);
    let mut paired_split_tax_spontaneous_ablation = Vec::with_capacity(spont_specs.len());

    for (si, spec) in spont_specs.iter().enumerate() {
        let (n_min, exact_status) = &n_mins[si];
        let outs_true = &spont_by_job[si * 2];
        let outs_false = &spont_by_job[si * 2 + 1];

        let cell_true = summarize_spontaneous_cell(spec, true, *n_min, exact_status, outs_true);
        let cell_false = summarize_spontaneous_cell(spec, false, *n_min, exact_status, outs_false);

        total_cliff_counters.accumulate(&cell_true.cliff_counters);
        total_cliff_counters.accumulate(&cell_false.cliff_counters);

        let paired_runs = outs_true.len().min(outs_false.len());
        let denom = paired_runs.max(1) as f64;
        let bits_t: f64 = outs_true.iter().map(|r| r.final_code_bits).sum();
        let bits_f: f64 = outs_false.iter().map(|r| r.final_code_bits).sum();
        let splits_t: f64 = outs_true.iter().map(|r| r.final_split_count as f64).sum();
        let splits_f: f64 = outs_false.iter().map(|r| r.final_split_count as f64).sum();
        let np_t: f64 = outs_true.iter().map(|r| r.n_primary_8000 as f64).sum();
        let np_f: f64 = outs_false.iter().map(|r| r.n_primary_8000 as f64).sum();

        paired_split_tax_spontaneous_ablation.push(PairedSplitTaxSpontaneousRow {
            task: spec.label.to_owned(),
            width_bits: spec.width.bits(),
            wire_delay: spec.wire_delay,
            epsilon: spec.epsilon,
            paired_runs,
            mean_code_bits_tax_true: bits_t / denom,
            mean_code_bits_tax_false: bits_f / denom,
            mean_mdl_tax_delta: (bits_t - bits_f) / denom,
            mean_splits_tax_true: splits_t / denom,
            mean_splits_tax_false: splits_f / denom,
            mean_n_primary_tax_true: np_t / denom,
            mean_n_primary_tax_false: np_f / denom,
            clean_locked_tax_true: cell_true.clean_locked_8000_runs,
            clean_locked_tax_false: cell_false.clean_locked_8000_runs,
        });

        spontaneous_cells.push(cell_true);
        spontaneous_cells.push(cell_false);
    }

    Ok(ExploreM3Report {
        provenance: BuildProvenance::current(),
        seed_offset,
        seeds_tested,
        injection_cells,
        paired_excision_ablation,
        paired_split_tax_injection_ablation,
        spontaneous_cells,
        paired_split_tax_spontaneous_ablation,
        conservation_summary: ConservationProbeSummary {
            total_probes: total_cons_probes,
            passed_probes: passed_cons_probes,
            max_epistemic_mdl_residual_b_per_sym: max_mdl_res,
            max_pre_excision_delta_e_residual_b_per_sym: max_de_res,
            max_exogenous_bits_err: max_exog_err,
        },
        total_cliff_counters,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holdout_guards_reject_all_five_milestone_banks() {
        // M1 (10_000..=10_019), M2a (30_000..=30_019), M2b (40_000..=40_019),
        // M3 (50_000..=50_019), M4a (60_000..=60_019) must all be unconditionally rejected.
        for holdout_start in [10_000u64, 30_000, 40_000, 50_000, 60_000] {
            // Exact bank start.
            assert!(
                check_explore_m3_seed_range_holdout(holdout_start, 20).is_err(),
                "expected rejection at {holdout_start}..+20"
            );
            // Single-seed hit inside bank.
            assert!(
                check_explore_m3_seed_range_holdout(holdout_start + 19, 1).is_err(),
                "expected rejection at tail {holdout_start}+19"
            );
            // Partial overlap from below.
            assert!(
                check_explore_m3_seed_range_holdout(holdout_start - 5, 10).is_err(),
                "expected rejection on partial overlap below {holdout_start}"
            );
            // Zero-seed request at holdout start still rejected (`seeds.max(1)`).
            assert!(
                check_explore_m3_seed_range_holdout(holdout_start, 0).is_err(),
                "expected rejection on seeds=0 at {holdout_start}"
            );
        }

        // Safe disjoint ranges must be accepted.
        for safe in [
            0u64, 9_980, 10_020, 20_060, 29_980, 30_020, 39_980, 40_020, 49_980, 50_020, 59_980,
            60_020,
        ] {
            assert!(
                check_explore_m3_seed_range_holdout(safe, 20).is_ok(),
                "expected safe seed range at {safe}..+20"
            );
        }
    }
}
