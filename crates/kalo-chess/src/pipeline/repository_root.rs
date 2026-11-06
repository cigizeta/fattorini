//! The repository root, which every binary runs from (each `bin/` wrapper `cd`s there), so the
//! relative paths of the pipeline (`configs/chess/`, `data/`, `examples/`) resolve against it. `cargo test`
//! instead runs each crate's tests from that crate's own directory, `crates/<name>/`.

use std::sync::Once;

/// Makes the repository root the process's working directory, once per process. For tests: every
/// test of a crate runs in one process, so a test that reads a real file under the root calls this
/// first, and every relative path then resolves as it does under the `bin/` wrappers.
pub fn enter_for_tests() {
    static ENTER: Once = Once::new();
    ENTER.call_once(|| {
        // This crate is `crates/kalo-chess`, two levels below the root, whichever crate's test calls this.
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        std::env::set_current_dir(root).unwrap_or_else(|error| panic!("failed to enter {root}: {error}"));
    });
}
