//! A run's frozen test set: drawn once from its test games, written to disk, and read from there by
//! training, never recomputed.
//!
//! The test games share nothing with the training games, so no game is ever on both sides. The
//! freezer reads every test game, labels each position at every lambda asked for, and keeps exactly
//! the run's `test_positions` of them with Algorithm R (Vitter 1985) under a fixed seed, drawn from
//! the positions the run's training keeps (`game_labels::KEPT`, recorded in the manifest). K vs K
//! (`cache::is_bare_kings`) and every position not kept are read and counted, but the reservoir never
//! sees them.
//!
//! ```text
//! manifest.txt                 the seed, the segments read, their games and positions, the rows
//! test.positions               cache.rs position records, one zstd stream
//! lambda-0.95/test.labels      one f32 per position, in the same order
//! lambda-0.95/manifest.txt     the positions' manifest again: this label set is complete
//! lambda-polyline/             a polyline arm's labels, each position at its own lambda, and
//!     polyline.txt             the ten values it was labelled at: another polyline is refused
//! ```
//!
//! **Frozen means frozen.** Once `manifest.txt` exists, the positions are never rebuilt, and test
//! segments added to the directory afterwards are never read. Each manifest is written last, as the
//! mark that what it describes is complete.
//!
//! **Adding lambdas.** A grid whose lambdas the frozen set lacks gets them added: the segments the
//! manifest names are read again, in its order, under the same seed. Which position fills which slot
//! depends only on its index in that stream and on the seed, never on the position itself, so the
//! same slots come out and every new label lands beside its position by construction.
//!
//! Sets frozen by the old `smithy freeze` were written in independent zstd frames; concatenated
//! frames are one valid zstd stream, so they read here unchanged.

use std::fs::File;
use std::io::{BufReader, BufWriter, Write};

use temper_chess::features::CanonicalPosition;
use temper_chess::labels::Curve;
use temper_chess::pipeline::preprocessing_config::{lambda_dir_name, lambdas_text, ArmLambda};
use temper_chess::pipeline::segments::SEGMENT_SUFFIX;

use super::buckets::{blend, Labelling};
use super::cache::{is_bare_kings, read_label, read_position, write_label, write_position, CacheHeader, CacheRecord, LabelHeader, Progress};
use super::game_labels::{self_play_game_kept, self_play_game_labels, SelfPlaySegmentReader, KEPT};
use super::pgn_stream::{splitmix64, StreamFile};

/// Fixed, never process-seeded: a label set added later must draw the same slots as the first freeze.
const FREEZE_SEED: u64 = 20261017;

/// zstd's level for the frozen files: they are written once and read at every start, and zstd
/// decompresses about as fast at high levels as at low ones. The self-play segments use 19 too.
const COMPRESSION_LEVEL: i32 = 19;

/// Freezes the test set in `dir`, `rows` positions drawn from the searched positions of the segments
/// of `games_dir` that `files` lists, with a label set for each of `lambdas`; or adds the label sets
/// an already frozen one lacks. `files` is called only for a first freeze: a frozen set reads the
/// segments its manifest names. Every score read through `curve`, the teacher's (Lc0's curve for
/// Lc0's corpus, `labels::LOGISTIC` for PeSTO and our networks). The curve is not recorded: a set
/// labelled under another curve is deleted by hand and frozen again. An error when the set was frozen
/// at another size or keeping other positions, or the games hold too few positions kept.
pub(crate) fn ensure(
    dir: &str, games_dir: &str, files: impl FnOnce() -> Result<Vec<StreamFile>, String>, rows: u64, labelling: &Labelling, curve: &Curve,
) -> Result<(), String> {
    if let Some(manifest) = read_manifest(dir, rows)? {
        if manifest.keep != KEPT {
            return Err(format!("{dir} was frozen keeping the {} positions, not the {KEPT}: another set's directory", manifest.keep));
        }
        check_polyline(dir, labelling)?;
        let missing = missing(dir, labelling);
        if missing.is_empty() {
            return Ok(());
        }
        let missing = Labelling::new(&missing, labelling.polyline().copied())?;
        log::info!("frozen test set {dir}: adding the labels of lambda {} from {games_dir}", lambdas_text(missing.arms()));
        // At the frozen size, whatever a run asks: its labels sit beside every frozen position.
        let sample = sample(games_dir, &manifest.segments, manifest.positions, manifest.rows, false, &missing, curve);
        if (sample.games, sample.searched()) != (manifest.games, manifest.positions) {
            return Err(format!(
                "the frozen segments of {games_dir} now hold {} games and {} searched positions, but {} and {} were frozen into {dir}",
                sample.games, sample.searched(), manifest.games, manifest.positions
            ));
        }
        write_label_sets(dir, &missing, &sample.labels, &manifest);
        return Ok(());
    }

    let files = files()?;
    let prefix = format!("{games_dir}/");
    let mut segments = Vec::with_capacity(files.len());
    for file in &files {
        let name = file.path.strip_prefix(&prefix).and_then(|name| name.strip_suffix(SEGMENT_SUFFIX));
        let Some(name) = name else {
            return Err(format!("{} is not a segment of {games_dir}", file.path));
        };
        segments.push(name.to_string());
    }
    let positions: u64 = files.iter().map(|file| file.positions).sum();
    if positions < rows {
        return Err(format!("{games_dir} holds {} positions, fewer than the {} of its test set", progress::grouped(positions), progress::grouped(rows)));
    }
    log::info!(
        "freezing the test set of {games_dir} into {dir}: {} of {} positions, {} segment(s)",
        progress::grouped(rows),
        progress::grouped(positions),
        segments.len()
    );
    let sample = sample(games_dir, &segments, positions, rows, true, labelling, curve);
    // `files` lists searched positions, the `.meta` ones less one final position a game.
    if sample.searched() != positions {
        return Err(format!("the segments of {games_dir} hold {} searched positions, but their .meta say {positions}", sample.searched()));
    }
    let kept = sample.positions_kept;
    if kept < rows {
        return Err(format!(
            "{games_dir} holds only {kept} {KEPT} positions besides K vs K, fewer than the {rows} of its test set: it needs more test games"
        ));
    }
    let manifest = Manifest { seed: FREEZE_SEED, keep: KEPT.to_string(), games: sample.games, positions, rows, segments };
    std::fs::create_dir_all(dir).map_err(|error| format!("failed to create {dir}: {error}"))?;
    write_positions(dir, sample.positions.as_deref().expect("a first freeze samples positions"));
    write_label_sets(dir, labelling, &sample.labels, &manifest);
    write_manifest(&manifest_path(dir), &manifest);
    log::info!(
        "froze {rows} of {kept} {KEPT} positions from {} games into {dir}, labelled at lambda {}",
        manifest.games,
        lambdas_text(labelling.arms())
    );
    Ok(())
}

