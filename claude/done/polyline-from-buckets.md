# The polyline measured from the buckets

Designed 2026-11-04 with the developer. Supersedes the order in polyline-whole-corpus.md (a pass of
its own over the segments, through Drive, before the scatter): a corpus is read once, by the scatter.

## The run's order

1. The share cut (a scaled run).
2. The scatter, which resumes as before.
3. The polyline measured from the buckets, unless `polyline.txt` exists.
4. The frozen test set, whose polyline labels need it.
5. Training.

## The measurement

A record holds `s`, `z` and its progress, both labels from the side to move, so `polyline::measure`
reads every bucket file, one thread per bucket at a time, and adds to the same `Moments`.

- **Left out:** a record with `|s| = 1` and `s = z`: mate scores, and the tables' wins and losses.
- **Kept, knowingly:** the tables' draws. A record holds `s = z = 0` for them, the same as a 0.00
  score in a drawn game, a common ordinary estimate, so the record alone cannot tell them apart.
  Measuring from the segments, the tables were not consulted at all, so their positions counted
  with the game's own score. Either way a small part of the late buckets, judged in play.
- **Stop:** checked between buckets, the job then ending as a stopped scatter does. Nothing is kept
  of a stopped pass: local files, cheap to read again.
- **Bar:** a live bar, `measuring lambda by progress`, as the scatter's.

## Scaled runs use only their own data (the developer)

A scaled run's buckets hold its share alone, so every scaled run measures on its share and keeps its
polyline and frozen test set in its own generation, the rule only a partial source had. `partial_source`
goes: `config.scale` alone decides. Its test loss is no longer comparable with a full run's.

## Transformers postponed (the developer)

They have no buckets: `lambda: polyline` on one is refused, and the stream's `measure` removed.

## Checks

- Tests and clippy.
- The first real pass timed, as the design's speed is unmeasured.
