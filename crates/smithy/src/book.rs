//! The smithy's book job: a Polyglot-style opening book from a PGN database of strong games: every
//! move played at least N times from its position, weighed by the points it scored. Each source has
//! its own folder on the remote, `data/chess/books/<source>/`, holding its PGN, `<source>.pgn` or `<source>.pgn.zst`, kept on
//! Drive for its backup and streamed from there. The job runs as the smithy's child
//! (`CHILD_COMMAND`), the stages it still lacks in order, each stored for the next:
//!
//! - `fit`: measures how the number of games a book move needs should grow with the database's
//!   size (`alpha`, below), as `fit.txt`.
//! - `graph`: the positions reached by at least N games, and the moves between them, as
//!   `graph.txt`. It depends on no network.
//! - `write`, at `points` or `counts` weights: the graph's moves played at least N times (the
//!   graph's own N), as the Polyglot book `book-n<N>-<weights>.bin` and its record
//!   `book-n<N>-<weights>.txt`. No min-score
//!   and no search: Kalo Chess's search, tried as a correction of the masters' shares, moved them by about
//!   3 cp at the root, not worth a network and an hour per book.
//!
//! **N from the database's size.** `N = max(3, ceil(10 * (G / 1,000,000)^alpha))`, G the games read.
//! The anchor, 10 games at a million, is a fixed choice of how much a book move must weigh. `alpha` is
//! data, measured per source by `fit`: a growing database is also more diverse, so a theory
//! position's share of it falls, and a linear N (`alpha = 1`) would slowly squeeze theory out. `fit`
//! builds the graphs of date prefixes of the source (where its games reach 40%, 55% and 70%)
//! and of all of it, and finds for each prefix the N at which its book is as large as the full one's
//! at the anchor's N; `alpha` is the slope of `ln N` against `ln G`. Prefixes by date, not random
//! samples: a random half has the whole's diversity, so its counts halve exactly and any `alpha` near
//! 1 would fit by construction. On Lumbra's Elite file (863,790 games) it came out near 0.83; a
//! square root (0.5) let the book grow by 40% over 2.5 times the games.
//!
//! **The graph.** A position is identified by its Polyglot key, so transpositions pool. Its support
//! is the number of games reaching it at least once: a game repeating a position counts once. The
//! nodes are the positions with at least N games; an edge is a move some game played
//! from one node to another. An edge returning to a position earlier in the same game is dropped,
//! and so is any edge that still closes a cycle across games (found by a depth-first walk from the
//! initial position, moves in order, so the result does not depend on hashing): the graph is a DAG.
//! Only what that walk reaches is kept: a position can reach N games by
//! transposition while each of its parents stays below N, and nothing leads to it.
//!
//! **Two passes over the file.** Which positions reach N is known only once every game is counted,
//! so the first pass counts and the second collects the edges between the positions that made it.
//! Plies are counted up to `DEFAULT_MAX_PLIES`, a bound on the first pass's memory rather than where the
//! book is expected to end; the record says how many positions at the cap still reached N, which is
//! 0 when the cap cut nothing.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs::File;
use std::hash::{BuildHasherDefault, Hasher};
use std::io::{BufWriter, Read, Write};
use std::ops::ControlFlow;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use pgn_reader::{RawTag, Reader, SanPlus, Visitor};
use serde::{Deserialize, Serialize};

use kalo_chess::board::chess_move::Move;
use kalo_chess::board::Board;
use kalo_chess::polyglot::{book_bytes, polyglot_key, polyglot_move, scaled_weights, Entry};

use pgn_replay::{try_resolve_san, STARTING_POSITION_FEN};
use super::remote;

/// The anchor of the rule for N: this many games at `ANCHOR_GAMES` games read.
const ANCHOR_N: f64 = 10.0;
const ANCHOR_GAMES: f64 = 1_000_000.0;
/// No N below this, however small the database: a book move is never one or two games.
const MIN_N: u32 = 3;
/// The N `fit` builds its graphs at: every book size from it upwards can then be read off one graph.
const FIT_GRAPH_N: u32 = MIN_N;
/// Where `fit` cuts its date prefixes, as shares of the source's games: the dates come from the
/// data, since a source's span cannot be known in advance.
const FIT_SHARES: [f64; 3] = [0.40, 0.55, 0.70];

/// A source's folder, under `data/` locally and on the remote alike.
fn source_dir(source: &str) -> String {
    format!("data/chess/books/{source}")
}

fn graph_path(source: &str) -> String {
    format!("{}/graph.txt", source_dir(source))
}

fn fit_path(source: &str) -> String {
    format!("{}/fit.txt", source_dir(source))
}

/// A source's PGN in its folder, as the remote holds it: `<source>.pgn.zst` when there is one, else
/// `<source>.pgn`. On Drive, for its backup; streamed from there, being read rarely.
fn pgn_path(source: &str) -> Result<String, String> {
    let candidates = ["pgn.zst", "pgn"].map(|extension| format!("{}/{source}.{extension}", source_dir(source)));
    candidates
        .iter()
        .find(|path| std::path::Path::new(&remote::remote_path(path)).is_file())
        .cloned()
        .ok_or_else(|| format!("no {} nor {} on the remote", candidates[0], candidates[1]))
}

/// A source's name is its folder's: one path component, with no separator to climb out of
/// `data/chess/books/`.
pub(crate) fn check_source(source: &str) -> Result<(), String> {
    let plain = !source.is_empty() && source != "." && source != ".." && !source.contains(['/', '\\', ':']);
    if plain { Ok(()) } else { Err(format!("{source:?} is not a source's name, a folder of data/chess/books/")) }
}

/// The rule for N: `max(MIN_N, ceil(ANCHOR_N * (games / ANCHOR_GAMES)^alpha))`.
fn n_for(games: u64, alpha: f64) -> u32 {
    let n = (ANCHOR_N * (games as f64 / ANCHOR_GAMES).powf(alpha)).ceil() as u32;
    n.max(MIN_N)
}
/// 60, not 40: on the Elite file at N = 20, 40 left 174 positions at the cap still reaching N, and
/// 72 leaves at ply 40 that were leaves only because the counting stopped there; 60 leaves 4.
const DEFAULT_MAX_PLIES: usize = 60;
/// The largest window zstd allows: a source's compression level is not ours to know.
const ZSTD_WINDOW_LOG_MAX: u32 = 31;
/// Longer than any comment a master game carries, so pgn-reader never splits one.
const SUPPORTED_COMMENT_BYTES: usize = 1 << 16;

/// The smithy's own subcommand the book job runs this as: no command for anyone to type, so it is in
/// no usage line.
pub(crate) const CHILD_COMMAND: &str = "book-make";

/// What starts a line of the child's that reports its pass, not one to print: the rest is a
/// `BookLive` as JSON.
pub(crate) const PROGRESS_LINE: &str = "@progress ";

/// How often the child reports its pass.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// What the book job is asked for: a source, and the weights of the book it writes.
#[derive(Debug, PartialEq)]
struct MakeArgs {
    source: String,
    weights: Weights,
}

/// `<source>`, then `--weights points|counts` (`points` when absent), at most once.
fn parse_make_args(args: &[String]) -> Result<MakeArgs, String> {
    let Some((source, rest)) = args.split_first() else {
        return Err(format!("{CHILD_COMMAND} takes a source's name first"));
    };
    check_source(source)?;
    let mut weights = None;
    for pair in rest.chunks(2) {
        let [flag, value] = pair else {
            return Err(format!("{} takes a value", pair[0]));
        };
        let repeated = match flag.as_str() {
            "--weights" => {
                let w = match value.as_str() {
                    "points" => Weights::Points,
                    "counts" => Weights::Counts,
                    _ => return Err(format!("--weights is points or counts, got {value:?}")),
                };
                weights.replace(w).is_some()
            }
            other => return Err(format!("unknown flag {other:?}")),
        };
        if repeated {
            return Err(format!("{flag} given twice"));
        }
    }
    Ok(MakeArgs { source: source.clone(), weights: weights.unwrap_or(Weights::Points) })
}

/// A source as the page's Books screen lists it: its PGN's file name, and how far its stages have
/// gone on the remote: its fit's `alpha`, its graph's N and games, and every book written of it.
#[derive(Debug, PartialEq, Serialize)]
pub(crate) struct SourceView {
    name: String,
    /// Its PGN's file name, `None` when the remote no longer holds it: its books stay, but
    /// no stage can run again.
    pgn: Option<String>,
    alpha: Option<f64>,
    graph: Option<GraphView>,
    books: Vec<WrittenView>,
}

#[derive(Debug, PartialEq, Serialize)]
struct GraphView {
    n: u32,
    games: u64,
}

