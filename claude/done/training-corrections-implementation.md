# Training corrections: implementation plan

**Status: step 1 written (2026-11-04), additive: `LC0_A_PAWNS`, `LC0_B`, `LC0`, `S_PAWNS`, `Curve`,
`LOGISTIC` in `labels.rs`, beside the old curves; callers move in the later steps. Step 2
written (2026-11-04): `nnue_network::score` is the logistic, `bce_loss` replaces `matching_loss`,
`arm_training_losses` / `arm_test_losses` are the binary cross entropy with logits. Step 3
written (2026-11-04): labels take a `Curve`, `Source::curve` chooses it (Lc0 through `LC0_FILES`
until step 4, PeSTO and self-play through `LOGISTIC`), `scale_fit` unhooked (`scale_fit.rs` to be
deleted by the developer), `buckets.tsv` records `Curve::describe`, the legacy carve-out gone.
Step 4 written (2026-11-04): `lc0/rewrite.rs`, the `lc0-rewrite` subcommand, resumable
(`<segment>.rewritten`, `rewrite-done.txt`, then the rename), `curve.txt` written last; training
reads Lc0 through `LC0` and refuses the corpus without `curve.txt`; Convert writes through `LC0`
and refuses to add to a corpus still in the old curve. Steps 5 and 6 written (2026-11-04): `.nnue`
`VERSION` 2, the same layout, versions 1 and 2 read; `nnue::file_version`, `curve_of_version`;
`pawn_percent` measured through each network's own curve (`measure_label_percent_through`); the
smithy refuses a version 1 `teacher.nnue` in `teacher()` and `train()`; `run_config.txt` freezes a
`curve` line, so a run started before is refused on resume. Step 7 written (2026-11-04):
`lambda_star.rs` and the whole of `engine_loss.rs` unhooked (the developer: the whole file, its
curve-free diagnostics too), to be deleted by the developer; `self_play_game_best_moves`, used by
`engine_loss` alone, removed; `nnue_bench`'s `--label-scale` removed, its tails' label-space column
read through `LOGISTIC`. Step 8's tests were written with each step.**

**Frozen test sets (the developer, 2026-11-04):** the curve their labels were made with is not
recorded; before the experiment the developer deletes them by hand and they are frozen again under
the new curves. A git revert needs the same. Implements training-corrections.md. An experiment: if it fails, the
developer reverts its commits with git; no switch keeps the old path alive.

## Decisions (the developer and Claude, 2026-11-04)

- **Prediction:** `p = sigma(net / S)`, S = 70.37 centipawns (0.7037 pawns: the networks output
  pawns), loss the binary cross entropy of `p` and `t`.
- **Targets**, all from `q` in [-1, +1] (lambda blend unchanged, in `q`), then `t = (q + 1) / 2`:
  - Lc0: `q` = Lc0's Q, read as it is, no fitted scale.
  - PeSTO and our networks: `q = 2 * sigma(cp / S) - 1`, the same S, no fitted scale.
  - Mates, tablebase results: `q = +-1`, as today.
- **Old networks** (trained under the atan curve): play only. They load and play everywhere, but
  are refused as a training warm start, as a resume, and as a self-play teacher.
- **Lc0's files** are rewritten once, without downloading Lc0 again: each stored score decoded to Q
  through the frozen curve it was written with, then re-encoded with Lc0's own curve,
  `cp = 90 * tan(1.5637541897 * Q)`; training decodes with its inverse.

## What the code does today (mapped 2026-11-04)

- Loss: `stacked_network::arm_training_losses` / `arm_test_losses`, for every architecture (the
  transformer included), the matching loss through `labels::NETWORK` (atan, one pawn 0.50, ceiling
  128). `nnue_network::matching_loss` is its test-only reference.
- Labels: `labels::compute_labels_direct`, `lambda * z + (1 - lambda) * scale.score(cp / 100)`,
  with `scale` a fitted g per corpus or generation (`scale_fit.rs`, `scale.txt`). **Lc0 too gets a
  fitted g today**: its Q, stored as pawns through the frozen `LC0_FILES`, is re-read through a
  scale fitted on the Lc0 corpus. The transformer's stream reads Lc0 through `LC0_FILES` directly.
- Lc0 storage: `lc0/convert.rs` writes `LC0_FILES.pawns(Q)` into the PGN comment as pawns with 2
  decimals. The rounding (at most 0.005 pawn) costs most near equality, where the frozen curve is
  steepest (0.63 of Q per pawn): at most 0.0031 of Q. The rewrite rounds again through Lc0's curve
  (0.71 per pawn at equality): at most 0.0036 more, about 0.007 of Q in the worst case, 0.0035 of
  `t`. **Accepted (the developer, 2026-11-04):** far below the noise of Q itself and of the blend
  with z, so Lc0 is rewritten in place, not downloaded again.
