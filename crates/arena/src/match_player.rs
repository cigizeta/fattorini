//! The players `arena match`, `sprt` and `duel` take on their command line. Kalo Chess's are built in
//! code, the same way `run_selection` builds its pairings'; every external engine's executable comes
//! from `data/chess/engines.yaml` (`Engines`), never from a name on the `PATH`. Between two Kalo Chess players,
//! at a fixed depth or under a clock, both play in-process, as in the gate. **Against an external
//! engine every one is a UCI subprocess**, so both sides play on equal footing: the same protocol,
//! the same kind of process, each managing its own time, and the same `Hash` and `Threads`
//! (`EQUAL_FOOTING_OPTIONS`).
//! Both sides probe the data root's Syzygy tables, named through `SyzygyPath`. An engine given a bad
//! `SyzygyPath` plays on without tables and says nothing, so the match loads them in-process first
//! (`pairing::load_tablebase`), which stops the run when they are missing.
//!
//! - `pesto`: Kalo Chess with the PeSTO evaluation.
//! - `stockfish-<level>`, level 0 to 20: the executable of the registry's `stockfish` entry, at that
//!   `Skill Level`. The bare `stockfish` only says where Stockfish is and is no player: full strength
//!   is `stockfish-20`, its one spelling, so its records never split between two folders.
//! - `stockfish-elo-<rating>`, rating 1320 to 3190: the same executable, limited to that calibrated
//!   rating through `UCI_LimitStrength` and `UCI_Elo`.
//! - Any other `kind: uci` engine of the registry, by its name, e.g. `reckless`, with its options.
//! - `<config>-g<g>-l<lambda>-r<n>`, e.g. `pesto-psq-g1-l0.95-r1`, a forge run's network
//!   (`players::parse_network_name`): Kalo Chess evaluating with that lambda's candidate in
//!   generation `g`.
//! - `<config>-g<g>-r<n>`, e.g. `pesto-psq-g1-r1`: generation `g`'s winner, as its `generations.tsv`
//!   records it. This machine's copy of the run first, then the remote's, where its closed
//!   generations move.
//!
//! A Kalo Chess player, pesto or a network, is always followed by what it searches with, so that its name
//! says it:
//!
//! - `@untuned`: the untuned `SearchParameters::DEFAULT`, whatever its `parameters.txt` says.
//! - `@tuned`: its tune, the `parameters.txt` beside a network or pesto's in
//!   `players::PESTO_TUNING_DIR`; refused when it has none.
//!
//! In-process, the suffix sets the parameters. Over UCI, against an external engine, the player is
//! built with its network and those parameters embedded, as a snapshot is (`snapshot::build_embedded`,
//! `build_uci_player`), into `UCI_PLAYERS_DIR`: the engine takes no option choosing either. An
//! external engine takes no suffix.
//!
//! The name is also the player's label in the PGN and the database.

use std::collections::HashMap;

use super::engines::{self, EngineEntry, EngineKind, DEFAULT_HASH_MB};
use super::{data_root, snapshot};
use kalo_chess::pipeline::{players, protocol, remote};
// Read by arena's other modules through this one, where every player's name is read.
pub use kalo_chess::pipeline::players::parse_network_name;
use kalo_chess::nnue::Tuning;
use kalo_chess::search::SearchParameters;

/// Stockfish's `Skill Level` option ranges over 0 to 20.
const MAX_STOCKFISH_SKILL: u64 = 20;

/// Stockfish's `UCI_Elo` range since Stockfish 16. A value outside it is ignored without a word, and
/// Stockfish would then play at full strength, so it is refused here instead.
const MIN_STOCKFISH_ELO: u64 = 1320;
const MAX_STOCKFISH_ELO: u64 = 3190;

/// Where a Kalo Chess player that plays over UCI is built, one binary per player name, rebuilt at
/// each match so it is of the current commit. Never the `bin/kalo-chess` wrapper: Windows cannot
/// spawn a Bash script, and a wrapper process in between would take the pin and the kill meant for
/// the engine.
const UCI_PLAYERS_DIR: &str = "target/uci-players";

/// The binary the Kalo Chess player `name` (suffix included) plays over UCI from.
fn uci_player_path(name: &str) -> String {
    format!("{UCI_PLAYERS_DIR}/{}{}", name.replace('@', "-"), std::env::consts::EXE_SUFFIX)
}

