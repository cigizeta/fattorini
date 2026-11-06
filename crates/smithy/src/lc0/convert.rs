//! One Lc0 game, its records, as one PGN game in the shape the server's self-play writes: a `[FEN]` start, a
//! `[Result]`, and after every move a `{<score> <best>}` comment, the score in pawns for the side to
//! move of the position the move is played from (`game_labels.rs`'s reader turns it White-relative)
//! and the search's best move there in UCI. With `counts`, the comment goes on with Lc0's visit count
//! of every legal move, `{<score> <best> <c1>,<c2>,...}` (`visit_counts`).
//!
//! Every record's board is checked against the board the game reaches by playing the previous
//! records' moves, so a misread plane or a wrong policy index stops the conversion at once.

use std::str::FromStr;

use shakmaty::fen::Fen;
use shakmaty::san::SanPlus;
use shakmaty::{CastlingMode, Chess, Color, File, Move, Piece, Position, Role, Square};

use kalo_chess::labels::LC0;

use super::moves::{IndexedMove, Promotion, MOVE_COUNT};
use super::record::{records, Record, Squares};

pub(super) struct ConvertedGame {
    pub(super) text: String,
    /// Its training positions: one per record, and the final one.
    pub(super) positions: u64,
    /// The first record's `invariance_info`, printed once a run.
    pub(super) invariance_info: u8,
}

/// `game`, a member's gunzipped bytes, named `game_id` in its headers; `None` when it is not standard
/// chess (`is_standard_start`). With `counts`, every comment carries the visit counts too.
pub(super) fn convert_game(game_id: &str, game: &[u8], table: &[IndexedMove], counts: bool) -> Result<Option<ConvertedGame>, String> {
    let records = records(game)?;
    let Some(start) = standard_start(&records)? else {
        return Ok(None);
    };
    let fen_text = records[0].fen()?;
    let result = result_token(&records)?;

    let mut text = String::new();
    for (name, value) in [
        ("Event", "lc0 test91"),
        ("Round", game_id),
        ("White", "lc0"),
        ("Black", "lc0"),
        ("Result", result),
        ("FEN", &fen_text),
        ("SetUp", "1"),
    ] {
        text.push_str(&format!("[{name} \"{value}\"]\n"));
    }
    text.push('\n');

    replay(&records, start, table, |ply, record, position, played| {
        let best = decode(table, record.best_idx(), record, position).map_err(|message| format!("ply {ply}: best {message}"))?;
        if position.turn() == Color::White {
            text.push_str(&format!("{}. ", position.fullmoves()));
        } else if ply == 0 {
            text.push_str(&format!("{}... ", position.fullmoves()));
        }
        let san = SanPlus::from_move(position.clone(), *played);
        // Through Lc0's own curve, so the file's centipawns are Lc0's and read back to its Q through
        // `labels::LC0`. A corpus converted before was rewritten into it (`rewrite.rs`).
        let pawns = LC0.pawns(record.best_q() as f64);
        text.push_str(&format!("{san} {{{pawns:.2} {}", best.to_uci(CastlingMode::Standard)));
        if counts {
            let counts = visit_counts(table, record, position).map_err(|message| format!("ply {ply}: {message}"))?;
            let counts: Vec<String> = counts.iter().map(u32::to_string).collect();
            text.push_str(&format!(" {}", counts.join(",")));
        }
        text.push_str("} ");
        Ok(())
    })?;
    text.push_str(result);
    text.push_str("\n\n");
    Ok(Some(ConvertedGame { text, positions: records.len() as u64 + 1, invariance_info: records[0].invariance_info() }))
}

