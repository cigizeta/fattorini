//! The keys a `psq_stack` network picks its head's weight sets by: our king's square for the first
//! layer, their king's for the second, the material for the output (or the game phase, for an
//! output bucketed by phase, or by piece count). One definition for the trainer
//! (from a `CanonicalPosition`), the engine (from a `Board`) and `smithy buckets`, so they cannot
//! drift apart.
//!
//! Both kings are seen from the side to move, rank flipped when Black is to move
//! (`features::oriented_square`), so our home rank is rank 1 and theirs rank 8; files are never
//! mirrored, the accumulator not being mirrored either.

use crate::board::color::Color;
use crate::board::piece::Piece;
use crate::board::piece_type::PieceType;
use crate::board::square::Square;
use crate::board::Board;
use crate::features::{oriented_square, unpack_canonical_piece, CanonicalPosition};
use crate::movegen::attacks::king_square;
use crate::pesto::{PHASE_INC, PHASE_MAX};

/// Weight sets of the first layer, one per square of our king.
pub const OUR_KING_BUCKETS: usize = 64;

/// Weight sets of the second layer, one per square of their king.
pub const THEIR_KING_BUCKETS: usize = 64;

/// Weight sets of the output, one per material value 1 to 78.
pub const MATERIAL_BUCKETS: usize = 78;

/// The classical material values, pawn 1, knight and bishop 3, rook 5, queen 9, king 0, indexed by
/// `PieceType`.
pub const MATERIAL_VALUE: [i32; 6] = [1, 3, 3, 5, 9, 0];

/// Both sides' material at the start, `2 * (8 + 6 + 6 + 10 + 9)`, and the cap after a promotion.
pub const MATERIAL_MAX: i32 = 78;

/// The output's bucket for `material` (both sides, kings excluded, uncapped): `material - 1` after
/// capping at `MATERIAL_MAX`. Material 0, king against king, shares bucket 0 with material 1: the
/// search scores it a draw before evaluating, but corpora played before self-play ended dead
/// positions still hold such rows.
pub fn material_bucket(material: u32) -> usize {
    material.clamp(1, MATERIAL_MAX as u32) as usize - 1
}

/// Weight sets of an output bucketed by game phase instead, one per phase value 0 to 24.
pub const PHASE_BUCKETS: usize = PHASE_MAX as usize + 1;

/// The output's bucket for `phase` (both sides, PeSTO's `PHASE_INC`, uncapped): the phase capped at
/// `PHASE_MAX`, so a promotion past the start's 24 stays in the last bucket.
pub fn phase_bucket(phase: u32) -> usize {
    phase.min(PHASE_MAX as u32) as usize
}

/// Both sides' game phase, uncapped.
pub fn phase_of_canonical(position: &CanonicalPosition) -> u32 {
    let phase: i32 = position.pieces.iter().map(|&packed| PHASE_INC[unpack_canonical_piece(packed).1 as usize]).sum();
    phase as u32
}

/// `board`'s phase bucket, from the minor and major pieces' bitboard counts, as
/// `material_bucket_of_board` computes the material.
pub fn phase_bucket_of_board(board: &Board) -> usize {
    let mut phase = 0u32;
    for piece_type in [PieceType::Knight, PieceType::Bishop, PieceType::Rook, PieceType::Queen] {
        let count = board.bitboard(Piece::new(Color::White, piece_type)).count() + board.bitboard(Piece::new(Color::Black, piece_type)).count();
        phase += count * PHASE_INC[piece_type as usize] as u32;
    }
    phase_bucket(phase)
}

/// Weight sets of an output bucketed by piece count instead, Stockfish's key: 2 to 32 pieces, kings
/// included, four to a bucket.
pub const COUNT_BUCKETS: usize = 8;

/// The output's bucket for `count` pieces on the board (both sides, kings included):
/// `(count - 1) / 4`, as Stockfish's layer stacks. Capped both ways, though a legal position holds 2
/// to 32 pieces (a promotion replaces its pawn).
pub fn count_bucket(count: u32) -> usize {
    (count.max(1) as usize - 1).min(4 * COUNT_BUCKETS - 1) / 4
}

/// Both sides' pieces, the two kings included.
pub fn count_of_canonical(position: &CanonicalPosition) -> u32 {
    position.pieces.len() as u32 + 2
}

/// `board`'s count bucket, from both sides' occupancy.
pub fn count_bucket_of_board(board: &Board) -> usize {
    count_bucket(board.occupancy(Color::White).count() + board.occupancy(Color::Black).count())
}

/// Weight sets of a tapered output: a middlegame set and an endgame set, both read at every position
/// and blended by the game phase (`taper`), as PeSTO blends its two tables. Not buckets: no set is
/// picked.
pub const TAPER_SETS: usize = 2;

