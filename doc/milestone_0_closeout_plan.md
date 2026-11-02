# Milestone 0 Closeout Plan: Instrument Honesty and Gate Alignment

> **Status:** Completed & verified closeout plan addressing [milestone_0_review.md](milestone_0_review.md) against [foundational_research_charter_v2.md](foundational_research_charter_v2.md) and [milestone_0_plan.md](milestone_0_plan.md).
>
> **Scope:** Repair silent metric/configuration approximations, separate distinct notions of state and loss optimality, align automated gates `G1`–`G4` with an explicitly registered specification, add build provenance, and execute a holdout-seed closeout run.

---

## 1. Objective, Scope Boundaries, and Deliverables Mapping

- **Primary Objective:** Make Milestone 0's ground-truth yardsticks, experimental configuration surface, scorecard metrics, and `verify-m0` acceptance suite strictly honest and reproducible so Milestones `M1a`–`M6` never evaluate kill criteria against silent fallbacks or mislabeled bounds.
- **Explicit Out-of-Scope (Non-Goals):**
  - Pulling forward `M1a`–`M6` capabilities (metabolic energy, retrograde credit, sprouting, sleep loopback, `egg` equality saturation, SMT solvers, or active interventions).
  - Adding the continuous `RNN/LSTM` baseline (remains deferred to `M1b/M2`).
  - Implementing local-lattice candidate search inside `M0` baselines (unsupported topologies are rejected explicitly in `execute_experiment` instead).
  - Re-tuning `CgpBaseline` hyperparameters or mutation operators on the closeout holdout seeds.

### Deliverable Traceability Matrix

| Review Finding / Closeout Requirement | Responsible Work Package(s) | Target Modules & Docs |
| :--- | :--- | :--- |
| **Finding 3 (CLI & unknown $h_\mu$):** Reject negative/invalid `--epsilon`; stop substituting `0.0` for missing $h_\mu$; record unavailable ground-truth reasons | `WP-M0C.1` | `src/lib.rs`, `src/config.rs`, `src/metrics/scorecard.rs`, `src/baselines/mod.rs` |
| **Finding 3 (Bounded vs. exact ground truth):** Label curriculum scope, bounded noisy MSP sync, and prefix vs. full-cycle Reeds–Sloane; certify small-circuit exact synthesis across non-zero `Delay` inits and full generator-circuit cycles | `WP-M0C.1`, `WP-M0C.2` | `src/env/epsilon_machine.rs`, `src/env/tiers.rs`, `src/baselines/exhaustive.rs`, `src/config.rs` |
| **Finding 2 (State bounds & boundary lag):** Separate internal `DELAY`, wire-hop, and boundary-lag registers; distinguish causal-state lower bounds from node-minimal and state-minimal circuits | `WP-M0C.2`, `WP-M0C.3` | `src/metrics/scorecard.rs`, `src/baselines/mod.rs`, `src/verify.rs` |
| **Finding 4 (Loss & compute comparability):** Separate probabilistic log-loss from uniform point-prediction KT residual loss; add paired noise-oracle regret; label `TickBudget` as candidate-replay accounting | `WP-M0C.3` | `src/metrics/scorecard.rs`, `src/baselines/mod.rs`, `src/config.rs` |
| **Finding 5 (Topology enforcement):** Fail fast on non-`WellMixed` `wiring_topology` in `execute_experiment`; thread configured topology into MDL accounting | `WP-M0C.3` | `src/config.rs`, `src/baselines/mod.rs` |
| **Finding 6 (Phase-24 window workaround):** Replace `lcm(24)` trimming assumption with `prior_frame` causal input seeding and cycle-modular autonomous state alignment for arbitrary periods | `WP-M0C.4` | `src/baselines/mod.rs`, `src/baselines/cgp.rs`, `src/baselines/random_search.rs`, `src/baselines/exhaustive.rs` |
| **Finding 1 & Closeout Hygiene (Gates `G1`–`G4`, provenance, holdout seeds):** Complete `G1`/`G2` matrices, register prospective `G3-rev1` amendment, enforce multi-config subprocess `G4`, embed build provenance, and run untouched holdout seeds | `WP-M0C.5` | `scripts/build.rs`, `src/config.rs`, `src/verify.rs`, `tests/`, `doc/milestone_0_plan.md`, `doc/milestone_0_lessons_learned.md`, `README.md` |

