# Evaluation weight tuning

**Superseded, see evolutionary-training.md.** Decided with the developer: there is neither the time
nor the resources to develop a strong hand-crafted evaluation. The hand-crafted evaluation stays
material plus piece-square tables only -- weak but sound -- and serves only as the loop's first
generator, a reference opponent and a fallback. Playing strength comes from the trained network
instead, whose first epoch already learns from AB(lambda) labels over game trajectories, with a far
more flexible model than these linear weights. The untuned terms (mobility, pawn structure, king
safety) stay in the code, unused: adding them without tuning would double-count overlapping
features and make the evaluation unsound.

## Status

Not started. The original blocking condition (search-technique gap: null-move pruning, LMR,
futility pruning, extensions) is closed. Sequenced behind the strength-measurement framework
(`TODO.md`) instead now.

## Goal

Tune the existing hand-crafted linear eval's weights (piece values, PST cells, mobility tables,
pawn-structure/king-safety constants) jointly -- every term stays structurally as it is, only the
numeric values change.

## Approach: AB(lambda)

Derived from TDLeaf(lambda) (Baxter/Tridgell/Weaver, 1999, KnightCap), with one deliberate
difference: TDLeaf(lambda) is online (its own `v_{t+1}` at every step comes from the
currently-training weights, so label generation and weight updates are inseparable). AB(lambda)
instead gets every leaf value from a fixed teacher -- Rotor's own search, at a fixed depth,
through the existing, non-learning eval -- so every position's label is a pure function of its
game trajectory, computable once, offline, fully in parallel. Same backward-recursion mechanism
`training.md` already uses for the learned-network track; this document applies it to the
hand-crafted linear eval's weights instead. Doesn't require self-play -- any real games work as
the walk through position-space; Rotor's own search supplies every actual target value regardless
of whose game it came from.

**Rejected alternatives**: a genetic algorithm/SPSA against real-game fitness (infeasible on one
machine -- needs on the order of 1M games); labeling positions with another engine's search score
(against this project's own rule 1 and its ethos, and logically circular); plain Texel tuning
against the final game result alone (too noisy per position).

## Gradient computation

The eval is (mostly) linear in its own weights, so each weight's derivative is just its own
feature count (e.g. `d(score)/d(knight_value)` = number of knights on the board) -- no autodiff
framework needed for this math.

## Architecture: dual implementation, verified equivalent by testing

- **Shipped, search-time eval**: plain Rust arithmetic over bitboards/lookup tables, called
  millions of times/sec -- never Torch.
- **Offline training**: runs rarely, not during play, so Torch is a legitimate option (mature,
  independently trusted autodiff) for computing updated weights.
- Learned weights export into the same hand-written Rust implementation used for play.
  Equivalence between the two is verified by testing (many real positions, checking outputs
  match), not trusted by construction -- the same pattern Stockfish's own NNUE pipeline
  (`nnue-pytorch`) uses. Unlike NNUE, Rotor's linear eval isn't expected to need quantization, so
  the equivalence bar can be exact/near-exact rather than NNUE's own looser int8/16 tolerance.
- **Gradient checking**: any hand-derived gradient (if one is ever written instead of using
  Torch's autodiff) is verified against a numerical estimate from central finite differences,
  `(loss(w+e) - loss(w-e)) / (2e)`, normalized difference <= 1e-7.

## Not yet settled

- Where training-game trajectories come from in practice (a public master database vs. Rotor's
  own self-play once it's strong enough) -- not urgent, AB(lambda) itself doesn't care.
- The dual-implementation's exact shape (a second eval implementation directly in Python/Torch vs.
  some shared position-feature extraction reused by both sides).
