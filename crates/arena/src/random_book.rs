//! The opening book -- `UHO_Lichess_4852_v1.epd` in the data root (`data_root::book_path`,
//! downloaded by `bin/lichess-book.sh`), read a line at a time rather than loaded wholesale, since
//! only a handful of lines (one per game pair, or one per self-play game) are ever needed out of its
//! 2.6M positions. Used by the matches, the gate and self-play alike.

use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};

use rand::RngExt;

/// The byte offset of every position in the book, scanned once up front -- self-play draws
/// millions of openings, one at random per game, and the matches and the gate one at random per
/// pair, and re-scanning the whole file per pick would cost O(index) *per pick*. Loading the offsets (not the
/// text itself) keeps memory to one `u64` per position -- about 21MB for the book's 2.6M
/// positions, not the ~180MB the full text would take -- while still letting every pick `seek`
/// straight to its line in O(1).
pub struct RandomBook {
    path: String,
    offsets: Vec<u64>,
}

impl RandomBook {
    pub fn open(path: &str) -> RandomBook {
        let file = File::open(path).unwrap_or_else(|error| panic!("failed to open opening book {path}: {error} -- run bin/lichess-book.sh first"));
        let mut reader = BufReader::new(file);
        let mut offsets = Vec::new();
        let mut position: u64 = 0;
        let mut line = String::new();
        loop {
            offsets.push(position);
            line.clear();
            let bytes_read = reader.read_line(&mut line).unwrap_or_else(|error| panic!("failed to scan opening book {path}: {error}"));
            if bytes_read == 0 {
                offsets.pop(); // the loop always records one offset past the last real line
                break;
            }
            position += bytes_read as u64;
        }
        RandomBook { path: path.to_string(), offsets }
    }

    /// One independent file handle for repeated random reads -- a worker thread opens exactly
    /// one at startup and reuses it for every game it plays, so concurrent self-play workers
    /// never contend on a single shared handle or seek position.
    pub fn reader(&self) -> RandomBookReader {
        let file = File::open(&self.path).unwrap_or_else(|error| panic!("failed to open opening book {}: {error}", self.path));
        RandomBookReader { path: self.path.clone(), file }
    }

    /// A uniformly random opening FEN from the book, via `reader`.
    pub fn random_fen(&self, reader: &mut RandomBookReader, rng: &mut impl RngExt) -> String {
        let index = rng.random_range(0..self.offsets.len());
        reader.read_at(self.offsets[index])
    }
}

pub struct RandomBookReader {
    path: String,
    file: File,
}

impl RandomBookReader {
    fn read_at(&mut self, offset: u64) -> String {
        self.file
            .seek(SeekFrom::Start(offset))
            .unwrap_or_else(|error| panic!("failed to seek in opening book {}: {error}", self.path));
        let mut line = String::new();
        BufReader::new(&mut self.file)
            .read_line(&mut line)
            .unwrap_or_else(|error| panic!("failed to read from {}: {error}", self.path));
        line.trim().to_string()
    }
}
