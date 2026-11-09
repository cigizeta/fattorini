# Move Overhead -- implementation plan

**Status:** planned 2026-11-09, not started. The design and its decisions are in move-overhead.md.
**Do not start before the current SPSA run has finished** (the developer, 2026-11-09).

## 1. The reduction (`search.rs`)

- A constant `pub const DEFAULT_MOVE_OVERHEAD_MS: u64 = 30;` beside `MAX_CLOCK_FRACTION`, its doc
  comment carrying the why of move-overhead.md's decision 2 (inline, no reference to `claude/`).
- `time_limit` gains a last parameter `move_overhead_ms: u64`:
  - `movetime`: `TimeLimit::Fixed { target_ms: movetime.saturating_sub(overhead).max(1) }`;
  - clock: `k = moves_to_go.map_or(2, |m| u64::from(m.max(1)) + 2)`, then
    `time_ms: my_time.saturating_sub(overhead.saturating_mul(k)).max(1)`.
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
  `option name Move Overhead type spin default 30 min 0 max 5000`, after Ponder.
- `go_search` passes `self.move_overhead_ms` to `time_limit`.

## 3. The arena (`crates/arena/src/in_process.rs`)

- `choose_move` passes `DEFAULT_MOVE_OVERHEAD_MS` to `time_limit`.

## 4. Tests

`search.rs`:
- Fischer: `wtime 60000`, overhead 30, gives `time_ms` 59940 (k = 2).
- `movestogo 10`, overhead 30: `time_ms` = T - 360.
- `movetime 2000`, overhead 30: target 1970; `movetime 10`, overhead 30: target 1.
- A clock below the reserve (`wtime 50`, overhead 30, Fischer): `time_ms` 1, and the search still
  returns a legal move.
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
