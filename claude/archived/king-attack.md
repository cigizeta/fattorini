# Step 4c of the textbook evaluation: king attack

**Archived:** implemented as a big five term, deleted when the big five was retired 2026-10-03. The hand-crafted evaluation is PeSTO only; see textbook-evaluation.md.

Status: done. Applied; tests pass. Part of textbook-evaluation.md (step 4 of 4, the last term). Branch `alessio`
only.

## Goal

Add the king attack to `eval.rs` and complete king safety: shield + storm - king attack. No search
changes.

## The rule

- `KING_ZONE: [Bitboard; 64]` by king square, not by color: the 3x3 block around the king's
  square with its file clamped to b..g and its rank to 2..7 (Stockfish 11's `kingRing`), always 9
  squares. Built as `king_attacks(center) | center`, in a `LazyLock`.
- For each enemy knight, bishop, rook and queen, `n` = the zone squares among its mobility
  squares: `attacks & !own & KING_ZONE[king]`, CPW's rule (a square held by the attacker's own
  piece does not count), full occupancy, no x-rays. Defenders are ignored. No gate.
- attack units = the sum of `ATTACK_<PIECE>[n]`.
- king attack = min(units² / 4096, 500). Both constants fixed, not tuned: scaling every table by k
  and 4096 by k² changes nothing, so 4096 is only the resolution; 500 caps a pile-up.
- king safety = pawn cover (shield + storm, with the castling max) - king attack, middlegame only.
  The old `king_safety` is renamed `pawn_cover`.

## Tables, in `tuned.rs`

Sized to each piece's largest possible count in a 3x3 block: knight 3 (0..=2), bishop 5, rook 5,
queen 9 (0..=8). 22 entries; entry 0 is pinned at 0 (a piece attacking nothing adds nothing), so 18
are tunable. Starting values `round(PeSTO_mg * n / 10)`, a linear model with PeSTO's proportions:

- knight: 0, 34, 67
- bishop: 0, 37, 73, 110, 146
- rook: 0, 48, 95, 143, 191
- queen: 0, 103, 205, 308, 410, 513, 615, 718, 820

## Tests

- `KING_ZONE`: g1 and h1 are f1..h3; e4 is d3..f5; a8 is a6..c8.
- The maxima: for every zone and every square, a lone piece on an otherwise empty board never
  attacks more zone squares than its table's last index.
- The tables equal the formula entry by entry, and entry 0 is 0.
- A queen on h5 against a king on g8 (zone f6..h8) reaches h6, h7, h8, g6, f7: 5 squares, 513
  units, 64 cp. With a White pawn on g6 it reaches h6, h7, h8 only: 308 units.
- The curve: 1431 units give 499, 1432 give the cap 500, 3000 stay at 500.
- Existing tests unchanged: in the king-and-knight test the knight reaches one zone square, 34
  units, 0 cp.

## Verification

- `cargo test`, and `cargo test --features training`.
- No new warnings.
