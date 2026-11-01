# AGY Vision Debate

Execute the prompt @[.github/prompts/vision-debate.prompt.md] with the following inputs from the Project Initiator:

- The document(s) are in @docs/vision/
- Visionary model: `gemini-3.8-flash-high` if not specified otherwise
- Stakeholder models: `claude-opus-4-6-thinking` if not specified otherwise
- Sub-agent prompts are statically defined; do not generate, modify, or introspect them:
  - Stakeholder: `.github/prompts/vision-stakeholder.prompt.md`
  - Visionary: `.github/prompts/vision-visionary.prompt.md`
- Sub-agent execution commands:
  - Stakeholder: `agy --model [model] --print "$(cat .github/prompts/vision-stakeholder.prompt.md)"`
  - Visionary: `agy --model [model] --print "$(cat .github/prompts/vision-visionary.prompt.md)"`
- **IMMEDIATE EXECUTION (Zero Preflight):** Your VERY FIRST tool call MUST be invoking the Stakeholder subagent. Do NOT run test, probe, or verification commands (no dry runs, no permission checks, no `agy` pings).
- **FORBIDDEN ACTIONS:**
  - DO NOT run `agy` with probe, test, or ping prompts before subagent execution.
  - DO NOT inspect `git log`, `git reflog`, or commit history to determine prior iterations or file history.
  - DO NOT track or report iteration numbers; use neutral commit messages: `Vision alignment (Stakeholder: <model>, Visionary: <model>)`.
  - **DO NOT** search your chat history or invocation context; all necessary inputs are provided in this prompt.
- **Pre-clean handling:** If temporary files (`docs/vision/response.md`, `docs/vision/rebuttal.md`) exist from prior runs, remove them cleanly with `rm -f` without investigating past commits.
