# Search

**Done:** every roadmap item is implemented. Later changes: move-picker.md (staged picker), search-simplification.md (one `MARGIN_PER_PLY`), singular-extensions.md (tried and removed), continuation-history.md. Constants and margins quoted below are historical; `search.rs`'s doc comments are the reference.

## Status

All roadmap items below are implemented in `search.rs` and wired in. What's genuinely still
missing, per `TODO.md`: continuation history (own document, `continuation-history.md`) and
singular extensions (not started, not designed). The strength-measurement framework needed to
validate any of the node-count-tuned constants below against real playing strength is decided
and partly built (`cutechess-cli`, adopted as an external match referee rather than a
from-scratch tool -- see `TODO.md`) but tier-1 analysis (result tally, Elo/SPRT from PGN headers)
isn't implemented yet. Essentially every margin/threshold below was settled by a node-count sweep
at a fixed benchmark depth, not by measured playing strength -- flagged throughout as provisional
pending that harness, not because the node-count measurements are wrong, but because node count
and playing strength are different questions.

## 1. Foundational design

**Search algorithm**: negamax (one recursive function, negating child return values) layered
with fail-soft alpha-beta pruning. `alpha_beta(board, depth, alpha, beta) -> (i32, u64)` is the
stable public/test API; the production path is `choose_best_move` -> `search_root` ->
`alpha_beta_with_buffer` (a `&mut self` method on `Search`).

**Evaluation contract**: `evaluate(board: &Board) -> i32`, side-to-move-relative, one call site,
swappable behind that one seam. UCI `score cp` is also side-to-move-relative, so no perspective
conversion happens at the UCI boundary. Whatever sits behind `evaluate` must return a value
already on `piece_value`'s centipawn scale.

**Piece values**: pawn 100, knight 320, bishop 330, rook 500, queen 900; king uncounted.
`piece_value(PieceType) -> i32` in `eval.rs`, `pub(crate)`, a fixed ruler MVV-LVA/`game_phase`/
every search margin constant depends on staying put -- distinct from `piece_value_mg`/
`piece_value_eg`, the actual tunable quantities.

**Checkmate/stalemate**: detected via `legal_moves` being empty at a node, not evaluation's job.
Checkmate scores by ply so faster mates score better; stalemate is `0`.

**Mate-score formula**: `-(MATE_SCORE - ply as i32)` everywhere (main search and quiescence
identically); `MATE_SCORE = 30_000`. `mate_in(ply)`/`mated_in(ply)` are the named helper forms
used by mate distance pruning. `is_loss(score) = score <= -(MATE_SCORE - MAX_PLY as i32)`;
`is_win` is its mirror. `INFINITY = i32::MAX`, never `i32::MIN` (negating it overflows).

**Depth representation**: two independent counters. `ply: u32` (distance from root, +1 per
recursive call, unmodified by quiescence) and `target_depth: u32` (fixed per
`alpha_beta_with_buffer` call). `remaining = target_depth - ply`, recomputed once per node,
drives every killer/history/margin calculation. `MAX_PLY: u32 = 20`, an absolute ceiling on total
ply (main search + quiescence combined), checked `>=` not `==`. `MAX_ITERATIVE_DEPTH: u32 = 10`.

## 2. Move ordering

**Priority chain in the main search's move loop**: TT move -> best noisy move (captures/
promotions via real SEE, see below) -> killer slot 0 -> killer slot 1 -> best combined-history
move -> arbitrary remaining quiet. A short-circuiting `||` chain of bool-returning stage
functions. Selection is lazy incremental (`try_best_noisy_move`/`select_best_move`): finds the
best-scoring move in the unexamined remainder and swaps it to the front, one call per loop
iteration, not a whole-slice sort -- needed since a cutoff means some moves are never looked at.
Realized as a swappable `fn(&Board, Move) -> i32` field (`Search::score`).

