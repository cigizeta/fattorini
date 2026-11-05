# rotor-train checkpoint retention

**Archived: written for the streaming pipeline, since retired.** What survives, in
`src/train/checkpoint_store.rs`: `checkpoints/` keeps the `BEST_CHECKPOINTS_KEPT = 4` lowest test
losses (`prune_checkpoints`, ranked from `history.tsv`, last row per step wins), weights only. Gone:
the `resume/` generations, `RESUME_GENERATIONS_KEPT`, the GPU-thread handshake. The "Scope" section
below is reversed today: `run_train_nnue` is the path that trains.

## Problem

`rotor-train`'s streaming pipeline currently leaves `data/networks/<name>/` in a shape with
two separate defects.

**It grows without bound.** Every evaluation round writes `checkpoints/step_{step:09}.vs`, and
nothing ever deletes one. A network trained for a few thousand rounds accumulates a few thousand
full weight dumps, the overwhelming majority of which will never be looked at again.

**A resume point is four independent in-place overwrites, spanning two threads.** `model.vs`,
`optimizer.opt`, `early_stopping.state` and `stream_checkpoint.state` sit at the run directory's
root and are each truncated and rewritten every five minutes and at every round boundary. Two
consequences follow:

- There is no older copy. A crash, a full disk or a power cut partway through
  `vs.save(&model_path)` leaves a truncated `model.vs`, and the run is unrecoverable -- not
  merely rewound, but dead, because the only copy of the weights was the one being overwritten.
- The four files are not a consistent set. The GPU thread writes `model.vs`/`optimizer.opt`; the
  main thread writes `stream_checkpoint.state`. The periodic path does coordinate these through
  the pause-and-ack handshake (`checkpoint_signal.rs`), but the round-boundary save does not: by
  the time the GPU thread saves after its test pass, the main thread has long since resumed
  dispatching PGN work, so the persisted stream cursor names a point in the self-play corpus that
  is ahead of the weights it sits beside.

## Shape

```
data/networks/amanda/
  history.tsv
  resume/
    000000039/{model.vs, optimizer.opt, stream_checkpoint.state}
    000000040/...
    000000041/...
    000000042/...
  checkpoints/
    step_000008192.vs
    step_000040960.vs
    step_000057344.vs
    step_000065536.vs
```

`history.tsv` stays at the root. It is an append-only log of every round the network ever ran,
not a checkpoint, and it is deliberately never pruned: it is the record that makes retention
decidable at all, and it stays complete even for rounds whose weights have been deleted.

### resume/

A resume point becomes a numbered *generation directory* holding all three files, committed
atomically:

1. The GPU thread, on a checkpoint request, creates `resume/.partial_{gen:09}/` (`create_dir_all`,
   idempotent) and writes `model.vs` and `optimizer.opt` into it, then acknowledges.
2. The main thread, on receiving the acknowledgement, writes `stream_checkpoint.state` into the
   same directory and then `rename`s `resume/.partial_{gen:09}/` to `resume/{gen:09}/`.
3. It then prunes generations beyond the newest `RESUME_GENERATIONS_KEPT` and increments the
   shared generation counter.

A directory rename is atomic on POSIX, so a generation either exists complete or does not exist
at all. There is no partially written generation for a later run to mistake for a good one. A
crash leaves a `.partial_*` directory behind, which startup sweeps and which the leading dot
keeps out of the numeric listing regardless.

`gen` is a monotonic index, not the optimizer's step count. Two checkpoints can land on the same
step -- the five-minute timer can fire twice while the queue is starved of positions and no
gradient step has happened in between -- and a step-named directory would then collide with
itself. This is the same reasoning that gave `stream_checkpoint.rs` its `current_round`. The
counter lives in an `AtomicU64` shared by the two threads, which also keeps the Ctrl+C handler
trivial: it still only raises the existing flag, and the GPU thread reads the generation number
for itself rather than being handed a path it would have had to be given from a signal handler.

The generation number needs no separate persisted state. On startup the next generation is one
past the highest directory present, so the numbering is self-describing.

**Loading** takes the highest-numbered generation. The older three exist for manual rollback:
if the newest generation turns out to hold bad data despite being structurally complete -- disk
corruption in the bytes themselves, or a divergence only noticed several rounds later -- the
developer deletes it and reruns. Automatic fallback to the next generation down is deliberately
not implemented: `AdamW::load` and the state-file readers panic on malformed input rather than
returning `Result` (CLAUDE.md rule 4, correctly), so catching that would mean `catch_unwind`
around a load, which is a large amount of machinery for a case where a human deleting one
directory is both simpler and more likely to be the right call anyway.

### Round boundaries fold into the handshake

The round-boundary save stops writing `model.vs`/`optimizer.opt` directly. Instead the main
thread, the moment it receives `round_complete` and rotates the test file, raises the checkpoint
flag and waits for the acknowledgement before sending the handoff. The GPU thread services that
request in the gap between sending `round_complete` and receiving the handoff -- *before* its test
pass, not after -- so the main thread waits for three file writes rather than for a whole test
pass with its PGN workers idle.

Committing before the handoff is not an optimization but a requirement. The rotation is not
reversible: the finished round's test file is about to be read back and deleted by the GPU thread,
so if the new `current_round` never reached disk, a resume would reopen a file that no longer
exists and `open_resumable_test_writer` would rebuild it as that many zero bytes, which reads back
as garbage test entries rather than as an error. The pre-existing code wrote
`stream_checkpoint.state` synchronously at the rotation for exactly this reason, and that property
has to survive the move into generations.

