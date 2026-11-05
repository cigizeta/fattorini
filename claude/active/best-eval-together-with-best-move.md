# NNUE predicts the best move too, not only the eval

## Goal

The NNUE returns two things: the eval in pawns (ceiling 32, lc0-label-scale.md), as now, and
a predicted best move. The search tries the TT move first, then
the NNUE's predicted move, then the rest in the usual order.
The predicted move is a move-ordering hint only; it never
changes the eval.

## Move encoding

Moves are encoded from the side to move's point of view: the
board is flipped for Black, as the NNUE inputs already are, so
one encoding serves both colours.

- **From-to moves.** Every move the position can allow goes
  from a starting square to a landing square, along a line
  that a queen or a knight can follow: an amazon's reach.
  Summed over the 64 starting squares, the queen part gives
  1456 pairs and the knight part 336, so 1792 indices. This
  covers king moves, pawn pushes and captures, and castling,
  written as the king moving two squares.
- **Promotion to a queen.** This is the plain from-to index
  of the pawn's move, with no extra index.
- **Underpromotions.** A pawn on the 7th rank has 22 from-to
  pairs: 8 straight pushes and 14 diagonal captures. Each can
  promote to a knight, a bishop or a rook, which gives
  22 * 3 = 66 extra indices.

So N = 1792 + 66 = **1858**, and the NNUE has 1 + 1858
outputs.

## Network

The move head is a linear layer with 1858 outputs, one logit
per move index. It sits next to the eval output, on top of
the same last hidden layer, so both heads share the
accumulator and the hidden layers.

## Training

- **Data.** Search(9) already finds the best move. Each
  training position stores it next to the eval, as a move
  index.
- **Move loss.** Softmax over the logits of the position's
  legal moves only, all other indices masked out, then
  cross-entropy against the best move's index. This is a
  classifier: it rewards only putting the best move on top,
  and does not try to match a distribution over moves.
- **Regularization.** No separate loss term. The trainer's
  optimizer is AdamW (hand-rolled in `src/train/adamw.rs`),
  whose decoupled weight decay, 0.01, already shrinks every
  parameter at each step. The move head gets it simply by
  being registered with the optimizer like the other layers.
  That matters more for the move head than elsewhere: with
  1858 * 16 weights it has far more parameters than the rest
  of the network after the accumulator, and many move indices
  are rarely or never the best move in the data, for example
  most underpromotions, so their weights get almost no
  gradient and only the decay keeps them small. If their
  logits still grow too large, the fix is a stronger decay
  for the move head only.
- **Total loss.** The eval loss as now, plus the move loss
  times a weight.
- ~~Gradient norm clipping~~ **Dropped** (the developer,
  2026-10-14): the cross-entropy's loss is unbounded, but its
  gradient on the logits, `p - onehot`, is bounded like the
  eval's `score(y) - t`, and no other network clips. The move
  loss and the eval loss are logged apart instead, with the
  move head's top-1 accuracy on the test split.

## Inference

At a node that orders moves, the engine computes logits only
for the legal moves: about 35 dot products of size 16
instead of 1858. It picks the one with the highest logit.
Taking the maximum over the legal moves only means the
predicted move is always legal. The move head is not
evaluated in quiescence search, or at nodes that return
before ordering moves.

The eval is computed eagerly when the network is called, and
the move scores are computed lazily, only when the node actually
gets to move ordering.

The network is still called at most once per node. The call
runs the forward pass up to the last hidden layer's 16
activations, computes the eval from them, and stores the 16
activations with the node. The eval is used first, for
pruning; if the node is pruned, the move scores are never
computed. Next comes the TT move; if it causes a beta cutoff,
the move scores are never computed either. Only then does
move ordering read the stored activations and score the legal
moves, without calling the network again: about 35 * 16
multiply-adds, under a tenth of the 512 * 16 of the hidden
layer.

