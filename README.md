# Arkhe

Deterministic variable-width modular circuit simulator, ground-truth yardstick, compute-matched baseline harness, and online neurogenesis learner implementing **Milestone 0 (`M0 — Simulator and yardstick`)** and **Milestone 1 (`M1 — Metabolic economy and residual neurogenesis`)** from the [Foundational Research Charter v3](doc/foundational_research_charter_v3.md). Historical Milestone 0–1 sign-off records are preserved in [doc/research_record.md](doc/research_record.md); active pre-Milestone 2 coordinate-free scaling and readiness plans live in [doc/coordinate_free_scaling_plan.md](doc/coordinate_free_scaling_plan.md) and [doc/pre_milestone_2_readiness_plan.md](doc/pre_milestone_2_readiness_plan.md).

## Overview

Arkhe provides a pure, reproducible Rust research platform over $\mathbb{Z}_{2^k}$ ($1 \le k \le 64$) with:

- **`substrate`**: Exact $\mathbb{Z}_{2^k}$ ring arithmetic, Level-0 primitives (`ADD`, `SUB`, `MUL`, `EQZ`, `SPLIT`, `DELAY`), Charter §2.5 structural invariant checks, scalar evaluator (`ScalarState`), and 64-lane bit-parallel evaluator (`BitParallel64State` at $k = 1$).
- **`structural`**: Coordinate-free graph editing over `substrate::Circuit`, metabolic reserves and retrograde sensitivity, state-preserving live surgery, and finite-ring residual diagnostics.
- **`env`**: Finite-state benchmark generators (`T1`, `T3b`, `T3c`), declared channel substitution noise (`NoiseModel`), an exact $\varepsilon$-machine solver ($C_\mu$, $h_\mu$, $L_{\text{sync}}$, scope & bounded MSP status), and **Reeds–Sloane** shift-register synthesis over $\mathbb{Z}_{2^k}$.
- **`metrics`**: Surprisal null models, Minimum Description Length ($L$) accounting, autonomous limit-cycle & attractor-basin inspection, and standardised `Scorecard` manifests separating distributional log-loss, KT point-prediction residual loss, paired noise-oracle regret, and internal vs. boundary-lag state registers.
- **`baselines`**: Compute-matched prequential baseline harness (`RandomSearch`, `(1+λ) CGP`, `CTW`, `ReedsSloane`, and certified small-circuit `Exhaustive` synthesis) over phase-exact sliding observation windows.
- **`rng`, `config`, & `verify`**: Domain-separated portable `ChaCha8Rng` streams (`SeedBank`), TOML/JSON `ExperimentConfig` & `RunReport` manifests with embedded `BuildProvenance`, and the M0 plus fixed-structure M1a verification batteries.

## Project Structure

```text
.
├── .devcontainer/
├── .github/
├── doc/
│   ├── coordinate_free_scaling_plan.md
│   ├── foundational_research_charter_v3.md
│   ├── pre_milestone_2_readiness_plan.md
│   └── research_record.md
├── scripts/
│   └── build.rs
├── src/
│   ├── baselines/
│   ├── env/
│   ├── metrics/
│   ├── structural/
│   ├── substrate/
│   ├── config.rs
│   ├── lib.rs
│   ├── main.rs
│   ├── rng.rs
│   └── verify.rs
├── tests/
│   ├── cli.rs
│   ├── determinism.rs
│   ├── m1a_economy.rs
│   ├── m1b_neurogenesis.rs
│   ├── m0_acceptance.rs
│   └── post-create.sh
├── Cargo.toml
├── GEMINI.md
├── LICENSE
├── README.md
└── rust-toolchain.toml
```

## Getting Started

1. Ensure the stable Rust toolchain (`1.99.1+`, edition `2024`) is installed via [rustup](https://rustup.rs/).
2. Build the workspace in release mode:

```bash
cargo build --locked --release
```

1. Run the Milestone 0 pre-registered acceptance suite (Gates G1–G4 across $k = 1$ and $k = 2$ on development or holdout seed banks):

```bash
cargo run --locked --release -- verify-m0 --seed-offset 0 --seeds 20
cargo run --locked --release -- verify-m0 --seed-offset 10000 --seeds 20
```

1. Run the M1a fixed-structure economy verification suite (single bank or 100-bank statistical sweep):

```bash
cargo run --locked --release -- verify-m1a --seed-offset 0 --seeds 20
cargo run --locked --release -- verify-m1a --banks 100 --seed-offset 20 --seeds 20
```

1. Run the full Milestone 1 acceptance verification suite (Stage M1a hard gate audit followed by Stage M1b neurogenesis, single bank or 100-bank sweep):

```bash
cargo run --locked --release -- verify-m1 --seed-offset 0 --seeds 20
cargo run --locked --release -- verify-m1b --banks 100 --seed-offset 20 --seeds 20
cargo run --locked --release -- verify-m1 --banks 100 --seed-offset 20 --seeds 20
cargo run --locked --release -- verify-m1 --seed-offset 10000 --seeds 20 --holdout
```

1. Inspect analytical $\varepsilon$-machine and Reeds–Sloane ground-truth yardsticks for a generator:

```bash
cargo run --release -- yardstick --generator '{"kind":"T1_Alternating","a":0,"b":3}' --width 2 --epsilon 0.05
```

1. Execute a reproducible experiment from a TOML configuration file:

```bash
cargo run --release -- run --config path/to/experiment.toml --output path/to/report.json
```

Alternatively, install the binary locally to invoke `arkhe` directly:

```bash
cargo install --path .
arkhe --help
```

## Library Example

```rust
use arkhe::run;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    run()
}
```

## Development

### Dev Container

The container image installs the Rust toolchain and components specified in `rust-toolchain.toml` during the Docker build, without self-updating rustup. Post-create setup checks the project (`cargo check`) without downloading toolchain updates.

If post-create setup previously failed, rerun it from the repository root:

```bash
bash .devcontainer/post-create.sh
```

To set your git identity in new containers, export `GIT_USER_NAME` and `GIT_USER_EMAIL` on the host where VS Code starts. Post-create setup copies each value into the container's global git config only when that setting is missing.

## Testing, Linting, and Packaging

Run unit and integration tests:

```bash
cargo test
```

Run Clippy across all targets and features with warnings denied:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Check code formatting:

```bash
cargo fmt --check
```

Check the container setup script with isolated mock commands:

```bash
bash tests/post-create.sh
```

Verify crate packaging metadata and archive contents:

```bash
cargo package --allow-dirty
```

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE) for details.
