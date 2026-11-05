# The forge clean-up: `smithy server --forge` replaces the old pipeline

Status: design agreed with the developer on 2026-10-22, on branch `dario`. Steps 1 to 5
implemented the same day (TODO.md); left, step 6 (the docs) and the real run of
`lc0-psq-direct-fine` on the training machine, now a forge started from the smithy's page.

## Goal

Retire the single-run `smithy forge` and the generation pipeline built around it, and serve every
training through one command, `smithy server --forge <config>` (today's `smithy gen-server`, renamed as
lan-only-servers.md names it). The new code reuses the old code: `gen-server` trains with forge's
`train_in_lockstep`, exports with its `export_best`, and reads its candidates through the generation
layout. So the deletion goes **by reachability, not by file**: whatever the surviving entry points still
reach stays, wherever it lives.

`main` holds the state before the clean-up (merged 2026-10-22), with a working old `forge`.
`lc0-psq-direct-fine` runs from `main`, not from `dario`.

## Progress

- **Step 1, done 2026-10-22:** `lockstep.rs`, `candidates.rs`, `rounds_display.rs` and `install.rs`
  extracted from `main.rs`, behaviour unchanged, 384 smithy tests passing on another machine.
- **Step 2, in progress:**
  - Done: `smithy gen-server` renamed `smithy server start --forge` (`gen_server.rs` moved to
    `server.rs`), still in the foreground. Lc0 runs: one generation, starting at Train on
    `data/lc0`, its test set the first `test_positions` of `data/lc0/test`, its labelling from its
    network config, ending at the round robin with its winner in `generations.tsv` and the phase
    `done`; never archived. Player names for server runs in the shared
    `rotor::pipeline::players`, which arena and `smithy install <player-name>` both read. Configs
    `lc0-psq-direct-fine`, and `gen-lc0-psq-smoke` on the tiny network `lc0-psq-smoke`.
  - **The checkpoint is not passed yet:** the smoke run trained on another machine's CPU to round 6 and was
    stopped (2026-10-22). To run on the training machine: `gen-lc0-psq-smoke` to its end, then `arena match` on
    its winner by name; if it passes, `lc0-psq-direct-fine` through the server, otherwise through
    the old forge from `main`.
  - Done 2026-10-22: `check`, `calibrate` and `correlate` on server runs (plan below, "Step 2:
    inspectors on runs"), tests passing on the Lichess machine; not yet tried on a real run's checkpoint.
  - Done 2026-10-22, tests passing on the Lichess machine, no real run yet: the transformer in the server (plan below, "Step 2: the
    transformer in the server"), config `gen-lc0-alpha-pieces`. Before that, the left item was the
    streamed transformer in the server (refused there for now, the old forge its only
    path), which must come before the old forge is deleted.

### Step 2: inspectors on runs (agreed 2026-10-22)

- `smithy check|calibrate|correlate <name> <step> [--lambda <l>]` keeps its shape. `<name>` is a server
  run when `data/gen/<name>/` exists; otherwise an older network, read as before until step 3.
- A run's inspected attempt: the latest attempt of its latest generation that has one
  (`players::latest_trained_attempt`), the same rule `<run>-NNNN-<l>` follows. `--lambda` is required
  (the grid has 9) and must be one of that attempt's `lambda-<l>/`; the step resolves through
  `resolve_checkpoint` as before. The network config is the run config's `network:`.
- The test set is the one the server trained against, read through one function in `server.rs`
  (`test_set_files`, `read_test_set`) that `Run::train` also calls: Lc0's first `test_positions` of
  `data/lc0/test`, labelled as the network config says, or the generation's self-play `test/`, blended
  directly. Held in memory, as `RecordsSource`.
- The three inspectors share one loader (`sweep_checkpoint`): network built, checkpoint loaded, the
  sweep and its printed line passed in. A transformer is refused, as before.
- No `--generation` flag; older generations are read from their `history.tsv`.

### Step 2: the transformer in the server (agreed 2026-10-22)

**The transformer is one architecture among the others (the developer):** the same grid, test set,
config keys and training rules; MCTS will one day let it play. Until then:

- `Run::train` shares everything up to the network: corpus, test set, lambdas, patience. Only the
  batch builder (`transformer_batch`), the stream (`BucketedTrainStream`, one buffer per piece count)
  and the stacked network (`StackedTransformer`) differ, chosen by `architecture.kind`.
- **The test set is grouped by piece count for every architecture** (`GroupedTestSet`), as the old
  forge did: a transformer batch holds one length, and every architecture is judged alike.
- A transformer's run goes from `train` to `done`: no round robin, no winner in `generations.tsv`,
  since the engine cannot play it. `round_robin_pairs` stays required, for the day it can.
- A self-play run of a transformer is refused: its games could not be played.
- **Step 3, written 2026-10-22, to build:** every entry point of the table below removed from both
  binaries' dispatch and usage, and the functions only they called: arena's `select`, `challenge`
  and their pairing code, smithy's `forge`, `stages`, `promote`, `upload` and the old track's
  helpers. The inspectors read server runs only, `freeze` going. The two lockstep tests that ran
  through the old forge now call `train_in_lockstep` as the server wires it (`lockstep.rs`). Leaf
  modules unhooked, their files left for `git rm`: arena `self_play_command.rs`; smithy
  `stream_bench.rs`, `teacher.rs`, `buckets.rs`, `regroup.rs`, `loop_server.rs`, `loop_trainer.rs`.
- **Step 4, under way 2026-10-22:** arena's generator half of `self_play.rs`, the selection's
  config, round robin and files gone; smithy's on-disk caches gone whole (`reader`, `framed`,
  `dataset_cache`, `dataset_paths`, `length_cache`; `freeze` came back, below), the labelling of
  scored games and `RoundProgress` moved to `game_labels.rs`, `round.rs`'s tests on in-memory
  records. Found on the way: the server's test set kept K vs K, which the grouped test set refuses;
  `read_test_set` now leaves it out, as `freeze` did.
- **Step 4 builds clean, 2026-10-22,** the frozen test set restored (below); the tests pass. Left:
  `rotor::pipeline`'s public items nothing calls any more, which the compiler cannot report.
- **Step 3b done 2026-10-22, builds clean, tests pass:** `Labelling` gone from `rotor` and smithy, `labels.rs`
  down to the direct blend, the loader refusing `blend`/`cut`/`cut_drop` (also from
  `net_config::load`), the six configs that set them converted.
- **Step 5 done 2026-10-22, builds clean, tests pass:** `gen_sprt.rs` renamed `sprt.rs`; `generation_files.rs`
  folded into `candidates.rs`. Kept by name: `--forge`, the `gen-*.yaml` run configs and `data/gen/`
  (the run name names its directories and players), and `loop_protocol`, `LOOP_PORT`, `loop_client`,
  `arena loop-client` (renamed once, by lan-only-servers.md). The comments that still say "forge"
  go with step 6. Configs nothing reads any more (`loop-psq*.yaml`, `self.yaml`) carry a RETIRED
  header (the developer, 2026-10-22: each config will be reviewed one by one, none deleted here).
- **Step 6 done 2026-10-22, builds clean, tests pass:** DEV.md's pipeline sections rewritten around the server,
  README's labels, TODO.md's clean-up item and a note on every older item naming retired commands;
  about 35 code comments. `rotor::pipeline`'s dead code (the developer approved): `self_config.rs`
  and `self_training.rs` gone; `generation.rs` down to the paths and readers the player names use;
  `generation_sync.rs` down to the pulls; `remote.rs` without the move, the replacing copies and the
  pointer. Kept, cheap and tested: `players::network_player_name`, `lan::request_text`,
  `load_lambdas`, `remote::is_segment_file`. Configs untouched beyond the RETIRED headers.
- **Then:** the real run of `lc0-psq-direct-fine` through the server on the training machine.

### Step 4: the server freezes its test set (agreed 2026-10-22)

**The test set is frozen once and never recomputed (the developer):** step 4 had dropped `freeze`,
and the server read its test set from the first test files every time, with no sampling. Restored,
as code the server calls rather than a command:

- `freeze.rs` keeps its sampling: Algorithm R under `FREEZE_SEED` with the hashed draw, K vs K never
  offered to the reservoir, every position of every test segment read. Its files: `manifest.txt`
  (seed, games, positions, rows, the segments read), `test.positions`, one `<label set>/test.labels`
  with its own `manifest.txt` per lambda. Frozen means frozen; a lambda the set lacks is added by
  reading the manifest's segments again under the same seed. The cut tally goes, with the cut (3b).
- **No `smithy freeze`:** `Run::train` freezes, or adds the lambdas the set lacks, before it trains;
  training and the inspectors then read only the frozen files. The inspectors never freeze: a run
  whose set is missing is an error.
- **Where:** an Lc0 run's set is `data/lc0/frozen` on the remote, as before, shared by every Lc0 run;
  a self-play generation's is `gen-NNNN/frozen`, beside the test games the server played for it.
- **Size:** the run config's `test_positions`, recorded as the manifest's rows; a set frozen at
  another size is refused.
- Written straight into its directory, each manifest last as the completion marker: no local copy
  and upload. Read as one zstd stream, so the sets frozen in framed files read unchanged.
- `cache.rs` gets back the position and label formats; `framed.rs` and `reader.rs` stay deleted.
  `first_files` and `read_test_set` go; `open_one_pass` is left to tests.

## Decisions (2026-10-22)

1. **Lc0 mode.** `smithy server --forge <config>` whose network config says `labels: lc0` runs one
   generation without a play phase: the grid trained in lockstep on `data/lc0`, judged on its test
   split, then the round robin through the LAN clients at depth 9, as `gen-server` plays it; the run
   stops at the winner. The duel against a reference (`lc0-psq-0001-0.15`) stays a manual
   `arena duel ... --time 10+0.1`: pair jobs carry no time control until lan-only-servers.md's orders.
2. **Player names for server runs.** `<run>-NNNN-<lambda>` is the candidate of generation NNNN's latest
   attempt at that lambda, `data/gen/<run>/gen-NNNN/attempt-<a>/lambda-<l>/candidate.nnue`;
   `<run>-NNNN` is that generation's round-robin winner. The existing `data/networks/<name>/gen-NNNN/`
   players (`lc0-psq-0001-0.15`, `caroline-0001`, ...) stay playable, **read-only and local only**: no
   Drive pulls. A run whose name is already a `data/networks/` network is refused, so a name never
   means two players.
3. **The transformer.** A transformer config in Lc0 mode trains and stops: the engine cannot run it,
   so it has no round robin. The archived path (`labels: archives`, `smithy regroup`, the length files,
   `BucketStream`) goes; the streamed transformer stays.
4. **`check`, `calibrate`, `correlate`** stay, pointed at server runs:
   `smithy check <run> <step> [--lambda <l>]`, on the run's test set.
5. **`smithy install <player-name>`** installs any resolvable Rotor player (or `hand-crafted`) from
   local files, with its `parameters.txt` when it has one. No Drive pointer, no `champion.txt`.
6. **Order:** first the new paths (rename, Lc0 mode, names, install), then the deletion. **Revised
   2026-10-22 (the developer):** the real run no longer gates the deletion; the whole clean-up is
   implemented first, and `lc0-psq-direct-fine` through the server is its one real check at the end.
   `main` keeps the old forge as the fallback should that run fail.

## Method

1. **Extract the shared core from `main.rs`** (3,416 lines) into modules of their own, behaviour
   unchanged, tests passing:
   - `lockstep.rs`: `train_in_lockstep`, `LockstepParts`, `Arm`, `open_arm`, `Stack`, `SlotFiles`,
     `StackedModel` and its two impls, `open_stack`, `replay_stages`, `stage_best_steps`,
     `recorded_rounds`, `roll_back`, `pieces_path`, `has_history_row`, `round_line`.
   - `candidates.rs`: `export_candidate`, `export_best`, `finish_training`, `write_engine_network`,
     `write_record`, `engine_runs`, `feature_set_of`, `resolve_checkpoint`, `PARAMETERS_FILE`.
   - `rounds_display.rs`: the redrawn round table and progress bars (`PassBar`, `RoundsOnScreen`,
     `print_above_rounds`, `print_progress`, the gauges and colours).
   - `install.rs`: `install` and its helpers.
   - `main.rs` keeps the dispatch and `USAGE` only.
2. **Add the new paths** on the extracted modules: `server.rs` (today's `gen_server.rs`, renamed, with
   Lc0 mode), the player names in `match_player.rs`, `install <player-name>`, the inspectors on runs.
   **Checkpoint: `lc0-psq-direct-fine` or a smaller Lc0 run trains, plays its round robin and names its
   winner through `smithy server --forge`; `arena duel` plays the winner by its new name.**
3. **Remove the old entry points:** the subcommands and their `match` arms (below).
3b. **Retire AB(lambda) and the cut (the developer, 2026-10-22): the direct blend is the only
   labelling.** Removed: `Labelling::Chain` and `Labelling::ChainCut`, AB(lambda) and the cut in
   `labels.rs`, and their branches and tests wherever step 4's warnings lead (`pgn_stream`,
   `pgn_train_stream`, `bucketed_train_stream`, `round`, and whatever of `freeze`, `length_cache` and
   `stream_bench` survives step 3). `label.blend`, `label.cut` and `label.cut_drop` go from the
   config: the loader refuses them by name, as `check_removed_fields` does. **No config is deleted:**
   every one is converted in place, its `blend:`/`cut:` keys dropped and its comments no longer
   describing AB(lambda) or the cut as its labelling; the networks already trained the old way stay
   playable from their `.nnue`, and their configs now train them anew with the direct blend. The
   self-play corpora need nothing: labels are computed from the games at load.
   **Agreed in detail 2026-10-22:** `Labelling` goes as a type, its parameter dropped from every
   signature; `self_play_game_labels` is the direct blend, and `self_play_game_labels_with`, the
   `played_is_best` flags and `mover_drops` go. `rotor::labels` keeps `compute_labels_direct`,
   `score`, `outcome_label`, `side_to_move_relative`; the recursion's tests that check what the
   direct blend shares (decisive scores, colour invariance) are rewritten against it. Frozen label
   sets keep their `lambda-<l>-direct` directories, so `data/lc0/frozen` stays readable.
   `label.lambda` stays until step 6.
