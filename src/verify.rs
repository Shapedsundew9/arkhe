//! Pre-registered Milestone 0 acceptance verification battery (Gates G1–G4, Charter §7 & §8).

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use rand_core::Rng;
use serde::{Deserialize, Serialize};

use crate::baselines::{
    SynthesisCertStatus, build_z4_motif_013_reactive_one_reg_witness, certify_circuit_against_spec,
    exact_synthesize_certified_circuit, sample_random_valid_circuit,
};
use crate::config::{
    BaselineConfig, BudgetConfig, BuildProvenance, CONFIG_SCHEMA_VERSION, EnvironmentConfig,
    ExperimentConfig, RunReport, SubstrateConfig, execute_experiment,
};
use crate::env::{
    FiniteStateGenerator, GeneratorConfig, NoiseModel, ReedsSloaneSynthesizer,
    declared_noise_entropy_bits_per_symbol, solve_epsilon_machine,
};
use crate::rng::{SeedBank, StreamDomain};
use crate::structural::WiringTopology;
use crate::substrate::{
    BitParallel64State, Circuit, EvalError, InvariantError, NodeId, Primitive, ScalarState,
    SignalSink, SignalSource, Width, WidthError, WireDelayMode,
};

static TMP_NONCE: AtomicU64 = AtomicU64::new(0);

/// Per-condition empirical summary recorded by Gate `G3-rev1`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GateCaseSummary {
    /// Width $k$ in bits.
    pub width_bits: u8,
    /// Substitution noise rate $\varepsilon$.
    pub epsilon: f64,
    /// Benchmark generator label.
    pub generator: String,
    /// Baseline predictor name.
    pub baseline: String,
    /// Whether this condition is an enforced pass/fail gate (`true`) or recorded characterization (`false`).
    pub gate_enforced: bool,
    /// Whether this condition satisfied its criterion (always `true` when `gate_enforced == false`).
    pub passed: bool,
    /// Number of seeds meeting the per-seed criterion.
    pub seeds_passed: u32,
    /// Total seeds evaluated in `[seed_offset, seed_offset + seeds_total)`.
    pub seeds_total: u32,
    /// Mean excess loss over analytical $h_\mu$ across evaluated seeds (`None` if $h_\mu$ unavailable).
    pub mean_excess_over_h_mu: Option<f64>,
    /// Mean paired oracle regret across evaluated seeds (`None` on noiseless deterministic checks).
    pub mean_paired_regret: Option<f64>,
    /// Worst-case (maximum) single-seed paired oracle regret across evaluated seeds.
    pub max_paired_regret: Option<f64>,
}

/// Summary of the Milestone 0 pre-registered acceptance suite (`arkhe verify-m0`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M0VerificationSummary {
    /// Compile-time binary, toolchain, and Git/lockfile provenance.
    pub provenance: BuildProvenance,
    /// Starting master seed offset (`seed_offset .. seed_offset + seeds_tested`).
    pub seed_offset: u32,
    /// Number of master seeds tested per stochastic condition.
    pub seeds_tested: u32,
    /// Gate G1: Substrate arithmetic ($k \in \{1,2,4,8,16,32,64\}$), §2.5 negative rejections & 64-lane equivalence.
    pub gate_g1_substrate_passed: bool,
    /// Gate G2: Analytical $\varepsilon$-machine (`T1`, `T3b`, `T3c`), Reeds–Sloane $2L$, & certified synthesis checks.
    pub gate_g2_yardsticks_passed: bool,
    /// Gate G3-rev1: T1 baselines at $k = 1$ and $k = 2$ (noiseless & $\varepsilon = 0.05$ noisy).
    pub gate_g3_t1_baselines_passed: bool,
    /// Gate G4: Cross-process CLI subprocess byte-for-byte JSON determinism across 6-config matrix + golden RNG vectors.
    pub gate_g4_reproducibility_passed: bool,
    /// Per-condition empirical breakdown across Gate `G3-rev1`.
    pub case_reports: Vec<GateCaseSummary>,
    /// Detailed failure diagnostics (empty when all gates pass).
    pub failures: Vec<String>,
}

impl M0VerificationSummary {
    /// Returns `true` iff all four pre-registered Milestone 0 gates passed.
    #[must_use]
    pub fn all_passed(&self) -> bool {
        self.gate_g1_substrate_passed
            && self.gate_g2_yardsticks_passed
            && self.gate_g3_t1_baselines_passed
            && self.gate_g4_reproducibility_passed
            && self.failures.is_empty()
    }
}

/// Runs all four pre-registered Milestone 0 gates across `0..num_seeds` (`seed_offset = 0`).
#[must_use]
pub fn run_m0_verification_suite(num_seeds: u32) -> M0VerificationSummary {
    run_m0_verification_suite_with_offset(0, num_seeds)
}

/// Runs all four pre-registered Milestone 0 gates across `seed_offset .. seed_offset + num_seeds`.
#[must_use]
pub fn run_m0_verification_suite_with_offset(
    seed_offset: u32,
    num_seeds: u32,
) -> M0VerificationSummary {
    let seeds_count = num_seeds.max(1);
    let mut failures = Vec::new();
    let mut case_reports = Vec::new();

    let g1 = verify_gate_g1_substrate(&mut failures);
    let g2 = verify_gate_g2_yardsticks(&mut failures);
    let g3 = verify_gate_g3_t1_optima(seed_offset, seeds_count, &mut case_reports, &mut failures);
    let g4 = verify_gate_g4_reproducibility(&mut failures);

    M0VerificationSummary {
        provenance: BuildProvenance::current(),
        seed_offset,
        seeds_tested: seeds_count,
        gate_g1_substrate_passed: g1,
        gate_g2_yardsticks_passed: g2,
        gate_g3_t1_baselines_passed: g3,
        gate_g4_reproducibility_passed: g4,
        case_reports,
        failures,
    }
}

