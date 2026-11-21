# Project Guidelines

Read `.shared/ai/core/service-instructions.md`.
Read `.shared/ai/core/common-instructions.md`.

## Freedoms

- Backwards compatibility is **NOT** necessary.
- Breaking changes are allowed.
- The project prioritizes flexibility, performance and innovation over compatibility.
- No implementation or dependency is sacred; everything can be changed if necessary.
- Nothing depends on the contents of this repo being stable or fixed.
- If something needs cleaning up or refactoring to reduce future effort, do it.

## Requirements

- If something needs to be done more than once, it shall be mechanically automated if viable.
- If a persistent inefficiency in the development process is identified, it shall be addressed systematically rather than repeatedly worked around.
- **NEVER, EVER** search for magic numbers. If you find yourself tuning a value to get tests or benchmarks to pass - STOP immediately & discuss it with the user.

## Repository Layout

- This is a pure Rust package repository.
- The crate/package project is configured by the repository-root `Cargo.toml`.
- Reusable package code belongs in `src/lib.rs` (and submodules under `src/`).
- Executable entry points belong in `src/main.rs` (or `src/bin/`).
- Integration tests belong in `tests/` and should import the package as `arkhe`.
- Put one-off developer utilities or scripts in `scripts/`.
- Keep build artifacts such as `target/` out of commits unless the repository explicitly tracks them.
- Be explicitly aware that GEMINI.md is a symbolic link to .github/copilot-instructions.md

## Rust Development

- Use the official Cargo toolchain (stable Rust) managed via `rustup`.
- Build and verify the project from the repository root with `cargo check` and `cargo build`.
- Declare runtime and development dependencies in `Cargo.toml`; do not add ad hoc dependency files.
- Keep the supported Rust edition, version (`rust-version`), and package metadata in `Cargo.toml` accurate.
- Follow standard code style and public APIs (`cargo fmt`, `cargo clippy`).
- Never hard-code, print, or commit credentials, API keys, tokens, or other secrets.

## Validation

- Run Rust tests with `cargo test`.
- Run linter checks with `cargo clippy --all-targets --all-features -- -D warnings`.
- Check formatting with `cargo fmt --check`.
- When changing packaging metadata or preparing for release, package the crate with:
  `cargo package`.
- Inspect the generated package archive in `target/package/` when packaging changes affect included files or binaries.
- Run focused checks for the files and behavior changed before running broader validation.
- Keep intentional Markdown exceptions narrow.

## Dependencies

- Prefer well-established, maintained crates when a dependency is genuinely needed.
- Add every new dependency to the appropriate section of `Cargo.toml` and verify with `cargo check`.
- Avoid introducing a dependency for functionality that is small and clear to implement with the standard library.

## Architectural Integrity & Parameter Discipline

1. **Topology is coordinate-free.** `substrate::Circuit` is the only topology representation.
   Do not reintroduce grid shapes, coordinates, relay hops, or perimeter anchors. Locality and
   fan-out are priced only through description length (`L(C)`, `split_code_tax`).
2. **Parameter classification and evolution of scaffolding constants into derived scaling laws.**
   Every constant, bound, or threshold in `src/structural/` and `src/verify_*/` is classified in
   `docs/parameter_inventory.md` (migrated from retired `doc/coordinate_free_scaling_plan.md` §2.1)
   as **Derived**, **Milestone-Scoped Scaffolding**, **Pre-registered**, **Search bound**,
   **Compute guardrail**, or **Numerical guard**. Adding or changing a constant requires updating
   that inventory. Early milestones may fix **Milestone-Scoped Scaffolding** constants (e.g.,
   `juvenile_ticks = 128`, `diagnostic_window = 64` calibrated at $K=1, \tau \le 3$) to keep initial
   experiments tractable. Scaffolding constants are **not** immutable across new scale axes
   ($\tau, K, k, N$). When a later milestone expands a scale axis and a scaffolding constant binds
   for a reasoned structural or mathematical cause (e.g., $\gamma^{\tau-1}$ credit attenuation,
   $\tau$-tick pipeline fill latency, alphabet size $2^k$, or description debt $\Delta L_v$), the
   **primary remedy** is replacing the fixed scalar with a **reasoned Derived Scaling Law** (no
   magic numbers—every constant or scaling relationship must have a documented, reasoned explanation
   for why it is what it is)—never weakening the benchmark task, narrowing a gate, or tuning an
   arbitrary magic number.
3. **Every limit is observable.** Any cap, guardrail, clip, or window truncation must emit a named
   telemetry counter when it binds. Guardrails must never bind in an accepted run.
4. **Golden traces serve the science and never block derived scaling laws.** Behavior-preserving
   refactors and backward-reducing extensions are proven by golden traces (`tests/golden_traces.rs`).
   Golden traces guard against accidental regressions; they must **never** block replacing a
   Milestone-Scoped Scaffolding constant with a Derived Scaling Law. Prefer derived scaling laws
   that reduce identically to the earlier milestone's value at that milestone's scale
   ($K=1, \tau \le 3$), which keeps golden traces bit-identical automatically. When a more
   fundamental derivation legitimately changes earlier-milestone trajectories while passing the
   retained scientific regression suites (`verify-m2a`, `verify-m2b`), re-blessing
   `tests/golden_traces.rs` with the written mathematical derivation is the expected scientific path.
