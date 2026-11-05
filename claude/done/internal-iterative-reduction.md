# Internal Iterative Reduction

**Done:** IIR is implemented in `search.rs` (`iir_reduction`), unit tested; later changes in search-simplification.md.

Resume with: "resume from internal iterative reduction."

## Status

Implemented in `search.rs`, unit tested.

## What it is

When a node has no TT move and clears a minimum depth, reduce that node's own `target_depth` by a
fixed amount before proceeding otherwise normally -- no extra search (unlike Internal Iterative
Deepening, which reruns a whole reduced-depth search just to populate the TT with a move first).

**Node-level, not move-level.** Decided once, before any move at the node is even generated, and
applies uniformly to every move tried there afterward. Contrast with LMR: per-move, only
later-ordered moves get reduced, and a reduced move that beats alpha triggers a full-depth
re-search. IIR has no re-search mechanism -- it expresses lowered confidence in the whole node,
not a bet on any one move. The two compose: IIR trims a node's own effective depth once, and
whatever depth it leaves in place, LMR still applies its own separate per-move reductions on top.

## Design

- Hooks in right where `tt_move` is already computed, in `alpha_beta_with_buffer`.
- `iir_reduction(ply, tt_move, remaining) -> u32`, a small free function matching
  `is_lmr_eligible`/`lmr_reduction`'s own pattern. Condition: no TT move, `remaining >=
  IIR_MIN_REMAINING` (`4`), `ply > 0`. Effect: `target_depth -= IIR_REDUCTION` (`1`), uniform
  across every node type -- no cut-node/PV-node distinction.
- Called right after `tt_move` is computed, shadowing both `target_depth` and `remaining` together
  so razoring/reverse futility/LMR eligibility further down the same function see the reduction
  too, not just recursive children.
- **Root excluded** (`ply > 0`, folded into `iir_reduction` itself). At the root, `target_depth`
  is also what `choose_best_move_with_progress` reports as the completed depth; a silently
  reduced root iteration wouldn't just misreport depth, it wouldn't advance iterative deepening at
  all that iteration. Interior nodes have no such externally-visible promise to break.
- Applies everywhere, including `rotor-curate generate`'s `Search(9)` label-generation calls --
  `generate` shares the same `choose_best_move`/`alpha_beta_with_buffer` as live play, no bypass
  flag.
- Compounding along a cold chain (each unexplored node reduces its own depth by 1 before passing
  depth-2 to its children) is bounded and self-limiting: the minimum-depth gate is re-checked
  fresh at every node, so once compounding drags a node below threshold, the reduction stops
  firing there. This is the intended selectivity, not a side effect to tolerate.

## Known interaction with null-move pruning

`NULL_MOVE_MIN_REMAINING` and `IIR_MIN_REMAINING` both happen to be `4`, so at a node with no TT
move and `remaining == 4`, IIR fires first and shaves `remaining` to `3` -- one below null-move
pruning's own minimum, silently disabling it right at its own boundary. Two existing null-move
unit tests that isolated that exact boundary with a fresh, empty TT broke because of this; fixed
by seeding the TT with a harmless pre-existing shallow move before calling the real search, so IIR
no longer fires in those tests. Real play has both techniques active and stacking this way; the
fix was to the tests' own isolation, not to either technique.

## TT size

Not a live concern at Rotor's real operating sizes. `DEFAULT_HASH_MB = 16` exists only for UCI
spec-compliance; real matches set `Hash` well above that via `setoption`. `rotor-curate
generate`'s own per-worker table (`TT_SIZE_MB = 64`, cleared once per game) measured a peak
`hashfull` of `275/1000` across several hundred real games -- comfortably far from saturating.

## Open, unvalidated (CLAUDE.md rule 5)

- Minimum depth threshold (`4`) is adopted directly from Stockfish's own published value, not
  measured for Rotor's own search shape.
- Whether IIR measurably helps at all -- blocked on the self-play/SPRT harness (`TODO.md`).