/// Whether `dir` holds a set frozen at `rows` with a label set for each of `labelling`'s arms, a
/// polyline one at its polyline: `ensure` would write nothing.
pub(crate) fn is_complete(dir: &str, rows: u64, labelling: &Labelling) -> bool {
    matches!(read_manifest(dir, rows), Ok(Some(_))) && missing(dir, labelling).is_empty() && check_polyline(dir, labelling).is_ok()
}

/// The frozen test set of `dir`, `rows` positions, each carrying its label for every one of
/// `labelling`'s arms in their order: all of it, or a seeded subset of it for a run asking fewer
/// (`subset`). An error when it is not frozen, was frozen smaller, lacks an arm's label set, or holds
/// a polyline label set of another polyline.
pub(crate) fn read(dir: &str, rows: u64, labelling: &Labelling) -> Result<Vec<CacheRecord>, String> {
    let Some(manifest) = read_manifest(dir, rows)? else {
        return Err(format!("{dir} holds no frozen test set: the server freezes it when it trains"));
    };
    let kept = subset(manifest.rows, rows);
    let missing = missing(dir, labelling);
    if !missing.is_empty() {
        return Err(format!("the frozen test set {dir} lacks lambda {}", lambdas_text(&missing)));
    }
    check_polyline(dir, labelling)?;
    let mut positions = open_decoder(&positions_path(dir));
    CacheHeader::read(&mut positions);
    let mut labels: Vec<_> = labelling
        .arms()
        .iter()
        .map(|&arm| {
            let path = labels_path(dir, arm);
            let mut reader = open_decoder(&path);
            let header = LabelHeader::read(&mut reader);
            let lambda = header_lambda(arm);
            assert_eq!(header.lambda, lambda, "{path} holds the labels of lambda {}, not {lambda}", header.lambda);
            reader
        })
        .collect();
    let mut records = Vec::with_capacity(rows as usize);
    // A live bar, a box of its own on the page, as the sample and the writing that froze the set:
    // a resumed run, which only reads it, shows its read as a starting one does (the developer,
    // 2026-10-27), not as a line of the training box.
    let progress = progress::bar(manifest.rows, progress::Unit::Count("positions"), "reading the frozen test set");
    for keep in kept {
        let canonical = read_position(&mut positions);
        let labels = labels.iter_mut().map(read_label).collect();
        if keep {
            records.push(CacheRecord { canonical, labels, progress: Progress::UNKNOWN });
        }
        progress.inc(1);
    }
    progress.finish();
    Ok(records)
}

fn open_decoder(path: &str) -> zstd::Decoder<'static, BufReader<File>> {
    let file = File::open(path).unwrap_or_else(|error| panic!("failed to open {path}: {error}"));
    zstd::Decoder::new(file).unwrap_or_else(|error| panic!("failed to open a zstd decoder for {path}: {error}"))
}

fn manifest_path(dir: &str) -> String {
    format!("{dir}/manifest.txt")
}

fn positions_path(dir: &str) -> String {
    format!("{dir}/test.positions")
}

/// A label set's directory, `lambda-0.95` or `lambda-polyline` (`lambda_dir_name`).
fn labels_dir(dir: &str, arm: ArmLambda) -> String {
    format!("{dir}/{}", lambda_dir_name(arm))
}

fn labels_path(dir: &str, arm: ArmLambda) -> String {
    format!("{}/test.labels", labels_dir(dir, arm))
}

fn labels_manifest_path(dir: &str, arm: ArmLambda) -> String {
    format!("{}/manifest.txt", labels_dir(dir, arm))
}

/// The polyline a polyline label set was labelled at (`Polyline::values_text`), beside its labels.
fn polyline_path(dir: &str) -> String {
    format!("{}/polyline.txt", labels_dir(dir, ArmLambda::Polyline))
}

/// A label file's header lambda: a constant's own, and for the polyline a value no constant takes.
const POLYLINE_HEADER_LAMBDA: f64 = -1.0;

