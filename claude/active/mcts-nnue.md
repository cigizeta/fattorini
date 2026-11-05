# Combine MCTS with NNUE: psq_threats_mcts design

This document is for psq_threats_mcts only (see `architectures.md`).
The transformer track is not covered here.

## Goal

Find out whether psq_threats_mcts, searched with MCTS, beats
psq_threats, searched with alpha-beta, at equal time.

## Network

psq_threats plus a policy head of 1858 outputs, one logit per move
index, next to the value output on top of the same last hidden
layer. Its shape may be the same as psq_threats_move's; the loss
differs: the policy head is trained against a distribution over
moves, not against one best move.

## Search

PUCT with certainty propagation (proven wins and losses backed up
as exact minimax values, as Lc0 does), as already decided for the
transformer track: start from the standard solution, and invent
only once it works.

## Training data

Self-play and replay, as for the other networks, but labelled by
MCTS with a fixed budget instead of search(9): the value label from
the root's MCTS value, the policy label from the root's visit
distribution.

### Generation 1

There is no policy network yet, so the first teacher is alpha-beta
with pesto, with MultiPV(3):

- **Value label:** the first PV's score, as search(9) gives now.
- **Policy label:** the three PV moves take most of the mass, in
  their order; the rest of the legal moves share the remainder
  uniformly.

From generation 2 on, the teacher is MCTS with the trained network.

### Lc0's counts as generation 1's policy teacher (the developer, 2026-10-28)

Lc0's V6 records hold this very target: the policy entries are raw visit shares, exact multiples of
`1 / (visits - 1)` in all 7,910,812 records of the two test tars (lc0-second-move.md), so share
times `visits - 1` gives Lc0's counts losslessly. Generation 1 can learn its policy from Lc0's
search instead of alpha-beta's three PV moves. **Decided:** after `-quiet`'s duels
(quiet-filter.md), Lc0's 275 tars are downloaded and converted again, about 16 hours unattended
(two connections, about 7 minutes a tar), each comment carrying the counts in the format below,
and the raw tars kept, so it is done once. Storage is no longer the constraint (2026-10-28).

### The policy target's format (the developer, 2026-10-14)

- **A complete policy:** the root's raw visit count of every legal move, lossless. Raw counts, not
  shares: the target can be reshaped later (a temperature, a weight by total visits) by retraining,
  without labelling again; the total says how confident the root was; and nothing is truncated, so
  the network is not more confident than the search. Lc0 stores every move too (1858 floats a
  position in its V6 records, -1 for an illegal move).
- **Only the counts, no move names** (the first space optimization): under PUCT with root
  Dirichlet noise and a few hundred visits nearly every legal move is visited, so the names would
  repeat what the position already says. The counts follow the legal moves **sorted by their UCI
  text**, an order independent of Rotor's move generator, so a change to it never reorders an
  archived file. Their number must equal the legal moves', a free check of record against position.
- **In the same PGN**, the comment extended as the best move extended it: the score (the root's
  value in pawns), the best move (the most visited), then the counts:
  `4... Nc6 {0.31 g8f6 412,120,0,3,0,61,...}`. Older comments, a score alone or with a move, still
  read.
- **Size:** about 35 small numbers a position, mostly 0 or tiny, perhaps 20 to 40 bytes compressed:
  10 to 25 GB for a corpus of generation 4's size, more than Drive. The labelling is MCTS with a
  budget of trials, not alpha-beta at depth 9 (the developer, 2026-10-14), and it is not slower:
  depth 9 costs about 19,000 nodes, about 5 ms a position on amanda (bench, 2026-10-14), while 800
  trials at perhaps 1 to 2 us each are 1 to 2 ms. So the corpus is as large as `corpus.yaml`'s
  windows make it. **The corpus stays on Drive, on a paid tier from MCTS on** (the developer,
  2026-10-14): Google keeps the copies, which a local disk cannot guarantee, and every machine, off the LAN
  included, keeps reaching the same folder. A LAN share was weighed and dropped: a machine off the
  LAN cannot reach it, and one disk would hold the only copy. Prices
  from the second year on: 100 GB 20 euros a year, 200 GB 30 euros a year. Not needed before MCTS:
  generation 4's corpus (2 to 4.5 GB) fits in the free 15 GB.
- Generation 1's alpha-beta MultiPV(3) gives no visit counts: whether it is kept, or MCTS labels
  from the first generation, is open (below).

## Open

1. **The mass split** of the policy label in generation 1. Not
   trivial, but not fundamental to the idea: decided later.
2. **The depth of MultiPV(3)**, which costs more than a single PV
   at the same depth.
3. **Positions with fewer than three legal moves.**
4. **The MCTS budget**, in nodes, from generation 2 on.
5. **The value head**: the scalar in pawns through `score`, as the
   other networks, or WDL.
6. **Self-play exploration** under MCTS: AlphaZero's root Dirichlet
   noise and visit-proportional move sampling, or the mechanism of
   `rotor-self-play-diversity.md`.
7. **Generation 1's teacher** now that the policy target is complete counts: alpha-beta's
   MultiPV(3), whose three moves give no counts, or MCTS from the first generation.
8. **Further space optimizations** of the counts, after the first (counts only, no names).
