---
name: arkhe-swarm-audit
description: >-
  Milestone-agnostic forensic workflow, wall-clock, quota, and token efficiency
  auditor for Antigravity swarm runs. Activate periodically after a milestone or
  major swarm session (or via `/arkhe-swarm-audit`) to quantify dead stalls,
  quota burn, no-op wakeups, context bloat, grind loops, and tool/skill gaps,
  and to propose concrete mechanical, skill, prompt, and workflow optimizations
  without compromising engineering or scientific rigor.
---

# Arkhe Swarm Workflow & Token Efficiency Audit (`/arkhe-swarm-audit`)

Use this skill periodically after completing a milestone or multi-agent swarm run to continuously push operations toward **deterministic mechanization, zero avoidable stalls, and minimal token waste** — **without compromising scientific rigor, test coverage, or verification quality**.

---

## Core Optimization Principle

> **Mechanize the mechanical; protect the scientific.**
> Every proposed adjustment MUST reduce pure operational waste (dead wall-clock stalls, no-op wakeups, cache-cold polling, context thrashing, redundant tool loops, oversized effort tiers) while leaving scientific invariants, test assertions, holdout isolation, and independent verification **strictly intact or stronger**.

When diagnosing recurring friction, prefer fixes in this leverage order:

1. **Deterministic scripts / tooling** (zero LLM tokens — e.g., background watchers, pre-flight validators, summary emitters)
2. **Runtime lifecycle & quota mechanics** (e.g., pre-emptive pause/auto-resume timers, respawn ceilings, clean teardown)
3. **Addressing, board & navigation hygiene** (e.g., line-1-only `@mentions`, board payload caps + file offload, CodeGraph-first lookup)
4. **Model/effort tiering & work deduplication** (e.g., `high` default vs `xhigh` opt-in, targeted worker tests vs single authoritative verifier pass)
5. **Codebase modularity** (splitting monolithic hotspots that force repeated context compactions)

Prose reminders in prompts are the **weakest** lever; whenever a defect can be caught or eliminated mechanically in Python/tooling, automate it.

---

## 5-Stage Audit Methodology

### Stage 1 — Deterministic Telemetry Extraction (1 Command)

Run the bundled analyzer script [analyze_swarm.py](./scripts/analyze_swarm.py) before reading any transcripts manually. It parses `members.json`, `board.jsonl`, `messages/*.json`, and every member's `transcript.jsonl` (using exact Antigravity token semantics where `total_prompt = input_tokens [uncached] + cache_read_tokens`, plus separate `CHECKPOINT` compaction overhead):

```bash
# List recent swarm sessions to confirm the target conversation ID
python3 .agents/skills/arkhe-swarm-audit/scripts/analyze_swarm.py --list

# Run full quantitative telemetry report on the latest (or a specific) swarm
python3 .agents/skills/arkhe-swarm-audit/scripts/analyze_swarm.py --latest --repo /workspaces/arkhe
# or: --swarm-id <coordinator_conversation_id> --repo /workspaces/arkhe
```

Do **not** dump full transcripts into context. Use the analyzer's output to locate the exact high-leverage anomalies.

---

### Stage 2 — Targeted Forensic Deep-Dive

Inspect **only** the top 2–5 anomalies surfaced by Stage 1, cross-referencing current governance files ([`.agents/skills/`](../../), [`docs/instructions/repo-common.md`](../../../docs/instructions/repo-common.md), and [`scripts/`](../../../scripts/)) to distinguish **four distinct root causes**:

| Root Cause Class | Diagnostic Question | Typical Remedy |
| :--- | :--- | :--- |
| **Missing mechanism** | Did the system lack a protocol/tool for a recurring failure (e.g., quota pause, runaway sweep)? | Build deterministic script + explicit state machine in skills |
| **Broken / misaligned tool** | Did a tool or check exist but fail on real log formats (e.g., Cargo exit `101` vs `1`, async `SYSTEM_MESSAGE`, truncated output)? | Fix tool + add unit test + replay against this swarm's transcripts |
| **Runtime-behavior trap** | Did a subtle platform behavior multiply waste (e.g., cache TTL < cron period, mid-body `@mention` waking `mentions_only: true` agents, `TimerCondition` cancellation)? | Change protocol to work with the runtime mechanics natively |
| **Policy / scoping drift** | Were workers over-bundled past compaction, over-provisioned on `xhigh`, ignoring CodeGraph, or duplicating tests? | Tighten effort matrix, respawn ceilings, and brief contracts |

