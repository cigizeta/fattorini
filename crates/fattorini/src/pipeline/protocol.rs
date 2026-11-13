//! The smithy's protocol (`smithy start`) with the arenas (`arena start`), over HTTP (`lan_http`), in
//! two directions. Every answer is `200` with a status word line.
//!
//! **Smithy to arena, the claim**, on the arena's `ARENA_PORT`. An arena started by hand idles until a
//! smithy claims it, then serves that smithy alone. The smithy probes the arenas `data/machines.yaml`
//! on the remote lists every `CLAIM_POLL`:
//!
//! - `POST /claim` with a `Claim`: `OK` with the arena's `Hello`; `CLAIMED` with the machine of the
//!   smithy holding it. A new boot of the smithy holding the arena (a restarted smithy, same machine)
//!   takes it over at once. Commits are never compared: the smithy only shows an arena's.
//! - `POST /alive` with the claim's boot id, every `CLAIM_POLL`: `OK` with an `Alive`, or `UNCLAIMED`
//!   when the arena is not this boot's (then claimed again). The polls are the claim's keep-alive: an
//!   arena unpolled for `RELEASE_AFTER` releases itself; a smithy unanswered for `LATE_AFTER` says the
//!   arena is late, for `RELEASE_AFTER` lost, and closes its chore so its work is dealt again.
//! - `POST /release` with the boot id: the arena idles again, from a smithy stopping.
//!
//! **Arena to smithy, the work**, on `SMITHY_PORT`, at the host the claim named:
//!
//! - `POST /hello` with the arena's `Hello`: `OK` with a `HelloReply`, its session, the token, and the
//!   store its chores' networks are read from. Said again at every job, whose token is new.
//! - `POST /chore` with the session: `CHORE` with a `Chore`, `WAIT` with why nothing is dealt now (no
//!   job, or one training or between phases), or `UNKNOWN` (a restarted smithy: say hello again).
//! - `POST /task` with the session, a newline, and one finished task's PGN, one game or a pair's two
//!   (`task_body`): `OK`; `ADD` with more entries for the chore (`entries_text`); `STOP`, play nothing
//!   more of it; `CHORE` with a new chore replacing it; or `UNKNOWN`.
//! - Every request but `/hello` carries the token (`lan_http::TOKEN_HEADER`); a wrong one is answered
//!   `TOKEN`, and the arena says hello again. The token tells smithy starts apart; it is not
//!   authentication.
//!
//! The smithy's job (a training run or a tune) is dealt to the arenas as chores: a chore describes its
//! tasks (players, limit) and how many to play, the arena sends each task as soon as it ends, and the
//! smithy keeps an arena's chore topped up, stopped or replaced in its answers. It counts the tasks
//! that arrive, so nothing is leased: a task lost with an arena is simply never counted. Each task
//! carries its identity in its games' tags (`CHORE_TAG`, `SESSION_TAG`, `SEQ_TAG`), and one arriving
//! twice is dropped. A task of a chore closed meanwhile is still taken, the work its own business.
//!
//! Both binaries also answer `GET /control/status` and `POST /control/stop` on `127.0.0.1` alone, so a
//! status or a stop can only come from the machine itself.

use std::time::Duration;

use serde::Deserialize;

use super::lan::{field, fields};
use crate::search::{SearchParameters, TimeManagement};

/// The port the smithy listens on, for the arenas and its control.
pub const SMITHY_PORT: u16 = 47_821;

/// The port an arena's control listens on, on `127.0.0.1` only; taken, it means an arena already runs.
pub const ARENA_CONTROL_PORT: u16 = 47_822;

/// The port an arena is claimed on, by a smithy, on the LAN and `127.0.0.1`.
pub const ARENA_PORT: u16 = 47_823;

pub const CLAIM_PATH: &str = "/claim";
pub const ALIVE_PATH: &str = "/alive";
pub const RELEASE_PATH: &str = "/release";

/// How often the smithy claims the machines it lacks and polls those it holds.
pub const CLAIM_POLL: Duration = Duration::from_secs(10);

/// A claimed arena unanswering this long is late: said, nothing done.
pub const LATE_AFTER: Duration = Duration::from_secs(30);

/// A claim with no poll this long is over: the arena releases itself, and the smithy, unanswered as
/// long, takes the arena for lost.
pub const RELEASE_AFTER: Duration = Duration::from_secs(60);

/// `data/machines.yaml`, on the remote (`remote::remote_path`), so every machine reads one copy: the
/// one machine a production smithy runs on, and the machines it claims arenas on. Placeholders in
/// `examples/machines.yaml`.
pub const MACHINES_PATH: &str = "data/machines.yaml";

