# The polyline measured on the whole corpus

**Superseded in its order and cost, see polyline-from-buckets.md:** the sample stays gone, but the
measurement reads the finished buckets, not the segments.

The developer, 2026-11-04: the polyline is measured on every training file, not on a sample.

## Why

`polyline::sample` kept every k-th file, about 10M positions, on the grounds that a whole corpus
reads at a few thousand positions a second and would take days. That rate does not fit the record:
Lc0's scatter read about 1.2B positions in about 1h30m through Drive (TODO.md), and the measurement
only adds six running sums per position. The cost is one more read of the corpus, once per corpus,
since `polyline.txt` is kept and reused. `measure` itself has not been timed on a whole corpus yet.

## The change

1. `polyline.rs`: `SAMPLE_POSITIONS`, `sample` and its test removed; `ensure` measures on every file
   it is given. The module's own text says so.
2. `smithy.rs`: the comment at the measurement no longer names `polyline::sample`.
3. TODO.md: step 4 of the status updated.

The order stays as it is: measured before the share is cut, the frozen test set and the scatter
after it. The scatter is not involved, since buckets hold both end labels and each record's progress,
so a stopped scatter resumes untouched.

## What follows from it

- **A polyline already measured stays until deleted:** `ensure` reuses `polyline.txt`. A corpus
  measured on a sample needs that file deleted, and the frozen set's `lambda-polyline` too if
  written (`lambda-polyline-direct` until 2026-11-04, when the suffix was dropped), or `freeze::check_polyline` refuses the new polyline.
- **A scaled run on a complete source** with no `polyline.txt` yet pays for the whole pass, once, and
  writes the shared file: not the minutes such a run otherwise starts in. Accepted, since the file
  must be the corpus's own: measuring on the share would disagree with the shared frozen set. A
  scaled run on a partial source reads what exists, in its own generation, as before.
- **The pass cannot be stopped:** `measure` takes no stop flag, so a Stop waits for it to end, and a
  smithy killed during it starts over. This mattered little on a sample. To decide after the first
  pass is timed, with the progress bar already asked for in TODO.md.
