# Coordinate-Free Architecture and Principled Parameter Scaling Plan

**Date:** 2026-11-08 (revision 2)  
**Status:** Pre-Milestone 2 refactoring plan. Revision 2 checks every claim in the draft against `HEAD` (`78ca414`). It puts behavior-preserving work ahead of behavior-changing work and adds a verification safety net (§3, Phase 0).  
**Scope:** Separate graph morphogenesis from the leftover 2D grid. Replace ungrounded scalar constants with scaling rules, or reclassify them explicitly. Make every remaining limit observable, so that parameter cliffs show up instead of failing silently.

---

## Executive summary

Milestones 1a and 1b showed the core hypotheses hold:

1. **H1 (Metabolic selection):** Prequential calibrated reward plus one-time description debt $\Delta L_i$ gives exact micro/macro MDL conservation ($\Delta E = -\Delta L_{\text{null}}$, measured on cumulative flux).
2. **H2 (Residual neurogenesis):** Online mutual-information residual diagnosis, causal $z^{-\tau}$ delay recruitment, and stacked finite-ring correctors drive clean error to zero. They also close autonomous recurrent loops at the Bayes noise floor without backpropagation.

The friction in both milestones came from two kinds of technical debt, not from the theory:

1. **The leftover 2D coordinate grid.** M0's cellular-automaton metaphor left behind a 64-cell ceiling, perimeter anchor crowding, and coordinate placement code. Under the default `WellMixed` topology the compute engine throws all of this away at lowering.
2. **Ungrounded scalar constants.** Values such as `max_energy_reserve = 25.0`, `max_nodes = 32`, reactive lags `τ ≤ 3`, and per-bank pass hurdles had no scaling justification. Several of them collided with debt amortization times, bit widths, or multi-channel needs, and **they failed silently when they bound**.

Revision 2 changes how the work is run, not what it aims at:

* **Verify before changing.** A golden-trace harness (Phase 0) must exist before any refactor. Grid removal and the multi-channel generalization must then reproduce M1 trajectories **bit for bit** under `WellMixed`. Behavior-changing parameter rules come last, each with its own re-baseline.
* **Observe before tuning.** Every cap, guardrail, and window emits telemetry when it binds. The M1 friction came from cliffs that nobody could see.
* **Classify, then ground.** Every constant is listed in a single inventory (§2.1) as *derived*, *pre-registered scientific parameter*, or *compute guardrail*. Free multipliers inside "derived" rules (the draft's `β = 2.0`, `N_channel = 16`) are magic numbers under another name. They are removed or turned into explicit decisions.
* **Reuse, don't duplicate.** `substrate::Circuit` is already a coordinate-free directed multigraph. It becomes the single topology representation, and no parallel `GraphCircuit` type is built.

---

## 1. Removing the 2D grid

### 1.1 What does the 2D grid add today?

**Under the default `WellMixed` regime, nothing behavioral.**

M0 assumed spatial locality meant nearest-neighbour routing on a planar substrate (`LocalVonNeumann` with single-cell `SPLIT` relays). As documented in [Charter v3 §3.1.1](foundational_research_charter_v3.md#311-spatial-lattice-and-graph-connectivity) and Decision Record #29, planar relays congested the output ports and blocked multi-stage neurogenesis. The M1b review measured this directly ([review §1.2](milestone_1b_review.md#12-architectural-and-mathematical-strengths)). Charter v3 therefore pivoted to:

* **Direct point-to-point wiring** (`WellMixed`) as the default.
* **Locality priced by description length:** $L_{\text{wire}} = \log_2(S \times D)$ bits per edge, plus explicit `SPLIT` taxes in [`circuit_code_bits_with_tax`](../src/metrics/mdl.rs). Under `WellMixed` this code length is already independent of coordinates, because `placement_bits_per_node = 0` and the wire cost uses port counts only.

### 1.2 What does keeping the grid cost?

The spatial data structure stayed behind in [`LatticeCircuit`](../src/structural/lattice.rs#L260) and [`EditableLattice`](../src/structural/surgery.rs#L68):

1. **Hard 64-cell ceiling.** [`ArkheLearner::new`](../src/structural/learner.rs#L190) hardcodes `GridShape::new(8, 8)`. Even under `WellMixed`, every node needs an empty cell (`place_node`, `nearest_empty_cell`, `best_empty_cell_between`), so placement fails with `NoSpace`/`UnknownEndpoint` at node 65.
2. **Perimeter anchor bookkeeping.** [`default_anchors`](../src/structural/learner.rs#L44) is about 110 lines of side and offset allocation that exists only to give boundary ports a coordinate.
3. **Code surface.** `lattice.rs` is 1,956 lines including tests. **The largest consumer is [`growth.rs`](../src/structural/growth.rs) (about 170 references to `Coord`/`LatticeCircuit`/`max_relay_hops`), not `surgery.rs`.** It covers candidate materialization, `materialize_const_loop` (BFS cell search), feature/motif start coordinates, ring placement (`2 × ⌈(p+2)/2⌉` rectangular loops), and `build_autonomous_ring_lattice`. `verify_m1/` (about 43), `config.rs`, `metrics/mdl.rs`, and four integration test files also depend on it.
4. **Misleading failure modes.** `LatticeError::NoSpace` and `UnknownEndpoint` invite future agents to write placement heuristics for an abstraction that has no meaning in the project.
5. **A `max_relay_hops` parameter threaded through every growth API**, even though it has no effect under `WellMixed`.

### 1.3 The replacement: `substrate::Circuit` as the only topology

`substrate::Circuit` ([`circuit.rs`](../src/substrate/circuit.rs#L61)) already stores `Vec<Primitive>` indexed by `NodeId(u32)` and `Vec<Edge { src, dst, hops }>`, and it already enforces invariants in `compile()`. `LatticeCircuit::lower()` copies nodes and edges in insertion order and assigns `hops = 1` per edge under `WellMixed` + `TicksPerHop`. Building another graph type would duplicate this. Instead:

* **Substrate additions (pure graph operations, integer-only, no new dependencies):** `disconnect(src, dst)`, `replace_edge_source`, and `remove_nodes(&BTreeSet<NodeId>) -> Vec<Option<NodeId>>` (stable, order-preserving compaction).
* **Structural policy helpers** (`tap_source` with automatic `SPLIT` insertion, `stack_output_binary_op`, const-loop and ring materialization) become free functions over `&mut Circuit` in a new `structural/graph_edit.rs`. They encode growth policy, not substrate semantics.
* **`EditableLattice` → `EditableCircuit`**, wrapping `Circuit`, `CompiledCircuit`, `ScalarState`, and `EconomyState`. `recompile_preserving_state` stays the single transactional edit path.
* **Timing:** combinational wires, with registers only as explicit `Primitive::Delay` nodes. Under the `TicksPerHop` ablation, every edge carries `hops = 1`, which matches the current `WellMixed` lowering.
* **Determinism constraint:** every edit helper must keep the **node and edge insertion order** of the current `WellMixed` code paths. Node indices drive feature names, candidate tie-breaking, and sensitivity sampling, so reordering would break the Phase 1 bit-identity gate (§3).

> [!NOTE]
> The draft listed a `SignalSink::InternalComparator(id)` variant. It does not exist in `HEAD`. Addressable internal comparator sinks are an M2/M4a requirement ([review §3.1, §3.3](milestone_1b_review.md#31-specific-readiness-analysis-for-m2-multi-channel--redundancy-sprouting)). They are **deferred to the M2 plan**, with one constraint here: Phase 2 must key per-port developmental state and scorers by a sink index type that can later grow a comparator variant without another refactor.

### 1.4 What is lost, and how to retire it properly

Removing the grid removes the `LocalVonNeumann`/`LocalMoore` ablations, which are **pre-registered evidence**:

| Pre-registered artefact | Location | Action |
| :--- | :--- | :--- |
| `G1B.4a` Axis 3 (spatial locality vs. `WellMixed`) | [`verify_m1/m1b.rs`](../src/verify_m1/m1b.rs#L1764) | Retire |
| M1a spatial case `Add(Osc2, ConstLoop(0))` on `LocalVonNeumann` | [`verify_m1/mod.rs`](../src/verify_m1/mod.rs) (around line 825) | Retire |
| Spatial unit and integration tests | `lattice.rs`, `surgery.rs`, `learner.rs`, `tests/m0_acceptance.rs`, `tests/m1b_neurogenesis.rs` | Delete or port to `WellMixed` |
| `grid_rows`, `grid_cols`, `max_relay_hops`, `wiring_topology` config fields | [`config.rs`](../src/config.rs) | Remove (breaking changes are allowed) |

Retirement protocol: (1) add Charter Decision Record #30 stating that the topology pivot is now structural and the spatial ablation is closed. (2) Tag the last commit that still contains the lattice (for example `m1-final-lattice`) as the reproduction anchor for the retired evidence. (3) Record the retirement in an addendum to `milestone_1b_plan.md` so the M1 sign-off record stays internally consistent.

---

## 2. Parameters: classification, observability, and scaling rules

### 2.0 Rules that apply to every parameter

1. **Classification.** Each constant is exactly one of:
   * **Derived:** computed from other quantities with no free multiplier.
   * **Pre-registered scientific parameter:** a free value that is justified in the Charter or a milestone plan, fixed before data is collected, and backed by a recorded sensitivity sweep.
   * **Compute guardrail:** exists only to bound runtime. It must **never bind in an accepted run**. If it binds, the run is flagged and does not count.
2. **Observability.** Every cap, guardrail, clip, or window truncation increments a named telemetry counter when it binds, for example `SproutBlocked::MaxNodes`, `ReserveClipped { bits }`, or `CandidateUndecidable`. Verification harnesses report these counters, and guardrail counters are asserted to be zero.
3. **Reduction to M1.** Where possible, a new rule must reduce *exactly* to M1 behavior at M1 configurations (`K = 1`, `k ∈ {1, 2}`). That makes it checkable by golden traces. A rule that cannot reduce this way is behavior-changing and goes in Phase 3.
4. **No post-hoc fitting.** Choosing a multiplier because it "recovers the historical constant" is fitting, not grounding. Any rule that still has a free multiplier is a pre-registered parameter and needs a sensitivity sweep.

### 2.1 Constants inventory (single source of truth)

| Constant | Current value / location | Class (revision 2) | Action | Phase |
| :--- | :--- | :--- | :--- | :---: |
| Grid shape | `8 × 8`, [`learner.rs`](../src/structural/learner.rs#L197) | Leftover | Delete (§1) | 1 |
| `max_relay_hops` | `4`, learner + growth APIs | Leftover | Delete (§1) | 1 |
| `max_energy_reserve` | `Some(25.0)`, [`economy.rs`](../src/structural/economy.rs#L97) | Ungrounded | Measure, then decide (§2.2) | 0 → 3 |
| `max_nodes` | `32`, [`growth.rs`](../src/structural/growth.rs#L43) | Compute guardrail | Guardrail semantics plus telemetry (§2.3) | 2 |
| Global juvenile sprout freeze | [`learner.rs`](../src/structural/learner.rs#L548-L554) | Multi-channel defect | Per-port developmental state (§2.4) | 2 |
| `obs_window` capacity | `juvenile + 2·W_diag = 256`, [`learner.rs`](../src/structural/learner.rs#L240-L241) | **Already implemented** | Add `CandidateUndecidable` telemetry (§2.5) | 0 |
| Miller–Madow alphabet | Empirical support, [`growth.rs`](../src/structural/growth.rs#L272-L276) | **Already implemented** | Fix doc claims; window/estimator scaling deferred to M5 (§2.6) | — |
| Reactive lag horizon | `τ ∈ 1..=3` (feature pool, lagged MI), [`growth.rs`](../src/structural/growth.rs#L630-L634) | Ungrounded | Search bound with telemetry (§2.7) | 2 |
| Candidate feature cap | `.take(6)`, [`growth.rs`](../src/structural/growth.rs#L231) | Ungrounded compute bound | Guardrail semantics plus telemetry (§2.7) | 2 |
| `juvenile_ticks` | `128` | Pre-registered (M1) | Keep; record derivation gap; `CandidateUndecidable` shows if it is too short | — |
| `diagnostic_window` | `64` | Pre-registered (M1) | Keep; revisit with the M5 estimator decision (§2.6) | — |
| `sprout_interval` | `32` | Pre-registered (M1) | Keep | — |
| `mi_threshold_bits`, `min_match_gain` | `0.05`, `4` | Pre-registered (M1) | Keep; candidate for consolidation in M5 (§2.6) | — |
| `initial_energy` | `4.0` | Pre-registered (M1) | Keep | — |
| `trace_discount` γ | `0.90` | Pre-registered (Charter DR #26) | Keep | — |
| Sensitivity `sample_count` | `8`, [`learner.rs`](../src/structural/learner.rs#L249) | Pre-registered (M1, `k > 1` only) | Keep; characterize in the M5 width sweep | — |
| KT prior mass in `q̂` | `½·2^k` pseudo-counts, [`point_forecast.rs`](../src/metrics/point_forecast.rs#L92) | Derived (symbol-level KT) | Characterize earning latency vs. `k` in the M5 width sweep | — |
| Stage-gate statistics | per-bank pass hurdles | Ungrounded | Pooled confidence bound (§2.8), pre-registered in the M2 plan | 4 |

### 2.2 Energy reserve ceiling $E_{\max}$

#### What the cap is for

[Charter §3.1.2](foundational_research_charter_v3.md#312-metabolism) says mature reserves are bounded "so obsolete structures cannot coast indefinitely after regime shifts." So the cap sets an **adaptation latency**: an obsolete node needs about $E_{\max} / \text{drain rate}$ ticks to starve. Any principled rule has to be derived from that purpose. **No current test exercises a regime shift**, so whatever value is chosen is unvalidated today.

#### Corrections to the draft

1. **The O(k) loss claim does not hold under `CalibratedPointForecast`.** A mismatch costs $\log_2\frac{1-q}{1-p}$. That is set by the predictor's confidence $q$ (roughly its running hit rate) and is about $\log_2 \varepsilon$ under substitution noise $\varepsilon$, whatever the width. Width enters the **hit** reward $\log_2(q/p)$ through $p \sim 2^{-k}$. A confident predictor that suddenly becomes wrong loses about $\log_2(t)$ bits per tick, which grows with its match history, not with $k$.
2. **$k / \ln 2$ has no derivation.** The quantities are already in bits, so the $\ln 2$ factor is spurious. The stated burst length $\ell \approx 3\text{–}5$ never appears in the formula.
3. **$\beta = 2.0$ "recovering 25.0" is post-hoc fitting.** Real per-node debts are not 12.5 bits. For example, `Split` owes about 13.75 bits ([review Finding 2](milestone_1b_review.md#finding-2-high--halting--metabolic-defect-epoch-1-bypass-of-flux-and-survival-checks-causes-infinite-sproutstarveprune-thrashing-on-noisy-non-zero-constant-streams)), and debts grow with graph size because $\log_2(S \cdot D)$ does.
4. **Plumbing.** Per-node $\Delta L_v$ lives in [`MarginalDebtLedger`](../src/structural/surgery.rs#L643) (and again in `LiveCircuit::description_debts`), not in `EconomyState`. A debt-dependent cap needs the debt to move into `NodeEconomy`, which also removes the duplicate copy.

#### Plan

1. **Phase 0, measure:** add `ReserveClipped { node, bits }` telemetry and report clip counts and clipped bits across the M1 development suites. If the 25-bit cap **never binds** in M1, any replacement that only loosens it is M1-neutral.
2. **Phase 0, add the missing test:** a regime-shift integration test. A circuit locks to task A, then the stream switches to task B, and the test records the ticks until the obsolete cone starves and a replacement sprouts. This test is what validates any $E_{\max}$ rule.
3. **Phase 3, decide (requires user sign-off under the magic-number rule). Candidate rules:**
   * **(A) Debt-proportional cap:** $E_{\max}(v) = \beta \cdot \Delta L_v$. Simple, but $\beta$ is a pre-registered free parameter and needs a sweep (for example $\beta \in \{1, 2, 4\}$) showing the gates do not depend on it.
   * **(B) Horizon-derived cap:** $E_{\max}(v)$ equals the share-weighted loss node $v$ would take over $H$ consecutive mismatches at its port's current confidence $q$, with $H$ tied to an existing lifecycle quantity (for example $W_{\text{obs}}$: the system cannot judge a replacement faster than it can observe one). This adapts to $k$, $\varepsilon$, and confidence, has no free multiplier, and guarantees obsolete nodes starve within about $H$ ticks.
   * **(C) Exponential reserve forgetting** with time constant $H$ in place of a hard cap.

   All three leak energy just as clipping does today. Conservation stays measured on cumulative flux, which is how the current telemetry already does it.

### 2.3 Node limit $N_{\max}$

The draft's $N_{\max} = K \times 16$ replaces one magic number with another. At $K = 1$ it would also **halve** the current M1 limit (32 → 16) and change behavior without saying so.

**Rule:** `max_nodes` is a **compute guardrail**, not a developmental parameter. Growth is meant to be limited by MDL debt and calibrated flux.

* Check `node_count + motif_size <= max_nodes`. Today the check is `node_count < max_nodes` before proposing ([`learner.rs`](../src/structural/learner.rs#L696)), so a multi-node motif can overshoot.
* Emit `SproutBlocked::MaxNodes`. Verification harnesses assert it is zero, and a run where it binds is flagged and excluded.
* Set the value from the Phase 0 compute profile (per-tick cost against $N$ and $K$), not from a per-channel node estimate. At M1 sizes (≤ 9 nodes observed) any value ≥ 32 is M1-neutral.

### 2.4 Per-port developmental state (cone-local maturity)

#### The problem is wider than the juvenile check

The draft scopes only the juvenile check to the target port's cone. Channel-isolated development needs **all** of the following, which are global in `HEAD`:

| Global coupling in `ArkheLearner::observe` | Effect at $K > 1$ |
| :--- | :--- |
| `all_mature` over every node ([L548-L554](../src/structural/learner.rs#L548-L554)) | Any juvenile node freezes diagnosis and sprouting on every channel |
| One `post_juvenile_diag` buffer, cleared on *any* sprout or prune | One channel's edit throws away every other channel's diagnostic evidence |
| One `ticks_since_last_check` timer, plus `break` after the first sprouting port | Serial development, plus a port-index bias (port 0 always gets first chance) |
| `all_driven_ports_primed` over every port | One unprimed `TicksPerHop` chain blocks every channel |
| Epoch snapshots hardcoded to port 0 / `sprout_events == 2` | No per-channel G1B.1-style evidence |

#### Rule

Each output sink $p$ carries its own `DiagnosticBuffer`, sprout timer, primed flag, and maturity gate:

$$\text{CanSprout}(p) \iff \forall v \in \text{UpstreamCone}(p),\ \text{juvenile}(v) = 0$$

In addition:

* **Exclude features sourced from juvenile nodes** from port $p$'s signal pool. Without this, cross-channel taps (`node:*` features from channel $A$'s cone feeding a candidate on $B$) would build on structure that is still settling, which is what the juvenile freeze was meant to prevent.
* A structural edit clears only the buffers of ports where [`port_cone_changed`](../src/structural/surgery.rs) is true. That predicate already scopes scorer resets.
* Keep **at most one committed structural edit per tick** for transactional simplicity, but rotate the starting port (`tick mod K`) to remove port-index bias.

**M1 reduction:** at $K = 1$ every live node sits in port 0's cone, so the per-port state matches the global state exactly. This step is therefore covered by the golden-trace gate. **New acceptance test:** $K$ independent single-channel tasks run side by side should each lock within the single-channel tick budget, not $K$ times later.

### 2.5 Observation window $W_{\text{obs}}$: already implemented

`HEAD` already sets `obs_capacity = juvenile_ticks + 2 · diagnostic_window` (256 ticks), introduced in the post-review hardening ([review §5.1](milestone_1b_review.md#51-resolution-of-review-findings), Finding 3).

**Correction:** the rule does **not** guarantee $W_{\text{obs}} \ge W^*$ in general. $W^* = \Delta L / \Delta I$ depends on the candidate's debt, the noise level, the width, and the motif size. The draft's "every candidate capable of surviving is recognized" holds only for the measured `NoisyAlternating(k=1)` case ($W^* \approx 152.9$).

**Remaining work (Phase 0, telemetry only, M1-neutral):** in candidate replay, compute $W^*_{\text{cand}} = \Delta L_{\text{cand}} / \widehat{\Delta I}_{\text{cand}}$. When a candidate is rejected and $W^*_{\text{cand}} > W_{\text{obs}}$, emit `CandidateUndecidable`. If this counter is non-zero in M2/M5 runs, the window (and with it `juvenile_ticks`) is too short for that regime. That is a visible signal rather than a silent cliff.

### 2.6 Miller–Madow support: already implemented; estimator scaling deferred to M5

[`score_feature_group`](../src/structural/growth.rs#L255-L277) already uses the empirical support of both the joint feature alphabet and the residual alphabet.

**Correction:** the draft's claim that empirical support "guarantees that the bias penalty can never exceed the empirical entropy" is false. With $N = 64$ and all-distinct values, $(63 \cdot 63)/(2 \cdot 64 \ln 2) \approx 44.7$ bits, against an entropy of $\log_2 64 = 6$ bits. $\min(N, |\text{Unique}|)$ is also redundant, since unique values never exceed $N$.

**The real scaling issue (M5):** a fixed $W_{\text{diag}} = 64$ undersamples large joint alphabets. The reliability of any plug-in MI test depends on $N / (|\hat{\mathcal A}_X||\hat{\mathcal A}_R|)$. Two options, to be decided in the M5 plan with width-sweep evidence:

1. Scale $W_{\text{diag}}$ with observed joint support.
2. Replace the triple (`mi_threshold_bits`, Miller–Madow, `min_match_gain`) with a single **prequential code-length gain** test: the KT-coded residual given the feature against the residual alone. This charges for model complexity automatically, fits the project's MDL foundations, and removes two pre-registered constants.

### 2.7 Reactive lag horizon and candidate caps (new)

The executive summary listed `τ ≤ 3` as a cliff, but the draft had no rule for it. It was the root cause of the sparse `PeriodicMotif([0,0,0,0,0,1])` stall ([review Finding 3](milestone_1b_review.md#finding-3-high--verification-gap-g1b2-and-g1b4-omit-pre-registered-plan-cases-masking-an-unimplemented-sparse-rare-event-capability)), which ring closure later worked around.

**Rule:** a lag horizon is a **search bound**, not a model constraint. Every materialized `Delay` is already priced by $L(C)$.

* Search lags by iterative deepening ($\tau_{\max} = 1, 2, 4, \dots$) only while no feature passes the growth criterion. Stop at `TickBudget` exhaustion or $\tau_{\max} \ge W_{\text{diag}}/2$, where the remaining sample count makes the MI estimate meaningless.
* Apply the same treatment to `.take(6)` candidate features and to lagged residual MI over lags `1..=3`, which is also currently unused (review Finding 6.1).
* Emit `LagHorizonExhausted` and `CandidateListTruncated`.

**M1 reduction:** run the deepening only when the `τ ≤ 3` pass finds nothing. M1 trajectories that sprouted with `τ ≤ 3` then stay identical. Runs that stalled (such as sparse $p = 6$) can now change, so this step needs a targeted test and does not rely on the golden trace alone.

### 2.8 Multi-bank statistical acceptance: pooled confidence bounds

The draft's arithmetic holds: $P(\text{Bin}(20, 0.95) \ge 18) = 0.9245$, and $0.9245^{100} \approx 0.04\%$. A per-bank hurdle applied across many banks is a compound filter that rejects reliable algorithms.

**Rule (to be pre-registered in the M2 plan before any M2 data is collected; M1 results are not re-scored):**

1. **Pooled success:** the one-sided 99% Clopper–Pearson lower bound on the pooled success rate over all $N = M \times B$ seeds must be $\ge P_{\text{target}}$. The draft's extra slack term $\epsilon$ is dropped as an unanchored constant.
2. **Homogeneity:** a $\chi^2$ test of equal success rates across banks at the same 1% level. Bank mean, standard deviation, and 5th percentile are reported. Clustered failures (rejected homogeneity) fail the gate even if the pooled bound passes. This gives "distributional stability" a falsifiable criterion.
3. Implement both as reusable helpers in `verify_m1/` (or a shared `verify` statistics module) in Phase 4, so that M2 and later harnesses use them instead of re-implementing them.

---

## 3. Engineering roadmap

```mermaid
flowchart LR
    P0["Phase 0: Safety net - golden traces, cliff telemetry, compute profile, regime-shift test"] --> P1["Phase 1: Coordinate-free refactor (behavior-preserving)"]
    P1 --> G1{"Golden traces bit-identical?"}
    G1 -- yes --> P2["Phase 2: Per-port developmental state, guardrails, lag search (M1-reducing)"]
    G1 -- no --> P1
    P2 --> G2{"K=1 golden traces identical and new K>1 tests pass?"}
    G2 -- yes --> P3["Phase 3: Behavior-changing rules (reserve cap) after user decision"]
    G2 -- no --> P2
    P3 --> G3{"M1 gates pass on dev banks and regime-shift test passes?"}
    G3 -- yes --> P4["Phase 4: Docs, decision record, instructions, stats helpers, provenance"]
    G3 -- no --> D["Stop and discuss with user"]
```

### Phase 0: Safety net (no behavior change)

1. **Golden-trace harness** (`tests/golden_traces.rs` plus `tests/fixtures/golden/`):
   * For every `WellMixed` learner configuration used by the M1a/M1b gates (both wire-delay modes, $k \in \{1, 2\}$, development seeds only), record a compact **event log**: tick and kind of every sprout, prune, and ring closure, plus the final `Circuit` edges. Also record a **trajectory digest**: FNV-1a over per-tick predictions, `structure_epoch`, and `f64::to_bits` of every node reserve.
   * Regenerate with an explicit bless switch (for example `ARKHE_BLESS=1 cargo test --test golden_traces`). On mismatch, report the **first divergent tick and event** so that a divergence is quick to localize.
   * Never use holdout seeds `10000..=10019`.
2. **Cliff telemetry** (counters only, M1-neutral): `ReserveClipped`, `SproutBlocked::MaxNodes`, placement `NoSpace` (until Phase 1 removes it), `CandidateUndecidable`, `CandidateListTruncated`. Surface them in `ArkheM1Telemetry` and the `verify-m1` summaries.
3. **Baseline measurements:** run the M1 development suites and record which limits bind and how often. Profile per-tick cost (sensitivity, diagnosis, candidate replay) against node count and channel count. This identifies M2's real compute bottleneck, which is most likely the $O(F^2)$ feature-pair scoring in `diagnose_residual` and per-node counterfactual sensitivity. Grid search is unlikely to matter, since `WellMixed` already avoids routing.
4. **Regime-shift test** (§2.2). It may fail today. Its job is to measure, not to gate.

**Exit:** fixtures committed, telemetry merged, measurements written into §2 of this document.

### Phase 1: Coordinate-free refactor (behavior-preserving)

Work in place, strangler-style: delete coordinate code path by path rather than rewriting from scratch, so the golden traces localize any regression.

1. Retire the spatial ablations following the §1.4 protocol (tag the commit, add Decision Record #30, add the plan addendum).
2. Add the `Circuit` mutation primitives (§1.3) with unit tests: order preservation, remap correctness, and invariant checks via `compile()`.
3. Port `EditableLattice` → `EditableCircuit`, then port `growth.rs` materialization (`materialize_source`, `materialize_const_loop`, motif and ring builders, `build_autonomous_ring_lattice`) and `MarginalDebtLedger` to `Circuit` plus `graph_edit.rs`. Keep insertion order identical.
4. Delete `lattice.rs`, `GridShape`, `Coord`, `PerimeterSide`, `BoundaryAnchor`, `default_anchors`, `WiringTopology` and the topology/grid parameters of the MDL functions, `max_relay_hops`, and the matching config fields. Update `README.md` and the CLI tests.

**Exit gate:** golden traces bit-identical; `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --locked` pass; `verify-m0` and `verify-m1` pass on development seeds, minus the retired cases.

### Phase 2: Multi-channel generalization (reduces to M1 at `K = 1`)

1. Per-port developmental state, juvenile-feature exclusion, and port rotation (§2.4), keyed by a sink index that can later hold comparators (§1.3 note).
2. `max_nodes` guardrail semantics (§2.3) and the lag/candidate search bounds (§2.7).
3. Per-port epoch snapshots, replacing the port-0 hardcoding.

**Exit gate:** `K = 1` golden traces identical (except the documented §2.7 stall cases, which are covered by their own tests). The new `K ≥ 2` concurrent-development test passes. Guardrail counters are zero.

### Phase 3: Behavior-changing rules (requires user decision)

1. Move per-node $\Delta L_v$ into `NodeEconomy`, removing the duplicate debt tables.
2. Implement the $E_{\max}$ rule chosen from §2.2 (A/B/C).
3. Re-run the M1 gates on development banks (pass/fail, not bit-identity) and the regime-shift test. Re-bless the golden traces **with a written rationale in the commit message**. Record the new `source_tree_fnv1a128` and evaluate on a fresh disjoint confirmation bank. The holdout stays untouched.

### Phase 4: Documentation, instructions, provenance

1. Charter: Decision Record #30 (grid retirement) and an updated §3.1.2 design note naming the chosen reserve rule.
2. Pooled-statistics helpers (§2.8), referenced from the M2 pre-registration.
3. Instructions amendment (§4).
4. Update this document's inventory (§2.1) with the final status of every row.

---

## 4. Proposed amendment to `.github/copilot-instructions.md`

`GEMINI.md` is a symlink to `.github/copilot-instructions.md`, so there is only one file to edit. The amendment states **principles and pointers**, not specific formulas: formulas may change, and copies inside instructions go stale. It extends the existing "NEVER search for magic numbers" requirement instead of duplicating it.

```markdown
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
```

---

## 5. Risk register

| Risk | Mitigation |
| :--- | :--- |
| A refactor silently changes node or edge order and therefore tie-breaking | Golden traces with first-divergence reporting (Phase 0); order-preservation unit tests for `Circuit` mutations |
| Retiring spatial gates looks like moving goalposts | Decision Record #30, a tagged reproduction commit, and a plan addendum (§1.4) |
| A reserve-cap change destabilizes M1 halting or noise-floor behavior | Phase 0 measures whether the cap binds; regime-shift test; Phase 3 is gated on user sign-off and a full M1 dev-bank re-run |
| Per-port development exposes cross-channel juvenile tapping | Juvenile-feature exclusion (§2.4) |
| M2 hits compute limits rather than structural limits | Phase 0 compute profile; `max_nodes` set from measured cost; guardrail counters asserted zero |
| Scope creep into M2 features (comparator sinks, redundancy protection) | Explicitly deferred to the M2 plan; Phase 2 only keeps the sink index extensible |

---

## 6. Revision log (draft → revision 2)

* **Corrected status:** §2.4 (observation window) and §2.5 (empirical Miller–Madow) were already implemented in `HEAD`. They are reclassified, and only telemetry remains.
* **Corrected claims:** the O(k) mismatch-loss rationale and the $k/\ln 2$ term for $E_{\max}$; "recovers 25.0" as post-hoc fitting; the false bound "Miller–Madow penalty ≤ entropy"; "~2,000 lines" (1,956 including tests); the speed-up claim, which is not expected under `WellMixed` and is no longer a success criterion; the nonexistent `InternalComparator` sink.
* **Removed:** a new `GraphCircuit` type (reuse `substrate::Circuit`); $N_{\max} = K \times 16$, which would also have halved the M1 limit; free multipliers inside the instruction text; the duplicate `GEMINI.md` edit (it is a symlink).
* **Fixed inconsistency:** the draft required bit-for-bit M0/M1 equivalence *after* behavior-changing parameter rules. Equivalence is now gated after Phases 1–2, and Phase 3 re-validates by pass/fail.
* **Added:** Phase 0 safety net (golden traces, cliff telemetry, compute profile, regime-shift test); constants inventory and classification rules; retirement protocol for the pre-registered spatial ablations; the full per-port developmental state needed for channel isolation; the missing `τ ≤ 3` / `.take(6)` rules; a falsifiable homogeneity criterion for bank statistics; `growth.rs`, `verify_m1/`, `config.rs`, and `mdl.rs` in the refactor scope.