fn header_lambda(arm: ArmLambda) -> f64 {
    arm.constant().unwrap_or(POLYLINE_HEADER_LAMBDA)
}

/// The arms of `labelling` whose label set `dir` lacks.
fn missing(dir: &str, labelling: &Labelling) -> Vec<ArmLambda> {
    labelling.arms().iter().copied().filter(|&arm| !label_set_frozen(dir, arm)).collect()
}

/// An error when `dir` holds a polyline label set frozen at another polyline than `labelling`'s: a
/// polyline measured again, whose set must be deleted to be frozen at the new one.
fn check_polyline(dir: &str, labelling: &Labelling) -> Result<(), String> {
    let (Some(polyline), true) = (labelling.polyline(), label_set_frozen(dir, ArmLambda::Polyline)) else {
        return Ok(());
    };
    let path = polyline_path(dir);
    let frozen = std::fs::read_to_string(&path).map_err(|error| format!("{path}: {error}"))?;
    if frozen.replace("\r\n", "\n") != polyline.values_text() {
        return Err(format!(
            "{} was labelled at another polyline than the one measured now: delete it, and the next training labels it again",
            labels_dir(dir, ArmLambda::Polyline)
        ));
    }
    Ok(())
}

/// The manifest of `dir`, or none when the set is not frozen. An error when it was frozen at fewer
/// positions than `rows`: a run asking fewer reads a subset of it (`subset`).
fn read_manifest(dir: &str, rows: u64) -> Result<Option<Manifest>, String> {
    let path = manifest_path(dir);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    let manifest = Manifest::parse(&text, &path);
    if manifest.keep.is_empty() {
        return Err(format!(
            "{dir} was frozen before the final position, never searched, was left out of every test set: delete it, and the \
             next training freezes it again"
        ));
    }
    if manifest.rows < rows {
        return Err(format!(
            "{dir} was frozen at {} positions, fewer than the run's test_positions of {}",
            progress::grouped(manifest.rows),
            progress::grouped(rows)
        ));
    }
    Ok(Some(manifest))
}

/// Fixed, as `FREEZE_SEED`: every run asking the same smaller size reads the same subset.
const SUBSET_SEED: u64 = 20261022;

/// Which of a frozen set's `frozen` slots a run asking `rows` reads, by slot: the `rows` slots of
/// smallest hash under `SUBSET_SEED`. Not the first `rows`: Algorithm R fills slot i with item i
/// before it replaces, so the first slots lean toward the first games read, while a seeded subset of
/// a uniform sample is uniform.
fn subset(frozen: u64, rows: u64) -> Vec<bool> {
    if rows >= frozen {
        return vec![true; frozen as usize];
    }
    let mut ranked: Vec<(u64, usize)> = (0..frozen as usize).map(|slot| (splitmix64(SUBSET_SEED ^ slot as u64), slot)).collect();
    ranked.select_nth_unstable(rows as usize);
    let mut kept = vec![false; frozen as usize];
    for &(_, slot) in &ranked[..rows as usize] {
        kept[slot] = true;
    }
    kept
}

/// Whether `arm`'s label set of `dir` is complete.
fn label_set_frozen(dir: &str, arm: ArmLambda) -> bool {
    std::path::Path::new(&labels_manifest_path(dir, arm)).exists()
}

/// What a freeze records, and what a label pass added later checks itself against.
#[derive(Clone, Debug, PartialEq)]
struct Manifest {
    seed: u64,
    /// The positions of a game it drew from (`KEPT`); empty in a set frozen before it was
    /// recorded, which kept the final positions and is refused (`read_manifest`).
    keep: String,
    games: u64,
    /// Every position read, K vs K included: the `.meta` files' count. The reservoir drew from all
    /// of them but K vs K.
    positions: u64,
    /// The positions kept: the run's `test_positions`.
    rows: u64,
    /// The segments read, relative to the games' directory, without `.pgn.zst`, in the order read.
    segments: Vec<String>,
}

impl Manifest {
    fn text(&self) -> String {
        let mut text = format!(
            "seed\t{}\nkeep\t{}\ngames\t{}\npositions\t{}\nrows\t{}\n",
            self.seed, self.keep, self.games, self.positions, self.rows
        );
        for segment in &self.segments {
            text.push_str(&format!("segment\t{segment}\n"));
        }
        text
    }

    /// Panics on anything malformed: a frozen manifest is written by this module alone.
    fn parse(text: &str, path: &str) -> Manifest {
        let mut manifest = Manifest { seed: 0, keep: String::new(), games: 0, positions: 0, rows: 0, segments: Vec::new() };
        let mut seen = [false; 4];
        for line in text.lines() {
            let (key, value) = line.split_once('\t').unwrap_or_else(|| panic!("{path}: malformed line {line:?}"));
            let number = || -> u64 { value.parse().unwrap_or_else(|_| panic!("{path}: {key} is not a number: {value:?}")) };
            match key {
                "seed" => (manifest.seed, seen[0]) = (number(), true),
                "keep" => manifest.keep = value.to_string(),
                "games" => (manifest.games, seen[1]) = (number(), true),
                "positions" => (manifest.positions, seen[2]) = (number(), true),
                "rows" => (manifest.rows, seen[3]) = (number(), true),
                "segment" => manifest.segments.push(value.to_string()),
                _ => panic!("{path}: unknown key {key:?}"),
            }
        }
        assert!(seen.iter().all(|&found| found), "{path}: seed, games, positions and rows are all required");
        assert!(!manifest.segments.is_empty(), "{path} names no segment");
        assert_eq!(manifest.seed, FREEZE_SEED, "{path} was frozen under another seed, so its slots cannot be drawn again");
        manifest
    }
}