4. **Let the compiler find the rest.** The developer runs `cargo build --release` and `cargo test` on
   any machine with the `.rotor` venv (another machine included: smithy builds and tests on the CPU) and pastes the `dead_code` and `unused` warnings; Claude deletes what they list, again until
   none is left. Whatever is still reached stays, whatever file it is in. Tests of deleted code go with
   it; tests of surviving code are kept even when they exercised it through an old command.
5. **Rename what survives** where its name still says `forge`, `generation_files`, `loop_` or `gen_`
   but now serves the server (`LOOP_PORT`, `loop_protocol.rs` waits for lan-only-servers.md).
6. **Docs:** README, DEV.md, `USAGE` and the module comments; TODO.md's items on the retired commands;
   the configs (`configs/*.yaml`) for keys nothing reads any more (`selection:`, `labels: archives`,
   `self.yaml`).

## Entry points removed

| Binary | Removed | Superseded by |
|---|---|---|
| smithy | `forge` | `server --forge` |
| smithy | `gen-server` | `server --forge` (renamed) |
| smithy | `loop-server` | `server --forge` (the continuous loop is retired) |
| smithy | `teacher`, `freeze`, `stream-bench`, `buckets` | the server's own play, test set and corpus |
| smithy | `stages` | nothing: no stage round robin since 2026-10-18 |
| smithy | `promote`, `upload` | the server's own promotion; no Drive |
| smithy | `regroup` | the streamed transformer |
| arena | `select`, `challenge` | the server's round robin and gate |
| arena | `self` | the server's self-play jobs |
| arena | ~~`replay`~~ kept (the developer, 2026-10-22: the server may get a replay mode) | -- |