pub const HELLO_PATH: &str = "/hello";
pub const CHORE_PATH: &str = "/chore";
pub const TASK_PATH: &str = "/task";
/// `GET`, from another machine's smithy taking a lease, on the LAN: `OK`, this smithy's lease id,
/// then the jobs whose leases it holds or is taking (the smithy's `lease.rs`).
pub const JOBS_PATH: &str = "/jobs";
pub const STATUS_PATH: &str = "/control/status";
pub const STOP_PATH: &str = "/control/stop";

/// The loopback address every control uses.
pub const LOOPBACK: &str = "127.0.0.1";

/// The network name of the PeSTO evaluation, the player `pesto`, which has no file: where a chore
/// names any other network by its file's path in the store (`Player`).
pub const PESTO: &str = "pesto";

/// The tags every game of a task carries, which identify the task: its chore, the session that
/// played it, and its sequence number in that session.
pub const CHORE_TAG: &str = "Chore";
pub const SESSION_TAG: &str = "Session";
pub const SEQ_TAG: &str = "Seq";

/// `machines.yaml`: every machine, highest rank first, each a `data/machine.yaml` name the LAN
/// resolves. A smithy claims the arena of each, and between two jobs of the same kind the smithy
/// ranked higher keeps the arenas (the developer, 2026-10-29). The older `smithy:` and `arenas:` are
/// refused, with the form to write instead.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Machines {
    pub machines: Vec<String>,
}

impl Machines {
    /// `machine`'s rank, 0 the highest; `None` for a machine not listed, ranked after every listed one.
    pub fn rank(&self, machine: &str) -> Option<u32> {
        self.machines.iter().position(|name| name == machine).map(|index| index as u32)
    }
}

/// The machines `path` names, at least one, none twice. An error for a missing or broken file, saying
/// how to write it.
pub fn machines(path: &str) -> Result<Machines, String> {
    let how = "write it as `machines: [<name>, ...]`, every machine by its data/machine.yaml name, highest rank first, \
               as examples/machines.yaml does";
    let text = std::fs::read_to_string(path).map_err(|error| format!("{path}: {error}; {how}"))?;
    let config: Machines = serde_yaml::from_str(&text).map_err(|error| format!("{path}: {error}; {how}"))?;
    if config.machines.is_empty() {
        return Err(format!("{path}: no machine; {how}"));
    }
    for (index, name) in config.machines.iter().enumerate() {
        validate_machine_name(name).map_err(|error| format!("{path}: {error}; {how}"))?;
        if config.machines[..index].contains(name) {
            return Err(format!("{path}: {name} twice"));
        }
    }
    Ok(config)
}

/// The commit this checkout is at, `(modified)` after it when the working tree has changes,
/// `unknown` without git. Read at run time: `bin/smithy` and `bin/arena` build before they run, so
/// the binary is the working tree's. Shown on the smithy's page, never checked.
pub fn current_commit() -> String {
    let git = |args: &[&str]| -> Option<String> {
        let output = std::process::Command::new("git").args(args).output().ok()?;
        output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
    };
    let Some(head) = git(&["rev-parse", "HEAD"]).filter(|head| !head.is_empty()) else {
        return "unknown".to_string();
    };
    match git(&["status", "--porcelain"]) {
        Some(porcelain) if porcelain.is_empty() => head,
        _ => format!("{head} (modified)"),
    }
}

/// A smithy's claim on an arena: who claims it, where the arena's work goes, which start of the
/// smithy this is, and the commit it was built from, for the record.
#[derive(Debug, Clone, PartialEq)]
pub struct Claim {
    /// The smithy's machine, as its `data/machine.yaml` names it.
    pub smithy: String,
    /// The host the arena reaches the smithy at, on `SMITHY_PORT`: its machine, or `127.0.0.1`.
    pub host: String,
    /// New at every start of the smithy: a restarted smithy is told apart by it.
    pub boot: String,
    pub commit: String,
    /// Whether the smithy's job is a scaled one, which gives way to a full one's claim.
    pub scaled: bool,
    /// The smithy's rank in `machines.yaml` (`Machines::rank`), `None` unlisted.
    pub rank: Option<u32>,
}

impl Claim {
    pub fn text(&self) -> String {
        let rank = self.rank.map_or("-".to_string(), |rank| rank.to_string());
        format!(
            "smithy\t{}\nhost\t{}\nboot\t{}\ncommit\t{}\nscaled\t{}\nrank\t{rank}\n",
            self.smithy, self.host, self.boot, self.commit, u8::from(self.scaled)
        )
    }

    pub fn parse(text: &str) -> Result<Claim, String> {
        let map = fields(text);
        let scaled = match field::<String>(&map, "scaled", "claim")?.as_str() {
            "0" => false,
            "1" => true,
            other => return Err(format!("a claim's scaled is 0 or 1, not {other:?}")),
        };
        let rank = match field::<String>(&map, "rank", "claim")?.as_str() {
            "-" => None,
            rank => Some(rank.parse().map_err(|_| format!("a claim's rank is a number or -, not {rank:?}"))?),
        };
        Ok(Claim {
            smithy: field(&map, "smithy", "claim")?,
            host: field(&map, "host", "claim")?,
            boot: field(&map, "boot", "claim")?,
            commit: field(&map, "commit", "claim")?,
            scaled,
            rank,
        })
    }

