# `rotor-arena margin`: the margin search as one resumable command

**Status:** done 2026-10-08, tests passing; amanda-0001 tuned to 125 cp per ply with it (below).
Since the same day also a stage of `rotor-arena select` (tuned-margin-storage.md), its matches then
under `gen-NNNN/selection/margin/`. Its tree superseded 2026-10-11 by a gauntlet, see
margin-gauntlet.md; the command, the folders and the stateless resume stay.

## Why

Round 2 of margin-round-robin.md was started by hand, one `rotor-arena match` at a time. The
developer (2026-10-08): one command should start every match the tree needs, up to the final
winner, and be resumable. The whole search restarts under it, round 1's 100 against 200 replayed in
the arena (the cutechess-cli results are not reused), and the hand-started `amanda-0001@100` against
`amanda-0001@150` (36.0 - 32.0 at 68 games, interrupted) is abandoned.

## Design

- `rotor-arena margin <player> (--depth <d> | --time <base+inc>) [--hash <MB>]`, for example
  `bin/rotor-arena margin amanda-0001 --time 10+0.1`. `<player>` is any Rotor player without `@`.
- The tree of margin-round-robin.md, fixed in code (`margin_search::search`), each match a best of
  400 between `<player>@<a>` and `<player>@<b>`, the best value so far the champion, a draw keeping
  it. 400 against 200 is not played.
- Each match in a fixed folder, `<champion>-vs-<challenger>/`, not the dated folders of
  `data/matches/`: in `data/margins/<player>/` at first (amanda-0001's search), since 2026-10-08
  under `gen-NNNN/selection/margin/` for a generation's network (tuned-margin-storage.md). Stateless resume: every start walks the tree from match 1; a match
  whose games already resolve it replays them to the verdict without playing
  (`runner::play`), so the walk reaches the first unresolved match and resumes it.
- The winner goes to `margin.txt` beside the player's network (`data/networks/amanda/gen-0001/margin.txt`
  for amanda-0001), `margin_per_ply\t<cp>`; the engine reading it is tuned-margin-storage.md.
- `main.rs`: `run_versus` split, `play_versus_in` playing one `Versus` in a given folder, shared.
- Also: `BestOf` prints an ETA (`most_pairs`, n/2), and its status no longer repeats the points.

## amanda-0001's search (2026-10-08, the Lichess machine, 10+0.1)

| match | result | games | score |
|---|---|---|---|
| 100 against 200 | 100 wins, 200.5 - 177.5 | 378 | 53.0% |
| 150 against 100 | 100 holds, 201.5 - 194.5 | 396 | 50.9% for 100 |
| 125 against 100 | 125 wins, 200.5 - 187.5 | 388 | 51.7% for 125 |

**amanda-0001: 125 cp per ply**, in `data/networks/amanda/gen-0001/margin.txt` (69 of her own cp at
her 55% pawn scale). Matches 2 and 3 were within noise: the curve is flat from 100 to 150.

## The control (the developer, 2026-10-08)

Tuned at 10+0.1 only, nothing longer, and nothing below 100. Accepted: the best margin likely grows
with thinking time, and CCRL's 40/15 gives about 90 times more per move, so the release may play
slightly below its best there. Not checked, by choice: a best of 400 at 60+0.6 is about 2.5 hours, at
40/15 about 1.5 days. The search's own result so far: 100 beat 200, 200.5 - 177.5 at 378 games
(+21 Elo; cutechess-cli's round 1 had +17).

## Tests

- `margin_search`: every branch plays its matches in order with its result; an interruption stops
  the walk; a draw keeps the champion.
- `best_of`: at most n/2 pairs.
