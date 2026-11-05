# Labels are White-relative, features are side-to-move relative

## Status

**Fixed, rebuilt, and confirmed.** Found by running `correlate`/`check` on amanda's champion
checkpoint after three epochs: `r_squared = 0.0731`, `correlation = 0.2703`, mean absolute error
`201.9` cp against a coin-flip baseline of `214.8` cp. A network that had learned only material should
score far better than that against these labels, which is what prompted reading the label path end to
end.

After the fix, a dataset rebuild and one fresh epoch, the same measurement on the same 423190 test
rows reads `correlation = 0.9330`, `r_squared = 0.8705`, and mean absolute error `73.9` cp against the
same `214.8` cp coin-flip baseline -- the broken run's `201.9` cp was 6% better than a coin flip, this
is 66% better. Test MSE over one epoch fell from about
`0.30` to `0.0403` at an identical step count -- the *worst* arm of the fixed grid (`1e-5`, `0.0602`)
beats the *best* arm of the broken one by five times. The prediction this document committed to,
"a different order of magnitude, not a few points", held. The architecture was never on trial.

## The defect

Every feature is side-to-move relative. `features.rs`'s `oriented_square(square, side_to_move)` flips
the board for the mover, and every identity is `piece_type * 2 + (0 if own else 1)` where "own" means
`color == side_to_move`. Naive additionally has **no side-to-move input at all**, deliberately:
`rotor-naive.md` argues one is unnecessary precisely because the label is supposed to mean "expected
score for the side to move". `halfkp`/`halfka_v2_hm` express the same thing through their own/opponent
perspectives.

Every label is White-relative:

1. `self_play.rs` writes each move's score as the searching side's own score -- side-to-move relative,
   as a search reports it.
2. `label_and_featurize_self_play_game` converts those to White-relative (`if mover_is_white { cp }
   else { -cp }`). Correct, and necessary: the blend below mixes plies of both colours with a
   game-level outcome, so it has to happen in one colour-invariant frame.
3. `compute_labels` blends White-relative scores with the White-relative outcome (`2 * outcome - 1`)
   and returns White-relative labels. Its own doc comment says so twice.
4. The label is then stored against `extract_canonical(&board)` with **no conversion back**.
5. `to_dense_batch` copies `record.label` through untouched -- there is a test named
   `to_dense_batch_carries_the_label_through_unchanged`.

`mover_is_white` occurs exactly once in `src/`, at step 2. Nothing in the codebase ever negates a
label.

### Why that caps R^2 near zero

The AB(lambda) recursion puts weight `0.3 * 0.7^k` on `score_{t+k}`, summing to 1, so the effective
horizon is about 3.3 plies: a label is essentially a locally smoothed version of the generator's own
search score, and that generator is material plus piece-square tables. Material therefore dominates
the label, and material is exactly what a single linear layer over these features computes -- each
piece contributes two rows at its own identity.

On White-to-move rows, "own material up" means label up. On Black-to-move rows it means label down.
The dataset is essentially half and half, so the material signal cancels. Worse, the information
needed to reconcile the two frames (which colour is to move) **is not in the input at all**, so the
network cannot learn the flip either. What is left is only the part of the label that is symmetric
under own/opponent relabeling, and `r_squared = 0.073` is what that residual looks like.

### The outcome is not the problem

Worth stating because it is the obvious suspect. `self_play.rs` records the result as White's score in
the `0/0.5/1` convention and `compute_labels` remaps it to `-1/0/+1`, which is the same frame the
scores were just converted into. Both inputs to the blend agree, and they must: the outcome is a
game-level constant while the recursion walks alternating colours. The missing step is the conversion
of the blended result back out, once per position.

The two ways of writing that are provably identical, which is what makes the one-line fix the complete
fix. With `W_t` the White-relative label and `s_t = +1` when White is to move at ply `t`, the current
recursion is `W_t = 0.7 * W_{t+1} + 0.3 * score_t^White`. Multiplying by `s_t`, and using
`s_{t+1} = -s_t` and `s_t * score_t^White = score_t^stm`:

```
S_t = -0.7 * S_{t+1} + 0.3 * score_t^stm
```

the negamax form of the same recursion. Clamping to `+/-(1 - CLAMP_EPS)` is symmetric so it commutes
with the negation, and the decisive branch takes `+/-1` from the White-relative score's own sign, so it
flips correctly too. The virtual anchor needs no special treatment either: it enters as `L_n` in the
White frame and the per-ply flip covers every emitted label uniformly.

## The fix

**A helper in `labels.rs`, applied where a label meets a board, taking its sign from the same
`CanonicalPosition` that produces the features:**

