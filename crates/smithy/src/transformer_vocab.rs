//! The Fattorini Transformer's piece-centric token vocabulary -- see the transformer design's own
//! "Vocabulary" section for the settled design this implements. Every square/file/rank here is
//! relative to the side to move (own's forward is always increasing rank), never the absolute
//! board -- `oriented_square` (made `pub(crate)` in `features.rs` for this module's sake) is what
//! actually performs that conversion; this file only consumes already-oriented squares.
//!
//! Deliberately its own top-level module, not nested under `features.rs`: that file's
//! submodules (`psq`, `psq_threats`) are NNUE-style fixed-width feature sets,
//! and this is a different kind of thing entirely (a variable-length token sequence, no
//! accumulator).

use fattorini::board::color::Color;
use fattorini::board::piece_type::PieceType;
use fattorini::board::square::Square;

use fattorini::features::{oriented_square, unpack_canonical_piece, CanonicalPosition};

/// One `[CLS]` token, prepended once per sequence -- see the Vocabulary section's item 1.
const CLS_TOKENS: usize = 1;

/// One token per (square, castling-rights pair): `64 * 4`.
const KING_TOKENS: usize = 64 * 4;

/// One token per square -- queen, rook, bishop, and knight tokens all share this shape (no
/// identity, no castling suffix), see the Vocabulary section's items 3, 4, 5, 6.
const QUEEN_TOKENS: usize = 64;
const ROOK_TOKENS: usize = 64;
const BISHOP_TOKENS: usize = 64;
const KNIGHT_TOKENS: usize = 64;

/// `48` base squares (ranks 2-7, relative) plus `8` extra en-passant-starred variants (rank 5
/// only -- a pawn can only ever be captured en passant the move immediately after its own double
/// step, which always lands it on relative rank 5, never rank 4) -- see the Vocabulary section's
/// item 7 for why ranks 1 and 8 never appear here at all (a pawn reaching the back rank promotes
/// and becomes one of the tokens above instead).
const PAWN_TOKENS: usize = 48 + 8;

/// The whole vocabulary's size -- `1 + 256 + 64 + 64 + 64 + 64 + 56 = 569`, matching
/// the transformer design's own settled total exactly.
pub const VOCAB_SIZE: usize =
    CLS_TOKENS + KING_TOKENS + QUEEN_TOKENS + ROOK_TOKENS + BISHOP_TOKENS + KNIGHT_TOKENS + PAWN_TOKENS;

/// `[CLS]` sits at vocabulary index `0` -- both the very first block and, not by coincidence, the
/// very first token of every sequence (see `token_sequence` below).
const CLS_INDEX: usize = 0;

/// Where each piece type's own block starts in the full `0..VOCAB_SIZE` vocabulary -- `K < Q < R <
/// B < N < P`, the same order the Vocabulary section itself lists them in (items 2-7). A local
/// index from `king_token_index`/`plain_square_token_index`/`pawn_token_index` becomes a real
/// vocabulary index by adding the matching offset directly at the call site below (5b-5e) --
/// deliberately no wrapper function per piece type for this, since that would just be
/// `OFFSET + local_index` copied six times under six different names.
const KING_OFFSET: usize = CLS_TOKENS;
const QUEEN_OFFSET: usize = KING_OFFSET + KING_TOKENS;
const ROOK_OFFSET: usize = QUEEN_OFFSET + QUEEN_TOKENS;
const BISHOP_OFFSET: usize = ROOK_OFFSET + ROOK_TOKENS;
const KNIGHT_OFFSET: usize = BISHOP_OFFSET + BISHOP_TOKENS;
const PAWN_OFFSET: usize = KNIGHT_OFFSET + KNIGHT_TOKENS;

/// Whether `vocabulary_index` (a value `token_sequence` produced, not a raw `CanonicalPosition`
/// field) names a King token -- kings-only attention's own mask (the transformer design's
/// Attention section: "zeroes out every pair that doesn't include a king row or column") needs
/// exactly this per-position check, and only this crate's own vocabulary layout can answer it, so
/// it lives here rather than the batch-assembly code re-deriving `KING_OFFSET..QUEEN_OFFSET` for
/// itself. True for *either* side's king (own or opponent) -- the check is purely "is this a king
/// token at all", not which one.
pub fn is_king_token(vocabulary_index: usize) -> bool {
    (KING_OFFSET..QUEEN_OFFSET).contains(&vocabulary_index)
}

