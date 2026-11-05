# Champion first -- the champion is listed before the challenger

## The convention

Wherever two players of a run are shown or named, **the champion comes first and the challenger
second**: on the command line, in event names, folder names, log lines and `ladder.txt`. Until now
the challenger was first everywhere (`runner.rs`: "The first player is the challenger").

## Decisions

- **CLI.** `rotor-arena match | sprt | duel <champion> <challenger> ...`. The SPRT stays one-sided,
  now read as: is the **second** player (the challenger) at least `elo_d` stronger than the first?
- **Names follow display order.** Event `<champion> vs <challenger>`; match folder
  `data/matches/<champion>-vs-<challenger>/<YYYYMMDD-HHMM>/`; console line
  `duel: amanda-0001 (champion) vs hand-crafted (challenger), ...`.
- **Ladder.** A ladder duel lists the holder first and the newcomer second; the final duel lists the
  previous champion first and the ladder's winner second. `generation::duel_dir(champion,
  challenger)` gives `gen-NNNN/ladder/<champion>-vs-<challenger>/`, and each `ladder.txt` duel line
  reads `<champion> vs <challenger>: ...`. The rule itself is unchanged: only the newcomer's (the
  challenger's) win takes the ladder, and only the gate challenger's win passes.
- **Role names in the code, not positions.** `Outcome::FirstWins / SecondWins` become
  `ChallengerWins / ChampionWins`; `Labels.first_wins / second_wins` become
  `challenger_wins / champion_wins`. `SequentialTest`'s docs speak of the challenger's side. Display
  order is then a presentation choice only, and a future change of it touches no rule.
- **Unchanged.** The challenger is still White in game 0 of every pair. `results.db` rows stay keyed
  by role (`engine_one` = challenger, `engine_two` = champion), as `Run` already passes them. Every
  rule still consumes scores from the challenger's side. The tournament bracket
  (`engine_one`/`engine_two`, `pairing.rs`) has no champion and is untouched.
- **Existing data is left as is.** The six folders under `data/matches/` keep their old names and
  old meaning (first name = challenger). A run started before the change resumes only if its command
  is not rerun under the new order -- in particular the running `hand-crafted-vs-amanda-0001` depth 9
  duel, if interrupted after the change, starts afresh under
  `data/matches/amanda-0001-vs-hand-crafted/`. No local ladder folders exist.

## Changes

1. `src/arena/runner.rs`: `Outcome` and `Labels` renamed as above; module doc ("The first player is
   the challenger") rewritten to the convention; the console line
   `{prefix}: {champion} (champion) vs {challenger} (challenger), {describe}`.
2. `src/arena/duel.rs`, `best_of.rs`, `sprt.rs` (its `SequentialTest` impl in `runner.rs`): the
   renamed variants; tests' expectations unchanged in substance.
3. `src/arena/main.rs`:
   - `Versus { champion, challenger, ... }`, parsed in that order; usage lines
     `<champion> <challenger>`, the SPRT line "whether the second player (the challenger) is at least
     elo_d stronger than the first (the champion)"; module doc at the top.
   - `run_versus`: event name, `pairing_dir(champion, challenger)`, `Run` fields by role,
     `versus_labels(rule, champion, challenger)` with role-named fields, doc comment's folder name.
   - `run_ladder` / `play_duel`: `play_duel(..., champion, challenger, ...)`; ladder duels pass
     `(holder, newcomer)`, the gate `(champion, challenger)`; `Outcome::ChallengerWins` checks; doc
     comment ("The newcomer plays first" becomes "the holder is listed first").
   - Tests: `parsed(...)` field names; order assertions.
4. `src/arena/config.rs:57`: "Whether the challenger (the second player) is at least `elo_d`
   stronger".
5. `src/arena/match_player.rs`: `pairing_dir(champion, challenger)` and its doc; its test.
6. `src/pipeline/generation.rs`: `duel_dir(generation_dir, champion, challenger)`, its doc and test
   (`.../ladder/hand-crafted-vs-amanda-0001-0.70`).
7. Docs: `README.md` (ladder folder, `match`/`sprt` examples and the SPRT sentence, match folder),
   `arena-network-pull.md` (its `duel` example), `rotor-evolve-match.md` (folder example),
   `rotor-network-gate.md` and `sprt.md` wherever they say the challenger is first. Documents in
   `claude/done/` are history and are not edited.

## Status

Implemented and verified: the tests pass, and `duel stockfish-0 hand-crafted --depth 1` recorded in
`data/matches/stockfish-0-vs-hand-crafted/`. Deviations: `sprt.md` and `rotor-network-gate.md`
already speak by role (`engine_one` is still the challenger in `results.db`) and needed no change;
`arena-network-pull.md`'s `duel` example is a finished verification and was left as written.

## Verification

```
cargo test --release --bin rotor-arena
```

Then a short run to see the console line and the folder name, e.g.
`bin/rotor-arena duel hand-crafted stockfish-0 --depth 1`.
