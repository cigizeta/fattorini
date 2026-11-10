# The tablebase score at the root

**Status: design agreed, plan written (2026-11-10).** Nothing implemented.

## The problem

On Lichess, in KQ vs KN with Syzygy tables loaded, `fattorini-bot` reported +1.59 for the queen's
side, a tablebase win.

- **Search** probes only positions reached by a zeroing move (`tablebase.rs`, `should_probe`):
  `probe_wdl_after_zeroing` is valid only with the 50-move counter at 0, and without DTZ in search
  a later position is not probed. In a ending reached long ago, the only zeroing node is the capture
  of the last piece; beyond the horizon, every leaf is the network's evaluation, cautious in late
  positions by design on Lc0's labels (continuous-training.md).
- **The root** is ranked by DTZ (`root_ranks`), and moves ranked below the best are never searched
  (`excluded_root_moves`), so play keeps the result within the 50-move rule. But the reported score
  is the search's, so the root's known result never reaches `info`.

Play is correct; the score shown is not. Stockfish reports the root's tablebase result instead.

## Decisions (the developer, 2026-11-10)

1. **When the root is ranked** (`root_rankable`: no more pieces than the tables cover, no castling
   right), the reported score comes from the result of the best rank (`result_of_rank`):
   - **Win:** `tb_win_at(0)`, the tablebase band, Stockfish's style: a large fixed value, not one
     shrinking with DTZ.
   - **Draw**, cursed wins and blessed losses included (already draws in `wdl_to_result` and the
     ranks): 0.
   - **Loss:** `tb_loss_at(0)`.
2. **A decisive search score of the same sign is kept:** a proven mate (`mate in N`) says more than
   the table's result, and a tablebase-band score from the search already says the same thing.
   Anything else is replaced.
3. **Only the reported score changes.** The search, its move choice and `excluded_root_moves` stay
   as they are, so strength cannot change: tests, no duel.
4. **Every report carries it:** each iteration's `info` line and the final one, so lichess-bot and
   GUIs show one score throughout.

## What it touches

- **The arena:** its early stop (`game.rs`, `Adjudicator`) reads reported scores, its win rule a
  large lead agreed by both sides, its draw rule scores near 0. Root-tablebase scores make both
  agree with the tables sooner, which is correct; early-stop chores also probe the arena's own
  tablebase. Matches (`--full`) do not adjudicate. Nothing to change.
- **Self-play labels:** a self-play root in the tables is played by `root_result_and_best_moves`
  without a search, so no label changes. To confirm while implementing.

## Plan

1. **`SyzygyTablebase::root_result(board, ancestors) -> Option<TbResult>`**, from `root_ranks` and
   the best rank, beside `root_result_and_best_moves` (or that function reused). `None` where the
   root is not ranked.
2. **`fn root_reported_score(search_score: i32, root: Option<TbResult>) -> i32`** in `search.rs`:
   decisions 1 and 2. Pure, so the rule has one definition and its own tests.
3. **The iterative loop** (`search.rs`, where `excluded` is computed): compute the root result
   once, beside `excluded`, and pass every `IterationReport`'s score through
   `root_reported_score` before it is reported, on every thread whose reports are used. The
   search's own internal scores (aspiration windows, `previous_score`) stay the search's.
4. **Tests:**
   - `root_reported_score`: a win replacing an ordinary score, a draw replacing +80, a loss, a mate
     of the same sign kept, a tablebase-band score of the same sign kept, a decisive score of the
     wrong sign replaced, `None` leaving the score alone.
   - With the local tables (as `tablebase.rs`'s tests, skipped without them): KQ vs KN, the queen
     to move, the final report in the tablebase band; KR vs KR, 0; a position with castling rights,
     untouched.
   - The UCI line: a root-tablebase win formats as `cp` in the band, not as `mate`.
5. **Then:** on Lichess, the next tablebase ending shows the band; TODO's item closed and this
   document moved to `done/`.
