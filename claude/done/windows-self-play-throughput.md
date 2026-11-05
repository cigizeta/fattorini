# Self-play throughput on Windows

**Status:** done 2026-10-05: Windows about 2,500 positions/s (from 1,700), WSL 2,150 (from
2,000); details below. Design agreed 2026-10-05. Steps 1 (mmap) and 4 (probe without FEN) applied and
measured together: about 2,405 positions/s, unchanged, as expected (step 1 is the experiment
itself; probes are rare). Step 2 (`PvTable`, `Search::quiets`) applied: `search_bench` depth 9
gives the same 3,183,462 nodes, in 1,104 ms against 1,166 (single thread, no `mimalloc`); self-play
with `mimalloc`, five segments, 2,503 to 2,521 positions/s. Without it, three segments, 2,208 to
2,229: 12% lower, so step 3 keeps `mimalloc`, now permanent in `rotor-arena` (version 0.1.52),
the measurement in its comment in Cargo.toml. The engine `rotor` keeps the system allocator.
Committed. Verification on WSL: `search_bench` 3,183,462 nodes in 1,692 ms against 1,752; but
self-play 1,832 and 1,844 positions/s against 1,988 and 1,908 at the commit before, in the same
session: about 6% slower. Candidate, applied, to be measured on WSL: `mimalloc` on Windows only. If
WSL is still slower with it, the memory maps are next to suspect.

Measured on WSL 2026-10-05, both clones at 7e8d31c, one segment each, in this order: 3f9e218 (before
this work) 2,074 positions/s; 7e8d31c 1,939; 7e8d31c with `Tablebase::new()` instead of the maps
(temporary, uncommitted) 2,097. So `mimalloc` was not the cause, and the maps are: they help Windows
and cost Linux about 7%. Step 5 below, applied and verified: Windows 2,493 positions/s (one whole
segment, unchanged), WSL 2,150, above the 2,074 before this work. `cargo test` passes on both.
Done.

Earlier status: design, to be discussed. Measured on the Lichess machine on 2026-10-05, right after its move from
WSL to Windows (windows-native.md). Two temporary experiments are in the Lichess machine's working tree,
uncommitted, each marked "TEMPORARY EXPERIMENT": `mimalloc` as `rotor-arena`'s global allocator,
and `shakmaty-syzygy`'s `mmap` feature. This design replaces both with their permanent form.

## The problem

Self-play on the Lichess machine (`rotor-arena self hand-crafted`, depth 9, 16 threads) ran at about 2,000
positions/s under WSL and 1,700 on Windows, although Windows was expected to be faster: no VM, and
the RAM no longer capped.

## Measurements

`search_bench` at the same commit on both sides, identical node counts on every position, so both
builds search the same tree:

| | Windows | WSL |
|---|---|---|
| 1 thread, depth 9 | 2,730,241 nps (1,166 ms) | 1,817,044 nps (1,752 ms) |
| 1 thread, depth 9, position 37 alone (2.25M nodes) | 591 ms | 686 ms |
| 16 separate processes, depth 11 | about 1.62M nps each, 26M in all | about 1.02M nps each, 16.3M in all |

Self-play, positions per second over a whole segment of 100,000:

| | positions/s |
|---|---|
| WSL | 2,000 |
| Windows, as ported | 1,700 |
| Windows, + `mimalloc` | 2,000 |
| Windows, + `mimalloc`, + `mmap` | 2,405 (one segment) |
| Windows, + `mmap`, probe without FEN, no allocation per node, + `mimalloc` | 2,503 to 2,521 (five segments) |
| the same, without `mimalloc` | 2,208 to 2,229 (three segments) |

## Diagnosis

The machine is not the cause: separate processes on Windows run 60% faster than on WSL, at full
load, so scheduling, power limits and thermals are ruled out. The loss happens only with 16 threads
inside one process, which is what self-play is. Two costs are of that kind, and each experiment
removed part of the loss.

1. **The shared process heap.** Rust uses the system allocator on both platforms. glibc serves small
   blocks from a per-thread cache; Windows' `HeapAlloc` serves every thread from one process heap.
   The search allocates at nearly every node:
   - the PV update, `vec![mv]` plus a `clone` of the child's line, whenever a move improves
     `best_score`, so at least once per node that searches a move (`search.rs`,
     `alpha_beta_with_buffer`);
   - `quiets_searched`, on its first push, in every node that searches a quiet move.

   One thread never contends, which is why the single-threaded bench is faster on Windows.
2. **Tablebase reads.** `shakmaty-syzygy`'s default `OsFilesystem` reads with one positioned read
   per access (`seek_read`, `ReadFile` on Windows), several per probe (`read_u8_at`,
   `read_u16_le_at`). A Windows handle opened without `FILE_FLAG_OVERLAPPED` serializes the
   synchronous I/O issued through it, so threads probing the same table queue behind each other.
   Linux's `pread` has no such lock, but is still a system call, and through a VM a costly one: WSL
   probably paid for this too.

Seen in the code, not measured: each probe converts the board to a FEN string and parses it back
into a `shakmaty::Chess` (`tablebase.rs`, `SyzygyTablebase::probe`), which formats, parses and
allocates on every probe.

