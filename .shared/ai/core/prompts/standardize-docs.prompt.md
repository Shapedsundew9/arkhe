# Standardize Documentation

Execute these steps to standardize documentation:

- Only apply these style guides. *DO NOT apply* any other style guides.
  - Use Mermaid for embedded markdown charts and diagrams where possible. Follow `.shared/docs/guides/mermaid-style-guide.md`.
  - Follow the mathematical notation style and compatibility rules in `.shared/docs/guides/math-style-guide.md`.
- Run `npx markdownlint-cli2 --fix "**/*.md"`.
- Only after the mechanical fixes have been done run `npx markdownlint-cli2 "**/*.md"` and address any remaining issues.
- These criteria apply to *ALL* markdown files in the repository, regardless of their location or purpose.
