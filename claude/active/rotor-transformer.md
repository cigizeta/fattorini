# Rotor Transformer (MCTS-paired evaluation and policy)

Resume with: "resume from rotor-transformer."

## Status

**Superseded in part, 2026-10-15:** this is alpha_pieces, and architectures.md's brainstorm now
sets its direction: a GPU engine for Lichess and TCEC, not a priority over the NNUEs, small first as
a value-only feasibility test. The vocabulary, token order and heads below still stand; the
"priority track" and the no-padding CPU inference below do not.

**Priority track, as of this session**: the alpha-beta-paired evaluation networks (naive/HalfKP/
HalfKAv2_hm, all `smithy`-scoped, see `rotor-halfkp.md` and siblings) have not been
promising in practice. This transformer is promising enough by comparison that it's now the one
to actually build -- first target is simply proving the value head converges, speed entirely
aside, ahead of any NNUE-track work. **Identity-free only, settled**: pieces are tracked purely by
(type, current square), never by which starting piece they descended from -- the identity-based
alternative (vocabulary and policy both) was dropped as unneeded complexity, not because it was
unworkable.

**Overall network shape now fully settled**: one shared BERT-like transformer encoder (learned
positional embeddings, full attention for now, piece-centric vocabulary below, built directly off
`Board`'s bitboards/mailbox with no tokenizer) feeding two heads -- a value head (an MLP on
`[CLS]`'s final hidden state, ending in a sigmoid) and a policy head (per-piece-type MLPs applied
to each token's own final embedding, king and pawn output widths included). The policy head's
computation mechanism was the previously blocking open question (LC0's own dense square-embedding
approach doesn't fit Rotor's piece-only vocabulary); it's resolved by routing each token through an
MLP specific to its piece type, each MLP emitting raw logits (no local softmax) sized to that piece
type's own move-offset count, with every present piece's logits concatenated, masked to the
position's legal moves, and normalized by one joint softmax -- the same masked-then-normalize
pattern AlphaZero and LC0 both use, just applied over piece-token logits instead of a flat
square-indexed vector.

**Near-term goal: value-head convergence only, regardless of speed.** The immediate priority is
confirming the value head actually trains -- nothing about inference or training speed matters yet,
only whether it converges -- using the same data-generation pipeline already running for the other
`smithy` networks (depth-`9` search over the hand-crafted evaluation function, AB(λ)
labeling) -- a necessary precondition before anything else here. The policy head is fully
specified structurally but its own training (which needs move-probability-style targets, not a
scalar label) is future work, not attempted yet. Every hyperparameter (embedding dimension,
layer/head counts, feed-forward width, dropout, value-head MLP shape, loss function) is left open,
to be pinned down at implementation time rather than now.

## Shared transformer encoder

**Piece-centric only**, settled directly by the developer -- a square-centric variant (`L = 64`,
one token per square) was considered and dropped in favor of this one.

- `L` = the actual number of pieces currently on the board (both colors, kings included), not a
  fixed `32` -- inference never runs in batches (one position at a time, matching this project's
  existing CPU-inference convention elsewhere), so there's no fixed-shape requirement forcing
  padding to a common length the way batched inference would. `32` is only the opening-position
  upper bound, not the real sequence length.
- Direct, real consequence: attention cost (quadratic in `L`) shrinks as material is traded off,
  so the network gets cheaper specifically as the game moves into the endgame -- exactly where
  more search depth/simulations are wanted, not a coincidence to work around.
- **Attention: both full and kings-only, as two architectures sharing one implementation.** Full
  attention (every piece attends to every other, `L * (L - 1)`, maxing at `32 * 31 = 992` in the
  opening position) and kings-only attention (each king attends to every other piece, `2 * (L - 1)`,
  maxing at `2 * 31 = 62`) are both supported from the start, not sequenced as "try one, then
  maybe the other." The two share the same multi-head-attention code; what differs is only the
  attention mask passed into it -- full attention's mask allows every pair, kings-only's mask
  zeroes out every pair that doesn't include a king row or column -- the same additive-masking
  mechanism BERT already uses for padding, just built from token type instead of padding position.
  No separate model class or forked implementation for the second variant.
- **Token order, settled: own/opponent-relative, not absolute color, plus a full total order
  within each side.** Three rules, applied in priority order: (1) own side (side to move) before
  opponent side; (2) within a side, piece type ordered `K < Q < R < B < N < P`; (3) within a side
  and piece type, current square ordered `a1 < a2 < ... < a8 < b1 < ... < h8`. Any two distinct
  pieces differ in at least one of these (two own pieces of the same type can never share a
  square), so the three rules together fully determine the sequence -- no tie-breaking left
  unspecified. Reverses the brief absolute-color revision above -- settled specifically because
  the network is a plain transformer: it only ever consumes the token sequence itself, no
  side-channel input exists for an auxiliary signal like side to move. Own/opponent ordering gives
  side to move for free (it's just which pieces are listed first), at zero extra tokens and zero
  extra attention cost -- unlike either an extra input (not representable in a plain transformer)
  or an extra token (costs attention, unaffordable). This also means the token *vocabulary* has to
  be own/opponent-relative too, not absolute-color (`K`/`k`), to stay consistent with the order --
  see Vocabulary below.
- **Opponent king's position: resolved without an extra input, without an extra token, and
  without scanning.** No `[SEP]` token -- rule 2 above (`K < Q < R < B < N < P` within each side)
  already places each side's king first in its own block, so the opponent king is always the very
  first token of the opponent block, immediately after the last own-side token. The code building
  the sequence already knows this index for free (it knows how many own-side tokens it just
  emitted), so kings-only attention reads it off directly -- no dedicated separator token, and no
  runtime scan, needed to find either the own/opponent boundary or the opponent king specifically.
- **Positional encoding: learned, not fixed/sinusoidal.** Standard BERT-style setup -- a learned
  embedding table indexed by sequence position (`0` through the max length, `[CLS]` plus up to
  `32` piece tokens = `33` slots), added to each token's own vocabulary embedding before the first
  transformer layer, rather than a fixed sinusoidal scheme.
- **Input construction: no tokenizer, ever.** The token sequence is built directly from `Board`'s
  own bitboards and mailbox array -- absolute-to-relative coordinate conversion (see Vocabulary
  below) happens as part of that construction, never via a FEN string or any parsing step. First
  implementation rebuilds the sequence from scratch on every inference call; a second
  implementation maintaining it incrementally via make/unmake alongside move-making (the same way
  `Board` already tracks castling rights/en passant incrementally) is real future work, not
  imminent.

Token embedding plus learned positional embedding is now the settled input representation; the
rest of the encoder is standard BERT-like (multi-head self-attention and feed-forward blocks,
LayerNorm, GELU). Exact hyperparameters (embedding dimension, layer/head counts, feed-forward
width, dropout) are left to implementation time.

## Vocabulary

Every distinct token this transformer's input sequence can contain, enumerated one by one -- not
via a formula, matching this project's own policy-head precedent below.

**Every square/file/rank named below is relative to the side to move, never the absolute PGN/FEN
square** -- the same own/opponent-relative framing already settled for token order above, applied
uniformly to every piece's coordinates, not just pawns. Own's forward direction is always
increasing rank regardless of which real color owns the pieces, so e.g. the own king's home square
is always `e1` in this vocabulary whether own is actually White or Black. Encoding true absolute
FEN squares instead would silently break as soon as own is Black. The conversion itself happens
directly off `Board`'s bitboards/mailbox as the token sequence is built -- see "Input
construction" in Shared transformer encoder above; no FEN string is ever involved.

Every piece is tracked only by (type, current square) -- plus castling rights for the king, plus
the en passant star for pawns -- never by which specific starting piece it descended from, matching
every other evaluation network in this project (naive/HalfKP/HalfKAv2_hm all encode type+color+
square only, with no piece provenance). No move-history/provenance tracking is needed anywhere in
this pipeline as a result.

1. **`[CLS]`** -- prepended once, at the very start of the sequence, before the own king token.
   Its own final hidden state is what the value head reads from directly, a single pooled
   representation of the whole position, rather than pooling over every piece token.
2. **King tokens** -- `K<square>-<rights>`: one per (square, castling-rights pair), e.g. `Ka1-00,
   Ka1-01, Ka1-10, Ka1-11, ..., Kh8-11`. `64 * 4 = 256` tokens. No separate own/opponent identity
   (recoverable from sequence position instead -- see Token order above). The trailing 2-bit suffix
   is *that* king's own kingside/queenside castling rights (still available or not) -- folded
   directly into the existing, already-mandatory king token rather than spent on a separate
   dedicated castling-rights token, since a plain transformer can't take a side-channel input and
   an extra token would cost attention. Bit order (kingside-then-queenside, matching FEN's own `KQ`
   convention) is assumed, not yet explicitly confirmed.
3. **Queen tokens** -- `Q<square>`. `64` tokens. Only one queen per side, so no identity
   distinction is needed.
4. **Rook tokens** -- `R<square>`. `64` tokens. No castling-rights suffix needed here (unlike the
   king token) -- whether castling is still legal is already fully carried by the king's own two
   bits; duplicating it on the rook would be redundant.
5. **Bishop tokens** -- `B<square>`. `64` tokens.
6. **Knight tokens** -- `N<square>`, one per current square only -- no king's-side/queen's-side
   split, since knights carry no identity either. `64` tokens.
7. **Pawn tokens** -- `P<square>*?`, current square restricted to ranks 2-7 (relative): a pawn
   reaching the back rank promotes and becomes one of the tokens above instead of staying a pawn
   token. Promotion needs no separate encoding of its own -- a promoted piece is indistinguishable
   from any other ordinary token of its resulting type. The trailing `*` is an en passant flag,
   legal only on a rank-5 square: a pawn can only ever be captured en passant the move immediately
   after its own double step, which always lands it on relative rank 5 (the pawn's own two-square
   move puts it there, and capture must happen before it moves again). `48` base squares (ranks
   2-7) + `8` extra starred variants (rank 5, on top of its already-counted unstarred form) = `56`.

Total: `1 + 256 + 64 + 64 + 64 + 64 + 56 = 569` tokens.

**Pairs with the per-piece-type offset policy head below** (see Policy head) -- this vocabulary
never tracks piece-identity information, so an identity-based policy encoding isn't an option.

## Value head

An MLP applied to `[CLS]`'s own final hidden state, producing the scalar evaluation -- the
standard BERT classification-head pattern (`[CLS]` final state into a dense layer, or small MLP,
into the task output), the same way ViT reads its own class token for image classification: a
single pooled token's final representation, rather than pooling over every piece token. **Ends in
a sigmoid activation**, bounding the output to `(0, 1)`. Hidden width, depth, and loss function are
implementation-time decisions, not fixed here.

## Policy head

MCTS needs a policy function: a probability distribution over legal moves at the current
position, used to bias tree search toward promising moves.

### Constraint: no factored/product-of-marginals encoding

A policy function must assign an independently-shaped probability to every legal move. Any
encoding that represents a move as a combination of several small, separately-normalized groups
(e.g. a file-softmax times a rank-softmax for a destination square, or a direction-softmax times
an offset-softmax times a promotion-softmax for a pawn move) implicitly forces the move's
probability to *factor* as a product of independent choices -- a real, generally-false modeling
assumption for a policy, not just an inefficiency. AlphaZero's own real move encoding avoids this
for the same reason: one dedicated output logit per distinct move type, never a jointly-decoded
combination of smaller distributions.

### Mechanism, settled: per-piece-type MLP on each token, not a flat move vocabulary

Resolves the previously blocking mechanism question: LC0's own approach (a source-MLP/target-MLP
dot product between every pair of square embeddings) needs a "target" embedding for every square,
including empty ones, so that a destination embedding always exists -- but Rotor's piece-centric
vocabulary deliberately has no token for empty squares, so that mechanism only ever covers
captures.

Instead: **every piece token's own final embedding is routed through an MLP specific to that
token's piece type**, and each piece type's MLP has its own output width -- one dedicated logit
per offset that piece type can ever move along (satisfying the no-factoring constraint above: an
offset already encodes direction and distance jointly, never decomposed into separate softmaxes).
Geometric offset counts, same reasoning already used for `L`/attention above:

- Queen: `8` directions `* 7` distances `= 56`.
- Rook: `4` directions `* 7` distances `= 28`.
- Bishop: `4` (diagonal) directions `* 7` distances `= 28`.
- Knight: `8` (the fixed set of L-shaped offsets).
- King: `10` -- `8` step directions (one square each way) plus `2` dedicated castling slots
  (kingside, queenside), folded into the same MLP's output the same way promotion gets folded into
  the pawn's own output below, rather than a separate mechanism.
- Pawn: `16`, enumerated directly (no formula, same convention as the Vocabulary section):
  1. Push (no promotion)
  2. Push, promote to Knight
  3. Push, promote to Bishop
  4. Push, promote to Rook
  5. Push, promote to Queen
  6. Double push (two steps, from the starting rank)
  7. Capture-left (no promotion) -- also covers en passant when legal: it's the same offset as an
     ordinary diagonal capture, with legality (and the choice between an ordinary and an en
     passant capture) resolved entirely at the masking step, not by a dedicated slot.
  8. Capture-left, promote to Knight
  9. Capture-left, promote to Bishop
  10. Capture-left, promote to Rook
  11. Capture-left, promote to Queen
  12. Capture-right (no promotion) -- same en passant handling as capture-left.
  13. Capture-right, promote to Knight
  14. Capture-right, promote to Bishop
  15. Capture-right, promote to Rook
  16. Capture-right, promote to Queen

An earlier, simpler version used one shared MLP for every token regardless of piece type, with a
uniform `64` outputs (one per destination square) -- considered and superseded by the
piece-type-routed version above, since routing by piece type keeps each MLP's output width matched
to that piece type's own real move count (all under `64`) instead of wasting capacity on
structurally-impossible destinations.

### Combining per-piece logits into a move distribution

Each piece-type MLP emits **raw logits only, no local softmax**. At inference, the logits from
every piece token currently on the board are concatenated into one vector, entries that aren't
legal moves in the current position are dropped, and a single softmax is applied over what's left
-- the same masked-then-normalize pattern AlphaZero and LC0 both use (raw logits over an
over-provisioned vocabulary, legality enforced entirely at the masking step, one joint softmax),
just applied over piece-token logits here instead of a flat square-indexed vector.

## Not yet settled

- Every hyperparameter: the shared encoder's embedding dimension, layer/head counts, feed-forward
  width, dropout; the value head's MLP shape (hidden width, depth) and loss function. All deferred
  to implementation time, not decided now.
- The policy head's own training: it needs move-probability-style targets, not the scalar AB(λ)
  label the value head uses, and no such data-generation plan exists yet. Not attempted until the
  value head's own convergence check succeeds.
- Incremental (make/unmake) maintenance of the network's input sequence, alongside move-making --
  real future optimization, not designed, not imminent.
- Relationship to the rest of the learned-evaluation work: this is a distinct, MCTS-paired track
  from the alpha-beta-paired `smithy` networks (`rotor-halfkp.md`/`rotor-halfkav2hm.md`/
  `rotor-naive.md`), reusing their existing AB(9)/λ data-generation pipeline for its own
  value-head convergence check -- not otherwise sequenced against them yet.
