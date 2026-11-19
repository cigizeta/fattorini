//! `arena rename-attempts [--apply]`: the one-off migration of 2026-11-19, when a training run became
//! an attempt. Removed once every machine holding a `data/`, and Drive, has been migrated.
//!
//! What it renames, in this machine's `data/` and in Drive's counterpart (`remote::remote_path`):
//!
//! - every network name `<config>-g<g>[-l<lambda>]-r<n>` to `-a<n>`, and every attempt name
//!   `<config>-r<n>` of a known config to `<config>-a<n>`, in the text files and in the file and
//!   directory names of `TREES`, and in `data/smithy-last.txt`;
//! - the attempt directories `data/networks/<config>/run-<n>/` to `attempt-<n>/`, the paths through
//!   them in the text files of `data/networks`, and each lambda's `run_config.txt` to
//!   `attempt_config.txt`;
//! - the player names of every `results.db` (`engine_one`, `engine_two`, `white`, `black`), each
//!   database in one transaction.
//!
//! A dry run by default, printing every change; `--apply` makes them. Idempotent: a second pass finds
//! nothing. Run with the smithy and every arena stopped. lichess-bot's config, outside the repository,
//! is left to the developer: the renamed snapshots are printed.

use std::path::Path;

use fattorini::pipeline::{players, remote};
use rusqlite::Connection;

/// The trees under `data/` whose text and names may hold a network's or an attempt's name.
const TREES: &[&str] = &["data/networks", "data/trials", "data/matches", "data/tournaments", "data/snapshots", "data/books", "data/spsa"];

/// The one file outside `TREES`: the smithy's last job, which may name a tune's player.
const LAST_JOB: &str = "data/smithy-last.txt";

/// The text files rewritten, by extension; every other file keeps its contents.
const TEXT_EXTENSIONS: &[&str] = &["txt", "tsv", "pgn", "yaml", "yml", "json", "log", "csv"];

/// `arena rename-attempts [--apply]`.
pub fn command(args: &[String]) {
    let apply = match args {
        [] => false,
        [flag] if flag == "--apply" => true,
        _ => {
            eprintln!("usage: arena rename-attempts [--apply]: a dry run without --apply");
            std::process::exit(2);
        }
    };
    let configs = known_configs();
    let mut changes = Changes { apply, count: 0, snapshots: Vec::new() };
    for tree in TREES {
        for root in [tree.to_string(), remote::remote_path(tree)] {
            if Path::new(&root).is_dir() {
                migrate_tree(&root, tree == &players::ATTEMPTS_ROOT, &configs, &mut changes);
            }
        }
    }
    if Path::new(LAST_JOB).is_file() {
        changes.rewrite_file(LAST_JOB, false, &configs);
    }
    let verb = if apply { "made" } else { "to make, a dry run: --apply makes them" };
    println!("rename-attempts: {} changes {verb}", changes.count);
    for (old, new) in &changes.snapshots {
        println!("snapshot renamed: {old} -> {new}: point lichess-bot's config at the new name");
    }
}

/// Every change, printed, and made only when `apply`.
struct Changes {
    apply: bool,
    count: usize,
    /// Each snapshot executable renamed, old and new file names, for lichess-bot.
    snapshots: Vec<(String, String)>,
}

impl Changes {
    /// Rewrites the text file at `path`, its network and attempt names, and with `paths` the paths
    /// through attempt directories.
    fn rewrite_file(&mut self, path: &str, paths: bool, configs: &[String]) {
        let Ok(text) = std::fs::read_to_string(path) else {
            return; // Not UTF-8, or unreadable: nothing of ours in it.
        };
        let new = rewrite(&text, paths, configs);
        if new == text {
            return;
        }
        self.count += 1;
        println!("rewrite {path}");
        if self.apply {
            let partial = format!("{path}.partial");
            std::fs::write(&partial, &new).and_then(|()| std::fs::rename(&partial, path))
                .unwrap_or_else(|error| panic!("failed to rewrite {path}: {error}"));
        }
    }

    /// Renames `from` to `to`, refusing to overwrite.
    fn rename(&mut self, from: &str, to: &str) {
        if Path::new(to).exists() {
            println!("WARNING: {from} not renamed: {to} exists already");
            return;
        }
        self.count += 1;
        println!("rename {from} -> {to}");
        if from.contains("snapshots") && from.ends_with(".exe") {
            let name = |path: &str| Path::new(path).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            self.snapshots.push((name(from), name(to)));
        }
        if self.apply {
            std::fs::rename(from, to).unwrap_or_else(|error| panic!("failed to rename {from} to {to}: {error}"));
        }
    }

