# Milestone 1 Implementation Plan: Metabolic Economy and Residual Neurogenesis

> **Status:** In progress: Stage M1a implemented and verified across 100 development banks (`20..2019`, $T \in \{1024, 2048, 4096\}$); Stage M1b pre-registered in [milestone_1b_plan.md](milestone_1b_plan.md).
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

1. **Backwards-compatible `StreamDomain` extension (`src/rng.rs`):** Append two orthogonal domain tags without altering tags `1..=4` (keeping every Milestone 0 golden ChaCha8 vector in `tests/determinism.rs` byte-identical):
   - `StreamDomain::SensitivityProbe { tick_block: u64 }` (tag `5`): draws counterfactual perturbation values for widths $k > 4$.
   - `StreamDomain::StructuralGrowth { epoch: u32 }` (tag `6`): breaks ties deterministically during exploratory first-neuron placement or equal-score candidate selection.
2. **Source-tree fingerprint (`scripts/build.rs`, `src/config.rs`):** Compute a canonical 128-bit FNV-1a digest (`source_tree_fnv1a128: String`) over sorted relative paths and raw bytes of `Cargo.toml`, `Cargo.lock`, `scripts/build.rs`, and every `*.rs` file under `src/`. Emit `cargo:rerun-if-changed` for `src` and `Cargo.toml` so `RunReport` JSON manifests uniquely identify the exact compiled Rust source even when `git_dirty == Some(true)` or inside `.git`-free `cargo package` tarballs.

---

## 3. Stage M1a Specification: Metabolic Economy and Retrograde Credit

### 3.1 Node Energy Reserves and Description Debt Allocation (`economy.rs`, `mdl.rs`)

Every placed node $i \in \lbrace 0, \dots, N - 1 \rbrace$ in a circuit holds a metabolic reserve $E_i(t) \in \mathbb{R}$ (`f64` inside `EconomyState`), initialised upon placement to $E_{\text{init}} > 0$ (default $E_{\text{init}} = 4.0\text{ bits}$) with juvenile grace counter $\tau_i(0) = T_{\text{juvenile}}$.

Rather than arbitrary per-tick maintenance rent ($c_{\text{base}}, c_{\text{split}}, c_{\text{wire}}$)—which grows linearly with horizon $T$ and conflicts with horizon-invariant MDL code length—each node owes an exact share $\Delta L_i$ of the circuit's total description length $L(C) = \sum_i \Delta L_i$ (computed via [`circuit_node_description_debts`](file:///workspaces/arkhe/src/metrics/mdl.rs) matching [`circuit_code_bits`](file:///workspaces/arkhe/src/metrics/mdl.rs)):

- **Node code bits:** Each node owes its own operator bits (palette index, `Delay` init bits, and spatial lattice placement coordinate bits).
- **Wire code bits:** Each wire is owed by the node that drives it. Wires from `BoundaryIn` are owed by the node they feed. Because `Primitive::Split` drives two wires, it owes two wire costs, satisfying Charter §3.1.2's $L(\text{split}) > L(\text{other})$ without tuning an ad hoc constant.
- **Circuit header bits:** Elias-$\gamma$ codes of node and wire counts are split equally among all nodes.

This one-time description debt is charged through the maintenance channel on the node's first mature tick (tick 0 in M1a where $T_{\text{juvenile}} = 0$). On mature ticks $t > 0$, ongoing maintenance drain is zero ($c_{\text{maint}}(i) = 0$).

Optional homeostatic reserve cap `max_energy_reserve: Option<f64>`: set to `None` in M1a fixed-structure proportionality audits (so $\Delta E(T) = \sum_i (E_i(T) - E_i(0))$ scales linearly without clipping) and to a finite ceiling (default `Some(25.0)`) during long-horizon online M1b runs so obsolete structures cannot coast indefinitely after environment shifts.

### 3.2 Calibrated Point-Forecast Reward (`RewardMode::CalibratedPointForecast`)

Charter §3.1.4 requires that reward enters at the point where a prediction is checked, rewarding informative predictions while starving blind guessers and silence. Centered surprisal $R^{\text{net}} = (\mathbf{1}[\hat{s} = s] - p)(-\log_2 p)$ had zero expected flux for blind guesses, but its positive variance induced a random walk whose cumulative spread grew as $O(\sqrt{T})$, creating a chance-predictor lottery.

**Calibrated Sequential Point-Forecast Reward (`RewardMode::CalibratedPointForecast`, default):**
Prior to observing symbol $s_t \in \mathbb{Z}_{2^k}$ at output port $p$, the circuit commits to prediction $\hat{s}_{p, t}$. Under the port's prequential `NullModel` (`LaplaceFrequency` default), let $p = p_{\text{null}}(\hat{s}_{p, t})$ be the prior probability assigned to the predicted symbol. Let $M$ be the cumulative number of previous ticks on which this port's prediction was exactly correct.

The port forms a calibrated probability forecast $\hat{q}$:

$$
\hat{q} = \frac{M + \frac{1}{2}}{t + \frac{1}{2} \cdot 2^k}, \qquad q = \max(\hat{q}, p).
$$

Here $\hat{q}$ inherits the null model's KT prior ($\frac{1}{2}$ pseudo-count per alphabet symbol). Clamping $q = \max(\hat{q}, p)$ enforces that a predictor performing no better than chance on its chosen symbol earns zero reward (matching $L_{\text{null}}$ baseline).

Once true symbol $s_{p, t}$ is observed, the reward $r_p(t)$ is the sequential log-loss savings over the null model:

$$
r_p(t) = \begin{cases}
\log_2 \dfrac{q}{p} & \text{if } \hat{s}_{p, t} = s_{p, t}, \\[1.5ex]
\log_2 \dfrac{1 - q}{1 - p} & \text{if } \hat{s}_{p, t} \ne s_{p, t}.
\end{cases}
$$

