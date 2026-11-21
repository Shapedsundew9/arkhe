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
   for a provable mathematical reason (e.g., $\gamma^{\tau-1}$ credit attenuation, $\tau$-tick
   pipeline fill latency, alphabet size $2^k$, or description debt $\Delta L_v$), the **primary
   remedy** to propose in a blocker report is replacing the fixed scalar with a
   **zero-free-parameter Derived Scaling Law**—never weakening the benchmark task, narrowing a
   gate, or tuning a new magic number.
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
   - a pre-registered gate fails on development seeds and the cause is not an implementation bug
     with a demonstrable fix within 2 attempts;
   - a fix would add, remove, or re-derive a constant, a threshold, a gate, a test assertion, or a
     golden fixture;
   - two consecutive fix attempts for the same failure brought no new diagnostic understanding;
   - the charter, a milestone plan, the inventory, and the code disagree;
   - a theoretical argument suggests the target is unreachable under the current rules.
6. **Multi-bank acceptance uses pooled confidence bounds plus a homogeneity test**, never compound
   per-bank hurdles.
7. **Decision rights & two-level escalation hierarchy.** Only the user (Principal Investigator)
   decides scientific parameters, derived scaling laws, gate thresholds, seed banks, evidence
   retirement, golden re-blessing, charter text, kill-criterion interpretation, and milestone
   sign-off (see Charter §7.1). Swarm execution follows a strict hierarchy with **two distinct
   escalation levels** above the implementation workers:
   - **Tier 1 — Implementation Workers (`gemini-4-hydrogen-high` / `gemini-4-hydrogen-xhigh`):**
     Always spawned fresh (**never** `fork: "coordinator"`, which inherits the coordinator's Argon
     model binding). Strictly bound to the parameters, formulas, file scope, and test
     specifications in their brief. Maximum **2 fix attempts** on any failing test; **never**
     weaken test assertions, alter generator parameters (e.g., `add_b`, `lag`, `Width`), mask or
     suppress guardrail counters (`SproutBlockedMaxNodes`, `SproutBudgetExhausted`), or edit
     out-of-scope files to make a test pass. When escalating (`@coordinator [escalation]` +
     `swarm_wait{mentions_only: true}`), the worker **must** present: (i) the root cause or exact
     failing observation (file, line, metric), (ii) **2–3 concrete options with trade-offs**, and
     (iii) whether any guardrail/limit bound is due to silly learner/harness behavior vs. genuine
     problem-class scale expansion.
   - **Tier 2 — Anti-Grind & Quota Watchdog (`gemini-4-hydrogen-high` with
     [`.github/skills/arkhe-watchdog/SKILL.md`](../../.github/skills/arkhe-watchdog/SKILL.md)):**
     Wakes on a 10-minute timer (`schedule`), uses a compact stateless delta check without dumping
     code or transcripts into its context, verifies workers run on `gemini-4-hydrogen-*`, and halts
     any worker that exceeds 2 retries on a failure, weakens a test, masks a guardrail counter,
     edits out-of-scope files, or runs an un-extrapolated multi-hour sweep (`W1`–`W6`), escalating
     to `@coordinator`.
   - **Level 1 Escalation — Senior Scientist Orchestrator (`Argon` `@coordinator` with
     [`.github/skills/arkhe-orchestrator/SKILL.md`](../../.github/skills/arkhe-orchestrator/SKILL.md)):**
     The Orchestrator is the **Senior Scientist** and first-line decision filter. It resolves
     **~80% of worker escalations directly in 1 turn** without waking the Chief Scientist:
     - **Anti-duplication rule:** When a worker has already identified the root cause and presented
       valid options within the pre-registered plan, `@coordinator` inspects at most the 1–2 cited
       code ranges to verify the options, **makes the decision directly**, grants file-scope
       authorization if needed, resets the 2-strike counter, and sends the worker back. Never
       re-run the worker's analysis or outsource routine bug/harness decisions to the Chief
       Scientist.
     - **Limit & guardrail triage:** Bounds like `max_nodes` exist to flag pathology, not to be
       ground against or silenced. When a limit binds due to **silly worker or learner behavior**
       (e.g., sprouting on unpredictable exogenous driver channels, or running deep candidate
       searches on an already-locked port), `@coordinator` directs a correction to the behavior
       while **maintaining the limit**.
     - **When to escalate to Level 2 (`@chief-scientist`):** Escalate to `@chief-scientist` **only**
       when: (a) every option has material disadvantages or cross-milestone fallout (`M3`–`M6`),
       (b) a limit/scaffolding constant is binding because the **class of problem genuinely expands
       the scale boundary**, requiring a broader directional tweak to make the problem tractable or
       a zero-free-parameter Derived Scaling Law, or (c) a pre-registered gate, oracle, or Charter
       hypothesis is in question.
   - **Level 2 Escalation — Chief Scientist (`gemini-4-argon-high` with
     [`.github/skills/arkhe-chief-scientist/SKILL.md`](../../.github/skills/arkhe-chief-scientist/SKILL.md)
     and [`docs/chief_scientist_role.md`](../chief_scientist_role.md)):**
     Always spawned fresh (**never** `fork: "coordinator"`), and **always** sleeps with
     `swarm_wait{mentions_only: true}` so it never auto-wakes on worker-to-coordinator traffic.
     Acts as a foundational arbiter and stand-in for the user on hard directional trade-offs: it
     cannot overturn Charter goals or user-owned thresholds unilaterally, but has broad authority
     to tweak research/mechanistic direction so the problem becomes tractable without contortion or
     guardrail masking. The Chief Scientist **never** performs interactive binary debugging
     (`lldb`, `gdb`, `objdump`) or writes copy-paste implementation patches for workers. When a
     foundational change, gate threshold change, or Derived Scaling Law requires Principal
     Investigator sign-off, it prepares a structured blocker report ([`docs/templates/blocker_report.md`](../templates/blocker_report.md))
     for the user (`Stop Point S2`).
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
    behavior-preserving commit, or accompanied by the written Derived Scaling Law rationale and
    `verify-m2a` / `verify-m2b` invariance proof.
