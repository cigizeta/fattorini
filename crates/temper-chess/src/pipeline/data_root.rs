//! The data root: where the files that live outside the repository go, shared by development and
//! deployment. The third-party downloads (the opening book and the Syzygy tables, fetched by
//! `bin/lichess-book.sh` and `bin/syzygy.sh`) and the engine `smithy install` sets up for
//! lichess-bot, in `bin/`. The project's own data stays in the repository (`configs/chess/`, `data/`).
//!
//! One fixed place, with no variable to move it: `%LOCALAPPDATA%\kalo-games` (native Windows usually has
//! no `HOME` outside Git Bash). The two scripts compute the same root in shell, and the two must
//! agree.

use std::process;

const BASE_VARIABLE: &str = "LOCALAPPDATA";
const UNDER_BASE: &str = "kalo-games";

/// The data root, exiting when `LOCALAPPDATA` is unset.
pub fn data_root() -> String {
    data_root_from(std::env::var(BASE_VARIABLE).ok()).unwrap_or_else(|message| {
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

/// `data_root` on an explicit value, so its tests do not mutate the process environment, which
/// other tests running in parallel would see. An empty value counts as unset. The separator is
/// `/` throughout, `LOCALAPPDATA`'s own backslashes included: every Windows file API accepts it,
/// callers join further `/` parts, and a printed path can be pasted into a double-quoted YAML string
/// (lichess-bot's `config.yml`, which `smithy install` prints), where a backslash starts an escape.
fn data_root_from(base: Option<String>) -> Result<String, String> {
    match base.filter(|value| !value.is_empty()) {
        Some(base) => Ok(format!("{}/{UNDER_BASE}", base.replace('\\', "/"))),
        None => Err(format!("{BASE_VARIABLE} is not set, so there is no data root under it")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn some(value: &str) -> Option<String> {
        Some(value.to_string())
    }

    #[test]
    fn the_data_root_is_under_local_app_data() {
        assert_eq!(
            data_root_from(some(r"C:\Users\me\AppData\Local")).unwrap(),
            "C:/Users/me/AppData/Local/kalo-games"
        );
    }

    #[test]
    fn no_base_is_no_data_root() {
        assert!(data_root_from(None).is_err());
        assert!(data_root_from(some("")).is_err(), "empty is unset");
    }
}
