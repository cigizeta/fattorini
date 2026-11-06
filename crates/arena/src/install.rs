//! An install, asked by the smithy (`protocol::INSTALL_PATH`): a fresh release `temper-chess` and the
//! network it plays, put where lichess-bot runs them, the release root's `bin/`. Run on a thread of
//! the arena's own (`chores.rs`), its workers playing on: their games run in-process, so no
//! `temper-chess.exe` of theirs is replaced.
//!
//! The names in `bin/` never change (`temper-chess.exe`, `temper-chess.nnue`):
//! the engine loads the `temper-chess.nnue` beside its own binary at startup, so no GUI needs an `EvalFile`,
//! lichess-bot's config is written once, and every install only replaces the files. `temper-chess.txt`
//! beside them says what they are; the arena's `Alive` carries it to the smithy's page.
//!
//! The release root, `%LOCALAPPDATA%\kalo-games`, holds the installed engine alone: this is the one
//! place that reads the variable.

use std::process;
use std::sync::atomic::AtomicBool;

use temper_chess::pipeline::protocol::{self, InstallRequest, PESTO};
use temper_chess::pipeline::lan;

use super::chores::{store_path, wait_for, Waited, NETWORK_POLL, NETWORK_TIMEOUT};

/// A network's tuned search parameters, beside its engine file (`nnue::parameters_file_beside`).
const PARAMETERS_FILE: &str = "parameters.txt";

/// The record beside a candidate's network, which `temper-chess.txt` carries with a `candidate_` prefix.
const CANDIDATE_FILE: &str = "candidate.txt";

/// The variable the release root sits under, and the folder there.
const RELEASE_BASE_VARIABLE: &str = "LOCALAPPDATA";
const UNDER_RELEASE_BASE: &str = "kalo-games";

/// The release root, exiting when `LOCALAPPDATA` is unset.
fn release_root() -> String {
    release_root_from(std::env::var(RELEASE_BASE_VARIABLE).ok()).unwrap_or_else(|message| {
        eprintln!("{message}");
        process::exit(1);
    })
}

/// `release_root` on an explicit value, so its tests do not mutate the process environment, which
/// other tests running in parallel would see. An empty value counts as unset. The separator is
/// `/` throughout, `LOCALAPPDATA`'s own backslashes included: every Windows file API accepts it,
/// and a printed path can be pasted into a double-quoted YAML string (lichess-bot's `config.yml`),
/// where a backslash starts an escape.
fn release_root_from(base: Option<String>) -> Result<String, String> {
    match base.filter(|value| !value.is_empty()) {
        Some(base) => Ok(format!("{}/{UNDER_RELEASE_BASE}", base.replace('\\', "/"))),
        None => Err(format!("{RELEASE_BASE_VARIABLE} is not set, so there is no release root under it")),
    }
}

/// An install the arena can run: the network's path in the store, `None` for the PeSTO evaluation.
#[derive(Debug, PartialEq)]
pub(crate) struct Install {
    network: Option<String>,
}

impl Install {
    /// `request`'s network, checked: `pesto` or a `.nnue` in the store.
    pub(crate) fn of(request: &InstallRequest) -> Result<Install, String> {
        let network = match request.network.as_str() {
            PESTO => None,
            path if path.ends_with(".nnue") => Some(in_store(path)?),
            other => return Err(format!("the network is pesto or a .nnue in the store, got {other:?}")),
        };
        Ok(Install { network })
    }

    /// What it installs, for the log and the page: the network's path, or `pesto`.
    pub(crate) fn name(&self) -> &str {
        self.network.as_deref().unwrap_or(PESTO)
    }
}

/// `path`, when it is a path in the store (`store_path`'s rule), as it is.
fn in_store(path: &str) -> Result<String, String> {
    store_path("", path)?;
    Ok(path.to_string())
}

