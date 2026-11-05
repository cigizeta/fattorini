# Rotor HalfKP

**Done:** the HalfKP feature set is implemented in `rotor-train` (`FeatureSet::HalfKp`), replacing the MoE design; later layout work in rotor-halfkav2hm.md and rotor-naive.md. Data, training and selection are now one-learning-rate.md and round-robin-selection.md; test counts below are historical.

## Status

Implemented and confirmed: `cargo build --features training` and `cargo test --bin rotor-train
--features training` both clean (58 passed, 0 failed). `moe.md` moved to `claude/archived/` with
a superseded note -- this replaced the whole MoE mixture (`Accumulator`/`Router`/`Expert`/
`KingEmbedding`) outright, not just its `Bypass` piece.

## Settled architecture

**No bypass, no PSQT-style linear term, anywhere.** Checked Stockfish's real PSQT mechanism
against CCRL data (Ethereal, no such term, 3600; RubiChess, has it, 3604 -- a gap inside noise)
and found no controlled ablation anywhere isolating the bypass's own contribution.

**No castling-rights or en-passant encoding.** Stockfish's own shipped feature set
(`half_ka_v2_hm.h`) omits both entirely (a shogi-origin inheritance, not a tested chess-specific
decision); Rotor's own hand-crafted `eval.rs` already omits both too. Not a new gap. A concrete
encoding exists for later if wanted (a castling-eligible rook as its own piece identity; the en
passant target square marked) -- deferred, not abandoned.

**Feature representation**: classic HalfKP convention -- the king anchors the lookup, never
itself a `P`-type feature (the opponent's king's square is not represented at all, a known,
accepted limitation). Per perspective, every non-king piece is a triple `(perspective's own king
square, piece square, identity)`, identity 10-wide: 5 real piece types x 2 relative colors
(own/opponent).

King square and piece square are each factored into file-one-hot (8) + rank-one-hot (8) rather
than a full 64-way one-hot, while staying genuinely crossed against each other (the real,
non-separable king-x-piece-square interaction is preserved; only each square's own file/rank
split loses separability internally). One active piece visits 4 rows of a shared `16 (king
file/rank) x 16 (piece file/rank) x 10 (identity) = 2560`-row table -- (king file, piece file),
(king file, piece rank), (king rank, piece file), (king rank, piece rank), all at that piece's
identity -- summed. `ROWS_PER_PERSPECTIVE = 2560`.

**Accumulator width**: `d = 128` per perspective, concatenated to `256`-wide, own perspective
first.

**MLP after the accumulator**: `256 -> 32 (ReLU) -> 32 (ReLU) -> 1 -> sigmoid`. No phase input
anywhere -- the network infers game phase implicitly from which pieces are present in the sparse
feature list, the same way Stockfish's own deep MLP path does.

**No seeding.** Unlike the old `Bypass` (seeded from real `piece_value`/PST numbers), the
`transform`/hidden/output layers use `tch`'s ordinary random initialization -- there's no
equivalent closed-form seed for a factored, non-separable king-crossed table. Untrained Rotor's
raw output is near-`0.5` (a coin flip), not a material-and-PST-informed reading, until trained --
consistent with `training.md`'s "generation 0 is allowed to be weak" framing.

## Implementation (current state, by file)

1. `features.rs`: `PositionFeatures { own_pieces, opponent_pieces, own_king_file, own_king_rank,
   opponent_king_file, opponent_king_rank }` (no `phase`), `pack_piece`/`unpack_piece`,
   `ROWS_PER_PERSPECTIVE = 2560`.
2. `cache.rs`: `CacheRecord`/`write_record`/`read_record`/`RECORD_SIZE` follow the new shape;
   `MAX_INDICES_PER_PERSPECTIVE` renamed `MAX_PIECES_PER_PERSPECTIVE` (30, the real max: 15
   non-king pieces per side, kings never included).
3. `batch.rs`: `DenseBatch { own_dense, opponent_dense, label }`, each `[batch, 2560]`;
   `accumulate_perspective` sums (not sets) the 4 crossed rows per active piece, since two
   pieces can share a row.
4. `network.rs`: `Network { transform, hidden1, hidden2, output }` per the settled architecture;
   old `Bypass`/`seed_weights`/`KingEmbedding`/`Accumulator`/`Router`/`Expert`/`EXPERT_COUNT`
   deleted outright.
5. `epoch.rs`: `network.forward(...)` two-argument call site; fixtures updated to match.
6. `builder.rs`, `reader.rs`: no logic changes; only fixture literals updated.

`material.rs`'s doc comment (previously citing the deleted `Bypass`/`moe.md` as the reason it's
`pub`) now states honestly that `eval.rs` is its only remaining consumer; left `pub` regardless.
