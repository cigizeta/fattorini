# One progress bar style, with statistics, for arena and smithy

**Status:** done 2026-10-15: implemented, `progress`, `arena` and `smithy` tests passing with no bar
drawn. Bars are hidden under unit tests by a `hidden` argument of `progress::bar` and
`progress::resumed`, every caller passing `cfg!(test)`: indicatif writes to stderr past `cargo
test`'s capture, and hiding a bar after construction let its first frame through. The round robin keeps its table above the bar
(`style_under`); on its jump over games replayed from the record it restyles the bar baselined at
the jump and resets its elapsed time, since the shared rate is measured from the bar's start and
indicatif's `reset_eta` no longer applies. TODO.md's "One progress bar style for every
command" (the developer, 2026-10-15). Decided the same day: the training passes are included, and the
style lives in a new workspace crate.

## Today

| Where | Counts | Style |
|---|---|---|
| arena `self_play::build_progress_bar` (replay uses it too) | positions | `[elapsed] bar pos/len positions (pct%) ETA (rate)`, ETA and rate unsmoothed over this run |
| arena `book.rs::read_pgn` | bytes | `label bar bytes/total eta`, indicatif's smoothed ETA, no rate |
| arena `round_robin::LiveTable` | games | the standings table, then `[bar] pos/len games msg elapsed, ETA` |
| smithy `dataset_cache` build | positions | bare `ProgressBar::new` |
| smithy `dataset_cache::ShuffleBuckets::gather` | buckets | bare |
| smithy `buckets` | positions | bare |
| smithy `main.rs::print_progress` (train, test, calibrate passes) | steps or rows | hand-rolled `\r` line, no time, ETA or rate |

Left alone: arena `runner.rs`'s gate lines, log lines ending in `, ETA n min`, not a bar.

## The style

One line, self-play's as the model:

    {label} [{elapsed}] [{bar:40}] {pos}/{len} {unit} ({percent}%) ETA {eta} ({rate} {unit}/s)

- **ETA and rate unsmoothed**, measured from the bar's start position over this run alone: after a
  resume, work already done took no time this run and must not inflate the rate (self-play's reason,
  kept). indicatif's own estimator is doubly smoothed and not configurable.
- **The unit is the caller's**, plural: positions, games, buckets, steps, rows. Bytes print
  human-readable (`HumanBytes`) in the counts and the rate (`12.3 MiB/s`).
- **The label** is `{msg}`, empty when the caller gives none; the bar is 40 wide, `##-`.
- Before any work this run, ETA and rate print `-`.

## Step 1: the crate, `crates/progress`

- `Cargo.toml`: workspace package fields, `indicatif.workspace = true`, `[lints] workspace = true`.
  A comment on why it exists: one style for both tools, kept out of `rotor`, the engine.
- `src/lib.rs`:
  - `pub enum Unit { Count(&'static str), Bytes }`.
  - `pub fn style(start: u64, unit: Unit) -> ProgressStyle`: the line above, with the `eta` and `rate`
    keys baselined at `start` (moved from `self_play::build_progress_bar`, its doc comment with it).
  - `pub fn style_under(start: u64, unit: Unit) -> ProgressStyle`: `{prefix}\n` and the same line,
    for a bar under a block of text (the round robin's table).
  - `pub fn bar(len: u64, unit: Unit, label: &str) -> ProgressBar`: a new bar at 0.
  - `pub fn resumed(start: u64, len: u64, unit: Unit, label: &str) -> ProgressBar`: a bar positioned
    at `start`, baselined there.
- Tests: the ETA and rate keys render `-` before any progress and the expected values after some, from
  a `ProgressState` driven by hand where indicatif allows it, else through formatting functions the
  keys call (`eta_text(observed, remaining, elapsed)`, `rate_text(observed, elapsed, unit)`), tested
  directly.

## Step 2: arena

- `Cargo.toml`: `progress = { path = "../progress" }` (or a workspace dependency, as `rotor` is).
- `self_play.rs`: `build_progress_bar(completed, file_positions)` becomes
  `progress::resumed(start, file_positions, Unit::Count("positions"), "")`; its keys and imports
  (`FormattedDuration`, `ProgressState`, `ProgressStyle`) go. Replay calls it unchanged.
- `book.rs::read_pgn`: `progress::bar(length, Unit::Bytes, label)`, still `wrap_read`.
- `round_robin::LiveTable`: keeps its draw target (`stderr_with_hz(REDRAWS_PER_SECOND)`) and its table
  in `{prefix}`; the style becomes `progress::style_under(0, Unit::Count("games"))`, the pairing in
  play as the label (`{msg}`), as now.

## Step 3: smithy, the plain bars

- `Cargo.toml`: the `progress` dependency.
- `dataset_cache` build: `progress::bar(expected_positions, Unit::Count("positions"), "")`.
- `ShuffleBuckets::gather`: `progress::bar(buckets, Unit::Count("buckets"), "shuffle")`; the
  `println!` before it stays.
- `buckets::run`: `progress::bar(expected, Unit::Count("positions"), "")`.

## Step 4: smithy, the training passes

`round.rs` is unchanged: its callbacks still report `(label, processed, total)`. Only what
`main.rs` draws changes.

- `print_progress(label, processed, total, suffix)` and `clear_progress_line()` keep their names and
  call sites, but draw through one current bar held in a `static Mutex<Option<PassBar>>` (`PassBar`:
  the bar, its label and total):
  - **A new bar** when the label or the total differs from the current one, or the train position falls
    back (a new round): the old one is cleared, the new one made with `progress::resumed(position,
    total, unit, label)`, so a resumed round's steps already done are not counted in its rate.
  - Otherwise `set_position`.
  - The label carries the suffix: `train, round 12`; units `steps` for train, `rows` for test and
    calibrate, as today; train's position within the round as today (`processed % total`, a boundary
    reading `total`).
- `clear_progress_line()` clears the current bar (`finish_and_clear`) and forgets it, so the
  `println!`s that follow it today, round blocks and messages, land on clean lines unchanged.
- `PROGRESS_BAR_WIDTH` and `PROGRESS_LINE_CLEAR_WIDTH` go.
- The bars draw on stderr (indicatif's default), the round blocks on stdout, as the other bars already
  do; on a terminal they interleave as today.

## Step 5: tests and commands

- `progress`'s own tests (step 1).
- The existing tests keep passing: `round.rs` is untouched, `buckets` and `dataset_cache` use hidden or
  new bars only at run time.
- `print_progress`'s bar choice (new bar on a label change, a total change, or a train position going
  back; otherwise the same bar) as a small pure function, `needs_new_bar(current, label, total,
  position) -> bool`, tested.

For the developer:

    cargo test --release -p progress

    cargo test --release -p arena

    cargo test --release -p smithy

Then a look at the real thing: `bin/smithy buckets amanda-0002` (a plain bar), and the next `forge` or
self-play run.

## Open

1. `style_under` for the round robin, or the round robin's table moved into the label: the former
   keeps `LiveTable` as it is.
