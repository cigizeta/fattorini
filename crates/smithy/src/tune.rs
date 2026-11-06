//! A tune, the smithy's job of a player, as its config's `tune:` says (`args_of`): a Kalo Chess player's ten search parameters by SPSA
//! (`kalo_chess::pipeline::spsa`), every pair played by the arenas. Pairs of games, colours
//! reversed on one random book opening, between theta + c_k Delta and theta - c_k Delta, one `k` per
//! pair; theta moves as each pair arrives, in the order they arrive, each with its own `k`, as
//! fishtest's workers do. Every game ends early by the tablebase, win and draw rules: a tune's chores
//! say so (`Chore::adjudicate`).
//!
//! The run's folder (`KaloChessPlayer::tune_dir`) holds:
//!
//! - `spec.txt`: what the run is (player, pairs, `r_end`, seed, control, the ten specs), written at the
//!   first start; a later start that disagrees is refused rather than resumed.
//! - `state.tsv`: the pairs whose result is in theta, and theta, rewritten after every pair, so a stop
//!   loses only the pairs in flight. A resume starts its new pairs from that count, so their `k`s,
//!   and Deltas, are drawn again.
//! - `pairs.tsv`: one row appended per pair, its `k`, result, the machine that played it, Delta and
//!   theta after: the history.
//! - `pairs.pgn`: every pair's games, for inspection only; nothing reads it back.
//!
//! The folder is this machine's while the tune runs, its files that change copied whole to the remote's
//! mirror of it now and then (`mirror`); done, it moves there whole (`hand_over`), beside the tuned
//! network's `parameters.txt`. A folder here without its finished twin on the remote is a tune in
//! progress; one found only on the remote is fetched back to resume or to say it is done.

use std::io::Write;
use std::path::Path;

use kalo_chess::pipeline::players::KaloChessPlayer;
use kalo_chess::pipeline::remote;

use super::{archive, lease, scale, tree};
use kalo_chess::pipeline::protocol::{Entry, Limit, Player};
use kalo_chess::pipeline::spsa::{self, Schedule, Spec, Theta, MAX_CLOCK_FACTOR, MIN_CLOCK_FACTOR};
use kalo_chess::search::SearchParameters;

/// A tune's control when none is given: arena's short time control.
pub(crate) const STC: &str = "10+0.1";

/// The fewest pairs `--pairs` takes: SPSA's schedule is set from the run's length (`spsa::Schedule`),
/// and a run shorter than this moves too little to mean anything.
const MIN_PAIRS: u32 = 100;

/// The tag a tune's pair carries its `k` in.
pub(crate) const K_TAG: &str = "K";

/// What a tune is, in the words its config is read into: the player, the control, and a new tune's
/// pairs, every tune starting from the default parameters.
pub(crate) struct TuneArgs {
    pub(crate) player: String,
    pub(crate) control: String,
    /// `--pairs <n>`: a new tune's; none for a resume.
    pub(crate) pairs: Option<u32>,
    /// A scaled run's network's `s`: its tune's pairs and control scaled as its run, for debugging
    /// the tune alone; none for every other player.
    pub(crate) scale: Option<u64>,
}

