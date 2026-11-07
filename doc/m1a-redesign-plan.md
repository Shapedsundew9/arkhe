# M1a Redesign Plan

- **Status:** agreed 2026-11-07; not yet implemented.
- **Replaces:** the three fixes in [m1a-revisited-proposal.md](./m1a-revisited-proposal.md).
- **Inputs:** [m1a_gate_findings.md](./m1a_gate_findings.md) (the failed dev run), [m1a-revisited-review.md](./m1a-revisited-review.md) (the 100-bank review of the proposal), and [ideas.md](./ideas.md) (literature-based redesign ideas).

This document records what we will change in the M1a economy and **why**, so the reasoning is kept in the git history next to the code. It also fixes, before any of the runs that judge them, the pass criteria for each step and the amended gate wording.

## Summary

The M1a development run triggered two kill criteria: G1A.2 (correct vs. incorrect separation) and G1A.4 (ΔE tracks −ΔL_null). The review of the first redesign traced the remaining conflict to one root cause. For a blind circuit the reward is a zero-mean lottery, so the economy needs a large per-tick rent to keep lucky runs below zero. Per-tick rent grows with the horizon T, while MDL charges code length once, so the two can agree at only one horizon. The only configuration that passed reliably was found by sweeping a constant, which the project rules forbid.

We will make four changes. None of them introduces a tunable constant.

| # | Change | Addresses | Approach |
| :--- | :--- | :--- | :--- |
| D1 | Cap each node's ancestry weight at 1 (replacing traces) | G1A.2 per-node check (`addc1`, `counter3`) | Implement directly |
| D2 | Replace centred surprisal with a calibrated point-forecast reward | G1A.2 chance-lottery check | Implement, then verify (step 2) |
| D3 | Charge each node's share of the description length once, instead of rent every tick | G1A.4 horizon conflict | Implement, then verify (step 3); depends on D2 |
| D4 | Let input activity decay with the existing trace discount γ instead of a permanent flag | Single-fire detour leak (M1b risk) | Implement, then verify (step 4) |

With D2 and D3, a circuit's total energy change equals minus its null-calibrated MDL change, on every run and at every horizon. G1A.4 then holds by construction. The empirical content of M1a moves to where charter §3.1.3 puts the project's main risk: does local credit let every useful node repay its own description cost (G1A.2), and stop useless nodes from doing so (G1A.3)?

## Why the current design fails

Three findings from the review drive every decision below.

1. **The economy and MDL share one score.** [`null_calibrated_mdl_delta`](../src/metrics/mdl.rs) and the economy's [`observe_predictions`](../src/structural/economy.rs) compute the same centred-surprisal flux F. The M1a panel runs with no juvenile period and no reserve cap. So a circuit's energy change is ΔE = F − T·M and its MDL score is −ΔL_null = F − L(C), where M is the circuit's total per-tick maintenance and L(C) its code length. G1A.4 therefore depends only on T·M against L(C), and no change to credit splitting can affect it.
2. **Zero mean is not zero on every run.** The centred rule was chosen because a blind guesser's *expected* flux is zero ([milestone_1_plan.md](./milestone_1_plan.md) §3.2, invariant 1). Its variance is positive, though, so its cumulative flux is a random walk whose spread grows like √T. Over 2000 seeds, `alt k=2 const1` has mean −8.9 bits, standard deviation 21.7 and maximum +64.9.
   - G1A.2 checks every run, so maintenance must beat the walk's upper tail: T·M must exceed a few σ√T.
   - G1A.4 wants T·M close to L(C), which does not depend on T.
   - Both can hold only near one horizon. The review measured this: the best maintenance scale is ×0.8 at T = 1024 and ×0.7 at T = 2048, and no scale works at T = 4096.
3. **Loops over-credit themselves.** In `addc1`, the loop `Delay(1) ↔ Split` adds its own ancestry back in every tick, so its weight converges to 1/(1 − γ) = 10. The `Add` node that does the work gets about 6% of the credit. This causes the per-node failures in G1A.2, and it is independent of the lottery.

The proposal's halved maintenance and the review's ×0.8 sweep both move along the trade-off in point 2 without removing it. This plan removes it in two steps. First, score predictions so that luck cannot build up (D2). Then charge code length the way MDL does, once (D3).

