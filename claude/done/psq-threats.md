# PSQ plus threats

**2026-10-11: emily is replaced.** The threats are now tried as two lineages, both taught at
generation 1 by amanda-0002 on amanda's grid and training: caroline (`psq_threats`, no hidden layer)
and dorothy (`psq_threats`, hidden 16). With amanda and betty on the same data, the four cross the
hidden layer with the threats. `configs/emily.yaml` is deleted. Step 6 below now means training them.

"Naive" below is today's psq (renamed 2026-10-04, rename-naive-to-psq.md): `naive::` is
`features::psq`, and the engine's network struct is `PsqNetwork`.

2 * (64 * 12 + 12 + 64 * 64) -> 2 * d -> 16 -> 1

In the 64 * 64 threat matrix, bit[i][j] is true when square i contains a
piece of us that attacks a piece of them on square j.

Be careful about flipping.

## Decided with the developer (2026-10-03)

- **The lineage is emily** (`configs/emily.yaml`, a new architecture kind, `psq_threats`; named
  dorothy until 2026-10-04).
- **The name `psq_threats`**: the piece-square rows plus threats. Naive is to be renamed `psq`
  later, not in this plan; the big five's retirement frees the naming from it.
- **The threat rows are the perspective's own threats only**: in a perspective, row
  `(i, j)` is set when its own piece on `i` attacks an opponent piece on `j`. The other perspective
  holds the opponent's threats, so both directions are seen, and every row means the same thing in
  both perspectives, as naive's own/opponent identities do. 4,096 rows, after naive's 780: 4,876 a
  perspective, one shared transform.
- **Flipping**: as naive, a perspective is oriented by the vertical mirror (`square ^ 56`) for
  Black, applied to both squares of a threat: `(i, j)` is stored as `(i ^ 56, j ^ 56)`. A
  position and its colour-swapped mirror give the same features (a test, as naive's).
- **`d` is amanda's, 256**, then a hidden layer of 16, then the output.
- **The hidden layer is tried on amanda's naive too**, separately. Trained on the same dataset as
  emily, that naive with 16 hidden is the control that isolates the threats: betty
  (`configs/betty.yaml`, caroline plus the hidden layer, since 2026-10-04).
- **Data: the full new corpus**, caroline's `hand-crafted` dataset (the only one available): no
  replay of its own, since generation 1 is labelled by the hand-crafted evaluation.
- **Training: the same as caroline**, field for field: batch 4096, 1024 steps a round, rates
  `[6e-3, 2e-3, 6e-4, 2e-4, 6e-5]`, shuffle buffer 2^21, test batch 16384, lambdas 0.70 to 0.95.
- **Judged by a duel, emily 1 against amanda 1**, at 10+0.1, after emily's own selection (the
  round robin, then the SPRT gate at elo_d 35 against the hand-crafted evaluation). Against
  amanda the duel mixes the threats, the hidden layer and the new data; caroline against amanda
  measures the data alone, so the two duels read side by side.