impl TuneArgs {
    /// `<player> [--pairs <n>] [--depth <d> | --time <base+inc>]`.
    pub(crate) fn parse(args: &[String]) -> Result<TuneArgs, String> {
        let usage = "a tune takes a Kalo Chess player, then --pairs <n> for a new tune, and optionally --depth <d> or --time <base+inc>";
        let (player, mut rest) = args.split_first().ok_or(usage)?;
        if player.starts_with("--") || player.contains('@') {
            return Err(format!("{usage}; got {player:?}"));
        }
        let (mut pairs, mut control) = (None, STC.to_string());
        while let Some((flag, tail)) = rest.split_first() {
            let value = || tail.first().filter(|value| !value.starts_with("--")).cloned().ok_or(format!("{flag} takes a value"));
            let (taken, consumed) = match flag.as_str() {
                "--pairs" => (Some(parse_pairs(&value()?)?), 2),
                "--depth" => {
                    let depth = value()?;
                    depth.parse::<u32>().ok().filter(|&depth| (1..=9).contains(&depth)).ok_or(format!("--depth takes 1 to 9, got {depth:?}"))?;
                    control = format!("depth {depth}");
                    (None, 2)
                }
                "--time" => {
                    control = value()?;
                    (None, 2)
                }
                other => return Err(format!("{usage}; {other:?} is none of them")),
            };
            if taken.is_some() {
                if pairs.is_some() {
                    return Err("give --pairs <n> once: it sets the tune's length".to_string());
                }
                pairs = taken;
            }
            rest = &tail[consumed - 1..];
        }
        // A clock the chores can carry: `base+inc` in seconds.
        if !control.starts_with("depth ") {
            Limit::parse(&format!("clock {control}"))?;
            let (base, increment) = control.split_once('+').ok_or(format!("--time takes <base+inc>, got {control:?}"))?;
            if base.parse::<f64>().is_err() || increment.parse::<f64>().is_err() {
                return Err(format!("--time takes <base+inc> in seconds, got {control:?}"));
            }
        }
        Ok(TuneArgs { player: player.clone(), control, pairs, scale: None })
    }
}

/// What a tune is, for any player it tunes: a config's `tune:` section, a forge config's for its
/// runs' generations, `configs/chess/pesto.yaml`'s for pesto (`pesto.rs`), as
///
/// ```yaml
/// tune:
///   length: default          # spsa::DEFAULT_TUNE_PAIRS, or a number of pairs: 4096
///   control: 10+0.1          # or: {depth: 9}; 10+0.1 when left out
/// ```
///
/// Every tune starts from the default parameters, whatever the player (the developer, 2026-10-23).
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TuneConfig {
    length: ConfigLength,
    #[serde(default)]
    control: Option<ConfigControl>,
}

/// Untagged, as `ConfigControl`: a number of pairs, or the one word `default`.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(untagged)]
enum ConfigLength {
    Pairs(u32),
    Named(String),
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(untagged)]
enum ConfigControl {
    Clock(String),
    Depth { depth: u32 },
}

/// The tune `config` describes, of `player`: the config read into the words a tune takes, so it is
/// checked exactly as they are (`TuneArgs::parse`).
pub(crate) fn args_of(config: &TuneConfig, player: &str) -> Result<TuneArgs, String> {
    let pairs = match &config.length {
        ConfigLength::Pairs(pairs) => *pairs,
        ConfigLength::Named(name) if name == "default" => spsa::DEFAULT_TUNE_PAIRS,
        ConfigLength::Named(name) => return Err(format!("tune: length: {name:?} is neither default nor a number of pairs")),
    };
    let mut words = vec![player.to_string(), "--pairs".to_string(), pairs.to_string()];
    match &config.control {
        Some(ConfigControl::Clock(clock)) => words.extend(["--time".to_string(), clock.clone()]),
        Some(ConfigControl::Depth { depth }) => words.extend(["--depth".to_string(), depth.to_string()]),
        None => {}
    }
    TuneArgs::parse(&words).map_err(|message| format!("tune: {message}"))
}

/// The player `name` names, from this machine's files.
fn resolve(name: &str) -> Result<KaloChessPlayer, String> {
    KaloChessPlayer::resolve(name)
}

/// The remote counterpart of `local`, a local path under `data/`.
fn remote_of(local: &str) -> String {
    remote::remote_path(local)
}

/// `player`'s tune's lease on the remote (`lease.rs`).
pub(crate) fn lease_path(player: &str) -> Result<String, String> {
    Ok(format!("{}/{}", remote_of(&resolve(player)?.tune_dir), lease::FILE))
}

/// The pairs whose result the tune in `dir` holds, 0 when it holds none.
fn pairs_done(dir: &str) -> u32 {
    read_state(&format!("{dir}/state.tsv"), spsa::start_theta(&spsa::SPECS)).map_or(0, |(k_done, _)| k_done)
}

