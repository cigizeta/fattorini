//! Lc0's second move: what a filter on the second-highest entry of a record's policy target would
//! drop beyond a quiet filter (not in check, the best move neither a capture nor a promotion; tried
//! and removed on 2026-11-01, as it hurt the networks it trained). A position whose best move is quiet but whose second is a capture still
//! has a capture as a real alternative, its label resting on tactics the board does not show.
//! Stockfish's trainers find that second move by a depth-6 MultiPV-2 search; Lc0's records may hold
//! it already, from the search that labelled them.
//!
//! `second_move` reads raw test91 tars, downloaded by hand and named by `LC0_RAW_TARS` (paths
//! separated by `;`, relative ones from the repository's root), and prints three tables: what the
//! policy target is, how strong its second move is, and what a filter on it would drop. It writes
//! nothing.

use std::io::{BufReader, Read};
use std::path::Path;

use shakmaty::{Chess, Move, Position};

use super::convert::{decode, replay, standard_start};
use super::moves::{table, IndexedMove, MOVE_COUNT};
use super::record::{records, Record};
use super::tar::TarReader;

/// The second move's lowest shares a filter is tried at; a second move never visited is never counted.
const THRESHOLDS: [f32; 4] = [0.0, 0.05, 0.10, 0.20];
/// Piece-count groups, inclusive.
const GROUPS: [(u32, u32); 4] = [(3, 6), (7, 13), (14, 20), (21, 32)];

#[derive(Default)]
struct Tally {
    games: u64,
    /// Games not of standard chess, skipped as the converter skips them.
    skipped: u64,
    records: u64,
    /// Records whose nonnegative entries are not exactly their position's legal moves.
    entries_off: u64,
    /// Records whose legal entries do not sum to 1 within 1e-4.
    sum_off: u64,
    visits_zero: u64,
    /// Records whose entries are all multiples of 1 / (visits - 1), and of 1 / visits.
    multiples_less_one: u64,
    multiples: u64,
    top_is_best: u64,
    top_is_played: u64,
    /// Records with a second legal move: its share, and the best move's lead over it, by tenths.
    second_share: [u64; 10],
    gap: [u64; 10],
    /// The quiet positions by piece group, and, by threshold, those whose second move is a capture or
    /// a promotion.
    quiet: [u64; 4],
    dropped: [[u64; 4]; 4],
}

fn tally_game(game: &[u8], table: &[IndexedMove], tally: &mut Tally) -> Result<(), String> {
    let records = records(game)?;
    let Some(start) = standard_start(&records)? else {
        tally.skipped += 1;
        return Ok(());
    };
    replay(&records, start, table, |ply, record, position, _| tally_record(ply, record, position, table, tally))?;
    tally.games += 1;
    Ok(())
}

fn tally_record(ply: usize, record: &Record, position: &Chess, table: &[IndexedMove], tally: &mut Tally) -> Result<(), String> {
    tally.records += 1;
    let mut entries: Vec<(usize, f32)> =
        (0..MOVE_COUNT).map(|index| (index, record.probability(index))).filter(|&(_, share)| share >= 0.0).collect();
    let decodes = entries.iter().all(|&(index, _)| decode(table, index as u16, record, position).is_ok());
    if entries.len() != position.legal_moves().len() || !decodes {
        tally.entries_off += 1;
    }
    let sum: f32 = entries.iter().map(|&(_, share)| share).sum();
    if (sum - 1.0).abs() > 1e-4 {
        tally.sum_off += 1;
    }
    let visits = record.visits();
    if visits == 0 {
        tally.visits_zero += 1;
    }
    let multiples_of = |count: u32| {
        count > 0
            && entries.iter().all(|&(_, share)| {
                let scaled = share as f64 * count as f64;
                (scaled - scaled.round()).abs() < 0.01
            })
    };
    if visits > 1 && multiples_of(visits - 1) {
        tally.multiples_less_one += 1;
    }
    if multiples_of(visits) {
        tally.multiples += 1;
    }

    entries.sort_by(|a, b| b.1.total_cmp(&a.1));
    let (best, played) = (record.best_idx() as usize, record.played_idx() as usize);
    if entries.first().is_some_and(|&(index, _)| index == best) {
        tally.top_is_best += 1;
    }
    if entries.first().is_some_and(|&(index, _)| index == played) {
        tally.top_is_played += 1;
    }
    // The second move: the highest entry other than the best move's, which the quiet filter reads.
    let best_share = entries.iter().find(|&&(index, _)| index == best).map(|&(_, share)| share);
    let second = entries.iter().find(|&&(index, _)| index != best).copied();
    if let Some((_, second_share)) = second {
        tally.second_share[tenth(second_share)] += 1;
        if let Some(best_share) = best_share {
            tally.gap[tenth((best_share - second_share).max(0.0))] += 1;
        }
    }

    let best_move = decode(table, best as u16, record, position).map_err(|message| format!("ply {ply}: best {message}"))?;
    if position.is_check() || is_tactical(&best_move) {
        return Ok(());
    }
    let group = group(position.board().occupied().count() as u32);
    tally.quiet[group] += 1;
    let Some((second_index, second_share)) = second else {
        return Ok(());
    };
    let second_move = decode(table, second_index as u16, record, position).map_err(|message| format!("ply {ply}: second {message}"))?;
    if is_tactical(&second_move) && second_share > 0.0 {
        for (row, &threshold) in THRESHOLDS.iter().enumerate() {
            if second_share >= threshold {
                tally.dropped[row][group] += 1;
            }
        }
    }
    Ok(())
}

