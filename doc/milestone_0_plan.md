# Milestone 0 Implementation Plan: Simulator and Yardstick

> **Status:** Approved implementation plan for Stage **M0** of [foundational_research_charter_v2.md](foundational_research_charter_v2.md).
>
> **Charter scope:** §2 (Exact $\mathbb{Z}_{2^k}$ substrate & tick invariants), §3.1.1 (2D spatial lattice topology), §3.1.4 & §3.4 (Surprisal null models & MDL $L$), §5 (Generators, noise, $\varepsilon$-machine & Berlekamp–Massey/Reeds–Sloane ground truth), §7 (M0 success criterion), §8 (Experimental standards & baselines), and §9 (Rust implementation guidance).

---

## 1. Purpose, Architectural Decisions, and Non-Goals

Milestone 0 builds the **deterministic simulator, exact ground-truth yardsticks, and equal-compute baseline harness** upon which every later milestone depends. No metabolic energy, retrograde credit assignment, sprouting, pruning, or sleep consolidation runs in M0; instead, M0 ensures that before any learning hypothesis (H1–H10) is tested, circuit evaluation, task difficulty ($C_\mu$, $h_\mu$, minimal recurrence length, minimal circuit size), description length $L$, and baseline performance are measured exactly and reproducibly at both Boolean ($k = 1$) and graded ($k \ge 2$) widths.

### 1.1 Resolved Design Decisions

| Topic | Decision for M0 | Rationale |
| :--- | :--- | :--- |
| **Crate, toolchain & dependency versions** | Rename package, library crate, and CLI binary from starter template names (`rust-base` / `rust_base` / `protoproject`) to `arkhe`; bump `rust-version` from `"1.85"` to `"1.99.1"` (latest stable Rust) and target the latest stable releases of all M0 crates | Aligns Cargo metadata, MSRV, and dependency APIs (`rand` / `rand_chacha` `0.10`, `toml` `1.1`, `clap` `4.6`) with current stable releases from day one |
| **Spatial lattice boundary** | Implement the abstract synchronous circuit graph in `substrate` **plus** 2D grid coordinates, perimeter ports, local vs. well-mixed wiring rules, and hop-delay lowering in `structural::lattice` (no economy or growth) | Enforces Charter §9 layer separation while supporting §2.5 wire-delay modes, §3.1.1 geometry, and M1a hand-built spatial circuits immediately |
| **Width & evaluation engines** | Uniform runtime-configured width $1 \le k \le 64$ per run (Phase W1) backed by `u64` scalar evaluation, plus a 64-lane `u64` bit-parallel evaluator at $k = 1$; defer $k > 1$ SIMD lane packing to M4a/M5 | Covers mandatory $k = 1$ and $k = 2$ runs (§7) up to 64-bit rings cleanly; accelerates $k = 1$ exhaustive synthesis and basin probes now without premature SIMD complexity |
| **Environment tiers & ground truth** | Include **T1** (local temporal), **T3b** (modular counters $\bmod 2^j$ and $\bmod n$), and **T3c** (LFSRs & sub-tap curricula), an exact finite-state $\varepsilon$-machine solver, and **Reeds–Sloane** shift-register synthesis over $\mathbb{Z}_{2^k}$ | Over $\mathbb{Z}_{2^k}$ for $k > 1$, zero divisors break classical $GF(2)$ Berlekamp–Massey; Reeds–Sloane reduces identically to Berlekamp–Massey at $k = 1$ and generalises exactly to $k \ge 2$ |
| **Baseline harness scope** | Implement the five discrete/symbolic baselines in M0 (`RandomSearch`, `(1+λ) CGP`, `CTW`, `ReedsSloane`, `ExhaustiveEnumeration`); defer the `RNN/LSTM` BPTT baseline to M1b/M2 | Keeps M0 pure Rust with zero ML/autodiff crate bloat while preserving a probabilistic `PredictorBaseline` interface into which `RNN/LSTM` drops cleanly later |

### 1.2 Explicit Non-Goals (Deferred to M1a–M6)

- **M1a:** Node energy reserves $E_i(t)$, maintenance costs ($c_{\text{base}}$, $c_{\text{split}}$), retrograde sensitivity-weighted credit flow, eligibility traces, and juvenile subsidies.
- **M1b–M3:** Residual-driven corrector sprouting, redundancy sprouting, internal comparator pruning, and online structural mutation during wake.
- **M1b/M2:** Small `RNN` / `LSTM` backpropagation-through-time baseline and benchmark tiers **T2** (cross-channel coupling) and **T3a** (spatially presented cellular automata).
- **M4a–M4c:** Sleep loopback controller (arms S-A and S-B), `egg` equality saturation, SMT bit-vector equivalence checking, derived axiom promotion, and **T4** task families.
- **M5–M6:** Heterogeneous widths (`TRUNC`, `EXT` in Phase W2), $k > 1$ SIMD multi-lane evaluation, and active environment interventions.

---

## 2. Package Transition, Dependencies, and Module Layout

### 2.1 Package & Toolchain Transition to `arkhe`

