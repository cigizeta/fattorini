# The king stages' training speed

**Status:** design agreed 2026-10-26 (step 1 first, a 20% bar, TF32 out of scope). Nothing
step 1 done; step 2 done, both candidates slower than today's (below). **C added (the
developer, 2026-10-26):** the delta as an `embedding_bag`, the transform's own sparse sum
(`bagged_stage_forward`), in the equality test and the timing. **Ended 2026-10-26, nothing kept:**
all three exact, all slower on l1 (the idle machine's run below); today's every-bucket product stays.
The candidates, their equality test and `stage_timing` removed from `stack_head.rs` the same day
(the developer); git's history keeps them. To be archived.

## Step 2's result (2026-10-26, buckets skewed; no build or training, but a video-call app running)

| | l1 ms a step | l2 ms a step |
|---|---|---|
| today's, every bucket | 17.98 | 2.03 |
| A, grouped | 65.25 | 10.54 |
| B, padded | 30.96 | 2.86 |
| the floor (1 bucket) | 2.83 | |

**The verdict's numbers (2026-10-26, three runs back to back, the machine checked idle: no video
call, no smithy, arena, build or training, plugged in), min to max:**

| | l1 ms a step | l2 ms a step |
|---|---|---|
| today's, every bucket | 17.98 to 18.33 | 2.03 to 2.04 |
| A, grouped | 65.09 to 65.85 | 10.03 to 12.99 |
| B, padded | 30.77 to 32.14 | 2.83 to 2.92 |
| C, bagged | 53.04 to 53.21 | 1.41 to 1.42 |
| the floor (1 bucket) | 2.84 | |

The runs before (one with a video call on the machine, one after it moved) fall within these ranges
or within 1% of them. C wins on l2 only,
0.6 ms a step, about 2.5 s of a 197 s round, about 1%, far under the bar; on l1 its backward pass
scatters 9 x 4096 x 512 weighted rows into the table, slower than one dense product. Today's
product, 64x the useful arithmetic but one dense `bmm` each way, is what the GPU does best at this
size; beating it would take a custom kernel, out of scope.

Both exact (the equality test passes), both slower. A pays a host read of the counts and up to 64
small products each way, launches not arithmetic. B pads every bucket to the largest, about a third
of the batch: about 64 x 1400 rows multiplied for 4096 used, more than today's 64x.

## Step 1's result (2026-10-26, `stack_head::tests::stage_timing`)

| | ms a step | s a round (4096 steps) |
|---|---|---|
| l1, 64 buckets (today) | 18.26 | 74.8 |
| l1, 1 bucket (the floor) | 2.85 | 11.7 |
| l2, 64 buckets | 2.03 | 8.3 |

l1 is about 75 of the 106 s; the floor puts about 60 s within reach, well past the 20% bar. l2 is
small; it is timed grouped and padded too, for completeness. The remaining ~23 s (clamps, output,
the optimizer on the deltas) is not this doc's. The timing's buckets were uniform; step 2's are
skewed as kings' squares (a third on one, a tenth on two others), synthetic, not from a batch. Not applied to a running
generation: kings' generation 1 (psq-threats-screlu-kings.md) trains on today's code to its end.

## Why

Kings' round 1 took 197.4 s, SCReLU's 91.4 s on the same trainer: about 106 s a round is the head,
about 11 h for a generation where SCReLU's took about 5. Everything else (batches, the sparse input,
the accumulator) is shared with SCReLU, so ~91 s is the floor: at most about 2x, realistically 1.4x
to 1.7x (110 to 140 s a round). Worth it only if kings wins its depth 9 duel (a generation 2 then
follows) or another king-bucketed head is trained; the design costs nothing to have ready.

## Where the time goes (estimated, not profiled)

`stacked_stage_forward` (`crates/smithy/src/stack_head.rs`), all 9 arms at once, batch 4096:

- **l1, 512 -> 16 over 64 buckets:** one `bmm` against every bucket's delta side by side, `[9, 4096,
  512] x [9, 512, 1024]`, about 19 G multiply-adds forward, twice that backward; 1/64 of it used.
- **The `[9, 4096, 1024]` intermediate:** about 150 MB written, read by `gather`, kept for the
  backward pass; its gradient a dense tensor of the same size, 63/64 zeros, then multiplied back into
  the deltas' gradient. On a consumer GPU this traffic may cost as much as the arithmetic.
- **l2, 16 -> 16 over 64 buckets:** `[9, 4096, 16] x [9, 16, 1024]`, 32x smaller than l1; and the
  plain output, negligible.

So l1 is the target; l2 stays as it is unless the timing says otherwise.

## Candidates

**A. Grouped by bucket (the main candidate).** Per batch, one `argsort` of the bucket column (the
same for every arm) and the counts per bucket (`bincount`); the rows permuted into bucket order;
for each non-empty bucket b, one `[9, n_b, 512] x [9, 512, 16]` product against `delta[:, b]`; the
results concatenated and permuted back. The shared part stays one `baddbmm` over all rows, as now.
Exactly the useful arithmetic, no `[.., 1024]` intermediate, the backward pass by autograd. Its
cost: up to 64 small products a step forward, more backward, about 64 rows a bucket on average but
skewed (castled kings hold most rows), so launch overhead, not arithmetic, decides it. Only the
timing can tell.

**B. Padded buckets in one product.** As A, but each bucket's rows padded to the batch's largest
bucket, `[9 * 64, max_n, 512] x [9 * 64, 512, 16]`, one `bmm`. One launch, but the skew makes
`max_n` large (a castled king's bucket may hold a quarter of the batch), so padding may waste more
than the every-bucket product saves. Timed beside A, cheap to write once A's permutation exists.

**Out of scope: TF32** (the developer, 2026-10-26). Allowing it would speed every f32 product, but
at a 10-bit mantissa inside each, changing every network's numerics, not just kings'. This doc keeps
to A and B, exact up to summation order.

**Rejected: per-row weights.** Gathering each row's `[16, 512]` weights, then one batched product:
`[9, 4096, 16, 512]` floats are about 1.2 GB a step, worse than the intermediate it replaces.

## Correctness

A and B compute exactly the same function as `stacked_stage_forward`, up to float summation order:

- a test, as `stacked_stage_forward`'s against `stage_forward`, that each candidate's output and its
  gradients for `x`, the shared weight and bias and the deltas agree with today's within 1e-5, on
  random inputs with every bucket present, some empty, and one bucket holding most rows;
- the existing tests (one row by hand, the export against the engine through a file) unchanged.

Equal within float noise, so a run resumed across the change would not notice it; still, it is
deployed between generations, never during one.

## The timing test

An `#[ignore]`d test in `stack_head.rs`, run on the training machine with the GPU free (no forge
running):

- l1 at the config's shape (9 arms, 4096 rows, 512 -> 16, 64 buckets), forward and backward, 200
  steps after 20 of warm-up, `Cuda::synchronize` around the timed loop: today's every-bucket product,
  A and B;
- the bucket column drawn from a real batch's king squares (so the skew is real), not uniform;
- printed: ms a step for each, and l2's every-bucket cost for scale.

Decision: the fastest of A and B replaces the every-bucket product in `stacked_stage_forward` (and
`stage_forward`, the test reference, stays as is) if it saves at least 20% of the step (the bar agreed, the developer, 2026-10-26); otherwise
nothing changes and this doc is archived.

## Plan (after the design is agreed)

1. The timing test with today's product only, run once: confirms l1 is where the 106 s goes
   (decided first, the developer, 2026-10-26); A and B are written only if it does.
2. A and B beside it, with the equality test.
3. The developer runs the timing test on the training machine when it is free.
4. The winner into `stacked_stage_forward`, deployed between generations; the next round times
   against kings' generation 1 (`history.tsv`'s `train_duration_secs`).
