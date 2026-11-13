# Pre-Milestone 2 Readiness and Agent Governance Plan

**Date:** 2026-11-13
**Status:** Proposed. Needs user review. It must be completed (§6) before `doc/milestone_2_plan.md` is drafted.
**Inputs reviewed:** [coordinate_free_scaling_plan.md](coordinate_free_scaling_plan.md), [foundational_research_charter_v3.md](foundational_research_charter_v3.md), [research_record.md](research_record.md) (consolidating durable findings from retired `milestone_1b_plan.md` and `milestone_1b_review.md`), `.github/copilot-instructions.md` (`GEMINI.md`), and the working tree on top of `2d009b0` (Phases 1–4 are uncommitted).

---

## Executive summary

The coordinate-free refactor met most of its aims. The lattice is gone and `substrate::Circuit` is the only topology. Per-port developmental state exists. Cliff counters exist. Pooled Clopper–Pearson and χ² helpers exist. The parameter-discipline rules are in the instructions.

The foundation still falls short of the plan's intent in three ways, and each one would put the next agent back into the failure mode we are trying to remove:

1. **The record claimed more than the code does.** The scaling plan's §2.1 inventory originally marked every row `Complete`, even though part of that work was never done (corrected to `Partial` / `Not started` in the `2026-11-13` doc cleanup, while the underlying code gaps remain). The §2.7 lag and candidate deepening is not implemented. The Option B reserve cap still contains a hard-coded `128.0`. Three code/doc definitions of $W_{\text{obs}}$ existed (`192` vs. `256` vs. `juvenile + 128`). Phase 0 measurements were never written down. The bit-identity gate of Phases 1–2 was never shown, because all phases landed in one uncommitted tree with re-blessed fixtures and no written rationale.
2. **Agents get rules but no process.** The instructions forbid magic numbers and say "stop at structural limits". They do not say who decides what, what counts as "done", how to tell a confirmatory target from an exploratory one, or what to hand the user when stuck. The charter's kill criteria exist, but nothing tells an agent that reaching one is a valid result to report rather than a problem to engineer around.
3. **M2 has an open theoretical conflict that no amount of implementation will settle.** Under the charter's own economy (§3.4: energy is MDL), a redundant derivation or a wake-time comparator adds $L(C)$ and lowers neither $L(\text{facts}\mid C)$ nor any reward. It also has zero boundary sensitivity, so `D1b.1` prunes it. As written, M2's kill criterion ("comparators never form spontaneously") is the *predicted* outcome. If an agent is handed M2 without this being resolved, it will grind against a target the theory makes impossible.

This plan lists the work (§2–§4) and the decisions only the user can make (§5) before M2 is planned.

---

## 1. Findings

### 1.1 Implementation vs. `coordinate_free_scaling_plan.md`

