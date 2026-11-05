# Capture history (B4 of search-optimizations.md)

**Status:** shelved 2026-10-07, the code reverted. The duel below showed no gain: 25 - 27 - 48
after 100 games, 0.490, -6.9 +/- 49.4 Elo, stopped there since about 0.555 over the remaining 900
would have been needed to clear 0.549 at 1000. Like B6, a change of a few Elo at most, which the
duel rule cannot confirm in 1000 games (it needs about +34 there); the developer kept the rule
rather than judging small changes by SPRT. The implementation cut the reference positions' nodes by
up to 4.5% with every move and score unchanged, so it orders as meant. On the label path: it changes the tree, so it
is judged by a duel under a clock (cutechess-cli, as B1), and lands before generation 2's
labelling.

## What it is

Rotor's history learns only from quiet moves: a capture that cuts off teaches nothing, and the
captures are ordered by the victim alone (`capture_score`, `7 * victim + promotion`). Capture
history records which captures, by moving piece, target square and captured type, have cut off in
this search, and adds that to their order; the score also feeds the good-capture threshold
(`see_ge(-score / 18)`), so a capture that keeps working is tried among the good ones even when the
exchange looks slightly losing.

## Stockfish 16.1 (fetched 2026-10-07, `movepick.cpp`, `movepick.h`, `search.cpp`)

- `CapturePieceToHistory = Stats<int16_t, 10692, PIECE_NB, SQUARE_NB, PIECE_TYPE_NB>`, against
  `ButterflyHistory`'s 7183.
- Captures score `7 * PieceValue[captured] + captureHistory[moved][to][captured]`; good captures
  pass `see_ge(m, -value / 18)`. Evasions score a capture by its victim only.
- `update_all_stats`, whenever the node has a best move: a capture as best move gets
  `stat_bonus(depth + 1)`; every other capture searched (at most 32) gets `-stat_malus(depth)`,
  whether the best move was a capture or a quiet.
- Also read by futility pruning of captures, which Rotor does not have.

## Design (decided 2026-10-07)

1. **The table**: `[12][64][6]` `i32`, by moving piece, target square and captured piece type,
   per thread and threaded through the iterative deepening as `history` is; fresh per `go`, as
   `history`.
2. **Ordering in the main search only** (developer decision): `capture_score` becomes
   `7 * victim + promotion + entry / 2` for a capture, Stockfish's weight with Rotor's pawn about
   half of its; the good-capture threshold reads the new score as before. A promotion that captures
   nothing has no entry. Evasions keep the victim alone, as Stockfish's. Quiescence keeps its
   ordering, so the duel measures one change.
3. **Updates as Rotor's quiet history**, not Stockfish's: at a cutoff only, the same `remaining^2`
   bonus, the same gravity (`apply_history_bonus`), a cap `CAPTURE_HISTORY_MAX` of 12288 (1.5 times
   `FLAT_HISTORY_MAX`, Stockfish's ratio). A capture that cuts off gets the bonus; every capture
   searched before the cutoff gets the malus, whether the cutoff move is a capture or a quiet. The
   captures searched are kept on a stack beside `quiets`, the same way.
4. Magnitudes picked by Stockfish's ratios, not tuned, as every search parameter.

## Tests

- The capture score: a positive entry raises a capture's order, and a large enough one turns a
  slightly losing capture into a good one.
- The update: a cutoff by a capture raises its entry; the captures searched before a cutoff are
  lowered, after a quiet cutoff too; entries stay within `CAPTURE_HISTORY_MAX`.
- The move picker still gives every move exactly once.
- Node counts change; recorded as the new references.

## Verification

A duel at 10+0.1 through cutechess-cli, against `~/rotor-b1.exe` (the committed search), both
hand-crafted, stopped by Rotor's duel rule (0.5 + 3.09 * 0.5 / sqrt(N)) or at 1000 games.
