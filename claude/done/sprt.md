# SPRT: the sequential test that gates a network

**Done:** implemented in `src/arena/sprt.rs` with the constants specified here; still the one specification of the test. The gate that uses it: round-robin-selection.md. The "not yet implemented" status below is stale.

## Status

Specification, not yet implemented. This is the **one** document that specifies the test. It was
extracted from `evolutionary-training.md`, which designed it and now points here; `rotor-network-gate.md`
describes the match that feeds it and points here for the test itself. Nothing about the test is to be
restated, re-derived or reformulated anywhere else -- an earlier draft of the gate document
reinvented it as a normal-approximation test with a draw-rate estimate, which is precisely the
substitution the "two derivations that look substitutable and are not" section below warns against.

## What the test decides

A challenger plays the champion at fixed depth 9 until the test resolves. It answers one question --
**is the challenger at least `ELO_D` Elo stronger?** -- with bounded error rates, and it stops as soon
as the evidence is sufficient rather than after a fixed number of games.

It does not estimate how much stronger. See "Deliberately not in this" at the end.

## The three chosen constants

**Three numbers are chosen. Everything else is derived from them**, and must be derived in the
implementation too rather than pasted in as a decimal:

```
ELO_D = 35.0            // the gain the test is powered to detect
ALPHA = 0.05            // false promotion
BETA  = 0.05            // missed promotion
```

`elo0 = 0` is forced: the question is whether the challenger beats the champion at all. `elo1 = 35`,
decided with the developer, the same value as `ELO_D`, so the gate introduces no constant of its own.
The width is deliberate -- games scale as `1/ELO_D^2`, and the interesting outcomes are large in either
direction, so sensitivity below 35 Elo buys nothing that would be acted on. The tradeoff taken
knowingly: a genuine improvement smaller than 35 Elo is reported as "not better".

### `ELO_D = 10` was considered and rejected on cost

Raising the sensitivity does not shorten the test, it lengthens it. The bounds `upper`/`lower` depend
only on `ALPHA` and `BETA` and do not move; the drift `B + s * slope` shrinks as `ELO_D^2`, because
`slope` is proportional to `ELO_D` and so is the gap between `p1` and `rate`. Expected games therefore
scale as `1/ELO_D^2`, and `(35/10)^2 = 12.25`.

| quantity | `ELO_D = 35` | `ELO_D = 10` |
|---|---|---|
| `p1` | 0.5501993 | 0.5143870 |
| `A` | +0.095672 | +0.028368 |
| `B` | -0.105804 | -0.029196 |
| `slope` | 0.201476 | 0.057565 |
| `rate` | 0.5251420 | 0.5071944 |
| drift at `H1` | 0.005048 | 0.000414 |
| typical games | ~525 | ~6,400 |
| network gate, 16 in flight | ~1.4 h | ~17 h |

Seventeen hours for the typical case, two days for the drift-free exit, against a stated limit of well
under seven. Rejected for generation 0, where a first network either carries substantial signal or it
does not and a 10-Elo distinction would not change what happens next.

**Worth revisiting later, and not for this reason.** In a generation where gains are incremental and a
stall is permanent, 10 Elo is exactly the size of improvement worth detecting. The thing that makes it
affordable is not a different test but a faster network: int8 quantization cuts the per-game cost about
4x and is already wanted for generation-1 self-play. `ELO_D = 20` is the cheap middle if one is ever
needed -- 3.1x the games, about 1,600 typical.

## The derived chain, in order

```
p0    = 0.5
p1    = 1 / (1 + 10^(-ELO_D / 400))          // the sigmoid Elo formula
A     = ln(p1 / p0)                           // LLR(win)
B     = ln((1 - p1) / (1 - p0))               // LLR(loss)
D     = (A + B) / 2                           // LLR(draw)
slope = A - B
rate  = -B / slope
upper = ln((1 - BETA) / ALPHA)
lower = ln(BETA / (1 - ALPHA))
```

At `ELO_D = 35` and `ALPHA = BETA = 0.05` this evaluates to:

| quantity | value |
|---|---|
| `p1` | 0.5501993 |
| `A` | +0.095672 |
| `B` | -0.105804 |
| `D` | -0.005066 |
| `slope` | 0.201476 |
| `rate` | 0.5251420 |
| `upper` | +2.944 |
| `lower` | -2.944 |

**Those numbers are here to check an implementation against, not to be typed into one.**

