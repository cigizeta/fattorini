# Corpus windows

Its chunking (chunks across months, the tail, windows that only grow) is superseded by
monthly-chunks.md; the windows, the monthly TCEC split and the self-play share stand.

Design, settled with the developer on 2026-10-03. Branch: `alessio`. Not yet implemented.
Supersedes the sampling design of corpus-sources.md: no source is sampled any more.

## Decisions

- **Both archives by month, every game in a window.** TCEC and CCRL are each a list of monthly
  files, and `corpus.yaml` gives each a window `{from, to}`. Every game of every month in the window
  goes into the corpus. No count, no per-player sample, no `all`.
- **Shared by every player.** A source is chunked once, in month order, and every player replays
  the same chunks.
- **No memorization concern.** The per-player sample existed so that a champion would never label
  positions it trained on. At this scale it does not matter: a small network seeing about 290M
  positions a few times each learns patterns, not positions.
- **Self-play is a share, not a count.** `self` is the percentage of training positions taken from
  self-play.
- **Fresh start.** The developer moved all of `data/games/` to `data/archived/games/` on Drive.
  Nothing chunked or replayed is reused.

## Config

```yaml
file: 100000

tcec:
  from: 2010-06
  to: 2026-03

ccrl:
  from: 2005-12
  to: 2026-09

self: 20%
```

- `file`: positions at which a chunk or self-play segment closes, as today.
- `from`, `to`: months `YYYY-MM`, inclusive, `from <= to`. Every month in between must exist on
  Drive with its `.count`, or the command stops naming the missing ones.
- `self`: `N%`, 0 < N < 100. Anything else is refused.

Measured on 2026-10-03: TCEC 4,413,680 positions (32,770 games, first month 2010-06); CCRL
285,615,946 positions over 249 months, 2005-12 to 2026-08. September 2026 is still to download
(CCRL publishes a month from the 5th of the next).

## TCEC by month

TCEC is published as one cumulative file. A new preprocessor script splits it into monthly files
so that TCEC goes through the same logic as CCRL:

- It reads the single file and writes `data/games/original/tcec/YYYY-MM.pgn.zst` with its `.count`,
  one per month of the games' `[Date]` tag, keeping the games in their order in the file.
