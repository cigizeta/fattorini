# net.yaml drives generation directly: no more "teacher"

**Archived: `rotor-curate` is gone**, and `preprocessing:` is the transformer track's alone
(`pgn`, `depth`); `lambda`, `pv` and `parent` are refused there (rotor-train-generations.md). The
principle survives as `run_config.txt`'s frozen fields and datasets named after their labeller.

Supersedes `rotor-curate-teacher-config.md`. That plan tried to fix a real gap (`rotor-curate
generate` had no tie to `data/teachers/<name>/teacher.yaml`'s `pgn_files`) by giving `generate` a
teacher name to resolve. Discussion surfaced a bigger problem: `depth` (and the future `pv`)
change what a database's scores actually *mean*, but nothing tied that parameter to the data it
produced -- a teacher name could silently drift from the depth it was actually built at, and once
a PGN file can have several databases (one per depth), "the db for this teacher" stops making
sense as a single path. The resolution: there is no more "teacher." `net.yaml`'s own
`preprocessing` section carries everything directly -- `lambda`, `depth`, `pv`, and `pgn` (a list
of bare PGN stems) -- so the parameters that define a database can never be named separately from
the database itself.

## Settled schema

```yaml
preprocessing:
  lambda: 0.0
  depth: 9
  pv: 2        # or ~ (null) when not applicable -- e.g. depth: 0 has no search, no PV to take
  pgn:
    - LumbrasGigaBase_OTB_ELO2400   # bare stem, no .pgn extension
```

`net_config.rs`'s `Preprocessing` struct becomes:

```rust
pub struct Preprocessing {
    pub lambda: f64,
    pub depth: u32,
    pub pv: Option<u32>,
    pub pgn: Vec<String>,
}
```

**`pv > 1` is not implemented yet.** The quiet-flag work just landed only checks a single
`best_move` (`is_position_quiet`, pv-depth-1 semantics) -- there is no code path that walks
further into `IterationReport::pv`. Per the developer: both `generate` and `prepare` panic
outright if `pv.unwrap_or(0) > 1`, rather than silently building a database whose filename claims
a pv depth it doesn't actually implement. `net_transformer.yaml`'s example `pv: 2` documents the
target shape; a real run at that setting waits until the N-ply check exists.

## Shared module: `src/bin/preprocessing_config.rs`

`#[path]`-included into both `rotor_curate.rs` and `rotor_train/main.rs`, the same mechanism
`pgn_replay.rs` already uses -- one canonical source compiled into both binaries, not two copies.
`net_config.rs`'s own `NetConfig.preprocessing` field uses this shared `Preprocessing` type
directly rather than redefining it.

```rust
pub struct Preprocessing { pub lambda: f64, pub depth: u32, pub pv: Option<u32>, pub pgn: Vec<String> }

/// Parses just a net.yaml's `preprocessing:` section, ignoring `architecture`/`training` entirely
/// (serde ignores fields a struct doesn't declare) -- rotor-curate never needs to know those
/// sections exist, let alone link anything architecture-specific.
pub fn load_preprocessing(net_yaml_path: &str) -> Preprocessing;

/// data/networks/{name}/net.yaml -- replaces main.rs's existing network_config_path, unchanged
/// behavior, moved here so rotor-curate can resolve the same bare network name rotor-train does.
pub fn net_config_path(name: &str) -> String;

/// data/games/{stem}.pgn -- replaces game_pgn_path; renamed since pgn entries are now bare stems.
pub fn pgn_file_path(stem: &str) -> String;

/// "depth{D}" or "depth{D}.pv{P}" -- the one place this naming convention is spelled out. In
/// practice only two shapes are ever produced today: "depth0" (pv: null) and "depth9.pv1"
/// (pv: Some(1)), since pv > 1 panics before any path is ever built from it.
fn generation_suffix(depth: u32, pv: Option<u32>) -> String;

/// data/games/{stem}.{suffix}.db
pub fn database_path(stem: &str, depth: u32, pv: Option<u32>) -> String;

/// data/games/{stem}.{suffix}/positions
pub fn positions_dir(stem: &str, depth: u32, pv: Option<u32>) -> String;

/// data/games/{stem}.{suffix}/labels/lambda-{value}  (format_lambda's existing fixed-precision
/// convention, moved here too)
pub fn labels_dir(stem: &str, depth: u32, pv: Option<u32>, lambda: f64) -> String;
```

## `rotor-curate generate`

CLI: `rotor-curate generate --threads <n> <network-name>`. Drops `--depth` (now read from
`net.yaml`); `--pv` never existed as a flag.

- Resolve `net_config_path(network-name)`, `load_preprocessing`, panic if `pv.unwrap_or(0) > 1`.
- For each stem in `preprocessing.pgn`: resolve `pgn_file_path(stem)` and
  `database_path(stem, depth, pv)`, then build/resume that **one file's own** database.

This is the pleasant surprise in this redesign: since every database is now single-PGN-file by
construction, all of today's `run_generate_core`/`open_database`/`read_resume_offset`/
`drain_ready_results` machinery is *already* exactly the right shape -- single `path`/
`database_path` pair, single `offset` resume column, no concatenated-multi-file replay, nothing to
add. Only `run_generate`'s own CLI parsing changes (teacher name/flags -> network name, resolved
via `preprocessing_config`), wrapped in a loop over `preprocessing.pgn`'s stems, calling the
existing per-file core once per stem.

