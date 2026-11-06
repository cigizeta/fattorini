use std::sync::atomic::AtomicBool;
use std::time::Instant;

use crate::bench_positions::{board, POSITIONS};
use crate::nnue::Network;
use crate::search::choose_best_move;
use crate::tablebase::SyzygyTablebase;
use crate::tt::TranspositionTable;

/// The depth `kalo-chess bench` searches every position to: `search_bench`'s reference depth, about a
/// second over the 49 positions. Not how a tune's games are played (those are timed): an
/// SPSA client runs the bench first, its node count telling the server whether the client searches
/// as it does, its nodes/s scaling the time control to the machine.
pub const BENCH_DEPTH: u32 = 9;

/// The table each bench position gets, fresh, so no position's search depends on another's.
const BENCH_HASH_MB: usize = 16;

/// The nodes and wall-clock seconds of `kalo-chess bench`'s searches with `evaluator`: every one of
/// `POSITIONS` to `BENCH_DEPTH`, one thread, a fresh table each, no tablebases. The node count is
/// the same on every run of one binary and network.
pub fn bench(evaluator: Option<&Network>) -> (u64, f64) {
    bench_at_depth(evaluator, BENCH_DEPTH)
}

/// `bench` to another depth (`kalo-chess bench <depth>`): what a search of that depth costs a position,
/// a fresh table each, as a replay or a filter pass over unrelated positions pays it.
pub fn bench_at_depth(evaluator: Option<&Network>, depth: u32) -> (u64, f64) {
    let tablebase = SyzygyTablebase::new();
    let mut nodes = 0;
    let started = Instant::now();
    for line in POSITIONS {
        let position = board(line);
        let tt = TranspositionTable::new(BENCH_HASH_MB);
        let (reports, _) = choose_best_move(&position, depth, None, &[], &tt, &tablebase, evaluator, 0, &AtomicBool::new(false));
        nodes += reports.last().map_or(0, |report| report.nodes);
    }
    (nodes, started.elapsed().as_secs_f64())
}

/// The last line of `kalo-chess bench`: `<n> nodes <n> nps`, the form engine testers read.
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
}
