# Batch 8192 and the first attempts on the larger corpus

**Status:** designed 2026-11-19 (the developer), plan below; nothing written yet. Two configs and
TODO's order, no code expected.

**Terms:** from 2026-11-19 a training run is an **attempt** (the developer). Older documents, the
code and candidate names (`-r2`) keep "run" until renamed, if ever.

## The decisions (the developer, 2026-11-19)

1. **Batch 8192 on the ~4.3B corpus, adopted, not judged by a duel.** The batch of 4096 was chosen
   for 1.2B positions, where each counts; Stockfish trains on far larger batches, and 4.3B affords
   8192. Measured on 2026-11-18 (`-hidden-512`, batch 4096): the GPU 85 to 88% busy, at about 70% of
   its power limit, most of its memory free, so a larger batch is the way to fill it.
2. **The rate does not change with the batch:** 1e-3 to 1e-5, geometric, as at 4096 (the developer
   always chooses the rate regardless of the batch). No square-root or linear scaling rule.
3. **A round stays 4096 steps,** so 4096 x 8192, about 33.5M positions. Not 2048 steps: that would
   only keep positions per round equal for the charts, while the round is a unit of work (warmup,
   test passes and checkpoints count steps). Over 4 epochs of ~4.3B: about 2.1M steps, about 256
   rounds.
4. **The first two attempts, at width 512 with 8 king buckets:**
   1. `lc0-halfk-threats-screlu-hidden-512-8-budget`
   2. `lc0-halfk-threats-pairwise-hidden-512-8-budget`

   The king buckets before `-budget` (the developer, 2026-11-19): `-512-8-budget`, not `-512-budget-8`.

   Both: batch 8192, `schedule: budget`, 4 epochs, 1 warmup round, the nine candidates and their
   round robin as fixed-budget-schedule.md. Width 256 may never run (the developer); its configs
   `-hidden-8-budget` and `-10-budget` (halfk-king-buckets-implementation.md, step 8, renamed the same
   day) stay unscheduled.

## What the attempts can tell

- **Pairwise against screlu-hidden,** both at 512 with 8 king buckets: one change, a clean duel at
  depth 9 then 10+0.1. TODO's prediction (the halfk line, step 4) still applies: clearly better than
  test 15's -30 Elo, the theory of pairwise's width holds; about -30 again, it fails.
- **The winner against `lc0-halfk-threats-screlu-hidden-g1-r1`,** today's best, mixes four changes
  (width, king buckets, corpus with batch, schedule): enough to decide what ships and plays on
  Lichess, not which change paid. Accepted (the developer, 2026-11-19).

## Risk

Same rate, half the updates over the same 4 epochs. If an attempt trains worse than expected (its
loss at an epoch above the 1.2B attempts' at the same epoch, or flat early), the first suspect is
the count of updates, and the fix the endpoints, not the batch.

## Checked 2026-11-19

- **Nothing hard-codes 4096:** `budget.rs` derives the steps of an epoch from `training_batch_size`
  (`train_rows / training_batch_size`), `lockstep.rs` reads it from the config; the 4096s left are
  tests and feature counts.
- `run_config.txt` freezes the batch (`net_config.rs`), so an attempt cannot change it on a resume.

## Plan

1. **`configs/lc0-halfk-threats-screlu-hidden-512-8-budget.yaml`:** a copy of
   `lc0-halfk-threats-screlu-hidden-512-budget.yaml` with `king_buckets: 8` and
   `training_batch_size: 8192`; its header comment saying what it differs by, and why, and the
   network line's row counts for 8 buckets at 512.
2. **`configs/lc0-halfk-threats-pairwise-hidden-512-8-budget.yaml`:** from
   `lc0-halfk-threats-pairwise-hidden.yaml`'s architecture (`kind: halfk_threats_pairwise_hidden`)
   with `embedding_dim: 512`, `king_buckets: 8`, and the training block of step 1; differing from
   step 1's config by its kind alone.
3. **No scaled smoke attempt (the developer, 2026-11-19):** the GPU machine converts until the
   corpus is whole, and memory is no worry: at batch 4096 the 32-bucket `-hidden-512` left most of it
   free, 8 buckets cut the transform's rows from 27328 to 10432, and a doubled batch adds only
   activations (8192 x 512 lanes a perspective, a few tens of MB). The full attempt's first rounds
   check it.
4. **On the first full attempt:** GPU busy and time per round logged against 4096's (85 to 88%), so
   the gain is seen, not assumed.
5. **Comparing attempts across batches** reads epochs, not rounds (budget-loss-chart.md to check).
6. **TODO.md:** "Next" and "While Lc0's Convert runs" updated to this order; "To design"'s batch item
   pointing here.

