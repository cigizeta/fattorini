//! A V6 training record, Lc0's one per ply: 8356 bytes, little-endian, laid out as `V6TrainingData`
//! in Lc0's `trainingdata.h`:
//!
//! ```text
//!    0  u32 version, u32 input_format
//!    8  f32 probabilities[1858]                 (the policy: read by the diagnostics only)
//! 7440  u64 planes[104]                         (8 positions of 13 planes)
//! 8272  u8 castling_us_ooo, us_oo, them_ooo, them_oo
//! 8276  u8 side_to_move_or_enpassant, rule50_count, invariance_info, dummy
//! 8280  f32 root_q, best_q, root_d, best_d, root_m, best_m, plies_left
//! 8308  f32 result_q, result_d
//! 8316  f32 played_q, played_d, played_m, orig_q, orig_d, orig_m
//! 8340  u32 visits
//! 8344  u16 played_idx, best_idx
//! 8348  u64 reserved
//! ```
//!
//! With `input_format` 1, the classical input, the planes are from the side to move's view: the
//! first 13 are the current position (ours P N B R Q K, theirs P N B R Q K, repetitions), the board
//! mirrored by rank when Black is to move, and `side_to_move_or_enpassant` is 1 for Black. Lc0
//! writes each plane with the bits of every byte reversed (`ReverseBitsInBytes`), undone here.

use shakmaty::{Color, Piece, Role};

pub(super) const RECORD_BYTES: usize = 8356;

const VERSION: u32 = 6;
const INPUT_FORMAT: u32 = 1;

const PROBABILITIES: usize = 8;
const PLANES: usize = 7440;
const CASTLING: usize = 8272;
const SIDE_TO_MOVE: usize = 8276;
const RULE50: usize = 8277;
const INVARIANCE: usize = 8278;
const BEST_Q: usize = 8284;
const RESULT_Q: usize = 8308;
const VISITS: usize = 8340;
const PLAYED_IDX: usize = 8344;
const BEST_IDX: usize = 8346;

/// A board's 64 squares, a1 first, as the real board (not the side to move's view).
pub(super) type Squares = [Option<Piece>; 64];

const ROLES: [Role; 6] = [Role::Pawn, Role::Knight, Role::Bishop, Role::Rook, Role::Queen, Role::King];

pub(super) struct Record<'a> {
    bytes: &'a [u8],
}

/// A game's records, each checked for the version and input format this converter decodes.
pub(super) fn records(game: &[u8]) -> Result<Vec<Record<'_>>, String> {
    if game.is_empty() || !game.len().is_multiple_of(RECORD_BYTES) {
        return Err(format!("{} bytes, not a positive multiple of a V6 record's {RECORD_BYTES}", game.len()));
    }
    let records: Vec<Record> = game.as_chunks::<RECORD_BYTES>().0.iter().map(|bytes| Record { bytes }).collect();
    for (ply, record) in records.iter().enumerate() {
        let (version, input_format) = (record.u32_at(0), record.u32_at(4));
        if (version, input_format) != (VERSION, INPUT_FORMAT) {
            return Err(format!(
                "ply {ply}: version {version} and input format {input_format}, but only {VERSION} and {INPUT_FORMAT} are decoded"
            ));
        }
    }
    Ok(records)
}