Always check these **seven universal audit dimensions**:

1. **Wall-Clock & Quota Continuity:** Active % vs mandatory cooldown % vs avoidable post-reset dead wait %; final-20% burn rate (`%/min`); whether `W8` pre-emptive pause triggered cleanly with `TimerCondition: "never"` or crashed on `429`; whether any new workers were spawned below the freeze threshold (`<= 18%`).
2. **Watchdog Overhead & Recall:** `@monitor` total calls, uncached input tokens, peak context, cache hit %, dead-gap queued ticks, and whether any 2-strike grind or quota drop went uncaught.
3. **Wakeup & Addressing Hygiene:** Pure no-op wakeup counts and wasted uncached tokens on sleeping roles (`chief-scientist`, `verifier*`, `monitor`); casual mid-body `@mention` counts; board total size and oversized (`> 3 KB`) posts.
4. **Context Lifecycle & Compaction Drag:** Agents hitting `CHECKPOINT` compactions or exceeding ceilings (`>= 300K` workers, `>= 150K` verifiers); mega-workers that should have landed sub-phase checkpoint commits and respawned fresh.
5. **Code Navigation & Codebase Friction:** Ratio of `codegraph_explore` / `codegraph explore` vs `read_file` / `rg`; top repeatedly-read files (line counts + estimated token size); `edit_file` target-not-found error rates; single-tool vs batched multi-tool turns.
6. **Model/Effort Fit & Verification Deduplication:** Avg output/thinking tokens per call on mechanical vs algorithmic phases (`high` vs `xhigh`); orchestrator doing inline data-analysis scripts instead of delegating; back-to-back duplicate full test suites between worker and verifier; dirty working-tree carryover; unkilled agents after `[done]`.
7. **Governance & Model Currency:** Outdated/retiring model names, broken doc links, or contradictions between `repo-common.md` and `.agents/skills/*`.

---

### Stage 3 — Impact-Ranked Remediation Plan & Quality-Preservation Gate

Write the structured audit report to an artifact (`<artifact-dir>/swarm_efficiency_audit_<swarm_id[:8]>.md`) containing:

1. **Executive Scorecard:** Wall-clock split, total/per-model token usage, quantified zero-work waste, and delta vs the prior audit (if available).
2. **Evidence-Backed Findings (`P0` / `P1` / `P2`):** Every claim cites exact agent IDs, board `#seq`, step numbers, timestamps, and token/time quantities.
3. **Prioritized Remediation Plan:** Concrete changes grouped into:
   - **P0 (Immediate):** Eliminates dead stalls, `429` crashes, and major zero-work token bleed.
   - **P1 (High-Leverage):** Cuts context drag, over-provisioning, and redundant work.
   - **P2 (Structural / Optional):** Codebase modularization or parallelism refinements.
4. **"No Quality Regression" Check:** For every proposed change, state explicitly why it does *not* weaken scientific rigor, test coverage, or verification independence.
5. **Implementation Scope & Open Decisions:** List exact files to create/modify. **Unless the user explicitly requested immediate implementation, present the plan for approval before editing repository files.**

---

### Stage 4 — Implementation & Historical Replay Verification (On User Approval)

When the user approves implementing the remediation plan:

1. Implement tooling/script changes first with unit tests covering every newly discovered failure mode.
2. **Historical Replay Proof:** Replay any updated watchdog or analyzer logic against the audited swarm's *actual* transcripts to prove mechanically that the new checks catch the observed failures (e.g., quota pause, grind loops, context ceilings) without false positives.
3. Update affected skills (`.agents/skills/*/SKILL.md`) and [`docs/instructions/repo-common.md`](../../../docs/instructions/repo-common.md), keeping all cross-references aligned.
4. Run repository validation checks:
   - Unit tests for all modified/added Python scripts (`python3 -m unittest ...`)
   - `npx markdownlint-cli2 --fix` on all added/modified `.md` files
   - `python3 scripts/check_doc_links.py`
   - Clean `git status` / `git diff --stat` review.
