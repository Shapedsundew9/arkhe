# Milestone 1b Implementation Plan: Online Residual Neurogenesis and Spatial Surgery

> **Status:** Pre-registered implementation specification for Stage M1b (Hypothesis **H2**). Stage M1a (Hypothesis **H1**) is complete, locked, and verified across 100 development banks (`20..2019`, $T \in \{1024, 2048, 4096\}$).
>
> **Parent documents:** [milestone_1_plan.md](milestone_1_plan.md) (§4–§9) and [foundational_research_charter_v3.md](foundational_research_charter_v3.md) (§2.5, §3.1.1–§3.1.7, §3.3, §3.4, §4.1, §6 **H2**, §7 M1b, §8, §9, and Appendix A).
>
> **Audience:** Autonomous AI implementation agent executing Stage M1b end-to-end from the current `main` branch through single-shot holdout sign-off (`10000..=10019`).

---

## 1. Purpose, Codebase Audit, and Non-Negotiable Ground Rules

Stage M1a validated that on **fixed circuits**, Arkhe's closed-loop metabolic economy—calibrated sequential point-forecast rewards (`D2`), replacing causal ancestry traces (`D1`), activity recency decay (`D4`), and one-time node description debt $\sum_i \Delta L_i = L(C)$ (`D3`)—separates minimal correct circuits from partial, chance, wrong, silent-stream, and detour-bloated circuits with exact macro-micro conservation $\Delta E(T) = -\Delta L_{\text{null}}(T)$.

Stage **M1b** holds that validated M1a economy fixed and enables **online gradient-free structural learning from an empty spatial lattice**:

1. **Homeostatic first-neuron seeding** (`BoundaryOut(p)` initially unconnected $\to$ first predictive receptor $y_1$).
2. **Residual diagnostics and causal $z^{-1}$ delay recruitment** over a rolling post-juvenile window $W_{\text{diag}}$ (`Miller–Madow` bias-corrected mutual information $\hat{I}_{\text{corr}}(R ; U)$, discrete match gain $\Delta\text{Matches}(U)$, and lagged residual autocorrelation).
3. **Finite-ring cascade-correlation boosting** ($\hat{y}_{m+1}(t) = \hat{y}_m(t) + \hat{r}_m(t) \pmod{2^k}$) via `Primitive::Add` stacking at the output frontier without overwriting $y_1$'s survival.
4. **Autonomous `Delay`-ring closure** under noisy periodic streams ($\varepsilon = 0.05$) when reactive `BoundaryIn` echoes suffer $2\varepsilon(1 - \varepsilon)$ double-error penalties.
5. **Invariant- and phase-preserving pruning** of both metabolically starved nodes ($E_i \le 0$ at or after maturity) and causally idle mature nodes (zero counterfactual sensitivity across $W_{\text{diag}}$).

### 1.1 Exact Starting State of the Repository

Before writing any code, understand what is **already implemented and locked** versus what is **stubbed or missing**:

| Module / File | Current State on `main` | M1b Action Required |
| :--- | :--- | :--- |
| `scripts/build.rs` | **Complete & locked.** Emits `ARKHE_SOURCE_TREE_FNV1A128` and enforces `ARKHE_SUBSTRATE_FLOAT_FREE`. | **Do not modify.** |
| `src/substrate/*` | **Complete & locked.** Exact $\mathbb{Z}_{2^k}$ arithmetic, §2.5 invariants (`SPLIT`-only fan-out, combinational DAGs), `CompiledCircuit`, `ScalarState`. | **Keep 100% float-free (`u64` only).** Only add read-only helpers if strictly needed. |
| `src/rng.rs` | **Complete & locked.** `StreamDomain::SensitivityProbe` (tag `5`) and `StreamDomain::StructuralGrowth` (tag `6`) already exist. | Reuse `StreamDomain::StructuralGrowth` for deterministic tie-breaking if needed; preserve M0 golden vectors. |
| `src/structural/economy.rs` | **M1a complete.** `EconomyConfig`, `RewardMode`, `EconomyState`, `apply_tick`, `apply_causal_tick`, `remap_structure`. | Apply narrow M1b extensions `D1b.2`–`D1b.5` (unclipped juvenile escrow, `split_code_tax`, unconnected-port handling) while keeping all M1a tests green. |
| `src/metrics/point_forecast.rs` | **M1a complete** for `LaplaceFrequency`. | Add `reset_predictor_counts` (`D1b.4`) and matched stratified `RepeatLast` estimator (`D1b.6`). |
| `src/metrics/mdl.rs` | **M1a complete.** `circuit_code_bits`, `circuit_node_description_debts` (panics on $n = 0$ or node-less boundary wires). | Add non-panicking `allocate_circuit_description_bits` (`D1b.7`) and `split_code_tax: bool` pricing toggle (`D1b.3`). Preserve `circuit_node_description_debts` panics for M1a unit tests. |
| `src/structural/lattice.rs` | **Scaffolded.** `GridShape`, `Coord`, `BoundaryAnchor`, `LatticeCircuit`, `route_connect` (BFS with `Split` relays), `remove_nodes`, `lower`. | Add coordinate relocation (`relocate_node`) and frontier tap/stack helpers for `LocalVonNeumann` border cells. |
| `src/structural/surgery.rs` | **Partially implemented.** `EditableLattice`, `edit`, `recompile_preserving_state`, `prune_starved_nodes` (zero-bypass + 1-consumer `Split` collapse). | Add marginal debt ledger tracking, frontier relocation/tapping/stacking surgery, `WellMixed` node-backed boundary routing guard, and `prune_zero_sensitivity_mature_nodes` (`D1b.1`). |
| `src/structural/growth.rs` | **Diagnostics only.** `GrowthConfig`, `diagnose_residual` (entropy, constant offset, Miller–Madow MI, lag 1..=3 MI, `match_gain`). | Implement candidate causal signal pool $\mathcal{U}$, on-demand $z^{-1}$ `Delay` recruitment, micro-motif bank $\mathcal{K}(u_1, u_2)$, `TickBudget`-metered candidate replay, and noisy `Delay`-ring closure. |
| `src/structural/learner.rs` | **STUB!** Currently a 114-line wrapper delegating to `RandomSearchBaseline`. | **Replace completely** with the genuine online closed-loop `ArkheLearner` driving `EditableLattice` + `EconomyState` + `growth` + `surgery`. |
| `src/baselines/mod.rs` | **M0 complete.** `PredictorBaseline`, `TickBudget`, `ObservationWindow`, `run_prequential_harness` (currently passes `None` for `grid_shape` in `compute_mdl`). | Add defaulted `grid_shape(&self) -> Option<GridShape>` and `split_code_tax(&self) -> bool` on `PredictorBaseline` so `Scorecard.mdl` uses exact spatial/ablation code bits. |
| `src/config.rs` | **M0 only.** Rejects non-`WellMixed` topologies at `execute_experiment`; lacks `BaselineConfig::ArkheM1` and `ArkheM1Telemetry`. | Add `BaselineConfig::ArkheM1` (dropping `Eq` on `BaselineConfig` to `PartialEq`), `ArkheM1Telemetry` on `RunReport`, and unlock `LocalVonNeumann` / `LocalMoore` for `ArkheM1`. |
| `src/verify_m1/*` & `src/lib.rs` | **M1a only.** `VerifyM1a` and `check_seed_range_holdout` (`10000..=10019`) exist. | Add `src/verify_m1/m1b.rs` (`G1B.1`–`G1B.4` + §8 ablations) and CLI `VerifyM1b` & `VerifyM1` (enforcing M1a as a hard pre-growth stage gate). Keep `arkhe::greeting() == "arkhe m0"` intact (`tests/cli.rs`). |

### 1.2 Non-Negotiable Engineering and Scientific Rules

