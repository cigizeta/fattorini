# The final `info` line repeated before `bestmove`

**Status:** applied and checked 2026-10-13 (tests pass; ponder hits on Lichess show a PV).

## Problem

On Lichess, some of rotor-bot's moves show no PV or score in lichess-bot's log, and they are ponder
hits. lichess-bot (through python-chess) collects a move's `info` lines only after it sends
`ponderhit`. Rotor prints an `info` line only when an iteration completes, so when no iteration
completes between `ponderhit` and `bestmove` (the ponder search was deep already, or had finished
and was waiting for the `ponderhit`), the move arrives with no `info` at all. The move itself is
the full search's; only the record is empty.

## Change

Just before every `bestmove`, `go_search` prints the last completed iteration's `info` line once
more, the way many engines end a search. The line is the winning thread's
(`select_best_result`), so its PV starts with the move `bestmove` plays, and `hashfull` is read at
that moment.

- **MultiPV above 1: no repeat.** It is an analysis setting, never used with ponder in a game, and
  repeating one rank out of several would be misleading.
- **No completed iteration: no line**, as today (no legal move, `bestmove 0000`).
- A ponder search the GUI stops prints it too: the GUI ignores that answer, line included.

Every GUI, the arena included, reads a repeated line harmlessly: it describes the same iteration.

## Tests

The tests that fix the number of lines a search prints, or check that each `info` line is one depth
deeper than the one before, account for the final repeat:

- `plain_go_returns_a_legal_move`, `go_depth_caps_iterative_deepening_at_the_requested_depth`,
  `multipv_of_one_reports_no_multipv_field_at_all`,
  `go_movetime_still_returns_a_real_move_even_when_the_budget_is_tiny`,
  `ponderhit_after_a_ponder_past_the_target_answers_at_once_from_the_same_search`.
- New: after a `ponderhit` that completes no further iteration, the output ends with an `info`
  line carrying a PV, then `bestmove`, and the PV starts with the move played.
- `setoption_multipv_makes_go_search_report_extra_ranked_lines` keeps its count: no repeat there.

## Checks

```bash
cargo test --release -p rotor uci
```

Then `./bin/smithy install amanda`, with lichess-bot stopped, and a look at the next ponder hits in
its log.
