# Lc0's book -- Design

Status: designing (2026-11-26); no plan yet. A second book beside Stockfish's (book-expansion.md):
the same graph, Dijkstra, fragments and Polyglot conversion, with Lc0 as the engine and one search
per expanded position. Built on the machine with a GPU, never beside a training.

## The idea (the developer, 2026-11-26)

- **One search per position.** Lc0 is MCTS with PUCT: a single search of a position, with a
  generous node budget, values every root move at once. Stockfish's book needs a `searchmoves`
  search per admitted move (about 17 a position); Lc0's needs one.
- **Lc0's own expected score.** Each move's score is read from Lc0's Q, so the book is Lc0's in
  Lc0's calibration, as Stockfish's is Stockfish's (book-expansion.md, "Units and thresholds").

## Reading the moves

- **`VerboseMoveStats=true`:** after the search, Lc0 prints an `info string` line per legal root
  move, with its visits (N), its prior (P), its win minus loss (WL), its draw probability (D) and
  its value (Q), from the side to move at the root.
- **A move's score** is `(1 + WL) / 2 * 100` points, 0 to 100, from the mover's side: W + D/2,
  read directly. No centipawns: Lc0's cp is only a display mapping of Q (`90 tan(1.5637 q)`), and
  a round trip through integer cp only loses precision.
- **The position's evaluation** is its best move's score, the one the search played.
- **Its loss** is the best score minus its own, as in Stockfish's book.

## The catch: visits go to the best moves (accepted, 2026-11-26)

PUCT spends most visits on the top two or three moves; a poor move may end with a handful, its Q
mostly the network's first impression. Accepted (the developer): the book wants good moves, and a
noisy "much worse" stays worse. The moves that matter, near the threshold, are the ones most
visited after the best. A light visit floor keeps out the starved moves (open question 2).

A child's Q is its subtree's average, not a minimax value: it lags behind a deeper line, but alike
for every sibling, so the ranking within a position barely changes.

## Settings, every search

- **A fixed node budget** (`go nodes N`), never time: the same tree on any load. N chosen from the
  probes.
- **`SmartPruningFactor=0`:** the search does not stop once the best move is settled, so the
  alternatives get the budget's visits too.
- **No noise, no temperature, no contempt, no WDL rescaling:** Lc0's defaults for a match, set
  explicitly so a later default cannot change the book.
- **A fresh tree per position** (`ucinewgame`): no reuse of a parent's subtree, so a position's
  scores do not depend on the order of expansion.
- **The network: a very strong one, as Lc0 ships it (the developer, 2026-11-26):** the network
  that comes with Lc0 installed by winget or from its official repository, not test91's: the book
  wants the best moves, and nothing shows that a book agreeing with the training data's teacher
  wins games (the duels start from the UHO book anyway). Pinned by its file name and hash in
  `data/engines.yaml`, fixed for the whole book; a new network is a new book.

## What stays from Stockfish's book

Unchanged (book-expansion.md): the graph, Dijkstra with the charge 10 and a loss weight, fragments
at a rising budget, resuming from the fragments, a move into a forced mate never admitted (Q at
-1 with certainty), decided positions never expanded, the Polyglot softmax, the files under
`data/books/`, one per engine.

To measure again, in Lc0's unit: Lc0's scale is not Stockfish's, so the threshold (7 points), the
decided cutoff (+/-300 cp, to be restated in points) and the loss weight (4) are chosen afresh by
probes on book-expansion.md's critical positions and `book-simulate`, not copied.

## Throughput over reproducibility (the developer, 2026-11-26)

The CPU, and above all the GPU, are kept busy: no setting that idles them for the sake of a
repeatable search. Lc0's GPU search is parallel in three ways: each search thread gathers a
minibatch of leaves under virtual loss, several threads overlap their gathering with the GPU's
work, and the GPU's half-precision kernels may differ in the last bits. Thread timing makes trees
differ at the same node count; accepted, since a fragment is built once and kept (a resume replays
the fragments, never searching an expanded position again), so only a rebuild from scratch or a
repeated probe would see it.

**Several positions at once, not one wide search:** a batch of the cheapest positions of the queue
expanded together, as Stockfish's book does one engine per core, here one Lc0 process per
position, the processes sharing the GPU. Each tree then needs less parallelism of its own, so
less virtual loss: better trees for the same nodes, the GPU as busy. A batch never changes which
positions a fragment holds. The number of processes, and each one's `Threads` and
`MinibatchSize`, are chosen by a probe: the most nodes a second, the GPU's utilisation near full,
its memory within bounds.

## The code: the same commands (the developer, 2026-11-26)

`book-fragment lc0`, `book-simulate lc0`, `book-polyglot lc0`: one pipeline. The engine's kind in
`data/engines.yaml` chooses how a position is expanded (Stockfish: a `searchmoves` search per
admitted move; Lc0: one search, every move read from `VerboseMoveStats`). The queue, fragments,
resume, simulation and Polyglot conversion are shared, unchanged.

## Open questions

All three need probes on the machine with a GPU, once it is free of training.

1. **The node budget: about a second's worth (the developer, 2026-11-26),** stated in nodes, not
   time: the probe measures nodes a second per process at the chosen number of processes, and N
   is fixed from it. Checked on the critical positions: the threshold's moves' scores should
   have stopped moving.
2. **A light visit floor, no second search (the developer, 2026-11-26): simpler first.** Lc0
   imposes none that we know of: a root move is visited only as its prior and the exploration
   term earn it, and an unvisited one reports a placeholder value. The rule: a move with fewer
   than 50 visits is not admitted, whatever its Q. PUCT starving a move is itself Lc0's verdict
   that it is not among the best, and the book wants good moves. One search a position, always.
   50 is a starting value, light on purpose; the probe checks that no move worth admitting falls
   below it, and only then is it changed.
3. **The processes the memory holds:** each loads the network and its own CUDA context, very
   roughly 0.5 to 1 GB (an estimate), so several should fit; the probe adds processes one at a
   time, watching `nvidia-smi`. Never beside a training, which holds much of the memory.
