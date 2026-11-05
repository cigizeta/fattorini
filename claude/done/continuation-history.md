# Continuation History (1-ply / Countermove History)

**Done:** 1-ply continuation (countermove) history is implemented in `search.rs` and feeds the staged move picker (move-picker.md); deeper look-back (2-ply and beyond) was never built. The "not yet confirmed" status below is stale.

Resume with: "resume from continuation history."

## Status

Implemented in `search.rs`. Not yet confirmed against `cargo test` -- that's the next step.

## What it is

Generalizes the existing flat history table (`self.history: [[i32; 64]; 12]`, `[piece][to]`) by
one extra dimension of context: indexed by both the current move and the move played 1 ply ago.
Since sides strictly alternate, that 1-ply-back move was necessarily the opponent's, so this is
specifically **countermove history**: "given the opponent just played that, what's my best
typical reply" -- sharper than the flat table's context-free average.

Distinct from killers (`self.killers`, indexed by ply only, no move context) and from deeper
look-back distances (2-ply/follow-up history and beyond -- not built; see "Not in scope").

## Design

- **State**: `self.prev_move: [Option<(Piece, Square)>; MAX_PLY]`, overwritten in place per ply
  (same shape as `self.killers`). Set right before each recursive call in the move loop; read at
  the start of a node as `self.prev_move[ply]`.
- **Table**: `continuation_history[prev_piece][prev_to][piece][to]`, flattened as a `Vec<i32>`
  (via `continuation_history_index`, not a nested array -- avoids a ~2.25 MB copy every
  iterative-deepening iteration), persisted across iterations alongside `history`.
- **Trigger/magnitude**: identical to flat history's own bonus/malus (`(target_depth - ply)^2`),
  applied to both tables from the same event, gated on `ply > 0` (a previous move must exist to
  condition on -- nothing updates at the root).
- **Ordering**: `history[piece][to] + continuation_history[prev_piece][prev_to][piece][to]`,
  plain unweighted sum (`combined_history_score`), used only by `try_best_history_move`.
  `history_score`/`is_history_pruned` are untouched.
- **Scope**: main search only, not quiescence. Ordering only, not folded into
  `is_history_pruned`/history pruning.
- **History gravity**: every write goes through `apply_history_bonus(entry, bonus, max)`
  (`entry += bonus - entry * abs(bonus) / D`-style decay, `bonus` clamped to `+-max` first, `i64`
  intermediate for the multiply). `FLAT_HISTORY_MAX = 8192`, `CONTINUATION_HISTORY_MAX = 32768`
  (4x, mirroring Stockfish's own asymmetric ceilings without copying its exact numbers).

## Not in scope

- 2-ply+ (follow-up history) or deeper look-back distances.
- Folding into history pruning.
- Quiescence.
- Whether this measurably helps -- blocked on the self-play/SPRT harness (`TODO.md`).
