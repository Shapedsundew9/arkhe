# Milestone M4b: Theorem Discovery and Consolidation — Preliminary Pre-Registration Plan

**Date:** 2026-11-27  
**Status:** Draft for review. Development-only characterisation proposed; no M4b thresholds approved, confirmation authorised, or holdout opened.  
**Basis:** [Charter §§0–1, 3.2–3.4, 4.2–4.4 and 7](foundational_research_charter_v3.md), [M4a independent review](milestone_4a_independent_review.md), [M4a plan and attainment record](milestone_4a_plan.md), [research record](research_record.md), [parameter inventory](parameter_inventory.md).

## 1. Charter Hypotheses & Decision Rights

**Primary question (H7):** Can the learner take two different computations built from circuits it already holds, discover during sleep that they are equivalent, prove it, and use that theorem to add a cheaper word to its wake vocabulary? The minimum Charter witness is **one non-trivial, spontaneously discovered, formally verified equivalence with a profitable promotion**. A smaller graph alone is not that witness. One witness establishes existence, not a discovery rate across tasks.

**Separate question (H5, DR #50 K2):** On already-correct, spontaneously learned `Clean` circuits, does S-A *with* consolidation finish with a shorter description than equal-compute wake-only, without sacrificing prediction? Compare it also with S-A *without* consolidation to isolate what consolidation added. A profitable local edit need not overcome the facts forgone during sleep. Report H7 and Clean H5 separately even if their verdicts differ.

Use consistency-only **S-A** as the reference sleep schedule. Retain S-B for M4a regression, not as the discovery arm. Keep the environment observational. In-scope reuse is an immutable, expandable macro usable in wake on the **same task**. Transfer speed on unseen families (M4c/H8), lifting across widths (M5/H9), action (M6/H10), and sustained open-ended growth are not claims of M4b.

Under [Charter §7.1](foundational_research_charter_v3.md#71-milestone-lifecycle-and-decision-rights) and [repository Rules 2–11](instructions/repo-common.md), the coordinator owns development and local fixes; workers stop after two failed fixes. The Chief Scientist owns gate/budget or schedule changes, Rule-4 golden decisions, holdout authorisation (S3) and progression (S4). Escalate an unresolved threat to the Charter core to the PI. This draft grants none of those approvals.

## 2. Exploratory Characterisation Summary

**What we know.** M4a S-A beat wake-only in 264/300 paired holdout cases on *three pre-selected noisy periodic cells*. All eight Clean spontaneous cells had zero observed incorrect-comparator opportunity at onset in characterisation. These are findings about **refutation**, not about the availability of repeated computations. The +14.61-bit mean favouring S-A over S-B was a schedule decision on a mixture of spontaneous and planted cases, not evidence that wake facts are dispensable. The [review](milestone_4a_independent_review.md) also distinguishes exact tick parity from the reported node-work envelope, equal-length from identical evaluation data, and episode-level removal from sleep-caused removal. **M4b opportunity and effect sizes are unknown.**

**M4a closeout prerequisite.** `60000..=60099` was consumed and sealed at `3582242`; S4 is still pending in the [attainment record](milestone_4a_plan.md#8-sign-off--attainment-record). Verify the provenance and SHA-256 of `retained_baselines_closeout_3582242.json` against the review and archive the clean standard-tier report in [M4a evidence](evidence/m4a/). If unavailable, regenerate the **retained development suites**, not the holdout, on a documented clean revision. On development pairs publish raw sleep/wake evaluation counts and ratios alongside the historical R2 ratio. The review's `0.985995` raw versus `0.985131` reported is a denominator difference, **not** an observed overrun. If raw work exceeds wake, qualify the compute-matched H5 interpretation and obtain an explicit R2 ruling. Record the M4a S4 disposition before formal M4b progression; do not rewrite the historical gate or reopen the bank.

**First new experiment: count opportunities, not successes.** Use the [M4a spontaneous registry](../src/verify_m4a/tasks.rs) unchanged: `Period6_LagRedundancy`, `MV_Periodic`, `MV_Lfsr`, and `ChannelFault`, each at uniform `k = 1, 2`. Start with Clean spontaneous circuits; profile noisy counterparts separately at the registered noise settings. Keep `ChannelFault` separately labelled even in nominally Clean runs: the post-fault channel is not clean. Keep episodes with no sleep onset. Take read-only snapshots at scheduled opportunities as well as actual onset; count each distinct occurrence in a circuit, not repeated observations of the same occurrence across ticks.

| Census output (by task, width and seed) | Why it decides the next step |
| :--- | :--- |
| Number of held circuits, scoreable sleep onsets, already-correct ports, candidate-bearing circuits with and without onset | Distinguishes lack of structure from lack of access to it. Correctness labels belong to the evaluator, never to the learner. |
| Repeated cones up to port-preserving renaming, and distinct held forms with the same small typed interface; non-overlapping use count, boundary ports, explicit `SPLIT`, register count, latency and wire mode | Identical repeats show potential for a dictionary, not discovery of an identity. Reject boundaries with hidden external dependencies; for stateful cones specify how port history and declared initial state determine internal state (§4.4). |
| Optimistic removable bits versus minimum definition and call costs; then actual whole-graph re-encoding for feasible candidates | Report both a bound and a measured ledger. Do not count shared interior nodes twice, ignore opcode repricing elsewhere, or amortise a definition across independent seeds or future M4c tasks. |
| Planted repeated cone with enough uses to repay its measured cost; already-minimal circuit; wrong-width and shared-evidence proposals | Positive and negative controls test the apparatus. Keep their results outside the spontaneous numerator and denominator. |

If even the optimistic bound cannot break even in the inspected class, stop before building a large search engine. If the bound is positive but only on skipped onsets, report a scheduling problem, not an H7 failure. Options for a new exploratory round are a better justified interface boundary, a declared multi-output generator with recurring structure learned from facts, or a separately approved consolidation trigger. Do not plant the answer, change S-A silently, or lower macro prices. A bounded census is not a proof that no theorem exists.

## 3. Pre-Registered Hypotheses & Falsifiable Success Criteria (Proposed)

### 3.1 Gate definitions and feasibility

**What counts as discovery.** Freeze and publish the primitive axioms, supplied rewrite set and canonicalisation rules before the experiment. Seed the e-graph with typed cones from the current learner, compose compatible ports, propose equalities between distinct forms, and extract by charged code length. A small exhaustive semantic signature may propose a join; sampled agreement or e-class membership is not proof. For a witness retain both held origins, the derivation and the extracted replacement. It must do more than rename a graph, alias a repeated body, read back a supplied rewrite (including a mere substitution), remove a planted detour, or observe agreement on one orbit. A novel composition of known steps is admissible if the resulting relation is not supplied and it enables the charged replacement. Log candidate birth, wake/sleep segment, refutation, proof, promotion and first wake call; wake pruning and no-onset copies count as zero discoveries.

**Independent proof and live replacement.** The checker takes the two expanded circuits, their typed port bijection, width, wire mode, latency and state relation, not the explorer's assertion. For a combinational cone with `m` input words enumerate all `2^(k*m)` valuations and compare **all** outputs. For `DELAY` or `TicksPerHop`, require a reset/live state map and sequential proof: related states produce equal outputs on each input and step to related states, including all pipeline registers from the first tick. Finite product-state exploration or a checkable bit-vector proof is sufficient; a finite rollout is not. Start with a combinational witness; leave unsupported stateful candidates unpromoted. Test `x - y = y - x`: true at `k=1`, false at `k=2` on `(1,0)` (`1` versus `3`). Also reject a one-tick phase shift with the same eventual orbit, a corrupted certificate and an exhausted proof. None may mutate the live learner.

**Promotion is a transaction** ([Charter §4.3](foundational_research_charter_v3.md#43-from-retention-to-promotion)):

1. Record loopback evidence of no persistent disagreement. A suspended comparator is not an agreeing comparator.
2. Record free-running versus wake-buffer fact statistics in S-A, including missing support; fact matching is required for S-B, not a new teacher-forced S-A phase. Neither agreement nor an oracle label is a proof.
3. On the *same* data and scorer state require strictly positive savings, `B = L_before - L_after > 0`, for `L = L(palette) + L(circuit given palette) + L(facts given circuit) + L(corroboration given circuit)`. Publish each term in bits. Encode a decodable frozen definition and interface once per palette, pay each call, all affected opcode/wire/`SPLIT` costs, and retain definition debt even for a single call. Do not count hoped-for usage. An exact interface-preserving substitution with the same independent routes should leave both data terms unchanged; investigate differences, do not claim them as syntactic savings. Check the existing null-calibrated energy/MDL conservation identity, including exogenous accounting, separately from this paired score.
4. Require the exact certificate above. Archive the original graph, frozen expansion, interface, initial/live mapping, proof and ledger. Recheck from the archive without the discovery engine. Apply via [transactional graph surgery](../src/structural/surgery.rs); preserve surviving registers, scorer/economy state and true causal support. Two calls may share a *definition*, not a state register or a second reward for the same evidence. Verify independent routes, fan-out and sleep-exit restoration after expansion. Demonstrate wake execution as an atomic call with the same per-tick outputs as expansion; an unused entry is retirable, not free capital.

**Paired experiment.** Run A = wake-only, B = canonical S-A with consolidation off, C = S-A with search, proof and promotion charged. Use the same initial wake trajectory, task seed and tick allocation; pair B/C on the same scheduled sleep slots and log actual onset/skips. Preserve grounding, paused environment, frozen wake scorers and zero boundary reward in sleep. Any new trigger requires an identically scheduled no-consolidation control, with canonical B retained. Count facts forgone. Keep the historical arm-terminal score for continuity; for causal comparisons use **one independent evaluation stream** per task/seed, with identical synchronisation prefix, warm-up exclusions, scoring horizon and trace digest for all frozen arms. No evaluation data may select candidates or enter the learner. Report circuit, palette, prediction and corroboration differences separately.

| Proposed gate | Case-level decision | Feasibility / status |
| :--- | :--- | :--- |
| G4B.1 Accounting and isolation (Clean / Noisy separately) | Equal substrate ticks; raw node work in each sleep arm no greater than actual wake total; unchanged pre-onset state; no extra uncharged logical work, isolation or conservation violations. | Tick equality and no uncharged work are construction requirements. Measure the raw envelope and a usable charged search budget on development before registering. |
| G4B.2 Sound replacement (both strata and controls) | Every installed replacement passes all applicable §4.3 checks, independent replay, per-tick equivalence and provenance checks; zero unsound promotions or double corroboration. | Exact finite proof and transactional rejection are achievable by construction. A system that never promotes can pass safety, **not H7**. |
| G4B.3 H7 discovery (spontaneous cells, by stratum) | Seed passes if C produces at least one qualifying, replayable promotion with `B > 0` and a subsequent wake use. Publish unconditional rate over all registered seeds, as well as counts conditional on opportunity. | Strict gain and one distinct witness are definitional minima. A planted success or a second copy of a theorem is not a second distinct theorem. Per-task rate targets need census and end-to-end development evidence. |
| G4B.4 Clean H5 (spontaneous; fault stress separate) | Test positive paired terminal savings `L_A - L_C` on the common stream, with per-target prediction non-inferiority; report `L_B - L_C` for incremental attribution. Report no-onset, lost locks and negative tails. | DR #50 assigns this endpoint to M4b; it does not supply a feasible M4b margin or rate. Freeze the operational definition of already-correct, task population, regret margin and targets before confirmation. Do not infer H5 from G4B.3. |

**Registration boundary:** No empirical `P_target`, superiority margin, or search limit is inferred from M4a's noisy win rate. Before confirmation, publish the full task matrix (generator, width, noise, wire mode, horizon and arm), positive/negative controls, discovery rubric, scoring and compute code, per-task denominators, thresholds and Rule-9 justification. If Clean H5 is infeasible, record it as unresolved and seek a roadmap decision; do not drop failing Clean cells to claim general superiority.

### 3.2 Option D per-task tails

| Gate | Finite metrics to report per task and arm/pair | Interpretation |
| :--- | :--- | :--- |
| G4B.1 | Maximum tick difference, raw and historical envelope ratios, raw evaluation difference, logical work and conservation error; p95 facts forgone | Print actual numerator/denominator and live, replay and sensitivity work. Equal ticks or a normalised ratio cannot hide a raw overrun. |
| G4B.2 | Maximum certificate failures among installed macros, lock losses, output mismatches, duplicated support and state-map violations; counts of rejected/unknown proofs | Zero unsound installs; refusals remain visible. |
| G4B.3 | Eligible, proposed, proved, installed and wake-used counts; peak e-graph size, p95 proof work, minimum realised savings among promotions | Give denominators. If no promotions, report count zero and no conditional sample, never a fabricated positive saving. Include finite unconditional net bits per episode. |
| G4B.4 | Minimum and p05 paired savings, p95 and maximum target prediction regret; per-term differences and fault-specific loss | Report both comparisons and both tails, not just mean compression. |

Use finite extrema helpers in [the statistics module](../src/stats.rs). No `Infinity`, JSON `null` masquerading as a measurement, or suppressed zero-opportunity task.

## 4. Verification Protocol & 100-Seed Confirmation / Holdout Ledger

Proposed allocation (inclusive endpoints below); reserve and enforce it in the CLI **before use**. Confirm disjointness against the seed ledger at registration. These are planned commands and banks, not results.

| Stage | Seeds / command | Required evidence |
| :--- | :--- | :--- |
| Smoke; exploratory census and proof pilot | `0..=19` smoke only; `20..=219` characterisation (10 × 20 development seeds) | Ungated opportunity distribution, counterexamples, replayable witness/control, full compute and bit ledgers. May reuse M4a development seeds for the paired audit. |
| Locked development | `2220..=2419` (10 × 20); proposed `verify-m4b --banks 10 --seed-offset 2220 --seeds 20` | Disjoint from census; registered stratified gates, zero exclusions, finite tails. |
| Confirmation | `20300..=20399` (5 × 20); proposed `verify-m4b --banks 5 --seed-offset 20300 --seeds 20` | Fresh; locked commit and configuration, same gates as development. No diagnosis on this bank. |
| Retained baselines, before **each** S3/S4 request | `python3 scripts/check_retained_baselines.py --require-clean`; additionally `cargo run --locked --release -- verify-m3 --banks 5 --seed-offset 20 --seeds 20` | Archive clean standard-tier JSON, goldens bit-identical, M2a/M2b/M4a pass; direct M3 free-rider/compression gates pass. The [manifest](retained_baselines.json) does not cover M3. No `--fast`, `--skip-golden` or dirty-tree substitute. |
| Single-shot holdout | Proposed `60100..=60199` (5 × 20); **only after S3**, `verify-m4b --banks 5 --seed-offset 60100 --seeds 20 --holdout` | One invocation on the locked clean revision; archive raw paired results, certificate bundle, per-task audit, inspected tails and source digest. No tuning or retry. |

Use [Charter §2.8](foundational_research_charter_v3.md#28-statistical-policy-holdout-governance-and-retained-baseline-regression-gate): `StratifiedGateAudit` for Clean and Noisy separately; within each stratum require the pooled 99% Clopper–Pearson and 1% homogeneity checks, and **each task's** exact binomial lower-tail and across-bank homogeneity at `0.01 / M_stratum` (all trials for exact-target gates). Require the per-seed Clean conjunction. A binding compute guardrail such as `SproutBlockedMaxNodes` excludes the case and blocks acceptance (`excluded_cases == 0` required); a depleted search allocation is an observable missed opportunity, never a passing discovery. Archive counts for both.

Reject exploratory/partial access to the proposed holdout; keep `10000..=10019`, `30000..=30019`, `40000..=40019`, `50000..=50019`, `50100..=50199` and `60000..=60099` permanently closed, even with `--holdout`. Seal the M4b range across CLI entry points with `allow_with_flag: false` and test overlap rejection on consumption. A failed holdout is consumed too.

## 5. Kill & Revisit Criteria

- **Opportunity:** No positive optimistic savings in the declared spontaneous search class, or none reachable with the canonical schedule: publish the census and stop at that boundary. A positive control only establishes that the apparatus works.
- **Discovery:** Only supplied/trivial identities, or no profitable, proved spontaneous wake-used macro in the registered experiment: H7 not attained. Report the first failing stage (discovery, proof, price, or deployment). Revisit exploration or informativeness, not the price of an observed failure.
- **Soundness or fairness:** One unsound install, loss of a required independent route, data leak, unmetered search, raw overrun or binding guardrail blocks acceptance. A counterexample rejects the candidate; unknown, timeout or unmappable state means no promotion. Preserve the witness and counters.
- **Attribution:** H7 can pass while C loses to A on Clean tasks. Record H5 as failed or unresolved under its own criterion; do not claim general sleep superiority or promise transfer. A failed confirmation or holdout goes to the Chief Scientist with a [blocker report](templates/blocker_report.md), not a revised threshold on the consumed data.

## 6. Parameter Inventory Changes

No constants or defaults change in this draft. At registration add the following to the [inventory](parameter_inventory.md) with final config paths, classifications, derivations, binding counters and development sensitivity results.

| Proposed item | Basis, telemetry and backward reduction |
| :--- | :--- |
| Palette/definition/call code and admissible interface | **Derived:** use a uniquely decodable extension of the [existing circuit code](../src/metrics/mdl.rs); record actual whole-graph bit deltas and break-even multiplicity. Empty palette reproduces current bits. Report sensitivities to interface size, overlap and number of live calls. |
| E-graph admission, extraction and proof budget | **Search bounds / compute guardrails:** derive from census interface size, finite valuation/state-space size and actual compute available. Count attempted matches, insertions/unions, extraction and checker transitions (including failed attempts); charge expanded primitive execution, not one cheap tick per macro. Publish saturation, truncation and `proof_unknown` counts. Vary bounds on development only. |
| Equal compute with logical work | **Pre-registered accounting:** retain exact substrate ticks and raw node-work ceiling. Before locking, demonstrate a deterministic conservative conversion of search/checker operations to charged work, debited from the paired wake-work allowance, or an independently validated common work budget all arms can spend. Report raw units and costs as well as totals. No defensible charge means no compute-matched claim. |
| Schedule, fact and evaluation windows, gate rates | Inherit M4a S-A grounding, fact buffer and derived observation horizon unless explicitly re-derived. Declare common prefix/horizon, usable sample counts, margin and `P_target` with development evidence. Consolidation off must reproduce existing behaviour and goldens. No ad hoc retirement clock or cross-width identity. |

## 7. Phased Implementation Plan

One commit per phase, with the named evidence linked here when produced. Keep consolidation opt-in. Behaviour-preserving phases must pass `cargo test --locked --test golden_traces`; changes to golden behaviour require a Rule-4 derivation and retained-suite evidence, not a fixture update to make CI pass.

| Phase | Deliverable | Decision / expected commit unit |
| :--- | :--- | :--- |
| 0. M4a reconciliation | Archive closeout evidence, compare R2 ledgers on development, record explicit S4 disposition. | Evidence/decision commit; no new holdout. Planning and read-only census do not confer progression. |
| 1. Opportunity | Instrument read-only snapshots, classify opportunities and missed onsets, cost positive/minimal controls at both widths. | Census code, tests and report. Stop if no viable spontaneous search class. |
| 2. Proof and deployment pilot | Small deterministic explorer, independent checker, priced palette entry and one reversible-to-primitives wake call. Exercise false, late, shared-state, zero-saving and budget-exhausted proposals. | Development pilot and certificate replay test. Require a working end-to-end replacement **and** plausible spontaneous opportunity before building the full verifier. |
| 3. Experiment lock | Exercise A/B/C, synchronised scoring and work metering on exploratory seeds. Publish measurements, proposed numeric gates and their feasibility, inventory, bank guards and decision record. | Pre-registration commit and explicit approval. Without both, stop; no confirmation. |
| 4. Registered verifier | Implement locked gates and rejection tests; validate M3 surgery, [M4a sleep](../tests/m4a_sleep.rs) and [oracle regressions](../tests/m4a_oracle.rs). Run locked development, then fresh confirmation and standard retained checks. | Verifier/evidence checkpoint; independent replay and per-task tail inspection before S3 request. |
| 5. Attainment | Only with S3, run the single-shot holdout, seal it and record both scientific verdicts in the [research record](research_record.md). | Closeout commit, clean retained checks, explicit S4 disposition. Hand a replayable reusable axiom to M4c only if supported. |

## 8. Sign-Off & Attainment Record

| Record | Status |
| :--- | :--- |
| M4a S4 and R2 qualification | Pending; sealed M4a holdout remains closed. |
| M4b census, proof certificate and independent replay | Not run. |
| M4b pre-registration approval / commit | Pending development evidence and Rule-9 thresholds. |
| Clean retained-baseline and direct M3 JSON; locked dev and 100-seed confirmation; Option D tails | Not run. |
| S3 authorisation; single-shot outcome; source commit and `source_tree_fnv1a128`; unconditional closure commit | Not authorised / not run. |
| H7 verdict; Clean H5 verdict; independent M4b review; S4 progression | Open. The independent review cited above concerns **M4a**, not this plan's attainment. |
