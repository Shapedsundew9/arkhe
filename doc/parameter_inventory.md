# Parameter & Constant Inventory

**Status:** Canonical inventory (Rule 2 of [`.github/copilot-instructions.md`](../.github/copilot-instructions.md), migrated from retired `doc/coordinate_free_scaling_plan.md` §2.1 under Readiness `C4`; see [research_record.md](research_record.md#12-archival-consolidation-of-superseded-milestone-1--pre-m2-plans--reviews)).  
**Discipline:** Every tunable scalar, bound, threshold, numerical clamp, or statistical policy in `src/structural/` and `src/verify_m1/` is classified here as **Derived**, **Pre-registered**, **Search bound**, **Compute guardrail**, or **Numerical guard**, with observable telemetry when any limit binds and concrete verification evidence.

---

## 1. Active Parameters, Bounds, and Guards

| Constant | Value | Config / Code path | Class | Derivation / justification link | Telemetry counter | Sensitivity / empirical evidence | Implementation status | Evidence |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| `reserve_cap` ($E_{\max}$) | `ReserveCap::HorizonDerived`, $W_{\text{obs}} = t_{\text{juv}} + 2 W_{\text{diag}}$ (`256` at defaults) | `EconomyConfig::reserve_cap` ([`economy.rs`](../src/structural/economy.rs)), `observation_horizon` ([`growth.rs`](../src/structural/growth.rs)) | Derived | [Charter §3.1.2](foundational_research_charter_v3.md#312-metabolism), DR #31 (`Q1(a)`) | `ReserveClippedCount` (`reserve_clipped_count`), `ReserveClippedBits` (`reserve_clipped_bits`) | [§3.2 `B6` latency distribution](#32-b6-multi-seed-regime-shift-latency-distribution) (`143` ticks on `Alt->Const`, `239..399` ticks / `1.559x W_obs` under `eps=0.05`) | Implemented | Commits `a47ff8d`, `22954df`; `cargo test --test regime_shift`; `structural::economy::tests::horizon_derived_reserve_cap_grounds_ceiling_from_mismatch_loss` |
| `mismatch_loss_floor` | `1e-12` | `RESERVE_MISMATCH_DENOM_FLOOR` ([`economy.rs`](../src/structural/economy.rs)) | Numerical guard | Prevents $\log_2((1-p_{\text{null}})/(1-q))$ division-by-zero when $q \to 1$ ([`economy.rs`](../src/structural/economy.rs)) | `ReserveCapMismatchFloored` (`reserve_mismatch_floored`) | Never triggered across 100-bank `verify-m1` sweep (`0` across `75.36M` reserve clips) | Implemented | Commit `a47ff8d`; [`verify-m1` §3.4](#34-a5-m1-verification-results-commit-7649e2c); `cargo test --locked` |
| `max_nodes` | `32` | `GrowthConfig::max_nodes` ([`growth.rs`](../src/structural/growth.rs)) | Compute guardrail | [§2.1 Guardrail derivation](#21-parameter-scaling-derivations--deferred-rules), DR #33 (`Q3`) | `SproutBlockedMaxNodes` (`sprout_blocked_max_nodes`, marks M1b case `Excluded`) | [§3.1 `B5` scaling profile](#31-b5-compute--counter-profile-phase-0) (`K=8` endogenous solves in `18` nodes `< 32`; noisy `N=32` blocks cleanly with `0.00` eval ticks wasted) | Implemented | Commit `a47ff8d`; `structural::growth::tests::b4_max_nodes_guardrail_counts_once_without_wasting_budget`; `verify_m1::m1b::tests::b3_guardrail_binding_marks_case_excluded_and_fails_gate` |
| `obs_capacity` ($W_{\text{obs}}$) | `juvenile_ticks + 2 * diagnostic_window` (`256` at defaults) | `observation_horizon` ([`growth.rs`](../src/structural/growth.rs), used in [`economy.rs`](../src/structural/economy.rs), [`learner.rs`](../src/structural/learner.rs), [`regime_shift.rs`](../tests/regime_shift.rs)) | Derived | [§2.1 Horizon derivation](#21-parameter-scaling-derivations--deferred-rules), DR #31 | `CandidateUndecidable` (`candidate_undecidable`) | Verified sufficient on M1 `NoisyAlternating` ($W^* \approx 152.9$) and [`B6`](#32-b6-multi-seed-regime-shift-latency-distribution) | Implemented | Commits `a47ff8d`, `22954df`; single shared `observation_horizon` across economy, learner, and tests |
| `max_reactive_lag` ($\tau_{\max}$) | `3` | `GrowthConfig::max_reactive_lag` ([`growth.rs`](../src/structural/growth.rs)) | Search bound | Reclassified per DR #32 (`Q2(a)`); iterative deepening deferred to M2 | `LagHorizonBound` (`lag_horizon_bound`) | Nyquist `W_diag / 2 = 32` lags addressable in buffer; bounded to `3` in M1 (`82,000` hits across 100-bank M1b, bypassed by rings) | Implemented | Commit `a47ff8d`; `structural::growth::tests::search_bound_telemetry_detects_lag_horizon_candidate_cap_and_ring_period` |
| `candidate_feature_cap` | `6` | `GrowthConfig::candidate_feature_cap` ([`growth.rs`](../src/structural/growth.rs)) | Search bound | Reclassified per DR #32 (`Q2(a)`); bounds ranked passing features evaluated per sprout | `CandidateListTruncated` (`candidate_list_truncated`) | Deferred to M2 plan alongside `B5` profile (`89,172` truncations observed across 100-bank M1b) | Implemented | Commit `a47ff8d`; `structural::growth::tests::search_bound_telemetry_detects_lag_horizon_candidate_cap_and_ring_period` |
| `min_ring_period`, `max_ring_period` | `2..=6` | `GrowthConfig::min_ring_period`, `GrowthConfig::max_ring_period` ([`growth.rs`](../src/structural/growth.rs)) | Search bound | Reclassified per DR #32 (`Q2(a)`); Nyquist ceiling $W_{\text{diag}}/2 = 32$, bounded to `2..=6` | `RingPeriodBound` (`ring_period_bound`) | `31` hits observed across 100-bank M1b (`0` on `0..20` and `20000..=20019`) | Implemented | Commit `a47ff8d`; `structural::growth::tests::search_bound_telemetry_detects_lag_horizon_candidate_cap_and_ring_period` |
| `mdl_tie_break_eps` | `1e-9` | `MDL_TIE_BREAK_EPS` ([`growth.rs`](../src/structural/growth.rs)) | Numerical guard | Deterministic tie-breaking on $\Delta J$ within floating-point rounding | None (order-preserving determinism) | Audited non-test literal in `src/structural/growth.rs` | Implemented | Commit `a47ff8d`; [`growth.rs`](../src/structural/growth.rs) |
| `sensitivity_sample_count` | `8` | `GrowthConfig::sensitivity_sample_count` ([`growth.rs`](../src/structural/growth.rs)) | Pre-registered | Sampled counterfactual perturbations per tick when $k > 4$ (`EXACT_SENSITIVITY_MAX_BITS`); exact below ([Charter §3.1.3](foundational_research_charter_v3.md#313-credit-assignment-retrograde-activity-gated-flow)) | Deterministic budgeted trace (exact at $k \le 4$) | Covered across M1a/M1b $k=1,2,8$ panel & `B5` sweep | Implemented | Commit `a47ff8d`; moved into `GrowthConfig` and `ArkheLearner` |
| `EXACT_SENSITIVITY_MAX_BITS` | `4` | `EXACT_SENSITIVITY_MAX_BITS` ([`sensitivity.rs`](../src/structural/sensitivity.rs), [`mod.rs`](../src/structural/mod.rs)) | Search / compute bound | Enumerates all $2^k \le 16$ input patterns exhaustively for $k \le 4$; switches to sampled perturbations (`sensitivity_sample_count`) for $k > 4$ | Deterministic sampled trace when $k > 4$ | Exercised by $k=8$ `verify-m1a` `G1A.1` AddConst1 check | Implemented | Commit `a47ff8d` (promoted from literal `4`); `cargo test --locked` |
| `RING_SIM_SENSITIVITY_SAMPLES` | `4` | `RING_SIM_SENSITIVITY_SAMPLES` ([`growth.rs`](../src/structural/growth.rs), [`mod.rs`](../src/structural/mod.rs)) | Search bound | Counterfactual sensitivity sample budget during `simulate_ring_juvenile_survival` | Deterministic internal replay | Exercised in autonomous delay ring selection (`G1B.2`, `G1B.4a`) | Implemented | Commit `a47ff8d`; `structural::learner::tests::sparse_rare_event_ring_discovery_and_mdl_sign_convention` |
| `SMALL_CONST_CANDIDATE_CAP` | `3` | `SMALL_CONST_CANDIDATE_CAP` ([`growth.rs`](../src/structural/growth.rs), [`mod.rs`](../src/structural/mod.rs)) | Search bound | Caps distinct synthetic nonzero constant candidates per sparse discrete residual | Candidate enumeration bound | Audited non-test literal in [`growth.rs`](../src/structural/growth.rs) | Implemented | Commit `a47ff8d` (promoted from literal `3`); `cargo test --locked` |
| `juvenile_ticks` | `128` | `EconomyConfig::juvenile_ticks` ([`economy.rs`](../src/structural/economy.rs)) | Pre-registered (M1) | [Charter §3.1.3](foundational_research_charter_v3.md#313-credit-assignment-retrograde-activity-gated-flow), [Research record §2.5](research_record.md#25-redesigned-m1a--milestone-1-attainment-sign-offs-2026-11-072026-11-08) | N/A (per-node countdown) | Validated across `0..20`, `20..2020`, and `20000..=20019` | Implemented & frozen for M1 | Commit `7649e2c` ([§3.4 `A5`](#34-a5-m1-verification-results-commit-7649e2c)) |
| `diagnostic_window` ($W_{\text{diag}}$) | `64` | `GrowthConfig::diagnostic_window` ([`growth.rs`](../src/structural/growth.rs)) | Pre-registered (M1) | [§2.1 M5 estimator scaling](#21-parameter-scaling-derivations--deferred-rules), M5 scaling planned | Per-port `DiagnosticBuffer` | Validated across `0..20`, `20..2020`, and `20000..=20019` | Implemented | Commit `7649e2c` ([§3.4 `A5`](#34-a5-m1-verification-results-commit-7649e2c)) |
| `sprout_interval` | `32` | `GrowthConfig::sprout_interval` ([`growth.rs`](../src/structural/growth.rs)) | Pre-registered (M1) | Cadence between per-port residual diagnosis attempts | N/A (per-port cadence) | Validated across `0..20`, `20..2020`, and `20000..=20019` | Implemented | Commit `7649e2c` ([§3.4 `A5`](#34-a5-m1-verification-results-commit-7649e2c)) |
| `mi_threshold_bits`, `min_match_gain` | `0.05`, `4` | `GrowthConfig::mi_threshold_bits`, `GrowthConfig::min_match_gain` ([`growth.rs`](../src/structural/growth.rs)) | Pre-registered (M1) | Residual diagnosis gate; M5 prequential replacement planned | `CandidateUndecidable` / below-threshold rejections | Validated across `0..20`, `20..2020`, and `20000..=20019` | Implemented | Commit `7649e2c`; `cargo test --test m1b_neurogenesis` |
| `initial_energy` | `4.0` | `EconomyConfig::initial_energy` ([`economy.rs`](../src/structural/economy.rs)) | Pre-registered (M1) | Initial juvenile endowment bits per placed node | Exact conservation in [`live.rs`](../src/structural/live.rs) | Validated across M1a/M1b 100-bank & confirmation sweeps | Implemented | `structural::live::tests::conservation_holds_every_tick`; [§3.4 `A5`](#34-a5-m1-verification-results-commit-7649e2c) |
| `trace_discount` ($\gamma$) | `0.90` | `EconomyConfig::trace_discount` ([`economy.rs`](../src/structural/economy.rs)) | Pre-registered (M1) | [Charter DR #26](foundational_research_charter_v3.md#appendix-a-decision-record) | Bounded trace clamp $\le 1.0$ | Validated on detour/lagged M1a cases | Implemented | `structural::economy::tests::recency_update_and_sparse_corrector_weight_floor`; [§3.4 `A5`](#34-a5-m1-verification-results-commit-7649e2c) |
| Miller–Madow alphabet support | Empirical support | [`score_feature_group`](../src/structural/growth.rs) | Derived | Empirical joint & residual support (M1b hardening) | None (analytic bias subtraction) | Verified in M1b noisy-constant halting | Implemented | `structural::learner::tests::noisy_nonzero_constants_halt_at_zero_nodes_without_thrashing` |
| KT prior mass in $\hat{q}$ | $\frac{1}{2} \cdot 2^k$ pseudo-counts | [`point_forecast.rs`](../src/metrics/point_forecast.rs) | Derived | Symbol-level Krichevsky–Trofimov prior matching $p_{\text{null}} = 2^{-k}$ | None (analytic identity) | Verified martingale bound | Implemented | `metrics::point_forecast::tests::martingale_property_exact_expectation_under_null_model` |
| Stage-gate pooled statistics | 99% Clopper–Pearson + 1% $\chi^2$ homogeneity | [`verify_m1/stats.rs`](../src/verify_m1/stats.rs) | Derived | [§2.1 Pooled bank statistics](#21-parameter-scaling-derivations--deferred-rules), DR #39 (`Q9`) | Bank mean, std, 5th percentile, $\chi^2$ p-value | Exact quantile cross-checks + `100/100` dev banks (`CP99 >= 0.948396`, $\chi^2=0.0, p=1.0$) | Implemented | `verify_m1::stats::tests::multi_bank_evaluation_homogeneous_vs_clustered`; [§3.4 `A5`](#34-a5-m1-verification-results-commit-7649e2c) |

### 1.1 Verification & Profiler Harness Constants (`src/verify_m1/`)

- `DEFAULT_HORIZON` = `1024`, `HOLDOUT_SEED_START` = `10_000`, `HOLDOUT_SEED_END` = `10_019` ([`src/verify_m1/mod.rs`](../src/verify_m1/mod.rs)): Pre-registered M1a panel horizon & untouched M1 holdout guard (`verify_m1::tests::holdout_seed_range_guard_rejects_overlaps_without_flag`).
- `DEFAULT_M1B_HORIZON` = `2000` ([`src/verify_m1/m1b.rs`](../src/verify_m1/m1b.rs)): Pre-registered M1b online horizon; `M2` holdout range `30_000..=30_019` reserved per DR #38 (`Q8`).
- `B5` Phase 0 profiler grid constants ([`src/verify_m1/profile.rs`](../src/verify_m1/profile.rs)): `DEFAULT_PROFILE_HORIZON = 512`, `DEFAULT_ENDOGENOUS_HORIZON = 1_024`, `ENDOGENOUS_WARMUP_TICKS = 500`, `PROFILE_MAX_EVAL_TICKS = 500_000`, `PROFILE_NODE_SCALES = [0, 2, 4, 8, 16, 24, 32]`, `PROFILE_CHANNEL_SCALES = [1, 2, 4, 8]`, `PROFILE_DIAGNOSTIC_EPSILON = 0.15`, `ENDOGENOUS_NOISE_EPSILON = 0.05`.

---

## 2. Structural Mechanics & Retired Baseline Items

- **Multi-channel isolation (`PortDevelopmentalState`, [`tests/multi_channel_development.rs`](../tests/multi_channel_development.rs)):** **Implemented (`a47ff8d`, `22954df`).** Replaces global juvenile freeze and port-0 snapshot hardcoding ([§2.1 Per-port development](#21-parameter-scaling-derivations--deferred-rules), DR #34 `Q4`). `port_epoch_snapshots(port)` keys snapshots strictly per port (`Some` for `0..K`, `None` out of range); [§3.3](#33-b7-multi-channel-lock-time-ratio-measurements) reports empirical solo vs concurrent lock-time ratios across `K=2` and `K=3`.
- **Retired spatial parameters:** `GridShape` (`grid_rows`, `grid_cols`), `max_relay_hops`, `Coord`, `PerimeterSide`, `BoundaryAnchor`, `wiring_topology` were deleted in commit `deb200a` under Charter Decision Record #30 (historical reproduction preserved at tag `m1-final-lattice` `2d009b0`).
- **Retired fixed cap:** `max_energy_reserve = 25.0` was superseded by `ReserveCap::HorizonDerived` in commit `deb200a` (with Phase 1–2 golden equivalence under `ReserveCap::Fixed(25.0)` verified in [research_record.md §7.0](research_record.md#70-readiness-a1-phase-12-bit-identity-proof)).

### 2.1 Parameter Scaling Derivations & Deferred Rules

Durable mathematical derivations consolidated from retired `doc/coordinate_free_scaling_plan.md` §§2.2–2.8:

- **Horizon-derived reserve ceiling (`ReserveCap::HorizonDerived`, DR #31):** Under `CalibratedPointForecast`, mismatch loss is $\log_2((1-q)/(1-p_{\text{null}}))$ (set by empirical predictor confidence $q$, not $O(k)$ width; width enters hit reward $\log_2(q/p_{\text{null}})$ via $p_{\text{null}} \sim 2^{-k}$). Earlier drafts proposing $k / \ln 2$ or debt multiplier $\beta = 2.0$ ("recovering `25.0`") were rejected as spurious unit conversions and post-hoc fitting. Option B sets $E_{\max}(v) = \max(E_{\text{init}}, W_{\text{obs}} \sum_p \text{share}(v, p) \log_2((1-p_{\text{null}}(p))/(1-\hat{q}_p)))$ with no free multiplier, where `1 - q` is guarded by `RESERVE_MISMATCH_DENOM_FLOOR = 1e-12`. Because $\hat{q}_p$ falls as mismatches accumulate and partial matches still pay reward, $E_{\max}(v)$ does not guarantee starvation in $\le W_{\text{obs}}$ ticks universally (see [§3.2](#32-b6-multi-seed-regime-shift-latency-distribution)).
- **Compute guardrail (`max_nodes`, DR #33):** Scaling $N_{\max} = K \times 16$ was rejected because it introduced an ungrounded per-channel constant and would halve M1's limit (`32 -> 16`) at $K = 1$. Growth is governed by MDL debt and calibrated flux; `max_nodes` exists solely to bound runtime, is checked once at proposal time (`node_count + motif_size <= max_nodes`), and marks any run where `SproutBlockedMaxNodes > 0` as `Excluded`.
- **Per-port cone-local maturity (`PortDevelopmentalState`, DR #34):** Each output sink $p$ maintains its own `DiagnosticBuffer`, sprout cadence timer, primed check, and cone-local maturity predicate $\text{CanSprout}(p) \iff \forall v \in \text{UpstreamCone}(p),\ \text{juvenile}(v) = 0$. Juvenile node outputs are excluded from every port's candidate feature pool (`build_signal_pool`), structural edits reset only ports where `port_cone_changed` holds, and sprout checks rotate starting port via `tick mod K` (reducing identically to M1 at $K = 1$).
- **Unified observation horizon ($W_{\text{obs}}$, DR #31):** Both `ObservationWindow` capacity and `HorizonDerived` reserve bounding call `observation_horizon(&econ, &growth) = t_juv + 2 * W_diag` (`256` at defaults). Note $W_{\text{obs}} \ge W^* = \Delta L / \Delta I$ holds for M1 `NoisyAlternating(k=1)` ($W^* \approx 152.9$) but is not a universal theorem across arbitrary motif debts or noise levels; `CandidateUndecidable` counts candidates whose crossover exceeds `obs_window`.
- **Miller–Madow support & M5 estimator scaling:** `score_feature_group` uses observed empirical joint and residual support. However, empirical support does not prevent plug-in bias from exceeding entropy at large alphabets (e.g., at $N = 64$ with all-distinct symbols, $(63 \cdot 63)/(2 \cdot 64 \ln 2) \approx 44.7$ bits vs. $\log_2 64 = 6$ bits). M5 will evaluate (1) scaling $W_{\text{diag}}$ with observed joint support vs. (2) replacing (`mi_threshold_bits`, Miller–Madow, `min_match_gain`) with a single prequential KT code-length gain test.
- **Iterative lag & candidate search deepening (DR #32, deferred to M2):** Reactive lag $\tau_{\max}$, `candidate_feature_cap`, and ring period range are search bounds rather than model constraints (since materialized `Delay` nodes pay exact $L(C)$). When expanded in M2+, iterative deepening ($\tau_{\max} = 1, 2, 4, \dots$ up to `TickBudget` exhaustion or Nyquist $W_{\text{diag}}/2$) triggers only when the $\tau \le 3$ pass yields no qualifying candidate, preserving M1 reduction on short-lag tasks.
- **Pooled multi-bank acceptance (`verify_m1/stats.rs`, DR #39):** Compound per-bank hurdles reject reliable learners across many banks (e.g., $\Pr(\text{Bin}(20, 0.95) \ge 18) = 0.9245 \implies 0.9245^{100} \approx 0.04\%$). Multi-bank gates require (1) the one-sided 99% Clopper–Pearson lower confidence bound over pooled $N = M \times B$ seeds to meet $P_{\text{target}}$ (with no ad hoc slack $\epsilon$) and (2) a 1% $\chi^2$ across-bank homogeneity check to reject clustered failures.

---

## 3. Compute Profile & Measurement Tables

All binaries built from clean git commit `7649e2caa38e78575381c03d8e6952118a816fc6` (`dirty=false`), `rustc 1.99.1 (58cf8f9c3 2026-10-13)`, `x86_64-unknown-linux-gnu`, **`source_tree_fnv1a128 = 26e9d332905dd6a6aeadce50c5266fa3`** (16 CPU cores).

### 3.1 `B5` Compute & Counter Profile (Phase 0)

Generated via `python3 scripts/measure_compute_profile.py --skip-build --with-m1-gates --seed-offset 0 --seeds 5 --gate-seeds 20` (`seed_offset=0`, `seeds=5`, `horizon=512`, `endogenous_horizon=1024`, `max_eval_ticks=500000`).

#### Controlled `steady_clean` (`us/tick` mean [eval_ticks/tick])

| $N$ | $K=1$ | $K=2$ | $K=4$ | $K=8$ |
| ---: | ---: | ---: | ---: | ---: |
| 0 | 2.04 [0.00] | 9.90 [0.00] | 66.96 [0.00] | 516.97 [0.00] |
| 2 | 11.40 [0.00] | 25.80 [0.00] | 100.89 [0.00] | 600.07 [0.00] |
| 4 | 16.05 [0.00] | 35.10 [0.00] | 127.37 [0.00] | 689.24 [0.00] |
| 8 | 29.30 [0.00] | 60.18 [0.00] | 189.54 [0.00] | 887.32 [0.00] |
| 16 | 67.10 [0.00] | 126.45 [0.00] | 336.89 [0.00] | 1311.10 [0.00] |
| 24 | 131.12 [0.00] | 219.85 [0.00] | 527.85 [0.00] | 1747.49 [0.00] |
| 32 | 206.86 [0.00] | 344.59 [0.00] | 756.10 [0.00] | 2293.29 [0.00] |

#### Controlled `diagnostic_noise` ($\varepsilon = 0.15$ on all $K$ channels, `us/tick` mean [eval_ticks/tick])

| $N$ | $K=1$ | $K=2$ | $K=4$ | $K=8$ |
| ---: | ---: | ---: | ---: | ---: |
| 0 | 8.45 [0.00] | 32.66 [1.00] | 159.26 [4.25] | 933.95 [56.33] |
| 2 | 17.83 [0.00] | 50.83 [0.80] | 203.52 [4.75] | 1067.56 [53.98] |
| 4 | 25.02 [0.80] | 65.92 [3.15] | 245.09 [8.60] | 1190.65 [75.71] |
| 8 | 38.77 [3.30] | 92.39 [4.10] | 309.40 [14.34] | 1404.75 [89.55] |
| 16 | 79.41 [0.80] | 164.36 [2.40] | 476.13 [22.20] | 1874.07 [130.85] |
| 24 | 138.70 [0.00] | 264.80 [4.25] | 687.29 [18.61] | 2424.47 [117.76] |
| 32 | 230.62 [0.00] | 408.93 [0.00] | 961.34 [0.00] | 3067.05 [0.00] |

*(Notice at $N=32$ under `diagnostic_noise`, `sprout_blocked_max_nodes` fired `160` ($K=1,2$), `483` ($K=4$), and `3055` ($K=8$) times while `eval/tick` stayed `0.00` — proving the single pre-check avoids wasting replay budget when saturated.)*

#### Endogenous Online Workloads ($T=1024$, `warmup=500`, seeds `0..5`)

| Task | $k$ | $K$ | $\varepsilon$ | us/tick (mean/max) | eval/tick | final_nodes | post500_err | sprout/ring/starve/idle | reserve_clip_cnt | reserve_clip_bits | mismatch_floored | blocked_max_nodes | cand_undecidable | cand_truncated | lag_bound | ring_bound |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | :---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `K=1:Motif011(k=1,clean)` | 1 | 1 | 0.00 | 12.57 / 12.61 | 11.75 | 4 | 0 | 10/0/0/0 | 1070 | 582.45 | 0 | 0 | 0 | 10 | 0 | 0 |
| `K=1:Motif013(k=2,clean)` | 2 | 1 | 0.00 | 14.71 / 14.74 | 15.38 | 5 | 0 | 10/0/0/0 | 2135 | 1529.54 | 0 | 0 | 0 | 10 | 0 | 0 |
| `K=1:Motif013(k=2,eps=0.05)` | 2 | 1 | 0.05 | 16.62 / 20.65 | 18.48 | 4 | 0 | 9/5/0/0 | 124 | 370.95 | 0 | 0 | 1 | 10 | 0 | 0 |
| `K=2:(Alt,Motif011)(k=1,clean)` | 1 | 2 | 0.00 | 43.89 / 43.96 | 16.75 | 5 | 0 | 15/0/0/0 | 1070 | 564.23 | 0 | 0 | 10 | 15 | 0 | 0 |
| `K=4:(4 motifs)(k=1,clean)` | 1 | 4 | 0.00 | 210.68 / 211.51 | 16.36 | 7 | 0 | 20/0/0/0 | 1040 | 1088.85 | 0 | 0 | 30 | 20 | 0 | 0 |
| `K=8:(8 motifs)(k=1,clean)` | 1 | 8 | 0.00 | 1329.00 / 1330.72 | 49.83 | 18 | 0 | 45/5/0/0 | 4020 | 2371.74 | 0 | 0 | 50 | 50 | 0 | 0 |

#### Aggregated Cliff & Bound Counter Totals (`B5` + `dev 0..20`)

| Suite | reserve_clip_cnt | reserve_clip_bits | mismatch_floored | blocked_max_nodes | cand_undecidable | cand_truncated | lag_bound | ring_bound |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `profile:controlled_sweep` | 53376 | 362.46 | 0 | 3871 | 601 | 81 | 0 | 0 |
| `profile:endogenous_runs` | 9459 | 6507.76 | 0 | 0 | 91 | 115 | 0 | 0 |
| `verify-m1a` (seeds `0..20`, `all_passed=true`) | 0 | 0.00 | 0 | 0 | 0 | 0 | 0 | 0 |
| `verify-m1b` (seeds `0..20`, `all_passed=true`, `excluded=0`) | 752272 | 498168.74 | 0 | 0 | 260 | 893 | 820 | 0 |

---

### 3.2 `B6` Multi-Seed Regime-Shift Latency Distribution

Measured via `cargo test --locked --test regime_shift -- --test-threads=1 --nocapture` (`2 passed; 0 failed`), with unified `W_obs = observation_horizon(&econ, &growth) = 256`:

- **Single-seed trace (`shift_tick=350`, `Motif5 -> Alt(k=1,clean)`):** `W_obs=256`, `starve_tick=Some(543)`, `delta_starve=Some(193)`, `resprout_tick=Some(543)`, `clipped_count=55`, `clipped_bits=36.749`.
- **Multi-seed distribution (seeds `0..10`):**

| Pair | Seeds | $W_{\text{obs}}$ | Starved | Resprouted | Within $\le W_{\text{obs}}$ | Latency min/p50/mean/p95/max | Max/$W_{\text{obs}}$ | ClippedCount mean | ClippedBits mean |
| :--- | ---: | ---: | ---: | ---: | ---: | :--- | ---: | ---: | ---: |
| `Motif5->Alt(k=1,clean)` | 10 | 256 | 10/10 | 10/10 | 10/10 | 207 / 207.0 / 207.0 / 207.0 / 207 | 0.809x | 57.0 | 57.2 |
| `Alt(0,1)->Constant(0)(k=1)` | 10 | 256 | 10/10 | 10/10 | 10/10 | 143 / 143.0 / 143.0 / 143.0 / 143 | 0.559x | 0.0 | 0.0 |
| `Motif013->Mod3(k=2,clean)` | 10 | 256 | 10/10 | 10/10 | 10/10 | 20 / 20.0 / 20.0 / 20.0 / 20 | 0.078x | 589.0 | 726.3 |
| `Motif5->Alt(k=1,eps=0.05)` | 10 | 256 | 10/10 | 10/10 | 2/10 | 239 / 294.0 / 307.2 / 355.0 / 399 | 1.559x | 15.5 | 18.0 |

---

### 3.3 `B7` Multi-Channel Lock-Time Ratio Measurements

Measured via `cargo test --locked --test multi_channel_development -- --nocapture` (`3 passed; 0 failed`, seeds `0..10`):

| Setting | Channel | Locked | Solo p50/max | Multi p50/max | Ratio min/p50/mean/p95/max | MaxSnapshots |
| :--- | ---: | ---: | :--- | :--- | :--- | ---: |
| `K=2:(Alt,Motif011) Combinational` | ch0 | 10/10 | 64/64 | 64/64 | 1.00/1.00/1.00/1.00/1.00x | 1 |
| `K=2:(Alt,Motif011) Combinational` | ch1 | 10/10 | 255/255 | 255/255 | 1.00/1.00/1.00/1.00/1.00x | 3 |
| `K=2:(Alt,Motif011) TicksPerHop` | ch0 | 10/10 | 64/64 | 67/67 | 1.05/1.05/1.05/1.05/1.05x | 1 |
| `K=2:(Alt,Motif011) TicksPerHop` | ch1 | 10/10 | 66/66 | 66/66 | 1.00/1.00/1.00/1.00/1.00x | 1 |
| `K=3:(Alt,Motif011,Motif001) Combinational` | ch0 | 10/10 | 64/64 | 64/64 | 1.00/1.00/1.00/1.00/1.00x | 1 |
| `K=3:(Alt,Motif011,Motif001) Combinational` | ch1 | 10/10 | 255/255 | 65/65 | 0.25/0.25/0.25/0.25/0.25x | 1 |
| `K=3:(Alt,Motif011,Motif001) Combinational` | ch2 | 10/10 | 66/66 | 67/67 | 1.02/1.02/1.02/1.02/1.02x | 1 |
| `K=3:(Alt,Motif011,Motif001) TicksPerHop` | ch0 | 10/10 | 64/64 | 64/64 | 1.00/1.00/1.00/1.00/1.00x | 1 |
| `K=3:(Alt,Motif011,Motif001) TicksPerHop` | ch1 | 10/10 | 66/66 | 66/66 | 1.00/1.00/1.00/1.00/1.00x | 1 |
| `K=3:(Alt,Motif011,Motif001) TicksPerHop` | ch2 | 10/10 | 66/66 | 66/66 | 1.00/1.00/1.00/1.00/1.00x | 1 |

---

### 3.4 `A5` M1 Verification Results (Commit `7649e2c`)

All three runs executed on compiled Release binary `7649e2caa38e78575381c03d8e6952118a816fc6` (`source_tree_fnv1a128 = 26e9d332905dd6a6aeadce50c5266fa3`):

| Bank / Suite | Command | Outcome | Case / Bank Counts | `excluded_cases` | `sprout_blocked_max_nodes` | Wall time (16 cores) | Key Cliff / Bound Counters |
| :--- | :--- | :--- | :--- | ---: | ---: | ---: | :--- |
| Dev fast slice (`0..20`) | `target/release/arkhe verify-m1 --seed-offset 0 --seeds 20` | `all_passed: true`, `stage_gate_aborted: false` | `G1A.1`: `1959`, `G1A.2`: `1065`, `G1A.3`: `1575`, `G1A.4`: `1638`; `G1B.1`: `80`, `G1B.2`: `360`, `G1B.3`: `1104`, `G1B.4`: `666` (0 failures) | `0` | `0` | `26.159s` | `reserve_clipped_count=752,272` (`498,168.74` bits), `candidate_undecidable=260`, `candidate_list_truncated=893`, `lag_horizon_bound=820`, `ring_period_bound=0` |
| Dev multi-bank (`20..2020`, `100` banks × `20` seeds) | `target/release/arkhe verify-m1 --banks 100 --seed-offset 20 --seeds 20` | `all_passed: true`, `stage_gate_aborted: false` | M1a: **`100/100`** (`G1A.1`–`G1A.4` all `100/100`); M1b: **`100/100`** (`G1B.1`–`G1B.4` all `100/100`); 99% two-sided Clopper–Pearson LB `0.948396`, $\chi^2=0.0, p=1.0$ | `0` | `0` | `7m19.102s` | `reserve_clipped_count=75,363,346` (`49,838,263.89` bits), `reserve_mismatch_floored=0`, `candidate_undecidable=23,348`, `candidate_list_truncated=89,172`, `lag_horizon_bound=82,000`, `ring_period_bound=31` |
| Fresh confirmation bank (`20000..=20019`) | `target/release/arkhe verify-m1 --seed-offset 20000 --seeds 20` | `all_passed: true`, `stage_gate_aborted: false` | `G1A.1`: `1959`, `G1A.2`: `1065`, `G1A.3`: `1575`, `G1A.4`: `1638`; `G1B.1`: `80`, `G1B.2`: `360`, `G1B.3`: `1104`, `G1B.4`: `666` (0 failures) | `0` | `0` | `26.387s` | `reserve_clipped_count=754,924` (`498,522.64` bits), `reserve_mismatch_floored=0`, `candidate_undecidable=237`, `candidate_list_truncated=891`, `lag_horizon_bound=820`, `ring_period_bound=0` |

- **Holdout Seed Governance:** M1 holdout range `10000..=10019` was **never touched** during readiness; M2 holdout range `30000..=30019` remains reserved per DR #38 (`Q8`).

---

## 4. Pre-Registered Milestone 2a Additions (Approved `2026-11-13` / Finalized `2026-11-14`, Pending Implementation)

Approved in [`doc/milestone_2a_plan.md`](milestone_2a_plan.md#8-user-sign-off--attainment-record) (`S1`–`S5`, `S3-A`, `S4-A`, `S4-A.1`, `S4-A.2`, `C1-a`, `C2-a`, `G2A.2(b)`); moved into §1 upon Phase 2–4 implementation and verification:

| Constant / Rule | Pre-Registered Value | Planned Config / Code path | Class | Derivation / justification link | Telemetry counter | Implementation status |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| `deepening_max_lag` | `12` (incremental bands `3 -> 6 -> 12`, $\le W_{\text{diag}}/2$) | `GrowthConfig::deepening_max_lag` (`src/structural/growth.rs`) | Search bound | [M2a Plan §2.3 #4 & §6](milestone_2a_plan.md#6-parameter-inventory-changes), `S2`, `C2-a` | `LagDeepeningSteps` (`lag_deepening_steps`), `LagHorizonBound` | Approved (`S2`, `C2-a`); pending Phase 2 |
| Per-port replay budget slice | Derived cumulative share from `total_budget / num_ports` + bounded idle-port pool, clamped to `budget.remaining()` | `ArkheLearner::observe` (`src/structural/learner.rs`) | Derived | [M2a Plan §2.3 #2 & §6](milestone_2a_plan.md#6-parameter-inventory-changes), `S2` | `SproutBudgetExhausted` (`sprout_budget_exhausted`) | Approved (`S2`); pending Phase 2 |
| `CandidateMotif::NegAddConst(u, c)` | Materialized as `Sub(ConstLoop(c), u)` for $k > 1$ over existing `SMALL_CONST_CANDIDATE_CAP` + `constant_offset` | `CandidateMotif::NegAddConst` (`src/structural/growth.rs`) | Primitive motif expansion (zero new numeric constants) | [M2a Plan §2.3 #6 & §6](milestone_2a_plan.md#6-parameter-inventory-changes), `S3-A` | Existing sprout / `L(C)` accounting | Approved (`S3-A`); pending Phase 2 |
| Cross-channel Epoch-1 selection (`C1-a`) | `best_same` (`c == out_port`, `is_1_op` priority) vs. `best_cross` (`c != out_port`, pure `j_cand`), compared by `j_cand` | `propose_sprout_with_telemetry` (`src/structural/growth.rs`) | Pre-registered structural policy | [M2a Plan §2.3 #3 & §8.1](milestone_2a_plan.md#81-decisions-requested-for-pre-registration-sign-off), `C1-a` | Existing sprout / M1 golden traces | Approved (`C1-a`); pending Phase 2 |
| `TicksPerHop` causal arm-lag rule | Strict `edge.hops == 1` (no `edge.hops > 1` or dummy `Split` buffers); $d_{\text{arm}} = 1 + h_{\text{arm}} + 2 n_{\text{delay}}$ | `src/structural/growth.rs`, `src/verify_m2a/mod.rs` | Derived | [M2a Plan §2.3 #5 & §8.1.1](milestone_2a_plan.md#811-baseline-metering-and-causal-lag-sign-off), `G2A.2(b)` | Existing sprout / `wire_hop_register_count` | Approved (`2026-11-14`); pending Phase 2 |
| `M2A_HOLDOUT_SEED_START` / `END` | `30_000..=30_019` (`40_000..=40_019` reserved for `M2b`) | `src/verify_m2a/mod.rs` | Pre-registered | [M2a Plan §4](milestone_2a_plan.md#4-verification-protocol--seed-governance), DR #38, `S5` | CLI `--holdout` guard | Approved (`S5`); pending Phase 4 |
| `DEFAULT_M2A_HORIZON` / `WARMUP` / `BUDGET` | `2_000` / `500` / `200_000` | `src/verify_m2a/mod.rs` | Pre-registered | [M2a Plan §2.2 & §6](milestone_2a_plan.md#6-parameter-inventory-changes), `S2` | `Scorecard::candidate_replay_ticks_consumed` | Approved (`S2`); pending Phase 4 |
| `CrossChannelCtwBaseline` context bound & metering | `symbol_context_depth = 9`, `max_interleaved_bit_depth = 64` (`(9 * K * k).clamp(4, 64)`); `BudgetAccountingMode::ContextTreeDepthSteps` (`bit_depth` ticks/step in `observe()`, `0` in `predict()`) | `BaselineConfig::CrossChannelCtw` (`src/baselines/cross_ctw.rs`, `src/metrics/scorecard.rs`, `src/config.rs`) | Pre-registered (baseline only) | [M2a Plan §8.1.1](milestone_2a_plan.md#811-baseline-metering-and-causal-lag-sign-off), `S4-A.1` | `Scorecard::candidate_replay_ticks_consumed` | Approved (`S4-A.1`, `2026-11-14`); pending Phase 3 |
| `RnnBaseline` reference config, init, & metering | 1-layer `tanh` Elman (`hidden_size = 16`, `bptt_len = 16`, `lr = 0.05`, `TBPTT(1, 16)`), one-hot $K \cdot 2^k$, $K$ softmax heads (nats grad, log2 loss), Glorot uniform init, zero biases, **unclipped**; `BudgetAccountingMode::BpttUnrollSteps` (`min(t, bptt_len)` ticks/step in `observe()`, `0` in `predict()`) | `BaselineConfig::Rnn` (`src/baselines/rnn.rs`, `src/metrics/scorecard.rs`, `src/config.rs`) | Pre-registered / Derived init (baseline only) | [M2a Plan §8.1.1](milestone_2a_plan.md#811-baseline-metering-and-causal-lag-sign-off), Charter §8, `S4-A.2` | `Scorecard::candidate_replay_ticks_consumed` | Approved (`S4-A.2`, `2026-11-14`); pending Phase 3 |
| `MAX_RNN_ONE_HOT_WIDTH_BITS` | `8` ($2^k \le 256$ per channel) | `src/baselines/rnn.rs`, `src/config.rs` | Compute / width guard (baseline only) | [M2a Plan §6 & §8.1.1](milestone_2a_plan.md#811-baseline-metering-and-causal-lag-sign-off), `S4-A.2` | Deterministic `ConfigRunError` if $k > 8$ | Approved (`S4-A.2`, `2026-11-14`); pending Phase 3 |
