# Retire `rotor-curate`

Status: done. Applied, files deleted by the developer, and `cargo test --features training`
passes.

The first step of the dead-code cleanup, one module at a time. Beyond the plan below, the apply
also removed `pgn_replay::GameCollector`/`CollectedGame` (their only users were `rotor-curate` and
the database builders), the `#[ignore]` diagnostic test that read a curated cache, and the
resumable-build half of `PositionCacheBuilder`/`LabelCacheWriter` (`resume`, `checkpoint`, the
resume-state files), used only by the database builders. `search::is_quiet` lost its `pub`, which
existed only for `rotor-curate`. The three `build_self_play_positions_and_labels` tests that
checked labelling (score back to its mover, one outcome with opposite signs, mate notation) were
rewritten against `self_play_game_labels` directly rather than deleted.

## Why

`rotor-arena replay` supersedes `rotor-curate generate`: it searches sampled CCRL/TCEC chunks and
writes the scores into the games themselves (`ccrl-replay-sampling.md`, `replay-command.md`), and
`rotor-train prepare` builds its datasets from those outputs. The SQLite teacher databases
(`data/teachers/{stem}.depth{N}.db`) that `generate` wrote have no producer left worth keeping, and
no config reads them: `configs/` holds `amanda` (`naive`) and `betty` (`psq`), neither with a
`preprocessing.pgn`. The `self-play` and `book-info` subcommands are manual test drivers, superseded
by `rotor-arena`.

## Scope

This step removes the binary and the SQLite teacher path, its only data flow. It does not touch
the transformer's training side (see "Left for a later step").

### Deleted outright

