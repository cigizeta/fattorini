# Book expansion -- Design

Status: designed (2026-11-26); no plan yet. Supersedes uci-book.md's book (the Lichess openings
evaluated); keeps its engine, its evaluations from White's side, its Polyglot softmax (T = 20 cp)
and its files in `data/books/`. Starts from aio64's half-design (book-expansion-v1.md in aio64),
reworked into a Dijkstra search of the game graph. To be described in "Inside Fattorini" once
built (the developer, 2026-11-26).

## The model (the developer, 2026-11-26)

- **The graph:** positions are nodes, legal moves are edges, from the initial position.
- **Expanding a position** gives its outgoing edges, with searches of that position alone, at
  depth 20, one thread, MultiPV 1 always, by a UCI engine of `data/engines.yaml`:
  1. a search from a fresh table (`ucinewgame`): the position's evaluation, its best move and
     the best score;
  2. again with `go searchmoves` listing the legal moves not yet found, the table kept: the best
     of the rest and its score;
  3. on, until a move's loss reaches the threshold (it is not admitted) or no legal move is left.
  `searchmoves` is plain UCI. One thread and the same sequence keep it reproducible.
- **A move's score** is the one its search gave, from the mover's side; its **loss** is the best
  score minus its own, in the unit below.
- **The book is the set of expanded positions, each with its moves:** a position never expanded
  has no moves, so for the book it does not exist (playing into it leaves the book). The Polyglot
  weights are the softmax of each position's own move scores; the positions the moves lead to
  need no search of their own. A position with no legal move (mate or stalemate) is expanded
  into no moves and adds nothing.
- **Which positions are expanded: Dijkstra.** An edge weighs the charge, 10, plus its loss times
  a loss weight: never negative, and a best move weighs the charge alone. The weight (the
  developer, 2026-11-26) answers fragment 0's eightfold branching at weight 1: admitted
  sidelines stay in the book but cost their depth; chosen from `book-simulate`. From the initial position at cost 0, the
  position of least cost is taken from the queue (its cost is then final, the minimum over every
  path, transpositions included), expanded, and its edges offer their positions the cost plus
  the edge's weight. A run stops when the least cost in the queue exceeds its budget: every
  position left costs more.
- **No Lichess openings (the developer, 2026-11-26):** they do not fit the model and are not in
  the Polyglot book; at most aio64 uses them to name openings.

## Fragments (the developer, 2026-11-26)

- **A rising budget, the charge fixed:** fragment 0 holds the positions expanded at a cost up to
  60, and each next fragment raises the budget by 10. A line of best moves costs 10 a ply, so
  fragment k reaches 6 + k plies of best moves. The edge weights never change, so the fragments
  are one Dijkstra run cut at 60, 70, 80, ...: disjoint and nested by construction. The book is
  their union.
