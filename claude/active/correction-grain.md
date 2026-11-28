# The correction history in a fine grain

**Decided 2026-11-28 (the developer, option 1 of two).** Measured in bench-network.md's run log:
the correction history's integer arithmetic depends on the size of the evaluator's numbers. The
same evaluation, screlu-hidden, at depth 13: with the correction history, 118.0M nodes in `lc0`
numbers and 54.9M in `elo` numbers (`--scale 2.47`); without it, 34.7M and 44.7M. Its caps are
translated like every margin, so the difference is its rounding, which no parameter reaches, and
manual constants per unit cannot make the two units search alike while it stays.

## Today

Entries are `i32` in the evaluator's own unit (`correction_history`). An update moves each of the
three entries by `(error - entry) * weight / 16`, `weight = min(depth, 8)`, truncated toward zero
(`update_correction`); a correction is `sum / 3`, truncated, clamped to `correction_cap`
(`correction`). So a step below one unit is lost: at depth 1, a gap under 16 units never moves an
entry, which is 16 `lc0` units but only about 6.5 `lc0`-equivalent ones in `elo` numbers. The
share lost depends on the unit.

## The change

Entries held in a fine fixed point, `CORRECTION_GRAIN = 256` steps per unit of evaluation, as
Stockfish keeps its correction entries finer than its evaluation:

- `update_correction`: the clamped error times `CORRECTION_GRAIN`, then the same update
  `entry += (error - entry) * weight / CORRECTION_WEIGHT_DIVISOR`. A step lost is now below
  1/256 of a unit, in either unit.
- `correction`: `sum / (3 * CORRECTION_GRAIN)`, then the same clamp. Rounded to the evaluation's
  own grain once, at the end.
- Range: the largest entry is `2 * correction_cap * 256`, about 46,000 at an `elo` cap of 90, and
  an update multiplies a gap of at most twice that by 8: under 10^6, far inside `i32`.

Nothing else changes: the caps, the weights, the keys, the tables' size. The doc comments of
`correction_history`, `CORRECTION_DEPTH_CAP` and the two functions say the grain.

## Tests

- An update too small for one unit still moves an entry: depth 1, an error of 10, the entry no
  longer zero, where today it stays zero.
- Scale invariance, the point of it: the same samples in numbers 2.5 times larger give a correction
  2.5 times larger, within one unit, where today they differ by the lost steps.
- The existing correction tests kept, their expected values recomputed where the grain moves them.
- The search's node-count tests whose counts move: updated, the change being the reason.

## Run log

- **Written 2026-11-28.** The fixed-depth search test re-recorded (PeSTO's six positions: 15,897
  nodes against 16,885, the first position's move g1f3 to e2e4).
- **2026-11-28, depth 13, screlu-hidden:** unscaled 23,724,654 nodes (1,608,449 nodes/s), scaled
  through Elo's curve 26,286,491 (1,733,774): 11% apart, from 118.0M against 54.9M before. The units
  now search alike, as with the correction history off (34.7M and 44.7M, 29% apart). The `lc0`
  tree itself shrank fivefold (118.0M to 23.7M): today's whole-unit rounding was making the
  correction history far from what it was meant to be, under `lc0` most, so the check duel matters.
- **The check duel, depth 9 (2026-11-28, `arena duel` from the command line):** the old build, a
  snapshot of screlu-hidden `@untuned` from 0befde7 registered as `screlu-hidden-old-corr`, against
  the same network on the new build: **drawn, curtailed,** the new build 371.5 of 738 (50.3%,
  +222 -217 =299 from its side). Equal strength a depth on a far smaller tree.
- **The check duel, 10+0.1 (2026-11-28):** **drawn, curtailed,** the new build 391 of 764 (51.2%,
  +191 -173 =400 from its side, about +8 Elo), leaning its way. The old build played over UCI, a
  slight handicap. **Kept,** as the plan says for a draw.

## Measured, then checked

1. Bench at depth 13, screlu-hidden unscaled and at `--scale 2.47`: the two trees close to one
   another (the 29% seen with the correction history off, or less), not 118.0M against 54.9M.
2. **The `lc0` check duel:** the change moves the `lc0` search everyone has played, so screlu-hidden
   `@untuned` on the new build against the same on today's, at depth 9 and 10+0.1. To settle first:
   how an arena plays an older build (a snapshot of today's commit on Drive's `data/snapshots/`).
   Won or drawn: kept. Lost: discussed before going further.
3. Then bench-network.md's plan resumes: the `elo`-manual set calibrated, the duals duelled.
