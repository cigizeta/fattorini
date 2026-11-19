//! What a network player's name means, for both binaries: `arena` plays it and takes its snapshot,
//! the smithy trains and tunes it, and neither may read a name differently from the other.
//!
//! - `<config>-g<g>-l<lambda>-a<n>` and `<config>-g<g>-a<n>` (`NetworkName`), of attempt `<n>` of
//!   `<config>` (`attempt_name`: `<config>-a<n>`) in `data/networks/<config>/attempt-<n>/`: that
//!   lambda's candidate in generation `g`, or the generation's winner as its `generations.tsv` records
//!   it.
//!
//! A training run is an attempt (2026-11-19): names spelled `-r<n>` and directories `run-<n>/` from
//! before are refused: every `data/` was migrated that day.
//!
//! This machine's copy of an attempt first, then the remote's (`remote::remote_path`): an attempt's
//! closed generations move there, and every machine reads them where they are.

use std::path::Path;

use super::preprocessing_config::ArmLambda;
use super::{generation, preprocessing_config, remote};
use crate::nnue::{self, Tuning, SEARCH_PARAMETERS_FILE, TIME_PARAMETERS_FILE};
use crate::search::{SearchParameters, TimeParameters};
use super::spsa::TuneKind;

/// Where pesto's tunes write their parameter files, and where its in-process player reads them:
/// pesto has no network file to sit beside.
pub const PESTO_TUNING_DIR: &str = "data/spsa/pesto";

/// What a Fattorini player's name says it plays with, after its `@`, each naming what is tuned (the
/// developer, 2026-11-13): a later kind of tune adds a word rather than making a name wrong. `@tuned`,
/// the one tuned suffix before the time tune, was renamed `@search-tuned` once, with no alias.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuningSuffix {
    /// `@untuned`: the defaults of the search and the clock, whatever its files say.
    Untuned,
    /// `@search-tuned`: its `search-parameters.txt`, and the default clock even when a
    /// `time-parameters.txt` exists, so the time tune's verdict duel can play the two clocks.
    SearchTuned,
    /// `@search-time-tuned`: its `search-parameters.txt` and its `time-parameters.txt`.
    SearchTimeTuned,
}

impl TuningSuffix {
    /// Every suffix, as written, for messages.
    pub const WORDS: &'static str = "untuned, search-tuned or search-time-tuned";

    /// As written after the `@`.
    pub fn text(self) -> &'static str {
        match self {
            TuningSuffix::Untuned => "untuned",
            TuningSuffix::SearchTuned => "search-tuned",
            TuningSuffix::SearchTimeTuned => "search-time-tuned",
        }
    }

    /// `text` read back, `None` for any other word.
    pub fn parse(text: &str) -> Option<TuningSuffix> {
        [TuningSuffix::Untuned, TuningSuffix::SearchTuned, TuningSuffix::SearchTimeTuned].into_iter().find(|suffix| suffix.text() == text)
    }

    /// Whether it plays a search tune.
    pub fn search_tuned(self) -> bool {
        self != TuningSuffix::Untuned
    }

    /// Whether it plays a time tune.
    pub fn time_tuned(self) -> bool {
        self == TuningSuffix::SearchTimeTuned
    }
}

/// The folder each kind of tune keeps its files in, named as the suffixes (`@search-tuned`,
/// `@search-time-tuned`): in a generation's folder, or pesto's `PESTO_TUNING_DIR`.
pub fn tune_dir_name(kind: TuneKind) -> &'static str {
    match kind {
        TuneKind::Search => "search-tune",
        TuneKind::Time => "time-tune",
    }
}

/// The file of pesto's named `file`, this machine's when it has it, else the remote's, where a
/// finished tune moves.
fn pesto_file(file: &str) -> String {
    let local = format!("{PESTO_TUNING_DIR}/{file}");
    if Path::new(&local).is_file() { local } else { remote::remote_path(&local) }
}

/// Pesto's tuned search parameters (`PESTO_TUNING_DIR`); `None` if never tuned.
pub fn pesto_parameters() -> Result<Option<SearchParameters>, String> {
    SearchParameters::read_file(&pesto_file(SEARCH_PARAMETERS_FILE))
}

