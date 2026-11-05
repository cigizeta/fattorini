# Deprecating rotor-prepare: streaming self-play training in rotor-train

**Archived: superseded, see rotor-train-dataset-cache.md.** The streaming pipeline designed here was
built, hit a round-boundary deadlock, and was deleted rather than repaired; `prepare` returned as
`rotor-train prepare`, a flat cache read through a shuffle buffer. `rotor-prepare` and `rotor-self`
no longer exist.

## Status

Design settled end to end -- no open questions remain. Every piece is written, builds clean, and the
full `rotor-train` suite passes at 247 tests. What has *not* happened yet: a real end-to-end run
against real self-play data, which is the only thing that can actually validate the orchestration
layer (it has no unit tests of its own, by nature). See "Progress" below. This document is detailed
enough to resume cold.

## Motivation

`rotor-prepare` builds `positions.cache`/labels once, on disk, so `rotor-train` can read a compact,
length-bucketed cache many times cheaply. That two-stage split exists to avoid re-parsing PGN and
re-running the AB(lambda) label recursion on every training pass. `rotor-curate` (and the curated-PGN
path generally) is deprecated -- this plan applies to **self-play sources only**. With curation gone
and no more cross-lambda/cross-architecture cache reuse need (self-play labels are embedded in the
PGN itself, no separate database, no lambda sweep against a shared positions cache), the two-stage
split stops paying for itself. Instead, `rotor-train` streams `.pgn.zst` segments directly: one
centralized reader thread walks the PGN, a pool of PGN-worker threads parse/featurize/label positions
in parallel and feed a bounded queue, one consumer thread drains it to drive GPU training steps --
avoiding the alternative failure mode of a single, synchronous, CPU-bound reader starving the GPU
between batches.

## Scope

- **Self-play only.** No curated-PGN stems (`preprocessing.pgn`), no SQLite teacher database, no
  `rotor-curate` dependency at all. A `net.yaml` for this pipeline has `preprocessing.parent` and no
  `preprocessing.pgn`.
- **Every `architecture.kind` except the transformer track is in scope: `naive`, `halfkp`,
  `halfka_v2_hm`.** Corrected directly by the developer after an earlier version of this plan wrongly
  assumed `halfkp`/`halfka_v2_hm` needed piece-count-homogeneous batches the same way length-bucketing
  implies -- in fact none of the three NNUE feature sets need padding: `to_dense_batch` builds a
  fixed-shape dense tensor per position regardless of piece count (`train/batch.rs`), so positions of
  different piece counts can already share a batch with no padding, for all three. **Only the
  transformer track's sequence-based representation actually needs padding** (fixed-length sequences
  to batch), which is why it stays out of scope here. `rotor-train` refuses to run this streaming path
  against the transformer architecture with a plain `panic!`, immediately after `net.yaml` is loaded
  and `architecture.kind` is known, before any pipeline thread spins up -- same style as the existing
  `pv.unwrap_or(0) > 1` guard (`rotor-preprocessing-without-teachers.md`: "both `generate` and
  `prepare` panic outright ... rather than silently building" something that doesn't work), not a
  recoverable `Result`.
- **`rotor-prepare` is deprecated, not deleted.** It stays in the repository, buildable, unmaintained
  -- not removed as part of this plan.
- **No length buckets, no epochs, no cycling.** One sweep of whatever `.pgn.zst` segments currently
  exist under `data/self/{parent}/` at process start. When the glob is exhausted, whatever's left in
  the queue below a full batch stays queued (not flushed as a short batch), so a later invocation --
  once `rotor-self` has produced more segments -- can pick up and finish it.

## Pipeline architecture

Three thread roles, not four -- an earlier version of this design gave the test file its own
dedicated writer thread; folded into the main thread instead (see "Round boundary and eval" below for
why that's not just a simplification but resolves a real correctness question).

- **Main thread.** Owns PGN glob traversal and the current file's read cursor (game index -- see
  "Main thread: PGN reading and dispatch" below for why there's no byte offset). Dispatches each
  collected game to one of N PGN threads over a per-worker bounded channel --
  the same shape `rotor-curate`'s own reader-to-worker-pool dispatch already uses (`curate.rs`'s
  `GameTask`/per-worker `mpsc::sync_channel`). Receives labeled, featurized positions back from all N
  PGN threads over one shared results channel (`curate.rs`'s own `GameResult` shape: N cloned
  `Sender`s, one `Receiver`). For each position it receives, applies the mod-33 split decision itself
  (see below) and either pushes it onto the bounded training queue or writes it to the round's test
  file. Also owns the periodic checkpoint, Ctrl+C handling, and the round-boundary handshake with the
  GPU thread.
- **N PGN threads** (`--threads <n>`, mandatory -- matching `rotor-self`/`rotor-curate generate`'s own
  convention exactly: same flag name, same no-default policy, hardware-dependent, tuned per machine).
  Each receives one collected game at a time from its own dispatch channel, computes labels and
  features for every position in it, and sends the results back to the main thread. This is where the
  CPU-heavy work actually lives -- the main thread's own per-position work (a channel send, or an
  occasional small file write) is cheap, so centralizing it on one thread doesn't reintroduce the
  single-threaded bottleneck this whole design exists to avoid.
- **GPU thread.** Pops train positions off the bounded queue (the main thread is its sole `Sender`),
  builds batches, drives `tch` steps. Once it has consumed `round_train_volume` positions
  (`steps_per_eval` steps), it sends a one-way "round complete" signal to the main thread. It never
  talks to a separate writer thread, and it never receives test positions directly from anyone -- only
  the main thread ever touches the test file.

### Discovery

Segment discovery reuses today's `self_play_segments` shape (`preprocessing_config.rs:158-168`):
`std::fs::read_dir("data/self/{parent}/")`, filtered to `.pgn.zst`, sorted for a deterministic order.
Re-globbed fresh at process start (and at resume) so newly-arrived segments are picked up without a
config edit, same as today.

