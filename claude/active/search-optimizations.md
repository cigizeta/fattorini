# Search optimizations: the candidate list

Drawn up with the developer on 2026-10-06, after Lichess showed Rotor at about 1M nodes/s with
4 threads on the Lichess machine (about 250k per thread), against Stockfish's 1 to 2M per thread with a far
larger network. A list to choose from, not a plan: each item picked gets its own design discussion
and plan doc, which this list then points to.

**2026-10-12:** measured on one thread, the Lichess machine plugged in and idle, Rotor (amanda-0002) does
3.32M nodes/s against Stockfish 19's 2.03M, about 1.6x (TODO.md, Search; node-cost.md). The 250k per thread
above came from Lichess under load on an older build and overstated the gap.

## Already in Rotor

Taken from `search.rs`'s constants, functions and comments, not a full read: aspiration windows; TT
with mate distance pruning; the staged move picker (move-picker.md); killers; history; 1-ply
continuation history; internal iterative reduction; razoring; reverse futility pruning; null-move
pruning (fixed `NULL_MOVE_REDUCTION` of 2); futility pruning; late-move pruning; LMR; history
pruning; SEE pruning; check extension; quiescence with delta and SEE pruning and `noisy_moves`
(quiescence-noisy-generator.md); the "improving" signal; Lazy SMP.

## A. Nodes per second: the same tree, faster

These keep the search tree, so `search_bench`'s node counts must come out identical: that is the
check that they stay off the label path.

| # | Optimization | Note |
|---|---|---|
| A1 | Legality before `make_move`: pinned pieces once per node, a real test only for king moves, pinned pieces and en passant | Today every move tried pays `make_move`, `is_in_check` and `unmake_move`, illegal and pruned ones included. The largest structural candidate. In TODO.md. Plan: legality-before-make-move.md |
| A2 | Pruning decided before `make_move` where it needs nothing from the new position | Futility and late-move pruning today decide after it. Same tree only if the decisions stay the same; overlaps A1 |
| A3 | TT prefetch of the child's entry once the move is known | Small, independent. Plan: tt-prefetch.md |
| A4 | TT layout | Checked 2026-10-07: 16-byte slots, four to a cache line, nothing to fix for speed. Buckets moved to B10 |
| A5 | Early-exit `see_ge` in place of a full SEE | Derived from Rotor's own `see`, not Stockfish's form (its king rule differs). Plan: see-ge-early-exit.md |
| A6 | Faster pawn move generation | 4.3% in the last profile |
| A7 | int16 accumulator, int8 head | Small for amanda (256 per side, no hidden layer); more for betty and emily. In TODO.md |

The last profile (2026-10-03, on the big five's bench) was flat: move generation about 11%,
make/unmake and legality about 9%, TT about 4%, the search functions themselves about 13%. A gap of
4 to 8 times from many small costs points at choices every node pays, A1 first. Re-profile on a psq
network, on idle the Lichess machine with one thread, before choosing.

## B. Fewer nodes for the same strength: selectivity

These change the tree, so they are on the label path: none before caroline's replay and
self-play end, and each judged by Elo in arena, not by nodes/s.

| # | Optimization | Note |
|---|---|---|
| B1 | Null-move reduction growing with depth and with static eval minus beta | Done 2026-10-07, +71.9 +/- 33.5 Elo at 10+0.1 (null-move-reduction.md) |
| B2 | Singular extensions | Shelved 2026-10-07: +20.1 +/- 23.6 Elo after 450 games, not confirmable by the duel rule (singular-extensions.md) |
| B3 | ProbCut | |
| B4 | Capture history, for ordering and pruning captures | Shelved 2026-10-07: no gain shown at 10+0.1 (capture-history.md) |
| B5 | 2-ply and 4-ply continuation history | Rotor has 1-ply |
| B6 | Correction history: the static eval corrected by past search results | Shelved 2026-10-07: pawn structure alone, no gain shown at 10+0.1, per `go` or per game (correction-history.md) |
| B7 | LMR adjustments: less for good history, PV nodes and improving; more for cut nodes | |
| B8 | TT cutoffs and stores in quiescence | Check what quiescence does today |
| B9 | SPSA tuning of margins and reductions | Needs many games; last |
| B10 | TT buckets: several entries per index, the least valuable (shallow or old) replaced | Today one slot per index, always overwritten by a colliding position; Stockfish keeps three per 32 bytes |

## Suggested order

A1 first (nodes/s, off the label path, can go ahead now); then B1 and B6 (cheap, usually worth
much) once the label path thaws; B2 after them.