/// Builds the binary of `entry`, the player `name`, when it is a Kalo Chess player over UCI
/// (`resolve_player`): its network and parameters embedded, as a snapshot's. Any other entry needs
/// nothing. Minutes at most, the engine crate alone recompiled when only what it embeds changed.
pub fn build_uci_player(name: &str, entry: &EngineEntry) -> Result<(), String> {
    let Some(path) = entry.path.as_deref().filter(|path| path.starts_with(UCI_PLAYERS_DIR)) else {
        return Ok(());
    };
    let parameters_file = match entry.parameters {
        Some(parameters) if parameters != SearchParameters::DEFAULT => {
            let file = format!("{}.parameters.txt", path.trim_end_matches(std::env::consts::EXE_SUFFIX));
            std::fs::create_dir_all(UCI_PLAYERS_DIR).map_err(|error| format!("{UCI_PLAYERS_DIR}: {error}"))?;
            parameters.write_file(&file)?;
            Some(file)
        }
        _ => None,
    };
    let description = snapshot::describe(name, &protocol::current_commit(), &snapshot::now_text());
    snapshot::build_embedded(entry.net.as_deref(), parameters_file.as_deref(), &description, path)
}

/// Set on every UCI player, so equal footing does not rest on two engines' defaults happening to
/// agree: `DEFAULT_HASH_MB` until `set_hash` gives the run's `--hash`; one thread, so each side
/// searches alone.
const EQUAL_FOOTING_OPTIONS: [(&str, u64); 2] = [("Hash", DEFAULT_HASH_MB as u64), ("Threads", 1)];

/// Gives `entry` a table of `hash_mb`: sent as `Hash` to a UCI player, the table size of an in-process
/// one (`EngineEntry::hash_mb`). Every player of a run gets the same.
pub fn set_hash(entry: &mut EngineEntry, hash_mb: usize) {
    entry.options.insert("Hash".to_string(), serde_yaml::Value::Number((hash_mb as u64).into()));
}

/// The data root's Syzygy directories as one `SyzygyPath` value, joined with the OS path-list
/// separator both Kalo Chess and Stockfish split it on.
fn syzygy_path() -> String {
    let joined = std::env::join_paths(data_root::syzygy_directories()).unwrap_or_else(|error| {
        eprintln!("cannot join the Syzygy directories into one SyzygyPath: {error}");
        std::process::exit(1);
    });
    joined.to_string_lossy().into_owned()
}

/// A UCI player at `path`, with `options`, and then `EQUAL_FOOTING_OPTIONS` and the tablebases over
/// them: no option a registry entry sets can give one side a bigger table, more threads or other
/// tablebases.
fn uci_entry(path: &str, options: Vec<(&str, serde_yaml::Value)>) -> EngineEntry {
    let mut all: HashMap<String, serde_yaml::Value> =
        options.into_iter().map(|(name, value)| (name.to_string(), value)).collect();
    for (name, value) in EQUAL_FOOTING_OPTIONS {
        all.insert(name.to_string(), serde_yaml::Value::Number(value.into()));
    }
    all.insert("SyzygyPath".to_string(), serde_yaml::Value::String(syzygy_path()));
    EngineEntry {
        kind: EngineKind::Uci,
        path: Some(path.to_string()),
        net: None,
        options: all,
        parameters: None,
    }
}

/// The registry entry that says where Stockfish is, for `stockfish-<level>` and
/// `stockfish-elo-<rating>`.
const STOCKFISH_ENTRY: &str = "stockfish";

/// The external engines this machine has: `data/chess/engines.yaml`'s `kind: uci` entries, by name (see
/// `examples/engines.yaml`). Read once per run, so every game of it plays the same executables.
/// Without the file there are none, and a run of Kalo Chess against Kalo Chess needs none.
pub struct Engines {
    origin: String,
    entries: Option<HashMap<String, EngineEntry>>,
}

impl Engines {
    /// The registry of this machine, `engines::ENGINE_REGISTRY_PATH`.
    pub fn load() -> Result<Engines, String> {
        let entries = engines::read_registry(engines::ENGINE_REGISTRY_PATH)?;
        Engines::from_entries(entries, engines::ENGINE_REGISTRY_PATH)
    }

    /// No registry at all: enough for a player known to be Kalo Chess's.
    #[cfg(test)]
    pub fn none() -> Engines {
        Engines { origin: engines::ENGINE_REGISTRY_PATH.to_string(), entries: None }
    }

    /// `entries`' UCI engines, read from `origin`. The in-process kinds a tournament registers are no
    /// external engine and are left out. Refuses a name a built-in player has or could have.
    fn from_entries(entries: Option<HashMap<String, EngineEntry>>, origin: &str) -> Result<Engines, String> {
        let entries: Option<HashMap<String, EngineEntry>> =
            entries.map(|all| all.into_iter().filter(|(_, entry)| entry.kind == EngineKind::Uci).collect());
        for name in entries.iter().flat_map(|entries| entries.keys()) {
            check_engine_name(name).map_err(|message| format!("{origin}: {message}"))?;
        }
        Ok(Engines { origin: origin.to_string(), entries })
    }

