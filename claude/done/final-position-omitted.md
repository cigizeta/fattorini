# The final position omitted everywhere

**Status:** done 2026-10-29, tests and clippy passing (the developer: the final position always omitted, the
transformer's track fixed if need be).

## Why

A game's final position has no move played from it, so no search labelled it: its label is the
result alone. The NNUE forges, the frozen test sets and `engine_loss` already leave it out
(`Keep::Searched`, `Keep::Quiet`, 2026-10-28). The transformer's stream (`pgn_train_stream.rs`) still
read it with `Keep::Every`, because its epoch is the `.meta` files' `positions` summed, and those
count the final position.

## Design

1. **`Keep::Every` deleted:** no reader keeps the final position. Left: `Searched` and `Quiet`.
2. **The transformer's stream reads `Keep::Searched`,** each file's positions `meta.positions -
   meta.games`: a game has exactly one final position, and every `.meta` records its games. No data
   rebuilt, no new file.
3. **`self_play_game_positions`** (tests only) uses `Keep::Searched`: it returns every position with
   the mask beside it, so its output is unchanged.
4. **Tests:** the stream's fixture counts 5 searched positions a pair of games, not 7; the
   `Keep::Every` assertion in `game_labels.rs` goes.
5. **Comments** of `Keep`, the stream's header and `start_epoch` reworded.

Unchanged: the converter still writes the final position and `.meta` still counts it; the omission
is on reading. `self_play_game_labels` still labels every position, the mask deciding. No transformer
has trained yet, so no run's epoch moves.

## Checks

`cargo test -p smithy`, `cargo clippy`.
