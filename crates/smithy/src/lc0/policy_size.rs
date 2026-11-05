//! What Lc0's visit counts would cost: the corpus converted with and without them, compressed as the
//! converter compresses a segment, and what a bucket record holding them would need, measured before
//! either format is chosen.
//!
//! `policy_size` reads raw test91 tars, downloaded by hand and named by `LC0_RAW_TARS` as
//! `second_move` takes them, and prints two tables. It writes nothing.

use std::io::{BufReader, Write};
use std::path::Path;

use super::convert::{convert_game, replay, standard_start, visit_counts};
use super::moves::{table, IndexedMove};
use super::record::records;
use super::second_move::gunzip;
use super::tar::TarReader;
use super::COMPRESSION_LEVEL;

/// A writer that only counts the bytes it is given: a zstd stream's compressed size, kept nowhere.
#[derive(Default)]
struct Counter {
    bytes: u64,
}

impl Write for Counter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.bytes += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[derive(Default)]
struct Tally {
    games: u64,
    /// Games not of standard chess, skipped as the converter skips them.
    skipped: u64,
    /// Games refused by `visit_counts`, with the first refusal's message.
    refused: u64,
    first_refusal: Option<String>,
    records: u64,
    /// The PGN text without and with counts, before compression.
    plain_text: u64,
    counted_text: u64,
    legal_moves: u64,
    /// Counts above zero.
    visited: u64,
    /// Records whose largest count fits 8 bits, 16 bits, or neither.
    widest: [u64; 3],
    /// Every record's visits less one, for its percentiles.
    children: Vec<u32>,
    /// A record's counts as one byte each when every count fits, two otherwise.
    dense_bytes: u64,
    /// Every count as a LEB128 varint.
    varint_bytes: u64,
    /// The counts above zero only, each as a one-byte index into the sorted legal moves and a varint.
    sparse_bytes: u64,
}

fn varint_len(value: u32) -> u64 {
    (32 - value.max(1).leading_zeros()).div_ceil(7) as u64
}

fn tally_game(game: &[u8], table: &[IndexedMove], plain: &mut impl Write, counted: &mut impl Write, tally: &mut Tally) -> Result<(), String> {
    let records = records(game)?;
    let Some(start) = standard_start(&records)? else {
        tally.skipped += 1;
        return Ok(());
    };
    let mut per_record = Vec::new();
    let replayed = replay(&records, start, table, |_, record, position, _| {
        per_record.push((visit_counts(table, record, position)?, record.visits() - 1));
        Ok(())
    });
    if let Err(message) = replayed {
        tally.refused += 1;
        tally.first_refusal.get_or_insert(message);
        return Ok(());
    }
    let plain_text = convert_game("0", game, table, false)?.expect("a standard game").text;
    let counted_text = convert_game("0", game, table, true)?.expect("a standard game").text;
    plain.write_all(plain_text.as_bytes()).map_err(|error| error.to_string())?;
    counted.write_all(counted_text.as_bytes()).map_err(|error| error.to_string())?;
    tally.plain_text += plain_text.len() as u64;
    tally.counted_text += counted_text.len() as u64;

    tally.games += 1;
    for (counts, children) in per_record {
        tally.records += 1;
        tally.legal_moves += counts.len() as u64;
        tally.visited += counts.iter().filter(|&&count| count > 0).count() as u64;
        let largest = counts.iter().copied().max().unwrap_or(0);
        let width = if largest <= u8::MAX as u32 { 0 } else if largest <= u16::MAX as u32 { 1 } else { 2 };
        tally.widest[width] += 1;
        tally.children.push(children);
        tally.dense_bytes += counts.len() as u64 * if width == 0 { 1 } else { 2 };
        tally.varint_bytes += counts.iter().map(|&count| varint_len(count)).sum::<u64>();
        tally.sparse_bytes += counts.iter().filter(|&&count| count > 0).map(|&count| 1 + varint_len(count)).sum::<u64>();
    }
    Ok(())
}

