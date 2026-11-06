use crate::board::castling_rights::CastlingRights;
use crate::board::color::Color;
use crate::board::piece::Piece;
use crate::board::piece_type::PieceType;
use crate::board::square::Square;
use crate::board::Board;

/// One position's canonical, perspective-agnostic snapshot: everything `extract_canonical` reads
/// off a `Board` once, cheap enough to store on disk directly (`cache.rs`), and everything
/// `psq::active_rows` and `psq_threats::expand` need to reconstruct both training
/// perspectives from, under whichever feature set the network trains on -- deliberately *not*
/// oriented, mirrored, or split by perspective, since it's shared by every feature set rather than
/// committed to one. `own_pieces`/`opponent_pieces` used to be two independently stored,
/// already-mirrored copies of the same physical board, but one is always a pure, deterministic
/// function of the other (mirror the squares, flip who's "own"), so storing both was storing the
/// same information twice. `side_to_move` is the one piece of information that used to be implicit
/// (baked into which physical color's list got written into which field at build time) and now has
/// to be explicit, since there's no longer a build-time "own" list to bake it into -- `expand`
/// needs it to know which king's perspective is "own" versus "opponent". `castling_rights` and
/// `en_passant_file` exist only for the psq feature set's sake (its castling and en passant rows)
/// but live here rather
/// than psq-locally: a frozen test set stores positions once and every feature set's `expand` only
/// ever reads back what got written to disk, so anything any feature set needs has to be a
/// `CanonicalPosition` field, not synthesized later from a `Board` that no longer exists by training
/// time.
#[derive(Clone)]
pub struct CanonicalPosition {
    pub side_to_move: Color,
    /// Every non-king piece on the board, packed via `pack_canonical_piece` -- at most 30 (a
    /// legal position starts with exactly 32 pieces, 2 of them kings, and only loses pieces to
    /// captures, never gains beyond that).
    pub pieces: Vec<u16>,
    pub white_king: Square,
    pub black_king: Square,
    /// Absolute (White/Black), never reoriented here -- reorienting into own/opponent kingside/
    /// queenside is each feature set's own `expand`'s job, same split `pieces` already uses.
    pub castling_rights: CastlingRights,
    /// The en passant target square's *file* only (0..7) -- its rank is never free information
    /// (always rank 3 or 6, already fully determined by `side_to_move`), and a vertical mirror
    /// only ever flips rank, never file, so this needs no reorientation at any `expand` site,
    /// unlike `pieces` or `castling_rights`. `None` when no en passant capture is available.
    pub en_passant_file: Option<u8>,
}

/// Packs one canonical (on-disk, unmirrored) piece into a single `u16`: its absolute `square`
/// (`0..64`) and its own type-and-color identity (`0..10` -- 5 non-king types x White/Black, not
/// own/opponent, since a canonical snapshot has no perspective yet to be relative to) never
/// collide and comfortably fit (`63*10 + 9 = 639`). Deliberately a different packing from either
/// feature set's own per-perspective `pack_piece`: this one describes the raw board every
/// perspective is derived from, not an already-computed perspective.
pub fn pack_canonical_piece(square: Square, piece_type: PieceType, color: Color) -> u16 {
    let type_and_color = piece_type as u16 * 2 + color as u16;
    square as u16 * 10 + type_and_color
}

/// The exact inverse of `pack_canonical_piece`.
pub fn unpack_canonical_piece(packed: u16) -> (Square, PieceType, Color) {
    let type_and_color = packed % 10;
    let square = Square::try_from((packed / 10) as u8).expect("packed canonical square out of range");
    let piece_type = PieceType::try_from((type_and_color / 2) as u8).expect("packed canonical piece type out of range");
    let color = Color::try_from((type_and_color % 2) as u8).expect("packed canonical color out of range");
    (square, piece_type, color)
}

/// Mirrors `square` vertically (rank flipped, file unchanged: `square XOR 56`, exact under LERF
/// numbering since rank occupies bits 3-5 and file occupies bits 0-2) when `color` is `Black`,
/// leaving it unchanged for `White`. Matches real Stockfish NNUE's own current convention (HalfKA,
/// mirroring rather than the older, castling-incorrect full-rotation HalfKP -- rotation swaps
/// kingside and queenside, which loses the real asymmetry castling rights introduce between them,
/// confirmed directly against Stockfish's own GitHub issue history rather than assumed): each
/// color's own perspective is expressed in a common coordinate frame so the same weights serve
/// both, never a raw absolute square. Shared by every feature set's own `expand`.
/// `pub` rather than private: `transformer_vocab.rs` reuses this exact same rule (own forward
/// always increasing rank) for its own, differently-shaped vocabulary, rather than reimplementing
/// an identical mirror -- and now that this module lives in the library rather than inside each
/// training binary, that reuse crosses a crate boundary, so `pub(crate)` no longer reaches it.
pub fn oriented_square(square: Square, color: Color) -> Square {
    match color {
        Color::White => square,
        Color::Black => Square::try_from(square as u8 ^ 56).unwrap(),
    }
}

/// One color's own king square out of `canonical`'s own two stored kings -- `white_king`/
/// `black_king` are both already absolute (never mirrored); orienting either for a specific
/// perspective is each feature set's own `expand`'s job, not this lookup's.
fn king_square_of(canonical: &CanonicalPosition, color: Color) -> Square {
    match color {
        Color::White => canonical.white_king,
        Color::Black => canonical.black_king,
    }
}