/// A new tune's `pairs`, scaled down for a scaled network as a round robin's are, never below
/// `MIN_PAIRS`, which SPSA's schedule needs.
fn scaled_pairs(pairs: u32, scale: Option<u64>) -> u32 {
    scale.map_or(pairs, |s| scale::games(pairs, s, MIN_PAIRS))
}

/// `player`'s tune as its files stand, for the page: its pairs done and its pairs in all; `None`
/// before it starts, or when its files cannot be read.
pub(crate) fn progress(player: &str) -> Option<(u32, u32)> {
    let local = resolve(player).ok()?.tune_dir;
    // Here while it runs, on the remote once done.
    let dir = if Path::new(&format!("{local}/spec.txt")).is_file() { local.clone() } else { remote_of(&local) };
    let (pairs, specs) = run_in_spec(&std::fs::read_to_string(format!("{dir}/spec.txt")).ok()?)?;
    let (k_done, _) = read_state(&format!("{dir}/state.tsv"), spsa::start_theta(&specs)).ok()?;
    Some((k_done, pairs))
}

/// `--pairs`' value: a whole number of at least `MIN_PAIRS`.
fn parse_pairs(value: &str) -> Result<u32, String> {
    value.parse().ok().filter(|&pairs| pairs >= MIN_PAIRS).ok_or_else(|| {
        format!("a tune's length is a whole number of at least {MIN_PAIRS} pairs, got {value:?}, or default")
    })
}

/// A tune under way: its settings, its point, and its files.
pub(crate) struct Tune {
    pub(crate) player: String,
    /// The tuned player's network file, `None` for pesto.
    pub(crate) network: Option<String>,
    pub(crate) pairs: u32,
    r_end: f64,
    seed: u64,
    /// As `spec.txt` spells it: `depth 9` or `10+0.1`.
    control: String,
    specs: [Spec; 10],
    schedule: Schedule,
    dir: String,
    parameters_path: String,
    pub(crate) theta: Theta,
    /// Pairs whose result is in theta.
    pub(crate) k_done: u32,
    /// The next `k` never handed out.
    next_k: u32,
    /// `k`s handed out and lost with a client, handed out again before `next_k`, smallest first: else
    /// `k_done` would never reach the run's pairs. A replayed `k` plays the same Delta.
    returned: Vec<u32>,
    /// The `k`s applied this run: a `k` handed out twice counts once.
    applied: std::collections::HashSet<u32>,
}

impl Tune {
    /// Opens `args`' tune in its folder, resuming what is there.
    pub(crate) fn open(args: &TuneArgs) -> Result<Tune, String> {
        let player = resolve(&args.player)?;
        let spec_path = format!("{}/spec.txt", player.tune_dir);
        // A tune moved to the remote, done, or its copy when this machine lost its own or another
        // smithy took it further: fetched back, so a done one says so and an unfinished one resumes
        // where it got to. The lease is the remote's alone.
        let moved = remote_of(&player.tune_dir);
        let remote_ahead = !Path::new(&spec_path).is_file() || pairs_done(&moved) > pairs_done(&player.tune_dir);
        if remote_ahead && Path::new(&format!("{moved}/spec.txt")).is_file() {
            tree::copy(&moved, &player.tune_dir, &|relative| relative != lease::FILE, true)?;
        }
        // A resumed tune's pairs are its spec's, scaled already when its network is a scaled run's.
        let (pairs, specs) = match args.pairs {
            Some(pairs) => (scaled_pairs(pairs, args.scale), spsa::SPECS),
            None => std::fs::read_to_string(&spec_path).ok().and_then(|text| run_in_spec(&text)).ok_or_else(|| {
                format!("{spec_path} records no tune yet: a new tune needs its length, --pairs <n>")
            })?,
        };
        let mut tune = Tune {
            player: args.player.clone(),
            network: player.network.clone(),
            pairs,
            r_end: spsa::R_END,
            seed: spsa::seed_of(&args.player),
            // Scaled at every open, a resume's included: its spec records the scaled control.
            control: args.scale.map_or(args.control.clone(), |s| scale::control(&args.control, s)),
            specs,
            schedule: Schedule::new(pairs, spsa::R_END, &specs),
            dir: player.tune_dir.clone(),
            parameters_path: player.parameters_path.clone(),
            theta: spsa::start_theta(&specs),
            k_done: 0,
            next_k: 0,
            returned: Vec::new(),
            applied: std::collections::HashSet::new(),
        };
        std::fs::create_dir_all(&tune.dir).map_err(|error| format!("{}: {error}", tune.dir))?;
        check_spec(&spec_path, &tune.spec_text())?;
        (tune.k_done, tune.theta) = read_state(&tune.state_path(), tune.theta)?;
        tune.next_k = tune.k_done;
        check_pairs_header(&tune.pairs_path())?;
        Ok(tune)
    }

