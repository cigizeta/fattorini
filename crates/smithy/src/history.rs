use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};

/// The literal first line of every `history.tsv` -- written once, by `append`'s very first call
/// for a given `path`, and checked (not just skipped) by `read_all`, so a genuinely unexpected
/// column layout fails loudly (CLAUDE.md rule 4) instead of being silently misparsed as data.
const HEADER: &str = "step\tlearning_rate\ttrain_loss\ttest_loss\ttrain_duration_secs\ttest_duration_secs";

/// One evaluation round's own recorded losses -- everything a later "pick the best N
/// checkpoints" step needs to rank every round a run ever completed, without reloading each
/// one's own weights just to find out how it did. See `history.rs`'s own module doc for why this
/// exists as a full per-round log rather than tracking only a single running "best." `step` is
/// the optimizer's own cumulative batch count as of this evaluation (`main.rs`'s own unit for
/// "how far into the run this happened," now that training no longer runs in discrete,
/// dataset-sized epochs). `train_duration_secs`/`test_duration_secs` are that round's own
/// wall-clock cost, kept separate rather than summed into one figure: the two phases have very
/// different row counts (a full training round's `steps_per_eval * batch_size` rows versus one
/// held-out split's own, usually much smaller, row count), so folding them together would make
/// per-row throughput uncomputable from this file alone -- both as actually observed, not
/// something to reconstruct later from terminal scrollback (CLAUDE.md rule 5: speed is measured,
/// not assumed). `learning_rate` is the rate the round trained at, for reading: which stage of the
/// run's list it was in is replayed from the test losses (`stages.rs`), never from this column.
pub struct EvalRecord {
    pub step: u64,
    pub learning_rate: f64,
    pub train_loss: f64,
    pub test_loss: f64,
    pub train_duration_secs: f64,
    pub test_duration_secs: f64,
}

/// Appends one line to `path` (creating it, with `HEADER` as its first line, if this is the first
/// evaluation) -- plain tab-separated text, not a binary format: nothing here is performance-
/// sensitive (one append per evaluation round, not per row), and a human-readable log a
/// developer can `cat`/`tail` directly is worth more here than the marginal compactness a binary
/// format would buy.
pub fn append(path: &str, record: &EvalRecord) {
    // Checked before opening (`OpenOptions::create` would otherwise make this always `false`,
    // since the file exists by the time it's checked) -- the header belongs only at the very
    // start of a brand new file, never repeated on a later, resumed invocation's own first append.
    let file_is_new = !std::path::Path::new(path).exists();
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap_or_else(|error| panic!("failed to open {path} for appending: {error}"));
    if file_is_new {
        writeln!(file, "{HEADER}").unwrap_or_else(|error| panic!("failed to write the header to {path}: {error}"));
    }
    writeln!(
        file, "{}\t{:e}\t{}\t{}\t{}\t{}", record.step, record.learning_rate, record.train_loss, record.test_loss,
        record.train_duration_secs, record.test_duration_secs
    )
    .unwrap_or_else(|error| panic!("failed to append to {path}: {error}"));
}

/// The piece counts of `pieces.tsv`'s columns, kings included: 3 to 32, K vs K (2) being never
/// frozen.
pub const PIECE_COUNTS: std::ops::RangeInclusive<usize> = 3..=32;

/// `pieces.tsv`'s first line: `step`, then `p3` to `p32`.
fn pieces_header() -> String {
    let mut header = "step".to_string();
    for pieces in PIECE_COUNTS {
        header.push_str(&format!("\tp{pieces}"));
    }
    header
}

/// Appends one round's test loss per piece count to `path`, a lambda run's `pieces.tsv`, creating
/// it with its header first: one wide row, `step` then a column per count of `PIECE_COUNTS`, empty
/// where `losses` has no entry (no test position of that count). For reading only: nothing is
/// replayed from it, so a row missing after a crash between this and `append` does no harm.
pub fn append_pieces(path: &str, step: u64, losses: &BTreeMap<usize, f64>) {
    let file_is_new = !std::path::Path::new(path).exists();
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap_or_else(|error| panic!("failed to open {path} for appending: {error}"));
    if file_is_new {
        writeln!(file, "{}", pieces_header()).unwrap_or_else(|error| panic!("failed to write the header to {path}: {error}"));
    }
    let mut line = step.to_string();
    for pieces in PIECE_COUNTS {
        line.push('\t');
        if let Some(loss) = losses.get(&pieces) {
            line.push_str(&loss.to_string());
        }
    }
    writeln!(file, "{line}").unwrap_or_else(|error| panic!("failed to append to {path}: {error}"));
}

