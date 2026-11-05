# Null-move reduction growing with depth and margin (B1 of search-optimizations.md)

**Status:** done 2026-10-07. On the label path: it changes the tree, so it was judged by a duel
under a clock, and lands before generation 2's labelling.

## Result (2026-10-07, the Lichess machine, hand-crafted both sides)

- Node counts at depth 9 (`nnue_bench --search-only`): hand-crafted 3,183,476 to 3,140,380 (-1.4%,
  0.97 s either way); amanda 1,579,395 in 0.83 s, her first reference. Depth 9 barely uses the
  new reduction, which first grows at remaining 5.
- **Duel at 10+0.1 through cutechess-cli** (UHO_Lichess_4852_v1 openings, colours reversed, 8 at
  once): `b1` against today's search, stopped at 250 games when Rotor's duel rule resolved
  (score 0.602 past 0.5 + 3.09 * 0.5 / sqrt(250) = 0.598): +100 -49 =101, **+71.9 +- 33.5 Elo**,
  0.623 as White and 0.581 as Black. Far above the +5 to +15 expected.
- The depth-9 duel of the plan's step 3 is not needed: the margin is not small.

## Today

`NULL_MOVE_REDUCTION = 2`: the null-move search runs 3 plies shallower than the node (R, plus the
null move's own ply), at every depth, from `NULL_MOVE_MIN_REMAINING = 4` up. Guards: not at the
root, not in check, not twice in a row, not against a mate or tablebase bound, not without
non-pawn material (the zugzwang guard), and only when the static evaluation is at least beta.

## What Stockfish does (fetched 2026-10-07, `search.cpp`, master)

`R = 7 + depth / 3 + max((staticEval - beta) / 256, 0)`, the null search at `depth - R`; only at
cut nodes, with an eval margin that shrinks with depth and `improving`, and a verification search
at high depth. Far more aggressive than Rotor's, and tuned together with features Rotor lacks
(node types above all). **Not followed (decided 2026-10-07): Rotor does not prune harder.**

## Design (decided 2026-10-07)

The form Stockfish used around versions 9 and 10, in Rotor's units:

- **Plies the null search loses:** `3 + remaining / 4 + min((eval - beta) / MARGIN_PER_PLY, 3)`,
  `MARGIN_PER_PLY` being 200 cp, one step of Rotor's pruning scale. Today's 3 at shallow depth,
  about 5 at depth 9 and 6 at depth 13 before the eval term, which adds up to 3 more when the
  position already clears beta by a wide margin.
- **Floor:** at most `remaining - 1`, so the null search always keeps at least one ply, as today.
  So remaining 4 loses 3 plies, exactly as today; the reduction first grows at remaining 5.
- **Unchanged:** every guard, `NULL_MOVE_MIN_REMAINING = 4`, the `eval >= beta` pre-filter, the
  mate and tablebase clamp of the result, and no verification search.

One pure function, `null_move_plies(remaining, eval_margin)`, unit-tested on its corners (today's
value at remaining 4, the depth term, the eval term and its cap, the floor).

## Verification

1. `cargo test --release --lib`; `search_bench` and `nnue_bench --search-only` node counts drop
   (expected: fewer nodes at the same depth), recorded as the new references.
2. **A duel at 10+0.1, the new search against today's**, both with hand-crafted, through
   cutechess-cli (two binaries, as the int8 duel): today's `rotor.exe` copied aside before the
   build. Kept only if it is not shown weaker; a win is the hope.
3. If the margin is small: a depth-9 duel, to see whether the labels' depth-9 quality dropped.
