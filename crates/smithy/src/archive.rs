//! The mover: keeps the smithy's disk bounded. Every run's closed generations (promoted, or their
//! lineage stopped) move to the remote, Google Drive (`remote::remote_path`): each file copied whole under the same relative path, then deleted here. A file is made
//! here, used here while its generation may still read it, and only then moved, so Drive receives
//! finished files only, never one being written or appended to.
//!
//! A move is safe to interrupt: a file already on the remote at its size is not copied again, and is
//! deleted here; the next pass finishes what the last left. A machine that cannot reach the remote
//! keeps everything, and tries again at the next pass.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

use kalo_chess::pipeline::{players, remote};

/// Every file under `dir`, recursively, with its size: paths joined with `/`.
pub(crate) fn files_under(dir: &str) -> Result<Vec<(String, u64)>, String> {
    let mut files = Vec::new();
    let mut pending = vec![dir.to_string()];
    while let Some(current) = pending.pop() {
        let entries = match std::fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("{current}: {error}")),
        };
        for entry in entries {
            let entry = entry.map_err(|error| format!("{current}: {error}"))?;
            let path = format!("{current}/{}", entry.file_name().to_string_lossy());
            let metadata = entry.metadata().map_err(|error| format!("{path}: {error}"))?;
            if metadata.is_dir() {
                pending.push(path);
            } else {
                files.push((path, metadata.len()));
            }
        }
    }
    files.sort();
    Ok(files)
}

/// Removes the directories under `dir`, and `dir` itself, left empty, deepest first.
fn remove_empty_dirs(dir: &str) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            remove_empty_dirs(&format!("{dir}/{}", entry.file_name().to_string_lossy()));
        }
    }
    // Fails, as it should, while anything is left in it.
    let _ = std::fs::remove_dir(dir);
}

/// Moves every file under `local_dir` to the same place under `remote_dir`: copied whole (through
/// `remote::copy_replacing`, whose reader never sees a half-copied file), unless the remote already
/// holds it at its size, then deleted here; the emptied directories go. The files moved and their
/// bytes.
pub(crate) fn move_dir(local_dir: &str, remote_dir: &str) -> Result<(usize, u64), String> {
    let files = files_under(local_dir)?;
    let mut bytes = 0;
    for (path, size) in &files {
        let target = format!("{remote_dir}{}", &path[local_dir.len()..]);
        if !std::fs::metadata(&target).is_ok_and(|metadata| metadata.len() == *size) {
            remote::copy_replacing(path, &target).map_err(|error| error.to_string())?;
        }
        std::fs::remove_file(path).map_err(|error| format!("{path}: {error}"))?;
        bytes += size;
    }
    remove_empty_dirs(local_dir);
    Ok((files.len(), bytes))
}

/// The runs here, every config's run directories holding a `state.txt`, by name
/// (`players::run_name`).
fn runs() -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(players::RUNS_ROOT) else { return Vec::new() };
    let mut names: Vec<String> = Vec::new();
    for config in entries.flatten().filter_map(|entry| entry.file_name().into_string().ok()) {
        for number in players::runs_in(&players::config_dir(&config)) {
            let run = players::run_name(&config, number);
            let Ok(dir) = players::run_dir(&run) else { continue };
            if Path::new(&dir).join("state.txt").is_file() {
                names.push(run);
            }
        }
    }
    names.sort();
    names
}

/// Moves generation `generation` of `run` to the remote, its directory gone here.
fn move_generation(run: &str, generation: u32) -> Result<(usize, u64), String> {
    let dir = format!("{}/{}", players::run_dir(run)?, players::generation_dir_name(generation));
    move_dir(&dir, &remote::remote_path(&dir))
}

fn gigabytes(bytes: u64) -> String {
    format!("{:.2} GB", bytes as f64 / 1e9)
}

/// What a pass needs from the smithy.
#[derive(Clone, Copy)]
pub(crate) struct Hooks {
    /// A run's closed generations still here, from its state and config.
    pub closed: fn(&str) -> Result<Vec<u32>, String>,
    /// Prints a line without breaking the smithy's live display.
    pub say: fn(String),
}

