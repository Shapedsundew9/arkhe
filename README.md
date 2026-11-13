# Arkhe

Deterministic variable-width modular circuit simulator, ground-truth yardstick, compute-matched baseline harness, and online neurogenesis learner implementing **Milestone 0 (`M0 — Simulator and yardstick`)** and **Milestone 1 (`M1 — Metabolic economy and residual neurogenesis`)** from the [Foundational Research Charter v3](doc/foundational_research_charter_v3.md). Canonical parameter, bound, and guardrail classifications live in [doc/parameter_inventory.md](doc/parameter_inventory.md), and historical Milestone 0–1 sign-off records plus pre-Milestone 2 readiness verification live in [doc/research_record.md](doc/research_record.md).

## Overview

Arkhe provides a pure, reproducible Rust research platform over $\mathbb{Z}_{2^k}$ ($1 \le k \le 64$) with:

- **`substrate`**: Exact $\mathbb{Z}_{2^k}$ ring arithmetic, Level-0 primitives (`ADD`, `SUB`, `MUL`, `EQZ`, `SPLIT`, `DELAY`), Charter §2.5 structural invariant checks, scalar evaluator (`ScalarState`), and 64-lane bit-parallel evaluator (`BitParallel64State` at $k = 1$).
- **`structural`**: Coordinate-free graph editing over `substrate::Circuit`, metabolic reserves and retrograde sensitivity, state-preserving live surgery, and finite-ring residual diagnostics.
- **`env`**: Finite-state benchmark generators (`T1`, `T3b`, `T3c`), declared channel substitution noise (`NoiseModel`), an exact $\varepsilon$-machine solver ($C_\mu$, $h_\mu$, $L_{\text{sync}}$, scope & bounded MSP status), and **Reeds–Sloane** shift-register synthesis over $\mathbb{Z}_{2^k}$.
- **`metrics`**: Surprisal null models, Minimum Description Length ($L$) accounting, autonomous limit-cycle & attractor-basin inspection, and standardised `Scorecard` manifests separating distributional log-loss, KT point-prediction residual loss, paired noise-oracle regret, and internal vs. boundary-lag state registers.
- **`baselines`**: Compute-matched prequential baseline harness (`RandomSearch`, `(1+λ) CGP`, `CTW`, `ReedsSloane`, and certified small-circuit `Exhaustive` synthesis) over phase-exact sliding observation windows.
- **`rng`, `config`, `verify`, & `verify_m1`**: Domain-separated portable `ChaCha8Rng` streams (`SeedBank`), TOML/JSON `ExperimentConfig` & `RunReport` manifests with embedded `BuildProvenance`, M0/M1a/M1b verification batteries, pooled Clopper–Pearson + $\chi^2$ homogeneity bank statistics, and `profile-compute` scaling/counter profiling.

## Project Structure

```text
.
├── .devcontainer/
├── .github/
├── doc/
│   ├── templates/
│   │   ├── blocker_report.md
│   │   └── milestone_plan_template.md
│   ├── foundational_research_charter_v3.md
│   ├── parameter_inventory.md
│   └── research_record.md
├── scripts/
│   ├── build.rs
│   ├── check_doc_links.py
│   └── measure_compute_profile.py
├── src/
│   ├── baselines/
│   ├── env/
│   ├── metrics/
│   ├── structural/
│   ├── substrate/
│   ├── verify_m1/
│   ├── config.rs
│   ├── lib.rs
│   ├── main.rs
│   ├── rng.rs
│   └── verify.rs
├── tests/
│   ├── fixtures/golden/
│   ├── cli.rs
│   ├── determinism.rs
│   ├── golden_traces.rs
│   ├── m0_acceptance.rs
│   ├── m1a_economy.rs
│   ├── m1b_neurogenesis.rs
│   ├── multi_channel_development.rs
│   ├── post-create.sh
│   └── regime_shift.rs
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

1. Run the Phase 0 / readiness compute and cliff-counter profiler:

```bash
cargo run --locked --release -- profile-compute --seed-offset 0 --seeds 5
python3 scripts/measure_compute_profile.py --skip-build --seed-offset 0 --seeds 5
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

Check code formatting and documentation relative links/anchors:

```bash
cargo fmt --check
python3 scripts/check_doc_links.py
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