1. Update `Cargo.toml`:
   - `[package] name = "arkhe"`, `edition = "2024"`, `rust-version = "1.99.1"` (replacing the starter template's `"1.85"` with the current latest stable Rust release; `rust-toolchain.toml` stays on `channel = "stable"`).
   - `[lib] name = "arkhe"`, `path = "src/lib.rs"`.
   - `[[bin]] name = "arkhe"`, `path = "src/main.rs"`.
2. Update `.github/copilot-instructions.md` directly (preserving the symbolic link `GEMINI.md -> .github/copilot-instructions.md`) so the integration-test guideline references `arkhe` instead of `rust_base`.
3. Update `README.md` and `tests/` to import `arkhe`.

### 2.2 Crate Dependencies (Latest Stable Releases)

Following the repository dependency rules, M0 uses the latest stable releases of well-established, deterministic crates needed for portable seeding, serialisation, and CLI parsing:

```toml
[dependencies]
clap = { version = "4.6.7", features = ["derive"] }
rand = { version = "0.10.3", default-features = false, features = ["std"] }
rand_chacha = "0.10.0"
rand_core = "0.10.1"
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1.0.154"
toml = "1.1.8"
```

- **Compile-time RNG hygiene:** Importing `rand` with `default-features = false, features = ["std"]` provides `rand::RngExt` (unbiased uniform range sampling and Bernoulli draws over `ChaCha8Rng` / `rand_core::Rng`) while omitting `sys_rng` and `thread_rng` (`getrandom`), making ambient OS randomness uncallable anywhere in the crate.
- **No floating-point arithmetic inside `substrate`** (§2.1). Floating-point (`f64`) is permitted strictly inside `metrics`, `env::epsilon_machine`, `env::noise`, and `baselines::ctw` for log-loss, entropy ($C_\mu$, $h_\mu$), surprisal ($-\log_2 p$), and Krichevsky–Trofimov probabilities.
- **No `HashMap` or `HashSet` with randomised hashing** in simulation, compilation, or solver paths; use dense index vectors (`Vec`) or ordered `BTreeMap` / `BTreeSet` everywhere to guarantee platform-invariant iteration order.

### 2.3 Target Repository & Module Hierarchy

```text
src/
├── lib.rs                      # Public crate exports and architecture overview
├── main.rs                     # CLI binary entry point (`arkhe`)
├── rng.rs                      # Domain-separated ChaCha RNG hierarchy
├── config.rs                   # ExperimentConfig, RunManifest, RunResult schemas
├── substrate/                  # Charter §2: Exact Z_{2^k} values, primitives, tick semantics
│   ├── mod.rs
│   ├── value.rs                # Width (1..=64), Val, wrapping ring ops, comparator, distance
│   ├── primitive.rs            # Add, Sub, Mul, Eqz, Delay, Split arities & semantics
│   ├── circuit.rs              # Port-accurate directed graph & WireDelayMode
│   ├── invariants.rs           # §2.5 structural checks & topological ordering
│   └── eval.rs                 # CompiledCircuit, ScalarState, BitParallel64State (k = 1)
├── structural/                 # Charter §3.1: Spatial geometry (economy/growth added in M1)
│   ├── mod.rs
│   └── lattice.rs              # 2D Grid, Coord, BoundaryPort, WiringTopology, lowering
├── env/                        # Charter §5: Finite-state generators, noise, ground truth
│   ├── mod.rs
│   ├── generator.rs            # FiniteStateGenerator trait & FiniteStateSpec export
│   ├── noise.rs                # Parameterised NoiseModel (channel substitution ε)
│   ├── tiers.rs                # T1 (rhythms), T3b (counters), T3c (LFSRs & curricula)
│   ├── epsilon_machine.rs      # Exact causal-state solver (C_mu, h_mu, L_sync)
│   └── reeds_sloane.rs         # Shift-register synthesis mod 2^k (GF(2) BM at k = 1)
├── metrics/                    # Charter §2.6, §3.1.4, §3.4, §8: Yardsticks
│   ├── mod.rs
│   ├── null_model.rs           # Repeat-last & Laplace/KT frequency null models, surprisal
│   ├── mdl.rs                  # Total description length L accounting
│   ├── dynamics.rs             # Autonomous limit-cycle, transient, and basin measurement
│   └── scorecard.rs            # Unified run metrics vs. ground-truth bounds
└── baselines/                  # Charter §8: Compute-matched prequential baselines
    ├── mod.rs                  # PredictorBaseline trait, TickBudget, HarnessRunner
    ├── random_search.rs        # Constructive random valid-circuit search
    ├── cgp.rs                  # (1 + λ) Cartesian Genetic Programming (Split-explicit)
    ├── ctw.rs                  # Bit-decomposed Context-Tree Weighting
    ├── bm_predictor.rs         # Online prequential Reeds–Sloane predictor
    └── exhaustive.rs           # Canonical small-circuit enumerator & exact synthesis oracle
```

---

## 3. Seeded Determinism and Reproducibility Contract (`rng.rs`, `config.rs`)

Charter §7, §8, and §9 require that every source of randomness is seeded through a portable, version-stable generator and that every run can be reproduced byte-for-byte from its seed and configuration file.

### 3.1 Domain-Separated RNG Derivation

A single global PRNG shared across environment stepping, channel noise, and baseline search creates fragile coupling: changing a baseline's candidate count would alter the noise stream observed on later ticks. M0 prevents this via deterministic child-stream derivation from a 64-bit `MasterSeed`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamDomain {
    GeneratorInit,
    ChannelNoise { channel: u16 },
    BaselineSearch { trial: u32 },
    BasinPerturbation { probe: u32 },
}

pub struct SeedBank {
    master_seed: u64,
}

impl SeedBank {
    #[must_use]
    pub const fn new(master_seed: u64) -> Self {
        Self { master_seed }
    }

    #[must_use]
    pub fn stream(&self, domain: StreamDomain) -> ChaCha8Rng;
}
```

- `SeedBank::stream` constructs a 32-byte seed `[u8; 32]` in little-endian byte order encoding `(master_seed, domain_tag, domain_index, protocol_version = 0)` and initialises `ChaCha8Rng::from_seed(...)`.
- **Golden-vector lock test:** `tests/determinism.rs` asserts exact hardcoded `next_u64()` outputs for `MasterSeed(0)` and `MasterSeed(0xA4_4B_48_45)` across every `StreamDomain`, guaranteeing that minor crate updates never alter trajectories silently.

### 3.2 Configuration & Run Manifests

- Every CLI run accepts an `ExperimentConfig` TOML file (Section 9) and emits a deterministic `RunReport` JSON document containing:
  - Canonical serialised `ExperimentConfig` and its deterministic FNV-1a / SHA-256 hex digest.
  - Ground-truth summary (`c_mu_bits`, `h_mu_bits_per_symbol`, `sync_order`, `reeds_sloane_min_len`, `exact_synthesis_min_nodes`).
  - Baseline `Scorecard` metrics and (for circuit-producing baselines) the serialised winning `Circuit`.

---

## 4. Substrate Specification (`src/substrate/`)

### 4.1 Variable-Width Modular Arithmetic (`value.rs`)

In Phase W1 (§2.4), every run operates at a uniform bit-width $k \in \{1, 2, \dots, 64\}$. Values are represented as `u64` integers invariant-masked to their lowest $k$ bits.

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Width(u8); // Validated: 1 <= k <= 64

impl Width {
    pub fn new(bits: u8) -> Result<Self, WidthError>;

    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    #[must_use]
    pub const fn mask(self) -> u64 {
        if self.0 == 64 {
            u64::MAX
        } else {
            (1u64 << self.0) - 1
        }
    }

    #[must_use]
    pub const fn add(self, a: u64, b: u64) -> u64 {
        a.wrapping_add(b) & self.mask()
    }

    #[must_use]
    pub const fn sub(self, a: u64, b: u64) -> u64 {
        a.wrapping_sub(b) & self.mask()
    }

    #[must_use]
    pub const fn mul(self, a: u64, b: u64) -> u64 {
        a.wrapping_mul(b) & self.mask()
    }

    #[must_use]
    pub const fn eqz(self, x: u64) -> u64 {
        ((x & self.mask()) == 0) as u64
    }

    /// Exact agreement indicator: EQZ(SUB(a, b)) (§2.3).
    #[must_use]
    pub const fn comparator(self, a: u64, b: u64) -> u64 {
        self.eqz(self.sub(a, b))
    }

    /// Graded circular residual magnitude min(d, 2^k - d) (§2.3), safe at k = 64.
    #[must_use]
    pub const fn circular_distance(self, a: u64, b: u64) -> u64 {
        let d = self.sub(a, b);
        let neg_d = d.wrapping_neg() & self.mask();
        if d < neg_d { d } else { neg_d }
    }
}
```

