//! A snapshot, asked by the smithy (`protocol::SNAPSHOT_PATH`): a release build of `kalo-chess` with a
//! player's network and parameters embedded, and a description of itself (`build.rs`), written to
//! the data root's `chess/snapshots/` under a name saying what it plays and when it was taken:
//! `kalo-chess-<player>-<tuned|untuned>-<YYYYMMDD-HHMM>.exe`, UTC. Any number are kept, deleted by
//! hand; one is never replaced. lichess-bot's `config.yml` names the one it plays. Run on a thread of
//! the arena's own (`chores.rs`), its workers playing on.
//!
//! The same build (`build_embedded`) gives a match against an external engine its Kalo Chess side
//! (`match_player.rs`). A release will be built the same way.
//!
//! Nothing here, nor anywhere in the repository, writes outside it and Drive.

use std::process;
use std::sync::atomic::AtomicBool;

use kalo_chess::pipeline::data_root::data_root;
use kalo_chess::pipeline::protocol::{self, SnapshotRequest, MAX_SNAPSHOTS_SAID, PESTO};

use super::chores::{store_path, wait_for, Waited, NETWORK_POLL, NETWORK_TIMEOUT};

/// Where a build with something embedded goes, never `target/release`: the plain build there embeds
/// nothing, and every match and the bench count on that.
const EMBEDDED_TARGET_DIR: &str = "target/snapshot";

/// Every snapshot's file name starts so.
const PREFIX: &str = "kalo-chess-";

/// The snapshots' folder, under the data root.
fn snapshots_dir() -> String {
    format!("{}/chess/snapshots", data_root())
}

/// A snapshot the arena can take, as the smithy asked it, checked.
#[derive(Debug, PartialEq)]
pub(crate) struct Snapshot {
    /// The player's name, as the file name and the description say it.
    player: String,
    /// The network's path in the store, `None` for the PeSTO evaluation.
    network: Option<String>,
    /// The parameters' path in the store, `None` for the untuned defaults.
    parameters: Option<String>,
}

impl Snapshot {
    /// `request`, checked: a player name fit for a file name, `pesto` or a `.nnue` in the store, and
    /// a `parameters.txt` in the store or none.
    pub(crate) fn of(request: &SnapshotRequest) -> Result<Snapshot, String> {
        let fit = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.';
        if request.player.is_empty() || !request.player.chars().all(fit) {
            return Err(format!("the player {:?} is not a name a file may carry", request.player));
        }
        let network = match request.network.as_str() {
            PESTO => None,
            path if path.ends_with(".nnue") => Some(in_store(path)?),
            other => return Err(format!("the network is pesto or a .nnue in the store, got {other:?}")),
        };
        let parameters = match request.tuned() {
            false => None,
            true if request.parameters.ends_with("parameters.txt") => Some(in_store(&request.parameters)?),
            true => return Err(format!("the parameters are a parameters.txt in the store, got {:?}", request.parameters)),
        };
        Ok(Snapshot { player: request.player.clone(), network, parameters })
    }

    /// What it takes, for the log and the page: the player and its suffix.
    pub(crate) fn name(&self) -> String {
        format!("{}@{}", self.player, tuned_word(self.parameters.is_some()))
    }
}

/// `tuned` or `untuned`, as a player's suffix and a snapshot's name say it.
fn tuned_word(tuned: bool) -> &'static str {
    if tuned { "tuned" } else { "untuned" }
}

/// `path`, when it is a path in the store (`store_path`'s rule), as it is.
fn in_store(path: &str) -> Result<String, String> {
    store_path("", path)?;
    Ok(path.to_string())
}

