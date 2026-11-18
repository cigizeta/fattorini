//! A source's segments on the remote, listed with their searched positions: what the scatter reads
//! into the buckets (`buckets.rs`), and what the census, the frozen test set and Lc0's rewrite read.
//! Training itself reads the buckets alone, every architecture: an NNUE's batches straight, a
//! transformer's grouped by piece count (`bucketed_train_stream.rs`).

use super::pgn_stream::StreamFile;
use fattorini::pipeline::segments;

use super::{preprocessing_config, remote};

/// A source's training segments on the remote, in the day directories of `{root}/train` (Lc0's
/// `data/lc0`, PeSTO's `data/pesto`), read in place, each with the positions its `.meta` records, by
/// day and then by name. An error when there are none.
pub(crate) fn training_files(root: &str) -> Result<Vec<StreamFile>, String> {
    segment_files(&preprocessing_config::source_train_dir(root))
}

/// A source's test segments, in `{root}/test`, as `training_files` reads its training ones.
pub(crate) fn test_files(root: &str) -> Result<Vec<StreamFile>, String> {
    segment_files(&preprocessing_config::source_test_dir(root))
}

/// The segments of `dir` on the remote, by day and then by name, each with its searched positions:
/// its `.meta` positions less its games, one final position a game; an error when there are none.
fn segment_files(dir: &str) -> Result<Vec<StreamFile>, String> {
    let remote_dir = remote::remote_path(dir);
    let names = segments::list_remote(dir).map_err(|error| format!("failed to list {remote_dir}: {error}"))?;
    if names.is_empty() {
        return Err(format!("{remote_dir} holds no segments"));
    }
    let mut files = Vec::with_capacity(names.len());
    // One `.meta` a segment, each fetched from the remote on its own: minutes for Lc0's 1,400, so
    // the page shows them counted rather than a run that seems stalled.
    let progress = progress::bar(names.len() as u64, progress::Unit::Count("segments"), &format!("listing the segments of {dir}"));
    for name in names {
        let meta = segments::read_remote_meta(dir, &name)?;
        let positions = meta.positions.saturating_sub(meta.games);
        files.push(StreamFile { path: format!("{remote_dir}/{name}{}", segments::SEGMENT_SUFFIX), positions });
        progress.inc(1);
    }
    progress.finish();
    Ok(files)
}

/// The reader's threads: half the logical cores, at least 1. The other half builds the batches
/// (`round::BatchBuilder` runs its CPU half on every core). 4 threads streamed 3.4M positions a
/// second, far above what a training step consumes.
pub(crate) fn reader_threads() -> usize {
    let cores = std::thread::available_parallelism().map_or(2, |cores| cores.get());
    (cores / 2).max(1)
}
