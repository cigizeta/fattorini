//! The judge: one UCI engine of `data/engines.yaml`, driven over stdin/stdout to evaluate one
//! position at a time at a fixed depth. Not the arena's `UciClient`, which is built for matches
//! (clocks, faults that decide a pairing, CPU pinning): a book needs only `go depth` and the score.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde::Deserialize;

/// Where each machine names its engines, gitignored; `examples/engines.yaml` shows its shape.
pub const ENGINE_REGISTRY_PATH: &str = "data/engines.yaml";

/// How long an engine may take to answer `uci` and `isready`, as cutechess-cli waits.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(35);

#[derive(Debug, Deserialize)]
struct RegistryFile {
    engines: HashMap<String, RegistryEntry>,
}

/// One entry, read for what a UCI engine needs. The arena's in-process kinds (`builtin`,
/// `network`) have no executable to drive and are refused.
#[derive(Debug, Clone, Deserialize)]
pub struct RegistryEntry {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub options: HashMap<String, serde_yaml::Value>,
}

/// The entry `name` of the registry's YAML text, checked to be a UCI engine with a path.
pub fn registry_entry(yaml: &str, name: &str) -> Result<RegistryEntry, String> {
    let file: RegistryFile = serde_yaml::from_str(yaml).map_err(|error| format!("{ENGINE_REGISTRY_PATH}: {error}"))?;
    let mut engines = file.engines;
    let entry = engines.remove(name).ok_or_else(|| format!("no engine {name:?} in {ENGINE_REGISTRY_PATH}"))?;
    if entry.kind.as_deref().is_some_and(|kind| kind != "uci") {
        return Err(format!("engine {name:?} is not a UCI engine"));
    }
    if entry.path.is_none() {
        return Err(format!("engine {name:?} has no path"));
    }
    Ok(entry)
}

/// A score as the engine reports it: from the side to move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Score {
    Cp(i32),
    Mate(i32),
}

/// What one search gave: the score of the deepest depth the engine completed, and that depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Search {
    pub score: Score,
    pub depth: u8,
}

pub struct Engine {
    /// The engine's `id name`, for the book's header.
    pub name: String,
    child: Child,
    stdin: ChildStdin,
    lines: mpsc::Receiver<String>,
}

