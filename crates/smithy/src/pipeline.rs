//! Builds training batches on every core while the main thread steps the GPU.
//!
//! Three stages. One reader thread calls `source` in order, so a stream and its RNG are only ever
//! touched by one thread and yield exactly the sequence a single-threaded loop would. Workers turn
//! each item into its prepared form (`prepare`), several items at once: this is the CPU work, and it
//! is parallel across batches, never within one. The calling thread takes the prepared items back
//! in their original order and hands each to `consume`, so everything order-dependent -- the
//! optimizer, the tensors, the GPU -- stays on one thread and sees the same sequence as before.
//!
//! Both channels are bounded, so the reader never runs more than a few items per worker ahead.
//! When `consume` breaks, the calling thread drops its end: workers see their sends fail and exit,
//! and the reader sees its own fail once they have, so nothing is left blocked when the scope joins.

use std::collections::BTreeMap;
use std::ops::ControlFlow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Mutex};

/// Items each worker may have queued ahead of it, in each channel.
const QUEUED_PER_WORKER: usize = 2;

/// Workers for this machine: every logical core but the reader's and the calling thread's, and at
/// least one.
pub fn worker_count() -> usize {
    let logical = std::thread::available_parallelism().map_or(1, |cores| cores.get());
    logical.saturating_sub(2).max(1)
}

/// Runs `source` to exhaustion (or until `consume` breaks), `prepare` on `workers` threads, and
/// `consume` on the calling thread in `source`'s order.
pub fn pipelined<I: Send, H: Send>(
    workers: usize, mut source: impl FnMut() -> Option<I> + Send, prepare: impl Fn(I) -> H + Sync,
    mut consume: impl FnMut(H) -> ControlFlow<()>,
) {
    assert!(workers >= 1, "a pipeline needs at least one worker");
    let capacity = workers * QUEUED_PER_WORKER;
    let abandoned = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let (work_sender, work_receiver) = sync_channel::<(u64, I)>(capacity);
        let (done_sender, done_receiver) = sync_channel::<(u64, H)>(capacity);
        // Shared by the workers and dropped with the last of them, which is what unblocks a reader
        // waiting on a full channel after the calling thread has stopped.
        let work_receiver = Arc::new(Mutex::new(work_receiver));
        let abandoned = &abandoned;
        let prepare = &prepare;

        scope.spawn(move || {
            let mut index = 0u64;
            while !abandoned.load(Ordering::SeqCst) {
                let Some(item) = source() else {
                    break;
                };
                if work_sender.send((index, item)).is_err() {
                    break;
                }
                index += 1;
            }
        });

        for _ in 0..workers {
            let work_receiver = Arc::clone(&work_receiver);
            let done_sender = done_sender.clone();
            scope.spawn(move || loop {
                // The lock is held only while waiting for the next item, never while preparing it.
                let next = work_receiver.lock().expect("no worker should have poisoned the lock").recv();
                let Ok((index, item)) = next else {
                    break;
                };
                if done_sender.send((index, prepare(item))).is_err() {
                    break;
                }
            });
        }
        drop(work_receiver);
        drop(done_sender);

        let mut pending: BTreeMap<u64, H> = BTreeMap::new();
        let mut next = 0u64;
        'receiving: for (index, prepared) in done_receiver.iter() {
            pending.insert(index, prepared);
            while let Some(prepared) = pending.remove(&next) {
                next += 1;
                if consume(prepared).is_break() {
                    break 'receiving;
                }
            }
        }
        abandoned.store(true, Ordering::SeqCst);
        drop(done_receiver);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_come_back_in_source_order_whatever_order_they_are_prepared_in() {
        let mut next = 0u64;
        let source = move || {
            next += 1;
            (next <= 200).then_some(next)
        };
        // Earlier items take longer, so they finish after later ones.
        let prepare = |item: u64| {
            std::thread::sleep(std::time::Duration::from_micros((200 - item) * 20));
            item * 10
        };
        let mut seen = Vec::new();
        pipelined(4, source, prepare, |prepared| {
            seen.push(prepared);
            ControlFlow::Continue(())
        });
        assert_eq!(seen, (1..=200).map(|item| item * 10).collect::<Vec<_>>());
    }

    #[test]
    fn a_break_stops_the_pipeline_without_blocking() {
        // An endless source: only the break ends it.
        let mut next = 0u64;
        let source = move || {
            next += 1;
            Some(next)
        };
        let mut seen = Vec::new();
        pipelined(3, source, |item: u64| item, |prepared| {
            seen.push(prepared);
            if seen.len() == 25 { ControlFlow::Break(()) } else { ControlFlow::Continue(()) }
        });
        assert_eq!(seen, (1..=25).collect::<Vec<_>>());
    }

    #[test]
    fn an_empty_source_consumes_nothing() {
        let mut consumed = 0;
        pipelined(2, || None::<u64>, |item: u64| item, |_| {
            consumed += 1;
            ControlFlow::Continue(())
        });
        assert_eq!(consumed, 0);
    }
}
