# `rotor-arena ladder <network>`: the lambda ladder, then the gate

## Status

Superseded, see round-robin-selection.md: the ladder's chain of duels was replaced by a round robin
ranked by points, before it ever ran on real candidates. The gate survives unchanged.

Was done: `rotor-arena` and `rotor-train` tests pass. Not yet run on real candidates. Follows per-lambda-training.md,
whose `ladder.txt`/`verdict.txt` contract with `rotor-train promote` it keeps.

## Decided with the developer

- **Replaces `rotor-arena gate`**, which goes. The ladder plays in grid order: the second lambda
  against the first, the third against the holder, and so on; then the holder, the generation's
  **challenger**, against the champion (the hand-crafted evaluation in generation 1).
- **(a) The newcomer plays first, and only its win switches the holder.** Amended for ladder duels by
  ladder-tie-break.md: a drawn ladder duel goes to the player with more wins. A draw keeps the holder,
  under every rule: the SPRT's H0, a drawn duel, a level best of n. Ties therefore favour the lambdas
  earlier in the list. In the final duel the challenger plays first, and only its win passes.
- **(b) One rule for every duel**, ladder and final alike: the config's `ladder:` section, the fields
  `gate:` had (`rule`, `best_of` or `elo_d` as the rule needs, exactly one of `depth` or `time`).
  `gate:` is gone.
- **(c) Every folder and record is named by its two players.**
- **A one-value grid plays no ladder duel**: its lambda is the winner, and only the final duel is
  played. That is the old gate.

## Player names

A network's player is `<network>-NNNN-<l>` or `<network>-NNNN`, everywhere: the ladder, and the
`match`, `sprt` and `duel` commands.

- **`betty-NNNN-0.7`**: generation NNNN's run at lambda 0.7. The champion's `champion.nnue` when
  generation NNNN was promoted from that lambda (`champion.txt`'s `lambda`), so the name works on a
  machine holding only champions; otherwise `gen-NNNN/lambda-0.7/candidate.nnue`, which must exist.
- **`betty-NNNN`**, lambda omitted: the lambda that won generation NNNN. The champion when the
  generation was promoted; otherwise the winner `ladder.txt` names; refused while it names none.
- `hand-crafted`, `stockfish-<level>`, `stockfish-elo-<rating>` as today.

The ladder writes full names only (with the lambda), so its records never depend on when they were
read. A champion promoted before `champion.txt` recorded a lambda answers to its bare name only.

## Layout

```
gen-0002/ladder/betty-0002-0.7-vs-betty-0002-0.65/    games.db, games.pgn: one ladder duel
gen-0002/ladder/betty-0002-0.75-vs-betty-0002-0.7/
...
gen-0002/ladder/betty-0002-0.75-vs-betty-0001-0.7/    the final duel: the challenger against the champion
gen-0001/ladder/betty-0001-0.75-vs-hand-crafted/      generation 1's final duel
gen-0002/ladder.txt                                   one line per duel, then winner
gen-0002/verdict.txt                                  pass or fail, from the final duel only
```

## Running and resuming

1. Refuse unless every lambda of the grid has its `candidate.nnue`.
2. Play the ladder duels in order. A duel already decided rebuilds its outcome from its own
   `games.db` at once, as the gate does, so a rerun after Ctrl+C resumes where it stopped with no
   state file of its own.
3. Write `ladder.txt`, every ladder duel's line and `winner`, before the final duel starts.
4. Play the final duel; add its line to `ladder.txt`, then write `verdict.txt`.

A `ladder.txt` line: `duel<TAB><first> vs <second>: <outcome>, <summary>`, the runner's summary
(`+W -L =D, score/games, <rule status>`), in the order played, the final duel last.

## Code

- `config.rs`: `gate:` becomes `ladder:` (`load_ladder_spec`, `LadderSpec`); the committed-config test
  follows.
- `match_player.rs`: the name rule above, tested against a temporary network directory.
- `main.rs`: `run_ladder` replaces `run_gate`; the usage names `ladder`.
- `generation.rs`: the ladder duel folder (`ladder_dir`); `gate_database_path`/`gate_pgn_path` go.
- `rotor-train`'s messages and the README name `rotor-arena ladder`.
