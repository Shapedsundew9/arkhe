# Milestone 2b Chief Scientist Review: Architectural Direction & Trade-Off Decisions

**Date:** 2026-11-15  
**Role:** Chief Scientist Assessment (Charter §7.1, Rules 2, 4, 5, 7, 9, 11)  
**Context:** Phase 4 `verify-m2b` dev evaluation (`0..20` fast slice; holdout `40000..=40019` untouched)  
**Companion Docs:** [Foundational Research Charter v3](foundational_research_charter_v3.md), [Milestone 2b Plan](milestone_2b_plan.md), [Parameter Inventory](parameter_inventory.md), [Research Record](research_record.md)

---

## 0. Executive Summary

1. **Question (a) — Fundamental Direction vs. Targeted Trade-Offs:** **Stay on the ratified Stage `M2b` Epistemic-Corroboration Architecture (`M2B-D1`..`M2B-D4`, Charter DR #40–#43).** Do **not** pivot to a new comparator economy or defer wake refutation. On first contact with `verify-m2b`, the three hardest architectural bets of `M2b` passed **100%** (`480/480` combined cases, `0` guardrail exclusions):
   - **`G2B.1` (`300/300`):** Exact epistemic MDL conservation ($\lvert \Delta E(T) + \Delta L_{\text{null}}^{\text{epistemic}}(T) \rvert / T \le 1.895 \times 10^{-15}$ b/sym), silent/constant/agreeing-hallucination starvation, and `100%` `M1`/`M2a` coupling non-regression.
   - **`G2B.3` (`140/140`):** Unsupervised offline loopback internal falsification (`H3`) on prefix-aliased shallow-vs-deep rules (`lag-2` vs. `period-6` `[0,1,0,1,0,0]` and cross-channel prefix aliases) plus 3-route majority pruning.
   - **`G2B.5` (`40/40`):** Mid-run channel-fault handoff ($\le 64$ ticks to `0` clean error) and refutation pruning, beating `redundancy_sprouting = false` by **`+0.99825` b/sym** (vs. `0.15` b/sym hurdle).
   - Both `G2B.2` and `G2B.4` failures trace to bounded, mathematically transparent boundary conditions (sliding-window tail statistics on `W_diag = 64`, a reactive-vs-autonomous oracle mismatch on `NoisyAlternating_k1`, and four isolable task/learner bugs), **not** a breakdown of Charter `H3` or `DR #40`–`#43`.
2. **Question (b) — Recommended Options & Refinements:**
   - **Trade-Off 1 (False Refutation Pruning Under 5% Noise):** **Adopt Refined Option A (`Proposal 1A+` — Dual-Timescale Reflex Handoff vs. Horizon-Derived Refutation).** Keep reversible driver handoff (`Step A`) on `W_diag = 64` ticks and widen irreversible refutation pruning (`Step B`) to the existing derived `observation_horizon(&economy_config, &growth_config) = t_juv + 2 * W_diag = 256` ticks (**zero** new constants). Pair this with fixing two `G2B.4` multi-view noise artifacts uncovered in our audit (Section 2.3). **Reject Option B**, which weakens a pre-registered anti-thrashing gate and leaves `M4a` sleep vulnerable to premature wake comparator destruction.
   - **Trade-Off 2 (`G2B.4:NoisyAlternating_k1` Oracle Mismatch):** **Adopt Option A (Generator-Class Exact Bayes Oracle).** Score periodic `T1_Alternating` against its true $\varepsilon$-machine / clean-phase Bayes optimum ($h_\mu = H_2(0.05) = 0.2864$ b/sym, matching [`src/verify_m1/m1b.rs`](../src/verify_m1/m1b.rs) `G1B.2b`) and aperiodic exogenous `T2_MultiViewCoupled` against `CausalReactiveOracle`. **Reject Option B**, whose one-sided reactive bound ($\le 0.493$ b/sym) would pass a `+0.167` b/sym learner regression.
3. **Chief-Scientist Audit of Section 3 Queued Fixes (Three Critical Catches):**
   - **Endorse Fix 3.1 & Fix 3.2**, noting that Fix 3.2 (`e_curr == 0 => e_alt_diag == 0`) is directly grounded in Charter §1.2 Popperian fallibilism while preserving prefix-aliased `e_alt_diag == 0` routes in `G2B.3`.
   - **Catch #1 (Deadlock Trap in Fix 3.3):** Do **not** hold Stage 3 while `any sibling !is_port_driven(q)`. On tasks with unpredictable exogenous LFSR channels, exogenous ports legitimately halt at `0` nodes (`!is_port_driven == true` for the entire horizon), which would permanently deadlock Stage 3 on `target_port = 2`. Use a two-pass per-check schedule (Stage 1+2 across all ports before Stage 3) and only defer Stage 3 while a sibling has juvenile nodes or has not yet completed its initial mature Stage-2 pass.
   - **Catch #2 (Root Cause of `MV_Eqz_k2_Comb` Solved):** `lfsr_deg4` over $\mathbb{Z}_4$ degenerates into a period-30 orbit (`Z == 2` only `6/30 = 20%` of ticks $\implies 300/1500$ clean errors) that ports `0` & `1` solve at lag 4 and decorate with comparators (`20+` nodes), inflating 5-node `EqzAddConst` `delta_l_sprout` past `95` bits while [`src/structural/growth.rs`](../src/structural/growth.rs) (`d_cone <= max_reactive_lag`) credits only **1 node** of `initial_energy` (`4.0 + 2 * 45.03 = 94.06` bits, emitting `candidate_undecidable = 223`).
   - **Catch #3 (Exogenous LFSR Aliasing in `G2B.2`/`G2B.4`):** Upgrading `MultiViewDriver::Lfsr` from 2-tap `lfsr_deg4`/`lfsr_deg5` to `lfsr_deg11` (matching `milestone_2b_plan.md` §3's *"high-degree LFSR latent driver $Z$"*) and computing `G2B.4` theoretical disagreement from the comparator's actual noisy input-leaf counts `(m_pri, m_alt)` eliminates `dis_rate = None` and the `mask = 0x5` (`0.1355` vs. `0.0950`) false failure.

---

## 1. Question (a): Should We Pivot Direction Toward the Charter Goal?

Before tuning or amending any `M2b` rule, we must ask whether the `M2b` comparator-economics architecture (`DR #40`–`#43`) is carrying structural complexity that a cleaner design would avoid across `M3` (free-rider resistance), `M4a` (online wake–sleep `S-A` vs. `S-B`), `M4b` (`egg` equality saturation & axiom promotion), `M4c` (`T4` transfer), and `M5` (width scaling).

We evaluated the three plausible fundamental pivots against rework cost and long-run Charter physics:

### 1.1 Fundamental Alternative I: Abolish Explicit Wake Refutation (`Step B`), Relying Only on Reversible Handoff (`Step A`) + Metabolic Starvation + Sleep Pruning

- **Appeal:** Eliminates `Step B` windowing altogether; pruning happens only via `NodeEconomy::energy <= 0` in wake or internal contradiction in sleep (`M4a`).
- **Why it fails mathematically:** Under [`PointForecastScorer`](../src/metrics/point_forecast.rs) ($q = \max(\hat{q}, p_{\text{null}})$) and `ReserveCap::HorizonDerived` ([`src/structural/economy.rs`](../src/structural/economy.rs)), when a previously clean `k = 1` channel corrupts to `50%` random substitution at $t_{\text{fault}} = 1000$ (`G2B.5`), it still matches ground truth on `50%` of ticks by chance (unlike deterministic regime shifts such as `Alt -> Constant(0)` where match rate drops to `0%`). As $\hat{q}$ decays toward $p_{\text{null}} = 0.5$:
  1. Per-tick expected loss shrinks as $O((\hat{q} - 0.5)^2)$ and hits exact `0.0` once $\hat{q} \le p_{\text{null}}$;
  2. Metabolic reserve drain stretches well beyond $2 W_{\text{obs}} > 500$ ticks (or stalls above `0.0` if $\hat{q}$ reaches $p_{\text{null}}$ before `energy` crosses `0.0`);
  3. Throughout that zombie interval, `active_derivations(p) == 2` **blocks Stage 3 from ever sprouting a fresh healthy backup** on a third channel, and carries dead circuitry into `M3` size audits and `M4a` sleep cycles.
- **Conclusion:** Explicit wake refutation (`Step B`) is structurally required whenever a peer derivation proves strictly positive predictive edge while the disagreeing route falls to non-positive calibrated flux.

### 1.2 Fundamental Alternative II: Replace Sliding Windows with an Unbounded Sequential Martingale / CUSUM Detector

- **Appeal:** Sequential probability-ratio / Page–Hinkley change-point tests are textbook optimal for detecting drift from $+0.55$ b/tick to $\le 0$ b/tick.
- **Why it is the wrong move in `M2b`:**
  1. A one-sided Page–Hinkley / CUSUM test requires choosing **two new parameters** (a reference drift slack $\delta > 0$ and a log-odds barrier $h > 0$), violating [`.github/copilot-instructions.md`](../.github/copilot-instructions.md) Rule 2 (zero ungrounded magic numbers) when the architecture *already* possesses a derived, parameter-free observation horizon $W_{\text{obs}} = t_{\text{juv}} + 2 W_{\text{diag}} = 256$ (`DR #31`).
  2. [Parameter Inventory §2.1](parameter_inventory.md#21-parameter-scaling-derivations--deferred-rules) explicitly scopes holistic prequential test unification (replacing `mi_threshold_bits`, Miller–Madow, `min_match_gain`, and `W_diag` together) to **`M5`** when width scales to $k \in \{4, 8\}$. Doing a partial rewrite inside `M2b` incurs multi-phase rework (`Phase 2`–`Phase 4` + golden re-audit) for zero capability gain over $W_{\text{obs}} = 256$.

### 1.3 Fundamental Alternative III: Restrict Redundancy Sprouting to Clean Streams or Scheduled Pre-Sleep Windows

- **Appeal:** Sidesteps stationary noise variance completely during wake.
- **Why it betrays the Charter:** Realizing `H3`, `H5`, and `H6` requires redundant derivations to form and survive under declared channel noise ($\varepsilon = 0.05$) and mid-run channel faults (`G2B.4`, `G2B.5`). Restricting redundancy to noise-free regimes would falsify fault tolerance by construction and starve `M4a` sleep of comparators whenever observations are noisy.

### 1.4 Strategic Verdict on Direction

**No fundamental pivot is warranted.** The `M2b` architecture (`M2B-D1`..`M2B-D4`) cleanly separates **fact-anchored wake corroboration** from **frozen-null sleep agreement**, satisfies exact $\Delta E(T) \equiv -\Delta L_{\text{null}}^{\text{epistemic}}(T)$ conservation, and plugs directly into `M3` (where `L(C)` + `split_code_tax` + `D1b.1` comparator-aware sensitivity prune detours) and `M4a` (where `run_offline_loopback` becomes the scheduled sleep phase). Completing `M2b` via principled, zero-free-parameter resolution of Trade-Offs 1 & 2 is unambiguously the highest-leverage path toward the Charter goal.

---

## 2. Trade-Off 1: False Refutation Pruning Under 5% Stationary Noise (`G2B.4`)

### 2.1 Exact Scan-Statistic Root Cause

In [`ArkheLearner::process_wake_comparator_events`](../src/structural/learner.rs), a comparator executes two distinct operations with fundamentally different reversibility profiles:

- **Step A (`driver_handoff` — reversible wire swap):** Re-routes `BoundaryOut(p)` to `alt` in $O(1)$ without deleting any nodes or state registers when `e_alt < e_primary` and $\sum (r_{\text{alt}} - r_{\text{primary}}) > \Delta L_{\text{comp}}$. Fast responsiveness ($\le W_{\text{diag}} = 64$ ticks) directly governs clean output recovery after a channel fault (`G2B.5`).
- **Step B (`disagreement_prune` — irreversible subgraph excision):** Permanently deletes derivation $d$ and its `SUB + EQZ` comparator when `disagreements >= min_match_gain` and $\sum r_d \le 0 < \sum r_{\text{peer}}$.

Why did [milestone_2b_plan.md §2.3 & §3](milestone_2b_plan.md#23-five-structural-invariants-governing-m2b-implementation) predict zero false prunes at $\varepsilon = 0.05$, whereas `verify-m2b` observed `4/20` (`NoisyAlternating_k1`) and `10/20` (`MV_AllChNoise_k1`)? Because §2.3 checked the **single-window expectation** ($\mathbb{E}[\sum_{W=64} r_d] \approx +35.0$ bits at `9.5%` miss; $+27.4$ bits at `13.55%` compound miss) rather than the **minimum over ~1,600 correlated sliding windows** $\min_{t \in [t_{\text{mat}}, 2000)} \sum_{s=t-W+1}^t r_d(s)$:

| Noisy Route ($k = 1$, $p_{\text{null}} = 0.5$) | Effective Miss Rate $\varepsilon_{\text{eff}}$ | Critical Misses for $\sum_W r_d \le 0$ | Single-Window Binomial Tail ($W = 64$) | Single-Window Binomial Tail ($W = 256$) | 2,000-Seed Prequential Sliding False-Prune Rate ($W = 64 \to W = 256$) | Min Sliding Window Flux ($W = 64 \to W = 256$) |
| :--- | ---: | :---: | ---: | ---: | :---: | :---: |
| 1-input (`NoisyAlternating_k1`, `MV_AllChNoise_k1` 1-lag) | `0.0950` | `26.6%` (`17/64` vs. `68/256`) | $7.22 \times 10^{-5}$ | **$3.88 \times 10^{-15}$** | `1.0%–20%* -> 0.00% (0/2000)` | `-2.73 b -> +63.48 b` |
| 2-input (`MV_AllChNoise_k1` 2-leaf route) | `0.1355`–`0.1430` | `29.7%` (`19/64` vs. `76/256`) | $6.07 \times 10^{-4}$ | **$1.57 \times 10^{-11}$** | `6.1%–50%* -> 0.00% (0/2000)` | `-2.74 b -> +32.79 b` |

*\*Early upward fluctuations in prequential $\hat{q}$ (e.g., $\hat{q} \approx 0.93$ after a quiet juvenile stretch) steepen miss penalties $\log_2((1-\hat{q})/0.5)$, lowering the `W = 64` critical miss count by 1–2 ticks and raising per-run sliding false-prune rates into the observed `20%–50%` band.*

Meanwhile, as the engineering summary rightly noted, replacing the `0.0` flux barrier with $-\Delta L_{\text{comp}} \approx -34\text{ to }-40$ bits is **impossible** at $k = 1$: by Krichevsky–Trofimov universality, a `50%`-corrupted binary route whose $\hat{q}$ approaches $p_{\text{null}} = 0.5$ incurs only $-O(\frac{1}{2}\log_2 W) \approx -2.5\text{ to }-5.0$ bits (or `0.0` bits once $\hat{q} \le p_{\text{null}}$), so a $-34$-bit threshold would break `k = 1` fault pruning.

### 2.2 Chief-Scientist Recommendation: Adopt Refined Option A (`Proposal 1A+`)

**Select Option A over Option B**, implemented as **`Proposal 1A+` (Dual-Timescale `ComparatorWakeWindow`)**:

1. **Timescale Separation (Zero New Constants, Rule 2):**
   - **Step A (`driver_handoff`):** Continues evaluating over the trailing `W_diag = growth_config.diagnostic_window` (`64`) ticks as soon as `len >= W_diag`. In `G2B.5` (`50%` fault at $k=1$, `75%` fault at $k=2$), handoff triggers in **$13\text{–}25 \le 64$ ticks**, preserving `0` post-recovery clean errors (`G2B5_RECOVERY_WINDOW_TICKS = 64`).
   - **Step B (`disagreement_prune`):** Evaluates over the trailing `W_obs = observation_horizon(&economy_config, &growth_config)` (`t_juv + 2 * W_diag = 256`) ticks once `len >= W_obs`, requiring `disagreements(W_obs) >= min_match_gain` and `flux_d(W_obs) <= 0.0 < flux_peer(W_obs)`.
   - **Verified `G2B.5` Fault-Prune Latency under `W_obs = 256`:** Across `2,000` simulated `G2B.5` trajectories ($t_{\text{fault}} = 1000$, $T = 2000$), because pre-fault hits earn $\le +1.0$ b/tick ($k=1$) / $+2.0$ b/tick ($k=2$) while post-fault misses against high pre-fault $\hat{q} \approx 0.999$ begin at $-9.9$ b/tick, `flux_bad(256)` crosses $\le 0.0$ in **$\le 124$ ticks** (`2,000/2,000` pruned by $t \le 1124 \ll 2000$). Even for a newly sprouted pure-chance route ($\hat{q} \le p_{\text{null}}$), `flux_d(256) == 0.0 <= 0.0` fires cleanly at tick `256` once `disagreements >= min_match_gain`.
   - **Backward Invariance:** On clean circuits (`tests/golden_traces.rs`, `verify-m1` clean gates, `verify-m2a` `G2A.1`–`G2A.3`), `disagreements == 0`, so trajectories are **100% bit-identical**. On `tests/regime_shift.rs` (`Alt -> Constant(0)`), both routes go non-positive simultaneously so `flux_peer > 0.0` remains false, preserving starvation semantics identically.
2. **Why Option B Must Be Rejected:** Allowing false disagreement prunes in `G2B.4` (or dropping `MV_AllChNoise`) masks a real structural defect: under stationary `5%` noise, wake would periodically amputate valid redundant comparators, causing re-sprout churn (`TickBudget` drain) and leaving `M4a` sleep without comparators right when wake transitions to sleep.

### 2.3 Two Coupled `G2B.4` Multi-View Artifacts to Fix Alongside `1A+`

Inspecting `verify-m2b --seed-offset 0 --seeds 1` exposed two additional `G2B.4` bugs that would continue failing `MV_AllChNoise_k1` and `MV_AllChNoise_k2` even with `dis_prunes == 0`:

1. **2-Input LFSR-Recurrence Disagreement Rate (`0.1355` vs. hardcoded `0.0950`):** When `MV_AllChNoise_k1` uses low-degree `lfsr_deg5` ($Z(t) = Z(t-2) \oplus Z(t-5)$), Stage 3 can wire a Tier-1 `alt` route combining **two** noisy channels (`ch0` and `ch2`, `mask = 0x5`, compound flip rate $2\varepsilon(1-\varepsilon) = 0.095$) against 1-input `primary` (`ch1`, flip rate $0.05$). Their exact mathematical disagreement rate is $0.05(0.905) + 0.95(0.095) = \mathbf{0.1355}$, which exceeds the hardcoded 1-vs-1 `theory = 0.0950` by **`0.0405 > G2B4_DISAGREEMENT_TOL (0.040)`**!
2. **Exogenous `lfsr_deg4`/`lfsr_deg5` Self-Prediction & `dis_rate = None`:** On `seed = 0`, both `MV_AllChNoise_k1` and `MV_AllChNoise_k2` achieved `paired_diff = 0.0000` and `dis_prunes = 0`, yet failed with **`dis_rate = None`** (`active_comps = 1` and `2`) because 2-tap `lfsr_deg4`/`lfsr_deg5` allowed exogenous ports `0` & `1` to self-predict and consume Stage-3 budget/maturity before `target_port = 2` matured a comparator.
3. **Clean Resolution:** Use **`lfsr_deg11`** (`taps` at lags `9` and `11`, matching `milestone_2b_plan.md` §3's specification of a *"high-degree LFSR latent driver $Z$"*) for `T2_MultiViewCoupled` tasks in `G2B.2`/`G2B.4`/`G2B.5`, **and** compute `G2B.4`'s `theory_dis_rate` from the formed comparator's actual noisy input-leaf counts `(m_pri, m_alt)` via `compound_flip_rate(eps, m, k)`.

---

## 3. Trade-Off 2: `G2B.4:NoisyAlternating_k1` Target KT (`0.286` vs. Reactive Oracle `0.453`)

### 3.1 Epistemological & Cross-Milestone Analysis

On `T1_Alternating { a: 0, b: 1 }` with $\varepsilon = 0.05$ substitution noise:

- A purely reactive lag-1/lag-2 predictor (`x(t-1) ⊕ 1` or `x(t-2)`) suffers **double noise** (input flip at $t-1$ plus output flip at $t$), giving error rate $2\varepsilon(1-\varepsilon) = 0.095$ and asymptotic code length $H_2(0.095) = \mathbf{0.4529}$ b/sym.
- An entrained autonomous period-2 ring (`Osc2`) ignores corrupted single-tick inputs and predicts the true underlying deterministic phase, suffering **only target observation noise** $\varepsilon = 0.05$ with asymptotic $\varepsilon$-machine entropy rate $h_\mu = H_2(0.05) = \mathbf{0.2864}$ b/sym.
- In Milestone 1b ([`src/verify_m1/m1b.rs`](../src/verify_m1/m1b.rs), `G1B.2b`), `ArkheLearner` was **explicitly gated** to achieve paired regret $\le 0.025$ b/sym against the $\varepsilon$-machine (`0.2864` b/sym) **and** strictly beat `FixedReactiveEchoBaseline` (`0.4529` b/sym).
- Comparing `ArkheLearner` (`kt = 0.249..0.326`) two-sidedly against `M2bCausalReactiveOracle` (`0.401..0.516`) in `G2B.4` mistakenly penalized the learner for achieving the Charter §5.2 Bayes optimum!

### 3.2 Chief-Scientist Recommendation: Adopt Option A (Generator-Class Exact Bayes Oracle)

**Select Option A over Option B:**

1. **Exact Generator-Class Bayes Reference (Charter §5.2):**
   - For periodic `T1` (`NoisyAlternating_k1`), score paired target KT difference against the **clean-phase Bayes oracle** ($\hat{y}(t) = s_{\text{clean}}(t)$ on the identical noise realization, matching `verify-m0` `G3-rev1`, `verify-m1` `G1B.2b`, and `verify-m2a` `G2A.4a`), with theoretical asymptotic entropy $H_{\text{opt}} = H_2(\varepsilon) = 0.2864$ b/sym.
   - For exogenous high-degree LFSR `T2_MultiViewCoupled`, where latent $Z(t)$ is aperiodic over the search horizon, retain `CausalReactiveOracle` and $H_{\text{reactive}}(\varepsilon, m, k)$ (`0.2864`/$`0.3369`$ for target-only noise; `compound_reactive_entropy_bits` for all-channel noise).
   - Change the pooled summary metric name/semantics in `G2B.4` from `|E[KT] - H_reactive|` to `|E[KT] - H_bayes_class| <= 0.040`, where each task contributes its own exact class-optimal entropy rate ($H_2(\varepsilon)$ for `NoisyAlternating_k1`, $H_{\text{reactive}}$ for `T2_MultiViewCoupled`).
2. **Why Option B Must Be Rejected:** Changing `NoisyAlternating_k1` to a one-sided reactive check (`learner_kt <= reactive_oracle_kt + 0.040 ≈ 0.493` b/sym) would pass a broken learner that **failed** to promote or retain its autonomous ring (a $+0.167$ b/sym performance regression relative to `M1b`), and leaves `h_reactive_sum` biased in the pooled `|E[KT] - H|` check.

---

## 4. Chief-Scientist Audit of Section 3 Queued Engineering Fixes

We audited all three queued fixes in the codebase and identified three subtleties that **must** be incorporated before running the 50-bank sweep (`20..1020`):

### 4.1 Queued Fix 3.1 (`verify_m2b/mod.rs` Multi-View Task Construction in `G2B.2`)

- **`MV_Affine_k2_TphSplit` (`view_1.delay = 1`):** **Verified & endorsed.** In `TicksPerHop`, `Split`-driven `ch0` reaches `sub_node.in(0)` at causal lag `3` (`prev_frame` + `BoundaryIn->Split` + `Split->sub_node`), yielding $Z(t-4)$ when `view_0.delay = 1`. Because `NegAddConst` on `ch1` also traverses `1` primitive (`BoundaryIn->Sub_neg->sub_node`, causal lag `3` with `0` `Delay`s, or lag `5` with `1` `Delay`), `view_1.delay = 0` hits odd-even hop parity mismatch ($Z(t-3)$ or $Z(t-5) \ne Z(t-4)$). Setting `view_1.delay = 1` aligns exact hop parity to $Z(t-4)$.
- **`MV_AddEqz_k2_Comb` / `MV_MulEqz_k2_Comb` (`view_1 = { delay: 0, mul_a: 1, add_b: 0 }`):** **Verified & endorsed.** Single-step `CandidateMotif::AddEqz` and `MulEqz` combine delayed raw sources without pre-inverting affine $3z + 1 \pmod 4$ on both arms simultaneously; distinct unscrambled delays (`view_0.delay = 1` vs. `view_1.delay = 0`) cleanly test cross-channel `C_alt ∩ C_primary = ∅` nonlinear comparator formation.
- **Required Addition — Upgrade `MultiViewDriver` from `lfsr_deg4`/`lfsr_deg5` to `lfsr_deg11`:** Over $\mathbb{Z}_4$, `lfsr_deg4` (`taps = [1, 0, 0, 1]`, `init = [1, 0, 1, 1]`) collapses into a **period-30** cycle with skewed symbol counts `{0: 8, 1: 6, 2: 6, 3: 10}` and 2-tap lag-4 self-predictability on exogenous ports `0` & `1`. Using `lfsr_deg11` (`taps` at `9, 11`, matching `milestone_2b_plan.md` §3) prevents exogenous view ports from solving themselves and stealing Stage-2/Stage-3 compute.

### 4.2 Queued Fix 3.2 (Clean-Port Stage-3 Exactness in [`src/structural/growth.rs`](../src/structural/growth.rs))

- **Rule:** When `baseline.e_curr_by_port[out_idx] == 0` over the replay evaluation span, reject Stage-3 candidates with `e_alt_diag > 0`.
- **Epistemological Audit (Charter §1.2 & §1.3):** **Endorsed.** Under Popperian fallibilism (Charter §1.2), when `e_curr_by_port[out_idx] == 0` (noise-free consistency with observed facts), a candidate with `e_alt_diag > 0` **already contradicts observed ground-truth facts** in wake and should never be sprouted as a corroborating theory. Crucially, prefix-aliased deceptive rules in Charter §1.3 / `G2B.3` (`lag-2` vs. `period-6` on an unrefuted prefix) have **`e_alt_diag == 0`** on the observed prefix, so they remain 100% admissible; noisy streams (`G2B.4`) have `e_curr_by_port[out_idx] > 0`, so noisy redundant routes also remain admissible.

### 4.3 Queued Fix 3.3 (Cross-Port Budget Deadlock & `MV_Eqz_k2_Comb` Root Cause)

1. **CRITICAL WARNING — Do NOT Gate Stage 3 on `∀ q: is_port_driven(q)`:**
   - The summary proposed: *"Hold optional Stage-3 searches while any sibling port still lacks a primary derivation (`!is_port_driven`)."*
   - **Why this will deadlock:** On `T2_MultiViewCoupled` with an unpredicted high-degree LFSR latent driver $Z$ (and on `M2a`-style exogenous driver channels), exogenous view ports `0` and `1` **legitimately remain unconnected (`!is_port_driven(0) && !is_port_driven(1)`) for the entire 2,000-tick run!** Blocking Stage 3 while any sibling is `!is_port_driven` would **permanently prevent `target_port = 2` from ever running Stage 3 (`0/20` comparators)!**
   - **Correct Scheduling & Budget Law:**
     - **Two-pass check within [`ArkheLearner::observe`](../src/structural/learner.rs):** Run Stage 1 + Stage 2 across **all** eligible ports first; run Stage 3 in a second pass only if no port sprouted in Stage 2 on that tick and `!self.sibling_has_juvenile_nodes(p)` and every sibling port has completed at least **one** mature Stage-2 diagnostic evaluation (`stage2_checks_completed >= 1`).
     - **`port_budget_allowance` idle-donor definition:** A port `q` is an idle donor iff `is_port_zero_error(q) && !port_redundancy_arity_eligible(q)` (or it has already exhausted Stage-3 candidates without structural change). Any port needing Stage 2 **or** Stage 3 (`!is_port_zero_error(p) || port_redundancy_arity_eligible(p)`) may draw from `idle_pool` once sibling ports are not actively consuming Stage-2 deepening (keeping the `base_share` cap during active sibling Stage-2 deepening only until each unsolved sibling has completed a full deepening cycle without progress).
2. **Exact Diagnosis of `MV_Eqz_k2_Comb` (`105,108` Stage-2 Ticks, `clean_mis = 300`, `candidate_undecidable = 223`):**
   - On `MV_Eqz_k2_Comb:seed=0`, `lfsr_deg4` (`period = 30`) emits `Z == 2` on only `6/30 = 20.0%` of ticks (`300 / 1500` clean errors; `12 / 64` ones per `W_diag`), giving exact `EqzAddConst(LagIn(0, 1), 2)` a 64-tick `pos_flux_cand = 45.03` bits.
   - Meanwhile, ports `0` and `1` solved `lfsr_deg4` and sprouted comparators (`settled = 0/3000`, `~22` nodes), raising 5-new-node `EqzAddConst`'s `delta_l_sprout` above **`95` bits**.
   - In [`src/structural/growth.rs`](../src/structural/growth.rs) (`evaluate_sprout_motif`), when `d_cone <= max_reactive_lag` (`d_cone = 1 <= 3`), `projected_survival` still uses M1's **1-node** endowment `initial_energy + 2.0 * pos_flux_cand = 4.0 + 90.06 = 94.06 <= delta_l_sprout` (emitting `candidate_undecidable = 223`), whereas the `d_cone > max_reactive_lag` branch right below it uses the true multi-node endowment `new_nodes * initial_energy + ...` (`5 * 4.0 + 90.06 = 110.06 > 95` bits)!
   - **Check:** Switching `T2_MultiViewCoupled` to `lfsr_deg11` already restores uniform `25%` symbol density (`pos_flux_cand ≈ 52.3` bits $\implies 4.0 + 104.6 = 108.6` bits) and `0` exogenous bloat on ports `0` & `1`(`delta_l_sprout ≈ 73` bits). If we also unify `endowment = new_nodes * initial_energy` in `evaluate_sprout_motif`, verify`cargo test --locked --test golden_traces` and `verify-m1` invariance first (Rule 4).

---

## 5. Ratified Governance Decisions (`2026-11-15` User Sign-Off)

Per [Charter §7.1](foundational_research_charter_v3.md#71-milestone-lifecycle-and-decision-rights) and [`.github/copilot-instructions.md`](../.github/copilot-instructions.md) Rules 2, 4, 5, and 7, all three pre-registration amendments have been **ratified by the user (`2026-11-15`)** and folded into [`docs/milestone_2b_plan.md`](milestone_2b_plan.md) (§2.3, §3, §6, §8.1) for execution by the `m2b_verify` worktree swarm (`holdout 40000..=40019` remains locked until dev `0..20`, `20..1020`, and confirmation `20040..=20059` pass cleanly):

1. **`M2B-A1` (Trade-Off 1 — `Proposal 1A+`) — RATIFIED (`2026-11-15`):** Decouple `ComparatorWakeWindow` into `W_handoff = W_diag` (`64`, reversible Step A) and `W_prune = observation_horizon(&economy_config, &growth_config)` (`256`, irreversible Step B), with zero new constants (`golden_traces`, `verify-m1`, `verify-m2a`, and `G2B.5` $\le 64$-tick recovery preserved).
2. **`M2B-A2` (Trade-Off 2 — Option A) — RATIFIED (`2026-11-15`):** Score each `G2B.4` generator against its exact structural-class Bayes optimum (clean-phase / $\varepsilon$-machine oracle $H_2(\varepsilon) = 0.2864$ b/sym for periodic `T1_Alternating`; `CausalReactiveOracle` and $H_{\text{reactive}}(\varepsilon, m, k)$ for exogenous LFSR `T2_MultiViewCoupled`, pooled `|E[KT] - H_bayes_class| <= 0.040`), with `theory_dis_rate` computed from the formed comparator's actual noisy input-leaf counts `(m_pri, m_alt)`.
3. **`M2B-A3` (Multi-View Driver & Cross-Port Stage-3 Guard) — RATIFIED (`2026-11-15`):** Upgrade `T2_MultiViewCoupled` tasks in `src/verify_m2b/mod.rs` from 2-tap `lfsr_deg4`/`lfsr_deg5` to high-degree `lfsr_deg11` (matching `milestone_2b_plan.md` §3), apply Fixes 3.1 (`TicksPerHop` `view_1.delay = 1` + unscrambled distinct-lag `AddEqz`/`MulEqz` `view_1`) and 3.2 (clean-port Stage-3 exactness `e_curr == 0 => e_alt_diag == 0`), and apply the corrected non-deadlocking Fix 3.3 (two-pass Stage 1+2 $\to$ Stage 3 check + idle-donor `port_budget_allowance` without blocking on permanently unpredicted `!is_port_driven` exogenous ports; check `golden_traces`/`verify-m1` if unifying multi-node `projected_survival` endowment).

---

## 6. Post-`83ca660` Blocker Audit: Are We Over-Protecting Early-Phase Constants?

Following implementation of `M2B-A1`–`M2B-A3` (`83ca660`), `verify-m2b --seed-offset 0 --seeds 20` now passes **`G2B.1` (`300/300`)**, **`G2B.3` (`140/140`)**, and **`G2B.4` (`180/180`, `pooled_|E[KT]-H_bayes_class| = 0.00105`, `0` false prunes)** cleanly, while surfacing the three boundary blockers documented in retired `doc/milestone_2b_blocker_report.md` (consolidated in §6.1–§6.2 below; `G2B.5` at `34/40`; `G2B.2` at `120/200`).

Stepping back to ask **whether over-protecting early Milestone-1 scaffolding constants is forcing us into Ptolemaic epicycles**, auditing all six M1 scaffolding constants in [`docs/parameter_inventory.md`](parameter_inventory.md) gives a sharp, asymmetric answer: **YES on `juvenile_ticks = 128` (and `1 * initial_energy = 4.0` at `growth.rs:2681`), NO on `W_obs = 256` and `W_diag = 64`.**

### 6.1 Where We *Are* Over-Protecting M1 Constants: `juvenile_ticks = 128` & `1 * initial_energy` (`Blocker #2`)

In M1 (`K = 1, d <= 3`, `1`–`3`-node motifs on `~1.0–2.0` b/tick streams), `juvenile_ticks = 128` and single-node `initial_energy = 4.0` happened to suffice. Because subsequent stages tried to keep `juvenile_ticks = 128` frozen at $d_{\text{cone}} \le 3$ (to avoid touching M1 `trajectory_digest_fnv1a` hashes), the codebase accumulated **three separate partial workarounds**—and just hit a fourth wall:

1. **M2a `d2-A` (`effective_juvenile_ticks`):** Compensated $\gamma^d$ register decay *only* when $d_{\text{cone}} > 3$, hardcoding `return 128` for $d_{\text{cone}} \le 3$.
2. **M2a [`src/structural/growth.rs`](../src/structural/growth.rs) (`evaluate_sprout_motif`):** Updated `endowment = new_nodes * initial_energy` *only* for $d_{\text{cone}} > 3$, leaving `1 * initial_energy (4.0)` for $d_{\text{cone}} \le 3$. On `MV_Eqz_k2_Comb` (`6` nodes, `e_cand = 0`, `delta_l = 121.63`) and `MV_MulEqz_k2_Comb` (`7` nodes, `e_cand = 0`, `delta_l = 135.46`), that M1 fossil rejects exact `0`-error solutions (`120.11 <= 121.63` and `119.85 <= 135.46`) even though true `new_nodes * 4.0` endowment clears every `6`–`9`-node candidate (`140.11..168.45 > delta_l`) and their aggregate amortization time is only `135`–`163 <= 256` ticks!
3. **M2b `M2B-D1` (`effective_comparator_juvenile_ticks`):** Compensated `(Z_alt + 2)/Z_alt` share dilution *only* for Stage-3 comparators, leaving Stage-2 multi-node nonlinear cones (`5`–`9` nodes at $d_{\text{cone}} \le 3$ on skewed `0.81`–`0.87` b/tick outputs, where per-node share $s_v = w_v / Z_{\text{cone}} \approx 0.14$) fixed at `128` ticks—so upstream `Delay`/`Split` nodes starve at graduation (`E = -9.37` against `debt = 21.22` at `t = 193`) despite `0%` prediction error!

If we merely bolt on a fourth Stage-2-specific multiplier while keeping fixed `128` sacred, the same contortion will resurface in `M3` (exact-synthesis subgraphs), `M4c` (`T4` composite motifs), and `M5` ($k = 4, 8$ `T3a`/`T3b` counters).

**Clean De-Contorted Law (`Proposal M2B-B1` — Unified Per-Node Debt-Amortization Maturation):**

Per [`docs/parameter_inventory.md` §2.1](parameter_inventory.md#21-parameter-scaling-derivations--deferred-rules) (*"Milestone-Scoped Scaffolding constants... evolve into zero-free-parameter Derived scaling laws"*):

- Reclassify `EconomyConfig::juvenile_ticks` (`128 = 2 * W_diag`; `0` in conservation tests) explicitly as the **minimum statistical maturation floor** $t_{\text{juv,min}}$.
- Unify `endowment = new_nodes * initial_energy` across **all** `d_cone` in [`src/structural/growth.rs`](../src/structural/growth.rs) (`evaluate_sprout_motif`).
- Compute juvenile maturation grace for any Stage-2 or Stage-3 structural addition $V_{\text{new}}$ from exact per-node marginal description debt $\Delta L_v$ and replay-measured positive per-tick node credit rate $\hat{r}_v^+$:
  $$t_{\text{juv}}(V_{\text{new}}) = \max\!\left(\text{effective\_juvenile\_ticks}(d_{\text{cone}}),\ d_{\text{cone}} + \max_{v \in V_{\text{new}}} \left\lceil \frac{\Delta L_v}{\hat{r}_v^+} \right\rceil\right),$$
  subject to three anti-bloat invariants:
  1. **Zero-share free-rider rejection:** Every new node $v \in V_{\text{new}}$ must achieve $\hat{r}_v^+ > 0$ on replay (otherwise reject immediately, strengthening `M3` free-rider defense).
  2. **Bounded decidability ceiling:** Require $t_{\text{juv}}(V_{\text{new}}) \le 2 W_{\text{obs}}$ (`512` ticks at defaults; emit `CandidateUndecidable` and reject if exceeded) plus buffered graduation survival (`energy > debt` across `t_juv`), so weak noise correlations cannot claim unbounded juvenile grace.
  3. **Depth/size-invariant `amort_scale` on lossy (`e_cand > 0`) screening:** Keep `amort_scale = (t_juv_min / W_diag).max(1.0)` (`2.0`) unchanged in `j_mat_cand < j_mat_curr` so lossy/noisy candidates still must justify `L(C)` over the base horizon (`M2a` `Rec 2-a`).
- **M1 / M2a impact:** On `1`–`3`-node M1 sprouts ($\lceil \Delta L_v / \hat{r}_v^+ \rceil \approx 13\text{–}69 \le 128$), $\max(128, \dots) = 128$ naturally with **zero** hardcoded `d_cone <= 3` cliff; if any multi-node M1 golden fixture's energy hash shifts while `verify-m1` and `verify-m2a` pass `100%`, re-bless per Rule 4 with written mathematical audit rather than contorting the law.

### 6.2 Where Constants Are *Not* the Problem: Split-Brain Bankruptcy Race (`Blocker #1`) & Exogenous Juvenile Pruning (`Blocker #3`)

1. **`Blocker #1` (`G2B.5` `MV_Fault_k1`, `6/20` failing at `t = 1118` with `win.len() = 252 < 256`):**
   - `W_obs = 256` is **not** miscallibrated—it just delivered **`180/180` (`100%`)** on `G2B.4` with `0` false prunes.
   - On `seed = 11`, comparator `0` matured at `t = 867`, handed off cleanly at `t = 1009` (`post_rec_clean_mis = 0`), and by `t = 1118` (`252` mature ticks total) accumulated **`-99.86` bits** of mismatch loss on corrupt `ch0`, driving `sub_node` to **`E = -0.929 <= 0.0`** (`EconomyState` bankruptcy) 4 ticks before `win.len()` reached `256`.
   - The bug is a **split-brain race**: [`src/structural/learner.rs`](../src/structural/learner.rs) deleted starved `sub_node` in isolation (`starvation_prune_events += 1`), leaving broken `ch0`'s `Delay` stump behind instead of recognizing that **1-sided metabolic bankruptcy of a disagreeing comparator while its peer derivation is solvent IS a wake disagreement refutation**.
   - **Clean Resolution (`Proposal M2B-B2` — Blocker Report Option `a1` with full-arm excision):** Keep `W_obs = 256` unchanged. When `starved_nodes` strikes a mature comparator (`sub_node`, `eqz_node`, or exclusive arm) while `win.len() >= W_diag`, `disagree(W_diag) >= min_match_gain`, `flux_worse(len) <= 0.0`, and the peer derivation is unstarved with `flux_peer(W_diag) > 0.0`, atomically execute `excise_port_comparator` on the failing arm and increment `disagreement_prune_events += 1` (preserving `G2B.4` `180/180`, `G2B.1a` silent/constant/agreeing-wrong controls where `disagree == 0` or `flux_peer <= 0`, and `regime_shift` where both arms fail).
2. **`Blocker #3` (`G2B.2` `MV_AddEqz`/`MV_MulEqz` lag separation & exogenous `port 0/1` `t = 64` probe starvation):**
   - Do **not** contort `W_diag = 64` or `mi_threshold_bits` now (ahead of `M5`'s width-scaling estimator study) to suppress occasional `t = 64` 1-node `Delay{0}` probes on unpredictable exogenous `lfsr_deg11` channels (`port 0`/`1`). Testing a finite-sample hypothesis during juvenile grace and **executing it at graduation (`t = 192/250`) when out-of-sample facts fail to fund its debt** is Charter §3.1.3 working as designed!
   - Instead, `G2B.2`'s anti-starvation check was over-broadly counting exogenous driver-port juvenile rejections against `target_port = 2`'s comparator stability, and `{view_0.delay = 1, view_1.delay = 0, (3, 4)}` let Stage 2 legitimately save a `Split` by mixing `ch0 + ch1` (`mask = 0x3`).
   - **Clean Resolution (`Proposal M2B-B3` — Blocker Report Option `a`):** Use `{view_0.delay = 0, view_1.delay = 3, (5, 6)}` on `MV_AddEqz_k2_Comb` and `MV_MulEqz_k2_Comb` so `ch1` (`0x2`) and `ch0` (`0x1`) separate cleanly across Stage 0 and Stage 3, and scope `G2B.2(i)`'s `starvation_prune_events == 0` assertion to **target-port and comparator nodes** (`comparator_or_target_starvations == 0`).

### 6.3 Ratified Post-`83ca660` Blocker Decisions (`2026-11-15` User Sign-Off)

All three proposals above (`M2B-B1` unified per-node debt-amortization maturation law + `new_nodes * initial_energy` with Rule-4 golden governance, `M2B-B2` atomic 1-sided comparator bankruptcy refutation preserving `W_obs = 256`, and `M2B-B3` `{0, 3, (5, 6)}` lag separation + target/comparator anti-starvation scope preserving `W_diag = 64` until `M5`) have been **ratified by the user (`2026-11-15`)** and folded into [`docs/milestone_2b_plan.md`](milestone_2b_plan.md) (§2.2, §2.3, §3, §6, §8.1; prior `doc/milestone_2b_blocker_report.md` retired after Commit 1 `ca7136c`) for execution by the `m2b_verify` swarm (`holdout 40000..=40019` remains locked).

---

## 7. Post-`ca7136c` Stage-3 Blocker Audit: Two-Clause Causal-Dilution Decidability (`Proposal M2B-C1`)

Following Commit 1 (`ca7136c`) in the `m2b_verify` worktree, `tests/golden_traces.rs` (`1/1` bit-identical), `verify-m1`, `verify-m2a`, **`G2B.1` (`300/300`)**, **`G2B.3` (`140/140`)**, **`G2B.4` (`179/180`, `99.44%`, `dis_prunes = 0`)**, and **`G2B.5` (`40/40`, `100%`, `+1.4385` b/sym advantage)** all pass with `0` guardrail exclusions ([`docs/milestone_2b_stage3_blocker_report.md`](milestone_2b_stage3_blocker_report.md)). Only `G2B.2` fails (`140/200`, `70.0% < 90.0%`): Stage 2 solves all `200/200` cases at `clean_mis = 0`, `target_starv = 0`, `comp_starv = 0`, and Stage 3 discovers the exact `e_alt = 0` disjoint backup on all three remaining tasks (`MV_Affine_k2_TphSplit`, `MV_Eqz_k2_Comb`, `MV_MulEqz_k2_Comb`), rejecting all three **solely** at the unscaled `horizon_ceiling = 512` (`2 * W_obs`) gate inside [`src/structural/growth.rs`](../src/structural/growth.rs) (`compute_debt_amortization_juvenile_ticks`).

### 7.1 Direction Check (`Question a`): Per-Node Economy Is Working as Designed; Eliminate the Split-Brain Ceiling

1. **Why Option (b) (Cone-Pooled Debt / Relaxed `Split` Share) Must Be Rejected:**
   - Under [Charter §3.1.2 & §3.1.3](foundational_research_charter_v3.md#312-metabolism), individual per-node description debt $\Delta L_v$ and normalized retrograde sensitivity credit $\text{share}(v) / Z_{\text{tot}}$ are the primary defense against `M3` credit free-riders (`H4`). Pooling debt across sibling nodes in a sprouted cone would allow parasitic branches or redundant `Split` nodes to survive graduation by riding on productive siblings, while breaking bit-identical `tests/golden_traces.rs`.
2. **Why Option (c) (Narrowing `G2B.2` or Doubling `W_obs`) Must Be Rejected:**
   - Dropping `MV_Eqz_k2_Comb`, `MV_MulEqz_k2_Comb`, or `MV_Affine_k2_TphSplit` weakens nonlinear and causal `TicksPerHop` coverage to protect an unscaled M1 ceiling (violating Principle 1 & Rule 2). Doubling `W_obs` globally doubles `ReserveCap::HorizonDerived` drainage latency (`tests/regime_shift.rs`) and pushes `Step B` wake refutation past `G2B.5`'s post-fault window.
3. **Root Cause — Anti-Epicycle Diagnosis of `compute_debt_amortization_juvenile_ticks`:**
   - Commit 1 (`ca7136c`) already had to introduce a Stage-2-only boolean workaround (`allow_deep_base_scaling = true` in Stage 2 so `c2a` at `d_cone = 14`, `t_base = 1175`, `t_debt = 1144` could pass, while setting `allow_deep_base_scaling = false` in Stage 3).
   - That boolean flag is a textbook split-brain seam: in Stage 3 it immediately rejected `MV_Affine_k2_TphSplit` (`t_debt = 308 <= 512`) solely because `M2B-D1`'s already-ratified `effective_comparator_juvenile_ticks(5, 1.5641) = 557` exceeded `512`.
   - Meanwhile, on `MV_Eqz_k2_Comb` (`EqzAddConst` + `Sub` + `Eqz`, `8` new nodes), exact first-principles trace physics explains `n31(Split)` (`ConstLoop(1)`'s `Split`, `debt = 29.37` bits, `r_hat = 0.03507` b/tick, `td = 838`): at $k = 2$, `Eqz` compresses uniform $\mathbb{Z}_4$ into a `75%/25%` Bernoulli stream ($H_2(0.25) = 0.811$ b/tick; `~0.756` b/tick over 64-tick KT warmup), and on the `75%` non-zero ticks `n31` is attenuated through `sens(n31) = 1/(2^k - 1) = 1/3`, `Add` fan-in `1/2`, and `sens(Add) = 1/3` ($\implies w_{n31} = 1/18$) while downstream `Eqz_alt`, `Sub_comp`, and `Eqz_comp` draw unit weight `1.0` each ($Z_{\text{tot}} = Z_{\text{alt}} + 2 = 4.30$), giving analytical $\mathbb{E}[\hat{r}_{n31}^+] \approx 0.035$ b/tick.

### 7.2 Quantitative Audit of Blocker Option (a) vs. `Proposal M2B-C1` (`Option a+`)

Evaluating Blocker Report Option (a) ($H_{\text{Opt (a)}} = \lceil 2 W_{\text{obs}} \cdot (t_{\text{base}} / t_{\text{juv,min}}) \rceil$) exposes **two mathematical flaws** when tested against tail statistics and adversarial noise:

1. **Inverted $Z_{\text{alt}}$ Scaling on Multi-Node Cones (`MV_Eqz` Binomial Tail Trap):**
   - In [`src/structural/economy.rs`](../src/structural/economy.rs) (`apply_tick_core`), comparator credit is divided by **`total_ancestry` $= Z_{\text{tot}} = Z_{\text{alt}} + 2$**.
   - However, [`effective_comparator_juvenile_ticks`](../src/structural/growth.rs) multiplies flat `t_eff(2) = 128` by $\frac{Z_{\text{alt}} + 2}{Z_{\text{alt}}} = 1 + \frac{2}{Z_{\text{alt}}}$, which **puts $Z_{\text{alt}}$ in the denominator** (converting Stage-2 multi-node time to Stage-3 time while omitting Stage-2's own $Z_{\text{alt}}$ dilution at $d \le 3$).
   - Consequently, as `alt` grows from a 1-node cone ($Z_{\text{alt}} = 0.90, Z_{\text{tot}} = 2.90$) to an 8-node `EqzAddConst` comparator ($Z_{\text{alt}} = 2.30, Z_{\text{tot}} = 4.30$), true denominator dilution **increases** from $2.90\times$ to $4.30\times$, whereas Option (a)'s ceiling **shrinks** from `1,652` ticks down to `960` ticks! While deterministic clean `G2B.2` (`m = 20/64` ones in `[192..256]`) clears `840 <= 960` by `120` ticks, under noisy or sparse Binomial($64, 0.25$) realizations ($m \le 9\text{–}10/64$ ones), $t_{\text{debt}}$ rises to `1,050`–`1,250 > 960`–`1,060` and **fails**.
2. **Un-Gated Small-$Z_{\text{alt}}$ Juvenile Over-Shielding Under Noise (`Threat A`):**
   - In [`evaluate_stage3_candidate_pre`](../src/structural/growth.rs), noisy Stage-3 candidates only check `flux_alt > 0.0` and `e_alt_diag + min_match_gain <= e_null_diag` (unlike Stage 2, which checks `projected_survival > delta_l_sprout` and `j_mat_cand < j_mat_curr`).
   - Under raw Option (a) alone, a barely-positive noisy candidate (`flux_alt = +3.0` bits over `64` ticks, $\Delta L_{\text{new}} = 85$ bits $\implies$ whole-cone crossover $W^*_{\text{cone}} = 1,818 \gg 512$ ticks) with $d_{\text{alt}} = 4, Z_{\text{alt}} = 0.90$ receives `H_Opt(a) = 2,388` ticks—creating an **un-prunable juvenile zombie** for the rest of $T = 2000$ (since [`process_wake_comparator_events`](../src/structural/learner.rs) skips `!comparator_cone_is_mature`).

**Clean Zero-Free-Parameter Synthesis (`Proposal M2B-C1` — Two-Clause Subgraph Crossover + Total-Ancestry Dilution Law):**

Eliminate `allow_deep_base_scaling: bool` entirely and enforce two orthogonal, zero-free-parameter decidability bounds uniformly across Stage 2 and Stage 3 in [`compute_debt_amortization_juvenile_ticks`](../src/structural/growth.rs):

1. **Clause (i) — Subgraph Epistemic MDL Crossover Bound (`Anti-Noise Guard`):**
   $$t_{\text{cone}}(V_{\text{new}}) \triangleq d_{\text{cone}} + \left\lceil \frac{\sum_{v \in V_{\text{new}}} \Delta L_v}{\sum_{v \in V_{\text{new}}} \hat{r}_v^+} \right\rceil \le 2 W_{\text{obs}} \quad (= 512\text{ ticks at defaults}).$$
   - By the mediant inequality ($\frac{\sum \Delta L_v}{\sum \hat{r}_v^+} \le \max_v \frac{\Delta L_v}{\hat{r}_v^+}$), **every** sprout that already passed `t_debt <= 512` in Commit 1 (`tests/golden_traces.rs`, `verify-m1`, `verify-m2a`, `G2B.1`, `G2B.3`–`G2B.5`, and `140/140` of `G2B.2`) satisfies $t_{\text{cone}} \le 512$ **identically**.
   - Every multi-node / deep exact cone also satisfies $t_{\text{cone}} \le 512$ with wide margin, while weak noisy correlations ($\hat{R}^+_{\text{new}}$ small relative to $\Delta L_{\text{new}}$) are rejected immediately with `CandidateUndecidable`.
2. **Clause (ii) — Per-Node Causal-Dilution Ceiling (`Anti-Parasite Guard`):**
   $$t_{\text{juv}}(V_{\text{new}}) = \max(t_{\text{base}},\, t_{\text{debt}}) \le H_{\text{node}}(V_{\text{new}}) \triangleq \left\lceil 2 W_{\text{obs}} \cdot \max\!\left(\frac{t_{\text{base}}}{t_{\text{juv,min}}},\ Z_{\text{tot}}\right) \right\rceil,$$
   where $Z_{\text{tot}} \ge 1.0$ is the replay-measured mean active causal ancestry on the target sink (`port_shares[out_port].sum()` in Stage 2; `comparator_shares[comp_idx].sum() = Z_alt + 2` in Stage 3).

| Workload / Regime | $d$ | $N_{\text{new}}$ | $Z_{\text{tot}}$ | $t_{\text{cone}}$ ($\le 512$) | $t_{\text{base}}$ | $t_{\text{debt}}$ | Opt (a) Ceiling | **`M2B-C1` $H_{\text{node}}$** | Outcome |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | :--- |
| M1 1-node (`k=1, d=1`) | `1` | `1` | `1.00` | `45` | `128` | `45` | `512` | **`512`** | Exact M1 reduction (`t_juv = 128`) |
| M2a `c2a` deep chain (`d=14`) | `14` | `15` | `7.71` | `234` | `1175` | `1144` | `4700` | **`4700`** | Passes (`t_juv = 1175`, no `bool` flag) |
| `MV_Affine_k2_TphSplit` | `5` | `7` | `3.56` | `55` | `557` | `308` | `2228` | **`2228`** | Passes (`t_juv = 557`, matures `t ≈ 820`) |
| `MV_MulEqz_k2_Comb` | `5` | `16` | `5.52` | `253` | `383` | `546` | `1532` | **`2826`** | Passes (`t_juv = 546`, matures `t ≈ 850`) |
| `MV_Eqz_k2_Comb` (`m=20/64`) | `2` | `8` | `4.30` | `226` | `240` | `840` | `960` (tight) | **`2200`** | Passes (`t_juv = 840`, matures `t ≈ 1100`) |
| `MV_Eqz_k2_Comb` (`m=10/64` tail) | `2` | `8` | `3.91` | `334` | `262` | `1210` | `1048` (**FAIL**) | **`2004`** | Passes sparse tail cleanly |
| Weak noisy fluke (`+3b/64t`, `d=4`) | `4` | `3` | `2.90` | **`1818 > 512`** | `597` | `1812` | `2388` (**LEAK**) | Rejected by Clause (i) | Blocked (`CandidateUndecidable`) |
| `M3` near-zero-share parasite leaf | `2` | `5` | `3.50` | `200` | `220` | **`10002`** | `880` | **`1792`** (`10002 > 1792`) | Blocked (`CandidateUndecidable`) |

### 7.3 Full-Surface Code Audit: Two Coupled Hygiene Catches in `src/structural/learner.rs`

Auditing Commit 1 (`ca7136c`) around Pass 2 (`ArkheLearner::observe`, `src/structural/learner.rs:2550–2560`) uncovered two concrete oversights that **must** be fixed alongside `M2B-C1`:

1. **Dropped Stage-3 `CandidateUndecidable` & `LagHorizonBound` Telemetry (Rule 3 Violation):**
   - At `learner.rs:2553–2558`, Pass 2 forwards `stage3_tel.sprout_blocked_max_nodes`, `stage3_tel.sprout_budget_exhausted`, and `stage3_tel.candidate_list_truncated`, **but omits `self.candidate_undecidable += stage3_tel.candidate_undecidable;` and `self.lag_horizon_bound += stage3_tel.lag_horizon_bound;`**—silently discarding `CandidateUndecidable` increments emitted by `compute_debt_amortization_juvenile_ticks` in Stage 3.
2. **Premature `stage3_exhausted` Before `obs_window` Fills (`learner.rs:2554` vs. `2425`):**
   - Stage 2 (`learner.rs:2425`) gates `stage2_exhausted = true` on `obs_full` (`self.obs_window.len() >= observation_horizon(...)`), whereas Stage 3 (`learner.rs:2554`) sets `stage3_exhausted = true` unconditionally whenever `stage3_ok_none && stage3_tel.sprout_budget_exhausted == 0`. Gating `stage3_exhausted = true` on `obs_full && (diag_zero_res || !self.port_needs_stage2(p))` (matching Stage 2's `obs_full` invariant) prevents a transient noise burst on a partially filled replay window from permanently locking out Stage 3 for the remainder of a noisy `G2B.4` run while still donating idle `TickBudget` once `obs_window` is full.

### 7.4 Operational & Quota Recommendation

With `gemini-4-hydrogen-*` at `5%` remaining (resetting `2026-11-21T11:27:38Z`) while `claude-opus-5-5-high` and `claude-sonnet-5-5-high` stand at `100%` (resetting in ~4 hours at `2026-11-16T00:34:28Z`), `M2B-C1` + the two `learner.rs` telemetry/exhaustion lines touch ~35 lines across `src/structural/growth.rs` and `src/structural/learner.rs` with zero golden-trace churn (`tests/golden_traces.rs` remains `1/1` bit-identical by the mediant inequality). Keep `@monitor` paused on Hydrogen and execute the surgical `M2B-C1` patch + `verify-m2b` dev/confirmation runs (`0..20`, `20..1020`, `20040..=20059`; keeping `40000..=40019` locked) using available `claude-sonnet-5-5-high` / `claude-opus-5-5-high` capacity before its `00:34Z` reset window.

### 7.5 Ratified Post-`ca7136c` Decision (`2026-11-15` User Sign-Off — `M2B-C1`)

Per [Charter §7.1](foundational_research_charter_v3.md#71-milestone-lifecycle-and-decision-rights) and [`.github/copilot-instructions.md`](../.github/copilot-instructions.md) Rules 2, 3, 4, 5, and 7, the Principal Investigator **ratified `Proposal M2B-C1` (`2026-11-15`)** for governance-doc handoff to the `swarm-m2b-verify-d32f3276` worktree:

1. **`M2B-C1` (Two-Clause Subgraph Crossover + Total-Ancestry Dilution Decidability Law):** Remove `allow_deep_base_scaling` and enforce both (i) $t_{\text{cone}}(V_{\text{new}}) = d_{\text{cone}} + \lceil (\sum_{v \in V_{\text{new}}} \Delta L_v) / (\sum_{v \in V_{\text{new}}} \hat{r}_v^+) \rceil \le 2 W_{\text{obs}}$ (`512` at defaults) and (ii) $t_{\text{juv}}(V_{\text{new}}) = \max(t_{\text{base}}, t_{\text{debt}}) \le \lceil 2 W_{\text{obs}} \cdot \max(t_{\text{base}} / t_{\text{juv,min}}, Z_{\text{tot}}) \rceil$ uniformly across Stage 2 and Stage 3 in [`src/structural/growth.rs`](../src/structural/growth.rs), alongside forwarding `stage3_tel.candidate_undecidable` + `stage3_tel.lag_horizon_bound` and gating `stage3_exhausted` on `obs_full` in [`src/structural/learner.rs`](../src/structural/learner.rs).
2. **Quota & Worktree Execution Authorization:** Keep `@monitor` paused to preserve the remaining `5%` `gemini-4-hydrogen-*` quota; temporary Tier-1/3 execution on `claude-sonnet-5-5-high` / `claude-opus-5-5-high` is authorized for applying `M2B-C1` in `swarm-m2b-verify-d32f3276` and running dev (`0..20`, `20..1020`) and confirmation (`20040..=20059`) suites (`holdout 40000..=40019` remains strictly locked until user sign-off).