| # | Plan item | Claimed | Observed in working tree | Severity |
| :--- | :--- | :--- | :--- | :--- |
| F1 | §2.7 lag/candidate iterative deepening | Complete | `max_tau = 3` is passed as a literal at every call site ([learner.rs](../src/structural/learner.rs)); lagged MI uses `for lag in 1..=3` and the candidate cap is `.take(6)` ([growth.rs](../src/structural/growth.rs)). There is no deepening. `lag_horizon_exhausted` fires when feature pairs exist but yield no motif, which is not what its name says. | High: false status; the sparse-motif cliff is still there |
| F2 | §2.2 Option B reserve cap | Complete, "derived, no free multiplier" | `w_obs = juvenile_ticks + 128.0` in [economy.rs](../src/structural/economy.rs). `128` is an unreferenced literal (it happens to equal `2·W_diag`). There is also a `1e-12` floor. The unit test hard-codes `w_obs = 128.0`. | High: a magic number inside a "derived" rule |
| F3 | $W_{\text{obs}}$ definition | One rule | Originally three sources: Charter §3.1.2 stated $W_{\text{diag}} + t_{\text{juv}}$ (192) (now deferred to Q1), Plan §2.5 and `obs_capacity` use $t_{\text{juv}} + 2W_{\text{diag}}$ (256), and `economy.rs` uses $t_{\text{juv}} + 128$. Still un-unified in code (`B1`). | High: conflicting definitions in code vs. spec |
| F4 | Reserve exhaustion bound in `<= W_obs` ticks | Originally claimed as guaranteed in Charter §3.1.2 | Not proven ($\hat q$ falls with every mismatch, so per-tick drain shrinks, and residual hits pay reward; only evidence is 1-seed regime-shift test with hard-coded `256`). Unproven claim removed from Charter §3.1.2 (`2026-11-13`); still needs `B6` latency distribution. | Medium: needs empirical characterization (`B6`) |
| F5 | §2.2 step 3 "requires user sign-off" | Implemented | No sign-off for Option B is recorded anywhere. | Medium: process |
| F6 | Phase 0 exit "measurements written into §2" | — | Not written. There are no 25-bit clip counts, no compute profile (cost vs. $N$, $K$), and no binding statistics. `max_nodes = 32` was therefore not set from a measurement, as §2.3 requires. | Medium |
| F7 | §2.0.2 "harnesses report counters; guardrails asserted zero" | — | `src/verify_m1/` never reads `sprout_blocked_max_nodes`, `candidate_undecidable`, `candidate_list_truncated`, `lag_horizon_exhausted`, or `reserve_clipped_*`. Only the multi-channel test asserts one counter. | High: limits can still bind silently in accepted runs |
| F8 | Phase 1/2 bit-identity gate | Implied passed | Not demonstrable. Phases 1–3 are one uncommitted diff. All 8 golden fixtures are re-blessed (for example `alt_k1_comb_s20` digest `a6f2…` → `fb74…`). Instruction rule 4 requires a written rationale, and there is none. | High: the safety net was bypassed |
| F9 | Phase 3 exit: dev-bank re-run, new `source_tree_fnv1a128`, fresh confirmation bank | — | No record in any document. | High |
| F10 | §2.3 `node_count + motif_size <= max_nodes` | Complete | The pre-check `node_count < max_nodes` is still in `learner.rs`, and staged checks use both `>=` and `>`. The semantics are inconsistent, and blocks may be counted twice. | Low |
| F11 | Phase 2.3 per-port epoch snapshots | Complete | Snapshots are still hard-coded to `p == 0` and `sprout_events == 2`. | Low (matters for per-channel M2 evidence) |
| F12 | §2.4 acceptance: "each channel locks within the single-channel budget, not $K\times$ later" | Complete | `tests/multi_channel_development.rs` checks tail accuracy after tick 600 on one seed. It never measures a single-channel lock time to compare against. | Low |
| F13 | Unlisted constants | — | Ring period search `2..=6` ([growth.rs](../src/structural/growth.rs)), `sample_count: 8` as a struct literal rather than config, `max_tau = 3` literals, and tie-break `1e-9`. None of them have telemetry, and some are missing from the inventory. | Medium |

### 1.2 Document consistency

