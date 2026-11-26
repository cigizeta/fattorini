//! `smithy book-fragment <engine>`: the next fragment of the engine's book. Fragment k holds the
//! positions Dijkstra expands at a cost in (budget(k - 1), budget(k)], budget(k) = 120 + 10k. The
//! run starts again from the initial position every time: positions of earlier fragments, and of
//! the progress file of a fragment stopped midway, are expanded from their stored moves without a
//! search, so the engines see only positions no file holds.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use fattorini::board::color::Color;
use fattorini::board::Board;
use fattorini::movegen::legal::legal_moves;
use fattorini::pipeline::data_root::data_root;
use fattorini::polyglot::{polyglot_key, polyglot_move};

use super::dijkstra::{self, Edges, Graph};
use super::engine::{self, Engine, RegistryEntry};
use super::expand::{expand, Expansion};
use super::format::{self, Header, Position};
use super::unit::{loss, Score};

/// Every position is searched to this depth, on one thread, from a fresh table.
pub const DEPTH: u8 = 20;
/// What each ply costs beside its loss: a line of best moves costs this much a ply.
pub const CHARGE: f64 = 10.0;
/// A move losing this many points or more is not admitted: a mistake (`?`) in a game review.
pub const THRESHOLD: f64 = 10.0;
/// Fragment 0's budget, and each next fragment's rise: fragment k reaches 12 + k plies of best moves.
const FIRST_BUDGET: f64 = 120.0;
const BUDGET_STEP: f64 = 10.0;

pub fn budget(index: u16) -> f64 {
    FIRST_BUDGET + BUDGET_STEP * index as f64
}

/// `data/books/book-<engine>-f<k>`, without its extension.
pub fn stem(books: &str, engine: &str, index: u16) -> String {
    format!("{books}/book-{engine}-f{index}")
}

pub fn books_dir() -> String {
    format!("{}/books", data_root())
}

/// A score from the side to move, from White's side.
pub fn to_white(score: Score, side_to_move: Color) -> Score {
    if side_to_move == Color::White { score } else { score.negated() }
}

/// The engine's fragments in order, each checked: indices from 0 with no gap, each at its budget,
/// all of one book.
pub fn read_fragments(books: &str, engine: &str) -> Result<Vec<(Header, Vec<Position>)>, String> {
    let mut fragments: Vec<(Header, Vec<Position>)> = Vec::new();
    for index in 0u16.. {
        let path = format!("{}.bin", stem(books, engine, index));
        let data = match fs::read(&path) {
            Ok(data) => data,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(format!("cannot read {path}: {error}")),
        };
        let (header, positions) = format::read(&data).map_err(|message| format!("{path}: {message}"))?;
        if header.index != index || header.budget as f64 != budget(index) {
            return Err(format!("{path}: holds fragment {} at budget {}, not {index} at {}", header.index, header.budget, budget(index)));
        }
        if let Some((first, _)) = fragments.first()
            && let Some(difference) = first.mismatch(&header)
        {
            return Err(format!("{path}: not of the same book as fragment 0 ({difference})"));
        }
        fragments.push((header, positions));
    }
    Ok(fragments)
}

/// What a batch of unknown positions is handed to: the engines, or a test's stand-in.
pub trait Searcher {
    fn expand_all(&mut self, boards: &[Board]) -> Vec<Result<Expansion, String>>;
}

/// The game graph over the stored positions and a searcher for the others.
pub struct BookGraph<S: Searcher> {
    /// Every position expanded so far, by key, scores from White's side.
    pub known: HashMap<u64, Position>,
    searcher: S,
    /// Positions this run searched, in the order searched, and the searches made.
    pub searched: Vec<u64>,
    pub searches: usize,
    progress: Option<File>,
}

impl<S: Searcher> BookGraph<S> {
    pub fn new(known: HashMap<u64, Position>, searcher: S, progress: Option<File>) -> BookGraph<S> {
        BookGraph { known, searcher, searched: Vec::new(), searches: 0, progress }
    }

    fn record(&mut self, board: &Board, expansion: &Expansion) -> Result<(), String> {
        let side = board.side_to_move();
        let position = Position {
            key: polyglot_key(board),
            eval: to_white(expansion.eval, side),
            moves: expansion.moves.iter().map(|&(mv, score)| (polyglot_move(mv), to_white(score, side))).collect(),
        };
        if let Some(file) = &mut self.progress {
            writeln!(file, "{}", progress_row(board, &position))
                .and_then(|()| file.flush())
                .map_err(|error| format!("cannot append to the progress file: {error}"))?;
        }
        self.searched.push(position.key);
        self.searches += expansion.trace.len();
        self.known.insert(position.key, position);
        Ok(())
    }

