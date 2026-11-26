# PeSTO's play counts searched positions

Agreed with the developer, 2026-11-08.

## Problem

PeSTO's play counted every position read, each game's final one included; the freezer and training
count the searched positions only, all but each game's final one. The corpus stopped at 4,194,449
test positions, 4,164,516 searched: fewer than the 4,194,304 of its test set, so the freeze refused
it. Its training set is short the same way, by one position a game (1,066,100,914 records against
1,073,741,824).

## Plan

1. **One count, searched positions:** only a position with a search evaluation counts, in every
   self-play. `Work::SelfPlay` loses `searched`: `record` always counts `positions - 1` a game, and
   its doc drops "PeSTO's play counts every position read".
2. **`pesto::remote_positions`** sums `positions - games` over the `.meta` files, as `searched_in`
   does locally. PeSTO's play then resumes from the searched counts, and `write_complete` records
   them.
3. **Tests:** the self-play test that sets `searched: false` expects the searched count; a
   `remote_positions`-style test checks `positions - games` (the summing split into a pure function
   over metas, if the remote cannot be faked).

## Recovering the corpus

The developer deletes `data/pesto/complete.txt` on the remote by hand, as for Lc0's, and presses
Play: it resumes from the searched counts, plays about 30k test and 7.6M training positions, and
writes `complete.txt` again. The buckets need no full scatter: `buckets::prepare` appends only the
new training files to those already scattered.