    /// Who claims, as another smithy's page and the arena's status say it: its rank and its job's
    /// kind, never its machine's name.
    pub fn standing(&self) -> String {
        let kind = if self.scaled { "a scaled job" } else { "a full job" };
        match self.rank {
            Some(rank) => format!("the smithy ranked {}, for {kind}", rank + 1),
            None => format!("an unranked smithy, for {kind}"),
        }
    }

    /// Whether this claim takes an arena `held` holds (the developer, 2026-10-29): a
    /// full job's over a scaled one's, whatever the ranks; then, between jobs of the same kind, a
    /// smithy ranked higher, an unranked one after every ranked one; otherwise the holder keeps it.
    pub fn outranks(&self, held: &Claim) -> bool {
        if self.scaled != held.scaled {
            return !self.scaled;
        }
        match (self.rank, held.rank) {
            (Some(mine), Some(theirs)) => mine < theirs,
            (Some(_), None) => true,
            (None, _) => false,
        }
    }
}

/// A claimed arena's answer to a poll: what it is doing.
#[derive(Debug, Clone, PartialEq)]
pub struct Alive {
    /// Its session in the current job, `-` before it has said hello.
    pub session: String,
    /// Its chore, `-` without one.
    pub chore: String,
    pub in_flight: usize,
    /// Tasks sent since it started.
    pub sent: u64,
    /// Finished tasks it holds, not yet sent.
    pub outbox: usize,
}

impl Alive {
    pub fn text(&self) -> String {
        format!(
            "session\t{}\nchore\t{}\nin_flight\t{}\nsent\t{}\noutbox\t{}\n",
            self.session, self.chore, self.in_flight, self.sent, self.outbox
        )
    }

    pub fn parse(text: &str) -> Result<Alive, String> {
        let map = fields(text);
        Ok(Alive {
            session: field(&map, "session", "alive")?,
            chore: field(&map, "chore", "alive")?,
            in_flight: field(&map, "in_flight", "alive")?,
            sent: field(&map, "sent", "alive")?,
            outbox: field(&map, "outbox", "alive")?,
        })
    }
}

/// Lowercase ASCII letters and digits only, and not empty: a machine's name, as `data/machine.yaml`
/// holds it. No `-`, which separates a session's machine from its number.
pub fn validate_machine_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("the machine name is empty".to_string());
    }
    if !name.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit()) {
        return Err(format!("the machine name {name:?} may hold only lowercase letters and digits"));
    }
    Ok(())
}

/// What an arena says of itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Hello {
    pub machine: String,
    pub workers: usize,
    pub commit: String,
}

impl Hello {
    pub fn text(&self) -> String {
        format!("machine\t{}\nworkers\t{}\ncommit\t{}\n", self.machine, self.workers, self.commit)
    }

    pub fn parse(text: &str) -> Result<Hello, String> {
        let map = fields(text);
        Ok(Hello { machine: field(&map, "machine", "hello")?, workers: field(&map, "workers", "hello")?, commit: field(&map, "commit", "hello")? })
    }
}

/// The smithy's answer to a hello: the arena's session, the token every later request carries, and
/// the store, the remote root (`remote::this_root`) its chores' networks are read from: Drive's.
#[derive(Debug, Clone, PartialEq)]
pub struct HelloReply {
    pub session: String,
    pub token: String,
    pub store: String,
}

impl HelloReply {
    pub fn text(&self) -> String {
        format!("session\t{}\ntoken\t{}\nstore\t{}\n", self.session, self.token, self.store)
    }

    pub fn parse(text: &str) -> Result<HelloReply, String> {
        let map = fields(text);
        Ok(HelloReply {
            session: field(&map, "session", "hello reply")?,
            token: field(&map, "token", "hello reply")?,
            store: field(&map, "store", "hello reply")?,
        })
    }
}

/// What a chore is, and so what one task of it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind {
    /// One game, its one player against itself: self-play. Both sides search with one transposition
    /// table, the same player's, where every other kind gives each side its own.
    SelfPlay,
    /// One opening played with both colours, the challenger White in the first game: a gate's.
    Pair,
    /// Pairs, as `Pair`'s, of several matchups, each entry one with its quota: a round robin's. The
    /// arena plays the entry with the most tasks left, so the matchups fill together, and a top-up
    /// raises an entry's quota rather than adding one beside it.
    Tournament,
    /// Pairs, as `Pair`'s, of one network at two parameter sets, each entry one `k` told apart by its
    /// tag: a tune's.
    Spsa,
    /// One book move searched, no game played: each entry one move of an opening book, its
    /// position after the move in its `SCORE_FEN_TAG`, searched to the chore's depth by its one
    /// player on one thread, with a fresh table and no tablebases, so every arena returns the same
    /// score; sent back as a `ScoreRecord`, not a PGN. A book's scoring.
    Score,
}

