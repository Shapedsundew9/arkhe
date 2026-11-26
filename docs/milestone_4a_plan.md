# Milestone M4a: Sleep as Internal Falsification — Plan (Chief Scientist Revision)

**Date:** 2026-11-24 (Phase 0.5 Pre-Registration: 2026-11-25; Phase 3 Closeout: 2026-11-25)  
**Status:** `Phase 1–2 & S3 Single-Shot Holdout PASSED (0f62e2f, source_tree_fnv1a128 = 349fe16d144f4fcd2c2184fe944d6c51); Phase 3 Closeout complete (60000..=60099 permanently closed allow_with_flag: false; S4 sign-off pending on closeout commit).`  
**Charter Links:** [Foundational Research Charter v3](foundational_research_charter_v3.md) (§4.2, §6 `H5`/`H6`, §7, §8, §2.8, [DR #49–#50](foundational_research_charter_v3.md#appendix-a-decision-record)), [Parameter Inventory](parameter_inventory.md), [Research Record](research_record.md)

This plan follows the [milestone plan template](templates/milestone_plan_template.md). It supersedes the
junior draft of the same date. Phase 0.5 Pre-Registration was signed off under [DR #50](foundational_research_charter_v3.md#appendix-a-decision-record),
Phase 1 (`verify-m4a` at `0f62e2f`), Phase 2 (`520..720` disjoint dev, `20200..=20299` confirmation, and clean `check_retained_baselines.py --require-clean`, `#64`), `S3` authorization (`#65`), and the single-shot 100-seed holdout (`60000..=60099`, `#67`) all passed with `overall_passed = true`, `excluded_cases = 0`, and `G4A.7` (`H6`) ruling `SaSuperiorDropReplay` (`delta_6 = +14.61` bits -> drop `S-B` replay in `M4b`). Holdout `60000..=60099` is permanently closed (`allow_with_flag: false`) and evidence is archived under [`docs/evidence/m4a/`](evidence/m4a/); **`S4` milestone sign-off** is pending `@chief-scientist` approval on the Phase 3 closeout commit.

## 0. Chief Scientist Review of the Draft

The draft had the correct governance outline but no mechanism, no definition of equal compute, no
definition of a spurious cycle, and no evidence that sleep, as currently implemented, can beat
wake-only. A Rule-9 feasibility argument for `H5` cannot be written until those gaps are closed. The
findings below come from the current source and are binding inputs to M4a-0.

### 0.1 Findings (source-verified)

| ID | Finding | Evidence | Consequence |
| :--- | :--- | :--- | :--- |
| `F1` | Sleep exists only as the M2b test primitive `run_offline_loopback`. It has no scheduler, fact buffer, `S-B` replay, compute metering, or wake-state restore on exit. | [`learner.rs` `run_offline_loopback`](../src/structural/learner.rs), [`live.rs` `settle_sleep`](../src/structural/live.rs) | M4a must build the scheduled wake–sleep machinery before characterisation can start. |
| `F2` | In sleep, two-route comparator credit is asymmetric: the primary acts as the reference, so agreement and disagreement flux reach only the `alt + SUB + EQZ` cone. A primary can change in sleep only through three-route majority excision. | [`economy.rs` `observe_comparators`](../src/structural/economy.rs) (Sleep branch), `run_offline_loopback` majority block | `S-A` can filter alternative derivations only. It cannot adjudicate a two-route conflict in which the primary is wrong. This is the main reason `H5` may lack mechanistic opportunity. |
| `F3` | Loopback feeds every input port its own prediction, including exogenous driver channels (for example LFSR drivers) whose predictions are at null level. | `run_offline_loopback` (`curr_input ← predictions` for all ports) | Cross-channel derivations receive hallucinated constant inputs. Their comparators can register disagreements that look conclusive and starve correct, fact-supported structure. This is a Charter §3.1.7 threat (mutually agreeing hallucinations and wrongful pruning). |
| `F4` | Wake already prunes fact-disagreeing alternatives with the two-clause rule (DR #40–#42). | [Research Record §10](research_record.md#10-stage-m2b-comparator-economics-theoretical-analysis-2026-11-14-charter-dr-4043) | Under equal compute, sleep has a comparative advantage only where wake refutation is slow or impossible: (i) noise masking in the `Noisy` stratum, where loopback comparison is exact and noise-free; (ii) contexts that occur rarely in the fact stream; (iii) description length held by spurious or unrefuted structure. Its benefit should therefore appear mainly in $L_{\text{epistemic}}$, not in boundary KT. |
| `F5` | `CTW` and `ReedsSloane` are `AnalyticalUnmetered`. | [Research Record §6.2](research_record.md#62-critical-open-issues--dropped-nuance-relevant-to-m2m5) item 3 | They may appear only as labelled references, never in an equal-compute gate. |
| `F6` | "Spurious cycle" has no operational definition. Generators export a `FiniteStateSpec`, and ε-machine and synthesis certification already exist. | [`epsilon_machine.rs`](../src/env/epsilon_machine.rs), [`tiers.rs`](../src/env/tiers.rs) | Spurious structure is labelled against the oracle of the true generator class (Principle 5). |
| `F7` | Loopback must start from live `DELAY` state, never from `init` values. | [Research Record §6.2](research_record.md#62-critical-open-issues--dropped-nuance-relevant-to-m2m5) item 4 | Retained as invariant `I3`. |

### 0.2 Binding Rulings (Level 2, recorded as [DR #49](foundational_research_charter_v3.md#appendix-a-decision-record) and refined in [DR #50](foundational_research_charter_v3.md#appendix-a-decision-record))

The DR #49 rulings below governed M4a-0 implementation and were finalized in [DR #50](foundational_research_charter_v3.md#appendix-a-decision-record) (`089ed72`): adding `R2` paired wake-envelope `max_episode_node_evals` & zero-flux sleep sensitivity skip, `R3` prediction-consistent sleep-entry window ($d_{\text{cone}}+1$ / $d_{\text{closed\_loop}}+1$), `R4` transitive closed-loop grounding + cross-port KT homogeneity guard + `S-B` loopback suspension, `R5(a)` fact-support solvency + `alt` exoneration + reference immunity, `R5(c)` Popperian deterministic loopback refutation, `I6` sleep pause of `juvenile_ticks` & `smoothed_drain_rate`, and `K2` opportunity scoping under canonical `eval_l_epistemic_bits`. They add no free constants: every quantity reuses an existing derived horizon or a zero-bit MDL decision.

1. **`R1` — Milestone split.** M4a becomes **M4a-0** (infrastructure, oracle instrumentation, opportunity
   analysis and characterisation, with no gates) followed by **M4a** (pre-registered confirmatory
   milestone). Rule 8 requires characterisation before thresholds. Bundling both would invite tuning
   toward exploratory numbers.
2. **`R2` — Equal compute.** The matching currency is the **substrate tick**: one synchronous evaluation
   of the live circuit. Wake ticks, sleep loopback ticks and `S-B` replay ticks are each charged one
   tick, and every arm receives the same total $T_{\text{total}}$.
   - The **environment is paused** during sleep, so sleep arms observe fewer facts. This is the cost
     sleep must overcome (Charter §8).
   - The per-wake-tick candidate-search `TickBudget` allowance uses the same policy function in every arm.
   - A secondary **node-evaluation ledger** covers live steps, candidate replay and counterfactual
     sensitivity re-evaluations. It is a **one-sided audit**: a seed fails if any sleep arm uses more
     node-evaluations than wake-only, because sleep may not win by using more computation. No slack is
     permitted.
   - If M4a-0 shows that this audit fails for structural reasons, the node-evaluation ledger becomes the
     matching currency through a Level-2 decision recorded **before** pre-registration.
3. **`R3` — State discipline.**
   - Sleep starts from live registers (`I3`).
   - On exit, the wake register snapshot is restored, remapped through the `remove_nodes`
     old→new map with `recompile_preserving_state` if sleep excised nodes.
   - Economy state (energies, deaths, excisions) persists. This is the effect of sleep.
4. **`R4` — Grounded-loop rule (fixes `F3`).**
   - A boundary channel $c$ is **grounded** at sleep entry iff its port's calibrated forecast beats null,
     $\hat{q}_c > p_{\text{null},c}$. This is the existing handoff criterion.
   - In `S-A`, a comparator is **suspended** (no reward, charge or excision) for the segment if the
     causal-support leaves of its primary or alt touch an ungrounded channel. Counter:
     `SleepComparatorSuspendedUngrounded`.
   - In `S-B`, ungrounded channels are driven from fact-buffer replay instead.
   - Basis: a disagreement is conclusive only between derivations whose inputs the held model generates
     itself. Two derivations fed with fabricated inputs are not independent evidence.
5. **`R5` — Sleep credit.** Both arms keep the DR #40 sleep economics unchanged: primary as reference,
   $\iota$-weighted agreement and full-weight disagreement. This preserves `G2B.3` and the golden traces.
   `S-B` adds two mechanisms, which makes `H6` a real mechanistic contrast:
   - **(a) Fact adjudication.** A comparator with sleep disagreement is replayed teacher-forced over the
     fact buffer. The derivation with more recorded-fact disagreements takes the disagreement charge;
     ties default to DR #40. This is the only way two-route sleep can reach a wrong primary (`F2`).
   - **(b) Statistics match.** For each grounded port, free-run and replayed symbol sequences are compared
     with a two-sample KT code-length test at context order $d = d_{\text{cone}}$, the held cone register
     depth. Order 0 would miss reordered short cycles such as `0101` against `0011`.
     - The port **fails** if coding the two samples separately saves more than **0 bits** relative to
       coding them jointly. This is an MDL model-selection decision with no free threshold.
     - A failing port's positive sleep agreement flux for that segment is voided. Disagreement charges
       still apply.
6. **`R6` — Fact buffer.** A bounded ring of the most recent $W_{\text{obs}}/2$ wake frames, together with
   the register snapshot at the oldest frame (`ObservationWindow::push_frame_with_state`).
   $W_{\text{obs}} = t_{\text{juv}} + 2W_{\text{diag}} = 256$, so the buffer holds 128 frames.
   - The half-horizon size gives `S-B` a **balanced two-sample design** (`R7`), which minimises variance
     for a fixed sleep allotment.
   - Arms record the buffer under `SleepSchedule::None` as well, but never read it.
   - Telemetry: `FactBufferEvictions`.
7. **`R7` — Schedule (derived; never selected empirically).**
   - The wake segment is $W_{\text{wake}} = W_{\text{obs}}$. A sprout born at segment start therefore
     matures, and its diagnostic windows fill, before it is judged in sleep.
   - The sleep segment is $W_{\text{sleep}} = W_{\text{obs}}$: one full economy evidence horizon.
   - `S-A` spends all $W_{\text{obs}}$ sleep ticks in loopback. `S-B` spends $W_{\text{obs}}/2$ in replay
     and $W_{\text{obs}}/2$ in loopback, so `S-A` and `S-B` are equal-compute to each other.
   - **Onset:** a segment runs only when at least one mature, grounded, scoreable comparator exists.
     Otherwise it is skipped and **not charged** (`SleepSegmentSkippedNoScoreableComparator`).
     Sleep with nothing to falsify is pure waste, and this keeps sleep arms bit-identical to wake-only
     until onset (`I2`).
   - M4a-0 may report sensitivity to $W_{\text{sleep}} \in \{W_{\text{diag}}, 2W_{\text{diag}}, W_{\text{obs}}\}$
     as a description only. Any change needs a written Level-2 derivation and may not pick the best
     observed value.
8. **`R8` — Oracle labels (fixes `F6`).**
   - A free-run word of length $m+1$ on a port is **forbidden** if it has zero probability under the
     clean generator's `FiniteStateSpec` language. Here $m$ is the clean ε-machine's Markov or
     synchronisation order. Noisy tasks use the clean underlying spec, because noise is declared.
   - A **spurious cycle** is a loopback attractor (register-state recurrence) whose orbit contains a
     forbidden word.
   - A derivation is **oracle-correct** if it is certified against the spec (`certify_circuit_against_spec`)
     on the generator's reachable set.
   - **Wrongful excision** is any sleep-caused death or excision of an oracle-correct node.
   - Labels are instrumentation only. The learner never reads them.
9. **`R9` — Two-sided `H5` evidence.** `H5` is gated on a **spontaneous** task suite with no injected
   structure. Planted injections test only the separate "spurious cycles are rejected" criterion.
   Planted tasks may not substitute for a spontaneous `H5` result.
10. **`R10` — Unmetered baselines.** Per `F5`, unmetered baselines are references only.

### 0.3 Invariants (each needs a unit or integration test in M4a-0)

| ID | Invariant |
| :--- | :--- |
| `I1` | `SleepSchedule::None` (default) leaves the learner bit-identical: `golden_traces` `1/1` and `check_retained_baselines.py --require-clean` passing. |
| `I2` | Before sleep onset, every sleep-arm trajectory is bit-identical to wake-only on the same seed. |
| `I3` | Loopback starts from live registers. The wake snapshot is restored exactly on exit, after remapping through `remove_nodes` if sleep excised nodes. |
| `I4` | $\Delta E(T) \equiv -\Delta L_{\text{null}}^{\text{epistemic}}(T) + E_{\text{exog}}$ holds across the whole run, wake and sleep, to numerical-guard tolerance. |
| `I5` | Every arm consumes exactly $T_{\text{total}}$ substrate ticks; the compute ledger difference between arms is exactly 0. |
| `I6` | Boundary reward is 0 throughout sleep; null models stay frozen in sleep; replay never updates wake null models or scorers. |
| `I7` | Suspended comparators (`R4`) receive exactly zero flux and zero excision. |

## 1. Charter Hypotheses & Decision Rights

- **H5:** Sleep filtering beats compute-matched wake-only learning (Charter §6). It is operationalised
  as lower total null-calibrated $L_{\text{epistemic}}$ at equal $T_{\text{total}}$, with boundary-KT
  non-inferiority (§3).
- **H6:** Consistency-only sleep (`S-A`) performs as well as consistency plus fact replay (`S-B`). The
  outcome is a pre-registered **decision**, not a pass/fail gate:
  - If `S-A` is equivalent to `S-B`, replay is dropped for M4b (Charter §4.2).
  - If `S-B` is superior, replay is retained.
- **H3 (offline component, retained):** M4a re-exercises loopback falsification at scale, but M2b remains
  the source of record.
- **Deferred:** equality saturation, theorem discovery, compression rewrites and promotion (M4b, `H7`);
  transfer (M4c); width scaling beyond $k \in \{1, 2\}$ (M5); action (M6). Perturbed multi-start dream
  lanes (bit-parallel basin exploration, Charter §9) are **exploratory only** in M4a-0, and M4a does not
  gate on them.
- **Decision rights:** per [Charter §7.1](foundational_research_charter_v3.md#71-milestone-lifecycle-and-decision-rights)
  and [`repo-common.md`](instructions/repo-common.md) Rule 7.
  - The Chief Scientist approves the transition from M4a-0 to M4a and the pre-registration, and holds
    `S3` and `S4`.
  - No holdout run may occur before `S3`.

## 2. Exploratory Characterisation (M4a-0) — Completed (`089ed72`, DR #50)

M4a-0 was implemented across five commits (`d053aea`, `7687172`, `abeb8fe`, `2bb81c0`, `089ed72`; `cargo test --locked` `242/242`, `golden_traces` `1/1`, `G2B.3` intact) and characterised on smoke `0..20` (`/tmp/explore_m4a_0_20_dr50.json`, `55.20s`, 16 threads) and `20..220` (`/tmp/explore_m4a_20_220_dr50.json`, `550.98s = 9.18 min`, 16 threads, `200` seeds × `36` tasks × `3` arms = `21,600` arm runs). `Clean` ($\varepsilon = 0.0$) and `Noisy` ($\varepsilon = 0.05$) regimes are never pooled.

1. **Invariants, Compute Audit (`R2`), and Guardrails (`0..20` and `20..220`):**
   - **Smoke `0..20`:** `max_epistemic_mdl_err = 1.31e-13 <= 1e-11` (`I4`), `max_compute_ledger_diff = 0` (`I5`), `i2 = 0`, `i6 = 0`, `i7 = 0`, `max_node_eval_ratio_sleep_over_wake = 1.0000`, `WE = 0`, `port_locks_lost = 0`, `SproutBlockedMaxNodes = 0`, `PL1:k1/k2` `1.00/1.00`, `PL2:k1/k2` `1.00/1.00` clean (`0.95/0.65` noisy).
   - **Characterisation `20..220` Invariants & `R2`:** `max_epistemic_mdl_err = 1.31e-13 <= 1e-11` (`I4`), `max_compute_ledger_diff = 0` (`I5`), `i2_pre_onset_violations = 0`, `i6_violations = 0`, `i7_violations = 0`, and **`max_node_eval_ratio_sleep_over_wake = 1.0000`** (`frac_le_1 = 1.0000` across all `7,200` paired arm-runs), settling `R2` under the paired wake-envelope cap (`max_episode_node_evals`).
   - **Characterisation `20..220` Guardrails:** `sprout_blocked_max_nodes = 0` (`0` exclusions across all `21,600` runs); observable totals: `sprout_budget_exhausted = 367,612`, `lag_horizon_bound = 165`, `candidate_undecidable = 1,009,169`, `sleep_comparator_suspended_ungrounded = 6,131,504`, `sleep_segment_skipped_no_scoreable_comparator = 145,013`, `fact_buffer_evictions = 106,289,328`, `sleep_match_failures = 8,617`, `max_suspended_comparator_ticks = 11,520`, `max_facts_forgone = 3,840`.
2. **Mechanistic `K2` Opportunity Analysis & Canonical `H5` Metric (`20..220`):**
   - **Cumulative MDL (`k2_null_triggered_on_cum_opp = true`):** Cumulative `delta_l_epistemic` opportunity was `<= 0` on all `16/16` `SP` tasks (`max_sp_opportunity_bound_cum_bits = 0.0`) solely because sleep arms pause the environment and forgo `3,840` wake observations. Under `DR #50`, the canonical `H5` metric is post-episode equal-dataset evaluation MDL `eval_l_epistemic_bits = L(C) + W_obs * eval_KT` over $W_{\text{obs}} = 256$ fresh continuation symbols.
   - **Clean `SP` (8 tasks):** `0/8` positive onset-incorrect opportunity (`p_onset_oracle_incorrect_overall = 0.0` on all 8 tasks; `opportunity_bound_onset_eval_bits = 0.0` on 6/8, `-2.92` on `ChannelFault:k1:clean` with `+10.29`/`+13.86` bits adv, `-569.05` on `ChannelFault:k2:clean`). Confirms `K2` scoping (`DR #50`): falsification-only sleep has zero opportunity on already-correct `Clean` `SP` circuits; general `Clean` `SP` compression superiority (`H5 + H7`) is scoped to `M4b`.
   - **Noisy `SP` (8 tasks, `mean / median / p05 / frac > 0` for `eval_l_epi_advantage_bits = wake - sleep`):**
     - **Positive-opportunity gated tasks (`M = 3`):**
       - `SP:Period6_LagRedundancy:k2:noisy`: `S-A +54.54 / +58.58 / +3.25 / 0.950`, `S-B +54.59 / +53.09 / +3.42 / 0.960` (`opp = +54.54 / +54.82`, `p_inc = 0.975`, `WE = 0/0`).
       - `SP:MV_Periodic:k1:noisy`: `S-A +139.26 / +144.29 / -5.54 / 0.935`, `S-B +141.79 / +146.52 / +3.55 / 0.950` (`opp = +135.27 / +135.78`, `p_inc = 0.320`).
       - `SP:MV_Periodic:k2:noisy`: `S-A +88.20 / +65.90 / -55.18 / 0.735`, `S-B +88.19 / +65.90 / -55.48 / 0.730` (`opp = +103.40 / +104.96`, `p_inc = 0.680`).
     - **Characterised-only noisy `SP` tasks (median `<= 0` or negative opportunity; gated in `G4A.1/2/4/6`):** `SP:Period6_LagRedundancy:k1:noisy` (`S-A +19.88 / +0.00 / -37.28 / 0.340`, `S-B +19.38 / +0.00 / -36.14 / 0.345`, onset `0.690`, `p_inc = 0.225`), `SP:MV_Lfsr:k1:noisy` (`+2.69` / `+13.19`), `SP:MV_Lfsr:k2:noisy` (`-0.48` / `+2.29`), `SP:ChannelFault:k1:noisy` (`-14.41` / `-226.28`), `SP:ChannelFault:k2:noisy` (`-1.32` / `-112.65`).
3. **Boundary KT Non-Inferiority (`G4A.4`, frozen $W_{\text{obs}} = 256$):**
   - **Clean (`SP` + `CT`, 14 tasks):** `mean = 0.000`, `p95 = 0.000`, **`max = 0.000`** across all `2,800` paired Clean runs (`100.0%`).
   - **Noisy (`SP` + `CT`, 14 tasks):** `S-A` task means `-0.013` to `+0.020` (`p95 <= 0.221`, `max <= 0.414`); `S-B` task means `-0.013` to `+0.033` (`p95 <= 0.243`, `max <= 0.379` except `SP:ChannelFault:k2:noisy` `max = 1.478`). Finite-window ($W_{\text{obs}} = 256, \varepsilon = 0.05$) binomial sampling produces `p95 ~ 0.14–0.24` b/sym even when true model regret is `0`.
4. **`PL` Spurious Rejection (`G4A.5`, `excision_rate / median_surv_segs / max_surv_segs`, all `0` WE):**
   - **Clean (4 tasks):** `PL1:k1` `1.000/0/0` & `1.000/0/0` (`+102.89` bits); `PL1:k2` `1.000/0/0` & `1.000/0/0` (`+108.45` bits); `PL2:k1` `1.000/0/0` & `1.000/0/0`; `PL2:k2` `1.000/0/0` & `1.000/0/0`.
   - **Noisy (4 tasks):** `PL1:k1` `1.000/0/1` & `1.000/0/1` (`+92.31` / `+92.40` bits); `PL1:k2` `1.000/0/0` & `1.000/0/0` (`+108.75` bits); `PL2:k1` `0.985/0/4` & `0.935/1/4` (`+365.98` / `+347.59` bits); `PL2:k2` `0.880/1/4` & `0.565/2/4` (`+644.66` / `+399.78` bits).
5. **Fact-Supported Preservation (`G4A.6`, `CT` + `SP` + `PL`):**
   - **Clean (`18/18` tasks):** **`0` wrongful excisions (`100.00%`), `0` port locks lost (`100.00%`)**.
   - **Noisy (`18/18` tasks):** **`0` port locks lost (`100.00%`)**; `20` total WE nodes across `15/7,200` runs: `S-A` `7/3,600` (`99.81%` zero-WE runs: `MV_Periodic:k1` `197/200 = 98.5%`, `CT2:k1` `197/200 = 98.5%`, `MV_Periodic:k2` `199/200 = 99.5%`, other 15 tasks `200/200`); `S-B` `8/3,600` runs (`13` WE nodes; `99.78%` zero-WE runs: `MV_Periodic:k1` `195/200 = 97.5%`, `CT2:k1` `198/200 = 99.0%`, `MV_Periodic:k2` `199/200 = 99.5%`, other 15 tasks `200/200`).
6. **`H6` Contrast (`S-A` vs `S-B`, `G4A.7`):**
   - `S-A` ties `S-B` on `SP:Period6_LagRedundancy:k2:noisy` (`+54.54` vs `+54.59`), `PL1`, and Clean `PL`/`SP`, while **`S-A` outperforms `S-B`** on `PL2:k2:noisy` (`0.880` vs `0.565`, `eval_l_epi(S-B) - eval_l_epi(S-A) = +244.87` bits), `PL2:k1:noisy` (`+18.39` bits), and `SP:ChannelFault:k1/k2:noisy`, yielding mean $\Delta_6 = +23.69$ bits across the `11` active falsification tasks (`G4A.3` + `G4A.5`).
7. **Runtime Extrapolation (16 threads):**
   - Measured `200` seeds (`20..220`) = `550.98s` (`2.755 s/seed`: `SP` `2.000`, `CT` `0.501`, `PL` `0.254` s/seed).
   - Projected `100`-seed confirmation (`20200..=20299`) & holdout (`60000..=60099`) = `275.49s` (`~4.6 min`).
   - Projected `200`-seed confirmatory dev sweep (`520..720`) = `~551s` (`~9.2 min <= 15 min` watchdog limit).

### 2.1 Task Suite (36 tasks across `SP`, `PL`, `CT` at $k \in \{1, 2\}$, `Clean` $\varepsilon = 0$ and `Noisy` $\varepsilon = 0.05$)

All tasks run at $k \in \{1, 2\}$ (Charter §7). `Clean` uses $\varepsilon = 0$. `Noisy` uses
$\varepsilon = 0.05$, matching `G2A4_EPSILON` and `G2B.4`. Horizons use `DEFAULT_M4A_HORIZON = 8_000` (`SP`, `CT`) and `DEFAULT_M4A_PL_HORIZON = 2_000` (`PL`), with `DEFAULT_M4A_FAULT_TICK = 1_000` ([`src/verify_m4a/tasks.rs`](../src/verify_m4a/tasks.rs)).

| Family | Role | Content (`9` task-width pairs × `2` regimes = `18` Clean + `18` Noisy) |
| :--- | :--- | :--- |
| `SP` — spontaneous (`8` Clean, `8` Noisy) | `H5` and `H6` gates (`M=3` positive-opportunity noisy tasks gated in `G4A.3`; all `16` gated in `G4A.1/2/4/6`) | `MV_Periodic`, `MV_Lfsr` (`G2B.2`-class multi-view `T2` tasks); `Period6_LagRedundancy` ($K=1$ shallow-vs-deep lag redundancy); `ChannelFault` (`G2B.5`-style fault recovery) |
| `PL` — planted (`4` Clean, `4` Noisy) | Spurious-cycle rejection (`G4A.5`) | `PL1_ShortCycle` (autonomous ring whose period divides the true period); `PL2_RareContext` (agrees with primary except in a low-stationary-probability context). Injected via `apply_external_edit` with exogenous ledger accounting. |
| `CT` — controls (`6` Clean, `6` Noisy) | Preservation and non-inferiority (`G4A.4`, `G4A.6`) | `CT1_ExogenousLfsr` (`R4` exogenous LFSR-driver cross-channel task); `CT2_RareEventCorrect` (rare-event-dependent correct structure); `CT3_NoComparator` (`I2` pre-onset bit-identity control) |

## 3. Pre-Registered Hypotheses & Falsifiable Success Criteria (`DR #50` Final)

`Clean` ($\varepsilon = 0$) and `Noisy` ($\varepsilon = 0.05$) strata are never pooled (`StratifiedGateAudit`, [`src/stats.rs`](../src/stats.rs)). The statistical unit is the independent `(task, k, eps, seed)` paired trajectory, requiring `excluded_cases == 0` (`sprout_blocked_max_nodes == 0`).

### 3.1 Gate Thresholds & Feasibility

| Gate | Stratum & Gated Tasks | Per-Seed Trajectory Pass Criterion | Target $P_{\text{target}}$ & FWER Rule | Rule-9 Feasibility Basis (`20..220`, $N=200$/task) |
| :--- | :--- | :--- | :--- | :--- |
| **`G4A.1` Conservation & Compute Accounting** | `Clean` (18 tasks) & `Noisy` (18 tasks); **both** `S-A` and `S-B` must hold on the seed | `max_epistemic_mdl_err <= 1e-9` b/sym (`I4`) **AND** `compute_ledger_diff == 0` (`I5`) **AND** `node_eval_ratio_sleep_over_wake <= 1.0` (`R2`) **AND** `sprout_blocked_max_nodes == 0` | `Clean`: $P_{\text{target}} = 1.0$ (`exact_mode`); `Noisy`: $P_{\text{target}} = 1.0$ (`exact_mode`) | Structural conservation laws; `20..220` empirical $= 100.0\%$ (`3,600/3,600` Clean, `3,600/3,600` Noisy; max MDL err $= 1.31\times 10^{-13}$, max ratio $= 1.0000$). |
| **`G4A.2` Backward Invariance & Sleep Isolation** | `Clean` (18 tasks) & `Noisy` (18 tasks); **both** `S-A` and `S-B` must hold on the seed | `I1` (`SleepSchedule::None` bit-identical to wake-only) **AND** `i2_pre_onset_violations == 0` (`I2`, full trajectory on `CT3`) **AND** `i6_violations == 0` (`I6`) **AND** `i7_violations == 0` (`I7`) | `Clean`: $P_{\text{target}} = 1.0$ (`exact_mode`); `Noisy`: $P_{\text{target}} = 1.0$ (`exact_mode`) | Structural invariants; `20..220` empirical $= 100.0\%$ (`3,600/3,600` Clean, `3,600/3,600` Noisy). |
| **`G4A.3` `H5` Spontaneous Falsification Superiority** | **Gated (`Noisy`, $M=3$ positive-opportunity `SP` tasks):** `SP:Period6_LagRedundancy:k2:noisy`, `SP:MV_Periodic:k1:noisy`, `SP:MV_Periodic:k2:noisy`. *(Other 13 `SP` tasks have M4a-0 median opportunity $\le 0$ per `K2` and are reported as characterisation + gated in `G4A.1/2/4/6`; general Clean `SP` compression is scoped to `M4b` `H5+H7`.)* | Paired `eval_l_epi_advantage_bits = eval_l_epistemic(wake) - eval_l_epistemic(sleep) > 0.0` bits ($m = 0.0$) **AND** `wrongful_excisions == 0` **AND** `port_locks_lost == 0` | `Noisy` ($M=3$): $P_{\text{target}} = 0.55$, `enforce_pooled_cp = true`, per-task FWER $\alpha = (0.01 / 3) / 2 = 0.001667$ (Bonferroni `/ 2` across `S-A`/`S-B`; passes if either arm passes) **AND** task `mean_l_epi_advantage >= +15.0` bits & `median_l_epi_advantage > 0.0` bits on each gated task | `20..220` empirical `frac > 0` (and mean advantage): `Period6:k2:noisy` `0.95` (`+54.5`b `S-A`), `0.96` (`+54.6`b `S-B`); `MV_Periodic:k1:noisy` `0.935` (`+139.3`b `S-A`), `0.95` (`+141.8`b `S-B`); `MV_Periodic:k2:noisy` `0.735` (`+88.2`b `S-A`), `0.73` (`+88.2`b `S-B`); pooled $\approx 87.3\%$. At $N=100$, $\text{Bin}(100, 0.55) \le k$ passes $\alpha = 0.001667$ for $k \ge 40$ (measured $k \approx 73..95$). |
| **`G4A.4` Boundary KT Non-Inferiority** | `Clean` ($M=14$: all 8 `SP` + 6 `CT` tasks) & `Noisy` ($M=14$: all 8 `SP` + 6 `CT` tasks) on frozen post-episode $W_{\text{obs}} = 256$ continuation window | `Clean`: `eval_kt_regret <= 0.05` b/sym ($\delta_{\text{clean}} = 0.05$); `Noisy`: `eval_kt_regret <= 0.35` b/sym ($\delta_{\text{noisy}} = 0.35$) | `Clean` ($M=14$): $P_{\text{target}} = 1.0$ (`exact_mode`); `Noisy` ($M=14$): $P_{\text{target}} = 0.85$, `enforce_pooled_cp = true`, FWER $\alpha = 0.01 / 14 = 0.000714$ **AND** task `mean_eval_kt_regret <= 0.10` b/sym & `p95_eval_kt_regret <= 0.35` b/sym | `20..220` Clean: `100.0%` (`2,800/2,800`, max regret $= 0.000$). `20..220` Noisy: finite-window ($W_{\text{obs}}=256, \varepsilon=0.05$) binomial spread has task means $-0.013..+0.020$ (`S-A`) / $+0.033$ (`S-B`) and $\text{p95} \le 0.221$ (`S-A`) / $0.243$ (`S-B`), giving $\ge 96\%$ per-task pass rate vs $0.85$ target. |
| **`G4A.5` Planted Spurious-Cycle Rejection** | `Clean` ($M=4$: `PL1:k1/k2:clean`, `PL2:k1/k2:clean`) & `Noisy` ($M=4$: `PL1:k1/k2:noisy`, `PL2:k1/k2:noisy`) | Planted spurious derivation excised (`planted_excised`) or spurious cycle extinguished within $\le 4$ sleep segments (`spurious_survival_segments <= 4`) **AND** `wrongful_excisions == 0` | `Clean` ($M=4$): $P_{\text{target}} = 1.0$ (`exact_mode`), `max_spurious_survival_segments <= 1`; `Noisy` ($M=4$): $P_{\text{target}} = 0.75$, `enforce_pooled_cp = true`, FWER $\alpha = (0.01 / 4) / 2 = 0.00125$ (passes if canonical arm `S-A` or either arm passes) | `20..220` Clean: `100.0%` (`800/800`, max survival $= 0$ segs). `20..220` Noisy (`S-A`): `PL1:k1 = 1.00`, `PL1:k2 = 1.00`, `PL2:k1 = 0.985`, `PL2:k2 = 0.880` (pooled $= 96.6\%$; at $N=100$, $\text{Bin}(100, 0.75)$ passes $\alpha=0.00125$ for $k \ge 61$ vs measured $k \approx 88..100$). |
| **`G4A.6` Fact-Supported Preservation** | `Clean` ($M=14$ `SP`+`CT`, or all 18 Clean tasks) & `Noisy` ($M=14$ `SP`+`CT`, or all 18 Noisy tasks) | `wrongful_excisions == 0 && port_locks_lost == 0` (plus hard stratum invariant `max_port_locks_lost == 0` across **100%** of seeds in both `Clean` and `Noisy`) | `Clean`: $P_{\text{target}} = 1.0$ (`exact_mode`, 0 WE & 0 locks lost); `Noisy`: $P_{\text{target}} = 0.90$, `enforce_pooled_cp = true`, FWER $\alpha = 0.01 / M_{\text{noisy}}$ **AND** hard `max_port_locks_lost == 0` across 100% of seeds | `20..220` Clean: `100.0%` (`0` WE, `0` locks lost). `20..220` Noisy: `0` locks lost (`100.0%`); zero-WE run rate $= 99.81\%$ in `S-A` (worst task `98.5%`) and $99.78\%$ in `S-B` (`97.5%` worst task), all well above $0.90$. |

- **`G4A.7` (`H6` Pre-Registered Decision Rule — `S-A` vs `S-B`):**
  - Audit and report `StratifiedGateAudit` for **both** `S-A` and `S-B`; `G4A.1` and `G4A.2` must pass on both arms, while `G4A.3`–`G4A.6` overall gate pass requires the canonical winning arm `S-A` (or either arm under Bonferroni `/ 2` where applicable) to pass all strata.
  - Compare `S-A` and `S-B` via mean paired difference $\Delta_{6} = \overline{L_{\text{eval}}^{\text{epistemic}}(\text{S-B}) - L_{\text{eval}}^{\text{epistemic}}(\text{S-A})}$ across the active falsification tasks (`G4A.3` gated `SP` + `G4A.5` `PL` tasks) with equivalence margin $\delta_6 = 10.0$ bits:
    - If $\Delta_6 > +10.0$ bits (`S-A` superior, as observed in `20..220` where `S-A` leads `S-B` by `+244.87` bits on `PL2:k2:noisy` and ties on `Period6:k2:noisy` and `PL1`, mean $\Delta_6 = +23.69$ bits) OR $|\Delta_6| \le 10.0$ bits (`S-A` equivalent) while `S-A` passes `G4A.1`–`G4A.6`: **Confirm `H6`, select `S-A` (`SleepSchedule::ConsistencyOnly`) as the canonical sleep arm for `M4b` consolidation, and drop `S-B` replay in `M4b`** (Charter §4.2).
    - If $\Delta_6 < -10.0$ bits (`S-B` superior): retain `S-B` for `M4b`.

### 3.2 Option D Per-Task Tail Metrics (`tail_metrics`)

Every `TaskBankInput` in `verify-m4a` populates finite Option D `tail_metrics` (`update_max_finite` / `update_min_finite`), and `verify-m4a` enforces:

| Metric key | Gate | Pre-registered Bound / Disposition |
| :--- | :--- | :--- |
| `max_epistemic_mdl_err` | `G4A.1` | `<= 1e-9` |
| `max_compute_ledger_diff` | `G4A.1` | `== 0.0` |
| `max_node_eval_ratio_sleep_over_wake` | `G4A.1` | `<= 1.0` |
| `mean_l_epi_advantage`, `median_l_epi_advantage`, `min_l_epi_advantage`, `p05_l_epi_advantage` | `G4A.3` | `mean >= 15.0` and `median > 0.0` on the `3` gated `G4A.3` tasks; `min` and `p05` finite and reported on all `SP` tasks |
| `max_eval_kt_regret`, `mean_eval_kt_regret`, `p95_eval_kt_regret` | `G4A.4` | `Clean`: `max <= 0.05`; `Noisy`: `mean <= 0.10` and `p95 <= 0.35` |
| `max_spurious_survival_segments`, `median_spurious_survival_segments` | `G4A.5` | `Clean`: `max <= 1.0`; `Noisy`: `median <= 2.0` |
| `max_port_locks_lost` | `G4A.6` | `== 0.0` (`Clean` AND `Noisy`) |
| `max_wrongful_excisions` | `G4A.6` | `== 0.0` (`Clean`); finite and reported in `Noisy` |
| `sprout_blocked_max_nodes` | All | `== 0.0` (any `> 0` marks `CaseOutcome::Excluded`; `excluded_cases == 0` required) |
| `sprout_budget_exhausted`, `max_suspended_comparator_ticks`, `max_t_onset`, `max_facts_forgone` | Diagnostic | Finite and reported |

## 4. Verification Protocol & 100-Seed Confirmation / Holdout Ledger

- **Smoke (`M4a-0` and `M4a`):** `0..20` (`1 × 20 = 20` seeds, `~55s`).
- **M4a-0 characterisation (completed & frozen):** `20..220` (`10 × 20 = 200` seeds, `550.98s = 9.18 min`). Pre-registered thresholds in §3 were derived solely from this range.
- **M4a confirmatory development sweep:** `520..720` (`10 × 20 = 200` seeds, strictly disjoint from characterisation `20..220`, $2\times$ the 100-seed confirmation/holdout size, `~9.2 min <= 15 min` watchdog limit).
- **Confirmation (`S3` precondition):** `20200..=20299` (`5 × 20 = 100` seeds, `~4.6 min`). Untouched during `M4a-0`; `explore-m4a` rejects it.
- **Single-shot holdout (`S3`-gated, `S4` precondition):** `60000..=60099` (`5 × 20 = 100` seeds, `272.63s`), executed once at `0f62e2f` under `S3` `#65` (`overall_passed = true`, `excluded_cases = 0`).
  - Permanently closed unconditionally (`allow_with_flag: false`) in every command (`HOLDOUT_BANKS`, `VERIFY_M4A_HOLDOUT_BANKS`, `explore-m4a`) in the Phase 3 closeout commit, rejected even when `--holdout` / `--allow-holdout` is passed.
- **Statistics:** [Charter §2.8](foundational_research_charter_v3.md#28-statistical-policy-holdout-governance-and-retained-baseline-regression-gate)
  Options A, B and D, applied to paired trajectory units, with `excluded_cases == 0`.

| Check | Command | Required outcome |
| :--- | :--- | :--- |
| Retained baselines (before `S3` and after Phase 3 closeout commit) | `python3 scripts/check_retained_baselines.py --require-clean` | `passed: true`, `git_dirty: false`, `golden_traces` `1/1` |
| Exploration (`M4a-0`, completed) | `cargo run --locked --release -- explore-m4a --seed-offset 20 --seeds 200` | Report only; no gates (`550.98s`, §2) |
| Confirmatory Dev Sweep (Phase 2, passed `#64`) | `cargo run --locked --release -- verify-m4a --banks 10 --seed-offset 520 --seeds 20` | All gates pass in both strata, `excluded_cases == 0`, tails inspected |
| Confirmation (`S3` precondition, Phase 2, passed `#64`) | `cargo run --locked --release -- verify-m4a --banks 5 --seed-offset 20200 --seeds 20` | All gates pass in both strata, `excluded_cases == 0`, tails inspected |
| Holdout (`S4` precondition, Phase 3, passed `#67`) | `./target/release/arkhe verify-m4a --banks 5 --seed-offset 60000 --seeds 20 --holdout` at `0f62e2f` | Same as confirmation, single shot (now permanently closed `allow_with_flag: false`) |

`docs/retained_baselines.json` gains an `m4a` entry (`verify-m4a` `G4A.1`–`G4A.7`) in the Phase 3 closeout commit alongside `scripts/check_retained_baselines.py` validation support.

## 5. Kill & Revisit Criteria

- **`K1` (Charter §7):** If `G4A.3` fails for both arms at confirmation, neither arm beats
  compute-matched wake-only. Revisit sleep before consolidation. This is a result to report, not to
  engineer around.
- **`K2` (M4a-0 opportunity scoping — triggered & resolved in [DR #50](foundational_research_charter_v3.md#appendix-a-decision-record)):** Under cumulative MDL, unequal wake fact counts (`3,840` forgone facts) made the opportunity bound $\le 0$ on all `16/16` `SP` tasks; even under equal-dataset `eval_l_epistemic_bits`, all `8/8` `Clean` `SP` tasks have `p_onset_incorrect = 0.0` (zero falsification-only opportunity). Per `DR #50`, `M4a` `G4A.3` is scoped to the `M=3` positive-opportunity noisy `SP` tasks (`Period6_LagRedundancy:k2:noisy`, `MV_Periodic:k1:noisy`, `MV_Periodic:k2:noisy`) alongside `G4A.5` (`PL1`/`PL2`), while general spontaneous `H5` compression superiority on already-correct circuits (`Clean` `SP`) is scoped to `M4b` (`H5 + H7` e-graph consolidation).
- **`K3` (wrongful excision):** If `G4A.6` fails on development data, revisit `R4` and `R5`. Sleep that
  destroys fact-supported structure violates the asymmetry principle.
- **Guardrails:** any guardrail binding (`SproutBlockedMaxNodes > 0`) is an exclusion and an escalation, never a pass. Masking a
  guardrail counter is forbidden (Case C).

## 6. Parameter Inventory Summary (`M4a-0` / `DR #50`, catalogued in [Parameter Inventory](parameter_inventory.md))

| Parameter / Law | Value / law | Class | Basis | Telemetry | Backward reduction |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `SleepSchedule` | `None` (default), `ConsistencyOnly` (`S-A`), `ConsistencyReplay` (`S-B`) | Configuration | §4.2 arms (`R1`) | `sleep_segments_executed`, `sleep_segment_skipped_no_scoreable_comparator` | `None` gives bit-identical behaviour (`I1`) |
| $W_{\text{wake}}$, $W_{\text{sleep}}$ | $W_{\text{obs}}$, $W_{\text{obs}}$ (`256` at defaults) | Derived | `R7` (`derived_wake_sleep_windows`) | Segment counters, `facts_forgone` | Not active under `None` |
| `fact_buffer_capacity` | $W_{\text{obs}}/2$ (`128` at defaults) | Derived | `R6` balanced two-sample design | `FactBufferEvictions` (`fact_buffer_evictions`) | Recorded but never read under `None` |
| `R3` prediction-consistent entry (`consistent_len`) | $d_{\text{cone}} + 1$ (reactive) or $d_{\text{closed\_loop}} + 1$ (cross-channel / lone 2-route) | Derived (zero-constant) | `R3` (`DR #50`) | `live_step_node_evals`, `sleep_comparator_suspended_ungrounded` | Exact `I3` wake snapshot restore |
| `R4` transitive closure & cross-port homogeneity | `(full_closure & !grounded_mask) == 0` ($\hat{q}_c > p_{\text{null},c}$), two-sided zero-bit KT homogeneity guard, `S-B` loopback suspension | Derived (reuse + zero-bit KT) | `R4` (`DR #49`–`#50`) | `SleepComparatorSuspendedUngrounded`, `max_suspended_comparator_ticks` | `I7` zero flux & zero excision |
| `R5(a)` solvency, `alt` exoneration & reference immunity | Primary excision requires `buffer_net_bits_primary <= 0 && buffer_net_bits_alt > 0`; `alt` exoneration suspension; `r5a_primary_at_fault` immunity | Derived (zero-bit MDL) | `R5(a)` (`DR #50`) | `sleep_excisions`, `sleep_starvation_excisions` | Ties default to DR #40 |
| `R5(b)` `S-B` match test | Two-sample KT code length at order $d_{\text{cone}}$; decision at 0 bits | Derived (zero-parameter) | `R5(b)` (`DR #49`) | `SleepMatchFailures` (`sleep_match_failures`) | Only active in `S-B` |
| `R5(c)` Popperian loopback refutation | Escrows positive sleep agreement flux and voids it when `segment_loopback_disagreements > 0`; `S-B` `Step 3` replay debit & `sleep_port_consensus` | Derived (Popperian asymmetry) | `R5(c)` (`DR #50`) | `sleep_excisions`, `I4` audit trace | `I4` exact conservation |
| `I6` sleep pause | `pause_juvenile_countdown` & frozen `smoothed_drain_rate` during `run_sleep_segment` | Derived (`I6` corollary) | `I6` (`DR #50`) | `i6_violations` | Wake unchanged |
| Compute ledger & paired wake envelope | Substrate ticks + paired wake `max_episode_node_evals` cap & zero-flux sleep sensitivity skip | Pre-registered accounting & Derived cap | `R2` (`DR #49`–`#50`) | `ComputeLedger` | `None` cap leaves wake untouched (`I1`) |
| Evaluation window | $W_{\text{obs}} = 256$ fresh symbols, frozen structure | Pre-registered harness | `G4A.4` (`DR #50`) | `eval_kt_regret`, `eval_l_epistemic_bits` | — |

## 7. Phased Implementation Plan

Each phase is one commit. `golden_traces` must stay bit-identical in every M4a-0 phase (`I1`).

| Phase | Scope | Gate / evidence |
| :---: | :--- | :--- |
| `0.1` | `SleepSchedule` and scheduler; compute ledger and node-evaluation audit; live-state entry and remapped snapshot restore (`R2`, `R3`, `R7`) | **Completed (`d053aea`).** `I1`, `I2`, `I3`, `I5` tests |
| `0.2` | Grounded-loop rule (`R4`); fact buffer (`R6`); `S-B` replay, fact adjudication and match test (`R5`) | **Completed (`7687172`).** `I4`, `I6`, `I7` tests; `G2B.3` unchanged |
| `0.3` | Oracle instrumentation: forbidden words, attractor detection, oracle-correct labels, wrongful-excision counter (`R8`); `PL` injections; `CT` controls | **Completed (`abeb8fe`).** 36-task registry & `tests/m4a_oracle.rs` |
| `0.4` / `0.4b` | `explore-m4a` harness (rejecting `20200..=20299` & `60000..=60099`); `DR #50` sleep containment, Popperian loopback refutation, & compute parity; `0..20` smoke & `20..220` characterisation | **Completed (`2bb81c0`, `089ed72`, `#51`).** Report in §2 (`cargo test --locked` `242/242`) |
| `0.5` | **Chief Scientist review.** Decide `K2`; finalise `R2` currency; write §3 thresholds with Rule-9 arguments; update inventory & Charter `DR #50` | **Signed off (`#54`, DR #50).** This revision |
| `1` | `verify-m4a` with `StratifiedGateAudit`, paired units, Option D `tail_metrics`, `S3`-gated holdout guard (`60000..=60099` blocked without `--holdout`) | **Completed (`0f62e2f`).** Smoke `0..20` ([`verify_m4a_dev20.json`](evidence/m4a/verify_m4a_dev20.json)), CLI guard & unit tests |
| `2` | Dev sweep `520..720`; confirmation `20200..=20299`; clean-commit `check_retained_baselines.py --require-clean` | **Completed (`#64`, `0f62e2f`).** `S3` authorized (`#65`); evidence [`verify_m4a_dev200.json`](evidence/m4a/verify_m4a_dev200.json), [`verify_m4a_conf100.json`](evidence/m4a/verify_m4a_conf100.json), [`retained_baselines_pre_s3.json`](evidence/m4a/retained_baselines_pre_s3.json) |
| `3` | `S3`, then single-shot holdout `60000..=60099`, attainment record, closeout (`allow_with_flag: false`), `S4` | **Holdout PASSED (`#67`, `0f62e2f`, [`verify_m4a_holdout_60000.json`](evidence/m4a/verify_m4a_holdout_60000.json)); `60000..=60099` sealed `allow_with_flag: false` & `m4a` added to `retained_baselines.json`; `S4` pending `@chief-scientist` sign-off on closeout commit.** |

## 8. Sign-Off & Attainment Record

- **M4a-0 authorization:** Chief Scientist, 2026-11-24 ([DR #49](foundational_research_charter_v3.md#appendix-a-decision-record)).
- **M4a-0 completion:** Commits `d053aea`, `7687172`, `abeb8fe`, `2bb81c0`, `089ed72` (`@verifier`-approved `#51`; `cargo test --locked` `242/242`, `golden_traces` `1/1`).
- **Pre-registration (`M4a` Phase 0.5):** Signed off by Chief Scientist, 2026-11-25 ([DR #50](foundational_research_charter_v3.md#appendix-a-decision-record), `#54` at commit `089ed72`); Phases 1–2 (`verify-m4a`, `520..720` dev, `20200..=20299` confirmation) authorized.
- **Phase 1 & Phase 2 (`0f62e2fbc4cdf9babb1e538d867ebc3e59f7c277`, `git_dirty = false`, `source_tree_fnv1a128 = 349fe16d144f4fcd2c2184fe944d6c51`, `cargo_lock_fnv1a128 = 8d29cd24b1fbdbb743cf5c0b3933b22d`):**
  - Pre-S3 retained baselines ([`retained_baselines_pre_s3.json`](evidence/m4a/retained_baselines_pre_s3.json)): `passed = true`, `git_dirty = false`, `golden_traces` `1/1`, `m2a` (`5/5` gates, `excluded_cases = 0`, `sprout_blocked_max_nodes = 0`, worst `G2A.5:CrossSum(d=1,2,k=2)` `0.9800` / `G2A.4b:AllNoise:CrossAdd(d=1,2,k=1)` `0.9900`), `m2b` (`5/5` gates, `excluded_cases = 0`, `sprout_blocked_max_nodes = 0`, `G2B.2` `1.0000`, worst `G2B.4:MV_AllChNoise_k2` `0.8900`).
  - Smoke `0..20` ([`verify_m4a_dev20.json`](evidence/m4a/verify_m4a_dev20.json), pre-commit `4370540` `git_dirty = true` with identical `source_tree_fnv1a128 = 349fe16d144f4fcd2c2184fe944d6c51`, `55.02s`): `overall_passed = true`, `excluded_cases = 0`, `G4A.7` `SaSuperiorDropReplay` `delta_6 = +19.23` bits (`S-A` passed all gates).
  - Disjoint 200-seed Dev `520..720` ([`verify_m4a_dev200.json`](evidence/m4a/verify_m4a_dev200.json), `10 × 20`, `546.92s`, clean `0f62e2f`): `overall_passed = true`, `excluded_cases = 0`, `sprout_blocked_max_nodes = 0`, `sprout_budget_exhausted = 178360`. `G4A.1` `7200/7200` (`Clean conj=200/200`, `max_mdl = 1.310e-13`, `max_diff = 0.0`, `max_ratio = 1.000000`), `G4A.2` `7200/7200` (`Clean conj=200/200`, `max_i2/i6/i7 = 0.0`), `G4A.3` `S-A 526/600` (`cp99 = 0.842083`, `hom_p = 0.557584`, worst `MV_Periodic:k2:noisy` `158/200 = 0.7900`; `S-B 525/600`), `G4A.4` `S-A 5599/5600` (`Clean 2800/2800 conj=200/200`, `Noisy 2799/2800`, `max_mean_reg = 0.0018`, `max_p95_reg = 0.1907`, `max_reg = 0.4049`), `G4A.5` `S-A 1580/1600` (`Clean 800/800 conj=200/200`, `Noisy 780/800`, worst `PL2:k2:noisy` `180/200 = 0.9000`, `med_surv = 0.0`; `S-B` failed `Noisy PL2:k2` `111/200 = 0.5550`), `G4A.6` `S-A 7195/7200` (`Clean 3600/3600 conj=200/200`, `Noisy 3595/3600`, `max_port_locks_lost = 0.0`, `max_we = 2.0`), `G4A.7` (`H6`) `SaSuperiorDropReplay` `delta_6 = +27.65` bits (`2200` obs).
  - 100-seed Confirmation `20200..=20299` ([`verify_m4a_conf100.json`](evidence/m4a/verify_m4a_conf100.json), `5 × 20`, `273.93s`, clean `0f62e2f`): `overall_passed = true`, `excluded_cases = 0`, `sprout_blocked_max_nodes = 0`, `sprout_budget_exhausted = 88320`. `G4A.1` `3600/3600` (`Clean conj=100/100`), `G4A.2` `3600/3600`, `G4A.3` `S-A 258/300` (`cp99 = 0.806941`, worst `MV_Periodic:k2:noisy` `77/100 = 0.7700`; `S-B 258/300`), `G4A.4` `S-A 2799/2800` (`Clean 1400/1400 conj=100/100`, `Noisy 1399/1400`, `max_mean_reg = 0.0179`, `max_p95_reg = 0.2354`), `G4A.5` `S-A 786/800` (`Clean 400/400 conj=100/100`, `Noisy 386/400`, worst `PL2:k2:noisy` `86/100 = 0.8600`; `S-B` failed `Noisy PL2:k2` `60/100 = 0.6000`), `G4A.6` `S-A 3598/3600` (`Clean 1800/1800 conj=100/100`, `Noisy 1798/1800`, `max_port_locks_lost = 0.0`), `G4A.7` (`H6`) `SaSuperiorDropReplay` `delta_6 = +21.03` bits (`1100` obs).
- **`S3` Single-Shot Holdout (`60000..=60099`, `5 × 20 = 100` seeds, `272.63s`, clean `0f62e2f`, [`verify_m4a_holdout_60000.json`](evidence/m4a/verify_m4a_holdout_60000.json)):**
  - Authorized by `@chief-scientist` (`S3` `#65`) and passed cleanly (`#67`): `overall_passed = true`, `excluded_cases = 0`, `sprout_blocked_max_nodes = 0`, `sprout_budget_exhausted = 83072`.
  - **`G4A.1`:** `3600/3600` (`Clean 1800/1800 conj=100/100 max_mdl = 1.310e-13`, `Noisy 1800/1800 max_mdl = 1.173e-13`, `max_compute_ledger_diff = 0.0`, `max_node_eval_ratio = 1.000000`).
  - **`G4A.2`:** `3600/3600` (`Clean 1800/1800 conj=100/100`, `Noisy 1800/1800`, `max_i2/i6/i7 = 0.0`).
  - **`G4A.3` (`H5`):** `S-A` `264/300 = 0.8800` (`cp99 = 0.829570`, `hom_p = 0.337192`, `min_binom_p = 1.000000`, `min_task_hom_p = 0.190650`, worst `SP:MV_Periodic:k2:noisy` `79/100 = 0.7900`; per-task mean/med `MV_Periodic:k1` `+117.32 / +130.47`, `MV_Periodic:k2` `+104.14 / +93.38`, `Period6_LagRedundancy:k2` `+60.65 / +58.58` bits); `S-B` `265/300 = 0.8833`.
  - **`G4A.4`:** `S-A` `2799/2800` (`Clean 1400/1400 conj=100/100 max_reg = 0.0000`, `Noisy 1399/1400 cp99 = 0.995268`, worst `MV_Periodic:k2:noisy` `99/100 = 0.9900`, `max_mean_reg = 0.0150`, `max_p95_reg = 0.2031`, `max_reg = 0.4048`); `S-B` `2796/2800` (`max_reg = 1.4249`).
  - **`G4A.5` (`PL`):** `S-A` `785/800` (`Clean 400/400 conj=100/100 max_surv = 0.0`, `Noisy 385/400 cp99 = 0.934123`, `hom_p = 0.807280`, `min_binom_p = 0.994579`, `min_task_hom_p = 0.770330`, `PL1:k1/k2` `1.00/1.00`, `PL2:k1` `1.00`, `PL2:k2` `85/100 = 0.8500` `med_surv = 0.0`, `max_surv = 4.0`); `S-B` `766/800` (`PL2:k2:noisy` `66/100 = 0.6600`, `med_surv = 2.0`).
  - **`G4A.6`:** `S-A` `3599/3600` (`Clean 1800/1800 conj=100/100`, `Noisy 1799/1800 cp99 = 0.996318`, worst `MV_Periodic:k1:noisy` `99/100 = 0.9900`, `max_port_locks_lost = 0.0`, `max_we = 1.0`); `S-B` `3599/3600`.
  - **`G4A.7` (`H6`):** `SaSuperiorDropReplay`, `delta_6 = +14.61` bits (`11` tasks, `1100` obs, `canonical_arm = Sa`, `canonical_arm_passed_all_gates = true`) -> **`S-A` selected as canonical sleep arm for `M4b` and `S-B` replay dropped in `M4b`**.
- **Phase 3 Closeout & `S4` Status:** Holdout `60000..=60099` permanently closed (`allow_with_flag: false` in `src/stats.rs`, `src/verify_m4a/verify.rs`, and `src/lib.rs`, enforced by `tests/cli.rs` even with `--holdout` / `--allow-holdout`); `m4a` added to `docs/retained_baselines.json` and `scripts/check_retained_baselines.py`; evidence archived in [`docs/evidence/m4a/`](evidence/m4a/). **`S4` sign-off is pending `@chief-scientist` review of the closeout commit and clean-commit `check_retained_baselines.py --require-clean` output.**
- **Source digest and independent review:** `0f62e2f` (`source_tree_fnv1a128 = 349fe16d144f4fcd2c2184fe944d6c51`, audited by `@verifier` `#64`, `#67`); Phase 3 closeout modifies only holdout-seal flag/status note (`src/stats.rs`, `src/verify_m4a/verify.rs`, `src/lib.rs`, `tests/cli.rs`), retained-baseline suite validation (`docs/retained_baselines.json`, `scripts/check_retained_baselines.py`), archived JSON evidence, and closeout docs with zero algorithmic, substrate, metric, or gate changes.