/// Gate G1: Ring identities across $k \in \{1, 2, 4, 8, 16, 32, 64\}$, complete negative §2.5
/// `InvariantError` + `WidthError` + `EvalError` rejection matrix, and 100 random circuits × 1,000
/// ticks comparing `BitParallel64State` against 64 `ScalarState` lanes.
fn verify_gate_g1_substrate(failures: &mut Vec<String>) -> bool {
    let start_len = failures.len();

    // 1. Width bounds rejection checks.
    if Width::new(0) != Err(WidthError(0)) {
        failures.push("G1: Width::new(0) was not rejected".to_owned());
    }
    if Width::new(65) != Err(WidthError(65)) {
        failures.push("G1: Width::new(65) was not rejected".to_owned());
    }

    // 2. Modular ring identities across all 7 Charter widths.
    for k in [1u8, 2, 4, 8, 16, 32, 64] {
        let w = Width::new(k).expect("valid width");
        let a = 0xDEAD_BEEF_CAFE_BABEu64 & w.mask();
        let b = 0x0123_4567_89AB_CDEFu64 & w.mask();
        let c = 0xFEDC_BA98_7654_3210u64 & w.mask();
        if w.add(w.add(a, b), c) != w.add(a, w.add(b, c)) || w.add(a, b) != w.add(b, a) {
            failures.push(format!("G1: ADD ring identity failed at k={k}"));
        }
        if w.sub(w.add(a, b), b) != a || w.sub(a, a) != 0 {
            failures.push(format!("G1: ADD/SUB inverse failed at k={k}"));
        }
        if w.mul(w.mul(a, b), c) != w.mul(a, w.mul(b, c))
            || w.mul(a, 1) != a
            || w.mul(a, w.add(b, c)) != w.add(w.mul(a, b), w.mul(a, c))
        {
            failures.push(format!("G1: MUL ring/distributivity failed at k={k}"));
        }
        if w.eqz(0) != 1 || w.eqz(1) != 0 || w.comparator(a, a) != 1 {
            failures.push(format!("G1: EQZ/comparator identity failed at k={k}"));
        }
    }

    // 3. Complete negative §2.5 InvariantError + EvalError rejection matrix.
    verify_g1_negative_invariants(failures);

    // 4. 100 random valid circuits × 1,000 ticks: 64-lane bit-parallel == 64 scalar lanes.
    let mut rng = SeedBank::new(0x61_61_61_61).stream(StreamDomain::BaselineSearch { trial: 99 });
    for idx in 0..100 {
        let circ = sample_random_valid_circuit(
            &mut rng,
            Width::BITS_1,
            2,
            2,
            WireDelayMode::Combinational,
            10,
        );
        let compiled = match circ.compile() {
            Ok(c) => c,
            Err(e) => {
                failures.push(format!(
                    "G1: random valid circuit #{idx} failed compile: {e}"
                ));
                continue;
            }
        };

        let num_regs = compiled.total_registers();
        let mut packed_init = vec![0u64; num_regs];
        for r in &mut packed_init {
            *r = rng.next_u64();
        }

        let mut bp = BitParallel64State::new(&compiled).expect("k == 1");
        bp.set_packed_registers(&packed_init).unwrap();

        let mut scalars: Vec<ScalarState> = (0..64)
            .map(|lane| {
                let mut st = ScalarState::new(&compiled);
                let lane_regs: Vec<u64> = packed_init.iter().map(|&w| (w >> lane) & 1).collect();
                st.set_registers(&compiled, &lane_regs).unwrap();
                st
            })
            .collect();

        let mut bp_out = [0u64; 2];
        let mut sc_out = [0u64; 2];

        for tick in 0..1_000 {
            let p_in = [rng.next_u64(), rng.next_u64()];
            bp.step(&compiled, &p_in, &mut bp_out).unwrap();

            for (lane, sc) in scalars.iter_mut().enumerate() {
                let s_in = [(p_in[0] >> lane) & 1, (p_in[1] >> lane) & 1];
                sc.step(&compiled, &s_in, &mut sc_out).unwrap();
                for p in 0..2 {
                    let bp_bit = (bp_out[p] >> lane) & 1;
                    if bp_bit != sc_out[p] {
                        failures.push(format!(
                            "G1: BitParallel64 mismatch on circuit #{idx}, tick {tick}, lane {lane}, port {p}"
                        ));
                        return false;
                    }
                }
            }
        }
    }

    failures.len() == start_len
}

