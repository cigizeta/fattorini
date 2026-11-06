//! Stockfish's staged move picker (`movepick.cpp`), filled with Kalo Chess's tables: the TT move, good
//! captures, the two killers, good quiets, losing captures, bad quiets; in check, every evasion
//! sorted. Each move is scored once per node, not rescanned at every pick.
//!
//! Chosen by duels against Kalo Chess's previous order (a chain that rescanned the remaining moves at
//! every pick): about +115 Elo at 10s+0.1. The killers stay, unlike Stockfish's: a duel with and
//! without them was even at depth 9, and the developer kept them.
//!
//! Generation is staged, as Stockfish's (2026-10-12, for labelling speed): the captures are
//! generated when the picker reaches them and the quiets only when it reaches those, so a node cut
//! off by its TT move or a good capture never generates, scores or sorts a quiet. The TT move and
//! the killers are tried before their moves exist, so each is first checked by `is_pseudo_legal`.

use super::{captured_piece, combined_history_score, is_quiet, see_ge, Board, Move, Piece, Square};
use crate::eval::piece_value;
use crate::movegen::{is_pseudo_legal, noisy_moves, pseudo_legal_moves, quiet_moves};

/// A capture's score: Stockfish's `7 * PieceValue[captured]`, Kalo Chess having no capture history, plus
/// the promoted piece's value for a promotion. Noisy moves are captures and promotions.
fn capture_score(board: &Board, mv: Move) -> i32 {
    let victim = captured_piece(board, mv).map_or(0, |victim| piece_value(victim.piece_type()));
    let promotion = mv.promotion().map_or(0, piece_value);
    7 * victim + promotion
}

/// Evasions put every capture first, by victim, as Stockfish's `2^28` offset does.
const EVASION_CAPTURE_OFFSET: i32 = 1 << 28;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Tt,
    CaptureInit,
    GoodCapture,
    Killer(usize),
    QuietInit,
    GoodQuiet,
    BadCapture,
    BadQuiet,
    EvasionInit,
    Evasion,
    Done,
}

/// One node's picker over `moves[start..end]`, with `scores` the same indices' scores, the moves
/// appended to the search's shared buffer as each stage needs them (children truncate it back, so
/// it ends at `end` whenever the picker runs). Noisy moves are `[start, capture_end)`, quiets
/// `[capture_end, end)`; a losing capture is moved down to `[start, bad_end)`, over slots already
/// picked, as Stockfish's `endBadCaptures`.
pub(super) struct MovePicker {
    stage: Stage,
    start: usize,
    end: usize,
    capture_end: usize,
    /// The next capture to look at, during `GoodCapture`; the next losing one, during `BadCapture`.
    cursor: usize,
    bad_end: usize,
    /// The next quiet to look at, in `GoodQuiet` and then `BadQuiet`.
    quiet_cursor: usize,
    tt_move: Option<Move>,
    killers: [Option<Move>; 2],
    /// Killers already returned, so the quiet stages skip them.
    returned_killers: [Option<Move>; 2],
    good_quiet_threshold: i32,
    /// No killers in check: every evasion is sorted together.
    use_killers: bool,
    /// Set by late-move pruning: no more quiets, but the losing captures are still searched.
    pub skip_quiets: bool,
    /// The stage after `Tt`: the captures, or the evasions in check.
    after_tt: Stage,
}

impl MovePicker {
    /// A picker whose moves will start at `start`, the shared buffer's length when the node begins.
    pub fn new(
        start: usize, tt_move: Option<Move>, killers: [Option<Move>; 2], good_quiet_threshold: i32, in_check: bool,
    ) -> MovePicker {
        MovePicker {
            stage: Stage::Tt,
            start,
            end: start,
            capture_end: start,
            cursor: start,
            bad_end: start,
            quiet_cursor: start,
            tt_move,
            killers,
            returned_killers: [None, None],
            good_quiet_threshold,
            use_killers: !in_check,
            skip_quiets: false,
            after_tt: if in_check { Stage::EvasionInit } else { Stage::CaptureInit },
        }
    }

    /// Whether the picker went past the TT move to generate the other moves: where a move head,
    /// tried right after the TT move, would have to score them.
    #[cfg(feature = "node-stats")]
    pub fn went_past_tt_move(&self) -> bool {
        return self.stage != Stage::Tt && self.stage != self.after_tt;
    }

    /// Whether a quiet stage must pass over `mv`: the TT move or a killer already returned.
    fn already_returned(&self, mv: Move) -> bool {
        Some(mv) == self.tt_move || self.returned_killers.contains(&Some(mv))
    }

    /// Appends one stage's moves to `moves` with `generate`, and grows `scores` to match.
    fn generate(
        &mut self, moves: &mut Vec<Move>, scores: &mut Vec<i32>, board: &Board,
        generate: fn(&Board, crate::board::color::Color, &mut Vec<Move>),
    ) {
        debug_assert_eq!(moves.len(), self.end, "a child left the shared move buffer longer than it found it");
        generate(board, board.side_to_move(), moves);
        self.end = moves.len();
        if scores.len() < self.end {
            scores.resize(self.end, 0);
        }
    }

