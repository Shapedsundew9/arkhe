# Arkhe Foundational Research Record

> **Companion to:** [Foundational Research Charter v3](foundational_research_charter_v3.md)
> **Scope:** Persistent historical ledger of empirical measurements, verification provenance, kill-criterion diagnoses, discovered mathematical traps, rejected design alternatives, parameter origins, and deferred Milestone 2–5 questions recovered from git history (`FOUNDATION` through `m1-final-lattice`, pre-M2 archival of `milestone_1b_plan.md`, `milestone_1b_review.md`, `coordinate_free_scaling_plan.md`, and `pre_milestone_2_readiness_plan.md`, and post-M2a archival of `milestone_2a_plan.md`).
> **Policy:** Historical planning and review drafts were periodically deleted before milestone tags (`b96fb8d`, `b541136`, `e9d38f6`, `ebf9b3d`, `ddd9056`, and pre-M2 cleanup). Going forward, **preserve durable results here rather than discarding them**, so future agents do not repeat falsified designs or guess ungrounded constants.

---

## 1. Document & Milestone Lineage

### 1.1 Milestone Commits & Pruned Documentation History

| Commit / Tag | Date | Event & Recovered Documents |
| :--- | :--- | :--- |
| `614572b` (`tag: FOUNDATION`) | 2026-11-01 | Baseline repository with Charter v1 and review debates. |
| `b96fb8d` | 2026-11-01 | Deleted `doc/foundational_research_charter.md`, `doc/foundational_research_charter_critique.md`, `doc/counter_critique.md`, and `doc/counterpoint_response.md` after adopting Charter v2. |
| `00eb700` (`tag: MILESTONE-0`) | 2026-11-01 | Milestone 0 complete; recorded closeout amendment (`G3 -> G3-rev1`) and M0 lessons. |
| `b541136` | 2026-11-02 | Deleted `doc/milestone_0_plan.md`, `doc/milestone_0_closeout_plan.md`, and `doc/milestone_0_lessons_learned.md`. |
| `9f5aeb7` | 2026-11-06 | Recorded pre-redesign M1a kill-criterion failure (`doc/m1a_gate_findings.md`). |
| `bcc2154`–`93f6029` | 2026-11-07 | Implemented M1a redesign (`D1`–`D4`) across six step commits (`step 0`–`step 5`). |
| `bc542fd` | 2026-11-07 | Renamed `doc/foundational_research_charter_v2.md` to [foundational_research_charter_v3.md](foundational_research_charter_v3.md) (adding Appendix A decision rows 25–28). |
| `e9d38f6` | 2026-11-07 | Deleted `doc/ideas.md`, `doc/m1a-revisited-proposal.md`, `doc/m1a-revisited-review.md`, `doc/m1a_gate_findings.md`, and `doc/milestone_0_review.md`. |
| `c6a49be` | 2026-11-07 | Recorded post-M1a interface decisions & open questions for M1b in `doc/milestone_1_plan.md` §4.0. |
| `ebf9b3d` | 2026-11-07 | Deleted `doc/m1a-redesign-plan.md`. |
| `c475982` (`tag: MILESTONE_1A_COMPLETE`) | 2026-11-07 | Stage M1a closeout & redesign lock. |
| `e29014a` / `6292920` | 2026-11-07 | Executed single-shot Milestone 1 holdout sign-off (`10000..=10019`) and recorded attainment in `doc/milestone_1_plan.md`. |
| `9544e98` | 2026-11-08 | Post-holdout M1b fix (`src/structural/growth.rs`, `src/structural/learner.rs`, changing `source_tree_fnv1a128` to `ae9e3c4d577d071163c1e0855f3342a0`); `2,000`-seed 100-bank hardening recorded `2026-11-08` (Section 2.5 below). |
| `2ed11fd` | 2026-11-08 | Added `doc/coordinate_free_scaling_plan.md`. |
| `ddd9056` (`origin/main`) | 2026-11-08 | Deleted `doc/milestone_1_plan.md` and `doc/milestone_1b_plan.md`; key schemas, sign-offs, and M1b findings were consolidated into Sections 2.5, 3.3, and 6.1 below. |
| `2d009b0` (`tag: m1-final-lattice`) | 2026-11-08 | Phase 0 anchor: golden-trace harness, cliff telemetry, and regime-shift baseline before coordinate-free scaling refactor (also final git anchor for retired spatial ablations and `doc/milestone_1b_review.md`). |
| `deb200a`–`eafcec6` | 2026-11-13 | Completed coordinate-free Phases 1–4 refactor (`A1` golden proof), governance rules & Charter DR #31–#39 (`5a7c27d`), Readiness `B1`–`B7` (`a47ff8d`, `22954df`, `7649e2c`), and [parameter_inventory.md](parameter_inventory.md) (`eafcec6`). |
| Pre-M2 plan retirement | 2026-11-13 | Folded durable M1b and Pre-M2 derivations/verification (`milestone_1b_plan.md`, `milestone_1b_review.md`, `coordinate_free_scaling_plan.md`, `pre_milestone_2_readiness_plan.md`) into [parameter_inventory.md](parameter_inventory.md) and Sections 2.5, 3.3, 6.2, and 7 below, then retired those completed plan files from `doc/`. |
| `691ad6f`–`a71d6c7` | 2026-11-14 | Implemented Milestone 2a Phases 1–4 (`691ad6f`, `e9fc185`, `5886135`, `9fc8689`, `918cb02`, `aa6fb6d`), executed single-shot M2a holdout sign-off (`30000..=30019`, `20/20` on `G2A.1`–`G2A.5`, `a71d6c7`), and landed behavior-preserving `Perf(...)` optimizations (`5f0391a`–`567f0a4`). |
| Post-M2a plan retirement | 2026-11-14 | Consolidated all durable `milestone_2a_plan.md` exploratory findings, pre-registered gate definitions (`G2A.1`–`G2A.5`), baseline metering rules (`S4-A.1`, `S4-A.2`), and derived scaling laws into [parameter_inventory.md](parameter_inventory.md) and Sections 8–9 below, retired `doc/milestone_2a_plan.md`, and ratified Stage M2b comparator economics (`DR #40`–`#43`, Section 10 & retired `docs/milestone_2b_plan.md`). |
| `8ca68a2`–`ffb4ca5` | 2026-11-14–15 | Implemented Milestone 2b Phases 1–4 (`8ca68a2` epistemic MDL/comparator economy/multi-view `T2`; `85d736e` Phase 2 redundancy sprouting, loopback, and Rule-4 `K=1 Comb` fixture re-bless; `498365a` Phase 3 telemetry & integration tests; `6658e10` `M2B-A1`/`A3` amendments; `d86cafd` `M2B-B1`/`B2`/`C1` two-clause causal-dilution decidability law & 1-sided bankruptcy refutation; `b211afa` `verify-m2b` suite; `ffb4ca5` single-shot holdout `40000..=40019` sign-off, `source_tree_fnv1a128 = 1e1c9dd8a7151060a011d956081fad50`). |
| Post-M2b cleanup (Pre-M3) | 2026-11-20 | Retired completed M2b working docs (`docs/milestone_2b_plan.md`, `docs/milestone_2b_chief_scientist_review.md`, `docs/milestone_2b_stage3_blocker_report.md`, `scratchpad/`), retired M0/M1/profiler harnesses (`src/verify.rs`, `src/verify_m1/{mod,m1b,panel,profile}.rs`, `scripts/measure_compute_profile.py`, `tests/m0_acceptance.rs`, `tests/m1a_economy.rs`, `tests/m1b_neurogenesis.rs`), promoted reusable multi-bank stats to [`src/stats.rs`](../src/stats.rs), and retained `verify-m2a` and `verify-m2b` as scientific regression gates for M3. |
| `9b5b950`–`dd13839` | 2026-11-20–22 | Implemented Milestone 3 (`H4`) Phases 1–4 (`9b5b950` `M3-E1`/`E2`/`E3` + `D0`–`D7b` detours; `e498424` `explore-m3`; `235a292` `verify-m3` + M4a holdout guards + `#13`/`#15` `stage2_exhausted` invariants + bit-identical speedups; `004aaa5` `excise_port_comparator` `#35`; `a24ea1a` `Option 4B`, independent-unit `G3.2`/`G3.3`, and `G3.1`/`G3.5b` fixes; `dd13839` Phase 4 doc promotion), passed all dev (`0..20`, `20..1020`) and confirmation (`20060..=20079`) sweeps plus single-shot holdout (`50000..=50019`, `S3`/`S4` approved, `source_tree_fnv1a128 = 4e5ff20e46d6d87c8716d21cec458b8d`, `excluded_cases = 0`) ([Section 12](#12-milestone-3-h4-implementation--verification-record-2026-11-22)); later superseded by `M3 v2` remediation after the retired M3 review identified a `G2B.2` regression. |
| `0c6112a`–`2b62140` + `S4` closeout | 2026-11-22 | Completed **Milestone 3 (`M3 v2`) remediation**: fixed `G2B.2` acyclic-register priming & `G3.4` comparator refutation/tap-`Split` accounting, adopted Charter §2.8 Statistical Policy Option A+B+D + `--require-clean` baseline gate, resolved re-review findings `R1`–`R5` + `R1 Residual` (`a826f02`, `2b62140`), passed clean-commit `check_retained_baselines.py --require-clean`, dev (`0..20`, `20..120`), 100-seed conf (`20100..=20199`), and `@chief-scientist` `S3` (`#32`)/`S4` (`#36`) single-shot 100-seed holdout (`50100..=50199`, `46,842/46,900`, `6/6` gates, `0` exclusions, `source_tree_fnv1a128 = bf22516980817549723269218e209d56`), permanently closed `50100..=50199` (`allow_with_flag: false`), and archived JSON evidence in `docs/evidence/m3_v2/` ([Section 12.5](#125-milestone-3-v2-remediation--statistical-policy-upgrade-pre-s3-evidence-packet)). |

### 1.2 Archival Consolidation of Superseded Milestone 1, Pre-M2, & Milestone 2a Plans

To prevent agents from confusing superseded blueprints (`milestone_1_plan.md`, `milestone_1b_plan.md`, `coordinate_free_scaling_plan.md`, `pre_milestone_2_readiness_plan.md`, `milestone_2a_plan.md`) or pre-refactor audits (`milestone_1b_review.md`, anchored against `9544e98` and deleted `src/structural/lattice.rs`) with active specifications, all durable specs, derivations, and measurements from those files live here (Sections 2.5, 3.2, 3.3, 6.1, 6.2, 7, 8, and 9) and in [parameter_inventory.md](parameter_inventory.md):

- **`milestone_1_plan.md` §4.0 (lines 334–352):** The seven post-M1a interface decisions and open questions (`idle nodes / switching vs pruning`, `juvenile_ticks & E_init`, `marginal ΔL_sprout`, `port M reset`, `max_energy_reserve`, `RepeatLast matched estimator`, `unowned circuit bits`). Reconstructed in Section 6.1.
- **`milestone_1_plan.md` §5.2 (lines 444–481):** The pre-registered M1 `ExperimentConfig` TOML block (`grid_rows = 8`, `grid_cols = 8`, `trace_discount = 0.90`, `initial_energy = 4.0`, `juvenile_ticks = 128`, `max_energy_reserve = 25.0`, `diagnostic_window = 64`, `sprout_interval = 32`, `mi_threshold_bits = 0.05`, `min_match_gain = 4`, `max_nodes = 32`, `horizon_ticks = 2_000`, `max_eval_ticks = 50_000`). Reproduced verbatim in Section 6.1.
- **`milestone_1b_plan.md` §9–§10 & `milestone_1b_review.md`:** Holdout (`e29014a`) and 100-bank hardening (`20..2020`) per-gate counts, DR #29/#30 `LocalVonNeumann` vs. `WellMixed` benchmark evidence, M1b defect diagnoses, and the `m1-final-lattice` archival anchor. Preserved in Sections 2.5 and 3.3.
- **`coordinate_free_scaling_plan.md` & `pre_milestone_2_readiness_plan.md`:** Parameter scaling derivations (`ReserveCap::HorizonDerived`, `max_nodes` guardrail, per-port `PortDevelopmentalState`, $W_{\text{obs}}$, M5 Miller–Madow scaling, M2 lag deepening, pooled Clopper–Pearson / $\chi^2$ gates) live in [parameter_inventory.md §2.1](parameter_inventory.md#21-parameter-scaling-derivations--deferred-rules); the `A1` Phase 1–2 golden-trace bit-identity proof, `B5`–`B7` / `A5` measurements, and `Q5` comparator-economics analysis (`DR #35`) live in Sections 6.2 and 7 below.
- **`milestone_2a_plan.md` §§2–8:** The six M2a mechanistic findings (`LFSR lag aliasing`, `per-port TickBudget fairness`, `C1-a` Epoch-1 cross-channel tie-break, `C2-a` ring-before-deepening order `(3..=6] -> (6..=12]`, `TicksPerHop` strict `edge.hops == 1` arm lag $d_{\text{arm}} = 1 + h_{\text{arm}} + 2 n_{\text{delay}}$, and `S3-A` `NegAddConst`), pre-registered gates `G2A.1`–`G2A.5`, `CrossChannelCtwBaseline` / `RnnBaseline` compute metering (`S4-A.1`, `S4-A.2`), derived scaling laws (`Rec 1-a`–`Rec 5-a`, `d2-A`), Cochran permutation fallback, and `20..1020` / `20020..=20039` / `30000..=30019` attainment results live in [parameter_inventory.md §1](parameter_inventory.md#1-active-parameters-bounds-and-guards) and Sections 8 and 9 below.

---

## 2. Empirical Measurements & Verification Ledger

### 2.1 Charter v1 to v2/v3 Foundational Decisions (`FOUNDATION`)

Between `doc/foundational_research_charter.md`, `doc/foundational_research_charter_critique.md`, `doc/counter_critique.md`, and `doc/counterpoint_response.md`, four architectural settlements shaped the platform before code was written:

1. **Substrate choice (`Decision D1`):** Boolean GF(2) was generalized to exact modular arithmetic over $\mathbb{Z}_{2^k}$ with `Mul` (so GF(2) is the $k=1$ special case). Pure `{+, -}` floats were rejected because `{+, -}` is linear (incapable of chaos), IEEE `f64` is still finite ($\le 2^{64}$ states), and non-associativity plus epsilon tolerances defeat exact equivalence and `egg` e-graph consolidation (`counterpoint_response.md` §5).
2. **Epistemological asymmetry:** *Disagreement is conclusive; agreement is provisional.* Contradictions justify pruning; agreement justifies only provisional retention—not permanent promotion (`counterpoint_response.md` §1).
3. **Single-channel redundancy & S-A vs. S-B sleep ablation:** Conceded that one channel suffices when two independent internal derivations contradict in loopback (e.g., period-2 rule $\hat{A}(t+1)=A(t-1)$ colliding with period-6 rule $\hat{A}(t+1)=A(t-5)$). Rather than assuming free-run loopback suffices, Charter v2+ mandated comparing **S-A** (free-running loopback) vs. **S-B** (loopback + buffered wake-trace replay) under equal compute (`counterpoint_response.md` §2–§3).
4. **Two routes to deduction:** Sleep performs **Route 1 deduction** (closure of held axioms/circuits via composition and e-graph equivalence), while wake performs **Route 2 deduction** under a class axiom (`Berlekamp–Massey` / `Reeds–Sloane`, `Gold`, `Angluin` $L^*$) from observed ground facts (`counterpoint_response.md` §7).

### 2.2 Milestone 0 Closeout Measurements (`MILESTONE-0`, `2026-11-01`)

#### Pre-Registered Amendment (`G3 -> G3-rev1`)

During M0 closeout (`doc/milestone_0_closeout_plan.md` §3 WP-M0C.5), original Gate `G3` was formally amended to `G3-rev1`:

- **State accounting separation:** Distinguished internal `Delay` count from `effective_state_registers = delay_count + wire_hop_register_count + boundary_lag_register_count`, checking `effective_excess_state_bits_over_c_mu >= 0.0` (while recording the $k=2$ `[0, 1, 3]` witness discrepancy documented in Section 3.1).
- **Prefix vs. certified synthesis:** Replaced finite-prefix synthesis claims with bounded-cycle joint certification, exposing the `period-257` false-zero trap (Section 3.1).
- **Paired oracle regret & noisy CTW horizon:** Added `paired_regret_bits_per_symbol` against the exact clean-phase Bayes channel oracle on identical noise draws, sized `CTW` to `context_depth = 9`, $T = 20{,}000$ post-warmup ticks (regret $\le 0.030$ b/sym on $\ge 19/20$ seeds, mean $\le 0.028$ b/sym), and scoped enforceable noisy `CGP` checks ($\le 0.020$ b/sym on $\ge 18/20$ seeds) to `T1_Constant` and `T1_Alternating` while keeping multi-register noisy `CGP` as non-gated characterization.

#### Closeout Suite Outcomes (`doc/milestone_0_review.md`)

| Check | Scope / Provenance | Measured Result |
| :--- | :--- | :--- |
| `cargo test --locked` | Dirty working tree prior to `00eb700`, `rustc 1.99.1` | **Passed:** `27` unit, integration, and acceptance tests |
| `cargo fmt --check`, `cargo clippy --locked --all-targets --all-features -- -D warnings`, `cargo package --allow-dirty`, `npx --no-install markdownlint-cli2` | Same working tree | **Passed** |
| `verify-m0 --seed-offset 0 --seeds 20` (dev) | `git_dirty: true`, no source-tree FNV digest yet | **Passed:** `G1`–`G4` `all_passed == true`, `40` case reports, `0` failures |
| `verify-m0 --seed-offset 10000 --seeds 20` (holdout) | Same (`git_dirty: true`) | **Passed:** `G1`–`G4` `all_passed == true`, `40` case reports, `0` failures; noisy `CGP` $\ge 18/20$ on enforced cases |
| Independent Reeds–Sloane brute-force probe | Off-harness verification + `cargo test` | Matched minimal lengths & taps on all **`65,536`** width-2 length-8 sequences, **`4,368`** width-2 recurrences of lengths `1..=3` after $2L$ symbols, and `500` random $\mathbb{Z}_4$ + adversarial zero-divisor streams |

#### Exploratory Noisy `T1_Alternating` Characterization (`seeds 20..=39`, $\varepsilon = 0.05$, `doc/milestone_0_review.md`)

| Width $k$ | Baseline | Mean excess over $h_\mu$ (bits/symbol) | Individual seeds $> 0.02$ |
| :---: | :--- | :---: | :---: |
| `1` | `CTW` | `+0.010872` | `5 / 20` |
| `1` | `CGP` | `-0.000033` | `1 / 20` |
| `2` | `CTW` | `+0.018848` | `11 / 20` |
| `2` | `CGP` | `-0.000122` | `5 / 20` |

> [!WARNING]
> **M0 Limitations Carried Forward (`doc/milestone_0_review.md`):**
>
> 1. **Equal-compute & common-loss gap:** `CTW` and `ReedsSloane` ignore `TickBudget` (`BudgetAccountingMode::AnalyticalUnmetered`). In a `budget = 0` width-1 alternating probe, `CTW` and `ReedsSloane` achieved `0` post-warmup misses reporting `0` ticks consumed while `CGP` made `160` misses. Furthermore, `CTW` reports distributional log-loss whereas circuit predictors report prequential KT residual point-prediction loss. This must be resolved before making cross-baseline M2 claims or M4 equal-compute sleep comparisons.
> 2. **Noisy $C_\mu$ in G2:** M0 closeout left noisy `c_mu_bits` unasserted in G2. Inspection of retired `src/verify.rs` confirmed `G2` subsequently asserted `(em_noisy.c_mu_bits - exp_c_mu).abs() < 1e-8` whenever `msp_sync_status` is `BoundedQuantizedEstimate` or `ExactZeroComplexity` (bounded by the `max_mixed_states = 4096` / $10^{-12}$ quantization threshold).

### 2.3 Stage M1a Kill-Criterion Trigger & Diagnosis (`2026-11-06`, `doc/m1a_gate_findings.md`)

Evaluated on development seeds `0..20` (`T = 1024`, `source_tree_fnv1a128 = 57b6dd875dbc57ec8a0a2751195688c7`) under the original pre-redesign economy (`c_base = 0.020`, `c_split = 0.035`, `c_wire = 0.004`, `CenteredSurprisal`, accumulating eligibility traces $\gamma = 0.90$, `ever_active`):

| Gate | Cases | Outcome | Finding |
| :--- | :---: | :---: | :--- |
| `G1A.1` | `1,770` | **Passed** | Max conservation error `0.000000000000 < 1e-12`; zero substrate floats; $k=8, M=32$ sampling matched exact within $\pm 0.05$ across `64` ticks |
| `G1A.2` | `1,060` | **FAILED (30)** | **Charter §7 Kill Triggered** under $\varepsilon = 0.05$: whole circuits earned $\Delta E > 149$ bits, yet interior combinational nodes ended $\le E_{\text{init}} = 4.0$ (`addc1` $k=1$ Node 2 `Add`: `20/20` seeds failed, $E_2 \in [-0.90, 1.89]$; `counter3` $k=2$ Node 5 `A2`: `10/20` seeds failed, $E_5 \in [2.13, 3.49]$) |
| `G1A.3` | `1,201` | **Passed** | Max post-tick-0 detour credit `0.0` on original panel (before `onefire` detour was added) |
| `G1A.4` | `1,467` | **FAILED (1)** | **Revisit Triggered ($\rho_s < 0.90$):** `0` sign inversions across all `1,467` runs, `17/18` groups $\rho_s \ge 0.93$, but `counter3` ($k=2, \varepsilon=0.05$) scored $\rho_s = 25/28 \approx 0.8929$ |

**Root Causes Quantified:**

1. **Accumulating trace dilution (`G1A.2`):** In `addc1`, the self-loop `Delay(1) <-> Split` summed $\sum_{t=0}^\infty \gamma^t = 1/(1 - 0.90) = 10.0$, giving ancestry weights `c_0 ≈ 7.5` (`Delay`), `c_1 ≈ 7.5` (`Split`), `c_2 = 1.0` (`Add`), normalizer `Z = 16.0`. `Add` received only `6.25%` (`≈ 28.9` bits vs `32.768` bits maintenance over `1,024` ticks, `-3.9` net) while the constant loop exceeded `150` bits. In `counter3`, `Eqz(x + 2)` gated `A2` to `0` on `66.7%` of ticks while charging `100%` maintenance.
2. **$O(T)$ rent vs. $O(1)$ MDL rank swap (`G1A.4`):** Measured 7-circuit rankings on `counter3` ($k=2, \varepsilon=0.05$, $T=1024$):

| Circuit | Class | Nodes | Accuracy | $-\Delta L_{\text{null}}$ (MDL) | $\Delta E$ (Old Economy) |
| :--- | :--- | :---: | :---: | :---: | :---: |
| `counter3` | `C_opt` | 8 | 95.0% | **839.9** (Rank 1) | **704.6** (Rank 1) |
| `addzero` | `C_detour` | 11 | 95.0% | **787.2** (Rank 2) | **611.4** (Rank 2) |
| `cancelsub` | `C_detour` | 14 | 95.0% | **731.6** (Rank 3) | **518.2** (Rank 4) |
| `mulzero` | `C_detour` | 15 | 95.0% | **714.4** (Rank 4) | **493.6** (Rank 5) |
| `inc1` | `C_partial` | 3 | 63.4% | **596.3** (Rank 5) | **543.0** (Rank 3) |
| `const1` | `C_chance` | 2 | 23.8% | **-27.3** (Rank 6) | **-69.0** (Rank 6) |
| `const0` | `C_chance` | 2 | 23.8% | **-29.3** (Rank 7) | **-73.0** (Rank 7) |

*(Arithmetic note recorded in `doc/m1a-revisited-review.md`: `m1a_gate_findings.md` §3.2 miscomputed the `cancelsub` vs `inc1` maintenance, code-length, and prediction gaps as `247 / 64 / ≈200` bits; recomputed values were `348 / 188 / ≈323` bits.)*

### 2.4 Pre-Redesign 100-Bank Sweep & `onefire` Exploit (`doc/m1a-revisited-review.md`, `seeds 20..2019`)

Evaluating 100 disjoint 20-seed banks exposed why the intermediate proposal (`m1a-revisited-proposal.md`: Fix #1 trace cap + Fix #2 `ever_active` + Fix #3 halved maintenance `0.010 / 0.0175 / 0.002`) only passed by luck on seeds `0..19`:

| Configuration (at $T = 1024$) | `G1A.2` (/100) | `G1A.4` (/100) | All Four Gates (/100) |
| :--- | :---: | :---: | :---: |
| Baseline code (`m1a_gate_findings` code) | 0 | 8 | **0** |
| Intermediate proposal (#1 + #2 + halved rent) | 24 | 100 | **24** |
| Fix #1 only (cap trace $\le 1$) | 100 | 8 | **8** |
| Fix #1 + maintenance `×0.8` *(magic-number sweep)* | 98 | 100 | **98** |
| Fixes #1 & #2 + maintenance `×0.8` *(sweep)* | 98 | 100 | **98** |
| No fixes + maintenance `×0.8` *(sweep)* | 97 | 100 | **97** |

- **Why Fix #3 failed:** Over `2,000` seeds, `alt` $k=2$ `const1` centered surprisal was a random walk with **mean `-8.9`, s.d. `21.7`, max `+64.9` bits**. Full maintenance was `68.6` bits (`1.9–2.8×` code length), but halving dropped it to `34.3 < 64.9`, failing `76/100` banks. Without Fix #1 at `×0.8`, margins were dangerously thin (`+1.65` bits on `addc1`, `+3.87` on `counter3`); Fix #1 alone rescued `counter3` `A2` to `+15.5` above `E_init`.
- **Why horizon scaling was irreconcilable:** At $T=2048$, best swept scale shifted to `×0.7` while `×0.8` fell to `~72%`; at **`T = 4096`, zero banks passed** under any constant scale. Making rent proportional to $L(C)$ peaked at `~59%`.
- **Single-fire detour exploit (`onefire`: `Add(Base, Delay{init:1}(ConstLoop(0)))`):**
  - Under `ever_active` (Fix #2): leaked **`220–800` bits**, surviving on **`9/9`** task/width pairs.
  - Under pre-redesign `ever_nonzero`: leaked **`28–263` bits**, surviving on **`7/9`** pairs.
  - Under windowed activity (`W ∈ {16, 64, 128}`): dropped to `0.4–17.6` bits (ending `54–71` bits below `E_init`).
  - Under **D4 ($\gamma = 0.90$ exponential decay)**: decayed `<1%` after `44` ticks, with total post-tick-0 leak **identical within $10^{-9}$ bits** between `T=1024` and `T=4096`.
- **Calibrated point-forecast (`D2`) off-harness Python sanity check on `alt` ($\varepsilon=0.05$, `2,000` runs at `1024`, `500` at `4096`):**

| Circuit on `alt` | Centered `T=1024` (bits) | Calibrated `D2` `T=1024` | Centered `T=4096` | Calibrated `D2` `T=4096` |
| :--- | :---: | :---: | :---: | :---: |
| `const1`, $k=2$ (`C_chance`) | `-84.5 .. +50.7` | **`0 .. 0`** | `-137.2 .. +101.8` | **`0 .. 0`** |
| `osc3`, $k=2$ (`C_chance`) | `-10.5 .. +34.5` | `-1.3 .. +4.2` | `-31.6 .. +53.2` | `-1.3 .. +2.9` |
| `osc2_anti`, $k=1$ (`C_wrong`) | `-480.0 .. -436.0` | `-3.2 .. +2.0` | `-1902.9 .. -1797.8` | `-3.2 .. 0.0` |
| `osc2`, $k=2$ (`C_opt`) | `+492.8 .. +541.8` | `+707.4 .. +862.8` | `+1966.8 .. +2078.0` | `+2952.8 .. +3299.9` |

### 2.5 Redesigned M1a & Milestone 1 Attainment Sign-Offs (`2026-11-07`–`2026-11-08`)

Implementing `D1`–`D4` (`RewardMode::CalibratedPointForecast`, one-time description debt `L(C) = Σ ΔL_i`, replacing traces $\le 1$, $\gamma = 0.90$ activity decay) resolved the $O(T)$ vs. $O(1)$ paradox completely: $\Delta E(T) \equiv -\Delta L_{\text{null}}(T)$ became an exact identity across all horizons.

- **M1a Multi-Horizon 100-Bank Result (`seeds 20..2019`, `doc/milestone_1_plan.md` §3.6.2):** **`100 / 100` banks passed all four gates (`G1A.1`–`G1A.4`) simultaneously at $T = 1024$, $T = 2048$, and $T = 4096$.**
- **Single-Shot Combined Milestone 1 Holdout (`2026-11-07`, `doc/milestone_1_plan.md` §10.1–10.2):**
  - Command: `cargo run --locked --release -- verify-m1 --seed-offset 10000 --seeds 20 --holdout`
  - Provenance: commit `e29014a5c4e410f90797db079d457740c5c2d4be`, `git_dirty: false`, `source_tree_fnv1a128 = 564de6fbdb0b8cbd898b91fc6b4a408a`, `cargo_lock_fnv1a128 = 8d29cd24b1fbdbb743cf5c0b3933b22d`, `rustc 1.99.1 (58cf8f9c3 2026-10-13)`, `x86_64-unknown-linux-gnu`.
  - Outcome (`all_passed: true`, `stage_gate_aborted: false`):
    - **Stage M1a:** **`6,245 / 6,245` passed** (`G1A.1`: `1,959`; `G1A.2`: `1,069`; `G1A.3`: `1,579`; `G1A.4`: `1,638`).
    - **Stage M1b:** **`294 / 294` passed** (`G1B.1`: `80`; `G1B.2`: `200`; `G1B.3`: `4`; `G1B.4`: `10`).
- **Post-Holdout Fix Caveat:** As documented in `milestone_1b_review.md` (Finding 4), commit `9544e98` subsequently modified `74` lines across `src/structural/growth.rs` and `src/structural/learner.rs` (`source_tree_fnv1a128 = ae9e3c4d577d071163c1e0855f3342a0`) after the `e29014a` holdout sign-off.
- **Post-Review Hardening 100-Bank Run (`2026-11-08`, `doc/milestone_1_plan.md` §10.3 & `doc/milestone_1b_plan.md` §9.3):**
  - Command: `cargo run --locked --release -- verify-m1 --banks 100 --seed-offset 20 --seeds 20` (leaving `10000..=10019` untouched), `source_tree_fnv1a128 = 10806db8e8a213e338d563a904cd36df`, `cargo_lock_fnv1a128 = 8d29cd24b1fbdbb743cf5c0b3933b22d`.
  - Result (`all_passed: true`, `stage_gate_aborted: false`):
    - **Stage M1a (`T = 1024`):** **`100 / 100` banks passed** (`G1A.1`: `100/100`, `G1A.2`: `100/100`, `G1A.3`: `100/100`, `G1A.4`: `100/100`; **`624,500` cases**, `0` failures).
    - **Stage M1b (`T = 2000`):** **`100 / 100` banks passed** (`G1B.1`: `100/100` [`8,000` cases], `G1B.2`: `100/100` [`36,000` cases], `G1B.3`: `100/100` [`126,400` cases], `G1B.4`: `100/100` [`82,600` cases]; **`253,000` cases** = `2,530`/bank, `0` failures).
  - Hardening suite status at that checkpoint: `104` workspace tests passed (`milestone_1b_review.md` §5.1), `TicksPerHop` unprimed post-edit output ancestry handled without panicking, Epoch-1 constant-mode parity resolved noisy constant thrashing, empty-circuit ring lockout removed, `ObservationWindow` expanded to `juvenile_ticks + 2 * diagnostic_window = 256` ticks covering the exact $W^* = 25.46 / 0.1665 = 152.9$-tick Shannon crossover on `NoisyAlternating(k=1)`, ring closure isolated per output port, `null_calibrated_mdl_delta_bits` signed negative for compression, Miller–Madow switched to empirical residual support, and `1u64 << width.bits()` replaced with `width.mask()`.

#### Spatial Topology Pivot & Congestion Evidence (`DR #29` / `DR #30`, `milestone_1b_review.md` §1.2 & `milestone_1b_plan.md` §10)

Independent probes at `9544e98` across all four 2-stage M1b tasks (`master_seed = 0`, `horizon = 2000`) quantified planar `BoundaryOut(0)` perimeter congestion under `LocalVonNeumann` vs. `WellMixed`:

| Task | $k$ | `WellMixed` (`tax=true`) match / nodes / splits / $L(C)$ | `WellMixed` (`tax=false`) $L(C)$ | `LocalVonNeumann` (`tax=true`) match / nodes / splits / $L(C)$ |
| :--- | :---: | :---: | :---: | :---: |
| `T1_PeriodicMotif([0, 1, 1])` | 1 | `1.0000` / 4 / 1 / `52.36b` | `44.60b` | `0.6175` / 2 / 1 / `34.17b` |
| `T1_PeriodicMotif([0, 1, 3])` | 2 | `1.0000` / 5 / 1 / `65.23b` | `57.03b` | `0.8600` / 9 / 6 / `125.26b` |
| `T1_AffineLag1(1, 1, 0)` | 2 | `1.0000` / 6 / 2 / `84.57b` | `66.72b` | `1.0000` / 8 / 4 / `119.68b` |
| `T3b_CounterModN(3)` | 2 | `1.0000` / 5 / 2 / `72.92b` | `55.75b` | `0.6169` / 2 / 1 / `34.17b` |

`LocalVonNeumann` blocked second-stage convergence on `3/4` tasks, whereas `WellMixed` solved `4/4` while `split_code_tax` still priced fan-out by `+7.76` to `+17.85` bits. Under Charter Decision Record #30, `LatticeCircuit`, `GridShape`, `Coord`, perimeter anchors, and the spatial ablations (`LocalVonNeumann`, `LocalMoore`, `G1B.4a` Axis 3, and the M1a spatial detour panel) were permanently retired, with full historical spatial reproduction anchored at commit tag **`m1-final-lattice`**.

---

## 3. Discovered Mathematical, Substrate, and Search Traps

### 3.1 M0 Computational-Mechanics, Window, and Evolutionary Traps (`doc/milestone_0_lessons_learned.md`)

1. **`k = 1` KT anti-correlation symmetry (Trap 1):** `KT(matches, mismatches)` is symmetric in matches/mismatches, and at $k = 1$, $m \log_2(2^1 - 1) = 0$. Consequently `facts_code_bits` scored a 100%-wrong inverter identically to a 0%-error predictor until monotonised above expected uniform errors $\lfloor T(1 - 2^{-k})\rfloor$.
2. **Sliding-window phase slip (Trap 2):** Single-tick eviction from trailing history shifted tick parity every step; a temporary `PHASE_ALIGNED_TRIM_STEP = 24` workaround still broke prime periods (`5`, `7`, `257`). Solved permanently via `ObservationWindow` (`start_tick`, `prior_frame`) plus cycle-modular register alignment (`align_autonomous_state_to_tick`).
3. **"Noisy echo" local minimum & MDL sample crossover (Trap 3):** On `T1_Alternating` ($\varepsilon=0.05, k=2$), reactive `BoundaryIn -> Delay(0)` suffers double-error echo misses (`~9.7%` error, `+0.24` b/sym excess), trapping `~15%` of CGP seeds (`mean_excess = 0.0367 > 0.02`). The optimal 3-node autonomous `Osc2(0, 3) + Split` costs `+~26` model bits while saving `~5.3` data bits per avoided error (`~4.7%` fewer errors), placing the **MDL sample crossover at $N \approx 130\text{–}180$ ticks**. Solved with two-stage CGP pacing (`<= 60` fast / `/6` throttled), zero-cost `drift_inactive`, and `mutate_recruit_gene`, reducing paired regret to `< 0.002` b/sym.
4. **Early-window starvation & state tie-breaking (Trap 4):** Evaluating `Exhaustive` below `t = 28..60` let empty `0`-node circuits (`2.0` bits) beat valid circuits (`27–50` bits) and burned `TickBudget`. Additionally, `1`-node reactive circuits (`BoundaryIn -> Eqz` = `1` effective register, `11.6` bits vs `BoundaryIn -> Delay` = `2` effective registers, `12.6` bits) tie-broke wrong without `(circuit_code_bits, effective_state_registers)` sorting.
5. **Channel-draw binomial variance & CTW universal redundancy (Trap 5):** At $T=4000, k=2, \varepsilon=0.05$, even the clean-state Bayes oracle fluctuates around $h_\mu$ with $\sigma \approx 0.0201\text{ b/sym}$ (`~16%` of optimal runs exceed `+0.02` by luck). Deterministic noisy cycles are infinite-order HMMs where CTW incurs $O(|\mathcal{S}| \log T / kT)$ WST redundancy (`~0.044` b/sym at $T=4000$, dropping to `~0.025` b/sym at $T=20000$, `context_depth = 9`).
6. **Boundary lag & state-vs-node divergence on $\mathbb{Z}_4$:** `BoundaryIn` carries `1` register of causal environment lag (`x(t-1)`). On `T1_PeriodicMotif([0, 1, 3])` over $\mathbb{Z}_4$ ($C_\mu = \log_2 3 \approx 1.585$ bits, 1-register bound), any 1-lag map violates mod-2 parity (`1 ≡ 3`, `3 ≢ 0 mod 2`) and requires **11 nodes** (`0 Delay + 1 BoundaryIn`), whereas a pure shift chain needs **2 nodes** (`2 Delay + 1 BoundaryIn = 3` effective registers). Period-257 `[0;256]++[1]` proved prefix lengths $\le 256$ cannot certify global optimality.
7. **Zero-initialized `TicksPerHop` pipeline illusion:** Initial `0` register fill makes `Eqz(0)` emit `1` on tick `0` before delayed boundary `0` arrives; latency tests must inject non-zero graded symbols (e.g., `[3, 2, 0, ...]`) through `Split`.
8. **Rust & ring subtleties:** Edition `2024` reserves `gen`; `rand_core 0.10` renamed `RngCore -> Rng`; `1u64 << 64` overflows (`Width::mask` and `wrapping_neg` fix circular distance); $\mathbb{Z}_{2^k}$ Reeds–Sloane requires $k$ valuation-indexed witness slots with $O(\log k)$ Newton–Hensel lifting for odd-unit inverses; `Primitive::Eqz` makes `f(x) = (x+1) + Eqz(x+2) mod 4` a global period-3 attractor (`basin_fraction = 1.0`).

### 3.2 Closed-Form Combinational Shortcuts Discovered in M1b (`milestone_1b_plan.md` §4.4)

- **`T1_AffineLag1(1, 1, 0)` at $k = 2$:** Solved without a `Delay` constant loop via `EQZ(x) + x + EQZ(EQZ(x))`.
- **`T3b_CounterModN(3)` at $k = 2$:** Synthesized in **4 combinational nodes** via `EQZ(x) + 2x mod 4` (replacing the hand-built 8-node `Counter3Opt` constant-loop circuit).

### 3.3 M1b Runtime, Halting, and Scaling Traps (`milestone_1b_review.md` §§2–3, commit `9544e98`)

1. **Epoch-1 bypass causing infinite noisy non-zero constant thrashing (`Finding 2`):** Exempting `1`-op Epoch-1 `Identity(in:0)` (`Split`, maturity debt `~13.75` bits vs. $E_{\text{init}} = 4.0$) from `delta_flux > 0` and juvenile survival caused opposing outcomes under $\varepsilon = 0.05$ (`seeds 0..19`, `horizon = 2000`, `warmup = 400`):

   | Task ($\varepsilon = 0.05$) | $k$ | Seeds with `paired_residual_regret <= 0.025` | Mean regret (b/sym) | Mean sprouts / starvation prunes |
   | :--- | :---: | :---: | :---: | :---: |
   | `T1_Constant(0)` | 1 | `20 / 20` | `0.0031` | `0.0` / `0.0` |
   | `T1_Constant(0)` | 2 | `20 / 20` | `0.0031` | `0.0` / `0.0` |
   | `T1_Constant(1)` | 1 | `0 / 20` | `0.6813` | `11.0` / `10.0` |
   | `T1_Constant(3)` | 2 | `0 / 20` | `1.2501` | `11.0` / `10.0` |

   `T1_Constant(0)` passed only by empty-circuit coincidence (`0`-output matches `0`), whereas `Constant(1)` and `Constant(3)` oscillated every cycle (`4.0 < 13.75` starvation at `t = 128`). Fixed by constant-mode parity + unified survival gating before the 100-bank run.
2. **`TicksPerHop` unprimed pipeline register panic (`Finding 1`):** Inserting a new output shift chain leaves `register_eligibility` initialized to zeros (`wire_ancestry == 0`) while an unprimed `0` output differs from `s_{t-1}`, triggering `MissingAncestry { port: 0 }`; fixed by treating unprimed post-edit pipeline registers identically to unconnected ports until upstream signal arrival.
3. **Empty-circuit ring lockout (`Finding 3`):** Guarding ring proposal with `!uses_boundary_inputs()` rejected `0`-node graphs, stalling sparse `T1_PeriodicMotif([0, 0, 0, 0, 0, 1])` at `0.8331 = 5/6`; fixed per-port by requiring `!has_port_driver || port_uses_boundary_in`.
4. **Width & alphabet traps (`M4/M5` preview):** Plug-in Miller–Madow scaled with theoretical $2^k$ rather than observed empirical support, subtracting `2.874` bits at $k = 8, W_{\text{diag}} = 64$ even for binary features; `1u64 << width.bits()` overflowed at $k = 64$. Both immediate bugs were fixed (`empirical support` + `width.mask()`), while fixed-$W_{\text{diag}}$ undersampling at larger joint alphabets remains deferred to M5 ([parameter_inventory.md §2.1](parameter_inventory.md#21-parameter-scaling-derivations--deferred-rules)).
5. **Pre-M2 readiness qualification (`DR #35`):** Although `milestone_1b_review.md` §5.2 wrote *"Unconditionally ready"* after the `2026-11-08` fixes, the same paragraph noted that M2's cross-channel comparators (`EQZ(SUB(a, b))`) and redundancy pathways remain unfunded under Charter §3.4 / §3.1.4 and conflict with `D1b.1` zero-sensitivity pruning. Under [Charter DR #35](foundational_research_charter_v3.md#appendix-a-decision-record), Milestone 2 was split into **`M2a` (cross-channel predictive coupling)** and **`M2b` (redundancy & comparators)** so `M2a` proceeds cleanly under existing corrector economics while `M2b` awaits a dedicated Charter comparator-reward amendment (Section 6.2 below).

---

## 4. Rejected Alternatives Registry (Do Not Re-Try)

| Proposal | Source Era | Reason Rejected / Empirical Falsification |
| :--- | :--- | :--- |
| **Floating-point `{+, -}` or `{+, -, mul}` substrate** | Charter critique / counter-critique | `{+, -}` is linear (no chaos); `f64` is still $\le 2^{64}$ finite states; non-associativity & $\lvert x-y \rvert < \varepsilon$ thresholds break exact `egg` e-graph equivalence. |
| **Active-flux reward (`Δx != 0`)** | Charter v1 | Maximally rewards 2-node `NOT <-> DELAY` metronomes and starves sparse event predictors. |
| **`AND` as inter-pathway comparator** | Charter v1 | `AND(0, 0) = 0` detects only `1`-coactivation, not agreement (`EQZ(SUB(a, b))` / `XNOR` needed). |
| **Uncentered gross surprisal reward** | Early M1a spec | On 50/50 streams constant `0` earns `0.250 b/tick/node`, out-earning exact 5-node `sparse6` predictor (`0.130 b/tick/node`). |
| **Centered surprisal reward** | Pre-redesign M1a (`m1a_gate_findings.md`, `review.md`) | Zero-mean random walk ($\sigma \approx 0.68$ b/tick on $k=2$ `alt`); luck excursions scale as $O(\sqrt{T})$ (up to `+64.9` bits at `1024`, `+101.8` at `4096`), forcing horizon-fitted rent. |
| **Per-tick maintenance rent (`c_base=0.020, c_split=0.035, c_wire=0.004, tau_mdl=400`, halved, `×0.8`, or `∝ L(C)`)** | Pre-redesign M1a (`m1a-revisited-review.md`) | Irreconcilable $O(T)$ drain vs. $O(1)$ $L(C)$ across horizons: halving passed `24/100` banks; `×0.8` was a magic number passing `98/100` at $T=1024$, `~72%` at $T=2048$, `0%` at $T=4096$; `∝ L(C)` peaked at `~59%`. |
| **Tick-by-tick `facts_code_bits` difference as reward** | `ideas.md` §3 (`m1a-redesign-plan.md`) | Anti-inversion floor is whole-run only; per-tick diffs lose the non-negative Ville martingale property and charge `log2(2^k - 1)` uniformly regardless of symbol skew. |
| **Accumulating eligibility traces** | Pre-redesign M1a | Recurrent self-loops accumulate $1/(1-\gamma) = 10.0$ weight (`93.75%` of credit in `addc1`), starving combinational `Add` correctors. |
| **Permanent `ever_active` / `ever_nonzero` flags** | `m1a-revisited-proposal.md` Fix #2 | `onefire` detour leaks `220–800` bits (`9/9` pairs survive under `ever_active`; `28–263` bits and `7/9` pairs survive under `ever_nonzero`). |
| **Counterfactual / Shapley clamp-to-identity credit** | `ideas.md` §2 (`m1a-redesign-plan.md`) | Produces negative marginal shares incompatible with `apply_tick`, conflates predicting `0` with branch absence, and requires costly multi-tick `Delay` re-simulation. |
| **Sliding window activity cutoff `W` (`16, 64, 128`)** | `m1a-revisited-review.md` | Introduces a free timescale parameter; replaced cleanly by existing $\gamma = 0.90$ exponential decay (`D4`). |
| **Leaky-integrator reserve EMA (`τ`)** | `ideas.md` pathway 1 | Reintroduces a free horizon constant `τ` (another reserve cap under a different name). |
| **Fixed-quantum history trim (`24`)** | Early M0 `CGP` | Corrupts phase alignment for prime periods (`5`, `7`, `257`); replaced by `ObservationWindow`. |
| **2D planar lattice (`LocalVonNeumann`) as default topology** | Late M1 (`coordinate_free_scaling_plan.md`) | Single-cell `Split` relays deadlocked boundary anchors during multi-neuron growth; coordinate-free graph `Circuit` + `L(C)` / `split_code_tax` replaced spatial coordinates while keeping `LocalVonNeumann`/`LocalMoore` only as legacy ablation/M1 verification history. |
| **Unamended M2b wake pruning (`ExcisionMode::BaselineD1b1`)** | M3 Phase 2 paired ablation (`explore-m3` `0..20` & `G3.5(a)`) | Single-node `D1b.1` zero-sensitivity pruning + `!ever_nonzero()` `Delay{0}` bypass fails on `85.9%` (`1,700/1,980`) of `Established` detours (`D1`–`D6`, `D7b`) and suffers `1,300/1,980` destructive primary-cone collapses across `Juvenile` `D2`–`D7b`; furthermore, switching M3's default mode back to `BaselineD1b1` did not restore the Phase 1 `9b5b950` `G2B.2` (`MV_AddEqz_k2_Comb`) regression (the retired M3 review), isolating the bug to delayed-motif priming rather than `M3-E1`/`M3-E2`. |

---

## 5. Parameter Provenance Ledger (M0–M1b)

*Classification records historical M0–M1b origins; active classifications, bounds, and telemetry live in [parameter_inventory.md](parameter_inventory.md).*

| Parameter | Value | Origin & Justification History | Classification |
| :--- | :--- | :--- | :--- |
| `c_base`, `c_split`, `c_wire`, `tau_mdl` | `0.020`, `0.035`, `0.004`, `400` | Heuristic pre-redesign rent constants (`removed_lines_milestone_1_plan.md`); conflicted across $T$ with $L(C)$ | **Removed** (in M1a `D3`) |
| `KT prior` ($\tfrac12$) | `0.5` | Exact Krichevsky–Trofimov prior inherited from `LaplaceFrequency` null model (`milestone_0_plan.md`, `m1a-redesign-plan.md` `D2`) | **Derived** |
| `CTW` verification horizon / depth | `T = 20_000`, `depth = 9` | Sized analytically so $O(\lvert \mathcal{S}\rvert \log T / kT)$ WST redundancy drops from `0.044` ($T=4000$) to $\le 0.025$ b/sym on $k=2$ `AffineLag1` (`lessons_learned.md` §4.5) | **Derived / Sized for WST** |
| `max_mixed_states` / merge eps | `4096` / `1e-12` | M0 non-unifilar ε-machine MSP bounding guard (`milestone_0_plan.md` §12) | **Compute guardrail** |
| `trace_discount` ($\gamma$) | `0.90` | Pre-registered M1a constant; reused unchanged for `D4` activity recency decay; **never sensitivity-swept** (`m1a-redesign-plan.md`) | **Pre-registered (M1)** |
| `initial_energy` ($E_{\text{init}}$) | `4.0` bits | Fixed seed endowment in `milestone_1_plan.md` §5.2; cancels out of M1a $\Delta E$ (`juvenile_ticks = 0`), sets M1b sprout deficit $\Delta L_i - 4.0$ | **Pre-registered (M1)** |
| `juvenile_ticks` ($T_{\text{juvenile}}$) | `128` ticks | Chosen in `milestone_1_plan.md` to exceed M0's `~100`-tick noisy oscillator crossover (M0 actually measured `130–180`; M1b later expanded `obs_capacity` to `256` for `152.9`-tick `NoisyAlternating`) | **Pre-registered (M1)** |
| `max_energy_reserve` ($E_{\max}$) | `Some(25.0)` bits | Locked in `milestone_1_plan.md` §5.2 to cap mature reserves after regime shifts (`D1b.5`); **lower than M1b $\Delta L_{\text{sprout}} \approx 17\text{–}28.5$ bits**, forcing `D1b.2` unclipped juvenile escrow workaround; superseded in `deb200a` / `a47ff8d` by `ReserveCap::HorizonDerived` ([Charter DR #31](foundational_research_charter_v3.md#appendix-a-decision-record), [parameter_inventory.md](parameter_inventory.md)) | **Legacy (superseded by `HorizonDerived`)** |
| `diagnostic_window` ($W_{\text{diag}}$), `sprout_interval` | `64`, `32` ticks | Locked in `milestone_1_plan.md` §5.2 (`obs_window` later unified as $t_{\text{juv}} + 2 W_{\text{diag}} = 256$); M5 scaling deferred ([parameter_inventory.md §2.1](parameter_inventory.md#21-parameter-scaling-derivations--deferred-rules)) | **Pre-registered (M1)** |
| `mi_threshold_bits` ($\theta_{\text{MI}}$), `min_match_gain` ($\Delta_{\min}$) | `0.05` bits, `4` symbols | Locked in `milestone_1_plan.md` §5.2 as M1b noise-floor halting thresholds; M5 prequential code-length test deferred | **Pre-registered (M1)** |
| `max_nodes` | `32` nodes | Compute/bloat cap in `milestone_1_plan.md` §5.2 | **Compute guardrail** |
| `DEFAULT_M1B_HORIZON`, `warmup`, `max_eval_ticks` | `2000`, `500`, `50_000` | Locked M1b evaluation protocol in `milestone_1_plan.md` §5.2 & §6 | **Pre-registered / Fixed** |
| `grid_rows`, `grid_cols`, `max_relay_hops` | `8`, `8`, `4` | Legacy 2D spatial layout defaults (`milestone_1_plan.md` §4.2, §5.2); superseded by coordinate-free `Circuit` | **Legacy (superseded)** |

---

## 6. Preserved Reference Specs & Inherited Questions for M2–M5

### 6.1 Verbatim Preserved Sections from Deleted `doc/milestone_1_plan.md`

For traceability of historical citations (`milestone_1_plan.md` and `milestone_1b_plan.md`), the two most-referenced deleted blocks were:

- **Interface Decisions & Open Questions (`milestone_1_plan.md` §4.0, lines 334–352; originally `m1a-redesign-plan.md` L321–333):**
  1. *Idle nodes vs. starved nodes:* Under description debt, repaid idle nodes maintain $E_i(t) > 0$. Two candidate remedies were identified: (a) structural pruning if removal does not increase $L(C)$ (chosen as `D1b.1`), or (b) **switching distributions** (Herbster & Warmuth, 1998; van Erven, Grünwald & de Rooij, 2012) to re-charge inactive structure using fixed switching priors without tuned rent.
  2. *Juvenile grace period and sprout debt:* Initial endowment $E_{\text{init}} = 4.0$, grace counter $T_{\text{juvenile}} = 128$, one-time $\Delta L_i$ charged at maturity.
  3. *Edits change existing nodes' debt shares:* Enforced conservation by charging each sprout marginal $\Delta L_{\text{sprout}} = L(C \cup \{\text{sprout}\}) - L(C)$ (`D1b.3`).
  4. *Port match counters ($M$):* Decide whether upstream graph edits preserve or reset $M$ (`D1b.4` chose reset on affected ports while preserving `NullModel`).
  5. *Homeostatic reserve cap ($E_{\max}$):* Set `max_energy_reserve = Some(25.0)` for online regime shifts (`D1b.5`).
  6. *Alternative null models:* Derived matched estimator for `NullModelKind::RepeatLast` (`D1b.6`).
  7. *Unowned circuit bits:* Track direct `BoundaryIn -> BoundaryOut` or zero-node header bits (`D1b.7`).

- **Locked `ExperimentConfig` Schema (`milestone_1_plan.md` §5.2, lines 444–481):**

```toml
schema_version = 1
master_seed = 42

[substrate]
width_bits = 2
wire_delay = "Combinational"
wiring_topology = "LocalVonNeumann"

[environment]
horizon_ticks = 2_000
transient_warmup_ticks = 250
generator = { kind = "T1_PeriodicMotif", motif = [0, 1, 3] }
noise = { kind = "None" }

[budget]
max_eval_ticks = 50_000

[baseline]
kind = "ArkheM1"
grid_rows = 8
grid_cols = 8

[baseline.economy]
trace_discount = 0.90
initial_energy = 4.0
juvenile_ticks = 128
max_energy_reserve = 25.0
null_model = "LaplaceFrequency"
reward_mode = "CalibratedPointForecast"

[baseline.growth]
diagnostic_window = 64
sprout_interval = 32
mi_threshold_bits = 0.05
min_match_gain = 4
max_nodes = 32
```

### 6.2 Critical Open Issues & Dropped Nuance Relevant to M2–M5

1. **Redundancy funding paradox (`counterpoint_response.md` §2.1, Readiness `Q5` / [Charter DR #35](foundational_research_charter_v3.md#appendix-a-decision-record) — resolved for `M2b` in DR #40–#42):** Under unamended wake rules ($\Delta E = -\Delta L_{\text{null}}$ with comparator reward in *"sleep only"*), a redundant derivation plus `EQZ(SUB(a, b))` adds $L(C) > 0$, earns `0` wake bits, and has `0` boundary sensitivity so `D1b.1` deterministically prunes it. Resolved in Sections 10–11 (archived from retired `docs/milestone_2b_plan.md`) via fact-anchored wake corroboration + frozen-null sleep agreement, strict causal-support disjointness, and derived $(Z_{\text{alt}} + 2)/Z_{\text{alt}}$ maturation scaling (`DR #40`–`#42`).
2. **Switching codes vs. ad-hoc reserve caps (`m1a-redesign-plan.md` Open Q #1):** Replacing `D1b.1` idle heuristics and `Some(25.0)` caps with information-theoretic switching codes (`Herbster & Warmuth 1998`; `van Erven et al. 2012`) was deferred from M1b and remains an open principled candidate for regime shifts in M3+.
3. **Common compute & loss across heterogeneous baselines (`milestone_0_review.md` §4, `milestone_0_plan.md` §8.3 — implemented in `M2a`):** `src/baselines/rnn.rs` (`RNN` BPTT) and `src/baselines/cross_ctw.rs` (`CrossChannelCtwBaseline`) were pre-registered and implemented in `M2a` Phase 3 (`5886135`) using `ContextTreeDepthSteps` and `BpttUnrollSteps` with unified target-channel prequential KT residual scoring ([Section 8.2](#82-m2a-architectural-mechanisms-baseline-metering--gate-definitions), [parameter_inventory.md §1](parameter_inventory.md#1-active-parameters-bounds-and-guards)). Unmetered single-channel `CTW` / `ReedsSloane` (`AnalyticalUnmetered`) remain explicitly labeled in `Scorecard` and must stay accounted for in `M4a` equal-compute sleep comparisons.
4. **Sleep replay phase integrity (`milestone_0_lessons_learned.md` §5.3):** M4a offline sleep loopback ($\hat{A}(t+1) \to A(t+1)$) must capture and step from live internal `Delay` register states (or full attractor cycles), never resetting registers to static $t = 0$ `init` values.
5. **Deferred Charter v1 critique mechanisms for M2–M6 (`doc/foundational_research_charter_critique.md` §3.4, §3.6, §4, §5.5):**
   - *Explicit uncertainty / abstention* (§3.4) to distinguish low confidence from channel noise (`rate` / `vote` / `abstain` coding).
   - *Macro boundary detection & retirement* (§3.6): small cut/interface, Markov-blanket test, Hoel causal-emergence effective information, usage decay + palette MDL cost in M5.
   - *Logic synthesis & open-endedness tooling* (§4, §5.5): sequential equivalence checking (SAT/BDD) for sleep consolidation proofs and Bedau–Packard activity statistics for open-ended novelty measurement.

---

## 7. Pre-Milestone 2 Readiness Measurements & Verification (2026-11-13)

Following Decisions `Q1`–`Q9` ([Charter DR #31–#39](foundational_research_charter_v3.md#appendix-a-decision-record)) and `G1B.4a:Axis1` **Option (a)** (`a47ff8d`), all Readiness `A`–`C` code changes landed across commits `deb200a`, `5a7c27d`, `a47ff8d`, `22954df`, `7649e2c`, and `eafcec6` (`source_tree_fnv1a128 = 26e9d332905dd6a6aeadce50c5266fa3`). Complete inventory and full tables live in [parameter_inventory.md §3](parameter_inventory.md#3-compute-profile--measurement-tables).

### 7.0 Readiness `A1` Phase 1–2 Bit-Identity Proof

Verified on `2026-11-13` (`deb200a`) by extracting the 8 Phase 0 golden fixtures from `2d009b0` (`git archive 2d009b0 tests/fixtures/golden | tar -x -C /tmp/arkhe_phase0_golden`), temporarily overriding `EconomyConfig::default()` with `ReserveCap::Fixed(25.0)` and pointing [`tests/golden_traces.rs`](../tests/golden_traces.rs) at the extracted fixtures, then running `cargo test --test golden_traces -- --nocapture`: `test golden_traces_are_bit_identical ... ok` (`1 passed; 0 failed; finished in 15.83s`). Reverting those temporary overrides confirmed that the re-blessed trajectory digests differ solely due to Phase 3 (`ReserveCap::HorizonDerived`), with identical structural event sequences and final circuits.

### 7.1 `B6` Empirical Regime-Shift Latencies (Charter §3.1.2)

- Single-seed diagnostic (`shift_tick=350`, `Motif5 -> Alt(k=1,clean)`): `W_obs = 256`, starvation at tick `543` (`delta_starve = 193`), immediate resprout at tick `543` (`clipped_count = 55`, `clipped_bits = 36.749`).
- Across 10 seeds (`0..10`) in [`tests/regime_shift.rs`](../tests/regime_shift.rs):
  - `Motif5 -> Alt(k=1,clean)`: `10/10` starved and resprouted cleanly at `207` ticks (`0.809x W_obs`).
  - `Alt(0,1) -> Constant(0)(k=1)`: `10/10` starved and resprouted cleanly at `143` ticks (`0.559x W_obs`).
  - `Motif013 -> Mod3(k=2,clean)`: `10/10` starved and resprouted cleanly at `20` ticks (`0.078x W_obs`).
  - `Motif5 -> Alt(k=1,eps=0.05)`: `10/10` starved and resprouted, but only `2/10` fell within `<= W_obs` (`min=239`, `p50=294.0`, `mean=307.2`, `p95=355.0`, `max=399` ticks = `1.559x W_obs`). This directly confirms DR #31 (`Q1(a)`): `ReserveCap::HorizonDerived` (`W_obs = 256`) bounds stationary energy reserve and eventually replaces obsolete predictors under noise, without guaranteeing `delta_starve <= W_obs` on stochastic mismatches. (Note also that `Alt(0,1) -> Motif011` retains `4/6 = 66.7% > p_null = 0.5` partial match and therefore remains solvent instead of starving.)

### 7.2 `B7` Multi-Channel Lock-Time Ratios

Across seeds `0..10` ([`tests/multi_channel_development.rs`](../tests/multi_channel_development.rs)) with per-port `PortDevelopmentalState`:

- `K=2:(Alt,Motif011)` `Combinational`: `ch0 = 1.00x` (`64/64`), `ch1 = 1.00x` (`255/255`, `MaxSnapshots = 3`).
- `K=2:(Alt,Motif011)` `TicksPerHop`: `ch0 = 1.05x` (`64 -> 67`), `ch1 = 1.00x` (`66/66`).
- `K=3:(Alt,Motif011,Motif001)` `Combinational`: `ch0 = 1.00x` (`64/64`), `ch1 = 0.25x` (`255 -> 65` concurrent speedup via cross-channel feature reuse), `ch2 = 1.02x` (`66 -> 67`).
- `K=3:(Alt,Motif011,Motif001)` `TicksPerHop`: `ch0 = 1.00x` (`64/64`), `ch1 = 1.00x` (`66/66`), `ch2 = 1.00x` (`66/66`).

### 7.3 `B5` Phase 0 Compute Scaling & Guardrail Awareness

Release compute profiling (retired `scripts/measure_compute_profile.py` on `7649e2c`, 5 seeds, `horizon=512` controlled / `1024` endogenous):

- **Node scaling (`steady_clean`, `K=1`):** rises smoothly from `2.04 us/tick` (`N=0`) through `29.30` (`N=8`) and `67.10` (`N=16`) to `206.86 us/tick` (`N=32`, roughly quadratic $\sim O(N^2)$).
- **Channel scaling (dominant cost axis):** even at `N=0`, clean per-tick cost climbs $\sim O(K^3)$ across $K$: `2.04` ($K=1$) $\to$ `9.90` ($K=2$) $\to$ `66.96` ($K=4$) $\to$ `516.97 us/tick` ($K=8$), reaching `2293.29 us/tick` (`steady_clean`) and `3067.05 us/tick` (`diagnostic_noise`, `sprout_blocked_max_nodes = 3055` with `0.00` wasted replay ticks) at $N=32, K=8$.
- **Endogenous $K=1..8$:** all 6 endogenous tasks (clean and $\varepsilon=0.05$) reached `0` post-500 clean errors with `0` starvation prunes and `0` `max_nodes` blocks; `K=8` solved all 8 binary motifs simultaneously in `18` final nodes (`< max_nodes = 32`). For Milestone 2 planning (`Q3`), `max_nodes = 32` leaves headroom for small multi-channel circuits, while Phase 0 of larger multi-channel / comparator suites should watch the $O(K^3)$ per-tick scaling factor.

### 7.4 `A5` M1 Verification Summary (`source_tree_fnv1a128 = 26e9d332905dd6a6aeadce50c5266fa3`)

- **Dev fast slice (`0..20`):** `all_passed: true`, `stage_gate_aborted: false`, `excluded_cases: 0`, `sprout_blocked_max_nodes: 0` (`26.159s`).
- **Dev multi-bank (`20..2020`, `100` banks × `20` seeds):** `100/100` M1a banks and `100/100` M1b banks passed all gates (`excluded_cases: 0`, `sprout_blocked_max_nodes: 0`, `reserve_mismatch_floored: 0`; 99% two-sided Clopper–Pearson lower bound `0.948396`, $\chi^2 = 0.0, p = 1.0$; `7m19.102s` on 16 cores).
- **Fresh confirmation bank (`20000..=20019`):** `all_passed: true`, `stage_gate_aborted: false`, `excluded_cases: 0`, `sprout_blocked_max_nodes: 0` (`26.387s`).
- M1 holdout (`10000..=10019`) remained untouched; M2a holdout (`30000..=30019`) was subsequently executed once at commit `aa6fb6d` (Section 9.2), while `40000..=40019` (`M2b`) and `50000..=50019` (`M3`) were then reserved and were subsequently consumed and closed (see §§11–12).

---

## 8. Milestone 2a Derived Scaling Laws & Rule 4 Golden Trace Rationale (2026-11-14)

When `verify-m2a` expanded the active scale axes ($\tau \le 12$, multi-channel $K \in \{2, 3, 4\}$, shared-wire `TicksPerHop`, and $\varepsilon = 0.05$ channel/target noise), five coupled scaffolding/credit bottlenecks emerged and were resolved cleanly with zero free parameters (`Rec 1-a` through `Rec 4-a`, approved `2026-11-14`):

1. **`effective_max_nodes` (`Rec 1-a`, [`growth.rs`](../src/structural/growth.rs)):** Replaces fixed `max_nodes = 32` for multi-channel graphs with $K \cdot \max(N_{\max}, 2\tau_{\text{deep}} + 8)$ (`32 K` at defaults) while reducing identically to `max_nodes` (`32`) at $K \le 1$, and moves sprout `sprout_blocked_max_nodes` emission from per-candidate rejection to failed band proposals (`best_circuit.is_none()`).
2. **Maturation-horizon MDL amortization (`Rec 2-a` + depth-invariant amendment, [`growth.rs`](../src/structural/growth.rs)):** `effective_juvenile_ticks(d_cone)` compensates per-node retrograde $\gamma^d$ credit attenuation in metabolic survival and `node.juvenile_ticks`, whereas output statistical bit savings over `W_diag` do not decay by $\gamma^d$. Both `amort_scale` and `rank_scale` therefore use the depth-invariant horizon ratio `(juvenile_ticks / W_diag).max(1.0)` (`2.0` at defaults for $K > 1$ or $d_{\text{cone}} > \tau_{\max}$, `1.0` at $K=1, d_{\text{cone}} \le 3$), admitting noisy $\varepsilon=0.05$ cross-channel couplings without exponentially amplifying noise correlations on deep lags.
3. **Additive active-zero symmetry & smoothed `HorizonDerived` reserve retention (`Rec 3-a`, [`sensitivity.rs`](../src/structural/sensitivity.rs), [`economy.rs`](../src/structural/economy.rs)):** `ring_identity_input_weights` assigns `raw[j] = input_active[j]` whenever `values[j] == 0` on `Add` (and on `Sub` right-hand `0` / double-`0`), matching `Mul`'s active identity `1` while keeping passive `0.0` inputs zeroed. In `EconomyState::apply_tick`, `NodeEconomy::smoothed_drain_rate` preserves $\max(\text{drain}, \gamma \cdot \text{smoothed\_drain\_rate})$ across mature ticks so single-tick zero-share dropouts decay smoothly rather than snapping `ReserveCap::HorizonDerived` down to `initial_energy` (`4.0`).
4. **Sibling-port non-regression (`Rec 4-a`, [`growth.rs`](../src/structural/growth.rs)):** `BaselineReplayMetrics` and `evaluate_sprout_motif` track per-port mismatches on all driven sibling ports $q \ne p_{\text{out}}$ and reject any sprout whose upstream `Split` / rewiring degrades a driven sibling.

### 8.1 Rule 4 Golden Trace Re-Blessing & Backward M1 Invariance Proof

- Under `Rec 3-a`, nodes with active `0` inputs on `Primitive::Add` retain symmetric causal shares and `smoothed_drain_rate` prevents abrupt `HorizonDerived` reserve collapses (reducing `verify-m1 --seed-offset 0 --seeds 20` `reserve_clipped_count` from `752,272` to `446,897`). Because [`tests/golden_traces.rs`](../tests/golden_traces.rs) hashes per-node bit-level `energy.to_bits()` into `trajectory_digest_fnv1a`, re-blessing (`ARKHE_BLESS=1 cargo test --locked --test golden_traces`) updated `trajectory_digest_fnv1a` on **only** the 3 combinational `Add`-containing fixtures (`affine_k2_comb_s23.json`, `motif011_k1_comb_s22.json`, `motif013_k2_comb_s22.json`), with **zero** changes to structural `events`, `final_circuit_nodes`, or `final_circuit_edges` across all 8 fixtures.
- Release `verify-m1 --seed-offset 0 --seeds 20` passed every M1a and M1b gate cleanly (`all_passed: true`, `stage_gate_aborted: false`, `excluded: 0`, `G1A.1`: `1959`, `G1A.2`: `1065`, `G1A.3`: `1575`, `G1A.4`: `1638`, `G1B.1`: `80`, `G1B.2`: `360`, `G1B.3`: `1104`, `G1B.4`: `666`; `reserve_mismatch_floored=0`, `sprout_blocked_max_nodes=0`), alongside `100 passed; 0 failed` unit tests (`cargo test --locked --lib -- --skip verify_m2a::tests::fast_single_seed`), `m0_acceptance` (`7/7`), `m1b_neurogenesis` (`8/8`), `multi_channel_development` (`4/4`), `regime_shift` (`2/2`), and `golden_traces` (`1/1`).

### 8.2 M2a Architectural Mechanisms, Baseline Metering & Gate Definitions

Consolidated from retired `doc/milestone_2a_plan.md` §§2–8:

1. **Six exploratory findings (`seeds 0..20`, `26e9d332905dd6a6aeadce50c5266fa3`) & structural resolutions:**
   - *LFSR lag aliasing:* `deg 5` ($x^5 + x^2 + 1$) aliases $A(t-8) = A(t-5) \oplus A(t-3)$ and lets univariate `ReedsSloane` solve linear sums in $2L = 40$ ticks; `verify-m2a` therefore combines small-state `deg 5 × deg 4` tasks ($\le 465$ states, exact $\varepsilon$-machine ground truth $C_\mu = 8.8611$ bits) with high-degree `deg 11 × deg 9` nonlinear (`EQZ`/`MUL`) tasks where `ReedsSloane` fails (`0/10`, `0.5853` err).
   - *Per-port `TickBudget` fairness (`S2`, `DEFAULT_M2A_BUDGET = 200_000`):* Unsolved exogenous LFSR drivers previously consumed `49,889..49,951 / 50,000` replay ticks; `ArkheLearner::port_budget_allowance` assigns each port `total_budget / num_ports` plus an equal share of locked ports' unused pool (`SproutBudgetExhausted` telemetry).
   - *Cross-channel Epoch-1 selection (`C1-a`):* Partitions Epoch-1 candidates into `best_same` (`c == out_port`, preserving M1 `is_1_op` priority) and `best_cross` (`c != out_port`, pure `j_cand`), comparing winners by `j_cand` so `20%`-error `Identity(in:target)` never overrides `0%`-error cross-channel nonlinear motifs.
   - *Ring-before-deepening order (`C2-a`, `deepening_max_lag = 12`):* Runs `propose_autonomous_delay_ring` before progressive lag bands `(3..=6] -> (6..=12]` (`LagDeepeningSteps`), suppressing intermediate cliff emissions until a stage succeeds or the deepest permitted band fails.
   - *`TicksPerHop` strict `edge.hops == 1` & arm-offset alignment (`G2A.2(b)`):* Forbids unpriced `edge.hops > 1` and dummy single-output `Split` buffers; aligns mixed-depth motif arms to exact causal lag $d_{\text{arm}} = 1 + h_{\text{arm}} + 2 n_{\text{delay}}$ (`tau_diag = h_arm + 2 n_delay`).
   - *$\mathbb{Z}_{2^k}$ affine motif closure (`S3-A`):* Adds `CandidateMotif::NegAddConst(u, c)` (`Sub(ConstLoop(c), u)` for $k > 1$) so non-decomposable bijections such as $3x + 2 \bmod 4$ are 1-step reachable with zero new constants.
2. **Baseline protocols (`S4-A.1`, `S4-A.2`):**
   - `CrossChannelCtwBaseline` (`src/baselines/cross_ctw.rs`): `symbol_context_depth = 9`, clamped to `max_interleaved_bit_depth = 64` (`(9 * K * k).clamp(4, 64)`), causal past-frame + intra-channel bit-plane conditioning, metered via `BudgetAccountingMode::ContextTreeDepthSteps` (`bit_depth` ticks/step in `observe()`, `0` in `predict()`, gated on `TickBudget::try_consume`).
   - `RnnBaseline` (`src/baselines/rnn.rs`): 1-layer `tanh` Elman RNN (`hidden_size = 16`, `bptt_len = 16`, `lr = 0.05`, online `TBPTT(1, 16)`), concatenated one-hot inputs $K \cdot 2^k$ guarded by `MAX_RNN_ONE_HOT_WIDTH_BITS = 8`, $K$ softmax heads, Glorot uniform init from `StreamDomain::BaselineSearch { trial: 0 }`, zero biases, unclipped gradients, metered via `BudgetAccountingMode::BpttUnrollSteps` (`min(history_len, bptt_len)` ticks/step in `observe()`).
3. **Pre-registered `verify-m2a` gate specifications (`T = 2000`, `warmup = 500`, `budget = 200_000`, `0` `Excluded` cases):**
   - `G2A.1` (`CP99 LB >= 0.90`): Clean `Combinational` cross-channel coupling (`T2_DelayedAffine`, `T2_CrossLinear`, `T2_CrossNonlinear`, $k \in \{1, 2\}$) achieves `0` post-warmup target clean mismatches (`excess <= 0.005` b/sym), wires to $\ge 1$ `BoundaryIn(c != target)`, and marginal $K=1$ control fails (`clean_err >= 0.10`).
   - `G2A.2` (`CP99 LB >= 0.90`): Progressive deepening solves non-aliasing deep lags ($d \in \{5, 6, 8\}$ on `deg 11 × deg 9`) in `Combinational`, and hop-aligned wiring solves causal `TicksPerHop` tasks (`DelayedAffine(d=3)`, `CrossSum(d=3,3)`, `CrossSum(d=3,5)`, `AddEqz(d=3,4)`, `MulEqzPair(d=4,4)`).
   - `G2A.3` (`CP99 LB >= 0.90`): Concurrent $K \in \{2, 3, 4\}$ lock-time scaling satisfies `t_lock(multi) <= 1.50 * max(t_lock(solo), W_diag)`.
   - `G2A.4` (`CP99 LB >= 0.85`): Under $\varepsilon = 0.05$, (a) target-only noise regret vs. clean Bayes oracle $\le 0.030$ b/sym; (b) all-channel noise paired difference vs. `CausalReactiveOracle` $\le 0.040$ b/sym **and** pooled/bank task mean $|\mathbb{E}[\mathrm{KT}] - H_{\text{reactive}}(\varepsilon, m, k)| \le 0.040$ b/sym.
   - `G2A.5` (`CP99 LB >= 0.85` & pooled mean KT margin $\ge 0.25$ b/sym): Equal-compute benchmark on `deg 11 × deg 9` (`TickBudget = 200_000`), admitting per-seed exact-solution parity against `RandomSearch`/`CGP` (`Rec 5-a`: both at `100%` clean match within `1e-6` KT b/sym) while keeping pooled mean margin $\ge 0.25$ and nonlinear `CTW`/`ReedsSloane` margins $\ge 0.25$ strict.

---

## 9. Milestone 2a Implementation, Cochran Fallback, & Verification Record (`2026-11-14`)

All four Milestone 2a phases (`691ad6f`, `e9fc185`, `5886135`, `9fc8689`, `918cb02`, and `aa6fb6d`) landed with **`source_tree_fnv1a128 = 19c86928ef76420ed887c0ead7a97abe`** (`rustc 1.99.1 (58cf8f9c3 2026-10-13)`, `x86_64-unknown-linux-gnu`, `cargo_lock_fnv1a128 = 8d29cd24b1fbdbb743cf5c0b3933b22d`), `cargo fmt --check` clean, `cargo clippy --all-targets --all-features -- -D warnings` clean (`0` warnings), and `cargo test --locked` passing `142/142` tests (`105` unit + `37` integration tests including bit-identical `golden_traces`).

### 9.1 Statistical Homogeneity & `G2A.4b`/`G2A.5` Resolution

- **Deferred lossy Stage-0 sprout (`918cb02`):** Across multi-channel deepening runs, transient short-lag noise/aliasing correlations at `t=63` with `e_cand > 0` previously committed prematurely before deepening bands `(3..=6]`/`(6..=12]` could discover exact deep-lag couplings. Deferring `e_cand > 0` Stage-0 candidates across `Stage 2b` + `Stage 2c` when `can_deepen_eligible` holds locks exact `d=6..12` rules cleanly while keeping M1 `K=1` trajectories bit-identical.
- **Cochran rare-failure conditional permutation fallback (`src/verify_m1/stats.rs`):** On `G2A.5` across `20..1020` (`983/1000`, `50/50` banks passing `>= 17/20`), bank failure counts (`38×0`, `9×1`, `1×2`, `2×3`) have expected failure count $E_{\text{fail}} = 20 \times 0.017 = 0.34 \ll 5.0$, violating Cochran's validity threshold for Pearson's continuous $\chi^2_{49}$ approximation ($p_{\chi^2} = 0.00895 < 0.01$). Replacing the asymptotic tail with a deterministic `100,000`-replicate conditional permutation test (`ChaCha8Rng::seed_from_u64(0)`) whenever $E_{\min} < 5.0$ preserves exact $\alpha = 0.01$ homogeneity rigor and yields **`p = 0.01997 >= 0.01`** on `G2A.5` (`p = 1.00000` on `G2A.4`).

### 9.2 Milestone 2a Acceptance Summary (`19c86928ef76420ed887c0ead7a97abe`)

| Bank / Suite | Command | Outcome | Gate / Case Summary | `excluded_cases` | `sprout_blocked_max_nodes` | Wall time (16 cores) | Key Cliff Counters |
| :--- | :--- | :--- | :--- | ---: | ---: | ---: | :--- |
| M1 backward check (`0..20`) | `target/release/arkhe verify-m1 --seed-offset 0 --seeds 20` | `all_passed: true`, `stage_gate_aborted: false` | `G1A.1`: `1959`, `G1A.2`: `1065`, `G1A.3`: `1575`, `G1A.4`: `1638`; `G1B.1`: `80`, `G1B.2`: `360`, `G1B.3`: `1104`, `G1B.4`: `666` (`0` failures) | `0` | `0` | `26.333s` | `reserve_clipped_count=446,897` (`259,214.76` bits), `reserve_mismatch_floored=0`, `candidate_undecidable=118`, `candidate_list_truncated=893`, `lag_horizon_bound=1,200` |
| M2a dev fast slice (`0..20`) | `target/release/arkhe verify-m2a --seed-offset 0 --seeds 20` | `all_passed: true` | `G2A.1`–`G2A.4`: **`20/20`** (`max_g4b_dev=0.010456 <= 0.040`); `G2A.5`: **`19/20`** (`min_margin=0.499830 >= 0.25`) | `0` | `0` | `1m51.943s` | `reserve_clipped_count=665,824`, `reserve_mismatch_floored=0`, `candidate_undecidable=48,303`, `lag_horizon_bound=15,197`, `lag_deepening_steps=33,118`, `sprout_budget_exhausted=93,819` |
| M2a dev multi-bank (`20..1020`, `50` banks × `20` seeds) | `target/release/arkhe verify-m2a --banks 50 --seed-offset 20 --seeds 20` | `all_passed: true` | `G2A.1`–`3`: **`50/50` banks (`1000/1000`, `CP99=0.995405`, $\chi^2=0.0, p=1.0$)**; `G2A.4`: **`50/50` (`990/1000`, `CP99=0.979957`, $p=1.00000$, `max_g4b=0.007207`)**; `G2A.5`: **`50/50` (`983/1000`, `CP99=0.970871`, $\chi^2=75.4593$, $p=0.01997$, `min_margin=0.904210`)** | `0` | `0` | `81m58.409s` | `reserve_clipped_count=33,533,643`, `reserve_mismatch_floored=0`, `candidate_undecidable=2,427,364`, `candidate_list_truncated=2,074,633`, `lag_horizon_bound=758,091`, `ring_period_bound=0`, `lag_deepening_steps=1,666,583`, `sprout_budget_exhausted=4,692,105` |
| M2a fresh confirmation (`20020..=20039`) | `target/release/arkhe verify-m2a --seed-offset 20020 --seeds 20` | `all_passed: true` | `G2A.1`–`G2A.5`: all **`20/20` (`1.0000`)** (`max_g4b_dev=0.007992 <= 0.040`, `min_margin=0.919261 >= 0.25`) | `0` | `0` | `1m52.358s` | `reserve_clipped_count=674,402`, `reserve_mismatch_floored=0`, `candidate_undecidable=48,916`, `lag_horizon_bound=15,165`, `lag_deepening_steps=33,174`, `sprout_budget_exhausted=94,208` |
| M2a single-shot holdout (`30000..=30019`, `aa6fb6d`, `git_dirty=false`) | `target/release/arkhe verify-m2a --seed-offset 30000 --seeds 20 --holdout` | `all_passed: true` | `G2A.1`–`G2A.5`: all **`20/20` (`1.0000`, `CP99=0.794328`)** (`max_g4b_dev=0.009303 <= 0.040`, `min_margin=0.895113 >= 0.25`) | `0` | `0` | `1m53.165s` | `reserve_clipped_count=675,584` (`1,184,947.79` bits), `reserve_mismatch_floored=0`, `candidate_undecidable=48,612`, `candidate_list_truncated=41,497`, `lag_horizon_bound=15,174`, `ring_period_bound=0`, `lag_deepening_steps=33,768`, `sprout_budget_exhausted=94,084` |

### 9.3 `G2A.5` Equal-Compute Multi-Bank Benchmark (`20..1020`, `1,000` seeds/task, $T = 2000$, Budget $= 200,000$)

Target-channel mean prequential KT residual bits/symbol [`clean_match_rate`] (plus mean compute ticks consumed over $T=2000$):

| Task ($k$-bit) | `ArkheM1` (`CandidateHistoryReplay`) | `RandomSearch` (`CandidateHistoryReplay`) | `CGP` (`CandidateHistoryReplay`) | `CTW` (`AnalyticalUnmetered`) | `ReedsSloane` (`AnalyticalUnmetered`) | `CrossChannelCTW` (`ContextTreeDepthSteps`) | `RnnBptt` (`BpttUnrollSteps`) |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `CrossSum(d=1,2,k=1)` | `0.0041` [`1.0000`] (`199,900`) | `1.0171` [`0.4867`] (`199,892`) | `0.9978` [`0.5004`] (`199,939`) | `1.0034` [`0.5100`] (`0`) | `0.0041` [`1.0000`] (`0`) | `0.0041` [`1.0000`] (`54,000`) | `0.4611` [`0.8312`] (`31,880`) |
| `CrossSum(d=1,2,k=2)` | `0.0041` [`1.0000`] (`199,904`) | `1.1736` [`0.5664`] (`199,892`) | `1.5180` [`0.4302`] (`199,940`) | `2.0044` [`0.2493`] (`0`) | `0.0041` [`1.0000`] (`0`) | `1.9502` [`0.3733`] (`108,000`) | `2.0054` [`0.2556`] (`31,880`) |
| `CrossMulEqz(d=1,2,k=2)` (NL) | `0.0041` [`1.0000`] (`199,840`) | `1.0672` [`0.7891`] (`199,892`) | `0.9083` [`0.8309`] (`199,941`) | `0.9463` [`0.8260`] (`0`) | `2.0157` [`0.2380`] (`0`) | `0.4877` [`0.9287`] (`108,000`) | `0.1009` [`0.9890`] (`31,880`) |
| `CrossAddEqz(d=1,2,k=2)` (NL) | `0.0041` [`1.0000`] (`199,840`) | `1.1573` [`0.7667`] (`199,892`) | `1.1573` [`0.7667`] (`199,938`) | `2.0035` [`0.2573`] (`0`) | `1.0897` [`0.7867`] (`0`) | `0.7816` [`0.8667`] (`108,000`) | `1.0858` [`0.7768`] (`31,880`) |

### 9.4 `G2A.5` Single-Shot Holdout Benchmark (`30000..=30019`, `20` seeds/task, $T = 2000$, Budget $= 200,000$)

Target-channel mean prequential KT residual bits/symbol [`clean_match_rate`] (plus mean compute ticks consumed over $T=2000$; frozen clean binary `aa6fb6d`, `git_dirty = false`):

| Task ($k$-bit) | `ArkheM1` (`CandidateHistoryReplay`) | `RandomSearch` (`CandidateHistoryReplay`) | `CGP` (`CandidateHistoryReplay`) | `CTW` (`AnalyticalUnmetered`) | `ReedsSloane` (`AnalyticalUnmetered`) | `CrossChannelCTW` (`ContextTreeDepthSteps`) | `RnnBptt` (`BpttUnrollSteps`) |
| :--- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `CrossSum(d=1,2,k=1)` | `0.0041` [`1.0000`] (`199,900`) | `1.0171` [`0.4867`] (`199,892`) | `0.9610` [`0.5194`] (`199,952`) | `1.0034` [`0.5100`] (`0`) | `0.0041` [`1.0000`] (`0`) | `0.0041` [`1.0000`] (`54,000`) | `0.4613` [`0.8332`] (`31,880`) |
| `CrossSum(d=1,2,k=2)` | `0.0041` [`1.0000`] (`199,904`) | `1.3037` [`0.5169`] (`199,892`) | `1.6081` [`0.3992`] (`199,942`) | `2.0044` [`0.2493`] (`0`) | `0.0041` [`1.0000`] (`0`) | `1.9502` [`0.3733`] (`108,000`) | `2.0056` [`0.2544`] (`31,880`) |
| `CrossMulEqz(d=1,2,k=2)` (NL) | `0.0041` [`1.0000`] (`199,840`) | `1.0912` [`0.7818`] (`199,892`) | `0.8992` [`0.8347`] (`199,935`) | `0.9463` [`0.8260`] (`0`) | `2.0157` [`0.2380`] (`0`) | `0.4877` [`0.9287`] (`108,000`) | `0.0812` [`0.9922`] (`31,880`) |
| `CrossAddEqz(d=1,2,k=2)` (NL) | `0.0041` [`1.0000`] (`199,840`) | `1.1573` [`0.7667`] (`199,892`) | `1.1573` [`0.7667`] (`199,945`) | `2.0035` [`0.2573`] (`0`) | `1.0897` [`0.7867`] (`0`) | `0.7816` [`0.8667`] (`108,000`) | `1.1835` [`0.7464`] (`31,880`) |

- **Holdout Seed Governance:** M2a single-shot holdout (`30000..=30019`) executed once (`all_passed: true`, `excluded_cases: 0`); M1 holdout (`10000..=10019`), M2b (`40000..=40019`) and M3 (`50000..=50019`) were not used during M2a; M2b and M3 were subsequently consumed and closed (see §§11–12).

---

## 10. Stage M2b Comparator-Economics Theoretical Analysis (`2026-11-14`, Charter DR #40–#43)

Prior to Milestone 2b (archived from retired `docs/milestone_2b_plan.md`), analytical MDL and calibrated-flux modeling resolved the Pre-M2 redundancy funding paradox (`DR #35`):

1. **Why unamended wake economics, pure boundary MDL, and sleep-only funding fail:**
   - On clean stationary streams where primary derivation $\hat{y}_a$ already achieves `0` mismatches, adding an independent route $\hat{y}_b$ plus `SUB` + `EQZ` adds $\Delta L_{\text{red}} \in [40.11, 137.34]$ bits of `L(C)` debt while saving `0` boundary data bits, causing `100%` starvation at graduation under pure single-stream boundary MDL.
   - Deferring comparator reward strictly to offline sleep causes any redundant complex sprouted in wake to hit maturity (`t_juv`) with `0` wake credit ($4.0 - \Delta L_{\text{red}} < 0$), starving before sleep begins.
   - Scoring `alt(t) == primary(t)` blindly in wake allows mutually agreeing hallucinating oscillators to sustain each other against observed facts and penalises a clean backup route whenever `primary`'s input channel corrupts.
2. **Per-node maturation dilution across composite comparator sprouts:**
   - Because `SUB` and `EQZ` add `COMPARATOR_PRIMITIVE_COUNT = 2` unit-sensitivity nodes downstream of `alt`, each node in `alt` receives fraction $Z_{\text{alt}} / (Z_{\text{alt}} + 2)$ of comparator flux. Under fixed `juvenile_ticks = 128`, a 5-new-node noisy (`eps = 0.05`, $k=1$) redundancy sprout ends at $E_{\min} = -0.57 \le 0$ bits (`dl = 73.31`, `flux = 89.73`) and a 10-new-node sparse (`p = 1/6`) period-6 ring + comparator ends at $E_{\min} \approx -7.58 \le 0$ bits (`dl = 137.34`, `flux = 78.99`).
   - Scaling comparator juvenile ticks by the zero-free-parameter causal-dilution law $\lceil \text{effective\_juvenile\_ticks}(d_{\text{alt}}) \cdot (Z_{\text{alt}} + 2) / Z_{\text{alt}} \rceil$ restores positive graduation reserves (`+12.3` to `+40.7` bits across all modeled `T1`/`T2` clean and $\varepsilon = 0.05$ cases) while reducing identically to `effective_juvenile_ticks` when `N_comp = 0`.
3. **Ratified Charter rules (`DR #40`–`#43`):** Fact-anchored wake corroboration + frozen-null sleep agreement with source-independence weight $\iota(a, b) = \frac{1}{\lvert C_a \cap C_b \rvert + 1}$ on positive sleep agreement (`1.0` on disagreement), exact $\Delta E(T) = -\Delta L_{\text{null}}^{\text{epistemic}}(T)$ conservation, strict causal-support disjointness (`active_derivations == 1` trigger), two-clause wake disagreement pruning (`disagreements >= min_match_gain` AND rolling calibrated flux $\le 0$ while peer $> 0$, avoiding `5%`-noise sprout/prune thrashing), and default `redundancy_sprouting = true`.

---

## 11. Milestone 2b Implementation, Derived Scaling Laws, & Verification Record (`2026-11-15`)

Milestone 2b (archived from retired `docs/milestone_2b_plan.md`, `docs/milestone_2b_chief_scientist_review.md`, and `docs/milestone_2b_stage3_blocker_report.md`) implemented wake/sleep epistemic comparator economics (`M2B-D1`), strict causal-support disjoint redundancy neurogenesis (`M2B-D2`), offline loopback falsification, and Phase 4 dev-ratified derived scaling laws (`M2B-A1`–`M2B-A3`, `M2B-B1`–`M2B-B3`, `M2B-C1`), passing dev/confirmation prior to user-authorized single-shot holdout `40000..=40019` (`b211afa`).

### 11.1 Implemented Architecture & Derived Amendments (`8ca68a2`–`220349f` + Phase 4)

- **Phase 1 (`8ca68a2`):** Added `ComparatorSite` registration/remap ([`src/structural/surgery.rs`](../src/structural/surgery.rs)), wake fact-anchored corroboration vs. frozen-null sleep agreement $\iota(a, b)$ & comparator-isolated causal sensitivity ([`src/structural/economy.rs`](../src/structural/economy.rs), [`src/structural/sensitivity.rs`](../src/structural/sensitivity.rs), [`src/structural/live.rs`](../src/structural/live.rs)), `epistemic_null_calibrated_mdl_delta` ([`src/metrics/mdl.rs`](../src/metrics/mdl.rs)), `GeneratorConfig::T2_MultiViewCoupled` with finite-state $\varepsilon$-machine export ([`src/env/tiers.rs`](../src/env/tiers.rs)), and scheduled channel-fault corruption ([`src/env/noise.rs`](../src/env/noise.rs)).
- **Phase 2 (`85d736e`):** Added `GrowthConfig::redundancy_sprouting` (default `true`), `effective_comparator_juvenile_ticks`, Stage 3 causal-support masking (Tier 1 channel-disjoint before Tier 2 disjoint-lag/autonomous ring) with all-port `∀ q: e_cand[q] <= e_curr[q]` protection and `TicksPerHop` autonomous-ring phase compensation ([`src/structural/growth.rs`](../src/structural/growth.rs), [`src/structural/graph_edit.rs`](../src/structural/graph_edit.rs)), and `run_offline_loopback` (`ExecutionPhase::Sleep`) falsification & majority pruning ([`src/structural/learner.rs`](../src/structural/learner.rs)).
- **Phase 3 (`498365a`):** Exposed comparator telemetry fields (`redundancy_sprout_events`, `active_comparator_count`, `comparator_agreement_ticks`, `comparator_disagreement_ticks`, `cumulative_comparator_flux`, `driver_handoff_events`, `disagreement_prune_events`) on `ArkheM1Telemetry` ([`src/config.rs`](../src/config.rs)) and added end-to-end integration tests ([`tests/m2b_comparators.rs`](../tests/m2b_comparators.rs)).
- **Dev Amendments (`83ca660`, `220349f`):**
  - `M2B-A1` / `M2B-A2` / `M2B-A3` (`83ca660`): Decoupled reversible `W_diag = 64` log-odds handoff from `W_obs = 256` irreversible disagreement pruning, aligned `G2B.4` with exact structural-class Bayes oracles, enforced clean-port exactness (`e_curr == 0 => e_alt_diag == 0`), and separated Pass-1 Stage 1+2 from Pass-2 Stage 3 without deadlocking on permanently exogenous `!is_port_driven` channels.
  - `M2B-B1` / `M2B-B2` / `M2B-C1` (`220349f`): Unified Stage-2 and Stage-3 per-node debt amortization (`compute_debt_amortization_juvenile_ticks`, `endowment = new_nodes * initial_energy` across all depths, evolving `juvenile_ticks = 128` to statistical floor $t_{\text{juv,min}}$) under the zero-free-parameter two-clause decidability law ($t_{\text{cone}} \le 2 W_{\text{obs}} = 512$ anti-noise crossover AND $t_{\text{juv}} \le \lceil 2 W_{\text{obs}} \max(t_{\text{base}}/t_{\text{juv,min}}, Z_{\text{tot}}) \rceil$ anti-parasite causal-dilution ceiling), added atomic 1-sided comparator bankruptcy excision (`excise_port_comparator`), forwarded `candidate_undecidable` + `lag_horizon_bound` from Pass 2, and gated `stage3_exhausted` on `obs_full`.
- **Phase 4 (`verify-m2b`):** Implemented [`src/verify_m2b/mod.rs`](../src/verify_m2b/mod.rs) (`G2B.1`–`G2B.5`), CLI dispatch ([`src/lib.rs`](../src/lib.rs)), and `--holdout` tests blocking `10000..=10019`, `30000..=30019`, `50000..=50019`, and unflagged `40000..=40019` ([`tests/cli.rs`](../tests/cli.rs)).

### 11.2 Rule 4 / `M2B-D3` Golden-Trace Structural Audit

Under ratified `M2B-D3` (`GrowthConfig::default().redundancy_sprouting = true`), once a single-channel `Combinational` predictor locks cleanly with `0` primary error, Stage 3 forms a causal-disjoint autonomous-delay-ring comparator (`Delay` chain + `Split` + `Split` + `Sub` + `Eqz`), triggering a Rule 4 re-blessing in commit `85d736e`:

1. **`3` `TicksPerHop` fixtures unchanged (`100%` bit-identical to `8ca68a2`):** `alt_k1_tph_s20.json` (`775be53cf08b2ef1efdeba055a49aa3f`), `alt_k2_tph_s21.json` (`ce91fd5dc0fec64afd2cd1092b450f37`), and `affine_k2_tph_s23.json` (`51700627f37ce63888f663a62b2ab1a4`) retain `1` node, `2` edges, and `1` event identically.
2. **`5` `K=1 Combinational` fixtures audited (`events` identical, appended autonomous-ring comparator only):**
   - `alt_k1_comb_s20.json`: `7` nodes, `9` edges, `1` boundary event, digest `4edeb338e057eecfc8b4b8d26ccb1ed8`.
   - `alt_k2_comb_s21.json`: `7` nodes, `9` edges, `1` boundary event, digest `9613bae1d97b370b9b9860b4e8e5a7bc`.
   - `motif011_k1_comb_s22.json`: `11` nodes, `14` edges, `2` boundary events, digest `a89d4b0a761e697d53df25af302ddabc`.
   - `motif013_k2_comb_s22.json`: `12` nodes, `15` edges, `2` boundary events, digest `594d8816b9ceffc91cba04108dff8867`.
   - `affine_k2_comb_s23.json`: `14` nodes, `18` edges, `2` boundary events, digest `66aaf7da3b1074cf9a626df90af7ccef`.
3. **Ablation proof (`redundancy_sprouting = false`):** `structural::learner::tests::redundancy_sprouting_disabled_is_bit_identical_to_all_golden_fixtures` ([`src/structural/learner.rs`](../src/structural/learner.rs)) proves disabling `redundancy_sprouting` reproduces all `8/8` pre-M2b `8ca68a2` trajectory digests bit-for-bit.

### 11.3 Milestone 2b Dev & Confirmation Attainment Summary (`1e1c9dd8a7151060a011d956081fad50`)

All release verification runs executed against the identical compiled Phase 4 source tree (`source_tree_fnv1a128 = 1e1c9dd8a7151060a011d956081fad50`; dev/confirmation recorded at `220349f` and re-verified on clean `main` at `b211afa8a8c30a381ff230c672ac961d03f6e442`, `git_dirty = false`, where single-shot holdout `40000..=40019` ran once after user authorization):

| Suite & Seed Range | Outcome | Gate Results & Pooled Statistics | Auxiliary / Characterization Metric | `excluded_cases` | `sprout_blocked_max_nodes` |
| :--- | :--- | :--- | :--- | ---: | ---: |
| `verify-m1` dev (`0..20`) | `all_passed: true`, `stage_gate_aborted: false` | `G1A.1` (`1959`), `G1A.2` (`1065`), `G1A.3` (`1575`), `G1A.4` (`1638`); `G1B.1` (`80`)–`G1B.4` all `passed: true` | Backward M1 invariance preserved | `0` | `0` |
| `verify-m2a` dev (`0..20`) | `all_passed: true` | `G2A.1` `20/20`, `G2A.2` `20/20`, `G2A.3` `20/20`, `G2A.4` `19/20`, `G2A.5` `19/20` | `G2A.4b` dev `0.010456` b/sym; `G2A.5` margin `0.499830` b/sym | `0` | `0` |
| `verify-m2b` dev (`0..20`) | `all_passed: true` | `G2B.1` `300/300` (`1.0`), `G2B.2` `200/200` (`1.0`), `G2B.3` `140/140` (`1.0`), `G2B.4` `178/180` (`0.988889`), `G2B.5` `40/40` (`1.0`) | `G2B.1` `max_err = 1.895e-15` b/sym; `G2B.4` `0.00105` b/sym; `G2B.5` `+1.43021` b/sym | `0` | `0` |
| `verify-m2b` dev (`20..1020`, `50` banks × `20` seeds = `1,000` seeds) | `all_passed: true` | `G2B.1` `15000/15000` (`LB99 = 0.999693`, $p = 1.0$); `G2B.2` `10000/10000` (`LB99 = 0.999540`, $p = 1.0$); `G2B.3` `7000/7000` (`LB99 = 0.999342`, $p = 1.0$); `G2B.4` `8797/9000` (`LB99 = 0.973532`, `mean = 0.977444`, `std = 0.009617`, `p5 = 0.961111`, $\chi^2 = 36.997051, df = 49, p = 0.90383$); `G2B.5` `2000/2000` (`LB99 = 0.997700`, $p = 1.0$) | `G2B.1` `max_err = 1.89478e-15` b/sym; `G2B.4` `0.003639` b/sym; `G2B.5` `+1.42432` b/sym | `0` | `0` |
| `verify-m2b` conf (`20040..=20059`) | `all_passed: true` | `G2B.1` `300/300` (`1.0`), `G2B.2` `200/200` (`1.0`), `G2B.3` `140/140` (`1.0`), `G2B.4` `177/180` (`0.983333`), `G2B.5` `40/40` (`1.0`) | `G2B.1` `max_err = 1.895e-15` b/sym; `G2B.4` `0.00865` b/sym; `G2B.5` `+1.42105` b/sym | `0` | `0` |
| `verify-m2b` single-shot holdout (`40000..=40019`, `b211afa`) | `all_passed: true` | `G2B.1` `300/300` (`1.0`), `G2B.2` `200/200` (`1.0`), `G2B.3` `140/140` (`1.0`), `G2B.4` `178/180` (`0.988889`), `G2B.5` `40/40` (`1.0`) | `G2B.1` `max_err = 1.895e-15` b/sym; `G2B.4` `0.00827` b/sym; `G2B.5` `+1.42922` b/sym | `0` | `0` |

- **1,000-seed cliff telemetry (`20..1020`):** `reserve_clipped_count = 94,653,731` (`72,251,297.41` bits), `reserve_mismatch_floored = 0`, `sprout_blocked_max_nodes = 0`, `candidate_undecidable = 569,573`, `candidate_list_truncated = 836,647`, `lag_horizon_bound = 0`, `ring_period_bound = 1,372`, `lag_deepening_steps = 330,576`, `sprout_budget_exhausted = 1,231,141`.
- **Single-shot holdout cliff telemetry (`40000..=40019`, `b211afa`):** `reserve_clipped_count = 1,902,736` (`1,439,402.43` bits), `reserve_mismatch_floored = 0`, `sprout_blocked_max_nodes = 0`, `candidate_undecidable = 11,469`, `candidate_list_truncated = 16,524`, `lag_horizon_bound = 0`, `ring_period_bound = 29`, `lag_deepening_steps = 5,868`, `sprout_budget_exhausted = 23,868`.
- **Holdout Seed Governance:** M1 (`10000..=10019`) was not reused during M2b; M3 (`50000..=50019`) was then reserved and subsequently consumed and closed at `dd13839` (§12); M2a single-shot holdout (`30000..=30019`) ran once at `aa6fb6d`, and M2b single-shot holdout (`40000..=40019`) ran once after user authorization at `b211afa` (`source_tree_fnv1a128 = 1e1c9dd8a7151060a011d956081fad50`, `all_passed: true`, `excluded_cases: 0`).

---

## 12. Milestone 3 (`H4`) Implementation & Verification Record (`2026-11-22`)

Milestone 3 (`H4`: Free-rider resistance, verified local excision, exogenous MDL conservation, and `SPLIT`-tax pricing, [§12.6](#126-retired-m3-gate-specifications-and-exploratory-lessons)) implemented `M3-E1`, `M3-E2`, `M3-E3`, exploratory paired ablations (`explore-m3`), confirmatory multi-bank verification (`verify-m3`), and four `@chief-scientist` binding adjudications (`#13`, `#15`, `#35`, `#46`) across commits `9b5b950`–`dd13839`, clearing all development (`0..20`, `20..1020`), fresh confirmation (`20060..=20079`), and `@chief-scientist`-authorized single-shot holdout (`50000..=50019`, Stop Point `S3` `#60` / `S4` `#64`) runs with `all_passed: true`, `excluded_cases = 0`, and `sprout_blocked_max_nodes = 0`.

### 12.1 Implemented Architecture & Adjudicated Amendments (`9b5b950`–`a24ea1a`)

- **Phase 1 (`9b5b950`):**
  - **`M3-E1` Target-anchored causal reachability & `W_diag` ring-identity bypass ([`src/structural/learner.rs`](../src/structural/learner.rs), [`src/structural/growth.rs`](../src/structural/growth.rs), [`src/structural/surgery.rs`](../src/structural/surgery.rs)):** Anchored per-frame register latch sensitivity to backward reachability from `BoundaryOut` or registered `ComparatorSite::eqz_node` with non-zero `ring_identity_input_weights`, flagged mature nodes emitting constant additive `0` (`Add`, `Sub.in(1)`, `Mul`) or multiplicative `1` (`Mul`) across `W_diag = 64` frames (or recency $\le \gamma^{W_{\text{diag}}}$) idle, and generalized `build_pruned_circuit` to bypass `Add`/`Sub`/`Mul` consumers cleanly without collapsing `BoundaryOut(p)`.
  - **`M3-E2` Verified local pass-through excision ([`src/structural/surgery.rs`](../src/structural/surgery.rs), [`src/structural/growth.rs`](../src/structural/growth.rs), [`src/config.rs`](../src/config.rs)):** Added `ExcisionMode::{VerifiedLocalExcision, BaselineD1b1}`, `primary_cone_nodes`, `local_excision_events` telemetry, and `excise_verified_passthrough_subgraphs` rewiring mature subgraph $v \to x \in \text{UpstreamCone}(v)$ iff: (i) empirical `W_diag` equality holds, (ii) exhaustive $(2^k)^{\lvert \mathcal{I}_{\text{free}} \rvert}$ equivalence ($\lvert \mathcal{I}_{\text{free}} \rvert \le 4, k \le 4$, binding `ConstLoop`s and tying shared `Split` inputs) or bounded joint-state certification + replay proof holds, (iii) $L(C') < L(C)$, and (iv) all registered `ComparatorSite::eqz_node` trajectories remain unchanged.
  - **`M3-E3` Exogenous injection hook & `D0`–`D7b` detour library ([`src/structural/learner.rs`](../src/structural/learner.rs), [`src/verify_m3/detours.rs`](../src/verify_m3/detours.rs)):** Added `ArkheLearner::apply_external_edit` (`InjectionMode::{Established, Juvenile}`) tracking `exogenous_injection_bits = L(C_post) - L(C_pre)` in `Established` mode and `MarginalDebtLedger::on_sprout` in `Juvenile` mode, plus width/delay-aware `build_detour_edit` across `D0Dead`, `D1AddZero`, `D2OneFire`, `D3MulZero`, `D4MulOne`, `D5CancelSub`, `D6Involution`, `D7aFakeComparator`, and `D7bAgreementGate`.
- **Phase 2 (`e498424`):** Implemented `arkhe explore-m3` ([`src/verify_m3/explore.rs`](../src/verify_m3/explore.rs), [`src/lib.rs`](../src/lib.rs)) and executed the `0..20` exploratory & paired `BaselineD1b1` / `split_code_tax` ablation sweep recorded in [§12.6](#126-retired-m3-gate-specifications-and-exploratory-lessons).
- **Phase 3 (`235a292`):** Implemented `arkhe verify-m3` ([`src/verify_m3/mod.rs`](../src/verify_m3/mod.rs), [`tests/m3_free_riders.rs`](../tests/m3_free_riders.rs), [`tests/cli.rs`](../tests/cli.rs)), added `M3` (`50_000..=50_019`) and `M4a` (originally `60_000..=60_019`, expanded to `60_000..=60_099` under Charter §2.8) holdout guards in `verify_m2a`/`verify_m2b`/`verify_m3`, landed Chief Scientist **`#13` & `#15`** `stage2_exhausted` preservation invariants (`clear_changed_ports_internal` when `!reset_scorer_counts && is_port_driven`; `commit_redundancy_sprout` `autonomous_ring.is_none()`; `perform_comparator_handoff` when `q_hat > p_null`; `apply_external_edit` marking driven ports exhausted) and Rule-9 gate calibrations (`G32_CLEAN_RAW_KT_MAX = 0.0075`, `G33_D2_TRANSIENT_KT_EXCESS_MAX = 0.020`, `G33_D2_PAIRED_KT_DIFF_MAX = 0.015`, post-Stage-3 `t_settle`, noisy periodic `N_min_eff`, and `G3.5(b)` per-clean-task & pooled noisy/overall `Split` non-regression), plus bit-identical speedups (`Option A1` Step-1 passthrough screening, `Option A3` sparse CSR forward reachability and ancestry epoch-zero skip in [`src/structural/sensitivity.rs`](../src/structural/sensitivity.rs), `Option A4` recycled buffers in [`src/baselines/mod.rs`](../src/baselines/mod.rs) and `DiagnosticBuffer`, and prefix-shared `verify-m3` settling).
- **Phase 3b (`004aaa5`):** Under Chief Scientist **`#35`** ruling on `confirm20060` `seed=20076`, extracted `surviving_driver_beats_null` and `refresh_stage_gates_after_comparator_transition` across `excise_port_comparator` and `perform_comparator_handoff`, preserving `stage2_exhausted` whenever `is_port_driven(target_port) && scorer.q_hat() > p_null` while reopening Stage 2 on driver loss or below-null degradation.
- **Phase 3c (`a24ea1a`):** Under Chief Scientist **`#46`** binding adjudication on 50-bank `20..1020`:
  1. **`Option 4B` `effective_max_nodes` Stage-3 scaling law ([`src/structural/growth.rs`](../src/structural/growth.rs)):** Added `stage3_allowance = tau_deep + COMPARATOR_PRIMITIVE_COUNT + 2` (`16`) per channel when `num_channels > 1 && growth_config.redundancy_sprouting` (`48 * K = 144` at $K=3$), reducing identically to `max_nodes = 32` at $K \le 1$ and `32 * K` when `!redundancy_sprouting` (`M2a`), eliminating `seeds 535, 812, 833` Stage-3 max-node blocks and $K=3$ ceiling compression (`92/96`).
  2. **Independent trajectory-unit `BankSample` (`G3.2`, `G3.3`):** Aggregated `G3.2` and `G3.3` `BankSample` at the independent `(task, wire_delay, eps, seed)` settled trajectory unit (`n = 660` per 20-seed bank; `Excluded` if any detour hits `sprout_blocked_max_nodes > 0`, `Passed` iff all applicable detour classes `D0`–`D7b` pass), preserving raw per-detour counts in `aux_metric_summary` / `pooled_metric_name`.
  3. **`G3.1` active-port reference parity (`port_scored_last_observe`):** Exposed read-only `ArkheLearner::port_scored_last_observe` so `evaluate_g31_seed` records offline `epistemic_null_calibrated_mdl_delta_with_tax` frames identically when `ArkheLearner::observe` scores (`is_driven && has_causal_ancestry`), resolving `seed=23` starved 1-node `Delay` divergence (`max_epistemic_mdl_err = 0.000e0`).
  4. **`G3.5(b)` clean solvability (`epsilon > 0.0 || clean_locked_8000`):** Enforced `100%` clean task solvability in `G3.5(b)` while leaving noisy `T2_CrossSum(k=1)` solvability gated at `P_target = 0.90` under `G3.4`.

### 12.2 Retained Regression & Golden-Trace Invariance (`a24ea1a`, Post-Closeout Review Correction)

All M3 v1 checks recorded below executed against clean commit `a24ea1ab24b6f3e2daf5e126c751f0b47e339a38` (`git_dirty = false`, `source_tree_fnv1a128 = 4e5ff20e46d6d87c8716d21cec458b8d`, `cargo_lock_fnv1a128 = 8d29cd24b1fbdbb743cf5c0b3933b22d`, `rustc 1.99.1 (58cf8f9c3 2026-10-13)`, `x86_64-unknown-linux-gnu`):

- `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --locked`: **All passing** (`155/155` unit, `5/5` `cli`, `5/5` `determinism`, `1/1` `golden_traces` bit-identical, `5/5` `m2b_comparators`, `4/4` `m3_free_riders`, `4/4` `multi_channel_development`, `2/2` `regime_shift`).
- `verify-m2a --seed-offset 0 --seeds 20` (`25.298s`): `all_passed: true`, `G2A.1` `20/20`, `G2A.2` `20/20`, `G2A.3` `20/20`, `G2A.4` `20/20` (improved from pre-M3 `19/20` at `b102259`, so not bit-identical to pre-M3 M2a metrics), `G2A.5` `19/20`, `excluded_cases = 0`, `sprout_blocked_max_nodes = 0`.
- `verify-m2b --seed-offset 0 --seeds 20` (`12.971s`): **Single-bank pooled `all_passed: true` masked a concentrated `G2B.2` regression (`180/200` vs. pre-M3 `200/200` at `b102259`)**, `G2B.1` `300/300`, `G2B.3` `140/140`, `G2B.4` `178/180`, `G2B.5` `40/40`, `excluded_cases = 0`, `sprout_blocked_max_nodes = 0`. As documented in the retired M3 review, every failure in `G2B.2` was `MV_AddEqz_k2_Comb` (`0/20` on `0..20`; `0/100` on 5-bank `20..120`, giving `900/1000` with `LB99 = 0.875814 < 0.90`, onset in Phase 1 `9b5b950`), proving that `golden_traces` (`1/1`) bit-identity does not imply blanket M2a/M2b metric identity.

### 12.3 Milestone 3 Dev & Confirmation Attainment Summary (`4e5ff20e46d6d87c8716d21cec458b8d`)

| Suite & Seed Range | Wall Time | Outcome | Gate Results & Pooled / Bank Statistics | Auxiliary & Raw Detour Metrics | `excluded_cases` | `sprout_blocked_max_nodes` |
| :--- | :---: | :--- | :--- | :--- | ---: | ---: |
| `verify-m3` dev fast (`0..20`, `/tmp/verify_m3_dev20.json`) | `82.232s` | `all_passed: true` | `G3.1` `5040/5040` (`1.000000`); `G3.2` `649/660` (`0.983333`); `G3.3` `660/660` (`1.000000`); `G3.4` `730/740` (`0.986486`); `G3.5a` `1540/1540` (`1.000000`); `G3.5b` `740/740` (`1.000000`) | `G3.1` `max_err = 0.000e0` b/sym; `G3.2` `max_excise_dt = 128`, `raw_detours = 5150/5240`; `G3.3` `max_excise_dt = 513`, `raw_detours = 5240/5240`; `G3.5b` `min_split_tax = 7.4919` b/Split, `noisy_splits = 465/507`, `all_splits = 1685/1727`, `inj_excised = 1600/1600 vs 1600/1600` | `0` | `0` |
| `verify-m3` dev multi-bank (`20..1020`, `50` banks × `20` seeds = `1,000` seeds, `/tmp/verify_m3_dev1000.json`) | `68m32.667s` (`4112.667s`) | `all_passed: true` | `G3.1` `252000/252000` (`1.000000`, `LB99 = 0.999982`, $\chi^2 = 0.0, p = 1.0$); `G3.2` `32596/33000` (`0.987758`, `LB99 = 0.986274`, `mean = 0.987758`, `std = 0.004071`, `p5 = 0.982500`, $\chi^2 = 44.314798, df = 49, p = 0.663233$); `G3.3` `33000/33000` (`1.000000`, `LB99 = 0.999860`, $p = 1.0$); `G3.4` `36575/37000` (`0.988514`, `LB99 = 0.987158`, `mean = 0.988514`, `std = 0.003467`, `p5 = 0.983041`, $\chi^2 = 38.382051, df = 49, p = 0.862782$); `G3.5a` `77000/77000` (`1.000000`, `LB99 = 0.999940`, $p = 1.0$); `G3.5b` `37000/37000` (`1.000000`, `LB99 = 0.999876`, $p = 1.0$) | `G3.1` `max_err = 0.000e0` b/sym; `G3.2` `max_excise_dt = 339` ticks, `raw_detours = 259145/262000`; `G3.3` `max_excise_dt = 513` ticks, `raw_detours = 262000/262000`; `G3.5b` `min_analytical_split_tax_bits = 6.9069` (`6.906890595608516`) b/Split | `0` | `0` |
| `verify-m3` confirmation (`20060..=20079`, `/tmp/verify_m3_confirm20060.json`) | `80.721s` | `all_passed: true` | `G3.1` `5040/5040` (`1.000000`); `G3.2` `659/660` (`0.998485`); `G3.3` `660/660` (`1.000000`); `G3.4` `733/740` (`0.990541`); `G3.5a` `1540/1540` (`1.000000`); `G3.5b` `740/740` (`1.000000`) | `G3.1` `max_err = 0.000e0` b/sym; `G3.2` `max_excise_dt = 128`, `raw_detours = 5239/5240`; `G3.3` `max_excise_dt = 513`, `raw_detours = 5240/5240`; `G3.5b` `min_split_tax = 7.4919` b/Split, `noisy_splits = 452/504`, `all_splits = 1672/1724`, `inj_excised = 1600/1600 vs 1600/1600` | `0` | `0` |
| `verify-m3` single-shot holdout (`50000..=50019`, `dd13839`, `/tmp/verify_m3_holdout_50000.json`) | `81.853s` | `all_passed: true` | `G3.1` `5040/5040` (`1.000000`); `G3.2` `648/660` (`0.981818`); `G3.3` `660/660` (`1.000000`); `G3.4` `729/740` (`0.985135`); `G3.5a` `1540/1540` (`1.000000`); `G3.5b` `740/740` (`1.000000`) | `G3.1` `max_err = 0.000e0` b/sym; `G3.2` `max_excise_dt = 427`, `raw_detours = 5181/5240`; `G3.3` `max_excise_dt = 513`, `raw_detours = 5240/5240`; `G3.5b` `min_split_tax = 7.4919` b/Split, `noisy_splits = 458/511`, `all_splits = 1678/1731`, `inj_excised = 1600/1600 vs 1600/1600` | `0` | `0` |

- **1,000-seed cliff telemetry (`20..1020`, `dev1000`):** `reserve_clipped_count = 4,446,789,247` (`2,659,471,805.41` bits), `reserve_mismatch_floored = 0`, `sprout_blocked_max_nodes = 0`, `candidate_undecidable = 12,503,095`, `candidate_list_truncated = 8,460,717`, `lag_horizon_bound = 250,000`, `ring_period_bound = 0`, `lag_deepening_steps = 4,319,219`, `sprout_budget_exhausted = 7,720,466`.
- **Fast dev & confirmation cliff telemetry:**
  - `0..20` (`dev20`): `reserve_clipped_count = 89,190,308` (`53,198,397.60` bits), `reserve_mismatch_floored = 0`, `sprout_blocked_max_nodes = 0`, `candidate_undecidable = 250,572`, `candidate_list_truncated = 169,366`, `lag_horizon_bound = 5,000`, `ring_period_bound = 0`, `lag_deepening_steps = 86,112`, `sprout_budget_exhausted = 154,881`.
  - `20060..=20079` (`confirm20060`): `reserve_clipped_count = 88,949,727` (`53,149,437.04` bits), `reserve_mismatch_floored = 0`, `sprout_blocked_max_nodes = 0`, `candidate_undecidable = 250,153`, `candidate_list_truncated = 169,167`, `lag_horizon_bound = 5,000`, `ring_period_bound = 0`, `lag_deepening_steps = 86,588`, `sprout_budget_exhausted = 154,521`.
- **Single-shot holdout cliff telemetry (`50000..=50019`, `dd138397e760eb30017a2077bd4a1a3983874a26`, `git_dirty = false`, `source_tree_fnv1a128 = 4e5ff20e46d6d87c8716d21cec458b8d`):** `reserve_clipped_count = 89,058,197` (`53,155,042.67` bits), `reserve_mismatch_floored = 0`, `sprout_blocked_max_nodes = 0`, `candidate_undecidable = 250,577`, `candidate_list_truncated = 169,051`, `lag_horizon_bound = 5,000`, `ring_period_bound = 0`, `lag_deepening_steps = 86,364`, `sprout_budget_exhausted = 154,300`.

### 12.4 Holdout Seed Governance & M3 v1 Superseded Closeout (`M3 v1 -> M3 v2`)

`M1` (`10000..=10019`) was not reused during M3; under M3 v1 (`dd13839`), each verification binary still permitted its own consumed range when `--holdout` was passed (`verify-m2a` for `30000..=30019`, `verify-m2b` for `40000..=40019`, `verify-m3` for `50000..=50019`), rather than enforcing true unconditional CLI closure across all commands (the retired M3 review). After `@chief-scientist` Stop Point `S3` authorization (`#60`), **`M3` v1 single-shot holdout (`50000..=50019`) executed once at `dd13839` (`81.853s`, `all_passed: true`, `excluded_cases = 0`, `sprout_blocked_max_nodes = 0`, `source_tree_fnv1a128 = 4e5ff20e46d6d87c8716d21cec458b8d`)** and received `S4` approval (`#64`), which was subsequently **superseded** following the `G2B.2` regression audit in the retired M3 review. Under M3 v2 remediation (§12.5, [Charter §2.8](foundational_research_charter_v3.md#28-statistical-policy-holdout-governance-and-retained-baseline-regression-gate)), consumed holdout ranges `10000..=10019` (`M1`), `30000..=30019` (`M2a`), `40000..=40019` (`M2b`), and `50000..=50019` (`M3 v1`) were permanently and unconditionally closed in CLI code (`HoldoutRangeSpec { allow_with_flag: false }`), and fresh 100-seed `M3 v2` holdout `50100..=50199` was subsequently executed once on clean `2b62140` (`S3` `#32` / `S4` `#36`) and permanently closed unconditionally (`allow_with_flag: false`) in the M3 v2 closeout commit across `verify-m2a`, `verify-m2b`, `explore-m3`, and `verify-m3`, while `M4a+` holdout `60000..=60099` remains locked and untouched.

### 12.5 Milestone 3 v2 Remediation & Statistical Policy Upgrade (Pre-S3 Evidence Packet)

> **S3/S4 Closeout Update (`2026-11-22`):** The Pre-S3 packet below was completed and verified on clean `HEAD = 2b62140ecc605b557ef526ae7cad622681478340` (`source_tree_fnv1a128 = bf22516980817549723269218e209d56`), passing `@chief-scientist` Stop Point `S3` (`#32`) single-shot holdout `50100..=50199` and Stop Point `S4` (`#36`) closeout with `50100..=50199` permanently closed (`allow_with_flag: false`) and evidence archived in `docs/evidence/m3_v2/`.

Following the retired M3 review, Milestone 3 v2 remediation implemented targeted mechanistic fixes and upgraded repository verification governance under [Charter §2.8](foundational_research_charter_v3.md#28-statistical-policy-holdout-governance-and-retained-baseline-regression-gate) (Decision Records `#47`–`#48`):

1. **Mechanistic fixes (`src/structural/growth.rs`, `src/structural/learner.rs`, `src/structural/surgery.rs`, `src/verify_m3/{mod,explore}.rs`):**
   - **`G2B.2` (`MV_AddEqz_k2_Comb`) acyclic-register priming:** M3 Phase 1 skipped replay warmup when settling delayed motifs, leaving newly sprouted combinational `Delay` pipelines in comparator candidates reading uninitialized `0` instead of prior frames. Added `new_acyclic_register_depth` and `prime_non_surviving_registers` in `evaluate_sprout_motif` and `evaluate_stage3_candidate_pre` when `eval_start_idx - start_idx < d_new && start_idx > 0` ([`tests/m2b_comparators.rs`](../tests/m2b_comparators.rs)).
   - **`G3.4` (`T2_MultiViewCoupled(k=2)(Combinational,eps=0.05)`) high-disagreement refutation & tap-`Split` accounting:** Tracked `disagree_diag` in `evaluate_stage3_candidate_pre` (rejecting candidates where `e_alt_diag > baseline.e_curr_by_port[out_idx] && 2 * disagree_diag >= eval_len`), added high-disagreement (`2 * disagreements >= win_after.disagree.len()`) wake comparator refutation in `process_wake_comparator_events`, and added `comparator_tap_split_nodes` / `primary_cone_nodes_without_comparator_taps` ([`tests/m3_free_riders.rs`](../tests/m3_free_riders.rs)) so `G3.4` (`run_spontaneous_case`) does not penalize pure registered-comparator fan-out `Split` taps as primary-cone bloat (while `G3.2`/`G3.3` retain unmodified `primary_cone_nodes`).
2. **Statistical Policy Option A + B + D (`src/stats.rs`, `src/lib.rs`, `src/verify_m2a/mod.rs`, `src/verify_m2b/mod.rs`, `src/verify_m3/{mod,explore}.rs`, `tests/cli.rs`):**
   - **Option A (Regime Stratification + Per-Task FWER):** `StratifiedGateAudit` separates `GateRegime::Clean` ($\varepsilon = 0.0$) and `GateRegime::Noisy` ($\varepsilon > 0.0$) tasks without cross-regime pooling (all 9 `G2A.4` tasks in `src/verify_m2a/mod.rs` execute at `G2A4_EPSILON = 0.05` under `GateRegime::Noisy`, target `0.85`). Each stratum enforces per-task exact binomial lower-tail & homogeneity tests at `alpha = 0.01 / M_stratum` (`total_successes == total_trials` when `target_rate >= 1.0`), per-seed `Clean` task conjunction (`seed_conjunction_multi_bank`), and macro Clopper–Pearson + homogeneity checks.
   - **Option B (100-Seed Confirmation & Holdout + Unconditional Closure):** `0..20` is fast dev smoke only; M3 v2 confirmation is `20100..=20199` (`100` seeds, `5 × 20`) and single-shot M3 v2 holdout is `50100..=50199` (`100` seeds, executed once at `2b62140` under `S3` `#32` / `S4` `#36` and now permanently closed unconditionally); M4a+ confirmation is `20200..=20299` (`100` seeds, `5 × 20`) and reserved holdout is `60000..=60099` (`100` seeds, unconditionally blocked until M4a `S3`). Consumed `10000..=10019`, `30000..=30019`, `40000..=40019`, `50000..=50019`, and `50100..=50199` are all permanently unconditionally closed (`allow_with_flag: false`).
   - **Option D (Finite Per-Task Tail Metrics):** Every per-task audit emits finite `tail_metrics` via `update_max_finite` / `update_min_finite`.
3. **Re-Review Remediation (`R1`–`R5` + `R1 Residual`, the retired M3 review `0d7b7e5` & `1f64fee`):**
   - **`0c6112a` (`gate-checker`: `R2`/`R4`):** Fixed live provenance binding (`git_commit_sha`, `git_dirty`, `source_tree_fnv1a128`, `cargo_lock_fnv1a128`) and statistical/tail validation in `scripts/check_retained_baselines.py` and `docs/retained_baselines.json`.
   - **`9b7bc80` (`stats-tails`: `R3`/`R4`):** Fixed clean-stratum pooled check in `src/stats.rs` and complete per-task `tail_metrics` emission across `src/verify_m2a/mod.rs`, `src/verify_m2b/mod.rs`, and `src/verify_m3/mod.rs`.
   - **`f55bb09` (`priming`: `R1`/`R5`):** Fixed surviving-state priming (`R1`) and priming budget metering (`R5`) in `src/structural/growth.rs`.
   - **`a826f02` & `2b62140` (`priming-3`: `R1 Residual` closure):** `a826f02` added `non_surviving_depends_on_surviving_cyclic`, `PrimingPlanOutcome::Unavailable`, `PrimingOutcome::Unavailable`, and unmetered `telemetry.candidate_undecidable += 1` deferral before `budget.try_consume` when `has_new_snapshot == false` and a non-surviving register traces back to a surviving cyclic node (`is_surviving_node && is_cyclic_node`), preserving `d_full` fallback for acyclic upstreams. `2b62140` restricted `non_surviving_depends_on_surviving_cyclic` DFS seeds strictly to the exact registers overwritten by `prime_non_surviving_registers_with_plan` (explicit non-surviving acyclic stateful nodes plus `TicksPerHop` non-protected/non-cyclic-internal hop-delayed edges), resolving the `a826f02` combinational over-seeding regression on `G3.5b` (`T2_CrossSum(k=1)(Combinational,eps=0.05):seed=11`) and earning `@chief-scientist` `R1 Residual` closure & `S3` authorization (`#32`).
4. **Clean-Commit `S3`/`S4` Verification Ledger (`HEAD = 2b62140ecc605b557ef526ae7cad622681478340`, `git_dirty = false`, `source_tree_fnv1a128 = bf22516980817549723269218e209d56`, `cargo_lock_fnv1a128 = 8d29cd24b1fbdbb743cf5c0b3933b22d`, raw JSONs archived in `docs/evidence/m3_v2/`):**

| Check / Suite | Seed Range | Status | Gate Results (`StratifiedGateAudit`, Worst Task, & Option D Tails) | `excluded_cases` | `sprout_blocked_max_nodes` |
| :--- | :--- | :--- | :--- | ---: | ---: |
| `scripts/check_retained_baselines.py --self-test` | Synthetic (`42` cases) | **PASS (`42/42`)** | Verifies valid `m2a`/`m2b` schemas, concentrated `MV_AddEqz_k2_Comb` `0/20` rejection, forged-pass detection, `--require-clean` provenance guards (`git_dirty`, `--fast`, `--skip-golden`), and strict non-finite/missing `tail_metrics` rejection. | — | — |
| `scripts/check_retained_baselines.py --require-clean` ([`verdict.json`](evidence/m3_v2/verdict.json), [`m2a_dev100.json`](evidence/m3_v2/m2a_dev100.json), [`m2b_dev100.json`](evidence/m3_v2/m2b_dev100.json)) | `golden_traces` + `20..120` (`5 × 20`) | **PASS (`passed = true`)** | `golden_traces` `1/1` bit-identical. **`verify-m2a` (`5297/5300`, 5 gates / 53 tasks):** `G2A.1` `1500/1500` (`max_target_kt_bits = [0.004067]`), `G2A.2` `1300/1300` (`max_target_kt_bits = [0.004067]`), `G2A.3` `1200/1200` (`max_lock_time_ratio = [1, 1.046875]`), `G2A.4` `899/900` (`noisy`, `worst = 0.9900`, `max_abs_kt_diff = [0, 0.428010]`), `G2A.5` `398/400` (`conj = 98/100`, `min_kt_advantage = [0, 1.085598]`). **`verify-m2b` (`4286/4300`, 5 gates / 43 tasks):** `G2B.1` `1500/1500` (`max_epistemic_mdl_err = [1.895e-15]`, `max_starved_energy = [3.167842]`, `max_target_kt_bits = [0.004067]`), **`G2B.2` `1000/1000`** (`min_settled_comp_ticks = [0, 4032]`), `G2B.3` `700/700` (`max_loopback_disagreement_ticks = [0, 104]`), `G2B.4` `886/900` (`noisy`, `worst = 0.8900`, `max_abs_kt_diff = [0, 1.878089]`), `G2B.5` `200/200` (`min_post_fault_kt_advantage = [0.942905, 1.794192]`). `0` missing/empty/non-finite `tail_metrics`. | `0` | `0` |
| `verify-m3` dev fast ([`verify_m3_dev20.json`](evidence/m3_v2/verify_m3_dev20.json)) | `0..20` (`20` seeds) | **PASS (`all_passed = true`)** | `t_20 = 79.741s`; **`9365/9380` across 6 gates / 469 tasks**: `G3.1` `5040/5040` (`clean 2880/2880`, `noisy 2160/2160`), `G3.2` `649/660` (`clean 440/440`, `noisy 209/220`), `G3.3` `660/660` (`clean 440/440`, `noisy 220/220`), `G3.4` `736/740` (`clean 520/520`, `noisy 216/220`), `G3.5a` `1540/1540` (`clean`), `G3.5b` `740/740` (`clean 520/520`, `noisy 220/220`). `0` missing/empty/non-finite `tail_metrics`. | `0` | `0` |
| `verify-m3` 5-bank dev ([`verify_m3_dev100.json`](evidence/m3_v2/verify_m3_dev100.json)) | `20..120` (`5 × 20` = `100` seeds) | **PASS (`all_passed = true`)** | **`46868/46900` across 6 gates / 469 tasks** (all Clean strata 100% & `conj = 100/100`): `G3.1` `25200/25200` (`max_epistemic_mdl_err = [0]`, `max_spec_transp_failed = [0]`, `max_post_edit_nodes = [8, 16]`), `G3.2` `3272/3300` (`clean 2200/2200`, `noisy 1072/1100`, `worst = 0.9500`, `max_excise_dt = [64, 339]`, `p95_excise_dt = [64, 96]`), `G3.3` `3300/3300` (`max_excise_dt = [129, 513]`, `p95_excise_dt = [129, 513]`), `G3.4` `3696/3700` (`clean 2600/2600`, `noisy 1096/1100`, `worst = 0.9700`, `max_t_lock = [256, 4829]`, `max_n_primary_8000 = [0, 6]`), `G3.5a` `7700/7700` (`min_vle_code_bits_advantage = [24.2316, 260.3298]`), `G3.5b` `3700/3700` (`min_split_tax_bits = [7.4919, 15.5843]`, `max_split_count_delta = [0, 4]`). `0` missing/empty/non-finite `tail_metrics`. | `0` | `0` |
| `verify-m3` 100-seed confirmation ([`verify_m3_conf100.json`](evidence/m3_v2/verify_m3_conf100.json)) | `20100..=20199` (`5 × 20` = `100` seeds) | **PASS (`all_passed = true`)** | **`46844/46900` across 6 gates / 469 tasks** (all Clean strata 100% & `conj = 100/100`): `G3.1` `25200/25200` (`clean 14400/14400`, `noisy 10800/10800`, `max_epistemic_mdl_err = [0]`, `max_spec_transp_failed = [0]`, `max_post_edit_nodes = [8, 16]`), `G3.2` `3255/3300` (`clean 2200/2200`, `noisy 1055/1100`, `worst = 0.8900`, `max_excise_dt = [64, 128]`, `p95_excise_dt = [64, 96]`), `G3.3` `3299/3300` (`clean 2200/2200`, `noisy 1099/1100`, `worst = 0.9900`, `max_excise_dt = [129, 513]`, `p95_excise_dt = [129, 513]`), `G3.4` `3690/3700` (`clean 2600/2600`, `noisy 1090/1100`, `worst = 0.9100`, `max_t_lock = [256, 4259]`, `max_n_primary_8000 = [0, 6]`), `G3.5a` `7700/7700` (`min_vle_code_bits_advantage = [24.2316, 260.3298]`), `G3.5b` `3700/3700` (`min_split_tax_bits = [7.4919, 15.5843]`, `max_split_count_delta = [0, 4]`). `0` missing/empty/non-finite `tail_metrics`. | `0` | `0` |
| `verify-m3` 50-bank dev (`20..1020`, optional) | `20..1020` (`50 × 20`) | **WAIVED (`#32`)** | Formally waived by `@chief-scientist` (`#32`) given `t_20 = 79.741s` (~66 min) and clean `20..120` + `20100..=20199` + retained baselines. | — | — |
| `verify-m3` 100-seed single-shot holdout ([`verify_m3_holdout_50100.json`](evidence/m3_v2/verify_m3_holdout_50100.json), `S3` `#32` / `S4` `#36`) | `50100..=50199` (`5 × 20` = `100` seeds, **permanently closed**) | **PASS (`all_passed = true`)** | `404.247s`; **`46842/46900` across 6 gates / 469 tasks** (`0` FWER/homogeneity failures; `469/469` finite Option D `tail_metrics`): `G3.1` Clean `14400/14400` (`conj = 100/100`) + Noisy `10800/10800` (`max_epistemic_mdl_err = 0.0`), `G3.2` Clean `2200/2200` (`cp99_lb = 0.997909`, `conj = 100/100`) + Noisy `1055/1100` (`cp99_lb = 0.942996`, `homog_p = 0.2552`, `min_task_homog_p = 0.1142`, worst task `88/100`, `max_excise_dt = [64, 339]`), `G3.3` Clean `2200/2200` (`conj = 100/100`) + Noisy `1100/1100` (`max_excise_dt = [129, 513]`), `G3.4` Clean `2600/2600` (`conj = 100/100`) + Noisy `1087/1100` (`cp99_lb = 0.978166`, `homog_p = 0.6505`, `min_task_homog_p = 0.5351`, worst task `91/100`, `max_t_lock = [256, 5194]`), `G3.5a` Clean `7700/7700` (`conj = 100/100`, `min_vle_code_bits_advantage = [24.2316, 260.3298]`), `G3.5b` Clean `2600/2600` (`conj = 100/100`) + Noisy `1100/1100` (`min_split_tax_bits = [7.4919, 15.5843]`, `max_split_count_delta = [0, 4]`). | `0` | `0` |

1. **Stop Point `S4` Closeout & Provenance Note:**
   - `@chief-scientist` formally approved Stop Point `S4` (`#36`) on clean `HEAD = 2b62140ecc605b557ef526ae7cad622681478340`, permanently closing `50100..=50199` unconditionally (`allow_with_flag: false` in `HoldoutRangeSpec`, rejected even with `--holdout`) and archiving all 7 clean-commit verification JSONs under `docs/evidence/m3_v2/`.
   - **Closeout Provenance:** Relative to `S3`/`S4`-verified `2b62140`, the `S4` closeout commit modifies ONLY the `50100..=50199` holdout guard flag (`src/verify_m3/mod.rs`), `VerifyM3` `--holdout` CLI help text (`src/lib.rs`), CLI holdout-refusal integration tests (`tests/cli.rs`), `docs/evidence/m3_v2/*.json`, and closeout documentation (`docs/research_record.md`, `docs/foundational_research_charter_v3.md`, `docs/parameter_inventory.md`, `docs/milestone_3_plan.md`, `README.md`), with **zero algorithmic, substrate, metric, or harness-gate changes** (and preserving `docs/milestone_3_review.md` and `.markdownlint-cli2.jsonc` untouched).

### 12.6 Retired M3 Gate Specifications and Exploratory Lessons

The table below preserves the original M3 gate definitions and feasibility arguments. The original pooled acceptance wording is historical: the final M3 verification uses the stratified per-task FWER, clean conjunction and tail policy in §12.5 and Charter §2.8. The original plan and review remain available in git history at `646628a`.

| Gate ID | Definition (metric, generator, width `k`, noise, equal-compute baseline) | Target Threshold (`P_target`, margin) | Feasibility Argument (Rule 9: theoretical bound or measured baseline) |
| :--- | :--- | :--- | :--- |
| `G3.1` | **Epistemic MDL Conservation Under Exogenous Detour Injection & Excision ($k \in \{1, 2\}$):** Across all 9 detour classes (`D0`–`D7b`) in both `Established` (`exogenous_injection_bits = L(C_post) - L(C_pre)`) and `Juvenile` (`exogenous_injection_bits = 0.0`, charged via `MarginalDebtLedger`) modes on `T1`, `T2`, and comparator-bearing `T2_MultiViewCoupled` circuits (with `juvenile_ticks = 0` / conservation configuration and `ReserveCap::Uncapped`, clean $\varepsilon = 0.0$ and noisy $\varepsilon = 0.05$), recording offline `epistemic_null_calibrated_mdl_delta_with_tax` frames strictly on ticks where `ArkheLearner::port_scored_last_observe(target_port)` is true (`is_driven && has_causal_ancestry`, matching `observe` on starved 1-node `Delay` probes, `#46`), total node energy change identically satisfies $\lvert (\Delta E(T) - \text{exogenous\_injection\_bits}) + \Delta L_{\text{null}}^{\text{epistemic}}(T) \rvert / T \le 10^{-11}$ bits/symbol, and `certify_circuit_against_spec` / window equivalence confirms every detour builder `build_detour_edit` is $100\%$ functionally transparent at $T_{\text{inj}}$ (except `D2` `onefire`'s single scheduled tick-0 impulse). | Exact `100%` (`1.0000`) across all conservation & detour-transparency cases; `0` `Excluded` cases | $\Delta E(T) \equiv -\Delta L_{\text{null}}^{\text{epistemic}}(T) + E_{\text{exog}}$ is an exact algebraic identity of normalized `apply_tick_core` shares and `MarginalDebtLedger::on_prune`; `M2b` `G2B.1(a)` measured `max_err = 1.895e-15 <= 1e-11` b/sym across `15,000 / 15,000` runs ([research_record.md §11.3](research_record.md#113-milestone-2b-dev--confirmation-attainment-summary-1e1c9dd8a7151060a011d956081fad50)). |
| `G3.2` | **Established Free-Rider Excision, Comparator Preservation & Primary-Cone Restoration ($k \in \{1, 2\}$, `InjectionMode::Established`):** On mature, locked circuits across the gated `T1` (`Alternating`, `PeriodicMotif([0,1,1])`, `PeriodicMotif([0,1,3])`, `CounterModN(3)`, `AffineLag1`), `T2` (`CrossSum`, `CrossMulEqz`), and `T2_MultiViewCoupled` panel (`Combinational` across all 9 classes `D0`–`D7b` at $\varepsilon \in \{0.0, 0.05\}$; `TicksPerHop` across parallel `D0`/`D7a` on all tasks and phase-aligned `D1`–`D7b` on autonomous `T1` tasks), injecting any detour in `Established` mode at $T_{\text{inj}} = t_{\text{settle}} + W_{\text{obs}}$ achieves within horizon $T_{\text{end}} = T_{\text{inj}} + 4 W_{\text{obs}}$ (`+1,024` ticks): (i) $100\%$ of injected detour nodes excised within $\Delta t_{\text{excise}} \le W_{\text{obs}} = 256$ ticks, (ii) final primary-cone size $N_{\text{primary}}(T_{\text{end}}) \le N_{\text{primary}}(T_{\text{inj}}^-)$ and $L(C_{\text{primary}}(T_{\text{end}})) \le L(C_{\text{primary}}(T_{\text{inj}}^-))$, (iii) on `T2_MultiViewCoupled`, registered `ComparatorSite` cones remain intact (`active_comparator_count >= 1`, `disagreement_prune_events == 0`), and (iv) post-excision clean accuracy (`#13`) requires exact `post_excision_clean_mismatches == 0`, paired `\|KT_learner - KT_oracle\| <= 0.005` b/sym, and raw unsubtracted finite-window KT $\le 0.0075$ b/sym over $N_{\text{post}} \ge 3 W_{\text{obs}} = 768$; noisy ($\varepsilon = 0.05$) paired KT difference vs. structural-class Bayes oracle $\le 0.040$ b/sym. **Bank aggregation (`#46`):** Aggregated at the independent `(task, wire_delay, eps, seed)` settled trajectory unit (`n = 660` units per 20-seed bank; `Excluded` if any detour binds `sprout_blocked_max_nodes > 0`, else `Passed` iff all applicable detour classes `D0`–`D7b` pass), with raw per-detour counts preserved in `aux_metric_summary` / `pooled_metric_name`. | Pooled 99% 1-sided Clopper–Pearson LB $\ge 0.90$ + 1% homogeneity ($p \ge 0.01$); `0` `Excluded` cases | Under `M3-E1` + `M3-E2`, an `Established` (`juvenile_ticks == 0`) detour refills `DiagnosticBuffer` in $W_{\text{diag}} = 64$ ticks and executes Stage-1 excision at the next `sprout_interval = 32` check ($\Delta t_{\text{excise}} \le W_{\text{diag}} + I_{\text{sprout}} = 96$ ticks, or $\le 192 < W_{\text{obs}} = 256$ ticks for a 2-step cascade), independent of channel noise $\varepsilon = 0.05$ because $v(t) \equiv x(t)$ holds identically on corrupted and clean input symbols alike. |
| `G3.3` | **Juvenile Free-Rider Excision & Non-Collapse Recovery Cost ($k \in \{1, 2\}$, `InjectionMode::Juvenile`):** Across the same panel with detours injected in `Juvenile` mode (charged marginal $\Delta L_{\text{inj}}$ via `MarginalDebtLedger::on_sprout`), within $T_{\text{end}} = T_{\text{inj}} + 4 W_{\text{obs}}$: (i) $100\%$ of injected detour nodes (`D0`–`D7b`, whether excised via graduation starvation bypass at $t_{\text{juv}}$ or via post-graduation Stage-1 `M3-E1`/`M3-E2`) are removed within $\Delta t_{\text{excise}} \le 3 W_{\text{obs}} = 768$ ticks, (ii) **zero destructive primary-cone collapses** occur (`0` post-injection clean mismatches across $[T_{\text{inj}}, T_{\text{end}})$ on transparent classes `D0`, `D1`, `D3`–`D7b`; on `D2OneFire` (`#13`), `post_inj_clean_mismatches <= 1 && post_excision_clean_mismatches == 0` with raw transient KT excess $\le 0.020$ b/sym — closed-form 1-error Dir(1/2) bound `0.01798 <= 0.020` — and paired `\|KT_learner - KT_oracle\| <= 0.015` b/sym over $4 W_{\text{obs}} = 1024$ ticks), and (iii) $N_{\text{primary}}(T_{\text{end}}) \le N_{\text{primary}}(T_{\text{inj}}^-)$ and $L(C_{\text{primary}}(T_{\text{end}})) \le L(C_{\text{primary}}(T_{\text{inj}}^-))$. **Bank aggregation (`#46`):** Independent `(task, wire_delay, eps, seed)` trajectory unit (`n = 660` per 20-seed bank) with raw per-detour counts preserved in `aux_metric_summary` / `pooled_metric_name`. | Pooled 99% 1-sided Clopper–Pearson LB $\ge 0.90$ + 1% homogeneity ($p \ge 0.01$); `0` `Excluded` cases | Under `M2B-B1`/`M2B-C1`, juvenile maturation is bounded by $t_{\text{cone}} \le 2 W_{\text{obs}} = 512$ ticks; zero-share detours (`D1`–`D4`, `D7b` gate) starve at graduation ($t \le 512$) and are bypassed cleanly by `M3-E1` without disconnecting `BoundaryOut(p)`, while positive-share `D6` involutions that graduate enter mature Stage-1 `M3-E2` within $+W_{\text{diag}} + I_{\text{sprout}} = +96$ ticks ($\le 512 + 96 = 608 < 3 W_{\text{obs}} = 768$ ticks). |
| `G3.4` | **Spontaneous Long-Horizon Size Discipline & Certified Exact-Minimum Yardstick ($k \in \{1, 2\}$, $T \in \{2000, 8000\}$):** On endogenous un-injected runs across `T1` (`Constant`, `Alternating`, `PeriodicMotif([0,1,1])`, `PeriodicMotif([0,1,3])`, `CounterModN(3)`, `AffineLag1`), `T2` (`DelayedAffine`, `CrossSum`, `CrossMulEqz`), and `T2_MultiViewCoupled` (`Combinational` & causal `TicksPerHop`, clean $\varepsilon = 0.0$ & noisy $\varepsilon = 0.05$) evaluated at standard $T = 2000$ and extended $T = 8000$ ($4\times$ M2 horizon, `TickBudget = 800,000` scaled proportionally with $T$): (i) **zero post-settlement growth (`#13`, `#35`):** $N_{\text{primary}}(8000) \le N_{\text{primary}}(2000) \le N_{\text{primary}}(t_{\text{settle}})$, where $t_{\text{settle}} \le 2000$ is the first tick `is_developmental_settled()` holds across all ports (including post-`t_lock` Stage-3 `Split` taps) and per-port `max_comparators_on_any_port <= 1`, (ii) `SproutBlockedMaxNodes == 0` across all runs, (iii) on `NoisyConstant(0)` and `NoisyConstant(2^k - 1)` ($\varepsilon = 0.05$), $N_{\text{primary}}(8000) == 0$, and (iv) on all exact-synthesis certifiable tasks ($N_{\min} \le 4, K \le 2$ via `exact_synthesize_certified_circuit`), locked primary-cone size satisfies $N_{\text{primary}}(8000) \le N_{\min} + 4$ and $N_{\text{primary}}(8000) \le 3 \cdot \max(1, N_{\min,\text{eff}})$ where $N_{\min,\text{eff}} = \max(N_{\min}, 3)$ on noisy ($\varepsilon > 0$) periodic non-constant `T1` tasks (3-node autonomous ring Bayes minimum) and $N_{\min}$ otherwise (`#13`). | Pooled 99% 1-sided Clopper–Pearson LB $\ge 0.90$ + 1% homogeneity ($p \ge 0.01$); `0` `Excluded` cases | Once `e_curr == 0`, Stage-2 corrector sprouting halts (`is_port_zero_error(p)`) and Stage-3 redundancy sprouting adds at most `Split` comparator taps before post-Stage-3 `t_settle`; the original exploratory baselines show $N_{\text{primary}}(t_{\text{lock}}) \in [0, 6]$ against $N_{\min} \in [0, 4]$ ($N_{\text{primary}} - N_{\min} \in [0, 3] \le 4$). |
| `G3.5` | **Charter §8 Verified Local Excision & `SPLIT`-Tax Ablation ($k \in \{1, 2\}$):** (a) **Local-excision ablation (`ExcisionMode::VerifiedLocalExcision` vs. `ExcisionMode::BaselineD1b1`):** Across the `Established` detour panel (`D1`–`D6`, `D7b`), `VerifiedLocalExcision` achieves $0$ surviving detour nodes and strictly lower final circuit description length $L(C_{\text{final}}^{\text{VLE}}) < L(C_{\text{final}}^{\text{Base}})$ on $\ge 95\%$ of runs where `BaselineD1b1` retains persistent bloat or suffers primary-cone collapse; (b) **`SPLIT`-tax pricing & anti-bloat ablation (`split_code_tax = true` vs. `split_code_tax = false`, `#13`, `#46`):** Verify analytically and empirically across the `T1`/`T2` panel that `split_code_tax = true` prices every `Primitive::Split` fan-out by $\Delta L_{\text{split}} = \log_2(\text{PALETTE\_SIZE}) + \log_2(N_{\text{src}} N_{\text{dst}}) \ge 6.58$ bits while maintaining `100%` clean ($\varepsilon = 0.0$) task solvability (`clean_locked_8000` required whenever `epsilon == 0.0`), non-higher (`<=`) final `Split` count on **every individual clean task** AND pooled across **noisy ($\varepsilon = 0.05$) and overall spontaneous runs**, and non-lower detour excision rate on the paired injection ablation. | `G3.5(a)` pooled 99% 1-sided CP LB $\ge 0.95$ + 1% homogeneity; `G3.5(b)` exact `100%` positive `Split` MDL tax, clean solvability & `Split` non-regression | the original M3 analysis showed `BaselineD1b1` retains $100\%$ of `Established` `D1`–`D6`/`D7b` detours (`\Delta L_{\text{inj}} \in [13.5, 75.0]` bits) while `M3-E1` + `M3-E2` excises $100\%$ of them; [Research Record §2.5](research_record.md#25-redesigned-m1a--milestone-1-attainment-sign-offs-2026-11-072026-11-08) measured `+7.76` to `+17.85` bits `split_code_tax` separation across `M1b` tasks with `4/4` convergence. |

The paired `0..20` exploration found established verified excision succeeded on `1980/1980` cases versus `280/1980` for `BaselineD1b1` (1,700 retained bloat); juvenile verified excision succeeded on `1980/1980` versus `600/1980`, with 1,300 baseline collapses. With `split_code_tax`, injection excision was `1600/1600` versus `1400/1600` without it. These are exploratory measurements, not new acceptance criteria. M3 demonstrated bounded primary-cone overhead rather than exact-minimum convergence.

For M4a, preserve live-register replay and meter priming and all wake/sleep work; retain per-task stratification and inspect tails, rather than relying on pooled success or golden traces alone. Historical M3 telemetry included nonzero lag-bound hits and substantial budget exhaustion; monitor these without masking them.
