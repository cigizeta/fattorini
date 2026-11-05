//! `data/chess/engines.yaml`: the engine registry, each external engine named once with the path to its
//! executable and the UCI options to set on it, as cutechess keeps its `engines.json`. The players
//! of `match`, `sprt`, `duel` take every external engine from it (`match_player::Engines`), and a
//! tournament `config.yaml`'s `participants` resolve against it. Lives under `data/`, not
//! `configs/chess/`: a binary path is the machine's own business, so each developer writes the file by
//! hand from the committed `examples/engines.yaml`.

use std::collections::HashMap;

use temper_chess::nnue::Tuning;
use temper_chess::search::SearchParameters;
use serde::Deserialize;

/// The registry's one place, per machine and gitignored.
pub const ENGINE_REGISTRY_PATH: &str = "data/chess/engines.yaml";

/// What kind of player a registry entry names. `Uci` is the default, so every `engines.yaml`
/// written before this field existed keeps working unchanged.
///
/// `Builtin` and `Network` both become an `InProcessPlayer` and differ only in the evaluation they
/// carry. They exist because the gate compares two evaluations inside one search, which driving
/// Temper over UCI could not do: that would compare two whole engine builds instead, and would put a
/// subprocess boundary in the way of the one thing being measured.
#[derive(Debug, Deserialize, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EngineKind {
    #[default]
    Uci,
    Builtin,
    Network,
}

#[derive(Debug, Deserialize)]
pub struct EngineEntry {
    #[serde(default)]
    pub kind: EngineKind,
    /// Required for `kind: uci`, absent for the two in-process kinds -- which have no binary,
    /// being this process. `validate` below enforces that rather than leaving a `None` to be
    /// discovered at spawn time.
    #[serde(default)]
    pub path: Option<String>,
    /// The `.nnue` file, required for `kind: network` and meaningless for the other two.
    #[serde(default)]
    pub net: Option<String>,
    #[serde(default)]
    pub options: HashMap<String, serde_yaml::Value>,
    /// An in-process player's search parameters from a player name's suffix, `@default`, `@t1` or
    /// `@<cp>` (`SearchParameters::with_margin`); `None` plays the player's own tuning (`tuning`), as
    /// a player built from paths does (the gate's). Never set on a UCI entry: a subprocess cannot be
    /// given any.
    #[serde(skip)]
    pub parameters: Option<SearchParameters>,
}

pub use temper_chess::pipeline::players::pesto_parameters;

impl EngineEntry {
    /// The binary to spawn. Panics if absent, which `validate` has already ruled out at load time --
    /// so this is an assertion about a checked invariant rather than a case a caller has to handle.
    pub fn binary_path(&self) -> &str {
        self
            .path
            .as_deref()
            .expect("a uci engine entry always has a path, enforced when data/chess/engines.yaml is loaded")
    }

    /// The `.nnue` file for `kind: network`, likewise already checked present at load time.
    pub fn network_path(&self) -> &str {
        self
            .net
            .as_deref()
            .expect("a network engine entry always has a net, enforced when data/chess/engines.yaml is loaded")
    }

    /// The table size in MB: the `Hash` option, which a UCI player is sent and an in-process player
    /// sizes its own table by, so the one number means the same thing for both kinds.
    /// `DEFAULT_HASH_MB` when the entry names none.
    pub fn hash_mb(&self) -> usize {
        self.options.get("Hash").and_then(serde_yaml::Value::as_u64).map_or(DEFAULT_HASH_MB, |mb| mb as usize)
    }

    /// The tuning this in-process player plays when its name has no suffix: a network's files beside
    /// it (`nnue::read_tuning_beside`), pesto's `players::PESTO_TUNING_DIR`. `Untuned` for a UCI
    /// player, whose search is its own business.
    pub fn own_tuning(&self) -> Result<Tuning, String> {
        match (self.kind, &self.net) {
            (EngineKind::Network, Some(net)) => temper_chess::nnue::read_tuning_beside(net),
            (EngineKind::Builtin, _) => Ok(pesto_parameters()?.map_or(Tuning::Untuned, Tuning::Parameters)),
            _ => Ok(Tuning::Untuned),
        }
    }

    /// `name` with what this player searches with, for the run's header line: a suffixed name as it
    /// is, else `@default` for an untuned player, ` (parameters.txt)` for a tuned one; a UCI player's
    /// name as it is. A malformed file shows as untuned here, and the player's own start reports it.
    pub fn label(&self, name: &str) -> String {
        if name.contains('@') || self.kind == EngineKind::Uci {
            return name.to_string();
        }
        match self.own_tuning().unwrap_or(Tuning::Untuned) {
            Tuning::Untuned => format!("{name}@default"),
            Tuning::Parameters(_) => format!("{name} (parameters.txt)"),
        }
    }
}

/// Every player's table size unless `--hash` says otherwise: `bin/temper-chess`'s own UCI default, 16 MB,
/// also the self-play size. A depth-9 search visits some 20k-70k nodes, about 1 MB of entries, so
/// 16 MB leaves ample room for what carries over between moves.
pub const DEFAULT_HASH_MB: usize = temper_chess::uci::DEFAULT_HASH_MB;

#[derive(Debug, Deserialize)]
struct EngineRegistryFile {
    engines: HashMap<String, EngineEntry>,
}

pub struct EngineRegistry {
    entries: HashMap<String, EngineEntry>,
}

