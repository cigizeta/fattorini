# A book judged by any UCI engine -- Implementation plan

Status: steps 1 to 8 done (2026-11-26): `cargo test -p smithy --release` passes, clippy's one
warning fixed. Step 9, the developer's run, next.
Changed on the way: no SHA-256 logged (no hashing crate in the workspace, and nothing pins the
book); a position with no legal move is written with depth 0.
Changed on the way: an engine that stops short of the depth (as on a forced mate) is not an
error; the score of the deepest depth it completed is kept, and that depth goes in the entry. Design: uci-book.md. The old book code stays untouched
until the developer says otherwise.

## Choices made by this plan (confirmed by the developer, 2026-11-26)

- **The UCI client is the smithy's own, small,** in the new module: spawn, `uci`/`uciok`,
  `setoption`, `isready`/`readyok`, `ucinewgame`, `position fen`, `go depth`, the last `info`
  score at the final depth, `bestmove`. The arena's `UciClient` is tied to its match runner
  (`Fault`, clocks, CPU pinning); sharing it would mean moving it out of the arena, more change
  than the hundred lines it saves. Likewise the registry: the smithy reads `path` and `options`
  of one `data/engines.yaml` entry with a struct of its own, and refuses an entry whose `kind`
  is not `uci`.

## Commands (hidden: in no usage line, as the smithy's other hand-run commands)

```
bin/smithy uci-book <engine> <version>
```

Builds `data/books/uci-book-<engine>-v<version>.bin` (the evaluations), resuming from
`data/books/uci-book-<engine>-v<version>.progress.tsv`.

```
bin/smithy uci-book-polyglot <evaluations file> [--temperature 20]
```

Writes `<evaluations stem>-t<T>.bin` beside it, a Polyglot book, and logs the counts.

## Steps

1. **The openings snapshot.** `crates/smithy/openings/a.tsv` to `e.tsv`, copied from aio64 by the
   developer (a fenced command), and a `README.md` there naming the source (Lichess's
   chess-openings repository), the snapshot's date and CC0. `.gitattributes`: the TSVs stored LF.
   Embedded in the smithy with `include_str!`, so a build reproduces from a clean checkout.
2. **The opening tree** (`crates/smithy/src/uci_book/tree.rs`): each TSV's `pgn` column parsed
   with `shakmaty`'s SAN and resolved by `pgn_replay::try_resolve_san` on Fattorini's board; every
   position along every line, prefixes included; edges (parent key, Polyglot move, child key) and
   the child positions, deduplicated by Polyglot key. A line with an unresolvable move keeps its
   moves so far and is counted. Unit tests: a two-line tree shares its prefix; a transposition
   gives one child; castling encodes as king onto rook.
3. **The format** (`crates/smithy/src/uci_book/format.rs`): magic `FUCB`, format version 1, book
   version, depth, engine `id name` (length and UTF-8), entry count; then 16-byte little-endian
   entries sorted by key, one per position: Polyglot key (8), evaluation from White's side
   (2, signed: cp clamped to +/-30,000, a mate in N as +/-(32,000 - N)), depth (1), reserved
   (5). Write and read. Unit tests: round trip, a mate both ways, a short or foreign file
   refused.
4. **The UCI client** (`crates/smithy/src/uci_book/engine.rs`): as above; the engine started in a
   new process group (as the arena's, so Ctrl+C reaches the smithy alone); `Threads` forced to 1
   after the entry's options; per position `ucinewgame`, `isready`, `position fen`,
   `go depth 20`, waiting for `bestmove` with no timeout; the score and depth taken from the
   last `info` line carrying a score that is not a bound, refused if none came. `id name` kept
   for the header. Unit tests on the `info` parsing.
5. **The build** (`crates/smithy/src/uci_book/build.rs`): the tree; the progress file (header
   line `# <id name>, depth 20`, then key, FEN, encoded evaluation per row, appended and flushed
   as each comes), a different header refused with a message; the children without a row
   queued; one thread and one engine per core (`available_parallelism`); a position with no
   legal move scored by the rules (checkmate: mate in 1 for the side that gave it, as aio64;
   stalemate: 0); every engine score turned from the side to move to White's; progress on the console (done of total, rate, time left); at the
   end the book file holds every searched position, with its entry count, size, time and
   SHA-256 logged. Ctrl+C stops the engines and keeps the progress file.
6. **The conversion** (`crates/smithy/src/uci_book/polyglot.rs`): the opening tree rebuilt from
   the embedded snapshot; per position, each move scored by the evaluation of the position it
   leads to, from the side that plays it (as is for White, negated for Black), in cp (a mate +/-10,000); a child missing from the file refused (a file
   from another snapshot); weight e^((score - best) / T) times
   65,535, rounded; 0 left out and counted; entries sorted by key, then weight descending;
   written with `polyglot::book_bytes`. Log: positions, moves, moves left out. Unit tests: two
   moves 46 cp apart at T = 20 weigh about 10 to 1; a mate; a zero left out; the bytes read back
   by key.
7. **`main.rs`:** the two commands, in no usage line; a test that `USAGE` names neither.
8. **Checks:** `cargo test -p smithy --release` and `cargo clippy --workspace --all-targets`
   clean; `DEV.md` gains the two commands.
9. **Run (the developer):** `bin/smithy uci-book stockfish 1`, then
   `bin/smithy uci-book-polyglot` at T = 20; the Polyglot file checked in a GUI on a few
   openings; lichess-bot's config pointed at it when the developer chooses.