/// A book whose record is on the remote, so whole (`run_write` moves the record last).
#[derive(Debug, PartialEq, Serialize)]
struct WrittenView {
    n: u32,
    weights: String,
    /// Its `.bin` in the remote's tree, as an install names it.
    path: String,
}

/// Every source on the remote, by name: each folder of `data/chess/books/` holding its PGN. None when the
/// remote cannot be read.
pub(crate) fn sources() -> Vec<SourceView> {
    let Ok(entries) = std::fs::read_dir(remote::remote_path("data/chess/books")) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| check_source(name).is_ok())
        .collect();
    names.sort();
    names.into_iter().filter_map(|name| source_view(&name)).collect()
}

/// `source`'s view, `None` when its folder holds neither its PGN nor a book written of it.
fn source_view(source: &str) -> Option<SourceView> {
    let pgn = pgn_path(source).ok().map(|pgn| pgn.rsplit('/').next().unwrap_or(&pgn).to_string());
    let graph = graph_header(source).and_then(|header| {
        let value = |name: &str| header.iter().find(|(key, _)| key == name).map(|(_, value)| value.clone());
        Some(GraphView { n: value("min_games")?.parse().ok()?, games: value("games_used")?.parse().ok()? })
    });
    let mut books: Vec<WrittenView> = std::fs::read_dir(remote::remote_path(&source_dir(source)))
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter_map(|file| written_book(source, &file))
        .collect();
    books.sort_by(|a, b| (a.n, &a.weights).cmp(&(b.n, &b.weights)));
    if pgn.is_none() && books.is_empty() {
        return None;
    }
    Some(SourceView { name: source.to_string(), pgn, alpha: read_alpha(source).ok(), graph, books })
}

/// The book whose record `file` is, in `source`'s folder: `book-n<N>-<weights>.txt`.
fn written_book(source: &str, file: &str) -> Option<WrittenView> {
    let stem = file.strip_prefix("book-n")?.strip_suffix(".txt")?;
    let (n, weights) = stem.split_once('-')?;
    matches!(weights, "points" | "counts").then_some(())?;
    Some(WrittenView {
        n: n.parse().ok()?,
        weights: weights.to_string(),
        path: format!("{}/book-n{n}-{weights}.bin", source_dir(source)),
    })
}

/// The header of `source`'s `graph.txt` on the remote, its lines before the first node: the file is
/// large, and the page's catalog is read often.
fn graph_header(source: &str) -> Option<Vec<(String, String)>> {
    use std::io::BufRead;
    let file = File::open(remote::remote_path(&graph_path(source))).ok()?;
    let header = std::io::BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .take_while(|line| !line.starts_with("node\t") && !line.starts_with("edge\t"))
        .filter_map(|line| line.split_once('\t').map(|(name, value)| (name.to_string(), value.to_string())))
        .collect();
    Some(header)
}

/// The book job, as the smithy's child: the stages `source` still lacks on the remote, in order,
/// `fit`, `graph`, then `write` at `--weights`. A stage whose output is there is skipped, so a job
/// stopped part-way goes on from the stage it was in; redoing one is deleting its file. Its output
/// is a pipe the smithy logs, so it draws no bars, only lines, and reports its pass as
/// `PROGRESS_LINE`s.
pub(crate) fn run(rest: &[String]) {
    let args = parse_make_args(rest).unwrap_or_else(|message| fail(message));
    if let Err(error) = remote::check_mounted() {
        fail(error.to_string());
    }
    let source = &args.source;
    let pgn = pgn_path(source).unwrap_or_else(|message| fail(format!("{source}: {message}")));
    std::thread::spawn(|| loop {
        if let Some(Ok(json)) = pass_live().map(|live| serde_json::to_string(&live)) {
            println!("{PROGRESS_LINE}{json}");
        }
        std::thread::sleep(PROGRESS_EVERY);
    });
    let on_remote = |path: &str| std::path::Path::new(&remote::remote_path(path)).is_file();
    if on_remote(&fit_path(source)) {
        println!("fit: done before, {} kept", fit_path(source));
    } else {
        run_fit(source, &pgn);
    }
    if on_remote(&graph_path(source)) {
        println!("graph: done before, {} kept", graph_path(source));
    } else {
        run_graph(source, &pgn);
    }
    run_write(source, args.weights);
}

/// One pass's place, as the page shows it: `done` of `total` bytes of the PGN, `rate` in bytes a
/// second and `eta_seconds` at it, both `None` before any progress. `pass` names the stage and the
/// pass (`graph: counting`, `fit: edges to 2010.06.30`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct BookLive {
    pass: String,
    done: u64,
    total: u64,
    rate: Option<f64>,
    eta_seconds: Option<f64>,
}

/// The child's last report, in the smithy: for its page's `state.json`.
static KEPT: Mutex<Option<BookLive>> = Mutex::new(None);

/// Keeps `live` as the child's last report, `None` once the child has ended.
pub(crate) fn keep(live: Option<BookLive>) {
    *KEPT.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = live;
}

/// The child's last report, if one runs.
pub(crate) fn kept() -> Option<BookLive> {
    KEPT.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
}

/// The pass under way: its name, its bytes read of its total, and when it started.
struct Pass {
    name: String,
    done: u64,
    total: u64,
    started: Instant,
}

static PASS: Mutex<Option<Pass>> = Mutex::new(None);

fn pass() -> std::sync::MutexGuard<'static, Option<Pass>> {
    PASS.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The pass under way as the page words it; `None` between passes.
fn pass_live() -> Option<BookLive> {
    let pass = pass();
    let pass = pass.as_ref()?;
    let seconds = pass.started.elapsed().as_secs_f64();
    let rate = (pass.done > 0 && seconds > 0.0).then(|| pass.done as f64 / seconds);
    let eta_seconds = rate.map(|rate| pass.total.saturating_sub(pass.done) as f64 / rate);
    Some(BookLive { pass: pass.name.clone(), done: pass.done, total: pass.total, rate, eta_seconds })
}

/// The PGN's bytes as a pass reads them, counted on `PASS`.
struct Counted<R> {
    inner: R,
}

impl<R: Read> Read for Counted<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buf)?;
        if let Some(pass) = pass().as_mut() {
            pass.done += read as u64;
        }
        Ok(read)
    }
}

/// The `alpha` of `source`'s `fit.txt` on the remote.
fn read_alpha(source: &str) -> Result<f64, String> {
    let path = remote::remote_path(&fit_path(source));
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {path}: {error}"))?;
    let value = text
        .lines()
        .find_map(|line| line.strip_prefix("alpha\t"))
        .ok_or_else(|| format!("{path} has no alpha line"))?;
    value.trim().parse().map_err(|_| format!("{path}: alpha is not a number: {value:?}"))
}

/// The graph stage: both passes over the source's PGN (`pgn`, its path on the remote's tree), then
/// its `graph.txt`, moved to the remote. N is the rule's, from the games read and the source's
/// `alpha`.
fn run_graph(source: &str, pgn: &str) {
    let alpha = read_alpha(source).unwrap_or_else(|message| fail(message));
    let start = Instant::now();
    let mut pass = 0;
    let built = build_graph(
        |on_game: &mut dyn FnMut(&PgnGame)| {
            pass += 1;
            let label = if pass == 1 { "graph: counting" } else { "graph: edges" };
            read_pgn(pgn, label, |game| on_game(game));
        },
        DEFAULT_MAX_PLIES,
        None,
        |games| n_for(games, alpha),
    );
    let graph = &built.graph;

    let plies = node_plies(graph);
    let leaves: Vec<u64> = graph.nodes.keys().copied().filter(|key| graph.is_leaf(*key)).collect();
    let mut leaves_by_ply: BTreeMap<u32, usize> = BTreeMap::new();
    for key in &leaves {
        *leaves_by_ply.entry(plies[key]).or_insert(0) += 1;
    }
    let edges: usize = graph.children.values().map(Vec::len).sum();
    let tally = built.tally;
    let header = [
        ("source", source.to_string()),
        ("pgn", pgn.to_string()),
        ("alpha", alpha.to_string()),
        ("min_games", built.min_games.to_string()),
        ("max_plies", DEFAULT_MAX_PLIES.to_string()),
        ("until", "none".to_string()),
        ("games", tally.games.to_string()),
        ("games_used", built.games_used.to_string()),
        ("games_after_until", tally.after_until.to_string()),
        ("games_without_date", tally.no_date.to_string()),
        ("games_from_fen", tally.from_fen.to_string()),
        ("games_unreadable", tally.unreadable.to_string()),
        ("games_without_result", tally.no_result.to_string()),
        ("positions_at_cap", built.at_cap.to_string()),
        ("cycle_edges_dropped", graph.cycle_edges.to_string()),
        ("nodes", graph.nodes.len().to_string()),
        ("edges", edges.to_string()),
        ("leaves", leaves.len().to_string()),
        ("built", current_time()),
    ];
    let path = graph_path(source);
    create_parent(&path);
    write_atomically(&path, graph_text(graph, &plies, &header).as_bytes());
    for (name, value) in &header {
        println!("{name}\t{value}");
    }
    println!("leaves by ply:");
    for (ply, count) in &leaves_by_ply {
        println!("  {ply}\t{count}");
    }
    println!("{:.0} s", start.elapsed().as_secs_f64());
    if let Err(error) = remote::move_up_replacing(&path) {
        fail(format!("{error}. The graph stays in {path}; make the book again to upload it"));
    }
    println!("moved the graph to {}", remote::remote_path(&path));
}

