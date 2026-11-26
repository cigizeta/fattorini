# A book judged by any UCI engine -- Design

Status: **superseded, see book-expansion.md** (2026-11-26). Built (uci-book-implementation.md),
then replaced the same day: the Lichess openings are not in the book, and the code is rewritten.
The engine, evaluations from White's side, the Polyglot softmax and the files' place carry over. A full redesign of book building, ported from aio64's book
(master-book-v1.md and book-expansion-v1.md in aio64) and rewritten in Rust here. No plan yet.

## Decided

- **Rewritten in Rust, in this repository (the developer, 2026-11-26).** aio64's Kotlin book tool
  (`bookGen`) is the starting design, not code to keep; book building leaves aio64 for good.
- **Any UCI engine judges (the developer, 2026-11-26).** The code speaks plain UCI and assumes no
  particular engine. The engine is named by its entry in the gitignored `data/engines.yaml`
  (the arena's registry, `examples/engines.yaml`), so no path is ever committed. In practice the
  judge is Stockfish installed with winget (the registry's `stockfish` entry).
  This reverses search-judged-book.md's "Fattorini judges, never another engine" (2026-11-13).
- **For anyone to read (the developer, 2026-11-26):** the deliverable is a carefully generated
  Polyglot file, usable by any Polyglot reader (lichess-bot among them).
- **Two stages (the developer, 2026-11-26):**
  1. the engine's evaluations, alone, in a custom format of the book's own: the costly search,
     run once;
  2. a cheap conversion of that file to Polyglot weights by softmax, each move weighing
     e^(score / T), T = 20 cp, the score from the mover's side. A new T never searches again.
- **A smithy command, a hidden feature (the developer, 2026-11-26):** run from the command line
  (`bin/smithy ...`), on one machine; not on the page, not dealt to the arenas.
- **The old book code is deleted (the developer, 2026-11-26):** book.rs (the LumbrasGigaBase graph
  and Polyglot `write`), book_scores.rs, the arena's `score` chore and the page's Books screen.
  search-judged-book.md's step 9 does not run; that document and book-building.md go to
  `archived/`, superseded by this one. **When:** only when the developer says so, not as part of
  this work (2026-11-26).
- **No expansion yet (the developer, 2026-11-26):** the book is the trunk alone; expansion beyond
  it (book-expansion-v1.md in aio64) is designed later.

- **What is scored, as aio64 does (the developer, 2026-11-26):** each book move by a search of the
  position it leads to, single PV, so every book move gets a score; a position reached by several
  move orders is searched once. A position with no legal move is scored by the rules: checkmate
  a mate in 1 for the side that gave it, stalemate 0.
- **Resumable, as aio64 does:** evaluations appended to a progress file as they come; its header
  names the engine and the depth, and a file from another engine or depth is refused.
- **Polyglot scaling (the developer, 2026-11-26), as search-judged-book.md decided:** per
  position, the best move weighs 65,535 and each other its softmax share of it, rounded; a weight
  rounding to 0 is left out and counted, never written as 0; a mate counts as +/-10,000 cp.

- **The custom format: positions and their evaluations, no moves (the developer, 2026-11-26).**
  The job evaluates positions, so an entry is a position and its evaluation; aio64's entry
  (position, move, evaluation) was a mistake not to copy. Binary, little-endian. A header (a magic
  of our own, format version, book version, depth, the engine's `id name`), then 16-byte entries
  sorted by key: the position's Polyglot key (8, from polyglot.rs), the evaluation from White's
  side (2, signed: centipawns, mates near +/-32,000 as aio64 encodes them), the depth (1),
  reserved (5).
- **From White's side (the developer, 2026-11-26),** as GUIs show evaluations: positive is good
  for White, whoever is to move. The build turns the engine's score, given from the side to move,
  to White's.
- **Moves enter at the conversion alone:** it rebuilds the opening tree from the embedded
  snapshot, so it knows each position's side to move, and scores each move by the evaluation of
  the position it leads to, from the side that plays the move (White's evaluation as is for a
  White move, negated for a Black one).

- **Files in `data/books/` (the developer, 2026-11-26):** per build, the progress file, the
  evaluations file in the custom format, and one Polyglot file per temperature (T in its name, as
  `-t20`). When ready, lichess-bot's config (in its own repository) points at the Polyglot file;
  the developer edits it, never this work.

- **Search settings, as aio64's (the developer, 2026-11-26):** a fixed depth of 20, one thread
  per engine process, one process per core, `ucinewgame` before each position so a rebuild gives
  the same evaluations; the entry's `options` from `data/engines.yaml` sent, Threads forced to 1.

- **Trunk, as aio64's (the developer, 2026-11-26):** every position along every line of Lichess's
  CC0 chess-openings files (`a.tsv` to `e.tsv`), prefixes included; a position reached by several
  move orders counts once. The files are committed in this repository as a snapshot (as in
  `crates/smithy/openings/`), with a note naming their source, the snapshot's date and CC0; a
  refresh is a deliberate new snapshot, never a download at build time.

## Open questions

None: the design is complete. Next, the implementation plan.

