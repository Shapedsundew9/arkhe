# Milestone [ID]: [Title] Pre-Registration Plan

**Date:** YYYY-MM-DD  
**Status:** `Draft awaiting user sign-off`  
**Charter Links:** [Foundational Research Charter v3](../foundational_research_charter_v3.md), [Parameter Inventory](../parameter_inventory.md), [Research Record](../research_record.md)

## 1. Charter Hypotheses & Decision Rights

- **Hypotheses tested:** `<H_x, ...>`
- **Explicit non-goals / deferred hypotheses:** `<What this milestone does NOT claim>`
- **Decision rights & escalation levels:** Per Charter §7.1 and [`.github/copilot-instructions.md`](../../.github/copilot-instructions.md) Rules 2, 4, 5, and 7, execution follows the two-level escalation hierarchy: Implementation Workers (`Hydrogen`, 2-strike bounded) and the Operational Watchdog (`arkhe-watchdog` sidecar) escalate to Level 1 (`@coordinator`, Senior Scientist Orchestrator) for intra-phase/intra-milestone mechanistic tweaks, bug fixes, and correcting substantively suboptimal behavior that trips a limit; Level 2 (`@chief-scientist`, Foundational Stand-In for the User) holds autonomous authority over Charter §§2–3 amendments (when required to achieve Charter §§0–1 goals), [`docs/parameter_inventory.md`](../parameter_inventory.md) reasoned Derived Scaling Laws (no magic numbers), pre-registered gate threshold recalibration (Rule 9), Rule-4 golden re-blessing, milestone splitting (`Ma/Mb/Mc`) or re-ordering, single-shot holdout authorization (`S3`), and milestone closeout/progression (`S4`). The User (Principal Investigator) is required only if Charter-Core Invariants (Charter §§0–1, holdout isolation) or the overarching Charter goal are at risk with no realistic path forward.

## 2. Exploratory Characterisation Summary

Document exploratory measurements recorded prior to pre-registration (Rule 8: exploratory runs measure and report without gating).

## 3. Pre-Registered Hypotheses & Falsifiable Success Criteria (Regime-Stratified)