/// Lc0's visit count of every legal move at `position`, the moves sorted by their UCI text: an order
/// independent of any move generator, so a change to Kalo Chess's never reorders a file already written,
/// and no move named, the position already saying which they are. The record's policy target holds
/// each move's share of the root's visits but the first, so a share times `visits - 1` is its count,
/// exactly; one that is not a whole count is an error, as is an entry for a move not legal or a legal
/// move without one.
pub(super) fn visit_counts(table: &[IndexedMove], record: &Record, position: &Chess) -> Result<Vec<u32>, String> {
    let visits = record.visits();
    if visits < 2 {
        return Err(format!("{visits} visits leave no counts"));
    }
    let children = (visits - 1) as f64;
    let mut counted = Vec::new();
    for index in 0..MOVE_COUNT {
        let share = record.probability(index);
        if share < 0.0 {
            continue;
        }
        let uci = decode(table, index as u16, record, position)?.to_uci(CastlingMode::Standard).to_string();
        let scaled = share as f64 * children;
        if (scaled - scaled.round()).abs() > 0.01 {
            return Err(format!("{uci}'s share {share} is not a whole count of {} visits", visits - 1));
        }
        counted.push((uci, scaled.round() as u32));
    }
    counted.sort();
    let entries = counted.len();
    counted.dedup_by(|a, b| a.0 == b.0);
    let legal = position.legal_moves().len();
    if counted.len() != entries || entries != legal {
        return Err(format!("{entries} policy entries for {legal} legal moves at {}", fen_of(position)));
    }
    Ok(counted.into_iter().map(|(_, count)| count).collect())
}

/// The position a game's first record holds; `None` when it is not standard chess
/// (`is_standard_start`).
pub(super) fn standard_start(records: &[Record]) -> Result<Option<Chess>, String> {
    let first = &records[0];
    if !is_standard_start(&first.squares()?, first.castling()) {
        return Ok(None);
    }
    let fen_text = first.fen()?;
    let fen = Fen::from_str(&fen_text).map_err(|error| format!("start position {fen_text:?}: {error}"))?;
    let position = fen.into_position(CastlingMode::Standard).map_err(|error| format!("start position {fen_text:?} is illegal: {error}"))?;
    Ok(Some(position))
}

/// Plays `records` from `position`, every record's board checked against the board the previous
/// records' moves reach; `visit` sees each record with its ply, the position it was recorded at and
/// the move played from it.
pub(super) fn replay(
    records: &[Record], mut position: Chess, table: &[IndexedMove], mut visit: impl FnMut(usize, &Record, &Chess, &Move) -> Result<(), String>,
) -> Result<(), String> {
    for (ply, record) in records.iter().enumerate() {
        if record.side_to_move() != position.turn() || record.squares()? != squares_of(&position) {
            return Err(format!("ply {ply}: the record's board is not the one the game's moves reach, {}", fen_of(&position)));
        }
        let played = decode(table, record.played_idx(), record, &position).map_err(|message| format!("ply {ply}: played {message}"))?;
        visit(ply, record, &position, &played)?;
        position = position.play(played).map_err(|error| format!("ply {ply}: a legal move was refused: {error}"))?;
    }
    Ok(())
}

/// Whether a game starting from `squares` with `castling` (White's short and long, then Black's) is
/// standard chess, not Chess960 or Chess324, which test91's book also opens with (the developer,
/// 2026-10-17: standard chess only, for now). Two checks:
///
/// - every castling right has its king on e1 or e8 and its rook in its corner, which Chess960 breaks;
/// - every knight, bishop or queen on a side's back rank stands where the standard setup has that same
///   piece, which Chess324 breaks (its king and rooks stay home, the others are shuffled).
///
/// A standard game a few book plies in passes: the pieces that left the back rank are no longer
/// there, and castling moves only a king and a rook. A standard game that brought a piece back to
/// another's home square is skipped too, which costs a game, never a wrong position.
fn is_standard_start(squares: &Squares, castling: [bool; 4]) -> bool {
    const HOME: [Role; 8] = [Role::Rook, Role::Knight, Role::Bishop, Role::Queen, Role::King, Role::Bishop, Role::Knight, Role::Rook];
    let holds = |square: usize, color: Color, role: Role| squares[square] == Some(Piece { color, role });
    let rights = [(4, 7, Color::White), (4, 0, Color::White), (60, 63, Color::Black), (60, 56, Color::Black)];
    for (&allowed, &(king, rook, color)) in castling.iter().zip(&rights) {
        if allowed && !(holds(king, color, Role::King) && holds(rook, color, Role::Rook)) {
            return false;
        }
    }
    for (color, back_rank) in [(Color::White, 0), (Color::Black, 56)] {
        for (file, &home) in HOME.iter().enumerate() {
            if let Some(piece) = squares[back_rank + file]
                && piece.color == color
                && matches!(piece.role, Role::Knight | Role::Bishop | Role::Queen)
                && piece.role != home
            {
                return false;
            }
        }
    }
    true
}

