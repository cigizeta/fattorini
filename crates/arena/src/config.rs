//! `data/chess/tournaments/<id>/config.yaml`: a tournament specification. See the tournament design's
//! "Tournament: single-elimination knockout" and "Fitness selection" sections. And the `selection:`
//! section of a network's `net.yaml`, the one thing `arena select` reads from it: the depth,
//! the round robin's pairs, and the gate's rule. Plus the rule and control values themselves, which
//! the `match`, `sprt` and `duel` commands take from the command line.
//!
//! A network's selection is not a tournament: its players are derived from the network's current
//! generation, and its record lives in that generation's own directory. So a tournament is best-of
//! only, and refuses the `decision`/`sprt`/`threads` keys a gate used to be configured with here
//! rather than silently running as a best-of match.

use serde::Deserialize;

use super::player::Control;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatchSpec {
    pub base_time_control: String,
    /// The control of the tie-break pairs played after `best_of` games end level. `None` has no
    /// tie-break: such a match ends drawn (`arena match`). A tournament always has one
    /// (`TournamentConfig::load`), since a knockout needs a winner.
    pub tiebreak_time_control: Option<String>,
    /// First to strictly more than half of `best_of` games wins the pairing outright, without
    /// necessarily playing all of them. Config-overridable, not a fixed constant -- decided
    /// directly with the developer, matching the same call already made for the two time
    /// controls. Default `12` (a real World Championship classical match) if omitted.
    #[serde(default = "default_best_of")]
    pub best_of: u32,
}

/// A run's sequential test: the margin it is powered to detect, the one chosen number. The error rates
/// are `sprt::ALPHA` and `sprt::BETA`, fixed for every run; everything else in the test is derived at
/// construction (`sprt.rs`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SprtSpec {
    pub elo_d: f64,
}

impl SprtSpec {
    /// The range `Sprt::new` asserts, checked first so a caller can report it rather than panic:
    /// `net.yaml` for the gate, the command line for `sprt`.
    pub fn validate(&self) -> Result<(), String> {
        // Negated on purpose: a NaN `elo_d` fails `> 0.0` and is refused, where `<= 0.0` would pass it.
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if !(self.elo_d > 0.0) {
            return Err(format!("elo_d = {} must be positive", self.elo_d));
        }
        Ok(())
    }
}

/// One of the three decision rules (`runner::SequentialTest`), with its one value where it has one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Rule {
    /// First past n/2 wins, n/2 each is drawn. n is even: results arrive in pairs.
    BestOf(u32),
    /// Whether the challenger (the second player) is at least `elo_d` stronger (`sprt.rs`).
    Sprt(SprtSpec),
    /// Which of the two is stronger (`duel.rs`).
    Duel,
}

impl Rule {
    /// A best of `n`, or why `n` is not one.
    pub fn best_of(n: u32) -> Result<Rule, String> {
        if n < 2 || !n.is_multiple_of(2) {
            return Err(format!("a best of n needs an even n of at least 2, since results arrive in pairs, got {n}"));
        }
        Ok(Rule::BestOf(n))
    }

    /// An SPRT at `elo_d`, or why `elo_d` is not a margin.
    pub fn sprt(elo_d: f64) -> Result<Rule, String> {
        let spec = SprtSpec { elo_d };
        spec.validate()?;
        Ok(Rule::Sprt(spec))
    }

    /// The rule as `spec.txt` names it, written from the parsed value, so `35` and `35.0` name the
    /// same test.
    pub fn spec_text(&self) -> String {
        match self {
            Rule::BestOf(n) => format!("best-of {n}"),
            Rule::Sprt(spec) => format!("sprt elo_d {}", spec.elo_d),
            Rule::Duel => "duel".to_string(),
        }
    }
}

/// The deepest fixed depth a run may play at: self-play's own, `generation::SEARCH_DEPTH`.
pub const MAX_DEPTH: u32 = 9;

/// How every game of a run is limited, with the text `spec.txt` names it by: `depth 9`, `3+2`.
#[derive(Debug, Clone)]
pub struct ControlSpec {
    pub control: Control,
    pub text: String,
}

