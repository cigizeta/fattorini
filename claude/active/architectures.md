# Architectures

| Architecture                     | Type        | Heads          | Search |
|----------------------------------|-------------|----------------|--------|
| pesto                            | hand-crafted| value          | AB     |
| psq                              | NNUE        | value          | AB     |
| psq_threats                      | NNUE        | value          | AB     |
| psq_threats_pairwise             | NNUE        | value          | AB     |
| psq_threats_screlu               | NNUE        | value          | AB     |
| psq_threats_screlu_material      | NNUE        | value          | AB     |
| psq_threats_screlu_phase         | NNUE        | value          | AB     |
| psq_threats_screlu_count         | NNUE        | value          | AB     |
| psq_threats_screlu_taper         | NNUE        | value          | AB     |
| psq_threats_screlu_kings         | NNUE        | value          | AB     |
| psq_threats_screlu_hidden        | NNUE        | value          | AB     |
| psq_threats_pairwise_hidden      | NNUE        | value          | AB     |
| psq_threats_pairwise_material    | NNUE        | value          | AB     |
| psq_threats_pairwise_stack       | NNUE        | value          | AB     |
| psq_threats_pairwise_stack_move  | NNUE        | value + move   | AB     |
| psq_threats_pairwise_stack_mcts  | NNUE        | value + policy | MCTS   |
| halfk_threats                    | NNUE        | value          | AB     |
| halfk_threats_move               | NNUE        | value + move   | AB     |
| halfk_threats_mcts               | NNUE        | value + policy | MCTS   |
| alpha_squares                    | transformer | value + policy | MCTS   |
| alpha_pieces                     | transformer | value + policy | MCTS   |

The sections below follow the table's rows.

**On psq inputs, the line ends at psq_threats_screlu (the developer, 2026-10-27, restated
2026-10-31):** neither pairwise products nor a hidden layer helps. psq_threats_screlu beat
psq_threats_pairwise directly (+38 Elo, depth 9); psq_threats_pairwise_hidden lost to it (-24 to
-55 Elo), its better test loss turning into no strength; the output buckets (material, phase,
count, taper) drew. So every row from psq_threats_screlu_material to psq_threats_pairwise_stack_mcts
is dropped or settled on psq inputs, the hidden-layer grid and kings2 with them (TODO.md). A hidden
layer, pairwise or buckets may still pay on other inputs (halfk_threats), where Stockfish's gains
come from; not tried.

## pesto

This is just the official PeSTO implementation with middlegame and
endgame tables tapered by game phase.

## The hidden layer

This applies to every NNUE but the stack, whose head is its own
(psq_threats_pairwise_stack below), and halfk_threats, which takes the
stack's depth.

Every NNUE comes in two variants: without a hidden layer (the
accumulators feed the output directly) and with a hidden layer of
16 between the accumulators and the output. The hidden layer is
expected to be stronger, because it lets the network combine
features nonlinearly after the transform, at the cost of a slower
evaluation (most of a node's evaluation cost, as measured for
psq_threats). **The evidence so far points the other way (the
developer, 2026-10-18):** amanda (no hidden layer) kept beating betty
(hidden layer) in their duels, though betty had the lower test loss,
and caroline against dorothy suggests the same for psq_threats. The
psq networks may be stronger without it, the hidden layer's quality
not repaying its cost per node. Never settled before generation 3
ended with the archive corpus, so **to verify again**, once: the
network experiments (TODO.md) train without it, then the best of them
gets one twin with the layer of 16 on the same corpus, judged by
games.

## psq

`2 * ((768 + 12) -> 256) -> 1`, or `2 * ((768 + 12) -> 256) -> 16 -> 1`
with the hidden layer: the 768 piece-square rows plus 4 castling
rights and 8 en passant files. It was formerly called `naive`.