fn is_tactical(mv: &Move) -> bool {
    mv.is_capture() || mv.promotion().is_some()
}

fn tenth(share: f32) -> usize {
    ((share * 10.0) as usize).min(9)
}

/// The group of `pieces`; fewer than 3 go with the first.
fn group(pieces: u32) -> usize {
    GROUPS.iter().position(|&(_, high)| pieces <= high).unwrap_or(GROUPS.len() - 1)
}

fn percent(part: u64, whole: u64) -> f64 {
    if whole == 0 { 0.0 } else { 100.0 * part as f64 / whole as f64 }
}

fn print(tally: &Tally) {
    let records = tally.records;
    println!("{} standard games ({} skipped), {records} records", tally.games, tally.skipped);

    println!("\n1. The policy target, of every record");
    for (name, count) in [
        ("entries not exactly the legal moves", tally.entries_off),
        ("legal entries not summing to 1", tally.sum_off),
        ("visits 0", tally.visits_zero),
        ("multiples of 1 / (visits - 1)", tally.multiples_less_one),
        ("multiples of 1 / visits", tally.multiples),
        ("top entry is best_idx", tally.top_is_best),
        ("top entry is played_idx", tally.top_is_played),
    ] {
        println!("  {name:<38} {count:>10} {:>8.3}%", percent(count, records));
    }

    let with_second: u64 = tally.second_share.iter().sum();
    let with_gap: u64 = tally.gap.iter().sum();
    println!("\n2. The second move, of the {with_second} records with one");
    println!("  {:<9} {:>9} {:>11}", "tenth", "its share", "best's lead");
    for tenth in 0..10 {
        println!(
            "  {:.1}-{:.1}   {:>8.2}% {:>10.2}%",
            tenth as f64 / 10.0,
            (tenth + 1) as f64 / 10.0,
            percent(tally.second_share[tenth], with_second),
            percent(tally.gap[tenth], with_gap)
        );
    }

    let quiet: u64 = tally.quiet.iter().sum();
    println!("\n3. Quiet positions whose second move is a capture or a promotion");
    print!("  {:<16}", "pieces");
    for (low, high) in GROUPS {
        print!(" {:>8}", format!("{low}-{high}"));
    }
    println!(" {:>8}", "all");
    print!("  {:<16}", "quiet positions");
    for count in tally.quiet {
        print!(" {count:>8}");
    }
    println!(" {quiet:>8}");
    for (row, threshold) in THRESHOLDS.iter().enumerate() {
        print!("  {:<16}", format!("second >= {threshold:.2}"));
        for (group, &count) in tally.dropped[row].iter().enumerate() {
            print!(" {:>7.2}%", percent(count, tally.quiet[group]));
        }
        println!(" {:>7.2}%", percent(tally.dropped[row].iter().sum(), quiet));
    }
}

pub(super) fn gunzip(gzipped: &[u8]) -> Result<Vec<u8>, String> {
    let mut game = Vec::new();
    flate2::read::GzDecoder::new(gzipped).read_to_end(&mut game).map_err(|error| format!("failed to gunzip: {error}"))?;
    Ok(game)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first game of `training-run2-test91-20261017-0417.tar`, as Lc0 published it
    /// (`tests/data/LC0-LICENSE.txt`).
    const REAL_GAME: &[u8] = include_bytes!("../../tests/data/lc0-test91-229828019.gz");

    #[test]
    fn a_real_game_s_policy_targets_are_its_legal_moves() {
        let mut tally = Tally::default();
        tally_game(&gunzip(REAL_GAME).unwrap(), &table(), &mut tally).unwrap_or_else(|message| panic!("{message}"));
        assert_eq!((tally.games, tally.records), (1, 69));
        assert_eq!(tally.entries_off, 0, "every nonnegative entry a legal move, every legal move an entry");
        assert_eq!(tally.sum_off, 0, "every policy target sums to 1");
    }

    #[test]
    fn piece_counts_fall_in_their_groups() {
        assert_eq!([2, 3, 6, 7, 13, 14, 20, 21, 32].map(group), [0, 0, 0, 1, 1, 2, 2, 3, 3]);
    }

    #[test]
    #[ignore = "reads raw Lc0 tars downloaded by hand, named by LC0_RAW_TARS"]
    fn second_move() {
        let paths = std::env::var("LC0_RAW_TARS").expect("LC0_RAW_TARS names the tars, separated by ;");
        // Tests run from the crate's directory: a relative path is the repository's.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let table = table();
        let mut tally = Tally::default();
        for path in paths.split(';').filter(|path| !path.is_empty()) {
            let file = std::fs::File::open(root.join(path)).unwrap_or_else(|error| panic!("failed to open {path}: {error}"));
            let mut archive = TarReader::new(BufReader::new(file));
            while let Some((name, gzipped)) = archive.next_file().unwrap_or_else(|message| panic!("{path}: {message}")) {
                if super::super::game_id(&name).is_none() {
                    continue;
                }
                let game = gunzip(&gzipped).unwrap_or_else(|message| panic!("{path}: {name}: {message}"));
                tally_game(&game, &table, &mut tally).unwrap_or_else(|message| panic!("{path}: {name}: {message}"));
            }
            println!("{path}: read, {} games so far", tally.games);
        }
        print(&tally);
    }
}
