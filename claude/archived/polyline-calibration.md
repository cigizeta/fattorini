# Is Lc0's late lambda of 0 right? A measurement first

**Status: closed 2026-11-06 without running the measurement (the developer):** the late lambdas of 0
are sound. The doubt came from z growing more reliable toward a game's end, but s does too (simpler
positions, the tables), so late in a game s already carries what z would add, with less noise.
`smithy polyline-diagnose` stays in the code until removed by choice.

**Earlier status:** the measurement built 2026-11-05 as `smithy polyline-diagnose <buckets> [one-in]`, not yet
run; it prints the 20 bins and leaves out the logit-space slope, the bins answering that. An open question on lambda-polyline.md, for after the
running `lc0-psq-threats-screlu` run. Only the measurement is proposed here; any change to the
labels is decided from its numbers.

## Why

`polyline.txt` for Lc0, on 1,138,144,749 positions: 0.0123, 0.0299, 0.0326, 0.0312, 0.0209, then 0
in buckets 5 to 9. The late values are clamps of a negative lambda*, so half of every game gives
the result no weight. AB(lambda), retired 2026-10-22, gave the result most weight late, whatever the
teacher. PeSTO's polyline rises the same way (0.11 to 0.97); Lc0's is the reverse.

## What is wrong with the reasoning so far (the developer's discussion, 2026-11-05)

lambda* takes `s = v + e_s`, `z = v + e_z`, with errors independent of v and of each other. Then
`Var(s) - Cov(s, z)` is `Var(e_s)`. In general it is `Var(e_s) + Cov(v, e_s)`, which an error running
against the truth makes negative. A squeezed teacher, `s = c v` with `c < 1`, gives `c (c - 1) Var(v)`:
with c = 0.8 and Var(v) = 1, the real error variance is 0.04 and the formula says -0.16.

So a negative lambda* says the model does not apply, not that s has no error. `polyline.rs` says
"no blend corrects" a squeezed s, so s alone, and that is doubtful: any lambda above 0 moves the
label's expected value toward v. lambda* also minimises a label's variance, while a network fitted
on a billion positions averages noise away and keeps bias.

## The measurement

A read-only `smithy` command over the scattered buckets of a source, writing nothing, printing one
line per progress bucket:

- the count;
- the mean of s and of z, for a plain bias;
- `Var(s)`, `Var(z)`, `Cov(s, z)`;
- lambda* unclamped;
- the slope `b = Cov / Var(s)`;
- the slope again in logit space, with s clipped away from 1 and -1, and z's logit undefined, so as
  a logistic regression of z's two ends on `logit(s)`, or binned: mean z against mean s in about 20
  bins of s. The bins are the plainer and show the shape, so they come first.

The score scale's slope is distorted by the caps at 1 and -1, where late positions gather, so the
bins are what is believed if the two disagree.

`Moments` and `buckets::for_each_pair` already hold most of this. The scatter's buckets are shuffled
files, so reading one in a hundred of them is a fair sample and much quicker than the whole. Run on
an idle machine, after training.

**Two things to check in it, because the buckets are not a clean sample:** the tables' wins and
losses are left out of the sums (`is_certainty`), but their draws stay, as `s = z = 0`, so late
buckets lose decisive positions and keep drawn ones; and game length depends on the result, so
buckets of progress are partly selected by z. A variant leaving out `s = z = 0` shows the first's
size.

## First result (2026-11-05, 1 in 100 of Lc0's bucket files, about 12M positions)

- **The tables' draws do not matter:** without `s = z = 0`, lambda* agrees to three decimals.
- **Slopes near 1:** 0.64 in bucket 0, 0.97 in bucket 4, 1.07 at most. Yet lambda* reaches -0.27: the
  small `Var(s) - Cov` makes it very sensitive, so it is a poor instrument here.
- **Late, an S, not a squeeze:** bucket 8 maps s 0.15, 0.25, 0.45, 0.55, 0.75, 0.86 to z 0.03, 0.10,
  0.41, 0.64, 0.92, 0.97, crossing the diagonal near 0.5. So one slope cannot describe it, and "`s' = b
  s`" below would stretch the small values the wrong way.
- **Early, z below s:** bucket 0 maps 0.24 to 0.14, 0.97 to 0.80: overconfidence, or Lc0's exploration
  moves spoiling good positions in its own games.
- **Side to move:** mean s about +0.015 early, mean z about 0.
- **The S may be hindsight.** Progress uses the game's final length, which depends on the result:
  a position late by progress is one whose game was about to end. So the same command now also groups
  by pieces on the board, which the position knows. If the S survives there, Lc0's scores are
  miscalibrated in endings, and the fix is a curve `E[z | s, pieces]`; if it flattens, the late 0 is
  harmless and the polyline stays, its comment corrected.

## Second result: by pieces, about 12M positions (2026-11-05)

- **The S survives, smaller.** 10 to 12 pieces besides the kings maps s 0.25, 0.35, 0.75, 0.85 to z
  0.13, 0.24, 0.83, 0.92; 7 to 9 pieces 0.11, 0.21, 0.82, 0.92. Bucket 8 by progress gave 0.75 to 0.92,
  so its extreme part was hindsight, and a real part remains, crossing the diagonal near 0.55 to 0.65.
- **It grows as pieces leave:** nearly straight at 25 or more, clear from about 20 down.
- **The opening's large gap was mostly hindsight too:** at 28 to 30 pieces, 0.25 gives 0.19, where
  bucket 0 gave 0.14.
- **Side to move:** s is about 0.02 high at 28 to 30 pieces (0.039 gives 0.016, -0.039 gives -0.041),
  fading to nothing by 4 to 6.
- **0 to 3 pieces hold only `s = z = 0`:** the tables answer them all, and their wins and losses are
  left out as certainties.
- **Decisions from this:** no squeeze, so `s' = b s` and the model of s are dropped. The late 0 is not
  shown wrong, but `polyline.rs`'s "squeezed, which no blend corrects" is to be corrected: a blend
  moves a label's expected value toward the S's true side.