Separate `Clean` ($\varepsilon = 0.0$) and `Noisy` ($\varepsilon > 0.0$) tasks strictly (`GateRegime::Clean` vs. `GateRegime::Noisy` in `StratifiedGateAudit`, [Charter §2.8](../foundational_research_charter_v3.md#28-statistical-policy-holdout-governance-and-retained-baseline-regression-gate)); never pool across regimes or hide single-task failures behind pooled empirical averages.

### 3.1 Gate Thresholds & Feasibility

| Gate ID | Regime Stratum (`Clean` / `Noisy`) | Definition (metric, generator, width `k`, noise, equal-compute baseline) | Target Threshold (`P_target`, margin, FWER $\alpha = 0.01 / M_{\text{stratum}}$) | Feasibility Argument (Rule 9: theoretical bound or measured baseline) |
| :--- | :---: | :--- | :--- | :--- |
| `G...` | `Clean` / `Noisy` | ... | ... | ... |

### 3.2 Option D Per-Task Tail Metrics (`tail_metrics`)

Record finite per-task extrema and 95th-percentile bounds emitted via `update_max_finite` / `update_min_finite` (`src/stats.rs`, preventing non-finite `INFINITY` from serializing as JSON `null`):

| Gate / Task Label | Tail Metric Key | Pre-Registered Bound / Diagnostic Expectation |
| :--- | :--- | :--- |
| `G...:<task>` | ... | ... |

## 4. Verification Protocol & 100-Seed Confirmation / Holdout Ledger

- **Fast development smoke only (non-confirmatory):** `0..20` (`20` seeds).
- **Development multi-bank sweep:** `<explicit range, e.g. 20..1020 (50 × 20)>`
- **Confirmation bank (`100` seeds = `5` banks $\times$ `20` seeds, fresh dev):** `<explicit range, e.g. 20200..=20299 for M4a+>`
- **Holdout seeds (`100` seeds = `5` banks $\times$ `20` seeds, single-shot only, disjoint from permanently closed ranges `10000..=10019`, `30000..=30019`, `40000..=40019`, `50000..=50019`, and `50100..=50199`):** `<explicit range, e.g. 60000..=60099>`
- **Unconditional closure of consumed holdout ranges:** Every consumed holdout range must be permanently closed with `allow_with_flag: false` in `HoldoutRangeSpec` across all CLI verification commands in the milestone closeout commit.
- **Statistical Policy Option A (`src/stats.rs` `StratifiedGateAudit`):**
  1. Never pool `Clean` ($\varepsilon = 0.0$) and `Noisy` ($\varepsilon > 0.0$) tasks into one gate criterion.
  2. Within each stratum of $M_{\text{stratum}}$ tasks, enforce per-task FWER exact binomial lower-tail check at $\alpha = 0.01 / M_{\text{stratum}}$ (exact `total_successes == total_trials` when `target_rate >= 1.0`) and per-task across-bank homogeneity at $\alpha = 0.01 / M_{\text{stratum}}$ (`evaluate_bank_homogeneity_with_alpha`).
  3. Enforce per-seed `Clean` conjunction across all clean tasks in a gate (`seed_conjunction_multi_bank`), plus stratum-level 99% Clopper–Pearson lower confidence bound and homogeneity checks.
- **Guardrail invariant:** Any run where a compute guardrail (`SproutBlockedMaxNodes`) binds is marked `excluded` and never counted as `passed`.
- **Mandatory Retained-Baseline Regression Gate (`S3` & `S4` precondition):** Before any `S3` or `S4` request, run `python3 scripts/check_retained_baselines.py --require-clean` against `docs/retained_baselines.json` on a clean git tree (forbidding `--fast`, `--skip-golden`, `git_dirty: true`, and prose-only or pooled-only sign-off claims).

| Check | Command | Required Outcome | Clean-Tree JSON Evidence |
| :--- | :--- | :--- | :--- |
| Golden traces + retained M2a/M2b gates | `python3 scripts/check_retained_baselines.py --require-clean` | `all_passed: true`, `git_dirty: false`, `golden_traces` `1/1` bit-identical | `<JSON artifact / summary>` |
| 100-seed confirmation (`S3` precondition) | `cargo run --locked --release -- verify-<m> --banks 5 --seed-offset <conf> --seeds 20` | `StratifiedGateAudit` clean & noisy strata pass, `excluded_cases == 0` | `<JSON artifact / summary>` |
| 100-seed single-shot holdout (`S4` precondition) | `cargo run --locked --release -- verify-<m> --banks 5 --seed-offset <holdout> --seeds 20 --holdout` | `StratifiedGateAudit` clean & noisy strata pass, `excluded_cases == 0`, Option D tails inspected | `<JSON artifact / summary>` |

## 5. Kill & Revisit Criteria

> [!IMPORTANT]
> Per Charter §7 and Rule 11, a kill or revisit outcome is a valid experimental result. If triggered on development or holdout runs, the agent records the finding, writes a blocker report, and escalates rather than altering mechanisms, thresholds, or tasks to avoid it.

- **Kill / revisit condition:** `<exact Charter §7 trigger and observable threshold>`

## 6. Parameter Inventory Changes

List every constant added, reclassified (e.g., **Milestone-Scoped Scaffolding** evolved into a reasoned **Derived Scaling Law**—with no magic numbers—as scale axes $\tau, K, k, N$ expand), derived, or bounded in [`docs/parameter_inventory.md`](../parameter_inventory.md), with its classification, config path, telemetry counter, backward-reduction behavior on golden traces, and sensitivity sweep plan.

## 7. Phased Implementation Plan

| Phase | Scope | Golden-Trace Gate / Rationale | Expected Commit Unit |
| :---: | :--- | :--- | :--- |
| Phase 1 | ... | Behavior-preserving (`golden_traces` bit-identical) | 1 commit |

## 8. User Sign-Off & Attainment Record

- **Pre-registration sign-off:** `<Pending user / Chief Scientist approval / commit hash>`
- **Retained-baseline gate (`python3 scripts/check_retained_baselines.py --require-clean`):** `<To be filled from clean-tree JSON output>`
- **Dev bank & 100-seed confirmation (`5 × 20`) summary:** `<To be filled post-implementation with StratifiedGateAudit + Option D tail_metrics>`
- **100-seed single-shot holdout (`5 × 20`) outcome:** `<To be filled once after confirmation + retained-baseline gate pass>`
- **Consumed holdout closure commit (`allow_with_flag: false`):** `<To be filled at S4 closeout>`
- **Source digest (`source_tree_fnv1a128`):** `<To be filled>`
- **Independent review status:** `<Open / Reviewed>`
