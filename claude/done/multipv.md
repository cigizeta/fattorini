# MultiPV search

**Done:** search-level and UCI MultiPV (steps 1-3) are implemented. Step 4's `rotor-curate` consumer was reverted (curate-drop-pv-quiet.md), and `rotor-curate` no longer exists.

## Status

**`rotor-curate`'s own consumption of MultiPV (step 4's `pv`-driven part) was later reverted** (see
`curate-drop-pv-quiet.md`): `preprocessing.pv`, `is_position_quiet`, and `is_quiet_across_root_
candidates` were all removed, and `play_game` went back to a single plain `choose_best_move` search
per ply. The general search/UCI `MultiPV` feature described below (steps 1-3) is unaffected and
stays implemented as described.

The search-level feature (steps 1-3 below) is implemented and verified: `Search::excluded_root_
moves`, `search_root`'s exclusion parameter, `choose_best_move_multipv_with_progress`, and the UCI
`MultiPV` option all landed, with unit tests plus a manual UCI session confirming two distinct,
correctly ranked `multipv 1`/`multipv 2` lines per depth. Step 4 (the `rotor-curate` consumer, plus
loading real Syzygy tablebases into `generate` for the first time) is implemented -- `play_game` now
calls `choose_best_move_multipv_with_progress` and `is_quiet_across_root_candidates` (the new,
separately unit-tested combinator), and `run_generate_core` builds one shared `SyzygyTablebase` per
run. A first version drove the root-candidate count from a fixed constant (`2`) independent of
`preprocessing.pv`, which meant `pv: 1` networks never actually got the old, fast, single-search
behavior back -- caught from a real run (`betty` as slow as `amanda`) and corrected: see "Consumer"
below. Confirmed: `cargo build --release` and `cargo test` both pass on the corrected version.