This document assumes psq stays 780 wide, and so does every
architecture built on it, the _move line and the _mcts line alike
(the developer, 2026-10-20: psq768 for _move was debated and dropped,
the 768-against-780 duel drawn and their speed the same, so one base
keeps the code uniform). psq768, the same without the 12 rows, is
expected to tie with it (psq768.md), and a tie keeps the 780 rows, for
psq_threats_pairwise_stack_mcts: there a leaf is evaluated once with no
quiescence, and the policy head must tell positions with castling
rights from those without.

## psq_threats

This is psq extended with threats, see `psq-threats.md`:
`2 * (768 + 12 + 64 * 64) -> 2 * 256 -> 1`, or
`2 * (768 + 12 + 64 * 64) -> 2 * 256 -> 16 -> 1` with the hidden
layer. Caroline has no hidden layer, dorothy has one.

In a perspective, threat row `(i, j)` is set when its own piece on
`i` attacks an opponent piece on `j`, both squares oriented as the
psq rows (`square ^ 56` for Black). The threat rows share the
transform with the piece-square rows and are added to them before
the activation, so that one neuron can learn a conjunction of a
piece and a threat.

## psq_threats_pairwise

psq_threats with each perspective's clipped accumulator split in two
halves multiplied element-wise, so the head reads `256` products, half
from each perspective: every product is a learned "group A present AND
group B present" (psq-threats-pairwise.md, pairwise-products.md).

## psq_threats_screlu

psq_threats, no hidden layer, with each perspective's clipped
accumulator squared lane by lane, `clamp(x, 0, 1)^2`, so the head reads
the same `2 * 256` values with a quadratic term in each (screlu.md).
Its integer form is pairwise's arithmetic with a lane times itself.
Implemented 2026-10-24; refused with psq rows, a hidden layer, pairwise
or a stack. Won its duels against psq_threats by a little (2026-10-24:
52.0% of 940 games at depth 9, 51.6% of 932 at 10+0.1), so it is the
line, and extended by material sets below.

## psq_threats_screlu_material

psq_threats_screlu with its one output replaced by 78 weight sets, one
per material on the board (1/3/3/5/9, both sides), each reading the
`2 * 256` squares: psq_threats_pairwise_material's head moved onto
SCReLU (psq-threats-screlu-material.md). Trained factorised, a shared set
plus each one's delta, summed at export; the engine reads one set in
place of another, at no measurable cost. Implemented 2026-10-24, file
tag 13. Its test loss beat SCReLU's by about 1.3%, but its duels were
drawn (48.3% at depth 9, 49.8% at 10+0.1).

## psq_threats_screlu_phase

psq_threats_screlu_material with 25 weight sets instead of 78, by
PeSTO's game phase (knight and bishop 1, rook 2, queen 4, both sides,
capped at 24), pawns not counted: fewer buckets, each with more data
(psq-threats-screlu-phase.md). Trained factorised the same way.
Implemented 2026-10-24, file tag 14.

## psq_threats_screlu_count

psq_threats_screlu_phase with 8 weight sets instead of 25, by
Stockfish's key: `(count - 1) / 4` over every piece on the board, kings
and pawns included, capped at 7 (psq-threats-screlu-count.md). The key
counts the pieces where the other two weigh them. Trained factorised
the same way. Implemented 2026-10-25, file tag 15; no measurable cost
in the bench.

## psq_threats_screlu_taper

psq_threats_screlu with two weight sets, a middlegame and an endgame
one, both read at every position and blended by PeSTO's taper,
`(phase * middlegame + (24 - phase) * endgame) / 24`, the phase as
psq_threats_screlu_phase's (psq-threats-screlu-taper.md). The phase
network with its 25 sets forced onto a line between two ends: fewer
weights, every position training both, and no jump at a bucket's
edge. Not factorised. Implemented 2026-10-25, file tag 16; a fused
kernel squares each lane once for both sets, about 3% per node.

## psq_threats_screlu_kings

psq_threats_screlu with a king-sensitive hidden layer
(psq-threats-screlu-kings2.md, 2026-10-26): each side's 256 squares
through 16 units picked by its own king's square (their king in their
own frame), each unit clipped and squared beside its value, then one
output over those 64 values and every square, so screlu's own output
stays as a linear skip. The sides' stages factorised, the output plain.
File tag 17, reused. Bench: head 2.2x screlu's, about 2.0x a node.