Using `d.wrapping_neg() & self.mask()` avoids `1u64 << 64` shift UB at $k = 64$ while computing exact $\min(d, 2^k - d)$ for every $1 \le k \le 64$ (including $d = 0 \implies 0$).

### 4.2 Level-0 Primitives (`primitive.rs`)

| Variant | Inputs (ordered) | Outputs (ordered) | State Registers | Tick Semantics ($w = \text{Width}(k)$) | At $k = 1$ |
| :--- | :---: | :---: | :---: | :--- | :--- |
| `Primitive::Add` | `in[0], in[1]` | `out[0]` | 0 | $y(t) = w.\text{add}(a(t), b(t))$ | `XOR` |
| `Primitive::Sub` | `in[0], in[1]` | `out[0]` | 0 | $y(t) = w.\text{sub}(a(t), b(t))$ (ordered: `in[0] - in[1]`) | `XOR` |
| `Primitive::Mul` | `in[0], in[1]` | `out[0]` | 0 | $y(t) = w.\text{mul}(a(t), b(t))$ | `AND` |
| `Primitive::Eqz` | `in[0]` | `out[0]` | 0 | $y(t) = w.\text{eqz}(x(t))$ | `NOT` |
| `Primitive::Delay { init }` | `in[0]` | `out[0]` | 1 | Emits $s(t)$ combinational-first; latches $s(t+1) = x(t)$ at tick end | `DELAY` |
| `Primitive::Split` | `in[0]` | `out[0], out[1]` | 0 | $y_0(t) = x(t),\; y_1(t) = x(t)$ | `SPLIT` |

### 4.3 Directed Port Graph & Wire Delay (`circuit.rs`)

