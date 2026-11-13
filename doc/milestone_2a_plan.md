# Milestone M2a: Cross-Channel Predictive Coupling Pre-Registration Plan

**Date:** 2026-11-13  
**Status:** `Draft awaiting user sign-off`  
**Charter Links:** [Foundational Research Charter v3](foundational_research_charter_v3.md), [Parameter Inventory](parameter_inventory.md), [Research Record](research_record.md)

## 1. Charter Hypotheses & Decision Rights

- **Hypotheses tested:**
  - **H2 (Cross-channel Tier T2 extension, Charter §5.3, §6, §7, DR #34–#37):** Online mutual-information residual diagnosis, causal delay recruitment, and finite-ring corrector stacking over coordinate-free `substrate::Circuit` discover cross-channel predictive couplings from boundary (`BoundaryIn(c)`, `LagIn(c, τ)`) and mature interior (`NodeOut`) signals across $K \ge 2$ channels at $k \in \{1, 2\}$, driving target-channel prediction loss to the irreducible entropy floor without gradients while maintaining $O(1)$ per-channel developmental lock-time scaling (`PortDevelopmentalState`).
- **Explicit non-goals / deferred hypotheses:**
  - **H3 (`Redundant derivations plus comparators`) is explicitly deferred to `M2b` and `M4a`** ([Charter DR #35 & #37](foundational_research_charter_v3.md#appendix-a-decision-record)). `M2a` solves `T2` strictly via boundary error-driven corrector sprouting (`SignalSink::BoundaryOut`) and makes **no** claim regarding spontaneous `EQZ(SUB(a, b))` comparator formation or wake redundancy retention. Planning `M2b` requires a separate theoretical & Charter comparator-economics phase first ([research_record.md §6.2](research_record.md#62-critical-open-issues--dropped-nuance-relevant-to-m2m5)).
  - **H4 (`Free-rider resistance`)** remains scoped to `M3`; **M5 estimator scaling** (support-scaled $W_{\text{diag}}$ or prequential KT feature scoring) remains scoped to `M5` ([parameter_inventory.md §2.1](parameter_inventory.md#21-parameter-scaling-derivations--deferred-rules)).
- **Decision rights:** Per [Charter §7.1](foundational_research_charter_v3.md#71-milestone-lifecycle-and-decision-rights) and [`.github/copilot-instructions.md`](../.github/copilot-instructions.md) Rules 5 and 7–11, all parameter additions/bounds, gate thresholds (`P_target`, margins), baseline protocol choices, seed bank ranges, and any golden fixture re-blessing require explicit user sign-off (§8) before confirmatory implementation begins.

---

## 2. Exploratory Characterisation Summary

Per Charter §7.1 Step 1 and Rule 8, non-invasive exploratory probes were executed on the unmodified post-readiness codebase (`source_tree_fnv1a128 = 26e9d332905dd6a6aeadce50c5266fa3`) strictly over development seeds `0..20` (`T = 2000`, `warmup = 500`, release profile, zero gating thresholds; holdout `10000..=10019`, M1 confirmation `20000..=20019`, and M2 holdout `30000..=30019` untouched).

### 2.1 Coupled vs. Marginal Solvability & Search-Bound Probe (`seeds 0..20`)

Driver channels used coprime maximal-length LFSRs (`deg 5` period `31` on `ch0`; `deg 4` period `15` on `ch1`; joint state count `465 <= 65,536`, verified via `solve_epsilon_machine`: $C_\mu = 8.8611$ bits, $h_\mu = 0.0$, `exact_sync_order = Some(4)`), plus a high-degree follow-up (`deg 11` period `2047` $\times$ `deg 9` period `511`, `seeds 0..10`):

| Task | $k$ | WireDelay | Mode (`max_eval = 50k` unless noted) | Locked | CrossCh | Target CleanErr | Lock p50/max | Mean Nodes | us/tick | `lag_bound` | `cand_trunc` |
| :--- | -: | :--- | :--- | ---: | ---: | ---: | :--- | ---: | ---: | ---: | ---: |
| `DelayedAffine(d=1, +1)` | 1 | `Comb` | Coupled (`tau=3, cap=6`) | `20/20` | `20/20` | `0.0000` | `63 / 63` | `7.0` | `74.4` | `0` | `20` |
| `DelayedAffine(d=1, +1)` | 1 | `Comb` | Marginal $B$-only (`K=1`) | `0/20` | `0/20` | `0.1667` | `n/a` | `0.0` | `6.9` | `0` | `0` |
| `DelayedAffine(d=2, +1)` | 1 | `Comb` | Coupled (`tau=3, cap=6`) | `20/20` | `20/20` | `0.0000` | `65 / 65` | `6.0` | `67.7` | `0` | `20` |
| `DelayedAffine(d=2, +1)` | 1 | `Hop1` | Coupled (`tau=8, cap=12`) | `20/20` | `20/20` | `0.0000` | `64 / 64` | `3.0` | `174.3` | `0` | `20` |
| `DelayedAffine(d=4, +1)` | 1 | `Comb` | Coupled (`tau=3, cap=6`) | `20/20` | `20/20` | `0.0000` | `62 / 62` | `8.0` | `82.4` | `0` | `20` |
| `CrossSum(d=1, 2)` | 1 | `Comb` | Coupled (`tau=3, cap=6`) | `20/20` | `20/20` | `0.0000` | `63 / 63` | `3.0` | `274.5` | `20` | `520` |
| `CrossSum(d=1, 2)` | 1 | `Comb` | Marginal $B$-only (`K=1`) | `0/20` | `0/20` | `0.4987` | `n/a` | `0.0` | `31.5` | `40` | `0` |
| `CrossSum(d=1, 2)` | 1 | `Hop1` | Coupled (`tau=8, cap=12`) | `0/20` | `0/20` | `0.4980` | `n/a` | `0.0` | `281.9` | `520` | `1360` |
| `CrossSum(d=1, 2)` | 2 | `Comb` | Coupled (`tau=3, cap=6`) | `20/20` | `20/20` | `0.0000` | `67 / 67` | `10.0` | `461.2` | `60` | `1960` |
| `CrossSum(d=1, 2)` | 2 | `Comb` | Marginal $B$-only (`K=1`) | `0/20` | `0/20` | `0.7487` | `n/a` | `0.0` | `76.1` | `20` | `1220` |
| `CrossSum(d=2, 5)` | 1 | `Comb` | Coupled (`tau=3, cap=6`) | `0/20` | `0/20` | `0.4920` | `n/a` | `5.0` | `157.6` | `80` | `440` |
| `CrossMul(d=1, 2)` | 1 | `Comb` | Coupled (`tau=3, cap=6, B=50k`) | `0/20` | `0/20` | `0.2760` | `n/a` | `0.0` | `189.2` | `600` | `820` |
| `CrossMul(d=1, 2)` | 1 | `Comb` | Coupled (`tau=3, cap=12, B=500k`) | `10/10` | `10/10` | `0.0000` | `<400` | `4.0` | — | — | — |
| `CrossMulEqz(d=1, 2)` | 2 | `Comb` | Coupled (`tau=3, cap=6`) | `20/20` | `20/20` | `0.0000` | `67 / 67` | `6.0` | `421.2` | `80` | `1920` |
| `CrossMulEqz(d=1, 2)` | 2 | `Comb` | Marginal $B$-only (`K=1`) | `0/20` | `0/20` | `0.1540` | `n/a` | `0.0` | `21.1` | `0` | `0` |
| `CascadeAddEqz(d=1, 2)` | 2 | `Comb` | Coupled (`tau=8, cap=12`) | `0/20` | `20/20` | `0.2000` | `n/a` | `4.0` | `1248.8` | `0` | `3360` |

### 2.2 Baseline & Noise Characterisation (`seeds 0..20` / `0..10`)

- **High-degree LFSR (`deg 11 × deg 9`, `T = 2000`, `budget = 200,000`, `seeds 0..10`):**
  - `CrossSum(d=1, 2, k=2)`: `ArkheLearner` **`10/10` locked** (`CleanErr = 0.0000`, `4.0` nodes); `CGP(1+4)` `1/10` (`0.6702`); `CTW(per-ch, d=9)` `0/10` (`0.7360`); `ReedsSloane(per-ch)` `10/10` (`0.0000`, marginal linear complexity $11 + 9 = 20 \implies 2L = 40$ ticks).
  - `CrossMulEqz(d=1, 2, k=2)`: `ArkheLearner` **`10/10` locked** (`CleanErr = 0.0000`, `5.0` nodes); `CGP(1+4)` `1/10` (`0.1663`); `CTW(per-ch, d=9)` `0/10` (`0.1800`); `ReedsSloane(per-ch)` **`0/10` (`0.5853`)** because $\mathbb{Z}_4$ `EQZ` product gating breaks low-order linear recurrence.
- **Noisy `T2` regimes ($\varepsilon = 0.05$, `seeds 0..20`, `T = 2000`):**
  - **Target-channel noise only** (`channels = [target]`): `DelayedAffine(d=2, k=1)` achieved `CleanErr = 0.0000`, `ObsKT = 0.2934` vs. $H_{\text{sub}}(0.05, 1) = 0.2864$ (**`+0.0070` b/sym**); `CrossSum(d=1, 2, k=2)` achieved `CleanErr = 0.0000`, `ObsKT = 0.3669` vs. $H_{\text{sub}}(0.05, 2) = 0.3656$ (**`+0.0013` b/sym**).
  - **All-channel observation noise** (`channels = 0..K`, clean $B = f(A_{\text{clean}})$): `DelayedAffine(d=2, k=1)` observed `CleanErr = 0.0498` (matching exact theoretical input-flip rate $\varepsilon_A = 0.0500$) and `ObsKT = 0.4568` (`+0.1704` over single-channel `SubH`), confirming that reactive cross-channel maps reading $m$ noisy driver taps face a higher irreducible observation entropy than latent-state phase oracles.

### 2.3 Six Mechanistic Findings Governing `M2a` Architecture

1. **Low-degree LFSR lag aliasing & marginal BM solvability:** Under `deg 5` LFSRs ($x^5 + x^2 + 1$), $A(t - 8) = A(t - 5) \oplus A(t - 3)$ aliases deeper lags into short 2-tap combinations and lets univariate `ReedsSloane` deduce marginal recurrences in $\le 40$ ticks. **Action:** Include both small-state `T2` tasks ($\le 465$ states, for exact $\varepsilon$-machine ground truth) and high-degree nonlinear `T2` tasks (`deg 11 × deg 9`, `EQZ`/`MUL`) where marginal linear synthesis fails.
2. **Shared `TickBudget` starvation by unpredicted exogenous driver channels:** Unpredicted LFSR driver channels never hit `zero_error` and consumed `49,889..49,951 / 50,000` replay ticks (`undec = 4,270..12,860`), starving skewed target channels (`CrossMul(k=1)` rose from `0/20` at `50k` to `10/10` at `500k`). **Action:** Pace candidate-replay deductions fairly across active unsolved ports (`remaining_budget` per-check cap) and report per-channel [`Scorecard`](../src/metrics/scorecard.rs) metrics so exogenous driver entropy is separated from coupled target performance.
3. **Epoch-1 `is_1_op` tie-break overriding exact 2-input couplings:** In [`propose_sprout_with_telemetry`](../src/structural/growth.rs), `if is_1_op && !prev_1_op { true }` unconditionally prefers a `20%`-error `Identity(in:0)` over an already-accepted `0%`-error `AddEqz(in:0, lag_in:1:1)` on `CascadeAddEqz(k=2)`. **Action:** Prefer `is_1_op` in Epoch 1 only when `j_cand <= prev_j + MDL_TIE_BREAK_EPS` (preserving all M1 `K=1` golden traces where `1-op` already wins or ties on MDL objective `j_cand`).
4. **Static wide lag search vs. §2.7 progressive deepening:** Naively setting static `max_reactive_lag = 8` inflates $\binom{F}{2}$ feature pairs (`cand_trunc = 920..3,360`) and exhausts replay budget before evaluating deeper lags (`DelayedAffine(d=8)` dropped from `20/20` to `0/20`). **Action:** Implement §2.7 **progressive iterative deepening** ($\tau = 3 \to 6 \to 12$, triggered *only* when the $\tau = 3$ pass yields no accepted candidate, keeping M1 `K=1` traces bit-identical).
5. **`TicksPerHop` physical latency floor & asymmetric hop skew:** Under `WireDelayMode::TicksPerHop` (`hops = 1` per edge), `BoundaryIn -> BinaryOp -> BoundaryOut` has minimum causal latency $1 + 2 = 3$ ticks (`d >= 3`; `+1` hop per `Split`/`Eqz`), and mixed-depth motifs (`AddPair(in:0, lag_in:1:1)`) suffer 1-hop relative skew unless path hops are balanced. **Action:** Compensate path-hop offsets during `TicksPerHop` candidate materialization/signal alignment and restrict `TicksPerHop` gates to physically causal lags $d \ge d_{\min}(\text{motif})$.
6. **Greedy 1-step exact-match horizon on non-decomposable $\mathbb{Z}_4$ bijections:** On `3x + 2 mod 4`, `Neg(x) = 3x` has `0%` point-match prior to adding `+2`, while `Identity(x)` leaves residual `2x + 2` outside 1-step `CandidateMotif` closure. **Action:** Either include affine-negated `NegAddConst(u, c)` in Level-0 `CandidateMotif` or document exact 1-step vs. 2-step monotone residual reachability in the `T2` generator catalogue (see Sign-Off `S3`).

---

## 3. Pre-Registered Hypotheses & Falsifiable Success Criteria

| Gate ID | Definition (metric, generator, width `k`, noise, equal-compute baseline) | Proposed Threshold (`P_target`, margin) | Feasibility Argument (Rule 9: theoretical bound or measured baseline) |
| :--- | :--- | :--- | :--- |
| `G2A.1` | **Clean Cross-Channel Coupling & Marginal Non-Shortcut ($k \in \{1, 2\}$, `Combinational`):** Across `T2_DelayedAffine` ($K=2$, $d \in \{1, 2, 4\}$), `T2_CrossLinear` (`Add`/`Sub`, $K=3$, $(d_0, d_1) = (1, 2)$), and `T2_CrossNonlinear` (`Mul`, `MulEqz`, `AddEqz`, $K=3$, $(d_0, d_1) = (1, 2)$) with coprime LFSR drivers: (i) target-channel post-warmup clean mismatches $= 0$ (`excess_residual_bits_over_h_mu <= 0.005` b/sym), (ii) target output cone wires to $\ge 1$ cross-channel `BoundaryIn(c != target)`, and (iii) marginal $K=1$ target-only control fails to lock (`clean_err >= 0.10`). | Pooled 99% 1-sided Clopper–Pearson LB $\ge 0.90$ + 1% $\chi^2$ homogeneity ($p \ge 0.01$); `0` `Excluded` cases | Exploratory §2.1 achieved `20/20` (`1.000`) lock on `DelayedAffine(d=1..4)`, `CrossSum(k=1,2)`, `CrossMulEqz(k=2)`, and `10/10` on `CrossMul(k=1)` (`B >= 200k`), while marginal `K=1` locked `0/20` (`err = 0.154..0.771`). |
| `G2A.2` | **Iterative Lag Deepening & `TicksPerHop` Causal Latency Alignment ($k \in \{1, 2\}$):** (a) Under `Combinational`, progressive lag deepening ($\tau: 3 \to 6 \to 12$) solves non-aliasing deep-lag `T2` tasks ($d \in \{5, 6, 8\}$ on `deg 11 × deg 9` drivers) with `0` post-warmup target clean errors while keeping all 8 M1 `K=1` golden traces bit-identical; (b) under `TicksPerHop`, hop-aligned wiring solves causally reachable `T2` tasks ($d \ge d_{\min}(\text{motif})$, e.g. `DelayedAffine(d=3)`, `CrossSum(d=3, 4)`). | Pooled 99% CP LB $\ge 0.90$ + 1% $\chi^2$ homogeneity; M1 `golden_traces` `100%` bit-identical | Deepening triggers strictly when $\tau \le 3$ returns `None` (exact M1 reduction); `TicksPerHop` already locked `20/20` on `DelayedAffine(d=2)` in §2.1 (`64` ticks, `3.0` nodes), and $d \ge 1 + \text{hops}$ is the exact causal graph bound. |
| `G2A.3` | **Multi-Channel Concurrent Lock-Time Scaling (`DR #34`, $K \in \{2, 3, 4\}$, $k \in \{1, 2\}$):** On concurrent solvable multi-channel panels (`Combinational` & `TicksPerHop`), every channel locks with `t_lock(multi) <= 1.50 * max(t_lock(solo), W_diag)`. | Pooled 99% CP LB $\ge 0.90$ + 1% $\chi^2$ homogeneity | Readiness `B7` ([research_record.md §7.2](research_record.md#72-b7-multi-channel-lock-time-ratios)) measured `min = 0.25x`, `p50 = 1.00x`, `max = 1.05x` across $K \in \{2, 3\}$; one rotational `sprout_interval = 32` offset at $W_{\text{diag}} = 64$ bounds delay to $(64 + 32)/64 = 1.50\times$. |
| `G2A.4` | **Noisy Cross-Channel Coupling & Bayes-Floor Halting ($\varepsilon = 0.05$, $k \in \{1, 2\}$):** (a) *Target-only noise*: paired target-channel KT residual regret vs. clean-target Bayes substitution oracle $\le 0.030$ b/sym; (b) *All-channel observation noise*: target-channel KT residual loss within $\le 0.040$ b/sym of the exact $m$-input compound reactive Bayes floor $H_{\text{reactive}}(\varepsilon, m, k)$, with `sprout_blocked_max_nodes == 0`. | Pooled 99% CP LB $\ge 0.85$ + 1% $\chi^2$ homogeneity; `0` `Excluded` cases | Exploratory §2.2 measured `+0.0013..+0.0070` b/sym excess over `SubH` under target-only noise (`<< 0.030`) and exact `0.0498` (`5.0%` theoretical) clean-mismatch rate under all-channel noise. |
| `G2A.5` | **Equal-Compute Cross-Channel Baselines & Common-Loss Protocol (Charter §8, `Q6`):** On the `T2` benchmark panel (`deg 11 × deg 9`, $k \in \{1, 2\}$, common `TickBudget = 200,000` and common target-channel prequential KT residual bits/symbol): `ArkheLearner` beats `RandomSearchBaseline` and `CgpBaseline` by $\ge 0.25$ b/sym mean margin on multi-input `T2` tasks, beats univariate `CtwBaseline` and `ReedsSloaneBaseline` by $\ge 0.25$ b/sym on nonlinear `T2_CrossNonlinear`, and is benchmarked alongside new `CrossChannelCtwBaseline` and BPTT `RnnBaseline` with explicit compute/loss reporting. | Mean target KT residual advantage $\ge 0.25$ b/sym & pooled seed win rate LB $\ge 0.85$ vs. `RandomSearch`/`CGP` (and vs. univariate `CTW`/`BM` on `T2_CrossNonlinear`) | Exploratory §2.2 measured `ArkheLearner = 0.0041` KT b/sym vs. `CGP = 1.2281..1.9212`, `RandomSearch = 0.8676..2.0087`, `PerChannelCTW = 0.8376..1.0474`, and `ReedsSloane = 0.5853` err (`> 1.5` b/sym) on `CrossMulEqz(deg 11 × deg 9)`. |

---

## 4. Verification Protocol & Seed Governance

- **Development fast slice:** `0..20` (`20` master seeds).
- **Development multi-bank sweep:** `20..1020` (`50` consecutive banks $\times$ `20` seeds = `1,000` seeds; sized from §2.1 `~275–460 µs/tick` at $K=3$ for ~6–8 min 16-core release runtime, giving theoretical max 1-sided 99% Clopper–Pearson LB `0.9954` at `1000/1000` and passing `>= 0.90` down to `919/1000`).
- **Confirmation bank (fresh dev):** `20020..=20039` (`20` seeds, disjoint from M1 confirmation bank `20000..=20019`).
- **Holdout seeds (single-shot only, DR #38):** `30000..=30019` (`20` seeds, enforced by CLI `--holdout` range guard; disjoint from untouched M1 holdout `10000..=10019`; `40000..=40019` reserved for `M2b`).
- **Multi-bank statistics:** One-sided 99% Clopper–Pearson lower confidence bound on pooled seed successes (`P_target`) plus 1% $\chi^2$ across-bank homogeneity test ([`src/verify_m1/stats.rs`](../src/verify_m1/stats.rs)).
- **Guardrail invariant:** Any run where a compute guardrail (`sprout_blocked_max_nodes`) binds is marked `CaseOutcome::Excluded` and never counted as `Passed`.

---

## 5. Kill & Revisit Criteria

> [!IMPORTANT]
> Per Charter §7 and Rule 11, a kill or revisit outcome is a valid experimental result. If triggered on development or holdout runs, the agent records the finding, writes a blocker report ([`doc/templates/blocker_report.md`](templates/blocker_report.md)), and escalates rather than altering mechanisms, thresholds, or tasks to avoid it.

- **Kill / revisit condition (Charter §7 `M2a`):** Cross-channel coupling consistently stalls on declared `T2` tasks across development banks (pooled pass rate `< 0.50` on `G2A.1` or `G2A.2` after implementing Phase 2, or two consecutive diagnostic fix attempts produce no new mechanistic understanding), or `max_nodes = 32` / `W_diag = 64` structurally binds on minimal 2–3 channel `T2` circuits. Action: halt, file a blocker report, and revisit residual diagnosis / signal-pool scoring with the user.

---

## 6. Parameter Inventory Changes

Proposed updates to [`doc/parameter_inventory.md`](parameter_inventory.md) (subject to §8 user sign-off):

| Constant | Proposed Value | Config / Code path | Class | Telemetry counter | Justification & M1 Reduction |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `deepening_max_lag` | `12` (steps `3 -> 6 -> 12`, $\le W_{\text{diag}}/2$) | `GrowthConfig::deepening_max_lag` (`src/structural/growth.rs`) | Search bound | `LagDeepeningSteps` (`lag_deepening_steps`), `LagHorizonBound` | Implements deferred DR #32 / §2.7 progressive lag search; runs **only** when $\tau \le 3$ yields no accepted sprout, keeping M1 `K=1` golden traces bit-identical. |
| Per-port replay budget slice | Derived: `( budget.remaining() / active_unsolved_ports ).max(w_replay)` per check | `ArkheLearner::observe` (`src/structural/learner.rs`) | Derived | `SproutBudgetExhausted` (`sprout_budget_exhausted`) | Zero free constants; prevents an unpredicted exogenous channel from starving sibling channels of `TickBudget` (`K=1` divides by `1`, preserving exact M1 behavior). |
| `M2A_HOLDOUT_SEED_START` / `END` | `30_000..=30_019` | `src/verify_m2a/mod.rs` | Pre-registered | CLI `--holdout` guard | Reserved in Charter DR #38 (`Q8`). |
| `DEFAULT_M2A_HORIZON` / `WARMUP` / `BUDGET` | `2_000` / `500` / `200_000` | `src/verify_m2a/mod.rs` | Pre-registered | `Scorecard::candidate_replay_ticks_consumed` | Matches M1b `2000`/`500` horizon while scaling `TickBudget` $4\times$ from `50k` ($K=1$) to `200k` ($K=2..4$) per §2.2 measurements. |
| `RnnBaseline` reference config | `hidden_size = 16`, `bptt_len = 16`, `lr = 0.05` | `BaselineConfig::Rnn` (`src/baselines/rnn.rs`, `src/config.rs`) | Pre-registered (baseline only) | Metered equivalent forward/backward step count in `Scorecard` | Fulfils Charter §8 BPTT baseline deferred from M0/M1 ([research_record.md §6.2](research_record.md#62-critical-open-issues--dropped-nuance-relevant-to-m2m5)); never affects `ArkheLearner`. |

---

## 7. Phased Implementation Plan

| Phase | Scope | Golden-Trace Gate / Rationale | Expected Commit Unit |
| :---: | :--- | :--- | :---: |
| **Phase 1** | **Environment `T2` Catalogue & Per-Channel `Scorecard`:** Add `T2` multi-channel generator configs (`T2_DelayedAffine`, `T2_CrossLinear`, `T2_CrossNonlinear`) in `src/env/tiers.rs`, compound reactive noise floor helper in `src/env/epsilon_machine.rs`, and `per_channel` breakdown in `src/metrics/scorecard.rs`. | Behavior-preserving (`cargo test --locked --test golden_traces` bit-identical). | 1 commit |
| **Phase 2** | **Multi-Channel Coupling & Progressive Deepening (`src/structural/`):** Implement §2.7 progressive lag deepening (`3 -> 6 -> 12` when $\tau \le 3$ finds nothing), `TicksPerHop` edge-hop path alignment, `j_cand`-consistent Epoch-1 `is_1_op` tie-break, optional `NegAddConst` motif (if approved in `S3`), and derived per-port `TickBudget` fairness + telemetry. | M1-reducing (`K=1` `golden_traces` bit-identical; escalate via blocker report if any fixture diverges). | 1 commit |
| **Phase 3** | **Cross-Channel Baselines (`src/baselines/`):** Add `CrossChannelCtwBaseline` (joint interleaved multi-channel context) and pure-Rust BPTT `RnnBaseline` (`src/baselines/rnn.rs`) with declared compute/KT-loss accounting in `Scorecard`. | Behavior-preserving on `substrate` & `structural` (`golden_traces` bit-identical). | 1 commit |
| **Phase 4** | **`verify-m2a` Suite, Inventory Update, & Dev/Confirmation Runs:** Implement `src/verify_m2a/` (`G2A.1`–`G2A.5` via `evaluate_multi_bank_gate`), CLI `verify-m2a` (`30000..=30019` holdout guard), update `doc/parameter_inventory.md`, and record dev (`0..20`, `20..1020`) + confirmation (`20020..=20039`) results before requesting single-shot holdout authorization. | Behavior-preserving (`golden_traces` bit-identical; `excluded_cases == 0`). | 1 commit (+1 post-approval holdout record commit) |

---

## 8. User Sign-Off & Attainment Record

### 8.1 Decisions Requested for Pre-Registration Sign-Off

1. **`S1` (Gate definitions & pooled thresholds):** Approve `G2A.1`–`G2A.5` (`P_target = 0.90` on clean/scaling gates `G2A.1`–`G2A.3`, `P_target = 0.85` on noisy/baseline gates `G2A.4`–`G2A.5`, 99% 1-sided Clopper–Pearson + 1% $\chi^2$ homogeneity)?
2. **`S2` (Deepening bound & `TickBudget`):** Approve `deepening_max_lag = 12` (progressive `3 -> 6 -> 12`), derived per-port replay fairness, and `DEFAULT_M2A_BUDGET = 200_000` (`T = 2000`, `warmup = 500`)?
3. **`S3` ($\mathbb{Z}_{2^k}$ affine motif closure):** Add `CandidateMotif::NegAddConst(SourceFeature, u64)` so general affine maps like $3x + 2 \bmod 4$ are 1-step reachable, or keep the current `CandidateMotif` set unchanged and scope `T2_DelayedAffine` to 1-step monotone affine forms ($x + c$, $2x$, $-x$)?
4. **`S4` (Baselines in `G2A.5`):** Include both `CrossChannelCtwBaseline` and the pure-Rust BPTT `RnnBaseline` (`src/baselines/rnn.rs`) now in `M2a` Phase 3, or include `CrossChannelCtwBaseline` now and defer `RnnBaseline`?
5. **`S5` (Seed banks):** Approve dev fast `0..20`, dev multi-bank `20..1020` (`50` banks $\times$ `20` seeds), fresh confirmation `20020..=20039`, and single-shot holdout `30000..=30019` (with `40000..=40019` reserved for `M2b`)?

### 8.2 Attainment Ledger

- **Pre-registration sign-off:** `Pending user approval`
- **Dev bank summary:** `To be filled post-implementation`
- **Single-shot holdout outcome:** `To be filled once after dev & confirmation banks pass`
- **Source digest (`source_tree_fnv1a128`):** `To be filled`
- **Independent review status:** `Open`
