//! The book's positions: every position along every line of Lichess's chess-openings files
//! (`crates/smithy/openings/`, a CC0 snapshot embedded in the binary), the lines' prefixes
//! included, with the moves leading out of each. A position reached by several move orders is
//! one position, by its Polyglot key.

use std::collections::{HashMap, HashSet};

use fattorini::board::color::Color;
use fattorini::board::Board;
use fattorini::polyglot::{polyglot_key, polyglot_move};
use pgn_replay::{try_resolve_san, STARTING_POSITION_FEN};
use shakmaty::san::SanPlus;

/// The snapshot's five volumes, in order. Each is a TSV with a header line and the columns
/// `eco`, `name`, `pgn`.
const OPENINGS: [&str; 5] = [
    include_str!("../../openings/a.tsv"),
    include_str!("../../openings/b.tsv"),
    include_str!("../../openings/c.tsv"),
    include_str!("../../openings/d.tsv"),
    include_str!("../../openings/e.tsv"),
];

/// One book move: from the position `parent`, where `mover` is to move, `mv` (Polyglot's
/// encoding) leads to the position `child`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edge {
    pub parent: u64,
    pub mover: Color,
    pub mv: u16,
    pub child: u64,
}

pub struct OpeningTree {
    /// Lines read whole; a line with a move Fattorini's rules reject still gives its moves before
    /// it, and is counted in `broken_lines`.
    pub lines: usize,
    pub broken_lines: usize,
    /// Every book move once, in the order first met.
    pub edges: Vec<Edge>,
    /// Every position a book move leads to, by key: what the build searches.
    pub children: HashMap<u64, Board>,
}

impl OpeningTree {
    /// The tree of the embedded snapshot.
    pub fn from_snapshot() -> OpeningTree {
        let pgns: Vec<&str> = OPENINGS
            .iter()
            .flat_map(|volume| volume.lines().skip(1))
            .filter_map(|line| line.trim_end_matches('\r').split('\t').nth(2))
            .collect();
        OpeningTree::of(&pgns)
    }

    /// The tree of lines in PGN movetext ("1. e4 e5 2. Nf3"), every one from the start.
    pub fn of(pgns: &[&str]) -> OpeningTree {
        let (start, _) = Board::from_fen(STARTING_POSITION_FEN).expect("the starting position parses");
        let mut seen = HashSet::new();
        let mut edges = Vec::new();
        let mut children = HashMap::new();
        let (mut lines, mut broken_lines) = (0, 0);
        for pgn in pgns {
            let mut board = start;
            let mut whole = true;
            for token in pgn.split_whitespace() {
                // Move numbers ("1." and "1...") are no moves.
                let san = token.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.');
                if san.is_empty() {
                    continue;
                }
                let resolved = san.parse::<SanPlus>().ok().and_then(|parsed| try_resolve_san(&board, &parsed.san));
                let Some(mv) = resolved else {
                    whole = false;
                    break;
                };
                let edge_parent = polyglot_key(&board);
                let mover = board.side_to_move();
                board.make_move(mv);
                let edge = Edge { parent: edge_parent, mover, mv: polyglot_move(mv), child: polyglot_key(&board) };
                // A position and a move decide the rest of the edge.
                if seen.insert((edge.parent, edge.mv)) {
                    edges.push(edge);
                }
                children.entry(edge.child).or_insert(board);
            }
            if whole {
                lines += 1;
            } else {
                broken_lines += 1;
            }
        }
        OpeningTree { lines, broken_lines, edges, children }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_lines_share_their_prefix() {
        let tree = OpeningTree::of(&["1. e4 e5 2. Nf3", "1. e4 e5 2. Nc3"]);
        assert_eq!(tree.lines, 2);
        assert_eq!(tree.edges.len(), 4, "e4, e5, Nf3, Nc3");
        assert_eq!(tree.children.len(), 4);
    }

    #[test]
    fn a_transposition_is_one_position() {
        let tree = OpeningTree::of(&["1. Nf3 Nf6 2. Nc3", "1. Nc3 Nf6 2. Nf3"]);
        assert_eq!(tree.edges.iter().filter(|edge| edge.mover == Color::White).count(), 4);
        assert_eq!(tree.edges[2].child, tree.edges[5].child, "both orders reach one position");
        assert_eq!(tree.children.len(), 5);
    }

    #[test]
    fn castling_is_the_king_onto_its_rook() {
        let tree = OpeningTree::of(&["1. e4 e5 2. Nf3 Nc6 3. Bc4 Bc5 4. O-O"]);
        let castle = tree.edges.last().unwrap();
        // e1 is square 4, h1 square 7: to in bits 0-5, from in bits 6-11.
        assert_eq!(castle.mv, 7 | (4 << 6));
    }

    #[test]
    fn a_rejected_move_keeps_the_moves_before_it() {
        // Black has no knight that reaches f3.
        let tree = OpeningTree::of(&["1. e4 e5 2. Nf3 Nf3 3. Nc3"]);
        assert_eq!((tree.lines, tree.broken_lines), (0, 1));
        assert_eq!(tree.edges.len(), 3);
    }

    #[test]
    fn the_snapshot_reads_whole() {
        let tree = OpeningTree::from_snapshot();
        assert!(tree.lines > 3_000, "{} lines", tree.lines);
        assert_eq!(tree.broken_lines, 0, "every line of the snapshot is legal");
    }
}