fn verify_g1_negative_invariants(failures: &mut Vec<String>) {
    // (1) InvalidBoundaryInput
    let mut c = Circuit::new(Width::BITS_2, 1, 1, WireDelayMode::Combinational);
    c.connect(SignalSource::BoundaryIn(1), SignalSink::BoundaryOut(0));
    if !matches!(
        c.compile(),
        Err(InvariantError::InvalidBoundaryInput { .. })
    ) {
        failures.push("G1: failed to reject InvalidBoundaryInput".to_owned());
    }

    // (2) InvalidBoundaryOutput
    let mut c = Circuit::new(Width::BITS_2, 1, 1, WireDelayMode::Combinational);
    c.connect(SignalSource::BoundaryIn(0), SignalSink::BoundaryOut(1));
    if !matches!(
        c.compile(),
        Err(InvariantError::InvalidBoundaryOutput { .. })
    ) {
        failures.push("G1: failed to reject InvalidBoundaryOutput".to_owned());
    }

    // (3) InvalidNode
    let mut c = Circuit::new(Width::BITS_2, 1, 1, WireDelayMode::Combinational);
    c.connect(
        SignalSource::NodeOut {
            node: NodeId(7),
            port: 0,
        },
        SignalSink::BoundaryOut(0),
    );
    if !matches!(c.compile(), Err(InvariantError::InvalidNode { .. })) {
        failures.push("G1: failed to reject InvalidNode".to_owned());
    }

    // (4) InvalidNodeInputPort
    let mut c = Circuit::new(Width::BITS_2, 1, 1, WireDelayMode::Combinational);
    let eqz = c.add_node(Primitive::Eqz);
    c.connect(
        SignalSource::BoundaryIn(0),
        SignalSink::NodeIn { node: eqz, port: 1 },
    );
    if !matches!(
        c.compile(),
        Err(InvariantError::InvalidNodeInputPort { .. })
    ) {
        failures.push("G1: failed to reject InvalidNodeInputPort".to_owned());
    }

    // (5) InvalidNodeOutputPort
    let mut c = Circuit::new(Width::BITS_2, 1, 1, WireDelayMode::Combinational);
    let eqz = c.add_node(Primitive::Eqz);
    c.connect(
        SignalSource::BoundaryIn(0),
        SignalSink::NodeIn { node: eqz, port: 0 },
    );
    c.connect(
        SignalSource::NodeOut { node: eqz, port: 1 },
        SignalSink::BoundaryOut(0),
    );
    if !matches!(
        c.compile(),
        Err(InvariantError::InvalidNodeOutputPort { .. })
    ) {
        failures.push("G1: failed to reject InvalidNodeOutputPort".to_owned());
    }

    // (6) ZeroHopInDelayedMode
    let mut c = Circuit::new(Width::BITS_2, 1, 1, WireDelayMode::TicksPerHop);
    c.connect_hops(SignalSource::BoundaryIn(0), SignalSink::BoundaryOut(0), 0);
    if !matches!(
        c.compile(),
        Err(InvariantError::ZeroHopInDelayedMode { .. })
    ) {
        failures.push("G1: failed to reject ZeroHopInDelayedMode".to_owned());
    }

    // (7) MultipleDrivers
    let mut c = Circuit::new(Width::BITS_2, 2, 1, WireDelayMode::Combinational);
    c.connect(SignalSource::BoundaryIn(0), SignalSink::BoundaryOut(0));
    c.connect(SignalSource::BoundaryIn(1), SignalSink::BoundaryOut(0));
    if !matches!(c.compile(), Err(InvariantError::MultipleDrivers { .. })) {
        failures.push("G1: failed to reject MultipleDrivers".to_owned());
    }

    // (8) UnconnectedInput
    let mut c = Circuit::new(Width::BITS_2, 1, 1, WireDelayMode::Combinational);
    let add = c.add_node(Primitive::Add);
    c.connect(
        SignalSource::BoundaryIn(0),
        SignalSink::NodeIn { node: add, port: 0 },
    );
    c.connect(
        SignalSource::NodeOut { node: add, port: 0 },
        SignalSink::BoundaryOut(0),
    );
    if !matches!(c.compile(), Err(InvariantError::UnconnectedInput { .. })) {
        failures.push("G1: failed to reject UnconnectedInput".to_owned());
    }

    // (9) IllegalFanOut
    let mut c = Circuit::new(Width::BITS_2, 1, 2, WireDelayMode::Combinational);
    c.connect(SignalSource::BoundaryIn(0), SignalSink::BoundaryOut(0));
    c.connect(SignalSource::BoundaryIn(0), SignalSink::BoundaryOut(1));
    if !matches!(c.compile(), Err(InvariantError::IllegalFanOut { .. })) {
        failures.push("G1: failed to reject IllegalFanOut".to_owned());
    }

    // (10) CombinationalLoop
    let mut c = Circuit::new(Width::BITS_1, 0, 0, WireDelayMode::Combinational);
    let n0 = c.add_node(Primitive::Eqz);
    let n1 = c.add_node(Primitive::Eqz);
    c.connect(
        SignalSource::NodeOut { node: n0, port: 0 },
        SignalSink::NodeIn { node: n1, port: 0 },
    );
    c.connect(
        SignalSource::NodeOut { node: n1, port: 0 },
        SignalSink::NodeIn { node: n0, port: 0 },
    );
    if !matches!(c.compile(), Err(InvariantError::CombinationalLoop { .. })) {
        failures.push("G1: failed to reject CombinationalLoop".to_owned());
    }

    // EvalError checks: BitParallel64State on k = 2, register mismatch, port slice mismatch.
    let mut c2 = Circuit::new(Width::BITS_2, 1, 1, WireDelayMode::Combinational);
    let d = c2.add_node(Primitive::delay_zero());
    c2.connect(
        SignalSource::BoundaryIn(0),
        SignalSink::NodeIn { node: d, port: 0 },
    );
    c2.connect(
        SignalSource::NodeOut { node: d, port: 0 },
        SignalSink::BoundaryOut(0),
    );
    let comp2 = c2.compile().unwrap();
    if !matches!(
        BitParallel64State::new(&comp2),
        Err(EvalError::UnsupportedBitParallelWidth { actual_bits: 2 })
    ) {
        failures.push("G1: BitParallel64State failed to reject k=2".to_owned());
    }
    let mut st2 = ScalarState::new(&comp2);
    if !matches!(
        st2.set_registers(&comp2, &[]),
        Err(EvalError::RegisterCountMismatch { .. })
    ) {
        failures.push("G1: ScalarState failed to reject RegisterCountMismatch".to_owned());
    }
    let mut out_buf = [0u64; 1];
    if !matches!(
        st2.step(&comp2, &[], &mut out_buf),
        Err(EvalError::PortSliceMismatch { .. })
    ) {
        failures.push("G1: ScalarState failed to reject PortSliceMismatch".to_owned());
    }
}

