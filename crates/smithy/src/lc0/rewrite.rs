//! The one-off rewrite of Lc0's corpus from the curve it was converted through, `labels::LC0_FILES`
//! (one pawn at 1/2, ceiling 32 pawns), to Lc0's own, `labels::LC0` (`cp = 90 tan(1.5637541897 Q)`),
//! without downloading Lc0 again: every comment's score decoded to Q and encoded again, both at the
//! files' 2 decimals. The two roundings cost at most about 0.007 of Q, near equality where both
//! curves are steepest, far below the noise of Q itself (the developer, 2026-11-04).
//!
//! Run once, by the developer: `smithy lc0-rewrite`. Each segment is written beside itself as
//! `<segment>.rewritten`, recorded in `rewrite-done.txt`, then renamed over the original, so a run
//! cut short resumes without ever converting a segment twice: a segment recorded is never decoded
//! again, only its `.rewritten` renamed if still there. `curve.txt`, written last, says the corpus
//! is in Lc0's curve; training refuses Lc0's corpus without it (`corpus_curve`).

use std::collections::HashSet;
use std::fs;
use std::io::{BufWriter, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use kalo_chess::labels::{Curve, LC0, LC0_FILES};

use super::{fail, remote, COMPRESSION_LEVEL, LC0_ROOT};
use crate::pgn_train_stream;

/// The smithy's subcommand for the rewrite, run by hand once.
pub(crate) const REWRITE_COMMAND: &str = "lc0-rewrite";

/// Written to the remote when every segment is in Lc0's curve.
const CURVE_PATH: &str = "data/chess/lc0/curve.txt";
const CURVE_TEXT: &str = "lc0 a 0.90 b 1.5637541897\n";

/// The segments rewritten so far, one remote path a line; deleted once `CURVE_PATH` is written.
const DONE_PATH: &str = "data/chess/lc0/rewrite-done.txt";

const REWRITTEN_SUFFIX: &str = ".rewritten";

/// Whether Lc0's corpus has been rewritten into Lc0's curve.
pub(crate) fn is_rewritten() -> bool {
    Path::new(&remote::remote_path(CURVE_PATH)).exists()
}

/// Records that the corpus is in Lc0's curve: for a Convert starting an empty corpus, which writes
/// every segment through it.
pub(super) fn mark_rewritten() -> Result<(), String> {
    remote::write_small(CURVE_PATH, CURVE_TEXT).map_err(|error| format!("failed to write {CURVE_PATH}: {error}"))
}

/// What Lc0's corpus is read through: Lc0's curve once rewritten; an error before, so no file is ever
/// read through the wrong curve.
pub(crate) fn corpus_curve() -> Result<Curve, String> {
    if is_rewritten() {
        Ok(Curve::Atan(*LC0))
    } else {
        Err(format!(
            "Lc0's corpus is still written through the old curve: run `smithy {REWRITE_COMMAND}` once, which writes {} when done",
            remote::remote_path(CURVE_PATH)
        ))
    }
}

/// A segment's path from `LC0_ROOT` on, how `DONE_PATH` is matched: the same on every machine,
/// wherever its remote is mounted, so a run resumed on another machine never converts a recorded
/// segment twice. Lines recorded whole, with the mount, match too.
fn segment_key(path: &str) -> &str {
    let root = format!("{LC0_ROOT}/");
    path.rfind(&root).map_or(path, |start| &path[start..])
}

/// One score as the files write it, in pawns, from the old curve to Lc0's, at 2 decimals.
fn rewrite_score(pawns: f64) -> String {
    format!("{:.2}", LC0.pawns(LC0_FILES.score(pawns)))
}

/// A segment's text with every comment's first word, its score, rewritten (`rewrite_score`); the rest
/// of each comment (the best move, the visit counts) and everything outside comments untouched.
pub(crate) fn rewrite_text(text: &str) -> Result<String, String> {
    let mut out = String::with_capacity(text.len() + text.len() / 16);
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..=open]);
        let after = &rest[open + 1..];
        let close = after.find('}').ok_or_else(|| "a comment never closed".to_string())?;
        let comment = &after[..close];
        let (score, tail) = match comment.split_once(' ') {
            Some((score, tail)) => (score, Some(tail)),
            None => (comment, None),
        };
        let pawns: f64 = score.parse().map_err(|_| format!("the comment {{{comment}}} starts with no score"))?;
        out.push_str(&rewrite_score(pawns));
        if let Some(tail) = tail {
            out.push(' ');
            out.push_str(tail);
        }
        rest = &after[close..];
    }
    out.push_str(rest);
    Ok(out)
}