`lower` is computed from `ALPHA` and `BETA` rather than negated from `upper`, so that raising one
error rate alone still gives the right test.

Rust cannot evaluate `ln` or `powf` in a `const`, so the derived values are computed once when the
test is constructed, by a function taking `(elo_d, alpha, beta)`. The production constants are then
the only hard-coded call site, and the derivation itself becomes testable at other parameter values.

### Two derivations that look substitutable and are not

- **`slope` is `A - B`, which equals `ln(10) * ELO_D / 400` exactly.** Either is fine, but deriving it
  from `A` and `B` keeps `p1` the single source of truth.
- **`rate` is `-B / (A - B) = 0.5251420`, not the midpoint `(p0 + p1) / 2 = 0.5250997`.** They differ
  by 0.0000423 and are the thresholds of two different tests: the midpoint belongs to the
  normal-approximation SPRT, and substituting it would quietly replace this test with that one.

  `rate` was recorded here, and in `evolutionary-training.md` before it, as 0.5251185, which is simply
  wrong: `-B / slope = 0.5 + 0.0050656 / 0.2014762 = 0.5251420`. Caught by the implementation's own
  test against this table, which is what the table is for. The stated difference from the midpoint was
  wrong in consequence -- 0.00004 rather than 0.00002 -- so the distinction this bullet draws is real
  and slightly larger than it claimed.

The asymmetry between a win and a loss is load-bearing rather than incidental: a loss must outweigh a
win, or an even result would not be evidence against `H1` and the test would never terminate at `H0`.

## The statistic is one counter

**Defining the draw as the mean of the two makes the LLR linear in the score**, since
`LLR(x) = x * A + (1 - x) * B` reproduces all three values at `x = 1, 0.5, 0`. Over `N` games with
total score `S`:

```
LLR = slope * (S - rate * N)
```

so the whole test is one running counter. **Promote when the score exceeds `rate` of the games played
by more than `upper / slope` points; fail when it falls short by `|lower| / slope`.** At the chosen
constants that is 52.514% and 14.61 points: 56.2% at 400 games, 54.9% at 600, 54.0% at 1000, tending
to 52.51% as the test lengthens.

Track the score in **half-points as an integer** (`win = 2`, `draw = 1`, `loss = 0`) and the
accumulation is exact, with no floating-point drift over thousands of games. The stopping test is then

```
promote when  S_half >= 2 * rate * N + 2 * upper / slope
fail when     S_half <= 2 * rate * N + 2 * lower / slope
```

two float comparisons per game against an integer counter.

## Why the error rates hold

These three increments are not the log-likelihood ratios of any pair of three-outcome distributions,
so Wald's bounds do not apply for the usual reason. They apply anyway. Writing `d` for the draw rate,
under `H0`:

```
E_0[exp(LLR)] = (1 - d)/2 * (exp(A) + exp(B)) + d * exp(D) = 1 - d * (1 - exp(D))
```

because `exp(A) + exp(B) = 2` exactly when `p0 = 0.5`. Since `A + B < 0` gives `exp(D) < 1`, this is
`<= 1` for **any** draw rate and any `ELO_D`, so `exp(LLR)` is a non-negative supermartingale under
`H0` and Ville's inequality gives `alpha <= exp(-upper)`. The other side has the same shape,
`E_1[exp(-LLR)] = 1 - d * ((exp(-A) + exp(-B))/2 - exp(-D)) <= 1`, giving `beta <= exp(lower)`. At the
chosen constants both bounds are `1/19 = 5.26%`, and the two coefficients of `d` are `0.00505` and
`0.00510`.

Both error rates are therefore honest, slightly conservative, and hold **with no draw-rate estimate
anywhere in the test**. The conservatism grows with the draw rate rather than the guarantee degrading.
That is the merit of defining the draw this way rather than modelling it.

This argument is why the test must be followed exactly as written. Every element of it -- the
midpoint draw, `p0 = 0.5`, `rate = -B/slope` -- is load-bearing for the bound.

## Games arrive in opening pairs

**Decided with the developer, superseding the "Games are drawn independently, not in opening pairs"
section this document's own earlier version carried.** Each worker plays one opening, then the same
opening with the colours reversed, and **applies two ordinary per-game updates when the pair
completes**.

**There is no pair-level observation and nothing to re-derive.** A completed pair contributes its two
games' half-points as two normal increments, and the stopping test is evaluated afterwards. Pairing is
a scheduling and colour-balance decision, not a change to the statistic. The implementation follows
the formulas above unchanged.