/// Scans `board` once for everything a `CanonicalPosition` needs: side to move, every non-king
/// occupied square (paired with its own type and color, absolute, never mirrored), both kings' own
/// absolute squares, castling rights, and the en passant target file. The only place this project
/// ever walks the board to build training features -- iterates `PieceType::ALL` x both colors
/// rather than all 64 squares, since a real position has at most 30 non-king occupied squares out
/// of 64. Feature-set-agnostic: one `prepare`d cache built from this works for training under any
/// feature set.
pub fn extract_canonical(board: &Board) -> CanonicalPosition {
    let mut pieces = Vec::with_capacity(30);
    for color in [Color::White, Color::Black] {
        for piece_type in PieceType::ALL {
            if piece_type == PieceType::King {
                continue;
            }
            for square in board.bitboard(Piece::new(color, piece_type)).squares() {
                pieces.push(pack_canonical_piece(square, piece_type, color));
            }
        }
    }

    let white_king = king_square(board, Color::White);
    let black_king = king_square(board, Color::Black);
    let en_passant_file = board.en_passant_square().map(|square| square.file() as u8);

    CanonicalPosition {
        side_to_move: board.side_to_move(),
        pieces,
        white_king,
        black_king,
        castling_rights: board.castling_rights(),
        en_passant_file,
    }
}

/// The exact inverse of `extract_canonical`, up to the halfmove clock, which a record does not
/// store: the board a training record was written from. psq_threats' attacks come from the engine's
/// own attack generation, which reads a `Board`, so the record goes back to one rather than the
/// attacks being rebuilt a second way.
pub fn board_from_canonical(canonical: &CanonicalPosition) -> Board {
    let mut placement: Vec<(Square, Piece)> = canonical
        .pieces
        .iter()
        .map(|&packed| {
            let (square, piece_type, color) = unpack_canonical_piece(packed);
            (square, Piece::new(color, piece_type))
        })
        .collect();
    placement.push((canonical.white_king, Piece::new(Color::White, PieceType::King)));
    placement.push((canonical.black_king, Piece::new(Color::Black, PieceType::King)));
    // The en passant square is on the rank the pawn skipped: the sixth for White to move, the third
    // for Black.
    let en_passant_square = canonical.en_passant_file.map(|file| {
        let rank = match canonical.side_to_move {
            Color::White => 5,
            Color::Black => 2,
        };
        Square::try_from(rank * 8 + file).expect("an en passant file is 0 to 7")
    });
    Board::from_placement(&placement, canonical.side_to_move, canonical.castling_rights, en_passant_square)
}

fn king_square(board: &Board, color: Color) -> Square {
    board
        .bitboard(Piece::new(color, PieceType::King))
        .squares()
        .next()
        .expect("a legal position always has exactly one king per side")
}

/// Kalo Chess psq's own feature encoding: plain piece-square, the input elsewhere called "768" or
/// bullet's `Chess768`, plus castling rights and the en passant file -- 780 rows per perspective.
///
/// Two perspectives, each oriented for its own color (mirrored vertically for Black) with every
/// identity own/opponent relative to that color. They go through one shared transform, so the
/// weights are 780 rows, not 1560. Two perspectives rather than one side-to-move-oriented view are
/// what make an incremental accumulator possible: each perspective belongs to a fixed color, so a
/// move changes only the few rows it touches, where a single side-to-move-oriented view would
/// reorient every feature at every move.
///
/// A row is never shared by two pieces (a square holds one piece), so every active row is exactly
/// 1.
pub mod psq {
    use crate::board::color::Color;
    use crate::board::piece_type::PieceType;
    use crate::board::square::Square;

    use super::{king_square_of, oriented_square, unpack_canonical_piece, CanonicalPosition};

    /// Both perspectives' active rows. `own` is the side to move's, `opponent` the other side's,
    /// the order the network concatenates them in.
    pub struct PositionFeatures {
        pub own: Vec<u16>,
        pub opponent: Vec<u16>,
    }

    /// 6 piece types (king included) x 2 relative colors.
    const IDENTITY_COUNT: usize = 12;

    /// The first of the four castling rows, right after the `12 * 64 = 768` piece rows.
    pub const CASTLING_ROW: usize = IDENTITY_COUNT * 64;

    /// The first of the eight en passant file rows, right after the castling ones.
    pub const EN_PASSANT_ROW: usize = CASTLING_ROW + 4;

    /// The transform's row count per perspective, `780`.
    pub const ROWS: usize = EN_PASSANT_ROW + 8;

    /// The row of one piece seen from one perspective: `is_own` is whether the piece belongs to
    /// the perspective's color, `oriented` its square already oriented for that color. Public
    /// because the engine's incremental update computes single rows without a
    /// `CanonicalPosition`, and must agree with `active_rows` exactly.
    pub fn piece_row(piece_type: PieceType, is_own: bool, oriented: Square) -> usize {
        let identity = piece_type as usize * 2 + if is_own { 0 } else { 1 };
        identity * 64 + oriented as usize
    }