**Mathematical Properties:**

1. **Exact zero flux for constant predictors on every run:** When $\hat{s}_{p, t}$ is constant, $M$ equals the null model's observation count for that symbol, so $\hat{q} \equiv p$ and $r \equiv 0.0$ on every tick and on every seed, completely eliminating the chance lottery.
2. **Martingale concentration:** For non-predictive circuits, $2^{\sum r}$ is a non-negative martingale with mean $1$ under the null model. By Ville's inequality, $\Pr(\sup_t \sum_{\tau=1}^t r(\tau) \ge c) \le 2^{-c}$, bounding cumulative luck across all horizons without tuning a horizon-dependent rent.
3. **Prequential MDL alignment:** The cumulative flux $\sum_t r(t)$ equals $L_{\text{null}}(\text{facts}) - L(\text{facts} \mid C)$ identically, connecting trophic reward directly to data compression.
4. **Sharp punishment on established failure:** An established predictor with high $q$ that fails suffers $\log_2((1-q)/(1-p)) \ll 0$, realizing Charter §3.1.6 pruning on disagreement directly through the code.

For Charter §8 reward ablations, `EconomyConfig` continues to support `RewardMode::CenteredSurprisal`, `RewardMode::UncenteredSurprisal`, and `RewardMode::RawAccuracy`.

### 3.3 Counterfactual DAG Sensitivity, Replacing Traces, and Activity Recency Decay (`sensitivity.rs`)

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

2. **D1 Replacing Ancestry Traces (Cap at $\le 1.0$):**
   In forward ancestry assembly, every node's ancestry entry is clamped to at most $1.0$:
   $$
   c_{u, i} = s_{u \to T}(t) \cdot \min\left(1,\ \delta_{u, i} + \sum_j \tilde{w}_{u, j}(t) \, c_{\text{src}(u, j), i}\right).
   $$
   This implements replacing eligibility traces (Singh & Sutton, 1996). A node firing on the active path resets its ancestry share to $1.0$ rather than accumulating unbounded feedback gain $\frac{1}{1-\gamma} = 10$, preventing self-looping `Delay <-> Split` oscillators from monopolizing credit and starving combinational `Add` correctors.

3. **D4 Activity Recency Decay ($\gamma = 0.90$) and Ring-Identity Input Gating:**
   `NodeActivity` tracks a continuous recency weight in $[0, 1]$ per node. When a node emits a non-identity symbol (non-zero for `Add`/`Sub`, non-one for `Mul`), its recency resets to $1.0$. On identity-emitting ticks, its recency decays by factor $\gamma = 0.90$. Across binary combinational nodes, inputs emitting the identity value are scaled by their recency weight:
   - **Closes the single-fire leak:** A detour such as `Add(Base, Delay{init:1}(ConstLoop(0)))` that fires only once at tick 0 has its identity input weight decay as $\gamma^t$, bounding total leaked credit to $\sum_{t \ge 1} \gamma^t$ bits across arbitrary horizons.
   - **Preserves sparse correctors:** A periodic corrector firing every 3 ticks never drops below $\gamma^2 = 0.81$ and is credited in full on active ticks.
   - **Legitimate constant preservation:** Non-zero constant loops `Delay(init=c) <-> Split` ($c \ne 0$) maintain recency $1.0$ and receive full proportional credit.

### 3.4 Discounted `DELAY` Eligibility Traces and Exact Macro-Micro Energy Conservation

To carry credit across arbitrary chains and recurrent loops of `Primitive::Delay` (and `TicksPerHop` pipeline registers) without interior starvation or runaway feedback gain, every synchronous register $r \in \lbrace 0, \dots, N_{\text{reg}} - 1 \rbrace$ maintains a non-negative **ancestry eligibility vector** $\mathbf{e}_r(t) \in [0, 1]^N$ (initialised to $\mathbf{0}$ at $t = 0$, with temporal discount $\gamma \in (0, 1)$, default $\gamma = 0.90$):

1. **Tick-$t$ forward ancestry assembly (for each downstream target $T$):**
   - Boundary inputs have zero metabolic ancestry: $\mathbf{c}_{\text{BoundaryIn}}(t) = \mathbf{0}$.
   - Explicit `Delay` node $u$ backed by register $r_u$ emits ancestry $\mathbf{c}_u^{(T)}(t) = s_{u \to T}(t) \cdot \min\big(1, \mathbf{1}_u + \gamma \, \mathbf{e}_{r_u}(t)\big)$ (for anonymous `TicksPerHop` wire stages, $\gamma \, \mathbf{e}_r(t)$ without a node self-term).
   - Combinational node $u$ in topological order emits:
     $$
     \mathbf{c}_u^{(T)}(t) = s_{u \to T}(t) \cdot \min\left(1, \mathbf{1}_u + \sum_{j : \tilde{w}_{u, j}(t) > 0} \tilde{w}_{u, j}(t) \, \mathbf{c}_{\text{src}(u, j)}^{(T)}(t)\right),
     $$
     where $\tilde{w}_{u, j}(t)$ are the normalized ring-identity-gated recency input weights of $u$ ($\sum_j \tilde{w}_{u, j}(t) = 1$ when non-empty).

2. **Conserved boundary payout at port $p$:**
   Let $\mathbf{c}^{(p)}(t)$ be the ancestry vector arriving at `BoundaryOut(p)`. If $Z_p(t) = \sum_{i=0}^{N-1} c_i^{(p)}(t) > 0$:
   $$
   \Delta E_{\text{credit}, i}^{(p)}(t) = R^{\text{net}}_p(t) \cdot \frac{c_i^{(p)}(t)}{Z_p(t)}.
   $$
   (If $R^{\text{net}}_p(t) < 0$, nodes currently inside their juvenile grace period $\tau_i(t) > 0$ have negative deductions clamped to $0.0$; for mature circuits with $\tau_i(t) = 0$, $\sum_{i=0}^{N-1} \Delta E_{\text{credit}, i}^{(p)}(t) = R^{\text{net}}_p(t)$ holds to machine precision.)

