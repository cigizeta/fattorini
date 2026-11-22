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
    // One `.meta` a segment, each a round trip to the remote of about 220 ms: read one at a time,
    // Lc0's 6,897 took about 25 minutes, so they are read `META_READERS` at once. The page shows
    // them counted rather than a run that seems stalled.
    let progress = progress::bar(names.len() as u64, progress::Unit::Count("segments"), &format!("listing the segments of {dir}"));
    let files = map_in_order(&names, META_READERS, |name| {
        let meta = segments::read_remote_meta(dir, name)?;
        progress.inc(1);
        let positions = meta.positions.saturating_sub(meta.games);
        Ok(StreamFile { path: format!("{remote_dir}/{name}{}", segments::SEGMENT_SUFFIX), positions })
    });
    progress.finish();
    files
}

/// The `.meta` files read at once: the remote's round trip, not its bandwidth, bounds the listing.
const META_READERS: usize = 16;

/// `map` of every item on `threads` threads, the results in the items' order: the scatter files
/// positions by their arrival in this order, so it must not depend on which read finished first.
/// The first error is returned; once one fails, no thread takes another item.
fn map_in_order<T, R, F>(items: &[T], threads: usize, map: F) -> Result<Vec<R>, String>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> Result<R, String> + Sync,
{
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;
    let next = AtomicUsize::new(0);
    let failed = AtomicBool::new(false);
    let slots: Vec<Mutex<Option<Result<R, String>>>> = items.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..threads.clamp(1, items.len().max(1)) {
            scope.spawn(|| {
                while !failed.load(Ordering::Relaxed) {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(index) else { break };
                    let result = map(item);
                    if result.is_err() {
                        failed.store(true, Ordering::Relaxed);
                    }
                    *slots[index].lock().expect("a reader panicked") = Some(result);
                }
            });
        }
    });
    let mut results = Vec::with_capacity(items.len());
    for slot in slots {
        match slot.into_inner().expect("a reader panicked") {
            Some(Ok(result)) => results.push(result),
            Some(Err(error)) => return Err(error),
            // Left unread once another item failed: that error comes first or later in the order.
            None => {}
        }
    }
    match results.len() == items.len() {
        true => Ok(results),
        false => Err("a segment's read failed".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::map_in_order;

    #[test]
    fn map_in_order_keeps_the_items_order_on_many_threads() {
        let items: Vec<u64> = (0..1000).collect();
        let mapped = map_in_order(&items, 16, |&item| {
            // Later items finish first, so a result placed by finish order would come out shuffled.
            std::thread::sleep(std::time::Duration::from_micros((1000 - item) % 7 * 50));
            Ok(item * 2)
        })
        .unwrap();
        assert_eq!(mapped, items.iter().map(|item| item * 2).collect::<Vec<_>>());
    }

    #[test]
    fn map_in_order_returns_an_error() {
        let items: Vec<u64> = (0..100).collect();
        let result = map_in_order(&items, 8, |&item| if item == 37 { Err(format!("item {item}")) } else { Ok(item) });
        assert_eq!(result, Err("item 37".to_string()));
    }

    #[test]
    fn map_in_order_of_nothing_is_empty() {
        let items: Vec<u64> = Vec::new();
        assert_eq!(map_in_order(&items, 4, |&item| Ok(item)), Ok(Vec::new()));
    }
}

/// The reader's threads: half the logical cores, at least 1. The other half builds the batches
/// (`round::BatchBuilder` runs its CPU half on every core). 4 threads streamed 3.4M positions a
/// second, far above what a training step consumes.
pub(crate) fn reader_threads() -> usize {
    let cores = std::thread::available_parallelism().map_or(2, |cores| cores.get());
    (cores / 2).max(1)
}
