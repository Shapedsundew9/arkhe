# Arkhe Foundational Research Record

> **Companion to:** [Foundational Research Charter v3](foundational_research_charter_v3.md)
> **Scope:** Persistent historical ledger of empirical measurements, verification provenance, kill-criterion diagnoses, discovered mathematical traps, rejected design alternatives, parameter origins, and deferred Milestone 2–5 questions recovered from git history (`FOUNDATION` through `m1-final-lattice` and pre-M2 cleanup archival of `milestone_1b_plan.md` / `milestone_1b_review.md`).
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
| `2ed11fd` | 2026-11-08 | Added [coordinate_free_scaling_plan.md](coordinate_free_scaling_plan.md). |
| `ddd9056` (`origin/main`) | 2026-11-08 | Deleted `doc/milestone_1_plan.md` and `doc/milestone_1b_plan.md`; key schemas, sign-offs, and M1b findings were consolidated into Sections 2.5, 3.3, and 6.1 below. |
| `2d009b0` (`tag: m1-final-lattice`) | 2026-11-08 | Phase 0 anchor: golden-trace harness, cliff telemetry, and regime-shift baseline before coordinate-free scaling refactor (also final git anchor for retired spatial ablations and `doc/milestone_1b_review.md`). |
| Pre-M2 doc cleanup | 2026-11-13 | Folded unique empirical findings, gate counts, and §10 retirement notes from `doc/milestone_1b_plan.md` and `doc/milestone_1b_review.md` into this record and removed both superseded M1b files from `doc/`. |

### 1.2 Archival Consolidation of Superseded Milestone 1 Plans & Reviews

To prevent agents from confusing superseded blueprints (`milestone_1_plan.md`, `milestone_1b_plan.md`) or pre-refactor audits (`milestone_1b_review.md`, anchored against `9544e98` and deleted `src/structural/lattice.rs`) with active specifications, all durable specs and measurements from those files live here (Sections 2.5, 3.2, 3.3, and 6.1):

- **`milestone_1_plan.md` §4.0 (lines 334–352):** The seven post-M1a interface decisions and open questions (`idle nodes / switching vs pruning`, `juvenile_ticks & E_init`, `marginal ΔL_sprout`, `port M reset`, `max_energy_reserve`, `RepeatLast matched estimator`, `unowned circuit bits`). Reconstructed in Section 6.1.
- **`milestone_1_plan.md` §5.2 (lines 444–481):** The pre-registered M1 `ExperimentConfig` TOML block (`grid_rows = 8`, `grid_cols = 8`, `trace_discount = 0.90`, `initial_energy = 4.0`, `juvenile_ticks = 128`, `max_energy_reserve = 25.0`, `diagnostic_window = 64`, `sprout_interval = 32`, `mi_threshold_bits = 0.05`, `min_match_gain = 4`, `max_nodes = 32`, `horizon_ticks = 2_000`, `max_eval_ticks = 50_000`). Reproduced verbatim in Section 6.1.
- **`milestone_1b_plan.md` §9–§10 & `milestone_1b_review.md`:** Holdout (`e29014a`) and 100-bank hardening (`20..2020`) per-gate counts, DR #29/#30 `LocalVonNeumann` vs. `WellMixed` benchmark evidence, M1b defect diagnoses, and the `m1-final-lattice` archival anchor. Preserved in Sections 2.5 and 3.3.

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
> 2. **Noisy $C_\mu$ in G2:** M0 closeout left noisy `c_mu_bits` unasserted in G2. Inspection of current [src/verify.rs](../src/verify.rs) confirms `G2` now asserts `(em_noisy.c_mu_bits - exp_c_mu).abs() < 1e-8` whenever `msp_sync_status` is `BoundedQuantizedEstimate` or `ExactZeroComplexity` (bounded by the `max_mixed_states = 4096` / $10^{-12}$ quantization threshold).

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
4. **Width & alphabet traps (`M4/M5` preview):** Plug-in Miller–Madow scaled with theoretical $2^k$ rather than observed empirical support, subtracting `2.874` bits at $k = 8, W_{\text{diag}} = 64$ even for binary features; `1u64 << width.bits()` overflowed at $k = 64$. Both immediate bugs were fixed (`empirical support` + `width.mask()`), while fixed-$W_{\text{diag}}$ undersampling at larger joint alphabets remains deferred to M5 ([coordinate_free_scaling_plan.md](coordinate_free_scaling_plan.md) §2.6).
5. **Pre-M2 readiness qualification:** Although `milestone_1b_review.md` §5.2 wrote *"Unconditionally ready"* after the `2026-11-08` fixes, the same paragraph noted that M2's cross-channel comparators (`EQZ(SUB(a, b))`) and redundancy pathways remain unfunded under Charter §3.4 / §3.1.4 and conflict with `D1b.1` zero-sensitivity pruning—an open theoretical blocker tracked as Decision **Q5** in [pre_milestone_2_readiness_plan.md](pre_milestone_2_readiness_plan.md).

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

---

## 5. Parameter Provenance Ledger (M0–M1b)

*Classification follows the project discipline: **Derived** (forced by information theory / exact algebra / environment), **Pre-registered hypothesis**, **Heuristic / Ungrounded** (introduced without principled scaling or sweep—flagged for [coordinate_free_scaling_plan.md](coordinate_free_scaling_plan.md) and [pre_milestone_2_readiness_plan.md](pre_milestone_2_readiness_plan.md)), or **Removed**.*

