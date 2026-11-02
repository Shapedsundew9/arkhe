# Milestone 1 Implementation Plan: Metabolic Economy and Residual Neurogenesis

> **Status:** Proposed implementation plan for Stages **M1a (`Economy on fixed structure`)** and **M1b (`First to second neuron`)** of [foundational_research_charter_v2.md](foundational_research_charter_v2.md), incorporating closeout findings from [milestone_0_review.md](milestone_0_review.md).
>
> **Charter scope:** §3.1.1–§3.1.7 (Spatial lattice, metabolic reserves, retrograde sensitivity-gated credit, surprisal-weighted exact-match reward, residual-driven corrector sprouting, starvation pruning, free-rider defences), §3.3 (First and second neuron as boosting over $\mathbb{Z}_{2^k}$ with causal $z^{-1}$), §3.4 (Calibration to total description length $L$), §4.1 (Online wake loop), §6 (Hypotheses **H1** & **H2**), §7 (M1a & M1b success and kill criteria at $k = 1$ and $k = 2$), §8 (Ablations, equal compute, pre-registration, reproducibility), and §9 (Rust layer separation).

---

## 1. Purpose, Stage-Gated Structure, Architectural Decisions, and Non-Goals

Milestone 1 transitions `arkhe` from Milestone 0's static simulator and search yardsticks to **online gradient-free structural learning**. Following Charter §7, Milestone 1 is divided into **two strictly sequential, kill-gated stages** executed across both Boolean ($k = 1$, $\mathbb{Z}_2$) and graded ($k = 2$, $\mathbb{Z}_4$) substrates:

1. **Stage M1a — Economy on fixed structure (Hypothesis H1):** Before any structural mutation is enabled, validate node energy reserves $E_i(t)$, maintenance costs ($c_{\text{base}}$, $c_{\text{split}} > c_{\text{base}}$, wire hop cost), surprisal-weighted exact-match reward, counterfactual DAG sensitivity, eligibility traces through `DELAY`, and juvenile subsidies on a **hand-built diagnostic panel** of minimal correct, partially correct, chance-level, anti-correlated, silent-stream, and detour-bloated circuits.
   - **Hard Stage Gate:** `M1b` growth code does not execute in milestone verification unless `M1a` passes every pre-registered separation and $\Delta L$-tracking gate (`G1A.1`–`G1A.4`). If energy changes fail to separate correct from incorrect or detour circuits, Charter §7 mandates halting before growth and redesigning credit assignment.
2. **Stage M1b — First to second neuron (Hypothesis H2):** Holding the validated `M1a` economy fixed, enable online **homeostatic first-neuron seeding**, **cascade-correlation residual corrector sprouting** ($\hat{y}(t) = y_1(t) + \hat{r}_1(t) + \hat{r}_2(t) + \cdots \pmod{2^k}$), causal $z^{-1}$ delay-tap recruitment, and **invariant-preserving starvation pruning** from an empty lattice on the Tier-1 suite (`T1` plus non-polynomial `T3b_CounterModN` completeness probes).

### 1.1 Resolved Design Decisions

| Topic | Decision for M1 | Rationale |
| :--- | :--- | :--- |
| **Pre-flight M0 review hardening (`WP-M1.0`)** | Close the two remaining [milestone_0_review.md](milestone_0_review.md) evidence items first: assert analytical noisy $C_\mu$ in `verify-m0` Gate `G2`, and embed a deterministic source-tree digest (`source_tree_fnv1a128`) in `BuildProvenance` | Ensures all M1a and M1b JSON run reports carry reconstructible source provenance (even during iterative worktree runs) and verified noisy ground truth |
| **Strict zero-float `substrate` boundary** | Keep `src/substrate/` 100% integer-only (`u64`); place all `f64` reserves $E_i(t)$, eligibility traces, surprisal rewards, and mutual-information diagnostics inside `src/structural/` | Preserves Charter §2.1 & §9 layer separation so exact arithmetic and formal equivalence in M4b remain uncontaminated by floating-point state |
| **Centered exact-match surprisal reward** | Pay exact-match surprisal $-\log_2 p_{\text{null}}(s_t)$ minus the pre-observation null chance expectation $p_{\text{null}}(\hat{s}_t)(-\log_2 p_{\text{null}}(\hat{s}_t))$ on driven ports (with raw uncentered surprisal and raw accuracy retained as explicit §8 ablations) | Solves the sparse-vs-chance impossibility (Section 3.2): blind or majority-guessing circuits have exact expected flux $0$ on *every* symbol skew and starve under maintenance, while rare-event predictors earn positive net flux |
| **Conserved retrograde DAG sensitivity + discounted `DELAY` traces** | Partition each port's net trophic flux $R^{\text{net}}_p(t)$ across upstream contributors (`sum of shares == 1.0`), combining whole-node counterfactual DAG perturbation with ring-identity input gating and $\gamma$-discounted register ancestry vectors | Guarantees an exact macro-micro conservation law ($\sum_i \Delta E_i = R^{\text{net}} - C_{\text{maint}}$) so $\Delta E$ tracks $-\Delta L$, reconvergent cancelling detours (`Sub(Add(x, b), b)`) and stuck-zero branches (`Add(x, Delay(0))`) get zero credit, and multi-`Delay` chains do not starve |
| **State- and phase-preserving graph surgery** | Implement deterministic node/edge excision and compaction on `EditableLattice` returning an explicit old-to-new `NodeId` and register map | Prevents online sprouting or starvation pruning from resetting surviving `Delay` registers to static `init` values and corrupting live attractor phase |
| **Spatial lattice routing in `ArkheM1`** | Embed `ArkheM1` circuits directly on `LatticeCircuit`, supporting both `LocalVonNeumann` / `LocalMoore` (shortest-path empty-cell placement and `Split` relay chains) and `WellMixed` | Activates `WiringTopology` end-to-end inside `execute_experiment` for `ArkheM1` and unlocks Charter §8's *"lattice vs. well-mixed wiring"* ablation |
| **Continuous `RNN/LSTM` baseline scope** | Keep the `RNN/LSTM` BPTT baseline deferred to **Milestone 2 (`M2`)**, where Charter §7 first specifies *"compared against baselines"* alongside multi-channel `T2`; benchmark `ArkheM1` in M1b against M0's five discrete/probabilistic baselines (`RandomSearch`, `CGP`, `CTW`, `ReedsSloane`, `Exhaustive`) | Heeds the M0 review lesson against bundling orthogonal engineering stacks into one milestone while still scoring `ArkheM1` on the exact same `PredictorBaseline` + `TickBudget` harness |

### 1.2 Explicit Non-Goals (Deferred to M2–M6)

- **M2:** Redundancy sprouting (growing alternative derivations of an already-predicted signal), internal `EQZ(SUB(a, b))` comparator pruning, multi-channel **T2** benchmarks, and the continuous `RNN/LSTM` baseline.
- **M3:** Adversarial bloat-injection stress tests on large circuits, **T3a** spatially presented cellular automata, and exact-synthesis size convergence under active bloat pressure.
- **M4a–M4c:** Autonomous sleep loopback controller (`S-A` consistency-only vs. `S-B` consistency-plus-replay), `egg` equality saturation, SMT bit-vector equivalence verification, derived axiom promotion, and **T4** task-family transfer.
- **M5–M6:** Heterogeneous wire widths (`TRUNC`, `EXT` in Phase W2), $k > 1$ SIMD lane packing, and active generator interventions.

### 1.3 Deliverable Traceability Matrix

