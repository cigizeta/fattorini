# Lc0's cap in training positions

**Status:** written, 2026-11-18, steps 1 to 4; step 5's Convert waits for the order below.

## The order (complete.txt)

A complete corpus is never extended: while `data/lc0/complete.txt` is on the remote the Convert
says done, and while it is not, no full run trains on Lc0 (new or resumed). The code may go live
any time: training does not read the cap. Then:

1. Any Lc0 run still going finishes on today's 1.2 billion. **No budget validation run on it**
   (the developer, 2026-11-19): the scaled runs proved the code end to end, and the budget runs train
   on the expanded corpus alone.
2. `complete.txt` deleted by hand on the remote; the Convert started from the page.
3. At the cap the Convert writes `complete.txt` again, and Lc0 runs may start. Scaled runs train
   meanwhile, so the king buckets' code is smoke-tested then.

## Why

The halfk line wants more data before the king buckets are compared (halfk-king-buckets.md): which
layout wins depends on the data it trains on, and the next corpus is at least 4 x 1024^3 training
positions (the developer, 2026-11-18). Training sees positions, not bytes, so the cap counts them.
At about 21.5 bytes a position, about 86 GiB on Drive.

## Design

- **`cap_positions` replaces `cap_bytes`** in `configs/lc0.yaml`, set to 4294967296. The config
  refuses `cap_bytes`, so a stale key is not silently ignored.
- **The cap counts training positions alone:** the `train` rows' `positions`, as the summary's
  `train_positions` does. The test tars are frozen apart and do not count.
- **The prediction:** `predicted` becomes a tar's predicted training positions, the past train rows'
  positions per tar byte times its `tar_bytes`. With no past row, the first tar goes, as today.
- `passes_cap`, `may_fetch` and `decide` compare positions: what the train rows hold, plus what is
  ahead (downloaded, not yet converted, in predicted positions), plus the tar's prediction.
- **The page:** `lc0::cap_bytes` becomes `lc0::cap_positions`, `[done, total]` in training
  positions; the catalog's line shows them as positions (e.g. "1.20 B of 4.29 B positions").
- **`staging_bytes` stays bytes:** it bounds the disk, not the corpus.
- Raising the cap only appends tars: the rows done, the corpus and the frozen test set are kept.

## Plan

1. `lc0/mod.rs`: the config field, its validation (positive), `predicted`, `passes_cap`,
   `may_fetch`, `decide`, the run's progress line ("% of the cap"), the `Flow` on the board, the
   module's doc comment; `ahead` in predicted positions where it is bytes today.
2. `smithy.rs`: `SourceEntry.cap` from `lc0::cap_positions`, its doc comment; `page.html` the cap's
   unit.
3. `configs/lc0.yaml`: the key, its comment rewritten (the reason: positions, 4 x 1024^3, about 86
   GiB).
4. Tests: the cap's tests rewritten in positions; a config with `cap_bytes` refused; train rows
   alone counted (a test row's positions do not move the cap).
5. Run: `cargo test -p smithy`, clippy, then a Convert from the page, watched past one tar.
