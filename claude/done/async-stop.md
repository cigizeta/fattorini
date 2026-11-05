# Async UCI `stop`

## Design

The problem, as already flagged in `TODO.md`: `parse_command` has no case for `stop` at all --
it falls through to `UciCommand::Unknown` and is silently ignored. This isn't a small parsing
gap; `run()`'s loop blocks on `Engine::handle` for a `go`'s entire duration (today, that means
blocking inside `go_search`, which itself blocks on `std::thread::scope` until every Lazy-SMP
worker joins), so nothing reads stdin again until a search already finished on its own. A real
`stop` needs `go` to hand off to a background thread and `run()`'s loop to keep reading stdin
concurrently -- a genuine restructuring of `uci.rs`'s interface layer, not an addition.

### Real-engine grounding

Checked directly against Stockfish's actual current `engine.cpp`/`thread.cpp`, rather than
assumed: `Engine::go()` calls `ThreadPool::start_thinking(...)`, which returns immediately after
waking its own persistent worker-thread pool (`main_thread()->start_searching()`) -- the
UCI-parsing loop keeps reading stdin on its own separate thread the entire time. `stop()` is
exactly `threads.stop = true;`, nothing more. The search's own thread prints `bestmove` itself,
via a callback (`onBestmove`), not the stdin-reading thread. `wait_for_search_finished()` is a
separate, explicit blocking join, used elsewhere (e.g. before starting a new search), not
something `stop()` itself does.

### Settled: keep today's synchronous `go_search` as the actual search; just run it on its own thread

Rotor's already-built, already-verified Lazy-SMP machinery (`go_search` spawning N workers via
`std::thread::scope`, joining them, then returning) doesn't need to change shape at all -- it
just needs to run somewhere other than the thread reading stdin. `go` spawns that whole call onto
a fresh background thread and returns immediately; `stop` sets a flag that background thread
polls, reusing the exact same `stop_signal` mechanism already built for Lazy SMP (see
`threading.md`), just now also visible to the command loop, not only to the worker threads
within one `go`.

### Settled: `Arc<TranspositionTable>`, not a clone or a fresh table per search

The background thread now needs to outlive the `go` call itself, unlike today's
`std::thread::scope`-based workers, which stay borrowed and are joined before `go_search`
returns. `Engine.tt` changes from a plain `TranspositionTable` to `Arc<TranspositionTable>` --
the background thread clones the `Arc` (a cheap refcount bump), not the table itself, so a
potentially-1GB table is never actually copied, and `TranspositionTable`'s own shared,
lock-free semantics (already designed for exactly this kind of sharing) are completely
unchanged.

`new_search`/`clear` still need `&mut TranspositionTable`, which `Arc` doesn't hand out for
free -- `Arc::get_mut` only succeeds when the refcount is genuinely `1`, no outstanding clone
anywhere. Rather than lean on GUI-protocol timing (a well-behaved GUI won't send a new `go`
before the previous one's `bestmove`, but that says nothing about exactly when the *previous*
background thread's own `Arc` clone actually gets dropped, which happens only once that thread
fully returns -- a race, not a guarantee), `Engine` explicitly joins the previous search's thread
handle before doing anything that needs exclusive `tt` access. See "`RunningSearch` and the join
discipline" below for exactly where this happens. With that join always done first,
`Arc::get_mut` is guaranteed to succeed; failure there indicates a real bug (some clone escaped
that shouldn't have), not an expected condition, so it's an `.expect(...)`, not a fallback path.

### Settled: the background thread prints its own output

Matches Stockfish's own real model. The background thread `println!`s its `info` lines and
final `bestmove` line directly, the same way `go_search` builds them today, just inline rather
than accumulated into a returned `Vec<String>`. This needs one addition Stockfish's own
`sync_cout` mechanism already has and Rotor doesn't yet: a shared print lock (a plain
`Mutex<()>`), so the background thread's own writes can never interleave mid-line with whatever
`run()`'s own loop is printing for a different, concurrently-handled command (e.g. an `isready`
answered while a search is still running). `Engine` owns this lock; `run()`'s own per-command
print loop takes it too, around each line, so every write to stdout anywhere in the process goes
through the same lock.

### Settled: `go perft <depth>` stays synchronous

Perft is a fixed-position correctness/speed check, not a real search -- no time budget, no
aspiration windows, no reason a GUI would need to `stop` it mid-flight, and nothing about
`TODO.md`'s own framing of this problem ever mentioned it. Only plain `go` (the real search)
becomes async; `go_perft` keeps returning its lines directly, exactly as today.

### Settled: a stray `position`/`go`/`ucinewgame` while a search is running is not specially handled

