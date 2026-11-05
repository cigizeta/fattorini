# Is Lc0's late lambda of 0 right? A measurement first

**Status:** the measurement built 2026-11-05 as `smithy polyline-diagnose <buckets> [one-in]`, not yet
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
