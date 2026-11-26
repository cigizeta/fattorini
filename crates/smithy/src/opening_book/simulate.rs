//! `smithy book-simulate <engine> [<loss weight>...]`: how big the fragments would be under other
//! loss weights, without a search. An expansion does not depend on the weight, only the costs do,
//! so Dijkstra is rerun over the positions already expanded (every fragment and the next one's
//! progress file); a position never expanded stands in as a leaf and is counted as one to search.
//! Past the first such positions the counts are lower bounds: their own moves are unknown.

use std::collections::HashSet;

use fattorini::board::Board;

use super::dijkstra;
use super::expand::Expansion;
use super::fragment::{books_dir, progress_rows, read_fragments, stem, BookGraph, Searcher, CHARGE};
use super::unit::{Judged, Score, WHOLE};

/// The weights tried unless the command names some.
const DEFAULT_WEIGHTS: [f64; 5] = [1.0, 2.0, 3.0, 4.0, 6.0];
/// The budgets reported: 20 to 100, one ply of best moves apart.
const BUDGETS: [f64; 9] = [20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0];

/// Answers every unknown position with no moves, so the run stops there.
struct Leaves;

impl Searcher for Leaves {
    fn expand_each(&mut self, boards: &[Board], each: &mut dyn FnMut(usize, Result<Expansion, String>) -> Result<(), String>) {
        for index in 0..boards.len() {
            let leaf = Expansion { eval: Judged { score: Score::Cp(0), expected: WHOLE / 2 }, moves: Vec::new(), trace: Vec::new() };
            let _ = each(index, Ok(leaf));
        }
    }
}

pub fn run(engine_name: &str, weights: &[f64]) -> Result<(), String> {
    let weights = if weights.is_empty() { &DEFAULT_WEIGHTS[..] } else { weights };
    let books = books_dir();
    let fragments = read_fragments(&books, engine_name)?;
    let mut known: std::collections::HashMap<u64, _> =
        fragments.iter().flat_map(|(_, positions)| positions.iter().map(|position| (position.key, position.clone()))).collect();
    let next = fragments.len() as u16;
    known.extend(progress_rows(&format!("{}.progress.tsv", stem(&books, engine_name, next)))?);
    println!("{} positions expanded so far. Per loss weight and budget: positions in the book, of which still to search.", known.len());
    let budgets = BUDGETS;
    print!("{:>7}", "weight");
    for budget in &budgets {
        print!("  {:>16}", format!("budget {budget}"));
    }
    println!();
    let (start, _) = Board::from_fen(pgn_replay::STARTING_POSITION_FEN).expect("the starting position parses");
    for &weight in weights {
        let mut graph = BookGraph::new(known.clone(), Leaves, None, weight);
        let expanded = dijkstra::run(&mut graph, start, CHARGE, *budgets.last().unwrap(), |_| {})?;
        let leaves: HashSet<u64> = graph.store.searched.iter().copied().collect();
        print!("{weight:>7}");
        for &budget in &budgets {
            let within: Vec<_> = expanded.iter().filter(|expanded| expanded.cost <= budget).collect();
            let to_search = within.iter().filter(|expanded| leaves.contains(&expanded.key)).count();
            print!("  {:>16}", format!("{} ({to_search})", within.len()));
        }
        println!();
    }
    Ok(())
}