### Main thread: PGN reading and dispatch

Owns the glob traversal (sorted order, one file fully processed before moving to the next -- never
multiple files read concurrently) and the current file's game index. Reads games sequentially with
`SelfPlayGameCollector` (existing, `length_cache.rs:1195-1289`, wrapped by the new
`SelfPlaySegmentReader`) and round-robin dispatches each collected game to one of the N PGN threads.

**No byte offset is checkpointed.** `zstd::stream::read::Decoder` implements `Read` but not `Seek`
(confirmed against its own docs.rs page, not assumed), so a self-play segment's `.pgn.zst` stream
genuinely can't be seeked to an arbitrary position the way the curated path's plain `.pgn` files can
(`open_pgn_reader_at`) -- there is no `Reader::stream_position()` available here at all, unlike an
earlier version of this design that assumed there would be. Resume instead reopens the segment from
byte `0` and skip-replays (discards) the first `last_dispatched_game_index` games via ordinary
`read_game()` calls before resuming real dispatch (`SelfPlaySegmentReader::skip_to`,
`length_cache.rs`). Confirmed cheap at this format's own real scale (`1024 * 128` games per segment,
decided directly with the developer): skip-replay only re-parses raw PGN text, never redoing the
actually expensive downstream work (dispatch, AB(lambda), feature extraction) a full segment restart
would.

Because there is exactly one thread doing this, the checkpoint's file-progress state (processed-file
list, current file, game index) is simple and non-racy -- no per-thread cursors to reconcile, unlike
an earlier version of this design that gave each producer thread its own file and cursor.

### PGN threads

Each PGN thread receives one collected game at a time from its own dispatch channel and, per game:

1. Computes `white_relative_scores` and runs AB(lambda) via `compute_labels` (existing, both already
   in `length_cache.rs:1352-1364`, mechanism documented in full in `training.md`) -- needs the whole
   game buffered before any of its positions can be labeled, which is why checkpoint granularity stays
   at game boundaries (never mid-game), same as today. `lambda` itself is unchanged: read once at
   startup from `net.yaml`'s `preprocessing.lambda` field (`amanda.yaml`'s `lambda: 0.7`, for
   example), not touched by this redesign -- every PGN thread uses the same value.
2. Extracts `CanonicalPosition` per position (`extract_canonical`, `features.rs:99-124`).
3. Never filters (`is_trainable` is not called for self-play today either, `length_cache.rs:1310-1311`
   -- "we do not throw away anything"). Labels are clamped via the existing `[-(1-CLAMP_EPS),
   1-CLAMP_EPS]` clamp (`labels.rs`'s `CLAMP_EPS`), same as today.
4. Sends the game's labeled, featurized positions back to the main thread over the shared results
   channel. The train/test split decision itself happens on the main thread, not here (see below).

**Out-of-order completion is tolerated, not reassembled -- but a lightweight watermark tracks how far
it's safe to resume from.** PGN threads can finish games out of order relative to dispatch order (a
game dispatched later can finish before one dispatched earlier, if threads are unevenly loaded). Unlike
`rotor-curate`'s own reader/dispatcher, which reassembles worker results into strict file order before
committing (`curate.rs:415-462`, a `pending` map + `next_to_write` cursor) because it needs a real
contiguous resume offset for its SQLite writes, this pipeline never reassembles results into dispatch
order and never blocks on that -- decided directly with the developer.

What the checkpoint actually records is not the dispatch cursor but a **completion watermark**
(`src/train/completion_watermark.rs`, `CompletionWatermark`): the highest game index N such that every
game `0..=N` has had *all* its positions fully routed (pushed to the queue or written to the test
file), tracked as results arrive out of order and only advancing once a contiguous run closes a gap.
Games past N may already be done too -- that part is genuinely not tracked -- but resume always
redispatches starting at N+1, regardless. This matters because it's not just about not losing games (a
game above the watermark that gets redispatched is merely redone, not lost either way, since its train
positions were only ever sitting in the in-memory queue) -- it's about not *duplicating* test file
entries: the router's own counters (split counter, test file progress, buffered overflow) are
checkpointed as a snapshot taken at this exact same watermark, never live, so redoing a game above the
watermark on resume can never re-write a test entry that's already on disk from before the crash.

**The bounded training queue's own live contents are never checkpointed.** An earlier version of this
design planned to snapshot the queue itself into the checkpoint, but `std::sync::mpsc::Receiver`
(and `SyncSender`) offer no way to non-destructively inspect what's buffered -- only the GPU thread,
which holds the `Receiver`, could ever see it, not the main thread that writes the checkpoint. Rather
than replacing `mpsc` with hand-rolled synchronization just to make that snapshot possible, the
requirement itself was relaxed, decided directly with the developer: the queue's contents are treated
as pure in-memory backlog, lost on a crash same as any other in-flight work, and regenerated for free
once the redispatched games above the watermark are reprocessed. What actually matters -- no game
silently skipped, no test entry silently duplicated -- is guaranteed by the watermark mechanism above
instead.

### Checkpoint drain: periodic vs. Ctrl+C

The two checkpoint triggers behave differently given the above:

- **Periodic (5 minute) checkpoint**: no draining. Just write out the completion watermark and the
  router counters last snapshotted at that same watermark (see "Checkpoint" below) -- PGN threads may
  still be mid-game, and the queue may hold more than the watermark's worth of positions; none of that
  needs to be stopped or drained first. Cheap, no pipeline stall.
- **Ctrl+C checkpoint**: the main thread stops dispatching further games, then each PGN thread
  finishes its *current* game (never abandoned mid-game) before stopping -- a short, bounded wait (at
  most one game per thread), not a full drain of any queue. The GPU thread goes through the exact same
  pause-and-report handshake described below (see "Main/GPU thread checkpoint handshake") before
  exiting instead of looping again. Only once every PGN thread and the GPU thread have stopped is the
  final checkpoint written, so a deliberate stop minimizes redo-on-resume even though a crash (no
  chance to drain first) doesn't get that same benefit.