**Quiescence keeps MVV-LVA** (`move_score`, captures scored `10 * victim_value -
attacker_value`, promotions add the promoted piece's value), called directly, not through
`Search::score` -- deliberate, permanent scope split: `try_best_noisy_move`'s "no more captures,
safe to break" guarantee depends on MVV-LVA's structural property that any real capture scores
strictly positive; real SEE can be `0` or negative, which would silently break that invariant.

**Quietness** is decided by `is_quiet(board, mv) -> bool`, decoupled from whichever `score`
strategy is active.

### Killer moves

Two slots per ply (`Vec<[Option<Move>; 2]>`), classic MRU replacement. Quiet moves only.
Indexed by `ply`, persisted across iterative-deepening iterations within one `go` (owned by
`choose_best_move`, sized `MAX_PLY`, threaded by `&mut` through every call across all depths,
carried forward even from an aborted iteration). `alpha_beta` (the untimed comparison API)
always constructs a fresh table. Recording gated on `ply < target_depth && is_quiet(...)` at the
`alpha >= beta` cutoff site.

### History heuristic

Shape `[[i32; 64]; 12]`, i.e. `[piece][to]`, not `[from][to]` -- chosen for Rotor's own smaller
node-count scale. `[from][to]` remains a filed, deferred alternative.

**Bonus/malus, with gravity**: every write goes through `apply_history_bonus(entry, bonus, max)`,
a self-limiting `entry += bonus - entry * abs(bonus) / D`-style formula (bonus clamped to
`+-max` first). Bonus on a cutoff-causing quiet move: `(remaining)^2`. Malus (the exact mirror,
subtracted): applies to every quiet move actually fully recursed into at that node before the
cutoff-causing move (tracked via `quiets_searched`), not moves skipped by pruning.
`FLAT_HISTORY_MAX = 8192`.

Persisted across iterative-deepening iterations, same lifetime as killers. Priority stage
`try_best_history_move`, tried after both killer slots. Combined with continuation history
(`continuation-history.md`) as `combined_history_score`, plain unweighted sum. Not applied inside
`quiescence` at all.

### History-based pruning of quiet moves (main search only)

```rust
fn is_history_pruned(applicable, history, board, mv, remaining) -> bool {
    applicable && is_quiet(board, mv)
        && history_score(history, board, mv) < HISTORY_PRUNING_MULTIPLIER * (remaining * remaining) as i32
}
```

`HISTORY_PRUNING_MULTIPLIER: i32 = -35` (converted from Stockfish's real gate into
"net-event-equivalents" against Rotor's own bonus formula, not a literal unit copy).
`applicable` mirrors futility pruning's own `best_move.is_some()` gate. Wired as one of the
independent pruning gates in the move loop; a history-pruned move never joins `quiets_searched`.

## 3. Static exchange evaluation (SEE)

`src/movegen/see.rs`. `attackers_to(board, target, occupancy) -> Bitboard` (both colors,
caller-supplied occupancy so it can answer "who attacks after these squares are removed" for
X-ray re-detection); `least_valuable_attacker(board, attackers, color)`. Attacker selection:
ascending piece value, alternating sides. X-ray attackers recomputed against shrunk occupancy
after each attacker's origin is cleared (sliding attackers only).

**Gain array**: `gain[0] = value(initial victim)`, each subsequent entry pushes the raw value of
whatever's on the target square; resolved backward, `gain[i] -= max(gain[i+1], 0)` (the next side
declines unless continuing is good for them). `gain[0]` after resolution is the exchange's true
net result.

**Known, accepted limitations** (shared with real engines): pins are ignored; a promotion is only
accounted for on the initiating move, never a later recapture landing on the back rank.

**Interface**: `see(board, mv) -> i32` (fully resolved), `see_ge(board, mv, threshold) -> bool`.
Piece values reused directly from `eval.rs`. Wired into three consumers: quiescence's bad-capture
skip, main-tree move ordering (`Search::score = see`), and main-tree SEE pruning.

### SEE pruning in the main search

```rust
const SEE_PRUNING_MARGIN: i32 = 2 * piece_value(PieceType::Pawn);

fn is_see_pruned(applicable, board, mv, remaining) -> bool {
    applicable && !see_ge(board, mv, -(SEE_PRUNING_MARGIN * remaining as i32))
}
```

`applicable` folds in `best_move.is_some()` and `!gives_check(board, mv)` (a checking move is
exempt, since it's an extension candidate instead). Unbounded, linear in `remaining`, no depth
cap -- unlike futility pruning, since SEE computes an exact forced-exchange result for one
specific move, not a whole-board heuristic guess. One unified margin for quiet and capturing
moves. `2 pawns/ply` was chosen over a faster `0.5 pawn/ply` deliberately (soundness over
node-count minimization, absent a strength harness) -- the effect is measurably non-monotonic in
the margin at real depths (a mid-range value can be worse than either a looser or tighter one),
since SEE pruning can discard a move that would have produced an early cutoff for
non-material reasons.

## 4. Quiescence search

`fn quiescence(&mut self, board, ply, alpha, beta, prev_to_sq: Option<Square>) -> i32`, sharing
`Search`'s `moves`/`nodes`. No `target_depth` -- depth control is `ply` vs `MAX_PLY` only.

**Not in check**: `stand_pat = evaluate(board)` is the initial `best_score` (a real candidate,
never a sentinel). `stand_pat >= beta` returns immediately. Quiet moves are skipped entirely.
Ordering: TT move first if noisy (a stored quiet TT move is ignored outright, since running it
through delta pruning's victim lookup would panic), then MVV-LVA, then delta pruning, then
SEE-margin pruning, then the move-count cap.

**In check**: no stand pat (`best_score` starts at `i32::MIN`); every legal move is mandatory,
quiet ones included.

**Known, accepted gap**: a genuine stalemate inside quiescence (not in check, zero legal moves)
falls through to stand pat's material evaluation rather than `0` -- catching it would require
legality-checking every move even when none are noisy. Real engines accept the same gap.

**Main search's leaf**: `if ply >= target_depth { quiescence(...) } else { best_score }`. PV
extraction stops at the main search's own horizon and never extends into quiescence.

### Delta pruning

Outside check, before a capture is made: `if stand_pat + piece_value(victim) + DELTA_MARGIN <=
alpha`, skip without recursing. Checked before SEE (cheaper). Never applied to a promotion.
`DELTA_MARGIN = 2 * piece_value(Pawn) = 200`.

### SEE-based pruning inside quiescence

`if !in_check && !see_ge(board, mv, -SEE_MARGIN) { continue; }`, checked after ordering, after
delta pruning. Every legal check evasion stays mandatory. `SEE_MARGIN = piece_value(Pawn) / 2 =
50` -- Rotor permits a capture that loses up to half a pawn through.

### Quiescence move-count pruning ("late-move cap")

Once `QUIESCENCE_MOVE_LIMIT` ordinary noisy moves have been tried at a node without a cutoff,
`break` on any further candidate. `QUIESCENCE_MOVE_LIMIT: u32 = 2` (Stockfish's own value).

**Four exemptions, all implemented**: (1) `!is_loss(best_score)` is the outer gate -- no pruning
in detected-forced-loss territory; (2) not a promotion; (3) not a recapture
(`is_recapture(mv, prev_to_sq)`, `prev_to_sq` a plain frame-local parameter); (4)
`!gives_check(board, mv)`, a cheap static check-detection test reusing `attackers_to`, not
make/unmake (castling isn't handled, unreachable since castling is quiet). This is the largest
single node-count win of the whole optimization arc. Known, accepted tradeoff: the `gives_check`
exemption increases node count relative to a version without it -- kept for tactical soundness.

## 5. Main-search pruning and reductions

All of the following live in `alpha_beta_with_buffer`, checked in roughly this order per node/
per move.

### Reverse futility pruning (static null-move pruning)

Once, before any move is generated: `if static_eval - FUTILITY_BASE_MARGIN * remaining as i32 >=
beta { return static_eval; }`. Reuses `FUTILITY_BASE_MARGIN` directly (see classical futility
pruning below). `ply > 0` gate is load-bearing (root has no move/`pv[0]` behind a bare-cutoff
return).

### Razoring

```rust
if ply > 0 && !is_win(alpha) && !in_check {
    eval = evaluate(board); // recorded into static_evals[ply]
    if eval < alpha.saturating_sub(RAZORING_MARGIN * remaining as i32) {
        return quiescence(board, ply, alpha, beta, None);
    }
}
```

Placed right after the TT probe, ahead of reverse futility pruning. No explicit depth ceiling
(the margin's linear growth self-limits). `saturating_sub` is required (`alpha` can legitimately
sit at the `-INFINITY` sentinel). `!is_win(alpha)` guard is required (a mate-scale `alpha` from a
sibling must not make every ordinary evaluation look hopeless). `RAZORING_MARGIN = 3 *
piece_value(Pawn) / 2 = 150`. One of the largest single node-count wins measured.

### `improving` (infrastructure)

```rust
fn is_improving(&self, ply: u32, beta: i32) -> bool {
    let Some(eval) = self.static_evals[ply as usize] else { return false };
    if eval >= beta { return true; }
    if ply < 2 { return false; }
    match self.static_evals[ply as usize - 2] {
        Some(previous) => eval > previous,
        None => false,
    }
}
```

`Search::static_evals: Vec<Option<i32>>`, one slot per ply. Does not persist across
iterative-deepening iterations (unlike killers/history). Unconditionally cleared at the top of
every node alongside `pv[ply].clear()`.

### Late move / move-count pruning (main search only)

```rust
fn has_non_pawn_material(board, color) -> bool
fn late_move_pruning_threshold(remaining, improving) -> u32 {
    (3 + remaining * remaining) / if improving { 1 } else { 2 }
}
```

Applicability: `ply > 0 && !in_check && has_non_pawn_material(board, color)` (avoids pruning in
king-and-pawn endings, where zugzwang means "try a different quiet move" isn't safe). Per-move:
`applicable && !is_loss(best_score) && is_quiet(board, mv) && moves_tried >=
late_move_pruning_threshold(remaining, improving)`. Captures/promotions are never touched. No
explicit depth ceiling. Once triggered, `break`s the whole remaining loop. The single largest
node-count reduction measured at fixed depth 4.

### Mate distance pruning

```rust
alpha = alpha.max(mated_in(ply));
beta = beta.min(mate_in(ply + 1));
if ply > 0 && alpha >= beta {
    return alpha;
}
```

Sound by construction, not a heuristic. Placed right after the draw checks. `beta` is a `mut`
local so the tightened value benefits the rest of that node too.

### Late move reductions (LMR, main search only)

Reduces one move's own subtree by handing its recursive call a smaller `target_depth`.

**Eligibility**: not the first move (`moves_tried >= 1`); `remaining >= 3`; `is_quiet(board,
mv)`; `!gives_check(board, mv)` (a checking move is extended, not reduced -- extension wins
outright).

```rust
fn lmr_reduction(remaining: u32, moves_tried: u32) -> u32 {
    let unclamped = remaining.ilog(3) + moves_tried.ilog(3);
    unclamped.min(remaining - 2)
}
```

Base `3` matches the `>= 3` eligibility floor, so `lmr_reduction(3, _floor) == 1` (a strict
generalization of the original flat-`1` behavior). The `.min(remaining - 2)` clamp is a direct,
provable guarantee that the reduced probe's own remaining depth never underflows.

**Application**: one extra cheap probe prepended in front of the existing PVS ladder -- a reduced
null-window scout at `target_depth - lmr_reduction(...)`; only a fail-low is trusted directly.
Anything else falls through to the full, unreduced PVS ladder. The move-count axis matters
considerably more in aggregate than the depth-only axis alone, since it applies at every
remaining depth as soon as several siblings have been tried.

### Search extensions: check extensions (main search only, no cap)

Whenever `mv` gives check, its genuine recursive call (not an LMR-reduced probe) gets
`target_depth + 1` instead of bare `target_depth` -- since `remaining = target_depth - ply`, the
child searches at the same `remaining` its parent had. No cap, deliberately: a real forcing
sequence is typically several checks in a row, and termination is guaranteed independently by
repetition/fifty-move/`MAX_PLY` regardless of extension count. This is the only technique
expected to and does increase node count/time at a fixed nominal depth -- correct behavior, not a
regression (its payoff is tactical accuracy, which node-count-at-depth-N can't measure).
`static_evals`/`pv` are sized `MAX_PLY + 1` unconditionally (needed once `ply` can run past
nominal depth along an extended line).

### Classical (move-level) futility pruning (main search only)

For each quiet candidate at low remaining depth: `static_eval + FUTILITY_BASE_MARGIN * remaining
as i32 <= alpha` skips that move without recursing (unsound -- can discard the true best move).
`FUTILITY_MAX_DEPTH = 3`, gate `remaining <= FUTILITY_MAX_DEPTH`. `FUTILITY_BASE_MARGIN = 2 *
piece_value(Pawn) = 200` ("2 pawns/ply") -- a deliberate strength-over-speed middle ground, not
the node-count optimum. Never applied to the first move at a node (`best_move.is_some()` gate).
Reused directly by reverse futility pruning above, not independently recalibrated.

### Null-move pruning (main search only)

At a node with enough remaining depth, search a "pass" (`Board::make_null_move`/
`unmake_null_move`: flips `side_to_move`, clears `en_passant_square`) at reduced depth with a
null window around `beta`; if that still fails high, prune the whole node.

**Guards**: `!in_check`; `has_non_pawn_material(board, color)`; no two null moves in a row
(`Search::null_move_in_progress`); `!is_win(beta)`; `eval >= beta` as a cheap pre-filter (shared
with razoring's own `eval` local).

**Reduction**: fixed `R = 2`, applied as a shrunk `target_depth - R`. Minimum `remaining >= R + 2
= 4` before applying. Mate-score safety: if the probe's own returned score is itself mate-range,
the cutoff returns plain `beta`, not that raw score. At fixed shallow depth 4 this structurally
cannot fire (`remaining` only reaches its minimum at `ply == 0`, excluded by the root guard); its
real, growing win shows at deeper depths. `R` itself has not been swept.

## 6. Draw detection

**Repetition**: `Search::ancestors: Vec<u64>`, every ancestor hash of the position the current
call is looking at, seeded once per `go` from the real game's move history and extended during
recursion (pushed before each recursive call, popped after). A single earlier occurrence counts
as a repetition for search purposes, not literal threefold -- without a TT, reaching an identical
position again means its subtree was already explored once at more remaining depth. Checked at
every node, main search and quiescence alike (a perpetual-check cycle can occur entirely within
quiescence's own horizon via quiet check-evasions). Windowed to the last `halfmove_clock()`
entries. `ply > 0` guard is required (a bare match at the root corrupts `bestmove` reporting).

**Fifty-move rule**: `FIFTY_MOVE_HALFMOVES: u8 = 100`. Checked after move generation has already
ruled out checkmate/stalemate (mate on the exact hundredth ply must still win).
`is_fifty_move_draw(board) -> bool`, a free function.

**Insufficient material**: King vs King; King+Knight vs King; King+Bishop vs King; King+Bishop vs
King+Bishop only when same-colored. King+2 Knights vs King is deliberately excluded (unforceable
mate, but not literally impossible). `is_insufficient_material(board) -> bool` lives in
`search.rs`, not `eval.rs` (efficiency and terminal-condition-detection-is-search's-job, matching
`is_repetition`). Needs no checkmate-priority guard. `ply > 0` guard is required, same bug class
as repetition.

## 7. Iterative deepening and time management

Depth `1, 2, 3, ...` up to `MAX_ITERATIVE_DEPTH = 10`, each a fully independent, from-scratch
search. The move played is whatever the last **fully completed** depth found (an aborted
iteration is discarded wholly, score and move together). Depth 1 always runs to completion
regardless of time budget (`deadline: None`); depths 2+ are gated by both a headroom check and a
hard deadline. `choose_best_move(board, ..., budget: Option<u64>) -> (Vec<IterationReport>,
Option<Move>)` drives the loop. `Go` has `depth: Option<u32>`; `target_depth =
depth.unwrap_or(MAX_ITERATIVE_DEPTH).min(MAX_ITERATIVE_DEPTH)`, clamped not raised.

**Superseded:** the time formula below is replaced by the one in `time-management.md` (a target
recomputed after every depth from best-move instability, `movestogo` supported, no overhead).

**Time formula**: `optimum == maximum` -- one real budget number, used two ways.

```
budget    = min(time - overhead, time / left + incr)
continue? = elapsed <= budget * 0.5   -- gate before starting the next iteration
abort?    = elapsed >= budget          -- checked periodically inside the tree
```

`left = movestogo.unwrap_or(30)`, `overhead = 50ms`, headroom fraction `0.5` (Stockfish's own
value, adopted directly). `movetime` short-circuits the whole formula and takes precedence over
`wtime`/`btime` when both are present.

**In-tree abort**: `Search::deadline: Option<Instant>`, `Search::stopped: bool`. An unconditional
cheap check at the top of both `alpha_beta_with_buffer` and `quiescence`, plus a throttled check
every `CHECK_INTERVAL = 1024` nodes against `deadline`. `search_root` carries an explicit
`stopped: bool` return field (an aborted search can still hand back a plausible-looking move).

**Scope boundaries**: `infinite` just means "no deadline" -- real async `go`/`stop` handling not
implemented. None of Stockfish's adaptive time-management sophistication (best-move-change
tracking, score-stability detection) is implemented.

## 8. Reporting

**PV extraction**: `Search::pv: Vec<Vec<Move>>`, indexed by `ply`, not a triangular array --
each recursive call prepends its move onto the child's already-built line. A terminal node
explicitly clears its own `pv[ply]` row. `best_root_move` is retired, replaced by
`self.pv[0].first()`. PV never extends into quiescence.

**`seldepth`**: the deepest `ply` actually reached, `self.seldepth = self.seldepth.max(ply)` at
the top of both `alpha_beta_with_buffer` and `quiescence`.

**Time/nps**: `time` measured cumulatively from `choose_best_move`'s own `started: Instant`, not
per-iteration. `nps` is not stored, computed at format time in `uci.rs`:
`nodes * 1000 / time_ms.max(1)`.

**Forced mate**: detected by score magnitude alone, `score.abs() > MATE_SCORE - MAX_PLY`.
Conversion (`ply_at_mate = MATE_SCORE - score.abs()`, `moves_to_mate = (ply_at_mate + 1) / 2`)
happens in `uci.rs` at format time, not `search.rs`.

## 9. Benchmarking (`search_bench.rs`)

Purpose: raw search-algorithm efficiency (time/nodes to a fixed depth), never playing strength.
Metric: wall-clock time to a fixed, CLI-supplied depth (`cargo bench -- --depth N
[--stockfish[=<path>]]`), not nodes/second, not depth-reached-in-fixed-time. No `criterion`
(deliberate exception -- its repeated-statistical-sampling model doesn't fit this project's
many-position, single-pass measurement). Position suite: Stockfish's own `benchmark.cpp`
`Defaults[]` (~49 usable FENs, Chess960 excluded). Single-threaded, 64 MB transposition table.
Node count (deterministic) is the trustworthy comparison number; wall-clock timing is not
statistically controlled and can show noise-level swings.

Ethereal (native UCI) is used as a second, independent reference engine alongside Stockfish. The
size and even direction of "the gap to a top engine" depends heavily on which engine and which
depth the comparison is drawn at, not one fixed fact about Rotor's design.

**Testing methodology**: unit tests check for bugs, not performance -- a test that only fails
because an unrelated technique elsewhere changed overall node counts gets deleted outright, not
patched. A genuine correctness invariant expressed via a fragile numeric proxy gets fixed to
assert the real invariant directly instead. Whole-search node-count A/B ablation testing for
search-wide techniques (killers, ordering) is not done any more -- the intended replacement (a
real UCI option per technique driving the existing bench harness, e.g. `setoption name
UseKillerMoves value false`) is filed as a "maybe," not yet designed or built.

**Frontier batching** (batched leaf evaluation for a future GPU-backed learned evaluation)
remains an unbuilt, unstarted future experiment, waiting on both alpha-beta and a learned
evaluation existing before it can be profiled at all.

## Not yet settled

- **The strength-measurement harness's tier-1 analysis** (result tally, Elo/SPRT, termination-
  reason breakdown from PGN headers) -- `cutechess-cli` itself is adopted and working; the
  analysis code that reads its output isn't written yet. This is what essentially every margin/
  threshold above is provisional pending.
- **Continuation history** and **singular extensions** -- the two named-missing search techniques
  per `TODO.md`; continuation history has its own document (`continuation-history.md`), singular
  extensions isn't designed at all yet.
- **When piece values/PST values start being tuned** (AB(lambda)/Texel-style vs. continuing
  hand-picked) -- open, not *whether* they're conceptually tunable.
- **`[piece][to]` vs `[from][to]` history indexing**, null-move `R`, and every node-count-swept
  margin constant above -- real playing-strength comparisons, blocked on the harness.
- **PV extraction's `Vec`-per-node shape vs. the classic triangular array** -- filed in
  `TODO.md`, contingent on profiling showing a reason to change.