| # | Location | Problem | Status (`2026-11-13`) |
| :--- | :--- | :--- | :--- |
| D1 | Charter header and §7 | M1b was described as "Active stage" and linked to deleted `milestone_1_plan.md` | **Resolved** (updated M1b status; linked [research_record.md](research_record.md)) |
| D2 | Charter §1.5, §2.3, §2.5.3, §3, §3.1.1, §3.1.2, §3.1.5, §4.1, §5.1, §5.3 T3a, §8, §9 | Still used "spatial substrate", "lattice", "grid boundaries", `LocalVonNeumann`/`LocalMoore` ablations, or unproven `W_obs` claim contradicting DR #30 and `F2`–`F4` | **Resolved** (rewritten to coordinate-free graph language; §3.1.2 reserve rule points to **Q1**) |
| D3 | Charter §6 H4 & §3.1.7 | Named "Locality" as a defence; locality now exists only as wire MDL pricing | **Resolved** (updated to wire description-length pricing) |
| D4 | `milestone_1b_review.md` §5.2 | Claimed "Unconditionally ready" for M2 while listing unresolved comparator-protection prerequisite | **Resolved by archival removal** (findings & Q5 note folded into [research_record.md](research_record.md) §3.3) |
| D5 | Scaling plan §2.1 | Temporary home for Constants Inventory with misleading `Status: Complete` column | **Partially resolved** (false `Complete` labels & draft churn removed; split to `doc/parameter_inventory.md` remains in **C4**) |
| D6 | `milestone_1b_plan.md` §8 | Listed `25.0`, `8x8` grid, and `max_relay_hops` alongside `milestone_1_plan.md` links | **Resolved by archival removal** (sign-off data folded into [research_record.md](research_record.md) §2.5; superseded plan removed) |

### 1.3 Governance gaps (why agents grind)

| # | Gap | Consequence seen in M1 history |
| :--- | :--- | :--- |
| G1 | Nothing distinguishes **exploratory** runs (measure, no thresholds) from **confirmatory** runs (pre-registered thresholds) | Agents treat early guesses (`25.0`, `τ ≤ 3`, per-bank hurdles) as targets and tune toward them |
| G2 | Gate thresholds need no **feasibility argument** (a theoretical bound such as $h_\mu$ or Berlekamp–Massey $2n$, or a measured baseline) | Impossible or arbitrary targets cannot be told apart from hard ones |
| G3 | No **decision-rights** table | Agents choose scientific parameters, retire evidence, re-bless fixtures, and edit the charter on their own |
| G4 | "Stop and discuss" fires only on "structural limit or starvation cliff" | It does not cover failing gates, conflicting specs, editing a test to make it pass, or repeated fix attempts with no new understanding |
| G5 | No **definition of done** | Inventory rows get marked `Complete` with no linked evidence (F1, F2) |
| G6 | No **blocker-report format** | Escalations, when they happen, do not give the user options to choose between |
| G7 | Kill criteria are not described as **valid outcomes** | Hypothesis failure gets engineered around instead of reported |
| G8 | No commit granularity rule | Phases merge into one diff, so the golden-trace gate cannot be shown (F8) |

---

## 2. Work package A: make the record match the code (provenance)

Behavior-neutral. An agent may do it without user input, except for A4.

* **A1. Recover the Phase 1–2 bit-identity evidence.** Check out the Phase 0 fixtures from `2d009b0` into a scratch directory. Run the golden-trace configurations on the current tree with `ReserveCap::Fixed(25.0)`, the only intended Phase 3 behavior change. Compare digests.
  * Match: record "Phase 1–2 bit-identical under the M1 cap" with the command and output in the scaling plan, and keep the re-blessed fixtures with that rationale.
  * Mismatch: report the first divergent tick and event to the user (§4.3 blocker report). **Do not re-bless.** An unexplained divergence means Phases 1–2 changed behavior.
