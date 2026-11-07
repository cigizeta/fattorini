//! The runner behind `arena match`, `sprt`, `duel` and `gate`: a challenger against a champion
//! in colour-reversed opening pairs until the decision rule resolves. The rules themselves are
//! `best_of.rs`, `sprt.rs` and `duel.rs`, each behind `SequentialTest`; this module is the play that
//! feeds them, and knows nothing about any one of them.
//!
//! **Roles, not positions.** A rule may be one-sided -- the SPRT asks whether the challenger is at
//! least `elo_d` stronger -- so the two names are not interchangeable, and everything here names them
//! by role. Wherever the two are shown, the champion is listed first and the challenger second.
//! `main.rs`'s `run_selection` derives both from the network's current generation, so no hand-written
//! config can swap them; the other commands take them in command-line order, champion first.
//!
//! How many pairs are in flight follows from the control (`Run::placement`): half the logical cores
//! under a clock, each pair pinned to its own physical core so parallel games don't distort each
//! other's clocks;
//! one pair per logical core at a fixed depth, where timing does not matter. With several pairs in
//! flight only whole pairs are recorded; with one, each game as it ends (`Run::record_half_pairs`).
//! No moves are printed: games are watched on lichess or in the cutechess GUI, and how time was
//! spent is read from the PGN.
//!
//! Why this is not a mode of `pairing.rs`: a tournament pairing has tie-break pairs and decides every
//! game on its own. This plays pairs and stops on a rule. The two share a game (`game.rs`) and the way
//! a player is built (`pairing::start_player`), and nothing else.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rusqlite::Connection;

use fattorini::board::color::Color;

use super::random_book::RandomBook;
use super::cpu_topology;
use super::data_root;
use super::engines::EngineRegistry;
use super::game::{self, PlyOutcome};
use super::pairing;
use super::pgn::{self, MoveAnnotation, Termination};
use super::player::{Control, Player};
use super::results_db::{self, GameRecord};
use super::sprt::{Sprt, Verdict};

/// How often the interrupt mirror re-checks Ctrl+C. Short enough that an interrupt aborts the game in
/// progress within a move, long enough to be free: a run lasts hours.
const INTERRUPT_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// How a run ended. `Drawn` is a rule's own verdict (a level best of n, a duel at its cap), never an
/// interruption.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    ChallengerWins,
    ChampionWins,
    Drawn,
}

/// A decision rule over games fed one at a time, from the challenger's side. The runner feeds it
/// whole pairs only, two calls per pair, and asks for the outcome after each pair.
pub trait SequentialTest {
    /// One game from the challenger's side: `1.0`, `0.5` or `0.0`, and nothing else.
    fn add_game(&mut self, challenger_score: f64);
    /// `None` while undecided.
    fn outcome(&self) -> Option<Outcome>;
    /// Wins, losses and draws, from the challenger's side.
    fn record(&self) -> (u64, u64, u64);
    /// The challenger's points.
    fn score(&self) -> f64;
    fn games(&self) -> u64;
    /// The rule's own state, printed after every pair: the LLR and its bounds, the score against the
    /// threshold, or the match score.
    fn status(&self) -> String;
    /// The whole rule as one line, printed when a run starts, so its log records which rule produced
    /// its verdict.
    fn describe(&self) -> String;
    /// How many pairs the rule needs at most, for a rule that only counts: the runner then starts no
    /// pair beyond it, so the run plays exactly that many and none is discarded unfinished. `None`
    /// for a rule that stops on the evidence.
    fn pair_limit(&self) -> Option<u64> {
        None
    }
    /// The most pairs the rule can take before it must resolve, for the time left printed after each
    /// pair: `pair_limit` by default, a cap for a rule that also stops on the evidence, `None` for a
    /// rule with no bound (the SPRT).
    fn most_pairs(&self) -> Option<u64> {
        self.pair_limit()
    }
    /// The pairs the rule likely still needs, for the time left: `most_pairs` less those read, by
    /// default; a rule that can say better (`BestOf`) does.
    fn pairs_left_estimate(&self) -> Option<u64> {
        Some(self.most_pairs()?.saturating_sub(self.games() / 2))
    }
}

/// A running score in half-points plus the record behind it: all `BestOf` and `Duel` need. An integer
/// so the accumulation stays exact over thousands of games.
pub struct Tally {
    score_half: u64,
    wins: u64,
    losses: u64,
    draws: u64,
}

impl Tally {
    pub fn new() -> Tally {
        Tally { score_half: 0, wins: 0, losses: 0, draws: 0 }
    }

    pub fn add_game(&mut self, challenger_score: f64) {
        match challenger_score {
            1.0 => {
                self.wins += 1;
                self.score_half += 2;
            }
            0.5 => {
                self.draws += 1;
                self.score_half += 1;
            }
            0.0 => self.losses += 1,
            other => panic!("a game's score must be 1.0, 0.5 or 0.0, got {other}"),
        }
    }

    pub fn record(&self) -> (u64, u64, u64) {
        (self.wins, self.losses, self.draws)
    }

    pub fn score(&self) -> f64 {
        self.score_half as f64 / 2.0
    }

    pub fn games(&self) -> u64 {
        self.wins + self.losses + self.draws
    }
}

/// The SPRT as a rule: `sprt.rs`'s derivation untouched, its two bounds mapped onto the outcomes.
/// It never draws.
impl SequentialTest for Sprt {
    fn add_game(&mut self, challenger_score: f64) {
        Sprt::add_game(self, challenger_score);
    }

    fn outcome(&self) -> Option<Outcome> {
        match self.verdict() {
            Verdict::Continue => None,
            Verdict::Promote => Some(Outcome::ChallengerWins),
            Verdict::Fail => Some(Outcome::ChampionWins),
        }
    }

    fn record(&self) -> (u64, u64, u64) {
        Sprt::record(self)
    }

    fn score(&self) -> f64 {
        Sprt::score(self)
    }

    fn games(&self) -> u64 {
        Sprt::games(self)
    }

    fn status(&self) -> String {
        format!("llr {:+.3} [{:+.3}, {:+.3}]", self.llr(), self.lower_bound(), self.upper_bound())
    }

    fn describe(&self) -> String {
        format!("sprt: {}", Sprt::describe(self))
    }
}

