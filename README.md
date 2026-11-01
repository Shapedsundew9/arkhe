# Rust Template

A lightweight Rust project template for building small CLI-based applications and package-friendly code.

## Overview

This repository provides a minimal starting point with:

- a Rust package layout under `src/`
- a simple CLI entry point in `src/main.rs`
- reusable crate library code in `src/lib.rs`
- project configuration and dependencies in `Cargo.toml`
- toolchain specifications in `rust-toolchain.toml`
- a docs folder for project notes and style guidance

## Project Structure

```text
.
├── .devcontainer/
├── .github/
├── docs/
├── src/
│   ├── lib.rs
│   └── main.rs
├── tests/
│   └── cli.rs
├── Cargo.toml
├── GEMINI.md
├── LICENSE
├── .gitignore
├── .markdownlint-cli2.jsonc
├── README.md
└── rust-toolchain.toml
```

## Getting Started

1. Ensure the Rust toolchain (via [rustup](https://rustup.rs/)) is installed and up to date.
2. Build the project:

```bash
cargo build
```

1. Run the CLI:

```bash
cargo run
```

This currently prints:

```text
Hello World!
```

Alternatively, install the binary locally to invoke it directly:

```bash
cargo install --path .
protoproject
```

## Development

You can extend the template by adding modules under `src/` and updating dependencies in `Cargo.toml`.

### Dev Container

The container image installs the Rust toolchain and components specified in
`rust-toolchain.toml` during the Docker build, without self-updating rustup.
Post-create setup checks the project but does not update Rust. This keeps
toolchain downloads out of startup, where editor tooling could otherwise run
rustup concurrently and interfere with temporary download files.

After changing `rust-toolchain.toml`, rebuild the container to install the new
configuration. To refresh Rust and the image-provided rustup, use **Dev Containers:
Rebuild Container Without Cache** so the toolchain installation runs again with
a refreshed base image.

If post-create setup previously failed, rerun it from the repository root:

```bash
bash .devcontainer/post-create.sh
```

To set your git identity in new containers, export `GIT_USER_NAME` and
`GIT_USER_EMAIL` on the host where VS Code starts. Post-create setup copies each
value into the container's global git config only when that setting is missing.

### Example

```rust
use rust_base::run;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    run()
}
```

## Testing and Verification

Run tests:

```bash
cargo test
```

Run the linter:

```bash
cargo clippy
```

Check code formatting:

```bash
cargo fmt --check
```

Check the container setup script with isolated mock commands (no downloads):

```bash
bash tests/post-create.sh
```

## Scripts and Tools

The project is configured with a binary entry point:

- `protoproject` -> `src/main.rs`

This makes it easy to run the CLI directly via Cargo:

```bash
cargo run --bin protoproject
```

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE) for details.

## Notes

This is intentionally a simple starter repository. It is designed to be easy to customize for your own Rust tools, scripts, or command-line apps.