- **Engine: the threat part recomputed at every evaluation**, as a first implementation.
  Stockfish (SFNNv10's threat inputs) keeps its threats in their own accumulator, updated
  incrementally, with 3-4x as many changed threat features per move as piece features; ours has
  about 10-30 active threats a perspective, so a recompute adds about as many rows as an update
  would. The threat part is kept in its own buffer nonetheless, so an incremental update can
  replace the recompute later without touching the network or the file.
- **Two accumulators a perspective, PST and threats, added; `d` unchanged.** One transform,
  4,876 x `d`. Per perspective, the PST accumulator holds the bias and the piece-square, castling
  and en passant rows, updated incrementally as naive's; the threat accumulator holds the threat
  rows, recomputed from zero at every evaluation. The two are added before the activation, so the
  head is naive's (`2d` = 512 -> 16 -> 1). The transform grows to about 1.25M weights
  (5 MB in f32), and the recompute costs about 10-30 rows of width `d` a perspective, so it scales
  with `d`. The split is the engine's, for speed only: the trained and exported network has one
  accumulator a perspective. Stockfish does the same (SFNNv10's accumulator combines HalfKA and
  threats, one L1 width for both): its threat sum is kept apart because it updates differently
  and its weights are i8, not because the network sees it apart.
- **Added, not concatenated, because the threats must interact with the pieces.** A threat row
  does not say which piece attacks which; the piece-square rows do. Added before the activation,
  one neuron sees both and can learn a conjunction (a weight from "own knight on `i`", one from
  "threat `(i, j)`", a negative bias: it fires only when both hold). Concatenated, the threats
  would meet the pieces only at the hidden layer of 16, after their own activation, and the head
  would read `4d`, so `d` would no longer mean amanda's. If emily fails to exploit the threats,
  the next step is not another layout but piece types inside the threat row, as Stockfish's
  (attacker piece on `i`, attacked piece on `j`): no conjunction left to learn, many more rows.

## Defaults, unless the developer objects

- **What "attacks" means**: the engine's plain attack sets. A slider stops at the first piece;
  pins are ignored; no x-rays; en passant is not a threat; the king attacks as any piece. Only
  attacks on opponent pieces, never defences of own ones (Stockfish includes both).
- **The plain 64 x 64 index**, dead cells included: only pairs a queen or knight move apart can
  ever be set (1,456 + 336 = 1,792 of 4,096, about 44%); the rest are rows never trained. A compact
  index can come later; the plain one is easier to check.

## Design

### Features (`features.rs`, a `psq_threats` module beside `naive`)

The naive rows unchanged (`naive::active_rows`), then `THREAT_ROW + 64 * i + j` for every threat
of the perspective, both squares oriented. The attacks come from a `Board` built from the
`CanonicalPosition`, through the engine's own attack generation, so training and engine cannot
disagree on what attacks what. `piece_row` stays public for the engine's incremental update; a
`threat_row(i, j)` joins it.

### Training

- `FeatureSet::PsqThreats`, `NetworkPsqThreats` in `nnue_network.rs`: naive's shape with
  4,876 rows and the value head `[16]` (Clipped ReLU, as naive's).
- The host batch as naive's: flat `i32` active indices, ones written on the device. The attack
  computation runs on the pipeline's workers (`parallel-batches.md`).
- `test-features-once.md`'s cache holds it as naive's, a little larger per position.

### Export and engine (`nnue.rs`)

- A new feature-set tag in the `.nnue` header; the layout as naive's (transform, bias, hidden
  layers, output), the transform 4,876 rows.
- The piece-square accumulators stay incremental, exactly naive's code. At evaluation, per
  perspective: the threat accumulator from zero plus every active threat row, added to the
  piece-square one, then the head. Checked: the hidden layer's activation is the same in the
  engine and in training.
- A full-refresh evaluation (`evaluate_pawns`) stays the reference every faster path is tested
  against, as for naive.

## Steps

Each its own commit, tests passing.

**Status (2026-10-14):** step 6 under way: caroline trains in generation 3 on the training machine, expected to
finish the same day. Earlier status (2026-10-03): steps 1 to 5 done (1 and 2 share a commit); step 6
waits for caroline's dataset and training.

**Step 4's result (the training machine, random weights, 256 and a hidden 16 for both):** per search node 504.7 ns
for naive, 536.9 ns for psq_threats, +6%; the threat recompute is about 31 ns of it, against about
410 ns for the 512 x 16 head; the update is unchanged (95 ns). In a depth-9 search, 1.07 against
1.18 us a node, +10%, likely the transform's 5 MB against 0.8 MB in the cache. Worth roughly 5-10
Elo at a fixed clock, which the threats must more than repay; an incremental threat update is not
worth building. The hidden layer of 16, not the threats, is most of a node's evaluation cost. In
the engine, one `PsqNetwork` serves both kinds, with `threats` set from the file's tag (5):
`refresh_by_color` and `update` keep the piece-square accumulators, and `evaluate_by_color`, which
now takes the board, adds the threat accumulators before the head.

1. **Features**: `psq_threats::active_rows`. Tests: hand-built positions with known threats
   (a pawn attacking a knight, a rook blocked by a piece, a king attacking), the colour-swap
   mirror test, and that no row is ever set outside the queen-or-knight pairs.
2. **Training network and batch**: `FeatureSet::PsqThreats`, the network, the host batch. Test:
   the dense batch from the indices equals one built row by row.
3. **Export and engine**: the tag, the loader, the evaluation. Test: the engine's evaluation of an
   exported network equals the trainer's forward pass on the same positions.
4. **Bench**: nodes/s of the engine with an emily network against amanda's, at depth 10 on the
   bench positions, to know the recompute's cost before any game.
5. **`configs/emily.yaml`**: caroline's training, the new kind, hidden `[16]`.
6. **Run**, after caroline's training (the GPU is one): `smithy forge emily`, selection,
   then the duel against `amanda-0001` at 10+0.1.

The control (amanda's naive with a hidden layer of 16, on the same dataset) is its own plan.