impl Record<'_> {
    fn u32_at(&self, offset: usize) -> u32 {
        u32::from_le_bytes(self.bytes[offset..offset + 4].try_into().expect("four bytes"))
    }

    fn f32_at(&self, offset: usize) -> f32 {
        f32::from_le_bytes(self.bytes[offset..offset + 4].try_into().expect("four bytes"))
    }

    fn u16_at(&self, offset: usize) -> u16 {
        u16::from_le_bytes(self.bytes[offset..offset + 2].try_into().expect("two bytes"))
    }

    /// Plane `index`, a1 as bit 0, from the side to move's view.
    fn plane(&self, index: usize) -> u64 {
        let offset = PLANES + 8 * index;
        let stored = u64::from_le_bytes(self.bytes[offset..offset + 8].try_into().expect("eight bytes"));
        reverse_bits_in_bytes(stored)
    }

    pub(super) fn side_to_move(&self) -> Color {
        if self.bytes[SIDE_TO_MOVE] == 1 { Color::Black } else { Color::White }
    }

    /// A square of the side to move's view on the real board.
    pub(super) fn real_square(&self, square: u8) -> u8 {
        if self.side_to_move() == Color::Black { square ^ 56 } else { square }
    }

    pub(super) fn invariance_info(&self) -> u8 {
        self.bytes[INVARIANCE]
    }

    /// The value under the search's best move, W - L in [-1, 1], for the side to move.
    pub(super) fn best_q(&self) -> f32 {
        self.f32_at(BEST_Q)
    }

    /// The game's result for the side to move: 1, 0 or -1.
    pub(super) fn result_q(&self) -> f32 {
        self.f32_at(RESULT_Q)
    }

    /// The policy target's entry for move `index` of Lc0's policy index (`moves.rs`): its share of
    /// the search, or -1 for an illegal move.
    pub(super) fn probability(&self, index: usize) -> f32 {
        self.f32_at(PROBABILITIES + 4 * index)
    }

    /// The search's visits, as the record holds them.
    pub(super) fn visits(&self) -> u32 {
        self.u32_at(VISITS)
    }

    pub(super) fn played_idx(&self) -> u16 {
        self.u16_at(PLAYED_IDX)
    }

    pub(super) fn best_idx(&self) -> u16 {
        self.u16_at(BEST_IDX)
    }

    /// The current position's pieces on the real board.
    pub(super) fn squares(&self) -> Result<Squares, String> {
        let us = self.side_to_move();
        let mut squares: Squares = [None; 64];
        for plane in 0..12 {
            let color = if plane < 6 { us } else { other(us) };
            let role = ROLES[plane % 6];
            let mut mask = self.plane(plane);
            while mask != 0 {
                let square = self.real_square(mask.trailing_zeros() as u8) as usize;
                mask &= mask - 1;
                if squares[square].is_some() {
                    return Err(format!("square {square} holds two pieces"));
                }
                squares[square] = Some(Piece { color, role });
            }
        }
        Ok(squares)
    }

    /// The current position as a FEN: the pieces, the side to move, the castling rights, the
    /// en-passant square read from the previous position's planes, the rule-50 count, move 1.
    pub(super) fn fen(&self) -> Result<String, String> {
        let squares = self.squares()?;
        let mut placement = String::new();
        for rank in (0..8).rev() {
            let mut empty = 0;
            for file in 0..8 {
                match squares[rank * 8 + file] {
                    None => empty += 1,
                    Some(piece) => {
                        if empty > 0 {
                            placement.push_str(&empty.to_string());
                            empty = 0;
                        }
                        placement.push(piece.char());
                    }
                }
            }
            if empty > 0 {
                placement.push_str(&empty.to_string());
            }
            if rank > 0 {
                placement.push('/');
            }
        }
        let side = if self.side_to_move() == Color::White { "w" } else { "b" };
        let mut castling: String = self
            .castling()
            .iter()
            .zip(['K', 'Q', 'k', 'q'])
            .filter(|(allowed, _)| **allowed)
            .map(|(_, letter)| letter)
            .collect();
        if castling.is_empty() {
            castling.push('-');
        }
        let en_passant = match self.en_passant_square() {
            Some(square) => {
                let real = self.real_square(square);
                format!("{}{}", (b'a' + real % 8) as char, real / 8 + 1)
            }
            None => "-".to_string(),
        };
        Ok(format!("{placement} {side} {castling} {en_passant} {} 1", self.bytes[RULE50]))
    }

    /// The castling rights, White's short and long, then Black's.
    pub(super) fn castling(&self) -> [bool; 4] {
        let [us_ooo, us_oo, them_ooo, them_oo] = [0, 1, 2, 3].map(|index| self.bytes[CASTLING + index] != 0);
        if self.side_to_move() == Color::White {
            [us_oo, us_ooo, them_oo, them_ooo]
        } else {
            [them_oo, them_ooo, us_oo, us_ooo]
        }
    }

    /// From the side to move's view: the square behind an opponent pawn that has just moved two
    /// squares, when one of ours stands beside it to take it. The classical input has no en-passant
    /// field, so the move is read from the difference between the current planes and the previous
    /// position's (planes 13 to 25), which are zero when the record has no history.
    fn en_passant_square(&self) -> Option<u8> {
        let (our_pawns, their_pawns, their_pawns_before) = (self.plane(0), self.plane(6), self.plane(13 + 6));
        for file in 0..8u8 {
            let (fifth, seventh) = (1u64 << (32 + file), 1u64 << (48 + file));
            let moved_two = their_pawns & fifth != 0
                && their_pawns & seventh == 0
                && their_pawns_before & seventh != 0
                && their_pawns_before & fifth == 0;
            let beside = [file.checked_sub(1), Some(file + 1).filter(|&next| next < 8)]
                .into_iter()
                .flatten()
                .any(|next| our_pawns & (1u64 << (32 + next)) != 0);
            if moved_two && beside {
                return Some(40 + file);
            }
        }
        None
    }
}