/// Creates the folder `path` goes in, if it has one.
fn create_parent(path: &str) {
    if let Some(parent) = std::path::Path::new(path).parent().filter(|parent| !parent.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).unwrap_or_else(|error| panic!("failed to create {}: {error}", parent.display()));
    }
}

/// Every game of the source's PGN, `path` in the remote's tree (`data/chess/books/...`), zstd-compressed
/// when its name ends in `.zst` and plain otherwise, in file order, streamed from the remote as pass
/// `label`. The pass counts the file's bytes: the number of games is not known before it ends.
fn read_pgn(path: &str, label: &str, on_game: impl FnMut(&PgnGame)) {
    let remote = remote::remote_path(path);
    let file = File::open(&remote).unwrap_or_else(|error| fail(format!("failed to open {remote}: {error}")));
    let length = file.metadata().unwrap_or_else(|error| fail(format!("failed to read the size of {remote}: {error}"))).len();
    *pass() = Some(Pass { name: label.to_string(), done: 0, total: length, started: Instant::now() });
    let input = Counted { inner: file };
    let read = if path.ends_with(".zst") {
        let mut decoder =
            zstd::Decoder::new(input).unwrap_or_else(|error| panic!("failed to open a zstd decoder for {path}: {error}"));
        decoder
            .window_log_max(ZSTD_WINDOW_LOG_MAX)
            .unwrap_or_else(|error| panic!("failed to allow a 2^{ZSTD_WINDOW_LOG_MAX} window for {path}: {error}"));
        for_each_game(decoder, on_game)
    } else {
        for_each_game(std::io::BufReader::new(input), on_game)
    };
    *pass() = None;
    read.unwrap_or_else(|error| fail(format!("failed to read {remote}: {error}")));
}

/// Every game of the PGN `input`, in order.
fn for_each_game<R: Read>(input: R, mut on_game: impl FnMut(&PgnGame)) -> Result<(), String> {
    let mut reader = Reader::build(input).set_supported_comment_length(SUPPORTED_COMMENT_BYTES).finish();
    let mut visitor = GameReader { game: PgnGame::default() };
    loop {
        match reader.read_game(&mut visitor) {
            Ok(Some(game)) => on_game(&game),
            Ok(None) => return Ok(()),
            Err(error) => return Err(error.to_string()),
        }
    }
}

/// One game, as much as the graph needs of it.
#[derive(Default)]
struct PgnGame {
    moves: Vec<SanPlus>,
    has_fen: bool,
    /// `None` for `*` or a missing or unreadable `[Result]`.
    result: Option<GameResult>,
    /// `[Date]` as the number `YYYYMMDD`, so dates compare as numbers; `None` unless the tag is a
    /// full `YYYY.MM.DD` in digits. A partial date (`2005.??.??`) cannot be placed between two full
    /// ones, and appears in historical databases only.
    date: Option<u32>,
}

/// `YYYY.MM.DD`, all digits, as `YYYYMMDD`; anything else is `None`.
fn parse_date(text: &[u8]) -> Option<u32> {
    let [y1, y2, y3, y4, b'.', m1, m2, b'.', d1, d2] = *text else {
        return None;
    };
    let digits = [y1, y2, y3, y4, m1, m2, d1, d2];
    if !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(digits.iter().fold(0, |date, &digit| date * 10 + (digit - b'0') as u32))
}

