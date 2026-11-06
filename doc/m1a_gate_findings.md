# Stage M1a Acceptance Gate Findings & Charter §7 Kill Criterion Diagnosis

**Execution Date:** 2026-11-06  
**Provenance Source Tree Digest:** `57b6dd875dbc57ec8a0a2751195688c7`  
**Evaluation Scope:** 20 development seeds (`--seed-offset 0 --seeds 20`), untouched holdout preserved.  
**Full Report:** `target/verify-m1a-dev.json`  

---

## 1. Executive Summary

In accordance with [milestone_1_plan.md §6.1](milestone_1_plan.md) and §0.1 Ground Rule 5 of the M1a Gate Strengthening Plan, this document records the closed-loop evaluation of Milestone 1a gates `G1A.1`–`G1A.4` on the real 6-class panel across widths $k \in \{1, 2\}$ and noise rates $\varepsilon \in \{0.0, 0.05\}$.

| Gate | Description | Cases | Result | Kill Criterion Triggered |
| :--- | :--- | :---: | :---: | :---: |
| **`G1A.1`** | Conservation & substrate purity | 1,770 | **Passed** (0 failures) | No |
| **`G1A.2`** | Correct vs. incorrect separation | 1,060 | **Failed** (30 failures) | **YES** (Charter §7 Kill Criterion) |
| **`G1A.3`** | Detour & free-rider penalty | 1,201 | **Passed** (0 failures) | No |
| **`G1A.4`** | $\Delta E$ tracks $-\Delta L_{\text{null}}$ | 1,467 | **Failed** (1 failure) | **YES** (Revisit Criterion $\rho < 0.90$) |

Under the pre-registered kill protocol, execution halts before Stage M1b growth code is written.

---

## 2. Gate Results Detail

### 2.1 G1A.1 — Conservation & Substrate Purity (Passed)

- **Substrate Float Purity:** Zero occurrences of `f32` or `f64` in `src/substrate/**/*.rs` (verified by build-time provenance scanner).
- **Macro-Micro Energy Conservation:** Verified across all mature ticks and seeds. Maximum observed conservation error $| \sum \Delta E_i - (\text{flux} - \text{maintenance}) | = 0.000000000000 < 10^{-12}$.
- **$k=8$ Sensitivity Sampling:** Sampled counterfactual sensitivity ($M = 32$) on `Split -> Eqz` and `Add(x, ConstLoop(1))` matched exact 255-value sensitivity within $\pm 0.05$ across all nodes and 64 ticks.

### 2.2 G1A.2 — Correct vs. Incorrect Separation (Kill Criterion Triggered)

- **Noiseless ($\varepsilon = 0.0$):** 100% of panel construction self-checks passed exactly. All $C_{\text{opt}}$ circuits gained energy ($\Delta E > 0$) with all nodes ending above initial reserve ($E_i > 4.0$). All $C_{\text{wrong}}$, $C_{\text{chance}}$, and $C_{\text{silence}}$ lost energy ($\Delta E < 0$).
- **Noisy Channel ($\varepsilon = 0.05$):** 30 failures observed where $C_{\text{opt}}$ total circuit energy was strongly positive ($\Delta E > 149$ bits), but individual interior feedforward combinational nodes dropped below initial reserve ($E_i \le E_{\text{init}} = 4.0$):
  - `affine` ($k = 1$, `addc1`): Node 2 (`Primitive::Add`) dropped below $4.0$ on **all 20 seeds** (final energy $E_2 \in [-0.90, 1.89]$ bits).
  - `counter3` ($k = 2$, `counter3`): Node 5 (`A2 = Primitive::Add`) dropped below $4.0$ on **10 of 20 seeds** (final energy $E_5 \in [2.13, 3.49]$ bits).

### 2.3 G1A.3 — Detour & Free-Rider Penalty (Passed)

- Detour nodes in `addzero`, `mulzero`, and `cancelsub` received exactly **0.0 cumulative credit** after tick 0 across all tasks and seeds (max observed detour credit = `0.0`).
- Every detour circuit strictly underperformed its reference circuit in net energy ($\Delta E(\text{detour}) < \Delta E(\text{reference})$).
- Spatial `Add(Osc2, ConstLoop(0))` on a $3 \times 3$ `LocalVonNeumann` lattice lower achieved exact 0.0 detour credit and lower net energy than spatial `Osc2`.

### 2.4 G1A.4 — $\Delta E$ Tracks $-\Delta L_{\text{null}}$ (Revisit Criterion Triggered)

- **Sign Inversion Check:** **Zero** instances observed where $\Delta E > 0$ while $\Delta L_{\text{null}} > 0$ across all 1,467 evaluations.
- **Rank Correlation Check:** 17 of 18 non-constant groups achieved $\rho_s \ge 0.93$ (with 10 groups achieving $\ge 0.98$ and 6 achieving $1.0000$).
- **Failure:** `counter3` at $k = 2, \varepsilon = 0.05$ achieved Spearman rank correlation $\rho_s = 0.8929 < 0.90$.

---

## 3. Mathematical & Economic Diagnosis

### 3.1 Diagnosis of G1A.2: Feedforward Combinational Credit Dilution

In `addc1` (`Add(x, ConstLoop(1))`), the constant source is a synchronous recurrence:

$$\text{Delay}(1) \longleftrightarrow \text{Split}$$

Because this loop recirculates state every tick, its causal eligibility trace $\mathbf{e}_r(t)$ accumulates the temporal discount series:

$$\sum_{t=0}^{\infty} \gamma^t = \frac{1}{1 - \gamma} = \frac{1}{1 - 0.90} = 10.0.$$

Consequently, the ancestry shares arriving at the output are:

- `Delay(1)`: $c_0 \approx 7.5$
- `Split`: $c_1 \approx 7.5$
- `Add`: $c_2 = 1.0$ (feedforward combinational, no register trace)

The total ancestry normaliser is $Z = 7.5 + 7.5 + 1.0 = 16.0$. Thus, `Add` receives only $\frac{1}{16} \approx 6.25\%$ of the prediction reward, while the constant generator absorbs $\approx 93.75\%$.

At $k = 1$, the centered surprisal for predicting 1 bit is $0.5$ bits/tick. Under $\varepsilon = 0.05$ substitution noise, reactive predictors suffer double-error penalties, yielding $\approx 10\%$ prediction error ticks. Over 1,024 ticks:

- Total net circuit reward: $\approx 414$ bits.
- Reward allocated to `Add`: $414 \times 0.0625 \approx 28.9$ bits.
- Maintenance cost of `Add`: $1024 \times (c_{\text{base}} + 3 c_{\text{wire}}) = 1024 \times 0.032 = 32.768$ bits.
- Net energy change for `Add`: $28.9 - 32.8 = -3.9$ bits.
- Final energy: $E_{\text{init}} - 3.9 = 4.0 - 3.9 = 0.1$ bits.

Because $28.9 < 32.8$, the feedforward `Add` node slowly starves despite the whole circuit earning a massive $+316$ bit surplus and the `Delay`/`Split` nodes ending with $> 150$ bits.

A parallel effect occurs in `counter3` (`Counter3Opt`): node `A2` feeds `Eqz(x + 2)`, which outputs `0` on 2 out of 3 ticks. By additive identity gating, `A2` receives $0$ credit on 66.7% of ticks while paying maintenance on 100% of ticks. Under 5% channel noise, `A2` drops to $2.13$–$3.49$ bits.

### 3.2 Diagnosis of G1A.4: Per-Tick Maintenance vs. One-Time MDL Divergence

On `counter3` at $k = 2, \varepsilon = 0.05$, the 7 panel circuits rank as follows:

| Circuit | Class | Nodes | Accuracy | $-\Delta L_{\text{null}}$ (MDL) | $\Delta E$ (Economy) |
| :--- | :--- | :---: | :---: | :---: | :---: |
| `counter3` | `C_opt` | 8 | 95.0% | **839.9** (Rank 1) | **704.6** (Rank 1) |
| `addzero` | `C_detour` | 11 | 95.0% | **787.2** (Rank 2) | **611.4** (Rank 2) |
| `cancelsub` | `C_detour` | 14 | 95.0% | **731.6** (Rank 3) | **518.2** (Rank 4) |
| `mulzero` | `C_detour` | 15 | 95.0% | **714.4** (Rank 4) | **493.6** (Rank 5) |
| `inc1` | `C_partial` | 3 | 63.4% | **596.3** (Rank 5) | **543.0** (Rank 3) |
| `const1` | `C_chance` | 2 | 23.8% | **-27.3** (Rank 6) | **-69.0** (Rank 6) |
| `const0` | `C_chance` | 2 | 23.8% | **-29.3** (Rank 7) | **-73.0** (Rank 7) |

Notice that `inc1` is displaced by 2 rank positions:

- In $-\Delta L_{\text{null}}$, circuit description length $L(\text{circuit})$ is paid **once** (14 nodes costs 108 bits vs. 44 bits for 3 nodes, a difference of 64 bits). High prediction accuracy saves $\approx 200$ bits of residual code length, so `cancelsub` ($731.6$) easily beats `inc1` ($596.3$).
- In $\Delta E$, hardware maintenance is paid **every tick for 1,024 ticks** ($14 \times 0.024 \times 1024 = 344$ bits for `cancelsub` vs. $97$ bits for `inc1`, a difference of $247$ bits). The $247$-bit maintenance penalty exceeds the $200$-bit prediction reward advantage, so the lean 3-node circuit finishes with higher net energy ($543.0$) than the bloated 14-node detour circuit ($518.2$).

Because of this 2-rank swap, $\sum d_i^2 = 2^2 + 1^2 + 1^2 = 6$, yielding:

$$\rho_s = 1 - \frac{6 \times 6}{7(49 - 1)} = 1 - \frac{36}{336} = \frac{25}{28} \approx 0.8929 < 0.90.$$

---

## 4. Required Redesigns Before Stage M1b

In accordance with [milestone_1_plan.md §6.1](milestone_1_plan.md) kill criteria:

1. **G1A.2 Redesign (Feedforward Credit Floor):**
   - Either restrict the sustain check in G1A.2 to interior `Delay` nodes (matching the requirement column: *"sustain all interior Delay nodes"*, which passed 100% with $E_i > 150 \gg 4.0$), or
   - Adjust the credit allocation so feedforward nodes outside recurrent loops do not suffer $1/(1-\gamma)$ dilution against constant loops.
2. **G1A.4 Redesign (Maintenance Horizon Scaling):**
   - Recalibrate $c_{\text{base}}$ and $c_{\text{wire}}$ relative to observation horizon $T$, or scale description-length circuit cost $L(\text{circuit})$ proportionally to time horizon $T$ so that rank order remains consistent between physical energy drain and static MDL across both small and large circuit structures.
