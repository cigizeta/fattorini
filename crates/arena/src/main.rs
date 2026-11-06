//! `arena`: everything that plays games, and no libtorch. Its modes:
//!
//! - `arena match <champion> <challenger> <n>`, `sprt <champion> <challenger> <elo_d>`,
//!   `duel <champion> <challenger>`: two players named on the command line, the champion first
//!   (`match_player.rs`), under one of the three decision rules -- a best of
//!   `n` (`best_of.rs`), the sequential probability ratio test (`sprt.rs`), or which of the two is
//!   stronger (`duel.rs`) -- each with exactly one of `--depth <1-9>` or `--time <base+inc>`.
//! - `arena start|stop|status`: an arena, playing the smithy's self-play, round-robin, gate and tune
//!   chores (`chores.rs`), and taking a snapshot of the engine when its smithy asks
//!   (`snapshot.rs`).
//! - `arena <tournament-id>`: fitness-selection phase 1 scope only, a single pairing -- round
//!   1's first real (bye-free) match, per proper bracket seeding -- rather than a full bracket. The
//!   bracket scheduler is not built yet.
//!
//! Every rule plays through one runner (`runner.rs`). How many pairs are in flight is not an option:
//! half the logical cores under a clock, each pair pinned to its own physical core so parallel games don't distort
//! each other's clocks (`cpu_topology.rs`), and one pair per logical core at a fixed depth, where
//! timing does not matter.
//!
//! `smithy` and this binary talk only through the files `kalo_chess::pipeline` defines.

mod best_of;
mod bracket;
mod chores;
mod clock;
mod config;
mod cpu_topology;
mod ctrlc_handler;
mod data_root;
mod duel;
mod engines;
mod game;
mod in_process;
mod machine;
mod match_player;
mod memory;
mod pairing;
mod pgn;
mod player;
mod random_book;
mod results_db;
mod runner;
mod self_play;
mod snapshot;
mod sprt;
mod tablebase_label;
mod uci_client;

use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::process;

use rusqlite::Connection;

use best_of::BestOf;
use config::{ControlSpec, Rule, TournamentConfig};
use duel::Duel;
use engines::{EngineRegistry, ENGINE_REGISTRY_PATH};
use runner::{Labels, Placement, Resolution, Run};
use sprt::Sprt;

// Self-play's threads all allocate at once, and Windows' system allocator does not scale with them;
// the measurement is beside `mimalloc` in Cargo.toml. The engine `kalo-chess` keeps the system
// allocator: a GUI game searches with one thread unless told otherwise.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    let mut args = env::args();
    args.next(); // skip argv[0], the binary's own path
    let Some(first) = args.next() else {
        print_usage_and_exit();
    };
    // A tournament id never starts with a dash, so `--help`, `-h`, or any stray flag here gets the
    // usage rather than a panic about a missing `data/chess/tournaments/--help/config.yaml`.
    if first.starts_with('-') {
        print_usage_and_exit();
    }
    match first.as_str() {
        "start" | "stop" | "status" => chores::run(&first, &args.collect::<Vec<String>>()),
        "match" | "sprt" | "duel" => {
            let rest: Vec<String> = args.collect();
            let versus = parse_versus_args(&first, &rest).unwrap_or_else(|message| {
                eprintln!("{message}");
                print_usage_and_exit();
            });
            run_versus(&versus);
        }
        _ => run_tournament(&first),
    }
}

