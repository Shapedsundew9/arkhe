# Milestone M4a: Sleep as Internal Falsification Draft Plan

**Date:** 2026-11-24  
**Status:** `Draft — exploratory characterisation and pre-registration pending`  
**Charter Links:** [Foundational Research Charter v3](foundational_research_charter_v3.md), [Parameter Inventory](parameter_inventory.md), [Research Record](research_record.md)

This draft follows the [milestone plan template](templates/milestone_plan_template.md).
It does not authorize confirmatory runs or opening the holdout.

## 1. Charter Hypotheses & Decision Rights

- **H5:** Sleep filtering beats compute-matched wake-only learning.
- **H6:** Consistency-only sleep (S-A) performs as well as consistency plus fact-buffer replay (S-B).
- **Scope:** Scheduled wake–sleep cycles, starting sleep from live `DELAY` state; test both arms against wake-only under equal total compute. Measure spurious self-consistent cycles and preservation of fact-supported structure.
- **Deferred:** Theorem discovery, equality saturation and promotion (M4b); transfer (M4c); width scaling and action (M5–M6).
- **Decision rights:** Follow [Charter §7.1](foundational_research_charter_v3.md#71-milestone-lifecycle-and-decision-rights). The Chief Scientist approves pre-registration, S3 and S4; no holdout may be opened before S3.

## 2. Exploratory Characterisation Summary

Pending. Measure wake-only and both sleep arms on development seeds, including equal-compute accounting, replay from live state, spurious-cycle and rare-event behavior, and guardrail telemetry. Record generators, widths, noise, costs and distributions before selecting thresholds. Existing retained-baseline lessons are in [Research Record §12](research_record.md#12-milestone-3-h4-implementation--verification-record-2026-11-22).

## 3. Pre-Registered Hypotheses & Falsifiable Success Criteria

### 3.1 Gate Thresholds & Feasibility

Pending exploratory measurements and Rule-9 feasibility arguments. Specify tasks, metrics, widths, noise, equal-compute baseline, margins and thresholds before pre-registration; do not treat this draft as a gate.

### 3.2 Option D Per-Task Tail Metrics

Pending task and metric selection. Record finite per-task tails for performance, compute, spurious cycles and guardrails; define any bounds before confirmation.

## 4. Verification Protocol & 100-Seed Confirmation / Holdout Ledger

- **Development smoke:** `0..20`; further development ranges and measurements pending.
- **Confirmation:** `20200..=20299`, 100 seeds in five banks of 20.
- **Single-shot M4a holdout:** `60000..=60099`, 100 seeds in five banks of 20, reserved and blocked in existing commands. Do not inspect or run before S3; permanently close it after consumption. Any future M4a verifier must enforce its own S3 authorization without opening this range to earlier commands.
- **Statistics:** Apply [Charter §2.8](foundational_research_charter_v3.md#28-statistical-policy-holdout-governance-and-retained-baseline-regression-gate): separate clean/noisy strata, per-task FWER, clean-seed conjunction, pooled checks, finite per-task tails, and zero exclusions.
- **S3 and S4 prerequisites:** Run `python3 scripts/check_retained_baselines.py --require-clean` on a clean tree and retain its JSON evidence; satisfy the respective confirmation or holdout requirements in Charter §2.8.
- **Commands and evidence:** Pending an implemented M4a verification harness; do not use the closed M3 banks.

## 5. Kill & Revisit Criteria

Per [Charter §7](foundational_research_charter_v3.md#7-milestones), if neither sleep arm beats compute-matched wake-only, report the finding and revisit sleep before consolidation. Exact observable criteria remain pending pre-registration. Guardrail binding is an exclusion and escalation, not a pass.

## 6. Parameter Inventory Changes

None proposed yet. Document any added or re-derived parameters, basis, telemetry and backward-reduction evidence in [the inventory](parameter_inventory.md) before approval.

## 7. Phased Implementation Plan

1. Characterise tasks, costs and competing arms on development seeds.
2. Pre-register feasible gates, verification protocol and any parameter changes.
3. Implement and verify scheduled wake–sleep arms and compute-matched comparisons in phase-sized commits with golden-trace checks.
4. Run development and confirmation, obtain S3, then perform the single-shot holdout and seek S4.

Exact phase boundaries and commands remain pending characterisation and sign-off.

## 8. Sign-Off & Attainment Record

- **Pre-registration:** Pending.
- **Retained baselines, development and confirmation:** Pending.
- **S3, holdout and S4:** Pending; no authorization is implied by this draft.
- **Source digest and independent review:** Pending.
