# Redesign Ideas

Replacing parameter sweeps with structural invariants requires examining the three failure points in the M1a review: recurrent trace inflation (Fix #1), identity-element credit leaks (Fix #2), and the fundamental dimensional mismatch between $O(T)$ per-tick rent and $O(1)$ description length (Fix #3). In reinforcement learning, algorithmic information theory, and game-theoretic credit assignment, these exact failure modes have well-established parameter-free solutions.

**1. Recurrent Credit Inflation (Fix #1) — Replacing vs. Accumulating Eligibility Traces**
The recurrent inflation in `addc1` occurs because a synchronous loop (`Delay(1) <-> Split`) adds its new participation on top of its discounted past participation every tick, converging to $\frac{1}{1-\gamma} = 10.0$.

* **The Domain Precedent:** In reinforcement learning, Singh & Sutton (1996) identified this exact pathology when revisiting the same state in a cycle under *accumulating traces* ($e_t = \gamma e_{t-1} + 1$). Their solution was **replacing traces** (or Dutch traces, van Hasselt et al., 2016), where a state visitation resets or caps its trace at $1.0$ ($e_t = \max(\gamma e_{t-1}, 1)$) rather than stacking infinitely. Similarly, in Holland’s Bucket Brigade and Baum’s Hayek machine, property rights prevent an agent in a cycle from billing downstream consumers multiple times for the same transaction.

* Parameter-Free Rule:Fix #1 as reviewed is already a structural rule rather than a magic number. Defining ancestry aggregation via the idempotent maximum (or clamping at the Boolean indicator of node presence, $\le 1.0$) turns ancestry weight into a true **probability/reachability bound** in $[0, 1]$ per node per tick. It requires zero tuning and passed 100/100 banks in the review.

**2. Identity Gating and the Single-Fire Detour Exploit (Fix #2) — Marginal Contribution vs. Windowed Activity**
The review exposed a critical flaw in the `ever_active` boolean flag: a detour that emits a non-identity value once at tick 0 and `0` forever after (`Add(base, Delay{init:1}(ConstLoop(0)))`) permanently unlocks a 50% credit share on every subsequent tick without ever contributing again. Using a sliding window $W$ works, but as you noted, $W$ is another magic number.

* **The Domain Precedent:** In cooperative game theory (Shapley values) and algorithmic causal responsibility (Halpern & Pearl, 2005; Janzing et al., 2013), an agent is not paid on tick $t$ for *being alive in the past*; it is paid for what its presence changes, or by **counterfactual baseline comparison**. Why does `Add(base, 0)` look sensitive to the `0` wire? Because the current sensitivity check perturbs the wire to an arbitrary non-zero value ($0 \mapsto 1$), asking *"Could this wire break the output if it misfired?"* rather than *"Did this wire help produce the output?"*

* **Pathway A — Counterfactual Pruning (Zero-Baseline / Clamp-to-Identity Intervention):** Instead of perturbing a wire to all $2^k - 1$ values and getting fooled by `0 -> 1`, test what happens when you **clamp the node's output to its algebraic identity (`0` for Add/Sub, `1` for Mul)** or **clamp it to its own running empirical mode**. If clamping a wire to `0` on tick $t$ does not change the circuit's output on tick $t$, that wire did zero work *on tick $t$*.
* **Why Sparse Correctors Survive Without `ever_active`:** Why did `A2 -> E` in `counter3` starve when only paid on the $1/3$ of ticks it outputs `1`? Because on the tick it fires `1`, it shares the pot equally with the base pathway (`Sy.out0`), even though `E` is the *rare specialist* that rescues the prediction on that tick! Under **Shapley / marginal-gain credit** (or **surprisal-weighted branch credit**), each branch into `Add(a, b)` is weighted by how much its emission reduces surprisal relative to the other branch acting alone:

$$\Delta R_b(t) = R^{\text{net}}(\text{Add}(a, b)) - R^{\text{net}}(a)$$

On the $2/3$ of ticks where $b = 0$, $\Delta R_b(t) = 0$ (so dead detours and once-at-tick-0 detours get **strictly zero** credit after tick 0, with no window $W$ needed!). On the $1/3$ of ticks where $b = 1$, `a` alone would have been **wrong** ($R^{\text{net}}(a) < 0$, paying a penalty), while `Add(a, b)` is **right** ($R^{\text{net}}(\text{Add}(a, b)) > 0$). Thus, the marginal gain $\Delta R_b(t)$ on that single tick is massive—it captures the entire swing from penalty to reward—sustaining the sparse corrector over the silent ticks purely from exact counterfactual value, with zero activity flags or windows.

**3. The Horizon Paradox in G1A.4 & G1A.2 (Fix #3) — Solving the $O(T)$ vs. $O(1)$ Conflict**
The review pinpointed why no constant maintenance scale works across all $T$:

1. **G1A.2 (Chance Lottery Check):** Across 20 seeds $\times$ multiple tasks, a blind circuit (`alt k=2 const1`) is a random walk with standard deviation $\propto \sqrt{T}$ (reaching $+64.9$ bits by luck over 2,000 seeds). To guarantee that *every* single seed ends below zero energy, cumulative maintenance must exceed the $O(\sqrt{T})$ martingale excursion bound (Law of the Iterated Logarithm: $\sim \sqrt{2 T \sigma^2 \ln \ln T}$).

2. **G1A.4 (MDL Rank Agreement):** Static MDL charges circuit description length $L(C)$ **once** ($O(1)$), whereas the metabolic economy charges maintenance **every tick** ($T \cdot M = O(T)$). As $T \to \infty$, $T \cdot M$ grows without bound while $L(C)$ stays fixed, so at $T = 4096$ *no* constant maintenance scale passes both gates.

Here are three principled, magic-number-free architectures from the literature to resolve this mismatch:

* **Pathway 1: Leaky Integrator / Exponential Moving Average (Homeostatic Metabolism)**
* **Concept:** In biological metabolism and non-stationary RL, an organism does not integrate infinite historical surplus from $t = 0$ to $T$. Energy is a **leaky capacitor** (or exponential moving average of flux) with a structural decay rate tied directly to the node's description length $\Delta L_i$:

$$E_i(t+1) = \left(1 - \frac{1}{\tau}\right) E_i(t) + R_i(t) - c_{\text{maint}}(i)$$

Or equivalently, keep `max_energy_reserve` active as a physical law of the substrate (not disabled in M1a) and measure G1A.4 using **metabolic flux rate** (or steady-state reserve + dissipated heat) rather than unclipped $T$-tick sums.

* **Why it still needs care:** A leaky integrator still has a timescale $\tau$, unless $\tau$ is eliminated by comparing rates directly.

* **Pathway 2: Prequential / Capital-Process Economy (Rent-to-Own / MDL Amortization)**
* **Concept:** Drawn from **Algorithmic Information Theory (Solomonoff / Rissanen MDL)** and **Game-Theoretic Probability / e-processes (Shafer & Vovk, 2019; Grünwald, 2007)**. In exact MDL, a circuit $C$ starts with an **initial capital debt** equal to its exact description length $L(C)$ bits (or each node $i$ is born owing its exact structural encoding cost $\Delta L_i$ bits).

* **How it works:** Instead of charging an arbitrary per-tick rent forever from $t = 0 \to \infty$, a node's structural cost is a **capital mortgage** of $\Delta L_i$ bits, plus an $O(\frac{1}{t})$ or KT-code parameter complexity cost (which grows as $\frac{1}{2}\log_2 T$ in Rissanen's two-part MDL).
* **Wait—what about pruning dead nodes in M1b and starving chance circuits in G1A.2?**
In Shafer–Vovk game-theoretic testing and **Page's CUSUM / Sequential Probability Ratio Test (SPRT)**, you separate **structural capital** ($L(C)$, paid once) from **martingale drift / sequential falsification**:
* Every node $i$ starts with initial energy $E_i(0)$ and must pay off its structural description cost $\Delta L_i$ (or hold $\Delta L_i$ as locked collateral).
* In `null_calibrated_mdl_delta`, why did `alt k=2 const1` score $+64.9$ bits on raw centered surprisal, whereas in M0's `compute_mdl` it was penalized? Because raw centered surprisal is a **zero-mean martingale** ($\mathbb{E}[R^{\text{net}}] = 0$) with positive variance $\sigma^2 > 0$! A zero-mean random walk hits $+\sqrt{T}$ half the time.

* In **Information Theory / Kelly Gambling / Universal Coding**, the true log-wealth (Kullback-Leibler divergence / KT prequential code) of a wrong or chance predictor does **not** have zero drift—it has **strictly negative drift** equal to $-D_{\text{KL}}(p_{\text{true}} \parallel p_{\text{model}}) < 0$!
* **Why did our centered surprisal have 0 drift instead of negative KL drift?** Because our reward rule `CenteredSurprisal` subtracts the *linear* expectation $p_{\text{null}}(\hat{s})(-\log_2 p_{\text{null}}(\hat{s}))$, making expected reward $0$ for any blind guess. If instead the reward is the **prequential MDL log-loss delta** (comparing the KT sequential code of the circuit's residuals $L_{\text{KT}}(\text{facts} \mid C)$ against $L_{\text{null}}(\text{facts})$ tick-by-tick), then a chance circuit has **strictly negative expected bit flux every tick** (or only $O(\log T)$ fluctuations), *and* the sum of flux minus the one-time structural cost $\Delta L_i$ **identically equals** $-\Delta L_{\text{null}}(T)$ at **every horizon $T$** ($\rho_s \equiv 1.0$ by mathematical identity!).

* **Pathway 3: Aligning the Economy Directly with Prequential MDL ($\Delta E(T) \equiv -\Delta L_{\text{null}}(T)$ by Construction)**
Look at what the review revealed in *"What drives every result"*:

> `null_calibrated_mdl_delta` uses the same prediction score as the economy reward. In the M1a panel, a circuit's energy gain is therefore its prediction score minus total maintenance, and its MDL score is the same prediction score minus its code length.
>
>

That single sentence exposes the entire root cause:

1. Currently, `null_calibrated_mdl_delta` does **not** actually use M0's KT residual code $L_{\text{KT}}(\text{facts} \mid C)$—it uses the economy's centered surprisal sum minus static code length $L(C)$!

2. Meanwhile, the economy uses the same centered surprisal sum minus $T \times C_{\text{maint}}(C)$.

3. Because centered surprisal has **zero mean** ($\mathbb{E}[R] = 0$) and **large variance** ($\sigma \approx 0.68$ bits/tick on $k=2$ alternating) for a constant guesser, the economy has to rely on a massive per-tick maintenance tax ($T \times C_{\text{maint}} \approx 68.6$ bits) just to push the $+3\sigma$ lucky tail of a zero-mean random walk (`alt k=2 const1`, $+64.9$ bits) below zero! And that huge per-tick tax then crushes large accurate circuits (`cancelsub`, `mulzero`) over long horizons $T$, breaking G1A.4.

**How Combining Marginal Credit + True Prequential Log-Loss Solves Both Gates Without Magic Numbers**
If we replace heuristic penalties with exact information-theoretic rules:

1. **Reward = Prequential KT Code Delta (Not Zero-Mean Linear Centered Surprisal):**
At each tick $t$, the environment emits $s_t$ and the port predicts $\hat{s}_t$. In M0 (`src/metrics/mdl.rs`), we already have the exact parameter-free Krichevsky–Trofimov (KT) sequential code for the residual stream! The incremental bits saved by the predictor on tick $t$ is:

$$r_p(t) = \ell_{\text{null}}(s_t) - \ell_{\text{KT}}(\hat{s}_t = s_t \mid \text{history}_{1..t-1})$$

* **What happens to `alt k=2 const1` (25% accuracy on a 50/50 2-bit alternating stream)?** Under the KT residual code, predicting with 25% accuracy (75% error rate on $\mathbb{Z}_4$) costs $\approx 2.39$ bits/tick, while the null model on alternating $(0, 3)$ costs $1.0$ bit/tick. Instead of a zero-mean lottery ticket that can reach $+64.9$ bits by luck, `const1` loses **$-1.39$ bits per tick in expectation** ($\approx -1,420$ bits over 1024 ticks, with 0 out of 2,000 seeds ever coming close to $+0$!).

* **What happens to any blind guesser on any skewed stream?** By Shannon's source coding theorem (Gibbs' inequality), if a deterministic predictor is independent of the stream or guesses a constant other than the 100% mode, its prequential code length is bounded below by the entropy plus the KL divergence + KT redundancy $\frac{1}{2}\log_2 T$.

1. **Credit Split = Clamp-to-Identity Marginal Gain + Unit-Capped Recurrent Traces:**

* Keep **Fix #1** (idempotent/unit-capped recurrent trace) so loops cannot stuff the ballot box.

* Replace `ever_active` (Fix #2) with **clamp-to-identity marginal contribution** on `Add`/`Sub`/`Mul`: an input wire gets credit on tick $t$ proportional to how much worse the port's output would be if that wire were clamped to the identity element (`0` for Add, `1` for Mul) *or* via **exponentially discounted trace of past non-zero emissions** using the **already-existing $\gamma$ (`trace_discount = 0.90`)** rather than inventing a new window parameter $W$! (Notice: we already have a temporal discount factor $\gamma$ in `EconomyConfig` for `Delay` registers—using that same $\gamma$ to decay activity weight when a branch outputs identity `0` introduces **zero new parameters**).

1. **Maintenance = Exact Structural Description Cost $\Delta L_i$ Amortized Over the Reservoir Capacity:**

* Each node $i$ has a known, parameter-free description length $\Delta L_i(\text{circuit})$ in bits from M0's `compute_mdl` (e.g., $\log_2(\text{primitives}) + \text{wire encoding bits}$).

* Instead of inventing three independent magic numbers (`c_base`, `c_split`, `c_wire`) that don't even match the ratio of code lengths in `mdl.rs` (as the review noted: *"Current maintenance is 1.9–2.8x code length, with small circuits charged the most"*), define every node's maintenance cost directly from its exact M0 MDL bit cost $\Delta L_i$!