impl ControlSpec {
    /// A fixed depth from 1 to `MAX_DEPTH`, given as text.
    pub fn depth(value: &str) -> Result<ControlSpec, String> {
        let depth: u32 = value
            .parse()
            .ok()
            .filter(|depth| (1..=MAX_DEPTH).contains(depth))
            .ok_or_else(|| format!("a depth is 1 to {MAX_DEPTH}, got {value:?}"))?;
        Ok(ControlSpec { control: Control::FixedDepth(depth), text: format!("depth {depth}") })
    }

    /// A Fischer control, `<seconds>+<seconds>` (`TimeControl::parse_checked`), checked here so a typo is reported rather than left to a panic.
    pub fn time(value: &str) -> Result<ControlSpec, String> {
        super::clock::TimeControl::parse_checked(value)?;
        Ok(ControlSpec { control: Control::parse(value), text: value.to_string() })
    }

    /// This control on a machine `factor` times slower than the reference: a Fischer control's base
    /// and increment both times `factor`, to the millisecond, its text one `time` parses back
    /// (`10+0.1` at 1.12 is `11.2+0.112`); a fixed depth unchanged, being the same on every machine.
    /// A tune's clients play at it (`client`).
    pub fn scaled(&self, factor: f64) -> ControlSpec {
        let Control::Fischer(time_control) = self.control else {
            return self.clone();
        };
        let milliseconds = |duration: std::time::Duration| -> f64 {
            (duration.as_secs_f64() * factor * 1000.0).round() / 1000.0
        };
        let text = format!("{}+{}", milliseconds(time_control.base), milliseconds(time_control.increment));
        ControlSpec::time(&text).expect("a scaled control is a control")
    }
}