3. **Synchronous register trace latch (end of tick $t$):**
   For each `Delay` register $r$ latched from input wire `in_wire(r)` (using target $T = \text{DelayLatch}(r)$ so dead detours downstream of `r` cannot pollute `r`, and capped at $\le 1.0$ per entry):
   $$
   \mathbf{e}_r(t + 1) = \mathbf{c}_{\text{in-wire}(r)}^{(\text{DelayLatch}(r))}(t).
   $$

**Properties verified in M1a:**

- **Interior chain equity:** On a 5-register shift chain `BoundaryIn -> d0 -> d1 -> d2 -> d3 -> d4 -> BoundaryOut` at $\gamma = 0.90$, shares are `(16.0%, 17.8%, 19.8%, 22.0%, 24.4%)`—eliminating upstream bucket-brigade starvation.
- **Exact Macro-Micro Conservation Law:** On every mature tick ($\tau_i = 0$, $E_{\max} = \text{None}$),
  $$
  \sum_{i=0}^{N-1} \Delta E_i(t) = \sum_{p : Z_p(t) > 0} R^{\text{net}}_p(t) - \sum_{i=0}^{N-1} c_{\text{maint}}(i).
  $$
  Over the full evaluation horizon $T$:
  $$
  \Delta E(T) = \sum_{t=0}^{T-1} R^{\text{net}}(t) - L(C) = -\Delta L_{\text{null}}(T).
  $$
  Connecting microscopic local survival directly to macroscopic description-length improvement, the exact amended G1A.4 identity holds on every run and horizon:
  $$
  \lvert \Delta E(T) + \Delta L_{\text{null}}(T) \rvert \le 10^{-9} \cdot \max(1, \lvert \Delta L_{\text{null}}(T) \rvert).
  $$

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

### 3.6.1 Pre-Registered M1a Panel Protocol

#### 1.1 Run protocol (frozen)

