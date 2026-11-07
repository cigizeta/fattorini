//! One game, from an opening position to a result: the arbiter's half of a match. Whose turn it is,
//! whether the position is checkmate or stalemate or a draw by rule, whose clock is running, and what
//! the two players are asked for -- but nothing about who wins a *match*, which is `pairing.rs`'s
//! best-of format and `gate.rs`'s sequential test respectively.
//!
//! Its own module because both of those need it, and a game is the same thing under either. Extracted
//! from `pairing.rs` unchanged apart from `print_moves`, which the parallel gate needs: sixteen games
//! writing move lines to one terminal at once is unreadable, so the gate prints one line per completed
//! pair instead and asks for silence here.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fattorini::board::color::Color;
use fattorini::board::piece::Piece;
use fattorini::board::piece_type::PieceType;
use fattorini::board::Board;
use fattorini::movegen::attacks::is_in_check;
use fattorini::movegen::legal::legal_moves;
use fattorini::search::is_insufficient_material;
use fattorini::tablebase::{SyzygyTablebase, TbResult};
use fattorini::uci::{format_move, parse_move};

use super::clock::Clock;
use super::pgn::{MoveAnnotation, Termination};
use super::player::{Control, Fault, MoveLimit, MoveOutcome, MoveRequest, Player};

pub struct GameOutcome {
    /// White's own score for this game: `1.0`/`0.5`/`0.0`.
    pub result: f64,
    pub termination: Termination,
    pub ply_count: u32,
    pub moves: Vec<MoveAnnotation>,
}

pub enum PlyOutcome {
    Decided(GameOutcome),
    Fault { faulting_color: Color, message: String, moves: Vec<MoveAnnotation>, ply_count: u32 },
    Interrupted,
}

pub fn fault_message(fault: Fault) -> String {
    match fault {
        Fault::HandshakeTimedOut => "handshake timed out".to_string(),
        Fault::Crashed(message) => message,
        Fault::IllegalMove(message) => message,
    }
}

/// The searching engine's own last-reported score, in pawns (`+0.35`/`-1.20`) or as a signed
/// mate count (`#3`/`-#3`) -- the live console's own compact form, distinct from `pgn.rs`'s
/// `annotate` (which adds depth/time and cutechess's brace syntax for the PGN file itself).
fn format_eval(score_cp: Option<i32>, mate_in: Option<i32>) -> String {
    match (score_cp, mate_in) {
        (Some(cp), _) => format!("{:+.2}", cp as f64 / 100.0),
        (None, Some(mate)) if mate >= 0 => format!("#{mate}"),
        (None, Some(mate)) => format!("-#{}", mate.abs()),
        (None, None) => "?".to_string(),
    }
}

/// `d<depth>`, or `-` if the engine never reported one -- the live console's own compact form
/// for `SearchReport::depth`.
fn format_depth(depth: Option<u32>) -> String {
    match depth {
        Some(depth) => format!("d{depth}"),
        None => "-".to_string(),
    }
}

/// `m:ss.t`, the mover's own clock reading right after this move was spent (including any
/// increment already added back by `Clock::spend`) -- the live console's own compact form for
/// how much time that side has left. `-` under a fixed depth, where there is no clock at all.
fn format_clock(remaining: Option<Duration>) -> String {
    let Some(remaining) = remaining else {
        return "-".to_string();
    };
    // Whole tenths first, truncated as a chess clock does, and only then split into minutes and
    // seconds: rounding the seconds after the split turned 59.95 s and up into `0:60.0`.
    let tenths = remaining.as_millis() / 100;
    let minutes = tenths / 600;
    let seconds = (tenths % 600) / 10;
    let tenth = tenths % 10;
    format!("{minutes}:{seconds:02}.{tenth}")
}

fn decisive_or_draw(is_decisive: bool, side_to_move: Color) -> f64 {
    if !is_decisive {
        return 0.5;
    }
    if side_to_move == Color::White { 0.0 } else { 1.0 }
}