* **A2. Commit in reviewable units.** If A1 matches, commit as (i) Phases 1–2 with the Phase 0 fixtures still valid under `Fixed(25.0)`, then (ii) Phase 3 together with the re-blessed fixtures, with the A1 evidence in the commit message. If the diff cannot be split cleanly, use one commit whose message holds the A1 evidence and an explicit statement that the split was not possible.
* **A3. Finalize the inventory's evidence links (`C4`).** A preliminary truth-pass in [coordinate_free_scaling_plan.md](coordinate_free_scaling_plan.md) §2.1 already cleared false `Complete` labels across `F1`–`F13`. Complete the move into standalone `doc/parameter_inventory.md` (`C4`) with concrete `Evidence` entries (test name, commit, or command output) once Work Package B closes out.
* **A4. Record the Option B decision.** Ask the user to confirm or reject Option B (§5, decision Q1). Record the answer in the charter decision record (new row #31).
* **A5. Run and record the Phase 3 exit evidence.** After work package B: `verify-m1` on development banks (seed range chosen by the user per Q8), pass/fail per gate, all cliff counters, the new `source_tree_fnv1a128`, and one fresh confirmation bank. The holdout `10000..=10019` stays untouched.

## 3. Work package B: close the implementation gaps

* **B1. One definition of $W_{\text{obs}}$.** Expose a single function, for example `observation_horizon(&EconomyConfig, &GrowthConfig) -> usize`, used by both `obs_capacity` and the reserve cap. Pass it into `EconomyState` and remove the `128.0` literal. Fix the unit test so that it derives the value instead of hard-coding it. Bring charter §3.1.2 in line. If the chosen definition changes M1 behavior (192 vs. 256), that is a behavior change, and it goes through Q1 and a golden re-bless with written rationale.
* **B2. Lag and candidate search (§2.7).** Choose one of these and do not leave a third state:
  * (a) implement the deepening exactly as specified, gated on "no feature passes", with targeted tests on sparse `PeriodicMotif([0,0,0,0,0,1])`; or
  * (b) reclassify `τ ≤ 3`, `take(6)` and ring period `2..=6` as **pre-registered search bounds** with telemetry that fires when they bind, and defer deepening to the M2 plan.

  This choice is Q2 in §5. Either way, every literal moves into `GrowthConfig` and the inventory, and the counters are renamed to say what they measure (`LagHorizonBound`, `CandidateListTruncated`, `RingPeriodBound`).
* **B3. Make the harnesses read the counters.** Add every cliff counter to the `verify-m1` per-case and per-bank summaries. For guardrails, a non-zero count marks the case `excluded` (not `passed`), which is the §2.0.1 semantics. Add a test asserting that any guardrail binding sets `excluded`.
* **B4. `max_nodes` semantics.** Use a single check, `node_count + motif_size <= max_nodes`, at proposal time only. Count it once per blocked proposal and remove the duplicate pre-check. Golden traces must stay identical, or the divergence must be explained.
* **B5. Phase 0 measurements.** Write a `scripts/` utility (automated, because it will be re-run for every milestone) that runs the dev suites and emits a markdown table of counter totals and per-tick cost against $N$ and $K$. Record the output in the inventory successor (C4). Set `max_nodes` from it only after user sign-off (Q3).
* **B6. Regime-shift test as a measurement.** Run it over several seeds and task pairs, and report the starvation latency distribution. Derive the bound from the B1 function instead of the literal `256`. Whether it then *asserts* anything depends on Q1. Until the user accepts a rule, it reports and does not gate.
* **B7. Per-port epoch snapshots and the lock-time comparison.** Key snapshots by port. Rewrite the multi-channel test to measure each channel's lock tick against the same task run alone, across several seeds, and report the ratio. The pass threshold is a pre-registered parameter (Q4); do not invent it.

## 4. Work package C: documents and governance

### 4.1 Charter & doc cleanup (behavior-neutral)

* **C1 (`Done 2026-11-13` for `D1`–`D3`; §3.1.2 final formula awaits `Q1` / `B1`).** Updated Charter header & §7 M1b status, linked [research_record.md](research_record.md), removed dead `milestone_1_plan.md` links, and rewrote spatial/lattice wording in §1.5, §2.3, §2.5.3, §3, §3.1.1, §3.1.5, §3.1.7, §4.1, §5.1, §5.3 T3a, §6 H4, §8, and §9 as coordinate-free graph language.
* **C2 (`Done 2026-11-13`).** Folded durable M1b empirical findings, 100-bank per-gate counts, defect syntheses, and DR #30 retirement anchors into [research_record.md](research_record.md), removed superseded `milestone_1b_plan.md` and `milestone_1b_review.md` from `doc/`, cleaned draft churn from [coordinate_free_scaling_plan.md](coordinate_free_scaling_plan.md), and updated `README.md` doc links.
* **C3 (Open).** Add an automated **doc-link check**: a test or `scripts/` utility that fails on relative links in `doc/` and `README.md` pointing at missing files. Dead links have now recurred more than once.

### 4.2 Constants inventory successor

* C4. Move the scaling plan's §2.1 into a standalone `doc/parameter_inventory.md`, with columns: constant, value, config path, class (derived / pre-registered / guardrail / search bound), derivation or justification link, telemetry counter, sensitivity sweep evidence, implementation status, evidence. Mark the scaling plan as *Completed, historical*, and point instruction rule 2 at the new file.

### 4.3 Proposed amendment to `.github/copilot-instructions.md`

This replaces rule 5 and adds rules 7–11. It states principles and pointers, not formulas.

```markdown
5. **Stop and escalate** (write a blocker report, then wait for the user) when any of these occur:
   - a structural limit, guardrail, starvation cliff, or search bound binds;
   - a pre-registered gate fails on development seeds and the cause is not an implementation bug
     with a demonstrable fix;
   - a fix would add, remove, or change a constant, a threshold, a gate, a test assertion, or a
     golden fixture;
   - two consecutive fix attempts for the same failure brought no new diagnostic understanding;
   - the charter, a milestone plan, the inventory, and the code disagree;
   - a theoretical argument suggests the target is unreachable under the current rules.
7. **Decision rights.** Agents decide engineering details that preserve behavior. Only the user
   decides scientific parameters and their values, gate thresholds, seed banks, evidence retirement,
   golden re-blessing, charter text, kill-criterion interpretation, and milestone sign-off. See
   Charter §7.1.
8. **Exploratory vs. confirmatory.** Before a threshold is pre-registered, runs are exploratory:
   they measure and report and never gate. Confirmatory runs use only thresholds the user has
   signed off. Never tune toward an exploratory number.
9. **Feasibility before targets.** Every gate threshold must cite a theoretical bound or a measured
   baseline that shows it is reachable. A threshold without one is a characterisation metric, not
   a gate.
10. **Definition of done.** A plan item is done only when it links to evidence: a test name, a commit,
    or a recorded command plus its output. "Complete" with no evidence is a defect.
11. **Kill criteria are results.** Reaching a charter kill or revisit criterion is a finding to report,
    not an obstacle to engineer around. One phase per commit; golden traces verified at each
    behavior-preserving commit.
```

Blocker-report template (to be added as `doc/templates/blocker_report.md`): *What was attempted → What was observed (counters, first divergence, gate output) → What was expected and why (cite charter or plan) → Hypotheses ranked by evidence → Options (each with the behavior it changes, the constants it touches, and the cost) → Recommendation → What the agent will not do without a decision.*

### 4.4 Charter additions

* C5. Add **§7.1 Milestone lifecycle and decision rights**: charter hypothesis → exploratory characterisation (no gates) → milestone plan with pre-registration (hypotheses, gates with feasibility arguments, seed banks, kill criteria, inventory changes) → **user sign-off** → phased implementation (one commit per phase, golden traces) → dev banks → single-shot holdout → attainment record → independent review. Include the decision-rights table from rule 7.
* C6. Add a sentence to §7: *A kill or revisit outcome is a successful experiment. The agent records it and escalates. It does not alter mechanisms, thresholds or tasks to avoid it.*
* C7. Add `doc/templates/milestone_plan_template.md` with the required sections listed in C5, so every milestone plan has the same shape.

---

## 5. Decisions required from the user (before M2 planning)

| ID | Question | Options (agent's recommendation first) |
| :--- | :--- | :--- |
| **Q1** | Reserve cap rule and horizon | (a) Keep Option B with $W_{\text{obs}} = t_{\text{juv}} + 2W_{\text{diag}}$ (matches `obs_capacity`), drop the "guaranteed within $W_{\text{obs}}$" claim, and record a multi-seed regime-shift latency distribution as evidence. (b) Option B with $W_{\text{diag}} + t_{\text{juv}}$ (charter text). (c) Option C, exponential forgetting with time constant $W_{\text{obs}}$. (d) Uncapped plus regime-shift handling deferred. |
| **Q2** | Lag and candidate search bounds | (a) Reclassify them as pre-registered search bounds with telemetry now, and specify deepening in the M2 plan once B5 measurements exist. (b) Implement the §2.7 deepening now. |
| **Q3** | `max_nodes` | Set it from the B5 compute profile, or keep `32` as a guardrail that is asserted never to bind |
| **Q4** | Multi-channel lock-time criterion | A pre-registered ratio or a pooled test against the single-channel lock time (an exploratory measurement first, per rule 8) |
| **Q5** | **Comparator and redundancy economics: the M2 blocker.** Under §3.4, redundancy costs $L(C)$ and earns nothing in wake, and `D1b.1` prunes nodes with zero boundary sensitivity. What funds a redundant derivation and its comparator? | (a) **Split M2.** M2a: T2 cross-channel coupling with the existing corrector mechanism (already possible, since `BoundaryIn(c)` features from every channel are in the signal pool). M2b: redundancy and comparators, only after a charter amendment that defines comparator reward. (b) Allow wake-time comparator reward: an internal sink with a calibrated scorer on agreement surprisal, which changes charter §3.1.4 ("sleep only"). (c) Justify redundancy in MDL terms through the error-detection value it has under declared noise or regime shift (needs a derivation before any code). (d) Escrow or protect redundancy until sleep exists, which in practice moves H3 to M4a. |
| **Q6** | Meaning of M2's success criteria | Define "T2 solved" (which generators, $k$, noise, and metric relative to $h_\mu$), "comparators flag injected inconsistencies" (flag in what signal, and what responds in wake), "recovery from declared noise", and "compared against baselines" (which baselines, metric, and margin at equal compute). Each needs a feasibility argument (rule 9). |
| **Q7** | Does M2 require comparators to be *the* mechanism that solves T2? | If T2 is passed by corrector sprouting alone, is H3 untested, or is that acceptable for M2a? |
| **Q8** | Seed governance | Confirm the dev seed range, a fresh confirmation bank (for example `20000..=20019`), and a new M2 holdout range disjoint from `10000..=10019` |
| **Q9** | Pooled gate parameters | $P_{\text{target}}$ per gate family and the number of banks. Clopper–Pearson 99% and χ² 1% are already pre-registered in the scaling plan §2.8. |

> [!IMPORTANT]
> Q5 is not an engineering question. M2's charter kill criterion ("comparators never form spontaneously") is what the current economics predicts. Any M2 plan written before Q5 is answered will either fail that criterion or quietly add an ungrounded subsidy, which is exactly the failure mode this plan exists to prevent.

---

## 6. Exit criteria (gate to drafting `milestone_2_plan.md`)

1. A1 evidence recorded. The working tree is committed per A2, and every golden re-bless has a written rationale.
2. B1–B4 and B6–B7 merged. `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings` and `cargo test --locked` pass. Every cliff counter appears in `verify-m1` output, and guardrail counters are zero in accepted cases.
3. B5 measurement table and A5 dev-bank and confirmation-bank results recorded, with the new `source_tree_fnv1a128`.
4. `doc/parameter_inventory.md` exists, each row has evidence, and there are no unlisted literals in `src/structural/` (checked by review).
5. Charter §3.1.2 finalized for `Q1`, Charter **C5** and **C6**, and the instructions amendment (§4.3) merged. The automated doc-link check (`C3`) passes. Templates (§4.3, `C7`) exist.
6. `Q1`–`Q9` answered and recorded as Charter decision records #31 onward. The M2 plan is then written against the answers to `Q5`–`Q9` only, using the `C7` template.

## 7. Suggested order

| Step | Work | Needs user? | Behavior change? |
| :---: | :--- | :---: | :---: |
| 1 | `A1` (bit-identity recovery) → `A2` (commits) | Only if `A1` diverges | No |
| 2 | `C3` automated doc-link check, `C4` standalone `doc/parameter_inventory.md`, and `A3` evidence links (`C1`–`C2` completed `2026-11-13`) | No | No |
| 3 | §4.3 instructions amendment, C5–C7 charter process and templates | Review the text | No |
| 4 | Present Q1–Q9 to the user | **Yes** | — |
| 5 | B3, B4, B5, B7 (observability and measurement) | No | No (golden identical) |
| 6 | B1, B2, B6 per Q1 and Q2 | Already decided in step 4 | Possibly; re-bless with rationale |
| 7 | A5 dev-bank and confirmation records, exit review (§6) | Sign-off | — |

---

## 8. Readiness Execution & Exit Verification Log (2026-11-13)

Status of **§6 Exit Criteria** after commits `5a7c27d`, `a47ff8d`, `22954df`, `7649e2c`, and final inventory/documentation closeout:

1. **Criterion 1 (`A1`, `A2`):** Clean git commit chain `5a7c27d` $\to$ `a47ff8d` $\to$ `22954df` $\to$ `7649e2c` $\to$ closeout commit. Bit-identical golden trace verification passes (`cargo test --locked --test golden_traces`, `1 passed; 0 failed`), with no unapproved fixture re-bless (`G1B.4a:Axis1` resolved via user-approved **Option (a)** in [`src/verify_m1/m1b.rs`](../src/verify_m1/m1b.rs)).
2. **Criterion 2 (`B1`–`B4`, `B6`–`B7`, CLI cliff telemetry):** Implemented in `a47ff8d` and `22954df`. `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --locked` pass with zero warnings, zero skipped tests, and `0` guardrail exclusions (`excluded_cases = 0`, `sprout_blocked_max_nodes = 0`) across `verify-m1a`, `verify-m1b`, and `verify-m1`.
3. **Criterion 3 (`B5`, `A5`):** Recorded in [`doc/parameter_inventory.md §3`](parameter_inventory.md#3-compute-profile--measurement-tables) and [`doc/research_record.md §7`](research_record.md#7-pre-milestone-2-readiness-measurements--verification-2026-11-13) on clean binary `7649e2caa38e78575381c03d8e6952118a816fc6` (`source_tree_fnv1a128 = 26e9d332905dd6a6aeadce50c5266fa3`): dev `0..20` pass, `100/100` dev banks (`20..2020`) pass, and fresh confirmation bank `20000..=20019` pass (`10000..=10019` untouched; `30000..=30019` reserved).
4. **Criterion 4 (`doc/parameter_inventory.md` & literal audit):** [`doc/parameter_inventory.md`](parameter_inventory.md) promotes and audits every non-test constant in `src/structural/` (`MDL_TIE_BREAK_EPS`, `RING_SIM_SENSITIVITY_SAMPLES`, `SMALL_CONST_CANDIDATE_CAP`, `EXACT_SENSITIVITY_MAX_BITS`, `RESERVE_MISMATCH_DENOM_FLOOR`, `EconomyConfig`, `GrowthConfig`) and `src/verify_m1/` with code links and test/measurement evidence.
5. **Criterion 5 (Charter §3.1.2, `C5`, `C6`, `C7`, instructions amendment, link check):** Completed in commit `5a7c27d` and verified via `python3 scripts/check_doc_links.py` across `README.md` and `doc/**/*.md`.
6. **Criterion 6 (`Q1`–`Q9` / DR #31–#39):** Recorded in [Charter Appendix A](foundational_research_charter_v3.md#appendix-a-decision-record). Readiness exit gate satisfied; `doc/milestone_2_plan.md` awaits explicit user initiation under [Charter §7.1](foundational_research_charter_v3.md#71-milestone-lifecycle-and-decision-rights).