- `src/curate.rs`, `src/bin/rotor_curate.rs`, `bin/rotor-curate`.
- `Cargo.toml`: the `[[bin]] rotor-curate` entry.
- `src/polyglot_book.rs`: its only user is `curate.rs` (`self-play`, `book-info`). The arena's book
  is not this module. Retired with the intention to restore it later (see "Restoring Polyglot
  support").
- `src/phase.rs` and `pub mod phase;` in `lib.rs`: already dead, no caller anywhere (the compiler
  never warned because it is `pub` in the library). Its doc comment names `rotor-curate` as a user.
- `src/train/dataset.rs`: the SQLite readers (`read_game_summaries`, `read_game_scores`,
  `prepare_game_scores_statement`, `GameSummary`) for the teacher databases.
- `src/train/builder.rs`, whole: `build_caches` (already dead), `count_rows_per_split`,
  `is_train_game` with `TRAIN_SPLIT` and `TRAIN_TEST_SPLIT_SEED`, and their tests.
- `src/train/length_cache.rs`: `build_self_play_positions_and_labels` (unused, kept only for a
  future transformer) and its tests. `self_play_game_labels`, `self_play_game_positions`,
  `label_and_featurize_self_play_game` and `SelfPlaySegmentReader` stay: `dataset_cache.rs` uses
  them.
- `src/pipeline/preprocessing_config.rs`: `self_play_positions_dir` and `self_play_labels_dir` and
  their test, unused for the same reason. `self_play_dir` stays: `rotor-arena self` uses it.
- `src/train/length_cache.rs`, the database-driven builders: `build_positions_cache`,
  `build_labels`, `build_positions_and_labels_together`, `build_positions_and_labels`, their resume
  states (`PositionsResumeState`, `LabelsResumeState`, `CombinedResumeState` and their magics and
  paths), `is_trainable`, `open_pgn_reader`/`open_pgn_reader_at` if nothing else uses them, and
  every test that opens a `Connection`. The reader side (`LengthCacheReader`, `open_readers*`,
  `row_counts_of`, `pick_length_index`, `LengthCacheWriter`, `RoundProgress`, the piece-count
  constants) stays: training reads it.
- `src/train/prepare.rs`: the `for stem in &preprocessing.pgn` loop, `CURATED_LAMBDA`, the
  `rusqlite`/`read_game_summaries` imports, and the paragraph about keeping
  `build_self_play_positions_and_labels`. `prepare` no longer reads `preprocessing.pgn` or `depth`
  at all: it always builds the replayed dataset of the network's current generation. The module
  and `run_prepare` doc comments are rewritten for that one path (no more length-bucketed caches
  per stem, no more "every architecture").
- `src/pipeline/preprocessing_config.rs`: `database_path` and `pgn_file_path` and their tests, and
  the `data/teachers/` mention in the module doc comment.

### Edited

- `src/curate.rs`'s `#[path]` exceptions go with it, so `data_root.rs` and `pgn_replay.rs` lose a
  crate that includes them. `pgn_replay.rs`: `try_resolve_san` is called by both `rotor-arena
  replay` and `rotor-train sample`, so its `#[allow(dead_code)]` and the sentence about
  `rotor-curate` go. The module doc comment names `rotor-train` and `rotor-arena` as its users.
- Comments that cite `rotor_curate.rs` or `rotor-curate` as a precedent (`labels.rs`,
  `arena/self_play.rs`, `arena/ctrlc_handler.rs`, `arena/pgn.rs`, `arena/results_db.rs`,
  `search.rs:680`, `train/length_cache.rs`, `train/net_config.rs`, `train/transformer_config.rs`,
  `pipeline/preprocessing_config.rs`): the "why" is inlined instead of pointing at a file that no
  longer exists. Where the precedent itself is the only content, the reference is simply dropped.
- `Cargo.toml` comments: `autobins` (drop `rotor-curate`), `rusqlite` (still needed by
  `rotor-arena`'s `results_db.rs`: reword around it), `tch`, `serde`/`serde_yaml`,
  `material-only-eval`.
- `README.md:218` and `CLAUDE.md`'s "Run the binaries through bin/" list.
- `claude/TODO.md`: the "Retire `rotor-curate`" item goes once this is done.

### Kept

- `rusqlite`: `rotor-arena` stores duel results in SQLite.
- `pgn_replay.rs`, `data_root.rs`: shared by `rotor-train` and `rotor-arena`.
- `eval::evaluate`: search and UCI still call it.

## Left for a later step

The transformer track still trains on length-bucketed caches keyed by `preprocessing.pgn` stems and
`depth` (`train/mod.rs`'s `stem_dirs`, `open_readers_for_stems`, `positions_dir`/`labels_dir`,
`net_config.rs`'s rule that a transformer must name `pgn`). After this step nothing can build those
caches. The transformer will be designed again from scratch when the time is right, so its current
code is not kept for a data source: retiring it is the next module of this cleanup, with its own
plan. That step takes `preprocessing.pgn`, `depth`, `positions_dir`/`labels_dir`,
`pgn_file_path`, the stem-keyed readers and the transformer rules in `net_config.rs` with it.
Noticed along the way: `transformer_round.rs` and `transformer_window.rs` look near-identical.

## Restoring Polyglot support

Polyglot support is retired, not abandoned. The module is self-contained (`.bin` loading, the
Polyglot Zobrist keys, `moves_at`, `select_move` with `MIN_PLY` and `SKIP_BOOK_PROBABILITY`) and
comes back from Git unchanged: it is last present at commit `08afda6`, as `src/polyglot_book.rs`,
with its unit tests. Its design history is `rotor-curate-polyglot-book.md` and
`rotor-self-play-diversity.md`, which stay in `claude/active/` with a note at the top saying the
module is retired and where to find it. `claude/TODO.md` gets an entry, "Restore Polyglot book
support", naming the commit, so the intention outlives this plan once it moves to `done/`.

What a restoration needs, beyond the file itself:

- A crate that includes it (`rotor-arena`, or the engine's UCI `OwnBook` option), since
  `rotor-curate` was its only one.
- The `book-info` diagnostic, if still wanted, as a subcommand of that binary.
- A check that its dependencies (`Board`, move generation, `rand`) still have the same API.

## Not in the code

The `data/teachers/*.db` files and the old files at the top of `data/games/`, locally and on
Drive, are the developer's to delete once this has landed.

## Settled

- `prepare` drops curated support entirely and never reads `preprocessing.pgn`: no error path for
  it, since the transformer step removes the field itself.
- `build_self_play_positions_and_labels`, `is_train_game` and the `self_play_*_dir` helpers go
  now, not kept for the transformer.

## Verification

- `cargo test` (all binaries, `--features training` for `rotor-train`).
- The dead-code check, to confirm the deletions left nothing orphaned:

      RUSTFLAGS="-W dead_code" cargo test --features training --no-run 2>&1 | grep -A3 "never used"

- `bin/rotor-train prepare amanda` still reaches the replayed path.
