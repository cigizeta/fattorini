# Big five in the engine

**Archived:** the engine-side big five was implemented, then retired 2026-10-03 and its code removed (branch carlo merged). Its torch side: big-five-training.md, also archived.

Implementation plan for the engine side of the big five architecture, described in
`architectures.md` ("Big five in detail" and "Output units and training loss"). The torch side,
and moving `naive` and `psq` to output pawns, come later and are out of scope here.

Branch: `alessio`.

## Goal

The engine loads a `.nnue` file holding a big five network and evaluates positions with it,
directly in pawns, from bitboards, recomputing everything on every evaluation. A network built
in Rust from PeSTO's values ("the PeSTO file") must reproduce the bootstrap, architecture (1),
to within 1 cp. The same values later initialize training.

## Decisions

Settled in discussion; `architectures.md` holds the ones about the architecture itself.

- **Container.** The existing `ROTORNNU` container and `VERSION` 1, with a new feature-set tag,
  `FEATURE_SET_BIG_FIVE = 4`, and a body of its own (below). So `EvalFile`, the `rotor.nnue`
  auto-load, `rotor-train install`, and the arena players work unchanged.
- **Numbers.** The file always stores f32. The engine evaluates in f32. A later int8 path, if
  any, quantizes at load time from the same file, so that f32 and int8 can be profiled on one
  file. Each component's forward pass is its own small function, so that one can switch to int8
  without touching the others.
- **No accumulators.** Every evaluation builds its inputs from the bitboards. The architecture is
  small and structured, and is meant to be fast without incremental updates.
- **Output.** The evaluation `y` is in pawns, clamped to `±CEILING_PAWNS` (32), and the engine
  plays `round(100 * y)` centipawns. No `clamped_pawns_from_score`.
- **Hidden layers.** One at most, plain ReLU, evaluated sparsely: the bias plus one weight column
  per active input.
- **Vectorized, decided during implementation.** No scalar loops in the evaluation's arithmetic.
  Two representations: `BigFiveParameters` (the file's and the trainer's view, each component's
  middlegame and endgame networks separately) and `BigFiveNetwork` (the engine's, built once at
  load):
  - the four linear components share one table of (middlegame, endgame) pairs, so they are one
    gather over one active list, material being one active input per piece;
  - each MLP keeps its middlegame and endgame first layers side by side, so one
    `add_assign_avx2` per active input updates both, then one `relu_avx2` and two
    `dot_product_avx2`;
  - in pawn structure and king shelter the default state of each square (non-pawn, other) is
    folded into the bias, and every other state's column becomes its difference from the default,
    so only occupied squares are active, not all 48 or 64.
- **Taper.** `taper(N)` is two networks of shape `N`, middlegame and endgame, with separate
  parameters, blended by `p`: `mg * p + eg * (1 - p)`.
- **Code.** A new `src/big_five.rs`, plus a small shared module for the definitions the
  hand-crafted evaluation and the big five have in common.

## Evaluation

For a board, with `us` the side to move:

1. **Phase.** `phase = w_N * #N + w_B * #B + w_R * #R + w_Q * #Q`, counting both colors' pieces,
   `S = 4 * w_N + 4 * w_B + 4 * w_R + 2 * w_Q`, `p = min(phase, S) / S`. The weights come from
   the file.
2. **Per perspective.** For each color `c`, `f(c)` is the sum of the seven tapered components
   (material, piece-square tables, mobility, pawn structure, king shelter, king danger, bishop
   pair), each computed from `c`'s oriented inputs (below). The linear components add their
   middlegame and endgame weights into two running sums. The MLP components compute both their
   networks.
3. **Difference.** `y = f(White) - f(Black)`.
4. **Opposite-color bishops.** `y /= 2` in the pure case: one bishop each, on squares of opposite
   colors, no knights, rooks, or queens.
5. **Clamp.** `y = clamp(y, -CEILING_PAWNS, CEILING_PAWNS)`.
6. **Side to move.** Negate for Black, then `round(100 * y)` centipawns.

### Inputs

`o(sq)` is the square oriented for the perspective's color: `sq` for White, `sq ^ 56` for Black.
"Own" and "enemy" are relative to the perspective's color. Piece-type orders are fixed as listed.

| Component | Inputs | Index | Active |
|---|---|---|---|
| Material | 5 | own piece count per type, pawn, knight, bishop, rook, queen | counts |
| Piece-square tables | 5 * 64 | `t * 64 + o(sq)`, `t` in knight, bishop, rook, queen, king, own pieces | 0/1 |
| Mobility | 66 | offsets: knight 0..8, bishop 9..22, rook 23..37, queen 38..65; `offset(t) + m` | counts |
| Pawn structure | 48 * 3 | `(o(sq) - 8) * 3 + state`, ranks 2..7; state 0 non-pawn, 1 own pawn, 2 enemy pawn | one-hot per square |
| King shelter | 64 * 4 + 2 | `o(sq) * 4 + state`; state 0 other, 1 own king, 2 own pawn, 3 enemy pawn; then 256 own kingside right, 257 own queenside right | one-hot per square, plus rights |
| King danger | 9 * 64 * 4 | `(r * 64 + o(s)) * 4 + t`; `r` the ring slot, `s` an enemy piece's square, `t` in knight, bishop, rook, queen | 0/1 |
| Bishop pair | 1 | own bishops >= 2 | 0/1 |

- **Mobility** of a piece: the squares it attacks that its own side does not occupy, sliders
  stopping at the first piece of either color. No mobility area, no x-rays.