### Main/GPU thread checkpoint handshake

`model.vs`/`optimizer.opt`/`early_stopping.state` are owned and saved by the GPU thread (it's the one
driving `tch` steps); the `StreamCheckpoint` above is owned and saved by the main thread. Left
independent, a periodic 5-minute tick on each thread's own timer could write the two checkpoints a few
training steps apart, so a resume could load model weights from one instant paired with round-progress
counters from a slightly different one -- not a correctness risk before this pipeline (today's
single-threaded `run_train_nnue` writes both together trivially), but a real one now that they're on
separate threads. Decided directly with the developer: GPU work pauses whenever a checkpoint is
written, so the two files are always paired to the same instant.

The handshake (`src/train/checkpoint_signal.rs`, `CheckpointRequester`/`CheckpointResponder`, held by
the main thread and the GPU thread respectively):

1. The main thread's periodic timer (or its Ctrl+C handler, alongside `stop_requested`) calls
   `CheckpointRequester::request()`, which raises a shared `Arc<AtomicBool>` -- same convention as
   `round.rs`'s own `stop_requested`, polled by the GPU thread once per training step, not a busy-poll
   loop on the main thread's side.
2. The GPU thread, on seeing the flag set at its next step boundary, finishes that step (never a
   partially-applied optimizer step, same discipline as `stop_requested` today), saves its own
   `model.vs`/`optimizer.opt`/`early_stopping.state`, then calls `CheckpointResponder::acknowledge`
   with a `CheckpointAck { round_train_positions_consumed, round_loss_sum }` -- the two round-progress
   numbers the GPU thread owns but the main thread's own checkpoint still needs. `acknowledge` sends
   the ack over a bounded (`sync_channel(1)`) channel and clears the flag, then the GPU thread keeps
   looping (or exits, on the Ctrl+C path) -- no separate "resume" signal is needed, since the main
   thread's own checkpoint write touches only its own files, never anything the GPU thread also
   touches.
3. The main thread blocks on `CheckpointRequester::await_ack()`, merges the returned
   `round_train_positions_consumed`/`round_loss_sum` into its own `StreamCheckpoint`, and writes it.

`mpsc` over a spin-polled second `AtomicBool` for the ack itself: a spin poll would waste cycles and
need a tuned poll interval for no reason, and every other cross-thread rendezvous in this pipeline
(`GameTask`/`GameResult`, the "round complete" signal) already uses `mpsc`, so this keeps one
convention rather than introducing a second.

### Train/test split: per-position, exact 32:1 ratio, no rounding

Splitting per game (today's `is_train_game`, `builder.rs:39-43`) is not used here -- decided directly
with the developer: split **per position**, accepting that a training batch and that round's test set
can both contain positions from the same game.

The ratio is chosen so the round's test-set size is an exact integer, no rounding, given that
`training_batch_size * steps_per_eval` (the number of train positions consumed by one round) is a
power of two for any realistic power-of-two config:

- One **plain counter** (`u64`, not atomic -- owned solely by the main thread, which is the only
  thread that ever applies the split decision, since positions arrive there already labeled and
  featurized from the PGN threads' results channel). Incremented once per position, in whatever order
  the main thread receives them from the shared results channel.
- The counter cycles through 33 states (`counter % 33`). Value `0` routes to test; values `1..=32`
  route to train. That is a 32:1 train:test ratio -- for every 32 train positions there is exactly 1
  test position.
- `test_target = round_train_volume >> 5` (`round_train_volume / 32`), exact for any
  `round_train_volume` divisible by 32. Startup assertion:
  `assert_eq!((training_batch_size * steps_per_eval) % 32, 0, ...)`, so a future config that violates
  this fails loudly instead of silently rounding. Both the main thread and the GPU thread compute
  `test_target` independently from the same `net.yaml` fields at startup -- they never need to tell
  each other what it is.
- The counter does **not** reset every round -- it runs continuously for the whole process lifetime,
  since the 32:1 ratio holds exactly no matter where a round boundary falls within the cycle. It is
  checkpointed (see below) so a resumed run continues the same pattern rather than re-syncing from 0.
- Train-routed positions go directly onto the shared bounded queue. Test-routed positions are written
  to the round's test file, or held back once `test_target` is reached for the current round (see
  "Round boundary and eval" below).

### Bounded training queue

One shared `mpsc::sync_channel` (not per-bucket -- no length bucketing). The main thread is its sole
`Sender`; the GPU thread is its sole `Receiver`. This gives natural backpressure: the main thread
blocks on send once the queue is full, so GPU-bound consumption paces CPU-bound production rather than
an unbounded backlog accumulating -- and, transitively, backpressure on the main thread's send loop
also throttles how fast it drains the PGN threads' results channel, which throttles how far ahead the
PGN threads themselves can get.

**Capacity**: `const TRAIN_QUEUE_CAPACITY_BATCHES: usize = 4;`, capacity computed at startup as
`TRAIN_QUEUE_CAPACITY_BATCHES * training_batch_size` (positions, not batches -- the channel itself
holds individual `(CanonicalPosition, label)` items). A hardcoded constant, not a CLI flag: unlike
thread count, this isn't hardware-dependent, it's an algorithmic tradeoff between smoothing over
transient producer stalls and how far a resume might have to redo -- the queue's own live contents are
never checkpointed (see "Out-of-order completion is tolerated" above), so a larger capacity means more
in-flight positions silently lost and regenerated on a crash, not a larger checkpoint (that concern
from an earlier version of this design no longer applies, now that the checkpoint carries a completion
watermark instead of a queue snapshot). Expressed as a multiple of `training_batch_size` rather than an
absolute position count so it scales automatically if a network's batch size changes. `4`, like
`training_batch_size = 4096` itself (`training.md`), is a starting point, not a measured value --
revisit once real throughput numbers exist (CLAUDE.md rule 5), same spirit as `rotor-curate`'s own
deliberately tight `PER_WORKER_QUEUE_CAPACITY = 2` (`curate.rs:736`).

### Round boundary and eval

The test file itself: one combined record per entry (position + label interleaved, not split into
separate `positions`/`labels` files the way the deprecated corpus cache does -- this file is
disposable and per-round, so the extra bookkeeping of the durable cache's split layout isn't worth it
here). Record encoding reuses the existing `cache::write_position` wire format for the position half
(variable-width: side-to-move byte, count + packed pieces, king squares, castling/en-passant byte)
plus a trailing `f32` label, matching the project's existing hand-rolled binary convention (no
serde/bincode in this state layer, consistent with every other resume-state struct in
`length_cache.rs`).