    /// Every active row of `perspective`'s view of `canonical`: one per piece (kings included),
    /// then its castling rights (own kingside, own queenside, opponent kingside, opponent
    /// queenside, relative to `perspective`), then the en passant file. The file needs no
    /// orientation: a vertical mirror keeps files.
    pub fn active_rows(canonical: &CanonicalPosition, perspective: Color) -> Vec<u16> {
        let opponent = perspective.opposite();
        let mut rows = Vec::with_capacity(canonical.pieces.len() + 2 + 4 + 1);

        for &packed in &canonical.pieces {
            let (square, piece_type, color) = unpack_canonical_piece(packed);
            rows.push(piece_row(piece_type, color == perspective, oriented_square(square, perspective)) as u16);
        }
        for (color, is_own) in [(perspective, true), (opponent, false)] {
            let king = oriented_square(king_square_of(canonical, color), perspective);
            rows.push(piece_row(PieceType::King, is_own, king) as u16);
        }

        let rights = canonical.castling_rights;
        let castling = [
            rights.kingside(perspective),
            rights.queenside(perspective),
            rights.kingside(opponent),
            rights.queenside(opponent),
        ];
        for (index, &active) in castling.iter().enumerate() {
            if active {
                rows.push((CASTLING_ROW + index) as u16);
            }
        }
        if let Some(file) = canonical.en_passant_file {
            rows.push((EN_PASSANT_ROW + file as usize) as u16);
        }
        rows
    }

    pub fn expand(canonical: &CanonicalPosition) -> PositionFeatures {
        let side_to_move = canonical.side_to_move;
        PositionFeatures {
            own: active_rows(canonical, side_to_move),
            opponent: active_rows(canonical, side_to_move.opposite()),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use super::super::extract_canonical;
        use crate::board::Board;

        const STARTING_POSITION_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

        fn sorted(mut rows: Vec<u16>) -> Vec<u16> {
            rows.sort_unstable();
            rows
        }

        #[test]
        fn the_starting_position_has_32_piece_rows_and_4_castling_rows_per_perspective() {
            let (board, _) = Board::from_fen(STARTING_POSITION_FEN).unwrap();
            let features = expand(&extract_canonical(&board));
            for rows in [&features.own, &features.opponent] {
                assert_eq!(rows.len(), 36);
                assert_eq!(rows.iter().filter(|&&row| (row as usize) < CASTLING_ROW).count(), 32);
                assert_eq!(rows.iter().filter(|&&row| (CASTLING_ROW..EN_PASSANT_ROW).contains(&(row as usize))).count(), 4);
            }
        }

        #[test]
        fn rows_are_in_range_and_never_repeat() {
            let fens = [
                STARTING_POSITION_FEN,
                "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10",
                "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1",
                "8/8/8/4k3/8/4K3/8/8 w - - 0 1",
            ];
            for fen in fens {
                let (board, _) = Board::from_fen(fen).unwrap();
                let features = expand(&extract_canonical(&board));
                for rows in [features.own, features.opponent] {
                    assert!(rows.iter().all(|&row| (row as usize) < ROWS), "{fen}: row out of range");
                    let mut unique = sorted(rows.clone());
                    unique.dedup();
                    assert_eq!(unique.len(), rows.len(), "{fen}: a row repeats");
                }
            }
        }

        #[test]
        fn a_color_symmetric_position_gives_both_perspectives_the_same_rows() {
            // The starting position is its own mirror with colors swapped, so White's view and
            // Black's view must coincide: orientation and own/opponent identities cancel color out.
            let (board, _) = Board::from_fen(STARTING_POSITION_FEN).unwrap();
            let features = expand(&extract_canonical(&board));
            assert_eq!(sorted(features.own), sorted(features.opponent));
        }

        #[test]
        fn a_position_and_its_color_swapped_mirror_give_the_same_features() {
            // The side to move's view is what `own` holds, so mirroring the board and swapping
            // colors and the side to move must leave both lists unchanged.
            let (white_to_move, _) = Board::from_fen("4k3/8/8/8/8/8/4P3/R3K3 w Q - 0 1").unwrap();
            let (black_to_move, _) = Board::from_fen("r3k3/4p3/8/8/8/8/8/4K3 b q - 0 1").unwrap();
            let white = expand(&extract_canonical(&white_to_move));
            let black = expand(&extract_canonical(&black_to_move));
            assert_eq!(sorted(white.own), sorted(black.own));
            assert_eq!(sorted(white.opponent), sorted(black.opponent));
        }

        #[test]
        fn castling_rights_are_relative_to_the_perspective() {
            // White keeps only kingside, Black only queenside.
            let (board, _) = Board::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w Kq - 0 1").unwrap();
            let canonical = extract_canonical(&board);
            let white = active_rows(&canonical, Color::White);
            let black = active_rows(&canonical, Color::Black);

            assert!(white.contains(&(CASTLING_ROW as u16)), "White: own kingside");
            assert!(white.contains(&(CASTLING_ROW as u16 + 3)), "White: opponent queenside");
            assert!(black.contains(&(CASTLING_ROW as u16 + 1)), "Black: own queenside");
            assert!(black.contains(&(CASTLING_ROW as u16 + 2)), "Black: opponent kingside");
            assert_eq!(white.iter().filter(|&&row| (CASTLING_ROW..EN_PASSANT_ROW).contains(&(row as usize))).count(), 2);
            assert_eq!(black.iter().filter(|&&row| (CASTLING_ROW..EN_PASSANT_ROW).contains(&(row as usize))).count(), 2);
        }

        #[test]
        fn the_en_passant_file_reaches_both_perspectives_unmirrored() {
            // After 1. e4 the target is on the e-file, index 4, for both perspectives.
            let (board, _) = Board::from_fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1").unwrap();
            let features = expand(&extract_canonical(&board));
            assert!(features.own.contains(&(EN_PASSANT_ROW as u16 + 4)));
            assert!(features.opponent.contains(&(EN_PASSANT_ROW as u16 + 4)));
        }

        #[test]
        fn a_piece_row_matches_its_square_and_identity() {
            // White's own knight on c3 seen by White, and the same knight seen by Black: an
            // opponent knight on the mirrored square c6.
            let (board, _) = Board::from_fen("4k3/8/8/8/8/2N5/8/4K3 w - - 0 1").unwrap();
            let canonical = extract_canonical(&board);
            assert!(active_rows(&canonical, Color::White).contains(&(piece_row(PieceType::Knight, true, Square::C3) as u16)));
            assert!(active_rows(&canonical, Color::Black).contains(&(piece_row(PieceType::Knight, false, Square::C6) as u16)));
        }
    }
}

/// PSQ plus threats: psq's 780 rows, then one row for every square pair `(i, j)` where the
/// perspective's own piece on `i` attacks an opponent piece on `j` -- 4,876 rows per perspective.
///
/// A perspective holds its own threats only; the other perspective holds the opponent's, so both
/// directions are seen and a threat row means the same thing in both. Both squares are oriented as
/// psq's are (mirrored vertically for Black), so a position and its color-swapped mirror give the
/// same rows. A threat row does not say which pieces attack and are attacked: the piece-square rows
/// do, and since both go through one shared transform, added before the activation, a neuron can
/// learn their conjunction.
///
/// "Attacks" are the engine's plain attack sets: a slider stops at the first piece of either color,
/// pins are ignored, no x-rays, en passant is not a threat, the king attacks as any piece. Only pairs
/// a queen or knight move apart can ever be set; the rest of the 64 x 64 rows are never active.
pub mod psq_threats {
    use crate::board::color::Color;
    use crate::board::piece::Piece;
    use crate::board::piece_type::PieceType;
    use crate::board::square::Square;
    use crate::board::Board;
    use crate::movegen::bishop::bishop_attacks;
    use crate::movegen::king::king_attacks;
    use crate::movegen::knight::knight_attacks;
    use crate::movegen::pawn::pawn_attacks;
    use crate::movegen::queen::queen_attacks;
    use crate::movegen::rook::rook_attacks;