| Charter / Review Requirement | Hypothesis / Gate | Responsible Work Package(s) | Target Modules |
| :--- | :--- | :--- | :--- |
| Close M0 review G2 noisy $C_\mu$ assertion & dirty-tree source digest | Pre-flight (`G0+`) | `WP-M1.0` | `scripts/build.rs`, `src/config.rs`, `src/verify.rs` |
| Node energy reserves $E_i(t)$, maintenance $c_{\text{base}}$, `SPLIT` tax $c_{\text{split}} > c_{\text{base}}$, juvenile subsidy (§3.1.2, §3.1.3) | **H1** (`G1A.1`, `G1A.2`) | `WP-M1.1` | `src/structural/economy.rs` |
| Centered surprisal-weighted exact-match reward & null-calibrated MDL $\Delta L_{\text{null}}$ (§3.1.4, §3.4) | **H1** (`G1A.2`, `G1A.4`) | `WP-M1.1` | `src/structural/economy.rs`, `src/metrics/mdl.rs` |
| Counterfactual DAG sensitivity ($k = 1$ exact, $k \le 4$ exact, $k > 4$ sampled) & `DELAY` eligibility traces (§3.1.3) | **H1** (`G1A.1`, `G1A.3`) | `WP-M1.2` | `src/structural/sensitivity.rs`, `src/structural/economy.rs` |
| Hand-built M1a fixed-circuit audit panel & hard pre-growth kill gate (§7 M1a) | **H1** (`G1A.1`–`G1A.4`) | `WP-M1.3` | `src/verify_m1.rs`, `tests/m1a_economy.rs` |
| Invariant- and phase-preserving graph/lattice surgery & starvation pruning (§2.5, §3.1.6) | **H2** (`G1B.3`) | `WP-M1.4` | `src/structural/surgery.rs`, `src/structural/lattice.rs` |
| Residual structure diagnostics (MI, autocorrelation, noise-floor stopping) & finite-ring corrector sprouting (§3.1.5, §3.3) | **H2** (`G1B.1`, `G1B.2`) | `WP-M1.5` | `src/structural/growth.rs`, `src/structural/learner.rs` |
| CLI subcommands (`verify-m1a`, `verify-m1b`, `verify-m1`), §8 M1 ablations, and holdout sign-off | **H1**, **H2** (`G1B.4`) | `WP-M1.6` | `src/config.rs`, `src/lib.rs`, `src/main.rs`, `tests/m1b_neurogenesis.rs` |

---

## 2. Module Layout, Dependencies, and Reproducibility Extensions

### 2.1 Zero New External Crate Dependencies

Milestone 1 introduces **zero new third-party crates** to `Cargo.toml`. All metabolic accounting, counterfactual DAG evaluation, empirical mutual-information / entropy estimators, shortest-path grid routing, and configuration schemas are implemented in deterministic standard-library Rust on top of the locked Milestone 0 dependencies (`clap`, `rand`, `rand_chacha`, `rand_core`, `serde`, `serde_json`, `toml`). Ambient OS randomness (`getrandom` / `thread_rng`) remains absent from `Cargo.lock`, and `HashMap` / `HashSet` remain banned in favour of `Vec`, `BTreeMap`, and `BTreeSet`.

### 2.2 Target Repository Additions

```text
scripts/
└── build.rs                        # + ARKHE_SOURCE_TREE_FNV1A128 deterministic digest
src/
├── config.rs                       # + EconomyConfig, GrowthConfig, BaselineConfig::ArkheM1
├── rng.rs                          # + StreamDomain::{SensitivityProbe, StructuralGrowth}
├── verify.rs                       # + G2 noisy C_mu assertion
├── verify_m1.rs                    # Pre-registered M1a (G1A.1–G1A.4) & M1b (G1B.1–G1B.4) suites
├── metrics/
│   └── mdl.rs                      # + null_calibrated_mdl_delta against NullModel baseline
└── structural/
    ├── mod.rs                      # Public structural-layer exports
    ├── lattice.rs                  # + node/edge mutation helpers & BFS neighbour routing
    ├── sensitivity.rs              # Counterfactual DAG perturbation & ring-identity gating
    ├── economy.rs                  # Reserves, maintenance, centered surprisal, eligibility traces
    ├── surgery.rs                  # Phase-preserving node addition, starvation excision, Split collapse
    ├── growth.rs                   # Residual MI / autocorrelation diagnostics & corrector kernels
    └── learner.rs                  # ArkheLearner implementing PredictorBaseline under TickBudget
tests/
├── m1a_economy.rs                  # Fixed-circuit H1 separation, conservation, and detour tests
└── m1b_neurogenesis.rs             # Online H2 residual reduction, phase preservation, & ablations
```

### 2.3 `SeedBank` Isolation and `BuildProvenance` Hardening

1. **Backwards-compatible `StreamDomain` extension (`src/rng.rs`):** Append two orthogonal domain tags without altering tags `0..=3` (keeping every Milestone 0 golden ChaCha8 vector in `tests/determinism.rs` byte-identical):
   - `StreamDomain::SensitivityProbe { tick_block: u64 }` (tag `4`): draws counterfactual perturbation values for widths $k > 4$.
   - `StreamDomain::StructuralGrowth { epoch: u32 }` (tag `5`): breaks ties deterministically during exploratory first-neuron placement or equal-score candidate selection.
2. **Source-tree fingerprint (`scripts/build.rs`, `src/config.rs`):** Compute a canonical 128-bit FNV-1a digest (`source_tree_fnv1a128: String`) over sorted relative paths and raw bytes of `Cargo.toml`, `Cargo.lock`, `scripts/build.rs`, and every `*.rs` file under `src/`. Emit `cargo:rerun-if-changed` for `src` and `Cargo.toml` so `RunReport` JSON manifests uniquely identify the exact compiled Rust source even when `git_dirty == Some(true)` or inside `.git`-free `cargo package` tarballs.

---

## 3. Stage M1a Specification: Metabolic Economy and Retrograde Credit

### 3.1 Node Energy Reserves and MDL-Calibrated Maintenance (`economy.rs`)

Every placed node $i \in \lbrace 0, \dots, N - 1 \rbrace$ in a circuit holds a metabolic reserve $E_i(t) \in \mathbb{R}$ (`f64` inside `EconomyState`), initialised upon placement to $E_{\text{init}} > 0$ with juvenile grace counter $\tau_i(0) = T_{\text{juvenile}}$.

On every tick $t$, each node $i$ incurs a deterministic per-tick maintenance drain $c_{\text{maint}}(i)$ calibrated directly to its structural description length $\Delta L(\text{circuit})$ divided by an amortisation horizon $\tau_{\text{mdl}}$ (default $\tau_{\text{mdl}} = 400$ ticks):

$$
c_{\text{maint}}(i) = c_{\text{node}}(\text{op}_i) + c_{\text{wire}} \cdot d_{\text{in}}(i)
$$

where:

- $c_{\text{node}}(\text{Primitive::Split}) = c_{\text{split}}$ and $c_{\text{node}}(\text{other}) = c_{\text{base}}$, enforcing Charter §2.3 & §3.1.2 ($c_{\text{split}} > c_{\text{base}} > 0$; defaults $c_{\text{base}} = 0.020\text{ bits/tick}$, $c_{\text{split}} = 0.035\text{ bits/tick}$).
- $d_{\text{in}}(i)$ is the sum of step distances of edges terminating at `NodeIn { node: i, .. }` (plus any `BoundaryOut` edge driven by node $i$), priced at $c_{\text{wire}} = 0.004\text{ bits/tick/step}$ (`1` step per edge in `WellMixed`, Manhattan/Chebyshev step distance in `LocalVonNeumann`/`LocalMoore`).
- Optional homeostatic reserve cap `max_energy_reserve: Option<f64>`: set to `None` in M1a fixed-structure proportionality audits (so $\Delta E(T) = \sum_i (E_i(T) - E_i(0))$ scales linearly without clipping) and to a finite ceiling (default `Some(25.0)`) during long-horizon online M1b runs so obsolete structures cannot coast indefinitely after environment shifts.

### 3.2 Centered Exact-Match Surprisal Reward (Solving the Sparse-vs-Chance Dilemma)

Charter §3.1.4 specifies that an exact match on symbol $s_t \in \mathbb{Z}_{2^k}$ earns $-\log_2 p_{\text{null}}(s_t)$ under a local null model (`NullModelKind::LaplaceFrequency` default, `RepeatLast` configurable), while Charter §3.4 & §7 M1a require that **incorrect/chance circuits lose energy, sparse rare-event predictors survive, and total energy tracks $-\Delta L$**.

**Why uncentered gross payout alone violates §3.4:** On `T1_Alternating` (`0, 1, 0, 1, ...` at $k = 1$), a wrong 2-node constant-`0` circuit (`Delay(0) + Split`) matches on 50% of ticks by blind luck, earning $0.5 \times (-\log_2 0.5) = 0.250\text{ bits/tick/node}$. Meanwhile, on the sparse `T1_PeriodicMotif([0, 0, 0, 0, 0, 1])` ($p = 6$), an **exact** 5-node predictor matching 100% of ticks earns the stream's full entropy $H(1/6) / 5 \approx 0.130\text{ bits/tick/node} < 0.250$. Hence no fixed $c_{\text{base}}$ under uncentered gross reward can simultaneously sustain the exact sparse predictor and starve the blind guesser.

