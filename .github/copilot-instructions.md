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
2. **Every constant is classified** as derived, pre-registered scientific parameter, or compute
   guardrail, in the inventory in `doc/coordinate_free_scaling_plan.md` §2.1 (or its successor).
   Adding or changing a constant requires updating that inventory.
3. **Every limit is observable.** Any cap, guardrail, clip, or window truncation must emit a named
   telemetry counter when it binds. Guardrails must never bind in an accepted run.
4. **Behavior-preserving refactors are proven by golden traces** (`tests/golden_traces.rs`).
   Re-blessing fixtures requires a written rationale and is only allowed for intentional behavior changes.
5. **Hitting a structural limit or starvation cliff means STOP and discuss** the scaling rule with
   the user. Do not apply local workarounds or retune constants.
6. **Multi-bank acceptance uses pooled confidence bounds plus a homogeneity test**, never compound
   per-bank hurdles.
