//! `smithy book-probe <engine> <moves>`: one position expanded as a fragment would expand it,
//! every search shown (move, score, loss, admitted or not, depth, nodes, time). For judging the
//! threshold on a few critical positions; writes nothing.

use std::fs;

use fattorini::board::Board;
use fattorini::movegen::legal::legal_moves;
use fattorini::pipeline::data_root::data_root;
use fattorini::uci::parse_move;

use super::engine::{self, Engine};
use super::expand::expand;
use super::fragment::{DEPTH, THRESHOLD};

pub fn run(engine_name: &str, moves: &[String]) -> Result<(), String> {
    let registry = fs::read_to_string(format!("{}/engines.yaml", data_root()))
        .map_err(|error| format!("cannot read {}: {error}", engine::ENGINE_REGISTRY_PATH))?;
    let entry = engine::registry_entry(&registry, engine_name)?;
    let (mut board, _) = Board::from_fen(pgn_replay::STARTING_POSITION_FEN).expect("the starting position parses");
    for text in moves {
        let legal = legal_moves(&board, board.side_to_move());
        let mv = parse_move(text, &legal).ok_or_else(|| format!("{text} is not a legal move in {}", board.to_fen(1)))?;
        board.make_move(mv);
    }
    let mut engine = Engine::start(&entry)?;
    println!("{} at depth {DEPTH}, threshold {THRESHOLD} points", engine.name);
    println!("{}", board.to_fen(1));
    let expansion = expand(&board, &mut engine, DEPTH, THRESHOLD)?;
    println!("{:>4}  {:<6}  {:>9}  {:>6}  {:<8}  {:>5}  {:>12}  {:>8}", "#", "move", "score", "loss", "", "depth", "nodes", "time");
    for (number, (search, lost)) in expansion.trace.iter().enumerate() {
        let admitted = if *lost < THRESHOLD { "admitted" } else { "stops" };
        println!(
            "{:>4}  {:<6}  {:>9}  {:>6.2}  {:<8}  {:>5}  {:>12}  {:>7.1}s",
            number + 1,
            search.best_move,
            format!("{:?}", search.score),
            lost,
            admitted,
            search.depth,
            search.nodes,
            search.time.as_secs_f64()
        );
    }
    let nodes: u64 = expansion.trace.iter().map(|(search, _)| search.nodes).sum();
    let seconds: f64 = expansion.trace.iter().map(|(search, _)| search.time.as_secs_f64()).sum();
    println!(
        "{} moves admitted of {} legal; {} searches, {nodes} nodes, {seconds:.1} s.",
        expansion.moves.len(),
        legal_moves(&board, board.side_to_move()).len(),
        expansion.trace.len()
    );
    Ok(())
}