/// Writes `manifest` to `path` via `.tmp` and a rename: a manifest is a completion marker, never half
/// written.
fn write_manifest(path: &str, manifest: &Manifest) {
    let tmp_path = format!("{path}.tmp");
    std::fs::write(&tmp_path, manifest.text()).unwrap_or_else(|error| panic!("failed to write {tmp_path}: {error}"));
    std::fs::rename(&tmp_path, path).unwrap_or_else(|error| panic!("failed to rename {tmp_path} to {path}: {error}"));
}

/// Algorithm R's choice for a stream of unknown length, `capacity` slots: the i-th item (from 0)
/// fills slot i while there are free slots, and afterwards replaces a slot drawn uniformly from 0 to
/// i, kept only when the draw falls among the slots. Every item ends up kept with probability
/// capacity / n. The draw is a hash of the index under the seed, not a library RNG, so it can never
/// change under a dependency update: a label set added years later draws the same slots.
struct Reservoir {
    capacity: u64,
    seed: u64,
    seen: u64,
}

impl Reservoir {
    fn new(capacity: u64, seed: u64) -> Reservoir {
        assert!(capacity > 0, "a reservoir holds at least one slot");
        Reservoir { capacity, seed, seen: 0 }
    }

    /// The slot the next item of the stream goes to, if it is kept.
    fn next(&mut self) -> Option<usize> {
        let index = self.seen;
        self.seen += 1;
        if index < self.capacity {
            return Some(index as usize);
        }
        // A modulo of a 64-bit hash: its bias is below 2^-30 for any stream this size.
        let draw = splitmix64(self.seed ^ splitmix64(index)) % (index + 1);
        if draw < self.capacity {
            return Some(draw as usize);
        }
        None
    }
}

/// Puts `item` in `slot`: appended while the reservoir is filling, replacing afterwards.
fn place<T>(slots: &mut Vec<T>, slot: usize, item: T) {
    if slot == slots.len() {
        slots.push(item);
    } else {
        slots[slot] = item;
    }
}

/// What one pass over the test segments kept.
struct Sample {
    /// The kept positions, in slot order, when the pass sampled them.
    positions: Option<Vec<CanonicalPosition>>,
    /// The kept positions' labels, one list per lambda, in slot order.
    labels: Vec<Vec<f32>>,
    games: u64,
    /// Every position read, K vs K and each game's final one included.
    positions_read: u64,
    /// The positions the reservoir drew from: those kept, but K vs K.
    positions_kept: u64,
}

impl Sample {
    /// The searched positions read: every one but each game's final one, as the segment listings
    /// count them (`pgn_train_stream::segment_files`, `game_files`).
    fn searched(&self) -> u64 {
        self.positions_read - self.games
    }
}

/// Reads `segments` of `games_dir` in order, drawing `capacity` of their searched positions with
/// `FREEZE_SEED`, their labels for each of `labelling`'s arms (a polyline arm's blended at the
/// polyline's lambda for the position's progress), and, with `with_positions`, the positions
/// themselves. `expected` sizes the progress bar.
///
/// K vs K, and each game's final position, is skipped before the reservoir, so it never takes a slot
/// or an index. Every pass skips the same positions, so a label pass added later still draws the same
/// slots as the first.
#[allow(clippy::too_many_arguments)]
fn sample(
    games_dir: &str, segments: &[String], expected: u64, capacity: u64, with_positions: bool, labelling: &Labelling, curve: &Curve,
) -> Sample {
    let mut reservoir = Reservoir::new(capacity, FREEZE_SEED);
    let mut positions: Vec<CanonicalPosition> = Vec::new();
    let mut labels: Vec<Vec<f32>> = vec![Vec::new(); labelling.arms().len()];
    let mut games = 0u64;
    let mut skipped = 0u64;
    let progress = progress::bar(expected, progress::Unit::Count("positions"), "reading test games");
    for segment in segments {
        let pgn_path = format!("{games_dir}/{segment}{SEGMENT_SUFFIX}");
        let mut reader = SelfPlaySegmentReader::open(&pgn_path);
        while let Some((game_index, collected)) = reader.next_game(&pgn_path) {
            let count = collected.position_count();
            // Every pass needs the positions now, to know which are K vs K.
            let (game_positions, keeps) = self_play_game_kept(&collected, game_index);
            let game_labels: Vec<Vec<f32>> = labelling
                .arms()
                .iter()
                .map(|&arm| match arm {
                    ArmLambda::Constant(lambda) => self_play_game_labels(&collected, lambda, game_index, curve),
                    // The direct blend is linear in lambda, so each searched position's label is the
                    // blend of its two ends at its own lambda; the final position, never kept, gets 0.
                    ArmLambda::Polyline => {
                        let at_zero = self_play_game_labels(&collected, 0.0, game_index, curve);
                        let at_one = self_play_game_labels(&collected, 1.0, game_index, curve);
                        let n = collected.scores_pawns().len();
                        (0..at_zero.len())
                            .map(|ply| {
                                if ply >= n {
                                    return 0.0;
                                }
                                let progress = Progress { i: ply as u16, n: n as u16 };
                                blend(at_zero[ply], at_one[ply], labelling.lambda(arm, progress))
                            })
                            .collect()
                    }
                })
                .collect();
            for ply in 0..count {
                if !keeps[ply] || is_bare_kings(&game_positions[ply]) {
                    skipped += 1;
                    continue;
                }
                let Some(slot) = reservoir.next() else { continue };
                if with_positions {
                    place(&mut positions, slot, game_positions[ply].clone());
                }
                for (set, game_set) in labels.iter_mut().zip(&game_labels) {
                    place(set, slot, game_set[ply]);
                }
            }
            games += 1;
            progress.inc(count as u64);
        }
    }
    progress.finish();
    Sample {
        positions: with_positions.then_some(positions),
        labels,
        games,
        positions_read: reservoir.seen + skipped,
        positions_kept: reservoir.seen,
    }
}

