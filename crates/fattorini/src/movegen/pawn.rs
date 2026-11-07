use crate::board::bitboard::Bitboard;
use crate::board::chess_move::Move;
use crate::board::color::Color;
use crate::board::piece::Piece;
use crate::board::piece_type::PieceType;
use crate::board::rank::Rank;
use crate::board::square::Square;
use crate::board::Board;

/// The four piece types a pawn may promote into -- never `Pawn` (can't promote into
/// itself) or `King` (chess rule: promotion is always to a knight, bishop, rook, or
/// queen).
const PROMOTION_PIECES: [PieceType; 4] =
    [PieceType::Knight, PieceType::Bishop, PieceType::Rook, PieceType::Queen];

/// The rank delta a pawn of `color` moves toward -- White moves toward higher ranks,
/// Black toward lower ranks. Pawn-specific, not a general property of `Color` itself (no
/// other piece's movement depends on a "forward" direction).
fn forward(color: Color) -> i8 {
    match color {
        Color::White => 1,
        Color::Black => -1,
    }
}

/// `color`'s pawn starting rank -- a double push is only allowed from here.
fn starting_rank(color: Color) -> Rank {
    match color {
        Color::White => Rank::R2,
        Color::Black => Rank::R7,
    }
}

/// The rank a pawn of `color` promotes on, reaching the far end of the board.
fn promotion_rank(color: Color) -> Rank {
    match color {
        Color::White => Rank::R8,
        Color::Black => Rank::R1,
    }
}

/// The squares a `color` pawn on `square` attacks -- the two diagonal squares in front of
/// it, regardless of whether anything is actually there. Deliberately independent of any
/// `Board`: unlike a pawn's *move* (forward, and only onto an empty square), its *attack*
/// pattern is pure geometry, same as `knight_attacks`/`king_attacks`. Needed for attacked-
/// square detection (`is_square_attacked`), which asks "could a pawn here reach that
/// square" without caring what's on `square` itself.
///
/// A table lookup, as `knight_attacks`: SEE and attack detection ask it at every node, and computed
/// by `try_offset` each time it showed in the labelling profile (2026-10-12).
pub fn pawn_attacks(square: Square, color: Color) -> Bitboard {
    PAWN_ATTACKS[color as usize][square as usize]
}

/// `pawn_attacks` for every square, White's then Black's, computed at compile time
/// (`Bitboard::step_attack_table`): the two diagonal squares one rank forward.
static PAWN_ATTACKS: [[Bitboard; 64]; 2] =
    [Bitboard::step_attack_table(&[(-1, 1), (1, 1)]), Bitboard::step_attack_table(&[(-1, -1), (1, -1)])];

/// Pseudo-legal pawn moves for `color`'s pawns on `board`, appended onto the caller's
/// shared `moves` buffer rather than returned in one of its own -- avoids a fresh heap
/// allocation on every call, which matters once this runs at every node of a perft or
/// search tree. Doesn't check whether a move leaves `color`'s own king in check (see the
/// design notes on the two-phase generation split). Covers single push, double push,
/// diagonal capture, en passant capture, and promotion (one `Move` per promotion piece,
/// on any push or capture reaching the last rank).
pub fn pawn_moves(board: &Board, color: Color, moves: &mut Vec<Move>) {
    pawn_noisy_moves(board, color, moves);
    pawn_quiet_moves(board, color, moves);
}

const FILE_A: Bitboard = Bitboard::from_bits(0x0101_0101_0101_0101);
const FILE_H: Bitboard = Bitboard::from_bits(0x8080_8080_8080_8080);

/// The rank `color`'s pawns promote on, as a mask.
fn last_rank_mask(color: Color) -> Bitboard {
    match color {
        Color::White => Bitboard::from_bits(0xFF << 56),
        Color::Black => Bitboard::from_bits(0xFF),
    }
}

/// The rank a single push from `color`'s starting rank lands on: a pawn there after a single
/// push may push again.
fn double_push_mask(color: Color) -> Bitboard {
    match color {
        Color::White => Bitboard::from_bits(0xFF << 16),
        Color::Black => Bitboard::from_bits(0xFF << 40),
    }
}

/// Every square of `bitboard` moved by `delta` square indices: 8 is one rank up, -8 one rank
/// down, 7 and 9 (or -9 and -7) the diagonals. A diagonal step must first drop the file it
/// would wrap from.
fn shifted(bitboard: Bitboard, delta: i8) -> Bitboard {
    if delta > 0 { bitboard << delta as u32 } else { bitboard >> (-delta) as u32 }
}

