use super::bitboard::Bitboard;
use super::castling_rights::CastlingRights;
use super::color::Color;
use super::file::File;
use super::piece::Piece;
use super::rank::Rank;
use super::square::Square;
use super::zobrist;
use super::Board;

/// A reason `Board::from_fen` rejected an input string. One variant per FEN field, so a
/// future caller (the UCI `position` command) can match on exactly what went wrong rather
/// than only displaying a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FenError {
    WrongFieldCount { found: usize },
    InvalidPiecePlacement,
    InvalidSideToMove,
    InvalidCastlingRights,
    InvalidEnPassantSquare,
    InvalidHalfmoveClock,
    InvalidFullmoveNumber,
}

/// Maps a FEN piece letter to the piece it denotes -- uppercase for White, lowercase for
/// Black. This case-encodes-color scheme is FEN's own convention (not, say, SAN's or
/// UCI's promotion-letter convention), so it stays a private detail of this module rather
/// than a `Piece` method.
fn piece_from_fen_char(c: char) -> Option<Piece> {
    match c {
        'P' => Some(Piece::WhitePawn),
        'N' => Some(Piece::WhiteKnight),
        'B' => Some(Piece::WhiteBishop),
        'R' => Some(Piece::WhiteRook),
        'Q' => Some(Piece::WhiteQueen),
        'K' => Some(Piece::WhiteKing),
        'p' => Some(Piece::BlackPawn),
        'n' => Some(Piece::BlackKnight),
        'b' => Some(Piece::BlackBishop),
        'r' => Some(Piece::BlackRook),
        'q' => Some(Piece::BlackQueen),
        'k' => Some(Piece::BlackKing),
        _ => None,
    }
}

/// The FEN letter for `piece` -- the inverse of `piece_from_fen_char`. Reached through
/// `to_fen`/`piece_placement_to_fen` below, no longer `#[cfg(test)]` themselves (see `to_fen`'s
/// own doc comment for why), so this one no longer needs the gate either.
fn piece_to_fen_char(piece: Piece) -> char {
    match piece {
        Piece::WhitePawn => 'P',
        Piece::WhiteKnight => 'N',
        Piece::WhiteBishop => 'B',
        Piece::WhiteRook => 'R',
        Piece::WhiteQueen => 'Q',
        Piece::WhiteKing => 'K',
        Piece::BlackPawn => 'p',
        Piece::BlackKnight => 'n',
        Piece::BlackBishop => 'b',
        Piece::BlackRook => 'r',
        Piece::BlackQueen => 'q',
        Piece::BlackKing => 'k',
    }
}

/// Parses FEN field 1 (piece placement) into the 12 piece bitboards and the mailbox.
/// Structural validation only: right shape, valid letters, each rank's squares summing to
/// exactly 8 -- not chess-legality properties (e.g. two white kings) `Board` itself
/// doesn't police.
fn parse_piece_placement(field: &str) -> Result<([Bitboard; 12], [Option<Piece>; 64]), FenError> {
    let ranks: Vec<&str> = field.split('/').collect();
    if ranks.len() != 8 {
        return Err(FenError::InvalidPiecePlacement);
    }

    let mut pieces = [Bitboard::EMPTY; 12];
    let mut mailbox: [Option<Piece>; 64] = [None; 64];

    for (rank_index, rank_field) in ranks.into_iter().enumerate() {
        // FEN lists ranks top to bottom (rank 8 first); LERF numbers them bottom to top.
        let rank = Rank::try_from(7 - rank_index as u8).unwrap();
        let mut file_index = 0u8;

        for c in rank_field.chars() {
            if file_index >= 8 {
                return Err(FenError::InvalidPiecePlacement);
            }

            if let Some(empty_count) = c.to_digit(10) {
                if empty_count == 0 || empty_count > 8 {
                    return Err(FenError::InvalidPiecePlacement);
                }
                file_index += empty_count as u8;
                continue;
            }

            let piece = piece_from_fen_char(c).ok_or(FenError::InvalidPiecePlacement)?;
            let file = File::try_from(file_index).unwrap();
            let square = Square::new(file, rank);

            pieces[piece as usize] = pieces[piece as usize] | Bitboard::from_square(square);
            mailbox[square as usize] = Some(piece);
            file_index += 1;
        }

        if file_index != 8 {
            return Err(FenError::InvalidPiecePlacement);
        }
    }

    Ok((pieces, mailbox))
}

