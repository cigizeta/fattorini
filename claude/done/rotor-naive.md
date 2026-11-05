# Rotor naive

## Status

Design settled. This document captures the agreed design; implementation has not started yet.
Will keep being updated in place as implementation proceeds, per this project's own
`claude/active/` convention.

## Motivation

`companion/rotor.md`'s own evaluation-scores-networks table already carries this row:
`Rotor naive | (8 + 8) * 12 * d | alpha-beta`. The developer's explicit reason to build this one
first, ahead of Rotor HalfKP/HalfKAv2_hm's own further tuning: a literature check turned up real
precedent that a flat, king-agnostic board encoding (no HalfKP-style king-relative crossing at all)
can still reach genuinely competitive strength, not just "it compiles" strength --

- **DeepChess** (David, Netanyahu & Wolf, 2016): a 768-bit one-hot board (`2 colors * 6 piece
  types * 64 squares`, king included) plus 5 extra bits (side to move, 4 castling rights), fed
  through a deep, unsupervised-pretrained stack (`773 -> 600 -> 400 -> 200 -> 100`), reached
  self-reported results of +63 Elo over Crafty (a two-time WCCC winner) and roughly parity with
  Falcon (a grandmaster-strength engine of that time) at equal time controls.
- **Deep Pink** (Bernhardsson, 2014), architecturally the closer match to this table's row --
  12x64 = 768 one-hot inputs, no extra bits at all (not even side to move), paired directly with
  negamax alpha-beta, exactly this row's own "paired with" column -- scored only 30-35% against
  Sunfish, a toy ~100-line Python engine not even good enough to be CCRL-rated.

Neither engine has ever been through CCRL testing (confirmed by direct query against CCRL's own
rating database, not just absence from a general web search), so there's no independently-verified
number for either. But the *contrast* between them is itself informative: same underlying raw
representation, wildly different outcomes. That points at training methodology and network depth
as the likely bottleneck, not the flat representation itself -- which is exactly the question this
experiment is designed to probe for Rotor's own case.

## Settled so far

**Scope: `rotor-train` only**, same as `rotor-halfkav2hm.md`'s own precedent. No code path loads
any trained network into the shipped `rotor` UCI binary yet, under any feature set; play-time
loading stays separate, future work.

**Identity is 12-wide, king included, no exclusion.** Unlike HalfKP (which excludes the own king
entirely, since it anchors the lookup) or HalfKAv2_hm (which excludes only the *own* king, while
representing the opponent's king as an extra identity), naive has no king-relative anchor at all --
there is nothing for a king to anchor. Every piece, both kings included, is just an ordinary
feature: `identity = piece_type * 2 + (0 if own else 1)`, 6 piece types (pawn, knight, bishop,
rook, queen, king) x 2 relative colors (own/opponent) = 12. No special "opponent king
representable" category is needed the way HalfKAv2_hm's 11th identity is -- naive never excluded
the king from anywhere in the first place, so there's nothing to add it back for.

**Mirroring: yes to the vertical/side-to-move mirror, no to the horizontal king-file mirror.**
Two genuinely different techniques share the word "mirror" here, worked out over the preceding
discussion (see this document's own conversation for the full derivation, not repeated in full
here):

- *Vertical mirror + own/opponent relabeling* (`oriented_square`, already shared/feature-set-
  agnostic code in `features.rs`): reorient every square by side-to-move and label every piece
  "own" or "opponent" rather than absolute White/Black. This one transfers to naive cleanly and for
  free -- its trigger (`side_to_move`) is a global, position-level fact, not tied to any one
  piece's square, so it applies uniformly across all 12 identities and doesn't change the table
  size at all (`(8+8)*12*d` either way). It also makes an explicit side-to-move input bit
  unnecessary (unlike DeepChess's extra bit for it): once every piece is labeled relative to
  whoever's about to move, and the training label itself means "expected score for the side to
  move" (already Rotor's existing convention), there's no side-to-move ambiguity left to resolve.
  Settled: naive uses this.
- *Horizontal, king-file-triggered mirror* ("hm", HalfKAv2_hm's own trick): its trigger is one
  *specific piece's own square* (the anchoring king's file), not a global fact -- so only that one
  piece's own resulting square is guaranteed to land in the narrowed range; the other 11 identities
  are untouched. In a genuinely crossed table (HalfKAv2_hm) that narrowing multiplies through the
  whole table for a real ~17.5% reduction (`2560 -> 2112`). In naive's flat, uncrossed table (one
  square axis shared by all 12 identities, not two axes multiplied together), the same trick only
  saves `11*(8+8)*d + 1*(4+8)*d = 188d` against `192d` unmirrored -- about 2%, and it stops being a
  clean product (`rows * identity * d`) once applied, unlike every other row in the table. Settled:
  not worth it for naive. `companion/rotor.md`'s own `naive_hm` rows (added, then found to use the
  wrong reduction, mixing up which mirror does what) should be corrected or dropped -- not yet
  decided which, and not this document's own concern until the naive design itself settles.

