//! `smithy book-polyglot <engine> [--temperature <T>]`: the union of the engine's fragments as a
//! Polyglot book. Each position's moves weigh e^((score - best) / T) of its best move, scaled to
//! 65,535, the scores in centipawns from the mover's side. A new T needs no new search.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;

use fattorini::board::Board;
use fattorini::movegen::legal::legal_moves;
use fattorini::polyglot::{book_bytes, polyglot_key, polyglot_move, Entry};

use super::format::Position;
use super::fragment::{books_dir, read_fragments, to_white};
use super::unit::Score;

/// The softmax temperature unless `--temperature` says otherwise, in centipawns: a move 46 cp
/// below its position's best is played about 10 times less often.
pub const DEFAULT_TEMPERATURE: f64 = 20.0;

/// A mate's score in the softmax, from the mover's side: far beyond any centipawn score, so a
/// mating move takes the whole weight and a mated one none.
const MATE_CP: f64 = 10_000.0;

pub fn run(engine_name: &str, temperature: f64) -> Result<(), String> {
    if temperature.is_nan() || temperature <= 0.0 {
        return Err(format!("the temperature must be positive, not {temperature}"));
    }
    let books = books_dir();
    let fragments = read_fragments(&books, engine_name)?;
    let Some(last) = fragments.len().checked_sub(1) else {
        return Err(format!("no fragment of {engine_name}'s book in {books}"));
    };
    let positions: HashMap<u64, Position> =
        fragments.iter().flat_map(|(_, positions)| positions.iter().map(|position| (position.key, position.clone()))).collect();
    let (start, _) = Board::from_fen(pgn_replay::STARTING_POSITION_FEN).expect("the starting position parses");
    let boards = reach(start, &positions)?;
    let (book, left_out) = weigh(&positions, &boards, temperature);
    let path = format!("{books}/book-{engine_name}-f0-{last}-t{temperature}.bin");
    fs::write(&path, book_bytes(&book)).map_err(|error| format!("cannot write {path}: {error}"))?;
    let (header, _) = &fragments[0];
    println!(
        "Wrote {path}: fragments 0 to {last} of {}'s book at depth {}, T = {temperature} cp; {} positions, {} moves, {left_out} left out (weight 0).",
        header.engine,
        header.depth,
        boards.len(),
        book.len()
    );
    Ok(())
}

/// Every stored position's board, found by replaying the stored moves from the initial position:
/// a Polyglot key does not say whose move it is, and the softmax needs the mover's side. Every
/// expanded position was reached from an expanded one, so all are found; one that is not means a
/// broken book.
fn reach(start: Board, positions: &HashMap<u64, Position>) -> Result<HashMap<u64, Board>, String> {
    let mut boards = HashMap::new();
    let mut seen = HashSet::new();
    let mut queue = VecDeque::from([start]);
    while let Some(board) = queue.pop_front() {
        let key = polyglot_key(&board);
        if !seen.insert(key) {
            continue;
        }
        let Some(position) = positions.get(&key) else { continue };
        boards.insert(key, board);
        let legal = legal_moves(&board, board.side_to_move());
        for &(code, _) in &position.moves {
            let mv = legal
                .iter()
                .copied()
                .find(|&mv| polyglot_move(mv) == code)
                .ok_or_else(|| format!("position {key:016x}: stored move {code:#06x} is not legal"))?;
            let mut child = board;
            child.make_move(mv);
            queue.push_back(child);
        }
    }
    if boards.len() != positions.len() {
        return Err(format!("{} stored positions are not reached from the initial position", positions.len() - boards.len()));
    }
    Ok(boards)
}

/// The Polyglot entries, sorted by key then weight from the heaviest, and how many moves were left
/// out for a weight rounding to 0.
fn weigh(positions: &HashMap<u64, Position>, boards: &HashMap<u64, Board>, temperature: f64) -> (Vec<Entry>, usize) {
    let mut book = Vec::new();
    let mut left_out = 0;
    for (key, position) in positions {
        let side = boards[key].side_to_move();
        let scores: Vec<f64> = position.moves.iter().map(|&(_, score)| centipawns(to_white(score, side))).collect();
        let best = scores.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        for (&(mv, _), score) in position.moves.iter().zip(scores) {
            let weight = (u16::MAX as f64 * ((score - best) / temperature).exp()).round() as u16;
            if weight == 0 {
                left_out += 1;
            } else {
                book.push(Entry { key: *key, mv, weight });
            }
        }
    }
    book.sort_by(|a, b| a.key.cmp(&b.key).then(b.weight.cmp(&a.weight)).then(a.mv.cmp(&b.mv)));
    (book, left_out)
}

