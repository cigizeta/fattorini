# The accumulator update read from the move

**Status:** designed and approved 2026-11-19 (the developer), finding 2 of speed-profile.md;
written the same day, `cargo test` and clippy passed. **Exact, and no faster:** the bench at depth
16 counted 27,589,863 nodes before and after; back to back on one idle machine, the old build
2,595,889 and 2,584,827 nodes/s, the new 2,596,781 and 2,316,075 (an outlier, 11% below every other
run). The profile's 27 ns in `between` were mostly waits on memory, which reading the move waits on
too. Kept or reverted: the developer's decision, pending.

## Why

`QuantizedPsqNetwork::update` finds the changed rows by diffing the parent and child boards
(`ChangedRows::between`, `between_halfk`): twelve piece bitboards XORed, every changed square
walked. Chosen for simplicity (castling, en passant, promotion and null moves need no case), it
costs 27 ns a node of the network's 437, more than the SIMD update it feeds (21 ns).

## The design (approved)

1. **`update_with_move(parent_acc, child_acc, parent, child, mv: Option<Move>)`** beside `update`;
   the search calls it, `None` for a null move (no piece row changes, only the en passant file).
   `update` keeps diffing, for its sixteen test and bench callers, and is the new path's oracle.
2. **`PieceChanges`**: at most four `(piece, square, added)` read from the move and the parent: the
   captured piece off `to` (or the en passant square), the moving piece off `from`, the placed piece
   (its promotion if any) onto `to`, and for castling the rook off and on. Mapped to rows by psq's
   `piece_row_for` or halfk's `halfk::piece_row`, both perspectives. Captured first, so a quiet move
   is one row added and one removed, a capture one and two, `update_i16_avx2`'s unrolled cases. The
   update wraps, so the order of rows never changes the result.
3. **The castling and en passant rows** still compared directly, only when either changed; psq's
   part of `between` split into `push_psq_state_rows`, shared by both paths; halfk's unchanged.
4. **`castling_rook_squares(color, king_to)`** in `make_move.rs`, `pub(crate)`, from the same
   `castling_side_of` that moves the rook in `make_move`: one source for the rook's squares.
5. **Halfk too:** a king changing bucket still refreshes its perspective; the other updates from the
   move's changes.

## Judged by

- **Exactness:** a test, every legal move and every reply of positions with every kind (castling
  both ways, en passant, promotions plain and capturing): the rows from the move equal `between`'s
  and `between_halfk`'s, as sets, per perspective, for psq and for 8-bucket and 32-bucket halfk.
- **The same search:** `nnue_bench --network lc0-psq-threats-screlu-g1-a1 --search-only --depth 16`
  must still count 27,589,863 nodes; any other count is a bug.
- **The speed:** that bench, the same idle machine, against 2.29M nodes/s; the profile predicts up to
  about 6%. No duel: the moves searched are identical.

## Plan

1. `make_move.rs`: `castling_rook_squares`.
2. `nnue.rs`: `PieceChanges`, `ChangedRows::of_pieces` and `of_pieces_halfk`, `push_psq_state_rows`,
   `update_with_move` over a shared private `update_from`; the test.
3. `search.rs`: `update_accumulators` takes the move, its three callers pass it (`None` for the null
   move).
4. The developer: `cargo test`, clippy, the bench's node count and speed.