/// Pesto's tuned time parameters (`PESTO_TUNING_DIR`); `None` if never tuned.
pub fn pesto_time_parameters() -> Result<Option<TimeParameters>, String> {
    TimeParameters::read_file(&pesto_file(TIME_PARAMETERS_FILE))
}

/// A Fattorini player as a tune sees it: the network it evaluates with (`None` for pesto), where
/// each kind of tuned parameters is read and written, and the folder each kind of tune keeps its
/// files in.
#[derive(Debug, Clone, PartialEq)]
pub struct FattoriniPlayer {
    pub network: Option<String>,
    pub search_parameters_path: String,
    pub time_parameters_path: String,
    pub search_tune_dir: String,
    pub time_tune_dir: String,
}

impl FattoriniPlayer {
    /// `name`, `pesto` or a network player, from this machine's files alone
    /// (`resolve_network_file`).
    pub fn resolve(name: &str) -> Result<FattoriniPlayer, String> {
        FattoriniPlayer::resolve_under(name, &|path| path.to_string())
    }

    /// `resolve`, every local path placed by `local`, which maps a path under `data/` to where this
    /// process keeps it. The remote is the mirrored tree's, as always.
    pub fn resolve_under(name: &str, local: &dyn Fn(&str) -> String) -> Result<FattoriniPlayer, String> {
        if name == super::protocol::PESTO {
            return Ok(FattoriniPlayer {
                network: None,
                search_parameters_path: local(&format!("{PESTO_TUNING_DIR}/{SEARCH_PARAMETERS_FILE}")),
                time_parameters_path: local(&format!("{PESTO_TUNING_DIR}/{TIME_PARAMETERS_FILE}")),
                search_tune_dir: local(&format!("{PESTO_TUNING_DIR}/{}", tune_dir_name(TuneKind::Search))),
                time_tune_dir: local(&format!("{PESTO_TUNING_DIR}/{}", tune_dir_name(TuneKind::Time))),
            });
        }
        let network = resolve_network_file_under(name, local)?;
        let parsed = parse_network_name(name)?;
        // Only a generation's winner is tuned (the developer, 2026-10-24), in its generation's
        // folder. A candidate has no tune of its own: the parameter files beside it exist only when
        // it is the winner, and are then the winner's, so it points at the winner's folders too.
        let generation = format!("{}/{}", attempt_dir(&parsed.attempt())?, generation_dir_name(parsed.generation));
        Ok(FattoriniPlayer {
            search_parameters_path: nnue::search_parameters_file_beside(&network),
            time_parameters_path: nnue::time_parameters_file_beside(&network),
            network: Some(network),
            search_tune_dir: local(&format!("{generation}/{}", tune_dir_name(TuneKind::Search))),
            time_tune_dir: local(&format!("{generation}/{}", tune_dir_name(TuneKind::Time))),
        })
    }

    /// Where a tune of `kind` writes its result, and the folder it keeps its files in.
    pub fn tune_paths(&self, kind: TuneKind) -> (&str, &str) {
        match kind {
            TuneKind::Search => (&self.search_parameters_path, &self.search_tune_dir),
            TuneKind::Time => (&self.time_parameters_path, &self.time_tune_dir),
        }
    }

    /// Its tuned time parameters, `None` if it has none: a network's file beside it, pesto's in
    /// `PESTO_TUNING_DIR`.
    pub fn own_time_tuning(&self) -> Result<Option<TimeParameters>, String> {
        match &self.network {
            Some(network) => nnue::read_time_tuning_beside(network),
            None => pesto_time_parameters(),
        }
    }

    /// The parameters it plays with when its name has no suffix: a network's file beside it,
    /// pesto's in `PESTO_TUNING_DIR`.
    pub fn own_tuning(&self) -> Result<Tuning, String> {
        match &self.network {
            Some(network) => nnue::read_tuning_beside(network),
            None => Ok(pesto_parameters()?.map_or(Tuning::Untuned, Tuning::Parameters)),
        }
    }
}

