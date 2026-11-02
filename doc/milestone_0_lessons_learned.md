# Milestone 0 Lessons Learned: Design Decisions, Traps, and Solutions

> **Context:** Engineering and research retrospective from implementing and closing out **Milestone 0 (`M0 — Simulator and yardstick`)** of [foundational_research_charter_v2.md](foundational_research_charter_v2.md) according to [milestone_0_plan.md](milestone_0_plan.md), [milestone_0_review.md](milestone_0_review.md), and [milestone_0_closeout_plan.md](milestone_0_closeout_plan.md).
>
> **Purpose:** Record concrete architectural choices, mathematical subtleties, evolutionary/MDL failure modes, and Rust implementation lessons so contributors working on Milestones **M1a–M6** understand *why* the M0 codebase is shaped the way it is—and which traps to avoid next.

---

## 1. Toolchain, Dependencies, and Architectural Boundaries

### 1.1 Upgrading to Rust `1.99.1` (Edition `2024`) and Latest Crates

The repository began from a minimal Rust template (`rust-base` / `protoproject`) pinned to `rust-version = "1.85"`. Before writing M0 code, we renamed the crate and binary to `arkhe` and moved directly to the latest stable toolchain (`1.99.1`, `edition = "2024"`) and current stable releases (`clap 4.6`, `rand 0.10`, `rand_chacha 0.10`, `rand_core 0.10`, `serde 1.0`, `toml 1.1`). Three practical details surfaced immediately:

1. **Edition `2024` reserves `gen`:** Variable names like `let mut gen = ...` fail to parse in Rust 2024 because `gen` is reserved for generator blocks. The codebase standardises on `generator` or `clean_gen`.
2. **`rand_core 0.10` trait rename (`RngCore` $\to$ `Rng`):** In `rand_core 0.10`, the low-level word-stream trait previously named `RngCore` is now `rand_core::Rng` (`next_u32`, `next_u64`, `fill_bytes`).
3. **Compile-time elimination of OS randomness:** Depending on `rand` with `default-features = false, features = ["std"]` omits `getrandom`, `sys_rng`, and `thread_rng` from `Cargo.lock` completely. Combined with banning `HashMap` / `HashSet` in favour of `Vec`, `BTreeMap`, and `BTreeSet`, nondeterministic iteration or seeding cannot be introduced accidentally—a guarantee locked in by `tests/determinism.rs` and Gate **G4**.

### 1.2 Separating `substrate` from `structural::lattice` Early

A key design question during planning was whether M0 needed 2D spatial coordinates at all before M1a introduces metabolic energy and local credit. Implementing pure topological execution in `src/substrate/` while placing 2D grid coordinates, perimeter anchors, `WiringTopology` (`LocalVonNeumann`, `LocalMoore`, `WellMixed`), and Manhattan/Chebyshev hop-delay lowering in `src/structural/lattice.rs` paid off in two ways:

- `src/substrate/` contains **zero floating-point types** (`f32`/`f64`) and zero geometry concepts, keeping the core tick evaluator compact and auditable.
- `WireDelayMode::TicksPerHop` and spatial wire-length MDL penalties (`compute_mdl`) are already exercised end-to-end in M0 rather than requiring a breaking graph refactor in M1a.

### 1.3 Deferring Continuous `RNN/LSTM` Baselines to M1b/M2

Charter §8 lists both discrete circuit/compression baselines (`RandomSearch`, `CGP`, `CTW`, `Berlekamp–Massey`, `Exhaustive`) and small `RNN/LSTM` predictors trained under matched tick budgets. Pulling an autodiff/tensor stack into M0 would have added substantial dependency weight before any online learning existed to compare against. Defining `PredictorBaseline` and `TickBudget` around prequential online prediction allowed all five discrete/symbolic baselines to ship in pure Rust now while keeping a drop-in seam for `RNN/LSTM` in M1b/M2.

---

## 2. Substrate Semantics and $\mathbb{Z}_{2^k}$ Arithmetic Subtlety

