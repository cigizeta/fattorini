# Converting a tablebase win: the shortest DTZ at the root

Status: designed and implemented 2026-11-16, to test. Left: the tests, a snapshot, then a won
ending seen converted on Lichess.

## The problem

`fattorini-bot` vs SeedChess, 180+2, 2026-11-16 (lichess.org/game/ERAKDeTB): from 58. h8=Q, KQ vs K
with the tables loaded, the queen gave ten moves of aimless checks, and White lost on time for a
draw. The clock was one cause; the play, the one this note fixes.

`root_ranks` (`tablebase.rs`) gives every win within the fifty-move clock the same top rank,
`MAX_DTZ`, and leaves the choice among them to the search (Stockfish's `root_probe`, `rankDTZ`
off). Stockfish can, because its evaluation knows KXK; Fattorini's cannot: its network, trained
mostly on results, sees every KQ vs K position alike, search probes only after a zeroing move, and
the mate is too deep for a blitz search. Only as the clock nears 100 plies does the ranking force
progress, too slowly for a blitz clock.

## The decision (the developer, 2026-11-16)

Only the root's choice changes, nothing in the evaluation (no mop-up term).

- **`excluded_root_moves`:** when the best rank is `MAX_DTZ` (a win within the clock), every move of
  that rank whose DTZ is above the shortest among them is excluded too. The search chooses among
  the moves of the shortest DTZ, so every move makes the most progress the tables know: towards the
  mate, or towards the next zeroing move that keeps the win.
- **Unchanged:** the ranks themselves, `root_result` and the reported score, `label_result`,
  `root_result_and_best_moves` (self-play already breaks the tie by DTZ itself) and
  `root_result_and_move`, the replay's label. The label's comment ("one of the moves play chooses
  among") is reworded: it is any move of the best rank, so the training data does not change.
- A draw, a loss, or a win the clock frustrates: as today (the frustrated ranks already prefer the
  shortest DTZ).
- DTZ is read with `ignore_rounding`: where a table rounds, two moves may compare off by one ply.
  Both still win within the clock; at worst the choice is not the very shortest.

## Tests (`tablebase.rs`)

- `a_move_that_throws_the_win_away_is_excluded_and_the_winning_ones_kept`: in `k7/7Q/1K6/...` the
  kept moves are exactly the four mates in one.
- `the_root_move_is_one_play_would_choose_and_never_spoils_the_result`: its first assertion pair
  no longer ties the label to play's moves.
- `the_engine_mates_with_queen_and_with_rook_within_the_fifty_move_rule`: each start, and the
  Lichess game's position after 58...Kc7 (`7Q/2k5/8/8/8/8/8/3K4 w - - 1 59`), mated within the
  root's DTZ, not merely within the fifty-move rule.