fn print_usage_and_exit() -> ! {
    eprintln!("usage: arena match <champion> <challenger> <n>        (--depth <d> | --time <base+inc>) [--hash <MB>]");
    eprintln!("       arena sprt  <champion> <challenger> <elo_d>    (--depth <d> | --time <base+inc>) [--hash <MB>]");
    eprintln!("       arena duel  <champion> <challenger>            (--depth <d> | --time <base+inc>) [--hash <MB>]");
    eprintln!("       arena start | stop | status");
    eprintln!("       arena <tournament-id>");
    eprintln!("  match plays a best of n games, n even: the first side with more than n/2 points wins, n/2 each after all n is drawn");
    eprintln!("  sprt  plays until the SPRT decides whether the challenger is at least elo_d stronger than the");
    eprintln!("        champion, at alpha = beta = 5%");
    eprintln!("  duel  plays pairs until one player's mean pair score passes 0.5 + 3.09 * sigma / sqrt(P), sigma");
    eprintln!("        the pair scores' deviation, drawn at 400 pairs (800 games), or as soon as neither could");
    eprintln!("        pass it by then whatever the pairs to come");
    eprintln!("  start plays the smithy's chores, self-play, round robins, gates and tunes, in the foreground until");
    eprintln!("        stopped: it idles on port 47823 until a smithy claims it, then serves that smithy alone, idle");
    eprintln!("        again once released; stop and status reach it from another terminal on this machine");
    eprintln!("  --depth <d>       a fixed depth from 1 to 9, one pair per logical core in parallel");
    eprintln!("  --time <base+inc> a Fischer clock, seconds plus seconds a move (180+2, 10+0.1), half the logical cores");
    eprintln!("                    in pairs, each pinned to its own physical core");
    eprintln!("  --hash <MB>       each player's transposition table, 1 to 1024, 16 by default; all the tables");
    eprintln!("                    together must stay under half the machine's RAM");
    eprintln!("  games are played in colour-reversed pairs");
    eprintln!("  a player is stockfish-<level> (0 to 20), stockfish-elo-<rating> (1320 to 3190), an engine of");
    eprintln!("  data/chess/engines.yaml, or a Kalo Chess player: pesto, <config>-g<g>-r<n>, generation g's winner in run n,");
    eprintln!("  or <config>-g<g>-l<lambda>-r<n>, that lambda's candidate in generation g");
    eprintln!("  a Kalo Chess player is always followed by what it searches with: @tuned, its tune, or @untuned,");
    eprintln!("  the defaults; against an external engine it plays over UCI, built first with both embedded");
    process::exit(1);
}

/// What `match`, `sprt` and `duel` were asked for: two players, a rule, a control, a table size.
struct Versus {
    champion: String,
    challenger: String,
    rule: Rule,
    control: ControlSpec,
    hash_mb: usize,
}

/// The arguments after `match`, `sprt` or `duel`: two players, champion then challenger, the rule's own value (`n` for `match`,
/// `elo_d` for `sprt`, nothing for `duel`), exactly one of `--depth <d>` or `--time <base+inc>`, and
/// optionally `--hash <MB>`, in any order after the positional arguments.
fn parse_versus_args(command: &str, args: &[String]) -> Result<Versus, String> {
    let usage = match command {
        "match" => {
            "match takes a champion and a challenger, an even n, one of --depth <d> or --time <base+inc>, and optionally \
             --hash <MB>"
        }
        "sprt" => {
            "sprt takes a champion and a challenger, elo_d, one of --depth <d> or --time <base+inc>, and optionally --hash \
             <MB>"
        }
        _ => "duel takes a champion and a challenger, one of --depth <d> or --time <base+inc>, and optionally --hash <MB>",
    };
    let positional = if command == "duel" { 2 } else { 3 };
    if args.len() < positional {
        return Err(usage.to_string());
    }
    let (positional_args, flag_args) = args.split_at(positional);
    let (champion, challenger, value) = match positional_args {
        [champion, challenger, value] => (champion, challenger, Some(value)),
        [champion, challenger] => (champion, challenger, None),
        _ => return Err(usage.to_string()),
    };
    let (control, hash_mb) = parse_flags(flag_args).map_err(|message| format!("{message}: {usage}"))?;
    let rule = match (command, value) {
        ("match", Some(value)) => {
            let n: u32 = value.parse().map_err(|_| format!("match: n must be a positive integer, got {value:?}"))?;
            Rule::best_of(n).map_err(|message| format!("match: {message}"))?
        }
        ("sprt", Some(value)) => {
            let elo_d: f64 = value.parse().map_err(|_| format!("sprt: elo_d must be a number of Elo, got {value:?}"))?;
            Rule::sprt(elo_d).map_err(|message| format!("sprt: {message}"))?
        }
        _ => Rule::Duel,
    };
    Ok(Versus { champion: champion.clone(), challenger: challenger.clone(), rule, control, hash_mb })
}

