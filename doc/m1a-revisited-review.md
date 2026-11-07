# Review: M1a redesign (`m1a-revisited-proposal.md`)

I'd adopt Fix #1 and drop Fix #2. Fix #3 is the right thing to change, but the halving doesn't hold up. As written, the redesign passes dev seeds 0–19 but would pass the holdout only about 24% of the time, so M1b would most likely never start.

There is a configuration that passes about 98% of the time, but I found it by sweeping the maintenance scale. That is the "magic number" search the new rule in `.github/copilot-instructions.md` forbids; the rule appeared while my runs were in progress. So I'm reporting the sweep as evidence, not as a recommendation, and Fix #3 needs your decision.

**How I checked:** I made a throwaway copy of the repo in /tmp with a switch for each fix (now deleted). I then scored 100 separate 20-seed banks from seeds 20–2019 with the real `verify-m1a` binary, the same way the holdout will be scored. Seeds 10000–10019 were never run, and I changed nothing in your working tree.

## What drives every result

`null_calibrated_mdl_delta` uses the same prediction score as the `economy reward`. In the M1a panel, a circuit's energy gain is therefore its prediction score minus total maintenance, and its MDL score is the same prediction score minus its code length. That splits the gates cleanly:

- **G1A.4, and G1A.2's wrong/chance/silence check**, depend only on maintenance versus code length, so Fixes #1 and #2 cannot move them.
- **G1A.2's per-node check and G1A.3** depend only on how credit is split between nodes, so Fix #3 cannot fix those robustly.
- **G1A.1** holds whatever the credit split is, so it tells you nothing about the fixes.

## Banks passing, out of 100