### 2.1 Causal Prequential Inputs Carry 1 Register of Boundary Lag

Under Charter §3.3, §4.1, and §4.2 autonomous loopback ($\hat{A}(t+1) \to A(t+1)$), predicting $\hat{x}(t)$ on tick $t$ feeds the previous observation $x(t-1)$ into `BoundaryIn` (with `0` supplied at $t = 0$).

During Gate **G3** verification, checking the causal-state lower bound $\text{delays} \ge \lceil C_\mu / k \rceil$ exposed an important physical distinction:

- On noiseless `T1_Alternating` (`0, 1, 0, 1, ...` at $k = 1$, where $C_\mu = 1$ bit), `ExhaustiveBaseline` discovered the 1-node **combinational** inverter `BoundaryIn(0) -> Eqz -> BoundaryOut(0)` with **zero** internal `Delay` nodes and 0 post-transient errors.
- Why didn't that violate $C_\mu = 1$ bit? Because the causal prequential boundary `BoundaryIn` *already* holds $x(t-1)$ ($k$ bits of lag-1 state supplied by the environment interface). Thus the total accessible state is `effective_state_registers = delay_count + wire_hop_register_count + boundary_lag_register_count`, which universally satisfies $\text{effective\_state\_registers} \cdot k \ge C_\mu$ (`effective_excess_state_bits_over_c_mu >= 0.0`).
- Furthermore, **node minimality and state minimality diverge at $k \ge 2$:** on `T1_PeriodicMotif([0, 1, 3])` over $\mathbb{Z}_4$ ($C_\mu = \log_2 3 \approx 1.585$ bits, giving a 1-register state bound $\lceil C_\mu / 2 \rceil = 1$), any lag-1 map $0 \mapsto 1 \mapsto 3 \mapsto 0$ violates parity preservation modulo $2$ ($1 \equiv 3 \pmod 2$ while $3 \not\equiv 0 \pmod 2$) and requires nonlinear `Eqz`, `Split`, and constant synthesis (`11` nodes in `build_z4_motif_013_reactive_one_reg_witness`), whereas the node-minimal circuit is the **2-node** pure shift chain `BoundaryIn -> Delay -> Delay -> BoundaryOut` (`3` effective registers, `0` arithmetic or `Split` nodes).
- Crucially, as soon as substitution noise ($\varepsilon > 0$) is introduced—or when the circuit is disconnected from external inputs during autonomous loopback—reactive `BoundaryIn` shortcuts degrade, forcing the predictor to internalise state into recurrent `Delay` loops. This directly previews Charter Hypothesis **H3** (reactive shortcuts vs. internal world models).

### 2.2 64-Bit Ring Extremes ($k = 64$)

Supporting uniform runtime widths $1 \le k \le 64$ in `u64` requires avoiding `1u64 << k` whenever $k = 64$ (which panics in debug mode or shifts `k & 63 == 0` in release). Two idioms in `src/substrate/value.rs` eliminated all width-64 special-case bugs:

- Computing bitmasks as `if k == 64 { u64::MAX } else { (1u64 << k) - 1 }` and storing alphabet cardinality $2^k$ in `u128` (`1u128 << k`).
- Computing modular circular distance $\min(d, 2^k - d)$ without `u64` subtraction overflow via wrapped two's-complement negation: `let d = a.wrapping_sub(b) & mask; d.min(d.wrapping_neg() & mask)`.

### 2.3 Zero-Initialised Pipeline Illusion in Hop-Delay Tests

When testing `WireDelayMode::TicksPerHop` on a 4-hop Manhattan wire feeding `Primitive::Eqz`, feeding `0` on tick $0$ appeared to arrive at tick $3$ instead of tick $4$. The culprit was not off-by-one hop lowering: every pipeline `Delay` register initialises to `0`, so `Eqz(0)` already emitted `1` from the *initial register zeros* before the boundary `0` arrived! Testing pipeline latency with non-zero graded symbols (`[3, 2, 0, ...]` at $k = 2$ through `Primitive::Split`) cleanly distinguished initial zero-fill from true signal arrival.

