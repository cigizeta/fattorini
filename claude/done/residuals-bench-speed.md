# A faster plain search for `nnue_bench --residuals`

**Status:** done 2026-10-08, verified. On the 100,000 Lumbra positions, hand-crafted's numbers
match the slow search's to the digit at every depth, and amanda's at depths 1 to 3 (her slow run
was stopped before depth 4). Hand-crafted 2:17 against about 3:50 (ordering alone); amanda 2:34
against about 15 minutes (ordering plus incremental accumulators). With ordering off, moves sort
exactly as before (the same key, shifted, and a stable sort). To move to `done/`.

## Why

Amanda's residuals on 100,000 positions take about 13 minutes at `d` = 4 alone
(expected-score-margins.md). The plain search is slow for two reasons that change nothing it
measures:

1. Every network evaluation is from scratch: both accumulators rebuilt from every piece, about
   1 us, against about 80 ns for the engine's incremental update plus head.
2. Only captures are ordered (MVV-LVA): the best quiet move is rarely first, so cutoffs come late,
   and each depth starts from nothing.

Both fixes leave every value exactly as it is: a full-window alpha-beta returns the same value
whatever the move order, and the incremental accumulator equals a refresh exactly (integer
arithmetic, `QuantizedPsqNetwork::update`). So results before and after are comparable as they are.

## 1. Incremental accumulators

As the engine does (`Search::evaluate`, `update_accumulators`):

- The evaluator becomes `HandCrafted` or `Psq(&QuantizedPsqNetwork)`, not a closure.
- For a network, the search holds one accumulator pair per ply (`[Vec<i16>; 2]`, by color),
  allocated once per thread; the root's comes from `refresh_by_color`, a child's from its parent's
  by `update(parent, child, parent_board, child_board)` right after the move is made, in the main
  search and in quiescence alike.
- A leaf evaluates with `evaluate_by_color_cp(&accumulators[ply], board)`.
- Hand-crafted is unchanged.

## 2. Move ordering

- **Order:** captures and promotions by MVV-LVA as today, then the two killers of the ply, then the
  other quiet moves by a history score `[piece][to]`.
- **Killers:** two per ply, the last two quiet moves that caused a cutoff there.
- **History:** a quiet move that causes a cutoff at remaining depth `r` gains `r * r`.
- **Iterative deepening per position:** each position is searched at `d` = 0, 1, ..., `D` in turn by
  one thread, the killers and history carried from one depth to the next, so a deeper search starts
  from the shallower one's order. Fresh tables per position, so a position's values never depend on
  which thread searched it or what it searched before.
- Quiescence keeps MVV-LVA only, as now.

## What changes in the bench's shape

Today the outer loop is the depth and the inner one the positions, so each depth prints when it
ends. With iterative deepening per position, the outer loop becomes the positions: every depth's
values arrive together at the end.

- **Work distribution:** positions handed out one at a time from a shared atomic index, instead of
  one fixed chunk per thread, so no thread sits idle while another finishes its hardest positions.
- **Progress:** a line every 10,000 positions, so a long run is never silent.
- **Timing:** per depth, the CPU time summed over threads, since the depths of one position are no
  longer timed apart in wall clock.

## Checks, at the start of every run

- The alpha-beta window check stays as it is, on the new search.
- **Ordering changes no value:** on 8 positions, at `d` = 0 to 2, the new search's values equal
  those of the search with MVV-LVA only and no killers or history (today's).
- **Incremental equals refresh:** on the same 8 positions, a network's incremental evaluation at
  every node visited equals `evaluate` from scratch (skipped for hand-crafted).

## Expected

Probably 2 to 3 minutes for amanda's `d` = 4 instead of 13, for identical numbers. Rerun once
hand-crafted's and amanda's 100,000-position results are recorded, and compare: they must match
to the digit.

## Decisions

1. **Positions as the outer loop, every depth printed at the end (decided 2026-10-08, the
   developer)**, with a progress line every 10,000 positions.

All decided; to implement once the running 100,000-position measurements are recorded.