---

## 2. Mathematical & Protocol Clarifications Locked Before Coding

### 2.1 Why Node-Minimal and State-Minimal Predictors Diverge at $k = 2$

On `T1_PeriodicMotif { motif: [0, 1, 3] }` over $\mathbb{Z}_4$ ($k = 2$), $C_\mu = \log_2 3 \approx 1.585$ bits, giving a universal causal-state lower bound of $\lceil C_\mu / 2 \rceil = 1$ width-2 register.

- **1-register transition is non-polynomial:** A lag-1 map $f(s_{t-1}) = s_t$ must satisfy $f(0) = 1$, $f(1) = 3$, and $f(3) = 0$. Because $1 \equiv 3 \pmod 2$ while $f(1) \equiv 3 \not\equiv 0 \equiv f(3) \pmod 2$, $f$ violates parity preservation over $\mathbb{Z}_4$ and cannot be built from `Add`, `Sub`, and `Mul` alone; it requires `Eqz`, `Split` fan-out, and constant generation (`11` nodes in `build_z4_motif_013_reactive_one_reg_witness`, using `0` `Delay` + `1` `BoundaryIn` = `1` effective register).
- **Multi-register shift/linear transitions use far fewer nodes:** Over three lags ($s_t = s_{t-3}$), the pure shift chain `BoundaryIn -> Delay -> Delay -> BoundaryOut` predicts `[0, 1, 3]` in **2 nodes** (`2` `Delay` + `1` `BoundaryIn` = `3` effective registers, requiring zero `Split` or arithmetic nodes), while over two lags $s_t \equiv -(s_{t-1} + s_{t-2}) \pmod 4$ uses `2` effective registers—trading extra lag state for smaller circuit description length.

Consequently, $\lceil C_\mu / k \rceil$ is a **necessary lower bound on total causal state** (`internal_delay + wire_hop + active_boundary_lag`), whereas node minimality and state minimality are distinct optimization targets that require separate fields and separate witness checks.

### 2.2 Finite-Sample Channel Variance Under Noisy Evaluation

At $k = 2$ and substitution rate $\varepsilon = 0.05$ over $T = 4{,}000$ evaluated ticks, even the exact Bayes-optimal clean-phase oracle incurs $-\log_2(0.95) \approx 0.0740$ bits on uncorrupted ticks and $-\log_2(0.05 / 3) \approx 5.9069$ bits on corrupted ticks ($\Delta \ell \approx 5.8329$ bits/corruption). With binomial corruption standard deviation $\sigma_M = \sqrt{4000 \times 0.05 \times 0.95} \approx 13.78$, the realized oracle loss on a finite seed has standard deviation:

$$
\sigma_{\text{oracle}} = \frac{\sigma_M \cdot \Delta \ell}{T} \approx 0.0201\text{ bits/symbol}.
$$

Comparing single-seed realized loss directly against population $h_\mu$ therefore conflates predictor error with $O(T^{-1/2})$ channel luck ($\approx 16$% of optimal runs exceed $+0.02$ bits/symbol by chance alone). `Scorecard` will retain asymptotic `excess_bits_over_h_mu` for population averages and add **paired oracle regret** (`paired_regret_bits_per_symbol`, comparing the predictor against the exact clean-phase Bayes oracle evaluated on the *same* realized noise trajectory) so single-seed noisy margins measure predictor suboptimality rather than noise-draw variance.

---

## 3. Work Package Specifications

### WP-M0C.1: Honest Ground-Truth Reporting and CLI Validation