Its first form (psq-threats-screlu-kings.md, archived), both
perspectives' 512 squares through 16 by our king then 16 by their king,
no skip, was dropped at round 10 of generation 1: 7.7% above screlu in
test loss, far ahead in endgames, behind from 8 pieces up, the 16 clipped
units likely a bottleneck for middlegames.

## psq_threats_screlu_hidden

psq_threats_screlu_kings without the king (psq-threats-screlu-hidden.md,
2026-10-26): all 512 squares through one layer of 16, each unit clipped
and squared beside its value, then one output over those 32 values and
every square, screlu's own output kept as a linear skip. The layer's
input detached in training, so the transform learns from the skip alone.
File tag 18. Bench: about 1.7x screlu a node.

## psq_threats_pairwise_hidden

psq_threats_screlu_hidden with pairwise in place of screlu
(psq-threats-pairwise-hidden.md, 2026-10-26): the 256 products through
one layer of 16, each unit clipped and squared beside its value, then
one output over those 32 values and every product, pairwise's own output
kept as a skip; the layer's input detached in training. Where pairwise
can pay, as in Stockfish: the hidden layer reads half the inputs. File
tag 19, not tag 8 with a hidden layer, whose output reads the hidden
layer alone. Bench: about 1.4x pairwise a node, 15% cheaper than
screlu_hidden.

## psq_threats_pairwise_material

psq_threats_pairwise with its output layer one of 78 weight sets, by
the material (1/3/3/5/9, pawns included, 1 to 78), and no king
buckets: TODO.md's `psq_pairwise_78`, over psq_threats' inputs. It
isolates the material buckets, one of the four things
psq_threats_pairwise_stack changes at once. Implemented 2026-10-20
(psq-threats-pairwise-material.md), factorised as the stack's output.

## psq_threats_pairwise_stack

psq_threats_pairwise's products into a head of two hidden layers and
the output, each layer one of several weight sets: the first by our
king's square (64), the second by their king's (64), the output by the
material (78). `(128 + 128) x 2 -> 256 -> 16 -> 16 -> 1`, trained
factorised, each set a shared part plus its own delta
(psq-threats-stack.md, psq-stack.md). Its kind in the code and
configs is `psq_threats_stack`.

**Its question is the stack against halfk_threats under a limited data
budget (the developer, 2026-10-18).** Both bring the king's square in:
the stack in the head, factorised so rare buckets learn from the rest,
halfk_threats in the input, each king square's rows seeing about 1/64
of the data. psq_threats_pairwise_material is expected to improve on
what comes before it; whether the stack succeeds is open.

**The stack was devised to be the faster of the two.** Its king
buckets are a lookup in the head, and its transform stays psq's, small
enough to stay in cache and never rebuilt when a king moves; halfk's
is 23 MB at width 256, and a king move rebuilds its perspective. Both
carry threats, so the comparison is fair only with the same head
depth on both sides: the stack's `256 -> 16 -> 16 -> 1` against
halfk_threats with the same head, material buckets only. The speed
benchmark (`nnue_bench`) needs halfk inference in the engine first.

## psq_threats_pairwise_stack_move

This is psq_threats_pairwise_stack with a move head added: one predicted best
move, not a distribution, searched with alpha-beta, see
`best-eval-together-with-best-move.md`. The move head is a linear
layer with 1858 outputs, one logit per move index, next to the
value output on top of the same last hidden layer. It is trained
as a classifier: masked softmax over the legal moves,
cross-entropy against the teacher's best move.

The predicted move is a move-ordering hint only, tried right after
the TT move, and never changes the eval. The engine scores only the
legal moves, lazily, from the stored hidden activations, so pruned
nodes and TT-move cutoffs never pay for it.

