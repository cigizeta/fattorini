# Perft

## Design

`claude/active/move-generation.md` closed out every increment it originally scoped (all
six piece types, castling, the top-level combinator, make/unmake, legality filtering),
103 tests passing. Before merging that work, deeper verification was wanted: perft, the
standard move-generation correctness tool (CLAUDE.md rule 4/rule 2), counts the number of
legal move sequences reachable from a position at a given depth, checked against
published values for known positions. Unlike the small, hand-built FEN tests each
increment used, perft exercises every piece type, special rule, and make/unmake
interaction at once, many moves deep -- real end-to-end confidence, not a unit-by-unit
guess.

### Where legality filtering applies inside perft

Discussed and settled before writing any code: perft does **not** call `legal_moves`.
Instead it fuses the legality check directly into its own recursive make/unmake loop --
generate pseudo-legal moves, make each one, check whether the mover's own king is now
attacked, recurse only if not, always unmake afterward. This is the same "efficient,
standard" shape `move-generation.md` explicitly deferred to "a future search increment"
(to avoid paying for make/unmake twice per legal move -- once to test legality, again to
actually explore it). Perft turns out to be that increment's first real preview: it's a
full tree walk with the same performance profile as search, so it earns that pattern now.

```rust
pub fn perft(board: &Board, depth: u32) -> u64 {
    if depth == 0 {
        return 1;
    }
    let color = board.side_to_move();
    let mut scratch = *board;
    let mut nodes = 0;
    for mv in pseudo_legal_moves(board, color) {
        let undo = scratch.make_move(mv);
        if !is_in_check(&scratch, color) {
            nodes += perft(&scratch, depth - 1);
        }
        scratch.unmake_move(mv, undo);
    }
    return nodes;
}
```

Needed `Board::side_to_move() -> Color`, which didn't exist yet -- every move-generation
function takes `color` explicitly instead (anticipating generating for either side), but
perft always walks the game exactly as it's being played, so it reads this directly.

### CLAUDE.md rule 6 amended: a narrow perft-dev-binary exception