Kept: smithy `lc0`, `check`, `calibrate`, `correlate`, `install`; arena `match`, `sprt`, `duel`,
`margin`, `spsa`, `book`, `loop-client` (until `arena client`), `<tournament-id>`.

## Inventory: what the survivors still reach

From the grep of 2026-10-22, to confirm by step 4's warnings.

- **`length_cache.rs` (1,553 lines) is split, not deleted.** The length files go, but `RoundProgress`
  (`run_config.rs`, `train_stream.rs`, `lockstep`), `BACKUP_INTERVAL`, `MIN_PIECE_COUNT` /
  `MAX_PIECE_COUNT` (`round.rs`, `transformer_network.rs`) and `self_play_game_labels` (`pgn_stream.rs`)
  are used by the streamed path. They move to a smaller module (`labels_stream.rs` or beside their users).
- **`dataset_cache.rs`:** `splitmix64` is used by `pgn_stream.rs` and `pgn_train_stream.rs`; it moves
  out, the rest (`Manifest`) goes with `regroup`.
- **`freeze.rs`:** kept, called by the server (see "Step 4: the server freezes its test set").
- **`generation_files.rs`:** `read_candidate_step` and `candidate_record_path` (the server's
  promotion and `export_candidate`) stay; `promotable_challenger`, `training_state_check`,
  `every_candidate_exported`, `starting_weights_path`, `champion_weights_path` go.
