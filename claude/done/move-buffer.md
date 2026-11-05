# Shared move buffer

## Design

`claude/done/perft.md` flagged, but deliberately didn't pursue, the largest remaining
speed lever found while comparing Rotor's perft against Stockfish/Ethereal: every call to
`pseudo_legal_moves` allocates a fresh `Vec<Move>`, and internally each of the seven
per-piece-type generators (`pawn_moves`, `knight_moves`, `bishop_moves`, `rook_moves`,
`queen_moves`, `king_moves`, `castling_moves`) *also* allocates its own separate
`Vec<Move>` before being merged in via `.extend(...)`. Up to 8 heap allocations per call,
at every node of a perft (or, eventually, search) tree -- potentially billions of times
in a real run. This branch replaces that with one shared, reused buffer.

### The mechanism: a stack-shaped shared `Vec<Move>`

Move generation is recursive (`perft` calls itself; `legal_moves`/future search would
too), which matters for how the shared buffer has to behave: it can't just grow forever
(that would try to hold every move from every node ever visited across an entire perft
run), it has to grow and shrink in step with the recursion, like a stack.

Settled mechanism, discussed at length before touching any code: a single `moves: &mut
Vec<Move>` threaded through every call. Each call records `let start = moves.len();` as
a plain local variable the instant it begins -- its own "marker" -- generates its own
moves by pushing onto the end of `moves`, iterates `moves[start..]` (ignoring anything a
child call pushes further along and later pops off before returning), and calls
`moves.truncate(start);` right before returning, restoring the buffer to exactly the
state the caller left it in.

Explicitly considered and rejected: a separate "companion Vec of depth markers" (the
developer's own first instinct, drawing on a prior attempt at a SAT solver's decision-
level trail). Not needed here, because `perft` is genuinely *recursive* -- Rust's own
call stack already gives every call its own local `start` variable for free, disappearing
automatically when that call returns. A SAT solver's CDCL needs an explicit trail because
non-chronological backtracking jumps back across multiple decision levels at once (further
than a single `return` can unwind); plain tree recursion never needs to jump further back
than its immediate caller, so there's nothing an explicit second stack would add here.
The child call never needs to know its parent's `start` either -- it computes its own the
same way, guaranteed correct since nothing else touches `moves` between the parent
finishing its own generation and the child beginning.

```rust
pub fn perft(board: &Board, depth: u32, moves: &mut Vec<Move>) -> u64 {
    if depth == 0 {
        return 1;
    }
    let start = moves.len();
    let color = board.side_to_move();
    pseudo_legal_moves(board, color, moves);
    let end = moves.len();

    let mut scratch = *board;
    let mut nodes = 0;
    for i in start..end {
        let mv = moves[i];
        let undo = scratch.make_move(mv);
        if !is_in_check(&scratch, color) {
            nodes += if depth == 1 { 1 } else { perft(&scratch, depth - 1, moves) };
        }
        scratch.unmake_move(mv, undo);
    }

    moves.truncate(start);
    return nodes;
}
```

### Scope: every generator's signature changes

The allocations don't live in `perft` -- they live inside `pseudo_legal_moves` and each
of the seven generators. Eliminating them means converting every one of those functions
from `fn xxx_moves(board: &Board, color: Color) -> Vec<Move>` (builds and returns its own
Vec) to `fn xxx_moves(board: &Board, color: Color, moves: &mut Vec<Move>)` (pushes
directly into the caller's buffer, returns nothing). This is a materially bigger, more
invasive change than the earlier bulk-counting tweak -- it touches every file in
`movegen/`, not one function in one file -- discussed and confirmed explicitly before
starting, rather than scoped down to just `perft`'s own hot path.

One pleasant discovery while planning: `pawn.rs`'s own private helpers
(`generate_pushes`/`generate_captures`) *already* take a `moves: &mut Vec<Move>`
parameter and push into it -- `pawn_moves` itself just happens to create that Vec locally
and return it afterward. So converting `pawn_moves`'s own signature to accept the buffer
from its caller, instead of creating one internally, is a smaller, more natural change
there than in the other generators, which currently build their Vec directly in a loop.

Cascades to both current consumers:
- `pseudo_legal_moves` (`movegen/mod.rs`): no longer builds+returns its own Vec, just
  calls each generator with the shared buffer -- no `.extend()` merging needed either,
  since every generator already writes into the same target.
- `legal_moves` (`movegen/legal.rs`) and `perft` (`perft.rs`) both adopt the
  `start`/`truncate` pattern above, replacing `for mv in pseudo_legal_moves(board,
  color) { ... }` (iterating a freshly-returned owned Vec) with generating into a shared
  buffer and iterating `moves[start..end]`.

And touches every existing test across all seven generator files plus `mod.rs`'s own two
and `legal.rs`'s four -- every `let moves = xxx_moves(&board, color);` call site needs
rewriting to the new calling convention. Mechanical, but numerous (30+ individual test
functions).

## Decisions summary

- One shared `Vec<Move>`, passed as `&mut Vec<Move>` through every call, used stack-
  fashion: each call's own `start = moves.len()` (a plain local variable, not a separate
  tracked structure) marks where its own slice begins; `moves.truncate(start)` before
  returning restores the buffer for whoever called it.
