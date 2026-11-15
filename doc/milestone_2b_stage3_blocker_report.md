# Blocker Report: `M2B-B1` `2 * W_obs` (`512`-Tick) Ceiling Blocking Exact `0`-Error Multi-Node Stage-3 Comparators (`G2B.2`)

**Date:** 2026-11-15  
**Milestone / Stage:** `M2b` Phase 4 (`M2B-B1..B3` Dev Validation on `0..20`)  
**Triggering Rule:** [`.github/copilot-instructions.md`](../.github/copilot-instructions.md) Rules 2, 5, and 7; [Charter §7](foundational_research_charter_v3.md)

## 1. What was attempted

In worktree `/home/vscode/.gemini/antigravity-cli/worktrees/2c5c65ed-1320-4641-b38c-889490a5b613/swarm-m2b-verify-d32f3276` (HEAD `ca7136c`), worker `@m2b_verify` implemented ratified amendments **`M2B-B1`** (unified per-node debt-amortization maturation law in `src/structural/growth.rs` and `src/structural/learner.rs`), **`M2B-B2`** (atomic 1-sided comparator bankruptcy refutation), and **`M2B-B3`** (`{view_0.delay = 0, view_1.delay = 3, (5, 6)}` lag separation on `MV_AddEqz_k2_Comb` / `MV_MulEqz_k2_Comb` plus target/comparator-scoped `G2B.2(i)` anti-starvation in `src/verify_m2b/mod.rs`).

After Commit 1 (`ca7136c`) passed all M1/M2a/M2b unit & integration suites cleanly, Phase 4 `verify-m2b --seed-offset 0 --seeds 20` (`--release`) was executed against the ratified `2 * W_obs = 512` Stage-3 decidability ceiling (`allow_deep_base_scaling = false` in `stage3_measure_z_alt_and_survive`).

## 2. What was observed

Commit 1 (`ca7136cb0bc39fe82c55754380e3d4e84b13ca3b`) verified clean prior to Phase 4 (`@m2b_verify` `#55`):

- `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, full `cargo test --locked` (`tests/golden_traces.rs` `1/1` bit-identical, plus `determinism`, `m0_acceptance`, `m1a_economy`, `m1b_neurogenesis`, `multi_channel_development`, `regime_shift`, `m2b_comparators`), `verify-m1`, and `verify-m2a` all passed.
- During Commit 1 development, preserving M2a's `c2a_ring_before_deepening_closes_ring_first_and_locks_deep_lags` (`lag = 15`, `d_cone = 14`) required setting `allow_deep_base_scaling = true` for Stage 2 (`horizon_ceiling = ceiling.max(base_eff_juv as usize)` where `effective_juvenile_ticks(14) = 1175` and `t_debt = 1144` from $\gamma^{13}$ normalized chain decay), while leaving `allow_deep_base_scaling = false` (`horizon_ceiling = 512`) for Stage 3 (`src/structural/growth.rs:5018`).

Verbatim release output from `verify-m2b --seed-offset 0 --seeds 20` (`task-671`, `provenance.git_commit_sha = ca7136c`, `source_tree_fnv1a128 = 92de38bba340311d081c26f5c206aeba`, `excluded_cases = 0`, `sprout_blocked_max_nodes = 0`):

- **`G2B.1`: PASS `300/300` (`100.0%`)**, `max_epistemic_mdl_err = 1.895e-15 b/sym`, `excluded = 0`
- **`G2B.3`: PASS `140/140` (`100.0%`)**, `excluded = 0`
- **`G2B.4`: PASS `179/180` (`99.44%` vs `85.0%` gate)**, `pooled_|E[KT]-H_bayes_class| = 0.00154 b/sym <= 0.040`, single failure `G2B.4:MV_AllChNoise_k2:seed=15` (`kt=0.6501, oracle_kt=0.5927, paired_diff=0.0575, dis_prunes=0`)
- **`G2B.5`: PASS `40/40` (`100.0%`)**, `pooled_post_fault_kt_advantage = 1.43850 b/sym >= 0.15` (`M2B-B2` resolved all `MV_Fault_k1` cases)
- **`G2B.2`: FAIL `140/200` (`70.0% < 90.0%`)**:
  - `MV_AddEqz_k2_Comb`, `MV_Affine_k1_Comb`, `MV_Affine_k2_Comb`, `T1_Alternating_k1`, `T1_PeriodicMotif_k2`, `T2_NonRedundant_*` passed `140/140` (`20/20` on `MV_AddEqz_k2_Comb`).
  - Three multi-node comparator tasks failed `0/20` each with **`clean_mis = 0`** (Stage 2 solved all 60 cleanly), **`target_starv = 0`**, **`comp_starv = 0`**, and **`target_comps = 0`**:
    1. `MV_Affine_k2_TphSplit` (`20/20` fail)
    2. `MV_Eqz_k2_Comb` (`20/20` fail)
    3. `MV_MulEqz_k2_Comb` (`20/20` fail)

Verbatim `seed = 0` Stage-3 diagnostics (`@m2b_verify` `#55` inside `stage3_measure_z_alt_and_survive`):

