# The smithy's bench, made comparable to the arenas'

Status: **design settled** with the developer 2026-11-13; steps 1 and 2 done, step 3 written, the
same day. Next: step 3 compiled, then step 2's check again.

## Why

A clocked test or tune scales every arena's clock by the smithy's bench against the arena's own
(`spsa::clock_factor`), so a game plays about as deep on every machine as the control would on the
smithy's. The smithy's bench comes out about a third low, so every clock is scaled down:

- **Tune of 2026-11-09** (TODO.md, "The smithy's bench on an idle machine"): the smithy benched its
  machine at 841,150 nodes/s per thread, the arena on the same machine 1.15M to 1.34M by the same
  method; the tune meant for 10+0.1 played about 6+0.06.
- **Test 5, 2026-11-13** (180+2, Classic against Reckless, stopped as useless): the arena on the
  smithy's machine benched 1,097,000 nodes/s per thread and was dealt 120.122+1.335, 0.667 of
  180+2, so the smithy's figure was about 732,000. The machine was idle: the arena had just started
  and waited to be claimed, and the smithy benches before it deals anything. Another arena played
  87.129+0.968.

So the gap is between the two processes, not (only) a busy machine, as TODO.md had assumed.

## The likely cause

Both run the same `loaded_bench` (half the logical cores, each thread `fattorini::bench::bench`),
but the arena's global allocator is `mimalloc` and the smithy's is Windows' system heap. The bench
allocates afresh for every position and depth (a 16 MB table, the history tables, the search's
buffers) on all its threads at once, and the system heap does not scale with them: the arena's own
Cargo.toml records self-play slowed by it, 2,215 positions per second against 2,505.

## Steps

1. **The smithy's global allocator `mimalloc`,** as the arena's (Cargo.toml and main.rs). It covers
   Rust's allocations alone; libtorch's tensors keep their own allocator.
2. **Checked on an idle machine:** the smithy's bench and its machine's arena bench within a few
   percent of each other, read from the two logs at the next clocked test. Then test 5's duel again
   (reckless-time-management.md).
   **Done 2026-11-13, partly:** test 6's smithy benched 925,334 against the arena's 1,096,999 on
   the same idle machine, 0.84 (0.67 before), so the allocator was about half the gap. Test 6
   stopped.
3. **The best of five runs, one function (the developer, 2026-11-13):** `loaded_bench` moves to
   `fattorini::bench`, the smithy's and the arena's copies removed, and keeps the fastest of
   `LOADED_BENCH_RUNS = 5` runs (about 5 s): one run of about a second is at the mercy of the CPU
   still raising its clock or a background task. Not done: the bench refused while an arena on the
   smithy's machine plays (test 6's machine was idle, so it would not have helped). Then step 2's
   check again, at the next 180+2 test. **Written 2026-11-13, not yet compiled.**
