# Lc0's conversion: a limit on a single tar's size

## Why

A Convert at 4.61 billion of its 5 billion positions started downloading a 37.87 GiB tar (2026-11-22).
`may_fetch` let it through: its predicted positions fit under `cap_positions`, and staging had room.
Nothing limited one tar's size. The developer stopped the conversion: such a tar

- blocks conversion, which goes in date order, for hours, on one connection, while the
  tars after it would come faster over several;
- could fill most of the remaining cap by itself, from one moment of Lc0's run, where many small tars
  give more variety;
- takes almost 38 GiB of staging.

Typical tars here are 160 to 190 MiB.

## Design

1. `Lc0Config` gains `max_tar_bytes: u64`, required and positive like `staging_bytes`, checked in the
   same validation. `configs/lc0.yaml` sets it to `4294967296` (4 GiB): the largest tar converted before was
   3.74 GiB (`20260128-1817`), and 42 were over 1 GB, so 1 GiB would have refused tars already taken.
2. `may_fetch` gains `max_tar_bytes`. A tar larger than that never may be fetched.
3. **A tar that may never be fetched must not wait for its turn.** `fetch_tars` waits on `may_fetch`
   with `wait_while`, and every later tar waits for `next_approve` to pass it, so a refusal with no
   way out would stall the run for good. So when it is its turn, a tar `may_fetch` refuses for its
   size alone is passed over instead of waited on: `next_approve` moves past it, its slot becomes
   `Slot::Skipped`, nothing is added to `staged_bytes`, and the converter goes on as it does for a
   slow tar. Its line reads `{date}: skipped, {size} larger than max_tar_bytes`.
   The size test is its own small function (`too_large`), called by both `may_fetch` and
   `fetch_tars`, so the two cannot disagree.
4. Not recorded in `SKIPPED_PATH`: the size is known from `Content-Length` before any byte
   downloads, so each run judges it again for free, and raising `max_tar_bytes` takes such tars
   back with no file to edit.
5. Nothing cleans up after the run stopped on 2026-11-22: the developer deletes its partial tar by
   hand (the developer, 2026-11-22).
6. An empty tar (`EMPTY_TAR_BYTES`) is never too large, so it goes on as before.

## Tests

- `may_fetch` refuses a tar of `max_tar_bytes + 1` even into an empty staging, before any training
  tar too, and accepts one of `max_tar_bytes`.
- An empty tar is never too large.
- The config without `max_tar_bytes`, or with `0`, is refused.