| Parameter | Value |
| :--- | :--- |
| Horizon per run | `T = 1024` ticks (amended investigation also validates `T = 2048` and `T = 4096`); energy and ΔL are measured over all ticks |
| Accuracy self-check window | ticks `16..1024` (transient excluded) |
| Widths | `k ∈ {1, 2}` |
| Noise | `ε ∈ {0.0, 0.05}` (`IndependentSubstitution` on channel 0) |
| Seeds | `ε = 0.0`: one run per case (fully deterministic at `k ≤ 4`). `ε = 0.05`: seeds `seed_offset .. seed_offset + seeds` |
| Economy | `EconomyConfig { juvenile_ticks: 0, max_energy_reserve: None, reward_mode: RewardMode::CalibratedPointForecast, ..Default::default() }` |
| Wiring | abstract `Circuit`, `WiringTopology::WellMixed`, `WireDelayMode::Combinational`; one spatial case in G1A.2/G1A.3 (Step 6) |
| Maintenance | Description debt $\Delta L_i$ via [`circuit_node_description_debts`](file:///workspaces/arkhe/src/metrics/mdl.rs) charged through the maintenance channel once on first mature tick; $\sum_i \Delta L_i = L(C)$ exactly; zero per-tick drain |
| Sensitivity | `SeedBank::new(seed)`, `tick_block = tick`, `sample_count = 8` (unused at `k ≤ 4`) |
| Ports | every panel circuit has exactly 1 input (possibly unused) and 1 output |

#### 1.2 Motif notation

`x` = `BoundaryIn(0)` = observed `s(t-1)`. `Out` = `BoundaryOut(0)`. All `Delay` nodes are `Primitive::Delay { init }`.

| Motif | Construction | Nodes | Emits |
| :--- | :--- | :---: | :--- |
| `ConstLoop(c)` | `D = Delay(c)`, `S = Split`; `D.out → S.in`, `S.out0 → D.in`, `S.out1 →` consumer | 2 | `c` every tick |
| `Osc2(a, b)` | `D1 = Delay(b)`, `D2 = Delay(a)`, `S = Split`; `D1 → D2 → S.in`, `S.out0 → D1.in`, `S.out1 →` consumer | 3 | `a, b, a, b, …` |
| `Osc3(a, b, a)` | `D1 = Delay(a)`, `D2 = Delay(b)`, `D3 = Delay(a)`, `S`; `D1 → D2 → D3 → S`, `S.out0 → D1`, `S.out1 →` consumer | 4 | `a, b, a, a, b, a, …` (period 3) |
| `Loop4(a, b)` | `D1..D4` with inits `(b, a, b, a)`, chain `D1 → D2 → D3 → D4 → S`, `S.out0 → D1`, `S.out1 →` consumer | 5 | `a, b, a, b, …` (bloated period-2) |
| `Shift(n)` | `x → D(0) → … → D(0) → Out` (`n` delays) | n | `s(t-1-n)` |
| `AddConst1` | `Add(x, ConstLoop(1)) → Out` | 3 | `s(t-1) + 1` |
| `Counter3Opt` | see 1.3 | 8 | `(x + 1) + Eqz(x + 2)` |

If the self-check in Step 5.3 shows a phase is off by one, fix the `Delay` inits. Do not touch the economy.

#### 1.3 `Counter3Opt` wiring (`k = 2`, exact for `CounterModN(3)`)

`Dc = Delay(1)`, `Sc = Split`, `Sc2 = Split`, `A1 = Add`, `Sy = Split`, `A2 = Add`, `E = Eqz`, `AF = Add`.

`Dc → Sc.in`; `Sc.out0 → Dc.in`; `Sc.out1 → Sc2.in`; `x → A1.0`; `Sc2.out0 → A1.1`; `A1 → Sy.in`; `Sy.out0 → AF.0`; `Sy.out1 → A2.0`; `Sc2.out1 → A2.1`; `A2 → E`; `E → AF.1`; `AF → Out`.

#### 1.4 Panel (tasks × classes)

Tasks (`hi = 2^k − 1`):

| Task id | Generator | Widths |
| :--- | :--- | :--- |
| `alt` | `T1_Alternating { a: 0, b: hi }` | 1, 2 |
| `motif3` | `T1_PeriodicMotif { motif: [0,1,1] }` (k=1) / `[0,1,3]` (k=2) | 1, 2 |
| `sparse6` | `T1_PeriodicMotif { motif: [0,0,0,0,0,1] }` | 1, 2 |
| `affine` | `T1_AffineLag1 { mul_a: 1, add_b: 1, init: 0 }` | 1, 2 |
| `counter3` | `T3b_CounterModN { modulus: 3, init: 0 }` | 2 |
| `const0` | `T1_Constant { value: 0 }` | 1, 2 |
| `consthi` | `T1_Constant { value: hi }` | 1, 2 |

Circuits per task. `Base` is the **first** listed `C_opt` of that task. `Bosc = Osc2(1, 0)`.

| Class | Circuit id | Tasks | Construction | Self-check at ε=0 |
| :--- | :--- | :--- | :--- | :--- |
| `C_opt` | `osc2` | alt | `Osc2(0, hi) → Out` | acc = 1.0 |
| `C_opt` | `eqz` | alt (k=1) | `Eqz(x) → Out` | acc = 1.0 |
| `C_opt` | `shift2` | motif3 | `Shift(2)` | acc = 1.0 |
| `C_opt` | `shift5` | sparse6 | `Shift(5)` | acc = 1.0 |
| `C_opt` | `addc1` | affine | `AddConst1` | acc = 1.0 |
| `C_opt` | `counter3` | counter3 | `Counter3Opt` | acc = 1.0 |
| `C_partial` | `eqz_lag2` | motif3 (k=1) | `x → Delay(0) → Eqz → Out` | 0.66 ≤ acc ≤ 0.67 |
| `C_partial` | `inc1` | counter3 | `AddConst1` | 0.66 ≤ acc ≤ 0.67 |
| `C_chance` | `const0`, `const1` | every non-constant task | `ConstLoop(0\|1) → Out` | none (report only) |
| `C_chance` | `osc3` | alt | `Osc3(0, hi, 0) → Out` | none |
| `C_wrong` | `osc2_anti` | alt | `Osc2(hi, 0) → Out` | acc = 0.0 |
| `C_wrong` | `shift1` | motif3 (k=2) | `Shift(1)` | acc = 0.0 |
| `C_silence` | `const_match` | const0, consthi | `ConstLoop(value) → Out` | acc = 1.0 |
| `C_detour` | `addzero` | every non-constant task | `Add(Base, ConstLoop(0)) → Out`; detour set = ConstLoop nodes | acc = 1.0 |
| `C_detour` | `mulzero` | every non-constant task | `Add(Base, Mul(Bosc, ConstLoop(0))) → Out`; detour set = `Mul` + ConstLoop + `Bosc` nodes | acc = 1.0 |
| `C_detour` | `cancelsub` | every non-constant task | `Sb = Split(Bosc)`; `Sub(Add(Base, Sb.out0), Sb.out1) → Out`; detour set = `Bosc` nodes + `Sb` | acc = 1.0 |
| `C_detour` | `serial_not` | alt (k=1) | `Eqz(Eqz(Eqz(x))) → Out`; compared against `eqz` | acc = 1.0 |
| `C_detour` | `loop4` | alt | `Loop4(0, hi) → Out`; compared against `osc2` | acc = 1.0 |
| `C_detour` | `onefire` | every non-constant task | `Add(Base, Delay{init:1}(ConstLoop(0))) → Out`; detour set = `{Delay, ConstLoop}` nodes | acc = 1.0 |

Each detour's **reference** is the `C_opt` it wraps (`Base`). For `serial_not` the reference is `eqz`; for `loop4` it is `osc2`; for `onefire` it is `Base`.

> [!NOTE]
> `C_partial` cases are **report-only** for G1A.2: the plan does not pre-register an energy condition for them. They do take part in G1A.4 ranking.

### 3.6.2 M1a Development-Seed and Bank Verification Results

The closed-loop metabolic economy under the M1a redesign (decisions D1–D4: replacing ancestry traces, calibrated point-forecast reward, description debt, and activity recency decay) was evaluated across 100 consecutive 20-seed development banks (seeds 20–2019) across horizons $T \in \{1024, 2048, 4096\}$.

- **Development Bank Outcomes (100 banks of 20 seeds each):**
  - **$T = 1024$ ticks:** `G1A.1`: 100/100 passed; `G1A.2`: 100/100 passed; `G1A.3`: 100/100 passed; `G1A.4`: 100/100 passed (100/100 banks passed all gates simultaneously).
  - **$T = 2048$ ticks:** `G1A.1`: 100/100 passed; `G1A.2`: 100/100 passed; `G1A.3`: 100/100 passed; `G1A.4`: 100/100 passed (100/100 banks passed all gates simultaneously).
  - **$T = 4096$ ticks:** `G1A.1`: 100/100 passed; `G1A.2`: 100/100 passed; `G1A.3`: 100/100 passed; `G1A.4`: 100/100 passed (100/100 banks passed all gates simultaneously).
- **Core Verification Invariants:**
  - `G1A.1` (Conservation & Substrate Purity): Exact macro-micro energy conservation holds to $< 10^{-12}$ on 100% of mature ticks. Zero float operations in `src/substrate/`. Sampled $k=8$ sensitivity matches exact within $\pm 0.05$.
  - `G1A.2` (Correct vs. Incorrect Separation): 100% of `C_opt` circuits on non-constant tasks achieve $\Delta E(T) > 0$ and every node repays its description debt ($\min_i E_i(T) > E_{\text{init}}$) across all noise levels and horizons. 100% of `C_wrong`, `C_chance`, and `C_silence` circuits end with $\Delta E(T) < 0$. Constant circuits score $r \equiv 0$ on every tick and end at $\Delta E(T) = -L(C) < 0$.
  - `G1A.3` (Detour & Free-Rider Penalty): Zero credit leaks into cancelling or identity detours after tick 0 ($\Delta E_{\text{detour}} = -\Delta L_{\text{detour}}$). On the `onefire` detour, activity decay with $\gamma = 0.90$ bounds single-fire leaks; every detour node ends strictly below $E_{\text{init}}$, and post-tick-0 leak credit is identical between $T=1024$ and $T=4096$ within $10^{-9}$ bits. 100% of detours satisfy $\Delta E(\text{detour}) < \Delta E(\text{reference})$.
  - `G1A.4` ($\Delta E$ tracks $-\Delta L_{\text{null}}$): The exact identity $\lvert \Delta E(T) + \Delta L_{\text{null}}(T) \rvert \le 10^{-9} \cdot \max(1, \lvert \Delta L_{\text{null}}(T) \rvert)$ holds on 100% of runs and horizons, with zero sign inversions.

---

## 4. Stage M1b Specification: First-to-Second Neuron Residual Neurogenesis

### 4.0 Interface Decisions and Open Questions from the M1a Redesign

Stage M1a fixed the metabolic economy on static circuits by replacing per-tick maintenance rent with one-time description debt $L(C) = \sum_i \Delta L_i$ charged upon maturity, introducing calibrated sequential point forecasts ($q = \max(\hat{q}, p)$, $r = \log_2(q/p)$), replacing ancestry traces (clamped to $\le 1.0$), and activity recency decay ($\gamma = 0.90$).

Because M1b introduces online graph surgery (`EditableLattice`), sprouting, and starvation pruning, the following interface decisions arising from the debt economy are resolved as `D1b.1`–`D1b.7` in [milestone_1b_plan.md](milestone_1b_plan.md#2-resolved-post-m1a-economy-and-debt-interfaces-d1b1d1b7):

1. **Idle nodes vs. starved nodes:** Under description debt, a node that has fully repaid its share $\Delta L_i$ and subsequently stops receiving credit maintains positive energy indefinitely ($E_i(t) > 0$). Under old per-tick rent, such idle nodes passively starved. For M1b:
   - *Structural pruning test:* A node is eligible for pruning if removing it does not increase global description length $L$ (aligning with Charter §3.2 sleep compression and Milestone 3).
   - *Switching distribution:* Alternatively, switching codes (e.g., Herbster & Warmuth, 1998; van Erven et al., 2012) can re-charge the description cost of structure that ceases predictive contribution, using fixed switching priors rather than tuned per-tick rent.
2. **Juvenile grace period and sprout debt:** With debt charged once upon node maturity, a newly sprouted candidate node $i$ begins with endowment $E_{\text{init}}$ (default $4.0\text{ bits}$) and juvenile grace counter $\tau_i(0) = T_{\text{juvenile}}$ (default $128\text{ ticks}$). Upon maturity ($\tau_i(t) = 0$), node $i$ is charged its description debt $\Delta L_i$. It survives if and only if accumulated juvenile earnings plus $E_{\text{init}}$ exceed $\Delta L_i$ ($E_i(t) > 0$).
3. **Edits change existing nodes' debt shares:** Adding or removing a node changes the Elias-$\gamma$ circuit header bits and wire encoding lengths for other nodes. M1b enforces conservation $\sum_i \text{debt}_i = L(C)$ by having each sprout transaction pay the marginal change in total description length:
   $$
   \Delta L_{\text{sprout}} = L(C \cup \{\text{sprout}\}) - L(C).
   $$
4. **Port match counters ($M$) across upstream edits:** The prequential point forecast uses $M$, the cumulative count of previous exact matches. When an upstream graph edit alters a port's driver or internal cone, decide whether $M$ is preserved (treating the learner's prequential sequence as continuous) or reset (re-starting calibration for the modified predictor).
5. **Homeostatic reserve cap ($E_{\max}$):** The M1a proportionality audit used `max_energy_reserve: None`. In M1b online runs, `max_energy_reserve: Some(25.0)` prevents long-lived first neurons from accumulating unbounded reserves that buffer against obsolete structure pruning after generator regime shifts.
6. **Alternative null models:** The calibrated reward $r = \log_2(q/p)$ was calibrated with `LaplaceFrequency` ($KT$ estimator). If M1b evaluates `NullModelKind::RepeatLast`, a matched prequential estimator is derived so constant predictors still score identically $0.0$ on every tick.
7. **Unowned circuit bits:** Growth can theoretically create direct `BoundaryIn(p) -> BoundaryOut(q)` connections with no intermediate nodes. Direct wire bits and minimal empty-graph headers are attributed to a circuit-level pool so $\sum_i \Delta L_i + L_{\text{unowned}} = L(C)$ remains exact.

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

### 4.2 Spatial Anchoring, Direct Graph Wiring, and Local Lattice Routing (`lattice.rs`)

> [!NOTE]
> **Architectural Decision (M1b) — Removal of Planar Relay Congestion:**
> Multi-neuron growth under 2D planar lattice nearest-neighbor routing (`LocalVonNeumann` with single-cell `Split` relays) rapidly choked border cells around boundary anchors, causing routing deadlock during multi-neuron neurogenesis. The physical 2D spatial grid metaphor was an accidental constraint. M1b structural growth defaults to direct graph wiring (`WiringTopology::WellMixed`), allowing any number of edges directly between endpoints without planar relay blockage. Description length ($L(C)$) naturally prices non-local connections, and `LocalVonNeumann` / `LocalMoore` are preserved as controlled ablations.

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

- `arkhe verify-m1a [--seed-offset <u32>] [--seeds <u32>] [--banks <u32>]` — executes Stage M1a Gates `G1A.1`–`G1A.4` at $k = 1$ and $k = 2$ (single bank or `N` parallel banks) and emits structured JSON.
- `arkhe verify-m1b [--seed-offset <u32>] [--seeds <u32>] [--banks <u32>]` — executes Stage M1b Gates `G1B.1`–`G1B.4` (including the §8 M1 ablation matrix) at $k = 1$ and $k = 2$ (single bank or `N` parallel banks).
- `arkhe verify-m1 [--seed-offset <u32>] [--seeds <u32>] [--banks <u32>]` — runs `verify-m1a` first (**aborting immediately if any M1a gate fails**, enforcing Charter §7's pre-growth kill gate), then runs `verify-m1b` and subprocess determinism checks.

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
trace_discount = 0.90
initial_energy = 4.0
juvenile_ticks = 128
max_energy_reserve = 25.0
null_model = "LaplaceFrequency"
reward_mode = "CalibratedPointForecast"

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
| **`G1A.2` — Correct vs. incorrect separation** | Correct circuits gain energy & sustain all interior nodes above debt; wrong, chance, and silent-stream circuits lose energy | Across 20 seeds ($\varepsilon \in \lbrace 0.0, 0.05 \rbrace$) at $k = 1$ and $k = 2$: **100%** of `C_opt` circuits on non-constant tasks satisfy $\Delta E(T) > 0$ and $\min_i E_i(T) > E_{\text{init}}$ (every node earns more than its description debt $\Delta L_i$); **100%** of `C_wrong`, `C_chance` (including lazy always-`0` on sparse `5/6` motif), and `C_silence` (`T1_Constant`) satisfy $\Delta E(T) < 0$ | **Charter M1a Kill Criterion:** Any task/width fails separation $\implies$ **halt before M1b** and redesign reward/credit |
| **`G1A.3` — Detour & free-rider penalty** | Detours receive zero positive credit or strictly bounded single-fire leak; all detours lower circuit $\Delta E$ | Detours that never fire receive `0.0` cumulative credit after tick 0 ($\Delta E_{\text{detour}} = -\Delta L_{\text{detour}}$); `onefire` detour nodes each end with $E_i(T) < E_{\text{init}}$ across all seeds and horizons; all detours satisfy $\Delta E(\text{detour}) < \Delta E(\text{reference})$ on 100% of runs | Credit leaks allow detours to repay description debt: redesign credit flow / activity decay |
| **`G1A.4` — $\Delta E$ tracks $-\Delta L_{\text{null}}$** | Local metabolic balance tracks global description-length improvement (§3.4) | On every run across the M1a circuit panel, $\lvert \Delta E(T) + \Delta L_{\text{null}}(T) \rvert \le 10^{-9} \cdot \max(1, \lvert \Delta L_{\text{null}}(T) \rvert)$, with zero sign inversions | **Implementation Bug:** Any violation of the exact identity signals an energy conservation or debt accounting error |

### 6.2 Stage M1b Gates (First to Second Neuron — Hypothesis H2)

| Gate | Requirement | Exact Quantitative Pass Criterion ($k \in \lbrace 1, 2 \rbrace$) | Kill / Revisit Criterion |
| :--- | :--- | :--- | :--- |
| **`G1B.1` — Stepwise first-to-second neuron reduction** | Second (and stacked) corrector reduces error as residual entropy/MI predicts, without backprop | On `T1_PeriodicMotif([0, 1, 1])` ($k = 1$), `T1_PeriodicMotif([0, 1, 3])` ($k = 2$), `T1_AffineLag1` ($k = 2$), and `T3b_CounterModN(3)` ($k = 2$): post-first-neuron loss $L_1 < L_0$, post-second-neuron loss $L_2 < L_1$, and empirical error drop matches diagnostic $\hat{I}(R_1 ; U^*)$ within $0.05\text{ bits/symbol}$ on $\ge 19/20$ seeds | Correctors fail to stack or overwrite $y_1$'s survival: revisit additive boosting / identity gating |
| **`G1B.2` — T1 suite convergence & noise-floor halting** | Consistent error reduction to $h_\mu(\varepsilon)$ across seeds on direct graph wiring (with local 2D lattice ablation) | Across 20 dev and 20 holdout seeds at $k = 1$ and $k = 2$ under direct graph wiring `WellMixed` (and `LocalVonNeumann` ablation): (a) **Noiseless ($\varepsilon = 0$):** `0` active steady-state nodes on `T1_Constant`; `0` post-transient mismatches on $\ge 18/20$ seeds across `T1_Alternating`, `T1_PeriodicMotif(p=3)`, and `T1_AffineLag1`; (b) **Noisy ($\varepsilon = 0.05$):** `paired_residual_regret_bits_per_symbol <= 0.025` on $\ge 18/20$ seeds for `T1_Constant` and `T1_Alternating`, strictly lower mean paired regret than a 1-node reactive echo on noisy `T1` tasks, and bounded final size `node_count <= max_nodes` (growth halts at noise floor) | **Charter M1b Kill Criterion:** No consistent error reduction across seeds on T1 $\implies$ revisit the §3.1.5 sprouting criterion |
| **`G1B.3` — Live invariant, phase, & budget integrity** | Every online sprout and starvation prune preserves §2.5 invariants, live register phases, and `TickBudget` | `0` `LatticeError` or `InvariantError` failures across all ticks and seeds; autonomous oscillator phase error $= 0$ across forced sprout+prune surgery tests; `tick_budget_consumed <= max_eval_ticks` everywhere | Any post-surgery compile error or phase reset: block sign-off |
| **`G1B.4` — Charter §8 M1 ablations & subprocess reproducibility** | Empirical verification of M1 ablations + separate-process determinism | Execute all four M1 ablations (Section 6.3) with expected directional effects verified; two separate OS child processes across a 6-config `ArkheM1` matrix ($k \in \lbrace 1, 2 \rbrace$, `LocalVonNeumann` & `WellMixed`, $\varepsilon \in \lbrace 0.0, 0.05 \rbrace$, `Combinational` & `TicksPerHop`) emit byte-identical `RunReport` JSON | Nondeterministic output across processes: fix ordering/seeding immediately |

### 6.3 Charter §8 Milestone 1 Ablation Matrix

| Ablation Axis | Default Setting | Ablated Setting | Measured Hypothesis Check |
| :--- | :--- | :--- | :--- |
| **1. Reward weighting** | `RewardMode::CalibratedPointForecast` | `CenteredSurprisal`, `UncenteredSurprisal`, & `RawAccuracy` | `RawAccuracy` and `UncenteredSurprisal` sustain useless nodes on `T1_Constant(0)` and lazy always-`0` circuits on sparse `[0, 0, 0, 0, 0, 1]`, whereas `CalibratedPointForecast` scores blind/constant predictors at `0.0`, starves both, and sustains the rare-event predictor |
| **2. `SPLIT` fan-out tax** | `split_code_tax = true` (full `Split` node + dual-wire MDL debt) | `split_code_tax = false` (`0` `Split` operator & extra-wire debt bits) | `split_code_tax = true` yields lower or equal mean `split_count` and lower unweighted `circuit_bits` at equal post-transient accuracy |
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

### WP-M1.1: Metabolic Reserves, Description Debt, and Calibrated Point-Forecast Reward (`src/structural/economy.rs`, `src/metrics/mdl.rs`, `src/metrics/point_forecast.rs`)

- **Goal & Scope:** Implement node energy state, one-time description debt allocation (`circuit_node_description_debts`), juvenile grace accounting, calibrated sequential point-forecast reward (`RewardMode::CalibratedPointForecast`), and null-calibrated MDL $\Delta L_{\text{null}}$.
- **Tasks:**
  1. Define `EconomyConfig`, `RewardMode`, `NodeEconomy`, and `EconomyState` in `src/structural/economy.rs`.
  2. Implement `PointForecastScorer` in `src/metrics/point_forecast.rs` backing `RewardMode::CalibratedPointForecast` and `null_calibrated_mdl_delta` in `src/metrics/mdl.rs`.
  3. Implement `circuit_node_description_debts` in `src/metrics/mdl.rs` charging one-time description debt $L(C) = \sum_i \Delta L_i$ on first mature tick.
- **Verification:** Unit tests prove constant predictors score $r \equiv 0.0$ on every tick and seed, sequential log-loss savings guarantee MDL separation on all dev banks, and $\sum_i \Delta L_i = L(C)$ holds exactly across all topologies.

---

### WP-M1.2: Counterfactual DAG Sensitivity, Replacing Traces, and Activity Recency Decay (`src/structural/sensitivity.rs`, `src/structural/economy.rs`)

- **Goal & Scope:** Implement whole-node counterfactual DAG perturbation, replacing eligibility traces (ancestry cap at $\le 1.0$, D1), and continuous input activity recency decay ($\gamma = 0.90$, D4).
- **Tasks:**
  1. Expose read-only topological inspection helpers on `CompiledCircuit` (in `src/substrate/eval.rs`, keeping `substrate` zero-float) so `src/structural/sensitivity.rs` evaluates localized counterfactual cones for $1 \le k \le 64$.
  2. Implement exact $(2^k - 1)$-value enumeration for $k \le 4$ and `SensitivityProbe`-seeded sampling for $k > 4$.
  3. Implement D1 replacing traces (clamped at $\le 1.0$) in `ancestry_for_target`, stopping recurrent loops from accumulating $1/(1-\gamma)$ feedback gain.
  4. Implement D4 continuous activity recency decay with $\gamma = 0.90$ in `NodeActivity` and `ring_identity_input_weights`.
- **Verification:** Unit tests confirm machine-precision conservation ($< 10^{-12}$), equitable shares across a 5-`Delay` chain, loop reachability bounded by $1.0$, and single-fire detour leak credit strictly bounded across horizons.

---

### WP-M1.3: Stage M1a Audit Suite, Bank Harness, and Pre-Growth Gate (`src/verify_m1/`, `tests/m1a_economy.rs`)

- **Goal & Scope:** Construct the complete hand-built M1a 6-class circuit panel (including `onefire` detour) across $k = 1$ and $k = 2$, wire `arkhe verify-m1a` with parallel bank evaluation, and validate **H1** across multiple horizons.
- **Tasks:**
  1. Build canonical spatial and abstract circuits for `{C_opt, C_partial, C_chance, C_wrong, C_silence, C_detour}` (including `onefire`) across `T1` and `T3b_CounterModN(3)`.
  2. Implement automated checks for Gates `G1A.1`–`G1A.4` (including exact identity $\lvert \Delta E(T) + \Delta L_{\text{null}}(T) \rvert \le 10^{-9} \cdot \max(1, \lvert \Delta L_{\text{null}}(T) \rvert)$) and parallel bank mode in `arkhe verify-m1a`.
  3. Execute `verify-m1a` on 100 development banks (`20..2020`); verify all 100 banks pass across $T \in \{1024, 2048, 4096\}$ before starting `WP-M1.4`.
- **Verification:** `cargo test --test m1a_economy` and `cargo run --locked --release -- verify-m1a --banks 100 --seed-offset 20 --seeds 20` pass 100/100 banks on all gates.

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
| **Blind or chance circuits surviving on lucky random walk** | Centered surprisal variance scales cumulative luck as $O(\sqrt{T})$, requiring fragile horizon-fitted rent rates | Calibrated point forecast (`RewardMode::CalibratedPointForecast`) scores constant circuits identically $0.0$, and bounds non-predictive flux by Ville's martingale inequality |
| **Recurrent loop credit inflation starving feedforward correctors** | Loops like `Delay(1) <-> Split` add their own ancestry repeatedly, reaching $1/(1-\gamma) = 10$ and starving combinational `Add` nodes | Replacing ancestry traces (D1) clamp every node's ancestry weight to $\le 1.0$, preventing self-voting and ensuring equitable credit distribution |
| **Single-fire detour leaking credit indefinitely** | A detour firing once at $t=0$ and remaining 0 thereafter leaks credit whenever both inputs to `Add` are zero | Activity recency decay with $\gamma = 0.90$ (D4) continuously decays identity input weights, bounding total leak across arbitrary horizons |
| **Horizon conflict between per-tick rent and one-time MDL code length** | Per-tick maintenance accumulates as $T \cdot c_{\text{maint}}$, diverging from horizon-invariant $L(C)$ at long horizons | Description debt (D3) charges each node's exact share $\Delta L_i$ once upon maturity through the maintenance channel, guaranteeing $\Delta E(T) \equiv -\Delta L_{\text{null}}(T)$ |
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
cargo run --locked --release -- verify-m1 --seed-offset 10000 --seeds 20 --holdout

# 4. Crate packaging verification
cargo package --allow-dirty

# 5. Documentation lint conformance
files=$( { git diff HEAD --name-only --diff-filter=AM -- '*.md'; git ls-files -o --exclude-standard -- '*.md'; } | sort -u )
[ -n "$files" ] && printf '%s\n' "$files" | xargs -d '\n' npx markdownlint-cli2 --fix
npx markdownlint-cli2 "doc/**/*.md" README.md
```

---

## 10. Milestone 1 Attainment Sign-Off

### 10.1 Single-Shot Holdout Verification Record

In accordance with Charter §7 and §6 of this plan, the Milestone 1 holdout sign-off was executed as a single-shot command on git commit `e29014a5c4e410f90797db079d457740c5c2d4be`:

```bash
cargo run --locked --release -- verify-m1 --seed-offset 10000 --seeds 20 --holdout
```

- **Execution Timestamp:** 2026-11-07
- **Git Commit SHA:** `e29014a5c4e410f90797db079d457740c5c2d4be`
- **Git Dirty:** `false`
- **Source Tree Digest (`source_tree_fnv1a128`):** `564de6fbdb0b8cbd898b91fc6b4a408a`
- **Cargo Lock Digest (`cargo_lock_fnv1a128`):** `8d29cd24b1fbdbb743cf5c0b3933b22d`
- **Rustc Version:** `rustc 1.99.1 (58cf8f9c3 2026-10-13)`
- **Target Triple:** `x86_64-unknown-linux-gnu`
- **Holdout Seed Range:** `10000..=10019` (20 seeds)
- **Status:** **ALL GATES PASSED** (`all_passed: true`, `stage_gate_aborted: false`)

### 10.2 Sign-Off Summary Across Stages

| Stage / Gate | Focus | Tested Cases | Result |
| :--- | :--- | :--- | :--- |
| **Stage M1a Stage Gate** | Fixed-Structure Economy | **6,245 panel runs** | **PASSED (0 failures)** |
| `G1A.1` | Exact macro-micro conservation & zero-float substrate | 1,959 | Passed |
| `G1A.2` | Correct vs. incorrect energy separation | 1,069 | Passed |
| `G1A.3` | Detour & free-rider penalty (zero leak) | 1,579 | Passed |
| `G1A.4` | Local $\Delta E$ tracks global $-\Delta L_{\text{null}}$ | 1,638 | Passed |
| **Stage M1b** | Online Neurogenesis & Ablations | **294 gate runs** | **PASSED (0 failures)** |
| `G1B.1` | Stepwise first-to-second neuron reduction | 80 | Passed |
| `G1B.2` | T1 suite convergence & noise-floor halting | 200 | Passed |
| `G1B.3` | Live invariant, phase, & budget integrity (500 edits) | 4 | Passed |
| `G1B.4` | Charter §8 4-axis ablations & subprocess determinism | 10 | Passed |

### 10.3 Post-Review Hardening & 100-Bank (`2,000`-Seed) Verification Addendum (`2026-11-08`)

Following the closeout review in [milestone_1b_review.md](milestone_1b_review.md), pre-M2 hardening expanded `verify-m1b` to `2,530` gate checks per 20-seed bank (covering noisy non-zero constants, multi-stage `TicksPerHop` tasks, online sparse rare-event rings, multi-seed fan-out/locality/delay ablations, cross-run budget/invariant audits, and `320`-tick separate-process determinism) while keeping all pre-registered metabolic and growth constants unchanged and leaving holdout seeds `10000..=10019` untouched. Parallel 100-bank verification (`verify-m1 --banks 100 --seed-offset 20 --seeds 20`, `source_tree_fnv1a128 = 10806db8e8a213e338d563a904cd36df`) passed **100 / 100 banks** across both Stage M1a (`624,500` cases, `0` failures) and Stage M1b (`253,000` cases, `0` failures).
