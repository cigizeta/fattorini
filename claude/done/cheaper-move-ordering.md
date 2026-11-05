# Cheaper move ordering

Implementation plan. Branch: `alessio`. Follows `one-evaluation-per-node.md` and
`leaf-to-quiescence.md`, which left a node's cost at about 1.7-1.9 us hand-crafted, of which the
evaluation is 20 ns.

## The profile

`perf record` on `nnue_bench --big-five --depth 9` (362 samples, enough to rank, not to quote):

- `see::attackers_to` 15.9% and `see::see` 14.0%: about 30%, SEE.
- `malloc`, `free`, `realloc`: about 8%.
- The big five (`side_value`, `inputs`, `mobility_squares`): about 11%.
- `make_move` 2.8%, history ordering 2.4%, the rest smaller.

## The causes

- **Quadratic SEE.** The main search's move loop calls `try_best_noisy_move(..., self.score)` (SEE
  in play) on every iteration. Each call scores every remaining move, quiet ones included, and it
  keeps doing so after no noisy move is left (it returns `false`, after scanning). A node of `n`
  moves pays about `n^2 / 2` SEE calls. Quiescence orders by `move_score` (MVV-LVA) and stops at
  the first failure, so it is not affected.
- **SEE allocates.** `see` builds its swap list in a `Vec` (`let mut gain = vec![...]`, then
  pushes): a heap allocation per call, and reallocation as it grows. `see_ge`, which quiescence
  calls, is a wrapper over `see`.

## Design

Settled in discussion. No behavioral change: every pick and every SEE value stays the same.

- **Stop scanning once no noisy move is left.** After `try_best_noisy_move` first returns
  `false` at a node, no remaining move scores above 0. The later stages (killers, history) only
  reorder moves, so they cannot create one. A per-node flag skips the call for the rest of the
  loop.
- **No SEE for quiet moves in the scan.** A quiet move's SEE is never positive: its `gain[0]` is 0,
  and the swap-list pass only subtracts. The scan picks only a positive best, so a quiet move can
  never be picked or displace one. Skipping its score changes no pick, provided the scan keeps
  taking the first maximum. This holds for any `score` whose quiet moves score at most 0, `see`
  and `move_score` alike; to be checked for `move_score` (the tests' scorer) and written down.
- **SEE's swap list on the stack.** A fixed array instead of the `Vec`. Its length bound is the
  number of pieces that can capture on one square in sequence: at most 32 entries, with one more
  for the initial gain. The swap-list arithmetic is unchanged.

## Steps

1. `see.rs`: `gain` as `[i32; N]` plus a length, with the bound's reasoning in a comment.
2. `try_best_noisy_move`: skip `score` on quiet moves (treat them as not positive), with the
   reasoning in its doc comment, and the first-maximum tie rule kept.
3. `alpha_beta_with_buffer`'s loop: a `noisy_exhausted` flag, set the first time the noisy
   stage returns `false`, skipping the stage once set.

## As implemented

- `move_score` gives a quiet move exactly 0, so skipping quiet moves holds for both scorers; the
  scan scores them as `i32::MIN`, which keeps the first-maximum rule.
- `see`'s swap list is `[i32; 33]` plus a length.
- `noisy_exhausted` wraps the noisy stage in the loop's priority chain.
- **No long-sequence SEE test.** The conversion is mechanical and the bound is argued in the code
  (each recapture removes its piece, at most 31); a hand-computed long sequence would be the
  error-prone part. The existing `see` tests and the fixed-depth search test cover the values.

## Tests

- **Same search.** `a_fixed_depth_search_reproduces_its_recorded_nodes_scores_and_moves`, unchanged.
- **SEE unchanged.** The existing `see` tests, plus one with a long recapture sequence on one
  square (many attackers of both colors), so the fixed array's length is exercised.
- The existing `try_best_noisy_move` tests pass.

## Verification

```
cargo test --release
```

Then `nnue_bench --big-five --depth 9`: identical node counts, lower time per node. Then a new
profile, to see what leads once SEE is cheap, and whether any allocation remains (the PV lines
built at each improvement, `quiets_searched`).

## Out of scope

- Caching each move's SEE score per node: the killer, TT-move, and history stages are function
  pointers swapping `self.moves` alone, so a parallel score buffer would change all of them. Worth
  it only if the profile still shows SEE after this.
- An early-exit `see_ge` (Stockfish's form).
- The big five's own share.