impl TaskKind {
    pub fn name(self) -> &'static str {
        match self {
            TaskKind::SelfPlay => "self",
            TaskKind::Pair => "pair",
            TaskKind::Tournament => "tournament",
            TaskKind::Spsa => "spsa",
            TaskKind::Score => "score",
        }
    }

    /// Its games' number, and its entries' players': a score's one player plays no game, but its
    /// entries hold one player as a game's do.
    pub fn games(self) -> usize {
        match self {
            TaskKind::SelfPlay | TaskKind::Score => 1,
            TaskKind::Pair | TaskKind::Tournament | TaskKind::Spsa => 2,
        }
    }

    /// The unit its tasks are counted in, for a status: `game`, `pair` or `move`.
    pub fn task(self) -> &'static str {
        match self {
            TaskKind::SelfPlay => "game",
            TaskKind::Pair | TaskKind::Tournament | TaskKind::Spsa => "pair",
            TaskKind::Score => "move",
        }
    }
}

/// A score entry's tags: the book position's Polyglot key in hex, the book move's Polyglot code, and
/// the position after the move, as FEN, the one the arena searches.
pub const SCORE_PARENT_TAG: &str = "Parent";
pub const SCORE_MOVE_TAG: &str = "BookMove";
pub const SCORE_FEN_TAG: &str = "Position";

/// Each score search's table, fresh, in MB: one figure for the arenas that search and the scores
/// file's header that names it. Large enough that a depth-20 search of an opening position (about a
/// million nodes) rarely replaces an entry it will want again.
pub const SCORE_HASH_MB: usize = 64;

/// Each score search's node cap, beside its depth: a position whose tree explodes stops at the
/// deepest depth it completed within it, the same on every machine, rather than holding its worker
/// for minutes (one bench position took 34 s to depth 18, the rest under 2 s). About 45 s of one
/// thread; named in the scores file's header.
pub const SCORE_MAX_NODES: u64 = 50_000_000;

/// What starts a score task's body after its session line, telling it from a PGN.
pub const SCORE_RECORD_LINE: &str = "score_record\t1";

/// A search's score, from the side that played the book move: centipawns, or a mate in that many
/// moves, negative when that side is mated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookScore {
    Cp(i32),
    Mate(i32),
}

impl BookScore {
    pub fn text(self) -> String {
        match self {
            BookScore::Cp(cp) => format!("cp {cp}"),
            BookScore::Mate(moves) => format!("mate {moves}"),
        }
    }

    pub fn parse(text: &str) -> Result<BookScore, String> {
        let bad = || format!("score: {text:?} is neither `cp <n>` nor `mate <n>`");
        match text.split_once(' ') {
            Some(("cp", cp)) => cp.parse().map(BookScore::Cp).map_err(|_| bad()),
            Some(("mate", moves)) => match moves.parse() {
                Ok(0) | Err(_) => Err(bad()),
                Ok(moves) => Ok(BookScore::Mate(moves)),
            },
            _ => Err(bad()),
        }
    }
}

/// One score task's result: its identity (chore, session, seq, as a game's tags carry them), its
/// entry's three tags back, the score from the side that played the move, the depth searched, the
/// nodes, and what the smithy checks before taking it: the commit of the arena that searched it, and
/// that arena's `bench` node count with the same player (`fattorini::bench::bench_with`, one thread,
/// a fresh table each), identical on every machine whose search is the smithy's own.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoreRecord {
    pub chore: String,
    pub session: String,
    pub seq: u64,
    pub parent: String,
    pub book_move: u16,
    pub fen: String,
    pub score: BookScore,
    pub depth: u32,
    pub nodes: u64,
    pub commit: String,
    pub bench_nodes: u64,
}

impl ScoreRecord {
    pub fn text(&self) -> String {
        format!(
            "{SCORE_RECORD_LINE}\n{CHORE_TAG}\t{}\n{SESSION_TAG}\t{}\n{SEQ_TAG}\t{}\n{SCORE_PARENT_TAG}\t{}\n{SCORE_MOVE_TAG}\t{}\n\
             {SCORE_FEN_TAG}\t{}\nscore\t{}\ndepth\t{}\nnodes\t{}\ncommit\t{}\nbench_nodes\t{}\n",
            self.chore,
            self.session,
            self.seq,
            self.parent,
            self.book_move,
            self.fen,
            self.score.text(),
            self.depth,
            self.nodes,
            self.commit,
            self.bench_nodes
        )
    }

