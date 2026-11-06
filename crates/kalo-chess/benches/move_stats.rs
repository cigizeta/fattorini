//! Where the main search's quiet history scores fall, per remaining depth, and what the search did
//! with each quiet move: never reached (late-move pruning or an earlier cutoff ended the loop),
//! reached but pruned, searched, or the cause of the cutoff. Measures where a move picker's quiet
//! thresholds should sit.
//!
//! In-process and single-threaded, like `nnue_bench`, and only under the `move-stats` feature, which
//! compiles the recording into the search:
//!
//! `cargo bench --bench move_stats --features move-stats -- --depth 9 --csv history.csv`
//!
//! The evaluator is a `.nnue` path; without one, the PeSTO evaluation. Positions are `search_bench`'s, Stockfish's bench list, or `--fens FILE`, one FEN per
//! line; either may end in `moves ...`, applied first. `--csv FILE` writes every depth and score
//! bucket, for plotting.

use std::io::Write;
use std::sync::atomic::AtomicBool;

use kalo_chess::move_stats::{bucket, bucket_low, take, MoveStats, BUCKETS, DEPTHS, OUTCOMES};
use kalo_chess::nnue::Network;
use kalo_chess::search::choose_best_move;
use kalo_chess::tablebase::SyzygyTablebase;
use kalo_chess::tt::TranspositionTable;

use kalo_chess::bench_positions::{board as board_from_line, POSITIONS};

/// Fresh per search, so one position's history and entries never carry into the next.
const TT_SIZE_MB: usize = 64;

/// Candidate thresholds, in history points: the fraction of quiets at or below each is printed.
const THRESHOLDS: &[i32] = &[64, 0, -64, -128, -256, -512, -1024, -3200];

/// Score buckets merged per printed band; the CSV keeps every bucket.
const BAND_BUCKETS: usize = 4;

fn argument_value<'a>(arguments: &'a [String], name: &str) -> Option<&'a str> {
    return arguments.iter().position(|argument| argument == name).and_then(|at| arguments.get(at + 1)).map(String::as_str);
}

fn percent(part: u64, whole: u64) -> f64 {
    return if whole == 0 { 0.0 } else { 100.0 * part as f64 / whole as f64 };
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let depth = argument_value(&arguments, "--depth").map(|value| value.parse::<u32>().expect("--depth takes a number")).unwrap_or(9);
    let path = arguments.iter().find(|argument| argument.ends_with(".nnue")).cloned();
    let network = path.as_ref().map(|path| Network::load(path).unwrap_or_else(|problem| panic!("{problem}")));
    let fens: Vec<String> = match argument_value(&arguments, "--fens") {
        Some(file) => std::fs::read_to_string(file)
            .unwrap_or_else(|problem| panic!("{file}: {problem}"))
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(String::from)
            .collect(),
        None => POSITIONS.iter().map(|fen| fen.to_string()).collect(),
    };

    let source = path.as_deref().unwrap_or("the PeSTO evaluation");
    println!("evaluation: {source}, depth {depth}, {} positions", fens.len());

    let tablebase = SyzygyTablebase::new();
    let stop = AtomicBool::new(false);
    take();
    for fen in &fens {
        let board = board_from_line(fen);
        let tt = TranspositionTable::new(TT_SIZE_MB);
        choose_best_move(&board, depth, None, &[], &tt, &tablebase, network.as_ref(), 0, &stop);
    }
    let stats = take();

    print_summary(&stats);
    print_zeros_and_losing_captures(&stats);
    print_bands(&stats);
    if let Some(file) = argument_value(&arguments, "--csv") {
        write_csv(&stats, file);
        println!("\nevery bucket written to {file}");
    }
}

/// Per depth: nodes, quiets per node, what happened to them, and how many sit below each candidate
/// threshold (strictly: the buckets below the one holding it).
fn print_summary(stats: &MoveStats) {
    println!("\nper remaining depth (quiet moves, TT move excluded, nodes not in check):\n");
    print!("  {:>5} {:>10} {:>7} {:>8} {:>8} {:>8} {:>8}", "depth", "nodes", "q/node", "unreach%", "pruned%", "search%", "cutoff%");
    for threshold in THRESHOLDS {
        print!(" {:>7}", format!("<{threshold}"));
    }
    println!();
    for depth in 0..DEPTHS {
        let nodes = stats.nodes.get(depth).copied().unwrap_or(0);
        if nodes == 0 {
            continue;
        }
        let totals = stats.depth_totals(depth);
        let quiets: u64 = totals.iter().sum();
        print!(
            "  {depth:>5} {nodes:>10} {:>7.1} {:>8.1} {:>8.1} {:>8.1} {:>8.1}",
            quiets as f64 / nodes as f64,
            percent(totals[0], quiets),
            percent(totals[1], quiets),
            percent(totals[2], quiets),
            percent(totals[3], quiets)
        );
        for &threshold in THRESHOLDS {
            let at_or_below: u64 = (0..bucket(threshold)).map(|index| stats.count(depth, index).iter().sum::<u64>()).sum();
            print!(" {:>6.1}%", percent(at_or_below, quiets));
        }
        println!();
    }
}

