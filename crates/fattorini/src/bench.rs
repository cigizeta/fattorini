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

/// The depth `fattorini tm-probe` searches to by default: about what a 180+2 game reaches in the
/// opening and middlegame, where the time is spent.
pub const TM_PROBE_DEPTH: u32 = 22;

/// `fattorini tm-probe [<depth>]`: the time management's five factors measured on this
/// search, every one of `POSITIONS` searched to `depth` as `bench` searches them (one thread, a
/// fresh table each, `parameters`; the positions side by side over the machine's threads), its
/// factors after every completed depth. Returns one line per
/// position and depth (`position depth ms f pv_stable eval_stable changes nodes trend pv eval churn
/// multiplier`, `ms` the search's time so far, showing a position whose depths grow too costly), then per depth the medians of the node fraction and of each factor, the product
/// last.
pub fn tm_probe(evaluator: Option<&Network>, parameters: SearchParameters, depth: u32) -> Vec<String> {
    use crate::search::{choose_best_move_probing, TimeFactors};
    // Every position at once, each on its own thread: the factors come from node counts and
    // scores, which a one-thread search with a fresh table finds the same however busy the machine
    // is, so this measures nothing of speed. A pool of the machine's threads takes them in turn.
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(POSITIONS.len());
    let next = std::sync::atomic::AtomicUsize::new(0);
    // One completed depth of a position: the depth, the search's time so far in ms, the factors.
    type ProbeRow = (u32, u64, TimeFactors);
    let probed: Vec<Vec<ProbeRow>> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let tablebase = SyzygyTablebase::new();
                    let mut done = Vec::new();
                    loop {
                        let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(line) = POSITIONS.get(index) else { break };
                        let tt = TranspositionTable::new(BENCH_HASH_MB);
                        let mut rows = Vec::new();
                        choose_best_move_probing(&board(line), depth, &tt, &tablebase, evaluator, parameters, &AtomicBool::new(false), |report, factors| {
                            if let Some(f) = factors {
                                rows.push((report.depth, report.time_ms, f));
                            }
                        });
                        done.push((index, rows));
                    }
                    done
                })
            })
            .collect();
        let mut all: Vec<(usize, Vec<ProbeRow>)> =
            workers.into_iter().flat_map(|worker| worker.join().expect("a probe thread panicked")).collect();
        all.sort_by_key(|(index, _)| *index);
        all.into_iter().map(|(_, rows)| rows).collect()
    });
    let mut lines =
        vec!["position\tdepth\tms\tf\tpv_stable\teval_stable\tchanges\tnodes\ttrend\tpv\teval\tchurn\tmultiplier".to_string()];
    let mut by_depth: Vec<Vec<TimeFactors>> = vec![Vec::new(); depth as usize + 1];
    for (index, rows) in probed.iter().enumerate() {
        for &(report_depth, ms, f) in rows {
            lines.push(format!(
                "{}\t{}\t{ms}\t{:.3}\t{}\t{}\t{}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}",
                index + 1,
                report_depth,
                f.fraction,
                f.pv_stable,
                f.eval_stable,
                f.best_move_changes,
                f.nodes,
                f.trend,
                f.pv,
                f.eval,
                f.changes,
                f.multiplier
            ));
            if let Some(slot) = by_depth.get_mut(report_depth as usize) {
                slot.push(f);
            }
        }
    }
    lines.push("medians: depth\tpositions\tf\tnodes\tpv\teval\tchurn\tmultiplier".to_string());
    for (d, factors) in by_depth.iter().enumerate().filter(|(_, factors)| !factors.is_empty()) {
        let median = |value: fn(&TimeFactors) -> f64| {
            let mut values: Vec<f64> = factors.iter().map(value).collect();
            values.sort_by(f64::total_cmp);
            values[values.len() / 2]
        };
        lines.push(format!(
            "median\t{d}\t{}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{:.3}",
            factors.len(),
            median(|f| f.fraction),
            median(|f| f.nodes),
            median(|f| f.pv),
            median(|f| f.eval),
            median(|f| f.changes),
            median(|f| f.multiplier)
        ));
    }
    lines
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
    fn the_tm_probe_reports_every_depth_of_every_position_then_the_medians() {
        let lines = tm_probe(None, SearchParameters::DEFAULT, 3);
        assert!(lines[0].starts_with("position\tdepth\tms\tf"));
        let rows = lines.iter().filter(|line| line.chars().next().is_some_and(|c| c.is_ascii_digit())).count();
        assert!(rows > POSITIONS.len() * 2 && rows <= POSITIONS.len() * 3, "depths 1 to 3 of about every position: {rows}");
        let medians: Vec<&String> = lines.iter().filter(|line| line.starts_with("median\t")).collect();
        assert_eq!(medians.len(), 3);
        for line in medians {
            let multiplier: f64 = line.rsplit('\t').next().unwrap().parse().unwrap();
            assert!(multiplier > 0.0, "{line}");
        }
    }

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
