//! What a network player's name means, for both binaries: `arena` plays it, `smithy install` installs
//! it, and neither may read a name differently from the other.
//!
//! - `<config>-g<g>-l<lambda>-r<n>` and `<config>-g<g>-r<n>` (`NetworkName`), of run `<n>` of
//!   `<config>` (`run_name`: `<config>-r<n>`) in `data/chess/networks/<config>/run-<n>/`: that lambda's
//!   candidate in generation `g`, or the generation's winner as its `generations.tsv` records it.
//!
//! This machine's copy of a run first, then the remote's (`remote::remote_path`): a run's closed
//! generations move there, and every machine reads them where they are.

use std::path::Path;

use super::preprocessing_config::ArmLambda;
use super::{generation, preprocessing_config, remote};
use crate::nnue::{self, Tuning};
use crate::search::SearchParameters;

/// Where pesto's tune writes its `parameters.txt`, and where its in-process player reads it: pesto
/// has no network file to sit beside.
pub const PESTO_TUNING_DIR: &str = "data/chess/spsa/pesto";

/// Pesto's tuned parameters (`PESTO_TUNING_DIR`), this machine's when it has them, else the remote's,
/// where a finished tune moves; `None` if never tuned.
pub fn pesto_parameters() -> Result<Option<SearchParameters>, String> {
    let local = format!("{PESTO_TUNING_DIR}/parameters.txt");
    if Path::new(&local).is_file() {
        return SearchParameters::read_file(&local);
    }
    SearchParameters::read_file(&remote::remote_path(&local))
}

/// A Kalo Chess player as a tune sees it: the network it evaluates with (`None` for pesto), where
/// its tuned parameters are read and written, and the folder its tune keeps its files in.
#[derive(Debug, Clone, PartialEq)]
pub struct KaloChessPlayer {
    pub network: Option<String>,
    pub parameters_path: String,
    pub tune_dir: String,
}

impl KaloChessPlayer {
    /// `name`, `pesto` or a network player, from this machine's files alone
    /// (`resolve_network_file`).
    pub fn resolve(name: &str) -> Result<KaloChessPlayer, String> {
        KaloChessPlayer::resolve_under(name, &|path| path.to_string())
    }