- No separate "depth markers" Vec -- recursion's own call stack already provides one
  local marker per active call, for free, since each call only ever needs its own start
  point, never anything about its ancestors or descendants.
- Full scope: every one of the seven per-piece-type generators, `pseudo_legal_moves`,
  `legal_moves`, and `perft` all convert from returning `Vec<Move>` to writing into a
  caller-provided buffer. Confirmed explicitly rather than narrowing to just `perft`'s
  own hot path.

## Implementation progress

Done. All seven per-piece-type generators (`pawn_moves`, `knight_moves`, `bishop_moves`,
`rook_moves`, `queen_moves`, `king_moves`, `castling_moves`), `pseudo_legal_moves`, and
`perft`'s internal recursive helper now write into a shared, caller-provided buffer
instead of each allocating and returning its own `Vec<Move>`. `legal_moves` keeps its own
public `-> Vec<Move>` signature (it's an explicitly non-hot-path utility, not called
recursively), but now uses the buffer-based `pseudo_legal_moves` internally via one local
buffer. `perft`'s public signature also stayed unchanged (`perft(board, depth) -> u64`),
delegating to a private `perft_with_buffer` that implements the actual `start`/`truncate`
pattern. Every existing test across all seven generator files, `mod.rs`, and `legal.rs`
was rewritten to the new calling convention; no test assertions changed, only how each
test builds its `moves` buffer. `cargo test` confirms 105 passed -- identical count to
before the conversion, as expected for a pure internal restructuring with no behavior
change.

Correctness re-verified via the `perft` dev binary on all six standard positions:
startpos depth 6 (119,060,324) and Kiwipete depth 5 (193,690,690) match both the
published values and the pre-conversion run exactly; pos3-pos6 at depth 5 (674,624 /
15,833,292 / 89,941,194 / 164,075,551) all match their published values too.

Speed re-measured (`cargo run --release --bin perft`) against the pre-conversion
baseline:

| position          | before (nodes/sec) | after (nodes/sec) | gain  |
|-------------------|---------------------|--------------------|-------|
| startpos depth 6  | 55.4M               | 78.3M              | +41%  |
| Kiwipete depth 5  | 50.7M               | 78.5M              | +55%  |

A real, substantial gain, not the modest constant-factor kind bulk counting produced --
consistent with the prediction that eliminating up to 8 heap allocations per node (rather
than just one function call per leaf) would matter once the tree gets deep.

## Progress log

- 2026-09-10: `move-buffer` branch created (from `main`, after the `movegen` branch
  merged). Design discussed and settled as described above, largely driven by the
  developer's own proposed mechanism (a shared Vec plus stack-like markers, drawing on a
  prior SAT-solver attempt), refined into "recursion's own call stack replaces the
  explicit marker stack" once confirmed the target (`perft`, and future search) is
  genuinely recursive rather than needing CDCL-style non-chronological jumps.
- 2026-09-10: Conversion implemented across all seven generators, `pseudo_legal_moves`,
  `legal_moves`, and `perft`, in the planned order (knight/king, then pawn, then the
  sliding pieces, then castling, then the three consumers). `cargo test` confirms 105
  passed. Correctness re-verified against all six standard positions (startpos depth 6,
  Kiwipete depth 5, pos3-pos6 depth 5) -- all still match published values exactly -- and
  speed re-measured against the pre-conversion baseline: +41% on startpos depth 6 (55.4M
  to 78.3M nodes/sec), +55% on Kiwipete depth 5 (50.7M to 78.5M nodes/sec). This closes
  out the branch's stated goal.
