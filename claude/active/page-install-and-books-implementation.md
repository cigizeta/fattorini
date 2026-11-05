# Install and book-making from the page: implementation plan

**Status: plan, 2026-10-26, from page-install-and-books.md (design agreed the same day). Step 1
and step 2's child done the same day, tests and clippy passing; step 3 done the same day, tests and
clippy passing; step 4 done the same day, a dev smithy making a points and a counts book of a
1,500,000-game cut of Lumbra's PGN end to end; steps 5 and 6 done the same day, tests and clippy passing; step 7 written the same day, tested on a dev smithy, `pesto` alone
installing; paused there (the developer), its open points in TODO.md.**

**As written, step 7:** an Install box on Home opens an Install screen (`#install`), as Books does:
each claimed machine's `rotor.txt` (network, book, commit, when) and its last install, redrawn at
every poll, then Install (machine, player, book). The player is a flat list of every network a test
may name, not the Tests screen's cascade. `POST /control/install` (control token) resolves the
player, puts its `candidate.txt` and `parameters.txt` in the store every time, then the network
(`publish`), and asks the arena. `MachineView` carries `install` and `installed`.

**As written, steps 5 and 6:** `InstallRequest` carries the claim's boot as well, checked as the
polls' is. `rotor.txt` names the network by its path in the store, its `weights` line gone (the two
said the same). The commit is `protocol::current_commit`, the arena's checkout. `smithy/src/install.rs`
stays until step 7, which removes `smithy install` with it; step 7's smithy puts a network's
`candidate.txt` and `parameters.txt` in the store before the network itself (`publish`).

**Found by step 4's test:** `fit` needs a source of about a million games or more. It anchors N at
`ANCHOR_GAMES`, so a smaller source's N_ref falls to `FIT_GRAPH_N` and no prefix can match its book
size (20,000 and 300,000 games were refused, 1,500,000 fitted). Step 8 says so in DEV.md. The Books
screen also shows how the last book job ended, which it first did not: a quick failure looked like
nothing.

**As written, step 4:** a Books box on Home opens a Books screen (`#books`), as the Tests box
does its own, rather than rows on Home itself; the screen holds one section a source, its stages
in a table, Make book with its weights, and the running book job's name, Stop, and pass over a bar.
The catalog's `books` reads `graph.txt`'s header lines alone.

**As written, step 3:** `Conversion` is now `SourceSlot`, holding a `SlotJob` (`Convert` or
`Book`); its view is `state.slot` on the page (was `state.conversion`), with the job's name
(`holder`) and `kind`, and a book job's pass (`book`) beside the converter's `progress`. `finished`
names the job it was. A stop of `lc0` or `books` stops the slot.

**As written, step 1 and 2:** they land together, since the workspace denies dead code and the
moved module must be reached from `main`. The child is `smithy book-make <source> [--weights
points|counts] [--scale <s>]`. The experiment flags (`--min-games`, `--until`, `--output`,
`--max-plies`) are gone with the command line: the page offers none, and `fit` still cuts its date
prefixes inside. The `pgn` line of `fit.txt` and `graph.txt` now names the PGN under `data/`, not
its full path on the machine.

## A change to the design (confirmed, the developer, 2026-10-26)

page-install-and-books.md has the smithy deal an **install chore**. A chore, though, is game tasks
its workers play and send back as PGN (`protocol::TaskKind`): an install is neither. Proposed
instead: an **install request**, the smithy calling the arena directly, as it already calls an
arena's claim and alive paths (`CLAIM_PATH`, `ALIVE_PATH`). The arena runs it on a thread of its
own beside its workers and answers when done. Nothing else in the design changes.

## A second change: the install answers at once (confirmed, the developer, 2026-10-26)

An install builds `rotor` in release, minutes, where a request to an arena times out at 120 s and
the smithy's claim loop would stall on it. So `POST /install` answers at once: `OK` (started),
`BUSY` (one runs), or `400` (a malformed body), and the arena runs it on a thread of its own. Its
`Alive` gains `install` (`-`, `running`, or how the last one ended) and the arena's `rotor.txt`,
which the claim loop already polls every `CLAIM_POLL`. The smithy resolves the player to its
network's `data/...` path, as a chore's `Player`; the body is that path (or `pesto`) and the book.
The arena reads the network, its `candidate.txt` and `parameters.txt` local first, then from the
store its hello reply named, waiting `NETWORK_TIMEOUT`; an arena not yet joined has no store and
refuses. Steps 5 and 6 land together: the protocol's pieces are used only once the arena answers
them. Install's `process::exit`s become errors, so a failed install says why and the arena lives.

## Steps

Books first (steps 1 to 4), then the install (5 to 7): each half is usable alone.

1. **`book.rs` into `smithy`,** as `smithy/src/book.rs` with its tests. `arena book` and its usage
   lines go. A stage takes a source's name, not a path: its PGN is
   `data/books/<source>/<source>.pgn` on the remote, streamed from there (`remote::remote_path`),
   checked present before the first pass. `fit`, `graph` and `write` keep their outputs and
   formats, so the books already written stay valid.
2. **The book job, a child of the smithy (`book::CHILD_COMMAND`, as `lc0::CHILD_COMMAND`):**
   `<source> [--weights points|counts]`, running the stages whose output is missing, in order.
   Progress as `lc0`'s: lines with a prefix, each a JSON record of the pass (stage, games read,
   total when known), kept for the page; every other line to the log.
3. **One source slot for Convert and the book job:** `Conversion` becomes the slot both use, one at
   a time (decision: they exclude each other), its view saying which job holds it. Started from the
   page as `--book <source> [--weights counts]`, stopped by the same Stop; refused while the slot is
   held, with a 409 naming the holder.
4. **The Books box on Home:** the catalog lists every `data/books/<source>/` holding a
   `<source>.pgn`, with `fit.txt` (its alpha), `graph.txt` (its N and games) and each written book
   (its N and weights). One row a source: those, Make book (points; counts in a select beside it),
   disabled while the slot is held; the pass's bar while its job runs. Control token only.
5. **The install request, in `rotor::pipeline::protocol`:** `INSTALL_PATH` on the arena's port, its
   body the player's name (or `pesto`) and the book (`none`, `keep`, or a remote path); the reply
   says the outcome and the new `rotor.txt`. The arena's alive answer carries its installed
   `rotor.txt`, so the page shows every machine's.
6. **Install's code into `arena`** (`arena/src/install.rs`, from `smithy/src/install.rs`, with its
   tests): the build, the moves aside, `rotor.txt`. The network read as a chore's is, local first,
   then the store (`chores.rs`, `NETWORK_TIMEOUT`); the book from the store. One install at a
   time; the arena's workers play on meanwhile (its games run in-process, so no `rotor.exe` of
   theirs is replaced).
7. **The Install box on Home:** every claimed machine with its installed `rotor.txt` (player, book,
   commit, when); Install: a player (the Tests screen's list, and `pesto`), a book (every written
   book, `none`, `keep`), a machine (the claimed ones). Control token only. `smithy install` goes.
8. **The rest:** `smithy`'s and `arena`'s usage texts and their tests, DEV.md's book and install
   sections, `CLAUDE.md` if it names either command, TODO's install item.
9. **Checks, by the developer:** `cargo test --workspace`; clippy clean; a dev smithy
   (`bin/smithy start --dev`) with a small source PGN: Make book end to end, its bar, Stop, Make book
   again skipping the stages done, Convert refused meanwhile; then an install from the page onto the
   dev arena, `rotor.txt` shown back, and lichess-bot's engine starting from it.
