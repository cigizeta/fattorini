//! The remote copy of `data/`, on Google Drive, reached as an ordinary folder.
//!
//! The remote holds Lc0's converted games and their frozen test set, the opening books, and the
//! server files clients find their servers by, so every machine reads the same tree.
//!
//! Google Drive for desktop, Google's own Windows client in Stream mode, presents the Drive as the
//! `G:` drive and uploads and downloads in the background. Fattorini only copies files into and out of
//! that folder: no Drive API, no OAuth app, no child process. Stream mode keeps only a cache locally, so the Drive
//! can hold far more than the local disk.
//!
//! The remote mirrors the local layout: `data/X` locally is `{ROOT}/data/X` remotely, so nothing
//! about remote paths is configured or typed. Files on the remote are written once and never
//! modified, except the opening books' files (`arena book`'s fit, graph and books), which each run
//! replaces whole, and the small files written straight to it (`write_small`). Every file is written
//! under a `.partial` name and renamed when complete, so no machine ever reads a half-written one.
//!
//! Every function returns a `Result` rather than panicking: an unmounted drive or a failing copy is
//! an ordinary event for a generator left running unattended, and the caller decides whether to warn
//! and carry on. Each has a variant taking the root, so the tests run against a temporary directory.

use std::fs;
use std::io::Write;
use std::path::Path;

/// The root of the remote mirror: `fattorini/` in My Drive, on `G:`. Forward slashes: every Windows
/// file API accepts them, and the paths below are joined with `/`.
pub const ROOT: &str = "G:/My Drive/fattorini";

/// This process's remote root when it is not `ROOT`: a local folder standing in for Drive. Set once,
/// before any remote call.
static ROOT_INSTEAD: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Makes `root` this process's remote root instead of `ROOT`. Once only, before any remote call: a
/// second call is refused, so no file of one root is ever read as another's.
pub fn set_root(root: &str) -> Result<(), String> {
    ROOT_INSTEAD.set(root.to_string()).map_err(|_| format!("the remote root is set already, to {}", this_root()))
}

/// This process's remote root: `ROOT`, unless `set_root` gave another.
pub fn this_root() -> &'static str {
    ROOT_INSTEAD.get().map_or(ROOT, String::as_str)
}

/// What a machine whose drive is missing must do, appended to `check_mounted`'s message.
const MOUNT_HINT: &str = "Google Drive for desktop must be running with My Drive on G:";

/// The suffix of a file still being copied into place. Never matches a filter below, so a
/// half-copied file is invisible to every listing and every copy.
const PARTIAL_SUFFIX: &str = ".partial";

#[derive(Debug, PartialEq)]
pub enum RemoteError {
    /// The remote folder's drive is not mounted: a setup error on this machine, not an outage.
    NotMounted(String),
    /// A copy, read or listing failed, with the reason.
    Failed(String),
}

impl std::fmt::Display for RemoteError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            RemoteError::NotMounted(message) | RemoteError::Failed(message) => write!(formatter, "{message}"),
        }
    }
}

/// A finished segment and the file written beside it, its `.meta` (`segments`). Never a `.tmp`, a
/// staging `{stem}.pgn`, a batch's private `{stem}.count` or a `.partial`.
pub fn is_segment_file(name: &str) -> bool {
    [".pgn.zst", ".pgn.zst.meta"].iter().any(|suffix| name.ends_with(suffix))
}