    fn state_path(&self) -> String {
        format!("{}/state.tsv", self.dir)
    }

    fn pairs_path(&self) -> String {
        format!("{}/pairs.tsv", self.dir)
    }

    pub(crate) fn games_path(&self) -> String {
        format!("{}/pairs.pgn", self.dir)
    }

    pub(crate) fn dir(&self) -> &str {
        &self.dir
    }

    /// The chores' limit.
    pub(crate) fn limit(&self) -> Limit {
        match self.control.strip_prefix("depth ") {
            Some(depth) => Limit::Depth(depth.parse().expect("TuneArgs checked the depth")),
            None => Limit::Clock(self.control.clone()),
        }
    }

    /// `spec.txt`'s text: one `name<TAB>value` line per setting, then one line per parameter with its
    /// start, range and `c_end`.
    fn spec_text(&self) -> String {
        let mut text = format!("player\t{}\npairs\t{}\nr_end\t{}\nseed\t{}\ncontrol\t{}\n", self.player, self.pairs, self.r_end, self.seed, self.control);
        for (name, spec) in SearchParameters::NAMES.iter().zip(self.specs.iter()) {
            text.push_str(&format!("{name}\t{}\t{}\t{}\t{}\n", spec.start, spec.min, spec.max, spec.c_end));
        }
        text
    }

    pub(crate) fn done(&self) -> bool {
        self.k_done >= self.pairs
    }

    /// Whether a `k` can be handed out now.
    pub(crate) fn has_k(&self) -> bool {
        !self.done() && (!self.returned.is_empty() || self.next_k < self.pairs)
    }

    /// The next `k` to play: the smallest returned one first, then a new one; `None` when every `k`
    /// is handed out.
    pub(crate) fn take_k(&mut self) -> Option<u32> {
        if self.done() {
            return None;
        }
        if let Some(at) = self.returned.iter().enumerate().min_by_key(|&(_, &k)| k).map(|(at, _)| at) {
            return Some(self.returned.swap_remove(at));
        }
        if self.next_k < self.pairs {
            self.next_k += 1;
            return Some(self.next_k - 1);
        }
        None
    }

    /// Gives `ks`, handed out and never applied, back to be played again.
    pub(crate) fn return_ks(&mut self, ks: &[u32]) {
        self.returned.extend(ks.iter().filter(|k| !self.applied.contains(k)));
    }

    /// Pair `k`'s entry: theta + c_k Delta against theta - c_k Delta at theta as it is now, explicit
    /// parameters both, `network` the tuned player's hash, its `k` in a tag.
    pub(crate) fn entry(&self, k: u32, network: &str) -> Entry {
        let delta = spsa::draw_delta(self.seed, k);
        let side = |sign: i32| Player {
            network: network.to_string(),
            parameters: Some(spsa::point(&self.theta, &self.schedule, &self.specs, k, &delta, sign)),
        };
        Entry { players: vec![side(1), side(-1)], count: 1, tags: vec![(K_TAG.to_string(), k.to_string())] }
    }

