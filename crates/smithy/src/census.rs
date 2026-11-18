//! `smithy king-buckets`: where the kings stand in Lc0's frozen test set and in its training corpus,
//! by the halfk network's king buckets. A one-off check, run by hand, read-only, on the CPU.
//!
//! A halfk network keeps 32 king buckets a perspective, each with rows of its own, trained only by
//! the positions that put that perspective's king there. Counting the kings answers two questions:
//! whether the frozen test set is sound (its distribution should match the corpus's up to noise; a
//! clear difference would mean its sampling went wrong), and how thin the rare buckets are, so how
//! many positions their rows see a pass over the corpus. Only the kings are read: never a label, never
//! a loss, so nothing here can steer a choice made by the test loss.
//!
//! The corpus is sampled, not read whole (that is hours of PGN on the CPU): segments spread evenly
//! over it, so the earliest tars do not stand for all of it, each position of them kept at random
//! under a fixed seed, by the rule the test set was drawn under (every searched position, K vs K left
//! out).

use fattorini::board::color::Color;
use fattorini::features::{halfk, oriented_square, CanonicalPosition};

use super::cache::is_bare_kings;
use super::game_labels::{self_play_game_kept, SelfPlaySegmentReader};
use super::pgn_stream::{splitmix64, StreamFile};
use super::{freeze, pgn_train_stream, preprocessing_config, remote};

/// The command, in no usage line: a check run by hand, as `lc0`'s rewrite is.
pub(crate) const COMMAND: &str = "king-buckets";

const BUCKETS: usize = halfk::BUCKETS;

/// Segments read for every position wanted, at most: enough spread that a few segments' games do not
/// stand for the corpus, few enough that the read stays minutes.
const SEGMENTS_PER_SAMPLE: u64 = 4;

/// Fixed, so a census read twice keeps the same positions.
const SAMPLE_SEED: u64 = 0xC0FF_EE11;

/// The corpus a full run trains on today, and the one considered (4 x 1024^3), for each bucket's
/// positions a pass.
const CORPORA: [(&str, f64); 2] = [("today's corpus", 0.0), ("4 x 1024^3", 4_294_967_296.0)];

/// The two perspectives a position's accumulators take, each with its own king's bucket.
const PERSPECTIVES: [&str; 2] = ["side to move", "opponent"];

/// Counts of positions by king bucket, a row per perspective.
#[derive(Clone, Debug, PartialEq)]
struct Census {
    counts: [[u64; BUCKETS]; 2],
    positions: u64,
}

impl Census {
    fn new() -> Census {
        Census { counts: [[0; BUCKETS]; 2], positions: 0 }
    }

    /// `position`'s two kings, each in its own perspective's bucket.
    fn add(&mut self, position: &CanonicalPosition) {
        let side = position.side_to_move;
        for (row, color) in [side, side.opposite()].into_iter().enumerate() {
            let king = match color {
                Color::White => position.white_king,
                Color::Black => position.black_king,
            };
            let (bucket, _) = halfk::king_bucket(oriented_square(king, color));
            self.counts[row][bucket] += 1;
        }
        self.positions += 1;
    }

    fn share(&self, row: usize, bucket: usize) -> f64 {
        self.counts[row][bucket] as f64 / self.positions.max(1) as f64
    }
}

pub(crate) fn run(args: &[String]) {
    let sample = match args {
        [] => None,
        [flag, count] if flag == "--sample" => match count.replace('_', "").parse::<u64>() {
            Ok(count) if count > 0 => Some(count),
            _ => fail(&format!("--sample takes a positive count of positions, not {count}")),
        },
        _ => fail("usage: smithy king-buckets [--sample <positions>], the corpus's sample the test set's size by default"),
    };
    match census(sample) {
        Ok(report) => println!("{report}"),
        Err(message) => fail(&message),
    }
}

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(1);
}

/// The test set counted whole, then the corpus sampled, `sample` positions or the test set's count,
/// and the report comparing them.
fn census(sample: Option<u64>) -> Result<String, String> {
    let root = preprocessing_config::LC0_ROOT;
    let frozen = remote::remote_path(&format!("{root}/frozen"));
    let mut test = Census::new();
    for position in freeze::read_positions(&frozen)? {
        test.add(&position);
    }
    let files = pgn_train_stream::training_files(root)?;
    let corpus_positions: u64 = files.iter().map(|file| file.positions).sum();
    let corpus = sample_corpus(&files, sample.unwrap_or(test.positions))?;
    Ok(report(&test, &corpus, corpus_positions))
}

