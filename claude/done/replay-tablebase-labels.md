# Replay labels from the tablebases

**Status:** implemented 2026-10-08, tests passing; generation 2's replay restarted under it the
same day on the Lichess machine (its search-only outputs so far, `ccrl/amanda/gen-0001/2017-05` chunks 002 to
004, deleted first). To `done/` once the replay has finished under it.

**First rates (2026-10-08, the Lichess machine):** 1,414 positions/s on the first chunks, about
1,000 sustained as the machine heats. Hand-crafted's replay ran at about 1,000 on the Lichess machine, and amanda
before the margin work took 1.92x hand-crafted's time (about 520 there): together with the tuned
margin (125, margin-search-command.md), amanda now labels about as fast as hand-crafted did. How
much of that the tables give alone was not measured (the developer: not needed).

## Why

The developer (2026-10-08) wants the replay faster, and of the levers found, takes only the exact
one: a position the Syzygy tables know needs no depth-9 search. The search probes the tables only
right after a zeroing move (`probe`'s WDL), never at the root, so a 6-piece ending reached by quiet
moves was searched in full with the network.

## Design (decided with the developer, 2026-10-08)

- **Replay only.** Play and self-play need a move, not a label, and self-play's no-adjudication
  decision (2026-09-22) stands. The search, UCI and self-play are untouched by the labelling.
- **The result under the fifty-move rule, from DTZ:** `SyzygyTablebase::root_result`, read off the
  root ranking `excluded_root_moves` already computed (now `root_ranks`, shared): a move winning
  within the clock, a win; every move losing within it, a loss; anything else a draw. `None` (no
  tables, castling rights, a missing table, no legal move) falls back to the search.
- **The label in the search's tablebase band**, `tb_win_at(1)` (`{318.98}`), 0, `tb_loss_at(1)`;
  not the label curve's 32-pawn ceiling, which belongs to training and could change. Both become
  a label of exactly 1, 0, -1 under today's curve.
- **No statistic** of how many positions were labelled so: the replay's speed shows the gain.

## A bug found and fixed on the way

`excluded_root_moves` takes the positions **before** the root, as UCI passes them. The arena's game
loop (`game.rs`), self-play and the replay passed a history ending with the root itself, so the
ranking saw a repetition at every root with a nonzero fifty-move counter: in arena games and
self-play a won root ranked its wins by DTZ instead of alike (still winning, a slightly different
move), and the new labels would have called most wins draws. The three callers now pass the
history without the root (`MoveRequest::ancestors` documents it); the search pushes the root itself,
so it searches as before.

## Tests

- `root_result`: KQ vs K won at clock 0 and 98, drawn at 99, lost for the bare king; with the root
  at the history's end, the win reads as a draw (the bug's signature); no tables, `None`.
