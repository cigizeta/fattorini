//! A tune, the smithy's job of a player, as its config's `tune:` says (`args_of`): one kind of a
//! Fattorini player's parameters by SPSA (`fattorini::pipeline::spsa`), its ten search parameters
//! or, after them, its 22 time parameters with its tuned search, every pair played by the arenas. Pairs of games, colours
//! reversed on one random book opening, between theta + c_k Delta and theta - c_k Delta, one `k` per
//! pair; theta moves as each pair arrives, in the order they arrive, each with its own `k`, as
//! fishtest's workers do. Every game ends early by the tablebase, win and draw rules: a tune's chores
//! say so (`Chore::adjudicate`).
//!
//! The run's folder (`FattoriniPlayer::tune_paths`: `search-tune/` or `time-tune/`) holds:
//!
//! - `spec.txt`: what the run is (player, a time tune's kind, pairs, `r_end`, seed, control, the
//!   specs), written at the first start; a later start that disagrees is refused rather than
//!   resumed. A search tune's has no kind line, as before the time tune.
//! - `state.tsv`: the pairs whose result is in theta, and theta, rewritten after every pair, so a stop
//!   loses only the pairs in flight. A resume starts its new pairs from that count, so their `k`s,
//!   and Deltas, are drawn again.
//! - `pairs.tsv`: one row appended per pair, its `k`, result, the machine that played it, Delta and
//!   theta after: the history.
//! - `pairs.pgn`: every pair's games, for inspection only; nothing reads it back.
//!
//! The folder is this machine's while the tune runs, its files that change copied whole to the remote's
//! mirror of it now and then (`mirror`); done, it moves there whole (`hand_over`), beside the tuned
//! network's `search-parameters.txt` or `time-parameters.txt`. A folder here without its finished twin on the remote is a tune in
//! progress; one found only on the remote is fetched back to resume or to say it is done.

use std::io::Write;
use std::path::Path;

use fattorini::pipeline::players::FattoriniPlayer;
use fattorini::pipeline::remote;

use super::{archive, lease, scale, tree};
use fattorini::pipeline::protocol::{Entry, Limit, Player};
use fattorini::pipeline::spsa::{self, Schedule, Spec, Theta, TuneKind, MAX_CLOCK_FACTOR, MIN_CLOCK_FACTOR};
use fattorini::search::SearchParameters;

/// A tune's control when none is given: arena's short time control.
pub(crate) const STC: &str = "10+0.1";

/// The fewest pairs `--pairs` takes: SPSA's schedule is set from the run's length (`spsa::Schedule`),
/// and a run shorter than this moves too little to mean anything.
const MIN_PAIRS: u32 = 100;

/// The tag a tune's pair carries its `k` in.
pub(crate) const K_TAG: &str = "K";

/// What a tune is, in the words its config is read into: the player, the control, and a new tune's
/// pairs and starting values.
pub(crate) struct TuneArgs {
    pub(crate) player: String,
    pub(crate) control: String,
    /// `--pairs <n>`: a new tune's; none for a resume.
    pub(crate) pairs: Option<u32>,
    /// What it tunes, the search unless the smithy says otherwise.
    pub(crate) kind: TuneKind,
    /// A new search tune's starting values: its teacher's tune (the smithy's `teacher_tune`), `None`
    /// for the defaults. A resume keeps its `spec.txt`'s, whatever this says. A time tune starts from
    /// the time defaults.
    pub(crate) start: Option<SearchParameters>,
    /// A time tune's search, the player's finished search tune, which both sides play; `None` for a
    /// search tune.
    pub(crate) search: Option<SearchParameters>,
    /// A scaled run's network's `s`: its tune's pairs and control scaled as its run, for debugging
    /// the tune alone; none for every other player.
    pub(crate) scale: Option<u64>,
}

