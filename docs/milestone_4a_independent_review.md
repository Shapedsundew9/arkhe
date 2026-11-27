# Independent review of M4a: what sleep established, and what should come next

**27 November 2026. Recommendation: plan M4b, but do not start confirmatory M4b runs.**
Keep consistency-only sleep (S-A) as the reference arm. First reconcile M4a closeout and run a short,
development-only study of whether the learner's *own* circuits contain anything worth promoting.
Do not reopen M4a's holdout. This review is not S4 sign-off or a change to a registered gate.

M4a crossed a useful boundary: a learner that already predicts and holds competing derivations can
sometimes use time without new observations to discard a worse model. It did **not** cross the next
boundary in [Charter §§0–1](foundational_research_charter_v3.md#15-core-research-question): deriving a
non-trivial equivalence, proving it, and making it a cheaper word in the learner's vocabulary. That is
why M4b is the right question and why a larger M4a falsification sweep is not the right next experiment.

## What the experiment established

The [pre-registration](milestone_4a_plan.md) separated spontaneous learning (SP), planted errors (PL),
and preservation controls (CT). All 36 task cells used one- or two-bit arithmetic. Characterisation
(`20..220`) preceded threshold registration (`4370540`); disjoint development, confirmation and the
single-shot holdout used clean `0f62e2f`. The registered analysis separates Clean and Noisy cases,
tests individual tasks across banks, requires the Clean per-seed conjunction and reports tails.
The counts below are **paired task-seed cases**, not independent new generator families.

| Registered outcome | [Dev (200 seeds)](evidence/m4a/verify_m4a_dev200.json) | [Confirmation (100)](evidence/m4a/verify_m4a_conf100.json) | [Sealed holdout (100)](evidence/m4a/verify_m4a_holdout_60000.json) |
| --- | ---: | ---: | ---: |
| G4A.1 accounting; G4A.2 isolation (both arms; each gate) | 7,200/7,200 | 3,600/3,600 | 3,600/3,600 |
| G4A.3 spontaneous advantage, S-A | 526/600 | 258/300 | 264/300 |
| G4A.4 target-port prediction non-inferiority, S-A | 5,599/5,600 | 2,799/2,800 | 2,799/2,800 |
| G4A.5 planted rejection, S-A | 1,580/1,600 | 786/800 | 785/800 |
| G4A.6 preservation, S-A | 7,195/7,200 | 3,598/3,600 | 3,599/3,600 |
| G4A.7 mean bits saved by S-A over S-B | +27.65 | +21.03 | +14.61 |

**H5: replicated on three pre-selected noisy periodic tasks, not on spontaneous learning generally.**
On holdout, `MV_Periodic` at widths 1 and 2 passed on 87 and 79 seeds; `Period6_LagRedundancy` at
width 2 passed on 98. Mean savings were 117.32, 104.14 and 60.65 bits respectively. The first two
tasks still had negative fifth-percentile savings (−38.76 and −46.57 bits). There is mechanistic
evidence as well as a score: on the period-6 task, the 200-seed exploratory report counted 195
spontaneous wrong-comparator refutations in the S-A arm versus zero in wake-only; the mean circuit
saving was 57.12 bits against a 54.54-bit total evaluation advantage. That supports pruning as an
explanation; it is not evidence of theorem discovery.

The restriction to 3 of 16 SP cells was explicit **before** confirmation, not a post-holdout rescue.
All eight Clean SP cells had zero observed incorrect-comparator opportunity at onset in
[characterisation](milestone_4a_plan.md#2-exploratory-characterisation-m4a-0--completed-089ed72-dr-50).
On holdout, six had zero mean S-A advantage. `ChannelFault:k2:clean` actually **lost** 564.89 bits
on average, despite zero target KT regret. The label means *clean before the fault*: the
[registry](../src/verify_m4a/tasks.rs#L405) substitutes channel 0 with probability 0.75 from
environment tick 1,000 at width 2, and runs this SP case for 2,000, not 8,000, substrate ticks.
Retain it as a separate stress case; neither harmlessness nor universal superiority follows from the
pooled result.

**H6: the operational choice of S-A is justified; “facts are unnecessary” is not.** The +14.61-bit
holdout mean clears the registered +10-bit decision margin over three SP and eight PL cells. The
largest contribution was `PL2_RareContext:k2:noisy` (+145.69 bits; 85/100 S-A vs 66/100 S-B
rejections). The three spontaneous S-B-minus-S-A means were −4.52, +2.57 and −1.86 bits. The
[decision rule](../src/verify_m4a/verify.rs#L1289) selects a schedule on that mixture, not a winner
on every task. S-A drops the separate teacher-forced replay/adjudication phase; it still uses wake
facts for [prediction-consistent loopback entry](../src/structural/learner.rs#L3310). Retain S-B as
a regression comparator, not the default for M4b.

**Preservation was strong, not absolute.** S-A lost no oracle-correct port locks on holdout; one
noisy case lost an oracle-correct node. Clean preservation was 1,800/1,800. No case was excluded for
`SproutBlockedMaxNodes`, but the report records 83,072 `sprout_budget_exhausted` events. Zero
exclusions does not mean unlimited search, and empirical preservation is not a proof of replacement
equivalence.

## Implementation and execution: where the claims stop

1. **Equal ticks are verified; strict raw node-work parity is not what the gate measures.**
   The [paired runner](../src/verify_m4a/explore.rs#L2198) obtains a real wake budget, forks at sleep
   onset and pauses the environment. G4A.1 reports zero tick difference and a maximum conservation
   error of about `1.31e-13` bits/symbol. But when only live-step work increases and neither replay nor
   sensitivity work increases, the [ratio helper](../src/verify_m4a/explore.rs#L2463) can divide by
   **sleep live work plus wake active work**, rather than actual wake total. This can conceal a raw
   overrun under the [stated R2 rule](parameter_inventory.md). In a fresh seed-0
   `ChannelFault:k2:clean` pair, S-A used 14,090,758 evaluations to wake's 14,290,901: raw ratio
   **0.985995**, reported **0.985131**. This example is *not* an overrun; it demonstrates that the
   reported maximum of 1.0 cannot certify raw parity. Report both ratios per pair on development
   data. If the literal no-extra-work condition fails, qualify H5 and resolve R2 explicitly rather
   than relaxing it retrospectively.
2. **The frozen score is comparable in length, not necessarily identical in data.**
   The [scorer](../src/verify_m4a/explore.rs#L750) clones each arm's *terminal* environment and scores
   its next 256 observations. In the seed-0 noisy width-1 `MV_Periodic` pair, S-A forwent 3,840
   observations; its noise stream and generator position need not match wake's. The score is circuit
   bits plus the sum of KT fact-code bits over output ports, with zero extra palette entries. It is
   neither the cumulative conservation identity nor the full four-term promotion objective in
   [Charter §3.4](foundational_research_charter_v3.md#34-a-global-objective-for-the-local-economy).
   Keep this operational result. For causal attribution add a common, independent fact sequence,
   identical synchronisation protocol and trace digest for both frozen models. Report circuit,
   prediction and corroboration changes separately; keep facts forgone visible as a real cost.
3. **Episode success and sleep-caused refutation are different observations.**
   [G4A.5](../src/verify_m4a/verify.rs#L995) accepts excision or extinction within its segment bound
   without wrongful excision. The [event tracker](../src/verify_m4a/explore.rs#L1768) can mark success
   during wake, and the [end check](../src/verify_m4a/explore.rs#L2079) can mark absence at episode
   end. Likewise, a [no-onset arm](../src/verify_m4a/explore.rs#L2244) reuses the wake result:
   agreement there is by construction, not an independent replication. The separate
   [sleep](../tests/m4a_sleep.rs) and [oracle](../tests/m4a_oracle.rs) tests matter: all 22 and 7
   respectively passed in this review, including remapped wake-register restoration, frozen wake
   scorers, zero flux for suspended comparators and planted refutation. In 4b record when each
   candidate was present, refuted, proved and promoted; compare with a same-schedule arm without
   consolidation. Do not count ordinary wake pruning as a theorem.
4. **Holdout closure, regression evidence and approval are separate.** The
   [pre-S3 report](evidence/m4a/retained_baselines_pre_s3.json) covers golden traces, M2a and M2b.
   Commit `3582242` [sealed the consumed range](../src/stats.rs#L775), with
   [CLI rejection tests](../tests/cli.rs#L405). I also inspected the local
   `retained_baselines_closeout_3582242.json`: standard tier, `require_clean=true`, `git_dirty=false`,
   golden tests not skipped, M2a/M2b/M4a passed, no errors (SHA-256
   `deb49acab0feaece1f4f627e5e2736bc64755e160ddbd67d69ebe015d69365c0`). It is not in the
   [tracked archive](evidence/m4a/), and the [attainment record](milestone_4a_plan.md#8-sign-off--attainment-record)
   still says S4 pending. Archive that report and reconcile the explicit S4 decision, including the R2
   qualification, before formal progression. The [retained manifest](retained_baselines.json) does
   not run M3: a green check is not evidence that a new macro preserves M3 free-rider resistance.

## Recommended sequence for M4b

| Step | Deliverable and decision |
| --- | --- |
| **1. Establish compression opportunity on development data.** | Census repeated subgraphs with small interfaces in unmodified, spontaneously learned Clean circuits at both widths. Record multiplicity, state and latency, possible circuit savings, palette definition and call costs, and missed candidates when no comparator triggers sleep. Use planted repeats only as a positive control; use already-minimal circuits as a negative control. If no spontaneous candidate can repay its definition even under optimistic accounting, revisit the task family or candidate boundaries *before* building a large search engine. |
| **2. Demonstrate one auditable replacement end to end.** | Separate learner-side discovery from an independent proof checker. Specify width, ports, fan-out, initial/live state mapping and per-tick latency. Start with exact finite input enumeration on small combinational cones; for stateful rewrites require sequential equivalence for admissible inputs and mapped states, including `DELAY` and `TicksPerHop`. The existing [bounded surgery checker](../src/structural/surgery.rs#L3106) is prior art; [eventual correctness on a generator orbit](../src/baselines/exhaustive.rs#L342) is not the same theorem. A counterexample, timeout or exhausted proof budget means no promotion. Test rejection with `x − y = y − x`: true at width 1, false at width 2 for `(1,0)`. |
| **3. Pre-register the H7 experiment only if steps 1–2 succeed.** | Compare wake-only, canonical S-A without consolidation, and S-A with charged search/proof/promotion. Match substrate ticks, expose raw work, and meter logical work rather than treating e-graph or solver time as free. Predeclare what counts as *discovered*: a non-trivial equivalence from held circuits, not a supplied rewrite read back or a planted identity. Separate the H7 proof-and-promotion endpoint from the deferred H5 Clean superiority endpoint. |

For each proposed promotion, retain a replayable certificate, the original subgraph, the replacement
and the bit ledger. Apply [§4.3](foundational_research_charter_v3.md#43-from-retention-to-promotion)
literally: unrefuted; fact matching recorded under S-A (required under S-B); strict reduction in
total epistemic description length **after** paying for the palette; and exact equivalence. Preserve
independent comparator routes: two calls sharing the same internal evidence must not earn double
corroboration. A shorter printed graph or one passing fact window meets neither the accounting nor
the proof requirement. Show that a frozen macro expands back to its proof object and can be used in
wake; leave claims about transfer speed on *new* families to M4c.

Before confirmation, fix tasks, baselines, proof/search budgets, feasibility-backed thresholds and
stop conditions. Preserve the prior suites, including a direct M3 regression for surgery changes;
retain per-task Clean/Noisy tails, zero exclusions and the clean baseline check. Use fresh disjoint
100-seed confirmation and single-shot holdout banks under [§2.8](foundational_research_charter_v3.md#28-statistical-policy-holdout-governance-and-retained-baseline-regression-gate).
If only supplied identities are recovered, or no verified spontaneous promotion pays for itself,
record that negative result and revisit discovery or the code lengths. Do not lower the price of a
macro to make it win. M4c needs a reusable axiom first; M5 width expansion and M6 action answer
different questions. None removes the present bottleneck.

**Review basis.** Source reviewed at `b75a839`; the holdout figures above are independent reads of
the archived summaries, not a fresh holdout run. Development checks were
`cargo test --locked --test m4a_sleep --test m4a_oracle` and
`cargo run --locked --release -- explore-m4a --seed-offset 0 --seeds 1 --threads 2`.
The `20..220` mechanistic comparison uses the original exploratory report named in the plan; its
source digest matches `089ed72`. No historical gate, threshold, seed allocation or implementation
was changed for this review.