/// `path` rewritten into `<path>.rewritten`, synced; the original untouched.
fn rewrite_segment(path: &str) -> Result<(), String> {
    let mut text = String::new();
    zstd::Decoder::new(fs::File::open(path).map_err(|error| format!("{path}: {error}"))?)
        .and_then(|mut decoder| decoder.read_to_string(&mut text))
        .map_err(|error| format!("failed to read {path}: {error}"))?;
    let rewritten = rewrite_text(&text).map_err(|error| format!("{path}: {error}"))?;
    let tmp = format!("{path}{REWRITTEN_SUFFIX}");
    let output = fs::File::create(&tmp).map_err(|error| format!("failed to create {tmp}: {error}"))?;
    let mut encoder = zstd::Encoder::new(BufWriter::new(output), COMPRESSION_LEVEL)
        .and_then(|mut encoder| {
            // Single-threaded: the segments themselves are rewritten in parallel (`rewrite_all`),
            // which keeps every core busy where zstd's own jobs at level 19 left most idle.
            encoder.include_checksum(true)?;
            Ok(encoder)
        })
        .map_err(|error| format!("failed to open a zstd encoder for {tmp}: {error}"))?;
    encoder.write_all(rewritten.as_bytes()).map_err(|error| format!("failed to write {tmp}: {error}"))?;
    let writer = encoder.finish().map_err(|error| format!("failed to finish {tmp}: {error}"))?;
    let output = writer.into_inner().map_err(|error| format!("failed to flush {tmp}: {error}"))?;
    output.sync_all().map_err(|error| format!("failed to sync {tmp}: {error}"))
}

/// One segment brought into Lc0's curve: rewritten and recorded unless `done` holds it, then its
/// `.rewritten` renamed over it if still there. Recorded before the rename, so a stop between the two
/// leaves a recorded `.rewritten` that the next run only renames. `record` serializes the done-file's
/// appends across workers, one whole line each.
fn finish_segment(path: &str, done: &HashSet<String>, done_path: &str, record: &Mutex<()>) -> Result<(), String> {
    let tmp = format!("{path}{REWRITTEN_SUFFIX}");
    if !done.contains(segment_key(path)) {
        rewrite_segment(path)?;
        let _guard = record.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(done_path)
            .map_err(|error| format!("failed to open {done_path}: {error}"))?;
        writeln!(file, "{path}").map_err(|error| format!("failed to write {done_path}: {error}"))?;
        file.sync_all().map_err(|error| format!("failed to sync {done_path}: {error}"))?;
    }
    if Path::new(&tmp).exists() {
        fs::rename(&tmp, path).map_err(|error| format!("failed to rename {tmp}: {error}"))?;
    }
    Ok(())
}