Matches the UCI spec's own assumption and Stockfish's own accepted behavior: a compliant GUI
never sends a new `position`/`go`/`ucinewgame` before receiving the previous search's `bestmove`.
No new guard code checks for this. Where it doesn't hold, nothing unsafe happens regardless --
`Board`/`ancestors` are cloned into the background thread already (each search works from its
own frozen snapshot of the position, never `Engine`'s own live fields), and `Arc`'s reference
counting means replacing `self.tt` mid-search (e.g. a `setoption name Hash` racing an in-flight
search) just means the in-flight search keeps using the old table via its own still-alive `Arc`
clone, not a crash -- just not a scenario worth designing around on purpose.

### `RunningSearch` and the join discipline

```rust
struct RunningSearch {
    stop_signal: Arc<AtomicBool>,
    thread: std::thread::JoinHandle<()>,
}
```

`Engine` gains one field, `running_search: Option<RunningSearch>`. Before starting a new `go`
(and, for the same `Arc::get_mut` reason, before `UciNewGame`'s `self.tt.clear()` and
`SetOptionHash`'s table rebuild), `Engine` joins any `Some(running_search)` first -- in the
well-behaved-GUI case this returns immediately (the thread already finished, `bestmove` already
printed), so it's not a real wait in practice, just a correctness guarantee against the race
above. `go` then creates a fresh `Arc<AtomicBool>::new(false)`, clones it into the spawned
thread, and stores the new `RunningSearch` (replacing whatever was there, already joined).

`Stop` (a new `UciCommand` variant, parsed from the literal `stop` token, currently swallowed by
`Unknown`) just sets the flag on `self.running_search`'s `stop_signal`, if any is running, and
returns no output of its own -- exactly Stockfish's own `threads.stop = true;`, nothing more.
The background thread notices on its own next `CHECK_INTERVAL` poll (the same mechanism
`threading.md` already built) and prints `bestmove` whenever it naturally unwinds, same as a
budget-driven stop today.

`Quit` is checked in `run()` itself before `Engine::handle` is ever called (today's code already
short-circuits there), so the stop-then-join sequence for a clean exit belongs in `run()`, not
`handle()`: set the flag (if a search is running) and join before actually breaking out of the
loop, so the process never exits out from under a still-writing background thread.

## Not yet settled / deferred

Nothing outstanding -- every fork raised so far is settled above. `ponderhit` isn't relevant
(Rotor doesn't implement pondering) and needs no design here.

## Progress log

Design settled via three explicit forks (TT ownership, output model, busy-command handling),
each confirmed against real Stockfish source (`engine.cpp`, `thread.cpp`) rather than assumed,
plus the mechanical detail work those three answers implied (the `RunningSearch`/join-before-
exclusive-access discipline, the shared print lock, `go perft` staying synchronous).

Implemented in three pieces, each tested before the next:

1. `Engine.tt` converted from a plain `TranspositionTable` to `Arc<TranspositionTable>`, with
   `Arc::get_mut` used everywhere exclusive access is still needed (`new_search`, `clear`).
   Purely mechanical at this stage -- `go_search` itself was still fully synchronous, so
   `Arc::get_mut` always succeeded trivially. Full suite (342 tests) passed, unchanged.
2. `UciCommand::Stop` added (previously swallowed by `Unknown`), parsed from the literal `stop`
   token. Still a no-op at this stage (nothing yet to stop). 343 tests, +1 new.
3. The actual background-thread spawn: `go_search` now moves its whole body (owned clones of
   `board`/`ancestors`, an `Arc` clone of `tt`, a fresh `Arc<AtomicBool>` stop signal) onto
   `std::thread::spawn`, returning an empty `Vec` immediately; the spawned thread prints its own
   `info`/`bestmove` lines under a shared `stdout_lock`. `Engine` gained `running_search:
   Option<RunningSearch>` and `join_running_search`, called before anything needing exclusive
   `tt` access. `Quit` now calls a new `shutdown` (signal stop, then join) before `run()` breaks
   out of its loop, so the process never exits mid-write.

   A real gap surfaced only once this piece was tested, not at design time: 5 existing
   integration tests asserted directly on `handle`'s *returned* lines for a `go` command --
   impossible now that those lines are produced asynchronously. Fixed by having the background
   thread also write its computed lines into a `last_search_output: Arc<Mutex<Vec<String>>>`
   buffer (in addition to printing them for real), plus a `#[cfg(test)]`-only
   `join_and_take_last_search_output` that joins the running search and returns that buffer.
   Considered and rejected: capturing real stdout in tests (fragile under `cargo test`'s
   parallel execution) and reverting to a synchronous return path (would undo the whole point of
   this piece). All 5 affected tests rewritten to join-then-read instead of reading `handle`'s
   own return value. 343 tests, unchanged count -- confirming the restructuring itself is
   behaviorally neutral for the default single-threaded, well-behaved-GUI case.

Verified with two real manual UCI sessions, not just the test suite: a `go infinite` with
`isready` sent one second in (came back `readyok` immediately, proving the stdin loop stays
responsive during a search) followed by `stop` two seconds in (produced a legal `bestmove`
within a few hundred milliseconds, well short of exhausting `MAX_ITERATIVE_DEPTH` on its own --
confirming `stop` actually interrupts the search rather than coincidentally finishing).

In passing, this also resolved a stale claim in `search.rs`: `time_budget`'s own doc comment and
one of its tests both said real `infinite` support "doesn't exist yet" pending exactly this
work -- corrected in place now that it does, since `alpha_beta_with_buffer`'s `CHECK_INTERVAL`
poll already checked the shared stop flag unconditionally regardless of whether a deadline was
ever set, so `go infinite` was already honoring `stop` correctly the moment this landed, with no
separate code change needed for that specific case.

Nothing outstanding. Ready to move to `done/`.