The one new cost is at nodes that today never call the
network, such as nodes in check, where pruning is off and no
static eval is computed. If such a node reaches move
ordering, it calls the network once, just for the move
scores.

## Brainstorm, 2026-10-14 (the developer and Claude)

Not decided; the design above is to be revised from it.

- **The hidden layer of 16 is not settled for the value.** If amanda keeps beating betty and
  caroline dorothy, the value has no hidden layer, and the design above, a move head on the
  value's 16 activations, loses its footing. Even where the 16 exist, they are a bottleneck shaped
  for the value: they make the move head cheap, not good.
- **Candidate: the hidden layer only on the move.** Caroline's value unchanged, the move head a
  branch of its own: `accumulator (512) -> 16 -> 1858`. It sees all 512 accumulator values, its 16
  are shaped by the move loss alone, and it costs about 512 * 16 + 35 * 16 multiply-adds only at
  the nodes that reach move ordering (never at pruned nodes or TT-move cutoffs). Whether that is
  cheap enough is measured first: node counts and time per node at depth 9, before any duel.
  Whether its first layer is `i8` like the value's goes with the trainer and engine design.
- **Cost estimate** (from psq-threats.md's step 4, the training machine, random weights, before the inference
  speed work): the 512 * 16 layer is about 410 ns of dorothy's 537 ns an evaluated node, so
  caroline's evaluation is about 130 ns, and a search node about 1.1 to 1.2 us at depth 9. The move
  branch is about 430 ns (that layer, plus about 35 * 16 for the legal moves), paid only at the
  share s of nodes that reach move ordering after the TT move. With s from 10% to 30%: emily
  (caroline plus a move branch) about +5% to +20% a node, fanny (dorothy plus its own move branch)
  about +4% to +12%; a fanny reading dorothy's own 16 would cost a few ns. It pays only if the
  better ordering cuts the nodes by more.
- **First step, before any training: measure s.** In a depth-9 search on the bench positions,
  count the nodes that reach move ordering after the TT move (not quiescence, not pruned, no
  TT-move cutoff), against all nodes. It needs no network, and turns the cost range into one
  number. **Measured 2026-10-14** (the Lichess machine, one thread, `nnue_bench --features node-stats
  --search-only`, the counter `ordered_nodes` in `node_stats.rs`): amanda-0002 s = 20.8% of nodes
  (29.4% of interior ones), 267 ns a node; betty-0002 s = 17.9% (29.3%), 355 ns a node. Betty's 88 ns
  more a node at 0.75 evaluations a node put the 512 * 16 layer at about 120 ns, far below the old
  410 ns. So the move branch costs about 0.208 * 135 = 28 ns a node, **about +10% for emily**, about
  +7% for fanny with a branch of its own, about +1% for a fanny reading dorothy's 16. Measured on
  amanda and betty, not caroline and dorothy; runs of under half a second, so about +/-10%.
- **The room, measured 2026-10-14** (same bench, amanda-0002): of the main search's beta cutoffs,
  25.6% come from the TT move, 62.3% from the first move after it, 12.1% from a later one (on
  average the 3.3rd after the TT move). The nodes searched in vain before those later cutoffs,
  each counted once, are **21.7% of the tree**: what a perfect move hint would save, about twice
  the move branch's cost. A head right half the time there would about break even. Ideas to tilt
  it: compute the hint only above some remaining depth, where the wasted subtrees are large and
  the ordered nodes few; try it after the good captures, as killers are, so it never displaces a
  winning capture's first-move cutoff. Both measurable by splitting the same counters by remaining
  depth and by capture or quiet.
