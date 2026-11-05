pub mod square;
pub mod file;
pub mod rank;
pub mod color;
pub mod piece_type;
pub mod piece;
pub mod bitboard;
pub mod castling_rights;
pub mod chess_move;
pub mod fen;
pub mod make_move;
pub mod zobrist;

use bitboard::Bitboard;
use castling_rights::CastlingRights;
use color::Color;
use piece::Piece;
use piece_type::PieceType;
use square::Square;

/// A chess position: piece placement plus the state needed to make and unmake moves
/// (side to move, castling rights, en passant target, halfmove clock). This is FEN fields
/// 1-5; the fullmove number (field 6) has no effect on legal moves or search and is
/// carried separately, wherever FEN/UCI-facing code needs it, rather than living here.
///
/// `Clone`/`Copy` derive for free -- every field already is one (arrays of `Copy` types,
/// plus `Color`/`CastlingRights`/`Square`/`u8`, all `Copy` themselves) -- and are useful
/// wherever code needs a scratch board to mutate and discard without touching the
/// original (e.g. `legal_moves`' per-candidate legality check), without needing a
/// `&mut Board` just to do so.
#[derive(Debug, Clone, Copy)]
pub struct Board {
    /// One bitboard per `Piece`, indexed by `piece as usize` -- `Piece`'s own numbering
    /// (`color * 6 + piece_type`, no gaps) was chosen specifically so this indexing needs
    /// no translation. Combined occupancy is deliberately not stored: it holds no
    /// information beyond what these 12 bitboards already contain, so it's derived on
    /// demand by OR-ing the relevant ones together instead of being kept in sync.
    pieces: [Bitboard; 12],
    /// What's on each square, kept in sync with `pieces` on every move, per CLAUDE.md's
    /// locked-in architecture -- answers "what's on this square" in O(1) instead of
    /// scanning up to 12 bitboards.
    mailbox: [Option<Piece>; 64],
    /// Whose turn it is.
    side_to_move: Color,
    castling_rights: CastlingRights,
    /// The square a pawn just double-pushed past, if any -- `None` when no en passant
    /// capture is available this move.
    en_passant_square: Option<Square>,
    /// FEN field 5: plies since the last pawn move or capture, for the fifty-move rule.
    halfmove_clock: u8,
    /// A Zobrist hash of this exact position -- piece placement, side to move, castling
    /// rights, en passant square, deliberately not `halfmove_clock` (two positions differing
    /// only in that count are still the same position for repetition purposes). Kept
    /// incrementally correct by `place_piece`/`remove_piece` (piece placement) and by
    /// `make_move`/`unmake_move` (the rest), the same reasoning `mailbox` itself is kept in
    /// sync rather than rebuilt: recomputing this from scratch on every use would defeat the
    /// entire reason to have an O(1) position-identity key in the first place.
    hash: u64,
    /// The keys a correction history is indexed by (`zobrist::correction_key`): the pawns of
    /// both sides, White's other pieces, Black's. Kept by `place_piece`/`remove_piece` as `hash`
    /// is, three more XORs a change.
    correction_keys: [u64; 3],
}

impl Board {
    /// The bitboard for one specific piece (e.g. white knights).
    pub fn bitboard(&self, piece: Piece) -> Bitboard {
        self.pieces[piece as usize]
    }

    /// Every square occupied by any of `color`'s pieces, regardless of piece type.
    /// Derived on demand, not stored -- see the note on `pieces` above. Goes through
    /// `Piece::new` for each piece type rather than relying on `Piece`'s numbering
    /// directly, so this code doesn't need to change if that numbering ever did.
    pub fn occupancy(&self, color: Color) -> Bitboard {
        let mut result = Bitboard::EMPTY;
        for piece_type in PieceType::ALL {
            result = result | self.bitboard(Piece::new(color, piece_type));
        }
        result
    }