/// The two capture steps of `color`'s pawns, toward file a then toward file h: the file a pawn
/// must not stand on for the step, and the step's `delta`.
fn capture_steps(color: Color) -> [(Bitboard, i8); 2] {
    match color {
        Color::White => [(FILE_A, 7), (FILE_H, 9)],
        Color::Black => [(FILE_A, -9), (FILE_H, -7)],
    }
}

/// One move onto every square of `targets` from the square `delta` behind it, each expanded into
/// the four promotions when `promotes`.
fn add_targets(moves: &mut Vec<Move>, targets: Bitboard, delta: i8, promotes: bool) {
    for to in targets.squares() {
        let from = Square::try_from((to as i8 - delta) as u8).unwrap();
        if promotes {
            for promotion in PROMOTION_PIECES {
                moves.push(Move::new(from, to, Some(promotion)));
            }
        } else {
            moves.push(Move::new(from, to, None));
        }
    }
}

/// The noisy part of `pawn_moves` only: every capture (en passant included, each one reaching the
/// last rank expanded into its four promotions) and the single push onto the last rank (its four
/// promotions). No other push, and never a double push, which cannot reach the last rank.
///
/// All pawns at once by shifts of the pawn bitboard, as Stockfish, not one pawn at a time: pawn
/// generation was 5.7% of a labelling search done square by square (2026-10-12).
pub fn pawn_noisy_moves(board: &Board, color: Color, moves: &mut Vec<Move>) {
    let pawns = board.bitboard(Piece::new(color, PieceType::Pawn));
    let empty = !board.all_occupancy();
    let enemies = board.occupancy(color.opposite());
    let last_rank = last_rank_mask(color);
    let push = 8 * forward(color);

    add_targets(moves, shifted(pawns, push) & empty & last_rank, push, true);
    for (wrapping_file, delta) in capture_steps(color) {
        let targets = shifted(pawns & !wrapping_file, delta) & enemies;
        add_targets(moves, targets & last_rank, delta, true);
        add_targets(moves, targets & !last_rank, delta, false);
    }
    if let Some(target) = board.en_passant_square() {
        // The pawns that attack the en passant square are those a pawn of the other colour on it
        // would attack.
        for from in (pawn_attacks(target, color.opposite()) & pawns).squares() {
            moves.push(Move::en_passant_capture(from, target));
        }
    }
}

/// The quiet part of `pawn_moves` only: every single and double push that does not reach the last
/// rank. With `pawn_noisy_moves`, exactly `pawn_moves`. By shifts, as `pawn_noisy_moves`.
pub fn pawn_quiet_moves(board: &Board, color: Color, moves: &mut Vec<Move>) {
    let pawns = board.bitboard(Piece::new(color, PieceType::Pawn));
    let empty = !board.all_occupancy();
    let push = 8 * forward(color);

    let single = shifted(pawns, push) & empty;
    add_targets(moves, single & !last_rank_mask(color), push, false);
    let double = shifted(single & double_push_mask(color), push) & empty;
    for to in double.squares() {
        let from = Square::try_from((to as i8 - 2 * push) as u8).unwrap();
        moves.push(Move::double_pawn_push(from, to));
    }
}