/// Every segment of `paths` finished (`finish_segment`) by `workers` threads taking them in turn.
/// They finish out of order, which the done-file, read as a set, does not mind. The first error stops
/// every worker at its next segment and is returned.
fn rewrite_all(paths: &[String], done: &HashSet<String>, done_path: &str, workers: usize) -> Result<(), String> {
    let next = AtomicUsize::new(0);
    let finished = AtomicUsize::new(0);
    let record = Mutex::new(());
    let error: Mutex<Option<String>> = Mutex::new(None);
    std::thread::scope(|scope| {
        for _ in 0..workers.max(1) {
            scope.spawn(|| loop {
                if error.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).is_some() {
                    return;
                }
                let index = next.fetch_add(1, Ordering::SeqCst);
                let Some(path) = paths.get(index) else { return };
                match finish_segment(path, done, done_path, &record) {
                    Ok(()) => {
                        let count = finished.fetch_add(1, Ordering::SeqCst) + 1;
                        println!("{count} of {} segments rewritten: {path}", paths.len());
                    }
                    Err(message) => {
                        error.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get_or_insert(message);
                        return;
                    }
                }
            });
        }
    });
    match error.into_inner().unwrap_or_else(|poisoned| poisoned.into_inner()) {
        Some(message) => Err(message),
        None => Ok(()),
    }
}

