# Arkhe Technical Review: Charter Goals, Architectural Principles, and Progress Through Milestone 4a

> **Document Purpose & Audience:** This document is a self-contained, dispassionate technical review of the Arkhe research project from its foundational premise (`M0`) through the completion of Milestone 4a (`M4a`). It is written for technically trained readers (with an undergraduate background in computer science, mathematics, physics, or engineering) who have no prior familiarity with this codebase or its specialist subfields. All domain-specific concepts—modular ring circuits, computational mechanics, Minimum Description Length (MDL), prequential coding, and equality saturation—are introduced from first principles with concrete worked examples.
>
> **Primary Sources:** [Foundational Research Charter v3](foundational_research_charter_v3.md), [Arkhe Foundational Research Record](research_record.md), [Parameter & Constant Inventory](parameter_inventory.md), [Milestone 4a Plan](milestone_4a_plan.md), and [Independent Review of M4a](milestone_4a_independent_review.md).

---

## Executive Summary

Arkhe investigates a foundational question in machine learning and artificial life: **can a system starting with only six exact integer-arithmetic logic primitives—and no gradients, no floating-point weights, no pre-trained representations, and no external reward shaping—grow, self-verify, compress, and reuse discrete circuits that model sequential environments?**

Rather than training a fixed neural network architecture via backpropagation, Arkhe treats learning as an online, two-layer process governed by strict information-theoretic accounting:

1. **A structural layer ("neurogenesis and metabolism"):** Circuits are directed graphs of exact modular-arithmetic primitives (`ADD`, `SUB`, `MUL`, `EQZ`, `DELAY`, `SPLIT`) operating over $k$-bit integers modulo $2^k$. While **awake**, the system observes multi-channel discrete symbol streams, predicts the next symbol on each channel, earns bit-calibrated information reward when its predictions beat a null model, pays an exact description-length bit cost for every node and wire it adds, grows residual "corrector" subcircuits where prediction errors remain structured, and grows causally independent "comparator" backup routes to cross-check itself.
2. **A logical layer ("deduction and consolidation"):** While **asleep**, external inputs are cut and the circuit's predictions are looped back into its inputs. During sleep, redundant derivations test one another for internal contradictions (pruning spurious models), and—in upcoming milestones—the system aims to prove exact algebraic equivalences over held subgraphs and compile recurring verified subcircuits into cheaper, reusable higher-order primitives ("derived axioms" or macros).

### Where the Project Stands Today

Out of the ten roadmap stages (`M0` through `M6`, testing hypotheses `H1`–`H10`), **seven stages (`M0`, `M1a`, `M1b`, `Pre-M2`, `M2a`, `M2b`, `M3`, and `M4a`) have completed implementation and passed pre-registered, single-shot holdout evaluations** (with `M4a` holdout sealed and awaiting final administrative closeout sign-off `S4`).

What has been **demonstrated empirically**:

- A gradient-free, local metabolic credit economy can be constructed whose total internal node-energy change equals, up to machine precision ($\le 10^{-12}$ bits/symbol), the negative change in global null-calibrated Minimum Description Length (`M1a`, `M2b`, `M3`, `M4a`).
- Starting from an empty 0-node graph, residual-driven circuit growth reliably synthesizes multi-step single-channel predictors (`M1b`) and multi-channel nonlinear delayed predictors up to lag 12 (`M2a`), outperforming compute-matched random search, Cartesian Genetic Programming, Context-Tree Weighting, and small online BPTT recurrent networks on exact discrete coupling benchmarks.
- Redundant, causally disjoint backup derivations can be funded without collapsing into hallucination loops (`M2b`), enabling rapid fault recovery when an input channel corrupts and resisting nine classes of injected "free-rider" circuit bloat (`M3`).
- Offline consistency-only sleep (`S-A`) reliably rejects planted spurious cycles and beats compute-matched wake-only learning on noisy periodic tasks where channel noise slows wake-time refutation, outperforming replay-augmented sleep (`S-B`) by `+14.61` bits on the sealed 100-seed holdout (`M4a`).

What has **not yet been demonstrated** (and defines the central remaining scientific risk):

- In `M4a`, sleep only *falsified and pruned* bad backup derivations; on clean (noise-free) spontaneous learning tasks, wake learning already produced consistent circuits with zero wrong comparators left for sleep to prune.
- The system has **not yet** discovered a non-trivial algebraic equivalence on its own, formally proved it, compiled it into a new palette primitive that reduces total description length after paying for its own definition (`M4b`, hypothesis `H7`), or transferred such a compiled primitive to accelerate learning on a new task family (`M4c`, hypothesis `H8`). Crossing from *structural pruning* to *verified vocabulary growth* is the immediate frontier.

| Roadmap Stage | Hypothesis | Core Technical Question | Current Status | Headline Empirical / Architectural Outcome |
| :--- | :---: | :--- | :--- | :--- |
| **`M0`** Simulator & Yardstick | — | Deterministic $\mathbb{Z}_{2^k}$ simulator, exact $\varepsilon$-machine / Reeds–Sloane ground truth, baseline harness | **Completed** | Verified exact tick semantics, ground-truth state/entropy solvers, and baseline metering gaps |
| **`M1a`** Metabolic Economy | `H1` | Does local retrograde credit sustain true circuits and starve false/detour ones without gradients? | **Completed** (after kill-criterion redesign) | Initial per-tick rent failed across horizons; redesigned debt economy proved exact $\Delta E \equiv -\Delta L_{\text{null}}$ (`100/100` banks at $T=1024, 2048, 4096$) |
| **`M1b`** Residual Neurogenesis | `H2` | Can residual-driven sprouting grow multi-neuron circuits from 0 nodes? | **Completed** (+ Pre-M2 pivot) | Passed `100/100` banks (`253,000` cases) & holdout; retired 2D spatial grid after perimeter congestion deadlock in favor of coordinate-free graphs |
| **`M2a`** Cross-Channel Coupling | `H2` | Does residual growth scale to multi-channel ($K \le 4$), deep-lag ($\tau \le 12$), nonlinear tasks? | **Completed** | Passed `50/50` dev banks & holdout (`20/20` all gates), beating equal-compute `CGP`, `RandomSearch`, `CrossCTW`, and online `RNN` |
| **`M2b`** Redundancy & Comparators | `H3` | Can independent derivations be grown and funded to detect internal errors and channel faults? | **Completed** | Solved comparator funding paradox via epistemic corroboration; passed `1,000`-seed dev & holdout (`200/200` fault recovery) |
| **`M3`** Free-Rider Resistance | `H4` | Do MDL wire/fan-out pricing and local verified excision remove credit-parasitic bloat? | **Completed** (`M3 v2`) | Excised `100%` of transparent detours (`D0`–`D7b`) where single-node pruning failed `85.9%`; remediated pooled-metric masking (`46,842/46,900` holdout) |
| **`M4a`** Sleep Falsification | `H5`, `H6` | Does offline sleep beat equal-tick wake-only learning, and is replay (`S-B`) needed over pure consistency (`S-A`)? | **Holdout Passed** (`S4` closeout pending) | `S-A` beat wake-only on noisy periodic tasks (`264/300`) and planted errors (`785/800`), beating `S-B` by `+14.61` bits; zero opportunity on clean spontaneous tasks |
| **`M4b`** Theorem Consolidation | `H7` | Can sleep discover, formally prove, and promote subcircuits into MDL-reducing macros? | **Next Stage** (planning) | Staged next: census spontaneous compression opportunity first, then build end-to-end proof checker before full `H7` sweep |
| **`M4c`** Cross-Task Transfer | `H8` | Do compiled axioms accelerate learning across related generator families (`T4`)? | **Planned** | Depends on reusable macros from `M4b` |
| **`M5`** Width Scaling | `H9` | Do graded residuals ($k \in \{1,2,4,8\}$) speed learning and lift ring theorems via `TRUNC`? | **Planned** | Characterisation & heterogeneous bit-width study |
| **`M6`** Active Intervention | `H10` | Can environment perturbations/queries refute spurious observational correlations? | **Planned** | Completes causal epistemology beyond passive observation |

---

## 1. The Foundational Charter: Goals, Epistemology, and Scope

### 1.1 The Bootstrapping Problem in Continual Learning

Modern deep learning succeeds by fixing a large, differentiable parametric graph up front, initializing continuous floating-point weights randomly, and optimizing a global loss over pre-collected batches via stochastic gradient descent. While effective at scale, that paradigm relies on three strong external scaffolds:

1. **Continuous differentiability:** The substrate must use floating-point numbers so small weight nudges yield smooth loss gradients, which makes learned representations opaque and exact equivalence undecidable due to rounding and non-associativity.
2. **Fixed representational capacity:** The primitive operations and network topology are largely static; the learner adjusts continuous parameters rather than expanding its discrete vocabulary of exact operations.
3. **External supervision or engineered reward:** Outside the training loop, a human designer specifies loss functions, learning-rate schedules, and validation splits.

The [Foundational Research Charter v3](foundational_research_charter_v3.md) asks how a continual learner can bootstrap a verifiable world model from scratch when given none of those three scaffolds. Suppose an agent receives only raw discrete symbols emitted over time by an unknown deterministic (or noisy) mathematical process. With no gradients and no outside teacher telling it whether its internal wiring is "right," **how can the system decide which internal structures are reliable enough to keep, build upon, and eventually freeze into permanent building blocks?**

### 1.2 Fallibilist Truth: Asymmetry and Informativeness

Because the learner never sees the hidden source code of its environment—only the stream of symbols emitted at the boundary—it can never prove in an absolute philosophical sense that its internal circuit *is* the true universe. Tomorrow's observation could always refute today's circuit. Arkhe therefore adopts a **Popperian fallibilist definition of truth**: a circuit structure counts as provisionally "true" solely while it remains **unrefuted**—consistent with observed boundary facts and with other independent derivations held by the learner.

Two engineering design principles follow directly from this premise:

1. **The Asymmetry Principle (*disagreement is conclusive; agreement is provisional*):**
   - If two deterministic derivations of the same quantity produce different answers on the same state, **at least one of them is definitely wrong**. Disagreement is decisive evidence justifying structural pruning.
   - Conversely, if two derivations agree for 1,000 steps, both are merely *not yet refuted*. Agreement justifies keeping a circuit alive (retention), never promoting it unconditionally to an immutable axiom without stricter formal compression and equivalence checks.
