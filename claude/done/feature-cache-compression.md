# Feature cache compression

**Done:** the canonical, zstd-compressed one-record-per-position cache is implemented in `train/cache.rs`; the format is now version 4, so the byte layout below is historical and the code is the reference (see rotor-naive.md for the castling/en passant byte).

## Status

Implemented and confirmed working: subsequent `rotor-train` work (`rotor-halfkav2hm.md`,
`rotor-naive.md`) builds directly on this format and passes `cargo test --bin rotor-train
--features training`.

Extended since by `rotor-naive.md`: `FORMAT_VERSION` bumped `2 -> 3`, one extra byte added to the
record (castling rights + en passant file) -- see that document for the current exact byte
layout. What follows describes the format this document itself settled.

## Design

One canonical, zstd-compressed snapshot per position, not two padded, duplicated perspectives.
`cache.rs`'s on-disk record:

```
1 byte    side_to_move
1 byte    piece count (real, no padding)
2*count   pieces, packed via pack_canonical_piece(square, piece_type, color) -- absolute square
                                           (0..64), absolute type+color identity (0..10), never
                                           mirrored
1 byte    white_king (absolute square)
1 byte    black_king (absolute square)
4 bytes   label (f32)
```

`= 8 + 2*count` bytes, variable, versus the old fixed `130`. The whole file is written/read
through `zstd::Encoder`/`Decoder` (`COMPRESSION_LEVEL = 3`, zstd's own default, not measured
against the real corpus).

`MAX_PIECES_PER_PERSPECTIVE` renamed `MAX_NON_KING_PIECES` -- one list now, not one per
perspective.

## Where perspective derivation lives

- `extract_canonical(board) -> CanonicalPosition`: the only place `rotor-train` walks a `Board`'s
  bitboards for training features.
- `expand(canonical) -> PositionFeatures`: reconstructs both perspectives from one canonical
  snapshot, called once per record by `batch.rs`'s `to_dense_batch`.

`accumulate_perspective` (the crossed-row math) is unaffected -- it operates on already-oriented
`PositionFeatures`, only what feeds it changed.

## Reader/writer surface

`reader.rs`'s `CacheFile`: `read_batch_cyclic(cursor, batch_size)` (training, wraps) and
`read_range(cursor, count)` (held-out test pass, one ordered sweep, rejects a span past
`row_count`). No arbitrary-row-index random access -- `cursor` means "how many records consumed
so far"; reopening/seeking only happens on resume or wraparound, never during normal steady-state
training.

## Touched files

`features.rs`, `cache.rs`, `builder.rs`, `batch.rs`, `reader.rs`, `window.rs`. `Cargo.toml`:
`zstd = { version = "0.13", optional = true }`, gated behind the `training` feature alongside
`tch`.
