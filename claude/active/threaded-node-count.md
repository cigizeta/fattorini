# The UCI node count over every thread

**Status:** designed and approved 2026-11-19 (the developer): the narrow version, the arena left
alone. Written the same day, `cargo test` and clippy passed. Left: seen in a real engine, the SMP
driver at 8 threads reporting about 16M nps, on the next snapshot.

## Why

With several threads, only thread 0 prints `info` lines, with its own nodes (`uci.rs`,
`go_search`), so 8 threads report about 2M nps where engines that sum their threads report about
16M (TODO.md, "Search", measured 2026-11-19). Testers and GUIs read the low figure. No effect on
strength either way: the search, time management and node limits are unchanged.

## The design (approved)

1. **One shared `AtomicU64`, the helpers' nodes,** made by `go_search` beside the stop signal, so it
   lives as long as one `go`.
2. **A helper (thread above 0) adds to it** at the checkpoint it already has, every
   `CHECK_INTERVAL` (1,024) nodes, in `alpha_beta_with_buffer` and `quiescence`, and the remainder
   when an iteration ends: one relaxed add per 1,024 nodes, the count late by at most 1,023 nodes a
   helper.
3. **Thread 0 adds it to its own nodes** in each `IterationReport` it makes, so the `info` line's
   `nodes` and `nps` are every thread's.
4. **Unchanged:** time management (its node fractions are thread 0's tree alone) and `go nodes`
   (thread 0's nodes, as today). With one thread the counter stays 0: the output, the bench's node
   counts and their determinism are as before. MultiPV's lines (thread 0 through
   `choose_best_move_multipv_with_progress`) keep thread 0's count: a later change if wanted.
5. **The arena left alone** (the developer): `choose_best_move_with_progress` and
   `choose_best_move_with_parameters` keep their signatures, the arena calling the first. One new
   public function, `choose_best_move_threaded`, takes the counter; `go_search` calls it for every
   thread. Inside, `iterate` and `search_root_limited` take `Option<&AtomicU64>`, every other caller
   `None`; `Search` holds it as `helper_nodes`, set for helpers only.

## Plan

1. `search.rs`: the `helper_nodes` field (`None` in `new_search` and the tests' `quiescence_search`),
   the two checkpoints, `search_root_limited`'s and `iterate`'s parameter, the remainder and thread
   0's sum in `iterate`, `choose_best_move_threaded`.
2. `uci.rs`: the counter beside the stop signal; both thread kinds call `choose_best_move_threaded`.
3. Tests: a helper's counter equals its own total nodes, an unstopped search; thread 0's report
   adds a counter given 1,000 to its own nodes.
4. The developer: `cargo test`, clippy; the SMP driver at 8 threads should report about 16M nps; the
   bench's one-thread node count unchanged.
