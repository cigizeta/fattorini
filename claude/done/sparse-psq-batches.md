# psq batches: send active indices, build the dense tensors on the GPU

**Done:** psq batches send flat active indices as i32 and are densified on the device (`src/train/batch.rs`). psq_threats needs `index_add` instead of `index_fill` (two threats can share a row), tracked in psq-threats.md and TODO.md.

Status: plan written 2026-09-28, awaiting the developer's approval. Not yet applied.

## Why

Dorothy's first run spent about as long testing as training: 2.2 s to train a round of 512 steps
(524k rows, forward, backward and optimizer step) against 1.4 s to test 537k rows (forward only).
A forward-only pass only 1.6 times faster per row than a full training step means the network is not
the cost; building the batches is.

`batch::to_dense_batch_psq` fills two `[batch, 780]` f32 arrays on the CPU for every batch, about
96% zeros (a psq position has at most 37 active rows per perspective: 32 pieces, 4 castling rights, 1
en passant file), and `DenseBatch::to_device` then copies all of it to the GPU: 6.4 MB per training
batch of 1024, about 3.3 GB per pass over the test set.

## The change

For psq only, the CPU writes the **flat indices** of the active rows (`row * 780 + feature`) instead
of dense floats, copies those to the device, and the device writes the ones into a tensor of zeros.
The result is exactly today's `DenseBatchPsq`: same shapes, same values, bit for bit. The network,
its weights, the export and the engine do not change.

- Copied per training batch: about 1024 * 37 * 2 i32 indices, about 300 KB instead of 6.4 MB.
- CPU work per row: pushing about 64 integers instead of zeroing and filling 1560 floats.
- On the device: one `zeros` and one `index_fill_` per perspective, both trivial for a GPU.

The decode itself (`psq::expand` over each `CanonicalPosition`) stays on the CPU and is unchanged.
If it turns out to be a large part of the cost, this change speeds the round up less than hoped;
the round times printed after every round show it either way.

## Decisions

- **psq only.** It is the only feature set being trained; halfkp, halfka_v2_hm and naive keep
  building dense on the CPU, unchanged.
- **One entry point with a device.** `to_dense_batch_on(records, feature_set, device)` builds the
  batch already on `device`: for psq by the indices route, for the others by building on the CPU and
  moving it, as today. `to_dense_batch(records, feature_set)` stays as `to_dense_batch_on(..., Cpu)`
  for the tests and `export.rs`, so every CPU caller is unchanged.
- **i32 indices over the copy, widened to i64 on the device**, since `index_fill_` needs i64. A flat
  index stays below 2^31 for any batch under 2.7 million rows; asserted.
- **The CPU dense builder for psq goes.** The indices route on the CPU device gives the same tensor,
  so one builder serves both. The existing test that checks the exact active rows keeps covering it.

## Implementation plan

### `src/train/batch.rs`

- `psq_indices(records) -> (Vec<i32>, Vec<i32>, Vec<f32>)`: own flat indices, opponent flat
  indices, labels.
- `to_dense_batch_psq_on(records, device) -> DenseBatchPsq`: the indices to `device`, widened to
  i64, `Tensor::zeros([rows * 780])` on `device`, `index_fill_(0, &indices, 1.0)`, viewed as
  `[rows, 780]`; the label moved to `device`.
- `pub fn to_dense_batch_on(records, feature_set, device) -> DenseBatch`, and `to_dense_batch`
  calling it with `Device::Cpu`.
- Tests: the existing exact-rows test unchanged; a new test that a batch of several different
  positions (castling, en passant, both sides to move) gives, on the CPU, exactly what the old
  per-element fill gives, the old fill kept in the test module as the reference; when CUDA is
  available, the same batch built on CUDA and moved back equals the CPU one.

### `src/train/mod.rs`

- The four NNUE `to_batch`/`compute_prediction` closures (`train`, `check`, `calibrate`,
  `correlate`) call `batch::to_dense_batch_on(records, feature_set, device)` instead of
  `to_dense_batch(...).to_device(device)`.

### Measurement

- Before: dorothy's round line, about 2.2 s + 1.4 s at 512 steps.
- After: the same config for a few rounds; both numbers recorded in this document's progress log.

## Progress

- 2026-09-28: plan written; approved and implemented, not yet built: `to_dense_batch_psq_on`,
  `to_dense_batch_on`, two new tests (the element-by-element reference, CUDA against CPU), the four
  callers in `mod.rs`.
- 2026-09-28: built, tests pass. Measured on dorothy at 4096 steps per round: test pass 1.4-1.7 s
  to 0.6 s (about 2.5x), training 16.8-19.9 s to 14.1-14.8 s (about 1.25x); a round about 19 s to
  about 15 s. Training is now bound elsewhere, about 3.5 ms per step of 1024; the suspect is the
  training loss read back to the CPU after every step, which serializes CPU and GPU.
