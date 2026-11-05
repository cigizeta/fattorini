# A faster tablebase label for the scatter: design and implementation plan

Status: applied 2026-11-05, tests not yet run. **The developer's decision the same day supersedes two
sections below:** the long-loss labels are fixed, not kept, so a loss is decided as a win is
(`|dtz| + clock <= 99`, a repetition a draw for both sides); `label_by_ranks` is the slow path and
the agreement test's reference, not `root_result`; and buckets are scattered again from the start
(`labels` now says `tablebase-2`). Follows
tablebase-rescoring.md, which judged the probes to cost almost nothing at the scatter. On the
training machine the scatter was slowed by them, and was stopped to apply this.

**Added the same day (the developer):** the frozen test set is labelled by the tables too, through
the same `self_play_game_tablebase`, ending tablebase-rescoring.md's "not in scope": that exclusion
kept test losses comparable with old networks, a reason the restart removed. Its manifest records
`labels`, the same `TABLEBASE_LABELS`; a set without it is refused, to be deleted and frozen again.

## Why

`self_play_game_tablebase` labels every position of at most 6 pieces with
`SyzygyTablebase::root_result`, which goes through `root_ranks`: one probe per legal move, each in
memory-mapped tables, at scattered offsets. When the tables are not in the system's cache, each is
likely a disk read. `root_ranks` exists for play, which needs every move's rank; a label needs only
the result.

## What this does not do

- **Skipping positions that are not kept gains almost nothing, so it is left out.** `keep_probability`
  is 1.0 at the scatter and `KEPT` is every searched position, so only unsearched positions are
  dropped. An earlier suggestion of this step overestimated it.
- **A WDL-only probe of the root is not needed.** `shakmaty-syzygy`'s `probe_dtz` already reads the
  WDL table first and returns 0 for a drawn position without touching a DTZ table; `probe_wdl` goes
  through the same DTZ path, so it would save nothing.
- **Play is unchanged:** `root_result` keeps its path, used by the arena's early stop.

## The design

`SyzygyTablebase::label_result(board, ancestors) -> Option<TbResult>`, the same answer as
`root_result` for fewer probes, in this order:

1. `None` as `root_ranks` gives it: more pieces than the tables, any castling right, no legal move.
2. **Slow path, `root_result`,** when the rules can matter move by move: a repetition in the window
   since the last zeroing move (`repeated`, which also covers every threefold a child could make), or
   a clock of 98 or more (a child reaching the fifty-move draw).
3. One `probe_dtz` of the root, `ignore_rounding`. `d == 0`: a draw. A cursed win or blessed loss has
   `|d| > 100` and falls under 4.
4. With `c` the clock, as the ranks in `root_ranks` decide:
   - `d > 0`: a win if `d + c <= 99`, otherwise a draw;
   - `d < 0`: a loss if `-2 * d + c < 100`, otherwise a draw.
5. **Slow path when `d` is within `MARGIN` of either limit,** since a rounded DTZ may be off and the
   root's DTZ is not computed exactly as the ranks' move DTZ. `MARGIN` starts at 4 plies, and the
   agreement test below decides whether it holds.

A position `shakmaty-syzygy` refuses or a failed probe gives `None`, as today. With the full tables
of `bin/syzygy.sh` a difference there cannot arise from a missing table; the tables are required at
the scatter.

`self_play_game_tablebase` calls `label_result`. Nothing else changes.

## Buckets already scattered

Labels are meant to be identical, so `labels tablebase` in `buckets.tsv` stays, and committed batches
are kept: a Resume continues the scatter. This relies on the agreement test passing.

## Steps

1. `crates/temper-chess/src/tablebase.rs`: `label_result`, `MARGIN`, and a comment saying why the
   label needs no ranks.
2. `crates/smithy/src/game_labels.rs`: `self_play_game_tablebase` calls it.
3. **The agreement test,** in `tablebase.rs`, skipped without tables as the others are: from a fixed
   list of FENs of 3 to 6 pieces (pawnless, pawns, en passant available, one side bare, known cursed
   wins and blessed losses), random legal walks by a seeded generator, at varied clocks including
   both sides of every limit, some with a repeated ancestor: `label_result` equals `root_result`
   every time. Also a case for each slow-path condition and each `None`.
4. **A timing test,** `#[ignore]`d, on one such sample: both functions, and the share of positions
   that took the slow path. Run on the machine holding the tables, idle.

## Commands for the developer

```
cargo test -p temper-chess tablebase
```

```
cargo test -p smithy
```

```
cargo test -p arena
```

```
cargo clippy --workspace --all-targets
```

## Open

- **Is it the tables?** Not measured: Task Manager during a scatter says whether the tables' disk or
  Drive is the limit. If it is Drive, this helps little.
- **The speed-up is unknown** until the timing test. A side whose DTZ is not stored costs child
  probes inside `probe_dtz`, still no more than today's one per move.
- **A long loss labelled a draw:** read in `root_ranks`, not run. A loss ranks as a loss only when
  `-2 * dtz + clock < 100`, so at clock 0 a loss of 50 plies or more reads as a draw, while its
  winner's side reads as a win up to 99. The same ending can thus get 1 at one ply and 0 at the next.
  It follows Stockfish's root ranks, written for choosing moves, not for labels. Kept here, since
  this plan changes no label; whether labels should instead use the win's limit on both sides is a
  separate decision, and would need the buckets scattered again.
