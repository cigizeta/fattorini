# Book expansion -- Implementation plan

Status: steps 1 to 10 done (2026-11-26): `cargo test -p smithy --release` and
`cargo clippy --workspace --all-targets` clean. Step 11, the developer's run, next.
Changed on the way: the conversion replays the book from the initial position to learn each
position's side to move (a Polyglot key does not say it, and the softmax needs the mover's side);
a stored position it does not reach is refused as a broken book.
"A rerun over a known graph asks for no expansion" is fragment.rs's to test (step 7): Dijkstra
always asks its graph for a band's edges, and the graph answers known positions from the
fragments without searching.
A checkmated position's evaluation is a mate in 1 against the side to move (`Mate(-1)`), as
aio64 kept it, so it turns to White's side by a plain negation. Design: book-expansion.md. Replaces uci-book.md's code,
all of it (uci-book-implementation.md). The old book code (book.rs and its kin) stays until the
developer says otherwise.

## Commands (hidden: in no usage line, as the smithy's other hand-run commands)

```
bin/smithy book-fragment <engine>
```

Builds the next fragment of the engine's book: fragment k when fragments 0 to k - 1 exist, at
budget 120 + 10k. Resumes a fragment stopped midway.

```
bin/smithy book-polyglot <engine> [--temperature 20]
```

The union of the engine's fragments as a Polyglot book.

```
bin/smithy book-probe <engine> <moves from the initial position, in UCI>
```

Expands one position and prints what the expansion saw: each search's move, score, loss in
points, admitted or not, depth, nodes and time. For the critical positions (the design), to
judge the threshold; writes nothing.

## Files, in `data/books/`

- `book-<engine>-f<k>.bin`: fragment k.
- `book-<engine>-f<k>.progress.tsv`: fragment k under way; deleted once the fragment is written.
- `book-<engine>-f0-<k>-t<T>.bin`: the Polyglot book of fragments 0 to k at temperature T.

## The fragment format, version 1

Little-endian. Header: magic `FBKF`, format version (2), fragment index (2), budget (4, f32),
charge (4, f32), threshold in points (4, f32), depth (1), reserved (3), the engine's `id name`
as length (2) and UTF-8 bytes, entry count (4). Then the entries, sorted by key, each:

- the position's Polyglot key (8);
- its evaluation from White's side (2, signed: centipawns within +/-30,000, a mate in N as
  +/-(32,000 - N), as uci-book.md);
- the number of moves (1), then per move, in the order found (best first): its Polyglot
  encoding (2) and its score from White's side (2, encoded as the evaluation).

Every fragment of one book must agree on charge, threshold, depth and engine, and fragment k
must hold budget 120 + 10k: anything else is refused, naming the difference.

## Modules: `crates/smithy/src/opening_book/`

- `unit.rs`: centipawns or mate to points, `100 / (1 + 10^(-cp / 400))` (`labels.rs`'s Elo
  convention), a mate 100 or 0; a move's loss in points from the mover's side, clamped at 0
  (a later `searchmoves` search may score a move a hair above the first's best; it is then
  admitted with loss 0).
- `engine.rs`: the UCI client (registry entry, start, `Threads` 1, `ucinewgame`, `isready`,
  `position fen`, `go depth`, `go depth ... searchmoves ...`, the last non-bound `info` score
  and its depth, `bestmove`), written anew.
- `expand.rs`: one position's expansion on one engine: `ucinewgame`; the plain search gives the
  evaluation, the best move and score; then `searchmoves` with the legal moves not yet found
  (Fattorini's `legal_moves`, written by `uci::format_move`), table kept, until a move's loss
  reaches the threshold or no move is left. No legal move: no search, the evaluation by the
  rules (checkmate a mate for the side that gave it, stalemate 0), no moves.
- `format.rs`: the fragment format, written and read, the header checks.
- `dijkstra.rs`: the run, pure, with expansion as a callback, so it is tested without an engine.
  A priority queue ordered by (cost, key), costs in f64; the known graph (every earlier
  fragment and the progress file) answers an expansion without searching; an unknown position
  is handed to the engines.
- `fragment.rs`: `book-fragment`: the registry, the fragments read and checked, the progress
  file, the engines, Ctrl+C, the console progress, the fragment written.
- `polyglot.rs`: `book-polyglot`.
- `probe.rs`: `book-probe`.
- `mod.rs`: the commands' parsing.

## The band: parallel and still exact

Every edge weighs at least the charge (10). So when the queue's least cost is m, every position
in it with cost below m + 10 already has its final cost: expanding any queued position gives
costs of at least m + 10. The whole band [m, m + 10), cut at the budget, is expanded at once,
one position per engine, one engine per core, and its edges are relaxed afterwards in (cost,
key) order. The fragment holds exactly the positions a one-at-a-time run would expand, whatever
the number of cores; only the order of the work differs. Positions already expanded (in a
fragment or the progress file) are relaxed from their stored moves at once, never searched.

## Steps

1. **Delete uci-book.md's code:** `crates/smithy/src/uci_book/`, `crates/smithy/openings/` (its
   README included), the `.gitattributes` line for the openings, the two commands and their
   test in `main.rs`, DEV.md's "The opening book" section (rewritten in step 10). uci-book.md
   and uci-book-implementation.md go to `archived/` with "superseded, see book-expansion.md".