2. **The Informativeness Principle (*agreement only counts in proportion to what it rules out*):**
   - In logic, the tautology $x = x$ is always true and conveys zero information. Similarly, in a circuit network, two dead wires that always output `0` will agree `100%` of the time if the environment happens to emit `0`s, and a 2-node oscillator (`0, 1, 0, 1, ...`) will agree with itself forever if unplugged from reality.
   - Therefore, agreement is never rewarded by raw hit rate or raw neural activity. Every prediction is scored **relative to a cheap statistical null model** (such as empirical symbol frequency). Correctly predicting a coin flip (`50%` baseline) earns at most $1$ bit; correctly predicting a rare `1` that occurs once every 64 ticks earns $\approx 6$ bits; predicting a constant `0` stream where the null model already expects `0` earns **identically zero bits**.

#### Concrete Toy Example: How Internal Redundancy Catches a Shallow Theory Offline

To see why *redundant derivations* matter even on a single input channel, consider a binary stream ($k = 1$) that repeats the length-6 motif `1, 1, 0, 1, 1, 0, ...` except on rare phase transitions, or suppose a learner holds two candidate predictors for channel $A$:

- **Pathway 1 (shallow lag-2 rule):** Predicts $\hat{A}(t+1) = A(t-1)$.
- **Pathway 2 (deeper lag-6 rule):** Predicts $\hat{A}(t+1) = A(t-5)$.

During short or noisy wake windows—for instance, while the stream temporarily sits inside an alternating `0, 1, 0, 1` segment—both rules agree with the data (`A(t-1) == A(t-5)` on period-2 segments). However, if external input is switched off (**sleep loopback**) and Pathway 2 drives the loop with a period-6 seed state `1, 1, 0, 1, 1, 0`, Pathway 1 immediately predicts `1` where Pathway 2 predicts `0`. Without observing a single new external symbol, running held derivations against each other in closed loop exposes their logical incompatibility.

One crucial caveat applies: internal checking can **only** detect errors that contradict another structure the learner already holds. If the entire circuit shares a single blind spot (for example, an accidental correlation between two channels that always held in passive observation), sleep cannot expose it; only new external facts or active interventions (`M6`) can.

### 1.3 Two Routes to Deduction and Explicit Non-Claims

The Charter frames learning not as approximate curve-fitting, but as **deduction** along two complementary routes:

- **Wake (deduction from facts under a class axiom):** Classic philosophy notes that finitely many observations cannot uniquely determine an arbitrary infinite sequence ("the problem of induction"). In computer science, however, if one assumes a **class axiom**—that the generator belongs to the class of finite-state circuits constructible from a known primitive set within a size bound—finitely many observations *can* lock onto the exact minimal generator. Classic examples include the **Berlekamp–Massey algorithm** (which deduces the exact minimal length-$n$ linear feedback shift register from just $2n$ symbols) and **Gold's identification in the limit**. If a later observation violates the deduced circuit, fallibilism triggers revision.
- **Sleep (deduction in isolation within held axioms):** With boundary inputs disconnected, the learner explores consequences of the circuits it already holds—testing internal consistency via loopback (`M4a`), searching for equivalent but smaller rewrites (`M4b`), and compiling verified subgraphs into new atomic operations (`M4b`/`M4c`).

Equally important for a grounded review are the Charter's **explicit non-claims** ([Charter §1.6](foundational_research_charter_v3.md)):

1. **The meta-level is hand-engineered.** Only the *object level* (the six arithmetic circuit primitives) is minimal. The *meta-level*—the algorithms for residual mutual-information diagnosis, retrograde sensitivity, energy accounting, pruning, and sleep scheduling—is explicitly engineered in Rust (`src/structural/`). Bootstrapping has been *moved* into a compact set of meta-rules, not magically eliminated. Making those meta-rules themselves representable inside the circuit substrate ("closing the meta-level") is a distant long-term horizon, not a current capability.
2. **There are no continuous "strange attractors."** Early project drafts used dynamical-systems metaphors loosely. Because a circuit with $N$ registers of $k$ bits each has at most $2^{kN}$ distinct states, deterministic autonomous execution is a map on a finite set: **every trajectory is eventually periodic** (a transient leading into a limit cycle). Even IEEE 64-bit floats have at most $2^{64}$ states per variable. Arkhe therefore uses exact finite-state dynamical quantities: cycle length, transient length, attractor basin fraction, and input entrainment.
3. **Every mechanism is a falsifiable hypothesis.** Nothing in the Charter is assumed to work by plausibility. Each mechanism (`H1`–`H10`) is paired with pre-registered pass thresholds and explicit **kill or revisit criteria**—and, as detailed in Section 4, several initial designs did fail those criteria and required foundational redesigns.

---

## 2. Primer on Core Technical Concepts

Because Arkhe draws simultaneously from digital logic, abstract algebra, information theory, computational mechanics, and programming-language verification, this section unpacks the four core technical pillars at an undergraduate level.

### 2.1 The Exact Substrate: Modular Rings $\mathbb{Z}_{2^k}$ and the Six Primitives

#### Why Exact Modular Integers Instead of Floating-Point Numbers?

Every wire in an Arkhe circuit carries an unsigned $k$-bit integer in the ring of integers modulo $2^k$:

$$\mathbb{Z}_{2^k} = \{0, 1, 2, \dots, 2^k - 1\}$$

where addition, subtraction, and multiplication wrap around modulo $2^k$ (just like unsigned integer arithmetic in hardware, e.g., mod $2$ for 1-bit Booleans, mod $4$ for $k=2$, mod $256$ for $k=8$).

Floating-point numbers (`f32`/`f64`) are strictly forbidden inside the circuit substrate for three reasons:

1. **Exact verification requires decidable equality:** Floating-point addition is **non-associative**: $(a + b) + c \ne a + (b + c)$ due to rounding. If the logical layer reorganizes a circuit sum during compression, a float circuit changes its numerical output, breaking exact equivalence proofs.
2. **Tolerance-based agreement is not transitive:** Defining "agreement" as $|a - b| < \varepsilon$ allows $A \approx B$ and $B \approx C$ while $A \not\approx C$, corrupting the asymmetry principle.
3. **Boolean logic is already modular arithmetic:** At width $k = 1$ ($\mathbb{Z}_2 = \{0, 1\}$), modular addition `(a + b) mod 2` is literally `XOR`, and modular multiplication `(a * b) mod 2` is literally `AND`. Increasing $k$ from $1$ to $2, 4, 8, \dots$ keeps Boolean logic as the exact $k=1$ special case while giving the learner **graded error magnitudes** at $k > 1$: if the true value mod $8$ is `7` and the circuit predicts `6`, the circular discrepancy $\min((a-b)\bmod 2^k, (b-a)\bmod 2^k) = 1$ reveals *how close* the prediction was, whereas at $k=1$ every error is just `1`.

#### The Six Level-0 Primitives and Functional Completeness

| Primitive | Arity (In $\to$ Out) | Mathematical Definition over $\mathbb{Z}_{2^k}$ | Role in the Learner | Equivalent at $k=1$ ($\mathbb{Z}_2$) |
| :--- | :---: | :--- | :--- | :--- |
| **`ADD`** | $2 \to 1$ | $y = (a + b) \bmod 2^k$ | Combining signals; stacking residual corrections | `XOR` |
| **`SUB`** | $2 \to 1$ | $y = (a - b) \bmod 2^k$ | Computing residual error ($y = 0 \iff a = b$) | `XOR` |
| **`MUL`** | $2 \to 1$ | $y = (a \cdot b) \bmod 2^k$ | Nonlinear interaction, coincidence, and gating | `AND` |
| **`EQZ`** | $1 \to 1$ | $y = 1$ if $x = 0$, else $0$ | Exact equality-to-zero indicator; logical inversion; restores completeness | `NOT` |
| **`DELAY`** | $1 \to 1$ | $y(t) = x(t-1)$ (with initial state `init`) | Unit memory register ($z^{-1}$); the **only** state-holding primitive | `DELAY` (flip-flop) |
| **`SPLIT`** | $1 \to 2$ | $y_1 = x,\ y_2 = x$ | Explicit signal fan-out so wire duplication pays a description cost | `SPLIT` |

Three subtle mathematical points explain why this exact set of six primitives was chosen:

1. **Why `EQZ` is mathematically necessary at $k > 1$:** Over $\mathbb{Z}_2$ ($k=1$), every truth table can be written as a polynomial using `XOR`, `AND`, and `1` (Algebraic Normal Form). Over $\mathbb{Z}_4$ ($k=2$) or any $\mathbb{Z}_{2^k}$ with $k > 1$, **polynomials built from `ADD`, `SUB`, and `MUL` alone cannot express all functions!** For example, over $\mathbb{Z}_4$, any polynomial $P(x)$ satisfies $P(x + 2) \equiv P(x) \pmod 2$—meaning no pure polynomial can map `0 -> 1` and `2 -> 0`. Adding the zero-test primitive `EQZ` restores **functional completeness**: first, the constant `1` can be constructed from *any* wire $x$ via `EQZ(SUB(x, x))` (since $x - x = 0$, and $\text{EQZ}(0) = 1$); repeated `ADD` builds any constant $a \in \mathbb{Z}_{2^k}$; and Lagrange-style indicator sums $f(\mathbf{x}) = \sum_{\mathbf{a}} f(\mathbf{a}) \prod_i \text{EQZ}(x_i - a_i)$ can then represent *every* possible finite-state transition function over $\mathbb{Z}_{2^k}$.
2. **Why `EQZ(SUB(a, b))` is the comparator, not `MUL`/`AND`:** An early v1 draft suggested `AND` detects agreement between two pathways. That is a logic bug: `AND(0, 0) = 0`, so `AND` only detects simultaneous `1`s (co-activation), missing agreements on `0` (or on `2` and `3` when $k=2$). By contrast, `SUB(a, b)` equals `0` *if and only if* $a = b$, so `EQZ(SUB(a, b))` outputs `1` on exact agreement and `0` on any disagreement across every bit width $k$.
3. **Why `SPLIT` is an explicit primitive:** In hardware diagrams or standard code, reading a variable ten times seems "free." In a growing graph economy, free fan-out encourages dense "spaghetti" wiring where every node taps every other node. Requiring an explicit 1-to-2 `SPLIT` primitive (inspired by Girard's linear logic) means duplicating a signal adds a node and two output ports, automatically increasing the circuit's bit description cost (`split_code_tax`).

#### Synchronous Tick Invariants

Every circuit (`substrate::Circuit` in `src/substrate/circuit.rs`) obeys five hard structural invariants checked on every edit:

- **Synchronous ticks:** At tick $t$, all combinational primitives (`ADD`, `SUB`, `MUL`, `EQZ`, `SPLIT`) evaluate in topological order in $O(N)$ time, and all `DELAY` registers latch their new values simultaneously for tick $t+1$.
- **No combinational loops:** Every directed cycle in the graph must contain at least one `DELAY` (or unit-hop wire delay). Cyclic combinational feedback (like an unclocked `NOT` wired to itself) has no well-defined synchronous value and is rejected at proposal time.
- **Coordinate-free point-to-point graph:** Nodes have no $(x, y)$ grid coordinates; wires connect source ports directly to sink ports (with an optional controlled mode `TicksPerHop` where every wire hop also carries 1 tick of propagation delay).
- **Single output driver per boundary port:** Each channel's prediction output port is driven by at most one wire at a time; stacking a residual corrector splices an `ADD` node cleanly onto that output wire.
- **Explicit `SPLIT` fan-out:** Every source port drives at most one edge; feeding $m$ destinations requires $m - 1$ `SPLIT` nodes, while leaving an output port unconnected ("discarding") is allowed and free.

### 2.2 Measuring Task Difficulty: Computational Mechanics ($\varepsilon$-Machines) and LFSRs

If you claim a learning algorithm "solved a hard sequence" or "found a compact circuit," how do you measure difficulty objectively rather than by intuition? Arkhe uses two exact mathematical yardsticks implemented in `src/env/`:

1. **Crutchfield's $\varepsilon$-Machine (Computational Mechanics):**
   Every benchmark environment in Arkhe is a stationary finite-state process. In information theory, two past histories of symbols belong to the same **causal state** if they induce the exact same conditional probability distribution over all future sequences. The minimal unifilar Hidden Markov Model over those causal states is called the process's **$\varepsilon$-machine**, and it yields three exact ground-truth numbers:
   - **Statistical complexity ($C_\mu$, in bits):** The Shannon entropy of the stationary causal-state distribution—the *strict information-theoretic minimum* number of memory bits any optimal predictor must retain. Since each `DELAY` register at width $k$ holds $k$ bits of state, any exact predictor circuit must have effective state bits $\ge C_\mu$.
   - **Entropy rate ($h_\mu$, in bits/symbol):** The irreducible randomness per step given infinite past history. For a clean deterministic generator ($\varepsilon = 0$), $h_\mu = 0$ bits/symbol; under declared symbol-substitution noise $\varepsilon > 0$, $h_\mu > 0$ sets the exact Bayes-optimal error floor that no predictor can beat.
   - **Synchronisation order ($L_{\text{sync}}$):** How many past symbols an ideal observer must see from cold start before locking onto the hidden state.
2. **Berlekamp–Massey and Reeds–Sloane Synthesis:**
   For Linear Feedback Shift Register (LFSR) sequences—streams generated by linear recurrences $s(t) = c_1 s(t-1) + \dots + c_L s(t-L) \bmod 2^k$—the Berlekamp–Massey algorithm (for $k=1$) and the **Reeds–Sloane algorithm** (its generalization to rings $\mathbb{Z}_{2^k}$ with zero divisors, using Newton–Hensel lifting) compute the exact minimal shift register length $L$ after just $2L$ symbols.

> **Why LFSRs are a classic search trap (the Parity Barrier):** An $L$-tap binary LFSR such as $s(t) = s(t-11) \oplus s(t-9) \oplus s(t-4)$ is trivial to *represent* in Arkhe (`11` `DELAY`s and `2` `ADD`s). Yet if an incremental learner tests candidate features one or two at a time, *any proper subset* of the true XOR taps has **zero correlation** with $s(t)$—it predicts at `50.0%` coin-flip chance! In computational learning theory (Kearns, 1998), parity functions are provably exponential for local statistical-query learners unless intermediate sub-parities are exposed first as a curriculum (`T3c`) or observed across spatial channels (`T2`/`T3a`). For similar reasons, predicting the single center column of Stephen Wolfram's Rule 30 cellular automaton (used as a cryptographic-grade pseudorandom generator) or computationally universal Rule 110 from 1 bit per tick is explicitly excluded from the Charter benchmarks.

### 2.3 Minimum Description Length (MDL) and Prequential Calibrated Reward

How should a local circuit economy trade off **accuracy** (making fewer mistakes) against **simplicity** (not growing giant Rube Goldberg circuits)? Arkhe grounds its entire metabolic economy in Rissanen's **Minimum Description Length (MDL)** principle: *the best model is the one that minimizes the total number of bits required to transmit both the model itself and the observed data errors given the model.*

#### Circuit Description Length $L(\text{circuit} \mid \text{palette})$

In `src/metrics/mdl.rs` (`circuit_code_bits`), a circuit with $N$ nodes and $M$ wires at bit-width $k$ has an exact prefix-free code length in bits:

$$L(C) = \underbrace{\gamma_{\text{Elias}}(N) + \gamma_{\text{Elias}}(M)}_{\text{graph size header}} + \underbrace{\sum_{i=1}^{N} \left(\log_2 |\mathcal{P}| + \mathbb{I}[\text{Delay}_i] \cdot (1 + k \cdot \mathbb{I}[\text{init}_i \ne 0])\right)}_{\text{node opcodes + initial register values}} + \underbrace{M \cdot \log_2(P_{\text{src}} \cdot P_{\text{dst}})}_{\text{point-to-point wiring table}}$$

where $|\mathcal{P}| = 6 + N_{\text{macros}}$ is the primitive palette size ($\log_2 6 \approx 2.585$ bits per Level-0 node), and $P_{\text{src}}, P_{\text{dst}}$ are the total available source and destination ports in the circuit. Notice that **zero arbitrary penalty weights appear in this formula**: adding a `SPLIT` node adds $\log_2 |\mathcal{P}|$ opcode bits plus two output ports and an extra wire, naturally charging $\ge 6.58$ bits for every wire branch (`split_code_tax`) simply because a decoder would need those bits to reconstruct the graph!

#### Prequential Krichevsky–Trofimov (KT) Forecasting and Ville's Bound

To convert discrete point predictions $\hat{s}(t) \in \mathbb{Z}_{2^k}$ into bit-valued energy rewards online without knowing the future horizon $T$:

1. Before observing the true symbol $s(t)$, the output port tracks how many past predictions matched ($a$) vs. missed ($b$), forming the **Krichevsky–Trofimov (Dirichlet-$1/2$) online accuracy estimate**:
   $$\hat{q}(t) = \frac{a + \tfrac{1}{2} \cdot 2^k p_{\text{null}}}{a + b + \tfrac{1}{2} \cdot 2^k}$$
   clamped from below at the null-model probability $q(t) = \max(\hat{q}(t), p_{\text{null}}(t))$, where $p_{\text{null}}$ is estimated online from symbol frequencies (e.g., $2^{-k}$ on uniform streams, or $\approx 0.98$ if the stream is almost always `0`).
2. When $s(t)$ arrives:
   - **Exact match ($\hat{s}(t) = s(t)$):** The port earns $+\log_2 \frac{q(t)}{p_{\text{null}}(t)} \ge 0$ bits.
   - **Mismatch ($\hat{s}(t) \ne s(t)$):** The port loses $\log_2 \frac{1 - q(t)}{1 - p_{\text{null}}(t)} \le 0$ bits.

Why use this specific log-likelihood-ratio formula?

- If a circuit is no better than chance ($\hat{q} \le p_{\text{null}}$), then $q(t) = p_{\text{null}}(t)$, so **both $\log_2(q/p_{\text{null}})$ and $\log_2((1-q)/(1-p_{\text{null}}))$ equal $0.0$!** A constant guesser or broken predictor earns *strictly zero* bits on every tick, eliminating random-walk luck streaks.
- By **Ville's martingale inequality** in probability theory, the probability that *any* non-predictive sequence of guesses ever accumulates $+\Delta$ bits of luck across *an infinite time horizon* is at most $2^{-\Delta}$ (e.g., accumulating `20` bits of pure luck has probability $< 10^{-6}$).
- Summing these prequential bit rewards minus one-time node description debts $\Delta L_i$ yields an **exact conservation law** ([Charter §3.4](foundational_research_charter_v3.md)):
  $$\Delta E_{\text{total}}(T) \equiv -\Delta L_{\text{null}}^{\text{epistemic}}(T)$$
  The local energy gained by all nodes in the circuit is algebraically identical to the global description-length compression achieved over the null model. Any discrepancy $> 10^{-11}$ bits/symbol in a test run immediately flags an implementation bug in credit routing.

### 2.4 Two-Layer Separation and Equality Saturation (E-Graphs)

Early artificial-life work by Fontana and Buss (1994), called **Algorithmic Chemistry ("AlChemy")**, modeled $\lambda$-calculus functions colliding in a well-mixed reactor (`f` applied to `g` produces `f(g)`), observing self-maintaining algebraic families ("Level 1 organisations") and occasional higher-order compositions ("Level 2"). A recent replication by Mathis et al. (2024) showed that while Level 1 structures form readily, combining separate families into higher-level abstractions hits a severe **combination barrier**.

Arkhe avoids claiming that raw logic gates "chemically react." Instead, it cleanly separates:

- **The Structural Layer (Engineering):** Graph mutation, residual correlation diagnosis, metabolic credit, and pruning—the machinery that grows working circuits from data.
- **The Logical Layer (Deductive Alchemy, targeted in `M4b`/`M4c`):** Once circuits exist, composing them, finding algebraic identities (e.g., proving that two differently grown subcircuits compute the exact same function over $\mathbb{Z}_{2^k}$), and extracting minimal forms uses **equality saturation** over **e-graphs** (Willsey et al., 2021, *egg*). An e-graph is a compact data structure from compiler optimization and automated theorem proving that stores exponentially many equivalent algebraic rewrites of a circuit simultaneously in equivalence classes ("e-classes") without having to choose an irreversible rewrite order, then extracts the lowest-MDL circuit and compiles recurring verified subgraphs into new palette primitives.

---

## 3. Experimental Methodology and Scientific Governance

Exploratory machine-learning and evolutionary-computation projects are notoriously vulnerable to **researcher degrees of freedom**: tweaking magic constants until a seed passes, pooling easy tasks with broken tasks so average accuracy looks high, or silently re-running test sets. Before reviewing the milestones, it is essential to understand the five methodological guardrails enforced across the repository ([`repo-common.md`](instructions/repo-common.md), [Parameter Inventory](parameter_inventory.md), and [Charter §2.8](foundational_research_charter_v3.md)).

1. **Strict Exploratory vs. Confirmatory Separation and Feasibility Bounds:**
   Every milestone begins with an ungated **characterisation phase** on designated development seeds (`0..20` smoke or `20..220` characterisation) to measure runtime profiles, noise variance, and failure modes. Only *after* characterisation are pass/fail gate thresholds written into a milestone plan, and every threshold must cite either an analytical mathematical bound or a measured characterisation baseline showing it is achievable ("feasibility before targets").
2. **Zero Magic Numbers and Derived Scaling Laws:**
   Every numerical constant in the learner and verification suites is catalogued in [`parameter_inventory.md`](parameter_inventory.md) under one of six classes: *Derived*, *Milestone-Scoped Scaffolding*, *Pre-registered*, *Search bound*, *Compute guardrail*, or *Numerical guard*. Early milestones (`M1`) fixed a handful of *Milestone-Scoped Scaffolding* constants at single-channel, short-lag scale ($K=1, \tau \le 3$): for instance, diagnostic window $W_{\text{diag}} = 64$ ticks, base juvenile grace period $t_{\text{juv}} = 128$ ticks, trace discount $\gamma = 0.90$, and initial endowment $E_{\text{init}} = 4.0$ bits. When a later milestone expands a scale axis—such as lag depth $\tau = 12$, channel count $K = 4$, or multi-node comparator dilution—tuning constants empirically is forbidden. Instead, scaffolding scalars must be replaced by **mathematically derived scaling laws** with zero free parameters (preferring formulas that reduce identically to the earlier constant at $K=1, \tau \le 3$).
3. **Observable Limits and Hard Guardrail Exclusions:**
   Every search cap, reserve clip, or compute ceiling increments a named telemetry counter (`CandidateListTruncated`, `LagHorizonBound`, `SproutBudgetExhausted`, `SproutBlockedMaxNodes`). If a structural guardrail such as `max_nodes` ever binds (`SproutBlockedMaxNodes > 0`), the run is marked **`Excluded`**, and confirmatory gates require `excluded_cases == 0`.
4. **Regime Stratification, Per-Task FWER, and Finite Tail Metrics (`Charter §2.8`):**
   Following a key lesson in Milestone 3 (detailed in Section 4.6), **Clean ($\varepsilon = 0.0$) and Noisy ($\varepsilon > 0.0$) tasks are never pooled together**. Within each stratum of $M$ tasks (`StratifiedGateAudit` in `src/stats.rs`):
   - Every *individual* task must pass an exact binomial lower-tail test and an across-bank $\chi^2$/permutation homogeneity test at **Bonferroni family-wise error rate $\alpha_{\text{FWER}} = 0.01 / M$** (so a single broken task can never hide inside a 95% pooled average).
   - Clean strata additionally require per-seed conjunction across all clean tasks.
   - Every task records finite worst-case and quantile **Option D `tail_metrics`** (e.g., worst-case regret, 5th/95th percentile MDL advantage, maximum excision latency).
5. **Sealed Single-Shot Holdout Banks and Clean-Tree Regression Gates:**
   Randomness uses portable, domain-separated `ChaCha8Rng` streams (`SeedBank`), embedding Git commit SHA, dirty-tree status, and 128-bit FNV-1a hashes of the entire `src/` tree and `Cargo.lock` into every JSON report. Confirmation and holdout sweeps use disjoint **100-seed (`5 × 20`) banks**. A milestone's holdout bank is opened **exactly once** on a clean commit after automated regression suites (`scripts/check_retained_baselines.py --require-clean`, including bit-identical `golden_traces`) pass; immediately upon closeout, the consumed holdout range (`10000..=10019`, `30000..=30019`, `40000..=40019`, `50000..=50019`, `50100..=50199`, `60000..=60099`) is hardcoded to `allow_with_flag: false` in CLI code so it can never be re-run accidentally.

---

## 4. The Milestone Journey (`M0` Through `M3`): Achievements, Dead Ends, and Redesigns

A realistic picture of Arkhe comes not just from the gates that passed, but from the hypotheses and engineering assumptions that *failed* along the way and forced deeper theoretical clarity.

### 4.1 Milestone 0 (`M0` — Simulator, Ground-Truth Solvers, and Early Mathematical Traps)

**Goal:** Build the deterministic $\mathbb{Z}_{2^k}$ circuit simulator (`1 <= k <= 64`, plus 64-lane bit-parallel evaluation at $k=1$), the $\varepsilon$-machine and Reeds–Sloane ground-truth solvers, and the baseline search suite (`RandomSearch`, `(1+\lambda)` Cartesian Genetic Programming `CGP`, Context-Tree Weighting `CTW`, and small-circuit `Exhaustive` enumeration).

**What was achieved & verified (`MILESTONE-0`):**

- The Reeds–Sloane $\mathbb{Z}_{2^k}$ linear-synthesis solver was cross-checked exhaustively against brute-force minimal shift registers across all **65,536** width-2 length-8 sequences and **4,368** width-2 recurrences without discrepancy.
- All four `M0` infrastructure gates passed on development (`0..19`) and initial holdout (`10000..10019`).

**What was learned (key traps uncovered):**

1. **The $k=1$ anti-correlation symmetry trap:** Standard Krichevsky–Trofimov sequence coding depends symmetrically on `(matches, mismatches)`. At width $k=1$ (binary), a circuit that is **100% wrong** (inverting every bit) is deterministic and therefore compressible to near 0 bits if you implicitly flip its output—causing `facts_code_bits` to score a 100%-wrong circuit identically to a 100%-right circuit! `facts_code_bits` had to be monotonised above the chance error floor $\lfloor T(1 - 2^{-k})\rfloor$ so worse-than-chance predictors never get a free logical `NOT` discount.
2. **The "Noisy Echo" local minimum and MDL sample crossover:** Consider predicting an alternating stream `0, 3, 0, 3, ...` over $\mathbb{Z}_4$ ($k=2$) corrupted by $\varepsilon = 5\%$ channel noise.
   - A lazy 1-node **reactive circuit** $\hat{x}(t+1) = 3 - x(t)$ simply echoes the noisy input: every single channel glitch causes *two* prediction errors (once when the glitch arrives, and again on the next tick when the glitchy input is echoed), giving a `~9.7%` error rate.
   - A 3-node **autonomous oscillator** (`0 <-> 3` closed `DELAY` loop) ignores noisy boundary glitches once synchronized and suffers only the irreducible `5.0%` channel error rate—saving `~0.24` bits per tick—but costs `~26` extra circuit bits to build.
   - Simple division ($26 / 0.24 \approx 108$, or `130–180` ticks with KT warmup) reveals a fundamental **MDL sample-size crossover**: over any window shorter than `~150` ticks, the worse reactive echo is *genuinely cheaper in total MDL* than the true autonomous model! This single calculation governed the sizing of observation windows ($W_{\text{obs}} = 256$ ticks) throughout all later milestones.
3. **State-count vs. node-count divergence on $\mathbb{Z}_4$:** For the period-3 motif `0, 1, 3, 0, 1, 3, ...` over $\mathbb{Z}_4$, the $\varepsilon$-machine statistical complexity is $C_\mu = \log_2 3 \approx 1.585$ bits, so 1 two-bit memory register (`2` bits $\ge 1.585$) suffices in principle. However, because $1 \equiv 3 \pmod 2$ while their next states $3 \not\equiv 0 \pmod 2$, a 1-lag combinational map requires **11 primitive nodes** (using `EQZ` to break mod-2 parity), whereas a 3-register pure shift chain (`2` `DELAY`s + `1` boundary lag = `6` state bits) needs only **2 nodes**! Minimum state ($C_\mu$) and minimum circuit size ($L(C)$) are distinct optima.
4. **Compute-metering asymmetry:** Analytical baselines like `CTW` and `ReedsSloane` do closed-form Bayesian tree updates or polynomial algebra rather than stepping candidate circuits, consuming `0` circuit-evaluation ticks if unmetered. They were explicitly tagged `AnalyticalUnmetered` and supplemented in `M2a` with step-metered `CrossChannelCtw` and `Rnn` baselines.

### 4.2 Milestone 1a (`M1a` — Metabolic Economy) and the First Kill-Criterion Redesign

**Goal (`H1`):** Before allowing any graph growth, test on **hand-built** optimal, partial, chance, wrong, and detour-bloated circuits whether local retrograde credit assignment sustains true circuits, starves non-contributors, and ranks circuits monotonically with global MDL.

**What broke (Charter Kill Criterion Triggered):**
In the initial `M1a` design, every node paid a small **per-tick maintenance rent** (`c_base = 0.020` bits/tick, `c_split = 0.035`), received **centered surprisal reward** $(\mathbb{I}[\text{hit}] - p_{\text{null}})(-\log_2 p_{\text{null}})$, propagated credit backward via **accumulating eligibility traces** ($e_t = \gamma e_{t-1} + s_t$ with $\gamma = 0.90$), and gated credit using a permanent `ever_active` flag.

When tested on `1,060`–`1,770` cases across `100` seed banks (`2,000` seeds), **this initial economy failed catastrophically**:

1. **Recurrent trace self-dealing (`G1A.2` kill trigger):** Inside a working circuit (`addc1`), a constant-1 generator loop (`DELAY(1) <-> SPLIT`) fed one arm of an `ADD` gate. Because eligibility traces accumulated across loop iterations as $\sum_{t=0}^\infty 0.90^t = \frac{1}{1 - 0.90} = 10.0$, the constant self-loop claimed `93.75%` of all incoming reward (`+150` bits), starving the essential downstream `ADD` gate to `-3.9` bits!
2. **The irreconcilable $O(T)$ rent vs. $O(1)$ MDL paradox (`G1A.4` failure across horizons):** Circuit description length $L(C)$ is an $O(1)$ constant (e.g., `50` bits regardless of whether the circuit runs for `1,000` or `1,000,000` ticks), whereas per-tick rent drains $O(T)$ energy proportional to run length. Sweeping every possible rent multiplier across `100` banks proved that **no constant rent rate works across horizons**: a multiplier tuned to pass `98/100` banks at $T = 1024$ fell to `72/100` at $T = 2048$ and **`0/100` (`0%`) at $T = 4096$**!
3. **Random-walk luck & the `onefire` parasite exploit:** Centered surprisal had mean zero for a coin-flip guesser, but its variance grew as $O(\sqrt{T})$, letting chance circuits win up to `+64.9` bits at $T=1024$ and `+101.8` bits at $T=4096$ by pure luck. Worse, an adversarial **"single-fire detour"** (`Add(TrueSignal, Delay{init:1}(ZeroLoop))`) emitted a single non-zero impulse at tick `0` and `0` forever after; under a permanent `ever_active` flag, that tick-0 impulse permanently unlocked `220–800` bits of leaked credit across `9/9` tasks!

**How it was solved (The `D1`–`D4` Redesign, Charter v2 $\to$ v3):**
Rather than tuning magic rent numbers, four structural changes aligned micro-economics with information theory:

- **`D1` (Replacing traces $\le 1.0$):** Clamping ancestry eligibility weights at `1.0` stopped recurrent loops from multiplying their own credit share.
- **`D2` (Calibrated point-forecast reward):** Replaced centered surprisal with the KT log-likelihood ratio (Section 2.3), forcing constant/chance guessers to `0.0` bits on every seed and bounding luck via Ville's inequality.
- **`D3` (One-time maturity description debt):** Eliminated per-tick rent completely; each node is charged a one-time debt $\Delta L_i$ upon graduating from its juvenile period such that $\sum_i \Delta L_i = L(C)$.
- **`D4` (Exponential activity recency decay $\gamma = 0.90$):** Identity-emitting inputs (`+ 0` or `* 1`) decay their sensitivity share by $0.90^t$ per idle tick, capping the `onefire` parasite's total lifetime leak to a finite constant independent of $T$.

**Result:** $\Delta E(T) \equiv -\Delta L_{\text{null}}(T)$ became an exact identity across all horizons, and **`100/100` seed banks passed all four gates (`G1A.1`–`G1A.4`) simultaneously at $T = 1024$, $T = 2048$, and $T = 4096$**, as well as the single-shot `M1` holdout (`10000..=10019`).

### 4.3 Milestone 1b & Pre-M2 Readiness (`M1b` — Residual Neurogenesis and the Coordinate-Free Pivot)

**Goal (`H2`):** Starting from an empty 0-node circuit, grow multi-neuron predictors online without backpropagation.

#### How Residual Neurogenesis Works ("Boosting Over a Finite Ring")

Suppose channel $A$ emits symbols in $\mathbb{Z}_4$ ($k=2$).

1. **First neuron:** Diagnosis over a 64-tick window (`W_diag = 64`) checks whether any candidate primitive motif (e.g., `DELAY(A)`, `EQZ(A)`, or an autonomous `DELAY` ring) has significant mutual information with $A(t+1)$ and lowers prequential MDL. Suppose `y_1(t)` is placed.
2. **Second neuron (error-driven corrector):** Once `y_1` matures (`128` juvenile ticks), the learner inspects its modular residual error stream:
   $$r(t) = (A(t+1) - y_1(t)) \bmod 2^k$$
   If $r(t)$ is pure white noise, entropy is maximal and growth halts (`noise-floor halting`). If $r(t)$ still contains predictable structure, the learner synthesizes a corrector $\hat{r}(t)$ from delayed signals (causality requires at least one `DELAY` because $r(t)$ is only revealed at tick $t+1$) and stacks an `ADD` node at the output:
   $$\hat{y}(t) = (y_1(t) + \hat{r}_1(t) + \hat{r}_2(t) + \dots) \bmod 2^k$$
   This is exact discrete gradient-free **functional boosting** over the modular ring $\mathbb{Z}_{2^k}$ (analogous to Fahlman & Lebiere's 1990 cascade-correlation).

**What was achieved & learned in `M1b` and `Pre-M2`:**

- **Combinational algebraic shortcuts:** On the mod-3 counter task `0, 1, 2, 0, 1, 2, ...` over $\mathbb{Z}_4$, human engineers hand-built an 8-node circuit using a stateful constant loop. Left to itself, `M1b` synthesized a **4-node purely combinational solution**: $f(x) = (\text{EQZ}(x) + 2x) \bmod 4$ (`x=0 -> 1`, `x=1 -> 2`, `x=2 -> 0`), discovering an exact modular identity smaller than the reference design.
- **The 2D Spatial Congestion Deadlock (`DR #29–#30`):** Originally, Arkhe placed nodes on an $8 \times 8$ 2D grid (`LocalVonNeumann`), routing wires through adjacent empty cells using single-cell `SPLIT` relays. On multi-stage `M1b` tasks, wires converging on `BoundaryOut(0)` saturated the 3–4 neighbor cells around the output port, **blocking second-stage convergence on `3 out of 4` benchmark tasks** (`61.7%` accuracy vs. `100.0%` under direct graph wiring)! The 2D physical "breadboard" metaphor was an artificial geometric bottleneck, whereas MDL wire-table bits ($M \log_2(P_{\text{src}} P_{\text{dst}})$) already penalize excessive wiring cleanly. Under `DR #30`, 2D coordinates were permanently retired in favor of coordinate-free `substrate::Circuit`, verified bit-identical on `8/8` golden traces (`A1`).
- **Horizon-derived reserve caps (`DR #31`) & $O(K^3)$ multi-channel profiling:** Fixed `25.0`-bit energy caps (which clashed with sprouts costing `28.5` bits) were replaced with a horizon-derived ceiling $E_{\max}(v)$ proportional to $W_{\text{obs}} = 256$ ticks of calibrated drain, allowing circuits to starve and re-sprout within `20`–`399` ticks when the environment abruptly switches regimes (`tests/regime_shift.rs`). Hardened `M1` passed **`100/100` banks (`253,000` `M1b` cases, `0` failures)**.

### 4.4 Milestone 2a (`M2a` — Cross-Channel Predictive Coupling)

**Why `M2` was split into `M2a` and `M2b` (`DR #35`):**
Pre-M2 analysis uncovered a structural catch-22: Charter `M2` originally asked for both cross-channel prediction (`T2`, where channel $B$ depends on delayed nonlinear combinations of channel $A$) *and* self-checking comparators (`H3`). Yet under the `M1` economy, a redundant second pathway on an already-accurate port saves `0` boundary error bits while costing `40–137` circuit bits, guaranteeing `100%` starvation! Splitting `M2` into **`M2a` (cross-channel predictive coupling)** and **`M2b` (epistemic comparator economics)** isolated the two hypotheses cleanly.

**Mechanisms & Derived Scaling Laws in `M2a`:**

- **Progressive iterative lag deepening (`3 -> 6 -> 12`):** Searching all lags up to `12` upfront wastes budget and overfits noise on short tasks; `M2a` checks autonomous rings first, then deepens reactive lag bands `(3..=6]` and `(6..=12]` only if shallower bands stall.
- **Causal `TicksPerHop` arm alignment:** When every wire hop adds 1 tick of delay, combining a 1-node arm with a 3-node arm skews signal arrival times; `M2a` derived the exact causal arm lag $d_{\text{arm}} = 1 + h_{\text{arm}} + 2 n_{\text{delay}}$ and inserted delay alignment automatically.
- **Derived scaling laws (`Rec 1-a`–`4-a`):** Multi-channel node guardrail scaled linearly as `32 * K`; deep-cone juvenile maturation scaled as $t_{\text{juv}}(d_{\text{cone}}) \propto \gamma^{-(d_{\text{cone}}-3)}$ to compensate for retrograde $\gamma^d$ trace attenuation; additive `0` inputs on active structural branches retained symmetric sensitivity credit; and candidate sprouts were barred from regressing sibling output ports sharing upstream wires.

**Empirical Results (`verify-m2a`, Holdout `30000..=30019`):**
Across `50/50` development banks (`1,000` seeds/task) and `20/20` single-shot holdout seeds (`0` exclusions), `Arkhe` solved linear and nonlinear (`MUL` + `EQZ`) delayed cross-channel couplings at $k \in \{1, 2\}$ under both `Combinational` and `TicksPerHop` delays and $\varepsilon = 5\%$ noise. Under a strict equal-compute budget (`200,000` replay/step ticks over $T = 2000$ on high-degree `deg 11 × deg 9` LFSR-driven channels, `G2A.5`), `Arkhe` achieved **`100.0%` post-warmup clean match (`0.0041` KT bits/symbol)** across all four linear and nonlinear tasks, whereas:

- `RandomSearch` and `(1+\lambda)` `CGP` scored `40%–83%` (`0.90–1.61` b/sym);
- `ReedsSloane` solved linear sums (`100%`) as expected, but collapsed to chance/near-chance (`23.8%` and `78.7%`) on nonlinear `MUL`/`EQZ` couplings;
- Metered `CrossChannelCTW` (`37.3%–92.9%` at $k=2$) and online BPTT `Elman RNN` (`25.4%–99.2%`) lagged substantially on $k=2$ modular wrap-around tasks (`CrossSum k=2`: `RNN` `25.4%` vs. `Arkhe` `100.0%`).

*(Statistical lesson from `M2a`: when `G2A.5` passed `983/1000` runs across 50 banks, the expected failure count per 20-seed bank was $0.34 \ll 5$, invalidating Pearson's continuous $\chi^2$ approximation and motivating an exact `100,000`-replicate conditional permutation test in `src/stats.rs`.)*

### 4.5 Milestone 2b (`M2b` — Redundant Derivations and Counterfactual Comparators)

**Goal (`H3`):** Enable the learner to grow causally independent second derivations of an already-predicted port, wire them into `EQZ(SUB(primary, alt))` comparators, detect contradictions in offline loopback, and survive sudden input-channel faults.

**How the Redundancy Funding Paradox Was Solved (`DR #40–#42`):**
Theoretical modeling in `M2b` showed why three naive reward schemes fail:

1. *Pure single-port boundary MDL:* Starves `100%` of backup pathways on clean streams because boundary error is already `0`.
2. *Sleep-only comparator reward:* Any backup pathway grown during wake hits juvenile graduation (`128+` ticks) *before* sleep runs and dies with negative energy ($4.0 - \Delta L_{\text{red}} < 0$).
3. *Rewarding `alt(t) == primary(t)` blindly during wake:* Allows two disconnected oscillators that happen to agree with each other to fund a shared hallucination against observed facts, and *penalizes* a healthy backup route the moment the primary channel breaks!

`M2b` resolved all three via **Epistemic Comparator Economics**:

- **In Wake:** A registered comparator scores its alternative derivation $\hat{y}_{\text{alt}}(t)$ prequentially **against observed ground-truth fact $s_p(t)$** under port $p$'s null model, routing credit *exclusively* to the `alt + SUB + EQZ` cone (never double-paying the primary cone), while enforcing **strict causal-support disjointness** (zero shared interior nodes and disjoint `(BoundaryIn(c), lag)` leaves, preferring a separate input channel over a deeper lag on the same channel).
- **Maturation dilution law:** Because `SUB` and `EQZ` add `2` downstream nodes sharing comparator flux with $Z_{\text{alt}}$ backup nodes, juvenile maturation scales by the exact zero-parameter dilution factor $(Z_{\text{alt}} + 2)/Z_{\text{alt}}$ (bounded by an anti-noise/anti-parasite ceiling $2 W_{\text{obs}} = 512$ ticks).
- **In Sleep:** Boundary reward shuts off (`0.0`); comparators score internal agreement $\hat{y}_{\text{alt}}(t) = \hat{y}_{\text{primary}}(t)$ against frozen wake null statistics, discounted by channel overlap $\iota(a, b) = \frac{1}{|C_a \cap C_b| + 1}$ on agreements (`1.0` on independent channels, `0.5` on same-channel lag redundancy) and full weight `1.0` on disagreements.
- **Dual-timescale fault recovery:** If a primary channel suddenly corrupts in wake (`G2B.5`), fast `W_diag = 64`-tick log-odds comparison hands the output port over to the healthy backup route (`driver_handoff_events`), while irreversible pruning waits for `W_obs = 256` ticks (or 1-sided bankruptcy) so `5%` stationary noise does not cause sprout/prune thrashing.

**Empirical Results (`verify-m2b`, Holdout `40000..=40019`):**
Passed `1,000`-seed multi-bank dev and `20/20` holdout across all five gates (`G2B.1` conservation `1.895e-15` b/sym; `G2B.2` `100%` spontaneous disjoint comparator formation; `G2B.3` `100%` offline loopback contradiction detection; `G2B.4` `98.9%` 5%-noise solvency; `G2B.5` `100%` channel-fault handoff beating `redundancy_sprouting = false` by `+1.43` bits/symbol).

### 4.6 Milestone 3 (`M3` & `M3 v2` — Free-Rider Resistance and the Pooled-Metric Lesson)

**Goal (`H4`):** Test whether the learner resists **credit free-riders and structural bloat** (`D0`–`D7b`: dead branches, `+ 0` adders, single-fire `onefire` impulses, `* 0` and `* 1` gates, cancelling pairs `(x + c) - c`, double-inversion involutions `EQZ(EQZ(x))` or `-(-x)`, fake disconnected comparators, and redundant agreement gates) injected in both mature (`Established`) and `Juvenile` modes, and whether spontaneous circuits stay bounded over long horizons ($T = 8000$).

**What broke in the baseline pruning rule (`BaselineD1b1`):**
Paired ablation (`explore-m3`) revealed that simple single-node zero-sensitivity pruning (`BaselineD1b1`) **failed on `85.9%` (`1,700 / 1,980`) of established detours** and caused **`1,300 / 1,980` catastrophic primary-cone collapses** on juvenile detours! Why? Think about a cancelling pair `y = (x + 1) - 1` or a mod-4 negation involution `y = -(-x) mod 4`: *every single node inside that detour has 100% counterfactual sensitivity!* If you perturb the output of the first `-x` node, the final output changes—so local sensitivity sees both nodes as "essential contributors," allowing arbitrary cancelling chains to survive forever!

**How `M3` solved transparent bloat (`M3-E1` + `M3-E2`):**

- **`M3-E1` (Target-anchored reachability & `W_diag` ring-identity operator bypass):** Anchors sensitivity strictly to paths reaching `BoundaryOut` or a registered `ComparatorSite`, and cleanly splices out any mature operator arm that emits an additive identity (`0`) or multiplicative identity (`1`) across all `64` diagnostic frames without disconnecting downstream output ports.
- **`M3-E2` (Verified local pass-through excision):** Whenever a mature interior node $v$ empirically equals one of its own upstream ancestors $x$ across `W_diag = 64` frames (`v(t) == x(t)`), the surgery engine runs an **exact local equivalence check**—exhaustive truth-table enumeration over all $(2^k)^{|\mathcal{I}_{\text{free}}|}$ input combinations (for $\le 4$ local free inputs at $k \le 4$) or bounded joint-state certification—and rewires $v \to x$ iff $v \equiv x$ is formally proved and $L(C') < L(C)$.

This excised **`100%` of transparent established and juvenile detours** across all nine classes (`D0`–`D7b`). Realistically, note what `M3` proved on spontaneous long-horizon circuits (`G3.4` at $T = 8000$): it proved **zero post-settlement drift** ($N_{\text{primary}}(8000) \le N_{\text{primary}}(t_{\text{settle}})$) and **bounded constant-factor size** ($N_{\text{primary}} \le N_{\min} + 4$), *not* guaranteed convergence to the exact brute-force minimum node count (since non-pass-through global circuit minimization awaits `M4b` e-graphs).

**The Post-Closeout Audit Lesson (`M3 v1` $\to$ `M3 v2`):**
`M3` also produced the project's sharpest methodological lesson. `M3 v1` initially passed its 20-seed holdout (`50000..=50019`), passed bit-identical `golden_traces`, and reported `all_passed: true` on a 20-seed `verify-m2b` regression check (`G2B.2 = 180/200 = 90.0%`). A post-closeout review inspected the per-task breakdown of those `20` missed cases in `180/200` and discovered they were **not random noise**: a single 2-bit multi-view task (`MV_AddEqz_k2_Comb`) had dropped from `20/20` (`100%`) to **`0/20` (`0%`)** (`0/100` across 5 banks) because a fast-settling optimization in `M3` Phase 1 skipped priming newly sprouted `DELAY` registers in comparator candidates!

Two governance vulnerabilities had allowed that bug through:

1. Pooling `10` tasks into a single `180/200 >= 90%` check let `9` tasks at `100%` mask `1` task at `0%`.
2. `golden_traces` (`8` single-channel `M1` fixtures) stayed bit-identical because none of them exercised multi-channel `M2b` delayed comparators.

Rather than sweeping the regression under the rug, `M3 v1` was formally **superseded**: the delayed-register priming bug was fixed (with careful guards against priming from cyclic upstream state without a matching snapshot), `Charter §2.8` (`StratifiedGateAudit` with Clean/Noisy separation, per-task FWER $\alpha = 0.01/M$, 100-seed confirmation/holdouts, and mandatory `check_retained_baselines.py --require-clean`) was enacted, old holdout `50000..=50019` was permanently sealed, and **`M3 v2` passed a fresh 100-seed holdout (`50100..=50199`: `46,842 / 46,900` cases, `6/6` gates, `G2B.2` restored to `1,000 / 1,000`)**.

---

## 5. Deep Dive into Milestone 4a (`M4a` — Sleep as Internal Falsification)

### 5.1 What Milestone 4a Set Out to Test (`H5` and `H6`)

Up through `M3`, offline loopback (`run_offline_loopback`) existed only as an isolated unit test from `M2b`. The learner had never alternated between **wake** and **sleep** during an online lifetime, nor paid an opportunity cost for sleeping. Milestone 4a ([`milestone_4a_plan.md`](milestone_4a_plan.md)) tested two Charter hypotheses across a 36-task suite (`18` Clean at $\varepsilon = 0.0$ and `18` Noisy at $\varepsilon = 0.05$, spanning widths $k \in \{1, 2\}$ across Spontaneous `SP`, Planted-error `PL`, and Control `CT` families):

- **`H5` (Sleep filtering vs. compute-matched wake-only):** When every arm is given the exact same total budget of synchronous substrate ticks ($T_{\text{total}} = 8,000$ on `SP`/`CT`, $2,000$ on `PL`), and **the external environment is paused during sleep** (so a sleeping learner *forgoes* real external observations that the wake-only control gets to see), does periodic sleep (`W_wake = 256`, `W_sleep = 256`, triggered only when at least one grounded comparator exists) produce a superior model (lower post-episode epistemic MDL without losing boundary prediction accuracy)?
- **`H6` (`S-A` Consistency-Only vs. `S-B` Consistency + Fact-Buffer Replay):**
  - **Arm `S-A` (`ConsistencyOnly`):** Spends all `256` sleep ticks running free-running closed-loop loopback ($\hat{A}(t+1) \to A(t+1)$), pruning on internal comparator disagreement.
  - **Arm `S-B` (`ConsistencyReplay`):** Maintains a bounded FIFO ring buffer of the most recent `128` (`W_obs / 2`) wake observations (`FactBuffer`), spending `128` sleep ticks replaying buffered facts teacher-forced (to adjudicate two-route disagreements and check statistics) and `128` sleep ticks in loopback (comparing free-running vs. replayed sequences via a zero-threshold order-$d_{\text{cone}}$ two-sample KT test).
  - Pre-registered decision rule (`G4A.7`): If `S-A` performs within `10.0` bits of `S-B` (or beats `S-B` by $> +10.0$ bits) while passing all gates, **select `S-A` as the canonical sleep schedule for `M4b` and drop `S-B` replay**.

### 5.2 Mechanistic Traps Discovered and Solved in `M4a-0` (`DR #49–#50`)

Following the exploratory-first protocol, `M4a` was split into ungated **`M4a-0` (infrastructure & 200-seed characterisation on `20..220`)** followed by confirmatory **`M4a`**. `M4a-0` exposed five subtle failure modes in naive "unplug the inputs and dream" designs—each instructive for anyone designing self-play or world-model dreaming systems:

1. **The Exogenous Driver Hallucination Trap (`F3` $\to$ `R4` Grounded-Loop Rule):**
   Suppose Channel 0 is a complex pseudorandom LFSR stream that the learner has *not* solved (so its Channel 0 predictor is still guessing at null chance level), while Channel 1 is a clean function of Channel 0 and Channel 2 that the learner *has* solved with two valid cross-channel derivations. If sleep blindly loops *every* output port back into its input port, Channel 0 is suddenly fed fabricated constant `0`s instead of LFSR bits! Under fabricated inputs, valid cross-channel comparators can disagree or starve, **destroying true, fact-supported circuits**.
   - *Fix (`R4`):* A boundary channel $c$ is **grounded** iff its wake predictor beats the null model ($\hat{q}_c > p_{\text{null},c}$) and passes cross-port consistency homogeneity. Any comparator whose transitive closed-loop input closure touches an ungrounded channel is **suspended** during loopback (`I7`: zero flux, zero excision; `0` violations across all runs).
2. **Noisy Sleep-Entry Transients (`R3` Prediction-Consistent Entry & `I3` State Restore):**
   On a `5%`-noise task, if sleep begins on tick `512` and tick `511` happened to suffer a random channel glitch, Reactive `DELAY` registers hold a corrupted state at sleep entry—sending deterministic loopback into a false transient! Furthermore, when sleep ends, internal `DELAY` registers have drifted along the dream trajectory and must not corrupt the next wake tick.
   - *Fix (`R3` & `I3`):* Sleep locates the latest prediction-consistent window ($d_{\text{cone}} + 1$ or $d_{\text{closed\_loop}} + 1$ ticks) inside the current epoch's `FactBuffer` to seed loopback cleanly (metering warmup steps), pauses juvenile countdowns during sleep (`I6`), and restores the exact end-of-wake register snapshot upon waking (remapped across any excised node indices).
3. **The Partial-Agreement Loophole at $k > 1$ (`R5(c)` Popperian Loopback Refutation):**
   Consider width $k = 2$ ($\mathbb{Z}_4$), where uniform wake null probability is $p_{\text{null}} = 1/4 = 0.25$. Suppose a planted spurious period-6 cycle agrees with the true period-6 primary on **5 out of 6 steps (`83.3%`)** and disagrees on **1 out of 6 steps (`16.7%`)**. Under wake-calibrated surprisal ($83.3\% \gg 25\%$), 5 agreements earn *more* positive bits than 1 disagreement loses—so a deterministic contradiction in noise-free loopback would actually **gain net energy and survive forever!**
   - *Fix (`R5(c)`):* In deterministic loopback (where external channel noise is zero by construction), Popperian asymmetry is absolute: positive sleep agreement flux is held in escrow across the sleep segment and **voided to zero if even a single deterministic loopback disagreement ($\ge 1$) occurs**, ensuring any contradicting orbit drains strictly and excises.
4. **Two-Route Reference Asymmetry and Solvency Guards (`F2` & `R5(a)`):**
   In two-route `S-A` loopback, `primary` acts as the reference (`alt` is tested against `primary`; replacing a wrong `primary` in `S-A` requires 3-way comparator majority, whereas `S-B` uses buffered wake facts to adjudicate which route contradicted reality). To prevent `5%` noise flukes in a 128-tick buffer from wrongfully excising a true primary in `S-B`, `R5(a)` requires zero-bit MDL solvency (`primary` net buffer bits $\le 0$ and `alt` net buffer bits $> 0$) before flipping culpability to the primary.
5. **Forgone Observations vs. Equal-Dataset Model Quality (`K2` Opportunity Analysis):**
   Over an 8,000-tick run with `15` sleep segments, a sleeping learner pauses the world and **forgoes up to `3,840` external observations** (`48%` of the stream). Because cumulative in-run MDL sums bit savings per observed fact, seeing `4,160` facts instead of `8,000` mechanically reduces cumulative bit totals even when the resulting circuit is strictly cleaner! `DR #50` therefore defined the canonical `H5` metric on a **frozen 256-tick post-episode continuation window** (`eval_l_epistemic_bits = L(C) + 256 * eval_KT`), measuring the MDL of the learned circuit on an equal number of post-training symbols.

### 5.3 Confirmatory and Sealed Holdout Results (`60000..=60099`)

Following pre-registration at commit `4370540` and implementation of `verify-m4a` at clean commit `0f62e2f` (`source_tree_fnv1a128 = 349fe16d144f4fcd2c2184fe944d6c51`), Arkhe executed three strictly disjoint confirmatory sweeps: `200`-seed development (`520..720`), `100`-seed confirmation (`20200..=20299`), and the **single-shot `100`-seed holdout (`60000..=60099`, now permanently sealed)**.

| Pre-Registered Gate | Meaning & Pass Requirement | Disjoint Dev (`520..720`, 200 seeds) | Confirmation (`20200..=20299`, 100 seeds) | Sealed Holdout (`60000..=60099`, 100 seeds) |
| :--- | :--- | ---: | ---: | ---: |
| **`G4A.1`** Accounting & Compute | Conservation $\le 10^{-9}$ b/sym, tick diff $= 0$, node-eval ratio $\le 1.0$, `0` exclusions (both arms) | `7,200 / 7,200` (`100%`) | `3,600 / 3,600` (`100%`) | **`3,600 / 3,600` (`100%`)** (max MDL err `1.31e-13`) |
| **`G4A.2`** Sleep Isolation | `None` bit-identical (`I1`), pre-onset bit-identical (`I2`), `0` wake-null/suspended leaks (`I6`/`I7`) | `7,200 / 7,200` (`100%`) | `3,600 / 3,600` (`100%`) | **`3,600 / 3,600` (`100%`)** (`0` violations) |
| **`G4A.3`** `H5` Spontaneous Advantage (`S-A`) | Post-episode epistemic MDL beats wake-only (`> 0` bits, `0` wrongful excisions) on the `M=3` positive-opportunity noisy `SP` tasks | `526 / 600` (`87.7%`, CP99 `0.842`) | `258 / 300` (`86.0%`, CP99 `0.807`) | **`264 / 300` (`88.0%`, CP99 `0.830`)** |
| **`G4A.4`** Prediction Non-Inferiority (`S-A`) | Frozen 256-tick target KT regret $\le 0.05$ (`Clean`, exact `100%`) and $\le 0.35$ (`Noisy`, target `85%`) across `28` `SP`+`CT` tasks | `5,599 / 5,600` (`99.98%`) | `2,799 / 2,800` (`99.96%`) | **`2,799 / 2,800` (`99.96%`)** (`Clean` `1,400/1,400` at `0.000` regret) |
| **`G4A.5`** Planted Spurious Rejection (`S-A`) | Excises or extinguishes planted short-cycle (`PL1`) & rare-context (`PL2`) spurious derivations in $\le 4$ sleep segments with `0` wrongful excisions | `1,580 / 1,600` (`98.8%`) | `786 / 800` (`98.3%`) | **`785 / 800` (`98.1%`)** (`Clean` `400/400`; `S-B` `766/800`) |
| **`G4A.6`** Fact-Supported Preservation (`S-A`) | `0` oracle-correct port locks lost (`100%` hard invariant) and `0` wrongful node excisions (`100%` `Clean`, $\ge 90\%$ `Noisy`) across all `36` tasks | `7,195 / 7,200` (`99.93%`, `0` locks lost) | `3,598 / 3,600` (`99.94%`, `0` locks lost) | **`3,599 / 3,600` (`99.97%`, `0` locks lost)** (`Clean` `1,800/1,800`) |
| **`G4A.7`** `H6` Schedule Decision (`S-B - S-A`) | Mean paired `eval_l_epistemic` bits saved by `S-A` over `S-B` across the `11` active falsification tasks (`> -10.0` bits selects `S-A`) | **`+27.65` bits** (`S-A` superior) | **`+21.03` bits** (`S-A` superior) | **`+14.61` bits** (`SaSuperiorDropReplay`) |

#### Why Did `S-A` Beat Wake-Only (and Beat `S-B` Replay)?

Two concrete mechanistic insights explain the numbers in the table above:

1. **Why sleep beats wake-only under noise (`G4A.3` & `G4A.5`):** During wake under $\varepsilon = 5\%$ channel noise, every comparator disagreement is clouded by random symbol corruption—wake must wait cautiously (`W_obs = 256` + solvency margins) before pruning, so spurious lag-2 or multi-view backup cones linger and bloat $L(C)$ by `55–140` bits. In `S-A` sleep loopback, external channel noise vanishes: the two held derivations run deterministically against each other on the exact same internal stream, triggering immediate Popperian refutation (`R5(c)`). On the 200-seed `Period6_LagRedundancy:k2:noisy` exploration, `S-A` executed **195 spontaneous wrong-comparator refutations vs. 0 in wake-only**, cutting circuit description cost by `57.12` bits on average (`+60.65` bits holdout evaluation advantage; `+117.32` and `+104.14` bits on `MV_Periodic` $k=1, 2$).
2. **Why `S-A` beat `S-B` replay on rare-context errors (`PL2_RareContext:k2:noisy`):** On `PL2:k2:noisy` (a planted backup circuit that agrees almost everywhere except on a rare state context), `S-A` rejected `85/100` holdout cases (`+145.69` bits ahead of `S-B`), whereas `S-B` rejected only `66/100` (and failed the per-task noisy gate on dev `55.5%` and confirmation `60.0%`). Because `S-B` splits its `256`-tick sleep budget in half—spending `128` ticks replaying noisy wake facts where the rare context may not even occur cleanly, leaving only `128` ticks for deterministic loopback—`S-A` gets **twice as many noise-free closed-loop steps per sleep segment** to hit the rare state and refute the bad circuit.

### 5.4 Grounded Technical Caveats: Where `M4a`'s Claims Stop

As synthesized in the [Independent Review of M4a](milestone_4a_independent_review.md), an objective reading of `M4a` requires stating clearly what these results **do not** prove:

1. **`H5` was established on three noisy periodic tasks, not across spontaneous learning in general.**
   During `M4a-0` characterisation (`20..220`), **all 8 Clean ($\varepsilon = 0.0$) spontaneous tasks had `0.0%` wrong-comparator opportunity at sleep onset**—because on clean data, wake's `M2b` exactness rules never sprouted an incorrect backup comparator in the first place! Without `M4b` equivalence rewriting/compression, falsification-only sleep has nothing to improve on an already-consistent clean circuit (6 of 8 Clean `SP` tasks had `0.0` mean advantage on holdout). Moreover, on the two noisy `MV_Periodic` tasks that *did* pass `G4A.3`, the 5th-percentile tails remained negative (`-38.76` and `-46.57` bits on seeds where sleep excised nothing while forfeiting wake observations), and the 2,000-tick scheduled-corruption stress test `ChannelFault:k2:clean` (which corrupts channel 0 with probability `0.75` after tick `1,000`) lost `564.89` bits on average in circuit overhead despite `0.0` target KT prediction regret.
2. **Selecting `S-A` (`H6`) drops teacher-forced replay; it does not mean recorded facts are unneeded.**
   `S-A` still uses the `FactBuffer` at sleep entry (`R3`) to locate a prediction-consistent register snapshot before starting loopback. Furthermore, `+14.61` bits is an operational win on the pre-registered `11`-task active mixture (driven largely by `PL2_RareContext:k2:noisy`, while the three spontaneous tasks alone were essentially tied at `-4.52`, `+2.57`, and `-1.86` bits). Keeping `S-B` in the codebase as a non-default diagnostic comparator remains useful.
3. **Substrate-tick parity is exact; the `R2` node-evaluation ratio helper has a denominator nuance.**
   `G4A.1` verified exact `0` substrate-tick difference between wake-only and sleep arms. However, the secondary `R2` node-evaluation ratio helper in `src/verify_m4a/explore.rs` divides by `sleep_live_work + wake_active_work` rather than raw `wake_total` whenever candidate-replay and counterfactual-sensitivity work do not exceed wake's—meaning a hypothetical increase in live-step node evaluations alone could clamp to `< 1.0` instead of flagging a raw node-work overrun. Future `M4b` harnesses should report both raw and envelope-normalized node-evaluation ratios explicitly.
4. **Equal-length continuation scoring vs. identical test trajectories, and event attribution.**
   Each arm's frozen post-episode circuit is scored on the next `256` symbols emitted by *that arm's* terminal environment clone. Because a sleeping arm paused the environment for up to `3,840` ticks, its generator phase and noise stream at episode end are offset relative to wake-only's terminal state; adding a synchronized, identical post-episode test trace in `M4b` will sharpen causal attribution between circuit bits, prediction bits, and corroboration bits. Similarly, `G4A.5` episode-level excision counts credit any planted error removed by episode end (including cases pruned during wake segments), so `M4b` should log exact phase-attributed timestamps (`present`, `sleep-refuted`, `proved`, `promoted`).
5. **Closeout housekeeping (`S4` and `M3` regression coverage).**
   Commit `3582242` sealed holdout `60000..=60099` (`allow_with_flag: false`) and passed clean-tree baseline checks (`golden_traces`, `M2a`, `M2b`, `M4a`), though archiving that post-closeout baseline JSON inside [`docs/evidence/m4a/`](evidence/m4a/) and recording formal `S4` sign-off in [`milestone_4a_plan.md`](milestone_4a_plan.md) remain administrative prerequisites before starting confirmatory `M4b` runs. Note also that [`docs/retained_baselines.json`](retained_baselines.json) runs `golden_traces`, `verify-m2a`, `verify-m2b`, and `verify-m4a`, but omits `verify-m3` for runtime reasons—so any `M4b` surgery or macro changes must explicitly run `verify-m3` to ensure free-rider resistance does not regress.

---

## 6. Cross-Cutting Technical Lessons Learned So Far

Stepping back from individual gate numbers, six engineering and scientific lessons from `M0`–`M4a` stand out for technical practitioners outside this domain:

1. **Continuous per-step penalties cannot price static structure across time horizons (`M1a`).**
   Whenever a learning system grows discrete structure (circuits, symbolic programs, or neural modules) over an unknown lifetime $T$, charging a per-tick metabolic "rent" creates an unsolvable scaling conflict: total rent grows as $O(T)$ while model complexity $L(C)$ is $O(1)$. Charging a **one-time description-length debt upon structural maturity** paired with **martingale-calibrated log-likelihood rewards** makes local energy balance an exact algebraic mirror of global Minimum Description Length across *all* horizons $T$ without a single tuned decay rate.
2. **Literal spatial metaphors can strangle graph growth where information-theoretic wire pricing succeeds (`M1b` $\to$ `Pre-M2`).**
   Placing logic gates on a 2D nearest-neighbor grid ("like biological tissue or a breadboard") forced empty cells to act as wire relays, rapidly congesting the perimeter around output ports and deadlocking learning. Removing spatial coordinates altogether—while charging exact Combinatorial Wire Table bits ($M \log_2(P_{\text{src}} P_{\text{dst}})$) and explicit `SPLIT` fan-out bits—preserved sparsity and prevented bloat without geometric gridlock.
3. **Self-checking redundancy dies under pure error-minimization unless corroborated epistemically (`M2b`).**
   A system optimized solely to minimize output prediction error will systematically strip away backup pathways the moment its primary pathway works—leaving it brittle and unable to detect internal contradictions offline. Sustaining healthy redundancy requires rewarding causally disjoint backup derivations against wake ground truth (isolated from the primary cone's budget) while enforcing channel-overlap discounts in offline sleep.
4. **Counterfactual sensitivity alone is blind to algebraic involutions and cancelling pairs (`M3`).**
   Asking "does wiggling node $v$'s output change the final answer?" catches dead wires and `* 0` branches, but fails completely on cancelling chains like `(x + c) - c` or `-(-x)`, where every intermediate node is `100%` counterfactually sensitive! Catching non-trivial structural bloat requires pairing local sensitivity with **verified local equivalence checking** ($v \equiv x$ over upstream cones).
5. **Pooled benchmark pass rates hide catastrophic single-task regressions (`M3 v1` $\to$ `M3 v2`).**
   Averaging success across `10` tasks allowed a `0/20` (`0%`) regression on a 2-bit delayed-comparator task to pass a `90%` (`180/200`) gate unnoticed, while bit-identical `M1` golden traces stayed green because they lacked multi-channel coverage. Separating Clean and Noisy regimes, testing *every* task at family-wise error rate $\alpha = 0.01/M$, requiring 100% Clean seed conjunction, and sealing 100-seed holdouts in CLI code eliminated that blind spot.
6. **Offline "dreaming" without external input requires strict grounding quarantine (`M4a`).**
   Unplugging a multi-channel world model and looping all predictions back into inputs is dangerous whenever some environment channels remain unlearned or noisy: hallucinated exogenous inputs propagate into valid subcircuits and trigger false refutations. Offline internal falsification works reliably only when ungrounded input closures are suspended (`R4`), entry states are prediction-consistent (`R3`), and deterministic disagreements void partial-orbit agreements (`R5(c)`).

---

## 7. Next Steps: Crossing From Pruning to Verified Vocabulary Growth (`M4b`–`M6`)

### 7.1 Assessing Progress Against the Core Research Question

Returning to [Charter §1.5](foundational_research_charter_v3.md), the overarching research question has four parts:

- **(a) Grow predictive circuits from minimal exact primitives without gradients:** *Established* across `M1b` and `M2a` for $k \in \{1, 2\}$, $K \le 4$, and lags up to $\tau = 12$.
- **(b) Cross-check predictions via redundant derivations and offline internal falsification:** *Established* across `M2b`, `M3`, and `M4a`, with clear characterization of where offline pruning helps (noisy and rare-context regimes) and where it has zero opportunity (already-consistent clean circuits).
- **(c) Deduce and formally verify new equivalences (theorems) in isolation:** *Not yet tested (`M4b`).*
- **(d) Compile verified theorems into higher-order axioms whose growing vocabulary transfers across task families:** *Not yet tested (`M4b` + `M4c`).*

Parts **(c)** and **(d)** are the true test of the Charter's "logical alchemy" thesis. Until a learned circuit is compressed via a formally proved equivalence into a reusable macro that earns back its own palette-definition bit cost, Arkhe is a principled, self-auditing discrete circuit learner—not yet an open-ended vocabulary builder.

### 7.2 Staged Engineering & Scientific Plan for Milestone 4b (`H7`)

Following the [Independent Review of M4a](milestone_4a_independent_review.md), jumping straight into a massive confirmatory e-graph sweep risks building complex search machinery before verifying whether spontaneously grown `M1`–`M4a` circuits actually contain recurring motifs large enough to repay macro-definition overhead under strict MDL accounting. `M4b` is therefore structured as a three-step gate:

1. **Step 1 — Development-Only Compression-Opportunity Census:**
   Before writing confirmatory gates, inspect the actual circuits grown spontaneously on Clean tasks at $k \in \{1, 2\}$. Catalogue repeated subgraphs with small port interfaces (Markov-blanket boundaries), measuring their multiplicity, internal state/latency, raw `circuit_code_bits` savings, and net epistemic MDL change *after* charging the exact bit cost of adding a new macro definition to the palette (`L(palette)` + call-site opcode bits). Also check whether sleep onset rules skip Clean circuits that have compression opportunity but zero comparator disagreements. If spontaneously grown circuits on current T1/T2 tasks are already too small (`4–14` nodes) for any macro to break even honestly, **do not cheapen macro bit pricing to force a win**—instead introduce structured multi-output or modular task families where repeated subcircuits genuinely recur.
2. **Step 2 — End-to-End Auditable Replacement and Independent Proof Checker:**
   Separate *candidate equivalence discovery* (equality saturation in `egg` over the $\mathbb{Z}_{2^k}$ ring/indicator algebra) from an **independent formal equivalence checker** that certifies every proposed rewrite before promotion ([Charter §4.3](foundational_research_charter_v3.md)):
   - For combinational subgraphs: exact exhaustive input-vector evaluation (for small arity) or bit-vector equivalence checking across all $(2^k)^m$ inputs—explicitly testing width-specific non-identities such as $x - y = y - x$, which is **true** at $k = 1$ (`XOR`) and **false** at $k = 2$ for $(1, 0)$!
   - For stateful subgraphs (`DELAY` and `TicksPerHop`): sequential equivalence over mapped register states and per-tick latency invariants (not merely eventual orbit matching on one generator).
   - Strict accounting invariants: every promoted macro stores a replayable proof certificate, expands losslessly back to Level-0 primitives for auditing, preserves causal-support disjointness (two macro calls sharing underlying wires cannot claim double comparator corroboration), and passes `verify-m3` free-rider regressions.
3. **Step 3 — Pre-Registered `H7` Confirmatory Experiment:**
   Once Steps 1 and 2 succeed on development seeds, pre-register a confirmatory three-arm comparison (**Wake-Only** vs. **Canonical `S-A` Sleep Without Consolidation** vs. **`S-A` Sleep With Metered E-Graph Consolidation & Promotion**) using fresh disjoint 100-seed confirmation and single-shot holdout banks:
   - Meter logical proof and e-graph search work explicitly rather than treating theorem proving as free wall-clock compute;
   - Score frozen models on a common, synchronized continuation sequence;
   - Separate `H7` (autonomous discovery, formal proof, and net MDL reduction of non-trivial theorems) from the deferred Clean-regime `H5` compression test.

### 7.3 Horizon Beyond `M4b` (`M4c`, `M5`, and `M6`)

If `M4b` demonstrates verified, MDL-profitable macro compilation, three final milestones complete the foundational Charter roadmap:

- **Milestone 4c (`M4c` — Cross-Family Transfer, `H8`):** Present sequences of related generators (`Tier 4` task families: e.g., cellular automata sharing neighborhood sums, or LFSR curricula sharing sub-parities) to test whether macros compiled on earlier tasks measurably shorten time-to-solution and compute budgets on unseen family members—directly testing whether verified logical interfaces overcome the AlChemy combination barrier.
- **Milestone 5 (`M5` — Bit-Width Scaling and Homomorphic Lifting, `H9`):** Scale uniform bit-width across $k \in \{1, 2, 4, 8\}$ (evolving fixed-$W_{\text{diag}}$ mutual-information estimators to handle larger $2^k$ alphabets without undersampling), then introduce heterogeneous bit-widths joined by `TRUNC` ($x \bmod 2^k$) and `EXT`. Because `TRUNC` is an exact ring homomorphism on the `{ADD, SUB, MUL, DELAY, SPLIT}` fragment, identities proved at width $k' = 8$ automatically hold after truncation at widths $4, 2, 1$, whereas `EQZ` identities require per-width re-verification.
- **Milestone 6 (`M6` — Active Intervention and Query Learning, `H10`):** Connect boundary output ports back to the environment generator so the learner can actively perturb or query the system. Passive observation (`M0`–`M5`) can never separate a structural law from an accidental correlation that happened to hold throughout historical data; active intervention completes the Charter's fallibilist epistemology by letting the learner test causal invariants directly in the style of Pearl's `do`-calculus and Angluin's $L^*$ query learning.