- **Next, no new code:** after the baseline, a constant lambda well above the polyline (0.1 or so) on
  the same config, which straightens the S in proportion to lambda, judged in play. The S is monotone,
  so it barely changes which of two similar positions looks better, and the gain may be small; a
  curve `E[z | s, pieces]` only if that test shows one.

## The shape wanted (the developer, 2026-11-05)

lambda higher at the end of a game and smaller at the beginning, with no constant chosen by hand.
A proposal under test: with independent errors `Var(z - s) = noise(s) + noise(z)`, so lambda is
`noise(s) / Var(z - s)`, and only the numerator of lambda* is broken. At the game's end z has no
noise left, so `Var(z - s)` there is s's noise, n. With each ply left adding its own variance,
`Var(z - s at k plies left) = n + k sigma^2`, and `lambda(k) = n / (n + k sigma^2)`, both measured.
`polyline-diagnose <buckets> 100 plies` prints the table and the fitted lines. The check: on PeSTO,
where lambda* is valid (0.11 to 0.97), the new curve must come out about the same. It assumes s's
noise constant through a game, and a table that is close to a line.

## Third result: by plies left, about 12M positions (2026-11-05)

`Var(z - s)` is a clean line up to about 40 plies left, slope 0.0033 a ply, flattening near 0.21 past
60 as results cap at 1. The intercept is 0.0625 (0.078 without `s = z = 0`), which cannot be s's
noise: it would give lambda near 0.28 through most of a game, where round robins chose 0.02. z is
not free of s's bias at the end, so `Var(z - s)` counts the S as noise. Odd plies left read about
0.0025 above even ones, unexplained.

So n is taken from steps instead, s flipping sign each ply: `Var(s[t] + s[t + 1]) = 2 n + sigma^2`,
`n = (Var - sigma^2) / 2`, a constant bias cancelling; `polyline-diagnose <buckets> 100 steps` reads
the games. With `n = sigma^2`, lambda would be `1 / (1 + k)`: 0.02 at 50 plies left, 0.5 at the last.

## Fourth result, and the design proposed (2026-11-05, not applied)

**Steps, 14 of Lc0's segments.** `Var(s[t] + s[t + 1])`, plies left even, then odd, by bucket of
progress: 0.00242, 0.00205; 0.00479, 0.00347; 0.00505, 0.00415; 0.00466, 0.00439; 0.00360, 0.00423;
0.00296, 0.00406; 0.00233, 0.00394; 0.00194, 0.00403; 0.00152, 0.00423; 0.00114, 0.00340.

- Late, the steps' mean, about 0.003, agrees with the slope of `Var(z - s)`, 0.0033: the score moves
  as a random walk there, and the step is all real change, so n cannot be read late.
- Early, `Var(z - s)` grows by about 0.0002 a ply while the score moves 0.0045 a step. Movement that
  never reaches the result is search noise: `n` is about `(0.0045 - 0.0002) / 2 = 0.002`; bucket 0
  gives about 0.001. Rough: the cap at 1 flattens that slope too, so part of it may be real change.
- The even and odd lines differ, by who moves last, which plies left ties to who won. Probably that
  selection, not shown.

**The formula.** The intercept of `Var(z - s)` is bias plus n, the bias being the S's:

```text
Var(z - s at k) = bias + n + remaining(k)
remaining(k)    = Var(z - s at k) - intercept
lambda(k)       = n / (n + remaining(k))
```

On Lc0, n 0.002, intercept 0.0625, by plies left 1, 5, 10, 20, 40, 60, 100 or more: 0.25, 0.10, 0.06,
0.03, 0.016, 0.014, 0.012. Small through most of a game, rising to the end, never negative, nothing
chosen: the far end, not fitted, lands on the polyline's first value and near emily's 0.02.

**In the code, once validated:** a table by plies left, or a line and its cap, in place of the ten
values by progress, in `polyline.rs`; `polyline.txt`'s format, and the frozen polyline label set,
change with it. Lambda is computed from k at batch time, so no rescatter.

**The validation:** lambda* cannot judge this on Lc0, where it fails, so PeSTO does: its lambda* is
valid in every bucket (0.11 to 0.97), and the formula, run on PeSTO's own games, must come out close
to it, by progress. PeSTO has about 1.9M positions on Drive, no buckets, so `polyline-diagnose` needs
to read games from a folder. If it disagrees, the formula is not used. Only play judges it on Lc0.

## How to read it

- **b and the bins near the diagonal:** Lc0's scores are about right, and a late lambda of 0 does
  little harm. The polyline stays; correct the comment in `polyline.rs` and stop.
- **b clearly above 1, the bins steeper than the diagonal:** the squeeze is real. Next, a design.

## If the squeeze is real (not decided)

- **Modelling it directly is not proposed:** `s = c v + e`, `z = v + f` has four unknowns and three
  measured numbers, so its answer would be the extra assumption's.
- **Calibrate, then blend:** per bucket of progress, `s' = b s` clamped, or the bins' curve, then a
  weight on z. Regression slopes are themselves pulled down by noise in s, so it corrects only part.
- **Try late lambdas in play:** a floor, or a rise as PeSTO's, against the running run's network.

Whichever, games decide, as `polyline.rs` already says of its ten values. Each needs its labels
changed, so the frozen polyline label set is frozen again, and it applies to every teacher, so
PeSTO's results must not regress.