/// The segments to read for `wanted` positions out of `files`, by index, evenly spread: about
/// `SEGMENTS_PER_SAMPLE` times the segments `wanted` fills, every one when that is more than there are.
fn spread(files: &[StreamFile], wanted: u64) -> Vec<usize> {
    let total: u64 = files.iter().map(|file| file.positions).sum();
    let per_file = (total / files.len().max(1) as u64).max(1);
    let count = (wanted.saturating_mul(SEGMENTS_PER_SAMPLE).div_ceil(per_file) as usize).clamp(1, files.len());
    let mut picked: Vec<usize> = (0..count).map(|index| index * files.len() / count).collect();
    picked.dedup();
    picked
}

/// About `wanted` of the corpus's kept positions, from the segments `spread` picks, each kept at the
/// probability that gives `wanted` from all of them.
fn sample_corpus(files: &[StreamFile], wanted: u64) -> Result<Census, String> {
    let picked = spread(files, wanted);
    let available: u64 = picked.iter().map(|&index| files[index].positions).sum();
    let probability = (wanted as f64 / available.max(1) as f64).min(1.0);
    let threshold = (probability * u64::MAX as f64) as u64;
    let mut census = Census::new();
    let mut seen: u64 = 0;
    let progress = progress::bar(available, progress::Unit::Count("positions"), &format!("sampling {} segments of the corpus", picked.len()));
    for &index in &picked {
        let path = &files[index].path;
        let mut reader = SelfPlaySegmentReader::open(path);
        while let Some((game_index, collected)) = reader.next_game(path) {
            let (positions, keeps) = self_play_game_kept(&collected, game_index);
            for (position, kept) in positions.iter().zip(keeps) {
                if !kept || is_bare_kings(position) {
                    continue;
                }
                seen += 1;
                progress.inc(1);
                if splitmix64(SAMPLE_SEED ^ seen) <= threshold {
                    census.add(position);
                }
            }
        }
    }
    progress.finish();
    if census.positions == 0 {
        return Err("the corpus's sample kept no position".to_string());
    }
    Ok(census)
}

/// A board of the 32 buckets, rank 8 on top, files a to d (e to h mirrored onto them), each square
/// `value` of its bucket.
fn board(title: &str, value: impl Fn(usize) -> String) -> String {
    let mut text = format!("{title}\n        a         b         c         d\n");
    for rank in (0..8).rev() {
        text.push_str(&format!("{} ", rank + 1));
        for file in 0..4 {
            text.push_str(&format!("{:>10}", value(rank * 4 + file)));
        }
        text.push('\n');
    }
    text
}

/// How far apart two shares of a bucket are, in standard errors of their difference, by the pooled
/// share: a sign of which set holds more, its size how surprising.
fn z_score(test: &Census, corpus: &Census, row: usize, bucket: usize) -> f64 {
    let (n1, n2) = (test.positions as f64, corpus.positions as f64);
    let pooled = (test.counts[row][bucket] + corpus.counts[row][bucket]) as f64 / (n1 + n2);
    let error = (pooled * (1.0 - pooled) * (1.0 / n1 + 1.0 / n2)).sqrt();
    if error == 0.0 {
        return 0.0;
    }
    (test.share(row, bucket) - corpus.share(row, bucket)) / error
}

