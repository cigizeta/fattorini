# The replay's book marker from the master graph

**Superseded, see ccrl-sources.md** (2026-10-14: no chunks, the replay detects the book by month).

**Status:** design, not implemented. Moved out of book-building.md, which describes the opening
book as implemented; this is the use of the same graph still to build.

**Why** (the developer, 2026-10-14): CCRL games are no longer downloaded with comments, so CCRL's
comment rule can no longer say where the testers' book ends. The master graph says it instead. This
is the purpose of the whole book work, beside the Lichess book.

**The marker is the graph's node set, not the book.** The `.bin` keeps only moves played at least
\\(N\\) times, weighted for Rotor to play; the question the marker answers is "is this known theory?",
which the graph's positions answer directly.

**The rule:**

- A CCRL game's book is its **longest prefix of plies whose resulting positions are nodes of
  `graph.txt`**, by Polyglot key, so transpositions count.
- At least `CCRL_BOOK_FLOOR_PLIES` (8) plies, as today: CCRL's books are never shorter.
- At most 24 plies (CCRL's books are up to 12 moves a side; lumbra-book.md's cap). Past it, a graph
  node is more likely an engine following master theory than a tester's choice.
- The same graph as the Lichess book, `data/books/<source>/graph.txt`, at its fitted \\(N\\).
- TCEC is untouched: its `{ Book exit }` marker stays its rule.

**In `smithy chunk`:**

- It loads the node keys of `graph.txt` from the remote once, as a `HashSet<u64>`, keyed by
  `rotor::polyglot::polyglot_key`.
- `ccrl_book_plies` replays the game's moves and stops at the first position not in the set, then
  applies the floor and the cap. The comment rule (`is_ccrl_book_comment` and its helpers) is
  deleted.
- The CCRL archive files are the bare downloads (ccrl-sources.md names them).
- `chunked.txt` records the graph's header, so a chunk always says which graph cut its books.

**Open:**

- **The `.count` files.** `smithy chunk` checks every CCRL month against a `.count` written with the
  comment rule. Under the new rule the counts differ: the `.count` must be written with the graph
  rule (by whom: the download script cannot, since it has no graph), or the check dropped for CCRL.
- **Months already chunked** under the comment rule: kept as they are (a month never changes once on
  the remote), or chunked again so every CCRL game follows one rule.
- **Which source's graph** `smithy chunk` reads: named in `configs/corpus.yaml`, or fixed.
- **lumbra-book.md:** its counting step, its calibrated \\(N = \lceil fG \rceil\\) and `lumbra.bin` are
  superseded by book-building.md; it is archived with a "superseded, see book-building.md" note once
  this design is agreed.
