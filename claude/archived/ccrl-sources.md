# CCRL 40/15, Blitz and Chess324, replayed by month

**Deprecated (2026-10-16), see rotor-alone-and-lc0.md:** CCRL and TCEC are no longer training
data, and generation 4 trains on self-play alone. Moves to `archived/` once every network's
generation 3 is promoted.

**Status:** design, 2026-10-14, for generation 4. Nothing implemented. Replaces this document's
version of 2026-10-13 (chunks, `lumbra.bin`) and replay-book-marker.md.

## Decided (the developer, 2026-10-13 and 2026-10-14)

1. **Three CCRL lists**, all under standard castling, by month:

   | List | Path | Key | Months | Time control |
   |---|---|---|---|---|
   | 40/15 | `/ccrl/4040` | `ccrl-40-15` | 2005-12 on | 40/15 |
   | Blitz | `/ccrl/404` | `ccrl-blitz` | 2020-01 on | 2'+1" |
   | Chess324 | `/ccrl/Chess324` | `ccrl-324` | 2026-07 on | 2'+1" |

   Left out: the 40/2 Archive (frozen in 2020), Chess960 and DFRC (960 castling, TODO.md). TCEC
   stays as it is, key `tcec`, by month, with its own `{ Book exit }` marker.
   **The key names every directory** (the developer, 2026-10-14): `data/games/original/<key>/` and
   `data/games/replayed/<key>/`, and `corpus.yaml`'s windows. The old `ccrl` key and its
   directories are generation 3's, left as they are, never mixed with generation 4's.
2. **The smallest files:** the bare monthly PGNs of every list. The commented 40/15 files on Drive
   are deleted, to save Drive's quota.
3. **No chunks.** `smithy chunk`, `data/games/chunked/` and `chunked.txt` go. **The unit of work is
   one month of an archive**, read in place on the remote: the replay claims a month, replays it,
   and seals it.
4. **The replay detects the book itself** and searches from the first move out of book:
   - **40/15 and Blitz:** the master graph (book-building.md). A game's book is its longest prefix
     of plies whose positions are nodes of the graph, by Polyglot key, at least 8 plies (CCRL's books
     are never shorter) and at most 24 (CCRL's books are up to 12 moves a side).
   - **Chess324:** no book. Every game starts from its `[FEN]`, which no book built from the
     standard start covers, so every ply is searched. Settled (the developer, 2026-10-14): its
     positions, opening plies included, are among the corpus's most valuable, so none is skipped.
   - **TCEC:** its `{ Book exit }` marker, as before; no marker, no book.
5. **Order of the sources** in the replay and in `prepare`'s manifest: `tcec`, `ccrl-40-15`,
   `ccrl-blitz`, `ccrl-324`, then self-play. The test split takes its held-out positions from the first segments.
6. **Every game is used** (the developer, 2026-10-14): the corpus is every month `corpus.yaml`'s
   windows hold, never narrowed to save labelling time. Without 40/15 that is 280M positions
   (Blitz 268.7M, Chess324 6.7M, TCEC 4.4M).
7. **From generation 4.** Generation 3's replays and datasets stay as they are on Drive; nothing
   reads chunks after this change except what is already built.

## The graph

- `corpus.yaml` names it, so every machine and every replay use the same one:

  ```yaml
  book: data/books/LumbrasGigaBase_OTB_ELO2400/graph.txt
  ```

- Read once per replay from the remote: its node keys as a set (191,917 for Elite at \\(N = 9\\),
  a few MB).
- Every replayed month records the graph's header (source, \\(\alpha\\), \\(N\\), built), so a month
  always says which graph cut its books. A rebuilt graph is a new corpus: `corpus.yaml` names it,
  and months already replayed are not redone.

## Downloads: `bin/ccrl-download.sh <40-15|blitz|324>`

- The argument is the key's suffix: it sets the page (`/ccrl/4040`, `/ccrl/404`, `/Chess324`), the
  remote directory (`ccrl-<argument>`),
  the staging directory, and the first month (the earliest monthly link on the page).
- The bare monthly links (`games-by-month/YYYY-MM.bare.[N].pgn.7z`, as seen for Blitz; to check for
  40/15 and Chess324), kept as `YYYY-MM.pgn.zst`.
- **The `.count` holds positions, as every source's always did** (the developer, 2026-10-14,
  replacing a draft that put games in it). The book needs the graph, which awk cannot read, so the
  script checks the games against CCRL's number in the file name and the plies against
  `[PlyCount]` in awk, then counts the positions with `bin/arena count <key> <archive>`: the
  replay's own reading of the month (`prepare_month`, the graph named by `corpus.yaml`), printing
  its positions. The count depends on the graph: a rebuilt graph is a new corpus, recounted by
  deleting the `.count`s and rerunning the download, which then reads the archives already on
  Drive and downloads nothing.
- **The replay checks** every month's positions against its archive's `.count` before searching,
  and stops on a difference: a count made with another graph or rule.
- Everything else (resume, `.tmp` names, the throttle, a month complete from the 5th) unchanged.
- **TCEC is redesigned later** (the developer, 2026-10-14): `bin/tcec-months.sh`, its `.count`s
  (still positions) and its replays are left as they are for now. Generation 3's hand-crafted
  replays (`replayed/{tcec,ccrl}/hand-crafted/`) are kept: a hand-crafted label does not depend on
  the generation, so they may be reused.

## `arena replay` by month

- **Work:** every month of every window, in decision 5's order. A month is finished when its
  replayed file's `.count` is on the remote, as a chunk was.
