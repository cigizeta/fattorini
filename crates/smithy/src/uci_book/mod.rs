//! A book judged by any UCI engine: the positions of Lichess's chess-openings files, each
//! evaluated by an engine of `data/engines.yaml` at a fixed depth, then turned into a Polyglot
//! book whose weights are a softmax of those evaluations.

mod build;
mod engine;
mod format;
mod polyglot;
mod tree;

use std::process;

/// `smithy uci-book <engine> <version>`, run by hand: in no usage line.
pub const BUILD_COMMAND: &str = "uci-book";
/// `smithy uci-book-polyglot <evaluations file> [--temperature <T>]`, likewise.
pub const POLYGLOT_COMMAND: &str = "uci-book-polyglot";

pub fn run_build(args: &[String]) {
    let parsed = match args {
        [engine, version] => version.parse::<u16>().ok().filter(|&v| v > 0).map(|version| (engine, version)),
        _ => None,
    };
    let Some((engine, version)) = parsed else {
        exit_with(&format!("usage: smithy {BUILD_COMMAND} <engine of data/engines.yaml> <book version, 1 to 65535>"));
    };
    if let Err(message) = build::run(engine, version) {
        exit_with(&message);
    }
}

pub fn run_polyglot(args: &[String]) {
    let parsed = match args {
        [file] => Some((file, polyglot::DEFAULT_TEMPERATURE)),
        [file, flag, value] if flag == "--temperature" => value.parse::<f64>().ok().map(|t| (file, t)),
        _ => None,
    };
    let Some((file, temperature)) = parsed else {
        exit_with(&format!("usage: smithy {POLYGLOT_COMMAND} <evaluations file> [--temperature <cp, default 20>]"));
    };
    if let Err(message) = polyglot::run(file, temperature) {
        exit_with(&message);
    }
}

fn exit_with(message: &str) -> ! {
    eprintln!("{message}");
    process::exit(1);
}