/// Runs `install`, its files read from `store`, a network not there yet waited for up to
/// `NETWORK_TIMEOUT` until `stop`. `Ok` with what was installed, or `Err` with why nothing was, or
/// what was left half done.
pub(crate) fn run(install: &Install, store: &str, stop: &AtomicBool) -> Result<String, String> {
    let network = match &install.network {
        Some(name) => Some(fetch(name, store, stop)?),
        None => None,
    };

    build_engine()?;

    let bin_dir = format!("{}/bin", release_root());
    create_dir(&bin_dir)?;
    let engine = engine_file_name();
    let fields = match &network {
        Some(network_path) => {
            install_file(&engine_build_path(), &format!("{bin_dir}/{engine}"))?;
            install_file(network_path, &format!("{bin_dir}/temper-chess.nnue"))?;
            install_parameters(network_path, &bin_dir)?;
            let record = std::fs::read_to_string(beside(network_path, CANDIDATE_FILE)).unwrap_or_default();
            install_record(install.name(), &record, &protocol::current_commit(), &current_time())
        }
        None => {
            // The network goes before the binary is replaced: an engine lichess-bot starts midway
            // then gets the old binary with no network, never the new binary beside a network it
            // might not load (the engine exits when its `EvalFile` fails to load).
            remove_if_present(&format!("{bin_dir}/temper-chess.nnue"))?;
            install_file(&engine_build_path(), &format!("{bin_dir}/{engine}"))?;
            pesto_install_record(&protocol::current_commit(), &current_time())
        }
    };
    // Last, so it only ever describes files already in place.
    write_record(&format!("{bin_dir}/temper-chess.txt"), &fields)?;
    Ok(format!("installed {} in {bin_dir}/", install.name()))
}

/// The `temper-chess.txt` of the engine installed on this machine, field by field; none before a first
/// install.
pub(crate) fn installed() -> Vec<(String, String)> {
    let text = std::fs::read_to_string(format!("{}/bin/temper-chess.txt", release_root())).unwrap_or_default();
    text.lines().filter_map(|line| line.split_once('\t')).map(|(field, value)| (field.to_string(), value.to_string())).collect()
}

/// The network `name` names, read local first, this machine's copy when it holds one (`name` is the
/// same `data/...` path in the repository's tree), else from `store`, waited for there. Its
/// `candidate.txt` and `parameters.txt`, when there are any, are read from the same place: the smithy
/// puts them in the store before the network, so they are there when it is.
fn fetch(name: &str, store: &str, stop: &AtomicBool) -> Result<String, String> {
    if std::path::Path::new(name).is_file() {
        return Ok(name.to_string());
    }
    let path = store_path(store, name)?;
    if !std::path::Path::new(&path).is_file() {
        log::info!("install: waiting for {name} in {store}, up to {} minutes", NETWORK_TIMEOUT.as_secs() / 60);
    }
    match wait_for(&path, NETWORK_TIMEOUT, NETWORK_POLL, stop) {
        Waited::Ready => Ok(path),
        Waited::Stopped => Err("the arena stopped first: nothing was installed".to_string()),
        Waited::TimedOut => Err(format!(
            "{name} not in {store} after {} minutes: nothing was installed",
            NETWORK_TIMEOUT.as_secs() / 60
        )),
    }
}

/// The file `name` in the directory of `path`.
fn beside(path: &str, name: &str) -> String {
    let dir = std::path::Path::new(path).parent().map_or(String::new(), |dir| dir.to_string_lossy().into_owned());
    format!("{dir}/{name}")
}