/// White's score when `flagged` runs out of time on `board`: a loss, unless the other side cannot
/// checkmate by any legal sequence of moves, which FIDE's rule 6.9 makes a draw. Judged as
/// cutechess-cli and Lichess do: the other side has a bare king, or the position is dead for both
/// (`is_insufficient_material`). A lone knight or bishop against more than a bare king stays a
/// loss, since the flagged side's own pieces can block its king into a mate.
fn time_forfeit_result(board: &Board, flagged: Color) -> f64 {
    let other = flagged.opposite();
    let bare_king = board.occupancy(other) == board.bitboard(Piece::new(other, PieceType::King));
    let cannot_mate = bare_king || is_insufficient_material(board);
    decisive_or_draw(!cannot_mate, flagged)
}

/// Early stop's win rule: a side ahead by this much, in its own and its opponent's reported
/// centipawns, or by a mate, for `WIN_PLIES` plies in a row, wins. fastchess's usual resign settings.
const WIN_SCORE_CP: i32 = 600;
const WIN_PLIES: u32 = 6;
/// Early stop's draw rule, fishtest's (`-draw movenumber=34 movecount=8 score=20`): `DRAW_PLIES`
/// plies in a row with |score| at most `DRAW_SCORE_CP`, once `DRAW_MIN_FULL_MOVES` moves are complete.
const DRAW_SCORE_CP: i32 = 20;
const DRAW_PLIES: u32 = 16;
const DRAW_MIN_FULL_MOVES: u32 = 34;

/// Early stop's two score rules, fed each move's report after it is played. Only a tune's games end
/// early: the decision of 2026-09-22, no adjudication of any kind, holds for every other game, so
/// that a gate or a duel judges networks on games played out.
#[derive(Default)]
struct Adjudicator {
    /// The side every score of the current run says is winning, and the run's length in plies.
    leader: Option<Color>,
    win_run: u32,
    /// Plies in a row whose mover reported a centipawn score within `DRAW_SCORE_CP`.
    draw_run: u32,
}

impl Adjudicator {
    /// Takes the report of `mover`'s move just played: `zeroing` when it was a capture or a pawn
    /// move, `full_moves_complete` counted after it. White's score and the rule, if the game ends.
    fn observe(
        &mut self, mover: Color, score_cp: Option<i32>, mate_in: Option<i32>, zeroing: bool, full_moves_complete: u32,
    ) -> Option<(f64, Termination)> {
        let mover_ahead = score_cp.is_some_and(|cp| cp >= WIN_SCORE_CP) || mate_in.is_some_and(|mate| mate > 0);
        let mover_behind = score_cp.is_some_and(|cp| cp <= -WIN_SCORE_CP) || mate_in.is_some_and(|mate| mate < 0);
        let leader = if mover_ahead {
            Some(mover)
        } else if mover_behind {
            Some(mover.opposite())
        } else {
            None
        };
        if leader.is_some() && leader == self.leader {
            self.win_run += 1;
        } else {
            self.leader = leader;
            self.win_run = if leader.is_some() { 1 } else { 0 };
        }

        // fastchess's `DrawTracker`: a zeroing move ends the run, and its own score may start the next.
        if zeroing {
            self.draw_run = 0;
        }
        if score_cp.is_some_and(|cp| cp.abs() <= DRAW_SCORE_CP) {
            self.draw_run += 1;
        } else {
            self.draw_run = 0;
        }

        if let Some(winner) = self.leader.filter(|_| self.win_run >= WIN_PLIES) {
            let white_score = if winner == Color::White { 1.0 } else { 0.0 };
            return Some((white_score, Termination::AdjudicatedWin));
        }
        if self.draw_run >= DRAW_PLIES && full_moves_complete >= DRAW_MIN_FULL_MOVES {
            return Some((0.5, Termination::AdjudicatedDraw));
        }
        None
    }
}

/// Early stop's tablebase rule: White's score at `board`, `side_to_move` to play, from the tables'
/// result under the fifty-move rule, so a win the clock frustrates is a draw. `None` where the tables
/// do not rank the position (more pieces than they cover, castling rights left).
fn tablebase_result(tablebase: &SyzygyTablebase, board: &Board, ancestors: &[u64]) -> Option<f64> {
    let mover_wins = match tablebase.root_result(board, ancestors)? {
        TbResult::Win => 1.0,
        TbResult::Draw => return Some(0.5),
        TbResult::Loss => 0.0,
    };
    Some(if board.side_to_move() == Color::White { mover_wins } else { 1.0 - mover_wins })
}

