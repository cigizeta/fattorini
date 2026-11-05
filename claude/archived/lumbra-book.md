# lumbra.bin: one book from Lumbra's master games, for the replay and for Lichess

**Superseded, see book-building.md and ccrl-sources.md** (2026-10-14): the book for Lichess is
`arena book`'s, with N fitted per source; the replay cuts CCRL's books with the master graph.

**Status:** design, 2026-10-13. Nothing implemented. Supersedes lichess-games.md, back from
`archived/`, and will supersede opening-book.md (`ccrl.bin`). **Open: f**, the fraction that sets N,
decided with the developer from the calibration (step 5); the window's first year and the
rapid/blitz filter, decided by the same calibration. Serves ccrl-sources.md and the Lumbra entry of
TODO.md.

**Decided (the developer, 2026-10-13):**

- **One book, `lumbra.bin`, two uses:**
  - **The replay:** it marks the book of every CCRL game. CCRL 40/15 and Blitz are downloaded bare,
    without comments, and Rotor detects the book itself; CCRL's comment rule is retired from
    `smithy chunk`. TCEC is untouched: its openings are chosen unbalanced, nothing to do with this
    book, and its own `{ Book exit }` marker stays its rule.
  - **Lichess:** it **replaces `ccrl.bin`** for lichess-bot. `smithy install` installs `lumbra.bin`
    as `rotor.bin`; `arena book` and `ccrl.bin` are retired (opening-book.md), its Polyglot code
    reused here.
- **Generated once per generation and saved to Drive** (`data/books/lumbra.bin` and `lumbra.txt`),
  where `smithy chunk` and `smithy install` both read it.
- **Lumbra's own Elite file is the source**, as long as Lumbra lives and no better source appears.
  Its licence, CC BY-NC 4.0, is accepted: the book holds position counts and move weights, not
  games.
- **Human games are not training data** (the developer, 2026-10-13, after briefly planning a
  `lumbra` corpus source with Rotor's playouts in place of human results: dropped the same day).
- Rejected: Lichess's rated games (30 GB a month, filtered to under 1%); TWIC (one weekly PGN since
  1994, rapid and blitz mixed in, fewer games); OTB Complete filtered here (needless now that the
  Elite file is offered again).

## Why

CCRL Blitz has bare PGNs only, and 40/15 is now downloaded bare too, so no comment tells the
testers' book from the engines' moves. A book curated outside CCRL marks it instead (the developer,
2026-10-13): a game's book is its longest prefix of positions the book holds. `ccrl.bin` cannot be
that book, being extracted from CCRL 40/15 itself. Lumbra's GigaBase, its **Elite** database, is
master over-the-board play, the theory testers' books are drawn from. The same theory serves Rotor
on Lichess better than CCRL testers' choices, so one book does both.

## The source

- Lumbra's GigaBase (Michael Jansen), https://lumbrasgigabase.com/en/download-in-pgn-format-en/,
  over 12 million OTB games, released monthly in the first week. Files on Mega.nz as `.7z`.
- **OTB Elite -- ELO > 2400**: the OTB games where both players are rated 2400 or more, 194.64 MB
  as 7z, version 2026-10-11 checked 2026-10-13. Downloaded by hand (Mega needs a browser), kept on
  Drive as `data/lumbra/LumbrasGigaBase_OTB_ELO2400.pgn.zst`, its version recorded beside it in
  `data/lumbra/version.txt`. A new release replaces it when a generation's book is rebuilt.