**Centered Exact-Match Surprisal (`RewardMode::CenteredSurprisal`, default):** Prior to observing $s_t$ on tick $t$, boundary output port $p$ commits to prediction $\hat{s}_{p, t} \in \mathbb{Z}_{2^k}$. Under the port's local prequential `NullModel` $p_{\text{null}}$, predicting $\hat{s}_{p, t}$ has blind-luck expected surprisal

$$
B_{\text{null}}(\hat{s}_{p, t}) = p_{\text{null}}(\hat{s}_{p, t}) \cdot \big(-\log_2 p_{\text{null}}(\hat{s}_{p, t})\big).
$$

Once ground truth $s_{p, t}$ is observed (and before `NullModel` updates its counts for tick $t$), the net trophic flux at port $p$ is:

```math
R^{\text{net}}_p(t) = \underbrace{\mathbf{1}[\hat{s}_{p, t} = s_{p, t}] \cdot \big(-\log_2 p_{\text{null}}(s_{p, t})\big)}_{\text{Charter §3.1.4 exact-match surprisal}} - \underbrace{p_{\text{null}}(\hat{s}_{p, t}) \cdot \big(-\log_2 p_{\text{null}}(\hat{s}_{p, t})\big)}_{\text{Pre-observation chance expectation}}
```

Because $\mathbf{1}[\hat{s}_{p, t} = s_{p, t}]$ is non-zero only when $\hat{s}_{p, t} = s_{p, t}$, this simplifies identically to:

$$
R^{\text{net}}_p(t) = \big(\mathbf{1}[\hat{s}_{p, t} = s_{p, t}] - p_{\text{null}}(\hat{s}_{p, t})\big) \cdot \big(-\log_2 p_{\text{null}}(\hat{s}_{p, t})\big).
$$

This exact-match reward rule satisfies four mathematical invariants across all widths $k \ge 1$ and all symbol distributions:

1. **Zero expected flux for every blind, uncorrelated, or lazy majority-class guesser:** For *any* constant or independent guess $a \in \mathbb{Z}_{2^k}$ on *any* stationary distribution $p$ (50/50, 5/6 sparse, or 99/100 extreme skew), $\mathbb{E}[R^{\text{net}}] = (p(a) - p(a))(-\log_2 p(a)) \equiv 0$. Subtracting $c_{\text{maint}} > 0$ drives every non-predictive circuit strictly below zero energy.
2. **Negative flux on anti-correlated errors (Asymmetry Principle, §1.2 & §3.1.6):** Whenever $\hat{s}_{p, t} \ne s_{p, t}$, $R^{\text{net}}_p(t) = -B_{\text{null}}(\hat{s}_{p, t}) \le 0$, actively withdrawing credit from the responsible pathway.
3. **Zero asymptotic flux on silence / constant DC lines (`T1_Constant`):** As $p_{\text{null}}(c) \to 1$, $-\log_2 p_{\text{null}}(c) \to 0$ and $R^{\text{net}}_p(t) \to 0$, so redundant nodes placed on unchanging channels starve automatically.
4. **Strictly positive flux on every informative predictor:** Whenever $\Pr(s_t = a \mid \hat{s}_t = a) > p_{\text{null}}(a)$—including both exact predictors and partially accurate **first neurons** in M1b—$\mathbb{E}[R^{\text{net}}_p(t)] > 0$. On sparse `[0, 0, 0, 0, 0, 1]`, the lazy always-`0` circuit averages $0.000\text{ bits/tick}$ and starves, whereas the exact period-6 predictor earns $+0.396\text{ bits/tick}$ ($+2.15\text{ bits}$ on each rare `1`!) and thrives.

To support Charter §8's reward ablations directly, `EconomyConfig` exposes `RewardMode`:

- `RewardMode::CenteredSurprisal` (default, above),
- `RewardMode::UncenteredSurprisal` ($\mathbf{1}[\hat{s} = s](-\log_2 p_{\text{null}}(s))$ without baseline subtraction),
- `RewardMode::RawAccuracy` ($\mathbf{1}[\hat{s} = s] - 2^{-k}$, unweighted by surprisal).

### 3.3 Counterfactual DAG Sensitivity and Ring-Identity Input Gating (`sensitivity.rs`)

Charter §3.1.3 requires that credit flows backward *only* along pathways that actually contributed to the checked output, blocking free-riders such as reconvergent cancelling detours (`Sub(Add(x, b), b)`), masked branches (`Mul(b, 0)`), and passive identity attachments (`Add(x, Delay(0))`), while **preserving** legitimate non-zero constant subgraphs (`Delay(c) <-> Split` for $c \ne 0$ in `Add(s(t-1), c)`).

On each tick $t$, given the current wire values `wires[0..num_wires]` produced by `ScalarState::step`:

1. **Whole-node counterfactual DAG perturbation $s_{v \to T}(t) \in [0, 1]$:**
   For each combinational or state-reading node $v$ in the circuit and each downstream sink target $T \in \lbrace \text{BoundaryOut}(p), \text{DelayLatch}(r) \rbrace$:
   - Let $y_v(t) \in \mathbb{Z}_{2^k}$ be node $v$'s emitted value on tick $t$ (propagated identically to both output ports if $v$ is `Primitive::Split`).
   - Construct the counterfactual candidate set $\mathcal{C}_v(t) \subseteq \mathbb{Z}_{2^k} \setminus \lbrace y_v(t) \rbrace$:
     - For **$1 \le k \le 4$** (covering $k = 1$ with $|\mathcal{C}| = 1$, $k = 2$ with $|\mathcal{C}| = 3$, and $k = 4$ with $|\mathcal{C}| = 15$): evaluate **all** $2^k - 1$ exact counterfactual values deterministically.
     - For **$k > 4$**: draw $M$ distinct values without replacement (default $M = 8$) from `StreamDomain::SensitivityProbe { tick_block: t }`.
   - Re-evaluate only the topological combinational cone downstream of $v$ within tick $t$, and set $s_{v \to T}(t)$ to the fraction of counterfactual values $c \in \mathcal{C}_v(t)$ for which target $T$'s value changes ($T(v \leftarrow c) \ne T(v \leftarrow y_v(t))$).
   - **Immediate detour rejection:** For any reconvergent cancelling subgraph such as `Sub(Add(x, b), b)` (where `b` fans out via `Split`), clamping `Split` (or `b`) to *any* $c \in \mathbb{Z}_{2^k}$ yields $(x + c) - c \equiv x \pmod{2^k}$, so $s_{b \to T}(t) = 0$ identically! Likewise, `Mul(b, 0)` yields $c \cdot 0 = 0$, giving $s_{b \to T}(t) = 0$.

2. **Ring-identity input gating across binary primitives:**
   Perturbation alone does not catch a stuck-at-zero wire $z \equiv 0$ attached to `Add(x, z)`, because perturbing $z$ from $0 \mapsto 1$ changes $x + 0$ to $x + 1$ even though $z \equiv 0$ was the passive additive identity every tick. `EconomyState` tracks a monotonic boolean `ever_active[node]` per node (`true` once the node emits a non-zero symbol on `Add`/`Sub` or transforms a non-zero input; for `Mul`, non-`1` emission). Across a binary combinational node $u$ with input wire values $(a, b)$:
   - **`Primitive::Add`:** If $b = 0$ and $a \ne 0$, input port `1` acted as additive identity (`raw_weight = (1.0, 0.0)`); if $a = 0$ and $b \ne 0$, `raw_weight = (0.0, 1.0)`; if $a = 0$ and $b = 0$, any input node with `ever_active == false` receives `0.0` (blocking stuck-at-zero `Delay(0)` parasites while sharing credit across genuinely oscillating signals).
   - **`Primitive::Sub`:** If $b = 0$ and ($a \ne 0$ or `rhs` has `ever_active == false`), input port `1` (`- 0`) receives `0.0`.
   - **`Primitive::Mul`:** Local input perturbation count $\lvert \lbrace c \ne x_j : f(x_j \leftarrow c) \ne f(x) \rbrace \rvert$, gated to `0.0` on multiplicative identity $1$ if the source never emitted any value $\ne 1$.
   - **Legitimate constant preservation:** A self-looping non-zero constant `d = Delay(init = c) <-> Split` ($c \in \mathbb{Z}_{2^k} \setminus \lbrace 0 \rbrace$, e.g. $c = 1$ in `Add(s(t-1), 1)`) has $y_d(t) = c \ne 0$, `ever_active == true`, and $s_{d \to T}(t) = 1.0$, so it receives full proportional credit and survives.

### 3.4 Discounted `DELAY` Eligibility Traces and Macro-Micro Energy Conservation

