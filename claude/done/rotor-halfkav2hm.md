# Rotor HalfKAv2_hm

## Status

Design settled below, across a design discussion (see the summary in this document's own
reasoning) that also produced the `companion/rotor.md` table row this document implements:
`Rotor HalfKAv2_hm | (4 + 8) * (8 + 8) * 11 * d | alpha-beta`. Implementation has not started --
deliberately deferred to a new session, per the developer's explicit request. This document is
meant to be sufficient on its own to resume with no other context.

**Scope, settled explicitly**: `rotor-train` (the training pipeline) only. There is currently no
code path anywhere that loads a trained network into the shipped `rotor` UCI binary at all --
confirmed by survey: `src/main.rs` is just `rotor::uci::run()`, `tch`/`zstd` are both optional,
`training`-feature-gated dependencies, and `training.md`'s own "weight handoff" section
describes the flat-binary export format as still unspecified. This document does not touch that;
play-time loading of either feature set is separate, future work.

## Settled architecture

**Adds a second feature set alongside Rotor HalfKP, selected at `train` time, not `prepare`
time.** The on-disk feature cache (`cache.rs`'s `CacheRecord { canonical: CanonicalPosition,
label: f32 }`, written by `prepare`) already stores a feature-set-agnostic canonical snapshot --
a raw piece list plus both kings' absolute squares plus side to move -- not pre-baked HalfKP
features. The HalfKP-specific encoding only happens later, in `features::expand`, when a cached
record is turned into a training batch. This is exactly why `builder.rs`, `cache.rs`, and
`reader.rs` needed no changes for the original HalfKP rollout, and why they need none here either:
one `prepare`d cache works for training under either feature set. `--lambda` and cache file paths
are entirely unaffected by this work.

**New `--features <halfkp|halfka_v2_hm>` flag on `train` only.** Optional, following the existing
`--lambda` precedent (`prepare`'s own optional flag with a `DEFAULT_LAMBDA` constant) rather than
`--lr`'s mandatory-with-no-default precedent: `DEFAULT_FEATURE_SET = FeatureSet::HalfKav2Hm`.
`prepare`'s CLI surface and `USAGE` string are unaffected.

**Two distinct architectures, not one parameterized shape.** Settled explicitly: `HalfKp` and
`HalfKav2Hm` get their own concrete `Network` struct each, with hardcoded layer widths, rather
than a single struct taking a width parameter. This matches the project's existing preference for
small, concrete, individually-explainable pieces over generic abstraction, and reflects that these
really are two different formats (different identity width, different king encoding), not one
shape with a knob.

**Feature representation.** Classic HalfKA convention: like HalfKP, the *own* king anchors the
lookup and is never itself a feature; unlike HalfKP, the *opponent's* king is representable as an
ordinary piece, at one additional identity slot. Per perspective, every piece the opponent's king
included is a triple `(perspective's own king square, piece square, identity)`, where identity is
11-wide: 5 real piece types x 2 relative colors (own piece / opponent's), plus 1 more category for
"the opponent's king." (Published fact, not implementation code, same standing as the numbers
already checked into `companion/rotor.md`'s table: real Stockfish's own HalfKAv2 feature set uses
this exact `PIECE_TYPES * COLORS - 1 = 11` identity count, the `-1` being the exclusion of *your
own* king only.)

**Horizontal mirror ("`hm`"), decided per perspective from that perspective's own king file.**
When a perspective's own king sits on a file in `e..=h`, the whole board is mirrored left-right
(file `f` becomes `a`, `g` becomes `b`, and so on) before any lookup for that perspective's own
accumulator -- collapsing that perspective's own king-file axis from 8 possible values to 4.
Every piece's file mirrors along with the king's, not just the king's own -- otherwise the real,
non-separable king-x-piece geometric relationship this project already preserves for HalfKP would
break. Consequence: the king's own two axes (file, rank) shrink from 16 combined lanes (`8 + 8`)
to 12 (`4 + 8`) -- rank is untouched, since `hm` mirrors left-right only -- while the *piece*
square's own two axes stay the full 16 (`8 + 8`): mirroring the king only guarantees the king's
own file narrows, not that every piece's post-mirror file does. This is exactly
`companion/rotor.md`'s `(4 + 8) * (8 + 8) * 11 * d` row, and mirrors (pun acknowledged) real
Stockfish HalfKAv2_hm's own king-square reduction, 64 combinations down to 32, applied the same
way to Rotor's additive file+rank factoring instead of a flat product.

**Table size and per-piece row count, same convention as `rotor-halfkp.md`'s own explanation.**
One active piece (including, now, an opponent's king) visits 4 rows of a shared `12 (king
file/rank, mirrored) x 16 (piece file/rank) x 11 (identity) = 2112`-row table -- (king lane, piece
file), (king lane, piece rank), and so on for the king's own two lanes -- summed, not set (two
pieces can share a row, e.g. doubled pawns). `ROWS_PER_PERSPECTIVE` for this feature set is `2112`,
against HalfKP's existing `2560`.

**Mechanical consequence for `features.rs`: the opponent's king becomes a real piece entry.**
Concretely, when building a given perspective's accumulator, the piece list fed into the crossed
table gains one more entry beyond HalfKP's existing own/opponent non-king pieces: the *other*
side's king, at identity slot 10 (0-indexed), at that king's own (mirrored) square. HalfKP has no
such entry at all (the opponent's king is invisible to it, a known, already-accepted limitation
per `rotor-halfkp.md`). Since both kings' squares are already tracked as separate fields on the
canonical position, this is naturally a `batch.rs`-level addition (one extra fixed contribution
per accumulator build) rather than a change to how pieces are stored on disk.

**Code organization: two submodules inside `features.rs`, not two files, not a rename of existing
code.** `CanonicalPosition`, `pack_canonical_piece`, `unpack_canonical_piece` stay exactly where
they are, at the top level of `features.rs` -- they're genuinely shared, feature-set-agnostic, and
already used directly by `cache.rs`. Everything HalfKP-specific that already exists
(`PositionFeatures`, `pack_piece`/`unpack_piece`, `NON_KING_TYPE_COUNT`, `IDENTITY_COUNT`,
`ROWS_PER_PERSPECTIVE`, the extraction/expansion functions) moves, unchanged in behavior, into a
`features::halfkp` submodule. A new, parallel `features::halfka_v2_hm` submodule holds the new
identity-11, mirrored-king equivalents. This keeps every existing test working (moved, not
rewritten) and makes "which format does this constant belong to" unambiguous by construction,
rather than by naming discipline (`_halfkp`/`_halfka_v2_hm` suffixes on otherwise-flat names).

**New shared `FeatureSet` enum**, `HalfKp` / `HalfKav2Hm`, living in its own small file
(`feature_set.rs`), matching this codebase's existing pattern of one small file per single
responsibility (`labels.rs`, `history.rs`). Round-trips to/from the `--features` CLI string
(`"halfkp"` / `"halfka_v2_hm"`) and to/from the one-byte discriminant written into the checkpoint
marker below -- one enum, one place, used by `main.rs`'s CLI parsing, `batch.rs`'s dispatch, and
`network.rs`'s dispatch alike.

**`batch.rs`**: two concrete `DenseBatch` shapes (`DenseBatchHalfKp`, matching today's existing
struct renamed, and a new `DenseBatchHalfKav2Hm` with `[batch, 2112]` tensors), wrapped in an
`enum DenseBatch { HalfKp(DenseBatchHalfKp), HalfKav2Hm(DenseBatchHalfKav2Hm) }`. `to_dense_batch`
takes a `FeatureSet` and dispatches to the matching expansion path.

**`network.rs`**: two concrete `Network` structs (`NetworkHalfKp`, matching today's existing
struct renamed, `transform: 2560 -> 128`; `NetworkHalfKav2Hm`, `transform: 2112 -> 128` -- both
share the same `256 -> 32 -> 32 -> 1` tail, since that shape came from the accumulator's
concatenated width, not from anything HalfKP-specific), wrapped in an
`enum Network { HalfKp(NetworkHalfKp), HalfKav2Hm(NetworkHalfKav2Hm) }`. `Network::new` takes a
`FeatureSet`; `Network::forward` matches its own variant against the matching `DenseBatch`
variant -- a `HalfKp` network is never called with a `HalfKav2Hm` batch, enforced by the match
arms themselves (mismatched pairs simply have no arm), not by a runtime check.

**Checkpoint directories: one format per directory, enforced by the tool, not by developer
discipline.** A `model.vs` trained under one architecture's shape does not load correctly under
the other's `Network::new` -- unlike caches, checkpoints genuinely are shape-sensitive. Settled:
a new small marker file, `{checkpoint_dir}/feature_set.state`, following the existing magic +
version header convention already used by `cache.rs` (`RTFC`) and `early_stopping.rs`'s own state
file -- a 4-byte magic (e.g. `RTFS`), a `u32` version, one byte for the `FeatureSet`
discriminant. On a fresh checkpoint directory (no marker present yet), `train` writes it once,
from whatever `--features` was passed. On every subsequent invocation into that same directory
(resume or otherwise), `train` reads the marker first and compares it against the requested
`--features`: a mismatch is treated as a CLI usage error -- a clear message to stderr plus
`process::exit(1)`, matching `main.rs`'s existing style for bad user input (distinct from
`cache.rs`'s own panic-on-corruption style, since pointing the tool at the wrong directory is a
usage mistake, not a corrupted file) -- rather than silently attempting a shape-mismatched load.

## Implementation plan

Code changes, in dependency order (mirrors `rotor-halfkp.md`'s own structure):

1. **`feature_set.rs`** (new): `FeatureSet` enum, CLI string parsing, checkpoint-marker
   discriminant round-trip. No dependencies on the other files below.
2. **`features.rs`**: move existing HalfKP-specific items into a `halfkp` submodule (pure
   reorganization, no behavior change -- existing tests move with it); add a `halfka_v2_hm`
   submodule with the 11-wide identity, the per-perspective horizontal-mirror logic (mirror
   trigger: own king file in `e..=h`; mirrors that perspective's own king file and every piece's
   file consistently), and `ROWS_PER_PERSPECTIVE = 2112`. `CanonicalPosition` and the
   `pack_canonical_piece`/`unpack_canonical_piece` pair stay top-level, untouched.
3. **`cache.rs`**, **`builder.rs`**, **`reader.rs`**: no changes. Confirmed by design: all three
   only ever handle `CanonicalPosition`, never `PositionFeatures`.
4. **`batch.rs`**: rename existing `DenseBatch` to `DenseBatchHalfKp`; add `DenseBatchHalfKav2Hm`
   (`[batch, 2112]` tensors) and the matching expansion path, including pushing the opponent
   king's own identity-10 entry per accumulator build; wrap both in a `DenseBatch` enum;
   `to_dense_batch` takes a `FeatureSet` and dispatches.
5. **`network.rs`**: rename existing `Network` to `NetworkHalfKp`; add `NetworkHalfKav2Hm`
   (`transform: 2112 -> 128`, same `256 -> 32 -> 32 -> 1` tail); wrap both in a `Network` enum;
   `Network::new` takes a `FeatureSet`; `Network::forward` matches variant-to-variant against
   `DenseBatch`.
6. **`main.rs`**: add `--features <halfkp|halfka_v2_hm>` parsing to `run_train` only (`prepare`'s
   `USAGE`/flags untouched), `DEFAULT_FEATURE_SET = FeatureSet::HalfKav2Hm` following the
   `--lambda` precedent; write/validate the `feature_set.state` checkpoint marker (see above)
   before entering the training loop.
7. **`window.rs`**: update the one `network.forward(...)` call site to go through the `Network`/
   `DenseBatch` enums; update fixture `PositionFeatures`/batch literals as needed for both
   variants.
8. **Tests**: shape tests for `NetworkHalfKav2Hm` (input/output dimensions, weight sharing between
   perspectives, output in `(0, 1)`, same battery as the existing `NetworkHalfKp` tests); a
   mirroring golden-value test in `features::halfka_v2_hm` (a king on file `a` producing an
   unmirrored lane, a king on file `e` producing the mirrored equivalent of file `d`'s lane, rank
   unaffected either way); an opponent-king-representability test (the opponent's king square
   shows up at identity 10, own king never appears as a feature at all); a checkpoint-marker
   mismatch test (a fresh directory tagged `halfkp`, then `train` invoked with
   `--features halfka_v2_hm` against it, exits with an error rather than proceeding).

**Compile checkpoints** (no Bash access here -- the developer runs these and reports back):

```bash
cargo build --features training
cargo test --bin rotor-train --features training
```

## Progress

Steps 1-6 of the implementation plan landed in this session. Two refinements surfaced during
implementation that the plan above didn't anticipate; both are applied in the code already, noted
here so the plan text and the actual code don't silently drift apart:

- **`reader.rs` does need a small change**, contrary to the plan's step 3: `read_range` and
  `read_batch_cyclic` call `batch::to_dense_batch` directly, and that function now takes a
  `FeatureSet` to know which variant to build. Both methods gained a `feature_set: FeatureSet`
  parameter (their callers, `window.rs`'s `run_test_pass`/`run_training_steps`, now take and thread
  through the same parameter). `CacheFile` itself still stores nothing about feature sets --
  the change is purely an added parameter, not a structural one, so `cache.rs`/`builder.rs` remain
  genuinely untouched as the plan said.
- **`DenseBatch` gained a `label()` accessor** (`batch.rs`) rather than every caller matching out
  the variant to reach `.label` -- `window.rs`'s loss computation and several tests read only the
  label regardless of variant, so this avoids repeating the same two-armed match everywhere.

What landed, matching the plan's own step numbering:

1. `feature_set.rs`: `FeatureSet` enum, `--features` CLI string round-trip, and the
   `feature_set.state` marker (magic `RTFS`, version, one discriminant byte) -- `write_marker`/
   `read_marker` only ever panic on genuine corruption; comparing against what a run actually
   requested is main.rs's own job (see step 6).
2. `features.rs`: reorganized into shared top-level items (`CanonicalPosition`,
   `pack_canonical_piece`/`unpack_canonical_piece`, `extract_canonical`, plus the previously
   HalfKP-only `oriented_square`/`king_square_of`, which turned out to be genuinely
   feature-set-agnostic geometry, not HalfKP-specific, so they stayed top-level rather than moving
   into `halfkp`) and two submodules, `halfkp` (moved unchanged) and `halfka_v2_hm` (new: 11-wide
   identity, opponent-king representability, per-perspective horizontal mirror). Each submodule
   carries its own tests; a small top-level `tests` module covers the shared items.
3. Confirmed unchanged: `cache.rs`, `builder.rs` (see the `reader.rs` note above for the one file
   that needed a small, non-structural change).
4. `batch.rs`: `DenseBatchHalfKp`/`DenseBatchHalfKav2Hm` wrapped in a `DenseBatch` enum;
   `to_dense_batch(records, feature_set)` dispatches; `accumulate_perspective_halfkp`/
   `accumulate_perspective_halfka_v2_hm` hold each feature set's own crossed-row arithmetic.
5. `network.rs`: `NetworkHalfKp`/`NetworkHalfKav2Hm` wrapped in a `Network` enum;
   `Network::new(vs, feature_set)` dispatches; `Network::forward(&self, &DenseBatch)` matches
   `(self, batch)` pairs, with a `_ => panic!(...)` wildcard for the two mismatched combinations
   that main.rs's own single-feature-set-per-run design never actually produces.
6. `main.rs`: `--features <halfkp|halfka_v2_hm>` added to `run_train` only (`prepare` untouched);
   `DEFAULT_FEATURE_SET = FeatureSet::HalfKav2Hm`; `feature_set.state` written on a fresh checkpoint
   directory or compared against on a resumed one, exiting with a usage error (not a panic) on
   mismatch; `Network::new`/`window::run_window` calls updated to pass `feature_set` through.

Also updated as part of steps 4/5/6's own ripple: `window.rs` (`run_window`/`run_training_steps`/
`run_test_pass` all take `feature_set: FeatureSet` now; `network.forward` calls updated to the new
`&DenseBatch` signature; every test call site updated, all pinned to `FeatureSet::HalfKp` since the
tests don't care which feature set they exercise).

Both follow-up items were then addressed in the same session:

- **`window.rs` now exercises both feature sets.** `run_window_produces_finite_losses_of_the_right_
  shape` loops over `[FeatureSet::HalfKp, FeatureSet::HalfKav2Hm]` rather than hardcoding `HalfKp`
  -- it's the one test in that module that runs the whole `CacheFile` -> `Network` -> loss pipeline
  end to end, so it's the natural place to confirm `HalfKav2Hm` behaves too, not just compiles.
- **The checkpoint-marker mismatch logic is now unit-tested**, via a small refactor rather than
  trying to test `process::exit` directly: `feature_set.rs` gained `MarkerDecision` (`WriteFresh` /
  `AlreadyMatches` / `Mismatch { checkpoint_feature_set }`) and a pure `decide_marker(existing:
  Option<FeatureSet>, requested: FeatureSet) -> MarkerDecision` function, unit tested for all three
  outcomes. `main.rs`'s `run_train` now calls `decide_marker` and matches on the result, keeping
  `process::exit` (and the error message text) as the one part that stays untested, consistent with
  every other CLI usage error in that file.

Compile checkpoints: run by the developer, green both times -- 81 passed before the two follow-ups
above, 84 passed after (the 3 new `decide_marker` tests).

This document's own scope is complete: `rotor-train` supports both `HalfKp` and `HalfKav2Hm`,
selected via `train`'s `--features` flag, with a checkpoint-directory marker preventing an
accidental architecture mismatch, and test coverage across `features.rs`, `batch.rs`, `network.rs`,
`feature_set.rs`, and `window.rs` for both feature sets. What's left is explicitly out of scope
here (see this document's own "Scope, settled explicitly" section): loading a trained network into
the shipped `rotor` UCI binary at play time, under either feature set -- separate, future work,
its own design discussion when the developer wants to start it.