/// The names of the directories directly in `local_dir`'s remote counterpart that `accept` takes,
/// sorted. Empty when the remote directory does not exist yet.
pub fn list_dirs(local_dir: &str, accept: &dyn Fn(&str) -> bool) -> Result<Vec<String>, RemoteError> {
    let dir = remote_path(local_dir);
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(RemoteError::Failed(format!("failed to list {dir}: {error}"))),
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| RemoteError::Failed(format!("failed to list {dir}: {error}")))?;
        let is_dir = entry.file_type().map_err(|error| RemoteError::Failed(format!("failed to list {dir}: {error}")))?.is_dir();
        let Ok(name) = entry.file_name().into_string() else { continue };
        if is_dir && accept(&name) {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

/// The remote counterpart of a local path under `data/`: `data/lc0/test` becomes
/// `{ROOT}/data/lc0/test`.
pub fn remote_path(local: &str) -> String {
    remote_path_under(this_root(), local)
}

fn remote_path_under(root: &str, local: &str) -> String {
    assert!(
        local == "data" || local.starts_with("data/"),
        "{local} is not under data/, so it has no remote counterpart"
    );
    format!("{root}/{}", local.trim_end_matches('/'))
}

/// Whether the remote folder can be reached at all, checked once at startup so a machine whose drive
/// is not mounted fails immediately with a clear message instead of warning at every sync. The drive
/// is there when `My Drive` is: `ROOT` itself is created by the first upload.
pub fn check_mounted() -> Result<(), RemoteError> {
    check_mounted_under(this_root())
}

/// `check_mounted` against `root`, which the tests give as a temporary directory.
pub fn check_mounted_under(root: &str) -> Result<(), RemoteError> {
    let drive = Path::new(root).parent().expect("the remote root has a parent");
    if drive.is_dir() {
        return Ok(());
    }
    Err(RemoteError::NotMounted(format!("{} is not there: {MOUNT_HINT}", drive.display())))
}

/// Uploads every file directly in `local_dir` that `accept` takes and the remote lacks. A file already
/// on the remote with the same size is skipped, and one with a different size is an error rather
/// than an overwrite. Sizes, not contents: comparing contents would download every remote file
/// again, and a differing file of the same name and size would take a broken invariant elsewhere.
pub fn copy_up(local_dir: &str, accept: &dyn Fn(&str) -> bool) -> Result<(), RemoteError> {
    copy_files(local_dir, &remote_path(local_dir), accept)
}

/// Downloads every file directly in `local_dir`'s remote counterpart that `accept` takes into
/// `target_dir`: a staging directory the caller then renames into place, or the local directory
/// itself for files that are complete one by one. The same skip and refusal rules as `copy_up`.
pub fn copy_down(local_dir: &str, target_dir: &str, accept: &dyn Fn(&str) -> bool) -> Result<(), RemoteError> {
    copy_files(&remote_path(local_dir), target_dir, accept)
}

/// The contents of the remote counterpart of `local_file`.
pub fn cat(local_file: &str) -> Result<String, RemoteError> {
    let path = remote_path(local_file);
    fs::read_to_string(&path).map_err(|error| RemoteError::Failed(format!("failed to read {path}: {error}")))
}

/// The names of the files directly in `local_dir`'s remote counterpart that `accept` takes, sorted.
/// Empty when the remote directory does not exist yet.
pub fn list(local_dir: &str, accept: &dyn Fn(&str) -> bool) -> Result<Vec<String>, RemoteError> {
    list_files(&remote_path(local_dir), accept)
}

/// Replaces the remote counterpart of `local_file` with `text`, via `.partial` and a rename. For the
/// small files written straight to the remote, as `replay`'s claims.
pub fn write_small(local_file: &str, text: &str) -> Result<(), RemoteError> {
    let path = remote_path(local_file);
    if let Some(parent) = Path::new(&path).parent() {
        fs::create_dir_all(parent)
            .map_err(|error| RemoteError::Failed(format!("failed to create {}: {error}", parent.display())))?;
    }
    let partial = format!("{path}{PARTIAL_SUFFIX}");
    let mut file = fs::File::create(&partial).map_err(|error| RemoteError::Failed(format!("failed to create {partial}: {error}")))?;
    file.write_all(text.as_bytes()).map_err(|error| RemoteError::Failed(format!("failed to write {partial}: {error}")))?;
    drop(file);
    fs::rename(&partial, &path).map_err(|error| RemoteError::Failed(format!("failed to rename {partial} to {path}: {error}")))?;
    Ok(())
}

/// Replaces the remote counterpart of `local_file` with a copy of it, via `.partial` and a rename, and
/// then removes `local_file`: a move. For a file rebuilt in full and replaced as a whole, the opening
/// book, which `copy_up` would refuse to overwrite. The local file is removed only once the remote one
/// is in place, so a failure leaves it where it was.
pub fn move_up_replacing(local_file: &str) -> Result<(), RemoteError> {
    move_replacing(local_file, &remote_path(local_file))
}

fn move_replacing(source: &str, target: &str) -> Result<(), RemoteError> {
    copy_replacing(source, target)?;
    fs::remove_file(source).map_err(|error| RemoteError::Failed(format!("failed to remove {source}: {error}")))?;
    Ok(())
}

/// Copies `source` over `target`, creating `target`'s directory, via `{target}.partial` and a rename,
/// so no reader of `target` ever sees a half-copied file.
pub fn copy_replacing(source: &str, target: &str) -> Result<(), RemoteError> {
    if let Some(parent) = Path::new(target).parent() {
        fs::create_dir_all(parent)
            .map_err(|error| RemoteError::Failed(format!("failed to create {}: {error}", parent.display())))?;
    }
    let partial = format!("{target}{PARTIAL_SUFFIX}");
    copy_bytes(source, &partial)?;
    fs::rename(&partial, target).map_err(|error| RemoteError::Failed(format!("failed to rename {partial} to {target}: {error}")))?;
    Ok(())
}

/// The names of the files directly in `dir` that `accept` takes, sorted. Empty when `dir` does not
/// exist. A `.partial` is never taken, whatever `accept` says.
fn list_files(dir: &str, accept: &dyn Fn(&str) -> bool) -> Result<Vec<String>, RemoteError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(RemoteError::Failed(format!("failed to list {dir}: {error}"))),
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| RemoteError::Failed(format!("failed to list {dir}: {error}")))?;
        let is_file = entry.file_type().map_err(|error| RemoteError::Failed(format!("failed to list {dir}: {error}")))?.is_file();
        let Ok(name) = entry.file_name().into_string() else { continue };
        if is_file && !name.ends_with(PARTIAL_SUFFIX) && accept(&name) {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

/// Copies every file directly in `source_dir` that `accept` takes into `target_dir`, creating it, in
/// name order. Each file goes to `{name}.partial` first and is renamed into place when complete. A
/// target already there with the source's size is skipped; one with another size is an error, and
/// nothing is overwritten. Public for callers that are handed both directories, so their tests can
/// use temporary ones; `copy_up` and `copy_down` derive the remote side themselves.
pub fn copy_files(source_dir: &str, target_dir: &str, accept: &dyn Fn(&str) -> bool) -> Result<(), RemoteError> {
    let names = list_files(source_dir, accept)?;
    if names.is_empty() {
        return Ok(());
    }
    fs::create_dir_all(target_dir).map_err(|error| RemoteError::Failed(format!("failed to create {target_dir}: {error}")))?;
    for name in names {
        let source = format!("{source_dir}/{name}");
        let target = format!("{target_dir}/{name}");
        let source_size = file_size(&source)?;
        if Path::new(&target).exists() {
            let target_size = file_size(&target)?;
            if target_size == source_size {
                continue;
            }
            return Err(RemoteError::Failed(format!(
                "{target} already exists with {target_size} bytes, but {source} has {source_size}: refusing to overwrite it"
            )));
        }
        let partial = format!("{target}{PARTIAL_SUFFIX}");
        copy_bytes(&source, &partial)?;
        fs::rename(&partial, &target).map_err(|error| RemoteError::Failed(format!("failed to rename {partial} to {target}: {error}")))?;
    }
    Ok(())
}

/// Copies `source`'s bytes into a new `target`, and nothing else: not `fs::copy`, which also copies
/// the file's attributes, which a Drive folder need not accept.
fn copy_bytes(source: &str, target: &str) -> Result<(), RemoteError> {
    let mut input = fs::File::open(source).map_err(|error| RemoteError::Failed(format!("failed to open {source}: {error}")))?;
    let mut output = fs::File::create(target).map_err(|error| RemoteError::Failed(format!("failed to create {target}: {error}")))?;
    std::io::copy(&mut input, &mut output)
        .map_err(|error| RemoteError::Failed(format!("failed to copy {source} to {target}: {error}")))?;
    Ok(())
}

fn file_size(path: &str) -> Result<u64, RemoteError> {
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .map_err(|error| RemoteError::Failed(format!("failed to read the size of {path}: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_dir(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("fattorini-remote-test-{name}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).expect("failed to create the fixture directory");
        dir.to_str().unwrap().to_string()
    }

    #[test]
    fn remote_path_mirrors_the_local_layout_under_the_root() {
        assert_eq!(remote_path("data/lc0/test"), format!("{ROOT}/data/lc0/test"));
        assert_eq!(remote_path_under("/r", "data/lc0/frozen/"), "/r/data/lc0/frozen");
        assert_eq!(remote_path_under("/r", "data/books/book.bin"), "/r/data/books/book.bin");
    }

    #[test]
    #[should_panic(expected = "is not under data/")]
    fn remote_path_refuses_a_path_outside_data() {
        remote_path("configs/pesto-psq.yaml");
    }

    #[test]
    #[should_panic(expected = "is not under data/")]
    fn remote_path_refuses_a_directory_that_only_starts_with_data() {
        remote_path("database/x");
    }

    #[test]
    fn check_mounted_needs_the_root_s_parent_not_the_root() {
        let drive = fixture_dir("mounted");
        assert_eq!(check_mounted_under(&format!("{drive}/fattorini")), Ok(()), "the root is created by the first upload");
        assert!(matches!(check_mounted_under(&format!("{drive}/missing/fattorini")), Err(RemoteError::NotMounted(_))));
        fs::remove_dir_all(&drive).ok();
    }

    #[test]
    fn move_replacing_overwrites_a_different_file_and_removes_the_source() {
        let dir = fixture_dir("move-replacing");
        let source = format!("{dir}/book.bin");
        let target = format!("{dir}/remote/books/book.bin");
        fs::write(&source, b"old").unwrap();
        move_replacing(&source, &target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"old");
        assert!(!Path::new(&source).exists(), "a move removes the source");

        fs::write(&source, b"rebuilt, longer").unwrap();
        move_replacing(&source, &target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"rebuilt, longer", "a rebuilt book replaces the old one");
        assert!(!Path::new(&format!("{target}{PARTIAL_SUFFIX}")).exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn copy_replacing_overwrites_a_file_of_the_same_size_and_keeps_the_source() {
        let dir = fixture_dir("copy-replacing");
        let source = format!("{dir}/candidate.nnue");
        let target = format!("{dir}/remote/lambda-0.25/candidate.nnue");
        fs::write(&source, b"old").unwrap();
        copy_replacing(&source, &target).unwrap();
        fs::write(&source, b"new").unwrap();
        copy_replacing(&source, &target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new", "the same size, replaced all the same");
        assert!(Path::new(&source).exists(), "a copy keeps the source");
        assert!(!Path::new(&format!("{target}{PARTIAL_SUFFIX}")).exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn is_segment_file_takes_segments_and_their_companions_only() {
        assert!(is_segment_file("alpha-20260926-1405.pgn.zst"));
        assert!(is_segment_file("alpha-20260926-1405.pgn.zst.meta"));
        for name in [
            "alpha-20260926-1405.pgn.zst.tmp",
            "alpha-20260926-1405.pgn.zst.meta.tmp",
            "alpha-20260926-1405.pgn.zst.count",
            "alpha.pgn",
            "alpha.count",
            "fattorini-self.lock",
            "x.pgn.zst.partial",
        ] {
            assert!(!is_segment_file(name), "{name:?}");
        }
    }

    #[test]
    fn copy_files_copies_what_is_accepted_and_missing_and_skips_what_is_there() {
        let source = fixture_dir("copy-source");
        let target = format!("{}/nested/target", fixture_dir("copy-target"));
        fs::write(format!("{source}/a.pgn.zst"), b"aaa").unwrap();
        fs::write(format!("{source}/a.pgn.zst.meta"), b"positions\t3\ngames\t1\n").unwrap();
        fs::write(format!("{source}/alpha.pgn"), b"staging").unwrap();
        fs::create_dir_all(format!("{source}/subdir")).unwrap();

        copy_files(&source, &target, &is_segment_file).unwrap();
        assert_eq!(list_files(&target, &|_| true).unwrap(), vec!["a.pgn.zst", "a.pgn.zst.meta"]);
        assert_eq!(fs::read(format!("{target}/a.pgn.zst")).unwrap(), b"aaa");

        // A second copy finds everything there with the same size and changes nothing.
        copy_files(&source, &target, &is_segment_file).unwrap();
        assert_eq!(list_files(&target, &|_| true).unwrap(), vec!["a.pgn.zst", "a.pgn.zst.meta"]);
    }

    #[test]
    fn copy_files_refuses_to_overwrite_a_different_file_of_the_same_name() {
        let source = fixture_dir("clobber-source");
        let target = fixture_dir("clobber-target");
        fs::write(format!("{source}/a.pgn.zst"), b"new games").unwrap();
        fs::write(format!("{target}/a.pgn.zst"), b"old").unwrap();

        assert!(matches!(copy_files(&source, &target, &is_segment_file), Err(RemoteError::Failed(_))));
        assert_eq!(fs::read(format!("{target}/a.pgn.zst")).unwrap(), b"old", "the existing file is untouched");
    }

    #[test]
    fn list_files_ignores_partials_and_is_empty_for_a_missing_directory() {
        let dir = fixture_dir("list");
        fs::write(format!("{dir}/b.pgn.zst"), b"").unwrap();
        fs::write(format!("{dir}/a.pgn.zst"), b"").unwrap();
        fs::write(format!("{dir}/c.pgn.zst.partial"), b"").unwrap();
        assert_eq!(list_files(&dir, &|_| true).unwrap(), vec!["a.pgn.zst", "b.pgn.zst"]);
        assert_eq!(list_files(&format!("{dir}/missing"), &|_| true).unwrap(), Vec::<String>::new());
    }
}
