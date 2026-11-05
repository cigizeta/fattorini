# Network names: the implementation

Status: approved 2026-10-24; every step done on another machine the same day, the page (step 5) with steps 3
and 4, the docs (step 6) in DEV.md; then the generation folders unpadded (below), the one-time
command renamed `smithy migrate-names` (`migrate_names.rs`, replacing `rename_players.rs`), run on
another machine, the training machine and the Lichess machine, then deleted. Left: this doc and network-names.md move to `done/`.

**Run on another machine, 2026-10-24:** another machine's `data` had nothing to rename; Drive's 8 generation folders renamed
(generations 1 and 2 of `lc0-psq`, `lc0-psq-threats` and `lc0-psq-threats-pairwise`, and generation
1 of the retired `smoke/run-1/` and `gen-psq-smoke/run-1/`), 27 candidate records rewritten; no
`gen-NNNN/` left under any `run-<n>/` on Drive. The retired runs' `run_config.txt` still name
`gen-0001` in their text, a record of runs nothing reads, left as written. Then the training machine.

**Run on the Lichess machine, 2026-10-24, the last machine:** `data`'s 4 generation folders renamed, 27 candidate
records rewritten; Drive found done (0 and 0); no `data/tests/tests.tsv`. The command then deleted
(`main.rs`'s `mod`, arm and usage; `matches::HEADER` and `players::latest_attempt` private again).

**Only a generation's winner is tuned (the developer, 2026-10-24):** a candidate has no tune of its
own and no folder for one. The `parameters.txt` beside a candidate exists only when it is its
generation's winner, the same file, and is then the winner's tune; `RotorPlayer::tune_dir` is the
generation's `tune/` for both, and `tune_args` refuses a candidate as before.

**Over UCI (step 2, 2026-10-24):** the suffix cannot set a subprocess's parameters, since the
release `rotor` reads the `parameters.txt` beside its network by itself and plays pesto at the
defaults. So against an external engine the suffix must say what the subprocess plays: a tuned
network `@t1`, an untuned one or pesto `@default`, anything else refused; `@<cp>` in-process only. The scheme and its
decisions are network-names.md's; this is how and where.

## What is on disk today (survey on another machine, 2026-10-24)

- **Drive** (`G:/My Drive/rotor/data/`): no file holds a player's name. The runs' `chores.txt`,
  `state.txt`, `generations.tsv`, `config.yaml` and `corpus.txt` hold none; no `tests/`, no tune
  folder (`tune/`, `tune-<lambda>/`). Directories keep their layout (`run-1/gen-0001/attempt-1/
  lambda-0.03/`), names being derived from it, so **Drive needs no migration**.
- **another machine** (`data/`): no `tests/`, no tune; the `--dev` tree (`data/smoke/`) holds no name either
  (`candidate.txt` names a checkpoint path, not a player). **Nothing to migrate.**
- **the training machine, the Lichess machine:** not surveyed from here. the training machine keeps tests 1 to 4 in `data/tests/tests.tsv`,
  where the smithy ran them. No run has ended, so no tune of a run's network can exist yet
  (`tune_args` refuses one before the run's end); pesto's tune, if any, is named `pesto`, unchanged.

So the rename on another machine and Drive is the code alone; the only stored names are the training machine's (and perhaps
the Lichess machine's) `tests.tsv`, rewritten there by the step below when each machine is available.

## The code

0. **Done first, 2026-10-24 (the developer): `TaskKind::Game` renamed `TaskKind::SelfPlay`, `self`
   on the wire** (`protocol.rs`), so that the kind says what it is: the one kind whose two sides
   share a transposition table (one player against itself), every other kind's sides each keeping
   their own (`InProcessPlayer`). A protocol change: every arena restarted with the smithy. No file
   on disk holds a chore's kind, so nothing is migrated. The status unit stays `game`.
1. **`crates/rotor/src/pipeline/players.rs`:**
   - `network_player_name(run, generation, candidate: Option<(attempt, lambda)>)`: the winner
     `<config>-gN-rN`, a candidate `<config>-gN-aN-lX-rN`, built from the run's config and number
     (`parse_run`), the run last.
   - `parse_network_name` returns a `NetworkName { run, generation, candidate: Option<(u32, f64)> }`,
     parsed by the letters from the right: `-r<n>` last, then optionally `-l<x.xx>` and `-a<n>`
     (both or neither), then `-g<n>`; the config what is left, non-empty. Spelled canonically only
     (no leading zeros, two decimals), so a name has one spelling. The old forms (`-NNNN`,
     `-r1-0001-0.03`) are refused with a message giving the new form.
   - `run_network_path` takes the attempt from the name instead of `latest_attempt`;
     `latest_attempt` stays for `latest_trained_attempt` only.
   - `RotorPlayer::resolve_under`: a winner's tune folder `gen-NNNN/tune/` as today; a candidate's
     `gen-NNNN/attempt-N/lambda-X/tune/`, beside its network (none exists yet, so nothing moves).
   - Module doc and tests rewritten to the new form, with refusals of the old ones.
2. **`crates/arena/src/match_player.rs`:** `split_suffix` reads `@default` and `@t1` (the network's
   own `parameters.txt`, an error if it has none); `@t2` and above refused (one tune a network,
   network-names.md, "Open"); `@<margin>` kept, arena's own. A network player without `@` refused,
   the suffix being always written. Pesto's `@` likewise required, `pesto@default` or `pesto@t1`,
   for one rule (the developer, 2026-10-24); engines unchanged. `check_engine_name`'s comment updated.
3. **`crates/smithy/src/smithy.rs`:** `test_networks` lists every attempt's candidates, not the
   latest's alone; `NetworkEntry` gains `attempt`, and the winner's `(attempt, lambda)` for the page
   (decision 6); `tune_args` and the generations' list on the new names; tests' specs built with the
   `@` suffix.
4. **`crates/smithy/src/matches.rs`:** `Record`'s `challenger` and `champion` hold the full player,
   `@` included; the two parameters columns go. `tests.tsv`'s header changes accordingly.
5. **`crates/smithy/src/page.html`:** the cascade's names, `(aN, lX)` beside a winner, the tests'
   titles with the `@` part instead of `(default)` / `(tuned)`; a candidate picked by attempt too.
6. **Docs:** DEV.md's player names and examples; TODO.md from now on.

## The migration, once per machine

Chosen by the developer, 2026-10-24: a one-time subcommand, `smithy rename-players`, run on each machine with its smithy stopped: it
rewrites `data/tests/tests.tsv` from the old form (names `<run>-NNNN[-<lambda>]` and the two
parameters columns) to the new one, the candidate's attempt being the latest on disk, as the old
name meant. Idempotent: a file already in the new form is left alone and said so, so it is safe to
run on another machine (nothing there) and twice anywhere. Removed from the code once every machine has run
it, with a TODO entry until then.

Rejected: rewriting by hand (four lines on the training machine today, but the header and column folding are easy
to get wrong), and a reader of both forms (network-names.md: none).

## Generation folders unpadded (the developer, 2026-10-24, after the steps above)

`gen-NNNN/` beside `run-<n>/`, `attempt-<a>/` and `lambda-<x>/` is inconsistent: the generation's
folder becomes `gen-<n>/`, unpadded like the rest (`run-1/gen-1/attempt-2/lambda-0.03/`). Sorting is
the code's, numeric, as `run-10/` already is. Chosen over letters (`r1/g1/a2/l0.03/`) and padding
everything (`run-0001/`), the smallest rename.

**The code:** every `gen-{:04}` becomes `gen-{}` (`players.rs` three times, `smithy.rs`'s `Dirs` and
two more, `archive.rs`, `rename_players.rs`); the doc comments saying `gen-NNNN` and the tests' paths.
Readers parse `gen-` then a number already, so they read both forms; only the writing changes.

**On disk, survey of 2026-10-24:**
- Drive: `data/gen/<config>/run-1/gen-0001/` and `gen-0002/` of `lc0-psq`, `lc0-psq-threats` and
  `lc0-psq-threats-pairwise`; generation 1's 27 `candidate.txt` record their checkpoint's path, which
  names `gen-0001`. Read by nothing (the step is what is read), but a record that should stay true.
- Local, every machine: its runs' `gen-NNNN/` under `data/gen/`, and a run's self-play buckets under
  `data/buckets/<run>/gen-NNNN/`, disposable (a new build would delete the old-named folder as
  stale and scatter again; renaming it saves the scatter).
- Left alone: the layout with no `run-<n>/` level, Drive's retired `gen-psq-smoke/gen-NNNN/` and
  `gen-psq-medium/gen-NNNN/`, and the old pipeline's `data/networks/<name>/gen-NNNN/`. Retired runs
  already in `run-<n>/` (`smoke/run-1/`, `gen-psq-smoke/run-1/`) are renamed with the rest: telling
  them apart would cost code for nothing (decided while implementing, 2026-10-24).

**The migration:** folded into the one-time command, renamed `smithy migrate-names` since it is no
longer players alone: `tests.tsv` as before, then every `gen-NNNN/` of every run in `data/gen/` and
`data/buckets/`, locally and on Drive, renamed to `gen-<n>/`, and each `candidate.txt`'s checkpoint
path rewritten. Idempotent: a folder already unpadded is left alone; both forms of one generation
present is refused, for the developer to look at. Run once a machine, smithy and arena stopped; the
first machine to run it renames Drive, the others find Drive done.

**Order, since Drive is shared:** an old build reads `gen-NNNN/` on Drive, so once Drive is renamed
no old smithy may run anywhere. Each machine: stop its smithy and arena, pull, build, `smithy
migrate-names`, then start. the training machine's runs in progress resume from their renamed folders.

## Tests

`cargo test -p rotor players`, `cargo test -p arena match_player`, `cargo test -p smithy`: names
built and parsed round trip, old forms refused, `@t1` without a tune refused, a candidate of an
earlier attempt resolved, `rename-players` on an old log, a new log, and no log.

## Order

1 and 2, their tests; then 3 to 5; then the subcommand; then the docs. After approval, on branch
`dario`.
