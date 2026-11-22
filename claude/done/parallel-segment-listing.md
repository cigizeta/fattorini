# The segments' `.meta` read in parallel

**Status:** done, 2026-11-22 (the developer: "this is a sync problem, fix"). Seen working the same
day on `lc0-halfk-threats-screlu-hidden-512-8-budget-a1`'s restart: "much faster" (the developer):
6,897 segments in about 43 seconds, about 160 a second, against 4.6 a second (about 25 minutes)
read one at a time. Left open: the listing still ignores Stop, which now waits seconds, not
minutes.

## Why

`pgn_train_stream::segment_files` read one `.meta` a segment from Drive, one after the other: about
220 ms each, Drive's round trip, so 6,897 segments of Lc0's larger corpus took about 25 minutes
before every attempt's scatter, even with nothing new to scatter.

## The change

- The `.meta` files read by `META_READERS` (16) threads under `std::thread::scope`, each taking the
  next index from an atomic counter and filling that index's slot: **the list keeps its order**, by
  day then by name, which the scatter's buckets depend on (a position's bucket is a hash of its
  arrival index, `buckets.rs`).
- The first error is returned, as before; the other threads stop taking new segments once one fails.
- The page's bar counts finished reads, as before.
- The ordering lives in a helper, `map_in_order`, tested without Drive: same results in the same order
  as a serial map, and an error returned.

## To see

The next attempt's "listing the segments" at much more than 4.6 segments a second. 16 is a guess at
what Drive's client serves at once; if the rate stays far below 16 times, Drive serializes, and more
threads gain nothing.