fn parse_side_to_move(field: &str) -> Result<Color, FenError> {
    match field {
        "w" => Ok(Color::White),
        "b" => Ok(Color::Black),
        _ => Err(FenError::InvalidSideToMove),
    }
}

fn parse_castling_rights(field: &str) -> Result<CastlingRights, FenError> {
    if field == "-" {
        return Ok(CastlingRights::NONE);
    }

    let mut rights = CastlingRights::NONE;
    for c in field.chars() {
        match c {
            'K' => rights.set_kingside(Color::White, true),
            'Q' => rights.set_queenside(Color::White, true),
            'k' => rights.set_kingside(Color::Black, true),
            'q' => rights.set_queenside(Color::Black, true),
            _ => return Err(FenError::InvalidCastlingRights),
        }
    }
    Ok(rights)
}

fn parse_en_passant_square(field: &str) -> Result<Option<Square>, FenError> {
    if field == "-" {
        return Ok(None);
    }

    let mut chars = field.chars();
    let file_char = chars.next().ok_or(FenError::InvalidEnPassantSquare)?;
    let rank_char = chars.next().ok_or(FenError::InvalidEnPassantSquare)?;
    if chars.next().is_some() {
        return Err(FenError::InvalidEnPassantSquare);
    }

    let file = File::from_char(file_char).ok_or(FenError::InvalidEnPassantSquare)?;
    let rank = Rank::from_char(rank_char).ok_or(FenError::InvalidEnPassantSquare)?;
    Ok(Some(Square::new(file, rank)))
}

fn parse_halfmove_clock(field: &str) -> Result<u8, FenError> {
    field.parse::<u8>().map_err(|_| FenError::InvalidHalfmoveClock)
}

fn parse_fullmove_number(field: &str) -> Result<u32, FenError> {
    field.parse::<u32>().map_err(|_| FenError::InvalidFullmoveNumber)
}

impl Board {
    /// Parses a full 6-field FEN string. Returns the `Board` (fields 1-5) alongside the
    /// fullmove number (field 6) separately, since `Board` itself doesn't store it -- see
    /// "FEN scope" in the design notes for why.
    pub fn from_fen(fen: &str) -> Result<(Board, u32), FenError> {
        let fields: Vec<&str> = fen.split_whitespace().collect();
        if fields.len() != 6 {
            return Err(FenError::WrongFieldCount { found: fields.len() });
        }

        let (pieces, mailbox) = parse_piece_placement(fields[0])?;
        let side_to_move = parse_side_to_move(fields[1])?;
        let castling_rights = parse_castling_rights(fields[2])?;
        let en_passant_square = parse_en_passant_square(fields[3])?;
        let halfmove_clock = parse_halfmove_clock(fields[4])?;
        let fullmove_number = parse_fullmove_number(fields[5])?;

        let mut board = Board {
            pieces,
            mailbox,
            side_to_move,
            castling_rights,
            en_passant_square,
            halfmove_clock,
            // Placeholder -- `zobrist::compute` needs a real `&Board` to read the rest of
            // this state back out of, so the real value is filled in right after.
            hash: 0,
            correction_keys: [0; 3],
        };
        board.hash = zobrist::compute(&board);
        board.correction_keys = zobrist::compute_correction_keys(&board);

        Ok((board, fullmove_number))
    }

    /// Prints a full 6-field FEN string. `fullmove_number` is a parameter, not a field,
    /// for the same reason `from_fen` returns it separately: `Board` doesn't carry it.
    ///
    /// Not `#[cfg(test)]`: besides the round-trip tests, real callers print positions with it, as
    /// the error messages that name the FEN where a game went wrong.
    pub fn to_fen(&self, fullmove_number: u32) -> String {
        let side_to_move = match self.side_to_move {
            Color::White => "w",
            Color::Black => "b",
        };
        let en_passant_square = match self.en_passant_square {
            Some(square) => format!("{}{}", square.file().to_char(), square.rank().to_char()),
            None => "-".to_string(),
        };

        format!(
            "{} {} {} {} {} {}",
            self.piece_placement_to_fen(),
            side_to_move,
            self.castling_rights_to_fen(),
            en_passant_square,
            self.halfmove_clock,
            fullmove_number
        )
    }

