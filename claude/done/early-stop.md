# Early stop: adjudicated games, for SPSA alone

Plan, not yet implemented. Decided with the developer on 2026-10-16, for spsa.md's tuning: its
2,500 pairs at STC per network take about 4 hours on the training machine, and games ended early buy more pairs
or shorter tunes.

## Decisions (the developer, 2026-10-16)

- **Automatic in SPSA, and nowhere else: no command-line flag** (the developer, 2026-10-16, "I
  cannot risk that I forget it"). Early stop is a setting of the game runner, which SPSA's command
  always turns on for its games; `match`, `sprt`, `duel`, the selection and self-play never do. The
  decision of 2026-09-22, no adjudication of any kind, holds for every game outside SPSA. A flag for
  the other commands can come later, if a use appears.
- **More aggressive than fishtest.** fishtest passes fastchess only a draw rule (`-draw
  movenumber=34 movecount=8 score=20`, `worker/games.py`, fetched 2026-10-16), and no resign or
  tablebase rule. Rotor adds both.
- **The constants are taken as they are, not measured first.** The draw rule's are fishtest's; the
  win rule's are fastchess's usual resign settings, as remembered, not fishtest's.

## The rules

Checked by the arbiter (`game.rs`'s `play_one_game`), after the rules of chess. Scores are each
mover's own reported score, from its own side, in its reported centipawns, or a mate score. Every
Rotor network reports through the same label conversion, so a threshold means about the same for
each; the hand-crafted evaluation's are material, a somewhat different scale, which matters only
in games against it.

1. **Tablebase,** before each move: a position with no castling rights and no more pieces than the
   arbiter's tables cover (6 today) ends with its result under the fifty-move rule, from
   `SyzygyTablebase::root_result(board, ancestors)`, so a win the clock frustrates is a draw. The
   arbiter loads the tables with `data_root::load_tablebase` only when the flag is given.
2. **Win,** after each move: the last 6 plies (3 moves by each side) all have the same side ahead,
   its own scores at least +600 cp or a mate for it, the other side's at most -600 cp or a mate
   against it. That side wins. A move without a score, or one inside the band, resets the run.
3. **Draw,** after each move, fastchess's exact semantics (`match.hpp`'s `DrawTracker`, fetched
   2026-10-16): a run counts the plies whose mover reports a centipawn score with |score| <= 20; a
   score outside, a mate score or no score resets it to 0, and so does a pawn move or a capture (the
   fifty-move clock at 0), after which the same move can start a new run. A draw once the run
   reaches 16 plies (8 moves by each side) and at least 34 full moves are complete.

Constants, in `game.rs` beside the rules: `WIN_SCORE_CP = 600`, `WIN_PLIES = 6`, `DRAW_SCORE_CP =
20`, `DRAW_PLIES = 16`, `DRAW_MIN_FULL_MOVES = 34`.

## Code

- **No change to `main.rs`'s flags.** The runner's settings gain `early_stop: bool`, false
  everywhere but in SPSA's command, which sets it unconditionally.
- **`spec_line`:** ends with ` with early stop` when the setting is on, so a run resumed with the
  other setting is refused as another run, as a changed control is.
- **`game.rs`:** `play_one_game` takes an `Option<&SyzygyTablebase>`, `Some` when early stop is on;
  a small `Adjudicator` holds the two runs and is fed each move's report; its rules are pure
  functions of the reports, unit tested on their own.
- **`pgn.rs`:** three `Termination`s, `AdjudicatedWin`, `AdjudicatedDraw` and `AdjudicatedTablebase`
  (`adjudication-win`, `adjudication-draw`, `adjudication-tablebase`), so an adjudicated game is
  never mistaken for one played out. `results_db.rs` stores the string as it does the others.
- **The two callers,** `pairing.rs` (one game at a time) and `runner.rs` (pairs in parallel), pass the
  tables through when the setting is on.
- **DEV.md and SPSA's usage text:** that its games end early by these three rules, and that no
  other command adjudicates.

## Tests

- The draw run: 16 plies at |score| <= 20 after move 34 draws; one at 21, a mate score, a missing
  score or a capture resets; before move 34 nothing happens, however long the run.
- The win run: 6 plies agreeing draw nothing until the sixth; a disagreement (both sides claiming
  to be ahead) or one score at 599 resets; mate scores count.
- The tablebase rule: a won KQK ends at once as a win, a KRKR as a draw, a won position the clock
  frustrates as a draw (a fixture with the clock near 100), a position with castling rights is left
  to play.
- `spec_line` with and without the setting.
- With the setting off, `play_one_game` behaves exactly as today (the existing tests).

## Open

- Whether the gate in `arena select` should ever use it: not now, the gate judges networks on games
  played out.
- Measuring what it saves at STC, once it exists: plies and minutes per game with and without it.
