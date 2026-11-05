# Parallel games under a clock in rotor-arena

**Done:** timed `match`/`sprt`/`duel` runs play one pair per physical core, pinned (`cpu_topology.rs`, `runner.rs::clock_placement`); fixed-depth runs keep one pair per logical core.

## Decision

For `rotor-arena match`, `sprt`, `duel`, and the ladder's duels (which share the same runner):

- `--depth`: one pair in flight per logical core, as today.
- `--time`: one pair in flight per physical core, each pair pinned to its own physical core.
- No printing of moves, under either control. Games are watched on lichess or in the cutechess
  GUI; how time was spent is read from the PGN, whose per-move comments carry score, depth and
  time.

Out of scope: `rotor-arena replay`, `rotor-arena self`, and the tournament mode (`pairing.rs`),
which keeps its own printing.

## Why one pair per physical core, pinned

- With no pondering, only one engine of a game searches at a time, single-threaded
  (`uci_client.rs` always sends `Threads 1`). So one core per game is enough: its two engines
  take turns on it.
- Two games on sibling hyperthreads would share one core's execution units and slow each other
  by a varying amount, which distorts exactly the clocks under test. One game per physical core
  leaves the siblings to the arena itself and the OS.
- Pinning keeps the scheduler from moving two games onto the same physical core.

Under WSL2, a pinned CPU is a virtual CPU: the hypervisor usually maps them stably, but doesn't
guarantee it. Check the topology the VM exposes once (step 5).

## What this costs

- **Ctrl+C loses more.** Half pairs can only be recorded with one pair in flight
  (`runner.rs::play`, the "Half pairs" doc comment), so a parallel timed run records whole pairs
  only, as `--depth` already does. A Ctrl+C then loses every pair in flight: at 3+2, up to about
  20 minutes of play per core. Nothing changes in the resume logic: a resume already skips a
  stale half pair when `record_half_pairs` is false.
- **More pairs are discarded at resolution.** When the rule resolves, the pairs still in flight
  are discarded (`record_pair`), as under `--depth` today.

## Implementation plan

### Step 1 -- physical cores

A new `src/arena/cpu_topology.rs`:

```rust
/// One logical CPU per physical core, the lowest-numbered of each core's siblings.
pub fn physical_core_cpus() -> Option<Vec<usize>>
```

It reads `/sys/devices/system/cpu/cpu<N>/topology/thread_siblings_list` for every
`cpu<N>` directory, parses each list (`"0,8"` or `"0-1"`), and keeps the smallest CPU of each
distinct list, sorted. It returns `None` if the directory can't be read or no list parses.

The parsing is a pure function of the list strings, so it's unit-tested apart from the file
reading.

### Step 2 -- pinning a worker thread

The `libc` crate becomes a direct dependency of `rotor-arena` (it's almost certainly already a
transitive one; check `Cargo.lock`). In `cpu_topology.rs`:

```rust
/// Pins the calling thread to `cpu`. Processes it spawns afterwards inherit the pin.
pub fn pin_current_thread(cpu: usize) -> Result<(), String>
```

with `sched_setaffinity(0, ...)` on a one-CPU set, on Linux only (`#[cfg(target_os = "linux")]`;
elsewhere it returns `Err`).

### Step 3 -- the runner

- `Run::threads` stays "pairs in flight". A new field `Run::pin_to: Option<Vec<usize>>`: the CPU
  of each worker, `None` at a fixed depth.
- `Run::threads_for(control)` is replaced by `Run::placement(control) -> (usize, Option<Vec<usize>>)`:
  - `FixedDepth`: `(available_parallelism, None)`, as today.
  - `Fischer`: `physical_core_cpus()` gives `(cpus.len(), Some(cpus))`. If it returns `None`,
    fall back to `(max(1, available_parallelism / 2), None)` with a warning: half the logical
    cores, unpinned, which matches the physical core count on a machine with two hyperthreads
    per core.
- `run_worker`, as its first act and before `start_player`, pins itself to `pin_to[worker]`. If
  pinning fails, the worker prints a warning and plays unpinned: the games are still valid, only
  less isolated.
- The start-up line `"{prefix}: {} pair(s) in flight, {:?}"` also says `pinned to physical cores`
  when `pin_to` is `Some`.

### Step 4 -- printing and half pairs

- `Run::print_moves` is deleted. `run_worker` passes `false` to `game::play_one_game`, which
  keeps its parameter for `pairing.rs`. The per-game `game N: ... by ...` lines, the per-game
  `match:` running score, and the `print_moves` branch of `record_pair` go; the pair line
  (`{prefix}: pair N  score/2  summary`) is what every run prints.
- `record_half_pairs` stays as it is, derived from `threads == 1`, in `run_versus` and
  `play_duel`. It is now true only for a timed run on a machine with one physical core (or,
  after the fallback, with fewer than four logical cores).

### Step 5 -- documentation

- `main.rs`'s module doc ("How many pairs are in flight is not an option: one game at a time
  under a clock ..."), `run_versus`'s doc (Ctrl+C under a clock), and `main.rs:357-360`.
- `runner.rs`'s module doc (lines 11-13), `Run`'s field docs, and the `--time` line of the
  usage text (`one game at a time`).
- `TODO.md`: nothing to remove; this was never an item.

### Tests

- `cpu_topology`: sibling lists `["0,8", "1,9", "0,8", "1,9"]` give `[0, 1]`; ranges
  `["0-1", "2-3"]` give `[0, 2]`; no hyperthreading `["0", "1"]` gives `[0, 1]`; a malformed list
  is skipped.
- `runner`: `placement(FixedDepth(9))` has no pins; `placement(Fischer)` has as many pins as pairs,
  or no pins and half the logical cores (at least 1). The fallback is a pure function of the
  topology result and the logical core count, so it's tested without reading `/sys`.
- The existing `threads_for` test in `runner.rs` is rewritten against `placement`.

### Verification

Pinning itself isn't unit-testable. Once built, on the machine:

```
lscpu -e
```

shows the CPU-to-core map WSL2 exposes: the `CORE` column should pair up sibling CPUs. Then,
during a `--time` run:

```
ps -eo pid,psr,comm | grep -E 'rotor|stockfish'
```

should show each pair's two engines on the same CPU, and no two pairs sharing one. Finally, a
short timed match compared with today's single-game one: similar depths per move at the same
clock mean the parallel games aren't slowing each other.
