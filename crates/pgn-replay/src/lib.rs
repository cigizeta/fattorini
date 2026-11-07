//! Resolving SAN to Fattorini moves, shared by `smithy` (`chunk`, and the dataset build) and
//! `arena replay`. Lives here rather than in the `fattorini` library crate: `shakmaty`/
//! `pgn-reader` must never link into the shipped `fattorini` engine binary (CLAUDE.md rule 1), and the
//! library crate is exactly what that binary links against. Its own crate, so both binaries depend
//! on one source rather than two hand-copied implementations kept in sync by hand. `nnue_bench`
//! `#[path]`-includes this file instead, since a dependency of `fattorini` on it would be a cycle.

use shakmaty::san::San;
use shakmaty::{CastlingSide as ShakmatyCastlingSide, Role, Square as ShakmatySquare};

use fattorini::board::castling_rights::CastlingSide;
use fattorini::board::chess_move::{Move, MoveKind};
use fattorini::board::piece_type::PieceType;
use fattorini::board::square::Square;
use fattorini::board::Board;
use fattorini::movegen::legal::legal_moves;

/// The standard starting position, where every sampled CCRL and TCEC game starts (`smithy
/// sample` refuses a game with a `[FEN]` header).
pub const STARTING_POSITION_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

/// `shakmaty`'s `Square` uses the identical LERF numbering (`A1 = 0 .. H8 = 63`) Fattorini's own
/// `Square` does -- confirmed directly against both crates' docs, not assumed -- so this is a
/// plain index round trip, never a real coordinate transform.
fn to_square(square: ShakmatySquare) -> Square {
    Square::try_from(square.to_u32() as u8).unwrap()
}

/// `shakmaty::Role` names the same six piece kinds `PieceType` does (`Pawn` .. `King`), just
/// under a different type in a different crate -- this is pure data translation between two
/// notations for the same chess fact, never a legality or move-generation decision.
fn to_piece_type(role: Role) -> PieceType {
    match role {
        Role::Pawn => PieceType::Pawn,
        Role::Knight => PieceType::Knight,
        Role::Bishop => PieceType::Bishop,
        Role::Rook => PieceType::Rook,
        Role::Queen => PieceType::Queen,
        Role::King => PieceType::King,
    }
}

/// Resolves one parsed SAN token into exactly one of Fattorini's own legal moves for `board`, using the
/// SAN purely as notation data (piece kind, destination, disambiguation, promotion) -- never calling
/// any of `shakmaty`'s own move-generation or legality logic, which is exactly the boundary the
/// design settled on: a PGN crate may parse the *text*, but only Fattorini's own board and move
/// generator ever decide what is actually legal.
///
/// `None` when the SAN names no legal move, more than one, or is a null move or a drop: the caller
/// decides whether that throws the game away or is a bug.
pub fn try_resolve_san(board: &Board, san: &San) -> Option<Move> {
    if matches!(san, San::Put { .. } | San::Null) {
        return None;
    }
    let candidates = san_candidates(board, san);
    if candidates.len() != 1 {
        return None;
    }
    Some(candidates[0])
}

/// Every legal move of `board` that `san` could name.
fn san_candidates(board: &Board, san: &San) -> Vec<Move> {
    let legal = legal_moves(board, board.side_to_move());

    let candidates: Vec<Move> = match san {
        San::Normal { role, file, rank, to, promotion, .. } => {
            let to_square_value = to_square(*to);
            let role_value = to_piece_type(*role);
            let promotion_value = promotion.map(to_piece_type);

            legal
                .into_iter()
                .filter(|mv| {
                    mv.to() == to_square_value
                        && board.piece_at(mv.from()).map(|piece| piece.piece_type()) == Some(role_value)
                        && mv.promotion() == promotion_value
                        && file.is_none_or(|f| mv.from().file() as u8 == f as u8)
                        && rank.is_none_or(|r| mv.from().rank() as u8 == r as u8)
                })
                .collect()
        }
        San::Castle(side) => {
            let castling_side = match side {
                ShakmatyCastlingSide::KingSide => CastlingSide::Kingside,
                ShakmatyCastlingSide::QueenSide => CastlingSide::Queenside,
            };
            let king_to = castling_side.squares(board.side_to_move()).king_to;

            legal.into_iter().filter(|mv| mv.kind() == MoveKind::Castle && mv.to() == king_to).collect()
        }
        San::Put { .. } | San::Null => {
            panic!("unexpected SAN variant in a standard (non-Crazyhouse, non-null-move) game: {san:?}");
        }
    };
    candidates
}
