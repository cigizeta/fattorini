//! A snapshot, asked by the smithy (`protocol::SNAPSHOT_PATH`): a fresh release build of `kalo-chess`
//! and the network it plays, taken at one commit into the data root's `chess/snapshot/`, where
//! anything may then run them (lichess-bot today). Run on a thread of the arena's own (`chores.rs`),
//! its workers playing on: their games run in-process, so no `kalo-chess.exe` of theirs is replaced.
//!
//! The names in the snapshot never change (`kalo-chess.exe`, `kalo-chess.nnue`): the engine loads
//! the `kalo-chess.nnue` beside its own binary at startup, so no GUI needs an `EvalFile`, lichess-bot's
//! config is written once, and every snapshot only replaces the files. `kalo-chess.txt` beside them
//! says what they are; the arena's `Alive` carries it to the smithy's page.
//!
//! Nothing here, nor anywhere in the repository, writes outside it and Drive: the snapshot is in the
//! repository's gitignored `data/`, never uploaded.

use std::process;
use std::sync::atomic::AtomicBool;

use kalo_chess::pipeline::data_root::data_root;
use kalo_chess::pipeline::lan;
use kalo_chess::pipeline::protocol::{self, SnapshotRequest, PESTO};

use super::chores::{store_path, wait_for, Waited, NETWORK_POLL, NETWORK_TIMEOUT};

/// A network's tuned search parameters, beside its engine file (`nnue::parameters_file_beside`).
const PARAMETERS_FILE: &str = "parameters.txt";

/// The record beside a candidate's network, which `kalo-chess.txt` carries with a `candidate_` prefix.
const CANDIDATE_FILE: &str = "candidate.txt";

/// The snapshot's folder, under the data root.
fn snapshot_dir() -> String {
    format!("{}/chess/snapshot", data_root())
}

/// A snapshot the arena can take: the network's path in the store, `None` for the PeSTO evaluation.
#[derive(Debug, PartialEq)]
pub(crate) struct Snapshot {
    network: Option<String>,
}

impl Snapshot {
    /// `request`'s network, checked: `pesto` or a `.nnue` in the store.
    pub(crate) fn of(request: &SnapshotRequest) -> Result<Snapshot, String> {
        let network = match request.network.as_str() {
            PESTO => None,
            path if path.ends_with(".nnue") => Some(in_store(path)?),
            other => return Err(format!("the network is pesto or a .nnue in the store, got {other:?}")),
        };
        Ok(Snapshot { network })
    }

    /// What it takes, for the log and the page: the network's path, or `pesto`.
    pub(crate) fn name(&self) -> &str {
        self.network.as_deref().unwrap_or(PESTO)
    }
}

/// `path`, when it is a path in the store (`store_path`'s rule), as it is.
fn in_store(path: &str) -> Result<String, String> {
    store_path("", path)?;
    Ok(path.to_string())
}

/// Takes `snapshot`, its files read from `store`, a network not there yet waited for up to
/// `NETWORK_TIMEOUT` until `stop`. `Ok` with what was taken, or `Err` with why nothing was, or what
/// was left half done.
pub(crate) fn run(snapshot: &Snapshot, store: &str, stop: &AtomicBool) -> Result<String, String> {
    let network = match &snapshot.network {
        Some(name) => Some(fetch(name, store, stop)?),
        None => None,
    };

    build_engine()?;

    let dir = snapshot_dir();
    create_dir(&dir)?;
    let engine = engine_file_name();
    let fields = match &network {
        Some(network_path) => {
            copy_into_place(&engine_build_path(), &format!("{dir}/{engine}"))?;
            copy_into_place(network_path, &format!("{dir}/kalo-chess.nnue"))?;
            take_parameters(network_path, &dir)?;
            let record = std::fs::read_to_string(beside(network_path, CANDIDATE_FILE)).unwrap_or_default();
            snapshot_record(snapshot.name(), &record, &protocol::current_commit(), &current_time())
        }
        None => {
            // The network goes before the binary is replaced: an engine lichess-bot starts midway
            // then gets the old binary with no network, never the new binary beside a network it
            // might not load (the engine exits when its `EvalFile` fails to load).
            remove_if_present(&format!("{dir}/kalo-chess.nnue"))?;
            copy_into_place(&engine_build_path(), &format!("{dir}/{engine}"))?;
            pesto_snapshot_record(&protocol::current_commit(), &current_time())
        }
    };
    // Last, so it only ever describes files already in place.
    write_record(&format!("{dir}/kalo-chess.txt"), &fields)?;
    Ok(format!("took {} in {dir}/", snapshot.name()))
}

