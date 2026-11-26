# Lc0's corpus to 10 billion positions: design

Status: being designed (the developer, 2026-11-26). Not before a halfk network is released for
CCRL, nor before 2026's last tar is published, and never while an attempt trains.

## Decided

### Conversion in the order downloads finish, once the test set is complete (2026-11-26)

Today the converter takes the tars in date order (`crates/smithy/src/lc0/mod.rs`): it waits for the
next tar by date however slow its download, and two workarounds exist only for that wait, the skip
of a tar slower than 256 KiB/s over its first two minutes (122 tars in `skipped.tsv`) and, in part,
`max_tar_bytes` (one tar of 37.87 GiB held the conversion up for hours, 2026-11-22).

- **While `test/` is short of `test.games`, date order, as today:** the earliest tars are the test
  set, which matters only for a corpus started from nothing.
- **Once the test set is complete, the converter takes whichever approved tar finishes downloading
  first.** Every tar then goes to `train/`. The frozen test set is complete today, so the whole move
  to 10B runs this way.
- **The corpus is then a set of tars, not a prefix of dates.** That already works: a run finds what
  is left by comparing the listing with `tars.tsv`'s rows, so a later run takes every tar without a
  row, holes included. Training shuffles positions into buckets, so segment order never mattered.
- **The cap stays checked in approval order:** converted positions plus every approved tar's
  predicted ones; once a tar would pass the cap nothing more is approved, and the approved downloads
  finish and convert.

**The slow-tar skip stays as it is (the developer, 2026-11-26):** a tar below 256 KiB/s over its
first two minutes is still skipped and recorded in `skipped.tsv`, so a stuck download never holds
one of the three connections for long. Without date order it no longer stalls the conversion: the
other two connections and the converter go on.

### Every skipped tar retried, none over 4 GiB (2026-11-26)

- **The tars skipped as too slow are retried** at the resumed Convert: `data/lc0/skipped.tsv`'s rows
  deleted on Drive before it starts (122 rows on 2026-11-23), no code needed. The Lc0 server may have
  solved its speed problem since (TODO.md, 2026-11-23). **Retried, not insisted on (the developer,
  2026-11-26):** one more chance each, under the same skip rule; a tar slow again is recorded again
  and passed over for good, as `GIVE_UP_AFTER` (1) already does. Its cost if every one is still
  slow: two minutes each on one of three connections, about 80 minutes for 122 tars, taken once.
  The developer's bet (2026-11-26): Lc0 has fixed its server, so most of them download normally.
  The resumed Convert's `skipped.tsv` shows how many were slow again.
- **`max_tar_bytes` stays at 4 GiB:** tars above it are still passed over. They were never in
  `skipped.tsv` (a too-large tar is passed over with no row nor record, at every run), so deleting
  its rows does not bring them back.

### The window: 2026's tars alone, up to 10 billion (2026-11-26)

Every test91 tar of 2026, none over 4 GiB, until `train/` reaches 10 billion training positions or
2026's tars run out, whichever comes first (the developer).

- **`cap_positions`: 10000000000** in `configs/lc0.yaml`.
- **An end to the window,** new: today's `from` has no counterpart, and the Convert takes every tar
  the listing has from `from` on. A key such as `until: 20270101-0000` (the first tar left out),
  the listing cut there, its validation and tests: a small code change, for the plan.
- **Exhausting 2026 before the cap is a normal end,** not a failure: the Convert says so and writes
  `complete.txt`, as at the cap, so that `lc0-` runs may start. Today, running out of tars ends the
  run without `complete.txt`, since a later run could still find newer tars; with an end to the
  window, none can come. For the plan.
- **What it means for timing:** at about 0.75B a month (TODO.md, 2026-11-23), 2026 may fall a little
  short of 10B, the cap then unreached. The Convert runs after 2026's last tar is published, so in
  January 2027 at the earliest.

