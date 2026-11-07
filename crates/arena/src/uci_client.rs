//! A UCI *client*: Fattorini driving an arbitrary engine subprocess over its own stdin/stdout --
//! the reverse role of `uci.rs`, which is a UCI *server* (Fattorini answering as the engine).
//! `uci.rs`'s command-parsing/dispatch loop is not reusable here; only its pure move-notation
//! functions (`format_move`/`parse_move`) are shared surface -- see the tournament design's "1. UCI
//! client" section.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use super::engines::{format_option_value, EngineEntry};
use super::player::{Fault, MoveLimit, MoveOutcome, MoveRequest, Player, SearchReport};

/// cutechess-cli's own real, source-confirmed defaults (`chessengine.h`'s
/// `defaultProtocolStartTimeout`/`defaultPingTimeout`), reused directly rather than picking
/// arbitrary numbers -- see the tournament design.
const PROTOCOL_START_TIMEOUT: Duration = Duration::from_secs(35);
const PING_TIMEOUT: Duration = Duration::from_secs(15);

/// How often `wait_for_bestmove`'s own wait re-checks `stop_requested` -- a blitz/bullet clock
/// budget can be tens of seconds long, and a single `recv_timeout` for the whole thing would
/// leave Ctrl+C unable to interrupt a game in progress until the searching side finally replies
/// or times out on its own.
const STOP_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub struct UciClient {
    name: String,
    child: Child,
    stdin: ChildStdin,
    lines: mpsc::Receiver<String>,
}

