//! An install, asked by the smithy (`protocol::INSTALL_PATH`): a fresh release `temper-chess` and the
//! network it plays, put where lichess-bot runs them, the data root's `bin/`, with the opening book
//! beside them. Run on a thread of the arena's own (`chores.rs`), its workers playing on: their games
//! run in-process, so no `temper-chess.exe` of theirs is replaced.
//!
//! The names in `bin/` never change (`temper-chess.exe`, `temper-chess.nnue`, and the opening book `temper-chess.bin`):
//! the engine loads the `temper-chess.nnue` beside its own binary at startup, so no GUI needs an `EvalFile`,
//! lichess-bot's config is written once, and every install only replaces the files. `temper-chess.txt`
//! beside them says what they are; the arena's `Alive` carries it to the smithy's page. The install
//! also creates the directory lichess-bot writes its games to (`games_dir`).

use std::process;
use std::sync::atomic::AtomicBool;

use temper_chess::pipeline::protocol::{self, InstallRequest, PESTO};
use temper_chess::pipeline::{data_root, lan};

use super::chores::{store_path, wait_for, Waited, NETWORK_POLL, NETWORK_TIMEOUT};

/// A network's tuned search parameters, beside its engine file (`nnue::parameters_file_beside`).
const PARAMETERS_FILE: &str = "parameters.txt";

/// The record beside a candidate's network, which `temper-chess.txt` carries with a `candidate_` prefix.
const CANDIDATE_FILE: &str = "candidate.txt";

/// What an install does with the opening book lichess-bot plays, `temper-chess.bin` beside the engine.
#[derive(Debug, PartialEq)]
enum BookChoice {
    /// `keep`: the installed book stays, so installing a new champion never drops it.
    Keep,
    /// The Polyglot book at that path in the store, as the smithy's book job leaves it there
    /// (`data/chess/books/<source>/book-n<N>-<weights>.bin`), its record the `.txt` beside it.
    Install(String),
    /// `none`.
    Remove,
}

/// An install the arena can run: the network's path in the store, `None` for the PeSTO evaluation,
/// and the book.
#[derive(Debug, PartialEq)]
pub(crate) struct Install {
    network: Option<String>,
    book: BookChoice,
}

