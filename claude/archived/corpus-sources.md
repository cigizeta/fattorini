# Corpus sources

Design, settled with the developer, one question at a time. Branch: `alessio`. A first part is
implemented (see "As implemented"); the rest below is designed, not yet implemented.

## The three sources

- **TCEC: unbalanced.** Its openings are chosen to be decisive, so it is the source of lopsided
  positions. Scarce: 32,770 games, 4,413,680 positions, growing about 300-350k positions a year.
- **CCRL: balanced.** The large archive, 285.6M positions.
- **Self-play: weak, but Rotor's own patterns**, the positions its search actually reaches.

## Config

`configs/corpus.yaml` (renamed by the developer from `self_play.yaml`), every value in trainable
positions:

```yaml
file: 100000
tcec: all
ccrl: 40000000
self: 10000000
```

- `file`: positions per chunk and per self-play segment.
- `tcec`, `ccrl`: `all`, every game of the archive, or a count, a sample. `all` is there for
  whoever can afford it (a fork with more machines); here TCEC is `all` and CCRL a count.
- `self`: a count only, the self-play positions of one generation. `all` is refused: self-play
  has no archive to be whole.

The corpus today is TCEC + `ccrl` + `self` = 54.4M, against 20M before: training about 2.7x longer
(about 13.5 h for five lambdas), affordable since the search speed-ups.

## A count samples anew for each player; `all` is shared

Dissected with the developer. A shared sample would be simpler, but champion i was trained on those
very positions, so its scores there are partly memorized, and every generation would distil the
previous one on the same points. A new sample per player gives labels on positions the champion
never saw, and coverage grows by `ccrl` positions a generation. So a count is drawn per player, by
the hash threshold seeded by the player's name, as CCRL is today. `all` is every game, so it is
the same for every player and shared; its memorization effect is diluted (TCEC is about 8% of the
corpus, and self-play is always new).

## Layout

```
data/games/original/<source>/              the archives, as downloaded (was all/)
data/games/chunked/<source>/all/           a shared `all`, cut into chunks once
data/games/chunked/<source>/<player>/      a player's sample, cut into chunks
data/games/replayed/<source>/<player>/     the chunks searched by the player
data/games/self/<player>/                  the player's self-play (was data/self/)
```

Every game, archived, chunked, searched or played, lives under `data/games/`.

`chunked` replaces `sampled`, since `all` samples nothing; `original` replaces `all`, which now
names a size. Chunks stay copies of the games (with
`{ Book exit }` and `[Source]`): virtual chunks, slices of the archives, were considered and
deferred (`TODO.md`). A player's sampled chunks are deleted once its dataset is built, as today; a
shared `all` is never deleted.

`replay` and `prepare` read `corpus.yaml` to know, for each source, whether its chunks are in
`all/` or in `<player>/`.

## Per generation

Generation i+1's corpus is labelled entirely by generation i's champion, the player
(`hand-crafted` for generation 1):

1. `rotor-train chunk <player>` (was `sample`): each source's chunks, shared or the player's own.
2. `rotor-arena replay <player>`: every chunk, at depth 9.
3. `rotor-arena self <player>`: until its directory holds more than `self` positions. Openings
   from the UHO book (2,632,036 positions, a random draw per game), 6 exploration plies at
   MultiPV 1-3 within 50 cp, depth 9, no adjudication: all as implemented.
4. `rotor-train prepare <network>`: the dataset from every replayed chunk and self-play segment.

Only the previous generation's champion: neither labels nor self-play carry over from older
generations.

## One way of chunking for both sources

The CCRL machinery becomes the only one. An archive is a list of files: CCRL's months, or TCEC's one
file. A game is keyed by its file and its index in it (TCEC's file is keyed `0`, outside any
`YYYYMM`). A count is a threshold `p = count / N`; `all` is `p = 1`. TCEC's book rule and CCRL's
stay as they are, each with its archive.

