use super::castling_rights::CastlingRights;
use super::color::Color;
use super::piece::Piece;
use super::square::Square;
use super::Board;

/// splitmix64: a small, fast, well-distributed pseudorandom generator, used here purely to
/// fill fixed tables of random-looking `u64`s -- nothing here needs to be cryptographically
/// secure, just well-spread and reproducible. `const fn` so every table below is a genuine
/// Rust `const`, computed once at compile time with no runtime initialization step at all.
const fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Fills a `[u64; N]` table by drawing `N` values in sequence from a splitmix64 stream
/// started at `seed`. Every table below uses its own distinct fixed seed (arbitrary, but
/// fixed) rather than sharing one running stream across tables -- simpler than threading one
/// state variable through several differently-shaped `const` initializers, at a collision
/// risk that's the same non-issue either way (drawing under a thousand `u64`s total, total,
/// out of a `2^64`-sized space).
const fn fill<const N: usize>(seed: u64) -> [u64; N] {
    let mut state = seed;
    let mut table = [0u64; N];
    let mut i = 0;
    while i < N {
        table[i] = splitmix64(&mut state);
        i += 1;
    }
    table
}

/// One random value per `(Piece, Square)` pair. `Piece`'s own `color * 6 + piece_type`
/// numbering and `Square`'s own LERF numbering are both already plain `0..N` indices, so no
/// translation is needed to use them directly as array indices here.
const PIECE_SQUARE: [[u64; 64]; 12] = {
    let mut state = 0x2545_F491_4F6C_DD1D;
    let mut table = [[0u64; 64]; 12];
    let mut piece = 0;
    while piece < 12 {
        table[piece] = fill(splitmix64(&mut state));
        piece += 1;
    }
    table
};

/// One random value per individual castling right, indexed the same way
/// `CastlingRights`' own bits are ordered: White kingside, White queenside, Black kingside,
/// Black queenside. Kept as 4 independent values (not one value per each of the 16 combined
/// states) so that losing one right -- the common case, a king or rook moving -- only ever
/// needs XORing out that one value, matching how `update_castling_rights` already clears
/// rights one at a time rather than by comparing whole before/after snapshots.
const CASTLING: [u64; 4] = fill(0x9E37_79B9_7F4A_7C16);

/// One random value per square, XORed in whenever `en_passant_square` is `Some` --
/// unconditionally, regardless of whether an en passant capture is actually legal for the
/// side to move right now. Checking legality (an enemy pawn actually positioned to capture,
/// and not pinned) would be real extra work for a case that's vanishingly rare in practice,
/// and being imprecise here can only ever cause a missed repetition, never a false one -- a
/// safe direction to be wrong in, if it's wrong at all.
const EN_PASSANT: [u64; 64] = fill(0x9E37_79B9_7F4A_7C17);

/// One random value, XORed in whenever it's Black's turn to move -- in effect, toggled once
/// every single move, since side to move flips every ply.
const SIDE_TO_MOVE: u64 = {
    let mut state = 0x9E37_79B9_7F4A_7C18;
    splitmix64(&mut state)
};

/// This `(piece, square)` pair's own contribution to the hash -- XOR this in when `piece`
/// is placed on `square`, XOR the same value out again when it's removed (XOR is its own
/// inverse, so "undo" is exactly "do it again").
pub(crate) fn piece_square(piece: Piece, square: Square) -> u64 {
    PIECE_SQUARE[piece as usize][square as usize]
}

/// `rights`' whole contribution to the hash -- the XOR of whichever of the 4 individual
/// per-right values are currently set. Used as "XOR the old contribution out, XOR the new
/// one in" around a castling-rights change, rather than diffing which specific bit flipped:
/// simpler to write and verify, and no less correct, since a bit that didn't change gets
/// XORed out and immediately back in (a no-op) either way.
pub(crate) fn castling(rights: CastlingRights) -> u64 {
    let mut hash = 0;
    if rights.kingside(Color::White) {
        hash ^= CASTLING[0];
    }
    if rights.queenside(Color::White) {
        hash ^= CASTLING[1];
    }
    if rights.kingside(Color::Black) {
        hash ^= CASTLING[2];
    }
    if rights.queenside(Color::Black) {
        hash ^= CASTLING[3];
    }
    hash
}