impl Install {
    /// `request`'s network and book, checked: a network is `pesto` or a `.nnue` in the store, a book
    /// `keep`, `none` or a `.bin` in the store.
    pub(crate) fn of(request: &InstallRequest) -> Result<Install, String> {
        let network = match request.network.as_str() {
            PESTO => None,
            path if path.ends_with(".nnue") => Some(in_store(path)?),
            other => return Err(format!("the network is pesto or a .nnue in the store, got {other:?}")),
        };
        let book = match request.book.as_str() {
            "keep" => BookChoice::Keep,
            "none" => BookChoice::Remove,
            path if path.ends_with(".bin") => BookChoice::Install(in_store(path)?),
            other => return Err(format!("the book is keep, none or a .bin in the store, got {other:?}")),
        };
        Ok(Install { network, book })
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
    let book_path = match &install.book {
        BookChoice::Install(path) => {
            let full = store_path(store, path)?;
            if !std::path::Path::new(&full).is_file() {
                return Err(format!("no opening book at {path} in {store}: nothing was installed"));
            }
            Some(full)
        }
        _ => None,
    };

    build_engine()?;

    let bin_dir = format!("{}/bin", data_root::data_root());
    create_dir(&bin_dir)?;
    create_dir(&games_dir())?;
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
    let book = install_book(&bin_dir, &install.book, book_path.as_deref())?;
    // Last, so it only ever describes files already in place.
    write_record(&format!("{bin_dir}/temper-chess.txt"), &with_book_fields(fields, book))?;
    Ok(format!("installed {} in {bin_dir}/", install.name()))
}

/// The `temper-chess.txt` of the engine installed on this machine, field by field; none before a first
/// install.
pub(crate) fn installed() -> Vec<(String, String)> {
    let text = std::fs::read_to_string(format!("{}/bin/temper-chess.txt", data_root::data_root())).unwrap_or_default();
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

/// Applies `book` to `temper-chess.bin` beside the engine, where lichess-bot's `polyglot:` section reads
/// it, and returns `temper-chess.txt`'s fields for it. Called before `temper-chess.txt` is rewritten, so `Keep`
/// can carry the previous one's book lines. `book_path` is the book's file, checked present.
fn install_book(bin_dir: &str, book: &BookChoice, book_path: Option<&str>) -> Result<Vec<(String, String)>, String> {
    let destination = format!("{bin_dir}/temper-chess.bin");
    match (book, book_path) {
        (BookChoice::Install(path), Some(book_path)) => {
            install_file(book_path, &destination)?;
            let record_path = format!("{}.txt", book_path.strip_suffix(".bin").expect("checked by Install::of"));
            let record = std::fs::read_to_string(record_path).unwrap_or_default();
            Ok(installed_book_fields(path, &record))
        }
        (BookChoice::Remove, _) => {
            remove_if_present(&destination)?;
            Ok(vec![("book".to_string(), "none".to_string())])
        }
        _ => {
            let previous = std::fs::read_to_string(format!("{bin_dir}/temper-chess.txt")).unwrap_or_default();
            Ok(kept_book_fields(&previous, std::path::Path::new(&destination).is_file()))
        }
    }
}

/// `temper-chess.txt`'s book fields for a book just installed from `path`: `book` names it, and its record
/// follows field by field with a `book_` prefix.
fn installed_book_fields(path: &str, record: &str) -> Vec<(String, String)> {
    let mut fields = vec![("book".to_string(), path.to_string())];
    for line in record.lines().filter(|line| !line.is_empty()) {
        let (field, value) = line.split_once('\t').unwrap_or((line, ""));
        fields.push((format!("book_{field}"), value.to_string()));
    }
    fields
}

/// `temper-chess.txt`'s book fields when the installed book stays: the previous `temper-chess.txt`'s `book` and
/// `book_` lines. `none` when no `temper-chess.bin` is there, whatever the old lines said; `unknown` when
/// one is there but no line describes it.
fn kept_book_fields(previous: &str, book_present: bool) -> Vec<(String, String)> {
    if !book_present {
        return vec![("book".to_string(), "none".to_string())];
    }
    let fields: Vec<(String, String)> = previous
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .filter(|(field, _)| *field == "book" || field.starts_with("book_"))
        .map(|(field, value)| (field.to_string(), value.to_string()))
        .collect();
    if fields.first().is_none_or(|(field, _)| field != "book") {
        return vec![("book".to_string(), "unknown".to_string())];
    }
    fields
}

/// `fields` with `book` inserted before `commit`, so `temper-chess.txt` lists what was installed before
/// when and from which commit.
fn with_book_fields(fields: Vec<(String, String)>, book: Vec<(String, String)>) -> Vec<(String, String)> {
    let at = fields.iter().position(|(field, _)| field == "commit").unwrap_or(fields.len());
    let mut result = fields;
    result.splice(at..at, book);
    result
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

/// Where lichess-bot keeps the PGN of its games (`pgn_directory` in its `config.yml`), created by
/// every install, so lichess-bot never needs a directory made by hand.
fn games_dir() -> String {
    format!("{}/lichess/games", data_root::data_root())
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

    fn request(network: &str, book: &str) -> InstallRequest {
        InstallRequest { boot: "b00t".to_string(), network: network.to_string(), book: book.to_string() }
    }

    #[test]
    fn an_install_takes_pesto_or_a_network_in_the_store_and_a_book_kept_none_or_in_the_store() {
        let network = "data/chess/networks/fine/run-1/gen-2/champion.nnue";
        let book = "data/chess/books/elite/book-n9-points.bin";
        assert_eq!(Install::of(&request(PESTO, "keep")), Ok(Install { network: None, book: BookChoice::Keep }));
        assert_eq!(
            Install::of(&request(network, book)),
            Ok(Install { network: Some(network.to_string()), book: BookChoice::Install(book.to_string()) })
        );
        assert_eq!(Install::of(&request(network, "none")).unwrap().book, BookChoice::Remove);
        assert_eq!(Install::of(&request(network, "none")).unwrap().name(), network);
        for (network, book) in [
            ("betty-g1-r1", "keep"),
            ("C:/elsewhere/champion.nnue", "keep"),
            ("data/../champion.nnue", "keep"),
            (PESTO, "elite.txt"),
            (PESTO, "data/../book.bin"),
            (PESTO, ""),
        ] {
            assert!(Install::of(&request(network, book)).is_err(), "{network} {book}");
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

    #[test]
    fn temper_chess_txt_names_the_book_before_the_commit_with_its_record_under_a_prefix() {
        let fields = with_book_fields(
            install_record("w.nnue", "", "abc123", "now"),
            installed_book_fields("data/chess/books/elite/book-n9-points.bin", "weights\tpoints\nmoves\t42\n"),
        );
        let actual: Vec<(&str, &str)> = fields.iter().map(|(field, value)| (field.as_str(), value.as_str())).collect();
        assert_eq!(
            actual,
            [
                ("network", "w.nnue"),
                ("book", "data/chess/books/elite/book-n9-points.bin"),
                ("book_weights", "points"),
                ("book_moves", "42"),
                ("commit", "abc123"),
                ("installed", "now"),
            ]
        );
    }

    #[test]
    fn a_kept_book_carries_its_previous_lines_or_says_what_is_known() {
        let previous = "network\tbetty\nbook\tdata/chess/books/elite/book-n9-points.bin\nbook_moves\t42\ncommit\tabc\n";
        let fields = kept_book_fields(previous, true);
        let actual: Vec<(&str, &str)> = fields.iter().map(|(field, value)| (field.as_str(), value.as_str())).collect();
        assert_eq!(actual, [("book", "data/chess/books/elite/book-n9-points.bin"), ("book_moves", "42")]);
        assert_eq!(kept_book_fields(previous, false), [("book".to_string(), "none".to_string())], "no temper-chess.bin there");
        assert_eq!(kept_book_fields("network\tbetty\n", true), [("book".to_string(), "unknown".to_string())]);
        assert_eq!(kept_book_fields("", true), [("book".to_string(), "unknown".to_string())], "a first install");
    }
}
