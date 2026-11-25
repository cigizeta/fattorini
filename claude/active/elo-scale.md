# The network's unit on Elo's scale

**Status:** plan, 2026-11-25 (the developer: "write a plan, then implement for the next
experiment, with backward compatibility; strength should not change"). **Written the same day,
steps 1 to 8; `cargo test` and clippy clean the same day** (one test's "far past a won label"
moved from 20 to 50 pawns, about 29 units of the new S). **Then revised the same day:** the unit
moved from a stamp on each new attempt to the config's `unit:` key ("The unit is a config's"),
written; `cargo test` and clippy clean the same day. Left of step 9: the retrained network's
attempt (its config with `unit: elo`), its gate and its duels. As written: `S_PAWNS =
4 * LOG10_E`; the old teacher check is `refuse_old_teacher` (versions 1 and 2 refused); the frozen
set's manifest records `<tables> <curve>`, and a manifest from before keeps its set under an atan
curve (Lc0's, unchanged) and refuses it under the logistic (PeSTO's, read at the old S); the shown
score keeps reading each network through its own curve, so a GUI sees the same number for the same
expected value, about 0.41 of a version-3 network's centipawns near 0.

## The decision (the developer, 2026-11-25)

A network's centipawns are on **Elo's scale**: the logistic with base 10 and 400, the Elo formula
applied to centipawns,

```text
expected score = 1 / (1 + 10^(-cp / 400)) = sigma(cp / S),   S = 400 / ln 10 = 173.72 centipawns
```

Justification in one sentence: an advantage of 400 centipawns means the same expected score as a
400-point rating gap, odds of 10 to 1. A pawn up, scoring about 60% in Lc0's games, becomes about 70
network centipawns (`S * logit(0.60)`).

It replaces `S = A * B / 2 = 70.37` centipawns, taken from Lc0's display curve, everywhere: in
training, in the loss, in the files (the developer: "I do not want to see S = 70 anymore"). The old
S survives only as the meaning of an old file format, so that existing networks (the release, the
Lichess bot's snapshot) still load and play.

S is free, since the loss compares expected scores only (labels.rs, on `S_PAWNS`); strength does not
depend on it. Terms (expected score from 0 to 1, expected value from -1 to 1) and the discussion:
mini-cpw.md.

## What is read through which curve

| Curve | Reads | |
|---|---|---|
| Lc0's formula | Lc0's corpus | unchanged: it wrote those centipawns |
| Elo's logistic | PeSTO's corpus | new: its centipawns are PeSTO's own evaluations, written by no formula; a pawn up reads as about 64%, near what games show (the old S read 81%) |
| Elo's logistic | every network's output, in training and in the engine | new |
| the old logistic, `S = 70.37` | version-2 network files only | kept for backward compatibility |
| the atan curve | version-1 network files only | kept, as today |

## The unit is a config's (revised 2026-11-25, the developer)

No build can be assumed (the developer: "you cannot assume the old build"): an attempt running when
the new build arrives must finish under its own S, on that build. So the unit is a key of the
config, `configs/<name>.yaml`, beside `architecture:` and `training:` (the developer: "unit: elo can
be put into a configuration, not into an attempt"):

- `unit: elo`: Elo's S, files written as version 3; `unit: lc0`: the old S, version 2. Only these
  two, for now (no network uses anything else); later, everything retrained, `elo` alone, and the
  historical networks deleted.
- An attempt freezes its config at start, as before, so its unit travels with it. A frozen config
  with no `unit:` predates the key: `lc0`, so the attempt running at the change resumes under the
  old S on any build, exports version 2, and its training's frozen `curve` line still matches.
- **Absent, the unit is `lc0`, everywhere** (the developer, 2026-11-25, for simplicity): every config
  in `configs/` writes it, `unit: lc0` in all sixteen network configs of that day, `unit: elo` in
  every new one; no attempt is refused for lacking it.
- The loss takes S from the config (`NetConfig::s_pawns`), export the version
  (`NetConfig::network_version`), a self-play generation its teacher's curve (`NetConfig::curve`);
  a teacher or seed must be of the attempt's own unit (`refuse_teacher_in_another_unit`).
- The most promising network is retrained under `unit: elo` (the developer, 2026-11-25).

## Steps

1. **`labels.rs`:** `S_PAWNS = 4 / ln 10` (pawns), `LOGISTIC` the logistic at it; the old S kept as
   `OLD_S_PAWNS = A * B / 2` and `OLD_LOGISTIC`, documented as version 2's curve only. Tests: the
   base-10 Elo form equals `LOGISTIC`; `S_PAWNS * 100` about 173.72; `OLD_S_PAWNS * 100` about 70.37.
2. **`nnue.rs`, the format:** `VERSION = 3`, `OLD_LOGISTIC_VERSION = 2`, `ATAN_VERSION = 1`;
   `curve_of_version`: 1 atan, 2 `OLD_LOGISTIC`, 3 `LOGISTIC`; `from_bytes` reads all three, `write`
   writes 3. Tests: a version-2 file loads with `OLD_LOGISTIC` and its pawn measured through it; a
   written file reads back as version 3.
3. **`smithy`, training:** the loss (`nnue_network::bce_loss`, `stacked_network::arm_training_losses`
   and every user of `S_PAWNS`) reads the new S, unchanged in form.
4. **`smithy`, the attempt's unit:** the frozen config of a new attempt records `unit: elo`; a resume
   without it is refused ("begun under the old unit: finish it with the build it began with, or
   restart it").
5. **`smithy`, teachers:** PeSTO's corpus (`Source::curve`) is read through `LOGISTIC`, now Elo's.
   Self-play (paused) reads a teacher network through its own version's curve (`curve_of_version`),
   never a fixed one; the check refusing atan teachers (`smithy.rs`, near line 4590) accepts version 3
   only, since a version-2 network's attempt cannot be continued (above).
6. **Labels on disk:** the buckets record the curve their labels were made with (`labels_kind`), so
   PeSTO buckets labelled under the old curve are labelled again, never mixed. The frozen test set
   does not record it (`freeze.rs`: "the curve is not recorded"): it records it from now on, as the
   buckets do, and a set with no record or another curve is labelled again. Lc0's corpus and its
   frozen set are unaffected (Lc0's curve does not change).
7. **Unchanged, checked:** the polyline (measured in expected values); mate and tablebase bands,
   far above any evaluation in either unit (the 128-pawn ceiling clamps only where the expected
   value is already 1); the search's margins, translated for an untuned network through its measured
   pawn (about 70 instead of 28) and tuned from there.
8. **Documentation:** labels.rs's comments on S; `shown_centipawns`' comment (its example numbers);
   the README's Inside section if it names the unit; mini-cpw.md's centipawns and pawn scale pages
   in the new unit.
9. **Verification (the developer):** `cargo test`, `cargo clippy`; then the retrained network's
   first attempt, its pawn percent (about 70), its gate, and its duels against the network it
   retrains: strength unchanged, within the duels' noise.

## The shown score (the developer, 2026-11-25)

A version-3 network's score is shown to a GUI as it is, its own centipawns on Elo's scale, with no
conversion: "the fair test to see if the new S makes sense". Older networks keep Lc0's centipawns
for the same expected value (shown-score.md, 2026-11-20), so the release and the networks not yet
retrained look as before. With every network retrained, Lc0's tangent leaves the display, kept
only to read Lc0's corpus. For a sense of the scale: a pawn up about +0.70, an expected score of 75%
about +1.91, 99% about +7.98, where Lc0's display reads +0.29, +0.77 and about +46.

## Not in scope

- Re-exporting existing networks in the new unit: they stay version 2, with their tunes.
- Renaming `score` to "expected value" in the code (mini-cpw.md's terms): separate.