- **Claims:** `YYYY-MM.claim.<machine>` beside the month's output, never expiring. **The remaining
  months are sorted by size, largest first, and split into equal contiguous parts, one per active
  machine of `machines.txt` (fastest first); each machine claims the largest of its own part** (the
  developer, 2026-10-14). With 12 months, the training machine may claim 1 to 4, the Lichess machine 5 to 8, another machine 9 to 12, and
  they claim 1, 5 and 9. The size is the archive file's on the remote, standing in for the
  positions, which are known only once a month is read. At the end another machine stops first, then the Lichess machine,
  and the training machine completes (at least three months left per active machine).
- **Reading:** the month's archive file, streamed from the remote (`--long=27` window), not loaded
  whole. Each game is checked as `smithy chunk` checked it: a move that is not legal, or a missing
  result, throws the game away, counted in the record; in `ccrl-324`, a game whose `[FEN]` is the
  standard start counts no positions. **A `[FEN]` in 40/15 or Blitz** (the developer, 2026-10-14,
  after Blitz 2022-12 held 321): the tester's own opening, so no book, every ply searched; a
  `[FEN]` of the standard position still gets the graph's book. A TCEC game may start from one too.
- **Identity:** a game is its index in the month's file, which never changes once on the remote.
  The output keeps the `[Source]` header (`<key> YYYY-MM <index>`) the chunks used to add, so resume
  and `prepare`'s checks keep working.
- **Output:** `data/games/replayed/<key>/<player>/YYYY-MM.pgn.zst` and its `.count` (positions),
  each game with `{ Book exit }` after its book and a score after every later move, as today, so
  `prepare` and `length_cache` read it unchanged. A game with no move out of book is left out.
- **Resume:** within a month, as within a chunk today: the staged output, the progress file, and
  the last `[Source]`.
- **Progress:** positions are known only once a month is read, so the bar counts games (the
  `.count`), and the corpus line counts months done and positions replayed so far.
- **The month's record**, beside the output: the graph's header, games read, thrown away, out of
  book, positions.

## `smithy prepare`

- Segments are months: `games/replayed/<key>/<player>/YYYY-MM`. Their positions come from the
  replayed `.count`s, as today.
- `check_replay`, per month, without reading the archive: the output's `.count` and the record's
  `positions` equal the archive's `.count`; the output's games are the record's `games`; their
  `[Source]`s are `<key> YYYY-MM <index>` with indexes rising strictly, from 1 to the record's
  `games_read`. The gaps are the games thrown away or without a move out of book.
- `corpus_config::chunked_dir` and `CHUNKED_ROOT` removed.

## `arena self`

`window_positions`, self-play's target, sums the archives' `.count` positions, unchanged: they are
positions again, exactly the replay's.

## Removals

- `crates/smithy/src/chunk.rs`, the `chunk` subcommand, `corpus_config::chunked_dir` and
  `CHUNKED_ROOT`, `corpus.yaml`'s `file` for chunks (it stays for self-play segments).
- CCRL's comment rule everywhere: `bin/ccrl-games.awk`'s CCRL branch, the download's
  `count_positions`, `is_ccrl_book_comment` and its helpers.
- `DEV.md`'s chunking step.

## Open

- **The bare links of 40/15 and Chess324** and Chess324's first month: checked on the site before
  the script changes.
- **Claims on large months:** whether a whole month per claim balances the machines well enough
  near the end of a generation, when one machine may hold the last large month alone.
- **lumbra-book.md** is superseded by book-building.md and this document: archived once this is
  agreed.

## Steps

1. **Done 2026-10-14:** every list's monthly links are `games-by-month/YYYY-MM.bare.[N].pgn.7z`
   (N the month's games). 40/15 from 2005-12, Blitz from 2020-01, Chess324 a single month,
   2026-07, holding all 57,152 games. Its `[FEN]`s are counted by the download (games from a FEN).
2. `ccrl-download.sh <list>`, games in `.count`; `tcec-months.sh` likewise. Download Blitz and
   Chess324, and 40/15's bare months; delete the commented ones.
3. `corpus_config`: the four sources (`SOURCES`), their `BookRule`, `book`, one `ARCHIVE_SUFFIX`;
   its tests. `smithy chunk` removed with it (generation 3's replay being finished); `arena self`
   only made to compile, its target still open. **Done 2026-10-14, tests passed.**
4. `arena replay` by month, with the book detection; its tests. **Done 2026-10-14, tests passed.** Settled while writing: a game's `[Source]` index counts from 1 over every game
   of the month's file, thrown ones included; a month's record is `YYYY-MM.txt` beside its output,
   uploaded before the `.count`; TCEC is replayed by its marker meanwhile, until its redesign.
5. Positions in the `.count`: `arena count`, the download calling it, the replay's check. Then
   `smithy prepare`. **Done 2026-10-14, tests passed; Chess324 and Blitz recounted.**
6. TCEC (2026-10-14): already monthly on Drive since generation 3; `tcec-months.sh` now counts
   with `arena count` like the CCRL lists, recounting from Drive a month whose `.count` is
   deleted; a `[FEN]` is refused only by the graph rule, so a TCEC game may start from one.
   Removals, `DEV.md`. **Done 2026-10-14:** `DEV.md`'s downloads take the list and the chunking
   step is gone; `ccrl-total.sh <key>` sums any source's `.count`s; `ccrl-games.awk`,
   `ccrl-ratings.sh`, `ccrl-results.sh` and `ccrl-first-no-book.sh`, which read the commented files
   with CCRL's comment rule, are deleted.
7. best-move-label.md, decided for generation 4 (the developer, 2026-10-14): its replay and
   self-play PGNs carry the best move.
8. `cargo test --release`, then generation 4.