**Castling rights: encoded, reversing the earlier default.** DeepChess's argument for these bits
holds where the side-to-move argument doesn't: castling rights aren't recoverable from piece
positions alone (a king and rook sitting on their home squares doesn't prove the right survives --
they could have moved away and back), so mirroring can't make them redundant the way it does
side-to-move. Settled: naive encodes all 4 rights.

Consistent with this design's own symmetry philosophy (own/opponent relabeling, not absolute
White/Black, for exactly the same reason pieces are relabeled): the 4 bits are *own kingside / own
queenside / opponent kingside / opponent queenside*, not White/Black-labeled -- still 4 bits either
way, just reoriented by side-to-move like everything else, so the network isn't forced to learn the
same castling-safety pattern twice for two absolute colors.

Encoding mechanism, following DeepChess's own precedent rather than inventing a new one: DeepChess
folds its 5 extra bits into the *same* 773-wide input feeding its first dense layer, which is
mathematically identical to giving each bit its own row in a sparse accumulator table (a dense
layer over a multi-hot vector is exactly "sum the weight columns for whichever inputs are active,"
the same formalism `rows * d` already describes for pieces). Naive does the same: 4 more flat rows
in the *same* accumulator, each just a bare 0/1 flag with no square to cross against, rather than a
separately-concatenated side input to the tail MLP. Table size becomes:

**`((8+8) * 12 + 4) * d = 196 * d` rows**, replacing the earlier `192 * d`.

**En passant: encoded too, as file only, not square.** The target square's *rank* is never free
information -- by the rules it's always rank 3 (White just pushed) or rank 6 (Black just pushed),
and which of those two applies is already fully determined by side-to-move (the mover now is never
the side that just pushed). Encoding the full square would spend rows on an axis with zero
additional information beyond what side-to-move already gives for free. Only the *file* (0..7) is
independent, and `oriented_square`'s vertical mirror only ever flips rank, never file, so the file
value needs no reorientation at all -- unlike pieces or castling rights, it's already
color-invariant as-is.

Same sparse-accumulator logic as everything else here: no "no en passant" placeholder row needed --
when no capture is available, none of the 8 file rows fire, exactly like an empty square
contributes nothing. `+8` rows, one per file.

Worth a conscious call, not a default: en passant availability is rare (one ply, only right after a
two-square pawn push) but genuinely tactically relevant when it applies, unlike the `naive_hm`
question earlier (which was cheap complexity for near-zero benefit) -- this is small, fixed row
cost for a rare-but-real signal, a different tradeoff shape. Settled here as "include it," but
flagged in case that's worth revisiting once real training data shows how much it actually moves
loss.

