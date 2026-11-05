**Superseded, see rotor-preprocessing-without-teachers.md.** The `teacher.yaml`/`data/teachers/`
concept this document assumes is gone entirely: `depth`/`pv` turned out to belong to `net.yaml`
itself, not a separately-named config, and once that's true there's no more "teacher" left to
config. The per-PGN-file database idea below was right, it just ended up attached to `net.yaml`
instead. No code was ever written from this plan. Ask the developer to move this file to
`claude/archived/`.

# rotor-curate generate: drive from a teacher config, not raw paths

## Problem

`rotor-train` already treats a teacher as `data/teachers/<name>/teacher.yaml` (`pgn_files: Vec<String>`,
replayed as one concatenated stream) plus `data/teachers/<name>/teacher.db` (`net_config.rs`'s module doc
comment, `main.rs`'s `teacher_config_path`/`teacher_database_path`). `rotor-curate generate`, the tool that
actually produces `teacher.db`, does not know about any of this: its CLI is
`generate --threads <n> [--depth <n>] <pgn-file> <database-path>`, two free-form paths with no tie to
`teacher.yaml` at all. Nothing enforces that the database lands at the right path, that it was built from
the PGN list `teacher.yaml` declares, or even that it was built from more than one PGN file --
`teacher.yaml.pgn_files` is a list, but `generate` only ever accepts one `<pgn-file>`.

## Design

### CLI

```
rotor-curate generate --threads <n> [--depth <n>] <teacher-name>
```

Drops the `<pgn-file>`/`<database-path>` positionals. `<teacher-name>` resolves to
`data/teachers/<teacher-name>/teacher.yaml` (read for `pgn_files`) and `data/teachers/<teacher-name>/
teacher.db` (the output), via the shared helpers below.

### Shared teacher-config module

`TeacherConfig`/`load_teacher` currently live only inside `rotor_train`'s own private `net_config.rs`, and
`teacher_config_path`/`teacher_database_path`/`teacher_name`/`game_pgn_path` only inside `rotor_train`'s
`main.rs` -- none reachable from `rotor_curate.rs`, a separate binary crate. Move all of them into a new
file `src/bin/teacher_config.rs`, `#[path]`-included into both binaries exactly the way `pgn_replay.rs`
already is (`#[path = "teacher_config.rs"]` from `rotor_curate.rs`, `#[path = "../teacher_config.rs"]` from
`rotor_train/main.rs`) -- one canonical source compiled twice, not two copies to keep in sync by hand:

- `pub struct TeacherConfig { pub pgn_files: Vec<String> }`
- `pub fn load_teacher(path: &str) -> TeacherConfig`
- `pub fn teacher_config_path(name: &str) -> String` (`data/teachers/{name}/teacher.yaml`)
- `pub fn teacher_database_path(teacher: &str) -> String` (`data/teachers/{teacher}/teacher.db`)
- `pub fn teacher_name(config_path: &str) -> String`
- `pub fn game_pgn_path(filename: &str) -> String` (`data/games/{filename}`)

`net_config.rs` drops its own `TeacherConfig`/`load_teacher`, using the shared ones via `use` instead.
`main.rs` drops its own four duplicated helpers the same way. No behavior change for `rotor-train`'s own
callers (`run_prepare`, `positions_dir`, `labels_dir`, tests) -- purely a relocation.

`pgn_replay.rs` (already shared) gains two more functions, moved verbatim from `length_cache.rs`, since
`rotor-curate generate` now needs the identical seek-before-wrap semantics `length_cache.rs`'s own doc
comment already got right once:

- `pub fn open_pgn_reader(pgn_path: &str) -> Reader<StdFile>`
- `pub fn open_pgn_reader_at(pgn_path: &str, offset: u64) -> Reader<StdFile>`

`length_cache.rs` switches to calling the shared versions instead of its own private copies (no behavior
change).

### Multi-file resumable replay in `rotor-curate generate`

`run_generate_core`'s signature changes from `(path: &str, database_path: &str, threads, depth)` to
`(pgn_paths: &[String], database_path: &str, threads, depth)`.

**Resume state.** The `resume` table's schema changes from a single `offset` column to a `(pgn_index,
offset)` pair -- a clean break, not a migration: `CREATE TABLE IF NOT EXISTS` never alters an existing
table's columns anyway, and no real teacher database has been generated yet under even the current
(pre-this-change) schema, so there is nothing in the wild to migrate.

```sql
CREATE TABLE IF NOT EXISTS resume (
    id        INTEGER PRIMARY KEY CHECK (id = 0),
    pgn_index INTEGER NOT NULL,
    offset    INTEGER NOT NULL
);
```

`read_resume_offset` becomes `read_resume_state(connection) -> (u64, u64)` (`(pgn_index, offset)`,
defaulting `(0, 0)` on a fresh database, same `.optional()` shape as today).

**Opening the reader.** Fresh run: `open_pgn_reader(&pgn_paths[0])`, unseeked. Resumed run:
`open_pgn_reader_at(&pgn_paths[pgn_index], offset)`, exactly mirroring `length_cache.rs`'s own
`build_positions_cache`.

**Advancing across file boundaries.** The main read loop's `Ok(None)` case (today: end of file, stop
reading) becomes: advance `pgn_index`, and if `pgn_paths.get(pgn_index)` exists, `open_pgn_reader` on it
(fresh, unseeked -- a resumed run only ever seeks the *first* file it reads this invocation) and keep
going; if not, this is the real end of input (every file in the teacher's list exhausted) and the loop
breaks exactly as today's top-level `Ok(None)` does. Unlike `length_cache.rs`'s `build_positions_cache`
(which panics on exhaustion, since it must hit a known `game_summaries.len()` target), `generate` has no
external target to hit -- running out of games across every file is its normal, expected end of run, not
an error.

**Threading `pgn_index` through results.** `GameTask` and `GameResult` each gain a `pgn_index: u64` field
alongside their existing `end_offset: u64`, threaded through `run_worker` the same pass-through way
`end_offset` already is (`GameTask::pgn_index` copied straight into `GameResult::pgn_index`, no worker
logic touches it).

**Writing the checkpoint.** `drain_ready_results`'s resume write becomes:

```sql
INSERT OR REPLACE INTO resume (id, pgn_index, offset) VALUES (0, ?1, ?2)
```

bound to `(result.pgn_index as i64, result.end_offset as i64)`.

**Progress bar total.** `count_games(path, starting_offset)` (one file, one offset) becomes a sum across
`pgn_paths`: for every file *before* `pgn_index`, add its own full game count (via today's `count_games`
with `starting_offset = 0`) to `total_games` only. For `pgn_paths[pgn_index]` itself, call `count_games`
with `starting_offset = offset` as today, adding its `total_games` to the grand total and its
`games_before_offset` to the grand `games_before_offset`. For every file *after* `pgn_index`, add its own
full game count to `total_games` only. `sequence`/`next_to_write`/`game_id` keep counting monotonically
across the whole concatenated stream, unaffected by file boundaries -- already true today for the
single-file case, no change needed there.

### Tests

Existing tests calling `run_generate_core` directly (temp PGN file + temp DB path) update to pass a
one-element `&[String]` in place of the single `&str` path. Add at least one test mirroring
`length_cache.rs`'s own `build_positions_cache_replays_multiple_pgn_files_as_one_concatenated_stream`:
two small fixture PGN files, confirm games from both land in the same database with contiguous
`game_id`s, and one resume test that crosses a file boundary (commit through the end of file 0, kill the
run, resume, confirm it picks up at the start of file 1 with the correct `pgn_index`/`offset`).

### Non-goals

- No auto-creation of `teacher.yaml` -- the user still writes it by hand, exactly as `prepare` already
  requires.
- No change to `--threads`/`--depth` themselves, or to anything in `play_game`/scoring.

## Progress

Not started.