---

## 3. Exact Ground Truth Over Rings With Zero Divisors

### 3.1 Why $GF(2)$ Berlekamp–Massey Breaks at $k \ge 2$

At $k = 1$, $\mathbb{Z}_2 \cong GF(2)$ is a field: every non-zero discrepancy $d = 1$ is invertible, so classical Berlekamp–Massey finds the shortest linear feedback shift register in $2L$ symbols. At $k \ge 2$, $\mathbb{Z}_{2^k}$ is a **ring with zero divisors**—for instance, in $\mathbb{Z}_4$, $2 \times 2 \equiv 0 \pmod 4$, so a discrepancy of $d = 2$ cannot be inverted.

To give Milestone 0 an exact analytical recurrence yardstick at all widths $1 \le k \le 64$, `src/env/reeds_sloane.rs` implements **Reeds–Sloane shift-register synthesis** over $\mathbb{Z}_{2^k}$:

- Every non-zero discrepancy $\theta_\eta \in \mathbb{Z}_{2^k}$ factors uniquely as $\theta_\eta = u_\eta \cdot 2^g$ with odd unit $u_\eta$ and 2-adic valuation $g = v_2(\theta_\eta) \in \lbrace 0, \dots, k - 1 \rbrace$.
- Odd-unit inverses $u_\eta^{-1} \bmod 2^k$ are computed in $O(\log k)$ multiplications via Newton–Hensel lifting (`inv = inv * (2 - u * inv)`), requiring zero lookup tables and working up to $k = 64$.
- Instead of a single previous witness polynomial, the synthesizer maintains $k$ witness slots indexed by valuation $g$.

**Implementation lesson:** Off-by-one mistakes in Reeds–Sloane's per-valuation length update only manifest on specific multi-valuation zero-divisor sequences and can pass simple unit tests unnoticed. Writing a **brute-force minimal-$L$ oracle** over $\mathbb{Z}_4$ in the unit tests and checking 500 random streams plus adversarial zero-divisor recurrences caught subtle score-comparison edge cases immediately.

### 3.2 `EQZ` Completeness and Self-Healing Non-Power-of-Two Counters

Over $\mathbb{Z}_{2^k}$ for $k \ge 2$, polynomial operations (`Add`, `Sub`, `Mul`) are congruence-preserving modulo $2$ ($f(x) \equiv f(x + 2) \pmod 2$), meaning no polynomial circuit over $\mathbb{Z}_4$ can implement a period-3 counter (`0, 1, 2, 0, 1, 2, ...`). Including `Primitive::Eqz` (`1` if $x = 0$, else `0`) restores functional completeness cleanly. In `tests/m0_acceptance.rs`, the identity

$$
f(x) = (x + 1) + \operatorname{EQZ}(x + 2) \pmod 4
$$

not only steps $0 \mapsto 1 \mapsto 2 \mapsto 0$ over $\mathbb{Z}_4$, it also maps the out-of-attractor perturbed state $3 \mapsto 0$ in a single tick—yielding an exact period-3 limit cycle with `basin_fraction == 1.0`.

---

## 4. MDL and Evolutionary Search Traps Uncovered by Gate G3

Pre-registering Gate **G3** / **G3-rev1** (`arkhe verify-m0`)—requiring near-zero excess prediction loss on both noiseless and $\varepsilon = 0.05$ noisy **T1** streams at $k = 1$ and $k = 2$—exposed five deep interactions between description length, sliding evaluation windows, channel variance, and discrete search.

### 4.1 Trap 1: The $k = 1$ KT Anti-Correlation Symmetry

To score residual errors without arbitrary hyperparameters, `src/metrics/mdl.rs` encodes the binary match/mismatch indicator sequence of length $T$ using the exact Krichevsky–Trofimov (`KT`) sequential code (`kt_binary_sequence_bits`), plus $m \log_2(2^k - 1)$ bits naming the true alternative on each of the $m$ mismatches.

