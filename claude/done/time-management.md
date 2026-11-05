# Time management

**Done:** the clock formula, best-move instability and `movestogo` are implemented (`TimeManager`, `clock_target_ms` in `search.rs`). What remains (score instability, settling alpha, beta, r, s) is tracked in TODO.md.

T = remaining time before the first iterative deepening iteration
M = moves to go before the next refill, or infinite if not provided
I = increment
C = discounted number of best move changes,
    normalized to [0, 1],
    as a measure of search instability

C[0] = 1

C[1] = 1

C[k + 1] = 0.5 * C[k] + 0.5 * (move[k] != move[k + 1])

k is the last iterative deepening iteration index, 1-based.

alpha = 5%, beta = 50%

r = 2, s = 0.5

0 < alpha < 1

0 < beta < 1

r >= 0

0 < s <= 1

alpha, beta, r, and s start with the values
specified above, then are tuned with automatic parameter
tuning, to be brainstormed in a separate document

(2026-10-12: not tuned. The values above are settled as they are, by reasonable
magnitudes like every search parameter; see TODO.md.)

target = min(((alpha + 1/M) * T + beta * I) * (1 + r * C), 0.5 * T)

The target is computed before the first
iterative deepening iteration, and recomputed
after every iterative deepening iteration

At the end of each iterative deepening iteration,
stop when elapsed >= s * target

Always stop current iteration when elapsed >= target

## Implementation plan

### Where things stand

- `search.rs::time_budget` turns a `go` into one `Option<u64>` budget:
  `min(time - 50ms, time / movestogo + inc)`, `movestogo` defaulting to 30, `movetime` taking
  precedence, `infinite` or a missing clock giving `None`.
- `choose_best_move_with_progress` and `choose_best_move_multipv_with_progress` each run the
  iterative deepening loop. Thread 0 alone reads the clock: before each depth after the first it
  stops (and sets `stop_signal`) when `elapsed > budget / 2`, otherwise it runs the depth with a
  deadline of `started + budget`, checked inside the tree every `CHECK_INTERVAL` nodes. Depth 1
  always runs with no deadline, so a legal move is always available.
- `go_search` in `uci.rs` is the only caller that passes a budget. `curate.rs`, `arena/replay.rs`,
  `arena/self_play.rs`, `arena/in_process.rs` and `benches/nnue_bench.rs` all pass `None` and are
  untouched by this plan.

The deadline is fixed for the whole of one iteration, and the target above is only recomputed
between iterations, so nothing inside the tree changes: `Search::deadline` keeps its meaning.

### Scope

- Clock controls: `wtime`/`btime`, `winc`/`binc`, and `movestogo`. Fischer (lichess) is the
  case without `movestogo` (M infinite); a repeating control such as CCRL's 40/15 (cutechess
  `tc=40/900`) sends `movestogo`, counting down to 1 before each refill. One formula and one set
  of parameters covers both.
- The cap `0.5 * T` is fixed, not tuned. Under Fischer it binds only in deep time trouble (at
  most instability, only when T < 4.3 I), and even then the clock settles at 2I instead of
  draining; near a refill it lets the last move before the control spend half the clock, the rest
  carrying over.
- `movetime`, `go depth N`, `go infinite` and a bare `go` keep today's behavior exactly.
- No safety margin is ever subtracted, on any path: `MOVE_OVERHEAD_MS` is deleted. When
  competing, lichess-bot's own move overhead hands over a reduced T; in local runs, the cap
  leaves half the clock after every move, far more than the local stop-to-`bestmove` latency.
  `movetime` is therefore used as given. Unlike the clock path, it has no cap, so it
  overruns by the stop latency (a few milliseconds), on the assumption that nothing enforces
  `movetime` strictly: neither lichess-bot nor `rotor-arena` sends it. Revisit if that changes.
- No score instability.

### Step 1 -- the time limit type

In `search.rs`, replace `time_budget`'s `Option<u64>` with:

```rust
pub(crate) enum TimeLimit {
    /// `go movetime`: today's behavior, a fixed target and s = 0.5, no instability.
    Fixed { target_ms: u64 },
    /// `wtime`/`btime` with `winc`/`binc` (0 when absent), for the side to move, and
    /// `movestogo` (`None` under Fischer, meaning M infinite).
    Clock { time_ms: u64, inc_ms: u64, moves_to_go: Option<u32> },
}
```

A `movestogo 0` is not valid UCI; it is read as M = 1 rather than dividing by zero.

`time_budget` becomes `time_limit(...) -> Option<TimeLimit>`, same arguments, `None` in the same
cases as today (`infinite`, or no clock for the side to move).

### Step 2 -- the parameters and the formula