/// `square`'s index in file-major order (`a1=0, a2=1, ..., a8=7, b1=8, ..., h8=63`) -- the
/// ordering the transformer design's "Token order" rule actually needs (`a1 < a2 < ... < a8 <
/// b1 < ...`), *not* `Square`'s own native LERF numbering (`Square as u8`), which is rank-major
/// (`a1=0, b1=1, ..., h1=7, a2=8, ...`) -- the opposite axis order. Shared by every piece type's
/// own token index, not just the king's below.
fn file_major_index(square: Square) -> u8 {
    square.file() as u8 * 8 + square.rank() as u8
}

/// One King token's index within the King block (`0..256`, before the `CLS_TOKENS` offset that
/// places it in the full vocabulary). `square` is already oriented (relative to the side to
/// move); `kingside`/`queenside` are that specific king's own remaining castling rights, not the
/// whole position's. Square is the outer axis and rights the inner one (`square_index * 4 +
/// rights`), matching the Vocabulary section's own enumeration order (`Ka1-00, Ka1-01, Ka1-10,
/// Ka1-11, ..., Kh8-11` -- every rights combination for `a1` before moving on to `a2`, not every
/// square for `00` before moving on to `01`). The rights bit order is kingside-then-queenside,
/// matching FEN's own `KQ` convention -- kingside alone is `2`, not `1`.
fn king_token_index(square: Square, kingside: bool, queenside: bool) -> usize {
    let rights = (kingside as usize) * 2 + (queenside as usize);
    file_major_index(square) as usize * 4 + rights
}

/// One token's index within its own `0..64` block, for any piece type whose vocabulary is just
/// "one token per square" -- queen, rook, bishop, and knight all share this exact shape (see the
/// Vocabulary section's items 3-6: no identity, no castling suffix, unlike king or pawn), so one
/// function serves all four rather than four identical wrappers around the same file-major index.
fn plain_square_token_index(square: Square) -> usize {
    file_major_index(square) as usize
}

/// One Pawn token's index within the Pawn block (`0..56`). `square` is already oriented (relative
/// to the side to move) and must be on a pawn-reachable rank -- human ranks `2..=7`, i.e. rank
/// index `1..=6` -- since ranks `1`/`8` are impossible for a pawn token at all: a pawn reaching
/// the back rank promotes into one of the other piece types instead (Vocabulary section item 7),
/// never staying a pawn token. `en_passant_star` is this specific pawn's own capturability flag
/// (the grammar's trailing `*`), legal only on human rank `5` (rank index `4`): a pawn is only
/// ever capturable en passant the move immediately after its own double step, and that step
/// always lands it on relative rank 5, never rank 4 -- moving to rank 4 is a single-square
/// advance, never a double step, so it's never en-passant-capturable.
///
/// Layout, an implementation choice (the design doc only settles the `48 + 8` *count*, not an
/// enumeration order): every unstarred token first (`0..48`, file-major over the `6`
/// pawn-reachable ranks), then every starred token (`48..56`, file-major over the single
/// en-passant-eligible rank) -- two disjoint blocks, since a given pawn uses exactly one or the
/// other, never both, for the same square.
fn pawn_token_index(square: Square, en_passant_star: bool) -> usize {
    let rank = square.rank() as usize;
    assert!((1..=6).contains(&rank), "a pawn token's square must be on rank index 1..=6, got {rank}");
    let file = square.file() as usize;

    if !en_passant_star {
        return file * 6 + (rank - 1);
    }

    assert!(rank == 4, "en passant star is only legal on rank index 4, got {rank}");
    48 + file
}

/// Every one of `piece_color`'s own `piece_type` pieces in `canonical`, as oriented (relative to
/// `canonical.side_to_move`) squares, sorted file-major -- the building block 5d's per-side block
/// reads from for queen/rook/bishop/knight/pawn alike. `piece_color` (whose pieces to gather) and
/// `canonical.side_to_move` (which single whole-board orientation every returned square is
/// expressed in) are deliberately different things: orientation is *always* driven by whoever is
/// to move, for both own and opponent pieces alike, through `oriented_square` -- never by
/// `piece_color` itself, which is why
/// this function doesn't take a separate "perspective" parameter that could be passed the wrong
/// value by mistake.
fn oriented_squares_of(canonical: &CanonicalPosition, piece_color: Color, piece_type: PieceType) -> Vec<Square> {
    let mut squares: Vec<Square> = canonical
        .pieces
        .iter()
        .filter_map(|&packed| {
            let (square, this_type, this_color) = unpack_canonical_piece(packed);
            if this_type == piece_type && this_color == piece_color {
                Some(oriented_square(square, canonical.side_to_move))
            } else {
                None
            }
        })
        .collect();
    squares.sort_by_key(|&square| file_major_index(square));
    squares
}

