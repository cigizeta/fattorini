# Tablebase hits in self-play: no search, as in replay

**Status:** done 2026-10-15, implemented as tablebase-hits-without-search-implementation.md, tests
passing; the DTZ tie-break as proposed below. Follows insufficient-material-ends-the-game.md
(implemented the same day: self-play and arena matches end a dead position a draw).

## Decision (the developer, 2026-10-15)

**Every training game is followed to its end** (checkmate, stalemate, insufficient material, the
fifty-move rule, twofold repetition in self-play), not cut at the first tablebase position. **Self-play
does not search a position the tablebases rank**, as replay already does not.

First considered and dropped the same day: ending every game at its first tablebase position, its WDL
the result. Arguments recorded for later: the network rarely evaluates tablebase positions in play
(the search probes WDL after every zeroing move, and the first tablebase position is always reached
by one, so its WDL is exact); but a CCRL tester may have only 5-piece tables (CCRL 40/15 lists "4, 5
or 6 piece tablebases"), leaving the network to evaluate 6-piece positions it barely trained on; and
cutting replay would change every archive `.count`.

## Today

| | Replay | Self-play |
|---|---|---|
| Followed to the end | yes, the archived moves | yes |
| Label of a tablebase position | from the tables, no search (`replay.rs::tablebase_label`) | a depth-9 search |
| Move played | the archive's | the search's |

The labels are nearly equivalent: self-play's search probes the same tables (DTZ at the root, WDL in
the tree after zeroing moves). Only the search's cost differs.

## The change

At each self-play position after the exploration plies:

1. `SyzygyTablebase::root_result_and_move(board, ancestors, None)`.
2. **Ranked:** the label is its result in the band replay writes (`tb_win_at(1)`, 0, `tb_loss_at(1)`),
   and the move played is the DTZ best move. No search.
3. **Not ranked** (too many pieces, castling rights): searched as today.

- **One shared function:** `tablebase_label` moves out of `replay.rs` into a module both use, so the
  two label tablebase positions identically. Replay passes its archived move to break ties, self-play
  none.
- **Ties are broken by DTZ in self-play.** `root_result_and_move` with `preferred: None` is
  supported, but it ranks every win within the clock alike (`MAX_DTZ`, Stockfish's `rankDTZ` off, so
  that in play the search chooses) and takes the first in generation order, which need not make
  progress: the ranking forbids repetitions and narrows to progressing moves only as the clock runs,
  so a won ending could drift up to about 100 plies before converting. Without a search, self-play
  instead plays, among the moves of the best rank: when winning, the smallest DTZ (the fastest route
  to the next zeroing move); when losing, the largest (the longest resistance); when drawn, one at
  random, for variety. A variant exposing each move's DTZ, which `root_ranks` already computes.
  Replay keeps its archived move.
- **Shorter tails:** DTZ play reaches the next zeroing move as fast as the tables allow, so won
  endings are shorter than under depth-9 play: fewer rows, every one labelled exactly.
- **Tables:** DTZ is complete on every machine (the developer, 2026-10-15), so no position falls back
  to the search for a missing table.
- **Counts:** self-play's `.count` follows each game as played; replay is unchanged.
- No label's meaning changes, only the searches go. Lands before generation 4's self-play.

## Decisions

1. **Self-play keeps twofold repetition (the developer, 2026-10-15)**, not FIDE's threefold: a
   threefold game only adds rows of positions already seen.