- **Loss weight 4, fragment 0 at 60 (the developer, 2026-11-26),** from `book-simulate` over the
  first 683 positions expanded (book-expansion-implementation.md's run log): the book grows about
  fivefold a ply at weight 4 (eight to thirteenfold at 1); weight 6 cut moves nearly as good as
  the best. At that growth fragment 0 at 120 would never finish; at 60 it takes about an hour on
  the machine that ran the first bands, and each next fragment about five times its predecessor (estimated: 70 about 6
  hours, 80 about a day, 90 about five days).
- **In order, no gaps, no maximum:** fragment k runs only after fragment k - 1; the operator stops
  when the book is big enough.
- **The format:** per expanded position, its Polyglot key, its evaluation (from White's side) and
  its moves, each with its score.
- **Resuming rebuilds the queue:** a new fragment reruns Dijkstra from the initial position,
  replaying the moves the earlier fragments hold, with no search: an expanded position's edges
  come from its fragment. The engine runs only on a position taken from the queue that no
  fragment holds. Nothing but the fragments is kept between them, beside a progress file for a
  fragment stopped midway.
- **The code is rewritten, all of it:** uci-book.md's code (the openings snapshot, the tree, the
  build, the format, the conversion) is replaced, nothing kept as is.

## Units and thresholds

- **The unit: Stockfish's own expected score (the developer, 2026-11-26),** replacing the Elo
  formula chosen the same morning: this is Stockfish's book ("not Fattorini's, not really
  another engine's, until Stockfish loses its crown"), so its scores are read in its own
  calibration. With `UCI_ShowWDL` on, each `info` line carries `wdl <win> <draw> <loss>` in
  thousandths, fitted by Stockfish to its own games (its model also weighs the material on the
  board); a score is `(win + draw / 2) / 10` points, 0 to 100, from the mover's side. An engine
  that does not send `wdl` is refused: one unit per book, no fallback. The probes before this
  change (Elo formula) showed it too lenient at equality: 16 of the 20 first moves admitted.
- **Decided positions are not expanded (the developer, 2026-11-26),** reversing "losses shrink by
  themselves, no cutoff needed": the probe of the Englund Gambit's trap (-708 cp, about 1.7
  points left to lose) admitted all 27 legal moves, a mate in one among them, each ply costing
  barely more than the charge, so decided positions would flood the book. A position whose
  evaluation is beyond +/-300 cp is never expanded: the game is decided, the book's job done.
  Known without searching it: the move leading to it was scored by its parent's search, so a
  move scored beyond +/-300 cp keeps its place among its parent's moves, but its position is
  never queued.
- **A move into a forced mate is never admitted (the developer, 2026-11-26),** unless the best
  move is itself a mate: its loss counts as infinite.
- **Admission: below the review's mistake, `?` (the developer, 2026-11-26):** a move is admitted
  unless it would be a mistake or worse in aio64's game review; inaccuracies (`?!`) are admitted,
  so the book covers the opponent's inaccuracies. The softmax keeps the damage small: at
  T = 20 cp, a move 70 cp worse weighs about 3% of the best, 110 cp worse about 0.4%.
- **The `?` threshold: 7 points (the developer, 2026-11-26),** chosen from the probes in
  Stockfish's unit (book-expansion-implementation.md's run log), not for being a round number:
  Stockfish's scale is flat near equality (nearly every game a draw) and steep beyond about
  +/-0.5. At 7, the sharp openings keep their established lines and stop just past them (Two
  Knights: Na5, b5, Nd4, not Nb4 at 9.6 nor Nxd5 at 22.9; King's Gambit: exf4, d5, Bc5, Nc6,
  not d6 at 7.3); at 5 the Fritz and the Classical Declined fell out, at 10 the King's Gambit
  took in 2... h6 and 2... g6. In the initial position 16 of 20 moves stay admitted, all draws
  in Stockfish's view; their cost (1. h4 costs 16.5 against 1. e4's 10) is what limits them.
  When aio64's review fixes its own `?`, the book follows it (the review runs Stockfish too).

## Critical positions for the threshold

Where the threshold decides the most, to see how many moves each admits and what the trimming
costs:

1. The initial position: many first moves near equality, the widest branching.
2. 1. e4 e5 2. Nf3 Nc6 3. Bb5 a6: a quiet position with many sound moves.
3. 1. e4 e5 2. Nf3 Nc6 3. Bc4 Nf6 4. Ng5 d5 5. exd5: one good reply (5... Na5), the rest worse;
   the threshold should keep few.
4. 1. e4 e5 2. f4: a move that loses points itself; how expansion beyond it fares.
5. 1. d4 e5 2. dxe5 Nc6 3. Nf3 Qe7 4. Bf4 Qb4+ 5. Bd2 Qxb2 6. Bc3 Bb4: a decided position (the
   Englund Gambit's trap), where losses in points shrink.

## For the plan

- One engine per core: a batch of the cheapest positions of the queue expanded at once, without
  changing which positions a fragment holds.
- The Polyglot conversion: the union of the fragments, the softmax over each position's move
  scores.

## Open questions

None: the design is complete. Next, the implementation plan.