    /// What's on `square`, or `None` if it's empty. The public face of the mailbox array
    /// (see its own field comment) -- an O(1) alternative to scanning all 12 `bitboard`s
    /// for whichever one happens to contain `square`, for callers (move ordering is the
    /// first one) that need "what's here" without also wanting a `Bitboard` back.
    pub fn piece_at(&self, square: Square) -> Option<Piece> {
        self.mailbox[square as usize]
    }

    /// Every occupied square, regardless of color -- unlike `occupancy`, which answers
    /// for one color at a time. Needed wherever "is this square empty at all" matters
    /// (e.g. a pawn push, which -- unlike a capture -- is only legal onto a truly empty
    /// square) rather than "empty of my own pieces specifically."
    pub fn all_occupancy(&self) -> Bitboard {
        self.occupancy(Color::White) | self.occupancy(Color::Black)
    }

    /// The square a pawn just double-pushed past, if an en passant capture is available
    /// this move -- `None` otherwise.
    pub fn en_passant_square(&self) -> Option<Square> {
        self.en_passant_square
    }

    /// Whose turn it actually is. Unlike every move-generation function (which takes
    /// `color` as an explicit parameter, anticipating generating for either side), perft
    /// always walks the game exactly as it's being played, so it reads this directly
    /// rather than tracking color itself.
    pub fn side_to_move(&self) -> Color {
        self.side_to_move
    }

    /// Which castling rights (kingside/queenside, per color) are still available.
    pub fn castling_rights(&self) -> CastlingRights {
        self.castling_rights
    }

    /// This exact position's Zobrist hash -- see the `hash` field's own doc comment for
    /// what it covers and how it's kept correct.
    pub fn hash(&self) -> u64 {
        self.hash
    }

    /// The pawn structure's key, both sides' pawns, for a correction history.
    pub fn pawn_key(&self) -> u64 {
        self.correction_keys[0]
    }

    /// `color`'s pieces other than pawns, its king included, as a key for a correction history.
    pub fn non_pawn_key(&self, color: Color) -> u64 {
        self.correction_keys[1 + color as usize]
    }

    /// Plies since the last pawn move or capture. Bounds how far back a repetition search
    /// ever needs to look: an irreversible move (a pawn move or capture) makes every
    /// position from before it permanently unreachable again, so nothing beyond this many
    /// plies back can ever be a genuine match.
    pub fn halfmove_clock(&self) -> u8 {
        self.halfmove_clock
    }

    /// A board with `placement` on it and the given state, a halfmove clock of 0. For rebuilding a
    /// position from what a training record stores, which has no FEN; no legality is checked, as
    /// the record was written from a legal board.
    pub fn from_placement(
        placement: &[(Square, Piece)], side_to_move: Color, castling_rights: CastlingRights,
        en_passant_square: Option<Square>,
    ) -> Board {
        let mut pieces = [Bitboard::EMPTY; 12];
        let mut mailbox = [None; 64];
        for &(square, piece) in placement {
            pieces[piece as usize] = pieces[piece as usize] | Bitboard::from_square(square);
            mailbox[square as usize] = Some(piece);
        }
        let mut board =
            Board { pieces, mailbox, side_to_move, castling_rights, en_passant_square, halfmove_clock: 0, hash: 0, correction_keys: [0; 3] };
        board.hash = zobrist::compute(&board);
        board.correction_keys = zobrist::compute_correction_keys(&board);
        board
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn piece_at_reads_an_occupied_square() {
        let (board, _) =
            Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        assert_eq!(board.piece_at(Square::E1), Some(Piece::new(Color::White, PieceType::King)));
        assert_eq!(board.piece_at(Square::E8), Some(Piece::new(Color::Black, PieceType::King)));
    }

    #[test]
    fn piece_at_is_none_on_an_empty_square() {
        let (board, _) =
            Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        assert_eq!(board.piece_at(Square::E4), None);
    }
}