/// Everything a run is, apart from its rule, where its players come from and where its rows go.
pub struct Run<'a> {
    pub event_name: &'a str,
    /// The rows' `round`: always `1`.
    pub round: u32,
    pub champion_name: &'a str,
    pub challenger_name: &'a str,
    pub control: Control,
    /// Pairs in flight at once.
    pub threads: usize,
    /// The CPU each worker pins itself to, one per worker, or `None` for no pinning.
    pub pin_to: Option<Vec<usize>>,
    /// Record a pair's first game as soon as it ends, and on resume finish a pair that has only its
    /// first game. Only sound with one pair in flight: see `play`.
    pub record_half_pairs: bool,
    pub labels: Labels,
    pub pgn_path: &'a str,
    /// Called with the challenger's record (wins, losses, draws) after each recorded pair, for a
    /// caller that draws its own display (the round robin's live table). The runner then prints
    /// none of its own lines, which would scroll that display away; errors still go to stderr.
    /// `None` prints as usual.
    pub report: Option<&'a (dyn Fn((u64, u64, u64)) + Sync)>,
}

/// Where a pairing's games are written and under which names: what recording a pair needs of a run,
/// shared with the round robin's interleaved schedule, which records many pairings' pairs from one
/// pool of workers.
pub struct Recording<'a> {
    pub event_name: &'a str,
    pub round: u32,
    pub champion_name: &'a str,
    pub challenger_name: &'a str,
    pub pgn_path: &'a str,
}

impl<'a> Run<'a> {
    pub fn recording(&self) -> Recording<'a> {
        Recording {
            event_name: self.event_name,
            round: self.round,
            champion_name: self.champion_name,
            challenger_name: self.challenger_name,
            pgn_path: self.pgn_path,
        }
    }
}

/// Prints `text` unless the run reports to its caller instead (`Run::report`).
fn say(run: &Run, text: String) {
    if run.report.is_none() {
        println!("{text}");
    }
}

/// How many pairs are in flight, and the CPU each is pinned to.
pub struct Placement {
    pub threads: usize,
    pub pin_to: Option<Vec<usize>>,
}

impl Run<'_> {
    /// The placement under `control`: at a fixed depth, one pair per logical core, unpinned; under a
    /// clock, half the logical cores in pairs, each pinned to its own physical core.
    pub fn placement(control: Control) -> Placement {
        let logical = std::thread::available_parallelism().map_or(1, |cores| cores.get());
        match control {
            Control::FixedDepth(_) => Placement { threads: logical, pin_to: None },
            Control::Fischer(_) => clock_placement(cpu_topology::physical_core_cpus(), logical),
        }
    }
}

/// A clock's placement: half the logical cores in pairs, each pinned to its own physical core, the
/// lowest-numbered ones. A pair per physical core contended too much on a 20-core machine (shared cache,
/// memory bandwidth, all-core turbo), and under a clock contention costs depth, so it can move results.
/// Never more pairs than physical cores. If the topology could not be read, the same count, unpinned.
fn clock_placement(physical_cores: Option<Vec<usize>>, logical: usize) -> Placement {
    let half = (logical / 2).max(1);
    match physical_cores {
        Some(mut cpus) => {
            cpus.truncate(half);
            Placement { threads: cpus.len(), pin_to: Some(cpus) }
        }
        None => {
            eprintln!("could not read the CPU topology: half the logical cores, unpinned");
            Placement { threads: half, pin_to: None }
        }
    }
}

/// How a run names itself and its outcomes on the console.
pub struct Labels {
    /// `match`, `sprt`, `duel` or `gate`, the start of every line this module prints.
    pub prefix: &'static str,
    pub challenger_wins: String,
    pub champion_wins: String,
    pub drawn: String,
}

impl Labels {
    pub fn line(&self, outcome: Outcome) -> &str {
        match outcome {
            Outcome::ChallengerWins => &self.challenger_wins,
            Outcome::ChampionWins => &self.champion_wins,
            Outcome::Drawn => &self.drawn,
        }
    }
}

/// How a run that reached a verdict ended: the outcome, and the record behind it as one line.
pub struct Resolution {
    pub outcome: Outcome,
    /// `+W -L =D, score/games, <status>`, from the challenger's side.
    pub summary: String,
}

/// One finished game, held until its partner finishes so the two can be written together, or
/// written at once when half pairs are recorded (`recorded`).
pub struct FinishedGame {
    game_index: u32,
    white_name: String,
    black_name: String,
    starting_fen: String,
    /// White's own score.
    result: f64,
    termination: Termination,
    ply_count: u32,
    moves: Vec<MoveAnnotation>,
    /// Already in the PGN and the database.
    recorded: bool,
}

impl FinishedGame {
    /// The challenger's own score for this game, which is what the rule consumes. Derived from
    /// `game_index` parity, the same rule `pairing::standings` uses: the challenger is White in the
    /// even-indexed game of each pair.
    pub fn challenger_score(&self) -> f64 {
        let challenger_was_white = self.game_index.is_multiple_of(2);
        if challenger_was_white { self.result } else { 1.0 - self.result }
    }
}

/// A pair whose first game is recorded and whose second is not, found on resume.
struct HalfPair {
    pair_index: u32,
    /// The recorded game's `game_index`; the game still to play is the pair's other one.
    game_index: u32,
    /// The recorded game's score from the challenger's side, fed to the rule with its partner.
    challenger_score: f64,
}

/// Everything the workers share, behind one lock. One mutex rather than several because the things
/// it protects have to move together: a pair's rows, its PGN, and the two rule updates are one event,
/// and a reader that saw the rule advance but not the rows would be looking at a database that
/// disagrees with the verdict.
///
/// Contention is not a concern at any plausible scale -- a pair takes seconds to minutes and the
/// critical section is two inserts and a formatted line.
struct Shared<'a, T> {
    test: T,
    /// The next pair's index. It numbers the pairs and so their `game_index`es; it does not pick the
    /// opening, which is drawn at random per pair.
    next_pair_index: usize,
    /// A half pair to finish before any new pair, taken by the first worker to ask.
    pending_half: Option<(HalfPair, String)>,
    /// Pairs handed to a worker and not yet recorded or abandoned, so a rule with a `pair_limit`
    /// starts no pair beyond it.
    pairs_in_flight: u64,
    /// Recorded pairs not yet fed to the rule, by pair index, with the challenger's two scores: a pair
    /// waits here until every pair below it is fed (`record_pair`).
    waiting: BTreeMap<u32, Vec<f64>>,
    /// The pair index the rule reads next.
    next_to_feed: u32,
    connection: &'a mut Connection,
    /// Set when the rule resolves, so the other workers finish their current pair and stop rather
    /// than starting another.
    resolved: Option<Outcome>,
    /// When this run started and how many pairs the rule had already read then, so the time left
    /// is paced on this run's own pairs only, never on a resumed run's earlier ones.
    started: Instant,
    pairs_fed_at_start: u64,
}

