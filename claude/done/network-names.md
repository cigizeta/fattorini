# Network and player names

Status: plan, 2026-10-23, decided with the developer one question at a time; nothing implemented.

## What (the developer, 2026-10-23)

A network's name, and a player's (a network with its search parameters), rethought: shorter in its
tail, each part marked by a letter, the run always written and always last.

## The scheme

| Part | Form | Example |
|---|---|---|
| config | its file's name in `configs/`, in full | `lc0-psq-threats` |
| generation | `gN` | `g1` |
| attempt, a candidate only | `aN` | `a2` |
| lambda, a candidate only | `lX`, two decimals | `l0.03` |
| run, always, last in the network | `rN` | `r1` |
| parameters, always | `@default` or `@tN` | `@t1` |

- A generation's winner: `lc0-psq-threats-g1-r1`; one lambda's candidate in one attempt:
  `lc0-psq-threats-g1-a2-l0.03-r1`.
- A player: `lc0-psq-threats-g1-r1@t1`, `lc0-psq-threats-g1-a2-l0.03-r1@default`.

## Decisions (the developer, 2026-10-23)

1. **`gN`, not `NNNN`:** shorter, self-describing, a pair with `rN`. Numeric sorting is the page's
   (`g10` after `g2`), not the alphabet's.
2. **The run always, and last:** never omitted, so a name never changes meaning when an `r2` appears;
   last, the least informative part, a tie-breaker, and sorting puts a recipe's repeats side by
   side (`g1-l0.03-r1` by `g1-l0.03-r2`).
3. **The lambda before the run:** from the left each part narrows the network (recipe, generation,
   lambda), the run saying which carrying-out of the whole recipe. Parsing goes by the letters, not
   by counting hyphens: the config ends at the last `-g<digits>` before them.
4. **Parameters after `@`, always written:** what follows `@` is how the network plays, not which
   file it is. `@default` is written too, though it is the one qualifier whose meaning moves (the
   engine's defaults change as search does, a tune is a fixed file): written, that stays visible.
   This extends arena's `@default` and `@<margin>` (`match_player::split_suffix`).
5. **`tN`, not `tuned`:** consistent with `gN`, `rN`, `lX`, and room for a second tune. A tune
   belongs to its run already, since it hangs off a name holding `rN`.
6. **The winner's attempt and lambda shown beside its name on the page:** `lc0-psq-g1-r1 (a2,
   l0.03)`.
7. **A candidate is named in full, its attempt included; a winner's attempt and lambda are
   implicit** (the developer, 2026-10-23, the attempt noticed missing). Today a candidate's name
   means the latest attempt's, so an earlier attempt's candidates have no name, and a name changes
   meaning when an attempt starts. A generation has at most one winner, its passing attempt's,
   after which it is closed: `gN` determines the winner's attempt and lambda, and leaving them out
   never changes the name's meaning. Order: config, `gN`, `aN`, `lX`, `rN`, then `@`.
8. **The config part stays in full:** names shorten once the best architectures are known.

## Open, for the implementation

- **`tN` beyond `t1`:** a network has one `parameters.txt` beside it and one tune folder (`tune/`,
  `tune-<lambda>/`), so only `t1` exists. A second tune needs its own folder and file; until then
  `@t2` is refused.
- **Names already written: renamed (the developer, 2026-10-23), unless the renaming can be
  avoided.** No reader of both forms: a one-time migration rewrites them to the new form, here and
  on the remote. Avoided where the name is not stored: the directories keep their layout
  (`run-1/gen-0001/attempt-1/lambda-0.03/`), a name being derived from it, not stored in it; the
  event log is stdout only. To rewrite: the tests' log (`data/tests/`, champion and challenger, with
  `@default` or `@t1` from its parameters columns, tests 1 and 2 so far); the tunes' `spec.txt` (its
  player); a candidate's tune folder, moved from `gen-NNNN/tune-<lambda>/` under its attempt; and
  any other file the survey finds holding a player's name (`chores.txt`, the arenas' chores, to
  check).
- **Where a name is shown without parameters** (the networks' list, a run's generations): the
  network's name alone, `@` only where a player is meant.
- **The run's own name** (`lc0-psq-r1`, in URLs, jobs, `parse_run`) is a run, not a network, and
  stays as it is.

## The work (from the survey, 2026-10-23)

- `rotor/src/pipeline/players.rs`: `network_player_name` and `parse_network_name` to the new form;
  `RotorPlayer::resolve_under` takes the run from it; their tests.
- `arena/src/match_player.rs`: `split_suffix` with `tN`; the suffix required for a network player.
- `smithy/src/smithy.rs`: `test_networks` (every attempt's candidates, not the latest's alone), the
  generations' list, `tune_args`; tests' specs with `@`. The tune folder of a candidate
  (`tune-<lambda>/`) is per generation today, not per attempt: it moves under its attempt.
- `smithy/src/page.html`: the cascade's names, the winner's lambda beside it, the tests' titles with
  the parameters in the name instead of `(default)` / `(tuned)`.
- Docs: DEV.md's player names and examples, TODO.md's names as written from now on.
