//! The data root: the repository's gitignored `data/`, which holds the machine's own files and the
//! third-party downloads (the opening book and the Syzygy tables, fetched by `bin/uho-book.sh`
//! and `bin/syzygy.sh`). The two scripts compute the same root in shell, and the two must agree.
//!
//! Development only, and nothing here reads the environment: `%LOCALAPPDATA%` belongs to a release,
//! the engine installed for lichess-bot, and is read in one place, the arena's `install`.
//!
//! The repository is found by walking up from the current directory to the first one holding
//! `Cargo.lock`: the `bin/` wrappers `cd` to the root, while `cargo test` runs in the crate's
//! directory, where a plain relative `data` would make the tests that need real tables skip
//! silently. No path is compiled in, since that would put the build machine's path in the binary.

use std::path::Path;
use std::process;

const MARKER: &str = "Cargo.lock";
const UNDER_REPOSITORY: &str = "data";

/// The data root, exiting when the current directory is not inside the repository.
pub fn data_root() -> String {
    let found = std::env::current_dir()
        .map_err(|error| format!("failed to read the current directory: {error}"))
        .and_then(|start| data_root_from(&start, |directory| directory.join(MARKER).is_file()));
    found.unwrap_or_else(|message| {
        eprintln!("{message}");
        process::exit(1);
    })
}

/// The four Syzygy sets `bin/syzygy.sh` downloads: WDL, which the search probes, and DTZ, which ranks
/// the root moves. All required by the tools that load them in-process (the arena's players, the
/// smithy's tablebase labels), unlike the engine, where a tester may give it any of them or none.
pub fn syzygy_directories() -> Vec<String> {
    let root = data_root();
    ["3-4-5-wdl", "6-wdl", "3-4-5-dtz", "6-dtz"].iter().map(|set| format!("{root}/chess/syzygy/{set}")).collect()
}

/// `data_root` from an explicit directory and test for the repository, so its tests need no real
/// directories. The separator is `/` throughout, the current directory's own backslashes included:
/// every Windows file API accepts it, and callers join further `/` parts.
fn data_root_from(start: &Path, is_repository: impl Fn(&Path) -> bool) -> Result<String, String> {
    match start.ancestors().find(|directory| is_repository(directory)) {
        Some(repository) => {
            Ok(format!("{}/{UNDER_REPOSITORY}", repository.to_string_lossy().trim_end_matches('\\').replace('\\', "/")))
        }
        None => Err(format!("no {MARKER} in {} or above it: run this from the repository", start.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_data_root_is_in_the_repository() {
        let root = data_root_from(Path::new(r"C:\dev\repository"), |directory| directory.ends_with("repository"));
        assert_eq!(root.unwrap(), "C:/dev/repository/data");
    }

    #[test]
    fn the_repository_is_found_from_below_it() {
        let root = data_root_from(Path::new(r"C:\dev\repository\crates\arena"), |directory| directory.ends_with("repository"));
        assert_eq!(root.unwrap(), "C:/dev/repository/data");
    }

    #[test]
    fn outside_a_repository_is_no_data_root() {
        assert!(data_root_from(Path::new(r"C:\dev\elsewhere"), |_| false).is_err());
    }
}