/// A zstd encoder over `{path}.tmp`, which `finish_file` renames to `path`: no reader ever opens a
/// half-written file.
fn create_encoder(path: &str) -> zstd::Encoder<'static, BufWriter<File>> {
    let tmp_path = format!("{path}.tmp");
    let file = File::create(&tmp_path).unwrap_or_else(|error| panic!("failed to create {tmp_path}: {error}"));
    zstd::Encoder::new(BufWriter::new(file), COMPRESSION_LEVEL)
        .unwrap_or_else(|error| panic!("failed to open a zstd encoder for {tmp_path}: {error}"))
}

fn finish_file(path: &str, encoder: zstd::Encoder<'static, BufWriter<File>>) {
    let tmp_path = format!("{path}.tmp");
    let mut inner = encoder.finish().unwrap_or_else(|error| panic!("failed to finish {tmp_path}: {error}"));
    inner.flush().unwrap_or_else(|error| panic!("failed to flush {tmp_path}: {error}"));
    drop(inner);
    std::fs::rename(&tmp_path, path).unwrap_or_else(|error| panic!("failed to rename {tmp_path} to {path}: {error}"));
}

fn write_positions(dir: &str, positions: &[CanonicalPosition]) {
    let path = positions_path(dir);
    let mut encoder = create_encoder(&path);
    CacheHeader.write(&mut encoder);
    let progress = progress::bar(positions.len() as u64, progress::Unit::Count("rows"), "writing positions");
    for position in positions {
        write_position(&mut encoder, position);
        progress.inc(1);
    }
    progress.finish();
    finish_file(&path, encoder);
}

