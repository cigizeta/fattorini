# TT prefetch (A3 of search-optimizations.md)

**Status:** plan, drafted and implemented 2026-10-07 while betty's duel against hand-crafted ran
on the Lichess machine; tested and committed the same day, node counts unchanged; done. Timed only together
with three other tree-identical changes of the day (TODO.md). Off the label path: no search decision changes, so node
counts must come out identical.

## Why

A TT probe reads one 16-byte slot at an index set by the hash's low bits: at any `Hash` larger
than the caches, a cold miss to memory, roughly 80-100 ns, paid at the top of every child node.
The child's hash is known as soon as the move is made, and before the child probes, the parent
still updates the network's accumulators (a few dozen ns) and the child checks repetition and the
clock. Stockfish prefetches the child's TT entry in `do_move` for the same reason.

## Design

1. `TranspositionTable::prefetch(hash)`: `_mm_prefetch` with `_MM_HINT_T0` on the slot `probe`
   would read, under `#[cfg(target_arch = "x86_64")]`; a no-op elsewhere. A hint only: no load,
   no store, nothing a probe can observe.
2. Called right after `make_move` in `alpha_beta_with_buffer`'s and `quiescence`'s move loops,
   before `update_accumulators`, with the child's hash.

## Layout: no change (discussed 2026-10-07)

A slot is 16 bytes, four to a 64-byte line, so a probe never straddles two lines: nothing to fix
for speed. Buckets of several entries (Stockfish's three per 32 bytes) change what the table keeps,
by replacing the least valuable entry instead of the one indexed: a strength question, decided by
a duel, so it moved from A4 to the B list.

## Verification

- `cargo test --release --lib`.
- Node counts, `nnue_bench --search-only` at depth 9: caroline 1,697,834, hand-crafted 3,183,476.
- Speed on idle the Lichess machine, before and after, with `nnue_bench --search-only`: its table is 64 MB,
  four times the Lichess machine's 16 MB L3, so probes do miss there as in a game.
