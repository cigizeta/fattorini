# Lc0's one pawn score fitted, not hard-coded

**Superseded 2026-11-04, see training-corrections.md and training-corrections-implementation.md:**
no fitted scale at all; Lc0's corpus is rewritten into Lc0's own curve and read as Q, every other
teacher through the logistic at S. Step 1 below was undone by that work.

**Status: plan (2026-11-03), agreed by the developer the same day; being implemented.** Follows label-scale.md and
label-scale-implementation.md. The developer: Lc0's one pawn score must not be a hard-coded constant;
the current Lc0 run restarts from scratch.

## Decisions

- **Every corpus goes through the same step:** its one pawn score fitted at the ceiling of 128 by
  `scale_fit::ensure`, kept in a `scale.txt` beside it. Lc0's in `data/lc0/scale.txt`, fitted on its
  training corpus (`data/lc0/train`).
- **One ceiling everywhere, 128 (the developer, 2026-11-03: "we need to be consistent"):** Convert
  writes Q as pawns through a curve whose ceiling is 128, not `LC0_FILES`'s 32, so Q = +/-1 is
  written as +/-128 pawns and read back as +/-1 like any other corpus's certain score. Lc0's corpus
  is re-encoded for it, not downloaded again (the developer, 2026-11-03): each score's pawns read
  back to Q through the old curve (ceiling 32, +/-32 pawns exactly +/-1), then written as pawns
  through the new one (ceiling 128). The games, positions and test set unchanged; each Q keeps the
  old 0.01 pawn rounding, far below what training notices. `LC0_FILES` lives on only inside the
  re-encoding, removed with it; no special case for +/-32 pawns anywhere.
- **Expected value:** a little under 0.50 (the ignored test `one_pawn_of_corpus`, 2026-11-03: the
  error rises from 0.50 upwards, the grid's lowest point).
- **The current Lc0 run restarts** (the developer, 2026-11-03): no compatibility with its buckets.

- **Lc0's one pawn score is recomputed, never hard-coded (the developer, 2026-11-03).** Lc0 gives Q,
  not pawns, so its files need some curve to write Q as pawns: Convert writes through `NETWORK`, the
  networks' own curve (the definition of what a network's pawn means, not a fact about Lc0). What
  Lc0's pawn is worth is then measured: `scale_fit::ensure` fits it against the games' results, as
  for any teacher. If Lc0's Q were perfectly calibrated the fit would return `NETWORK`'s value; the
  gap is Lc0's own calibration.
- **A one-time reconversion (the developer, 2026-11-03):** an ignored test the developer runs once
  with `cargo test`, then deleted. It rewrites every segment of `data/lc0` (train, test) locally and
  on Drive, each through a temporary file and a rename, and adjusts the state that depends on the
  bytes: each segment's `.meta` and `tars.tsv`'s `pgn_bytes`. Rerunning it is harmless: a file
  already re-encoded is recognised (a marker in its `.meta`) and skipped. `data/lc0/frozen` and
  `data/lc0/scale.txt` are deleted, so the next run freezes and fits again.

## Open points

- **Precision near the ceiling.** Scores are written to 0.01 pawns; at a ceiling of 128 the curve is
  steeper in pawns near Q = +/-1, so the rounding error in Q is smaller there, not larger. To check
  with the round-trip test.

## Steps

1. **`smithy.rs` `train`:** Lc0's scale from `scale_fit::ensure(remote data/lc0/scale.txt)`, as
   PeSTO's, before the test set is frozen and the corpus scattered.
2. **Convert through `NETWORK`:** `lc0/convert.rs` writes Q as pawns through it; `LC0_FILES` and
   every other use of it (tests included) removed; the round-trip test of Q kept on `NETWORK`.
3. **The one-time reconversion** (Decisions): old curve to Q to `NETWORK`, with tests that +/-32
   pawns become +/-128, that a round trip keeps Q, and that a rerun skips what is done. The
   developer runs it; it is deleted in the next commit.
4. **The frozen test sets record their scale:** the manifest of `freeze.rs` gains the one pawn score
   and ceiling; a set frozen under another scale is frozen again. Needed beyond Lc0: a self-play
   corpus whose scale is refitted after growing would otherwise keep labels of the old one. Lc0's
   frozen sets (`data/lc0/frozen`), shared across runs, are frozen again on the first run. Test: a
   set frozen under one scale, ensured under another, frozen again.
5. **`buckets.rs`:** the compatibility for old `labels tablebase` lines read with `LC0_FILES` is
   removed, with its test; any mismatch scatters again.
6. **Tests and docs:** label-scale.md and the TODO updated.

## Progress

- **Step 1 done 2026-11-03:** `train` fits Lc0's scale by `scale_fit::ensure` (Lc0's `scale.txt`),
  as PeSTO's; clippy passing. Until steps 2 and 3, Lc0's corpus is still written through the old
  curve: **no Lc0 run before the reconversion.**
- **Next: step 2** (Convert through `NETWORK`), then step 3 (the one-time reconversion, run by the
  developer), then steps 4 to 6.

## Restart

The developer deletes the current Lc0 run's directory (or starts a new run name); its buckets would
be scattered again regardless, by the `labels` line's mismatch.