1. **`MV_Affine_k2_TphSplit`:** exact candidate (`d_alt = 5`, `z_alt = 1.5641`, **`t_debt = 308 <= 512`**), rejected solely by `horizon_ceiling = 512` (`allow_deep_base_scaling = false`) because `M2B-D1` derived `base_eff_juv = effective_comparator_juvenile_ticks(5, 1.5641) = 557 > 512`.
2. **`MV_Eqz_k2_Comb`:** exact `e_alt = 0` candidate (`d_alt = 2`, `z_alt = 2.2957`, `base_juv = 240`, `8` new nodes), rejected at **`t_debt = 840 > 512`**: bottleneck nodes `n31(Split)` (`debt = 29.37`, `r_hat = 0.03507`, `td = 838`) and `n32(Delay)` (`debt = 20.05`, `r_hat = 0.03255`, `td = 617`).
3. **`MV_MulEqz_k2_Comb`:** exact `e_alt = 0` candidate (`d_alt = 5`, `z_alt = 3.5179`, `base_juv = 383`, `16` new nodes), `15/16` nodes at `td <= 487`, rejected solely by deepest leaf `n31(Delay)` (`debt = 16.63`, `r_hat = 0.03077`, `td = 541` -> **`t_debt = 546 > 512`**).

## 3. What was expected and why

