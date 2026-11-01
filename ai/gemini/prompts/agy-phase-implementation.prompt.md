# AGY Phase Implementation

Execute the prompt @[.github/prompts/phase-implementation.prompt.md] with the following inputs from the Project Initiator:

- The phase execution plan is in @docs/vision/
- Implementer model: `gemini-3.8-flash-high` if not specified otherwise
- Sub-agent prompt is statically defined; do not generate, modify, or introspect it:
  - Implementer: `.github/prompts/phase-implementer.prompt.md`
- Sub-agent execution command:
  - `agy --model [model] --print "Implement Work Package <WP-ID> from docs/vision/phase<N>-plan.md. Follow directives in $(cat .github/prompts/phase-implementer.prompt.md)"`
- **IMMEDIATE EXECUTION (Zero Preflight):** Your VERY FIRST tool call MUST be verifying working tree hygiene (`git status --porcelain`) and extracting the Work Package sequence from the phase plan. Do NOT run test, probe, or verification commands (no dry runs, no permission checks, no `agy` pings).
- **FORBIDDEN ACTIONS:**
  - DO NOT run `agy` with probe, test, or ping prompts before subagent execution.
  - DO NOT inspect `git log`, `git reflog`, or commit history except as required by the exit gate.
  - DO NOT write application or test code in the orchestrator context; all implementation is strictly performed by the Implementer subagent.
  - **DO NOT** search your chat history or invocation context; all necessary inputs are provided in this prompt.