/// One pass: every run's closed generations still here moved, oldest first, until done, the remote
/// fails to take one, or `stop`.
fn pass(hooks: Hooks, stop: &AtomicBool) {
    let say = hooks.say;
    let mut pending = Vec::new();
    for run in runs() {
        match (hooks.closed)(&run) {
            Ok(generations) => pending.extend(generations.into_iter().map(|generation| (run.clone(), generation))),
            Err(message) => say(format!("move: WARNING: {run}: {message}, so none of it is moved")),
        }
    }
    for (index, (run, generation)) in pending.iter().enumerate() {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let started = Instant::now();
        match move_generation(run, *generation) {
            Ok((files, bytes)) => {
                let seconds = started.elapsed().as_secs();
                say(format!(
                    "move: {run} gen-{generation} moved to {}, {files} files, {}, in {}m{:02}s",
                    remote::this_root(),
                    gigabytes(bytes),
                    seconds / 60,
                    seconds % 60
                ));
            }
            Err(message) => {
                say(format!("move: {run} gen-{generation}: {message}; {} generations pending", pending.len() - index));
                return;
            }
        }
    }
}

/// The mover's background thread: a pass whenever poked, pokes arriving during one collapsing into
/// one more.
pub(crate) struct Archiver {
    poke: mpsc::Sender<()>,
    busy: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

impl Archiver {
    /// Starts the thread, and its first pass.
    pub(crate) fn start(hooks: Hooks, stop: Arc<AtomicBool>) -> Archiver {
        let (poke, pokes) = mpsc::channel::<()>();
        let busy = Arc::new(AtomicBool::new(false));
        let thread = {
            let busy = Arc::clone(&busy);
            std::thread::spawn(move || {
                while pokes.recv().is_ok() {
                    while pokes.try_recv().is_ok() {}
                    busy.store(true, Ordering::SeqCst);
                    pass(hooks, &stop);
                    busy.store(false, Ordering::SeqCst);
                }
            })
        };
        let archiver = Archiver { poke, busy, thread };
        archiver.poke();
        archiver
    }

    /// Asks for a pass: at every phase boundary, so a generation just closed moves, and a pending
    /// move is tried again.
    pub(crate) fn poke(&self) {
        let _ = self.poke.send(());
    }

    /// Lets the pass under way, and one asked for, finish: for a smithy with nothing left to do.
    pub(crate) fn finish(self, say: fn(String)) {
        if self.busy.load(Ordering::SeqCst) {
            say("move: finishing, Ctrl+C to leave it for next time".to_string());
        }
        drop(self.poke);
        let _ = self.thread.join();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("smithy-move-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.to_string_lossy().replace('\\', "/")
    }

    #[test]
    fn a_directory_moves_whole_files_and_leaves_nothing_here() {
        let (local, drive) = (temp("local"), temp("drive"));
        let generation = format!("{local}/gen-1");
        for (path, text) in [("teacher.nnue", "t"), ("train/a.pgn.zst", "games"), ("lambda-0.95/history.tsv", "h\n")] {
            let path = format!("{generation}/{path}");
            std::fs::create_dir_all(Path::new(&path).parent().unwrap()).unwrap();
            std::fs::write(&path, text).unwrap();
        }
        // A candidate already published, and a copy an interrupted move left: neither copied again.
        let remote_gen = format!("{drive}/gen-1");
        std::fs::create_dir_all(format!("{remote_gen}/train")).unwrap();
        std::fs::write(format!("{remote_gen}/teacher.nnue"), "t").unwrap();
        assert_eq!(move_dir(&generation, &remote_gen), Ok((3, 1 + 5 + 2)));
        assert!(!Path::new(&generation).exists(), "nothing left here, the directory included");
        assert_eq!(std::fs::read_to_string(format!("{remote_gen}/train/a.pgn.zst")).unwrap(), "games");
        assert_eq!(std::fs::read_to_string(format!("{remote_gen}/lambda-0.95/history.tsv")).unwrap(), "h\n");
        assert!(!Path::new(&format!("{remote_gen}/train/a.pgn.zst.partial")).exists(), "no half-copied file left");
        assert_eq!(move_dir(&generation, &remote_gen), Ok((0, 0)), "a second pass finds nothing");
        std::fs::remove_dir_all(&local).ok();
        std::fs::remove_dir_all(&drive).ok();
    }
}
