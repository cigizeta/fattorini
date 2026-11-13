//! What the game loop needs from a side to move, and nothing more: one call per move.
//!
//! `play_one_game` used to drive `UciClient` through four calls per ply -- `set_position`, `go`,
//! `wait_for_bestmove`, plus `new_game` once per game. That split is UCI's own shape, not a game
//! loop's, and an in-process player that had to imitate it would be pretending to be a protocol:
//! it has no position to send ahead of a search and no asynchronous reply to wait for. Collapsing
//! the three per-ply calls into `choose_move` lets each implementation be as simple as it actually
//! is, and leaves the game loop written in terms of "ask whoever is to move for a move".
//!
//! `Fault`, `SearchReport` and `MoveOutcome` live here rather than in `uci_client.rs` because they
//! are this trait's own vocabulary -- every player answers in them, including the one that has no
//! subprocess and therefore no protocol to violate.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use fattorini::board::color::Color;
use fattorini::board::Board;

use super::clock::TimeControl;

/// A fault is a protocol violation or an engine malfunction -- never a normal chess outcome (a
/// time forfeit is handled separately, by `Clock`, as a normal game-ending result that still
/// counts toward the pairing's own score). Zero tolerance, decided directly with the developer:
/// any fault disqualifies the offending engine from the *whole pairing* immediately, regardless
/// of the score so far -- see the tournament design's "zero tolerance" fault handling.
#[derive(Debug)]
pub enum Fault {
    HandshakeTimedOut,
    Crashed(String),
    IllegalMove(String),
}

/// The last `info` line's own score/depth seen before `bestmove` arrived -- feeds the PGN
/// writer's cutechess-shaped `{score/depth time}` annotation (see `pgn.rs`). `score_cp`/
/// `mate_in` are mutually exclusive: whichever kind of score the engine last reported wins,
/// matching how a real `info score cp|mate` line itself only ever reports one at a time.
pub struct SearchReport {
    pub best_move_uci: String,
    pub score_cp: Option<i32>,
    pub mate_in: Option<i32>,
    pub depth: Option<u32>,
}

pub enum MoveOutcome {
    Move(SearchReport),
    TimedOut,
    Interrupted,
}

/// How a match limits each search: a real Fischer clock, or a fixed depth with no clock at all.
///
/// The gate needs the second one. Both sides of an evaluation A/B must search to the identical
/// depth, so that the only difference between them is the evaluation function -- under a clock the
/// network side would simply be measured on its own speed handicap, which is already known and is
/// not the question. See the network gate design.
#[derive(Debug, Clone, Copy)]
pub enum Control {
    Fischer(TimeControl),
    FixedDepth(u32),
}

impl Control {
    /// Parses `match.base_time_control`/`tiebreak_time_control`: `"180+2"` stays Fischer and goes to
    /// `TimeControl::parse` unchanged, `"depth 9"` becomes `FixedDepth(9)`. One extra grammar on
    /// the field that already exists, rather than a second config concept beside it.
    pub fn parse(text: &str) -> Control {
        if let Some(depth_text) = text.trim().strip_prefix("depth ") {
            let depth: u32 = depth_text
                .trim()
                .parse()
                .unwrap_or_else(|error| panic!("invalid control {text:?}: {error}"));
            assert!(depth >= 1, "invalid control {text:?}: depth must be at least 1");
            return Control::FixedDepth(depth);
        }
        Control::Fischer(TimeControl::parse(text))
    }

    /// The per-side starting clock, or `None` under a fixed depth -- `play_one_game` builds its two
    /// `Clock`s from this and skips clock handling entirely when there is none.
    pub fn time_control(&self) -> Option<TimeControl> {
        match self {
            Control::Fischer(time_control) => Some(*time_control),
            Control::FixedDepth(_) => None,
        }
    }
}

/// What the side to move is told, once per ply. Both halves are already in hand at the call site,
/// so each implementation reads the half it needs and neither has to reconstruct anything: a UCI
/// player replays the game from `starting_fen` plus `moves_uci`, while an in-process player
/// searches `board` directly and hands `ancestors` to its own repetition detection.
pub struct MoveRequest<'a> {
    pub starting_fen: &'a str,
    pub moves_uci: &'a [String],
    pub board: &'a Board,
    /// The game's positions before `board`, oldest first, never `board` itself: what the search
    /// and the tablebase root ranking (`SyzygyTablebase::excluded_root_moves`) take, as UCI gives
    /// them. With `board` at the end, the ranking would see a repetition at every root with a
    /// nonzero fifty-move counter.
    pub ancestors: &'a [u64],
    pub limit: MoveLimit,
}

