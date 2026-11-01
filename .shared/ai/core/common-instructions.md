# Common Instructions

## General

- Only call tools listed for the current session. e.g. ensure `apply_patch` exisst before using it.
- Make surgical, right-sized edits and apply typing and markdownlint conventions upfront.

## Documentation

- Keep `README.md` aligned with the install, usage, development, and publishing workflows.
- Always run `files=$( { git diff HEAD --name-only --diff-filter=AM -- '*.md'; git ls-files -o --exclude-standard -- '*.md'; } | sort -u ) [ -n "$files" ] && printf '%s\n' "$files" | xargs -d '\n' npx markdownlint-cli2 --fix` after markdown changes but do not address issues that cannot be fixed mechanically without explicit permission.
- Never modify `.markdownlint-cli2.jsonc`. Ask the user first if a change is necessary.

## CLI Output Rules

These rules only apply when a mermaid diagram is part of a response. If there is no mermaid diagram you will output to the terminal as normal.

- Always write or update the output directly into `scratchpad/` creating the folder as necessary.
- In the terminal, provide only a 1–2 sentence summary and state that the full diagram/plan has been written to the scratchpad.

<!-- mermaid-ai-skills:start -->
## Mermaid Diagrams

When the user asks to create, edit, or visualize a diagram, follow the
instructions in `.shared/ai/core/mermaid.instructions.md`.
<!-- mermaid-ai-skills:end -->
