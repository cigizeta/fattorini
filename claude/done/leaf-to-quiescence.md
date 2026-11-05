# Leaf straight to quiescence

Implementation plan. Branch: `alessio`. Follows `one-evaluation-per-node.md`: with a trained network
every node's cost matters, and a hand-crafted node already costs 1.87 us of which its evaluation is
20 ns, so the rest of the node is the target.

## The problem

A leaf (`ply >= target_depth` in `alpha_beta_with_buffer`) that survives razoring and reverse
futility (`alpha <= eval < beta`, or in check) runs the whole move loop: it generates every
pseudo-legal move, and for each runs the ordering chain (TT move, best noisy, killers, history: a
selection scan over the remaining moves), makes it, tests for check, updates the accumulators, and
unmakes it. Everything else in the loop is gated on `ply < target_depth`: the alpha update, the
recursion, pruning, killer and history updates. The loop's one result is `found_legal_move`. After
it the leaf returns mate or stalemate if there was no legal move, applies the fifty-move rule, and
calls `quiescence`, which generates its own moves.

## Design

Settled in discussion.

- **Hand the leaf to quiescence early.** In `alpha_beta_with_buffer`, right after `node_eval` (so
  the TT probe and its cutoffs, and the earlier draw checks, have run), when `ply >= target_depth`:
  1. not in check: if no legal move exists, return 0 (stalemate). Checked by generating the
     pseudo-legal moves and stopping at the first legal one, usually the first tried;
  2. `return self.quiescence_from(board, ply, alpha, beta, None, node_eval)`.
- **Mate and the fifty-move rule are quiescence's.** In check, quiescence searches every evasion,
  scores none as `-(MATE_SCORE - ply)`, and puts mate ahead of the fifty-move rule, as the leaf
  does now. Not in check, quiescence applies the fifty-move rule first, as the leaf would after
  finding a legal move.
- **Stalemate stays found.** Quiescence does not detect it (its doc comment accepts the gap), so
  the leaf keeps the check, at the cost of one move generation and about one make/unmake.
- **No behavioral change.** Razoring at a leaf already hands off to quiescence. Reverse futility at
  a leaf (`eval >= beta`) returns `eval`, which is what quiescence's stand-pat cutoff returns, its
  TT probe and draw checks having already hit or missed at the leaf. The remaining case produces
  the same mate, stalemate, and fifty-move results. The loop changes no state but
  `found_legal_move` and counts no nodes, so node counts stay too.

## Steps

1. A legal-move-exists test in `search.rs` (or `movegen`): pseudo-legal moves into `self.moves`,
   make and test each until one is legal, truncate. Reuses the buffer, so no allocation.
2. The early hand-off in `alpha_beta_with_buffer`, with a comment on why mate and the fifty-move
   rule need nothing here.
3. Update the comments that describe the leaf going through the loop: the `alpha >= beta` and
   killer gates (`ply < target_depth`), the `found_legal_move` and fifty-move block before the
   leaf's `quiescence` call, and `seldepth`'s doc comment.
4. Remove the code the hand-off makes unreachable: the `ply >= target_depth` call to quiescence
   after the loop, and any `ply < target_depth` gate left always true.

## As implemented

- **The hand-off sits where the leaf's loop began**, after razoring, reverse futility, and null-move
  pruning, not right after `node_eval`. Earlier, a leaf that reverse futility now cuts off would
  enter quiescence (one more node), and a stalemated leaf with `eval >= beta` would score 0 where
  it now scores `eval`: either would change the search.
- **The legal-move test runs in check too.** A mated leaf now returns mate without entering
  quiescence; letting quiescence find it would count one more node. The leaf then decides, in the
  interior nodes' order: no legal move (mate or stalemate), the fifty-move rule, quiescence.
- `has_legal_move` is a `Search` method next to `evaluate`.
- The loop's outer `if ply < target_depth` block, now always true, stays, with a comment, rather
  than re-indenting the 220 lines inside it; the cutoff's `ply < target_depth` gate and the old
  post-loop leaf branch are removed.
- Tests: `a_stalemated_leaf_scores_a_draw_not_its_material` and `a_mated_leaf_scores_mate_at_its_ply`.

## Tests

- **Same search.** `a_fixed_depth_search_reproduces_its_recorded_nodes_scores_and_moves`, unchanged.
  If it fails, the design missed something: find what, do not re-record.
- **Stalemate at the horizon.** A position where the side to move at the leaf is stalemated, with
  material unbalanced: a depth-1 search scores it 0, not the evaluation.
- **Mate at the horizon.** A depth-1 search into a mate scores it as mate at that ply.
- The existing search tests pass.

## Verification

```
cargo test --release
```

Then `nnue_bench --depth 9`, with and without `--big-five`: identical node counts, lower time per
node.

## Out of scope

- Dropping the stalemate check, as quiescence and Stockfish do: slightly faster, but a behavioral
  change needing a match.
- The cost of the ordering chain at interior nodes.