1. **Zero Magic-Number Tuning (`GEMINI.md` Hard Rule):**
   > **NEVER, EVER** search for magic numbers. If you find yourself tuning a value to get tests or benchmarks to pass — **STOP immediately & discuss it with the user.**
   All pre-registered defaults (`trace_discount = 0.90`, `initial_energy = 4.0`, `juvenile_ticks = 128`, `max_energy_reserve = Some(25.0)`, `diagnostic_window = 64`, `sprout_interval = 32`, `mi_threshold_bits = 0.05`, `min_match_gain = 4`, `max_nodes = 32`) come directly from [milestone_1_plan.md](milestone_1_plan.md#L463-L476) and **must not be swept or tuned**.
2. **Untouched Holdout Seed Bank (`10000..=10019`):**
   Development, debugging, and ablation verification must use seeds outside `10000..=10019` (standard dev bank: `--seed-offset 0 --seeds 20`; optional extended dev banks: `20..2020`). Do **not** pass `--holdout` until `Step 6` after all code, tests, and documentation are locked.
3. **Zero New Crate Dependencies & Deterministic Collections:**
   Do not modify `[dependencies]` in `Cargo.toml`. Never use `HashMap` or `HashSet` in structural routing, candidate scoring, surgery, or learner state; use `Vec`, `BTreeMap`, and `BTreeSet` with explicit lexicographic tie-breakers so separate OS processes emit byte-identical `RunReport` JSON (`G1B.4`).
4. **Substrate Float Purity:**
   `src/substrate/` must contain zero `f32`/`f64` tokens (`scripts/build.rs` enforces this at compile time and Gate `G1A.1` re-verifies it).

---

## 2. Resolved Post-M1a Economy and Debt Interfaces (`D1b.1`–`D1b.7`)

Stage M1a's redesign replaced per-tick rent with one-time maturity description debt, leaving seven M1b interface items documented in [milestone_1_plan.md](milestone_1_plan.md#L334-L352). All seven are resolved below and **must be implemented exactly as specified** in `Step 1` and `Step 2`.

### 2.1 `D1b.1` — Pruning Idle Mature Nodes via Structural Zero-Sensitivity

Under one-time description debt, a mature node (`juvenile_ticks == 0`, debt paid) that stops contributing causally to outputs or live stateful registers retains $E_i(t) > 0$ forever instead of starving via per-tick rent.

- **Rule:** Track per-node cumulative sensitivity ticks across each post-juvenile diagnostic window $W_{\text{diag}}$. A node $u$ is **causally idle** over $W_{\text{diag}}$ if and only if:
  1. $u$ was mature (`juvenile_ticks == 0`) on **every** tick of $W_{\text{diag}}$, and
  2. On **every** tick of $W_{\text{diag}}$, its counterfactual output sensitivity and register-latch sensitivity were identically zero:
     $$
     \forall p:\; s_{u \to \text{Out}_p}(t) = 0 \quad \text{and} \quad \forall r:\; s_{u \to \text{Latch}_r}(t) = 0.
     $$
- **Action (`prune_zero_sensitivity_mature_nodes`):** At the end of each `W_diag` diagnostic check (before evaluating new sprouts), identify all causally idle mature nodes. Attempt algebraic excision on a staged `LatticeCircuit` (using the same zero-bypass, 1-consumer `Split` collapse, and dead-end upstream sweep as `prune_starved_nodes`). Commit the excision if and only if:
  1. `staged.lower()` and `Circuit::compile()` succeed,
  2. Replaying the staged circuit (with preserved live register values) on the `W_diag` input frame sequence produces **identical output predictions on all ticks of `W_diag`**, and
  3. Total circuit code length does not increase: $L(C_{\text{staged}}) \le L(C_{\text{current}})$.

### 2.2 `D1b.2` — Juvenile Grace Period and Unclipped Juvenile Escrow

> [!WARNING]
> **Critical Trap in Current `EconomyState::apply_tick` (`src/structural/economy.rs:405-409`):**
> On an $8 \times 8$ lattice (`LocalVonNeumann`) or in `WellMixed` ($N \approx 8\text{--}12$), a newly sprouted node's marginal description debt $\Delta L_i$ is typically $17\text{--}28.5\text{ bits}$. With `EconomyConfig::default()` (`initial_energy = 4.0`, `juvenile_ticks = 128`, `max_energy_reserve = Some(25.0)`), `apply_tick` currently clamps `node.energy` to `25.0` on **every juvenile tick** (`0..127`), and then deducts lump-sum $\Delta L_i$ on tick `128` (`juvenile_ticks == 0`). Any sprout with $\Delta L_i \ge 25.0$ is therefore guaranteed to hit $E_i \le 0$ and starve on tick `128` even at `100%` prediction accuracy!

- **Rule:** During juvenile ticks (`node.juvenile_ticks > 0`), positive credit accumulates **unclipped** in escrow so the node can save toward its lump-sum maturity settlement $\Delta L_i$ (negative credit and maintenance remain exempt while `juvenile_ticks > 0`). On the first mature tick (`node.juvenile_ticks == 0`), one-time description debt $\Delta L_i$ is passed in `maintenance_costs[i]`, deducted from `unclipped`, and **`max_energy_reserve` is applied only when `node.juvenile_ticks == 0` (after deducting `maintenance_costs[i]`)**:

  ```rust
  let next_energy = if node.juvenile_ticks > 0 {
      unclipped
  } else {
      self.config
          .max_energy_reserve
          .map_or(unclipped, |cap| unclipped.min(cap))
  };
  ```

- **M1a Compatibility:** M1a sets `juvenile_ticks: 0` and `max_energy_reserve: None`, so every M1a tick has `node.juvenile_ticks == 0` and behavior is bit-for-bit unchanged.

### 2.3 `D1b.3` — Marginal Sprout Description Debt and `split_code_tax` Ablation

Adding new nodes $S_{\text{new}}$ to an existing circuit $C_{\text{old}} \to C_{\text{new}}$ changes Elias-$\gamma$ header bits and (under `WellMixed`) global port-pair wire widths $\log_2(\lvert\text{src}\rvert \cdot \lvert\text{dst}\rvert)$ for all nodes. Re-charging already-mature surviving nodes would violate one-time debt settlement; ignoring header/wire-width growth would leak uncharged MDL bits.

- **Marginal Sprout Debt Rule:**
  - Define $L(\emptyset) = 0.0$ for an empty circuit with `0` nodes and `0` edges (no circuit is transmitted when all outputs are unconnected default `0`).
  - Maintain `node_debts: Vec<f64>` and `debt_paid: Vec<bool>` on the learner alongside `EditableLattice`.
  - When a sprout transaction transforms $C_{\text{old}} \to C_{\text{new}}$ with newly created node indices $S_{\text{new}} \subseteq \{0, \dots, N_{\text{new}} - 1\}$:
    1. Compute total effective code lengths $L_{\text{old}} = L_{\text{eff}}(C_{\text{old}})$ (with $L_{\text{eff}}(\emptyset) = 0$) and $L_{\text{new}} = L_{\text{eff}}(C_{\text{new}})$.
    2. Compute marginal sprout debt:
       $$
       \Delta L_{\text{sprout}} = \max\!\big(0.0,\; L_{\text{new}} - L_{\text{old}}\big).
       $$
    3. Compute raw per-node allocations $d_i$ on $C_{\text{new}}$ via `allocate_circuit_description_bits`. Let $D_{\text{new}} = \sum_{i \in S_{\text{new}}} d_i$. Assign each new node $i \in S_{\text{new}}$ an owed maturity debt:
       $$
       \text{debt}_i = \begin{cases}
       \Delta L_{\text{sprout}} \cdot \dfrac{d_i}{D_{\text{new}}} & \text{if } D_{\text{new}} > 0, \\[6pt]
       \dfrac{\Delta L_{\text{sprout}}}{\lvert S_{\text{new}} \rvert} & \text{otherwise.}
       \end{cases}
       $$
    4. Surviving nodes $j \notin S_{\text{new}}$ keep their existing `debt_j` and `debt_paid[j]` flags unchanged across the remap.
    5. Each node $i$ has `maintenance_costs[i] = debt_i` on the **single** tick where `juvenile_ticks == 0 && !debt_paid[i]`, setting `debt_paid[i] = true` immediately thereafter; on all other ticks `maintenance_costs[i] = 0.0`.
  - **Exact Conservation Invariant:** On any prune-free growth trajectory $\emptyset = C_0 \to C_1 \to \dots \to C_m$ where $L_{\text{eff}}(C_s)$ is monotonically increasing, total maturity maintenance charged across all nodes equals $L_{\text{eff}}(C_m)$ to within $< 10^{-12}\text{ bits}$.
- **`split_code_tax: bool` Toggle (§8 Ablation Axis 2):**
  - Add `pub split_code_tax: bool` to `EconomyConfig` (`default = true` via `#[serde(default = "default_true")]`).
  - Add `circuit_code_bits_with_tax(..., split_code_tax: bool)` and `allocate_circuit_description_bits(..., split_code_tax: bool)` in `src/metrics/mdl.rs`:
    - When `split_code_tax == true` (default): identical to `circuit_code_bits` (`Primitive::Split` pays full palette bits, spatial cell placement bits, and bits for **both** outgoing fan-out wires).
    - When `split_code_tax == false` (ablated): `Primitive::Split` nodes contribute `0.0` operator/placement bits and their **second** outgoing fan-out wire (`port == 1`) contributes `0.0` wire bits (making fan-out structurally free in both candidate MDL selection and node maturity debt).

### 2.4 `D1b.4` — Resetting Port Match Counters Across Upstream Edits & Unconnected Ports

1. **Port `(t, M)` Reset on Cone Change:**
   - When an upstream graph edit (sprout, ring closure, or prune) changes the topological cone driving `BoundaryOut(p)` (detected by comparing canonical lowered cone signature or checking whether `BoundaryOut(p)`'s driver / upstream node set changed), call `scorer.reset_predictor_counts()` on port $p$'s `PointForecastScorer`:
     - Resets predictor accuracy counters (`t = 0`, `matches = 0`, plus `RepeatLast` stratum counters) so a newly stacked corrector is not penalized during its 128-tick juvenile window for hundreds of mismatches committed by an earlier circuit.
     - **Preserves `scorer.null_model`** (`steps`, `counts`, `last_symbol`, `repeat_matches`) unchanged so environmental symbol-frequency calibration is continuous across the entire run.
   - Because $q_t = \max(\hat{q}_t, p_t)$ remains strictly $\mathcal{F}_{t-1}$-measurable after resetting $(t, M) \leftarrow (0, 0)$, Ville's martingale $\mathbb{E}_{s_t \sim p_{\text{null}}}[2^{r_t} \mid \mathcal{F}_{t-1}] = 1$ holds **exact** across edits.
2. **Unconnected `BoundaryOut(p)` Guard (`MissingAncestry` Prevention):**
   - On an empty lattice (or whenever `BoundaryOut(p)` has no driving edge in `Circuit`), `port_ancestry[p]` is all zeros (`total_ancestry == 0.0`). Under `CenteredSurprisal`, `UncenteredSurprisal`, or `RawAccuracy` (used in `G1B.4` Ablation 1), predicting default `0` produces `reward != 0.0`, which causes `EconomyState::apply_tick` to fail with `Err(EconomyError::MissingAncestry { port: p })`!
   - **Rule:** Before scoring port $p$, check whether `BoundaryOut(p)` has a driving edge in `self.circuit`. If **unconnected**:
     - Advance `economy.port_scorer_mut(p).null_model_mut().observe_and_surprisal(facts[p])` directly (keeping null statistics warm), and
     - Set `port_rewards[p] = 0.0` (an empty circuit transmits no prediction and earns/loses zero metabolic flux).

### 2.5 `D1b.5` — Post-Maturity Reserve Cap (`max_energy_reserve`)

Keep `EconomyConfig::default().max_energy_reserve = Some(25.0)` for all online M1b runs. Once a node settles its maturity debt (`juvenile_ticks == 0`), its reserve is capped at `25.0` bits so long-lived early receptors cannot accumulate unbounded reserves across thousands of ticks.

### 2.6 `D1b.6` — Matched Sequential Point-Forecast Estimator for `NullModelKind::RepeatLast`

`PointForecastScorer` currently uses the `LaplaceFrequency` KT prior $\hat{q} = \frac{M + 0.5}{t + 0.5 \cdot 2^k}$ unconditionally, which does **not** match `NullModelKind::RepeatLast` (`src/metrics/null_model.rs:66-84`). Extend `PointForecastScorer` with stratified counts `(rep_t, rep_matches, chg_t, chg_matches)`:

- Let $A = \max(1, 2^k - 1)$ be the number of non-repeat alternatives (`card_f64` when $k = 64$).
- Inspect `self.null_model.last_symbol()` **before** updating `null_model` with `s`:
  1. **Tick $t = 0$ (`last_symbol == None`):** $p_{\text{null}}(\hat{s}_0) = 2^{-k}$; define $\hat{q}_0 = 2^{-k}$, so $q_0 = p_0$ and $r_0 = 0.0$.
  2. **Repeat prediction (`Some(prev)` with $\hat{s}_t = \text{prev}$):**
     $$
     \hat{q}_{\text{rep}} = \frac{m_{\text{rep}} + 0.5}{t_{\text{rep}} + 1.0}.
     $$
     After scoring $r_t$, increment `t_rep += 1` and (if $\hat{s}_t = s_t$) `m_rep += 1`.
  3. **Change prediction (`Some(prev)` with $\hat{s}_t \ne \text{prev}$):**
     $$
     \hat{q}_{\text{chg}} = \frac{m_{\text{chg}} + \dfrac{0.5}{A}}{t_{\text{chg}} + 1.0}.
     $$
     After scoring $r_t$, increment `t_chg += 1` and (if $\hat{s}_t = s_t$) `m_chg += 1`.
- **Exact Algebraic Guarantees (Verify in Unit Tests):**
  - A repeat-last predictor ($\hat{s}_t = s_{t-1}$ for $t \ge 1$) has $t_{\text{rep}} = \text{transitions}$ and $m_{\text{rep}} = \text{repeat\_matches}$, so $\hat{q}_{\text{rep}} \equiv p_{\text{null}}(\hat{s}_t)$ and **$r_t \equiv 0.0$ on every tick of every stream**.
  - Any constant predictor ($\hat{s}_t \equiv c$) on **any** constant stream ($s_t \equiv c$ or $s_t \equiv c' \ne c$) satisfies $\hat{q}_t \equiv p_{\text{null}}(c)$ on both strata ($\frac{0.5 / A}{\tau} \equiv \frac{1 - ((\tau - 1 + 0.5)/\tau)}{A}$ for all $\tau \ge 1$), scoring **$r_t \equiv 0.0$ on every tick**.
  - Conditional expectation $\mathbb{E}_{s_t \sim p_{\text{null}}}[2^{r_t} \mid \mathcal{F}_{t-1}] = 1$ holds identically.

### 2.7 `D1b.7` — Non-Panicking `CircuitDescriptionAllocation` and Node-Backed Boundary Paths

1. In `src/metrics/mdl.rs`, add:

   ```rust
   #[derive(Clone, Debug, PartialEq)]
   pub struct CircuitDescriptionAllocation {
       pub node_debts: Vec<f64>,
       pub unowned_bits: f64,
   }
   ```

   and `pub fn allocate_circuit_description_bits(circuit: &Circuit, topology: WiringTopology, grid_shape: Option<GridShape>, extra_palette_entries: usize, split_code_tax: bool) -> CircuitDescriptionAllocation`:
   - If `circuit.node_count() == 0 && circuit.edges().is_empty()`, return `node_debts: vec![], unowned_bits: 0.0`.
   - If `circuit.node_count() == 0` with direct boundary edges, attribute `header_bits + wiring_bits` to `unowned_bits`.
   - If `circuit.node_count() > 0` and any direct `BoundaryIn -> BoundaryOut` edge exists, attribute that wire's `bit_per_wire` to `unowned_bits` rather than panicking, maintaining `(node_debts.iter().sum::<f64>() + unowned_bits - total_bits).abs() < 1e-12`.
   - Keep `circuit_node_description_debts` unchanged as a wrapper that panics when `n == 0` or `unowned_bits > 0.0` so existing M1a tests (`src/metrics/mdl.rs:417-432`) continue to pass untouched.
2. **Surgical Invariant (`No Node-Less Active Output Wires`):**
   - Neither `growth` nor `surgery` may leave a direct `BoundaryIn(c) -> BoundaryOut(p)` wire with zero intermediate nodes:
     - Under `WellMixed`, if a sprout or zero-bypass connects `BoundaryIn(c)` directly to `BoundaryOut(p)` (`y(t) = s(t-1)`), place a single `Primitive::Split` node at the first free cell so `BoundaryIn(c) -> Split -> BoundaryOut(p)` has `1` metabolic node to own description debt and receive credit/starvation signals.
     - During `prune_starved_nodes` or `prune_zero_sensitivity_mature_nodes`, if collapsing a 1-consumer `Split` or zero-bypassing `Add`/`Sub` would create `BoundaryIn(c) -> BoundaryOut(p)` directly without any node:
       - If the collapsed node was **starved**, disconnect `BoundaryOut(p)` cleanly (reverting to empty/unconnected).
       - If the collapsed node was **alive** (e.g., surviving branch of a starved corrector `Add(y_1, r_hat)` where $y_1$ was `BoundaryIn(c)`), retain a single `Split` node between `BoundaryIn(c)` and `BoundaryOut(p)`.

---

## 3. Spatial Lattice Anchoring, Direct Graph Wiring, and Frontier Surgery (`lattice.rs`, `surgery.rs`)

> [!NOTE]
> **Architectural Decision (M1b) — Removal of 2D Planar Relay Bottleneck:**
> During multi-neuron neurogenesis, treating intermediate empty cells as single-cell `Split` relays under `LocalVonNeumann` nearest-neighbor routing rapidly created spatial congestion around boundary anchors `(0, 3)` and `(0, 4)`. Intermediate relay chains consumed available cells, leading to `RouteUnavailable` routing deadlock and requiring fragile coordinate heuristics.
> The early metaphor of a 2D physical grid/droplet had become an artificial constraint. We have removed this restriction entirely: M1b structural growth uses direct graph wiring (`WiringTopology::WellMixed` as the default developmental architecture), permitting any number of edges directly between endpoints without planar relay hops. Description complexity is rigorously tracked by MDL wire code bits ($L(C)$), enforcing parsimony without spatial gridlock. `LocalVonNeumann` and `LocalMoore` remain preserved as controlled ablations.

### 3.1 Default Perimeter Layout ($8 \times 8$ Grid)

For single-channel (`K = 1`) M1b runs (`GridShape { rows: 8, cols: 8 }` default; generalized across $c \in 0..K$ along the `North` side at offsets `2*c + 3` and `2*c + 4` when `cols >= 2*K + 4`, wrapping clockwise if needed):

- `BoundaryIn(0)` is anchored at `BoundaryAnchor { side: PerimeterSide::North, offset: 3 }` $\implies$ adjacent border cell `(0, 3)`.
- `BoundaryOut(0)` is anchored at `BoundaryAnchor { side: PerimeterSide::North, offset: 4 }` $\implies$ adjacent border cell `(0, 4)`.
- Under `WiringTopology::LocalVonNeumann` + `WireDelayMode::Combinational`, Manhattan step distance between `(0, 3)` and `(0, 4)` is `1`, so a 1–2 node first neuron connects directly along row `0` with zero relay overhead.

### 3.2 Solving Border-Cell Congestion and `BoundaryIn` `SPLIT`-Only Fan-Out

> [!IMPORTANT]
> **Critical Spatial Constraint (`src/substrate/invariants.rs:178` & `src/structural/lattice.rs:744-761`):**
>
> 1. Charter §2.5 enforces `SPLIT`-only fan-out on **all** signal sources—including `BoundaryIn(0)`! Once Epoch 1 wires `BoundaryIn(0) -> node_a`, Epoch 2 **cannot** add a second wire `BoundaryIn(0) -> node_b` without a `Primitive::Split` at cell `(0, 3)`.
> 2. Under `LocalVonNeumann` + `Combinational`, **only** cell `(0, 3)` has distance `1` from `BoundaryIn(0)` at `(North, 3)`, and **only** cell `(0, 4)` has distance `1` to `BoundaryOut(0)` at `(North, 4)`. If Epoch 1 placed `Eqz` at `(0, 3)` or `Add` at `(0, 4)`, Epoch 2 will fail with `LatticeError` unless frontier cells can be surgically vacated while preserving live state!

Implement three deterministic surgical helpers on `LatticeCircuit` / `EditableLattice` (using lexicographic `(row, col)` tie-breaking everywhere):

1. **`relocate_node(node: NodeId, new_coord: Coord) -> Result<(), LatticeError>` on `LatticeCircuit`:**
   - Moves `node` from `old_coord` to unoccupied `new_coord`, keeping its **exact `NodeId`**, `Primitive`, live `Delay` register, and `NodeEconomy`!
   - Re-routes all incident incoming/outgoing connections via `route_connect` (under `Combinational`, splicing local `Split` relays up to `max_relay_hops = 4`; under `TicksPerHop`, updating `hops = step_distance` directly). If any incident route fails, transactionally roll back (`EditableLattice::edit` already clones `staged`).
2. **`tap_source(&mut LatticeCircuit, source: SignalSource, max_relay_hops: u8) -> Result<SignalSource, LatticeError>`:**
   - Returns a usable fan-out `SignalSource` carrying the exact same signal as `source`:
     - If `source` has `0` existing outgoing edges, return `source` directly.
     - If `source` is `NodeOut { node, .. }` where `primitive(node) == Primitive::Split` and one of its ports (`0` or `1`) is free, return that free `NodeOut` port.
     - If `source` already drives an edge `source -> existing_sink` where `existing_sink` is a `Primitive::Split` with a free port (`1`), return that free `Split` port!
     - Otherwise, insert a new `Primitive::Split`:
       - When `source == SignalSource::BoundaryIn(c)` under `LocalVonNeumann`/`LocalMoore` + `Combinational`, the `Split` **must** sit on the border cell `adj` adjacent to `BoundaryIn(c)` (e.g., `(0, 3)`). If `adj` is currently occupied by `occ_node` ($\ne \text{Split}$ with free port), first `relocate_node(occ_node, nearest_empty_neighbor(adj))` (searching `(1, 3)`, `(0, 2)`, `(1, 2)`, ... in BFS order), place `Split` at `adj`, wire `BoundaryIn(c) -> Split.in` and `Split.out0 -> existing_sink` (via `route_connect`), and return `Split.out1`.
       - When `source == SignalSource::NodeOut { node, port }`, place `Split` at the nearest empty cell reachable from `coord(node)` that can still route to `existing_sink` (relocating `existing_sink`'s node by 1 step if adjacent cells are saturated), wire `source -> Split.in` and `Split.out0 -> existing_sink`, and return `Split.out1`.
3. **`stack_output_adder(&mut LatticeCircuit, out_port: u16, corrector_src: SignalSource, max_relay_hops: u8) -> Result<NodeId, LatticeError>`:**
   - Implements $\hat{y}_{m+1} = \hat{y}_m + \hat{r} \pmod{2^k}$ at `BoundaryOut(out_port)` (adjacent border cell `out_cell = (0, 4)`):
     - **Case A (`BoundaryOut(out_port)` currently unconnected):** Route `corrector_src` directly to `BoundaryOut(out_port)` via `route_connect` (placing a 1-node `Split` at `out_cell` if `corrector_src` is `BoundaryIn` under `WellMixed`, per `D1b.7`).
     - **Case B (`out_cell` currently holds a 1-consumer relay `Primitive::Split` whose sole consumer is `BoundaryOut(out_port)`):** Replace/upgrade that relay `Split` with `Primitive::Add` at `out_cell` (`Add.in0 <- old_split_driver`, `Add.in1 <- corrector_src` via `route_connect`, `Add.out -> BoundaryOut(out_port)`), eliminating 1 relay node for free!
     - **Case C (`out_cell` is occupied by an active operator `prev_driver` driving `BoundaryOut(out_port)`):** Relocate `prev_driver` to the nearest empty interior neighbor of `out_cell` (e.g., `(1, 4)` or `(0, 5)` via `relocate_node`, disconnecting its old `BoundaryOut` wire first so `prev_driver` no longer needs distance-1 access to `BoundaryOut`), place `Primitive::Add` at `out_cell`, wire `prev_driver -> Add.in0` (distance `1`!), route `corrector_src -> Add.in1` via `tap_source` + `route_connect`, and wire `Add.out -> BoundaryOut(out_port)`.

---

## 4. Residual Diagnostics, Causal $z^{-1}$ Recruitment, Motif Bank $\mathcal{K}$, and Noisy Loop Closure (`growth.rs`)

### 4.1 Post-Juvenile Rolling Diagnostic Trigger

Maintain a rolling post-juvenile trace buffer of length $W_{\text{diag}}$ (`GrowthConfig::default().diagnostic_window = 64`):

- **Maturity Gate:** A tick $t$ is eligible to append to the post-juvenile diagnostic buffer **only when every currently live node in `EditableLattice` has `juvenile_ticks == 0`** (or when the circuit has `0` nodes). Whenever any graph edit (sprout, ring closure, or prune) modifies the circuit, clear the post-juvenile diagnostic buffer and reset `ticks_since_last_sprout_check = 0` so diagnostics always measure the settled post-edit predictor $\hat{y}_m$.
- **Cadence:** Once `post_juvenile_buffer.len() == W_diag` and `ticks_since_last_sprout_check >= sprout_interval` (`32`), run:
  1. `prune_zero_sensitivity_mature_nodes` (`D1b.1`). If any node was pruned, clear buffer and return.
  2. Residual diagnostics `diagnose_residual` on channel $p$'s residual trace $r(t) = (s_p(t) - \hat{y}_p(t)) \bmod 2^k$ over the `W_diag` window.
  3. **Zero-error fast exit:** If `diagnostics.zero_error` is `true`, do nothing!
  4. **Noisy periodic loop-closure check (§4.5):** If the circuit already uses `BoundaryIn` and `lagged_mutual_information_bits` or period-$p$ residual autocorrelation indicates reactive echo double-error noise, evaluate autonomous `Delay`-ring closure (§4.5).
  5. **Corrector sprouting (§4.2–§4.3):** If `diagnostics.growth_criterion_met` and `circuit.node_count() < config.max_nodes`, evaluate the candidate motif bank $\mathcal{K}(u_1, u_2)$ metered against `TickBudget`.

### 4.2 Candidate Causal Signal Pool $\mathcal{U}$ (Hop-Latency Aware)

Over the `W_diag` post-juvenile ticks, construct `Vec<ResidualFeature>` in deterministic order:

1. **Direct boundary inputs:** `in:c` for each channel $c \in 0..K$, carrying `inputs[t][c] = s_c(t - 1)`.
2. **Live internal node outputs:** `node:{id}:{port}` for every live non-relay node (and `Split` port `0`) within spatial routing reach (`manhattan_dist(coord, out_cell) <= max_relay_hops + 1` under `LocalVonNeumann`, Chebyshev under `LocalMoore`, all nodes under `WellMixed`), recording the exact value emitted on tick $t$.
3. **Virtual causal $z^{-\tau}$ delay taps ($\tau \in \{1, 2, 3\}$):**
   - `lag_in:{c}:{tau}` carrying $s_c(t - 1 - \tau)$ (materializable on demand via a chain of $\tau$ `Primitive::Delay { init: 0 }` nodes tapped from `BoundaryIn(c)`).
   - `lag_yhat:{p}:{tau}` carrying $\hat{y}_p(t - \tau)$ when `BoundaryOut(p)` is currently driven (materializable via $\tau$ `Primitive::Delay { init: 0 }` nodes tapped from the current output driver $y_m$).
   - `lag_res:{p}:{tau}` carrying $r_p(t - \tau) = (s_p(t - \tau) - \hat{y}_p(t - \tau)) \bmod 2^k$ (for $\tau = 1$, materializable directly from Level-0 primitives as `Sub(BoundaryIn(p), Delay(y_m))` because `BoundaryIn(p)` at tick $t$ **is** $s_p(t - 1)$!).
4. **`WireDelayMode::TicksPerHop` Lag Alignment:**
   - Under `Combinational`, a candidate placed at distance $d$ from `out_cell` affects `BoundaryOut(p)` on the **same** tick ($\Delta_{\text{hop}} = 0$).
   - Under `TicksPerHop`, a wire of Manhattan/Chebyshev distance $d \ge 1$ introduces $d$ synchronous hop registers! Therefore, when scoring features in `diagnose_residual`, use raw causal traces, and during **candidate replay (§4.3)** evaluate the actual lowered + compiled `EditableLattice` candidate on `ObservationWindow` so `TicksPerHop` pipeline latencies are scored with **100% physical fidelity** (never approximated).

### 4.3 Micro-Motif Bank $\mathcal{K}(u_1, u_2)$, Homeostatic Epoch-1 Priority, and Silent-Stream Guard

From `ResidualDiagnostics`:

- Extract `u_1` (the primary feature from `best_feature`, defaulting to `BoundaryIn(0)` if only `constant_offset` triggered) and optional `u_2` (the second feature if `best_feature` is a pair `[i, j]`, or the highest-scoring singleton distinct from `u_1`).
- **On-Demand $z^{-\tau}$ Materialization:** When a candidate motif references a virtual `lag_*` feature, its surgical builder first taps the underlying live source via `tap_source` and places the required `Delay { init: 0 }` chain on adjacent empty cells before wiring the motif operator(s).
- **Fixed Level-0 Candidate Motif Bank $\mathcal{K}(u_1, u_2)$ ($\le 24$ motifs, evaluated in fixed canonical order):**
  1. **1-Primitive Unary / Direct Receptors:**
     - `Identity(u_1)` ($u_1$)
     - `Eqz(u_1)` ($\operatorname{EQZ}(u_1)$)
     - If $u_2$ is present and distinct: `Identity(u_2)`, `Eqz(u_2)`
  2. **2-Primitive Binary & Self-Interaction Motifs:**
     - `AddSelf(u_1)` ($u_1 + u_1 \equiv 2 u_1 \pmod{2^k}$, built via `Split(u_1) -> Add`, active when $k > 1$)
     - `AddPair(u_1, u_2)`, `SubPair(u_1, u_2)`, `SubPair(u_2, u_1)`, `MulPair(u_1, u_2)`
  3. **Constant-Loop & Shifted/Scaled Motifs ($c \in \{1, \dots, \min(2^k - 1, 3)\}$, plus `constant_offset` $c^*$ if distinct):**
     - `ConstLoop(c)`: `Delay { init: c } <-> Split` emitting constant $c$
     - `AddConst(u_1, c)`: `Add(u_1, ConstLoop(c))`
     - `EqzAddConst(u_1, c)`: `Eqz(Add(u_1, ConstLoop(c)))` (for $k > 1$)
     - `Neg(u_1)`: `Sub(ConstLoop(0_or_self_cancel), u_1)` — specifically `Split(u_1)` + `Sub(u_1_zero, u_1)` or `Mul(u_1, ConstLoop(2^k - 1))` (at $k = 2$, $3 \cdot u_1 \equiv -u_1 \pmod 4$)
     - `ScaleEqz(u_1, c)`: `Mul(Eqz(u_1), ConstLoop(c))` (for $k > 1, c > 1$)
  4. **2–3 Primitive Nonlinear Interaction Motifs:**
     - `AddEqz(u_1, u_2)`: $u_1 + \operatorname{EQZ}(u_2)$ and $u_2 + \operatorname{EQZ}(u_1)$
     - `MulEqz(u_1, u_2)`: $u_1 \cdot \operatorname{EQZ}(u_2)$ and $\operatorname{EQZ}(u_1) \cdot \operatorname{EQZ}(u_2)$

- **Candidate Evaluation Protocol (`TickBudget`-Metered Replay on `ObservationWindow`):**
  For each motif candidate in $\mathcal{K}(u_1, u_2)$ whose node count fits within `config.max_nodes`:
  1. Clone a staged `LatticeCircuit` from `self.editable.lattice()`, materialize any virtual delay tap + motif + `stack_output_adder`, and call `staged.lower()` and `Circuit::compile()`. Skip cleanly on `Err` (e.g., spatial congestion or `max_nodes` exceeded).
  2. Replay the compiled candidate over the most recent `W_replay = ObservationWindow` slice (`min(window.len(), 2 * W_diag)` ticks, aligning registers to `slice_start_tick`):
     - **Before** stepping `W_replay` ticks, call `budget.try_consume(W_replay as u64)`. If `false` (budget exhausted), **stop candidate evaluation immediately** and leave the current circuit unchanged (`G1B.3` budget invariant!).
  3. Measure over the trailing `W_diag` ticks of that replay:
     - Candidate mismatches $e_{\text{cand}}$ vs. current circuit mismatches $e_{\text{curr}}$ (`match_improvement = e_curr - e_cand`).
     - Simulated prequential net reward flux gain $\Delta \text{Flux} = \text{Flux}_{\text{cand}}(W_{\text{diag}}) - \text{Flux}_{\text{curr}}(W_{\text{diag}})$ under the learner's active `RewardMode` and warm `NullModel` snapshot (with a fresh predictor `(t, M) = (0, 0)` on the candidate port, matching `D1b.4`).
     - Total local MDL objective on `W_diag`:
       $$
       J(\text{cand}) = L_{\text{eff}}(C_{\text{cand}}) + \text{facts\_code\_bits}(k, W_{\text{diag}}, e_{\text{cand}}).
       $$
  4. **Strict Acceptance Guards (Preventing Silent-Stream Thrashing & Overfitting):**
     A candidate is admissible **only if all three hold**:
     - `e_cand + config.min_match_gain <= e_curr` (achieves at least `min_match_gain = 4` fewer errors on `W_diag`),
     - $\Delta \text{Flux} > 0.0$ (**Silent-Stream Guard:** on `T1_Constant(c)` for *any* $c \in \mathbb{Z}_{2^k}$, `CalibratedPointForecast` scores `ConstLoop(c)` at $\text{Flux}_{\text{cand}} \equiv 0.0 = \text{Flux}_{\text{curr}}$, so $\Delta \text{Flux} = 0.0 \not> 0.0$ and **zero nodes ever sprout on constant streams**!), and
     - Projected juvenile survival check: $\text{initial\_energy} + \frac{T_{\text{juvenile}}}{W_{\text{diag}}} \cdot \text{Flux}_{\text{cand}}(W_{\text{diag}}) > \Delta L_{\text{sprout}}$ (when `juvenile_ticks > 0`), preventing doomed high-debt bloat from thrashing.
  5. **Epoch-1 Homeostatic Receptor Selection vs. Epoch $\ge 2$ Corrector Selection (Charter §3.3):**
     - **Epoch 1 (`BoundaryOut(p)` currently unconnected):** Following Charter §3.3 (*"First neuron — homeostatic seeding"*), if one or more **single-operator reactive receptors** ($\Delta N_{\text{non-split}} = 1$, e.g., `Identity(in:0)` or `Eqz(in:0)`) pass all acceptance guards above, select the admissible 1-operator receptor minimizing $J(\text{cand})$ (breaking ties by higher $\Delta \text{Flux}$, then canonical motif index). Otherwise, select the admissible motif minimizing $J(\text{cand})$ across the full bank.
     - **Epoch $\ge 2$ (`BoundaryOut(p)` already driven):** Select the admissible motif minimizing $J(\text{cand})$ across the full bank (requiring $J(\text{cand}) < J(\text{curr})$ or $e_{\text{cand}} = 0$ with positive $\Delta\text{Flux}$).

### 4.4 Exact Verification of Two-Stage Trajectories on All Four `G1B.1` Tasks

With the Epoch-1 1-operator receptor rule and motif bank $\mathcal{K}(u_1, u_2)$ above, verify that **every single `G1B.1` task** executes a clean two-stage neurogenesis trajectory ($L_0 > L_1 > L_2$ and $|(H(R_1) - H(R_2)) - \hat{I}(R_1 ; U^*)| \le 0.05$):

| `G1B.1` Benchmark | Width $k$ | Epoch 0 ($C_0 = \emptyset$) | Epoch 1 First Neuron $y_1$ | Residual $R_1 = s - y_1 \pmod{2^k}$ | Epoch 2 Stacked Corrector $\hat{r}_1$ | Post-Epoch-2 Predictor $\hat{y}_2 = y_1 + \hat{r}_1$ |
| :--- | :---: | :--- | :--- | :--- | :--- | :--- |
| `T1_PeriodicMotif([0, 1, 1])` | $1$ | $66.7\%$ err | $y_1 = \operatorname{EQZ}(x)$ ($33.3\%$ err, survives debt) | Cycle `(0, 0, 1)`, $H(R_1) = 0.9183\text{ bits}$ | $z^{-1}$ tap $\hat{r}_1 = \operatorname{Delay}(y_1)$ ($\hat{I}(R_1 ; y_1(t-1)) = 0.9183$) | $\operatorname{EQZ}(x) + \operatorname{Delay}(\operatorname{EQZ}(x)) \pmod 2 \implies$ **`0.0%` err** ($H(R_2) = 0$) |
| `T1_PeriodicMotif([0, 1, 3])` | $2$ | $66.7\%$ err | $y_1 = \operatorname{EQZ}(x)$ ($33.3\%$ err, $+0.138\text{ b/tick}$) | Cycle `(0, 0, 3)`, $H(R_1) = 0.9183\text{ bits}$ | $\hat{r}_1 = 3 \cdot \operatorname{Delay}(y_1)$ or $\operatorname{Sub}(0, \operatorname{Delay}(y_1))$ ($\hat{I} = 0.9183$) | $\operatorname{EQZ}(x) - \operatorname{Delay}(\operatorname{EQZ}(x)) \pmod 4 \implies$ **`0.0%` err** ($H(R_2) = 0$) |
| `T1_AffineLag1(a=1, b=1)` | $2$ | $75.0\%$ err | $y_1 = \operatorname{EQZ}(x)$ ($50.0\%$ err: matches $0 \to 1$ & $3 \to 0$) | Cycle `(0, 2, 3, 0)`, $H(R_1) = 1.5000\text{ bits}$ | $\hat{r}_1 = x + \operatorname{EQZ}(y_1)$ ($\hat{I}(R_1 ; (x, y_1)) = 1.5000$) | $\operatorname{EQZ}(x) + x + \operatorname{EQZ}(\operatorname{EQZ}(x)) \equiv x + 1 \pmod 4 \implies$ **`0.0%` err** ($0$ `Delay`s!) |
| `T3b_CounterModN(3)` | $2$ | $66.7\%$ err | $y_1 = \operatorname{EQZ}(x)$ ($33.3\%$ err: matches $0 \to 1$ & $2 \to 0$) | Cycle `(0, 2, 0)`, $H(R_1) = 0.9183\text{ bits}$ | $\hat{r}_1 = x + x \pmod 4$ (`AddSelf(x)`, $\hat{I}(R_1 ; x) = 0.9183$) | $\operatorname{EQZ}(x) + 2x \equiv (x + 1) \bmod 3$ on $\{0,1,2\} \implies$ **`0.0%` err** in 4 nodes! |

> [!NOTE]
> Look closely at `T1_AffineLag1` ($k = 2$) and `T3b_CounterModN(3)` ($k = 2$) in the table above:
>
> - On $\mathbb{Z}_4$, $\operatorname{EQZ}(0) + 0 + \operatorname{EQZ}(1) = 1 + 0 + 0 = 1$, and for $x \in \{1, 2, 3\}$, $\operatorname{EQZ}(x) + x + \operatorname{EQZ}(0) = 0 + x + 1 \equiv x + 1 \pmod 4$! Thus `T1_AffineLag1` reaches $x + 1 \pmod 4$ in two purely combinational stages with **zero constant-loop `Delay` registers** ($C_{\mu}^{\text{internal}} = 0$).
> - On $\{0, 1, 2\} \subset \mathbb{Z}_4$: for $x = 0$, $1 + 0 = 1$; for $x = 1$, $0 + 2 = 2$; for $x = 2$, $0 + 4 \equiv 0 \pmod 4$! Thus $\operatorname{EQZ}(x) + 2x \pmod 4$ computes $(x + 1) \bmod 3$ **exactly** in 4 combinational nodes (compared to 8 nodes in `Counter3Opt`)!

### 4.5 Autonomous `Delay`-Ring Closure Under Channel Noise (`G1B.2` Noisy Dominance)

Under memoryless substitution noise $\varepsilon = 0.05$ (`IndependentSubstitution`), a reactive circuit reading `BoundaryIn(0)` ($= \tilde{s}(t - 1)$) on a periodic `T1` task (`T1_Alternating`, `T1_PeriodicMotif`) makes an error both when tick $t$ is corrupted **and** when tick $t - 1$ was corrupted, suffering error rate $\approx 2\varepsilon(1 - \varepsilon) = 9.5\%$ (`paired_residual_regret_bits_per_symbol` $\approx 0.167 > 0.025$!). Crucially, this double-error leaves a **detectable negative lag-$p$ residual signature**: when $\tilde{s}(t - 1)$ was corrupted by $+e$, $\hat{y}(t)$ is perturbed, creating correlated non-zero residuals across lags $1..=6$.

Implement `propose_autonomous_delay_ring` inside `src/structural/growth.rs`:

1. **Trigger Condition (No Oracle $\varepsilon$ Required):**
   Evaluate autonomous `Delay`-ring candidates of period $p \in 2..=6$ only when:
   - `!diagnostics.zero_error` (noiseless already-exact circuits have `zero_error == true` and **never** replace a minimal reactive circuit with a ring!),
   - `compiled.uses_boundary_inputs()` is `true` (current predictor is reactive), and
   - `ObservationWindow.len() >= 2 * W_diag` (`128` frames available to distinguish persistent periodicity from short transients).
2. **Phase-Aligned Ring Construction (`Osc_p`):**
   For each period $p \in 2..=6$ (`p + 1 <= max_nodes`):
   - Estimate the empirical clean phase template $\mu(0), \dots, \mu(p - 1) \in \mathbb{Z}_{2^k}$ over the buffered `ObservationWindow` (`frames[idx]` at global tick `t = start_tick + idx`) by taking the plurality (mode) symbol in each residue class $t \bmod p$ (lexicographically lowest symbol on ties). Skip if $\mu$ is constant (handled by empty circuit).
   - Build a staged `LatticeCircuit` containing only the $p$-stage autonomous ring $D_0 \to D_1 \to \dots \to D_{p-1} \to S$, with $S.\text{out}_0 \to D_0$ (routed locally via `route_connect`) and $S.\text{out}_1 \to \text{BoundaryOut}(0)$:
     - **Static `init` alignment (for `best_circuit()` at $t = 0$):** Under `Combinational` (`hop_sum = 0`), set `D_{p - 1 - j}`'s `Primitive::Delay { init }` to $\mu(j \bmod p)$ for $j = 0..p$. (Under `TicksPerHop`, let $P_{\text{eff}}$ be the total registers around the cycle; only construct when $P_{\text{eff}} = p$, assigning the $p$ register `init`s around the cycle to match $\mu(0..p)$.)
     - **Live register alignment (at next tick $t_{\text{next}}$):** Set live register state of `D_{p - 1 - j}` to $\mu((t_{\text{next}} + j) \bmod p)$.
3. **`TickBudget`-Metered Selection:**
   - Deduct `W_replay` ticks from `TickBudget` via `budget.try_consume(W_replay as u64)` for each candidate $p$.
   - Accept the best period $p^*$ replacing the reactive subgraph if and only if on `W_replay` (`128` ticks):
     - Mismatches strictly drop by at least `config.min_match_gain` (`e_ring + min_match_gain <= e_curr`),
     - Simulated prequential `CalibratedPointForecast` flux strictly improves ($\text{Flux}_{\text{ring}} > \text{Flux}_{\text{curr}}$), and
     - Replay MDL objective strictly improves: $L_{\text{eff}}(C_{\text{ring}}) + \text{facts\_code\_bits}(k, W_{\text{replay}}, e_{\text{ring}}) < L_{\text{eff}}(C_{\text{curr}}) + \text{facts\_code\_bits}(k, W_{\text{replay}}, e_{\text{curr}})$.
   - Once installed, the autonomous ring is immune to `BoundaryIn` corruptions, achieving mismatch rate $\approx \varepsilon = 5\%$ (`paired_residual_regret_bits_per_symbol <= 0.025`), and subsequent `diagnose_residual` checks see i.i.d. substitution noise ($\Delta\text{Matches}(U) < 4$, $\hat{I}_{\text{corr}} \le 0$), **halting all further growth at the noise floor $h_\mu(\varepsilon)$**!

---

## 5. `ArkheLearner` Online Wake Loop and Harness Integration (`learner.rs`, `config.rs`)

### 5.1 `ArkheLearner` State and Per-Tick Protocol (`src/structural/learner.rs`)

Replace the `RandomSearchBaseline` stub in `src/structural/learner.rs` entirely. `ArkheLearner` holds:

- `editable: EditableLattice` (initialized on an **empty** `LatticeCircuit` with `0` nodes and `0` edges, lowered and compiled cleanly),
- `node_debts: Vec<f64>` and `debt_paid: Vec<bool>` (initially empty `vec![]`),
- `last_committed_code_bits: f64` (`0.0` at $t = 0$),
- `economy_config: EconomyConfig`, `growth_config: GrowthConfig`, `seed_bank: SeedBank`, `sample_count: usize` (`8`),
- `tick: u64` (`0`), `prev_observed_frame: Vec<u64>` (`vec![0; channels]`), `pending_prediction: Option<(Vec<u64>, CausalAncestry, SensitivitySnapshot)>`,
- `obs_window: ObservationWindow` (capacity `2 * growth_config.diagnostic_window`, i.e., `128`),
- `post_juvenile_diag: DiagBuffer` (rolling `W_diag` traces of residuals, boundary inputs, internal wire outputs, predictions, and per-node zero-sensitivity flags),
- Telemetry counters (`sprout_events: u64`, `starvation_prune_events: u64`, `idle_prune_events: u64`, `ring_closure_events: u64`, `epoch_snapshots: Vec<GrowthEpochSnapshot>`).

#### Exact Two-Phase Step (`PredictorBaseline` Contract)

1. **`predict(&mut self, _budget: &mut TickBudget) -> Vec<u64>` (Causal Phase at tick `t`):**
   - Feed `inputs = &self.prev_observed_frame` (`[0; K]` at $t = 0$; `y(t - 1)` for $t \ge 1$, matching `ObservationWindow` and M0 reactive semantics).
   - Step `self.editable.state_mut().step(self.editable.compiled(), inputs, &mut predictions)` (**not** billed against `TickBudget`: live wake execution of the single active organism is O(1) online inference; `TickBudget` meters internal candidate replay evaluations during sprouting).
   - Record node output values via `economy_mut().record_activity(&node_outputs)`.
   - Compute `snapshot = counterfactual_sensitivity(...)` (`seed_bank`, `tick`, `sample_count = 8`) and `ancestry = assemble_causal_ancestry(...)`.
   - Cache `(predictions.clone(), ancestry, snapshot)` in `self.pending_prediction` and return `predictions`.

2. **`observe(&mut self, truth: &[u64], budget: &mut TickBudget) -> Option<f64>` (Settlement, Pruning & Growth Phase at tick `t`):**
   - Take `(predictions, ancestry, snapshot)` from `self.pending_prediction`.
   - Push `truth` into `self.obs_window.push_frame(truth)` and update `self.prev_observed_frame.copy_from_slice(truth)`.
   - **Port Reward Scoring (`D1b.4` unconnected guard):**
     - For each output port $p \in 0..K$: if `BoundaryOut(p)` has a driving edge in `self.editable.circuit()`, compute `port_rewards[p]` via `economy_mut().score_port_single(p, predictions[p], truth[p])`; otherwise advance `economy_mut().port_scorer_mut(p).null_model_mut().observe_and_surprisal(truth[p])` and set `port_rewards[p] = 0.0`.
   - **Maturity Debt Settlement (`D1b.2` & `D1b.3`):**
     - Build `maintenance_costs` of length `N`: for each node $i$, if `economy.nodes()[i].juvenile_ticks == 0 && !self.debt_paid[i]`, set `maintenance_costs[i] = self.node_debts[i]` and mark `self.debt_paid[i] = true`; else `0.0`.
     - Call `econ_tick = self.editable.economy_mut().apply_causal_tick(&port_rewards, &ancestry, &maintenance_costs)`.
   - **Starvation Pruning (`econ_tick.starved_nodes` non-empty):**
     - Call `editable.prune_starved_nodes(&econ_tick.starved_nodes)` (enforcing `D1b.7` node-backed boundary rule), remap `node_debts` & `debt_paid`, reset port predictor counts (`D1b.4`) on affected output ports, update `last_committed_code_bits = effective_code_bits()`, increment `starvation_prune_events`, and clear `post_juvenile_diag`.
   - **Post-Juvenile Diagnostic Buffer & Sprouting (`all juvenile_ticks == 0`):**
     - Append current tick's residual `(truth[p] - predictions[p]) mod 2^k`, feature values, and `snapshot` sensitivity flags to `post_juvenile_diag`.
     - When `post_juvenile_diag.len() == W_diag` and `ticks_since_check >= sprout_interval`:
       1. Run `D1b.1` `prune_zero_sensitivity_mature_nodes` (if edited: remap `node_debts`/`debt_paid`, reset affected port `(t, M)`, clear `post_juvenile_diag`, increment `idle_prune_events`).
       2. Otherwise run §4.5 autonomous ring-closure check and §4.3 corrector sprouting metered by `budget: &mut TickBudget`. If a sprout or ring closure commits: assign marginal sprout debt $\Delta L_{\text{sprout}}$ (`D1b.3`) to newly added nodes (`debt_paid = false`), update `last_committed_code_bits`, reset port `(t, M)` (`D1b.4`), clear `post_juvenile_diag`, and increment `sprout_events` (or `ring_closure_events`).
   - Increment `self.tick += 1` and return `None` (deterministic circuit learner; harness computes KT residual loss & `Scorecard.mdl` automatically, or optionally expose prequential forecast bits in `ArkheM1Telemetry`).

### 5.2 Configuration Schema, `ArkheM1Telemetry`, and Spatial Topology Unlock (`src/config.rs`)

1. **`BaselineConfig::ArkheM1`:**
   - Change `#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]` on `BaselineConfig` (`src/config.rs:160`) to `#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]` (because `EconomyConfig` and `GrowthConfig` contain `f64`).
   - Add variant matching [milestone_1_plan.md](milestone_1_plan.md#L458-L477):

     ```rust
     ArkheM1 {
         #[serde(default = "default_arkhe_grid_dim")]
         grid_rows: u16,
         #[serde(default = "default_arkhe_grid_dim")]
         grid_cols: u16,
         #[serde(default = "default_max_relay_hops")]
         max_relay_hops: u8,
         #[serde(default)]
         economy: EconomyConfig,
         #[serde(default)]
         growth: GrowthConfig,
     }
     ```

     with `default_arkhe_grid_dim() -> u16 { 8 }` and `default_max_relay_hops() -> u8 { 4 }`.
2. **Unlock Spatial Topologies in `execute_experiment` (`src/config.rs:406-412`):**
   - Keep rejecting non-`WellMixed` topologies with `ConfigError::UnsupportedTopology` **only** when `!matches!(config.baseline, BaselineConfig::ArkheM1 { .. })`.
   - For `BaselineConfig::ArkheM1`, support all three `WiringTopology` modes (`LocalVonNeumann`, `LocalMoore`, `WellMixed`) and both `WireDelayMode` modes (`Combinational`, `TicksPerHop`)!
   - Implement `PredictorBaseline::grid_shape(&self) -> Option<GridShape>` and `PredictorBaseline::split_code_tax(&self) -> bool` on `ArkheLearner` and pass them into `compute_mdl` inside `run_prequential_harness` (`src/baselines/mod.rs:500-510`) so `Scorecard.mdl.circuit_bits` uses the exact spatial grid dimensions and `split_code_tax` flag.
3. **`ArkheM1Telemetry` on `RunReport`:**
   - Add `#[serde(default, skip_serializing_if = "Option::is_none")] pub m1_telemetry: Option<ArkheM1Telemetry>` to `RunReport` (`None` for all M0 baselines so M0 `RunReport` schema and tests remain 100% compatible):

     ```rust
     #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
     pub struct ArkheM1Telemetry {
         pub grid_rows: u16,
         pub grid_cols: u16,
         pub final_node_count: usize,
         pub final_split_count: usize,
         pub final_delay_count: usize,
         pub final_total_energy: f64,
         pub min_mature_node_energy: Option<f64>,
         pub cumulative_net_flux: f64,
         pub cumulative_maturity_debt_paid: f64,
         pub null_calibrated_mdl_delta_bits: f64,
         pub sprout_events: u64,
         pub ring_closure_events: u64,
         pub starvation_prune_events: u64,
         pub idle_prune_events: u64,
     }
     ```

---

## 6. Pre-Registered M1b Acceptance Gates (`G1B.1`–`G1B.4`), §8 Ablations, and CLI

Create `src/verify_m1/m1b.rs` and re-export from `src/verify_m1/mod.rs`. Enforce `check_seed_range_holdout(seed_offset, seeds, allow_holdout)` at the top of both `verify-m1b` and `verify-m1`.

### 6.1 Quantitative Gate Specifications

| Gate | Exact Pre-Registered Protocol & Quantitative Pass Criteria |
| :--- | :--- |
| **`G1B.1` — Stepwise first-to-second neuron reduction** | Run `ArkheLearner` (`WellMixed` direct wiring default, or `LocalVonNeumann` 8x8 ablation, `Combinational`, $\varepsilon = 0.0$, $T = 2000$, `max_eval_ticks = 50_000`) across `seeds` (`20` default) on all 4 tasks: `T1_PeriodicMotif([0, 1, 1])` ($k = 1$), `T1_PeriodicMotif([0, 1, 3])` ($k = 2$), `T1_AffineLag1 { mul_a: 1, add_b: 1, init: 0 }` ($k = 2$), and `T3b_CounterModN { modulus: 3, init: 0 }` ($k = 2$). Record prequential KT residual loss (`facts_code_bits / W`, $W = 128$) and empirical Shannon residual entropy $H(R_e)$ over the `W_diag = 64` window immediately **before** Sprout 1 ($L_0, H(R_0)$), after Sprout 1 settles ($L_1, H(R_1)$, plus Sprout 2's diagnostic plug-in $\hat{I}(R_1 ; U^*)$), and after Sprout 2 settles ($L_2, H(R_2)$). **Pass:** On $\ge \lceil 0.95 \cdot \text{seeds} \rceil$ ($\ge 19/20$) seeds for each of the 4 tasks: (1) $L_1 < L_0$, (2) $L_2 < L_1$, (3) Sprout 1 survives (`starvation_prune_events == 0` on $y_1$'s core operator), and (4) $\lvert (H(R_1) - H(R_2)) - \hat{I}(R_1 ; U^*) \rvert \le 0.05\text{ bits/symbol}$. |
| **`G1B.2` — `T1` suite convergence & noise-floor halting** | Run `ArkheLearner` (`WellMixed` direct wiring default, or `LocalVonNeumann` 8x8 ablation, `Combinational`, $T = 2000$, `warmup_ticks = 500`, `max_eval_ticks = 50_000`) across `seeds` at $k \in \{1, 2\}$: **(a) Noiseless ($\varepsilon = 0.0$):** On `T1_Constant(0)` and `T1_Constant(2^k - 1)`, `final_node_count == 0` on **100%** of runs; on `T1_Alternating(0, 2^k - 1)`, `T1_PeriodicMotif([0,1,1] / [0,1,3])`, and `T1_AffineLag1(1, 1, 0)`, `post_transient_clean_mismatches == 0` on $\ge \lceil 0.90 \cdot \text{seeds} \rceil$ ($\ge 18/20$) seeds. **(b) Noisy ($\varepsilon = 0.05$, `IndependentSubstitution`):** On `T1_Constant(0)` and `T1_Alternating(0, 2^k - 1)`, `paired_residual_regret_bits_per_symbol <= 0.025` on $\ge \lceil 0.90 \cdot \text{seeds} \rceil$ ($\ge 18/20$) seeds; across noisy `T1_Alternating` and `T1_PeriodicMotif(p=3)`, mean `paired_residual_regret_bits_per_symbol` is **strictly lower** than a fixed 1-lag reactive optimal echo (`Eqz(BoundaryIn)` / `Shift(2)`) evaluated on the exact same seeds; and `final_node_count <= growth.max_nodes` (`32`) on **100%** of runs. |
| **`G1B.3` — Live invariant, phase, & budget integrity** | **(a) Online audit:** Across every run in `G1B.1`, `G1B.2`, and `G1B.4`, `0` `LatticeError` / `InvariantError` / `EconomyError` failures occur, every intermediate and final `EditableLattice::circuit()` passes `Circuit::compile()`, and `scorecard.candidate_replay_ticks_consumed <= budget.max_eval_ticks`. **(b) Forced 500-edit phase stress battery:** Across $k \in \{1, 2\}$ and `{Combinational, TicksPerHop}` on `LocalVonNeumann`, embed a live autonomous period-3 oscillator (`Osc3`) driving `BoundaryOut(0)` and execute 500 deterministic interleaved sprout + starvation-prune + idle-prune transactions on secondary branches; assert `0` phase slips (`0` mismatches against clean `Osc3` trajectory across all 500 edits). |
| **`G1B.4` — Charter §8 M1 ablations & subprocess reproducibility** | **(a) Four-axis ablation matrix (§6.2 below):** All 4 pre-registered directional checks pass across the seed bank. **(b) Separate-process JSON determinism:** Spawn **two independent OS child processes** (`arkhe run --config ...`) for each of 6 `ArkheM1` configurations covering $k \in \{1, 2\}$, `{LocalVonNeumann, WellMixed}`, $\varepsilon \in \{0.0, 0.05\}$, and `{Combinational, TicksPerHop}`; assert `stdout_1 == stdout_2` byte-for-byte on all 6 runs. |

### 6.2 Charter §8 Ablation Matrix Specification (`G1B.4a`)

| Axis | Default (`ArkheM1`) | Ablated Variant(s) | Quantitative Directional Check Verified in `G1B.4` |
| :--- | :--- | :--- | :--- |
| **1. Reward weighting** | `RewardMode::CalibratedPointForecast` | `CenteredSurprisal`, `UncenteredSurprisal`, `RawAccuracy` | Evaluated on `T1_Constant(0)` and sparse `T1_PeriodicMotif([0,0,0,0,0,1])` ($k \in \{1, 2\}$) starting from a seeded 2-node `ConstLoop(0)` circuit and online runs: `RawAccuracy` and `UncenteredSurprisal` sustain `ConstLoop(0)` (`final_node_count > 0` / positive energy on silent or 5/6-zero stream), whereas `CalibratedPointForecast` scores `ConstLoop(0)` at `0.0` flux, starves/prunes it on both tasks, and sustains the true rare-event predictor (`post_transient_clean_match_rate >= 0.99`). (`CenteredSurprisal` is also reported alongside `CalibratedPointForecast`.) |
| **2. `SPLIT` fan-out tax** | `split_code_tax: true` | `split_code_tax: false` | Evaluated across `T1_PeriodicMotif` + `T1_AffineLag1` + `T3b_CounterModN(3)` on `LocalVonNeumann`: `split_code_tax: true` achieves `<=` mean `final_split_count` and strictly lower mean unweighted `circuit_code_bits(..., split_code_tax = true)` (or strictly fewer redundant `Split` relays when extra relay paths exist) without degrading post-transient clean match rate (`diff <= 0.02`). |
| **3. Spatial locality** | `WiringTopology::LocalVonNeumann` | `WiringTopology::WellMixed` | Both achieve `>= 90%` seed solve rate on noiseless `T1` (`Alternating`, `PeriodicMotif`, `AffineLag1`), while `LocalVonNeumann` records finite spatial relay overhead (`mean_nodes_local >= mean_nodes_well_mixed`) cleanly quantified in the ablation report. |
| **4. Wire propagation delay** | `WireDelayMode::Combinational` | `WireDelayMode::TicksPerHop` | On `T1_Alternating`, `T1_PeriodicMotif(p=3)`, and `T1_AffineLag1` ($k \in \{1, 2\}$, $\varepsilon = 0.0$), `ArkheM1` under `TicksPerHop` compensates for hop pipeline latency via replay-aligned lag selection and achieves `0` post-transient mismatches on $\ge \lceil 0.90 \cdot \text{seeds} \rceil$ seeds. |

### 6.3 CLI Entry Points (`src/lib.rs`)

Add two subcommands to `Commands` in `src/lib.rs` (preserving `arkhe::greeting() == "arkhe m0"` and all existing `Run`, `Yardstick`, `VerifyM0`, `VerifyM1a` flags):

1. `VerifyM1b { seed_offset: u32, seeds: u32, horizon: u64, banks: Option<u32>, holdout: bool }`:
   - Calls `check_seed_range_holdout(seed_offset, total_seeds, holdout)?`.
   - In single-bank mode, runs `run_m1b_verification_suite(seed_offset, seeds, horizon)` (`DEFAULT_M1B_HORIZON = 2000`), prints pretty JSON `M1bVerificationSummary`, and exits non-zero if `!summary.all_passed`. When `--banks <N>` is set, evaluates `N` consecutive banks in parallel via `run_m1b_bank_evaluation` and emits `M1bBankReport`.
2. `VerifyM1 { seed_offset: u32, seeds: u32, m1a_horizon: u64, m1b_horizon: u64, banks: Option<u32>, holdout: bool }`:
   - Calls `check_seed_range_holdout(seed_offset, total_seeds, holdout)?`.
   - **Hard Stage Gate (Charter §7):** Executes M1a verification (`run_m1a_verification_suite_with_horizon_and_holdout` or `run_m1a_bank_evaluation`) **first**. If M1a fails, emits `stage_gate_aborted: true` with `m1b: None` and exits non-zero **without running M1b**.
   - When M1a passes, executes M1b (`run_m1b_verification_suite` or `run_m1b_bank_evaluation`) and emits `M1CombinedSummary` / `M1CombinedBankReport`.

---

## 7. Phased Implementation Roadmap (`Step 0`–`Step 6`)

Execute the seven steps below **strictly in order**. Commit after each step only when `cargo fmt --check`, `cargo clippy --locked --all-targets --all-features -- -D warnings`, and `cargo test --locked` are 100% green.

### Step 0 — Pre-Flight Audit and Baseline Regression Lock

- **Do:**
  1. Run `cargo test --locked`, `cargo run --locked --release -- verify-m0 --seed-offset 0 --seeds 2`, and `cargo run --locked --release -- verify-m1a --seed-offset 0 --seeds 2`.
  2. Read `src/structural/{economy.rs, surgery.rs, lattice.rs, growth.rs, learner.rs}`, `src/metrics/{mdl.rs, point_forecast.rs}`, and `src/verify_m1/mod.rs` to confirm line-level alignment with §1–§6 above.
- **Pass:** All existing M0 and M1a unit, integration, and CLI tests pass with zero warnings.
- **Stop rule:** If any pre-existing test fails on clean `HEAD`, stop and report before changing a single line of code.

---

### Step 1 — Economy & MDL Extensions (`D1b.2`–`D1b.7`)

- **Target files:** `src/structural/economy.rs`, `src/metrics/point_forecast.rs`, `src/metrics/mdl.rs`, `src/structural/live.rs`.
- **Do:**
  1. Implement `D1b.2` in `EconomyState::apply_tick`: leave `node.energy` unclipped while `node.juvenile_ticks > 0` and apply `max_energy_reserve` only on mature ticks (`node.juvenile_ticks == 0`) after deducting `maintenance_costs[i]`.
  2. Add `pub split_code_tax: bool` (`default = true`) to `EconomyConfig` (`D1b.3`).
  3. Implement `PointForecastScorer::reset_predictor_counts(&mut self)` (`D1b.4`, preserving `NullModel`) and the stratified `NullModelKind::RepeatLast` matched estimator (`D1b.6`) in `src/metrics/point_forecast.rs`.
  4. Implement `CircuitDescriptionAllocation` and non-panicking `allocate_circuit_description_bits(..., split_code_tax: bool)` plus `circuit_code_bits_with_tax` in `src/metrics/mdl.rs` (`D1b.7`), keeping `circuit_node_description_debts` panics intact.
  5. Add helper `EconomyState::observe_unconnected_port(&mut self, port: usize, raw_fact: u64)` (`D1b.4`) that warms `null_model` and returns `0.0` reward without requiring node ancestry.
- **Pass:**
  - Unit test: a juvenile sprout with `initial_energy = 4.0`, `juvenile_ticks = 128`, `max_energy_reserve = Some(25.0)`, and maturity debt `28.0` bits earning `0.30 bits/tick` reaches `42.4` bits at tick 127, pays `28.0` bits on tick 128 (`14.4 > 0`, surviving!), and clamps to `<= 25.0` on subsequent mature ticks.
  - Unit test: under `NullModelKind::RepeatLast`, both a repeat-last predictor on random streams and any constant predictor on any constant stream score `0.0` on every tick (`|r_t| == 0.0`), and empirical $\mathbb{E}[2^{r_t}] = 1.0 \pm 10^{-12}$ by exhaustive enumeration over $k \in \{1, 2\}$.
  - Unit test: `allocate_circuit_description_bits` never panics on $N = 0$ or `BoundaryIn -> BoundaryOut`, satisfies `sum(node_debts) + unowned_bits == circuit_code_bits_with_tax` to `< 10^{-12}`, and sets `Split` node + port-1 wire cost to `0.0` when `split_code_tax == false`.
  - Regression: `cargo test --test m1a_economy` passes 100% unchanged.
- **Stop rule:** If any M1a gate (`G1A.1`–`G1A.4`) changes numerical output or fails, stop immediately—`Step 1` must be strictly backwards-compatible with `juvenile_ticks = 0, max_energy_reserve = None, split_code_tax = true`.

---

### Step 2 — Spatial Frontier Relocation, Tapping, Additive Stacking, and Idle Mature Pruning (`D1b.1`)

- **Target files:** `src/structural/lattice.rs`, `src/structural/surgery.rs`.
- **Do:**
  1. Implement `LatticeCircuit::relocate_node`, `tap_source`, and `stack_output_adder` (§3.2) handling `(North, 3)` / `(North, 4)` border congestion and `BoundaryIn` `SPLIT`-only fan-out under `LocalVonNeumann`, `LocalMoore`, and `WellMixed`.
  2. Enforce `D1b.7` in both `stack_output_adder` and `prune_starved_nodes`: never leave an active node-less `BoundaryIn -> BoundaryOut` wire (`WellMixed` inserts/keeps a 1-node `Split` owner; starved last-node excision disconnects `BoundaryOut` cleanly).
  3. Implement `EditableLattice::prune_zero_sensitivity_mature_nodes` (`D1b.1`), checking zero output + latch sensitivity over `W_diag`, output equivalence on `W_diag`, and $L(C_{\text{new}}) \le L(C_{\text{old}})$.
  4. Implement marginal sprout debt accounting helper `MarginalDebtLedger` (`D1b.3`) and verify prune-free telescoping $\sum \Delta L_{\text{sprout}} = L_{\text{eff}}(C_m)$.
- **Pass:**
  - Unit test (`LocalVonNeumann` `8x8` border stacking): start empty $\to$ sprout `Eqz(BoundaryIn(0)) -> BoundaryOut(0)` occupying `(0, 3)` $\to$ tap `BoundaryIn(0)` and stack `Add` with `AddSelf(BoundaryIn(0))` at `BoundaryOut(0)`; assert `lower().is_ok()`, `compile().is_ok()`, and exact truth table $\operatorname{EQZ}(x) + 2x \pmod 4$.
  - Unit test (`D1b.1` idle pruning): construct `Add(Eqz(x), ConstLoop(0))` with mature debt already paid (`E_i = 10.0 > 0`); run `W_diag = 64` ticks, invoke `prune_zero_sensitivity_mature_nodes`, and assert `ConstLoop(0)` + `Add` are excised while `Eqz(x)` remains in phase with zero output change.
  - Randomized stress test (`G1B.3b`): 500 interleaved sprout + starvation-prune + idle-prune edits alongside a live `Osc3` autonomous oscillator across $k \in \{1, 2\}$ and `{Combinational, TicksPerHop}`; assert `0` invariant errors and `0` phase slips.
- **Stop rule:** If any surgical operation resets a surviving `Delay` register or `TicksPerHop` pipeline stage, fix the `old_to_new` mapping before proceeding to `Step 3`.

---

### Step 3 — Causal Signal Pool, $z^{-1}$ Recruitment, Motif Bank $\mathcal{K}$, and Noisy `Delay`-Ring Closure

- **Target files:** `src/structural/growth.rs`.
- **Do:**
  1. Implement `SignalPoolBuilder` (§4.2) collecting `BoundaryIn`, reachable live node wires, and virtual `lag_in`, `lag_yhat`, and `lag_res` ($\tau \in \{1, 2, 3\}$) traces over `W_diag`.
  2. Implement `MotifBank` (§4.3) generating the canonical $\le 24$ Level-0 candidates $\mathcal{K}(u_1, u_2)$ (with on-demand $z^{-\tau}$ `Delay` tap materialization), replaying each candidate on `ObservationWindow` metered via `TickBudget::try_consume`, and applying the strict acceptance guards (`min_match_gain`, $\Delta\text{Flux} > 0$, juvenile survival projection, and Epoch-1 1-primitive receptor priority).
  3. Implement `propose_autonomous_delay_ring` (§4.5) for $p \in 2..=6$ under noisy periodic streams (`!zero_error && uses_boundary_inputs()`), setting both tick-0 `Delay { init }` and live `ScalarState` registers in exact phase alignment.
- **Pass:**
  - Unit tests verifying exact two-stage algebraic synthesis (§4.4 table) on noiseless synthetic streams for all 4 `G1B.1` tasks (`[0, 1, 1]` $k=1$, `[0, 1, 3]` $k=2$, `AffineLag1` $k=2$, `CounterModN(3)` $k=2$).
  - Unit test verifying silent-stream guard: on `T1_Constant(0)` and `T1_Constant(3)` ($k = 2$), `0` candidates pass $\Delta\text{Flux} > 0$.
  - Unit test verifying noisy ring closure: on `T1_Alternating(0, 3)` at $\varepsilon = 0.05$, reactive `BoundaryIn` predictor transitions cleanly to `Osc2(0, 3)` and thereafter halts further sprouting (`0` additional sprouts over `1500` subsequent ticks).
- **Stop rule:** Do **not** tune `mi_threshold_bits` (`0.05`) or `min_match_gain` (`4`). If a `G1B.1` task fails to find its second stage, inspect spatial routing / feature alignment / motif construction bug—never sweep thresholds.

---

### Step 4 — `ArkheLearner` Online Wake Loop, `ExperimentConfig`, and Spatial Harness Wiring

- **Target files:** `src/structural/learner.rs`, `src/baselines/mod.rs`, `src/config.rs`, `tests/m1b_neurogenesis.rs`.
- **Do:**
  1. Replace `src/structural/learner.rs` with the full online `ArkheLearner` (§5.1) implementing `PredictorBaseline` (`BudgetAccountingMode::CandidateHistoryReplay`).
  2. Add defaulted `grid_shape(&self) -> Option<GridShape>` and `split_code_tax(&self) -> bool` on `PredictorBaseline` in `src/baselines/mod.rs`, wiring both into `compute_mdl` inside `run_prequential_harness`.
  3. Add `BaselineConfig::ArkheM1` and `ArkheM1Telemetry` in `src/config.rs` (§5.2), unlocking `LocalVonNeumann` and `LocalMoore` topologies for `ArkheM1` while keeping `UnsupportedTopology` rejection for M0 baselines.
  4. Expand `tests/m1b_neurogenesis.rs` with end-to-end TOML `execute_experiment` tests across `LocalVonNeumann`, `LocalMoore`, `WellMixed`, `Combinational`, and `TicksPerHop`.
- **Pass:**
  - `arkhe run --config <arkhe_m1_toml>` executes cleanly and emits valid `RunReport` JSON containing `m1_telemetry`.
  - Existing M0 CLI & config tests (`tests/cli.rs`, `tests/m0_acceptance.rs`, `tests/determinism.rs`) pass 100% unchanged (including `UnsupportedTopology` for `RandomSearch` on `LocalVonNeumann`).
- **Stop rule:** If `Scorecard.mdl.circuit_bits` disagrees with `ArkheLearner`'s internal `circuit_code_bits` on spatial topologies, fix `grid_shape()` plumbing before `Step 5`.

---

### Step 5 — Pre-Registered `G1B.1`–`G1B.4` Suite, §8 Ablations, CLI Wiring, and Dev-Bank Validation

- **Target files:** `src/verify_m1/m1b.rs`, `src/verify_m1/mod.rs`, `src/lib.rs`, `tests/m1b_neurogenesis.rs`.
- **Do:**
  1. Implement `G1B.1`, `G1B.2`, `G1B.3`, and `G1B.4` (including the 4-axis §8 ablation runner and 6-config separate-process `Command` determinism check) in `src/verify_m1/m1b.rs`.
  2. Wire `Commands::VerifyM1b` and `Commands::VerifyM1` (with the hard M1a pre-growth abort gate and `check_seed_range_holdout`) in `src/lib.rs`.
  3. Add CLI tests in `tests/m1b_neurogenesis.rs` verifying:
     - `verify-m1b` and `verify-m1` refuse `--seed-offset 10000 --seeds 20` without `--holdout`,
     - `verify-m1` runs M1a first and aborts before M1b if M1a fails (unit test via injected failing M1a summary helper), and
     - Fast 2-seed slice (`--seed-offset 0 --seeds 2`) passes all gates in integration tests.
  4. Run the full 20-seed pre-registered **development bank**:

     ```bash
     cargo run --locked --release -- verify-m0 --seed-offset 0 --seeds 20
     cargo run --locked --release -- verify-m1 --seed-offset 0 --seeds 20
     ```

- **Pass:**
  - `verify-m0 --seed-offset 0 --seeds 20` passes `G1`–`G4`.
  - `verify-m1 --seed-offset 0 --seeds 20` passes `G1A.1`–`G1A.4` **and** `G1B.1`–`G1B.4` (`all_passed: true`, exit code `0`).
- **Stop rule (Charter §7 M1b Kill Criterion & `GEMINI.md`):**
  - If any gate `G1B.1`–`G1B.4` fails on the 20-seed development bank (`0..20`), diagnose whether the failure is a **deterministic implementation bug** (e.g., off-by-one register phase, spatial relocation rollback bug, `TicksPerHop` lag misalignment) vs. an **algorithmic/economic finding** (e.g., a sprout legitimately starving under pre-registered debt or failing Charter §3.1.5 thresholds).
  - Fix implementation bugs with unit tests; if the failure is economic or statistical, **STOP immediately and report the exact diagnostic table to the user—NEVER tune pre-registered constants (`0.90`, `4.0`, `128`, `25.0`, `64`, `32`, `0.05`, `4`, `32`)**.

---

### Step 6 — Lock Specification, Single-Shot Holdout Sign-Off (`10000..=10019`), and Packaging

- **Target files:** `doc/milestone_1_plan.md`, `doc/milestone_1b_plan.md`, `README.md` (CLI subcommand list if needed).
- **Do:**
  1. Verify working tree cleanliness, formatting, Clippy, tests, and documentation lints:

     ```bash
     cargo fmt --check
     cargo clippy --locked --all-targets --all-features -- -D warnings
     cargo test --locked
     ```

  2. Commit all code changes so `BuildProvenance.source_tree_fnv1a128` and `git_commit` are locked prior to touching holdout seeds.
  3. Execute the **single-shot untouched holdout verification** (`10000..=10019`) **exactly once**:

     ```bash
     cargo run --locked --release -- verify-m1 --seed-offset 10000 --seeds 20 --holdout
     ```

  4. Record the exact holdout outcome, `source_tree_fnv1a128`, and gate pass counts in `doc/milestone_1_plan.md` and `doc/milestone_1b_plan.md` **whatever it shows**—zero post-holdout parameter or logic edits are permitted.
  5. Verify crate packaging and Markdown lint conformance:

     ```bash
     cargo package --allow-dirty
     files=$( { git diff HEAD --name-only --diff-filter=AM -- '*.md'; git ls-files -o --exclude-standard -- '*.md'; } | sort -u )
     [ -n "$files" ] && printf '%s\n' "$files" | xargs -d '\n' npx markdownlint-cli2 --fix
     npx markdownlint-cli2 "doc/**/*.md" README.md
     ```

- **Pass:** Holdout `verify-m1 --seed-offset 10000 --seeds 20 --holdout` exits `0` (`all_passed: true` across `G1A.1`–`G1A.4` and `G1B.1`–`G1B.4`), `cargo package --allow-dirty` succeeds, and `markdownlint-cli2` reports `0` errors.
- **Stop rule:** Never re-run holdout after modifying code or thresholds. If holdout fails any gate, record the exact failure verbatim and report it to the user as an empirical Milestone 1 finding.

---

## 8. Summary of Locked M1b Constants (Zero New Tuned Parameters)

| Parameter | Value | Origin / Role |
| :--- | :--- | :--- |
| `trace_discount` ($\gamma$) | `0.90` | Locked in M1a (`EconomyConfig::default()`); register ancestry & activity recency decay |
| `initial_energy` ($E_{\text{init}}$) | `4.0` bits | Locked in [milestone_1_plan.md](milestone_1_plan.md#L465); juvenile seed endowment |
| `juvenile_ticks` ($T_{\text{juvenile}}$) | `128` ticks | Locked in [milestone_1_plan.md](milestone_1_plan.md#L466); grace escrow window before maturity debt settlement |
| `max_energy_reserve` ($E_{\max}$) | `Some(25.0)` bits | Locked in [milestone_1_plan.md](milestone_1_plan.md#L467); applied post-maturity (`juvenile_ticks == 0`) |
| `reward_mode` | `CalibratedPointForecast` | Locked in M1a redesign `D2`; scores silent/constant predictors identically `0.0` |
| `split_code_tax` | `true` | Structural `Split` operator + dual-wire MDL code length (`false` in §8 Ablation 2) |
| `grid_rows`, `grid_cols` | `8, 8` | Locked in [milestone_1_plan.md](milestone_1_plan.md#L460-L461) |
| `max_relay_hops` | `4` | Locked in [milestone_1_plan.md](milestone_1_plan.md#L372); local BFS `Split` relay cap |
| `diagnostic_window` ($W_{\text{diag}}$) | `64` ticks | Locked in [milestone_1_plan.md](milestone_1_plan.md#L472) |
| `sprout_interval` | `32` ticks | Locked in [milestone_1_plan.md](milestone_1_plan.md#L473) |
| `mi_threshold_bits` ($\theta_{\text{MI}}$) | `0.05` bits | Locked in [milestone_1_plan.md](milestone_1_plan.md#L474); Miller–Madow corrected MI threshold |
| `min_match_gain` ($\Delta_{\min}$) | `4` symbols | Locked in [milestone_1_plan.md](milestone_1_plan.md#L475); discrete noise-floor guard |
| `max_nodes` | `32` nodes | Locked in [milestone_1_plan.md](milestone_1_plan.md#L476) |
| `DEFAULT_M1B_HORIZON` | `2000` ticks | Locked in [milestone_1_plan.md](milestone_1_plan.md#L450) (`warmup_ticks = 500`, `max_eval_ticks = 50_000`) |

---

## 9. Milestone 1b Attainment & Verification Record

### 9.1 Single-Shot Holdout Sign-Off Verification

In accordance with Charter §7 and §6 of this plan, holdout verification was performed with locked pre-registered constants on git commit `e29014a5c4e410f90797db079d457740c5c2d4be`:

```bash
cargo run --locked --release -- verify-m1 --seed-offset 10000 --seeds 20 --holdout
```

- **Execution Date:** 2026-11-07
- **Git Commit SHA:** `e29014a5c4e410f90797db079d457740c5c2d4be`
- **Git Dirty:** `false` (locked prior to execution)
- **Source Tree Digest (`source_tree_fnv1a128`):** `564de6fbdb0b8cbd898b91fc6b4a408a`
- **Cargo Lock Digest (`cargo_lock_fnv1a128`):** `8d29cd24b1fbdbb743cf5c0b3933b22d`
- **Rustc Version:** `rustc 1.99.1 (58cf8f9c3 2026-10-13)`
- **Target Triple:** `x86_64-unknown-linux-gnu`
- **Seed Range:** `10000..=10019` (20 holdout seeds)
- **Status:** **ALL GATES PASSED** (`all_passed: true`, `stage_gate_aborted: false`)

### 9.2 Gate-by-Gate Results

#### Stage M1a Pre-Growth Gate Audit (6,245 cases across 20 holdout seeds)

- **`G1A.1` — Conservation & Substrate Purity:** **PASSED** (1,959 / 1,959 cases passed, 0 failures). $\lvert \sum \Delta E_i - (\text{flux} - \text{maint}) \rvert < 10^{-12}$, zero float arithmetic in `substrate`.
- **`G1A.2` — Correct vs. Incorrect Separation:** **PASSED** (1,069 / 1,069 cases passed, 0 failures). 100% of optimal circuits sustain positive energy; all chance, wrong, and silent circuits starve.
- **`G1A.3` — Detour & Free-Rider Penalty:** **PASSED** (1,579 / 1,579 cases passed, 0 failures). Zero credit to idle detours; single-fire detour leak credit strictly bounded across horizons.
- **`G1A.4` — $\Delta E$ Tracks $-\Delta L_{\text{null}}$:** **PASSED** (1,638 / 1,638 cases passed, 0 failures). $\lvert \Delta E + \Delta L_{\text{null}} \rvert \le 10^{-9} \max(1, \lvert \Delta L_{\text{null}} \rvert)$ with zero sign inversions.

#### Stage M1b Neurogenesis Gates (294 cases across 20 holdout seeds)

- **`G1B.1` — Stepwise First-to-Second Neuron Reduction:** **PASSED** (80 / 80 cases passed, 0 failures across 4 tasks on 20 seeds). Monotonic prequential KT loss reduction $L_2 < L_1 < L_0$, Sprout 1 survival, and $\lvert (H(R_1) - H(R_2)) - \hat{I}(R_1; U^*) \rvert \le 0.05\text{ bits/symbol}$ on $\ge 95\%$ of seeds.
- **`G1B.2` — T1 Suite Convergence & Noise-Floor Halting:** **PASSED** (200 / 200 cases passed, 0 failures across noiseless & noisy streams on 20 seeds). Noiseless: 0 nodes on silence, 0 mismatches on alternating/motif/affine. Noisy ($\varepsilon = 0.05$): paired regret $\le 0.025\text{ bits/symbol}$, strictly outperforms 1-lag reactive echo baseline, and growth halts at noise floor (`node_count <= 32`).
- **`G1B.3` — Live Invariant, Phase, & Budget Integrity:** **PASSED** (4 / 4 cases passed, 0 failures). Zero runtime invariant errors across all runs; forced 500-edit live oscillator phase test across $k \in \{1, 2\}$ and `{Combinational, TicksPerHop}` recorded 0 phase slips.
- **`G1B.4` — Charter §8 Ablations & Separate-Process Determinism:** **PASSED** (10 / 10 cases passed, 0 failures). 4-axis ablation matrix confirmed all directional hypotheses; 6-configuration independent child process spawning confirmed byte-identical JSON determinism.

### 9.3 Post-Review Hardening & 100-Bank Statistical Sign-Off (`2026-11-08`)

Following the independent closeout audit in [milestone_1b_review.md](milestone_1b_review.md), all pre-M2 structural fixes (`TicksPerHop` post-edit causal ancestry priming, Epoch-1 constant-mode parity eliminating noisy non-zero constant thrashing, empty-circuit ring discovery + 2D rectangular spatial loop placement + juvenile metabolic survival simulation, full first-cycle `ObservationWindow` capacity `juvenile_ticks + 2 * diagnostic_window = 256` ticks covering the $152.9$-tick Shannon MDL crossover on `NoisyAlternating(k=1)`, multi-channel non-wiping ring surgery, Charter §3.4 negative-for-compression `null_calibrated_mdl_delta_bits` sign, and `k = 64` width safety) and the complete pre-registered gate matrix (`2,530` M1b gate cases per 20-seed bank) were verified across **100 consecutive banks of 20 seeds (`2,000` seeds: `20..2020`, disjoint from untouched holdout `10000..=10019`)** with zero changes to pre-registered constants:

```bash
cargo run --locked --release -- verify-m1 --banks 100 --seed-offset 20 --seeds 20
```

- **Execution Date:** 2026-11-08
- **Source Tree Digest (`source_tree_fnv1a128`):** `10806db8e8a213e338d563a904cd36df`
- **Cargo Lock Digest (`cargo_lock_fnv1a128`):** `8d29cd24b1fbdbb743cf5c0b3933b22d`
- **Seed Range:** `20..2020` (`100` banks $\times$ `20` master seeds/bank = `2,000` master seeds; holdout `10000..=10019` untouched)
- **Stage M1a Result (`T = 1024`):** **100 / 100 banks passed** (`G1A.1`: `100/100`, `G1A.2`: `100/100`, `G1A.3`: `100/100`, `G1A.4`: `100/100`; `624,500` cases, `0` failures)
- **Stage M1b Result (`T = 2000`):** **100 / 100 banks passed** (`G1B.1`: `100/100` [`8,000` cases], `G1B.2`: `100/100` [`36,000` cases], `G1B.3`: `100/100` [`126,400` cases], `G1B.4`: `100/100` [`82,600` cases]; `253,000` cases, `0` failures)
- **Combined Status:** **`all_passed: true`, `stage_gate_aborted: false`**