Under [doc/milestone_2b_plan.md §2.2 & §3](milestone_2b_plan.md#3-pre-registered-hypotheses--falsifiable-success-criteria), all declared `T2_MultiViewCoupled` tasks (`MV_Affine_k2_TphSplit`, `MV_Eqz_k2_Comb`, `MV_MulEqz_k2_Comb`) are expected to sprout a disjoint `EQZ(SUB(primary, alt))` comparator in Stage 3 (`G2B.2`), bounded by `M2B-B1`'s ratified ceiling $t_{\text{juv}}(V_{\text{new}}) \le 2 W_{\text{obs}} = 512$.

## 4. Hypotheses ranked by evidence

1. **Primary hypothesis (Exact Individual-Node Credit Normalization Bounds vs. Unscaled `512` Ceiling — `MV_Affine_k2_TphSplit`, `MV_Eqz_k2_Comb`, `MV_MulEqz_k2_Comb`):**
   - `MV_Affine_k2_TphSplit` has `t_debt = 308 <= 512` and is blocked solely because `allow_deep_base_scaling = false` clipped `M2B-D1`'s already-derived `effective_comparator_juvenile_ticks(5, 1.5641) = 557` at `512`.
   - On `MV_Eqz_k2_Comb` and `MV_MulEqz_k2_Comb`, `EconomyState` normalizes a bounded comparator reward ($\approx 1$–$2$ bits/tick) across total sensitivity `Z_alt + 2` (`4.30` and `5.52`) and propagates `gamma^d` decay to upstream `Split` (`29.37` bits including `split_code_tax`) and `Delay` leaves (`r_hat = 0.031`–`0.035` b/tick). Consequently, exact `e_alt = 0` leaf amortization legitimately requires **`546` (`MV_MulEqz`)** and **`840` (`MV_Eqz`)** ticks (`> 512`, though still `< 2000`). Note that `163` ticks estimated earlier in [`doc/milestone_2b_chief_scientist_review.md §6.1`](milestone_2b_chief_scientist_review.md#61-where-we-are-over-protecting-m1-constants-juvenile_ticks--128--1--initial_energy-blocker-2) was an aggregate cone average rather than individual unpooled `Split`/leaf `EconomyState` payback time.
2. **Alternative hypothesis (Bug in Stage-3 `pos_credit_sums` or `sim_ledger` debt accounting):**
   - Disproven by consistency across nodes: `MV_MulEqz_k2_Comb` has `15/16` new nodes under `487` ticks and only `n31(Delay)` at `541` (`gamma^5` attenuation); `MV_AddEqz_k2_Comb` already passes `20/20`; `r_hat = 0.031`–`0.035` matches actual `EconomyState` credit allocation (so graduating before `838` ticks would cause `n31` to starve immediately at graduation).

## 5. Options

| Option | Behavior changed | Constants / gates touched | Engineering & scientific cost |
| :--- | :--- | :--- | :--- |
| **(a) Zero-Free-Parameter Dilution-Scaled Decidability Horizon** | Replace fixed `512` ceiling in `compute_debt_amortization_juvenile_ticks` uniformly across Stage 2 & Stage 3 with $H_{\text{decidable}} = \lceil 2 W_{\text{obs}} \cdot (t_{\text{base}} / t_{\text{juv,min}}) \rceil$, where $t_{\text{base}}$ is `effective_juvenile_ticks(d_cone)` (Stage 2) or `effective_comparator_juvenile_ticks(d_alt, z_alt)` (Stage 3). Clears `TphSplit` (`557 <= 2228`), `MV_MulEqz` (`546 <= 1532`), `MV_Eqz` (`840 <= 960`), and `c2a d=14` (`1175 <= 4700`). | Re-derives `M2B-B1` ceiling (`milestone_2b_plan.md §2.2, §8.1 #8`); reduces identically to `2 * W_obs = 512` at $K=1, d \le 3$. | **Substantive trade-offs:** (1) `MV_Eqz` (`t_juv = 840`) matures near `t ≈ 1100` inside `T = 2000` (leaves `~900` settled ticks on `G2B.2`, but slow cones would mature after `t_fault = 1000` if used in fault tasks); (2) `840` vs `960` margin on `MV_Eqz` is relatively tight (`120` ticks) across `1,000` seeds if `Z_alt` or leaf sensitivity varies; (3) noisy candidates with small `Z_alt` gain longer grace unless guarded by existing `e_alt_diag == 0` (clean ports) and `e_alt + min_match_gain <= e_null` + buffered survival (`G2B.4`). |
| **(b) Allow Unscaled `M2B-D1` `base_eff_juv` (`allow_deep_base_scaling = true`) + Relax `Split` Share / Cone-Pooled Debt Settlement** | Unclip `MV_Affine_k2_TphSplit` (`557`, since `t_debt = 308 <= 512`), AND pool `split_code_tax` / internal debt across newborn sibling nodes or adjust how upstream `Split` shares credit in `economy.rs` / `mdl.rs`. | Touches core `economy.rs` / `mdl.rs` mechanics and `M2B-B1` (`milestone_2b_plan.md`); keeps `512` debt cap. | **Substantive trade-offs:** breaks Commit 1 bit-identical `tests/golden_traces.rs`, weakens node-level sensitivity free-rider defense (`M3`), and contradicts per-node ledger accounting in `verify-m1` / `verify-m2a`. |
| **(c) Keep `t_debt <= 512` Unchanged on Deep/Nonlinear Stage-3 Cones (`MV_Eqz` / `MV_MulEqz`) and Revise `G2B.2` Multi-Node Cones / Horizon** | Accept that 8–16-node normalized nonlinear Stage-3 comparators cannot amortize in `<= 512` ticks and either extend `DEFAULT_M2B_HORIZON` / `W_obs` or split/narrow nonlinear Stage-3 tasks. | Touches `milestone_2b_plan.md §3 / §6` (`W_obs` or `G2B.2` task definitions). | Narrowing `G2B.2` weakens nonlinear coverage (`Rule 2`); doubling `W_obs` doubles noise-wait time on clean/fault tasks. |

## 6. Recommendation

Wait for User & Chief Scientist adjudication between **Option (a)** (recommended as the zero-free-parameter Derived Scaling Law path under Rule 2, while explicitly flagging the tight `840` vs `960` margin on `MV_Eqz_k2_Comb` for analytical audit—or whether `(Z_alt + 2) / Z_alt` underestimates worst-case per-leaf `1 / Z_tot` vs `(new_nodes)` dilution when `Z_alt` spreads over `8` nodes) vs **Option (b)/(c)**.

Also note operational quota status: `gemini-4-hydrogen-xhigh` / `-high` stands at **`5%` remaining** (weekly reset `2026-11-21T11:27:38Z`), while `claude-opus-5-5-high` and `claude-sonnet-5-5-high` are at `100%` (resets `2026-11-16T00:34:28Z`). `@monitor` cron has been paused to preserve the remaining Hydrogen quota.

## 7. What the agent will not do without a decision

- Will not weaken `G2B.2` (`MV_Eqz_k2_Comb`, `MV_MulEqz_k2_Comb`, `MV_Affine_k2_TphSplit`) or alter `512` (`2 * W_obs`) / `M2B-B1` unilaterally (`GEMINI.md` Rules 2, 5, 7).
- Will not modify `economy.rs` / `mdl.rs` debt pooling or `split_code_tax`, and will not run `20..1020`, `20040..=20059`, or holdout `40000..=40019` until ratified.

## 8. Ratified Decision (`2026-11-15` User Sign-Off — `Proposal M2B-C1`)

Per [Charter §7.1](foundational_research_charter_v3.md#71-milestone-lifecycle-and-decision-rights) and [`doc/milestone_2b_chief_scientist_review.md §7`](milestone_2b_chief_scientist_review.md#7-post-ca7136c-stage-3-blocker-audit-two-clause-causal-dilution-decidability-proposal-m2b-c1), the Principal Investigator ratified **`Proposal M2B-C1` (Refined Option `a+`)** (`2026-11-15`):

1. **Unified Two-Clause Decidability Law (`src/structural/growth.rs`, zero new constants):**
   - Delete `allow_deep_base_scaling: bool` from `compute_debt_amortization_juvenile_ticks`.
   - Measure mean active sink causal ancestry $Z_{\text{tot}} = \max(1.0, \bar{Z}_{\text{sink}})$ during the Pass-1 replay (`port_shares[out_port].sum()` in Stage 2 `verify_stage2_debt_amortization_and_survival`; `z_alt + COMPARATOR_PRIMITIVE_COUNT as f64` in Stage 3 `stage3_measure_z_alt_and_survive`).
   - Require **both** bounds (emitting `candidate_undecidable += 1` and rejecting if either binds):
     1. **Subgraph Epistemic Crossover (`Anti-Noise Guard`):**
        $$t_{\text{cone}}(V_{\text{new}}) = d_{\text{cone}} + \left\lceil \frac{\sum_{v \in V_{\text{new}}} \Delta L_v}{\sum_{v \in V_{\text{new}}} \hat{r}_v^+} \right\rceil \le 2 W_{\text{obs}} \quad (= 512\text{ at defaults}).$$
     2. **Per-Node Causal-Dilution Ceiling (`Anti-Parasite Guard`):**
        $$t_{\text{juv}}(V_{\text{new}}) = \max(t_{\text{base}},\, t_{\text{debt}}) \le H_{\text{node}}(V_{\text{new}}) = \left\lceil 2 W_{\text{obs}} \cdot \max\!\left(\frac{t_{\text{base}}}{t_{\text{juv,min}}},\ Z_{\text{tot}}\right) \right\rceil.$$
2. **Coupled Pass-2 Hygiene Fixes (`src/structural/learner.rs`):**
   - Forward `self.candidate_undecidable += stage3_tel.candidate_undecidable;` and `self.lag_horizon_bound += stage3_tel.lag_horizon_bound;` in Pass 2 (`ArkheLearner::observe`, Rule 3 observability).
   - Gate `self.port_states[p].stage3_exhausted = true` on `obs_full` (`self.obs_window.len() >= observation_horizon(&self.economy_config, &self.growth_config)`, matching Stage 2's `stage2_exhausted` invariant) so partial-window noise bursts cannot prematurely lock out Stage 3.
3. **Operational & Quota Authorization:**
   - Keep `@monitor` paused to preserve the remaining `5%` `gemini-4-hydrogen-*` quota; temporary execution on `claude-sonnet-5-5-medium` (prior to `2026-11-16T00:34:28Z` reset) is authorized to apply `M2B-C1` in `swarm-m2b-verify-d32f3276`, verify bit-identical `tests/golden_traces.rs` + `verify-m1` + `verify-m2a`, and execute `verify-m2b` on `0..20`, `20..1020`, and `20040..=20059`. Single-shot holdout `40000..=40019` remains locked until explicit user sign-off.
