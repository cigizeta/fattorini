//! `arena snapshot <player>@<tuned|untuned>`: a release build of `fattorini` with that player's
//! network and parameters embedded, and a description of itself (`build.rs`), written straight to
//! Drive's `data/snapshots/` (never locally), under a name saying what it plays and when it was
//! taken: `fattorini-<player>-<tuned|untuned>-<YYYYMMDD-HHMM>.exe`, UTC. A command line feature only,
//! run on any machine, no smithy nor claim involved. One a player and suffix: a new
//! snapshot replaces the older ones of the same player and suffix. lichess-bot's `config.yml` names the one it plays.
//!
//! The same build (`build_embedded`) gives a match against an external engine its Fattorini side
//! (`match_player.rs`), and a release its `.exe` (`release.rs`).
//!
//! Nothing here, nor anywhere in the repository, writes outside it and Drive.

use std::process;

use fattorini::pipeline::{players, protocol, remote};
use fattorini::search::SearchParameters;

use super::match_player::{self, Engines};

/// Where a build with something embedded goes, never `target/release`: the plain build there embeds
/// nothing, and every match and the bench count on that.
const EMBEDDED_TARGET_DIR: &str = "target/snapshot";

/// Where snapshots go, as a path under `data/`: written only on Drive, never locally.
const SNAPSHOTS_DIR: &str = "data/snapshots";

/// `arena snapshot <player>@<tuned|untuned>`: takes the snapshot, or says why not and exits.
pub(crate) fn command(rest: &[String]) {
    let [name] = rest else {
        eprintln!("usage: arena snapshot <player>@<tuned|untuned>, the player pesto or a generation's winner");
        process::exit(2);
    };
    match take(name) {
        Ok(path) => println!("took {path}"),
        Err(message) => {
            eprintln!("{message}");
            process::exit(1);
        }
    }
}

/// Takes a snapshot of `name`, a player with its suffix: its network and parameters as a match
/// against an external engine resolves them (this machine's copy first, then Drive's), the file named
/// then built, and the same player's older snapshots removed. `Ok` with the file written.
fn take(name: &str) -> Result<String, String> {
    let Some((player, suffix)) = name.split_once('@') else {
        return Err(format!("{name:?}: say what it searches with, {name}@tuned or {name}@untuned"));
    };
    may_snapshot(player)?;
    // Fattorini players only reach here, so the registry is read only to tell them from an engine.
    let entry = match_player::resolve_player(name, false, &Engines::load()?)?;
    // Straight to Drive, so every machine (the Lichess one included) runs the same file.
    remote::check_mounted().map_err(|error| error.to_string())?;
    let dir = remote::remote_path(SNAPSHOTS_DIR);
    let now = time::OffsetDateTime::now_utc();
    let stamp = format!("{:04}{:02}{:02}-{:02}{:02}", now.year(), u8::from(now.month()), now.day(), now.hour(), now.minute());
    let prefix = format!("fattorini-{player}-{suffix}-");
    let stem = format!("{prefix}{stamp}");
    let out = format!("{dir}/{stem}.exe");
    let parameters_file = parameters_file(entry.parameters, EMBEDDED_TARGET_DIR, &stem)?;
    let description = describe(name, &protocol::current_commit(), &now_text());
    let built = build_embedded(&EmbeddedBuild::plain(entry.net.as_deref(), parameters_file.as_deref(), &description), &out);
    if let Some(file) = &parameters_file {
        std::fs::remove_file(file).ok();
    }
    built?;
    // One snapshot a player and suffix: the older ones go, only once the new one is in place.
    let older = remote::list(SNAPSHOTS_DIR, &|file| is_older_snapshot(file, &prefix, &stem)).map_err(|error| error.to_string())?;
    for file in older {
        let path = format!("{dir}/{file}");
        std::fs::remove_file(&path).map_err(|error| format!("took {out}, but failed to remove {path}: {error}"))?;
        println!("removed {path}");
    }
    Ok(out)
}

/// The parameters as a local file in `dir`, for `build.rs`, beside the build while it runs; none for
/// the untuned defaults, which need no file. The caller removes it after the build.
pub(crate) fn parameters_file(parameters: Option<SearchParameters>, dir: &str, stem: &str) -> Result<Option<String>, String> {
    match parameters {
        Some(parameters) if parameters != SearchParameters::DEFAULT => {
            std::fs::create_dir_all(dir).map_err(|error| format!("failed to create {dir}: {error}"))?;
            let file = format!("{dir}/{stem}.parameters.txt");
            parameters.write_file(&file)?;
            Ok(Some(file))
        }
        _ => Ok(None),
    }
}

