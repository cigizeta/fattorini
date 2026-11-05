# Duel: which of two engines is stronger

A sequential z-test, and the third decision rule of `rotor-arena` beside the best-of `match` and the
`sprt`. How it is wired into the commands and the gate is `arena-commands.md`; this document is the
rule itself and the reasoning behind its one constant.

## Status

Done. Implemented as `rotor-arena duel` (`arena-commands.md`). First run, at depth 9:
amanda-0001 against hand-crafted, +50 -21 =15, S 0.669 against a threshold of 0.667 at N = 86,
amanda-0001 stronger.

## Premise

The SPRT (`sprt.md`) tests a hypothesis that depends on an Elo difference: is the first player at
least `elo_d` stronger, or not? The answer depends on `elo_d`, a number that has to be chosen and
justified.

The duel asks only: **which of the two engines is stronger?** No Elo margin appears anywhere.

## The rule

Games are played in colour-reversed pairs, so after every completed pair the number of games N is
even. After each pair, with S an engine's score as a fraction, `(wins + draws / 2) / N`:

```
an engine wins      if   S > 0.5 + c * 0.5 / sqrt(N)
the duel is drawn   if   N reaches 1000 and neither engine has won
c = Phi^-1(1 - alpha) = sqrt(2) * erfinv(1 - 2 * alpha),   alpha = 0.001
  = 3.090232...
```

`Phi^-1` is the inverse of the standard normal cumulative distribution function and `erfinv` the
inverse error function; neither has a closed form, so `c` is computed numerically. Decided: the
code hard-codes `3.09`, documented as `Phi^-1(0.999)`. The extra digits change the threshold by
less than 0.0001 at N = 1000, far below the resolution of any match.

The rule is symmetric: the second engine's score is `1 - S`, so at most one engine can be past its
threshold at a time. The 1000-game cap ends the duel regardless; reaching it means neither engine
was shown to be stronger.

## Where the threshold comes from

`0.5 / sqrt(N)` is the standard deviation of S for two equal engines when no game is drawn: each
game scores 0 or 1 with probability 1/2, standard deviation 1/2, and S averages N of them. The
threshold is therefore "c standard deviations above an even score", a z-test.

`c` is a quantile of the standard normal distribution, `c = Phi^-1(1 - alpha)`, where `Phi` is the
normal cumulative distribution function. It has no closed form and is computed numerically:

| alpha | One-sided `Phi^-1(1 - alpha)` | Two-sided `Phi^-1(1 - alpha / 2)` |
|---|---|---|
| 5% | 1.645 | 1.960 |
| 1% | 2.326 | 2.576 |
| 0.1% | 3.090 | 3.291 |

The one-sided value is what the rule uses per engine.

## What alpha means

With `c = Phi^-1(1 - alpha)`, alpha is the probability that, **if the two engines are exactly
equal and the score is checked once, at an N chosen in advance, one particular engine ends up past
its threshold.** Each part of that sentence matters:

1. It is a false-alarm rate: how often luck alone makes an equal engine look stronger. It says
   nothing about how often a truly stronger engine is detected.
2. It is one-sided. Since either engine can win, even a single check has a `2 * alpha` chance of
   producing some false winner.
3. It assumes a single check. The rule checks after every pair, up to 500 times (see below).
4. It assumes no draws. Draws shrink the true spread of S below `0.5 / sqrt(N)`, so each check is
   in fact somewhat stricter than alpha.

## The problem: 500 checks

The first choice was `c = 1.645`, the one-sided 5% quantile, taken without any correction for the
number of checks. But a 5% chance per check, taken 500 times, is not a 5% chance overall.

The checks are strongly correlated (the score after 202 games is the score after 200 plus one
pair), so the overall rate is far below `1 - 0.95^500`, which would be almost 1. It is still far
above 5%. A standard approximation treats the running score as a continuous random walk observed
from N = 4 to N = 1000; for a boundary `c * 0.5 / sqrt(N)` the chance that either of two equal
engines is ever declared the winner is roughly

```
P(false winner) ~ c * phi(c) * ln(1000 / 4),     phi(c) = exp(-c^2 / 2) / sqrt(2 pi)
```

This approximation is only meaningful for larger c, and it is an estimate, not a bound. Checking
only after whole pairs, and draws, both push the true rate lower.

Without a cap it would be worse: with any constant c and no limit on N, two equal engines are
certain to cross the threshold eventually (the law of the iterated logarithm). A constant c is
therefore only ever safe up to a horizon, here the 1000-game cap.

## Alternatives considered

- **A constant tuned to the cap**, solving `c * phi(c) * ln(250) = alpha_overall`: about 3.14 for
  5% overall, about 3.66 for 1%. Rejected as a formula because it refers to the cap explicitly.
- **A boundary that does not depend on the cap** (Robbins' normal-mixture boundary, valid at every
  N forever): `S > 0.5 + sqrt((N + m) * (2 ln(1/alpha) + ln(1 + N/m))) / (2N)`, with a tuning
  constant m. Rejected as more complex than needed.
- **Checking less often** (for example only at N = 100, 200, ..., 1000), which allows a smaller c.
  Rejected: the duel should be able to stop after any pair.

Decided instead: keep the simple formula and pick the per-check alpha by the overall rate it
produces within the cap.

## Choosing the per-check alpha

Overall rates below are the approximation above, both engines counted; the true rates are
somewhat lower. The Elo column is the edge whose expected score equals the threshold at N = 1000,
roughly the smallest edge that is likely to be detected before the cap.

| Per check (one-sided) | c | Overall false-winner rate (approx.) | Threshold at N = 1000 | Edge needed near the cap |
|---|---|---|---|---|
| 5% | 1.645 | well over 50% | 0.526 | about 18 Elo |
| 1% | 2.326 | about 34% | 0.537 | about 26 Elo |
| 0.5% | 2.576 | about 21% | 0.541 | about 29 Elo |
| **0.1%** | **3.090** | **about 6%, likely under 5% in practice** | **0.549** | **about 34 Elo** |
| 0.05% | 3.291 | about 3% | 0.552 | about 36 Elo |
| 0.01% | 3.719 | about 1% | 0.559 | about 41 Elo |

**Decided: 0.1% per check, `c = 3.090`.** It is a round percentage, and it brings the whole duel
back to about the 5% originally intended for a single check. The price: engines less than about
34 Elo apart will usually end in a draw at the cap.

## The threshold at `c = 3.090`

| N | Threshold |
|---|---|
| 10 | 0.989 |
| 100 | 0.655 |
| 200 | 0.609 |
| 500 | 0.569 |
| 1000 | 0.549 |

No engine can win before N = 10: the threshold exceeds 1 while `sqrt(N) < 3.090`. The earliest win
is a 10-0 after five pairs.

## The name

Chosen over "sequential z-test" (accurate but describes the method, not the question), "stronger"
(awkward as a noun), "showdown" and "verdict" (which clashes with `sprt.rs`'s `Verdict`). A duel is
two engines, one question, and a winner or a draw, and it takes no parameter, unlike `sprt
<elo_d>` or `match <n>`.

## Verification

The overall false-winner rate is an approximation. A seeded unit test should simulate many duels
between two equal engines, with a realistic draw rate, and assert the rate stays at or below 5%.
It pins the choice with a measurement rather than a formula.
