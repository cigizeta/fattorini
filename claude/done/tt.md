# Transposition Table

## Design

All five "big five" hand-crafted evaluation terms now exist (material, PST, mobility, pawn
structure, king safety -- see `king-safety.md` in `done/`). Per the developer's own roadmap,
next is a transposition table (TT) for search, then multi-threaded search.

This document was written after the design discussion was already complete, to capture it
for resumption -- see the "Progress log" section for exactly what's settled and what's next.

### Builds on Rotor's existing Zobrist hashing, not new infrastructure

Rotor already has a complete, working Zobrist hash: `Board::hash: u64` (piece placement,
side to move, castling rights, en passant -- deliberately not halfmove clock), kept
incrementally correct through `place_piece`/`remove_piece`/`make_move`/`unmake_move`,
implemented in `src/board/zobrist.rs` via a `const fn splitmix64` PRNG filling fixed `const`
tables (no runtime initialization). Already used by `Search::ancestors: Vec<u64>` for
threefold-repetition detection. The TT reuses `board.hash()` directly as its lookup key --
nothing new to build there.

### Where the table lives: owned by `Engine`, new module `src/tt.rs`

Like `Engine::board` and `Engine::ancestors`, the table persists across every `go` within a
game (the developer: "If I play a game, I do not want to reset my table for each new move"),
and resets only on `UciCommand::UciNewGame`, the same pattern those two fields already
follow. It does not live in `uci.rs` itself -- it gets its own module, `src/tt.rs`, holding
the `TranspositionTable` struct, its entry type, and the index/probe/store logic. A `&mut`
reference to it threads into `Search`/`alpha_beta_with_buffer`, a genuinely new kind of field
there (unlike killers/history/ancestors, which `Search` owns outright).

### Sizing: real UCI `Hash` option, small default matching real convention

Rotor doesn't implement `setoption` at all yet (it currently parses to
`UciCommand::Unknown` -- confirmed via grep). Landing the TT means implementing
`setoption name Hash value <MB>` for real, sizing the table in MB the way every real UCI
engine does, rather than inventing a Rotor-specific mechanism.

Default size verified against three independent real sources, not assumed:
- The UCI protocol spec's own text for the `Hash` option.
- SCID's own real default.
- The developer's own real `stockfish` binary: `echo "uci" | stockfish` was run and pasted,
  showing `option name Hash type spin default 16 min 1 max ...` -- Stockfish 17.1 itself
  defaults to 16 MB, and a user must explicitly raise it via `setoption` for a larger table
  (confirmed this is genuinely how Stockfish ships, not a surprise).

Rotor follows the same convention: small shipped default (~16 MB), real size controlled via
`setoption`, evaluation-framework tests able to set it explicitly through the same option
once it exists.

### Indexing: power-of-2 size, bitmask, no modulo -- explicit tradeoff accepted

`index = hash & (table_size - 1)`, `table_size` a power of two. Explicitly not `hash %
table_size`: "accept the loss of budget. Do not use %." A non-power-of-2 size (using modulo)
would use the full byte budget from `setoption`; a power-of-2 size under a bitmask can waste
up to nearly half the requested budget (rounding down to the nearest power of two below the
UCI-requested size). Accepted deliberately for the bitmask's speed, with the developer's own
reasoning: "I believe TCEC competitions do not have this problem" (top engines size their
tables generously enough that this rounding loss isn't the bottleneck).

The full 64-bit `hash` is stored as the entry's own verification key, not truncated --
confirmed collision safety over a truncated key was worth the extra bytes; the design never
seriously considered a smaller stored key.

### Entry schema

- **Verification key**: full `u64` hash (see above) -- confirms a probe's index match is
  actually the same position, not an index collision.
- **Best move**: the move that produced this entry's score, for move-ordering (see priority
  chain, below) and eventual best-move reporting.
- **`depth`**: `target_depth - ply` at the moment of writing ("remaining depth-to-go" --
  the same quantity `search.rs`'s history table already uses for its cutoff bonus, see
  `search.rs:522-523`, `search.rs:689`). For entries written from `quiescence` rather than
  `alpha_beta_with_buffer`, a fixed sentinel `0`, mirroring Stockfish's own `DEPTH_QS = 0`
  (verified against `types.h`/`search.cpp` -- see "Quiescence" below).
- **Score**: the stored value, side-to-move-relative like every other score in Rotor's
  search.
- **Bound flag**: `EXACT` / `LOWERBOUND` / `UPPERBOUND`, the standard alpha-beta-with-TT
  distinction (an entry that caused a beta cutoff only proves a lower bound on the true
  score; one that never raised alpha only proves an upper bound; one found strictly between
  alpha and beta is exact).

