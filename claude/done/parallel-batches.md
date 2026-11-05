# Parallel batches: build batches on every core while the GPU steps

**Done:** the reader thread and batches built on every core are implemented and merged; caroline's run uses them. The motivation (betty's big five inputs at 6% GPU) is historical: the big five was retired 2026-10-03, and the name betty now means psq plus a hidden 16.

## Status

Agreed with the developer, 2026-10-02: betty's generation 1 training was stopped and deleted (6% GPU)
to implement this first. Implemented, not yet compiled or tested.

## Problem

`round::run_training_steps` does everything on one thread, in turn: read a batch of records
(`TrainStream::next_batch`: zstd read plus the shuffle buffer), build its tensors (`to_batch`), step
the GPU. The GPU waits while the batch is built. Amanda (naive inputs, cheap) ran at about 27% GPU;
betty (the big five's inputs: the engine's `position_inputs` on a rebuilt board per position) at 6%.
The test pass has the same shape.

## Decided

- **A pipeline, three stages** (`pipeline.rs`, one function used by the training half and the test
  pass):
  1. One **reader** thread calls the stream, in order. The stream and its RNG are touched by this
     one thread only, so the sequence of record batches is exactly what the single-threaded loop
     read.
  2. **Workers**, one per logical core minus two (the reader and the main thread), at least one, each
     turn a whole batch of records into its host-side form (`prepare`): the CPU work, all of it.
     Parallel across batches, not within one: a batch's construction is unchanged code.
  3. The **main thread** takes the prepared batches back in their original order (a reorder map by
     sequence number), uploads each (`upload`: `Tensor::from_slice`, `to_device`), and steps. All
     tensor and GPU work stays on the main thread.
- **Bounded**: both channels hold a few batches per worker, so memory stays bounded and the reader
  never runs far ahead.
- **Exactly the batches the round needs.** The training half knows its step count up front (up to the
  next multiple of `steps_per_round`, always at least one), so the reader reads exactly that many
  batches and the stream ends the round where the single-threaded loop would have: the next round
  continues from the same place. A batch's cursors (`TrainStream::cursors`, the transformer's resume
  state) are read right after it, as today, and travel with it.
- **Stop**: when the main thread stops (Ctrl+C), it drops its end; workers and reader see their
  sends fail and exit. Batches read ahead are lost, as today's in-flight batch is; the NNUE resume
  derives its position from the step count, and the transformer's from the cursors of the last
  consumed batch.
- **Host and upload split** (`batch.rs`): `HostBatch` per feature set holds the plain vectors;
  `host_batch` builds it, `upload` makes the `DenseBatch`. `to_dense_batch_on` is the two in a row,
  so every other caller (check, calibrate, correlate, export, tests) is unchanged.
- **RNG**: `rand::rng()` is tied to its thread; the streams take a `StdRng` seeded from it
  instead. A run was already randomly seeded, so the data distribution is unchanged.
- **The transformer** passes its records through unchanged (`prepare` clones them) and builds its
  batch in `upload`, as today: it compiles and behaves as before, with only the read overlapped.

## Measured after the pipeline (2026-10-02): the step is the bottleneck

Betty with the pipeline: still 6% GPU, `rotor-train` at 187% CPU. Per thread: main 66%, PyTorch's
autograd thread 34%, the reader 9%, the 18 workers 1-2% each. So batches were never betty's limit:
the step is. The big five is small, and its step is dozens of tiny kernels, forward, backward and
optimizer; launching them is the cost, and the GPU idles between them. Decided with the developer:
no larger batch (unfair to amanda, whose batch it would change), every other fix.

- **No loss readback per step** (`round.rs`). `f64::try_from(&loss)` every step made the main
  thread wait for the GPU to finish before queueing the next step. The weighted losses are summed
  on the device (in double) and read only at a backup and at the end of the round; the test pass
  reads once at its end.
- **Both sides in one set of bags** (`big_five_network.rs`). The side to move's inputs in rows
  `0..n`, the opponent's in `n..2n`: each component is one `index_select` and one `index_add` for
  both sides instead of two, halving the model's launches forward and backward. Same weights, same
  variables, same export; only the float summation order inside a scatter-add can differ.
- **Next, if still launch-bound: the optimizer.** `AdamW::step` updates the big five's 15
  parameters one by one, about a dozen kernels each: some 180 launches a step, likely more than the
  model. `tch` 0.24 exposes neither `_fused_adamw_` nor the `_foreach_` ops, so fusing means
  flattening the parameters by hand. Measured first.

## Not changed

The data: same stream, same order, same batch contents. GPU results are not bit-identical run to run
in any case (`index_add` on CUDA accumulates in no fixed order).

## Verification

```
cargo test --features training --bin rotor-train
```

Then betty's training restarted: round time and GPU utilization against the 6%.