2. **`unit.rs`.** Tests: 0 cp is 50 points; +400 cp about 90.9; a mate 100 or 0; a loss clamped
   at 0; 70 cp below equality is about 10 points.
3. **`engine.rs`.** Tests: `info` parsing (score, depth, bounds skipped); the `go` line with
   `searchmoves`; the registry entry.
4. **`expand.rs`,** over a trait the engine implements, so a scripted engine tests it. Tests: the
   trimming stops at the first move reaching the threshold; it stops when the legal moves run
   out; each `searchmoves` lists exactly the moves not yet found; mate and stalemate give no
   search.
5. **`format.rs`.** Tests: round trip; scores and mates; a header that differs refused.
6. **`dijkstra.rs`.** Tests on a small hand-made graph: costs are minimal over transpositions;
   the run stops past the budget; the band gives the same set as one position at a time; a
   rerun over a known graph asks for no expansion; fragment k holds exactly the positions of
   cost in (budget k - 1, budget k].
7. **`fragment.rs`** and `book-fragment`: the progress file (header: engine, depth, charge,
   threshold; a row per expanded position: key, FEN, evaluation, moves and scores; a row cut
   short searched again), the engines, the band, the console (positions expanded, the band's
   cost, positions per second, the fragment's size so far), Ctrl+C keeping the progress
   file, the fragment written and the progress file deleted. Logged at the end: positions,
   moves, moves per position, the deepest ply, time, searches per position.
8. **`polyglot.rs`** and `book-polyglot`: fragments 0 to k read and checked; per position the
   softmax of its move scores from the mover's side (a mate +/-10,000 cp), the best 65,535, 0
   left out and counted; sorted by key then weight; `polyglot::book_bytes`. Tests: as
   uci-book.md's (46 cp apart about 10 to 1, a mate, a zero left out), now from move scores.
9. **`probe.rs`** and `book-probe`.
10. **`main.rs`** (the three commands, none in the usage, a test of it), **DEV.md** (the three
    commands, rewritten), `cargo test -p smithy --release` and
    `cargo clippy --workspace --all-targets` clean.
11. **Run (the developer):** `book-probe` on the five critical positions, the threshold judged;
    then fragment 0, its size and time read; then fragments while the growth allows. Each
    fragment's numbers go into the run log below.

## Run log

For "Inside Fattorini" and for deciding when to stop: per fragment, its budget, positions,
moves, deepest ply, time and machine role; per critical position, the moves admitted and
their losses.

| Fragment | Budget | Positions | Moves | Deepest ply | Time |
|---|---|---|---|---|---|

## For "Inside Fattorini" (written later, not in this work)

`docs/index.html`'s Search chapter has "Opening books" as a coming entry ("Books built from
games, their lines judged by search."), which no longer describes the book: its line becomes
"A book grown from the initial position by an engine's own judgment.", and its page is written
from the material below and the run log.

1. **Why an engine book.** Fattorini's books came from human games (LumbrasGigaBase), and some
   moves good in human hands proved dubious in engine play on Lichess; game collections also
   carry licensing doubts. Lichess's openings files were tried as a trunk and left out: named
   lines are a catalogue, not a judgment. The book here comes from the initial position and
   one engine's searches alone.
2. **The graph and an expansion.** Positions are nodes, moves edges. To expand a position: a
   search at depth 20 for its best move, then the same search restricted (`searchmoves`) to
   the moves not yet found, again and again, until a move falls 10 points short of the best.
   Why not MultiPV: it fixes the number of lines in advance and re-searches the earlier lines
   at every depth, while trimming stops at the first bad move and pays for one new line per
   step, the table kept warm.
3. **The unit.** Expected score by the Elo formula, `100 / (1 + 10^(-cp / 400))`; a pawn's worth
   of loss costs about 10 points in an even position and less once the game is decided, so no
   separate cutoff is needed. The threshold is a game review's "mistake".
4. **Cost and Dijkstra.** Each ply costs a charge of 10 plus the move's loss: without the
   charge a line of best moves would never end. Dijkstra takes positions by least cost from
   the initial position, so a position reached by several move orders gets its cheapest, and
   a budget stops the run exactly: positions left in the queue all cost more. A worked
   example: 1. e4 (loss 0) e5 (loss 2) costs 10 + 10 + 2 = 22.
5. **Fragments.** The charge stays fixed and the budget rises by 10 each fragment, so the edge
   weights never change, a fragment is the same Dijkstra run continued, and fragment k adds one
   ply to every line of best moves (12 + k plies). Disjoint, nested, and the book stops
   growing when the operator stops it: the run log's sizes show the growth per fragment.
6. **Parallel and exact.** The band argument above, and why reproducibility holds: one thread
   per search, a fresh table per position, a fixed depth, the same sequence of searches.
7. **The book a bot plays.** The softmax over each position's move scores, T = 20 cp, and what
   it means: a move 46 cp worse is played ten times less often; a mistake is never in the book,
   an inaccuracy rarely played.
8. **What it does not do.** No human statistics, no frequencies; no positions beyond the cost
   budget; the engine's opinion at depth 20, no more.