/// `YYYYMMDD` as `YYYY.MM.DD`.
fn show_date(date: u32) -> String {
    format!("{:04}.{:02}.{:02}", date / 10000, date / 100 % 100, date % 100)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GameResult {
    WhiteWins,
    Draw,
    BlackWins,
}

impl GameResult {
    /// The index into an edge's `[W, D, L]` for the side that played a move: White for even plies
    /// from the initial position (the ply before the move), Black for odd ones.
    fn index_for_mover(self, ply: usize) -> usize {
        let white_moved = ply.is_multiple_of(2);
        match (self, white_moved) {
            (GameResult::Draw, _) => 1,
            (GameResult::WhiteWins, true) | (GameResult::BlackWins, false) => 0,
            (GameResult::WhiteWins, false) | (GameResult::BlackWins, true) => 2,
        }
    }
}

struct GameReader {
    game: PgnGame,
}

impl Visitor for GameReader {
    type Tags = ();
    type Movetext = ();
    type Output = PgnGame;

    fn begin_tags(&mut self) -> ControlFlow<Self::Output, Self::Tags> {
        self.game = PgnGame::default();
        ControlFlow::Continue(())
    }

    fn tag(&mut self, _tags: &mut Self::Tags, name: &[u8], value: RawTag<'_>) -> ControlFlow<Self::Output> {
        if name == b"FEN" {
            self.game.has_fen = true;
        } else if name == b"Result" {
            self.game.result = match value.as_bytes() {
                b"1-0" => Some(GameResult::WhiteWins),
                b"0-1" => Some(GameResult::BlackWins),
                b"1/2-1/2" => Some(GameResult::Draw),
                _ => None,
            };
        } else if name == b"Date" {
            self.game.date = parse_date(value.as_bytes());
        }
        ControlFlow::Continue(())
    }

    fn begin_movetext(&mut self, _tags: Self::Tags) -> ControlFlow<Self::Output, Self::Movetext> {
        ControlFlow::Continue(())
    }

    fn san(&mut self, _movetext: &mut Self::Movetext, san_plus: SanPlus) -> ControlFlow<Self::Output> {
        self.game.moves.push(san_plus);
        ControlFlow::Continue(())
    }

    fn end_game(&mut self, _movetext: Self::Movetext) -> Self::Output {
        std::mem::take(&mut self.game)
    }
}

/// An edge's games by result, from the side that played the move: wins, draws, losses.
type Results = [u32; 3];

/// Polyglot keys are uniformly random already, so a key is its own hash: hashing tens of millions of
/// them through SipHash would only cost time.
#[derive(Default)]
struct KeyHasher(u64);

impl Hasher for KeyHasher {
    fn write(&mut self, _bytes: &[u8]) {
        unreachable!("only u64 keys are hashed");
    }

    fn write_u64(&mut self, key: u64) {
        self.0 = key;
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

type KeyMap<V> = HashMap<u64, V, BuildHasherDefault<KeyHasher>>;
type KeySet = HashSet<u64, BuildHasherDefault<KeyHasher>>;

fn initial_board() -> Board {
    let (board, _) = Board::from_fen(STARTING_POSITION_FEN).expect("the starting position is a valid FEN");
    board
}

/// The moves of `game` the graph reads: at most `max_plies`, up to the first that names no legal
/// move. The flag is false when one did.
fn resolved_moves(game: &PgnGame, max_plies: usize) -> (Vec<Move>, bool) {
    let mut board = initial_board();
    let mut moves = Vec::with_capacity(game.moves.len().min(max_plies));
    for san_plus in game.moves.iter().take(max_plies) {
        let Some(mv) = try_resolve_san(&board, &san_plus.san) else {
            return (moves, false);
        };
        board.make_move(mv);
        moves.push(mv);
    }
    (moves, true)
}

/// How the games went, for the record. Counted in the first pass.
#[derive(Default, Clone, Copy, Debug)]
struct Tally {
    games: u64,
    from_fen: u64,
    unreadable: u64,
    /// Games without a result: their moves shape the graph, but count in no edge's results.
    no_result: u64,
    /// With `--until`: games played after it, and games without a full date. Both skipped.
    after_until: u64,
    no_date: u64,
}

/// Whether `game` is outside `until`'s prefix of the database (a `YYYYMMDD`), and so skipped in both
/// passes: played after it, or without a full date, which cannot be placed.
fn outside_prefix(game: &PgnGame, until: Option<u32>) -> Option<DateExclusion> {
    let until = until?;
    match game.date {
        None => Some(DateExclusion::NoDate),
        Some(date) if date > until => Some(DateExclusion::AfterUntil),
        Some(_) => None,
    }
}

enum DateExclusion {
    AfterUntil,
    NoDate,
}

/// The first pass: each position's number of games, and the positions some game reached at exactly
/// the cap.
#[derive(Default)]
struct Support {
    games: KeyMap<u32>,
    at_cap: KeySet,
}

/// Adds one game's positions to `support`, each once however often the game reached it.
fn add_support(moves: &[Move], max_plies: usize, support: &mut Support) {
    let mut board = initial_board();
    let mut keys = Vec::with_capacity(moves.len() + 1);
    keys.push(polyglot_key(&board));
    for &mv in moves {
        board.make_move(mv);
        keys.push(polyglot_key(&board));
    }
    if moves.len() == max_plies {
        support.at_cap.insert(*keys.last().expect("the initial position is always there"));
    }
    keys.sort_unstable();
    keys.dedup();
    for key in keys {
        *support.games.entry(key).or_insert(0) += 1;
    }
}

/// A node: its number of games, and its position as first met, for the leaf searches.
#[derive(Debug, Clone, Copy)]
struct Node {
    games: u32,
    board: Board,
}

/// The second pass, before pruning: the nodes met, and each edge `(parent, move)` with its child and
/// its games' results.
#[derive(Default)]
struct RawGraph {
    nodes: KeyMap<Node>,
    edges: HashMap<(u64, u16), (u64, Results)>,
}

/// Adds one game's nodes and edges to `graph`, and its result to each edge from the side that played
/// the move. An edge whose child the game had already reached is a repetition and closes a cycle:
/// dropped, so a game counts at most once per edge. A game without a result adds its edges with no
/// result.
fn add_edges(moves: &[Move], result: Option<GameResult>, kept: &KeyMap<u32>, graph: &mut RawGraph) {
    let mut board = initial_board();
    let mut key = polyglot_key(&board);
    note_node(key, &board, kept, graph);
    let mut seen = vec![key];
    for (ply, &mv) in moves.iter().enumerate() {
        let parent = key;
        board.make_move(mv);
        key = polyglot_key(&board);
        let repeated = seen.contains(&key);
        seen.push(key);
        note_node(key, &board, kept, graph);
        if repeated || !kept.contains_key(&parent) || !kept.contains_key(&key) {
            continue;
        }
        let (_, results) = graph.edges.entry((parent, polyglot_move(mv))).or_insert((key, [0; 3]));
        if let Some(result) = result {
            results[result.index_for_mover(ply)] += 1;
        }
    }
}

fn note_node(key: u64, board: &Board, kept: &KeyMap<u32>, graph: &mut RawGraph) {
    if let Some(&games) = kept.get(&key) {
        graph.nodes.entry(key).or_insert(Node { games, board: *board });
    }
}

/// One move of a node: the Polyglot move, the child, the games' results.
type Child = (u16, u64, Results);

/// The graph as stored: a DAG of the nodes reachable from `root`, each node's moves sorted.
struct Graph {
    root: u64,
    nodes: KeyMap<Node>,
    children: KeyMap<Vec<Child>>,
    /// Edges dropped because they closed a cycle across games.
    cycle_edges: usize,
}

impl Graph {
    fn is_leaf(&self, key: u64) -> bool {
        self.children.get(&key).is_none_or(Vec::is_empty)
    }
}

/// Keeps what a depth-first walk from `root` reaches, moves in order, dropping every edge to a node
/// still on the walk's path: those close a cycle.
fn prune(raw: RawGraph, root: u64) -> Graph {
    let mut adjacency: KeyMap<Vec<Child>> = KeyMap::default();
    for (&(parent, mv), &(child, results)) in &raw.edges {
        adjacency.entry(parent).or_default().push((mv, child, results));
    }
    for moves in adjacency.values_mut() {
        moves.sort_unstable();
    }
    // Absent: not reached yet; true: on the walk's path; false: finished.
    let mut on_path: KeyMap<bool> = KeyMap::default();
    let mut children: KeyMap<Vec<Child>> = KeyMap::default();
    let mut cycle_edges = 0;
    let mut stack: Vec<(u64, usize)> = vec![(root, 0)];
    on_path.insert(root, true);
    while let Some(top) = stack.last_mut() {
        let node = top.0;
        match adjacency.get(&node).and_then(|moves| moves.get(top.1)) {
            Some(&edge) => {
                let child = edge.1;
                top.1 += 1;
                match on_path.get(&child).copied() {
                    Some(true) => cycle_edges += 1,
                    Some(false) => children.entry(node).or_default().push(edge),
                    None => {
                        children.entry(node).or_default().push(edge);
                        on_path.insert(child, true);
                        stack.push((child, 0));
                    }
                }
            }
            None => {
                on_path.insert(node, false);
                stack.pop();
            }
        }
    }
    let nodes = raw.nodes.into_iter().filter(|(key, _)| on_path.contains_key(key)).collect();
    Graph { root, nodes, children, cycle_edges }
}

/// Each node's ply: the fewest moves from the root to it.
fn node_plies(graph: &Graph) -> KeyMap<u32> {
    let mut plies: KeyMap<u32> = KeyMap::default();
    plies.insert(graph.root, 0);
    let mut queue = VecDeque::from([graph.root]);
    while let Some(node) = queue.pop_front() {
        let ply = plies[&node];
        for &(_, child, _) in graph.children.get(&node).map(Vec::as_slice).unwrap_or(&[]) {
            if let std::collections::hash_map::Entry::Vacant(e) = plies.entry(child) {
                e.insert(ply + 1);
                queue.push_back(child);
            }
        }
    }
    plies
}

/// What `build_graph` returns: the graph, how the games went, the games it used (`G`), the N it
/// chose from them, and how many positions at the cap still reached N.
struct Built {
    graph: Graph,
    tally: Tally,
    games_used: u64,
    min_games: u32,
    at_cap: usize,
}

/// Both passes, each reading every game through `read_games`, then the pruning. Plies are read up
/// to `max_plies`, games after `until` are skipped, and N is `choose_n` of the games used, known
/// once the first pass has counted them.
fn build_graph(
    mut read_games: impl FnMut(&mut dyn FnMut(&PgnGame)), max_plies: usize, until: Option<u32>,
    choose_n: impl FnOnce(u64) -> u32,
) -> Built {
    let mut tally = Tally::default();
    let mut support = Support::default();
    read_games(&mut |game: &PgnGame| {
        tally.games += 1;
        match outside_prefix(game, until) {
            Some(DateExclusion::AfterUntil) => {
                tally.after_until += 1;
                return;
            }
            Some(DateExclusion::NoDate) => {
                tally.no_date += 1;
                return;
            }
            None => {}
        }
        if game.has_fen {
            tally.from_fen += 1;
            return;
        }
        if game.result.is_none() {
            tally.no_result += 1;
        }
        let (moves, readable) = resolved_moves(game, max_plies);
        if !readable {
            tally.unreadable += 1;
        }
        add_support(&moves, max_plies, &mut support);
    });

    let games_used = tally.games - tally.after_until - tally.no_date - tally.from_fen;
    let min_games = choose_n(games_used);
    let root = polyglot_key(&initial_board());
    let at_cap = support.at_cap.iter().filter(|key| support.games.get(key).is_some_and(|&n| n >= min_games)).count();
    let mut kept: KeyMap<u32> = support.games.into_iter().filter(|&(_, n)| n >= min_games).collect();
    // The initial position is always a node, even in a database smaller than N.
    kept.entry(root).or_insert(games_used as u32);

    let mut raw = RawGraph::default();
    read_games(&mut |game: &PgnGame| {
        if game.has_fen || outside_prefix(game, until).is_some() {
            return;
        }
        let (moves, _) = resolved_moves(game, max_plies);
        add_edges(&moves, game.result, &kept, &mut raw);
    });
    Built { graph: prune(raw, root), tally, games_used, min_games, at_cap }
}

/// A graph's book size for every N from `FIT_GRAPH_N` up: `sizes[i]` is the number of moves played in
/// at least `FIT_GRAPH_N + i` games, the moves a Polyglot-style book at that N keeps. Stops at the
/// first N with no move left.
fn book_sizes(graph: &Graph) -> Vec<u64> {
    let games: Vec<u32> =
        graph.children.values().flatten().map(|&(_, _, [wins, draws, losses])| wins + draws + losses).collect();
    let mut sizes = Vec::new();
    let mut n = FIT_GRAPH_N;
    loop {
        let size = games.iter().filter(|&&g| g >= n).count() as u64;
        if size == 0 {
            return sizes;
        }
        sizes.push(size);
        n += 1;
    }
}

/// The book size at a real-valued N, interpolated on a log scale between the whole numbers around
/// it: a book's size falls roughly as a power of N. `None` outside the measured range.
fn size_at(sizes: &[u64], n: f64) -> Option<f64> {
    let index = n - FIT_GRAPH_N as f64;
    if sizes.is_empty() || index < 0.0 || index > (sizes.len() - 1) as f64 {
        return None;
    }
    let low = index.floor() as usize;
    if low + 1 >= sizes.len() {
        return Some(sizes[low] as f64);
    }
    let (n_low, n_high) = ((FIT_GRAPH_N as usize + low) as f64, (FIT_GRAPH_N as usize + low + 1) as f64);
    let t = (n.ln() - n_low.ln()) / (n_high.ln() - n_low.ln());
    let (s_low, s_high) = ((sizes[low] as f64).ln(), (sizes[low + 1] as f64).ln());
    Some((s_low + t * (s_high - s_low)).exp())
}

/// The real-valued N at which a book's size is `target`, the inverse of `size_at`. `None` when the
/// target is outside the measured sizes.
fn n_at(sizes: &[u64], target: f64) -> Option<f64> {
    for low in 0..sizes.len().saturating_sub(1) {
        let (s_low, s_high) = (sizes[low] as f64, sizes[low + 1] as f64);
        if s_low >= target && target >= s_high {
            let (n_low, n_high) = ((FIT_GRAPH_N as usize + low) as f64, (FIT_GRAPH_N as usize + low + 1) as f64);
            let t = if s_low == s_high { 0.0 } else { (s_low.ln() - target.ln()) / (s_low.ln() - s_high.ln()) };
            return Some((n_low.ln() + t * (n_high.ln() - n_low.ln())).exp());
        }
    }
    None
}

/// The slope of the least-squares line through `points`, `(ln G, ln N)`.
fn slope(points: &[(f64, f64)]) -> f64 {
    let count = points.len() as f64;
    let (mean_x, mean_y) =
        (points.iter().map(|p| p.0).sum::<f64>() / count, points.iter().map(|p| p.1).sum::<f64>() / count);
    let covariance: f64 = points.iter().map(|p| (p.0 - mean_x) * (p.1 - mean_y)).sum();
    let variance: f64 = points.iter().map(|p| (p.0 - mean_x).powi(2)).sum();
    covariance / variance
}

/// One point of the fit: a prefix (or the whole), up to which date (`YYYYMMDD`), its games, its book
/// sizes.
struct FitPoint {
    until: u32,
    games: u64,
    sizes: Vec<u64>,
}

/// `alpha` from the prefixes' and the whole's book sizes, by fixed point: from `alpha = 1`, the whole
/// gets `N_ref = ANCHOR_N * (G / ANCHOR_GAMES)^alpha` (real, not rounded); each prefix gets the N at
/// which its book is as large as the whole's at `N_ref`; the least-squares slope of `ln N` against
/// `ln G` over all points is the next `alpha`. Returns `alpha` and each point's N, the whole last.
fn fit_alpha(prefixes: &[FitPoint], whole: &FitPoint) -> Result<(f64, Vec<f64>), String> {
    let mut alpha = 1.0;
    for _ in 0..100 {
        let n_ref = (ANCHOR_N * (whole.games as f64 / ANCHOR_GAMES).powf(alpha)).max(FIT_GRAPH_N as f64);
        let target = size_at(&whole.sizes, n_ref).ok_or_else(|| format!("the whole's book has no size at N = {n_ref:.2}"))?;
        let mut ns = Vec::new();
        for prefix in prefixes {
            let n = n_at(&prefix.sizes, target).ok_or_else(|| {
                format!(
                    "the prefix up to {} ({} games) never has a book of {target:.0} moves at N >= {FIT_GRAPH_N}: too small to fit",
                    show_date(prefix.until),
                    prefix.games
                )
            })?;
            ns.push(n);
        }
        ns.push(n_ref);
        let points: Vec<(f64, f64)> = prefixes
            .iter()
            .chain(std::iter::once(whole))
            .zip(&ns)
            .map(|(point, n)| ((point.games as f64).ln(), n.ln()))
            .collect();
        let next = slope(&points);
        if (next - alpha).abs() < 1e-6 {
            return Ok((next, ns));
        }
        alpha = next;
    }
    Err("alpha did not settle in 100 rounds".to_string())
}

/// The dates at which the games, in date order, first reach each of `FIT_SHARES` of their total,
/// from `dates` (each game's `YYYYMMDD`). Days, not years, so a source spanning months (a Lichess
/// export) cuts as well as one spanning centuries. Distinct and before the last date, so every
/// prefix is a proper part of the whole.
fn cut_dates(dates: &BTreeMap<u32, u64>) -> Vec<u32> {
    let total: u64 = dates.values().sum();
    let last = *dates.keys().next_back().expect("a source with dated games");
    let mut cuts = Vec::new();
    for share in FIT_SHARES {
        let mut cumulative = 0;
        for (&date, &count) in dates {
            cumulative += count;
            if cumulative as f64 >= share * total as f64 {
                if date < last && cuts.last() != Some(&date) {
                    cuts.push(date);
                }
                break;
            }
        }
    }
    cuts
}

/// The fit stage: the source's dates, its date prefixes' and whole graphs at `FIT_GRAPH_N`, `alpha`
/// from their book sizes, and the source's `fit.txt`, moved to the remote. `pgn` is the source's
/// PGN in the remote's tree.
fn run_fit(source: &str, pgn: &str) {
    let start = Instant::now();
    let mut dates: BTreeMap<u32, u64> = BTreeMap::new();
    let (mut games, mut undated) = (0u64, 0u64);
    read_pgn(pgn, "fit: dates", |game| {
        if game.has_fen {
            return;
        }
        games += 1;
        match game.date {
            Some(date) => *dates.entry(date).or_insert(0) += 1,
            None => undated += 1,
        }
    });
    // A game without a full date cannot be placed in a prefix, so it is left out of every point, the
    // whole included, and reported; a source made mostly of them shows as too few dates to cut.
    let (Some(&first), Some(&last)) = (dates.keys().next(), dates.keys().next_back()) else {
        fail(format!("{pgn}: no game with a full YYYY.MM.DD date among {games}"));
    };
    let cuts = cut_dates(&dates);
    if cuts.len() < 2 {
        fail(format!("{pgn}: its games span too few dates to cut {} prefixes", FIT_SHARES.len()));
    }

    let point = |until: u32| -> FitPoint {
        let mut pass = 0;
        let built = build_graph(
            |on_game: &mut dyn FnMut(&PgnGame)| {
                pass += 1;
                let stage = if pass == 1 { "fit: counting" } else { "fit: edges" };
                read_pgn(pgn, &format!("{stage} to {}", show_date(until)), |game| on_game(game));
            },
            DEFAULT_MAX_PLIES,
            Some(until),
            |_| FIT_GRAPH_N,
        );
        FitPoint { until, games: built.games_used, sizes: book_sizes(&built.graph) }
    };
    let prefixes: Vec<FitPoint> = cuts.iter().map(|&until| point(until)).collect();
    // Every dated game: the same games the prefixes are cut from.
    let whole = point(last);
    let (alpha, ns) = fit_alpha(&prefixes, &whole).unwrap_or_else(|message| fail(format!("{pgn}: {message}")));

    let mut text = String::new();
    for (name, value) in [
        ("source", source.to_string()),
        ("pgn", pgn.to_string()),
        ("games", games.to_string()),
        ("games_without_date", undated.to_string()),
        ("dates", format!("{} to {}", show_date(first), show_date(last))),
        ("shares", FIT_SHARES.iter().map(|share| share.to_string()).collect::<Vec<_>>().join(",")),
        ("until", cuts.iter().map(|&date| show_date(date)).collect::<Vec<_>>().join(",")),
        ("anchor", format!("N = {ANCHOR_N} at {ANCHOR_GAMES} games")),
        ("graph_n", FIT_GRAPH_N.to_string()),
        ("max_plies", DEFAULT_MAX_PLIES.to_string()),
    ] {
        text.push_str(&format!("{name}\t{value}\n"));
    }
    for (point, n) in prefixes.iter().chain(std::iter::once(&whole)).zip(&ns) {
        text.push_str(&format!("point\t{}\t{}\t{n:.3}\n", show_date(point.until), point.games));
    }
    text.push_str(&format!("alpha\t{alpha:.4}\n"));
    text.push_str(&format!("n_now\t{}\n", n_for(whole.games, alpha)));
    text.push_str(&format!("built\t{}\n", current_time()));
    let path = fit_path(source);
    create_parent(&path);
    write_atomically(&path, text.as_bytes());
    print!("{text}");
    println!("{:.0} s", start.elapsed().as_secs_f64());
    if let Err(error) = remote::move_up_replacing(&path) {
        fail(format!("{error}. The fit stays in {path}; make the book again to upload it"));
    }
    println!("moved the fit to {}", remote::remote_path(&path));
}

/// `graph.txt`: the header as `name<TAB>value` lines, then one `node` line per node, by ply then
/// key (key in hex, games, 1 for a leaf, FEN), then one `edge` line per edge, by parent then move
/// (parent, Polyglot move, child, then wins, draws and losses for the side that played it).
fn graph_text(graph: &Graph, plies: &KeyMap<u32>, header: &[(&str, String)]) -> String {
    let mut text = String::new();
    for (name, value) in header {
        text.push_str(&format!("{name}\t{value}\n"));
    }
    let mut keys: Vec<u64> = graph.nodes.keys().copied().collect();
    keys.sort_unstable_by_key(|key| (plies[key], *key));
    for key in &keys {
        let node = &graph.nodes[key];
        let fen = node.board.to_fen(plies[key] / 2 + 1);
        text.push_str(&format!("node\t{key:016x}\t{}\t{}\t{fen}\n", node.games, u8::from(graph.is_leaf(*key))));
    }
    let mut parents: Vec<u64> = graph.children.keys().copied().collect();
    parents.sort_unstable();
    for parent in parents {
        for (mv, child, [wins, draws, losses]) in &graph.children[&parent] {
            text.push_str(&format!("edge\t{parent:016x}\t{mv}\t{child:016x}\t{wins}\t{draws}\t{losses}\n"));
        }
    }
    text
}

/// `graph.txt` read back: what the book needs. The nodes are checked line by line but only counted.
struct StoredGraph {
    header: Vec<(String, String)>,
    nodes: usize,
    /// Parent, move, child, results.
    edges: Vec<(u64, u16, u64, Results)>,
}

/// The inverse of `graph_text`.
fn parse_graph_text(text: &str) -> Result<StoredGraph, String> {
    let key = |field: &str| u64::from_str_radix(field, 16).map_err(|_| format!("not a key: {field:?}"));
    let mut graph = StoredGraph { header: Vec::new(), nodes: 0, edges: Vec::new() };
    for (index, line) in text.lines().enumerate() {
        let fields: Vec<&str> = line.split('\t').collect();
        let bad = || format!("graph line {}: {line:?}", index + 1);
        match fields.as_slice() {
            ["node", node_key, games, leaf, _fen] => {
                key(node_key)?;
                games.parse::<u32>().map_err(|_| bad())?;
                if !matches!(*leaf, "0" | "1") {
                    return Err(bad());
                }
                graph.nodes += 1;
            }
            ["edge", parent, mv, child, wins, draws, losses] => {
                let count = |field: &str| field.parse::<u32>().map_err(|_| bad());
                let results = [count(wins)?, count(draws)?, count(losses)?];
                graph.edges.push((key(parent)?, mv.parse().map_err(|_| bad())?, key(child)?, results));
            }
            ["edge", ..] => {
                return Err(format!("graph line {}: an edge without results, from before version 2: run graph again", index + 1));
            }
            [name, value] => graph.header.push((name.to_string(), value.to_string())),
            _ => return Err(bad()),
        }
    }
    Ok(graph)
}

/// What a book move weighs: the points it scored for the side that played it (2 a win, 1 a draw),
/// as Polyglot's `make-book` weights moves, or how often masters played it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Weights {
    Points,
    Counts,
}

impl Weights {
    fn name(self) -> &'static str {
        match self {
            Weights::Points => "points",
            Weights::Counts => "counts",
        }
    }

    fn weight(self, [wins, draws, losses]: Results) -> u64 {
        match self {
            Weights::Points => 2 * wins as u64 + draws as u64,
            Weights::Counts => wins as u64 + draws as u64 + losses as u64,
        }
    }
}

/// What `book_entries` builds.
#[derive(Debug, PartialEq)]
struct Book {
    entries: Vec<Entry>,
    /// Positions with at least one book move.
    positions: usize,
    /// Graph edges played fewer than N times: both ends have N games, but transpositions brought most
    /// of the child's.
    below_n: usize,
    /// Moves weighing 0 (under points, every game lost): left out, since `scaled_weights` never
    /// writes 0, which readers take as "never play", and would give them a small chance instead.
    weightless: usize,
}

/// The Polyglot-style book over `edges`: every move played at least `min_games` times from its
/// position, weighed by `weights`, scaled per position. Sorted as Polyglot requires: by key, then
/// heaviest first, then by move.
fn book_entries(edges: &[(u64, u16, u64, Results)], min_games: u32, weights: Weights) -> Book {
    let mut by_parent: BTreeMap<u64, Vec<(u16, u64)>> = BTreeMap::new();
    let (mut below_n, mut weightless) = (0, 0);
    for &(parent, mv, _, results) in edges {
        if Weights::Counts.weight(results) < min_games as u64 {
            below_n += 1;
            continue;
        }
        let weight = weights.weight(results);
        if weight == 0 {
            weightless += 1;
            continue;
        }
        by_parent.entry(parent).or_default().push((mv, weight));
    }
    let mut entries = Vec::new();
    for (&key, moves) in &by_parent {
        let counts: Vec<u64> = moves.iter().map(|&(_, weight)| weight).collect();
        for (&(mv, _), weight) in moves.iter().zip(scaled_weights(&counts)) {
            entries.push(Entry { key, mv, weight });
        }
    }
    entries.sort_by(|a, b| a.key.cmp(&b.key).then(b.weight.cmp(&a.weight)).then(a.mv.cmp(&b.mv)));
    Book { entries, positions: by_parent.len(), below_n, weightless }
}

fn fail(message: String) -> ! {
    eprintln!("{message}");
    std::process::exit(1);
}

/// Writes `bytes` to `path` via `{path}.tmp` and a rename, so a reader never sees half a file.
fn write_atomically(path: &str, bytes: &[u8]) {
    let tmp_path = format!("{path}.tmp");
    let mut file = BufWriter::new(File::create(&tmp_path).unwrap_or_else(|error| panic!("failed to create {tmp_path}: {error}")));
    file.write_all(bytes).unwrap_or_else(|error| panic!("failed to write {tmp_path}: {error}"));
    file.flush().unwrap_or_else(|error| panic!("failed to write {tmp_path}: {error}"));
    std::fs::rename(&tmp_path, path).unwrap_or_else(|error| panic!("failed to rename {tmp_path} to {path}: {error}"));
}

/// The current time as `YYYY-MM-DD HH:MM UTC`.
fn current_time() -> String {
    let now = time::OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02} UTC",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute()
    )
}