/// `color`'s own King token, as a full vocabulary index (`KING_OFFSET`-shifted, not the raw
/// `0..256` local index `king_token_index` returns on its own). `color`'s own king square comes
/// straight off `canonical.white_king`/`black_king` (both always present -- every legal position
/// has exactly one king per side), oriented the same way every other square in this sequence is.
/// `color`'s own castling rights come from `canonical.castling_rights` -- `CastlingRights::
/// kingside`/`queenside` already take the specific color to check, so this reads exactly that
/// king's own two bits, never the whole position's four.
fn king_token(canonical: &CanonicalPosition, color: Color) -> usize {
    let king_square = match color {
        Color::White => canonical.white_king,
        Color::Black => canonical.black_king,
    };
    let oriented = oriented_square(king_square, canonical.side_to_move);
    let kingside = canonical.castling_rights.kingside(color);
    let queenside = canonical.castling_rights.queenside(color);
    KING_OFFSET + king_token_index(oriented, kingside, queenside)
}

/// `color`'s entire block of tokens, `K < Q < R < B < N < P` order (the Vocabulary section's own
/// item order) -- one call per side builds the own-side block, another the opponent-side block;
/// 5e concatenates the two (own first) into the full sequence.
///
/// The en passant star: `canonical.en_passant_file` is set exactly when the side to move has a
/// legal en passant capture available, naming the file of the pawn that would be captured -- that
/// pawn always belongs to the *other* color (the one that just played the double step) and always
/// sits on oriented rank index `4` (relative rank 5), confirmed above. So a pawn here gets the
/// star only when `color` is not `canonical.side_to_move` (this is the opponent's own block, from
/// whichever side_block call is building it) and its file matches -- never on `side_to_move`'s own
/// pawns, which can never be the captured pawn.
fn side_block(canonical: &CanonicalPosition, color: Color) -> Vec<usize> {
    let mut tokens = vec![king_token(canonical, color)];

    for &square in &oriented_squares_of(canonical, color, PieceType::Queen) {
        tokens.push(QUEEN_OFFSET + plain_square_token_index(square));
    }
    for &square in &oriented_squares_of(canonical, color, PieceType::Rook) {
        tokens.push(ROOK_OFFSET + plain_square_token_index(square));
    }
    for &square in &oriented_squares_of(canonical, color, PieceType::Bishop) {
        tokens.push(BISHOP_OFFSET + plain_square_token_index(square));
    }
    for &square in &oriented_squares_of(canonical, color, PieceType::Knight) {
        tokens.push(KNIGHT_OFFSET + plain_square_token_index(square));
    }
    for &square in &oriented_squares_of(canonical, color, PieceType::Pawn) {
        let en_passant_star = color != canonical.side_to_move
            && square.rank() as usize == 4
            && canonical.en_passant_file == Some(square.file() as u8);
        tokens.push(PAWN_OFFSET + pawn_token_index(square, en_passant_star));
    }

    tokens
}