impl EngineRegistry {
    pub fn load(path: &str) -> EngineRegistry {
        let contents = std::fs::read_to_string(path).unwrap_or_else(|error| panic!("failed to read {path}: {error}"));
        let file: EngineRegistryFile =
            serde_yaml::from_str(&contents).unwrap_or_else(|error| panic!("failed to parse {path}: {error}"));
        for (name, entry) in &file.engines {
            validate(name, entry, path);
        }
        EngineRegistry { entries: file.engines }
    }

    /// A registry built in memory rather than read from `data/chess/engines.yaml`, validated the same way.
    /// For the gate, whose two players are derived from a network's generation directory rather than
    /// registered by hand.
    pub fn from_entries(entries: HashMap<String, EngineEntry>, origin: &str) -> EngineRegistry {
        for (name, entry) in &entries {
            validate(name, entry, origin);
        }
        EngineRegistry { entries }
    }

    /// Resolves a bare engine name (as it appears in a tournament `config.yaml`'s
    /// `participants` list) to its registry entry. Panics on an unregistered name -- a
    /// tournament referencing an engine `data/chess/engines.yaml` doesn't know about is a config
    /// error, not something to run around (CLAUDE.md rule 4: assertions, not eyeballing).
    pub fn resolve(&self, name: &str) -> &EngineEntry {
        self
            .entries
            .get(name)
            .unwrap_or_else(|| panic!("engine {name:?} is not registered"))
    }
}

/// The entries of the registry at `path`, each checked by `validate`, or `None` when there is no such
/// file: a run that needs no external engine needs no registry either.
pub fn read_registry(path: &str) -> Result<Option<HashMap<String, EngineEntry>>, String> {
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("failed to read {path}: {error}")),
    };
    let file: EngineRegistryFile =
        serde_yaml::from_str(&contents).map_err(|error| format!("failed to parse {path}: {error}"))?;
    for (name, entry) in &file.engines {
        validate(name, entry, path);
    }
    Ok(Some(file.engines))
}

/// Checks one entry's own fields against its `kind`, at load time rather than at the moment the
/// missing field is first needed -- a tournament that would fail on its first move because an entry
/// names no binary should fail before any engine is started, and a run that has already played games
/// should never be able to discover a config error.
fn validate(name: &str, entry: &EngineEntry, registry_path: &str) {
    match entry.kind {
        EngineKind::Uci => assert!(
            entry.path.is_some(),
            "engine {name:?} in {registry_path} has kind: uci and needs a path"
        ),
        EngineKind::Network => assert!(
            entry.net.is_some(),
            "engine {name:?} in {registry_path} has kind: network and needs a net"
        ),
        // Nothing to require: the PeSTO evaluation is compiled in, with no file behind it.
        EngineKind::Builtin => {}
    }
}

/// Formats a YAML option value as the plain text `setoption ... value <text>` expects --
/// integers as plain decimal, booleans as the literal `true`/`false` UCI's own `check`-type
/// options require, strings as-is. UCI itself has no notion of an option's YAML type: every
/// value is sent as text regardless, so this is the one place that distinction actually matters.
pub fn format_option_value(value: &serde_yaml::Value) -> String {
    match value {
        serde_yaml::Value::Bool(value) => value.to_string(),
        serde_yaml::Value::Number(value) => value.to_string(),
        serde_yaml::Value::String(value) => value.clone(),
        other => panic!("unsupported engine option value in data/chess/engines.yaml: {other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(yaml: &str) -> HashMap<String, EngineEntry> {
        let file: EngineRegistryFile = serde_yaml::from_str(yaml).expect("valid yaml");
        file.engines
    }

    /// The compatibility property the default exists for: an entry written before `kind` existed
    /// still reads as a UCI engine with its path intact.
    #[test]
    fn an_entry_with_no_kind_is_a_uci_engine() {
        let entries = parse("engines:\n  stockfish:\n    path: stockfish\n");
        let entry = &entries["stockfish"];
        assert_eq!(entry.kind, EngineKind::Uci);
        assert_eq!(entry.binary_path(), "stockfish");
        validate("stockfish", entry, "test");
    }

    #[test]
    fn a_builtin_entry_needs_neither_a_path_nor_a_net() {
        let entries = parse("engines:\n  pesto:\n    kind: builtin\n");
        let entry = &entries["pesto"];
        assert_eq!(entry.kind, EngineKind::Builtin);
        assert!(entry.path.is_none());
        validate("pesto", entry, "test");
    }

    #[test]
    fn a_network_entry_carries_the_nnue_file() {
        let entries = parse("engines:\n  amanda-e5:\n    kind: network\n    net: data/chess/networks/a.nnue\n");
        let entry = &entries["amanda-e5"];
        assert_eq!(entry.kind, EngineKind::Network);
        assert_eq!(entry.network_path(), "data/chess/networks/a.nnue");
        validate("amanda-e5", entry, "test");
    }

    #[test]
    #[should_panic(expected = "needs a path")]
    fn a_uci_entry_without_a_path_is_rejected_at_load_time() {
        let entries = parse("engines:\n  broken:\n    options:\n      Hash: 16\n");
        validate("broken", &entries["broken"], "test");
    }

    #[test]
    #[should_panic(expected = "needs a net")]
    fn a_network_entry_without_a_net_is_rejected_at_load_time() {
        let entries = parse("engines:\n  broken:\n    kind: network\n");
        validate("broken", &entries["broken"], "test");
    }
}