Motivated by a real gap found while generalizing `rotor-curate`'s own quiet-position filtering
(`training.md`'s "Generalizing `pv > 1`" section): `is_position_quiet` only ever looks at the single
chosen best line's own next few moves. A quiet-looking best move doesn't rule out a nearly-as-good
tactical alternative sitting right behind it, and there is no way to see that alternative today --
every other root move is only ever "refuted, worse than X" by a narrow-window scout search inside
the normal single-line search, never given its own exact score or PV. Getting a real answer needs a
real second search: MultiPV.

## Why this is a real search feature, not a training-only concern

UCI's `MultiPV` option (Stockfish and most other engines support it) is useful on its own merits --
showing a human or a GUI the top N candidate moves, not just one -- so this is designed and built as
a genuine engine capability first, with `rotor-curate`'s own use of it (below) as one consumer, not
the reason for its existence.

## The search-level primitive: root move exclusion

The standard technique (Stockfish's own): to find the second-best root move with an exact score, run
the *same* full search again, at the same depth, with the already-found best move excluded from the
root's own move list entirely. Repeating this for each additional slot generalizes to any `MultiPV`
count via a plain loop, not special-cased for `N = 2`.

- `Search` gains a field, e.g. `excluded_root_moves: Vec<Move>`, empty by default (`MultiPV = 1`,
  today's exact behavior, unaffected). Only consulted at `ply == 0` inside `alpha_beta_with_buffer`'s
  own root move loop -- a move present in it is skipped outright, the same way an illegal
  pseudo-legal move already is.
- `search_root` gains a `excluded_root_moves: &[Move]` parameter, set on the `Search` it builds the
  same way `search.tablebase = Some(tablebase)` already is today (a field set after `new_search`,
  not threaded through `new_search`'s own already-long parameter list).
- `choose_best_move_with_progress`'s iterative-deepening loop, per depth: first call (empty
  exclusion list) gets rank 1, exactly as today. For `slot in 2..=multipv`, call `search_root` again
  at the *same* depth, excluding every rank-`1..slot` move already found this iteration. Each slot's
  own `(pv, score, nodes, ...)` is real and exact -- a genuine full-window search, not a scout bound.

## Reporting

`IterationReport` stays exactly as it is today (`depth`/`seldepth`/`score`/`nodes`/`pv`/`time_ms`,
rank 1 only) -- every existing caller (`rotor_curate.rs`, `uci.rs`'s current single-PV printing, the
existing test suite) keeps working unchanged when `MultiPV = 1`. A new, additional
`Vec<IterationReport>` (or a small `PvLine { score, pv }` if `depth`/`seldepth` end up genuinely
shared across every rank at one iteration, which they are) carries ranks `2..=multipv` for whichever
caller wants them -- `uci.rs`, when `MultiPV > 1`, and `rotor-curate`, per below.

`uci.rs`: new `setoption name MultiPV value N` (a `spin`, default `1`, matching `Hash`/`Threads`/
`SyzygyPath`'s own existing option style). Output gains a `multipv <i>` field (Stockfish's own
convention: right after `seldepth`, before `score`), one `info depth D ... multipv i score cp X ...
pv ...` line per rank, sharing `depth`/`seldepth`/`nodes`/`nps`/`time`/`hashfull` across every rank at
that iteration (matching Stockfish's own real output, not per-line accounting).

## Explicitly out of scope / deferred

- **Lazy-SMP interaction.** Every worker thread but thread 0 keeps searching only the single best
  line regardless of the engine-wide `MultiPV` setting -- the same convention `choose_best_move_with_
  progress`'s own doc comment already uses for live progress reporting ("a live progress stream from
  every worker would just be overlapping, conflicting noise"). Splitting Lazy-SMP workers across
  ranks is a harder problem, not tackled here.
- **Per-slot aspiration windows.** Every additional slot's own `search_root` call starts from the
  full sentinel window (`-INFINITY, INFINITY`), not a centered aspiration window the way rank 1
  already gets from `previous_score`. Slower than it could be, but correct, and simple -- worth
  revisiting only if profiling ever says otherwise (CLAUDE.md rule 5).
- **Checking further into each candidate's own line, for the quiet check below.** Explicitly decided
  with the developer: the multi-candidate quiet check applies at the root only, for now -- not
  recursively at every ply `pv_depth` already checks against the single best line. The two
  generalizations (deeper into one line, wider at the root) stay decoupled.

## Consumer: `rotor-curate`'s `is_position_quiet`, corrected

**Reverted** (see `curate-drop-pv-quiet.md`): this whole section describes work later undone.
`rotor-curate` no longer calls into MultiPV at all. Kept for history.

`is_position_quiet`'s existing `pv_depth` check (`training.md`'s "Generalizing `pv > 1`" section)
stays exactly as it is -- it still checks the single chosen (rank 1) line's own first `pv_depth`
moves for captures/promotions. Added on top, not replacing it: a position is quiet only if *neither*
of the top-ranked root moves is itself a capture or promotion, checked via the new
`is_quiet_across_root_candidates` combinator (`is_position_quiet`'s own depth check, `&&`ed with
`is_quiet` on rank 2's own first move when a rank 2 exists at all).

**Reversed from an earlier version of this document**: `K` (how many root candidates to consider)
was first built as a fixed constant, `2`, deliberately kept separate from `preprocessing.pv` (which
still means "how many plies deep into the chosen line") on the reasoning that the two answered
different questions and there was no present need for `K` to vary per network. That reasoning didn't
survive contact with a real run: the constant applied unconditionally, so *every* `generate` run paid
the extra full `search_root` call per ply regardless of its own `pv` setting -- including `pv: 1`
networks, which were supposed to get the old, fast, single-search behavior back. Caught only once
`betty` (`pv: 1`, meant as a fast baseline) turned out just as slow as `amanda` (`pv: 2`). Corrected,
directly at the developer's own instruction ("There should not be such a constant. pv is defined by
the yaml file!"): there is no more separate constant. `play_game` resolves `preprocessing.pv` once,
to `pv_depth`, and uses that same number both for `is_position_quiet`'s own depth check and as the
`multipv` count passed to `choose_best_move_multipv_with_progress` -- `pv: 1` (or no `pv` at all)
means exactly one root candidate, so `2..=1` is an empty range and no second `search_root` call ever
runs, restoring the old single-search behavior byte for byte. `pv: 2` now means both "check two plies
deep into rank 1's own line" and "consider the top two root candidates" from the same one number --
one config value, one meaning, not two axes pretending to be independent when only one of them was
ever actually exercised (nothing in this codebase has yet used `pv > 1` in production, so there was
no real cost to collapsing them). Only rank 2's own *first* move is ever checked for the breadth
side, at the root, not itself recursed `pv_depth` deep -- ranks beyond 2 are never requested at all
(`pv` has never been configured above `2` in practice), but the loop is general for any count.

While wiring this in, `rotor-curate` also starts using Syzygy tablebases for the first time --
`play_game` used to always construct an empty `SyzygyTablebase`, since there was no `SyzygyPath`
option to configure one through outside a real UCI session. Now loaded once per `generate` run (not
once per game or per worker -- probing is stateless and the loaded tables are read-only, so one
`SyzygyTablebase` is built up front and shared via `Arc` across every worker thread, the same
sharing shape `tt`/`tablebase` already have inside a single Lazy-SMP search) from the two fixed,
repo-root-relative directories `syzygy.sh` itself downloads into (`syzygy-3-4-5-wdl`,
`syzygy-6-wdl`) -- the same "run from the repo root" convention every other bare relative path in
`preprocessing_config.rs` already assumes. A directory that doesn't exist yet is not an error
(`add_directory`'s `Result` discarded, exactly `uci.rs`'s own `SetOptionSyzygyPath` handling already
does it), so this is safe to land before every machine running `generate` has actually downloaded
the tables.

## Implementation order

1. `Search::excluded_root_moves`, root-loop skip logic, `search_root`'s new parameter -- unit-tested
   directly (two moves available at a shallow, hand-built position, confirm excluding the better one
   makes the worse one win with its own correct, exact score). Done.
2. `choose_best_move_with_progress`'s per-depth slot loop, generalized to any `multipv` count. Done.
3. `uci.rs`: `MultiPV` option, `multipv <i>` output field. Done, verified via a manual UCI session.
4. `rotor-curate`: thread `multipv = 2` through `play_game`, extend `is_position_quiet`'s own check
   per above, and load real tablebases (see above). This is the one step that changes what
   `prepare`/`generate` actually write -- needs the same full rebuild-and-restart-from-scratch
   treatment as every other data-shape change this session (`training.md`'s own recurring note on
   this). Done, confirmed via `cargo build --release` / `cargo test`.
