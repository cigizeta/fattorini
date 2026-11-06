//! Fixed-depth, time-measured search benchmark: wall-clock time to complete a fixed depth,
//! not nodes/second and not depth-reached-in-a-fixed-time. Both alternatives were tried and
//! rejected -- nps alone can look like a regression exactly when pruning is working (pruning
//! adds per-node bookkeeping while cutting total nodes searched far more), and node count
//! grows roughly exponentially with depth, so a real but modest improvement usually isn't
//! enough to cross into one more whole completed depth within a fixed budget. Fixing the
//! depth and measuring time instead moves proportionally with any real improvement, however
//! small. Not a `criterion` benchmark either, for the same reason: this isn't chasing a
//! sub-1% regression, just the order-of-magnitude change real pruning should produce, so one
//! deterministic pass per position is enough.
//!
//! Runs each position through an external UCI engine -- Kalo Chess's own compiled binary by
//! default, or any other engine via `--engine PATH` -- single-threaded with a fixed hash
//! size, rather than calling into Kalo Chess's `search` module in-process: one engine, one code
//! path, one way of measuring every engine compared this way, so the comparison is never
//! skewed by different engines being measured differently. Reading Kalo Chess's own node/time
//! numbers back over UCI also keeps this benchmark honest about what a real `go depth N`
//! reports, the same numbers a GUI would see.
//!
//! Usage: `cargo bench -- --depth N` (the leading `--` forwards the flag past `cargo` itself
//! to this binary; without it `cargo bench` tries to parse `--depth` as its own argument).
//! Defaults to depth 4 when no flag is given.
//!
//! `--engine PATH` (space-separated, same as `--depth N`) runs the suite through that
//! external UCI engine instead of Kalo Chess -- one engine per run, not a combined report, so
//! comparing several is just diffing each run's own `total` line (e.g. Kalo Chess, Stockfish, and
//! a third UCI engine, three separate invocations). The report line's own engine name is
//! taken from `PATH`'s file name, not hardcoded, so it's accurate for whichever binary is
//! passed -- `--engine stockfish` (resolved via `$PATH`, same as typing `stockfish` at a
//! shell) reports as "stockfish", `--engine /some/other/engine` reports as "other-engine".
//! Running Kalo Chess itself (the default, no flag needed) reads its compiled `[[bin]]` from
//! `target/release/kalo-chess`, which `cargo bench` does not build
//! automatically -- run `cargo build --release` first.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

const TT_SIZE_MB: usize = 64;
const DEFAULT_DEPTH: u32 = 4;

use kalo_chess::bench_positions::POSITIONS;

/// Reads `--depth N` from the process's own arguments (not `cargo bench`'s -- the `--`
/// separator on the command line forwards everything after it straight to this binary).
/// Falls back to `DEFAULT_DEPTH` if the flag is missing or malformed, rather than erroring:
/// this is a diagnostic tool run by hand, not something a bad invocation should hard-fail.
fn parse_depth_arg() -> u32 {
    let args: Vec<String> = std::env::args().collect();
    for i in 0..args.len() {
        if args[i] == "--depth"
            && let Some(value) = args.get(i + 1)
                && let Ok(depth) = value.parse::<u32>() {
                    return depth;
                }
    }
    DEFAULT_DEPTH
}

/// Reads `--engine PATH` from the process's own arguments, space-separated exactly like
/// `parse_depth_arg`'s own `--depth N` -- not `--engine=PATH`, an earlier, inconsistent choice
/// inherited from an even earlier `--stockfish`/`--stockfish=PATH` flag (where `=` disambiguated
/// a path from the bare flag's own "use the default name `stockfish`" meaning). `--engine` has
/// no such bare form -- there's no universal default binary name to fall back to across
/// arbitrary engines the way `"stockfish"` was a reasonable one for Stockfish specifically -- so
/// `=`'s original justification never applied here, and this project's own file-wide
/// consistency matters more than preserving an inherited convention that no longer earns its
/// keep. `Some(path)` means run the suite through that external UCI engine instead of Kalo Chess;
/// `None` means the flag wasn't passed, the original default, Kalo Chess alone.
fn parse_engine_arg() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    for i in 0..args.len() {
        if args[i] == "--engine"
            && let Some(path) = args.get(i + 1) {
                return Some(path.clone());
            }
    }
    None
}

/// Path to Kalo Chess's own compiled `[[bin]]`, used when `--engine` isn't given. `cargo bench`
/// only builds this benchmark's own binary, not `[[bin]] kalo-chess` -- running Kalo Chess over UCI
/// needs that binary to already exist, so this is a `cargo build --release` prerequisite,
/// not something this benchmark builds for you. `CARGO_MANIFEST_DIR` is baked in at compile
/// time, so the resulting path is correct regardless of where `cargo bench` is actually
/// invoked from. The workspace's `target/` is two levels above this crate.
fn default_kalo_chess_binary_path() -> String {
    format!("{}/../../target/release/kalo-chess{}", env!("CARGO_MANIFEST_DIR"), std::env::consts::EXE_SUFFIX)
}

/// Formats `n` with `,` as a thousands separator (e.g. `23426862` -> `"23,426,862"`).
/// `std::fmt` has no built-in support for this, and pulling in a crate for one cosmetic
/// benchmark line isn't worth a new dependency.
fn with_thousands(n: u128) -> String {
    let digits = n.to_string();
    let mut result = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(c);
    }
    result
}