**Growing appends chunks**, for either source and either size: the chunks written stay untouched,
and the games no earlier selection holds go into new chunks after the last one, from a selection
file of their own (`selected-2.txt.zst`, then `-3`). A count grows when it is raised; `all` grows
when the archive does (a new CCRL month, new TCEC games appended). An archive must only grow by
appending: a game selected before and missing now stops the command. The record, `sample.txt`, is
replaced last, on the remote too, after the new chunks are uploaded.

## Migration

Once the current replay is finished or stopped, never while it runs, since it reads `sampled/`:

- `data/games/all/` moves to `data/games/original/` (the archives: on Drive a rename);
- `data/games/sampled/tcec/` moves to `data/games/chunked/tcec/all/`;
- `data/games/sampled/ccrl/hand-crafted/` moves to `data/games/chunked/ccrl/hand-crafted/`;
- `data/self/` moves to `data/games/self/` (only stale pre-PeSTO games today, on the delete list);

locally and on Drive. Nothing replayed moves. `chunk hand-crafted` then grows CCRL from 15.5M to
40M, about 245 new chunks.

## As implemented (first part)

- `self_play_config.rs`: `{ file, ccrl, self }`, the last as the field `self_play`.
- `sample.rs`: growing a CCRL sample by appending chunks (`growth`, `selection_file`, `consumed`,
  the progress record seeded from the old `sample.txt`, the remote `sample.txt` replaced after the
  upload). The CCRL budget is `ccrl`.
- `prepare.rs`: datasets named from the remote's `data/` (`games/replayed/...`, `self/...`), with
  the self-play segments after the replayed chunks; it waits for more than `self` self-play
  positions. A dataset built before is refused with a message to delete it.
- `self_play.rs`: `config.file`, `config.self_play`.
- `README.md`'s offline flow.

## As implemented (second part)

1. `corpus_config.rs` (`CorpusConfig`, `Size`, `chunked_dir`) replaces `self_play_config.rs`:
   `tcec`/`ccrl` read as `all` or a count through an untagged enum, `self: all` refused with its
   reason.
2. `chunk.rs` (was `sample.rs`), the command `chunk`: `Archive` gains `name`, `files`, `path`, `key`
   and `source`, and `chunk_archive` runs both archives, `all` as `p = 1` with the seed `all`. TCEC's
   first chunking, whose record has no `budget`, `p` or `selected`, reads as `all` at its
   `archive_positions`, every one of its `archive_games` selected (a `selected.txt.zst` is written
   for it the first time it grows). A selection label is a month or `tcec`. The record's header
   names its `archive` and lists `files`.
3. `replay.rs` and `prepare.rs`: each chunk carries its `chunked_dir`, from the config; `prepare`
   deletes only a player's own sampled chunks, never a shared `all`.
4. Self-play under `data/games/self/<player>/` (`self_play_dir`, `self`'s root and lock); every
   dataset segment is then under `games/`, which is what tells a dataset built before.
5. `bin/` scripts: `games/all` to `games/original`. `README.md`'s offline flow; `promote`'s next-step
   message names `chunk` and `replay`.

## Measured

- The replay at depth 9 with the hand-crafted evaluation, on the Lichess machine: about 1,000 positions/s, a
  chunk of 100,000 positions in about 1 min 40 s (2026-10-01, after the search speed-ups; the plan's
  estimate was 580). 100,000 positions a chunk is the right size: claims and uploads cost nothing
  next to it, and a stop loses at most the games in flight.

## Out of scope

- Virtual chunks (slices of the archives instead of copies).
- Comparing with and without self-play (the totals differ, so a duel would measure more data and
  new data together).
- A CCRL floor: TCEC would take about 15 years to crowd CCRL even at a 50% floor.

## Verification

```
cargo test --release
```

```
cargo test --release --features training --bin rotor-train
```

Then, after the migration, `bin/rotor-train chunk hand-crafted`: TCEC found complete in
`chunked/tcec/all/`, CCRL chunks 0000-0155 unchanged with about 245 new ones after them, and
`sample.txt` recording both thresholds.
