# Move Overhead -- implementation plan

**Status:** implemented and tested 2026-11-09 (`cargo test -p fattorini`, `-p arena`), after the
SPSA run ended. The measurement that may revise the default is a TODO.md item of its own. The
design and its decisions are in move-overhead.md. Also done beyond the plan: README's and
`RELEASE-NOTES.md`'s option tables list `Move Overhead`.

## 1. The reduction (`search.rs`)

- A constant `pub const DEFAULT_MOVE_OVERHEAD_MS: u64 = 20;` beside `MAX_CLOCK_FRACTION`, its doc
  comment carrying the why of move-overhead.md's decision 2 (inline, no reference to `claude/`).
- `time_limit` gains a last parameter `move_overhead_ms: u64`:
  - `movetime`: `TimeLimit::Fixed { target_ms: movetime.saturating_sub(overhead) }`;
  - clock: `k = moves_to_go.map_or(2, |m| u64::from(m.max(1)) + 2)`, then
    `time_ms: my_time.saturating_sub(overhead.saturating_mul(k))`.
  - No floor of 1 ms (amended 2026-11-09): `go_movetime_still_returns_a_real_move_even_when_the_budget_is_tiny`
    showed that 1 ms lets depth 2 start where `movetime 0` meant depth 1 alone.
  `TimeLimit`, `clock_target_ms`, `TimeManager` and `single_move_limit` are untouched: they receive
  the reduced clock.
- Its doc comment states the reduction and why `movetime` is reduced too.

## 2. The option (`uci.rs`)

- Constants `DEFAULT_MOVE_OVERHEAD_MS` (re-used from `search.rs`), `MOVE_OVERHEAD_MS_MIN = 0`,
  `MOVE_OVERHEAD_MS_MAX = 5000`.
- `UciCommand::SetOptionMoveOverhead(u64)`; `parse_setoption` matches
  `["name", "Move", "Overhead", "value", ms]`, a non-numeric value falling to `Unknown`.
- `Engine` gains `move_overhead_ms: u64`, set to the default in its constructor; `handle` clamps
  the value to the range and stores it. Kept across `ucinewgame`, like Hash.
- The `uci` reply advertises
  `option name Move Overhead type spin default 20 min 0 max 5000`, after Ponder.
- `go_search` passes `self.move_overhead_ms` to `time_limit`.

## 3. The arena (`crates/arena/src/in_process.rs`)

- `choose_move` passes `DEFAULT_MOVE_OVERHEAD_MS` to `time_limit`.

## 4. Tests

`search.rs`:
- Fischer: `wtime 60000`, overhead 20, gives `time_ms` 59960 (k = 2).
- `movestogo 10`, overhead 20: `time_ms` = T - 240.
- `movetime 2000`, overhead 20: target 1980; `movetime 10`, overhead 20: target 0.
- A clock below the reserve (`wtime 30`, overhead 20, Fischer): `time_ms` 0, and the search still
  returns a legal move, from depth 1 alone.
- Overhead 0 leaves every existing expectation as it was: the existing `time_limit` tests pass 0.

`uci.rs`:
- `setoption name Move Overhead value 100` parses to `SetOptionMoveOverhead(100)`; `banana` and a
  missing value parse to `Unknown`.
- A value above 5000 is stored as 5000.
- The `uci` reply's option list includes the new line (the existing list test updated).
- A fresh engine holds the default.

Run:

    cargo test -p fattorini
    cargo test -p arena

## 5. Afterwards

- The measurement of move-overhead.md, then the default revised if needed.
- The repeating-control check of TODO.md, on top of this change.
- README's options, if it lists them.