/// Whether `file` is another snapshot of the player and suffix `prefix` names: the prefix, then a
/// stamp alone (`YYYYMMDD-HHMM.exe`), so `pesto-untuned-` never matches `pesto-psq-g1-r1-untuned-`'s
/// files. Not `stem` itself, the one just written.
fn is_older_snapshot(file: &str, prefix: &str, stem: &str) -> bool {
    let Some(rest) = file.strip_prefix(prefix).and_then(|rest| rest.strip_suffix(".exe")) else { return false };
    let stamp = rest.as_bytes();
    stamp.len() == 13
        && stamp[8] == b'-'
        && stamp.iter().enumerate().all(|(i, byte)| i == 8 || byte.is_ascii_digit())
        && file != format!("{stem}.exe")
}

/// Whether `player` may be snapshotted (the developer, 2026-10-29, kept 2026-11-06): pesto, or a full
/// run's generation winner, one that passed its gate; never a candidate nor a scaled run's network,
/// whose strength means nothing outside its run.
/// A release follows the same rule.
pub(crate) fn may_snapshot(player: &str) -> Result<(), String> {
    if player == protocol::PESTO {
        return Ok(());
    }
    let parsed = players::parse_network_name(player)?;
    if parsed.candidate.is_some() {
        return Err(format!("{player} is a candidate: only pesto or a generation's winner is snapshotted"));
    }
    let local = players::run_dir(&parsed.run())?;
    let homes = [local.clone(), remote::remote_path(&local)];
    if homes.iter().all(|home| players::run_winner(home, parsed.generation).is_err()) {
        return Err(format!("{player} has no winner that passed its gate: only a generation's winner is snapshotted"));
    }
    if let Some(scale) = homes.iter().find_map(|home| scale_of(&format!("{home}/config.yaml"))) {
        return Err(format!("{player} is a scaled run's network (1/{scale}): it plays in its own run alone"));
    }
    Ok(())
}

/// The `scale:` of the frozen run config at `path`, `None` for a full run's or no file.
fn scale_of(path: &str) -> Option<u64> {
    let text = std::fs::read_to_string(path).ok()?;
    let config: serde_yaml::Value = serde_yaml::from_str(&text).ok()?;
    config.get("scale")?.as_u64()
}

/// What a build embeds as its description (`fattorini about`, UCI's `id name`), each word said
/// once: the player and its suffix, the commit (`-dirty` when modified, as git says it), and when,
/// as `pesto@untuned 7b89e3f-dirty 2026-11-06 09:36 UTC`.
pub(crate) fn describe(player: &str, commit: &str, taken: &str) -> String {
    let short_commit: String = commit.chars().take(7).collect();
    let dirty = if commit.contains("modified") { "-dirty" } else { "" };
    format!("{player} {short_commit}{dirty} {taken}")
}

/// A release's description: the tag, written whole, where a snapshot says the commit, as
/// `lc0-psq-threats-screlu-g1-r1@tuned v0.1.0 2026-11-20 10:00 UTC`.
pub(crate) fn describe_release(player: &str, tag: &str, built: &str) -> String {
    format!("{player} {tag} {built}")
}

/// The time now, as a description says it: `YYYY-MM-DD HH:MM UTC`.
pub(crate) fn now_text() -> String {
    let now = time::OffsetDateTime::now_utc();
    format!("{:04}-{:02}-{:02} {:02}:{:02} UTC", now.year(), u8::from(now.month()), now.day(), now.hour(), now.minute())
}

/// What `build_embedded` builds: the files and text embedded (`build.rs`), where cargo builds, and
/// whether the C runtime is linked statically.
pub(crate) struct EmbeddedBuild<'a> {
    /// The `.nnue`; none, the PeSTO evaluation.
    pub(crate) network: Option<&'a str>,
    /// The `parameters.txt`; none, the untuned defaults.
    pub(crate) parameters: Option<&'a str>,
    pub(crate) description: &'a str,
    /// The third-party notices, a release's alone.
    pub(crate) notices: Option<&'a str>,
    pub(crate) target_dir: &'a str,
    /// A release's: no Visual C++ Redistributable needed where it runs.
    pub(crate) static_runtime: bool,
}

