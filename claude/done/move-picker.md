# Move picker: Stockfish's staged picker

Started 2026-10-03. Status: **done** (2026-10-03): implemented, decided by duels, hard-coded (see
"Results"). Below, the design as it was argued, then what the duels chose. Implemented 2026-10-03 (`src/search/move_picker.rs`,
`MoveOrdering`, the UCI options, the `+t` names), not yet run. Implemented now on the Lichess machine, before the search freeze ends
(TODO.md): the freeze protects generation 1's selection on the training machine, and the training machine does not pull before
betty's generation 1 is promoted; the default stays `Legacy`, so nothing changes until a duel says so.

## Why

The profile in TODO.md (`perf` on `nnue_bench --big-five --depth 9`) puts about 23% of a node in
move ordering, most of it rescanning: every pick re-examines every remaining move. The noisy scan
(`try_best_noisy_move` 7.2%) recomputes `is_quiet` and SEE (8.5%) for every remaining capture on
every pick; the killer scans (3.2%) and the history scan (`try_best_history_move` 3.6%) do the same
for the quiets; quiescence's scan (`move_score`) is quadratic the same way.

## Today's order (`alpha_beta_with_buffer`, the loop at `for i in start..end`)

At each `i`, the first stage that succeeds swaps its move into `moves[i]`: the TT move (at `i ==
start` only); the highest SEE among the remaining non-quiet moves if `> 0`, recomputed every pick;
killer slots 0 and 1; the highest `combined_history_score` (flat plus continuation history) if `> 0`,
read live every pick; otherwise whatever sits at `moves[i]`. Captures with SEE `<= 0` are ordered
with the quiets. Quiescence: a noisy TT move, then the best `move_score` (MVV-LVA plus promotion)
until none is left.

History changes while a node searches its moves (every child subtree writes the same tables), and
today's history stage sees those changes. Stockfish scores quiets once per node and does not. So the
new picker cannot give today's order: **it is a new search**, measured as one (below).

## Stockfish's picker (`movepick.cpp` and `history.h`, fetched 2026-10-03)

Stages, main search: `MAIN_TT, CAPTURE_INIT, GOOD_CAPTURE, QUIET_INIT, GOOD_QUIET, BAD_CAPTURE,
BAD_QUIET`; in check: `EVASION_TT, EVASION_INIT, EVASION`; quiescence: `QSEARCH_TT, QCAPTURE_INIT,
QCAPTURE`. The TT stage is skipped when the TT move is not pseudo-legal, and `select` skips the TT
move in every later stage (`*cur != ttMove`).

- **Captures**, scored once at `CAPTURE_INIT`: `captureHistory[pc][to][captured] + 7 *
  PieceValue[captured]`, sorted fully (`partial_insertion_sort(..., INT_MIN)`). SEE **lazily**, at
  pick: `GOOD_CAPTURE` takes the next capture if `see_ge(m, -value / 18)`, else moves it to the bad
  captures and goes on.
- **Quiets**, scored once at `QUIET_INIT`: `2 * mainHistory + 2 * pawnHistory + continuation
  histories 1, 2, 3, 4 and 6 plies back`, `+ 16384` for a check that `see_ge(m, -75)`, a threat term
  (`20 * PieceValue` for leaving or entering a square attacked by a lesser piece), and a low-ply
  history term. Sorted above `-3560 * depth` (`partial_insertion_sort`), the rest selected lazily.
  `GOOD_QUIET` takes quiets with value `> -14000`.
- Then `BAD_CAPTURE` (in the order they were set aside), then `BAD_QUIET` (value `<= -14000`).
- **Evasions**: captures `PieceValue[captured] + 2^28`, quiets `mainHistory + continuationHistory[0]`,
  sorted fully.
- **Quiescence**: every capture by the capture score, sorted fully, no SEE filter at pick.
- **No killers.**
- Bounds: main history 7183, capture history 10692, continuation history 30000, pawn history 8192;
  every update `val += bonus - val * |bonus| / D`, which is Rotor's `apply_history_bonus`.