- **The file on Drive today** (an earlier release, dated 2026-07-08 on Drive, so probably July's):
  863,790 games, counted
  2026-10-13 by `[Event ` headers. By year: about 134k before 1994, 729,230 from 1994, 638,864 from
  2000, 425,147 from 2010, 264,658 from 2016; flat at 25k to 32k a year since 2008, 11k in 2020
  (COVID); almost nothing before 1971 (FIDE's first rating lists); 78 games without a year. Since
  2010, 77,613 games (18%) have an `[Event]` naming rapid, blitz, bullet, Armageddon, online or
  titled play.

## The rule (agreed 2026-10-13)

- Each Elite game contributes the positions after each of its first **24 plies** at most (CCRL's
  book is up to 12 moves a side). Games from a `[FEN]` are skipped.
- Positions are keyed by their Polyglot key, so transpositions pool.
- A position is book if **at least N games** reach it. An absolute N, not a share of the parent's
  games: a share marks every move of a rare line, deep in an obscure branch, as book.
- A CCRL game's book is its longest prefix of book positions, at least the 8-ply floor (from the
  standard start), at most 24 plies.
- **Frozen per generation:** built when a generation's teacher is named and kept for it, since a new
  release changes G, the counts, and every game's cut.
- **For Lichess,** each book position keeps every move the Elite games played there, weighted by its
  count (`scaled_weights`, opening-book.md's 16-bit rule), so lichess-bot's `weighted_random` plays
  the masters' distribution. Only positions at or above N are in the file at all, so the replay and
  Lichess read the same positions; a move leading out of the book is still played, and the book ends
  at the next position.

## Choosing N

**N is dynamic, linear in the database's size** (the developer, 2026-10-13): the games G of the
window are counted first, then

    N = max(2, ceil(f * G))

A theory position is reached by a roughly constant share of master games, so its count grows with
G; a linear N keeps the same positions in the book as Elite grows, and f, once calibrated, holds
across rebuilds. A logarithmic N was rejected: it grows slower than the counts, so every rebuild
would admit rarer positions and run past CCRL's books more. The floor of 2: a position one game
reaches is never book. For scale, G = 400k and f = 1e-4 give N = 40.

On CCRL 40/15, where the comment rule gives the book, for f giving N from about 5 to 200 (refined
around the best). The commented 40/15 months already on Drive (`*.commented.pgn.zst`) are the only
place the comment rule can still be read, so they are **kept** at least until f is chosen, and
again for any recalibration; the bare downloads do not replace them.

- **short:** the share of games where the Lumbra book stops before the comment rule's book;
- **past:** the share where it runs beyond it, into an engine move;
- the distribution of the difference, in plies.

f is chosen where **past** stays rare, accepting some **short** (the floor bounds it, and the
plies it leaves in are theory positions, repeated but sound). The same calibration settles two
choices, each run as its own arm: the window's first year (every year, or from 2000 or 2010:
CCRL's books follow modern theory), and dropping the games whose `[Event]` names rapid or blitz.
One refinement, only if the calibration asks for it: a stricter N past ply 16, if running past
gathers in deep plies.

The book's record carries the Elite release, the window, the filter, G, f, N, and the calibration's
**short** and **past**. A rebuild recomputes N from the new G with the same f; f is recalibrated
only if the record's **short** or **past** drift.

## The code

`arena book` (`crates/arena/src/opening_book.rs`) builds `ccrl.bin` for lichess-bot; most of it
carries over.

- **Reused as is:** `polyglot_key` (with its en passant rule), `polyglot_move`, `POLYGLOT_RANDOM`,
  `Entry`, `book_entries`, `scaled_weights`, `write_book`, `write_atomically`, the record, the upload
  to Drive, and their tests.
- **Shared:** the Polyglot core moves out of `arena` into a crate both `arena` and `smithy` use,
  since `smithy chunk` reads the book to mark CCRL games.
- **New:** the input (the Elite PGN on Drive), the window and the event filter, the first 24 plies
  of every game in place of `tester_plies`, the count per position, and the cut at N. `arena book`
  builds `lumbra.bin` instead of `ccrl.bin`, and moves it and `lumbra.txt` up to Drive's
  `data/books/` as it does today.
- **`smithy install`** installs `lumbra.bin` as `rotor.bin`, its record in `rotor.txt`; `ccrl.bin`
  is no longer built or installed.
- **`smithy chunk`:** a CCRL game's book is its longest prefix of `lumbra.bin` positions, between the
  8-ply floor and 24 plies; the comment rule (`ccrl_book_plies`, `is_ccrl_book_comment`) goes, kept
  only in the calibration. TCEC's `{ Book exit }` rule is unchanged.
- **Memory:** every (position, move) pair is held before the cut. 864k games at 24 plies is at most
  about 20M pairs, most deep ones occurring once; if that proves too large, a first pass counts
  positions only and a second keeps the moves of positions at or above N.

## Steps

1. The Elite file, version 2026-10-11, downloaded from Mega, recompressed to `.pgn.zst`, and moved
   to Drive over the old one, with `version.txt`; its games counted.
2. The distinct positions within 24 plies counted, for the memory estimate.
3. The Polyglot core moved to the shared crate; `arena book`'s tests still pass.
4. The Lumbra book command: count, cut at N, write `lumbra.bin` and `lumbra.txt`.
5. The calibration on CCRL 40/15: **short** and **past** for each N, window and filter; chosen with
   the developer.
6. `smithy chunk` marks every CCRL list with `lumbra.bin` (ccrl-sources.md); the bare downloads.
7. `smithy install` installs `lumbra.bin`; `ccrl.bin` retired, opening-book.md archived with a
   "superseded, see lumbra-book.md" note.
