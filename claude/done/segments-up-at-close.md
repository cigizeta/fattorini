# Self-play segments go up at close

## The fault (2026-11-01)

A forge run's self-play segments reached the remote only through `Run::copy_up`, which mirrors the
whole open generation at the run's release and every 30 minutes of training, never during play. So:

- Every segment closed during a play phase stayed local until the run was released: on 2026-11-01
  about 150 segments of `lc0-psq-threats-screlu` run-1 gen-2 were local only.
- `copy_up` also copied the `.pgn.open` file of the moment. Closed locally later, it stayed on the
  remote beside its own segment, since `tree::copy` never deletes. A resume elsewhere copies it down,
  and `SegmentWriter::open` closes it into `<stamp>.pgn.zst` over the full segment of the same name,
  losing the games written after the snapshot.

## The model

A segment is written locally, compressed when closed, and sent to the remote right then. An open
file never leaves the machine.

## Changes

1. `SegmentWriter::close` copies its `.pgn.zst`, then its `.meta`, to the directory's remote
   counterpart, through `remote::copy_files`, when the directory is under `data/` (a test's scratch
   directory has no remote). Inline: `G:` writes land in Drive for desktop's local cache, well under a
   second for a segment that takes over a minute to play. A failed copy is a warning, and play goes
   on: the segment is local still, and the next `copy_up` takes it.
2. `tree::in_open_generation` refuses a `.pgn.open`, so neither `copy_up` nor a resume's copy down
   ever moves one.
3. `Run::copy_up` removes any `.pgn.open` from the remote generation's `test/` and `train/`: the
   leftovers of earlier builds go at the run's next release.
4. Tests: a close under a stand-in remote puts both files there; `in_open_generation` refuses an open
   file; the remote clean-up removes open files and nothing else.

## Done by hand

`lc0-psq-threats-screlu` run-1 gen-2: smithy stopped cleanly, the remote's five `.pgn.open` deleted.
`lc0-psq-threats-screlu-quiet` run-1 gen-2 still holds stale ones on the remote; change 3 removes
them at that run's next release.