`depth` is **not** used by the replacement policy (see below, now "always replace") -- its
only remaining job is at probe time: deciding whether a hash-matched entry was searched deep
enough to trust for an immediate cutoff, or only shallow enough to use as a move-ordering
hint. This was confirmed as a real, separate use before agreeing to keep the field, after an
initial "don't store depth" instinct turned out to be based on the (mistaken) assumption
that depth only existed for replacement.

### Algorithmic base: Wikipedia's Negamax-with-TT pseudocode, two corrections identified

The developer's own historical reference
([Negamax with alpha-beta and TT](https://en.wikipedia.org/wiki/Negamax#Negamax_with_alpha_beta_pruning_and_transposition_tables),
from a pre-AI personal attempt years ago) is the algorithmic base, with two corrections
already identified against Rotor's actual code:

1. The Wikipedia pseudocode never stores a move in the entry, only value/flag/depth/validity
   -- Rotor's entry stores a best move too (see schema above), since move-ordering is one of
   the three real probe outcomes (below), not represented at all in the bare pseudocode.
2. Its `alphaOrig := alpha` pattern (saving alpha's original value before the search loop, to
   classify the bound correctly afterward) maps directly onto
   `alpha_beta_with_buffer`'s own existing `mut alpha: i32` parameter -- already the thing
   mutated in-loop via `alpha = alpha.max(best_score);` -- so this isn't new machinery, just
   reusing what's already there.

### Probe outcomes: three-way, confirmed against the developer's own prior attempt

- **Not found**: proceed with the node normally, no special handling.
- **Found, but not deep/bound-sufficient to trust for a cutoff**: still used as a
  move-ordering hint (see priority chain below), not discarded.
- **Found, and trustworthy** (`depth` sufficient and bound compatible with the current
  `alpha`/`beta` window): immediate cutoff, the node's own search is skipped entirely.

Confirmed as complete by the developer, matching their own recollection of a pre-AI personal
implementation attempt.

### Alpha-beta integration: three additive insertion points, not a rewrite

Read in full (`search.rs:544-729`) to answer "does alpha-beta need to change, and by a lot?"
directly, rather than assume. Confirmed structurally minimal:

1. A probe, added among the existing early-return checks near the top (alongside the
   repetition/fifty-move/insufficient-material checks already there).
2. One more stage in the existing move-ordering priority chain -- the TT-suggested best move
   tried ahead of killers and history, mirroring how `try_best_noisy_move` and the killer/
   history stages already compose.
3. One store, before the function's single existing final return.

The real new piece of work is threading the `&mut TranspositionTable` reference through
`Search` itself (see "Where the table lives," above) -- not the control flow inside
`alpha_beta_with_buffer`, which needed no restructuring.

### Replacement policy: always replace, refinement deferred

Settled after a real design detour, kept here for context on resumption:

- Initial instinct: "replace only if the new depth is greater or equal to the old depth."
- This surfaced a real tension: the table persists across the *whole game* (see "Where the
  table lives"), not just one search, so a pure depth-comparison rule risks a stale-but-deep
  entry from many real moves ago permanently blocking a currently-relevant new one -- a
  problem that doesn't exist in a single, self-contained search.
- Explored using `ply` (the same concept `Search::killers` is indexed by) as a fix. Ruled
  out: `Search::killers`'s use of `ply` (`search.rs:343-347`) is specifically because `ply`
  stays stable across iterative-deepening iterations, unrelated to the TT's staleness
  problem; and raw `ply` alone can't distinguish two entries at the same `ply` written during
  searches with very different `target_depth` (worked example: `ply = 2` during
  `target_depth = 3` has 1 real ply searched below it; `ply = 2` during `target_depth = 9`
  has 7 -- same `ply`, very different trustworthiness). This is exactly why `depth` in the
  schema above is `target_depth - ply`, not `ply` alone.
- Verified how Stockfish actually solves the staleness problem, against its real source
  (`tt.cpp`/`tt.h`/`types.h`): a small `generation8` counter on the table, bumped once per
  new search (`TranspositionTable::new_search()`); each entry stores the generation it was
  written at (`genBound8`); `relative_age()` computes how many generations old an entry is;
  and the actual replace condition in `TTEntry::save()` includes `|| relative_age(...)` as an
  unconditional clause -- **any** entry not from the current generation is eligible for
  overwrite regardless of its depth, so a stale-but-deep entry can never block a fresh write
  indefinitely. (Stockfish also clusters several entries per hash bucket and picks which to
  evict via `depth8 - 8 * relative_age`; that's a clustering feature, orthogonal to the
  generation mechanism itself, and not something Rotor adopts given the bitmask/single-slot
  indexing decided above.)
- Decision: **too much for this first landing.** Replacement is simply **always replace** --
  a write always overwrites whatever was in its slot, no comparison at all. The
  generation/age mechanism above is captured here as the concrete refinement to make later,
  not invented from scratch when the time comes.

### Quiescence: included, using a fixed sentinel depth

Verified against Stockfish's real `search.cpp`/`types.h` rather than assumed: `qsearch()`
does probe and write the same shared table as the main search, not a separate one, using a
fixed sentinel depth `DEPTH_QS = 0` instead of a computed remaining-depth-to-go (quiescence
has no `target_depth` to subtract `ply` from). Rotor follows the same shape: `quiescence`
probes and writes the same `TranspositionTable` as `alpha_beta_with_buffer`, storing `depth
= 0` for every entry it writes. This keeps every quiescence-origin entry self-marked as
low-trust by construction (any main-search probe asking for real depth will never treat it
as cutoff-worthy, only as a move-ordering hint), without needing a separate table or an
explicit exclusion check.

## Not yet settled / deferred

- **Age/generation-based replacement**, refining "always replace" -- picked back up as part
  of the in-progress multi-threaded search design, no longer deferred in place here; see
  `threading.md` for where that design actually landed (multi-threading is what made "always
  replace" genuinely untenable, not just theoretically imperfect, so it made sense to fold
  the refinement into that document rather than finish it here first).

## Progress log

Design discussion complete, across a full first-principles walkthrough matching the
project's established methodology (verify every real-engine claim against actual source
rather than memory; never invent an unjustifiable number; land the naive version first).
Verified against: the developer's own historical Wikipedia negamax-with-TT reference,
Stockfish's real `tt.h`/`tt.cpp`/`search.cpp`/`types.h` source (generation/age mechanism,
quiescence TT usage, `DEPTH_QS`), the real UCI protocol spec and SCID's default (`Hash`
sizing convention), and the developer's own real Stockfish 17.1 `echo "uci" | stockfish`
output (confirming the 16 MB default).