- **King ring:** the 3 * 3 block around the own king, its file clamped to b..g and its rank to
  2..7, so always 9 squares. Slot `r` numbers the block from the perspective's own side,
  `r = 3 * (oriented rank offset + 1) + (file offset + 1)`, offsets in -1..1 from the clamped
  center.
- **King danger bit** `(r, s, t)`: the enemy piece of type `t` on `s` attacks ring slot `r` by
  the mobility rule, so a ring square held by the attacker's own piece does not count. Defenders
  are ignored.

The tests below pin every index, so that the torch side can copy them exactly.

## File format

After the common header (`MAGIC`, `VERSION`, `feature_set = 4`), all little-endian:

1. `step: u64`, `learning_rate: f64`: provenance, as for the other networks.
2. Phase weights: 4 f32, knight, bishop, rook, queen.
3. The seven components, in the order of the inputs table. For each:
   - `input_dim: u32`, checked against this build's value for the component;
   - `hidden_dim: u32`, checked like `input_dim`: 8, 32, and 32 for pawn structure, king shelter,
     and king danger, as `architectures.md` fixes them, and 0 for the four linear components.
     Fixed at compile time, so that each hidden layer is an array of known size;
   - the middlegame network, then the endgame network. A linear network is `weight[input_dim]`
     then `bias`. An MLP is `w1[input_dim][hidden_dim]` (input-major, so one input's column is
     contiguous for the sparse sum), `b1[hidden_dim]`, `w2[hidden_dim]`, `b2`.
4. Nothing else: trailing bytes are an error.

The loader dispatches on the tag before `shape_of`, which refuses unknown tags, and reuses
`nnue.rs`'s `Reader`. The writer reuses `nnue.rs`'s byte conventions. Output biases cancel in
`f(White) - f(Black)` but are stored anyway, so that every network has the same form.

## The PeSTO file

`big_five::pesto() -> BigFiveParameters` builds the bootstrap's evaluation as a big five network, in
pawns (PeSTO's centipawns / 100):

- **Phase weights:** 1, 1, 2, 4.
- **Material:** PeSTO's `MG_VALUE` and `EG_VALUE` for pawn to queen.
- **Piece-square tables:** PeSTO's knight, bishop, rook, queen, and king tables, read at
  `o(sq) ^ 56` (PeSTO prints a8 first).
- **Pawn structure:** PeSTO's pawn tables through one hidden unit kept in its linear range. Its
  input weight for "own pawn on sq" is the table entry, every other input weight is 0, its bias
  is 10 (above the most negative sum of 8 pawn entries), its output weight is 1, the output bias
  is -10. The other hidden units are all zero.
- **Mobility, king shelter, king danger, bishop pair:** zero.

A way to write it to disk, for matches and for initializing training: a `rotor-train` subcommand,
`bin/rotor-train big-five-pesto <path>`.

## Steps

1. **Shared definitions.** Move `mobility_squares`, the clamped `KING_ZONE`, and
   `is_pure_opposite_bishops` from `eval.rs` to a new `src/eval_terms.rs`. `eval.rs` uses them
   from there. The rest of the disabled terms stay in `eval.rs` as dead code.
2. **`labels.rs`.** Make `CEILING_PAWNS` public and no longer test-only.
3. **`src/big_five.rs`.** `BigFiveParameters` with `write` and `read`, `BigFiveNetwork::new`
   (the fused engine form), the input builders, and `evaluate(board) -> i32`.
4. **`nnue.rs`.** `FEATURE_SET_BIG_FIVE = 4` with its name, a `Network::BigFive` variant,
   dispatch in `Network::load` before `shape_of`, and the `evaluate` arm. Search needs no change:
   it already calls `network.evaluate(board)` for any network but `psq`.
5. **The PeSTO file.** `big_five::pesto()` and the `rotor-train big-five-pesto` subcommand.
6. **Benchmark.** A big five case in `benches/nnue_bench.rs`, next to the hand-crafted
   evaluation, to compare their speed.

## Tests

- **Inputs.** For hand-picked positions, the exact active indices of each component, Black's
  perspective included (orientation), and the ring slots of a king on g1, h1, e4, and a8.
- **Fused against plain.** The fused hidden layers, defaults folded in, equal a plain dense
  evaluation of the parameters with every square's state given.
- **Mirror symmetry.** A position and its color mirror evaluate the same, as in `eval.rs`'s test.
- **Round trip.** `write` then `load` gives back the same network, bit for bit.
- **Refusals.** Trailing bytes, a wrong `input_dim`, and a nonzero `hidden_dim` on a linear
  component are each refused with a message naming the component.
- **Reproduction.** The PeSTO file evaluates within 1 cp of `eval::evaluate` on the `eval.rs` test
  positions and on several thousand positions from deterministic random playouts, except that in
  pure opposite-color bishop positions it is compared against half of `eval::evaluate`.
- **Halving and clamp.** The pure opposite-bishop halving, and a clamp at exactly
  `±CEILING_PAWNS`, each with a network built for it.

## Verification

```
cargo test --release
```

Then a match at a fixed depth between the bootstrap and the PeSTO file, which must play the same
moves, to confirm the evaluation inside search, and to compare speed:

```
bin/rotor-train big-five-pesto data/networks/big-five-pesto.nnue
```

The match command is settled once `rotor-arena` can name a network file as a player; until then,
`cutechess-cli` with `option.EvalFile=` set to the file, as for the PeSTO and Michniewski match.

## Out of scope

- The torch implementation and training.
- Moving `naive` and `psq` to output pawns.
- int8 quantization.
