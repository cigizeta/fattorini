//! A frozen test set held in memory, grouped by piece count, for the test pass of both tracks: the
//! transformer cannot pad, so each of its test batches must hold positions of one length, and the
//! NNUEs are judged on the very same batches. Their test loss is unchanged by the grouping, since
//! `round::run_test_pass` weights every batch by its rows.
//!
//! About 0.6 GB for a frozen set of 4 x 2^20 positions at six lambdas, read once per run.

use std::collections::BTreeMap;

use super::cache::{is_bare_kings, CacheRecord};
use super::train_stream::BatchSource;

pub(crate) struct GroupedTestSet {
    /// The records of each piece count, in rising order of it.
    groups: Vec<Vec<CacheRecord>>,
    /// Each group's first row in the sweep.
    starts: Vec<u64>,
    rows: u64,
}

impl GroupedTestSet {
    /// Every record of `source`, in one sweep, read `batch_size` at a time. Panics on K vs K, which a
    /// test set frozen today never holds (`freeze::sample` skips it): one that does was frozen before
    /// that, and is deleted and frozen again, so that every run is judged on the same positions.
    pub fn load(source: &mut impl BatchSource, batch_size: usize) -> GroupedTestSet {
        let mut by_pieces: BTreeMap<usize, Vec<CacheRecord>> = BTreeMap::new();
        let mut cursor = 0;
        if source.row_count() > 0 {
            loop {
                let (records, next) = source.read_batch_stop_short(cursor, batch_size);
                for record in records {
                    assert!(
                        !is_bare_kings(&record.canonical),
                        "the frozen test set holds K vs K: it was frozen before K vs K was left out, so delete it and \
                         freeze it again"
                    );
                    by_pieces.entry(record.canonical.pieces.len()).or_default().push(record);
                }
                cursor = next;
                if cursor == 0 {
                    break;
                }
            }
        }
        let groups: Vec<Vec<CacheRecord>> = by_pieces.into_values().collect();
        let mut starts = Vec::with_capacity(groups.len());
        let mut rows = 0;
        for group in &groups {
            starts.push(rows);
            rows += group.len() as u64;
        }
        assert_eq!(rows, source.row_count(), "the test set's sweep gave {rows} rows, but it holds {}", source.row_count());
        GroupedTestSet { groups, starts, rows }
    }
}

impl BatchSource for GroupedTestSet {
    fn row_count(&self) -> u64 {
        self.rows
    }

    /// Up to `batch_size` records of the group `cursor` falls in, never crossing into the next one, so
    /// every batch holds a single piece count. The cursor is back to 0 after the last group.
    fn read_batch_stop_short(&mut self, cursor: u64, batch_size: usize) -> (Vec<CacheRecord>, u64) {
        assert!(cursor < self.rows, "cursor {cursor} is past the test set's {} rows", self.rows);
        let group = self.starts.partition_point(|&start| start <= cursor) - 1;
        let offset = (cursor - self.starts[group]) as usize;
        let end = (offset + batch_size).min(self.groups[group].len());
        let records = self.groups[group][offset..end].to_vec();
        let next = self.starts[group] + end as u64;
        (records, if next == self.rows { 0 } else { next })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use temper_chess::board::castling_rights::CastlingRights;
    use temper_chess::board::color::Color;
    use temper_chess::board::square::Square;
    use temper_chess::features::CanonicalPosition;

    /// A flat source over `records`, read in order and stopping short at its end, as `CacheFile` is.
    struct Flat(Vec<CacheRecord>);

    impl BatchSource for Flat {
        fn row_count(&self) -> u64 {
            self.0.len() as u64
        }

        fn read_batch_stop_short(&mut self, cursor: u64, batch_size: usize) -> (Vec<CacheRecord>, u64) {
            let start = cursor as usize;
            let end = (start + batch_size).min(self.0.len());
            let next = if end == self.0.len() { 0 } else { end as u64 };
            (self.0[start..end].to_vec(), next)
        }
    }

    /// A record with `pieces` white knights on a1, a2 and so on, labelled `label`. The positions need
    /// not be legal: only their piece count matters here.
    fn record(pieces: usize, label: f32) -> CacheRecord {
        let canonical = CanonicalPosition {
            side_to_move: Color::White,
            pieces: (0..pieces as u16).map(|square| square * 10 + 2).collect(),
            white_king: Square::E1,
            black_king: Square::E8,
            castling_rights: CastlingRights::NONE,
            en_passant_file: None,
        };
        CacheRecord::new(canonical, label)
    }

    #[test]
    fn one_sweep_gives_every_record_once_in_batches_of_one_piece_count() {
        // Piece counts mixed along the source, 23 records in all.
        let records: Vec<CacheRecord> = (0..23).map(|index| record(1 + index % 3, index as f32)).collect();
        let mut grouped = GroupedTestSet::load(&mut Flat(records), 4);
        assert_eq!(grouped.row_count(), 23);

        let mut seen = Vec::new();
        let mut cursor = 0;
        loop {
            let (batch, next) = grouped.read_batch_stop_short(cursor, 5);
            assert!(!batch.is_empty() && batch.len() <= 5);
            let pieces = batch[0].canonical.pieces.len();
            assert!(batch.iter().all(|record| record.canonical.pieces.len() == pieces), "a batch mixes piece counts");
            seen.extend(batch.iter().map(|record| record.label()));
            cursor = next;
            if cursor == 0 {
                break;
            }
        }
        seen.sort_by(f32::total_cmp);
        assert_eq!(seen, (0..23).map(|index| index as f32).collect::<Vec<_>>(), "every record exactly once");
    }

    #[test]
    #[should_panic(expected = "holds K vs K")]
    fn a_test_set_frozen_with_k_vs_k_is_refused() {
        let _ = GroupedTestSet::load(&mut Flat(vec![record(2, 0.0), record(0, 0.0)]), 4);
    }
}
