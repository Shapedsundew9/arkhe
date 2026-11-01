# Foundational Research Charter: Axiomatic Neurogenesis and Open-Ended Learning

## 1. Epistemological Premise: How Do We Know What Is True?

A central paradox of continuous learning in artificial intelligence is the **bootstrapping problem**: a system seemingly needs an existing world model to evaluate, filter, and integrate new information, yet it must construct that world model from scratch without a fixed context window, pre-baked representations, or an external teacher.

Returning to first principles, how does any intelligence determine what is true—meaning what is invariant, reliable, and safe to extrapolate from?

### 1.1 Physical Grounding vs. Logical Invariance

In biological organisms, truth is established through **multi-sensory cross-referencing**. Touch, sight, sound, smell, and taste provide independent physical pathways into the brain. When multiple sensory modalities converge on the same prediction across different neural pathways, the underlying concept is reinforced as a consistent "world model." This observation drives the modern argument for *Physical AI*: that true intelligence requires embodiment in a physical environment.

However, **mathematics and pure logic offer a striking counterexample**:

* During the Great Plague of 1665–1666, Isaac Newton developed calculus in near-total isolation, without needing new physical apparatus to validate each theorem.
* Once a foundational set of **axioms** is assumed, mathematics becomes a self-contained universe.
* In an abstract logical environment, multi-sensory confirmation is replaced by **logical invariance**: when two independent chains of deduction or computation process different facets of a system and converge on the exact same state, that convergence confirms an underlying structural truth just as reliably as touch confirming sight.

### 1.2 Core Research Question

> *Can we define a minimal set of computational axioms—analogous to the first primitive neurons and their metabolic constraints—within a self-contained mathematical environment such that a learning system can autonomously sprout new structure, cross-reference independent pathways, settle into self-reinforcing strange attractors ("thinking"), and periodically consolidate its discoveries ("sleep") into higher-order derived axioms?*

---

## 2. Theoretical Pillars & Novel Synthesis

Our research sits at the intersection of **evolutionary neurobiology**, **dynamical systems theory**, and **Algorithmic Chemistry (AlChemy)**, resolving classic bottlenecks in each by combining them into a unified architecture.

### 2.1 The First and Second Neuron: Homeostasis and Residual Error

Biological brains did not begin as large, randomly initialized matrices waiting for global gradient descent.

1. **The First Neuron (Homeostatic Predictor):** In early metazoans, the proto-neuron was a direct metabolic bridge between a boundary sensor and an effector. In our abstract framing, the "first neuron" is a single primitive operator node dropped into a data stream. It survives only if its output reduces prediction error on the immediate next state, earning metabolic energy.
2. **The Second Neuron (Error-Driven Interneuron):** When the first neuron locks onto a dominant local pattern, it inevitably fails on deeper exceptions, generating a structured **residual error stream**. Rather than resizing a fixed network, persistent non-random error acts as the local trigger for **neurogenesis**—spawning a second interneuron with a temporal delay ($z^{-1}$) specifically to predict *when and how the first neuron will be wrong*.

### 2.2 "Thinking" as a Dynamical Feedback Loop and Strange Attractor

When interneurons wire back into existing pathways, they form recurrent feedback loops.

* In this framework, **"thinking" is an active, self-reinforcing feedback loop**.
* A validated concept, invariant, or internal "theory" is not a static weight; it is a **strange attractor** (or stable periodic attractor basin)—a recurrent pattern of activity that maintains coherence across varied inputs and reinforces itself when independent pathways agree.

### 2.3 Overcoming the 1994 Algorithmic Chemistry Bottlenecks

In 1994, Walter Fontana and Leo Buss demonstrated in *The Arrival of the Fittest* that colliding $\lambda$-calculus expressions in an abstract reactor spontaneously self-organize into **Level 1 organizations** (autocatalytic, self-maintaining feedback loops). However, transitioning from **Level 1** to **Level 2** (organizations composed of multiple Level 1 organizations interacting as higher-order primitives) stalled for decades due to three bottlenecks, which our architecture explicitly resolves:

| Classical Bottleneck (1994) | Our Architectural Resolution |
| :--- | :--- |
| **1. The Halting Problem & Self-Consumption:** Unrestricted $\lambda$-calculus reductions can loop infinitely between time steps, and pure combinator evaluation destroys the graph structure as signals pass through it. | **Synchronous Logic + Linear Resource Bounds:** We separate *signal flow* from *graph rewriting*. During runtime, nodes are persistent synchronous bitwise primitives with explicit $z^{-1}$ unit delays, guaranteeing $O(1)$ termination per tick. Signal duplication (`SPLIT`) carries a linear metabolic cost, starving runaway parasites. |
| **2. Parasitic Collapse in a Well-Mixed Soup:** In a global collision space, a single trivial self-replicator ("parasite") rapidly consumes all resources and overwrites complex attractors. | **2D Spatial Lattice Topology:** Nodes exist on a discrete 2D grid and can only wire to local neighbors. Separate sensory channels enter at different boundaries, allowing distinct Level 1 attractors to form as spatially isolated "droplets" that only cross-reference where their signaling wavefronts physically meet. |
| **3. Destructive Interference at Level 2:** Directly wiring two recurrent Level 1 graphs together scrambles their internal feedback timing and destroys both attractors. | **Biomimetic Wake/Sleep Consolidation:** We introduce an offline **Sleep Phase** that acts as an automated program-synthesis and subgraph-compression pass, sealing verified Level 1 subgraphs into immutable, multi-port **Derived Macro-Axioms**. |

---

## 3. System Architecture: The Dual-Timescale Loop

### 3.1 The "Synthetic Physics" Environment & The Quenched Observation Regime

Instead of a 3D physics engine, the environment is a deterministic generator emitting $K$ parallel bitstreams (e.g., Channel $A$, Channel $B$, Channel $C$) along distinct boundaries of the 2D grid.

#### Observation First, Action Later

In physical AI, an organism can act on its world to test hypotheses. However, in a vast or "quenched" physical system, a nascent organism's actions are initially inconsequential to the macroscopic state of its environment—it must first learn to survive by predicting external invariants.

* **Initial Observational Phase:** We begin in a purely observational regime where the environment's state transitions are unaffected by the network. At each tick $t$, the network receives boundary inputs $A(t), B(t)$ and emits boundary predictions $\hat{A}(t+1), \hat{B}(t+1)$.
* **The Architectural Bridge to Action:** By designing the boundary output ports ($\hat{A}, \hat{B}$) as explicit physical interfaces on the grid perimeter, those exact same ports provide the structural hook for **effector actions** in later phases—where the network's boundary outputs will actively perturb or query the generator to test its internal theories.

The generator's hidden mathematical invariants are organized in escalating tiers:

* **Tier 1 (Local Temporal Invariants):** Simple alternating rhythms or short cyclic periods solvable by $1\text{--}2$ primitive nodes with unit delay $z^{-1}$.
* **Tier 2 (Cross-Channel Logical Coupling):** Channel $B$ encodes a delayed parity or logical combination (`XOR`) of phase shifts in Channel $A$, requiring two independent spatial pathways to converge and cross-reference.
* **Tier 3 (Deep Algorithmic Invariants):** Streams driven by Linear Feedback Shift Registers (LFSRs), elementary cellular automata (e.g., Rule 110 / Rule 30), or modular arithmetic counters that appear chaotic at the surface unless the network builds hierarchical internal state models.

### 3.2 The Foundational Axiom Set (Level 0 Primitives)

Every cell on the 2D grid is either empty or occupied by a node holding a small metabolic energy reserve $E_i(t)$. Each tick costs a baseline maintenance energy $c_{\text{base}}$ per node and wire. Energy is replenished only when downstream boundary predictions match the true next environmental bit.

The system starts with only five foundational axioms:

1. **`AND` (Coincidence / Confirmation):** Outputs $1$ only when two independent input pathways agree. Serves as the foundational mechanism for cross-referencing.
2. **`XOR` (Discrepancy / Residual Error):** Outputs $1$ when two signals disagree. Directly isolates prediction error between an internal expectation and a sensory stream.
3. **`NOT` (Inversion / Inhibition):** Flips a bit, enabling inhibitory feedback loops and boolean completeness.
4. **`DELAY` ($z^{-1}$ Unit Memory):** Retains its input bit for exactly one clock tick ($y(t) = x(t-1)$), providing the temporal state required for recurrent feedback loops and attractors.
5. **`SPLIT` (Linear Fan-Out):** Duplicates a single input signal to two output ports at an elevated metabolic tax ($c_{\text{split}} > c_{\text{base}}$), enforcing linear logic resource conservation so signals cannot proliferate without predictive payoff.

### 3.3 Why Brains Must Sleep: Boundary Loopback & Self-Reinforcement

Continuous biological brains cannot grow new synapses indefinitely while awake without accumulating noise, redundant detours, and runaway parasitic feedback loops. Our system alternates between two regimes:

#### Phase I: WAKE (Online Metabolism & Error-Driven Neurogenesis)

* External sensory streams $A(t), B(t)$ drive the grid boundaries.
* Existing pathways propagate signals synchronously, earning energy when predictions $\hat{A}(t+1), \hat{B}(t+1)$ match reality and strengthening correlated edges.
* Where prediction error remains persistently high and structured (detected via `XOR` residuals), local energy surplus triggers **sprouting**: spawning adjacent primitive nodes to model the residual variance.

#### Phase II: SLEEP (Closed-Loop Dreaming, Consistency Filtering, & Axiom Crystallization)

During sleep, there is no external supervisor grading the network. Instead, the system tests its own internal consistency by **regenerating its own inputs**:

1. **Boundary Loopback ("Dreaming"):** External sensory inputs are severed, and the network's boundary prediction ports are short-circuited directly back into its sensory input ports ($\hat{A}(t+1) \rightarrow A(t+1)$ and $\hat{B}(t+1) \rightarrow B(t+1)$). Retaining the internal $z^{-1}$ register states from the end of the wake cycle seeds the loop, so the network immediately drives itself with its own extrapolated world model.
2. **Cross-Pathway Consistency as the Survival Filter:** As the self-generated stream propagates through the interior, local metabolic rules remain active. If a pathway learned a brittle coincidence while awake, driving the network with its own echoed predictions causes separate branches to diverge within a few ticks. When internal pathways collide and disagree (`XOR` $= 1$), local error spikes, energy drains, and contradictory edges are pruned. Conversely, if two independent pathways captured true coupled invariants, their internal signals continue to converge and agree (`AND` $= 1$) inside the closed loop—settling into a self-sustaining **strange attractor**.
3. **Preventing the "Echo Chamber" Flatline (Active Information Flux):** A network outputting constant $0$s on every pathway is trivially non-contradictory ($0$ always agrees with $0$). To prevent sleep from lobotomizing the network into static silence, metabolic reinforcement during both wake and sleep requires **active information flux**: energy is awarded only when pathways accurately predict or cross-confirm *state transitions* ($\Delta x(t) \neq 0$) across distinct temporal paths. Static silence costs zero energy to hold, so it does not immediately die, but it cannot accumulate the reinforcement required for consolidation.
4. **Subgraph Encapsulation ($\text{Level } 1 \rightarrow \text{Level } 2$):** Surviving recurrent subgraphs that maintain stable, high-flux attractor dynamics across the sleep cycle are identified, frozen, and compressed into a single **Derived Macro-Axiom** added to the system's palette. In subsequent Wake cycles, the network can spawn this entire verified circuit as a single atomic unit.

---

## 4. Falsifiable Expectations & Research Milestones

To ensure our research remains grounded and measurable, we define four progressive hypotheses that the system must confirm:

1. **Milestone 1 — Autonomous 1-to-2 Neuron Transition (Error-Driven Growth):**
   * *Expectation:* Dropped into a Tier 1/2 stream, a single primitive node will lock onto the primary alternation, and its structured residual error (`XOR`) will autonomously trigger the sprouting of a secondary interneuron that captures the secondary phase shift—reducing total error without global backpropagation.
2. **Milestone 2 — Emergent Multi-Pathway Cross-Referencing:**
   * *Expectation:* When Channel $A$ and Channel $B$ contain coupled invariants injected at opposite edges of the 2D grid, localized pathways growing from each boundary will meet in the interior and form `AND`/`XOR` confirmation bridges that survive noise spikes injected into either individual channel.
3. **Milestone 3 — Attractor Stability & Parasite Resistance:**
   * *Expectation:* Because of the 2D spatial topology and linear `SPLIT` metabolic cost, artificially injecting a parasitic self-excitation loop into one region of the grid will remain locally contained and starve from lack of predictive reward rather than overwriting established predictors.
4. **Milestone 4 — Level 2 Scaling via Closed-Loop Sleep Consolidation:**
   * *Expectation:* When external inputs are severed and boundary predictions are looped back into sensory inputs during Sleep, brittle pathways will self-destruct via internal `XOR` discrepancy, while invariant subgraphs will sustain active-flux limit cycles. Compressing these surviving attractors into reusable macro-axioms will allow the system to model Tier 3 sequences (e.g., multi-bit LFSRs) that cause Wake-only networks to plateau.

---

## 5. Foundational Reading & Intellectual Lineage

1. **Evolutionary Neurobiology & First Principles of Brains**
   * **Bennett, M. (2023).** *A Brief History of Intelligence: Evolution, AI, and the Five Breakthroughs That Made Our Brains.* Mariner Books. *(Foundational reference for how early bilaterian nervous systems evolved from simple homeostatic steering reflexes into valence, reinforcement learning, and internal world simulation).*
   * **Braitenberg, V. (1984).** *Vehicles: Experiments in Synthetic Psychology.* MIT Press. *(Demonstrates how minimalist cross-wired sensorimotor feedback loops generate complex emergent behaviors and internal dynamics).*
2. **Algorithmic Chemistry & Self-Maintaining Organizations**
   * **Fontana, W., & Buss, L. W. (1994).** "The arrival of the fittest": Toward a theory of biological organization. *Bulletin of Mathematical Biology*, 56(1), 1–64. *(Introduces AlChemy, defining Level 0 collisions, Level 1 autocatalytic organizations, and the theoretical challenge of reaching Level 2 meta-organizations).*
   * **Dittrich, P., Ziegler, J., & Banzhaf, W. (2001).** Artificial chemistries—a review. *Artificial Life*, 7(3), 225–275. *(Surveys spatialized chemistries, lattice topologies, and resource conservation laws that prevent parasitic collapse).*
3. **Resource-Bounded Logic & Local Graph Rewriting**
   * **Girard, J.-Y. (1987).** Linear logic. *Theoretical Computer Science*, 50(1), 1–101. *(Establishes resource-conscious logic where propositions cannot be freely duplicated or discarded without explicit structural operators).*
   * **Lafont, Y. (1990).** Interaction nets. *Proceedings of the 17th ACM SIGPLAN-SIGACT Symposium on Principles of Programming Languages (POPL)*, 95–108. *(Provides a deterministic, strongly normalizing graphical rewrite formalism with local port-to-port reductions).*
4. **Wake-Sleep Consolidation & Library Learning**
   * **Ellis, K., Wong, C., Nye, M., Sablé-Meyer, M., Morales, L., Hewitt, L., Cary, L., Solar-Lezama, A., & Tenenbaum, J. B. (2021).** DreamCoder: Bootstrapping inductive program synthesis with wake-sleep library learning. *Proceedings of the 42nd ACM SIGPLAN International Conference on Programming Language Design and Implementation (PLDI)*, 835–850. *(Formalizes the wake-sleep cycle where recurring sub-solutions discovered while awake are compressed into new higher-order primitives during sleep).*