At $k \ge 2$, $\log_2(2^k - 1) > 0$ breaks symmetry between `0` errors and `T` errors. At $k = 1$, however, $\log_2(2^1 - 1) = 0$! Because `KT(matches, mismatches)` is symmetric in its two arguments, a circuit with **100% mismatches** (predicting the exact opposite bit every tick) received the **exact same MDL score** as a circuit with **0% mismatches**.

- **Fix:** Monotonised `facts_code_bits` for error counts exceeding the uniform-chance expectation $\lfloor T(1 - 2^{-k}) \rfloor$ so worse-than-chance predictors never beat or tie chance predictors.

### 4.2 Trap 2: Sliding-Window Phase Slip on Autonomous Oscillators

To bound memory and evaluation cost on long horizons, circuit-search baselines evaluate candidates over a trailing window of recent observations (`fact_window`). Initially, once `history.len()` reached capacity, `history.remove(0)` evicted one tick per step.

On an autonomous period-2 oscillator `d0 = Delay(0) <-> d1 = Delay(3)`, `evaluate_circuit_on_window` resets registers to `(0, 3)` at `history[0]`. Evicting 1 element per tick shifted the global tick parity of `history[0]` every single step—making the **exact optimal circuit** score 95% accuracy on even ticks and 5% accuracy on odd ticks!

- **Fix (two stages):** An initial M0 workaround drained sliding windows in multiples of `24` ($\operatorname{lcm}(1, 2, 3, 4, 6, 8)$), which still left prime periods (`5`, `7`, `257`) vulnerable to phase slip and zero-reset `prev_obs` at window boundaries. During Milestone 0 closeout ([milestone_0_closeout_plan.md](milestone_0_closeout_plan.md)), all circuit-search baselines were unified onto `ObservationWindow` (`src/baselines/mod.rs`), which tracks `start_tick` and `prior_frame` across evictions: reactive circuits seed `prev_obs` directly from `prior_frame`, while purely autonomous `Delay` circuits align their initial state from $t = 0$ to `start_tick` via cycle-modular jump (`align_autonomous_state_to_tick`), preserving exact phase for arbitrary prime or composite periods.

### 4.3 Trap 3: The Deceptive "Noisy Echo" Local Minimum and MDL Crossover

The hardest acceptance sub-check in M0 was noisy $k = 2$ `CGP` on `T1_Alternating` (`0, 3, 0, 3, ...`) with $\varepsilon = 0.05$ substitution noise, requiring mean post-transient excess loss $\le 0.02$ bits/symbol over $h_\mu(0.05)$.

Within the first few ticks, CGP reliably discovers the 1-node lag-2 input echo:

```text
BoundaryIn(0) ──► Delay(init = 0) ──► BoundaryOut(0)
```

Because `BoundaryIn(0)` supplies $x(t-1)$, passing it through one `Delay` predicts $\hat{x}(t) = x(t-2)$. On noiseless `0, 3, 0, 3, ...`, $x(t-2) = x(t)$ is 100% accurate! Under $\varepsilon = 0.05$ substitution noise, however, each corrupted symbol causes **two** prediction misses—once when the corruption occurs at tick $t$, and a second time at tick $t + 2$ when the corrupted observation is echoed back ($\approx 9.7$% error rate, excess loss $\approx 0.24$ bits/symbol).

To reach the Bayes-optimal $5.0$% error ceiling ($0$ excess loss), CGP must disconnect from the noisy boundary input and form an **autonomous** 2-register feedback oscillator with explicit `Split` fan-out:

```text
┌─────────────────────────────┐
▼                             │
Delay(init = 0) ──► Delay(init = 3) ──► Split ──► BoundaryOut(0)
```

Three coupled effects initially trapped ~15% of seeds in the noisy echo (`mean_excess = 0.0367 > 0.02`):

