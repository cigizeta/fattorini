# Lichess's strongest games: training data and the book marker

**Superseded, see lumbra-book.md** (2026-10-13): human games are not training data, and the book
comes from Lumbra's Elite file; filtering Lichess's monthly 30 GB to under 1% was not worth it.

**Status:** design, 2026-10-13. Nothing implemented. Supersedes lumbra-book.md. **Open: q** (the
share of games kept, 0.5% proposed) and **f** (the book's threshold), both settled on the first
filtered month.

## Why

Two needs, one source (the developer, 2026-10-13):

- **A book curated outside CCRL**, to mark the testers' book in CCRL lists without comments
  (Blitz; ccrl-sources.md). `ccrl.bin` cannot be it, being extracted from CCRL 40/15.
- **Human games as training data**, any position being fine, their results replaced where they
  are not sound.

Rejected sources: Lumbra's GigaBase (its site changed and its Elite file is no longer offered; the
book must not rely on a file only the developer has; CC BY-NC); the Lichess Elite Database
(database.nikonoel.fr, one curator, stops at 2025-11); Lichess's broadcasts (each organiser fills
the headers, some ratings may be Lichess's; 2,000 to 5,000 strong games a month).

**The source:** Lichess's standard rated games, https://database.lichess.org/#standard_games, run by
Lichess itself, monthly since 2013, CC0, about 90M games and 30 GB a month
(`standard/lichess_db_standard_rated_YYYY-MM.pgn.zst`, the name to be checked).

## A month, from download to Drive

1. **Download** into local staging, resumable (`curl -C -`): streaming through a filter could not
   resume after a dropped connection.
2. **Filter**, in two passes over the local file:
   - Dropped first, for every purpose: games from a `[FEN]` (thematic tournaments), bullet and
     ultrabullet (`[Event]`'s speed), games with a provisional rating (how the database marks one,
     a `?` or otherwise, to be checked on the first month), games ended by "Rules infraction".
     Games without increment: dropping them is still open.
   - A game's rating is **min(WhiteElo, BlackElo)**.
   - **Top q% per speed and month:** the threshold is the rating keeping the top q% of the
     remaining games, separately for blitz, rapid and classical (Lichess's pools are not
     comparable, and drift). Ties at the threshold are kept. The first pass builds the
     distributions, the second filters.
3. **Write** `<YYYY-MM>.pgn.zst` with its game count, and a record: the Lichess month, the rules, q,
   the three thresholds, the games kept per speed and the share actually kept. The month is
   reproducible from the record although its original is gone.
4. **Move** the filtered month to Drive (`data/games/original/lichess/`), **delete** the original.

q: about 300k games a month at 0.5% (estimated from 60M non-bullet games), about 20M positions
after the book cut with playouts; the labelling budget of a week (about 1B positions in all)
leaves room for a window of a year or more. Confirmed on the first month: if the blitz threshold
falls below about Lichess 2500, q is lowered.

## Use 1: the book

- Built from **every filtered month of the window**; G is their games.
- Each game contributes the positions after each of its first **24 plies** (CCRL's book is up to
  12 moves a side), keyed by Polyglot key so transpositions pool.
- A position is book if at least **N = max(2, ceil(f * G))** games reach it: linear in G, so the
  same positions stay book as the window grows and f holds across rebuilds (a logarithmic N would
  admit rarer positions at every rebuild). An absolute count, not a share of the parent's games,
  which would mark every move of a rare line.
- A game's book is its longest prefix of book positions, at most 24 plies; for a CCRL game at
  least the 8-ply floor.
- **f** is calibrated on CCRL 40/15, where the comment rule gives the book: per game, how often the
  book stops **short** of it and runs **past** it, for f giving N from about 5 to 200. f is chosen
  where **past** stays rare. Refinements only if asked for: a stricter N past ply 16.
- **Frozen per generation:** built when a generation's teacher is named and kept for it, since a
  new month changes G, the counts, and every source's cut.
- The record: the months, G, f, N, **short** and **past**.

## Use 2: training data

- A corpus source, key `lichess`, a window of months in `corpus.yaml`, chunked, replayed and
  labelled like the others (ccrl-sources.md's changes apply).
- **The book cuts the Lichess games themselves:** their longest prefix of book positions is skipped,
  as CCRL's testers' books are. Not circular: the book says which positions are common theory.
- **Results:** a final position that decides the game by the rules (checkmate, stalemate,
  repetition, fifty moves, insufficient material) keeps its result; one the tablebases know takes
  theirs. Every other game, a resignation, a time forfeit, an agreed draw or an abandoned game, is
  **played out by Rotor** from its final position, with the game's moves as ancestors, and that
  result replaces the human one. The playout's positions are self-play positions, labelled too.
- To check: `nnue_bench --residuals` holds out human over-the-board games from every training
  corpus; Lichess's games are online, but whether its hold-out still holds is to be confirmed.

## The code

`arena book` (`crates/arena/src/opening_book.rs`) builds `ccrl.bin` for lichess-bot; most of it
carries over.

- **Reused:** `polyglot_key` (with its en passant rule), `polyglot_move`, `POLYGLOT_RANDOM`,
  `Entry`, `book_entries`, `scaled_weights`, `write_book`, `write_atomically`, the record, the upload,
  their tests. The Polyglot core moves to a crate `smithy` also uses, since `smithy chunk` marks
  books.
- **New:** the first 24 plies of every game in place of `tester_plies`, the count per position, the
  cut at N. `lichess.bin` has its own paths: `smithy install` keeps copying `ccrl.bin` for
  lichess-bot.
- **Memory:** every (position, move) pair is held before the cut; if a month shows it too large, a
  first pass counts positions and a second keeps the moves of positions at or above N.

## Steps

1. The first month by hand: its file name, the provisional marker, the speeds' shares, the rating
   distributions; q chosen with the developer.
2. The download-and-filter script in `bin/`, one month at a time.
3. A few recent months filtered onto Drive.
4. The Polyglot core moved; the book command; `lichess.bin`.
5. The calibration on CCRL 40/15; f chosen with the developer.
6. The `lichess` corpus source and the book cut (with ccrl-sources.md); the playouts.