/// `seconds` in whole minutes, or in seconds below 90 (the developer, 2026-10-16), where a fast
/// fixed-depth match spends most of its life: rounded minutes would show "1 min" for anything from 30
/// to 89 s. From 90 s up the minutes round to 2 or more, so "1 min" never appears.
fn minutes_or_seconds(seconds: f64) -> String {
    if seconds < 90.0 {
        return format!("{seconds:.0} s");
    }
    format!("{:.0} min", seconds / 60.0)
}

/// The time left at this run's pace so far: the pairs the rule likely still needs
/// (`pairs_left_estimate`), at the average time per pair read since this run started. `None` before
/// this run's first pair, or for a rule with no bound.
fn time_left<T: SequentialTest>(state: &Shared<'_, T>) -> Option<Duration> {
    let remaining = state.test.pairs_left_estimate()?;
    let fed = state.test.games() / 2;
    let fed_here = fed.saturating_sub(state.pairs_fed_at_start);
    if fed_here == 0 {
        return None;
    }
    Some(state.started.elapsed().mul_f64(remaining as f64 / fed_here as f64))
}

/// Plays the run to a verdict under `test`, a fresh rule, or returns `None` if it was interrupted
/// before the rule resolved.
///
/// **Half pairs, and why only with one pair in flight.** At a fixed depth only whole pairs are
/// recorded: with several workers several pairs are half-played at any moment and finish out of
/// order, and one could finish after the rule resolved, so its games would be in the database without
/// being part of the verdict. With one game in flight none of that can happen: at most one pair is
/// half-played, always the last, and no pair starts after resolution. So `record_half_pairs` writes a
/// first game as soon as it ends, the rule still sees it only with its partner (`replay` consumes
/// whole pairs only), and a resume plays the missing game first. A Ctrl+C then loses only the game in
/// flight.
pub fn play<T: SequentialTest + Send>(
    run: &Run,
    test: T,
    registry: &EngineRegistry,
    db_connection: &mut Connection,
    stop_requested: &Arc<AtomicBool>,
) -> Option<Resolution> {
    assert!(
        !run.record_half_pairs || run.threads == 1,
        "half pairs can only be recorded with one pair in flight, got {} threads",
        run.threads
    );
    let prefix = run.labels.prefix;

    // Rebuilt from the recorded games rather than from any persisted counter: the rule is a pure
    // function of the results, so there is no second piece of state that could disagree with them.
    let existing = results_db::pairing_game_rows(db_connection, run.round, run.challenger_name, run.champion_name);
    let Resumed { test, completed_pairs, mut next_pair_index, trailing_half } = replay(&existing, run.challenger_name, test);
    // A caller drawing its own display starts from the recorded games, however many.
    if let Some(report) = run.report {
        report(test.record());
    }
    if completed_pairs > 0 {
        say(run, format!("{prefix}: resuming from {completed_pairs} recorded pairs: {}", summary(&test)));
        if next_pair_index > completed_pairs {
            say(
                run,
                format!(
                    "{prefix}: note: {} pair index(es) were assigned but never completed, so the next is {next_pair_index}",
                    next_pair_index - completed_pairs
                ),
            );
        }
    }
    // A run with pairs in parallel never writes a half pair, so one found in its database is an old
    // fragment and stays skipped, as `replay` already skipped it. Only a run that records half pairs
    // finishes one.
    let pending_half = match trailing_half {
        Some(half) if run.record_half_pairs => {
            let starting_fen = pairing::last_starting_fen(run.pgn_path);
            say(
                run,
                format!(
                    "{prefix} {} vs {}: pair {} has only its first game recorded, so its second is played first",
                    run.champion_name,
                    run.challenger_name,
                    half.pair_index + 1
                ),
            );
            next_pair_index = half.pair_index as usize;
            Some((half, starting_fen))
        }
        _ => None,
    };
    // What each side searches with, as a suffixed name already shows it: a gate's players read theirs
    // from the files beside their networks, and nothing else in the output says which one played.
    let label = |name: &str| -> String { registry.resolve(name).label(name) };
    say(
        run,
        format!(
            "{prefix}: {} (champion) vs {} (challenger), {}",
            label(run.champion_name),
            label(run.challenger_name),
            test.describe()
        ),
    );
    let pinning = if run.pin_to.is_some() { ", pinned to physical cores" } else { "" };
    say(run, format!("{prefix}: {} pair(s) in flight{pinning}, {:?}", run.threads, run.control));

    // A resumed run whose recorded games already resolve the rule is finished: report and stop, rather
    // than spawning workers that each start their players only to break on their first check. This is
    // also the path that makes the verdict stable across a restart -- it reaches the same conclusion
    // from the same rows.
    if let Some(outcome) = test.outcome() {
        say(run, format!("{prefix}: {} (from the recorded games)", run.labels.line(outcome)));
        return Some(Resolution { outcome, summary: summary(&test) });
    }
    // Every index below `next_pair_index` is either fed already or abandoned for good, so the rule
    // reads on from there.
    let pairs_fed_at_start = test.games() / 2;
    let shared = Mutex::new(Shared {
        test,
        next_pair_index,
        pending_half,
        pairs_in_flight: 0,
        waiting: BTreeMap::new(),
        next_to_feed: next_pair_index as u32,
        connection: db_connection,
        resolved: None,
        started: Instant::now(),
        pairs_fed_at_start,
    });

    let tablebase = pairing::load_tablebase();
    // One scan of the book for the whole run; each worker then reads it through its own handle.
    let book = RandomBook::open(&data_root::book_path());
    let started = Instant::now();

    // The one signal every worker and every game in progress watches: "stop now, for any reason".
    // Set when the rule resolves, when Ctrl+C arrives, or when a player fails to start.
    //
    // A single flag rather than the workers checking two, because `play_one_game` consults it once per
    // ply and so aborts a game in progress within one move -- which is what makes a resolution stop
    // the run *immediately* instead of after the pairs still in flight play themselves out. Those
    // games are not part of the verdict, so finishing them is pure waste.
    let finish = Arc::new(AtomicBool::new(false));

    // Scoped threads so the workers can borrow `shared`, `registry`, the book and the names without
    // any of it having to be cloned into an `Arc` or given a `'static` lifetime.
    std::thread::scope(|scope| {
        // Mirrors Ctrl+C into `finish`. Without it the workers would watch one flag and the games
        // another, and an interrupt would only take effect between games.
        let interrupt_mirror = {
            let finish = Arc::clone(&finish);
            scope.spawn(move || {
                while !finish.load(Ordering::SeqCst) {
                    if stop_requested.load(Ordering::SeqCst) {
                        finish.store(true, Ordering::SeqCst);
                        return;
                    }
                    std::thread::sleep(INTERRUPT_POLL_INTERVAL);
                }
            })
        };

        let workers: Vec<_> = (0..run.threads)
            .map(|worker| {
                let shared = &shared;
                let tablebase = &tablebase;
                let book = &book;
                let finish = &finish;
                scope.spawn(move || {
                    run_worker(worker, run, registry, tablebase, book, shared, finish);
                })
            })
            .collect();

        // Joined here rather than left to the scope's own join at the end of this closure, because the
        // mirror has to be released *after* the workers are done and before this closure returns --
        // the scope would otherwise wait forever on a mirror still polling a flag nobody will set.
        for worker in workers {
            worker.join().expect("a worker panicked");
        }
        finish.store(true, Ordering::SeqCst);
        interrupt_mirror.join().expect("the interrupt mirror panicked");
    });

    let shared = shared.into_inner().expect("no worker should have poisoned the lock");
    say(
        run,
        format!(
            "{prefix}: {} games in {}, {}",
            shared.test.games(),
            minutes_or_seconds(started.elapsed().as_secs_f64()),
            summary(&shared.test)
        ),
    );
    match shared.resolved {
        Some(outcome) => {
            say(run, format!("{prefix}: {}", run.labels.line(outcome)));
            Some(Resolution { outcome, summary: summary(&shared.test) })
        }
        None => {
            say(run, format!("{prefix}: interrupted before the rule resolved, resumable"));
            None
        }
    }
}

