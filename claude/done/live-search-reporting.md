# Live search progress reporting

## Design

Discovered via real manual play (the developer's own Scid session, using Rotor as an analysis
engine, not a test): Rotor's `info depth ...` lines never actually stream live. Scid's analysis
pane sat at `Depth: 0, Nodes: 0` indefinitely, while the same session's Stockfish tab updated
continuously up to depth 43. Confirmed via a direct terminal replay of the exact command sequence
Scid sends (`uci`, `isready`, `position`, `go infinite`, `stop`) that Rotor's own protocol
handling is correct -- every line eventually appears, in the right order, with the right
content. The gap is real-time delivery, not correctness, and it predates this session's async
`stop` work: `go_search` (both before and after that work) only converts `choose_best_move`'s
returned `Vec<IterationReport>` into text and prints it once the entire search is over (every
depth done, or `stop` received) -- never incrementally, one line per depth, as a real UCI engine
does. Earlier manual terminal tests never caught this because a static paste of a finished
session looks identical whether lines streamed out over time or all landed at once at the end;
only a live-updating GUI actually exposes the difference.

### Real-engine grounding

Checked directly against Stockfish's current `search.cpp` rather than assumed: only the main
thread (`mainThread` non-null, i.e. thread `0`) ever calls `output_pv` to print an `info` line --
worker threads never print anything themselves, regardless of what depth they've reached or
whether their own result is what eventually gets reported as `bestmove`. Printing happens per
completed depth (and, in Stockfish's own richer case, sometimes mid-iteration on a fail-high/
fail-low once enough nodes have been spent -- a MultiPV/aspiration-retry refinement Rotor has no
current use for, since Rotor has neither MultiPV nor mid-iteration reporting today).

### Settled: only thread 0 drives live `info` output; final `bestmove` selection is unaffected

Worker threads (`thread_idx != 0`) continue exactly as today: silent, contributing only to the
shared TT and to `select_best_result`'s final choice at the very end. Thread `0` alone prints its
own `info depth N ...` line the moment each depth completes, live, under the same `stdout_lock`
every other writer already uses.

This deliberately reproduces a real, accepted asymmetry Stockfish itself has: the live `info`
stream reflects thread `0`'s own progress, not necessarily whichever thread's result eventually
wins the final `bestmove` selection (a different worker can still finish deeper and be chosen by
`select_best_result`, unchanged from how it works today). No retroactive "correction" line is
printed for the actual winner if it differs from thread `0` -- Stockfish doesn't either; the
concluding `bestmove` line is the only place the true winner is ever reported.

### Settled: a new `choose_best_move_with_progress` wrapper, not a signature change to `choose_best_move` itself

`choose_best_move` already computes each `IterationReport` incrementally inside its own loop
(`reports.push(...)`, once per completed depth) -- the data needed for live reporting already
exists at exactly the right moment; nothing in `search.rs`'s own iterative-deepening logic needs
restructuring, only a way to *observe* that moment from outside.

Considered and rejected: adding an `on_iteration` callback parameter directly to
`choose_best_move`'s own signature. That function has dozens of existing test call sites across
`search.rs`; changing its signature would mean touching every one of them for a capability only
one real caller (`go_search`, and only for `thread_idx == 0`) actually needs -- the same kind of
avoidable, wide-reaching ripple this project has declined elsewhere (e.g. not adding `Move`
methods for one caller's packing need in `tt.rs`).

Settled instead: `choose_best_move` keeps its exact current signature and behavior, used
unchanged by every existing caller. A new function, `choose_best_move_with_progress`, takes one
extra parameter -- `on_iteration: impl FnMut(&IterationReport)`, called once per completed depth,
right after each `reports.push(...)` -- and contains the real loop; `choose_best_move` itself
becomes a thin wrapper calling it with a no-op closure. `go_search` is the only caller of
`choose_best_move_with_progress` directly, and only for `thread_idx == 0`; every other thread
still calls plain `choose_best_move`, exactly as today.

### `go_search`'s own thread-0 callback

Formats and prints exactly the same `info depth ...` line `go_search` already builds today,
just per-report instead of over the whole finished `Vec` at once, under the existing
`stdout_lock`. No new formatting logic -- the same `format_move`/`format_score` calls, moved
from the post-hoc `.map()` over the final `reports` into this callback instead.

## Not yet settled / deferred

Nothing outstanding. `MultiPV` and mid-iteration (aspiration fail-high/fail-low) reporting are
both real refinements Stockfish's own model has that Rotor doesn't need yet -- Rotor has neither
MultiPV nor a reason to report a score before a depth's own iteration is exact, so both are
out of scope here, not deferred gaps in this design.

## Progress log

Design settled: grounded in Stockfish's real `search.cpp` (only the main thread prints, worker
threads never do); the wrapper-function approach (`choose_best_move_with_progress`) chosen
specifically to avoid rippling a new parameter through `choose_best_move`'s dozens of existing
test call sites, an avoidable cost this project has declined to pay elsewhere for similar
single-caller capabilities.

Implemented: `choose_best_move_with_progress` added to `search.rs` (calls `on_iteration` right
after each depth's `IterationReport` is pushed); `choose_best_move` reduced to a thin no-op
wrapper around it; `go_search` rewritten so thread 0 alone gets a real, printing callback and
every other thread still calls plain `choose_best_move`, unchanged. One new test added
(`choose_best_move_calls_on_iteration_once_per_completed_depth_with_matching_content`). 344
passed.

Verified live, and found a second real bug in the process: a first real manual test (`go
infinite` piped through a shell loop that timestamps each line as it's read) still showed every
`info` line and `bestmove` landing within a ~25ms burst, all clustered right around when `stop`
was sent -- despite each line's own `time` field showing depths finishing anywhere from 2ms to
1.8s apart. The callback wiring itself was already correct (confirmed by adding temporary
`eprintln!` instrumentation at the top of `on_iteration`, which -- unbuffered, straight to
stderr -- showed each depth's callback firing at exactly the real wall-clock moment its own
`time` field implied). The actual bug: `println!`'s reliance on `Stdout`'s implicit
flush-on-newline was not dependable on this machine's real pipe/tty stack (WSL2) -- lines sat
unflushed for seconds, then all appeared together the moment something else (process exit, or
apparently just the unrelated stderr writes) forced a flush. Fixed by adding an explicit
`std::io::stdout().flush()` after every `println!` this engine makes, in both `go_search`'s
live/final lines and `run()`'s own reply loop, rather than trusting implicit line-buffering.
Debug instrumentation removed once the question was answered, per this project's own stderr-
diagnostic convention. Re-verified after the fix: a plain timestamped terminal test (no debug
lines) shows lines landing at the right real moments, e.g. depth 7 arriving ~494ms after depth
1 and depth 8 ~1.86s after, matching their own reported `time` fields; 344 passed.

Re-verified in Scid itself (the GUI that originally exposed the bug): the analysis pane now
updates continuously while Rotor thinks, matching Stockfish's own pane instead of sitting at
`Depth: 0, Nodes: 0` until the search ended. Closed.
