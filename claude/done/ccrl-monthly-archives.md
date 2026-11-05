# CCRL 40/15 monthly archives on Drive, as `.pgn.zst` with `.count`

**Done:** `bin/ccrl-download.sh` has run: CCRL 40/15 months from 2005-12 are on Drive as `.pgn.zst`, under `data/games/original/ccrl/`, not the `ccrl-40-15/` named below. The book-move rules are now applied by `rotor-arena replay` to chunked games (corpus-windows.md).

## Status

Script written (`bin/ccrl-download.sh`), not yet run.

## Decided with the developer

- **Source**: CCRL 40/15's commented monthly archives, from 2005-12 on:
  `https://computerchess.org.uk/ccrl/4040/games-by-month-commented/<YYYY-MM>.commented.[<games>].pgn.7z`,
  the links read from `games.html` (the game count in the name cannot be guessed). Commented only.
- **Only complete months**: month M is taken from the 5th of month M+1 on (September from 5 October).
- **Per month, one at a time, throttled**: download the `.7z` locally, decompress, compress with
  `zstd -19 --long=27`, count, and move only the `.pgn.zst` and its `.count` to
  `/mnt/g/My Drive/rotor/data/games/ccrl-40-15/`. Everything else is deleted. No brackets in our
  names: `2005-12.commented.pgn.zst`.
- **No Rust**: a shell script with `curl`, `7z`, `zstd` and `awk`.

## What `.count` counts

The positions Rotor will analyse at depth 9: **every position from the first non-book move to the
final position, inclusive**. The final position matters most: its label is the result itself.

- A move is a **book move** when its comment holds **no score/depth pair**
  (`[-+]?([0-9]+\.[0-9]+|M[0-9]+)/[0-9]+`, as in `+0.19/12` or `(Bxd5) +0.25/14`), **or a `0s`
  token** (exactly `0s`, after the start, a space or `/`, and before the end, a space or `,`, so
  `10s` or `0.50s` never match). An engine's move has its evaluation with its depth and a time
  above 0 s. Book formats seen so far: no comment at all (2011-07), `{1s}`, `{0s}` and
  `{Both last book move 0s}` (2005-12), `{+0.00/1 0s}` (2006-02), and `{book}`. Each earlier rule
  missed one: `book` or `0s` missed the first two, and score/depth alone missed the last but one.
- **The book is a prefix**: it ends at the first move that is not book. From there on everything
  counts, whatever its comment (a forced move or a ponder hit mid-game is not book).
- **A floor of 8 plies**: the first 8 plies are book in every game, whatever their comments. Without
  it, a game in which one side has no book (seen in 2006-03: White thinks from move 1, Black plays
  book to move 9) or one started from a FEN would count the start position and the most repeated
  openings. 8 is below the typical book (15 to 20 plies), so games with a normal book are untouched.
  "No book move detected" is still reported before the floor, as the sign of an unknown format.
- **The floor is for the standard start only.** A game without a `[FEN]` header starts from the
  standard position, and gets the floor. A game with one starts where its tester chose, past any
  opening, so it gets only its detected book. The report counts the games from a FEN.
- Per game, with `p` plies and `b` book plies after the floor (`b = min(max(detected, 8), p)`):
  `p - b + 1` when `p > b`, else 0.
- Duplicates across games count every time.

**Checking the plies.** Every game with a `[PlyCount]` header, written by the GUI that recorded it,
gets its counted plies compared with it; the report says how many games were checked and how many
disagree, and the script stops when more than 1% do (a few can be a GUI's own miscount). Checked
by hand first on 2006-03's game 1729: 184 plies counted, `[PlyCount "184"]`, 177 trainable positions.

The script checks the games it counts against the count in CCRL's name, and reports per month the
games, the book moves, the trainable positions, and the games with no book move detected. The book
format changes from file to file, so the last number is the warning to watch: many such games mean
a book format the rule does not recognise. A month is recounted by removing its `.count` on Drive and
rerunning: a `.pgn.zst` without a `.count` is counted from Drive, with nothing downloaded.

## Resuming and Ctrl+C

Every step resumes, and Ctrl+C is safe at any point:

- A month whose `.pgn.zst` and `.count` are both on Drive is skipped, so a rerun also picks up new
  months.
- The download goes to `<month>.pgn.7z.part`, resumed with `curl -C -`, and is renamed only once
  complete: a local `.pgn.7z` is never downloaded again.
- The recompression writes `.tmp` and renames it once finished: a local `.pgn.zst` is never
  recompressed. The count is always recomputed, from the `.zst`, which also proves it decodes.
- The copy to Drive goes through `.tmp` names, the `.pgn.zst` first and its `.count` last, so a
  `.count` is only ever beside a complete file.
- Ctrl+C (and every start) removes the unfinished `.tmp` files, locally and on Drive, and keeps the
  `.part` and every finished file.
