# `prepare` builds positions.cache and labels in one PGN pass, not two

## Status

Implemented. Not yet built/tested by the developer (no Bash access from this session).

## Problem

`run_prepare` (`main.rs`) calls `length_cache::build_positions_cache` then `length_cache::
build_labels` for each stem. When both actually need building (the common case: a stem's first
`prepare`, for a network with a lambda no earlier network has used against this stem), each
function independently: opens its own `Reader` over the same PGN, replays every game from move 1,
runs the identical `dataset::read_game_scores` query per game, applies the identical `is_trainable`
filter, and calls `extract_canonical` on the identical board -- all of it duplicated, just to end
up writing to two different output files (positions.cache is lambda-independent; a lambda's own
label file is not, `length_cache.rs`'s own module doc comment). Confirmed by two independent
`indicatif` progress bars visibly running back to back during a real manual run.

## Design

### The two artifacts don't always need building together

Because `positions.cache` is shared across every lambda a stem's networks ever use, a stem
reaches four possible states, and only one of them is today's actual double-replay:

| positions.cache | labels (this lambda) | what to do |
|---|---|---|
| done | done | nothing -- already true today, both functions already no-op |
| done | not done | build labels alone -- already just one replay today (`build_positions_cache` is already a fast no-op) |
| not done | done | build positions alone -- symmetric, rare, but keep for correctness |
| not done | not done | **the real case this item targets** -- one combined replay |

So the fix is a **dispatcher**, not a rewrite of the existing single-purpose functions:
`build_positions_cache`/`build_labels` stay exactly as they are (every existing test keeps passing
unmodified) and keep handling the first three rows themselves. Only the fourth row gets a new,
genuinely combined function that writes both outputs from one replay.

### New public entry point

```rust
pub fn build_positions_and_labels(
    pgn_path: &str, connection: &Connection, game_summaries: &[GameSummary], lambda: f64,
    positions_dir: &str, labels_dir: &str,
)
```

`run_prepare` calls this once per stem instead of today's two calls. Internally:

```rust
match (positions_cache_is_complete(positions_dir), labels are already complete) {
    (true, true)   => print both-already-built messages, return,
    (true, false)  => build_labels(...),           // unchanged existing function
    (false, true)  => build_positions_cache(...),  // unchanged existing function
    (false, false) => build_positions_and_labels_together(...),  // new, below
}
```

### The new combined loop

Structurally the same shape as the two existing loops (open/resume a `Reader`, replay game by
game, `read_game_scores` once per game, `is_trainable` filter, checkpoint every `BACKUP_INTERVAL`,
finish), merged so each trainable ply computes `extract_canonical` **once** and feeds it to both a
`PositionCacheBuilder` and a `LabelCacheWriter` in the same iteration.

### Resumability: one merged resume file, not two independently-checkpointed ones

The risk to design around: if this combined build checkpointed `PositionsResumeState` and
`LabelsResumeState` as two separate file writes (even back to back, same instant), a crash between
the two writes leaves them disagreeing about `next_sequence` -- and resuming from either one alone
would replay some games twice into the *other* artifact, silently duplicating rows into an
already-append-only zstd stream or label file. That failure mode does not exist today (each
function only ever has its own single resume file to reason about).

**Fix**: a new `CombinedResumeState` struct, one file (`{labels_dir}/combined_resume.state`,
distinct from the plain `{labels_dir}/resume.state` `LabelsResumeState` uses, so the two can never
be confused), carrying everything both `PositionCacheBuilder::resume` and `LabelCacheWriter::resume`
need (their existing signatures are unchanged -- the combined function just constructs a throwaway
`PositionsResumeState`/`LabelsResumeState` from `CombinedResumeState`'s own fields to call them):

```rust
struct CombinedResumeState {
    pgn_offset: u64,
    next_sequence: u64,
    positions_train_row_counts: [u64; PIECE_COUNT_BUCKETS],
    positions_test_row_counts: [u64; PIECE_COUNT_BUCKETS],
    positions_train_file_lengths: [u64; PIECE_COUNT_BUCKETS],
    positions_test_file_lengths: [u64; PIECE_COUNT_BUCKETS],
    labels_train_row_counts: [u64; PIECE_COUNT_BUCKETS],
    labels_test_row_counts: [u64; PIECE_COUNT_BUCKETS],
}
```

One file, one write, one read -- the same crash-safety envelope every other resume state in this
module already has (a crash strictly mid-write leaves the old file intact and loadable; a crash
elsewhere leaves this file absent, meaning "resume from scratch," exactly like today). `save`/
`load`/magic bytes (`RTCS`, format version `1`) follow the same pattern as `PositionsResumeState`/
`LabelsResumeState` verbatim.

`LabelCacheWriter` itself doesn't track row counts (only `build_labels`'s own loop does, as two
local arrays) -- the combined loop keeps that same pattern locally for the labels half, and reads
`PositionCacheBuilder`'s own already-tracked `train_row_counts`/`test_row_counts` fields directly
for the positions half (same-module access, no new accessor needed).

## File-by-file plan

### `src/bin/rotor_train/length_cache.rs`
- Add `CombinedResumeState` (struct, `save`/`load`, magic/version consts, path helper).
- Add `build_positions_and_labels_together` (private): the merged single-pass loop described above.
- Add `build_positions_and_labels` (public): the four-way dispatcher `run_prepare` calls.
- `build_positions_cache`/`build_labels`: **unchanged**.
- New tests (existing test patterns/fixtures reused, e.g. `write_teacher_fixture`):
  - building both fresh together produces the same positions.cache/labels content the two
    separate functions would have (row counts, bucket assignment, label values all match).
  - already-both-complete is a no-op against a nonexistent PGN path (same proof style as the
    existing `build_positions_cache_is_a_noop_once_already_complete`).
  - positions-already-complete dispatches to (unchanged) `build_labels` alone; labels-already-
    complete dispatches to (unchanged) `build_positions_cache` alone.

### `src/bin/rotor_train/main.rs`
- `run_prepare`: replace its two calls (`build_positions_cache` then `build_labels`) with one call
  to `length_cache::build_positions_and_labels`.

## Not in scope
- No change to `positions_dir`/`labels_dir` path conventions, database schema, or `run_train`/
  `run_check`/etc. -- this is purely `prepare`'s own build step.
- No migration concern: no generated `positions.cache`/label files exist yet in this repo (`data/`
  is gitignored and currently empty of any such build output), so there's no stale on-disk resume
  state in either the old two-file shape or a partial combined build to reconcile.

## Verification plan
- `cargo build --release` and `cargo test` both pass.
- A real `rotor-train prepare amanda` run (once `amanda`'s database exists) shows one progress bar
  per stem instead of two.

## Progress log

2026-09-22: Implemented as planned.
- `length_cache.rs`: added `CombinedResumeState` (struct/save/load, magic `RTCS`, its own path
  `{labels_dir}/combined_resume.state`, distinct from either single-purpose resume file), the
  private `build_positions_and_labels_together` (the real merged single-pass loop), and the public
  `build_positions_and_labels` dispatcher (the four-way branch on `(positions_done, labels_done)`).
  `build_positions_cache`/`build_labels` themselves untouched. Module doc comment's "each is its
  own independent, full PGN replay" paragraph updated to describe the new dispatcher and fixed a
  stale claim found while touching it (`build_positions_cache` does read `plies.score`, contrary to
  what the paragraph said).
- `main.rs::run_prepare`: now calls `length_cache::build_positions_and_labels` once per stem
  instead of `build_positions_cache` then `build_labels`.
- New tests: building both fresh together (content matches what the two separate functions would
  have produced), no-op once both already complete (against a nonexistent PGN path, same proof
  style as the existing single-purpose no-op tests), and the two "only one side needed" dispatch
  branches, checking the untouched side isn't redundantly rebuilt/doubled.

**Not done in this session** (no Bash/build access): running `cargo build --release` and
`cargo test`. Resumability of the new combined path (interrupt-and-resume) has no dedicated test,
matching this module's existing convention of not test-covering crash/resume for the two
single-purpose builders either -- exercised via real runs, not unit tests.