    /// Renames the players of the `results.db` at `path`, in one transaction.
    fn rewrite_results(&mut self, path: &str, configs: &[String]) {
        let mut connection = Connection::open(path).unwrap_or_else(|error| panic!("failed to open {path}: {error}"));
        let renames = results_renames(&connection, configs).unwrap_or_else(|error| panic!("{path}: {error}"));
        if renames.is_empty() {
            return;
        }
        for (column, old, new) in &renames {
            println!("results {path}: {column} {old} -> {new}");
        }
        self.count += renames.len();
        if self.apply {
            let transaction = connection.transaction().unwrap_or_else(|error| panic!("{path}: {error}"));
            for (column, old, new) in &renames {
                transaction
                    .execute(&format!("UPDATE games SET {column} = ?1 WHERE {column} = ?2"), [new, old])
                    .unwrap_or_else(|error| panic!("{path}: {error}"));
            }
            transaction.commit().unwrap_or_else(|error| panic!("{path}: {error}"));
        }
    }
}

/// The player columns of `results.db`.
const RESULT_COLUMNS: &[&str] = &["engine_one", "engine_two", "white", "black"];

/// Each player name of `connection`'s games to rename: its column, old and new.
fn results_renames(connection: &Connection, configs: &[String]) -> Result<Vec<(&'static str, String, String)>, String> {
    let mut renames = Vec::new();
    for &column in RESULT_COLUMNS {
        let mut statement = connection.prepare(&format!("SELECT DISTINCT {column} FROM games")).map_err(|error| error.to_string())?;
        let names: Vec<String> =
            statement.query_map([], |row| row.get(0)).map_err(|error| error.to_string())?.collect::<Result<_, _>>().map_err(|error| error.to_string())?;
        for old in names {
            let new = rewrite(&old, false, configs);
            if new != old {
                renames.push((column, old, new));
            }
        }
    }
    Ok(renames)
}

/// Every entry under `root`, files first rewritten, then every name renamed, the deepest first so
/// each path is still valid when its turn comes. `networks`: the attempts' tree, whose `run-<n>`
/// directories, `run_config.txt` files and paths through them are renamed too.
fn migrate_tree(root: &str, networks: bool, configs: &[String], changes: &mut Changes) {
    let mut entries = Vec::new();
    walk(Path::new(root), &mut entries);
    for path in entries.iter().filter(|path| path.is_file()) {
        let text = path.to_string_lossy();
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        if name == "results.db" {
            changes.rewrite_results(&text, configs);
        } else if path.extension().is_some_and(|extension| TEXT_EXTENSIONS.contains(&extension.to_string_lossy().as_ref())) {
            changes.rewrite_file(&text, networks, configs);
        }
    }
    entries.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for path in entries {
        let Some(name) = path.file_name().map(|name| name.to_string_lossy().into_owned()) else { continue };
        let new = renamed_entry(&name, networks, configs);
        if new != name {
            let to = path.with_file_name(&new);
            changes.rename(&path.to_string_lossy(), &to.to_string_lossy());
        }
    }
}

/// Every file and directory under `dir`, recursively, `dir` itself excluded.
fn walk(dir: &Path, into: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, into);
        }
        into.push(path);
    }
}

/// A file or directory's new name: its names rewritten, and in the attempts' tree `run-<n>` and
/// `run_config.txt` as they are now called.
fn renamed_entry(name: &str, networks: bool, configs: &[String]) -> String {
    if networks {
        if name == "run_config.txt" {
            return "attempt_config.txt".to_string();
        }
        if let Some(digits) = name.strip_prefix("run-").filter(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())) {
            return format!("attempt-{digits}");
        }
    }
    rewrite(name, false, configs)
}

/// The config names an attempt name may start with: those with attempts here or on Drive, and every
/// config file's, archived ones included. Longest first, so a config is never read as a shorter one's
/// prefix.
fn known_configs() -> Vec<String> {
    let mut configs = Vec::new();
    for dir in [players::ATTEMPTS_ROOT.to_string(), remote::remote_path(players::ATTEMPTS_ROOT)] {
        if let Ok(entries) = std::fs::read_dir(dir) {
            configs.extend(entries.flatten().filter(|entry| entry.path().is_dir()).filter_map(|entry| entry.file_name().into_string().ok()));
        }
    }
    let mut yaml = Vec::new();
    for dir in ["configs", "archived/configs"] {
        walk(Path::new(dir), &mut yaml);
    }
    configs.extend(
        yaml.iter()
            .filter(|path| path.extension().is_some_and(|extension| extension == "yaml"))
            .filter_map(|path| path.file_stem().map(|stem| stem.to_string_lossy().into_owned())),
    );
    configs.sort_by_key(|config| std::cmp::Reverse(config.len()));
    configs.dedup();
    configs
}