/// Writes one label file per arm, each followed by its manifest; the polyline's set also records its
/// polyline, before the manifest that marks it complete.
fn write_label_sets(dir: &str, labelling: &Labelling, labels: &[Vec<f32>], manifest: &Manifest) {
    for (&arm, set) in labelling.arms().iter().zip(labels) {
        assert_eq!(set.len() as u64, manifest.rows, "lambda {arm} has {} labels for {} positions", set.len(), manifest.rows);
        let set_dir = labels_dir(dir, arm);
        std::fs::create_dir_all(&set_dir).unwrap_or_else(|error| panic!("failed to create {set_dir}: {error}"));
        let path = labels_path(dir, arm);
        let mut encoder = create_encoder(&path);
        LabelHeader { lambda: header_lambda(arm) }.write(&mut encoder);
        for &label in set {
            write_label(&mut encoder, label);
        }
        finish_file(&path, encoder);
        if let (ArmLambda::Polyline, Some(polyline)) = (arm, labelling.polyline()) {
            let path = polyline_path(dir);
            std::fs::write(&path, polyline.values_text()).unwrap_or_else(|error| panic!("failed to write {path}: {error}"));
        }
        write_manifest(&labels_manifest_path(dir, arm), manifest);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::polyline::Polyline;
    use temper_chess::labels::{Curve, LC0_FILES};

    /// Lc0's corpus as the tests' fixtures are written: through the frozen curve.
    const LC0_CURVE: Curve = Curve::Atan(LC0_FILES);

    #[test]
    fn a_reservoir_keeps_every_item_while_it_has_free_slots() {
        let mut reservoir = Reservoir::new(5, FREEZE_SEED);
        let slots: Vec<Option<usize>> = (0..5).map(|_| reservoir.next()).collect();
        assert_eq!(slots, vec![Some(0), Some(1), Some(2), Some(3), Some(4)]);
    }

    #[test]
    fn a_reservoir_draws_the_same_slots_under_the_same_seed() {
        let draw = |seed: u64| {
            let mut reservoir = Reservoir::new(100, seed);
            (0..10_000).map(|_| reservoir.next()).collect::<Vec<_>>()
        };
        assert_eq!(draw(FREEZE_SEED), draw(FREEZE_SEED));
        assert_ne!(draw(FREEZE_SEED), draw(FREEZE_SEED + 1));
    }

    #[test]
    fn a_reservoir_keeps_every_item_with_probability_capacity_over_n() {
        // 1,000 slots over 10,000 items: each tenth of the stream should keep about a tenth of the slots.
        let (capacity, n) = (1_000u64, 10_000u64);
        let mut reservoir = Reservoir::new(capacity, FREEZE_SEED);
        let mut owner = vec![0u64; capacity as usize];
        for index in 0..n {
            if let Some(slot) = reservoir.next() {
                owner[slot] = index;
            }
        }
        for tenth in 0..10 {
            let kept = owner.iter().filter(|&&index| index / (n / 10) == tenth).count();
            assert!((60..=140).contains(&kept), "tenth {tenth} kept {kept} of about 100");
        }
    }

    #[test]
    fn a_manifest_reads_back_what_it_wrote() {
        let manifest = Manifest {
            seed: FREEZE_SEED,
            keep: "quiet".to_string(),
            games: 70_123,
            positions: 7_000_456,
            rows: 4_194_304,
            segments: vec!["20261017/gamma-120000".to_string(), "alpha-20261017-130000".to_string()],
        };
        assert_eq!(Manifest::parse(&manifest.text(), "manifest.txt"), manifest);
    }

    #[test]
    #[should_panic(expected = "frozen under another seed")]
    fn a_manifest_under_another_seed_is_refused() {
        Manifest::parse("seed\t1\ngames\t1\npositions\t1\nrows\t1\nsegment\tx\n", "manifest.txt");
    }

    #[test]
    fn a_subset_keeps_exactly_the_rows_asked_and_never_leans_to_the_first_slots() {
        let kept = subset(10_000, 1_000);
        assert_eq!(kept.iter().filter(|&&keep| keep).count(), 1_000);
        // Each tenth of the slots should hold about a tenth of the subset.
        for tenth in kept.chunks(1_000) {
            let count = tenth.iter().filter(|&&keep| keep).count();
            assert!((60..=140).contains(&count), "a tenth of the slots kept {count} of about 100");
        }
        assert_eq!(subset(50, 50), vec![true; 50]);
    }

    #[test]
    fn a_label_set_is_named_after_its_lambda() {
        assert_eq!(labels_path("data/chess/lc0/frozen", ArmLambda::Constant(0.25)), "data/chess/lc0/frozen/lambda-0.25/test.labels");
        assert_eq!(labels_path("data/chess/lc0/frozen", ArmLambda::Polyline), "data/chess/lc0/frozen/lambda-polyline/test.labels");
    }

    /// Constant arms at `lambdas`, as the grid labels them.
    fn at(lambdas: &[f64]) -> Labelling {
        let arms: Vec<ArmLambda> = lambdas.iter().map(|&lambda| ArmLambda::Constant(lambda)).collect();
        Labelling::new(&arms, None).unwrap()
    }

    /// A polyline label set frozen beside a constant one holds, at each slot, the blend of the
    /// constant ends at the polyline's lambda for that position; a polyline measured again is
    /// refused until its set is deleted, while the constant set still reads.
    #[test]
    fn a_polyline_label_set_is_frozen_beside_constants_and_a_new_polyline_refused() {
        let games_dir = fixture_dir("polyline-games");
        let dir = format!("{}/frozen", fixture_dir("polyline-set"));
        let files = write_segments(&games_dir, &[(GAMES.repeat(20), 100), (GAMES.repeat(20), 100)]);
        let polyline = Polyline::new([0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9]);
        let both = Labelling::new(&[ArmLambda::Constant(0.5), ArmLambda::Polyline], Some(polyline)).unwrap();
        ensure(&dir, &games_dir, || Ok(files.clone()), 50, &both, &LC0_CURVE).unwrap();
        assert!(is_complete(&dir, 50, &both));
        let records = read(&dir, 50, &both).unwrap();

        // The hand blend, from the two ends drawn into the same slots, with each slot's progress:
        // the segments read in order, the game's searched positions alone.
        let segments = vec!["gamma-1".to_string(), "gamma-2".to_string()];
        let ends = sample(&games_dir, &segments, 280, 50, false, &at(&[0.0, 1.0]), &LC0_CURVE);
        let mut progress: Vec<Progress> = Vec::new();
        let mut reservoir = Reservoir::new(50, FREEZE_SEED);
        for segment in &segments {
            let path = format!("{games_dir}/{segment}{SEGMENT_SUFFIX}");
            let mut reader = SelfPlaySegmentReader::open(&path);
            while let Some((game_index, collected)) = reader.next_game(&path) {
                let (positions, keeps) = self_play_game_kept(&collected, game_index);
                let n = collected.scores_pawns().len() as u16;
                for ply in 0..collected.position_count() {
                    if !keeps[ply] || is_bare_kings(&positions[ply]) {
                        continue;
                    }
                    if let Some(slot) = reservoir.next() {
                        place(&mut progress, slot, Progress { i: ply as u16, n });
                    }
                }
            }
        }
        for (slot, record) in records.iter().enumerate() {
            let (zero, one) = (ends.labels[0][slot], ends.labels[1][slot]);
            let lambda = polyline.at(usize::from(progress[slot].i), usize::from(progress[slot].n));
            // The constant set is labelled directly at 0.5, so equal to the blend within an f32.
            assert!((record.labels[0] - blend(zero, one, 0.5)).abs() < 1e-6, "slot {slot}");
            assert_eq!(record.labels[1], blend(zero, one, lambda), "slot {slot}");
        }

        let never = || -> Result<Vec<StreamFile>, String> { panic!("a frozen set lists no files") };
        let other = Labelling::new(&[ArmLambda::Polyline], Some(Polyline::new([0.5; 10]))).unwrap();
        assert!(!is_complete(&dir, 50, &other));
        let refused = ensure(&dir, &games_dir, never, 50, &other, &LC0_CURVE).expect_err("another polyline");
        assert!(refused.contains("delete it"), "{refused}");
        assert!(read(&dir, 50, &other).is_err());
        assert_eq!(read(&dir, 50, &at(&[0.5])).unwrap().len(), 50, "the constant set still reads");
        std::fs::remove_dir_all(&games_dir).ok();
        std::fs::remove_dir_all(std::path::Path::new(&dir).parent().unwrap()).ok();
    }

    /// Two games of different lengths and scores, so positions and labels differ along the stream. A
    /// white rook stands on a1, so no position is K vs K.
    const GAMES: &str = concat!(
        "[Result \"1-0\"]\n[FEN \"4k3/8/8/8/8/8/8/R3K3 w - - 0 1\"]\n[SetUp \"1\"]\n\n1. Kd2 {1.50} Kd7 {-1.20}\n1-0\n\n",
        "[Result \"0-1\"]\n[FEN \"4k3/8/8/8/8/8/8/R3K3 w - - 0 1\"]\n[SetUp \"1\"]\n\n1. Ke2 {-0.40} Ke7 {0.30} 2. Kf2 {-0.90}\n0-1\n\n",
    );

    /// The same two games with the kings alone: every position is K vs K.
    const BARE_KINGS_GAMES: &str = concat!(
        "[Result \"1-0\"]\n[FEN \"4k3/8/8/8/8/8/8/4K3 w - - 0 1\"]\n[SetUp \"1\"]\n\n1. Kd2 {1.50} Kd7 {-1.20}\n1-0\n\n",
        "[Result \"0-1\"]\n[FEN \"4k3/8/8/8/8/8/8/4K3 w - - 0 1\"]\n[SetUp \"1\"]\n\n1. Ke2 {-0.40} Ke7 {0.30} 2. Kf2 {-0.90}\n0-1\n\n",
    );

    fn fixture_dir(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("smithy-freeze-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("failed to create the fixture directory");
        dir.to_str().unwrap().replace('\\', "/")
    }

    /// Writes `texts` as segments `gamma-1`, `gamma-2`, ... of `dir`, and returns them as the server
    /// lists them, with their positions.
    fn write_segments(dir: &str, texts: &[(String, u64)]) -> Vec<StreamFile> {
        let mut files = Vec::new();
        for (index, (text, positions)) in texts.iter().enumerate() {
            let path = format!("{dir}/gamma-{}{SEGMENT_SUFFIX}", index + 1);
            let compressed = zstd::encode_all(text.as_bytes(), 3).expect("failed to compress the fixture");
            std::fs::write(&path, compressed).expect("failed to write the fixture");
            files.push(StreamFile { path, positions: *positions });
        }
        files
    }

    #[test]
    fn a_label_pass_added_later_puts_every_label_beside_its_position() {
        let dir = fixture_dir("label-pass");
        let segments = vec!["gamma-1".to_string(), "gamma-2".to_string()];
        write_segments(&dir, &[(GAMES.repeat(20), 140), (GAMES.repeat(20), 140)]);
        // 2 segments x 20 x (3 + 4) positions = 280, 200 of them searched, drawn into 50 slots.
        let first = sample(&dir, &segments, 280, 50, true, &at(&[0.5, 1.0]), &LC0_CURVE);
        assert_eq!((first.games, first.positions_read, first.positions_kept), (80, 280, 200));
        assert_eq!(first.positions.as_ref().map(Vec::len), Some(50));
        assert_eq!(first.labels.iter().map(Vec::len).collect::<Vec<_>>(), vec![50, 50]);

        let later = sample(&dir, &segments, 280, 50, false, &at(&[1.0]), &LC0_CURVE);
        assert!(later.positions.is_none());
        assert_eq!(later.labels[0], first.labels[1], "the same slots hold the same positions' labels");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn k_vs_k_is_read_and_counted_but_never_kept() {
        let dir = fixture_dir("bare-kings");
        let segments = vec!["gamma-1".to_string(), "gamma-2".to_string()];
        write_segments(&dir, &[(GAMES.repeat(20), 140), (BARE_KINGS_GAMES.repeat(20), 140)]);
        // 140 positions with the rook, then 140 of bare kings: all 280 read, as the .meta count them,
        // but the reservoir holds only the first segment's searched ones, 100.
        let sample = sample(&dir, &segments, 280, 500, true, &at(&[1.0]), &LC0_CURVE);
        assert_eq!((sample.games, sample.positions_read, sample.positions_kept), (80, 280, 100));
        let kept = sample.positions.expect("a first freeze samples positions");
        assert_eq!(kept.len(), 100);
        assert!(kept.iter().all(|position| !is_bare_kings(position)));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A freeze writes its files, reads back what it drew, adds a lambda later beside the same
    /// positions, and never reads its games again once frozen.
    #[test]
    fn a_frozen_set_reads_back_and_grows_lambdas_without_redrawing() {
        let games_dir = fixture_dir("frozen-games");
        let dir = format!("{}/frozen", fixture_dir("frozen-set"));
        // 280 positions, 200 searched: listed as their `.meta` less one final position a game.
        let files = write_segments(&games_dir, &[(GAMES.repeat(20), 100), (GAMES.repeat(20), 100)]);

        assert!(read(&dir, 50, &at(&[0.5])).is_err(), "nothing is frozen yet");
        ensure(&dir, &games_dir, || Ok(files.clone()), 50, &at(&[0.5]), &LC0_CURVE).unwrap();
        let first = read(&dir, 50, &at(&[0.5])).unwrap();
        assert_eq!(first.len(), 50);
        assert!(read(&dir, 50, &at(&[1.0])).is_err(), "lambda 1.0 is not frozen yet");

        // Frozen means frozen: the files are never listed again.
        let never = || -> Result<Vec<StreamFile>, String> { panic!("a frozen set lists no files") };
        ensure(&dir, &games_dir, never, 50, &at(&[0.5, 1.0]), &LC0_CURVE).unwrap();
        let both = read(&dir, 50, &at(&[0.5, 1.0])).unwrap();
        let drawn = sample(&games_dir, &["gamma-1".to_string(), "gamma-2".to_string()], 280, 50, false, &at(&[1.0]), &LC0_CURVE);
        for ((record, before), &later) in both.iter().zip(&first).zip(&drawn.labels[0]) {
            assert_eq!(record.canonical.pieces, before.canonical.pieces);
            assert_eq!(record.labels, vec![before.labels[0], later]);
        }
        assert!(ensure(&dir, &games_dir, never, 60, &at(&[0.5]), &LC0_CURVE).is_err(), "frozen smaller");

        // A run asking fewer reads the same seeded subset every time, in the frozen order.
        ensure(&dir, &games_dir, never, 20, &at(&[0.5]), &LC0_CURVE).unwrap();
        let fewer = read(&dir, 20, &at(&[0.5, 1.0])).unwrap();
        assert_eq!(fewer.len(), 20);
        let kept: Vec<&CacheRecord> = both.iter().zip(subset(50, 20)).filter(|(_, keep)| *keep).map(|(record, _)| record).collect();
        for (record, expected) in fewer.iter().zip(kept) {
            assert_eq!((&record.canonical.pieces, &record.labels), (&expected.canonical.pieces, &expected.labels));
        }
        std::fs::remove_dir_all(&games_dir).ok();
        std::fs::remove_dir_all(std::path::Path::new(&dir).parent().unwrap()).ok();
    }

    #[test]
    fn a_set_frozen_before_the_final_position_was_left_out_is_refused() {
        let dir = fixture_dir("old-manifest");
        std::fs::write(manifest_path(&dir), "seed\t20261017\ngames\t1\npositions\t3\nrows\t1\nsegment\tgamma-1\n").unwrap();
        let Err(refused) = read(&dir, 1, &at(&[0.5])) else { panic!("an old set is refused") };
        assert!(refused.contains("delete it"), "{refused}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_set_frozen_keeping_other_positions_is_refused() {
        let games_dir = fixture_dir("quiet-games");
        let dir = format!("{}/frozen", fixture_dir("quiet-set"));
        let files = write_segments(&games_dir, &[(GAMES.repeat(20), 100), (GAMES.repeat(20), 100)]);
        ensure(&dir, &games_dir, || Ok(files), 50, &at(&[0.5]), &LC0_CURVE).unwrap();
        // As the quiet filter's sets were frozen, before it was removed.
        let manifest = std::fs::read_to_string(manifest_path(&dir)).unwrap().replace(&format!("keep\t{KEPT}"), "keep\tquiet");
        std::fs::write(manifest_path(&dir), manifest).unwrap();
        let never = || -> Result<Vec<StreamFile>, String> { panic!("a frozen set lists no files") };
        let refused = ensure(&dir, &games_dir, never, 50, &at(&[0.5, 1.0]), &LC0_CURVE).expect_err("another set's directory");
        assert!(refused.contains("quiet"), "{refused}");
        std::fs::remove_dir_all(&games_dir).ok();
        std::fs::remove_dir_all(std::path::Path::new(&dir).parent().unwrap()).ok();
    }

    #[test]
    fn a_listing_of_every_position_is_refused() {
        let games_dir = fixture_dir("full-count-games");
        let dir = format!("{}/frozen", fixture_dir("full-count-set"));
        // The `.meta` positions themselves, final ones included: not what a listing holds.
        let files = write_segments(&games_dir, &[(GAMES.repeat(20), 140), (GAMES.repeat(20), 140)]);
        let refused = ensure(&dir, &games_dir, || Ok(files), 50, &at(&[0.5]), &LC0_CURVE).expect_err("a full count");
        assert!(refused.contains("200 searched positions, but their .meta say 280"), "{refused}");
        std::fs::remove_dir_all(&games_dir).ok();
        std::fs::remove_dir_all(std::path::Path::new(&dir).parent().unwrap()).ok();
    }

    #[test]
    fn too_few_positions_kept_are_refused_not_frozen_smaller() {
        let games_dir = fixture_dir("too-few-games");
        let dir = format!("{}/frozen", fixture_dir("too-few-set"));
        // 200 searched positions, enough for 150, but 100 of them bare kings: only 100 kept.
        let files = write_segments(&games_dir, &[(GAMES.repeat(20), 100), (BARE_KINGS_GAMES.repeat(20), 100)]);
        let refused = ensure(&dir, &games_dir, || Ok(files), 150, &at(&[0.5]), &LC0_CURVE).expect_err("too few");
        assert!(refused.contains("more test games"), "{refused}");
        assert!(!std::path::Path::new(&manifest_path(&dir)).exists(), "nothing frozen");
        std::fs::remove_dir_all(&games_dir).ok();
        std::fs::remove_dir_all(std::path::Path::new(&dir).parent().unwrap()).ok();
    }
}