To carry credit across arbitrary chains and recurrent loops of `Primitive::Delay` (and `TicksPerHop` pipeline registers) without interior starvation or runaway feedback gain, every synchronous register $r \in \lbrace 0, \dots, N_{\text{reg}} - 1 \rbrace$ maintains a non-negative **ancestry eligibility vector** $\mathbf{e}_r(t) \in \mathbb{R}_{\ge 0}^N$ (initialised to $\mathbf{0}$ at $t = 0$, with temporal discount $\gamma \in (0, 1)$, default $\gamma = 0.90$):

1. **Tick-$t$ forward ancestry assembly (for each downstream target $T$):**
   - Boundary inputs have zero metabolic ancestry: $\mathbf{c}_{\text{BoundaryIn}}(t) = \mathbf{0}$.
   - Explicit `Delay` node $u$ backed by register $r_u$ emits ancestry $\mathbf{c}_u^{(T)}(t) = s_{u \to T}(t) \cdot \big(\mathbf{1}_u + \gamma \, \mathbf{e}_{r_u}(t)\big)$ (for anonymous `TicksPerHop` wire stages, $\gamma \, \mathbf{e}_r(t)$ without a node self-term).
   - Combinational node $u$ in topological order emits:
     $$
     \mathbf{c}_u^{(T)}(t) = s_{u \to T}(t) \cdot \left( \mathbf{1}_u + \sum_{j : \tilde{w}_{u, j}(t) > 0} \tilde{w}_{u, j}(t) \, \mathbf{c}_{\text{src}(u, j)}^{(T)}(t) \right),
     $$
     where $\tilde{w}_{u, j}(t)$ are the normalised ring-identity-gated input weights of $u$ ($\sum_j \tilde{w}_{u, j}(t) = 1$ when non-empty).

2. **Conserved boundary payout at port $p$:**
   Let $\mathbf{c}^{(p)}(t)$ be the ancestry vector arriving at `BoundaryOut(p)`. If $Z_p(t) = \sum_{i=0}^{N-1} c_i^{(p)}(t) > 0$:
   $$
   \Delta E_{\text{credit}, i}^{(p)}(t) = R^{\text{net}}_p(t) \cdot \frac{c_i^{(p)}(t)}{Z_p(t)}.
   $$
   (If $R^{\text{net}}_p(t) < 0$, nodes currently inside their juvenile grace period $\tau_i(t) > 0$ have negative deductions clamped to $0.0$; for mature circuits with $\tau_i(t) = 0$, $\sum_{i=0}^{N-1} \Delta E_{\text{credit}, i}^{(p)}(t) = R^{\text{net}}_p(t)$ holds to machine precision.)

3. **Synchronous register trace latch (end of tick $t$):**
   For each `Delay` register $r$ latched from input wire `in_wire(r)` (using target $T = \text{DelayLatch}(r)$ so dead detours downstream of `r` cannot pollute `r`, and capping $\lVert \mathbf{e}_r \rVert_1$ naturally by $\frac{N}{1 - \gamma}$):
   $$
   \mathbf{e}_r(t + 1) = \mathbf{c}_{\text{in-wire}(r)}^{(\text{DelayLatch}(r))}(t).
   $$

**Properties verified in M1a:**

- **Interior chain equity:** On a 5-register shift chain `BoundaryIn -> d0 -> d1 -> d2 -> d3 -> d4 -> BoundaryOut` at $\gamma = 0.90$, shares are `(16.0%, 17.8%, 19.8%, 22.0%, 24.4%)`—eliminating upstream bucket-brigade starvation.
- **Exact Macro-Micro Conservation Law:** On every mature circuit ($\tau_i = 0$, $E_{\max} = \text{None}$),
  $$
  \sum_{i=0}^{N-1} \Delta E_i(t) = \sum_{p : Z_p(t) > 0} R^{\text{net}}_p(t) - \sum_{i=0}^{N-1} c_{\text{maint}}(i),
  $$
  connecting microscopic local survival directly to macroscopic description-length improvement $-\Delta L_{\text{null}}$.

### 3.5 Juvenile Subsidy and Null-Calibrated MDL ($\Delta L_{\text{null}}$)