    use super::{board_from_canonical, psq, oriented_square, CanonicalPosition};

    /// Both perspectives' active rows. `own` is the side to move's, `opponent` the other side's,
    /// the order the network concatenates them in.
    pub struct PositionFeatures {
        pub own: Vec<u16>,
        pub opponent: Vec<u16>,
    }

    /// The first threat row, right after psq's.
    pub const THREAT_ROW: usize = psq::ROWS;

    /// The transform's row count per perspective, `4876`.
    pub const ROWS: usize = THREAT_ROW + 64 * 64;

    /// The row of the threat from `attacker` to `target`, both already oriented for the perspective.
    /// Public because the engine computes threat rows from a `Board`, and must agree exactly.
    pub fn threat_row(attacker: Square, target: Square) -> usize {
        THREAT_ROW + 64 * attacker as usize + target as usize
    }

    /// Pushes onto `rows` the threat row of every attack by `perspective`'s pieces on an opponent
    /// piece in `board`.
    pub fn push_threat_rows(board: &Board, perspective: Color, rows: &mut Vec<u16>) {
        push_threat_rows_from(board, perspective, THREAT_ROW, false, rows);
    }

    /// `push_threat_rows` with the threat rows starting at `first_row`, both squares of every
    /// threat mirrored left-right when `mirror` (`halfk`'s, decided by the perspective's king).
    pub fn push_threat_rows_from(board: &Board, perspective: Color, first_row: usize, mirror: bool, rows: &mut Vec<u16>) {
        let flip = if mirror { 7 } else { 0 };
        let oriented = |square: Square| oriented_square(square, perspective) as usize ^ flip;
        let occupancy = board.all_occupancy();
        let targets = board.occupancy(perspective.opposite());
        for piece_type in PieceType::ALL {
            for square in board.bitboard(Piece::new(perspective, piece_type)).squares() {
                let attacks = match piece_type {
                    PieceType::Pawn => pawn_attacks(square, perspective),
                    PieceType::Knight => knight_attacks(square),
                    PieceType::Bishop => bishop_attacks(square, occupancy),
                    PieceType::Rook => rook_attacks(square, occupancy),
                    PieceType::Queen => queen_attacks(square, occupancy),
                    PieceType::King => king_attacks(square),
                };
                let attacker = oriented(square);
                for target in (attacks & targets).squares() {
                    rows.push((first_row + 64 * attacker + oriented(target)) as u16);
                }
            }
        }
    }

    /// Every active row of `perspective`'s view: psq's rows of `canonical`, then the threat rows
    /// of `board`, the same position rebuilt.
    fn active_rows(canonical: &CanonicalPosition, board: &Board, perspective: Color) -> Vec<u16> {
        let mut rows = psq::active_rows(canonical, perspective);
        push_threat_rows(board, perspective, &mut rows);
        rows
    }

