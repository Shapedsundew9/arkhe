# Foundational Research Charter v3: Axiomatic Neurogenesis and Open-Ended Learning

> **Status:** Current authoritative research charter. Supersedes Charter v2 and v1.
>
> **Milestone Progress:**
>
> - **Milestone 0 (`M0 — Simulator and yardstick`):** Completed & verified. Exact synchronous tick semantics, variable-width arithmetic, seeded determinism, $\varepsilon$-machine and Berlekamp–Massey ground truth, and compute-matched baselines are operational.
> - **Milestone 1a (`M1a — Metabolic economy and retrograde credit`):** Completed & verified. Closed-loop metabolic economy with replacing ancestry traces (D1), calibrated point-forecast reward (D2), one-time description debt (D3), and activity recency decay (D4) passed all acceptance gates (`G1A.1`–`G1A.4`) on 100/100 development banks across $T \in \{1024, 2048, 4096\}$ and on the untouched holdout bank (`seeds 10000..=10019`).
> - **Milestone 1b (`M1b — First to second neuron residual neurogenesis`):** Completed & verified. Residual-driven neurogenesis, finite-ring corrector stacking, autonomous ring closure, and noise-floor halting passed holdout (`e29014a`, `seeds 10000..=10019`), 100-bank statistical hardening (`seeds 20..2020`), and coordinate-free migration (`substrate::Circuit`); see [research_record.md](research_record.md).
> - **Pre-Milestone 2 (`Readiness & Scaling Closure`):** Completed & verified. Coordinate-free `substrate::Circuit` refactor (`A1` golden bit-identity proof), unified `observation_horizon` (`ReserveCap::HorizonDerived`, DR #31), per-port developmental state (`PortDevelopmentalState`, DR #34), cliff/guardrail telemetry (`Excluded` semantics, DR #33), `B5`–`B7` empirical profiles, `A5` 100-bank + confirmation runs (`source_tree_fnv1a128 = 26e9d332905dd6a6aeadce50c5266fa3`), and parameter/governance consolidation ([parameter_inventory.md](parameter_inventory.md), [research_record.md §7](research_record.md#7-pre-milestone-2-readiness-measurements--verification-2026-11-13)) are complete.
> - **Milestone 2a (`M2a — Cross-channel predictive coupling`):** Next active stage (DR #35–#37); pre-registration complete and signed off in [milestone_2a_plan.md](milestone_2a_plan.md) (`S1`–`S5`, `S3-A`, `S4-A` / `S4-A.1` / `S4-A.2`, `C1-a`, `C2-a`, and `G2A.2(b)` ratified `2026-11-13`–`2026-11-14`; ready for Phase 1–4 execution).
>
> **Provenance & Decision Record:** Version 3 incorporates the Stage M1a metabolic economy and credit redesign decisions (D1–D4). [Appendix A](#appendix-a-decision-record) details the cumulative decision record across charter revisions.

---

## 0. Purpose in One Paragraph

We are building a learning system that **starts from a minimal set of exact arithmetic primitives** and, with no gradients, no pre-trained representations, and no outside grader beyond the facts its environment emits, **grows the structure needed to model a mathematical world**. While awake, it **gathers facts and grows circuits** that predict them. While asleep, it **deduces in isolation**: it composes what it already holds, prunes what contradicts itself, proves equivalences, and **compiles verified circuits into new, higher-order axioms**. The aim is a learner whose **vocabulary grows**, where every new word is a verified, compressed piece of the world's structure, and where every level of the hierarchy can be audited exactly.

---

## 1. Epistemological Premise

### 1.1 The bootstrapping problem

A continual learner seems to need a world model in order to evaluate and absorb new information, yet it has to build that model from scratch, with no fixed context window, no pre-baked representations, and no external teacher. This charter answers the question: *how can any intelligence decide what is reliable enough to build on?*

### 1.2 What "true" means in this system

> **Direction:** Truth here is **fallibilist**. A structure counts as true while it is **unrefuted**, meaning consistent with every fact observed and every other structure held. It stays true until a contradiction appears.
>
> **Why:** No intelligence has direct access to objective truth, however many senses or channels it has. A model consistent with all experience *works as* truth. The system's claims are therefore always provisional, and the charter never says a structure is "true" in an absolute sense.

Two principles follow, and both shape the architecture directly.

**The asymmetry principle.** *Disagreement is conclusive; agreement is provisional.* If two derivations of the same quantity disagree, at least one of them is definitely wrong. If they agree, both are only *not yet refuted*. In practice:

- disagreement **justifies pruning**, because the evidence is decisive;
- agreement **justifies retention only**. Promotion to a permanent axiom needs the stricter tests in §4.3.

**The informativeness principle.** *Agreement only counts as knowledge if it is informative.* A tautology ($x = x$) is true and says nothing. Silent pathways agreeing on silence, or a free-running oscillator agreeing with itself, add no knowledge. Agreement is therefore weighted by **how much it rules out**: its surprisal under a cheap null model (§3.1.4).

There is a limit to what internal checking can do: **the system can only detect errors that conflict with something it already holds.** An error consistent with everything encoded so far can only be exposed by *new facts*, from further observation or (later) action (§4.5). Sleep is internal falsification. Wake, and eventually action, is external falsification. Both are needed.

### 1.3 Redundant derivation as the source of internal error signals

In biology, reliability comes from **multi-sensory cross-referencing**: when independent pathways converge on the same prediction, confidence rises. In this system the equivalent is **redundancy of derivation**.

> **Direction:** What matters is **two or more independent derivations of the same quantity**, not two or more input channels. A single input channel is enough, provided the system holds more than one route from it to the same prediction. Separate channels give the *strongest* check, but they are not the *only* one.
>
> **Why:** A shallow derivation can look consistent and still contradict a deeper derivation over the same data. Suppose a period-6 stream is predicted by a lag-2 rule on one pathway and by a lag-6 rule on another. Run on its own predictions, the lag-2 pathway soon conflicts with the lag-6 pathway. Deeper computation exposes the contradiction. One caveat: derivations fed from a *shared* source can share a blind spot. By the asymmetry principle their *disagreements* are still conclusive, but their *agreements* count for less than agreement between independent channels.

The architecture must therefore **actively grow redundancy**: alternative derivations of predictions it already makes, plus *comparator sites* where those derivations meet (§3.1.5). A system that only grows to reduce boundary error will strip out exactly the redundancy that self-checking needs.

### 1.4 The aim is deductive, not a physics simulation

> **Direction:** The environment is a **mathematical generator**, not a simulated physical world. Its outputs are **ground facts**. The aim is to derive **verifiable theories** from those facts and from the foundational primitives.
>
> **Why:** The project is about how verified structure arises from axioms. Embodiment and physical realism are not research goals. They would add cost and noise without addressing the core question.

Deduction reaches theories by two routes, one for each phase of the system.

**Route 1: deduction within the closure of held axioms (sleep).** With no input, the system explores the consequences of what it already holds. It composes verified circuits, derives what follows, finds **equivalences** (theorems) and **contradictions** (refutations). This is the sense of the Newton illustration from v1. During the plague years of 1665–1666, Newton developed calculus largely in isolation, drawing consequences out of premises he already held. The illustration is about *deduction from held premises*. It does not claim his work lacked empirical motivation.

**Route 2: deduction from facts under a class axiom (wake).** Finite observations cannot, in general, establish a universal invariant; that is the problem of induction. They **can** if the system holds an **axiom about the class of possible generators**:

> **The class axiom.** *The world is generated by structures built from these primitives, within a size bound.*

Within such a class, finitely many facts determine the theory exactly:

- **Berlekamp–Massey:** if a stream is produced by a linear feedback shift register of length at most $n$, then $2n$ observed symbols determine the minimal register **exactly**.
- **Gold (identification in the limit):** for suitable classes, a learner converges to the exact generator and then never revises it.
- **Angluin ($L^*$):** finite automata can be learned exactly from **queries** and counterexamples. Queries are what the action phase provides (§4.5).

The class axiom is where fallibilism comes back in. If the class is wrong or the bound too small, a deduced theory can still be refuted by a new fact, and the system must respond by revising it (§1.2).

### 1.5 Core research question

> *Can a minimal set of exact arithmetic axioms, placed on a coordinate-free graph substrate with local metabolic constraints, enable a system to (a) grow structure that predicts a mathematical environment, (b) check itself through redundant derivations, (c) deduce and verify new theorems in isolation, and (d) compile those theorems into higher-order axioms, so that its vocabulary grows open-endedly and every level of the hierarchy can be verified exactly?*

### 1.6 What this charter does not claim

- **The meta level is hand-designed.** The *object level* (primitives, circuits) is minimal. The *meta level* (growth, pruning, sleep scheduling, consolidation criteria, energy constants) is engineered. The bootstrapping problem has been *moved* into the meta-rules, not solved. A sharper version of the research question is therefore: **how small can the hand-designed meta level be?** Closing the meta level is a long-term goal (§10).
- **There are no strange attractors in the simulation.** See §2.6.
- **Proposed mechanisms are hypotheses, not resolutions.** Every mechanism in this charter is tied to a testable hypothesis (§6) and a milestone with a kill criterion (§7).

---

## 2. The Substrate: Exact Modular Arithmetic of Variable Width

### 2.1 Why arithmetic, why exact, why nonlinear

> **Direction:** The substrate is **exact integer arithmetic modulo $2^k$**, where the **width $k$** (in bits) is a research parameter. It starts at $k = 1$ and is increased in controlled steps. Floating point is **not** used inside the substrate.
>
> **Why, in four parts:**
>
> 1. **Exactness is required for verification.** The asymmetry principle needs disagreement to be *unambiguous*. Theorems need equivalence to be *decidable*. Sleep compression must *preserve meaning*. Floats break all three: agreement becomes a tolerance setting, addition is not associative (so reorganising a sum can change its output), and equivalence becomes approximate.
> 2. **Nonlinearity is required for rich dynamics.** Addition and subtraction alone are linear. Over the reals, linear systems can only decay, grow, or oscillate neutrally. At least one nonlinear primitive is needed.
> 3. **Boolean logic was already arithmetic.** `XOR` is addition mod 2 and `AND` is multiplication mod 2. The v1 primitive set *is* arithmetic over GF(2). So the real decision was never "logic vs. arithmetic". It was **which modulus and which nonlinearity**. Making the width variable keeps v1 as the exact special case $k = 1$ and opens the way to graded, higher-resolution values.
> 4. **Finite is not the same as predictable.** Every finite deterministic system, floats included, is eventually periodic. That does not make it easy to predict: maximal-length shift registers and the Rule 30 centre column are finite and deterministic, yet effectively unpredictable without running them. And the network grows, so it is not a fixed finite system in any case.

### 2.2 Values and width

Every wire carries a value in $\mathbb{Z}_{2^k} = \{0, 1, \dots, 2^k - 1\}$, and arithmetic wraps modulo $2^k$. Powers of two are used (rather than general $m$) because they correspond directly to fixed-width machine integers, to SMT bit-vector theory, and to the truncation homomorphism in §2.4.

### 2.3 The foundational axioms (Level 0 primitives)

| Axiom | Definition | Role | At $k = 1$ (v1 equivalent) |
| :--- | :--- | :--- | :--- |
| `ADD` | $y = a + b \bmod 2^k$ | Combination, accumulation | `XOR` |
| `SUB` | $y = a - b \bmod 2^k$ | **Discrepancy / residual error**: zero exactly when $a = b$ | `XOR` |
| `MUL` | $y = a \cdot b \bmod 2^k$ | The ring's nonlinearity; **coincidence / gating** | `AND` |
| `EQZ` | $y = 1$ if $x = 0$, else $0$ | **Exact agreement test**; inversion; restores completeness | `NOT` |
| `DELAY` | $y(t) = x(t-1)$ | $z^{-1}$ unit memory; the only state-holding primitive | `DELAY` |
| `SPLIT` | $y_1 = y_2 = x$, priced via explicit `SPLIT` node and dual-wire MDL cost (`split_code_tax`) | Linear fan-out; signals cannot multiply for free | `SPLIT` |

At $k = 1$ the set reduces **exactly** to v1's five axioms, so all v1 intuitions and early milestones carry over unchanged.

**Two derived structures appear throughout this charter:**

- **Comparator** = `EQZ(SUB(a, b))`. It outputs 1 exactly when two derivations agree. At $k = 1$ this is `XNOR`.
  > **Why this matters:** v1 described `AND` as detecting agreement. `AND` detects **co-activation**: `AND(0,0) = 0`. Agreement is equality, and the comparator tests it exactly.
- **Discrepancy** = `SUB(a, b)`. It is non-zero exactly when two derivations disagree. For $k > 1$ its **circular magnitude** $\min(d, 2^k - d)$ measures *how far* apart they are, which gives the graded residual signal unavailable at $k = 1$.

**Completeness.** Over $\mathbb{Z}_{2^k}$ with $k > 1$, the polynomial functions (built from `ADD`, `SUB`, `MUL` alone) are a strict subset of all functions. `EQZ` restores completeness. The constant 1 can be derived as `EQZ(SUB(x, x))`, and any constant follows by repeated `ADD`. Every function $f : \mathbb{Z}_{2^k}^n \to \mathbb{Z}_{2^k}$ can then be written as

$$f(\mathbf{x}) = \sum_{\mathbf{a}} f(\mathbf{a}) \prod_i \text{EQZ}(x_i - a_i).$$

With `DELAY`, every finite-state generator over these values is therefore representable. That is what makes the class axiom of §1.4 meaningful.

### 2.4 Width as a research axis

> **Direction:** Width grows in two phases. **Phase W1:** a single uniform width $k$ per run, with $k \in \{1, 2, 4, 8, \dots\}$ treated as a controlled experimental variable. **Phase W2:** heterogeneous widths within one network, joined by explicit conversion primitives.
>
> **Why:** Uniform width keeps early experiments interpretable and makes $k = 1$ an exact reproduction of v1. Heterogeneous width is the long-term target, because different parts of a world model need different resolution. But it brings conversion semantics that should only be added once the uniform case is understood.

Phase W2 adds two conversion primitives:

- **`TRUNC`** ($\mathbb{Z}_{2^{k'}} \to \mathbb{Z}_{2^k}$, for $k < k'$): $x \bmod 2^k$. This is a **ring homomorphism**. For circuits in the *ring fragment* (`ADD`, `SUB`, `MUL`, `DELAY`, `SPLIT`), any identity proven at width $k'$ also holds at width $k$ after truncation. Narrow theories are images of wide ones, which gives widening a principled meaning. (`EQZ` is not preserved by truncation. Identities that use it must be re-verified at each width.)
- **`EXT`** ($\mathbb{Z}_{2^k} \to \mathbb{Z}_{2^{k'}}$): zero-extension. It is *not* a homomorphism, and its use marks a point where a theory's meaning depends on the width.

### 2.5 Tick semantics (structural invariants)

These are hard invariants, enforced whenever structure changes:

1. **Synchronous update.** All `DELAY` registers update simultaneously at each tick. All other primitives are combinational within a tick.
2. **Every cycle contains at least one `DELAY`.** Combinational loops are forbidden, because they have no well-defined value. Sprouting that would create one is rejected. Evaluation is then a topological pass, costing $O(1)$ per node and $O(N)$ per tick.
3. **Wire delay is an explicit configuration parameter on a coordinate-free graph.** Circuits use direct point-to-point wiring (`substrate::Circuit`) allowing arbitrary edges between endpoints without planar relay blockage; combinational wires propagate without tick delay and only `DELAY` holds state. Per-hop wire delays (`TicksPerHop`, unit delay per graph edge) remain supported as a controlled ablation (§8). Planar grid routing (`LocalVonNeumann` / `LocalMoore`) is retired under Decision Record #30 (historical M1 baseline anchored at commit tag `m1-final-lattice`).
4. **Each output port has one driver.** Competing hypotheses meet at comparators and output adders. Only one structure drives each boundary prediction port at a time. Replacing or augmenting the driver with a stacked corrector is a structural event decided by the economy.
5. **Fan-out happens only through `SPLIT`.** Unconnected outputs are allowed (discarding is free). When an output signal feeds multiple destinations, explicit `SPLIT` primitives distribute it. Description length ($L(C)$) prices non-local wiring and branching, preventing bloat without planar congestion.

### 2.6 Dynamical vocabulary

> **Direction:** Validated structures are described as **limit cycles, attractor basins, transients, and entrainment**, not as strange attractors.
>
> **Why:** A network with $N$ state registers of width $k$, running autonomously, is a deterministic map on a finite set of $2^{kN}$ states. Every trajectory is eventually periodic. Strange attractors need a continuous state space, and floats do not supply one either: on a computer they are finite-precision approximations. When the network is *driven* by input (wake), the right concept is **entrainment** (generalised synchronisation) of a finite-state transducer to its input.

The measurable quantities are cycle length, transient length, **basin size** (the fraction of perturbed register states that return to the same cycle), and **recovery** (Hamming or circular-distance convergence after perturbation). For large $k$, chaos-like behaviour can be quantified as a **finite-resolution Lyapunov exponent** over the time before periodicity sets in. The intuition behind v1's "strange attractor" survives as a **continuum-limit question**: how does the system behave as $k \to \infty$? That is a question for analysis, not something the simulation must literally contain.

"Thinking" in this charter means **self-sustaining recurrent activity in which redundant derivations stay mutually consistent**. In the autonomous regime that is an attractor cycle. In the driven regime it is stable entrainment.

---

## 3. A Two-Layer Architecture

> **Direction:** The system has **two distinct layers**, and the charter, the code, and the hypotheses keep them apart.
>
> - The **structural layer** is *engineering*. It contains graph surgery, metabolism, credit, growth, and pruning. It decides **where circuits come from**.
> - The **logical layer** is the *alchemy*. It contains composition, equivalence, theorems, and closure. It decides **what new truths follow from what is held**.
>
> **Why:** v1 tied its use of Algorithmic Chemistry to *network structure*. That invited the objection that gates never produce gates, so the system is not a chemistry. The intent was always that alchemy happens in the **logical space**: combining axioms produces new truths. Structural growth is the supporting machinery and makes no claim to be a chemistry.

### 3.1 The structural layer

#### 3.1.1 Coordinate-free graph connectivity

Circuits are coordinate-free directed multigraphs (`substrate::Circuit`). Wiring connects primitive and boundary ports directly without intermediate planar routing paths or coordinate placement.

> **Architectural Decision (M1b & Decision Record #30):** The early metaphor of a physical 2D planar lattice (`LocalVonNeumann` with single-cell `SPLIT` relays and `max_relay_hops`) treated empty cells as routing relays. During multi-neuron neurogenesis, routing wires through empty cells rapidly saturated border cells around input/output ports and produced severe spatial congestion and routing deadlock (measured in [research_record.md](research_record.md)). Under Decision Records #29 and #30, circuits use direct point-to-point wiring (`substrate::Circuit`), and 2D grid coordinates, perimeter anchors, and `LocalVonNeumann` / `LocalMoore` ablations are permanently retired (historical spatial baseline anchored at commit tag `m1-final-lattice`). Locality and fan-out are priced purely through MDL circuit description length ($L(C)$ wire code bits and `split_code_tax`), preventing bloat without planar gridlock.

#### 3.1.2 Metabolism

Each node holds an energy reserve $E_i(t)$. Rather than arbitrary per-tick maintenance rent (which diverges with horizon $T$), each placed node incurs a **one-time description debt** $\Delta L_i$ charged upon maturity through the maintenance channel, matching its share of the circuit's description length $L(C) = \sum_i \Delta L_i$. A node driving multiple wires (such as `SPLIT`) naturally owes more wire code bits ($L(\text{split}) > L(\text{other})$) without tuning arbitrary constants. A node whose reserve runs out is removed. Energy enters the system **only** through reward (§3.1.4).

> **Design note on post-maturity reserve bounding:** Online runs cap mature reserves using **Option B (horizon-derived ceiling, Decision Record #31)**: $E_{\max}(v) = \max\left(E_{\text{init}},\ W_{\text{obs}} \sum_{p} \text{share}(v,p)\,\log_2\frac{1-p_{\text{null}}(p)}{1-\hat{q}_p}\right)$ with unified observation horizon $W_{\text{obs}} = t_{\text{juv}} + 2 W_{\text{diag}}$ matching `obs_capacity` ([parameter_inventory.md](parameter_inventory.md)). This supersedes M1's fixed `25.0`-bit ceiling so obsolete structures drain at their calibrated confidence rate while preventing arbitrary early starvation, without asserting an unproved universal $O(W_{\text{obs}})$ starvation theorem (empirical regime-shift latencies are recorded in [research_record.md](research_record.md)).

#### 3.1.3 Credit assignment: retrograde, activity-gated flow

> **Direction:** Reward enters at the point where a prediction is checked (a boundary port in wake, a comparator in sleep). From there it flows **backward** along the edges that actually carried the rewarded signal, split according to each upstream contribution.
>
> **Why:** This is the main risk in the project. Interior nodes far from the boundary starve unless credit reaches them, and credit that leaks to non-contributors feeds free-riders. Rule-economy systems such as Holland's bucket brigade struggled with exactly this. The mechanism chosen is local and has a biological model: in neural development, neurons survive by receiving trophic factor sent *backward* from the targets they innervate.

How contribution is measured:

- **Sensitivity:** an input contributed on a rewarded tick if changing it would have changed the output. This is computed exhaustively across all $2^k - 1$ alternative values for widths $k \le 4$ (`EXACT_SENSITIVITY_MAX_BITS = 4`) and sampled (`sensitivity_sample_count`) for wider rings $k > 4$.
- **Eligibility traces & replacing cap:** credit reaches structures whose contribution came several ticks earlier through `DELAY`. Ancestry weights are clamped to $\le 1.0$ (replacing traces), preventing recurrent loops from accumulating unbounded feedback gain.
- **Activity recency decay:** input weights on identity values decay by $\gamma = 0.90$ on identity-emitting ticks, bounding single-fire detour leaks across arbitrary horizons while preserving sparse correctors.
- **Juvenile subsidy:** new sprouts get an energy endowment and a grace period, so they can show their worth before maintenance costs kill them.

#### 3.1.4 Reward: calibrated point-forecast surprisal, relative to a null model

> **Direction:** Prior to observing symbol $s$, a port's point prediction commits to a calibrated probability forecast $q = \max(\hat{q}, p_{\text{null}})$, where $\hat{q}$ is estimated prequentially under the null model's KT prior. A correct prediction earns $\log_2(q/p_{\text{null}})$, while an incorrect prediction incurs $\log_2((1-q)/(1-p_{\text{null}}))$. Reward is given for **exact** matches. Graded discrepancy (for $k > 1$) drives *growth* (§3.1.5) but is not paid out as reward.
>
> **Why:** This puts the informativeness principle (§1.2) into practice and eliminates the chance-predictor lottery. Centered surprisal had zero expected flux, but its positive variance created a random walk whose spread grew as $O(\sqrt{T})$. Under calibrated point forecasting, constant predictors score identically $0.0$ on every tick and seed, and non-predictive cumulative luck is strictly bounded by Ville's martingale inequality across arbitrary horizons. Weighting by surprisal pays nothing for silence or for a metronome, and pays most for correctly predicting rare events. Paying reward only for exact matches keeps the asymmetry principle sharp.

**Where reward comes from in each phase:**

- **Wake:** correct predictions at the boundary ports.
- **Sleep:** **only** surprisal-weighted agreements at internal comparators, with $p_{\text{null}}$ estimated from that signal's wake statistics. Boundary reward is **disabled** in sleep. In loopback the prediction *is* the next input, so boundary reward would be earned automatically and mean nothing.

#### 3.1.5 Growth: two kinds of sprouting

Energy surplus near a persistent, structured signal triggers sprouting of one of two kinds:

1. **Corrector sprouting** (reduces error). Triggered where a residual stream is *structured*, that is, predictable below its maximum entropy. Structure is detected locally by residual autocorrelation, a small compressibility estimate, or **mutual information between the residual and another candidate signal in the circuit**. The candidate input with the highest mutual information becomes the new node's input. This is the cascade-correlation criterion. See §3.3 for the formal version.
2. **Redundancy sprouting** (enables self-checking). Triggered where an established prediction has **only one derivation**. The system grows an alternative route to the same quantity, plus a comparator where the two meet. This is what makes internal falsification possible (§1.3).

#### 3.1.6 Pruning

- **On disagreement:** when a comparator fires disagreement persistently, credit is withdrawn from the losing derivation (the one that disagrees with the facts in wake, or with the majority of derivations in sleep). That is decisive evidence under the asymmetry principle.
- **On starvation:** nodes whose maintenance costs exceed their earned credit die.

#### 3.1.7 Threat model: free-riders and bloat, not replicators

> **Direction:** The main structural threat is **credit free-riders**: no-op detours, redundant chains, and structures that sit inside a rewarded pathway and draw energy without contributing. This is the circuit equivalent of genetic-programming bloat.
>
> **Why:** Nothing in this substrate self-replicates, so there are no parasites in the artificial-chemistry sense. An isolated self-exciting loop earns no reward and starves by construction, so testing it proves nothing. The real exploit is getting onto the credit path. Wire MDL pricing, the `SPLIT` tax, sensitivity-based credit, and sleep compression (§3.2) are the defences, and Milestone M3 tests them.

### 3.2 The logical layer (the alchemy)

The logical layer is where **new truths are produced by combining axioms**.

| Concept | Meaning in this system | Analogue in Fontana & Buss's AlChemy |
| :--- | :--- | :--- |
| **Object** | A verified circuit over the primitive set | λ-expression |
| **Reaction** | **Composition**: wiring circuit outputs to circuit inputs | Application $M\,N$ |
| **Truth (theorem)** | An **exactly verified equivalence** ($C_1 \circ C_2 \equiv C_3$) or invariant ("this output has period 6") | Functional equivalence class |
| **Rewrite rule** | A theorem used to replace one circuit with an equivalent, cheaper one | Reduction to normal form |
| **Level 0** | The primitive axioms | Individual expressions |
| **Level 1 organisation** | A **family of derived circuits closed under composition** (an algebraic sub-structure) | Self-maintaining organisation |
| **Level 2 organisation** | Composition **across** Level 1 families through shared interfaces | Meta-organisation |
| **Derived axiom (macro)** | A verified circuit compiled into a single new primitive | — |

**Mechanism: equality saturation.** Sleep explores compositions of held circuits in an **e-graph**, a data structure that stores many equivalent forms of an expression compactly. It collects the equivalences that hold, verified exactly (§2.1), and extracts the cheapest equivalent form. Discovered equivalences become rewrite rules, and rewrite rules drive compression. Equality saturation is a mature technique (Willsey et al., *egg*, available as a Rust crate).

**Why exactness is non-negotiable here:** a theorem is a claim of *exact* equivalence. With approximate arithmetic, the logical layer would be producing conjectures, not theorems.

**Position on the AlChemy lineage.** Fontana & Buss (1994) found that Level 0 copiers dominated unless copy reactions were filtered out. With that filter, Level 1 organisations emerged, and Level 2 organisations formed under specific conditions, sometimes through mediating "glue" expressions. A modern revisit (Mathis, Patel, Weimer & Forrest, 2024) found Level 1 organisations to be robust and common but **hard to combine**. That **combination barrier** is the open problem this charter targets. It is addressed **at the logical layer**, through explicit interfaces and verified equivalence. No claim is made that the structural layer is a chemistry.

### 3.3 The first and second neuron, formalised

v1's account of neurogenesis is kept, and it can be stated exactly as **residual correction over $\mathbb{Z}_{2^k}$**.

- **The first neuron (homeostatic predictor).** A single primitive placed in a data stream. It survives only if its prediction $y_1(t)$ of $A(t+1)$ earns more reward than it costs.
- **The second neuron (error-driven interneuron).** The first neuron's residual is

  $$r(t) = A(t+1) - y_1(t) \pmod{2^k}.$$

  If $r$ is structured, a corrector produces $\hat{r}(t)$, and the corrected prediction is $\hat{y}(t) = y_1(t) + \hat{r}(t)$. The combined error is $r(t) - \hat{r}(t)$, the residual of the residual. Correctors stack: $y_1 + \hat{r}_1 + \hat{r}_2 + \cdots$. This is **boosting over a finite ring**. At $k = 1$ it reduces to v1's XOR residual.

> **Why the second neuron needs a $z^{-1}$:** this follows from causality and is not a design choice. The residual for tick $t$ is not known until tick $t+1$. So a corrector can only work from **delayed** residuals and inputs, and the `DELAY` in v1's second neuron is required.

### 3.4 A global objective for the local economy

Local rules are hard to debug without a global quantity they are supposed to improve. The economy is calibrated against **total description length**:

$$L = L(\text{palette}) + L(\text{circuit} \mid \text{palette}) + L(\text{facts} \mid \text{circuit}).$$

With one-time description debt (§3.1.2) and calibrated prequential reward (§3.1.4), maintenance cost exactly realizes the second term and prediction reward exactly realizes the third. Total energy change and global null-calibrated MDL change are **identically equal**:

$$\Delta E(T) = -\Delta L_{\text{null}}(T).$$

A derived axiom earns its place in the palette only if it lowers the first two terms by more than it adds. **A run in which total energy change departs from $-\Delta L_{\text{null}}$ signals an implementation bug in the economy.**

---

## 4. The Dual-Timescale Loop

### 4.1 Wake: gathering facts and deducing under the class axiom

- The $K$ channels drive the boundary input ports. Each tick the network emits predictions $\hat{A}(t+1), \hat{B}(t+1), \dots$ and is rewarded according to §3.1.4.
- Facts are written to a **bounded fact buffer**, the system's record of ground truth.
- Corrector sprouting (§3.1.5) and pruning (§3.1.6) run online in wake; redundancy sprouting (§3.1.5) is staged for `M2b` once comparator economics is pre-registered (Decision Record #35).

### 4.2 Sleep: deduction in isolation

External input is cut. Each prediction port is **looped back** into its input port ($\hat{A}(t+1) \to A(t+1)$), and the `DELAY` register states from the end of wake seed the loop. The network now drives itself with its own world model. During sleep:

1. **Internal falsification.** Redundant derivations run against each other. Comparators that fire disagreement prune the losing derivation (§3.1.6). Reward comes only from informative internal agreement (§3.1.4).
2. **Theorem discovery.** The logical layer runs equality saturation over held circuits and collects verified equivalences and invariants (§3.2).
3. **Compression.** Rewrite rules and logic minimisation replace circuits with cheaper equivalents and remove free-rider detours (§3.1.7).
4. **Consolidation.** Structures that pass the promotion test (§4.3) are compiled into derived axioms.

> **Direction on spurious attractors:** whether internal self-consistency is enough to filter spurious structure, or whether sleep must also check against recorded facts, is decided **by experiment**, not by assumption.
>
> - **Arm S-A (consistency only):** free-running loopback, pruning on internal disagreement.
> - **Arm S-B (consistency plus replay):** additionally replay the fact buffer and accept only structures whose free-running statistics match the replayed statistics.
>
> **Why:** Free-running networks tend to fall into short, self-consistent cycles that are disconnected from the facts. Some classic theories of sleep say dreaming exists to *remove* such modes. S-A may be enough when redundant derivations are dense. S-B guards against the sparse case. Replay is not an outside teacher: the fact buffer is the system's own record of ground facts, and checking theorems against recorded facts is ordinary deduction. If S-A performs as well as S-B, replay is dropped.

### 4.3 From retention to promotion

Under the asymmetry principle, surviving sleep earns **retention**, not promotion. A structure is **promoted** to a derived axiom only if **all** of the following hold:

1. **Unrefuted:** no persistent comparator disagreement in loopback.
2. **Matches the facts:** its free-running statistics match the fact buffer (required in arm S-B; recorded in arm S-A).
3. **Compresses:** promotion lowers total description length $L$ (§3.4).
4. **Verified:** the compiled axiom is **formally proven equivalent** to the subgraph it replaces, using sequential equivalence checking over bit-vectors or an e-graph proof.

### 4.4 Identifying and managing derived axioms

- **Boundaries:** candidate subgraphs are those with a **small interface** relative to their internal size, whose future internal state is predictable from their ports alone (a Markov-blanket test).
- **Immutable but retirable:** the inside of a derived axiom is frozen. The axiom itself is retired if it stops being used (usage-based decay), and each palette entry pays a description-length cost, so the palette cannot grow without limit.
- **Reuse:** in later wake cycles a derived axiom can be sprouted as a single atomic unit.

### 4.5 Observation first, action later

> **Direction:** The system starts in a **purely observational** regime, where the generator is unaffected by the network. Boundary output ports are designed from the start as the hooks for later **actions** (perturbing or querying the generator). Action is a planned phase that **completes** the system's epistemology. It is not an optional extra.
>
> **Why:** A newly grown learner has to survive by predicting before acting means anything. But observation alone cannot tell a structural invariant from an accidental correlation that happened to hold throughout the data: both are equally consistent. Only intervention can tell them apart. Action also turns wake into exact *query-based* learning in the style of Angluin's $L^*$.

---

## 5. The Environment

### 5.1 Generators and channels

The environment is a deterministic generator that emits $K$ parallel channels of $k$-bit symbols, entering via distinct boundary input ports of the circuit. For robustness tests, a **declared noise model** (for example, independent symbol substitution at rate $\varepsilon$ on named channels) may be added. Noise is always explicit, parameterised, and recorded.

### 5.2 Measuring difficulty: computational mechanics

> **Direction:** Tier difficulty is **measured**, not asserted. Every generator in the environment is a stationary finite-state process, so its **ε-machine** (its minimal optimal predictor) can be computed exactly.
>
> **Why:** The ε-machine gives ground truth that every learned circuit can be scored against.

| Quantity | Meaning | Use |
| :--- | :--- | :--- |
| Statistical complexity $C_\mu$ | Bits of memory any optimal predictor must hold | Lower bound on a circuit's state (`DELAY` count × $k$) |
| Entropy rate $h_\mu$ | Irreducible unpredictability, which is zero without noise | Floor on achievable error |
| Markov order / synchronisation time | History needed before prediction becomes possible | Expected transient before a correct circuit locks on |

Two further sources of ground truth are **Berlekamp–Massey**, which gives the exact minimal shift register for LFSR streams, and **exact synthesis**, which gives the true minimum circuit size for the smallest targets.

### 5.3 Tiers

| Tier | Content | Notes |
| :--- | :--- | :--- |
| **T1 — Local temporal** | Short periods and rhythms on one channel | Solvable by 1–2 primitives with `DELAY`; identical to v1 at $k = 1$ |
| **T2 — Cross-channel coupling** | Channel $B$ is a delayed function of channel $A$ (for example, a delayed sum or parity) | Requires pathways from separate boundaries to meet at comparators |
| **T3a — Spatially presented CA** | A cellular-automaton row is fed as $K$ parallel boundary channels | The local rule is learnable by low-degree sparse wiring, priced compactly by wire MDL $L(C)$. Outer-totalistic rules are a neighbourhood **sum and comparison**; elementary rules are polynomials over GF(2) |
| **T3b — Modular counters** | Counters modulo $2^j$ (native to the ring) and modulo other $n$ (which need `EQZ`) | Tests width and completeness |
| **T3c — LFSR curricula** | Shift registers presented through sub-structures that are learned first | See the caveat below |
| **T4 — Task families** | Families of generators that share sub-structure (CA rules with common neighbourhood logic, LFSRs with shared taps, channels recombining base rhythms) | Needed to test reuse and transfer of derived axioms (Level 2) |

> **Why LFSRs need a curriculum:** an LFSR is built entirely from native primitives, so it is easy to *represent*. It is very hard to *find* by incremental search. Any circuit using only a proper subset of the correct taps predicts at chance level, so partial solutions earn **no partial credit**. This is the parity problem, which is hard for local and statistical-query learners. A single LFSR therefore gives growth nothing to climb. A curriculum in which smaller parities are learned and reused first gives it a path.
>
> **Explicitly excluded: the Rule 30 centre column as a single channel.** Predicting it is an open problem (it is used as a pseudo-random generator). Rule 110 observed as a single channel is effectively arbitrary, because Rule 110 is computationally universal. Cellular automata enter the environment **only** in the spatially presented form (T3a).

---

## 6. Hypotheses

Every mechanism in this charter is a **hypothesis** linked to a milestone. None is claimed as a resolution.

| ID | Hypothesis | Mechanism | Tested in |
| :--- | :--- | :--- | :--- |
| **H1** | Local, retrograde, surprisal-weighted credit sustains correct circuits and starves incorrect ones without gradients | §3.1.3, §3.1.4 | M1a |
| **H2** | Residual-driven corrector sprouting reduces error toward $h_\mu$ as the residual's entropy predicts | §3.1.5, §3.3 | M1b, M2a |
| **H3** | Redundant derivations plus comparators detect errors internally, offline | §1.3, §3.1.5 | M2b, M4a |
| **H4** | Wire description-length pricing, the `SPLIT` tax, sensitivity-based credit, and compression remove free-riders, keeping circuits near minimal size | §3.1.7 | M3 |
| **H5** | Sleep filtering beats **compute-matched** wake-only learning | §4.2 | M4a |
| **H6** | Consistency-only sleep (S-A) performs as well as consistency-plus-replay sleep (S-B) | §4.2 | M4a |
| **H7** | Equality saturation over held circuits discovers non-trivial, verified theorems, and promotion lowers $L$ | §3.2, §4.3 | M4b |
| **H8** | Derived axioms transfer across task families, overcoming the combination barrier at the logical layer (Level 2) | §3.2, §5.3 T4 | M4c |
| **H9** | Graded residuals at $k > 1$ shorten time-to-solution compared with $k = 1$ encodings of the same generators, and ring-fragment theories lift across widths through `TRUNC` | §2.3, §2.4 | M5 |
| **H10** | Interventions refute spurious but self-consistent theories that sleep cannot refute | §4.5 | M6 |

---

## 7. Milestones

From M1a on, every milestone runs at **$k = 1$ and $k = 2$ at least**, so the economy is never accidentally tuned to Boolean values only.

| Stage | Goal | Success criterion | Kill / revisit criterion |
| :--- | :--- | :--- | :--- |
| **M0 — Simulator and yardstick** | Tick semantics and invariants (§2.5); variable-width values; seeded determinism; generator library with ε-machine and Berlekamp–Massey ground truth; baseline harness | **Completed & verified.** Baselines reach known optima on T1; every run reproducible from seed and configuration | — (infrastructure) |
| **M1a — Economy on fixed structure** | Validate credit and reward (H1) on **hand-built** correct, incorrect, and bloated circuits, before any growth | **Completed & verified.** Correct circuits gain energy; incorrect ones and detours lose it; exact identity $\Delta E(T) \equiv -\Delta L_{\text{null}}(T)$ holds on all runs; 100/100 banks passed | Energy does not separate correct from incorrect circuits: redesign credit before attempting growth |
| **M1b — First to second neuron** | Error-driven corrector sprouting (H2) | **Completed & verified.** Combined error falls as the residual's entropy predicts; no backpropagation ([research_record.md](research_record.md#25-redesigned-m1a--milestone-1-attainment-sign-offs-2026-11-072026-11-08)) | No consistent error reduction on T1 across seeds: revisit the sprouting criterion |
| **M2a — Cross-channel predictive coupling** | Error-driven coupling across channels using boundary and interior signals over existing corrector sprouting (H2 cross-channel extension; DR #35–#37) | Pre-registered T2 families solved vs. $h_\mu$ and equal-compute baselines; $K$-channel lock-time ratio meets pre-registered criterion (with feasibility argument) | Cross-channel coupling consistently stalls on declared T2 tasks across dev banks: revisit residual diagnosis / signal pool |
| **M2b — Redundancy and comparators** | Redundancy sprouting; independent pathways meet at comparators after Charter comparator-reward amendment (H3; DR #35–#37) | Pre-registered T2 invariance, declared comparator flagging signal, wake/sleep response semantics, and noise recovery (defined post-Charter amendment) | Comparators never form spontaneously under calibrated redundancy reward: revisit redundancy sprouting |
| **M3 — Free-rider resistance** | Inject no-op detours and redundant branches into working circuits (H4) | Bloat is pruned; circuit size converges toward the exact-synthesis minimum | Circuit size grows without bound under stable error: revisit credit and the `SPLIT` tax |
| **M4a — Sleep as internal falsification** | Arms S-A and S-B against compute-matched wake-only (H5, H6) | Sleep beats wake-only by a pre-registered margin; spurious cycles are rejected | Neither arm beats compute-matched wake-only: revisit sleep before attempting consolidation |
| **M4b — Theorem discovery and consolidation** | Equality saturation; promotion test (§4.3) (H7) | The system discovers and **formally verifies** at least one non-trivial equivalence by itself; each promotion lowers $L$ | Only trivial identities found: revisit e-graph exploration or the informativeness weighting |
| **M4c — Transfer** | Task families (T4) (H8) | Derived axioms shorten time-to-solution on related generators | No transfer beyond chance: revisit interface identification (§4.4) |
| **M5 — Width scaling** | Phase W1 for $k \in \{1, 2, 4, 8\}$, then Phase W2 with heterogeneous widths (H9) | The H9 effects are confirmed; `TRUNC` lifting is verified | — (characterisation study) |
| **M6 — Action** | Boundary ports perturb or query the generator (H10) | Planted spurious correlations are refuted by intervention | — (opens the next charter) |

A kill or revisit outcome is a successful experiment. The agent records it and escalates. It does not alter mechanisms, thresholds or tasks to avoid it.

### 7.1 Milestone lifecycle and decision rights

Every milestone follows the fixed experimental sequence:

1. **Charter hypothesis** (§6) $\rightarrow$ **exploratory characterisation** (measuring distributions and costs with **no** gating thresholds, per Rule 8).
2. **Milestone plan with pre-registration** ([milestone_plan_template.md](templates/milestone_plan_template.md)): hypotheses, gates with feasibility arguments citing theoretical bounds or measured baselines (Rule 9), explicit dev/confirmation/holdout seed banks, kill/revisit criteria, and [parameter_inventory.md](parameter_inventory.md) updates.
3. **User sign-off** before confirmatory code or threshold changes.
4. **Phased implementation:** one phase per commit with golden-trace invariance checks (or written rationale when behavior changes intentionally).
5. **Development banks** $\rightarrow$ **confirmation bank** $\rightarrow$ **single-shot holdout** (opened once after dev/confirmation pass) $\rightarrow$ **attainment record** in [research_record.md](research_record.md) $\rightarrow$ **independent review**.

| Decision domain | Agent authority | User authority |
| :--- | :--- | :--- |
| Behavior-preserving engineering refactors, telemetry wiring, test coverage | **Decides** (proven by golden traces) | Reviews commits |
| Exploratory measurement harnesses and profiles (no pass/fail gates) | **Decides** | Reviews findings |
| Scientific parameters, values, search bounds, guardrails (`parameter_inventory.md`) | Recommends / implements only after sign-off | **Decides** |
| Pre-registered gate thresholds, $P_{\text{target}}$, seed banks, golden re-blessing, Charter text, kill-criterion interpretation, milestone sign-off | Drafts & escalates with options (`blocker_report.md`) | **Decides** |

---

## 8. Experimental Standards

- **Metrics:** prediction loss in bits per symbol relative to $h_\mu$; state size relative to $C_\mu$; circuit size relative to the exact-synthesis minimum (where computable); total description length $L$; ticks to solution; palette size and reuse depth; for sleep, the rate of spurious cycles and basin statistics (§2.6).
- **Baselines:**
  - random circuit search at equal size and compute,
  - Cartesian Genetic Programming with a $(1+\lambda)$ evolution strategy,
  - a small RNN or LSTM trained by backpropagation through time,
  - **Context-Tree Weighting** or a PPM-family predictor (strong universal sequence predictors) as a demanding reference,
  - **Berlekamp–Massey** as the exact deductive reference for LFSR streams,
  - exhaustive enumeration for the smallest tiers.
- **Ablations:** sleep vs. no sleep; S-A vs. S-B; `SPLIT` cost $> 0$ vs. $= 0$; surprisal reward vs. raw accuracy; redundancy sprouting on vs. off; wire delay on vs. off.
- **Equal compute:** every comparison that involves sleep uses the same total tick budget, so sleep cannot win simply by getting more computation.
- **Pre-registration:** for each milestone, success margins, seed counts, and kill criteria are written down **before** the runs.
- **Reproducibility:** every source of randomness is seeded through a portable, version-stable generator, and every run can be reproduced from its seed and configuration file.

---

## 9. Implementation Guidance (Rust)

- **Layer separation:** keep the **substrate** (values, primitives, tick semantics, invariants), the **structural layer** (graph editing, economy, growth, pruning), the **logical layer** (composition, e-graph, verification), the **sleep controller**, the **environment** (generators, ground truth), and **measurement** behind distinct traits or modules. Most research steps change one layer and hold the others fixed.
- **Values:** store values as unsigned machine integers masked to $k$ bits, using `wrapping_add`, `wrapping_sub`, and `wrapping_mul`, which are deterministic on every platform. Uniform width can use const generics for speed; heterogeneous width (Phase W2) needs a width tag at runtime.
- **Bit-parallel evaluation:** at $k = 1`, pack 64 independent evaluations of **one fixed circuit** into a `u64` (64 noise realisations, perturbed dream starts, or phase offsets). For $k > 1$, use SIMD lanes in the same way. This speeds up energy estimation, basin measurement, and verification. It does not parallelise structural search, because structure differs between seeds.
- **Determinism:** use a portable, seedable RNG whose output is stable across crate versions (for example, a ChaCha-based generator) rather than a platform-default generator.
- **Logical layer:** the `egg` crate for equality saturation. Formal equivalence checking over bit-vectors (for example, through an SMT solver) can sit behind a Cargo feature flag to keep the default build light.
- **Repository conventions:** follow the repository's project guidelines for layout, validation (`cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --check`), and dependency hygiene.

---

## 10. Long-Term Horizon

1. **Measurable open-endedness.** Palette growth, reuse depth, and total description length over time give a *quantitative* handle on whether novelty keeps accumulating, something open-endedness research has rarely had.
2. **Action and causality.** Moving from observation to intervention (§4.5) completes the epistemology: the system becomes able to tell structural invariants from coincidences.
3. **The continuum limit.** The behaviour of learned dynamics as width $k \to \infty$: where the finite-resolution Lyapunov exponent (§2.6) and the v1 intuition about strange attractors meet rigorous analysis.
4. **Closing the meta level.** The growth, pruning, and compression rules become **objects represented in the substrate**, open to the same logical alchemy as everything else, so the system can grow its own growth rules. This returns the project to Fontana & Buss's original aim of constructive closure, now as a stated destination rather than a starting claim.

---

## 11. Foundational Reading and Intellectual Lineage

### 11.1 Epistemology and deduction from facts

- **Popper, K. (1959).** *The Logic of Scientific Discovery.* Fallibilism; empirical content as what a statement rules out.
- **Bar-Hillel, Y., & Carnap, R. (1953).** Semantic information. *British Journal for the Philosophy of Science*, 4(14), 147–157. Informativeness as exclusion of possibilities.
- **Massey, J. L. (1969).** Shift-register synthesis and BCH decoding. *IEEE Transactions on Information Theory*, 15(1), 122–127. Exact deduction of the minimal LFSR from $2n$ symbols.
- **Gold, E. M. (1967).** Language identification in the limit. *Information and Control*, 10(5), 447–474.
- **Angluin, D. (1987).** Learning regular sets from queries and counterexamples. *Information and Computation*, 75(2), 87–106.
- **Pearl, J. (2009).** *Causality* (2nd ed.). Cambridge University Press. Why intervention is needed to separate invariance from coincidence.

### 11.2 Evolutionary neurobiology and first principles

- **Bennett, M. (2023).** *A Brief History of Intelligence.* Mariner Books. How nervous systems evolved from homeostatic steering to internal simulation.
- **Braitenberg, V. (1984).** *Vehicles: Experiments in Synthetic Psychology.* MIT Press. Complex behaviour from minimal cross-wired loops.
- **Levi-Montalcini, R. (1987).** The nerve growth factor 35 years later. *Science*, 237(4819), 1154–1162. Retrograde trophic support as the model for credit flow.

### 11.3 Algorithmic chemistry and organisation

- **Fontana, W., & Buss, L. W. (1994).** "The arrival of the fittest": Toward a theory of biological organization. *Bulletin of Mathematical Biology*, 56(1), 1–64.
- **Mathis, C., Patel, D., Weimer, W., & Forrest, S. (2024).** Self-organization in computation & chemistry: Return to AlChemy. *Chaos* / arXiv:2408.12137. The current status of the combination barrier.
- **Dittrich, P., Ziegler, J., & Banzhaf, W. (2001).** Artificial chemistries — a review. *Artificial Life*, 7(3), 225–275.
- **Agüera y Arcas, B., et al. (2024).** Computational life: How well-formed, self-replicating programs emerge from simple interaction. arXiv:2406.19108.

### 11.4 Growth, economies, and circuits

- **Fahlman, S. E., & Lebiere, C. (1990).** The cascade-correlation learning architecture. *NIPS 2*. Adding units to fit the residual.
- **Holland, J. H. (1986).** Escaping brittleness: The possibilities of general-purpose learning algorithms applied to parallel rule-based systems. *Machine Learning II*. Classifier systems and the bucket brigade.
- **Baum, E. B. (1999).** Toward a model of intelligence as an economy of agents. *Machine Learning*, 35(2), 155–185. The Hayek machine and property rights in credit.
- **Miller, J. F. (Ed.) (2011).** *Cartesian Genetic Programming.* Springer. Gate-level evolved circuits; module acquisition.
- **Kauffman, S. A. (1969).** Metabolic stability and epigenesis in randomly constructed genetic nets. *Journal of Theoretical Biology*, 22(3), 437–467. Attractors in Boolean networks.

### 11.5 Sleep, consolidation, and library learning

- **Crick, F., & Mitchison, G. (1983).** The function of dream sleep. *Nature*, 304, 111–114. Dreaming as removal of spurious modes.
- **Hinton, G. E., Dayan, P., Frey, B. J., & Neal, R. M. (1995).** The "wake-sleep" algorithm for unsupervised neural networks. *Science*, 268(5214), 1158–1161.
- **McClelland, J. L., McNaughton, B. L., & O'Reilly, R. C. (1995).** Why there are complementary learning systems in the hippocampus and neocortex. *Psychological Review*, 102(3), 419–457. Replay of real experience.
- **Tononi, G., & Cirelli, C. (2014).** Sleep and the price of plasticity. *Neuron*, 81(1), 12–34. Sleep as synaptic downscaling.
- **Ellis, K., et al. (2021).** DreamCoder: Bootstrapping inductive program synthesis with wake-sleep library learning. *PLDI 2021*, 835–850.

### 11.6 Logic, rewriting, and verification

- **Girard, J.-Y. (1987).** Linear logic. *Theoretical Computer Science*, 50(1), 1–101. Resource-conscious duplication, the basis for the `SPLIT` tax.
- **Lafont, Y. (1990).** Interaction nets. *POPL 1990*, 95–108. Local, port-based graph rewriting, a candidate formalism for sleep rewriting.
- **Willsey, M., et al. (2021).** egg: Fast and extensible equality saturation. *POPL 2021*. E-graphs as the engine of the logical layer.
- **Blum, A., & Mitchell, T. (1998).** Combining labeled and unlabeled data with co-training. *COLT 1998*. Conditions under which agreement between views implies correctness.

### 11.7 Prediction, complexity, and dynamics

- **Crutchfield, J. P., & Young, K. (1989).** Inferring statistical complexity. *Physical Review Letters*, 63(2), 105–108.
- **Shalizi, C. R., & Crutchfield, J. P. (2001).** Computational mechanics: Pattern and prediction, structure and simplicity. *Journal of Statistical Physics*, 104, 817–879.
- **Rissanen, J. (1978).** Modeling by shortest data description. *Automatica*, 14(5), 465–471. The MDL principle behind §3.4.
- **Willems, F. M. J., Shtarkov, Y. M., & Tjalkens, T. J. (1995).** The context-tree weighting method. *IEEE Transactions on Information Theory*, 41(3), 653–664.
- **Kearns, M. (1998).** Efficient noise-tolerant learning from statistical queries. *Journal of the ACM*, 45(6), 983–1006. Why parity-like targets resist incremental search.
- **Tucker, W. (2002).** A rigorous ODE solver and Smale's 14th problem. *Foundations of Computational Mathematics*, 2(1), 53–117. Strange attractors require proof beyond finite-precision simulation.

---

## Appendix A: Decision Record

Each row records a point where earlier designs were revised or where their intent needed stating explicitly. The **Why** column is the short rationale. Rows 1–24 record the v1 → v2 evolution; rows 25–28 record the v2 → v3 M1a metabolic economy redesign; rows 29–30 record the M1 coordinate-free and developmental topology scaling; rows 31–39 record the Pre-M2 Readiness governance and architectural decisions (`Q1`–`Q9`).

| # | Topic | Earlier Baseline (v1/v2) | Direction (v2/v3) | Why |
| :--- | :--- | :--- | :--- | :--- |
| 1 | Meaning of truth | Survivors are "true invariants" | Truth = **unrefuted**; disagreement conclusive, agreement provisional (§1.2) | No direct access to objective truth; internal checks cannot catch errors consistent with all held structure |
| 2 | Source of internal checks | Multiple channels emphasised | **Redundant derivations** suffice; channels are the strongest form (§1.3) | Deeper derivations over one channel can contradict shallow ones |
| 3 | Growth for self-checking | Growth only reduces boundary error | Add **redundancy sprouting** and comparators (§3.1.5) | Error-only growth removes the redundancy that self-checking needs |
| 4 | Aim and framing | Logic vs. physical AI, loosely stated | **Deductive**: sleep deduces in isolation; wake deduces under a **class axiom** (§1.4) | Makes "verifiable theories from primitives" rigorous and says where fallibilism enters |
| 5 | Substrate | Boolean gates | **Exact $\mathbb{Z}_{2^k}$ arithmetic, variable width**; $k = 1$ equals v1 (§2) | Verification needs exactness; rich dynamics need nonlinearity; Boolean logic is GF(2) arithmetic |
| 6 | Floats | (proposed during review) | **Not used** in the substrate (§2.1) | Tolerance-based agreement, non-associativity, and approximate equivalence defeat verification; $\{+,-\}$ alone is linear |
| 7 | Agreement primitive | `AND` "detects agreement" | **Comparator** `EQZ(SUB(a, b))` (§2.3) | `AND` detects co-activation; agreement is equality |
| 8 | Completeness at $k > 1$ | n/a | `EQZ` added (§2.3) | Polynomials over $\mathbb{Z}_{2^k}$ are not complete; indicators restore completeness |
| 9 | Width growth | n/a | Uniform-width phase W1, then heterogeneous phase W2 with `TRUNC`/`EXT` (§2.4) | Interpretability first; `TRUNC` is a homomorphism, so ring-fragment theories lift across widths |
| 10 | Tick semantics | "$O(1)$ termination per tick" | Explicit invariants: a `DELAY` in every cycle, configurable wire delay, one driver per port, `SPLIT`-only fan-out (§2.5) | Unspecified semantics would be fixed by accident during implementation |
| 11 | Dynamics vocabulary | "Strange attractors" | **Limit cycles, basins, entrainment**; strange attractors only as a continuum limit (§2.6) | Finite deterministic systems, floats included, are eventually periodic |
| 12 | Alchemy | Tied to network structure | **Logical layer**: composition, exact equivalence, theorems, closure; structure is engineering (§3) | Gates do not produce gates; the intent was always alchemy in the logical space |
| 13 | AlChemy history | Level 2 "stalled for decades"; "explicitly resolves" | Level 2 shown under specific conditions; the **combination barrier** is the target; mechanisms are **hypotheses** (§3.2, §6) | Accuracy, and claims that can be tested |
| 14 | Credit assignment | "Energy when downstream predictions match" | **Retrograde, activity-gated** flow with sensitivity, eligibility traces, and juvenile subsidy (§3.1.3) | The main risk; interior nodes otherwise starve and free-riders feed |
| 15 | Anti-silence reward | Reward only state transitions ("active flux") | **Surprisal-weighted** exact agreement (§3.1.4) | Same intent; the flux rule rewarded a free-running oscillator and penalised sparse streams |
| 16 | Sleep reward | Metabolic rules "remain active" | Boundary reward **disabled**; internal comparator agreement only (§3.1.4) | In loopback the prediction is the input, so boundary reward is meaningless |
| 17 | Threat model | Parasitic self-replicators | **Credit free-riders and bloat** (§3.1.7) | Nothing self-replicates; isolated loops starve by construction |
| 18 | Second neuron | Intuitive description | **Residual correction over $\mathbb{Z}_{2^k}$**; the $z^{-1}$ is causally required (§3.3) | An exact, testable statement of the same idea |
| 19 | Spurious attractors in sleep | Free-run survival = validation | Decided **by experiment**: S-A vs. S-B (§4.2) | Free-running networks favour short self-consistent cycles; replay of recorded facts is deduction, not supervision |
| 20 | Promotion | Survivors are frozen into macros | Retention vs. **promotion** with four tests, including formal verification (§4.3) | Asymmetry principle; claims of exact equivalence |
| 21 | Tier 3 | LFSR, Rule 30 / 110 on a single channel | **Spatially presented CA**, modular counters, LFSR **curricula**; single-channel Rule 30 excluded (§5.3) | LFSRs are parity-hard to search; Rule 30 prediction is an open problem; spatial CA fit the lattice |
| 22 | Measuring difficulty | Tiers by intuition | **ε-machines**, Berlekamp–Massey, exact synthesis (§5.2) | Measured ground truth for every learned circuit |
| 23 | Library reuse | One environment stream | **Task families** (T4) (§5.3) | Library learning needs sub-solutions that recur across tasks |
| 24 | Falsifiability | Expectations without metrics | Hypotheses, baselines, ablations, equal compute, pre-registered kill criteria (§6–§8) | Results that can count as failures as well as successes |
| 25 | Metabolism | Per-tick rent ($c_{\text{base}}, c_{\text{split}}, c_{\text{wire}}$) | **One-time description debt** charged on node maturity (§3.1.2) | Per-tick rent diverges with horizon $T$ while $L(C)$ is horizon-invariant; one-time debt guarantees $\sum \Delta L_i = L(C)$ and exact macro-micro conservation without tuning magic rates |
| 26 | Credit traces & activity | Accumulating eligibility traces & permanent activity flags | **Replacing traces** (clamped to $\le 1.0$) and **activity recency decay** with $\gamma = 0.90$ (§3.1.3) | Accumulating traces cause recurrent loops to over-credit themselves ($1/(1-\gamma) = 10$), starving combinational correctors; permanent activity flags let single-fire detours leak indefinitely |
| 27 | Reward calibration | Centered surprisal $(\mathbf{1}[\hat{s} = s] - p)(-\log_2 p)$ | **Calibrated point-forecast reward** $r = \log_2(q/p)$ or $\log_2((1-q)/(1-p))$ (§3.1.4) | Centered surprisal is a zero-mean random walk whose variance spreads as $O(\sqrt{T})$, creating a chance lottery; calibrated forecast guarantees constant circuits score $0$ on every run and bounds cumulative variance by Ville's inequality |
| 28 | Global-local economy alignment | Maintenance and reward "approximate" description length terms | Total energy change **identically equals** minus null-calibrated MDL change: $\Delta E(T) = -\Delta L_{\text{null}}(T)$ (§3.4) | With description debt (§3.1.2) and prequential calibrated reward (§3.1.4), local energy balance is an exact microscopic realization of global MDL, turning G1A.4 into a structural conservation law |
| 29 | Connectivity & graph topology | 2D planar grid nearest-neighbour routing (`LocalVonNeumann` with single-cell `SPLIT` relays and `max_relay_hops`) | **Arbitrary direct graph wiring / well-mixed connectivity** without planar relay blockage (§3.1.1) | Treating empty cells as relays rapidly created spatial congestion and blocked output ports during multi-neuron neurogenesis, forcing brittle coordinate heuristics; the physical 2D breadboard/spatial droplet metaphor had become an accidental constraint, whereas MDL description length already penalises non-local edges without artificial planar gridlock |
| 30 | Coordinate-free topology & spatial ablation closure | Legacy 2D grid coordinates, perimeter anchors, and LocalVonNeumann/Moore ablations retained in code | **Coordinate-free `substrate::Circuit` as single representation**; complete retirement of 2D grid and spatial ablations | Spatial lattice provided no behavioral value under default WellMixed wiring, imposed artificial 64-cell ceilings and placement code debt; locality is priced purely via MDL wire description length; historical reproduction anchored at commit tag `m1-final-lattice` |
| 31 | Reserve cap rule & unified $W_{\text{obs}}$ (`Q1`) | Option B draft cited $W_{\text{diag}} + t_{\text{juv}}$ vs. `economy.rs` `juvenile + 128` (`obs_capacity = juvenile + 2 W_diag`) and asserted guaranteed starvation within $\le W_{\text{obs}}$ | **Option B with unified $W_{\text{obs}} = t_{\text{juv}} + 2 W_{\text{diag}}$** via single helper (§3.1.2); drop unproved $\le W_{\text{obs}}$ guarantee; characterize multi-seed regime-shift latency empirically | Eliminates duplicate horizon math, preserves M1-validated calibrated drain scaling without adding free parameters, and avoids claiming an unproved starvation bound |
| 32 | Reactive lag & candidate search bounds (`Q2`) | Literal `max_tau = 3`, `.take(6)` feature cap, and ring range `2..=6` hidden inside `learner.rs`/`growth.rs` | Reclassify as **pre-registered search bounds** in `GrowthConfig` with observable binding telemetry (`LagHorizonBound`, `CandidateListTruncated`, `RingPeriodBound`); defer lag deepening to M2 | Separates M1-validated search geometry from compute-bounded iterative deepening until `B5` runtime scaling is measured |
| 33 | `max_nodes` compute guardrail (`Q3`) | Hardcoded `32` with duplicate pre-check and silently dropped proposals | Retain `max_nodes = 32` as a **compute guardrail** checked once at proposal time; any non-zero `SproutBlockedMaxNodes` marks the run `excluded`; confirm or revise from `B5` compute profile | Prevents structural limits from masquerading as compute exhaustion and forces guardrail saturation to surface explicitly |
| 34 | Multi-channel lock-time criterion (`Q4`) | Informal concurrent two-channel check without cross-seed single-channel baseline comparison | Run exploratory multi-seed measurement of per-channel lock tick vs. single-channel run alone (ratio distribution, Rule 8), then pre-register ratio/pooled gate in M2a (Rule 9) | Establishes an empirical feasibility baseline before locking a pass threshold |
| 35 | M2 comparator economics & milestone split (`Q5`) | Single M2 targeting both T2 coupling and spontaneous H3 comparators despite §3.4 offering zero wake comparator reward | **Split M2 into M2a** (T2 cross-channel coupling via existing corrector mechanism & `BoundaryIn(c)` features) **and M2b** (redundancy & comparators after Charter §3.1.4/§3.4 comparator-reward amendment) | Prevents M2a from hitting the predicted wake comparator-reward block or inserting an ungrounded subsidy without user-approved economics |
| 36 | Meaning of M2 success criteria (`Q6`) | Prose targets ("T2 solved", "comparators flag", "recovery from declared noise", "vs. baselines") without metric or margin | Specify M2a (generator families, $k \in \{1,2\}$, noise model, loss vs. $h_\mu$, equal-compute baselines with feasibility arguments) separately from post-Charter-amendment M2b criteria | Enforces falsifiability and feasibility arguments before any confirmatory data collection |
| 37 | Solving T2 without comparators (`Q7`) | Ambiguous whether T2 coupling must wait for comparator formation | **Acceptable for M2a to solve T2 via corrector sprouting alone**; `H3` comparator formation stays explicitly untested in M2a and is deferred to M2b/M4a | Cleanly isolates cross-channel predictive coupling (`H2`) from self-checking redundancy economics (`H3`) |
| 38 | Seed governance (`Q8`) | M1 holdout `10000..=10019` alongside ad hoc dev slices | Keep M1 holdout `10000..=10019` untouched; run M1 dev (`0..20` fast, `20..2020` multi-bank) and fresh confirmation bank `20000..=20019`; reserve disjoint M2 holdout `30000..=30019` | Prevents holdout contamination across milestone boundaries and enforces harness guards |
| 39 | Pooled multi-bank gate statistics (`Q9`) | Informal per-bank hurdle multiplication from M1b | Pre-register **99% one-sided Clopper–Pearson** pooled lower bound plus **1% $\chi^2$ across-bank homogeneity** (`verify_m1/stats.rs`), with explicit $P_{\text{target}}$ per gate family & bank count | Eliminates compound-filter bias across independent banks while detecting clustered seed failures |