fn report(test: &Census, corpus: &Census, corpus_positions: u64) -> String {
    let percent = |share: f64| format!("{:.3}%", 100.0 * share);
    let mut text = format!(
        "King buckets: the frozen test set, {} positions, against the corpus, {} sampled of {}.\n\
         A bucket is a king's square for its own perspective, files e to h mirrored onto d to a.\n",
        progress::grouped(test.positions),
        progress::grouped(corpus.positions),
        progress::grouped(corpus_positions)
    );
    for (row, perspective) in PERSPECTIVES.iter().enumerate() {
        text.push_str(&format!("\n== The {perspective}'s king ==\n\n"));
        text.push_str(&board("The test set, share of positions:", |bucket| percent(test.share(row, bucket))));
        text.push('\n');
        text.push_str(&board("The corpus, share of positions:", |bucket| percent(corpus.share(row, bucket))));
        text.push('\n');
        text.push_str(&board("Test minus corpus, in standard errors:", |bucket| format!("{:+.1}", z_score(test, corpus, row, bucket))));
        let largest = (0..BUCKETS)
            .map(|bucket| (bucket, z_score(test, corpus, row, bucket)))
            .max_by(|left, right| left.1.abs().total_cmp(&right.1.abs()))
            .expect("32 buckets");
        text.push_str(&format!("The largest difference: bucket {}, {:+.1} standard errors.\n", square_name(largest.0), largest.1));
        let rarest = (0..BUCKETS).min_by_key(|&bucket| corpus.counts[row][bucket]).expect("32 buckets");
        let share = corpus.share(row, rarest);
        text.push_str(&format!(
            "The rarest in the corpus: bucket {}, {} sampled positions, {}.\n",
            square_name(rarest),
            progress::grouped(corpus.counts[row][rarest]),
            percent(share)
        ));
        for (name, size) in CORPORA {
            let size = if size == 0.0 { corpus_positions as f64 } else { size };
            text.push_str(&format!(
                "  its positions a pass over {name} ({}): about {}\n",
                progress::grouped(size as u64),
                progress::grouped((share * size).round() as u64)
            ));
        }
    }
    text
}

/// A bucket by the square of its king on files a to d, `a1` for 0, `d8` for 31.
fn square_name(bucket: usize) -> String {
    let (rank, file) = (bucket / 4, bucket % 4);
    format!("{}{}", (b'a' + file as u8) as char, rank + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fattorini::board::Board;
    use fattorini::features::extract_canonical;

    fn position(fen: &str) -> CanonicalPosition {
        let (board, _) = Board::from_fen(fen).expect("a valid FEN");
        extract_canonical(&board)
    }

    #[test]
    fn each_perspective_counts_its_own_king_oriented_and_mirrored() {
        let mut census = Census::new();
        // White to move, its king on g1 (mirrored onto b1, bucket 1); Black's on b8, its own rank 1
        // once oriented (bucket 1 too).
        census.add(&position("1k6/8/8/8/8/8/8/6K1 w - - 0 1"));
        assert_eq!(census.counts[0][1], 1, "the side to move, white, on g1: b1 mirrored");
        assert_eq!(census.counts[1][1], 1, "the opponent, black, on b8: b1 oriented");
        // Black to move, its king on h1, its own rank 8 once oriented (h8 mirrored onto a8, bucket 28);
        // White's on d4 (bucket 15).
        census.add(&position("8/8/8/8/3K4/8/8/7k b - - 0 1"));
        assert_eq!(census.counts[0][28], 1, "the side to move, black, on h1: a8 oriented and mirrored");
        assert_eq!(census.counts[1][15], 1, "the opponent, white, on d4");
        assert_eq!(census.positions, 2);
        assert_eq!((square_name(0), square_name(13), square_name(31)), ("a1".to_string(), "b4".to_string(), "d8".to_string()));
    }

    #[test]
    fn the_sample_s_segments_are_spread_over_the_corpus() {
        let files: Vec<StreamFile> = (0..100).map(|index| StreamFile { path: format!("s{index}"), positions: 1000 }).collect();
        // 2,000 wanted, 4 times over: 8 segments of 1,000, evenly spread.
        assert_eq!(spread(&files, 2000), vec![0, 12, 25, 37, 50, 62, 75, 87]);
        assert_eq!(spread(&files, 1_000_000).len(), 100, "every segment when more are wanted than there are");
        assert_eq!(spread(&files, 1), vec![0], "at least one");
    }

    #[test]
    fn identical_shares_are_no_difference_and_a_gap_is_signed() {
        let mut test = Census::new();
        let mut corpus = Census::new();
        let center = position("8/8/8/8/3K4/8/8/7k w - - 0 1");
        let corner = position("8/8/8/8/8/8/8/K6k w - - 0 1");
        for _ in 0..100 {
            test.add(&center);
            corpus.add(&center);
        }
        let bucket = 15; // d4
        assert_eq!(z_score(&test, &corpus, 0, bucket), 0.0);
        for _ in 0..100 {
            corpus.add(&corner);
        }
        assert!(z_score(&test, &corpus, 0, bucket) > 5.0, "the test set holds d4 more often");
    }
}
