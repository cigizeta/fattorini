# Run 1 named `<config>-r1`, as every other run

**Status: agreed with the developer, 2026-10-23; implemented, checks pending.** The developer:
bare names and old names refused; a run-1 tune not finished restarts fresh (its `spec.txt`'s player
line no longer matches); the code fixed wholesale now.

## Why

A forge run is named `<config>` for run 1 and `<config>-r<n>` for the others (`players::run_name`):
run 1 kept the bare name records used before runs existed. On the page, the bare name reads as the
config, not as one of its runs. The developer, 2026-10-23: the design must be uniform, run 1 is
`<config>-r1`; at worst, the production run restarts.

## Inventory: where a run's name is stored

- **Directories: nowhere.** A run lives in `data/gen/<config>/run-<n>/` (`players::run_dir`), on this
  machine and on Drive alike: numbered already. No directory moves.
- **Networks:** found by path (`publish`), never by run name: the arenas' tasks, the teacher, the
  pairs. Unchanged.
- **`data/smithy-last.txt`:** `--forge <config> <number>`, a number. Unchanged.
- **A tune's `spec.txt`:** its `player` line holds the player's name (`pesto-psq-0001`), and a resumed
  tune checks it (`tune::check_spec`). The only stored name. Only tunes of run 1's networks are
  concerned, and only to resume one not finished: a finished tune is never checked again.
- **Typed names:** DEV.md's examples, `configs/*.yaml`'s `tune: {fine: <player>}` if any, the
  commands the developer types (`arena duel`, `smithy install`). `claude/` keeps its history as
  written.

So the production run on the training machine, run 1 of lc0-psq, in training, with no tune yet, needs no change on
disk: the new binary calls it `lc0-psq-r1`, in the same `run-1/`.

## Design

1. `run_name(config, n)` is `<config>-r<n>` for every n, 1 included.
2. `parse_run` reads `-r<n>` from 1, and refuses a bare config: it is no run's name any more. It
   returns a `Result`, and so does `run_dir`; their callers say the error (a player `lc0-psq-0001`
   is refused with what to type: `lc0-psq-r1-0001`).
3. `is_run_suffixed` unchanged: a config may still not end in `-r<digits>`.
4. Tunes of run 1's networks not finished: their `spec.txt`'s `player` line rewritten by hand (on this
   machine and on Drive), a `sed` given in the plan's checks; none is expected.
5. DEV.md's examples renamed (`pesto-psq-r1-0001`); its run 1 note removed.

## Plan

1. `players.rs`: `run_name`, `parse_run`, `run_dir` as above; tests: `-r1` round-trips, a bare
   config refused, `-r0` and `-r01` refused.
2. Callers in `smithy.rs`, `archive.rs`, `main.rs`, `match_player.rs`, `nnue_bench.rs`: the `Result`
   passed on or said.
3. DEV.md.
4. Checks: `cargo test`; `smithy start --dev`, the page lists `pesto-psq-r1`, Resume works; on the training machine
   after the pull, Resume of lc0-psq run 1 continues its training; any unfinished run-1 tune found
   and its `spec.txt` fixed.
