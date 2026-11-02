# Milestone 0 Review: Simulator and Yardstick

**Review date:** 2026-11-01  
**Scope:** The working-tree implementation, the [Foundational Research Charter v2](foundational_research_charter_v2.md), the [approved M0 plan](milestone_0_plan.md), and the [implementation lessons](milestone_0_lessons_learned.md).

## Executive judgment

**Milestone 0 now appears to have achieved the Charter-level goal, with one remaining gap in the amended verification matrix.** The closeout materially repairs the earlier measurement and configuration problems, and both the development and reserved holdout runs pass all four gates. This is credible evidence that the scoped T1 baselines reach their registered optima and that the tested configuration matrix is reproducible across processes.

The qualification is that G2 checks noisy entropy rate but does not check noisy \(C_\mu\), although the amended plan explicitly requires both. In addition, the verification reports mark noisy multi-register CGP outcomes as characterization, not enforced gates, and the runs record a dirty worktree without a digest of its exact contents. These limitations narrow the closeout claims; they do not overturn the positive Charter-level verdict.

## Closeout reassessment

The [closeout plan](milestone_0_closeout_plan.md) addresses the substance of the original findings without pulling later milestone work into M0:

| Initial finding | Current assessment |
| :--- | :--- |
| Finite-prefix recurrence and synthesis results presented as exact | **Materially addressed.** Ground-truth reporting distinguishes prefix results from certification, and synthesis now certifies candidates across the joint generator/circuit cycle within a bounded search. Period-257 and independent-witness tests exercise the former failure mode. |
| Boundary lag, state lower bounds, and node minima conflated | **Addressed for the revised gate.** Effective state accounts for delay, wire-hop, and boundary-lag registers; node and state objectives are reported separately, with the width-2 motif witnesses documented. |
| Unknown entropy reported as zero; loss measures conflated | **Addressed.** Ground-truth export failures remain explicit, and probabilistic log loss, KT residual loss, and paired oracle regret are separate measures. |
| Topology configuration silently ignored | **Addressed.** Unsupported non-well-mixed experiment topology is rejected rather than recorded as if enforced. |
| Replay phase depended on a fixed trim quantum | **Addressed for arbitrary-period windows.** Replay tracks the prior frame and aligns autonomous state to the observation window; acceptance tests cover periods 5 and 7. |
| Acceptance coverage, holdout evidence, and reproducibility scope were unclear | **Substantially addressed.** The report contains structured per-case results and build provenance; G4 launches separate OS processes across a six-configuration matrix. Noisy multi-register CGP remains explicitly characterization-only. |

### Remaining acceptance and evidence limits