`round_train_volume = training_batch_size * steps_per_eval` (from `net.yaml`'s `training:` section,
unchanged). The GPU thread runs training steps, draining the shared queue and driving `tch` steps
exactly as `run_training_steps` does today (`round.rs:101-146`), until it has consumed
`round_train_volume` train positions (`steps_per_eval` steps at `training_batch_size` each), then
sends a one-way "round complete" signal to the main thread.

**The GPU thread must never wait on the queue uninterruptibly.** Found while implementing, not
predicted by this design: a half-filled batch has to keep answering the checkpoint handshake and the
stop flag while it waits, so `stream_round::run_streaming_training_steps` waits with a bounded
`recv_timeout` and re-checks both on every timeout, rather than a plain blocking `recv`. With a
blocking `recv` the pipeline deadlocks outright, and not in some exotic corner: **the normal end of
every sweep** leaves an incomplete final batch by design (it is deliberately not consumed, so a later
invocation can finish it once more segments exist), at which point the main thread asks for its final
checkpoint and the GPU thread sits forever on a position that will never come, each waiting on the
other. A Ctrl+C landing while the PGN threads are behind hangs the same way. Positions in a partial
batch count toward neither `rows_done` nor `loss_sum`, and are dropped rather than trained on if the
wait ends in a stop -- so a checkpoint written from inside that wait reports only whole completed
steps, exactly as one written at a step boundary does.

**The main thread self-gates the test file, independently of that signal.** Because the training
queue has slack (`TRAIN_QUEUE_CAPACITY_BATCHES = 4`), the main thread can run somewhat ahead of what
the GPU thread has actually consumed -- it may have already pushed more than `round_train_volume`
train positions into the queue (buffered, not yet popped) by the time the GPU thread gets around to
sending its signal. Left unchecked, that means the main thread could also keep writing test positions
past `test_target` into the *same* file, contaminating a round's test set with positions that actually
belong to the next round. So the main thread tracks its own "test positions written to the current
file" count and, the moment it reaches `test_target` **on its own**, stops writing further test-routed
positions to that file -- holding them instead in a small in-memory buffer (checkpointed, see below)
-- while continuing to push train positions onto the queue as normal (no reason to stall those; round
boundaries are a purely GPU-side concept on the train side, the queue itself is never segregated by
round).

In the common case (CPU production outpaces GPU consumption, which is the point of the queue), the
main thread reaches `test_target` *before* the GPU thread's signal even arrives -- so by the time the
signal lands, the file is already exactly `test_target` entries, finalized, and there is nothing to
wait for. On receiving the signal, the main thread:

1. Hands the current test file off as ready (the GPU thread reads it back: a full, deterministic
   sweep over exactly `test_target` rows -- same shape as today's `run_test_pass`, `round.rs:155-183`,
   just now scoped to this round's own sample rather than a fixed corpus-wide split).
2. Opens a fresh, empty file for the next round and flushes its held-back overflow buffer into it
   (any test-routed positions received after this round's `test_target` was already reached).
3. Keeps running -- there is no need for the main thread to itself block waiting on anything here; the
   gating already happened earlier, continuously, as positions arrived.

The GPU thread, after sending its signal, reads the handed-off file, runs the eval pass, computes test
loss, appends to `history.tsv` as today, then deletes the file. The mod-33 counter itself is never
reset (see above).

**File naming for the handoff.** `RoundRouter<W: Write>` is generic and knows nothing about paths --
just whatever `Write`r it's given -- but the GPU thread needs a real file path to reopen and read the
handed-off file back (`stream_round::run_streaming_test_pass` takes a path, not a `Write`r). A single
fixed filename reused every round would risk the main thread overwriting it with the *next* round's
data while the GPU thread is still reading the *previous* round's, if the GPU thread ever falls behind
by more than one round. Decided directly with the developer: a `current_round: u64` counter, starting
at `0`, incremented by the main thread every time it rotates, never reset or reused -- each round's
test file is named `round_test_{current_round:09}.cache`, so successive rounds can never collide no
matter how far behind the GPU thread gets. The main thread's "round complete" handler sends the
just-finished round's own index (not a path -- both threads derive the same path from the same
formula) over the same handoff channel the "round complete" signal itself travels on; the GPU thread
reads that round's file, evaluates it, then deletes it. `current_round` is checkpointed (see below), so
a resumed run keeps counting up rather than risking a stale collision with a leftover file from before
the crash. Because a crash can in principle leave one round's file undeleted (between the GPU thread
reading it and deleting it), the main thread does a small startup cleanup pass on resume: glob
`round_test_*.cache` in the run directory and delete anything that isn't the resumed `current_round`'s
own file, so orphaned files never pile up across repeated crash/resume cycles over a long-running job.

### No length buckets, no padding