/// `square`'s contribution to the hash: its own random value if there's an en passant
/// square at all, `0` (XOR's own identity element) if there isn't.
pub(crate) fn en_passant(square: Option<Square>) -> u64 {
    match square {
        Some(square) => EN_PASSANT[square as usize],
        None => 0,
    }
}

/// The side-to-move contribution to the hash -- one fixed value, toggled every move.
pub(crate) fn side_to_move() -> u64 {
    SIDE_TO_MOVE
}

/// Which of `Board`'s correction keys `piece` feeds: `0`, the pawns of both sides; `1`, White's
/// other pieces, its king included; `2`, Black's. Stockfish's `pawnKey` and `nonPawnKey[c]`, the
/// keys its correction histories are indexed by (`position.cpp`, master, 2026-10-31), the kings in
/// the non-pawn keys as there (the developer, 2026-10-31).
pub(crate) fn correction_key(piece: Piece) -> usize {
    match piece.piece_type() {
        super::piece_type::PieceType::Pawn => 0,
        _ => 1 + piece.color() as usize,
    }
}

/// The three correction keys (`correction_key`) built from scratch, as `compute` builds the hash:
/// each the XOR of its pieces' `PIECE_SQUARE` values.
pub(crate) fn compute_correction_keys(board: &Board) -> [u64; 3] {
    let mut keys = [0; 3];
    for (square_index, piece) in board.mailbox.iter().enumerate() {
        if let Some(piece) = piece {
            keys[correction_key(*piece)] ^= PIECE_SQUARE[*piece as usize][square_index];
        }
    }
    keys
}

/// Builds a hash from scratch by scanning `board`'s own current state -- the ground truth
/// `Board`'s own incrementally-maintained `hash` field is checked against in tests, and what
/// sets that field's initial value wherever a `Board` is first constructed. Never called on
/// every move in real search -- that's exactly the cost incremental maintenance exists to
/// avoid -- only at construction time and to verify incremental maintenance stays correct.
pub(crate) fn compute(board: &Board) -> u64 {
    let mut hash = 0;
    for (square_index, piece) in board.mailbox.iter().enumerate() {
        if let Some(piece) = piece {
            hash ^= PIECE_SQUARE[*piece as usize][square_index];
        }
    }
    hash ^= castling(board.castling_rights);
    hash ^= en_passant(board.en_passant_square);
    if board.side_to_move == Color::Black {
        hash ^= side_to_move();
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;

    #[test]
    fn the_same_fen_always_hashes_identically() {
        let (a, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K2R w K - 0 1").unwrap();
        let (b, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K2R w K - 0 1").unwrap();
        assert_eq!(compute(&a), compute(&b));
    }

    #[test]
    fn side_to_move_alone_changes_the_hash() {
        let (white_to_move, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let (black_to_move, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K3 b - - 0 1").unwrap();
        assert_ne!(compute(&white_to_move), compute(&black_to_move));
    }

    #[test]
    fn a_single_castling_right_alone_changes_the_hash() {
        let (with_right, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K2R w K - 0 1").unwrap();
        let (without_right, _) = Board::from_fen("4k3/8/8/8/8/8/8/4K2R w - - 0 1").unwrap();
        assert_ne!(compute(&with_right), compute(&without_right));
    }

    #[test]
    fn the_en_passant_square_alone_changes_the_hash() {
        let (with_ep, _) = Board::from_fen("8/8/8/3pP3/8/8/8/4K2k w - d6 0 1").unwrap();
        let (without_ep, _) = Board::from_fen("8/8/8/3pP3/8/8/8/4K2k w - - 0 1").unwrap();
        assert_ne!(compute(&with_ep), compute(&without_ep));
    }
}
