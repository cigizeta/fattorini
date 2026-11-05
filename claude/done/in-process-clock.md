# In-process Rotor players under a clock, each with its own margin

**Status:** done 2026-10-08, tests passing; amanda-0001's margin search ran on it (three best-of-400
matches at 10+0.1, margin-search-command.md).

- `search.rs`: `MARGIN_PER_PLY` is `pub`; `Search::margin_per_ply` (default `MARGIN_PER_PLY`) feeds
  `eval_margin_per_ply` and delta pruning; `choose_best_move_with_margin` added, and
  `choose_best_move_with_progress` and `search_root_pondering` take the margin as an override,
  `Option<i32>` since tuned-margin-storage.md (UCI and MultiPV pass `None`: the network's own margin,
  else the default); `time_limit` is `pub`. `EVAL_MARGIN_PER_PLY`, `ROTOR_MARGIN_PER_PLY` and
  `parse_centipawns` removed.
- `in_process.rs`: under `MoveLimit::Fischer`, `time_limit` from both clocks, depth `MAX_PLY`,
  against a fresh stop flag per move (the search raises its flag when it declines the next depth;
  on the match's Ctrl+C flag that would end the run).
- `match_player.rs`: `plays_in_process(one, two)` with no control; `split_margin` for `@<cp>`;
  `EngineEntry::margin_per_ply` (`engines.rs`), passed by `pairing::start_player`. The gate
  (`main.rs`) is unchanged: in-process at a fixed depth only.
- Round 2 runs as `rotor-arena match <incumbent> <probe> 400 --time 10+0.1`.

## Why

Round 2 of margin-round-robin.md plays margins against each other in `rotor-arena`'s best of 400
(`best_of.rs`, its early stop already there), not through cutechess-cli (the developer,
2026-10-08). Two things stand in the way:

- Under a clock every Rotor player is a UCI subprocess of the one release build
  (`match_player::plays_in_process`); the in-process player panics on `MoveLimit::Fischer`.
- The margin is a compile-time constant (`EVAL_MARGIN_PER_PLY`, from `ROTOR_MARGIN_PER_PLY`), so
  two margins need two builds, which the arena cannot play.

A fixed depth is no substitute: a tighter margin saves nodes, worth something only when there is a
clock to spend them on.

## Design

1. **The in-process player keeps a clock.** On `MoveLimit::Fischer` it builds its `TimeLimit` with
   `search::time_limit` (made `pub`), from both clocks and the increment, exactly as
   `go wtime btime winc binc` does, and searches with it to `MAX_PLY`. One thread, as the UCI
   players are given. The panic goes.
2. **Both players Rotor means in-process, under either control** (the developer, 2026-10-08:
   "in-process with a clock, if the engine is a rotor one"). Any Stockfish player keeps both sides
   on UCI, as now.
3. **The margin is a player's value.** The search's `eval_margin_per_ply` is set from the player's
   margin (in hand-crafted's pawn) times the evaluator's pawn scale, instead of from the constant;
   a player with none uses `MARGIN_PER_PLY`. The `ROTOR_MARGIN_PER_PLY` build variable and
   `parse_centipawns` are removed.
4. **The match:** `BestOf::new(400)`, unchanged: first past 200 points wins, 200 each is drawn and
   keeps the incumbent.

## To check

- **Concurrency:** 8 games at once is 8 search threads plus the game loop in one process, one
  per physical core, as `-concurrency 8` today. The clock is the game loop's wall time,
  as for UCI players.
- **Comparability:** no protocol latency on either side; fair between the two, but Elo numbers
  are not comparable with the cutechess-cli duels.

5. **The notation (the developer, 2026-10-08):** `<player>@<cp>`, for example `amanda-0001@150` or
   `hand-crafted@150`, recorded wherever the name is (results db, PGN, pairing directory).
   `resolve_player` strips the suffix into the entry's margin, a positive integer, anything else
   refused. Refused on a UCI player: Stockfish has no such margin, and a Rotor subprocess cannot
   be given one.

## Tests

- The in-process player under a clock returns a legal move within its budget, and spends more
  with more time on the clock.
- `plays_in_process`: Rotor against Rotor in-process under a clock and at a fixed depth; any
  Stockfish player, UCI for both.
- The notation: `amanda-0001@150` resolves with margin 150; `@0`, `@x`, `@` and `stockfish-5@150`
  are refused; no suffix means `MARGIN_PER_PLY`.
- The margin reaches the search: hand-crafted at depth 9 on the 49 bench positions with margin 200
  gives today's 3,140,380 nodes; 100 gives a different count.

## Decisions

All decided; to implement once approved.
