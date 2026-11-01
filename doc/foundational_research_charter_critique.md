# A Constructive Critique of the Foundational Research Charter

> Scope: a review of [foundational_research_charter.md](foundational_research_charter.md) (*Axiomatic Neurogenesis and Open-Ended Learning*). It looks at the direction the charter points in, what may be new about it, where I disagree with it and why, and what I would add before any code is written.

## 0. Summary Verdict

The charter is a **good first step and points in a worthwhile direction**. Its main strengths:

- It picks a **tiny, deterministic, fully observable** testbed where every claim can be measured.
- It treats **structure, not weights**, as the thing being learned, and it insists on **local, gradient-free** mechanisms.
- It has a real and under-explored idea at its centre: **agreement between spatially separate pathways works as an internal error signal, and that signal is still there when the world is switched off.** That is what makes "sleep" possible.
- It frames hierarchy-building (Level 1 → Level 2) as **compiling verified dynamics into new primitives**. That is library learning moved from symbolic programs into a spatial, dynamical substrate.

I disagree with it substantively in five places. Each is covered in [Section 2](#2-substantive-disagreements):

1. **Sleep, as specified, checks coherence, not truth.** Free-running stability is a necessary condition for a good model but not a sufficient one, and the spurious attractors it favours are exactly the ones Crick & Mitchison argued dreaming should *remove*.
2. **The active-flux rule rewards the simplest parasite in the system**: a two-node `NOT`→`DELAY` oscillator.
3. **A finite, deterministic Boolean network has no strange attractors.** It only has limit cycles. This is a terminology problem, but it hides a real measurement question.
4. **The AlChemy lineage is invoked more strongly than the architecture supports.** By swapping constructive λ-calculus for fixed Boolean gates with an external growth rule, the design leaves the artificial-chemistry paradigm. The history of the Level 2 "bottleneck" also needs correcting.
5. **The Tier 3 difficulty ladder is placed in the wrong spot.** LFSRs are *native* to the primitive set but *parity-hard* to find by incremental search. Predicting the centre column of Rule 30 is an open problem.

Most of the remaining risk sits in **underspecified mechanisms** ([Section 3](#3-underspecified-mechanisms-that-will-decide-the-outcome)), above all credit assignment. Whether the project works depends almost entirely on how energy flows from the boundary back to interior nodes, and the charter does not yet say.

---

## 1. What Is Promising, and What Could Be New

None of the individual parts is new. Error-driven unit addition goes back to cascade-correlation (1990), energy economies of rules to Holland's classifier systems, spatial soups to Tierra and Avida, and wake/sleep library learning to DreamCoder. What could be new is **the particular combination and the claim it makes possible**:

| Ingredient | Prior art | What the charter adds |
| :--- | :--- | :--- |
| Growing structure from residual error | Cascade-correlation (Fahlman & Lebiere 1990), boosting, NEAT | Growth is *local* and *metabolically funded*, on a sequential Boolean substrate with explicit memory (`DELAY`) |
| Energy economy for credit | Holland's bucket brigade, Baum's Hayek machine | Economy coupled to a *spatial* lattice, so locality and credit flow share one geometry |
| Agreement between views as supervision | Co-training (Blum & Mitchell 1998), Thousand Brains voting, self-distillation | Agreement used **offline**: the network checks itself with no external data stream |
| Wake/sleep compression into primitives | DreamCoder, Explore–Compress, CGP module acquisition | Compression of *recurrent dynamical circuits* with timing, not stateless λ-expressions |
| Interpretability | Mostly absent in neural approaches | Every learned model is a sequential Boolean circuit that can be **read, minimised, and formally checked against the generator** |

The direction the charter points toward is a measurable account of **representational open-endedness**: a learner whose *vocabulary* grows, where each new word is a verified, compressed piece of the world's dynamics, and where the hierarchy can be audited at every level. If that worked even at toy scale it would be a meaningful result for artificial life (the open-endedness problem; Taylor et al. 2016, Packard et al. 2019) and for continual learning.

The last row of the table is undersold. Because the substrate is digital logic, the project gets mature tooling almost for free: exact synthesis, BDD and SAT-based equivalence checking, and the ABC logic-synthesis system. Few learning paradigms can *prove* that what was learned matches the generating process. This one can, and the charter should say so.

---

## 2. Substantive Disagreements

### 2.1 Coherence is not correspondence: sleep filters inconsistency, not falsehood

**The charter's claim (§3.3, Phase II.2):** if a pathway learned a brittle coincidence while awake, driving the network with its own predictions makes branches diverge. Brittle structure therefore self-destructs and true invariants survive.

**Why I disagree.** A closed-loop generative model is stable whenever it is *self-consistent*, whether or not it is *true*. Three cases:

1. **Spurious correlations survive.** Suppose channel $B$ happened to equal $A$ delayed by two ticks for the whole wake period. A network that learned this produces a perfectly self-consistent dream. Nothing internal can tell that the correlation is accidental, because the evidence against it is in data the network never saw. Only new observation or intervention can catch it.
2. **Single-channel models get no filter at all.** Tier 1 and Tier 3 streams can be modelled by one pathway. If no second pathway predicts the same quantity, there is nothing to disagree with, and sleep cannot reject anything.
3. **Free-running favours spurious attractors.** In attractor networks, the states a network falls into when run without input are typically the *spurious* ones: fixed points, short cycles, and mixtures. That is the basis of Crick & Mitchison's (1983) "reverse learning" hypothesis, which says the function of dreaming is to *unlearn* the modes the network falls into on its own. Hinton's wake-sleep algorithm and Boltzmann machine learning formalise the same idea: learning signal = *wake statistics − free-running statistics*. The charter currently treats "survives free-running" as evidence *for* a structure. The literature suggests it is at best neutral, and often evidence *against*.

There is also an accounting gap. In loopback, $\hat{A}(t+1)$ *is* $A(t+1)$ by construction, so any boundary-prediction reward is automatically earned. The charter says metabolic rules "remain active" during sleep. It needs to say explicitly that boundary rewards are **switched off** in sleep and that energy comes only from internal agreement.

**Recommendation.** Keep closed-loop dreaming, but make sleep a **two-phase comparison**, not a survival test:

- **Replay:** re-drive the network with *buffered* wake traces, as hippocampal replay does in Complementary Learning Systems theory. This tests the circuit against real data under controlled conditions.
- **Free-run:** loop predictions back, as the charter describes, and record the statistics of the dreamed stream (transition frequencies, short n-gram distribution, cycle length).
- **Consolidate** structures whose free-run statistics *match* the replayed wake statistics. **Penalise** structures whose dreams collapse into short cycles or drift away from the wake distribution.

This keeps the charter's intuition and fixes the failure mode. Internal agreement becomes the *cheap first filter*, and statistical match with experience is the *acceptance test*.

### 2.2 The active-flux rule rewards the simplest parasite

**The charter's claim (§3.3, Phase II.3):** rewarding only correctly predicted *transitions* ($\Delta x \neq 0$) stops sleep from collapsing into silence.

**Why I disagree.** The cheapest maximum-flux structure on the substrate is a two-node ring $x(t+1) = \lnot x(t)$, built from one `NOT` and one `DELAY`. It needs no input, flips every tick, is perfectly self-consistent in loopback, and any two copies started in phase agree forever. If the wake stream has any period-2 component, the ring also earns real reward. Under the flux rule, then, the most likely thing to consolidate is a metronome.

The flux rule also has the opposite bias. Streams with long runs, such as sparse events or the high-order bits of a counter, are *under*-rewarded even when predicted perfectly, because they rarely change. That biases the learner toward high-frequency structure.

**Recommendation.** Replace "reward transitions" with an **information-theoretic** criterion:

- Reward **surprise reduction**: $\log_2$-loss improvement over a cheap null predictor (for example, "repeat the last bit" or the empirical bit frequency). A metronome only earns where the stream really has period-2 structure.
- In sleep, use the wake-versus-dream **distribution match** from §2.1. A metronome's dream (strictly alternating) has the wrong statistics for almost any non-trivial stream and is penalised automatically.

A smaller point in the same area: §3.2 says `AND` "outputs 1 only when two independent input pathways agree". It does not. `AND(0,0) = 0`. `AND` detects **co-activation**. **Agreement** is `XNOR` (`NOT ∘ XOR`). This matters, because "agreement on silence" is exactly the degenerate case the flux rule is trying to handle. Being exact about which gate detects what will make the sleep rules easier to get right.

### 2.3 There are no strange attractors in a finite deterministic Boolean network

**The charter's claim (§2.2, §3.3):** validated concepts are "strange attractors" (or stable periodic basins).

**Why I disagree.** A network with $N$ `DELAY` bits, run autonomously in sleep loopback, is a deterministic map on a finite state space of size $2^N$. Every trajectory is **eventually periodic**. Strange attractors need a continuous state space, sensitive dependence, and fractal geometry, and none of these can exist here. The same holds for the generators: an $n$-bit LFSR has period at most $2^n - 1$, and Rule 30 on a finite ring is periodic too.

When the network is *driven* (wake), the right concept is not an attractor at all but **entrainment, or generalised synchronisation**, of a non-autonomous finite-state transducer to its input. This is the language of echo-state and reservoir computing.

**Recommendation.** Use the precise terms (*limit cycle*, *attractor basin*, *transient length*, *synchronisation*) and measure them directly:

- cycle length and transient length in free-run,
- basin size: the fraction of perturbed `DELAY` states that return to the same cycle,
- robustness: Hamming-distance recovery after bit-flip perturbations.

These numbers are cheap to compute exactly on small circuits, and they make "thinking as attractor" an empirical claim rather than a metaphor.

### 2.4 The AlChemy lineage is invoked more strongly than the architecture supports

**The charter's claim (§2.3):** the architecture "explicitly resolves" the three 1994 bottlenecks that stalled Level 1 → Level 2 for decades.

**Why I disagree, in three parts.**

1. **The history needs nuance.** Fontana & Buss (1994) *did* report Level 2 organisations. They arose when Level 1 organisations were combined, sometimes with mediating "glue" expressions. What has resisted progress is *spontaneous, robust* integration. Mathis, Patel, Weimer & Forrest's 2024 revisit (*Self-Organization in Computation & Chemistry: Return to AlChemy*) found Level 1 organisations to be more robust and common than expected, but hard to combine. The original work also handled non-termination pragmatically with bounded reduction, and Level 0 copiers dominated unless copy reactions were filtered out. The honest framing is "we target the combination barrier", not "it stalled for decades and we resolve it".
2. **The architecture is no longer an artificial chemistry.** AlChemy's organising power comes from **constructive self-reference**: objects act on objects to *produce new objects*. In the charter, gates never produce gates. Structure changes only through an **external meta-rule** (sprouting, pruning, sleep compression). That is a sound engineering choice, but it makes the system a *developmental, growing neural network with an energy economy*, in the same family as NEAT, cascade-correlation, Cartesian Genetic Programming, and classifier systems. "Level 1 organisation" in Fontana's sense (closure under interaction, self-maintenance) is not the same as "a recurrent subgraph that keeps earning energy". Bottleneck 1 is not *resolved*; it is *removed* by giving up constructive universality.
3. **"Parasites" mean something different here.** Nothing in this substrate self-replicates, so there are no parasites in the Tierra or AlChemy sense. The real threat is **free-riders on the credit path**: structures that sit inside a rewarded pathway and draw energy without helping. Examples are a `NOT`–`NOT` pair or a redundant `DELAY` chain, the circuit equivalent of genetic-programming "bloat" and introns. Milestone 3's injected self-excitation loop, which gets no reward and pays maintenance, will die by construction. It tests nothing.

**Recommendation.** Present the AlChemy problem as **motivation and long-term horizon**, and present the current architecture as an engineered, tractable stand-in. Swap the "resolution" column for a "hypothesis" column (see §2.7). Rewrite Milestone 3 around **credit exploits and bloat**: inject no-op detours and redundant branches into working pathways, and measure whether the economy prunes them. If constructive closure is the long-term goal, put it on the roadmap explicitly: a later phase in which the sprouting and compression rules are *themselves* things represented in the substrate (see §6).

Also on lineage: Lafont's interaction nets are cited, but nothing in the design uses them. They would fit naturally as the **formalism for sleep-phase graph rewriting** (local, confluent, port-based compression). Either adopt them there or drop the citation.

### 2.5 The Tier 3 ladder is placed in the wrong spot

**The charter's claim (§3.1, Milestone 4):** LFSRs and elementary CA (Rule 110 / Rule 30) are "deep algorithmic invariants" that macro-axioms will unlock.

**Why I disagree.**

- **An LFSR is literally made of `DELAY`s and `XOR`s.** It is native to the primitive set, not deep. The hard part is **search**, and there the problem is severe. Any circuit that uses only a *proper subset* of the correct taps predicts the stream at chance level. Partial solutions get **no partial credit**, so incremental, residual-driven growth has no gradient to follow. This is the parity problem, which is hard for statistical-query and local learners (Minsky & Papert; Kearns 1998). Macro-axioms do not help unless smaller parities were already learned as useful sub-results, and for a single LFSR they are not.
- **Predicting the centre column of Rule 30 is an open problem.** It is the subject of Wolfram's 2019 prize problems, and the column is used as a pseudo-random generator. It should not be a milestone.
- **Rule 110 is computationally universal.** As a single observed channel it is effectively arbitrary.

**Recommendation.**

- **Present CA spatially.** Feed a CA row as $K$ parallel channels along one boundary. The local rule (radius 1) then becomes learnable by local wiring, which fits the lattice's own geometry exactly. This is a better Tier 3: deep-looking dynamics, learnable through spatial locality.
- **Build LFSRs as a curriculum.** Use task *families* in which short-tap parities are learned first and reused, for example a set of LFSRs that share tap sub-patterns. This is also what library learning needs (§5.6).
- **Define tiers with computational mechanics** rather than by hand (§5.1), so that difficulty is measured, not asserted.

### 2.6 The epistemological framing: induction in a synthetic physics, not deduction

**The charter's claim (§1.1):** mathematics is a counterexample to physical grounding. Once axioms are assumed, convergence of independent deductions confirms truth as reliably as touch confirms sight.

**Why I disagree in part.** The argument is attractive, but the system as designed does **not do deduction**. It does **inductive next-step prediction on a black-box generator**. That is an empirical science in a synthetic world. Its "axioms" are *representational primitives*, not premises from which theorems follow. Its notion of truth is *predictive adequacy*, not proof. In other words, the charter has built a small **physical-AI setting**, with bitstreams standing in for senses and channels standing in for modalities. That is fine and arguably the right choice, but the charter should not present it as an *alternative* to grounding. It is grounding, made minimal.

Two related points:

- "Without … an external teacher" (§1) is not quite accurate. The world's next bit *is* the teacher. That is self-supervision, and it is a strength, so say it plainly.
- The bootstrapping problem has not gone away. It has moved into the **meta-rules**: sprouting thresholds, pruning, sleep scheduling, macro detection, energy constants. The *object level* is minimal and the *meta level* is hand-designed. Saying so makes the research question sharper: *how small can the hand-designed meta level be?*

(On the history: Newton's plague-year work was closely tied to optics and gravitation, so it is not a clean example of mathematics done without empirical motivation. The point stands, but the example is weaker than it looks.)

### 2.7 "Resolves" should read "hypothesises"

The §2.3 table says the architecture "explicitly resolves" each bottleneck. None of these has been tested. Rewording the column to **"Proposed mechanism (Hypothesis H*n*)"** and linking each row to the milestone that tests it would make the charter more credible, and it costs nothing.

---

## 3. Underspecified Mechanisms That Will Decide the Outcome

These are not objections. They are the places where an implementation will be forced to make choices the charter has not made, and where those choices will largely decide whether the milestones succeed.

### 3.1 Tick semantics (needed before any code)

- **Combinational cycles.** Does a signal cross several gates in one tick? If so, every cycle **must** contain at least one `DELAY`, or the network is ill-defined (combinational loops oscillate or have no fixed point). This should be a hard structural invariant enforced at sprouting time.
- **Wire delay.** Does a hop to a neighbouring cell cost a tick? If yes, distance becomes delay, and cross-channel timing depends on geometry. That is interesting, but it makes Tier 2 much harder. If no, the lattice limits topology but not timing. Either is defensible. Choose deliberately.
- **Complexity.** "$O(1)$ termination per tick" is $O(1)$ *per node*, so $O(N)$ per tick in total, given the acyclicity invariant above.
- **Port ownership.** Which node drives $\hat{A}$? Is there one driver per port, or a vote? This decides how competing hypotheses coexist.
- **Fan-out and discard.** `SPLIT` is the only fan-out. Can an output go unconnected (linear-logic *weakening*)? If unused outputs are free, the "linear" story is weaker than claimed.

### 3.2 Credit assignment: the central risk

"Energy is replenished only when downstream boundary predictions match" (§3.2) leaves open *how energy reaches interior nodes*. This is the problem Holland's bucket brigade struggled with: credit decays over long chains, and free-riders collect it. Baum's Hayek machine addressed it with explicit property rights and auctions. Without a concrete mechanism, interior nodes far from the boundary will starve before they become useful.

**Suggested mechanism: retrograde, activity-gated energy flow.** This is biologically grounded and fully local. In the neurotrophic theory of development, neurons survive by receiving trophic factor (for example NGF) sent *backward* from the targets they innervate. The analogue here: a rewarded boundary port releases energy that flows *backward* along edges, split in proportion to each upstream edge's **contribution on the rewarded tick**. Contribution can be measured locally, for example by whether flipping that input would have flipped the output (a local sensitivity, computable exactly for Boolean gates). Add:

- a **juvenile subsidy** (a grace period with an energy endowment) so new sprouts can show their worth before starving,
- **eligibility traces** so energy can reach structures whose contribution was several ticks earlier, which is needed for anything that goes through `DELAY`.

### 3.3 A reward baseline

A constant-output node earns energy on any biased stream. A random node earns on about half of ticks. Reward should be **relative to a null model** (§2.2), or trivial predictors will set the energy floor for the whole system.

### 3.4 Uncertainty

A single Boolean output cannot say "I don't know". Milestone 2 adds noise spikes, and a deterministic predictor must commit to a bit on every tick. Options include population or vote coding at the ports, a separate "abstain" line, or rate coding over a short window. Without some such mechanism, robustness to noise and confidence cannot be told apart.

### 3.5 The sprouting rule

"Persistent, structured error triggers sprouting" needs a **local test for structure**. Candidates: autocorrelation of the residual at small lags, a tiny local Lempel–Ziv or n-gram compressibility estimate, or **mutual information between the residual and a neighbouring signal**. The last is exactly cascade-correlation's candidate-unit criterion, and it gives a natural way to choose the new node's *inputs* as well as its location.

### 3.6 Macro identification and interfaces

"Surviving recurrent subgraphs … are identified, frozen, and compressed" (§3.3.4) hides a hard problem: **where does a subgraph end?** Some principled options:

- **Small cut / small interface:** few ports relative to internal size.
- **Markov-blanket test:** the subgraph's future internal state is predictable from its ports alone.
- **Causal-emergence measures** (Hoel's effective information) for choosing the grain at which a macro is "more causal" than its parts.

Also: frozen macros should be **internally immutable but retirable**, with usage-based decay and a description-length cost on the size of the palette (§5.4). Otherwise the palette grows without limit.

---

## 4. Missing Lineage Worth Engaging

| Area | Work | Why it matters here |
| :--- | :--- | :--- |
| Error-driven growth | Fahlman & Lebiere (1990), *Cascade-Correlation* | The closest ancestor of the "second neuron": add a unit to fit the residual |
| Rule economies | Holland (1986), classifier systems and bucket brigade; Baum (1999), *Hayek machine* | The credit-assignment failure modes this design will run into |
| Circuits as genomes | Miller (2011), *Cartesian Genetic Programming*; Walker & Miller, Embedded CGP (module acquisition) | Gate-level evolved circuits that compress subgraphs into modules: a direct baseline |
| Sleep and attractors | Crick & Mitchison (1983), reverse learning; Hinton et al. (1995), wake-sleep; Tononi & Cirelli, synaptic homeostasis | Sleep as unlearning or downscaling, which cuts against "survivors are good" (§2.1) |
| Memory consolidation | McClelland, McNaughton & O'Reilly (1995), Complementary Learning Systems | Replay of real experience, not free generation |
| Multi-view agreement | Blum & Mitchell (1998), co-training | Formal conditions (view independence) under which agreement implies correctness |
| Optimal prediction | Crutchfield & Young (1989); Shalizi & Crutchfield (2001), computational mechanics and ε-machines | Ground-truth minimal predictors and complexity measures for every generator (§5.1) |
| Compression as learning | Rissanen, MDL; Schmidhuber, compression progress | A global objective the local economy can be checked against (§5.4) |
| Boolean dynamics | Kauffman (1969), random Boolean networks | Decades of results on attractors, cycle lengths, and criticality in exactly this kind of substrate |
| Emergent replicators | Agüera y Arcas et al. (2024), *Computational Life* | Current evidence on when self-organisation emerges from computational soups |
| AlChemy revisited | Mathis, Patel, Weimer & Forrest (2024), *Return to AlChemy* | Current status of the Level 1 → Level 2 combination barrier |
| Parity hardness | Minsky & Papert (1969); Kearns (1998), statistical-query learning | Why LFSR-style targets resist incremental search (§2.5) |
| Open-endedness | Bedau & Packard activity statistics; Taylor et al. (2016) | Established ways to measure whether novelty keeps accumulating |
| Causality and action | Pearl (2009), *Causality* | Why the later action phase is necessary, not optional (§6) |

---

## 5. Additions and Guidance

### 5.1 Use computational mechanics as the yardstick

Every generator in the tier ladder is a stationary, finite-state process, so its **ε-machine** (minimal optimal predictor) can be computed exactly. That gives, for each stream:

- **Statistical complexity** $C_\mu$: the bits of memory any optimal predictor *must* have. This is a hard lower bound on the number of `DELAY`s a learned circuit needs.
- **Entropy rate** $h_\mu$: the floor on achievable prediction error once noise is added.
- **Markov order and synchronisation time**: how much history is needed before prediction becomes possible.

Tiers can then be **defined** by these numbers rather than by intuition, and every learned circuit gets a score: *distance from the optimal error* and *memory overhead above $C_\mu$*. This one addition turns the milestones from qualitative expectations into quantitative results.

### 5.2 Formalise the 1 → 2 neuron transition as residual correction over GF(2)

The charter's intuition here is better than it looks, and it can be made exact. Let $y_1(t)$ be the first unit's prediction of $A(t+1)$, and let its residual be

$$r(t) = y_1(t) \oplus A(t+1).$$

A second unit produces $\hat{r}(t)$, and the corrected prediction is $\hat{y}(t) = y_1(t) \oplus \hat{r}(t)$. The combined error is then $r(t) \oplus \hat{r}(t)$: the residual of the residual. This is **boosting over GF(2)**, and it stacks. A chain of correctors $y_1 \oplus \hat{r}_1 \oplus \hat{r}_2 \oplus \dots$ is a well-defined architecture in which each new unit fits what the previous ones got wrong.

It also *explains* the charter's $z^{-1}$. The residual for tick $t$ is not known until $t+1$, so the corrector must work from **delayed** residuals and inputs. The delay in the second neuron is causally required, not an arbitrary design choice. Milestone 1 can then be stated exactly: *the residual stream of unit 1 has entropy rate below 1 bit/tick, and a corrector unit lowers the combined error by an amount predicted by that entropy rate.*

### 5.3 Make sleep a positive/negative phase comparison

See §2.1. In short: **replay** buffered wake traces (positive phase), **free-run** the loopback (negative phase), and consolidate structures whose statistics agree across the two. That brings the charter's sleep into line with a well-understood learning principle and blocks the metronome and echo-chamber failure modes.

### 5.4 Anchor the local economy to a global objective

Local rules are hard to debug when there is no global quantity they are meant to improve. A natural candidate is **total description length**:

$$L = L(\text{palette}) + L(\text{circuit} \mid \text{palette}) + L(\text{wake history} \mid \text{circuit}).$$

Maintenance cost approximates the second term, prediction reward approximates the third, and a macro-axiom earns its place if it lowers the first two terms by more than it costs. The metabolic constants can then be **calibrated** so that local energy changes track changes in $L$. A run in which energy rises while $L$ also rises is a bug detector.

### 5.5 Use logic synthesis and formal verification for sleep compression

Sleep compression is, technically, **sequential logic minimisation**. Do not reinvent it:

- **Exact synthesis** for tiny circuits gives the true minimum size for Tier 1/2 targets, so a learned circuit's distance from optimal can be reported.
- **Sequential equivalence checking** (SAT- or BDD-based) can *prove* a consolidated macro matches the generator on all reachable states. Of everything in the project, this is the claim most other learning paradigms cannot make.
- **Logic minimisation** (ABC-style rewriting) can strip free-rider detours during sleep, which directly addresses the bloat problem from §2.4.

### 5.6 Library learning needs a task distribution

DreamCoder's library learning works because sub-solutions *recur across many tasks*. With one generator there is little pressure for reuse, and a "macro-axiom" is just a frozen copy of the current solution. To test the Level 2 hypothesis properly, the environment should be a **family of generators that share sub-structure**: CA rules with common neighbourhood logic, LFSRs with shared taps, channels that recombine a few base rhythms. The key metric is then **transfer**: does a macro learned on generator $g_i$ shorten time-to-solution on a related $g_j$?

### 5.7 Experimental standards (baselines, ablations, kill criteria)

The milestones are currently *expectations*, with no metrics, baselines, or conditions under which they would count as failed. I suggest:

- **Metrics:** log-loss or bits/tick relative to $h_\mu$; node and `DELAY` count relative to $C_\mu$; energy budget; ticks to solution; palette size and reuse.
- **Baselines:**
  - random circuit search at equal size and compute,
  - CGP with a $(1+\lambda)$ evolution strategy,
  - a small RNN or LSTM trained by backpropagation through time,
  - **Context-Tree Weighting**: a strong, near-optimal universal sequence predictor that sets a demanding upper reference,
  - exhaustive enumeration for the smallest tiers.
- **Ablations:** sleep vs. no sleep; lattice vs. well-mixed wiring; `SPLIT` cost $> 0$ vs. $= 0$; flux rule vs. information-gain rule; replay vs. free-run only.
- **Equal compute:** sleep must beat *more wake time* under the same tick budget. Otherwise its benefit cannot be told apart from extra computation.
- **Kill criteria:** for example, "if sleep consolidation does not beat compute-matched wake-only learning on Tier 2 by margin X across N seeds, revisit the sleep design before going on to Level 2."

### 5.8 Implementation notes for the Rust codebase

- **Bit-parallel simulation.** Every primitive is a bitwise operation, so a `u64` can carry **64 parallel universes** through one fixed circuit in a single instruction: 64 noise realisations, 64 perturbed dream start states, or 64 phase offsets. This does not parallelise *structural* search, because structure differs between seeds, but it makes energy estimation, basin-size measurement (§2.3), and macro verification almost free.
- **Determinism.** Give every source of randomness (sprouting position and gate choice, noise injection) a seeded RNG, and make runs fully reproducible from a seed and configuration file. Treat this as a requirement, not an extra.
- **Separate the layers.** Keep the *simulator* (tick semantics, invariants), the *economy* (energy rules), the *meta-rules* (sprouting, pruning, sleep), and the *environment* (generators) behind distinct traits. Most of the research will consist of swapping one layer while keeping the others fixed.

---

## 6. Where the Direction Could Lead

Taking the long view, the charter points at three questions that are open and worth pursuing:

1. **Measurable open-ended representation.** A system whose palette grows by verified, compressed pieces of the world's dynamics gives a *quantitative* handle on open-endedness: palette growth, reuse depth, and total description length over time. Most open-endedness research lacks a measure this clean.
2. **From observation to intervention.** The charter rightly puts action later. But action is not just an add-on. It is the **only** way to tell correlation from causation (Pearl), and so the only real defence against the spurious-coherence problem in §2.1. The boundary-port design (§3.1) is a good structural hook. The roadmap should treat the action phase as the point where the system's epistemology becomes complete, not as an optional extension.
3. **Closure of the meta level.** At present the rules that grow, prune, and compress structure sit outside the substrate. The step that would bring the project back to Fontana & Buss's original ambition is making those rules **representable and modifiable in the substrate itself**, so that the system can grow its own growth rules. That is far off, but naming it as the horizon gives the early work a clear purpose.

---

## 7. Suggested Revised Roadmap

| Stage | Goal | Success criterion |
| :--- | :--- | :--- |
| **M0 — Simulator and yardstick** | Tick semantics, acyclicity invariant, seeded determinism, generator library with ε-machine ground truth, baseline harness | Baselines reproduce known optima on Tier 1; all runs reproducible from seed |
| **M1a — Economy on fixed structure** | Validate energy and credit flow (§3.2) on hand-built correct and incorrect circuits, *before* any growth | Correct circuits gain energy; free-rider detours lose it; reward tracks $\Delta L$ |
| **M1b — 1 → 2 transition (GF(2) residual)** | Error-driven sprouting of a corrector unit (§5.2) | Combined error falls as the residual's entropy rate predicts; no backpropagation |
| **M2 — Cross-channel bridges** | Pathways from separate boundaries meet and form comparator (`XNOR`/`XOR`) sites | Tier 2 solved; recovers from noise spikes; compared against CGP and CTW |
| **M3 — Credit-exploit resistance** | Inject no-op detours and redundant branches (§2.4) | Bloat is pruned; circuit size converges toward exact-synthesis optimum |
| **M4a — Sleep as filtering** | Replay plus free-run comparison (§2.1, §5.3) | Beats compute-matched wake-only; metronomes and collapsed modes are rejected |
| **M4b — Consolidation** | Compress verified subgraphs into macros with formal equivalence checks (§5.5) | Each macro is proven equivalent to its source subgraph; $L$ decreases |
| **M4c — Transfer** | Generator families with shared sub-structure (§5.6) | Macros shorten time-to-solution on related generators |
| **M5 — Action** | Boundary ports perturb or query the generator | Spurious correlations from §2.1 are identified and pruned by intervention |

---

## 8. Bottom Line

**Is it a solid start?** Yes. The testbed is the right size, the commitment to local, gradient-free, structural learning is clear, and the core idea (internal cross-pathway agreement as a learning signal that works offline, plus compression of verified dynamics into new primitives) is worth a serious research programme.

**Is it the right first step?** Almost. Before writing simulation code I would:

1. fix tick semantics and the credit-assignment mechanism,
2. replace the flux rule with an information-gain criterion,
3. change sleep from a *survival test* into a *wake-versus-dream statistical comparison*,
4. change the language from "strange attractor" to "limit cycle" and from "resolves" to "hypothesises",
5. adopt ε-machines as the yardstick and add baselines, ablations, and kill criteria to every milestone.

**Is there a reasonable path forward?** Yes. The roadmap in §7 keeps the charter's spirit and ambition, puts the most uncertain mechanism (the energy economy) at the front where it can fail cheaply, and makes each later claim depend on a measurable earlier result.