### Drive's room (2026-11-26)

About 200 GiB in `data/lc0` at 10B (about 21.5 bytes a position), double today's: room enough on
Drive (the developer). Nothing to plan.

### The budget: 4 passes, as on 5B (2026-11-26)

Configs keep `epochs: 4` (the developer): each position seen four times, as on 5B, so about 1,184
rounds of 4096 steps at batch 8192, about 19 hours an attempt against 9.5 on 5B. Every experiment
on 10B costs twice as much GPU time. The round robin's checkpoints are unchanged, every quarter
epoch from 2 to 4. Width 512 on 5B was still improving at the end of its four passes, so four
passes over twice the data is the natural next step.

### The training machine's local disk (2026-11-26)

Every `lc0-` config scatters into one folder, `data/buckets/lc0` (`smithy.rs`, `source_buckets`),
so the 10B buckets replace the 5B ones: the peak is the 10B size alone.

- **Estimated, from the format:** a record is 14 + 2p bytes before compression, p the pieces beside
  the kings (`cache::write_position`, then two labels and the progress, `buckets.rs`), about 54 at
  p = 20; zstd at level 1 perhaps halves it: **about 25 to 35 bytes a position, roughly 250 to 350
  GB at 10B.**
- **A precondition, on the training machine before the Convert:** the exact bytes a position from
  `buckets.tsv`, and the free space plus what `data/buckets/lc0` takes today, at least that:

      awk -F'\t' '$1 == "bucket" { bytes += $3; records += $4 } END { printf "%.1f bytes a position, %.0f GiB at 10B\n", bytes / records, bytes / records * 1e10 / 2^30 }' data/buckets/lc0/buckets.tsv

      du -sh data/buckets/lc0

### The lambda polyline re-measured (2026-11-26)

As for 5B (the developer): `data/lc0/polyline.txt` and the frozen set's `lambda-polyline/` deleted
on Drive once the Convert is complete, so the first 10B attempt measures the polyline on the whole
corpus and labels the frozen set again. The polyline fits the corpus it labels, and the second half
comes from later test91 networks. Its cost: the measurement, and the scatter starting over
(buckets labelled otherwise are scattered again from the start, `buckets.rs`). Test losses are then
comparable among 10B attempts alone; only duels compare across corpora.

### When: after a halfk release (2026-11-26)

**The 10B Convert waits until a halfk network has been released for CCRL** (the developer). The
release's network is trained on 5B, so the halfk line's experiments (the head, the buckets, the
retrain with castling and en passant) all finish on 5B first, and 10B starts from a released
baseline. January 2027 is only the earliest date the data allows; the release decides.

### The teacher's drift, accepted (2026-11-26)

The second half of 2026 comes from later test91 networks, likely stronger, and from smaller tars
(about 0.13 GB an hour in October against 0.8 GB in January), so more hours of Lc0's play a
position. Accepted as before (the developer), as lc0-data.md accepted test91's networks over its
months: one corpus, one window, nothing weighted.

## For the implementation plan

The code changes, all in the Convert (`crates/smithy/src/lc0/mod.rs`) and `configs/lc0.yaml`:

1. Conversion in the order downloads finish once the test set is complete; date order while it is
   short. The cap still checked in approval order.
2. `until`, the window's end, and running out of the window's tars writing `complete.txt`.
3. `cap_positions` 10000000000 and `until: 20270101-0000` in `configs/lc0.yaml`, applied only when
   the Convert is to run: the config is read once at its start.

By hand, in order, once a halfk network is released for CCRL and 2026's last tar is published:

1. On the training machine, the disk precondition above.
2. On Drive: `skipped.tsv`'s rows deleted, then `complete.txt`.
3. The Convert started from the page, run to its end.
4. On Drive: `data/lc0/polyline.txt` and the frozen set's `lambda-polyline/` deleted.
5. The first 10B attempt, which measures the polyline and scatters from the start.
