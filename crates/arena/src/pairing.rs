//! Plays one pairing -- `engine_one` (the stronger seed) vs. `engine_two` -- to a decided
//! result, under the World-Championship-style match format decided directly with the developer
//! (see the tournament design's "Fitness selection" section): up to `best_of` games at the base
//! time control, first past the win threshold wins outright, tied after `best_of` falls through
//! to tiebreak pairs at the tiebreak control until one side is strictly ahead -- or, with no
//! tiebreak control (`arena match`), ends as a drawn match. This is the core
//! primitive everything else calls -- a plain two-engine match is just this function invoked
//! once, and the (not yet built) bracket scheduler is this function invoked once per pairing.
//!
//! The *other* decision rule, the sequential test a network gate uses, is `gate.rs`. The two share
//! `game.rs` (one game) and `start_player` below (building a player from the registry), and differ
//! only in what they do with the results.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rusqlite::Connection;

use temper_chess::board::color::Color;
use temper_chess::nnue::Network;
use temper_chess::tablebase::SyzygyTablebase;

use super::random_book;
use super::data_root;
use super::config::MatchSpec;
use super::engines::{EngineKind, EngineRegistry};
use super::game::{self, PlyOutcome};
use super::in_process::InProcessPlayer;
use super::pgn::{self, MoveAnnotation, Termination};
use super::player::{Control, Fault, Player};
use super::results_db::{self, GameRecord};
use super::uci_client::UciPlayer;

/// How a pairing ended: one engine won it, or, only without a tie-break control, the two ended
/// level after `best_of` games.
#[derive(Debug, PartialEq)]
pub enum MatchOutcome {
    Won(String),
    Drawn,
}

/// Plays `round`'s pairing between `engine_one_name` and `engine_two_name` to a decided result,
/// resuming from whatever `results.db` already has for this exact `(round, engine_one,
/// engine_two)` key. Returns the outcome, printing the final score, or `None` only if the run was
/// interrupted (Ctrl+C) before a decision was reached.
#[allow(clippy::too_many_arguments)]
pub fn play_pairing(
    event_name: &str,
    round: u32,
    engine_one_name: &str,
    engine_two_name: &str,
    registry: &EngineRegistry,
    match_spec: &MatchSpec,
    pgn_path: &str,
    db_connection: &mut Connection,
    stop_requested: &Arc<AtomicBool>,
) -> Option<MatchOutcome> {
    let outcome = play_pairing_unannounced(
        event_name, round, engine_one_name, engine_two_name, registry, match_spec, pgn_path, db_connection, stop_requested,
    )?;
    println!("{}", pairing_score(db_connection, round, engine_one_name, engine_two_name).line(engine_one_name, engine_two_name));
    Some(outcome)
}

/// A pairing's score so far: each side's points, and `engine_one`'s wins, draws and losses.
#[derive(Debug, PartialEq)]
pub struct MatchScore {
    pub one_points: f64,
    pub two_points: f64,
    pub wins: u32,
    pub draws: u32,
    pub losses: u32,
}

impl MatchScore {
    /// `match:  amanda-0001 2.5 - 0.5 stockfish-0   (+2 =1 -0)`: the tally, then the first player's
    /// wins, draws and losses, so the counts read from the same side as the left number and never
    /// flip with the colours.
    pub fn line(&self, engine_one_name: &str, engine_two_name: &str) -> String {
        format!(
            "match:  {engine_one_name} {:.1} - {:.1} {engine_two_name}   (+{} ={} -{})",
            self.one_points, self.two_points, self.wins, self.draws, self.losses
        )
    }
}

/// The pairing's score so far, from the games `results.db` holds.
pub fn pairing_score(db_connection: &Connection, round: u32, engine_one_name: &str, engine_two_name: &str) -> MatchScore {
    score_of(&results_db::pairing_games(db_connection, round, engine_one_name, engine_two_name))
}

/// `games`' score, `engine_one` White in the even-indexed games, as `standings` reads them.
fn score_of(games: &[(f64, String)]) -> MatchScore {
    let (one_points, two_points) = standings(games);
    let mut score = MatchScore { one_points, two_points, wins: 0, draws: 0, losses: 0 };
    for (index, (white_result, _termination)) in games.iter().enumerate() {
        let one_result = if index % 2 == 0 { *white_result } else { 1.0 - white_result };
        if one_result == 1.0 {
            score.wins += 1;
        } else if one_result == 0.0 {
            score.losses += 1;
        } else {
            score.draws += 1;
        }
    }
    score
}