    /// Both perspectives' rows. The attacks need a `Board`, so the record is rebuilt into one
    /// (`board_from_canonical`), once for both.
    pub fn expand(canonical: &CanonicalPosition) -> PositionFeatures {
        let board = board_from_canonical(canonical);
        let side_to_move = canonical.side_to_move;
        PositionFeatures {
            own: active_rows(canonical, &board, side_to_move),
            opponent: active_rows(canonical, &board, side_to_move.opposite()),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use super::super::extract_canonical;
        use crate::board::bitboard::Bitboard;
        use crate::movegen::see::attackers_to;

        const STARTING_POSITION_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

        fn sorted(mut rows: Vec<u16>) -> Vec<u16> {
            rows.sort_unstable();
            rows
        }

        fn threats(rows: &[u16]) -> Vec<u16> {
            sorted(rows.iter().copied().filter(|&row| row as usize >= THREAT_ROW).collect())
        }

        fn features(fen: &str) -> PositionFeatures {
            let (board, _) = Board::from_fen(fen).unwrap();
            expand(&extract_canonical(&board))
        }

        /// `fen` mirrored vertically with colors and the side to move swapped. Castling and en
        /// passant are dropped, so callers pass positions with neither.
        fn color_swapped_mirror(fen: &str) -> String {
            let fields: Vec<&str> = fen.split_whitespace().collect();
            let placement: Vec<String> = fields[0]
                .split('/')
                .rev()
                .map(|rank| {
                    rank.chars()
                        .map(|c| if c.is_ascii_uppercase() { c.to_ascii_lowercase() } else { c.to_ascii_uppercase() })
                        .collect()
                })
                .collect();
            let side = if fields[1] == "w" { "b" } else { "w" };
            format!("{} {side} - - 0 1", placement.join("/"))
        }

        #[test]
        fn the_starting_position_has_no_threats() {
            let features = features(STARTING_POSITION_FEN);
            assert!(threats(&features.own).is_empty());
            assert!(threats(&features.opponent).is_empty());
        }

        #[test]
        fn the_psq_rows_come_first_unchanged() {
            let (board, _) = Board::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10").unwrap();
            let canonical = extract_canonical(&board);
            let features = expand(&canonical);
            let psq = psq::expand(&canonical);
            assert_eq!(features.own[..psq.own.len()], psq.own[..]);
            assert_eq!(features.opponent[..psq.opponent.len()], psq.opponent[..]);
        }

        #[test]
        fn a_pawn_attacking_a_knight_is_a_threat_of_its_own_side_only() {
            // White's pawn on e4 attacks Black's knight on d5; the knight attacks nothing.
            let features = features("4k3/8/8/3n4/4P3/8/8/4K3 w - - 0 1");
            assert_eq!(threats(&features.own), vec![threat_row(Square::E4, Square::D5) as u16]);
            assert!(threats(&features.opponent).is_empty());
        }

        #[test]
        fn black_threats_are_oriented_for_black() {
            // Black's pawn on e5 attacks White's knight on d4: from Black's mirrored view, e4 to d5.
            let features = features("4k3/8/8/4p3/3N4/8/8/4K3 b - - 0 1");
            assert_eq!(threats(&features.own), vec![threat_row(Square::E4, Square::D5) as u16]);
        }

        #[test]
        fn a_slider_attacks_the_first_piece_on_its_line_only() {
            // White's rook on a1 sees Black's knight on a3, not the queen behind it on a6.
            let features = features("4k3/8/q7/8/8/n7/8/R3K3 w - - 0 1");
            let own = threats(&features.own);
            assert!(own.contains(&(threat_row(Square::A1, Square::A3) as u16)));
            assert!(!own.contains(&(threat_row(Square::A1, Square::A6) as u16)));
        }

        #[test]
        fn a_king_attacks_as_any_piece() {
            // White's king on d1 attacks Black's pawn on d2.
            let features = features("8/8/8/8/8/3k4/3p4/3K4 w - - 0 1");
            assert!(threats(&features.own).contains(&(threat_row(Square::D1, Square::D2) as u16)));
        }

        #[test]
        fn a_position_and_its_color_swapped_mirror_give_the_same_features() {
            let fens = [
                "4k3/8/8/3n4/4P3/8/8/4K3 w - - 0 1",
                "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w - - 0 10",
                "2r3k1/1q3ppp/p2p4/1p1Pb3/4P3/1P1Q1N2/P4PPP/2R3K1 b - - 0 1",
            ];
            for fen in fens {
                let original = features(fen);
                let mirrored = features(&color_swapped_mirror(fen));
                assert_eq!(sorted(original.own), sorted(mirrored.own), "{fen}");
                assert_eq!(sorted(original.opponent), sorted(mirrored.opponent), "{fen}");
            }
        }

        #[test]
        fn threats_agree_with_see_attackers_and_stay_within_queen_and_knight_pairs() {
            // Random games: each perspective's threat rows are exactly the (own attacker, opponent
            // target) pairs that SEE's own attacker enumeration finds, and every one is a queen or
            // knight move apart.
            use crate::movegen::legal::legal_moves;
            let mut seed = 0x9E3779B97F4A7C15u64;
            let mut positions = 0;
            for _ in 0..30 {
                let mut board = Board::from_fen(STARTING_POSITION_FEN).unwrap().0;
                for _ in 0..120 {
                    let features = expand(&extract_canonical(&board));
                    for (perspective, rows) in
                        [(board.side_to_move(), &features.own), (board.side_to_move().opposite(), &features.opponent)]
                    {
                        let mut expected = Vec::new();
                        for target in board.occupancy(perspective.opposite()).squares() {
                            let attackers = attackers_to(&board, target, board.all_occupancy()) & board.occupancy(perspective);
                            for attacker in attackers.squares() {
                                expected.push(threat_row(
                                    oriented_square(attacker, perspective),
                                    oriented_square(target, perspective),
                                ) as u16);
                            }
                        }
                        let actual = threats(rows);
                        assert_eq!(actual, sorted(expected), "{}", board.to_fen(1));
                        for row in actual {
                            let pair = row as usize - THREAT_ROW;
                            let attacker = Square::try_from((pair / 64) as u8).unwrap();
                            let target = Bitboard::from_square(Square::try_from((pair % 64) as u8).unwrap());
                            let reachable = queen_attacks(attacker, Bitboard::EMPTY) | knight_attacks(attacker);
                            assert!((reachable & target).count() == 1, "{}: row {row}", board.to_fen(1));
                        }
                        assert!(rows.iter().all(|&row| (row as usize) < ROWS));
                    }
                    positions += 1;
                    let moves = legal_moves(&board, board.side_to_move());
                    if moves.is_empty() {
                        break;
                    }
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    board.make_move(moves[(seed % moves.len() as u64) as usize]);
                }
            }
            assert!(positions > 1000, "only {positions} positions");
        }
    }
}

/// Stockfish's HalfKAv2_hm, called halfk for short, plus psq_threats' threats: each perspective's
/// pieces seen from its own king's square, 22,528 rows, then the 4,096 threat rows -- 26,624 rows per
/// perspective.
///
/// A row is (king bucket, identity, square). The perspective is oriented as psq's (mirrored
/// vertically for Black), then mirrored left-right when its own king stands on files e to h, so the
/// king takes one of 32 buckets (4 files x 8 ranks) and every other square is mirrored with it. The
/// identities are 11: own pawn to queen, the opponent's pawn to queen, and one plane both kings share
/// (Stockfish's: the perspective's own king is always in its bucket's plane, so the two never meet
/// on one square). No castling or en passant rows, as Stockfish's.
///
/// The threat rows are psq_threats', shared by every bucket, their squares mirrored with the king.
/// Training adds `FACTOR_ROWS` rows of its own, one per (identity, square) whatever the bucket, which
/// the export folds into every bucket's rows, so that a rarely seen bucket still learns from every
/// position through the shared rows; the engine never
/// sees them.
pub mod halfk {
    use crate::board::color::Color;
    use crate::board::piece_type::PieceType;
    use crate::board::square::Square;
    use crate::board::Board;