```rust
pub(crate) struct TimeParameters { alpha: f64, beta: f64, r: f64, s: f64 }
pub(crate) const TIME_PARAMETERS: TimeParameters =
    TimeParameters { alpha: 0.05, beta: 0.5, r: 2.0, s: 0.5 };

/// The cap on any one move, as a fraction of the remaining clock. Fixed, not tuned.
const MAX_CLOCK_FRACTION: f64 = 0.5;
```

A constant for now. How the tuner overrides it (UCI options or otherwise) belongs to the tuning
document, not here.

A pure function, the one place the formula lives:

```rust
fn clock_target_ms(time_ms: u64, inc_ms: u64, moves_to_go: Option<u32>, instability: f64, p: &TimeParameters) -> u64
// min(((alpha + 1/M) * T + beta * I) * (1 + r * C), 0.5 * T), with 1/M = 0 when moves_to_go is None
```

### Step 3 -- a small time manager shared by both loops

Both iterative deepening loops duplicate the same gate today; a new `TimeManager` replaces that
duplicate in each:

```rust
struct TimeManager { started: Instant, limit: Option<TimeLimit>, instability: f64, last_move: Option<Move> }

impl TimeManager {
    fn new(limit: Option<TimeLimit>, started: Instant) -> TimeManager; // instability = C[0] = 1
    /// After a completed iteration: C[1] = 1, then C[k+1] = 0.5 * C[k] + 0.5 * changed.
    fn record_iteration(&mut self, best_move: Move);
    fn target(&self) -> Option<Duration>;             // None when limit is None
    fn should_start_next(&self) -> bool;              // elapsed < s * target
    fn deadline(&self) -> Option<Instant>;            // started + target
}
```

For `Fixed`, `target` is `target_ms` and `s` is 0.5, so `record_iteration` has no effect on it.

In the loops, thread 0 only (and always, in the MultiPV loop):

- Depth 1: no deadline, as today.
- Before each later depth: if `!should_start_next()`, set `stop_signal` and break, as today.
  Otherwise run the depth with `deadline()`.
- After each completed depth (after the `stopped` and empty-`pv` breaks): `record_iteration(pv[0])`.

This covers both requirements of the design: the target is computed before the first iteration
(from C[0]) and recomputed after every iteration. A stable iteration can shrink the target below
the elapsed time, which only means the gate stops at once -- correct, and no special case is
needed.

`choose_best_move`'s signature changes from `budget: Option<u64>` to `limit: Option<TimeLimit>`
(same for the two other entry points), so every `None` caller compiles unchanged.

### Step 4 -- `go_search`

`uci.rs::go_search` calls `time_limit` instead of `time_budget` and passes it through. Thread
0's search is the only one that gets it; the other threads keep reacting to `stop_signal` only.

### Step 5 -- tests (`search.rs`)

- `clock_target_ms` under Fischer at 3+2 (T = 180,000, I = 2,000): C = 0 gives 10,000; C = 1
  gives 30,000.
- The cap binds under Fischer: T = 1,000, I = 2,000, C = 1 gives 500 (uncapped: 3,150).
- `clock_target_ms` at 40/15 (T = 900,000, I = 0, C = 0): M = 40 gives 67,500; M = 1 gives the
  cap, 450,000.
- `movestogo 0` is read as M = 1.
- `TimeManager` instability sequence: moves A, A, B, B, B give C = 1, 0.5, 0.75, 0.375, 0.1875.
- `time_limit`: `movetime` wins over the clock; `infinite` gives `None`; a missing clock for the
  side to move gives `None`; `winc`/`binc` absent means 0; `movestogo` reaches `moves_to_go`.
- The existing `time_budget_*` tests are rewritten against `time_limit`, the two `movestogo`
  tests included.
- `choose_best_move_with_an_exhausted_budget_still_completes_depth_one` keeps passing with a
  `Fixed { target_ms: 0 }`.
- `uci.rs::go_movetime_still_returns_a_real_move_even_when_the_budget_is_tiny` relies on
  `movetime 1` minus the 50 ms overhead saturating to 0: send `movetime 0` instead, and update its
  comment.

### Step 6 -- documentation

- Rewrite the doc comments of `time_budget` (now `time_limit`), `choose_best_move` and
  `choose_best_move_with_progress` (the budget/headroom paragraphs), and the time management
  comment in `go_search`.
- Update `search.md` section 7 to point at this document, and replace the "Time management is
  minimal" entry in `TODO.md` with what remains: score instability, pondering, tuning the four
  parameters, and checking repeating controls with a few scaled-down games (`tc=40/60`) before
  entering CCRL.

### Verification

```
cargo test --release time_
```

```
cargo test --release choose_best_move
```

Then a match of the new build against the current one at 3+2 under `rotor-arena`.