impl<'a> EmbeddedBuild<'a> {
    /// A snapshot's or a match's build: in `EMBEDDED_TARGET_DIR`, no notices, the runtime dynamic.
    pub(crate) fn plain(network: Option<&'a str>, parameters: Option<&'a str>, description: &'a str) -> EmbeddedBuild<'a> {
        EmbeddedBuild { network, parameters, description, notices: None, target_dir: EMBEDDED_TARGET_DIR, static_runtime: false }
    }
}

/// The flags of a build with the C runtime static. `RUSTFLAGS` replaces `.cargo/config.toml`'s
/// `rustflags` rather than adding to them, so the CPU level is said again.
const STATIC_RUSTFLAGS: &str = "-C target-cpu=x86-64-v3 -C target-feature=+crt-static";

/// Builds `fattorini` in release as `build` says, then copies the binary to `out`. cargo's own
/// output goes to the terminal as it builds.
pub(crate) fn build_embedded(build: &EmbeddedBuild, out: &str) -> Result<(), String> {
    println!("building fattorini for {out}: {}", build.description);
    let mut command = process::Command::new("cargo");
    command.args(["build", "--release", "--bin", "fattorini", "--target-dir", build.target_dir]);
    // Absolute: cargo runs `build.rs` from the engine's crate directory, not from here, so a path
    // relative to the repository's root would name nothing there.
    let absolute = |path: &str| {
        std::path::absolute(path).map(|path| path.into_os_string()).map_err(|error| format!("{path}: {error}"))
    };
    for (variable, path) in [("FATTORINI_EVALFILE", build.network), ("FATTORINI_PARAMETERS", build.parameters), ("FATTORINI_NOTICES", build.notices)] {
        match path {
            Some(path) => command.env(variable, absolute(path)?),
            None => command.env_remove(variable),
        };
    }
    command.env("FATTORINI_DESCRIPTION", build.description);
    if build.static_runtime {
        command.env("RUSTFLAGS", STATIC_RUSTFLAGS);
    }
    let status = command.status().map_err(|error| format!("failed to run cargo: {error}"))?;
    if !status.success() {
        return Err(format!("cargo build --release --bin fattorini failed ({status}): nothing written"));
    }
    let built = format!("{}/release/fattorini{}", build.target_dir, std::env::consts::EXE_SUFFIX);
    // Via `.partial` and a rename: no reader, on Drive or in a match, sees a half-copied `.exe`.
    remote::copy_replacing(&built, out).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_description_says_the_player_the_commit_and_what_is_embedded() {
        let description = describe("pesto@untuned", "0123456789abcdef (modified)", "2026-11-06 15:32 UTC");
        assert_eq!(description, "pesto@untuned 0123456-dirty 2026-11-06 15:32 UTC");
        assert_eq!(describe("pesto@tuned", "0123456789abcdef", "2026-11-06 15:32 UTC"), "pesto@tuned 0123456 2026-11-06 15:32 UTC");
        assert_eq!(describe_release("pesto@tuned", "v2026.11.0-rc.2", "2026-11-06 15:32 UTC"), "pesto@tuned v2026.11.0-rc.2 2026-11-06 15:32 UTC");
    }

    #[test]
    fn a_new_snapshot_replaces_only_the_same_players_older_ones() {
        let prefix = "fattorini-pesto-untuned-";
        let stem = "fattorini-pesto-untuned-20261106-1532";
        assert!(is_older_snapshot("fattorini-pesto-untuned-20261105-0900.exe", prefix, stem));
        assert!(!is_older_snapshot("fattorini-pesto-untuned-20261106-1532.exe", prefix, stem));
        assert!(!is_older_snapshot("fattorini-pesto-tuned-20261105-0900.exe", prefix, stem));
        assert!(!is_older_snapshot("fattorini-pesto-psq-g1-r1-untuned-20261105-0900.exe", "fattorini-pesto-", stem));
        assert!(!is_older_snapshot("fattorini-pesto-untuned-20261105-0900.exe.partial", prefix, stem));
    }

    #[test]
    fn only_pesto_or_a_generations_winner_is_snapshotted() {
        assert!(may_snapshot("pesto").is_ok());
        assert!(may_snapshot("fine-g1-l0.05-r1").unwrap_err().contains("candidate"));
        assert!(may_snapshot("stockfish-5").is_err());
    }

    #[test]
    fn a_scaled_run_says_its_scale_in_its_frozen_config() {
        let dir = std::env::temp_dir().join(format!("arena-snapshot-scale-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml").to_str().unwrap().to_string();
        std::fs::write(&path, "kind: psq\nscale: 1024\n").unwrap();
        assert_eq!(scale_of(&path), Some(1024));
        std::fs::write(&path, "kind: psq\n").unwrap();
        assert_eq!(scale_of(&path), None);
        assert_eq!(scale_of("no/such/config.yaml"), None);
        std::fs::remove_dir_all(&dir).ok();
    }
}
