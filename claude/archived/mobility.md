# Mobility

**Archived:** hand-crafted mobility left `eval.rs` when the evaluation became PeSTO material and tables only (pesto-evaluation.md); its reuse as a big five input went with the big five (retired 2026-10-03).

## Status

Implemented in `eval.rs` for all four piece types (knight, bishop, rook, queen), wired into
`material_value`, using Rotor's own rescaled constants (below), not Stockfish's originals.

## Design

Scoped to four piece types: knight, bishop, rook, queen. Pawns excluded (sideways/forward
movement isn't the same "scope" concept); king excluded deliberately -- a mobile king in the
middlegame is usually a liability, not an asset (king safety is its own separate term).

**Definition**: attacked squares under current board occupancy, not legal moves -- no
check-legality filtering, no distinction between an empty, friendly-occupied, or enemy-occupied
square at the raw-attack stage.

**Mobility area** (`mobility_area(board, color) -> Bitboard`, the shared exclusion mask):
excludes own king, own queen, every friendly pawn's square (regardless of rank or whether
blocked -- simplified from Stockfish's own blocked-or-low-rank-only rule), and every square
attacked by an enemy pawn. Does **not** exclude pinned-piece squares (`blockers_for_king`) --
Rotor's legality filtering is brute-force make/unmake, no pin/ray primitive to reuse for this;
same deferral as backward pawns. Does **not** x-ray through the queen for bishop/rook attacks --
no piece ever sees through another, a deliberate simplification, not a cost-driven one.

## Constants

Rescaled from classical (pre-NNUE) Stockfish's own `MobilityBonus[4][32]` (`evaluate.cpp`,
commit `804a29c738847b7ea5f8a4bff001964bd234d332`), same methodology as pawn structure: each cell
as a fraction of Stockfish's own pawn value (`136` mg / `208` eg), rescaled onto Rotor's pawn
value (`100`), rounded to the nearest 5 (ties rounded up). A per-count table, not a linear
formula -- a `slope * count` model was considered and rejected: several entries are meaningfully
negative (a boxed-in piece is actively bad, which `count = 0 -> 0` can't express), and even with
an added intercept, two numbers per piece type can't capture the tables' real
diminishing-returns curvature.

**Knight** (9 entries, counts 0-8):

| attacked | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 |
|---|---|---|---|---|---|---|---|---|---|
| mg | -45 | -40 | -10 | -5 | 0 | 10 | 15 | 20 | 25 |
| eg | -40 | -25 | -15 | -10 | 0 | 5 | 10 | 10 | 10 |

**Bishop** (14 entries, counts 0-13):

| attacked | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| mg | -35 | -15 | 10 | 20 | 30 | 40 | 40 | 45 | 45 | 50 | 60 | 60 | 65 | 70 |
| eg | -30 | -10 | 0 | 5 | 10 | 20 | 25 | 25 | 30 | 35 | 40 | 40 | 40 | 45 |

**Rook** (15 entries, counts 0-14):

| attacked | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| mg | -45 | -15 | 0 | 0 | 0 | 10 | 15 | 25 | 30 | 30 | 30 | 35 | 40 | 40 | 45 |
| eg | -35 | -10 | 10 | 20 | 35 | 50 | 50 | 60 | 65 | 65 | 75 | 80 | 80 | 80 | 85 |

**Queen** (28 entries, counts 0-27):

| attacked | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| mg | -20 | -10 | -5 | -5 | 15 | 15 | 15 | 25 | 30 | 40 | 45 | 50 | 50 | 50 |
| eg | -25 | -15 | -5 | 10 | 20 | 25 | 30 | 35 | 40 | 45 | 45 | 50 | 60 | 60 |

| attacked | 14 | 15 | 16 | 17 | 18 | 19 | 20 | 21 | 22 | 23 | 24 | 25 | 26 | 27 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| mg | 50 | 50 | 55 | 55 | 55 | 60 | 70 | 80 | 80 | 80 | 80 | 85 | 85 | 85 |
| eg | 65 | 65 | 65 | 70 | 70 | 70 | 75 | 80 | 80 | 80 | 90 | 90 | 90 | 105 |

Pending AB(lambda) tuning like every other founded constant in this project -- a mechanically
rescaled seed, not independently chess-justified per entry.