/// Gate G2: Exact $\varepsilon$-machine ($C_\mu, h_\mu, L_{\text{sync}}$) across `T1`, `T3b`, and small `T3c`
/// at $k \in \{1, 2\}$ ($\varepsilon = 0$ and $\varepsilon = 0.05$), Reeds–Sloane $2L$ convergence at $k = 1, 2$,
/// and certified small-circuit + state-minimal witness checks.
fn verify_gate_g2_yardsticks(failures: &mut Vec<String>) -> bool {
    let start_len = failures.len();

    for width in [Width::BITS_1, Width::BITS_2] {
        let k = width.bits();
        let high = if k == 1 { 1 } else { 3 };
        let noisy = NoiseModel::IndependentSubstitution {
            epsilon: 0.05,
            channels: vec![0],
        };
        let expected_noisy_h = declared_noise_entropy_bits_per_symbol(&noisy, 1, width);

        // Analytical (generator, expected_c_mu, expected_sync_order) table at k in {1, 2}:
        let mut analytic_cases: Vec<(GeneratorConfig, f64, usize)> = vec![
            (GeneratorConfig::T1_Constant { value: 1 }, 0.0, 0),
            (GeneratorConfig::T1_Alternating { a: 0, b: high }, 1.0, 1),
            (
                GeneratorConfig::T1_PeriodicMotif {
                    motif: vec![0, 1, high],
                },
                3.0f64.log2(),
                if k == 1 { 2 } else { 1 },
            ),
            (
                GeneratorConfig::T1_AffineLag1 {
                    mul_a: 1,
                    add_b: 1,
                    init: 0,
                },
                f64::from(k),
                1,
            ),
            (
                GeneratorConfig::T3b_CounterPow2 {
                    log2_modulus: k,
                    init: 0,
                },
                f64::from(k),
                1,
            ),
        ];

        if k == 1 {
            // Primitive L = 3 LFSR (period 7):
            analytic_cases.push((
                GeneratorConfig::T3c_Lfsr {
                    taps: vec![1, 0, 1],
                    init: vec![1, 0, 0],
                },
                7.0f64.log2(),
                3,
            ));
        } else {
            // Non-power-of-two mod-3 counter + Z_4 period-6 L = 2 LFSR:
            analytic_cases.push((
                GeneratorConfig::T3b_CounterModN {
                    modulus: 3,
                    init: 0,
                },
                3.0f64.log2(),
                1,
            ));
            analytic_cases.push((
                GeneratorConfig::T3c_Lfsr {
                    taps: vec![1, 1],
                    init: vec![0, 1],
                },
                6.0f64.log2(),
                2,
            ));
        }

        for (gen_cfg, exp_c_mu, exp_sync) in analytic_cases {
            let clean_gen = gen_cfg.build(width).expect("valid analytic generator");
            let spec = clean_gen.to_finite_state_spec().expect("small state space");

            let em_clean = solve_epsilon_machine(&spec, &NoiseModel::None);
            if (em_clean.c_mu_bits - exp_c_mu).abs() >= 1e-9
                || em_clean.h_mu_bits_per_symbol.abs() >= 1e-9
                || em_clean.exact_sync_order != Some(exp_sync)
            {
                failures.push(format!(
                    "G2(clean k={k}): {gen_cfg:?} expected (C_mu={exp_c_mu}, h_mu=0, sync={exp_sync}), got {em_clean:?}"
                ));
            }

            let em_noisy = solve_epsilon_machine(&spec, &noisy);
            if (em_noisy.h_mu_bits_per_symbol - expected_noisy_h).abs() >= 1e-9 {
                failures.push(format!(
                    "G2(noisy k={k}): {gen_cfg:?} expected h_mu={expected_noisy_h}, got {}",
                    em_noisy.h_mu_bits_per_symbol
                ));
            }
        }

        // Certified synthesis check across all 4 T1 tasks.
        for gen_cfg in GeneratorConfig::m0_t1_suite(width) {
            let clean_gen = gen_cfg.build(width).unwrap();
            let spec = clean_gen.to_finite_state_spec().unwrap();
            let (circ, status) = exact_synthesize_certified_circuit(
                width,
                1,
                WireDelayMode::Combinational,
                Some(&spec),
                &[],
                3,
                1024,
            );
            if circ.is_none() || !matches!(status, SynthesisCertStatus::CertifiedExact { .. }) {
                failures.push(format!(
                    "G2(synth k={k}): failed to certify T1 synthesis on {gen_cfg:?}: {status:?}"
                ));
            }
        }
    }

    // Explicit 1-register nonlinear Eqz witness verification on k = 2 `[0, 1, 3]`.
    let motif_013 = GeneratorConfig::T1_PeriodicMotif {
        motif: vec![0, 1, 3],
    }
    .build(Width::BITS_2)
    .unwrap();
    let spec_013 = motif_013.to_finite_state_spec().unwrap();
    let witness = build_z4_motif_013_reactive_one_reg_witness();
    match witness.compile() {
        Ok(comp) => {
            let eff_regs = comp.total_registers() + usize::from(comp.active_boundary_input_count());
            if eff_regs != 1 || certify_circuit_against_spec(&comp, &spec_013, 1024) != Some(1) {
                failures.push(format!(
                    "G2(witness): k=2 [0,1,3] 1-register witness failed certification (eff_regs={eff_regs})"
                ));
            }
        }
        Err(e) => failures.push(format!(
            "G2(witness): k=2 [0,1,3] 1-register witness failed compile: {e}"
        )),
    }

    // Reeds–Sloane 2L checks:
    // (a) k = 1 LFSR of length L = 12 (primitive polynomial x^12 + x^6 + x^4 + x + 1).
    let mut lfsr12 = GeneratorConfig::T3c_Lfsr {
        taps: vec![1, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 1],
        init: vec![1, 0, 1, 1, 0, 0, 1, 0, 1, 0, 0, 1],
    }
    .build(Width::BITS_1)
    .unwrap();
    let mut rs1 = ReedsSloaneSynthesizer::new(Width::BITS_1);
    let mut frame = [0u64];
    for t in 0..80 {
        lfsr12.next_clean_frame(&mut frame);
        if t >= 24 && rs1.predict_next() != frame[0] {
            failures.push(format!("G2: k=1 L=12 LFSR prediction failed at tick {t}"));
            break;
        }
        rs1.push_symbol(frame[0]);
    }
    if rs1.minimal_length() != 12 {
        failures.push(format!(
            "G2: expected minimal_length 12 on primitive LFSR, got {}",
            rs1.minimal_length()
        ));
    }

    // (b) k = 2 LFSR of length L = 8 over Z_4 with zero-divisor tap `2` and unit taps `1, 3`.
    let mut lfsr8_z4 = GeneratorConfig::T3c_Lfsr {
        taps: vec![2, 1, 0, 3, 2, 0, 1, 3],
        init: vec![1, 2, 3, 0, 2, 1, 3, 1],
    }
    .build(Width::BITS_2)
    .unwrap();
    let mut rs2 = ReedsSloaneSynthesizer::new(Width::BITS_2);
    for t in 0..64 {
        lfsr8_z4.next_clean_frame(&mut frame);
        if t >= 16 && rs2.predict_next() != frame[0] {
            failures.push(format!(
                "G2: k=2 L=8 zero-divisor recurrence failed at tick {t}"
            ));
            break;
        }
        rs2.push_symbol(frame[0]);
    }
    if rs2.minimal_length() > 8 {
        failures.push(format!(
            "G2: k=2 recurrence length {} exceeded generator L=8",
            rs2.minimal_length()
        ));
    }

    failures.len() == start_len
}