| Configuration | G1A.2 | G1A.4 | All four gates |
| --- | --- | --- | --- |
| Current code | 0 | 8 | **0** |
| Proposal as written (#1, #2, halved maintenance) | 24 | 100 | **24** |
| Fix #1 only | 100 | 8 | 8 |
| Fix #1, maintenance ×0.8 (sweep result) | 98 | 100 | 98 |
| Fix #1 and #2, maintenance ×0.8 (sweep result) | 98 | 100 | 98 |
| No fixes, maintenance ×0.8 (sweep result) | 97 | 100 | 97 |

G1A.1 and G1A.3 passed in all 100 banks for every configuration. Your dev seeds 0–19 happen to be one of the lucky banks for the proposal.

## Fix #1 (cap recurrent credit): adopt

- **It's sound.** The diagnosis is right. The cap stops a feedback loop from crediting itself again every tick, which is the standard remedy for this kind of over-crediting.
- **It's enough for the per-node check.** On its own, Fix #1 passes that check in all 100 banks. It also rescues `counter3` (A2 ends 15.5 bits above its starting energy at current maintenance), which removes the case for Fix #2.
- **It matters for M1b.** At ×0.8 maintenance without it, margins are thin: `addc1` +1.65 bits and `counter3` +3.87 bits over 1024 ticks. Loops would keep out-earning the `Add` correctors that M1b grows.
- **Implementation notes:**
  - The change belongs in `ancestry_for_target`, not `economy.rs` as the proposal's heading says.
  - The proposal gives two cap formulas. They are equivalent under the cap, so state just one.
  - The shift-chain test `discounted_register_ancestry_preserves_credit_through_delay_chains` still passes.
  - The only test that breaks is `m1a_gates_pass_fast_seed_slice`, which asserts that G1A.2 and G1A.4 currently fail.

## Fix #2 (keep credit for inputs that were ever active): drop

- **It isn't needed.** Once Fix #1 is in, adding Fix #2 changes nothing (98 vs 98 banks). On its own it leaves `addc1` failing (−4.9 bits on dev seeds).
- **It's harmful.** It pays an input for showing up rather than for what it changed.
  - I tested a detour that fires once at tick 0 and then outputs 0 forever: `Add(base, Delay{init:1}(ConstLoop(0)))`.
  - Under Fix #2 it collects 220–800 bits and ends with positive energy on all 9 task/width pairs, so M1b would never prune it.
  - G1A.3 only passes because the panel has no detour like this.
- **The M1b argument is overstated.** Under the current rule, a sparse corrector is already paid on the ticks it fires.
- **The current code already has a smaller version of this hole.** When both `Add` inputs are 0, the code checks the "ever non-zero" flag in `NodeActivity`. The same detour leaks 28–263 bits and stays positive on 7 of 9 pairs.
- **Suggested fix: windowed activity.** Count an input as active only if it emitted a non-identity value in the last W ticks.
  - The leak falls to 0.4–17.6 bits, and the detour's nodes end 54–71 bits below where they started, so they would be pruned.
  - The standard panel and all tests are unchanged, and W = 16, 64 and 128 behaved the same. Tie W to something principled such as `juvenile_ticks` rather than picking it from a sweep.
  - Apply the same windowing to the `ever_non_one` flag used for `Mul`.
- **If you add this detour to G1A.3,** "zero credit after tick 0" fails it in every bank. The check would need to be "zero credit after tick W, and the detour's nodes end below their starting energy".

## Fix #3 (halve maintenance): right target, unsupported value

- **Right target.** G1A.4 compares total maintenance (T·M) with code length (L) at a fixed horizon of T = 1024 ticks, so maintenance is the only lever for it.
- **The arithmetic is wrong**, though the overall story survives. The same errors are in `m1a_gate_findings.md` §3.2.

  | Quantity | Proposal | Recomputed |
  | --- | --- | --- |
  | `cancelsub` vs `inc1` maintenance gap | 247 bits | 348 bits |
  | `cancelsub` vs `inc1` code-length gap | 64 bits | 188 bits |
  | `cancelsub` vs `inc1` prediction-score gap | ≈200 bits | ≈323 bits |
  | ΔE after halving: `cancelsub` / `mulzero` / `inc1` | 690 / 678 / 591 | ≈741 / 729 / 592 |

- **"Chance circuits stay negative" is true on average, but G1A.2 checks every run.**
  - alt k=2 `const1` always predicts a rare symbol, so its score is like a lottery ticket. Over 2000 seeds its prediction score has mean −8.9 bits, standard deviation 21.7 and maximum +64.9.
  - Its maintenance is 68.6 bits at current rates but only 34.3 bits when halved.
  - That is why halving fails G1A.2 in 76 of 100 banks.
- **The two gates pull in opposite directions.**
  - If maintenance exactly equalled code length, some chance circuit would beat the null in 99 of 100 banks. So G1A.2 needs the economy to be stricter than MDL, while G1A.4 needs it to rank circuits like MDL.
  - Current maintenance is 1.9–2.8× code length, with small circuits charged the most.
  - Making maintenance proportional to code length is worse at every multiplier (at best about 59% of banks).
- **The values that pass depend on the horizon.**
  - At T = 1024, scaling maintenance by ×0.75–0.8 passes 96–98% of banks.
  - At T = 2048 the best scale is about ×0.7, and ×0.8 drops to about 72%. At T = 4096, no scale passes.
  - The economy charges per tick while MDL charges code length once, so their rankings only agree near one horizon. Choosing ×0.8 would be fitting the gate's hard-coded T = 1024, not deriving anything.

## What this means for M1b

- **Halving weakens pruning.** A node that fills its 25-bit reserve can coast with no income for about 2080 ticks instead of about 1040. That is longer than M1b's 2000-tick run, which undermines G1B.2's noise-floor halting.
- **Reserve and starting energy should move with maintenance.** Whatever maintenance you choose, scale `max_energy_reserve`, and optionally `E_init`, by the same factor. This does not affect M1a, because the panel uses no reserve cap and no juvenile period.
- **G1A.2's chance check is M1a's stand-in for M1b's overfitting guard.** Weakening it to satisfy G1A.4 gives up the property M1b needs most.
- **Fix #1 plus windowed activity help M1b**, because correctors are no longer starved by loops and free riders get pruned. Fix #2 would make M1b worse.

## Decision needed on Fix #3

Whichever option you choose, lock it before the single holdout run:

1. **Derive maintenance from a stated principle** (for example an explicit horizon plus a safety margin), without looking at gate outcomes, and accept whatever the holdout shows.
2. **Revisit the gates through the charter's §7/§8 process.** G1A.2's "every chance run" check is stricter than MDL itself, and G1A.4's rank check uses one horizon and only the first seed of the bank (`mod.rs`).
3. **Treat G1A.4 as a triggered kill criterion** and redesign how the economy depends on the horizon.
