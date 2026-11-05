# Zobrist hashing

**Done:** incremental Zobrist hashing is implemented in `board/zobrist.rs`; repetition detection and the transposition table (tt.md) build on it.

## Status

Implemented: `board::zobrist` (`src/board/zobrist.rs`) and wired into `Board`. Both downstream
consumers this document's own sequencing plan called for -- repetition detection (`search.md`)
and the transposition table (`tt.md`, `claude/done/`) -- are also done. This document's own scope
is complete.

## Design

`Board::hash: u64`, maintained incrementally (XORed in/out on every `make_move`/`unmake_move`),
never recomputed from scratch on each use -- the entire performance reason for choosing Zobrist
over exact field-by-field comparison depends on this.

**Random tables**: fixed-seed, compile-time `const` (a small seeded splitmix64-style generator,
no crate needed) -- reproducible run to run and machine to machine, needed for exact-value unit
tests and byte-identical game-log replay.

**What feeds the hash:**
- **Piece placement**: one random value per `(Piece, Square)` pair, `[[u64; 64]; 12]`, indexed
  directly by `piece as usize`/`square as usize`.
- **Side to move**: one value, XORed in whenever it's Black's turn.
- **Castling rights**: one value per individual right (4 total: White/Black x kingside/
  queenside), XORed independently as each flag changes -- not one value per combined 16-state
  pattern, since only "this one flag flipped" is ever needed.
- **En passant square**: one value per square, XORed in whenever `en_passant_square` is `Some`,
  unconditionally -- regardless of whether an en passant capture is actually legal right now.
  Imprecise only in the safe direction (can cause a missed repetition, never a false one).
- **Halfmove clock is deliberately excluded** -- not part of position identity for repetition
  purposes (FIDE's own definition doesn't count it either); tracked separately on `Board` for the
  fifty-move rule.

**Module surface**: `zobrist::compute(board: &Board) -> u64` (from-scratch hash, used to set the
initial value in `from_fen` and to verify incremental correctness in tests); accessor functions
(`piece_square`, `castling`, `en_passant`, `side_to_move`) over the raw tables, rather than
exposing them directly. `place_piece`/`remove_piece` (the existing single sync point every move
type funnels through for keeping `mailbox` correct) XOR piece-placement hashing in/out
automatically for every move kind, no per-move-kind special-casing. `make_move`/`unmake_move` XOR
the whole old castling/en-passant contribution out and the new one in around each field's
existing update, and `side_to_move()` once at the existing side-to-move flip. `pub fn hash(&self)
-> u64` is the public accessor.

## Sequencing (settled, all since completed)

Zobrist hashing -> position history/repetition detection (`search.md`) -> full transposition
table (`tt.md`, `claude/done/`) -- each landed as its own piece, in that order, avoiding building
the hashing piece twice inside a bigger TT effort.