- **Goal & Scope:** Eliminate silent error suppression, zero-entropy substitution, negative `--epsilon` bypasses, and unlabeled approximations in `yardstick` and `GroundTruthReport`. Out of scope: changing `substrate` evaluation semantics.
- **Target Artifacts & Changes:**
  - Files: `src/lib.rs`, `src/env/epsilon_machine.rs`, `src/env/tiers.rs`, `src/config.rs`
  - Schemas / Types:
    - `EpsilonMachineScope`: `StationaryProcess` | `CurriculumFinalStage { stage_index: usize, total_stages: usize, ticks_per_stage: u64 }`
    - `MspSyncStatus`: `ExactZeroComplexity` | `BoundedQuantizedEstimate { quant_grid: u64, max_steps: usize, max_mixed_states: usize }` | `SkippedAlphabetCap` | `ExceededMixedStateCap` | `ExceededStepCap`
    - `GroundTruthReport`: add `epsilon_machine_error: Option<String>`, `declared_noise_entropy_bits_per_symbol: f64`, `reeds_sloane_prefix_ticks: usize`, `reeds_sloane_certified_full_cycle: bool`, and `exact_synthesis_status: SynthesisCertStatus`
- **Implementation Tasks:**
  1. In `src/lib.rs` (`Commands::Yardstick`), add `allow_hyphen_values = true` on `--epsilon`; select `NoiseModel::None` **only** when `epsilon == 0.0`, routing all non-zero or `NaN` values (`epsilon < 0.0`, `epsilon >= 1.0`, `NaN`) into `NoiseModel::IndependentSubstitution` across `0..channels` so `NoiseModel::validate` returns `NoiseError`.
  2. In `src/env/tiers.rs` and `src/env/epsilon_machine.rs`, tag exported `FiniteStateSpec` / `EpsilonMachineSummary` with `EpsilonMachineScope` so `T3c_LfsrCurriculum` explicitly marks its $\varepsilon$-machine as `CurriculumFinalStage`. Rename `noisy_msp_sync_ticks_99pct` to `noisy_msp_approx_sync_ticks` paired with `msp_sync_status: Option<MspSyncStatus>`, updating docstrings to state its quantized/bounded nature accurately.
  3. In `src/config.rs` (`compute_ground_truth_yardstick`), capture `Err(e)` from `to_finite_state_spec()` into `epsilon_machine_error: Option<String>` instead of discarding via `.ok()`. When `epsilon_machine` is `Some(em)` and `t_0 + 2 * p_min <= 4_096`, run Reeds–Sloane over at least `t_0 + 2 * p_min` clean ticks and set `reeds_sloane_certified_full_cycle = true`; otherwise set `reeds_sloane_certified_full_cycle = false` with `reeds_sloane_prefix_ticks` recorded explicitly.
- **Verification & Proof Criteria:**
  - Unit and CLI tests verifying `arkhe yardstick --epsilon -0.05 ...` and `--epsilon=-0.05` exit non-zero with `NoiseError`.
  - Test on period-257 motif (`256` zeros followed by `1`) confirming `yardstick` extends across the full attractor cycle (`reeds_sloane_certified_full_cycle == true`, `reeds_sloane_min_len != [0]`), and test on a $> 2^{16}$-state noisy LFSR confirming `epsilon_machine == None`, `epsilon_machine_error == Some(...)`, and `declared_noise_entropy_bits_per_symbol > 0.36`.

---

### WP-M0C.2: Certified Small-Circuit Synthesis and Independent Witness Oracle

- **Goal & Scope:** Upgrade `ExhaustiveBaseline` and `exact_synthesize_clean_circuit` to enumerate non-zero `Delay { init }` states and certify candidates over the complete joint `(generator, circuit)` state cycle rather than a finite trace prefix. Out of scope: general-purpose SAT/SMT synthesis beyond small `M0` node caps (`max_nodes <= 3` or `4`).
- **Target Artifacts & Changes:**
  - Files: `src/baselines/exhaustive.rs`, `src/config.rs`
  - Schemas / Types:
    - `SynthesisCertStatus`: `CertifiedExact { max_nodes_searched: usize, min_nodes: usize }` | `NoCircuitWithinCap { max_nodes_searched: usize }` | `PrefixOnlyUncertified { prefix_ticks: usize, min_nodes: Option<usize> }` | `SkippedBoundsExceeded { reason: String }`
