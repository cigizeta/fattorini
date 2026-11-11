//! A run's files copied between this machine and the remote, so a run moves between smithies
//! (`lease.rs`): up when a smithy lets the run go and every 30 minutes of training, down when a
//! smithy resumes a run whose newest state is on the remote. Each file whole, through
//! `remote::copy_replacing`, whose reader never sees a half-copied file.

use std::path::Path;
use std::time::SystemTime;

use fattorini::pipeline::remote;

use super::archive;

/// Copies every file under `from` that `keep` takes, by its path relative to `from`, to the same
/// place under `to`: all of them when `force`, otherwise those `to` lacks, holds at another size, or
/// holds older than `from`'s. A `.partial`, a copy cut short, is never copied. Returns the files and
/// bytes copied.
pub(crate) fn copy(from: &str, to: &str, keep: &dyn Fn(&str) -> bool, force: bool) -> Result<(usize, u64), String> {
    let (mut files, mut bytes) = (0, 0);
    for (path, size) in archive::files_under(from)? {
        let relative = path[from.len()..].trim_start_matches('/');
        if relative.ends_with(".partial") || !keep(relative) {
            continue;
        }
        let target = format!("{to}/{relative}");
        if !force && current(&path, &target, size) {
            continue;
        }
        remote::copy_replacing(&path, &target).map_err(|error| error.to_string())?;
        files += 1;
        bytes += size;
    }
    Ok((files, bytes))
}

/// Whether `target` already holds `source`, `size` bytes: at its size, and modified no earlier.
fn current(source: &str, target: &str, size: u64) -> bool {
    let modified = |path: &str| std::fs::metadata(path).and_then(|metadata| metadata.modified()).ok();
    let Ok(metadata) = std::fs::metadata(target) else { return false };
    metadata.len() == size && modified(target).unwrap_or(SystemTime::UNIX_EPOCH) >= modified(source).unwrap_or(SystemTime::now())
}

/// Whether `relative`, a path under a run's directory, is one of its own top-level files or under
/// `generation_dir`, its open generation; the lease is never copied, being the remote's alone, nor an
/// open self-play file (`.pgn.open`), whose copy would outlive its closing into a segment.
pub(crate) fn in_open_generation(relative: &str, generation_dir: &str, lease_file: &str) -> bool {
    if relative.ends_with(".pgn.open") {
        return false;
    }
    match relative.split_once('/') {
        None => relative != lease_file,
        Some((top, _)) => top == generation_dir,
    }
}

/// Whether `dir` exists.
pub(crate) fn exists(dir: &str) -> bool {
    Path::new(dir).is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("smithy-tree-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        dir.to_str().unwrap().replace('\\', "/")
    }

    fn write(path: &str, text: &str) {
        std::fs::create_dir_all(Path::new(path).parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn only_new_or_changed_files_are_copied_unless_forced() {
        let (from, to) = (tree("from"), tree("to"));
        write(&format!("{from}/state.txt"), "a");
        write(&format!("{from}/gen-1/lambda-0.01/model.vs"), "weights");
        write(&format!("{from}/gen-1/x.partial"), "cut");
        let all = |_: &str| true;
        assert_eq!(copy(&from, &to, &all, false).unwrap(), (2, 8), "the partial left out");
        assert_eq!(copy(&from, &to, &all, false).unwrap(), (0, 0), "nothing changed");
        write(&format!("{from}/state.txt"), "ab");
        assert_eq!(copy(&from, &to, &all, false).unwrap(), (1, 2));
        assert_eq!(std::fs::read_to_string(format!("{to}/state.txt")).unwrap(), "ab");
        assert_eq!(copy(&from, &to, &all, true).unwrap(), (2, 9), "forced");
        std::fs::remove_dir_all(&from).ok();
        std::fs::remove_dir_all(&to).ok();
    }

    #[test]
    fn a_run_s_open_generation_and_top_files_are_kept_its_lease_never() {
        let keep = |relative: &str| in_open_generation(relative, "gen-2", "lease.txt");
        assert!(keep("state.txt"));
        assert!(keep("gen-2/lambda-0.01/optimizer.opt"));
        assert!(!keep("gen-1/teacher.nnue"), "a closed generation, the mover's");
        assert!(!keep("lease.txt"));
        assert!(!keep("gen-2/train/1793525797.pgn.open"), "an open self-play file");
        assert!(keep("gen-2/train/1793525715.pgn.zst"));
    }
}