/// Puts the network's tuned parameters, the `parameters.txt` beside it, beside `temper-chess.nnue`, where
/// the engine reads it (`nnue::parameters_file_beside`), so lichess-bot plays at them. A network never
/// tuned has none: an old `parameters.txt` is then removed, so the engine never plays one network
/// with another's parameters. A `margin.txt` left by an older install is removed too, being retired.
fn install_parameters(network_path: &str, bin_dir: &str) -> Result<(), String> {
    remove_if_present(&format!("{bin_dir}/margin.txt"))?;
    let destination = format!("{bin_dir}/{PARAMETERS_FILE}");
    let parameters_path = beside(network_path, PARAMETERS_FILE);
    if std::path::Path::new(&parameters_path).is_file() {
        install_file(&parameters_path, &destination)?;
        log::info!("install: the tuned parameters {parameters_path} as {destination}");
        return Ok(());
    }
    remove_if_present(&destination)?;
    log::info!("install: no tuned parameters beside the network: the engine plays at the default parameters");
    Ok(())
}

/// Removes the file at `path` if there is one.
fn remove_if_present(path: &str) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("failed to remove {path}: {error}")),
    }
}

/// `temper-chess.txt`'s fields for the pesto install: no weights to name.
fn pesto_install_record(commit: &str, installed: &str) -> Vec<(String, String)> {
    vec![
        ("network".to_string(), PESTO.to_string()),
        ("commit".to_string(), commit.to_string()),
        ("installed".to_string(), installed.to_string()),
    ]
}

/// `temper-chess.txt`'s fields: what was installed, and when. `record` is the record beside the network, its
/// `candidate.txt`, carried over field by field with `candidate_` as a prefix.
fn install_record(network: &str, record: &str, commit: &str, installed: &str) -> Vec<(String, String)> {
    let mut fields = vec![("network".to_string(), network.to_string())];
    for line in record.lines().filter(|line| !line.is_empty()) {
        let (field, value) = line.split_once('\t').unwrap_or((line, ""));
        fields.push((format!("candidate_{field}"), value.to_string()));
    }
    fields.push(("commit".to_string(), commit.to_string()));
    fields.push(("installed".to_string(), installed.to_string()));
    fields
}

/// Writes `fields` to `path` as one `name<TAB>value` line each, whole (`lan::write_whole`).
fn write_record(path: &str, fields: &[(String, String)]) -> Result<(), String> {
    lan::write_whole(path, &fields.iter().map(|(name, value)| format!("{name}\t{value}\n")).collect::<String>())
}

/// Creates `dir` and its parents. An existing directory is left as it is.
fn create_dir(dir: &str) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|error| format!("failed to create {dir}: {error}"))
}

/// The engine's file name, both where `cargo build --release` leaves it and where the install puts
/// it.
fn engine_file_name() -> String {
    format!("temper-chess{}", std::env::consts::EXE_SUFFIX)
}

/// Where `cargo build --release` leaves the engine, relative to the repository root every binary
/// runs from.
fn engine_build_path() -> String {
    format!("target/release/{}", engine_file_name())
}

/// Runs `cargo build --release --bin temper-chess`: only `temper-chess`, never the `arena` that is running. Its
/// output goes to the log, a line at a time once it ends.
fn build_engine() -> Result<(), String> {
    log::info!("install: building a release temper-chess");
    let output = process::Command::new("cargo")
        .args(["build", "--release", "--bin", "temper-chess"])
        .output()
        .map_err(|error| format!("failed to run cargo: {error}"))?;
    for line in String::from_utf8_lossy(&output.stderr).lines() {
        log::info!("install: {line}");
    }
    if !output.status.success() {
        return Err(format!("cargo build --release --bin temper-chess failed ({}): nothing was installed", output.status));
    }
    Ok(())
}

/// Copies `source` to `destination` through `destination.partial` and a rename. The rename matters
/// for the engine: Windows refuses to replace a binary lichess-bot is running, while a rename leaves
/// the running process its old file (`move_aside`).
fn install_file(source: &str, destination: &str) -> Result<(), String> {
    let staging_path = format!("{destination}.partial");
    std::fs::copy(source, &staging_path).map_err(|error| format!("failed to copy {source} to {staging_path}: {error}"))?;
    move_aside(destination)?;
    std::fs::rename(&staging_path, destination).map_err(|error| format!("failed to rename {staging_path} to {destination}: {error}"))
}