    fn get(&self, name: &str) -> Option<&EngineEntry> {
        self.entries.as_ref().and_then(|entries| entries.get(name))
    }

    /// The entry `name`, which the player `player` needs, with an executable that exists, or what to
    /// write in the registry.
    fn executable(&self, name: &str, player: &str) -> Result<&EngineEntry, String> {
        let origin = &self.origin;
        let Some(entries) = &self.entries else {
            return Err(format!(
                "{player:?} needs {origin}, which does not exist: copy examples/engines.yaml there and set the path \
                 of each engine's executable"
            ));
        };
        let entry = entries.get(name).ok_or_else(|| {
            format!(
                "{player:?} needs an engine named {name} in {origin}, with the path of its executable (see \
                 examples/engines.yaml)"
            )
        })?;
        let path = entry.binary_path();
        if !std::path::Path::new(path).is_file() {
            return Err(format!("{origin}: the path of {name}, {path}, is not a file"));
        }
        Ok(entry)
    }
}

/// Refuses a registry name that a built-in player has or could have, so a name always means one
/// player: `pesto`, `stockfish-...`, a network's (`players::parse_network_name`), and anything
/// with `@`, which a Kalo Chess player's suffix would take apart. `stockfish` itself is the entry Stockfish's
/// players read, and is no player.
fn check_engine_name(name: &str) -> Result<(), String> {
    if name == "pesto" || name.starts_with("stockfish-") || parse_network_name(name).is_ok() {
        return Err(format!("{name:?} is a built-in player's name: give the engine another"));
    }
    if name.contains('@') {
        return Err(format!("{name:?} holds an @, which a Kalo Chess player's parameters follow: give the engine another name"));
    }
    Ok(())
}

/// The registered engine `entry` as a player: its executable and options, `handicap` over them, and
/// equal footing over everything (`uci_entry`).
fn external_entry(entry: &EngineEntry, handicap: Vec<(&str, serde_yaml::Value)>) -> EngineEntry {
    let mut options: Vec<(&str, serde_yaml::Value)> =
        entry.options.iter().map(|(name, value)| (name.as_str(), value.clone())).collect();
    options.extend(handicap);
    uci_entry(entry.binary_path(), options)
}

/// Whether `name` is an engine other than Kalo Chess, which can only play over UCI: one of Stockfish's
/// players, or an engine of `engines`.
pub fn is_external(name: &str, engines: &Engines) -> bool {
    name.starts_with("stockfish-") || engines.get(name).is_some()
}

/// Whether a run between `one` and `two` plays Kalo Chess in-process. Only when both players are Kalo Chess,
/// at a fixed depth or under a clock: then both play in-process, exactly as in the gate (same search,
/// same table, no subprocess per game, no protocol latency on either side), and each can carry its
/// own parameters (its suffix). Otherwise both play over UCI, so the two sides are on equal footing --
/// the same protocol, the same kind of process, the same `Hash` and `Threads`.
pub fn plays_in_process(one: &str, two: &str, engines: &Engines) -> bool {
    !is_external(one, engines) && !is_external(two, engines)
}

/// What a Kalo Chess player's name says it searches with, after its `@`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Suffix {
    /// `@untuned`: `SearchParameters::DEFAULT`.
    Untuned,
    /// `@tuned`: its tune's `parameters.txt`.
    Tuned,
}

/// `name` split into the player and its suffix, `None` when it has no `@`: `pesto-psq-g1-r1@tuned` is
/// `pesto-psq-g1-r1` with `Suffix::Tuned`.
fn split_suffix(name: &str) -> Result<(&str, Option<Suffix>), String> {
    let Some((player, suffix)) = name.split_once('@') else {
        return Ok((name, None));
    };
    match suffix {
        "untuned" => Ok((player, Some(Suffix::Untuned))),
        "tuned" => Ok((player, Some(Suffix::Tuned))),
        _ => Err(format!("{name:?}: a suffix is tuned (its tune) or untuned, as in {player}@tuned or {player}@untuned")),
    }
}

/// The parameters the Kalo Chess player `player` has tuned, `None` if it has none: pesto's in
/// `players::PESTO_TUNING_DIR`, a network's beside its file (`nnue::read_tuning_beside`).
fn tuned_parameters(player: &str, run_dir: &dyn Fn(&str) -> Result<String, String>) -> Result<Option<SearchParameters>, String> {
    if player == "pesto" {
        return players::pesto_parameters();
    }
    let path = players::resolve_network_file_in(player, run_dir)?;
    Ok(match kalo_chess::nnue::read_tuning_beside(&path)? {
        Tuning::Parameters(parameters) => Some(parameters),
        Tuning::Untuned => None,
    })
}