/// A tapered output's value: `(phase * middlegame + (24 - phase) * endgame) / 24`, `phase` capped at
/// `PHASE_MAX` (`phase_bucket`). In f32, the sets' values being f32 already, on every path.
pub fn taper(middlegame: f32, endgame: f32, phase: usize) -> f32 {
    let phase = phase.min(PHASE_MAX as usize) as f32;
    let max = PHASE_MAX as f32;
    (phase * middlegame + (max - phase) * endgame) / max
}

/// `board`'s bucket for an output of `sets` weight sets: by phase for `PHASE_BUCKETS` sets, by piece
/// count for `COUNT_BUCKETS`, by material otherwise. The count of sets is what tells the keys apart,
/// in the file as in memory. For `TAPER_SETS`, the capped phase, which blends both sets (`taper`)
/// rather than picking one.
pub fn output_bucket_of_board(board: &Board, sets: usize) -> usize {
    if sets == PHASE_BUCKETS || sets == TAPER_SETS {
        return phase_bucket_of_board(board);
    }
    if sets == COUNT_BUCKETS {
        return count_bucket_of_board(board);
    }
    material_bucket_of_board(board)
}

/// Our king and their king, in the side to move's frame.
pub fn kings_of_canonical(position: &CanonicalPosition) -> (Square, Square) {
    let mover = position.side_to_move;
    let (ours, theirs) = match mover {
        Color::White => (position.white_king, position.black_king),
        Color::Black => (position.black_king, position.white_king),
    };
    (oriented_square(ours, mover), oriented_square(theirs, mover))
}

/// Both sides' material, kings excluded, uncapped.
pub fn material_of_canonical(position: &CanonicalPosition) -> u32 {
    let material: i32 = position.pieces.iter().map(|&packed| MATERIAL_VALUE[unpack_canonical_piece(packed).1 as usize]).sum();
    material as u32
}

/// The three bucket indices of `position`: our king, their king, material.
pub fn buckets_of_canonical(position: &CanonicalPosition) -> [usize; 3] {
    let (ours, theirs) = kings_of_canonical(position);
    [ours as usize, theirs as usize, material_bucket(material_of_canonical(position))]
}

/// The three bucket indices of `board`, as `buckets_of_canonical` computes them for its canonical
/// form. The material from the ten non-king bitboards' counts: no incremental count is kept.
pub fn buckets_of_board(board: &Board) -> [usize; 3] {
    let mover = board.side_to_move();
    let ours = oriented_square(king_square(board, mover), mover);
    let theirs = oriented_square(king_square(board, mover.opposite()), mover);
    [ours as usize, theirs as usize, material_bucket_of_board(board)]
}