impl UciClient {
    /// Spawns `entry`'s own binary, completes the `uci`/`uciok` handshake, forces
    /// single-threaded play (`setoption name Threads value 1`, always, regardless of what
    /// `entry.options` says -- see the tournament design's "Match runner" section), sends every
    /// configured option, and confirms readiness (`isready`/`readyok`). Returns `Err(Fault)`
    /// rather than panicking: a slow-to-start or malformed engine is exactly the zero-tolerance
    /// case the match runner needs to turn into a pairing-deciding fault, not a process abort.
    pub fn start(name: &str, entry: &EngineEntry) -> Result<UciClient, Fault> {
        let mut command = Command::new(entry.binary_path());
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        // A console's Ctrl+C reaches every process attached to it, which would kill the engine out
        // from under us before `stop_requested` is even checked again, except those in a new
        // process group, where Ctrl+C is disabled: only the arena's own `ctrlc::set_handler` then
        // decides when to stop it, via `stop_and_kill`'s explicit `stop` + kill.
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP);
        }
        let mut child = command
            .spawn()
            .map_err(|error| Fault::Crashed(format!("failed to spawn {name} ({}): {error}", entry.binary_path())))?;
        // Windows does not pass the worker thread's pin on to the child (`cpu_topology.rs`).
        if let Err(message) = super::cpu_topology::pin_child(&child) {
            eprintln!("{name} plays unpinned: {message}");
        }

        let stdin = child.stdin.take().expect("spawned with piped stdin");
        let stdout = BufReader::new(child.stdout.take().expect("spawned with piped stdout"));
        let lines = spawn_line_reader(stdout);

        let mut client = UciClient { name: name.to_string(), child, stdin, lines };

        client.send("uci")?;
        client.wait_for(PROTOCOL_START_TIMEOUT, |line| line == "uciok")?;

        client.send("setoption name Threads value 1")?;
        for (option_name, value) in &entry.options {
            client.send(&format!("setoption name {option_name} value {}", format_option_value(value)))?;
        }

        client.send("isready")?;
        client.wait_for(PING_TIMEOUT, |line| line == "readyok")?;

        Ok(client)
    }

    /// `ucinewgame` followed by a fresh `isready`/`readyok` sync -- sent once per game, before
    /// that game's first `position`/`go`, the same convention real GUIs use so an engine's own
    /// between-games cleanup (e.g. clearing its hash) is known to be finished before play
    /// starts.
    pub fn new_game(&mut self) -> Result<(), Fault> {
        self.send("ucinewgame")?;
        self.send("isready")?;
        self.wait_for(PING_TIMEOUT, |line| line == "readyok")?;
        Ok(())
    }

    /// `position fen <starting FEN> moves <uci moves so far>` -- the whole move history resent
    /// every time, not an incremental update, the simplest and most robust convention (what most
    /// real GUIs/arbiters do) since it never depends on the engine having tracked anything from a
    /// previous `position` call correctly. Always an explicit `fen`, never the `startpos`
    /// shorthand, since a game can start from any opening book position, not just the standard
    /// starting position.
    pub fn set_position(&mut self, starting_fen: &str, moves_uci: &[String]) -> Result<(), Fault> {
        let moves = moves_uci.join(" ");
        self.send(&format!("position fen {starting_fen} moves {moves}"))
    }

    pub fn go(&mut self, wtime: Duration, btime: Duration, winc: Duration, binc: Duration) -> Result<(), Fault> {
        self.send(&format!(
            "go wtime {} btime {} winc {} binc {}",
            wtime.as_millis(),
            btime.as_millis(),
            winc.as_millis(),
            binc.as_millis()
        ))
    }

    /// Blocks until `bestmove` arrives or `budget` (the searching side's own remaining clock
    /// time) elapses first -- exceeding it is `MoveOutcome::TimedOut`, a time forfeit and a
    /// normal game outcome, not a `Fault`; only a genuinely malformed reply (`bestmove (none)`,
    /// or none at all before the process exits) is a fault. Tracks the last `info score`/
    /// `depth` seen along the way for the PGN annotation.
    pub fn wait_for_bestmove(&mut self, budget: Duration, stop_requested: &Arc<AtomicBool>) -> Result<MoveOutcome, Fault> {
        let deadline = Instant::now() + budget;
        let mut score_cp = None;
        let mut mate_in = None;
        let mut depth = None;

        loop {
            if stop_requested.load(Ordering::SeqCst) {
                return Ok(MoveOutcome::Interrupted);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(MoveOutcome::TimedOut);
            }
            match self.lines.recv_timeout(remaining.min(STOP_POLL_INTERVAL)) {
                Ok(line) => {
                    if let Some(rest) = line.strip_prefix("info ") {
                        let tokens: Vec<&str> = rest.split_whitespace().collect();
                        match parse_info_score(&tokens) {
                            (Some(cp), None) => {
                                score_cp = Some(cp);
                                mate_in = None;
                            }
                            (None, Some(mate)) => {
                                mate_in = Some(mate);
                                score_cp = None;
                            }
                            _ => {}
                        }
                        if let Some(parsed_depth) = parse_info_depth(&tokens) {
                            depth = Some(parsed_depth);
                        }
                    } else if let Some(rest) = line.strip_prefix("bestmove ") {
                        let best_move_uci = rest.split_whitespace().next().unwrap_or("").to_string();
                        if best_move_uci.is_empty() || best_move_uci == "(none)" {
                            return Err(Fault::IllegalMove(format!("{} returned no move: {line:?}", self.name)));
                        }
                        return Ok(MoveOutcome::Move(SearchReport { best_move_uci, score_cp, mate_in, depth }));
                    }
                    // Anything else (stray `id`/`option` lines, blank lines): ignore, keep waiting.
                }
                // A poll slice elapsing isn't the real deadline -- just loop back around to
                // re-check `stop_requested` and the actual remaining budget.
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(Fault::Crashed(format!("{} exited unexpectedly", self.name)));
                }
            }
        }
    }

    /// `go depth <n>` -- the fixed-depth counterpart of `go`, with no clock tokens at all rather
    /// than a clock the engine is then asked to ignore. A UCI engine given both would be free to
    /// stop early on time, which is exactly the handicap a fixed-depth match exists to remove.
    pub fn go_depth(&mut self, depth: u32) -> Result<(), Fault> {
        self.send(&format!("go depth {depth}"))
    }

    /// Sends `stop`, gives the engine a short grace window to answer `bestmove` on its own,
    /// then kills the subprocess outright either way. Used for both an interrupted (Ctrl+C
    /// dropped) game and normal end-of-pairing shutdown -- the same "stop the engine currently
    /// to move" the design doc describes, not a graceful `quit` handshake that could itself
    /// hang against a broken engine.
    pub fn stop_and_kill(&mut self) {
        let _ = self.send("stop");
        let _ = self.lines.recv_timeout(Duration::from_millis(200));
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn send(&mut self, command: &str) -> Result<(), Fault> {
        writeln!(self.stdin, "{command}")
            .map_err(|error| Fault::Crashed(format!("{} stdin write failed: {error}", self.name)))?;
        self
            .stdin
            .flush()
            .map_err(|error| Fault::Crashed(format!("{} stdin flush failed: {error}", self.name)))
    }

    /// Reads lines until one matches `matches`, discarding everything else (`id`/`option`
    /// lines during the handshake) -- or returns `Fault::HandshakeTimedOut` if `timeout`
    /// elapses first, including the case where the engine never sends a single line at all
    /// (`recv_timeout` on the background reader's channel bounds that directly, unlike a plain
    /// blocking `read_line` would).
    fn wait_for(&mut self, timeout: Duration, matches: impl Fn(&str) -> bool) -> Result<(), Fault> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Fault::HandshakeTimedOut);
            }
            match self.lines.recv_timeout(remaining) {
                Ok(line) => {
                    if matches(&line) {
                        return Ok(());
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => return Err(Fault::HandshakeTimedOut),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(Fault::Crashed(format!("{} exited unexpectedly", self.name)));
                }
            }
        }
    }
}