/// Gate G3-rev1: T1 suite at $k = 1$ and $k = 2$ across `seed_offset .. seed_offset + num_seeds`
/// under both $\varepsilon = 0$ and $\varepsilon = 0.05$.
fn verify_gate_g3_t1_optima(
    seed_offset: u32,
    num_seeds: u32,
    case_reports: &mut Vec<GateCaseSummary>,
    failures: &mut Vec<String>,
) -> bool {
    let start_len = failures.len();
    let min_req_95pct = if num_seeds >= 20 {
        (num_seeds * 19).div_ceil(20)
    } else {
        num_seeds.saturating_sub(1).max(1)
    };
    let min_req_90pct = if num_seeds >= 20 {
        (num_seeds * 18).div_ceil(20)
    } else {
        num_seeds.saturating_sub(1).max(1)
    };

    for width in [Width::BITS_1, Width::BITS_2] {
        let suite = GeneratorConfig::m0_t1_suite(width);

        // (a) Noiseless T1 suite across all 4 generators:
        for gen_cfg in &suite {
            let gen_label = format!("{gen_cfg:?}");

            for baseline in [
                BaselineConfig::Exhaustive {
                    max_nodes: 3,
                    fact_window: 64,
                },
                BaselineConfig::Ctw { context_depth: 8 },
            ] {
                let cfg = ExperimentConfig {
                    schema_version: CONFIG_SCHEMA_VERSION,
                    master_seed: u64::from(seed_offset),
                    substrate: SubstrateConfig {
                        width_bits: width,
                        wire_delay: WireDelayMode::Combinational,
                        wiring_topology: WiringTopology::WellMixed,
                    },
                    environment: EnvironmentConfig {
                        horizon_ticks: 240,
                        transient_warmup_ticks: 40,
                        generator: gen_cfg.clone(),
                        noise: NoiseModel::None,
                    },
                    budget: BudgetConfig {
                        max_eval_ticks: 100_000,
                    },
                    baseline,
                };
                let report = execute_experiment(&cfg).expect("valid noiseless config");
                let mut ok = report.scorecard.post_transient_observed_mismatches == 0;
                if !ok {
                    failures.push(format!(
                        "G3-rev1(noiseless): {} had {} mismatches on {gen_cfg:?} at k={}",
                        report.baseline_name,
                        report.scorecard.post_transient_observed_mismatches,
                        width.bits()
                    ));
                }

                if report.baseline_name == "Exhaustive" {
                    if !matches!(
                        report.ground_truth.exact_synthesis_status,
                        SynthesisCertStatus::CertifiedExact { .. }
                    ) {
                        ok = false;
                        failures.push(format!(
                            "G3-rev1(synth-cert): Exhaustive ground truth not CertifiedExact on {gen_cfg:?}"
                        ));
                    }
                    if report.scorecard.excess_nodes_over_exact_synthesis != Some(0) {
                        ok = false;
                        failures.push(format!(
                            "G3-rev1(exact-synth): Exhaustive excess nodes {:?} != Some(0) on {gen_cfg:?}",
                            report.scorecard.excess_nodes_over_exact_synthesis
                        ));
                    }
                    if let Some(eff_excess) = report.scorecard.effective_excess_state_bits_over_c_mu
                    {
                        if eff_excess < -1e-9 {
                            ok = false;
                            failures.push(format!(
                                "G3-rev1(state-bound): effective_excess_state_bits_over_c_mu={eff_excess} < 0 on {gen_cfg:?}"
                            ));
                        }
                        if width == Width::BITS_1
                            && matches!(
                                gen_cfg,
                                GeneratorConfig::T1_Alternating { .. }
                                    | GeneratorConfig::T1_AffineLag1 { .. }
                            )
                            && eff_excess.abs() > 1e-9
                        {
                            ok = false;
                            failures.push(format!(
                                "G3-rev1(state-tightness): expected 0.0 excess state bits on k=1 {gen_cfg:?}, got {eff_excess}"
                            ));
                        }
                    } else {
                        ok = false;
                        failures.push(format!(
                            "G3-rev1(state-bound): missing effective_excess_state_bits_over_c_mu on {gen_cfg:?}"
                        ));
                    }
                }

                let excess = report
                    .scorecard
                    .excess_prob_bits_over_h_mu
                    .or(report.scorecard.excess_residual_bits_over_h_mu);
                case_reports.push(GateCaseSummary {
                    width_bits: width.bits(),
                    epsilon: 0.0,
                    generator: gen_label.clone(),
                    baseline: report.baseline_name,
                    gate_enforced: true,
                    passed: ok,
                    seeds_passed: u32::from(ok),
                    seeds_total: 1,
                    mean_excess_over_h_mu: excess,
                    mean_paired_regret: report.scorecard.paired_regret_bits_per_symbol,
                    max_paired_regret: report.scorecard.paired_regret_bits_per_symbol,
                });
            }

            // Stochastic noiseless CGP across `seed_offset .. seed_offset + num_seeds`:
            let mut cgp_passes = 0u32;
            let mut sum_excess = 0.0f64;
            let mut sum_regret = 0.0f64;
            let mut max_regret = f64::NEG_INFINITY;
            for s_idx in 0..num_seeds {
                let seed = u64::from(seed_offset) + u64::from(s_idx);
                let cfg = ExperimentConfig {
                    schema_version: CONFIG_SCHEMA_VERSION,
                    master_seed: seed,
                    substrate: SubstrateConfig {
                        width_bits: width,
                        wire_delay: WireDelayMode::Combinational,
                        wiring_topology: WiringTopology::WellMixed,
                    },
                    environment: EnvironmentConfig {
                        horizon_ticks: 240,
                        transient_warmup_ticks: 60,
                        generator: gen_cfg.clone(),
                        noise: NoiseModel::None,
                    },
                    budget: BudgetConfig {
                        max_eval_ticks: 80_000,
                    },
                    baseline: BaselineConfig::Cgp {
                        lambda: 4,
                        max_genes: 8,
                        fact_window: 64,
                    },
                };
                let rep = execute_experiment(&cfg).expect("valid noiseless CGP config");
                if rep.scorecard.post_transient_observed_mismatches == 0 {
                    cgp_passes += 1;
                }
                let ex = rep.scorecard.excess_residual_bits_over_h_mu.unwrap_or(0.0);
                let reg = rep.scorecard.paired_regret_bits_per_symbol.unwrap_or(0.0);
                sum_excess += ex;
                sum_regret += reg;
                max_regret = max_regret.max(reg);
            }
            let passed = cgp_passes >= min_req_95pct;
            if !passed {
                failures.push(format!(
                    "G3-rev1(noiseless CGP): passed {cgp_passes}/{num_seeds} (< {min_req_95pct}) on {gen_cfg:?} at k={}",
                    width.bits()
                ));
            }
            case_reports.push(GateCaseSummary {
                width_bits: width.bits(),
                epsilon: 0.0,
                generator: gen_label,
                baseline: "CGP(1+lambda)".to_owned(),
                gate_enforced: true,
                passed,
                seeds_passed: cgp_passes,
                seeds_total: num_seeds,
                mean_excess_over_h_mu: Some(sum_excess / f64::from(num_seeds)),
                mean_paired_regret: Some(sum_regret / f64::from(num_seeds)),
                max_paired_regret: Some(max_regret),
            });
        }

        // (b) Noisy T1 suite (eps = 0.05) across ALL 4 T1 generators:
        let noise = NoiseModel::IndependentSubstitution {
            epsilon: 0.05,
            channels: vec![0],
        };

        for gen_cfg in &suite {
            let gen_label = format!("{gen_cfg:?}");
            let is_single_osc_family = matches!(
                gen_cfg,
                GeneratorConfig::T1_Constant { .. } | GeneratorConfig::T1_Alternating { .. }
            );

            // 1. Universal probabilistic CTW (enforced across all 4 noisy T1 generators):
            {
                let mut passes = 0u32;
                let mut sum_excess = 0.0f64;
                let mut sum_regret = 0.0f64;
                let mut max_regret = f64::NEG_INFINITY;

                // Universal probabilistic CTW (`context_depth = 9`, T = 20,000 post-warmup ticks):
                // enforces per-seed paired regret <= 0.030 on >= 19/20 seeds, mean paired regret
                // <= 0.028, and population mean excess over h_mu <= 0.030 across all 4 T1 tasks.
                let per_seed_reg_cap = 0.030;
                let mean_reg_cap = 0.028;
                let mean_ex_cap = 0.030;

                for s_idx in 0..num_seeds {
                    let seed = u64::from(seed_offset) + u64::from(s_idx);
                    let cfg = ExperimentConfig {
                        schema_version: CONFIG_SCHEMA_VERSION,
                        master_seed: seed,
                        substrate: SubstrateConfig {
                            width_bits: width,
                            wire_delay: WireDelayMode::Combinational,
                            wiring_topology: WiringTopology::WellMixed,
                        },
                        environment: EnvironmentConfig {
                            horizon_ticks: 20_800,
                            transient_warmup_ticks: 800,
                            generator: gen_cfg.clone(),
                            noise: noise.clone(),
                        },
                        budget: BudgetConfig {
                            max_eval_ticks: 1_000_000,
                        },
                        baseline: BaselineConfig::Ctw { context_depth: 9 },
                    };
                    let rep = execute_experiment(&cfg).expect("valid noisy CTW config");
                    let ex = rep
                        .scorecard
                        .excess_prob_bits_over_h_mu
                        .expect("stationary T1 has h_mu");
                    let reg = rep
                        .scorecard
                        .paired_prob_regret_bits_per_symbol
                        .expect("stationary T1 has oracle regret");
                    if reg <= per_seed_reg_cap {
                        passes += 1;
                    }
                    sum_excess += ex;
                    sum_regret += reg;
                    max_regret = max_regret.max(reg);
                }

                let mean_ex = sum_excess / f64::from(num_seeds);
                let mean_reg = sum_regret / f64::from(num_seeds);
                let mean_ok = if num_seeds >= 15 {
                    mean_ex <= mean_ex_cap && mean_reg <= mean_reg_cap
                } else {
                    mean_reg <= mean_reg_cap
                };
                let passed = passes >= min_req_95pct && mean_ok;
                if !passed {
                    failures.push(format!(
                        "G3-rev1(noisy CTW): k={} {gen_cfg:?} passed={passes}/{num_seeds} (req {min_req_95pct}), mean_excess={mean_ex:.4}, mean_regret={mean_reg:.4}",
                        width.bits()
                    ));
                }
                case_reports.push(GateCaseSummary {
                    width_bits: width.bits(),
                    epsilon: 0.05,
                    generator: gen_label.clone(),
                    baseline: "CTW".to_owned(),
                    gate_enforced: true,
                    passed,
                    seeds_passed: passes,
                    seeds_total: num_seeds,
                    mean_excess_over_h_mu: Some(mean_ex),
                    mean_paired_regret: Some(mean_reg),
                    max_paired_regret: Some(max_regret),
                });
            }

            // 2. Discrete circuit CGP (enforced on Constant & Alternating; recorded transparently on PeriodicMotif & AffineLag1):
            {
                let mut passes = 0u32;
                let mut sum_excess = 0.0f64;
                let mut sum_regret = 0.0f64;
                let mut max_regret = f64::NEG_INFINITY;

                for s_idx in 0..num_seeds {
                    let seed = u64::from(seed_offset) + u64::from(s_idx);
                    let cfg = ExperimentConfig {
                        schema_version: CONFIG_SCHEMA_VERSION,
                        master_seed: seed,
                        substrate: SubstrateConfig {
                            width_bits: width,
                            wire_delay: WireDelayMode::Combinational,
                            wiring_topology: WiringTopology::WellMixed,
                        },
                        environment: EnvironmentConfig {
                            horizon_ticks: 4_400,
                            transient_warmup_ticks: 400,
                            generator: gen_cfg.clone(),
                            noise: noise.clone(),
                        },
                        budget: BudgetConfig {
                            max_eval_ticks: 450_000,
                        },
                        baseline: BaselineConfig::Cgp {
                            lambda: 6,
                            max_genes: 6,
                            fact_window: 360,
                        },
                    };
                    let rep = execute_experiment(&cfg).expect("valid noisy CGP config");
                    let ex = rep
                        .scorecard
                        .excess_residual_bits_over_h_mu
                        .expect("stationary T1 has h_mu");
                    let reg = rep
                        .scorecard
                        .paired_residual_regret_bits_per_symbol
                        .expect("stationary T1 has oracle regret");
                    if reg <= 0.020 {
                        passes += 1;
                    }
                    sum_excess += ex;
                    sum_regret += reg;
                    max_regret = max_regret.max(reg);
                }

                let mean_ex = sum_excess / f64::from(num_seeds);
                let mean_reg = sum_regret / f64::from(num_seeds);
                let mean_ok = if num_seeds >= 15 {
                    mean_ex <= 0.020 && mean_reg <= 0.020
                } else {
                    mean_reg <= 0.020
                };
                let crit_met = passes >= min_req_90pct && mean_ok;
                let passed = !is_single_osc_family || crit_met;
                if !passed {
                    failures.push(format!(
                        "G3-rev1(noisy CGP): k={} {gen_cfg:?} passed={passes}/{num_seeds} (req {min_req_90pct}), mean_excess={mean_ex:.4}, mean_regret={mean_reg:.4}",
                        width.bits()
                    ));
                }
                case_reports.push(GateCaseSummary {
                    width_bits: width.bits(),
                    epsilon: 0.05,
                    generator: gen_label,
                    baseline: "CGP(1+lambda)".to_owned(),
                    gate_enforced: is_single_osc_family,
                    passed,
                    seeds_passed: passes,
                    seeds_total: num_seeds,
                    mean_excess_over_h_mu: Some(mean_ex),
                    mean_paired_regret: Some(mean_reg),
                    max_paired_regret: Some(max_regret),
                });
            }
        }
    }

    failures.len() == start_len
}

