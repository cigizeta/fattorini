# Training the big five

**Archived:** the big five network was retired 2026-10-03 and its code removed (branch carlo merged). "Betty" here is the old big five lineage; the name now means psq plus a hidden 16.

Implementation plan for the torch side of the big five architecture (`architectures.md`). The
engine side is done (`big-five-engine.md`): `src/big_five.rs` loads and evaluates a big five
`.nnue`, and `big_five::pesto()` builds the bootstrap's evaluation as big five parameters.

Branch: `alessio`. Network: `betty` (`configs/betty.yaml`, `kind: big_five`).

## Goal

`rotor-train train betty` trains a big five from the existing datasets, starting generation 1
from `pesto()`, and exports each lambda's candidate as a big five `.nnue` the engine loads, so
that the ladder, `promote`, and `install` work as for the other networks.

## Decisions

Settled in discussion.

- **Inputs come from the engine's own code.** Each record becomes a `Board`, and the trainer calls
  the same input builder the engine evaluates with (`big_five.rs`), so training and engine cannot
  index differently. This replaces a test comparing two implementations with one implementation.
- **The engine's form is trained.** Only the engine's active inputs: occupied squares, default
  states (non-pawn, other) left out. The default columns are never active, stay at zero, and are
  exported as zero. The same model as the plain form, without its redundant direction.
- **Loss.** The matching gradient of `architectures.md`: `((score(y).detach() - t) * y).mean()`,
  whose gradient on `y` is `score(y) - t`. `score` is `labels.rs`'s, with the same constants.
- **Reported loss.** The test loss, which the learning-rate stages and early stopping read and
  `history.tsv` records, is `mse(score(y), t)` in label space: the quantity `naive` and `psq`
  report, so the losses compare across architectures.
- **Initialization.** Generation 1 starts from `pesto()`, so its evaluation at step 0 is PeSTO's.
  An MLP all of whose weights are zero never learns (no unit is active, and no gradient reaches
  either layer), so every hidden unit `pesto()` leaves empty gets torch's default first layer,
  uniform in `±1 / sqrt(fan_in)`, and an output weight of 0. Pawn structure's unit 0 keeps its
  PeSTO pawn table. Later generations start from their champion, as for the other networks.
- **Halving in training, clamp in the engine only.** The pure opposite-color bishop halving is
  part of the model, so training applies it too, from the engine's own `is_pure_opposite_bishops`.
  The `±CEILING_PAWNS` clamp is not: in training it would zero the gradient of any overshoot past
  32 pawns, while `score` already saturates there.
- **Phase weights non-negative.** Clamped at 0 after every optimizer step.
- **Config.** `kind: big_five` takes no other field. `embedding_dim` and `value_head` move into the
  kinds that use them, and are refused under `big_five`.

## The model

For a batch, per side (side to move, then opponent), the inputs are index lists, sent as
`embedding_bag` indices and offsets:

- **Linear:** one bag over the shared linear table, `[LINEAR_INPUTS][2]` (middlegame, endgame),
  mode sum, plus a bias `[2]`. Material is one index per piece, so counts need no weights.
- **Each MLP:** one bag over `w1`, `[inputs][2 * hidden]` (middlegame then endgame columns), plus
  `b1`; then ReLU; then `w2` per half and `b2`, giving `[batch][2]`.

Then `value(side) = sum of the components' [batch][2]`, the phase `p` from the piece counts and
the 4 phase weights (`min(phase, S) / S`), `y = taper(value(side to move) - value(opponent))`,
halved where the record's opposite-bishop flag is set.

The variables are named after `BigFiveParameters`' fields, so export is a direct copy:
`linear` splits back into material, piece-square tables, mobility, and bishop pair at their
offsets, and each `w1` splits into its middlegame and endgame halves.

## Steps

1. **`features.rs`: a `Board` from a record.** `board_from_canonical(&CanonicalPosition) -> Board`,
   the inverse of `extract_canonical`, with a round-trip test over random playouts.
2. **`big_five.rs`: inputs for training.** A public function returning one position's active
   index lists per side (linear, pawn structure, king shelter, king danger), the opposite-bishop
   flag, and the phase counts, built by the code `evaluate` uses. Plus `pesto()`'s parameters in
   the engine's form, ready to copy into the model.
3. **`net_config.rs`.** A `BigFive` kind with no fields, `embedding_dim` and `value_head` moved
   into the kinds that need them. `configs/betty.yaml` loads as written.
4. **`src/train/big_five_network.rs`.** The model above, `new` (initialized from `pesto()` plus the
   random first layers), `forward`, and the phase-weight clamp.
5. **`batch.rs`.** A `BigFive` batch: the per-side indices and offsets of each component, the
   phase counts, the halving flag, and the labels.
6. **`mod.rs`.** `BigFive` in every match on the kind and feature set, the matching loss for
   training and the label-space MSE for the test loss, the clamp after each step, and generation
   1 seeded from `pesto()` rather than a random initialization.
7. **`export.rs`.** `big_five_parameters_from(&VarStore) -> BigFiveParameters`, and big five in
   `export_candidate` and `engine_runs`, so a candidate `.nnue` is written and `promote` works.

## As implemented

- The model, its batch, and the export (`parameters_from`) are one module,
  `src/train/big_five_network.rs`, rather than the export living in `export.rs`: the export is the
  exact inverse of the layout `NetworkBigFive::new` builds, so the two stay side by side.
- The export writes the default-state columns as exactly 0 whatever the store holds, since the
  engine folds them into the bias.
- The big five's training loss has the label-space MSE as its value and the matching loss's
  gradient (`mse + matching - matching.detach()`), so `history.tsv`'s `train_loss`, like
  `test_loss`, is the same quantity for every architecture. The first betty run, before this,
  logged the matching loss as `train_loss`.
- `run_config.txt` records `architecture` through its `Debug` form, which changed with the optional
  `embedding_dim`/`value_head`: a run started before this change is refused on resume. Every such
  run is retired anyway.
- `Network::of` builds any NNUE network from its config, so `check`, `calibrate`, and `correlate`
  work for the big five too, on its label-space prediction `score(y)`.

## Tests

- **Round trip.** `board_from_canonical(extract_canonical(board))` gives back the board, on random
  playouts.
- **Step 0 is PeSTO.** The freshly initialized model's `y`, through `score`, matches the engine's
  evaluation of `pesto()` on random playout positions, to f32 tolerance.
- **Export round trip.** A model with random weights, exported and loaded by the engine, evaluates
  the same positions as the model's own forward pass.
- **The gradient.** On a tiny batch, the gradient on `y` equals `score(y) - t`.
- **Clamp.** A negative phase weight is 0 after a step.
- **Config.** `kind: big_five` loads with no other field and is refused with `embedding_dim`.

## Verification

```
cargo test --release
```

Then a short real run on betty, with `rotor-train prepare betty` first if its dataset is not
built, and its exported candidate loaded by the engine.

## Out of scope

- Retiring the old `naive` and renaming `psq` (next in `TODO.md`).
- Moving the other networks to output pawns.
- Quantization.