1. **Noisy \(C_\mu\) is not asserted in G2.** The amended plan calls for analytical \(C_\mu\) and \(h_\mu\) checks under both noise settings for every listed generator. In [the G2 implementation](../src/verify.rs#L393), clean cases assert both quantities, but the noisy branch asserts only `h_mu_bits_per_symbol`; the computed noisy `c_mu_bits` is never compared with its analytic expectation. Therefore, the gate passing does not establish that part of the amended criterion. This is a narrow verification gap, not evidence that the reported \(C_\mu\) is wrong.
2. **Noisy multi-register CGP is characterization, not acceptance.** Each run reports those outcomes, but marks them `gate_enforced: false`. The amended G3-rev1 explicitly scopes enforceable noisy CGP thresholds to constant and alternating T1 tasks, so neither the pass nor the verdict should imply that CGP meets a noisy threshold on every T1 task.
3. **Provenance identifies a dirty build, not its complete source snapshot.** Both reports record the same commit SHA and lockfile digest, but also `git_dirty: true`. They identify the base commit and disclose local modifications; they do not provide a digest of the complete modified source tree. Results below are therefore evidence for the tested working tree, but are not independently reconstructible from the commit SHA alone.

### Validation and measured outcome

On 2026-11-01, these checks passed against the working tree:

| Check | Result |
| :--- | :--- |
| `cargo fmt --check` | Passed |
| `cargo clippy --locked --all-targets --all-features -- -D warnings` | Passed |
| `cargo test --locked` | Passed: 27 unit, integration, and acceptance tests |
| `cargo run --locked --release -- verify-m0 --seed-offset 0 --seeds 20` | Passed: G1–G4 true, no failures, 40 case reports |
| `cargo run --locked --release -- verify-m0 --seed-offset 10000 --seeds 20` | Passed: G1–G4 true, no failures, 40 case reports |
| `cargo package --allow-dirty` | Passed: package built and verified |
| `npx --no-install markdownlint-cli2 "doc/**/*.md" README.md` | Passed after this reassessment edit |

The two `verify-m0` reports include populated toolchain, target, commit, dirty-state, and lockfile provenance. All enforced case reports passed. The holdout report includes the required noisy CGP threshold result of at least 18/20 on each enforced constant/alternating case; unenforced multi-register noisy CGP results remain explicitly reported as characterization.

**Conclusion:** M0 meets the Charter’s scoped success criterion on the evidence available: the required T1 baseline suite reaches its amended known optima, and cross-process reproducibility passes. For strict sign-off against every detail of the closeout plan, add the missing noisy \(C_\mu\) assertion (or explicitly amend that criterion) and retain a source-tree digest when recording results from a dirty tree. Neither requires significant scope expansion.

## What M0 was meant to achieve

The charter's [M0 milestone](foundational_research_charter_v2.md#7-milestones) is infrastructure for later experiments: exact synchronous tick semantics and structural invariants, variable-width values, seeded determinism, a generator library with computational-mechanics and shift-register ground truth, and a baseline harness. Its stated success criterion is that **baselines reach known optima on T1 and every run is reproducible from its seed and configuration**. It is not a test of whether growth, metabolism, sleep, or theorem discovery works.

The [plan](milestone_0_plan.md#10-pre-registered-milestone-0-acceptance-criteria) sensibly made this more concrete with G1-G4, two mandatory widths, and explicit seeds and margins. Those plan commitments are stronger than the charter's short M0 row. They should be assessed separately rather than treating a passing implementation of a gate as proof that the registered gate was met.

| Charter concern | Assessment |
| :--- | :--- |
| Tick semantics, invariants, and variable width | **Substantially present.** The substrate has typed width validation, explicit synchronous state, invariant checks, and scalar and bit-parallel evaluators. Tests exercise representative violations and compare 64 packed lanes with scalar execution. |
| Seeded, reproducible runs | **Strong evidence for the tested matrix.** Domain-separated seeded streams and serialized configurations are implemented. G4 reproduced byte-identical JSON in separate processes across its six-configuration matrix; this does not establish every configuration or platform. |
| Generators and ground truth | **Substantially established, with a G2 assertion gap.** T1, counters, and LFSRs have analytical and independent small-case checks; recurrence and synthesis reporting now distinguish prefixes from certified results. However, noisy \(C_\mu\) is not asserted in G2, and noisy synchronization remains a bounded estimate. |
| Baseline harness and T1 optima | **Achieved for the amended, enforced T1 scope.** Development and holdout runs pass the revised optima checks. Noisy multi-register CGP is characterization-only, and the candidate-replay budget is not a common compute limit across all baseline types. |

## What went well

- **The core separation is sound.** Keeping exact arithmetic and evaluation in the [substrate](../src/substrate/mod.rs), spatial placement in the [lattice](../src/structural/lattice.rs), and experiments and measurements outside both provides useful seams for M1a.
- **The Boolean and graded cases received real attention.** Width-64 masking, synchronous latching, explicit `SPLIT` fan-out, and scalar versus 64-lane differential tests are valuable safeguards, not just scaffolding. The hand-built mod-3 counter and its dynamics test are a useful demonstration of `EQZ` at width 2.
- **The ring issue was taken seriously.** Substituting Reeds-Sloane for field-only Berlekamp-Massey at larger widths is a well-motivated decision. In addition to the repository's tests, an independent brute-force review probe matched its minimum lengths and returned taps for all 65,536 width-2 sequences of length 8. It also checked 4,368 generated width-2 recurrences of lengths 1-3 over a finite horizon after `2L` observations. This is substantial small-case evidence, not a proof for every width or length.
- **The lessons identify genuine experimental hazards.** Boundary-provided lag, initial pipeline values, residual coding, and phase-sensitive replay are exactly the kinds of details that could otherwise invalidate later comparisons.
- **The RNN/LSTM deferral is reasonable.** The charter lists it as a research baseline but does not require a neural training stack to answer M0's infrastructure question. Keeping that work for the first milestone that needs the comparison avoids unnecessary dependencies now.

## Initial-review findings (historical; reassessed above)

### 1. The passing gates are narrower than the registered gates

The command reports four passing booleans, but the checks behind them do not cover the complete [plan's G1-G4 matrix](milestone_0_plan.md#10-pre-registered-milestone-0-acceptance-criteria):

| Gate | What is actually checked | Important difference |
| :--- | :--- | :--- |
| G1 | Seven widths and 100 sampled circuits, each compared for 1,000 ticks across 64 scalar lanes. | The gate itself does not run the negative invariant cases. Some are covered separately by `cargo test`, but that is not the complete designated-error matrix described in the plan. |
| G2 | One noiseless mod-3 epsilon-machine case, one binary length-12 recurrence, and one width-2 length-8 recurrence. | The plan calls for analytical checks across T1, T3b, and small T3c, both noiseless and noisy. Separate unit tests cover additional examples, not that full matrix. See [G2](../src/verify.rs#L156) and the [epsilon-machine tests](../src/env/mod.rs#L20). |
| G3 | Four noiseless T1 cases at both widths; CGP must pass at least 95% of the requested seeds. Noisy CTW and CGP are checked only on alternating streams, using a **mean** excess-loss upper bound. | This does not establish a per-seed noisy margin or noisy performance across the whole T1 suite. Its state check is also weaker than the plan's stated equality; see finding 2. |
| G4 | A TOML round trip and two executions of one configuration in the **same process**. | The plan calls for separate-process byte identity. The independent process check in this review is encouraging, but covers only one configuration. |

The lessons' ["40/40 seeds passing" wording](milestone_0_lessons_learned.md#43-trap-3-the-deceptive-noisy-echo-local-minimum-and-mdl-crossover) is therefore stronger than the reported evidence: G1, G2, and G4 are not evaluated per seed, and noisy G3 checks an aggregate mean, not 40 individual passes. This does not imply the thresholds were changed; it means the result should be described at the granularity actually measured.

### 2. State lower bounds, minimum-size circuits, and boundary memory are conflated

The plan asks the exhaustive baseline to match `ceil(C_mu / k)`. [G3 instead rejects only values below that bound](../src/verify.rs#L275), counting one unit of boundary-provided lag when an input is used. It never requires equality.

A concrete registered case exposes the distinction. For the width-2 motif `[0, 1, 3]`, `C_mu = log2(3)` and the reported lower bound is one width-2 register. The passing exhaustive run returns a **two-`DELAY`** circuit, plus the boundary lag counted by G3. Thus its effective count under G3's own rule is three, not one, while the gate still passes. The circuit has zero post-warmup prediction errors; the issue is the claimed state criterion, not its predictive success.

The [lessons' boundary-lag observation](milestone_0_lessons_learned.md#21-causal-prequential-inputs-carry-1-register-of-boundary-lag) is important and should be retained. But the [scorecard](../src/baselines/mod.rs#L215) subtracts `C_mu` from internal `DELAY` bits alone: a correct width-1 alternating predictor with zero internal delays reports `-1` excess state bit because its input supplies the missing lag.

More fundamentally, an information-theoretic lower bound is not automatically an attainable minimum under a particular primitive set and interface, and a minimum-**node** predictor need not also minimize **state**. The plan should distinguish those objectives, specify which external and wire-delay state is counted, and require equality only when an attainable witness is part of the accepted criterion.

### 3. "Exact ground truth" includes results that are not established as exact

The [synthesis oracle](../src/baselines/exhaustive.rs#L243) checks a finite clean trace, searches only up to a node cap, and enumerates `DELAY` nodes with zero initial state. It cannot, on that basis alone, certify the smallest valid circuit for the complete generator. The recurrence length in the [yardstick](../src/config.rs#L214) is likewise inferred from a bounded prefix.

For example, the supported width-1 periodic motif consisting of 256 zeros followed by one `1` has period 257. The `yardstick` command examines 256 symbols and reports both recurrence length **0** and exact synthesis minimum **0**. At the next tick the symbol is `1`, so the zero-node echo that fits the examined prefix fails. These values describe that prefix, not a proven optimum for the periodic process. The default T1 gate cases are much shorter; this example demonstrates why the public claim needs a clear validity boundary, not a demand to solve arbitrary large synthesis problems.

There are related reporting hazards:

- If a generator's explicit state table exceeds its cap, [the export error is discarded](../src/config.rs#L226), and the [scorecard substitutes `h_mu = 0`](../src/baselines/mod.rs#L203). In a tested noisy width-2 length-9 LFSR, the epsilon-machine was `null` but the reported excess equaled the full loss, even though the declared substitution noise alone has conditional entropy about `0.365645` bits/symbol. Unknown is not zero.
- The noisy synchronization estimate [merges quantized beliefs and has a cap](../src/env/epsilon_machine.rs#L216); it should be identified as a bounded estimate, not an unrestricted exact result. A curriculum's exported epsilon-machine is for its [final stage](../src/env/tiers.rs#L580), not a description of its entire changing training schedule.
- The CLI accepts `yardstick --epsilon=-0.05` and reports a noiseless result because [negative values select `NoiseModel::None`](../src/lib.rs#L131), bypassing the noise validator.

### 4. The harness is not yet a general equal-compute or common-loss comparison

The shared budget charges candidate-history replay, but [CTW](../src/baselines/ctw.rs#L302) and [Reeds-Sloane](../src/baselines/bm_predictor.rs#L37) ignore it, as do ordinary prediction steps and some setup work. In a width-1 alternating run with a budget of zero, both CTW and Reeds-Sloane learned with zero post-warmup mismatches and reported zero consumed ticks; CGP could not search and made 160 mismatches. This is a useful **candidate replay budget**, but not evidence of equal compute across different algorithms.

The loss numbers also have different constructions: CTW supplies prequential probabilistic log loss, while circuit predictors are scored from their point-prediction residuals using a KT-style code, with zero mismatches explicitly assigned zero bits. See the [harness scoring](../src/baselines/mod.rs#L195). Both can be useful, but a common unit of bits/symbol does not by itself make them the same predictive likelihood or make the `0.02` margin directly comparable.

The charter requires equal compute for **sleep comparisons**, which are later work. M0 need not solve cross-architecture accounting completely, but it should not describe the current accounting as a general compute-matched comparison. Before such comparisons are used, define the cost and loss protocols or report the measures separately.

### 5. A recorded topology setting does not control experiment topology

The lattice API enforces geometry for manually built lattice circuits. However, [experiment configuration](../src/config.rs#L285) passes width and wire-delay mode to the baselines but does not apply `wiring_topology`; baseline scoring also uses well-mixed wiring. In a separate-process probe, otherwise identical noisy CGP configurations using `WellMixed` and `LocalVonNeumann` produced identical results after removing the configuration and its digest from the reports.

This does not invalidate the lattice implementation itself. It does mean that a `run` report recording a local topology is not evidence that local wiring was enforced. Rejecting unsupported non-well-mixed experiment settings, or clearly marking them unsupported until wired through, is a small and safer M0 response than building the later locality ablation now.

### 6. The phase workaround is narrower than the generator API

Trimming history in multiples of 24 preserves the phase of cycles whose periods divide 24, including the planned short motifs. It does **not** preserve arbitrary periods such as 5 or 7, although the generator accepts arbitrary nonempty motifs. See the [CGP trim](../src/baselines/cgp.rs#L24) and the [generator definition](../src/env/tiers.rs#L80). Retain the useful lesson, but describe the workaround as scoped to those periods or preserve the actual replay state/phase before relying on broader tasks.

## Assessment of the plan and execution

The plan's strengths were its explicit non-goals, layered interfaces, deterministic seed design, and quantitative gates. The staged design and the choice to test width 2 early are particularly good defenses against accidentally building a Boolean-only platform.

The plan also made M0 harder than the charter required. It combined a general-width simulator, spatial lowering, multiple search baselines, noisy computational mechanics, exact synthesis, recurrence synthesis over rings, and statistical performance thresholds in one delivery. Several of these are independent research-sized risks. A thinner first vertical slice at widths 1 and 2, followed by independently checked yardsticks and then additional baseline capabilities, would have exposed the same integration risks with less simultaneous uncertainty.

The most consequential planning ambiguity was treating theoretical state bounds, finite-trace fit, exact circuit minimality, and near-optimal noisy loss as if they were interchangeable notions of an optimum. They need separate definitions and witnesses. Similarly, the lessons show CGP pacing and mutation operators were refined against the same G3 seed set. That is legitimate development, but those seeds should not also be treated as independent evidence of generalization. Keep the original registration visible and record any change to coverage or interpretation explicitly.

## Initial-review verification (historical)

All commands below ran against the current working tree with Rust `1.99.1`:

```bash
cargo test --locked
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo run --locked --release -- verify-m0 --seeds 20
```

The 20 unit and integration tests passed; formatting and Clippy passed; the verification command reported all four gates passing. These results validate the checks that ran, not the broader claims identified above.

Additional exploratory checks used the same G3 configurations but seeds `20..=39`. CGP had zero post-warmup errors on all 20 seeds for each of the four noiseless T1 cases at each width. For the noisy alternating case, the current scorecard yielded:

| Width | Baseline | Mean excess bits/symbol | Individual runs above `0.02` |
| :--- | :--- | ---: | ---: |
| 1 | CTW | 0.010872 | 5/20 |
| 1 | CGP | -0.000033 | 1/20 |
| 2 | CTW | 0.018848 | 11/20 |
| 2 | CGP | -0.000122 | 5/20 |

These are **post hoc exploratory results**, not a new preregistered pass/fail test. Negative realized excess can occur on a finite sample and is not, by itself, a defect. The table illustrates both encouraging average performance and why an aggregate mean is not a per-seed guarantee; it also inherits the different loss definitions described above.

## Original closeout recommendations (historical)

1. **Make unsupported or approximate results explicit.** Do not substitute zero for unknown entropy; validate noise arguments; label finite-prefix recurrence and synthesis results and bounded noisy synchronization accordingly. For the small T1 targets, use independent witnesses and a complete, explicitly bounded check of the relevant circuit class and generator cycle before calling a size minimum exact. A general-purpose formal solver is not required for this step.
2. **Resolve the state criterion before rerunning G3.** Agree on internal versus boundary and wire state, distinguish a lower bound from an attainable optimum, and decide whether minimum state and minimum nodes are separate goals. Then either enforce the original criterion with witnesses or record an approved, prospective revision; do not silently relabel the weaker check as the original gate.
3. **Make the verification report match the accepted matrix.** Exercise the agreed G1/G2 cases, all intended T1 conditions, and separate-process reproducibility; report per-case and per-seed outcomes as well as any mean. Reserve a fresh seed set before the closeout run. Record the exact source revision or working-tree snapshot, lockfile, toolchain, and configuration alongside results; a configuration digest alone does not identify the implementation.
4. **Protect the existing experimental surface.** Reject or clearly mark topology settings that the harness cannot enforce, and label the current budget and loss measures accurately. Use a common, declared comparison protocol when later experiments actually require one. Limit the phase-alignment claim to supported periods or make replay phase-aware.

This is a focused repair and evidence task within the existing M0 design. It does **not** require adding the RNN/LSTM baseline, new task tiers, growth, sleep, theorem discovery, or action to M0.