/// The record behind a rule, from the champion's side, the side named first on the command line and
/// in every line: `+W -L =D` rather than only a score, since two runs with the same score can be a
/// drawish match and a bloodbath. The points follow as `champion - challenger`, both sides' totals
/// rather than the champion's out of the games played. The rules count from the challenger's side;
/// only the display turns.
fn summary<T: SequentialTest>(test: &T) -> String {
    let (challenger_wins, challenger_losses, draws) = test.record();
    let challenger_score = test.score();
    let champion_score = test.games() as f64 - challenger_score;
    let points = format!("+{challenger_losses} -{challenger_wins} ={draws}, {champion_score:.1} - {challenger_score:.1}");
    let status = test.status();
    if status.is_empty() {
        return points;
    }
    format!("{points}, {status}")
}

/// What a resumed run needs from the recorded games: the rule rebuilt from them, how many whole pairs
/// that was, which pair index to hand out next, and a trailing half pair if the last pair has one game.
struct Resumed<T> {
    test: T,
    completed_pairs: usize,
    /// **Derived from the highest index used, not from the count of recorded pairs.** A run stopped
    /// with pairs in flight leaves gaps -- workers are handed pair indexes in order but finish out of
    /// order, so pairs 0..20 may be assigned while only 1, 2, 3, 8 and 15 were recorded. Taking the
    /// count would reissue a pair index already used and write a second row with a `game_index` that
    /// already exists, which nothing in the schema forbids and which would then break the pair
    /// grouping below.
    next_pair_index: usize,
    /// The highest recorded pair, when it has exactly one game. Not consumed by the rule either way.
    trailing_half: Option<HalfPair>,
}

/// Rebuilds the rule from the recorded games, feeding them to `test`, a fresh one.
///
/// **Only whole pairs are consumed.** A colour-unbalanced fragment is not something a rule should
/// consume, whether it is an old fragment of a parallel run or the first game of a pair still waiting
/// for its partner. The last pair's fragment is reported as `trailing_half` so a run recording half
/// pairs can finish it; any other is skipped, its pair index never reused.
fn replay<T: SequentialTest>(rows: &[results_db::GameRow], challenger_name: &str, mut test: T) -> Resumed<T> {
    let mut by_pair: BTreeMap<u32, Vec<&results_db::GameRow>> = BTreeMap::new();
    for row in rows {
        by_pair.entry(row.game_index / 2).or_default().push(row);
    }

    // From the recorded colour, not from index parity: the row says who was White, so there is no
    // convention here that a malformed database could invert silently.
    let challenger_score =
        |row: &results_db::GameRow| if row.white == challenger_name { row.result } else { 1.0 - row.result };

    // In index order, as `record_pair` feeds them, and no further than the first resolution: pairs
    // recorded past it finished early and waited, and were never part of the verdict.
    let mut completed_pairs = 0;
    for games in by_pair.values() {
        if test.outcome().is_some() {
            break;
        }
        if games.len() != 2 {
            continue;
        }
        completed_pairs += 1;
        for row in games {
            test.add_game(challenger_score(row));
        }
    }

    let next_pair_index = by_pair.keys().next_back().map_or(0, |highest| *highest as usize + 1);
    let trailing_half = by_pair.iter().next_back().filter(|(_, games)| games.len() == 1).map(|(pair_index, games)| {
        HalfPair { pair_index: *pair_index, game_index: games[0].game_index, challenger_score: challenger_score(games[0]) }
    });
    Resumed { test, completed_pairs, next_pair_index, trailing_half }
}