| Parameter | Value | Origin & Justification History | Classification |
| :--- | :--- | :--- | :--- |
| `c_base`, `c_split`, `c_wire`, `tau_mdl` | `0.020`, `0.035`, `0.004`, `400` | Heuristic pre-redesign rent constants (`removed_lines_milestone_1_plan.md`); conflicted across $T$ with $L(C)$ | **Removed** (in M1a `D3`) |
| `KT prior` ($\tfrac12$) | `0.5` | Exact Krichevsky–Trofimov prior inherited from `LaplaceFrequency` null model (`milestone_0_plan.md`, `m1a-redesign-plan.md` `D2`) | **Derived** |
| `CTW` verification horizon / depth | `T = 20_000`, `depth = 9` | Sized analytically so $O(\lvert \mathcal{S}\rvert \log T / kT)$ WST redundancy drops from `0.044` ($T=4000$) to $\le 0.025$ b/sym on $k=2$ `AffineLag1` (`lessons_learned.md` §4.5) | **Derived / Sized for WST** |
| `max_mixed_states` / merge eps | `4096` / `1e-12` | M0 non-unifilar ε-machine MSP bounding guard (`milestone_0_plan.md` §12) | **Compute guardrail** |
| `trace_discount` ($\gamma$) | `0.90` | Pre-registered M1a constant; reused unchanged for `D4` activity recency decay; **never sensitivity-swept** (`m1a-redesign-plan.md`) | **Pre-registered / Unswept** |
| `initial_energy` ($E_{\text{init}}$) | `4.0` bits | Fixed seed endowment in `milestone_1_plan.md` §5.2; cancels out of M1a $\Delta E$ (`juvenile_ticks = 0`), sets M1b sprout deficit $\Delta L_i - 4.0$ | **Heuristic / Ungrounded** |
| `juvenile_ticks` ($T_{\text{juvenile}}$) | `128` ticks | Chosen in `milestone_1_plan.md` to exceed M0's `~100`-tick noisy oscillator crossover (M0 actually measured `130–180`; M1b later expanded `obs_capacity` to `256` for `152.9`-tick `NoisyAlternating`) | **Heuristic (M0-informed, undershoots at scale)** |
| `max_energy_reserve` ($E_{\max}$) | `Some(25.0)` bits | Locked in `milestone_1_plan.md` §5.2 to cap mature reserves after regime shifts (`D1b.5`); **lower than M1b $\Delta L_{\text{sprout}} \approx 17\text{–}28.5$ bits**, forcing `D1b.2` unclipped juvenile escrow workaround; replacement (Option B vs. alternatives) is pending Q1 / B1 in [pre_milestone_2_readiness_plan.md](pre_milestone_2_readiness_plan.md) | **Heuristic / Superseded (pending Q1)** |
| `diagnostic_window` ($W_{\text{diag}}$), `sprout_interval` | `64`, `32` ticks | Locked in `milestone_1_plan.md` §5.2 without sensitivity sweep (`milestone_1b_plan.md` §5.1 notes `obs_window = 2 * W_diag = 128`, later expanded to `256` to cover `152.9`-tick crossover) | **Heuristic / Unswept** |
| `mi_threshold_bits` ($\theta_{\text{MI}}$), `min_match_gain` ($\Delta_{\min}$) | `0.05` bits, `4` symbols | Locked in `milestone_1_plan.md` §5.2 as M1b noise-floor halting heuristic without derivation across $T$ or $k$ | **Heuristic / Unswept** |
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

1. **Redundancy funding paradox (`counterpoint_response.md` §2.1 — still unresolved before M2):** Error-only corrector sprouting (`D1b.1` / `G1B`) removes or declines redundant derivations. Yet Charter Hypothesis **H3 (Milestone 2)** relies on growing *alternative derivations* that meet at `Comparator` (`EQZ(SUB(a, b))` / `XNOR`) sites! As noted in `counterpoint_response.md`, M2 must specify when and why a **second, independent pathway predicting an already-covered target** earns credit or avoids idle pruning—and account for correlation when both pathways share an input source.
2. **Switching codes vs. ad-hoc reserve caps (`m1a-redesign-plan.md` Open Q #1):** Replacing `D1b.1` idle heuristics and `Some(25.0)` caps with information-theoretic switching codes (`Herbster & Warmuth 1998`; `van Erven et al. 2012`) was deferred from M1b and remains an open principled candidate for regime shifts in M2+.
3. **Common compute & loss across heterogeneous baselines (`milestone_0_review.md` §4, `milestone_0_plan.md` §8.3):** `src/baselines/rnn.rs` (`RNN/LSTM` BPTT) was deferred to **M2**. Both adding it and running `M2` baseline comparisons or `M4` sleep-vs-wake equal-compute ablations require bringing `CTW`, `ReedsSloane`, neural baselines, and `ArkheLearner` onto an explicitly declared compute and loss protocol.
4. **Sleep replay phase integrity (`milestone_0_lessons_learned.md` §5.3):** M4a offline sleep loopback ($\hat{A}(t+1) \to A(t+1)$) must capture and step from live internal `Delay` register states (or full attractor cycles), never resetting registers to static $t = 0$ `init` values.
5. **Deferred Charter v1 critique mechanisms for M2–M6 (`doc/foundational_research_charter_critique.md` §3.4, §3.6, §4, §5.5):**
   - *Explicit uncertainty / abstention* (§3.4) to distinguish low confidence from channel noise (`rate` / `vote` / `abstain` coding).
   - *Macro boundary detection & retirement* (§3.6): small cut/interface, Markov-blanket test, Hoel causal-emergence effective information, usage decay + palette MDL cost in M5.
   - *Logic synthesis & open-endedness tooling* (§4, §5.5): sequential equivalence checking (SAT/BDD) for sleep consolidation proofs and Bedau–Packard activity statistics for open-ended novelty measurement.