/// The full token sequence for `canonical`, as vocabulary indices ready for an embedding lookup --
/// no string, no tokenizer, ever (see the design doc's settled "no FEN, no tokenizer" point):
/// `[CLS]` followed by the side-to-move's own block, then the opponent's, per the Vocabulary
/// section's "own side before opponent" ordering rule. Length varies with piece count (`2..=32`
/// pieces on the board, `3..=33` tokens once `[CLS]` is counted), which is exactly why this
/// returns a `Vec` rather than a fixed-width array -- there's no padding at this layer; that's a
/// batch-construction concern (step 3), not a per-position one.
pub fn token_sequence(canonical: &CanonicalPosition) -> Vec<usize> {
    let mut tokens = vec![CLS_INDEX];
    tokens.extend(side_block(canonical, canonical.side_to_move));
    tokens.extend(side_block(canonical, canonical.side_to_move.opposite()));
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;
    use fattorini::features::extract_canonical;
    use fattorini::board::Board;

    const STARTING_POSITION_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
    const STARTING_POSITION_BLACK_TO_MOVE_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR b KQkq - 0 1";

    #[test]
    fn oriented_squares_of_gathers_and_sorts_file_major_when_unmirrored() {
        let (board, _) = Board::from_fen(STARTING_POSITION_FEN).unwrap();
        let canonical = extract_canonical(&board);
        // White to move: no mirroring, so White's own knights come back at their real squares.
        assert_eq!(oriented_squares_of(&canonical, Color::White, PieceType::Knight), vec![Square::B1, Square::G1]);
    }

    #[test]
    fn oriented_squares_of_mirrors_every_color_the_same_way_when_black_is_to_move() {
        let (board, _) = Board::from_fen(STARTING_POSITION_BLACK_TO_MOVE_FEN).unwrap();
        let canonical = extract_canonical(&board);
        // Black to move mirrors the *whole* board -- including White's own pieces, not just
        // Black's -- since orientation follows side_to_move alone, never piece_color.
        assert_eq!(oriented_squares_of(&canonical, Color::White, PieceType::Knight), vec![Square::B8, Square::G8]);
        assert_eq!(oriented_squares_of(&canonical, Color::Black, PieceType::Knight), vec![Square::B1, Square::G1]);
    }

    #[test]
    fn king_token_for_the_starting_position_with_both_castling_rights() {
        let (board, _) = Board::from_fen(STARTING_POSITION_FEN).unwrap();
        let canonical = extract_canonical(&board);
        // White's king: E1, unmirrored (White to move), both rights -- file_major(E1) = 4*8+0 = 32,
        // rights = kingside(1)*2 + queenside(1) = 3, local = 32*4+3 = 131, full = KING_OFFSET+131.
        assert_eq!(king_token(&canonical, Color::White), KING_OFFSET + 131);
        // Black's king: E8, unmirrored too (orientation follows side_to_move, not this king's own
        // color) -- file_major(E8) = 4*8+7 = 39, same both-rights suffix, local = 39*4+3 = 159.
        assert_eq!(king_token(&canonical, Color::Black), KING_OFFSET + 159);
    }

    #[test]
    fn king_token_reads_each_color_own_two_rights_bits_not_the_others() {
        // White kingside only ("K"), Black queenside only ("q") -- if this function ever read the
        // wrong color's bits, this is what would catch it.
        let (board, _) = Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w Kq - 0 1").unwrap();
        let canonical = extract_canonical(&board);
        // White: kingside=true, queenside=false -> rights = 1*2+0 = 2, local = 32*4+2 = 130.
        assert_eq!(king_token(&canonical, Color::White), KING_OFFSET + 130);
        // Black: kingside=false, queenside=true -> rights = 0*2+1 = 1, local = 39*4+1 = 157.
        assert_eq!(king_token(&canonical, Color::Black), KING_OFFSET + 157);
    }

    #[test]
    fn king_token_mirrors_when_black_is_to_move() {
        let (board, _) = Board::from_fen(STARTING_POSITION_BLACK_TO_MOVE_FEN).unwrap();
        let canonical = extract_canonical(&board);
        // White's king is oriented to E8 now (mirrored), landing on the exact same full index
        // Black's own king had in the White-to-move test above -- both rights, same math.
        assert_eq!(king_token(&canonical, Color::White), KING_OFFSET + 159);
    }

    #[test]
    fn side_block_orders_pieces_king_first_then_queen_rook_bishop_knight_pawn() {
        let (board, _) = Board::from_fen(STARTING_POSITION_FEN).unwrap();
        let canonical = extract_canonical(&board);
        let white = side_block(&canonical, Color::White);
        // King, one queen, two rooks, two bishops, two knights, eight pawns: 1+1+2+2+2+8 = 16.
        assert_eq!(white.len(), 16);
        assert_eq!(white[0], KING_OFFSET + 131); // Same E1, both-rights math as the king_token test.
        assert_eq!(white[1], QUEEN_OFFSET + plain_square_token_index(Square::D1));
        assert_eq!(white[2], ROOK_OFFSET + plain_square_token_index(Square::A1));
        assert_eq!(white[3], ROOK_OFFSET + plain_square_token_index(Square::H1));
        assert_eq!(white[4], BISHOP_OFFSET + plain_square_token_index(Square::C1));
        assert_eq!(white[5], BISHOP_OFFSET + plain_square_token_index(Square::F1));
        assert_eq!(white[6], KNIGHT_OFFSET + plain_square_token_index(Square::B1));
        assert_eq!(white[7], KNIGHT_OFFSET + plain_square_token_index(Square::G1));
        // Eight pawns, one per file, none starred -- no en passant is available at move 1.
        let pawn_squares =
            [Square::A2, Square::B2, Square::C2, Square::D2, Square::E2, Square::F2, Square::G2, Square::H2];
        for (i, &square) in pawn_squares.iter().enumerate() {
            assert_eq!(white[8 + i], PAWN_OFFSET + pawn_token_index(square, false));
        }
    }

    #[test]
    fn side_block_stars_the_capturable_pawn_on_the_opponent_side_only() {
        // 1. e4 e6 2. e5 d5 -- Black's just-double-stepped d5 pawn is en-passant-capturable by
        // White (to move), whose own e5 pawn sits on that very same oriented rank (5) but must
        // never itself be starred, since it isn't the pawn en passant would capture.
        let fen = "rnbqkbnr/ppp2ppp/4p3/3pP3/8/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 3";
        let (board, _) = Board::from_fen(fen).unwrap();
        let canonical = extract_canonical(&board);
        assert_eq!(canonical.en_passant_file, Some(3)); // d-file.

        let black = side_block(&canonical, Color::Black);
        let white = side_block(&canonical, Color::White);
        assert!(black.contains(&(PAWN_OFFSET + pawn_token_index(Square::D5, true))));
        assert!(white.contains(&(PAWN_OFFSET + pawn_token_index(Square::E5, false))));
    }

    #[test]
    fn token_sequence_starts_with_cls_then_own_side_then_opponent_side() {
        let (board, _) = Board::from_fen(STARTING_POSITION_FEN).unwrap();
        let canonical = extract_canonical(&board);
        let sequence = token_sequence(&canonical);

        // 1 CLS + 16 White tokens + 16 Black tokens = 33, the maximum length (32 pieces).
        assert_eq!(sequence.len(), 33);
        assert_eq!(sequence[0], CLS_INDEX);
        assert_eq!(&sequence[1..17], side_block(&canonical, Color::White).as_slice());
        assert_eq!(&sequence[17..33], side_block(&canonical, Color::Black).as_slice());
    }

    #[test]
    fn token_sequence_puts_the_side_to_move_first_regardless_of_color() {
        let (board, _) = Board::from_fen(STARTING_POSITION_BLACK_TO_MOVE_FEN).unwrap();
        let canonical = extract_canonical(&board);
        let sequence = token_sequence(&canonical);

        // Black is to move here, so Black's block comes right after [CLS], not White's.
        assert_eq!(&sequence[1..17], side_block(&canonical, Color::Black).as_slice());
        assert_eq!(&sequence[17..33], side_block(&canonical, Color::White).as_slice());
    }

    #[test]
    fn is_king_token_is_true_for_exactly_the_king_block() {
        assert!(!is_king_token(CLS_INDEX));
        assert!(is_king_token(KING_OFFSET));
        assert!(is_king_token(KING_OFFSET + KING_TOKENS - 1));
        assert!(!is_king_token(QUEEN_OFFSET));
        assert!(!is_king_token(PAWN_OFFSET + PAWN_TOKENS - 1));
    }

    #[test]
    fn vocab_size_matches_the_settled_design_doc_total() {
        assert_eq!(VOCAB_SIZE, 569);
    }

    #[test]
    fn block_offsets_tile_the_vocabulary_with_no_gaps_or_overlaps() {
        assert_eq!(CLS_INDEX, 0);
        assert_eq!(KING_OFFSET, CLS_TOKENS);
        assert_eq!(QUEEN_OFFSET, KING_OFFSET + KING_TOKENS);
        assert_eq!(ROOK_OFFSET, QUEEN_OFFSET + QUEEN_TOKENS);
        assert_eq!(BISHOP_OFFSET, ROOK_OFFSET + ROOK_TOKENS);
        assert_eq!(KNIGHT_OFFSET, BISHOP_OFFSET + BISHOP_TOKENS);
        assert_eq!(PAWN_OFFSET, KNIGHT_OFFSET + KNIGHT_TOKENS);
        assert_eq!(PAWN_OFFSET + PAWN_TOKENS, VOCAB_SIZE);
    }

    #[test]
    fn file_major_index_orders_by_file_first_then_rank() {
        assert_eq!(file_major_index(Square::A1), 0);
        assert_eq!(file_major_index(Square::A2), 1);
        assert_eq!(file_major_index(Square::A8), 7);
        assert_eq!(file_major_index(Square::B1), 8);
        assert_eq!(file_major_index(Square::H8), 63);
    }

    #[test]
    fn king_token_index_enumerates_rights_before_advancing_square() {
        // a1 with no rights is the very first king token.
        assert_eq!(king_token_index(Square::A1, false, false), 0);
        // Bit order is kingside-then-queenside: kingside alone is 2, not 1.
        assert_eq!(king_token_index(Square::A1, false, true), 1);
        assert_eq!(king_token_index(Square::A1, true, false), 2);
        assert_eq!(king_token_index(Square::A1, true, true), 3);
        // Moving to the next square (still file a) advances by a full block of 4.
        assert_eq!(king_token_index(Square::A2, false, false), 4);
    }

    #[test]
    fn king_token_index_stays_within_the_king_block_for_every_square_and_rights_combination() {
        for n in 0u8..64 {
            let square = Square::try_from(n).unwrap();
            for kingside in [false, true] {
                for queenside in [false, true] {
                    let index = king_token_index(square, kingside, queenside);
                    assert!(index < KING_TOKENS, "{square:?} kingside={kingside} queenside={queenside} -> {index}");
                }
            }
        }
    }

    #[test]
    fn plain_square_token_index_matches_file_major_index_directly() {
        assert_eq!(plain_square_token_index(Square::A1), 0);
        assert_eq!(plain_square_token_index(Square::B1), 8);
        assert_eq!(plain_square_token_index(Square::H8), 63);
    }

    #[test]
    fn plain_square_token_index_stays_within_a_64_token_block_for_every_square() {
        // Not `QUEEN_TOKENS` here on purpose -- this function is shared by queen, rook, bishop,
        // and knight alike (all four constants happen to equal 64), so bounding against one of
        // them by name would imply a connection to that piece specifically that isn't real.
        for n in 0u8..64 {
            let square = Square::try_from(n).unwrap();
            let index = plain_square_token_index(square);
            assert!(index < 64, "{square:?} -> {index}");
        }
    }

    #[test]
    fn pawn_token_index_unstarred_block_boundaries() {
        assert_eq!(pawn_token_index(Square::A2, false), 0);
        assert_eq!(pawn_token_index(Square::A7, false), 5);
        // Moving to the next file advances by a full block of 6 (the 6 pawn-reachable ranks).
        assert_eq!(pawn_token_index(Square::B2, false), 6);
        assert_eq!(pawn_token_index(Square::H7, false), 47);
    }

    #[test]
    fn pawn_token_index_starred_block_boundaries() {
        assert_eq!(pawn_token_index(Square::A5, true), 48);
        assert_eq!(pawn_token_index(Square::B5, true), 49);
        assert_eq!(pawn_token_index(Square::H5, true), 55);
    }

    #[test]
    #[should_panic]
    fn pawn_token_index_rejects_the_back_ranks() {
        pawn_token_index(Square::A1, false);
    }

    #[test]
    #[should_panic]
    fn pawn_token_index_rejects_a_star_outside_rank_five() {
        pawn_token_index(Square::A4, true);
    }

    #[test]
    fn pawn_token_index_never_collides_across_every_valid_combination() {
        let mut seen = std::collections::HashSet::new();
        for n in 0u8..64 {
            let square = Square::try_from(n).unwrap();
            let rank = square.rank() as u8;
            if !(1..=6).contains(&rank) {
                continue;
            }
            let star_options: &[bool] = if rank == 4 { &[false, true] } else { &[false] };
            for &star in star_options {
                let index = pawn_token_index(square, star);
                assert!(index < PAWN_TOKENS, "{square:?} star={star} -> {index}");
                assert!(seen.insert(index), "{square:?} star={star} -> {index} collides with an earlier combination");
            }
        }
    }
}