/// Takes `snapshot`, its files read from `store`, waited for up to `NETWORK_TIMEOUT` until `stop`.
/// `Ok` with the file written, or `Err` with why none was.
pub(crate) fn run(snapshot: &Snapshot, store: &str, stop: &AtomicBool) -> Result<String, String> {
    let network = snapshot.network.as_deref().map(|name| fetch(name, store, stop)).transpose()?;
    let parameters = snapshot.parameters.as_deref().map(|name| fetch(name, store, stop)).transpose()?;
    let now = time::OffsetDateTime::now_utc();
    let stamp = format!("{:04}{:02}{:02}-{:02}{:02}", now.year(), u8::from(now.month()), now.day(), now.hour(), now.minute());
    let dir = snapshots_dir();
    std::fs::create_dir_all(&dir).map_err(|error| format!("failed to create {dir}: {error}"))?;
    let file = format!("{PREFIX}{}-{}-{stamp}.exe", snapshot.player, tuned_word(parameters.is_some()));
    let out = format!("{dir}/{file}");
    if std::path::Path::new(&out).exists() {
        return Err(format!("{file} exists already: one snapshot of a player a minute"));
    }
    let description = describe(&snapshot.name(), &protocol::current_commit(), &now_text(), network.as_deref(), parameters.as_deref())?;
    build_embedded(network.as_deref(), parameters.as_deref(), &description, &out)?;
    Ok(format!("took {out}"))
}

/// What a build embeds as its description (`kalo-chess about`, UCI's `id name`): the player and its
/// suffix, the commit, when, and a hash of each embedded file, so two builds of one name but other
/// files tell themselves apart.
pub(crate) fn describe(player: &str, commit: &str, taken: &str, network: Option<&str>, parameters: Option<&str>) -> Result<String, String> {
    let hash = |path: Option<&str>, none: &str| -> Result<String, String> {
        match path {
            None => Ok(none.to_string()),
            Some(path) => std::fs::read(path).map(|bytes| format!("{:016x}", fnv1a(&bytes))).map_err(|error| format!("{path}: {error}")),
        }
    };
    let short_commit: String = commit.chars().take(12).collect();
    let modified = if commit.contains("modified") { " (modified)" } else { "" };
    Ok(format!(
        "{player}, commit {short_commit}{modified}, taken {taken}, network {}, parameters {}",
        hash(network, "PeSTO")?,
        hash(parameters, "untuned")?
    ))
}

/// The time now, as a description says it: `YYYY-MM-DD HH:MM UTC`.
pub(crate) fn now_text() -> String {
    let now = time::OffsetDateTime::now_utc();
    format!("{:04}-{:02}-{:02} {:02}:{:02} UTC", now.year(), u8::from(now.month()), now.day(), now.hour(), now.minute())
}

/// FNV-1a, 64 bits: enough to tell two files apart at a glance, with no dependency for it.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3))
}

/// Builds `kalo-chess` in release with `network` and `parameters` embedded (none: the PeSTO
/// evaluation, the untuned defaults) and `description`, in `EMBEDDED_TARGET_DIR`, then copies the
/// binary to `out`. Its output goes to the log, a line at a time once it ends.
pub(crate) fn build_embedded(network: Option<&str>, parameters: Option<&str>, description: &str, out: &str) -> Result<(), String> {
    log::info!("building kalo-chess for {out}: {description}");
    let mut command = process::Command::new("cargo");
    command.args(["build", "--release", "--bin", "kalo-chess", "--target-dir", EMBEDDED_TARGET_DIR]);
    for (variable, value) in [("KALO_CHESS_EVALFILE", network), ("KALO_CHESS_PARAMETERS", parameters), ("KALO_CHESS_DESCRIPTION", Some(description))] {
        match value {
            Some(value) => command.env(variable, value),
            None => command.env_remove(variable),
        };
    }
    let output = command.output().map_err(|error| format!("failed to run cargo: {error}"))?;
    for line in String::from_utf8_lossy(&output.stderr).lines() {
        log::info!("build: {line}");
    }
    if !output.status.success() {
        return Err(format!("cargo build --release --bin kalo-chess failed ({}): nothing written", output.status));
    }
    let built = format!("{EMBEDDED_TARGET_DIR}/release/kalo-chess{}", std::env::consts::EXE_SUFFIX);
    if let Some(dir) = std::path::Path::new(out).parent() {
        std::fs::create_dir_all(dir).map_err(|error| format!("failed to create {}: {error}", dir.display()))?;
    }
    std::fs::copy(&built, out).map_err(|error| format!("failed to copy {built} to {out}: {error}"))?;
    Ok(())
}

