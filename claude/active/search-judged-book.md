# A book judged by Fattorini's own search

Status: **steps 1 to 8 done** (2026-11-13): `cargo test -p fattorini -p arena -p smithy
--release` and `cargo clippy --workspace --all-targets` clean. Step 9, the run, waits for test 7. Changed on the way: the arena's bench node count travels in every score record
(`bench_nodes`, benched once per player on the arena) rather than on joining, so no new message
is needed; a record whose count, commit, depth or position differs refuses that arena's session
for the scoring. Job words: `--score-book <source> <network>@<tuned|untuned> <depth>`; a book job
`--book <source> --weights softmax --temperature <T> --scores <file>`.

## Why

Lichess's other weak point after time management (the developer, 2026-11-13). The book today
(book-building.md) is LumbrasGigaBase OTB 2400+, a move kept when played at least N = 9 times,
weighted by the points it scored: human results alone. Seen on Lichess (TODO.md, 2026-11-11): it
sometimes leaves the bot worse out of the opening, a move good in human hands being dubious in
engine play. Meanwhile lichess-bot's `min_weight` was raised to 10 and `max_depth` lowered to 12.

For Lichess alone: CCRL testers use their own books, and the arena's matches keep the UHO book.

## Shape

The human graph kept, for its natural lines and variety; every book move judged by search, so no
move gets in on human results alone.

## Decided

- **Fattorini judges, never another engine (the developer, 2026-11-13):** Stockfish's judgment
  would be cheating, carrying another engine's understanding into Fattorini's games. Its own
  search is also the best judge of the positions it handles badly, the Lichess problem itself.
- **A fixed depth of 20 (the developer, 2026-11-13):** about what Fattorini reaches in a blitz
  opening, and reproducible on any machine. The whole book costs a few hours (the cost, below), so it can be rebuilt as the search improves; the depth can be raised then.
- **No move dropped by score; weights by softmax (the developer, 2026-11-13):** the minimum count
  (N = 9) stays the only filter. Each move's weight is proportional to e^(score / T), the score in
  cp from the mover's side, T = 20 cp to start: a move 46 cp worse is played 10 times less often,
  one 100 cp worse about 1 in 150.
- **T changeable without searching again (the developer, 2026-11-13):** the search's scores are
  kept in a file of their own, every book move with its score; the Polyglot book is made from that
  file in a separate, cheap step, which takes T. The `min_weight` of lichess-bot (10) cuts small
  Polyglot weights, so the scaling of weights to integers is settled in the plan.
- **The whole book searched (the developer, 2026-11-13):** every position, not only those
  lichess-bot reaches (`max_depth` 12), since the book may serve other applications; the cost at
  depth 20 is small either way.

- **The scores file, so the search is never rerun for a weighting change (the developer,
  2026-11-13):**
  1. Every book move scored by its own search, of the position after it, not only the
     position's best move.
  2. Scores raw: cp from the mover's side, unrounded, unclipped, mates marked as mates.
  3. Self-contained: each row holds position, move, score and human game count, so T, the
     integer scaling, the minimum count and `min_weight` are applied from this file alone.
  4. A header names what the scores depend on (network, its tuned search parameters, search
     build, depth); only a change there calls for a new search, a deliberate rebuild. The search
     is the one Lichess plays: the network with its tune (the developer, 2026-11-13), whose
     pruning also sets what depth 20 costs.
  5. Incremental and resumable: rows appended as searched, keyed by position and move; a stop,
     a crash, or a lower minimum count searches only the missing rows.

- **Search tried before, no reason to stop (the developer, 2026-11-13):** book.rs's header says
  Fattorini's search, tried as a correction of the masters' shares, moved them about 3 cp at the
  root; Fattorini has changed a lot since. That trial measured the root, where every move is sound,
  and corrected the human shares rather than replacing them.
- **The first gate, before any book reaches Lichess:** from the scores file, the book moves at least
  50 cp below their position's best, and how many lie within lichess-bot's 12 moves. Almost none,
  and the old conclusion holds: we stop there.
- **lichess-bot's hidden cut:** its `normalization: "sum"` makes `min_weight: 10` drop any move under
  10% of its position (book-building.md); at T = 20, a move about 46 cp worse than a single rival.

- **The cost (2026-11-13):** a "correction" to 27 to 54 hours took a search's 0.5 to 1 s as a whole
  machine's; it is one thread's. Test 7's PGN (180+2, the tuned Lichess network, one thread per
  arena worker) shows depth 21 to 27 completed in 2 to 10 s, so depth 20 in about 1 s or less:
  196,000 searches are 1 to 2 hours across every arena's 48 workers, 3 to 5 hours on one machine of
  20. Middlegames with a warm table, so a fresh table costs somewhat more; `fattorini bench 20` on
  an idle machine gives the real figure. Cheap enough to rebuild as the search improves.
- **Each search on one thread,** many positions at once across the cores, so the scores are
  reproducible; Lazy SMP's are not.

- **On the arenas, started from the page (the developer, 2026-11-13):** all three arenas search.
- **Two jobs (the developer, 2026-11-13):** the book job keeps `fit`, `graph` and `write` in the
  source slot; a new job, "score the book", takes the arena slot as tests and tunes do, and
  refuses to start without the source's graph. Graph, then score, then write with softmax, each
  started from the page.

- **A `score` chore kind (the developer, 2026-11-13):** a fifth `TaskKind` beside self, pair,
  tournament and spsa. One task is one book move: an entry of count 1, one player (the network
  with its tune), tags naming the parent's Polyglot key, the Polyglot move and the FEN after it.
  The arena searches that FEN, not a UHO opening: one thread, a fresh table, the chore's
  `depth <d>`. Its result is a score record, not a PGN: the tags back, the score from the side that
  played the move, the depth and the nodes, sent as games are. A chore holds a few hundred book
  moves, minutes of work, topped up as a test's; claiming, dealing, stopping and the outbox are
  unchanged.