- **By remaining depth, measured 2026-10-14** (same bench; the developer's idea: the value at every
  node, the move hint only some of the times). The hint computed only at nodes with at least d
  plies left, as shares of all nodes; its cost a node is about half the ordered share (about 135 ns
  where it runs, against amanda's 267 ns a node):

  | d | ordered nodes | waste reachable |
  |---|---|---|
  | 1 | 20.8% | 21.7% |
  | 2 | 8.1% | 16.8% |
  | 3 | 3.7% | 13.6% |
  | 4 | 1.2% | 10.6% |
  | 5 | 0.6% | 8.8% |
  | 6 | 0.3% | 6.5% |
  | 7 | 0.1% | 4.2% |

  The nodes with one ply left are more than half the cost and under a quarter of the waste. **From
  d = 3 or 4 the hint costs about 0.6% to 2% a node and can reach 11% to 14% of the tree**: a head
  right half the time there saves 5% to 7% of the nodes. So the design: the value at every node as
  today, the move branch lazily only from a threshold of about 3 plies left, tuned on real games.
  49 bench positions at depth 9; the waste is a ceiling, the head's real accuracy decides.
- **By the cutting move's kind, measured 2026-10-15** (same bench and network, the counters split by
  `is_quiet`): of the first-move cutoffs after the TT move, 28.1% are by a quiet move, 71.9% by a
  capture; of the later cutoffs, 60.9% quiet. **Of the 21.7% waste, 15.4% of the tree (70.9%) comes
  before a quiet cutoff.** So a hint tried after the good captures, as killers are, keeps most of
  the room and never displaces the first-move capture cutoffs, nearly three quarters of them. 15.4%
  is a ceiling for that placement: the waste before a quiet cutoff includes any captures searched
  in vain before it, which a hint placed after the captures still pays for.
- **Break-even:** the move hint must cut the nodes by more than its cost: about 10% if computed at
  every ordered node, about 1% to 2% from d = 3 or 4. Measured on the bench
  positions before any duel: the trained move head's move tried after the TT move, the nodes
  against the same search without it.
- **Two losses, one weight `w`** (`eval + w * move`). AdamW divides each parameter's step by its own
  gradient's size, so `w` makes no difference to the parameters only the move loss reaches; it
  matters only where the two gradients add up, in the shared part (now the accumulator).
- **First experiment: the move gradient stopped at the shared part** (a detach on its input). The
  move loss trains the move branch only; the value trains exactly as today, so it cannot get worse,
  and `w` has no effect at all. The technique is standard (linear probing, a frozen body under a new
  head, detached auxiliary heads); its use for an alpha-beta move head is this project's. If the
  move branch is too weak on the value's features, joint training is the second experiment.
- **Lc0 trains jointly** (one body, policy, value and moves-left heads, no stop), with every loss
  weight 1.0 in `lczero-training`'s example config and no documented search behind it. AlphaGo
  (2016) had separate policy and value networks; AlphaGo Zero merged them into one two-headed
  network, stronger in its ablation, and scaled the value loss to 0.01 in supervised training.
  Their bodies are hundreds of channels wide; Rotor's shared part is the 512 accumulator values.
  Our value loss's gradient is small once the value is good, while a cross-entropy over about 35
  moves stays larger longer, so `w = 1` may not be an even balance here: the two gradients' sizes on
  the shared part are measured before trusting it.
- **The legal-move mask** for the softmax: computed by the loader from the cached position (side,
  pieces, kings, castling, en passant), so `prepare` stores one `u16` per position, about 1 GB for
  generation 4, against about 30 GB to store the masks (Lc0 stores them in its data, marking
  illegal moves -1). Proposed, not yet agreed.
- **A policy later** (psq_threats_mcts) would follow Lc0: a visit-distribution target, joint
  training, weights of 1.

## Details to settle later

1. The weight of the move loss, whether the move head needs
   a stronger weight decay than 0.01, and a check that the eval
   does not get worse because it now shares the hidden
   layers with the move head.
2. ~~The training data format~~ Settled 2026-10-14 (best-move-label.md): from generation 4 the
   replay and self-play PGNs carry the best move after each score, in UCI; older corpora have none.
3. The trainer, export format and engine changes: the new
   head, the masked loss, the move-index table.
4. Measuring cost and benefit: node counts and time per node
   at depth 9 in the bench, then Elo in arena.
