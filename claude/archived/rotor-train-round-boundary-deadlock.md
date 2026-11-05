# The streaming round-boundary deadlock

## Status

**Superseded: the streaming pipeline was retired rather than repaired, and this fix was never
applied.** See rotor-train-dataset-cache.md, which moved the NNUE track back onto a cache built
ahead of time by `rotor-prepare`; `run_train_streaming` and the modules that existed only for it
are gone, and with them the deadlock below.

Kept for the diagnosis, which is the part worth keeping: a producer holding a bounded queue's send
across a point where its consumer has stopped consuming and started waiting on it. Anything built
here later with the same handshake shape should read it first.

Diagnosed from a thread backtrace of a real hang, not inferred. Fix designed, never applied.

## The hang

`./bin/rotor-train train amanda --lr 3e-4 --threads 8 --patience 1000` completed eight rounds and
froze. `gdb -batch -ex 'thread apply all bt 15'` shows exactly two threads that matter:

```
Thread 1  (main): run_train_streaming -> RoundRouter::route -> SyncSender<CacheRecord>::send -> park
Thread 30 (GPU):  run_streaming_gpu_thread -> Receiver<TestFileHandoff>::recv_timeout -> park
```

Both are where they should not be at the same time.

## Why it deadlocks

The round boundary is a three-step handshake:

1. The GPU thread finishes the round's last optimizer step and sends `round_complete`
   (`main.rs`, `run_streaming_gpu_thread`). From that instant it consumes nothing from the train
   queue.
2. It then waits for `TestFileHandoff` in a poll loop, which gives way only to a checkpoint request
   or a stop.
3. The main thread notices `round_complete`, rotates the test file, checkpoints, and sends the
   handoff. Only then does the GPU thread run its test pass and go back to consuming.

Step 3's check sits *after* the routing loop in the main thread's inner loop:

```rust
if let Some((game_index, records)) = pool.recv_result_timeout(DISPATCH_POLL_INTERVAL) {
    for record in records {
        router.route(record);          // blocks when the bounded queue is full
    }
    ...
}

if round_complete_receiver.try_recv().is_ok() {
    ...                                 // the only place the handoff is ever sent
}
```

`RoundRouter::route` pushes train positions onto a `sync_channel` of
`TRAIN_QUEUE_CAPACITY_BATCHES * training_batch_size` records (4 * 1024 = 4096 for amanda) and blocks
when it is full. That block is correct backpressure *while the GPU thread is consuming*. It is a
deadlock the moment the GPU thread has stopped consuming and is waiting on the main thread instead.

The two conditions meet at every round boundary. Whether the run survives one is luck: the main
thread re-checks `round_complete` once per game, so it escapes only if the queue had more than one
game's worth of free slots (roughly 36 train positions) at the instant `round_complete` was sent.
CPU production outpaces GPU consumption, so the queue is typically full again by then -- the queue
refills during the forward/backward of the batch the GPU already pulled. Eight rounds happened to
land on the lucky side.

The stuck progress bar reading `510/512` rather than `512/512` is unrelated: indicatif throttles
redraws, so the last two `set_position` calls were coalesced away with nothing following to flush
them.

## The fix

**`route` must never block while a round boundary can be pending.** The queue's backpressure stays;
what changes is that it becomes interruptible, so the main thread can service the handoff and come
back to the record it could not place.

### `SplitCounter`: decide and advance separately

`route()` currently decides and advances in one call, so a send that has to be retried would consume
a cycle position and corrupt the exact 32:1 split. Split it into `peek()` and `advance()`, keeping
`route()` as the two composed -- every existing test stays meaningful, and the test arm keeps using
it unchanged.

### `RoundRouter::route` returns the record it could not place

```rust
pub(crate) fn route(&mut self, record: CacheRecord) -> Option<CacheRecord>
```

- `Split::Test`: unchanged, always `None`.
- `Split::Train`: `try_send`. On success, advance and return `None`. On `Full`, return
  `Some(record)` **without advancing**. On `Disconnected`, advance and return `None` -- dropping the
  record is already the documented, tested behaviour for a GPU thread that stopped first.

Non-blocking rather than a timed send: `SyncSender::send_timeout` is not stable, and keeping the
router free of both blocking and sleeping leaves the pacing where it is readable, in the loop below.

### The main loop drains a pending game across iterations

One game's records become a `VecDeque` held across iterations, together with its `game_index`, so
the round-boundary block runs while the queue is full:

```rust
let mut pending: Option<(u64, VecDeque<CacheRecord>)> = None;
```

- Dispatch and `recv_result_timeout` are skipped while `pending` is `Some`: the loop finishes the
  game in hand before taking on more work.
- The drain pushes the unplaced record back to the front and sets a `stalled` flag.
- `outstanding -= 1` and `watermark.mark_complete(game_index)` fire only once the game is fully
  drained, so the resume watermark never claims a game whose positions are still in hand.
- `round_complete`, the periodic checkpoint and the stop check all run every iteration, stalled or
  not. This is the actual fix.
- A stalled iteration ends with a short `TRAIN_QUEUE_BACKOFF` sleep instead of spinning. A few
  milliseconds bounds how long a round boundary waits behind a full queue, and costs nothing next to
  the poll intervals already in this loop.

`outstanding == 0 && reader_exhausted` stays correct unchanged: `pending` being `Some` implies
`outstanding >= 1`.

### The handoff send stops panicking

```rust
handoff_sender.send(...).expect("the GPU thread's own test-file handoff receiver dropped unexpectedly");
```

This is what turned the Ctrl+C out of the hang into a panic. A GPU thread that stopped on its own --
patience exhausted, or a stop it observed first -- drops the receiver, and arriving here a beat later
is a normal shutdown, exactly as `route` already argues for the train queue. Tolerate the error and
delete the finished round's test file, since the reader that would normally delete it after its pass
is gone.

## Tests

- `RoundRouter::route` returns the record instead of blocking when the train queue is full and
  nothing is reading, and the split counter has not advanced -- the same record is placed, in the
  same split, once a slot frees. This is the regression test for the deadlock itself.
- The existing router tests keep their assertions, updated for the new return type.
- `SplitCounter`: `peek` does not advance, and `peek` + `advance` reproduces `route` exactly.

## Not in scope

The deadlock is independent of the learning-rate grid (rotor-train-learning-rate-grid.md) and of
evolutionary-training.md's loop, and the fix is a prerequisite for both: lockstep arms make the GPU
thread's round boundary longer, which widens the window this bug lives in.