    /// A known position's edges: each stored move replayed on the board, its loss against the
    /// position's evaluation, both from the side to move.
    fn stored_edges(&self, board: &Board, position: &Position) -> Result<Vec<(Board, f64)>, String> {
        let side = board.side_to_move();
        let best = to_white(position.eval, side);
        let legal = legal_moves(board, side);
        position
            .moves
            .iter()
            .map(|&(code, score)| {
                let mv = legal
                    .iter()
                    .copied()
                    .find(|&mv| polyglot_move(mv) == code)
                    .ok_or_else(|| format!("position {:016x}: stored move {code:#06x} is not legal", position.key))?;
                let mut child = *board;
                child.make_move(mv);
                Ok((child, loss(best, to_white(score, side))))
            })
            .collect()
    }
}

impl<S: Searcher> Graph for BookGraph<S> {
    type Node = Board;

    fn key(&self, node: &Board) -> u64 {
        polyglot_key(node)
    }

    fn edges(&mut self, batch: &[Board]) -> Result<Edges<Board>, String> {
        let unknown: Vec<Board> = batch.iter().filter(|board| !self.known.contains_key(&polyglot_key(board))).copied().collect();
        if !unknown.is_empty() {
            let results = self.searcher.expand_all(&unknown);
            let mut failure = None;
            for (board, result) in unknown.iter().zip(results) {
                match result {
                    Ok(expansion) => self.record(board, &expansion)?,
                    Err(message) => {
                        failure.get_or_insert(message);
                    }
                }
            }
            if let Some(message) = failure {
                return Err(message);
            }
        }
        batch.iter().map(|board| self.stored_edges(board, &self.known[&polyglot_key(board)])).collect()
    }
}

/// The engines, one per worker thread, each expanding the positions handed to it.
pub struct Pool {
    jobs: Option<mpsc::Sender<(usize, Board)>>,
    results: mpsc::Receiver<(usize, Result<Expansion, String>)>,
    workers: Vec<thread::JoinHandle<()>>,
}

impl Pool {
    pub fn new(engines: Vec<Engine>, stop: Arc<AtomicBool>) -> Pool {
        let (jobs, job_receiver) = mpsc::channel::<(usize, Board)>();
        let job_receiver = Arc::new(Mutex::new(job_receiver));
        let (result_sender, results) = mpsc::channel();
        let workers = engines
            .into_iter()
            .map(|mut engine| {
                let (job_receiver, result_sender, stop) = (Arc::clone(&job_receiver), result_sender.clone(), Arc::clone(&stop));
                thread::spawn(move || {
                    loop {
                        let job = job_receiver.lock().unwrap().recv();
                        let Ok((index, board)) = job else { return };
                        let result = if stop.load(Ordering::SeqCst) {
                            Err("stopped".to_string())
                        } else {
                            expand(&board, &mut engine, DEPTH, THRESHOLD)
                        };
                        if result_sender.send((index, result)).is_err() {
                            return;
                        }
                    }
                })
            })
            .collect();
        Pool { jobs: Some(jobs), results, workers }
    }
}