/// A score from the mover's side in centipawns, a mate at `MATE_CP`.
fn centipawns(score: Score) -> f64 {
    match score {
        Score::Cp(cp) => cp as f64,
        Score::Mate(n) if n > 0 => MATE_CP,
        Score::Mate(_) => -MATE_CP,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start() -> Board {
        Board::from_fen(pgn_replay::STARTING_POSITION_FEN).unwrap().0
    }

    fn code(board: &Board, text: &str) -> u16 {
        polyglot_move(fattorini::uci::parse_move(text, &legal_moves(board, board.side_to_move())).unwrap())
    }

    fn child(board: &Board, text: &str) -> Board {
        let mut next = *board;
        next.make_move(fattorini::uci::parse_move(text, &legal_moves(board, board.side_to_move())).unwrap());
        next
    }

    /// The root with e4 and d4; after 1. e4, Black's e5 and c5; scores from White's side.
    fn book(e4: i32, d4: i32, e5: i32, c5: i32) -> HashMap<u64, Position> {
        let root = start();
        let after_e4 = child(&root, "e2e4");
        HashMap::from([
            (polyglot_key(&root), Position {
                key: polyglot_key(&root),
                eval: Score::Cp(e4),
                moves: vec![(code(&root, "e2e4"), Score::Cp(e4)), (code(&root, "d2d4"), Score::Cp(d4))],
            }),
            (polyglot_key(&after_e4), Position {
                key: polyglot_key(&after_e4),
                eval: Score::Cp(e5),
                moves: vec![(code(&after_e4, "e7e5"), Score::Cp(e5)), (code(&after_e4, "c7c5"), Score::Cp(c5))],
            }),
        ])
    }

    fn weights(positions: &HashMap<u64, Position>, board: &Board) -> Vec<(u16, u16)> {
        let boards = reach(start(), positions).unwrap();
        let (book, _) = weigh(positions, &boards, 20.0);
        book.iter().filter(|entry| entry.key == polyglot_key(board)).map(|entry| (entry.mv, entry.weight)).collect()
    }

    #[test]
    fn two_moves_46_cp_apart_weigh_about_10_to_1() {
        let root = start();
        let found = weights(&book(30, -16, 30, 30), &root);
        assert_eq!(found[0], (code(&root, "e2e4"), 65_535));
        let ratio = found[0].1 as f64 / found[1].1 as f64;
        assert!((ratio - 10.0).abs() < 0.1, "{ratio}");
    }

    #[test]
    fn black_prefers_what_is_worse_for_white() {
        let after_e4 = child(&start(), "e2e4");
        let found = weights(&book(30, 30, 30, -10), &after_e4);
        assert_eq!(found[0], (code(&after_e4, "c7c5"), 65_535));
    }

    #[test]
    fn a_mate_takes_the_weight_and_a_far_worse_move_is_left_out() {
        let mut positions = book(30, 30, 30, 30);
        let root = start();
        let entry = positions.get_mut(&polyglot_key(&root)).unwrap();
        entry.moves = vec![(code(&root, "e2e4"), Score::Mate(5)), (code(&root, "d2d4"), Score::Cp(500))];
        let boards = reach(root, &positions).unwrap();
        let (book, left_out) = weigh(&positions, &boards, 20.0);
        assert_eq!(left_out, 1);
        assert_eq!(book.iter().filter(|entry| entry.key == polyglot_key(&root)).count(), 1);
    }

    #[test]
    fn a_position_out_of_reach_is_refused() {
        let mut positions = book(30, 30, 30, 30);
        let stray = child(&child(&start(), "g1f3"), "g8f6");
        positions.insert(polyglot_key(&stray), Position { key: polyglot_key(&stray), eval: Score::Cp(0), moves: Vec::new() });
        assert!(reach(start(), &positions).is_err());
    }
}