**Real consequence these two decisions force, beyond naive itself:** castling rights and en passant
currently exist nowhere in the pipeline naive would need them from. `CanonicalPosition` (the
shared, feature-set-agnostic struct every feature set's own `expand` reads from, `features.rs`'s
own doc comment) has neither field today -- only `side_to_move`, `pieces`, `white_king`,
`black_king`. Since a cache is `prepare`d once and every feature set's `expand` only ever reads back
what got written to disk, both fields *must* be added to `CanonicalPosition` itself, not synthesized
naive-locally at `train` time -- there's no `Board` left to read them from by then. That means:

- `cache.rs`'s on-disk record format changes (two new fields to pack/unpack -- one `FORMAT_VERSION`
  bump covers both, no need for two separate migrations), which under this project's own existing
  discipline (`CacheHeader`'s magic + `FORMAT_VERSION`) means treating every existing `prepare`d
  cache as stale -- a real, not cosmetic, breaking change, not something to sneak in as a side
  effect.
- `extract_canonical` (`features.rs`, called from `builder.rs` at `prepare` time) needs read access
  to both off of `Board` -- needs confirming `Board` actually exposes castling rights and the en
  passant target square/file in a form `extract_canonical` can read, not yet checked.
- HalfKP and HalfKAv2_hm don't need either new field (their own design deliberately excluded both,
  `rotor-halfkp.md`/`rotor-halfkav2hm.md`'s own scope), but once `CanonicalPosition` carries them,
  they exist in every cache regardless of which feature set that cache ends up training under --
  unused by two of the three, used only by naive. Not a problem, just worth being clear-eyed about:
  this is a shared-infrastructure change motivated by one feature set's own decisions, not a
  naive-local addition.

**Single perspective, not two.** HalfKP/HalfKAv2_hm compute two independently-weighted crossed
accumulators (one per king) and concatenate them into `2*d`-wide input to the tail MLP. Naive has
no king to anchor a second, independently-weighted view off of -- a single flat `(8+8)*12`-row
table, `d`-wide output, is the whole accumulator. This also confirms the table's own existing
"for 2 perspectives" annotation correctly does *not* apply to the naive rows.

**Table size: `((8+8) * 12 + 4 + 8) * d = 204 * d` rows** (`+4` castling rights, `+8` en passant
file, both above), superseding `companion/rotor.md`'s current `(8 + 8) * 12 * d` row -- that row
predates both decisions and needs updating once this document's own design settles. Mirroring still
changes which row a square maps to, never the row count, same as HalfKP/HalfKAv2_hm.

**Code organization follows the existing three-way extension points, not new ones.** The
HalfKP/HalfKAv2_hm rollout already built `FeatureSet` (enum), `DenseBatch` (enum), `Network` (enum)
specifically to make adding a third variant mechanical: a `features::naive` submodule alongside
`features::halfkp`/`features::halfka_v2_hm`, a `FeatureSet::Naive` variant, `DenseBatchNaive`/
`NetworkNaive` structs, one more arm on each existing match. No new infrastructure pattern needed.

**Training methodology: settled as plain end-to-end supervised, not DeepChess's autoencoder
pretraining.** Naive trains through Rotor's own existing AB(lambda) pipeline exactly the way
HalfKP/HalfKAv2_hm already do -- one supervised pass, no unsupervised Pos2Vec-style reconstruction
stage, no pairwise comparator objective. This resolves the tension the DeepChess-widths hypothesis
ran into (below): since naive's own architecture is no longer borrowed from DeepChess's numbers at
all, there's no "untested combination" risk to worry about any more -- the architecture and the
training method are both native to Rotor's own existing practice now, not a mismatched graft.

**Tail architecture: derived directly, own numbers, not DeepChess's.** The DeepChess Pos2Vec widths
(`600 -> 400 -> 200 -> 100`) were considered and dropped -- not because they were shown to be wrong,
but because they were never validated apart from DeepChess's own pretraining regime, which naive
isn't using (settled above), so borrowing them would have been an untested combination. Replaced
with a shape derived from naive's own dimensions instead.

First attempt, a single hidden layer via the classic "geometric mean of input and output" rule of
thumb (`H = sqrt(input * output)`): with `input = 204` (naive's own raw sparse feature count,
the same role `773` plays for DeepChess) and `output = 1` (the true scalar), `H = sqrt(204) ~= 14`.
Extending the same rule to two hidden layers (a geometric progression `input, H1, H2, output`)
gave `H1 ~= 34.6, H2 ~= 5.9` -- and `H2` in particular is far too small. This isn't a rounding
problem: geometric progression shrinks by a constant *ratio* each step, so whichever term sits
closest to a small endpoint like `1` gets crushed multiplicatively, no matter how many extra hidden
layers get inserted before it. The geometric-mean rule of thumb was devised for moderately-sized
outputs (e.g. classifiers with dozens of classes); a literal scalar output is a degenerate case for
it.

Fixed by switching to a *linear* (arithmetic) progression instead of a geometric one -- shrinking by
a constant *amount* each step rather than a constant *ratio*, which doesn't collapse near a small
endpoint the way geometric progression does. For `n` hidden layers between input `I` and output `O`:
common difference `d = (O - I) / (n + 1)`, then `H_k = I + k*d` for `k = 1..n`. For naive's own case
(`I = 204`, `O = 1`, `n = 2`): `d = (1 - 204) / 3 ~= -67.7`, giving `H1 ~= 136`, `H2 ~= 69`.

Rounded to the nearest power of 2 for practical convenience (hardware-friendly tensor shapes,
matching this codebase's existing preference for round widths): **`204 -> 128 -> 64 -> 1`**. A
useful side effect of where the derived numbers happened to land, not a forced coincidence: `128`
is exactly Rotor's own existing `d` for HalfKP/HalfKAv2_hm, so all three feature sets now share the
same accumulator width before their own tails diverge.

```
accumulator (204 rows, summed, ReLU)  -- d = 128
  -> 128 -> 64 (ReLU)
  -> 64 -> 1 (sigmoid)
```

Both hidden layers land at comfortable, non-degenerate sizes without needing an artificial anchor
value the way the geometric version would have (an earlier draft of this fix proposed anchoring the
geometric progression to `D = 128`, Rotor's own existing `d`, specifically to dodge the collapse --
retired once linear progression solved the same problem directly, using the real output (`1`), no
stand-in required; `128` reappears here anyway, as a rounding target rather than a derivation input).

**`128`/`64` accepted as-is.** Not independently re-derived or tuned beyond the linear-progression
fix above -- going with these for now. Whether they're actually well-sized for naive's own task
stays an open check against real training data, not a design question to resolve on paper first,
the same "measured, not assumed" standard this project already holds itself to elsewhere.

**Castling rights and en passant stay naive-only.** HalfKP and HalfKAv2_hm keep excluding both,
per their own original designs (`rotor-halfkp.md`/`rotor-halfkav2hm.md`) -- `CanonicalPosition`
gaining the two new fields for naive's sake doesn't change that. The two existing feature sets'
own `expand` implementations simply never read the new fields.

**`Board` already exposes both fields, directly and cleanly.** Checked against `src/board/mod.rs`
and `src/board/castling_rights.rs`:

- `Board::en_passant_square() -> Option<Square>` (`mod.rs`), and `Square::file()` (`square.rs`)
  gets the file-only value the design calls for -- no rank ever needs reading.
- `Board::castling_rights() -> CastlingRights` (`mod.rs`), a packed 4-bit struct with
  `kingside(Color)`/`queenside(Color)` accessors (`castling_rights.rs`) -- exactly the four rights
  `extract_canonical` needs, ready to reorient into own/opponent form the same way pieces are.

No gap. `cache.rs`'s format change can proceed as scoped above.

## Open questions (where the discussion currently is)

None currently open -- naive's design is settled pending implementation.

## Implementation plan

Surveyed the existing `rotor-train` infrastructure directly (file:line references below) rather
than guessing at it. Naive slots into the existing `FeatureSet`/`DenseBatch`/`Network` three-way
extension pattern, with one real architectural wrinkle: naive is **single-perspective** (one flat
`204`-wide dense vector per position), not the two-perspective own/opponent split HalfKP/
HalfKAv2_hm use (`own_dense`/`opponent_dense`, concatenated before the tail MLP). `DenseBatchNaive`/
`NetworkNaive` get their own shape rather than copying that split verbatim -- this is the one place
naive's implementation genuinely diverges from the existing two feature sets' code shape, not just
their numbers.

Ordered steps (adding a new exhaustively-matched enum variant leaves the crate non-compiling
partway through by design -- steps 1-2 compile standalone, steps 3-7 need to land together before
`cargo build` succeeds again, same as any single-session enum-variant addition):

1. **`CanonicalPosition` + `extract_canonical`** (`src/bin/rotor_train/features.rs:19-27, 81-98`).
   Add two fields: `castling_rights: CastlingRights` (stored absolute, same convention as
   `pieces`/`white_king`/`black_king` -- reoriented to own/opponent at `expand` time, not here) and
   `en_passant_file: Option<u8>` (0..7, extracted directly via `Square::file()` on
   `Board::en_passant_square()` -- no reorientation ever needed, per the design's own file-only,
   rank-is-implicit reasoning above). `extract_canonical` reads both off `Board::castling_rights()`
   and `Board::en_passant_square()` (`src/board/mod.rs:97-98, 110-111`), confirmed both already
   public and directly usable. Update `CanonicalPosition`'s own doc comment (still feature-set-
   agnostic, now used by three feature sets instead of two).

2. **`cache.rs` on-disk format** (`src/bin/rotor_train/cache.rs`). One extra byte per record: bits
   0-3 = `CastlingRights`' own raw byte (its existing bit layout reused as-is, no reinvention),
   bit 4 = en-passant-present flag, bits 5-7 = en passant file when present. Written/read right
   after the two king-square bytes, before the label, in both `write_record`/`read_record`.
   `FORMAT_VERSION` bumps `2 -> 3` (`cache.rs:18`) -- every existing `prepare`d cache becomes stale
   and must be regenerated, expected and already flagged in this document's own "settled" section
   above. Update the three round-trip tests that hardcode record byte counts (`cache.rs:177-221`):
   sparse record `8 -> 9` bytes, full 30-piece record `68 -> 69` bytes.

3. **`FeatureSet` enum** (`src/bin/rotor_train/feature_set.rs:8-50`). Add `Naive` variant,
   discriminant `2` (next free slot after `HalfKp = 0`, `HalfKav2Hm = 1`), CLI string `"naive"` in
   both `from_cli_str`/`as_cli_str`, and the `from_discriminant` panic-on-unrecognized arm. Extend
   the two tests that iterate `[FeatureSet::HalfKp, FeatureSet::HalfKav2Hm]` literally
   (`feature_set.rs:127, 144`) to a three-element array.

4. **`features::naive` submodule**, inline in `features.rs` alongside `halfkp`/`halfka_v2_hm`
   (matching the existing one-file, `pub mod` per feature set convention, not a new directory
   layout). Own `PositionFeatures`, own constants (`IDENTITY_COUNT = 12`,
   `PIECE_ROWS = (8+8)*12 = 192`, `CASTLING_ROWS = 4`, `EN_PASSANT_ROWS = 8`,
   `ROWS = 204`), own `pack_piece`/`unpack_piece` for the flat (non-crossed) piece rows, and an
   `expand` that: reuses the shared `oriented_square` (`features.rs:49-63`, already
   feature-set-agnostic, no change needed) for the vertical/side-to-move mirror only, computes each
   piece's `identity = piece_type * 2 + (0 if own else 1)` (no king exclusion, unlike
   `halfkp`/`halfka_v2_hm`), reorients `castling_rights` into own-kingside/own-queenside/
   opponent-kingside/opponent-queenside using `side_to_move` (same "reorient at expand time, store
   absolute on `CanonicalPosition`" split pieces already use), and passes `en_passant_file` straight
   through unmirrored. No `hm`-style king-file mirror (settled: not worth it for naive's flat
   table). Own `#[cfg(test)] mod tests`, modeled directly on `halfka_v2_hm`'s own
   (`features.rs:474-544`): pack/unpack round trip, a starting-position piece-count sanity check,
   plus naive-specific cases for castling reorientation and en-passant-file passthrough that
   `halfka_v2_hm`'s tests have no analog for.

5. **`batch.rs`**. `DenseBatchNaive { dense: Tensor, label: Tensor }` -- one `[batch, 204]` tensor,
   not the `own_dense`/`opponent_dense` pair `DenseBatchHalfKp`/`DenseBatchHalfKav2Hm` carry
   (`batch.rs:21-51`). `to_dense_batch_naive` calls `naive::expand` once per record (not twice) and
   accumulates directly into the single dense row -- no crossed-row nesting like
   `accumulate_perspective_halfka_v2_hm`'s `king_slots x piece_slots` double loop (`batch.rs:79-91`),
   since naive has no king anchor to cross against; each active feature index increments its own row
   directly. Add the `DenseBatch::Naive` arm to `to_dense_batch`'s dispatch (`batch.rs:156-161`) and
   to `DenseBatch::label`'s match (`batch.rs:31-34`). Tests mirroring `batch.rs:195-210, 283-329`'s
   halfka_v2_hm shape/accumulation tests, adjusted for the single-tensor shape.

6. **`network.rs`**. `NetworkNaive { transform: nn::Linear, hidden1: nn::Linear, output: nn::Linear }`
   -- three linear layers, not four (`transform`/`hidden1`/`hidden2`/`output` in
   `NetworkHalfKav2Hm`), matching the settled `204 -> 128 -> 64 -> 1` shape exactly: `transform`
   (204->128, ReLU on the summed accumulator), `hidden1` (128->64, ReLU), `output` (64->1, sigmoid).
   `forward` takes one `dense: &Tensor` argument, not two -- no `Tensor::cat` step, since there is
   only one perspective to begin with. Add `Network::Naive`/`DenseBatch::Naive` arms to both
   `Network::new` (`network.rs:21-24`, exhaustive, compiler-enforced) and `Network::forward`
   (`network.rs:34-40`, **has a wildcard `_ => panic!(...)` arm, so this one will silently fall
   through to the mismatch panic instead of a compile error if forgotten** -- add it deliberately).
   Extend the parametrized tests at `network.rs:139, 151` (and the `zero_batch` helper's own
   `match feature_set` at `network.rs:116-135`) to cover `Naive`.

7. **`main.rs` CLI wiring**. `prepare` needs no change (feature-set-agnostic, confirmed at
   `main.rs:60-107` -- it never references `FeatureSet` at all). `train`'s `--features` flag: add
   `naive` to the `USAGE` string (`main.rs:48`) and the `--features must be one of ...` error message
   (`main.rs:241`). `DEFAULT_FEATURE_SET` (`main.rs:112`) stays `HalfKav2Hm` -- naive is opt-in via
   explicit `--features naive`, not a default change.

8. **`window.rs`** end-to-end test (`window.rs:219-255`, the one test that runs a full `prepare`d
   cache through `to_dense_batch` + `Network::forward` together): extend its feature-set
   parametrization to include `Naive`.

9. **`companion/rotor.md`** table. Replace the current `Rotor naive | (8 + 8) * 12 * d | alpha-beta`
   row with `Rotor naive | ((8+8)*12 + 4 + 8) * d = 204 * d | alpha-beta`. Drop the `naive_hm` rows
   entirely rather than correcting their reduction figure: this document's own "settled" section
   above rejected the hm mirror for naive outright (not worth it, ~2% saving, breaks the clean
   `rows * identity * d` product), so a corrected-but-still-unused row would just be documenting a
   rejected approach rather than naive's actual design. Flagging this call explicitly since the
   design section above left it as "not yet decided which, and not this document's own concern
   until the naive design itself settles" -- it's being decided now, as part of landing this plan,
   not a re-opening of the earlier discussion.

Not itself part of this plan, per this document's own scope section above: no play-time loading of
naive (or any feature set) into the shipped `rotor` UCI binary. Confirmed via a broader grep of
`src/` outside `rotor_train` that no hand-written Rust NNUE inference path exists yet for any
feature set -- naive has no prior art to diverge from there, only the tch-rs training path above.

## Progress

All 9 implementation-plan steps above applied to the working tree:

1. `CanonicalPosition` gained `castling_rights`/`en_passant_file`; `extract_canonical` populates
   both off `Board::castling_rights()`/`Board::en_passant_square()`.
2. `cache.rs` bumped `FORMAT_VERSION` `2 -> 3`, packs both new fields into one extra byte per
   record, updated its own round-trip tests' byte counts and added two new tests for the packed
   byte itself.
3. `FeatureSet::Naive` added (discriminant `2`, CLI string `"naive"`), with its own marker/CLI
   round-trip tests extended.
4. `features::naive` submodule written: `PositionFeatures`, `pack_piece`/`unpack_piece`,
   `piece_identity`, `expand` (single perspective, king included, no hm mirror, castling
   reoriented to own/opponent, en passant file passed through), `ROWS = 204`, own test module.
5. `batch.rs`: `DenseBatchNaive` (single `dense` tensor, no own/opponent split), `accumulate_naive`
   (2 rows per piece via file/rank lanes, no king crossing, flat castling/en-passant rows appended
   after the 192 piece rows), `to_dense_batch` dispatch and `DenseBatch::label` arms, three new
   tests.
6. `network.rs`: `NetworkNaive` (`204 -> 128 -> 64 -> 1`, three linear layers, single-tensor
   `forward`, no `Tensor::cat`), `Network::new`/`Network::forward` arms (the latter's wildcard-arm
   risk noted in the plan -- added deliberately), parametrized tests extended to all three feature
   sets.
7. `main.rs`: `USAGE` string and `--features` error message now mention `naive`; `prepare` and
   `DEFAULT_FEATURE_SET` left untouched per the plan.
8. `window.rs`'s end-to-end test extended to all three feature sets.
9. `companion/rotor.md`'s `Rotor naive` row updated to `((8 + 8) * 12 + 4 + 8) * d`, with a new
   footnote explaining why that row (uniquely) includes castling/en passant while every HalfKP/
   HalfKAv2_hm row still doesn't. No `naive_hm` rows existed in the actual file to drop (checked
   directly -- the earlier design-phase concern about them must have referred to a draft that was
   never actually committed).

Verified by the developer: `cargo test --bin rotor-train --features training` -- 95 passed, 0
failed. (The `training` Cargo feature gates `rotor-train` entirely, per `Cargo.toml` -- `tch`/
`zstd` are optional dependencies behind it; this session's first suggested command omitted it.)

Still outstanding, unrelated to whether it compiles and passes tests: the two deferred
play-time-loading and HalfKP/HalfKAv2_hm-adopting-the-new-fields questions remain out of scope per
this document's own settled sections above, and the open capacity question (`128`/`64` sizing)
still needs real training data once an actual training run becomes possible.

Implementation complete and verified. This document is ready to move from `active/` to `done/`
once the developer confirms there's nothing else to resume here.