Real, deep perft verification (depth 5-6, millions to 100M+ node visits for standard test
positions) doesn't belong in the routine `cargo test` suite -- it would slow down every
future test run for the life of the project. But rule 6 ("UCI as the only external
interface -- no GUI, no custom CLI beyond what UCI needs") ruled out a standalone perft
binary as originally written.

Discussed explicitly, two options: (a) build a minimal UCI command loop now, exposing
perft the way real engines conventionally do (`go perft <depth>`) -- fits rule 6 as
written, but pulls a whole separate, currently-unplanned increment (a real UCI loop)
into this branch just to reach it; (b) amend rule 6 with a narrow, explicit exception for
a perft-only dev binary. Chose (b). Rule 6 now reads (see the file itself for the exact
wording): UCI remains the only interface for actually playing/analyzing a game, with one
carve-out for a perft-only dev binary, since perft is a deliberately-invoked deep
verification run, not a pass/fail assertion, and has nothing to do with what UCI itself
does.

### Crate restructuring: `src/lib.rs` added

A second binary (`perft`) needs `board`/`movegen`/`perft` too, but those modules were
declared with plain `mod` (not `pub mod`) directly inside `src/main.rs`, private to that
one binary. Two ways a second binary could get at them: duplicate the `mod` declarations
inside `src/bin/perft.rs` (compiles each module's source a second time, as an entirely
separate copy -- not just ugly, architecturally wrong), or move them into a library crate
both binaries depend on. Not really a contested fork -- the library crate is simply the
correct, standard Rust answer once a project has more than one binary sharing code.

`src/lib.rs` added (`pub mod board; pub mod movegen; pub mod perft;`); `src/main.rs`'s
`mod board; mod movegen;` lines removed (unused there currently -- `main` is still just
`println!("Hello, world!")`, a placeholder, and doesn't reference either module yet).
`src/bin/perft.rs` is auto-discovered by Cargo (any file under `src/bin/` becomes its own
binary target named after the file, no `Cargo.toml` changes needed) and depends on the
library crate via `use rotor::board::Board;` / `use rotor::perft::perft;`.

Side effect: `cargo test` now reports each compilation target separately (library tests,
each binary's own tests, doc-tests) instead of one combined block -- more output lines,
same tests, nothing lost. This is standard, unavoidable behavior once a crate has a lib
plus multiple bins, not a regression.

### Testing split: fast function tests vs. deep verification runs

Discussed explicitly: perft "cannot be a test we run with cargo test" (the developer's
own framing) -- meaning the *deep, purpose-built verification runs* (multiple depths,
multiple standard test positions, run deliberately before things like a merge) don't
belong baked into the routine test suite as `#[test]`s, matching the new CLAUDE.md
carve-out's own reasoning.

Distinct from that: `perft()` the *function* still gets a couple of ordinary, fast
`#[test]`s in `perft.rs` itself (depth 0-3 against the starting position, using among the
most widely published, independently-verified numbers in all of chess programming --
CLAUDE.md rule 2) -- proving the function's own basic correctness as a piece of code,
the same way every other file in this codebase is tested, not attempting the deep
verification job the dev binary exists for.

### The `perft` dev binary

`src/bin/perft.rs`: `cargo run --bin perft -- <depth> [fen-or-name]` (defaults to
"startpos" when omitted), prints node count, elapsed time, and nodes/sec. Kept
deliberately minimal -- no "divide" (per-first-move breakdown, the standard companion
tool for pinpointing exactly which branch disagrees with a published value) yet; a
natural future addition if a real discrepancy ever needs debugging, not built speculatively
now.

All six standard published test positions (Chess Programming Wiki's "Perft Results" --
"startpos", "kiwipete", "pos3" through "pos6", each chosen to stress something ordinary
play doesn't reach for many moves) added as named constants in the binary itself,
selectable by name instead of pasting a FEN by hand; anything not matching a name falls
back to being parsed as a literal FEN. Live-fetched from the wiki rather than typed from
memory, given these are less universally memorized than the starting position's own
values.

### Speed comparison against Stockfish, and bulk counting

After verification, wanted a speed sense check -- not competing with Stockfish (an
explicit non-goal), but wanting to confirm Rotor is "reasonably fast," not accidentally
pathological.

**Threads considered, then ruled irrelevant.** Before comparing, confirmed (via
Stockfish's own source, `engine.cpp`, current master) that `Engine::perft` calls
`Benchmark::perft` directly with no thread-pool dispatch at all -- Stockfish's `go perft`
is always single-threaded, completely independent of the `Threads` UCI option. Since
Rotor's `perft` is also single-threaded (no parallelism attempted), the comparison
already run was apples-to-apples on that front without needing to force any setting on
either side.

**Measurements** (Stockfish timed externally via the shell's `time`, since `go perft`
doesn't print elapsed time itself -- this includes Stockfish's own process startup and
NNUE-file loading, which perft doesn't use at all, so its true perft-only throughput is
understated here, if anything):

| Position | Rotor | Stockfish (wall time, incl. ~small startup overhead) |
|---|---|---|
| Startpos, depth 6 | 55.4M nodes/sec | ~184M nodes/sec |
| Kiwipete, depth 5 | 50.7M nodes/sec | ~248M nodes/sec |

Roughly 3.3x-4.9x slower than Stockfish -- expected, and not concerning on its own, given
Stockfish's years of micro-optimization Rotor was never trying to match.

**Bulk counting.** One concrete, well-understood technique Rotor's `perft` was missing:
at `depth == 1`, instead of recursing into `perft(&scratch, 0)` for every legal move
(which trivially hits the base case and returns 1), just add 1 directly. Precisely
scoped before implementing: this does **not** skip any `pseudo_legal_moves` calls or
their heap allocations (Rotor's `depth == 0` base case already generated no moves at all,
so there was nothing to skip there) -- it only skips one function call and comparison
per leaf. A real but modest, constant-per-leaf saving, not a structural one; framed
honestly as such before measuring, per rule 5.

Measured: **~4.7%** faster on startpos depth 6 (55.4M -> 58.1M nodes/sec), **~6.3%**
faster on Kiwipete depth 5 (50.7M -> 53.9M nodes/sec) -- consistent with the modest
gain predicted, given run-to-run variance in the "before" numbers was itself a few
percent. `cargo test` reconfirmed unaffected (105 tests, still passing -- bulk counting
cannot change the actual counts, only how they're computed).

**A third comparison: Ethereal.** For a broader "pride check" (the developer's own
framing) beyond just Stockfish, also cross-checked against `ethereal-chess` (Ubuntu's
package for Ethereal, a genuinely well-regarded, competitively-rated open-source UCI
engine) -- installed and run the same black-box-oracle way as Stockfish (rule 1 as
clarified above). Discovered its perft command is a bare `perft <depth>`, not `go perft
<depth>` -- not every engine follows Stockfish's exact non-standard convention here, so
this needed a small diagnostic step (`echo -e "uci"`, confirming a working UCI
handshake, before finding the right command). Its output also matched exactly
(119,060,324 / 193,690,690), a third independent correctness cross-check for both
positions, on top of the speed comparison.

Result: Rotor was measured *faster* than Ethereal on both positions (~58.1M vs ~42.9M
nodes/sec on startpos depth 6; ~53.9M vs ~44.1M on Kiwipete depth 5). Worth being
precise about what this does and doesn't show, rather than over-claiming: Ethereal's
reputation rests on its *search* (pruning, move ordering, evaluation, transposition
tables) -- perft exercises none of that, only raw move generation and make/unmake, a
different code path its authors likely invested comparatively little optimization
effort into (a correctness/debugging tool, not the thing its competitive strength rests
on). So this result supports "Rotor's move generation happens to be faster than
Ethereal's move generation," not "Rotor is a faster engine than Ethereal" -- only the
narrower claim is actually what was measured. Stockfish remains dramatically faster than
both, for the same reason its move generation specifically has also had enormous
optimization effort, not just its search.

**Two dead ends: Glaurung and Toga II.** Tried two more engines from Ubuntu's
repositories for a broader sense of the landscape -- `glaurung` (genuinely significant:
the direct ancestor Stockfish was forked from in 2008, a "how far has this lineage come"
data point) and `toga2` (Toga II, a well-regarded mid-2000s engine built on Fruit's
codebase). Neither implements a `perft` command through UCI at all: `go perft <depth>`
was parsed by both as an ordinary, unbounded `go` search (no depth/time limit given, so
it just kept iterating deeper with no natural stopping point), and Toga II never even
got to processing a follow-up bare `perft <depth>` command as a result. Useful negative
result: `perft` support is a convention Stockfish and Ethereal happen to share, not
something the UCI protocol itself mandates -- concluded the cross-engine exploration
here rather than chasing more engines' idiosyncratic command sets, with two solid real
comparisons (Stockfish, Ethereal) already in hand.

The larger, unexplored lever: `pseudo_legal_moves` and each per-piece-type generator
allocate a fresh `Vec<Move>` on every single call, at every node, at every depth greater
than 0 -- almost certainly the dominant cost, far more than the function-call overhead
bulk counting addressed. Not pursued in this pass; noted as the natural next place to
look if closing more of the gap to Stockfish is ever wanted, but a materially bigger,
more invasive change (touching every `movegen/*.rs` generator) than bulk counting was,
so it gets its own explicit discussion first rather than being bundled in here.

## Decisions summary

- Perft fuses legality checking into its own make/unmake loop rather than calling
  `legal_moves` -- the same efficient shape planned for a future search increment,
  applied here since perft has the same full-tree-walk performance profile.
- `Board::side_to_move()` added -- perft is the first place that needs "whoever's turn it
  actually is" rather than an explicit `color` parameter.
- CLAUDE.md rule 6 amended: one narrow exception to "no custom CLI beyond UCI," for a
  perft-only dev binary specifically.
- `src/lib.rs` added, turning Rotor from a single-binary into a lib-plus-two-binaries
  crate (`rotor`, the future UCI engine; `perft`, the dev tool) -- the standard Rust
  answer once more than one binary needs to share code.
- Deep, multi-depth, multi-position perft verification runs happen through the dev
  binary, on demand -- never as part of `cargo test`. `perft()` itself still gets
  ordinary, fast, shallow `#[test]`s, same as any other function in the codebase.
- Bulk counting added at `depth == 1` -- a real but modest, constant-per-leaf saving
  (skips one function call per leaf, not any move-generation allocation), scoped and
  framed honestly as such before measuring. The bigger, unexplored lever (per-node
  `Vec<Move>` allocation in every generator) is deliberately not pursued in this pass.

## Implementation progress

Done:
- `CLAUDE.md` rule 6 amended with the perft-dev-binary exception.
- `src/lib.rs` added; `src/main.rs`'s now-redundant `mod` declarations removed.
- `Board::side_to_move()` accessor added to `board/mod.rs`.
- `perft(board, depth)` implemented in a new `src/perft.rs`, with two tests (depth 0 base
  case; depths 1-3 against published starting-position values).
- `src/bin/perft.rs` dev binary implemented (depth + optional FEN in, node count/timing/
  nodes-per-second out).

`cargo test`: 105 tests, all passing (103 from the `movegen` branch's own work, plus 2
new in `perft.rs`).

Done:
- `cargo run --release --bin perft -- 6` from the starting position: exact three-way
  agreement (published value, Rotor's own perft, an independently-installed Stockfish's
  perft) on 119,060,324.
- `cargo run --release --bin perft -- 5` from Kiwipete (`r3k2r/p1ppqpb1/bn2pnp1/3PN3/
  1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1`), the standard test position stressing
  castling/promotion/en passant: exact three-way agreement on 193,690,690.
- All six standard published test positions added as named constants in
  `src/bin/perft.rs` ("startpos", "kiwipete", "pos3"-"pos6"); depth 5 against the
  remaining four (pos3-pos6) matched their published values exactly.

Nothing left pending -- perft verification is complete.

## Progress log

- 2026-09-10: Design and implementation as described above. `cargo test` confirmed
  passing: 105 tests. Real depth-5/6 verification run(s) via `cargo run --bin perft`
  not yet performed.
- 2026-09-10 (continued): before running real verification, the published starting-
  position values (20/400/8,902/197,281/4,865,609/119,060,324 at depths 1-6) were
  live-confirmed against the Chess Programming Wiki rather than trusted from memory
  alone. Separately, using an installed Stockfish as a live cross-check for perft counts
  came up against rule 1's "or even as a reference implementation to check against"
  wording -- discussed explicitly, and clarified (developer's own reading, affirmed):
  rule 1 governs how Rotor's own chess logic gets *written* (never copied from, modeled
  on, or informed by another engine's implementation), not how results get *verified*
  afterward. Running Stockfish purely as a black-box oracle -- its source and algorithms
  never consulted, only a final node count read off and compared -- doesn't touch the
  writing process at all. No rule amendment needed; rule 1 already permits this as
  written.
- 2026-09-10 (continued): `cargo run --release --bin perft -- 6` from the starting
  position: **119,060,324 nodes** in 2.24s (~53M nodes/sec) -- an exact match to the
  published depth-6 value. Strong end-to-end confirmation of everything built across the
  `movegen` branch at once.
- 2026-09-10 (continued): independently cross-checked against a freshly-installed
  Stockfish (`sudo apt install stockfish`, then `echo -e "position startpos\ngo perft 6"
  | stockfish`, per rule 1 as clarified above -- output only, never its source). Stockfish
  reported the same **119,060,324** total (and its own per-first-move breakdown, not
  compared move-by-move here but available if ever needed). Three-way agreement now:
  published value, Rotor's own perft, and an independent reference engine's perft. Perft
  verification at the starting position is complete and about as convincing as this
  branch could reasonably get.
- 2026-09-10 (continued): decided to also run "Kiwipete" (`r3k2r/p1ppqpb1/bn2pnp1/
  3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1`), the standard published test position
  specifically designed to stress castling (both sides, both colors), promotions, and en
  passant heavily and early -- exactly what the starting position doesn't reach for many
  moves. Its published depth-5 value (193,690,690) was live-confirmed against the Chess
  Programming Wiki first, then both `cargo run --release --bin perft` and an
  independently-installed Stockfish were run against it: both reported exactly
  193,690,690, matching the published value. Second exact three-way match. Perft
  verification is complete: starting position (depth 6) and Kiwipete (depth 5) both
  agree exactly across published values, Rotor's own perft, and Stockfish's perft. This
  increment, and the `movegen` branch's remaining pre-merge verification, is done.
- 2026-09-10 (continued): added all six standard published test positions ("startpos",
  "kiwipete", "pos3"-"pos6", live-fetched from the Chess Programming Wiki) as named
  constants in `src/bin/perft.rs`, selectable by name on the command line instead of
  pasting a FEN by hand. Ran depth 5 against the four not yet checked (pos3-pos6): all
  four matched their published values exactly (674,624 / 15,833,292 / 89,941,194 /
  164,075,551). All six standard positions now confirmed exact.
- 2026-09-10 (continued): a broader "pride check" against `ethereal-chess` (Ethereal, a
  well-regarded competitive UCI engine, installed via `apt`). Its perft command turned
  out to be a bare `perft <depth>`, not Stockfish's `go perft <depth>` -- found via a
  small `echo -e "uci"` handshake diagnostic first. Output matched exactly on both
  positions (a third independent correctness cross-check), and timing showed Rotor
  *faster* than Ethereal on both (~58.1M vs ~42.9M nodes/sec startpos depth 6; ~53.9M vs
  ~44.1M Kiwipete depth 5) -- caveat recorded in the design section above: this reflects
  Ethereal's perft-path specifically, not a claim that Rotor's overall engine strength
  or speed rivals Ethereal's, since perft never touches search/evaluation, the actual
  basis of Ethereal's competitive reputation.
- 2026-09-10 (continued): tried two more engines for a broader sense of the landscape --
  `glaurung` (Stockfish's direct 2008 ancestor) and `toga2` (Toga II, Fruit-derived).
  Neither supports a `perft` command via UCI at all -- `go perft N` was parsed as an
  ordinary, unbounded `go` search by both (a real practical hazard: no time/depth limit
  means no natural stopping point). Concluded the cross-engine exploration here with two
  solid comparisons in hand (Stockfish, Ethereal) rather than chasing further engines'
  idiosyncratic command sets.
- 2026-09-10 (continued): speed comparison against Stockfish, for a sense check ("not
  competing with Stockfish, but reasonably fast" -- the developer's own framing).
  Confirmed from Stockfish's own source that `go perft` is always single-threaded
  regardless of the `Threads` UCI option, same as Rotor's `perft` -- the comparison
  already run was apples-to-apples without needing to force any setting. Measured: Rotor
  ~55.4M nodes/sec (startpos depth 6) and ~50.7M nodes/sec (Kiwipete depth 5) versus
  Stockfish's ~184M/~248M (wall-time via `time`, including its own unavoidable NNUE-
  loading overhead that perft doesn't even use -- if anything understating Stockfish's
  true perft-only speed). Roughly 3.3x-4.9x slower, expected and unconcerning given
  Stockfish's years of micro-optimization. Added bulk counting at `depth == 1`
  (`perft.rs`), precisely scoped before implementing as a real-but-modest,
  constant-per-leaf saving (skips one function call per leaf, not any move-generation
  allocation -- Rotor's base case already generated no moves to skip). Measured: ~4.7%
  faster on startpos depth 6, ~6.3% faster on Kiwipete depth 5, matching the modest gain
  predicted. `cargo test` reconfirmed unaffected: 105 tests, still passing. The larger,
  unexplored lever (per-node `Vec<Move>` allocation in every generator) is noted but
  deliberately not pursued in this pass -- a materially bigger, more invasive change than
  bulk counting, left for its own explicit discussion if ever wanted.
