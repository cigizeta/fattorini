# Aspiration windows

## Design

Identified as a hard prerequisite for multi-threaded search (see `threading.md`): Lazy SMP's
real thread-diversity mechanism depends on varying the aspiration window's width per thread,
which presupposes aspiration windows existing at all. Independently valuable on its own too --
a narrower window prunes more aggressively at every single-threaded depth, not just as a
stepping stone to threading.

### What this actually changes, and where

Iterative deepening currently searches every depth with the full `(-INFINITY, INFINITY)`
window (`search_root` -> `alpha_beta_with_buffer(board, 0, depth, -INFINITY, INFINITY)`).
Instead, once a previous depth's own score exists, the next depth starts from a narrow guess
-- `[previous_score - delta, previous_score + delta]` -- and only widens and retries *the same
depth* if the real score turns out to lie outside that guess (a "fail low", `score <= alpha`,
or "fail high", `score >= beta`). This is purely a root-level change: interior nodes of
`alpha_beta_with_buffer` are untouched, since aspiration windows are a top-level iterative-
deepening technique, not something recursed with.

### Reference verified against real Stockfish source, not memory

Modern Stockfish (`search.cpp`, `master`) carries real complexity that doesn't fit Rotor yet:
a `meanSquaredScore`-scaled initial delta (a volatility statistic Rotor doesn't track), a
`threadIdx % 8` diversity term (only meaningful once multiple threads exist), an
`adjustedDepth` reduction during repeated fail-highs (a separate, deferrable optimization),
and a `47 * delta / 128` growth ratio -- clearly tuned against Stockfish's own testing
infrastructure, not something to copy verbatim without understanding why.

A much better-matched reference: an older, simpler tagged release,
[`sf_10`](https://raw.githubusercontent.com/official-stockfish/Stockfish/sf_10/src/search.cpp),
from before most of that complexity existed:

```cpp
// Reset aspiration window starting size
if (rootDepth >= 5 * ONE_PLY)
{
    Value previousScore = rootMoves[pvIdx].previousScore;
    delta = Value(20);
    alpha = std::max(previousScore - delta, -VALUE_INFINITE);
    beta  = std::min(previousScore + delta, VALUE_INFINITE);
    ...
}
...
delta += delta / 4 + 5;   // growth on each fail-high/fail-low retry
```

Confirmed `Rotor::piece_value(Pawn) == 100`, the same centipawn scale these constants assume
-- borrowing here is at least dimensionally meaningful, unlike borrowing an unrelated unit
would be.

### The `depth >= 5` gate: reused directly, for a reasoned justification

Initially worried this was disproportionate to Rotor's own `MAX_ITERATIVE_DEPTH = 10` (half
the entire range, versus an early fraction of Stockfish's routine 20-40+ plies). Resolved by
reframing what the gate actually protects against: it isn't "wait until some fraction of our
typical ceiling," it's "wait until iterative deepening's own score has stopped swinging
wildly between iterations" -- a property of how few plies have been searched so far, not of
how deep the engine eventually reaches. A depth-2 score is volatile regardless of whether the
engine tops out at 10 or 40, since almost nothing tactical has resolved either way. Combined
with the fact that depths 1-4 cost so little wall-clock time for Rotor that gating them off
loses nothing real regardless of what fraction of the nominal depth count they represent --
**settled: reuse `depth >= 5` directly**, now for a reasoned justification rather than a
borrowed-because-convenient one.

### The initial delta: no principled derivation exists anywhere, so `50` is an honest placeholder

Checked how Stockfish actually arrives at constants like `20` at all, rather than assume a
formula exists: a real commit,
["Tweak initial aspiration window"](https://github.com/official-stockfish/Stockfish/commit/a0259d8ab9661b7f625474d2cbe18481ef69bbf2),
shows the entire justification given for a change to this exact mechanism is a pair of SPRT
self-play test results (`LLR: 2.97 (-2.94,2.94) ...`), not a formula or first-principles
argument. Every constant in this area of Stockfish's search is empirically tuned against
`fishtest`'s distributed self-play infrastructure -- which is precisely the "decades of
distributed self-play tuning" Rotor's own `CLAUDE.md` opens by saying it explicitly has none
of. `20` was never validated against anything Rotor has.

The developer's own reasoning for departing from it: Rotor's evaluation (material/PST/
mobility/pawn structure/king safety) has no NNUE-level positional judgment at the leaf the
way Stockfish does, so it plausibly can't anticipate an approaching tactic or positional
swing the way a trained network already partially can -- meaning Rotor's own scores likely
swing more between successive iterative-deepening depths, and a window sized for Stockfish's
own smaller, NNUE-smoothed swings would cause excessive fail-high/fail-low churn. Plausible,
not measured (a real measurement would mean temporary, rule-7-style instrumentation printing
real depth-to-depth score deltas on representative positions) -- the developer chose to
proceed with a directionally-reasoned placeholder now rather than block on gathering that
data first. **Settled: `ASPIRATION_INITIAL_DELTA = 50`**, explicitly weaker-justified than
even `Hash`'s `16` MB default (a real, load-bearing GUI convention) -- closer in kind to
`DEFAULT_MOVES_LEFT = 30`, a placeholder to revisit once real measurement exists, most
naturally through the same deferred self-play strength-testing harness already in `TODO.md`
(which now also carries a related, separately unverified hypothesis: that Rotor's cheaper
per-node evaluation might make its raw nodes/second competitive with Stockfish's even where
search quality isn't).

### The widening formula: doubling, not Stockfish's `/4 + 5`

`sf_10`'s `delta += delta / 4 + 5` has two components worth separating:
1. **Multiplicative growth** (`delta / 4`, roughly +25% per retry) -- a real, portable
   justification: it bounds the worst-case retry count *logarithmically* rather than
   linearly. Growing from `50` by ~25% each time covers the full `+-MATE_SCORE` (30,000)
   range in roughly 30 retries; flat additive growth (e.g. always `+25`) could need well over
   a thousand retries in the worst case, each one a real re-search.
2. **The additive `+5` floor** -- justified in Stockfish's own code as guarding against
   integer-division truncation stalling growth entirely once `delta` gets small (`delta / 4`
   rounds to `0` once `delta < 4`, which would stop growth and never terminate without a
   floor).

Caught directly in discussion: **the `+5` floor's justification doesn't apply to Rotor at
all**. `delta` starts at `50` and only ever grows monotonically across retries within one
depth -- it can never revisit a value anywhere near small enough for `delta / 4` to truncate
to zero. Keeping `+5` would be carrying a fix for a problem that cannot occur here. That
removes the only real justification either borrowed constant (`1/4`, `5`) had beyond "this is
what tested well for Stockfish's own engine at some point in its history" -- exactly the same
unprincipled-for-Rotor category `20` itself turned out to be.

**Settled: plain doubling, `delta *= 2`.** Keeps the one genuinely portable insight
(multiplicative growth for a logarithmic retry bound) while dropping every borrowed magic
number -- doubling needs no justification beyond being the standard exponential-growth idiom,
understood on its own terms, and needs no separate floor term at all (a positive number can
never round down to a smaller one by doubling, unlike dividing).

### The actual shape, in `search_root`

```rust
let (mut alpha, mut beta, mut delta) = match previous_score {
    Some(score) if depth >= ASPIRATION_MIN_DEPTH => {
        ((score - ASPIRATION_INITIAL_DELTA).max(-INFINITY),
         (score + ASPIRATION_INITIAL_DELTA).min(INFINITY),
         ASPIRATION_INITIAL_DELTA)
    }
    _ => (-INFINITY, INFINITY, ASPIRATION_INITIAL_DELTA), // depth < 5, or no prior score yet
};

let score = loop {
    let attempt = search.alpha_beta_with_buffer(board, 0, depth, alpha, beta);
    if attempt <= alpha {
        beta = alpha;
        alpha = (attempt - delta).max(-INFINITY);
    } else if attempt >= beta {
        alpha = (beta - delta).max(alpha);
        beta = (attempt + delta).min(INFINITY);
    } else {
        break attempt;
    }
    delta *= 2;
};
```

The same `Search` instance is reused across retries within one depth, not rebuilt per
attempt -- it's genuinely the same depth being re-attempted, so `killers`/`history` learned
during a failed attempt remain valid, relevant hints for the immediate retry, unlike the
cross-*depth* case where staleness is a real (if still accepted) concern. For `depth <
ASPIRATION_MIN_DEPTH` or no `previous_score` yet, `alpha`/`beta` start at the full sentinel
range -- since no real score can ever reach `i32::MAX`, the fail-high/fail-low checks can
never trigger, so the loop always succeeds on its first attempt. That's not a special case in
the code; it falls out of the same loop unconditionally, and matches today's exact
single-attempt behavior for those depths.

New parameter: `search_root` gains `previous_score: Option<i32>`. `choose_best_move` threads
it exactly the way it already threads `killers`/`history` across its `for depth in
1..=max_depth` loop -- `None` initially, set to `Some(score)` after each iteration that
actually completes (an aborted iteration's own score is already known unreliable and
`choose_best_move` breaks immediately on `stopped` regardless, so there is never a "next
depth" that would need a previous score from an aborted one).

New constants: `ASPIRATION_MIN_DEPTH: u32 = 5`, `ASPIRATION_INITIAL_DELTA: i32 = 50`,
alongside the existing `MATE_SCORE`/`INFINITY`/`MAX_PLY`.

### Two things confirmed to compose correctly with no extra work

- **The transposition table handles root retries correctly with no special-casing.** A
  fail-high retry re-probes the same root position, which the previous (failed) attempt
  already stored a real `Lower`-bound entry for. The probe compares that stored bound against
  whatever `alpha`/`beta` *this* call actually received, never a cached notion of "the same
  window as before" -- so it either legitimately short-circuits the retry (if the stored
  score also fails the new, wider window, which is still a true fact) or falls through to a
  normal re-search. Correctness holds either way; this is just an ordinary transposition, not
  a special case the aspiration loop needs to reason about.
- **Termination is guaranteed without an explicit retry cap.** `delta` doubles from `50` each
  retry, exceeding any real achievable score (bounded by `MATE_SCORE = 30_000`) within about
  10 iterations, at which point `alpha`/`beta` are already clamped to the full sentinel range
  and no further failure is possible. No `i32` overflow risk either -- `delta` never needs to
  approach anywhere near `i32::MAX`.

## Not yet settled / deferred

- **`ASPIRATION_INITIAL_DELTA = 50` is a directionally-reasoned but unmeasured placeholder.**
  Revisit once real data exists -- either the deferred self-play strength-testing harness in
  `TODO.md`, or narrower temporary (rule 7) instrumentation printing Rotor's own real
  depth-to-depth score deltas on representative positions.
- Everything else in this document reads as settled, not deferred -- unlike `tt.md`/
  `threading.md` at equivalent points in their own design discussions, there is no other open
  fork left blocking an implementation attempt.

## Progress log

Design discussion complete, following the project's established methodology (verify every
real-engine claim against actual source rather than memory; never invent an unjustifiable
number where a better-grounded alternative exists; land the naive version first). Verified
against: Stockfish's real `search.cpp` (both current `master` and the simpler historical
`sf_10` tag), and a real Stockfish commit demonstrating that even Stockfish's own constants
in this exact mechanism are SPRT-tuned empirically, not derived. Rotor's own
`piece_value(Pawn) == 100` confirmed to match the centipawn scale these constants assume.

Settled: the retry-loop mechanism and its placement (inside `search_root`, reusing one
`Search` instance across retries); the `depth >= 5` gate (reused directly, with a reasoned
justification rather than a borrowed one); the initial delta (`50`, an explicit placeholder,
larger than Stockfish's own `20` on the developer's own reasoning that Rotor's weaker
evaluation likely produces noisier inter-depth scores); the widening formula (plain doubling,
deliberately dropping both of Stockfish's own borrowed constants once one of them -- the `+5`
floor -- was shown not to even apply to Rotor's own starting conditions); the new
`search_root`/`choose_best_move` state threading (`previous_score: Option<i32>`, parallel to
`killers`/`history`); and composability with the existing transposition table (verified to
need no special-casing).

Implemented in `search.rs`: `ASPIRATION_MIN_DEPTH`/`ASPIRATION_INITIAL_DELTA` constants, the
retry loop in `search_root`, and `previous_score` threaded through `choose_best_move` exactly
as designed above.

A real bug was found and fixed during implementation, not just in review: once
`search.stopped` is set (a deadline tripping mid-search), `alpha_beta_with_buffer`'s own
unconditional top-of-function check returns its `alpha` parameter exactly, on every
subsequent call. That satisfies the retry loop's `attempt <= alpha` fail-low condition by
plain equality, which -- without an explicit check for it -- would make the loop "retry"
forever once stopped, doubling `delta` without bound until it overflows `i32`. Fixed by
checking `search.stopped` immediately after each attempt and breaking out before the fail-
low/fail-high comparisons ever run; the exact value returned in that case is never used
regardless, since `choose_best_move` already discards a stopped iteration's score/pv
entirely. Covered by a dedicated regression test
(`search_root_aspiration_retry_terminates_cleanly_once_stopped`) using an already-past
deadline, alongside two others: `search_root_ignores_previous_score_below_the_aspiration_
minimum_depth` (confirms the gate) and `search_root_converges_to_the_correct_score_after_a_
forced_fail_high` (confirms a forced retry still lands on exactly the same score `alpha_beta`'s
own full-window reference implementation produces -- the core correctness invariant this
whole technique must never violate).

Confirmed: `cargo build && cargo test` passes, 307 tests (304 prior + the 3 new ones above),
no regressions. This item is complete. Next: return to `threading.md`'s own remaining open
items (generation bump timing, the atomic `Entry` layout, thread count/result-selection) now
that this prerequisite is actually landed, not just designed.
