# "Run" renamed "attempt" for a training run

**Status:** designed 2026-11-19 (the developer), plan below; nothing written yet. Every layer: the
page, the code, the `claude/` documents from now on, and the names on disk. The model:
trial-rename.md.

## Why

A training run of the smithy is an **attempt** (the developer, 2026-11-19). "Run" stays for the
verb and for every other sense, so one word no longer means two things.

## The new names

- **`-rN` becomes `-aN`** in every attempt and network name: `lc0-halfk-threats-screlu-g1-a2`,
  `<config>-g<g>-l<lambda>-a<n>`. The old attempt part `-a<N>`, dropped from names on 2026-11-02, has
  no data left (the developer, 2026-11-19), so the letter is free: no old shape to tell apart, its
  remaining tests (`trials.rs` :658, :667) and the note at `players.rs:446` rewritten.
- **`run-N/` becomes `attempt-N/`** under `data/networks/<config>/`, locally and on Drive.
- **`run_config.txt` becomes `attempt_config.txt`**, its module `run_config.rs` becoming
  `attempt_config.rs` (`git mv`, the developer's).
- `data/networks` (`RUNS_ROOT`) keeps its name; the constant becomes `ATTEMPTS_ROOT`.

## Renamed

- **`players.rs`:** `run_name`, `parse_run`, `is_run_suffixed`, `run_dir`, `runs_of`, `runs_in`,
  `NetworkName::run_number` and `run()`, `run_winner`, the module docs, the errors ("no forge
  attempt", "`-a<n>`"), `numbered(run, 'r')` to `'a'`.
- **`smithy.rs`:** `run_dir`, `remote_run_dir`, `run_home`, `runs_of`, `known_runs`, `remote_runs`,
  `next_run_number`, `RUN_CONFIG` (the file stays `config.yaml`), `load_run_config`,
  `freeze_run_config`, `run_state`, `run_finished`, `run_winners`, `run_generations`,
  `run_lease_path`, `run_name`, `run_number`, `struct Run`, `RunEntry`; `job_name`'s "a new
  attempt", "a new attempt scaled"; log lines and errors; tests' names.
- **Interfaces:** the route `/view/<attempt>/<generation>`; the hash `#<config>/<attempt>`; the
  catalog's JSON `attempt`, `attempts`, `AttemptEntry`; `data-attempt`.
- **The page:** "Attempts", "New attempt", "New scaled attempt", "no attempt yet", the `attempt N`
  option, `currentAttempt`, `attemptWhere`, `attemptBlock`, `attemptScreen`, `findAttempt`,
  `attemptNumber`, `ofAttempt`, the `.attempt` and `.newattempt` classes, `step("attempt")`, the
  regexes `/-a\d+$/` and `/-a(\d+)$/`.
- **Elsewhere:** the arena's docs and tests naming `-rN` (`match_player.rs`, `snapshot.rs`,
  `main.rs`), `nnue_bench.rs`'s examples, `book_scores.rs`'s test, `archive.rs`, `lockstep.rs:180`'s
  file name, `net_config.rs`'s mentions, `main.rs:50`'s `mod`.
- **Documents:** DEV.md, the comments of `configs/` and `examples/engines.yaml`; CLAUDE.md where it
  means a training run. `claude/` documents keep their history as written, new text says attempt.

## Not renamed

- The verb: run a command, games, a job (`run_job`, `run_in_child`, `run_tune`, `run_buckets`,
  `Runs`, `running`, `isRunning`), `cargo run`, the arena's `runner.rs`.
- **`candidates.rs` and `lockstep.rs`'s `run_dir` (a lambda's directory) and `run_step` (an
  optimizer step):** another sense, left alone.
- A Convert's runs, the LICENSE, `.cargo/config.toml`.
- **Published history:** `RELEASE-NOTES.md`'s 2026.11.0 entry, the released binary's description,
  git history.

## The data on disk: a one-off migration

Too many files, a SQLite database and Drive's snapshots to rename by hand, so **`arena
rename-attempts`** (in the arena, not the smithy, 2026-11-19: the arena already links `rusqlite`, the
smithy would need it added), run by the developer with the smithy and every arena stopped:

- **A dry run by default,** printing every rename and every changed row; `--apply` to do it.
- **What it renames:**
  - `data/networks/<config>/run-N/` to `attempt-N/`, locally and on Drive; `run_config.txt` to
    `attempt_config.txt` in every lambda directory.
  - Every network name `-g<g>[-l<lambda>]-r<n>` to `-a<n>`, wherever stored: `trials.tsv`,
    `trial-NNNN.tsv` and `.pgn`; the arena's `results.db` (one transaction); PGNs; the directories
    `data/matches/<champion>-vs-<challenger>/`; `scores-<player>-...txt`; Drive's
    `data/snapshots/fattorini-<player>-...exe` and their `.search-parameters.txt`;
    `data/smithy-last.txt`. `round-robin.tsv` and `gate.tsv` hold indices, no names (checked).
  - An attempt name `<config>-r<n>` only for a known config (those on disk, `configs/`,
    `archived/configs/`), so Lc0's `training-run2-` and the like are never touched.
  - History rewritten whole (the developer, 2026-11-19), not only what the code reads.
- **Not touched:** lichess-bot's config, outside the repository: the developer points it at the
  renamed snapshot, the command printing the new name.
- **Idempotent:** a second pass finds nothing; run on every machine holding a `data/`, Drive's once.
- **Afterwards no dual reading:** the code reads `-aN` and `attempt-N/` only; an old name is refused,
  its message naming `--rename-attempts` (the arena's `rename-attempts`). The command removed once
  every machine has migrated.

## Plan

1. `git mv` of `run_config.rs` to `attempt_config.rs` (the developer), then the edits.
2. `players.rs` and its tests: the names, the suffix, the directories.
3. `smithy.rs`, `main.rs`, `page.html`, the arena crate, `book_scores.rs`, `archive.rs`,
   `lockstep.rs`, `net_config.rs`, `trials.rs`'s tests, `nnue_bench.rs`.
4. `--rename-attempts` with its dry run, and tests over a scratch tree and a scratch `results.db`.
5. DEV.md, `configs/`, `examples/`, CLAUDE.md.
6. `cargo test`, clippy; the developer: the dry run read, `--apply` on each machine and on Drive,
   lichess-bot's config, the smithy started, an attempt's page and a trial resumed.
7. TODO.md updated; the command removed in a later commit.

## Settled while writing

- `round-robin.tsv` and `gate.tsv` store indices and results, no names.
- `results.db` keys games by `game_id`; the players are plain text columns (`engine_one`,
  `engine_two`, `white`, `black`), updated in place.
