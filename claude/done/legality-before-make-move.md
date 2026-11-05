# Legality before `make_move` (A1 of search-optimizations.md)

**Status:** plan, drafted 2026-10-07 while betty's duel against hand-crafted ran on the Lichess machine;
implemented, tested and committed the same day; done. Timed only together with three other
tree-identical changes of the day (TODO.md): hand-crafted +10% nodes/s. As built: `movegen::legal::Legality`
(`new`, `is_legal`); `between` and `line` computed from the PEXT slider lookups on the fly, not
tables (a few lookups each); `perft` counts last-ply moves without making them. The search tests'
reference negamax keeps the plain make-and-check path, as an independent oracle. Node counts
unchanged (`nnue_bench --search-only`, depth 9): caroline 1,697,834, hand-crafted 3,183,476, the
latter equal to HEAD's in a separate worktree (the DTZ commit had moved it from 3,183,462). Off the label path by construction: the tree must not change, so `search_bench`'s
and `nnue_bench --search-only`'s node counts must come out identical.

## Today

Every pseudo-legal move a node's picker hands out pays `scratch.make_move`, then
`is_in_check(&scratch, color)` (a king lookup and a full attack test), then `unmake_move`:

- `alpha_beta_with_buffer`'s loop (`search.rs`): make, check, and only then the four pruning
  decisions (futility, late-move, history, SEE), all of which read `board` and `mv` only, never
  `scratch`. So a move pruned there paid make, check and unmake for nothing.
- `quiescence`'s loop: SEE pruning already comes first; then make, check, unmake.
- `has_legal_move` (the leaf's mate and stalemate test), `movegen::legal::legal_moves` and
  `perft`: make, check, unmake per move.

The last profile (2026-10-03) put make/unmake and legality at about 9% of a search, before the
networks; a network's search spends more per node elsewhere, so the share is smaller there.

## What Stockfish does

`Position::legal(move)` on the position before the move, with the pinned pieces
(`blockers_for_king`) computed once per position: a non-king move is legal unless its piece is
pinned and leaves the line through its king; a king move is legal unless its destination is
attacked with the king itself removed from the occupancy; en passant, rare, is tested by
recomputing the slider attacks on the king after both pawns move. Check evasions come from a
separate generator, so every non-king evasion already blocks or captures the single checker.

## Design

1. **Two tables in `movegen`:** `between(a, b)`, the squares strictly between two squares on a
   line (empty if not aligned), and `line(a, b)`, the whole line through both (empty if not
   aligned). 64 x 64 bitboards each, 64 KB together, built once (a `OnceLock`, or `const fn` if
   that stays readable), from `sliding.rs`'s ray attacks.
2. **`Pins`, once per node:** the side to move's king square, its pinned pieces (own pieces alone
   between the king and an enemy slider aligned with it: `between` against the occupancy), and
   the checkers (`attackers_to(king) & enemy`). Computed only when the node reaches its move loop
   (after the TT cutoff, null move, razoring and reverse futility), so cut nodes do not pay it.
3. **`is_legal(board, mv, pins) -> bool`:**
   - castling: true (the generator already checks every square of the king's path);
   - other king moves: no enemy attacker of `to` with the king removed from the occupancy
     (`attackers_to(board, to, occupancy ^ king)`, the captured piece on `to`, if any, excluded);
   - en passant: make, `is_in_check`, unmake, as today (rare; exactness first);
   - two checkers: false;
   - one checker: `to` captures it or lands in `between(king, checker)`, and the piece is not
     pinned or stays on `line(king, from)`;
   - no checker: the piece is not pinned, or `to` is on `line(king, from)`.
4. **The search loop, reordered:** legality first (`is_legal`), then the four pruning decisions,
   then `make_move` only for a move actually searched, and `unmake_move` after its search.
   `found_legal_move`, `moves_tried`, `late_move_pruned` and `searched_quiet_move` keep their
   meaning: they count and mark legal moves, as today, since legality is still known before the
   pruning. Quiescence, `has_legal_move`, `legal_moves` and `perft` use `is_legal` too.
5. **No change to move generation:** still pseudo-legal; no evasion generator.

## Tests

1. **The oracle:** for every pseudo-legal move of many positions (Stockfish's 49 bench positions,
   perft's suites, and random games from the start including checks, pins, en passant and
   castling), `is_legal` equals make, `!is_in_check`, unmake. Positions chosen to include an en
   passant capture exposing the king along the rank, a pinned piece moving along and off its pin,
   a king stepping back along a checking ray, and a double check.
2. **perft** counts, unchanged, through the new `legal_moves`.
3. **Node counts:** `search_bench` (hand-crafted) and `nnue_bench --search-only` (caroline and
   betty), identical to the last digit.

## Measurement

On idle the Lichess machine, plugged in: `search_bench` time and `nnue_bench --search-only` time, before and
after. Expected: a few percent nodes/s, more for hand-crafted (cheaper nodes, so the saving is a
larger share).

## Open questions

None left. In check: the exact evasion rule of design 3, not the old make-and-check path
(decided 2026-10-07).
