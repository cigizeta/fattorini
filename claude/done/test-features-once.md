# Test features once per run

Agreed with the developer on 2026-10-03. Status: implemented (`src/train/test_cache.rs`), not yet
compiled or tested by the developer, not yet run on the training machine. Left: the first run's `test cache:` line
and its round's `test_duration_secs`, and whether the compact upload slows betty's training steps.

## Problem

Every round's test pass (`round::run_test_pass`) re-reads the test cache and runs the CPU half of
every test batch (`to_batch.prepare`) again, although the test positions never change. For the big
five that half is `position_inputs` per position (board rebuild, attacks, king ring): 15-18 s of
betty's 60 s round in generation 1. From generation 2 the corpus is 3.5x larger and the training
wall clock starts to matter (TODO.md).

## Design

Keep the prepared test batches in memory across rounds, as many as a memory budget allows; each
round uploads the kept ones and recomputes only the rest. Nothing trained or measured changes: the
same rows, in the same order, in the same batches, give the same test loss.

### One path for every kind

- The cache holds the prepared host batch (`H`, today's `batch::HostBatch`) itself, not a second
  representation. `upload` takes `&H` instead of `H`, so a kept batch is uploaded every round
  without being consumed.
- A prepared batch reports its own size (`HostBatch::bytes()`: the sum of its vectors' lengths times
  their element sizes), measured, not estimated.
- No kind is special-cased. Naive's `i32` indices are small (about 0.3 KB a position); halfkp and
  halfka_v2_hm's dense `f32` rows are large and simply fill the budget sooner, the rest recomputed.

### The big five's host batch becomes compact

Today `HostBigFiveBatch` holds `i64` index and `i64` row pairs per active input, about 4-5 KB a
position (rough estimate, about 140 indices a side), over 20 GB for a 5M-position test half. It
becomes, per position and per side: the `u16` indices `SideInputs` already produces and a length per
list, plus the phase counts, scale and label as today; about 0.6 KB a position, about 3 GB for 5M.
The widening to `i64` and the row numbers move into `upload_big_five_batch`, on the main thread:
plain copying, no board work. Training batches go through the same compact form, so there is one
host format, not two.

Check, when implementing, that the extra main-thread work in `upload` does not slow training steps
(betty is launch-bound, parallel-batches.md); if it does, widen on the workers for training batches
only and keep the cache compact.

### The budget

Read once, at the start of the run, from `/proc/meminfo`:

    budget = min((MemTotal - 1 GB) * 50%, MemAvailable)

- `MemTotal`, not `MemAvailable`, so the budget depends on the machine only and every run on it
  caches the same amount. Under WSL `MemTotal` is the VM's (about 7.6 GB on the training machine's 16 GB).
- 1 GB for the operating system, then 50% to leave room for the trainer's own memory, which
  `MemTotal` does not subtract: libtorch and CUDA's host side, the shuffle buffer, batches in flight.
- `MemAvailable` as a guard only: nothing else runs on the training machine while it trains (the developer's rule,
  stated in the code as the reason the margins are small), so it never binds when the rule holds.
- No `/proc/meminfo`, or unreadable: cache nothing, say so, train as today.

75% was considered (2026-10-03) and deferred: it gains nothing for naive and the big five, which fit
at 50%, and leaves the rest for the trainer itself, whose footprint is not measured.
50% stays until the training machine gets more RAM; then measure the trainer's peak (`VmHWM` in
`/proc/<pid>/status`) and set the share to leave that plus a margin.

On the training machine: (7.6 - 1) * 50% = about 3.3 GB, enough for betty's whole test half if the 0.6 KB estimate
holds. The first run's printed line says what it really is; 50% is one constant to revisit then.

### Filling the cache

Lazily, during the first test pass, so there is no separate sweep: the pass keeps each prepared
batch, in order, while the running total plus the batch fits the budget, and stops keeping at the
first one that does not (a prefix, so later rounds know where the uncached part starts). Later
passes upload the kept prefix, then read and prepare the rest from the reader, starting at the first
uncached row, through the pipeline as today.

Printed once, after the first pass:

    test cache: 3.1 GB, 5,000,000 of 5,000,000 positions (budget 3.3 GB)

The cache lives in `run_train_nnue` for the whole run (across rounds, and across learning-rate
stages, since the test half never changes within a run) and is passed to `run_round` by mutable
reference. The transformer track passes none and is unchanged.

## Tests

- A cached test pass and an uncached one give exactly the same `test_loss`, for a budget that holds
  everything, one that holds part (a prefix), and one that holds nothing.
- Over two rounds, the second pass prepares only the uncached batches (count `prepare` calls).
- The budget formula from fixed `/proc/meminfo` text, including a missing or malformed file.
- The compact big five batch uploads to the same tensors as today's form, on a few positions.

## Out of scope

Feeding training batches faster (parallel-batches.md), stacking the grid's arms
(rotor-train-learning-rate-grid.md).