- **`rotor::pipeline::generation`:** `lambda_dir`, `candidate_network_path`, `generation_dir`,
  `network_dir`, `champion_network_path`, `champion_record_path`, `read_champion_lambda`,
  `read_selection_winner` stay for the read-only old players; teacher and pointer code goes.
- **`rotor::pipeline::generation_sync`:** goes whole once `match_player.rs` resolves locally only.
- **`rotor::pipeline::segments`:** `SegmentMeta`, `META_SUFFIX`, `day_dir` stay (`smithy lc0`, the
  server's corpus); `list_remote`, `read_remote_meta` go with Drive (see open question 1).
- **`rotor::pipeline::self_config`:** goes with `arena self` and `freeze`.
- **arena `self_play.rs`:** stays, `loop_client` plays self-play jobs through it; `segments::list_remote`
  and the remote paths in it go. `self_play_command.rs` goes.
- **arena `round_robin.rs`, `round_robin_interleaved.rs`, `selection_files.rs`:** used by `select`
  only, dead whole by step 4. **The ranking moves, not goes (the developer, 2026-10-22: the server
  needs a real round robin):** points, the head-to-head tie-break and each score's standard error go
  to smithy's `round_robin.rs`, which the server ranks with, the error now over pairs (the server
  records pairs, and a pair is the unit). Berger order, the interleaved schedule, the live table and
  the `Pairing` rule go: the server's pair jobs replace them.