    /// Applies pair `k`, result `r` from the plus side, played on `source`, and records it:
    /// `state.tsv` rewritten, one `pairs.tsv` row. A `k` already applied is skipped.
    pub(crate) fn record(&mut self, k: u32, r: i32, source: &str) -> Result<(), String> {
        if self.done() || !self.applied.insert(k) {
            return Ok(());
        }
        let delta = spsa::draw_delta(self.seed, k);
        self.theta = spsa::update(&self.theta, &self.schedule, &self.specs, k, &delta, r);
        self.k_done += 1;
        write_atomically(&self.state_path(), &state_text(self.k_done, &self.theta))?;
        let signs: String = delta.iter().map(|&sign| if sign > 0 { '+' } else { '-' }).collect();
        let values: Vec<String> = self.theta.iter().map(|value| format!("{value:.3}")).collect();
        append(&self.pairs_path(), &format!("{k}\t{r}\t{source}\t{signs}\t{}\n", values.join("\t")))
    }

    /// Writes the result, theta rounded, to the player's `parameters.txt`: in the tune's folder first,
    /// then, when the network's is elsewhere (on the remote, its generation moved there), copied there
    /// whole.
    pub(crate) fn finish(&self) -> Result<SearchParameters, String> {
        let tuned = spsa::rounded(&self.theta);
        let here = format!("{}/parameters.txt", self.dir);
        tuned.write_file(&here)?;
        if here != self.parameters_path {
            remote::copy_replacing(&here, &self.parameters_path).map_err(|error| error.to_string())?;
        }
        Ok(tuned)
    }

