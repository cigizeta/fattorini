# The label-space scale `c`, step 1: measure it beside `p`

**Status: done 2026-10-20.** Measured (Results 1 to 4); step 2, the search's pawn from `c`, is
label-scale-search.md.

The first step of TODO's BIG Evaluation item (replace the pawn scale `p` with `c`, human-rotor.md's
"Step 1"): measure `c` on today's networks, without changing the engine, and check human-rotor.md's
sanity condition, `100 / c` near `p`. A large gap means the label curve does not fit PeSTO's shape,
and the replacement is rethought before it is designed.

## What is measured

- **`c`**, per network: the `c` minimising `sum (score(h_i / 100 / c) - score(n_i / 100))^2` over
  the 10,000 compiled-in positions (`pawn_scale_boards`), all of them, no window, `h = 0` kept.
  `h_i` is PeSTO's evaluation in centipawns, `n_i` the network's, both from the side to move;
  `score` is the label curve (`labels::score`). A golden-section search over `[0.25, 4]`, since `c`
  sits inside the `atan` (bracket widened to `[0.25, 10]` after result 1). Unrounded, so the
  measurement shows what rounding would hide.
- **Beside it:** `100 / c`, in percent like `p`, the network's `p` as measured at load, and the
  residual RMS of the fit in label space, against the same RMS for `p`'s straight line pushed
  through the curve (`score(h_i * p / 100 / 100)`), so the two fits compare on one footing.

## Changes

1. `nnue::fit_label_scale(pairs)`: the fit on `(h, n)` pairs in centipawns, pure, tested.
   `nnue::measure_label_scale(evaluate)`: the fit on `pawn_scale_boards`, as
   `measure_pawn_percent` is on its window.
2. `nnue_bench <network.nnue> --label-scale`: prints `c`, `100 / c`, `p`, and both residuals.

## Result 1 (the Lichess machine, 2026-10-20): the sanity check fails

| Network | `p` | `c` | `100 / c` | RMS `c` | RMS `p` through the curve |
|---|---|---|---|---|---|
| caroline-0001 | 50% | 3.90 | 25.6% | 0.256 | 0.273 |
| amanda-0002 | 55% | 3.65 | 27.4% | 0.250 | 0.270 |
| betty-0002 | 60% | 4.00 (bound) | 25.0% | 0.235 | 0.267 |
| dorothy-0001 | 60% | 4.00 (bound) | 25.0% | 0.244 | 0.281 |

`100 / c` is about half of `p`, two networks hit the bracket's upper bound, and both fits leave a
residual of about 0.25 in label space: one constant does not relate these networks to PeSTO,
neither `p` nor `c`. A likely reason: trained on labels blended with game results, the networks
predict expected outcome, compressing lopsided positions far more than the label curve does, a
region `p`'s ±300 cp window never saw. **Next:** the bracket widened to 10, and `--label-scale`
prints the shape by bins of PeSTO's evaluation. **The developer (2026-10-20):** Lc0-trained
networks may change the picture, their labels coming from a stronger search; the coming self-play
networks still have a weak search side, so both kinds are to be measured.

## Result 2 (the Lichess machine, 2026-10-20): the outliers are PeSTO's, mid-exchange

By bins of PeSTO's static evaluation, every network reads the 520 positions where the side to move
is 3 to 10 pawns behind (`h < -300`) as about equal (`n / h` near 0), while the other side, only 60
positions, looks normal (`n / h` 0.5 to 0.77 from +100 to +300; caroline's `s_net` there equal to
`p`'s prediction, 0.370 and 0.551 against 0.370 and 0.555). Positions of real games taken with the
side to move far behind by static count are mid-exchange: the recapture is coming, PeSTO counts the
material as lost, the network rightly does not. Those 5% dominated the label-space fit, pulling `c`
to 4. **Decided (the developer, 2026-10-20): PeSTO's side becomes its quiescence**
(`search::hand_crafted_quiescence`) on all 10,000 positions, option 1 of two (the other: only the
positions where quiescence equals the static evaluation). `p`'s window had mostly hidden the flaw.

## Result 3 (the Lichess machine, 2026-10-20): quiescence as PeSTO's side

| Network | `p` | `c` | `100 / c` | RMS `c` | RMS `p` through the curve |
|---|---|---|---|---|---|
| caroline-0001 | 50% | 2.860 | 35.0% | 0.239 | 0.246 |
| betty-0002 | 60% | 2.847 | 35.1% | 0.213 | 0.231 |

Quiescence moved 2,768 of the 10,000 positions; the bins below -300 shrank from about 520 positions
to 94. `c` fell from about 4 to 2.85, off the bracket's bound. Read by bins:

- **The shape fits.** From -300 to +300, `c`'s prediction tracks the network's median label within
  about 0.03 in every bin. The network flattens only beyond 300 cp, on about 200 positions: there it
  behaves like an expected outcome, as Result 1 guessed.
- **The residual is scatter, not shape.** The RMS barely moved (0.24 and 0.21). The bin medians
  fit, so what is left is the spread from one position to the next, which no single constant
  removes.
- **`c` is steadier than `p`.** caroline and betty give the same `c` (2.86 and 2.85), although
  their `p` differs (50% and 60%).
- **The sanity check still compares unlike things.** `p` is measured against PeSTO's *static*
  evaluation, `c` now against its quiescence. Against quiescence, `n / h` is 0.3 to 0.5 in every
  bin, so `p` would also come out lower. A fair check needs `p` measured against the same
  reference. **Next:** `--label-scale` also prints `p` against quiescence (same slope and window
  as at load, unrounded) and its residual through the curve; only the bench changes.

## Result 4 (the Lichess machine, 2026-10-20): `p` against quiescence, the gap is real

| Network | `p` static | `p` quiescence | `c` | `100 / c` | RMS `c` | RMS `p` static | RMS `p` quiescence |
|---|---|---|---|---|---|---|---|
| caroline-0001 | 50% | 53.1% | 2.860 | 35.0% | 0.239 | 0.246 | 0.249 |
| betty-0002 | 60% | 64.1% | 2.847 | 35.1% | 0.213 | 0.231 | 0.236 |
| amanda-0002 | 55% | 52.8% | 2.823 | 35.4% | 0.236 | 0.247 | 0.245 |
| dorothy-0001 | 60% | 63.0% | 2.975 | 33.6% | 0.220 | 0.240 | 0.244 |

The guess in Result 3 was wrong: against quiescence `p` stays at 53 to 64%. The gap with `100 / c`
is not caused by the reference. It comes from how each fit weighs the scatter of `n` around the
trend:

- `p` is a least-squares slope in centipawns, weighted by `h^2`. Large `|h|` and large `|n|`
  dominate it.
- `c` is fitted in label space, where the curve is concave. A position whose `n` lands far from
  the trend weighs much less there. On noisy data the concave curve also pulls the mean label
  toward 0 (Jensen), so the `c` that matches the mean labels corresponds to a smaller pawn.

In label space `c` wins on every network and in every bin from -300 to +300, where `p`'s line
overshoots each one. `c` is also nearly the same on all four networks (2.82 to 2.98), while `p`
ranges from 50% to 64%. **Conclusion:** human-rotor.md's sanity check (`100 / c` near `p`)
assumed little scatter, and it does not hold for these networks. It cannot decide the question.
`c` fits the space the networks are trained in better.

## Steps for the developer

Run it on at least `lc0-psq-0001-0.15`, `lc0-psq-threats-0001-0.25` and caroline-0001's champion.
