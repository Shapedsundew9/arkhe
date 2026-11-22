# Milestone 3 Review: Implementation, Verification, and Handover Notes

**Review date:** 2026-11-22  
**Reviewed HEAD:** `7cb1ee0`  
**Scope:** Implementation evidence, recorded M3 outcomes, retained M2 regressions, documentation consistency, and information useful to M4 planning.

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