| Bottleneck | Why It Happened | How It Was Solved |
| :--- | :--- | :--- |
| **1. MDL sample-size crossover** | The autonomous oscillator adds 2 extra nodes (`Delay(3)` + `Split`, $\approx +26$ model bits). At $\approx 4.7$% fewer errors ($\approx 5.3$ data bits saved per avoided error at $k = 2$), MDL genuinely prefers the smaller reactive echo until $N \gtrsim 130\text{--}180$ ticks! Searching every tick early burned $> 50$% of `TickBudget` *before* MDL even favoured the oscillator. | Introduced two-stage pacing in `CgpBaseline::observe`: fast search up to tick `60` (capped at $\le 50$% budget) so noiseless tasks lock on rapidly, then throttled to every `6` ticks so long noisy runs retain $> 80$% of their compute budget past the MDL crossover ($t \in [150, 400]$). |
| **2. Frozen silent genes under SAM** | Miller's Single-Active Mutation (SAM) mutates silent genes along the way, then *always* mutates at least one active gene. Around a minimal 1-node circuit, single active edits degrade MDL and get rejected—discarding the accompanying silent-gene drift too. | Added `Genotype::drift_inactive` once per generation, mutating one inactive gene directly on `parent_geno` at **zero evaluation cost** (since inactive genes do not alter the lowered `Circuit`). |
| **3. Multi-edge feedback valley** | Converting `Delay(0)` into `Delay(0) <-> Delay(3)` requires configuring an inactive gene (`Delay(3)` reading `Delay(0)`) *and* rewiring `Delay(0)`'s input to close the loop simultaneously. | Added a general, task-agnostic subgraph expansion operator (`Genotype::mutate_recruit_gene`) on one offspring slot that samples an inactive gene over live sources (including self-loops if `Delay`) and wires one valid cycle-safe active consumer to it. |

Together, these three changes brought noisy `T1_Constant` and `T1_Alternating` paired residual regret down from `0.0367` to `< 0.002` bits/symbol across $k \in \lbrace 1, 2 \rbrace$ (while multi-register noisy `T1_PeriodicMotif` and `T1_AffineLag1` remain tracked in `verify-m0` `case_reports` alongside universal probabilistic `CTW` as baseline reference curves for `M1b`–`M2`).

### 4.4 Trap 4: Early-Window MDL Starvation and Tie-Breaking in `ExhaustiveBaseline`

During M0 closeout, expanding `ExhaustiveBaseline` (`src/baselines/exhaustive.rs`) to include `0`-node unconnected-`BoundaryOut` circuits (`circuit_bits = 2.0`), non-zero `Delay { init }` assignments on recurrent loops, and `max_nodes = 4` prime-period benchmarks enlarged the candidate bank ~10× and exposed two subtle traps:

1. **Early-window budget starvation:** On tiny initial windows (`4..20` ticks), a `0`-node circuit (`2.0 + facts_bits`) has strictly lower total MDL than a 2–4 node `0`-error circuit (`27..50` circuit bits). Sweeping the full bank every tick from `t = 4` exhausted `TickBudget` before `window.len()` ever reached the MDL crossover (`~28–60` ticks).
2. **Same-node-count state tie-breaking:** On `T1_Alternating` (`0, 1, 0, 1, ...`), both `BoundaryIn -> Eqz -> BoundaryOut` (`0` `Delay` + `1` `BoundaryIn` = `1` effective register, `11.6` bits) and `BoundaryIn -> Delay -> BoundaryOut` (`1` `Delay` + `1` `BoundaryIn` = `2` effective registers, `12.6` bits) achieve `0` post-transient errors in `1` node. Ordering `Delay` topologies ahead of combinational ones within a fixed node count silently selected the 2-register lag echo over the 1-register state-minimal inverter.

- **Fix:** Sorted each fixed-node-count bucket deterministically by `(circuit_code_bits, effective_state_registers)`, omitted `Sub` at $k = 1$ (identical to `Add` on $\mathbb{Z}_2$), paced full sweeps to informative window horizons (`t = 28, 64, 96, ...`), and added exact MDL branch-and-bound pruning (`if *circ_bits >= best_mdl { continue; }`, valid since `facts_bits >= 0`).