- **The scores file (the developer, 2026-11-13):**
  - Beside the graph on Drive, one file per scoring:
    `data/books/<source>/scores-<network>-<tuned|untuned>-d<depth>.txt`; a new network or depth is
    a new file, never an overwrite.
  - Header: the source, the graph's `min_games` and `built`, so scores never meet another graph;
    the network's path and its search parameters in full; the build's version; the depth; one
    thread, a fresh table and its size.
  - One row per book move: parent key, Polyglot move, child key, `cp <n>` or `mate <n>` from the
    side that played the move, depth, nodes.
  - Appended locally as results come, copied to Drive every few minutes and at the end.
  - On start: the header checked against the job's; equal, only the moves without a row are
    dealt; different, refused.

- **Identical scores on any arena (the developer, 2026-11-13):**
  - Each score record carries its arena's commit; one differing from the header's, or a
    `(modified)` tree, is refused, logged, and its move dealt again. The claim's commit is shown
    today, never checked.
  - No tablebases in a score search: rare in the opening, and a difference between machines.
  - An arena joining a score job is benched as any (`loaded_bench`, one thread, a fresh table, the
    network with its tune); its node count, identical on every run of one binary and network, is
    compared with the smithy's, and an arena whose count differs is refused. No move is searched
    twice.

- **`write` from the scores (the developer, 2026-11-13):** `--weights softmax --temperature <T>`
  beside `points` and `counts`, reading the graph and one scores file named on the page. Every move
  played at least N times weighs e^(score / T), from the mover's side; a mate for the mover counts
  +10,000 cp, against it -10,000 cp. Per position the best move gets 65,535 and each other its
  share of it, rounded: lichess-bot normalizes by the sum, so only ratios matter. A weight rounding
  to 0 (about 220 cp below the best at T = 20) is left out and counted, never written as 0. The
  book is `book-n<N>-softmax-t<T>-<scores stem>.bin` and its record, which holds the scores
  file's header beside the graph's.

- **The first-gate report (the developer, 2026-11-13):** made by every softmax `write`, in its
  record and its log, no search added:
  - the book moves at least 50 cp below their position's best, counted and as a share;
  - the same within lichess-bot's reach, plies up to 23 (`max_depth` 12, read to ply
    `2 * 12 - 1`, book-building.md);
  - the chance the old `points` book plays such a move, averaged over the positions within reach:
    the Lichess problem's own measure, since a bad move the old book never picks harms nothing;
  - the worst twenty by that chance, each position's FEN and move, for Scid vs. PC.
  The gate is the developer's call on reading it; no threshold in code.

- **The page (the developer, 2026-11-13),** in each source's section of the Books screen:
  - the table gains a "scores" row: each scores file's network, tuned or untuned, depth, and
    moves scored of the total;
  - a Score book row once the graph exists: network and tuned/untuned selects as the test form's,
    a depth field (20), Score book, disabled while any arena job runs; an existing file's network
    and depth resume it;
  - while scoring runs, its progress where a test's shows: moves done of total, rate, time left,
    arenas refused and records rejected with why;
  - Make book's weights select gains "softmax", showing a select of the complete scores files and
    a temperature field (20);
  - each softmax book shows its T, its scores file, and the report's two numbers: the share of bad
    moves within reach, and the chance the old book plays one.

## Open questions

None: the steps below are the plan.

## Steps

1. **protocol.rs:** `TaskKind::Score` (`score`, one player, counted in `move`s); its entries' tags
   (`parent`, `move`, `fen`); the score record as a task body (the tags, `cp <n>` or `mate <n>`
   from the mover's side, depth, nodes, the arena's commit), written and parsed. Unit tests.
2. **The arena** (chores.rs, a new `score.rs`): a `score` chore's worker searches each entry's FEN
   to the chore's depth, one thread, a fresh table, no tablebases, the player's network and
   parameters; the result negated to the mover's side; sent as games are. The arena's bench node
   count joins what it reports on joining. Unit tests: a known position's record, two runs equal.
3. **The scores file** (smithy, a new `book_scores.rs`): its name, header, rows, written and parsed;
   appended locally, copied to Drive every few minutes and at the end; on start, the header
   checked and the moves without a row listed. Unit tests: round trip, resume, a different header
   refused.
4. **The score job** (smithy.rs): `--score-book <source> <network>@<tuned|untuned> --depth <d>`,
   in the arena slot, refused without the graph; the benching, an arena of a different node count
   refused; chores of a few hundred moves dealt and topped up; records checked (commit, a move
   asked for, none twice) and appended; its progress in `state.json`. Unit tests on the dealing
   and the checks.
5. **`write` softmax** (book.rs): `--weights softmax --temperature <T> --scores <file>`; the
   weights, mates, scaling and zeros as decided; the file names; the record with both headers.
   Unit tests: a position's weights at a known T, a mate, a zero left out.
6. **The first-gate report** in the softmax record and log. Unit test on a small graph.
7. **page.html:** the scores row, the Score book row, its progress, softmax in Make book, the
   softmax books' two numbers.
8. **`cargo test -p fattorini -p arena -p smithy --release`, `cargo clippy --workspace
   --all-targets`** clean; book.rs's header updated (no longer "no search").
9. **Run:** after test 7, `bin/fattorini bench 20` on an idle machine for the real cost; every
   arena on this build; Score book on the Elite source with the Lichess network, tuned, depth 20;
   a softmax book at T = 20; the report read; then the gate.