/// Where the smithy's forges keep their attempts, one directory per config holding one per attempt,
/// relative to the repository root every binary runs from.
pub const ATTEMPTS_ROOT: &str = "data/networks";

/// An attempt's directory in its config's, `attempt-<n>`; the prefix alone for reading them back.
const ATTEMPT_DIR_PREFIX: &str = "attempt-";

/// A forge attempt's name: `<config>-a<n>` for the config's n-th attempt, the first included, so that
/// an attempt's name never reads as its config's.
pub fn attempt_name(config: &str, number: u32) -> String {
    format!("{config}-a{number}")
}

/// `attempt_name` read back: the config and the attempt's number. A config never ends in
/// `-a<digits>` (`is_attempt_suffixed`), so the reading is never ambiguous. A bare config is refused:
/// it names no attempt.
pub fn parse_attempt(attempt: &str) -> Result<(&str, u32), String> {
    // Spelled exactly as `attempt_name` writes it: digits only, no leading zero, from 1.
    if let Some((config, digits)) = attempt.rsplit_once("-a") {
        let canonical = !config.is_empty() && !digits.starts_with('0') && digits.bytes().all(|b| b.is_ascii_digit());
        if let Some(number) = digits.parse::<u32>().ok().filter(|&number| canonical && number >= 1) {
            return Ok((config, number));
        }
    }
    Err(format!("{attempt:?} is no forge attempt's name: an attempt is <config>-a<n>, the first {attempt}-a1"))
}

/// Whether `config` ends in `-a<digits>`, which an attempt's name would read as an attempt number:
/// such a config name is refused.
pub fn is_attempt_suffixed(config: &str) -> bool {
    config.rsplit_once("-a").is_some_and(|(head, digits)| !head.is_empty() && !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

/// A generation's directory in its attempt's, `gen-<g>`, unpadded as `attempt-<n>` is.
pub fn generation_dir_name(generation: u32) -> String {
    format!("gen-{generation}")
}

/// A config's directory, holding its attempts.
pub fn config_dir(config: &str) -> String {
    format!("{ATTEMPTS_ROOT}/{config}")
}

/// A forge attempt's directory, from its name (`attempt_name`): `data/networks/<config>/attempt-<n>`.
pub fn attempt_dir(attempt: &str) -> Result<String, String> {
    let (config, number) = parse_attempt(attempt)?;
    Ok(format!("{}/{ATTEMPT_DIR_PREFIX}{number}", config_dir(config)))
}

/// The numbers of `config`'s attempts on disk, ascending.
pub fn attempts_of(config: &str) -> Vec<u32> {
    attempts_in(&config_dir(config))
}

/// The numbers of the attempts in `config_dir`, a config's directory wherever it is, ascending.
pub fn attempts_in(config_dir: &str) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir(config_dir) else {
        return Vec::new();
    };
    let mut numbers: Vec<u32> = entries
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok()?.strip_prefix(ATTEMPT_DIR_PREFIX)?.parse().ok())
        .collect();
    numbers.sort_unstable();
    numbers
}

/// A forge attempt's network, as its name says: the config, the attempt's number, the generation, and
/// for one lambda's candidate its lambda. A generation's winner has none: a generation has at most
/// one, so `g<g>` alone names it.
#[derive(Debug, Clone, PartialEq)]
pub struct NetworkName {
    pub config: String,
    pub attempt_number: u32,
    pub generation: u32,
    /// The candidate's lambda; `None` for the generation's winner.
    pub candidate: Option<ArmLambda>,
}

impl NetworkName {
    /// Its attempt's name (`attempt_name`).
    pub fn attempt(&self) -> String {
        attempt_name(&self.config, self.attempt_number)
    }

    /// Its name: `<config>-g<g>-l<lambda>-a<n>` for a candidate, `<config>-g<g>-a<n>` for a winner,
    /// each part marked by its letter and the attempt last. The lambda is spelled as its directory
    /// spells it, with two decimals (`preprocessing_config::lambda_text`).
    pub fn text(&self) -> String {
        let (config, attempt, generation) = (&self.config, self.attempt_number, self.generation);
        match self.candidate {
            Some(lambda) => format!("{config}-g{generation}-l{}-a{attempt}", preprocessing_config::lambda_text(lambda)),
            None => format!("{config}-g{generation}-a{attempt}"),
        }
    }
}

