# Segments by day, with one `.meta` each

Implementation plan, 2026-10-17, with the developer (TODO.md's two items, done together at one stop
of all machines).

## Layout

```
data/self/<teacher>/{train,test}/YYYYMMDD/{stem}-{YYYYMMDD-HHMMSS}.pgn.zst      (+ .meta)
data/lc0/{test,train}/YYYYMMDD/{tar-stem}-{NN}.pgn.zst                          (+ .meta)
```

The day is the segment's own date: a self-play segment's timestamp (its batch's finish), an Lc0
segment's tar date. A `.meta` replaces `.count` and `.games`:

```
positions	1002041
games	6991
```

## What stays

- `arena self`'s private state stays flat in `games_dir`: the staging `{stem}.pgn`, the sealed
  `{stem}-{timestamp}.pgn`, the batch's `{stem}.count`, the lock. Only the finished segment and its
  `.meta` go into the day directory, so resume, recovery and `other_active_stem` read the same files.
- `.meta` is the completion marker, as `.count` was: written before the rename that makes the
  segment visible, kept locally after upload.
- The legacy trees (`data/games/...`) keep `.count`: `prepare`, `buckets`, `replay` untouched.

## Changes

- `rotor::pipeline::segments`, new: `SegmentMeta { positions, games }` (text, parse), the day
  directory of a timestamp, and `list_remote(dir)`: every segment under the remote counterpart of
  `dir`, as `YYYYMMDD/name`, sorted.
- `remote::is_segment_file` also takes `.pgn.zst.meta` (and still the legacy companions).
- `arena` `self_play.rs`: the segment and its `.meta` in `{games_dir}/{day}/`; `recover_sealed` looks
  there and for `.meta`; `corpus_so_far` sums `.meta` over the day directories, local and remote;
  after a batch every day directory is published (`publish_all_segments`).
- `smithy`: `freeze`, `pgn_train_stream`, `stream_bench`'s new-tree branch read
  `segments::list_remote` and `.meta`; a frozen manifest names `YYYYMMDD/name`.
- `smithy lc0`: segments sealed in `data/lc0/{dir}/{day}/`, with `.meta`.

## By hand, all machines stopped (the developer)

For each segment on Drive and in each machine's local `data/self`: move it into its day directory
and write its `.meta` from its `.count` and `.games`, then delete those two. The two frozen
manifests (`data/self/hand-crafted/frozen/manifest.txt`, `data/self/caroline/gen-0001/frozen/`,
locally and on Drive, the label sets' copies too) get every `segment` line prefixed with its day.
A script for it is given with the code.

## Done (2026-10-17)

Built and tested on the training machine (`cargo test -p rotor pipeline`, `-p arena self_play`, `-p smithy`;
the one failure, `the_committed_configs_name_their_labels`, was caroline's `labels: self` and the
test now says so). All machines stopped; the one-off script migrated Drive's `data/self` and
`data/lc0` and each machine's local `data/self`, with nothing left flat, and the two frozen
manifests now name `YYYYMMDD/name`. Self-play restarted on the Lichess machine, another machine and the training machine, the converter on
the training machine.