Batches are built directly from whatever the queue yields, mixed piece counts included -- fine for
every in-scope architecture (`naive`, `halfkp`, `halfka_v2_hm`), since NNUE input is fixed-size by
construction, not variable-length: `to_dense_batch` (`train/batch.rs`) builds a fixed-shape dense
tensor per position for all three feature sets, whatever the piece count, the same way a piece count
of 5 and a piece count of 30 both land in the same fixed-width board/feature representation. There is
nothing to pad, so nothing about `to_dense_batch` itself needs to change -- it stays feature-set-
agnostic at the tensor-construction level like it is today; only what feeds it changes (one
heterogeneous queue instead of one-bucket-at-a-time reads from `LengthCacheReader`). Only the
transformer's sequence-based representation actually varies in length with piece count, which is why
it alone needs padding and stays out of scope.

### Shutdown (Ctrl+C)

Same `Arc<AtomicBool>` + `ctrlc::set_handler` pattern already used elsewhere (`round.rs`'s
`stop_requested`, `curate.rs`'s `stop`) -- one shared flag, set once, observed by every thread in the
pipeline. The Ctrl+C handler raises `stop_requested` and calls `CheckpointRequester::request()`
together, in the same handler. The main thread and PGN threads behave as described in "Checkpoint
drain" above (main thread stops dispatching, each PGN thread finishes its current game then stops).
The GPU thread finishes its current training step (never a partially-applied optimizer step) then
goes through the same pause-save-ack handshake described in "Main/GPU thread checkpoint handshake"
above before exiting instead of looping again -- it does not need to send its "round complete" signal
on the way out; a partial round's progress is checkpointed as-is instead (see below), carried in the
same `CheckpointAck` the handshake already produces. Once the main thread's `await_ack()` returns and
every PGN thread has stopped, the main thread writes the `StreamCheckpoint` unconditionally -- same
struct and fields as the periodic 5-minute backup below, just triggered immediately by the stop flag
instead of waiting for the next scheduled tick, matching `round.rs`'s existing behavior of writing a
checkpoint on Ctrl+C rather than only on a timer.

No mid-round eval happens on Ctrl+C -- the in-progress round's train-position count and the current
test file's own received-so-far count are simply checkpointed as-is (part of "round progress" in the
checkpoint fields below), picked up again on the next resume.

## Checkpoint

Fires every 5 minutes, same cadence as today's `BACKUP_INTERVAL` (`length_cache.rs:94`), paired to the
same instant as the GPU thread's own `model.vs`/`optimizer.opt`/`early_stopping.state` save via the
handshake in "Main/GPU thread checkpoint handshake" above. A new hand-rolled binary struct (magic +
version + fields, matching every other resume state in this codebase -- no serde/bincode), replacing
`RoundProgress`'s file-cursor role for this pipeline. Fields:

- **Fully-processed segment filenames** -- the list of `.pgn.zst` files this run has completely
  swept, so a re-glob at resume doesn't re-walk them.
