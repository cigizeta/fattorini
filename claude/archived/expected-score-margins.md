# Pruning margins in expected score, for networks

**Superseded, see margin-search-command.md:** a network's margin is now tuned by play.

**Status:** closed 2026-10-08. Three duels against scaled, none passed: `q` = 99% shown weaker,
`q` = 99.9% stopped inconclusive, the anchored shape losing (34.6% at 13 games) and 26% slower to
depth 9. Every move away from the scaled margins lost, in either direction; the developer accepted
the scaled margins. The temporary features are removed. What the day did find: the pawn scale
itself was mismeasured at load (pawn-scale-positions.md).

## Why

Both the scaled and the measured margins are centipawns anchored to hand-crafted's 200 cp per ply,
a magnitude picked for PeSTO, and the measured ones also depend on which positions were measured.

## The idea (the developer, 2026-10-08)

- **Hand-crafted keeps centipawns:** 200 cp per ply, untouched, so its trees and node counts do not
  move.
- **Every network compares in expected score:** its evaluation and the bound both go through
  `labels::score(pawns) = atan(pawns / A) / K`, the curve in `[-1, 1]` the networks are trained
  against (1 pawn = 0.5, a 75% expected score). Only this forward conversion is used, never the
  inverse, which the developer judges unreliable. At a node, for a margin `M(d)` in label units:
  - razoring: `score(e) < score(alpha) - M(d)`;
  - classical futility: `score(static) + M(d) <= score(alpha)`;
  - reverse futility: `score(static) - M(d) >= score(beta)`.
- In centipawns a fixed `M` widens away from equality, where the residuals were found wider.
- The label range is `[-1, 1]`: today's scaled 140 cp per ply is about 0.6 of it near equality, so
  a margin linear in depth would span the whole range by `d` = 2. `M(d)` is much smaller per ply,
  or not linear.

## Measuring `M(d)`: more Lumbra games (the developer, 2026-10-08)

`M(d)` is read from the residual tails measured in label space, on many more held-out master
games than the 5,000 of today's reference, for a precise p99 (about 1,000 tail positions instead of
about 100).

- **A second reference**, never regenerated, beside the first:
  `data/lumbra/LumbrasGigaBase_OTB_ELO2400-50000.pgn.zst`, 50,000 games of 40 plies or more,
  evenly spaced over the source on Drive, cut by `--cut-reference` with a game count (today
  fixed at 5,000).
- **Two positions per game**, at plies drawn deterministically from the game's index, so 100,000
  positions without holding 4 million boards in memory: the reader keeps them while it streams.
- **Tails in label space:** `--residuals` also prints each tail as
  `score(value) - score(qsearch)` and `score(static) - score(value)`, in thousandths of label
  space, beside the centipawn ones.
- **Cost:** amanda at `d` = 4 took 75 s per 10,000 positions, so about 12 minutes for 100,000.
- **Implemented 2026-10-08** in `nnue_bench`: `--reference-games N` (to cut or read the reference
  of N games; not `--games`, which `--search-only` already takes as a directory), `--per-game K`
  (K positions per game, evenly spaced from an offset hashed from the game's index, kept while
  streaming), and every tail printed in centipawns and in thousandths of label space.

## Results

**Hand-crafted, 50,000-game reference, 100,000 positions (2 per game), 2026-10-08, the Lichess machine, about
230 s.** p99, centipawns and thousandths of label space:

| d | `value - qsearch` cp / label | `static - value` cp / label |
|---|---|---|
| 1 | 68 / 305 | 71 / 254 |
| 2 | 93 / 369 | 80 / 319 |
| 3 | 114 / 443 | 94 / 377 |
| 4 | 130 / 472 | 107 / 435 |

- The centipawn p99s match the 10,000-position run within a few cp (66, 91, 109, 123 and 67, 81,
  91, 106): the reference is consistent, and the p99 now rests on about 1,000 tail positions.
- In label space the tails grow sub-linearly, about +60 per ply on a base near 250: `M(d)` looks
  like `a + b * d`, not `b * d`.
- Hand-crafted keeps centipawns regardless; these rows are the baseline for the networks'.