/// `board`'s material bucket alone, for a head bucketed by material only
/// (`psq_threats_pairwise_material`), sparing the kings' lookups.
pub fn material_bucket_of_board(board: &Board) -> usize {
    let mut material = 0u32;
    for piece_type in [PieceType::Pawn, PieceType::Knight, PieceType::Bishop, PieceType::Rook, PieceType::Queen] {
        let count = board.bitboard(Piece::new(Color::White, piece_type)).count() + board.bitboard(Piece::new(Color::Black, piece_type)).count();
        material += count * MATERIAL_VALUE[piece_type as usize] as u32;
    }
    material_bucket(material)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::extract_canonical;

    fn both(fen: &str) -> ([usize; 3], [usize; 3]) {
        let (board, _) = Board::from_fen(fen).unwrap();
        (buckets_of_board(&board), buckets_of_canonical(&extract_canonical(&board)))
    }

    #[test]
    fn the_buckets_are_seen_from_the_side_to_move_and_agree_between_board_and_canonical() {
        let cases = [
            ("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", [Square::E1 as usize, Square::E8 as usize, 77]),
            ("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR b KQkq - 0 1", [Square::E1 as usize, Square::E8 as usize, 77]),
            // Black to move: Black's g6 is our g3, White's c2 their c7; a rook and a knight, 8.
            ("8/8/6k1/8/8/8/2K5/R5n1 b - - 0 1", [Square::G3 as usize, Square::C7 as usize, 7]),
            // White to move, the same pieces: no flip.
            ("8/8/6k1/8/8/8/2K5/R5n1 w - - 0 1", [Square::C2 as usize, Square::G6 as usize, 7]),
            // King against king: material 0 shares bucket 0 with material 1.
            ("8/8/4k3/8/8/4K3/8/8 w - - 0 1", [Square::E3 as usize, Square::E6 as usize, 0]),
            // The start with a second white queen for the a-pawn: 86, capped at 78.
            ("rnbqkbnr/pppppppp/8/8/3Q4/8/1PPPPPPP/RNBQKBNR w KQkq - 0 1", [Square::E1 as usize, Square::E8 as usize, 77]),
        ];
        for (fen, expected) in cases {
            let (board, canonical) = both(fen);
            assert_eq!(board, expected, "{fen}: from the board");
            assert_eq!(canonical, expected, "{fen}: from the canonical position");
        }
    }

    #[test]
    fn the_phase_bucket_agrees_between_board_and_canonical() {
        let cases = [
            ("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", 24, 24),
            ("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR b KQkq - 0 1", 24, 24),
            // Kings and pawns only.
            ("8/pppk4/8/8/8/8/4KPPP/8 w - - 0 1", 0, 0),
            // A queen each, Black to move.
            ("3qk3/8/8/8/8/8/8/3QK3 b - - 0 1", 8, 8),
            // A rook and a knight: 3.
            ("8/8/6k1/8/8/8/2K5/R5n1 b - - 0 1", 3, 3),
            // The start with a second white queen for the a-pawn: 28, capped at 24.
            ("rnbqkbnr/pppppppp/8/8/3Q4/8/1PPPPPPP/RNBQKBNR w KQkq - 0 1", 28, 24),
        ];
        for (fen, phase, bucket) in cases {
            let (board, _) = Board::from_fen(fen).unwrap();
            let canonical = extract_canonical(&board);
            assert_eq!(phase_of_canonical(&canonical), phase, "{fen}: the phase");
            assert_eq!(phase_bucket(phase_of_canonical(&canonical)), bucket, "{fen}: from the canonical position");
            assert_eq!(phase_bucket_of_board(&board), bucket, "{fen}: from the board");
        }
    }

    #[test]
    fn the_phase_bucket_is_the_phase_capped() {
        assert_eq!(PHASE_BUCKETS, 25);
        assert_eq!(phase_bucket(0), 0);
        assert_eq!(phase_bucket(24), 24);
        assert_eq!(phase_bucket(30), 24);
    }

    #[test]
    fn the_count_bucket_agrees_between_board_and_canonical() {
        let cases = [
            ("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", 32, 7),
            ("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR b KQkq - 0 1", 32, 7),
            // Kings alone.
            ("8/8/4k3/8/8/4K3/8/8 w - - 0 1", 2, 0),
            // Kings and three pawns, Black to move: 5, the first of bucket 1.
            ("8/ppp1k3/8/8/8/8/4K3/8 b - - 0 1", 5, 1),
            // The start without four pawns: 28, the last of bucket 6.
            ("rnbqkbnr/pppp4/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", 28, 6),
            // The start without three pawns: 29, the first of bucket 7.
            ("rnbqkbnr/ppppp3/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", 29, 7),
        ];
        for (fen, count, bucket) in cases {
            let (board, _) = Board::from_fen(fen).unwrap();
            let canonical = extract_canonical(&board);
            assert_eq!(count_of_canonical(&canonical), count, "{fen}: the count");
            assert_eq!(count_bucket(count_of_canonical(&canonical)), bucket, "{fen}: from the canonical position");
            assert_eq!(count_bucket_of_board(&board), bucket, "{fen}: from the board");
            assert_eq!(output_bucket_of_board(&board, COUNT_BUCKETS), bucket, "{fen}: as an output's bucket");
        }
    }

    #[test]
    fn the_count_bucket_is_stockfish_s_capped() {
        assert_eq!(count_bucket(0), 0);
        assert_eq!(count_bucket(2), 0);
        assert_eq!(count_bucket(4), 0);
        assert_eq!(count_bucket(5), 1);
        assert_eq!(count_bucket(32), 7);
        assert_eq!(count_bucket(40), 7);
    }

    #[test]
    fn the_output_set_counts_are_distinct() {
        assert_ne!(COUNT_BUCKETS, PHASE_BUCKETS);
        assert_ne!(COUNT_BUCKETS, MATERIAL_BUCKETS);
        for sets in [PHASE_BUCKETS, COUNT_BUCKETS, MATERIAL_BUCKETS] {
            assert_ne!(TAPER_SETS, sets);
        }
    }

    #[test]
    fn the_taper_is_one_set_at_either_end_and_their_mean_halfway() {
        assert_eq!(taper(3.0, -5.0, 24), 3.0);
        assert_eq!(taper(3.0, -5.0, 0), -5.0);
        assert_eq!(taper(3.0, -5.0, 12), -1.0);
        assert_eq!(taper(3.0, -5.0, 30), 3.0, "capped at 24");
        let (board, _) = Board::from_fen("3qk3/8/8/8/8/8/8/3QK3 b - - 0 1").unwrap();
        assert_eq!(output_bucket_of_board(&board, TAPER_SETS), 8, "the phase, two queens");
    }

    #[test]
    fn the_material_bucket_is_material_minus_one_capped() {
        assert_eq!(material_bucket(0), 0);
        assert_eq!(material_bucket(1), 0);
        assert_eq!(material_bucket(2), 1);
        assert_eq!(material_bucket(78), 77);
        assert_eq!(material_bucket(90), 77);
    }
}