- **Current segment's cursor** -- filename and `completed_through_game_index` (`Option<u64>`), the
  completion watermark from `CompletionWatermark::completed_through()` (see "Out-of-order completion
  is tolerated" above), not a dispatch cursor. No byte offset: unlike the curated path's plain `.pgn`
  files, a self-play segment's `.pgn.zst` stream can't be seeked, so resume reopens from the start and
  skip-replays `completed_through_game_index + 1` games instead (see "Main thread: PGN reading and
  dispatch" above).
- **Split counter, test file progress, buffered test overflow** -- the router's own counters
  (`RoundRouter`'s `split_counter_value`/`test_entries_written_this_round`/`test_file_byte_length`/
  `buffered_overflow`), captured as a snapshot taken at the same instant the watermark above last
  advanced, never the router's live value -- see "Out-of-order completion is tolerated" above for why
  that distinction matters (it's what keeps a redone game from duplicating a test file entry). Test
  file progress is entry count *and* byte length (not count alone: `cache::write_position` records are
  variable-width, so byte length is what a truncate-on-resume actually needs -- same reason
  `PositionsResumeState` checkpoints file length today, `length_cache.rs:502-509`). On resume, truncate
  the in-progress test file to this byte length to discard whatever was written after the watermark,
  same trick `PositionCacheBuilder` already uses for a crash's dangling partial record.
- **`current_round`** -- the current round's own index, counting up from `0`, never reset or reused --
  names its test file on disk (`round_test_{current_round:09}.cache`, see "Round boundary and eval"
  above for why a fixed or two-slot-rotating name isn't safe here).
- **Round progress** -- train positions consumed so far this round (to know how many more steps are
  needed before the next round boundary), and running loss accumulator, same shape as today's
  `RoundProgress.rows_done`/`loss_sum`.

The bounded training queue's own live contents are deliberately **not** a checkpoint field at all --
see "Out-of-order completion is tolerated" above for why that requirement was relaxed instead of
solved with custom synchronization.

## Progress

Implemented:

- `src/train/split_counter.rs`: `SplitCounter` (the mod-33 cycle, `new`/`resume`/`value`/`route`) and
  `test_target` (`round_train_volume >> 5`, asserting divisibility by 32). Unit-tested in isolation
  (fresh-cycle routing, exact 32:1 ratio over repeated cycles, resume-mid-cycle reproduces an
  uninterrupted run's remaining splits, both panic guards). Confirmed passing: `cargo test --features
  training --bin rotor-train split_counter` (7 tests).
- `src/train/stream_checkpoint.rs`: `StreamCheckpoint`/`SegmentCursor`, the full checkpoint struct
  from this doc's own "Checkpoint" section (processed segments, current segment cursor, split counter,
  test file progress, buffered test overflow, round progress) -- `save`/`load`, hand-rolled
  magic+version binary format (`RTSP`, matching every other resume state in this codebase). Reuses
  `cache::write_record`/`read_record`/`CacheRecord` directly for the `buffered_test_overflow` field
  rather than inventing a new record encoding. **Revised three times** since first written: once to
  drop `SegmentCursor`'s `byte_offset` field (zstd streams can't be seeked), again to replace
  `last_dispatched_game_index` with `completed_through_game_index: Option<u64>` and drop
  `queue_snapshot` entirely, once the queue-checkpointing requirement was relaxed in favor of the
  completion-watermark mechanism (see "Out-of-order completion is tolerated" above), and again to add
  `current_round: u64` for the round-test-file naming scheme (see "Round boundary and eval" above). 5
  tests (added a case for a current segment with no game completed yet,
  `completed_through_game_index: None`). Confirmed passing: `cargo test --features training --bin
  rotor-train stream_checkpoint`.
- `src/train/completion_watermark.rs`: `CompletionWatermark` (`new`/`resume`/`mark_complete`/
  `completed_through`), the out-of-order-safe watermark tracker described above -- a `BTreeSet` of
  completed-but-not-yet-contiguous game indices plus a `next_contiguous` cursor that only advances
  through a run of indices it actually has. 5 unit tests (fresh state, in-order advance, out-of-order
  advance only once the gap closes, resume from a checkpointed index, resume from `None`). Confirmed
  passing: `cargo test --features training --bin rotor-train completion_watermark`.
- `src/train/checkpoint_signal.rs`: `CheckpointRequester`/`CheckpointResponder` (built by
  `checkpoint_signal()`) and `CheckpointAck`, the main/GPU thread checkpoint-pausing handshake from
  this doc's own "Main/GPU thread checkpoint handshake" section -- an `Arc<AtomicBool>` request flag
  (`request`/`is_requested`, same convention as `round.rs`'s own `stop_requested`) paired with a
  bounded (`sync_channel(1)`) ack channel carrying `CheckpointAck { round_train_positions_consumed,
  round_loss_sum }` back to the main thread (`acknowledge`/`await_ack`). 4 unit tests (fresh state has
  no pending request, requesting sets it, acknowledging delivers the ack and clears the flag, a second
  request/ack cycle works the same as the first) -- all exercised as plain synchronous calls, no actual
  thread spawning needed since the type itself is what's under test, not concurrent timing. Confirmed
  passing: `cargo test --features training --bin rotor-train checkpoint_signal`.
- `src/train/stream_round.rs`: `run_streaming_training_steps`/`run_streaming_test_pass`, the GPU
  thread's own training-step and eval-pass loops -- the streaming-pipeline counterparts to
  `round.rs`'s `run_training_steps`/`run_test_pass`, adapted from reading `LengthCacheReader` cursors
  to pulling exactly `batch_size` positions per step off the bounded queue (blocking `recv()`, so
  there's no short batch and no cursor to resume) and to reading a fixed `entry_count` of records
  sequentially from the round's handed-off test file instead of sweeping length buckets. Also wires in
  the main/GPU checkpoint handshake: checked once per step, same spot `stop_requested` is, via
  `CheckpointResponder::is_requested`/`acknowledge`. 5 unit tests, same trivial-probe-network pattern
  `round.rs`'s own tests use (a one-layer sigmoid network wrapped as `compute_loss`, real
  `tch::nn::VarStore`/`AdamW`, no mocking of the tensor math itself): reaches the round boundary with a
  finite loss, stops cleanly on `stop_requested` (after training on whatever batch was already pulled),
  stops cleanly when the queue disconnects mid-batch (before any optimizer step), checkpoints exactly
  once when requested and keeps going afterward with the ack carrying the right progress numbers, and
  the test pass reports a finite row-weighted loss over a short final batch. **Revised afterwards** to
  fix the mid-batch deadlock described under "Round boundary and eval" above (bounded `recv_timeout`
  waits instead of a blocking `recv`, re-checking the checkpoint and stop flags on every timeout),
  with a sixth test covering exactly that: a batch that can never be filled still answers a pending
  checkpoint request and then a stop, without ever reaching an optimizer step. That test is worth
  keeping deliberately un-threaded -- it raises both flags up front, the way the Ctrl+C handler does,
  so it stays deterministic; under the old blocking `recv` it hangs rather than fails, which is
  exactly the failure being pinned down. Confirmed passing (6 tests): `cargo test --features training
  --bin rotor-train stream_round`.
- `src/length_cache.rs`: extracted `label_and_featurize_self_play_game` (the per-game AB(lambda) +
  featurization work, pulled out of `build_self_play_positions_and_labels`'s own loop body so both it
  and the new streaming pipeline's PGN threads share one implementation, not two) and added
  `SelfPlaySegmentReader` (`open`/`skip_to`/`next_game`), a `pub(crate)` wrapper around the private
  `Reader`/`SelfPlayGameCollector` machinery so `train/`'s new code never has to touch `pgn_reader`/
  `zstd` types directly. `CollectedSelfPlayGame` promoted from private to `pub(crate)` (fields stay
  private) so it can be named and moved across the new pipeline's channels.
  `build_self_play_positions_and_labels` itself refactored to call the extracted function, behavior
  unchanged (its own existing tests should still pass unmodified). Added tests for
  `SelfPlaySegmentReader` (in-order reads, skip-then-continue reproduces the right next game, skipping
  past the end panics). **Real correction made while implementing this**: the original design assumed
  `Reader::stream_position()` would give a checkpointable byte offset for self-play segments the same
  way it does for the curated path -- confirmed against `zstd::stream::read::Decoder`'s own docs.rs
  page that it implements `Read` but not `Seek`, so that method isn't even callable here. Resolved
  directly with the developer: `SegmentCursor`/`StreamCheckpoint` carry only the game index now:
  resume reopens a segment from byte `0` and skip-replays (discards) games by count, confirmed cheap
  at this format's real scale (`1024 * 128` games/segment).
- `src/length_cache.rs`: added `PgnWorkerPool` (`spawn`/`dispatch`/`recv_result`/`shutdown`), the N
  PGN worker threads themselves -- same reader-to-worker-pool topology `rotor-curate`'s own
  `GameTask`/`GameResult` dispatch already uses (per-worker bounded `mpsc::sync_channel` for dispatch
  out, one shared unbounded channel for results back), each worker just calling
  `label_and_featurize_self_play_game`. Kept in `length_cache.rs` rather than a new `train/` module
  specifically so its tests can build real `CollectedSelfPlayGame` fixtures via the already-`pub(crate)`
  `SelfPlaySegmentReader` (its fields are private outside this module, so a separate module could only
  have tested it against hand-rolled mocks, not real parsed games). Tests: every dispatched game comes
  back exactly once across concurrent workers (no lost or duplicated results), and a pool with nothing
  ever dispatched to it still shuts down cleanly. Later gained `recv_result_timeout` (the timed
  counterpart to `recv_result`), which is what lets the main thread's own dispatch loop notice its
  periodic checkpoint timer, a round-complete signal or a Ctrl+C while no worker has a result ready
  yet, rather than blocking indefinitely on `recv_result`; 2 further tests (times out cleanly with
  nothing ready, returns a result once one is). Confirmed passing: `cargo test --features training
  --bin rotor-train length_cache`, which also re-exercises every pre-existing self-play test against
  the refactor above.
- `src/train/round_router.rs`: `RoundRouter<W: Write>`, the round-boundary self-gating logic from this
  doc's own "Round boundary and eval" section -- `route` (mod-33 split via `SplitCounter`, train
  positions straight to the bounded queue, test positions to the current round's file until
  `test_target` is met, then buffered as overflow), `test_file_complete`, and `start_next_round`
  (hands back the finished writer + its entry count, resets against a fresh writer the caller
  supplies, flushes any held-back overflow into it immediately). Generic over the test file's own
  `Write` so tests drive it against a plain `Vec<u8>` instead of a real file. `new`/`resume` mirror
  `SplitCounter`'s own shape. Unit-tested: train positions reach the queue, test positions cap exactly
  at `test_target` and further ones buffer instead of contaminating the file, `start_next_round` hands
  off the right entry count and flushes overflow into the next round correctly, and a resumed router
  reproduces an uninterrupted one's remaining routing decisions exactly. **Revised** while wiring in
  the main-thread orchestration loop below: `route`'s train-queue send no longer panics on a
  disconnected receiver, only silently drops the record -- the GPU thread can legitimately stop on its
  own (patience exhausted) between one call to this and the next, and by that point the checkpoint is
  already fully up to date (written the instant that round completed), so a stray position arriving a
  beat later with nobody left to consume it must not crash what is otherwise a clean shutdown.
  Confirmed passing (5 tests, including the disconnect-tolerance case): `cargo test --features
  training --bin rotor-train round_router`.
- `src/train/net_config.rs`: `Training::batch_size` split into `training_batch_size` (rows per
  gradient step) and `test_batch_size` (rows per no-grad batch), reconciling the Rust schema with the
  `net.yaml` structure the developer had already moved to earlier in this session -- `amanda.yaml`
  carried the new field names, the code still expected the old one, so every `train` invocation
  panicked at config-parse time before reaching any of this pipeline. `test_batch_size` is what the
  streaming round's own test pass uses, and what `check`/`calibrate`/`correlate`'s sweeps use;
  `training_batch_size` is what feeds the training steps, the queue capacity and `round_train_volume`.
  The two old corpus-cache paths (`run_train_nnue`, `run_train_transformer`) still hand `round.rs`'s
  `run_round` a single batch size for both halves, now `training_batch_size` -- exactly preserving
  their previous behaviour rather than widening `run_round`'s own signature for a path this work
  deprecates. `examples/net_naive.yaml`/`net_nnue.yaml`/`net_transformer.yaml` updated to match
  (`examples/net.yaml` is `transformer_config.rs`'s own older, separate schema -- untouched).
- `src/features.rs`/`src/cache.rs`: `CanonicalPosition`/`CacheRecord` both gained `#[derive(Clone)]` --
  a genuine need surfaced while building the main thread's "last-safe" `RouterSnapshot` (below), which
  has to clone `RoundRouter::buffered_overflow()`'s records into an owned, independent copy. An earlier
  point in this session worked around not having `Clone` in a test rather than adding it; this is the
  real need that justifies adding it now, not scope creep. `round_router.rs`'s own resume test
  (`resume_continues_the_same_split_pattern_as_an_uninterrupted_router`) was restored to use
  `.to_vec()` directly instead of its earlier `Vec::new()` workaround, now that it compiles.

Implemented, main-thread orchestration (all in `src/train/main.rs`; builds clean and the full suite
passes at 247 tests -- but none of this layer has unit tests of its own, so "compiles and doesn't
regress anything else" is the only thing actually verified about it so far, see "Not yet done" below):

- **`parse_job_flags`**: extended to also parse `--threads` (mandatory, no default, `curate.rs`'s own
  `run_generate`/`self_play/main.rs`'s own identical convention), alongside the existing
  `--patience`/`--min-steps`. Required for every `train` invocation regardless of architecture, even
  though the transformer track's own `run_train_transformer` doesn't use it -- keeps parsing simple
  rather than conditional on a config file `parse_job_flags` hasn't loaded yet.
- **`run_train`'s dispatch**: NNUE kinds now call `run_train_streaming` instead of `run_train_nnue`.
  `run_train_nnue` itself is kept, `#[allow(dead_code)]`, not deleted -- same "deprecated, not
  dismissed" treatment as `rotor-prepare` itself, decided directly with the developer for this exact
  case earlier in this session.
- **`round_test_file_path`/`cleanup_stale_round_test_files`/`open_resumable_test_writer`**: the file-
  naming and resume-truncation helpers behind "Round boundary and eval"'s own file-naming scheme
  above.
- **`RouterSnapshot`/`TestFileHandoff`/`build_stream_checkpoint`**: the "last-safe" snapshot type, the
  main-to-GPU-thread round handoff message, and the pure function assembling one `StreamCheckpoint`
  from the main thread's own current state (called at every point this pipeline checkpoints: a round
  boundary, the periodic timer, a stop).
- **Progress reporting** (`style_round_progress_bar`): a real `indicatif` bar for both phases of every
  round -- `{pos}/{len} steps` with ETA and rate while training, `{pos}/{len} rows` likewise over the
  round's own test file -- built from the same hand-rolled `eta_unsmoothed`/`rate` keys
  `rotor-self`'s and `rotor-curate`'s bars already use (indicatif's own `{eta}` is doubly
  exponentially smoothed and not reachable through a public API), including their `s/step` vs
  `step/s` flip at the one-second mark and their singular unit. The training bar's own baseline is
  the round's resume point, so a round picked up mid-way doesn't read artificially fast. **One** bar
  is shared between both threads rather than one built per phase, specifically so the main thread can
  route its own messages (`sweeping {segment}`, the periodic-checkpoint line, the final stop line)
  through `ProgressBar::println`, which draws above a live bar instead of garbling it -- these two
  threads share one terminal, which the old single-threaded path never had to reckon with. Wired only
  after a first real run sat completely silent: both callbacks had been left as no-op closures, so
  nothing would have printed until a round *completed*, i.e. `training_batch_size * steps_per_eval`
  positions in (33.5M, hours), with no way to tell a working run from a wedged one. `print_progress`/
  `clear_progress_line` stay as they are for the corpus-cache paths that still use them.
- **`StreamingGpuThreadArgs`/`run_streaming_gpu_thread`**: the GPU thread's own body -- builds and
  owns its own `VarStore`/`Network`/`AdamW`/`EarlyStopping` entirely locally, loops
  `stream_round::run_streaming_training_steps` -> round-complete handoff -> await the main thread's
  test-file handoff -> `stream_round::run_streaming_test_pass` -> per-round save/report, mirroring
  `run_train_nnue`'s own per-round shape almost exactly.
- **`run_train_streaming`**: the main thread's own entry point -- transformer guard (redundant given
  `run_train`'s own dispatch already routes by architecture kind, kept anyway as an explicit assertion
  per CLAUDE.md rule 4); `preprocessing.parent`/`pgn` validation; an empty-segment-glob guard that
  returns early with a "generate some with rotor-self first" message before anything is spawned or
  written (otherwise a run started before any self-play data exists would leave behind a
  `model.vs`/`optimizer.opt` for a network that never saw a position); a temporary, discarded
  `VarStore`/`AdamW`/`EarlyStopping` load just to evaluate the same "patience already exhausted" early
  exit `run_train_nnue` has, before any thread is spawned; loads or default-initializes a
  `StreamCheckpoint`; spawns the GPU thread and the `PgnWorkerPool`; then the segment-by-segment
  dispatch loop (dispatch a game, drain a ready PGN result with `recv_result_timeout`, handle a
  pending round-complete signal, handle the periodic checkpoint timer, check `stop_requested`) with
  the resume-cursor/watermark handling from "Out-of-order completion is tolerated" above.
  **Checkpoint-handshake shutdown logic, worked out directly while implementing (not pre-specified in
  the design above)**: the main thread only ever calls `CheckpointRequester::await_ack` when it knows
  an ack is actually coming -- either it just called `request` itself (the periodic timer, or the
  final "segments exhausted" checkpoint), or `checkpoint_requester.request_flag()` is independently
  observed still set (meaning Ctrl+C raised it, and the GPU thread is guaranteed to see and ack it
  before it could possibly exit on its own, since `stream_round.rs`'s own per-step check always tests
  the checkpoint flag before the stop flag). It deliberately never calls `await_ack` in reaction to a
  GPU-triggered patience-exhaustion stop, since that path's own round-complete handoff (which always
  fires before the GPU thread sets `stop_requested`) has already written a fully accurate,
  zero-progress checkpoint by the time the main thread could observe the stop flag. The Ctrl+C
  handler itself sets the checkpoint flag *before* the stop flag (not simultaneously) so that SeqCst's
  total ordering guarantees any thread observing `stop_requested == true` afterward also observes
  `checkpoint_requested == true` if it checks -- eliminating what would otherwise be a race.

`split_counter.rs`, `stream_checkpoint.rs`, `round_router.rs`, `completion_watermark.rs`,
`checkpoint_signal.rs`, and `stream_round.rs` are all now referenced from `src/train/main.rs`'s own
orchestration code above -- no more `dead_code` warnings expected for any of them.

Not yet done:

- **An actual end-to-end run against real self-play data** -- the only thing that can validate this
  layer, since neither `run_train_streaming` nor `run_streaming_gpu_thread` has (or can practically
  have) unit tests of its own, exactly as `run_train_nnue`/`run_train_transformer` don't today:
  correctness rests on the tested components they call plus actually running them. Three cases worth
  exercising deliberately, in this order: a **fresh start** (no `stream_checkpoint.state`, watch the
  first round boundary actually rotate the test file and produce a `history.tsv` row), a **Ctrl+C then
  resume** (confirm the resumed run skip-replays the right game count, truncates the test file to the
  checkpointed byte length, and continues the mod-33 split pattern rather than re-syncing), and a
  **"segments exhausted" stop** (confirm it writes a final checkpoint, stops the GPU thread, and that
  rerunning it once a new segment exists picks up without re-walking the processed ones).
- **The `check`/`calibrate`/`correlate` gap** for self-play-streamed networks -- flagged early in this
  document, still entirely unaddressed: those three subcommands read a persistent length-bucketed test
  cache (`length_cache::open_readers_for_stems(&stem_dirs, "test")`) that a network trained purely by
  this streaming pipeline never builds, since its own test sets are per-round and deleted after use.
