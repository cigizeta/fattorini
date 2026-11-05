# The pawn scale on held-out master positions

**Status:** implemented 2026-10-08, in two steps since `include_str!` cannot compile before the file
exists: the writer (`nnue_bench --write-pawn-scale-positions`, 10,000 positions written to
`src/pawn_scale_positions.txt`), then the engine reading it at load. All 616 library tests pass;
hand-crafted 3,140,380 nodes, unchanged; amanda-0001 now measures 55%, 1,194,538 nodes on the 49
bench positions at depth 9 (0.73 s), against 1,226,878 at 70% (-2.6%). The duel against 70%
started the same day, `~/rotor-pawn55.exe` against `~/rotor-scaled.exe`, and was **stopped by the
developer at 274 games, inconclusive**: +75 -69 =130 for 55%, 51.1%, mid-way between the bounds
(40.7% and 59.3% at 274). No verdict, but moot: amanda's margin is now chosen by a tournament
(margin-round-robin.md), which compensates for any scale, and the Lumbra measurement stays as the
better one for networks without a tuned margin (the lambda candidates).

**Since (2026-10-08):** the pawn scale stays the unit, and a network's margin per ply is tuned by
play on top of it (margin-search-command.md); a network never tuned (a lambda candidate) searches at
`MARGIN_PER_PLY` times this scale. Done.

## Why

`nnue::measure_pawn_percent` measures a network's pawn at load on Stockfish's 49 bench positions
and every position one legal move from each (about 1,700). That set is unstable
(psq-node-cost.md, 2026-10-08): by the same statistic,

| network | at load | Lumbra, 10,000 positions | Lumbra, 100,000 positions |
|---|---|---|---|
| amanda-0001 | 70% | 55% (slope 53.0) | 55% (53.1) |
| betty-0001 | 25% (the clamp's floor) | 60% (58.0) | 60% (59.3) |
| caroline-0001 | 150% | 55% (52.7) | 55% (53.2) |

The two Lumbra sets share no game and agree to the rounded percent; the small set scatters from
the clamp's floor to 150%. Likely cause, unverified: one random legal move often leaves a piece
hanging, which a network sees and hand-crafted does not.

## Design

- **The positions:** the 10,000 positions of the 5,000-game Lumbra reference read two per game
  (`nnue_bench --pawn-scale --per-game 2`'s set, exactly), as FENs, one per line, in a text file
  of the library, `src/pawn_scale_positions.txt` (about 600 KB). Held-out master games, never in a
  training corpus.
- **Written once** by a bench mode, `nnue_bench --write-pawn-scale-positions`, from the reference
  on Drive; checked into git; never regenerated.
- **Read at load** with `include_str!`, so a release stays `rotor.exe` and its `.nnue`, with no
  `data/` and no Drive. About 10,000 evaluations from scratch, about 10 ms.
- **The statistic is unchanged:** the slope through the origin over positions not in check, with
  hand-crafted within 300 cp and not 0, rounded to 5% and clamped to 25 to 200%. Only the positions
  change. Hand-crafted stays exactly 100%, so its search does not move.
- `--pawn-scale` keeps its own copies of the constants until then; once the engine reads the same
  positions, the bench's "at load" column and its Lumbra column agree by construction, and the
  copies can go.

## Tests

- Hand-crafted measures exactly 100% (as now), and the existing doubled-weights test still gives
  twice the scale.
- The embedded file parses: every line a legal FEN, the side to move not in check.
- amanda-0001, betty-0001 and caroline-0001 measure 55%, 60% and 55%, as the bench found.

## The duel

Amanda is generation 2's teacher, and her margins move from 140 to 110 cp per ply. One duel:
amanda at 55% against amanda at 70% (`~/rotor-scaled.exe`), 10+0.1, the usual settings,
`duel-pawn-scale.pgn`. **Pass rule (the developer, 2026-10-08, before the duel): a draw passes**,
since this corrects a measurement found unstable rather than testing a hypothesis; only 55% shown
weaker keeps 70%.

## Decisions

All decided; to implement once approved.
