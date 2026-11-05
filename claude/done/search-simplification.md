# Search simplification

**Done (2026-10-03).** Four margins became `MARGIN_PER_PLY` (2 pawns/ply; razoring moved from
1.5, its duel stopped undecided at 196 games); the aspiration delta lost its 8-thread repeat; every
kept parameter's doc comment cut to its value and one reason, its sweep history left here and in
the older plans, and the references to `claude/` documents in those comments removed. Left for
later: scaling the margin to the evaluation's own pawn (the scale test below).

Agreed with the developer 2026-10-03. TODO.md's "Simplify the search" item: the search's parameters
are picked by reasonable magnitudes, not tuned (no SPSA); go over them technique by technique, drop
or merge what does not earn its place, and state a reason for every value that stays.

## The inventory and the decisions

Margins are in Rotor's fixed pawn, `piece_value(Pawn) = 100`.

### Pruning margins: four become one

| constant | today | technique | decision |
|---|---|---|---|
| `FUTILITY_BASE_MARGIN` | 2 pawns/ply | classical and reverse futility | becomes `MARGIN_PER_PLY` |
| `RAZORING_MARGIN` | 1.5 pawns/ply | razoring | merged: 2 pawns/ply, slightly more conservative |
| `SEE_PRUNING_MARGIN` | 2 pawns/ply | SEE pruning (main search) | merged, same value already |
| `DELTA_MARGIN` | 2 pawns | delta pruning (quiescence) | merged: the shared margin at one ply |
| `SEE_MARGIN` | 0.5 pawn | SEE filter (quiescence) | kept: Stockfish lets slightly losing captures through (`see_ge(move, -74)`) |
| `FUTILITY_MAX_DEPTH` | 3 | futility | kept: a ply count, not a margin |

Each of the four was picked by its own node-count sweep, "settled in the middle", none measured for
strength. One margin is simpler to state and keeps the techniques consistent with each other. The
value is decided after the scale test below.

### Kept, reason stated in the code

- Null move: `NULL_MOVE_REDUCTION = 2`, `NULL_MOVE_MIN_REMAINING = R + 2` (the reduced search keeps
  one full ply).