#[allow(clippy::too_many_arguments)]
fn play_pairing_unannounced(
    event_name: &str,
    round: u32,
    engine_one_name: &str,
    engine_two_name: &str,
    registry: &EngineRegistry,
    match_spec: &MatchSpec,
    pgn_path: &str,
    db_connection: &mut Connection,
    stop_requested: &Arc<AtomicBool>,
) -> Option<MatchOutcome> {
    let base_control = Control::parse(&match_spec.base_time_control);
    let tiebreak_control = match_spec.tiebreak_time_control.as_deref().map(Control::parse);
    let has_tiebreak = tiebreak_control.is_some();
    let win_threshold = super::config::win_threshold(match_spec.best_of);

    let existing = results_db::pairing_games(db_connection, round, engine_one_name, engine_two_name);
    if let Some(winner) = fault_winner(&existing, engine_one_name, engine_two_name) {
        return Some(MatchOutcome::Won(winner));
    }
    let (mut one_score, mut two_score) = standings(&existing);
    let mut games_played = existing.len() as u32;
    if let Some(outcome) = decide(
        games_played, one_score, two_score, match_spec.best_of, win_threshold, has_tiebreak, engine_one_name, engine_two_name,
    ) {
        return Some(outcome);
    }

    // Each pair draws its opening uniformly at random from the whole book, with the RNG seeded from
    // OS entropy, so separate matches never replay the same handful of positions. A run resumed
    // between the two games of a pair has no pick of its own to reuse, so it takes the first game's
    // opening back from the PGN, which gets every game appended in the same order `results.db`
    // records them.
    let book = random_book::RandomBook::open(&data_root::book_path());
    let mut book_reader = book.reader();
    let mut rng = rand::rng();
    let mut starting_fen = if games_played % 2 == 1 { last_starting_fen(pgn_path) } else { String::new() };

    let tablebase = load_tablebase();

    let Ok(mut engine_one_player) = start_player(engine_one_name, registry, &tablebase) else {
        return Some(MatchOutcome::Won(engine_two_name.to_string()));
    };
    let Ok(mut engine_two_player) = start_player(engine_two_name, registry, &tablebase) else {
        engine_one_player.shutdown();
        return Some(MatchOutcome::Won(engine_one_name.to_string()));
    };

    loop {
        if stop_requested.load(Ordering::SeqCst) {
            engine_one_player.shutdown();
            engine_two_player.shutdown();
            return None;
        }

        // Past `best_of` only with a tie-break: without one, `decide` ends a level match there.
        let control = if games_played < match_spec.best_of {
            base_control
        } else {
            tiebreak_control.expect("a match without a tie-break is decided after best_of games")
        };
        // Both games of a pair (one per color -- `games_played` 2k and 2k+1) share the same
        // opening, so a color-balanced comparison actually happens on identical positions rather
        // than each side facing a different one: a fresh pick only at the start of a pair.
        if games_played.is_multiple_of(2) {
            starting_fen = book.random_fen(&mut book_reader, &mut rng);
        }
        let engine_one_is_white = games_played.is_multiple_of(2);
        let (white_name, black_name) =
            if engine_one_is_white { (engine_one_name, engine_two_name) } else { (engine_two_name, engine_one_name) };
        let (white_player, black_player): (&mut dyn Player, &mut dyn Player) = if engine_one_is_white {
            (&mut *engine_one_player, &mut *engine_two_player)
        } else {
            (&mut *engine_two_player, &mut *engine_one_player)
        };

        if let Err(fault) = white_player.new_game() {
            let winner = if engine_one_is_white { engine_two_name } else { engine_one_name };
            record_fault(db_connection, pgn_path, event_name, round, engine_one_name, engine_two_name, games_played, &starting_fen, white_name, black_name, white_name, &game::fault_message(fault), &[], 0);
            // Through the borrowed references rather than the boxes: both are still borrowed here,
            // and shutting the two of them down is what the boxes would have done anyway.
            white_player.shutdown();
            black_player.shutdown();
            return Some(MatchOutcome::Won(winner.to_string()));
        }
        if let Err(fault) = black_player.new_game() {
            let winner = if engine_one_is_white { engine_one_name } else { engine_two_name };
            record_fault(db_connection, pgn_path, event_name, round, engine_one_name, engine_two_name, games_played, &starting_fen, white_name, black_name, black_name, &game::fault_message(fault), &[], 0);
            white_player.shutdown();
            black_player.shutdown();
            return Some(MatchOutcome::Won(winner.to_string()));
        }

        // Who has which colour, since it swaps every game and the move lines below do not say.
        println!("game {}: {white_name} (White) vs {black_name} (Black)", games_played + 1);
        match game::play_one_game(games_played + 1, &starting_fen, white_player, black_player, control, true, None, stop_requested) {
            PlyOutcome::Interrupted => {
                engine_one_player.shutdown();
                engine_two_player.shutdown();
                return None;
            }
            PlyOutcome::Fault { faulting_color, message, moves, ply_count } => {
                let faulting_name = if faulting_color == Color::White { white_name } else { black_name };
                let winner = if faulting_name == engine_one_name { engine_two_name } else { engine_one_name };
                record_fault(
                    db_connection, pgn_path, event_name, round, engine_one_name, engine_two_name, games_played,
                    &starting_fen, white_name, black_name, faulting_name, &message, &moves, ply_count,
                );
                engine_one_player.shutdown();
                engine_two_player.shutdown();
                return Some(MatchOutcome::Won(winner.to_string()));
            }
            PlyOutcome::Decided(outcome) => {
                pgn::append_to_file(
                    pgn_path,
                    &pgn::format_game(
                        &pgn::GameHeaders {
                            event: event_name.to_string(),
                            round,
                            white: white_name.to_string(),
                            black: black_name.to_string(),
                            starting_fen: starting_fen.clone(),
                        },
                        &outcome.moves,
                        pgn_result(outcome.result),
                        outcome.termination,
                        true,
                    ),
                );
                results_db::insert_game(
                    db_connection,
                    &GameRecord {
                        round,
                        engine_one: engine_one_name,
                        engine_two: engine_two_name,
                        game_index: games_played,
                        white: white_name,
                        black: black_name,
                        result: outcome.result,
                        termination: outcome.termination,
                        ply_count: outcome.ply_count,
                    },
                );

                if engine_one_is_white {
                    one_score += outcome.result;
                    two_score += 1.0 - outcome.result;
                } else {
                    two_score += outcome.result;
                    one_score += 1.0 - outcome.result;
                }
                games_played += 1;
                let verdict = match outcome.result {
                    1.0 => format!("{white_name} wins"),
                    0.0 => format!("{black_name} wins"),
                    _ => "drawn".to_string(),
                };
                // The game, the match so far, then a blank line, so every game stands apart.
                println!("game {games_played}: {} by {}, {verdict}", pgn_result(outcome.result), outcome.termination.as_str());
                println!(
                    "{}\n",
                    pairing_score(db_connection, round, engine_one_name, engine_two_name).line(engine_one_name, engine_two_name)
                );

                if let Some(match_outcome) = decide(
                    games_played, one_score, two_score, match_spec.best_of, win_threshold, has_tiebreak, engine_one_name,
                    engine_two_name,
                ) {
                    engine_one_player.shutdown();
                    engine_two_player.shutdown();
                    return Some(match_outcome);
                }
            }
        }
    }
}