It is expected to outperform psq_threats_pairwise_stack, but this
needs to be verified: the better move ordering must repay the move
head's cost per node, and sharing the last hidden layer with the move
head must not make the eval worse. The duel is
psq_threats_pairwise_stack_move against psq_threats_pairwise_stack,
trained on the same data.

The stack's last hidden layer of 16 keeps the move head cheap: on the
accumulators directly it would be 1858 * 512 weights, and scoring the
legal moves would cost about 35 * 512 multiply-adds.

## psq_threats_pairwise_stack_mcts

This is psq_threats_pairwise_stack with a policy head added: a
distribution over moves that gives the search its priors. It is
searched with MCTS, as halfk_threats_mcts is, see `mcts-nnue.md`.

Its policy head is larger than psq_threats_pairwise_stack_move's move
head (see the ruling below), and its loss differs. The policy head is trained against a distribution over
moves, such as MCTS visit counts, not against one best move,
because MCTS needs calibrated probabilities to spread its visits.

**Ruled (the developer, 2026-10-20): the _mcts network cannot serve as
the _move one; two networks, trained apart.** Not for the loss's form
(both are a cross-entropy over moves, and a distribution's ranking
would order moves well), but for the search each serves:

- alpha-beta reads its move head at nearly every interior node, so the
  head must cost about what the value head costs (e.g. from-square
  plus to-square scores, 64 + 64 outputs), while MCTS reads its policy
  once per expansion and can afford, and needs, a far larger one;
- MCTS reads the value at any leaf with no quiescence, so its value
  must judge tactical positions itself, where alpha-beta's is read
  after quiescence, at quiet ones.

What may be shared is a start, not the training: a value-trained
transform can initialise either. An _mcts policy could also teach the
_move head (its distribution as the target), should both exist.
Rotor's Lc0 corpus does not keep each position's visit distribution,
the policy's target: a policy means downloading and converting the
games again, with their distributions (the developer, 2026-10-20).

## halfk_threats

halfk means halfkav2_hm, the most recent and standard HalfK version,
the input of strong engines (halfkp is the older one; both are in the
code). Parked (the developer, 2026-10-12): it is what every other
engine does, so it would contribute nothing of Rotor's own. Lc0's
corpus lifted the other reason, a dataset too small for its 45k rows
a perspective (2026-10-15), so on the same labels it is the reference
Rotor's own shapes must beat.

**Kept, uneasily (the developer, 2026-10-20):** halfk feels like
copying, and king-awareness is first the stack's job, Rotor's own
answer; halfk stays in the plan as the outside reference, "you never
know the future", its input Stockfish's exactly (a reference is only
one if standard), with no commitment yet to ship it as a champion.

halfk_threats is halfk's input plus psq_threats' threat rows, and
**embeds what the psq experiments prove (the developer, 2026-10-18):**
the pairwise products and the material buckets of the head, if
psq_threats_pairwise and psq_threats_pairwise_material win on the
simpler psq inputs. Its king buckets are the input's own, so the
stack's head buckets by king are not needed there. Open: whether its
threat rows are bucketed by king too, or shared by every king square
as psq_threats' are.

**halfk_threats_screlu, the halfk line's first step (2026-11-02,
halfk-threats-screlu.md):** halfka_v2_hm exactly (32 king buckets,
mirrored, 22,528 rows), psq_threats' threat rows shared by every bucket
and mirrored with the king, SCReLU, no hidden layer, width 256; the
piece rows trained factorised, folded at export. The old pipeline's
halfkp and halfka_v2_hm code is gone (overwritten the same day).

**halfk_threats_screlu_hidden, the halfk line's second step (2026-11-02,
halfk-threats-screlu-hidden.md):** halfk_threats_screlu with its output
replaced by Reckless's head, 512 -> 16 -> 32 -> 1, each hidden layer
clipped to [0, 1], no skip, trained end to end from scratch. The first
layer in int16 or int8, chosen on speed and on the trained network's
gap to f32; no weight bound in training, the engine quantizing each
row on its own scale.