/// The player `name` names, or why it names none. `in_process` is `plays_in_process` for the whole
/// run: `pesto` and networks then play in-process; otherwise every player, Kalo Chess included, is
/// a UCI subprocess. An external engine's executable comes from `engines`.
pub fn resolve_player(name: &str, in_process: bool, engines: &Engines) -> Result<EngineEntry, String> {
    // This machine's copy of a run first, then the remote's, where its closed generations move.
    resolve_player_with(name, in_process, engines, &players::run_dir).or_else(|local| {
        let on_remote = |run: &str| -> Result<String, String> { Ok(remote::remote_path(&players::run_dir(run)?)) };
        resolve_player_with(name, in_process, engines, &on_remote)
            .map_err(|remote| if remote == local { local } else { format!("{local}; nor on the remote: {remote}") })
    })
}

/// An in-process player: the PeSTO evaluation when `net` is `None`, the network at `net`
/// otherwise.
fn in_process_entry(net: Option<String>) -> EngineEntry {
    let kind = if net.is_some() { EngineKind::Network } else { EngineKind::Builtin };
    EngineEntry { kind, path: None, net, options: HashMap::new(), parameters: None }
}

/// Kalo Chess with the network at `net`, or the PeSTO evaluation when `None`: in-process when
/// `in_process`; over UCI otherwise, a binary of its own with `net` embedded, whose path and
/// parameters `resolve_player_with` gives it once the suffix is known, and `build_uci_player` builds.
fn kalo_chess_entry(net: Option<String>, in_process: bool) -> EngineEntry {
    if in_process {
        return in_process_entry(net);
    }
    let mut entry = uci_entry(UCI_PLAYERS_DIR, Vec::new());
    entry.net = net;
    entry
}

/// `resolve_player`, with where a run's directory is, so the tests can use temporary ones.
fn resolve_player_with(name: &str, in_process: bool, engines: &Engines, run_dir: &dyn Fn(&str) -> Result<String, String>) -> Result<EngineEntry, String> {
    let (player, suffix) = split_suffix(name)?;
    let mut entry = resolve_player_without_suffix(player, in_process, engines, run_dir)?;
    if is_external(player, engines) {
        if suffix.is_some() {
            return Err(format!("{name:?}: an external engine takes no suffix, which only a Kalo Chess player has"));
        }
        return Ok(entry);
    }
    let Some(suffix) = suffix else {
        return Err(format!("{name:?}: a Kalo Chess player says what it searches with, {name}@tuned (its tune) or {name}@untuned"));
    };
    entry.parameters = Some(match suffix {
        Suffix::Untuned => SearchParameters::DEFAULT,
        Suffix::Tuned => tuned_parameters(player, run_dir)?.ok_or_else(|| format!("{name:?}: {player} has no tune, so no @tuned"))?,
    });
    if !in_process {
        entry.path = Some(uci_player_path(name));
    }
    Ok(entry)
}

/// `resolve_player_with` for a name with no `@` suffix.
fn resolve_player_without_suffix(name: &str, in_process: bool, engines: &Engines, run_dir: &dyn Fn(&str) -> Result<String, String>) -> Result<EngineEntry, String> {
    if name == "pesto" {
        return Ok(kalo_chess_entry(None, in_process));
    }
    if name == STOCKFISH_ENTRY {
        return Err(format!(
            "{name:?} only says where Stockfish is, in {}: Stockfish at full strength is stockfish-20",
            engines.origin
        ));
    }
    if let Some(elo) = name.strip_prefix("stockfish-elo-") {
        let elo: u64 = elo
            .parse()
            .ok()
            .filter(|elo| (MIN_STOCKFISH_ELO..=MAX_STOCKFISH_ELO).contains(elo))
            .ok_or_else(|| format!("{name:?}: a Stockfish Elo is {MIN_STOCKFISH_ELO} to {MAX_STOCKFISH_ELO}"))?;
        return Ok(external_entry(
            engines.executable(STOCKFISH_ENTRY, name)?,
            vec![("UCI_LimitStrength", serde_yaml::Value::Bool(true)), ("UCI_Elo", serde_yaml::Value::Number(elo.into()))],
        ));
    }
    if let Some(level) = name.strip_prefix("stockfish-") {
        let level: u64 = level
            .parse()
            .ok()
            .filter(|&level| level <= MAX_STOCKFISH_SKILL)
            .ok_or_else(|| format!("{name:?}: a Stockfish skill level is 0 to {MAX_STOCKFISH_SKILL}"))?;
        return Ok(external_entry(
            engines.executable(STOCKFISH_ENTRY, name)?,
            vec![("Skill Level", serde_yaml::Value::Number(level.into()))],
        ));
    }
    if engines.get(name).is_some() {
        return Ok(external_entry(engines.executable(name, name)?, Vec::new()));
    }

    let path = players::resolve_network_file_in(name, run_dir)?;
    Ok(kalo_chess_entry(Some(path), in_process))
}