/// The `kalo-chess.txt` of this machine's snapshot, field by field; none before a first snapshot.
pub(crate) fn record() -> Vec<(String, String)> {
    let text = std::fs::read_to_string(format!("{}/kalo-chess.txt", snapshot_dir())).unwrap_or_default();
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
        log::info!("snapshot: waiting for {name} in {store}, up to {} minutes", NETWORK_TIMEOUT.as_secs() / 60);
    }
    match wait_for(&path, NETWORK_TIMEOUT, NETWORK_POLL, stop) {
        Waited::Ready => Ok(path),
        Waited::Stopped => Err("the arena stopped first: no snapshot was taken".to_string()),
        Waited::TimedOut => Err(format!(
            "{name} not in {store} after {} minutes: no snapshot was taken",
            NETWORK_TIMEOUT.as_secs() / 60
        )),
    }
}

/// The file `name` in the directory of `path`.
fn beside(path: &str, name: &str) -> String {
    let dir = std::path::Path::new(path).parent().map_or(String::new(), |dir| dir.to_string_lossy().into_owned());
    format!("{dir}/{name}")
}

/// Puts the network's tuned parameters, the `parameters.txt` beside it, beside `kalo-chess.nnue`, where
/// the engine reads it (`nnue::parameters_file_beside`), so lichess-bot plays at them. A network never
/// tuned has none: an old `parameters.txt` is then removed, so the engine never plays one network
/// with another's parameters.
fn take_parameters(network_path: &str, dir: &str) -> Result<(), String> {
    let destination = format!("{dir}/{PARAMETERS_FILE}");
    let parameters_path = beside(network_path, PARAMETERS_FILE);
    if std::path::Path::new(&parameters_path).is_file() {
        copy_into_place(&parameters_path, &destination)?;
        log::info!("snapshot: the tuned parameters {parameters_path} as {destination}");
        return Ok(());
    }
    remove_if_present(&destination)?;
    log::info!("snapshot: no tuned parameters beside the network: the engine plays at the default parameters");
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

/// `kalo-chess.txt`'s fields for a snapshot of pesto: no weights to name.
fn pesto_snapshot_record(commit: &str, taken: &str) -> Vec<(String, String)> {
    vec![
        ("network".to_string(), PESTO.to_string()),
        ("commit".to_string(), commit.to_string()),
        ("taken".to_string(), taken.to_string()),
    ]
}

/// `kalo-chess.txt`'s fields: what was taken, and when. `record` is the record beside the network, its
/// `candidate.txt`, carried over field by field with `candidate_` as a prefix.
fn snapshot_record(network: &str, record: &str, commit: &str, taken: &str) -> Vec<(String, String)> {
    let mut fields = vec![("network".to_string(), network.to_string())];
    for line in record.lines().filter(|line| !line.is_empty()) {
        let (field, value) = line.split_once('\t').unwrap_or((line, ""));
        fields.push((format!("candidate_{field}"), value.to_string()));
    }
    fields.push(("commit".to_string(), commit.to_string()));
    fields.push(("taken".to_string(), taken.to_string()));
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

/// The engine's file name, both where `cargo build --release` leaves it and in the snapshot.
fn engine_file_name() -> String {
    format!("kalo-chess{}", std::env::consts::EXE_SUFFIX)
}

/// Where `cargo build --release` leaves the engine, relative to the repository root every binary
/// runs from.
fn engine_build_path() -> String {
    format!("target/release/{}", engine_file_name())
}

/// Runs `cargo build --release --bin kalo-chess`: only `kalo-chess`, never the `arena` that is running. Its
/// output goes to the log, a line at a time once it ends.
fn build_engine() -> Result<(), String> {
    log::info!("snapshot: building a release kalo-chess");
    let output = process::Command::new("cargo")
        .args(["build", "--release", "--bin", "kalo-chess"])
        .output()
        .map_err(|error| format!("failed to run cargo: {error}"))?;
    for line in String::from_utf8_lossy(&output.stderr).lines() {
        log::info!("snapshot: {line}");
    }
    if !output.status.success() {
        return Err(format!("cargo build --release --bin kalo-chess failed ({}): no snapshot was taken", output.status));
    }
    Ok(())
}

/// Copies `source` to `destination` through `destination.partial` and a rename. The rename matters
/// for the engine: Windows refuses to replace a binary lichess-bot is running, while a rename leaves
/// the running process its old file (`move_aside`).
fn copy_into_place(source: &str, destination: &str) -> Result<(), String> {
    let staging_path = format!("{destination}.partial");
    std::fs::copy(source, &staging_path).map_err(|error| format!("failed to copy {source} to {staging_path}: {error}"))?;
    move_aside(destination)?;
    std::fs::rename(&staging_path, destination).map_err(|error| format!("failed to rename {staging_path} to {destination}: {error}"))
}

/// Windows refuses to replace or delete a file a process has open, a running `kalo-chess.exe` above all,
/// but lets it be renamed. So `destination` is renamed aside to `{destination}.old-{seconds}`, its
/// process keeping its file, and the rename into place then finds the name free. The asides of
/// earlier snapshots are removed first, best-effort: one whose engine still runs stays until a later
/// snapshot. Seconds in the name, so an aside still running never blocks a newer one.
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

    fn request(network: &str) -> SnapshotRequest {
        SnapshotRequest { boot: "b00t".to_string(), network: network.to_string() }
    }

    #[test]
    fn a_snapshot_takes_pesto_or_a_network_in_the_store() {
        let network = "data/chess/networks/fine/run-1/gen-2/champion.nnue";
        assert_eq!(Snapshot::of(&request(PESTO)), Ok(Snapshot { network: None }));
        assert_eq!(Snapshot::of(&request(network)), Ok(Snapshot { network: Some(network.to_string()) }));
        assert_eq!(Snapshot::of(&request(network)).unwrap().name(), network);
        for network in ["betty-g1-r1", "C:/elsewhere/champion.nnue", "data/../champion.nnue"] {
            assert!(Snapshot::of(&request(network)).is_err(), "{network}");
        }
    }

    #[test]
    fn snapshot_record_carries_the_record_beside_the_network() {
        let fields = snapshot_record("data/chess/networks/w.nnue", "lambda\t0.05\nstep\t120000\n", "abc123", "2026-10-22 14:03 UTC");
        let actual: Vec<(&str, &str)> = fields.iter().map(|(field, value)| (field.as_str(), value.as_str())).collect();
        assert_eq!(
            actual,
            [
                ("network", "data/chess/networks/w.nnue"),
                ("candidate_lambda", "0.05"),
                ("candidate_step", "120000"),
                ("commit", "abc123"),
                ("taken", "2026-10-22 14:03 UTC"),
            ]
        );
        let names: Vec<String> = snapshot_record("w.nnue", "", "unknown", "now").into_iter().map(|(field, _)| field).collect();
        assert_eq!(names, ["network", "commit", "taken"], "no record beside it");
    }

    #[test]
    fn the_pesto_snapshot_record_names_no_weights() {
        let names: Vec<String> = pesto_snapshot_record("abc123", "now").into_iter().map(|(field, _)| field).collect();
        assert_eq!(names, ["network", "commit", "taken"]);
    }
}
