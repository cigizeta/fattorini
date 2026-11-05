# Opening book from CCRL's testers' moves, for lichess-bot

## Status

**Superseded, see book-building.md** (2026-10-14): `ccrl.bin` and its stages are retired; the
Lichess book is built from master games by `arena book`. Its Polyglot code, weights and install
carried over.

Designed with the developer, 2026-10-02. Implemented (`src/arena/opening_book.rs`, and
`install_book_or_exit` in `smithy`, `arena book`). Open: judging book lines by search,
not only by frequency (TODO.md).

The key table is python-chess's `POLYGLOT_RANDOM_ARRAY`, read from the copy in lichess-bot's venv;
the test keys, move encoding and en passant rule were fetched from the specification
(`http://hgm.nubati.net/book_format.html`). python-chess also confirms the en passant rule:
"Legality of the potential capture is irrelevant."

`data/books/ccrl.txt` beside the book records how it was built (months, games used and skipped,
longest line, positions, entries, time); `smithy install` carries it into `rotor.txt` with a
`book_` prefix, or writes `book none`.

## Purpose

Rotor plays on Lichess from move 1, with no book. The book is for competition only: lichess-bot reads
it and plays its moves without asking Rotor. It has nothing to do with the training pipeline (chunks,
replay, datasets), and shares no code with it.

## Decided

### Source

- Every CCRL month on Drive, `data/games/original/ccrl/<YYYY-MM>.commented.pgn.zst`.
- **Only the testers' book moves**: each game's prefix of moves whose comment marks them as book,
  up to the first move an engine chose. CCRL testers start their games from their own curated opening
  books, so these moves are the testers' choices, not the engines'.
- The CCRL comment rule, as a fact about CCRL's PGN: a move is book when its comment holds no
  score/depth pair (`+0.19/12`, `-M5/20`), or a token that is exactly `0s`. The book is a prefix: it
  ends at the first move that is not book. A move with no comment while still in book is book.
- **No floor.** Unlike the training data, which counts the first 8 plies of a game as book whatever
  their comments, the book takes only moves the comments mark as book: an engine's choice must not
  enter a book of testers' choices.
- A game in which no engine move is detected is skipped: with no move commented as an engine's,
  its comments cannot tell the testers' moves from the engines', and taking the whole game as book
  would let engine moves in. Found while implementing.
- A move that resolves to no legal move ends that game's contribution, and the game is counted as
  unreadable in the record.
- Games with a `[FEN]` header are skipped: they start where a tester chose, not from the initial
  position, and their moves would hang off positions the book never reaches from move 1.
- Results are not used. A line's game outcomes are not an alpha-beta evaluation of it; judging book
  lines by search is a future discussion (TODO).

### Weights

- Per position, keyed by its Polyglot key, so games reaching it by transposition pool their counts:
  for each move played there, the number of games that played it, `n_i`.
- The probability of a move is `n_i / N`. lichess-bot's `weighted_random` picks a move with
  probability weight over the position's sum, so storing weights proportional to `n_i` plays the
  testers' distribution exactly: the softmax over log-frequencies at temperature 1.
- **Weights, not exact frequencies.** A Polyglot weight is 16 bits. Where a position's largest count
  exceeds 65535, every count of that position is divided by the same factor, so the ratios hold up
  to rounding; a weight that would round to 0 is set to 1, since 0 means "never play" to many readers.
  Positions that fit keep their raw counts.
- Every tester move is in the book: no minimum number of games.

### Temperature

1 for now, which needs nothing: raw counts and `weighted_random`. lichess-bot has no temperature of
its own (`weighted_random` is 1, `best_move` its limit at 0, `uniform_random` its limit at infinity),
so another temperature would be baked in at build time as `n_i^(1/T)`, rescaled into 16 bits: a
`--temperature` flag, not built now.

### Polyglot

- The standard Polyglot `.bin`: 16-byte entries (key, move, weight, learn), sorted by key, `learn`
  zero. The key's random table and its layout fetched from the Polyglot specification, not recalled.
- **En passant, carefully** (chess.com gets it wrong): the key includes the en passant file only when
  a pawn of the side to move stands beside the pawn that just made the double step, ready to capture,
  not whenever a double step happened. Implemented explicitly from the specification, independent of
  how Rotor's `Board` stores its en passant square. Whether a pinned pawn still counts is read from
  the specification.
- Polyglot's move encoding: castling as the king moving onto its own rook (`e1h1`), promotions by
  their own field. From the specification.
- Checked against the specification's published keys (positions with and without a capturable en
  passant among them), and against python-chess, which is what lichess-bot reads the book with.

### Command

`arena book`: reads every CCRL month from Drive, builds the counts in memory, writes the book
to `data/books/ccrl.bin` (via a `.tmp` and a rename). arena is the competition binary and has
no libtorch.

### Install

`smithy install` already puts what lichess-bot runs in the data root's `bin/`, under names that
never change (`rotor`, `rotor.nnue`, `rotor.txt`), so lichess-bot's config is written once. Every
install also replaces `rotor.bin` there with the current `data/books/ccrl.bin`, exactly as it
replaces the engine and the network, the hand-crafted install included, and names it
in `rotor.txt`. With no book built yet, the install goes on without one and says so, removing any
old `rotor.bin`, so lichess-bot never plays from a book `rotor.txt` does not describe.

### lichess-bot

In its `config.yml`, set once: `polyglot:` with `enabled: true`, `book: standard:
[<data root>/bin/rotor.bin]`,
`selection: "weighted_random"`, and `max_depth` at its default (the book itself stops at ply 20). `min_weight` and `normalization` do not affect `weighted_random`.

## Decided after the first build (2026-10-02)

- **The book stops at ply 20, regardless.** The first build read 2,429,853 games, used 2,429,377
  (476 without an engine move, 393 of them in 2026-05), and found a longest testers' line of 117
  plies, far past any curated opening: engine moves the comment rule took for book. Rather than chase
  the rule, every game contributes at most its first 20 plies (`MAX_BOOK_PLIES`). The record keeps
  `longest_line_plies` (before the cut) and adds `max_book_plies` and `games_cut`. The second build:
  276,324 games cut, 535,825 positions, 725,937 entries.
- **The book lives on the remote.** lichess-bot runs on another PC, so `arena book` moves
  `ccrl.bin` and then `ccrl.txt` to `data/books/` on the remote (`remote::move_up_replacing`: copied
  via `.partial`, renamed over the old one, the local file removed only after), and both installs
  read the book from there. The book is the second remote file ever rewritten, after the network
  pointer. `install hand-crafted` now checks that the remote is mounted, so an unmounted drive is
  never taken for a missing book, which would remove `rotor.bin`.

## Verification

- Unit tests: the comment rule, the per-position counting with transpositions, the 16-bit scaling
  (ratios, no zero weights), the Polyglot keys against the specification's test positions, the move
  encoding.
- A cross-check of a built book with python-chess: every entry's key resolves to a position whose
  move is legal.