A `Circuit` is parameterised by `Width`, `num_inputs: u16` ($K$ boundary input ports), `num_outputs: u16` ($K$ boundary prediction ports), and `WireDelayMode`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireDelayMode {
    /// Default (§2.5.3): wires are combinational (0 ticks); only DELAY holds state.
    Combinational,
    /// Ablation (§2.5.3, §8): every edge carries `hops >= 1` synchronous pipeline ticks.
    TicksPerHop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SignalSource {
    BoundaryIn(u16),
    NodeOut { node: NodeId, port: u8 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SignalSink {
    NodeIn { node: NodeId, port: u8 },
    BoundaryOut(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub src: SignalSource,
    pub dst: SignalSink,
    /// Ignored (treated as 0) in `Combinational` mode; must be >= 1 in `TicksPerHop` mode.
    pub hops: u16,
}
```

### 4.4 Structural Invariants & Compilation (`invariants.rs`)

`Circuit::compile(&self) -> Result<CompiledCircuit, InvariantError>` validates all five Charter §2.5 invariants before producing an executable schedule:

1. **Port bounds check:** Every `BoundaryIn`, `BoundaryOut`, `NodeId`, input slot, and output slot index must exist on its target primitive (`InvariantError::InvalidPort`).
2. **Single driver per sink (§2.5.4):** Every active primitive input slot `NodeIn { node, port }` must have **exactly one** incoming `Edge` (`InvariantError::UnconnectedInput` if 0, `InvariantError::MultipleDrivers` if $> 1$). Each `BoundaryOut(p)` must have **at most one** incoming `Edge` (unconnected boundary outputs default deterministically to `0`, or may be flagged in strict mode; multiple drivers return `InvariantError::MultipleDrivers`).
3. **`SPLIT`-only fan-out (§2.5.5):** Every `SignalSource` — including `BoundaryIn(c)` and each individual port `0` and `1` of `Primitive::Split` — may drive **at most one** `SignalSink` (`InvariantError::IllegalFanOut`). Zero sinks (discarded output) is valid.
4. **Wire-delay consistency (§2.5.3):** Under `WireDelayMode::TicksPerHop`, every edge must declare `hops >= 1` (`InvariantError::ZeroHopInDelayedMode`), expanding internally into a shift chain of `hops` synchronous wire registers.
5. **Combinational acyclicity (§2.5.1–§2.5.2):** Treating `Primitive::Delay` outputs and $>0$-tick wire-delay outputs as state sources (in-degree 0 in the combinational graph), Kahn's algorithm topologically sorts all remaining combinational dependencies using deterministic `NodeId` tie-breaking. If visited count $<$ combinational node count, compilation fails with `InvariantError::CombinationalLoop { participants }`.

### 4.5 Scalar and $k = 1$ Bit-Parallel Evaluators (`eval.rs`)

`CompiledCircuit` flattens execution into contiguous arrays with zero heap allocation per tick:

- **Phase 1 (Read state & inputs):** Load boundary inputs and current `Delay` / wire-hop register values into wire slots.
- **Phase 2 (Combinational pass):** Execute topological `CombOp` instructions sequentially ($O(1)$ per primitive, $O(N)$ per tick).
- **Phase 3 (Synchronous latch):** Copy next-state wire slots into the register array simultaneously; write driven boundary outputs.

Two state backends execute the exact same `CompiledCircuit`:

1. **`ScalarState` (`1 <= k <= 64`):** Stores wire values and registers as `Vec<u64>` masked to `width.mask()`. Exposes `step(&mut self, inputs: &[u64], outputs: &mut [u64])`.
2. **`BitParallel64State` (`k == 1` only, Charter §9):** Packs 64 independent trajectories (e.g., 64 initial register perturbations, 64 noise realisations, or 64 candidate verification traces) into a single `u64` per wire/register:
   - `Add` / `Sub` $\to$ `lhs ^ rhs`
   - `Mul` $\to$ `lhs & rhs`
   - `Eqz` $\to$ `!arg`
   - `Split` $\to$ `(arg, arg)`
   - `Delay` $\to$ `u64` synchronous register swap
   - Attempting to construct `BitParallel64State` when `width.bits() != 1` returns `EvalError::UnsupportedBitParallelWidth`.

---

## 5. 2D Spatial Lattice Topology (`src/structural/lattice.rs`)

To support Charter §3.1.1 geometry and §8 wiring ablations without entangling `substrate` with spatial coordinates, `structural::lattice` provides `LatticeCircuit`:

- **Grid geometry:** `GridShape { rows: u16, cols: u16 }`. Each cell `Coord { row, col }` is either empty or holds a single placed primitive (`NodeId`).
- **Perimeter boundary ports:** Input ports `BoundaryIn(c)` and output ports `BoundaryOut(c)` attach to explicit `BoundaryAnchor { side: North | South | East | West, offset: u16 }` positions on the perimeter.
- **Wiring topology modes (`WiringTopology`):**
  - `LocalVonNeumann`: Under `Combinational` wire delay, edges may only connect cells at Manhattan distance $d_1 = 1$; under `TicksPerHop`, longer straight/routed edges set `hops = manhattan_distance(src, dst) >= 1`.
  - `LocalMoore`: Chebyshev distance $d_\infty = 1$ neighbourhood variant.
  - `WellMixed`: Arbitrary cell-to-cell connections permitted with `hops = 1` (if `TicksPerHop`) or `0` (if `Combinational`), implementing the Charter §8 *"lattice vs. well-mixed wiring"* ablation cleanly.
- **Lowering:** `LatticeCircuit::lower(&self) -> Result<Circuit, LatticeError>` validates geometric constraints first and then delegates port/cycle checks to `Circuit::compile`.

---

## 6. Environment Generators and Ground-Truth Yardsticks (`src/env/`)

### 6.1 Finite-State Generator Abstraction (`generator.rs`, `noise.rs`)

Every M0 environment generator is a stationary deterministic finite-state machine over internal states $S = \{0, 1, \dots, |S|-1\}$ emitting $K$ channels of `Width(k)` values per tick:

```rust
pub struct FiniteStateSpec {
    pub width: Width,
    pub channels: u16,
    pub num_states: usize,
    pub initial_state: usize,
    /// Deterministic transition map: for each state s in 0..num_states,
    /// returns (next_state, emitted_symbols[0..channels]).
    pub transitions: Vec<(usize, Vec<u64>)>,
}

pub trait FiniteStateGenerator {
    fn name(&self) -> &str;
    fn width(&self) -> Width;
    fn channels(&self) -> u16;
    fn reset(&mut self);
    fn next_clean_frame(&mut self, out: &mut [u64]);
    /// Exact finite-state representation for analytical ground-truth solvers (§5.2).
    fn to_finite_state_spec(&self) -> Result<FiniteStateSpec, SpecExportError>;
}
```

`NoisyEnvironment` wraps any `FiniteStateGenerator` plus a declared `NoiseModel` (§5.1):

- `NoiseModel::None` ($\varepsilon = 0$).
- `NoiseModel::IndependentSubstitution { epsilon: f64, channels: Vec<u16> }`: on each named channel independently, with probability $\varepsilon \in [0, 1)$, replaces the clean symbol $x \in \mathbb{Z}_{2^k}$ uniformly at random with one of the $2^k - 1$ distinct symbols in $\mathbb{Z}_{2^k} \setminus \{x\}$ (drawn from `StreamDomain::ChannelNoise { channel }`). Both `clean_frame` and `observed_frame` are recorded in the run trace.

### 6.2 M0 Benchmark Catalogue (`tiers.rs`)

All M0 generators are parameterised by `Width(k)` and exercised at $k = 1$ and $k = 2$ (and configurable up to $k = 64$; note `to_finite_state_spec` exports full state tables whenever $|S| \le S_{\max}$, defaulting to $2^{16}$ states):

| Generator ID | Tier | Definition over $\mathbb{Z}_{2^k}$ ($K = 1$) | Purpose in M0 & M1 |
| :--- | :---: | :--- | :--- |
| `T1_Constant { value }` | T1 | $s(t) = c$ | Zero-complexity baseline ($C_\mu = 0, h_\mu = 0$); checks anti-silence null model |
| `T1_Alternating { a, b }` | T1 | Period-2 oscillation $a, b, a, b, \dots$ ($a \ne b$) | Minimal 1-bit memory test ($C_\mu = 1\text{ bit}$, solvable by 1 `DELAY` + `EQZ`/affine) |
| `T1_PeriodicMotif { motif }` | T1 | Repeats slice `motif[t mod p]` ($p \in \{3, 4, 6\}$) | Tests multi-register state bounds ($C_\mu = \log_2 p_{\min}$) and lag-2 vs. lag-6 period traps (§1.3) |
| `T1_AffineLag1 { mul_a, add_b, s0 }` | T1 | $s(t) = a \cdot s(t-1) + b \bmod 2^k$ | Single-register ring recurrence at $k \ge 1$ |
| `T3b_CounterPow2 { log2_mod }` | T3b | $s(t) = (s(t-1) + 1) \bmod 2^j$ ($1 \le j \le k$) | Native ring counter ($C_\mu = j\text{ bits}$) |
| `T3b_CounterModN { modulus }` | T3b | $s(t) = (s(t-1) + 1) \bmod n$ ($2 < n < 2^k$, $n \ne 2^j$, e.g. $n = 3$ at $k = 2$) | Non-polynomial recurrence requiring `EQZ` wrap-around (§2.3 completeness) |
| `T3c_Lfsr { taps, init }` | T3c | $s(t) = \sum_{i=1}^L c_i s(t-i) \bmod 2^k$ | Tests Reeds–Sloane synthesis & demonstrates parity hardness for local search (§5.3) |
| `T3c_LfsrCurriculum { stages }` | T3c | Ordered sequence of `T3c_Lfsr` stages sharing intermediate sub-parities | Curriculum harness ready for M4c library transfer |

### 6.3 Exact $\varepsilon$-Machine Solver (`epsilon_machine.rs`)

Charter §5.2 requires exact ground-truth statistical complexity $C_\mu$, entropy rate $h_\mu$, and synchronisation order $L_{\text{sync}}$ computed from the generator's finite-state process rather than noisy empirical sequence histograms.

Given `FiniteStateSpec` and `NoiseModel`:

1. **Recurrent component extraction:** Follow transitions from `initial_state` (or extract strongly connected attractor components reachable under `NoiseModel`) and compute the exact stationary distribution $\pi_{\text{gen}}$ over recurrent internal states $S_{\text{rec}}$ (cyclic uniform over the attractor cycle when $\varepsilon = 0$; power/linear stationary eigenvector solve when $\varepsilon > 0$).
2. **Symbol-labelled transition operators:** Over joint alphabet $\mathcal{A}$ ($|\mathcal{A}| = 2^{kK}$ for small $kK$), form sub-stochastic operators $T^{(x)}_{s, s'} = \Pr(X_t = x, S_{t+1} = s' \mid S_t = s)$.
3. **Causal-state construction (two exact regimes):**
   - **Unifilar / Noiseless fast path ($\varepsilon = 0$):** Perform Moore/Hopcroft partition refinement on $S_{\text{rec}}$: initialise equivalence classes by immediate emission $f(s)$, then iteratively split any class whose members transition to distinct equivalence classes until fixed point. Each equivalence class $\sigma \in \mathcal{S}$ is an exact recurrent causal state with probability $\Pr(\sigma) = \sum_{s \in \sigma} \pi_{\text{gen}}(s)$ and $h_\mu = 0$. Synchronisation order $L_{\text{sync}}$ is computed exactly from the subset-construction DAG starting at $\eta_0 = S_{\text{rec}}$: it is the shortest path length $L$ at which all reachable positive-probability subsets lie wholly inside single causal-state classes (or `None` if strictly non-synchronisable).
   - **Noisy HMM Mixed-State Presentation ($\varepsilon > 0$):** Because channel substitution noise breaks unifilarity, internal states are no longer causal states. Starting from belief distribution $\eta_0 = \pi_{\text{gen}}$, explore belief updates
     $$\eta' = \frac{\eta^\top T^{(x)}}{\eta^\top T^{(x)} \mathbf{1}}$$
     quantised/merged at tolerance $10^{-12}$ up to a configurable state cap (default `4096` mixed states), extract the recurrent ergodic causal-state classes $\mathcal{S}$ and their stationary weights $\Pr(\sigma)$, and compute:
     $$C_\mu = -\sum_{\sigma \in \mathcal{S}} \Pr(\sigma) \log_2 \Pr(\sigma), \qquad h_\mu = -\sum_{\sigma \in \mathcal{S}} \Pr(\sigma) \sum_{x \in \mathcal{A}} \Pr(x \mid \sigma) \log_2 \Pr(x \mid \sigma).$$
4. **Derived circuit bound:** Expose `min_delay_registers = (c_mu_bits / f64::from(width.bits())).ceil() as usize` (§5.2).

### 6.4 Reeds–Sloane Synthesis over $\mathbb{Z}_{2^k}$ (`reeds_sloane.rs`)

Over a field ($k = 1$, $GF(2)$), classical Berlekamp–Massey divides discrepancies by prior non-zero discrepancies. Over $\mathbb{Z}_{2^k}$ with $k > 1$, even non-zero discrepancies such as $2 \in \mathbb{Z}_4$ are zero divisors and have no multiplicative inverse. M0 implements **Reeds & Sloane (1985)** (*SIAM J. Comput.*, 14(3), 505–513) specialised to prime-power modulus $2^k$:

1. **Unit-valuation factorisation:** Any non-zero $d \in \mathbb{Z}_{2^k}$ decomposes uniquely as $d = u \cdot 2^v \pmod{2^k}$ where $v = \text{trailing\_zeros}(d) \in \{0, \dots, k-1\}$ and $u = d \gg v$ is odd.
2. **Hensel odd inversion:** Every odd $u$ has a unique inverse $u^{-1} \bmod 2^{64}$ computed in 6 deterministic Newton–Hensel steps ($x_{i+1} = x_i \cdot (2 - u \cdot x_i)$ starting from $x_0 = 1$), then masked to `width.mask()`.
3. **$k$-track ideal update:** Maintain $k$ polynomial pairs $(a^{(\eta)}(D), b^{(\eta)}(D))$ for $\eta \in \{0, \dots, k-1\}$ satisfying $a^{(\eta)}(0) \equiv 2^\eta \pmod{2^k}$, updating discrepancies in $O(k \cdot N^2)$ operations as symbols arrive incrementally via `push_symbol(s)`.
4. **Exact $k = 1$ reduction:** At $k = 1$, $\eta \in \{0\}$ only, $v = 0$, and $u = u^{-1} = 1$, reducing step-for-step to classical $GF(2)$ Berlekamp–Massey and recovering the exact minimal LFSR of length $L$ within $2L$ symbols (§1.4, §5.2).

---

## 7. Measurement and MDL Yardsticks (`src/metrics/`)

### 7.1 Null Models and Surprisal (`null_model.rs`)

Implements Charter §3.1.4 surprisal $-\log_2 p_{\text{null}}(s(t))$ cleanly decoupled from reward payout so M1a can consume it unchanged:

- `NullModelKind::LaplaceFrequency`: maintains symbol counts $N_x$ over $x \in \mathbb{Z}_{2^k}$ with Krichevsky–Trofimov $\alpha = \tfrac{1}{2}$ smoothing:
  $$p_{\text{null}}(s_t = x) = \frac{N_x(t) + \tfrac{1}{2}}{t + 2^{k-1}}.$$
  On a constant stream (`T1_Constant`), $p_{\text{null}} \to 1$ and surprisal $\to 0\text{ bits}$, automatically starving silence/trivial oscillators in M1a.
- `NullModelKind::RepeatLast`: tracks smoothed empirical match probability $\Pr(s_t = s_{t-1})$ vs. uniform change over the remaining $2^k - 1$ symbols.

### 7.2 Total Description Length $L$ (`mdl.rs`)

Charter §3.4 calibrates the system against

$$L = L(\text{palette}) + L(\text{circuit} \mid \text{palette}) + L(\text{facts} \mid \text{circuit}).$$

M0 fixes an explicit, auditable bit-length encoding:

1. **$L(\text{palette})$:** `0.0` bits for the fixed 6-primitive Level-0 base palette in M0–M4a; for derived macro definitions (M4b+), sum of each macro's internal circuit description length plus interface arity header.
2. **$L(\text{circuit} \mid \text{palette})$:**
   - Elias-$\gamma$ prefix code for node count $N$ and edge count $M$: $\ell_\gamma(n) = 2 \lfloor \log_2(n + 1) \rfloor + 1$ bits.
   - Primitive type tags: $N \cdot \log_2 |\mathcal{P}|$ bits ($|\mathcal{P}| = 6$ at Level 0, plus $k$ bits per `Delay` initial register if non-zero).
   - Wiring description: under `WellMixed`, $\sum_{e=1}^M \log_2(|V_{\text{src}}| \cdot |V_{\text{sink}}|)$ bits; under `LocalVonNeumann` on a lattice, occupied-cell coordinates plus 2-bit compass-direction codes per local edge, directly rewarding spatial locality in bits.
3. **$L(\text{facts} \mid \text{circuit})$ over $T$ observed ticks (per channel):**
   - Let $E$ be the number of exact prediction matches over $T$ ticks ($e_t = 0$ if $\hat{s}_t = s_t$, else $1$). Encode the binary error sequence via KT combinadic/sequential code $L_{\text{KT}}(E, T - E)$, plus $(T - E) \cdot \log_2(2^k - 1)$ bits to specify the non-matching residual symbol whenever $\hat{s}_t \ne s_t$.
   - When a circuit predicts every tick after a transient of $T_0$ ticks ($T - E = T_0 \ll T$), $L(\text{facts} \mid \text{circuit}) / T \to 0$, and total $L$ reduces to $L(\text{circuit}) + O(\log T)$.

### 7.3 Dynamical Inspector (`dynamics.rs`) and Scorecard (`scorecard.rs`)

- **Autonomous limit-cycle & transient analysis (§2.6):** Connects a `CompiledCircuit`'s boundary outputs back to its boundary inputs ($\hat{A} \to A$) and detects exact `(transient_ticks, cycle_length)` from any initial `Delay` state vector using Brent's cycle-detection algorithm.
- **Basin size estimation (§2.6):** Given a target attractor cycle, perturbs `Delay` registers (exhaustively when $k \cdot N_{\text{delay}} \le 12$, or via seeded random sampling accelerated 64× at $k = 1$ using `BitParallel64State`) and reports `basin_fraction: f64` and mean Hamming/circular recovery time.
- **`Scorecard` (§8):** Aggregates `post_transient_bits_per_symbol`, `excess_loss_over_h_mu`, `exact_match_rate`, `mean_circular_discrepancy`, `delay_count`, `excess_state_bits_over_c_mu`, `node_count`, `excess_nodes_over_exact_synthesis`, `mdl_total_bits`, `ticks_to_solution`, and `tick_budget_consumed`.

---

## 8. Equal-Compute Baseline Harness (`src/baselines/`)

### 8.1 Prequential Protocol and `TickBudget` (`baselines/mod.rs`)

Every baseline implements `PredictorBaseline` and is stepped through `HarnessRunner` under strict prequential causal ordering:

```rust
pub struct TickBudget {
    remaining: u64,
    consumed: u64,
}

pub struct PredictionStep {
    /// Point prediction in Z_{2^k} per channel (used for exact-match & residual metrics).
    pub symbols: Vec<u64>,
    /// Optional log2 probability assigned to the true symbol once revealed, or full distribution.
    pub symbol_probs: Option<Vec<Vec<f64>>>,
}

pub trait PredictorBaseline {
    fn name(&self) -> &str;
    /// Step 1: Commit to prediction for tick `t` before seeing ground truth `y(t)`.
    fn predict(&mut self, budget: &mut TickBudget) -> PredictionStep;
    /// Step 2: Observe ground truth `y(t)` and optionally search/update within `budget`.
    fn observe(&mut self, truth: &[u64], budget: &mut TickBudget);
    /// Return best discovered substrate Circuit, if this baseline synthesises circuits.
    fn best_circuit(&self) -> Option<&Circuit>;
}
```

- **Equal-compute enforcement (§8):** Every candidate circuit evaluation over a history buffer of length $W$ deducts $W$ ticks from `TickBudget` (even if executed 64-wide via `BitParallel64State`, each logical trajectory tick is counted 1-for-1 so bit-parallelism speeds up wall-clock runtime without granting unfair extra algorithmic compute!). When `budget.remaining == 0`, circuit search halts and the incumbent circuit predicts remaining ticks directly.

### 8.2 M0 Baseline Specifications

1. **`RandomSearchBaseline` (`random_search.rs`):**
   - Constructively samples invariant-valid `Circuit` instances with $N \sim \text{Uniform}(1..=N_{\max})$ nodes from `StreamDomain::BaselineSearch` by maintaining an available-source pool (guaranteeing Invariants 2, 4, and 5 by construction without rejection-sampling waste), inserting `Split` nodes whenever an existing wire must be tapped twice.
   - Scores candidates on the bounded fact buffer of recent wake observations (default capacity $W = 256$) by MDL $L$ (tie-breaking on fewer nodes) and retains the lowest-$L$ incumbent.
2. **`CgpBaseline` (`cgp.rs`):**
   - Implements Miller's $(1 + \lambda)$ Evolution Strategy ($\lambda = 4$ default) over an indexed grid genotype of `Add`, `Sub`, `Mul`, `Eqz`, and `Delay` genes.
   - **Invariant-preserving `Split` lowering:** Standard CGP allows implicit multi-consumer fan-out on active genes. Before evaluating and scoring any CGP phenotype, active fan-out $f > 1$ on any wire is deterministically expanded into a balanced binary tree of $f - 1$ `Primitive::Split` nodes, then compiled via `Circuit::compile`. Thus CGP circuit size, `Split` cost, and $L(\text{circuit})$ obey Charter §2.5.5 on identical terms with all other circuits.
   - Uses neutral genetic drift: an offspring replaces the parent whenever `offspring_mdl <= parent_mdl`.
3. **`CtwBaseline` (`ctw.rs`):**
   - Implements Willems–Shtarkov–Tjalkens (1995) Context-Tree Weighting up to configurable Markov depth $D$ (default $D = 12$).
   - **Multi-width extension ($k \ge 1$):** Uses Tjalkens–Volf–Willems bit-decomposed CTW — each $k$-bit symbol step traverses $k$ binary KT context trees conditioned on preceding symbols plus the higher bits of the current symbol. At $k = 1$ this is exact classical binary CTW; at $k > 1$ it outputs an exact normalised distribution over all $2^k$ symbols without $2^{kD}$ branching blow-up, predicting $\hat{s}_t = \arg\max_x q_t(x)$ and reporting exact sequential code length $-\log_2 q_t(s_t)$.
4. **`ReedsSloaneBaseline` (`bm_predictor.rs`):**
   - Online prequential wrapper around `env::reeds_sloane::ReedsSloaneSynthesizer`. Predicts $\hat{s}(t) = \sum_{i=1}^L c_i s(t-i) \bmod 2^k$ using the current minimal polynomial, then calls `push_symbol(s(t))`. Achieves zero error for all $t \ge 2L$ on noiseless linear recurrences over $\mathbb{Z}_{2^k}$.
5. **`ExhaustiveBaseline` (`exhaustive.rs`):**
   - Enumerates canonical invariant-valid circuits in strict size order $N = 0, 1, 2, \dots, N_{\max}$ (default $N_{\max} = 4$ for T1).
   - Applies symmetry reductions: canonical topological node numbering, commutative input ordering (`src_0 <= src_1`) on `Add` and `Mul`, and pruning dead subgraphs disconnected from `BoundaryOut`.
   - At $k = 1$, packs 64 candidate evaluations (or 64 initial states) into `BitParallel64State` for rapid pruning. Doubles as `env`'s **exact-synthesis minimum circuit size oracle** (§5.2, §8).

### 8.3 Deferred Neural Baseline Hook

`PredictorBaseline` already supports internal continuous state and categorical `symbol_probs` without requiring `best_circuit() -> Some(...)` (just like `CtwBaseline`). When Milestone M1b/M2 adds the small `RNN` / `LSTM` BPTT baseline, it will implement `PredictorBaseline` directly in `src/baselines/rnn.rs` with zero changes to `HarnessRunner` or `Scorecard`.

---

## 9. CLI Entry Points and Configuration Schema

### 9.1 CLI Subcommands (`src/main.rs`)

- `arkhe run --config <path.toml> [--output <report.json>]` — executes one reproducible experiment configuration and writes its `RunReport`.
- `arkhe yardstick --tier <generator_spec> --width <k> [--epsilon <f64>]` — prints the exact analytical ground truth ($C_\mu$, $h_\mu$, $L_{\text{sync}}$, minimal `DELAY` count, Reeds–Sloane recurrence length, and optional small-circuit exact synthesis size).
- `arkhe verify-m0 [--seeds <N>]` — runs the pre-registered M0 T1 verification battery across $k = 1$ and $k = 2$ and exits non-zero if any acceptance gate fails.

### 9.2 Example `ExperimentConfig` TOML

```toml
schema_version = 1
master_seed = 42

[substrate]
width_bits = 2
wire_delay = "Combinational"
wiring_topology = "WellMixed"

[environment]
horizon_ticks = 500
transient_warmup_ticks = 50
generator = { kind = "T1_PeriodicMotif", motif = [0, 1, 3] }
noise = { kind = "None" }

[budget]
max_eval_ticks = 50_000

[baseline]
kind = "Cgp"
lambda = 4
max_genes = 8
fact_window = 128
```

---

## 10. Pre-Registered Milestone 0 Acceptance Criteria

Per Charter §7 (*"Baselines reach known optima on T1; every run reproducible from seed and configuration"*) and §8 (*"From M1a on, every milestone runs at $k = 1$ and $k = 2$ at least... success margins, seed counts, and kill criteria are written down before the runs"*), M0 completion is gated on four automated acceptance checks across **20 pre-registered master seeds (`0..20`)**:

| Gate | Requirement | Exact Quantitative Pass Criterion |
| :--- | :--- | :--- |
| **G1 — Substrate & invariants** | Exact $\mathbb{Z}_{2^k}$ arithmetic, §2.5 invariant rejection, and bit-parallel equivalence | 100% pass at $k \in \{1, 2, 4, 8, 16, 32, 64\}$; every §2.5 invariant violation returns its designated `InvariantError`; `BitParallel64State` matches 64 `ScalarState` lanes bit-for-bit across 100 random valid circuits × 1,000 ticks |
| **G2 — Ground-truth yardsticks** | Exact $\varepsilon$-machine ($C_\mu, h_\mu, L_{\text{sync}}$) and Reeds–Sloane synthesis | $\lvert C_\mu - C_\mu^{\text{analytic}} \rvert < 10^{-9}$ and $\lvert h_\mu - h_\mu^{\text{analytic}} \rvert < 10^{-9}$ on all T1, T3b, and small T3c generators ($\varepsilon = 0$ and $\varepsilon = 0.05$); `ReedsSloane` achieves 0 prediction error for all $t \ge 2L$ on $k = 1$ ($L \le 12$) and $k = 2$ ($L \le 8$, including even/zero-divisor taps) |
| **G3 — T1 optima at $k = 1$ & $k = 2$** | Baselines reach known optima on T1 suite (`T1_Constant`, `T1_Alternating`, `T1_PeriodicMotif(p=3)`, `T1_AffineLag1`) | Across seeds `0..20` at both $k = 1$ and $k = 2$: (a) **Noiseless ($\varepsilon = 0$):** `Exhaustive`, `Ctw`, and `Cgp` achieve `0.0` post-transient errors on $\ge 19/20$ seeds (`100%` for deterministic `Exhaustive` and `Ctw`), with `Exhaustive` finding circuits matching minimal state bound $\lceil C_\mu / k \rceil$; (b) **Noisy ($\varepsilon = 0.05$):** `Ctw` and `Cgp` post-transient loss lies within $0.02\text{ bits/symbol}$ of exact $h_\mu(\varepsilon)$ |
| **G4 — Determinism & reproducibility** | Seed + config uniquely determine output | Running any `ExperimentConfig` twice in separate OS processes produces byte-for-byte identical JSON `RunReport` outputs; golden `ChaCha8Rng` vectors match exactly |

---

## 11. Phased Implementation Roadmap

Each phase is scoped as a self-contained, reviewable increment passing `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo fmt --check`:

### Phase M0.1: Crate Rename, Latest Stable Toolchain & Dependencies, and Deterministic Core

- Rename package/lib/bin to `arkhe` and bump `rust-version` from `"1.85"` to `"1.99.1"` in `Cargo.toml`; update `.github/copilot-instructions.md`, `README.md`, and `tests/cli.rs`.
- Add latest stable releases of `clap` (`4.6.7`), `rand` (`0.10.3`, without `sys_rng`/`thread_rng`), `rand_chacha` (`0.10.0`), `rand_core` (`0.10.1`), `serde` (`1.0.229`), `serde_json` (`1.0.154`), and `toml` (`1.1.8`).
- Implement `src/rng.rs` (`SeedBank`, `StreamDomain`) and `src/config.rs` schema stubs.
- Add `tests/determinism.rs` with golden ChaCha output vectors.

### Phase M0.2: Substrate Values, Primitives, Invariants, and Evaluators

- Implement `src/substrate/value.rs` (`Width`, wrapping ring ops, `comparator`, `circular_distance`, including $k = 64$ boundary tests).
- Implement `src/substrate/primitive.rs`, `src/substrate/circuit.rs`, and `src/substrate/invariants.rs` enforcing all five §2.5 invariants.
- Implement `src/substrate/eval.rs` (`CompiledCircuit`, `ScalarState`, `BitParallel64State`).
- Add differential equivalence tests (`64 × ScalarState == BitParallel64State`) and negative invariant tests.

### Phase M0.3: 2D Spatial Lattice Topology

- Implement `src/structural/lattice.rs` (`GridShape`, `Coord`, `BoundaryAnchor`, `WiringTopology`, Manhattan hop calculation, and lowering to `substrate::Circuit`).
- Test `Combinational` vs. `TicksPerHop` and `LocalVonNeumann` vs. `WellMixed` lowering invariants.

### Phase M0.4: Environment Generators and Ground-Truth Solvers

- Implement `src/env/generator.rs`, `src/env/noise.rs`, and `src/env/tiers.rs` (`T1`, `T3b`, `T3c` at arbitrary $k$).
- Implement `src/env/epsilon_machine.rs` (Moore/Hopcroft unifilar partition refinement + Mixed-State Presentation for $\varepsilon > 0$).
- Implement `src/env/reeds_sloane.rs` (Hensel odd inversion + $k$-track ideal synthesis).
- Verify closed-form $C_\mu$, $h_\mu$, $L_{\text{sync}}$, and $2L$ LFSR convergence at $k = 1, 2, 4, 8$.

### Phase M0.5: Yardstick Metrics and Five Discrete Baselines

- Implement `src/metrics/` (`null_model.rs`, `mdl.rs`, `dynamics.rs`, `scorecard.rs`).
- Implement `src/baselines/` (`TickBudget`, `HarnessRunner`, `random_search.rs`, `cgp.rs` with `Split` tree expansion, bit-decomposed `ctw.rs`, `bm_predictor.rs`, and canonical `exhaustive.rs`).
- Verify `TickBudget` conservation and prequential causal isolation (predictors cannot peek at $y(t)$ before predicting $\hat{y}(t)$).

### Phase M0.6: CLI Integration, Pre-Registered Suite, and Packaging Check

- Wire `arkhe run`, `arkhe yardstick`, and `arkhe verify-m0` in `src/main.rs`.
- Add integration test suite `tests/m0_acceptance.rs` executing Gates G1–G4 at $k = 1$ and $k = 2$.
- Update `README.md` with M0 CLI usage and run full repository validation including `cargo package --allow-dirty` / `cargo package`.

---

## 12. Technical Risks, Edge Cases, and Mitigations

| Risk / Edge Case | Failure Mode | Mitigation Built into M0 |
| :--- | :--- | :--- |
| **`1u64 << 64` shift UB at width $k = 64$** | Panic in debug builds or wrong wrap-around mask in release builds | `Width::mask` branches explicitly on `bits == 64`, and `circular_distance` uses `d.wrapping_neg() & mask`; $k = 64$ is included in Gate G1 unit tests |
| **CGP implicit fan-out cheating Invariant 5** | Standard CGP reuses node outputs for free, undercounting circuit size and MDL $L$ relative to substrate circuits | `CgpBaseline` expands every $f$-way fan-out on active wires into $f - 1$ explicit `Primitive::Split` nodes prior to `Circuit::compile` and MDL scoring |
| **Mixed-State Presentation blow-up under noise** | Non-unifilar noisy HMMs can generate infinitely many distinct real-valued belief vectors $\eta$ | Unifilar ($\varepsilon = 0$) processes take the exact $O(\lvert \mathcal{A} \rvert \cdot \lvert S \rvert \log \lvert S \rvert)$ Hopcroft fast path; noisy processes quantise/merge belief vectors within $\ell_\infty < 10^{-12}$ up to an explicit `max_mixed_states` bound, returning a typed `ApproxBound` warning if truncated |
| **Bit-parallel speedup distorting equal-compute comparisons** | Packing 64 trajectories into one `u64` op could accidentally give `k = 1` search 64× more algorithmic ticks than `k = 2` | `TickBudget` always charges logical trajectory ticks (`num_lanes_active * ticks_run`), keeping algorithmic compute invariant across scalar and bit-parallel backends |
| **Broken `GEMINI.md` symlink during doc updates** | Overwriting `GEMINI.md` via naive file creation could replace the symlink with a regular file | Phase M0.1 edits `.github/copilot-instructions.md` in place and verifies `test -L GEMINI.md` |

---

## 13. Validation Checklist

Every M0 PR and final milestone sign-off must pass:

```bash
# 1. Format, lint, unit & integration tests
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test

# 2. Pre-registered M0 benchmark & reproducibility gate
cargo run --release -- verify-m0 --seeds 20

# 3. Markdown lint check on documentation
files=$( { git diff HEAD --name-only --diff-filter=AM -- '*.md'; git ls-files -o --exclude-standard -- '*.md'; } | sort -u )
[ -n "$files" ] && printf '%s\n' "$files" | xargs -d '\n' npx markdownlint-cli2 --fix
npx markdownlint-cli2 "doc/**/*.md" README.md
```