- **`loop_server.rs`, `loop_trainer.rs`:** go whole; `loop_client.rs` stays, serving `server --forge`.
- **`archive.rs`:** stays (the server's archiver).
- **`remote.rs`:** what is left once nothing above uses it is a question for lan-only-servers.md's
  "Drive out of runs", not this clean-up.

## Open questions

1. **Lc0's corpus is on Drive.** `pgn_train_stream::training_files` lists the segments through
   `segments::list_remote`, under the remote root. Lc0 mode either keeps reading them from Drive's
   local mirror for now (Drive is a folder on disk, kept offline on every machine) or reads a local
   `data/lc0`. **Settled 2026-10-22:** the current read is kept in this clean-up; moving it is
   lan-only-servers.md's.
2. **Where a finished Lc0 run's winner is recorded,** so that `<run>-0001` resolves: a `winner.txt`
   beside `round-robin.tsv`, or `generations.tsv`'s last row. Suggested: the latter, which already
   records every attempt's winner. **Settled 2026-10-22:** `generations.tsv`'s last row.
3. **Settled 2026-10-22: a smaller Lc0 run first** as step 2's checkpoint, so a bug costs an hour and
   not a whole run; `lc0-psq-direct-fine` after it.

## Machines

Only step 2's checkpoint needs the training machine (the GPU); every other step builds and tests on another machine. The training machine is
free only later on 2026-10-22, and `lc0-psq-direct-fine` from `main` was planned for it then:
whichever runs first, the other waits.