/// The flags after the positional arguments, as flag-value pairs: exactly one control, and `--hash`
/// at most once, from `HASH_MB_MIN` to `HASH_MB_MAX`, `DEFAULT_HASH_MB` when absent.
fn parse_flags(flags: &[String]) -> Result<(ControlSpec, usize), String> {
    let mut control = None;
    let mut hash_mb = None;
    for pair in flags.chunks(2) {
        let [flag, value] = pair else {
            return Err(format!("{} takes a value", pair[0]));
        };
        let repeated = match flag.as_str() {
            "--depth" | "--time" => {
                let parsed = if flag == "--depth" { ControlSpec::depth(value)? } else { ControlSpec::time(value)? };
                control.replace(parsed).is_some()
            }
            "--hash" => {
                let mb: usize = value
                    .parse()
                    .ok()
                    .filter(|mb| (kalo_chess::uci::HASH_MB_MIN..=kalo_chess::uci::HASH_MB_MAX).contains(mb))
                    .ok_or_else(|| {
                        format!("--hash is {} to {} MB, got {value:?}", kalo_chess::uci::HASH_MB_MIN, kalo_chess::uci::HASH_MB_MAX)
                    })?;
                hash_mb.replace(mb).is_some()
            }
            other => return Err(format!("unknown flag {other:?}")),
        };
        if repeated {
            return Err("exactly one of --depth and --time, and --hash at most once".to_string());
        }
    }
    let Some(control) = control else {
        return Err("one of --depth or --time is required".to_string());
    };
    Ok((control, hash_mb.unwrap_or(engines::DEFAULT_HASH_MB)))
}

/// Stops the run before anything starts unless every table of it fits (`memory.rs`).
fn check_memory(hash_mb: usize, threads: usize) {
    if let Err(message) = memory::check(hash_mb, 2 * threads) {
        eprintln!("{message}");
        process::exit(1);
    }
}

/// The line a folder's `spec.txt` holds: the rule by its value and the control, `duel at depth 9`. The
/// number of pairs in flight is not part of it: it changes how fast the games are played, not which
/// run they belong to, so a run can resume on another machine.
fn spec_line(rule: Rule, control: &ControlSpec) -> String {
    format!("{} at {}", rule.spec_text(), control.text)
}

/// The console prefix and outcome lines of a run under `rule`, `challenger` against `champion`.
fn versus_labels(rule: Rule, champion: &str, challenger: &str) -> Labels {
    match rule {
        Rule::BestOf(_) => Labels {
            prefix: "match",
            challenger_wins: format!("{challenger} wins the match"),
            champion_wins: format!("{champion} wins the match"),
            drawn: "the match is drawn".to_string(),
        },
        Rule::Sprt(spec) => Labels {
            prefix: "sprt",
            challenger_wins: format!("H1: {challenger} is at least {} Elo stronger than {champion}", spec.elo_d),
            champion_wins: format!("H0: {challenger} is not {} Elo stronger than {champion}", spec.elo_d),
            drawn: "the SPRT never draws".to_string(),
        },
        Rule::Duel => Labels {
            prefix: "duel",
            challenger_wins: format!("{challenger} is stronger than {champion}"),
            champion_wins: format!("{champion} is stronger than {challenger}"),
            // Not "in 1000 games": a curtailed duel draws earlier, and its status says so.
            drawn: "the duel is drawn: neither is shown stronger".to_string(),
        },
    }
}