    /// The tune's files that change along it (`spec.txt`, `state.tsv`, `pairs.tsv`, `chores.txt`), each
    /// copied whole to the remote's mirror of its folder, replacing the last copy.
    pub(crate) fn mirror(&self) -> Result<(), String> {
        for name in ["spec.txt", "state.tsv", "pairs.tsv", "chores.txt"] {
            let local = format!("{}/{name}", self.dir);
            if Path::new(&local).is_file() {
                remote::copy_replacing(&local, &remote_of(&local)).map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }

    /// A finished tune's folder moved whole to the remote (`archive::move_dir`), its twin there.
    pub(crate) fn hand_over(&self) -> Result<(), String> {
        archive::move_dir(&self.dir, &remote_of(&self.dir)).map(|_| ())
    }

    pub(crate) fn parameters_path(&self) -> &str {
        &self.parameters_path
    }

    /// Each parameter's name, start and theta now.
    pub(crate) fn rows(&self) -> Vec<(&'static str, i32, f64)> {
        SearchParameters::NAMES.iter().zip(self.specs.iter()).zip(self.theta.iter()).map(|((&name, spec), &now)| (name, spec.start, now)).collect()
    }

    /// Whether `tag`, a game's `TimeControl`, is the tune's clock scaled within the clamps a client
    /// may scale it by. A fixed depth needs none.
    pub(crate) fn check_clock(&self, tag: Option<&str>) -> Result<(), String> {
        let Limit::Clock(clock) = self.limit() else {
            return Ok(());
        };
        let parse = |text: &str| -> Option<(f64, f64)> {
            let (base, increment) = text.split_once('+')?;
            Some((base.parse().ok()?, increment.parse().ok()?))
        };
        let (base, increment) = parse(&clock).expect("TuneArgs checked the clock");
        let Some((played_base, played_increment)) = tag.and_then(parse) else {
            return Err(format!("a tune's game without a TimeControl tag of the form base+inc, got {tag:?}"));
        };
        let within = |played: f64, given: f64| -> bool {
            given == 0.0 && played == 0.0
                || (MIN_CLOCK_FACTOR * given * (1.0 - 1e-3)..=MAX_CLOCK_FACTOR * given * (1.0 + 1e-3)).contains(&played)
        };
        if !within(played_base, base) || !within(played_increment, increment) {
            return Err(format!("a tune's game at {played_base}+{played_increment}, outside the clamps of {clock}"));
        }
        Ok(())
    }
}

/// The pairs and specs a `spec.txt`'s `text` records.
fn run_in_spec(text: &str) -> Option<(u32, [Spec; 10])> {
    let pairs = text.lines().find_map(|line| line.strip_prefix("pairs\t")).and_then(|value| value.parse().ok())?;
    let mut specs = spsa::SPECS;
    for (spec, name) in specs.iter_mut().zip(SearchParameters::NAMES.iter()) {
        let line = text.lines().find_map(|line| line.strip_prefix(&format!("{name}\t")))?;
        let values: Vec<&str> = line.split('\t').collect();
        let [start, min, max, c_end] = values.as_slice() else { return None };
        *spec = Spec { start: start.parse().ok()?, min: min.parse().ok()?, max: max.parse().ok()?, c_end: c_end.parse().ok()? };
    }
    Some((pairs, specs))
}

/// Writes `spec.txt` at a first start, or checks that an existing one says the same.
fn check_spec(path: &str, text: &str) -> Result<(), String> {
    match std::fs::read_to_string(path) {
        Ok(existing) if existing == text => return Ok(()),
        Ok(existing) => {
            // Both in full in the smithy's log; the error, which the page shows, in one line.
            log::warn!("{path} describes another tune than the one asked for:\n{existing}\nthe one asked for:\n{text}");
            let summary = |text: &str| -> String {
                let value = |key: &str| {
                    let field = text.lines().find_map(|line| line.split_once('\t').filter(|(name, _)| *name == key).map(|(_, value)| value));
                    field.unwrap_or("?").replace('\t', " ")
                };
                format!("{} pairs at {}", value("pairs"), value("control"))
            };
            return Err(format!(
                "{} holds another tune, {}, than the one asked for, {}: resume it, or move the folder away to start afresh",
                std::path::Path::new(path).parent().map_or(path.to_string(), |dir| dir.display().to_string()),
                summary(&existing),
                summary(text)
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("{path}: {error}")),
    }
    write_atomically(path, text)
}

fn write_atomically(path: &str, text: &str) -> Result<(), String> {
    let tmp_path = format!("{path}.tmp");
    std::fs::write(&tmp_path, text).map_err(|error| format!("{tmp_path}: {error}"))?;
    std::fs::rename(&tmp_path, path).map_err(|error| format!("{path}: {error}"))
}

/// `state.tsv`'s text: `k_done`, then theta, one `name<TAB>value` line each, the values exact (Rust's
/// shortest representation reads back to the same `f64`).
fn state_text(k_done: u32, theta: &Theta) -> String {
    let mut text = format!("k_done\t{k_done}\n");
    for (name, value) in SearchParameters::NAMES.iter().zip(theta.iter()) {
        text.push_str(&format!("{name}\t{value}\n"));
    }
    text
}

/// `state.tsv` at `path`: `k_done` and theta, or 0 and `start` when there is none.
fn read_state(path: &str, start: Theta) -> Result<(u32, Theta), String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((0, start)),
        Err(error) => return Err(format!("{path}: {error}")),
    };
    let malformed = || format!("{path}: expected k_done and the ten parameters, one name<TAB>value line each");
    let mut lines = text.lines();
    let k_done: u32 = match lines.next().and_then(|line| line.split_once('\t')) {
        Some(("k_done", value)) => value.parse().map_err(|_| malformed())?,
        _ => return Err(malformed()),
    };
    let mut theta = start;
    for (i, name) in SearchParameters::NAMES.iter().enumerate() {
        match lines.next().and_then(|line| line.split_once('\t')) {
            Some((found, value)) if found == *name => theta[i] = value.parse().map_err(|_| malformed())?,
            _ => return Err(malformed()),
        }
    }
    if lines.next().is_some() {
        return Err(malformed());
    }
    Ok((k_done, theta))
}

/// `pairs.tsv`'s header: the pair's `k`, its result from the plus side, the machine that played it,
/// its Delta as signs, theta after.
fn pairs_header() -> String {
    format!("k\tr\tsource\tdelta\t{}\n", SearchParameters::NAMES.join("\t"))
}

/// Starts `pairs.tsv` with its header, or checks that an existing one has today's.
fn check_pairs_header(path: &str) -> Result<(), String> {
    match std::fs::read_to_string(path) {
        Ok(text) if text.lines().next() == Some(pairs_header().trim_end()) => Ok(()),
        Ok(_) => Err(format!("{path} has another header than today's ({}): move it away to resume", pairs_header().trim_end())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => append(path, &pairs_header()),
        Err(error) => Err(format!("{path}: {error}")),
    }
}

fn append(path: &str, text: &str) -> Result<(), String> {
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).map_err(|error| format!("{path}: {error}"))?;
    file.write_all(text.as_bytes()).map_err(|error| format!("{path}: {error}"))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn a_scaled_network_s_tune_plays_fewer_pairs_never_below_the_schedule_s_least() {
        assert_eq!(scaled_pairs(16_384, None), 16_384, "a full run's network");
        let scaled = scaled_pairs(16_384, Some(1024));
        assert!((MIN_PAIRS..16_384).contains(&scaled), "{scaled}");
        assert_eq!(scaled_pairs(200, Some(1024)), MIN_PAIRS);
    }

    fn args(text: &str) -> Result<TuneArgs, String> {
        TuneArgs::parse(&text.split_whitespace().map(str::to_string).collect::<Vec<_>>())
    }

    /// The tune a `tune:` section's `text` describes, of `player`, the first of its lineage.
    fn config_args_from(text: &str, player: &str) -> Result<TuneArgs, String> {
        let config: TuneConfig = serde_yaml::from_str(text).map_err(|error| error.to_string())?;
        args_of(&config, player)
    }

    #[test]
    fn a_tune_section_reads_as_the_words_it_stands_for() {
        let default = config_args_from("length: default\n", "pesto").unwrap();
        assert_eq!((default.player.as_str(), default.control.as_str()), ("pesto", STC));
        assert_eq!(default.pairs, Some(spsa::DEFAULT_TUNE_PAIRS));

        let pairs = config_args_from("length: 400\ncontrol: {depth: 3}\n", "x-0001").unwrap();
        assert_eq!(pairs.pairs, Some(400));
        assert_eq!(pairs.control, "depth 3");
        assert_eq!(config_args_from("length: 4096\ncontrol: 60+0.6\n", "x-0001").unwrap().control, "60+0.6");

        assert!(config_args_from("length: 10\n", "x-0001").is_err(), "too few pairs");
        assert!(config_args_from("length: default\nplayer: x\n", "x-0001").is_err(), "an unknown key");
        assert!(config_args_from("control: 10+0.1\n", "x-0001").is_err(), "no length");
        for gone in ["first", "chained", "{pairs: 400}", "{fine: pesto}"] {
            assert!(config_args_from(&format!("length: {gone}\n"), "x-0001").is_err(), "{gone}: no longer a length");
        }
    }

    /// A tune of pesto in `dir`, `pairs` pairs at depth 2, as `open` would make it.
    pub(crate) fn tune_in(dir: &str, pairs: u32) -> Tune {
        std::fs::create_dir_all(dir).unwrap();
        let specs = spsa::SPECS;
        let mut tune = Tune {
            player: "pesto".to_string(),
            network: None,
            pairs,
            r_end: spsa::R_END,
            seed: spsa::seed_of("pesto"),
            control: "depth 2".to_string(),
            specs,
            schedule: Schedule::new(pairs, spsa::R_END, &specs),
            dir: dir.to_string(),
            parameters_path: format!("{dir}/parameters.txt"),
            theta: spsa::start_theta(&specs),
            k_done: 0,
            next_k: 0,
            returned: Vec::new(),
            applied: std::collections::HashSet::new(),
        };
        check_spec(&format!("{dir}/spec.txt"), &tune.spec_text()).unwrap();
        (tune.k_done, tune.theta) = read_state(&tune.state_path(), tune.theta).unwrap();
        tune.next_k = tune.k_done;
        check_pairs_header(&tune.pairs_path()).unwrap();
        tune
    }

    fn temp_dir(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("smithy-tune-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        dir.to_str().unwrap().to_string()
    }

    #[test]
    fn the_arguments_take_one_length_and_a_control() {
        let tune = args("pesto --pairs 400").unwrap();
        assert_eq!(tune.pairs, Some(400));
        assert_eq!(tune.control, STC);
        assert_eq!(args("amanda-0001 --pairs 400 --depth 4").unwrap().control, "depth 4");
        assert_eq!(args("pesto --pairs 400 --time 2+0.02").unwrap().control, "2+0.02");
        assert!(args("pesto").unwrap().pairs.is_none(), "a resume");
        for bad in [
            "", "--pairs 400", "pesto --pairs 400 --pairs 500", "pesto --pairs 99", "pesto --depth 10", "pesto --time fast",
            "pesto@150 --pairs 400", "pesto --first", "pesto --fine amanda-0001",
        ] {
            assert!(args(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn ks_come_in_order_lost_ones_first_and_each_counts_once() {
        let dir = temp_dir("ks");
        let mut tune = tune_in(&dir, 4);
        assert_eq!((tune.take_k(), tune.take_k(), tune.take_k()), (Some(0), Some(1), Some(2)));
        tune.record(1, 2, "alpha").unwrap();
        tune.return_ks(&[2, 1, 0]);
        assert_eq!((tune.take_k(), tune.take_k(), tune.take_k()), (Some(0), Some(2), Some(3)), "1 is applied, so never again");
        assert_eq!(tune.take_k(), None);
        tune.record(1, -2, "alpha").unwrap();
        assert_eq!(tune.k_done, 1, "a k applied twice counts once");
        for k in [0, 2, 3] {
            tune.record(k, 0, "beta").unwrap();
        }
        assert!(tune.done());
        assert_eq!(std::fs::read_to_string(tune.pairs_path()).unwrap().lines().count(), 1 + 4);
        let again = tune_in(&dir, 4);
        assert_eq!((again.k_done, again.theta), (4, tune.theta), "the state reads back");
        assert_eq!(SearchParameters::read_file(tune.parameters_path()).unwrap(), None);
        let tuned = tune.finish().unwrap();
        assert_eq!(SearchParameters::read_file(tune.parameters_path()).unwrap(), Some(tuned));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_entry_plays_both_sides_of_its_k_and_names_it() {
        let dir = temp_dir("entry");
        let tune = tune_in(&dir, 100);
        let entry = tune.entry(7, "pesto");
        assert_eq!(entry.count, 1);
        assert_eq!(entry.tags, vec![(K_TAG.to_string(), "7".to_string())]);
        assert_ne!(entry.players[0].parameters, entry.players[1].parameters);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_spec_that_disagrees_is_refused() {
        let dir = temp_dir("spec");
        let tune = tune_in(&dir, 4);
        assert!(check_spec(&format!("{dir}/spec.txt"), &tune.spec_text()).is_ok());
        let refused = check_spec(&format!("{dir}/spec.txt"), &tune.spec_text().replace("pairs\t4", "pairs\t5")).unwrap_err();
        assert!(refused.contains("4 pairs") && refused.contains("5 pairs") && !refused.contains('\n'), "{refused}");
        assert_eq!(run_in_spec(&tune.spec_text()), Some((4, spsa::SPECS)));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_game_s_clock_is_the_tune_s_within_the_clamps() {
        let dir = temp_dir("clock");
        let mut tune = tune_in(&dir, 4);
        assert!(tune.check_clock(None).is_ok(), "a depth needs none");
        tune.control = "10+0.1".to_string();
        assert!(tune.check_clock(Some("10+0.1")).is_ok());
        assert!(tune.check_clock(Some("21.3+0.213")).is_ok());
        assert!(tune.check_clock(Some("50+0.5")).is_err(), "past the clamp");
        assert!(tune.check_clock(None).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