- **Juvenile protection (`T_juvenile`):** Newly sprouted nodes receive `E_init` (default `4.0` bits) and `juvenile_ticks = T_juvenile` (default `128` ticks in M1b, outlasting the ~100-tick noisy MDL crossover identified in M0; set to `0` in M1a's steady-state audit panel, with dedicated unit/integration tests exercising `T_juvenile > 0`). While $\tau_i(t) > 0$, node $i$ accumulates positive credit normally, has maintenance and mismatch withdrawals waived, and decrements $\tau_i(t+1) = \tau_i(t) - 1$. Once $\tau_i(t) = 0$, full maintenance and mismatch withdrawals apply; if $E_i(t) \le 0$, node $i$ is marked starved.
- **Null-calibrated MDL improvement (`null_calibrated_mdl_delta` in `src/metrics/mdl.rs`):** Alongside M0's raw `MdlBreakdown` (which encodes mismatches relative to uniform $2^k$ alternatives and defaults an empty circuit to literal `0`), compute the description-length delta relative to the prequential `NullModel` baseline $L_{\text{null}}(\text{facts}) = \sum_{t} (-\log_2 p_{\text{null}}(s_t))$:
  $$
  \Delta L_{\text{null}}(C, T) = \big(L(\text{palette}) + L(\text{circuit} \mid \text{palette}) + L_{\text{residual}}(\text{facts} \mid C)\big) - L_{\text{null}}(\text{facts}).
  $$
  On constant streams (`T1_Constant` for *any* constant $c \in \mathbb{Z}_{2^k}$), $L_{\text{null}}(\text{facts}) = O(\log T)$, so adding circuit nodes yields $\Delta L_{\text{null}} > 0$ and $\Delta E < 0$ in complete agreement.

### 3.6 Hand-Built M1a Audit Panel (`k = 1` and `k = 2`)

Before any growth code runs, Stage M1a evaluates six circuit classes per benchmark stream across $k \in \lbrace 1, 2 \rbrace$ and $\varepsilon \in \lbrace 0.0, 0.05 \rbrace$:

1. **`C_opt` (Minimal exact predictors):** `Eqz` / `Delay` reactive and `Delay <-> Delay + Split` autonomous oscillators on `T1_Alternating`; 2-`Delay` and 5-`Delay` shift predictors on `T1_PeriodicMotif([0, 1, 3])` and sparse `[0, 0, 0, 0, 0, 1]`; `Add(BoundaryIn, Delay(1) <-> Split)` on `T1_AffineLag1`; 6-node self-healing `(x + 1) + Eqz(x + 2)` on `T3b_CounterModN(3)`.
2. **`C_partial` (Informative first-neuron predictors):** 66.7%-accurate `Eqz(Delay(in))` on `[0, 1, 1]` ($k = 1$) and 66.7%-accurate `in + 1 mod 4` on `CounterModN(3)` ($k = 2$).
3. **`C_chance` (Blind / majority-class circuits):** Constant-`0` (`Delay(0) + Split`) on `0, 1, 0, 1`, lazy always-`0` on sparse `[0, 0, 0, 0, 0, 1]`, and mismatched-period (`p = 3` on `p = 2`) autonomous oscillators.
4. **`C_wrong` (Anti-correlated predictors):** `Delay(0)` lag-2 invert-mismatch on `T1_Alternating` (`0%` accuracy at $k = 1$ and $k = 2$).
5. **`C_silence` (Redundant structure on `T1_Constant`):** Active nodes placed on constant `0` and constant `3` streams.
6. **`C_detour` (Free-rider bloat):**
   - `Detour_AddZero`: `Add(y_opt, Delay(0))`
   - `Detour_CancelSub`: `Sub(Add(y_opt, Split.0(b)), Split.1(b))` with oscillating `b`
   - `Detour_MulZero`: `Add(y_opt, Mul(b, Delay(0)))`
   - `Detour_SerialDoubleNot`: `Eqz(Eqz(y_opt))` ($k = 1$) and redundant shift-loop detours.

---

## 4. Stage M1b Specification: First-to-Second Neuron Residual Neurogenesis

### 4.1 Invariant- and Phase-Preserving Graph Surgery (`surgery.rs`)

Online sprouting and starvation pruning mutate a live circuit at tick $t$. Naive deletion of a starved node $u$ would leave unconnected input slots on downstream consumers (violating Charter §2.5 Invariant 2) or force a fresh `ScalarState::new`, wiping live `Delay` register phases across the surviving network.

`EditableLattice` (`src/structural/surgery.rs`) wraps `LatticeCircuit` + `CompiledCircuit` + `ScalarState` + `EconomyState` and enforces **algebraic, phase-preserving mutations**:

1. **Starvation excision (`prune_starved_nodes`):** When mature node $u$ reaches $E_u(t) \le 0$:
   - **Additive / subtractive zero-bypass:** If $u$ feeds `Add(x, u)` or `Add(u, x)` (such as a failed corrector $\hat{r}$ summed into `Add(y_1, r_hat)`!) or `Sub(x, u)`, splicing `x` directly to the binary node's downstream consumer removes both $u$ and the now-identity `Add`/`Sub` node cleanly.
   - **Single-consumer `Split` collapse:** Whenever removing a branch leaves a `Primitive::Split` with only 1 connected output port, splice `Split`'s input directly to that surviving consumer and delete `Split` (immediately eliminating its $c_{\text{split}}$ tax); if 0 outputs remain, delete `Split` and sweep upstream dead-end nodes iteratively to fixed point.
   - **Cascade cleanup:** Any downstream combinational node whose required input lost its driver without an identity bypass is excised alongside $u$; if `BoundaryOut(p)` loses its driver, `BoundaryOut(p)` reverts cleanly to unconnected (`0` default, valid under §2.5 Invariant 2).
2. **State-preserving recompilation (`recompile_preserving_state`):**
   - Compact surviving nodes in ascending coordinate/creation order `0..N_alive`, emitting an explicit bijective mapping `surviving_old_to_new: Vec<(NodeId, NodeId)>`.
   - Re-lower `LatticeCircuit::lower()` and `Circuit::compile()`, asserting `Ok(compiled)` on every edit.
   - Copy live register values `old_state.registers()[old_reg]` into `new_state.registers()[new_reg]` for every surviving `Delay` node and unchanged `TicksPerHop` edge, while initialising only newly sprouted `Delay` registers to their `init` value.
   - Permute `EconomyState` (`E_i`, `juvenile_ticks`, `ever_active`, and every register's eligibility vector $\mathbf{e}_r \in \mathbb{R}^{N_{\text{alive}}}$) through the exact same index map.

### 4.2 Spatial Anchoring and Local Lattice Routing (`lattice.rs`)

- **Default perimeter layout (`GridShape` default $8 \times 8$ for $K = 1$):** Place `BoundaryIn(0)` at `BoundaryAnchor { side: North, offset: 3 }` (adjacent cell `(0, 3)`) and `BoundaryOut(0)` at `BoundaryAnchor { side: North, offset: 4 }` (adjacent cell `(0, 4)`), or configurable perimeter anchors in `GrowthConfig`. Because `(0, 3)` and `(0, 4)` have Manhattan distance $1$, even a minimal 1–2 node first neuron wires locally under `WiringTopology::LocalVonNeumann` + `WireDelayMode::Combinational`, while stacked correctors expand radially into interior rows `1..8`.
- **Local routing under `LocalVonNeumann` and `LocalMoore`:** When sprouting a new primitive that connects source endpoint $A$ to target endpoint $B$ at lattice distance $d > 1$ in `Combinational` mode, run deterministic Breadth-First Search (`BFS` with lexicographic `(row, col)` tie-breaking) over unoccupied grid cells to place $d - 1$ intermediate relay cells (`Primitive::Split` with port `1` left free for future taps), or set `hops = d` directly under `WireDelayMode::TicksPerHop`. If the grid is full or no local route exists within `max_relay_hops` (default `4`), the sprout candidate is cleanly skipped.

### 4.3 Residual Structure Diagnostics and Noise-Floor Stopping (`growth.rs`)

Charter §3.1.5 triggers corrector sprouting **only** when the boundary residual $r(t) = s(t) - \hat{y}(t) \pmod{2^k}$ is *structured* (predictable below maximum / noise-floor entropy), preventing runaway growth once a circuit reaches an environment's irreducible entropy rate $h_\mu(\varepsilon)$.

Over a rolling post-juvenile diagnostic buffer of $W_{\text{diag}}$ ticks (default $W_{\text{diag}} = 64$, evaluated every `sprout_interval = 32` ticks once all existing nodes have $\tau_i(t) = 0$):

1. **Zero-error fast exit:** If mismatches $= 0$ across $W_{\text{diag}}$, do nothing.
2. **Candidate causal signal pool $\mathcal{U}$:** Collect time-aligned value traces over $W_{\text{diag}}$ for `BoundaryIn(c)` ($= s_c(t-1)$), every live internal wire $w_i(t)$ within spatial routing reach, and—if `Sub(BoundaryIn(c), Delay(y_hat))` is not yet materialised on the grid—virtual causal lagged residuals $r(t - \tau)$ and lagged inputs $s(t - 1 - \tau)$ for $\tau \in \lbrace 1, 2, 3 \rbrace$ (which tell the sprouter whether recruiting a new `Primitive::Delay` tap unlocks mutual information!).
3. **Three-way structure test (§3.1.5):**
   - **Constant residual offset:** Empirical mode $c^* = \arg\max_{c \in \mathbb{Z}_{2^k}} \text{count}(R = c)$ has $c^* \ne 0$ and $\text{count}(R = c^*) - \text{count}(R = 0) \ge \Delta_{\min}$ (e.g., $r(t) \equiv 1$ on `T1_AffineLag1` when $y_1(t) = s(t-1)$).
   - **Candidate Mutual Information (Cascade-Correlation criterion):** For each single signal $U \in \mathcal{U}$ and top local pair $(U_1, U_2)$, compute plug-in mutual information $\hat{I}(R ; U)$ minus the Miller–Madow finite-sample null bias:
     $$
     \hat{I}_{\text{corr}}(R ; U) = \hat{I}(R ; U) - \frac{(\lvert \mathcal{A}_U \rvert - 1)(2^k - 1)}{2 \ln 2 \cdot W_{\text{diag}}}.
     $$
   - **Positive discrete match gain (Noise-floor guard):** Require both $\hat{I}_{\text{corr}}(R ; U) > \theta_{\text{MI}}$ (default $0.05\text{ bits}$) **and** positive match improvement over predicting $0$:
     $$
     \Delta \text{Matches}(U) = \sum_{u} \max_{c \in \mathbb{Z}_{2^k}} \big(\text{count}(U = u, R = c) - \text{count}(U = u, R = 0)\big) \ge \Delta_{\min}.
     $$
   **Why this halts cleanly at $h_\mu(\varepsilon)$:** On an optimal circuit under memoryless substitution noise $\varepsilon < 1 - 2^{-k}$ (e.g., $\varepsilon = 0.05$), $\Pr(R = 0 \mid U = u) = 1 - \varepsilon \gg \frac{\varepsilon}{2^k - 1}$ for *every* causal feature $U$, so $\Delta \text{Matches}(U) = 0$ and $\hat{I}_{\text{corr}}(R ; U) \le 0$ without needing any oracle knowledge of $\varepsilon$.

### 4.4 Causal $z^{-1}$ Delay Taps and Finite-Ring Boosting (§3.3)

When the structure diagnostic triggers on residual $r_m(t) = s(t) - \hat{y}_m(t) \pmod{2^k}$:

1. **On-demand $z^{-1}$ `Delay` recruitment (§3.3):**
   - If the highest-MI feature in $\mathcal{U}$ is a lagged signal $w(t - \tau)$ not yet buffered on the grid, sprout a `Split -> Delay(init = 0)` tap adjacent to $w$ (or materialise the exact Level-0 causal residual tap `Split(y_hat) -> Delay -> Sub(BoundaryIn, Delay)` yielding $r(t-1) = s(t-1) - \hat{y}(t-1) \pmod{2^k}$ purely from substrate primitives!).
   - Under noisy streams where reactive `BoundaryIn` echoes suffer $2\varepsilon(1-\varepsilon)$ double-error penalties (M0 Trap 3) and residual lag-autocorrelation $\hat{I}_{\text{corr}}(R(t) ; R(t - p)) > \theta_{\text{MI}}$ persists, allow autonomous state-loop closure (`Delay` ring synced to current live values) to replace the noisy reactive driver when its replayed MDL + net energy strictly dominates on `W_diag`.

2. **Bounded local corrector micro-motifs $\mathcal{K}(u_1, u_2)$:**
   Given the top-1 MI wire $u_1$ (and top complementary local wire $u_2$ maximising $\hat{I}_{\text{corr}}(R ; u_1, u_2)$), evaluate a small, fixed Level-0 micro-motif bank ($\le 24$ candidates of 1–3 primitives, **metering every candidate evaluation tick on $W_{\text{diag}}$ against `TickBudget`**):
   - Direct / unary: $u_1$, $\operatorname{EQZ}(u_1)$, constant bias `(Delay(c) <-> Split)` ($c \in \lbrace 1, \dots, \min(2^k - 1, 7) \rbrace$).
   - Shifted / scaled indicator (for $k > 1$): $\operatorname{EQZ}(u_1 + c)$, $c \cdot \operatorname{EQZ}(u_1)$, $c \cdot u_1$.
   - Binary local interaction: $u_1 + u_2$, $u_1 - u_2$, $u_1 \cdot u_2$, $u_1 \cdot \operatorname{EQZ}(u_2)$, $\operatorname{EQZ}(u_1) \cdot \operatorname{EQZ}(u_2)$.

3. **Additive residual stacking (Finite-ring boosting, §3.3):**
   Select the motif $\hat{r}$ achieving the largest net MDL / error reduction on $W_{\text{diag}}$.
   - **First neuron (when `BoundaryOut` is currently unconnected):** Connect $\hat{r}$ directly to `BoundaryOut(p)`.
   - **Second and subsequent neurons (when current predictor $\hat{y}_m$ already drives `BoundaryOut(p)`):** Keep $\hat{y}_m$ intact, insert `Primitive::Add` at the output frontier, wire `Add.0 <- y_hat_m` and `Add.1 <- r_hat`, and drive `BoundaryOut(p) <- Add.out`:
     $$
     \hat{y}_{m+1}(t) = \hat{y}_m(t) + \hat{r}(t) \pmod{2^k}.
     $$
   All newly placed nodes receive juvenile subsidy $(E_{\text{init}}, T_{\text{juvenile}})$ and thereafter stand or fall solely under M1a's local retrograde economy.

**Worked verification trace (`T3b_CounterModN(3)` at $k = 2$, `0, 1, 2, 0, 1, 2, ...`):**

- **Epoch 1 (First predictor):** Residual $r_0 = s(t)$ has $\hat{I}(R_0 ; \text{BoundaryIn}) = 1.585\text{ bits}$; sprouting $y_1(t) = s(t-1) + 1 \pmod 4$ matches phases `1` and `2` (`66.7%` accuracy, $+0.704\text{ bits/tick}$ net energy, surviving homeostatically).
- **Epoch 2 (Second neuron / error-driven interneuron):** Residual $r_1(t) = s(t) - y_1(t) \pmod 4 = (0, 0, 1)$ has entropy $0.918\text{ bits}$ and $\hat{I}(R_1 ; y_1) = 0.918\text{ bits}$. Motif $\hat{r}_1(t) = \operatorname{EQZ}(y_1(t) + 1)$ predicts $(0, 0, 1)$ exactly. Summing $\hat{y}_2(t) = y_1(t) + \hat{r}_1(t) \pmod 4 = (s(t-1) + 1) + \operatorname{EQZ}(s(t-1) + 2) \pmod 4$ drops error from `33.3%` to **`0.0%`** with zero gradients and zero backpropagation.

### 4.5 `ArkheLearner` Integration with `PredictorBaseline` (`learner.rs`)

`ArkheLearner` implements `PredictorBaseline` (`BudgetAccountingMode::CandidateHistoryReplay`) so `HarnessRunner` scores it identically to M0's five baselines (`Scorecard` reporting KT residual loss, paired clean-phase Bayes oracle regret, internal `Delay` + hop + boundary-lag effective registers, `MdlBreakdown`, `null_calibrated_mdl_delta`, plus M1-specific telemetry `final_total_energy`, `min_node_energy`, `sprout_events`, and `starvation_prune_events`).

---

## 5. CLI Entry Points and Configuration Schema

### 5.1 CLI Subcommands (`src/main.rs`)

Preserve `arkhe run`, `arkhe yardstick`, and `arkhe verify-m0` unchanged, and add:

- `arkhe verify-m1a [--seed-offset <u64>] [--seeds <usize>]` — executes Stage M1a Gates `G1A.1`–`G1A.4` at $k = 1$ and $k = 2$ and emits structured JSON.
- `arkhe verify-m1b [--seed-offset <u64>] [--seeds <usize>]` — executes Stage M1b Gates `G1B.1`–`G1B.4` (including the §8 M1 ablation matrix) at $k = 1$ and $k = 2$.
- `arkhe verify-m1 [--seed-offset <u64>] [--seeds <usize>]` — runs `verify-m1a` first (**aborting immediately if any M1a gate fails**, enforcing Charter §7's pre-growth kill gate), then runs `verify-m1b` and subprocess determinism checks.

### 5.2 Example `ExperimentConfig` TOML for `ArkheM1`

```toml
schema_version = 1
master_seed = 42

[substrate]
width_bits = 2
wire_delay = "Combinational"
wiring_topology = "LocalVonNeumann"

[environment]
horizon_ticks = 2_000
transient_warmup_ticks = 250
generator = { kind = "T1_PeriodicMotif", motif = [0, 1, 3] }
noise = { kind = "None" }

[budget]
max_eval_ticks = 50_000

[baseline]
kind = "ArkheM1"
grid_rows = 8
grid_cols = 8

[baseline.economy]
c_base = 0.020
c_split = 0.035
c_wire = 0.004
trace_discount = 0.90
initial_energy = 4.0
juvenile_ticks = 128
max_energy_reserve = 25.0
null_model = "LaplaceFrequency"
reward_mode = "CenteredSurprisal"

[baseline.growth]
diagnostic_window = 64
sprout_interval = 32
mi_threshold_bits = 0.05
min_match_gain = 4
max_nodes = 32
```

---

## 6. Pre-Registered Acceptance Gates, Seed Banks, Ablations, and Kill Criteria

In accordance with Charter §7 and §8, all thresholds, seed counts, and kill criteria are locked **before** running experiments. Development and hyperparameter verification use **20 development seeds (`--seed-offset 0 --seeds 20`)**; final milestone sign-off requires passing an **untouched holdout seed bank (`--seed-offset 10000 --seeds 20`)** with zero post-inspection parameter changes.

### 6.1 Stage M1a Gates (Economy on Fixed Structure — Hypothesis H1)

| Gate | Requirement | Exact Quantitative Pass Criterion ($k \in \lbrace 1, 2 \rbrace$) | Kill / Revisit Criterion |
| :--- | :--- | :--- | :--- |
| **`G1A.1` — Conservation & substrate purity** | Exact macro-micro energy conservation, zero-float `substrate`, sensitivity convergence | $\lvert \sum_i \Delta E_i(t) - (\text{flux}(t) - \text{maint}(t)) \rvert < 10^{-12}$ on 100% of mature ticks; `0` occurrences of `f32`/`f64` in `src/substrate/`; $k = 8$ sampled sensitivity ($M = 32$) matches exact 255-value sensitivity within $\pm 0.05$ | Conservation violation or float leak in `substrate`: block merge |
| **`G1A.2` — Correct vs. incorrect separation** | Correct circuits gain energy & sustain all interior `Delay` nodes; wrong, chance, and silent-stream circuits lose energy | Across 20 seeds ($\varepsilon \in \lbrace 0.0, 0.05 \rbrace$) at $k = 1$ and $k = 2$: **100%** of `C_opt` circuits on non-constant tasks satisfy $\Delta E(T) > 0$ and $\min_i E_i(T) > E_{\text{init}}$; **100%** of `C_wrong`, `C_chance` (including lazy always-`0` on sparse `5/6` motif), and `C_silence` (`T1_Constant`) satisfy $\Delta E(T) < 0$ | **Charter M1a Kill Criterion:** Any task/width fails separation $\implies$ **halt before M1b** and redesign reward/credit |
| **`G1A.3` — Detour & free-rider penalty** | Non-contributing detours receive zero positive credit and lower circuit $\Delta E$ | `Detour_AddZero`, `Detour_CancelSub`, and `Detour_MulZero` detour nodes receive `0.0` cumulative credit after tick `1` ($\Delta E_{\text{detour}} = -T \cdot C_{\text{maint}}$); serial detours satisfy $\Delta E(C_{\text{serial}}) < \Delta E(C_{\text{opt}})$ on 100% of cases | Positive credit leaks into cancelling or zero-identity branches: redesign `sensitivity.rs` |
| **`G1A.4` — $\Delta E$ tracks $-\Delta L_{\text{null}}$** | Local metabolic balance tracks global description-length improvement (§3.4) | Across the 6-class M1a circuit panel on every non-constant task at $k = 1$ and $k = 2$, Spearman rank correlation $\rho_s(\Delta E(T), -\Delta L_{\text{null}}(T)) \ge 0.90$; **zero** instances where $\Delta E(T) > 0$ while $\Delta L_{\text{null}}(T) > 0$ | $\rho_s < 0.90$ or §3.4 sign inversion: recalibrate maintenance/null-model scaling before M1b |

### 6.2 Stage M1b Gates (First to Second Neuron — Hypothesis H2)

| Gate | Requirement | Exact Quantitative Pass Criterion ($k \in \lbrace 1, 2 \rbrace$) | Kill / Revisit Criterion |
| :--- | :--- | :--- | :--- |
| **`G1B.1` — Stepwise first-to-second neuron reduction** | Second (and stacked) corrector reduces error as residual entropy/MI predicts, without backprop | On `T1_PeriodicMotif([0, 1, 1])` ($k = 1$), `T1_PeriodicMotif([0, 1, 3])` ($k = 2$), `T1_AffineLag1` ($k = 2$), and `T3b_CounterModN(3)` ($k = 2$): post-first-neuron loss $L_1 < L_0$, post-second-neuron loss $L_2 < L_1$, and empirical error drop matches diagnostic $\hat{I}(R_1 ; U^*)$ within $0.05\text{ bits/symbol}$ on $\ge 19/20$ seeds | Correctors fail to stack or overwrite $y_1$'s survival: revisit additive boosting / identity gating |
| **`G1B.2` — T1 suite convergence & noise-floor halting** | Consistent error reduction to $h_\mu(\varepsilon)$ across seeds on local 2D lattices | Across 20 dev and 20 holdout seeds at $k = 1$ and $k = 2$ under `LocalVonNeumann`: (a) **Noiseless ($\varepsilon = 0$):** `0` active steady-state nodes on `T1_Constant`; `0` post-transient mismatches on $\ge 18/20$ seeds across `T1_Alternating`, `T1_PeriodicMotif(p=3)`, and `T1_AffineLag1`; (b) **Noisy ($\varepsilon = 0.05$):** `paired_residual_regret_bits_per_symbol <= 0.025` on $\ge 18/20$ seeds for `T1_Constant` and `T1_Alternating`, strictly lower mean paired regret than a 1-node reactive echo on noisy `T1` tasks, and bounded final size `node_count <= max_nodes` (growth halts at noise floor) | **Charter M1b Kill Criterion:** No consistent error reduction across seeds on T1 $\implies$ revisit the §3.1.5 sprouting criterion |
| **`G1B.3` — Live invariant, phase, & budget integrity** | Every online sprout and starvation prune preserves §2.5 invariants, live register phases, and `TickBudget` | `0` `LatticeError` or `InvariantError` failures across all ticks and seeds; autonomous oscillator phase error $= 0$ across forced sprout+prune surgery tests; `tick_budget_consumed <= max_eval_ticks` everywhere | Any post-surgery compile error or phase reset: block sign-off |
| **`G1B.4` — Charter §8 M1 ablations & subprocess reproducibility** | Empirical verification of M1 ablations + separate-process determinism | Execute all four M1 ablations (Section 6.3) with expected directional effects verified; two separate OS child processes across a 6-config `ArkheM1` matrix ($k \in \lbrace 1, 2 \rbrace$, `LocalVonNeumann` & `WellMixed`, $\varepsilon \in \lbrace 0.0, 0.05 \rbrace$, `Combinational` & `TicksPerHop`) emit byte-identical `RunReport` JSON | Nondeterministic output across processes: fix ordering/seeding immediately |

### 6.3 Charter §8 Milestone 1 Ablation Matrix

| Ablation Axis | Default Setting | Ablated Setting | Measured Hypothesis Check |
| :--- | :--- | :--- | :--- |
| **1. Reward weighting** | `RewardMode::CenteredSurprisal` | `UncenteredSurprisal` & `RawAccuracy` | `RawAccuracy` sustains useless nodes on `T1_Constant(0)` and lazy always-`0` circuits on sparse `[0, 0, 0, 0, 0, 1]`, whereas `CenteredSurprisal` starves both and sustains the rare-event predictor |
| **2. `SPLIT` fan-out tax** | $c_{\text{split}} = 0.035 > c_{\text{base}}$ | $c_{\text{split}} = 0.0$ | $c_{\text{split}} > c_{\text{base}}$ yields lower mean `split_count` and lower `circuit_bits` at equal post-transient accuracy |
| **3. Spatial locality** | `WiringTopology::LocalVonNeumann` | `WiringTopology::WellMixed` | Quantifies spatial routing relay overhead (`circuit_bits`, `node_count`, `ticks_to_solution`) while verifying both solve `T1` |
| **4. Wire propagation delay** | `WireDelayMode::Combinational` | `WireDelayMode::TicksPerHop` | Verifies that hop latency is compensated by causal lag alignment and `TicksPerHop` eligibility traces on `T1` |

---

## 7. Phased Work Package Roadmap

Each work package is a self-contained, reviewable commit slice keeping `cargo fmt --check`, `cargo clippy --locked --all-targets --all-features -- -D warnings`, and `cargo test --locked` green throughout.

### WP-M1.0: Pre-Flight Instrument Hardening (`scripts/build.rs`, `src/config.rs`, `src/verify.rs`)

- **Goal & Scope:** Close out the two narrow residual observations from [milestone_0_review.md](milestone_0_review.md) before writing M1 learning code.
- **Tasks:**
  1. Compute deterministic `ARKHE_SOURCE_TREE_FNV1A128` in `scripts/build.rs` over sorted manifest and `src/**/*.rs` files; expose `source_tree_fnv1a128` in `BuildProvenance`.
  2. Assert analytical noisy $C_\mu$ ($\lvert C_\mu^{\text{MSP}} - \log_2 p_{\min} \rvert < 10^{-8}$ wherever `msp_truncated == false`) alongside noisy $h_\mu$ in `src/verify.rs` Gate `G2`.
  3. Add `StreamDomain::{SensitivityProbe, StructuralGrowth}` in `src/rng.rs` and verify existing M0 golden vectors remain unchanged.
- **Verification:** `cargo test --locked` and `cargo run --locked --release -- verify-m0 --seed-offset 0 --seeds 20` pass cleanly.

---

### WP-M1.1: Metabolic Reserves, Maintenance Pricing, and Centered Surprisal (`src/structural/economy.rs`, `src/metrics/mdl.rs`)

- **Goal & Scope:** Implement node energy state, maintenance costs, juvenile grace accounting, `RewardMode`, and null-calibrated MDL $\Delta L_{\text{null}}$.
- **Tasks:**
  1. Define `EconomyConfig`, `RewardMode`, `NodeEconomy`, and `EconomyState` in `src/structural/economy.rs`.
  2. Implement per-port prequential centered surprisal flux $R^{\text{net}}_p(t)$ backed by `metrics::NullModel` (`LaplaceFrequency` and `RepeatLast`).
  3. Add `null_calibrated_mdl_delta` in `src/metrics/mdl.rs`.
- **Verification:** Unit tests prove expected flux $\mathbb{E}[R^{\text{net}}] = 0$ on arbitrary skewed distributions for blind guesses, $\to 0$ on `T1_Constant(0)` and `T1_Constant(3)`, and $> 0$ on sparse `[0, 0, 0, 0, 0, 1]` exact predictors.

---

### WP-M1.2: Counterfactual DAG Sensitivity and `DELAY` Eligibility Traces (`src/structural/sensitivity.rs`, `src/structural/economy.rs`)

- **Goal & Scope:** Implement whole-node counterfactual DAG perturbation, ring-identity input gating, and $\gamma$-discounted register ancestry traces.
- **Tasks:**
  1. Expose read-only topological inspection helpers on `CompiledCircuit` (in `src/substrate/eval.rs`, keeping `substrate` zero-float) so `src/structural/sensitivity.rs` can evaluate localized counterfactual cones for $1 \le k \le 64$.
  2. Implement exact $(2^k - 1)$-value enumeration for $k \le 4$ and `SensitivityProbe`-seeded sampling for $k > 4$, plus ring-identity gating (`Add`/`Sub`/`Mul` with `ever_active`).
  3. Wire $\gamma$-discounted register ancestry vectors $\mathbf{e}_r(t)$ and conserved payout normalisation into `EconomyState::step_tick`.
- **Verification:** Unit tests confirm machine-precision conservation ($< 10^{-12}$), equitable shares across a 5-`Delay` chain, survival of `Delay(1) <-> Split` constant motifs, and `0.0` credit on `Detour_AddZero`, `Detour_CancelSub`, and `Detour_MulZero`.

---

### WP-M1.3: Stage M1a Audit Suite and Pre-Growth Kill Gate (`src/verify_m1.rs`, `tests/m1a_economy.rs`)

- **Goal & Scope:** Construct the complete hand-built M1a 6-class circuit panel across $k = 1$ and $k = 2$, wire `arkhe verify-m1a`, and validate **H1** before writing M1b growth logic.
- **Tasks:**
  1. Build canonical spatial and abstract circuits for `{C_opt, C_partial, C_chance, C_wrong, C_silence, C_detour}` across `T1` and `T3b_CounterModN(3)`.
  2. Implement automated checks for Gates `G1A.1`–`G1A.4` (including Spearman $\rho_s(\Delta E, -\Delta L_{\text{null}}) \ge 0.90$) and `arkhe verify-m1a`.
  3. Execute `verify-m1a` on development seeds (`0..20`); verify zero kill-criterion triggers before starting `WP-M1.4`.
- **Verification:** `cargo test --test m1a_economy` and `cargo run --locked --release -- verify-m1a --seed-offset 0 --seeds 20` pass `100%`.

---

### WP-M1.4: Invariant- and Phase-Preserving Graph & Lattice Surgery (`src/structural/surgery.rs`, `src/structural/lattice.rs`)

- **Goal & Scope:** Enable live online node/edge insertion, local `BFS` grid routing, algebraic starvation excision, `Split` collapse, and state-preserving recompilation.
- **Tasks:**
  1. Add deterministic node/edge mutation and `BFS` shortest-path empty-cell routing to `LatticeCircuit`.
  2. Implement `EditableLattice` in `src/structural/surgery.rs` with `prune_starved_nodes` (identity bypasses + 1-consumer `Split` collapse) and `recompile_preserving_state` (preserving live `ScalarState` register phases and `EconomyState` reserves/traces).
- **Verification:** Property/randomized stress tests perform 500 interleaved sprout + starvation-prune edits across $k = 1$ and $k = 2$ under `LocalVonNeumann` and `TicksPerHop`, asserting `lower().is_ok()` and exact oscillator phase preservation on every step.

---

### WP-M1.5: Residual Diagnostics, Corrector Sprouting, and `ArkheLearner` (`src/structural/growth.rs`, `src/structural/learner.rs`)

- **Goal & Scope:** Implement §3.1.5 & §3.3 residual structure detection, on-demand $z^{-1}$ `Delay` taps, finite-ring corrector stacking, and `PredictorBaseline` integration.
- **Tasks:**
  1. Implement `ResidualDiagnostics` (`growth.rs`): constant-offset check, Miller–Madow bias-corrected $\hat{I}_{\text{corr}}(R ; U)$, lagged autocorrelation, and $\Delta \text{Matches}(U)$ noise-floor stopping.
  2. Implement bounded Level-0 corrector motif evaluation (metered against `TickBudget`), `Primitive::Add` output stacking ($\hat{y}_{m+1} = \hat{y}_m + \hat{r}$), and `Sub(BoundaryIn, Delay(y_hat))` causal residual taps.
  3. Implement `ArkheLearner` (`learner.rs`) conforming to `PredictorBaseline`.
- **Verification:** Stepwise neurogenesis tests verify $L_2 < L_1 < L_0$ on `T1_PeriodicMotif`, `T1_AffineLag1`, and `T3b_CounterModN(3)` at $k = 1$ and $k = 2$, and verify growth halts at $h_\mu(\varepsilon)$ under $\varepsilon = 0.05$ noise.

---

### WP-M1.6: CLI Wiring, Ablation Suite, Holdout Sign-Off, and Packaging (`src/config.rs`, `src/main.rs`, `tests/m1b_neurogenesis.rs`)

- **Goal & Scope:** Wire `BaselineConfig::ArkheM1` into `execute_experiment` (unlocking `LocalVonNeumann` and `LocalMoore` topologies for `ArkheM1`), add `verify-m1b` and `verify-m1`, run the §8 ablation matrix, and execute the untouched holdout seed bank.
- **Tasks:**
  1. Extend `ExperimentConfig` and `RunReport` with optional `ArkheM1Telemetry`; enable spatial topologies for `ArkheM1` while preserving fast rejection for non-spatial M0 baselines.
  2. Implement Gates `G1B.1`–`G1B.4` in `src/verify_m1.rs` and wire `arkhe verify-m1b` and `arkhe verify-m1`.
  3. Lock all default constants on development seeds (`--seed-offset 0 --seeds 20`), then execute the untouched holdout sign-off (`--seed-offset 10000 --seeds 20`) and `cargo package --allow-dirty`.
- **Verification:** Full Section 9 validation checklist passes cleanly.

---

## 8. Technical Risks, Edge Cases, and Built-In Mitigations

| Risk / Edge Case | Failure Mode | Mitigation Built into M1 |
| :--- | :--- | :--- |
| **Blind or majority-class circuits surviving on positive gross surprisal** | On 50/50 streams a constant guess earns $0.5\text{ bits/tick}$, exceeding sparse-stream entropy ($0.13\text{ bits/tick/node}$) | Centered exact-match surprisal subtracts $p_{\text{null}}(\hat{s}_t)(-\log_2 p_{\text{null}}(\hat{s}_t))$, making expected flux identically $0$ for every uninformative guess across all symbol skews |
| **Stuck-at-zero free-rider `Add(x, Delay(0))` stealing 50% of credit** | Naive perturbation $0 \mapsto 1$ changes $x + 0$ to $x + 1$, falsely crediting `Delay(0)` | Ring-identity input gating weights additive `0` (`ever_active == false`) at `0.0` while preserving active non-zero constants `Delay(c != 0) <-> Split` |
| **Reconvergent cancelling detours `Sub(Add(x, b), b)`** | Local single-edge checks see `Add` and `Sub` each respond to `b` individually | Sensitivity perturbs node `b`'s output across the whole combinational DAG cone simultaneously, where $+c$ and $-c$ cancel identically ($s_b = 0$) |
| **Runaway corrector sprouting under channel noise ($\varepsilon > 0$)** | Residual entropy $H(R) \approx 0.37\text{ bits} < k$ on an already-optimal noisy circuit triggers infinite overfitting sprouts | Sprouting requires both bias-corrected $\hat{I}_{\text{corr}}(R ; U) > \theta_{\text{MI}}$ and positive discrete match gain $\Delta \text{Matches}(U) \ge \Delta_{\min}$ over predicting $0$ |
| **Phase corruption during online sprouting or starvation pruning** | Recompiling `Circuit` into a fresh `ScalarState` resets `Delay` registers to `init`, breaking autonomous limit-cycle phase | `EditableLattice::recompile_preserving_state` permutes live register states and `EconomyState` eligibility vectors across every graph edit |
| **Lattice congestion near boundary anchors under `LocalVonNeumann`** | Adjacent cells around `(North, 3..4)` saturate after several sprouts, blocking new connections | `BFS` empty-cell router threads short `Split` relay paths (up to `max_relay_hops = 4`) and starvation excision reclaims dead cells immediately |

---

## 9. Validation Checklist

Every Milestone 1 PR and final milestone sign-off must pass:

```bash
# 1. Formatting, strict Clippy lints, and unit + integration tests
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked

# 2. Regression check: Milestone 0 gates remain 100% green (including noisy C_mu & source digest)
cargo run --locked --release -- verify-m0 --seed-offset 0 --seeds 20

# 3. Pre-registered Milestone 1 gates (M1a kill gate -> M1b neurogenesis + §8 ablations)
#    Run on development seeds (0..20) and untouched holdout seeds (10000..10020)
cargo run --locked --release -- verify-m1 --seed-offset 0 --seeds 20
cargo run --locked --release -- verify-m1 --seed-offset 10000 --seeds 20

# 4. Crate packaging verification
cargo package --allow-dirty

# 5. Documentation lint conformance
files=$( { git diff HEAD --name-only --diff-filter=AM -- '*.md'; git ls-files -o --exclude-standard -- '*.md'; } | sort -u )
[ -n "$files" ] && printf '%s\n' "$files" | xargs -d '\n' npx markdownlint-cli2 --fix
npx markdownlint-cli2 "doc/**/*.md" README.md
```