    /// The next move to search, or `None` once every move has been given.
    pub fn next(
        &mut self, moves: &mut Vec<Move>, scores: &mut Vec<i32>, board: &Board, history: &[[i32; 64]; 12],
        continuation_history: &[i32], prev_move: Option<(Piece, Square)>,
    ) -> Option<Move> {
        loop {
            match self.stage {
                Stage::Tt => {
                    self.stage = self.after_tt;
                    if let Some(tt_move) = self.tt_move
                        && is_pseudo_legal(board, board.side_to_move(), tt_move)
                    {
                        return Some(tt_move);
                    }
                }
                Stage::CaptureInit => {
                    self.generate(moves, scores, board, noisy_moves);
                    self.capture_end = self.end;
                    for i in self.start..self.capture_end {
                        scores[i] = capture_score(board, moves[i]);
                    }
                    sort_descending(moves, scores, self.start, self.capture_end);
                    self.cursor = self.start;
                    self.bad_end = self.start;
                    self.stage = Stage::GoodCapture;
                }
                Stage::GoodCapture => {
                    while self.cursor < self.capture_end {
                        let mv = moves[self.cursor];
                        let score = scores[self.cursor];
                        self.cursor += 1;
                        if Some(mv) == self.tt_move {
                            continue;
                        }
                        // Stockfish's `see_ge(m, -value / 18)`.
                        if see_ge(board, mv, -score / 18) {
                            return Some(mv);
                        }
                        moves[self.bad_end] = mv;
                        scores[self.bad_end] = score;
                        self.bad_end += 1;
                    }
                    self.stage = Stage::Killer(0);
                }
                Stage::Killer(slot) => {
                    self.stage = if slot == 0 { Stage::Killer(1) } else { Stage::QuietInit };
                    if !self.use_killers || self.skip_quiets {
                        continue;
                    }
                    let Some(killer) = self.killers[slot] else { continue };
                    if self.already_returned(killer)
                        || !is_quiet(board, killer)
                        || !is_pseudo_legal(board, board.side_to_move(), killer)
                    {
                        continue;
                    }
                    self.returned_killers[slot] = Some(killer);
                    return Some(killer);
                }
                Stage::QuietInit => {
                    if !self.skip_quiets {
                        self.generate(moves, scores, board, quiet_moves);
                        for i in self.capture_end..self.end {
                            scores[i] = combined_history_score(history, continuation_history, prev_move, board, moves[i]);
                        }
                        sort_descending(moves, scores, self.capture_end, self.end);
                    }
                    self.quiet_cursor = self.capture_end;
                    self.stage = Stage::GoodQuiet;
                }
                Stage::GoodQuiet => {
                    while !self.skip_quiets && self.quiet_cursor < self.end {
                        let mv = moves[self.quiet_cursor];
                        if scores[self.quiet_cursor] <= self.good_quiet_threshold {
                            break;
                        }
                        self.quiet_cursor += 1;
                        if self.already_returned(mv) {
                            continue;
                        }
                        return Some(mv);
                    }
                    self.cursor = self.start;
                    self.stage = Stage::BadCapture;
                }
                Stage::BadCapture => {
                    if self.cursor < self.bad_end {
                        let mv = moves[self.cursor];
                        self.cursor += 1;
                        return Some(mv);
                    }
                    self.stage = Stage::BadQuiet;
                }
                Stage::BadQuiet => {
                    while !self.skip_quiets && self.quiet_cursor < self.end {
                        let mv = moves[self.quiet_cursor];
                        self.quiet_cursor += 1;
                        if self.already_returned(mv) {
                            continue;
                        }
                        return Some(mv);
                    }
                    self.stage = Stage::Done;
                }
                Stage::EvasionInit => {
                    self.generate(moves, scores, board, pseudo_legal_moves);
                    for i in self.start..self.end {
                        let mv = moves[i];
                        scores[i] = if is_quiet(board, mv) {
                            combined_history_score(history, continuation_history, prev_move, board, mv)
                        } else {
                            EVASION_CAPTURE_OFFSET + capture_score(board, mv)
                        };
                    }
                    sort_descending(moves, scores, self.start, self.end);
                    self.cursor = self.start;
                    self.stage = Stage::Evasion;
                }
                Stage::Evasion => {
                    while self.cursor < self.end {
                        let mv = moves[self.cursor];
                        self.cursor += 1;
                        if Some(mv) != self.tt_move {
                            return Some(mv);
                        }
                    }
                    self.stage = Stage::Done;
                }
                Stage::Done => return None,
            }
        }
    }
}

/// Insertion sort of `moves[from..until]` by `scores`, highest first, keeping the two in step:
/// Stockfish's `partial_insertion_sort` with no limit. Stable, so equal scores keep generation order.
fn sort_descending(moves: &mut [Move], scores: &mut [i32], from: usize, until: usize) {
    for i in (from + 1)..until {
        let mv = moves[i];
        let score = scores[i];
        let mut j = i;
        while j > from && scores[j - 1] < score {
            moves[j] = moves[j - 1];
            scores[j] = scores[j - 1];
            j -= 1;
        }
        moves[j] = mv;
        scores[j] = score;
    }
}