    /// The inverse of `text`; `None` when `text` is no score record at all (a PGN), an error when it
    /// is one, malformed.
    pub fn parse(text: &str) -> Option<Result<ScoreRecord, String>> {
        let rest = text.strip_prefix(SCORE_RECORD_LINE)?;
        let map = fields(rest);
        let what = "score record";
        Some((|| {
            Ok(ScoreRecord {
                chore: field(&map, CHORE_TAG, what)?,
                session: field(&map, SESSION_TAG, what)?,
                seq: field(&map, SEQ_TAG, what)?,
                parent: field(&map, SCORE_PARENT_TAG, what)?,
                book_move: field(&map, SCORE_MOVE_TAG, what)?,
                fen: field(&map, SCORE_FEN_TAG, what)?,
                score: BookScore::parse(map.get("score").ok_or(format!("{what}: no score line"))?)?,
                depth: field(&map, "depth", what)?,
                nodes: field(&map, "nodes", what)?,
                commit: field(&map, "commit", what)?,
                bench_nodes: field(&map, "bench_nodes", what)?,
            })
        })())
    }
}

/// How each move is searched: to a fixed depth, or under a clock in seconds as arena's controls spell
/// it, `base+inc`, `moves/base` or `moves/base+inc`; the clock's text is checked by the arena alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Limit {
    Depth(u32),
    Clock(String),
}

impl Limit {
    pub fn text(&self) -> String {
        match self {
            Limit::Depth(depth) => format!("depth {depth}"),
            Limit::Clock(clock) => format!("clock {clock}"),
        }
    }

    pub fn parse(text: &str) -> Result<Limit, String> {
        match text.split_once(' ') {
            Some(("depth", depth)) => depth.parse().map(Limit::Depth).map_err(|_| format!("limit: {depth:?} is not a depth")),
            Some(("clock", clock)) if !clock.is_empty() && !clock.contains(char::is_whitespace) => Ok(Limit::Clock(clock.to_string())),
            _ => Err(format!("limit: {text:?} is neither `depth <d>` nor `clock <clock>`")),
        }
    }
}

/// A player of a chore: a network by its file's path below the store (the `HelloReply`'s), `data/...`
/// as the mirrored tree names it, one exact file, or `PESTO`; and the search parameters it plays with, `None` for its own (a network's tuning, or the defaults). The smithy
/// writes a tuned player's values in full, so an arena reads no file for them. `time_management`:
/// which one plans its clock (`TimeManagement`), `Split` unless a test says otherwise, written as a
/// third field only when not `Split`, so every chore of before reads the same; an arena of a build
/// before it refuses the field rather than playing the wrong management.
#[derive(Debug, Clone, PartialEq)]
pub struct Player {
    pub network: String,
    pub parameters: Option<SearchParameters>,
    pub time_management: TimeManagement,
}

impl Player {
    pub fn text(&self) -> String {
        let parameters = match &self.parameters {
            None => "own".to_string(),
            Some(parameters) => {
                SearchParameters::NAMES.iter().zip(parameters.values()).map(|(name, value)| format!("{name}={value}")).collect::<Vec<_>>().join(",")
            }
        };
        match self.time_management {
            TimeManagement::Split => format!("{}\t{parameters}", self.network),
            management => format!("{}\t{parameters}\t{}", self.network, management.name()),
        }
    }

    pub fn parse(text: &str) -> Result<Player, String> {
        let (network, rest) = text.split_once('\t').ok_or(format!("player: {text:?} has no parameters"))?;
        if network.is_empty() {
            return Err("player: no network".to_string());
        }
        let (parameters, time_management) = match rest.split_once('\t') {
            None => (rest, TimeManagement::Split),
            Some((parameters, management)) => (
                parameters,
                TimeManagement::parse(management).ok_or(format!("player: {management:?} is no time management"))?,
            ),
        };
        let parameters = if parameters == "own" {
            None
        } else {
            let lines: Vec<String> = parameters.split(',').map(|pair| pair.replacen('=', "\t", 1)).collect();
            Some(SearchParameters::parse_file_text("player", &lines.join("\n"))?)
        };
        Ok(Player { network: network.to_string(), parameters, time_management })
    }
}

/// Tasks alike but for their openings, which the arena draws: one player (a game) or two (a pair,
/// challenger first), `count` tasks, and tags the smithy alone reads, copied into every game.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub players: Vec<Player>,
    pub count: u32,
    pub tags: Vec<(String, String)>,
}

/// A chore: its id, its tasks' kind, its limit, and its entries, played in their order.
#[derive(Debug, Clone, PartialEq)]
pub struct Chore {
    pub id: String,
    pub kind: TaskKind,
    pub limit: Limit,
    /// Whether its games end early by the tablebase, win and draw rules (`game::play_one_game`): a
    /// tune's, never a round robin's or a gate's.
    pub adjudicate: bool,
    /// A tune's alone, under a clock: the smithy's nodes per second with the chore's network, which an
    /// arena scales its clock by against its own (`spsa::clock_factor`), writing the clock it played
    /// in each game's `TimeControl` tag. `None` plays the clock as given.
    pub reference_nps: Option<f64>,
    pub entries: Vec<Entry>,
}

