# One evaluation per node

Implementation plan. Branch: `alessio`. Motivated by the big five: the PeSTO big five, the
bootstrap's own evaluation at 392 ns a call (14x the hand-crafted one), lost to the bootstrap by
-88.7 +/- 38.8 Elo at 10+0.1 on speed alone (`TODO.md`).

## The problem

In `search.rs`, a node not in check calls `self.evaluate(board, ply)` up to three times, each on
the same board at the same ply, so each returns the same number:

1. **`static_eval`** in `alpha_beta_with_buffer`, when `remaining <= FUTILITY_MAX_DEPTH`, for
   reverse futility and classical futility (the latter also at the root). Computed before the TT
   probe, so a TT cutoff pays for it and discards it.
2. **`eval`**, a few lines later, when `ply > 0`, at any depth, for razoring, null-move pruning,
   and `static_evals[ply]` (`is_improving`).
3. **Quiescence's stand pat.** A leaf (`ply >= target_depth`) and a razoring cutoff both hand the
   same board at the same ply to `quiescence`, which evaluates it again.

A leaf, the most common node, pays all three: about 1.2 us with the big five.

## Design

Settled in discussion.

- **One `eval: Option<i32>` per node**, computed right after the TT probe and its cutoffs (before
  IIR), `Some(self.evaluate(board, ply))` when `!in_check && (ply > 0 || remaining <=
  FUTILITY_MAX_DEPTH)`: the union of the two current conditions, the second disjunct being the
  root's classical futility. `remaining` here is before the IIR reduction, as the current
  `static_eval` gate reads it.
- **Each consumer keeps its own gate and reads `eval`:**
  - reverse futility and classical futility: `eval` only where `remaining <= FUTILITY_MAX_DEPTH`
    held (the current `static_eval`), so they behave as now;
  - razoring, null-move pruning, `static_evals[ply]`: where `ply > 0` (the current `eval`).
  The simplest faithful form keeps two names, `static_eval` (filtered from `eval` by the
  futility gate) and `eval`, so every consumer's code is unchanged.
- **`quiescence` takes the stand pat.** A new parameter `stand_pat: Option<i32>`, a value the
  caller already computed for this same board and ply. The leaf call and the razoring call pass
  `eval`; quiescence's own recursive calls pass `None`, and the child evaluates itself. Quiescence
  evaluates only when it is not in check and was given `None`. Its TT probe stays ahead of the
  stand pat, so a cutoff there still skips it.
- **No behavioral change.** Every pruning decision reads the same numbers in the same order; a
  node only skips evaluations whose results were identical or discarded. The psq accumulator path
  (`evaluate`'s `ply` argument) is unaffected: the reused value comes from the same ply.

## Steps

1. `quiescence(board, ply, alpha, beta, prev_to_sq, stand_pat)`: use the given stand pat, or
   evaluate when `None` and not in check. Update every caller, tests included (`None`).
2. `alpha_beta_with_buffer`: move the evaluation after the TT probe, merge the two into one call,
   derive `static_eval` from it, and pass `eval` to the two `quiescence` calls on this node's board.
3. Update the comments that explain the separate evaluations (the razoring and null-move doc
   comments say why `eval` is computed apart from `static_eval`).

## Tests

- **Same search.** On a fixed set of positions (the bench positions), a fixed-depth search before
  and after gives identical node counts, best moves, and scores. Recorded as a test with the
  current counts, taken before the change, so the change must reproduce them.
- **Fewer evaluations.** An evaluation counter (test-only) shows the calls per node drop.
- The existing search and quiescence tests pass unchanged, apart from the new argument.

## Verification

```
cargo test --release
```

Then `nnue_bench` at a fixed depth, with the hand-crafted evaluation and with `--big-five`:
identical node counts, lower time. No Elo match is needed, since the search does not change.

## As implemented

- `quiescence` keeps its signature, as a wrapper over a new `quiescence_from(..., evaluation)`, so
  the twenty-odd test calls stay as they are; the two calls from `alpha_beta_with_buffer` use
  `quiescence_from`.
- The node's evaluation is `node_eval`; `static_eval` (futility) and `eval` (below the root) are
  both derived from it, so every consumer reads the same variable as before.
- **Same search** is `a_fixed_depth_search_reproduces_its_recorded_nodes_scores_and_moves`:
  six of the bench positions, depth 6, recorded on the code before the change.
- **No evaluation counter.** Tests build `Search` as struct literals in several places, so a
  test-only field would touch them all; the benchmark's timing shows the saving instead.

## Out of scope

- **The leaf's move loop.** A leaf also generates its moves and plays them for legality before
  handing off to quiescence. Removing that changes how mate and stalemate are found at the
  horizon: a separate design (`TODO.md`).
- Caching evaluations across nodes (an evaluation cache or TT-stored static eval).
