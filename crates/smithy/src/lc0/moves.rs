//! Lc0's policy index: the 1858 moves a V6 record's `played_idx` and `best_idx` point into, from
//! the side to move's view (a1 is square 0, ranks mirrored when Black is to move).
//!
//! The order is Lc0's own: every queen-like or knight move of an empty board, by from-square and then
//! to-square, both in a1, b1, ..., h8 order (1792 moves); then the promotions from the seventh rank to
//! the eighth, straight or diagonal, by from-square and to-square, each to a queen, a rook and a
//! bishop (66 moves). A promotion to a knight has no entry of its own: it is the plain move from the
//! seventh rank to the eighth. Every move the converter decodes is checked against the board the next
//! record shows, so a wrong order here fails on the first game.

/// The entries of Lc0's policy index.
pub(super) const MOVE_COUNT: usize = 1858;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Promotion {
    Queen,
    Rook,
    Bishop,
}

/// One entry: squares 0 to 63 from the side to move's view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct IndexedMove {
    pub(super) from: u8,
    pub(super) to: u8,
    pub(super) promotion: Option<Promotion>,
}

impl IndexedMove {
    /// The entry in UCI, from the side to move's view, as Lc0's own table spells it: `a7a8q`.
    pub(super) fn uci(&self) -> String {
        let suffix = match self.promotion {
            None => "",
            Some(Promotion::Queen) => "q",
            Some(Promotion::Rook) => "r",
            Some(Promotion::Bishop) => "b",
        };
        format!("{}{}{suffix}", square_name(self.from), square_name(self.to))
    }
}

/// The whole index, in Lc0's order.
pub(super) fn table() -> Vec<IndexedMove> {
    let mut moves = Vec::with_capacity(MOVE_COUNT);
    for from in 0..64u8 {
        for to in 0..64u8 {
            if to != from && (is_queen_move(from, to) || is_knight_move(from, to)) {
                moves.push(IndexedMove { from, to, promotion: None });
            }
        }
    }
    for from in 48..56u8 {
        for to in 56..64u8 {
            if (file(from) as i32 - file(to) as i32).abs() <= 1 {
                for promotion in [Promotion::Queen, Promotion::Rook, Promotion::Bishop] {
                    moves.push(IndexedMove { from, to, promotion: Some(promotion) });
                }
            }
        }
    }
    assert_eq!(moves.len(), MOVE_COUNT, "Lc0's policy index has {MOVE_COUNT} moves");
    moves
}

fn file(square: u8) -> u8 {
    square % 8
}

fn rank(square: u8) -> u8 {
    square / 8
}

fn square_name(square: u8) -> String {
    format!("{}{}", (b'a' + file(square)) as char, rank(square) + 1)
}

fn is_queen_move(from: u8, to: u8) -> bool {
    let file_delta = (file(from) as i32 - file(to) as i32).abs();
    let rank_delta = (rank(from) as i32 - rank(to) as i32).abs();
    file_delta == 0 || rank_delta == 0 || file_delta == rank_delta
}

fn is_knight_move(from: u8, to: u8) -> bool {
    let file_delta = (file(from) as i32 - file(to) as i32).abs();
    let rank_delta = (rank(from) as i32 - rank(to) as i32).abs();
    (file_delta == 1 && rank_delta == 2) || (file_delta == 2 && rank_delta == 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_index_has_lc0_s_size_and_no_entry_twice() {
        let moves = table();
        assert_eq!(moves.len(), MOVE_COUNT);
        let mut names: Vec<String> = moves.iter().map(IndexedMove::uci).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), MOVE_COUNT);
    }

    #[test]
    fn the_index_starts_and_ends_as_lc0_s() {
        let names: Vec<String> = table().iter().map(IndexedMove::uci).collect();
        assert_eq!(&names[..4], ["a1b1", "a1c1", "a1d1", "a1e1"]);
        assert_eq!(&names[7..13], ["a1a2", "a1b2", "a1c2", "a1a3", "a1b3", "a1c3"]);
        assert_eq!(names[1792], "a7a8q");
        assert_eq!(&names[1855..], ["h7h8q", "h7h8r", "h7h8b"]);
        assert!(names.contains(&"e2e4".to_string()));
        assert!(names.contains(&"g1f3".to_string()));
        assert_eq!(names.iter().filter(|name| name.len() == 5).count(), 66);
    }
}