/// One worker: build both players once, then take pairs until the rule resolves or the run is
/// interrupted.
///
/// The players are built once per worker and reused across pairs, not rebuilt per pair -- an
/// in-process player's transposition table is the expensive part of it, and `new_game` clears that
/// between games anyway, which is the thing correctness actually requires.
fn run_worker<T: SequentialTest>(
    worker: usize,
    run: &Run,
    registry: &EngineRegistry,
    tablebase: &Arc<fattorini::tablebase::SyzygyTablebase>,
    book: &RandomBook,
    shared: &Mutex<Shared<T>>,
    finish: &Arc<AtomicBool>,
) {
    let prefix = run.labels.prefix;
    // Before the players start, so both engine processes inherit the pin.
    if let Some(cpu) = run.pin_to.as_ref().and_then(|cpus| cpus.get(worker))
        && let Err(message) = cpu_topology::pin_current_thread(*cpu) {
            eprintln!("{prefix}: worker {worker} plays unpinned: {message}");
        }
    let Ok(mut challenger) = pairing::start_player(run.challenger_name, registry, tablebase) else {
        eprintln!("{prefix}: worker {worker} could not start {}, stopping the run", run.challenger_name);
        finish.store(true, Ordering::SeqCst);
        return;
    };
    let Ok(mut champion) = pairing::start_player(run.champion_name, registry, tablebase) else {
        eprintln!("{prefix}: worker {worker} could not start {}, stopping the run", run.champion_name);
        finish.store(true, Ordering::SeqCst);
        challenger.shutdown();
        return;
    };
    let mut book_reader = book.reader();
    let mut rng = rand::rng();

    loop {
        if finish.load(Ordering::SeqCst) {
            break;
        }
        let (pair_index, half) = {
            let mut state = shared.lock().expect("the lock should not be poisoned");
            if state.resolved.is_some() {
                break;
            }
            let taken = match state.pending_half.take() {
                Some((half, starting_fen)) => (half.pair_index, Some((half, starting_fen))),
                None => {
                    // Counted against the recorded pairs plus those in flight, not against the pair
                    // index: a resumed run has gaps in its indexes, which still need filling.
                    if let Some(limit) = state.test.pair_limit()
                        && state.test.games() / 2 + state.waiting.len() as u64 + state.pairs_in_flight >= limit {
                            break;
                        }
                    let pair_index = state.next_pair_index as u32;
                    state.next_pair_index += 1;
                    (pair_index, None)
                }
            };
            state.pairs_in_flight += 1;
            taken
        };

        // A fresh pair draws its opening at random; a half pair replays the one its first game used.
        let (starting_fen, earlier_score, games_to_play): (String, Option<f64>, Vec<u32>) = match half {
            Some((half, starting_fen)) => {
                let missing = if half.game_index % 2 == 0 { 1 } else { 0 };
                (starting_fen, Some(half.challenger_score), vec![missing])
            }
            None => (book.random_fen(&mut book_reader, &mut rng), None, vec![0, 1]),
        };

        let recording = run.recording();
        // The pair's first game, written now rather than with its partner, so an interrupt during the
        // second loses only that one. The rule still waits for the partner.
        let mut record_first = |finished_game: &mut FinishedGame| {
            if run.record_half_pairs && earlier_score.is_none() {
                record_game(&recording, shared, finished_game);
                finished_game.recorded = true;
            }
        };
        let finished = play_games(
            prefix, worker, &recording, run.control, pair_index, &games_to_play, &starting_fen, &mut *challenger,
            &mut *champion, None, finish, &mut record_first,
        );

        // **Only a whole pair reaches the rule.** A pair cut short by Ctrl+C or a fault is not fed to
        // it. A run with pairs in parallel drops it entirely, including a completed first game -- that
        // game is a real result, but keeping it would break the invariant that its database holds only
        // whole pairs. A run recording half pairs has already written its first game, and a resume
        // finishes it.
        let games_in_pair = finished.len() + usize::from(earlier_score.is_some());
        if games_in_pair < 2 {
            shared.lock().expect("the lock should not be poisoned").pairs_in_flight -= 1;
            break;
        }
        record_pair(run, shared, &finished, earlier_score, finish);
    }

    challenger.shutdown();
    champion.shutdown();
}

/// Plays the games `games_to_play` (`0`, `1`, or both, in that order) of pair `pair_index` from
/// `starting_fen`, between two players already started, and returns those that finished, in order.
/// Fewer than asked means the pair was cut short: by `finish` (a Ctrl+C, or the rule resolving), or
/// by a player faulting, which also raises `finish` and stops the run. `after_first` sees the first
/// game played as soon as it ends, before the second starts.
///
/// Game `2k` of a pair has the challenger as White, game `2k + 1` reverses it: the parity rule
/// `FinishedGame::challenger_score` and `replay` both read back. `early_stop`, the tables, ends games
/// early (`game::play_one_game`): a tune's games only.
#[allow(clippy::too_many_arguments)]
pub fn play_games(
    prefix: &str, worker: usize, recording: &Recording, control: Control, pair_index: u32, games_to_play: &[u32],
    starting_fen: &str, challenger: &mut dyn Player, champion: &mut dyn Player,
    early_stop: Option<&fattorini::tablebase::SyzygyTablebase>, finish: &Arc<AtomicBool>,
    after_first: &mut dyn FnMut(&mut FinishedGame),
) -> Vec<FinishedGame> {
    let mut finished: Vec<FinishedGame> = Vec::with_capacity(2);
    for &game_in_pair in games_to_play {
        let game_index = pair_index * 2 + game_in_pair;
        let challenger_is_white = game_in_pair == 0;
        let (white_name, black_name) = if challenger_is_white {
            (recording.challenger_name, recording.champion_name)
        } else {
            (recording.champion_name, recording.challenger_name)
        };
        let (white_player, black_player): (&mut dyn Player, &mut dyn Player) =
            if challenger_is_white { (&mut *challenger, &mut *champion) } else { (&mut *champion, &mut *challenger) };

        // A player that faults here ends the run: there is no score for the fault to decide, because
        // the rule is about playing strength and a crashed opponent is not evidence about it.
        if white_player.new_game().is_err() || black_player.new_game().is_err() {
            eprintln!("{prefix}: worker {worker} lost a player between games, stopping the run");
            finish.store(true, Ordering::SeqCst);
            break;
        }

        match game::play_one_game(game_index + 1, starting_fen, white_player, black_player, control, false, early_stop, finish) {
            PlyOutcome::Decided(outcome) => {
                let mut finished_game = FinishedGame {
                    game_index,
                    white_name: white_name.to_string(),
                    black_name: black_name.to_string(),
                    starting_fen: starting_fen.to_string(),
                    result: outcome.result,
                    termination: outcome.termination,
                    ply_count: outcome.ply_count,
                    moves: outcome.moves,
                    recorded: false,
                };
                if finished.is_empty() {
                    after_first(&mut finished_game);
                }
                finished.push(finished_game);
            }
            PlyOutcome::Interrupted => break,
            PlyOutcome::Fault { faulting_color, message, .. } => {
                let faulting_name = if faulting_color == Color::White { white_name } else { black_name };
                eprintln!("{prefix}: worker {worker} saw {faulting_name} fault ({message}), stopping the run");
                finish.store(true, Ordering::SeqCst);
                break;
            }
        }
    }
    finished
}