/// Where every match between two players is recorded, one dated folder each:
/// `data/chess/matches/<champion>-vs-<challenger>/`.
pub fn pairing_dir(champion: &str, challenger: &str) -> String {
    format!("data/chess/matches/{champion}-vs-{challenger}")
}

/// A finished match's `result.txt`: the outcome and the final score. Its presence is what makes a
/// match finished, so the next run of the same pairing starts a new one rather than resuming it.
pub fn result_path(match_dir: &str) -> String {
    format!("{match_dir}/result.txt")
}

/// Writes `text` to the match's `result.txt`, via `.tmp` and a rename.
pub fn write_result(match_dir: &str, text: &str) {
    write_atomically(&result_path(match_dir), text);
}

fn write_atomically(path: &str, text: &str) {
    let tmp_path = format!("{path}.tmp");
    std::fs::write(&tmp_path, text).unwrap_or_else(|error| panic!("failed to write {tmp_path}: {error}"));
    std::fs::rename(&tmp_path, path).unwrap_or_else(|error| panic!("failed to rename {tmp_path} to {path}: {error}"));
}

/// What a match folder was created for, one line: the rule and the control, `duel at depth 9`.
pub fn spec_path(match_dir: &str) -> String {
    format!("{match_dir}/spec.txt")
}

/// Makes `match_dir` a folder for `spec`, or says why it is not one. `match`, `sprt` and `duel` share
/// a pairing's folders and `choose_match_dir` resumes the latest unfinished one, so without this an
/// interrupted best of 12 could be resumed as a duel, or under another control, mixing two runs in one
/// database.
///
/// A folder with no `spec.txt` is new and gets `spec`. A mismatch is refused rather than answered with
/// a new folder: an unfinished run should be noticed, not buried.
pub fn claim_spec(match_dir: &str, spec: &str) -> Result<(), String> {
    let path = spec_path(match_dir);
    let recorded = match std::fs::read_to_string(&path) {
        Ok(text) => text.trim().to_string(),
        Err(_) => {
            write_atomically(&path, &format!("{spec}\n"));
            return Ok(());
        }
    };
    if recorded != spec {
        return Err(format!(
            "{match_dir} is an unfinished \"{recorded}\", not \"{spec}\": finish it with the command that started it, or \
             remove the folder"
        ));
    }
    Ok(())
}

/// The folder this run of the pairing plays in: the latest dated folder under `pairing_dir` if it has
/// no `result.txt`, to resume it; otherwise a new one, `pairing_dir/<now>`. Only the latest counts: an
/// older unfinished folder behind a finished one is left as it is. Not created here.
pub fn choose_match_dir(pairing_dir: &str, now: &str) -> String {
    let mut dated: Vec<String> = match std::fs::read_dir(pairing_dir) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().is_dir())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| is_timestamp(name))
            .collect(),
        Err(_) => Vec::new(),
    };
    dated.sort();
    if let Some(latest) = dated.last() {
        let dir = format!("{pairing_dir}/{latest}");
        if !std::path::Path::new(&result_path(&dir)).exists() {
            return dir;
        }
        assert!(
            latest.as_str() != now,
            "{dir} is a match that finished this same minute: wait a minute before starting the next one"
        );
    }
    format!("{pairing_dir}/{now}")
}

/// The current time as `YYYYMMDD-HHMM`, UTC: a match folder's name, spelled like a self-play segment's
/// timestamp.
pub fn current_timestamp() -> String {
    let now = time::OffsetDateTime::now_utc();
    format!("{:04}{:02}{:02}-{:02}{:02}", now.year(), u8::from(now.month()), now.day(), now.hour(), now.minute())
}