/// Reads every recorded evaluation back, in file order (which is step order, since `append` is
/// the only writer and it's only ever called once per completed evaluation round, in sequence)
/// -- the read side a later "best N" selection tool needs; not yet called anywhere in this
/// codebase, since that tool doesn't exist yet (see `training.md`'s own "Candidate selection"
/// section).
pub fn read_all(path: &str) -> Vec<EvalRecord> {
    let file = std::fs::File::open(path).unwrap_or_else(|error| panic!("failed to open {path}: {error}"));
    let mut lines = BufReader::new(file).lines();

    let header = lines
        .next()
        .unwrap_or_else(|| panic!("{path} is empty, expected a header row"))
        .unwrap_or_else(|error| panic!("failed to read {path}'s own header row: {error}"));
    assert_eq!(header, HEADER, "{path} has an unexpected header row: {header:?}");

    let mut records = Vec::new();
    for line in lines {
        let line = line.unwrap_or_else(|error| panic!("failed to read a line from {path}: {error}"));
        let mut columns = line.split('\t');
        records.push(EvalRecord {
            step: parse_column(&line, path, columns.next(), "step"),
            learning_rate: parse_column(&line, path, columns.next(), "learning_rate"),
            train_loss: parse_column(&line, path, columns.next(), "train_loss"),
            test_loss: parse_column(&line, path, columns.next(), "test_loss"),
            train_duration_secs: parse_column(&line, path, columns.next(), "train_duration_secs"),
            test_duration_secs: parse_column(&line, path, columns.next(), "test_duration_secs"),
        });
    }
    records
}

/// A single closure can't serve all three columns here since it isn't generic (Rust infers one
/// concrete return type per closure from its first use) -- a real generic function is needed so
/// `step`'s `u64` and the two loss columns' `f64` can share this parsing logic.
fn parse_column<T: std::str::FromStr>(line: &str, path: &str, value: Option<&str>, name: &str) -> T {
    value
        .unwrap_or_else(|| panic!("{path} has a line missing its {name} column: {line:?}"))
        .parse()
        .unwrap_or_else(|_| panic!("{path} has a non-numeric {name} column: {line:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A record at `step` with `test_loss`, the only two fields the header and append tests vary.
    fn sample(step: u64, test_loss: f64) -> EvalRecord {
        EvalRecord {
            step, learning_rate: 1.0e-3, train_loss: 1.0, test_loss, train_duration_secs: 9.0, test_duration_secs: 1.0,
        }
    }

    #[test]
    fn append_then_read_all_round_trips_every_field() {
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("smithy-history-test-{pid}.tsv"));
        let path = path.to_str().unwrap();

        append(
            path,
            &EvalRecord {
                step: 8192, learning_rate: 3.0e-3, train_loss: 9.72, test_loss: 10.51, train_duration_secs: 40.1,
                test_duration_secs: 2.2,
            },
        );
        append(
            path,
            &EvalRecord {
                step: 16384, learning_rate: 1.0e-3, train_loss: 8.67, test_loss: 10.15, train_duration_secs: 39.6,
                test_duration_secs: 2.2,
            },
        );

        let records = read_all(path);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].step, 8192);
        assert_eq!(records[0].learning_rate, 3.0e-3);
        assert_eq!(records[1].learning_rate, 1.0e-3);
        assert_eq!(records[0].train_loss, 9.72);
        assert_eq!(records[0].test_loss, 10.51);
        assert_eq!(records[0].train_duration_secs, 40.1);
        assert_eq!(records[0].test_duration_secs, 2.2);
        assert_eq!(records[1].step, 16384);
        assert_eq!(records[1].test_loss, 10.15);
        assert_eq!(records[1].train_duration_secs, 39.6);
        assert_eq!(records[1].test_duration_secs, 2.2);

        std::fs::remove_file(path).ok();
    }

    #[test]
    fn append_writes_the_header_exactly_once_as_the_files_first_line() {
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("smithy-history-test-{pid}-header.tsv"));
        let path = path.to_str().unwrap();

        append(path, &sample(8192, 1.5));
        append(path, &sample(16384, 1.4));

        let contents = std::fs::read_to_string(path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines[0], HEADER, "expected the header as the file's very first line");
        assert_eq!(lines.len(), 3, "expected one header line plus one line per appended evaluation, not a repeated header");

        std::fs::remove_file(path).ok();
    }

    #[test]
    fn append_extends_an_existing_file_rather_than_overwriting_it() {
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("smithy-history-test-{pid}-extend.tsv"));
        let path = path.to_str().unwrap();

        append(path, &sample(8192, 1.5));
        // Simulates a resumed run: a fresh process appending to a history file an earlier
        // invocation already started -- must add to it, not truncate what's already there.
        append(path, &sample(16384, 1.4));

        let records = read_all(path);
        assert_eq!(records.len(), 2);

        std::fs::remove_file(path).ok();
    }

    #[test]
    fn pieces_tsv_is_wide_with_its_header_once_and_empty_fields_where_no_count_was_tested() {
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("smithy-history-test-{pid}-pieces.tsv"));
        let path = path.to_str().unwrap();

        append_pieces(path, 0, &BTreeMap::from([(3, 0.5), (32, 0.25)]));
        append_pieces(path, 4096, &BTreeMap::from([(4, 0.125)]));

        let contents = std::fs::read_to_string(path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 3, "one header, one row per round");
        let header: Vec<&str> = lines[0].split('\t').collect();
        assert_eq!((header.len(), header[0], header[1], header[30]), (31, "step", "p3", "p32"));
        let first: Vec<&str> = lines[1].split('\t').collect();
        assert_eq!((first.len(), first[0], first[1], first[2], first[30]), (31, "0", "0.5", "", "0.25"));
        let second: Vec<&str> = lines[2].split('\t').collect();
        assert_eq!((second.len(), second[0], second[1], second[2]), (31, "4096", "", "0.125"));

        std::fs::remove_file(path).ok();
    }
}