fn print(tally: &mut Tally, plain_bytes: u64, counted_bytes: u64) {
    let records = tally.records.max(1) as f64;
    // The converter counts the final position too, which has no record and no counts.
    let positions = (tally.records + tally.games).max(1) as f64;
    println!("{} standard games ({} skipped, {} refused), {} records", tally.games, tally.skipped, tally.refused, tally.records);
    if let Some(message) = &tally.first_refusal {
        println!("first refusal: {message}");
    }

    println!("\n1. The corpus, bytes a position (the final one included, as the converter counts)");
    println!("  {:<16} {:>10} {:>12}", "", "text", "zstd 19");
    for (name, text, compressed) in [("without counts", tally.plain_text, plain_bytes), ("with counts", tally.counted_text, counted_bytes)] {
        println!("  {name:<16} {:>10.2} {:>12.2}", text as f64 / positions, compressed as f64 / positions);
    }
    println!(
        "  growth: {:.2} compressed bytes a position, {:.1}%",
        (counted_bytes as f64 - plain_bytes as f64) / positions,
        100.0 * (counted_bytes as f64 / plain_bytes.max(1) as f64 - 1.0)
    );

    println!("\n2. The counts, a record");
    println!("  legal moves          {:>8.2}", tally.legal_moves as f64 / records);
    println!("  counts above zero    {:>8.2}", tally.visited as f64 / records);
    for (name, count) in [("largest fits 8 bits", tally.widest[0]), ("largest fits 16 bits", tally.widest[1]), ("largest wider", tally.widest[2])] {
        println!("  {name:<20} {:>7.3}%", 100.0 * count as f64 / records);
    }
    tally.children.sort_unstable();
    let at = |fraction: f64| tally.children.get(((tally.children.len() as f64 - 1.0) * fraction) as usize).copied().unwrap_or(0);
    println!(
        "  visits less one: 1% {}, 10% {}, 50% {}, 90% {}, 99% {}, max {}",
        at(0.01),
        at(0.10),
        at(0.50),
        at(0.90),
        at(0.99),
        at(1.0)
    );
    println!("  uncompressed bytes a record, by encoding:");
    for (name, bytes) in [
        ("dense, 1 or 2 bytes a count", tally.dense_bytes),
        ("dense, varints", tally.varint_bytes),
        ("sparse, index and varint", tally.sparse_bytes),
    ] {
        println!("    {name:<28} {:>8.2}", bytes as f64 / records);
    }
}

#[test]
#[ignore = "reads raw Lc0 tars downloaded by hand, named by LC0_RAW_TARS"]
fn policy_size() {
    let paths = std::env::var("LC0_RAW_TARS").expect("LC0_RAW_TARS names the tars, separated by ;");
    // Tests run from the crate's directory: a relative path is the repository's.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let table = table();
    let threads = std::thread::available_parallelism().map_or(1, |count| count.get()) as u32;
    let encoder = || {
        let mut encoder = zstd::Encoder::new(Counter::default(), COMPRESSION_LEVEL).expect("a zstd encoder");
        encoder.multithread(threads).expect("zstd's threads");
        encoder
    };
    let (mut plain, mut counted) = (encoder(), encoder());
    let mut tally = Tally::default();
    for path in paths.split(';').filter(|path| !path.is_empty()) {
        let file = std::fs::File::open(root.join(path)).unwrap_or_else(|error| panic!("failed to open {path}: {error}"));
        let mut archive = TarReader::new(BufReader::new(file));
        while let Some((name, gzipped)) = archive.next_file().unwrap_or_else(|message| panic!("{path}: {message}")) {
            if super::game_id(&name).is_none() {
                continue;
            }
            let game = gunzip(&gzipped).unwrap_or_else(|message| panic!("{path}: {name}: {message}"));
            tally_game(&game, &table, &mut plain, &mut counted, &mut tally).unwrap_or_else(|message| panic!("{path}: {name}: {message}"));
        }
        println!("{path}: read, {} games so far", tally.games);
    }
    let plain_bytes = plain.finish().expect("zstd finishes").bytes;
    let counted_bytes = counted.finish().expect("zstd finishes").bytes;
    print(&mut tally, plain_bytes, counted_bytes);
}

#[test]
fn varints_take_seven_bits_a_byte() {
    assert_eq!([0, 1, 127, 128, 16_383, 16_384, u32::MAX].map(varint_len), [1, 1, 1, 2, 2, 3, 5]);
}