/// Writes one UCI command line to the child's stdin. A tiny helper only because every call
/// site below would otherwise repeat the same `writeln!` + `expect` pair.
fn send_uci(stdin: &mut std::process::ChildStdin, command: &str) {
    writeln!(stdin, "{command}").expect("failed to write to the engine's stdin");
}

/// Runs one suite entry through an external UCI engine at a fixed depth -- Kalo Chess and
/// Stockfish alike, both spoken to the same way: single-threaded, `TT_SIZE_MB` MB of hash,
/// searched via `go depth N`. `entry` is passed straight through as `position fen <entry>`
/// -- it already has any trailing `moves ...` in exactly the shape UCI's own `position`
/// command expects (`POSITIONS`' own doc comment), so no FEN parsing is needed on this end
/// at all.
///
/// Reads `nodes`/`time` off the engine's own last `info depth` line before `bestmove`,
/// rather than timing the round-trip with `Instant`: the subprocess's own reported numbers
/// are what a real comparison should use, so process spawn and pipe overhead on this end
/// doesn't leak into the result -- and both engines are measured the identical way, so
/// neither is disadvantaged by IPC overhead the other doesn't also pay.
fn run_uci_engine(binary: &str, depth: u32, entry: &str) -> (u128, u64) {
    let mut child = Command::new(binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to start '{binary}': {e}"));

    let mut stdin = child.stdin.take().expect("child stdin was requested as piped");
    let stdout = child.stdout.take().expect("child stdout was requested as piped");
    let mut lines = BufReader::new(stdout).lines();

    // `for line in &mut lines` (not `lines`) borrows the iterator instead of consuming it,
    // so each of these loops picks up exactly where the previous one left off in the
    // engine's output stream, rather than each starting a fresh read from the top.
    send_uci(&mut stdin, "uci");
    for line in &mut lines {
        if line.expect("failed to read the engine's stdout") == "uciok" {
            break;
        }
    }

    send_uci(&mut stdin, "setoption name Threads value 1");
    send_uci(&mut stdin, &format!("setoption name Hash value {TT_SIZE_MB}"));
    send_uci(&mut stdin, "isready");
    for line in &mut lines {
        if line.expect("failed to read the engine's stdout") == "readyok" {
            break;
        }
    }

    send_uci(&mut stdin, &format!("position fen {entry}"));
    send_uci(&mut stdin, &format!("go depth {depth}"));

    let mut nodes = 0u64;
    let mut elapsed_ms = 0u128;
    for line in &mut lines {
        let line = line.expect("failed to read the engine's stdout");
        if line.starts_with("info depth") {
            // Read as `nodes`/`time` fields, whichever token they follow -- neither engine's
            // own `info` field order is a documented UCI guarantee, so this doesn't assume a
            // fixed position within the line the way parsing `POSITIONS`' own fixed,
            // hand-verified shape could.
            let tokens: Vec<&str> = line.split_whitespace().collect();
            for pair in tokens.windows(2) {
                match pair[0] {
                    "nodes" => nodes = pair[1].parse().unwrap_or(nodes),
                    "time" => elapsed_ms = pair[1].parse().unwrap_or(elapsed_ms),
                    _ => {}
                }
            }
        }
        if line.starts_with("bestmove") {
            break;
        }
    }

    send_uci(&mut stdin, "quit");
    child.wait().expect("engine did not exit cleanly");

    (elapsed_ms, nodes)
}

fn main() {
    let depth = parse_depth_arg();
    let engine_binary = parse_engine_arg();

    // The report line's own engine name: derived from `path`'s own file name for `--engine`
    // (so an arbitrary third engine reports as itself, not as a hardcoded "stockfish"), or
    // the plain `"kalo-chess"` literal for the no-flag default -- `default_kalo_chess_binary_path()`'s
    // own file name would technically also just be `"kalo-chess"`, but naming it directly here
    // avoids depending on that path's own shape for something this simple.
    let (engine_name, binary) = match &engine_binary {
        Some(path) => {
            let name = std::path::Path::new(path).file_name().and_then(|f| f.to_str()).unwrap_or(path).to_string();
            (name, path.clone())
        }
        None => ("kalo-chess".to_string(), default_kalo_chess_binary_path()),
    };
    println!(
        "search_bench: engine {engine_name} ({binary}), depth {depth}, {} positions, {TT_SIZE_MB} MB hash, single-threaded",
        POSITIONS.len()
    );

    let mut total_time_ms: u128 = 0;
    let mut total_nodes: u64 = 0;

    for (index, entry) in POSITIONS.iter().enumerate() {
        let (elapsed_ms, nodes) = run_uci_engine(&binary, depth, entry);

        total_time_ms += elapsed_ms;
        total_nodes += nodes;

        println!(
            "{:>2}: {:>10} ms  {:>14} nodes  {entry}",
            index + 1,
            with_thousands(elapsed_ms),
            with_thousands(nodes as u128)
        );
    }

    let total_nps = total_nodes as u128 * 1000 / total_time_ms.max(1);

    println!(
        "total: {} ms, {} nodes ({} nps) across {} positions at depth {depth}",
        with_thousands(total_time_ms),
        with_thousands(total_nodes as u128),
        with_thousands(total_nps),
        POSITIONS.len()
    );
}