impl Chore {
    pub fn text(&self) -> String {
        let mut text = format!("chore\t{}\nkind\t{}\nlimit\t{}\n", self.id, self.kind.name(), self.limit.text());
        if self.adjudicate {
            text.push_str("adjudicate\t1\n");
        }
        if let Some(nps) = self.reference_nps {
            text.push_str(&format!("reference\t{nps}\n"));
        }
        text.push_str(&entries_text(&self.entries));
        text
    }

    pub fn parse(text: &str) -> Result<Chore, String> {
        let (head, rest) = match text.find("\nentry\t") {
            Some(at) => (&text[..at + 1], &text[at + 1..]),
            None => (text, ""),
        };
        let map = fields(head);
        let kind = match field::<String>(&map, "kind", "chore")?.as_str() {
            "self" => TaskKind::SelfPlay,
            "pair" => TaskKind::Pair,
            "tournament" => TaskKind::Tournament,
            "spsa" => TaskKind::Spsa,
            "score" => TaskKind::Score,
            other => return Err(format!("chore: kind {other:?} is none of self, pair, tournament, spsa and score")),
        };
        let reference_nps = match map.get("reference") {
            None => None,
            Some(_) => Some(field::<f64>(&map, "reference", "chore")?).filter(|nps| nps.is_finite() && *nps > 0.0),
        };
        let chore = Chore {
            id: field(&map, "chore", "chore")?,
            kind,
            limit: Limit::parse(&field::<String>(&map, "limit", "chore")?)?,
            adjudicate: map.contains_key("adjudicate"),
            reference_nps,
            entries: parse_entries(rest)?,
        };
        for entry in &chore.entries {
            if entry.players.len() != kind.games() {
                return Err(format!("chore {}: an entry of {} players for {}s", chore.id, entry.players.len(), kind.name()));
            }
        }
        Ok(chore)
    }

    /// One line saying what the chore is, for a status.
    pub fn summary(&self) -> String {
        let tasks: u64 = self.entries.iter().map(|entry| u64::from(entry.count)).sum();
        format!("chore {}, {} of {} {}s at {}", self.id, self.kind.name(), tasks, self.kind.task(), self.limit.text())
    }
}

/// `entries` as text, each an `entry` line with its count, then its players and tags.
pub fn entries_text(entries: &[Entry]) -> String {
    let mut text = String::new();
    for entry in entries {
        text.push_str(&format!("entry\t{}\n", entry.count));
        for player in &entry.players {
            text.push_str(&format!("player\t{}\n", player.text()));
        }
        for (name, value) in &entry.tags {
            text.push_str(&format!("tag\t{name}\t{value}\n"));
        }
    }
    text
}

/// The inverse of `entries_text`.
pub fn parse_entries(text: &str) -> Result<Vec<Entry>, String> {
    let mut entries: Vec<Entry> = Vec::new();
    for line in text.lines().filter(|line| !line.is_empty()) {
        let (key, value) = line.split_once('\t').ok_or(format!("entries: {line:?} has no value"))?;
        if key == "entry" {
            let count = value.parse().map_err(|_| format!("entries: {value:?} is not a count"))?;
            entries.push(Entry { players: Vec::new(), count, tags: Vec::new() });
            continue;
        }
        let entry = entries.last_mut().ok_or(format!("entries: {line:?} before any entry"))?;
        match key {
            "player" => entry.players.push(Player::parse(value)?),
            "tag" => {
                let (name, value) = value.split_once('\t').ok_or(format!("entries: tag {value:?} has no value"))?;
                if !valid_tag(name, value) {
                    return Err(format!("entries: tag {name:?} {value:?} cannot be written in a PGN"));
                }
                entry.tags.push((name.to_string(), value.to_string()));
            }
            other => return Err(format!("entries: unknown line {other:?}")),
        }
    }
    Ok(entries)
}

/// A tag a PGN can hold as is: a name of letters, digits and `_`, a value with no quote, backslash or
/// line break.
pub fn valid_tag(name: &str, value: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        && !value.contains(['"', '\\', '\n', '\r'])
}