fn locate_arkhe_binary() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("CARGO_BIN_EXE_arkhe") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Some(pb);
        }
    }
    let exe = std::env::current_exe().ok()?;
    if exe.file_stem().is_some_and(|s| s == "arkhe") {
        return Some(exe);
    }
    let sibling = exe
        .parent()?
        .parent()?
        .join(if cfg!(windows) { "arkhe.exe" } else { "arkhe" });
    sibling.is_file().then_some(sibling)
}

/// Gate G4: Portable `ChaCha8Rng` golden vectors + 6-configuration cross-process OS subprocess
/// reproducibility check (`arkhe run --config ...` invoked in two separate child processes).
fn verify_gate_g4_reproducibility(failures: &mut Vec<String>) -> bool {
    let start_len = failures.len();

    // 1. Portable ChaCha8Rng golden vector check.
    let mut s_gold = SeedBank::new(0xA4_4B_48_45).stream(StreamDomain::BaselineSearch { trial: 0 });
    let g0 = s_gold.next_u64();
    let g1 = s_gold.next_u64();
    if g0 != 10_570_056_588_342_067_228 || g1 != 10_419_271_532_414_833_232 {
        failures.push(format!(
            "G4: ChaCha8Rng golden vector drift: got ({g0}, {g1})"
        ));
    }

    // 2. Locate `arkhe` binary for true OS cross-process execution.
    let Some(arkhe_bin) = locate_arkhe_binary() else {
        failures.push("G4: could not locate arkhe CLI binary for subprocess check".to_owned());
        return false;
    };

    // 3. Diverse 6-configuration matrix covering k=1/2, Combinational/TicksPerHop, clean/noisy, all 5 baselines.
    let matrix = [
        ExperimentConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            master_seed: 42,
            substrate: SubstrateConfig {
                width_bits: Width::BITS_2,
                wire_delay: WireDelayMode::Combinational,
                wiring_topology: WiringTopology::WellMixed,
            },
            environment: EnvironmentConfig {
                horizon_ticks: 200,
                transient_warmup_ticks: 40,
                generator: GeneratorConfig::T1_PeriodicMotif {
                    motif: vec![0, 1, 3],
                },
                noise: NoiseModel::IndependentSubstitution {
                    epsilon: 0.05,
                    channels: vec![0],
                },
            },
            budget: BudgetConfig {
                max_eval_ticks: 25_000,
            },
            baseline: BaselineConfig::Cgp {
                lambda: 4,
                max_genes: 6,
                fact_window: 64,
            },
        },
        ExperimentConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            master_seed: 7,
            substrate: SubstrateConfig {
                width_bits: Width::BITS_1,
                wire_delay: WireDelayMode::TicksPerHop,
                wiring_topology: WiringTopology::WellMixed,
            },
            environment: EnvironmentConfig {
                horizon_ticks: 160,
                transient_warmup_ticks: 32,
                generator: GeneratorConfig::T1_Alternating { a: 0, b: 1 },
                noise: NoiseModel::None,
            },
            budget: BudgetConfig {
                max_eval_ticks: 20_000,
            },
            baseline: BaselineConfig::RandomSearch {
                max_nodes: 4,
                fact_window: 48,
                samples_per_error: 12,
            },
        },
        ExperimentConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            master_seed: 99,
            substrate: SubstrateConfig {
                width_bits: Width::BITS_2,
                wire_delay: WireDelayMode::Combinational,
                wiring_topology: WiringTopology::WellMixed,
            },
            environment: EnvironmentConfig {
                horizon_ticks: 240,
                transient_warmup_ticks: 40,
                generator: GeneratorConfig::T1_AffineLag1 {
                    mul_a: 1,
                    add_b: 1,
                    init: 0,
                },
                noise: NoiseModel::IndependentSubstitution {
                    epsilon: 0.05,
                    channels: vec![0],
                },
            },
            budget: BudgetConfig {
                max_eval_ticks: 10_000,
            },
            baseline: BaselineConfig::Ctw { context_depth: 6 },
        },
        ExperimentConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            master_seed: 13,
            substrate: SubstrateConfig {
                width_bits: Width::BITS_2,
                wire_delay: WireDelayMode::Combinational,
                wiring_topology: WiringTopology::WellMixed,
            },
            environment: EnvironmentConfig {
                horizon_ticks: 80,
                transient_warmup_ticks: 16,
                generator: GeneratorConfig::T3c_Lfsr {
                    taps: vec![2, 1, 0, 3],
                    init: vec![1, 2, 3, 1],
                },
                noise: NoiseModel::None,
            },
            budget: BudgetConfig {
                max_eval_ticks: 5_000,
            },
            baseline: BaselineConfig::ReedsSloane,
        },
        ExperimentConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            master_seed: 5,
            substrate: SubstrateConfig {
                width_bits: Width::BITS_1,
                wire_delay: WireDelayMode::Combinational,
                wiring_topology: WiringTopology::WellMixed,
            },
            environment: EnvironmentConfig {
                horizon_ticks: 140,
                transient_warmup_ticks: 30,
                generator: GeneratorConfig::T1_PeriodicMotif {
                    motif: vec![0, 1, 1],
                },
                noise: NoiseModel::None,
            },
            budget: BudgetConfig {
                max_eval_ticks: 40_000,
            },
            baseline: BaselineConfig::Exhaustive {
                max_nodes: 3,
                fact_window: 48,
            },
        },
        ExperimentConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            master_seed: 101,
            substrate: SubstrateConfig {
                width_bits: Width::BITS_1,
                wire_delay: WireDelayMode::TicksPerHop,
                wiring_topology: WiringTopology::WellMixed,
            },
            environment: EnvironmentConfig {
                horizon_ticks: 180,
                transient_warmup_ticks: 30,
                generator: GeneratorConfig::T1_Constant { value: 1 },
                noise: NoiseModel::IndependentSubstitution {
                    epsilon: 0.05,
                    channels: vec![0],
                },
            },
            budget: BudgetConfig {
                max_eval_ticks: 20_000,
            },
            baseline: BaselineConfig::Cgp {
                lambda: 4,
                max_genes: 5,
                fact_window: 64,
            },
        },
    ];

    let pid = std::process::id();
    let tmp_dir = std::env::temp_dir();

    for (idx, cfg) in matrix.iter().enumerate() {
        let seq = TMP_NONCE.fetch_add(1, Ordering::Relaxed);
        let cfg_path = tmp_dir.join(format!("arkhe_g4_{pid}_{seq}_{idx}.toml"));
        let out1_path = tmp_dir.join(format!("arkhe_g4_{pid}_{seq}_{idx}_a.json"));
        let out2_path = tmp_dir.join(format!("arkhe_g4_{pid}_{seq}_{idx}_b.json"));

        let toml_text = toml::to_string_pretty(cfg).expect("serializes to TOML");
        let roundtrip_cfg: ExperimentConfig = toml::from_str(&toml_text).expect("parses from TOML");
        if &roundtrip_cfg != cfg {
            failures.push(format!("G4(case {idx}): TOML round-trip altered config"));
            continue;
        }
        if let Err(e) = fs::write(&cfg_path, &toml_text) {
            failures.push(format!("G4(case {idx}): failed writing temp config: {e}"));
            continue;
        }

        let st1 = Command::new(&arkhe_bin)
            .args([
                "run",
                "--config",
                cfg_path.to_str().unwrap(),
                "--output",
                out1_path.to_str().unwrap(),
            ])
            .status();
        let st2 = Command::new(&arkhe_bin)
            .args([
                "run",
                "--config",
                cfg_path.to_str().unwrap(),
                "--output",
                out2_path.to_str().unwrap(),
            ])
            .status();

        match (st1, st2) {
            (Ok(s1), Ok(s2)) if s1.success() && s2.success() => {
                let bytes1 = fs::read(&out1_path).unwrap_or_default();
                let bytes2 = fs::read(&out2_path).unwrap_or_default();
                if bytes1.is_empty() || bytes1 != bytes2 {
                    failures.push(format!(
                        "G4(case {idx}): cross-process JSON outputs differed"
                    ));
                }
                if let Ok(parsed) = serde_json::from_slice::<RunReport>(&bytes1)
                    && parsed.provenance.package_version.is_empty()
                {
                    failures.push(format!("G4(case {idx}): empty BuildProvenance"));
                }
            }
            other => {
                failures.push(format!(
                    "G4(case {idx}): subprocess execution failed: {other:?}"
                ));
            }
        }

        // Also check distinct seed produces distinct manifest digest.
        let mut alt_cfg = cfg.clone();
        alt_cfg.master_seed = cfg.master_seed.wrapping_add(1);
        let in_proc_a = execute_experiment(cfg).expect("in-proc A");
        let in_proc_b = execute_experiment(&alt_cfg).expect("in-proc B");
        if in_proc_a.config_digest_fnv1a128 == in_proc_b.config_digest_fnv1a128 {
            failures.push(format!(
                "G4(case {idx}): distinct seeds produced identical config digest"
            ));
        }

        let _ = fs::remove_file(&cfg_path);
        let _ = fs::remove_file(&out1_path);
        let _ = fs::remove_file(&out2_path);
    }

    failures.len() == start_len
}
