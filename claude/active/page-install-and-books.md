# Install and book-making from the smithy's page

**Status: design agreed with the developer, 2026-10-26. Implementation plan next; nothing
implemented.**

## Why

The page does everything; every command but `smithy start` is deprecated (terminal-log.md). Two
jobs still need a terminal: `smithy install` and `arena book fit|graph|write`.

## Decisions (the developer, 2026-10-26)

1. **Book-making is a smithy job.** It is data preparation, as Lc0's Convert and PeSTO's corpus
   are, and its only consumer is the install. `book.rs` moves from `arena` into `smithy`; `arena
   book` goes.
2. **It runs beside the arena job, as Lc0's Convert does:** a child process of the smithy's own
   binary, so a long pass stops without touching a running forge and its memory goes with it.
3. **A source's PGN lives in a fixed folder,** listed by the page; no path is ever typed. **On
   Drive, beside its outputs:** `data/books/<source>/<source>.pgn` (the developer, 2026-10-26: it
   needs a reliable backup, and is used rarely, so it is streamed from Drive rather than copied
   to a local disk first).
4. **An install lands on a claimed arena's machine:** the page picks the player, the book and the
   machine, and the smithy sends that machine's arena an install request (not a chore, which is
   games: page-install-and-books-implementation.md), which builds `rotor` and installs it there.

## The page

### Books (agreed, the developer, 2026-10-26)

- **A Books box on Home,** one row per source: its PGN, and which of `fit.txt`, `graph.txt` and
  the books are there, with their N and weights. **Make book** runs what is missing, in order:
  `fit`, then `graph`, then `write`. A stage already done is skipped; redoing one is deleting its
  file, as rare as a new source.
- **Weights:** `points` by default, `counts` offered beside it.
- **Progress:** each pass's games read, as a graphical bar (no text bar, no-text-bars.md).

### Install (agreed, the developer, 2026-10-26)

- **An Install box on Home:** what each claimed machine has installed (its `rotor.txt`, read back
  by the arena), and **Install**: a player (any network player the Tests screen offers, or
  `pesto`), a book (any written book, `none`, or keep the installed one), and the machine.
- **The request:** the player's name and the book's remote path. The arena pulls nothing new: the
  protocol already has every arena on the smithy's commit (tournament-chores.md), so it builds the
  same `rotor` the smithy would.
- **Where install's code lives:** in `arena`, beside the rest of what runs on the arena's machine;
  `smithy install` goes.

## Open

1. ~~The PGN folder~~: settled, decision 3.
2. ~~The network's file~~: settled (the developer, 2026-10-26): the install reads it as a chore's
   network, local first, then Drive, so the target machine needs no copy of the run beforehand.
3. ~~Who may start them~~: settled (the developer, 2026-10-26): the page on the smithy's own
   machine alone (the control token), as every other job.
4. ~~Beside Lc0's Convert~~: settled (the developer, 2026-10-26): a book job and Lc0's Convert
   exclude each other, one slot for both, while one runs the other's button disabled; the arena job
   runs beside either.

Every open question is settled.