/// `text` with every network name's `-r<n>` as `-a<n>`, every attempt name `<config>-r<n>` of
/// `configs` as `<config>-a<n>`, and with `paths` every path component `run-<n>` as `attempt-<n>`.
fn rewrite(text: &str, paths: bool, configs: &[String]) -> String {
    let mut text = rewrite_network_names(text);
    for config in configs {
        text = rewrite_attempt_names(&text, config);
    }
    if paths { rewrite_paths(&text) } else { text }
}

/// The end of the ASCII digits of `bytes` from `at`.
fn digits_end(bytes: &[u8], at: usize) -> usize {
    at + bytes[at.min(bytes.len())..].iter().take_while(|b| b.is_ascii_digit()).count()
}

/// Whether a name may end before `at`: the text ends, or what follows is no letter nor digit.
fn ends_at(bytes: &[u8], at: usize) -> bool {
    bytes.get(at).is_none_or(|b| !b.is_ascii_alphanumeric())
}

/// `-g<g>[-l<lambda>]-r<n>` as `-g<g>[-l<lambda>]-a<n>`, wherever it stands.
fn rewrite_network_names(text: &str) -> String {
    let bytes = text.as_bytes();
    let (mut out, mut copied, mut from) = (String::with_capacity(text.len()), 0, 0);
    while let Some(offset) = text[from..].find("-g") {
        let start = from + offset;
        from = start + 2;
        let mut at = digits_end(bytes, start + 2);
        if at == start + 2 {
            continue;
        }
        if text[at..].starts_with("-l") {
            let lambda = at + 2;
            let end = if text[lambda..].starts_with("polyline") {
                lambda + "polyline".len()
            } else {
                lambda + bytes[lambda..].iter().take_while(|b| b.is_ascii_digit() || **b == b'.').count()
            };
            if end > lambda {
                at = end;
            }
        }
        if text[at..].starts_with("-r") {
            let end = digits_end(bytes, at + 2);
            if end > at + 2 && ends_at(bytes, end) {
                out.push_str(&text[copied..at]);
                out.push_str("-a");
                out.push_str(&text[at + 2..end]);
                copied = end;
                from = end;
            }
        }
    }
    out.push_str(&text[copied..]);
    out
}

/// `<config>-r<n>` as `<config>-a<n>`, wherever it stands.
fn rewrite_attempt_names(text: &str, config: &str) -> String {
    let marker = format!("{config}-r");
    let bytes = text.as_bytes();
    let (mut out, mut copied, mut from) = (String::with_capacity(text.len()), 0, 0);
    while let Some(offset) = text[from..].find(&marker) {
        let at = from + offset + marker.len();
        from = at;
        let end = digits_end(bytes, at);
        if end > at && ends_at(bytes, end) {
            out.push_str(&text[copied..at - 1]);
            out.push('a');
            out.push_str(&text[at..end]);
            copied = end;
            from = end;
        }
    }
    out.push_str(&text[copied..]);
    out
}