/// Plays one game out. `print_moves` false suppresses the per-ply console line entirely, for callers
/// running many games at once; the PGN annotations are collected either way, so nothing about the
/// recorded game depends on it. `early_stop`, the tables, ends the game early by the tablebase, win
/// and draw rules (`Adjudicator`); `None` plays every game out.
#[allow(clippy::too_many_arguments)]
pub fn play_one_game(
    game_number: u32,
    starting_fen: &str,
    white_player: &mut dyn Player,
    black_player: &mut dyn Player,
    control: Control,
    print_moves: bool,
    early_stop: Option<&SyzygyTablebase>,
    stop_requested: &Arc<AtomicBool>,
) -> PlyOutcome {
    let mut adjudicator = Adjudicator::default();
    let (mut board, mut fullmove_number) =
        Board::from_fen(starting_fen).unwrap_or_else(|error| panic!("opening book gave an invalid FEN {starting_fen:?}: {error:?}"));
    let mut ancestor_hashes = vec![board.hash()];
    let mut moves_uci: Vec<String> = Vec::new();
    let mut annotations: Vec<MoveAnnotation> = Vec::new();
    // `None` under a fixed depth: there is no clock to run, so there is also no time forfeit and
    // nothing to print in the clock column. A `Clock` of some arbitrary large size would have been
    // a clock that merely never runs out, which is not the same thing and would have left a
    // forfeit branch reachable in a match format that has no such outcome.
    let mut white_clock = control.time_control().map(Clock::new);
    let mut black_clock = control.time_control().map(Clock::new);

    // No hard ply cap: the fifty-move rule (checked unconditionally below, the same as a real
    // arbiter claiming it automatically rather than waiting on a player) already guarantees
    // this loop terminates within a finite, bounded number of plies on its own -- a pawn move
    // or capture is the only thing that can ever reset it, and both are strictly finite over
    // the course of any legal game, so no separate backstop is needed. Removed entirely,
    // including `Termination::PlyCap`, per explicit developer instruction -- not standard
    // practice elsewhere either (TCEC uses eval-based adjudication, not a flat ply count; CCRL
    // documents no such rule at all), only cutechess-cli's own optional `-maxmoves` is
    // comparable, and this project isn't using it.
    let mut ply: u32 = 0;
    // Each move-pair prints on one line, white's cell first and black's cell trailing after
    // it -- white's own cell is printed (and flushed) as soon as it's known, without a newline,
    // so the line only completes once black replies, or -- if the game or run ends first --
    // right after the loop below, so a lone final white move still ends on its own line.
    let mut pending_newline = false;
    let outcome = loop {
        let side_to_move = board.side_to_move();
        let legal = legal_moves(&board, side_to_move);

        if legal.is_empty() {
            let checkmate = is_in_check(&board, side_to_move);
            let result = decisive_or_draw(checkmate, side_to_move);
            let termination = if checkmate { Termination::Checkmate } else { Termination::Stalemate };
            break PlyOutcome::Decided(GameOutcome { result, termination, ply_count: ply, moves: annotations });
        }
        // A dead position, neither side able to mate: drawn at once, as an arbiter would.
        if is_insufficient_material(&board) {
            break PlyOutcome::Decided(GameOutcome { result: 0.5, termination: Termination::InsufficientMaterial, ply_count: ply, moves: annotations });
        }
        if board.halfmove_clock() >= 100 {
            break PlyOutcome::Decided(GameOutcome { result: 0.5, termination: Termination::FiftyMoveRule, ply_count: ply, moves: annotations });
        }
        // Threefold, the real rule -- unlike self-play's own deliberate twofold deviation
        // (`self_play.rs`'s own doc comment covers why), tournaments follow the rules of chess
        // strictly, so this stays untouched.
        if ancestor_hashes.iter().filter(|&&hash| hash == board.hash()).count() >= 3 {
            break PlyOutcome::Decided(GameOutcome { result: 0.5, termination: Termination::Repetition, ply_count: ply, moves: annotations });
        }
        if let Some(result) = early_stop.and_then(|tablebase| {
            tablebase_result(tablebase, &board, &ancestor_hashes[..ancestor_hashes.len() - 1])
        }) {
            break PlyOutcome::Decided(GameOutcome { result, termination: Termination::AdjudicatedTablebase, ply_count: ply, moves: annotations });
        }
        if stop_requested.load(Ordering::SeqCst) {
            break PlyOutcome::Interrupted;
        }

        let limit = match control {
            Control::Fischer(time_control) => MoveLimit::Fischer {
                white_remaining: white_clock.expect("a Fischer control builds both clocks").remaining(),
                black_remaining: black_clock.expect("a Fischer control builds both clocks").remaining(),
                increment: time_control.increment,
            },
            Control::FixedDepth(depth) => MoveLimit::FixedDepth(depth),
        };
        let request = MoveRequest {
            starting_fen,
            moves_uci: &moves_uci,
            board: &board,
            // The positions before `board`: `ancestor_hashes` ends with `board` itself, kept for the
            // threefold check above, which the search's history must not hold (`MoveRequest`).
            ancestors: &ancestor_hashes[..ancestor_hashes.len() - 1],
            limit,
        };

        let think_started = Instant::now();
        let move_outcome = if side_to_move == Color::White {
            white_player.choose_move(&request, stop_requested)
        } else {
            black_player.choose_move(&request, stop_requested)
        };
        let elapsed = think_started.elapsed();

        let report = match move_outcome {
            Ok(MoveOutcome::Move(report)) => report,
            Ok(MoveOutcome::TimedOut) => {
                let result = time_forfeit_result(&board, side_to_move);
                break PlyOutcome::Decided(GameOutcome { result, termination: Termination::TimeForfeit, ply_count: ply, moves: annotations });
            }
            Ok(MoveOutcome::Interrupted) => break PlyOutcome::Interrupted,
            Err(fault) => {
                break PlyOutcome::Fault { faulting_color: side_to_move, message: fault_message(fault), moves: annotations, ply_count: ply };
            }
        };

        let clock = if side_to_move == Color::White { &mut white_clock } else { &mut black_clock };
        let remaining_clock = match clock {
            Some(clock) => {
                if !clock.spend(elapsed) {
                    let result = time_forfeit_result(&board, side_to_move);
                    break PlyOutcome::Decided(GameOutcome { result, termination: Termination::TimeForfeit, ply_count: ply, moves: annotations });
                }
                Some(clock.remaining())
            }
            None => None,
        };

        let Some(mv) = parse_move(&report.best_move_uci, &legal) else {
            break PlyOutcome::Fault {
                faulting_color: side_to_move,
                message: format!("illegal or unparseable move {:?}", report.best_move_uci),
                moves: annotations,
                ply_count: ply,
            };
        };

        let mv_uci = format_move(mv);
        if print_moves {
            let eval = format_eval(report.score_cp, report.mate_in);
            let depth = format_depth(report.depth);
            let clock_display = format_clock(remaining_clock);
            if side_to_move == Color::White {
                print!("game {game_number} {fullmove_number:>3}. {mv_uci:<8}{eval:>8} {depth:<4}{clock_display:>8}    ");
                std::io::stdout().flush().ok();
                pending_newline = true;
            } else if ply == 0 {
                // The opening itself starts mid-move, with Black to move first: the row gets its
                // number and an empty White cell of the same width, so Black's move sits in Black's
                // column like every other row. The PGN keeps its own "5..." convention (pgn.rs).
                let empty_white_cell = format!("{:<8}{:>8} {:<4}{:>8}    ", "", "", "", "");
                println!(
                    "game {game_number} {fullmove_number:>3}. {empty_white_cell}{mv_uci:<8}{eval:>8} {depth:<4}{clock_display:>8}"
                );
                pending_newline = false;
            } else {
                println!("{mv_uci:<8}{eval:>8} {depth:<4}{clock_display:>8}");
                pending_newline = false;
            }
        }
        if side_to_move == Color::Black {
            fullmove_number += 1;
        }

        annotations.push(MoveAnnotation { uci: mv_uci.clone(), score_cp: report.score_cp, mate_in: report.mate_in, depth: report.depth, time: elapsed, best: None });
        moves_uci.push(mv_uci);
        board.make_move(mv);
        ancestor_hashes.push(board.hash());
        ply += 1;

        if early_stop.is_some() {
            let zeroing = board.halfmove_clock() == 0;
            // `fullmove_number` is the move now under way (already advanced past a Black move), so one
            // fewer is complete.
            if let Some((result, termination)) =
                adjudicator.observe(side_to_move, report.score_cp, report.mate_in, zeroing, fullmove_number - 1)
            {
                break PlyOutcome::Decided(GameOutcome { result, termination, ply_count: ply, moves: annotations });
            }
        }
    };

    if pending_newline {
        println!();
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_clock_shows_a_dash_when_the_control_has_no_clock() {
        assert_eq!(format_clock(Some(Duration::from_millis(65_400))), "1:05.4");
        assert_eq!(format_clock(None), "-");
    }

    #[test]
    fn format_clock_never_shows_sixty_seconds() {
        assert_eq!(format_clock(Some(Duration::from_millis(59_999))), "0:59.9");
        assert_eq!(format_clock(Some(Duration::from_millis(60_000))), "1:00.0");
        assert_eq!(format_clock(Some(Duration::from_millis(119_960))), "1:59.9");
        assert_eq!(format_clock(Some(Duration::from_millis(5_049))), "0:05.0");
    }

    #[test]
    fn format_eval_prefers_a_mate_count_over_centipawns_and_signs_it() {
        assert_eq!(format_eval(Some(35), None), "+0.35");
        assert_eq!(format_eval(None, Some(3)), "#3");
        assert_eq!(format_eval(None, Some(-3)), "-#3");
        assert_eq!(format_eval(None, None), "?");
    }

    #[test]
    fn a_time_forfeit_is_drawn_only_when_the_other_side_cannot_mate() {
        let result = |fen: &str| -> f64 {
            let (board, _) = Board::from_fen(fen).unwrap();
            time_forfeit_result(&board, board.side_to_move())
        };
        // White flags against a bare king, whatever White still has: drawn.
        assert_eq!(result("4k3/8/8/8/8/8/8/Q3K3 w - - 0 1"), 0.5);
        // White flags with a bare king against a lone knight: dead, drawn.
        assert_eq!(result("4k3/8/8/3n4/8/8/8/4K3 w - - 0 1"), 0.5);
        // Same-colored bishops: dead, drawn.
        assert_eq!(result("4kb2/8/8/8/8/8/8/2B1K3 w - - 0 1"), 0.5);
        // A knight against king and pawn can still mate with the pawn's help: White loses.
        assert_eq!(result("4k3/8/8/3n4/8/8/4P3/4K3 w - - 0 1"), 0.0);
        // Black flags against a queen: Black loses.
        assert_eq!(result("4k3/8/8/8/8/8/8/Q3K3 b - - 0 1"), 1.0);
    }

    /// Feeds `plies` reports, alternating movers from White, each `(score_cp, mate_in)`, none zeroing,
    /// all at `full_moves_complete`; the ply (1-based) and verdict of the first adjudication, if any.
    fn adjudicate(plies: &[(Option<i32>, Option<i32>)], full_moves_complete: u32) -> Option<(usize, f64, Termination)> {
        let mut adjudicator = Adjudicator::default();
        for (index, &(score_cp, mate_in)) in plies.iter().enumerate() {
            let mover = if index % 2 == 0 { Color::White } else { Color::Black };
            if let Some((result, termination)) = adjudicator.observe(mover, score_cp, mate_in, false, full_moves_complete) {
                return Some((index + 1, result, termination));
            }
        }
        None
    }

    /// White ahead by `cp` in both sides' own scores: White reports `+cp`, Black `-cp`.
    fn white_ahead(cp: i32, plies: usize) -> Vec<(Option<i32>, Option<i32>)> {
        (0..plies).map(|ply| (Some(if ply % 2 == 0 { cp } else { -cp }), None)).collect()
    }

    #[test]
    fn six_agreeing_plies_win_and_nothing_less() {
        assert_eq!(adjudicate(&white_ahead(600, 6), 10), Some((6, 1.0, Termination::AdjudicatedWin)));
        assert_eq!(adjudicate(&white_ahead(600, 5), 10), None);
        assert_eq!(adjudicate(&white_ahead(599, 12), 10), None, "inside the band");
        // Black ahead: Black's own scores positive, White's negative.
        assert_eq!(adjudicate(&white_ahead(-700, 6), 10), Some((6, 0.0, Termination::AdjudicatedWin)));
    }

    #[test]
    fn a_disagreement_a_missing_score_or_one_inside_the_band_resets_the_win_run() {
        let mut plies = white_ahead(700, 10);
        plies[3] = (Some(700), None); // Black also claims to be ahead.
        assert_eq!(adjudicate(&plies, 10), Some((10, 1.0, Termination::AdjudicatedWin)), "restarts at ply 5");
        let mut plies = white_ahead(700, 10);
        plies[3] = (None, None);
        assert_eq!(adjudicate(&plies, 10), Some((10, 1.0, Termination::AdjudicatedWin)));
        let mut plies = white_ahead(700, 10);
        plies[3] = (Some(-599), None);
        assert_eq!(adjudicate(&plies, 10), Some((10, 1.0, Termination::AdjudicatedWin)));
    }

    #[test]
    fn mate_scores_count_for_the_win_rule() {
        let plies: Vec<_> = (0..6).map(|ply| if ply % 2 == 0 { (None, Some(5)) } else { (None, Some(-5)) }).collect();
        assert_eq!(adjudicate(&plies, 10), Some((6, 1.0, Termination::AdjudicatedWin)));
    }

    #[test]
    fn sixteen_quiet_plies_draw_only_after_move_thirty_four() {
        let quiet = white_ahead(20, 16);
        assert_eq!(adjudicate(&quiet, 34), Some((16, 0.5, Termination::AdjudicatedDraw)));
        assert_eq!(adjudicate(&quiet[..15], 34), None);
        assert_eq!(adjudicate(&white_ahead(20, 100), 33), None, "too early, however long the run");
        assert_eq!(adjudicate(&white_ahead(21, 16), 34), None, "outside the band");
    }

    #[test]
    fn a_wide_score_a_mate_a_missing_score_or_a_zeroing_move_resets_the_draw_run() {
        for breaker in [(Some(21), None), (None, Some(3)), (None, None)] {
            let mut plies = white_ahead(0, 20);
            plies[3] = breaker;
            assert_eq!(adjudicate(&plies, 40), Some((20, 0.5, Termination::AdjudicatedDraw)), "{breaker:?}");
        }
        // A zeroing move ends the run, and its own quiet score starts the next.
        let mut adjudicator = Adjudicator::default();
        for ply in 0..15 {
            assert!(adjudicator.observe(Color::White, Some(0), None, false, 40).is_none(), "ply {ply}");
        }
        assert!(adjudicator.observe(Color::White, Some(0), None, true, 40).is_none());
        for _ in 0..14 {
            assert!(adjudicator.observe(Color::White, Some(0), None, false, 40).is_none());
        }
        assert_eq!(adjudicator.observe(Color::White, Some(0), None, false, 40), Some((0.5, Termination::AdjudicatedDraw)));
    }

    /// Skipped where `bin/syzygy.sh` has not run.
    #[test]
    fn the_tablebase_rule_scores_under_the_fifty_move_rule() {
        let Ok(tablebase) = SyzygyTablebase::from_directories(&super::super::data_root::syzygy_directories()) else {
            println!("skipped: no Syzygy WDL and DTZ tables");
            return;
        };
        let result = |fen: &str| -> Option<f64> {
            let (board, _) = Board::from_fen(fen).unwrap();
            tablebase_result(&tablebase, &board, &[])
        };
        assert_eq!(result("4k3/8/8/8/8/8/8/Q3K3 w - - 0 1"), Some(1.0), "KQK, White to move, wins");
        assert_eq!(result("4k3/8/8/8/8/8/8/Q3K3 b - - 0 1"), Some(1.0), "KQK, Black to move, still lost");
        // Not two rooks on one open file, where the side to move simply takes the other's.
        assert_eq!(result("4k3/7r/8/8/8/8/8/R3K3 w - - 0 1"), Some(0.5), "KRKR");
        assert_eq!(result("4k3/8/8/8/8/8/8/Q3K3 w - - 98 80"), Some(0.5), "a win the clock frustrates");
        assert_eq!(result("4k3/7r/8/8/8/8/8/R3K3 w Q - 0 1"), None, "castling rights left");
    }

    #[test]
    fn decisive_or_draw_scores_the_side_to_move_as_the_loser() {
        assert_eq!(decisive_or_draw(true, Color::White), 0.0);
        assert_eq!(decisive_or_draw(true, Color::Black), 1.0);
        assert_eq!(decisive_or_draw(false, Color::White), 0.5);
    }
}