**halfk_threats_pairwise_hidden, the halfk line's third step (2026-11-02,
halfk-threats-pairwise-hidden.md):** halfk_threats_screlu_hidden with
each perspective's halves multiplied in place of the squares, width 256
kept, so the first hidden layer reads 256 products instead of 512
squares. Trained only if step 2 wins or draws.

### halfk_threats_move and halfk_threats_mcts

halfk_threats with a move head, searched with alpha-beta, and with a
policy head, searched with MCTS: as psq_threats_pairwise_stack_move and
psq_threats_pairwise_stack_mcts are to psq_threats_pairwise_stack,
with the same heads and losses.

## The transformers

This applies to alpha_squares and alpha_pieces.

A transformer is searched only with MCTS: it is far too slow per
evaluation for alpha-beta. So, besides the value head, it has a
policy head, trained against a distribution over moves as
psq_threats_pairwise_stack_mcts's.

### alpha_squares

A transformer in which squares attend each other, inspired by Lc0.
The name evokes AlphaZero. It was formerly called
`transformer_square_centric`.

### alpha_pieces

A transformer in which pieces attend each other. It was formerly
called `transformer_piece_centric`. It is the proposed alternative
to alpha_squares: the input length depends on the number of pieces
on the board, so it is potentially more friendly to the quadratic
computational cost induced by attention.

**Brainstorm, 2026-10-15 (the developer and Claude).**

- **A GPU engine, not a CCRL one.** CCRL tolerates GPU engines
  rather than serves them: Lc0's newest entries are about 2022's,
  on CPU in 40/40 (3443 at best) and on an RTX 2080 in Blitz (3697).
  alpha_pieces is for Lichess on a consumer GPU and, one
  day, TCEC, which provides GPUs; the NNUE with alpha-beta stays the
  CCRL line.
- **33 tokens beat 64 even padded:** a layer costs about
  `L x 12 x d^2` multiply-adds, so pieces (at most 32 plus `[CLS]`)
  cost about half of Lc0's squares at any width, attention itself
  almost a quarter.
- **Pure piece attention to begin with:** no empty-square tokens
  and no attack features. The tokens' squares already say which
  squares are empty; blocking is a relation among three pieces, so
  it needs at least two layers, as it does for Lc0's squares.
  Attack features only if training shows trouble with lines.
- **Small first, as a feasibility test:** 33 tokens, width 64,
  4 layers, about 6.5M multiply-adds an evaluation (about 3,000
  evaluations a second on one CPU core). If it fails, investigate,
  then perhaps medium (width 256, 10 layers, about 260M, an estimated
  20,000 to 60,000 a second on a consumer GPU).
- **The first test is the value alone:** the small network's test loss
  against the NNUEs' on the same frozen test sets, as experiment 5,
  `lc0-alpha-pieces` and `self-alpha-pieces` (transformer-streaming.md;
  first planned as zelda on amanda's dataset, never run). Reaching them
  says piece attention learns chess at least as well as an NNUE, per
  position. The policy waits for move targets.

## Output units and training loss

This applies to the value head of every learned architecture. The
move and policy heads have their own losses, given in their
sections.

Every network outputs its evaluation `y` in pawns, so that all of
them share one loss, one engine conversion, and one unit. The engine
clamps `y` to `±CEILING_PAWNS` (32 pawns, `labels.rs`), the same
bound `clamped_pawns_from_score` gave before, and plays `100 * y`
centipawns.

This replaces the earlier scheme, where the old `naive` networks
output label space directly and were trained with MSE against the
label. The networks trained that way are retired, not migrated:
nothing needs to load them any more.

Training uses the matching loss of the label scale: the labels
stay in `[-1, 1]` (AB(lambda) blending included), the prediction
is `score(y)`, and the gradient on `y` is `score(y) - t`, where `t`
is the position's label in `[-1, 1]`, with no
backpropagation through `score`. `score` is monotone, so this is
the gradient of a convex loss whose minimum is `score(y) = t`,
and the gradient is bounded, so decisive positions cannot dominate
the way they would under MSE in pawns.