## Decisions

### D1 — Cap recurrent credit at 1

**What.** In [`ancestry_for_target`](../src/structural/sensitivity.rs), clamp every ancestry entry to at most 1:

$$c_{u,i} = s_{u \to T} \cdot \min\Big(1,\ \delta_{u,i} + \textstyle\sum_j w_{u,j}\, c_{\mathrm{src}(u,j),\,i}\Big)$$

For a `Delay` node, the inherited term is γ·e_{r,i}, the discounted trace of its register. The proposal gave two cap formulas. They are equivalent once every entry is at most 1, so this one formula replaces both.

**Why.**

- This is the *replacing* eligibility trace of Singh & Sutton (1996). A node that fires on the path resets its entry to 1 instead of adding 1 to its decayed past, so a loop cannot vote for itself again every tick. The weight becomes a reachability bound in [0, 1], and no parameter is added.
  - [ideas.md](./ideas.md) writes the replacing trace as e_t = max(γe_{t−1}, 1), which is always 1. The correct form is "1 on a visit, γe_{t−1} otherwise", and the clamp above produces exactly that.
- The review's evidence:
  - On its own, D1 passes G1A.2's per-node check in 100 of 100 banks.
  - It rescues `counter3`: A2 ends 15.5 bits above its starting energy.
  - It leaves the shift-chain shares unchanged (`discounted_register_ancestry_preserves_credit_through_delay_chains`).
- It matters for M1b. Without it, loops would out-earn the `Add` correctors that growth creates.

### D2 — Calibrated point-forecast reward

**What.** For each output port on tick t, define:

- ŝ: the circuit's prediction.
- s: the observed fact.
- p = p_null(ŝ): the null model's probability for the predicted symbol, before it sees s.
- M: the number of earlier ticks on which this port's prediction was exactly right.

Then

$$\hat q = \frac{M + \tfrac12}{t + \tfrac12 \cdot 2^k}, \qquad q = \max(\hat q,\ p), \qquad r = \begin{cases} \log_2 \dfrac{q}{p} & \text{if } \hat s = s, \\[1ex] \log_2 \dfrac{1 - q}{1 - p} & \text{otherwise.} \end{cases}$$

The current rule is r = (𝟙[ŝ = s] − p)·(−log₂ p).

**How to read it.**

- The circuit's point prediction becomes a probability forecast: probability q on ŝ, and the remaining 1 − q spread over the other symbols in the null model's proportions.
- r is the number of bits this forecast saves over the null model on the symbol that actually arrived. Σr is therefore exactly L_null(facts) − L(facts | circuit) for a sequential (prequential) code.
- q̂ uses the null model's own prior for a single symbol. [`NullModelKind::LaplaceFrequency`](../src/metrics/null_model.rs) is a KT estimator, (n_s + ½)/(t + ½·2^k), so q̂ is the same estimator applied to the event "the prediction was right". At k = 1, q̂ is exactly the KT estimator M0 uses in [`kt_binary_sequence_bits`](../src/metrics/mdl.rs).

**Implementation.**

- Implement the rule once, as a small per-port type in `src/metrics/`.
- Call it from both `observe_predictions` (as a new `RewardMode`, made the default) and `null_calibrated_mdl_delta`, so the economy and MDL cannot drift apart.
- Keep `UncenteredSurprisal` and `RawAccuracy` for the charter §8 reward ablation.
- M0's `compute_mdl` yardstick is unchanged.

**Why.**

- **Constant predictors score exactly zero on every run.** If ŝ never changes, M equals the null model's count for ŝ. Then q̂ = p exactly, and r = 0 on every tick. This applies to `const0`, `const1` and the silence circuits, which are the review's lottery tickets. It is guaranteed on every run, not just on average.
- **No credit for being wrong.** q is never below p, so a predictor that is no better than the null model on its own predicted symbol scores exactly zero. That includes inverted predictors such as `osc2_anti`. M0's [`facts_code_bits`](../src/metrics/mdl.rs) takes the same stance when it refuses an "implicit free inversion discount".
- **Luck cannot grow like √T.** Suppose the stream really were generated by the null model. The forecast is a probability distribution fixed before s arrives, so 2^{Σr} would be a non-negative martingale with mean 1. By Ville's inequality, the chance that Σr ever reaches +c bits would then be at most 2^{−c}, at any horizon. Panel streams are structured, not drawn from the null model, so for non-constant blind circuits such as `osc3` this holds only approximately. Step 2 measures it.
- **Informative predictors earn more, not less.** A predictor with accuracy a earns about log₂(a/p) per hit. The cost of learning a (about ½·log₂ T bits) is paid automatically inside the sequential code. No separate parameter charge is needed, which is why D3 can charge for structure alone.
- **The exact-match principle survives.** r depends only on whether the prediction matched exactly, as charter §3.1.4 requires.