#[cfg(test)]
mod tests {
    use super::super::CONTINUATION_HISTORY_LEN;
    use super::*;
    use crate::board::color::Color;
    use crate::board::piece_type::PieceType;

    /// White to move: `Nxg5` wins a pawn (good), `Qxd6` loses the queen to `c7` (losing), and the
    /// rest are quiets.
    const FEN: &str = "4k3/2p5/3p4/6p1/8/5N2/8/3QK3 w - - 0 1";

    fn mv(text: &str, board: &Board) -> Move {
        let legal = crate::movegen::legal::legal_moves(board, board.side_to_move());
        crate::uci::parse_move(text, &legal).unwrap()
    }

    /// Every move the picker gives, in order, with `history` and the options given; `skip_after`
    /// sets `skip_quiets` once that many moves were given.
    fn picked(
        board: &Board, history: &[[i32; 64]; 12], tt_move: Option<Move>, killers: [Option<Move>; 2], threshold: i32,
        skip_after: Option<usize>,
    ) -> Vec<Move> {
        let mut moves = Vec::new();
        let mut scores = Vec::new();
        let continuation_history = vec![0; CONTINUATION_HISTORY_LEN];
        let in_check = crate::movegen::attacks::is_in_check(board, board.side_to_move());
        let mut picker = MovePicker::new(0, tt_move, killers, threshold, in_check);
        let mut given = Vec::new();
        while let Some(mv) = picker.next(&mut moves, &mut scores, board, history, &continuation_history, None) {
            given.push(mv);
            if Some(given.len()) == skip_after {
                picker.skip_quiets = true;
            }
        }
        given
    }

    #[test]
    fn every_pseudo_legal_move_is_given_exactly_once() {
        let (board, _) = Board::from_fen(FEN).unwrap();
        let tt_move = Some(mv("d1d3", &board));
        let given = picked(&board, &[[0; 64]; 12], tt_move, [Some(mv("e1f2", &board)), None], 0, None);
        let mut all = Vec::new();
        pseudo_legal_moves(&board, board.side_to_move(), &mut all);
        assert_eq!(given.len(), all.len(), "{given:?}");
        for m in all {
            assert_eq!(given.iter().filter(|&&g| g == m).count(), 1, "{m:?}");
        }
        assert_eq!(given[0], mv("d1d3", &board), "the TT move first");
        assert_eq!(given[2], mv("e1f2", &board), "the killer right after the good capture");
    }

    #[test]
    fn a_tt_move_or_killer_not_playable_here_is_never_given() {
        // From another position sharing the hash: a move from an empty square, and a killer that
        // is a capture here (killers are quiet moves). Every real move still comes exactly once.
        let (board, _) = Board::from_fen(FEN).unwrap();
        let from_nowhere = Move::new(Square::A1, Square::A2, None);
        let capture_as_killer = mv("f3g5", &board);
        let given = picked(&board, &[[0; 64]; 12], Some(from_nowhere), [Some(capture_as_killer), Some(from_nowhere)], 0, None);
        let mut all = Vec::new();
        pseudo_legal_moves(&board, board.side_to_move(), &mut all);
        assert!(!given.contains(&from_nowhere));
        assert_eq!(given.len(), all.len(), "{given:?}");
        for m in all {
            assert_eq!(given.iter().filter(|&&g| g == m).count(), 1, "{m:?}");
        }
    }

    #[test]
    fn a_losing_capture_comes_after_the_good_quiets_and_before_the_bad_ones() {
        let (board, _) = Board::from_fen(FEN).unwrap();
        let good_capture = mv("f3g5", &board);
        let losing_capture = mv("d1d6", &board);
        let liked_quiet = mv("d1d3", &board);
        let mut history = [[0; 64]; 12];
        history[Piece::new(Color::White, PieceType::Queen) as usize][liked_quiet.to() as usize] = 100;

        let given = picked(&board, &history, None, [None, None], 0, None);
        assert_eq!(&given[..3], &[good_capture, liked_quiet, losing_capture], "threshold 0: zeros are bad quiets");

        let given = picked(&board, &history, None, [None, None], -128, None);
        assert_eq!(&given[..2], &[good_capture, liked_quiet]);
        assert_eq!(*given.last().unwrap(), losing_capture, "threshold -128: zeros are good quiets");
    }

    #[test]
    fn skipping_quiets_still_gives_the_losing_captures() {
        let (board, _) = Board::from_fen(FEN).unwrap();
        let given = picked(&board, &[[0; 64]; 12], None, [None, None], -128, Some(2));
        assert_eq!(given.len(), 3, "{given:?}");
        assert_eq!(given[0], mv("f3g5", &board));
        assert!(is_quiet(&board, given[1]));
        assert_eq!(given[2], mv("d1d6", &board));
    }
}
