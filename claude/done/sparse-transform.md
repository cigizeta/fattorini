# Sparse transform for psq and psq_threats

**Status:** done 2026-10-14; generation 3 trains with it on the training machine, much faster by the developer's
observation (no figure recorded). Earlier status (2026-10-12): implemented on the Lichess machine, tests passing
(the CUDA one skipped there), not yet measured. Written and
implemented while generation 3's replay ran; to be measured on the training machine before caroline's and
dorothy's first training.

As implemented: `DenseBatchPsq` keeps one index list for both perspectives (`flat`, `features`,
`offsets` over `2 * rows` bags) and gives the dense rows on request (`dense()`, for the unstacked
`Network` and the tests); `StackedNetwork::sparse_transform` is the `embedding_bag`, and
`dense_pawns` (tests only) the matmul it replaced. Tests: `batch.rs`'s
`each_bag_holds_exactly_its_rows_active_features` and the dense-view tests; `stacked_network.rs`'s
`the_sparse_transform_computes_what_the_dense_one_does`, `..._on_cuda_...`,
`sparse_and_dense_stay_together_over_optimizer_steps`, and the gradient test extended to
psq_threats.

## Why

A generation's replay and self-play are paid once, but training is paid once per architecture, one
after another on the training machine's single GPU: generation 3 trains amanda, betty, caroline and dorothy on one
corpus. With the arms stacked (stacked-lockstep-arms.md) the GPU runs at 90%, so a round is bound by
arithmetic, and most of it is the feature transform, computed dense:

- `batch::psq_on` writes the ones into a zero `[rows, width]` tensor per perspective (`index_fill`),
- `StackedNetwork::transform` multiplies it by every arm's weight side by side:
  `addmm(input [rows, width], weight [arms * embedding, width]^T)`, forward and backward.

Almost all of it multiplies zeros:

| | width | active a perspective | density | dense / sparse work |
|---|---|---|---|---|
| psq (amanda, betty) | 780 | at most 37 | about 5% | about 20x |
| psq_threats (caroline, dorothy) | 4,876 | about 37 + 10 to 30 | about 1 to 1.5% | about 70 to 100x |

psq_threats's dense transform is about 6x psq's, while its sparse work is only about 1.5 to 2x. So
psq_threats gains the most, and it is the kind that would otherwise train slowest.

The goal is speed only. The trained network is the same function of the same data, up to float
summation order: no strength is expected from this change itself, only time.

## Design

### The batch carries indices, not dense rows

`DenseBatchPsq` keeps the flat active indices the host batch already has (`active_indices`:
`row * width + feature`, sorted by row, side to move first) instead of the dense tensors:

- `features`: `Int64 [nnz]`, `index % width`, on the device,
- `offsets`: `Int64 [rows]`, the position of each row's first index (rows sorted, so a cumulative sum
  of per-row counts),
- `label`, as today.

Both are derived on the device from the uploaded `i32` indices (a divide, a remainder, a `bincount`
and a `cumsum`), so the host side, the reader thread, `HostBatch` and the test cache
(test-features-once.md) are unchanged. `index_fill` and the dense `[rows, width]` tensors are no
longer built in training or in the test pass.

`DenseBatch::inputs()` serves the dense kinds only. psq gets its own accessor. halfkp and
halfka_v2_hm stay dense; the TODO's "active indices instead of dense rows" item for them is
separate, and could reuse this transform later.

### The stacked transform is one `embedding_bag`

`StackedNetwork::transform` for psq and psq_threats:

1. `table = weight.view([arms * embedding, width]).tr()`, i.e. `[width, arms * embedding]`: row
   `feature` holds every arm's column for it side by side, so one gather serves all arms. It is
   made contiguous once a step (`embedding_bag` needs it): a copy of the weight, about 9 MB for
   psq's eleven arms of 256 and 55 MB for psq_threats's, small against the matmul it replaces.