/// A `/chore` or `/task` request's body: the session line, then, for a task, its PGN.
pub fn task_body(session: &str, pgn: &str) -> Vec<u8> {
    format!("{session}\n{pgn}").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str, contents: Option<&str>) -> String {
        let path = std::env::temp_dir().join(format!("fattorini-protocol-test-{name}-{}.yaml", std::process::id())).display().to_string();
        std::fs::remove_file(&path).ok();
        if let Some(contents) = contents {
            std::fs::write(&path, contents).unwrap();
        }
        path
    }

    #[test]
    fn the_machines_are_listed_each_once_highest_rank_first() {
        let how = "`machines: [<name>, ...]`";
        let missing = fixture("missing", None);
        assert!(machines(&missing).unwrap_err().contains(how));
        let listed = fixture("listed", Some("machines: [beta, alpha, gamma]\n"));
        let expected = Machines { machines: vec!["beta".to_string(), "alpha".to_string(), "gamma".to_string()] };
        assert_eq!(machines(&listed).as_ref(), Ok(&expected));
        assert_eq!((expected.rank("beta"), expected.rank("gamma"), expected.rank("delta")), (Some(0), Some(2), None));
        std::fs::remove_file(&listed).ok();
        for (name, contents) in [
            ("bad-name", "machines: [Beta-1]\n"),
            ("old-shape", "smithy: beta\narenas: [beta]\n"),
            ("old-arenas", "arenas: [beta]\n"),
            ("unknown-key", "machines: [beta]\nport: 1\n"),
            ("none", "machines: []\n"),
            ("empty", ""),
        ] {
            let path = fixture(name, Some(contents));
            assert!(machines(&path).unwrap_err().contains(how), "{contents:?}");
            std::fs::remove_file(&path).ok();
        }
        let twice = fixture("twice", Some("machines: [gamma, gamma]\n"));
        assert!(machines(&twice).unwrap_err().contains("gamma twice"));
        std::fs::remove_file(&twice).ok();
    }

    #[test]
    fn a_full_claim_outranks_a_scaled_one_then_the_higher_rank_wins() {
        let claim = |scaled: bool, rank: Option<u32>| Claim {
            smithy: "beta".to_string(),
            host: "beta".to_string(),
            boot: "b".to_string(),
            commit: "c".to_string(),
            scaled,
            rank,
        };
        assert!(claim(false, None).outranks(&claim(true, Some(0))), "full over scaled, whatever the ranks");
        assert!(!claim(true, Some(0)).outranks(&claim(false, None)));
        assert!(claim(false, Some(0)).outranks(&claim(false, Some(1))));
        assert!(!claim(false, Some(1)).outranks(&claim(false, Some(0))));
        assert!(!claim(false, Some(1)).outranks(&claim(false, Some(1))), "an equal keeps the holder");
        assert!(claim(true, Some(3)).outranks(&claim(true, None)), "ranked over unranked");
        assert!(!claim(true, None).outranks(&claim(true, None)), "two unranked: first come keeps");
    }

    #[test]
    fn a_claim_and_an_alive_read_back() {
        let mut claim = Claim {
            smithy: "beta".to_string(),
            host: "beta".to_string(),
            boot: "b00t".to_string(),
            commit: "abc (modified)".to_string(),
            scaled: true,
            rank: Some(2),
        };
        assert_eq!(Claim::parse(&claim.text()), Ok(claim.clone()));
        (claim.scaled, claim.rank) = (false, None);
        assert_eq!(Claim::parse(&claim.text()), Ok(claim));
        let alive = Alive {
            session: "gamma-0".to_string(),
            chore: "c000003".to_string(),
            in_flight: 4,
            sent: 17,
            outbox: 1,
        };
        assert_eq!(Alive::parse(&alive.text()), Ok(alive));
        assert!(Claim::parse("smithy\tbeta\n").is_err());
    }

    #[test]
    fn hello_and_its_reply_read_back() {
        let hello = Hello { machine: "alpha".to_string(), workers: 8, commit: "abc (modified)".to_string() };
        assert_eq!(Hello::parse(&hello.text()), Ok(hello));
        let reply = HelloReply { session: "alpha-3".to_string(), token: "t0k3n".to_string(), store: "G:/My Drive/fattorini".to_string() };
        assert_eq!(HelloReply::parse(&reply.text()), Ok(reply));
    }

    #[test]
    fn every_chore_reads_back() {
        let own = |network: &str| Player { network: network.to_string(), parameters: None, time_management: TimeManagement::Split };
        let tuned = SearchParameters { razoring_margin: -7, ..SearchParameters::DEFAULT };
        let chores = [
            Chore {
                id: "c000001".to_string(),
                kind: TaskKind::SelfPlay,
                limit: Limit::Depth(9),
                adjudicate: false,
                reference_nps: None,
                entries: vec![Entry { players: vec![own(PESTO)], count: 32, tags: Vec::new() }],
            },
            Chore {
                id: "c000002".to_string(),
                kind: TaskKind::Pair,
                limit: Limit::Clock("10+0.1".to_string()),
                adjudicate: true,
                reference_nps: Some(1_234_567.5),
                entries: vec![
                    Entry {
                        players: vec![
                            Player { network: "aa".to_string(), parameters: Some(tuned), time_management: TimeManagement::Split },
                            Player { network: "bb".to_string(), parameters: None, time_management: TimeManagement::Reckless },
                        ],
                        count: 1,
                        tags: vec![("K".to_string(), "17".to_string())],
                    },
                    Entry { players: vec![own("aa"), own(PESTO)], count: 4, tags: vec![("Challenger".to_string(), "0.93".to_string())] },
                ],
            },
            Chore {
                id: "c000003".to_string(),
                kind: TaskKind::Tournament,
                limit: Limit::Depth(9),
                adjudicate: false,
                reference_nps: None,
                entries: vec![
                    Entry { players: vec![own("aa"), own("bb")], count: 3, tags: Vec::new() },
                    Entry { players: vec![own("aa"), own("cc")], count: 2, tags: Vec::new() },
                ],
            },
            Chore {
                id: "c000004".to_string(),
                kind: TaskKind::Spsa,
                limit: Limit::Clock("10+0.1".to_string()),
                adjudicate: true,
                reference_nps: None,
                entries: vec![Entry {
                    players: vec![
                        Player { network: "aa".to_string(), parameters: Some(tuned), time_management: TimeManagement::Classic },
                        own("aa"),
                    ],
                    count: 1,
                    tags: vec![("K".to_string(), "3".to_string())],
                }],
            },
        ];
        for chore in chores {
            assert_eq!(Chore::parse(&chore.text()), Ok(chore.clone()));
            assert_eq!(parse_entries(&entries_text(&chore.entries)), Ok(chore.entries));
        }
    }

    #[test]
    fn a_score_chore_and_its_record_read_back() {
        let chore = Chore {
            id: "c000005".to_string(),
            kind: TaskKind::Score,
            limit: Limit::Depth(20),
            adjudicate: false,
            reference_nps: None,
            entries: vec![Entry {
                players: vec![Player { network: "aa".to_string(), parameters: None, time_management: TimeManagement::Split }],
                count: 1,
                tags: vec![
                    (SCORE_PARENT_TAG.to_string(), "463b96181691fc9c".to_string()),
                    (SCORE_MOVE_TAG.to_string(), "796".to_string()),
                    (SCORE_FEN_TAG.to_string(), "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1".to_string()),
                ],
            }],
        };
        assert_eq!(Chore::parse(&chore.text()), Ok(chore));
        let mut record = ScoreRecord {
            chore: "c000005".to_string(),
            session: "alpha-1".to_string(),
            seq: 7,
            parent: "463b96181691fc9c".to_string(),
            book_move: 796,
            fen: "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1".to_string(),
            score: BookScore::Cp(-23),
            depth: 20,
            nodes: 1_234_567,
            commit: "abc".to_string(),
            bench_nodes: 2_345_678,
        };
        assert_eq!(ScoreRecord::parse(&record.text()), Some(Ok(record.clone())));
        record.score = BookScore::Mate(-3);
        assert_eq!(ScoreRecord::parse(&record.text()), Some(Ok(record.clone())));
        assert_eq!(ScoreRecord::parse("[Event \"e\"]\n"), None, "a PGN is no score record");
        assert!(ScoreRecord::parse(&format!("{SCORE_RECORD_LINE}\nChore\tc1\n")).unwrap().is_err());
        for bad in ["cp", "cp x", "mate 0", "pawns 3", ""] {
            assert!(BookScore::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_player_names_its_time_management_only_when_not_split() {
        let player = |time_management| Player { network: "aa".to_string(), parameters: None, time_management };
        assert_eq!(player(TimeManagement::Split).text(), "aa\town", "as every chore before it");
        assert_eq!(player(TimeManagement::Reckless).text(), "aa\town\tReckless");
        assert_eq!(Player::parse("aa\town"), Ok(player(TimeManagement::Split)));
        assert_eq!(Player::parse("aa\town\treckless"), Ok(player(TimeManagement::Reckless)), "in any case");
    }

    #[test]
    fn a_malformed_chore_is_refused() {
        let head = "chore\tc1\nkind\tpair\nlimit\tdepth 9\n";
        assert!(Chore::parse(&format!("{head}entry\t1\nplayer\taa\town\n")).is_err(), "a pair of one player");
        assert!(Chore::parse("chore\tc1\nkind\ttrio\nlimit\tdepth 9\n").is_err());
        assert!(Chore::parse("chore\tc1\nkind\tself\nlimit\tnodes 9\n").is_err());
        assert!(parse_entries("player\taa\town\n").is_err(), "a player before any entry");
        assert!(parse_entries("entry\t1\nplayer\taa\trazoring_margin=1\n").is_err(), "parameters are all given, or none");
        assert!(parse_entries("entry\t1\nplayer\taa\town\tSlow\n").is_err(), "an unknown time management");
        assert!(parse_entries("entry\t1\ntag\tK\ta\"b\n").is_err(), "a value no PGN tag can hold");
    }
}