/// The file names of this machine's snapshots, newest first by the stamp ending each, at most
/// `MAX_SNAPSHOTS_SAID`; none before a first one.
pub(crate) fn snapshots() -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(snapshots_dir()) else {
        return Vec::new();
    };
    let mut files: Vec<String> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with(PREFIX) && name.ends_with(".exe"))
        .collect();
    files.sort_by_key(|name| std::cmp::Reverse(stamp_of(name)));
    files.truncate(MAX_SNAPSHOTS_SAID);
    files
}

/// The `YYYYMMDD-HHMM` ending a snapshot's file name, so the newest sorts first whatever its player.
fn stamp_of(file: &str) -> String {
    let stem = file.trim_end_matches(".exe");
    stem.get(stem.len().saturating_sub(13)..).unwrap_or("").to_string()
}

/// The file `name` names, read local first, this machine's copy when it holds one (`name` is the
/// same `data/...` path in the repository's tree), else from `store`, waited for there. The smithy
/// puts the parameters in the store before the network.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn request(player: &str, network: &str, parameters: &str) -> SnapshotRequest {
        SnapshotRequest { boot: "b00t".to_string(), player: player.to_string(), network: network.to_string(), parameters: parameters.to_string() }
    }

    #[test]
    fn a_snapshot_takes_pesto_or_a_network_in_the_store() {
        let network = "data/chess/networks/fine/run-1/gen-2/champion.nnue";
        let parameters = "data/chess/networks/fine/run-1/gen-2/parameters.txt";
        assert_eq!(
            Snapshot::of(&request(PESTO, PESTO, protocol::UNTUNED)),
            Ok(Snapshot { player: PESTO.to_string(), network: None, parameters: None })
        );
        let tuned = Snapshot::of(&request("fine-g2-r1", network, parameters)).unwrap();
        assert_eq!(tuned.name(), "fine-g2-r1@tuned");
        assert_eq!(Snapshot::of(&request("fine-g2-r1", network, protocol::UNTUNED)).unwrap().name(), "fine-g2-r1@untuned");
        for network in ["betty-g1-r1", "C:/elsewhere/champion.nnue", "data/../champion.nnue"] {
            assert!(Snapshot::of(&request("fine-g2-r1", network, protocol::UNTUNED)).is_err(), "{network}");
        }
        for player in ["", "Fine", "a@b", "a/b"] {
            assert!(Snapshot::of(&request(player, network, protocol::UNTUNED)).is_err(), "{player:?}");
        }
        assert!(Snapshot::of(&request("fine-g2-r1", network, "data/x.txt")).is_err());
    }

    #[test]
    fn the_newest_snapshot_comes_first_whatever_its_player() {
        let mut files = ["kalo-chess-zeta-g1-r1-tuned-20261106-0930.exe", "kalo-chess-pesto-untuned-20261106-1532.exe"];
        files.sort_by_key(|name| std::cmp::Reverse(stamp_of(name)));
        assert_eq!(files[0], "kalo-chess-pesto-untuned-20261106-1532.exe");
        assert_eq!(stamp_of("kalo-chess-pesto-untuned-20261106-1532.exe"), "20261106-1532");
    }

    #[test]
    fn a_description_says_the_player_the_commit_and_what_is_embedded() {
        let description = describe("pesto@untuned", "0123456789abcdef (modified)", "2026-11-06 15:32 UTC", None, None).unwrap();
        assert_eq!(description, "pesto@untuned, commit 0123456789ab (modified), taken 2026-11-06 15:32 UTC, network PeSTO, parameters untuned");
        assert_ne!(fnv1a(b"a"), fnv1a(b"b"));
    }
}