2. `embedding_bag(table, features, offsets, mode = sum, sparse = false)`: `[rows, arms * embedding]`,
   the sum of each row's active weight columns. Fused gather and sum, so the `[nnz, arms *
   embedding]` intermediate (1.7 GB for psq at batch 4096, eleven arms) is never materialized.
   This rules out the simpler `index_select` plus `index_add`.
3. Plus the bias, then the same `view`, `permute` and activation as today.

**Both perspectives in one call**: own and opponent are concatenated as `2 * rows` bags (the
opponent's offsets shifted by the own `nnz`) and split after, so the table is made contiguous once
and the kernel runs once per step.

### Gradient and optimizer: dense, unchanged

`sparse = false`: the backward returns a dense gradient for the whole table, zero on the rows no
batch touched, exactly the gradient the dense matmul gives. `StackedAdamW` is untouched, and so is
the training: weight decay and the moments still move every row every step, as today.

A lazy (row-sparse) AdamW is not part of this change. At 780 rows, nearly every row is touched by
a batch of 4096, so it would buy almost nothing for psq. For psq_threats many threat rows are
untouched in a given batch, so it would save more there, but it changes what is trained (untouched
rows would skip their decay and momentum), so it would be a separate experiment, judged by games.

### The unstacked `Network` stays dense

The unstacked `Network` is not used for training (`mod.rs` builds a `StackedNetwork`); it serves
export and is the oracle of `stacked_network.rs`'s tests. It keeps its dense matmul, fed by a test-
and export-only helper that builds the dense rows from `features`/`offsets` (the current `index_fill`
moved there). That keeps an independent dense reference for the equivalence tests below.

## Tests (`cargo test`)

1. **The batch**: `features`/`offsets` recover exactly the dense rows `psq_on` builds today, for psq
   and psq_threats, including a row with no threat and the first and last rows of a batch.
2. **Forward**: the sparse stacked transform equals the dense `addmm` within the existing tolerance
   (`allclose(1e-5, 1e-6)`), for psq and psq_threats, with and without a hidden layer, three arms.
   The existing `every_arm_computes_what_its_own_unstacked_network_does` already checks this against
   the dense oracle once the stack goes sparse; it stays as is.
3. **Gradient**: the existing `the_summed_objective_gives_every_arm_its_own_gradient` gains
   PsqThreats, and checks the transform's gradient against the dense oracle, including rows no
   position touches (zero gradient).
4. **Optimizer steps**: a few `StackedAdamW` steps from equal weights, sparse against dense, stay
   within tolerance: the training is the same, not only one step.
5. **CUDA**: the forward test on `Device::Cuda(0)` when present, as `batch.rs` already does.

The CUDA backward of `embedding_bag` accumulates with atomics, so runs are not bit-identical; they
were not under the dense matmul either. Tolerance, not equality, is the criterion throughout.

## Measurement

On the training machine, after the replay and self-play (the GPU is free now, but the CPUs, which build the
batches, are not):

- `history.tsv`'s train seconds per round before and after, for betty (eleven arms, 12.7 s a round
  dense, 2026-10-11) and for one psq_threats run (caroline or dorothy) on its first rounds,
- the GPU utilization: if the transform was most of a round, the GPU should drop well below 90% and
  the reader thread and batch building may become the limit; that is then the next bottleneck, not
  a failure of this change,
- the test pass time, which loses its dense build too.

No duel: the networks are the same function of the same data.

## Implementation steps

1. `batch.rs`: `DenseBatchPsq` holds `features`/`offsets`/`label`; `psq_on` derives them on the
   device; the dense builder moves to a helper for the unstacked `Network`; batch tests updated.
2. `stacked_network.rs`: the psq transform through `embedding_bag`, both perspectives in one call;
   halfkp and halfka_v2_hm keep `addmm`.
3. `nnue_network.rs` and `export.rs`: the unstacked `Network` reads the dense helper.
4. Tests 1 to 5 above.
5. Measure on the training machine once it is free; then this doc moves to `done/`.

## Open

- Whether `embedding_bag`'s dense backward through `tch` is as fast as hoped on the training machine's GPU, or a
  hand-written `index_add` backward is needed. Decided by the measurement, not in advance.
- Storing the transform weight transposed (`[width, arms * embedding]`) would remove the per-step
  contiguous copy, but changes the parameter's shape and so `arm_slice` and export. Only if the copy
  shows in the profile.