/// The game's result for White, from the first record's `result_q`, which every record must agree
/// with from its own side to move.
fn result_token(records: &[Record]) -> Result<&'static str, String> {
    let mut white_result = None;
    for (ply, record) in records.iter().enumerate() {
        let result = record.result_q();
        let rounded = result.round();
        if (result - rounded).abs() > 1e-6 || rounded.abs() > 1.0 {
            return Err(format!("ply {ply}: result {result} is not 1, 0 or -1"));
        }
        let for_white = if record.side_to_move() == Color::White { rounded } else { -rounded };
        if *white_result.get_or_insert(for_white) != for_white {
            return Err(format!("ply {ply}: the result disagrees with the first record's"));
        }
    }
    Ok(match white_result.expect("a game has a record") as i32 {
        1 => "1-0",
        -1 => "0-1",
        _ => "1/2-1/2",
    })
}

/// The legal move policy index `index` names in `position`. The index is from the side to move's
/// view. A plain move from the seventh rank to the eighth by a pawn is a knight promotion; a castling
/// may be named by the king's target square or by its rook's.
pub(super) fn decode(table: &[IndexedMove], index: u16, record: &Record, position: &Chess) -> Result<Move, String> {
    let entry = table.get(index as usize).ok_or_else(|| format!("index {index} is past the policy's {}", table.len()))?;
    let from = Square::new(record.real_square(entry.from) as u32);
    let to = Square::new(record.real_square(entry.to) as u32);
    let promotion = entry.promotion.map(|promotion| match promotion {
        Promotion::Queen => Role::Queen,
        Promotion::Rook => Role::Rook,
        Promotion::Bishop => Role::Bishop,
    });
    for candidate in position.legal_moves() {
        let matches = match &candidate {
            Move::Normal { from: move_from, to: move_to, promotion: move_promotion, .. } => {
                *move_from == from
                    && *move_to == to
                    && (*move_promotion == promotion || (promotion.is_none() && *move_promotion == Some(Role::Knight)))
            }
            Move::EnPassant { from: move_from, to: move_to } => *move_from == from && *move_to == to && promotion.is_none(),
            Move::Castle { king, rook } => {
                let target_file = if rook.file() as u32 > king.file() as u32 { File::G } else { File::C };
                *king == from && (*rook == to || Square::from_coords(target_file, king.rank()) == to) && promotion.is_none()
            }
            Move::Put { .. } => false,
        };
        if matches {
            return Ok(candidate);
        }
    }
    Err(format!("move {index} ({} in the side to move's view) is not legal at {}", entry.uci(), fen_of(position)))
}

fn squares_of(position: &Chess) -> Squares {
    let mut squares: Squares = [None; 64];
    for (index, square) in squares.iter_mut().enumerate() {
        *square = position.board().piece_at(Square::new(index as u32));
    }
    squares
}