/// Plays `run` under a fresh `rule`. The one place a rule becomes a concrete `SequentialTest`.
fn play_rule(
    rule: Rule,
    run: &Run,
    registry: &EngineRegistry,
    db_connection: &mut Connection,
    stop_requested: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Option<Resolution> {
    match rule {
        Rule::BestOf(n) => runner::play(run, BestOf::new(n), registry, db_connection, stop_requested),
        Rule::Sprt(spec) => {
            runner::play(run, Sprt::new(spec.elo_d, sprt::ALPHA, sprt::BETA), registry, db_connection, stop_requested)
        }
        Rule::Duel => runner::play(run, Duel::new(), registry, db_connection, stop_requested),
    }
}

/// `arena match | sprt | duel`: one run between two players named on the command line
/// (`match_player::resolve_player`), with no tournament config; an external engine comes from
/// `data/chess/engines.yaml` (`match_player::Engines`), read once for the run. Recorded in
/// a dated folder, `data/chess/matches/<champion>-vs-<challenger>/<YYYYMMDD-HHMM>/`, whose `spec.txt` names its rule and
/// control and which gets a `result.txt` when the run ends. Rerunning the same command resumes the
/// latest run if it has none, and otherwise starts a new one (`match_player::choose_match_dir`); a
/// rerun with another rule, value or control is refused rather than resuming it.
///
/// The three rules share everything but the rule itself. The SPRT is one-sided: it asks whether the
/// challenger is at least `elo_d` stronger than the champion, so the order matters. With pairs in parallel, under either
/// control, only whole pairs are recorded, so a Ctrl+C loses the pairs in flight; with one pair in
/// flight each game is recorded as it ends (`Run::record_half_pairs`), so it loses only the game.
fn run_versus(versus: &Versus) {
    let (champion, challenger) = (versus.champion.as_str(), versus.challenger.as_str());
    if champion == challenger {
        eprintln!("a run needs two different players, got {champion:?} twice");
        process::exit(1);
    }
    let placement = Run::placement(versus.control.control);
    check_memory(versus.hash_mb, placement.threads);
    let dir =
        match_player::choose_match_dir(&match_player::pairing_dir(champion, challenger), &match_player::current_timestamp());
    let stop_requested = ctrlc_handler::install_ctrlc_handler();
    if play_versus_in(versus, &dir, &placement, &stop_requested).is_none() {
        println!("interrupted -- re-run the same command to resume");
    }
}

/// Plays `versus` in the folder `dir`, created if new and resumed otherwise, and writes its
/// `result.txt` once the rule resolves. A folder whose games already resolve the rule plays nothing:
/// the runner replays them to the same verdict. `None` if interrupted first.
fn play_versus_in(
    versus: &Versus, dir: &str, placement: &Placement, stop_requested: &std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Option<Resolution> {
    let (champion, challenger, rule, control) =
        (versus.champion.as_str(), versus.challenger.as_str(), versus.rule, &versus.control);
    let engines = match_player::Engines::load().unwrap_or_else(|message| {
        eprintln!("{message}");
        process::exit(1);
    });
    let in_process = match_player::plays_in_process(champion, challenger, &engines);
    let mut entries = HashMap::new();
    for name in [champion, challenger] {
        let mut entry = match_player::resolve_player(name, in_process, &engines).unwrap_or_else(|message| {
            eprintln!("{message}");
            process::exit(1);
        });
        match_player::set_hash(&mut entry, versus.hash_mb);
        if let Err(message) = match_player::build_uci_player(name, &entry) {
            eprintln!("{message}");
            process::exit(1);
        }
        entries.insert(name.to_string(), entry);
    }
    std::fs::create_dir_all(dir).unwrap_or_else(|error| panic!("failed to create {dir}: {error}"));
    let spec = spec_line(rule, control);
    if let Err(message) = match_player::claim_spec(dir, &spec) {
        eprintln!("{message}");
        process::exit(1);
    }
    let registry = EngineRegistry::from_entries(entries, dir);
    let mut db_connection = results_db::open(&format!("{dir}/results.db"));
    let event_name = format!("{champion} vs {challenger}");
    let pgn_path = format!("{dir}/games.pgn");
    println!("{event_name}: {spec}, {} MB of hash per player, recorded in {dir}", versus.hash_mb);

    let run = Run {
        event_name: &event_name,
        round: 1,
        champion_name: champion,
        challenger_name: challenger,
        control: control.control,
        threads: placement.threads,
        pin_to: placement.pin_to.clone(),
        // Half pairs need one pair in flight; with several, whole pairs only.
        record_half_pairs: placement.threads == 1,
        labels: versus_labels(rule, champion, challenger),
        pgn_path: &pgn_path,
        report: None,
    };
    let resolution = play_rule(rule, &run, &registry, &mut db_connection, stop_requested)?;
    let verdict = run.labels.line(resolution.outcome);
    match_player::write_result(dir, &format!("{}\n{verdict}\n", resolution.summary));
    println!("{verdict}, recorded in {}", match_player::result_path(dir));
    Some(resolution)
}

fn run_tournament(tournament_id: &str) {
    let config_path = config::config_path(tournament_id);
    let config = TournamentConfig::load(&config_path);
    assert_eq!(
        config.format, "single-elimination",
        "unsupported tournament format {:?} in {config_path} -- only \"single-elimination\" is implemented so far",
        config.format
    );

    // Phase 1 scope: the bracket scheduler isn't built yet (see the tournament design's
    // "Implementation phasing" section) -- this plays only round 1's first real pairing, per
    // proper bracket seeding (see `bracket.rs`), proving out the match runner itself ahead of
    // the scheduler that will eventually drive it across a whole bracket. Byes are skipped
    // rather than picked, so this is never the tournament's top two seeds -- those can only meet
    // in the final.
    let pairings = bracket::round_one_pairings(&config.participants);
    let first_pairing = pairings.first().expect("a tournament with at least 2 participants has at least one round-1 pairing");
    let engine_one_name = &first_pairing.engine_one;
    let engine_two_name = &first_pairing.engine_two;

    let registry = EngineRegistry::load(ENGINE_REGISTRY_PATH);

    let tournament_dir = PathBuf::from(config::tournament_dir(tournament_id));
    let pgn_path = tournament_dir.join("games.pgn");
    let db_path = tournament_dir.join("results.db");
    let mut db_connection = results_db::open(db_path.to_str().expect("tournament directory path is not valid UTF-8"));

    let stop_requested = ctrlc_handler::install_ctrlc_handler();

    println!("{}: round 1, {engine_one_name} vs {engine_two_name}", config.name);

    let winner = pairing::play_pairing(
        &config.name,
        1,
        engine_one_name,
        engine_two_name,
        &registry,
        &config.match_spec,
        pgn_path.to_str().expect("tournament directory path is not valid UTF-8"),
        &mut db_connection,
        &stop_requested,
    );

    match winner {
        Some(pairing::MatchOutcome::Won(winner)) => println!("round 1 decided: {winner}"),
        Some(pairing::MatchOutcome::Drawn) => unreachable!("a tournament always has a tie-break, so a pairing is never drawn"),
        None => println!("interrupted -- re-run the same command to resume"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    fn parsed(command: &str, list: &[&str]) -> Versus {
        parse_versus_args(command, &args(list)).unwrap_or_else(|message| panic!("{command} {list:?}: {message}"))
    }

    fn line_of(command: &str, list: &[&str]) -> String {
        let versus = parsed(command, list);
        spec_line(versus.rule, &versus.control)
    }

    #[test]
    fn each_command_takes_the_champion_then_the_challenger_its_value_and_one_control() {
        let versus = parsed("sprt", &["stockfish-elo-1500", "amanda-g1-r1@tuned", "35", "--time", "3+2"]);
        assert_eq!((versus.champion.as_str(), versus.challenger.as_str()), ("stockfish-elo-1500", "amanda-g1-r1@tuned"));
        assert_eq!(versus.rule, Rule::Sprt(config::SprtSpec { elo_d: 35.0 }));
        assert_eq!(versus.control.text, "3+2");
        assert_eq!(versus.hash_mb, 16, "the default");
        assert_eq!(parsed("match", &["a", "b", "12", "--depth", "9"]).rule, Rule::BestOf(12));
        assert_eq!(parsed("duel", &["a", "b", "--depth", "1"]).rule, Rule::Duel);
    }

    #[test]
    fn hash_is_optional_and_may_come_before_or_after_the_control() {
        assert_eq!(parsed("duel", &["a", "b", "--depth", "9", "--hash", "64"]).hash_mb, 64);
        assert_eq!(parsed("duel", &["a", "b", "--hash", "1024", "--time", "3+2"]).hash_mb, 1024);
        for list in [
            &["a", "b", "--depth", "9", "--hash", "0"][..],
            &["a", "b", "--depth", "9", "--hash", "1025"][..],
            &["a", "b", "--depth", "9", "--hash", "x"][..],
            &["a", "b", "--depth", "9", "--hash", "16", "--hash", "32"][..],
            &["a", "b", "--depth", "9", "--hash"][..],
            &["a", "b", "--hash", "16"][..],
        ] {
            assert!(parse_versus_args("duel", &args(list)).is_err(), "{list:?}");
        }
    }

    /// No default control, exactly one, with its value; no `--threads`; an even n; a positive elo_d;
    /// `duel` takes no value.
    #[test]
    fn each_command_refuses_anything_else() {
        for (command, list) in [
            ("match", &["a", "b", "12"][..]),
            ("match", &["a", "b", "--depth", "9"][..]),
            ("match", &["a", "b", "7", "--depth", "9"][..]),
            ("match", &["a", "b", "0", "--depth", "9"][..]),
            ("match", &["a", "b", "x", "--depth", "9"][..]),
            ("match", &["a", "b", "12", "--depth", "9", "--time", "3+2"][..]),
            ("match", &["a", "b", "12", "--threads", "4"][..]),
            ("sprt", &["a", "b", "0", "--depth", "9"][..]),
            ("sprt", &["a", "b", "x", "--time", "3+2"][..]),
            ("sprt", &["a", "b", "35", "--depth", "10"][..]),
            ("sprt", &["a", "b", "35", "--depth", "9", "--threads", "4"][..]),
            ("duel", &["a", "b"][..]),
            ("duel", &["a", "b", "35", "--depth", "9"][..]),
            ("duel", &["a", "b", "--depth", "0"][..]),
            ("duel", &["a", "b", "--time", "3"][..]),
        ] {
            assert!(parse_versus_args(command, &args(list)).is_err(), "{command} {list:?}");
        }
    }

    #[test]
    fn the_spec_line_names_the_rule_by_its_value_and_the_control() {
        assert_eq!(line_of("match", &["a", "b", "12", "--time", "3+2"]), "best-of 12 at 3+2");
        assert_eq!(line_of("sprt", &["a", "b", "35", "--time", "3+2"]), "sprt elo_d 35 at 3+2");
        assert_eq!(line_of("sprt", &["a", "b", "10", "--depth", "9"]), "sprt elo_d 10 at depth 9");
        assert_eq!(line_of("duel", &["a", "b", "--depth", "9"]), "duel at depth 9");
        assert_eq!(
            line_of("sprt", &["a", "b", "35", "--time", "3+2"]),
            line_of("sprt", &["a", "b", "35.0", "--time", "3+2"]),
            "the value, not its spelling"
        );
    }
}