**Amanda-0001, the same 100,000 positions (2026-10-08, the Lichess machine, 2:34 with the faster search of
residuals-bench-speed.md; her depths 1 to 3 match the slow search's to the digit).** p99:

| d | `value - qsearch` cp / label | `static - value` cp / label | the same, `|static|` > 300 cp, label |
|---|---|---|---|
| 1 | 45 / 188 | 67 / 267 | 75 / 252 |
| 2 | 55 / 242 | 62 / 270 | 97 / 173 |
| 3 | 62 / 260 | 85 / 348 | 132 / 309 |
| 4 | 73 / 304 | 77 / 327 | 153 / 198 |

- **In label space her tails grow slowly**, roughly `160 + 37 d` (`value - qsearch`) and
  `245 + 25 d` (`static - value`) thousandths: `M(d) = a + b * d`, a base most of the margin.
- **Far from equality they are narrower than overall** (the last column), at every depth, where in
  centipawns they are several times wider: one `M(d)` in label space fits every evaluation level,
  generous rather than tight far from equality.
- **Against today's scaled margin in label space:** 140 cp per ply near equality is
  `score(1.4) = 0.61` at `d` = 1 and `score(2.8) = 0.79` at `d` = 2, two to three times her p99.
  A p99 margin in label space would prune far more than today's near equality.
- Reverse futility's tail stays the wider of the two at every depth, as in centipawns.

## The duel (decided 2026-10-08)

- **`q` = 99% (the developer)**, the natural reading, a bet failing 1 time in 100; no duel chooses
  it. If this duel fails, at most one more at a far value (99.9%), never a series.
- **`M(d) = a + b * d`, fitted by least squares to amanda-0001's p99 at `d` = 1 to 4**, in label
  units: razoring and classical futility (`value - qsearch`) `0.157 + 0.037 d`; reverse futility
  (`static - value`) `0.239 + 0.026 d`. Razoring beyond `d` = 4 extrapolates the fit.
- **The tests**, `score` being `labels::score` of the network's centipawns over 100, forward only:
  razoring `score(e) < score(alpha) - M(d)`; classical futility `score(static) + M(d) <= score(alpha)`;
  reverse futility `score(static) - M(d) >= score(beta)`. An infinite bound maps to `±1`, past
  which no margin can reach, so no extra guard is needed; the mate and tablebase guards stay.
- **Unchanged:** the null move's reduction term and delta pruning keep the pawn scale; hand-crafted
  keeps 200 cp per ply, its node counts unchanged.
- **The build (temporary):** a feature `expected-score-margins` with amanda-0001's four constants,
  applied only when a network is loaded; `~/rotor-expected.exe` copied aside, never run with
  another network. If it passes, a design for measuring and storing `a` and `b` per network
  follows; if not, the code goes.
- **Pass rule (the developer, before the duel): a draw passes**; only expected score shown weaker
  keeps the scaled margins. The developer already judges scaled unsatisfactory, and expected score
  needs no hand-crafted anchor and one `q` for every network.
- **The duel:** cutechess-cli on idle the Lichess machine, plugged in, `expected` against `~/rotor-scaled.exe`,
  both amanda-0001, 10+0.1, the usual settings, 1000 games, `duel-expected-margins.pgn`.
- **Before it:** node counts on the 49 bench positions with the feature, beside scaled's
  1,226,878; hand-crafted 3,140,380 unchanged.

**Result (2026-10-08, the Lichess machine): expected score shown weaker at 60 games**, +9 -34 =17, 29.2%,
below the duel rule's loss bound of 30.0% at 60 (`duel-expected-margins.pgn`). The scaled margins
stay. Before it, on the 49 bench positions at depth 9: 1,574,117 nodes against scaled's 1,226,878
(+28%), yet 5% faster (2.19M nodes/s against 1.61M, razoring sending more nodes to cheap
quiescence).

Why, read afterwards: near equality the p99 margins are four to five times tighter than scaled's
(about 31 cp at `d` = 1 and 45 cp at `d` = 3 in amanda's centipawns, against 140 and 420),
an extremely aggressive pruner. Her p99 in centipawns (45 to 73) says the same, so the conversion is
not at fault: **p99 of the error at played positions is far too small a margin in play.** Interior
nodes err more than played positions, and one misjudgement repeats across many siblings. Today's
margins sit far beyond p99, so a working `q` is well above 99%.

## The second duel: `q` = 99.9% (the developer, 2026-10-08)

The one further duel allowed, at a far value. Amanda's label-space p99.9 on the same 100,000
positions (her p50 to p99 matching the first run to the digit), fitted by least squares over
`d` = 1 to 4:

| bet | p99.9 at `d` = 1..4 | fit |
|---|---|---|
| razoring, classical futility (`value - qsearch`) | 0.435, 0.527, 0.566, 0.625 | `0.386 + 0.061 d` |
| reverse futility (`static - value`) | 0.552, 0.552, 0.646, 0.622 | `0.517 + 0.030 d` |

Near equality, in amanda's centipawns (read only): razoring and futility about 85, 105, 124 cp at
`d` = 1 to 3, reverse futility about 115, 127, 139, against scaled's 140, 280, 420. Still up to
three times tighter from `d` = 2; halfway between the p99 margins and scaled. Same build, constants,
pass rule (a draw passes) and duel settings as the first, `duel-expected-margins-999.pgn`.

**Result (2026-10-08): stopped by the developer at 57 games, inconclusive**, +14 -18 =25, 46.5%,
far from the loss bound (28.9% at 57). A draw passes only at 1000 games, so the p99.9 margins are
not adopted; the anchored version below needs no `q` and is tried instead, rather than wait an
hour for a likely near miss.

## Next: the shape in label space, the level anchored (the developer, 2026-10-08)

No `q` and no measurement. At remaining depth `d`, the margin is today's scaled one carried into
label space by the forward conversion alone: `M(d) = score(m(d))`, `m(d)` being
`eval_margin_per_ply * d` in the network's centipawns (140 cp per ply for amanda), the same `M(d)`
for all three prunings.

- **Near equality** it prunes exactly as scaled does: at a static evaluation of 0, futility's
  `score(0) + score(m) <= score(alpha)` holds exactly when `alpha >= m`.
- **Away from equality** it widens in centipawns, as the measured errors do.
- **Across depths** it grows sub-linearly, `score` being concave (0.61, 0.79, 0.87 at `d` = 1 to 3
  for amanda), the shape the residuals showed, unfitted.
- **Kept:** the level at equality is today's 200 cp per ply in the pawn scale, the hand-crafted
  anchor, for the level only. **Dropped:** `q`, the fitted constants, anything per network beyond
  the pawn scale already measured at load. So it applies to every network as it is.
- **Build:** a feature `anchored-label-margins` (implying `expected-score-margins`), its margins
  from `eval_margin_per_ply` instead of amanda's constants. Same duel settings and pass rule (a draw
  passes), `duel-anchored-margins.pgn`, after the p99.9 duel, never beside it.

## Open questions

1. `M(d)`'s shape: read from the tails, linear, square root or a table per depth.
2. Which quantile of the label-space tails: not anchored to hand-crafted any more, so chosen
   outright.
3. The lookup: `atan` per comparison, or a table over the clamped centipawn range.
4. The null move's eval term and delta pruning: stay scaled, or move to expected score too.
5. The duel and its pass rule.