- **Implementation Tasks:**
  1. Extend canonical circuit enumeration in `src/baselines/exhaustive.rs` so each topology with `d` internal `Delay` nodes evaluates all $(2^k)^d$ initial register assignments whenever $(2^k)^d \le 256$, materializing `Primitive::Delay { init }` values on matching circuits (enabling constant loops such as `Delay(init = c) <-> Split` at $k \ge 2$).
  2. Implement `certify_circuit_against_spec(compiled: &CompiledCircuit, spec: &FiniteStateSpec, max_joint_steps: usize) -> Option<usize>`: step `(gen_state, prev_obs, circuit_regs)` from `(spec.initial_state, 0, init_regs)` until a joint state repeats (cycle closure). Accept as `CertifiedExact` iff cycle closure occurs within `max_joint_steps` (`4_096`) and **every** transition around the closed joint cycle has `pred == clean_emission`.
  3. Add an **independent raw-port-graph witness enumerator** in `#[cfg(test)]` (enumerating raw node/edge tuples for `0..=2` nodes at $k \in \lbrace 1, 2 \rbrace$ without `rec_place_comb`'s pool-pruning heuristics) and assert exact agreement on minimum node count across the `M0` `T1` suite so `ExhaustiveBaseline` is validated against an independent implementation.
  4. Construct and test an explicit 1-register ($\lceil \log_2(3) / 2 \rceil = 1$) nonlinear `Eqz` witness circuit for `T1_PeriodicMotif([0, 1, 3])` at $k = 2$, proving both the attainability of the 1-register state bound at larger node count and why the 3-node optimum uses 2 lag registers.
- **Verification & Proof Criteria:**
  - `cargo test --lib baselines::exhaustive` passes independent witness cross-checks at $k = 1, 2$, certifies `T1_Constant { value: 3 }` autonomous synthesis via `Delay { init: 3 }`, and rejects a 0-node echo on the period-257 motif `[0; 256] ++ [1]`.

---

### WP-M0C.3: Scorecard Separation, State Accounting, and Topology Guardrails

- **Goal & Scope:** Make `Scorecard` loss, state, and compute fields unambiguous across probabilistic and circuit baselines, eliminate `h_mu = 0.0` fallback, and reject unsupported `wiring_topology` settings in `execute_experiment`. Out of scope: implementing local-lattice evolution in `CgpBaseline` or `RandomSearchBaseline`.
- **Target Artifacts & Changes:**
  - Files: `src/metrics/scorecard.rs`, `src/baselines/mod.rs`, `src/config.rs`
  - Schemas / Types:
    - `BudgetAccountingMode`: `CandidateHistoryReplay` | `AnalyticalUnmetered`
    - `ConfigRunError::UnsupportedExperimentTopology(WiringTopology)`
    - Updated `Scorecard`:
      - Loss fields: `native_prob_log_loss_bits_per_symbol: Option<f64>` (distributional predictors only), `prequential_kt_residual_bits_per_symbol: f64` (computed identically from observed point-prediction mismatches for **all** baselines without the `0 -> 0.0` jump), `excess_prob_bits_over_h_mu: Option<f64>`, `excess_residual_bits_over_h_mu: Option<f64>`, `realized_oracle_bits_per_symbol: Option<f64>`, and `paired_regret_bits_per_symbol: Option<f64>`.
      - State fields: `delay_count: Option<usize>`, `wire_hop_register_count: Option<usize>`, `boundary_lag_register_count: Option<usize>` (count of `BoundaryIn(c)` channels with a directed path to any output or state register), `effective_state_registers: Option<usize>`, `internal_state_bits: Option<usize>`, and `effective_excess_state_bits_over_c_mu: Option<f64>` (`>= 0.0` on all valid noiseless predictors).
      - Compute fields: `budget_accounting_mode: BudgetAccountingMode` and `candidate_replay_ticks_consumed: u64`.
- **Implementation Tasks:**
  1. In `src/config.rs` (`execute_experiment`), validate `config.substrate.wiring_topology == WiringTopology::WellMixed`, returning `Err(ConfigRunError::UnsupportedExperimentTopology(topo))` otherwise, and pass `config.substrate.wiring_topology` into `run_prequential_harness`.
  2. In `src/baselines/mod.rs` (`run_prequential_harness`), compute `realized_oracle_bits_per_symbol` on noisy runs from `(clean_frame, observed_frame)` using the declared `NoiseModel` conditional probabilities ($1 - \varepsilon$ when `observed == clean`, $\frac{\varepsilon}{2^k - 1}$ otherwise) whenever `em_ground_truth` is `Some` (or `NoiseModel` is validated), yielding exact `paired_regret_bits_per_symbol` with channel-draw variance removed.
  3. Set `excess_*_over_h_mu` strictly to `None` whenever `em_ground_truth` is `None`. Compute `boundary_lag_register_count` by graph reachability on `best_circuit` so `T1_Alternating`'s 0-`Delay` reactive inverter reports `effective_state_registers = Some(1)` and `effective_excess_state_bits_over_c_mu = Some(0.0)` instead of `-1.0`.
- **Verification & Proof Criteria:**
  - Test confirming `execute_experiment` returns `Err(ConfigRunError::UnsupportedExperimentTopology(WiringTopology::LocalVonNeumann))` rather than emitting an identical `WellMixed` report.
  - Test on noisy $> 2^{16}$-state LFSR confirming `excess_prob_bits_over_h_mu == None` and `excess_residual_bits_over_h_mu == None`.
  - Test on `T1_Alternating` confirming `delay_count == Some(0)`, `boundary_lag_register_count == Some(1)`, and `effective_excess_state_bits_over_c_mu == Some(0.0)`.

---

### WP-M0C.4: Phase-Exact Sliding-Window Replay for Arbitrary Periods

- **Goal & Scope:** Remove the `PHASE_ALIGNED_TRIM_STEP = 24` assumption from candidate window evaluation so arbitrary generator periods (such as prime periods `5` and `7`) remain phase-aligned across sliding windows. Out of scope: `M4a` offline sleep replay controllers.
- **Target Artifacts & Changes:**
  - Files: `src/baselines/mod.rs`, `src/baselines/cgp.rs`, `src/baselines/random_search.rs`, `src/baselines/exhaustive.rs`
  - Interfaces:
    - `ObservationWindow`: tracks `start_tick: u64`, `prior_frame: Vec<u64>`, and `frames: Vec<Vec<u64>>`
    - `evaluate_circuit_on_window(compiled: &CompiledCircuit, window: &ObservationWindow, budget: &mut TickBudget) -> Option<(u64, u64, ScalarState)>`
- **Implementation Tasks:**
  1. Replace raw `Vec<Vec<u64>>` history buffers + `drain(0..24)` across `CgpBaseline`, `RandomSearchBaseline`, and `ExhaustiveBaseline` with a shared `ObservationWindow` helper that records `start_tick` and `prior_frame` whenever oldest frames are evicted.
  2. Seed `prev_obs` in `evaluate_circuit_on_window` from `window.prior_frame` (instead of `[0; n_ch]`), eliminating artificial boundary errors at `window.frames[0]` for all reactive circuits at any `start_tick`.
  3. When `compiled` has no reachable `BoundaryIn` dependency (`start_tick > 0` and purely autonomous state evolution), advance `ScalarState` from `t = 0` to `window.start_tick` via cycle-modular jump (detecting `(t_0, cycle_len)` over internal registers using bounded Brent/table cycle detection and stepping `t_0 + (start_tick - t_0) % cycle_len`, charging the actual stepped ticks to `TickBudget`).
- **Verification & Proof Criteria:**
  - Unit tests running `ExhaustiveBaseline` and `CgpBaseline` over long horizons (`horizon_ticks = 600`, `fact_window = 64`) on prime-period `T1_PeriodicMotif` streams of **period 5** (`[0, 1, 2, 3, 1]` at $k = 2$) and **period 7** (`[0, 0, 1, 0, 1, 1, 1]` at $k = 1$), proving autonomous and reactive circuits suffer zero phase-slip accuracy collapse after window eviction.

---

### WP-M0C.5: Build Provenance, Amended `G1`–`G4` Battery, and Holdout Closeout

- **Goal & Scope:** Record full binary/source/toolchain provenance in reports, align `src/verify.rs` with the complete `G1`–`G4` matrix (including documented prospective `G3-rev1`), and execute the closeout battery on reserved holdout seeds. Out of scope: modifying `CgpBaseline` search logic after inspecting holdout seeds.
- **Target Artifacts & Changes:**
  - Files: `scripts/build.rs` (new), `src/config.rs`, `src/verify.rs`, `src/lib.rs`, `tests/determinism.rs`, `tests/m0_acceptance.rs`, `doc/milestone_0_plan.md`, `doc/milestone_0_lessons_learned.md`, `README.md`
  - Schemas / CLI:
    - `BuildProvenance`: `{ package_version, rustc_version, target_triple, git_commit_sha: Option<String>, git_dirty: Option<bool>, cargo_lock_fnv1a128: String }` embedded in `RunReport` and `M0VerificationSummary`
    - `Commands::VerifyM0`: add `--seed-offset <u32>` (default `0`) alongside `--seeds <u32>` (default `20`), plus structured `case_reports: Vec<GateCaseSummary>` in `M0VerificationSummary`
- **Prospective Gate Amendment (`G3-rev1`, recorded alongside original `G3` in `doc/milestone_0_plan.md`):**
  - **G1 (Substrate & Invariants):** Ring identities across $k \in \lbrace 1, 2, 4, 8, 16, 32, 64 \rbrace$; complete negative §2.5 `InvariantError` + `WidthError` + `EvalError` rejection matrix; `BitParallel64State` == 64 `ScalarState` lanes across 100 random circuits $\times$ 1,000 ticks.
  - **G2 (Analytical & Synthesis Yardsticks):** $\lvert C_\mu - C_\mu^{\text{analytic}} \rvert < 10^{-9}$ and $\lvert h_\mu - h_\mu^{\text{analytic}} \rvert < 10^{-9}$ across **all** `T1`, `T3b`, and small `T3c` generators at $k \in \lbrace 1, 2 \rbrace$ under both $\varepsilon = 0$ and $\varepsilon = 0.05$; Reeds–Sloane 0 post-$2L$ error at $k = 1$ ($L \le 12$) and $k = 2$ ($L \le 8$, zero-divisor taps); independent witness cross-check on small-circuit exact synthesis.
  - **G3-rev1 (T1 Baselines Across Widths $k \in \lbrace 1, 2 \rbrace$):**
    - *Noiseless ($\varepsilon = 0$, all 4 `T1` tasks):* `Exhaustive` and `CTW` achieve `0` post-transient errors on `100%` of cases; `Exhaustive` matches `CertifiedExact` minimum node count (`excess_nodes == 0`), obeys `effective_state_registers >= min_delay_registers` everywhere, and matches `effective_state_registers == min_delay_registers` on all tasks except `k = 2` `[0, 1, 3]` (where both the 3-node/2-register linear witness and 1-register nonlinear witness are verified); `CGP` achieves `0` post-transient errors on $\ge 19/20$ seeds per task.
    - *Noisy ($\varepsilon = 0.05$, all 4 `T1` tasks evaluated and reported per seed):* Universal probabilistic `CTW` (`context_depth = 9`, $T = 20{,}000$ post-warmup ticks) achieves paired oracle regret $\le 0.030\text{ bits/symbol}$ on $\ge 19/20$ seeds, mean paired regret $\le 0.028\text{ bits/symbol}$, and population mean excess over $h_\mu \le 0.030\text{ bits/symbol}$ across **all 4 `T1` tasks** at $k = 1, 2$. `CGP` achieves paired residual regret $\le 0.020\text{ bits/symbol}$ on $\ge 18/20$ seeds (and mean excess $\le 0.020\text{ bits/symbol}$) on `T1_Constant` and `T1_Alternating` at $k = 1, 2$, while its per-seed outcomes on multi-register noisy `T1_PeriodicMotif(p=3)` and `T1_AffineLag1(p=4)` are recorded transparently in `case_reports` as baseline characterization data for `M1b/M2`.
  - **G4 (Cross-Process Reproducibility):** Spawns two separate OS child processes (`arkhe run --config ...`) across a 6-configuration matrix ($k \in \lbrace 1, 2 \rbrace$, `Combinational` & `TicksPerHop`, noiseless & $\varepsilon = 0.05$, `Cgp` / `RandomSearch` / `Ctw` / `ReedsSloane` / `Exhaustive`) and verifies byte-for-byte identical `RunReport` JSON outputs plus `ChaCha8Rng` golden vectors.
- **Implementation Tasks:**
  1. Add `scripts/build.rs` (`build = "scripts/build.rs"` in `Cargo.toml`) emitting deterministic compile-time env vars (`ARKHE_RUSTC_VERSION`, `ARKHE_TARGET`, `ARKHE_CARGO_LOCK_FNV1A128`, `ARKHE_GIT_SHA`, `ARKHE_GIT_DIRTY`, gracefully returning `"none"` inside `cargo package` tarballs without `.git`).
  2. Refactor `src/verify.rs` to execute the full `G1`, `G2`, `G3-rev1`, and subprocess `G4` suites, populating per-case and per-seed pass counts, means, and max regrets in `M0VerificationSummary`.
  3. Append an explicit **Amendment Record (`G3 -> G3-rev1`)** in `doc/milestone_0_plan.md` (preserving original text), update `doc/milestone_0_lessons_learned.md` (replacing the imprecise `"40/40 seeds"` phrasing and documenting the phase-exact `ObservationWindow` and `k = 2` state-vs-size theorem), and update `README.md` CLI examples.
  4. Execute closeout verification once on development seeds (`--seed-offset 0 --seeds 20`) and once on **untouched holdout seeds (`--seed-offset 10000 --seeds 20`)**, plus `cargo package --allow-dirty` archive inspection.

---

## 4. Closeout Verification & Exit Gate

Milestone 0 closeout is complete when every command below succeeds cleanly with **zero post-inspection tuning on `--seed-offset 10000`**:

```bash
# 1. Formatting, strict lints, and unit + integration tests
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked

# 2. Full G1-G4 battery on original seeds (0..20) and reserved holdout seeds (10000..10020)
cargo run --locked --release -- verify-m0 --seed-offset 0 --seeds 20
cargo run --locked --release -- verify-m0 --seed-offset 10000 --seeds 20

# 3. Package verification (ensuring scripts/build.rs works cleanly outside .git)
cargo package --allow-dirty

# 4. Documentation lint conformance
npx markdownlint-cli2 "doc/**/*.md" README.md
```

- **Exit Criteria:**
  - Both `verify-m0` runs exit `0` with `all_passed == true`, full `BuildProvenance` populated, and per-case/per-seed tables emitted in JSON.
  - `yardstick` rejects invalid `--epsilon` values, never substitutes `0.0` for uncomputed $h_\mu$, and labels prefix vs. certified full-cycle metrics explicitly.
  - `run` rejects unsupported non-`WellMixed` topologies rather than emitting mislabeled reports.
  - `cargo package --allow-dirty` builds and tests cleanly from the packaged crate archive.
