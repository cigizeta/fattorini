//! `smithy uci-book-polyglot <evaluations file> [--temperature <T>]`: the book's evaluations turned
//! into a Polyglot book. The moves come from the openings tree, rebuilt from the embedded snapshot;
//! each move is scored by the evaluation of the position it leads to, from the side that plays it,
//! and weighs e^((score - best) / T) of its position's best move, scaled to 65,535. Changing T
//! needs no new search.

use std::collections::HashMap;
use std::fs;

use fattorini::board::color::Color;
use fattorini::polyglot::{book_bytes, Entry as PolyglotEntry};

use super::format::{self, Eval};
use super::tree::{Edge, OpeningTree};

/// The softmax temperature unless `--temperature` says otherwise, in centipawns: a move 46 cp
/// below its position's best is played about 10 times less often.
pub const DEFAULT_TEMPERATURE: f64 = 20.0;

/// A mate's score in the softmax, from the side that plays the move: far beyond any centipawn
/// score, so a mating move takes the whole weight and a mated one none.
const MATE_CP: f64 = 10_000.0;

/// The Polyglot book's path: the evaluations file's stem with `-t<T>`, beside it.
pub fn output_path(evaluations: &str, temperature: f64) -> String {
    let stem = evaluations.strip_suffix(".bin").unwrap_or(evaluations);
    format!("{stem}-t{temperature}.bin")
}

pub fn run(evaluations: &str, temperature: f64) -> Result<(), String> {
    if temperature.is_nan() || temperature <= 0.0 {
        return Err(format!("the temperature must be positive, not {temperature}"));
    }
    let data = fs::read(evaluations).map_err(|error| format!("cannot read {evaluations}: {error}"))?;
    let (header, entries) = format::read(&data).map_err(|message| format!("{evaluations}: {message}"))?;
    let evals: HashMap<u64, Eval> = entries.iter().map(|entry| (entry.key, entry.eval)).collect();
    let tree = OpeningTree::from_snapshot();
    let (book, left_out) = weigh(&tree.edges, &evals, temperature)?;
    let path = output_path(evaluations, temperature);
    fs::write(&path, book_bytes(&book)).map_err(|error| format!("cannot write {path}: {error}"))?;
    let positions = book.iter().map(|entry| entry.key).collect::<std::collections::HashSet<_>>().len();
    println!(
        "Wrote {path}: book version {}, {} at depth {}, T = {temperature} cp; {positions} positions, {} moves, {left_out} moves left out (weight 0).",
        header.book_version,
        header.engine,
        header.depth,
        book.len()
    );
    Ok(())
}

/// The Polyglot entries of `edges`, sorted by key then weight from the heaviest, and how many moves
/// were left out for a weight rounding to 0. A position a move leads to without an evaluation is
/// refused: the evaluations were made from another snapshot.
fn weigh(edges: &[Edge], evals: &HashMap<u64, Eval>, temperature: f64) -> Result<(Vec<PolyglotEntry>, usize), String> {
    let mut by_parent: HashMap<u64, Vec<(u16, f64)>> = HashMap::new();
    for edge in edges {
        let eval = evals
            .get(&edge.child)
            .ok_or_else(|| format!("no evaluation for position {:016x}: the evaluations come from another openings snapshot", edge.child))?;
        by_parent.entry(edge.parent).or_default().push((edge.mv, mover_score(*eval, edge.mover)));
    }
    let mut book = Vec::new();
    let mut left_out = 0;
    for (key, moves) in by_parent {
        let best = moves.iter().map(|&(_, score)| score).fold(f64::NEG_INFINITY, f64::max);
        for (mv, score) in moves {
            let weight = (u16::MAX as f64 * ((score - best) / temperature).exp()).round() as u16;
            if weight == 0 {
                left_out += 1;
            } else {
                book.push(PolyglotEntry { key, mv, weight });
            }
        }
    }
    book.sort_by(|a, b| a.key.cmp(&b.key).then(b.weight.cmp(&a.weight)).then(a.mv.cmp(&b.mv)));
    Ok((book, left_out))
}

/// A stored evaluation, from White's side, as the score of the move leading to it for `mover`:
/// as is for White, negated for Black; a mate at `MATE_CP`.
fn mover_score(eval: Eval, mover: Color) -> f64 {
    let white = match eval {
        Eval::Cp(cp) => cp as f64,
        Eval::Mate(n) => n.signum() as f64 * MATE_CP,
    };
    if mover == Color::White { white } else { -white }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(parent: u64, mover: Color, mv: u16, child: u64) -> Edge {
        Edge { parent, mover, mv, child }
    }

    #[test]
    fn two_moves_46_cp_apart_weigh_about_10_to_1() {
        let edges = [edge(1, Color::White, 10, 2), edge(1, Color::White, 11, 3)];
        let evals = HashMap::from([(2, Eval::Cp(30)), (3, Eval::Cp(-16))]);
        let (book, left_out) = weigh(&edges, &evals, 20.0).unwrap();
        assert_eq!(left_out, 0);
        assert_eq!((book[0].mv, book[0].weight), (10, 65_535));
        let ratio = book[0].weight as f64 / book[1].weight as f64;
        assert!((ratio - 10.0).abs() < 0.1, "{ratio}");
    }

    #[test]
    fn black_prefers_what_is_worse_for_white() {
        let edges = [edge(1, Color::Black, 10, 2), edge(1, Color::Black, 11, 3)];
        let evals = HashMap::from([(2, Eval::Cp(30)), (3, Eval::Cp(-30))]);
        let (book, _) = weigh(&edges, &evals, 20.0).unwrap();
        assert_eq!((book[0].mv, book[0].weight), (11, 65_535));
    }

    #[test]
    fn a_mate_takes_the_weight_and_a_far_worse_move_is_left_out() {
        let edges = [edge(1, Color::White, 10, 2), edge(1, Color::White, 11, 3)];
        let evals = HashMap::from([(2, Eval::Mate(1)), (3, Eval::Cp(500))]);
        let (book, left_out) = weigh(&edges, &evals, 20.0).unwrap();
        assert_eq!(left_out, 1);
        assert_eq!(book, [PolyglotEntry { key: 1, mv: 10, weight: 65_535 }]);
    }

    #[test]
    fn a_missing_evaluation_is_refused() {
        let edges = [edge(1, Color::White, 10, 2)];
        assert!(weigh(&edges, &HashMap::new(), 20.0).is_err());
    }

    #[test]
    fn the_book_is_named_by_its_temperature() {
        assert_eq!(output_path("data/books/uci-book-judge-v1.bin", 20.0), "data/books/uci-book-judge-v1-t20.bin");
        assert_eq!(output_path("data/books/uci-book-judge-v1.bin", 12.5), "data/books/uci-book-judge-v1-t12.5.bin");
    }
}