impl Searcher for Pool {
    fn expand_all(&mut self, boards: &[Board]) -> Vec<Result<Expansion, String>> {
        let jobs = self.jobs.as_ref().expect("the pool is open");
        for (index, board) in boards.iter().enumerate() {
            jobs.send((index, *board)).expect("the workers are alive");
        }
        let mut results: Vec<Option<Result<Expansion, String>>> = (0..boards.len()).map(|_| None).collect();
        for _ in 0..boards.len() {
            match self.results.recv() {
                Ok((index, result)) => results[index] = Some(result),
                Err(_) => break,
            }
        }
        results.into_iter().map(|result| result.unwrap_or_else(|| Err("a worker died".to_string()))).collect()
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.jobs = None;
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

fn progress_header(engine_id: &str, index: u16) -> String {
    format!("# {engine_id}; depth {DEPTH}; charge {CHARGE}; threshold {THRESHOLD}; fragment {index}")
}

/// One row: key in hex, FEN (for a human), the evaluation and each move with its score, encoded
/// as in the fragment.
fn progress_row(board: &Board, position: &Position) -> String {
    let moves: Vec<String> = position.moves.iter().map(|&(code, score)| format!("{code}:{}", format::encode(score))).collect();
    format!("{:016x}\t{}\t{}\t{}", position.key, board.to_fen(1), format::encode(position.eval), moves.join(","))
}

/// The progress file's positions, refused when its header is another book's or fragment's. A row
/// cut short by a crash is dropped, and its position searched again.
fn parse_progress(text: &str, header_line: &str) -> Result<HashMap<u64, Position>, String> {
    let mut lines = text.lines();
    let first = lines.next().unwrap_or("");
    if first != header_line {
        return Err(format!("made as {first:?}, not {header_line:?}; delete it to start the fragment over"));
    }
    let mut positions = HashMap::new();
    for line in lines {
        let Some(position) = parse_row(line) else { continue };
        positions.insert(position.key, position);
    }
    Ok(positions)
}

fn parse_row(line: &str) -> Option<Position> {
    let fields: Vec<&str> = line.split('\t').collect();
    let [key, _fen, eval, moves] = fields[..] else { return None };
    let key = u64::from_str_radix(key, 16).ok()?;
    let eval = format::decode(eval.parse().ok()?);
    let moves = if moves.is_empty() {
        Vec::new()
    } else {
        moves
            .split(',')
            .map(|item| {
                let (code, score) = item.split_once(':')?;
                Some((code.parse().ok()?, format::decode(score.parse().ok()?)))
            })
            .collect::<Option<Vec<_>>>()?
    };
    Some(Position { key, eval, moves })
}

pub fn run(engine_name: &str) -> Result<(), String> {
    let registry = fs::read_to_string(format!("{}/engines.yaml", data_root()))
        .map_err(|error| format!("cannot read {}: {error}", engine::ENGINE_REGISTRY_PATH))?;
    let entry = engine::registry_entry(&registry, engine_name)?;
    let books = books_dir();
    fs::create_dir_all(&books).map_err(|error| format!("cannot create {books}: {error}"))?;
    let fragments = read_fragments(&books, engine_name)?;
    let index = u16::try_from(fragments.len()).map_err(|_| "too many fragments".to_string())?;

    let engines = start_engines(&entry)?;
    let engine_id = engines[0].name.clone();
    if let Some((first, _)) = fragments.first()
        && first.engine != engine_id
    {
        return Err(format!("the fragments were made by {}, the engine is now {engine_id}", first.engine));
    }
    let header = Header { index, budget: budget(index) as f32, charge: CHARGE as f32, threshold: THRESHOLD as f32, depth: DEPTH, engine: engine_id.clone() };
    if let Some((first, _)) = fragments.first()
        && let Some(difference) = first.mismatch(&header)
    {
        return Err(format!("fragment 0 differs from this build ({difference})"));
    }

    let mut known: HashMap<u64, Position> = HashMap::new();
    for (_, positions) in &fragments {
        known.extend(positions.iter().map(|position| (position.key, position.clone())));
    }
    let earlier = known.len();
    let stem = stem(&books, engine_name, index);
    let progress_path = format!("{stem}.progress.tsv");
    let header_line = progress_header(&engine_id, index);
    let resumed = match fs::read_to_string(&progress_path) {
        Ok(text) => parse_progress(&text, &header_line).map_err(|message| format!("{progress_path}: {message}"))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
        Err(error) => return Err(format!("cannot read {progress_path}: {error}")),
    };
    println!(
        "Fragment {index} of {engine_id}'s book, budget {}: {earlier} positions in {} earlier fragments, {} resumed; {} engines.",
        budget(index),
        fragments.len(),
        resumed.len(),
        engines.len()
    );
    known.extend(resumed);
    let progress = open_progress(&progress_path, &header_line)?;

    let stop = Arc::new(AtomicBool::new(false));
    {
        let stop = Arc::clone(&stop);
        ctrlc::set_handler(move || {
            println!("Stopping after the searches under way; the progress file keeps what is done.");
            stop.store(true, Ordering::SeqCst);
        })
        .map_err(|error| format!("cannot set the Ctrl+C handler: {error}"))?;
    }
    let mut graph = BookGraph::new(known, Pool::new(engines, Arc::clone(&stop)), Some(progress));
    let (start, _) = Board::from_fen(pgn_replay::STARTING_POSITION_FEN).expect("the starting position parses");
    let started = Instant::now();
    let previous_budget = if index == 0 { -1.0 } else { budget(index - 1) };
    let mut fragment_size = 0usize;
    let mut last_report = 0usize;
    let result = dijkstra::run(&mut graph, start, CHARGE, budget(index), |band| {
        fragment_size += band.iter().filter(|expanded| expanded.cost > previous_budget).count();
        if fragment_size >= last_report + 1000 {
            last_report = fragment_size;
            let elapsed = started.elapsed().as_secs_f64();
            println!("  {fragment_size} positions, cost {:.1}, {:.0} s", band[0].cost, elapsed);
        }
    });
    let expanded = match result {
        Ok(expanded) => expanded,
        Err(message) if stop.load(Ordering::SeqCst) => {
            println!("Stopped ({message}): {} positions searched this run; rerun to resume.", graph.searched.len());
            return Ok(());
        }
        Err(message) => return Err(message),
    };

    let new: Vec<_> = expanded.iter().filter(|expanded| expanded.cost > previous_budget).collect();
    let positions: Vec<Position> = new.iter().map(|expanded| graph.known[&expanded.key].clone()).collect();
    let bytes = format::write(&header, &positions);
    let path = format!("{stem}.bin");
    fs::write(&path, &bytes).map_err(|error| format!("cannot write {path}: {error}"))?;
    drop(graph);
    let _ = fs::remove_file(&progress_path);
    let moves: usize = positions.iter().map(|position| position.moves.len()).sum();
    let deepest = new.iter().map(|expanded| expanded.ply).max().unwrap_or(0);
    println!(
        "Wrote {path}: {} positions, {moves} moves ({:.2} a position), deepest ply {deepest}, {} bytes, {:.0} s.",
        positions.len(),
        moves as f64 / positions.len().max(1) as f64,
        bytes.len(),
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

fn start_engines(entry: &RegistryEntry) -> Result<Vec<Engine>, String> {
    let cores = thread::available_parallelism().map_or(1, |n| n.get());
    (0..cores).map(|_| Engine::start(entry)).collect()
}

fn open_progress(path: &str, header_line: &str) -> Result<File, String> {
    let exists = fs::metadata(path).is_ok();
    let mut file = OpenOptions::new().create(true).append(true).open(path).map_err(|error| format!("cannot open {path}: {error}"))?;
    if !exists {
        writeln!(file, "{header_line}").map_err(|error| format!("cannot write {path}: {error}"))?;
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A searcher that must never be asked.
    struct Refuses;

    impl Searcher for Refuses {
        fn expand_all(&mut self, boards: &[Board]) -> Vec<Result<Expansion, String>> {
            panic!("asked to search {} positions", boards.len());
        }
    }

    fn start() -> Board {
        Board::from_fen(pgn_replay::STARTING_POSITION_FEN).unwrap().0
    }

    fn after(moves: &[&str]) -> Board {
        let mut board = start();
        for text in moves {
            let legal = legal_moves(&board, board.side_to_move());
            board.make_move(fattorini::uci::parse_move(text, &legal).unwrap());
        }
        board
    }

    fn code(board: &Board, text: &str) -> u16 {
        polyglot_move(fattorini::uci::parse_move(text, &legal_moves(board, board.side_to_move())).unwrap())
    }

    #[test]
    fn a_rerun_over_known_positions_searches_nothing() {
        let root = start();
        let e4 = after(&["e2e4"]);
        let known = HashMap::from([
            (polyglot_key(&root), Position {
                key: polyglot_key(&root),
                eval: Score::Cp(30),
                moves: vec![(code(&root, "e2e4"), Score::Cp(30))],
            }),
            (polyglot_key(&e4), Position {
                key: polyglot_key(&e4),
                eval: Score::Cp(30),
                moves: vec![(code(&e4, "e7e5"), Score::Cp(30))],
            }),
        ]);
        let mut graph = BookGraph::new(known, Refuses, None);
        let expanded = dijkstra::run(&mut graph, root, CHARGE, 15.0, |_| {}).unwrap();
        assert_eq!(expanded.len(), 2, "the root at 0, 1. e4 at 10; 1... e5 costs 20");
        assert!(graph.searched.is_empty());
    }

    #[test]
    fn a_stored_loss_is_from_the_side_to_move() {
        // Black to move after 1. e4: e5 at +30 for White is Black's best, c5 at +60 is worse.
        let e4 = after(&["e2e4"]);
        let position = Position {
            key: polyglot_key(&e4),
            eval: Score::Cp(30),
            moves: vec![(code(&e4, "e7e5"), Score::Cp(30)), (code(&e4, "c7c5"), Score::Cp(60))],
        };
        let graph = BookGraph::new(HashMap::new(), Refuses, None);
        let edges = graph.stored_edges(&e4, &position).unwrap();
        assert_eq!(edges[0].1, 0.0);
        let expected = loss(Score::Cp(-30), Score::Cp(-60));
        assert!((edges[1].1 - expected).abs() < 1e-12 && expected > 4.0);
    }

    #[test]
    fn the_progress_resumes_and_refuses_another_fragment() {
        let header = progress_header("Judge 1", 3);
        let board = start();
        let position = Position { key: polyglot_key(&board), eval: Score::Cp(25), moves: vec![(code(&board, "e2e4"), Score::Cp(25)), (code(&board, "d2d4"), Score::Cp(20))] };
        let row = progress_row(&board, &position);
        let text = format!("{header}\n{row}\n0000000000000100\tfen\t31\n");
        let read = parse_progress(&text, &header).unwrap();
        assert_eq!(read.len(), 1, "a row cut short is searched again");
        assert_eq!(read[&position.key], position);
        assert!(parse_progress(&text, &progress_header("Judge 1", 4)).is_err());
        assert!(parse_progress(&text, &progress_header("Judge 2", 3)).is_err());
    }

    #[test]
    fn budgets_rise_by_ten_from_120() {
        assert_eq!((budget(0), budget(1), budget(8)), (120.0, 130.0, 200.0));
    }
}
