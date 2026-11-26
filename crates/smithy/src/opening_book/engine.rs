//! The judge: one UCI engine of `data/engines.yaml`, driven over stdin/stdout. Not the arena's
//! `UciClient`, which is built for matches (clocks, faults that decide a pairing, CPU pinning): a
//! book needs `go depth`, `searchmoves`, the score and the best move.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;

use super::unit::Score;

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
    path: Option<String>,
    #[serde(default)]
    options: HashMap<String, serde_yaml::Value>,
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

/// What one search gave: the best move in UCI notation, its score from the side to move, the
/// deepest depth the engine completed, and the nodes and time it took.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Search {
    pub best_move: String,
    pub score: Score,
    pub depth: u8,
    pub nodes: u64,
    pub time: Duration,
}

/// What an expansion asks of an engine; a trait so a scripted engine tests the expansion.
pub trait Judge {
    /// A fresh state before a new position: `ucinewgame`, so a rebuild gives the same searches.
    fn new_position(&mut self) -> Result<(), String>;
    /// `fen` searched to `depth`, restricted to `searchmoves` (UCI notation) when given; the
    /// table is kept from the searches before it since `new_position`.
    fn search(&mut self, fen: &str, depth: u8, searchmoves: Option<&[String]>) -> Result<Search, String>;
}

pub struct Engine {
    /// The engine's `id name`, for the fragments' header.
    pub name: String,
    child: Child,
    stdin: ChildStdin,
    lines: mpsc::Receiver<String>,
}

impl Engine {
    /// Starts the entry's engine: `uci`, its options in name order, then `Threads` 1 whatever they
    /// say (one thread, so a rebuild gives the same searches), `isready`.
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

impl Judge for Engine {
    fn new_position(&mut self) -> Result<(), String> {
        self.send("ucinewgame")?;
        self.ready()
    }

    /// Waits for `bestmove` without a timeout: a fixed depth takes what it takes.
    fn search(&mut self, fen: &str, depth: u8, searchmoves: Option<&[String]>) -> Result<Search, String> {
        self.send(&format!("position fen {fen}"))?;
        let started = Instant::now();
        self.send(&go_command(depth, searchmoves))?;
        let mut last: Option<Info> = None;
        let mut best_move = None;
        self.wait_for(None, |line| {
            if let Some(info) = line.strip_prefix("info ")
                && let Some(parsed) = parse_info(info)
            {
                last = Some(parsed);
            }
            if let Some(rest) = line.strip_prefix("bestmove ") {
                best_move = rest.split_whitespace().next().map(str::to_string);
                return true;
            }
            false
        })?;
        let info = last.ok_or_else(|| format!("{} gave no score for {fen}", self.name))?;
        let best_move = best_move.filter(|mv| mv != "(none)").ok_or_else(|| format!("{} gave no move for {fen}", self.name))?;
        Ok(Search { best_move, score: info.score, depth: info.depth, nodes: info.nodes, time: started.elapsed() })
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

/// `go depth <d>`, with `searchmoves` and its moves when given.
fn go_command(depth: u8, searchmoves: Option<&[String]>) -> String {
    match searchmoves {
        Some(moves) => format!("go depth {depth} searchmoves {}", moves.join(" ")),
        None => format!("go depth {depth}"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Info {
    score: Score,
    depth: u8,
    nodes: u64,
}

/// An `info` line's score, depth and nodes, or `None` when it carries no score, or only a bound (a
/// `lowerbound` or `upperbound` score is a fail-high or fail-low, not the depth's result).
fn parse_info(info: &str) -> Option<Info> {
    let tokens: Vec<&str> = info.split_whitespace().collect();
    let value_after = |key: &str| tokens.iter().position(|&token| token == key).and_then(|index| tokens.get(index + 1));
    if tokens.iter().any(|&token| token == "lowerbound" || token == "upperbound") {
        return None;
    }
    let depth = value_after("depth")?.parse().ok()?;
    let nodes = value_after("nodes").and_then(|nodes| nodes.parse().ok()).unwrap_or(0);
    let score_index = tokens.iter().position(|&token| token == "score")?;
    let value: i32 = tokens.get(score_index + 2)?.parse().ok()?;
    let score = match *tokens.get(score_index + 1)? {
        "cp" => Score::Cp(value),
        "mate" => Score::Mate(value),
        _ => return None,
    };
    Some(Info { score, depth, nodes })
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
    fn an_info_line_gives_its_score_depth_and_nodes() {
        let line = "depth 20 seldepth 27 multipv 1 score cp -35 nodes 123456 nps 1000000 pv e7e5";
        assert_eq!(parse_info(line), Some(Info { score: Score::Cp(-35), depth: 20, nodes: 123_456 }));
        let mate = parse_info("depth 9 seldepth 9 score mate -3 nodes 100 pv e1e2").unwrap();
        assert_eq!(mate.score, Score::Mate(-3));
    }

    #[test]
    fn a_bound_or_a_line_without_score_is_no_result() {
        assert_eq!(parse_info("depth 20 seldepth 25 score cp 40 lowerbound nodes 99"), None);
        assert_eq!(parse_info("depth 20 currmove e2e4 currmovenumber 1"), None);
        assert_eq!(parse_info("string NNUE evaluation using nn.nnue"), None);
    }

    #[test]
    fn go_lists_the_searchmoves() {
        assert_eq!(go_command(20, None), "go depth 20");
        let moves = ["d2d4".to_string(), "g1f3".to_string()];
        assert_eq!(go_command(20, Some(&moves)), "go depth 20 searchmoves d2d4 g1f3");
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