/// A path component `run-<n>`, after a `/` or a `\`, as `attempt-<n>`.
fn rewrite_paths(text: &str) -> String {
    let bytes = text.as_bytes();
    let (mut out, mut copied, mut from) = (String::with_capacity(text.len()), 0, 0);
    while let Some(offset) = text[from..].find("run-") {
        let start = from + offset;
        from = start + 4;
        let after_separator = start > 0 && matches!(bytes[start - 1], b'/' | b'\\');
        let end = digits_end(bytes, start + 4);
        if after_separator && end > start + 4 && ends_at(bytes, end) {
            out.push_str(&text[copied..start]);
            out.push_str("attempt-");
            out.push_str(&text[start + 4..end]);
            copied = end;
            from = end;
        }
    }
    out.push_str(&text[copied..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configs() -> Vec<String> {
        vec!["lc0-psq-threats-screlu".to_string(), "lc0-psq".to_string()]
    }

    #[test]
    fn network_and_attempt_names_are_rewritten_and_nothing_else() {
        let configs = configs();
        let cases = [
            ("lc0-psq-threats-screlu-g1-r1@search-tuned", "lc0-psq-threats-screlu-g1-a1@search-tuned"),
            ("fine-g12-l0.05-r3 vs fine-g2-lpolyline-r10", "fine-g12-l0.05-a3 vs fine-g2-lpolyline-a10"),
            ("fattorini-lc0-psq-g1-r1-untuned-20261105-0900.exe", "fattorini-lc0-psq-g1-a1-untuned-20261105-0900.exe"),
            ("scores-lc0-psq-g1-r2-search-tuned-d20.txt", "scores-lc0-psq-g1-a2-search-tuned-d20.txt"),
            ("job lc0-psq-threats-screlu-r2 started", "job lc0-psq-threats-screlu-a2 started"),
            ("lc0-psq-r1", "lc0-psq-a1"),
            // Not ours: Lc0's run, a config not known, a name already migrated, a longer word.
            ("training-run2-test91-", "training-run2-test91-"),
            ("other-r1", "other-r1"),
            ("lc0-psq-g1-a1", "lc0-psq-g1-a1"),
            ("x-g1-r1x", "x-g1-r1x"),
        ];
        for (old, new) in cases {
            assert_eq!(rewrite(old, false, &configs), new, "{old:?}");
            assert_eq!(rewrite(new, false, &configs), new, "idempotent: {new:?}");
        }
    }

    #[test]
    fn paths_through_attempt_directories_are_rewritten_only_when_asked() {
        let text = "data/networks/lc0-psq/run-1/gen-1/lambda-0.05 and G:\\x\\run-12\\y, a run-1 alone";
        assert_eq!(
            rewrite(text, true, &[]),
            "data/networks/lc0-psq/attempt-1/gen-1/lambda-0.05 and G:\\x\\attempt-12\\y, a run-1 alone"
        );
        assert_eq!(rewrite(text, false, &[]), text);
    }

    #[test]
    fn entries_of_the_attempts_tree_are_renamed() {
        let configs = configs();
        assert_eq!(renamed_entry("run-3", true, &configs), "attempt-3");
        assert_eq!(renamed_entry("run_config.txt", true, &configs), "attempt_config.txt");
        assert_eq!(renamed_entry("run-3", false, &configs), "run-3", "only in the attempts' tree");
        assert_eq!(renamed_entry("run-x", true, &configs), "run-x");
        assert_eq!(renamed_entry("lc0-psq-g1-r1-vs-pesto", false, &configs), "lc0-psq-g1-a1-vs-pesto");
    }

    #[test]
    fn a_tree_and_a_results_database_are_migrated_and_a_second_pass_finds_nothing() {
        let root = std::env::temp_dir().join(format!("arena-rename-attempts-{}", std::process::id()));
        let root = root.to_str().unwrap().to_string();
        std::fs::remove_dir_all(&root).ok();
        let lambda = format!("{root}/networks/lc0-psq/run-1/gen-1/lambda-0.05");
        std::fs::create_dir_all(&lambda).unwrap();
        std::fs::write(format!("{lambda}/run_config.txt"), "parent\tlc0\n").unwrap();
        std::fs::write(format!("{root}/networks/lc0-psq/run-1/chores.txt"), "data/networks/lc0-psq/run-1/gen-1 lc0-psq-g1-r1\n").unwrap();
        let matches = format!("{root}/matches/lc0-psq-g1-r1-vs-pesto/20261110-0859");
        std::fs::create_dir_all(&matches).unwrap();
        {
            let connection = crate::results_db::open(&format!("{matches}/results.db"));
            connection
                .execute(
                    "INSERT INTO games (round, engine_one, engine_two, game_index, white, black, result, termination, ply_count) \
                     VALUES (1, 'lc0-psq-g1-r1@untuned', 'pesto@untuned', 0, 'lc0-psq-g1-r1@untuned', 'pesto@untuned', 1.0, 'mate', 80)",
                    [],
                )
                .unwrap();
        }
        let configs = configs();
        let pass = |apply: bool| {
            let mut changes = Changes { apply, count: 0, snapshots: Vec::new() };
            migrate_tree(&format!("{root}/networks"), true, &configs, &mut changes);
            migrate_tree(&format!("{root}/matches"), false, &configs, &mut changes);
            changes.count
        };
        assert!(pass(false) > 0);
        assert!(Path::new(&format!("{lambda}/run_config.txt")).exists(), "a dry run changes nothing");
        assert!(pass(true) > 0);
        let attempt = format!("{root}/networks/lc0-psq/attempt-1");
        assert!(Path::new(&format!("{attempt}/gen-1/lambda-0.05/attempt_config.txt")).is_file());
        assert_eq!(std::fs::read_to_string(format!("{attempt}/chores.txt")).unwrap(), "data/networks/lc0-psq/attempt-1/gen-1 lc0-psq-g1-a1\n");
        let database = format!("{root}/matches/lc0-psq-g1-a1-vs-pesto/20261110-0859/results.db");
        let connection = Connection::open(&database).unwrap();
        let white: String = connection.query_row("SELECT white FROM games", [], |row| row.get(0)).unwrap();
        assert_eq!(white, "lc0-psq-g1-a1@untuned");
        drop(connection);
        assert_eq!(pass(false), 0, "a second pass finds nothing");
        std::fs::remove_dir_all(&root).ok();
    }
}