/// Whether `name` is spelled `YYYYMMDD-HHMM`.
fn is_timestamp(name: &str) -> bool {
    name.len() == 13
        && name.bytes().enumerate().all(|(index, byte)| if index == 8 { byte == b'-' } else { byte.is_ascii_digit() })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every match player is a UCI subprocess with the same `Hash`, `Threads` and tablebases.
    fn assert_equal_footing(entry: &EngineEntry) {
        assert_eq!(entry.kind, EngineKind::Uci);
        assert_eq!(entry.options["Hash"], serde_yaml::Value::Number(16u64.into()));
        assert_eq!(entry.options["Threads"], serde_yaml::Value::Number(1u64.into()));
        let expected = std::env::join_paths(data_root::syzygy_directories()).unwrap();
        assert_eq!(entry.options["SyzygyPath"], serde_yaml::Value::String(expected.to_string_lossy().into_owned()));
    }

    /// An executable that exists, for registry entries: an empty file, never run by these tests.
    fn test_executable() -> String {
        let path = std::env::temp_dir().join(format!("arena-match-player-engine-{}.exe", std::process::id()));
        std::fs::write(&path, b"").unwrap();
        path.to_str().unwrap().to_string()
    }

    /// A UCI registry entry at `path` with `options`.
    fn registered(path: &str, options: &[(&str, serde_yaml::Value)]) -> EngineEntry {
        EngineEntry {
            kind: EngineKind::Uci,
            path: Some(path.to_string()),
            net: None,
            options: options.iter().map(|(name, value)| (name.to_string(), value.clone())).collect(),
            parameters: None,
        }
    }

    /// A registry with `stockfish` and `reckless`, both at `test_executable`.
    fn engines() -> Engines {
        let path = test_executable();
        let entries =
            HashMap::from([("stockfish".to_string(), registered(&path, &[])), ("reckless".to_string(), registered(&path, &[]))]);
        Engines::from_entries(Some(entries), "test-engines.yaml").unwrap()
    }

    #[test]
    fn pesto_over_uci_is_a_build_of_its_own_with_nothing_to_set() {
        let entry = resolve_player("pesto@untuned", false, &Engines::none()).unwrap();
        assert_equal_footing(&entry);
        assert_eq!(entry.binary_path(), uci_player_path("pesto@untuned"));
        assert_eq!((entry.net, entry.parameters), (None, Some(SearchParameters::DEFAULT)));
    }

    #[test]
    fn at_a_fixed_depth_pesto_plays_in_process_and_stockfish_stays_uci() {
        let entry = resolve_player("pesto@untuned", true, &engines()).unwrap();
        assert_eq!((entry.kind, entry.net), (EngineKind::Builtin, None));
        let entry = resolve_player("stockfish-5", true, &engines()).unwrap();
        assert_equal_footing(&entry);
    }

    /// The one number means the same table for both kinds of player.
    #[test]
    fn set_hash_sizes_a_uci_and_an_in_process_player_alike() {
        let mut uci = resolve_player("stockfish-5", false, &engines()).unwrap();
        let mut in_process = resolve_player("pesto@untuned", true, &engines()).unwrap();
        assert_eq!((uci.hash_mb(), in_process.hash_mb()), (16, 16), "the default");
        set_hash(&mut uci, 256);
        set_hash(&mut in_process, 256);
        assert_eq!((uci.hash_mb(), in_process.hash_mb()), (256, 256));
    }

    /// Both in-process or both over UCI, never one of each.
    #[test]
    fn kalo_chess_plays_in_process_only_against_kalo_chess() {
        let engines = engines();
        assert!(plays_in_process("amanda-g1-r1@tuned", "pesto@untuned", &engines));
        assert!(!plays_in_process("amanda-g1-r1@tuned", "stockfish-0", &engines), "an external engine puts both on UCI");
        assert!(!plays_in_process("stockfish-elo-1500", "pesto@untuned", &engines));
        assert!(!plays_in_process("reckless", "amanda-g1-r1@tuned", &engines), "a registered engine is external");
    }

    #[test]
    fn a_kalo_chess_player_always_says_what_it_searches_with() {
        let engines = engines();
        let entry = resolve_player("pesto@untuned", true, &engines).unwrap();
        assert_eq!((entry.kind, entry.parameters), (EngineKind::Builtin, Some(SearchParameters::DEFAULT)));
        assert!(resolve_player("pesto", true, &engines).unwrap_err().contains("says what it searches with"));
        assert!(resolve_player("pesto", false, &engines).is_err(), "over UCI too");
        for name in ["pesto@150", "pesto@x", "pesto@", "pesto@t2", "pesto@Tuned", "pesto@untuned+blend"] {
            assert!(resolve_player(name, true, &engines).is_err(), "{name:?}");
            assert!(resolve_player(name, false, &engines).is_err(), "{name:?}");
        }
        assert!(resolve_player("stockfish-5@untuned", false, &engines).is_err(), "an external engine takes no suffix");
        assert!(resolve_player("reckless@tuned", false, &engines).is_err());
    }

    #[test]
    fn stockfish_takes_a_skill_level_from_0_to_20() {
        let engines = engines();
        for level in [0u64, 20] {
            let entry = resolve_player(&format!("stockfish-{level}"), false, &engines).unwrap();
            assert_equal_footing(&entry);
            assert_eq!(entry.binary_path(), test_executable());
            assert_eq!(entry.options["Skill Level"], serde_yaml::Value::Number(level.into()));
        }
        for name in ["stockfish-21", "stockfish-x", "stockfish-"] {
            assert!(resolve_player(name, false, &engines).is_err(), "{name:?}");
        }
    }

    #[test]
    fn stockfish_takes_a_calibrated_elo_from_1320_to_3190() {
        let engines = engines();
        for elo in [1320u64, 3190] {
            let entry = resolve_player(&format!("stockfish-elo-{elo}"), false, &engines).unwrap();
            assert_equal_footing(&entry);
            assert_eq!(entry.binary_path(), test_executable());
            assert_eq!(entry.options["UCI_LimitStrength"], serde_yaml::Value::Bool(true));
            assert_eq!(entry.options["UCI_Elo"], serde_yaml::Value::Number(elo.into()));
            assert!(!entry.options.contains_key("Skill Level"));
        }
        for name in ["stockfish-elo-1319", "stockfish-elo-3191", "stockfish-elo-x", "stockfish-elo-"] {
            assert!(resolve_player(name, false, &engines).is_err(), "{name:?}");
        }
    }

    #[test]
    fn the_bare_stockfish_is_no_player() {
        let error = resolve_player("stockfish", false, &engines()).unwrap_err();
        assert!(error.contains("stockfish-20"), "{error}");
    }

    #[test]
    fn a_registered_engine_plays_with_its_options_under_equal_footing() {
        let path = test_executable();
        let reckless = registered(
            &path,
            &[("Move Overhead", serde_yaml::Value::Number(50u64.into())), ("Threads", serde_yaml::Value::Number(8u64.into()))],
        );
        let engines = Engines::from_entries(Some(HashMap::from([("reckless".to_string(), reckless)])), "test").unwrap();
        let entry = resolve_player("reckless", false, &engines).unwrap();
        assert_equal_footing(&entry);
        assert_eq!(entry.binary_path(), path);
        assert_eq!(entry.options["Move Overhead"], serde_yaml::Value::Number(50u64.into()), "its own option");
    }

    #[test]
    fn stockfish_without_a_registry_or_an_entry_or_a_file_is_refused() {
        let error = resolve_player("stockfish-5", false, &Engines::none()).unwrap_err();
        assert!(error.contains("examples/engines.yaml"), "{error}");

        let path = test_executable();
        let only_reckless = HashMap::from([("reckless".to_string(), registered(&path, &[]))]);
        let error = resolve_player("stockfish-5", false, &Engines::from_entries(Some(only_reckless), "test").unwrap()).unwrap_err();
        assert!(error.contains("an engine named stockfish"), "{error}");

        let missing = HashMap::from([("stockfish".to_string(), registered("C:/no/such/stockfish.exe", &[]))]);
        let error = resolve_player("stockfish-5", false, &Engines::from_entries(Some(missing), "test").unwrap()).unwrap_err();
        assert!(error.contains("is not a file"), "{error}");
    }

    #[test]
    fn a_registry_name_a_built_in_player_has_is_refused() {
        let path = test_executable();
        for name in ["pesto", "stockfish-5", "stockfish-elo-1500", "amanda-g1-r1", "amanda-g1-l0.70-r1", "kalo-chess@tuned"] {
            let entries = HashMap::from([(name.to_string(), registered(&path, &[]))]);
            assert!(Engines::from_entries(Some(entries), "test").is_err(), "{name:?}");
        }
        let entries = HashMap::from([("stockfish".to_string(), registered(&path, &[]))]);
        assert!(Engines::from_entries(Some(entries), "test").is_ok(), "the entry Stockfish's players read");
    }

    #[test]
    fn a_forge_runs_player_plays_its_candidate_over_uci_or_in_process() {
        let root = std::env::temp_dir().join(format!("arena-run-player-{}", std::process::id()));
        let root = root.to_str().unwrap().to_string();
        std::fs::remove_dir_all(&root).ok();
        let runs = |run: &str| -> Result<String, String> { Ok(format!("{root}/runs/{run}")) };
        let candidate = format!("{root}/runs/fine-r1/gen-1/lambda-0.05/candidate.nnue");
        std::fs::create_dir_all(std::path::Path::new(&candidate).parent().unwrap()).unwrap();
        std::fs::write(&candidate, b"").unwrap();

        let name = "fine-g1-l0.05-r1";
        let entry = resolve_player_with(&format!("{name}@untuned"), false, &Engines::none(), &runs).unwrap();
        assert_equal_footing(&entry);
        assert_eq!(entry.binary_path(), uci_player_path(&format!("{name}@untuned")));
        assert_eq!((entry.net.clone(), entry.parameters), (Some(candidate.clone()), Some(SearchParameters::DEFAULT)), "what its build embeds");
        let entry = resolve_player_with(&format!("{name}@untuned"), true, &Engines::none(), &runs).unwrap();
        assert_eq!((entry.kind, entry.net, entry.parameters), (EngineKind::Network, Some(candidate.clone()), Some(SearchParameters::DEFAULT)));
        let error = resolve_player_with("other-g1-r1@untuned", false, &Engines::none(), &runs).unwrap_err();
        assert!(error.contains("no forge run"), "{error}");
        for name in ["fine", "fine-g0-r1", "fine-x", "-g1-r1", "fine-g1-l1.50-r1", "fine-g1-l0.x-r1", "fine-0001-0.05", "fine-g1-l0.5-r1", "fine-g1-a1-l0.05-r1"] {
            assert!(resolve_player_with(&format!("{name}@untuned"), false, &Engines::none(), &runs).is_err(), "{name:?}");
        }

        // Untuned: no @tuned.
        for in_process in [false, true] {
            assert!(resolve_player_with(name, in_process, &Engines::none(), &runs).is_err(), "no suffix");
            assert!(resolve_player_with(&format!("{name}@tuned"), in_process, &Engines::none(), &runs).is_err(), "no tune");
        }
        // Tuned: @tuned is its parameters.txt, in-process and over UCI alike, and @untuned the defaults.
        let tuned = SearchParameters { razoring_margin: 120, ..SearchParameters::DEFAULT };
        tuned.write_file(&kalo_chess::nnue::parameters_file_beside(&candidate)).unwrap();
        for in_process in [false, true] {
            let entry = resolve_player_with(&format!("{name}@tuned"), in_process, &Engines::none(), &runs).unwrap();
            assert_eq!(entry.parameters, Some(tuned));
            let entry = resolve_player_with(&format!("{name}@untuned"), in_process, &Engines::none(), &runs).unwrap();
            assert_eq!(entry.parameters, Some(SearchParameters::DEFAULT));
        }
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_pairing_is_recorded_under_both_names() {
        assert_eq!(pairing_dir("stockfish-0", "amanda-0001"), "data/chess/matches/stockfish-0-vs-amanda-0001");
    }

    #[test]
    fn the_latest_unfinished_match_is_resumed_and_a_finished_one_starts_a_new_one() {
        let pairing = std::env::temp_dir().join(format!("kalo-chess-evolve-match-dirs-{}", std::process::id()));
        let pairing = pairing.to_str().unwrap().to_string();
        std::fs::remove_dir_all(&pairing).ok();

        assert_eq!(choose_match_dir(&pairing, "20260927-1200"), format!("{pairing}/20260927-1200"), "no folder yet");

        std::fs::create_dir_all(format!("{pairing}/20260927-1000")).unwrap();
        std::fs::create_dir_all(format!("{pairing}/20260927-1100")).unwrap();
        std::fs::create_dir_all(format!("{pairing}/not-a-date")).unwrap();
        assert_eq!(choose_match_dir(&pairing, "20260927-1200"), format!("{pairing}/20260927-1100"), "resume the latest");

        write_result(&format!("{pairing}/20260927-1100"), "drawn\n");
        assert_eq!(
            choose_match_dir(&pairing, "20260927-1200"),
            format!("{pairing}/20260927-1200"),
            "a finished latest starts a new one, and the older unfinished 1000 is not resumed"
        );
        assert_eq!(std::fs::read_to_string(result_path(&format!("{pairing}/20260927-1100"))).unwrap(), "drawn\n");
        std::fs::remove_dir_all(&pairing).ok();
    }

    #[test]
    fn a_match_folder_is_claimed_once_and_refused_to_another_spec() {
        let dir = std::env::temp_dir().join(format!("arena-claim-spec-{}", std::process::id()));
        let dir = dir.to_str().unwrap().to_string();
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        let best_of = "best-of 12 at 3+2";
        let sprt = "sprt elo_d 35 at 3+2";

        claim_spec(&dir, sprt).expect("a new folder takes any spec");
        assert_eq!(std::fs::read_to_string(spec_path(&dir)).unwrap(), format!("{sprt}\n"));
        claim_spec(&dir, sprt).expect("the same spec resumes");
        let refused = claim_spec(&dir, best_of).unwrap_err();
        assert!(refused.contains(sprt) && refused.contains(best_of), "{refused}");
        assert!(claim_spec(&dir, "sprt elo_d 10 at 3+2").is_err(), "another margin");
        assert!(claim_spec(&dir, "sprt elo_d 35 at depth 9").is_err(), "another control");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_timestamp_is_yyyymmdd_hhmm() {
        assert!(is_timestamp(&current_timestamp()));
        assert!(is_timestamp("20260927-1030"));
        for name in ["2026092-1030", "20260927_1030", "20260927-103a", "not-a-date"] {
            assert!(!is_timestamp(name), "{name:?}");
        }
    }
}
