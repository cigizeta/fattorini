//! Real Fischer time control -- base time plus a per-move increment, both in seconds, e.g. `"180+2"`
//! (3 minutes base, 2 second increment) -- parsed from `config.yaml`'s `match.base_time_control`/
//! `tiebreak_time_control` fields. Not a flat per-move budget: decided directly with the
//! developer, see the tournament design's "Clock" section.

use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub struct TimeControl {
    pub base: Duration,
    pub increment: Duration,
}

impl TimeControl {
    /// Parses `"<seconds>+<seconds>"`, base and increment, either one a decimal: `"180+2"` is
    /// 3 minutes and 2 seconds a move, `"10+0.1"` Stockfish's short testing control. Seconds only,
    /// no minutes, so a control never reads two ways. Panics on anything else; `parse_checked`
    /// says why instead.
    pub fn parse(text: &str) -> TimeControl {
        TimeControl::parse_checked(text).unwrap_or_else(|message| panic!("{message}"))
    }

    /// `parse`, or why `text` is not a time control.
    pub fn parse_checked(text: &str) -> Result<TimeControl, String> {
        let invalid = || {
            format!("a time control is <seconds>+<seconds>, e.g. 180+2 or 10+0.1, got {text:?}")
        };
        let (base_text, increment_text) = text.split_once('+').ok_or_else(invalid)?;
        let base = base_text
            .parse::<f64>()
            .ok()
            .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
            .ok_or_else(invalid)?;
        let increment = increment_text
            .parse::<f64>()
            .ok()
            .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
            .ok_or_else(invalid)?;
        Ok(TimeControl { base: Duration::from_secs_f64(base), increment: Duration::from_secs_f64(increment) })
    }
}

/// One side's own remaining time during a game -- decremented by the actual measured
/// wall-clock time between sending `go` and receiving that side's `bestmove`, incremented by
/// `increment` after every move it completes in time. Reaching zero is a time-forfeit loss -- a
/// normal game outcome (counts toward the pairing's own running score), not a fault: see
/// the tournament design's "zero tolerance" fault handling, which is about engine malfunctions, not
/// about losing on time.
#[derive(Debug, Clone, Copy)]
pub struct Clock {
    remaining: Duration,
    increment: Duration,
}

impl Clock {
    pub fn new(time_control: TimeControl) -> Clock {
        Clock { remaining: time_control.base, increment: time_control.increment }
    }

    pub fn remaining(&self) -> Duration {
        self.remaining
    }

    /// Spends `elapsed` off the clock, then adds the increment back -- the same order a real
    /// Fischer clock uses (the increment is earned for having moved, not a head start before
    /// the move that earns it). Returns `false` if `elapsed` exhausted the clock before the
    /// increment was ever added (a time forfeit), `true` otherwise.
    pub fn spend(&mut self, elapsed: Duration) -> bool {
        if elapsed >= self.remaining {
            self.remaining = Duration::ZERO;
            return false;
        }
        self.remaining -= elapsed;
        self.remaining += self.increment;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reads_seconds_and_seconds() {
        let time_control = TimeControl::parse("180+2");
        assert_eq!(time_control.base, Duration::from_secs(180));
        assert_eq!(time_control.increment, Duration::from_secs(2));
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
    fn spend_subtracts_then_adds_the_increment() {
        let mut clock = Clock::new(TimeControl { base: Duration::from_secs(10), increment: Duration::from_secs(2) });
        assert!(clock.spend(Duration::from_secs(3)));
        assert_eq!(clock.remaining(), Duration::from_secs(9)); // 10 - 3 + 2
    }

    #[test]
    fn spend_reaching_or_exceeding_remaining_time_is_a_forfeit() {
        let mut clock = Clock::new(TimeControl { base: Duration::from_secs(5), increment: Duration::from_secs(2) });
        assert!(!clock.spend(Duration::from_secs(5)));
        assert_eq!(clock.remaining(), Duration::ZERO);
    }
}