Recorded for whoever compares this against the superseded wording: the argument for independent games
was that the supermartingale conditions game by game, so the unconditional error bound is proved for
independent games and not for correlated pairs. Pairing gives exact colour balance and exact
cancellation of opening bias instead -- a stopped run can never have given the challenger one more
White than Black. The error rates are then conservative under realistic pair distributions rather than
unconditionally guaranteed, and that trade was made deliberately.

## Cost

Drift is `B + s * slope`, which depends only on the mean score, so **the expected length does not
depend on the draw rate at all: about 525 games at either hypothesis.** Drift vanishes at
`s = rate`, i.e. `elo = 17.47`, where the walk exits by variance alone at roughly 1420 games (about
2100 at a 60% draw rate), with an unbounded tail.

**No cap.** The wall-clock cost depends on who is playing, and only the network gate is expensive:

| games | Rotor vs Rotor, hand-crafted | network challenger, 16 games in flight |
|---|---|---|
| 525 (typical, either hypothesis) | ~7 min | ~1.4 h |
| 1420 (drift-free, variance exit) | ~19 min | ~3.8 h |
| 2100 (same at 60% draws) | ~29 min | ~5.6 h |

The network column comes from `benches/nnue_bench.rs`: the search is 29x slower with the network, or
9.6 s per game with 16 games running at once. A factor of about 12 over the hand-crafted column.
**"No cap" is defensible at these numbers** -- the typical case is under 90 minutes and even the
drift-free exit is an afternoon. A cap would only earn its keep if the tail actually bit in practice.

Generation 1 roughly doubles the network column, since both sides then pay the 29x.

## What it buys over the 400-game gate

The gate this replaces was AlphaGo Zero's: 400 games, promote at 55%.

| | 400 @ 55% | This SPRT |
|---|---|---|
| Promotes a true +35 | ~50% | ~95% |
| Promotes a true +20 | ~13% | ~60% |
| Promotes a true 0 | ~0.5% | ~5% |
| Games | 400 | ~525 typical, ~1420 worst |

False promotions rise tenfold, from 0.5% to 5%, and that is the right trade here: a false promotion at
`elo = 0` installs a generator *equal* to its predecessor rather than worse, so the compounding worry
is milder than it sounds, while the power at +20 is what decides whether a generation promotes or
stalls when one dataset holds a real but modest improvement. Under `evolutionary-training.md`'s
refinement 2 a stall is permanent, so power matters far more than it did when a failed gate brought
fresh games.

## Implementation notes

- **`ELO_D`, `ALPHA`, `BETA` are the only constants.** `p1`, `A`, `B`, `D`, `slope`, `rate`, `upper`
  and `lower` are derived at construction by one function taking all three.
- **The counter is `i64` half-points and a `u64` game count.** Nothing else is state.
- **The LLR is recomputed from the results database, never persisted.** It is a pure function of the
  recorded games, so there is no second piece of state that could disagree with them. This is what
  makes a resumed run correct by construction.
- **A pair must be recorded atomically.** A crash between a pair's two games would otherwise leave a
  colour-unbalanced fragment that the recomputation would have to special-case. Writing both games in
  one transaction, or discarding an odd trailing game on resume, keeps the invariant that the database
  only ever holds whole pairs.
- **The verdict is three-valued**: continue, promote, fail. A run that is interrupted before crossing
  either bound is none of the three and resumes.

## Deliberately not in this

- **No Elo point estimate.** The test answers which hypothesis the evidence supports, not by how much.
  A confidence interval is a separate calculation, and reporting an Elo number from a
  sequentially-stopped test is a standard way to overstate an effect -- stopping early biases the
  observed margin upward. If a number is wanted afterwards, it needs its own fixed-length run.
- **No draw-rate estimate.** Not an omission: the error bounds above hold without one, and
  introducing one would replace this test with a different one.
- **No cap on games.** See "Cost".
- **Not the tournament decision rule.** `rotor-evolve.md` records SPRT as *rejected* for its own
  tournament pairings, on the grounds that it needs `elo0`/`elo1`/`alpha`/`beta` tuning that does not
  transfer from fishtest's defaults. That decision stands for engine-versus-engine tournaments. The
  gate is a different question with its own designed test, which is why `decision` is a per-match
  setting rather than a change to the tournament format.
