use std::sync::atomic::AtomicBool;
use std::time::Instant;

use crate::bench_positions::{board, POSITIONS};
use crate::nnue::Network;
use crate::search::{choose_best_move_with_parameters, default_parameters_for, SearchParameters};
use crate::tablebase::SyzygyTablebase;
use crate::tt::TranspositionTable;

/// The depth `fattorini bench` searches every position to: `search_bench`'s reference depth, about a
/// second over the 49 positions. Not how a tune's games are played (those are timed): an
/// SPSA client runs the bench first, its node count telling the server whether the client searches
/// as it does, its nodes/s scaling the time control to the machine.
pub const BENCH_DEPTH: u32 = 9;

/// The table each bench position gets, fresh, so no position's search depends on another's.
const BENCH_HASH_MB: usize = 16;

/// The nodes and wall-clock seconds of `fattorini bench`'s searches with `evaluator`: every one of
/// `POSITIONS` to `BENCH_DEPTH`, one thread, a fresh table each, no tablebases. The node count is
/// the same on every run of one binary and network.
pub fn bench(evaluator: Option<&Network>) -> (u64, f64) {
    bench_at_depth(evaluator, BENCH_DEPTH)
}

/// `bench` to another depth (`fattorini bench <depth>`): what a search of that depth costs a position,
/// a fresh table each, as a replay or a filter pass over unrelated positions pays it.
pub fn bench_at_depth(evaluator: Option<&Network>, depth: u32) -> (u64, f64) {
    bench_with(evaluator, default_parameters_for(evaluator), depth)
}

/// `bench_at_depth` with `parameters` in place of the evaluator's own: what a build plays, its
/// embedded parameters (`fattorini bench`).
pub fn bench_with(evaluator: Option<&Network>, parameters: SearchParameters, depth: u32) -> (u64, f64) {
    let tablebase = SyzygyTablebase::new();
    let mut nodes = 0;
    let started = Instant::now();
    for line in POSITIONS {
        let position = board(line);
        let tt = TranspositionTable::new(BENCH_HASH_MB);
        let (reports, _) = choose_best_move_with_parameters(
            &position, depth, None, &[], &tt, &tablebase, evaluator, parameters, 0, &AtomicBool::new(false),
        );
        nodes += reports.last().map_or(0, |report| report.nodes);
    }
    (nodes, started.elapsed().as_secs_f64())
}

/// How many times `loaded_bench` runs, keeping the fastest: one run is about a second, short enough
/// that a CPU still raising its clock or a background task slows it; the fastest of five is the
/// machine's speed. The smithy and the arena read one run each before 2026-11-13, and on one idle
/// machine the smithy's came out 925,334 nodes per second per thread against the arena's 1,096,999,
/// scaling every arena's clock to 0.84 of the control asked for.
pub const LOADED_BENCH_RUNS: usize = 5;

/// The nodes per second of one search thread on this machine with `network`, loaded: `bench` on
/// `threads` threads at once, the mean of the threads, the fastest of `LOADED_BENCH_RUNS` runs. The
/// one measure the smithy's reference speed and every arena's own speed are both taken by, so a
/// clocked test or tune scales each arena's clock by like against like.
pub fn loaded_bench(network: Option<&Network>, threads: usize) -> f64 {
    let runs: Vec<f64> = (0..LOADED_BENCH_RUNS).map(|_| loaded_bench_once(network, threads)).collect();
    fastest(&runs)
}

/// One run of `loaded_bench`: `bench` on `threads` threads at once, the mean of their nodes per second.
fn loaded_bench_once(network: Option<&Network>, threads: usize) -> f64 {
    let runs: Vec<(u64, f64)> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads.max(1)).map(|_| scope.spawn(move || bench(network))).collect();
        handles.into_iter().map(|handle| handle.join().expect("a bench thread panicked")).collect()
    });
    runs.iter().map(|&(nodes, seconds)| nodes as f64 / seconds.max(1e-9)).sum::<f64>() / runs.len().max(1) as f64
}

/// The fastest of `runs`, 0 for none.
fn fastest(runs: &[f64]) -> f64 {
    runs.iter().copied().fold(0.0, f64::max)
}

/// The last line of `fattorini bench`: `<n> nodes <n> nps`, the form engine testers read.
pub fn bench_line(nodes: u64, seconds: f64) -> String {
    let nps = (nodes as f64 / seconds.max(1e-9)) as u64;
    format!("{nodes} nodes {nps} nps")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bench_line_gives_nodes_then_nps() {
        assert_eq!(bench_line(3_000_000, 1.5), "3000000 nodes 2000000 nps");
    }

    #[test]
    fn a_loaded_bench_keeps_its_fastest_run() {
        assert_eq!(fastest(&[925_334.0, 1_096_999.0, 1_050_000.0]), 1_096_999.0);
        assert_eq!(fastest(&[]), 0.0);
        assert_eq!(LOADED_BENCH_RUNS, 5);
    }
}
