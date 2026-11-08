# Milestone 1b Review: Residual Neurogenesis and Downstream Readiness

**Review date:** 2026-11-08  
**Scope:** Working-tree implementation at commit `9544e98` (`HEAD`), evaluated against the [Foundational Research Charter v3](foundational_research_charter_v3.md), the [Milestone 1b plan](milestone_1b_plan.md), and the [master Milestone 1 plan](milestone_1_plan.md).

## Executive judgment

### 1. Did Milestone 1b meet the Charter's intent?

**Substantively yes on the primary `WellMixed` + `Combinational` developmental regime across $k \in \{1, 2\}$, with material qualifications on verification coverage, noisy non-zero constant-stream halting, `TicksPerHop` pipeline ancestry, and holdout provenance.**

The implementation delivers a genuine, gradient-free, online demonstration of Charter Hypothesis **H2** ([Charter §1.2, §7](foundational_research_charter_v3.md#7-milestones)): starting from an empty graph, [`ArkheLearner`](../src/structural/learner.rs#L65) diagnoses boundary prediction error over a sliding window, recruits causal $z^{-\tau}$ delay taps, sprouts a coarse first-stage predictor, diagnoses the resulting residual via Miller–Madow-corrected mutual information, stacks an additive or subtractive second-stage finite-ring corrector over $\mathbb{Z}_{2^k}$ to drive post-transient clean error to zero, and—under substitution noise ($\varepsilon = 0.05$) on `T1_Alternating` and `T1_PeriodicMotif(p=3)`—closes phase-aligned autonomous `Delay` rings that beat reactive lag-echo local minima and halt at the Bayes noise floor. Throughout, [`src/substrate/`](../src/substrate/mod.rs) remains strictly integer-only, deterministic, and dependency-minimal.

However, the executable acceptance suite in [`src/verify_m1/m1b.rs`](../src/verify_m1/m1b.rs) enforces a narrower matrix than pre-registered in [§6 of `milestone_1b_plan.md`](milestone_1b_plan.md#6-pre-registered-m1b-acceptance-gates-g1b1g1b4). Independent review probes on development seeds (`0..19`, leaving holdout seeds `10000..=10019` untouched) confirmed that several omitted cases pass cleanly (noiseless `T1_AffineLag1` at $k = 1$; noisy `T1_Constant(0)`; noisy `T1_PeriodicMotif(p=3)` across $20/20$ seeds at both $k = 1$ and $k = 2$), **but also exposed three functional defects and one provenance gap that the narrowed gates missed:**

1. **`TicksPerHop` runtime panic (`MissingAncestry { port: 0 }`):** Multi-epoch sprouting in `TicksPerHop` mode panics at [`learner.rs:395`](../src/structural/learner.rs#L395) on `T1_PeriodicMotif([0, 1, 1])` ($k = 1$) when a newly inserted output-edge pipeline register emits a prediction with non-zero port flux before upstream node eligibility has propagated into that register.
2. **Noisy non-zero constant-stream thrashing:** Because 1-op Epoch-1 candidates bypass the $\Delta\text{Flux} > 0$ and juvenile survival checks ([`growth.rs:1749`](../src/structural/growth.rs#L1749)), `T1_Constant(1)` ($k = 1$) and `T1_Constant(3)` ($k = 2$) at $\varepsilon = 0.05$ enter a deterministic sprout–starve–prune loop (`11` sprouts, `10` prunes over `2,000` ticks; `0/20` seeds meet the `0.025` bits/sym regret bound), whereas `T1_Constant(0)` passes `20/20` only because an empty circuit already predicts `0`.
3. **Unmet sparse rare-event gate claim (`T1_PeriodicMotif([0, 0, 0, 0, 0, 1])`):** Registered in `G1B.4a` Axis 1 as locking to `>= 0.99` clean match under `CalibratedPointForecast`, this task was omitted from [`verify_gate_g1b4`](../src/verify_m1/m1b.rs#L735) and actually stalls at `0` nodes (`0.8331 = 5/6` match) because reactive taps stop at $\tau \le 3$ and [`propose_autonomous_delay_ring`](../src/structural/growth.rs#L1952) locks itself out on empty circuits (`!editable.compiled().uses_boundary_inputs()`).
4. **Post-holdout source mutation (`9544e98` after `e29014a`):** Both plans record single-shot holdout sign-off on `e29014a` (`source_tree_fnv1a128 = 564de6fbdb0b8cbd898b91fc6b4a408a`), after which commit `9544e98` modified [`growth.rs`](../src/structural/growth.rs) and [`learner.rs`](../src/structural/learner.rs) (`source_tree_fnv1a128 = ae9e3c4d577d071163c1e0855f3342a0`).

### 2. Does it provide a sound basis for subsequent milestones?

**Yes at the substrate, surgery, and metabolic-economy layers; conditionally at the learner and growth layers, provided specific single-channel and replay shortcuts are refactored before M2 and M4.**

[`EditableLattice`](../src/structural/surgery.rs#L67), [`EconomyState`](../src/structural/economy.rs#L148), [`PointForecastScorer`](../src/metrics/point_forecast.rs#L22), and the zero-float [`substrate`](../src/substrate/mod.rs) form a clean, well-tested foundation. Before starting **M2** (multi-channel redundancy and cross-channel bridges), however, four seams in [`learner.rs`](../src/structural/learner.rs) and [`growth.rs`](../src/structural/growth.rs) must be addressed: whole-graph replacement during autonomous ring closure (which wipes other channels), perimeter anchor collisions for $K \ge 4$, global predictor-count resets on single-port pruning, and the structural tension between `D1b.1` zero-sensitivity pruning and M2's co-existing redundant pathways / internal `EQZ(SUB(a, b))` comparators. Additional targeted fixes are identified below for **M4** (state-snapshot candidate replay, internal comparator null models, MDL delta sign and palette accounting) and **M5** ($2^k$ Miller–Madow penalty scaling and `k = 64` bit-shift safety).

### Validation summary

All repository checks and development-seed verification suites were executed against `HEAD` (`9544e98923aee33d44ed853fc9f1159da0a97df1`, clean tree; holdout seeds `10000..=10019` were not executed):

| Check | Result |
| :--- | :--- |
| `cargo fmt --check` | Passed |
| `cargo clippy --locked --all-targets --all-features -- -D warnings` | Passed (0 warnings) |
| `cargo test --locked` | Passed: 97 tests (67 unit + 30 integration across 5 test binaries) |
| `cargo run --locked --release -- verify-m0 --seed-offset 0 --seeds 20` | Passed: G1–G4 all `true` |
| `cargo run --locked --release -- verify-m1 --seed-offset 0 --seeds 20` | Passed: M1a (`G1A.1`–`G1A.4`) and M1b (`G1B.1`–`G1B.4`) all `true`, `stage_gate_aborted: false` |
| Independent `arkhe run` development-seed probes (`seeds 0..19`) | Confirmed omitted `G1B.2` wins on `AffineLag1(k=1)` and noisy `PeriodicMotif(p=3)`, and isolated Findings 1–3 and 5 |

---

## 1. Assessment against Charter v3 intent

### 1.1 Charter commitments vs. delivered capabilities

| Charter v3 requirement | Implementation status | Assessment |
| :--- | :--- | :--- |
| **§2.1–§2.5** Pure integer $\mathbb{Z}_{2^k}$ substrate, explicit `SPLIT` fan-out $\le 2$, synchronous latching, zero floats in substrate | Preserved intact in [`src/substrate/`](../src/substrate/mod.rs); every structural edit lowers and re-verifies invariants via `Circuit::compile()` | **Fully met.** |
| **§3.1 / §7 (H2)** Residual-driven first-to-second neuron neurogenesis over $k = 1$ and $k = 2$ without gradients | [`diagnose_residual`](../src/structural/growth.rs#L126), [`generate_candidate_motifs`](../src/structural/growth.rs#L700), and [`stack_output_adder`](../src/structural/lattice.rs#L1266) / [`stack_output_subtractor`](../src/structural/lattice.rs#L1279) | **Met on structured T1/T3b tasks.** Monotonic residual drop $H(R_2) = 0 < H(R_1) < H(R_0)$ holds across `20/20` dev seeds on all four registered 2-stage tasks. |
| **§3.1** Causal temporal recruitment ($z^{-1}$ delay taps) when combinational features lack MI | [`SignalPool::build_features`](../src/structural/growth.rs#L621) exposes `LagIn`, `LagYhat`, and `LagRes` for $\tau \in 1..=3$, materialized as `Delay` chains | **Met for reactive horizons $\le 4$ ticks.** Deeper periodic memory relies on autonomous ring closure (`p <= 6`). |
| **§3.1.1** Planar wiring congestion treated as diagnostic ablation; `WellMixed` + `split_code_tax` default | [`WiringTopology::WellMixed`](../src/structural/lattice.rs#L156) default with [`split_code_tax`](../src/metrics/mdl.rs#L135); `LocalVonNeumann`/`LocalMoore` retained | **Well-motivated and empirically validated** (see §1.2). |
| **§3.2 / §3.4** Calibrated metabolic reward, juvenile protection, maturity debt, starvation pruning | [`PointForecastScorer`](../src/metrics/point_forecast.rs#L80), [`EconomyState::apply_tick`](../src/structural/economy.rs#L386), [`MarginalDebtLedger`](../src/structural/surgery.rs#L632) | **Sound in M1a/M1b online operation**, except where bypassed in Epoch 1 (Finding 2) and on unprimed `TicksPerHop` output edges (Finding 1). |
| **§7 M1b** Noise-floor halting under substitution noise ($\varepsilon > 0$) without memorizing noise | Miller–Madow penalty + `min_match_gain` + autonomous ring template synthesis ([`propose_autonomous_delay_ring`](../src/structural/growth.rs#L1943)) | **Met on `T1_Alternating`, `T1_Constant(0)`, and `T1_PeriodicMotif(p=3)`**, **not met** on noisy non-zero constants `T1_Constant(c != 0)` (Finding 2). |
| **§8 / §9** Pre-registered gates, ablations, deterministic separate-process replay, stage-gated M1a $\to$ M1b | [`src/verify_m1/m1b.rs`](../src/verify_m1/m1b.rs) and [`Commands::VerifyM1`](../src/lib.rs#L267) | **Partially met.** Stage gate and core determinism work, but `G1B.2` and `G1B.4` omit registered cases and run 1-seed / short-horizon checks (Finding 3). |

### 1.2 Architectural and mathematical strengths

1. **Principled resolution of the M1a-to-M1b metabolic interface (`D1b.1`–`D1b.7`).**
   - **Stratified `RepeatLast` point-forecast reward ([`point_forecast.rs:80-166`](../src/metrics/point_forecast.rs#L80)):** Conditioning both the `RepeatLast` null baseline and the sequential Krichevsky–Trofimov predictor hit-rate estimator $\hat{q}_t(\text{hit} \mid s_{t-1})$ on $s_{t-1}$ cleanly eliminates Jensen cross-context pooling bias while preserving exact zero flux (`0.0` bits/tick) whenever $\hat{y}_t = s_{t-1}$ and preserving Ville's martingale identity $\mathbb{E}_{y \sim P_{\text{null}}}[2^{r_t}] = 1$ (verified directly in unit tests).
   - **Unclipped juvenile escrow ([`economy.rs:447-454`](../src/structural/economy.rs#L447)):** Allowing juvenile nodes (`juvenile_ticks > 0`) to accumulate unclipped flux above `max_energy_reserve = Some(25.0)` until maturity debt $\Delta L_v$ is settled prevents multi-node motifs (`L > 25` bits) from starving deterministically at maturity on tick `128`.
   - **Phase-preserving live surgery ([`surgery.rs:174-226`](../src/structural/surgery.rs#L174)):** `EditableLattice::recompile_preserving_state` remaps both `ScalarState` registers (`Delay` latches and `ShiftChain` stages) and `EconomyState` node/register eligibility traces atomically. Gate `G1B.3b` ([`m1b.rs:571-730`](../src/verify_m1/m1b.rs#L571)) stresses an active 3-stage oscillator through `500` live sprout/excise transactions across $k \in \{1, 2\}$ and `{Combinational, TicksPerHop}` with **zero phase slips**.
2. **Empirical confirmation of the Charter §3.1.1 topology pivot.**
   Review probes across all four 2-stage tasks (`master_seed = 0`, `horizon = 2000`) confirm why decoupling graph morphogenesis (`WellMixed` + `split_code_tax`) from 2D 4-neighbor routing (`LocalVonNeumann`) was the right scientific call:

   | Task | $k$ | `WellMixed` (`tax=true`) match / nodes / splits / $L(C)$ | `WellMixed` (`tax=false`) $L(C)$ | `LocalVonNeumann` (`tax=true`) match / nodes / splits / $L(C)$ |
   | :--- | :---: | :---: | :---: | :---: |
   | `T1_PeriodicMotif([0, 1, 1])` | 1 | `1.0000` / 4 / 1 / `52.36b` | `44.60b` | `0.6175` / 2 / 1 / `34.17b` |
   | `T1_PeriodicMotif([0, 1, 3])` | 2 | `1.0000` / 5 / 1 / `65.23b` | `57.03b` | `0.8600` / 9 / 6 / `125.26b` |
   | `T1_AffineLag1(1, 1, 0)` | 2 | `1.0000` / 6 / 2 / `84.57b` | `66.72b` | `1.0000` / 8 / 4 / `119.68b` |
   | `T3b_CounterModN(3)` | 2 | `1.0000` / 5 / 2 / `72.92b` | `55.75b` | `0.6169` / 2 / 1 / `34.17b` |

   Under `LocalVonNeumann`, perimeter cell congestion around `BoundaryOut(0)` blocks second-stage routing on three of the four tasks, whereas `WellMixed` solves all four cleanly while `split_code_tax` still prices fan-out explicitly (`+7.76` to `+17.85` bits).

---

## 2. Verification, provenance, and implementation findings

### Finding 1 (High — Runtime defect): `TicksPerHop` panics with `MissingAncestry { port: 0 }` after multi-stage structural edits

- **Location:** [`src/structural/learner.rs:395`](../src/structural/learner.rs#L395), [`src/structural/economy.rs:413-418`](../src/structural/economy.rs#L413), [`src/structural/sensitivity.rs:251-253,318-324`](../src/structural/sensitivity.rs#L251), [`src/substrate/invariants.rs:237-246`](../src/substrate/invariants.rs#L237).
- **Reproducer:** Running `ArkheM1` on `T1_PeriodicMotif([0, 1, 1])` with `width_bits = 1`, `wiring_topology = "WellMixed"`, `wire_delay = "TicksPerHop"`, `master_seed = 0` panics:

  ```text
  thread 'main' panicked at src/structural/learner.rs:395:14:
  valid causal tick: MissingAncestry { port: 0 }
  ```

- **Root cause:** In `TicksPerHop` mode, every edge—including `NodeOut -> BoundaryOut(0)`—lowers to a `ShiftChain` with `hops >= 1` ([`lattice.rs:1553-1558`](../src/structural/lattice.rs#L1553), [`invariants.rs:237-246`](../src/substrate/invariants.rs#L237)). Consequently, `compiled.output_drivers()[0]` reads from the shift chain's `tail_reg`. When a structural edit inserts a new output edge (for example, `stack_output_adder` placing a new `Add` node before `BoundaryOut(0)`), `register_mapping` maps the new shift-chain register to `None`, so its `register_eligibility` vector initializes to all zeros (`vec![0.0; nodes]`). On the immediately following tick, `BoundaryOut(0)` is connected (`driver.is_some()`), so [`learner.rs:356`](../src/structural/learner.rs#L356) scores `port_rewards[0]`. Whenever the unprimed pipeline register's `0` output differs from `RepeatLast` ($s_{t-1}$), `port_rewards[0] != 0.0`, while `wire_ancestry[driver]` (`trace_discount * register_eligibility[tail_reg]`) sums to `0.0`. `EconomyState::apply_tick` then returns `Err(EconomyError::MissingAncestry { port: 0 })` and `.expect("valid causal tick")` panics.
- **Why `G1B.4` missed it:** `G1B.4a` Axis 4 ([`m1b.rs:970-1021`](../src/verify_m1/m1b.rs#L970)) and `G1B.4b` ([`m1b.rs:1095`](../src/verify_m1/m1b.rs#L1095)) only run `TicksPerHop` on `T1_Alternating(0, mask)`, where Epoch 1 sprouts at tick `63` ($y_{63} = 1, y_{64} = 0$) and solves in a single stage without a second-stage output-edge replacement on an incompatible parity tick.

### Finding 2 (High — Halting / metabolic defect): Epoch-1 bypass of flux and survival checks causes infinite sprout–starve–prune thrashing on noisy non-zero constant streams

- **Location:** [`src/structural/growth.rs:1662-1667,1748-1759`](../src/structural/growth.rs#L1662), [`src/structural/learner.rs:556-565`](../src/structural/learner.rs#L556).
- **Mechanism:** [Plan §4.3](milestone_1b_plan.md#43-motif-instantiation--mdl-admission-gate) specifies that silent/constant-stream halting should arise naturally from requiring positive calibrated flux ($\Delta\text{Flux} > 0$) and juvenile survival ($\widehat{E}_{\text{mat}} > \Delta L_{\text{sprout}}$). In the implementation, however, [`propose_sprout`](../src/structural/growth.rs#L1748) explicitly exempts 1-op Epoch-1 candidates (`Identity(in:0)` / `Eqz(in:0)`) from **both** `delta_flux <= 0.0` and `projected_survival <= delta_l_sprout`, relying instead on an explicit noiseless check `let is_constant_stream = obs_window.frames().iter().all(|f| f[out_idx] == first_obs)` ([`growth.rs:1665`](../src/structural/growth.rs#L1665)).
- **Measured failure under noise ($\varepsilon = 0.05$, seeds `0..19`, `horizon = 2000`, `warmup = 400`):**

  | Task ($\varepsilon = 0.05$) | $k$ | Seeds with `paired_residual_regret <= 0.025` | Mean regret (bits/sym) | Mean sprouts / starvation prunes |
  | :--- | :---: | :---: | :---: | :---: |
  | `T1_Constant(0)` | 1 | **20 / 20** | `0.0031` | `0.0` / `0.0` |
  | `T1_Constant(0)` | 2 | **20 / 20** | `0.0031` | `0.0` / `0.0` |
  | `T1_Constant(1)` | 1 | **0 / 20** | `0.6813` | `11.0` / `10.0` |
  | `T1_Constant(3)` | 2 | **0 / 20** | `1.2501` | `11.0` / `10.0` |

  On `T1_Constant(1)` with $\varepsilon = 0.05$, `is_constant_stream` is `false` because ~5% of observations are corrupted to `0`. Although $I(R; X) \approx 0$, `constant_offset` detects residual mode `1` (`>= W_diag / 2`), [`learner.rs:556-565`](../src/structural/learner.rs#L556) falls back to `BoundaryIn(0)`, and `Identity(in:0)` (`Split`, i.e. $\hat{y}_t = y_{t-1}$) is admitted via the Epoch-1 bypass despite having `delta_flux == 0.0` against the `RepeatLast` null model. Online, `Split` earns `0.0` net flux, starves at tick `128` (`4.0 < 13.75` bits debt), is pruned back to the empty `0`-predictor, and re-sprouts `64` ticks later—oscillating indefinitely between `5%` accuracy (empty) and `90.25%` accuracy (`RepeatLast`).

### Finding 3 (High — Verification gap): `G1B.2` and `G1B.4` omit pre-registered plan cases, masking an unimplemented sparse rare-event capability

Comparing [§6 of `milestone_1b_plan.md`](milestone_1b_plan.md#6-pre-registered-m1b-acceptance-gates-g1b1g1b4) and [§6.2 of `milestone_1_plan.md`](milestone_1_plan.md#62-stage-m1b-gates-first-to-second-neuron-neurogenesis) against [`src/verify_m1/m1b.rs`](../src/verify_m1/m1b.rs) reveals several discrepancies between registered criteria and enforced checks:

| Registered gate item | Plan commitment | Actual check in `src/verify_m1/m1b.rs` | Independent review probe result (`seeds 0..19`) |
| :--- | :--- | :--- | :--- |
| **`G1B.2a` structured T1** | All structured T1 tasks at $k = 1$ and $k = 2$ | Omits `T1_AffineLag1` at $k = 1$; hardcodes `cases: 10 * seeds` (`200`) while running 11 learner configs ([`m1b.rs:129,331-365`](../src/verify_m1/m1b.rs#L129)) | **Passes:** `T1_AffineLag1(1, 1, 0)` at $k = 1$ locks to `1.0000` match (`1` node). |
| **`G1B.2b` noisy `T1_Constant`** | `paired_residual_regret <= 0.025` on $\ge 90\%$ of seeds | **Omitted** from `verify_gate_g1b2` | **Split:** `T1_Constant(0)` passes `20/20` (`0.0031`); `T1_Constant(c != 0)` fails `0/20` (Finding 2). |
| **`G1B.2b` noisy `T1_PeriodicMotif(p=3)`** | Beat reactive `Shift(2)` echo by $> 0.02$ clean match on $\ge 80\%$ of seeds | **Omitted** from `verify_gate_g1b2` (only `T1_Alternating` tested under noise at [`m1b.rs:437-559`](../src/verify_m1/m1b.rs#L437)) | **Passes strongly:** `20/20` seeds close the period-3 ring at both $k = 1$ (`mean = 0.9999`) and $k = 2$ (`mean = 0.9989`) vs `~0.950` for `Shift(2)`. |
| **`G1B.3a` cross-run budget & invariant audit** | Audit `candidate_replay_ticks_consumed <= max_eval_ticks` and `Circuit::compile()` across all `G1B.1`/`2`/`4` runs | Only `G1B.3b` (the 4-case forced 500-edit oscillator stress test) is implemented ([`m1b.rs:565-732`](../src/verify_m1/m1b.rs#L565)) | `TickBudget` is enforced inside `propose_sprout` / `propose_autonomous_delay_ring`, though not re-asserted in `G1B.3`. |
| **`G1B.4a` Axis 1 (`RewardMode`)** | Compare all 4 `RewardMode`s across seed bank, including sparse `T1_PeriodicMotif([0,0,0,0,0,1])` (`>= 0.99` match) | Ignores `_seeds` (`1` seed only); omits `UncenteredSurprisal`, `CenteredSurprisal`, and `T1_PeriodicMotif([0,0,0,0,0,1])` ([`m1b.rs:736,743-867`](../src/verify_m1/m1b.rs#L736)) | **Fails on sparse `p = 6`:** `CalibratedPointForecast` gets `0.8331` (`5/6`), `0` sprouts, `0` rings (see below). |
| **`G1B.4a` Axes 2–4 (`split_code_tax`, `Topology`, `WireDelay`)** | Evaluate across multi-stage fan-out tasks and seed bank | Runs **1 seed** on **1-node `T1_Alternating(k=1)`** ([`m1b.rs:870-1021`](../src/verify_m1/m1b.rs#L870)), where `splits == 0` regardless of `split_code_tax` (`0 <= 0` passes vacuously) | On multi-stage tasks (`PeriodicMotif`, `AffineLag1`, `CounterModN`), `TicksPerHop` panics on `PeriodicMotif([0,1,1])` (Finding 1). |
| **`G1B.4b` subprocess determinism** | Exercise sprouting, debt settlement, and pruning across 6 configs | Uses `horizon_ticks: 120` while `juvenile_ticks: 128` ([`m1b.rs:1093`](../src/verify_m1/m1b.rs#L1093)), terminating before the first sprout at tick `63` ever matures (`63 + 128 = 191`) or `obs_window` reaches `128` | Separate-process determinism holds, but `horizon_ticks >= 300` is needed to exercise maturity, pruning, and ring closure. |
| **Stage-gate integration test** | Verify `verify-m1` aborts M1b when M1a fails | [`m1_stage_gate_aborts_m1b_when_m1a_fails`](../tests/m1b_neurogenesis.rs#L289) constructs and serializes a literal `M1CombinedSummary` struct without calling `run_with_args` | Stage-gate logic in [`src/lib.rs:281-290`](../src/lib.rs#L281) is correct by inspection, but untested end-to-end. |

**Why sparse `T1_PeriodicMotif([0, 0, 0, 0, 0, 1])` (`p = 6`) stalls at `0` nodes:**  
Reactive causal features only reach lag $\tau = 3$ ($y_{t-1}, \dots, y_{t-4}$), which cannot distinguish five consecutive zeros without introducing a false positive (`match_gain == 0`). Meanwhile, [`propose_autonomous_delay_ring`](../src/structural/growth.rs#L1952) begins with:

```rust
if !editable.compiled().uses_boundary_inputs() {
    return None;
}
```

Intended to prevent re-closing a circuit that has *already* transitioned into an autonomous ring, this predicate also evaluates to `true` on an **empty initial circuit (`0` nodes)**—permanently locking out autonomous ring discovery whenever reactive `propose_sprout` does not attach a boundary input first.

### Finding 4 (Moderate — Provenance hygiene): Post-holdout bug-fix commit `9544e98` invalidates the recorded holdout digest

- **Evidence:** [§9.1 of `milestone_1b_plan.md`](milestone_1b_plan.md#91-execution-sign-off-2026-11-08) and [§10.1 of `milestone_1_plan.md`](milestone_1_plan.md#101-combined-milestone-1-holdout-sign-off-2026-11-08) record single-shot holdout sign-off at commit `e29014a5c4e410f90797db079d457740c5c2d4be` (`source_tree_fnv1a128 = 564de6fbdb0b8cbd898b91fc6b4a408a`), under the pre-registered rule that no logic edits follow holdout execution. Subsequently, commit `9544e98` (*"fix(m1b): guard unconnected port subtraction in candidate materialization and preserve secondary feature pairing"*) modified `74` lines across [`src/structural/growth.rs`](../src/structural/growth.rs) and [`src/structural/learner.rs`](../src/structural/learner.rs), changing `source_tree_fnv1a128` to `ae9e3c4d577d071163c1e0855f3342a0`.
- **Assessment:** All three changes in `9544e98` were genuine correctness fixes (`Neg` / `ScaleEqz` guarding `stack_output_subtractor` on an unconnected output port; preserving `secondary_feature` when `candidate_features` is non-empty; tightening `total_regs < p` to `total_regs != p`), not threshold tuning. Nevertheless, `HEAD` no longer matches the signed-off source digest. Rather than silently re-running `10000..=10019`, closeout documentation should record `9544e98` (plus any pre-M2 bug fixes) in an explicit post-sign-off addendum or evaluate on a fresh disjoint confirmation seed bank (e.g. `20000..=20019`).

### Finding 5 (Moderate — Metric semantics): `ArkheM1Telemetry::null_calibrated_mdl_delta_bits` has inverted sign relative to Charter §3.4

- **Location:** [`src/structural/learner.rs:279`](../src/structural/learner.rs#L279) vs. [`src/metrics/mdl.rs:469`](../src/metrics/mdl.rs#L469) and [Charter §3.4](foundational_research_charter_v3.md#34-equivalence-of-metabolism-and-mdl).
- **Details:** Charter §3.4 defines $\Delta L_{\text{null}}(C) = L(C) + L(\text{facts}\mid C) - L_{\text{null}}(\text{facts})$, where **negative** values indicate compression ($\Delta L < 0$) and $\Delta E = -\Delta L_{\text{null}}$. [`metrics::null_calibrated_mdl_delta`](../src/metrics/mdl.rs#L469) follows this convention (`code_bits + facts_bits - null_bits`). In contrast, [`ArkheLearner::telemetry`](../src/structural/learner.rs#L279) computes:

  ```rust
  let null_calibrated_mdl_delta_bits = self.cumulative_net_flux - code_bits;
  ```

  which is $+\Delta E = -\Delta L_{\text{null}}$. On `T1_AffineLag1(k=1)` where the learner compresses `2,000` ticks cleanly with a `12.58`-bit circuit, `m1_telemetry.null_calibrated_mdl_delta_bits` reports **`+1919.61`**; on `T1_Constant(1)` ($\varepsilon = 0.05$) where thrashing loses `208.13` bits of flux, it reports **`-221.88`**. In addition, because `reset_predictor_counts()` (`D1b.4`) resets sequential KT counts `(t, M)` across structural edits and `max_energy_reserve = Some(25.0)` clips mature reserves while pruning discards excised node energy, `cumulative_net_flux - code_bits` is an online switching-prequential surplus rather than the static circuit's $\Delta L_{\text{null}}$. Aligning the sign and naming before **M4b** (where $\Delta L < 0$ governs axiom promotion) will prevent sign-flip bugs.

### Finding 6 (Minor — Diagnostic and mechanism notes): Unused autocorrelation diagnostic, tautological `G1B.1` plug-in MI check, and global ring-template synthesis

1. **Unused `lagged_mutual_information_bits`:** [`diagnose_residual`](../src/structural/growth.rs#L205) computes residual autocorrelation $I(R_t; R_{t-\ell})$ for lags $1..=4$, and [Plan §4.1](milestone_1b_plan.md#41-end-to-end-tick-lifecycle-single-live-circuit) describes checking autocorrelation before reactive sprouting. In [`learner.rs:536-677`](../src/structural/learner.rs#L536), `lagged_mutual_information_bits` is never read; `propose_autonomous_delay_ring` runs unconditionally *after* `propose_sprout`.
2. **`G1B.1` condition (4) checks plug-in MI on the top diagnostic feature rather than corrected MI of the winning motif:** [`learner.rs:517-531`](../src/structural/learner.rs#L517) records `mutual_information_bits` (uncorrected plug-in $\hat{I}(R_1; U_{\text{best}})$) in `EpochSnapshot`, and [`m1b.rs:249-256`](../src/verify_m1/m1b.rs#L249) asserts $|(H(R_1) - H(R_2)) - \hat{I}(R_1; U_{\text{best}})| \le 0.05$. Whenever the second stage achieves exact zero residual entropy ($H(R_2) = 0$) and $R_1$ is a deterministic function of $U_{\text{best}}$ ($H(R_1 \mid U_{\text{best}}) = 0$), $\hat{I}(R_1; U_{\text{best}}) = H(R_1)$ holds algebraically (`0.0 <= 0.05`). This still confirms that $U_{\text{best}}$ fully determines $R_1$ and that the synthesized corrector eliminates $R_1$, which is sufficient for H2, though the check is an algebraic consequence of $H(R_2) = 0$ and $H(R_1 \mid U_{\text{best}}) = 0$.
3. **Global template estimation in `propose_autonomous_delay_ring`:** [`growth.rs:2007-2025`](../src/structural/growth.rs#L2007) estimates residue-class modes `(global_t % p)` over `obs_window` using global `start_tick` and builds a fresh `Delay` ring from scratch rather than incrementally closing a feedback edge around an existing reactive `Delay` chain. This is an effective, MDL-checked scaffold for M1b's noisy periodic tasks, but it should be recognized as global period-template synthesis rather than local synaptic rewiring.

---

## 3. Soundness as a basis for subsequent milestones (`M2`–`M6`)

| Milestone | Charter goal | Foundation readiness | Key architectural blockers / seams to resolve |
| :--- | :--- | :--- | :--- |
| **M2** | Multi-channel coupling (`T2`), redundancy sprouting, internal `EQZ(SUB(a, b))` comparators (**H1**) | **Conditional — refactor `learner`/`growth` first** | Whole-lattice wipe on ring closure; North-perimeter anchor collisions at $K \ge 4$; all-port count resets on prune; `D1b.1` zero-sensitivity pruning vs. redundant pathways & internal comparators. |
| **M3** | Free-rider resistance & bloat convergence (**H3**) | **Strong** | M1a causal ancestry + ring-identity weights (`+0`, `*1`) + `MarginalDebtLedger` work well; `build_pruned_lattice` only handles simple `Delay(0)`/1-consumer `Split` patterns, not general dead cones or `+x - x` detours. |
| **M4a–M4c** | Sleep loopback, e-graph equality saturation, MDL axiom promotion & `T4` transfer (**H4, H5**) | **Good core IR; 3 seams need work** | Candidate replay uses cold `ScalarState::new` instead of snapshot registers (phase hazard); `EconomyState` scores only `BoundaryOut` ports (not internal sleep comparators); `extra_palette_entries` hardcoded to `0` & MDL delta sign inverted. |
| **M5** | Width scaling $k \in \{4, 8, \dots\}$ over $\mathbb{Z}_{2^k}$ | **Substrate ready; diagnostics blocked at $k \ge 6$** | Miller–Madow penalty scales as $(2^k - 1)/W_{\text{diag}}$, blocking binary features at $k = 8$ unless `W_diag` scales or empirical residual support is used; `1u64 << width.bits()` overflows at $k = 64$. |
| **M6** | Action ports & active prediction | **Ready after M2** | Multi-port `Circuit` and transactional `EditableLattice` cleanly generalize once M2 multi-port modularity is completed. |

### 3.1 Specific readiness analysis for M2 (Multi-channel & redundancy sprouting)

1. **Critical interaction between `D1b.1` zero-sensitivity pruning and M2 redundancy / comparators:**  
   In M1b, [`idle_mature_nodes`](../src/structural/growth.rs#L528) and [`prune_zero_sensitivity_mature_nodes`](../src/structural/surgery.rs#L277) excise any mature node with zero counterfactual sensitivity on `BoundaryOut` across `W_diag = 64` ticks. In **M2** ([Charter §3.1, §7](foundational_research_charter_v3.md#31-growth-mechanisms-wake)), two core mechanisms intentionally have zero *boundary-output* counterfactual sensitivity when first formed or when agreement holds:
   - An internal agreement comparator `c = EQZ(SUB(a, b))` whose output is an internal diagnostic/sleep wire rather than driving `BoundaryOut`, and
   - A secondary cross-channel pathway $A \to B$ sprouted redundantly alongside an already-accurate local predictor for $B$ before consolidation.  
   Before M2, counterfactual sensitivity and metabolic survival must treat registered internal comparator sinks (or multi-pathway escrow) as first-class targets rather than pruning any node that does not uniquely perturb `BoundaryOut`.
2. **Multi-channel assumptions in `ArkheLearner` and `growth`:**
   - **Whole-graph wipe on ring closure:** [`build_autonomous_ring_lattice`](../src/structural/growth.rs#L1810) constructs a blank `LatticeCircuit::new(...)` containing *only* the ring on `out_port`, and [`learner.rs:644`](../src/structural/learner.rs#L644) passes `old_to_new = vec![None; old_count]`. For $K > 1$ channels, closing a ring on channel `0` will **delete all learned nodes and state on channels `1..K-1`**.
   - **Boundary anchor collisions:** [`default_anchors`](../src/structural/learner.rs#L42) places all inputs (`offset = (2*c) % cols`) and all outputs (`offset = (2*c + 3) % cols`) on `PerimeterSide::North`. On the default $8 \times 8$ grid, `in` anchors wrap and collide at $K \ge 4$ (`c = 0` and `c = 4` share `(North, 0)`), and all channels crowd one border instead of entering along distinct boundaries as envisioned in [Charter §5.1](foundational_research_charter_v3.md#51-tier-t2--multi-channel-coupling).
   - **Cross-channel scorer resets:** On starvation or idle pruning ([`learner.rs:412-415,487-490`](../src/structural/learner.rs#L412)), `reset_predictor_counts()` is called on **every** output port `0..num_ports` rather than only ports whose upstream causal cone changed (`D1b.4`).
   - **Channel-0 snapshot hardcoding:** [`epoch_snapshots`](../src/structural/learner.rs#L510) tracks only channel `0` (`f[0]`, `p == 0`).

### 3.2 Specific readiness analysis for M3 (Free-rider resistance)

- **What is already strong:** M1a's counterfactual cone sensitivity ([`sensitivity.rs:90`](../src/structural/sensitivity.rs#L90)), identity-element input suppression (`+ 0` and `* 1` receive zero weight until non-identity activity is observed), and `MarginalDebtLedger::on_prune` ([`surgery.rs:717`](../src/structural/surgery.rs#L717)) preserving unpaid debts across unrelated edits provide a solid anti-free-rider backbone.
- **What M3 will need:** [`EditableLattice::build_pruned_lattice`](../src/structural/surgery.rs#L424) only rewires around `Add`/`Sub` nodes fed by zero-initialized `Delay`s and single-consumer `Split` relays. Multi-node zero-sum detours (e.g., adding and subtracting the same live wire so both inputs are non-zero!) have non-zero local counterfactual sensitivity on each branch individually and will require cone-level metabolic competition or M4 algebraic cancellation (`(x + y) - y -> x`).

### 3.3 Specific readiness analysis for M4a–M4c (Sleep, e-graph rewriting, and axiom promotion)

1. **Candidate replay starts from cold `ScalarState::new` rather than historical register snapshots ([`growth.rs:1612,1689,1963`](../src/structural/growth.rs#L1612)):**  
   During candidate evaluation, both the incumbent circuit and candidate extensions are stepped from `ScalarState::new` (`init` values) at `obs_window[start_idx]` rather than from a snapshot of the incumbent's live registers at `start_idx`. For purely reactive circuits of lag $\le 3$, the `64`-tick warmup half of `obs_window` (`128` ticks) flushes the cold state before scoring. However, once a circuit contains an **autonomous recurrent subcircuit** (such as an `Osc_p` ring on channel `0` while channel `1` is sprouting, or a stateful macro in `T4`), `ScalarState::new` initializes registers to tick-`0` phase, which is **out of phase** whenever `obs_window.start_tick() % p != 0`. Storing register snapshots alongside frames in `ObservationWindow` will eliminate this phase mismatch cleanly.
2. **Internal comparator `NullModel` support in `EconomyState`:**  
   [Charter §3.3](foundational_research_charter_v3.md#33-sleep-loopback-and-equality-saturation) requires sleep loopback to disable boundary rewards and route metabolic reward exclusively to internal comparator sites calibrated from wake wire statistics. Currently, `EconomyState` indexes `PointForecastScorer` strictly by `BoundaryOut` port (`0..outputs`). Generalizing scorers to addressable sink targets (`BoundaryOut(p)` vs. `InternalComparator(id)`) will let M4a reuse the exact M1a/M1b reward machinery without duplication.
3. **Macro palette MDL accounting (`extra_palette_entries`):**  
   [`mdl.rs`](../src/metrics/mdl.rs#L27) already implements `palette_code_bits(width, extra_palette_entries)` and scales primitive tag width as $\lceil \log_2(6 + M_{\text{extra}}) \rceil$, but `extra_palette_entries` is currently passed as literal `0` across `learner.rs`, `surgery.rs`, and `growth.rs`, and `Circuit` / `Primitive` does not yet carry a `Macro(MacroId)` variant.

### 3.4 Specific readiness analysis for M5 (Width scaling $k \in \{4, 8, \dots\}$)

1. **Miller–Madow bias penalty scales with $2^k$ instead of observed residual support ([`growth.rs:265-268`](../src/structural/growth.rs#L265)):**  
   `score_feature_group` computes:

   ```rust
   let residual_alphabet_size = 2.0f64.powi(i32::from(width.bits()));
   let correction = (alphabet_size.saturating_sub(1) as f64) * (residual_alphabet_size - 1.0)
       / (2.0 * std::f64::consts::LN_2 * residuals.len() as f64);
   ```

   At $k = 8$ ($2^k - 1 = 255$) and the default `W_diag = 64`, even a 2-valued `EQZ` feature (`alphabet_size = 2`) receives a subtraction of $255 / (128 \ln 2) \approx 2.874\text{ bits}$, which exceeds the maximum possible mutual information of a binary feature ($\le 1.0\text{ bit}$) and forces `corrected_mutual_information_bits < 0` on noiseless deterministic signals. Standard Miller–Madow uses the **non-empty empirical support** $(|\hat{\mathcal{A}}_X| - 1)(|\hat{\mathcal{A}}_R| - 1) / (2 N \ln 2)$ (or `W_diag` must scale with $k$).
2. **Width-64 shift overflow ([`growth.rs:750,1483`](../src/structural/growth.rs#L750)):**  
   `let ring_size = 1u64 << width.bits();` panics/overflows in Rust when `width.bits() == 64` (supported by `Width::new(64)` in `substrate`). Using `width.mask()` (e.g., `c == width.mask()` for $-1 \bmod 2^k$) avoids `1u64 << 64`.
3. **Hard `max_energy_reserve` cap (`Some(25.0)` bits) vs. width and complexity scaling ([`economy.rs:54,450`](../src/structural/economy.rs#L54)):**  
   While `D1b.2` unclips juvenile escrow prior to maturity, mature nodes are still clamped to a hard `25.0`-bit ceiling (`D1b.5`). At wider bit-widths ($k \ge 8$ in M5) or on higher-debt promoted macros (M4), single-tick noisy log-loss penalties scale as $O(k)$ bits, so a static `25.0`-bit cap can starve valid mature structures on 2–3 consecutive noise ticks. Replacing the hard constant with a scaling rule tied to $\max(\Delta L_i, k)$ (recorded in [Charter §3.1.2](foundational_research_charter_v3.md#312-metabolism)) avoids tuning per-width reserve caps.

---

## 4. Recommended pre-M2 closeout checklist

### Priority 1 — Address before starting M2

1. **Fix `TicksPerHop` new-edge ancestry initialization (Finding 1):** When `EditableLattice::recompile_preserving_state` creates unmapped shift-chain pipeline registers driving `BoundaryOut(p)` (or while a pipeline chain is still flushing its initial `hops` ticks after an edit), either seed initial register eligibility from the driving node or treat unprimed post-edit pipeline ticks identically to unconnected ports (`total_ancestry == 0.0` does not panic when pipeline output has not yet arrived). Add `T1_PeriodicMotif` and `T1_AffineLag1` under `TicksPerHop` to `G1B.4a`.
2. **Remove the Epoch-1 metabolic flux bypass (Finding 2):** Require Epoch-1 candidates in `propose_sprout` to satisfy `delta_flux > 0.0` and juvenile survival on the evaluation window (scoring against the already-warmed `PointForecastScorer` null model) rather than bypassing flux checks and relying on `is_constant_stream`. Add noisy `T1_Constant(0)` **and** `T1_Constant(width.mask())` to `G1B.2b`.
3. **Fix empty-circuit lockout in `propose_autonomous_delay_ring` and complete `G1B.2`/`G1B.4` coverage (Finding 3):** Replace `!editable.compiled().uses_boundary_inputs()` at [`growth.rs:1952`](../src/structural/growth.rs#L1952) with a check that `BoundaryOut(out_port)` is not *already* driven by an autonomous input-free cone (`node_count > 0 && !port_uses_boundary_inputs(out_port)`), and wire the omitted `G1B.2` and `G1B.4` cases (`AffineLag1` at $k=1$, noisy `Constant`, noisy `PeriodicMotif(p=3)`, sparse `PeriodicMotif([0,0,0,0,0,1])`, multi-seed fan-out ablations, and `G1B.4b` with `horizon_ticks >= 300`).
4. **Make `ArkheLearner` multi-channel safe and document M2 comparator/redundancy protection (§3.1, Finding 4):**
   - Refactor `build_autonomous_ring_lattice` to edit only `out_port`'s upstream subgraph in place rather than replacing the entire `LatticeCircuit`,
   - Distribute `default_anchors` across distinct grid sides (`North`/`West` for inputs, `South`/`East` for outputs) so $K \ge 4$ does not collide,
   - Scope `reset_predictor_counts()` on prune to output ports whose upstream cone changed, and
   - Record post-holdout closeout provenance cleanly on a fresh confirmation seed bank (`20000..=20019`) without re-touching `10000..=10019`.

### Priority 2 — Address before M4 and M5

1. **Snapshot live registers in `ObservationWindow` (§3.3):** Initialize candidate and incumbent replays from the recorded register state at `obs_window[start_idx]` rather than cold `ScalarState::new`.
2. **Align `null_calibrated_mdl_delta_bits` sign with Charter §3.4 (Finding 5):** Report `code_bits - cumulative_net_flux` (negative = compression) or rename the positive surplus field to `cumulative_net_mdl_surplus_bits` alongside exact static `null_calibrated_mdl_delta`.
3. **Generalize `EconomyState` scorers to internal comparator sinks (§3.3):** Allow M2/M4a `EQZ(SUB(a, b))` comparators to carry calibrated `NullModel` scorers and protect comparator cones from `D1b.1` zero-boundary-sensitivity pruning.
4. **Use empirical residual alphabet support in Miller–Madow, `width.mask()` for negation, and a scaling rule for `max_energy_reserve` (§3.4):** Replace `2^k` in `score_feature_group` with observed residual support size `residual_counts.len()`, replace `1u64 << width.bits()` with `width.mask()` arithmetic, and replace the fixed `max_energy_reserve = Some(25.0)` hard cap before M4/M5 with a scaling rule proportional to $\max(\Delta L_i, k)$.

---

## 5. Post-review hardening and 100-bank closeout reassessment (`2026-11-08`)

All pre-M2 defects, multi-channel structural seams, candidate-replay snapshot gaps, width-safety hazards, and verification omissions identified in §§2–4 were subsequently resolved in the codebase without modifying any pre-registered metabolic or growth constants, and verified across **100 consecutive 20-seed banks (`2,000` seeds: `20..2020`)** while keeping the reserved holdout range (`10000..=10019`) strictly untouched.

### 5.1 Resolution of review findings

| Finding / Seam | Implemented Structural Fix | Verification Coverage |
| :--- | :--- | :--- |
| **Finding 1 — `TicksPerHop` `MissingAncestry` panic** | [`ArkheLearner::observe`](../src/structural/learner.rs#L461) and [`DiagnosticBuffer`](../src/structural/learner.rs#L554) treat driven output ports whose pipeline registers have not yet accumulated upstream causal ancestry identically to unprimed/unconnected ports until first signal arrival. | Unit test `ticks_per_hop_multistage_tasks_run_without_missing_ancestry_panic` + `G1B.4a` Axis 4 across `T1_Alternating`, `T1_PeriodicMotif(p=3)`, and `T1_AffineLag1` (`100/100` banks). |
| **Finding 2 — Epoch-1 noisy non-zero constant thrashing** | [`propose_sprout`](../src/structural/growth.rs#L1562) populates candidate feature pairs strictly from `diagnostics.candidate_features`, scores replays via a synchronized [`PointForecastScorer`](../src/metrics/point_forecast.rs#L22) + [`score_with_reward_mode`](../src/structural/economy.rs#L25), enforces constant-mode error parity (`e_cand + min_match_gain <= e_const_mode` with binary `Eqz` scaffold allowance), and enforces positive juvenile survival across all candidates. | Unit test `noisy_nonzero_constants_halt_at_zero_nodes_without_thrashing` + `G1B.2b` checking both `T1_Constant(0)` and `T1_Constant(2^k - 1)` across `2,000` seeds (`100/100` banks, `0` thrashing). |
| **Finding 3 — Omitted gates, empty-graph ring lockout, & 128-tick MDL window truncation** | Replaced global `!uses_boundary_inputs()` with per-port `has_port_driver && !port_uses_boundary_in` in [`propose_autonomous_delay_ring`](../src/structural/growth.rs#L2305); added $2 \times \lceil(p+2)/2\rceil$ local rectangular loop placement and exact juvenile survival simulation (`simulate_ring_juvenile_survival`); expanded [`ObservationWindow`](../src/baselines/mod.rs#L91) capacity in [`ArkheLearner`](../src/structural/learner.rs#L240) to `juvenile_ticks + 2 * diagnostic_window` (`256` ticks, `TickBudget`-metered) so MDL amortization covers the exact $W^* = 25.46 / 0.1665 = 152.9$-tick Shannon crossover on `NoisyAlternating(k=1)`; expanded [`src/verify_m1/m1b.rs`](../src/verify_m1/m1b.rs) from `294` to `2,530` gate cases per bank; wired [`evaluate_m1_stage_gate`](../src/verify_m1/m1b.rs#L193) in [`tests/m1b_neurogenesis.rs`](../tests/m1b_neurogenesis.rs#L305). | `G1B.1`–`G1B.4` all pass **100 / 100 banks** (`253,000` M1b gate cases + `624,500` M1a gate cases). |
| **Finding 4 — Post-holdout provenance & statistical coverage** | Preserved single-shot holdout `10000..=10019` untouched and added parallel `--banks <N>` evaluation to `arkhe verify-m1b` and `arkhe verify-m1`, recording `source_tree_fnv1a128 = 10806db8e8a213e338d563a904cd36df` across `2,000` disjoint seeds (`20..2020`). | CLI holdout-overlap refusal tests for `--banks` + 100-bank sign-off record in [milestone_1b_plan.md](milestone_1b_plan.md#93-post-review-hardening--100-bank-statistical-sign-off-2026-11-08). |
| **Finding 5 — `null_calibrated_mdl_delta_bits` sign** | Aligned [`ArkheLearner::telemetry`](../src/structural/learner.rs#L380) to `code_bits - cumulative_net_flux` so negative values denote compression, matching [`metrics::null_calibrated_mdl_delta`](../src/metrics/mdl.rs#L469) and Charter §3.4. | Unit test `sparse_rare_event_ring_discovery_and_mdl_sign_convention`. |
| **M2 Multi-channel readiness (§3.1)** | [`build_autonomous_ring_lattice`](../src/structural/growth.rs#L1960) excises only `out_port`'s exclusive upstream cone while remapping surviving channels (`old_to_new`); [`default_anchors`](../src/structural/learner.rs#L42) places adjacent `(in_c, out_c)` pairs across all four perimeter sides without collisions; `reset_predictor_counts()` is scoped via [`port_cone_changed`](../src/structural/surgery.rs#L162). | Unit tests `two_channel_ring_closure_preserves_other_channel_subcircuit` and `default_anchors_distribute_multi_channel_without_border_collisions`. |
| **M4 / M5 replay & width readiness (§3.3, §3.4)** | [`ObservationWindow`](../src/baselines/mod.rs#L91) snapshots `(structure_epoch, registers)` per frame for phase-exact candidate replay; [`score_feature_group`](../src/structural/growth.rs#L265) uses empirical residual support size in Miller–Madow; `1u64 << width.bits()` shifts replaced by `width.mask()`; reserve-bounding scaling note added to [Charter §3.1.2](foundational_research_charter_v3.md#312-metabolism). | Unit test `k64_width_safety_diagnose_and_materialize_without_overflow` + all 104 workspace tests passing. |

### 5.2 Updated readiness verdict for Milestone 2 (`M2`)

**Unconditionally ready to proceed to Milestone 2 (`M2 — Multi-channel coupling and redundancy sprouting`).**  
When implementing `M2`'s cross-channel agreement comparators (`EQZ(SUB(a, b))`) and redundant pathway sprouting, `EconomyState` and `prune_zero_sensitivity_mature_nodes` (`D1b.1`) should register internal comparator sinks as first-class sensitivity/reward targets as outlined in §3.1.1 above so newly formed redundant or internal-comparator pathways are protected prior to consolidation.