Both waits at a round boundary poll rather than block. A Ctrl+C can land precisely there, and a
bare `recv` on the handoff would leave the GPU thread waiting for something the main thread will
never send, so `join` would never return. The same reasoning adds a drain after the GPU thread's
main loop: whatever ended it, an unanswered checkpoint request left behind would block the main
thread in `await_ack` for good, since the GPU thread is the only one that can ever answer one.

### Early-stopping state is derived, not stored

A generation is committed at a round boundary, before that round's test pass has produced the loss
an `early_stopping.state` inside it would have to absorb. Any copy stored there would therefore be
exactly one round stale by construction, and a resume would silently drop one round's contribution
to `best_step` and to the patience counter.

So the streaming pipeline drops the file and rebuilds the state instead, by replaying every
`(step, test_loss)` in `history.tsv` through `EarlyStopping::observe` -- `EarlyStopping::replay`.
That turns a second copy that could disagree into a derivation that cannot, and it shrinks the set
of files that have to be mutually consistent from four to three. The same last-row-wins,
order-by-step rule the checkpoint ranking uses applies here, and for the same reason.

`EarlyStopping::save`/`load` stay for the two corpus-cache paths, which have no such ordering
problem and keep their root-level state file.

### checkpoints/

Stays a flat directory of bare `step_{step:09}.vs` weight files, unchanged in both naming and
content, so `check`, `calibrate` and `correlate` keep resolving a step to a path exactly as they
do now. Their six identical copies of that lookup collapse into one `resolve_checkpoint`, which
also lists the steps that *do* exist when the requested one is gone: with pruning in place, a step
that `train` genuinely reached and that `history.tsv` still records can legitimately have had its
weights deleted, and the old message read as though the run had never got that far.

Every round still writes one. Retention is by validation loss, not recency: after writing, rank
every surviving checkpoint by its `test_loss` and delete everything from `BEST_CHECKPOINTS_KEPT +
1`-th place onward. A round that was not a new best can still outrank an older best (losses 1.00,
0.90, 0.95, 0.92 put two non-bests in the top four), so this is a different set from "the four
most recent improvements" and the ranking has to be computed rather than inferred from
`EarlyStopping::observe`'s return value.

Losses come from `history::read_all`, whose doc comment already anticipates this as its first
caller. Two ordering consequences:

- The history row must be appended *before* the archive is written, the reverse of the current
  order, so the step being pruned against is already in the log. A crash between the two leaves a
  history row with no checkpoint, which is harmless; the current order would leave a checkpoint
  with no history row, which would make it unrankable.
- A step may appear in `history.tsv` more than once. Resuming from a generation rewinds the
  optimizer's step count, and any round that completed after that generation was committed gets
  redone and re-appended. The last row for a given step wins, being the most recent invocation's
  measurement.

A checkpoint file whose step has no history row at all is an assertion failure, not something to
skip: this code is the only writer of that directory, so such a file means something upstream is
already wrong.

`checkpoints/` holds weights only, no optimizer state. Its purpose is restarting training with
different hyperparameters, and a hyperparameter change resets the optimizer: carrying AdamW's
moment estimates across an optimizer that is about to be reconstructed anyway would be dead
weight on disk.

## Constants

```rust
const RESUME_GENERATIONS_KEPT: usize = 4;
const BEST_CHECKPOINTS_KEPT: usize = 4;
```

Hard-coded rather than exposed in `net.yaml`, following `TRAIN_QUEUE_CAPACITY_BATCHES`: this is
an operational disk-versus-recoverability tradeoff that is the same for every network trained
under this project's single-PC budget, not a per-network modelling choice worth a config field
that would then have to be kept meaningful across every existing config file.

## Scope

The new layout applies to the streaming path (`run_train_streaming` and
`run_streaming_gpu_thread`) only. `run_train_nnue` is already dead code, and
`run_train_transformer` is still reachable for the transformer architecture, which the streaming
pipeline explicitly does not support; both keep the flat root-level layout. They write the same
`checkpoints/step_{step:09}.vs` files, so `check`/`calibrate`/`correlate` continue to work
against either.

No migration path is needed. `data/networks/amanda/` has been deleted, and nothing else has
trained end to end under the streaming pipeline yet.

## Implementation

A new `src/train/checkpoint_store.rs`, holding everything above as plain filesystem operations
with no `tch` dependency, so it is testable against a temporary directory without building a
`VarStore`:

- `next_generation(resume_dir) -> u64` -- one past the highest numbered directory, 0 if empty.
- `latest_generation(resume_dir) -> Option<u64>`.
- `partial_dir(resume_dir, gen) -> String` and `generation_dir(resume_dir, gen) -> String`.
- `commit_generation(resume_dir, gen)` -- the rename.
- `sweep_partials(resume_dir)` -- startup cleanup of `.partial_*`.
- `prune_generations(resume_dir, keep)`.
- `prune_checkpoints(checkpoints_dir, history_path, keep)` -- the loss ranking.

`main.rs` then changes at the call sites: `resolve_checkpoint` replacing the six copied lookups,
path construction in `run_train_streaming`, `save_resume_files` and `commit_resume_generation` as
the two threads' halves of a generation, the round-boundary poll loop and post-loop drain in
`run_streaming_gpu_thread`, and the history-then-archive reordering.

## Status

Implemented. Builds clean, suite passes at 258 tests. Not yet exercised against a real run: the
retention rules are covered by unit tests, but the crash-safety the generation layout exists for
can only really be judged by Ctrl+C'ing a live run and resuming it, which is waiting on the
desktop along with the rest of the streaming pipeline's end-to-end testing.

Deliberately still open, unchanged by this work: the `check`/`calibrate`/`correlate` gap, where all
three read a persistent length-bucketed test cache that a streamed network never builds.
