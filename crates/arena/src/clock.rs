//! Real time controls, in seconds, parsed from `config.yaml`'s `match.base_time_control`/
//! `tiebreak_time_control` fields and the command line's `--time`: Fischer, base time plus a
//! per-move increment (`"180+2"`, 3 minutes and 2 seconds a move), or repeating, a base for every
//! so many moves (`"40/900"`, CCRL's 40/15), with an increment or not (`"40/900+1"`). Not a flat
//! per-move budget: decided directly with the developer, see the tournament design's "Clock"
//! section.

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeControl {
    pub base: Duration,
    pub increment: Duration,
    /// Under a repeating control, the moves each base must cover before it is given again; `None`
    /// under Fischer.
    pub moves: Option<u32>,
}

impl TimeControl {
    /// Parses `"<seconds>+<seconds>"` (Fischer), `"<moves>/<seconds>"` or
    /// `"<moves>/<seconds>+<seconds>"` (repeating): `"180+2"` is 3 minutes and 2 seconds a move,
    /// `"10+0.1"` Stockfish's short testing control, `"40/900"` 15 minutes every 40 moves. Seconds
    /// only, no minutes, so a control never reads two ways; seconds may be decimals, moves are a
    /// whole number from 1. Panics on anything else; `parse_checked` says why instead.
    pub fn parse(text: &str) -> TimeControl {
        TimeControl::parse_checked(text).unwrap_or_else(|message| panic!("{message}"))
    }

    /// `parse`, or why `text` is not a time control.
    pub fn parse_checked(text: &str) -> Result<TimeControl, String> {
        let invalid = || {
            format!(
                "a time control is <seconds>+<seconds>, <moves>/<seconds> or <moves>/<seconds>+<seconds>, \
                 e.g. 180+2, 10+0.1, 40/900 or 40/900+1, got {text:?}"
            )
        };
        let seconds = |part: &str, positive: bool| {
            part.parse::<f64>()
                .ok()
                .filter(|seconds| seconds.is_finite() && if positive { *seconds > 0.0 } else { *seconds >= 0.0 })
                .map(Duration::from_secs_f64)
                .ok_or_else(invalid)
        };
        let (moves, rest) = match text.split_once('/') {
            Some((moves_text, rest)) => {
                let moves = moves_text.parse::<u32>().ok().filter(|moves| *moves >= 1).ok_or_else(invalid)?;
                (Some(moves), rest)
            }
            None => (None, text),
        };
        let (base_text, increment_text) = match (moves, rest.split_once('+')) {
            (_, Some((base_text, increment_text))) => (base_text, Some(increment_text)),
            // Fischer needs its increment, written even when 0; a repeating control may leave it out.
            (Some(_), None) => (rest, None),
            (None, None) => return Err(invalid()),
        };
        let base = seconds(base_text, true)?;
        let increment = match increment_text {
            Some(increment_text) => seconds(increment_text, false)?,
            None => Duration::ZERO,
        };
        Ok(TimeControl { base, increment, moves })
    }
}

/// One side's own remaining time during a game -- decremented by the actual measured
/// wall-clock time between sending `go` and receiving that side's `bestmove`, incremented by
/// `increment` after every move it completes in time, and under a repeating control given `base`
/// again after the last move of every period, the unspent time carried over. Reaching zero is a
/// time-forfeit loss -- a normal game outcome (counts toward the pairing's own running score), not a
/// fault: see the tournament design's "zero tolerance" fault handling, which is about engine
/// malfunctions, not about losing on time.
#[derive(Debug, Clone, Copy)]
pub struct Clock {
    remaining: Duration,
    increment: Duration,
    base: Duration,
    /// The period's length under a repeating control; `None` under Fischer.
    moves: Option<u32>,
    /// The moves still to play before `base` is given again; meaningless under Fischer.
    moves_left: u32,
}

impl Clock {
    pub fn new(time_control: TimeControl) -> Clock {
        Clock {
            remaining: time_control.base,
            increment: time_control.increment,
            base: time_control.base,
            moves: time_control.moves,
            moves_left: time_control.moves.unwrap_or(0),
        }
    }

    pub fn remaining(&self) -> Duration {
        self.remaining
    }

    /// The moves this side has left before its next `base`, UCI's `movestogo`; `None` under
    /// Fischer.
    pub fn moves_to_go(&self) -> Option<u32> {
        self.moves.map(|_| self.moves_left)
    }

