//! The opening book: grown from the initial position by Dijkstra over the game graph, each
//! position expanded by a UCI engine of `data/engines.yaml`, in fragments of a rising budget, then
//! turned into a Polyglot book whose weights are a softmax of each position's move scores.

mod dijkstra;
mod engine;
mod expand;
mod format;
mod fragment;
mod polyglot;
mod probe;
mod unit;

use std::process;

/// `smithy book-fragment <engine>`, run by hand: in no usage line.
pub const FRAGMENT_COMMAND: &str = "book-fragment";
/// `smithy book-polyglot <engine> [--temperature <T>]`, likewise.
pub const POLYGLOT_COMMAND: &str = "book-polyglot";
/// `smithy book-probe <engine> <moves>`, likewise.
pub const PROBE_COMMAND: &str = "book-probe";

pub fn run_fragment(args: &[String]) {
    let [engine] = args else {
        exit_with(&format!("usage: smithy {FRAGMENT_COMMAND} <engine of data/engines.yaml>"));
    };
    if let Err(message) = fragment::run(engine) {
        exit_with(&message);
    }
}

pub fn run_polyglot(args: &[String]) {
    let parsed = match args {
        [engine] => Some((engine, polyglot::DEFAULT_TEMPERATURE)),
        [engine, flag, value] if flag == "--temperature" => value.parse::<f64>().ok().map(|t| (engine, t)),
        _ => None,
    };
    let Some((engine, temperature)) = parsed else {
        exit_with(&format!("usage: smithy {POLYGLOT_COMMAND} <engine> [--temperature <cp, default 20>]"));
    };
    if let Err(message) = polyglot::run(engine, temperature) {
        exit_with(&message);
    }
}

pub fn run_probe(args: &[String]) {
    let Some((engine, moves)) = args.split_first() else {
        exit_with(&format!("usage: smithy {PROBE_COMMAND} <engine> [moves from the initial position, in UCI]"));
    };
    if let Err(message) = probe::run(engine, moves) {
        exit_with(&message);
    }
}

fn exit_with(message: &str) -> ! {
    eprintln!("{message}");
    process::exit(1);
}