    /// Builds FEN field 1 by reading the mailbox, not the piece bitboards -- the mailbox
    /// exists specifically to answer "what's on this square" in O(1). Reached through `to_fen`
    /// above, no longer `#[cfg(test)]` itself -- see its own doc comment for why.
    fn piece_placement_to_fen(&self) -> String {
        let mut ranks = Vec::with_capacity(8);

        for rank_value in (0u8..8).rev() {
            let rank = Rank::try_from(rank_value).unwrap();
            let mut rank_field = String::new();
            let mut empty_count = 0u8;

            for file_value in 0u8..8 {
                let file = File::try_from(file_value).unwrap();
                let square = Square::new(file, rank);

                match self.mailbox[square as usize] {
                    Some(piece) => {
                        if empty_count > 0 {
                            rank_field.push_str(&empty_count.to_string());
                            empty_count = 0;
                        }
                        rank_field.push(piece_to_fen_char(piece));
                    }
                    None => {
                        empty_count += 1;
                    }
                }
            }

            if empty_count > 0 {
                rank_field.push_str(&empty_count.to_string());
            }

            ranks.push(rank_field);
        }

        ranks.join("/")
    }

    /// Builds FEN field 3 in "KQkq"-subset order, or "-" if no rights remain. Reached through
    /// `to_fen`, same reasoning as `piece_placement_to_fen` just above for no longer being
    /// `#[cfg(test)]`.
    fn castling_rights_to_fen(&self) -> String {
        let mut field = String::new();
        if self.castling_rights.kingside(Color::White) {
            field.push('K');
        }
        if self.castling_rights.queenside(Color::White) {
            field.push('Q');
        }
        if self.castling_rights.kingside(Color::Black) {
            field.push('k');
        }
        if self.castling_rights.queenside(Color::Black) {
            field.push('q');
        }

        if field.is_empty() {
            return "-".to_string();
        }
        field
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starting_position_round_trips() {
        let starting_fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
        let (board, fullmove_number) = Board::from_fen(starting_fen).unwrap();
        assert_eq!(fullmove_number, 1);
        assert_eq!(board.to_fen(fullmove_number), starting_fen);
    }

    #[test]
    fn en_passant_and_no_castling_round_trip() {
        let fen = "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1";
        let (board, fullmove_number) = Board::from_fen(fen).unwrap();
        assert_eq!(board.to_fen(fullmove_number), fen);
    }

    #[test]
    fn wrong_field_count_is_rejected() {
        let bad_fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -";
        assert!(matches!(
            Board::from_fen(bad_fen),
            Err(FenError::WrongFieldCount { found: 4 })
        ));
    }

    #[test]
    fn invalid_piece_letter_is_rejected() {
        let bad_fen = "rnbqkbnx/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
        assert!(matches!(Board::from_fen(bad_fen), Err(FenError::InvalidPiecePlacement)));
    }

    #[test]
    fn wrong_rank_count_is_rejected() {
        let bad_fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP w KQkq - 0 1";
        assert!(matches!(Board::from_fen(bad_fen), Err(FenError::InvalidPiecePlacement)));
    }

    #[test]
    fn rank_not_summing_to_eight_is_rejected() {
        let bad_fen = "rnbqkbn/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
        assert!(matches!(Board::from_fen(bad_fen), Err(FenError::InvalidPiecePlacement)));
    }

    #[test]
    fn invalid_side_to_move_is_rejected() {
        let bad_fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR x KQkq - 0 1";
        assert!(matches!(Board::from_fen(bad_fen), Err(FenError::InvalidSideToMove)));
    }

    #[test]
    fn invalid_castling_rights_is_rejected() {
        let bad_fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkqX - 0 1";
        assert!(matches!(Board::from_fen(bad_fen), Err(FenError::InvalidCastlingRights)));
    }

    #[test]
    fn invalid_en_passant_square_is_rejected() {
        let bad_fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq z9 0 1";
        assert!(matches!(Board::from_fen(bad_fen), Err(FenError::InvalidEnPassantSquare)));
    }

    #[test]
    fn invalid_halfmove_clock_is_rejected() {
        let bad_fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - x 1";
        assert!(matches!(Board::from_fen(bad_fen), Err(FenError::InvalidHalfmoveClock)));
    }

    #[test]
    fn invalid_fullmove_number_is_rejected() {
        let bad_fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 x";
        assert!(matches!(Board::from_fen(bad_fen), Err(FenError::InvalidFullmoveNumber)));
    }
}