    use super::{board_from_canonical, king_square_of, oriented_square, psq_threats, unpack_canonical_piece, CanonicalPosition};

    /// Both perspectives' active rows. `own` is the side to move's, `opponent` the other side's,
    /// the order the network concatenates them in.
    pub struct PositionFeatures {
        pub own: Vec<u16>,
        pub opponent: Vec<u16>,
    }

    /// The king's buckets: 4 files (after the mirror) x 8 ranks.
    pub const BUCKETS: usize = 32;

    /// Own pawn to queen, the opponent's pawn to queen, and the kings' shared plane.
    pub const IDENTITIES: usize = 11;

    /// The identity both kings share.
    const KING_IDENTITY: usize = 10;

    /// One bucket's rows, and the factor's row count: 11 x 64 = 704.
    pub const FACTOR_ROWS: usize = IDENTITIES * 64;

    /// The piece rows, `32 * 704 = 22,528`.
    pub const HALFK_ROWS: usize = BUCKETS * FACTOR_ROWS;

    /// The first threat row, right after the piece rows.
    pub const THREAT_ROW: usize = HALFK_ROWS;

    /// The transform's row count per perspective, `26,624`.
    pub const ROWS: usize = THREAT_ROW + 64 * 64;

    /// The trainer's transform: the engine's rows, then the factor's, `27,328`.
    pub const TRAINING_ROWS: usize = ROWS + FACTOR_ROWS;

    /// The bucket of a king already oriented for its perspective, and whether that perspective is
    /// mirrored left-right (the king on files e to h).
    pub fn king_bucket(king: Square) -> (usize, bool) {
        let (file, rank) = (king.file() as usize, king.rank() as usize);
        let mirrored = file >= 4;
        let file = if mirrored { 7 - file } else { file };
        (rank * 4 + file, mirrored)
    }

    /// The row of one piece seen from one perspective: its king's `bucket` and `mirrored`
    /// (`king_bucket`), `is_own` whether the piece belongs to the perspective's color (ignored for a
    /// king), `oriented` its square oriented for that color, not yet mirrored. Public because the
    /// engine's incremental update computes single rows without a `CanonicalPosition`, and must agree
    /// with `active_rows` exactly.
    pub fn piece_row(bucket: usize, mirrored: bool, piece_type: PieceType, is_own: bool, oriented: Square) -> usize {
        let identity = match piece_type {
            PieceType::King => KING_IDENTITY,
            _ => piece_type as usize + if is_own { 0 } else { 5 },
        };
        let square = oriented as usize ^ if mirrored { 7 } else { 0 };
        bucket * FACTOR_ROWS + identity * 64 + square
    }