5. **Stop and escalate** through the **two-level escalation hierarchy** (Rule 7) when any of these
   occur:
   - a structural limit, guardrail (`SproutBlockedMaxNodes`, `SproutBudgetExhausted`), starvation
     cliff, search bound, or milestone-scoped scaffolding constant binds;
   - a pre-registered gate fails on development, confirmation, or holdout seeds and the cause is not
     an implementation bug with a demonstrable fix within 2 attempts;
   - a fix would add, remove, or re-derive a constant, a threshold, a gate, a test assertion, or a
     golden fixture;
   - two consecutive fix attempts for the same failure brought no new diagnostic understanding;
   - the charter, a milestone plan, the inventory, and the code disagree;
   - a theoretical argument suggests the target is unreachable under the current rules.
6. **Multi-bank acceptance uses pooled confidence bounds plus a homogeneity test**, never compound
   per-bank hurdles.
7. **Decision rights, Charter-Core Invariants & two-level escalation hierarchy (see Charter §7.1).**
   - **Charter-Core Invariants (Immutable without User / Principal Investigator Sign-Off):**
     1. **Overarching Charter purpose & epistemological premise ([Charter §§0–1](../foundational_research_charter_v3.md#0-purpose-in-one-paragraph)):**
        Starting from minimal exact arithmetic primitives with no gradients, no pre-trained
        representations, and no outside grader beyond environment facts; fallibilist truth
        (disagreement conclusive, agreement provisional and null-surprisal-weighted); wake empirical
        prediction + redundant derivations/comparators; sleep isolated deduction + formal theorem
        compilation into higher-order axioms so the learner's vocabulary grows open-endedly and
        every level of the hierarchy is auditable.
     2. **Holdout Seed Bank Isolation:** Pre-registered single-shot holdout banks (`10000..=10019`,
        `30000..=30019`, `40000..=40019`, `50000..=50019`, `60000..=60019`, and future milestone
        holdouts) are never inspected during development or used to tune thresholds.
   - **Implementation Workers (`gemini-4-hydrogen-high` / `gemini-4-hydrogen-xhigh`, phase authors & `@verifier`):**
     Always spawned fresh (**never** `fork: "coordinator"`). Strictly bound to the parameters,
     formulas, file scope, and test specifications in their brief. Maximum **2 fix attempts** on any
     failing test; **never** weaken test assertions, alter generator parameters (`add_b`, `lag`,
     `Width`), mask or suppress guardrail counters (`SproutBlockedMaxNodes`,
     `SproutBudgetExhausted`), or edit out-of-scope files to make a test pass. When escalating
     (`@coordinator [escalation]` + `swarm_wait{mentions_only: true}`), the worker **must** present:
     (i) the root cause or exact failing observation (file, line, metric), (ii) **2–3 concrete
     options with trade-offs** aimed at an optimal or near-optimal solution, and (iii) whether any
     guardrail/limit bound is due to substantively suboptimal learner/harness behavior vs. genuine
     problem-class scale expansion.
   - **Operational Watchdog (`@monitor`, `gemini-4-hydrogen-high` with
     [`.github/skills/arkhe-watchdog/SKILL.md`](../../.github/skills/arkhe-watchdog/SKILL.md)):**
     Parallel operational sidecar (not an escalation decision level). Wakes on a 10-minute timer
     (`schedule`), uses a compact stateless delta check without dumping code or transcripts into its
     context, verifies workers run on `gemini-4-hydrogen-*`, and halts any worker that exceeds 2
     retries on a failure, weakens a test, masks a guardrail counter, edits out-of-scope files, or
     runs an un-extrapolated multi-hour sweep (`W1`–`W6`), alerting Level 1 (`@coordinator`).
   - **Level 1 Escalation — Senior Scientist Orchestrator (`Argon` `@coordinator` with
     [`.github/skills/arkhe-orchestrator/SKILL.md`](../../.github/skills/arkhe-orchestrator/SKILL.md)):**
     The Orchestrator is the **Senior Scientist** for **intra-phase and intra-milestone execution**.
     It resolves **~80% of worker escalations directly in 1 turn** without waking the Chief
     Scientist:
     - **Scope of authority:** Tweaking mechanistic and architectural implementation details
       *within* the active phase or milestone to make the problem tractable and achieve an optimal
       or near-optimal implementation, resolving implementation/harness bugs, granting cross-file
       edit permissions, and resetting the 2-strike counter.
     - **Anti-duplication rule:** When a worker has already identified the root cause and presented
       valid options within the active milestone's gates and parameters, `@coordinator` inspects at
       most the 1–2 cited code ranges, **makes the decision directly**, and sends the worker back.
       Never re-run the worker's analysis or outsource phase-level decisions to `@chief-scientist`.
     - **Limit & guardrail triage:** Bounds like `max_nodes` exist to flag pathology, not to be
       ground against or silenced. When a limit binds due to **substantively suboptimal worker or
       learner behavior** (e.g., sprouting on unpredictable exogenous driver channels, or running
       deep candidate searches on an already-locked port), `@coordinator` directs a correction to
       eliminate the suboptimal behavior while **maintaining the limit**.
     - **When to escalate to Level 2 (`@chief-scientist`):** Escalate to `@chief-scientist` **only**
       when: (a) every option has material disadvantages or cross-milestone fallout (`M3`–`M6`),
       (b) a limit/scaffolding constant is binding because the **class of problem genuinely expands
       the scale boundary**, (c) achieving Charter §§0–1 goals requires amending Charter §§2–3,
       recalibrating a pre-registered gate threshold, or splitting/re-ordering a milestone, or (d)
       dev and confirmation sweeps are complete and ready for single-shot holdout authorization
       (`S3`) or milestone closeout (`S4`).
   - **Level 2 Escalation — Chief Scientist (`gemini-4-argon-high` with
     [`.github/skills/arkhe-chief-scientist/SKILL.md`](../../.github/skills/arkhe-chief-scientist/SKILL.md)):**
     Always spawned fresh (**never** `fork: "coordinator"`), and **always** sleeps with
     `swarm_wait{mentions_only: true}`. Acts as the **Foundational Stand-In for the User** across
     the milestone roadmap (`M3`–`M6`):
     - **Full authority over Charter §§2–3 amendments, parameters, derived laws, gates, holdouts,
       and milestone structure:** So long as the **Charter-Core Invariants (Charter §§0–1 & holdout
       isolation)** are preserved and no arbitrary magic-number tuning occurs, the Chief Scientist
       has full authority to:
       1. **Amend Charter §2 (Substrate) or Charter §3 (Two-Layer Architecture)** if required to
          meet the foundational goals in Charter §§0–1 (recording the amendment and rationale in
          Charter Appendix A Decision Record `DR #`);
       2. Alter or re-derive any parameter in [`docs/parameter_inventory.md`](../parameter_inventory.md)
          (evolving Milestone-Scoped Scaffolding constants, search bounds, or compute guardrails
          into **reasoned Derived Scaling Laws** where every number or relationship has a
          documented, non-arbitrary basis) and authorize **Rule-4 golden trace re-blessing**;
       3. Recalibrate **pre-registered gate thresholds, $P_{\text{target}}$, margins, or yardsticks**
          that prove scientifically or practically mis-calibrated during execution (provided the
          revised gate cites a theoretical bound or measured baseline per Rule 9 and still tests
          the Charter hypothesis);
       4. **Split a milestone into sub-milestones (`Ma / Mb / Mc`)**, re-order phases, or
          **re-order/re-prioritize milestones** in Charter §7 to take a more effective, tractable
          route to the Charter §§0–1 goal (ratifying updates in `docs/milestone_*_plan.md`,
          `docs/parameter_inventory.md`, and Charter Appendix A Decision Record);
       5. **Approve and authorize single-shot holdout runs (`S3`)** once dev and confirmation banks
          pass cleanly, **approve milestone completion and progression to the next milestone (`S4`)**
          when holdout passes, and **adjudicate holdout failures** as a Level-2 escalation (e.g.,
          restructuring the milestone and allocating a fresh disjoint holdout bank without tuning to
          the consumed holdout seeds).
     - **When to escalate to the User (Principal Investigator):** The User is needed **only** when
       the overarching goal of the Charter (Charter §§0–1) is genuinely at risk—i.e., a core
       Charter hypothesis is falsified or blocked and **the Chief Scientist cannot identify a
       scientifically sound path forward with a realistic chance of success**.
     - **Hard execution boundary:** The Chief Scientist **never** performs interactive binary
       debugging (`lldb`, `gdb`, `objdump`), never masks a guardrail counter, and never writes
       copy-paste implementation patches for workers.
8. **Exploratory vs. confirmatory.** Before a threshold is pre-registered, runs are exploratory:
   they measure and report and never gate. Confirmatory runs use thresholds signed off in the
   milestone plan (or recalibrated by the Chief Scientist with a documented Rule-9 feasibility bound
   prior to opening the single-shot holdout bank). Never tune toward an exploratory or holdout
   number.
9. **Feasibility before targets.** Every gate threshold must cite a theoretical bound or a measured
   baseline that shows it is reachable. A threshold without one is a characterisation metric, not
   a gate.
10. **Definition of done.** A plan item is done only when it links to evidence: a test name, a commit,
    or a recorded command plus its output. "Complete" with no evidence is a defect.
11. **Kill criteria are results.** Reaching a charter kill or revisit criterion is a finding to report,
    not an obstacle to engineer around. One phase per commit; golden traces verified at each
    behavior-preserving commit, or accompanied by the written Derived Scaling Law rationale and
    `verify-m2a` / `verify-m2b` invariance proof.