fn default_best_of() -> u32 {
    12
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TournamentConfig {
    pub name: String,
    pub format: String,
    pub participants: Vec<String>,
    #[serde(rename = "match")]
    pub match_spec: MatchSpec,
}

/// Where a tournament id's own directory lives -- `data/chess/tournaments/{id}/`, flat, holding
/// `config.yaml` plus that tournament's own generated `games.pgn`/`results.db` -- the same
/// "bare name in, real path out" convention `preprocessing_config::net_config_path` already
/// established for `configs/chess/{name}.yaml`.
pub fn tournament_dir(id: &str) -> String {
    format!("data/chess/tournaments/{id}")
}

/// `data/chess/tournaments/{id}/config.yaml` -- the hand-written spec inside `tournament_dir(id)`,
/// colocated with that same directory's own generated `games.pgn`/`results.db` rather than
/// living separately under `configs/chess/` -- see the tournament design's "PGN writer" section (the
/// `data/chess/tournaments/<id>/` layout decision).
pub fn config_path(id: &str) -> String {
    format!("{}/config.yaml", tournament_dir(id))
}

impl TournamentConfig {
    /// Reads and validates `path`. Panics on anything wrong with it -- a missing file, invalid
    /// YAML, an odd `best_of`, or fewer than two participants -- the same panic discipline
    /// `self_play_command::load_preprocessing` already established for `net.yaml` (CLAUDE.md
    /// rule 4: assertions, not eyeballing).
    pub fn load(path: &str) -> TournamentConfig {
        let contents = std::fs::read_to_string(path).unwrap_or_else(|error| panic!("failed to read {path}: {error}"));
        let config: TournamentConfig =
            serde_yaml::from_str(&contents).unwrap_or_else(|error| panic!("failed to parse {path}: {error}"));
        assert!(
            config.match_spec.best_of.is_multiple_of(2),
            "match.best_of = {} in {path} must be even, for clean color balance",
            config.match_spec.best_of
        );
        assert!(
            config.match_spec.tiebreak_time_control.is_some(),
            "match.tiebreak_time_control is missing in {path}: a knockout needs every pairing decided"
        );
        assert!(
            config.participants.len() >= 2,
            "participants in {path} must list at least 2 entries, found {}",
            config.participants.len()
        );
        config
    }
}

/// The score a side must reach to win a pairing of `best_of` games outright: strictly more than
/// half (e.g. `6.5` of `12`, `1.5` of `2`) -- derived, not itself configurable, so it always stays a
/// clean majority regardless of `best_of`.
pub fn win_threshold(best_of: u32) -> f64 {
    best_of as f64 / 2.0 + 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scaled_clock_scales_base_and_increment_and_a_depth_stays() {
        let scaled = ControlSpec::time("10+0.1").unwrap().scaled(1.12);
        assert_eq!(scaled.text, "11.2+0.112");
        let Control::Fischer(time_control) = scaled.control else { panic!("a clock stays a clock") };
        assert_eq!((time_control.base.as_secs_f64() * 1000.0).round(), 11_200.0);
        assert_eq!((time_control.increment.as_secs_f64() * 1000.0).round(), 112.0);
        assert_eq!(ControlSpec::time("10+0.1").unwrap().scaled(1.0).text, "10+0.1");
        assert_eq!(ControlSpec::depth("2").unwrap().scaled(1.5).text, "depth 2");
    }

    #[test]
    fn win_threshold_is_strictly_more_than_half() {
        assert_eq!(win_threshold(12), 6.5);
        assert_eq!(win_threshold(2), 1.5);
    }

    fn write_fixture(name: &str, contents: &str) -> String {
        let path = std::env::temp_dir().join(format!("temper-chess-evolve-config-test-{}-{name}.yaml", std::process::id()));
        std::fs::write(&path, contents).expect("failed to write the fixture file");
        path.to_str().unwrap().to_string()
    }

    #[test]
    fn the_committed_ladder_example_loads() {
        temper_chess::pipeline::repository_root::enter_for_tests();
        let config = TournamentConfig::load("examples/tournament.yaml");
        assert_eq!(config.match_spec.best_of, 2);
    }

    /// A gate configured the old way must not quietly run as a best-of-12 match.
    #[test]
    #[should_panic(expected = "unknown field `decision`")]
    fn a_tournament_still_configured_as_a_gate_is_refused() {
        let path = write_fixture(
            "old-gate",
            "name: g\nformat: single-elimination\nparticipants: [a, b]\nmatch:\n  base_time_control: depth 9\n  \
             tiebreak_time_control: depth 9\n  decision: sprt\n",
        );
        TournamentConfig::load(&path);
    }

    #[test]
    #[should_panic(expected = "tiebreak_time_control is missing")]
    fn a_tournament_without_a_tiebreak_is_refused() {
        let path = write_fixture(
            "no-tiebreak",
            "name: t\nformat: single-elimination\nparticipants: [a, b]\nmatch:\n  base_time_control: 3+2\n",
        );
        TournamentConfig::load(&path);
    }

    #[test]
    fn a_control_is_a_depth_from_1_to_9_or_a_fischer_clock() {
        assert_eq!(ControlSpec::depth("1").unwrap().text, "depth 1");
        assert_eq!(ControlSpec::depth("9").unwrap().text, "depth 9");
        assert!(ControlSpec::depth("0").is_err());
        assert!(ControlSpec::depth("10").is_err());
        assert!(ControlSpec::depth("x").is_err());
        assert_eq!(ControlSpec::time("180+2").unwrap().text, "180+2");
        assert_eq!(ControlSpec::time("10+0.1").unwrap().text, "10+0.1");
        assert_eq!(ControlSpec::time("180+2.5").unwrap().text, "180+2.5", "a decimal increment");
        for text in ["3", "3+", "+2", "10s+0.1", "x+2"] {
            assert!(ControlSpec::time(text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn a_rule_names_itself_by_its_value() {
        assert_eq!(Rule::best_of(12).unwrap().spec_text(), "best-of 12");
        assert_eq!(Rule::sprt(35.0).unwrap().spec_text(), "sprt elo_d 35");
        assert_eq!(Rule::Duel.spec_text(), "duel");
        assert!(Rule::best_of(7).is_err() && Rule::best_of(0).is_err());
    }

    #[test]
    fn validate_accepts_a_positive_margin_only() {
        assert!(SprtSpec { elo_d: 35.0 }.validate().is_ok());
        assert!(SprtSpec { elo_d: 0.0 }.validate().is_err());
        assert!(SprtSpec { elo_d: -5.0 }.validate().is_err());
        assert!(SprtSpec { elo_d: f64::NAN }.validate().is_err(), "NaN is not positive");
    }
}