`src/tt.rs` now exists: `Bound` (Exact/Lower/Upper), `Entry` (private fields, accessors --
same pattern as `Move`), and `TranspositionTable` (`new(size_mb)`, `probe`, `store`,
`clear`). Indexing is the settled bitmask (`hash & mask`, `mask = slots - 1`, `slots` a power
of two); `new` translates a byte budget into a slot count via a private `with_slots`, split
out so tests can build a table with an exact, tiny slot count (e.g. 2) and hit an index
collision deterministically rather than by chance. Replacement is "always replace," as
decided. `mod tt;` wired into `main.rs`. Covered by unit tests (rounding to a power of two,
zero-budget doesn't panic, probe miss, store-then-probe round trip, index collision, always-
replace overwrite, clear). Not yet run against `cargo test`/`cargo build` in this session --
do that next.

`cargo build`/`cargo test` were run (295 passed) after that landing, confirming `src/tt.rs`
itself before wiring it in any further.

`Engine` now owns `tt: TranspositionTable` (`uci.rs`), built at a new `DEFAULT_HASH_MB = 16`
(Stockfish's own real default, per the verification above) and cleared -- not rebuilt -- on
`UciNewGame`, the same pattern `board`/`ancestors` already follow.

`Search` gained a lifetime parameter purely for this: `tt: &'a mut TranspositionTable`, the
one field that's a reference rather than owned (see its own doc comment). Threaded through
`new_search` -> `search_root` -> `choose_best_move` -> `Engine::go_search` (now `&mut self`,
since storing needs `&mut`), and separately through `alpha_beta`'s own throwaway one-slot
table for the untimed test/comparison API.

All three insertion points from the design above are implemented in `alpha_beta_with_buffer`:
a probe (single `self.tt.probe(...).copied()`, converting to an owned `Option<Entry>`
immediately rather than holding a borrow across the loop) right after the `MAX_PLY` check; a
TT-move stage prepended to the priority chain, gated to `i == start`, reusing `try_killer`
itself rather than a new function (it already does exactly "find this candidate move and swap
it to the front"); and a store before the final return, but only on the branch that returns
`best_score` itself, not the branch that delegates to `quiescence` (which stores its own
result). A new pure helper, `classify_bound(score, original_alpha, beta)`, turns a fail-soft
result into `Exact`/`Lower`/`Upper` against `original_alpha` (captured before the loop ever
touches `alpha`) -- covered by its own unit tests. `best_move: Option<Move>` is tracked
alongside `best_score` in the loop (same "strictly greater, not `.max()`" moment PV
bookkeeping already uses); provably `Some` by the store site given `ply < target_depth` and at
least one legal move, so the store call does `best_move.expect(...)` there.

One deliberate simplification, called out in a comment at the probe site: a `Lower`/`Upper`
hit that doesn't clear the window outright does *not* narrow `alpha`/`beta` before falling
through, unlike some textbook presentations of this lookup. Fully correct either way (a wider
window costs pruning, never correctness) -- and matches this document's own three-outcome
probe design above ("hint only" vs. "immediate cutoff," no third "narrow and continue" case),
not an accidental departure from it.

`quiescence` got the matching treatment: probes the same table (no depth check needed there --
its own remaining depth is always the `0` sentinel, and every stored `depth` is a `u32`, so
"deep enough" is tautological), and stores with `depth = 0` at its own final return. Unlike the
main search, `best_move` there can legitimately stay `None` at the return point (the in-check
placeholder never overwritten, or a stand-pat value no capture beat) -- `Entry` has no slot for
"no move," so those cases are simply left uncached rather than stretching the schema; a missed
optimization, not a correctness gap.

Every test call site that constructed a `Search` directly, or called `new_search`/
`search_root`/`choose_best_move` (roughly two dozen sites across `search.rs`), was updated to
supply a `TranspositionTable` -- most a fresh, throwaway one-per-call, but the two
node-count-comparison tests (`move_ordering_reduces_nodes_relative_to_no_ordering`,
`killer_moves_reduce_nodes_relative_to_no_killer_preference`) now persist their own `tt`
across the `for depth in 1..=6` loop, matching how `killers`/`history` already persist there --
the same reasoning that caught an earlier measurement artifact when those two tables were
reset fresh per depth instead of carried forward, applied proactively to `tt` this time rather
than repeated.

`cargo build && cargo test` run after this landing: 298 passed (295 before this piece, plus
the 3 new `classify_bound` tests) -- confirms the wiring compiles clean and nothing regressed,
including the two node-count-comparison tests, whose `assert!` inequalities held as expected.
Their hardcoded historical node counts in comments still predate TT, though, and remain
unverified as exact numbers (only the inequality was ever asserted) -- re-measure and update
those comments only if there's a concrete reason to (e.g. actually touching that code again),
not proactively.

`setoption name Hash value <MB>` is now implemented for real, in `uci.rs`. A new
`UciCommand::SetOptionHash(usize)` variant (specific to `Hash`, not a generic
`{ name, value }` shape -- Rotor has only ever needed the one option, worth generalizing once
a second one actually shows up) is produced by a new `parse_setoption`, matching the exact
four-token shape `["name", "Hash", "value", mb]` and falling through to `Unknown` for anything
else (unrecognized option name, missing value, non-numeric value) -- the same "ignore anything
unrecognized" robustness rule every other malformed command already follows. `Engine::handle`
responds to it with a full rebuild (`self.tt = TranspositionTable::new(mb)`), not a resize in
place, matching how Stockfish itself handles a `Hash` change -- losing whatever was cached is
an accepted, unavoidable cost of resizing mid-game.

The `uci` response now also advertises `option name Hash type spin default 16 min 1 max
1024`, the standard UCI spin-option shape, so a real GUI actually offers it as configurable --
implementing `setoption` without advertising the option in `uci` would leave most GUIs unable
to ever send it. `HASH_MB_MAX = 1024` is a new, deliberately naive ceiling (documented
alongside `DEFAULT_MOVES_LEFT`'s own precedent as "revisit only if there's a real reason to"),
not derived from measuring anything -- purely informational, since `setoption` itself doesn't
clamp to it (respecting min/max is the GUI's own responsibility per the UCI spec) and
`TranspositionTable::new` already tolerates any `usize`.

Added `TranspositionTable::slots()` (a plain accessor for the actual, real slot count) so
"`setoption` actually rebuilds the table at the new size" is a real assertion
(`setoption_hash_rebuilds_the_table_at_the_requested_size` in `uci.rs`) rather than only
checking the command is accepted without erroring. Also new: `parse_setoption`'s own four
unit tests (accepts `Hash`, rejects an unrecognized option/missing value/non-numeric value),
`TranspositionTable::slots`'s own test, and the pre-existing `uci_command_replies_with_
identity_and_uciok` test updated for the new `option` line. Not yet run against
`cargo build`/`cargo test` in this session -- do that next.

This closes the last implementation item from this document's own "not yet settled" list --
only the age/generation replacement refinement (see "Replacement policy" above) remains
deferred, by deliberate choice, not oversight.