/// This ply's own limit, as opposed to `Control`'s match-wide one: under a clock the searching side
/// needs the *live* readings of both clocks, which `Control` cannot carry because it is a config
/// value shared by every game of the match.
///
/// `Fischer` covers every clock, repeating ones included: `moves_to_go` is the side to move's own
/// count of moves before its next base (`Clock::moves_to_go`), UCI's `movestogo`, `None` under
/// Fischer proper.
#[derive(Debug, Clone, Copy)]
pub enum MoveLimit {
    Fischer { white_remaining: Duration, black_remaining: Duration, increment: Duration, moves_to_go: Option<u32> },
    FixedDepth(u32),
}

/// How long a fixed-depth search is allowed to take before the subprocess is treated as having
/// stopped answering. There is no clock at a fixed depth, so this is a hang backstop rather than a
/// time control, and it is set far above any plausible real search -- the deepest gate search is
/// depth 9, seconds of work, and Fattorini's own in-process player cannot reach this path at all since
/// it returns from `choose_best_move` or not at all.
const FIXED_DEPTH_BACKSTOP: Duration = Duration::from_secs(3600);

impl MoveLimit {
    /// The mover's own wall-clock allowance, which is what a subprocess is timed against.
    pub fn budget(&self, side_to_move: Color) -> Duration {
        match self {
            MoveLimit::Fischer { white_remaining, black_remaining, .. } => {
                if side_to_move == Color::White {
                    *white_remaining
                } else {
                    *black_remaining
                }
            }
            MoveLimit::FixedDepth(_) => FIXED_DEPTH_BACKSTOP,
        }
    }
}

/// One side of a game. Implemented by `UciPlayer` over a subprocess and by `InProcessPlayer` over
/// this crate's own `Search`.
pub trait Player {
    /// Called once per game, before its first move, so a player can clear whatever it carries
    /// between games -- its hash table above all.
    fn new_game(&mut self) -> Result<(), Fault>;

    fn choose_move(&mut self, request: &MoveRequest, stop_requested: &Arc<AtomicBool>) -> Result<MoveOutcome, Fault>;

    /// Called on every exit path, normal or not: end of pairing, a fault, or Ctrl+C. Infallible on
    /// purpose -- there is nothing useful to do about a failure to shut down, and every caller is
    /// already on its way out.
    fn shutdown(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reads_a_fischer_control_exactly_as_before() {
        match Control::parse("180+2") {
            Control::Fischer(time_control) => {
                assert_eq!(time_control.base, Duration::from_secs(180));
                assert_eq!(time_control.increment, Duration::from_secs(2));
            }
            Control::FixedDepth(depth) => panic!("expected Fischer, got depth {depth}"),
        }
    }

    #[test]
    fn parse_reads_a_fixed_depth_control() {
        match Control::parse("depth 9") {
            Control::FixedDepth(depth) => assert_eq!(depth, 9),
            Control::Fischer(_) => panic!("expected a fixed depth"),
        }
    }

    #[test]
    fn parse_reads_a_repeating_control_as_a_clock() {
        match Control::parse("40/900") {
            Control::Fischer(time_control) => assert_eq!(time_control.moves, Some(40)),
            Control::FixedDepth(depth) => panic!("expected a clock, got depth {depth}"),
        }
    }

    #[test]
    fn a_fischer_control_still_has_a_time_control_and_a_fixed_depth_has_none() {
        assert!(Control::parse("3+2").time_control().is_some());
        assert!(Control::parse("depth 9").time_control().is_none());
    }

    #[test]
    fn a_fischer_limit_budgets_the_mover_s_own_remaining_time() {
        let limit = MoveLimit::Fischer {
            white_remaining: Duration::from_secs(30),
            black_remaining: Duration::from_secs(20),
            increment: Duration::from_secs(2),
            moves_to_go: None,
        };
        assert_eq!(limit.budget(Color::White), Duration::from_secs(30));
        assert_eq!(limit.budget(Color::Black), Duration::from_secs(20));
    }
}