impl Engine {
    /// Starts the entry's engine: `uci`, its options, then `Threads` 1 whatever they say (one
    /// thread, so a rebuild gives the same evaluations), `isready`.
    pub fn start(entry: &RegistryEntry) -> Result<Engine, String> {
        let path = entry.path.as_deref().expect("registry_entry checked the path");
        let mut command = Command::new(path);
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        // In a process group of its own, so a console's Ctrl+C reaches the smithy alone, which then
        // stops its engines itself.
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP);
        }
        let mut child = command.spawn().map_err(|error| format!("cannot start {path}: {error}"))?;
        let stdin = child.stdin.take().expect("piped stdin");
        let lines = spawn_line_reader(BufReader::new(child.stdout.take().expect("piped stdout")));
        let mut engine = Engine { name: String::new(), child, stdin, lines };

        engine.send("uci")?;
        let mut name = None;
        engine.wait_for(Some(HANDSHAKE_TIMEOUT), |line| {
            if let Some(id) = line.strip_prefix("id name ") {
                name = Some(id.trim().to_string());
            }
            line == "uciok"
        })?;
        engine.name = name.ok_or("the engine sent no id name")?;
        let mut options: Vec<_> = entry.options.iter().filter(|(option, _)| option.as_str() != "Threads").collect();
        options.sort_by(|a, b| a.0.cmp(b.0));
        for (option, value) in options {
            engine.send(&format!("setoption name {option} value {}", option_value(value)?))?;
        }
        engine.send("setoption name Threads value 1")?;
        engine.ready()?;
        Ok(engine)
    }

    /// The position of `fen` searched to `depth` from a fresh state (`ucinewgame`). Waits for
    /// `bestmove` without a timeout: a fixed depth takes what it takes.
    pub fn search(&mut self, fen: &str, depth: u8) -> Result<Search, String> {
        self.send("ucinewgame")?;
        self.ready()?;
        self.send(&format!("position fen {fen}"))?;
        self.send(&format!("go depth {depth}"))?;
        let mut last = None;
        self.wait_for(None, |line| {
            if let Some(info) = line.strip_prefix("info ")
                && let Some(search) = parse_info(info)
            {
                last = Some(search);
            }
            line.starts_with("bestmove")
        })?;
        last.ok_or_else(|| format!("{} gave no score for {fen}", self.name))
    }

    fn ready(&mut self) -> Result<(), String> {
        self.send("isready")?;
        self.wait_for(Some(HANDSHAKE_TIMEOUT), |line| line == "readyok")
    }

    fn send(&mut self, command: &str) -> Result<(), String> {
        writeln!(self.stdin, "{command}").and_then(|()| self.stdin.flush()).map_err(|error| format!("{}: {error}", self.name))
    }

    fn wait_for(&mut self, timeout: Option<Duration>, mut done: impl FnMut(&str) -> bool) -> Result<(), String> {
        loop {
            let line = match timeout {
                Some(timeout) => self.lines.recv_timeout(timeout).map_err(|error| match error {
                    mpsc::RecvTimeoutError::Timeout => "the engine did not answer in time".to_string(),
                    mpsc::RecvTimeoutError::Disconnected => "the engine exited".to_string(),
                })?,
                None => self.lines.recv().map_err(|_| "the engine exited".to_string())?,
            };
            if done(&line) {
                return Ok(());
            }
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.send("quit");
        let _ = self.lines.recv_timeout(Duration::from_millis(200));
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// An `info` line's depth and score, or `None` when it carries no score, or only a bound (a
/// `lowerbound` or `upperbound` score is a fail-high or fail-low, not the depth's result).
fn parse_info(info: &str) -> Option<Search> {
    let tokens: Vec<&str> = info.split_whitespace().collect();
    let value_after = |key: &str| tokens.iter().position(|&token| token == key).and_then(|index| tokens.get(index + 1));
    if tokens.iter().any(|&token| token == "lowerbound" || token == "upperbound") {
        return None;
    }
    let depth = value_after("depth")?.parse().ok()?;
    let score_index = tokens.iter().position(|&token| token == "score")?;
    let value: i32 = tokens.get(score_index + 2)?.parse().ok()?;
    let score = match *tokens.get(score_index + 1)? {
        "cp" => Score::Cp(value),
        "mate" => Score::Mate(value),
        _ => return None,
    };
    Some(Search { score, depth })
}

/// A YAML option value as `setoption ... value` text.
fn option_value(value: &serde_yaml::Value) -> Result<String, String> {
    match value {
        serde_yaml::Value::Bool(value) => Ok(value.to_string()),
        serde_yaml::Value::Number(value) => Ok(value.to_string()),
        serde_yaml::Value::String(value) => Ok(value.clone()),
        other => Err(format!("unsupported option value in {ENGINE_REGISTRY_PATH}: {other:?}")),
    }
}

/// The engine's stdout a line at a time on a channel, so a wait can time out.
fn spawn_line_reader(mut stdout: BufReader<std::process::ChildStdout>) -> mpsc::Receiver<String> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut line = String::new();
        loop {
            line.clear();
            match stdout.read_line(&mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {
                    if sender.send(line.trim_end().to_string()).is_err() {
                        return;
                    }
                }
            }
        }
    });
    receiver
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_info_line_gives_its_depth_and_score() {
        let line = "depth 20 seldepth 27 multipv 1 score cp -35 nodes 123456 nps 1000000 pv e7e5";
        assert_eq!(parse_info(line), Some(Search { score: Score::Cp(-35), depth: 20 }));
        assert_eq!(parse_info("depth 9 seldepth 9 score mate -3 nodes 100 pv e1e2"), Some(Search { score: Score::Mate(-3), depth: 9 }));
    }

    #[test]
    fn a_bound_or_a_line_without_score_is_no_result() {
        assert_eq!(parse_info("depth 20 seldepth 25 score cp 40 lowerbound nodes 99"), None);
        assert_eq!(parse_info("depth 20 currmove e2e4 currmovenumber 1"), None);
        assert_eq!(parse_info("string NNUE evaluation using nn.nnue"), None);
    }

    #[test]
    fn the_registry_gives_a_uci_entry_and_refuses_the_others() {
        let yaml = "engines:\n  judge:\n    path: \"C:/engines/judge.exe\"\n    options:\n      Hash: 64\n  pesto:\n    kind: builtin\n";
        let entry = registry_entry(yaml, "judge").unwrap();
        assert_eq!(entry.options.len(), 1);
        assert!(registry_entry(yaml, "pesto").is_err());
        assert!(registry_entry(yaml, "missing").is_err());
    }
}