## Rotor's version: the shape, with the tables Rotor has

Rotor has flat history `[piece][to]` (max 8192), continuation history one ply back (max 32768), and
killers; no capture, pawn, low-ply or deeper continuation histories, no check or threat terms. The
picker takes Stockfish's **stages and their rules**, and fills each score with the Rotor signal that
plays its part:

- **Captures**: `7 * piece_value(captured)` (plus the promoted piece's value for a promotion, noisy
  as today), no capture history. Good if `see(m) >= -value / 18`, the same proportion of the score
  as Stockfish's threshold, for want of capture history the victim's value alone.
- **Quiets**: `flat history + continuation history`, today's `combined_history_score`, scored once.
- **Evasions**: Stockfish's rule with Rotor's quiet score.
- **Quiescence**: unchanged for the duels (as implemented, 2026-10-03): it already picks by MVV-LVA,
  so sorting once instead of rescanning is a speed step only, done when the winner is hard-coded.
- **Killers: kept at first**, as a stage after the good captures; a duel decides whether they go
  (open choice 1).
- **The two quiet thresholds**, from the measurement in history-distribution.md (amanda's generation
  1), not from Stockfish's numbers, which Rotor's scores never reach: **no sort limit** (every quiet
  sorted), and the good-quiet threshold one of two candidates, decided by a duel (open choice 2).

The missing signals (capture history, the check and threat terms, deeper continuation history) are
each a later, separately measured step, not part of this one.

## Open choices

1. **Killers**: decided by a duel, not by Stockfish's example (the developer, 2026-10-03). Rotor's
   histories are thinner than Stockfish's, and killers may be doing work Stockfish's other tables do.
   The picker is first built with killers as a stage after the good captures, then a duel, killers
   kept against killers dropped, decides.
2. **The good-quiet threshold `T`**: decided by a duel between two candidates (the developer,
   2026-10-03), see "The two quiet thresholds" below.

All of them are `rotor-arena duel`s, one question per duel, the rest of the picker held fixed.

## Playing variants side by side

A duel plays two players in one run, in-process at a fixed depth and over UCI under a clock, so the
ordering is a per-search setting, never a constant or a build:

- `pub enum MoveOrdering { Legacy, Picker { good_quiet_threshold: i32, killers: bool } }` in
  `search.rs`, `Copy`, **default `Legacy`** (today's chain, unchanged) until the duels decide. Passed
  to `choose_best_move`, its two siblings and `search_root` right after `evaluator`, for the reason
  `evaluator` is: two players in one process. Self-play and replay pass the default, so training data
  never changes by accident.
- UCI options: `MoveOrdering` (combo, `legacy` or `picker`, default `legacy`), `GoodQuietThreshold`
  (spin, default `0`), `Killers` (check, default `true`). lichess-bot sets none, so it plays `Legacy`.
- Arena player names take an optional suffix: `<player>+t<T>` is the picker with threshold `T` and
  killers, `<player>+t<T>-nokillers` without them: `amanda-0001+t0`, `amanda-0001+t-128`,
  `amanda-0001+t0-nokillers`. No suffix is `Legacy`. The name stays the PGN and database label, so
  every game records its variant. In-process the entry carries the `MoveOrdering`; over UCI it becomes
  the three options.

**All of this is temporary** (the developer, 2026-10-03): once the duels have decided, the winning
ordering becomes the search's only one, hard-coded, and `MoveOrdering`, the three UCI options, the
`+t` suffix and the losing code (`Legacy`'s stage chain, or the killers) are removed.

## Results

- 2026-10-03, the Lichess machine, duel 1, `amanda-0001` against `amanda-0001+t0`, records from the champion's
  side (`amanda-0001`): at depth 9, `+t0` stronger, `+7 -28 =5`, S 0.238 in 40 games; at `10s+0.1`,
  `+t0` stronger, `+20 -50 =22`, S 0.337 in 92 games (the picker about +115 Elo), no time forfeits.
  The picker stays.
- 2026-10-03, the Lichess machine, duel 2, `amanda-0001+t0` against `amanda-0001+t-128`, from `+t0`'s side: at
  depth 9, drawn at 1000 games, `+279 -355 =366`, S 0.462 (bounds 0.451 to 0.549), leaning to
  `+t-128` by about 26 Elo; at `10s+0.1`, drawn at 1000 games, `+269 -322 =409`, S 0.474, leaning to
  `+t-128` by about 18 Elo. Both lean the same way, neither decides: **`T = -128` stays** (the
  developer, 2026-10-03), Stockfish's "small tail" shape; the two are close to equal in strength.
- 2026-10-03, the Lichess machine, duel 3, `amanda-0001+t-128` against `amanda-0001+t-128-nokillers`, from the
  killers' side: at depth 9, drawn at 1000 games, `+313 -308 =379`, S 0.502: no lean at all.
  **The killers stay** (the developer, 2026-10-03: "We are not Stockfish"); no clock duel.

**Decided: the picker with `T = -128` and killers** is Rotor's move order. Hard-coded 2026-10-03
(`GOOD_QUIET_THRESHOLD`), and what was temporary removed: `MoveOrdering`, the three UCI options, the
`+t` names, `Legacy`'s chain with `try_best_history_move` and the `score`/`try_killer`/`try_history`
fields. Quiescence keeps `try_killer`, `try_best_noisy_move` and `move_score`. The fixed-depth test
is re-recorded.

## The duels

Each first at depth 9 (a smoke test: deterministic, cheap, catches a broken picker), then at 10+0.1,
which decides; a close 10+0.1 result may be confirmed at 3+2.

1. `amanda-0001` against `amanda-0001+t0`: the picker against today's order.
2. `amanda-0001+t0` against `amanda-0001+t-128`: the threshold.
3. The winner against its `-nokillers`: the killers.
3. **SEE**: a full `see` compared with the threshold today; an early-exit `see_ge` (TODO.md) is a
   later speed step, not needed for correctness.

## The two quiet thresholds

Stockfish has two, with different jobs:

- **The sort limit, `-3560 * depth`**, is a speed trade, not an ordering rule. `partial_insertion_sort`
  sorts only the quiets scoring at or above it; the rest stay unsorted, in generation order, at the
  tail. At shallow depth, late-move pruning cuts most quiets before they are reached, so sorting them
  is wasted work; deeper, more are reached, so more are sorted. It changes the order only among quiets
  with very bad history.
- **The good-quiet threshold, `-14000`**, is an ordering rule: it decides whether a quiet with very bad
  history is tried before or after the losing captures. Above it, quiets come before the bad captures;
  at or below it, after.

**Their unit.** A history entry has no physical unit; it counts bonus points. Rotor adds `d * d`
points (`d` the remaining depth) when a quiet move causes a cutoff, and subtracts the same from each
quiet searched before it. Gravity (`apply_history_bonus`) makes the entry converge, for a move that
causes a cutoff a fraction `p` of the times it is searched, to `max * (2p - 1)`. So `entry / max` is
the move's **net cutoff rate**: `+1` always cuts, `-1` is always beaten, `0` is even or unknown,
weighted toward deeper and more recent events. Rotor's `combined_history_score` adds the flat entry
(max 8192) and the continuation entry (max 32768) raw, so continuation history weighs four times as
much. A threshold on the sum is a threshold on a weighted net cutoff rate. If every table agreed on a
rate `r`, Stockfish's `-14000` would be `r = -14000 / 180750 = -0.077`, and its sort limit
`r = -0.020 * depth`. The range-ratio scaling keeps those `r`s in Rotor. But the tables do not agree,
and a never-updated entry reads `0`: Stockfish has five continuation terms, many of them often empty,
so its real sums are smaller than its range suggests.

What decides their values is not the score's range but **where real scores fall**: what fraction of a
node's quiets sit below each threshold. Histories rarely reach their bounds, so the range ratio is only
a guess. To understand them:

1. Measure the distribution of `combined_history_score` at `QUIET_INIT` in Rotor's search (a debug
   counter, histogram per depth, on `nnue_bench --depth 9`), and how many quiets late-move pruning
   leaves unreached per depth.
2. Choose the sort limit from it, or remove it: sorting every quiet (`i32::MIN`) gives up only speed,
   and is one unknown fewer for the first match.
3. Choose the good-quiet threshold from it, so that it marks a small tail of clearly bad quiets.
   Today's search has an analogue: the history stage takes only scores `> 0`, and a capture with
   SEE `<= 0` is ordered with the quiets.

**Measured** (history-distribution.md, amanda's generation 1, 49 positions, depth 9): Rotor's scores
run from about `-640` to `+4400`; 54% to 58% of quiets read exactly `0` (never updated) and are the
worst of all; losing captures cut more often than every quiet scoring `<= 0` from depth 3 on.

- **Sort limit: none.** About 28 quiets per node, cheap to sort whole; a depth-scaled limit is a later
  speed step, measured on its own.
- **Good-quiet threshold, two candidates for the duel:**
  - **`T = 0`**: quiets `> 0` before the losing captures, quiets `<= 0` (about 90%) after them. What the
    measurement points to from depth 2 on. Not Stockfish's shape: most quiets are "bad".
  - **`T = -128`**: only the bottom 6% after the losing captures, Stockfish's "small tail" shape.

**Late-move pruning must skip quiets, not break the loop.** Today's `break` relies on every later move
being quiet. With bad captures after the good quiets, a `break` in `GOOD_QUIET` would also drop the
losing captures, which the measurement shows still cut. Stockfish sets `skipQuiets` and goes on to
`BAD_CAPTURE`; Rotor's picker does the same.

## Implementation

- A `MovePicker` per node over `self.moves[start..end]` with a parallel `scores` buffer (same stack
  discipline: pushed per node, truncated with it), Stockfish's stages as an enum, and `next() ->
  Option<Move>`. Rotor generates every move at once (`pseudo_legal_moves`), so `CAPTURE_INIT`
  partitions the noisy moves to the front and scores them, `QUIET_INIT` scores the rest; bad captures
  are swapped into a region at the front as Stockfish does (`endBadCaptures`).
- `partial_insertion_sort` and `select` as Stockfish's, with the TT move skipped in `select`.
- `alpha_beta_with_buffer` and `quiescence_from` loop on `picker.next()` instead of the stage chain.
  Everything after the pick (legality, futility, late-move and history pruning, LMR, the history
  updates, `quiets_searched`) is unchanged, except late-move pruning: instead of `break`, it tells the
  picker to skip the remaining quiets (`skip_quiets`), so the bad captures are still searched.
- `try_best_noisy_move`, `try_killer`, `try_best_history_move`, and `Search::score`/`try_killer`/
  `try_history` (the swappable fn fields) go. The killer table and its updates stay until the killer
  match decides.

## Tests and measurement

- Unit tests on the picker: the stage order on hand-built positions (TT move first and never twice,
  good captures by score, a losing capture after the good quiets, quiets by history above and below
  the thresholds, evasions in check, quiescence captures only).
- The fixed-depth test (`a_fixed_depth_search_reproduces_its_recorded_nodes_scores_and_moves`)
  changes by design: re-recorded in the same commit, its old and new node counts in the message.
- `search_bench` and `nnue_bench --depth 9`: nodes per second and nodes to depth, before and after.
- **Duels decide**: first the picker against the engine before the change, then `T = 0` against
  `T = -128`, then killers kept against dropped, each with `rotor-arena duel` at a fixed depth and
  under a clock. Nodes per second alone cannot say, since the tree changes.