    /// Spends `elapsed` off the clock, then adds the increment back -- the same order a real
    /// Fischer clock uses (the increment is earned for having moved, not a head start before
    /// the move that earns it) -- and, the period's last move made, the base. Returns `false` if
    /// `elapsed` exhausted the clock before anything was added (a time forfeit), `true` otherwise.
    pub fn spend(&mut self, elapsed: Duration) -> bool {
        if elapsed >= self.remaining {
            self.remaining = Duration::ZERO;
            return false;
        }
        self.remaining -= elapsed;
        self.remaining += self.increment;
        if let Some(moves) = self.moves {
            self.moves_left -= 1;
            if self.moves_left == 0 {
                self.remaining += self.base;
                self.moves_left = moves;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fischer(base_secs: u64, increment_secs: u64) -> TimeControl {
        TimeControl { base: Duration::from_secs(base_secs), increment: Duration::from_secs(increment_secs), moves: None }
    }

    #[test]
    fn parse_reads_seconds_and_seconds() {
        let time_control = TimeControl::parse("180+2");
        assert_eq!(time_control, fischer(180, 2));
    }

    #[test]
    fn a_decimal_increment_writes_stockfishs_short_control() {
        let time_control = TimeControl::parse_checked("10+0.1").unwrap();
        assert_eq!(time_control.base, Duration::from_secs(10));
        assert_eq!(time_control.increment, Duration::from_millis(100));
        assert_eq!(TimeControl::parse_checked("1.5+2").unwrap().base, Duration::from_millis(1500));
        for text in ["10+0.1x", "10", "0+1", "-1+1", "10+-1", "10s+0.1", "x+1"] {
            assert!(TimeControl::parse_checked(text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn parse_reads_a_repeating_control_with_or_without_an_increment() {
        assert_eq!(
            TimeControl::parse_checked("40/900").unwrap(),
            TimeControl { base: Duration::from_secs(900), increment: Duration::ZERO, moves: Some(40) }
        );
        assert_eq!(
            TimeControl::parse_checked("40/60+0.5").unwrap(),
            TimeControl { base: Duration::from_secs(60), increment: Duration::from_millis(500), moves: Some(40) }
        );
        assert_eq!(TimeControl::parse_checked("1/2.5").unwrap().base, Duration::from_millis(2500));
        for text in ["0/900", "-1/900", "40.5/900", "x/900", "40/", "40/0", "40/-1", "/900", "40/900+", "40/900+-1", "40/900/2"] {
            assert!(TimeControl::parse_checked(text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn spend_subtracts_then_adds_the_increment() {
        let mut clock = Clock::new(fischer(10, 2));
        assert!(clock.spend(Duration::from_secs(3)));
        assert_eq!(clock.remaining(), Duration::from_secs(9)); // 10 - 3 + 2
        assert_eq!(clock.moves_to_go(), None, "Fischer");
    }

    #[test]
    fn spend_reaching_or_exceeding_remaining_time_is_a_forfeit() {
        let mut clock = Clock::new(fischer(5, 2));
        assert!(!clock.spend(Duration::from_secs(5)));
        assert_eq!(clock.remaining(), Duration::ZERO);
    }

    #[test]
    fn a_repeating_clock_gives_the_base_again_after_the_periods_last_move() {
        let mut clock = Clock::new(TimeControl::parse("3/30"));
        assert_eq!(clock.moves_to_go(), Some(3));
        assert!(clock.spend(Duration::from_secs(5)));
        assert_eq!((clock.remaining(), clock.moves_to_go()), (Duration::from_secs(25), Some(2)));
        assert!(clock.spend(Duration::from_secs(5)));
        assert_eq!((clock.remaining(), clock.moves_to_go()), (Duration::from_secs(20), Some(1)));
        assert!(clock.spend(Duration::from_secs(5)));
        assert_eq!((clock.remaining(), clock.moves_to_go()), (Duration::from_secs(45), Some(3)), "15 carried over");
    }

    #[test]
    fn a_repeating_clock_adds_its_increment_every_move() {
        let mut clock = Clock::new(TimeControl::parse("2/30+1"));
        assert!(clock.spend(Duration::from_secs(5)));
        assert_eq!(clock.remaining(), Duration::from_secs(26)); // 30 - 5 + 1
        assert!(clock.spend(Duration::from_secs(5)));
        assert_eq!(clock.remaining(), Duration::from_secs(52)); // 26 - 5 + 1 + 30
        assert_eq!(clock.moves_to_go(), Some(2));
    }

    #[test]
    fn a_repeating_clock_still_forfeits_on_the_periods_last_move() {
        let mut clock = Clock::new(TimeControl::parse("1/5"));
        assert!(!clock.spend(Duration::from_secs(5)), "the base comes after the move, not before");
        assert_eq!(clock.remaining(), Duration::ZERO);
    }
}