    /// `resolve`, every local path placed by `local`, which maps a path under `data/` to where this
    /// process keeps it. The remote is the mirrored tree's, as always.
    pub fn resolve_under(name: &str, local: &dyn Fn(&str) -> String) -> Result<KaloChessPlayer, String> {
        if name == super::protocol::PESTO {
            return Ok(KaloChessPlayer {
                network: None,
                parameters_path: local(&format!("{PESTO_TUNING_DIR}/parameters.txt")),
                tune_dir: local(PESTO_TUNING_DIR),
            });
        }
        let network = resolve_network_file_under(name, local)?;
        let parsed = parse_network_name(name)?;
        // Only a generation's winner is tuned (the developer, 2026-10-24), in its generation's
        // `tune/`. A candidate has no tune of its own: the `parameters.txt` beside it exists only when
        // it is the winner, and is then the winner's, so it points at the winner's folder too.
        let tune_dir = local(&format!("{}/{}/tune", run_dir(&parsed.run())?, generation_dir_name(parsed.generation)));
        Ok(KaloChessPlayer { parameters_path: nnue::parameters_file_beside(&network), network: Some(network), tune_dir })
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

/// Where the smithy's forges keep their runs, one directory per config holding one per run, relative
/// to the repository root every binary runs from.
pub const RUNS_ROOT: &str = "data/chess/networks";

/// A forge run's name: `<config>-r<n>` for the config's n-th run, the first included, so that a run's
/// name never reads as its config's.
pub fn run_name(config: &str, number: u32) -> String {
    format!("{config}-r{number}")
}

/// `run_name` read back: the config and the run's number. A config never ends in `-r<digits>`
/// (`is_run_suffixed`), so the reading is never ambiguous. A bare config is refused: it names no run,
/// as it did for run 1 before every run had its number.
pub fn parse_run(run: &str) -> Result<(&str, u32), String> {
    // Spelled exactly as `run_name` writes it: digits only, no leading zero, from 1.
    if let Some((config, digits)) = run.rsplit_once("-r") {
        let canonical = !config.is_empty() && !digits.starts_with('0') && digits.bytes().all(|b| b.is_ascii_digit());
        if let Some(number) = digits.parse::<u32>().ok().filter(|&number| canonical && number >= 1) {
            return Ok((config, number));
        }
    }
    Err(format!("{run:?} is no forge run's name: a run is <config>-r<n>, the first {run}-r1"))
}

/// Whether `config` ends in `-r<digits>`, which a run's name would read as a run number: such a
/// config name is refused.
pub fn is_run_suffixed(config: &str) -> bool {
    config.rsplit_once("-r").is_some_and(|(head, digits)| !head.is_empty() && !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

/// A generation's directory in its run's, `gen-<g>`, unpadded as `run-<n>` is.
pub fn generation_dir_name(generation: u32) -> String {
    format!("gen-{generation}")
}

/// A config's directory, holding its runs.
pub fn config_dir(config: &str) -> String {
    format!("{RUNS_ROOT}/{config}")
}

/// A forge run's directory, from its name (`run_name`): `data/chess/networks/<config>/run-<n>`.
pub fn run_dir(run: &str) -> Result<String, String> {
    let (config, number) = parse_run(run)?;
    Ok(format!("{}/run-{number}", config_dir(config)))
}

/// The numbers of `config`'s runs on disk, ascending.
pub fn runs_of(config: &str) -> Vec<u32> {
    runs_in(&config_dir(config))
}

/// The numbers of the runs in `config_dir`, a config's directory wherever it is, ascending.
pub fn runs_in(config_dir: &str) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir(config_dir) else {
        return Vec::new();
    };
    let mut numbers: Vec<u32> =
        entries.filter_map(|entry| entry.ok()?.file_name().into_string().ok()?.strip_prefix("run-")?.parse().ok()).collect();
    numbers.sort_unstable();
    numbers
}

/// A forge run's network, as its name says: the config, the run's number, the generation, and for one
/// lambda's candidate its lambda. A generation's winner has none: a generation has at most one, so
/// `g<g>` alone names it.
#[derive(Debug, Clone, PartialEq)]
pub struct NetworkName {
    pub config: String,
    pub run_number: u32,
    pub generation: u32,
    /// The candidate's lambda; `None` for the generation's winner.
    pub candidate: Option<ArmLambda>,
}

impl NetworkName {
    /// Its run's name (`run_name`).
    pub fn run(&self) -> String {
        run_name(&self.config, self.run_number)
    }

    /// Its name: `<config>-g<g>-l<lambda>-r<n>` for a candidate, `<config>-g<g>-r<n>` for a winner,
    /// each part marked by its letter and the run last. The lambda is spelled as its directory
    /// spells it, with two decimals (`preprocessing_config::lambda_text`).
    pub fn text(&self) -> String {
        let (config, run, generation) = (&self.config, self.run_number, self.generation);
        match self.candidate {
            Some(lambda) => format!("{config}-g{generation}-l{}-r{run}", preprocessing_config::lambda_text(lambda)),
            None => format!("{config}-g{generation}-r{run}"),
        }
    }
}

/// The name of run `run`'s network (`NetworkName::text`) in generation `generation`: its winner when
/// `candidate` is `None`, else its candidate at that lambda. `run` is a run's name, as `run_name`
/// writes it; anything else is a bug, and panics.
pub fn network_player_name(run: &str, generation: u32, candidate: Option<ArmLambda>) -> String {
    let (config, run_number) = parse_run(run).unwrap_or_else(|message| panic!("{message}"));
    NetworkName { config: config.to_string(), run_number, generation, candidate }.text()
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

/// `NetworkName::text` read back, by its letters from the right, not by counting hyphens: `r<n>`
/// last, then either `l<lambda>` and `g<g>`, or `g<g>` alone; the config is what is left. Any other
/// spelling, the old `<run>-NNNN[-<lambda>]` and `-g<g>-a<a>-l<lambda>-` among them, is refused.
pub fn parse_network_name(name: &str) -> Result<NetworkName, String> {
    let not_a_player = || {
        format!(
            "{name:?} is not a player: pesto, stockfish-<level>, stockfish-elo-<rating>, an engine of \
             data/chess/engines.yaml, <config>-g<g>-r<n> (generation g's winner in run n) or \
             <config>-g<g>-l<lambda>-r<n> (that lambda's candidate in generation g)"
        )
    };
    let parse = || -> Option<NetworkName> {
        let (rest, run) = name.rsplit_once('-')?;
        let run_number = numbered(run, 'r')?;
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
        // `betty-l0.70-g2-r1`, the lambda out of place, is refused, not read as config `betty-l0.70`.
        let ends_as_a_part =
            rest.rsplit_once('-').is_some_and(|(_, tail)| lambda_part(tail).is_some() || numbered(tail, 'g').is_some());
        if rest.is_empty() || is_run_suffixed(rest) || ends_as_a_part {
            return None;
        }
        Some(NetworkName { config: rest.to_string(), run_number, generation, candidate })
    };
    parse().ok_or_else(not_a_player)
}

/// The engine file the network player `name` plays with, a forge run's: this machine's copy of the run
/// when it holds the file, else the remote's, where a closed generation moves.
pub fn resolve_network_file(name: &str) -> Result<String, String> {
    resolve_network_file_under(name, &|path| path.to_string())
}

/// `resolve_network_file`, this machine's runs placed by `local` (`KaloChessPlayer::resolve_under`).
fn resolve_network_file_under(name: &str, local: &dyn Fn(&str) -> String) -> Result<String, String> {
    // A name that is no network's is said once, not looked for twice.
    parse_network_name(name)?;
    match resolve_network_file_in(name, &|run| Ok(local(&run_dir(run)?))) {
        Ok(path) => Ok(path),
        Err(local) => resolve_network_file_in(name, &|run| Ok(remote::remote_path(&run_dir(run)?)))
            .map_err(|on_remote| format!("{local}; nor on the remote: {on_remote}")),
    }
}

/// `resolve_network_file`, with where a run's directory is, so the tests can use temporary ones; an
/// error from `run_dir` for a name that is no run's.
pub fn resolve_network_file_in(name: &str, run_dir: &dyn Fn(&str) -> Result<String, String>) -> Result<String, String> {
    let parsed = parse_network_name(name)?;
    let run = parsed.run();
    let dir = run_dir(&run).map_err(|message| format!("{name:?}: {message}"))?;
    if !Path::new(&dir).is_dir() {
        return Err(format!("{name:?}: {run} is no forge run on this machine, {dir}"));
    }
    run_network_path(&dir, parsed.generation, parsed.candidate).map_err(|message| format!("{name:?}: {message}"))
}

/// The engine file a server run's player plays with, in `run_dir`:
///
/// - a candidate named, its lambda: that lambda's candidate in generation `generation_number`;
/// - none: the generation's winner, as `generations.tsv` records it (`run_winner`).
pub fn run_network_path(run_dir: &str, generation_number: u32, candidate: Option<ArmLambda>) -> Result<String, String> {
    let generation_dir = format!("{run_dir}/{}", generation_dir_name(generation_number));
    let lambda = match candidate {
        Some(lambda) => lambda,
        None => run_winner(run_dir, generation_number)?,
    };
    let path = generation::candidate_network_path(&generation::lambda_dir(&generation_dir, lambda));
    if !Path::new(&path).exists() {
        return Err(format!("{path} does not exist"));
    }
    Ok(path)
}

/// Generation `generation_number`'s winner in `run_dir`'s `generations.tsv`, its lambda: the row for
/// it whose `passed` is `true`. A failed gate's winner is never the generation's. A constant, or the
/// polyline (`ArmLambda::parse`).
pub fn run_winner(run_dir: &str, generation_number: u32) -> Result<ArmLambda, String> {
    let path = format!("{run_dir}/generations.tsv");
    let text = std::fs::read_to_string(&path).map_err(|error| format!("{path}: {error}"))?;
    let rows: Vec<&str> = text.lines().skip(1).collect();
    for line in rows.into_iter().rev() {
        let fields: Vec<&str> = line.split('\t').collect();
        let &[generation, winner, _, _, passed] = fields.as_slice() else {
            return Err(format!("{path}: malformed line {line:?} (a run from before attempts were removed is migrated by hand)"));
        };
        if generation.parse::<u32>() == Ok(generation_number) && passed == "true" {
            return ArmLambda::parse(winner).map_err(|_| format!("{path}: malformed line {line:?}"));
        }
    }
    Err(format!(
        "generation {generation_number} of {run_dir} has no winner yet: its round robin or its gate has not finished"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_name_reads_back_and_a_bare_config_names_no_run() {
        assert_eq!(run_name("gen-psq", 1), "gen-psq-r1");
        assert_eq!(run_name("gen-psq", 2), "gen-psq-r2");
        assert_eq!(parse_run("gen-psq-r1"), Ok(("gen-psq", 1)));
        assert_eq!(parse_run("gen-psq-r2"), Ok(("gen-psq", 2)));
        assert_eq!(parse_run("gen-psq-r12"), Ok(("gen-psq", 12)));
        for odd in ["gen-psq", "gen-psq-r0", "gen-psq-r02", "gen-psq-r", "gen-psq-rx", "-r2"] {
            assert!(parse_run(odd).is_err(), "{odd:?} names no run");
        }
        assert_eq!(run_dir("gen-psq-r1"), Ok("data/chess/networks/gen-psq/run-1".to_string()));
        assert_eq!(run_dir("gen-psq-r3"), Ok("data/chess/networks/gen-psq/run-3".to_string()));
        assert!(run_dir("gen-psq").is_err(), "run 1's old, bare name");
        assert!(is_run_suffixed("gen-psq-r2") && is_run_suffixed("x-r1"));
        assert!(!is_run_suffixed("gen-psq") && !is_run_suffixed("gen-psq-rx") && !is_run_suffixed("lc0-psq-direct"));
    }

    fn named(config: &str, run_number: u32, generation: u32, candidate: Option<f64>) -> NetworkName {
        NetworkName { config: config.to_string(), run_number, generation, candidate: candidate.map(ArmLambda::Constant) }
    }

    #[test]
    fn a_network_name_reads_back() {
        let constant = |lambda: f64| Some(ArmLambda::Constant(lambda));
        assert_eq!(network_player_name("betty-r1", 2, constant(0.75)), "betty-g2-l0.75-r1");
        assert_eq!(network_player_name("betty-r1", 2, constant(0.7)), "betty-g2-l0.70-r1");
        assert_eq!(network_player_name("betty-r12", 2, None), "betty-g2-r12");
        assert_eq!(network_player_name("betty-r1", 2, Some(ArmLambda::Polyline)), "betty-g2-lpolyline-r1");
        assert_eq!(parse_network_name("betty-g2-l0.70-r1"), Ok(named("betty", 1, 2, Some(0.7))));
        assert_eq!(parse_network_name("betty-g2-lpolyline-r1").unwrap().candidate, Some(ArmLambda::Polyline));
        assert_eq!(parse_network_name("betty-g2-r12"), Ok(named("betty", 12, 2, None)));
        assert_eq!(parse_network_name("betty-g2-r12").unwrap().run(), "betty-r12");
        // A config named by its labels and architecture, hyphens and all, as the experiments are.
        assert_eq!(parse_network_name("lc0-psq-threats-g1-l0.03-r1"), Ok(named("lc0-psq-threats", 1, 1, Some(0.03))));
        assert_eq!(parse_network_name("lc0-psq-threats-g10-r2"), Ok(named("lc0-psq-threats", 2, 10, None)));
        for name in ["lc0-psq-threats-g1-l0.03-r1", "lc0-psq-threats-g10-r2", "pesto-psq-g3-l1.00-r1", "pesto-psq-g3-lpolyline-r1"] {
            assert_eq!(parse_network_name(name).unwrap().text(), name, "one spelling, read and written");
        }
        for name in [
            // The old forms.
            "betty-0002",
            "betty-0002-0.70",
            "lc0-psq-r1-0001",
            "lc0-psq-r1-0001-0.03",
            // A part missing, misspelled or out of place.
            "betty",
            "betty-g2",
            "betty-r1",
            "-g2-r1",
            "betty-g0-r1",
            "betty-g02-r1",
            "betty-g2-r0",
            "betty-g2-r01",
            "betty-g2-l0.7-r1",
            "betty-g2-l1.50-r1",
            "betty-l0.70-g2-r1",
            "betty-g2-r1-l0.70",
            // The attempt, gone from the names (2026-11-02).
            "betty-g2-a1-l0.70-r1",
            "betty-g2-a1-r1",
            // A config that reads as a run.
            "betty-r1-g2-r1",
        ] {
            assert!(parse_network_name(name).is_err(), "{name:?}");
        }
        assert!(resolve_network_file("gen-psq-0001").unwrap_err().contains("is not a player"));
    }

    /// A fresh temporary root named after `test`, with its network and run directory functions.
    fn root(test: &str) -> String {
        let root = std::env::temp_dir().join(format!("kalo-chess-players-{test}-{}", std::process::id()));
        let root = root.to_str().unwrap().to_string();
        std::fs::remove_dir_all(&root).ok();
        root
    }

    fn touch(path: &str) {
        std::fs::create_dir_all(Path::new(path).parent().unwrap()).unwrap();
        std::fs::write(path, b"").unwrap();
    }

    #[test]
    fn a_server_runs_player_is_a_lambda_of_its_generation_or_the_generations_winner() {
        let root = root("run");
        let runs = |run: &str| -> Result<String, String> { Ok(format!("{root}/runs/{run}")) };
        let candidate = |generation: u32, lambda: &str| format!("{root}/runs/fine-r1/gen-{generation}/lambda-{lambda}/candidate.nnue");
        for (generation, lambda) in [(1, "0.04"), (1, "0.06"), (2, "0.05")] {
            touch(&candidate(generation, lambda));
        }
        let header = "generation\twinner\tgate_games\tgate_share\tpassed\n";
        let log = format!("{root}/runs/fine-r1/generations.tsv");
        let resolve = |name: &str| resolve_network_file_in(name, &runs);

        assert!(resolve("other-g1-r1").unwrap_err().contains("no forge run"));
        std::fs::write(&log, header).unwrap();
        assert_eq!(resolve("fine-g1-l0.06-r1"), Ok(candidate(1, "0.06")));
        assert_eq!(resolve("fine-g2-l0.05-r1"), Ok(candidate(2, "0.05")));
        assert!(resolve("fine-g2-l0.04-r1").unwrap_err().contains("does not exist"), "not in generation 2");
        assert!(resolve("fine-g1-r1").unwrap_err().contains("no winner yet"));

        // Generation 1 passed its gate, generation 2 failed its: only the first has a winner.
        std::fs::write(&log, format!("{header}1\t0.06\t300\t0.5600\ttrue\n2\t0.05\t200\t0.4800\tfalse\n")).unwrap();
        assert_eq!(resolve("fine-g1-r1"), Ok(candidate(1, "0.06")));
        assert!(resolve("fine-g2-r1").unwrap_err().contains("no winner yet"));

        // A log from before attempts were removed, its six columns: said, not misread.
        std::fs::write(&log, "generation\tattempt\twinner\tgate_games\tgate_share\tpassed\n1\t1\t0.06\t300\t0.5600\ttrue\n").unwrap();
        assert!(resolve("fine-g1-r1").unwrap_err().contains("migrated by hand"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn only_the_winner_has_a_tune_folder() {
        let root = root("tune");
        let local = |path: &str| format!("{root}/{path}");
        let generation_dir = local("data/chess/networks/fine/run-1/gen-1");
        touch(&format!("{generation_dir}/lambda-0.03/candidate.nnue"));
        std::fs::write(
            local("data/chess/networks/fine/run-1/generations.tsv"),
            "generation\twinner\tgate_games\tgate_share\tpassed\n1\t0.03\t300\t0.5600\ttrue\n",
        )
        .unwrap();
        let tune_dir = |name: &str| KaloChessPlayer::resolve_under(name, &local).unwrap().tune_dir;
        assert_eq!(tune_dir("fine-g1-r1"), format!("{generation_dir}/tune"), "the winner's, its generation's");
        assert_eq!(tune_dir("fine-g1-l0.03-r1"), format!("{generation_dir}/tune"), "a candidate has none of its own");
        std::fs::remove_dir_all(&root).ok();
    }
}
