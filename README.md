# Arkhe

Deterministic variable-width modular circuit simulator, ground-truth yardstick, compute-matched baseline harness, and online neurogenesis learner implementing **Milestone 0 (`M0 — Simulator and yardstick`)**, **Milestone 1 (`M1 — Metabolic economy and residual neurogenesis`)**, **Milestone 2a (`M2a — Cross-channel predictive coupling`)**, **Milestone 2b (`M2b — Redundant derivations and counterfactual comparators`)**, and **Milestone 3 (`M3 — Free-rider resistance and structural compression`; Completed & verified — `M3 v2` passed `50100..=50199` holdout under Charter §2.8, ready for `M4a`)** from the [Foundational Research Charter v3](docs/foundational_research_charter_v3.md). Canonical parameter, bound, and guardrail classifications live in [docs/parameter_inventory.md](docs/parameter_inventory.md), Milestone 0–3 sign-off records, review findings, and derivations live in [docs/research_record.md](docs/research_record.md) and [docs/milestone_3_review.md](docs/milestone_3_review.md) (with the Milestone 3 plan and attainment ledger in [docs/milestone_3_plan.md](docs/milestone_3_plan.md)), and two-level swarm escalation skills live in [.agents/skills/arkhe-orchestrator/SKILL.md](.agents/skills/arkhe-orchestrator/SKILL.md), [.agents/skills/arkhe-chief-scientist/SKILL.md](.agents/skills/arkhe-chief-scientist/SKILL.md), and [.agents/skills/arkhe-watchdog/SKILL.md](.agents/skills/arkhe-watchdog/SKILL.md).

## Overview

Arkhe provides a pure, reproducible Rust research platform over $\mathbb{Z}_{2^k}$ ($1 \le k \le 64$) with:

- **`substrate`**: Exact $\mathbb{Z}_{2^k}$ ring arithmetic, Level-0 primitives (`ADD`, `SUB`, `MUL`, `EQZ`, `SPLIT`, `DELAY`), Charter §2.5 structural invariant checks, scalar evaluator (`ScalarState`), and 64-lane bit-parallel evaluator (`BitParallel64State` at $k = 1$).
- **`structural`**: Coordinate-free graph editing over `substrate::Circuit`, metabolic reserves and retrograde sensitivity, state-preserving live surgery, causal-disjoint redundancy sprouting, wake/sleep counterfactual comparators, finite-ring residual diagnostics with progressive lag deepening, and verified local pass-through & ring-identity wake excision (`M3-E1` + `M3-E2`).
- **`env`**: Finite-state benchmark generators (`T1`, `T2`, `T2_MultiViewCoupled`, `T3b`, `T3c`), declared channel substitution & scheduled channel-fault noise (`NoiseModel`), an exact $\varepsilon$-machine solver ($C_\mu$, $h_\mu$, $L_{\text{sync}}$, scope & bounded MSP status), and **Reeds–Sloane** shift-register synthesis over $\mathbb{Z}_{2^k}$.
- **`metrics`**: Epistemic & boundary-null surprisal models ($\Delta E \equiv -\Delta L_{\text{null}}^{\text{epistemic}}$), Minimum Description Length ($L$) accounting, autonomous limit-cycle & attractor-basin inspection, and standardised `Scorecard` manifests (with per-channel breakdowns) separating distributional log-loss, KT point-prediction residual loss, paired noise-oracle regret, and internal vs. boundary-lag state registers.
- **`baselines`**: Compute-matched prequential baseline harness (`RandomSearch`, `(1+λ) CGP`, `CTW`, `CrossChannelCtw`, BPTT `Rnn`, `ReedsSloane`, and certified small-circuit `Exhaustive` synthesis) over phase-exact sliding observation windows.
- **`rng`, `config`, `stats`, `verify_m2a`, `verify_m2b`, & `verify_m3`**: Domain-separated portable `ChaCha8Rng` streams (`SeedBank`), TOML/JSON `ExperimentConfig` & `RunReport` manifests with embedded `BuildProvenance`, regime-stratified per-task FWER (`StratifiedGateAudit`) + finite Option D `tail_metrics` + pooled Clopper–Pearson + $\chi^2$/permutation homogeneity bank statistics (`stats`), and M2a/M2b/M3 verification & exploratory batteries (`G2A.1`–`G2A.5`, `G2B.1`–`G2B.5`, `G3.1`–`G3.5b`, `explore-m3`), gated by `scripts/check_retained_baselines.py` against `docs/retained_baselines.json`.

## Project Structure

```text
.
├── .agents/
├── .devcontainer/
├── .github/
├── docs/
│   ├── instructions/
│   │   └── repo-common.md
│   ├── templates/
│   │   ├── blocker_report.md
│   │   └── milestone_plan_template.md
│   ├── foundational_research_charter_v3.md
│   ├── milestone_3_plan.md
│   ├── milestone_3_review.md
│   ├── parameter_inventory.md
│   ├── research_record.md
│   └── retained_baselines.json
├── scripts/
│   ├── build.rs
│   ├── check_doc_links.py
│   └── check_retained_baselines.py
├── src/
│   ├── baselines/
│   ├── env/
│   ├── metrics/
│   ├── structural/
│   ├── substrate/
│   ├── verify_m2a/
│   ├── verify_m2b/
│   ├── verify_m3/
│   ├── config.rs
│   ├── lib.rs
│   ├── main.rs
│   ├── rng.rs
│   └── stats.rs
├── tests/
│   ├── fixtures/golden/
│   ├── cli.rs
│   ├── determinism.rs
│   ├── golden_traces.rs
│   ├── m2b_comparators.rs
│   ├── m3_free_riders.rs
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

1. Run the Milestone 2a cross-channel predictive coupling verification suite (`G2A.1`–`G2A.5`):

```bash
cargo run --locked --release -- verify-m2a --seed-offset 0 --seeds 20
cargo run --locked --release -- verify-m2a --banks 50 --seed-offset 20 --seeds 20
```

1. Run the Milestone 2b redundant derivations and counterfactual comparators suite (`G2B.1`–`G2B.5`):

```bash
cargo run --locked --release -- verify-m2b --seed-offset 0 --seeds 20
cargo run --locked --release -- verify-m2b --banks 50 --seed-offset 20 --seeds 20
cargo run --locked --release -- verify-m2b --seed-offset 20040 --seeds 20
```

1. Run the Milestone 3 free-rider resistance and verified local excision exploratory & verification suites (`G3.1`–`G3.5b`; M3 v2 uses 100-seed confirmation `20100..=20199`, while `50100..=50199` is now permanently closed unconditionally alongside `50000..=50019`, with durable `S3`/`S4` evidence archived in `docs/evidence/m3_v2/`):

```bash
cargo run --locked --release -- explore-m3 --seed-offset 0 --seeds 20
cargo run --locked --release -- verify-m3 --seed-offset 0 --seeds 20
cargo run --locked --release -- verify-m3 --banks 50 --seed-offset 20 --seeds 20
cargo run --locked --release -- verify-m3 --banks 5 --seed-offset 20100 --seeds 20
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

Check code formatting, documentation relative links/anchors, and the mandatory retained-baseline regression gate (`golden_traces` + `verify-m2a` + `verify-m2b` under `StratifiedGateAudit` against `docs/retained_baselines.json`, required clean before any `S3`/`S4` request):

```bash
cargo fmt --check
python3 scripts/check_doc_links.py
python3 scripts/check_retained_baselines.py --require-clean
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
