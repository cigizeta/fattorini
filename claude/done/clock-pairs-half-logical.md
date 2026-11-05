# Timed pairs: half the logical cores

Decided with the developer 2026-10-06. Under a clock, `rotor-arena` plays one pair per physical
core today (`runner.rs` `clock_placement`): 20 on the training machine, which contend too much (shared L3, memory
bandwidth, all-core turbo), and under a clock contention costs depth, so it can move results. From
now on a clock plays **half the logical cores** in pairs. A fixed depth is unchanged: one pair per
logical core, unpinned, since contention there costs only wall time.

| machine | logical | physical | clock today | clock after | depth |
|---|---|---|---|---|---|
| the training machine | 20 (presumably, no SMT) | 20 | 20 | 10 | 20 |
| the Lichess machine | 16 | 8 | 8 | 8 | 16 |

## The change

`clock_placement(physical_cores, logical)` in `src/arena/runner.rs`:

- `threads = (logical / 2).max(1)`, whether or not the topology could be read.
- With the topology: pinned to the first `threads` entries of `physical_core_cpus()`, which is
  sorted, so the lowest-numbered physical cores. Each pair keeps a physical core to itself. On a
  hybrid Intel CPU those are the performance cores, which Windows numbers first (an assumption, not
  checked on the training machine). If `threads` exceeds the physical count (impossible with at most two logical
  per core, kept as a guard), every physical core is used and `threads` drops to that count.
- Without it: unchanged, half the logical cores, unpinned.

Doc comments updated to match: `runner.rs`'s module doc, `Run::placement`, `clock_placement`;
`main.rs`'s module doc (line 21) and `--time`'s usage line ("one pair per physical core" becomes
"half the logical cores in pairs, each pinned to its own physical core"); `cpu_topology.rs`'s module
doc if it states the count.

## Tests

`runner.rs`'s `a_clock_pins_one_pair_per_physical_core` becomes
`a_clock_plays_half_the_logical_cores_pinned_to_physical_ones`:

- `clock_placement(Some(vec![0, 2, 4, 6, 8, 10, 12, 14]), 16)`: 8 pairs, pinned to all eight
  (the Lichess machine's shape).
- `clock_placement(Some((0..20).collect()), 20)`: 10 pairs, pinned to `0..10` (the training machine's shape).
- `clock_placement(Some(vec![0]), 1)`: 1 pair.

The no-topology and one-pin-per-pair tests stay.

## Verify (Windows)

```
cargo test --bin rotor-arena runner
```

Then a short duel on the training machine prints `10 pair(s) in flight, pinned to physical cores`.

## Not affected

Results already recorded: a `spec.txt` names the rule and control, not the pair count, so a
resumed run continues under the new placement.
