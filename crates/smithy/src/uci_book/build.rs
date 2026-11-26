//! `smithy uci-book <engine> <version>`: every position of the openings tree evaluated by the
//! engine at `DEPTH`, one engine of one thread per core, written as the book's own file.
//! Resumable: each evaluation is appended to a progress file as it comes, and a rerun searches only
//! the positions without a row.

use std::collections::{HashMap, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use fattorini::board::color::Color;
use fattorini::board::Board;
use fattorini::movegen::attacks::is_in_check;
use fattorini::movegen::legal::legal_moves;
use fattorini::pipeline::data_root::data_root;

use super::engine::{self, Engine, Score};
use super::format::{self, Entry, Eval, Header};
use super::tree::OpeningTree;

/// Every position is searched to this depth: one depth for the whole book, so its evaluations
/// compare, and on one thread, so a rebuild gives the same book.
pub const DEPTH: u8 = 20;

/// The book's files, in `data/books/`: `<stem>.bin` and `<stem>.progress.tsv`.
pub fn stem(engine: &str, version: u16) -> String {
    format!("uci-book-{engine}-v{version}")
}

pub fn run(engine_name: &str, version: u16) -> Result<(), String> {
    let registry = fs::read_to_string(format!("{}/engines.yaml", data_root()))
        .map_err(|error| format!("cannot read {}: {error}", engine::ENGINE_REGISTRY_PATH))?;
    let entry = engine::registry_entry(&registry, engine_name)?;
    let books = format!("{}/books", data_root());
    fs::create_dir_all(&books).map_err(|error| format!("cannot create {books}: {error}"))?;
    let stem = stem(engine_name, version);
    let progress_path = format!("{books}/{stem}.progress.tsv");
    let book_path = format!("{books}/{stem}.bin");

    let tree = OpeningTree::from_snapshot();
    println!(
        "Openings: {} lines ({} broken), {} book moves, {} positions.",
        tree.lines,
        tree.broken_lines,
        tree.edges.len(),
        tree.children.len()
    );
    let engine_id = Engine::start(&entry)?.name.clone();
    let header_line = progress_header(&engine_id, DEPTH);
    let mut done = read_progress(&progress_path, &header_line)?;

    // Positions with no legal move need no engine.
    let mut todo = VecDeque::new();
    let mut by_rules = Vec::new();
    for (&key, board) in &tree.children {
        if done.contains_key(&key) {
            continue;
        }
        match rules_eval(board) {
            Some(eval) => by_rules.push((key, *board, eval)),
            None => todo.push_back((key, *board)),
        }
    }
    let progress = Arc::new(Mutex::new(open_progress(&progress_path, &header_line)?));
    for (key, board, eval) in by_rules {
        append(&progress, key, &board, eval, 0)?;
        done.insert(key, (eval, 0));
    }
    println!("Evaluated already: {}; to search: {}.", done.len(), todo.len());

    let stop = Arc::new(AtomicBool::new(false));
    {
        let stop = Arc::clone(&stop);
        ctrlc::set_handler(move || {
            println!("Stopping after the searches under way; the progress file keeps what is done.");
            stop.store(true, Ordering::SeqCst);
        })
        .map_err(|error| format!("cannot set the Ctrl+C handler: {error}"))?;
    }

    let total = todo.len();
    let queue = Arc::new(Mutex::new(todo));
    let finished = Arc::new(AtomicUsize::new(0));
    let results = Arc::new(Mutex::new(Vec::new()));
    let started = Instant::now();
    let cores = thread::available_parallelism().map_or(1, |n| n.get()).min(total.max(1));
    let workers: Vec<_> = (0..cores)
        .map(|_| {
            let (queue, progress, finished, results, stop) =
                (Arc::clone(&queue), Arc::clone(&progress), Arc::clone(&finished), Arc::clone(&results), Arc::clone(&stop));
            let entry = entry.clone();
            thread::spawn(move || -> Result<(), String> {
                if queue.lock().unwrap().is_empty() {
                    return Ok(());
                }
                let mut engine = Engine::start(&entry)?;
                while !stop.load(Ordering::SeqCst) {
                    let Some((key, board)) = queue.lock().unwrap().pop_front() else { break };
                    let search = engine.search(&board.to_fen(1), DEPTH)?;
                    let eval = to_white(search.score, board.side_to_move());
                    append(&progress, key, &board, eval, search.depth)?;
                    results.lock().unwrap().push((key, (eval, search.depth)));
                    let count = finished.fetch_add(1, Ordering::SeqCst) + 1;
                    if count % 100 == 0 || count == total {
                        let elapsed = started.elapsed().as_secs_f64();
                        let left = elapsed / count as f64 * (total - count) as f64;
                        println!("  {count} / {total}, {:.1} positions/s, {:.0} min left", count as f64 / elapsed, left / 60.0);
                    }
                }
                Ok(())
            })
        })
        .collect();
    let mut failure = None;
    for worker in workers {
        if let Err(message) = worker.join().map_err(|_| "a worker panicked".to_string()).and_then(|result| result) {
            stop.store(true, Ordering::SeqCst);
            failure.get_or_insert(message);
        }
    }
    if let Some(message) = failure {
        return Err(message);
    }
    done.extend(results.lock().unwrap().drain(..));
    if done.len() < tree.children.len() {
        println!("Stopped: {} of {} positions evaluated; rerun to resume.", done.len(), tree.children.len());
        return Ok(());
    }

    let entries: Vec<Entry> = tree.children.keys().map(|key| {
        let (eval, depth) = done[key];
        Entry { key: *key, eval, depth }
    }).collect();
    let bytes = format::write(&Header { book_version: version, depth: DEPTH, engine: engine_id }, &entries);
    fs::write(&book_path, &bytes).map_err(|error| format!("cannot write {book_path}: {error}"))?;
    println!("Wrote {book_path}: {} positions, {} bytes, {:.0} s.", entries.len(), bytes.len(), started.elapsed().as_secs_f64());
    Ok(())
}

fn progress_header(engine_id: &str, depth: u8) -> String {
    format!("# {engine_id}, depth {depth}")
}

/// The rows of the progress file at `path` (none when it does not exist yet), refused when its
/// header names another engine or depth.
fn read_progress(path: &str, header_line: &str) -> Result<HashMap<u64, (Eval, u8)>, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(error) => return Err(format!("cannot read {path}: {error}")),
    };
    parse_progress(&text, header_line).map_err(|message| format!("{path}: {message}"))
}