/// The board and side to move, for error messages.
fn fen_of(position: &Chess) -> String {
    let side = if position.turn() == Color::White { "w" } else { "b" };
    format!("{} {side}", position.board())
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::super::moves::table;
    use super::*;

    /// The first game of `training-run2-test91-20261017-0417.tar`, as Lc0 published it
    /// (`tests/data/LC0-LICENSE.txt`).
    const REAL_GAME: &[u8] = include_bytes!("../../tests/data/lc0-test91-229828019.gz");

    fn real_game() -> Vec<u8> {
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(REAL_GAME).read_to_end(&mut bytes).expect("the test game gunzips");
        bytes
    }

    #[test]
    fn a_real_lc0_game_converts_with_every_board_checked() {
        let converted =
            convert_game("229828019", &real_game(), &table(), false).unwrap_or_else(|message| panic!("{message}")).expect("a standard game");
        assert_eq!(converted.positions, 70, "69 records and the final position");
        assert!(converted.text.starts_with("[Event \"lc0 test91\"]\n[Round \"229828019\"]\n"));
        assert_eq!(converted.text.matches('{').count(), 69, "one comment a move");
    }

    #[test]
    fn a_real_game_s_visit_counts_cover_its_legal_moves_and_its_visits() {
        let game = real_game();
        let records = records(&game).unwrap();
        let start = standard_start(&records).unwrap().expect("a standard game");
        let table = table();
        replay(&records, start, &table, |ply, record, position, _| {
            let counts = visit_counts(&table, record, position)?;
            assert_eq!(counts.len(), position.legal_moves().len(), "ply {ply}: one count a legal move");
            assert_eq!(counts.iter().map(|&count| count as u64).sum::<u64>(), record.visits() as u64 - 1, "ply {ply}: every visit but the root's");
            Ok(())
        })
        .unwrap_or_else(|message| panic!("{message}"));
    }

    #[test]
    fn with_counts_every_comment_carries_them() {
        let converted = convert_game("229828019", &real_game(), &table(), true).unwrap_or_else(|message| panic!("{message}")).expect("a standard game");
        let comments: Vec<&str> = converted.text.split('{').skip(1).map(|rest| rest.split('}').next().unwrap()).collect();
        assert_eq!(comments.len(), 69);
        for comment in comments {
            let tokens: Vec<&str> = comment.split(' ').collect();
            assert_eq!(tokens.len(), 3, "score, best move, counts: {comment:?}");
            assert!(tokens[2].split(',').all(|count| count.parse::<u32>().is_ok()), "{comment:?}");
        }
    }

    #[test]
    fn a_converted_game_reads_back_as_a_self_play_game() {
        use super::super::super::game_labels::{self_play_game_labels, self_play_game_positions, SelfPlaySegmentReader};

        let converted = convert_game("229828019", &real_game(), &table(), false).unwrap().expect("a standard game");
        let dir = std::env::temp_dir().join(format!("smithy-lc0-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("game.pgn.zst").to_str().unwrap().to_string();
        std::fs::write(&path, zstd::encode_all(converted.text.as_bytes(), 3).unwrap()).unwrap();

        let mut reader = SelfPlaySegmentReader::open(&path);
        let (index, collected) = reader.next_game(&path).expect("one game");
        assert_eq!(self_play_game_positions(&collected, index).len(), 70);
        assert_eq!(self_play_game_labels(&collected, 0.98, index, &kalo_chess::labels::Curve::Atan(*LC0)).len(), 70);
        assert!(reader.next_game(&path).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A FEN's piece placement as squares, a1 first.
    fn squares(placement: &str) -> Squares {
        let mut squares: Squares = [None; 64];
        for (row, rank_text) in placement.split('/').enumerate() {
            let mut file = 0;
            for character in rank_text.chars() {
                if let Some(empty) = character.to_digit(10) {
                    file += empty as usize;
                } else {
                    let color = if character.is_ascii_uppercase() { Color::White } else { Color::Black };
                    let role = Role::from_char(character.to_ascii_lowercase()).expect("a piece letter");
                    squares[(7 - row) * 8 + file] = Some(Piece { color, role });
                    file += 1;
                }
            }
        }
        squares
    }

    #[test]
    fn only_standard_chess_starts_are_kept() {
        let all = [true; 4];
        let standard = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR";
        assert!(is_standard_start(&squares(standard), all));
        // After 1. e4 e5 2. Nf3 Nc6 3. Bc4 Bc5 4. O-O: the king and rook moved, the back rank still fits.
        assert!(is_standard_start(&squares("r1bqk1nr/pppp1ppp/2n5/2b1p3/2B1P3/5N2/PPPP1PPP/RNBQ1RK1"), [false, false, true, true]));
        // The Chess960 start that stopped the first run: king on f1 with both rights.
        assert!(!is_standard_start(&squares("rbbqnknr/pppppppp/8/8/8/8/PPPPPPPP/RBBQNKNR"), all));
        // Chess324: king and rooks home, so castling is fine, but the minor pieces and queen shuffled.
        assert!(!is_standard_start(&squares("rbnqknbr/pppppppp/8/8/8/8/PPPPPPPP/RBNQKNBR"), all));
    }

    #[test]
    fn a_game_cut_inside_a_record_is_refused() {
        let game = real_game();
        assert!(convert_game("x", &game[..game.len() - 1], &table(), false).is_err());
    }
}
