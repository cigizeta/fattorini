# `rotor-evolve match`: one match, no tournament, no registry

**Archived: implemented and played, since overtaken in its details.** It is `rotor-arena match`
now, with `--sprt` and `--depth` (match-at-fixed-depth.md); at a fixed depth Rotor plays in process
again (`match_player::plays_in_process`), reversing "every match player over UCI" below; self-play
lives under `data/games/self/`. For today's rules, read the code.

The developer wants the promoted amanda champion to play Stockfish at skill level 0, as a plain
match: not a tournament, and not through `data/engines.yaml`.

## Decisions (with the developer)

- `rotor-evolve match <player> <player>`. No tournament config, no `data/engines.yaml`.
- Best of 12, like the World Championship. Stops as soon as one side reaches 6.5.
- 3+2 for every game. No tie-break: 6-6 is a drawn match.

## Players, named on the command line

Built in code, as `run_gate` already builds its two players with `EngineRegistry::from_entries`:

- `hand-crafted`: Rotor in process with the hand-crafted evaluation (`EngineKind::Builtin`).
- `stockfish-<level>`, level 0 to 20: the `stockfish` binary on the `PATH` over UCI, with
  `Skill Level` set to the level (`EngineKind::Uci`, path `stockfish`). Anything outside 0-20 is
  refused.
- `<network>-NNNN`, e.g. `amanda-0001`: a promoted challenger, in process
  (`EngineKind::Network`). Only promoted challengers are cared for, and there is exactly one per
  generation, so `NNNN` counts promotions: `amanda-NNNN` is the champion promoted out of
  generation `NNNN`, `data/networks/<network>/gen-(NNNN+1)/initial.nnue`. The same numbering as
  self-play's labels, which store that champion's games under `data/self/amanda/gen-NNNN`. A
  promotion that has not happened yet is refused, and so is a bare `amanda`: a record's name must
  not depend on when the command ran. The label is the name itself, `amanda-0001`.

The two players must differ.

## Equal footing: every match player over UCI

Added after the first run: an in-process player cannot play under a clock, and the developer wants
both sides on equal footing anyway. So every match player is a UCI subprocess:

- `rotor` gets `setoption name EvalFile value <path.nnue>`: loads a network; empty means the
  hand-crafted evaluation, the default. Advertised in the `uci` reply. A file that fails to load
  prints the error and exits, rather than silently playing hand-crafted.
- `hand-crafted` is `bin/rotor` with no `EvalFile`; `<network>-NNNN` is `bin/rotor` with `EvalFile`.
- Every match player gets `Hash` 16 and `Threads` 1 explicitly (the defaults of both engines), and
  neither loads tablebases.
- The in-process player stays fixed-depth only: the gate keeps using it, for its evaluation A/B.

## Record

`data/matches/<champion>-vs-<challenger>/<date>/`, e.g. `data/matches/stockfish-0-vs-amanda-0001/20260927-1030/`
(the champion first, see champion-first.md):
`games.pgn` and `results.db`, round 1, the same format a tournament writes. `<date>` is when the match
started, `YYYYMMDD-HHMM` UTC, the same spelling as a segment's timestamp.

Added after the first match, so the same pairing can be played again:

- When a match ends, `result.txt` is written into its folder (the winner or `drawn`, and the final
  score), via `.tmp` and a rename -- the same idea as the gate's `verdict.txt`.
- Running the command resumes the latest dated folder if it has no `result.txt`; otherwise (finished,
  or no folder yet) it starts a new match in a new dated folder.
- No backward compatibility: the undated layout of the first match is not recognised (developer's
  call).

## Console

Also asked for after the first match: the move lines say which game a move is in, but not who has
which colour, which swaps every game. `play_pairing`, which knows both, prints:

- before each game: `game 2: stockfish-0 (White) vs amanda-0001 (Black)`;
- after each game, two lines: `game 3: 1-0 by checkmate, amanda-0001 wins` (or `drawn`), then
  `match:  amanda-0001 2.5 - 0.5 stockfish-0   (+2 =1 -0)` -- the tally, and the first player's wins,
  draws and losses (`+W =D -L`, the chess order), always from the first player's side so the counts
  never flip with the colours. The `match:` line is printed once more at the end, and `result.txt`
  holds it with the verdict.

For tournaments too, since they share `play_pairing`.

- A blank line after each game's result line, so every game stands apart. `play_one_game`'s own
  closing `println!()` only ends a half-written move line when the game ends on White's move, so it
  does not separate games reliably.

## Code

1. `config.rs`: `MatchSpec::tiebreak_time_control` becomes `Option<String>`. A tournament config
   still requires it (checked in `TournamentConfig::load`), so tournaments are unchanged.
2. `pairing.rs`:
   - `pub enum MatchOutcome { Won(String), Drawn }`; `play_pairing` returns
     `Option<MatchOutcome>` (`None` still means interrupted).
   - `decide`: after `best_of` games, a tie with no tie-break control is `Drawn`; with one, the
     tie-break games as today.
   - The final score is returned or printed: `amanda-0001 7.0 - 5.0 stockfish-0`.
3. `main.rs`: the `match` subcommand; `resolve_player(name) -> (label, EngineEntry)`; the match
   directory; `run_tournament` adapted to `MatchOutcome` (a tournament never gets `Drawn`).
4. README: the `rotor-evolve match` line.

## Tests

- `resolve_player`: `hand-crafted`; `stockfish-0` and `stockfish-20` with the skill option;
  `stockfish-21` and `stockfish-x` refused; `<network>-0001` resolving to `gen-0002/initial.nnue`
  in a temporary network directory; `<network>-0002` refused there (not promoted yet); a bare
  `<network>` refused.
- `decide`: 6.5 after 7 wins wins outright; 6-6 with no tie-break is `Drawn`; 6-6 with a tie-break
  continues.
- The match directory name for two labels.
- Choosing the folder: no dated folder starts a new one; the latest without `result.txt` is resumed;
  the latest with one starts a new one; an older unfinished folder behind a finished latest one is
  not resumed.
- `result.txt` round trip for a winner and for a drawn match.