    /// The factor row of a piece row: the same identity and square, whatever the bucket.
    pub fn factor_row(row: usize) -> usize {
        debug_assert!(row < HALFK_ROWS, "row {row} is not a piece row");
        row % FACTOR_ROWS
    }

    /// Every active row of `perspective`'s view: its piece rows (`piece_rows`), then the threat rows
    /// of `board`, the same position rebuilt.
    pub fn active_rows(canonical: &CanonicalPosition, board: &Board, perspective: Color) -> Vec<u16> {
        let (_, mirrored) = king_bucket(oriented_square(king_square_of(canonical, perspective), perspective));
        let mut rows = piece_rows(canonical, perspective);
        psq_threats::push_threat_rows_from(board, perspective, THREAT_ROW, mirrored, &mut rows);
        rows
    }

    /// `perspective`'s piece rows: one per piece, both kings included, in its own king's bucket.
    /// What the engine keeps incrementally; the threats it adds at evaluation.
    pub fn piece_rows(canonical: &CanonicalPosition, perspective: Color) -> Vec<u16> {
        let (bucket, mirrored) = king_bucket(oriented_square(king_square_of(canonical, perspective), perspective));
        let mut rows = Vec::with_capacity(canonical.pieces.len() + 2 + 32);
        for &packed in &canonical.pieces {
            let (square, piece_type, color) = unpack_canonical_piece(packed);
            let oriented = oriented_square(square, perspective);
            rows.push(piece_row(bucket, mirrored, piece_type, color == perspective, oriented) as u16);
        }
        for color in [perspective, perspective.opposite()] {
            let king = oriented_square(king_square_of(canonical, color), perspective);
            rows.push(piece_row(bucket, mirrored, PieceType::King, color == perspective, king) as u16);
        }
        rows
    }

