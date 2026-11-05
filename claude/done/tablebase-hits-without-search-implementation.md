# Tablebase hits without search: implementation plan

**Status:** done 2026-10-15: implemented, `cargo test --release -p rotor tablebase` and `-p arena`
passing. The self-play test plays the exploration
plies as they are (searched) and checks only the plies after them: no parameter added. Implements tablebase-hits-without-search.md.

## Summary

At each self-play position past the exploration plies, the DTZ ranking of the root
(`SyzygyTablebase::root_ranks`) is tried first. Where it ranks the root, the position's label is its
result in the tablebase band and the move played is chosen by DTZ among the moves of the best rank,
with no search. Elsewhere, the depth-9 search as today. Replay is unchanged except that its
tablebase score comes from a function shared with self-play.

## Step 1: `crates/rotor/src/tablebase.rs`

1. **`RankedMove` keeps its DTZ:** `struct RankedMove { mv, rank, dtz }`, `dtz` being the value
   `root_ranks` already computes per move, from the mover's side (positive winning, 1 a mate in one,
   negative losing, 0 drawing).
2. **New `pub fn root_result_and_best_moves(&self, board, ancestors) -> Option<(TbResult, Vec<(Move,
   i32)>)>`:** the root's result (`result_of_rank` of the best rank) and every move of the best rank
   with its DTZ, in generation order. `None` where `root_ranks` is `None` or there is no legal move.
   No choice made here: the rotor crate stays free of the randomness the self-play choice needs.
3. **`root_result_and_move`** (replay's) becomes a thin wrapper over it: the preferred move when
   among the best, otherwise the first. Behaviour unchanged.
4. Doc comments: `root_result_and_best_moves` says why the DTZ is exposed (self-play plays without a
   search, so it must break ties by progress, Stockfish's `rankDTZ`, which play leaves to the search).

## Step 2: the shared score, `crates/arena/src/tablebase_label.rs` (new)

- `pub fn tablebase_score(result: TbResult) -> i32`: `tb_win_at(1)`, 0 or `tb_loss_at(1)`, moved
  out of `replay.rs::tablebase_label` with its doc comment (why the band and not the label curve's
  ceiling).
- `pub fn self_play_tablebase_move(best: &[(Move, i32)], result: TbResult, rng) -> Move`:
  - **Win:** the smallest DTZ among `best` (the fastest route to the next zeroing move or mate);
    ties by generation order.
  - **Loss:** the largest DTZ in absolute value (the longest resistance); ties by generation order.
  - **Draw:** one of `best` uniformly at random, for variety.
- `replay.rs::tablebase_label` calls `tablebase_score`; `mod tablebase_label;` in `main.rs`.

## Step 3: `crates/arena/src/self_play.rs`, `play_one_game`

In the branch past `EXPLORATION_PLIES`, before `choose_best_move`:

    match tablebase.root_result_and_best_moves(&board, ancestors) {
        Some((result, best)) => {
            let mv = self_play_tablebase_move(&best, result, rng);
            (mv, mv, tablebase_score(result), 0)
        }
        None => // choose_best_move as today
    }

- **Depth 0** for a tablebase label: no search ran. Self-play's PGN does not write the depth
  (`include_depth_and_time` false), so nothing in the training data changes shape.
- **The score** goes through the existing `score_cp`/`mate_in` branch: the tablebase band sits below
  `MATE_SCORE - MAX_PLY`, so it is written as `score_cp`, as a search's tablebase score is today.
- **The exploration plies** are untouched: a tablebase position cannot occur that early.
- The comment above `score_cp`/`mate_in` that says Rotor has no DTZ tables is stale (DTZ ranks the
  root since 2026-10-07); corrected in passing.

## Step 4: tests

- **`tablebase.rs`** (with `local_tables`, skipped without tables, as the existing DTZ tests):
  - `root_result_and_best_moves` on KQ vs K: a win, every listed move winning, a mate in one listed
    with DTZ 1 when there is one.
  - On a drawn position (KB vs KB is dead, so KR vs KR): a draw, every listed move drawing.
  - `root_result_and_move` still prefers the given move when it is among the best.
- **`tablebase_label.rs`** (no tables needed, plain vectors):
  - A win picks the smallest DTZ; a loss the largest absolute DTZ; ties keep the first.
  - A draw always picks one of the given moves.
  - `tablebase_score` maps Win, Draw and Loss to `tb_win_at(1)`, 0 and `tb_loss_at(1)`.
- **`self_play.rs`** (with tables, skipped without): a game from KQ vs K, White to move, past the
  exploration plies, ends in checkmate, every annotation's score `tb_win_at(1)` or its negation, no
  search run (depth 0 on every annotation). Needs `play_one_game` to start counting plies past the
  exploration window; the test passes a FEN and checks only the plies after `EXPLORATION_PLIES`, or
  the exploration constant is made a parameter for the test (to decide while implementing, the
  smaller change).

## Commands, for the developer

    cargo test --release -p rotor tablebase

    cargo test --release -p arena

## Not changed

- Replay's labels and its archive `.count` files.
- The search's own probing (WDL in the tree, DTZ at the root).
- Arena matches: a match plays by the engines' own searches.