/// The name of attempt `attempt`'s network (`NetworkName::text`) in generation `generation`: its
/// winner when `candidate` is `None`, else its candidate at that lambda. `attempt` is an attempt's
/// name, as `attempt_name` writes it; anything else is a bug, and panics.
pub fn network_player_name(attempt: &str, generation: u32, candidate: Option<ArmLambda>) -> String {
    let (config, attempt_number) = parse_attempt(attempt).unwrap_or_else(|message| panic!("{message}"));
    NetworkName { config: config.to_string(), attempt_number, generation, candidate }.text()
}

/// `part` as `<letter><n>`, `n` from 1 spelled with no leading zero, so a name has one spelling.
fn numbered(part: &str, letter: char) -> Option<u32> {
    let digits = part.strip_prefix(letter)?;
    if digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok().filter(|&n| n >= 1)
}

/// `part` as `l<lambda>`: a constant in `[0, 1]`, spelled with two decimals as every record spells it,
/// so `l0.7` is refused for `l0.70`; or `lpolyline`.
fn lambda_part(part: &str) -> Option<ArmLambda> {
    let text = part.strip_prefix('l')?;
    let lambda = ArmLambda::parse(text).ok()?;
    let in_range = lambda.constant().is_none_or(|constant| (0.0..=1.0).contains(&constant));
    (in_range && preprocessing_config::lambda_text(lambda) == text).then_some(lambda)
}

/// `NetworkName::text` read back, by its letters from the right, not by counting hyphens: `a<n>`
/// last, then either `l<lambda>` and `g<g>`, or `g<g>` alone; the config is what is left. Any other
/// spelling, the old `-r<n>` before attempts and `<run>-NNNN[-<lambda>]` among them, is refused.
pub fn parse_network_name(name: &str) -> Result<NetworkName, String> {
    let not_a_player = || {
        format!(
            "{name:?} is not a player: pesto, stockfish-<level>, stockfish-elo-<rating>, an engine of \
             data/engines.yaml, <config>-g<g>-a<n> (generation g's winner in attempt n) or \
             <config>-g<g>-l<lambda>-a<n> (that lambda's candidate in generation g); a name in -r<n> \
             is from before attempts (2026-11-19), when attempts were called runs"
        )
    };
    let parse = || -> Option<NetworkName> {
        let (rest, attempt) = name.rsplit_once('-')?;
        let attempt_number = numbered(attempt, 'a')?;
        let (rest, last) = rest.rsplit_once('-')?;
        let (rest, generation_part, candidate) = match lambda_part(last) {
            Some(lambda) => {
                let (rest, generation_part) = rest.rsplit_once('-')?;
                (rest, generation_part, Some(lambda))
            }
            None => (rest, last, None),
        };
        let generation = numbered(generation_part, 'g')?;
        // A config whose last part reads as a name's own part would make a name read two ways:
        // `betty-l0.70-g2-a1`, the lambda out of place, is refused, not read as config `betty-l0.70`.
        let ends_as_a_part =
            rest.rsplit_once('-').is_some_and(|(_, tail)| lambda_part(tail).is_some() || numbered(tail, 'g').is_some());
        if rest.is_empty() || is_attempt_suffixed(rest) || ends_as_a_part {
            return None;
        }
        Some(NetworkName { config: rest.to_string(), attempt_number, generation, candidate })
    };
    parse().ok_or_else(not_a_player)
}

/// The engine file the network player `name` plays with, a forge attempt's: this machine's copy of the
/// attempt when it holds the file, else the remote's, where a closed generation moves.
pub fn resolve_network_file(name: &str) -> Result<String, String> {
    resolve_network_file_under(name, &|path| path.to_string())
}