## Design

### 1. Memory maps for the Syzygy tables

`SyzygyTablebase::new` uses `Tablebase::with_mmap_filesystem()` (feature `mmap`) instead of
`Tablebase::new()`, on every platform. A probe then reads mapped memory, with no system call. It
serves every caller alike: the engine's `SyzygyPath`, `rotor-arena`'s in-process players,
self-play and replay. Stockfish reads its tables the same way.

The constructor is `unsafe`, with two conditions, written at the call site:

- **No table file is modified while it is mapped.** `bin/syzygy.sh` writes new tables to `.part`
  and renames them; it rewrites a file under its final name only when that file is incomplete (its
  "Resuming the incomplete" branch). Running it while a Rotor process has the tables open is
  therefore not allowed. Both the script's header comment and README's tablebase section gain a
  sentence saying so.
- **A read error on a table crashes the process** instead of returning an error. Acceptable for
  tables on a local disk. A table on a network or Drive folder would be another matter; none is.

The mapping is the size of the files, about 70 GB of address space for 3 to 6 pieces WDL: address
space only, not memory. Pages are read on first touch and evicted like the file cache.

### 2. No allocation per node

- **PV:** a triangular table, fixed size, allocated once per `Search`: row `ply` holds a length and
  `MAX_PLY + 1` moves. An improvement writes `mv` at the head of row `ply` and copies row `ply + 1`
  behind it, in place. Each row is cleared on entry to its node, which the current `Vec` rows
  already rely on for correctness (`Search::pv`'s doc comment), so that invariant keeps its meaning.
  `search_root` returns row 0 as a `Vec`, once per iteration, not per node.
- **Quiets searched:** one buffer in `Search`, used as a stack, the way `self.moves` already is: a
  node notes the buffer's length on entry, pushes its quiets behind it, reads its own slice at a
  cutoff, and truncates back on exit. The buffer grows to its largest size once and is reused.

Every other `Vec` in the search is allocated per `Search` or per iteration, not per node: out of
scope, unless the measurement after this step points at one. One candidate, should it come to that:
`continuation_history`, 2.36 MB (`12 * 64 * 12 * 64` `i32`s), allocated and zeroed afresh for every
move of every game.

### 3. The allocator, decided by measurement

After step 2, self-play is measured on Windows without `mimalloc`. If it is within a few percent of
the 2,405 measured with it, `mimalloc` is not needed: no dependency, and no C code in the engine. If
it is clearly below, `mimalloc` becomes `rotor-arena`'s allocator, with the measurement in the
comment beside it. The engine `rotor` takes it too only if its own multi-threaded search (`Threads`
above 1) shows the same loss.

### 4. A probe without FEN

`SyzygyTablebase::probe` builds the `shakmaty` position directly from the bitboards: the twelve
piece bitboards into a `shakmaty::Board`, then side to move, castling rights, en passant square and
halfmove clock into a `Setup`, then `Chess::from_setup` with `CastlingMode::Standard`. The exact
names are to be checked against `shakmaty` 0.30. A unit test probes a set of positions both ways,
the FEN round trip and the direct one, and requires the same position.

### 5. Memory maps on Windows only

Measured, not explained: on WSL the maps lose about 7% against positioned reads. A plausible cause,
not verified: 16 threads faulting pages of the same mapping contend on the process's memory-map
lock, while `pread` goes through the page cache without it. Windows gains from the maps because its
synchronous handle serializes the reads; Linux's `pread` has no such lock.

`SyzygyTablebase::new` becomes a `#[cfg(windows)]` / `#[cfg(not(windows))]` pair: on Windows,
`with_mmap_filesystem()` as now, with its `SAFETY` comment; elsewhere, `Tablebase::new()`, no
`unsafe`. The doc comment gives both measurements. The `syzygy.sh` and README sentences about not
running the script while tables are mapped stay: true on Windows, harmless on Linux. The `mmap`
feature stays on in Cargo.toml for every target (a target-specific feature is not worth it; it
only compiles `memmap2`); its comment there says "on Windows".

Verification: `cargo test` on both; one WSL segment, expected about 2,100; one Windows segment,
expected about 2,510, unchanged.

## Order

1. Step 1 (mmap), with `mimalloc` still in: measure, expected about the 2,405 already seen.
2. Step 4 (probe without FEN): measure.
3. Step 2 (no allocation per node): measure with `mimalloc`, then without it, which decides step 3.
4. Remove whatever of the experiments is left.

Each measurement is one or more whole self-play segments on the Lichess machine, positions per second over
the segment, not the progress bar's instantaneous rate.

## Verification

- `cargo test` on Windows, and on Linux (WSL on the Lichess machine, still installed).
- `search_bench` at depth 9: node counts unchanged on every position (steps 2 and 4 must not change
  the search), time not worse, on both platforms.
- Self-play on WSL, one segment after all steps: not slower than its 2,000 positions/s.
- The engine through a GUI or `cutechess-cli` with `SyzygyPath` set: tables load, and an endgame
  position reports a tablebase score, as before.
