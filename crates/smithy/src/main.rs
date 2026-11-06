//! `smithy`: everything that touches tensors and datasets, and so the one binary that needs
//! libtorch. `start` runs the smithy, whose jobs, started from its page, train every network,
//! generation by generation, or tune a player's search parameters, every game played by the arenas
//! (`arena start`), make a source's corpus, Lc0's converted or PeSTO's played, or make an opening
//! book; and ask an arena to install a player for lichess-bot. The two binaries talk only through the
//! files and requests `temper_chess::pipeline` defines.

// MSVC's linker reports the import library it creates for smithy's exported symbols (the one
// `build.rs` forces in to keep `torch_cuda.dll` linked): a side product, nothing to act on.
#![allow(linker_messages)]

// The pipeline contract, shared with `arena`.
use temper_chess::pipeline::{preprocessing_config, remote};

mod cache;
mod game_labels;
mod lc0;
mod bucketed_train_stream;
mod grouped_test_set;
mod pgn_stream;
mod pgn_train_stream;

// The one-network optimizer: training steps every arm through `StackedAdamW`, which is tested to
// match it and reads and writes its files.
#[cfg(test)]
mod adamw;
mod archive;
mod arm_slice;
mod batch;
mod book;
mod candidates;
mod checkpoint_store;
mod export;
mod feature_set;
mod freeze;
mod history;
mod lease;
mod lockstep;
mod matches;
mod net_config;
mod nnue_network;
mod pesto;
mod pipeline;
mod polyline;
mod round;
mod round_robin;
mod rounds_display;
mod run_config;
mod scale;
mod smithy;
mod sprt;
mod stack_head;
mod stacked_adamw;
mod stacked_network;
mod stacked_transformer;
mod stages;
mod test_cache;
mod train_stream;
mod transformer_batch;
mod transformer_network;
mod transformer_vocab;
mod tree;
mod buckets;
mod tasks;
mod tune;
mod value_head;

use std::env;
use std::process;

fn main() {
    let mut args = env::args();
    args.next(); // skip argv[0], the binary's own path
    let subcommand = args.next();
    let rest: Vec<String> = args.collect();

    match subcommand.as_deref() {
        // The Convert job's child, started by the smithy alone: in no usage line.
        Some(lc0::CHILD_COMMAND) => lc0::run(&rest),
        // The one-off rewrite of Lc0's corpus into Lc0's curve, run by hand once: in no usage line.
        Some(lc0::rewrite::REWRITE_COMMAND) => lc0::rewrite::run(&rest),
        // A read-only look at a corpus's buckets, run by hand: in no usage line.
        Some(polyline::DIAGNOSE_COMMAND) => polyline::run_diagnose(&rest),
        // The book job's child, likewise.
        Some(book::CHILD_COMMAND) => book::run(&rest),
        Some(verb @ ("start" | "stop" | "status")) => smithy::run(verb, &rest),
        _ => {
            eprintln!("{}", USAGE);
            process::exit(1);
        }
    }
}

const USAGE: &str = "usage: smithy start | stop | status\n       \
     (the smithy, on any machine, in the foreground, on 127.0.0.1\n        \
     and the LAN, every game played by the arenas it claims on the machines that file lists, each\n        \
     started by hand (arena start); its jobs are started and stopped from its page opened on this machine,\n        \
     http://127.0.0.1:47821/: a forge, the training run of a configs/chess/ config, full where CUDA is or\n        \
     scaled anywhere on the CPU, a tune of a player's search\n        \
     parameters, a test, or PeSTO's Play, one at a time; beside them Lc0's Convert, which converts Lc0's\n        \
     training tars into data/chess/lc0 up to configs/chess/lc0.yaml's cap, or a book job, which makes an opening\n        \
     book of a PGN in data/chess/books/ on Drive; and an install of a player for lichess-bot, on\n        \
     any machine whose arena it claims. stop and status reach it from another terminal on the same\n        \
     machine)";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_usage_offers_no_install_command() {
        assert!(!USAGE.contains("smithy install"), "an install is asked from the page");
    }

    #[test]
    fn the_usage_offers_no_dev_smithy() {
        assert!(!USAGE.contains("--dev"), "a scaled run is asked from the page, on any smithy");
    }

    #[test]
    fn the_usage_offers_no_lc0_command() {
        assert!(!USAGE.contains("smithy lc0") && !USAGE.contains(lc0::CHILD_COMMAND), "Lc0's conversion is a job of the page");
    }
}