/// The starting position of the last game in `pgn_path`: its last `[FEN "..."]` tag. Also how the
/// SPRT runner resumes a half pair.
pub fn last_starting_fen(pgn_path: &str) -> String {
    let text = std::fs::read_to_string(pgn_path)
        .unwrap_or_else(|error| panic!("failed to read {pgn_path} to resume a half-played opening pair: {error}"));
    fen_of_last_game(&text).unwrap_or_else(|| panic!("{pgn_path} has no [FEN] tag to resume a half-played opening pair"))
}

fn fen_of_last_game(pgn_text: &str) -> Option<String> {
    pgn_text
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix("[FEN \"").and_then(|rest| rest.strip_suffix("\"]")))
        .map(str::to_string)
}

/// White's own result as the PGN result tag. Shared with `gate.rs`, which writes the same games.
pub fn pgn_result(white_result: f64) -> &'static str {
    match white_result {
        1.0 => "1-0",
        0.0 => "0-1",
        _ => "1/2-1/2",
    }
}

/// One `SyzygyTablebase` for both sides, shared rather than one per player because probing is
/// stateless and the loaded tables are never mutated. Loaded even when both sides are UCI engines,
/// which read the same tables through `SyzygyPath`: an engine given a bad path plays on without them
/// and says nothing, so loading them here is the check that stops the run instead.
pub fn load_tablebase() -> Arc<SyzygyTablebase> {
    Arc::new(data_root::load_tablebase())
}