fn other(color: Color) -> Color {
    match color {
        Color::White => Color::Black,
        Color::Black => Color::White,
    }
}

fn reverse_bits_in_bytes(value: u64) -> u64 {
    let bytes = value.to_le_bytes().map(u8::reverse_bits);
    u64::from_le_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A record with `planes` (each as the real mask from the side to move's view) and side to move.
    fn record_bytes(planes: &[(usize, u64)], black_to_move: bool, castling: [u8; 4]) -> Vec<u8> {
        let mut bytes = vec![0u8; RECORD_BYTES];
        bytes[0..4].copy_from_slice(&VERSION.to_le_bytes());
        bytes[4..8].copy_from_slice(&INPUT_FORMAT.to_le_bytes());
        for &(index, mask) in planes {
            let offset = PLANES + 8 * index;
            bytes[offset..offset + 8].copy_from_slice(&reverse_bits_in_bytes(mask).to_le_bytes());
        }
        bytes[CASTLING..CASTLING + 4].copy_from_slice(&castling);
        bytes[SIDE_TO_MOVE] = u8::from(black_to_move);
        bytes
    }

    #[test]
    fn a_record_with_black_to_move_reads_back_as_the_real_board() {
        // Black to move, so "ours" is Black and the view is mirrored by rank: our king on the view's
        // e1 is on e8, their king on the view's e8 is on e1, their pawn on the view's d7 is on d2.
        let (view_e1, view_e8, view_d7) = (4u64, 60u64, 51u64);
        let planes = [(5, 1u64 << view_e1), (6 + 5, 1u64 << view_e8), (6, 1u64 << view_d7)];
        // us_oo set: Black may castle short.
        let bytes = record_bytes(&planes, true, [0, 1, 0, 0]);
        let game = records(&bytes).unwrap();
        assert_eq!(game[0].fen().unwrap(), "4k3/8/8/8/8/8/3P4/4K3 b k - 0 1");
    }

    #[test]
    fn the_policy_and_the_visits_read_back() {
        let mut bytes = record_bytes(&[], false, [0; 4]);
        bytes[PROBABILITIES..PROBABILITIES + 4].copy_from_slice(&0.25f32.to_le_bytes());
        let last = PROBABILITIES + 4 * 1857;
        bytes[last..last + 4].copy_from_slice(&(-1.0f32).to_le_bytes());
        bytes[VISITS..VISITS + 4].copy_from_slice(&801u32.to_le_bytes());
        let game = records(&bytes).unwrap();
        assert_eq!(game[0].probability(0), 0.25);
        assert_eq!(game[0].probability(1857), -1.0);
        assert_eq!(game[0].visits(), 801);
    }

    #[test]
    fn a_record_of_another_version_is_refused() {
        let mut bytes = record_bytes(&[], false, [0; 4]);
        bytes[0] = 5;
        assert!(records(&bytes).is_err());
        assert!(records(&bytes[..100]).is_err());
    }

    #[test]
    fn reversing_the_bits_of_every_byte_twice_is_the_identity() {
        let value = 0x0123_4567_89ab_cdefu64;
        assert_eq!(reverse_bits_in_bytes(reverse_bits_in_bytes(value)), value);
        assert_eq!(reverse_bits_in_bytes(1), 0x80);
    }
}
