# AGY Architectural Iteration

Execute the prompt @[.github/prompts/architecture-iteration.prompt.md] with the following inputs from the Project Initiator:

- The document(s) are in @docs/vision/
- Lead Developer model: `gemini-3.8-flash-high` if not specified otherwise
- Architect model: `claude-opus-4-6-thinking` if not specified otherwise
- Sub-agent prompts are statically defined; do not generate, modify, or introspect them:
  - Lead Developer: `.github/prompts/architecture-lead-developer.prompt.md`
  - Architect: `.github/prompts/architecture-architect.prompt.md`
- Sub-agent execution commands:
  - Lead Developer: `agy --model [model] --print "$(cat .github/prompts/architecture-lead-developer.prompt.md)"`
  - Architect: `agy --model [model] --print "$(cat .github/prompts/architecture-architect.prompt.md)"`
- **IMMEDIATE EXECUTION (Zero Preflight):** Your VERY FIRST tool call MUST be invoking the Lead Developer subagent. Do NOT run test, probe, or verification commands (no dry runs, no permission checks, no `agy` pings).
- **FORBIDDEN ACTIONS:**
  - DO NOT run `agy` with probe, test, or ping prompts before subagent execution.
  - DO NOT inspect `git log`, `git reflog`, or commit history to determine prior iterations or file history.
  - DO NOT track or report iteration numbers; use neutral commit messages: `Architecture alignment (LD: <model>, Arch: <model>)`.
  - **DO NOT** search your chat history or invocation context; all necessary inputs are provided in this prompt.
- **Pre-clean handling:** If temporary files (`docs/vision/response.md`, `docs/vision/triage.md`, `docs/vision/rebuttal.md`) exist from prior runs, remove them cleanly with `rm -f` without investigating past commits.
