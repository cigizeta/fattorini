# Self-play counted from the .meta files; the quiet filter removed

## Why (2026-11-01)

At every resume, smithy decompressed and replayed every self-play segment of the generation to count
the positions its filter keeps (`kept_in`), because a `.meta` records positions read, not kept. With
about 800 segments at the end of a play phase, that is a read of some 400M positions per resume. The
quiet filter, the only reason for the replay, hurt the networks it trained, and goes (the developer,
2026-11-01): its `-quiet` run is deleted.

## What a .meta already says

A self-play game carries a score after every move and no book (`tasks.rs` checks it), so its `.meta`
count, `plies + 1` a game, is its searched positions plus its final one. Searched positions, the ones
training and the test sets keep, are exactly `positions - games`: the same rule
`pgn_train_stream::segment_files` already applies to a source's corpus. No `.meta` is rewritten, and
the format does not change: readers that stream a segment (`pgn_stream`, `freeze`) still need the
positions read.

## Changes

1. `smithy.rs`
   - `kept_in` becomes `searched_in`: the `.meta` files' `positions - games`, and the open file's
     own counts, which `SegmentWriter::resume` already reads. Nothing is decompressed.
   - `record` counts a game's `positions() - 1`, not a replay of its text.
   - `Work::SelfPlay`'s `keep: Option<Keep>` becomes `searched: bool`: a forge counts searched
     positions, PeSTO's Play every position, as before.
   - Gone: `GenConfig::filter`, `Filter`, `keep()`, `filter_suffix()`, `source_buckets`'s quiet
     branches, `LC0_VALUE_BUCKETS`.
2. `game_labels.rs`: `Keep`, `is_quiet`, `second_move`, `kept_count`, `segment_kept_count` and their
   tests go; `self_play_game_kept` keeps every searched position. Visit counts are still read and
   checked against the legal moves.
3. `buckets.rs`, `pgn_stream.rs`, `pgn_train_stream.rs`, `freeze.rs`: the `keep` parameter goes.
   A bucket state's or a frozen manifest's `keep` line is still read, and a value other than
   `searched` refused, so a quiet set left on disk is never taken for a searched one; new ones write
   `searched`.
4. `configs/lc0-psq-threats-screlu-quiet.yaml` is removed by the developer (`git rm`).

## Tests

- `searched_in` counts from the `.meta` files and the open file, and agrees with a replay.
- The existing bucket, stream and freeze tests, without `Keep`.

## Left alone

`lc0/second_move.rs`, a by-hand diagnostic of Lc0's raw tars, only names the filter in its doc.