/// Windows refuses to replace or delete a file a process has open, a running `temper-chess.exe` above all,
/// but lets it be renamed. So `destination` is renamed aside to `{destination}.old-{seconds}`, its
/// process keeping its file, and the rename into place then finds the name free. The asides of
/// earlier installs are removed first, best-effort: one whose engine still runs stays until a later
/// install. Seconds in the name, so an aside still running never blocks a newer one.
fn move_aside(destination: &str) -> Result<(), String> {
    let path = std::path::Path::new(destination);
    let (Some(dir), Some(name)) = (path.parent(), path.file_name().and_then(|name| name.to_str())) else {
        return Ok(());
    };
    let aside_prefix = format!("{name}.old-");
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.file_name().to_str().is_some_and(|other| other.starts_with(&aside_prefix)) {
                std::fs::remove_file(entry.path()).ok();
            }
        }
    }
    if !path.exists() {
        return Ok(());
    }
    let seconds = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs());
    let aside = format!("{destination}.old-{seconds}");
    std::fs::rename(destination, &aside).map_err(|error| format!("failed to move {destination} aside to {aside}: {error}"))
}

/// The current time as `YYYY-MM-DD HH:MM UTC`.
fn current_time() -> String {
    let now = time::OffsetDateTime::now_utc();
    format!("{:04}-{:02}-{:02} {:02}:{:02} UTC", now.year(), u8::from(now.month()), now.day(), now.hour(), now.minute())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(network: &str) -> InstallRequest {
        InstallRequest { boot: "b00t".to_string(), network: network.to_string() }
    }

    fn some(value: &str) -> Option<String> {
        Some(value.to_string())
    }

    #[test]
    fn the_release_root_is_under_local_app_data() {
        assert_eq!(
            release_root_from(some(r"C:\Users\me\AppData\Local")).unwrap(),
            "C:/Users/me/AppData/Local/kalo-games"
        );
    }

    #[test]
    fn no_base_is_no_release_root() {
        assert!(release_root_from(None).is_err());
        assert!(release_root_from(some("")).is_err(), "empty is unset");
    }

    #[test]
    fn an_install_takes_pesto_or_a_network_in_the_store() {
        let network = "data/chess/networks/fine/run-1/gen-2/champion.nnue";
        assert_eq!(Install::of(&request(PESTO)), Ok(Install { network: None }));
        assert_eq!(Install::of(&request(network)), Ok(Install { network: Some(network.to_string()) }));
        assert_eq!(Install::of(&request(network)).unwrap().name(), network);
        for network in ["betty-g1-r1", "C:/elsewhere/champion.nnue", "data/../champion.nnue"] {
            assert!(Install::of(&request(network)).is_err(), "{network}");
        }
    }

    #[test]
    fn install_record_carries_the_record_beside_the_network() {
        let fields = install_record("data/chess/networks/w.nnue", "lambda\t0.05\nstep\t120000\n", "abc123", "2026-10-22 14:03 UTC");
        let actual: Vec<(&str, &str)> = fields.iter().map(|(field, value)| (field.as_str(), value.as_str())).collect();
        assert_eq!(
            actual,
            [
                ("network", "data/chess/networks/w.nnue"),
                ("candidate_lambda", "0.05"),
                ("candidate_step", "120000"),
                ("commit", "abc123"),
                ("installed", "2026-10-22 14:03 UTC"),
            ]
        );
        let names: Vec<String> = install_record("w.nnue", "", "unknown", "now").into_iter().map(|(field, _)| field).collect();
        assert_eq!(names, ["network", "commit", "installed"], "no record beside it");
    }

    #[test]
    fn the_pesto_install_record_names_no_weights() {
        let names: Vec<String> = pesto_install_record("abc123", "now").into_iter().map(|(field, _)| field).collect();
        assert_eq!(names, ["network", "commit", "installed"]);
    }
}