**Sanity check, not evidence.** A throwaway Python model of the `alt` task with ε = 0.05 substitution noise, written outside the repository and then deleted, gave the cumulative flux F below. Values are min to max in bits, over 2000 simulated runs at T = 1024 and 500 at T = 4096. The real panel measurement is step 2.

| Circuit on `alt` | Centred, T = 1024 | New, T = 1024 | Centred, T = 4096 | New, T = 4096 |
| :--- | :--- | :--- | :--- | :--- |
| `const1`, k = 2 (chance) | −84.5 to +50.7 | 0 to 0 | −137.2 to +101.8 | 0 to 0 |
| `osc3`, k = 2 (chance) | −10.5 to +34.5 | −1.3 to +4.2 | −31.6 to +53.2 | −1.3 to +2.9 |
| `osc2_anti`, k = 1 (wrong) | −480.0 to −436.0 | −3.2 to +2.0 | −1902.9 to −1797.8 | −3.2 to 0.0 |
| `osc2`, k = 2 (correct) | +492.8 to +541.8 | +707.4 to +862.8 | +1966.8 to +2078.0 | +2952.8 to +3299.9 |

Under the centred rule, the best chance run doubles when T quadruples (`const1`: +50.7 to +101.8), which is the √T signature. Under the new rule, constant circuits score exactly zero, and the others stay within a few bits at both horizons. That is well below the code length of any panel circuit (about 25 bits for a `ConstLoop`, about 50 for `osc3`).

**What it costs.**

- Wrong circuits are no longer drained every tick. In the sanity check, `osc2_anti` scores about −480 bits under the centred rule and about 0 under the new one. Wrong circuits still lose energy, but only their description debt from D3.
- An established circuit that starts failing is punished hard. Each miss costs log₂((1 − q)/(1 − p)), which is large when q is high. This is the charter's "pruning on disagreement" (§3.1.6), arising from the code rather than from a separate rule.
- The exact-zero property relies on q̂ using the null model's prior. It holds for `LaplaceFrequency`, the panel's null model. `RepeatLast` would need its own matched estimator.

### D3 — Charge description length once (description debt)

**What.** Remove per-tick maintenance (`c_base`, `c_split`, `c_wire`). Instead, each node owes a share ΔL_i of the circuit's code length L(C), computed by [`circuit_code_bits`](../src/metrics/mdl.rs). The debt is charged once, through the existing maintenance channel, on the node's first mature tick. In M1a that is tick 0, because `juvenile_ticks = 0`.

Shares are allocated as follows:

- Each node owes its own node bits: palette index, `Delay` init bits, and lattice placement bits.
- Each wire is owed by the node that drives it. Wires from `BoundaryIn` are owed by the node they feed.
- The header bits (Elias-γ codes of the node and edge counts) are split equally between nodes.

So Σ ΔL_i = L(C) exactly. The panel's palette term is 0.

