use crate::board::chess_move::Move;
use crate::board::Board;
use crate::movegen::legal::Legality;
use crate::movegen::pseudo_legal_moves;

/// Counts the number of leaf positions reachable from `board` after exactly `depth` half
/// moves, considering only legal move sequences -- the standard perft algorithm, used to
/// verify move generation correctness against published values (CLAUDE.md rule 2) far
/// more thoroughly than any hand-built test position can, since it exercises every piece
/// type, special rule, and make/unmake interaction at once, many moves deep.
///
/// A thin public wrapper: allocates one shared move buffer, reused for the entire
/// recursive tree walk beneath it, rather than asking every caller (tests, the dev
/// binary) to manage a buffer of their own for what's really an internal implementation
/// detail.
pub fn perft(board: &Board, depth: u32) -> u64 {
    let mut moves = Vec::new();
    perft_with_buffer(board, depth, &mut moves)
}

/// The actual recursive tree walk, threading one shared `moves` buffer through every
/// call instead of letting `pseudo_legal_moves` (and each generator beneath it) allocate
/// its own fresh `Vec` at every single node -- a real cost when one perft run can visit
/// hundreds of millions of nodes.
///
/// The buffer is used stack-fashion: `start` marks where this call's own slice of moves
/// begins (a plain local variable -- recursion's own call stack already gives every
/// active call its own marker, so no separate tracking structure is needed), this node's
/// moves are appended onto the end, and `moves.truncate(start)` restores the buffer to
/// exactly the state the caller left it in, right before returning. A child call always
/// leaves the buffer back at its own `end` before it returns, so this call's own slice
/// (`moves[start..end]`) stays valid to read across the whole loop.
///
/// Each pseudo-legal candidate is judged legal on the board before it (`Legality`), and made
/// only when perft recurses into it: at the last ply a legal move is counted without being
/// made at all.
fn perft_with_buffer(board: &Board, depth: u32, moves: &mut Vec<Move>) -> u64 {
    if depth == 0 {
        return 1;
    }

    let start = moves.len();
    let color = board.side_to_move();
    pseudo_legal_moves(board, color, moves);
    let end = moves.len();

    let legality = Legality::new(board, color);
    let mut scratch = *board;
    let mut nodes = 0;

    for i in start..end {
        let mv = moves[i];
        if !legality.is_legal(board, mv) {
            continue;
        }
        // Bulk counting: at the last ply, every legal move is a leaf worth exactly 1.
        if depth == 1 {
            nodes += 1;
            continue;
        }
        let undo = scratch.make_move(mv);
        nodes += perft_with_buffer(&scratch, depth - 1, moves);
        scratch.unmake_move(mv, undo);
    }

    moves.truncate(start);
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_zero_is_one_node_by_definition() {
        let (board, _) = Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        assert_eq!(perft(&board, 0), 1);
    }

    #[test]
    fn starting_position_matches_published_values_at_shallow_depths() {
        // These are among the most widely published, independently-verified numbers in
        // all of chess programming (CLAUDE.md rule 2) -- kept shallow here so this stays
        // a fast, routine `cargo test` check; deeper verification (depth 5-6, and other
        // standard test positions) runs through the `perft` dev binary instead, on
        // demand, not as part of the regular test suite.
        let (board, _) = Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        assert_eq!(perft(&board, 1), 20);
        assert_eq!(perft(&board, 2), 400);
        assert_eq!(perft(&board, 3), 8_902);
    }
}