- IIR: 1 ply at `remaining >= 4` (Stockfish's threshold).
- LMR: `ilog3(remaining) + ilog3(moves_tried)`, at most `remaining - 2`, eligible from `remaining >= 3`.
- LMP: `(3 + remaining^2) / (improving ? 1 : 2)` (Stockfish's formula).
- `QUIESCENCE_MOVE_LIMIT = 2` (Stockfish's `moveCount > 2`).
- Singular: `SINGULAR_MIN_DEPTH = 6`, `SINGULAR_MARGIN_PER_PLY = 2` (singular-extensions.md).
- `HISTORY_PRUNING_MULTIPLIER = -35`, in history units, converted from Stockfish's.
- History: the `remaining^2` bonus, `FLAT_HISTORY_MAX = 8192`, continuation history at 4x.

### Aspiration windows: one constant fewer

`ASPIRATION_MIN_DEPTH = 5` and `ASPIRATION_INITIAL_DELTA = 20` stay. `ASPIRATION_THREAD_STEP = 5`
and `ASPIRATION_THREAD_DIVERSITY = 8` exist only to make Lazy SMP threads differ: the delta becomes
`20 + 5 * thread`, without the repeat every 8 threads (bounded at 128 threads to 655 cp; the
constant's own comment finds no reason for the cap). Self-play, replay and the arena all search with
one thread, so this changes nothing they play: no duel.

### Out of scope

`GOOD_QUIET_THRESHOLD` (decided by duel, move-picker.md), `CHECK_INTERVAL`, time management.

## First: do the margins fit a network's scale?

TODO.md's open question, a precondition for choosing the shared margin: the margins are in a fixed
100-per-pawn, while a network regressed toward a blended target may compress its output toward
zero, and then clears fewer margins (a network searched 1.8x to 3.8x more nodes than the
hand-crafted evaluation at the same depth).

The test, no match needed: over a few thousand real positions (every position of amanda's
generation-1 selection games, `data/networks/amanda/gen-0001/selection/*/games.pgn`), the static
evaluation of the hand-crafted evaluation and of amanda's champion, both in centipawns from the side
to move. Reported:

- percentiles of `|eval|` for each (50, 75, 90, 99);
- the least-squares slope of the network's evaluation on the hand-crafted one, through the origin:
  below 1 means the network is compressed;
- the fraction of positions where each evaluation is beyond 1, 2, 4 and 6 pawns.

A bench (`nnue_bench --eval-scale`, or a sibling bench), not engine code. Then:

- slope near 1: the shared margin is 2 pawns/ply, as today's futility;
- slope well below 1: the margins are scaled by it, or expressed in the evaluation's own units;
  decided with the developer on the numbers.

### Result (2026-10-03, the training machine, amanda's generation-1 champion, 355,207 positions not in check)

| `\|eval\|` (cp) | p50 | p75 | p90 | p99 |
|---|---|---|---|---|
| hand-crafted | 116 | 362 | 763 | 1681 |
| network | 66 | 218 | 904 | 3172 |

| beyond | 1 pawn | 2 | 4 | 6 |
|---|---|---|---|---|
| hand-crafted | 54.0% | 37.9% | 22.8% | 14.2% |
| network | 38.8% | 26.2% | 17.9% | 13.9% |

Slope through the origin 1.283, correlation 0.911. Not one scale: the network is compressed near
equality (its pawn about 0.6 of the hand-crafted one at p50 and p75) and stretched far from it (p99
ratio 1.89), the shape of the old label curve converted back to pawns. The slope is pulled up by the
tails and describes neither region. Where the margins act, near the bounds, a 2-pawn margin is
about 3.3 hand-crafted pawns for amanda: every margin is more conservative under the network than
intended, plausibly much of the 1.8x to 3.8x more nodes.

Decision: the merge goes ahead at **2 pawns/ply** (one question for its duel). Scaling the margin to
the evaluation's own pawn (about 1.2 pawns/ply for amanda) is a separate, later question, and would
make the margin depend on the evaluation in use, since the hand-crafted evaluation shares the
constants.

## The merge's duel (2026-10-03)

Futility, SEE pruning and delta pruning already used two pawns, so the merge changes razoring alone,
from 1.5 to 2 pawns/ply: the duel asks exactly that. `MARGIN_PER_PLY` replaces the four constants;
the old razoring value survives only behind a temporary process-wide flag, `SHARED_MARGIN` (UCI
option `SharedMargin`, default off, so the champion is today's search), set by the arena suffix
`+margin`, refused in-process. The duel: `amanda-0001` (champion) against `amanda-0001+margin`
(challenger) at 10+0.1. Then the flag, option and suffix go, and razoring keeps whichever won.

**Result: stopped by the developer at 196 games**, `+61 -55 =80` from the champion's side (razoring
at 1.5), S 0.515 [0.390, 0.610]: nothing shown either way. By this plan's rule an undecided result
goes to the simpler version, so **razoring reads `MARGIN_PER_PLY`**; the flag, `OLD_RAZORING_MARGIN`,
the `SharedMargin` option and `+margin` are removed.

**Aspiration merge done** (step 3): `ASPIRATION_THREAD_DIVERSITY` removed, the delta is
`20 + 5 * thread` with no repeat; its wrap test removed, the per-thread test extended to 128 threads.

## Order of work

1. The scale test (a bench, runnable while nothing else holds the CPU).
2. The margin merge, one duel for the four margins together (one question: "one shared margin
   instead of four"), today's search as champion, at 10+0.1.
3. The aspiration merge, no duel.
4. The kept parameters' doc comments brought to one stated reason each, no behaviour change.
5. TODO.md updated; this document to `done/`.