impl TuneArgs {
    /// `<player> [--pairs <n>] [--depth <d> | --time <base+inc>]`.
    pub(crate) fn parse(args: &[String]) -> Result<TuneArgs, String> {
        let usage = "a tune takes a Fattorini player, then --pairs <n> for a new tune, and optionally --depth <d> or --time <base+inc>";
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
        Ok(TuneArgs { player: player.clone(), control, pairs, kind: TuneKind::Search, start: None, search: None, scale: None })
    }
}

/// What a tune is, for any player it tunes: a config's `tune:` section, a forge config's for its
/// runs' generations, `configs/pesto.yaml`'s for pesto (`pesto.rs`), as
///
/// ```yaml
/// tune:
///   length: default          # spsa::DEFAULT_TUNE_PAIRS, or a number of pairs: 4096
///   control: 10+0.1          # or: {depth: 9}; 10+0.1 when left out
/// ```
///
/// A tune starts from its teacher's tune, or the defaults without one (`TuneArgs::start`).
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
fn resolve(name: &str) -> Result<FattoriniPlayer, String> {
    FattoriniPlayer::resolve(name)
}

/// The remote counterpart of `local`, a local path under `data/`.
fn remote_of(local: &str) -> String {
    remote::remote_path(local)
}

/// `player`'s tune of `kind`'s folder, here.
fn tune_dir(player: &str, kind: TuneKind) -> Result<String, String> {
    Ok(resolve(player)?.tune_paths(kind).1.to_string())
}

/// `player`'s tune of `kind`'s lease on the remote (`lease.rs`).
pub(crate) fn lease_path(player: &str, kind: TuneKind) -> Result<String, String> {
    Ok(format!("{}/{}", remote_of(&tune_dir(player, kind)?), lease::FILE))
}

/// The pairs whose result the tune of `kind` in `dir` holds, 0 when it holds none.
fn pairs_done(dir: &str, kind: TuneKind) -> u32 {
    read_state(&format!("{dir}/state.tsv"), kind.names(), spsa::start_theta(kind.specs())).map_or(0, |(k_done, _)| k_done)
}

/// A new tune's `pairs`, scaled down for a scaled network as a round robin's are, never below
/// `MIN_PAIRS`, which SPSA's schedule needs.
fn scaled_pairs(pairs: u32, scale: Option<u64>) -> u32 {
    scale.map_or(pairs, |s| scale::games(pairs, s, MIN_PAIRS))
}

