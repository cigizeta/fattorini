# Singular extensions (B2 of search-optimizations.md)

**Status:** shelved 2026-10-07, the code reverted. The second duel stopped at 450 games:
135 - 109 - 206, 0.529, +20.1 +/- 23.6 Elo, LOS 95%, after leading at 0.595 at 100 games; about 0.565 over
the remaining 550 would have been needed to clear 0.549 at 1000. Likely a small real gain, which
the duel rule cannot confirm; a later design (the +2/+3 extensions, a cap on extensions per line,
or a network evaluation, whose TT moves are less often far ahead) would be a new experiment.

History: implemented 2026-10-07. First duel (minimum remaining 4, margin 1 cp a ply): even,
66 - 65 - 108 after 239 games, 0.502, stopped. At depth 9 over the 49 bench positions it took
3,813,752 hand-crafted nodes against B1's 3,140,380; a margin of 3 cp a ply changed that by under
1%, a minimum of 6 plies to 3,754,663 and 1.20 s (B1 about 0.96 s): the cost is the extensions,
not the singular searches. Second duel, minimum remaining 6 (`SINGULAR_MIN_REMAINING`), margin 1
(`SINGULAR_MARGIN_PER_PLY`): running; shelved if it does not clear the duel rule by 1000 games.
One duel per design (developer decision, 2026-10-07): no rerun of the same engines and no raised
cap after the fact, since either would only chase luck. On the label path: it changes the tree, so it
is judged by a duel under a clock (cutechess-cli, as B1), and lands before generation 2's
labelling.

## What it is

When the TT says one move is good, search the node again without it, at reduced depth and a
window a little below the TT score. If every other move fails low, the TT move is singular, the
only good move, and is searched a ply deeper. If another move also beats a beta, the node fails
high at once (multi-cut). If the TT move is shown not to be special, it is searched a ply
shallower.

## Stockfish 16.1 (fetched 2026-10-07, `search.cpp`)

- Condition: `!rootNode && move == ttMove && !excludedMove && depth >= 4 (+ ttPv) &&
  abs(ttValue) < VALUE_TB_WIN_IN_MAX_PLY && (tte->bound() & BOUND_LOWER) &&
  tte->depth() >= depth - 3`.
- `singularBeta = ttValue - (60 + 54 * (ttPv && !PvNode)) * depth / 64`,
  `singularDepth = newDepth / 2`; the node is searched with `excludedMove = move` on the null
  window `(singularBeta - 1, singularBeta)`.
- `value < singularBeta`: extension 1; at non-PV nodes 2 or 3, capped per line.
- else `singularBeta >= beta`: return `singularBeta` (multi-cut).
- else `ttValue >= beta`: extension `-2 - !PvNode`; else `cutNode`: -2; else `ttValue <= value`:
  -1.
- While a move is excluded: no TT cutoff, no TT store, no null move, the static evaluation reused;
  with no other legal move the node returns `alpha`.

## Design (decided 2026-10-07)

1. **The core and the negative extensions** (developer decision): +1 when singular, multi-cut,
   and -1 when `tt_score >= beta` or `tt_score <= value`. Not the +2/+3 extensions (they need
   PV-ness and a per-line cap) nor the cut-node rule (Rotor has no cut nodes); each a later item
   with its own duel.
2. **Condition**, on the node's `remaining` after IIR: not the root, the move is the TT move, no
   move already excluded at this ply, `remaining >= 4`, the entry a lower or exact bound at least
   `remaining - 3` deep, its score neither a mate nor a tablebase score.
3. **Magnitudes in Rotor's centipawns**: `singular_beta = tt_score - remaining` (1 cp a ply,
   between Stockfish's two margins of about 0.45 and 0.85 of Rotor's cp a ply); the singular
   search at `(remaining - 1) / 2` plies, as `newDepth / 2`.
4. **The exclusion**: `Search::excluded_moves`, one `Option<Move>` per ply. The singular search
   recurses at the same ply with the TT move excluded; that node skips the excluded move in its
   loop, takes no TT cutoff, makes no TT store, does no null move, and returns `alpha` if no other
   move is legal (rather than a mate or stalemate score).
5. **With the check extension**: the move's extension is the larger of the two, so a singular
   check gets +1, not +2, and a checking move is never shortened by a negative extension. Rotor's
   check extension has no cap, and adding a second +1 on top would let lines grow unbounded.
6. Picked by Stockfish's shapes, not tuned, as every search parameter.

## Tests

- A singular TT move is searched one ply deeper (a position with one forced move).
- The excluded node: it never returns the excluded move's score, takes no TT cutoff, stores
  nothing, and returns `alpha` when the excluded move is the only legal one.
- Multi-cut returns `singular_beta` when two moves beat it.
- Node counts change; recorded as the new references.

## Verification

A duel at 10+0.1 through cutechess-cli, against `~/rotor-b1.exe` (the committed search), both
hand-crafted, stopped by Rotor's duel rule (0.5 + 3.09 * 0.5 / sqrt(N)) or at 1000 games.