/// A `Player` over a subprocess: the four-call UCI sequence collapsed into one `choose_move`, with
/// no behaviour change whatsoever -- the same `position`, the same `go`, the same
/// `wait_for_bestmove`, in the same order, against the same `UciClient` underneath. External
/// engines keep working exactly as they did.
pub struct UciPlayer {
    client: UciClient,
}

impl UciPlayer {
    pub fn start(name: &str, entry: &EngineEntry) -> Result<UciPlayer, Fault> {
        Ok(UciPlayer { client: UciClient::start(name, entry)? })
    }
}

impl Player for UciPlayer {
    fn new_game(&mut self) -> Result<(), Fault> {
        self.client.new_game()
    }

    fn choose_move(&mut self, request: &MoveRequest, stop_requested: &Arc<AtomicBool>) -> Result<MoveOutcome, Fault> {
        self.client.set_position(request.starting_fen, request.moves_uci)?;
        match request.limit {
            MoveLimit::Fischer { white_remaining, black_remaining, increment } => {
                self.client.go(white_remaining, black_remaining, increment, increment)?;
            }
            MoveLimit::FixedDepth(depth) => self.client.go_depth(depth)?,
        }
        self.client.wait_for_bestmove(request.limit.budget(request.board.side_to_move()), stop_requested)
    }

    fn shutdown(&mut self) {
        self.client.stop_and_kill();
    }
}

/// Streams `stdout` a line at a time onto a channel from a dedicated background thread -- the
/// only way to give a blocking `ChildStdout` a real timeout in `std`, which exposes no
/// per-read deadline of its own: `recv_timeout` on the receiving end bounds the wait instead,
/// correctly covering the case where the child never writes anything at all, not just the gap
/// between two lines. The thread exits (dropping `sender`, closing the channel) on EOF, a read
/// error, or once nothing is listening any more.
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

fn parse_info_score(tokens: &[&str]) -> (Option<i32>, Option<i32>) {
    let Some(index) = tokens.iter().position(|&token| token == "score") else {
        return (None, None);
    };
    match tokens.get(index + 1) {
        Some(&"cp") => (tokens.get(index + 2).and_then(|value| value.parse().ok()), None),
        Some(&"mate") => (None, tokens.get(index + 2).and_then(|value| value.parse().ok())),
        _ => (None, None),
    }
}

fn parse_info_depth(tokens: &[&str]) -> Option<u32> {
    let index = tokens.iter().position(|&token| token == "depth")?;
    tokens.get(index + 1).and_then(|value| value.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_info_score_reads_a_centipawn_score() {
        let tokens: Vec<&str> = "depth 12 seldepth 14 score cp 35 nodes 1000".split_whitespace().collect();
        assert_eq!(parse_info_score(&tokens), (Some(35), None));
    }

    #[test]
    fn parse_info_score_reads_a_mate_score() {
        let tokens: Vec<&str> = "depth 5 score mate 3 nodes 1000".split_whitespace().collect();
        assert_eq!(parse_info_score(&tokens), (None, Some(3)));
    }

    #[test]
    fn parse_info_depth_reads_the_depth_token_not_seldepth() {
        let tokens: Vec<&str> = "seldepth 20 depth 12 score cp 10".split_whitespace().collect();
        assert_eq!(parse_info_depth(&tokens), Some(12));
    }
}