```rust
pub fn side_to_move_relative(white_relative: f64, side_to_move: Color) -> f64
```

Deriving the sign from the position being written -- rather than from ply parity, or from a
`starting_side_to_move` parameter threaded down -- is the point. Parity arithmetic is a second
derivation of a fact the board already states, and the two could drift; the features and the label then
cannot disagree by construction.

`compute_labels` keeps its signature and its documented White-relative contract, so its hand-computed
tests stay valid. To make the remaining risk visible, every call site renames its local from `labels`
to `white_relative_labels`: a reader who sees `white_relative_labels` being written into a cache has the
bug in front of them.

### Call sites

| site | path | live? |
|---|---|---|
| `length_cache.rs` `label_and_featurize_self_play_game` | self-play, NNUE track | **yes, this is the one that produced the bad data** |
| `length_cache.rs` `build_labels` | curated, labels only | yes, transformer track |
| `length_cache.rs` `build_positions_and_labels_together` | curated, combined pass | yes, transformer track |
| `builder.rs` `build_caches` | superseded single-cache format | dead -- **left alone, see below** |

The three live sites already have the board in hand -- the curated ones replay the PGN for piece-count
bucketing, the self-play one for feature extraction -- so `extract_canonical(&board).side_to_move` is
available at the write point with no new plumbing.

**`builder.rs`'s `build_caches` is deliberately not fixed.** Reading it to apply the same change turned
up a *second* defect: it makes move `i` before extracting the position, so every label is paired with
the position *after* its own move rather than before it -- the exact off-by-one the live paths carry an
explicit comment about having fixed. It is dead code (nothing calls it outside its own tests) writing a
superseded single-cache format, and fixing two bugs in something nobody runs is worse than deleting it.
It belongs to the dead-code pass rotor-train-dataset-cache.md already lists, now with a reason to
prefer deletion over repair.

### Tests

- **The property test that would have caught this**: a self-play fixture where White is a queen up and
  wins, checked at two consecutive plies. With the material advantage unchanged between them, the label
  must be positive at the White-to-move ply and negative at the Black-to-move ply. Both are positive
  today.
- The negamax equivalence above, asserted directly on `side_to_move_relative` composed with
  `compute_labels`: flipping the frame ply by ply equals running the recursion with a negated `next`.
- The existing cache-content tests (`build_labels_matches_compute_labels_and_shares_positions_caches_own_bucketing`,
  `build_positions_and_labels_builds_both_from_scratch_in_one_pass`, `builder.rs`'s own) compare written
  labels against `compute_labels` output directly and must gain the same flip. Their fixtures start from
  the standard position, so ply 0 keeps its sign, ply 1 flips, ply 2 keeps.

## Blast radius

- **The self-play PGN is fine.** Its per-move score comments are side-to-move relative and correct.
  Nothing needs regenerating.
- **`data/datasets/hand-crafted/lambda-0.70/` must be rebuilt.** The labels change, so `prepare` reruns
  (a few minutes of replay, and the manifest is the completion marker). Patching the cache in place is
  possible in principle -- every record carries its own `side_to_move`, so a rewrite pass could negate
  exactly the right labels without replaying anything -- but that is a throwaway tool with its own tests
  against a rebuild that costs minutes and keeps the PGN as the single source of truth.
- **`data/networks/amanda/` must be deleted.** Three epochs across six arms were fitted to an ill-posed
  target.
- **The curated length-bucketed caches under `data/caches/` are affected too**, but they are empty
  today.
- **Every earlier result on this track is void**, including the streaming run's own `correlation = 0.22`
  from before the dataset cache existed. The defect predates all of it.

## What this does not tell us

- **Nothing about the naive architecture.** It was never given a well-posed target.
- **Nothing about `M`.** "Still falling steeply after three epochs, so raise `M`" was measured on the
  wrong target.
- **The learning-rate grid's answer is suspect.** The unimodal curve with its minimum at 3e-4/1e-4 was
  fitted to the same ill-posed target. The ordering may survive -- optimizer dynamics do not care much
  what the target means -- but it has to be re-measured rather than assumed.

  It did not survive. Re-measured over one epoch on the fixed target, the champion is `1e-3`
  (`0.040275` at step 15872), with `3e-3` and `3e-4` within `0.0013` of it and the three slower arms
  never reaching an interior minimum at all -- their best round is the last one, so they are
  undertrained rather than beaten. Caution about the ordering was warranted.

## Afterwards

Rerun `correlate`/`check` on the first epoch's champion before anything else. The prediction is a
different order of magnitude, not a few points, because material becomes learnable again. If R^2 is
still a few percent with the frames aligned, then the architecture is genuinely on trial and that is a
real finding rather than a bug.