/// Writes a whole pair: both rows in one transaction, which is what keeps a parallel run's database
/// holding only whole pairs, then both games to the PGN.
pub fn write_pair(connection: &mut Connection, recording: &Recording, first: &FinishedGame, second: &FinishedGame) {
    results_db::insert_pair(connection, &game_record(recording, first), &game_record(recording, second));
    append_pgn(recording, first);
    append_pgn(recording, second);
}

fn game_record<'a>(run: &Recording<'a>, finished_game: &'a FinishedGame) -> GameRecord<'a> {
    GameRecord {
        round: run.round,
        engine_one: run.challenger_name,
        engine_two: run.champion_name,
        game_index: finished_game.game_index,
        white: &finished_game.white_name,
        black: &finished_game.black_name,
        result: finished_game.result,
        termination: finished_game.termination,
        ply_count: finished_game.ply_count,
    }
}

pub fn append_pgn(run: &Recording, finished_game: &FinishedGame) {
    pgn::append_to_file(run.pgn_path, &pgn_text(run, finished_game));
}

/// `finished_game` as `append_pgn` writes it, for a caller that sends it rather than keeps it.
pub fn pgn_text(run: &Recording, finished_game: &FinishedGame) -> String {
    pgn::format_game(
        &pgn::GameHeaders {
            event: run.event_name.to_string(),
            round: run.round,
            white: finished_game.white_name.clone(),
            black: finished_game.black_name.clone(),
            starting_fen: finished_game.starting_fen.clone(),
        },
        &finished_game.moves,
        pairing::pgn_result(finished_game.result),
        finished_game.termination,
        true,
    )
}

/// Writes one game on its own: the PGN first, then its row, so a row in the database always has its
/// opening in the PGN, where a resume reads it back.
fn record_game<T>(run: &Recording, shared: &Mutex<Shared<T>>, finished_game: &FinishedGame) {
    let mut state = shared.lock().expect("the lock should not be poisoned");
    append_pgn(run, finished_game);
    results_db::insert_game(state.connection, &game_record(run, finished_game));
}