/// Whether `mv`, from a square holding one of `color`'s pawns, is a move `pawn_moves` would
/// generate on `board`: checked directly, without generating, for a TT move or a killer
/// (`movegen::is_pseudo_legal`).
pub fn is_pseudo_legal_pawn_move(board: &Board, color: Color, mv: Move) -> bool {
    use crate::board::chess_move::MoveKind;
    let (from, to) = (mv.from(), mv.to());
    let empty = !board.all_occupancy();
    let is_empty = |square: Square| (empty & Bitboard::from_square(square)) != Bitboard::EMPTY;
    let promotes_correctly = (to.rank() == promotion_rank(color)) == mv.promotion().is_some();
    let single_target = from.try_offset(0, forward(color));
    match mv.kind() {
        MoveKind::Quiet => {
            let push = single_target == Some(to) && is_empty(to);
            let capture = (pawn_attacks(from, color) & board.occupancy(color.opposite()) & Bitboard::from_square(to))
                != Bitboard::EMPTY;
            (push || capture) && promotes_correctly
        }
        MoveKind::DoublePawnPush => {
            from.rank() == starting_rank(color)
                && single_target.is_some_and(is_empty)
                && from.try_offset(0, 2 * forward(color)) == Some(to)
                && is_empty(to)
        }
        MoveKind::EnPassantCapture => {
            board.en_passant_square() == Some(to) && (pawn_attacks(from, color) & Bitboard::from_square(to)) != Bitboard::EMPTY
        }
        MoveKind::Castle => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::chess_move::MoveKind;

    #[test]
    fn the_compile_time_attack_table_matches_try_offset_on_every_square() {
        for color in [Color::White, Color::Black] {
            for square_value in 0u8..64 {
                let square = Square::try_from(square_value).unwrap();
                let mut expected = Bitboard::EMPTY;
                for file_offset in [-1i8, 1i8] {
                    if let Some(target) = square.try_offset(file_offset, forward(color)) {
                        expected = expected | Bitboard::from_square(target);
                    }
                }
                assert_eq!(pawn_attacks(square, color), expected, "{color:?} {square:?}");
            }
        }
    }

    #[test]
    fn pawn_attacks_are_the_two_diagonals_ahead() {
        let attacks = pawn_attacks(Square::E4, Color::White);
        let expected = Bitboard::from_square(Square::D5) | Bitboard::from_square(Square::F5);
        assert_eq!(attacks, expected);

        let attacks = pawn_attacks(Square::E4, Color::Black);
        let expected = Bitboard::from_square(Square::D3) | Bitboard::from_square(Square::F3);
        assert_eq!(attacks, expected);
    }

    #[test]
    fn pawn_attacks_do_not_wrap_at_the_board_edge() {
        let attacks = pawn_attacks(Square::A4, Color::White);
        assert_eq!(attacks, Bitboard::from_square(Square::B5));
    }

    #[test]
    fn single_push_when_not_on_starting_rank() {
        let (board, _) = Board::from_fen("8/8/8/8/4P3/8/8/8 w - - 0 1").unwrap();
        let mut moves = Vec::new();
        pawn_moves(&board, Color::White, &mut moves);
        assert_eq!(moves.len(), 1);
        assert_eq!(moves[0].from(), Square::E4);
        assert_eq!(moves[0].to(), Square::E5);
        assert_eq!(moves[0].kind(), MoveKind::Quiet);
        assert_eq!(moves[0].promotion(), None);
    }

    #[test]
    fn double_push_available_from_starting_rank() {
        let (board, _) = Board::from_fen("8/8/8/8/8/8/4P3/8 w - - 0 1").unwrap();
        let mut moves = Vec::new();
        pawn_moves(&board, Color::White, &mut moves);
        assert_eq!(moves.len(), 2);

        let single = moves.iter().find(|mv| mv.to() == Square::E3).unwrap();
        assert_eq!(single.kind(), MoveKind::Quiet);

        let double = moves.iter().find(|mv| mv.to() == Square::E4).unwrap();
        assert_eq!(double.kind(), MoveKind::DoublePawnPush);
    }

    #[test]
    fn push_blocked_directly_ahead_prevents_single_and_double_push() {
        let (board, _) = Board::from_fen("8/8/8/8/8/4p3/4P3/8 w - - 0 1").unwrap();
        let mut moves = Vec::new();
        pawn_moves(&board, Color::White, &mut moves);
        assert!(moves.is_empty());
    }

    #[test]
    fn double_push_blocked_two_squares_ahead_still_allows_single_push() {
        let (board, _) = Board::from_fen("8/8/8/8/4p3/8/4P3/8 w - - 0 1").unwrap();
        let mut moves = Vec::new();
        pawn_moves(&board, Color::White, &mut moves);
        assert_eq!(moves.len(), 1);
        assert_eq!(moves[0].to(), Square::E3);
        assert_eq!(moves[0].kind(), MoveKind::Quiet);
    }

    #[test]
    fn diagonal_captures_onto_enemy_pieces() {
        // White pawn on E4, Black pawns on D5 and F5 (both capturable), E5 empty.
        let (board, _) = Board::from_fen("8/8/8/3p1p2/4P3/8/8/8 w - - 0 1").unwrap();
        let mut moves = Vec::new();
        pawn_moves(&board, Color::White, &mut moves);

        let mut destinations: Vec<Square> = moves.iter().map(|mv| mv.to()).collect();
        let mut expected = vec![Square::D5, Square::E5, Square::F5];
        destinations.sort_by_key(|square| *square as u8);
        expected.sort_by_key(|square| *square as u8);
        assert_eq!(destinations, expected);

        for mv in &moves {
            assert_eq!(mv.kind(), MoveKind::Quiet);
            assert_eq!(mv.promotion(), None);
        }
    }

    #[test]
    fn en_passant_capture_is_generated() {
        // White pawn E5, Black pawn just double-pushed to D5 -- en passant target d6.
        let (board, _) = Board::from_fen("8/8/8/3pP3/8/8/8/8 w - d6 0 1").unwrap();
        let mut moves = Vec::new();
        pawn_moves(&board, Color::White, &mut moves);
        assert_eq!(moves.len(), 2);

        let push = moves.iter().find(|mv| mv.to() == Square::E6).unwrap();
        assert_eq!(push.kind(), MoveKind::Quiet);

        let en_passant = moves.iter().find(|mv| mv.to() == Square::D6).unwrap();
        assert_eq!(en_passant.kind(), MoveKind::EnPassantCapture);
        assert_eq!(en_passant.from(), Square::E5);
    }

    #[test]
    fn promotion_on_push_and_capture_generates_four_moves_each() {
        // White pawn E7, Black rook on D8 -- push to E8 and capture to D8 both promote;
        // F8 is empty and not en passant, so no move there.
        let (board, _) = Board::from_fen("3r4/4P3/8/8/8/8/8/8 w - - 0 1").unwrap();
        let mut moves = Vec::new();
        pawn_moves(&board, Color::White, &mut moves);
        assert_eq!(moves.len(), 8);

        let mut push_promotions: Vec<PieceType> =
            moves.iter().filter(|mv| mv.to() == Square::E8).map(|mv| mv.promotion().unwrap()).collect();
        let mut capture_promotions: Vec<PieceType> =
            moves.iter().filter(|mv| mv.to() == Square::D8).map(|mv| mv.promotion().unwrap()).collect();

        let mut expected = vec![PieceType::Knight, PieceType::Bishop, PieceType::Rook, PieceType::Queen];
        push_promotions.sort_by_key(|piece_type| *piece_type as u8);
        capture_promotions.sort_by_key(|piece_type| *piece_type as u8);
        expected.sort_by_key(|piece_type| *piece_type as u8);
        assert_eq!(push_promotions, expected);
        assert_eq!(capture_promotions, expected);
    }

    #[test]
    fn file_edge_does_not_wrap_around() {
        // White pawn on A4 (not starting rank): only one diagonal (B5) exists at all.
        let (board, _) = Board::from_fen("8/8/8/1p6/P7/8/8/8 w - - 0 1").unwrap();
        let mut moves = Vec::new();
        pawn_moves(&board, Color::White, &mut moves);

        let mut destinations: Vec<Square> = moves.iter().map(|mv| mv.to()).collect();
        let mut expected = vec![Square::A5, Square::B5];
        destinations.sort_by_key(|square| *square as u8);
        expected.sort_by_key(|square| *square as u8);
        assert_eq!(destinations, expected);
    }

    #[test]
    fn captures_by_shift_never_wrap_across_the_board_edge() {
        // White pawn on H4: G5 is a capture, A6 (black) is where a wrapping shift would land; the
        // white pawn on H5 only pushes.
        // Black pawn on A5: B4 is a capture, H3 (a white knight) is where a wrapping shift would land.
        let (board, _) = Board::from_fen("8/8/p7/p5pP/1P5P/7N/8/8 w - - 0 1").unwrap();
        let mut moves = Vec::new();
        pawn_moves(&board, Color::White, &mut moves);
        let mut white: Vec<(Square, Square)> = moves.iter().map(|mv| (mv.from(), mv.to())).collect();
        white.sort_by_key(|(from, to)| (*from as u8, *to as u8));
        assert_eq!(
            white,
            vec![(Square::B4, Square::A5), (Square::B4, Square::B5), (Square::H4, Square::G5), (Square::H5, Square::H6)]
        );

        let mut moves = Vec::new();
        pawn_moves(&board, Color::Black, &mut moves);
        let mut black: Vec<(Square, Square)> = moves.iter().map(|mv| (mv.from(), mv.to())).collect();
        black.sort_by_key(|(from, to)| (*from as u8, *to as u8));
        assert_eq!(
            black,
            vec![(Square::A5, Square::A4), (Square::A5, Square::B4), (Square::G5, Square::G4), (Square::G5, Square::H4)]
        );
    }

    #[test]
    fn black_pawn_moves_toward_lower_ranks() {
        let (board, _) = Board::from_fen("8/4p3/8/8/8/8/8/8 b - - 0 1").unwrap();
        let mut moves = Vec::new();
        pawn_moves(&board, Color::Black, &mut moves);
        assert_eq!(moves.len(), 2);

        let single = moves.iter().find(|mv| mv.to() == Square::E6).unwrap();
        assert_eq!(single.kind(), MoveKind::Quiet);

        let double = moves.iter().find(|mv| mv.to() == Square::E5).unwrap();
        assert_eq!(double.kind(), MoveKind::DoublePawnPush);
    }
}