/// Per depth: the quiets scoring exactly `0` against the rest of `[0, 64)`, and the losing captures
/// (SEE below `0`), each with its reached and cutoff rates.
fn print_zeros_and_losing_captures(stats: &MoveStats) {
    println!("\nper remaining depth: exactly 0, the rest of [0, 64), losing captures (SEE < 0)");
    println!("  (count, reached %, cutoff % of searched)\n");
    for depth in 0..DEPTHS {
        let nodes = stats.nodes.get(depth).copied().unwrap_or(0);
        if nodes == 0 {
            continue;
        }
        let quiets: u64 = stats.depth_totals(depth).iter().sum();
        let zeros = stats.zeros[depth];
        let mut low = [0u64; OUTCOMES];
        for index in bucket(0)..bucket(64) {
            let count = stats.count(depth, index);
            for outcome in 0..OUTCOMES {
                low[outcome] += count[outcome];
            }
        }
        let rest: [u64; OUTCOMES] = std::array::from_fn(|outcome| low[outcome] - zeros[outcome]);
        let describe = |count: [u64; OUTCOMES]| {
            let total: u64 = count.iter().sum();
            return format!(
                "{total:>7} {:>5.1}% {:>5.1}%",
                percent(total - count[0], total),
                percent(count[3], count[2] + count[3])
            );
        };
        println!(
            "  depth {depth:>2}  zero {} ({:.1}% of quiets)  (0, 64) {}  losing captures {}",
            describe(zeros),
            percent(zeros.iter().sum(), quiets),
            describe(rest),
            describe(stats.losing_captures[depth])
        );
    }
}

/// Per depth and score band: how many quiets, how many were reached, and of those searched, how
/// many caused the cutoff.
fn print_bands(stats: &MoveStats) {
    let band_width = BAND_BUCKETS as i32 * (bucket_low(1) - bucket_low(0));
    println!("\nper score band of {band_width} points: share of the depth's quiets, reached %, cutoff % of searched\n");
    for depth in 0..DEPTHS {
        let nodes = stats.nodes.get(depth).copied().unwrap_or(0);
        if nodes == 0 {
            continue;
        }
        let quiets: u64 = stats.depth_totals(depth).iter().sum();
        println!("  depth {depth}:");
        for band in 0..BUCKETS / BAND_BUCKETS {
            let mut count = [0u64; OUTCOMES];
            for index in band * BAND_BUCKETS..(band + 1) * BAND_BUCKETS {
                let bucket_count = stats.count(depth, index);
                for outcome in 0..OUTCOMES {
                    count[outcome] += bucket_count[outcome];
                }
            }
            let total: u64 = count.iter().sum();
            if total == 0 {
                continue;
            }
            let low = bucket_low(band * BAND_BUCKETS);
            println!(
                "    [{low:>6}, {:>6})  {:>6.2}%  reached {:>5.1}%  cutoff {:>5.1}%  ({total})",
                low + band_width,
                percent(total, quiets),
                percent(total - count[0], total),
                percent(count[3], count[2] + count[3])
            );
        }
    }
}

fn write_csv(stats: &MoveStats, file: &str) {
    let mut out = std::fs::File::create(file).unwrap_or_else(|problem| panic!("{file}: {problem}"));
    writeln!(out, "depth,score_low,not_reached,pruned,searched,cutoff").expect("csv write");
    for depth in 0..DEPTHS {
        for index in 0..BUCKETS {
            let count = stats.count(depth, index);
            if count.iter().sum::<u64>() == 0 {
                continue;
            }
            writeln!(out, "{depth},{},{},{},{},{}", bucket_low(index), count[0], count[1], count[2], count[3]).expect("csv write");
        }
    }
}