/// Builds whichever kind of player `name`'s registry entry asks for. Only the UCI kind can fail --
/// starting a subprocess involves a spawn and a handshake, while an in-process player is a struct
/// and a hash table -- but a failure to load a `.nnue` panics rather than becoming a fault: a
/// missing or corrupt network file is a config error, like an unregistered engine name, not a
/// misbehaving opponent that should lose the pairing.
pub fn start_player(
    name: &str,
    registry: &EngineRegistry,
    tablebase: &Arc<SyzygyTablebase>,
) -> Result<Box<dyn Player>, Fault> {
    let entry = registry.resolve(name);
    match entry.kind {
        EngineKind::Uci => Ok(Box::new(UciPlayer::start(name, entry)?)),
        EngineKind::Builtin => {
            // Pesto has no network to carry its tuning, so it is read here; a malformed file
            // is a config error, as a corrupt network is.
            let parameters = match entry.parameters {
                Some(parameters) => Some(parameters),
                None => super::engines::pesto_parameters()
                    .unwrap_or_else(|error| panic!("failed to read pesto's tuning for engine {name:?}: {error}")),
            };
            Ok(Box::new(InProcessPlayer::new(None, Arc::clone(tablebase), entry.hash_mb(), parameters)))
        }
        EngineKind::Network => {
            let network_path = entry.network_path();
            let network = Network::load(network_path)
                .unwrap_or_else(|error| panic!("failed to load network {network_path} for engine {name:?}: {error}"));
            Ok(Box::new(InProcessPlayer::new(
                Some(Arc::new(network)),
                Arc::clone(tablebase),
                entry.hash_mb(),
                entry.parameters,
            )))
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn record_fault(
    db_connection: &mut Connection,
    pgn_path: &str,
    event_name: &str,
    round: u32,
    engine_one_name: &str,
    engine_two_name: &str,
    game_index: u32,
    starting_fen: &str,
    white_name: &str,
    black_name: &str,
    faulting_name: &str,
    message: &str,
    moves: &[MoveAnnotation],
    ply_count: u32,
) {
    eprintln!("fault: {faulting_name} disqualified from round {round} ({engine_one_name} vs {engine_two_name}): {message}");
    let result = if faulting_name == white_name { 0.0 } else { 1.0 };
    let termination = if message.contains("illegal") || message.contains("no move") {
        Termination::IllegalMove
    } else {
        Termination::Crashed
    };
    pgn::append_to_file(
        pgn_path,
        &pgn::format_game(
            &pgn::GameHeaders {
                event: event_name.to_string(),
                round,
                white: white_name.to_string(),
                black: black_name.to_string(),
                starting_fen: starting_fen.to_string(),
            },
            moves,
            pgn_result(result),
            termination,
            true,
        ),
    );
    results_db::insert_game(
        db_connection,
        &GameRecord {
            round,
            engine_one: engine_one_name,
            engine_two: engine_two_name,
            game_index,
            white: white_name,
            black: black_name,
            result,
            termination,
            ply_count,
        },
    );
}

/// Sums each already-played game's own contribution to `engine_one`'s/`engine_two`'s running
/// score, from its `(white_result, termination)` row alone -- which engine was white that game
/// is derived from `game_index` parity (the fixed "`engine_one` white in game 0, then
/// alternate" rule), not stored separately.
fn standings(games: &[(f64, String)]) -> (f64, f64) {
    let mut one_score = 0.0;
    let mut two_score = 0.0;
    for (index, (white_result, _termination)) in games.iter().enumerate() {
        let engine_one_is_white = index % 2 == 0;
        if engine_one_is_white {
            one_score += white_result;
            two_score += 1.0 - white_result;
        } else {
            two_score += white_result;
            one_score += 1.0 - white_result;
        }
    }
    (one_score, two_score)
}

/// A fault-terminated game, if any exists among `games`, decides the whole pairing immediately
/// -- see the tournament design's "zero tolerance" fault handling. Checked ahead of the normal
/// score-based `decide` so a resumed run recognizes an already-faulted pairing without ever
/// re-spawning either engine.
fn fault_winner(games: &[(f64, String)], engine_one: &str, engine_two: &str) -> Option<String> {
    for (index, (white_result, termination)) in games.iter().enumerate() {
        if results_db::is_fault_termination(termination) {
            let engine_one_is_white = index % 2 == 0;
            let white_won = *white_result == 1.0;
            let engine_one_won = white_won == engine_one_is_white;
            return Some(if engine_one_won { engine_one.to_string() } else { engine_two.to_string() });
        }
    }
    None
}

/// The pairing's own decision rule, applied at the right checkpoints only: every game during
/// the initial `best_of` phase (a side can win before all of them are played), but only after a
/// *completed pair* once in tiebreak territory (`games_played > best_of`) -- checking mid-pair
/// would let one lucky tiebreak game decide things the real World Championship format never
/// would.
///
/// Without a tie-break (`has_tiebreak` false), a match still level after `best_of` games is drawn.
#[allow(clippy::too_many_arguments)]
fn decide(
    games_played: u32, one_score: f64, two_score: f64, best_of: u32, win_threshold: f64, has_tiebreak: bool,
    engine_one: &str, engine_two: &str,
) -> Option<MatchOutcome> {
    if games_played <= best_of {
        if one_score >= win_threshold {
            return Some(MatchOutcome::Won(engine_one.to_string()));
        }
        if two_score >= win_threshold {
            return Some(MatchOutcome::Won(engine_two.to_string()));
        }
        if games_played == best_of && !has_tiebreak {
            return Some(MatchOutcome::Drawn);
        }
        return None;
    }

    let tiebreak_games_played = games_played - best_of;
    if !tiebreak_games_played.is_multiple_of(2) {
        return None;
    }
    if one_score > two_score {
        return Some(MatchOutcome::Won(engine_one.to_string()));
    }
    if two_score > one_score {
        return Some(MatchOutcome::Won(engine_two.to_string()));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn won(name: &str) -> Option<MatchOutcome> {
        Some(MatchOutcome::Won(name.to_string()))
    }

    #[test]
    fn decide_stops_as_soon_as_the_win_threshold_is_reached_before_best_of_is_exhausted() {
        assert_eq!(decide(9, 6.5, 2.5, 12, 6.5, true, "one", "two"), won("one"));
        assert_eq!(decide(9, 5.0, 4.0, 12, 6.5, true, "one", "two"), None);
        assert_eq!(decide(8, 1.5, 6.5, 12, 6.5, false, "one", "two"), won("two"), "no tie-break changes nothing here");
    }

    #[test]
    fn decide_falls_through_to_tiebreak_pairs_on_an_exact_tie_at_best_of() {
        assert_eq!(decide(12, 6.0, 6.0, 12, 6.5, true, "one", "two"), None);
        assert_eq!(decide(13, 7.0, 6.0, 12, 6.5, true, "one", "two"), None); // mid-pair, not decided
        assert_eq!(decide(14, 7.5, 6.5, 12, 6.5, true, "one", "two"), won("one"));
        assert_eq!(decide(14, 7.0, 7.0, 12, 6.5, true, "one", "two"), None); // still tied, another pair
    }

    #[test]
    fn decide_draws_a_level_match_without_a_tiebreak() {
        assert_eq!(decide(12, 6.0, 6.0, 12, 6.5, false, "one", "two"), Some(MatchOutcome::Drawn));
        assert_eq!(decide(11, 5.5, 5.5, 12, 6.5, false, "one", "two"), None, "one game still to play");
    }

    #[test]
    fn fen_of_last_game_takes_the_last_fen_tag() {
        let text = "[Event \"e\"]\n[FEN \"8/8/8/8/8/8/8/K6k w - - 0 1\"]\n\n1. Kb1 *\n\n\
                    [Event \"e\"]\n[FEN \"8/8/8/8/8/8/8/K5k1 b - - 0 1\"]\n\n1... Kf1 *\n";
        assert_eq!(fen_of_last_game(text), Some("8/8/8/8/8/8/8/K5k1 b - - 0 1".to_string()));
        assert_eq!(fen_of_last_game("[Event \"e\"]\n"), None);
    }

    #[test]
    fn pgn_result_maps_white_s_score_to_the_result_tag() {
        assert_eq!(pgn_result(1.0), "1-0");
        assert_eq!(pgn_result(0.0), "0-1");
        assert_eq!(pgn_result(0.5), "1/2-1/2");
    }

    #[test]
    fn the_score_counts_wins_draws_and_losses_from_engine_one_s_side() {
        // Game 0: engine_one White, wins. Game 1: engine_two White, wins (a loss for engine_one).
        // Game 2: engine_one White, draw. Game 3: engine_two White, loses (a win for engine_one).
        let games: Vec<(f64, String)> =
            [1.0, 1.0, 0.5, 0.0].iter().map(|&result| (result, "checkmate".to_string())).collect();
        let score = score_of(&games);
        assert_eq!(score, MatchScore { one_points: 2.5, two_points: 1.5, wins: 2, draws: 1, losses: 1 });
        assert_eq!(score.line("amanda-0001", "stockfish-0"), "match:  amanda-0001 2.5 - 1.5 stockfish-0   (+2 =1 -1)");
    }

    #[test]
    fn standings_derives_each_side_from_white_result_and_game_index_parity() {
        // Game 0: engine_one is white, wins (1.0). Game 1: engine_two is white, draws (0.5).
        let games = vec![(1.0, "checkmate".to_string()), (0.5, "repetition".to_string())];
        assert_eq!(standings(&games), (1.5, 0.5));
    }
}