/// The write stage: the source's graph from the remote, its book at the graph's own N, and the
/// `.bin` and its record, moved to the remote beside the graph: the book first, the record last, so
/// a remote record never describes a book that is not there yet. Skipped when that record is there
/// already.
fn run_write(source: &str, weights: Weights) {
    let graph_path = remote::remote_path(&graph_path(source));
    let graph_text = std::fs::read_to_string(&graph_path).unwrap_or_else(|error| fail(format!("failed to read {graph_path}: {error}")));
    let graph = parse_graph_text(&graph_text).unwrap_or_else(|message| fail(format!("{graph_path}: {message}")));
    let min_games: u32 = graph
        .header
        .iter()
        .find(|(name, _)| name == "min_games")
        .and_then(|(_, value)| value.parse().ok())
        .unwrap_or_else(|| fail(format!("{graph_path}: no min_games line")));
    let stem = format!("{}/book-n{min_games}-{}", source_dir(source), weights.name());
    let (book_path, record_path) = (format!("{stem}.bin"), format!("{stem}.txt"));
    if std::path::Path::new(&remote::remote_path(&record_path)).is_file() {
        println!("write: done before, {record_path} kept");
        return;
    }
    let book = book_entries(&graph.edges, min_games, weights);
    create_parent(&book_path);
    write_atomically(&book_path, &book_bytes(&book.entries));
    let mut record = String::new();
    for (name, value) in &graph.header {
        record.push_str(&format!("graph_{name}\t{value}\n"));
    }
    for (name, value) in [
        ("weights", weights.name().to_string()),
        ("positions", book.positions.to_string()),
        ("moves", book.entries.len().to_string()),
        ("edges_below_n", book.below_n.to_string()),
        ("moves_weighing_0", book.weightless.to_string()),
        ("written", current_time()),
    ] {
        record.push_str(&format!("{name}\t{value}\n"));
    }
    write_atomically(&record_path, record.as_bytes());
    println!(
        "{} graph nodes, {} edges: {} positions, {} moves ({:.2} a position); {} edges below N = {min_games}, {} weighing 0",
        graph.nodes,
        graph.edges.len(),
        book.positions,
        book.entries.len(),
        book.entries.len() as f64 / book.positions.max(1) as f64,
        book.below_n,
        book.weightless
    );
    for path in [&book_path, &record_path] {
        if let Err(error) = remote::move_up_replacing(path) {
            fail(format!("{error}. The book stays in {}; make the book again to upload it", source_dir(source)));
        }
    }
    println!("moved the book to {} and its record beside it", remote::remote_path(&book_path));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    /// The graph of `pgn`'s games.
    fn built(pgn: &str, min_games: u32, max_plies: usize) -> Built {
        let read = |on_game: &mut dyn FnMut(&PgnGame)| {
            for_each_game(std::io::Cursor::new(pgn.as_bytes()), |game| on_game(game)).unwrap();
        };
        build_graph(read, max_plies, None, |_| min_games)
    }

    fn games(movetexts: &[&str]) -> String {
        movetexts.iter().map(|moves| format!("[Event \"e\"]\n[Result \"*\"]\n\n{moves} *\n\n")).collect()
    }

    /// The key of the position after `moves` (SAN, from the start).
    fn key_after(moves: &[&str]) -> u64 {
        let mut board = initial_board();
        for san in moves {
            let parsed: shakmaty::san::San = san.parse().unwrap();
            let mv = try_resolve_san(&board, &parsed).unwrap();
            board.make_move(mv);
        }
        polyglot_key(&board)
    }

    #[test]
    fn a_book_job_takes_a_source_then_its_weights() {
        assert_eq!(parse_make_args(&args(&["elite"])), Ok(MakeArgs { source: "elite".to_string(), weights: Weights::Points }));
        assert_eq!(
            parse_make_args(&args(&["s", "--weights", "counts"])),
            Ok(MakeArgs { source: "s".to_string(), weights: Weights::Counts })
        );
        for list in [
            &[][..],
            &["--weights", "points"][..],
            &["s", "--weights"][..],
            &["s", "--weights", "results"][..],
            &["s", "--weights", "points", "--weights", "counts"][..],
            &["s", "--scale", "1024"][..],
            &["s", "--tau", "20"][..],
            &["a/b"][..],
            &[".."][..],
            &["G:x"][..],
        ] {
            assert!(parse_make_args(&args(list)).is_err(), "{list:?}");
        }
    }

    #[test]
    fn a_source_s_files_are_in_its_folder() {
        assert_eq!(graph_path("twic"), "data/chess/books/twic/graph.txt");
        assert_eq!(fit_path("twic"), "data/chess/books/twic/fit.txt");
    }

    #[test]
    fn a_written_book_is_read_from_its_record_s_name() {
        assert_eq!(
            written_book("twic", "book-n9-points.txt"),
            Some(WrittenView { n: 9, weights: "points".to_string(), path: "data/chess/books/twic/book-n9-points.bin".to_string() })
        );
        for other in ["book-n9-points.bin", "book-n9-results.txt", "book-nx-counts.txt", "graph.txt", "fit.txt"] {
            assert_eq!(written_book("twic", other), None, "{other}");
        }
    }

    #[test]
    fn n_follows_the_anchor_and_alpha_and_never_falls_below_the_floor() {
        assert_eq!(n_for(1_000_000, 0.83), 10, "the anchor, whatever alpha");
        assert_eq!(n_for(1_000_000, 0.5), 10);
        assert_eq!(n_for(863_790, 0.83), 9, "Elite today: 8.86");
        assert_eq!(n_for(863_790, 1.0), 9, "8.64");
        assert_eq!(n_for(2_000_000, 0.83), 18, "17.8");
        assert_eq!(n_for(10_000, 0.83), 3, "0.22: the floor");
    }

    #[test]
    fn only_full_dates_are_read() {
        assert_eq!(parse_date(b"2024.03.17"), Some(20240317));
        assert_eq!(show_date(20240317), "2024.03.17");
        for partial in [&b"2005.??.??"[..], b"2005.06.??", b"????.??.??", b"2005", b"", b"2005.6.1"] {
            assert_eq!(parse_date(partial), None, "{:?}", std::str::from_utf8(partial));
        }
    }

    #[test]
    fn prefixes_are_cut_where_the_games_reach_their_shares() {
        // 100 games a day over ten days of one month, as in a Lichess export.
        let dates: BTreeMap<u32, u64> = (20240301..=20240310).map(|date| (date, 100)).collect();
        assert_eq!(
            cut_dates(&dates),
            vec![20240304, 20240306, 20240307],
            "400, 600 and 700 games of 1,000 reach 40%, 55%, 70%"
        );
        let two: BTreeMap<u32, u64> = [(20000101, 10), (20000102, 10)].into_iter().collect();
        assert!(cut_dates(&two).len() < 2, "too few dates to cut");
    }

    #[test]
    fn book_sizes_interpolate_on_a_log_scale_and_invert() {
        let sizes = [1000, 500, 250];
        assert!((size_at(&sizes, 3.0).unwrap() - 1000.0).abs() < 1e-9);
        assert!((size_at(&sizes, 5.0).unwrap() - 250.0).abs() < 1e-9);
        assert!(size_at(&sizes, 2.0).is_none() && size_at(&sizes, 6.0).is_none());
        let size = size_at(&sizes, 3.5).unwrap();
        assert!(size < 1000.0 && size > 500.0);
        assert!((n_at(&sizes, size).unwrap() - 3.5).abs() < 1e-9, "n_at inverts size_at");
        assert!(n_at(&sizes, 2000.0).is_none(), "beyond the largest book");
    }

    #[test]
    fn the_fit_recovers_alpha_from_books_that_follow_a_power_law() {
        // A book of size 1e6 * (G^0.8 / N)^1.5 at each whole N: equal sizes need N to grow as G^0.8.
        let point = |until: u32, games: u64| {
            let sizes = (FIT_GRAPH_N..60).map(|n| (1e6 * ((games as f64).powf(0.8) / n as f64 / 1e4).powf(1.5)) as u64).collect();
            FitPoint { until, games, sizes }
        };
        let prefixes = [point(20051231, 300_000), point(20101231, 450_000), point(20151231, 600_000)];
        let (alpha, ns) = fit_alpha(&prefixes, &point(20201231, 900_000)).unwrap();
        assert!((alpha - 0.8).abs() < 0.01, "{alpha}");
        assert_eq!(ns.len(), 4);
        assert!(ns.windows(2).all(|pair| pair[0] < pair[1]), "N grows with G: {ns:?}");
    }

    #[test]
    fn a_date_prefix_keeps_the_games_up_to_its_day_and_skips_partly_dated_ones() {
        let pgn = "[Date \"2004.12.31\"]\n[Result \"1-0\"]\n\n1. e4 1-0\n\n\
                   [Date \"2005.06.30\"]\n[Result \"1-0\"]\n\n1. e4 1-0\n\n\
                   [Date \"2005.07.01\"]\n[Result \"1-0\"]\n\n1. d4 1-0\n\n\
                   [Date \"2005.??.??\"]\n[Result \"1-0\"]\n\n1. c4 1-0\n\n\
                   [Date \"????.??.??\"]\n[Result \"1-0\"]\n\n1. c4 1-0\n\n";
        let read = |on_game: &mut dyn FnMut(&PgnGame)| {
            for_each_game(std::io::Cursor::new(pgn.as_bytes()), |game| on_game(game)).unwrap();
        };
        let built = build_graph(read, 40, Some(20050630), |games| {
            assert_eq!(games, 2, "G is the games kept: up to 2005.06.30");
            1
        });
        assert_eq!((built.tally.games, built.tally.after_until, built.tally.no_date), (5, 1, 2));
        let graph = built.graph;
        assert_eq!(graph.nodes[&key_after(&[])].games, 2, "the 2004 and 2005.06.30 games only");
        assert!(graph.nodes.contains_key(&key_after(&["e4"])));
        assert!(!graph.nodes.contains_key(&key_after(&["d4"])), "2005.07.01 is after the prefix");
        assert!(!graph.nodes.contains_key(&key_after(&["c4"])), "a partly dated game cannot be placed");
    }

    #[test]
    fn transpositions_pool_into_one_node_with_both_parents() {
        let pgn = games(&["1. Nf3 Nf6 2. g3", "1. Nf3 Nf6 2. g3", "1. g3 Nf6 2. Nf3", "1. g3 Nf6 2. Nf3"]);
        let graph = built(&pgn, 2, 40).graph;
        let pooled = key_after(&["Nf3", "Nf6", "g3"]);
        assert_eq!(pooled, key_after(&["g3", "Nf6", "Nf3"]));
        assert_eq!(graph.nodes[&pooled].games, 4);
        let parents = graph.children.values().filter(|moves| moves.iter().any(|&(_, child, _)| child == pooled)).count();
        assert_eq!(parents, 2);
        assert!(graph.is_leaf(pooled));
        assert_eq!(graph.nodes.len(), 6, "the root, two plies on each side, and the pooled position");
    }

    #[test]
    fn a_repeated_position_counts_its_game_once_and_its_cycle_edge_is_dropped() {
        let built = built(&games(&["1. Nf3 Nf6 2. Ng1 Ng8 3. Nf3"]), 1, 40);
        let graph = built.graph;
        let root = key_after(&[]);
        assert_eq!(graph.nodes[&root].games, 1, "reached twice by one game");
        assert_eq!(graph.nodes[&key_after(&["Nf3"])].games, 1);
        let back = key_after(&["Nf3", "Nf6", "Ng1"]);
        assert!(graph.is_leaf(back), "Ng8 returns to the start: dropped");
        assert_eq!(graph.nodes.len(), 4);
        assert_eq!(graph.cycle_edges, 0, "dropped within the game, before the walk");
    }

    #[test]
    fn an_edge_closing_a_cycle_across_games_is_dropped() {
        let board = initial_board();
        let mut raw = RawGraph::default();
        for key in [1, 2, 3] {
            raw.nodes.insert(key, Node { games: 1, board });
        }
        raw.edges.insert((1, 10), (2, [0; 3]));
        raw.edges.insert((2, 20), (3, [0; 3]));
        raw.edges.insert((3, 30), (2, [0; 3]));
        let graph = prune(raw, 1);
        assert_eq!(graph.cycle_edges, 1);
        assert!(graph.is_leaf(3), "3 -> 2 closes the cycle 2 -> 3 -> 2");
        assert_eq!(graph.nodes.len(), 3);
    }

    #[test]
    fn a_node_reached_only_through_thin_parents_is_dropped() {
        let pgn = games(&["1. Nf3 Nf6 2. g3", "1. g3 Nf6 2. Nf3"]);
        let graph = built(&pgn, 2, 40).graph;
        assert_eq!(graph.nodes.len(), 1, "only the root: every ply between it and the pooled position has one game");
        assert!(graph.is_leaf(key_after(&[])));
    }

    #[test]
    fn games_from_a_fen_are_skipped_and_an_illegal_move_ends_its_game() {
        let pgn = format!(
            "[Event \"f\"]\n[FEN \"4k3/8/8/8/8/8/8/4K3 w - - 0 1\"]\n[Result \"*\"]\n\n1. Kd2 *\n\n{}",
            games(&["1. e4 e5 2. Ke3", "1. e4 e5"])
        );
        let built = built(&pgn, 2, 40);
        assert_eq!((built.tally.games, built.tally.from_fen, built.tally.unreadable), (3, 1, 1));
        assert_eq!(built.graph.nodes[&key_after(&["e4", "e5"])].games, 2, "the illegal game's prefix counts");
    }

    #[test]
    fn positions_reaching_n_at_the_cap_are_reported() {
        let built = built(&games(&["1. e4 e5 2. Nf3", "1. e4 e5 2. Nf3", "1. d4 d5"]), 2, 2);
        assert_eq!(built.at_cap, 1, "1. e4 e5 is at the cap with two games; 1. d4 d5 with one");
        assert!(!built.graph.nodes.contains_key(&key_after(&["e4", "e5", "Nf3"])), "past the cap");
    }

    #[test]
    fn the_stored_graph_lists_nodes_by_ply_and_edges_by_parent() {
        let graph = built(&games(&["1. e4", "1. e4"]), 2, 40).graph;
        let plies = node_plies(&graph);
        let text = graph_text(&graph, &plies, &[("min_games", "2".to_string())]);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "min_games\t2");
        assert_eq!(lines[1], "node\t463b96181691fc9c\t2\t0\trnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
        assert!(lines[2].starts_with("node\t823c9b50fd114196\t2\t1\t"), "{}", lines[2]);
        assert_eq!(lines[3], format!("edge\t463b96181691fc9c\t{}\t823c9b50fd114196\t0\t0\t0", (12 << 6) | 28), "no results");
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn the_stored_graph_reads_back_as_written() {
        let graph = built(&games(&["1. e4 e5", "1. e4 e5", "1. d4", "1. d4"]), 2, 40).graph;
        let plies = node_plies(&graph);
        let text = graph_text(&graph, &plies, &[("min_games", "2".to_string())]);
        let stored = parse_graph_text(&text).unwrap();
        assert_eq!(stored.header, vec![("min_games".to_string(), "2".to_string())]);
        assert_eq!(stored.nodes, graph.nodes.len());
        assert_eq!(stored.edges.len(), 3);
        for line in text.lines().filter(|line| line.starts_with("node\t")) {
            let fields: Vec<&str> = line.split('\t').collect();
            let (board, _) = Board::from_fen(fields[4]).unwrap();
            assert_eq!(format!("{:016x}", polyglot_key(&board)), fields[1], "the FEN is the node's position");
        }
        assert!(parse_graph_text("node\tzz\t2\t0\tfen").is_err());
        assert!(parse_graph_text("node\t1\t2\t5\tfen").is_err());
        assert!(parse_graph_text("one\ttwo\tthree").is_err());
    }

    #[test]
    fn edge_results_are_counted_once_per_game_from_the_side_that_moved() {
        let pgn = "[Result \"1-0\"]\n\n1. e4 e5 1-0\n\n[Result \"1-0\"]\n\n1. e4 e5 1-0\n\n\
                   [Result \"1/2-1/2\"]\n\n1. e4 e5 1/2-1/2\n\n[Result \"*\"]\n\n1. e4 *\n\n";
        let built = built(pgn, 1, 40);
        assert_eq!(built.tally.no_result, 1);
        let results_of = |parent: u64, child: u64| {
            let moves = &built.graph.children[&parent];
            moves.iter().find(|&&(_, c, _)| c == child).expect("the edge").2
        };
        let (root, e4, e5) = (key_after(&[]), key_after(&["e4"]), key_after(&["e4", "e5"]));
        assert_eq!(results_of(root, e4), [2, 1, 0], "White played e4 and won twice; the game without a result counts nowhere");
        assert_eq!(results_of(e4, e5), [0, 1, 2], "Black played e5 and lost those two");
    }

    #[test]
    fn a_graph_from_before_edge_results_is_refused() {
        let message = parse_graph_text("edge\t0000000000000001\t10\t0000000000000002\n").err().unwrap();
        assert!(message.contains("run graph again"), "{message}");
    }

    #[test]
    fn the_two_weights_count_points_or_games() {
        assert_eq!(Weights::Points.weight([3, 2, 1]), 8, "2 a win, 1 a draw, 0 a loss");
        assert_eq!(Weights::Counts.weight([3, 2, 1]), 6);
    }

    #[test]
    fn the_book_keeps_moves_played_n_times_and_drops_weightless_ones_under_points() {
        let edges = [
            (1, 10, 2, [5, 4, 1]), // 10 games, 14 points
            (1, 11, 3, [1, 1, 1]), // 3 games: below N between two nodes
            (1, 12, 4, [0, 0, 9]), // 9 games, 0 points
            (2, 20, 5, [0, 9, 0]), // 9 games, 9 points
        ];
        let points = book_entries(&edges, 9, Weights::Points);
        assert_eq!(
            points.entries,
            vec![Entry { key: 1, mv: 10, weight: 14 }, Entry { key: 2, mv: 20, weight: 9 }],
            "the weights fit in 16 bits, so they are the points themselves"
        );
        assert_eq!((points.positions, points.below_n, points.weightless), (2, 1, 1));
        let counts = book_entries(&edges, 9, Weights::Counts);
        assert_eq!(
            counts.entries,
            vec![Entry { key: 1, mv: 10, weight: 10 }, Entry { key: 1, mv: 12, weight: 9 }, Entry { key: 2, mv: 20, weight: 9 }],
            "every game lost still counts as played"
        );
        assert_eq!((counts.below_n, counts.weightless), (1, 0));
    }

    #[test]
    fn the_book_of_a_built_graph_reads_back_from_its_bytes() {
        let pgn = "[Result \"1-0\"]\n\n1. e4 1-0\n\n[Result \"1-0\"]\n\n1. e4 1-0\n\n[Result \"1/2-1/2\"]\n\n1. d4 1/2-1/2\n\n\
                   [Result \"1/2-1/2\"]\n\n1. d4 1/2-1/2\n\n";
        let graph = built(pgn, 2, 40).graph;
        let text = graph_text(&graph, &node_plies(&graph), &[("min_games", "2".to_string())]);
        let book = book_entries(&parse_graph_text(&text).unwrap().edges, 2, Weights::Points);
        let bytes = book_bytes(&book.entries);
        assert_eq!(bytes.len(), 32, "two entries of 16 bytes");
        let root = key_after(&[]).to_be_bytes();
        assert_eq!(&bytes[0..8], &root);
        assert_eq!(&bytes[16..24], &root);
        let e4 = (12u16 << 6) | 28;
        let d4 = (11u16 << 6) | 27;
        assert_eq!(&bytes[8..12], &[(e4 >> 8) as u8, e4 as u8, 0, 4], "e4: two wins, 4 points, first");
        assert_eq!(&bytes[24..28], &[(d4 >> 8) as u8, d4 as u8, 0, 2], "d4: two draws, 2 points");
    }
}