/// `player`'s tune of `kind` as its files stand, for the page: its pairs done and its pairs in all;
/// `None` before it starts, or when its files cannot be read.
pub(crate) fn progress(player: &str, kind: TuneKind) -> Option<(u32, u32)> {
    let local = tune_dir(player, kind).ok()?;
    // Here while it runs, on the remote once done.
    let dir = if Path::new(&format!("{local}/spec.txt")).is_file() { local.clone() } else { remote_of(&local) };
    let (_, pairs, specs) = run_in_spec(&std::fs::read_to_string(format!("{dir}/spec.txt")).ok()?)?;
    let (k_done, _) = read_state(&format!("{dir}/state.tsv"), kind.names(), spsa::start_theta(&specs)).ok()?;
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
    pub(crate) kind: TuneKind,
    /// A time tune's search, played by both sides; `None` for a search tune.
    search: Option<SearchParameters>,
    pub(crate) pairs: u32,
    r_end: f64,
    seed: u64,
    /// As `spec.txt` spells it: `depth 9` or `10+0.1`.
    control: String,
    /// Under a clock, the speed every arena's clock is scaled to, benched once at the first start and
    /// kept in `spec.txt`, so a restart never moves the clock (2026-11-13: three restarts gave
    /// 1.26M, 1.39M and 1.02M); `None` until benched.
    pub(crate) reference_nps: Option<f64>,
    specs: Vec<Spec>,
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
        let kind = args.kind;
        if kind == TuneKind::Time && args.search.is_none() {
            return Err(format!("{}'s time tune needs its search tune, which both sides play", args.player));
        }
        let (parameters_path, dir) = player.tune_paths(kind);
        let (parameters_path, dir) = (parameters_path.to_string(), dir.to_string());
        let spec_path = format!("{dir}/spec.txt");
        // A tune moved to the remote, done, or its copy when this machine lost its own or another
        // smithy took it further: fetched back, so a done one says so and an unfinished one resumes
        // where it got to. The lease is the remote's alone.
        let moved = remote_of(&dir);
        let remote_ahead = !Path::new(&spec_path).is_file() || pairs_done(&moved, kind) > pairs_done(&dir, kind);
        if remote_ahead && Path::new(&format!("{moved}/spec.txt")).is_file() {
            tree::copy(&moved, &dir, &|relative| relative != lease::FILE, true)?;
        }
        // A resumed tune's pairs and starts are its spec's, its pairs scaled already when its network
        // is a scaled run's; a new search tune's start from its teacher's tune, or the defaults; a new
        // time tune's from the time defaults.
        let (pairs, specs) = match args.pairs {
            Some(pairs) => {
                let specs = match (kind, &args.start) {
                    (TuneKind::Search, Some(start)) => spsa::specs_from(&spsa::SEARCH_SPECS, &spsa::search_values(start)),
                    _ => kind.specs().to_vec(),
                };
                (scaled_pairs(pairs, args.scale), specs)
            }
            None => {
                let recorded = std::fs::read_to_string(&spec_path).ok().and_then(|text| run_in_spec(&text));
                let (recorded_kind, pairs, specs) =
                    recorded.ok_or_else(|| format!("{spec_path} records no tune yet: a new tune needs its length, --pairs <n>"))?;
                if recorded_kind != kind {
                    return Err(format!("{spec_path} records a {} tune, not a {} one", recorded_kind.text(), kind.text()));
                }
                (pairs, specs)
            }
        };
        let mut tune = Tune {
            player: args.player.clone(),
            network: player.network.clone(),
            kind,
            search: args.search,
            pairs,
            r_end: spsa::R_END,
            seed: spsa::seed_of(&args.player, kind),
            // Scaled at every open, a resume's included: its spec records the scaled control.
            control: args.scale.map_or(args.control.clone(), |s| scale::control(&args.control, s)),
            reference_nps: std::fs::read_to_string(&spec_path).ok().and_then(|text| reference_in_spec(&text)),
            schedule: Schedule::new(pairs, spsa::R_END, &specs),
            theta: spsa::start_theta(&specs),
            specs,
            dir,
            parameters_path,
            k_done: 0,
            next_k: 0,
            returned: Vec::new(),
            applied: std::collections::HashSet::new(),
        };
        std::fs::create_dir_all(&tune.dir).map_err(|error| format!("{}: {error}", tune.dir))?;
        check_spec(&spec_path, &tune.spec_text())?;
        (tune.k_done, tune.theta) = read_state(&tune.state_path(), kind.names(), tune.theta.clone())?;
        tune.next_k = tune.k_done;
        check_pairs_header(&tune.pairs_path(), kind)?;
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
    /// start, range and `c_end`. A time tune says its kind, and the search both sides play; a search
    /// tune's text is what it was before the time tune, so one under way still resumes.
    fn spec_text(&self) -> String {
        let mut text = format!("player\t{}\n", self.player);
        if self.kind == TuneKind::Time {
            text.push_str(&format!("kind\t{}\n", self.kind.text()));
            if let Some(search) = &self.search {
                let values: Vec<String> = search.values().iter().map(i32::to_string).collect();
                text.push_str(&format!("search\t{}\n", values.join(",")));
            }
        }
        text.push_str(&format!("pairs\t{}\nr_end\t{}\nseed\t{}\ncontrol\t{}\n", self.pairs, self.r_end, self.seed, self.control));
        if let Some(nps) = self.reference_nps {
            text.push_str(&format!("reference_nps\t{nps}\n"));
        }
        for (name, spec) in self.kind.names().iter().zip(self.specs.iter()) {
            text.push_str(&format!("{name}\t{}\t{}\t{}\t{}\n", spec.start, spec.min, spec.max, spec.c_end));
        }
        text
    }

    /// Records `nps`, the first start's bench, as the tune's reference speed, `spec.txt` rewritten
    /// with it, so every resume reads it back rather than benching again.
    pub(crate) fn record_reference(&mut self, nps: f64) -> Result<(), String> {
        self.reference_nps = Some(nps);
        write_atomically(&format!("{}/spec.txt", self.dir), &self.spec_text())
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
    /// parameters both, `network` the tuned player's hash, its `k` in a tag. A search tune's sides
    /// play the default clock; a time tune's play its search, the same on both sides, and their own
    /// time points.
    pub(crate) fn entry(&self, k: u32, network: &str) -> Entry {
        let delta = spsa::draw_delta(self.seed, k, self.specs.len());
        let side = |sign: i32| {
            let values = spsa::point(&self.theta, &self.schedule, &self.specs, k, &delta, sign);
            let (parameters, time) = match self.kind {
                TuneKind::Search => (spsa::search_parameters(&values), None),
                TuneKind::Time => (self.search.expect("a time tune has its search"), Some(spsa::time_parameters(&values))),
            };
            Player { network: network.to_string(), parameters: Some(parameters), time }
        };
        Entry { players: vec![side(1), side(-1)], count: 1, tags: vec![(K_TAG.to_string(), k.to_string())] }
    }

    /// Applies pair `k`, result `r` from the plus side, played on `source`, and records it:
    /// `state.tsv` rewritten, one `pairs.tsv` row. A `k` already applied is skipped.
    pub(crate) fn record(&mut self, k: u32, r: i32, source: &str) -> Result<(), String> {
        if self.done() || !self.applied.insert(k) {
            return Ok(());
        }
        let delta = spsa::draw_delta(self.seed, k, self.specs.len());
        self.theta = spsa::update(&self.theta, &self.schedule, &self.specs, k, &delta, r);
        self.k_done += 1;
        write_atomically(&self.state_path(), &state_text(self.k_done, self.kind.names(), &self.theta))?;
        let signs: String = delta.iter().map(|&sign| if sign > 0 { '+' } else { '-' }).collect();
        let values: Vec<String> = self.theta.iter().map(|value| format!("{value:.3}")).collect();
        append(&self.pairs_path(), &format!("{k}\t{r}\t{source}\t{signs}\t{}\n", values.join("\t")))
    }

    /// Writes the result to the player's file of its kind: a search tune's theta rounded to
    /// `search-parameters.txt`, a time tune's as it is to `time-parameters.txt`. In the tune's folder
    /// first, then, when the network's is elsewhere (on the remote, its generation moved there),
    /// copied there whole.
    pub(crate) fn finish(&self) -> Result<(), String> {
        let file = Path::new(&self.parameters_path).file_name().map_or(String::new(), |name| name.to_string_lossy().into_owned());
        let here = format!("{}/{file}", self.dir);
        match self.kind {
            TuneKind::Search => spsa::search_parameters(&self.theta).write_file(&here)?,
            TuneKind::Time => spsa::time_parameters(&self.theta).write_file(&here)?,
        }
        if here != self.parameters_path {
            remote::copy_replacing(&here, &self.parameters_path).map_err(|error| error.to_string())?;
        }
        Ok(())
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

    /// Each parameter's name, what it does, its start and theta now.
    pub(crate) fn rows(&self) -> Vec<(&'static str, &'static str, f64, f64)> {
        let kind = self.kind;
        (0..self.specs.len()).map(|i| (kind.names()[i], kind.descriptions()[i], self.specs[i].start, self.theta[i])).collect()
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

/// The reference speed a `spec.txt`'s `text` records, `None` before its first bench.
fn reference_in_spec(text: &str) -> Option<f64> {
    text.lines().find_map(|line| line.strip_prefix("reference_nps\t")).and_then(|value| value.parse().ok())
}

/// The kind, pairs and specs a `spec.txt`'s `text` records; a spec without a kind line a search
/// tune's.
fn run_in_spec(text: &str) -> Option<(TuneKind, u32, Vec<Spec>)> {
    let kind = match text.lines().find_map(|line| line.strip_prefix("kind\t")) {
        Some(kind) => TuneKind::parse(kind).ok()?,
        None => TuneKind::Search,
    };
    let pairs = text.lines().find_map(|line| line.strip_prefix("pairs\t")).and_then(|value| value.parse().ok())?;
    let mut specs = kind.specs().to_vec();
    for (spec, name) in specs.iter_mut().zip(kind.names().iter()) {
        let line = text.lines().find_map(|line| line.strip_prefix(&format!("{name}\t")))?;
        let values: Vec<&str> = line.split('\t').collect();
        let [start, min, max, c_end] = values.as_slice() else { return None };
        *spec = Spec { start: start.parse().ok()?, min: min.parse().ok()?, max: max.parse().ok()?, c_end: c_end.parse().ok()? };
    }
    Some((kind, pairs, specs))
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

/// `state.tsv`'s text: `k_done`, then theta, one `name<TAB>value` line each in `names`' order, the
/// values exact (Rust's shortest representation reads back to the same `f64`).
fn state_text(k_done: u32, names: &[&str], theta: &[f64]) -> String {
    let mut text = format!("k_done\t{k_done}\n");
    for (name, value) in names.iter().zip(theta.iter()) {
        text.push_str(&format!("{name}\t{value}\n"));
    }
    text
}

/// `state.tsv` at `path`, its parameters `names`: `k_done` and theta, or 0 and `start` when there
/// is none.
fn read_state(path: &str, names: &[&str], start: Theta) -> Result<(u32, Theta), String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((0, start)),
        Err(error) => return Err(format!("{path}: {error}")),
    };
    let malformed = || format!("{path}: expected k_done and the {} parameters, one name<TAB>value line each", names.len());
    let mut lines = text.lines();
    let k_done: u32 = match lines.next().and_then(|line| line.split_once('\t')) {
        Some(("k_done", value)) => value.parse().map_err(|_| malformed())?,
        _ => return Err(malformed()),
    };
    let mut theta = start;
    for (i, name) in names.iter().enumerate() {
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
fn pairs_header(kind: TuneKind) -> String {
    format!("k\tr\tsource\tdelta\t{}\n", kind.names().join("\t"))
}

/// Starts `pairs.tsv` with its header, or checks that an existing one has today's.
fn check_pairs_header(path: &str, kind: TuneKind) -> Result<(), String> {
    let header = pairs_header(kind);
    match std::fs::read_to_string(path) {
        Ok(text) if text.lines().next() == Some(header.trim_end()) => Ok(()),
        Ok(_) => Err(format!("{path} has another header than today's ({}): move it away to resume", header.trim_end())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => append(path, &header),
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

    /// A search tune of pesto in `dir`, `pairs` pairs at depth 2, as `open` would make it.
    pub(crate) fn tune_in(dir: &str, pairs: u32) -> Tune {
        tune_of_kind_in(dir, pairs, TuneKind::Search)
    }

    /// The search a time tune of the tests plays.
    const TIME_TUNE_SEARCH: SearchParameters = SearchParameters { razoring_margin: 111, ..SearchParameters::DEFAULT };

    /// A tune of `kind` of pesto in `dir`, as `tune_in`; a time tune plays `TIME_TUNE_SEARCH`.
    fn tune_of_kind_in(dir: &str, pairs: u32, kind: TuneKind) -> Tune {
        std::fs::create_dir_all(dir).unwrap();
        let specs = kind.specs().to_vec();
        let file = match kind {
            TuneKind::Search => fattorini::nnue::SEARCH_PARAMETERS_FILE,
            TuneKind::Time => fattorini::nnue::TIME_PARAMETERS_FILE,
        };
        let mut tune = Tune {
            player: "pesto".to_string(),
            network: None,
            kind,
            search: (kind == TuneKind::Time).then_some(TIME_TUNE_SEARCH),
            pairs,
            r_end: spsa::R_END,
            seed: spsa::seed_of("pesto", kind),
            control: "depth 2".to_string(),
            reference_nps: std::fs::read_to_string(format!("{dir}/spec.txt")).ok().and_then(|text| reference_in_spec(&text)),
            schedule: Schedule::new(pairs, spsa::R_END, &specs),
            theta: spsa::start_theta(&specs),
            specs,
            dir: dir.to_string(),
            parameters_path: format!("{dir}/{file}"),
            k_done: 0,
            next_k: 0,
            returned: Vec::new(),
            applied: std::collections::HashSet::new(),
        };
        check_spec(&format!("{dir}/spec.txt"), &tune.spec_text()).unwrap();
        (tune.k_done, tune.theta) = read_state(&tune.state_path(), kind.names(), tune.theta.clone()).unwrap();
        tune.next_k = tune.k_done;
        check_pairs_header(&tune.pairs_path(), kind).unwrap();
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
        assert_eq!((again.k_done, &again.theta), (4, &tune.theta), "the state reads back");
        assert_eq!(SearchParameters::read_file(tune.parameters_path()).unwrap(), None);
        tune.finish().unwrap();
        assert!(tune.parameters_path().ends_with("/search-parameters.txt"));
        assert_eq!(SearchParameters::read_file(tune.parameters_path()).unwrap(), Some(spsa::search_parameters(&tune.theta)));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A time tune: both sides play its search and their own time points; its state, spec and result
    /// are the 22 time parameters', unrounded.
    #[test]
    fn a_time_tune_plays_its_search_and_tunes_the_clock() {
        let dir = temp_dir("time");
        let mut tune = tune_of_kind_in(&dir, 4, TuneKind::Time);
        let entry = tune.entry(2, "pesto");
        for player in &entry.players {
            assert_eq!(player.parameters, Some(TIME_TUNE_SEARCH), "the same search on both sides");
        }
        assert_ne!(entry.players[0].time, entry.players[1].time);
        assert!(entry.players[0].time.is_some());
        assert_eq!(run_in_spec(&tune.spec_text()), Some((TuneKind::Time, 4, spsa::TIME_SPECS.to_vec())));
        assert!(tune.spec_text().contains("kind\ttime\n"));
        for k in 0..4 {
            tune.take_k();
            tune.record(k, 1, "alpha").unwrap();
        }
        let again = tune_of_kind_in(&dir, 4, TuneKind::Time);
        assert_eq!((again.k_done, &again.theta), (4, &tune.theta), "22 values read back");
        assert_eq!(tune.rows().len(), 22);
        tune.finish().unwrap();
        assert!(tune.parameters_path().ends_with("/time-parameters.txt"));
        let tuned = fattorini::search::TimeParameters::read_file(tune.parameters_path()).unwrap().unwrap();
        assert_eq!(tuned, spsa::time_parameters(&tune.theta), "unrounded");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The reference speed is benched once: recorded in the spec, read back by a reopen, which then
    /// agrees with the spec rather than being refused; none before it is recorded.
    #[test]
    fn a_tunes_reference_speed_is_kept_in_its_spec_across_a_reopen() {
        let dir = temp_dir("reference");
        let mut tune = tune_in(&dir, 4);
        assert_eq!(tune.reference_nps, None);
        assert!(!tune.spec_text().contains("reference_nps"));
        tune.record_reference(1_154_842.5).unwrap();
        let again = tune_in(&dir, 4);
        assert_eq!(again.reference_nps, Some(1_154_842.5), "read back, exact");
        assert!(again.spec_text().contains("reference_nps\t1154842.5\n"));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A search tune's spec is what it was before kinds existed, so one under way resumes.
    #[test]
    fn a_search_tunes_spec_has_no_kind_line() {
        let dir = temp_dir("search-spec");
        let tune = tune_in(&dir, 4);
        let text = tune.spec_text();
        assert!(text.starts_with("player\tpesto\npairs\t4\n"), "{text}");
        assert!(text.contains("\nrazoring_margin\t150\t50\t400\t19\n"), "whole numbers as before: {text}");
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
        assert_eq!(run_in_spec(&tune.spec_text()), Some((TuneKind::Search, 4, spsa::SEARCH_SPECS.to_vec())));
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