/// Writes whatever of the pair is not written yet, then feeds the rule every recorded pair whose turn
/// has come, in index order, printing each one's line -- all under one lock, in that order, so the
/// database is never behind the verdict.
/// `earlier_score` is the challenger's score in a first game recorded before a resume, which this run
/// never held as a `FinishedGame`.
fn record_pair<T: SequentialTest>(
    run: &Run,
    shared: &Mutex<Shared<T>>,
    finished: &[FinishedGame],
    earlier_score: Option<f64>,
    finish: &Arc<AtomicBool>,
) {
    let prefix = run.labels.prefix;
    let mut state = shared.lock().expect("the lock should not be poisoned");
    state.pairs_in_flight -= 1;
    // Counted from 1 on screen, as games are: pair `k` is games `2k - 1` and `2k`.
    let pair_number = finished[0].game_index / 2 + 1;

    // **A pair that finishes after the rule resolved is discarded, not recorded.** When the rule
    // resolves there are up to `threads - 1` pairs still in flight, and they are not part of the
    // verdict: the stopping rule is "stop at the first resolution", so games played after it are not
    // evidence the verdict rests on.
    //
    // Recording them would break the property that makes resuming correct. The rule's state is
    // deliberately never persisted -- it is recomputed from the recorded games -- and that is only
    // sound while the database holds exactly the games the rule consumed. Post-resolution games can
    // move it back (observed on the SPRT: an LLR of +2.949 at the crossing, +2.929 two pairs later),
    // so a resumed run would replay them, read no verdict, and continue a run that had already
    // finished. With one pair in flight this cannot happen: the pair that resolved the rule is the
    // last one started.
    if state.resolved.is_some() {
        say(
            run,
            format!(
                "{prefix} {} vs {}: pair {pair_number:>4}  discarded, the rule had already resolved",
                run.champion_name, run.challenger_name
            ),
        );
        return;
    }

    let recording = run.recording();
    let unrecorded: Vec<&FinishedGame> = finished.iter().filter(|finished_game| !finished_game.recorded).collect();
    match unrecorded.as_slice() {
        [first, second] => write_pair(state.connection, &recording, first, second),
        [only] => {
            append_pgn(&recording, only);
            results_db::insert_game(state.connection, &game_record(&recording, only));
        }
        _ => {}
    }

    // **The rule reads pairs in index order, not in the order they finish.** With many pairs in flight
    // the first to finish are the shortest games, mostly quick decisive ones, so a rule fed in
    // completion order could resolve on a sample of short games alone (a gate once passed 10-0 on the
    // first 5 pairs out of 20 in flight). Pair indexes are handed out before any game is played, so
    // index order is independent of how the games go. A pair finished early waits for the pairs below
    // it; it is already recorded, and `replay` reads the database in the same order and stops at the
    // same resolution, so pairs past it in the database never move a resumed verdict.
    let scores: Vec<f64> = earlier_score.into_iter().chain(finished.iter().map(FinishedGame::challenger_score)).collect();
    state.waiting.insert(finished[0].game_index / 2, scores);
    loop {
        let next = state.next_to_feed;
        let Some(scores) = state.waiting.remove(&next) else {
            break;
        };
        state.next_to_feed += 1;
        // Two ordinary per-game updates, one per game: a completed pair is not a pair-level
        // observation.
        for &score in &scores {
            state.test.add_game(score);
        }

        // No per-game lines are printed, so the pair's line names the pair and carries the record.
        // The champion's, as every number printed (`summary`).
        let pair_score: f64 = 2.0 - scores.iter().sum::<f64>();
        let eta = match time_left(&state) {
            Some(left) => format!(", ETA {}", minutes_or_seconds(left.as_secs_f64())),
            None => String::new(),
        };
        say(
            run,
            format!(
                "{prefix} {} vs {}: pair {:>4}  {pair_score:.1}/2   {}{eta}",
                run.champion_name,
                run.challenger_name,
                next + 1,
                summary(&state.test)
            ),
        );
        if let Some(report) = run.report {
            report(state.test.record());
        }

        // Announced the moment it happens, not only when the workers have drained. Without this a
        // parallel run looks like it is ignoring the rule: pairs keep completing for a while as the
        // other workers finish, and nothing says why.
        if let Some(outcome) = state.test.outcome() {
            state.resolved = Some(outcome);
            let status = state.test.status();
            let status = if status.is_empty() { String::new() } else { format!(", {status}") };
            say(run, format!("{prefix}: resolved at {} games{status} -- {}", progress::grouped(state.test.games()), run.labels.line(outcome)));
            // Every game in progress sees this within a move and abandons. Those games are not part of
            // the verdict, so playing them out would be waste -- and recording them would be worse, see
            // above.
            finish.store(true, Ordering::SeqCst);
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::sprt;

    #[test]
    fn a_duration_is_in_seconds_below_90_and_never_1_min() {
        assert_eq!(minutes_or_seconds(42.4), "42 s");
        assert_eq!(minutes_or_seconds(89.4), "89 s");
        assert_eq!(minutes_or_seconds(90.0), "2 min");
        assert_eq!(minutes_or_seconds(19.3 * 60.0), "19 min");
    }

    fn fresh() -> Sprt {
        Sprt::new(35.0, sprt::ALPHA, sprt::BETA)
    }

    fn finished_game(game_index: u32, white_result: f64) -> FinishedGame {
        FinishedGame {
            game_index,
            white_name: "w".to_string(),
            black_name: "b".to_string(),
            starting_fen: String::new(),
            result: white_result,
            termination: Termination::Checkmate,
            ply_count: 0,
            moves: Vec::new(),
            recorded: false,
        }
    }

    /// The parity rule, which is the one thing shared between playing a pair and replaying it. An
    /// error here would silently invert the rule's sign in the reversed game of every pair.
    #[test]
    fn the_challenger_is_white_in_the_even_game_of_a_pair() {
        assert_eq!(finished_game(0, 1.0).challenger_score(), 1.0, "even game: White is the challenger");
        assert_eq!(finished_game(1, 1.0).challenger_score(), 0.0, "odd game: White is the champion");
        assert_eq!(finished_game(2, 0.0).challenger_score(), 0.0);
        assert_eq!(finished_game(3, 0.0).challenger_score(), 1.0);
        assert_eq!(finished_game(5, 0.5).challenger_score(), 0.5, "a draw is a draw either way round");
    }

    const CHALLENGER: &str = "amanda";
    const CHAMPION: &str = "pesto";

    /// `white` carries the colour, so a row is built from the index and who had White.
    fn row(game_index: u32, white: &str, white_result: f64) -> results_db::GameRow {
        results_db::GameRow {
            game_index,
            white: white.to_string(),
            result: white_result,
        }
    }

    /// A resumed pairing counts whole pairs only and hands out the next index past the highest used,
    /// so the gaps and the half pair a Ctrl+C leaves are neither counted nor reused.
    #[test]
    fn a_resume_counts_whole_pairs_and_hands_out_past_the_highest_index() {
        // Pairs 0 and 3 whole, pair 5 half, 1, 2 and 4 never recorded.
        let rows = vec![
            row(0, CHALLENGER, 1.0),
            row(1, CHAMPION, 0.5),
            row(6, CHALLENGER, 0.0),
            row(7, CHAMPION, 1.0),
            row(10, CHALLENGER, 0.5),
        ];
        let resumed = replay(&rows, CHALLENGER, fresh());
        assert_eq!((resumed.completed_pairs as u64, resumed.next_pair_index as u32), (2, 6));
    }

    /// Replaying recorded rows must give the same rule state as having played them, including the
    /// colour flip in the reversed game of each pair.
    #[test]
    fn replay_rebuilds_the_rule_from_recorded_rows() {
        // Challenger wins as White, then draws as Black: 1.0 + 0.5 = 1.5 of 2.
        let rows = vec![row(0, CHALLENGER, 1.0), row(1, CHAMPION, 0.5)];
        let resumed = replay(&rows, CHALLENGER, fresh());
        assert_eq!(resumed.completed_pairs, 1);
        assert_eq!(resumed.next_pair_index, 1);
        assert_eq!(SequentialTest::games(&resumed.test), 2);
        assert_eq!(SequentialTest::score(&resumed.test), 1.5);
        assert!(resumed.trailing_half.is_none());
    }

    /// The challenger losing both games of a pair, one with each colour, scores zero -- the flip
    /// applied to the reversed game must not cancel the loss out into a point.
    #[test]
    fn replay_scores_a_pair_lost_with_both_colours_as_zero() {
        let rows = vec![row(0, CHALLENGER, 0.0), row(1, CHAMPION, 1.0)];
        let resumed = replay(&rows, CHALLENGER, fresh());
        assert_eq!(SequentialTest::score(&resumed.test), 0.0);
    }

    /// The colour comes from the recorded `white`, not from index parity. Here the two are made to
    /// disagree: the pair is stored with the challenger as White in the *odd* game. Parity would score
    /// this 0.0; the recorded colour scores it 2.0, which is what actually happened.
    #[test]
    fn replay_reads_the_colour_from_the_row_rather_than_from_index_parity() {
        let rows = vec![row(0, CHAMPION, 0.0), row(1, CHALLENGER, 1.0)];
        let resumed = replay(&rows, CHALLENGER, fresh());
        assert_eq!(SequentialTest::score(&resumed.test), 2.0, "the challenger won both, whichever index it was White in");
    }

    /// A half pair does not move the rule, whatever it holds: a first game waiting for its partner, or
    /// a parallel run's old fragment. The last pair's is reported, to be finished by a run that records
    /// half pairs.
    #[test]
    fn replay_does_not_consume_a_trailing_half_pair_but_reports_it() {
        let rows = vec![row(0, CHALLENGER, 1.0), row(1, CHAMPION, 0.5), row(2, CHALLENGER, 1.0)];
        let resumed = replay(&rows, CHALLENGER, fresh());
        assert_eq!(resumed.completed_pairs, 1, "one whole pair; the lone third game is not consumed");
        assert_eq!(SequentialTest::games(&resumed.test), 2);
        assert_eq!(SequentialTest::score(&resumed.test), 1.5);
        assert_eq!(resumed.next_pair_index, 2, "past the half pair, for a run that skips it");
        let half = resumed.trailing_half.expect("pair 1 has one game");
        assert_eq!((half.pair_index, half.game_index, half.challenger_score), (1, 2, 1.0));
    }

    /// The trailing half pair's score comes from the recorded colour too: a first game the challenger
    /// lost as White scores 0.
    #[test]
    fn a_trailing_half_pair_is_scored_from_the_challenger_s_side() {
        let half = replay(&[row(0, CHALLENGER, 0.0)], CHALLENGER, fresh()).trailing_half.expect("one game");
        assert_eq!((half.pair_index, half.game_index, half.challenger_score), (0, 0, 0.0));
    }

    /// Only the last pair's fragment is a trailing half pair. One below a whole pair is an old gap,
    /// skipped and never reported.
    #[test]
    fn a_half_pair_below_a_whole_one_is_not_trailing() {
        let rows = vec![row(0, CHALLENGER, 1.0), row(2, CHALLENGER, 1.0), row(3, CHAMPION, 0.0)];
        let resumed = replay(&rows, CHALLENGER, fresh());
        assert_eq!(resumed.completed_pairs, 1);
        assert!(resumed.trailing_half.is_none());
    }

    /// The bug this exists to prevent. A run stopped with pairs in flight leaves gaps, because workers
    /// take pair indexes in order but finish out of order. Handing out the *count* of recorded pairs
    /// would reissue a pair index already used and write a duplicate `game_index`, which nothing in the
    /// schema forbids and which would corrupt the pair grouping on the next resume.
    #[test]
    fn replay_takes_the_next_pair_index_past_the_highest_recorded_one_not_past_the_count() {
        // Pairs 0, 1 and 4 completed; 2 and 3 were abandoned in flight.
        let rows = vec![
            row(0, CHALLENGER, 1.0),
            row(1, CHAMPION, 0.0),
            row(2, CHALLENGER, 1.0),
            row(3, CHAMPION, 0.0),
            row(8, CHALLENGER, 1.0),
            row(9, CHAMPION, 0.0),
        ];
        let resumed = replay(&rows, CHALLENGER, fresh());
        assert_eq!(resumed.completed_pairs, 3);
        assert_eq!(SequentialTest::games(&resumed.test), 6, "all three recorded pairs are consumed despite the gap");
        assert_eq!(resumed.next_pair_index, 5, "past pair 4, not past the count of 3");
    }

    /// Resolves once it has read `games` games, whatever they were.
    struct StopsAfter {
        games: u64,
        tally: Tally,
    }

    impl SequentialTest for StopsAfter {
        fn add_game(&mut self, challenger_score: f64) {
            self.tally.add_game(challenger_score);
        }
        fn outcome(&self) -> Option<Outcome> {
            (self.tally.games() >= self.games).then_some(Outcome::ChallengerWins)
        }
        fn record(&self) -> (u64, u64, u64) {
            self.tally.record()
        }
        fn score(&self) -> f64 {
            self.tally.score()
        }
        fn games(&self) -> u64 {
            self.tally.games()
        }
        fn status(&self) -> String {
            String::new()
        }
        fn describe(&self) -> String {
            String::new()
        }
    }

    /// A live run feeds pairs in index order and stops at the first resolution, while pairs past it
    /// may already be recorded, having finished early. The replay stops at the same pair, so the
    /// verdict and its record survive a restart.
    #[test]
    fn replay_reads_pairs_in_index_order_and_stops_where_the_rule_resolved() {
        let rows = vec![
            row(0, CHALLENGER, 1.0),
            row(1, CHAMPION, 0.0),
            row(2, CHALLENGER, 0.5),
            row(3, CHAMPION, 0.5),
            // Recorded, but past the resolution at pair 1.
            row(4, CHALLENGER, 0.0),
            row(5, CHAMPION, 1.0),
        ];
        let resumed = replay(&rows, CHALLENGER, StopsAfter { games: 4, tally: Tally::new() });
        assert_eq!(resumed.completed_pairs, 2);
        assert_eq!(resumed.test.record(), (2, 0, 2), "pairs 0 and 1 only");
        assert_eq!(resumed.test.outcome(), Some(Outcome::ChallengerWins));
    }

    #[test]
    fn replay_of_an_empty_database_is_a_fresh_rule() {
        let resumed = replay(&[], CHALLENGER, fresh());
        assert_eq!(resumed.completed_pairs, 0);
        assert_eq!(resumed.next_pair_index, 0);
        assert_eq!(SequentialTest::games(&resumed.test), 0);
        assert_eq!(resumed.test.outcome(), None);
        assert!(resumed.trailing_half.is_none());
    }

    #[test]
    fn the_labels_name_each_outcome() {
        let labels = Labels {
            prefix: "duel",
            challenger_wins: "challenger".to_string(),
            champion_wins: "champion".to_string(),
            drawn: "drawn".to_string(),
        };
        assert_eq!(labels.line(Outcome::ChallengerWins), "challenger");
        assert_eq!(labels.line(Outcome::ChampionWins), "champion");
        assert_eq!(labels.line(Outcome::Drawn), "drawn");
    }

    #[test]
    fn a_tally_counts_half_points_exactly() {
        let mut tally = Tally::new();
        for score in [1.0, 0.5, 0.0, 0.5] {
            tally.add_game(score);
        }
        assert_eq!(tally.record(), (1, 1, 2));
        assert_eq!(tally.score(), 2.0);
        assert_eq!(tally.games(), 4);
    }

    #[test]
    fn a_fixed_depth_plays_one_unpinned_pair_per_logical_core() {
        let placement = Run::placement(Control::FixedDepth(9));
        assert!(placement.threads >= 1);
        assert!(placement.pin_to.is_none());
    }

    #[test]
    fn a_clock_plays_half_the_logical_cores_pinned_to_physical_ones() {
        // Two logical per physical core, so every physical core.
        let placement = clock_placement(Some(vec![0, 2, 4, 6, 8, 10, 12, 14]), 16);
        assert_eq!(placement.threads, 8);
        assert_eq!(placement.pin_to, Some(vec![0, 2, 4, 6, 8, 10, 12, 14]));
        // One logical per physical core, so the lowest-numbered half.
        let placement = clock_placement(Some((0..20).collect()), 20);
        assert_eq!(placement.threads, 10);
        assert_eq!(placement.pin_to, Some((0..10).collect()));
        assert_eq!(clock_placement(Some(vec![0]), 1).threads, 1, "at least one pair");
    }

    #[test]
    fn a_clock_without_a_topology_plays_half_the_logical_cores_unpinned() {
        let placement = clock_placement(None, 16);
        assert_eq!(placement.threads, 8);
        assert!(placement.pin_to.is_none());
        assert_eq!(clock_placement(None, 1).threads, 1, "at least one pair");
    }

    #[test]
    fn a_clock_placement_has_one_pin_per_pair() {
        let placement = Run::placement(Control::parse("3+2"));
        if let Some(cpus) = &placement.pin_to {
            assert_eq!(cpus.len(), placement.threads);
        }
        assert!(placement.threads >= 1);
    }
}
