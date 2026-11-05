//! The third-party downloads in the data root: the opening book and the Syzygy tables, downloaded
//! there by `bin/lichess-book.sh` and `bin/syzygy.sh`. Where the root itself is lives in
//! `temper_chess::pipeline::data_root`, shared with `smithy`, which installs the engine under it.

use std::process;

use temper_chess::tablebase::SyzygyTablebase;

pub use temper_chess::pipeline::data_root::{data_root, syzygy_directories};

/// The opening book every match, gate and self-play game draws its openings from.
pub fn book_path() -> String {
    format!("{}/chess/books/UHO_Lichess_4852_v1.epd", data_root())
}

/// Every table in `syzygy_directories`, exiting when a directory is missing or holds none. Loud on
/// purpose: an in-process player without its tables still plays, only differently, so a silent miss
/// would change self-play's games and a gate's verdict with nothing on the console to say so.
pub fn load_tablebase() -> SyzygyTablebase {
    SyzygyTablebase::from_directories(&syzygy_directories()).unwrap_or_else(|message| {
        eprintln!("{message}");
        process::exit(1);
    })
}