- Only months entirely over are written: up to the month before the month of the file's last game,
  not before the download date. The release lags the live site (a file downloaded at the end of
  September 2026 ends with Season 29's Superfinal, 2026-04-15), so the download date says nothing
  about the file's content; the last game's month may still get games from the next season, so it
  is incomplete by definition: the split detects it as the month of the last game, writes no
  `.pgn.zst`/`.count` pair for it, and reports its games as left for the next download.
- The month of a game is its `[Date]` tag, the date the release records for it. `[GameStartTime]`
  (UTC) can fall a day later near midnight; `[Date]` is used throughout for consistency.
- Every month from the first to the held-back one is written, those without a game too (TCEC pauses
  between seasons: 54 such months up to 2026-03, 2011-06 to 2012-12 the longest run), as an empty
  `.pgn.zst` with a `.count` of 0. So every month of a window is on Drive for both archives, and a
  missing one always means a month not downloaded or split yet. Found by the first `chunk`, which
  refused the window for these gaps.
- A month already on Drive is never rewritten. Games of a later download that fall in a month
  already written are reported and skipped, never added: chunking relies on a month never changing.
- A game with no usable date (`????.??.??`, or a year only) stops the split, naming it. None exist
  in the file held on 2026-10-03.
- Same discipline as `bin/tcec-prepare.sh`: counts checked against `[PlyCount]`, `.tmp` names,
  resumable, the `.count` written last.

Source and cadence: the compact release of TCEC-Chess/tcecgames on GitHub
(`TCEC-everything-compact.zip`, its `TCEC-everything-compet-traditional.pgn`), downloaded about
once a year. Releases come per season (sometimes with interims), the latest being "Season 29 Final"
(`S29-final`), which ends on 2026-04-15. Compact files exist only as these big files; the per-event
files (`TCEC-events-full.7z`, the repository's `master-archive/`) are in full format, without the
`{ Book exit }` markers, so they are not used. Each yearly download goes through the same script,
which adds the new months only.

## Chunking

- `rotor-train chunk` (no player any more): for each source, the window's months in order, every
  game, cut into chunks of `file` positions, under `data/games/chunked/<source>/`.
- The record (`chunks.txt`, replacing `sample.txt`) lists the months chunked and the chunk table.
  No seed, no threshold `p`, no selection files: a whole month is in or out.
- **Growing.** Moving `to` later, or `from` earlier, appends the new months' games as new chunks
  after the last one; the chunks written stay untouched. Moving `from` later or `to` earlier would
  drop games already chunked and replayed: refused.

## Replay

`rotor-arena replay <player>`: every chunk of every source, into
`data/games/replayed/<source>/<player>/`, as today.

## Self-play

With `A` the positions of both windows and `p` the `self` share, self-play needs
`S = A * p / (1 - p)` positions to be exactly `p` of the total: with `p = 20%`, a quarter of `A`.

- `rotor-arena self <player>` writes complete segments (`file` positions plus the game in progress)
  until its finished segments hold at least `S`. Other machines may add a few extra segments.
- `prepare` takes the segments in name order until `S`, cutting the last one at a game boundary,
  exact to within one game. Extra segments are not used.

## Size and time, at the measured rates

| Source | Positions |
|---|---|
| TCEC | 4.4M |
| CCRL, with September | about 288M |
| Self-play at 20% | about 73M |
| **Total** | **about 365M** |

- Replay at about 1,000 positions/s on the Lichess machine: about 80 h for the archives, once (shared), then only
  the new months.
- Self-play: about 20 h per generation.
- Training: scaling the old 13.5 h for 54.4M, about 90 h for five lambdas.

## Layout

```
data/games/original/<source>/YYYY-MM.pgn.zst(.count)   the archives, by month
data/games/chunked/<source>/                           shared chunks and chunks.txt
data/games/replayed/<source>/<player>/                 the chunks searched by the player
data/games/self/<player>/                              the player's self-play
data/archived/games/                                   the old tree, untouched
```

## Migration

1. The developer copies the archives back from `data/archived/games/original/` to
   `data/games/original/`: CCRL's months as they are; TCEC's single file only as the input of the
   split.
2. Download CCRL 2026-09 from the 5th of October.
3. Run the TCEC split.
4. `chunk`, then `replay hand-crafted`.

## Implementation plan

Six steps, each landing on its own with `cargo test` green, in this order. Step 1 needs no Rust and
can run on Drive while the rest is written.

### 1. `bin/tcec-months.sh`: the split

A new script beside `bin/tcec-prepare.sh`, same discipline (staging directory, `.tmp` names, the
`.count` last, Ctrl+C cleanup, resumable).

- Usage: `bin/tcec-months.sh <TCEC-everything-compet-traditional.pgn.zst>`, the release's compact
  traditional file, compressed with `zstd --long=27`.
- One pass of awk over the decompressed file appends each game, in file order, to a local staging
  `YYYY-MM.pgn` by its `[Date]`'s year and month. A game whose `[Date]` has no numeric year and month
  stops the script, naming its index.
- The month of the last game in the file is held back: no file written, its games reported as left
  for the next release.
- A month already on Drive (`.pgn.zst` and `.count`) is not rewritten. If the download holds a
  different number of games for it, the difference is reported (late additions, skipped).
- Every new month: `zstd -19 --long=27`, counted with `tcec-prepare.sh`'s awk counter (copied into
  the new script), its game count checked against the split's, its plies against `[PlyCount]` with
  the same 1% rule, then copied to `data/games/original/tcec/YYYY-MM.pgn.zst` and its `.count`.
- `bin/tcec-prepare.sh` is then retired (the developer deletes it). The single file moves to
  `data/archived/games/original/tcec/` once the split is checked.

Check on the file held now: 2010-06 to 2026-03 written, 77 games of 2026-04 held back, the written
months' `.count`s summing to 4,413,680 minus 2026-04's positions (a check by hand, after the run).

### 2. `corpus_config.rs`: windows and the share

- `RawConfig { file, tcec: RawWindow, ccrl: RawWindow, self: String }`, with
  `RawWindow { from: String, to: String }` and `deny_unknown_fields` on both.
- `CorpusConfig { file, tcec: Window, ccrl: Window, self_share: f64 }`; `Window { from: Month, to:
  Month }`, `Month` a `(year, month)` pair parsed from `YYYY-MM` and ordered.
- `Window::months()`: every month from `from` to `to`, as `YYYY-MM` labels.
- `load` refuses: a malformed month, `from > to`, `self` not `N%` with 0 < N < 100.
- `self_play_positions(archive_positions)`: `S = A * p / (1 - p)`, rounded up.
- `Size`, `chunked_dir`'s player, and `size()` go away; `chunked_dir(source)` is
  `data/games/chunked/<source>`.
- Tests rewritten: the committed config loads; each refusal; `months()` across a year boundary;
  `S` at 20% is a quarter of `A`.

The committed `configs/corpus.yaml` changes in the same step, to the windows of this design, so the
`the_committed_config_loads` test holds.

### 3. `chunk.rs`: by month, no sampling

`rotor-train chunk` takes no player any more.

- `Archive::files()` becomes the same for both: the monthly files on the remote with their
  `.count`, `ccrl/YYYY-MM.commented.pgn.zst` and `tcec/YYYY-MM.pgn.zst`. `key`, `TCEC_FILE_LABEL`
  and `TCEC_ARCHIVE_FILE` go away; `[Source]` becomes `<archive> YYYY-MM <index>` for both.
- Every month of the window must be on the remote with its `.count`, else the command stops naming
  the missing months.
- Removed: `player_folder`, `champion_exists`, `player_seed`, `draw`, `splitmix64`, `select`,
  `growth`, the selection files and their reading/writing, `budget`/`p`/`seed`.
- The record is `chunked.txt` (the progress record keeps the name `chunks.txt`): a header
  (`archive`, `from`, `to`, `months`, `file_positions`, totals) then the progress lines, as today.
- **Growing.** The months to chunk are the window's months not in the record. If a recorded month
  is outside the window, the command stops: a window only grows. New months are read whole, in
  order, and appended as chunks after the last one, through the existing resume logic
  (`ChunkWriter`, `consumed`). `chunked.txt` is replaced last, on the remote too, as `sample.txt` is
  today.
- **Every chunk is full** (decided after the first run sealed TCEC's last chunk at 96,962 positions):
  the games after the last full chunk stay unchunked, as a tail, and `chunked.txt` records
  `tail: <month> <entries of it already chunked>`. A growth reads from the tail's month (skipping
  those entries), then the new months, so the tail fills the next chunk. Under one chunk of
  positions waits outside the corpus until the next growth.
- Every month read is checked against its `.count` (`check_count`), as `select` does today; with no
  separate selection pass, the check happens while copying.
- The book rules stay: TCEC's `{ Book exit }`, CCRL's comment rule with its floor.
- Tests: the selection tests go; new ones for the months to chunk (from nothing, growth at either
  end, a shrunk window refused) and for the record header.

### 4. `replay.rs`: one shared directory

- `list_chunks` reads `data/games/chunked/<source>/chunked.txt` for both sources; `corpus.yaml` is
  no longer needed here (the record says what was chunked).
- Outputs unchanged: `data/games/replayed/<source>/<player>/`.

### 5. `prepare.rs` and self-play: the share

- `recorded_chunks` reads the shared records; `RecordedChunk.shared` and `delete_sampled_chunks`
  go: chunks are never deleted.
- `A` is the positions of every replayed chunk; `S = self_play_positions(A)`.
- `wait_for_self_play` waits for at least `S`, then takes segments in name order until their
  positions reach `S`. **Cut at a segment boundary, not mid-segment**: the dataset manifest records
  whole segments, and `add_label_sets` re-reads them whole, so a mid-segment cut would need the
  manifest to record it. One segment is about 100,000 positions, under 0.03 points of the share at
  this scale. A segment's own positions decide: the last one taken is the one that reaches `S`.
- `rotor-arena self` (`self_play.rs`, `self_play_command.rs`) stops at `S` computed from the
  windows' `.count` sums on the remote, read once at start. Those sums are at least the replayed
  positions (thrown games only remove), so self-play never stops short of what `prepare` needs.
- Tests: the segment choice (exactly at `S`, past it, extra segments unused).

### 6. Help text and `README.md`

`rotor-train`'s usage (`src/train/mod.rs`), the module doc comments touched above, and
`README.md`'s offline flow: `tcec-months.sh`, `chunk` without a player, `replay`, `self`,
`prepare`.

### Verification

```
cargo test --release
```

```
cargo test --release --features training --bin rotor-train
```

Then on Drive: `bin/tcec-months.sh` on the file held now (the check of step 1);
`bin/rotor-train chunk` (TCEC 2010-06 to 2026-03, CCRL 2005-12 to the last month downloaded, each
month matching its `.count`); `bin/rotor-arena replay hand-crafted` on one chunk of each source.

## Open

- None about the data held now. Measured on 2026-10-03: every TCEC game has a `[Date]` with year
  and month (no `????` line), so the rule for undated games is only a safeguard for later
  downloads: the split stops on one. The file runs 2010-06 to 2026-04, the last month with 77 games
  against 247-644 before it: the release ends with Season 29's Superfinal (last game 2026-04-15),
  although downloaded at the end of September. The split writes up to 2026-03, and `tcec.to` is
  2026-03 until a release with later games.

## Progress

- 2026-10-03: step 1 done. `bin/tcec-months.sh` takes no argument: it reads the one file in
  `data/games/tcec/` (`.pgn` or `.pgn.zst`), stopping on zero or several. The `[PlyCount]` 1% rule
  runs once over the whole file (per month it tripped on 1 game of 98 in 2013-02); each month only
  reports its mismatches. Run on the release held: months 2010-06 to 2026-03 on Drive, 2026-04 held
  back. The single file moved to `data/archived/games/original/tcec/`. Still to do by the
  developer: `git rm bin/tcec-prepare.sh`. Checked: the 136 months' `.count`s sum to 4,400,520,
  the file's 4,413,680 minus 13,160 for April's 77 games.
- 2026-10-03: steps 2 to 6 written together, since removing `Size` and the per-player
  `chunked_dir` breaks every caller at once; not yet compiled (no shell here). Choices made:
  - The archives' directories and suffixes moved to `corpus_config` (`archive_dir`,
    `archive_suffix`), so `rotor-arena self` reads the windows' `.count`s
    (`CorpusConfig::window_positions`) without depending on `chunk.rs`.
  - `self` is a whole percentage, `self_percent`; `S` is computed in integers, rounded up.
  - `chunk.rs` writes `months.txt` before its first chunk and refuses to resume a stopped run over
    different months (its progress counts the games of exactly those).
  - Games with no trainable position are left out of chunks, as the old selection did.
  - `prepare` now always compares a chunk's `[Source]`s with its output's (chunks are never deleted).
  - `self` stops at `>= S`, matching `prepare`'s cut.
  Next: the developer runs the tests; fix whatever the compiler finds.
- 2026-10-03: tests passed and committed, with `bin/tcec-prepare.sh` removed. Next, on Drive:
  download CCRL 2026-09 from the 5th of October (`bin/ccrl-download.sh`), then
  `bin/rotor-train chunk`, then `bin/rotor-arena replay hand-crafted` on one chunk of each source as
  the last check of this design.
- 2026-10-03: chunked with `ccrl.to: 2026-08`. TCEC: 44 chunks, 4,400,520 positions, 0 thrown.
  CCRL: 2,854 chunks, 285,600,371 positions, 0 thrown, 128 games and 15,575 positions in the tail
  (2026-08); chunks plus tail equal the 249 months' `.count`s. Both on Drive.
- 2026-10-03, decided with the developer: **no full replay now.** The replay is only a test of this
  design; its outputs are deleted afterwards. The next full replay comes only after the planned
  work on the search algorithm is complete, so the corpus is labelled by the search that will stay.

- 2026-10-03: migration step 1 done. The developer moved `original/` back from
  `data/archived/games/` to `data/games/original/` on Drive (`ccrl/` and `tcec/`, the latter still
  the single file and its `.count`). `data/archived/games/` keeps `chunked/`, `replayed/` and
  `self/`.