fn parse_progress(text: &str, header_line: &str) -> Result<HashMap<u64, (Eval, u8)>, String> {
    let mut lines = text.lines();
    let first = lines.next().unwrap_or("");
    if first != header_line {
        return Err(format!("made by {first:?}, not {header_line:?}; delete it to start over"));
    }
    let mut done = HashMap::new();
    for line in lines.filter(|line| !line.trim().is_empty()) {
        let fields: Vec<&str> = line.split('\t').collect();
        // A row cut short by a crash is searched again.
        let [key, _fen, raw, depth] = fields[..] else { continue };
        let (Ok(key), Ok(raw), Ok(depth)) = (u64::from_str_radix(key, 16), raw.parse::<i16>(), depth.parse::<u8>()) else { continue };
        done.insert(key, (Eval::decode(raw), depth));
    }
    Ok(done)
}

fn open_progress(path: &str, header_line: &str) -> Result<File, String> {
    let exists = fs::metadata(path).is_ok();
    let mut file = OpenOptions::new().create(true).append(true).open(path).map_err(|error| format!("cannot open {path}: {error}"))?;
    if !exists {
        writeln!(file, "{header_line}").map_err(|error| format!("cannot write {path}: {error}"))?;
    }
    Ok(file)
}

/// One row: the key in hex, the FEN (for a human), the encoded evaluation, the depth.
fn append(progress: &Arc<Mutex<File>>, key: u64, board: &Board, eval: Eval, depth: u8) -> Result<(), String> {
    let mut file = progress.lock().unwrap();
    writeln!(file, "{key:016x}\t{}\t{}\t{depth}", board.to_fen(1), eval.encode())
        .and_then(|()| file.flush())
        .map_err(|error| format!("cannot append to the progress file: {error}"))
}

/// The engine's score, from the side to move, as the book keeps it: from White's side.
fn to_white(score: Score, side_to_move: Color) -> Eval {
    let sign = if side_to_move == Color::White { 1 } else { -1 };
    match score {
        Score::Cp(cp) => Eval::Cp(sign * cp),
        Score::Mate(n) => Eval::Mate(sign * n),
    }
}

/// A position with no legal move, evaluated by the rules: checkmate is a mate in 1 for the side
/// that gave it, stalemate a draw. `None` while there is a move to search.
fn rules_eval(board: &Board) -> Option<Eval> {
    let mover = board.side_to_move();
    if !legal_moves(board, mover).is_empty() {
        return None;
    }
    if !is_in_check(board, mover) {
        return Some(Eval::Cp(0));
    }
    Some(Eval::Mate(if mover == Color::White { -1 } else { 1 }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_score_turns_to_white() {
        assert_eq!(to_white(Score::Cp(30), Color::White), Eval::Cp(30));
        assert_eq!(to_white(Score::Cp(30), Color::Black), Eval::Cp(-30));
        assert_eq!(to_white(Score::Mate(2), Color::Black), Eval::Mate(-2));
    }

    #[test]
    fn the_rules_score_mate_and_stalemate() {
        // Fool's mate: White is mated.
        let (mated, _) = Board::from_fen("rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3").unwrap();
        assert_eq!(rules_eval(&mated), Some(Eval::Mate(-1)));
        let (stalemate, _) = Board::from_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").unwrap();
        assert_eq!(rules_eval(&stalemate), Some(Eval::Cp(0)));
        let (start, _) = Board::from_fen(pgn_replay::STARTING_POSITION_FEN).unwrap();
        assert_eq!(rules_eval(&start), None);
    }

    #[test]
    fn the_progress_resumes_and_refuses_another_engine() {
        let header = progress_header("Judge 1", 20);
        let text = format!("{header}\n00000000000000ff\tfen\t-35\t20\n0000000000000100\tfen\t31\n");
        let done = parse_progress(&text, &header).unwrap();
        assert_eq!(done.len(), 1, "a row cut short is searched again");
        assert_eq!(done[&0xff], (Eval::Cp(-35), 20));
        assert!(parse_progress(&text, &progress_header("Judge 2", 20)).is_err());
        assert!(parse_progress(&text, &progress_header("Judge 1", 18)).is_err());
    }
}