### 4.5 Trap 5: Finite-Sample Channel Variance and Universal Coding Redundancy Under Noise

Evaluating noisy ($\varepsilon = 0.05$) predictors directly against asymptotic entropy rate $h_\mu$ uncovered two statistical effects that must be separated in `Scorecard`:

1. **Single-seed noise-draw variance:** Over $T = 4{,}000$ ticks at $k = 2, \varepsilon = 0.05$, even the exact clean-phase Bayes channel oracle fluctuates around population $h_\mu$ with standard deviation $\sigma \approx 0.0201\text{ bits/symbol}$ solely from binomial corruption counts ($\approx 16$% of optimal runs exceed $+0.02\text{ bits/symbol}$ by channel luck alone). Adding `paired_*_regret_bits_per_symbol`—scoring each predictor against the exact clean-phase Bayes oracle on the *identical* realized noise trajectory—isolates predictor suboptimality from channel luck.
2. **Finite-order HMM + Willems–Shtarkov–Tjalkens (`WST`) redundancy in `CTW`:** A deterministic cycle of period $p = 4$ (`T1_AffineLag1` at $k = 2$) observed through substitution noise is an infinite-order hidden Markov process, and universal context-tree weighting incurs $O(\frac{\lvert \mathcal{S} \rvert \log T}{k T})$ parameter redundancy across multi-period noisy $\mathbb{Z}_4$ branches ($\approx 0.044\text{ bits/symbol}$ at $T = 4{,}000$, converging to $\approx 0.025\text{ bits/symbol}$ at $T = 20{,}000$). Separating distributional `native_prob_log_loss_bits_per_symbol` from point-prediction `prequential_kt_residual_bits_per_symbol` and sizing `CTW` horizons to $T = 20{,}000$ (`context_depth = 9`) kept per-seed paired regret $\le 0.030$ across `20/20` development and `20/20` holdout seeds.

---

## 5. Implications for Milestones M1a–M4

The challenges encountered in M0 carry direct design lessons for the upcoming learning milestones:

1. **Juvenile protection (`T_juvenile`) in M1b must outlast the MDL crossover horizon:** Just as MDL penalised the 3-node autonomous oscillator relative to the 1-node noisy echo on short windows, metabolic energy and structural pruning in M1b/M2 will kill newly sprouted multi-node state loops under noise unless `T_juvenile` spans enough ticks ($\propto \Delta L_{\text{model}} / \Delta \ell_{\text{symbol}}$) for their error reduction to amortise their maintenance and description cost.
2. **Self-looping `Delay` registers are the native constant motif:** Because the Level-0 primitive set $\lbrace \text{ADD}, \text{SUB}, \text{MUL}, \text{EQZ}, \text{SPLIT}, \text{DELAY} \rbrace$ has no dedicated `CONST(c)` primitive, constants $c \in \mathbb{Z}_{2^k}$ arise as self-looping registers `d = Delay(init = c)` fanned out through `Split` (`Split.0 -> d`, `Split.1 -> consumer`). Both M1b sprouting templates and M4b `egg` rewrite rules should recognise and preserve `(Delay(c) <-> Split)` constant subgraphs explicitly.
3. **Sleep replay (M4a) must preserve register phase:** Sliding-window trimming proved that resetting `Delay` registers to static `init` values at arbitrary tick offsets corrupts autonomous attractor phase. When M4a enters offline sleep loopback ($\hat{A}(t+1) \to A(t+1)$), equivalence tests and consolidation rewrites must evaluate from captured live register states (or across full attractor cycles) rather than assuming `t = 0` phase alignment.
4. **Always gate noisy single-seed kill criteria on paired oracle regret + reserved holdout seeds:** Comparing finite noisy runs against population $h_\mu$ conflates learner quality with $O(T^{-1/2})$ channel luck, and tuning evolutionary/plasticity schedules on a single seed bank risks silent overfitting. Milestones `M1a`–`M6` should retain paired clean-state oracle regret alongside population excess over $h_\mu$, and lock gate thresholds prior to evaluating a disjoint holdout seed offset.
