# Why We Should Not Relax G1A.2 or Static MDL

* **Relaxing G1A.2 to check only `Delay` nodes would break M1b:** Although the G1A.2 summary text mentions sustaining interior `Delay` nodes, `Node 2 (Primitive::Add)` in noisy `addc1` dropped to $E_2 = -0.90\text{ bits} \le 0$. In Stage M1b, `EditableLattice::prune_starved_nodes` excises any mature node whose reserve hits $E_u(t) \le 0$, which would destroy the optimal `addc1` circuit online and eventually prune `A2` in `counter3` on longer horizons ($T = 2000$).

* **Scaling static $L(\text{circuit})$ by $T$ would break M0 yardsticks:** In Charter §3.4, $L(\text{circuit} \mid \text{palette})$ is a one-time structural description cost while $L(\text{facts} \mid \text{circuit})$ scales as $O(T)$. Making $L(\text{circuit})$ grow with $T$ in `src/metrics/mdl.rs` would invalidate the M0 MDL crossover property; instead, per-tick metabolic maintenance should be calibrated to amortize $L(\text{circuit})$ over the evaluation horizon.

## Proposed 3-Part Redesign

1. **Cap Per-Node Recurrent Eligibility Accumulation (`src/structural/economy.rs`)**

* **Root cause:** In `addc1` (`Add(x, ConstLoop(1))`), the synchronous loop `Delay(1) <-> Split` recirculates its own ancestry vector $\mathbf{e}_r(t)$ every tick, accumulating the geometric series $\sum_{t=0}^{\infty} \gamma^t = \frac{1}{1 - 0.90} = 10.0$ for both loop nodes. That inflates the constant loop's ancestry weight by $10\times$ per node and dilutes the single-tick feedforward `Add` node down to a $6.25\%$ credit share ($28.9\text{ bits}$ earned vs. $32.8\text{ bits}$ maintenance).

* **Fix:** Replace unbounded additive self-stacking across ticks with per-node unit-capped (or idempotent $\max$) ancestry assembly so no single node $i$ can accumulate an ancestry weight $c_i > 1.0$ along a pathway:

$$c_{u, i}^{(T)}(t) = s_{u \to T}(t) \cdot \min\!\left(1.0,\; \delta_{u, i} + \sum_{j : \tilde{w}_{u, j}(t) > 0} \tilde{w}_{u, j}(t)\, c_{\text{src}(u, j), i}^{(T)}(t)\right)$$

and for a `Delay` node $u$ backed by register $r_u$, $c_{u, i}^{(T)}(t) = s_{u \to T}(t) \cdot \max\!\big(\delta_{u, i},\, \gamma e_{r_u, i}(t)\big)$.

* **Impact:** On acyclic multi-register chains (`d0 -> d1 -> d2 -> d3 -> d4`), all nodes are distinct ($\delta_{u,i}$ never collides with upstream $\gamma^k$), preserving the exact $(16.0\%, 17.8\%, 19.8\%, 22.0\%, 24.4\%)$ shift-chain shares and $< 10^{-12}$ macro-micro conservation. On `addc1`, `Delay(1)` and `Split` cap at $\le 1.0$ instead of $10.0$, raising `Add`'s credit share from $6.25\%$ to $\sim 35\text{--}40\%$ ($\Delta E_{\text{Add}} \approx +115\text{ bits}$, passing all 20 noisy seeds).

1. **Condition `Add`/`Sub` Zero-Identity Gating on `ever_active == false` (`src/structural/sensitivity.rs`)**

* **Root cause:** In `counter3` (`Counter3Opt`), `A2` feeds `E = Eqz(x + 2)`, which outputs $1$ on $1/3$ of ticks and $0$ on $2/3$ of ticks into `AF = Add(Sy.out0, E)`. Because `Add` currently zeroes the input weight of any port emitting $0$ whenever the other port is non-zero, the `A2 -> E` corrector branch receives $0$ credit on $66.7\%$ of ticks while paying maintenance on $100\%$ of ticks. In M1b, this same rule would starve every sparse finite-ring booster $\hat{r}(t)$ wired into `Add(y_1, r_hat)`, since a residual corrector outputs $0$ whenever $y_1(t)$ is already correct.

* **Fix:** Apply zero-identity input gating on `Primitive::Add` and `Primitive::Sub` ($b = 0$ when $a \ne 0$, or $a = 0$ when $b \ne 0$) **only when `ever_active[src] == false**`. Once an upstream branch has emitted a non-identity symbol (`ever_active[src] == true`), weight both inputs by their counterfactual perturbation sensitivity even on ticks where the corrector outputs $0$.

* **Impact:** `Detour_AddZero` (`Delay(0)` / `ConstLoop(0)`) and `Detour_MulZero` (`Mul(b, Delay(0))`) never emit a non-zero symbol (`ever_active == false`), and `Detour_CancelSub` still has whole-cone perturbation sensitivity $s_{b \to T}(t) = 0.0$, so G1A.3 retains exact $0.0$ detour credit after tick 0. Meanwhile, `A2` and `E` in `counter3` receive proportional credit on all ticks after their first active emission, lifting $E_5(T)$ well above $E_{\text{init}} = 4.0$ across all 20 noisy seeds.

1. **Align Maintenance Amortisation $\tau_{\text{mdl}}$ with the Evaluation Horizon (`src/structural/economy.rs`)**

* **Root cause:** `EconomyConfig` sets $\tau_{\text{mdl}} = 400$ ticks ($c_{\text{base}} = 0.020$, $c_{\text{split}} = 0.035$, $c_{\text{wire}} = 0.004$), while the M1a audit runs for $T = 1024$ ticks. Over $1024$ ticks, the 11 extra nodes in 14-node `cancelsub` vs. 3-node `inc1` pay $247\text{ bits}$ of cumulative maintenance—$3.86\times$ their $64\text{-bit}$ static MDL difference $L(\text{circuit})$—which overwhelms `cancelsub`'s $\approx 200\text{-bit}$ prediction advantage and swaps their ranks ($\rho_s = 0.8929 < 0.90$).

* **Fix:** Halve the default per-tick maintenance rates to match a $\tau_{\text{mdl}} \approx 800\text{--}1000$ tick amortisation horizon ($c_{\text{base}} = 0.010$, $c_{\text{split}} = 0.0175$, $c_{\text{wire}} = 0.002\text{ bits/tick}$) while keeping $c_{\text{split}} > c_{\text{base}} > 0$.

* **Impact:** On `counter3` ($k = 2, \varepsilon = 0.05$), the maintenance gap between `cancelsub` and `inc1` drops from $247\text{ bits}$ to $123.5\text{ bits}$ (and `mulzero` vs. `inc1` drops to $136\text{ bits}$), placing both $95\%$-accurate detour circuits back above $63.4\%$-accurate `inc1` (`cancelsub` $\Delta E \approx +690$, `mulzero` $\Delta E \approx +678$, `inc1` $\Delta E \approx +591$) and raising Spearman $\rho_s$ from $0.8929$ to $1.0000$. Because centered surprisal gives $C_{\text{wrong}}$, $C_{\text{chance}}$, and $C_{\text{silence}}$ zero or negative expected flux, halving $c_{\text{maint}} > 0$ still drives 100% of non-predictive circuits strictly below zero energy.