Bits with no node to owe them are left unallocated: a direct `BoundaryIn` → `BoundaryOut` wire, or the header of a circuit with no nodes. The substrate allows both, but no M1a panel circuit has either. Step 3 asserts this rather than inventing an owner (see [Open questions for M1b](#open-questions-for-m1b)).

**Why.**

- **It is what MDL does.** Charter §3.4 says maintenance should approximate L(circuit | palette) and reward should approximate L(facts | circuit).
  - With D2 and D3, both are exact. So ΔE(T) = F − L(C) = −ΔL_null(T) on every run and at every horizon.
  - The horizon conflict disappears, because the only term that grows with T is the data term, and the two sides share it.
  - §3.4's warning that "a run in which total energy rises while L also rises signals a bug" becomes an invariant.
- **There is no constant to choose.** Per-tick rent needed a rate, and every rate the review tried was fitted to T = 1024. The debt is read off the existing code-length function.
- **D2 makes it safe.** Under the centred reward, charging only code length lets some chance circuit beat the null model in 99 of 100 banks (review). Under D2, constant circuits score exactly 0 and the others a few bits, so their ΔE ≈ −L(C) < 0.
- **The `SPLIT` tax falls out of the code.** A `SPLIT` drives two wires, so it owes two wires' bits. Charter §3.1.2's c_split > c_base therefore needs no separate constant. This is why wires are charged to the node that drives them. The current rent charges them to the consumer.
- **The per-node check gets a clearer meaning.** min_i E_i(T) > E_init now reads "every node of a correct circuit earned more than its share of the description length". At T = 1024, current maintenance is 1.9–2.8× code length (review). So correct circuits face a lower bar than now, while chance circuits are held below zero by D2 instead of by rent.

**What it costs.**

- **G1A.4 becomes an identity.** It turns into a regression guard rather than evidence. This is the strongest form of the charter's M1a success criterion ("energy changes track ΔL"), not a weaker one, but the gate no longer tests anything new. The empirical content moves to G1A.2 and G1A.3.
- **There is no background drain.** Under rent, small credit leaks were swamped by maintenance. Under debt, a node that leaks more than its share stays positive forever. That is why D4 and the new detour test become more important after D3.
- **It raises M1b design questions**, most importantly idle nodes (see [Open questions for M1b](#open-questions-for-m1b)).

### D4 — Let input activity decay with γ

**What.**

- Replace the permanent `ever_nonzero` / `ever_non_one` flags in `NodeActivity` with recency weights in [0, 1].
- A node's weight is set to 1 on a tick when it emits a non-identity value. Otherwise it is multiplied by γ, the existing `trace_discount` of 0.90.
- In [`ring_identity_input_weights`](../src/structural/sensitivity.rs), an identity-valued input currently gets weight 1 or 0 depending on the flag. It gets its recency weight instead.

**Why.**

- **It closes the single-fire leak.** The review's detour `Add(base, Delay{init:1}(ConstLoop(0)))` fires once at tick 0 and then emits 0 forever.
  - The permanent flag keeps paying it whenever both `Add` inputs are 0. Under the current code that leaks 28–263 bits, and the detour stays positive on 7 of 9 task/width pairs.
  - With recency weights, its share decays as γ^t, falling below 1% after 44 ticks. The total leak is bounded and stops growing.
- **It adds no new constant.** The review's fix was a window W (W = 16, 64 and 128 behaved the same).
  - γ already sets how fast credit fades with distance in time through `Delay` registers.
  - Reusing it for how fast credit fades after an input's last activity adds nothing new, and γ is not re-tuned.
- **It keeps sparse correctors.**
  - A corrector that fires every few ticks keeps most of its weight: `counter3`'s E fires every 3 ticks, so its weight never drops below γ² = 0.81.
  - On the ticks where it fires, it is credited in full.
  - Weights are normalised per node, so only relative recency matters.

**Why not counterfactual (Shapley-style) credit ([ideas.md](./ideas.md) §2).**

- Marginal contributions can be negative, but [`apply_tick`](../src/structural/economy.rs) only accepts non-negative ancestry shares.
- Clamping an input to its identity value treats "predicts 0" as "absent", but 0 is a legitimate prediction.
- Counterfactuals through `Delay` registers span several ticks, so they need re-simulation. That adds a cost for every node on every tick, on top of the current sensitivity analysis.
- Its motivating case, `counter3` starving, is already fixed by D1.

### Rejected alternatives

| Option | Source | Why rejected |
| :--- | :--- | :--- |
| Keep credit for inputs that were ever active (Fix #2) | Proposal | Pays for presence, not contribution: the single-fire detour collects 220–800 bits and survives on all 9 task/width pairs. Adds nothing once D1 is in (98 vs. 98 banks). |
| Halve maintenance (Fix #3) | Proposal | Passes 24 of 100 banks; its arithmetic is wrong (see the review's table). |
| Scale maintenance ×0.75–0.8 | Review sweep | Passes 96–98 of 100 banks at T = 1024, but ×0.8 drops to about 72% at T = 2048, and no scale passes at T = 4096. Fitted to the gate's horizon; forbidden by the no-magic-number rule in `.github/copilot-instructions.md`. |
| Maintenance proportional to code length | Review | At most about 59% of banks at any multiplier, and the multiplier is still a free constant. |
| Leaky-integrator energy | [ideas.md](./ideas.md) pathway 1 | Introduces a timescale τ; a reserve cap under another name. |
| M0's `facts_code_bits` as a per-tick reward | [ideas.md](./ideas.md) §3 | Its anti-inversion floor is defined on whole-run totals, so its tick-by-tick differences are not a forecast and lose the Ville bound. It prices every miss at log₂(2^k − 1) bits whichever symbols occur, so its −1.39 bits/tick for `const1` reflects that pricing, not information. |
| Relax G1A.2, or accept G1A.4 as triggered | Review options 2 and 3 | Unnecessary if D2 and D3 work. They remain the fallback if step 2's stop rule fires. |

## Pre-registered gate amendments

This wording is fixed now, before the runs that judge it. Step 5 copies it into [milestone_1_plan.md](./milestone_1_plan.md) §6.1.

| Gate | Current | Amended |
| :--- | :--- | :--- |
| G1A.1 | Per-tick macro-micro conservation < 10⁻¹²; no floats in `substrate`; sampled sensitivity matches exact | Unchanged. The debt goes through the maintenance channel, so the per-tick conservation check applies as it is. |
| G1A.2 | `C_opt`: ΔE(T) > 0 and min_i E_i(T) > E_init. `C_wrong`, `C_chance`, `C_silence`: ΔE(T) < 0 | Wording unchanged. Under D3, the per-node condition means every node earned more than its debt ΔL_i. |
| G1A.3 | Detour nodes get 0.0 credit after the first tick (ΔE_detour = −T·C_maint); every detour has ΔE below its reference | Detours that never fire: 0.0 credit after tick 0 (ΔE_detour = −ΔL_detour). New `onefire` detour, `Add(Base, Delay{init:1}(ConstLoop(0)))` on every non-constant task: its detour set is the `Delay` and `ConstLoop` nodes and its reference is `Base`; every detour node must end with E_i(T) < E_init. All detours: ΔE(detour) < ΔE(reference) on 100% of runs. |
| G1A.4 | Spearman ρ ≥ 0.90 on the first seed of each group; zero sign inversions | On every run, $\lvert \Delta E(T) + \Delta L_{\text{null}}(T) \rvert \le 10^{-9} \cdot \max(1, \lvert \Delta L_{\text{null}}(T) \rvert)$, which implies zero sign inversions. The 10⁻⁹ is a floating-point tolerance, not a model parameter. The rank check is dropped because it is identically 1. |

The kill criteria keep their meaning for G1A.2 and G1A.3: on failure, redesign credit before attempting growth. A G1A.4 failure now means an implementation bug.

## Steps

### Protocol for every step

- **Development banks:** seeds 20–2019, scored as 100 banks of 20, the same banks the review used. Dev seeds 0–19 remain the routine fast check.
- **Holdout:** seeds 10000–10019 are not run until step 5.
- **Horizons:** gates stay at the frozen T = 1024. The investigation also runs T = 2048 and T = 4096, to confirm that results do not depend on the horizon. That is the property the old design lacked.
- **No tuning:** if a criterion fails, record it and apply that step's stop rule. Do not adjust a constant to make it pass.
- **Commits:** one per step, prefixed `M1a-redesign step N:`, following the `M1a-strengthen` series.

Step 2 must pass before step 3 starts. Step 4's criteria are measured on top of step 3. Step 1 is independent of the others.

### Step 0 — Make bank scoring permanent

**Do.**

- Add a horizon option to the panel runner. T = 1024 is currently hard-coded in several places in `src/verify_m1/`.
- Add a bank mode to `verify-m1a`. It scores N consecutive 20-seed banks and reports, per gate, how many banks pass. Per circuit class, it also reports the min and max of flux F, code length L(C), and per-node credit minus debt.
- Refuse any seed range that overlaps the holdout range 10000–10019 unless an explicit holdout flag is given.

**Why.** The review built this harness in `/tmp` and deleted it, yet every step below needs it, and the project rules require repeated work to be automated. The guard makes the pre-registration mechanical instead of a matter of care.

**Pass.** It reproduces the review's "current code" row (G1A.2 0/100, G1A.4 8/100) before any economy change.

### Step 1 — D1: cap recurrent credit

**Do.**

- Implement the cap.
- Unit-test that the `addc1` loop's entries stay at most 1 and that the shift-chain shares are unchanged.
- Update `m1a_gates_pass_fast_seed_slice` to assert the measured outcome.

**Pass.** It reproduces the review's "Fix #1 only" row (G1A.2 100/100, G1A.4 8/100), and G1A.1 and G1A.3 are unchanged.

**Stop rule.** If the numbers differ from the review's, the implementation differs from what was reviewed. Resolve that before continuing.

### Step 2 — D2: new reward, checked through MDL alone

**Do.**

- Implement the shared reward type and make it the default `RewardMode`.
- Unit-test the following:
  - A constant predictor gives r = 0.0 exactly on every tick, for random streams at k ∈ {1, 2, 4, 8}.
  - At k = 1, q̂ equals the KT estimator.
  - A predictor never better than the null model gives r = 0.0.
  - The economy reward and `null_calibrated_mdl_delta` agree on random sequences.

**Measure.** ΔL_null under the new code, on all development banks at T ∈ {1024, 2048, 4096}. This quantity depends on neither credit splitting nor maintenance. The step therefore asks one question: does MDL itself, with the new code, separate the classes the way G1A.2 requires? The gates are not judged in this step, because the old rent is still in place.

**Pass**, at every horizon and on every run:

- ΔL_null > 0 for every `C_wrong`, `C_chance` and `C_silence` run.
- ΔL_null < 0 for every `C_opt` run.

**Report** (not pass/fail): the maximum F of non-predictive circuits at each horizon. The sanity check predicts exactly 0 for constant circuits and a few bits for the others, not growing with T.

**Stop rule.** If either condition fails, MDL itself disagrees with G1A.2's classification. In that case no economy that tracks MDL can pass both G1A.2 and G1A.4. Stop and take the review's option 2 (revisit the gates through charter §7 and §8). Do not start D3.

**Why before D3.** D3 is safe only if chance circuits cannot out-earn their code length. This step checks exactly that, cheaply, and before the economy changes.

### Step 3 — D3: description debt

**Do.**

- Implement the debt allocation and charge it through the maintenance channel.
- Remove `c_base`, `c_split` and `c_wire`; git history keeps them.
- Unit-test the following:
  - Σ ΔL_i equals `circuit_code_bits` for well-mixed and lattice circuits.
  - Allocation fails loudly on bits with no node owner, and no panel circuit triggers it.
  - A `SPLIT` owes more than a single-input node with the same wiring.
  - ΔE = −ΔL_null on the panel circuits.

**Pass.**

- The amended G1A.4 identity holds on every run at all three horizons.
- All four amended gates pass in at least 98 of 100 banks at T = 1024. The `onefire` detour is excluded until step 4. 98 is what the review's best swept constant reached; here no constant is swept.
- The pass count does not fall at T = 2048 or T = 4096.

**Stop rule.** If the per-node check fails, that is a credit-splitting finding, which is the core H1 question. Record which nodes fail and why. Do not change the debt allocation rule after seeing results.

### Step 4 — D4: activity decay and the `onefire` detour

**Do.**

- Implement the recency weights.
- Add the `onefire` detour to the panel and to G1A.3.
- Unit-test the recency update, and that a sparse corrector's weight floor is γ^(gap − 1).

**Pass.**

- Every `onefire` detour node ends with E_i(T) < E_init, on every run and at all three horizons.
- Detour credit after tick 0 is the same at T = 4096 as at T = 1024, to within 10⁻⁹ bits. This shows the leak is bounded.
- Step 3's results on the existing panel do not regress.

**Stop rule.** If a detour node repays its debt from the leak, stop and discuss. Do not tune γ.

### Step 5 — Lock and run the holdout

**Do.**

1. Update [milestone_1_plan.md](./milestone_1_plan.md):
   - §3.1–§3.4: maintenance, reward, credit gating and traces.
   - §3.6.1: the economy and maintenance rows, and the `onefire` detour.
   - §6.1: the amended gates above.
   - WP-M1.1 to WP-M1.3, and §8 risks.
2. Update [foundational_research_charter_v3.md](./foundational_research_charter_v3.md), adding Appendix A decision-record rows for each change:
   - §3.1.2: metabolism as description debt.
   - §3.1.3: replacing traces and activity recency.
   - §3.1.4: the calibrated reward.
   - §3.4: "approximates" becomes "equals".
3. Update `m1a_gates_pass_fast_seed_slice` to assert that all gates pass.
4. Commit the lock and record the source-tree digest.
5. Run the holdout once, `verify-m1a --seed-offset 10000 --seeds 20`, and record the result, whatever it shows.

## Constants after the redesign

| Constant | Before | After |
| :--- | :--- | :--- |
| `c_base`, `c_split`, `c_wire` | 0.020 / 0.035 / 0.004 bits per tick; decide G1A.2 and G1A.4 | Removed |
| `trace_discount` γ | 0.90; discounts `Delay` traces | 0.90, unchanged; also drives activity decay |
| `initial_energy` | 4.0 | Unchanged. It cancels out of ΔE, and the per-node check reduces to credit_i > ΔL_i. |
| `juvenile_ticks`, `max_energy_reserve` | Unused in M1a | Unused in M1a; open for M1b |
| G1A.4 threshold ρ ≥ 0.90 | Gate threshold | Replaced by the identity (floating-point tolerance only) |
| KT prior ½ | Null model | Also q̂'s prior; inherited from the null model, not chosen |

After the redesign, M1a's outcome depends on no tunable constant except γ, which is reused unchanged.

## Open questions for M1b

None of these blocks M1a, which uses fixed structure. All must be settled in the M1b design before any G1B run.

1. **Idle nodes.** Under debt, a node that has repaid its share and then stops receiving credit keeps its energy forever. Rent used to remove such nodes. Two candidates:
   - An exact structural test: prune a node if removing it does not increase L. This fits charter §3.2's sleep compression and M3.
   - Switching codes (Herbster & Warmuth's fixed share; van Erven, Grünwald & de Rooij's switch distribution). These re-charge the description cost of structure that stops paying, using switching priors fixed in advance rather than tuned.
2. **Juvenile period.** With the debt charged at maturity, a sprout must earn ΔL_i − E_init during its grace period. E_init and `juvenile_ticks` need a principled basis.
3. **Edits change everyone's debt.** Adding a node changes the header bits and each wire's bits for every node. We need a rule that keeps Σ debt = L(C), for example that the edit pays the marginal change in L(C).
4. **Match counters.** M in q̂ describes the current circuit. Decide whether an edit to a port's upstream nodes resets it.
5. **Reserve cap.** The review's advice to scale the cap with maintenance no longer applies. Decide whether a cap is still needed.
6. **Other null models.** If M1b uses `RepeatLast`, derive its matched estimator so that constant predictors still score exactly zero.
7. **Unowned bits.** Growth can produce direct `BoundaryIn` → `BoundaryOut` wires. Decide who owes their bits, for example a circuit-level reserve, so that Σ debt = L(C) still holds.

## References

- Dawid, A. P. (1984). Statistical theory: the prequential approach. *Journal of the Royal Statistical Society A*, 147(2), 278–292.
- Grünwald, P. D. (2007). *The Minimum Description Length Principle*. MIT Press.
- Herbster, M., & Warmuth, M. K. (1998). Tracking the best expert. *Machine Learning*, 32(2), 151–178.
- Krichevsky, R. E., & Trofimov, V. K. (1981). The performance of universal encoding. *IEEE Transactions on Information Theory*, 27(2), 199–207.
- Rissanen, J. (1984). Universal coding, information, prediction, and estimation. *IEEE Transactions on Information Theory*, 30(4), 629–636.
- Shafer, G., & Vovk, V. (2019). *Game-Theoretic Foundations for Probability and Finance*. Wiley.
- Singh, S. P., & Sutton, R. S. (1996). Reinforcement learning with replacing eligibility traces. *Machine Learning*, 22, 123–158.
- van Erven, T., Grünwald, P., & de Rooij, S. (2012). Catching up faster by switching sooner. *Journal of the Royal Statistical Society B*, 74(3), 361–417.
- Ville, J. (1939). *Étude critique de la notion de collectif*. Gauthier-Villars.
