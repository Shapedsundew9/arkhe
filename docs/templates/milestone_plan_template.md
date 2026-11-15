# Milestone [ID]: [Title] Pre-Registration Plan

**Date:** YYYY-MM-DD  
**Status:** `Draft awaiting user sign-off`  
**Charter Links:** [Foundational Research Charter v3](../foundational_research_charter_v3.md), [Parameter Inventory](../parameter_inventory.md), [Research Record](../research_record.md)

## 1. Charter Hypotheses & Decision Rights

- **Hypotheses tested:** `<H_x, ...>`
- **Explicit non-goals / deferred hypotheses:** `<What this milestone does NOT claim>`
- **Decision rights & execution tiers:** Per Charter §7.1 and [`.github/copilot-instructions.md`](../../.github/copilot-instructions.md) Rules 2, 4, 5, and 7, parameter values, Milestone-Scoped Scaffolding $\to$ Derived Scaling Law evolutions, gate thresholds, seed banks, golden re-blessing approvals, and kill-criterion interpretation require explicit user sign-off before confirmatory execution. Implementation follows the 3-tier hierarchy (Tier 1 Hydrogen workers with a 2-strike escalation rule; Tier 2 `arkhe-watchdog`; Tier 3 Argon orchestrator).

## 2. Exploratory Characterisation Summary

Document exploratory measurements recorded prior to pre-registration (Rule 8: exploratory runs measure and report without gating).

## 3. Pre-Registered Hypotheses & Falsifiable Success Criteria

| Gate ID | Definition (metric, generator, width `k`, noise, equal-compute baseline) | Target Threshold (`P_target`, margin) | Feasibility Argument (Rule 9: theoretical bound or measured baseline) |
| :--- | :--- | :--- | :--- |
| `G...` | ... | ... | ... |

## 4. Verification Protocol & Seed Governance

- **Development seeds:** `<explicit range>`
- **Confirmation bank (fresh dev):** `<explicit range>`
- **Holdout seeds (single-shot only, disjoint from M1 `10000..=10019`):** `<explicit range>`
- **Multi-bank statistics:** One-sided 99% Clopper–Pearson lower confidence bound on pooled seed successes (`P_target`) plus 1% chi-squared across-bank homogeneity test (`src/verify_m1/stats.rs`).
- **Guardrail invariant:** Any run where a compute guardrail binds is marked `excluded` and never counted as `passed`.

## 5. Kill & Revisit Criteria

> [!IMPORTANT]
> Per Charter §7 and Rule 11, a kill or revisit outcome is a valid experimental result. If triggered on development or holdout runs, the agent records the finding, writes a blocker report, and escalates rather than altering mechanisms, thresholds, or tasks to avoid it.

- **Kill / revisit condition:** `<exact Charter §7 trigger and observable threshold>`

## 6. Parameter Inventory Changes

List every constant added, reclassified (e.g., **Milestone-Scoped Scaffolding** evolved into a zero-free-parameter **Derived Scaling Law** as scale axes $\tau, K, k, N$ expand), derived, or bounded in [`docs/parameter_inventory.md`](../parameter_inventory.md), with its classification, config path, telemetry counter, backward-reduction behavior on golden traces, and sensitivity sweep plan.

## 7. Phased Implementation Plan

| Phase | Scope | Golden-Trace Gate / Rationale | Expected Commit Unit |
| :---: | :--- | :--- | :--- |
| Phase 1 | ... | Behavior-preserving (`golden_traces` bit-identical) | 1 commit |

## 8. User Sign-Off & Attainment Record

- **Pre-registration sign-off:** `<Pending user approval / commit hash>`
- **Dev bank summary:** `<To be filled post-implementation>`
- **Single-shot holdout outcome:** `<To be filled once after dev banks pass>`
- **Source digest (`source_tree_fnv1a128`):** `<To be filled>`
- **Independent review status:** `<Open / Reviewed>`