    /// Both perspectives' rows. The attacks need a `Board`, so the record is rebuilt into one
    /// (`board_from_canonical`), once for both.
    pub fn expand(canonical: &CanonicalPosition) -> PositionFeatures {
        let board = board_from_canonical(canonical);
        let side_to_move = canonical.side_to_move;
        PositionFeatures {
            own: active_rows(canonical, &board, side_to_move),
            opponent: active_rows(canonical, &board, side_to_move.opposite()),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use super::super::extract_canonical;

        const STARTING_POSITION_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

        fn sorted(mut rows: Vec<u16>) -> Vec<u16> {
            rows.sort_unstable();
            rows
        }

        fn features(fen: &str) -> PositionFeatures {
            let (board, _) = Board::from_fen(fen).unwrap();
            expand(&extract_canonical(&board))
        }

        fn pieces(rows: &[u16]) -> Vec<u16> {
            rows.iter().copied().filter(|&row| (row as usize) < THREAT_ROW).collect()
        }

        fn threats(rows: &[u16]) -> Vec<u16> {
            sorted(rows.iter().copied().filter(|&row| row as usize >= THREAT_ROW).collect())
        }

        #[test]
        fn the_sizes_are_stockfishs() {
            assert_eq!(HALFK_ROWS, 22_528);
            assert_eq!(ROWS, 26_624);
            assert_eq!(FACTOR_ROWS, 704);
        }

        #[test]
        fn the_buckets_cover_32_squares_each_reached_mirrored_and_not() {
            let mut seen = [[false; 2]; BUCKETS];
            for index in 0u8..64 {
                let (bucket, mirrored) = king_bucket(Square::try_from(index).unwrap());
                assert!(bucket < BUCKETS);
                seen[bucket][mirrored as usize] = true;
            }
            assert!(seen.iter().all(|both| both[0] && both[1]), "every bucket from both halves of the board");
            assert_eq!(king_bucket(Square::D1), (3, false));
            assert_eq!(king_bucket(Square::E1), (3, true));
            assert_eq!(king_bucket(Square::A8), (28, false));
            assert_eq!(king_bucket(Square::H8), (28, true));
        }

        #[test]
        fn the_starting_position_has_32_piece_rows_in_its_kings_bucket() {
            let features = features(STARTING_POSITION_FEN);
            for rows in [&features.own, &features.opponent] {
                let pieces = pieces(rows);
                assert_eq!(pieces.len(), 32);
                // Both kings on e1 once oriented: mirrored, bucket 3.
                assert!(pieces.iter().all(|&row| row as usize / FACTOR_ROWS == 3));
                assert!(threats(rows).is_empty());
            }
            assert_eq!(sorted(features.own), sorted(features.opponent), "a color-symmetric position");
        }

        #[test]
        fn rows_are_in_range_and_never_repeat() {
            let fens = [
                STARTING_POSITION_FEN,
                "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 10",
                "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
                "8/8/8/4k3/8/4K3/8/8 w - - 0 1",
            ];
            for fen in fens {
                let features = features(fen);
                for rows in [features.own, features.opponent] {
                    assert!(rows.iter().all(|&row| (row as usize) < ROWS), "{fen}: row out of range");
                    let mut unique = sorted(rows.clone());
                    unique.dedup();
                    assert_eq!(unique.len(), rows.len(), "{fen}: a row repeats");
                }
            }
        }

        #[test]
        fn rows_by_hand_unmirrored_and_mirrored() {
            // White: king a1 (bucket 0, unmirrored), knight c3. Black: king e8, oriented e1
            // (bucket 3, mirrored).
            let (board, _) = Board::from_fen("4k3/8/8/8/8/2N5/8/K7 w - - 0 1").unwrap();
            let canonical = extract_canonical(&board);
            let white = active_rows(&canonical, &board, Color::White);
            let black = active_rows(&canonical, &board, Color::Black);

            // White's own knight (identity 1) on c3 (18), bucket 0.
            assert!(white.contains(&(64 + 18)));
            // Both kings in plane 10: White's own on a1 (0), Black's on e8 (60).
            assert!(white.contains(&(10 * 64)));
            assert!(white.contains(&(10 * 64 + 60)));
            assert_eq!(white.len(), 3);

            // Black sees an opponent knight (identity 6): c3, oriented c6 (42), mirrored f6 (45).
            assert!(black.contains(&((3 * 11 + 6) * 64 + 45)));
            // White's king: a1, oriented a8 (56), mirrored h8 (63).
            assert!(black.contains(&((3 * 11 + 10) * 64 + 63)));
            // Black's own king: e1 once oriented, d1 (3) mirrored.
            assert!(black.contains(&((3 * 11 + 10) * 64 + 3)));
            assert_eq!(black.len(), 3);
        }

        #[test]
        fn a_king_on_d1_and_on_e1_share_a_bucket_with_every_piece_flipped() {
            let unmirrored = features("4k3/8/8/8/8/8/1P6/3K4 w - - 0 1");
            // Only White's view: Black's king is elsewhere on its own board, so its view differs.
            // Black's king mirrored too, e8 to d8, so every square is.
            let mirrored = features("3k4/8/8/8/8/8/6P1/4K3 w - - 0 1");
            assert_eq!(sorted(unmirrored.own), sorted(mirrored.own));
        }

        #[test]
        fn a_position_and_its_color_swapped_mirror_give_the_same_features() {
            let white = features("4k3/8/8/3n4/8/2N5/4P3/R3K3 w - - 0 1");
            let black = features("r3k3/4p3/2n5/8/3N4/8/8/4K3 b - - 0 1");
            assert_eq!(sorted(white.own), sorted(black.own));
            assert_eq!(sorted(white.opponent), sorted(black.opponent));
        }

        #[test]
        fn threat_rows_are_mirrored_with_their_king() {
            // White's king on h1 mirrors White's view: the knight on a1 attacking the pawn on c2
            // becomes h1 (7) attacking f2 (13).
            let features = features("4k3/8/8/8/8/8/2p5/N6K w - - 0 1");
            assert_eq!(threats(&features.own), vec![(THREAT_ROW + 64 * 7 + 13) as u16]);
        }

        #[test]
        fn the_factor_row_drops_the_bucket_alone() {
            for bucket in [0, 3, 31] {
                let row = piece_row(bucket, false, PieceType::Rook, false, Square::B7);
                assert_eq!(factor_row(row), (3 + 5) * 64 + Square::B7 as usize);
                assert!(factor_row(row) < FACTOR_ROWS);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STARTING_POSITION_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

    #[test]
    fn board_from_canonical_rebuilds_the_board_a_record_was_written_from() {
        use crate::movegen::legal::legal_moves;
        let mut seed = 0x2545F4914F6CDD1Du64;
        let mut positions = 0;
        for _ in 0..50 {
            let mut board = Board::from_fen(STARTING_POSITION_FEN).unwrap().0;
            for _ in 0..120 {
                let rebuilt = board_from_canonical(&extract_canonical(&board));
                // The halfmove clock is the one field a record does not keep.
                let fen = |position: &Board| -> String {
                    let fen = position.to_fen(1);
                    let fields: Vec<&str> = fen.split_whitespace().collect();
                    fields[..4].join(" ")
                };
                assert_eq!(fen(&rebuilt), fen(&board));
                assert_eq!(rebuilt.hash(), board.hash(), "{}", fen(&board));
                positions += 1;
                let moves = legal_moves(&board, board.side_to_move());
                if moves.is_empty() {
                    break;
                }
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                board.make_move(moves[(seed % moves.len() as u64) as usize]);
            }
        }
        assert!(positions > 2000, "only {positions} positions");
    }

    #[test]
    fn oriented_square_leaves_white_unchanged_and_mirrors_black() {
        assert_eq!(oriented_square(Square::E1, Color::White), Square::E1);
        assert_eq!(oriented_square(Square::E1, Color::Black), Square::E8);
        assert_eq!(oriented_square(Square::A2, Color::Black), Square::A7);
    }

    #[test]
    fn pack_canonical_piece_round_trips_every_square_type_and_color() {
        for square_index in 0u8..64 {
            let square = Square::try_from(square_index).unwrap();
            for piece_type in PieceType::ALL {
                if piece_type == PieceType::King {
                    continue;
                }
                for color in [Color::White, Color::Black] {
                    let packed = pack_canonical_piece(square, piece_type, color);
                    assert_eq!(
                        unpack_canonical_piece(packed),
                        (square, piece_type, color),
                        "square {square_index} piece_type {piece_type:?} color {color:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn starting_position_extracts_30_non_king_pieces() {
        // 32 pieces total, minus the 2 kings, which are never part of `pieces` at all -- they're
        // expressed only via `white_king`/`black_king`.
        let (board, _) = Board::from_fen(STARTING_POSITION_FEN).unwrap();
        let canonical = extract_canonical(&board);
        assert_eq!(canonical.pieces.len(), 30);
    }
}