/// `resolve_network_file`, this machine's attempts placed by `local` (`FattoriniPlayer::resolve_under`).
fn resolve_network_file_under(name: &str, local: &dyn Fn(&str) -> String) -> Result<String, String> {
    // A name that is no network's is said once, not looked for twice.
    parse_network_name(name)?;
    match resolve_network_file_in(name, &|attempt| Ok(local(&attempt_dir(attempt)?))) {
        Ok(path) => Ok(path),
        Err(local) => resolve_network_file_in(name, &|attempt| Ok(remote::remote_path(&attempt_dir(attempt)?)))
            .map_err(|on_remote| format!("{local}; nor on the remote: {on_remote}")),
    }
}

/// `resolve_network_file`, with where an attempt's directory is, so the tests can use temporary ones;
/// an error from `attempt_dir` for a name that is no attempt's.
pub fn resolve_network_file_in(name: &str, attempt_dir: &dyn Fn(&str) -> Result<String, String>) -> Result<String, String> {
    let parsed = parse_network_name(name)?;
    let attempt = parsed.attempt();
    let dir = attempt_dir(&attempt).map_err(|message| format!("{name:?}: {message}"))?;
    if !Path::new(&dir).is_dir() {
        return Err(format!("{name:?}: {attempt} is no forge attempt on this machine, {dir}"));
    }
    attempt_network_path(&dir, parsed.generation, parsed.candidate).map_err(|message| format!("{name:?}: {message}"))
}

/// The engine file an attempt's player plays with, in `attempt_dir`:
///
/// - a candidate named, its lambda: that lambda's candidate in generation `generation_number`;
/// - none: the generation's winner, as `generations.tsv` records it (`attempt_winner`).
pub fn attempt_network_path(attempt_dir: &str, generation_number: u32, candidate: Option<ArmLambda>) -> Result<String, String> {
    let generation_dir = format!("{attempt_dir}/{}", generation_dir_name(generation_number));
    let lambda = match candidate {
        Some(lambda) => lambda,
        None => attempt_winner(attempt_dir, generation_number)?,
    };
    let path = generation::candidate_network_path(&generation::lambda_dir(&generation_dir, lambda));
    if !Path::new(&path).exists() {
        return Err(format!("{path} does not exist"));
    }
    Ok(path)
}