## `rotor-train prepare`

CLI: `rotor-train prepare <network-name>`. Drops `--lambda` (now read from `net.yaml` alongside
everything else).

- `load_preprocessing`, same `pv.unwrap_or(0) > 1` panic (idempotent guard in case `prepare` ever
  runs against a `net.yaml` `generate` hasn't touched).
- For each stem in `preprocessing.pgn`: open that stem's own `database_path` connection, read its
  own `game_summaries`, build/resume that stem's own `positions_dir`/`labels_dir`.

Same simplification as `generate`: `length_cache::build_positions_cache`/`build_labels` shrink
from `pgn_paths: &[String]` back to a single `pgn_path: &str` -- the whole `PositionsResumeState`/
`LabelsResumeState` `pgn_index` field, and the "advance to the next file on EOF" loop in both
builders, is dead code once every build is single-file. Remove it rather than leave it unused.

## `rotor-train prepare`: self-play sources

**Superseded, 2026-09-22: `preprocessing.self_play: Vec<SelfPlaySource>` (this section's own
original design, each entry an explicit `{stem, timestamp}` pair naming one exact segment) was
replaced by a single `preprocessing.parent: Option<String>` field -- a machine-hostname-derived
`stem` and a per-segment `timestamp` were never meaningful to reference from a checked-in config
(decided directly with the developer: configs are real source code). `parent` names which
eval/network produced the self-play games (`hand-crafted` for today's only one) instead of one
exact file; every `.pgn.zst` segment under `data/self/{parent}/` is discovered fresh by
`self_play_segments` each time `prepare`/`train` runs, so a config stays correct as more segments
accumulate later, no edit needed. Current source of truth: `preprocessing_config.rs`'s own doc
comments (`Preprocessing`, `self_play_segments`, `self_play_pgn_path`/`self_play_positions_dir`/
`self_play_labels_dir`) and `prepare.rs`/`train/main.rs`'s `stem_dirs`. The rest of this section
below describes the original, now-replaced design and is kept only for history.**

A second, parallel source `prepare` also builds caches from -- `net.yaml`'s `preprocessing.
self_play: Vec<SelfPlaySource>` (`preprocessing_config.rs`, `#[serde(default)]` so an existing
`net.yaml` without this field still parses), each entry an explicit `{stem, timestamp}` pair
(never a single combined string split back apart: a stem can itself contain dashes, so only the
fixed-width timestamp would make that split unambiguous). `run_prepare` loops over
`preprocessing.self_play` exactly the way it already loops over `preprocessing.pgn`, one call per
entry, no CLI flags of its own -- decided directly with the developer, after first considering a
separate per-file `--stem`/`--timestamp` CLI invocation and settling on the net.yaml-listed,
auto-looped shape instead so one `prepare <network-name>` call still builds everything a network
needs.

Genuinely a different path from the curated-PGN one above, not a variation of it:

- **No database, no `rotor-curate generate` step at all.** A self-play segment's own PGN
  (`rotor-self`'s own output, `data/self/hand-crafted/{stem}-{timestamp}.pgn.zst`)
  already carries a `{score}` comment on every move it played (`self_play.rs`'s own
  `pgn::format_game` call, `include_depth_and_time = false`) -- there is nothing to re-derive via a
  fresh depth search the curated path's database exists to hold.
- **No `depth` in the path convention.** `self_play_positions_dir`/`self_play_labels_dir`
  (`preprocessing_config.rs`) resolve to `data/caches/hand-crafted/{stem}-{timestamp}/...` with no
  `depth{D}` component: there is no `prepare`-chosen depth here, the embedded score already fixes
  it, whatever depth self-play itself searched at.
- **A new `Visitor`, not `pgn_replay::GameCollector`.** `length_cache.rs`'s own
  `SelfPlayGameCollector`/`CollectedSelfPlayGame`, kept apart from the shared `GameCollector`
  (`pgn_replay.rs`'s own doc comment: shared by `rotor-curate` and `rotor-train`'s curated-PGN
  path, neither of which ever reads a self-play PGN) since a self-play PGN needs two things an
  ordinary curated PGN never has: its own `[FEN]`/`[SetUp]` starting position (self-play games
  don't all start from the standard position, `pgn.rs`'s own convention) and a per-move `{score}`
  comment, read via `pgn_reader::Visitor::comment`.
- **Mover-relative to White-relative, by parity, not by replay.** Each move's own comment is in
  the searching side's own perspective (Rotor's negamax convention), but `compute_labels` needs
  White-relative scores. Side to move strictly alternates every ply in standard chess, so which
  color searched ply `i` is already known from the starting position's own side to move and `i`'s
  parity alone -- no need to replay the game just to answer that (a real replay still happens
  once, afterward, to pair each already-computed label with its own board for `extract_canonical`
  -- unavoidable there, since a board only exists by actually making each move in order).
- **Keeps every position, no `is_trainable` filter.** "We do not throw away anything" -- the
  developer's own words. Every position that has a real self-play-searched score gets written,
  unconditionally; instead of excluding a would-be-saturating label the way the curated path's
  `is_trainable` does, each computed label is clamped to `[-(1 - CLAMP_EPS), 1 - CLAMP_EPS]`
  (`labels.rs`'s own `CLAMP_EPS = 0.01`, made `pub(crate)` for this reuse) before being written.
- **Restart from scratch on interruption, no partial resume.** A `.pgn.zst` segment is one
  complete, independent zstd stream -- not cheaply seekable to an arbitrary byte offset and
  resumed mid-decompression the way the curated path's plain-`.pgn` `open_pgn_reader_at` is. At
  this format's own scale (`CHECKPOINT_GAMES` self-play games per segment, not the curated path's
  multi-hundred-MB corpora), redoing one interrupted file's build from the beginning is cheap --
  decided directly with the developer. `build_self_play_positions_and_labels` is still a no-op,
  after printing as much, once both outputs already exist in full.

## `rotor-train train`: the read side becomes multi-file

A network's training corpus is the union, across `preprocessing.pgn`, of each stem's own
`positions_dir`/`labels_dir`. This is the one place real multi-file logic has to exist now --
everywhere else got simpler, this got a little more general:

- `open_readers(positions_dir, labels_dir, split)` today returns `Vec<LengthCacheReader>` of
  length `PIECE_COUNT_BUCKETS`. New: open one such vector **per stem** and flatten them into a
  single `Vec<LengthCacheReader>` of length `num_stems * PIECE_COUNT_BUCKETS` -- concatenation,
  not nesting.
- `pick_length_index`'s existing scheme already *is* a flat, weighted-by-remaining-count pick over
  a fixed-size `[u64; PIECE_COUNT_BUCKETS]` (`row_counts[i] - cursors[i]`, recomputed fresh every
  call, `length_cache.rs:244-254`). Generalizing to multiple stems needs no new algorithm, only a
  varying length: change `row_counts`/`cursors` from `[u64; PIECE_COUNT_BUCKETS]` to `Vec<u64>` (or
  `&[u64]`) sized `num_stems * PIECE_COUNT_BUCKETS`, matching the flattened `readers` above
  index-for-index. Picking proportional to remaining-count over this flattened space is exactly
  equivalent to "pick a piece-count bucket weighted by its total across stems, then pick a stem
  weighted by its own share" (the two are the same distribution by the chain rule) -- so there is
  no separate two-level pick to design, just the existing one-level pick over a longer vector.
- `RoundProgress.train_cursors` becomes a `Vec<u64>` instead of a fixed-size array -- a real
  on-disk format change (`ROUND_PROGRESS_MAGIC`/`_FORMAT_VERSION` at `length_cache.rs:917-918`):
  its `save`/`load` need a length prefix now, since the vector's length depends on how many stems
  a network has. Bump `ROUND_PROGRESS_FORMAT_VERSION` for this.
- `run_training_steps`/`run_test_pass` in `round.rs` change their `[u64; PIECE_COUNT_BUCKETS]`
  parameters to `Vec<u64>`/`&[u64]` and their `&mut [LengthCacheReader]` stays a plain slice
  (already length-agnostic, no change needed there beyond what `open_readers` now hands it).

## File/example fallout

- `examples/teacher.yaml` deleted -- no more teacher config format to document.
- `net_config.rs`: `TeacherConfig`/`load_teacher` deleted, `Preprocessing` now `use`s the shared
  module's type; test `load_teacher_parses_the_committed_example_teacher_yaml` deleted.
- `main.rs`: `teacher_config_path`/`teacher_database_path`/`teacher_name`/`game_pgn_path`/
  `positions_dir`/`labels_dir`/`format_lambda` and their tests replaced by the shared module's
  equivalents; `network_config_path` renamed to the shared `net_config_path`; `parse_lambda_flag`
  deleted (`prepare` no longer takes `--lambda`).
- `examples/net_naive.yaml`/`net_nnue.yaml` still have the old `teacher: lumbra-zero` / bare
  `lambda:` shape (only `zelda/net.yaml` and `net_transformer.yaml` were updated by hand so far) --
  update both to the new `preprocessing` schema.
- `data/teachers/lumbra-rubbish/` becomes unused. Flagged for the developer to remove by hand (no
  delete/Bash access here).

## Progress

Implemented:

- New shared `src/bin/preprocessing_config.rs` (`Preprocessing`, `load_preprocessing`,
  `net_config_path`, `pgn_file_path`, `database_path`, `positions_dir`, `labels_dir`), `#[path]`-
  included into both `rotor_curate.rs` and `rotor_train/main.rs`.
- `net_config.rs`: `Preprocessing`/`TeacherConfig`/`load_teacher` removed, `NetConfig.preprocessing`
  now uses the shared type.
- `rotor_curate.rs`: `generate` takes `--threads <n> <network-name>`, loops over
  `preprocessing.pgn`, panics on `pv > 1`. `run_generate_core`/its tests untouched (already
  single-PGN-file).
- `length_cache.rs`: `build_positions_cache`/`build_labels` back to single-`pgn_path` signatures,
  `pgn_index` removed from both resume states. Read side (`open_readers_for_stems`,
  `row_counts_of` -> `Vec<u64>`, `pick_length_index` -> `&[u64]`, `RoundProgress.train_cursors` ->
  `Vec<u64>` with a length-prefixed save/load, format version left at `1` per the developer) now
  spans however many stems a network names.
- `round.rs`: `run_round`/`run_training_steps`/`run_test_pass` signatures widened from
  `[u64; PIECE_COUNT_BUCKETS]` to `Vec<u64>`/`&[u64]`.
- `main.rs`: `prepare`/`train` both take a bare network name only (no more `--lambda`, no more
  teacher name); `run_prepare` loops over `preprocessing.pgn`; `run_train_nnue`/
  `run_train_transformer` build `stem_dirs` and read via `open_readers_for_stems`.
- `examples/net_naive.yaml`/`net_nnue.yaml` updated to the new `preprocessing` schema.
- Orphaned, uncompiled files (`nnue_round.rs`, `transformer_round.rs`, `transformer_window.rs`,
  `transformer_config.rs`) deliberately left untouched, as in the prior session.

Not done by this session (no Bash/delete access):
- `examples/teacher.yaml` is now unused -- ask the developer to delete it.
- `data/teachers/lumbra-rubbish/` is now unused -- ask the developer to delete it.
- Build/test not yet run -- next step.