/// The rewrite: every training and test segment of Lc0's corpus, then `CURVE_PATH`.
pub(crate) fn run(rest: &[String]) {
    if !rest.is_empty() {
        fail(format!("{REWRITE_COMMAND} takes nothing"));
    }
    if let Err(error) = remote::check_mounted() {
        fail(error.to_string());
    }
    if is_rewritten() {
        println!("Lc0's corpus is already in Lc0's curve: {} says so", remote::remote_path(CURVE_PATH));
        return;
    }
    let mut files = pgn_train_stream::training_files(LC0_ROOT).unwrap_or_else(|error| fail(error));
    files.extend(pgn_train_stream::test_files(LC0_ROOT).unwrap_or_else(|error| fail(error)));
    let done_path = remote::remote_path(DONE_PATH);
    let recorded = fs::read_to_string(&done_path).unwrap_or_default();
    // A record cut short mid-line by a stop would glue the next path onto it, so that segment would
    // never read as done and a later run would convert it twice: the line is ended first. The cut
    // path itself matches no segment, so its segment, never renamed, is rewritten again.
    if !recorded.is_empty() && !recorded.ends_with('\n') {
        let mut record = fs::OpenOptions::new().append(true).open(&done_path).unwrap_or_else(|error| fail(format!("failed to open {done_path}: {error}")));
        writeln!(record).unwrap_or_else(|error| fail(format!("failed to write {done_path}: {error}")));
        record.sync_all().unwrap_or_else(|error| fail(format!("failed to sync {done_path}: {error}")));
    }
    let done: HashSet<String> = recorded.lines().map(|line| segment_key(line).to_string()).collect();
    let paths: Vec<String> = files.into_iter().map(|file| file.path).collect();
    let workers = std::thread::available_parallelism().map_or(1, |count| count.get()).saturating_sub(1).max(1);
    println!("rewriting {} segments with {workers} workers", paths.len());
    rewrite_all(&paths, &done, &done_path, workers).unwrap_or_else(|error| fail(error));
    remote::write_small(CURVE_PATH, CURVE_TEXT).unwrap_or_else(|error| fail(format!("failed to write {CURVE_PATH}: {error}")));
    fs::remove_file(&done_path).ok();
    println!("Lc0's corpus is in Lc0's curve: {} says so", remote::remote_path(CURVE_PATH));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_comment_s_score_is_rewritten_and_the_rest_kept() {
        let text = "[Event \"lc0 test91\"]\n\n1. e4 {0.12 e7e5 3,40,7} e5 {-0.08 g1f3} 2. Nf3 {32.00 b8c6} 1-0\n\n";
        let rewritten = rewrite_text(text).unwrap();
        let expected = format!(
            "[Event \"lc0 test91\"]\n\n1. e4 {{{} e7e5 3,40,7}} e5 {{{} g1f3}} 2. Nf3 {{{} b8c6}} 1-0\n\n",
            rewrite_score(0.12),
            rewrite_score(-0.08),
            rewrite_score(32.0)
        );
        assert_eq!(rewritten, expected);
        assert_eq!(rewrite_score(32.0), format!("{:.2}", LC0.pawns(1.0)), "Q = 1 lands on Lc0's ceiling");
    }

    /// Read back through Lc0's curve, a rewritten score is the Q the old one meant, within the two
    /// roundings.
    #[test]
    fn a_rewritten_score_keeps_its_q() {
        let tolerance = 0.005 / (LC0_FILES.a() * LC0_FILES.k()) + 0.005 / (LC0.a() * LC0.k()) + 1e-9;
        for stored in [-31.5, -2.0, -0.01, 0.0, 0.37, 1.0, 5.25, 32.0] {
            let rewritten: f64 = rewrite_score(stored).parse().unwrap();
            assert!((LC0.score(rewritten) - LC0_FILES.score(stored)).abs() <= tolerance, "{stored}");
        }
    }

    #[test]
    fn a_segment_is_known_by_its_path_from_the_corpus_root_on() {
        let key = "data/chess/lc0/train/20260101/a-00.pgn.zst";
        assert_eq!(segment_key("X:/Drive/kalo-chess/data/chess/lc0/train/20260101/a-00.pgn.zst"), key);
        assert_eq!(segment_key("/y/elsewhere/kalo-chess/data/chess/lc0/train/20260101/a-00.pgn.zst"), key);
        assert_eq!(segment_key(key), key);
    }

    fn write_zst(path: &str, text: &str) {
        fs::write(path, zstd::encode_all(text.as_bytes(), 1).unwrap()).unwrap();
    }

    fn read_zst(path: &str) -> String {
        String::from_utf8(zstd::decode_all(fs::File::open(path).unwrap()).unwrap()).unwrap()
    }

    /// Several workers over a resumed run: a recorded segment whose `.rewritten` is still there is only
    /// renamed, never decoded again; every other segment is rewritten once and recorded once.
    #[test]
    fn segments_rewritten_in_parallel_resume_without_converting_twice() {
        let dir = std::env::temp_dir().join(format!("kalo-chess-rewrite-parallel-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        let dir = dir.to_string_lossy().replace('\\', "/");
        let text = |index: usize| format!("1. e4 {{0.{index:02} e7e5}} e5 {{-1.50 g1f3}} 1-0\n\n");
        let paths: Vec<String> = (0..9).map(|index| format!("{dir}/data/chess/lc0/train/s-{index}.pgn.zst")).collect();
        fs::create_dir_all(format!("{dir}/data/chess/lc0/train")).unwrap();
        for (index, path) in paths.iter().enumerate() {
            write_zst(path, &text(index));
        }
        // Segment 0 was rewritten and recorded by an earlier run, stopped before its rename: its
        // `.rewritten` holds a marker, so a second conversion would show.
        write_zst(&format!("{}{REWRITTEN_SUFFIX}", paths[0]), "already rewritten\n");
        let done_path = format!("{dir}/done.txt");
        fs::write(&done_path, format!("{}\n", paths[0])).unwrap();
        let done: HashSet<String> = [segment_key(&paths[0]).to_string()].into();

        rewrite_all(&paths, &done, &done_path, 4).unwrap();

        assert_eq!(read_zst(&paths[0]), "already rewritten\n");
        for (index, path) in paths.iter().enumerate().skip(1) {
            assert_eq!(read_zst(path), rewrite_text(&text(index)).unwrap(), "{path}");
        }
        assert!(paths.iter().all(|path| !Path::new(&format!("{path}{REWRITTEN_SUFFIX}")).exists()));
        let mut recorded: Vec<String> = fs::read_to_string(&done_path).unwrap().lines().map(String::from).collect();
        recorded.sort();
        let mut expected = paths.clone();
        expected.sort();
        assert_eq!(recorded, expected, "every segment recorded once, whole lines");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_comment_without_a_score_is_an_error() {
        assert!(rewrite_text("1. e4 {e7e5} 1-0").is_err());
        assert!(rewrite_text("1. e4 {0.12 e7e5 1-0").is_err());
    }
}
