# Label scale: `atan`/`tan` instead of `tanh`/`atanh`

**Done:** the atan/tan label scale is implemented in `src/labels.rs` with `CEILING_PAWNS = 32`; networks output pawns. The retraining it called for is today's lineages.

## Status

Implemented, awaiting `cargo test` (see the end). Constants decided: C = 32, 75% anchor.
Needs a full label rebuild and a retrain of every network (betty and psq included) once applied.

## Problem

Every network's centipawn output is capped at +/-7.64 pawns. `labels.rs` maps pawns to labels
with `tanh(SCALE * pawns)`, `SCALE = ln(2) / 2`, and the engine inverts with `atanh`. `atanh`
diverges at +/-1, so `clamped_pawns_from_score` clamps to `1 - CLAMP_EPS = 0.99` first, and
`atanh(0.99) / SCALE = 7.64`. The labels are clamped the same way, so the networks never see
anything past that.

Loosening only the clamp (0.999) moves the ceiling to 10.97 pawns but barely helps: `tanh`'s tail
is exponential, so large advantages are crushed together in label space (a queen is 0.9961, a
queen and a rook 0.99988, a gap of 0.004, under any network's error).

## The new pair

```
pawns_from_score(q) = A * tan(K * q)                 q clamped to [-1, 1]
score(pawns)        = atan(pawns / A) / K            result clamped to [-1, 1]
```

The shape is the one Lc0 uses to display its Q as centipawns, but none of Lc0's constants are
reused: `A` and `K` are derived below from two choices of Rotor's own.

Why this shape: `atan`'s tail is polynomial (`1 - score ~ A / (K * pawns)`), not exponential, so
large advantages stay apart in label space. And with `K < pi / 2`, `tan(K * q)` is finite and
increasing on all of `[-1, 1]`, so the inverse never diverges and needs no epsilon.

Both functions are odd, so `side_to_move_relative` and the negamax argument in `compute_labels`
are unaffected. The AB(lambda) blend still happens in label space, unchanged.

## The two constants, and what each one means

- `K` must lie strictly below `pi / 2`, where `tan` has its asymptote. At `K = pi / 2` a label of
  +/-1 would mean infinite pawns, the same problem `atanh` has. Above it, `tan` wraps around and a
  winning output would read as losing. Below it, the largest reportable evaluation is finite.
- `A` is a length in pawns: it sets where the curve bends. Near equality `pawns ~ A * K * q`;
  in the tail the label approaches 1 like `A / (K * pawns)`.

Rather than pick `A` and `K` directly, we pick two quantities with a chess meaning and derive both.

## Choice 1: the ceiling C = 32 pawns

`C` is the largest evaluation the network can express, `pawns_from_score(1) = A * tan(K)`.

Decided: **C = 32 pawns, all non-pawn material of one side**, with the queen counted as 10:

```
2 knights (6) + 2 bishops (6) + 2 rooks (10) + queen (10) = 32
```

Reasoning:

- A static evaluation more than a full army of pieces ahead carries no useful distinction. Positions
  that are truly decided (mates, tablebase results) never go through this curve: the search scores
  them itself, and `compute_labels` gives decisive plies a hard +/-1.
- Pawns are left out on purpose: they are the smallest unit of the scale, not part of the army.
- Counting the queen as 10 rather than 9 makes `C` a power of two, which makes the constants below
  exact and simple (`t = sqrt(15) / 4`, `cos K = 1 / 31`).
- 32 pawns (3200 cp) is well below the tablebase band (`TB_WIN_SCORE - MAX_PLY = 31799` cp), so a
  network eval can never be confused with a proven result.
- A small ceiling keeps `K` away from `pi / 2`, so the conversion back to pawns is gentle near the
  edge (see "Noise near the edge").

For comparison, Lc0 pins every one of its centipawn formulas to 12,800 cp (128 pawns) at `wl = 1`,
apparently as a display bound. Tested against the 75% anchor, `C = 128` would give nearly the same
curve body (queen at 0.934 instead of 0.947); it would only grade positions beyond 32 pawns, at the
cost of about 3x noisier large evals.

## Choice 2: one pawn is a 75% expected score

Decided: **an advantage of exactly one pawn maps to label 0.5**, which is a 75% expected score on
the `[0, 1]` scale (`expected = (1 + label) / 2`).

Reasoning:

- It makes one pawn the midpoint of the label range: half the distance from equality to certainty.
- It matches the popular reading of a +1.00 evaluation. Stockfish normalizes its displayed eval so
  that +1.00 is roughly a 50% win probability, which with the remaining games mostly drawn is an
  expected score of about 75%. Lc0's default formula puts 100 cp at about 77%.
- It replaces the old anchor (one pawn = 2/3 expected score, label 1/3), which was inherited from
  the `sigmoid(ln(2) * pawns)` formula rather than measured. The alternatives weighed were 60%
  (close to Lichess's win-percentage formula) and keeping 2/3.

Consequence: near equality the curve is about 1.8x steeper than today (0.629 labels per pawn
against 0.347), so training puts relatively more weight on small evaluations.

## Deriving t, A and K

Write `t = tan(K / 2)`. The two choices are two equations:

1. **Anchor.** `score(1) = 1/2` means `atan(1 / A) = K / 2`, so `1 / A = tan(K / 2) = t`:

   ```
   A = 1 / t
   ```

2. **Ceiling.** `C = A * tan(K)`. The double-angle identity `tan(K) = 2t / (1 - t^2)` gives

   ```
   C = (1 / t) * 2t / (1 - t^2) = 2 / (1 - t^2)
   ```

   so `t` depends on `C` alone:

   ```
   t = sqrt(1 - 2 / C)
   ```

With `C = 32`:

```
t = sqrt(1 - 2/32) = sqrt(15/16) = sqrt(15) / 4        ~ 0.968246
A = 1 / t          = 4 / sqrt(15)                      ~ 1.032796 pawns
K = 2 * atan(t)    = acos(1 / 31)                      ~ 1.538532
```

The closed form for `K` comes from `cos(K) = (1 - t^2) / (1 + t^2) = (1/16) / (31/16) = 1/31`.

Check: `A * tan(K) = (4 / sqrt(15)) * 8 * sqrt(15) = 32`, since `tan(acos(1/31)) = sqrt(960) = 8 * sqrt(15)`.

In code, `C` is the one literal; `t`, `A` and `K` are computed from it, so both meanings hold by
construction.

## Resulting curve

| Eval (pawns) | New label | Old label (`tanh`) |
|---|---|---|
| 1 | 0.500 | 0.333 |
| 3 | 0.805 | 0.778 |
| 5 (rook) | 0.889 | 0.939 |
| 9 (queen) | 0.947 | 0.9961 |
| 14 (queen and rook) | 0.973 | 0.99988 |
| 32 and above | 1 (clamped) | 0.99 (clamped) |

The queen vs queen and rook gap grows from 0.004 to 0.026, and the reportable range from +/-7.64
to +/-32 pawns.

## Clamps

1. **Forward (`score`).** `atan(p / A) / K` tends to `(pi / 2) / K ~ 1.021`, not 1, so any
   evaluation beyond 32 pawns (mate-scale scores included) would land past 1. The result is clamped
   to `[-1, 1]`; everything from 32 pawns upward is exactly +/-1.
2. **Inverse (engine and `rotor-train check`).** Required: a raw output past 1 (1.68 has been seen
   on a real checkpoint) would push `K * q` past `pi / 2`, where `tan` flips sign. Clamping `q` to
   `[-1, 1]` gives the finite ceiling of 32 pawns.

## Noise near the edge

`d pawns / d q = A * K / cos^2(K * q)`.

- Near equality: 0.001 of raw output is about 0.16 cp (today: 0.29 cp).
- At `q = 0.99`, about 21.7 pawns: 0.001 of raw output is about 0.7 pawns, roughly 3% relative.

## What changes in the code

- `labels.rs`: `SCALE` replaced by `C` (with `t`, `A`, `K` derived); `score`, `pawns_from_score`,
  `clamped_pawns_from_score` rewritten; `compute_labels`' clamps follow the new bound; doc comments
  and tests rewritten (the `2^p` closed form, 1/3, 31/33, 511/513 and the 0.99 literals all go).
- `length_cache.rs::is_trainable`: the `f32` saturation exclusion now triggers from about 3200 cp
  (32 pawns) instead of about 2500 cp. Still reachable, still meaningful; doc comment and tests
  updated with the new figure.
- `nnue.rs`: doc comment on `evaluate` (no more `atanh`). The engine's maximum eval rises from
  764 cp to 3200 cp, below the tablebase band and within `i16` for the TT.
- `round.rs`: tests compute their expectations through the functions, so they should survive.
- Search margins tuned in centipawns: pawns round-trip exactly, so an eval means the same number
  of pawns as before; only the networks' label distribution changes.
- Data: every label cache rebuilt, every network retrained. An old network loaded after the change
  reports wrong centipawns without any error.

## Decisions

1. `CLAMP_EPS` removed; every clamp is at exactly +/-1, so a decisive ply's label is a true +/-1.
2. No backward compatibility: no label-scale tag in the network file header. Old networks and
   caches are simply rebuilt.

## Status of the implementation

Applied to `labels.rs`, `length_cache.rs` (`is_trainable` doc and tests), `nnue.rs`, `round.rs`
and `lib.rs` doc comments. Not yet compiled or tested by the developer.