- Engine: `nnue::fit_label_scale` reads the network through `labels::score` to compute
  `pawn_percent`, which scales the search margins and time management. **This depends on the
  curve** and must follow it.
- No network file records its curve: `.nnue` has `VERSION = 1`; `.vs` files and `run_config.txt`
  carry nothing of the kind.

## Steps

1. **The curve.** In `labels.rs`, a sigmoid scale beside the atan one: `S_PAWNS = 0.7037`,
   `score(pawns) = 2 * sigma(pawns / S_PAWNS) - 1`, and its inverse. `NETWORK` and every
   `labels::score` caller move to it. `LC0_FILES` stays, only to decode not yet rewritten files.
   An `LC0` curve, `atan(pawns / 0.90) / 1.5637541897` clamped, and its inverse, for Lc0's files.
2. **The loss.** `arm_training_losses` and `arm_test_losses` become binary cross entropy with logits,
   `net / S_PAWNS` against `(label + 1) / 2`; `matching_loss` and its tests replaced by the BCE
   reference. The reported, early stopping and `candidate.txt` losses become BCE: not comparable
   with earlier runs, which is accepted (history files of old runs are not mixed with new ones).
3. **The labels.** `compute_labels_direct` takes a curve instead of a fitted `Scale`: the sigmoid
   for PeSTO and our networks, `LC0` for Lc0's rewritten files. `scale_fit.rs`, `scale.txt` and
   their callers in `smithy.rs` are removed; `buckets.tsv`'s `labels` line records the curve by name,
   so caches built under a fitted g are refused and rebuilt (`same_labels`'s legacy carve-out goes).
   The transformer's stream reads Lc0 through `LC0`.
4. **Lc0's files rewritten.** A one-off smithy command walks the Lc0 corpus: each comment's pawns
   decoded through `LC0_FILES` to Q, re-encoded through `LC0`'s inverse, written with 2 decimals
   (Q = 1 gives about 128.00 pawns). A marker beside the corpus records the rewrite; reading
   refuses a corpus without it, so no file is ever decoded through the wrong curve. `convert.rs`
   writes through `LC0` from then on. Run by the developer, once, through `bin/smithy`.
5. **Marking new networks.** `.nnue` `VERSION` 2 means "trained under the sigmoid"; the engine loads
   1 and 2. `run_config.txt` gains a `curve` line, so a run started before is refused on resume by
   the existing verification. `teacher()` and `seed()` refuse a `teacher.nnue` of version 1 and
   a run whose `teacher.vs` comes from one, with a message saying old networks only play.
6. **The engine's `pawn_percent`.** `fit_label_scale` reads a version 2 network through the sigmoid,
   a version 1 network through the old atan, kept for this alone, so old networks keep their
   margins.
7. **Diagnostics removed (the developer, 2026-11-04):** the label-scale and lambda* diagnostics
   that read through the old curves (`engine_loss.rs`'s label-space parts, `lambda_star.rs`,
   `nnue_bench.rs`'s `--label-scale`) are deleted, not ported; rewritten later if needed.
8. **Tests.** Every test pinning the atan `NETWORK`, the matching loss or `scale_fit` rewritten for
   the sigmoid and BCE; new: BCE equals the hand computation, its gradient is `(p - t) / S_PAWNS`,
   the Lc0 rewrite round trip, a version 1 teacher refused, a version 1 network still plays.

## The reality check (the developer, 2026-11-04)

- **The run:** a generation 1 on Lc0's rewritten labels with the new loss, the same architecture and
  lambda grid as the last generation 1 on Lc0, so the curve and the loss are the only change; no
  self-play involved.
- **Success:** its winner duelled at once against that old winner, both with the search parameters
  as they are, before any SPSA. A clear loss stops the experiment (git revert); a draw or a win goes
  on to SPSA. The engine's `pawn_percent` rescales the margins per network (step 6), which softens
  the handicap of margins tuned for the old unit.

## Open

1. **Quantisation:** confirm the new outputs stay within the engine's range (`centipawns()`'s clamp
   at 128 pawns; mates push `net` only to a few hundred centipawns).
2. **Found while mapping, not in scope:** the frozen test set never applies the tablebase override
   that training labels get (`freeze.rs` against `pgn_stream.rs`).