/// Generation `generation_number`'s winner in `attempt_dir`'s `generations.tsv`, its lambda: the row
/// for it whose `passed` is `true`. A failed gate's winner is never the generation's. A constant, or
/// the polyline (`ArmLambda::parse`).
pub fn attempt_winner(attempt_dir: &str, generation_number: u32) -> Result<ArmLambda, String> {
    let path = format!("{attempt_dir}/generations.tsv");
    let text = std::fs::read_to_string(&path).map_err(|error| format!("{path}: {error}"))?;
    let rows: Vec<&str> = text.lines().skip(1).collect();
    for line in rows.into_iter().rev() {
        let fields: Vec<&str> = line.split('\t').collect();
        let &[generation, winner, _, _, passed] = fields.as_slice() else {
            return Err(format!("{path}: malformed line {line:?} (a log of six columns, from before 2026-11-02, is migrated by hand)"));
        };
        if generation.parse::<u32>() == Ok(generation_number) && passed == "true" {
            return ArmLambda::parse(winner).map_err(|_| format!("{path}: malformed line {line:?}"));
        }
    }
    Err(format!(
        "generation {generation_number} of {attempt_dir} has no winner yet: its round robin or its gate has not finished"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_attempt_name_reads_back_and_a_bare_config_names_no_attempt() {
        assert_eq!(attempt_name("gen-psq", 1), "gen-psq-a1");
        assert_eq!(attempt_name("gen-psq", 2), "gen-psq-a2");
        assert_eq!(parse_attempt("gen-psq-a1"), Ok(("gen-psq", 1)));
        assert_eq!(parse_attempt("gen-psq-a2"), Ok(("gen-psq", 2)));
        assert_eq!(parse_attempt("gen-psq-a12"), Ok(("gen-psq", 12)));
        for odd in ["gen-psq", "gen-psq-a0", "gen-psq-a02", "gen-psq-a", "gen-psq-ax", "-a2", "gen-psq-r1"] {
            assert!(parse_attempt(odd).is_err(), "{odd:?} names no attempt");
        }
        assert_eq!(attempt_dir("gen-psq-a1"), Ok("data/networks/gen-psq/attempt-1".to_string()));
        assert_eq!(attempt_dir("gen-psq-a3"), Ok("data/networks/gen-psq/attempt-3".to_string()));
        assert!(attempt_dir("gen-psq").is_err(), "a bare config");
        assert!(is_attempt_suffixed("gen-psq-a2") && is_attempt_suffixed("x-a1"));
        assert!(!is_attempt_suffixed("gen-psq") && !is_attempt_suffixed("gen-psq-ax") && !is_attempt_suffixed("lc0-psq-alpha"));
    }

    #[test]
    fn attempts_are_read_from_attempt_directories_alone() {
        let root = root("attempts-in");
        for dir in ["attempt-2", "attempt-1", "run-3", "attempt-x"] {
            std::fs::create_dir_all(format!("{root}/{dir}")).unwrap();
        }
        assert_eq!(attempts_in(&root), vec![1, 2], "run-3 is from before attempts, never read");
        std::fs::remove_dir_all(&root).ok();
    }

    fn named(config: &str, attempt_number: u32, generation: u32, candidate: Option<f64>) -> NetworkName {
        NetworkName { config: config.to_string(), attempt_number, generation, candidate: candidate.map(ArmLambda::Constant) }
    }

    #[test]
    fn a_network_name_reads_back() {
        let constant = |lambda: f64| Some(ArmLambda::Constant(lambda));
        assert_eq!(network_player_name("betty-a1", 2, constant(0.75)), "betty-g2-l0.75-a1");
        assert_eq!(network_player_name("betty-a1", 2, constant(0.7)), "betty-g2-l0.70-a1");
        assert_eq!(network_player_name("betty-a12", 2, None), "betty-g2-a12");
        assert_eq!(network_player_name("betty-a1", 2, Some(ArmLambda::Polyline)), "betty-g2-lpolyline-a1");
        assert_eq!(parse_network_name("betty-g2-l0.70-a1"), Ok(named("betty", 1, 2, Some(0.7))));
        assert_eq!(parse_network_name("betty-g2-lpolyline-a1").unwrap().candidate, Some(ArmLambda::Polyline));
        assert_eq!(parse_network_name("betty-g2-a12"), Ok(named("betty", 12, 2, None)));
        assert_eq!(parse_network_name("betty-g2-a12").unwrap().attempt(), "betty-a12");
        // A config named by its labels and architecture, hyphens and all, as the experiments are.
        assert_eq!(parse_network_name("lc0-psq-threats-g1-l0.03-a1"), Ok(named("lc0-psq-threats", 1, 1, Some(0.03))));
        assert_eq!(parse_network_name("lc0-psq-threats-g10-a2"), Ok(named("lc0-psq-threats", 2, 10, None)));
        for name in ["lc0-psq-threats-g1-l0.03-a1", "lc0-psq-threats-g10-a2", "pesto-psq-g3-l1.00-a1", "pesto-psq-g3-lpolyline-a1"] {
            assert_eq!(parse_network_name(name).unwrap().text(), name, "one spelling, read and written");
        }
        for name in [
            // The old forms.
            "betty-0002",
            "betty-0002-0.70",
            "lc0-psq-r1-0001",
            "lc0-psq-r1-0001-0.03",
            // From before attempts (2026-11-19): migrated, never read.
            "betty-g2-r1",
            "betty-g2-l0.70-r1",
            // A part missing, misspelled or out of place.
            "betty",
            "betty-g2",
            "betty-a1",
            "-g2-a1",
            "betty-g0-a1",
            "betty-g02-a1",
            "betty-g2-a0",
            "betty-g2-a01",
            "betty-g2-l0.7-a1",
            "betty-g2-l1.50-a1",
            "betty-l0.70-g2-a1",
            "betty-g2-a1-l0.70",
            // A config that reads as an attempt.
            "betty-a1-g2-a1",
        ] {
            assert!(parse_network_name(name).is_err(), "{name:?}");
        }
        assert!(parse_network_name("betty-g2-r1").unwrap_err().contains("from before attempts"));
        assert!(resolve_network_file("gen-psq-0001").unwrap_err().contains("is not a player"));
    }

    /// A fresh temporary root named after `test`.
    fn root(test: &str) -> String {
        let root = std::env::temp_dir().join(format!("fattorini-players-{test}-{}", std::process::id()));
        let root = root.to_str().unwrap().to_string();
        std::fs::remove_dir_all(&root).ok();
        root
    }

    fn touch(path: &str) {
        std::fs::create_dir_all(Path::new(path).parent().unwrap()).unwrap();
        std::fs::write(path, b"").unwrap();
    }

    #[test]
    fn an_attempts_player_is_a_lambda_of_its_generation_or_the_generations_winner() {
        let root = root("attempt");
        let attempts = |attempt: &str| -> Result<String, String> { Ok(format!("{root}/attempts/{attempt}")) };
        let candidate =
            |generation: u32, lambda: &str| format!("{root}/attempts/fine-a1/gen-{generation}/lambda-{lambda}/candidate.nnue");
        for (generation, lambda) in [(1, "0.04"), (1, "0.06"), (2, "0.05")] {
            touch(&candidate(generation, lambda));
        }
        let header = "generation\twinner\tgate_games\tgate_share\tpassed\n";
        let log = format!("{root}/attempts/fine-a1/generations.tsv");
        let resolve = |name: &str| resolve_network_file_in(name, &attempts);

        assert!(resolve("other-g1-a1").unwrap_err().contains("no forge attempt"));
        std::fs::write(&log, header).unwrap();
        assert_eq!(resolve("fine-g1-l0.06-a1"), Ok(candidate(1, "0.06")));
        assert_eq!(resolve("fine-g2-l0.05-a1"), Ok(candidate(2, "0.05")));
        assert!(resolve("fine-g2-l0.04-a1").unwrap_err().contains("does not exist"), "not in generation 2");
        assert!(resolve("fine-g1-a1").unwrap_err().contains("no winner yet"));

        // Generation 1 passed its gate, generation 2 failed its: only the first has a winner.
        std::fs::write(&log, format!("{header}1\t0.06\t300\t0.5600\ttrue\n2\t0.05\t200\t0.4800\tfalse\n")).unwrap();
        assert_eq!(resolve("fine-g1-a1"), Ok(candidate(1, "0.06")));
        assert!(resolve("fine-g2-a1").unwrap_err().contains("no winner yet"));

        // A log from before 2026-11-02, its six columns: said, not misread.
        std::fs::write(&log, "generation\tattempt\twinner\tgate_games\tgate_share\tpassed\n1\t1\t0.06\t300\t0.5600\ttrue\n").unwrap();
        assert!(resolve("fine-g1-a1").unwrap_err().contains("migrated by hand"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn only_the_winner_has_a_tune_folder() {
        let root = root("tune");
        let local = |path: &str| format!("{root}/{path}");
        let generation_dir = local("data/networks/fine/attempt-1/gen-1");
        touch(&format!("{generation_dir}/lambda-0.03/candidate.nnue"));
        std::fs::write(
            local("data/networks/fine/attempt-1/generations.tsv"),
            "generation\twinner\tgate_games\tgate_share\tpassed\n1\t0.03\t300\t0.5600\ttrue\n",
        )
        .unwrap();
        let player = |name: &str| FattoriniPlayer::resolve_under(name, &local).unwrap();
        assert_eq!(player("fine-g1-a1").search_tune_dir, format!("{generation_dir}/search-tune"), "the winner's, its generation's");
        assert_eq!(player("fine-g1-a1").time_tune_dir, format!("{generation_dir}/time-tune"));
        assert_eq!(player("fine-g1-l0.03-a1").search_tune_dir, format!("{generation_dir}/search-tune"), "a candidate has none of its own");
        assert_eq!(player("fine-g1-a1").tune_paths(TuneKind::Time).1, format!("{generation_dir}/time-tune"));
        std::fs::remove_dir_all(&root).ok();
    }
}
