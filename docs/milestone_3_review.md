# Milestone 3 Review: Implementation, Verification, and Handover Notes

**Review date:** 2026-11-22  
**Reviewed HEAD:** `7cb1ee0`  
**Scope:** Implementation evidence, recorded M3 outcomes, retained M2 regressions, documentation consistency, and information useful to M4 planning.

> **Latest re-review:** [Pre-S3, 2026-11-22 at `a7ec23c`](#pre-s3-re-review-at-a7ec23c-2026-11-22). R2-R5 are resolved and the clean-commit evidence matches the current source. R1 remains partially open for a surviving cyclic source without an earlier matching snapshot. Earlier reviews below are preserved as historical evidence.

## Executive Summary

The planned M3 mechanisms, verification harnesses, and closeout records are present. The M3 plan and research record document successful dev, confirmation, and single-shot holdout runs, and the M3 holdout is recorded as passing.

However, the claim that M3 preserved the M2b regression gate is contradicted by reproducible results. `verify-m2b` G2B.2 fell from 200/200 cases before M3 to 180/200 after Phase 1; the failures are all the `MV_AddEqz_k2_Comb` task. A five-bank run on current HEAD also fails the pooled G2B.2 criterion. Thus M3 implementation and holdout completion are evidenced, but retained-regression equivalence—and therefore an unqualified “fully verified” assessment—needs investigation and adjudication.

This review records the discrepancy; it does not change code, gate definitions, holdout governance, or the prior S4 decision.

## Implementation and Completion Evidence

- The five planned phases are committed: Phase 1 `9b5b950`, Phase 2 `e498424`, Phase 3 `235a292`, follow-up fixes `004aaa5` and `a24ea1a`, Phase 4 `dd13839`, and Phase 5 `7cb1ee0`.
- The planned mechanisms are present in `src/structural/growth.rs`, `src/structural/learner.rs`, and `src/structural/surgery.rs`; M3 exploration and verification are in `src/verify_m3/`. Focused integration and CLI coverage is in `tests/m3_free_riders.rs` and `tests/cli.rs`.
- The plan’s attainment ledger records the 1,000-seed dev run (`20..1020`), confirmation (`20060..=20079`), and single-shot M3 holdout (`50000..=50019`) as passing with zero excluded cases and zero `sprout_blocked_max_nodes`. The holdout result is recorded at `dd13839`, with S3/S4 approvals #60/#64.
- M3 mechanisms, constants, derived rules, results, and holdout provenance are recorded in [the M3 plan](milestone_3_plan.md), [the research record](research_record.md) §12, and [the parameter inventory](parameter_inventory.md). The charter records M3 progress and decisions #44–#46.
- The recorded M3 holdout result is `G3.2 = 648/660` (`0.981818`) with `max_excise_dt = 427` ticks, against the pre-registered 256-tick established-excision bound. The aggregate gate passed, but the per-detour maximum is useful operational context for M4.
- The doc-link checker passed during the audit: `python3 scripts/check_doc_links.py`.

## Retained-Regression Discrepancy: G2B.2

The plan (§§6–7 and 8.2), parameter inventory, and Charter DR #45 describe M3 changes as bit-identical on retained M2a/M2b checks. The recorded and reproduced gate counts do not support that blanket claim.

| Comparison | G2A.4 (`verify-m2a`, seeds `0..20`) | G2B.2 (`verify-m2b`, seeds `0..20`) |
| :--- | :---: | :---: |
| Pre-M3 commit `b102259` | 19/20 | 200/200 |
| Current commit `7cb1ee0` | 20/20 | **180/200** |

Both versions were built from clean source at their respective commits and run with `--seed-offset 0 --seeds 20`. The G2B.2 failures on current HEAD are all `MV_AddEqz_k2_Comb` (20/20 seeds); each reports `target_comps=0` and no settled comparator ticks. The other G2B gates in this slice pass.

A current-HEAD five-bank run (`verify-m2b --banks 5 --seed-offset 20 --seeds 20`) also fails G2B.2: 900/1000 pooled cases, pooled 99% lower bound `0.875814` versus target `0.90`. Each bank scored 0.90, so the homogeneity test passes; it is the pooled confidence-bound criterion that fails. The M2b research record reports 10000/10000 G2B.2 cases for its earlier 50-bank run.

The regression is already present at the Phase 1 commit `9b5b950` and remains at `e498424` and `235a292`. A scratch experiment changing the current default `ExcisionMode` to `BaselineD1b1` did not restore the gate. This narrows the onset to Phase 1 but does **not** identify the root cause. The improved G2A.4 count is also a departure from the pre-M3 slice, though not a regression.

**Action for milestone governance:** This retained-regression failure warrants Level-1 coordinator investigation under the repository escalation rules. If resolving it requires changing a pre-registered threshold, a derived law, or the milestone’s completion/progression decision, that decision belongs at Level 2. Do not treat the five-bank result as a full 50-bank rerun; it is a reproducible failing slice.

## Documentation and Governance Inconsistencies

1. **Stale M3 holdout status:** `parameter_inventory.md` §1.1 and §6 still describe `50000..=50019` as pending S3/untouched, although §12 of `research_record.md` and the plan record that it ran at `dd13839`.
2. **Incorrect “unconditionally rejected” wording:** `research_record.md` §12.4 says consumed M2a/M2b ranges are unconditionally rejected across the verification commands. The harnesses still permit their own range when invoked with `--holdout`; the M3 range is likewise guarded by that flag in `verify-m3`. Thus the CLI flag guard is not, by itself, a mechanism preventing a second execution of a consumed bank. This review does not alter holdout code or authorize any rerun.
3. **Charter table status:** In Charter §7, the M3 row’s success criterion is not marked “Completed & verified,” unlike earlier completed milestones, while the header and Appendix A record M3 closeout.
4. **No M3 rejected-alternatives entry:** `research_record.md` §4 has no M3-specific entries. The paired `BaselineD1b1` pruning alternative and what was learned from its ablation may be worth recording there.
5. **Plan retirement:** `milestone_3_plan.md` remains in `docs/`. Prior milestone closeouts describe folding durable findings into the inventory and research record before retiring completed plan files. Retiring it is a documentation-maintenance task, not evidence that the implementation is incomplete.

## Handover Notes for an M4 Planner

- **M4a holdout:** `60000..=60019` is reserved and rejected by the existing M2a, M2b, M3 verification, and M3 exploration guards. This review found no M4a confirmation range allocated in the reviewed plan/inventory. `20080..=20099` follows the existing confirmation-bank sequence, but it is only a candidate, not an approved allocation.
- **Resolve comparator regression first:** G2B.2 is a core comparator-formation check. The failing task forms no comparator on any tested seed, so M4a sleep/falsification experiments should account for this baseline failure before relying on comparator behavior.
- **Circuit-size scope:** M3 establishes bounded overhead, not convergence to the exact-synthesis minimum. The plan defers e-graph resynthesis and tighter minimization to M4b; the certified exact-synthesis yardstick remains capped at `N <= 4`, `K <= 2`.
- **Sleep/replay questions already open in `research_record.md` §6.2:** Replay should begin from live Delay registers rather than reset initial values. CTW and Reeds–Sloane baseline compute is not currently metered, which matters for equal-compute comparisons. Switching codes remain an open alternative.
- **Telemetry and limits:** The recorded 1,000-seed M3 dev run has `lag_horizon_bound = 250,000` (zero in the M2b record), `reserve_clipped_count = 4,446,789,247`, `candidate_undecidable = 12,503,095`, and `sprout_budget_exhausted = 7,720,466`. These counters did not cause exclusions in the accepted run, but their magnitude and the nonzero lag-bound count merit monitoring under multi-cycle wake–sleep.
- **Scaffolding:** `W_diag=64`, `sprout_interval=32`, `juvenile_ticks=128`, `gamma=0.90`, the MI threshold, and initial energy are recorded as fixed M3-era settings rather than fully derived scaling laws. M4 should assess whether its expanded wake–sleep regime causes any of them to bind, following the parameter-inventory and escalation rules.
- **Excise latency:** G3.2 was the weakest M3 gate; the M3 holdout maximum of 427 ticks exceeded the 256-tick per-detour bound despite the aggregate gate passing.
- **Reproducibility artifacts:** Full verification JSON outputs are referenced under `/tmp` in the historical record and are not repository artifacts. The durable tables and key metrics are in `research_record.md` §12; any M4 rerun should preserve its outputs and provenance in a durable location.

## Audit Boundaries

This review did not rerun M3’s 1,000-seed suite or its consumed holdout. It did reproduce the pre-M3/current 20-seed M2a/M2b comparisons and the current five-bank M2b slice described above. No source, plan, inventory, charter, or existing research-record file was changed as part of this review.

## Re-review at b33c550 (2026-11-22)

### Assessment and Resolved Findings

The remediation is substantive, but **M3 v2 is not yet ready for an unqualified completion or S3 recommendation**. Its plan correctly records it as pre-S3 rather than retaining the superseded v1 completion claim. The additional defects below should be resolved before collecting final acceptance evidence.

- **Original G2B.2 regression:** The acyclic-register priming change addresses the reported `MV_AddEqz_k2_Comb` failure. Its new focused test, `mv_addeqz_k2_comb_forms_causal_disjoint_comparator_on_seed_0`, passed during this re-review. [Research record section 12.5](research_record.md#125-milestone-3-v2-remediation--statistical-policy-upgrade-pre-s3-evidence-packet) records `G2B.2 = 1000/1000`, including `MV_AddEqz_k2_Comb = 100/100`, in the pre-commit retained-baseline run.
- **Anti-masking changes:** Clean/noisy stratification, per-task FWER tests, clean-seed conjunctions, and concentrated-failure regression tests are implemented. These directly address the original pooling failure, subject to the consistency defects below.
- **Consumed holdouts:** M2a, M2b, and M3 v1 ranges are now unconditionally blocked, including with `--holdout`. The fresh M3 v2 holdout is `50100..=50199`; the M4a reservation is expanded to `60000..=60099`. Neither was accessed in this re-review.
- **Documentation:** The plan, charter, and research record now explain the v1 supersession, the priming and comparator-refutation/accounting fixes, the new statistical policy, and pending v2 verification. M4a confirmation is now allocated as `20200..=20299`, replacing the initial review's unapproved suggestion.
- **Evidence checked:** The existing M3 v2 dev-20, dev-100, and confirmation-100 JSON reports agree with the recorded passing outcomes and zero exclusions. All identify pre-commit `05ca3d5`, `git_dirty = true`, and source digest `5ee45123a16157ced68df776181f2fa4`; they are not clean-commit sign-off evidence.

### R1 - High: Priming Still Resets Surviving-State History

**Source:** [growth.rs](../src/structural/growth.rs), `prime_non_surviving_registers`, line 2564 at the reviewed commit.

The helper initializes its shadow simulation with `ScalarState::new(staged_compiled)`. It does not restore the surviving circuit's historical registers at `prime_start`. Meanwhile, `new_acyclic_register_depth` stops at surviving nodes. Consequently, replaying only the newly added pipeline depth is insufficient when a new Delay consumes an existing stateful node.

**Reproduced:** A surviving `Delay { init: 0 }` feeds a newly added `Delay { init: 0 }` in a combinational circuit. With observed frames `[3, 2, 1]` and `start_idx = eval_start_idx = 3`, full historical replay gives registers `[2, 3]`. Calling the new priming helper on `[2, 0]`, with the old register mapped as surviving, leaves `[2, 0]`. The new register should contain `3`, not `0`; the old register is correctly left unchanged.

This means the fix works for the reported boundary-fed comparator but can still mis-score candidates that use live interior state. Restore the surviving state at the beginning of the priming interval, or use another state-consistent replay method, and add coverage for this case without changing gate thresholds.

### R2 - High: Clean Sign-off Is Not Bound to the Reviewed Revision

**Source:** [check_retained_baselines.py](../scripts/check_retained_baselines.py), provenance validation at line 240 and binary execution at line 897.

`--require-clean` checks only the report's `provenance.git_dirty == false`. It neither checks the current worktree nor requires the reported commit/source digest to match the code being approved. The script runs golden traces from the current checkout but executes an existing release binary without rebuilding it or checking its revision.

**Reproduced:** `validate_suite_report(..., require_clean=True)` accepted an otherwise passing report carrying the old `7cb1ee0` commit and v1 source digest with `git_dirty = false`. This was a synthetic validator probe, not a claim that the old binary passes the new gates.

A stale clean binary can therefore supply baseline evidence for newer, untested source while golden traces exercise a different revision. Bind every suite report and the golden-trace run to the current clean revision/source, rebuilding or rejecting stale binaries before sign-off.

### R3 - Medium: Clean-Stratum Acceptance Skips a Required Macro Check

**Source:** [stats.rs](../src/stats.rs), `evaluate_regime_stratum`, lines 642-652 at the reviewed commit; [Charter section 2.8](foundational_research_charter_v3.md#28-statistical-policy-holdout-governance-and-retained-baseline-regression-gate).

When a clean-seed conjunction is present, the non-exact branch uses `conj.passed && all_tasks_passed` but does not require `pooled_multi_bank.passed`. The charter requires the conjunction alongside stratum-level pooled confidence/homogeneity checks; the Python checker also rejects a failed `pooled_multi_bank`.

**Reproduced:** Five clean tasks, each with two banks scoring `199/200` and `195/200`, and clean conjunction banks of `195/200` and `195/200`, at target `0.90`. Every per-task test and the conjunction pass, but pooled homogeneity fails. `evaluate_stratified_gate_audit(..., enforce_pooled_cp=true)` nevertheless returns `passed = true`.

Align the Rust acceptance rule, checker, and charter. If conjunction is intended to replace rather than supplement pooled checks, that requires an explicit policy clarification instead of inconsistent verdicts.

### R4 - Medium: Mandatory Statistical Evidence Can Be Missing or Contradictory

**Source:** [check_retained_baselines.py](../scripts/check_retained_baselines.py), `_validate_task` and `_validate_stratum`, especially lines 92-119 and 145-182.

The checker trusts nested pass flags without validating their consistency with numeric results. It never validates `seed_conjunction_multi_bank`; `tail_metrics` is checked only when present.

**Reproduced:** The validator returned success for each of these modifications to its passing synthetic fixture:

- A clean conjunction explicitly reporting `passed = false`, with `0/100` successes.
- A task with zero successes, `empirical_rate = 0`, and `binomial_lower_tail_p = 0`, while its pass flags remained true.
- All task `tail_metrics` fields removed, despite `require_finite_tail_metrics = true`.

This is not solely hypothetical missing telemetry. The existing M3 v2 dev-100 and confirmation-100 JSON reports omit tails for 36/144 clean G3.1 tasks, all 77 G3.5a tasks, and all 37 G3.5b tasks. Omission does not establish a scientific failure, but it does not meet the documented "every per-task audit" evidence contract.

Define required per-gate evidence, check conjunctions and numerical/flag consistency, and reject missing required metrics rather than treating absent data as valid finite data. The current 26 self-tests pass but do not cover these counterexamples.

### R5 - Medium: Additional Priming Simulation Is Unmetered

**Source:** [growth.rs](../src/structural/growth.rs), the shadow-step loop beginning at line 2566 and callers at lines 3105 and 5819.

The callers charge `baseline.w_replay` ticks to the candidate budget. The new helper then performs additional full-circuit `shadow.step` calls over `prime_start..start_idx` without receiving or debiting that budget. These steps lie before the charged replay interval.

Charge the additional simulation through the existing budget/telemetry mechanism. Otherwise candidate-replay cost and budget exhaustion understate the actual work, which is particularly relevant to M4a equal-compute comparisons. This finding follows directly from the call path; no large benchmark was rerun to quantify its frequency.

### Remaining Verification and Review Boundaries

The existing [pre-S3 evidence packet](research_record.md#125-milestone-3-v2-remediation--statistical-policy-upgrade-pre-s3-evidence-packet) still lists these completion prerequisites:

1. Resolve the findings and obtain clean-commit retained-baseline evidence using `--require-clean` after repairing its validation gaps.
2. Complete the M3 v2 50-bank dev sweep (`20..1020`) and ensure confirmation evidence corresponds to the final reviewed source.
3. Obtain S3 authorization before the fresh 100-seed holdout (`50100..=50199`).
4. Obtain S4 closeout after acceptance, permanently close the consumed v2 range, and retain the raw JSON evidence durably.

This re-review ran only the original comparator regression test (passed), the checker self-tests (`26/26` passed), two isolated regression probes (both failed as detailed in R1/R3), synthetic validator counterexamples, and documentation-link validation. The probes ran in a temporary worktree of `b33c550` with no production-mechanism edits; that worktree was removed. No broad scientific suite or holdout was rerun. Only this review document was changed in the main worktree.

## Pre-S3 Re-review at a7ec23c (2026-11-22)

### Disposition of R1-R5

The remediation and its evidence are substantially improved. **R2-R5 can be closed for this review. R1's original acyclic counterexample is fixed, but a reproducible cyclic-source fallback defect remains.** This is a pre-S3 assessment, not a holdout authorization or milestone-completion decision.

| Finding | Disposition | Evidence |
| :--- | :--- | :--- |
| R1: surviving-state priming | **Partially resolved; residual below** | The original two-Delay reproducer now yields `[2, 3]`, with and without an earlier matching snapshot. Snapshot-backed cyclic priming passes. The combination of a cyclic source and missing earlier snapshot still fails. |
| R2: provenance binding | **Resolved** | The checker captures live worktree state and source/lock digests, rejects stale commit/digest evidence, requires expected provenance for clean validation, and supports explicit rebuild. Its stale-report and dirty-worktree self-tests pass. |
| R3: clean-stratum macro checks | **Resolved** | The Rust rule now requires pooled acceptance, clean conjunction, and per-task checks together. The exact previous pooled-homogeneity counterexample is retained as a passing regression test. |
| R4: missing or contradictory evidence | **Resolved** | The checker recomputes binomial/Clopper-Pearson quantities, cross-checks counts and flags, requires clean conjunctions, and enforces nonempty finite tails plus manifest-required keys. Previously failing synthetic cases are rejected. Archived M3 dev/confirmation reports now contain nonempty finite tails for all 469 tasks. |
| R5: unmetered priming | **Resolved** | Stage 2 and Stage 3 charge `w_replay + prime_steps`, accounting for any already-charged fast-path replay. Exact-budget and insufficient-budget regression cases pass and exhaustion remains observable. |

### R1 Residual - High: Missing-Snapshot Fallback Cannot Reconstruct Cyclic Phase

**Sources at the reviewed commit:** [growth.rs](../src/structural/growth.rs), `plan_non_surviving_register_priming` at line 2554, `acyclic_register_depth` at line 2435, and the snapshot-loading callback in `evaluate_sprout_motif` at line 3320.

If the snapshot at `prime_start_new` is absent or belongs to a different structure epoch, the planner falls back to a "full" acyclic register depth. That depth still stops at cyclic nodes. The shadow replay consequently starts the surviving ring from its declared initial registers when no matching snapshot exists at the fallback start. Replaying the new pipeline's depth does not restore the ring's historical phase, and the helper returns normally rather than reporting unavailable state.

**Reproduced by extending the existing cyclic-ring regression in an isolated worktree:**

1. Use its surviving period-3 ring `[1, 2, 3]` feeding a newly added Delay.
2. Preserve all observed frames and the valid snapshot at `start_idx = 3`, but omit earlier snapshots. The live surviving registers at index 3 are `[3, 2, 1]`; the ring output at index 2 was `3`.
3. Prime with `start_idx = eval_start_idx = 3`.
4. The surviving registers remain correct, but the new Delay receives **`1` instead of `3`**. The unchanged snapshot-backed variant passes.

This is the cyclic counterpart of the missing-snapshot case already covered for acyclic pipelines, not a failure of the previously reported two-Delay fix. The production Stage-2 path uses the same optional snapshot loading and proceeds to score the resulting candidate.

**Required correction:** Seed replay from valid historical state or a rigorously reconstructed cyclic phase. If that history cannot be established, explicitly reject/defer the candidate with an observable reason instead of evaluating it from an arbitrary initial phase. Cover both missing and mismatched-epoch snapshots for cyclic sources, while retaining the repaired budget accounting.

### Evidence Rechecked

- **Source equivalence:** `a7ec23c` differs from clean code commit `f55bb096782b36806d15e7ca9119992b98004ea4` only in four documentation files. The independently computed current source digest is `398a25d5e5f4cbe765d98403cc85a3a6`, matching the archived evidence; the lock digest is `8d29cd24b1fbdbb743cf5c0b3933b22d`.
- **Retained baselines:** Revalidated `/tmp/m3_v2_r5_retained/m2a_dev100.json` and `m2b_dev100.json` using the updated validator and manifest against their recorded clean `f55bb09` provenance. Both pass. The archived verdict includes an unskipped, passing golden-trace run with the same clean provenance.
- **M3 evidence:** Checked `/tmp/m3_v2_r5_dev20.json`, `m3_v2_r5_dev100.json`, and `m3_v2_r5_conf100.json`. Each reports `all_passed = true`, zero exclusions, zero max-node guardrail hits, matching provenance, and no missing/empty/non-finite task tails. These were inspected, not rerun.
- **Focused Rust tests:** `cargo test --locked --lib -- r1_ r3_ r5_` passed all five selected tests.
- **Validator tests:** `python3 scripts/check_retained_baselines.py --self-test` passed all 42 tests.
- **Additional probe:** The isolated missing-snapshot cyclic-source extension failed with `left: 1`, `right: 3`. Its scratch worktree was removed after recording the result.

### Pre-S3 Boundary

The checked dev/confirmation results remain valid evidence for the recorded source, but do not establish correctness of the residual R1 fallback. Resolve or explicitly adjudicate that defect before opening the fresh holdout. After any code correction, refresh the affected clean-commit evidence.

The latest plan and charter status now label the unexecuted 50-bank development sweep optional; it is not represented here as completed or as a newly discovered failure. S3 authorization, the fresh `50100..=50199` holdout, S4 closeout, permanent closure of that consumed range, and durable retention of raw artifacts remain subsequent steps.

No holdout or broad scientific suite was run during this re-review. Only this review document was changed in the main worktree.
