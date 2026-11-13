//! The smithy, `smithy start`, on any machine: its jobs dealt as chores to the arenas (`arena start`), over HTTP
//! (`fattorini::pipeline::protocol`), on `127.0.0.1` and on the LAN. It claims an arena on each machine
//! that file's `arenas` lists that answers, unclaimed, and polls those it holds
//! (`keep_claims`). Its jobs are started and stopped from its page. A job of the arena slot, one at a time,
//! is a tune (`--tune <player>`, `tune.rs`), PeSTO's Play (`--play pesto`, its corpus, `pesto.rs`), or
//! a forge (`--forge <config> <run>`), a run of a config: the simplified self-training route, whole
//! generations run unattended. Beside it, the source slot holds Lc0's Convert (`--convert lc0`,
//! `lc0/`) or a book job (`--book <source> [--weights counts]`, `book.rs`), one at a time: neither
//! needs the arenas nor the GPU, and both stream large files through the remote. A generation:
//!
//! 1. **Play:** self-play with the teacher, a frozen test set first, then the training set, each
//!    game appended as it arrives, closed into segments of `SEGMENT_POSITIONS`. Generation 1 plays
//!    nothing: it trains on its source's whole corpus (`lc0-`, `pesto-`), judged on the source's
//!    frozen test set.
//! 2. **Train:** `train_in_lockstep`, 9 lambdas stacked, the direct blend, patience and rollback on
//!    the frozen test set (`freeze.rs`), warm-started from the teacher.
//! 3. **Round robin** among the 9 candidates.
//! 4. **Gate:** the winner against the teacher, an SPRT.
//! 5. **Decide:** a pass promotes the winner and centres the next grid on its lambda, the next
//!    generation waiting for the developer's Resume; a fail stops the run (`Phase::Stopped`), a new
//!    run starting over. A generation has one training set, no retry with more data (the developer,
//!    2026-11-02).
//!
//! Each run lives in the repository's `data/networks/<config>/run-<n>/` (`players::run_dir`), with its
//! own frozen copy of its config, local to the smithy; every phase resumes from its files. Its files
//! reach Drive, the remote (`remote::remote_path`), whole, never written there piece by piece: the
//! files that change along the run copied at every phase boundary (`Run::mirror_living`), every
//! network as soon as it is made (`publish`), the arenas reading each by its name there, and a closed
//! generation moved whole (`archive.rs`). `smithy stop` and `status` reach a running smithy over
//! `127.0.0.1` alone.
//!
//! A smithy runs on any machine. A full run trains only where CUDA is (`has_gpu`); a scaled run
//! (`GenConfig::scale`, `scale.rs`) is a run like any other, on Drive, at `1/s` of its volumes and
//! work, and trains on the CPU.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use rand::RngExt;
use serde::{Deserialize, Serialize};

use fattorini::pipeline::lan::{self, first_line_bytes};
use lan_http::{Listener, Reply, Request};
use fattorini::pipeline::protocol::{
    self, entries_text, Alive, Chore, Claim, Entry, Hello, HelloReply, Limit, Player, TaskKind, ALIVE_PATH, ARENA_PORT,
    CHORE_PATH, CHORE_TAG, CLAIM_PATH, CLAIM_POLL, HELLO_PATH, JOBS_PATH, LATE_AFTER, LOOPBACK, MACHINES_PATH, PESTO, RELEASE_AFTER,
    RELEASE_PATH, SCORE_FEN_TAG, SCORE_MOVE_TAG, SCORE_PARENT_TAG, SEQ_TAG, SESSION_TAG, SMITHY_PORT, STATUS_PATH, STOP_PATH,
    TASK_PATH, ScoreRecord,
};
use fattorini::pipeline::preprocessing_config;
use fattorini::pipeline::preprocessing_config::{lambdas_text, ArmLambda};
use fattorini::pipeline::segments::{self, SegmentMeta, META_SUFFIX};
use fattorini::pipeline::{generation, players, remote};

use super::archive::{self, Archiver};
use super::{lease, tree};
use super::scale;
use super::cache::CacheRecord;
use fattorini::nnue::Tuning;
use fattorini::labels::{Curve, LOGISTIC};
use fattorini::bench::loaded_bench;
use fattorini::pipeline::spsa::TuneKind;
use fattorini::search::{SearchParameters, TimeParameters};

use super::matches;
use super::rounds_display;
use super::sprt::{Sprt, Verdict};
use super::tune::{self, Tune};
use super::{book, book_scores, freeze, lc0, pesto, polyline, tasks};

use super::bucketed_train_stream::BucketedTrainStream;
use super::grouped_test_set::GroupedTestSet;
use super::net_config::{self, ArchitectureKind, NetConfig};
use super::stacked_transformer::StackedTransformer;
use super::{transformer_batch, transformer_network};
use super::pgn_stream::StreamFile;
use super::buckets::{self, BucketStream, Labelling, Prepared};
use super::pgn_train_stream;
use super::stacked_network::StackedNetwork;
use super::train_stream::BatchSource;
use super::candidates::{export_best, feature_set_of};
use super::lockstep::{self, open_stack, train_in_lockstep, Arm, LockstepParts};
use super::{batch, candidates, checkpoint_store, nnue_network, round, round_robin};

const PREFIX: &str = "smithy";

/// Where the NNUE forges' buckets are scattered (`buckets.rs`): local, disposable, never on Drive.
const BUCKETS_ROOT: &str = "data/buckets";

/// The grid's size and fine step: 9 lambdas 0.01 apart, centred on the previous winner.
const GRID: i32 = 9;
const STEP_HUNDREDTHS: i32 = 1;

const POLL: Duration = Duration::from_millis(500);

/// A chore's tasks per arena worker, and what each top-up adds. Each task is sent as it ends, so
/// a larger chore costs nothing but the tasks still in flight when the work it serves is done: a
/// gate's pairs when its SPRT decides, wasted once.
const TASKS_PER_WORKER: u32 = 4;

/// An arena's chore is topped up once the tasks it has left, by the smithy's estimate (dealt, less
/// arrived, less one in flight per worker), fall below this many per worker: before any worker runs
/// out, so none idles while the top-up travels.
const LOW_WATER_PER_WORKER: u64 = 2;

/// How long a claim request or a poll may take: an arena is on the LAN, and a machine that does not
/// answer must not hold up the others'.
const CLAIM_TIMEOUT: Duration = Duration::from_secs(5);

/// The longest wait between two claims of a machine that keeps failing them: its waits double from
/// `CLAIM_POLL` up to this.
const CLAIM_BACKOFF_CAP: Duration = Duration::from_secs(5 * 60);

/// Self-play games are appended, as they arrive, to an open file per directory, closed into a
/// segment (`.pgn.zst` and its `.meta`, as the readers take them) at this many positions, and when
/// the play phase ends. Lc0's segments' size: a generation of at most 1B positions is then about
/// 1,075 segments in one flat directory, no more than Lc0's corpus (the developer, 2026-11-02).
const SEGMENT_POSITIONS: u64 = 1_000_000;

/// The gate's pairs at most, should the SPRT never decide: then it fails.
const GATE_MAX_PAIRS: u32 = 5_000;

/// Logs `text`, one of the smithy's events: the terminal is their log (`progress::log_events`).
fn say(text: String) {
    log::info!("{text}");
}

/// A forge config's own keys: one file holds them and its network's sections (`NETWORK_KEYS`), which
/// `net_config::load` reads.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenConfig {
    /// How each generation's arms are labelled: the grid of constant lambdas (the default), or one
    /// arm at the polyline, each position at its own lambda by its progress (`polyline.rs`).
    #[serde(default)]
    lambda: LambdaMode,
    /// Generation 1's grid centre; later ones centre on the previous winner. A grid's alone: a
    /// polyline run has none.
    #[serde(default)]
    grid_center: Option<f64>,
    /// Training positions a self-play generation plays, from generation 2 on: generation 1 trains on
    /// its source's whole corpus instead.
    training_positions: u64,
    /// Each teacher's frozen test set, as `training_positions`: a source's generation 1 is judged on
    /// the source's own.
    test_positions: u64,
    round_robin_pairs: u32,
    gate_elo_d: f64,
    /// The tune of each generation whose challenger passed its gate, started by hand from the page.
    tune: tune::TuneConfig,
    /// A scaled run's `s`, its volumes and work `1/s` of a full run's (`scale_forge`): written into
    /// the run's own copy of its config at New run (`freeze_run_config`), never in `configs/`, so a
    /// run is scaled or not for its whole life.
    #[serde(default)]
    scale: Option<u64>,
    /// The source generation 1 trains on, from the file's `labels:`: set by `load_config`, never
    /// written as a forge key, and always there once loaded.
    #[serde(skip)]
    source: Option<Source>,
}

/// A forge config's `lambda:` key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum LambdaMode {
    #[default]
    Grid,
    Polyline,
}

impl GenConfig {
    fn training_positions(&self) -> u64 {
        self.training_positions
    }

    fn gate_elo_d(&self) -> f64 {
        self.gate_elo_d
    }

    /// The source generation `generation` trains on: a sourced run's first, gated against PeSTO;
    /// every later one is self-play taught by its predecessor's winner.
    fn on_source(&self, generation: u32) -> Option<Source> {
        self.source.filter(|_| generation == 1)
    }
}

/// The name of `source`'s buckets in `BUCKETS_ROOT`: the source's name (`lc0`, `pesto`).
fn source_buckets(source: Source) -> String {
    source.name().to_string()
}

/// A source: a corpus made once, by its own job, for every forge named after it. Lc0's is converted
/// from Lc0's training tars, PeSTO's played by the arenas with the PeSTO evaluation; both live on the
/// remote, read in place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    Lc0,
    Pesto,
}

impl Source {
    fn of(labels: preprocessing_config::Labels) -> Source {
        match labels {
            preprocessing_config::Labels::Lc0 => Source::Lc0,
            preprocessing_config::Labels::Pesto => Source::Pesto,
        }
    }

    /// Its config's name, the prefix of every forge it serves.
    fn name(self) -> &'static str {
        match self {
            Source::Lc0 => "lc0",
            Source::Pesto => PESTO,
        }
    }

    fn root(self) -> &'static str {
        match self {
            Source::Lc0 => preprocessing_config::LC0_ROOT,
            Source::Pesto => preprocessing_config::PESTO_ROOT,
        }
    }

    /// What its corpus's centipawns are read through: Lc0's files through Lc0's curve, which gives back
    /// its Q itself, an error until they are rewritten into it (`lc0::rewrite`); PeSTO's through the
    /// logistic at S.
    fn curve(self) -> Result<Curve, String> {
        match self {
            Source::Lc0 => lc0::rewrite::corpus_curve(),
            Source::Pesto => Ok(LOGISTIC),
        }
    }

    fn corpus(self) -> &'static str {
        match self {
            Source::Lc0 => "Lc0's corpus",
            Source::Pesto => "PeSTO's corpus",
        }
    }

    fn is_complete(self) -> bool {
        match self {
            Source::Lc0 => lc0::is_complete(),
            Source::Pesto => pesto::is_complete(),
        }
    }

    /// Its frozen test set's positions, its config's.
    fn test_positions(self) -> Result<u64, String> {
        match self {
            Source::Lc0 => lc0::test_positions(),
            Source::Pesto => Ok(pesto::load()?.test_positions),
        }
    }

    /// Why a run cannot start on it yet: its corpus incomplete, and the job that completes it.
    fn incomplete(self) -> String {
        let job = match self {
            Source::Lc0 => "Convert, on the lc0 page",
            Source::Pesto => "Play, on the pesto page",
        };
        format!("{} is not complete: {job} makes it, and writes {}/complete.txt", self.corpus(), self.root())
    }
}

/// A forge config's name is its source's and an architecture's, `lc0-psq`, `pesto-psq`.
fn check_forge_name(name: &str, source: Option<Source>) -> Result<(), String> {
    let Some(source) = source else {
        return Err(format!("{name}: a forge has a source, labels: lc0 or pesto"));
    };
    if name.strip_prefix(source.name()).is_some_and(|rest| rest.len() > 1 && rest.starts_with('-')) {
        return Ok(());
    }
    Err(format!("{name}: a config with labels: {} is named {}-<architecture>", source.name(), source.name()))
}

/// A forge config's top-level keys that are its network's (`net_config::load`, `labels:` read by
/// `preprocessing_config::load_labels`). Any other key is a forge key, read strictly, so a retired
/// key is refused as an unknown one.
const NETWORK_KEYS: [&str; 3] = ["labels", "architecture", "training"];

/// The forge config at `path`, one file: its forge keys, strictly, and its network's sections.
/// An error for a file that is not one (no `architecture:`, as a tune config); a network section
/// that cannot be read panics, as `net_config::load` does.
fn load_config(path: &str) -> Result<(GenConfig, NetConfig), String> {
    let text = std::fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?;
    let mut keys: serde_yaml::Mapping = serde_yaml::from_str(&text).map_err(|error| format!("{path}: {error}"))?;
    if !keys.contains_key("architecture") || !keys.contains_key("labels") {
        return Err(format!("{path} is not a forge config: it has no architecture: or labels:"));
    }
    keys.retain(|key, _| !key.as_str().is_some_and(|key| NETWORK_KEYS.contains(&key)));
    let mut config: GenConfig = serde_yaml::from_value(serde_yaml::Value::Mapping(keys)).map_err(|error| format!("{path}: {error}"))?;
    config.source = Some(Source::of(preprocessing_config::load_labels(path)));
    if config.test_positions == 0 || config.round_robin_pairs == 0 || config.training_positions == 0 {
        return Err(format!("{path}: training_positions, test_positions and round_robin_pairs must be positive"));
    }
    if config.gate_elo_d <= 0.0 {
        return Err(format!("{path}: gate_elo_d must be positive"));
    }
    match (config.lambda, config.grid_center) {
        (LambdaMode::Grid, Some(center)) if (0.0..1.0).contains(&center) => {}
        (LambdaMode::Grid, _) => return Err(format!("{path}: grid_center must lie in (0, 1)")),
        (LambdaMode::Polyline, Some(_)) => return Err(format!("{path}: grid_center is a grid's; lambda: polyline has none")),
        (LambdaMode::Polyline, None) => {}
    }
    tune::args_of(&config.tune, PESTO).map_err(|message| format!("{path}: {message}"))?;
    let mut net = net_config::read(path)?;
    // A transformer is labelled per lambda by its PGN stream, which knows no progress.
    if config.lambda == LambdaMode::Polyline && matches!(net.architecture.kind, ArchitectureKind::Transformer { .. }) {
        return Err(format!("{path}: lambda: polyline trains an NNUE only, not a transformer"));
    }
    if config.scale.is_some() && path == preprocessing_config::net_config_path(&config_name_of(path)) {
        return Err(format!("{path}: scale: is a run's, chosen at New run, never a config's"));
    }
    if config.scale == Some(0) {
        return Err(format!("{path}: scale must be a whole number from 1"));
    }
    if let Some(s) = config.scale {
        scale_forge(&mut config, &mut net, s);
    }
    Ok((config, net))
}

/// The config name `path`, a `configs/` file's path, would be the file of: its stem.
fn config_name_of(path: &str) -> String {
    Path::new(path).file_stem().map_or(String::new(), |stem| stem.to_string_lossy().into_owned())
}

/// `config` and its network shrunk by a scaled run's `s` (`scale.rs`): its data volumes and its
/// work a round by `1/s`, its pairs by `sqrt(s)`, its shape kept.
fn scale_forge(config: &mut GenConfig, net: &mut NetConfig, s: u64) {
    let training = &mut net.training;
    let batch = |size: i64| scale::volume(size.max(0) as u64, s, scale::MIN_BATCH) as i64;
    training.training_batch_size = batch(training.training_batch_size);
    training.test_batch_size = batch(training.test_batch_size);
    training.steps_per_round = scale::volume(training.steps_per_round, s, 1);
    config.training_positions = scale::volume(config.training_positions, s, scale::MIN_POSITIONS);
    config.test_positions = scale::volume(config.test_positions, s, scale::MIN_BATCH);
    config.round_robin_pairs = scale::games(config.round_robin_pairs, s, scale::MIN_PAIRS);
}

impl GenConfig {
    /// The depth a run's games are searched at: `generation::SEARCH_DEPTH`, scaled for a scaled run
    /// (`scale::depth`). The chore carries it, so an arena plays whatever it is told.
    fn depth(&self) -> u32 {
        match self.scale {
            Some(s) => scale::depth(generation::SEARCH_DEPTH, s),
            None => generation::SEARCH_DEPTH,
        }
    }
}

/// Run `run`'s local directory: `players::run_dir`. `run` is a run's name already: made by
/// `players::run_name`, or one from outside read by `players::parse_run` first, where it enters
/// (`generation_view`, `tune_args`).
fn run_dir(run: &str) -> String {
    players::run_dir(run).expect("a run's name, checked where it entered")
}

/// `players::run_dir` on the remote, for a run's name already checked as `run_dir`'s.
fn remote_run_dir(run: &str) -> String {
    remote::remote_path(&players::run_dir(run).expect("a run's name, checked where it entered"))
}

/// The numbers of `config`'s runs here, ascending, as `run_dir` places them.
fn runs_of(config: &str) -> Vec<u32> {
    players::runs_in(&players::config_dir(config))
}

/// A scaled run's share of a source's training `files`: the first of them, in their fixed order, until
/// they hold `1/s` of the corpus's positions (`scale::volume`), at least one file. The same files every
/// time, so the share's buckets are made once.
fn scaled_share(files: Vec<StreamFile>, s: u64) -> Vec<StreamFile> {
    let wanted = scale::volume(files.iter().map(|file| file.positions).sum(), s, scale::MIN_POSITIONS);
    let mut held = 0;
    files
        .into_iter()
        .take_while(|file| {
            let more = held < wanted;
            held += file.positions;
            more
        })
        .collect()
}

/// A new run's number: past every run here and every run on the remote.
fn next_run_number(local: &[u32], remote: &[u32]) -> u32 {
    local.iter().chain(remote).max().map_or(1, |last| last + 1)
}

/// A run's frozen copy of its config.
const RUN_CONFIG: &str = "config.yaml";

/// The config run `run` trains by: the copy frozen into its directory as it started, so editing
/// `configs/` changes only new runs; `configs/`' own for a run moved there by hand without one,
/// until `freeze_run_config` writes it.
fn load_run_config(run: &str) -> Result<(GenConfig, NetConfig), String> {
    // A run started in another smithy may be on the remote alone, its frozen copy there.
    match frozen_config(&[run_dir(run), remote_run_dir(run)]) {
        Some(frozen) => load_config(&frozen),
        None => load_config(&preprocessing_config::net_config_path(players::parse_run(run)?.0)),
    }
}

/// The first of `roots`, run directories in order of preference, holding a frozen config: its path.
fn frozen_config(roots: &[String]) -> Option<String> {
    roots.iter().map(|root| format!("{root}/{RUN_CONFIG}")).find(|frozen| Path::new(frozen).is_file())
}

/// Where run `run`'s whole-run files (`state.txt`, `generations.tsv`) are read: this machine's copy
/// while it has one, else the remote's, for a run started in another smithy.
fn run_home(run: &str) -> Dirs {
    home_of(Dirs { root: run_dir(run) }, Dirs { root: remote_run_dir(run) })
}

/// `local` when it holds a run's state, else `remote`.
fn home_of(local: Dirs, remote: Dirs) -> Dirs {
    if Path::new(&local.state()).is_file() { local } else { remote }
}

/// The numbers of `config`'s runs here and on the remote, ascending: one started in another smithy
/// is listed, viewed and tuned here too.
fn known_runs(config: &str) -> Vec<u32> {
    let mut numbers = runs_of(config);
    numbers.extend(players::runs_in(&remote::remote_path(&players::config_dir(config))));
    numbers.sort_unstable();
    numbers.dedup();
    numbers
}

/// Copies `configs/<config>.yaml` into run `run`'s directory, the run's own from then on, with
/// `scale:` appended for a scaled run; a copy already there is kept.
fn freeze_run_config(run: &str, config: &str, scale: Option<u64>) -> Result<(), String> {
    let frozen = format!("{}/{RUN_CONFIG}", run_dir(run));
    if Path::new(&frozen).is_file() {
        return Ok(());
    }
    let source = preprocessing_config::net_config_path(config);
    let mut text = std::fs::read_to_string(&source).map_err(|error| format!("{source}: {error}"))?;
    if let Some(s) = scale {
        text.push_str(&format!("\n# A scaled run, chosen at New run: its volumes and work 1/{s} of a full run's.\nscale: {s}\n"));
    }
    std::fs::write(&frozen, text).map_err(|error| format!("{frozen}: {error}"))
}

/// The coarse grid, 0.10 to 0.90, of the first generation on new labels: one that plays its own
/// games after a parent trained on a source. Where its best lambda lies is not known: Lc0's
/// deep-search scores won near 0.02, earlier shallow self-play near 0.98. The next generation is fine
/// again, around its winner (the developer, 2026-10-30).
const COARSE_STEP_HUNDREDTHS: i32 = 10;
const COARSE_CENTER_HUNDREDTHS: i32 = 50;

/// The 9 lambdas `step_hundredths` apart centred on `center_hundredths`, sliding so that all lie in
/// 0.01..=0.99, in hundredths, ascending.
fn grid(center_hundredths: i32, step_hundredths: i32) -> Vec<i32> {
    let half = GRID / 2 * step_hundredths;
    let center = center_hundredths.clamp(1 + half, 99 - half);
    (0..GRID).map(|index| center - half + index * step_hundredths).collect()
}

/// The next generation's grid centre and step, once a generation won by `winner` is promoted: the
/// coarse grid when that generation trained on a source, since the next labels are its own games'
/// and new; the fine grid around the winner otherwise.
fn next_grid(on_source: bool, winner: f64) -> (i32, i32) {
    if on_source {
        (COARSE_CENTER_HUNDREDTHS, COARSE_STEP_HUNDREDTHS)
    } else {
        ((winner * 100.0).round() as i32, STEP_HUNDREDTHS)
    }
}

fn lambda_of(hundredths: i32) -> f64 {
    f64::from(hundredths) / 100.0
}

// -- The run's state --------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Play,
    Train,
    RoundRobin,
    Gate,
    /// A run of a network the engine cannot play, its training over: no round robin, no gate. Every
    /// other lineage ends by a failed gate instead, and never reaches it.
    Done,
    /// A lineage ended by its generation's failed gate, or by a generation that learned nothing: a
    /// generation has one training set, so nothing is left to try (the developer, 2026-11-02).
    Stopped,
}

impl Phase {
    fn name(self) -> &'static str {
        match self {
            Phase::Play => "play",
            Phase::Train => "train",
            Phase::RoundRobin => "round-robin",
            Phase::Gate => "gate",
            Phase::Done => "done",
            Phase::Stopped => "stopped",
        }
    }

    fn parse(text: &str) -> Result<Phase, String> {
        match text {
            "play" => Ok(Phase::Play),
            "train" => Ok(Phase::Train),
            "round-robin" => Ok(Phase::RoundRobin),
            "gate" => Ok(Phase::Gate),
            "done" => Ok(Phase::Done),
            "stopped" => Ok(Phase::Stopped),
            other => Err(format!("unknown phase {other:?}")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GenState {
    /// The generation being made: 1 is taught by PeSTO.
    generation: u32,
    phase: Phase,
    center_hundredths: i32,
    /// The grid's step: `STEP_HUNDREDTHS`, or `COARSE_STEP_HUNDREDTHS` on new labels.
    step_hundredths: i32,
}

impl GenState {
    /// The generation's lambdas, in hundredths.
    fn grid(&self) -> Vec<i32> {
        grid(self.center_hundredths, self.step_hundredths)
    }

    fn text(&self) -> String {
        format!(
            "generation\t{}\nphase\t{}\ncenter\t{}\nstep\t{}\n",
            self.generation,
            self.phase.name(),
            self.center_hundredths,
            self.step_hundredths
        )
    }

    fn parse(text: &str) -> Result<GenState, String> {
        let map = lan::fields(text);
        if map.contains_key("attempt") || map.contains_key("failed_gates") {
            return Err("a state from before attempts were removed (attempt, failed_gates): migrate the run by hand".to_string());
        }
        Ok(GenState {
            generation: lan::field(&map, "generation", "state")?,
            phase: Phase::parse(&lan::field::<String>(&map, "phase", "state")?)?,
            center_hundredths: lan::field(&map, "center", "state")?,
            // A state written before the step was: the fine grid, all it knew.
            step_hundredths: if map.contains_key("step") { lan::field(&map, "step", "state")? } else { STEP_HUNDREDTHS },
        })
    }
}

/// The run's paths.
struct Dirs {
    root: String,
}

impl Dirs {
    fn state(&self) -> String {
        format!("{}/state.txt", self.root)
    }
    fn log(&self) -> String {
        format!("{}/generations.tsv", self.root)
    }
    fn generation(&self, generation: u32) -> String {
        format!("{}/{}", self.root, players::generation_dir_name(generation))
    }
    fn teacher_vs(&self, generation: u32) -> String {
        format!("{}/teacher.vs", self.generation(generation))
    }
    fn teacher_nnue(&self, generation: u32) -> String {
        format!("{}/teacher.nnue", self.generation(generation))
    }
    fn test(&self, generation: u32) -> String {
        format!("{}/test", self.generation(generation))
    }
    /// The generation's training games, beside its test games.
    fn train(&self, generation: u32) -> String {
        format!("{}/train", self.generation(generation))
    }
    fn arm(&self, generation: u32, lambda: ArmLambda) -> String {
        generation::lambda_dir(&self.generation(generation), lambda)
    }
    fn round_robin(&self, generation: u32) -> String {
        format!("{}/round-robin.tsv", self.generation(generation))
    }
    fn gate(&self, generation: u32) -> String {
        format!("{}/gate.tsv", self.generation(generation))
    }
    /// Beside each pair record, its games.
    fn games_of(record: &str) -> String {
        format!("{}.pgn", record.strip_suffix(".tsv").unwrap_or(record))
    }
    /// The next chore's number, so chore ids stay unique across restarts.
    fn chores(&self) -> String {
        format!("{}/chores.txt", self.root)
    }
}

/// The `.pgn.zst` files of `dir` with their searched positions, by name: the `.meta` positions less
/// one final position a game, as a source's segments are listed (`pgn_train_stream::segment_files`),
/// so the buckets and the freeze count alike whatever the games' origin.
fn game_files(dir: &str) -> Result<Vec<StreamFile>, String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(Vec::new());
    };
    let mut names: Vec<String> =
        entries.filter_map(|entry| entry.ok()?.file_name().into_string().ok()).filter(|name| name.ends_with(".pgn.zst")).collect();
    names.sort();
    let mut files = Vec::new();
    for name in names {
        let path = format!("{dir}/{name}");
        let meta = SegmentMeta::read(&format!("{path}{META_SUFFIX}"))?;
        files.push(StreamFile { path, positions: meta.positions.saturating_sub(meta.games) });
    }
    Ok(files)
}

fn positions_in(dir: &str) -> Result<u64, String> {
    Ok(game_files(dir)?.iter().map(|file| file.positions).sum())
}

/// The searched positions of `dir`'s self-play games, the ones training and the test sets keep: its
/// closed segments' and `open`'s, the file still open there. A self-play game has no book and a score
/// after every move (`tasks.rs`), so its searched positions are all but its final one: `positions -
/// games`, read from the `.meta` files and `open`'s own counts, nothing decompressed.
fn searched_in(dir: &str, open: &SegmentWriter) -> Result<u64, String> {
    let closed: u64 = game_files(dir)?.iter().map(|file| file.positions).sum();
    Ok(open.positions - open.games + closed)
}

// -- What the request thread and the phases share --------------------------------------------------

/// What is being dealt.
#[derive(Clone)]
enum Work {
    Idle,
    /// Self-play with `teacher`: `test_left` positions into `test_dir` for the test set first, then
    /// `train_left` into `train_dir`. Every count is of searched positions, those with a search
    /// evaluation, all but each game's final one, as training and the freezer keep them: counting
    /// the final ones too left a corpus short of its test set. `before`, the positions each set held
    /// when this play began (test, training):
    /// the page shows the run's totals, a resume moving their start and never their total, its rate
    /// and ETA this play's alone. `targets`, each set's whole target (test, training), as the page
    /// shows it: a set already past its target, its last game overshooting, has none left, and
    /// `before` plus what is left would show the overshoot as the target. `parameters`, what the
    /// teacher plays: its tune.
    SelfPlay {
        teacher: String,
        parameters: SearchParameters,
        test_dir: String,
        test_left: u64,
        train_dir: String,
        train_left: u64,
        before: (u64, u64),
        targets: (u64, u64),
    },
    /// Pairs: for each pairing, `(challenger, teacher)` by name (`publish`), `wanted` pairs each, every result
    /// appended to `record` as `pairing<TAB>r<TAB>chore<TAB>session<TAB>seq`, its games to the PGN
    /// beside it (`Dirs::games_of`). `sides`, what the challenger and the teacher play in a gate or a
    /// round robin (a test's sides play `Shared::test`'s).
    Pairs { pairings: Vec<(String, String)>, wanted: u32, record: String, sides: [SearchParameters; 2] },
    /// A tune (`Shared::tune`) of the player whose network is `network`, by name or `PESTO`:
    /// one pair per `k`, each with its own parameters.
    Tune { network: String },
    /// A book's scoring (`Shared::score`): one task per book move, its record a row of the scores file.
    Score,
}

// What the forge's sides play (the developer, 2026-11-08, reversing 2026-11-03's "the forge never
// plays a tune"): the tune cannot be escaped, every network that ships being tuned, so it is used.
// Self-play plays its teacher's tune, the gate and round robin the defaults so they judge the network
// alone, except PeSTO, which plays its tune wherever it is a side. The tune at depth 9 cost no
// quality: the lc0 network's against the defaults drew at depth 9, leaning to the tune.

/// `player`'s finished search tune, its `search-parameters.txt` (PeSTO's in its tuning folder); `None` while it has
/// none, a tune under way included, which writes it only at its end.
fn finished_tune(player: &str) -> Result<Option<SearchParameters>, String> {
    if player == PESTO {
        return players::pesto_parameters();
    }
    match players::FattoriniPlayer::resolve(player)?.own_tuning()? {
        Tuning::Parameters(parameters) => Ok(Some(parameters)),
        Tuning::Untuned => Ok(None),
    }
}

/// `player`'s finished tune, refused while it has none (the developer, 2026-11-08: refused, not
/// waited for), saying what needs it and what to run.
fn required_tune(player: &str, needed_by: &str) -> Result<SearchParameters, String> {
    finished_tune(player)?.ok_or_else(|| format!("{player} has no finished tune, which {needed_by}: tune {player} first, from the page"))
}

/// The player that taught generation `generation` of `run`: generation g - 1's winner, or for
/// generation 1 its source's player, PeSTO for a PeSTO run; `None` for an Lc0 run's generation 1,
/// Lc0 being no Fattorini player.
fn teacher_player(run: &str, generation: u32, source: Option<Source>) -> Option<String> {
    if generation > 1 {
        Some(players::network_player_name(run, generation - 1, None))
    } else if source == Some(Source::Pesto) {
        Some(PESTO.to_string())
    } else {
        None
    }
}

/// What a forge's gate or round robin side plays: PeSTO its tune, every network the defaults.
fn side_parameters(player: &str, needed_by: &str) -> Result<SearchParameters, String> {
    if player == PESTO { required_tune(PESTO, needed_by) } else { Ok(SearchParameters::DEFAULT) }
}

/// A tune's `k`s per arena worker in a chore and in each top-up: each `k` is played at theta as it
/// was when dealt, so fewer than other chores' tasks, keeping theta fresh.
const KS_PER_WORKER: u32 = 2;

/// A chore dealt: to whom, for which work, and how many of its tasks were dealt and have arrived.
struct Dealt {
    session: String,
    /// The `Shared::epoch` it was dealt in: a task of a chore dealt for earlier work is dropped.
    epoch: u64,
    kind: TaskKind,
    /// Its players, as every task of it must name them.
    players: Vec<Player>,
    /// The pairing its tasks play, for pairs.
    pairing: Option<usize>,
    /// A tournament's matchups, its pairings each with its own counts; `players` and `pairing` are
    /// then unused.
    matchups: Vec<Matchup>,
    /// Its tasks dealt and arrived, every matchup's together for a tournament.
    dealt: u64,
    arrived: u64,
    /// Nothing more of it is wanted: a `STOP` or another chore was sent instead.
    closed: bool,
    /// A tune's `k`s dealt in it and not yet arrived, given back to the tune when it closes.
    ks: Vec<u32>,
}

/// One pairing of a tournament chore: its two players, challenger first, and its tasks dealt and
/// arrived.
struct Matchup {
    pairing: usize,
    players: Vec<Player>,
    dealt: u64,
    arrived: u64,
}

struct Session {
    hello: Hello,
    /// The chore its arena plays.
    chore: Option<String>,
    last_seen: Instant,
    /// When it said hello, or its last task arrived: where the next gap of play is measured from.
    joined: Instant,
    last_task: Option<Instant>,
    /// Tasks taken from it, for the status, and the time it played them: the gaps up to each
    /// arrival, from the hello or the arrival before it, each over `PAUSE_SECONDS` left out as a
    /// pause (its machine off or asleep, or no work dealt), so its rate is its pace while it plays.
    taken: u64,
    played: Duration,
}

impl Session {
    /// Counts a task arriving at `now`, its gap of play with it unless that gap was a pause.
    fn took(&mut self, now: Instant) {
        let gap = now.saturating_duration_since(self.last_task.unwrap_or(self.joined));
        if gap <= Duration::from_secs(PAUSE_SECONDS) {
            self.played += gap;
        }
        self.last_task = Some(now);
        self.taken += 1;
    }
}

#[derive(Default)]
struct Shared {
    work: Option<Work>,
    /// Raised at every change of work, so tasks of an earlier chore are told apart.
    epoch: u64,
    sessions: HashMap<String, Session>,
    next_session: u32,
    /// The next chore's number, persisted in `chores_path` (none in the tests) at every chore.
    next_chore: u64,
    chores_path: Option<String>,
    chores: HashMap<String, Dealt>,
    /// The tasks taken since the smithy started, by `(chore, session, seq)`: one arriving twice is dropped.
    /// A restart needs none: its new token turns every earlier session away.
    seen: HashSet<(String, String, String)>,
    /// The self-play files open in the test and training directories.
    test_segment: Option<SegmentWriter>,
    train_segment: Option<SegmentWriter>,
    /// Self-play positions received for the test set and the training set, this phase.
    test_got: u64,
    train_got: u64,
    /// When this self-play began dealing, a resume's start included: its box's time, as the other
    /// phases' (`self_play_time`).
    self_play_started: Option<Instant>,
    /// Pair results received this phase, by pairing, in arrival order.
    pairs: Vec<Vec<i32>>,
    /// Each of `pairs`' wins, losses and draws, aligned with it: read from the record
    /// (`recorded_wdl`) and kept as each comes; `None` for a pair recorded before they were. A test's
    /// card shows their sum.
    pair_wdl: Vec<Vec<Option<matches::Wdl>>>,
    /// The record's times, UTC Unix seconds, in order: each start of dealing (`false`) and each pair's
    /// arrival (`true`), read from the record (`recorded_times`) and kept as each comes: the phase's
    /// elapsed time, rate and ETA (`play_time`).
    pair_times: Vec<(u64, bool)>,
    /// The tune `Work::Tune` deals, and the smithy's speed with its network under a clock.
    tune: Option<Tune>,
    reference_nps: Option<f64>,
    /// A test's limit and its two sides' parameters, which its pair chores carry; none for a forge's
    /// pairs, at the forge's depth with their `Work::Pairs::sides`.
    test: Option<TestDealing>,
    /// The book's scoring `Work::Score` deals.
    score: Option<ScoreDealing>,
    /// The running forge's depth (`GenConfig::depth`), scaled for a scaled run;
    /// `generation::SEARCH_DEPTH` without a forge.
    forge_depth: Option<u32>,
    /// Whether the job is a scaled one (a scaled run's forge or its network's tune), whose claims give
    /// way to a full job's (`Claim::outranks`).
    scaled_job: bool,
    /// What the running phase knows beyond the work it deals, for the status page.
    view: View,
    /// A forge's files, which the page rebuilds every phase of its current generation from.
    forge: Option<ForgeFiles>,
    /// How the job ended, while the smithy stays up for its page until stopped.
    finished: Option<String>,
    /// The job's name, `None` while the smithy has had none.
    job: Option<String>,
    /// The config the job is of (`job_config`), whose page shows it.
    config: Option<String>,
    /// A training run's `state.txt`, for the page; none for a tune.
    state_path: Option<String>,
    /// The token every arena request carries, new with each job (`run_job`).
    token: String,
}

/// How a test's pairs are played: its limit, and the challenger's and the champion's parameters, each
/// given in full, the defaults or its tune; and their time parameters, each its time tune in full, or
/// `None` for the default clock.
struct TestDealing {
    limit: Limit,
    parameters: [SearchParameters; 2],
    time: [Option<TimeParameters>; 2],
}

/// A book's scoring as it is dealt: what every record must match (the network's player, depth,
/// commit and bench node count, the scores file's header's), the moves still unscored when it
/// started, each dealt by its index as a tune deals its `k`s (`Dealt::ks`), and its file.
struct ScoreDealing {
    source: String,
    /// The side as the job names it, `<network>@search-tuned`.
    side: String,
    network: String,
    parameters: SearchParameters,
    depth: u32,
    commit: String,
    bench_nodes: u64,
    moves: Vec<book::BookMove>,
    /// The indices of `moves` free to deal: never dealt, or given back by a closed chore.
    free: VecDeque<u32>,
    done: Vec<bool>,
    done_count: u64,
    /// The book's moves, and those scored before this start.
    total: u64,
    before: u64,
    file: book_scores::ScoresFile,
    /// Records turned away, and the sessions refused for good, each with why.
    rejected: u64,
    refused: Vec<(String, String)>,
    started: Instant,
}

/// How many book moves a score chore deals each arena worker, and tops it up by: each about a second
/// at depth 20, so a chore lasts minutes and a stopped arena loses little.
const SCORE_MOVES_PER_WORKER: usize = 16;

impl ScoreDealing {
    fn complete(&self) -> bool {
        self.done_count == self.moves.len() as u64
    }

    /// Up to `count` free moves, as score entries of `player`, and their indices.
    fn entries(&mut self, player: &Player, count: usize) -> (Vec<Entry>, Vec<u32>) {
        let ks: Vec<u32> = std::iter::from_fn(|| self.free.pop_front()).take(count).collect();
        let entries = ks
            .iter()
            .map(|&k| {
                let book_move = &self.moves[k as usize];
                Entry {
                    players: vec![player.clone()],
                    count: 1,
                    tags: vec![
                        (SCORE_PARENT_TAG.to_string(), format!("{:016x}", book_move.parent)),
                        (SCORE_MOVE_TAG.to_string(), book_move.book_move.to_string()),
                        (SCORE_FEN_TAG.to_string(), book_move.fen.clone()),
                    ],
                }
            })
            .collect();
        (entries, ks)
    }

    /// Gives `ks` back to be dealt again, those not scored meanwhile.
    fn give_back(&mut self, ks: &[u32]) {
        for &k in ks {
            if !self.done[k as usize] {
                self.free.push_back(k);
            }
        }
    }

    fn player(&self) -> Player {
        Player { network: self.network.clone(), parameters: Some(self.parameters), time: None }
    }
}

/// What the status page shows of a phase that `Work` does not say: set by the phase as it starts,
/// cleared as it ends.
#[derive(Clone, Default)]
enum View {
    #[default]
    None,
    /// The round robin: its lambdas, each pairing's two, and the pairs each pairing wants.
    RoundRobin { lambdas: Vec<f64>, players: Vec<(usize, usize)> },
    /// The gate, its SPRT at `elo_d`.
    Gate { elo_d: f64 },
    /// A test (`matches.rs`), its time from its record.
    Test { number: u32, spec: matches::Spec },
}

/// Where the page reads a forge's phases: its run directory, its config, and the rates' schedule its
/// lambdas' stages are replayed by.
#[derive(Clone)]
struct ForgeFiles {
    root: String,
    config: GenConfig,
    schedule: Schedule,
}

impl ForgeFiles {
    fn of(name: &str, config: &GenConfig, net: &NetConfig) -> ForgeFiles {
        let training = &net.training;
        let schedule = Schedule {
            stage_count: training.learning_rate.len(),
            steps_per_round: training.steps_per_round,
            warmup_rounds: training.warmup_rounds.expect("net_config::load already validated this is Some"),
            patience_rounds: training.patience_rounds.expect("net_config::load already validated this is Some"),
            rates: training.learning_rate.clone(),
        };
        ForgeFiles { root: run_dir(name), config: config.clone(), schedule }
    }
}

impl Shared {
    fn work(&self) -> Work {
        self.work.clone().unwrap_or(Work::Idle)
    }

    /// The depth the work's games are searched at.
    fn depth(&self) -> u32 {
        self.forge_depth.unwrap_or(generation::SEARCH_DEPTH)
    }

    /// The work as the page draws it, its counts from memory.
    fn work_view(&self) -> WorkView {
        match (self.work(), &self.view) {
            (Work::SelfPlay { teacher, test_left, train_left, before, targets, .. }, _) => WorkView::SelfPlay {
                teacher,
                // The run's totals: what each set held when this play began, and this play's on top.
                test_got: before.0 + self.test_got,
                test_wanted: targets.0,
                train_got: before.1 + self.train_got,
                train_wanted: targets.1,
                // Each set counted up to its own target, the test set's last game overshooting it.
                time: Some(self_play_time(self.self_play_started, self.test_got.min(test_left) + self.train_got, test_left + train_left)),
            },
            (Work::Pairs { wanted, .. }, View::RoundRobin { lambdas, players, .. }) => {
                round_robin_view(lambdas, players, &self.pairs, wanted, &self.pair_times, self.depth())
            }
            (Work::Pairs { .. }, View::Gate { elo_d }) => {
                gate_view(*elo_d, self.pairs.first().map_or(&[][..], Vec::as_slice), &self.pair_times, self.depth())
            }
            (Work::Pairs { .. }, View::Test { number, spec }) => {
                test_view(
                    *number,
                    spec,
                    self.pairs.first().map_or(&[][..], Vec::as_slice),
                    self.pair_wdl.first().map_or(&[][..], Vec::as_slice),
                    &self.pair_times,
                )
            }
            (Work::Pairs { pairings, wanted, .. }, _) => {
                WorkView::Pairs { played: self.pairs.iter().map(Vec::len).sum(), pairings: pairings.len(), wanted }
            }
            (Work::Score, _) => match &self.score {
                Some(score) => score_view(score),
                None => WorkView::Idle,
            },
            // A finished tune is idle, and still shown.
            (work @ (Work::Tune { .. } | Work::Idle), _) => match &self.tune {
                Some(tune) => WorkView::Tune {
                    player: tune.player.clone(),
                    done: tune.k_done,
                    pairs: tune.pairs,
                    reference_nps: self.reference_nps,
                    theta: tune.rows().into_iter().map(|(name, start, now)| ParameterView { name, start, now }).collect(),
                    time: matches!(work, Work::Tune { .. })
                        .then(|| pairs_eta(&self.pair_times, u64::from(tune.pairs.saturating_sub(tune.k_done)))),
                },
                None => WorkView::Idle,
            },
        }
    }

    /// Why nothing is dealt, carried by `WAIT`: an arena says it once per reason, so a wait of hours
    /// between jobs reads as one line.
    fn wait_reason(&self) -> String {
        match (&self.job, self.work()) {
            (None, _) => "the smithy has no job".to_string(),
            (Some(job), _) if self.finished.is_some() => format!("{job} has ended"),
            (Some(job), Work::Idle) => format!("{job} is training, or between phases"),
            (Some(job), _) => format!("{job} wants nothing more for now"),
        }
    }

    /// Tasks of `pairing` dealt and not yet arrived, in every open chore, a pair's or a tournament's.
    fn outstanding(&self, pairing: usize) -> u64 {
        self
            .chores
            .values()
            .filter(|dealt| !dealt.closed && dealt.epoch == self.epoch)
            .map(|dealt| {
                let matchups: u64 =
                    dealt.matchups.iter().filter(|m| m.pairing == pairing).map(|m| m.dealt.saturating_sub(m.arrived)).sum();
                let own = if dealt.pairing == Some(pairing) { dealt.dealt.saturating_sub(dealt.arrived) } else { 0 };
                matchups + own
            })
            .sum()
    }

    /// Up to `count` tasks of a round robin spread over its `pairings`, one at a time to the pairing
    /// furthest behind (played and in flight), the first on a tie, none past what it wants: each
    /// pairing and its share, in pairing order, the shares not zero.
    fn spread(&self, pairings: usize, count: u64) -> Vec<(usize, u64)> {
        let mut behind: Vec<u64> = (0..pairings).map(|p| self.pairs.get(p).map_or(0, Vec::len) as u64 + self.outstanding(p)).collect();
        let mut room: Vec<u64> = (0..pairings).map(|p| self.wanted(Some(p))).collect();
        let mut shares = vec![0u64; pairings];
        for _ in 0..count {
            let Some(p) = (0..pairings).filter(|&p| room[p] > 0).min_by_key(|&p| behind[p]) else {
                break;
            };
            behind[p] += 1;
            room[p] -= 1;
            shares[p] += 1;
        }
        shares.into_iter().enumerate().filter(|&(_, share)| share > 0).collect()
    }

    /// `shares` of `pairings` as a tournament's entries, and counted into chore `id`'s matchups.
    fn tournament_entries(&mut self, id: &str, pairings: &[(String, String)], shares: &[(usize, u64)]) -> Vec<Entry> {
        let [first, second] = match self.work() {
            Work::Pairs { sides, .. } => sides.map(Some),
            _ => [Some(SearchParameters::DEFAULT); 2],
        };
        let dealt = self.chores.get_mut(id).expect("the chore is there");
        let mut entries = Vec::new();
        for &(pairing, share) in shares {
            let (challenger, teacher) = pairings[pairing].clone();
            let players = vec![
                Player { network: challenger, parameters: first, time: None },
                Player { network: teacher, parameters: second, time: None },
            ];
            match dealt.matchups.iter_mut().find(|m| m.pairing == pairing) {
                Some(matchup) => matchup.dealt += share,
                None => dealt.matchups.push(Matchup { pairing, players: players.clone(), dealt: share, arrived: 0 }),
            }
            dealt.dealt += share;
            entries.push(Entry { players, count: share as u32, tags: Vec::new() });
        }
        entries
    }

    /// How many more tasks the work wants of `pairing` (`None` for self-play), beyond those dealt.
    fn wanted(&self, pairing: Option<usize>) -> u64 {
        match (self.work(), pairing) {
            (Work::SelfPlay { test_left, train_left, .. }, None) => {
                // Counted in positions, not tasks: wanted while short.
                u64::from(self.test_got + self.train_got < test_left + train_left) * u64::MAX
            }
            (Work::Pairs { wanted, .. }, Some(p)) => {
                let done = self.pairs.get(p).map_or(0, Vec::len) as u64;
                u64::from(wanted).saturating_sub(done + self.outstanding(p))
            }
            (Work::Tune { .. }, None) => u64::from(self.tune.as_ref().is_some_and(Tune::has_k)) * u64::MAX,
            (Work::Score, None) => self.score.as_ref().map_or(0, |score| score.free.len() as u64),
            _ => 0,
        }
    }

    /// The next chore's id, its counter persisted.
    fn next_chore_id(&mut self) -> String {
        let id = format!("c{:06}", self.next_chore);
        self.next_chore += 1;
        if let Some(path) = &self.chores_path
            && let Err(message) = lan::write_whole(path, &format!("{}\n", self.next_chore)) {
                log::warn!("{message}");
            }
        id
    }

    /// Up to `workers * KS_PER_WORKER` of the tune's next `k`s, as entries, and the `k`s.
    fn tune_entries(&mut self, network: &str, workers: usize) -> (Vec<Entry>, Vec<u32>) {
        let Some(tune) = self.tune.as_mut() else {
            return (Vec::new(), Vec::new());
        };
        let ks: Vec<u32> = std::iter::from_fn(|| tune.take_k()).take(KS_PER_WORKER as usize * workers.max(1)).collect();
        (ks.iter().map(|&k| tune.entry(k, network)).collect(), ks)
    }

    /// A tune's chore: the next `k`s, each its own entry; `None` when none is free.
    fn deal_tune(&mut self, session: &str, workers: usize, network: &str) -> Option<Chore> {
        let (entries, ks) = self.tune_entries(network, workers);
        if ks.is_empty() {
            return None;
        }
        let tune = self.tune.as_ref().expect("a tune deals");
        let limit = tune.limit();
        let id = self.next_chore_id();
        let reference_nps = if matches!(limit, Limit::Clock(_)) { self.reference_nps } else { None };
        let chore = Chore { id: id.clone(), kind: TaskKind::Spsa, limit, adjudicate: true, reference_nps, entries };
        let players = vec![Player { network: network.to_string(), parameters: None, time: None }; 2];
        let dealt = Dealt {
            session: session.to_string(),
            epoch: self.epoch,
            kind: TaskKind::Spsa,
            players,
            pairing: None,
            matchups: Vec::new(),
            dealt: ks.len() as u64,
            arrived: 0,
            closed: false,
            ks,
        };
        self.chores.insert(id.clone(), dealt);
        if let Some(session) = self.sessions.get_mut(session) {
            session.chore = Some(id);
        }
        Some(chore)
    }

    /// Ends chore `id`: nothing more of it is wanted, and its `k`s not yet arrived go back to the tune,
    /// a score chore's moves to the scoring.
    fn close(&mut self, id: &str) {
        if let Some(dealt) = self.chores.get_mut(id) {
            dealt.closed = true;
            let ks = std::mem::take(&mut dealt.ks);
            if dealt.kind == TaskKind::Score {
                if let Some(score) = self.score.as_mut().filter(|_| dealt.epoch == self.epoch) {
                    score.give_back(&ks);
                }
            } else if let Some(tune) = self.tune.as_mut() {
                tune.return_ks(&ks);
            }
        }
    }

    /// A book's scoring's chore: up to `SCORE_MOVES_PER_WORKER` free moves a worker, each its own
    /// entry; `None` when none is free, or the session was refused.
    fn deal_score(&mut self, session: &str, workers: usize) -> Option<Chore> {
        let score = self.score.as_mut()?;
        if score.refused.iter().any(|(refused, _)| refused == session) {
            return None;
        }
        let player = score.player();
        let (entries, ks) = score.entries(&player, SCORE_MOVES_PER_WORKER * workers.max(1));
        if ks.is_empty() {
            return None;
        }
        let depth = score.depth;
        let id = self.next_chore_id();
        let chore = Chore { id: id.clone(), kind: TaskKind::Score, limit: Limit::Depth(depth), adjudicate: false, reference_nps: None, entries };
        let dealt = Dealt {
            session: session.to_string(),
            epoch: self.epoch,
            kind: TaskKind::Score,
            players: vec![player],
            pairing: None,
            matchups: Vec::new(),
            dealt: ks.len() as u64,
            arrived: 0,
            closed: false,
            ks,
        };
        self.chores.insert(id.clone(), dealt);
        if let Some(session) = self.sessions.get_mut(session) {
            session.chore = Some(id);
        }
        Some(chore)
    }

    /// Takes one score record, or says why not, as `take` a game's: checked against the scoring
    /// (its move one its chore holds, the depth, the arena's commit and bench node count), then its
    /// row appended. A record from an arena whose search differs refuses its session for good: its
    /// chore closed, its moves dealt elsewhere, `STOP` sent.
    fn take_score(&mut self, session: &str, record: ScoreRecord, workers: usize, now: Instant) -> Reply {
        if record.session != session {
            return Reply::error(400, "a score record of another session");
        }
        let Some(dealt) = self.chores.get(&record.chore).filter(|dealt| dealt.session == session && dealt.kind == TaskKind::Score) else {
            return Reply::error(400, &format!("score chore {} was never dealt to {session}", record.chore));
        };
        let key = (record.chore.clone(), session.to_string(), record.seq.to_string());
        if dealt.epoch != self.epoch || self.seen.contains(&key) {
            return reply("OK", b"");
        }
        let Some(score) = self.score.as_mut() else {
            return reply("OK", b"");
        };
        let held = dealt.ks.iter().copied().find(|&k| {
            let book_move = &score.moves[k as usize];
            format!("{:016x}", book_move.parent) == record.parent && book_move.book_move == record.book_move
        });
        let Some(k) = held else {
            return Reply::error(400, &format!("chore {} holds no move {} of {}", record.chore, record.book_move, record.parent));
        };
        let machine = self.sessions.get(session).map_or("unknown".to_string(), |s| s.hello.machine.clone());
        let mismatch = if record.commit != score.commit {
            Some(format!("its commit {} is not the scoring's {}", record.commit, score.commit))
        } else if record.bench_nodes != score.bench_nodes {
            Some(format!("its bench searched {} nodes, the smithy's {}: another search", record.bench_nodes, score.bench_nodes))
        // A shallower depth is the node cap's (`SCORE_MAX_NODES`), the same on every arena.
        } else if record.depth > score.depth || record.fen != score.moves[k as usize].fen {
            Some(format!("it searched depth {} of {:?}, not what was dealt", record.depth, record.fen))
        } else {
            None
        };
        if let Some(why) = mismatch {
            score.rejected += 1;
            log::warn!("{machine}'s score records refused for this scoring: {why}");
            score.refused.push((session.to_string(), format!("{machine}: {why}")));
            self.close(&record.chore);
            if let Some(s) = self.sessions.get_mut(session) {
                s.chore = None;
            }
            return reply("STOP", b"");
        }
        self.seen.insert(key);
        let book_move = &score.moves[k as usize];
        let row = book_scores::Row {
            parent: book_move.parent,
            book_move: book_move.book_move,
            child: book_move.child,
            score: record.score,
            depth: record.depth,
            nodes: record.nodes,
        };
        if let Err(message) = score.file.append(&row) {
            // The smithy's own disk failing: said, and the move dealt again.
            log::warn!("a score not recorded: {message}");
            score.free.push_back(k);
        } else if !score.done[k as usize] {
            score.done[k as usize] = true;
            score.done_count += 1;
        }
        let dealt = self.chores.get_mut(&record.chore).expect("the chore is there");
        dealt.arrived += 1;
        dealt.ks.retain(|&held| held != k);
        if let Some(s) = self.sessions.get_mut(session) {
            s.took(now);
        }
        self.answer(&record.chore, workers)
    }

    /// A new chore for `session`, or `None` when the work wants nothing more.
    fn deal(&mut self, session: &str, workers: usize) -> Option<Chore> {
        let size = u64::from(TASKS_PER_WORKER) * workers.max(1) as u64;
        let (kind, limit, players, pairing, count) = match self.work() {
            Work::Idle => return None,
            Work::Tune { network } => return self.deal_tune(session, workers, &network),
            Work::Score => return self.deal_score(session, workers),
            Work::SelfPlay { teacher, parameters, .. } => {
                if self.wanted(None) == 0 {
                    return None;
                }
                (
                    TaskKind::SelfPlay,
                    Limit::Depth(self.depth()),
                    vec![Player { network: teacher, parameters: Some(parameters), time: None }],
                    None,
                    size,
                )
            }
            Work::Pairs { pairings, .. } if pairings.len() > 1 => return self.deal_tournament(session, size, &pairings),
            Work::Pairs { pairings, sides, .. } => {
                // A gate's or a test's one pairing, while it wants more.
                let pairing = (0..pairings.len())
                    .filter(|&p| self.wanted(Some(p)) > 0)
                    .min_by_key(|&p| self.pairs.get(p).map_or(0, Vec::len) as u64 + self.outstanding(p))?;
                let (challenger, teacher) = pairings[pairing].clone();
                // A test's sides play the parameters it gives them, under its limit; a gate's its
                // `sides`, at the forge's depth.
                // A test's sides also play their time tunes; a gate plays a depth, no clock.
                let (limit, [first, second], [first_time, second_time]) = match &self.test {
                    Some(test) => (test.limit.clone(), test.parameters.map(Some), test.time),
                    None => (Limit::Depth(self.depth()), sides.map(Some), [None; 2]),
                };
                let players = vec![
                    Player { network: challenger, parameters: first, time: first_time },
                    Player { network: teacher, parameters: second, time: second_time },
                ];
                (TaskKind::Pair, limit, players, Some(pairing), size.min(self.wanted(Some(pairing))))
            }
        };
        let id = self.next_chore_id();
        // Under a clock, the speed each arena scales it by: a test's (`test_job`), set beside its limit.
        let reference_nps = if matches!(limit, Limit::Clock(_)) { self.reference_nps } else { None };
        let chore = Chore {
            id: id.clone(),
            kind,
            limit,
            adjudicate: false,
            reference_nps,
            entries: vec![Entry { players: players.clone(), count: count as u32, tags: Vec::new() }],
        };
        let dealt =
            Dealt { session: session.to_string(), epoch: self.epoch, kind, players, pairing, matchups: Vec::new(), dealt: count, arrived: 0, closed: false, ks: Vec::new() };
        self.chores.insert(id.clone(), dealt);
        if let Some(session) = self.sessions.get_mut(session) {
            session.chore = Some(id);
        }
        Some(chore)
    }

    /// A round robin's chore: `size` tasks spread over the pairings furthest behind (`spread`), each
    /// pairing an entry the arena plays together with the others; `None` when none wants more.
    fn deal_tournament(&mut self, session: &str, size: u64, pairings: &[(String, String)]) -> Option<Chore> {
        let shares = self.spread(pairings.len(), size);
        if shares.is_empty() {
            return None;
        }
        let id = self.next_chore_id();
        let dealt = Dealt {
            session: session.to_string(),
            epoch: self.epoch,
            kind: TaskKind::Tournament,
            players: Vec::new(),
            pairing: None,
            matchups: Vec::new(),
            dealt: 0,
            arrived: 0,
            closed: false,
            ks: Vec::new(),
        };
        self.chores.insert(id.clone(), dealt);
        let entries = self.tournament_entries(&id, pairings, &shares);
        if let Some(session) = self.sessions.get_mut(session) {
            session.chore = Some(id.clone());
        }
        Some(Chore { id, kind: TaskKind::Tournament, limit: Limit::Depth(self.depth()), adjudicate: false, reference_nps: None, entries })
    }

    /// Ends `session`, and the chore its arena plays.
    fn drop_session(&mut self, session: &str) {
        if let Some(id) = self.sessions.remove(session).and_then(|gone| gone.chore) {
            self.close(&id);
        }
    }

    /// An arena lost, on `machine`: the chores of its sessions closed, which deals their work again.
    /// Its sessions stay, so a task it sends once back is still taken (`take`); a hello from it ends
    /// them.
    fn lose(&mut self, machine: &str) {
        let held: Vec<String> =
            self.sessions.values_mut().filter(|session| session.hello.machine == machine).filter_map(|session| session.chore.take()).collect();
        for id in held {
            self.close(&id);
        }
    }

    /// The answer to a task of chore `id`, the chore its session plays: more of it, a stop, or the
    /// next chore, as the work wants. `OK` when it needs nothing.
    fn answer(&mut self, id: &str, workers: usize) -> Reply {
        let Some(dealt) = self.chores.get(id) else {
            return reply("OK", b"");
        };
        let (session, pairing, players) = (dealt.session.clone(), dealt.pairing, dealt.players.clone());
        let current = self.sessions.get(&session).and_then(|s| s.chore.as_deref()) == Some(id);
        if dealt.closed || !current {
            return reply("OK", b"");
        }
        let stale = dealt.epoch != self.epoch;
        let left = dealt.dealt.saturating_sub(dealt.arrived).saturating_sub(workers as u64);
        if dealt.kind == TaskKind::Tournament {
            return self.answer_tournament(id, &session, workers, stale, left);
        }
        let wanted = if stale { 0 } else { self.wanted(pairing) };
        if wanted == 0 {
            // Done with this chore: done for good, or done with its pairing, when another pairing may
            // still want tasks. A pairing whose tasks are only in flight still waits on them.
            // A tune whose every `k` is out waits on them, as a pairing does.
            let complete = match (self.work(), pairing) {
                (Work::Pairs { wanted, .. }, Some(p)) if !stale => self.pairs.get(p).map_or(0, Vec::len) as u64 >= u64::from(wanted),
                (Work::Tune { .. }, _) if !stale => self.tune.as_ref().is_none_or(Tune::done),
                (Work::Score, _) if !stale => self.score.as_ref().is_none_or(ScoreDealing::complete),
                _ => true,
            };
            if !complete {
                return reply("OK", b"");
            }
            self.close(id);
            if let Some(session) = self.sessions.get_mut(&session) {
                session.chore = None;
            }
            return match self.deal(&session, workers) {
                Some(chore) => reply("CHORE", chore.text().as_bytes()),
                None => reply("STOP", b""),
            };
        }
        if left >= LOW_WATER_PER_WORKER * workers as u64 {
            return reply("OK", b"");
        }
        if let Work::Tune { network } = self.work() {
            let (entries, ks) = self.tune_entries(&network, workers);
            let dealt = self.chores.get_mut(id).expect("the chore is there");
            dealt.dealt += ks.len() as u64;
            dealt.ks.extend(ks);
            return reply("ADD", entries_text(&entries).as_bytes());
        }
        if let (Work::Score, Some(score)) = (self.work(), self.score.as_mut()) {
            let player = score.player();
            let (entries, ks) = score.entries(&player, SCORE_MOVES_PER_WORKER * workers.max(1));
            let dealt = self.chores.get_mut(id).expect("the chore is there");
            dealt.dealt += ks.len() as u64;
            dealt.ks.extend(ks);
            return reply("ADD", entries_text(&entries).as_bytes());
        }
        let count = (u64::from(TASKS_PER_WORKER) * workers.max(1) as u64).min(wanted);
        self.chores.get_mut(id).expect("the chore is there").dealt += count;
        reply("ADD", entries_text(&[Entry { players, count: count as u32, tags: Vec::new() }]).as_bytes())
    }

    /// `answer` for a tournament's chore `id`, `left` of its tasks still to play beyond one a worker:
    /// topped up over the pairings furthest behind while any wants more; once none does, done when
    /// every pairing is complete, a new chore or `STOP`, and kept while some are only in flight.
    fn answer_tournament(&mut self, id: &str, session: &str, workers: usize, stale: bool, left: u64) -> Reply {
        let pairings = match self.work() {
            Work::Pairs { pairings, .. } if !stale => pairings,
            _ => Vec::new(),
        };
        let wanted: u64 = (0..pairings.len()).map(|p| self.wanted(Some(p))).sum();
        if wanted == 0 {
            let complete = match self.work() {
                Work::Pairs { wanted, .. } if !stale => {
                    (0..pairings.len()).all(|p| self.pairs.get(p).map_or(0, Vec::len) as u64 >= u64::from(wanted))
                }
                _ => true,
            };
            if !complete {
                return reply("OK", b"");
            }
            self.close(id);
            if let Some(session) = self.sessions.get_mut(session) {
                session.chore = None;
            }
            return match self.deal(session, workers) {
                Some(chore) => reply("CHORE", chore.text().as_bytes()),
                None => reply("STOP", b""),
            };
        }
        if left >= LOW_WATER_PER_WORKER * workers as u64 {
            return reply("OK", b"");
        }
        let shares = self.spread(pairings.len(), u64::from(TASKS_PER_WORKER) * workers.max(1) as u64);
        let entries = self.tournament_entries(id, &pairings, &shares);
        reply("ADD", entries_text(&entries).as_bytes())
    }

    /// Takes one task's games, or says why not: an HTTP refusal for a task that is not what its chore
    /// dealt, `OK` and nothing recorded for one already taken or played for earlier work.
    fn take(&mut self, session: &str, text: &str, now: Instant) -> Reply {
        let Some(workers) = self.sessions.get_mut(session).map(|s| {
            s.last_seen = now;
            s.hello.workers
        }) else {
            return reply("UNKNOWN", b"");
        };
        // A score task sends its record, not a PGN.
        match ScoreRecord::parse(text) {
            Some(Ok(record)) => return self.take_score(session, record, workers, now),
            Some(Err(message)) => return Reply::error(400, &format!("a malformed score record: {message}")),
            None => {}
        }
        let games = match tasks::read(text) {
            Ok(games) => games,
            Err(message) => return Reply::error(400, &format!("a task that is not whole legal games: {message}")),
        };
        let identity = |game: &tasks::TaskGame| -> Option<(String, String, String)> {
            Some((game.tag(CHORE_TAG)?.to_string(), game.tag(SESSION_TAG)?.to_string(), game.tag(SEQ_TAG)?.to_string()))
        };
        let Some(key) = identity(&games[0]) else {
            return Reply::error(400, "a task without its Chore, Session and Seq tags");
        };
        if games.iter().any(|game| identity(game).as_ref() != Some(&key)) || key.1 != session {
            return Reply::error(400, "a task whose games disagree on their identity, or another session's");
        }
        let Some(dealt) = self.chores.get(&key.0).filter(|dealt| dealt.session == session) else {
            return Reply::error(400, &format!("chore {} was never dealt to {session}", key.0));
        };
        if games.len() != dealt.kind.games() {
            return Reply::error(400, &format!("{} games for a {}", games.len(), dealt.kind.name()));
        }
        // A tournament's task is its matchup's whose challenger is White in the first game.
        let matchup = match dealt.kind {
            TaskKind::Tournament => match dealt.matchups.iter().position(|m| games[0].tag("White") == Some(m.players[0].network.as_str())
                && games[0].tag("Black") == Some(m.players[1].network.as_str()))
            {
                Some(index) => Some(index),
                None => return Reply::error(400, "a task whose players are no matchup of its chore"),
            },
            _ => None,
        };
        let players = match matchup {
            Some(index) => &dealt.matchups[index].players,
            None => &dealt.players,
        };
        let names: Vec<&str> = players.iter().map(|player| player.network.as_str()).collect();
        let sides: Vec<[&str; 2]> = match dealt.kind {
            TaskKind::SelfPlay => vec![[names[0], names[0]]],
            TaskKind::Pair | TaskKind::Tournament | TaskKind::Spsa => vec![[names[0], names[1]], [names[1], names[0]]],
            TaskKind::Score => return Reply::error(400, "a score task sent as games"),
        };
        for (game, [white, black]) in games.iter().zip(&sides) {
            if game.tag("White") != Some(white) || game.tag("Black") != Some(black) {
                return Reply::error(400, "a task whose players are not its chore's");
            }
            if dealt.kind == TaskKind::SelfPlay && !game.scored {
                return Reply::error(400, "a self-play game with a move whose score is missing or malformed");
            }
        }
        // A tune's pair: one of the chore's `k`s, at a clock within the clamps.
        let k = match (&self.tune, dealt.epoch == self.epoch && matches!(self.work, Some(Work::Tune { .. }))) {
            (Some(tune), true) => {
                let k = games[0].tag(tune::K_TAG).and_then(|k| k.parse::<u32>().ok());
                if games.iter().any(|game| game.tag(tune::K_TAG).and_then(|k| k.parse::<u32>().ok()) != k) || k.is_none() {
                    return Reply::error(400, "a tune's pair without one K tag on both games");
                }
                // A `k` given back with a dropped session may be dealt again: only its holder's counts.
                if !k.is_some_and(|k| dealt.ks.contains(&k)) {
                    return Reply::error(400, &format!("chore {} holds no k {k:?}", key.0));
                }
                for game in &games {
                    if let Err(message) = tune.check_clock(game.tag("TimeControl")) {
                        return Reply::error(400, &message);
                    }
                }
                k
            }
            _ => None,
        };
        if !self.seen.insert(key.clone()) {
            return reply("OK", b"");
        }
        let (epoch, pairing) = (dealt.epoch, matchup.map_or(dealt.pairing, |index| Some(dealt.matchups[index].pairing)));
        let dealt = self.chores.get_mut(&key.0).expect("the chore is there");
        dealt.arrived += 1;
        if let Some(index) = matchup {
            dealt.matchups[index].arrived += 1;
        }
        if let Some(k) = k {
            dealt.ks.retain(|&held| held != k);
        }
        if let Some(s) = self.sessions.get_mut(session) {
            s.took(now);
        }
        if epoch == self.epoch
            && let Err(message) = self.record(&games, text, pairing, &key) {
                // The smithy's own disk failing: said, and the task answered as taken, which it is not.
                log::warn!("a task not recorded: {message}");
            }
        self.answer(&key.0, workers)
    }

    /// Writes a task of the current work to disk and counts it.
    fn record(&mut self, games: &[tasks::TaskGame], text: &str, pairing: Option<usize>, key: &(String, String, String)) -> Result<(), String> {
        match (self.work(), pairing) {
            (Work::SelfPlay { test_dir, test_left, train_dir, .. }, None) => {
                // The segment's `.meta` counts every position read; the targets, the searched ones,
                // all but the final position (`searched_in`).
                let positions = games[0].positions();
                let counted = positions - 1;
                let test = self.test_got < test_left;
                let (slot, dir) = if test { (&mut self.test_segment, test_dir) } else { (&mut self.train_segment, train_dir) };
                if slot.is_none() {
                    *slot = Some(SegmentWriter::open(&dir)?);
                }
                let writer = slot.as_mut().expect("just opened");
                writer.append(text, positions)?;
                if writer.positions >= SEGMENT_POSITIONS {
                    slot.take().expect("just written").close()?;
                }
                if test {
                    self.test_got += counted;
                } else {
                    self.train_got += counted;
                }
            }
            (Work::Pairs { record, wanted, .. }, Some(pairing)) => {
                // A pair past what its pairing wants is dropped, unrecorded: one played by an arena
                // lost and back, its chore's tasks dealt again meanwhile. Every pairing so ends at
                // `wanted` exactly, as the round robin's ranking and the gate's count assume.
                if self.pairs.get(pairing).map_or(0, Vec::len) as u64 >= u64::from(wanted) {
                    log::info!("a pair of pairing {pairing} past its {wanted}, from chore {} of {}: dropped", key.0, key.1);
                    return Ok(());
                }
                // The challenger is White in the first game and Black in the second.
                let score = (2.0 * games[0].result - 1.0) + (2.0 * (1.0 - games[1].result) - 1.0);
                let r = score.round() as i32;
                let wdl = matches::Wdl::of_pair(games[0].result, games[1].result);
                append(&Dirs::games_of(&record), text)?;
                // Its arrival, then its wins, losses and draws: the fields older records lack.
                let arrived = unix_now();
                append(&record, &format!("{pairing}\t{r}\t{}\t{}\t{}\t{arrived}\t{}\n", key.0, key.1, key.2, wdl.field()))?;
                if self.pairs.len() <= pairing {
                    self.pairs.resize(pairing + 1, Vec::new());
                }
                if self.pair_wdl.len() <= pairing {
                    self.pair_wdl.resize(pairing + 1, Vec::new());
                }
                self.pairs[pairing].push(r);
                self.pair_wdl[pairing].push(Some(wdl));
                self.pair_times.push((arrived, true));
            }
            (Work::Tune { .. }, None) => {
                let Some(k) = games[0].tag(tune::K_TAG).and_then(|k| k.parse::<u32>().ok()) else {
                    return Ok(());
                };
                // The plus side is White in the first game and Black in the second.
                let r = ((2.0 * games[0].result - 1.0) + (2.0 * (1.0 - games[1].result) - 1.0)).round() as i32;
                let machine = self.sessions.get(&key.1).map_or("unknown".to_string(), |session| session.hello.machine.clone());
                let tune = self.tune.as_mut().expect("a tune is dealt");
                append(&tune.games_path(), text)?;
                tune.record(k, r, &machine)?;
                // Its arrival, in memory alone: the tune's elapsed time, pace and ETA.
                self.pair_times.push((unix_now(), true));
            }
            _ => {}
        }
        Ok(())
    }

    /// Closes the open self-play files into segments.
    fn close_segments(&mut self) -> Result<(), String> {
        for writer in [self.test_segment.take(), self.train_segment.take()].into_iter().flatten() {
            writer.close()?;
        }
        Ok(())
    }
}

/// Appends `text` to `path` in one write.
fn append(path: &str, text: &str) -> Result<(), String> {
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).map_err(|error| format!("{path}: {error}"))?;
    file.write_all(text.as_bytes()).map_err(|error| format!("{path}: {error}"))
}

/// The self-play file open in one directory: whole games appended as they arrive, `<stamp>.pgn.open`,
/// then closed into `<stamp>.pgn.zst` and its `.meta`, which is all the readers take, and copied to
/// the remote at once when the directory is under `data/`.
struct SegmentWriter {
    path: String,
    positions: u64,
    games: u64,
}

const OPEN_SUFFIX: &str = ".pgn.open";

impl SegmentWriter {
    /// The file open in `dir`, resumed with its whole games (a last game cut short by a crash is
    /// dropped), or a new one.
    fn open(dir: &str) -> Result<SegmentWriter, String> {
        std::fs::create_dir_all(dir).map_err(|error| format!("{dir}: {error}"))?;
        let mut open: Vec<String> = std::fs::read_dir(dir)
            .map_err(|error| format!("{dir}: {error}"))?
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .filter(|name| name.ends_with(OPEN_SUFFIX))
            .collect();
        open.sort();
        // More than one only after a crash at a close: all but the last are closed now.
        let last = open.pop();
        for name in open {
            SegmentWriter::resume(&format!("{dir}/{name}"))?.close()?;
        }
        match last {
            Some(name) => SegmentWriter::resume(&format!("{dir}/{name}")),
            None => {
                let stamp = time::OffsetDateTime::now_utc().unix_timestamp();
                Ok(SegmentWriter { path: format!("{dir}/{stamp}{OPEN_SUFFIX}"), positions: 0, games: 0 })
            }
        }
    }

    /// The file at `path` as far as its games are whole, cut there when they are not.
    fn resume(path: &str) -> Result<SegmentWriter, String> {
        let text = std::fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?;
        let starts: Vec<usize> = text.match_indices("[Event ").map(|(at, _)| at).filter(|&at| at == 0 || text[..at].ends_with('\n')).collect();
        let (mut kept, mut positions, mut games) = (0, 0, 0);
        for (index, &start) in starts.iter().enumerate() {
            let end = starts.get(index + 1).copied().unwrap_or(text.len());
            match tasks::read(&text[start..end]) {
                Ok(read) if read.len() == 1 && read[0].scored && text[start..end].ends_with("\n\n") => {
                    positions += read[0].positions();
                    games += 1;
                    kept = end;
                }
                _ => break,
            }
        }
        if kept < text.len() {
            log::info!("{path}: a game cut short dropped, {games} whole games kept");
            lan::write_whole(path, &text[..kept])?;
        }
        Ok(SegmentWriter { path: path.to_string(), positions, games })
    }

    fn append(&mut self, text: &str, positions: u64) -> Result<(), String> {
        append(&self.path, text)?;
        self.positions += positions;
        self.games += 1;
        Ok(())
    }

    /// The segment: compressed, its `.meta` written, then renamed into place, and the open file gone.
    fn close(self) -> Result<(), String> {
        if self.games == 0 {
            std::fs::remove_file(&self.path).ok();
            return Ok(());
        }
        let text = std::fs::read(&self.path).map_err(|error| format!("{}: {error}", self.path))?;
        let segment = format!("{}.pgn.zst", self.path.strip_suffix(OPEN_SUFFIX).expect("an open file's name"));
        let compressed = zstd::encode_all(text.as_slice(), 3).map_err(|error| format!("{segment}: {error}"))?;
        let partial = format!("{segment}.partial");
        std::fs::write(&partial, compressed).map_err(|error| format!("{partial}: {error}"))?;
        std::fs::write(format!("{segment}{META_SUFFIX}"), SegmentMeta { positions: self.positions, games: self.games }.text())
            .map_err(|error| format!("{segment}{META_SUFFIX}: {error}"))?;
        std::fs::rename(&partial, &segment).map_err(|error| format!("{segment}: {error}"))?;
        std::fs::remove_file(&self.path).map_err(|error| format!("{}: {error}", self.path))?;
        let dir = Path::new(&segment).parent().map_or(String::new(), |dir| dir.to_string_lossy().replace('\\', "/"));
        if (dir == "data" || dir.starts_with("data/"))
            && let Err(message) = upload_segment(&segment, &dir, &remote::remote_path(&dir))
        {
            log::warn!("{segment} not copied to the remote, the next copy of the run takes it: {message}");
        }
        Ok(())
    }
}

/// Copies the closed `segment`, in `dir`, to `remote_dir` at once: its `.pgn.zst`, then its `.meta`,
/// so a reader never finds a `.meta` without its segment. An open file never goes: a copy of one
/// outlives its closing, and a resume elsewhere would close it over the full segment of its name.
fn upload_segment(segment: &str, dir: &str, remote_dir: &str) -> Result<(), String> {
    let name = Path::new(segment).file_name().map_or(String::new(), |name| name.to_string_lossy().into_owned());
    let meta = format!("{name}{META_SUFFIX}");
    // `copy_files` copies in name order, the segment before its `.meta`.
    remote::copy_files(dir, remote_dir, &|candidate: &str| candidate == name || candidate == meta).map_err(|error| error.to_string())
}

/// Removes every open self-play file from `remote_generation`'s `test/` and `train/`: copies older
/// builds left there, each stale beside its closed segment once the original closed.
fn remove_remote_open_files(remote_generation: &str) -> Result<usize, String> {
    let mut removed = 0;
    for set in ["test", "train"] {
        let dir = format!("{remote_generation}/{set}");
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(OPEN_SUFFIX) {
                std::fs::remove_file(entry.path()).map_err(|error| format!("{dir}/{name}: {error}"))?;
                removed += 1;
            }
        }
    }
    Ok(removed)
}

fn reply(status: &str, body: &[u8]) -> Reply {
    Reply::status(status, body)
}

/// The reply to one arena request: the whole protocol, no socket. The control paths are not here:
/// `control` answers them, on the loopback listener alone.
fn handle(shared: &Mutex<Shared>, token: &str, request: &Request, now: Instant) -> Reply {
    let body = request.body.as_slice();
    // Another machine's smithy taking a lease (`lease.rs`): no token, it says only which jobs run.
    if (request.method.as_str(), request.path.as_str()) == ("GET", JOBS_PATH) {
        return reply("OK", lease::jobs_text().as_bytes());
    }
    let endpoint = match (request.method.as_str(), request.path.as_str()) {
        ("POST", HELLO_PATH) => "HELLO",
        ("POST", CHORE_PATH) => "CHORE",
        ("POST", TASK_PATH) => "TASK",
        _ => return Reply::error(404, &format!("no {} {}", request.method, request.path)),
    };
    if endpoint != "HELLO" && request.token.as_deref() != Some(token) {
        return reply("TOKEN", b"");
    }
    let mut state = shared.lock().expect("the lock should not be poisoned");
    match endpoint {
        "HELLO" => {
            let Ok(hello) = Hello::parse(&String::from_utf8_lossy(body)) else {
                return Reply::error(400, "a malformed hello");
            };
            // A machine saying hello again is a restarted arena, not a second one: its old session
            // goes, and its chore with it. Its tasks in play are never counted.
            let old: Vec<String> = state.sessions.iter().filter(|(_, s)| s.hello.machine == hello.machine).map(|(id, _)| id.clone()).collect();
            for id in &old {
                state.drop_session(id);
            }
            let id = format!("{}-{}", hello.machine, state.next_session);
            state.next_session += 1;
            log::info!("arena {} {}, {} workers", hello.machine, if old.is_empty() { "joined" } else { "rejoined" }, hello.workers);
            state.sessions.insert(id.clone(), Session { hello, chore: None, last_seen: now, joined: now, last_task: None, taken: 0, played: Duration::ZERO });
            let store = remote::this_root().to_string();
            reply("OK", HelloReply { session: id, token: token.to_string(), store }.text().as_bytes())
        }
        "CHORE" => {
            let session = String::from_utf8_lossy(body).trim().to_string();
            let Some(workers) = state.sessions.get_mut(&session).map(|s| {
                s.last_seen = now;
                s.hello.workers
            }) else {
                return reply("UNKNOWN", b"");
            };
            // An arena asks only once its chore is played out: whatever it held is over.
            if let Some(held) = state.sessions.get_mut(&session).and_then(|s| s.chore.take()) {
                state.close(&held);
            }
            match state.deal(&session, workers) {
                Some(chore) => reply("CHORE", chore.text().as_bytes()),
                None => reply("WAIT", state.wait_reason().as_bytes()),
            }
        }
        _ => {
            let Ok((session, pgn)) = first_line_bytes(body) else {
                return Reply::error(400, "a task without its session line");
            };
            let Ok(pgn) = std::str::from_utf8(pgn) else {
                return Reply::error(400, "a task that is not text");
            };
            state.take(session, pgn, now)
        }
    }
}

/// The page's job controls, on the loopback listener alone: a job to start, its words one per line
/// (`job_name`), and a stop of the job, the smithy staying up.
const JOB_PATH: &str = "/control/job";
const JOB_STOP_PATH: &str = "/control/job/stop";
/// What the loopback listener alone answers beside the arenas' requests: `GET /control/status`, the
/// job as it stands; `POST /control/stop`, the first Ctrl+C's stop of the job and the smithy; and the
/// page's job controls. These carry `Status::control_token`, which only the loopback page
/// is given: another site open in the same browser can neither read it nor send the header without it.
fn control(status: &Status, request: &Request) -> Option<Reply> {
    let from_page = request.token.as_deref() == Some(status.control_token.as_str());
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", STATUS_PATH) => Some(reply("OK", status.text().as_bytes())),
        ("POST", STOP_PATH) => {
            status.job_stop.store(true, Ordering::SeqCst);
            stop_slot(&status.slot);
            if !status.stop.swap(true, Ordering::SeqCst) {
                log::info!("stop asked by smithy stop; stopping the job at its next safe point, then the smithy");
            }
            Some(reply("OK", b""))
        }
        ("POST", JOB_PATH | JOB_STOP_PATH) if !from_page => Some(Reply::error(403, "only the page opened on this machine starts or stops a job")),
        ("POST", JOB_PATH) => {
            let args: Vec<String> = String::from_utf8_lossy(&request.body).lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_string).collect();
            if let Err(message) = job_name(&args) {
                return Some(Reply::error(400, &message));
            }
            // Two slots: the source slot's, Convert and the book jobs, and the arena job's for every
            // other job.
            if let Some(job) = SlotJob::of(&args) {
                return Some(start_in_slot(&status.slot, job));
            }
            if status.running.swap(true, Ordering::SeqCst) {
                return Some(Reply::error(409, "a job is running: stop it first"));
            }
            *status.requested.lock().expect("the lock should not be poisoned") = Some(args);
            Some(reply("OK", b""))
        }
        // The body names the config whose job to stop: lc0's and the books' are the source slot's,
        // any other the arena job.
        ("POST", JOB_STOP_PATH) if matches!(String::from_utf8_lossy(&request.body).trim(), "lc0" | BOOKS_SCREEN) => {
            stop_slot(&status.slot);
            Some(reply("OK", b""))
        }
        ("POST", JOB_STOP_PATH) => {
            if status.running.load(Ordering::SeqCst) && !status.job_stop.swap(true, Ordering::SeqCst) {
                log::info!("stop asked from the page; stopping the job at its next safe point");
            }
            Some(reply("OK", b""))
        }
        _ => None,
    }
}

/// Everything the page lists, read from the files: every forge config with every run of it, each
/// run's generations, and the tunes. Internal to the page, as `Snapshot`.
#[derive(Serialize)]
struct Catalog {
    configs: Vec<ConfigEntry>,
    /// Pesto's tune, always offered.
    pesto: PlayerEntry,
    /// The sources, lc0 and pesto, each its corpus's state.
    sources: Vec<SourceEntry>,
    /// Every player a test may name: PeSTO, then each run's networks, by config, run and generation.
    networks: Vec<NetworkEntry>,
    /// The tests logged, oldest first (`matches::read_log`).
    tests: Vec<KeptTest>,
    /// The books' sources on the remote, by name, each as far as its stages have gone.
    books: Vec<book::SourceView>,
}

/// A test logged, with its block as it was drawn while it played.
#[derive(Serialize)]
struct KeptTest {
    #[serde(flatten)]
    record: matches::Record,
    view: Option<WorkView>,
}

#[derive(Serialize)]
struct NetworkEntry {
    /// Its player name (`players::network_player_name`), or `pesto`.
    name: String,
    /// Its run, `None` for PeSTO, and its generation, which the page groups the networks by.
    run: Option<String>,
    generation: Option<u32>,
    /// Whether it is the generation's winner, rather than its candidate at one lambda.
    winner: bool,
    /// Its lambda: a candidate's, as its name says, or the winner's, which its name leaves out and
    /// the page shows beside it. `None` for PeSTO.
    lambda: Option<ArmLambda>,
    /// Whether it has a search tune, so may play `@search-tuned`.
    tuned: bool,
    /// Whether it also has a time tune, so may play `@search-time-tuned`.
    time_tuned: bool,
}

/// Every network a test may name, PeSTO first: in each run's generations, the winner of each
/// generation that passed its gate, then the generation's candidates, by lambda.
fn test_networks(runs: &[(String, Vec<u32>, Vec<u32>)]) -> Vec<NetworkEntry> {
    let tuned = |name: &str| {
        players::FattoriniPlayer::resolve(name)
            .and_then(|player| player.own_tuning())
            .is_ok_and(|tuning| matches!(tuning, Tuning::Parameters(_)))
    };
    let time_tuned = |name: &str| {
        tuned(name) && players::FattoriniPlayer::resolve(name).and_then(|player| player.own_time_tuning()).is_ok_and(|time| time.is_some())
    };
    let pesto = NetworkEntry {
        name: PESTO.to_string(),
        run: None,
        generation: None,
        winner: false,
        lambda: None,
        tuned: tuned(PESTO),
        time_tuned: time_tuned(PESTO),
    };
    let mut networks = vec![pesto];
    for (run, generations, winners) in runs {
        for &generation in generations {
            let entry = |name: String, winner: bool, lambda: ArmLambda| NetworkEntry {
                tuned: tuned(&name),
                time_tuned: time_tuned(&name),
                name,
                run: Some(run.clone()),
                generation: Some(generation),
                winner,
                lambda: Some(lambda),
            };
            if winners.contains(&generation)
                && let Ok(winner) = players::run_winner(&run_dir(run), generation) {
                    networks.push(entry(players::network_player_name(run, generation, None), true, winner));
                }
            let dirs = readable_dirs(run, generation);
            for arm in generation_arms(&dirs, generation) {
                if Path::new(&generation::candidate_network_path(&dirs.arm(generation, arm))).is_file() {
                    networks.push(entry(players::network_player_name(run, generation, Some(arm)), false, arm));
                }
            }
        }
    }
    networks
}

#[derive(Serialize)]
struct SourceEntry {
    name: &'static str,
    /// Whether the corpus its job fills is complete, which disables that job's button.
    complete: bool,
    /// One line on its corpus: what it holds, or what it is to hold.
    summary: String,
    /// Lc0's bytes the cap counts and the cap (`lc0::cap_bytes`), `[done, total]`; none for pesto.
    cap: Option<(u64, u64)>,
}

#[derive(Serialize)]
struct ConfigEntry {
    name: String,
    /// The source its generation 1 trains on, `lc0` or `pesto`, which the page groups its box under.
    source: Option<&'static str>,
    runs: Vec<RunEntry>,
}

#[derive(Serialize)]
struct RunEntry {
    number: u32,
    /// Its name (`players::run_name`), which its players start with.
    name: String,
    /// Where its state stands; none for a run created and never started.
    generation: Option<u32>,
    phase: Option<&'static str>,
    finished: bool,
    /// A scaled run's `s` (`GenConfig::scale`), which the page marks it with.
    scale: Option<u64>,
    generations: Vec<GenerationEntry>,
}

#[derive(Serialize)]
struct GenerationEntry {
    generation: u32,
    /// Whether a challenger of it passed its gate: its winner then has a player name, and a tune
    /// once the run is finished.
    passed: bool,
    player: PlayerEntry,
}

#[derive(Serialize)]
struct PlayerEntry {
    name: String,
    /// Its search tune's pairs done and pairs in all; none before it starts.
    tune: Option<(u32, u32)>,
    /// Its time tune's, likewise.
    time_tune: Option<(u32, u32)>,
}

/// The names of `configs/`' files, sorted.
fn config_names() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir("configs")
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok()?.strip_suffix(".yaml").map(str::to_string))
        .collect();
    names.sort();
    names
}

/// Whether `name` is a forge config in `configs/`: one that reads as one, named for its source
/// (`check_forge_name`), never named as a run.
fn is_forge_config(name: &str) -> bool {
    !players::is_run_suffixed(name)
        && load_config(&preprocessing_config::net_config_path(name)).is_ok_and(|(config, _)| check_forge_name(name, config.source).is_ok())
}

/// A run's state, `None` before it starts or when it cannot be read.
fn run_state(dirs: &Dirs) -> Option<GenState> {
    std::fs::read_to_string(dirs.state()).ok().and_then(|text| GenState::parse(&text).ok())
}

/// Whether run `run`, at `state`, has ended: done, or stopped by a failed gate. A run whose config
/// cannot be read counts as ended, never resumed; one never started has not.
fn run_finished(run: &str, state: Option<&GenState>) -> bool {
    match (state, load_run_config(run)) {
        (Some(state), Ok(_)) => matches!(state.phase, Phase::Done | Phase::Stopped),
        (Some(_), Err(_)) => true,
        (None, _) => false,
    }
}

fn player_entry(name: String) -> PlayerEntry {
    let tune = tune::progress(&name, TuneKind::Search);
    let time_tune = tune::progress(&name, TuneKind::Time);
    PlayerEntry { name, tune, time_tune }
}

fn catalog() -> Catalog {
    let names = config_names();
    let configs: Vec<ConfigEntry> = names
        .iter()
        .filter(|name| is_forge_config(name))
        .map(|name| {
            // Every run here and on the remote: one started in another smithy is resumed here too.
            let runs = known_runs(name)
                .into_iter()
                .map(|number| {
                    let run = players::run_name(name, number);
                    let dirs = Dirs { root: run_dir(&run) };
                    let state = run_state(&dirs).or_else(|| run_state(&Dirs { root: remote_run_dir(&run) }));
                    let winners = run_winners(&run_home(&run));
                    let last = state.map_or(0, |state| state.generation);
                    let generations = (1..=last)
                        .filter_map(|generation| {
                            // A generation shows once it has trained something: a lambda's directory.
                            let trained = !generation_arms(&readable_dirs(&run, generation), generation).is_empty();
                            let player = players::network_player_name(&run, generation, None);
                            trained.then(|| GenerationEntry {
                                generation,
                                passed: winners.contains(&generation),
                                player: player_entry(player),
                            })
                        })
                        .collect();
                    RunEntry {
                        number,
                        finished: run_finished(&run, state.as_ref()),
                        scale: load_run_config(&run).ok().and_then(|(config, _)| config.scale),
                        name: run,
                        generation: state.map(|state| state.generation),
                        phase: state.map(|state| state.phase.name()),
                        generations,
                    }
                })
                .collect();
            let source = load_config(&preprocessing_config::net_config_path(name)).ok().and_then(|(config, _)| config.source).map(Source::name);
            ConfigEntry { name: name.clone(), source, runs }
        })
        .collect();
    let sources = vec![
        SourceEntry { name: Source::Lc0.name(), complete: lc0::is_complete(), summary: lc0::summary(), cap: lc0::cap_bytes() },
        SourceEntry { name: Source::Pesto.name(), complete: Source::Pesto.is_complete(), summary: pesto::summary(), cap: None },
    ];
    let runs: Vec<(String, Vec<u32>, Vec<u32>)> = configs
        .iter()
        .flat_map(|config| config.runs.iter())
        // A scaled run's networks play in their run alone (`network_scale`).
        .filter(|run| run.scale.is_none())
        .map(|run| {
            let generations = run.generations.iter().map(|entry| entry.generation).collect();
            let winners = run.generations.iter().filter(|entry| entry.passed).map(|entry| entry.generation).collect();
            (run.name.clone(), generations, winners)
        })
        .collect();
    let networks = test_networks(&runs);
    let dir = matches::TESTS_DIR;
    let tests = matches::read_log(dir)
        .into_iter()
        .map(|mut record| {
            // Its live block once more, rebuilt from its record, so an ended test keeps it.
            let path = matches::pairs_path(dir, record.number);
            let view = match (record.spec(), recorded_pairs(&path, 1)) {
                (Ok(spec), Ok(mut pairs)) => {
                    let pairs = pairs.remove(0);
                    // An interrupted line holds the counts of its start: its pairs say what was played.
                    if record.verdict == matches::INTERRUPTED {
                        let judged = matches::judge(spec.kind, &pairs);
                        record.games = judged.games;
                        record.challenger_points = judged.challenger_points;
                    }
                    let wdl = recorded_wdl(&path, 1).remove(0);
                    Some(test_view(record.number, &spec, &pairs, &wdl, &recorded_times(&path)))
                }
                _ => None,
            };
            KeptTest { record, view }
        })
        .collect();
    Catalog { configs, pesto: player_entry(PESTO.to_string()), sources, networks, tests, books: book::sources() }
}

/// `player`'s tune, as its config's `tune:` says: pesto's `pesto::CONFIG_PATH`, a network player's
/// its run's frozen config, every one from the default parameters. Refused unless `player` is pesto, or the winner of a generation whose challenger passed its gate,
/// in a run that has ended.
/// The scale of the run whose network `player` names (`GenConfig::scale`): none for a full run's,
/// PeSTO, an engine, or a run whose config cannot be read here.
fn network_scale(player: &str) -> Option<u64> {
    let parsed = players::parse_network_name(player).ok()?;
    load_run_config(&parsed.run()).ok()?.0.scale
}

/// `player`, unless it is a scaled run's network: one is never a side of a test, its
/// strength meaning nothing outside its run, whose round robin and gate alone play it.
fn unscaled(player: &str) -> Result<(), String> {
    match network_scale(player) {
        Some(s) => Err(format!("{player} is a scaled run's network (1/{s}): it plays in its own run alone")),
        None => Ok(()),
    }
}


fn tune_args(player: &str, kind: TuneKind) -> Result<tune::TuneArgs, String> {
    let mut args = search_tune_args(player)?;
    if kind == TuneKind::Time {
        // After the search's (the developer, 2026-11-13): both sides play the finished search tune,
        // refused while there is none; the clock from the time defaults.
        args.kind = TuneKind::Time;
        args.start = None;
        args.search = Some(required_tune(player, &format!("{player}'s time tune plays"))?);
    }
    Ok(args)
}

/// The words of a tune job of `kind` of `player`: `--tune` for its search, `--time-tune` for its
/// time management.
fn tune_words(player: &str, kind: TuneKind) -> Vec<String> {
    let mode = match kind {
        TuneKind::Search => TUNE,
        TuneKind::Time => TIME_TUNE,
    };
    vec![mode.to_string(), player.to_string()]
}

/// The kind a tune job's word asks for, `None` for any other word.
fn tune_kind(mode: &str) -> Option<TuneKind> {
    match mode {
        TUNE => Some(TuneKind::Search),
        TIME_TUNE => Some(TuneKind::Time),
        _ => None,
    }
}

/// A search tune job's word.
const TUNE: &str = "--tune";
/// A time tune job's word: the time management's SPSA, after the search's.
const TIME_TUNE: &str = "--time-tune";

/// `player`'s search tune, as its config's `tune:` says (`tune_args`).
fn search_tune_args(player: &str) -> Result<tune::TuneArgs, String> {
    if player == PESTO {
        return tune::args_of(&pesto::load()?.tune, player).map_err(|message| format!("{}: {message}", pesto::CONFIG_PATH));
    }
    let parsed = players::parse_network_name(player)?;
    let (run, generation) = (parsed.run(), parsed.generation);
    if parsed.candidate.is_some() {
        return Err(format!(
            "{player} is one lambda's candidate: a tune takes a generation's winner, {}",
            players::network_player_name(&run, generation, None)
        ));
    }
    if !known_runs(&parsed.config).contains(&parsed.run_number) {
        return Err(format!("{run} is no forge run, here or on the remote"));
    }
    // Any winner that passed its gate, started by hand, the run ended or waiting (the developer,
    // 2026-11-02): one job at a time already keeps a tune and its run's next generation apart.
    let winners = run_winners(&run_home(&run));
    if !winners.contains(&generation) {
        return Err(format!("generation {generation} of {run} has no challenger that passed its gate"));
    }
    let (config, _) = load_run_config(&run)?;
    let mut args = tune::args_of(&config.tune, player).map_err(|message| format!("{run}'s {RUN_CONFIG}: {message}"))?;
    // A scaled network's tune is scaled as its run, for debugging the tune alone.
    args.scale = config.scale;
    // It goes on from its teacher's tune (the developer, 2026-11-08), refused while the teacher has
    // none; an Lc0 run's generation 1, taught by no Fattorini player, from the defaults.
    args.start = match teacher_player(&run, generation, config.source) {
        Some(teacher) => Some(required_tune(&teacher, &format!("{player}'s tune starts from"))?),
        None => None,
    };
    Ok(args)
}

/// The generations of the run in `dirs` that have a winner, as its `generations.tsv` records them:
/// a generation that passed its gate.
fn run_winners(dirs: &Dirs) -> Vec<u32> {
    let Ok(text) = std::fs::read_to_string(dirs.log()) else {
        return Vec::new();
    };
    let mut generations: Vec<u32> = text
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            let passed = fields.get(4).is_some_and(|&passed| passed == "true");
            fields.first().and_then(|generation| generation.parse().ok()).filter(|_| passed)
        })
        .collect();
    generations.dedup();
    generations
}

/// What `smithy status` and the status page show: the job, a training run's state as it was last
/// written, what is dealt, and the arenas; and what the smithy and its page's controls share.
struct Status {
    shared: Arc<Mutex<Shared>>,
    /// The smithy's stop, raised by `smithy stop` or Ctrl+C, which stop the job first.
    stop: Arc<AtomicBool>,
    /// The running job's stop, raised also from the page; lowered as each job starts.
    job_stop: Arc<AtomicBool>,
    /// The job the page asked for, its words, taken by `start`'s loop.
    requested: Mutex<Option<Vec<String>>>,
    /// Set as a job is asked for, cleared as it ends: one job at a time.
    running: AtomicBool,
    /// What the page's job controls carry, handed to the loopback page alone.
    control_token: String,
    /// The status page's address, the LAN's when there is one.
    page: String,
    /// Each lambda's latest round, by its `history.tsv`'s path, read again only when the file changes.
    history: Mutex<HashMap<String, (std::time::SystemTime, Option<RoundRow>)>>,
    /// Lc0's Convert job or a book job, in their own slot beside the arena job's.
    slot: Arc<SourceSlot>,
    /// The arenas this smithy claims.
    claims: Claims,
}

/// How long a job deals nothing before its arenas are released: a pause between two chores of one
/// phase keeps them, a phase without games (training, a scatter) or no job lets them go to another
/// smithy.
const CLAIM_IDLE_GRACE: Duration = Duration::from_secs(60);

/// The arenas a smithy claims, only while its job deals games (the developer, 2026-10-29): another
/// smithy's job plays on them meanwhile. Across jobs, which say hello afresh.
struct Claims {
    /// Who this smithy is to its arenas; its `scaled` set from the job at each claim.
    claim: Claim,
    /// Since when the job has dealt nothing, `None` while it deals.
    idle_since: Mutex<Option<Instant>>,
    /// Where it claims them: the machines listed.
    targets: Vec<String>,
    /// The port the arenas are claimed on: `ARENA_PORT`, another in the tests.
    port: u16,
    /// The arenas held, by their target.
    held: Mutex<HashMap<String, Held>>,
    /// What each target last came to, said once a change, and shown on the page.
    notes: Mutex<HashMap<String, String>>,
    /// Each target whose claims fail: what the last failure was, the wait it now has, and when it is
    /// tried again.
    backoff: Mutex<HashMap<String, Backoff>>,
}

/// A target's failing claims, retried after waits that double (`CLAIM_BACKOFF_CAP`).
struct Backoff {
    failure: String,
    wait: Duration,
    next: Instant,
}

/// An arena held: what it said of itself when claimed, when it last answered a poll, and that answer.
struct Held {
    hello: Hello,
    answered: Instant,
    alive: Option<Alive>,
}

impl Claims {
    fn new(claim: Claim, targets: Vec<String>) -> Claims {
        Claims {
            claim,
            idle_since: Mutex::new(None),
            targets,
            port: ARENA_PORT,
            held: Mutex::new(HashMap::new()),
            notes: Mutex::new(HashMap::new()),
            backoff: Mutex::new(HashMap::new()),
        }
    }

    fn backoff(&self) -> std::sync::MutexGuard<'_, HashMap<String, Backoff>> {
        self.backoff.lock().expect("the lock should not be poisoned")
    }

    /// A claim of `target` failed at `now` with `failure`: said, and its next try put off, by
    /// `CLAIM_POLL` after a new failure, by twice the last wait, at most `CLAIM_BACKOFF_CAP`, after the
    /// same one again.
    fn failed(&self, target: &str, failure: String, now: Instant) {
        self.note(target, failure.clone());
        let mut backoff = self.backoff();
        let wait = match backoff.get(target) {
            Some(last) if last.failure == failure => (last.wait * 2).min(CLAIM_BACKOFF_CAP),
            _ => CLAIM_POLL,
        };
        backoff.insert(target.to_string(), Backoff { failure, wait, next: now + wait });
    }

    fn held(&self) -> std::sync::MutexGuard<'_, HashMap<String, Held>> {
        self.held.lock().expect("the lock should not be poisoned")
    }

    /// Says `text` of `target`, when it differs from what was last said of it.
    fn note(&self, target: &str, text: String) {
        let mut notes = self.notes.lock().expect("the lock should not be poisoned");
        if notes.get(target) != Some(&text) {
            log::info!("arena {target}: {text}");
            notes.insert(target.to_string(), text);
        }
    }

    fn post(&self, target: &str, path: &str, body: &[u8]) -> Result<(String, String), String> {
        let reply = lan_http::post(&lan_http::url(target, self.port, path), None, body, CLAIM_TIMEOUT)?;
        let (word, body) =
            lan_http::status_of(&reply).ok_or_else(|| format!("HTTP {}: {}", reply.code, String::from_utf8_lossy(&reply.body).trim()))?;
        Ok((word, String::from_utf8_lossy(&body).to_string()))
    }

    /// One round: each target held polled, each not held claimed while the job deals games. An arena
    /// unanswering for `RELEASE_AFTER`, or answering that it is no longer this smithy's, is lost: its
    /// chores in `shared` closed, and the target claimed again from the next round. A job dealing
    /// nothing for `CLAIM_IDLE_GRACE` releases every arena held.
    fn round(&self, shared: &Mutex<Shared>, now: Instant) {
        let (dealing, scaled) = {
            let shared = shared.lock().expect("the lock should not be poisoned");
            (!matches!(shared.work, None | Some(Work::Idle)), shared.scaled_job)
        };
        {
            let mut idle_since = self.idle_since.lock().expect("the lock should not be poisoned");
            if dealing {
                // Back to dealing: what failed while idle is tried at once.
                if idle_since.take().is_some() {
                    self.backoff().clear();
                }
            } else if now.saturating_duration_since(*idle_since.get_or_insert(now)) >= CLAIM_IDLE_GRACE {
                drop(idle_since);
                self.release_idle(shared);
            }
        }
        for target in &self.targets {
            let held = self.held().get(target).map(|held| held.hello.machine.clone());
            match held {
                Some(machine) => self.poll(shared, target, &machine, now),
                None if !dealing => {}
                None if self.backoff().get(target).is_some_and(|backoff| now < backoff.next) => {}
                None => self.claim_one(target, scaled, now),
            }
        }
    }

    /// Every arena held released, the job dealing nothing: its chores closed, each arena free for
    /// another smithy.
    fn release_idle(&self, shared: &Mutex<Shared>) {
        let released: Vec<(String, String)> =
            self.held().drain().map(|(target, arena)| (target, arena.hello.machine)).collect();
        for (target, machine) in released {
            let _ = self.post(&target, RELEASE_PATH, self.claim.boot.as_bytes());
            self.note(&target, format!("{machine} released: this smithy's job deals no games now"));
            shared.lock().expect("the lock should not be poisoned").lose(&machine);
        }
    }

    fn poll(&self, shared: &Mutex<Shared>, target: &str, machine: &str, now: Instant) {
        let answer = self.post(target, ALIVE_PATH, self.claim.boot.as_bytes());
        let lost = match answer {
            Ok((word, body)) if word == "OK" => {
                let text = self.held().get_mut(target).map(|held| {
                    held.answered = now;
                    held.alive = Alive::parse(&body).ok();
                    claimed_text(&held.hello)
                });
                if let Some(text) = text {
                    self.note(target, text);
                }
                false
            }
            Ok((word, _)) if word == "UNCLAIMED" => {
                self.note(target, format!("{machine} is no longer this smithy's: lost, claimed again"));
                true
            }
            _ => {
                let silent = self.held().get(target).map_or(Duration::ZERO, |held| now.saturating_duration_since(held.answered));
                if silent > RELEASE_AFTER {
                    self.note(target, format!("{machine} silent for {} s: lost, its work dealt again", silent.as_secs()));
                    true
                } else {
                    if silent > LATE_AFTER {
                        self.note(target, format!("{machine} late, silent for {} s", silent.as_secs()));
                    }
                    false
                }
            }
        };
        if lost {
            self.held().remove(target);
            shared.lock().expect("the lock should not be poisoned").lose(machine);
        }
    }

    fn claim_one(&self, target: &str, scaled: bool, now: Instant) {
        let claim = Claim { scaled, ..self.claim.clone() };
        match self.post(target, CLAIM_PATH, claim.text().as_bytes()) {
            Ok((word, body)) if word == "OK" => match Hello::parse(&body) {
                Ok(hello) => {
                    self.note(target, claimed_text(&hello));
                    self.backoff().remove(target);
                    self.held().insert(target.to_string(), Held { hello, answered: now, alive: None });
                }
                Err(message) => self.failed(target, format!("claimed, but its hello is unreadable: {message}"), now),
            },
            // The holder by its standing, `Claim::standing`.
            Ok((word, body)) if word == "CLAIMED" => self.failed(target, format!("held by {}", body.trim()), now),
            Ok((word, _)) => self.failed(target, format!("answered {word}, which this build does not know"), now),
            Err(_) => self.failed(target, "no arena answering".to_string(), now),
        }
    }

    /// Every arena held released, as the smithy stops: each idles at once, rather than after
    /// `RELEASE_AFTER`.
    fn release_all(&self) {
        let targets: Vec<String> = self.held().drain().map(|(target, _)| target).collect();
        for target in targets {
            let _ = self.post(&target, RELEASE_PATH, self.claim.boot.as_bytes());
        }
    }

    /// Each target, its machine and its state, for the page and `smithy status`.
    fn views(&self, now: Instant) -> Vec<MachineView> {
        let held = self.held();
        let notes = self.notes.lock().expect("the lock should not be poisoned");
        let backoff = self.backoff();
        self
            .targets
            .iter()
            .map(|target| match held.get(target) {
                Some(arena) => {
                    let silent = now.saturating_duration_since(arena.answered);
                    let standing = if silent > LATE_AFTER { format!("late, silent for {} s", silent.as_secs()) } else { "claimed".to_string() };
                    // Shown, never refused: while developing every tree is modified, and squashes
                    // rewrite hashes, so whether the code differs is the developer's to judge.
                    let commit = if arena.hello.commit == self.claim.commit {
                        String::new()
                    } else {
                        format!("; at {}, this smithy at {}", short_commit(&arena.hello.commit), short_commit(&self.claim.commit))
                    };
                    MachineView {
                        target: target.clone(),
                        machine: arena.hello.machine.clone(),
                        state: format!("{standing}{commit}"),
                        outbox: arena.alive.as_ref().map_or(0, |alive| alive.outbox),
                    }
                }
                None => {
                    let note = notes.get(target).cloned().unwrap_or_else(|| "not tried yet".to_string());
                    let retry = backoff.get(target).map_or(String::new(), |backoff| {
                        let wait = backoff.next.saturating_duration_since(now).as_secs();
                        format!("; tried again in {wait} s, waits doubling up to {} min", CLAIM_BACKOFF_CAP.as_secs() / 60)
                    });
                    MachineView {
                        target: target.clone(),
                        machine: String::new(),
                        state: format!("{note}{retry}"),
                        outbox: 0,
                    }
                }
            })
            .collect()
    }
}

/// `commit` (`protocol::current_commit`) with its hash cut to 12 characters, as the page shows
/// commits: `26ff1f838fd4 (modified)`.
fn short_commit(commit: &str) -> String {
    let (hash, rest) = commit.split_once(' ').map_or((commit, ""), |(hash, rest)| (hash, rest));
    let hash = hash.get(..12).unwrap_or(hash);
    if rest.is_empty() { hash.to_string() } else { format!("{hash} {rest}") }
}

/// What is said of an arena claimed, and again only once it is back from being late.
fn claimed_text(hello: &Hello) -> String {
    format!("{} claimed, {} workers", hello.machine, hello.workers)
}

/// Claims and polls the arenas every `CLAIM_POLL` until `stop`, then releases them.
fn keep_claims(status: &Status, stop: &AtomicBool) {
    while !stop.load(Ordering::SeqCst) {
        status.claims.round(&status.shared, Instant::now());
        let started = Instant::now();
        while started.elapsed() < CLAIM_POLL && !stop.load(Ordering::SeqCst) {
            std::thread::sleep(POLL);
        }
    }
    status.claims.release_all();
}

/// The source slot: Lc0's Convert job or a book job, one at a time. Neither uses the arenas nor the
/// GPU, so either runs beside the arena job; both stream large files through the remote, so they
/// exclude each other (the developer, 2026-10-26). Each is run
/// as this smithy's child process (`lc0::run`, `book::run`), its lines into the smithy's log; its
/// Stop kills the child, and a converter's curls with it.
#[derive(Default)]
struct SourceSlot {
    /// Set as a job is asked for, cleared as it ends: one at a time.
    running: AtomicBool,
    stopping: AtomicBool,
    /// The job, while one runs, for the page and a refusal.
    job: Mutex<Option<SlotJob>>,
    /// The child's process id, while it runs.
    child: Mutex<Option<u32>>,
    /// How the last one ended, for the page.
    finished: Mutex<Option<String>>,
}

/// A job of the source slot, from the words that ask for it.
#[derive(Clone, Debug, PartialEq)]
enum SlotJob {
    /// `--convert lc0`.
    Convert,
    /// `--book <source> [--weights points|counts|softmax ...]`: `rest`, the words after the source,
    /// as `book::parse_make_args` reads them.
    Book { source: String, rest: Vec<String> },
}

impl SlotJob {
    /// The slot's job `args` asks for, if any; `job_name` has refused any other words first.
    fn of(args: &[String]) -> Option<SlotJob> {
        match args {
            [mode, ..] if mode == CONVERT => Some(SlotJob::Convert),
            [mode, source, rest @ ..] if mode == BOOK => Some(SlotJob::Book { source: source.clone(), rest: rest.to_vec() }),
            _ => None,
        }
    }

    /// Which job it is, for the page: `convert` or `book`.
    fn kind(&self) -> &'static str {
        match self {
            SlotJob::Convert => "convert",
            SlotJob::Book { .. } => "book",
        }
    }

    /// Its name, as `job_name` gives it.
    fn name(&self) -> String {
        match self {
            SlotJob::Convert => "Lc0's conversion".to_string(),
            SlotJob::Book { source, rest } => {
                // Its words after `--weights` and their flags, `points` when unsaid.
                let weights: Vec<&str> = rest.iter().map(String::as_str).filter(|word| !word.starts_with("--")).collect();
                format!("the book of {source}, {}", if weights.is_empty() { "points".to_string() } else { weights.join(" ") })
            }
        }
    }

    /// The child's words after the smithy's own path.
    fn child_args(&self) -> Vec<String> {
        match self {
            SlotJob::Convert => vec![lc0::CHILD_COMMAND.to_string()],
            SlotJob::Book { source, rest } => {
                [book::CHILD_COMMAND.to_string(), source.clone()].into_iter().chain(rest.iter().cloned()).collect()
            }
        }
    }

    /// Keeps one of the child's progress records for the page, `None` once the child has ended; false
    /// when `json` is not one of its records.
    fn keep(&self, json: Option<&str>) -> bool {
        match (self, json) {
            (SlotJob::Convert, None) => lc0::keep(None),
            (SlotJob::Book { .. }, None) => book::keep(None),
            (SlotJob::Convert, Some(json)) => match serde_json::from_str::<lc0::Lc0Live>(json) {
                Ok(live) => lc0::keep(Some(live)),
                Err(_) => return false,
            },
            (SlotJob::Book { .. }, Some(json)) => match serde_json::from_str::<book::BookLive>(json) {
                Ok(live) => book::keep(Some(live)),
                Err(_) => return false,
            },
        }
        true
    }

    /// What starts a progress line of its child's.
    fn progress_prefix(&self) -> &'static str {
        match self {
            SlotJob::Convert => lc0::PROGRESS_LINE,
            SlotJob::Book { .. } => book::PROGRESS_LINE,
        }
    }

    /// The prefix of its child's other lines in the smithy's log.
    fn log_prefix(&self) -> &'static str {
        match self {
            SlotJob::Convert => "lc0",
            SlotJob::Book { .. } => "book",
        }
    }
}

/// Starts `job` in the source slot, refused while a job holds it, naming that job.
fn start_in_slot(slot: &Arc<SourceSlot>, job: SlotJob) -> Reply {
    if slot.running.swap(true, Ordering::SeqCst) {
        let holder = slot.job.lock().expect("the lock should not be poisoned").as_ref().map(SlotJob::name).unwrap_or_default();
        return Reply::error(409, &format!("{holder} is running beside the arena job: stop it first"));
    }
    slot.stopping.store(false, Ordering::SeqCst);
    *slot.finished.lock().expect("the lock should not be poisoned") = None;
    *slot.job.lock().expect("the lock should not be poisoned") = Some(job.clone());
    let slot = Arc::clone(slot);
    std::thread::spawn(move || {
        let name = job.name();
        log::info!("job {name} started, beside the arena job");
        let line = match run_in_child(&slot, &job) {
            Ok(message) => message,
            Err(message) => format!("failed: {message}"),
        };
        log::info!("job {name}: {line}");
        *slot.child.lock().expect("the lock should not be poisoned") = None;
        *slot.finished.lock().expect("the lock should not be poisoned") = Some(format!("{name}: {line}"));
        *slot.job.lock().expect("the lock should not be poisoned") = None;
        slot.running.store(false, Ordering::SeqCst);
    });
    reply("OK", b"")
}

/// Runs `job`'s child until it ends or is killed, its progress records kept for the page and every
/// other line it prints logged with its prefix.
fn run_in_child(slot: &SourceSlot, job: &SlotJob) -> Result<String, String> {
    // One smithy at a time converts into the corpus. Books are made whole and replaced whole, so need
    // none.
    let _lease = match job {
        SlotJob::Convert => {
            let path = remote::remote_path(&format!("{}/{}", lc0::LC0_ROOT, lease::FILE));
            Some(lease::take(&path, &job.name())?)
        }
        SlotJob::Book { .. } => None,
    };
    let exe = std::env::current_exe().map_err(|error| format!("this smithy's own path: {error}"))?;
    let mut child = std::process::Command::new(exe)
        .args(job.child_args())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to start {}: {error}", job.name()))?;
    *slot.child.lock().expect("the lock should not be poisoned") = Some(child.id());
    // A stop asked before the child had an id kills it now.
    if slot.stopping.load(Ordering::SeqCst) {
        kill_tree(child.id());
    }
    let forward = |stream: Option<Box<dyn std::io::Read + Send>>| {
        let job = job.clone();
        std::thread::spawn(move || {
            let Some(stream) = stream else { return };
            for line in std::io::BufRead::lines(std::io::BufReader::new(stream)).map_while(Result::ok) {
                // Its progress, for the page; any other line, to the log.
                let kept = line.strip_prefix(job.progress_prefix()).is_some_and(|json| job.keep(Some(json)));
                if !kept {
                    log::info!("{}: {line}", job.log_prefix());
                }
            }
        })
    };
    let out = forward(child.stdout.take().map(|stream| Box::new(stream) as Box<dyn std::io::Read + Send>));
    let err = forward(child.stderr.take().map(|stream| Box::new(stream) as Box<dyn std::io::Read + Send>));
    let status = child.wait().map_err(|error| format!("failed to wait for {}: {error}", job.name()));
    out.join().ok();
    err.join().ok();
    // Its last report would otherwise stay on the page.
    job.keep(None);
    let status = status?;
    if slot.stopping.load(Ordering::SeqCst) {
        return Ok(match job {
            SlotJob::Convert => "stopped; the tar in flight is converted again at the next Convert".to_string(),
            SlotJob::Book { .. } => "stopped; the next Make book goes on from the stage it was in".to_string(),
        });
    }
    if !status.success() {
        return Err(format!("it ended with {status}, as its lines above say"));
    }
    Ok(match job {
        SlotJob::Convert if lc0::is_complete() => format!("done: {} is complete", lc0::LC0_ROOT),
        _ => "done, as its lines above say".to_string(),
    })
}

/// Stops the source slot's job at once, when one runs: its child killed, a converter's curls with it.
fn stop_slot(slot: &SourceSlot) {
    if !slot.running.load(Ordering::SeqCst) || slot.stopping.swap(true, Ordering::SeqCst) {
        return;
    }
    let holder = slot.job.lock().expect("the lock should not be poisoned").as_ref().map(SlotJob::name).unwrap_or_default();
    log::info!("stop of {holder} asked; its pass in flight is abandoned");
    if let Some(pid) = *slot.child.lock().expect("the lock should not be poisoned") {
        kill_tree(pid);
    }
}

/// Kills process `pid` and every process it started, as Windows' `taskkill /T` does: the converter's
/// curls die with it.
fn kill_tree(pid: u32) {
    let killed = std::process::Command::new("taskkill.exe")
        .args(["/T", "/F", "/PID", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    if !killed.is_ok_and(|status| status.success()) {
        log::warn!("taskkill could not end the converter, process {pid}");
    }
}

/// The source slot as the page shows it.
#[derive(Serialize)]
struct SlotView {
    running: bool,
    stopping: bool,
    /// The job holding it, while one runs (`SlotJob::name`), and which one it is (`SlotJob::kind`).
    holder: Option<String>,
    kind: Option<&'static str>,
    finished: Option<String>,
    /// A converter's last report of its downloads, its conversion and the cap (`lc0::kept`).
    progress: Option<lc0::Lc0Live>,
    /// A book job's last report of its pass (`book::kept`).
    book: Option<book::BookLive>,
}

/// A training's rates and patience, which replay a lambda's stages from its `history.tsv`.
#[derive(Clone)]
struct Schedule {
    rates: Vec<f64>,
    stage_count: usize,
    steps_per_round: u64,
    warmup_rounds: u64,
    patience_rounds: u64,
}

/// The status page, polling `/state.json`.
const PAGE: &str = include_str!("page.html");
const PAGE_PATH: &str = "/";
const STATE_PATH: &str = "/state.json";
/// The smithy's live bars, drawn nowhere but here (`progress::live`), polled four times a second: kept
/// apart from `/state.json`, which reads the run's files.
const PROGRESS_PATH: &str = "/progress.json";
/// Every forge config with its runs, and the tunes (`catalog`): read-only, on every listener.
const CATALOG_PATH: &str = "/catalog.json";
/// One generation of a run, `/view/<run>/<generation>` (`Status::generation_view`).
const VIEW_PREFIX: &str = "/view/";

/// A lambda's latest round, a row of the page's training table.
#[derive(Clone, Serialize)]
struct RoundRow {
    round: u64,
    learning_rate: f64,
    train_loss: f64,
    test_loss: f64,
    best_loss: f64,
    best_round: u64,
    /// The round's rate, counted from 1, of `stages`; `None` for a rate the schedule does not name.
    stage: Option<usize>,
    stages: usize,
    /// Rounds without a new best, of `patience`.
    waiting: u64,
    patience: u64,
    finished: bool,
    /// How long the round took, training and test: alike for every lambda trained in it, which
    /// train in lockstep.
    train_seconds: f64,
    test_seconds: f64,
    /// Every round's training and test seconds so far, round 0's test included: the training's elapsed
    /// time as its files record it, across stops and restarts, for the page's ETA.
    elapsed_seconds: f64,
}

/// What the status page draws, polled as JSON. Internal to the page: no other reader is promised
/// its shape.
#[derive(Serialize)]
struct Snapshot {
    job: Option<String>,
    /// The config the job is of, whose box and page show it.
    config: Option<String>,
    generation: Option<u32>,
    phase: Option<&'static str>,
    /// The smithy is stopping, or the job alone.
    stopping: bool,
    job_stopping: bool,
    /// A job is running, or asked for and about to.
    running: bool,
    finished: Option<String>,
    /// The page's job controls' token, on the loopback listener alone: the page shows them only then.
    control: Option<String>,
    /// Why nothing is dealt, as the arenas are told, when the work is idle.
    waiting: String,
    work: WorkView,
    /// A forge's phases of its current generation, each as far as it has gone.
    forge: Option<ForgeView>,
    arenas: Vec<ArenaView>,
    /// Every machine this smithy claims arenas on, and how each stands.
    machines: Vec<MachineView>,
    /// Whether the job deals games with no arena held: every arena free of it is another smithy's, or
    /// none answers, and the job waits.
    no_arena: bool,
    /// Whether this machine can train a full run (`has_gpu`): the page disables its buttons without
    /// it, never a scaled run's.
    gpu: bool,
    /// The source slot, beside the arena job.
    slot: SlotView,
}

/// A forge's generation, phase by phase: `None` for a phase that has not started, or that this kind
/// of run does not have.
#[derive(Serialize)]
struct ForgeView {
    play: Option<WorkView>,
    training: Option<Vec<LambdaView>>,
    /// The training's round under way, on the running forge's view alone (`rounds_display`).
    round: Option<rounds_display::RoundPassView>,
    round_robin: Option<WorkView>,
    gate: Option<WorkView>,
    /// A polyline run's ten values once measured (`polyline::show`), shown beside its training.
    polyline: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum WorkView {
    Idle,
    /// `time` while it plays (`self_play_time`); none for a phase shown from its files, not running.
    SelfPlay { teacher: String, test_got: u64, test_wanted: u64, train_got: u64, train_wanted: u64, time: Option<SelfPlayTime> },
    /// `time` from its record's arrivals (`pairs_eta`); `limit` its games' search, `depth <d>`.
    RoundRobin { pairings: usize, wanted: u32, standings: Vec<StandingView>, time: PlayTime, limit: String },
    /// The gate in champion and challenger's words: the champion is the generation's teacher, the
    /// challenger the round robin's winner; `challenger_share` its points over the games, as a test's. Its time as
    /// the round robin's, with no ETA: its SPRT ends whenever its evidence is enough. `limit` as the
    /// round robin's.
    Gate {
        limit: String,
        champion_points: f64,
        challenger_points: f64,
        games: u64,
        challenger_share: f64,
        llr: f64,
        lower: f64,
        upper: f64,
        time: PlayTime,
    },
    Pairs { played: usize, pairings: usize, wanted: u32 },
    /// `reference_nps`: the bench's nodes per second per thread on the smithy's machine, the speed
    /// every arena's clock is scaled to; none at a fixed depth, or before this smithy benched.
    /// `time` from its pairs' arrivals since it began dealing (`pairs_eta`); none once it no longer
    /// deals, `pair_times` then perhaps another phase's.
    Tune { player: String, done: u32, pairs: u32, reference_nps: Option<f64>, theta: Vec<ParameterView>, time: Option<PlayTime> },
    /// A test as its pairs have it so far (`matches::judge`); its number, which its screen is.
    Test {
        number: u32,
        test: String,
        settings: String,
        /// Each side as a player, its parameters in its name (`matches::Side::text`).
        challenger: String,
        champion: String,
        mixed: bool,
        limit: String,
        games: u64,
        challenger_points: f64,
        /// The champion's wins, losses and draws over the same games (`matches::Wdl`); none when a
        /// pair was recorded without them.
        wdl: Option<matches::Wdl>,
        status: String,
        gauge: matches::Gauge,
        verdict: Option<String>,
        /// A match's games in all, or a duel's projection; none for an SPRT.
        expected_games: Option<u64>,
        /// Its time from its record (`pairs_eta`), its ETA toward `expected_games`.
        time: PlayTime,
    },
    /// A book's scoring: the book's moves scored of all of them, those before this start among them;
    /// its pace this start, in moves a second, and the time left at it; records refused and the
    /// arenas refused for good, with why.
    Score {
        source: String,
        side: String,
        depth: u32,
        done: u64,
        total: u64,
        before: u64,
        elapsed_seconds: u64,
        moves_per_second: Option<f64>,
        eta_seconds: Option<u64>,
        rejected: u64,
        refused: Vec<String>,
    },
}

/// A book's scoring as the page draws it.
fn score_view(score: &ScoreDealing) -> WorkView {
    let elapsed = score.started.elapsed().as_secs_f64();
    let rate = (score.done_count > 0 && elapsed > 0.0).then(|| score.done_count as f64 / elapsed);
    let left = score.moves.len() as u64 - score.done_count;
    WorkView::Score {
        source: score.source.clone(),
        side: score.side.clone(),
        depth: score.depth,
        done: score.before + score.done_count,
        total: score.total,
        before: score.before,
        elapsed_seconds: elapsed as u64,
        moves_per_second: rate,
        eta_seconds: rate.map(|rate| (left as f64 / rate) as u64),
        rejected: score.rejected,
        refused: score.refused.iter().map(|(_, why)| why.clone()).collect(),
    }
}

#[derive(Serialize)]
struct LambdaView {
    lambda: ArmLambda,
    last: Option<RoundRow>,
}

#[derive(Clone, Serialize)]
struct StandingView {
    lambda: f64,
    points: f64,
    games: u64,
    score: f64,
    margin: f64,
}

#[derive(Clone, Serialize)]
struct ParameterView {
    name: &'static str,
    start: f64,
    now: f64,
}

#[derive(Serialize)]
struct ArenaView {
    session: String,
    machine: String,
    workers: usize,
    commit: String,
    chore: Option<String>,
    /// Tasks its workers are playing, as its last poll said; none before one.
    in_play: Option<usize>,
    /// Tasks taken from it this job.
    done: u64,
    /// `done` an hour of its play (`Session::played`); none before a task, or in its first minute.
    per_hour: Option<f64>,
    seen_seconds_ago: u64,
}

/// A machine the smithy claims an arena on: where, which machine answered, how it stands (claimed,
/// late, or why it is not held), and the tasks its arena holds unsent.
#[derive(Serialize)]
struct MachineView {
    target: String,
    machine: String,
    state: String,
    outbox: usize,
}

impl Status {
    fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().expect("the lock should not be poisoned")
    }

    /// The latest round of the `history.tsv` at `path`, its stages replayed by `schedule` as
    /// training replays them; `None` before its first. Cached by the file's modification time,
    /// so a poll reads only the files that grew.
    fn last_row(&self, path: &str, schedule: &Schedule) -> Option<RoundRow> {
        let modified = std::fs::metadata(path).and_then(|meta| meta.modified()).ok()?;
        let mut cache = self.history.lock().expect("the lock should not be poisoned");
        if let Some((when, row)) = cache.get(path)
            && *when == modified {
                return row.clone();
            }
        let per_round = schedule.steps_per_round;
        let elapsed_seconds: f64 =
            super::history::read_all(path).iter().map(|record| record.train_duration_secs + record.test_duration_secs).sum();
        let row = lockstep::latest_round(path, per_round, schedule.warmup_rounds, schedule.patience_rounds, schedule.stage_count).map(
            |(record, stages)| {
                RoundRow {
                    round: record.step / per_round,
                    learning_rate: record.learning_rate,
                    train_loss: record.train_loss,
                    test_loss: record.test_loss,
                    best_loss: stages.best_loss,
                    best_round: stages.best_step / per_round,
                    stage: schedule.rates.iter().position(|&rate| rate == record.learning_rate).map(|index| index + 1),
                    stages: schedule.stage_count,
                    waiting: stages.rounds_without_improvement(record.step, per_round),
                    patience: schedule.patience_rounds,
                    finished: stages.finished,
                    train_seconds: record.train_duration_secs,
                    test_seconds: record.test_duration_secs,
                    elapsed_seconds,
                }
            },
        );
        cache.insert(path.to_string(), (modified, row.clone()));
        row
    }

    /// The job as the status page draws it, the job controls' token given when `control` (the
    /// loopback listener's).
    fn snapshot(&self, now: Instant, control: bool) -> Snapshot {
        let (job, config, state_path) = {
            let shared = self.lock();
            (shared.job.clone(), shared.config.clone(), shared.state_path.clone())
        };
        let state = state_path.as_ref().and_then(|path| GenState::parse(&std::fs::read_to_string(path).ok()?).ok());
        // Each claimed machine's tasks in play, from its last poll, read before the job's lock.
        let in_play: HashMap<String, usize> = self
            .claims
            .held()
            .values()
            .filter_map(|held| Some((held.hello.machine.clone(), held.alive.as_ref()?.in_flight)))
            .collect();
        let (work, forge, finished, waiting, arenas) = {
            let shared = self.shared.lock().expect("the lock should not be poisoned");
            let mut arenas: Vec<ArenaView> = shared
                .sessions
                .iter()
                .map(|(id, session)| {
                    // Its time playing, not since its hello: a machine off or asleep adds nothing.
                    let hours = session.played.as_secs_f64() / 3600.0;
                    ArenaView {
                        session: id.clone(),
                        machine: session.hello.machine.clone(),
                        workers: session.hello.workers,
                        commit: session.hello.commit.clone(),
                        chore: session.chore.clone(),
                        in_play: in_play.get(&session.hello.machine).copied(),
                        done: session.taken,
                        // A first minute's rate swings too much to read.
                        per_hour: (session.taken > 0 && hours >= 1.0 / 60.0).then(|| session.taken as f64 / hours),
                        seen_seconds_ago: now.saturating_duration_since(session.last_seen).as_secs(),
                    }
                })
                .collect();
            arenas.sort_by(|a, b| a.session.cmp(&b.session));
            (shared.work_view(), shared.forge.clone(), shared.finished.clone(), shared.wait_reason(), arenas)
        };
        let slot_job = self.slot.job.lock().expect("the lock should not be poisoned").clone();
        // Read outside the lock: a training run's files are the phase's, not the requests'.
        let forge = match (forge, &state) {
            (Some(files), Some(state)) => {
                let lambdas = arms_of(&files.config, state);
                // A generation closed as the job ended (promoted, or its lineage stopped) has moved to
                // the remote: read there, as the View reads it (`readable_dirs`), so the page keeps the
                // job's phases on screen after it ends.
                let local = Dirs { root: files.root.clone() };
                let files = if Path::new(&local.generation(state.generation)).is_dir() {
                    files
                } else {
                    ForgeFiles { root: remote::remote_path(&files.root), ..files }
                };
                let round = if state.phase == Phase::Train { rounds_display::current_pass() } else { None };
                let view = self.forge_view(&files, state.generation, &lambdas, &work);
                // A new run's training, before its first round: every lambda listed as not started, so
                // the box is drawn whole from the frozen test set's read on, as a resumed run's is.
                let training = view.training.or_else(|| {
                    (state.phase == Phase::Train).then(|| lambdas.iter().map(|&lambda| LambdaView { lambda, last: None }).collect())
                });
                Some(ForgeView { round, training, ..view })
            }
            _ => None,
        };
        // Each lock taken and let go in turn.
        let dealing = !matches!(self.lock().work, None | Some(Work::Idle));
        let no_arena = dealing && self.claims.held().is_empty();
        return Snapshot {
            job,
            config,
            generation: state.map(|state| state.generation),
            phase: state.map(|state| state.phase.name()),
            stopping: self.stop.load(Ordering::SeqCst),
            job_stopping: self.job_stop.load(Ordering::SeqCst) && self.running.load(Ordering::SeqCst),
            running: self.running.load(Ordering::SeqCst),
            finished,
            control: control.then(|| self.control_token.clone()),
            waiting,
            work,
            forge,
            arenas,
            machines: self.claims.views(now),
            no_arena,
            gpu: has_gpu(),
            slot: SlotView {
                running: self.slot.running.load(Ordering::SeqCst),
                stopping: self.slot.stopping.load(Ordering::SeqCst) && self.slot.running.load(Ordering::SeqCst),
                holder: slot_job.as_ref().map(SlotJob::name),
                kind: slot_job.as_ref().map(SlotJob::kind),
                finished: self.slot.finished.lock().expect("the lock should not be poisoned").clone(),
                progress: lc0::kept(),
                book: book::kept(),
            },
        };
    }

    /// Generation `generation` of run `run`, as the page's View draws it: from its files, its running
    /// phase from memory when it is the running forge's own generation.
    fn generation_view(&self, run: &str, generation: u32) -> Result<ForgeView, String> {
        let (config, number) = players::parse_run(run)?;
        let plain = config.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
        if !plain || !is_forge_config(config) || !known_runs(config).contains(&number) {
            return Err(format!("{run} is no forge run"));
        }
        let (gen_config, net) = load_run_config(run)?;
        let current = run_state(&run_home(run)).filter(|state| state.generation == generation);
        // A closed generation is read where the mover put it.
        let files = ForgeFiles { root: readable_dirs(run, generation).root, ..ForgeFiles::of(run, &gen_config, &net) };
        let dirs = Dirs { root: files.root.clone() };
        // The running generation's lambdas are its grid's, before any has a directory; an earlier
        // generation's, those it trained.
        let lambdas = match current {
            Some(state) => arms_of(&gen_config, &state),
            None => generation_arms(&dirs, generation),
        };
        let live = {
            let shared = self.lock();
            let running = self.running.load(Ordering::SeqCst) && shared.forge.as_ref().is_some_and(|forge| forge.root == files.root);
            if current.is_some() && running { shared.work_view() } else { WorkView::Idle }
        };
        Ok(self.forge_view(&files, generation, &lambdas, &live))
    }

    /// Every phase of generation `generation` of the forge in `files`, at `lambdas`, as far as its
    /// files go, each phase that `live` shows as it runs taken from it instead: its counts are in
    /// memory before they are in sealed segments and records.
    fn forge_view(&self, files: &ForgeFiles, generation: u32, lambdas: &[ArmLambda], live: &WorkView) -> ForgeView {
        let dirs = Dirs { root: files.root.clone() };
        let config = &files.config;
        let play = match live {
            WorkView::SelfPlay { .. } => Some(live.clone()),
            _ if config.on_source(generation).is_some() || !Path::new(&dirs.test(generation)).exists() => None,
            _ => Some(WorkView::SelfPlay {
                teacher: if Path::new(&dirs.teacher_nnue(generation)).exists() {
                    format!("generation {}'s winner", generation - 1)
                } else {
                    PESTO.to_string()
                },
                test_got: positions_in(&dirs.test(generation)).unwrap_or(0),
                test_wanted: config.test_positions,
                train_got: positions_in(&dirs.train(generation)).unwrap_or(0),
                train_wanted: config.training_positions(),
                time: None,
            }),
        };
        let training: Vec<LambdaView> = lambdas
            .iter()
            .map(|&lambda| {
                let history = format!("{}/history.tsv", dirs.arm(generation, lambda));
                LambdaView { lambda, last: self.last_row(&history, &files.schedule) }
            })
            .collect();
        let round_robin = match live {
            WorkView::RoundRobin { .. } => Some(live.clone()),
            _ => {
                // Its players are the lambdas that learned, as the round robin chose them: a grid's
                // constants alone, a polyline run having no round robin.
                let constants: Vec<f64> = lambdas.iter().filter_map(|arm| arm.constant()).collect();
                let learned = learned_lambdas(&dirs, generation, &constants);
                let players = round_robin_players(learned.len());
                let record = dirs.round_robin(generation);
                let results = Path::new(&record).exists().then(|| recorded_pairs(&record, players.len()).ok()).flatten();
                results.map(|results| round_robin_view(&learned, &players, &results, config.round_robin_pairs, &recorded_times(&record), config.depth()))
            }
        };
        let gate = match live {
            WorkView::Gate { .. } => Some(live.clone()),
            _ => {
                let record = dirs.gate(generation);
                let results = Path::new(&record).exists().then(|| recorded_pairs(&record, 1).ok()).flatten();
                results.map(|results| gate_view(config.gate_elo_d(), &results[0], &recorded_times(&record), config.depth()))
            }
        };
        ForgeView {
            play,
            training: training.iter().any(|lambda| lambda.last.is_some()).then_some(training),
            round: None,
            round_robin,
            gate,
            polyline: (config.lambda == LambdaMode::Polyline)
                .then(|| polyline::read(&polyline_path(config, &dirs, generation)))
                .flatten()
                .map(|polyline| polyline::show(&polyline)),
        }
    }

    fn text(&self) -> String {
        let (job, state_path, finished) = {
            let shared = self.lock();
            (shared.job.clone(), shared.state_path.clone(), shared.finished.clone())
        };
        let mut text = match &job {
            Some(job) => format!("job {job}"),
            None => "no job yet".to_string(),
        };
        if let Some(line) = finished {
            text.push_str(&format!(", ended: {line}"));
        }
        if let Some(state_path) = &state_path {
            match std::fs::read_to_string(state_path).map_err(|error| error.to_string()).and_then(|text| GenState::parse(&text)) {
                Ok(state) => text.push_str(&format!(", generation {}, phase {}", state.generation, state.phase.name())),
                Err(message) => text.push_str(&format!(", state unreadable: {message}")),
            }
        }
        if self.stop.load(Ordering::SeqCst) {
            text.push_str(", stopping");
        }
        text.push('\n');
        let shared = self.shared.lock().expect("the lock should not be poisoned");
        let work = match shared.work() {
            Work::Idle => format!("nothing: {}", shared.wait_reason()),
            Work::SelfPlay { teacher, before, targets, .. } => format!(
                "self-play with {teacher}: test set {} of {} searched positions, training set {} of {}",
                before.0 + shared.test_got,
                targets.0,
                before.1 + shared.train_got,
                targets.1
            ),
            Work::Pairs { pairings, wanted, .. } => {
                let played: usize = shared.pairs.iter().map(Vec::len).sum();
                format!("pairs: {played} of {} played, {} pairings of {wanted}", pairings.len() * wanted as usize, pairings.len())
            }
            Work::Tune { .. } => match &shared.tune {
                Some(tune) => format!(
                    "a {} tune of {}: {} of {} pairs",
                    tune.kind.text(),
                    tune.player,
                    progress::grouped(tune.k_done),
                    progress::grouped(tune.pairs)
                ),
                None => "a tune".to_string(),
            },
            Work::Score => match &shared.score {
                Some(score) => format!(
                    "the scores of {}, {}, depth {}: {} of {} moves",
                    score.source,
                    score.side,
                    score.depth,
                    progress::grouped(score.before + score.done_count),
                    progress::grouped(score.total)
                ),
                None => "a book's scoring".to_string(),
            },
        };
        text.push_str(&format!("dealing {work}\n"));
        if let Some(job) = self.slot.job.lock().expect("the lock should not be poisoned").as_ref() {
            text.push_str(&format!("beside it, {}\n", job.name()));
        }
        for machine in self.claims.views(Instant::now()) {
            text.push_str(&format!("machine {}: {}\n", machine.target, machine.state));
        }
        if shared.sessions.is_empty() {
            text.push_str("no arena\n");
        }
        let mut sessions: Vec<(&String, &Session)> = shared.sessions.iter().collect();
        sessions.sort_by_key(|(id, _)| *id);
        for (id, session) in sessions {
            let chore = match session.chore.as_ref().and_then(|chore| shared.chores.get(chore).map(|dealt| (chore, dealt))) {
                Some((chore, dealt)) => format!("chore {chore}, {} of {}s", dealt.kind.name(), dealt.kind.task()),
                None => "no chore".to_string(),
            };
            text.push_str(&format!(
                "arena {id}, {} workers, {}: {chore}, {} tasks done\n",
                session.hello.workers, session.hello.commit, session.taken
            ));
        }
        text.push_str(&format!("page {}\n", self.page));
        text
    }
}

/// The status page and its state, on every listener and without a token: read-only, and nothing an
/// arena could not see, but for the job controls' token on the loopback listener (`control_here`).
fn page(status: &Status, request: &Request, control_here: bool) -> Option<Reply> {
    if request.method != "GET" {
        return None;
    }
    let json = |value: Result<Vec<u8>, serde_json::Error>, what: &str| -> Reply {
        match value {
            Ok(body) => Reply { code: 200, body },
            Err(error) => Reply::error(500, &format!("{what} as JSON: {error}")),
        }
    };
    match request.path.as_str() {
        PAGE_PATH => Some(Reply { code: 200, body: PAGE.as_bytes().to_vec() }),
        STATE_PATH => Some(json(serde_json::to_vec(&status.snapshot(Instant::now(), control_here)), "the state")),
        PROGRESS_PATH => Some(json(serde_json::to_vec(&progress::live()), "the progress")),
        CATALOG_PATH => Some(json(serde_json::to_vec(&catalog()), "the catalog")),
        path => {
            let rest = path.strip_prefix(VIEW_PREFIX)?;
            let parts: Vec<&str> = rest.split('/').collect();
            let [run, generation] = parts.as_slice() else {
                return Some(Reply::error(404, "a view is /view/<run>/<generation>"));
            };
            let Ok(generation) = generation.parse::<u32>() else {
                return Some(Reply::error(404, "a view's generation is a number"));
            };
            Some(match status.generation_view(run, generation) {
                Ok(view) => json(serde_json::to_vec(&view), "the view"),
                Err(message) => Reply::error(404, &message),
            })
        }
    }
}

/// The arms generation `generation` in `dirs` trained, from their directories: the constants
/// ascending, then the polyline.
fn generation_arms(dirs: &Dirs, generation: u32) -> Vec<ArmLambda> {
    let mut arms: Vec<ArmLambda> = std::fs::read_dir(dirs.generation(generation))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| ArmLambda::parse(entry.file_name().to_string_lossy().strip_prefix("lambda-")?).ok())
        .collect();
    arms.sort_by(|a, b| match (a.constant(), b.constant()) {
        (Some(a), Some(b)) => a.total_cmp(&b),
        (a, b) => a.is_none().cmp(&b.is_none()),
    });
    arms
}

/// The arms a generation trains under `config`: the polyline's one, or `state`'s grid.
fn arms_of(config: &GenConfig, state: &GenState) -> Vec<ArmLambda> {
    match config.lambda {
        LambdaMode::Polyline => vec![ArmLambda::Polyline],
        LambdaMode::Grid => state.grid().into_iter().map(|hundredths| ArmLambda::Constant(lambda_of(hundredths))).collect(),
    }
}

/// Where generation `generation`'s polyline is kept: beside its source's corpus on the remote for a
/// full run, shared by every full run of it as its frozen test set is; in the generation's directory
/// in self-play and for a scaled run, which uses only its own data, so no other run inherits a
/// polyline or a test set drawn from a share.
fn polyline_path(config: &GenConfig, dirs: &Dirs, generation: u32) -> String {
    match config.on_source(generation) {
        Some(on) if config.scale.is_none() => remote::remote_path(&format!("{}/polyline.txt", on.root())),
        _ => format!("{}/polyline.txt", dirs.generation(generation)),
    }
}

/// Serves the arenas and the status page on `listener` until `stop`, and the control too when
/// `control_here` is set: the loopback listener's, never the LAN's.
fn serve(listener: Listener, status: Arc<Status>, control_here: bool, stop: Arc<AtomicBool>) {
    let port = listener.port();
    let answer = |request: &Request| -> Reply {
        if control_here && !is_loopback_host(request.host.as_deref(), port) {
            return Reply::error(403, "the smithy's own machine reaches it as 127.0.0.1 or localhost only");
        }
        if let Some(reply) = page(&status, request, control_here) {
            return reply;
        }
        if let Some(reply) = control_here.then(|| control(&status, request)).flatten() {
            return reply;
        }
        let token = status.lock().token.clone();
        handle(&status.shared, &token, request, Instant::now())
    };
    listener.serve_concurrently(SERVE_THREADS, &answer, &stop);
}

/// Requests each listener answers at once. One at a time, a page poll building its snapshot (its
/// forge's files read from the remote) held up every worker of the arena on this machine, which
/// reaches the smithy over the loopback listener as the page does, each worker waiting in its
/// per-game send: that arena then played at a fraction of its cores.
const SERVE_THREADS: usize = 8;

/// Whether a request's `Host` names the loopback listener on `port`, as every legitimate caller's
/// does: a browser opened on this machine, `smithy stop` and `status`, a local arena. A DNS-rebound
/// page carries its own site's name instead, and would otherwise be handed the control token as a
/// page of this origin: the browser compares names, not addresses.
fn is_loopback_host(host: Option<&str>, port: u16) -> bool {
    host.is_some_and(|host| host == format!("{LOOPBACK}:{port}") || host == format!("localhost:{port}"))
}

// -- The phases ------------------------------------------------------------------------------------

struct Run<'a> {
    /// The run's name (`players::run_name`), which its players start with.
    name: String,
    config: &'a GenConfig,
    net: &'a NetConfig,
    dirs: Dirs,
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    /// The mover, poked at every boundary.
    archiver: &'a Archiver,
}

/// What a phase ended in.
enum Ended {
    Done,
    Stopped,
}

impl Run<'_> {
    /// A phase boundary: the state saved, the files that change along the run copied to the remote
    /// (`mirror_living`), and the mover asked for a pass, so a generation just closed moves and a
    /// pending move is tried again.
    fn boundary(&self, state: &GenState) -> Result<(), String> {
        write_state(&self.dirs, state)?;
        if let Err(message) = self.mirror_living(state.generation) {
            // A copy, not the run's own record: said, and the run goes on.
            log::warn!("the run's files not copied to {}: {message}", remote::this_root());
        }
        self.archiver.poke();
        Ok(())
    }

    /// The run's top-level files and its open generation `generation`, each copied to the remote
    /// when new or changed (`tree::copy`): its training state, histories, self-play segments, games
    /// and frozen set, so another smithy resumes it where it is. At its release and every
    /// `COPY_UP_EVERY` of training. Self-play segments go up as they close (`SegmentWriter::close`),
    /// open files never; any open file an older build left on the remote is removed here.
    fn copy_up(&self, generation: u32) -> Result<(), String> {
        if !tree::exists(&self.dirs.root) {
            return Ok(());
        }
        let generation_dir = players::generation_dir_name(generation);
        let keep = |relative: &str| tree::in_open_generation(relative, &generation_dir, lease::FILE);
        let remote_root = remote_run_dir(&self.name);
        let (files, bytes) = tree::copy(&self.dirs.root, &remote_root, &keep, false)?;
        if files > 0 {
            log::info!("{}: {} files, {} bytes copied to the remote", self.name, progress::grouped(files), progress::grouped(bytes));
        }
        let removed = remove_remote_open_files(&format!("{remote_root}/{generation_dir}"))?;
        if removed > 0 {
            log::info!("{}: {removed} stale open self-play files removed from the remote", self.name);
        }
        Ok(())
    }

    /// The run's files that change along it, each copied whole to the remote, replacing the last copy:
    /// its state, its log, its chores' counter, its config and corpus records, and every record of
    /// generation `generation` (each lambda's `history.tsv`, the round robin's and the gate's
    /// results). The remote never holds one being written or appended to, and a dead local disk loses
    /// at most a phase. A closed generation's are moved whole by the mover instead.
    fn mirror_living(&self, generation: u32) -> Result<(), String> {
        let root = &self.dirs.root;
        let mut files: Vec<String> =
            ["state.txt", "generations.tsv", "chores.txt", RUN_CONFIG, "corpus.txt"].iter().map(|name| format!("{root}/{name}")).collect();
        files.push(self.dirs.round_robin(generation));
        files.push(self.dirs.gate(generation));
        for arm in generation_arms(&self.dirs, generation) {
            files.push(format!("{}/history.tsv", self.dirs.arm(generation, arm)));
        }
        // A self-play generation's polyline, measured on its own games.
        files.push(format!("{}/polyline.txt", self.dirs.generation(generation)));
        for file in files.iter().filter(|file| Path::new(file).is_file()) {
            remote::copy_replacing(file, &remote::remote_path(file)).map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().expect("the lock should not be poisoned")
    }

    fn set_work(&self, work: Work) {
        set_work(&self.shared, work);
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    /// The teacher's network by its name (`publish`), `PESTO` for generation 1. A network trained under
    /// the atan curve is refused (`refuse_atan_teacher`).
    fn teacher(&self, generation: u32) -> Result<String, String> {
        let path = self.dirs.teacher_nnue(generation);
        if !Path::new(&path).exists() {
            return Ok(PESTO.to_string());
        }
        refuse_atan_teacher(&path)?;
        publish(&path)
    }

    fn play(&self, state: &GenState) -> Result<Ended, String> {
        let generation = state.generation;
        let (test_dir, train_dir) = (self.dirs.test(generation), self.dirs.train(generation));
        // The files a stopped run left open count too: their whole games are kept.
        let (test_segment, train_segment) = (SegmentWriter::open(&test_dir)?, SegmentWriter::open(&train_dir)?);
        // Counted as training and the freezer keep them, the searched positions, from the `.meta` files.
        let before = (searched_in(&test_dir, &test_segment)?, searched_in(&train_dir, &train_segment)?);
        let test_left = self.config.test_positions.saturating_sub(before.0);
        let train_target = self.config.training_positions();
        let train_left = train_target.saturating_sub(before.1);
        let teacher = self.teacher(generation)?;
        // The teacher plays its tune, refused while it has none (the developer, 2026-11-08).
        let parameters = match teacher_player(&self.name, generation, self.config.source) {
            Some(player) => required_tune(&player, &format!("generation {generation}'s self-play plays"))?,
            None => SearchParameters::DEFAULT,
        };
        log::info!(
            "playing: self-play with {teacher}, its tune, test set {} of {} searched positions, training set {} of {train_target}",
            self.config.test_positions - test_left,
            self.config.test_positions,
            train_target - train_left
        );
        let started = Instant::now();
        let targets = (self.config.test_positions, train_target);
        let work = Work::SelfPlay { teacher, parameters, test_dir, test_left, train_dir, train_left, before, targets };
        if let Ended::Stopped = deal_self_play(&self.shared, &self.stop, work, test_segment, train_segment)? {
            return Ok(Ended::Stopped);
        }
        log::info!("playing: done in {}", minutes(started));
        Ok(Ended::Done)
    }

    /// Loads the teacher's weights into `vs` when the generation has one; otherwise `vs` keeps its
    /// random initialisation. Says which.
    fn seed(&self, vs: &mut tch::nn::VarStore, teacher_vs: &str) {
        if Path::new(teacher_vs).exists() {
            vs.load(teacher_vs).unwrap_or_else(|error| panic!("failed to load {teacher_vs}: {error}"));
            log::info!("training: starting from the teacher, {teacher_vs}");
        } else if let Some(source) = self.config.source {
            log::info!("training: starting from random weights ({})", source.corpus());
        } else {
            log::info!("training: starting from random weights (PeSTO teaches)");
        }
    }

    fn train(&self, state: &GenState, arms: &[ArmLambda]) -> Result<Ended, String> {
        // The constant arms' lambdas, for the transformer's stream: a polyline run is NNUE only.
        let lambdas: Vec<f64> = arms.iter().filter_map(|arm| arm.constant()).collect();
        let lambdas = lambdas.as_slice();
        let generation = state.generation;
        // The students start from the teacher's weights (`seed`), so a teacher trained under the atan
        // curve would carry its unit into the new training.
        let teacher_nnue = self.dirs.teacher_nnue(generation);
        if Path::new(&teacher_nnue).exists() {
            refuse_atan_teacher(&teacher_nnue)?;
        }
        // Self-play trains on the generation's own games, a sourced run's generation 1 on its source's
        // whole corpus; both are judged on the generation's frozen `TestSet`, frozen here the first time.
        let on_source = self.config.on_source(generation);
        let (mut files, source) = match on_source {
            Some(on) => {
                if self.config.scale.is_none() && !on.is_complete() {
                    return Err(on.incomplete());
                }
                (pgn_train_stream::training_files(on.root())?, on.name().to_string())
            }
            None => (game_files(&self.dirs.train(generation))?, players::generation_dir_name(generation)),
        };
        // What the teacher's centipawns are read through: Lc0's curve for Lc0's corpus, the logistic
        // at S for PeSTO's and for a self-play generation's, the networks' own unit; nothing fitted.
        let threads = pgn_train_stream::reader_threads();
        let say = |line: String| log::info!("{line}");
        let curve = match on_source {
            Some(on) => on.curve()?,
            None => LOGISTIC,
        };
        // A scaled run on a source reads a `1/s` share of its corpus, so it starts in minutes on any
        // machine, never scattering the whole corpus first; its own games, self-play's, are scaled
        // already.
        let share = on_source.and(self.config.scale);
        if let Some(s) = share {
            files = scaled_share(files, s);
        }
        let test = TestSet::of(self.config, &self.dirs, generation)?;
        let config = self.net;
        let transformer = matches!(config.architecture.kind, ArchitectureKind::Transformer { .. });
        // An NNUE trains on its training set fully shuffled (`buckets.rs`), scattered here first: once for
        // a source's corpus, shared by every forge of that source; per generation for self-play. A disposable local cache, outside the run's directory, so
        // the archive never moves it; a run's older generations' buckets go.
        let bucket_dir = if let Some(on) = on_source {
            // A scaled share has buckets of its own: the full corpus's are never mixed with it.
            let scaled = share.map_or(String::new(), |s| format!("-s{s}"));
            format!("{BUCKETS_ROOT}/{}{scaled}", source_buckets(on))
        } else {
            let run_buckets = format!("{BUCKETS_ROOT}/{}", self.name);
            let current = players::generation_dir_name(generation);
            for entry in std::fs::read_dir(&run_buckets).into_iter().flatten().flatten() {
                if entry.file_name().to_string_lossy() != current {
                    std::fs::remove_dir_all(entry.path()).ok();
                }
            }
            format!("{run_buckets}/{current}")
        };
        let train_rows: u64 = if transformer {
            files.iter().map(|file| file.positions).sum()
        } else {
            match buckets::prepare(&bucket_dir, &files, threads, &self.stop, &say, curve)? {
                Prepared::Done(records) => records,
                Prepared::Stopped => return Ok(Ended::Stopped),
            }
        };
        // The polyline, measured from the buckets just scattered, so the corpus is read once: a
        // source's beside its corpus on the remote, shared by every full run of it as its frozen test
        // set is; a self-play generation's and a scaled run's in the generation's directory. The
        // frozen test set's polyline labels need it, so that set comes after.
        let polyline = if arms.contains(&ArmLambda::Polyline) {
            if transformer {
                return Err(format!("{}: lambda polyline needs buckets, which a transformer does not scatter", self.name));
            }
            let path = polyline_path(self.config, &self.dirs, generation);
            match polyline::ensure(&path, &bucket_dir, threads, &self.stop, &say)? {
                Some(polyline) => Some(polyline),
                None => return Ok(Ended::Stopped),
            }
        } else {
            None
        };
        let labelling = Labelling::new(arms, polyline)?;
        test.freeze(&labelling, &curve)?;
        log::info!(
            "training: lambda {} on {} positions, {} training set files",
            lambdas_text(arms),
            progress::grouped(train_rows),
            progress::grouped(files.len())
        );
        if let Some(on) = on_source {
            record_corpus(&self.dirs, on, train_rows, test.rows)?;
        }
        let started = Instant::now();
        let device = tch::Device::cuda_if_available();
        // Grouped by piece count for every architecture: a transformer's batch holds one length, and
        // every architecture is judged alike.
        let mut records = test.read(&labelling)?;
        let mut test_readers = [GroupedTestSet::load(&mut records, config.training.test_batch_size as usize)];
        drop(records);
        let lambda_dir = |lambda: ArmLambda| self.dirs.arm(generation, lambda);
        let parts = LockstepParts { source: &source, grid: arms.to_vec(), lambda_dir: &lambda_dir, persists_position: false, device };
        let teacher_vs = self.dirs.teacher_vs(generation);
        let batch_size = config.training.training_batch_size as usize;
        // Said as the rate switches are: what ended, and where its best was.
        let stage_count = config.training.learning_rate.len();
        let finish = |arm: &Arm| {
            let (_, test_loss) = export_best(config, &arm.run_dir, arm.lambda, &arm.stages);
            log::info!(
                "lambda {}: finished after stage {stage_count} of {stage_count}, best at round {} (test loss {test_loss:.6}), \
                 candidate exported",
                arm.lambda,
                arm.stages.best_step / config.training.steps_per_round
            );
        };
        let stop: &AtomicBool = &self.stop;
        // The training's state to the remote every `COPY_UP_EVERY`, between rounds, when no file is
        // being written: a crash loses at most that, and another smithy resumes from it.
        let copied_at = std::cell::Cell::new(Instant::now());
        let after_round = || {
            if copied_at.get().elapsed() < COPY_UP_EVERY {
                return;
            }
            copied_at.set(Instant::now());
            if let Err(message) = self.copy_up(generation) {
                log::warn!("{}: the training's state not copied to the remote: {message}", self.name);
            }
        };
        // Only the batches, the stream and the stacked network depend on the architecture.
        let done = if transformer {
            let shuffle = config.training.shuffle_buffer.expect("a streamed transformer has a shuffle buffer") as usize;
            let attention = config.training.attention.expect("net_config::load requires attention for a transformer");
            let to_batch = round::BatchBuilder {
                prepare: |records: &[CacheRecord]| transformer_batch::host_batch(records, attention),
                upload: |host: &transformer_batch::HostTransformerBatch| transformer_batch::upload(host, device),
            };
            let stack_of = |arms: &[Arm], active: &[usize]| {
                let network = StackedTransformer::new(&config.architecture, active.len() as i64, device);
                let starting_weights = || {
                    let mut vs = tch::nn::VarStore::new(device);
                    let _ = transformer_network::TransformerNetwork::new(&vs.root(), &config.architecture);
                    self.seed(&mut vs, &teacher_vs);
                    vs.variables()
                };
                open_stack(config, network, Vec::new(), arms, active, starting_weights, false, device)
            };
            // One shuffle buffer per piece count, so that every batch holds one length.
            let stream_from = |steps_done: u64, _: Option<Vec<u64>>| {
                BucketedTrainStream::open(files.clone(), lambdas, shuffle, batch_size, steps_done * batch_size as u64, threads)
            };
            train_in_lockstep(
                config, &parts, arms, train_rows, &mut test_readers, stack_of, stream_from, &to_batch,
                transformer_batch::HostTransformerBatch::bytes, finish, &after_round, stop,
            )
        } else {
            let feature_set = feature_set_of(&config.architecture.kind);
            let to_batch = round::BatchBuilder {
                prepare: |records: &[CacheRecord]| batch::host_batch(records, feature_set),
                upload: |host: &batch::HostBatch| batch::upload(host, device),
            };
            let stack_of = |arms: &[Arm], active: &[usize]| {
                let network = StackedNetwork::new(feature_set, &config.architecture, active.len() as i64, device);
                let bounds = network.weight_bounds();
                let starting_weights = || {
                    let mut vs = tch::nn::VarStore::new(device);
                    let _ = nnue_network::Network::of(&vs.root(), feature_set, &config.architecture);
                    self.seed(&mut vs, &teacher_vs);
                    vs.variables()
                };
                open_stack(config, network, bounds, arms, active, starting_weights, false, device)
            };
            // The buckets were just prepared, so a failure to open them is a disk gone wrong.
            let stream_from = |steps_done: u64, _: Option<Vec<u64>>| {
                BucketStream::open(&bucket_dir, labelling.clone(), steps_done * batch_size as u64)
                    .unwrap_or_else(|message| panic!("{message}"))
            };
            train_in_lockstep(
                config, &parts, arms, train_rows, &mut test_readers, stack_of, stream_from, &to_batch, batch::HostBatch::bytes,
                finish, &after_round, stop,
            )
        };
        self.lock().view = View::None;
        if !done {
            return Ok(Ended::Stopped);
        }
        log::info!("training: done in {}", minutes(started));
        // Every candidate to the remote at once, where the arenas read it: the round robin then deals
        // nothing an arena must wait long for.
        for &lambda in arms {
            let candidate = generation::candidate_network_path(&lambda_dir(lambda));
            if Path::new(&candidate).is_file() {
                publish(&candidate)?;
            }
        }
        Ok(Ended::Done)
    }

    /// Plays `pairings`, `wanted` pairs each, as chores, recording into `record`, until `decided`
    /// says so (given every pairing's results so far) or every pairing is complete; the challengers
    /// and teachers playing `sides`.
    fn play_pairs(
        &self, pairings: Vec<(String, String)>, sides: [SearchParameters; 2], wanted: u32, record: &str,
        mut decided: impl FnMut(&[Vec<i32>]) -> bool,
    ) -> Result<(Ended, Vec<Vec<i32>>), String> {
        let results = recorded_pairs(record, pairings.len())?;
        mark_start(record)?;
        {
            // At once, so no chore is dealt from an empty table.
            let mut shared = self.lock();
            shared.epoch += 1;
            shared.pairs = results.clone();
            shared.pair_wdl = recorded_wdl(record, pairings.len());
            shared.pair_times = recorded_times(record);
            shared.work = Some(Work::Pairs { pairings, wanted, record: record.to_string(), sides });
        }
        let mut seen = usize::MAX;
        loop {
            let results = self.lock().pairs.clone();
            let total: usize = results.iter().map(Vec::len).sum();
            let complete = results.iter().all(|pairs| pairs.len() as u32 >= wanted);
            if total != seen {
                seen = total;
                if decided(&results) || complete {
                    self.set_work(Work::Idle);
                    return Ok((Ended::Done, results));
                }
            }
            if self.stopped() {
                self.set_work(Work::Idle);
                return Ok((Ended::Stopped, results));
            }
            std::thread::sleep(POLL);
        }
    }

    /// The candidate of arm `lambda` by its name, on the remote (`publish`).
    fn candidate(&self, state: &GenState, lambda: ArmLambda) -> Result<String, String> {
        let dir = self.dirs.arm(state.generation, lambda);
        publish(&generation::candidate_network_path(&dir))
    }

    /// The round robin: the winning lambda, by points.
    fn round_robin(&self, state: &GenState, lambdas: &[f64]) -> Result<(Ended, f64), String> {
        let names: Vec<String> =
            lambdas.iter().map(|&lambda| self.candidate(state, ArmLambda::Constant(lambda))).collect::<Result<_, _>>()?;
        let players = round_robin_players(lambdas.len());
        let pairings: Vec<(String, String)> = players.iter().map(|&(i, j)| (names[i].clone(), names[j].clone())).collect();
        let wanted = self.config.round_robin_pairs;
        let total_games = pairings.len() as u64 * u64::from(wanted) * 2;
        log::info!(
            "round robin: {} lambdas, {} pairings of {} pairs, {} games",
            lambdas.len(),
            pairings.len(),
            progress::grouped(wanted),
            progress::grouped(total_games)
        );
        let started = Instant::now();
        let record = self.dirs.round_robin(state.generation);
        // Its progress is its section's graphical bar alone, timed from its record (`round_robin_view`).
        self.lock().view = View::RoundRobin { lambdas: lambdas.to_vec(), players: players.clone() };
        // Candidates against candidates: the defaults, judging the networks alone.
        let (ended, results) = self.play_pairs(pairings, [SearchParameters::DEFAULT; 2], wanted, &record, |_| false)?;
        self.lock().view = View::None;
        if let Ended::Stopped = ended {
            return Ok((Ended::Stopped, 0.0));
        }
        // Every pairing is complete here, so the points rank (`round_robin`'s module doc).
        let first = &round_robin::standings(lambdas.len(), &players, &results)[0];
        log::info!(
            "round robin: done in {}, winner lambda {:.2}, {} of {} points ({:.1}%)",
            minutes(started),
            lambdas[first.candidate],
            points_text(first.points),
            first.games,
            100.0 * first.score
        );
        Ok((Ended::Done, lambdas[first.candidate]))
    }

    /// The gate: `winner`'s candidate against the teacher, an SPRT. `Some(pass)` once decided.
    fn gate(&self, state: &GenState, winner: ArmLambda) -> Result<(Ended, bool, f64), String> {
        let challenger = self.candidate(state, winner)?;
        let teacher = self.teacher(state.generation)?;
        // The challenger the defaults, untuned yet; the teacher the defaults too, so the gate judges
        // the networks alone, or PeSTO's tune when PeSTO teaches.
        let sides = [SearchParameters::DEFAULT, side_parameters(&teacher, &format!("generation {}'s gate plays", state.generation))?];
        log::info!("gate: lambda {winner} against {teacher}, SPRT at {} Elo", self.config.gate_elo_d());
        let started = Instant::now();
        let elo_d = self.config.gate_elo_d();
        let record = self.dirs.gate(state.generation);
        self.lock().view = View::Gate { elo_d };
        let (ended, results) = self.play_pairs(vec![(challenger, teacher)], sides, GATE_MAX_PAIRS, &record, |results| {
            sprt_of(elo_d, &results[0]).verdict() != Verdict::Continue
        })?;
        self.lock().view = View::None;
        if let Ended::Stopped = ended {
            return Ok((Ended::Stopped, false, 0.0));
        }
        let sprt = sprt_of(elo_d, &results[0]);
        let share = sprt.score() / sprt.games().max(1) as f64;
        let pass = sprt.verdict() == Verdict::Promote;
        log::info!(
            "gate: {} in {}, after {} games, the champion {} of them ({:.1}%), llr {:+.2}",
            if pass { "passed" } else { "failed" },
            minutes(started),
            sprt.games(),
            points_text(sprt.games() as f64 - sprt.score()),
            100.0 * (1.0 - share),
            sprt.llr()
        );
        Ok((Ended::Done, pass, share))
    }
}

/// Deals `work` from now on: every chore dealt before is over, its tasks arriving late dropped.
fn set_work(shared: &Mutex<Shared>, work: Work) {
    let mut shared = shared.lock().expect("the lock should not be poisoned");
    shared.epoch += 1;
    (shared.test_got, shared.train_got) = (0, 0);
    shared.self_play_started = matches!(work, Work::SelfPlay { .. }).then(Instant::now);
    shared.work = Some(work);
}

/// Self-play's time as the other phases show theirs, its pace in the positions its targets count
/// rather than games, whose length varies: elapsed since it began dealing, and what the positions
/// left take at the pace of those received (`got` of `wanted`, both sets together), with that pace
/// a second; none until a position has arrived, and no ETA once nothing is left. Its `PlayTime`
/// carries no games a second (`SelfPlayTime::positions_per_second` instead).
fn self_play_time(started: Option<Instant>, got: u64, wanted: u64) -> SelfPlayTime {
    let none = PlayTime { elapsed_seconds: None, games_per_second: None, eta_seconds: None };
    let Some(started) = started else {
        return SelfPlayTime { time: none, positions_per_second: None };
    };
    let seconds = started.elapsed().as_secs();
    let paced = got > 0 && seconds > 0;
    let left = wanted.saturating_sub(got);
    SelfPlayTime {
        time: PlayTime {
            elapsed_seconds: Some(seconds),
            eta_seconds: (paced && left > 0).then(|| (seconds as f64 * left as f64 / got as f64).round() as u64),
            ..none
        },
        positions_per_second: paced.then(|| got as f64 / seconds as f64),
    }
}

/// Self-play's time for the page (`self_play_time`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
struct SelfPlayTime {
    time: PlayTime,
    positions_per_second: Option<f64>,
}

/// Deals `work`, a `Work::SelfPlay`, until both its sets are played or `stop` is raised, its games
/// appended to `test_segment` and `train_segment`, the files open in its directories: closed into
/// segments once played, left open when stopped, so that a resume goes on with their whole games.
fn deal_self_play(
    shared: &Mutex<Shared>, stop: &AtomicBool, work: Work, test_segment: SegmentWriter, train_segment: SegmentWriter,
) -> Result<Ended, String> {
    let (test_left, train_left) = match &work {
        Work::SelfPlay { test_left, train_left, .. } => (*test_left, *train_left),
        _ => return Err("deal_self_play deals self-play only".to_string()),
    };
    let lock = || shared.lock().expect("the lock should not be poisoned");
    set_work(shared, work);
    {
        let mut shared = lock();
        shared.test_segment = Some(test_segment);
        shared.train_segment = Some(train_segment);
    }
    // Its progress is its box's, drawn from its view (`WorkView::SelfPlay`).
    loop {
        let (test_got, train_got) = {
            let shared = lock();
            (shared.test_got, shared.train_got)
        };
        if test_got >= test_left && train_got >= train_left {
            break;
        }
        if stop.load(Ordering::SeqCst) {
            // Left open: resumed with the job.
            set_work(shared, Work::Idle);
            let mut shared = lock();
            (shared.test_segment, shared.train_segment) = (None, None);
            return Ok(Ended::Stopped);
        }
        std::thread::sleep(POLL);
    }
    set_work(shared, Work::Idle);
    lock().close_segments()?;
    Ok(Ended::Done)
}

/// A gap between two pairs' arrivals longer than this is a pause, not play: a stop and a restart, or
/// no arena (the developer, 2026-10-23).
const PAUSE_SECONDS: u64 = 5 * 60;

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

/// The play time `times` record (`Shared::pair_times`): the sum of the gaps up to each pair's arrival,
/// from the arrival or start before it, each over `PAUSE_SECONDS` left out as a pause; and the pairs
/// so counted. The time from a start to its first pair counts, which is most of a short phase played
/// at once; the gap up to a start is never play, a long stop adding nothing. The pace is the
/// pairs counted over that time.
fn play_time(times: &[(u64, bool)]) -> (u64, u64) {
    let (mut seconds, mut pairs) = (0, 0);
    for window in times.windows(2) {
        let ((before, _), (at, pair)) = (window[0], window[1]);
        let gap = at.saturating_sub(before);
        if pair && gap <= PAUSE_SECONDS {
            seconds += gap;
            pairs += 1;
        }
    }
    (seconds, pairs)
}

/// A phase's time as its pairs' arrivals record it (`play_time`), for the page.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
struct PlayTime {
    /// None before two arrivals.
    elapsed_seconds: Option<u64>,
    /// Games a second at that pace, two a pair; none without play counted.
    games_per_second: Option<f64>,
    /// The seconds the pairs left take at that pace; none without play counted.
    eta_seconds: Option<u64>,
}

/// The time `times` record, and what `pairs_left` more take at its pace.
fn pairs_eta(times: &[(u64, bool)], pairs_left: u64) -> PlayTime {
    if times.len() < 2 {
        return PlayTime { elapsed_seconds: None, games_per_second: None, eta_seconds: None };
    }
    let (seconds, gaps) = play_time(times);
    let paced = gaps > 0 && seconds > 0;
    PlayTime {
        elapsed_seconds: Some(seconds),
        games_per_second: paced.then(|| 2.0 * gaps as f64 / seconds as f64),
        // None once nothing is left: a finished phase has no ETA, not `0s`.
        eta_seconds: (paced && pairs_left > 0).then(|| (seconds as f64 * pairs_left as f64 / gaps as f64).round() as u64),
    }
}

/// A pairs record's line saying when its phase began dealing, `#start<TAB><UTC Unix seconds>`: one
/// at each start, a resume's included. Lines starting `#` carry no pair.
const START_LINE: &str = "#start";

/// Notes in `record` that its phase begins dealing now.
fn mark_start(record: &str) -> Result<(), String> {
    append(record, &format!("{START_LINE}\t{}\n", unix_now()))
}

/// The times `record` holds, in order (`Shared::pair_times`): each start, and each pair's arrival,
/// its sixth field; a pair's line written before arrivals were recorded has none, and adds nothing.
fn recorded_times(record: &str) -> Vec<(u64, bool)> {
    let Ok(text) = std::fs::read_to_string(record) else {
        return Vec::new();
    };
    text
        .lines()
        .filter_map(|line| match line.strip_prefix(START_LINE) {
            Some(rest) => Some((rest.trim().parse().ok()?, false)),
            None => Some((line.split('\t').nth(5)?.parse().ok()?, true)),
        })
        .collect()
}

/// The round robin as the page shows it: `results` by pairing of `players`, `wanted` pairs each, its
/// time from `times`, its games at `depth`.
fn round_robin_view(lambdas: &[f64], players: &[(usize, usize)], results: &[Vec<i32>], wanted: u32, times: &[(u64, bool)], depth: u32) -> WorkView {
    let played: u64 = results.iter().map(|pairs| pairs.len() as u64).sum();
    WorkView::RoundRobin {
        pairings: players.len(),
        wanted,
        time: pairs_eta(times, (players.len() as u64 * u64::from(wanted)).saturating_sub(played)),
        limit: format!("depth {depth}"),
        standings: round_robin::standings(lambdas.len(), players, results)
            .iter()
            .map(|standing| StandingView {
                lambda: lambdas[standing.candidate],
                points: standing.points,
                games: standing.games,
                score: standing.score,
                margin: 1.96 * standing.standard_error,
            })
            .collect(),
    }
}

/// The gate as the page shows it: its SPRT at `elo_d` over `pairs`, its time from `times`, its games
/// at `depth`.
fn gate_view(elo_d: f64, pairs: &[i32], times: &[(u64, bool)], depth: u32) -> WorkView {
    let sprt = sprt_of(elo_d, pairs);
    let (lower, upper) = sprt.bounds();
    // No ETA: its SPRT ends whenever its evidence is enough.
    let time = PlayTime { eta_seconds: None, ..pairs_eta(times, 0) };
    WorkView::Gate {
        limit: format!("depth {depth}"),
        time,
        champion_points: sprt.games() as f64 - sprt.score(),
        challenger_points: sprt.score(),
        games: sprt.games(),
        challenger_share: sprt.score() / sprt.games().max(1) as f64,
        llr: sprt.llr(),
        lower,
        upper,
    }
}

/// Test `number` as the page shows it: `spec`, judged over `pairs`, its wins, losses and draws from
/// `wdl` (aligned with `pairs`) over the same pairs the verdict counts, its time from `times`; its
/// ETA toward its expected games, none without them (an SPRT).
fn test_view(number: u32, spec: &matches::Spec, pairs: &[i32], wdl: &[Option<matches::Wdl>], times: &[(u64, bool)]) -> WorkView {
    let judged = matches::judge(spec.kind, pairs);
    let wdl = matches::Wdl::sum(wdl, judged.pairs);
    let pairs_left = judged.expected_games.map_or(0, |expected| expected.saturating_sub(judged.games) / 2);
    let time = pairs_eta(times, pairs_left);
    WorkView::Test {
        number,
        expected_games: judged.expected_games,
        // An SPRT, with no expected length, has no ETA.
        time: PlayTime { eta_seconds: judged.expected_games.and(time.eta_seconds), ..time },
        test: spec.kind.name().to_string(),
        settings: spec.kind.settings(),
        challenger: spec.challenger.text(),
        champion: spec.champion.text(),
        mixed: spec.mixed(),
        limit: spec.limit.text(),
        games: judged.games,
        challenger_points: judged.challenger_points,
        wdl,
        status: judged.status,
        gauge: judged.gauge,
        verdict: judged.verdict,
    }
}

/// Each pair's wins, losses and draws that `record` holds, by pairing, of `count` pairings, aligned
/// with `recorded_pairs`: its seventh field, `None` for a line written before it was; none without
/// the file.
fn recorded_wdl(record: &str, count: usize) -> Vec<Vec<Option<matches::Wdl>>> {
    let mut wdl = vec![Vec::new(); count];
    if let Ok(text) = std::fs::read_to_string(record) {
        for line in text.lines().filter(|line| !line.starts_with('#')) {
            let fields: Vec<&str> = line.split('\t').collect();
            if let Some(pairing) = fields.first().and_then(|pairing| pairing.parse::<usize>().ok()).filter(|&pairing| pairing < count) {
                wdl[pairing].push(fields.get(6).and_then(|field| matches::Wdl::parse(field)));
            }
        }
    }
    wdl
}

/// The pair results `record` holds, by pairing, of `count` pairings; none without the file.
fn recorded_pairs(record: &str, count: usize) -> Result<Vec<Vec<i32>>, String> {
    let mut results = vec![Vec::new(); count];
    if let Ok(text) = std::fs::read_to_string(record) {
        for line in text.lines().filter(|line| !line.starts_with('#')) {
            // `pairing<TAB>r`, then the pair's identity and arrival, which a resume needs not; a
            // `#` line, a start (`START_LINE`), carries no pair.
            let mut fields = line.split('\t');
            let (Some(pairing), Some(r)) = (fields.next(), fields.next()) else {
                return Err(format!("{record}: malformed line {line:?}"));
            };
            let (pairing, r): (usize, i32) =
                (pairing.parse().map_err(|_| format!("{record}: {line:?}"))?, r.parse().map_err(|_| format!("{record}: {line:?}"))?);
            if pairing < count {
                results[pairing].push(r);
            }
        }
    }
    Ok(results)
}

/// The round robin's pairings of `count` lambdas, by index: every two, the first before the second.
fn round_robin_players(count: usize) -> Vec<(usize, usize)> {
    (0..count).flat_map(|i| (i + 1..count).map(move |j| (i, j))).collect()
}

/// The SPRT over `pairs`, in their order.
fn sprt_of(elo_d: f64, pairs: &[i32]) -> Sprt {
    let mut sprt = Sprt::new(elo_d);
    for &r in pairs {
        sprt.add_pair(r);
        if sprt.verdict() != Verdict::Continue {
            break;
        }
    }
    sprt
}

fn points_text(value: f64) -> String {
    if value.fract() == 0.0 { format!("{value:.0}") } else { format!("{value:.1}") }
}

fn minutes(started: Instant) -> String {
    let seconds = started.elapsed().as_secs();
    let (days, hours, mins) = (seconds / 86_400, seconds / 3600 % 24, seconds / 60 % 60);
    if days > 0 { format!("{days}d{hours:02}h{mins:02}m") } else { format!("{hours}h{mins:02}m") }
}

/// Generation `generation`'s test set (`freeze.rs`): where it is frozen, the games it is drawn from,
/// and its positions, drawn from the searched ones, as training keeps them. On a source's corpus (a
/// sourced run's generation 1), `<root>/frozen` on the remote, drawn from the source's test split at
/// the source's size, shared by every run of that source; in self-play, `gen-<g>/frozen`, drawn from the test games the arenas played for
/// the generation, at the run's `test_positions`. `train` and the inspectors both find it here, so they
/// never measure against different positions.
struct TestSet {
    frozen: String,
    games: String,
    source: Option<Source>,
    /// The positions the run reads, and those the set is frozen at.
    rows: u64,
    frozen_rows: u64,
}

impl TestSet {
    fn of(config: &GenConfig, dirs: &Dirs, generation: u32) -> Result<TestSet, String> {
        if let Some(source) = config.on_source(generation) {
            let frozen_rows = source.test_positions()?;
            // A scaled run uses only its own data: frozen in its own generation, at its scaled size.
            if let Some(s) = config.scale {
                let rows = scale::volume(frozen_rows, s, scale::MIN_BATCH);
                return Ok(TestSet {
                    frozen: format!("{}/frozen", dirs.generation(generation)),
                    games: remote::remote_path(&preprocessing_config::source_test_dir(source.root())),
                    source: Some(source),
                    rows,
                    frozen_rows: rows,
                });
            }
            return Ok(TestSet {
                frozen: remote::remote_path(&format!("{}/frozen", source.root())),
                games: remote::remote_path(&preprocessing_config::source_test_dir(source.root())),
                source: Some(source),
                rows: frozen_rows,
                frozen_rows,
            });
        }
        Ok(TestSet {
            frozen: format!("{}/frozen", dirs.generation(generation)),
            games: dirs.test(generation),
            source: None,
            rows: config.test_positions,
            frozen_rows: config.test_positions,
        })
    }

    /// Freezes the set at its positions, or adds the label sets of `labelling`'s arms it lacks, its
    /// scores read through `curve`, the teacher's.
    fn freeze(&self, labelling: &Labelling, curve: &Curve) -> Result<(), String> {
        let files = || {
            match self.source {
                Some(source) => pgn_train_stream::test_files(source.root()),
                None => game_files(&self.games),
            }
        };
        // A source's set is on the remote, shared by every run of the source: one smithy at a time
        // writes it, under a lease taken only when there is something to write, so a resume waits
        // for no sync. A run's own set is written under its run's lease.
        let _lease = match self.source {
            Some(_) if !freeze::is_complete(&self.frozen, self.frozen_rows, labelling) => {
                let path = format!("{}/{}", self.frozen, lease::FILE);
                let job = format!("the frozen test set {}", self.frozen);
                Some(lease::take(&path, &job)?)
            }
            _ => None,
        };
        freeze::ensure(&self.frozen, &self.games, files, self.frozen_rows, labelling, curve)
    }

    /// The frozen set, labelled for each of `labelling`'s arms.
    fn read(&self, labelling: &Labelling) -> Result<RecordsSource, String> {
        Ok(RecordsSource(freeze::read(&self.frozen, self.rows, labelling)?))
    }
}

/// An error when the network at `path` was trained under the atan curve (`nnue::ATAN_VERSION`): it may
/// still play, but neither teaches self-play nor seeds a student, since its centipawns are in the old
/// unit, not what `sigma(cp / S)` reads.
fn refuse_atan_teacher(path: &str) -> Result<(), String> {
    if fattorini::nnue::file_version(path)? == fattorini::nnue::ATAN_VERSION {
        return Err(format!(
            "{path} was trained under the old atan curve: it may still play, but neither teaches nor is trained further; start a new run"
        ));
    }
    Ok(())
}

/// What a sourced run's generation 1 trained on, `corpus.txt` in its directory, written at its first
/// training and kept: the source's size may grow later, and the record says which size this run had.
fn record_corpus(dirs: &Dirs, source: Source, training_positions: u64, test_positions: u64) -> Result<(), String> {
    let path = format!("{}/corpus.txt", dirs.root);
    if Path::new(&path).exists() {
        return Ok(());
    }
    let text = format!("source\t{}\ntraining_positions\t{training_positions}\ntest_positions\t{test_positions}\n", source.name());
    lan::write_whole(&path, &text)
}

/// Whether the engine can play `net`'s candidates: every architecture but the transformer, until
/// MCTS.
fn engine_plays(net: &NetConfig) -> bool {
    !matches!(net.architecture.kind, ArchitectureKind::Transformer { .. })
}

/// A test set held in memory, swept in order.
pub(crate) struct RecordsSource(Vec<CacheRecord>);

impl BatchSource for RecordsSource {
    fn row_count(&self) -> u64 {
        self.0.len() as u64
    }

    fn read_batch_stop_short(&mut self, cursor: u64, batch_size: usize) -> (Vec<CacheRecord>, u64) {
        let start = cursor as usize;
        let end = (start + batch_size).min(self.0.len());
        (self.0[start..end].to_vec(), if end == self.0.len() { 0 } else { end as u64 })
    }
}

fn new_token() -> String {
    let mut rng = rand::rng();
    format!("{:016x}{:016x}", rng.random::<u64>(), rng.random::<u64>())
}

fn machine_name() -> Result<String, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Machine {
        name: String,
    }
    let path = "data/machine.yaml";
    let text = std::fs::read_to_string(path).map_err(|error| format!("{path}: {error} (write it once: `echo 'name: <machine>' > {path}`)"))?;
    let machine: Machine = serde_yaml::from_str(&text).map_err(|error| format!("{path}: {error}"))?;
    Ok(machine.name)
}

/// The network at `path` by its name, its path under `data/`, which a chore carries and an arena reads it by from the remote: copied there
/// first, whole, when the remote does not hold it at its size. A network on the remote already
/// (`players::resolve_network_file` found it there) is named as it is. A network is never rewritten,
/// so a name always means the same bytes.
fn publish(path: &str) -> Result<String, String> {
    if let Some(name) = path.strip_prefix(&format!("{}/", remote::this_root())) {
        return Ok(name.to_string());
    }
    let name = path.to_string();
    let target = remote::remote_path(&name);
    let size = std::fs::metadata(path).map_err(|error| format!("{path}: {error}"))?.len();
    if !std::fs::metadata(&target).is_ok_and(|metadata| metadata.len() == size) {
        remote::copy_replacing(path, &target).map_err(|error| error.to_string())?;
    }
    Ok(name)
}


/// Where generation `generation` of run `run` is read (`readable_of`): this machine's copy while it
/// has one, else the remote's, where a closed generation moves (`archive.rs`).
fn readable_dirs(run: &str, generation: u32) -> Dirs {
    readable_of(Dirs { root: run_dir(run) }, Dirs { root: remote_run_dir(run) }, generation)
}

/// `local`'s generation while it has a folder, unless that folder trained nothing (no `lambda-*`)
/// and `remote`'s did: then it is not the generation's copy but a tune's folder alone, fetched back
/// to show the tune (`Tune::open`), and the generation itself is the remote's. A running generation,
/// local and with no arm yet during its self-play, has none on the remote either, so stays local.
fn readable_of(local: Dirs, remote: Dirs, generation: u32) -> Dirs {
    if !Path::new(&local.generation(generation)).is_dir() {
        return remote;
    }
    if generation_arms(&local, generation).is_empty() && !generation_arms(&remote, generation).is_empty() {
        return remote;
    }
    local
}

fn copy(from: &str, to: &str) -> Result<(), String> {
    let partial = format!("{to}.partial");
    std::fs::copy(from, &partial).map_err(|error| format!("copying {from}: {error}"))?;
    std::fs::rename(&partial, to).map_err(|error| format!("{to}: {error}"))
}

/// Writes `state`, its `boundary` one past the last written: the count of the run's states ever
/// written, which tells a run's newest copy, here or on the remote (`sync_down`).
fn write_state(dirs: &Dirs, state: &GenState) -> Result<(), String> {
    let next = std::fs::read_to_string(dirs.state()).map_or(0, |text| boundary_of(&text)) + 1;
    lan::write_whole(&dirs.state(), &format!("{}boundary\t{next}\n", state.text()))
}

/// A `state.txt`'s boundary count (`write_state`); 0 for one written before it was counted.
fn boundary_of(text: &str) -> u64 {
    lan::fields(text).get("boundary").and_then(|value| value.parse().ok()).unwrap_or(0)
}

/// Brings run `run`'s newest copy here before it resumes: when the remote's `state.txt` is past this
/// machine's (`boundary_of`), or this machine has none, the remote's top-level files and its open
/// generation copied down over the local ones, all of them, since the local ones are an older run's.
/// Closed generations stay on the remote, the mover's. Nothing when the remote has no state.
fn sync_down(run: &str) -> Result<(), String> {
    let (local, remote_dir) = (run_dir(run), remote_run_dir(run));
    let Ok(remote_text) = std::fs::read_to_string(format!("{remote_dir}/state.txt")) else {
        return Ok(());
    };
    let remote_state = GenState::parse(&remote_text).map_err(|message| format!("{remote_dir}/state.txt: {message}"))?;
    let local_boundary = std::fs::read_to_string(format!("{local}/state.txt")).ok().map(|text| boundary_of(&text));
    if local_boundary.is_some_and(|boundary| boundary >= boundary_of(&remote_text)) {
        return Ok(());
    }
    let generation_dir = players::generation_dir_name(remote_state.generation);
    log::info!("{run}: the remote holds its newest state, copying it here");
    let keep = |relative: &str| tree::in_open_generation(relative, &generation_dir, lease::FILE);
    let (files, bytes) = tree::copy(&remote_dir, &local, &keep, true)?;
    log::info!("{run}: {} files, {} bytes copied from the remote", progress::grouped(files), progress::grouped(bytes));
    Ok(())
}

/// `run`'s closed generations still on this machine, for the mover: every one below its state's
/// (promoted), and its state's own once the lineage has stopped.
fn closed_generations(run: &str) -> Result<Vec<u32>, String> {
    let dirs = Dirs { root: run_dir(run) };
    let text = std::fs::read_to_string(dirs.state()).map_err(|error| format!("{}: {error}", dirs.state()))?;
    let state = GenState::parse(&text).map_err(|message| format!("{}: {message}", dirs.state()))?;
    let stopped = state.phase == Phase::Stopped;
    let last = if stopped { state.generation } else { state.generation - 1 };
    Ok((1..=last).filter(|&generation| Path::new(&dirs.generation(generation)).is_dir()).collect())
}

fn fail(message: String) -> ! {
    eprintln!("{PREFIX}: {message}");
    std::process::exit(1);
}

/// `smithy start`, `smithy stop`, `smithy status`. `start` runs in the foreground with no job, logging
/// its events (`progress::log_events`), until it is stopped; jobs are started and stopped from its page opened on this
/// machine. `stop` and `status`, from another terminal on the same machine, reach it over `127.0.0.1`.
pub(crate) fn run(verb: &str, rest: &[String]) {
    const USAGE: &str = "usage: smithy start | stop | status";
    match (verb, rest) {
        ("start", []) => start(),
        ("stop", []) => {
            let message = control_request(STOP_PATH).unwrap_or_else(|message| fail(message));
            println!("{PREFIX}: {}", if message.is_empty() { "stop asked: the smithy stops its job at its next safe point, then itself" } else { &message });
        }
        ("status", []) => print!("{}", control_request(STATUS_PATH).unwrap_or_else(|message| fail(message))),
        _ => fail(USAGE.to_string()),
    }
}

/// A control request to this machine's smithy, its answer's body.
fn control_request(path: &str) -> Result<String, String> {
    lan_http::control_request(SMITHY_PORT, path, "no smithy is running on this machine")
}

/// `smithy start`, on any machine: catches Ctrl+C, binds `127.0.0.1` and this machine's LAN address,
/// claims the arenas of the machines `data/machines.yaml` on the remote lists (`keep_claims`), serves
/// them, the page and the control, and runs each job the page asks for, one at a time, until
/// stopped. Only a stop ends it: a job that ends, or fails, leaves its result on the page until the
/// next one starts. Several smithies may run at once: an arena serves the smithy whose claim it
/// holds, so a second one only gets the arenas the first does not hold.
fn start() {
    progress::log_events();
    if !has_gpu() {
        log::warn!("{NO_GPU}: every job but a full run's training runs, scaled runs train on the CPU");
    }
    let machine = machine_name().unwrap_or_else(|message| fail(message));
    let machines = arenas_to_claim().unwrap_or_else(|message| fail(message));
    let rank = machines.rank(&machine);
    match rank {
        Some(rank) => log::info!("this smithy ranks {} of {} in {MACHINES_PATH}", rank + 1, machines.machines.len()),
        None => log::info!("this machine is not in {MACHINES_PATH}: its smithy ranks after every listed one"),
    }
    let targets = machines.machines;
    // Every lease this smithy takes asks the other machines' smithies first (`lease.rs`).
    lease::set_peers(&machine, &targets);
    let lan = Some(machine.clone());
    // Claimed as a full job's for now: a scaled job does not yet give way.
    let claim = Claim { smithy: machine.clone(), host: machine.clone(), boot: new_token(), commit: protocol::current_commit(), scaled: false, rank };
    let stop = Arc::new(AtomicBool::new(false));
    let job_stop = Arc::new(AtomicBool::new(false));
    let slot = Arc::new(SourceSlot::default());
    {
        let (stop, job_stop, slot) = (Arc::clone(&stop), Arc::clone(&job_stop), Arc::clone(&slot));
        ctrlc::set_handler(move || {
            log::info!("stopping the job at its next safe point, then the smithy; Ctrl+C again to stop at once");
            job_stop.store(true, Ordering::SeqCst);
            stop_slot(&slot);
            if stop.swap(true, Ordering::SeqCst) {
                std::process::exit(130);
            }
        })
        .unwrap_or_else(|error| fail(format!("cannot catch Ctrl+C: {error}")));
    }
    // A smithy already running here holds the port: the bind says so before anything starts.
    let loopback = Listener::bind(LOOPBACK, SMITHY_PORT)
        .unwrap_or_else(|message| fail(format!("{message}: is a smithy already running here? smithy status says")));
    let lan_listener = lan.as_deref().map(|host| Listener::bind(host, SMITHY_PORT).unwrap_or_else(|message| fail(message)));
    let page = format!("http://{}:{SMITHY_PORT}/", lan.as_deref().unwrap_or(LOOPBACK));
    let status = Arc::new(Status {
        shared: Arc::new(Mutex::new(last_job())),
        stop,
        job_stop,
        requested: Mutex::new(None),
        running: AtomicBool::new(false),
        control_token: new_token(),
        page: page.clone(),
        history: Mutex::new(HashMap::new()),
        slot,
        claims: Claims::new(claim, targets.clone()),
    });
    let listeners_stop = Arc::new(AtomicBool::new(false));
    let mut listener_threads = Vec::new();
    for (listener, control_here) in std::iter::once((loopback, true)).chain(lan_listener.map(|listener| (listener, false))) {
        let (status, listeners_stop) = (Arc::clone(&status), Arc::clone(&listeners_stop));
        listener_threads.push(std::thread::spawn(move || serve(listener, status, control_here, listeners_stop)));
    }
    // Stopped before the listeners close: its arenas are released while the smithy still answers.
    let claims_stop = Arc::new(AtomicBool::new(false));
    let claims_thread = {
        let (status, claims_stop) = (Arc::clone(&status), Arc::clone(&claims_stop));
        std::thread::spawn(move || keep_claims(&status, &claims_stop))
    };
    log::info!("up on {LOOPBACK} and {machine}, port {SMITHY_PORT}, with no job");
    log::info!("claiming the arenas on {}, every {} s; start each with bin/arena start", targets.join(", "), CLAIM_POLL.as_secs());
    log::info!("its page is {page}; jobs are started from it opened on this machine, http://{LOOPBACK}:{SMITHY_PORT}/");
    while !status.stop.load(Ordering::SeqCst) {
        let requested = status.requested.lock().expect("the lock should not be poisoned").take();
        match requested {
            Some(args) => run_job(&status, &args),
            None => std::thread::sleep(POLL),
        }
    }
    // The source slot's job stops with the smithy, at once: its end is said before the smithy's.
    stop_slot(&status.slot);
    for _ in 0..100 {
        if !status.slot.running.load(Ordering::SeqCst) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    claims_stop.store(true, Ordering::SeqCst);
    claims_thread.join().ok();
    listeners_stop.store(true, Ordering::SeqCst);
    for thread in listener_threads {
        thread.join().ok();
    }
}

/// The machines whose arenas a smithy claims, ranked, from `machines.yaml` on the remote, the one copy
/// every machine reads.
fn arenas_to_claim() -> Result<protocol::Machines, String> {
    remote::check_mounted().map_err(|error| error.to_string())?;
    protocol::machines(&remote::remote_path(MACHINES_PATH))
}

/// The name the page and the arenas know a job by, from the words that ask for it: a forge's run's
/// name (`players::run_name`), a new run's said so until it has a number; a tune's player's; a
/// source's job's.
fn job_name(args: &[String]) -> Result<String, String> {
    match args {
        [mode, config, run] if mode == "--forge" => Ok(match run.parse::<u32>() {
            Ok(number) => players::run_name(config, number),
            Err(_) => format!("{config}, a new run"),
        }),
        [mode, config, run, scaled] if mode == "--forge" && run == "new" && scaled == SCALED => {
            Ok(format!("{config}, a new run scaled 1/{}", scale::DEFAULT))
        }
        [mode, player] if mode == TUNE => Ok(format!("tune of {player}")),
        [mode, player] if mode == TIME_TUNE => Ok(format!("tune of {player}, time")),
        [mode, source] if mode == PLAY && source == PESTO => Ok("PeSTO's play".to_string()),
        [mode, source] if mode == CONVERT && source == "lc0" => Ok(SlotJob::Convert.name()),
        [mode, rest @ ..] if mode == BOOK => {
            book::parse_make_args(rest)?;
            Ok(SlotJob::of(args).expect("a book job's words, checked above").name())
        }
        [mode, source, side, depth] if mode == SCORE_BOOK => {
            let (side, depth) = score_book_words(source, side, depth)?;
            Ok(format!("the scores of {source}, {}, depth {depth}", side.text()))
        }
        [mode, words @ ..] if mode == TEST => Ok(format!("test: {}", matches::Spec::parse(words)?.name())),
        [mode, number] if mode == RESUME_TEST => Ok(format!("test {number}, resumed")),
        _ => Err(format!(
            "a job is --forge <config> <run number or new>, {TUNE} <player>, {TIME_TUNE} <player>, {TEST} <test>, {PLAY} {PESTO}, {CONVERT} lc0, {BOOK} <source> \
             or {SCORE_BOOK} <source> <network>@<suffix> <depth>"
        )),
    }
}

/// A test's job word (`matches::Spec::parse` reads the words after it).
const TEST: &str = "--test";
/// A stopped test's resume, by its number.
const RESUME_TEST: &str = "--resume-test";
/// The page's screen of the tests, and the config a test's job is of, so the page shows it there.
const TESTS_SCREEN: &str = "tests";

/// PeSTO's Play job's word: its self-play played into its corpus, in the arena slot.
const PLAY: &str = "--play";
/// Lc0's Convert job's word: its tars converted into its corpus, in the source slot, beside the
/// arena job.
const CONVERT: &str = "--convert";
/// A book job's word: a source's missing book stages, in the source slot, beside the arena job.
const BOOK: &str = "--book";
/// The page's screen of the books, and the config a book job is of, so the page shows it there.
const BOOKS_SCREEN: &str = "books";
/// A book's scoring's word: every move of a source's book searched on the arenas into a scores file
/// (`book_scores`), in the arena slot.
const SCORE_BOOK: &str = "--score-book";

/// A score job's words checked: a source with a graph's name, a side as a test's, a depth from 1 to
/// `MAX_SCORE_DEPTH`.
fn score_book_words(source: &str, side: &str, depth: &str) -> Result<(matches::Side, u32), String> {
    book::check_source(source)?;
    let side = matches::Side::parse(side)?;
    let depth = depth.parse::<u32>().ok().filter(|depth| (1..=MAX_SCORE_DEPTH).contains(depth)).ok_or(format!("{SCORE_BOOK}: the depth is 1 to {MAX_SCORE_DEPTH}, not {depth:?}"))?;
    Ok((side, depth))
}

/// The deepest scoring asked for: depth 20 costs about a second of one thread a move, and each
/// depth more about half again.
const MAX_SCORE_DEPTH: u32 = 30;

/// The config the job `args` asks for is of: a forge's own; pesto for pesto's tune and its Play;
/// lc0 for its Convert; the books' screen for a book job; for a network's tune, its run's config.
fn job_config(args: &[String]) -> Option<String> {
    match args {
        [mode, config, ..] if mode == "--forge" => Some(config.clone()),
        [mode, player] if tune_kind(mode).is_some() && player == PESTO => Some(PESTO.to_string()),
        [mode, player] if tune_kind(mode).is_some() => players::parse_network_name(player).ok().map(|parsed| parsed.config),
        [mode, source] if mode == PLAY || mode == CONVERT => Some(source.clone()),
        [mode, ..] if mode == BOOK || mode == SCORE_BOOK => Some(BOOKS_SCREEN.to_string()),
        [mode, ..] if mode == TEST || mode == RESUME_TEST => Some(TESTS_SCREEN.to_string()),
        _ => None,
    }
}

/// Runs the job `args` asks for on its own thread, so that neither its errors nor a panic end the
/// smithy, and leaves its result on the page. Each job starts from a fresh `Shared` with a new token:
/// the arenas say hello again, and a task of an earlier job is turned away by its token.
fn run_job(status: &Status, args: &[String]) {
    let name = job_name(args).unwrap_or_else(|message| message);
    status.job_stop.store(false, Ordering::SeqCst);
    *status.lock() = Shared { token: new_token(), job: Some(name.clone()), config: job_config(args), ..Shared::default() };
    let kind = match args.first().map(String::as_str) {
        Some("--forge") => "a forge",
        Some(PLAY) => "a source's corpus",
        Some(TEST | RESUME_TEST) => "a test",
        Some(SCORE_BOOK) => "a book's scoring",
        _ => "a tune",
    };
    log::info!("job {name} ({kind}) started");
    let result = std::thread::scope(|scope| scope.spawn(|| job(status, args)).join())
        .unwrap_or_else(|_| Err("the job panicked, as its message above says".to_string()));
    let line = match result {
        Ok(message) => message,
        Err(message) => format!("failed: {message}"),
    };
    // A new run is named once it has a number.
    let name = status.lock().job.clone().unwrap_or(name);
    log::info!("job {name}: {line}");
    {
        let mut shared = status.lock();
        shared.work = Some(Work::Idle);
        shared.epoch += 1;
        shared.finished = Some(line);
    }
    status.running.store(false, Ordering::SeqCst);
}

/// The last job started, kept for the page of the next smithy: its words, one a line.
const LAST_JOB_PATH: &str = "data/smithy-last.txt";

/// Records `words` as the last job: a forge's with its run's number, a new run's once it has one, so
/// the next smithy's page shows that run. The page's memory only, so a failure to write it is said
/// and nothing more.
fn remember_job(words: &[String]) {
    if let Err(error) = std::fs::write(LAST_JOB_PATH, words.join("\n") + "\n") {
        log::warn!("{LAST_JOB_PATH}: {error}; the next smithy's page will not show this job");
    }
}

/// The last job, as the page of a smithy just started shows it: ended, its phases or its tune read
/// from its files. A fresh `Shared` when there is none, or it cannot be read back.
fn last_job() -> Shared {
    let fresh = || Shared { token: new_token(), ..Shared::default() };
    let Ok(text) = std::fs::read_to_string(LAST_JOB_PATH) else {
        return fresh();
    };
    let args: Vec<String> = text.lines().filter(|line| !line.is_empty()).map(str::to_string).collect();
    let Ok(name) = job_name(&args) else {
        return fresh();
    };
    let mut shared =
        Shared {
            job: Some(name.clone()),
            config: job_config(&args),
            finished: Some("last job, before this smithy started".to_string()),
            ..fresh()
        };
    match args.as_slice() {
        [mode, _, _] if mode == "--forge" => {
            let Ok((config, net)) = load_run_config(&name) else {
                return fresh();
            };
            let files = ForgeFiles::of(&name, &config, &net);
            shared.state_path = Some(Dirs { root: files.root.clone() }.state());
            shared.forge = Some(files);
        }
        [mode, player] if tune_kind(mode).is_some() => {
            let kind = tune_kind(mode).expect("a tune's word, checked above");
            shared.tune = tune_args(player, kind).and_then(|args| Tune::open(&args)).ok();
        }
        // PeSTO's play: its corpus, on its page, says where it stands.
        [mode, _] if mode == PLAY => {}
        _ => return fresh(),
    }
    shared
}

/// One job of the arena slot, `--forge <config> <run>`, `--tune <player>` or `--play pesto`, until it
/// ends or `Status::job_stop` is raised.
fn job(status: &Status, args: &[String]) -> Result<String, String> {
    match args {
        [mode, config, run] if mode == "--forge" => forge(status, config, run, None),
        [mode, config, run, scaled] if mode == "--forge" && scaled == SCALED => forge(status, config, run, Some(scale::DEFAULT)),
        [mode, player] if tune_kind(mode).is_some() => tune_job(status, player, tune_kind(mode).expect("a tune's word, checked above")),
        [mode, source] if mode == PLAY && source == PESTO => play_pesto(status),
        [mode, words @ ..] if mode == TEST => test_job(status, words),
        [mode, number] if mode == RESUME_TEST => resume_test_job(status, number),
        [mode, source, side, depth] if mode == SCORE_BOOK => score_book_job(status, source, side, depth),
        _ => Err(job_name(args).err().unwrap_or_else(|| format!("{} is no job of the arena slot", args.join(" ")))),
    }
}

/// `--play pesto`: PeSTO's self-play dealt to the arenas into its corpus (`pesto.rs`), the test games
/// first, then the training games, to `configs/pesto.yaml`'s sizes, resumable. Each segment is made
/// locally, in the day directory of the start, and moved to the remote once closed: at the start
/// (what a stopped play left), when stopped, and when done, when `complete.txt` follows.
fn play_pesto(status: &Status) -> Result<String, String> {
    let config = pesto::load()?;
    remote::check_mounted().map_err(|error| error.to_string())?;
    if pesto::is_complete() {
        return Ok(format!("{} is complete already: nothing to play", Source::Pesto.corpus()));
    }
    // One smithy at a time adds to the corpus.
    // PeSTO plays its tune (the developer, 2026-11-08), refused while it has none: a corpus played
    // with other parameters is set aside by hand first.
    let parameters = required_tune(PESTO, "PeSTO's play plays")?;
    let lease_path = remote::remote_path(&format!("{}/{}", preprocessing_config::PESTO_ROOT, lease::FILE));
    let _lease = lease::take(&lease_path, "PeSTO's play")?;
    remember_job(&[PLAY.to_string(), PESTO.to_string()]);
    let (test_root, train_root) = (pesto::test_dir(), pesto::train_dir());
    for root in [&test_root, &train_root] {
        settle_segments(root)?;
    }
    let (test_done, train_done) = (pesto::remote_positions(&test_root)?, pesto::remote_positions(&train_root)?);
    let test_left = config.test_positions.saturating_sub(test_done);
    let train_left = config.training_positions.saturating_sub(train_done);
    let now = time::OffsetDateTime::now_utc();
    let day = format!("{:04}{:02}{:02}", now.year(), u8::from(now.month()), now.day());
    let (test_dir, train_dir) = (segments::day_dir(&test_root, &day), segments::day_dir(&train_root, &day));
    log::info!(
        "playing: PeSTO's self-play into its corpus, PeSTO's tune, test set {test_done} of {} positions, training set {train_done} of {}",
        config.test_positions, config.training_positions
    );
    let local_root = preprocessing_config::PESTO_ROOT;
    std::fs::create_dir_all(local_root).map_err(|error| format!("{local_root}: {error}"))?;
    begin(&status.shared, &format!("{local_root}/chores.txt"), None)?;
    let started = Instant::now();
    let (test_segment, train_segment) = (SegmentWriter::open(&test_dir)?, SegmentWriter::open(&train_dir)?);
    let work = Work::SelfPlay {
        teacher: PESTO.to_string(),
        parameters,
        test_dir,
        test_left,
        train_dir,
        train_left,
        before: (test_done, train_done),
        targets: (config.test_positions, config.training_positions),
    };
    let ended = deal_self_play(&status.shared, &status.job_stop, work, test_segment, train_segment)?;
    // Stopped, the open files are closed too: their whole games go to the remote, and a resume
    // starts new files.
    for root in [&test_root, &train_root] {
        settle_segments(root)?;
    }
    if let Ended::Stopped = ended {
        return Ok("stopped; start it again to resume".to_string());
    }
    let (test, train) = (pesto::remote_positions(&test_root)?, pesto::remote_positions(&train_root)?);
    pesto::write_complete(test, train)?;
    Ok(format!("{} complete in {}: {test} test and {train} training positions", Source::Pesto.corpus(), minutes(started)))
}

/// Closes every file left open in the day directories under `root`, locally, and moves every closed
/// segment to `root`'s remote counterpart: its `.pgn.zst`, then its `.meta`, then removes them
/// locally.
fn settle_segments(root: &str) -> Result<(), String> {
    let local_root = root.to_string();
    let Ok(entries) = std::fs::read_dir(&local_root) else {
        return Ok(());
    };
    let mut days: Vec<String> = entries.flatten().filter_map(|entry| entry.file_name().into_string().ok()).filter(|name| segments::is_day(name)).collect();
    days.sort();
    for day in days {
        let dir = format!("{local_root}/{day}");
        SegmentWriter::open(&dir)?.close()?;
        let remote_dir = remote::remote_path(&format!("{root}/{day}"));
        remote::copy_files(&dir, &remote_dir, &|name| remote::is_segment_file(name)).map_err(|error| error.to_string())?;
        for entry in std::fs::read_dir(&dir).map_err(|error| format!("{dir}: {error}"))?.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if remote::is_segment_file(&name) {
                std::fs::remove_file(entry.path()).map_err(|error| format!("{dir}/{name}: {error}"))?;
            }
        }
        std::fs::remove_dir(&dir).ok();
    }
    Ok(())
}

/// The job's chore counter and state file: chore ids continue from `chores_path`, and a training
/// run's state is read from `state_path` for the page.
fn begin(shared: &Mutex<Shared>, chores_path: &str, state_path: Option<String>) -> Result<(), String> {
    let next_chore = match std::fs::read_to_string(chores_path) {
        Ok(text) => text.trim().parse().map_err(|_| format!("{chores_path}: {text:?} is not a chore number"))?,
        Err(_) => 0,
    };
    let mut shared = shared.lock().expect("the lock should not be poisoned");
    shared.next_chore = next_chore;
    shared.chores_path = Some(chores_path.to_string());
    shared.state_path = state_path;
    Ok(())
}

/// `--tune <player>` or `--time-tune <player>`: a tune (`tune.rs`) of `kind` of `player` as its
/// config's `tune:` says (`tune_args`), its pairs dealt to the arenas until it is done or stopped,
/// then its result written to the player's `search-parameters.txt` or `time-parameters.txt`. One
/// tune of each kind a player: an unfinished one resumes, a finished one is refused, since retuning
/// is not supported yet.
fn tune_job(status: &Status, player: &str, kind: TuneKind) -> Result<String, String> {
    // The arenas read the tuned network from the remote, and the tune's files go there.
    remote::check_mounted().map_err(|error| error.to_string())?;
    let args = tune_args(player, kind)?;
    status.lock().scaled_job = args.scale.is_some();
    // Taken before the tune opens, which fetches the remote's copy when it is further on.
    let _lease = lease::take(&tune::lease_path(player, kind)?, player)?;
    let tune = Tune::open(&args)?;
    if tune.done() {
        return Err(format!(
            "{player}'s {} is tuned already, in {}: retuning is not supported yet; move or delete that folder by hand to tune it again",
            kind.text(),
            tune.dir()
        ));
    }
    remember_job(&tune_words(player, kind));
    let network = match &tune.network {
        Some(path) => Some(fattorini::nnue::Network::load(path).map_err(|error| format!("failed to load network {path}: {error}"))?),
        None => None,
    };
    // The speed every arena's clock is scaled to: this machine's, loaded as an arena's would be.
    let reference_nps = match tune.limit() {
        Limit::Clock(_) => {
            let threads = (std::thread::available_parallelism().map_or(1, |cores| cores.get()) / 2).max(1);
            log::info!("benching this machine on {threads} threads, the arenas' reference speed");
            let nps = loaded_bench(network.as_ref(), threads);
            log::info!("bench: {nps:.0} nodes per second per thread");
            Some(nps)
        }
        Limit::Depth(_) => None,
    };
    begin(&status.shared, &format!("{}/chores.txt", tune.dir()), None)?;
    status.lock().reference_nps = reference_nps;
    run_tune(&status.shared, tune, &status.job_stop)
}

/// One side of a test as its chores name it: its network by its name on the remote (`publish`) or
/// `PESTO`, the parameters it plays with in full, its time parameters in full (`None`, the default
/// clock), and its network loaded, for the bench.
struct TestPlayer {
    name: String,
    parameters: SearchParameters,
    time: Option<TimeParameters>,
    network: Option<fattorini::nnue::Network>,
}

/// `side` resolved: a forge run's network or PeSTO, with the tunes its suffix names, which it must
/// then have (the developer, 2026-11-13: refused, never the defaults quietly).
fn test_player(side: &matches::Side) -> Result<TestPlayer, String> {
    let player = players::FattoriniPlayer::resolve(&side.player)?;
    let parameters = match (side.tuning.search_tuned(), player.own_tuning()?) {
        (false, _) => SearchParameters::DEFAULT,
        (true, Tuning::Parameters(parameters)) => parameters,
        (true, Tuning::Untuned) => return Err(format!("{} has no search tune: it can only play @untuned", side.player)),
    };
    let time = if side.tuning.time_tuned() {
        Some(player.own_time_tuning()?.ok_or_else(|| format!("{} has no time tune: no @search-time-tuned", side.player))?)
    } else {
        None
    };
    let (name, network) = match &player.network {
        Some(path) => {
            let network = fattorini::nnue::Network::load(path).map_err(|error| format!("failed to load network {path}: {error}"))?;
            (publish(path)?, Some(network))
        }
        None => (PESTO.to_string(), None),
    };
    Ok(TestPlayer { name, parameters, time, network })
}

/// How often a scoring's file is copied to the remote, beside its end.
const SCORES_MIRROR_EVERY: Duration = Duration::from_secs(5 * 60);

/// `parameters` as one header value, `name=value` pairs joined by commas, as a chore's player writes
/// them.
fn parameters_text(parameters: &SearchParameters) -> String {
    SearchParameters::NAMES.iter().zip(parameters.values()).map(|(name, value)| format!("{name}={value}")).collect::<Vec<_>>().join(",")
}

/// `--score-book <source> <side> <depth>`: every move of `source`'s book still without a score in
/// its scores file searched on the arenas to `depth` by `side` (`book_scores`), each record checked
/// and appended, the file copied to the remote every `SCORES_MIRROR_EVERY` and at the end. A build
/// with uncommitted changes is refused: its scores could not name the code that made them.
fn score_book_job(status: &Status, source: &str, side: &str, depth: &str) -> Result<String, String> {
    let (side, depth) = score_book_words(source, side, depth)?;
    remote::check_mounted().map_err(|error| error.to_string())?;
    let commit = protocol::current_commit();
    if commit == "unknown" || commit.contains("(modified)") {
        return Err(format!("this build is {commit}: commit first, so the scores name the code that made them"));
    }
    let player = test_player(&side)?;
    let (graph_header, moves) = book::book_moves(source)?;
    let graph_value = |name: &str| graph_header.iter().find(|(held, _)| held == name).map_or(String::new(), |(_, value)| value.clone());
    log::info!("benching {} at depth {}, one thread: every arena's search must count the same nodes", side.text(), fattorini::bench::BENCH_DEPTH);
    let bench_nodes = fattorini::bench::bench_with(player.network.as_ref(), player.parameters, fattorini::bench::BENCH_DEPTH).0;
    log::info!("bench: {bench_nodes} nodes");
    let header: Vec<(String, String)> = [
        ("source", source.to_string()),
        ("graph_min_games", graph_value("min_games")),
        ("graph_built", graph_value("built")),
        ("book_moves", moves.len().to_string()),
        ("network", player.name.clone()),
        ("side", side.text()),
        ("parameters", parameters_text(&player.parameters)),
        ("commit", commit.clone()),
        ("depth", depth.to_string()),
        ("max_nodes", protocol::SCORE_MAX_NODES.to_string()),
        ("threads", "1".to_string()),
        ("hash_mb", protocol::SCORE_HASH_MB.to_string()),
        ("tablebases", "none".to_string()),
        ("bench_nodes", bench_nodes.to_string()),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_string(), value))
    .collect();
    let path = format!("{}/{}", book::folder(source), book_scores::file_name(&side.player, side.tuning, depth));
    // The remote's copy is the scoring so far, after a stop or on another machine.
    let remote_copy = remote::remote_path(&path);
    if !Path::new(&path).is_file() && Path::new(&remote_copy).is_file() {
        remote::copy_replacing(&remote_copy, &path).map_err(|error| error.to_string())?;
    }
    let file = book_scores::ScoresFile::open(&path, &header)?;
    let total = moves.len() as u64;
    let pending: Vec<book::BookMove> = moves.into_iter().filter(|book_move| !file.scored.contains(&(book_move.parent, book_move.book_move))).collect();
    let before = total - pending.len() as u64;
    let mirror = || remote::copy_replacing(&path, &remote_copy).map_err(|error| error.to_string());
    if pending.is_empty() {
        mirror()?;
        return Ok(format!("{path}: every one of the book's {} moves scored already", progress::grouped(total)));
    }
    begin(&status.shared, &format!("{}/chores.txt", book::folder(source)), None)?;
    log::info!(
        "scoring {source}'s book with {} at depth {depth}: {} of {} moves to go, into {path}",
        side.text(),
        progress::grouped(pending.len()),
        progress::grouped(total)
    );
    {
        let mut shared = status.lock();
        shared.epoch += 1;
        shared.score = Some(ScoreDealing {
            source: source.to_string(),
            side: side.text(),
            network: player.name,
            parameters: player.parameters,
            depth,
            commit,
            bench_nodes,
            free: (0..pending.len() as u32).collect(),
            done: vec![false; pending.len()],
            moves: pending,
            done_count: 0,
            total,
            before,
            file,
            rejected: 0,
            refused: Vec::new(),
            started: Instant::now(),
        });
        shared.work = Some(Work::Score);
    }
    let mut mirrored = Instant::now();
    let complete = loop {
        let complete = status.lock().score.as_ref().is_some_and(ScoreDealing::complete);
        if complete || status.job_stop.load(Ordering::SeqCst) {
            break complete;
        }
        if mirrored.elapsed() >= SCORES_MIRROR_EVERY {
            mirrored = Instant::now();
            if let Err(message) = mirror() {
                log::warn!("the scores not copied to {}: {message}", remote::this_root());
            }
        }
        std::thread::sleep(POLL);
    };
    let done = {
        let mut shared = status.lock();
        shared.work = Some(Work::Idle);
        shared.epoch += 1;
        shared.score.as_ref().map_or(0, |score| score.before + score.done_count)
    };
    mirror()?;
    let said = format!("{} of the book's {} moves scored, in {remote_copy}", progress::grouped(done), progress::grouped(total));
    Ok(if complete { said } else { format!("stopped, {said}; start it again to go on") })
}

/// The time of day a test's log records, in UTC.
fn log_time() -> String {
    let now = time::OffsetDateTime::now_utc();
    format!("{:04}-{:02}-{:02} {:02}:{:02} UTC", now.year(), u8::from(now.month()), now.day(), now.hour(), now.minute())
}

/// `--test <words>`: a new test, numbered past every kept one, the oldest past `matches::KEPT`
/// going with their games first (`play_test`).
fn test_job(status: &Status, words: &[String]) -> Result<String, String> {
    let spec = matches::Spec::parse(words)?;
    unscaled(&spec.challenger.player)?;
    unscaled(&spec.champion.player)?;
    let dir = matches::TESTS_DIR.to_string();
    std::fs::create_dir_all(&dir).map_err(|error| format!("{dir}: {error}"))?;
    // Room for this one among the kept.
    matches::prune(&dir, matches::KEPT - 1)?;
    let number = matches::next_number(&dir);
    play_test(status, &dir, &spec, number, log_time(), Vec::new())
}

/// `--resume-test <number>`: a stopped test still kept, from its recorded pairs, under its own number
/// and start; its log line replaced when it ends.
fn resume_test_job(status: &Status, number: &str) -> Result<String, String> {
    let number: u32 = number.parse().map_err(|_| format!("{number:?} is no test's number"))?;
    let dir = matches::TESTS_DIR.to_string();
    let record = matches::read_log(&dir).into_iter().find(|record| record.number == number).ok_or_else(|| format!("test {number} is not kept"))?;
    if !record.stopped() {
        return Err(format!("test {number} ended: {}; only a stopped test resumes", record.verdict));
    }
    let spec = record.spec()?;
    let pairs = recorded_pairs(&matches::pairs_path(&dir, number), 1)?.remove(0);
    play_test(status, &dir, &spec, number, record.started, pairs)
}

/// Test `number` of `spec` (`matches.rs`), in `dir`, started at `started`, from `pairs` already
/// played: its pairs dealt to the arenas as a gate's are, one pairing, the challenger first, until its
/// rule decides or it is stopped; then logged, its line replacing any it had. A stopped test is logged
/// as stopped, with what it had, and may be resumed while it is kept.
fn play_test(status: &Status, dir: &str, spec: &matches::Spec, number: u32, started: String, pairs: Vec<i32>) -> Result<String, String> {
    let resumed_games = 2 * pairs.len() as u64;
    // The arenas read the networks from the remote.
    remote::check_mounted().map_err(|error| error.to_string())?;
    let challenger = test_player(&spec.challenger)?;
    let champion = test_player(&spec.champion)?;
    let reference_nps = match spec.limit {
        Limit::Clock(_) => {
            let threads = (std::thread::available_parallelism().map_or(1, |cores| cores.get()) / 2).max(1);
            log::info!("benching this machine on {threads} threads with the challenger, the arenas' reference speed");
            let nps = loaded_bench(challenger.network.as_ref(), threads);
            log::info!("bench: {nps:.0} nodes per second per thread");
            Some(nps)
        }
        Limit::Depth(_) => None,
    };
    let record = matches::pairs_path(dir, number);
    begin(&status.shared, &format!("{dir}/chores.txt"), None)?;
    mark_start(&record)?;
    // Logged at once as interrupted, replaced by its final line when it ends: a test that dies
    // without ending (a power cut, a crash) stays listed and resumable, and `prune` never removes
    // its files unseen.
    let at_start = matches::judge(spec.kind, &pairs);
    matches::put(dir, &test_record(spec, number, &started, at_start.games, at_start.challenger_points, matches::INTERRUPTED.to_string()))?;
    let mixed = if spec.mixed() { ", the two sides tuned differently" } else { "" };
    let resumed = if resumed_games > 0 { format!(", resumed at {} games", progress::grouped(resumed_games)) } else { String::new() };
    log::info!(
        "test {number}: {} ({}), {} against {}, {}{mixed}{resumed}",
        spec.kind.name(),
        spec.kind.settings(),
        spec.challenger.text(),
        spec.champion.text(),
        spec.limit.text()
    );
    {
        let mut shared = status.lock();
        shared.reference_nps = reference_nps;
        shared.test = Some(TestDealing {
            limit: spec.limit.clone(),
            parameters: [challenger.parameters, champion.parameters],
            time: [challenger.time, champion.time],
        });
        shared.view = View::Test { number, spec: spec.clone() };
        shared.epoch += 1;
        shared.pairs = vec![pairs];
        shared.pair_wdl = recorded_wdl(&record, 1);
        shared.pair_times = recorded_times(&record);
        shared.work = Some(Work::Pairs {
            pairings: vec![(challenger.name, champion.name)],
            wanted: spec.kind.max_pairs(),
            record: record.clone(),
            sides: [challenger.parameters, champion.parameters],
        });
    }
    // Its progress, bar and gauge, is the page's, drawn from its view (`test_view`).
    let (judged, stopped) = loop {
        let pairs = status.lock().pairs.first().cloned().unwrap_or_default();
        let judged = matches::judge(spec.kind, &pairs);
        if judged.verdict.is_some() {
            break (judged, false);
        }
        if status.job_stop.load(Ordering::SeqCst) {
            break (judged, true);
        }
        std::thread::sleep(POLL);
    };
    {
        let mut shared = status.lock();
        shared.work = Some(Work::Idle);
        shared.epoch += 1;
        shared.test = None;
    }
    let verdict = match (&judged.verdict, stopped) {
        (Some(verdict), _) => verdict.clone(),
        (None, _) => format!("stopped: {}", judged.status),
    };
    matches::put(dir, &test_record(spec, number, &started, judged.games, judged.challenger_points, verdict.clone()))?;
    Ok(format!(
        "test {number}: {verdict}, after {} games, the champion {} of them{mixed}",
        judged.games,
        points_text(judged.games as f64 - judged.challenger_points)
    ))
}

/// Test `number`'s log line, ended now.
fn test_record(spec: &matches::Spec, number: u32, started: &str, games: u64, challenger_points: f64, verdict: String) -> matches::Record {
    matches::Record {
        number,
        started: started.to_string(),
        ended: log_time(),
        kind: spec.kind.name().to_string(),
        settings: spec.kind.settings(),
        challenger: spec.challenger.text(),
        champion: spec.champion.text(),
        limit: spec.limit.text(),
        games,
        challenger_points,
        verdict,
        mixed: spec.mixed(),
    }
}

/// How often a tune's files that change along it are copied to the remote (`Tune::mirror`), beside its
/// end.
const TUNE_MIRROR_EVERY: Duration = Duration::from_secs(15 * 60);

/// Deals `tune` until it is done or `stop` is raised, its pairs counted for the page, its files
/// copied to the remote every `TUNE_MIRROR_EVERY`; done, its result written and its folder moved there.
fn run_tune(shared: &Arc<Mutex<Shared>>, tune: Tune, stop: &AtomicBool) -> Result<String, String> {
    let network = match &tune.network {
        Some(path) => publish(path)?,
        None => PESTO.to_string(),
    };
    log::info!(
        "tuning {} in {}, {} of {} pairs done, {}, early stop on",
        tune.player,
        tune.dir(),
        tune.k_done,
        tune.pairs,
        tune.limit().text()
    );
    {
        let mut shared = shared.lock().expect("the lock should not be poisoned");
        shared.epoch += 1;
        shared.tune = Some(tune);
        shared.work = Some(Work::Tune { network });
        // Its times, as a pair phase's but kept in memory alone: a resume's pace is measured afresh.
        shared.pair_times = vec![(unix_now(), false)];
    }
    // Its progress is its box's, drawn from its view (`WorkView::Tune`).
    let mut mirrored = Instant::now();
    loop {
        let done = {
            let shared = shared.lock().expect("the lock should not be poisoned");
            let tune = shared.tune.as_ref().expect("the tune is there");
            if mirrored.elapsed() >= TUNE_MIRROR_EVERY {
                mirrored = Instant::now();
                if let Err(message) = tune.mirror() {
                    log::warn!("the tune's files not copied to {}: {message}", remote::this_root());
                }
            }
            tune.done()
        };
        if done || stop.load(Ordering::SeqCst) {
            break;
        }
        std::thread::sleep(POLL);
    }
    let mut shared = shared.lock().expect("the lock should not be poisoned");
    shared.work = Some(Work::Idle);
    shared.epoch += 1;
    let tune = shared.tune.as_ref().expect("the tune is there");
    if !tune.done() {
        tune.mirror()?;
        return Ok("stopped; start it again to resume".to_string());
    }
    tune.finish()?;
    tune.hand_over()?;
    Ok(format!("{} pairs done, theta written to {}, its folder moved to {}", progress::grouped(tune.pairs), tune.parameters_path(), remote::this_root()))
}

/// Whether libtorch sees a CUDA device here, probed once. Training would otherwise fall back to the
/// CPU without a word (`Device::cuda_if_available`), so a full run, which trains every generation,
/// is refused without one; a scaled run trains on the CPU, as every other job runs anywhere.
fn has_gpu() -> bool {
    static GPU: OnceLock<bool> = OnceLock::new();
    *GPU.get_or_init(tch::Cuda::is_available)
}

/// Whether a run of `scale` (`GenConfig::scale`) needs the GPU this machine lacks: a full one.
fn lacks_gpu(scale: Option<u64>) -> bool {
    scale.is_none() && !has_gpu()
}

/// Whether a run of `scale` at `phase` needs the GPU this machine lacks (`gpu` false): a full run's
/// training alone. Self-play, the round robin and the gate are played by the arenas on `.nnue` files,
/// so they run anywhere, and a run can play here and train elsewhere.
fn needs_gpu_now(scale: Option<u64>, phase: Phase, gpu: bool) -> bool {
    scale.is_none() && phase == Phase::Train && !gpu
}

/// Why a forge is refused on a machine without `has_gpu`, as the page says it too.
const NO_GPU: &str = "no CUDA GPU on this machine: training is not available here";

/// How often a training's state is copied to the remote (`Run::copy_up`): a crash loses at most
/// this much, and nine lambdas' weights and optimizers every round would be much for a home upload.
const COPY_UP_EVERY: Duration = Duration::from_secs(30 * 60);

/// The word after `--forge <config> new` that asks for a scaled run, at `scale::DEFAULT`.
const SCALED: &str = "scaled";

/// Run `run`'s lease key (`lease.rs`), `run` a run's name.
fn run_lease_path(run: &str) -> String {
    remote::remote_path(&format!("{}/{}", run_dir(run), lease::FILE))
}

/// Run `run` of config `name`, a number or `new`; `scale`, a new run's `1/s` (`GenConfig::scale`),
/// none for a full one and for an existing run, which keeps the scale it started with.
fn forge(status: &Status, name: &str, run: &str, scale: Option<u64>) -> Result<String, String> {
    // A full run needs the GPU; a scaled one trains on the CPU, that being what it is for.
    if run == "new" && lacks_gpu(scale) {
        return Err(NO_GPU.to_string());
    }
    if scale.is_some() && run != "new" {
        return Err(format!("{name} run {run}: a run keeps the scale it started with, so only a new run takes one"));
    }
    if scale == Some(0) {
        return Err("a scale is a whole number from 1".to_string());
    }
    if players::is_run_suffixed(name) {
        return Err(format!("{name}: a config's name may not end in -r<digits>, which reads as a run's number; rename it"));
    }
    // A new run is refused before its directory is made, so a refusal leaves no empty run behind.
    if run == "new" {
        let (config, _) = load_config(&preprocessing_config::net_config_path(name))?;
        check_forge_name(name, config.source)?;
        // A scaled run reads a share of its source, so starts on what is there.
        if let Some(source) = config.source.filter(|source| scale.is_none() && !source.is_complete()) {
            return Err(source.incomplete());
        }
    }
    // The arenas read every network from the remote, and the run's files go there.
    remote::check_mounted().map_err(|error| error.to_string())?;
    // A run started in another smithy may be on the remote alone, and a new one never takes its number.
    let (runs, remote_runs) = (runs_of(name), players::runs_in(&remote::remote_path(&players::config_dir(name))));
    let number = match run {
        "new" => next_run_number(&runs, &remote_runs),
        number => number
            .parse::<u32>()
            .ok()
            .filter(|number| runs.contains(number) || remote_runs.contains(number))
            .ok_or(format!("{name} has no run {number}"))?,
    };
    let run_name = players::run_name(name, number);
    let dirs = Dirs { root: run_dir(&run_name) };
    // Held until the job ends: no other smithy runs this run meanwhile (`lease.rs`). Taken before
    // anything is read, so nothing read is another smithy's half-written state.
    let _lease = lease::take(&run_lease_path(&run_name), &run_name)?;
    if run != "new" {
        sync_down(&run_name)?;
    }
    // A run's state is written as it starts, so one without it, here and on the remote, has lost it:
    // starting it over at generation 1 would overwrite what it had, here and, through the mover, on
    // the remote.
    if run != "new" && !Path::new(&dirs.state()).is_file() {
        return Err(format!(
            "{run_name} has no {}, here or on the remote: refused rather than started over at generation 1; delete its directory to start afresh",
            dirs.state()
        ));
    }
    if run != "new"
        && let Some(state) = run_state(&dirs)
        && needs_gpu_now(load_run_config(&run_name)?.0.scale, state.phase, has_gpu())
    {
        return Err(NO_GPU.to_string());
    }
    std::fs::create_dir_all(&dirs.root).map_err(|error| format!("{}: {error}", dirs.root))?;
    freeze_run_config(&run_name, name, scale)?;
    let (config, net) = load_run_config(&run_name)?;
    check_forge_name(name, config.source)?;
    {
        let mut shared = status.lock();
        shared.forge_depth = Some(config.depth());
        shared.scaled_job = config.scale.is_some();
    }
    // A sourced run's generation 1 trains on its source's whole corpus: refused until it is complete.
    // A scaled run reads a share of it, so starts on what is there.
    if let Some(source) = config.source.filter(|_| run_state(&dirs).is_none_or(|state| state.generation == 1))
        && config.scale.is_none()
        && !source.is_complete() {
            return Err(source.incomplete());
        }
    status.lock().job = Some(run_name.clone());
    remember_job(&["--forge".to_string(), name.to_string(), number.to_string()]);
    log::info!("run {number} of {name}, {run_name}, in {}", dirs.root);
    let mut state = match std::fs::read_to_string(dirs.state()) {
        Ok(text) => GenState::parse(&text).map_err(|message| format!("{}: {message}", dirs.state()))?,
        Err(_) => {
            let state = GenState {
                generation: 1,
                // Generation 1 trains on its source's corpus, already made.
                phase: Phase::Train,
                // A polyline run has no grid: its centre is never read.
                center_hundredths: (config.grid_center.unwrap_or(0.5) * 100.0).round() as i32,
                step_hundredths: STEP_HUNDREDTHS,
            };
            write_state(&dirs, &state)?;
            std::fs::write(dirs.log(), "generation\twinner\tgate_games\tgate_share\tpassed\n")
                .map_err(|error| format!("{}: {error}", dirs.log()))?;
            state
        }
    };
    begin(&status.shared, &dirs.chores(), Some(dirs.state()))?;
    status.lock().forge = Some(ForgeFiles::of(&run_name, &config, &net));

    // Every run's closed generations, not only this one's: a lineage stopped long ago is moved by
    // whichever run starts next.
    let archiver = Archiver::start(archive::Hooks { closed: closed_generations, say }, Arc::clone(&status.job_stop));
    let run = Run {
        name: run_name,
        config: &config,
        net: &net,
        dirs,
        shared: Arc::clone(&status.shared),
        stop: Arc::clone(&status.job_stop),
        archiver: &archiver,
    };
    let result = run_generations(&run, &mut state);
    // Before the lease goes, everything another smithy needs to go on: a planned move loses nothing.
    if let Err(message) = run.copy_up(state.generation) {
        log::warn!("{}: its files not all copied to {}: {message}", run.name, remote::this_root());
    }
    drop(run);
    // With nothing left to do, the move under way is let finish, else a stopped lineage would never
    // be moved by itself.
    archiver.finish(say);
    result
}

fn run_generations(run: &Run, state: &mut GenState) -> Result<String, String> {
    let stopped = || "stopped; start it again to resume".to_string();
    loop {
        if state.phase == Phase::Done && !engine_plays(run.net) {
            return Ok(
                "this run is finished: the engine cannot play a transformer yet, so it has no round robin; compare its \
                 lambdas' history.tsv, or train it again as a new run"
                    .to_string(),
            );
        }
        // A lineage already stopped stays stopped: replaying its last gate from the records would
        // log it twice.
        if state.phase == Phase::Stopped {
            return Ok(format!(
                "generation {}: the lineage stopped at its failed gate (generations.tsv has it); start a new run to begin again",
                state.generation
            ));
        }
        // A polyline run trains one arm every generation; a grid run its grid's.
        let arms = arms_of(run.config, state);
        let training_set = if let Some(source) = run.config.on_source(state.generation) {
            source.corpus().to_string()
        } else {
            format!("{} positions", progress::grouped(run.config.training_positions()))
        };
        log::info!("generation {}: lambda {}, {training_set}", state.generation, lambdas_text(&arms));
        if state.phase == Phase::Play {
            if let Ended::Stopped = run.play(state)? {
                return Ok(stopped());
            }
            state.phase = Phase::Train;
            run.boundary(state)?;
            if needs_gpu_now(run.config.scale, state.phase, has_gpu()) {
                return Ok(format!(
                    "generation {}: self-play done; training waits for Resume on a machine with CUDA",
                    state.generation
                ));
            }
        }
        if state.phase == Phase::Train {
            if let Ended::Stopped = run.train(state, &arms)? {
                return Ok(stopped());
            }
            // A network the engine cannot play ends at its training: no round robin, no winner.
            state.phase = if engine_plays(run.net) { Phase::RoundRobin } else { Phase::Done };
            run.boundary(state)?;
            if state.phase == Phase::Done {
                continue;
            }
        }
        // A lambda whose best checkpoint is step 0 fell back to its teacher: nothing it trained beat the
        // teacher's own weights, so its candidate is the teacher. It plays no round robin, whose copies
        // of the teacher would only add noise, and a winner among them would meet itself at the gate;
        // with none left, the generation learned nothing, a failed gate without a game (the developer,
        // 2026-10-30).
        let learned = learned_arms(&run.dirs, state.generation, &arms);
        let (winner, pass, share, gate_games) = if learned.is_empty() {
            log::warn!(
                "generation {}: every lambda fell back to the teacher, its best checkpoint step 0; no round robin, no gate",
                state.generation
            );
            (None, false, 0.0, 0)
        } else {
            let fell_back = arms.len() - learned.len();
            if fell_back > 0 {
                log::info!("{fell_back} of {} lambdas fell back to the teacher and play no round robin", arms.len());
            }
            let winner = if learned.len() == 1 {
                log::info!("one lambda learned, {}: no round robin, straight to the gate", learned[0]);
                learned[0]
            } else {
                // Two arms or more are a grid's: a polyline run trains one.
                let constants: Vec<f64> = learned.iter().map(|arm| arm.constant().expect("a grid's arm is a constant")).collect();
                let (ended, winner) = run.round_robin(state, &constants)?;
                if let Ended::Stopped = ended {
                    return Ok(stopped());
                }
                ArmLambda::Constant(winner)
            };
            state.phase = Phase::Gate;
            run.boundary(state)?;
            let (ended, pass, share) = run.gate(state, winner)?;
            if let Ended::Stopped = ended {
                return Ok(stopped());
            }
            let gate_games = std::fs::read_to_string(run.dirs.gate(state.generation))
                .map_or(0, |text| text.lines().filter(|line| !line.starts_with('#')).count() * 2);
            (Some(winner), pass, share, gate_games)
        };
        let winner_text = winner.map_or("none".to_string(), |winner| winner.to_string());
        std::fs::OpenOptions::new()
            .append(true)
            .open(run.dirs.log())
            .and_then(|mut file| writeln!(file, "{}\t{winner_text}\t{gate_games}\t{share:.4}\t{pass}", state.generation))
            .map_err(|error| format!("{}: {error}", run.dirs.log()))?;

        // A scaled run checks the pipeline, not strength: a network trained on a sliver of the data may
        // well lose its gate, so its winner teaches the next generation all the same, the failure
        // recorded in generations.tsv (the developer, 2026-10-30). A generation that learned nothing
        // has no winner to promote.
        let scaled_on = !pass && run.config.scale.is_some() && winner.is_some();
        let nothing_learned = winner.is_none();
        let winner = winner.unwrap_or(ArmLambda::Constant(0.0));
        if pass || scaled_on {
            // The winner becomes the next generation's teacher: its candidate network, and the
            // checkpoint it was exported from, for the next generation's warm start.
            let lambda_dir = run.dirs.arm(state.generation, winner);
            let step = candidates::read_candidate_step(&lambda_dir)?;
            let checkpoint = checkpoint_store::checkpoint_path(&format!("{lambda_dir}/checkpoints"), step);
            let next = state.generation + 1;
            std::fs::create_dir_all(run.dirs.generation(next)).map_err(|error| format!("{}: {error}", run.dirs.generation(next)))?;
            copy(&checkpoint, &run.dirs.teacher_vs(next))?;
            copy(&generation::candidate_network_path(&lambda_dir), &run.dirs.teacher_nnue(next))?;
            // The new teacher on the remote at once, where the arenas will read it.
            publish(&run.dirs.teacher_nnue(next))?;
            log::info!("generation {} promoted: lambda {winner} teaches generation {next}", state.generation);
            // A polyline run has no grid to move: its state's centre stays, never read.
            let (center_hundredths, step_hundredths) = match winner.constant() {
                Some(winner) => next_grid(run.config.on_source(state.generation).is_some(), winner),
                None => (state.center_hundredths, state.step_hundredths),
            };
            *state = GenState { generation: next, phase: Phase::Play, center_hundredths, step_hundredths };
        } else {
            // A generation has one training set, so a failed gate leaves nothing to try: the lineage
            // stops, a new run starting over (the developer, 2026-11-02). The boundary that closes the
            // generation: the mover takes it from here.
            state.phase = Phase::Stopped;
            run.boundary(state)?;
            let why = if nothing_learned {
                "learned nothing, every lambda falling back to the teacher".to_string()
            } else if let Some(source) = run.config.on_source(state.generation) {
                format!("on {} failed its gate against {PESTO}, so the network is wrong", source.corpus())
            } else {
                "failed its gate".to_string()
            };
            return Ok(format!("generation {} {why}: the run stops here (generations.tsv has its gate)", state.generation));
        }
        // A promotion ends the job: the next generation plays a new training set, so the developer
        // starts it, by Resume, once the gate has been read.
        run.boundary(state)?;
        return Ok(if scaled_on {
            format!(
                "generation {} failed its gate, promoted all the same in a scaled run, lambda {winner}: generation {} waits for Resume",
                state.generation - 1,
                state.generation
            )
        } else {
            format!("generation {} promoted, lambda {winner}: generation {} waits for Resume", state.generation - 1, state.generation)
        });
    }
}

/// The lambdas of generation `generation` that learned something: their candidate is a checkpoint
/// after step 0. One whose candidate record cannot be read is kept, as before this check, and its
/// round robin says what is wrong.
fn learned_arms(dirs: &Dirs, generation: u32, arms: &[ArmLambda]) -> Vec<ArmLambda> {
    arms.iter().copied().filter(|&arm| candidates::read_candidate_step(&dirs.arm(generation, arm)).map_or(true, |step| step > 0)).collect()
}

/// `learned_arms` of a grid's constant lambdas.
fn learned_lambdas(dirs: &Dirs, generation: u32, lambdas: &[f64]) -> Vec<f64> {
    let arms: Vec<ArmLambda> = lambdas.iter().map(|&lambda| ArmLambda::Constant(lambda)).collect();
    learned_arms(dirs, generation, &arms).into_iter().filter_map(ArmLambda::constant).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A teacher's header alone decides: version 1, the atan curve, refused; version 2 accepted.
    #[test]
    fn an_atan_teacher_is_refused_and_a_logistic_one_accepted() {
        let path = std::env::temp_dir().join(format!("smithy-teacher-{}.nnue", std::process::id())).to_str().unwrap().to_string();
        for (version, refused) in [(fattorini::nnue::ATAN_VERSION, true), (fattorini::nnue::VERSION, false)] {
            let mut bytes = fattorini::nnue::MAGIC.to_vec();
            bytes.extend_from_slice(&version.to_le_bytes());
            std::fs::write(&path, &bytes).unwrap();
            assert_eq!(refuse_atan_teacher(&path).is_err(), refused, "version {version}");
        }
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_run_s_files_are_read_here_first_then_on_the_remote() {
        let scratch = std::env::temp_dir().join(format!("smithy-run-home-{}", std::process::id()));
        std::fs::remove_dir_all(&scratch).ok();
        let root = scratch.to_str().unwrap().replace('\\', "/");
        let (local, remote) = (format!("{root}/local"), format!("{root}/remote"));
        std::fs::create_dir_all(&local).unwrap();
        std::fs::create_dir_all(&remote).unwrap();
        let roots = [local.clone(), remote.clone()];
        let home = || home_of(Dirs { root: local.clone() }, Dirs { root: remote.clone() }).root;
        // On the remote alone: its frozen config and its home are there.
        assert_eq!(frozen_config(&roots), None);
        std::fs::write(format!("{remote}/{RUN_CONFIG}"), "").unwrap();
        assert_eq!(frozen_config(&roots), Some(format!("{remote}/{RUN_CONFIG}")));
        assert_eq!(home(), remote);
        // Here too: this machine's copy first.
        std::fs::write(format!("{local}/{RUN_CONFIG}"), "").unwrap();
        std::fs::write(Dirs { root: local.clone() }.state(), "").unwrap();
        assert_eq!(frozen_config(&roots), Some(format!("{local}/{RUN_CONFIG}")));
        assert_eq!(home(), local);
        std::fs::remove_dir_all(&scratch).ok();
    }

    #[test]
    fn only_a_full_run_s_training_needs_the_gpu() {
        for phase in [Phase::Play, Phase::Train, Phase::RoundRobin, Phase::Gate, Phase::Done, Phase::Stopped] {
            assert_eq!(needs_gpu_now(None, phase, false), phase == Phase::Train, "{phase:?}");
            assert!(!needs_gpu_now(None, phase, true), "{phase:?} with a GPU");
            assert!(!needs_gpu_now(Some(1024), phase, false), "{phase:?} scaled");
        }
    }

    #[test]
    fn a_scaled_run_s_own_config_is_scaled_and_configs_refuse_a_scale() {
        fattorini::pipeline::repository_root::enter_for_tests();
        let (full, full_net) = load_config(&preprocessing_config::net_config_path("pesto-psq-threats-screlu")).unwrap();
        assert_eq!(full.scale, None);
        let text = std::fs::read_to_string(preprocessing_config::net_config_path("pesto-psq-threats-screlu")).unwrap();
        let dir = std::env::temp_dir().join(format!("smithy-scaled-config-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml").to_str().unwrap().to_string();
        std::fs::write(&path, format!("{text}\nscale: 1024\n")).unwrap();
        let (scaled, scaled_net) = load_config(&path).unwrap();
        assert_eq!(scaled.scale, Some(1024));
        assert!(scaled.training_positions < full.training_positions);
        assert!(scaled_net.training.steps_per_round < full_net.training.steps_per_round);
        assert!(scaled.depth() < full.depth(), "a scaled run searches shallower");
        std::fs::write(&path, format!("{text}\nscale: 0\n")).unwrap();
        assert!(load_config(&path).is_err(), "a scale of 0");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_config_of_a_kind_this_binary_does_not_know_is_an_error_not_a_panic() {
        // The server reads every config in `configs/`: one written for a newer binary must leave it
        // running, that config simply not a forge to it.
        fattorini::pipeline::repository_root::enter_for_tests();
        let text = std::fs::read_to_string(preprocessing_config::net_config_path("lc0-psq-threats-screlu")).unwrap();
        let (kind, unknown) = ("kind: psq_threats_screlu", "kind: a_kind_from_the_future");
        assert!(text.contains(kind), "lc0-psq-threats-screlu's kind line");
        let dir = std::env::temp_dir().join(format!("smithy-unknown-kind-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.yaml").to_str().unwrap().to_string();
        std::fs::write(&path, text.replace(kind, unknown)).unwrap();
        let loaded = std::panic::catch_unwind(|| load_config(&path));
        std::fs::remove_dir_all(&dir).ok();
        let message = loaded.expect("no panic").expect_err("an unknown kind");
        assert!(message.contains("a_kind_from_the_future"), "{message}");
    }

    #[test]
    fn a_source_s_buckets_are_named_by_its_corpus() {
        assert_eq!(source_buckets(Source::Pesto), "pesto");
        assert_eq!(source_buckets(Source::Lc0), "lc0");
    }

    #[test]
    fn a_scaled_run_reads_the_first_files_holding_its_share_of_the_corpus() {
        let files = |sizes: &[u64]| -> Vec<StreamFile> {
            sizes.iter().enumerate().map(|(i, &positions)| StreamFile { path: format!("f{i}"), positions }).collect()
        };
        let paths = |files: Vec<StreamFile>| -> Vec<String> { files.into_iter().map(|file| file.path).collect() };
        // 1/4 of 400,000 is 100,000: the first two files reach it.
        assert_eq!(paths(scaled_share(files(&[60_000, 60_000, 140_000, 140_000]), 4)), ["f0", "f1"]);
        // One file already holds the share: it alone.
        assert_eq!(paths(scaled_share(files(&[200_000, 200_000]), 1024)), ["f0"]);
        assert_eq!(paths(scaled_share(files(&[10, 20]), 1)), ["f0", "f1"], "a scale of 1 is the whole corpus");
    }

    #[test]
    fn every_state_written_counts_one_boundary_more_and_reads_back() {
        let root = std::env::temp_dir().join(format!("smithy-boundary-{}", std::process::id())).to_str().unwrap().replace('\\', "/");
        std::fs::remove_dir_all(&root).ok();
        let dirs = Dirs { root: root.clone() };
        let state = GenState { generation: 2, phase: Phase::Train, center_hundredths: 3, step_hundredths: 1 };
        write_state(&dirs, &state).unwrap();
        write_state(&dirs, &state).unwrap();
        let text = std::fs::read_to_string(dirs.state()).unwrap();
        assert_eq!(boundary_of(&text), 2);
        assert_eq!(GenState::parse(&text).unwrap(), state, "the count beside the state, not in it");
        assert_eq!(boundary_of("generation\t1\n"), 0, "a state written before it was counted");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_new_run_is_numbered_past_every_run_here_and_on_the_remote() {
        assert_eq!(next_run_number(&[], &[]), 1);
        assert_eq!(next_run_number(&[1, 2], &[1]), 3);
        // Run 3 lives on the remote alone, its local directory gone: a new run must not take its number.
        assert_eq!(next_run_number(&[1, 2], &[1, 2, 3]), 4);
    }

    #[test]
    fn play_time_leaves_out_pauses_and_the_eta_follows_its_pace() {
        let pairs = |at: &[u64]| -> Vec<(u64, bool)> { at.iter().map(|&at| (at, true)).collect() };
        // Pairs 10 s apart, then a stop of an hour, then 20 s more: 30 s of play in 3 gaps.
        let times = pairs(&[1000, 1010, 1020, 4620, 4630]);
        assert_eq!(play_time(&times), (30, 3));
        // A start, its first pair 60 s later and the next 10 s after; a stop, a new start about a day
        // later, its first pair 50 s later: 120 s in 3 pairs, the stop and the gap up to a start left out.
        let started = [(1000, false), (1060, true), (1070, true), (90_000, false), (90_050, true)];
        assert_eq!(play_time(&started), (120, 3));
        assert_eq!(play_time(&[(1000, false), (1000 + PAUSE_SECONDS + 1, true)]), (0, 0), "a first pair later than a pause");
        assert_eq!(
            pairs_eta(&times, 6),
            PlayTime { elapsed_seconds: Some(30), games_per_second: Some(0.2), eta_seconds: Some(60) },
            "6 pairs at 10 s each, 2 games a pair"
        );
        let none = PlayTime { elapsed_seconds: None, games_per_second: None, eta_seconds: None };
        assert_eq!(pairs_eta(&times, 0).eta_seconds, None, "nothing left: no ETA, not 0s");
        assert_eq!(pairs_eta(&pairs(&[1000]), 6), none, "one arrival: no time yet");
        assert_eq!(pairs_eta(&pairs(&[1000, 5000]), 6), PlayTime { elapsed_seconds: Some(0), ..none }, "only a pause: no pace");
        assert_eq!(play_time(&pairs(&[1000, 1000 + PAUSE_SECONDS])), (PAUSE_SECONDS, 1), "a gap of exactly the limit is play");
        // A gate dealt at once, its pairs arriving within 2 s, a minute after its start: its minute counts.
        let gate = [(1000, false), (1060, true), (1061, true), (1062, true)];
        assert_eq!(pairs_eta(&gate, 0).elapsed_seconds, Some(62));
    }

    #[test]
    fn a_record_s_times_are_its_lines_sixth_field_older_lines_none() {
        let dir = scratch("recorded-times");
        let record = format!("{dir}/round-robin.tsv");
        std::fs::write(&record, "0\t1\tc1\ts1\t0\n#start\t1699999990\n1\t-1\tc1\ts1\t1\t1700000000\n0\t0\tc1\ts1\t2\t1700000012\n").unwrap();
        assert_eq!(recorded_times(&record), vec![(1_699_999_990, false), (1_700_000_000, true), (1_700_000_012, true)]);
        assert_eq!(recorded_pairs(&record, 2).unwrap(), vec![vec![1, 0], vec![-1]], "the results read as before, the start no pair");
        mark_start(&record).unwrap();
        assert_eq!(recorded_times(&record).last().map(|&(_, pair)| pair), Some(false), "a start appended");
        assert_eq!(recorded_pairs(&record, 2).unwrap(), vec![vec![1, 0], vec![-1]]);
        assert!(recorded_times(&format!("{dir}/none.tsv")).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_test_s_wins_losses_and_draws_are_its_records_seventh_field_over_the_counted_pairs() {
        use matches::Wdl;
        let dir = scratch("recorded-wdl");
        let record = format!("{dir}/pairs.tsv");
        // An older line without the field, then two with it, a start between.
        std::fs::write(
            &record,
            "0\t0\tc1\ts1\t0\t1700000000\n#start\t1700000005\n0\t2\tc1\ts1\t1\t1700000010\t2,0,0\n0\t1\tc1\ts1\t2\t1700000020\t1,0,1\n",
        )
        .unwrap();
        let read = recorded_wdl(&record, 1);
        assert_eq!(read, vec![vec![None, Some(Wdl { wins: 2, losses: 0, draws: 0 }), Some(Wdl { wins: 1, losses: 0, draws: 1 })]]);
        assert_eq!(read[0].len(), recorded_pairs(&record, 1).unwrap()[0].len(), "aligned with the pairs");
        assert!(recorded_wdl(&format!("{dir}/none.tsv"), 1)[0].is_empty());
        let spec = matches::Spec::parse(&["match", "3", "aa@untuned", "bb@untuned", "depth 9"].map(str::to_string)).unwrap();
        let wdl_of = |view: WorkView| match view {
            WorkView::Test { wdl, .. } => wdl,
            _ => panic!("a test's view"),
        };
        // The older line's pair unknown: no sum.
        assert_eq!(wdl_of(test_view(1, &spec, &[0, 2, 1], &read[0], &[])), None);
        // Its pairs all known: their sum.
        assert_eq!(wdl_of(test_view(1, &spec, &[2, 1], &read[0][1..], &[])), Some(Wdl { wins: 3, losses: 0, draws: 1 }));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_grid_is_nine_hundredths_centred_and_clamped() {
        assert_eq!(grid(95, 1), (91..=99).collect::<Vec<_>>());
        assert_eq!(grid(97, 1), (91..=99).collect::<Vec<_>>(), "clamped at the top");
        assert_eq!(grid(50, 1), (46..=54).collect::<Vec<_>>());
        assert_eq!(grid(2, 1), (1..=9).collect::<Vec<_>>(), "clamped at the bottom");
    }

    #[test]
    fn the_coarse_grid_spans_a_tenth_to_nine_tenths() {
        assert_eq!(grid(COARSE_CENTER_HUNDREDTHS, COARSE_STEP_HUNDREDTHS), (1..=9).map(|tenth| tenth * 10).collect::<Vec<_>>());
    }

    #[test]
    fn a_generation_on_a_source_hands_the_coarse_grid_on_a_self_play_one_the_fine() {
        assert_eq!(next_grid(true, 0.02), (50, 10));
        assert_eq!(next_grid(false, 0.37), (37, 1));
    }

    #[test]
    fn a_state_reads_back_what_it_wrote() {
        let state = GenState { generation: 3, phase: Phase::RoundRobin, center_hundredths: 96, step_hundredths: 10 };
        assert_eq!(GenState::parse(&state.text()), Ok(state));
        let stopped = GenState { phase: Phase::Stopped, ..state };
        assert_eq!(GenState::parse(&stopped.text()), Ok(stopped));
    }

    #[test]
    fn a_state_from_before_attempts_were_removed_is_refused() {
        let text = "generation\t2\nattempt\t1\nphase\tplay\ncenter\t2\nstep\t1\nfailed_gates\t0\n";
        assert!(GenState::parse(text).unwrap_err().contains("migrate"));
    }

    #[test]
    fn a_lambda_whose_candidate_is_step_0_fell_back_to_its_teacher() {
        let root = std::env::temp_dir().join(format!("smithy-learned-{}", std::process::id())).to_str().unwrap().replace('\\', "/");
        std::fs::remove_dir_all(&root).ok();
        let dirs = Dirs { root: root.clone() };
        for (lambda, step) in [(0.1, "0"), (0.2, "8192"), (0.3, "0")] {
            let dir = dirs.arm(2, ArmLambda::Constant(lambda));
            std::fs::create_dir_all(&dir).unwrap();
            candidates::write_record(&candidates::candidate_record_path(&dir), &[("step", step.to_string())]);
        }
        // 0.4 has no candidate record: kept, its round robin saying what is wrong.
        assert_eq!(learned_lambdas(&dirs, 2, &[0.1, 0.2, 0.3, 0.4]), vec![0.2, 0.4]);
        assert_eq!(learned_lambdas(&dirs, 2, &[0.1, 0.3]), Vec::<f64>::new(), "every one fell back: nothing learned");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_state_without_a_step_reads_as_the_fine_grid() {
        let text = "generation\t2\nphase\tplay\ncenter\t2\n";
        assert_eq!(GenState::parse(text).map(|state| state.step_hundredths), Ok(STEP_HUNDREDTHS));
    }

    use fattorini::pipeline::protocol::task_body;

    /// A fresh directory for one test.
    fn scratch(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("fattorini-smithy-test-{name}-{}", std::process::id())).display().to_string();
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// One game with the tags of task `seq` of `chore`, played from `fen` by `moves`.
    #[allow(clippy::too_many_arguments)]
    fn game(chore: &str, session: &str, seq: u64, white: &str, black: &str, result: &str, fen: &str, moves: &str) -> String {
        format!(
            "[Event \"e\"]\n[White \"{white}\"]\n[Black \"{black}\"]\n[Result \"{result}\"]\n[FEN \"{fen}\"]\n[SetUp \"1\"]\n\
             [{CHORE_TAG} \"{chore}\"]\n[{SESSION_TAG} \"{session}\"]\n[{SEQ_TAG} \"{seq}\"]\n\n{moves} {result}\n\n"
        )
    }

    const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
    const ENDGAME: &str = "8/8/8/4k3/8/8/8/4K2R w K - 0 1";

    /// A scored self-play game of three positions.
    fn self_play_game(chore: &str, session: &str, seq: u64) -> String {
        game(chore, session, seq, PESTO, PESTO, "1/2-1/2", ENDGAME, "1. O-O {0.31 e1g1} Kd5 {#-4}")
    }

    /// Says hello for an arena of `workers` workers: its session.
    fn hello(shared: &Mutex<Shared>, workers: usize) -> String {
        let hello = Hello { machine: "alpha".to_string(), workers, commit: "c".to_string() };
        let reply = handle(shared, "t", &request("POST", HELLO_PATH, None, hello.text().as_bytes()), Instant::now());
        HelloReply::parse(&String::from_utf8(lan_http::status_of(&reply).unwrap().1).unwrap()).unwrap().session
    }

    fn post(shared: &Mutex<Shared>, path: &str, body: &[u8]) -> Reply {
        handle(shared, "t", &request("POST", path, Some("t"), body), Instant::now())
    }

    fn chore(reply: &Reply) -> Chore {
        let (word, body) = lan_http::status_of(reply).expect("a protocol answer");
        assert_eq!(word, "CHORE");
        Chore::parse(&String::from_utf8(body).unwrap()).unwrap()
    }

    #[test]
    fn every_committed_forge_config_loads_and_the_others_are_not_forges() {
        fattorini::pipeline::repository_root::enter_for_tests();
        for (name, source) in [("pesto-psq-threats-screlu", Some(Source::Pesto)), ("lc0-psq-threats-screlu", Some(Source::Lc0)),
            ("lc0-halfk-threats-screlu", Some(Source::Lc0)),
            ("pesto-halfk-threats-screlu", Some(Source::Pesto)), ("lc0-halfk-threats-screlu-hidden", Some(Source::Lc0)),
            ("lc0-halfk-threats-pairwise-hidden", Some(Source::Lc0))]
        {
            let (config, _) = load_config(&preprocessing_config::net_config_path(name)).unwrap_or_else(|message| panic!("{message}"));
            assert_eq!(config.source, source, "{name}");
            assert!(is_forge_config(name), "{name}");
        }
        for name in ["lc0", "pesto"] {
            assert!(load_config(&preprocessing_config::net_config_path(name)).is_err(), "{name}");
        }
        // The sources' configs size their frozen test sets.
        assert!(Source::Lc0.test_positions().unwrap() > 0 && Source::Pesto.test_positions().unwrap() > 0);
    }

    #[test]
    fn a_forge_is_named_after_its_source() {
        assert!(check_forge_name("lc0-psq", Some(Source::Lc0)).is_ok());
        assert!(check_forge_name("pesto-threats", Some(Source::Pesto)).is_ok());
        for (name, source) in
            [("pesto-psq", Some(Source::Lc0)), ("lc0-", Some(Source::Lc0)), ("lc0psq", Some(Source::Lc0)), ("smoke", None), ("lc0-psq", None)]
        {
            assert!(check_forge_name(name, source).is_err(), "{name}");
        }
    }

    #[test]
    fn a_source_s_jobs_are_named_and_shown_on_its_page() {
        let words = |text: &str| text.split(' ').map(str::to_string).collect::<Vec<_>>();
        assert_eq!(job_name(&words("--play pesto")), Ok("PeSTO's play".to_string()));
        assert_eq!(job_name(&words("--convert lc0")), Ok("Lc0's conversion".to_string()));
        assert_eq!(job_config(&words("--play pesto")), Some("pesto".to_string()));
        assert_eq!(job_config(&words("--convert lc0")), Some("lc0".to_string()));
        assert_eq!(job_config(&words("--tune pesto-psq-g3-r2")), Some("pesto-psq".to_string()));
        assert_eq!(job_config(&words("--tune pesto-psq-g1-r1")), Some("pesto-psq".to_string()));
        assert_eq!(job_config(&words("--tune pesto-psq-r1-0001")), None, "the old form");
        for wrong in ["--play lc0", "--convert pesto", "--play"] {
            assert!(job_name(&words(wrong)).is_err(), "{wrong}");
        }
    }

    #[test]
    fn a_book_job_is_named_shown_on_the_books_screen_and_held_by_the_source_slot() {
        let words = |text: &str| text.split(' ').map(str::to_string).collect::<Vec<_>>();
        assert_eq!(job_name(&words("--book elite")), Ok("the book of elite, points".to_string()));
        assert_eq!(job_name(&words("--book elite --weights counts")), Ok("the book of elite, counts".to_string()));
        assert_eq!(job_config(&words("--book elite")), Some(BOOKS_SCREEN.to_string()));
        assert_eq!(SlotJob::of(&words("--book elite")), Some(SlotJob::Book { source: "elite".to_string(), rest: Vec::new() }));
        assert_eq!(
            job_name(&words("--book elite --weights softmax --temperature 20 --scores scores-n-tuned-d20.txt")),
            Ok("the book of elite, softmax 20 scores-n-tuned-d20.txt".to_string())
        );
        assert_eq!(
            job_name(&words("--score-book elite lc0-psq-threats-screlu-g1-r1@search-tuned 20")),
            Ok("the scores of elite, lc0-psq-threats-screlu-g1-r1@search-tuned, depth 20".to_string())
        );
        assert_eq!(job_config(&words("--score-book elite n@search-tuned 20")), Some(BOOKS_SCREEN.to_string()));
        assert_eq!(SlotJob::of(&words("--score-book elite n@search-tuned 20")), None, "the arena slot's");
        for wrong in [
            "--score-book elite n@search-tuned",
            "--score-book elite n@search-tuned 0",
            "--score-book elite n@search-tuned@slow 20",
            "--score-book a/b n@search-tuned 20",
            "--score-book elite n@tuned 20",
        ] {
            assert!(job_name(&words(wrong)).is_err(), "{wrong}");
        }
        assert_eq!(SlotJob::of(&words("--convert lc0")), Some(SlotJob::Convert));
        assert_eq!(SlotJob::of(&words("--play pesto")), None, "the arena slot's");
        assert_eq!(
            SlotJob::of(&words("--book elite --weights counts")).unwrap().child_args(),
            words(&format!("{} elite --weights counts", book::CHILD_COMMAND))
        );
        for wrong in ["--book", "--book a/b", "--book elite --weights results", "--book elite --tau 20"] {
            assert!(job_name(&words(wrong)).is_err(), "{wrong}");
        }
    }

    #[test]
    fn the_source_slot_refuses_a_second_job_naming_the_first() {
        let slot = Arc::new(SourceSlot::default());
        slot.running.store(true, Ordering::SeqCst);
        *slot.job.lock().unwrap() = Some(SlotJob::Convert);
        let refused = start_in_slot(&slot, SlotJob::Book { source: "elite".to_string(), rest: Vec::new() });
        let text = String::from_utf8_lossy(&refused.body).into_owned();
        assert_eq!(refused.code, 409);
        assert!(text.contains("Lc0's conversion"), "{text}");
    }

    #[test]
    fn only_a_source_s_generation_1_trains_on_its_corpus() {
        fattorini::pipeline::repository_root::enter_for_tests();
        let (config, _) = load_config(&preprocessing_config::net_config_path("pesto-psq-threats-screlu")).unwrap();
        assert_eq!((config.on_source(1), config.on_source(2)), (Some(Source::Pesto), None));
    }

    #[test]
    fn a_scaled_run_s_scale_shrinks_a_forge_by_kind_and_one_is_the_config_itself() {
        fattorini::pipeline::repository_root::enter_for_tests();
        let load = || load_config(&preprocessing_config::net_config_path("pesto-psq-threats-screlu")).unwrap();
        let (config, net) = load();
        let sizes = |config: &GenConfig, net: &NetConfig| {
            let training = &net.training;
            (
                config.training_positions,
                config.test_positions,
                config.round_robin_pairs,
                training.training_batch_size,
                training.test_batch_size,
                training.steps_per_round,
                training.patience_rounds,
            )
        };
        let (mut same, mut same_net) = load();
        scale_forge(&mut same, &mut same_net, 1);
        assert_eq!(sizes(&same, &same_net), sizes(&config, &net), "a scale of 1 is the config itself");
        let (mut small, mut small_net) = load();
        scale_forge(&mut small, &mut small_net, scale::DEFAULT);
        assert_eq!(sizes(&small, &small_net), (1_048_576, 4_096, 12, 256, 256, 4, net.training.patience_rounds), "the shape is kept");
        let (mut tiny, mut tiny_net) = load();
        scale_forge(&mut tiny, &mut tiny_net, u64::MAX);
        assert_eq!(
            sizes(&tiny, &tiny_net),
            (scale::MIN_POSITIONS, scale::MIN_BATCH, scale::MIN_PAIRS, 256, 256, 1, net.training.patience_rounds),
            "every floor holds"
        );
    }

    #[test]
    fn a_forge_config_is_one_file_of_forge_keys_and_network_sections() {
        let dir = scratch("one-file-config");
        let network = "labels: lc0\narchitecture:\n  kind: psq\n  embedding_dim: 8\n  value_head:\n    hidden_dim: []\n\
                       training:\n  learning_rate: 0.1\n  training_batch_size: 1\n  test_batch_size: 1\n  steps_per_round: 1\n  \
                       warmup_rounds: 0\n  patience_rounds: 1\n";
        let forge = "grid_center: 0.05\ntest_positions: 1\nround_robin_pairs: 1\ngate_elo_d: 35\n\
                     tune:\n  length: 100\n  control: {depth: 2}\n";
        let load = |name: &str, text: String| {
            let path = format!("{dir}/{name}.yaml");
            std::fs::write(&path, text).unwrap();
            load_config(&path)
        };

        let (config, net) = load("whole", format!("{forge}training_positions: 7\n{network}")).unwrap();
        assert_eq!((config.training_positions, config.source), (7, Some(Source::Lc0)));
        assert_eq!(net.training.steps_per_round, 1);
        assert!(load("typo", format!("{forge}training_positions: 7\ngate_elo: 1\n{network}")).unwrap_err().contains("gate_elo"));
        for retired in ["max_failed_gates: 1", "corpus_positions: 7", "network: x", "label:\n  lambda: [0.95]", "preprocessing: {}", "data: {}"] {
            assert!(load("retired", format!("{forge}training_positions: 7\n{retired}\n{network}")).is_err(), "{retired}");
        }
        assert!(load("tune", "tune:\n  length: default\n".to_string()).unwrap_err().contains("not a forge config"));
        let without_tune = forge.split("tune:").next().unwrap();
        assert!(load("no-tune", format!("{without_tune}training_positions: 7\n{network}")).unwrap_err().contains("tune"), "tune: is required");
        let bad_tune = format!("{without_tune}tune:\n  length: 10\ntraining_positions: 7\n{network}");
        assert!(load("bad-tune", bad_tune).unwrap_err().contains("pairs"), "tune: is checked as the words it stands for");

        // The grid is the default; a polyline run has no grid centre, a grid run needs one.
        assert_eq!(config.lambda, LambdaMode::Grid);
        let rest = forge.strip_prefix("grid_center: 0.05\n").unwrap();
        let (polyline, _) = load("polyline", format!("lambda: polyline\n{rest}training_positions: 7\n{network}")).unwrap();
        assert_eq!((polyline.lambda, polyline.grid_center), (LambdaMode::Polyline, None));
        assert!(load("both", format!("lambda: polyline\n{forge}training_positions: 7\n{network}")).unwrap_err().contains("grid_center"));
        assert!(load("no-center", format!("{rest}training_positions: 7\n{network}")).unwrap_err().contains("grid_center"));
        assert!(load("unknown-mode", format!("lambda: curve\n{forge}training_positions: 7\n{network}")).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }


    #[test]
    fn self_play_fills_the_test_set_first_is_topped_up_and_closes_into_segments() {
        let dir = scratch("self-play");
        let (test_dir, train_dir) = (format!("{dir}/test"), format!("{dir}/train"));
        let shared = Mutex::new(Shared::default());
        shared.lock().unwrap().work =
            Some(Work::SelfPlay {
                teacher: PESTO.to_string(),
                parameters: TEACHER_TUNE,
                test_dir: test_dir.clone(),
                test_left: 2,
                train_dir: train_dir.clone(),
                train_left: 100,
                before: (0, 0),
                targets: (2, 100),
            });
        let session = hello(&shared, 1);
        let dealt = chore(&post(&shared, CHORE_PATH, session.as_bytes()));
        assert_eq!((dealt.kind, &dealt.limit, dealt.entries[0].count), (TaskKind::SelfPlay, &Limit::Depth(generation::SEARCH_DEPTH), TASKS_PER_WORKER));
        assert_eq!(
            dealt.entries[0].players,
            vec![Player { network: PESTO.to_string(), parameters: Some(TEACHER_TUNE), time: None }],
            "the teacher plays its tune"
        );

        let first = task_body(&session, &self_play_game(&dealt.id, &session, 0));
        assert_eq!(word(&post(&shared, TASK_PATH, &first)), "OK", "4 dealt, 1 arrived, 1 in flight: 2 left, enough");
        assert_eq!(word(&post(&shared, TASK_PATH, &first)), "OK", "a task arriving twice");
        let got = {
            let shared = shared.lock().unwrap();
            (shared.test_got, shared.train_got)
        };
        assert_eq!(got, (2, 0), "counted once, into the test set: 3 positions read, the final one not searched");
        let second = task_body(&session, &self_play_game(&dealt.id, &session, 1));
        let reply = post(&shared, TASK_PATH, &second);
        assert_eq!(word(&reply), "ADD", "1 left: topped up");
        assert_eq!(protocol::parse_entries(&String::from_utf8(lan_http::status_of(&reply).unwrap().1).unwrap()).unwrap()[0].count, TASKS_PER_WORKER);
        assert_eq!(shared.lock().unwrap().train_got, 2, "then the training set");

        let unscored = self_play_game(&dealt.id, &session, 2).replace(" {#-4}", "");
        assert_eq!(post(&shared, TASK_PATH, &task_body(&session, &unscored)).code, 400, "a game training could not read");
        let stranger = self_play_game("c999999", &session, 3);
        assert_eq!(post(&shared, TASK_PATH, &task_body(&session, &stranger)).code, 400, "a chore never dealt");

        shared.lock().unwrap().close_segments().unwrap();
        let files = game_files(&test_dir).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].positions, 2, "one game of 3 positions, its final one not searched");
        assert_eq!(positions_in(&train_dir).unwrap(), 2);
        assert!(std::fs::read_dir(&test_dir).unwrap().all(|entry| !entry.unwrap().file_name().to_string_lossy().ends_with(OPEN_SUFFIX)));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn self_play_s_time_paces_the_positions_left_by_those_received() {
        let none = PlayTime { elapsed_seconds: None, games_per_second: None, eta_seconds: None };
        assert_eq!(self_play_time(None, 10, 100), SelfPlayTime { time: none, positions_per_second: None }, "not dealing: no time");
        let started = Instant::now() - Duration::from_secs(100);
        assert_eq!(
            self_play_time(Some(started), 25, 100),
            SelfPlayTime { time: PlayTime { elapsed_seconds: Some(100), eta_seconds: Some(300), ..none }, positions_per_second: Some(0.25) },
            "a quarter in 100 s: the three quarters left in 300 s, no games a second"
        );
        assert_eq!(self_play_time(Some(started), 100, 100).time.eta_seconds, None, "nothing left: no ETA");
        assert_eq!(
            self_play_time(Some(started), 0, 100),
            SelfPlayTime { time: PlayTime { elapsed_seconds: Some(100), ..none }, positions_per_second: None },
            "nothing received yet: no pace"
        );
    }

    #[test]
    fn an_arenas_play_leaves_out_its_pauses() {
        let joined = Instant::now();
        let hello = Hello { machine: "alpha".to_string(), workers: 1, commit: "c".to_string() };
        let mut session = Session { hello, chore: None, last_seen: joined, joined, last_task: None, taken: 0, played: Duration::ZERO };
        let at = |seconds: u64| joined + Duration::from_secs(seconds);
        session.took(at(60));
        session.took(at(120));
        // Its machine off for an hour: that gap is a pause, not play.
        session.took(at(120 + 3600));
        session.took(at(120 + 3600 + 60));
        assert_eq!((session.taken, session.played), (4, Duration::from_secs(180)), "60 + 60 + 60 s of play, the hour left out");
    }

    #[test]
    fn a_resumed_self_play_shows_the_runs_totals_from_where_it_stood() {
        let shared = Shared {
            work: Some(Work::SelfPlay {
                teacher: PESTO.to_string(),
                parameters: TEACHER_TUNE,
                test_dir: String::new(),
                test_left: 30,
                train_dir: String::new(),
                train_left: 100,
                before: (70, 0),
                targets: (100, 100),
            }),
            test_got: 5,
            ..Default::default()
        };
        let WorkView::SelfPlay { test_got, test_wanted, train_got, train_wanted, .. } = shared.work_view() else {
            panic!("a self-play view");
        };
        assert_eq!((test_got, test_wanted), (75, 100), "70 held before, 5 this play, of the whole 100");
        assert_eq!((train_got, train_wanted), (0, 100));
    }

    #[test]
    fn a_self_play_resumed_past_its_test_target_shows_the_target_not_the_overshoot() {
        let shared = Shared {
            work: Some(Work::SelfPlay {
                teacher: PESTO.to_string(),
                parameters: TEACHER_TUNE,
                test_dir: String::new(),
                test_left: 0,
                train_dir: String::new(),
                train_left: 60,
                before: (103, 40),
                targets: (100, 100),
            }),
            ..Default::default()
        };
        let WorkView::SelfPlay { test_got, test_wanted, train_got, train_wanted, .. } = shared.work_view() else {
            panic!("a self-play view");
        };
        assert_eq!((test_got, test_wanted), (103, 100), "the last test game overshot by 3");
        assert_eq!((train_got, train_wanted), (40, 100));
    }

    #[test]
    fn self_play_counts_its_searched_positions() {
        let dir = scratch("self-play-kept");
        let (test_dir, train_dir) = (format!("{dir}/test"), format!("{dir}/train"));
        let shared = Mutex::new(Shared::default());
        shared.lock().unwrap().work = Some(Work::SelfPlay {
            teacher: PESTO.to_string(),
            parameters: TEACHER_TUNE,
            test_dir: test_dir.clone(),
            test_left: 3,
            train_dir: train_dir.clone(),
            train_left: 100,
            before: (0, 0),
            targets: (3, 100),
        });
        let session = hello(&shared, 1);
        let dealt = chore(&post(&shared, CHORE_PATH, session.as_bytes()));
        let first = task_body(&session, &self_play_game(&dealt.id, &session, 0));
        assert_eq!(word(&post(&shared, TASK_PATH, &first)), "OK");
        assert_eq!(shared.lock().unwrap().test_got, 2, "3 positions read, the final one not kept");
        let second = task_body(&session, &self_play_game(&dealt.id, &session, 1));
        post(&shared, TASK_PATH, &second);
        assert_eq!(shared.lock().unwrap().test_got, 4, "still short after the first game: the second into the test set too");

        shared.lock().unwrap().close_segments().unwrap();
        assert_eq!(positions_in(&test_dir).unwrap(), 4, "the segment's .meta positions less its games, the searched ones");
        let none = SegmentWriter::open(&test_dir).unwrap();
        assert_eq!(searched_in(&test_dir, &none).unwrap(), 4, "a resume counts the searched positions again, from the .meta");

        // A third game left in an open file counts too, from the file's own counts.
        let mut open = SegmentWriter::open(&test_dir).unwrap();
        open.append(&self_play_game(&dealt.id, &session, 2), 3).unwrap();
        let resumed = SegmentWriter::open(&test_dir).unwrap();
        assert_eq!((resumed.games, resumed.positions), (1, 3));
        assert_eq!(searched_in(&test_dir, &resumed).unwrap(), 6, "two closed games and the open one, 2 searched positions each");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn pair_chores_take_the_pairing_furthest_behind_and_stop_when_every_one_is_complete() {
        let dir = scratch("pairs");
        let record = format!("{dir}/round-robin.tsv");
        let shared = Mutex::new(Shared::default());
        {
            let mut shared = shared.lock().unwrap();
            let pairings = vec![("aa".to_string(), "bb".to_string()), ("aa".to_string(), "cc".to_string())];
            shared.work = Some(Work::Pairs { pairings, wanted: 1, record: record.clone(), sides: [SearchParameters::DEFAULT, TEACHER_TUNE] });
            shared.pairs = vec![vec![2], vec![]];
        }
        let session = hello(&shared, 1);
        let dealt = chore(&post(&shared, CHORE_PATH, session.as_bytes()));
        assert_eq!(dealt.entries[0].players.iter().map(|player| player.network.as_str()).collect::<Vec<_>>(), ["aa", "cc"], "the pairing behind");
        assert_eq!(dealt.entries[0].count, 1, "never past wanted");
        assert_eq!(
            dealt.entries[0].players.iter().map(|player| player.parameters).collect::<Vec<_>>(),
            [Some(SearchParameters::DEFAULT), Some(TEACHER_TUNE)],
            "a gate's sides: the challenger the defaults, the teacher what the gate gives it"
        );

        let swapped = format!(
            "{}{}",
            game(&dealt.id, &session, 0, "cc", "aa", "1-0", START, "1. e4"),
            game(&dealt.id, &session, 0, "aa", "cc", "1/2-1/2", START, "1. e4")
        );
        assert_eq!(post(&shared, TASK_PATH, &task_body(&session, &swapped)).code, 400, "the challenger is White first");
        let pair = format!(
            "{}{}",
            game(&dealt.id, &session, 0, "aa", "cc", "1-0", START, "1. e4"),
            game(&dealt.id, &session, 0, "cc", "aa", "1/2-1/2", START, "1. e4")
        );
        assert_eq!(word(&post(&shared, TASK_PATH, &task_body(&session, &pair))), "STOP", "every pairing complete");
        let line = std::fs::read_to_string(&record).unwrap();
        let fields: Vec<&str> = line.trim_end().split('\t').collect();
        assert_eq!(fields[..5].join("\t"), format!("1\t1\t{}\t{session}\t0", dealt.id), "a win and a draw: r = 1");
        assert!(fields[5].parse::<u64>().unwrap() > 0, "its arrival, sixth");
        assert_eq!(fields[6..], ["0,1,1"], "the champion's wins, losses and draws, last: a loss and a draw");
        assert_eq!(recorded_times(&record).len(), 1);
        assert_eq!(std::fs::read_to_string(Dirs::games_of(&record)).unwrap(), pair);
        assert_eq!(word(&post(&shared, CHORE_PATH, session.as_bytes())), "WAIT");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_round_robin_s_chore_spreads_its_tasks_over_the_pairings_behind() {
        let dir = scratch("tournament");
        let record = format!("{dir}/round-robin.tsv");
        let shared = Mutex::new(Shared::default());
        {
            let mut shared = shared.lock().unwrap();
            let pairings = ["bb", "cc", "dd"].iter().map(|second| ("aa".to_string(), second.to_string())).collect();
            shared.work = Some(Work::Pairs { pairings, wanted: 4, record: record.clone(), sides: [SearchParameters::DEFAULT; 2] });
            shared.pairs = vec![vec![0, 0], vec![], vec![0]];
        }
        let session = hello(&shared, 1);
        let dealt = chore(&post(&shared, CHORE_PATH, session.as_bytes()));
        assert_eq!(dealt.kind, TaskKind::Tournament);
        assert!(
            dealt.entries.iter().flat_map(|entry| &entry.players).all(|player| player.parameters == Some(SearchParameters::DEFAULT)),
            "a round robin plays its sides, the defaults"
        );
        let shares: Vec<(&str, u32)> = dealt.entries.iter().map(|entry| (entry.players[1].network.as_str(), entry.count)).collect();
        // Behind: 2, 0, 1; four tasks: cc, cc (the first of cc and dd at 1), dd, then bb (the first of
        // the three at 2).
        assert_eq!(shares, [("bb", 1), ("cc", 2), ("dd", 1)]);
        assert_eq!(shared.lock().unwrap().outstanding(1), 2);

        let pair = format!(
            "{}{}",
            game(&dealt.id, &session, 0, "aa", "cc", "1-0", START, "1. e4"),
            game(&dealt.id, &session, 0, "cc", "aa", "1/2-1/2", START, "1. e4")
        );
        assert_eq!(word(&post(&shared, TASK_PATH, &task_body(&session, &pair))), "OK", "3 left: not yet low");
        assert_eq!(shared.lock().unwrap().pairs[1], vec![1], "credited to its matchup by its players");
        let stranger = format!(
            "{}{}",
            game(&dealt.id, &session, 1, "bb", "cc", "1-0", START, "1. e4"),
            game(&dealt.id, &session, 1, "cc", "bb", "1-0", START, "1. e4")
        );
        assert_eq!(post(&shared, TASK_PATH, &task_body(&session, &stranger)).code, 400, "no matchup of its chore");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_test_s_chore_carries_its_limit_and_each_side_s_parameters() {
        let dir = scratch("test-chore");
        let tuned = SearchParameters { razoring_margin: -7, ..SearchParameters::DEFAULT };
        let shared = Mutex::new(Shared::default());
        {
            let mut shared = shared.lock().unwrap();
            shared.reference_nps = Some(1_000_000.0);
            shared.test = Some(TestDealing {
                limit: Limit::Clock("10+0.1".to_string()),
                parameters: [tuned, SearchParameters::DEFAULT],
                time: [Some(TimeParameters { pv_min: 0.6, ..TimeParameters::DEFAULT }), None],
            });
            shared.work = Some(Work::Pairs {
                pairings: vec![("aa".to_string(), "bb".to_string())],
                wanted: 3,
                record: format!("{dir}/test-0001.tsv"),
                sides: [SearchParameters::DEFAULT; 2],
            });
        }
        let session = hello(&shared, 1);
        let dealt = chore(&post(&shared, CHORE_PATH, session.as_bytes()));
        assert_eq!((dealt.kind, &dealt.limit, dealt.reference_nps), (TaskKind::Pair, &Limit::Clock("10+0.1".to_string()), Some(1_000_000.0)));
        let parameters: Vec<Option<SearchParameters>> = dealt.entries[0].players.iter().map(|player| player.parameters).collect();
        assert_eq!(parameters, [Some(tuned), Some(SearchParameters::DEFAULT)], "both given in full, the defaults too");
        let time: Vec<Option<TimeParameters>> = dealt.entries[0].players.iter().map(|player| player.time).collect();
        assert_eq!(time, [Some(TimeParameters { pv_min: 0.6, ..TimeParameters::DEFAULT }), None], "a time tune in full, else none");
        assert_eq!(dealt.entries[0].count, 3, "never past what the test may play");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_arena_lost_and_back_never_takes_a_pairing_past_what_it_wants() {
        let dir = scratch("lost-and-back");
        let record = format!("{dir}/round-robin.tsv");
        let shared = Mutex::new(Shared::default());
        {
            let mut shared = shared.lock().unwrap();
            let pairings = vec![("aa".to_string(), "bb".to_string()), ("aa".to_string(), "cc".to_string())];
            shared.work = Some(Work::Pairs { pairings, wanted: 1, record: record.clone(), sides: [SearchParameters::DEFAULT; 2] });
        }
        let lost = hello(&shared, 1);
        let first = chore(&post(&shared, CHORE_PATH, lost.as_bytes()));
        shared.lock().unwrap().lose("alpha");

        // Its tasks dealt again, to another machine's arena, which plays one of them.
        let other = Hello { machine: "gamma".to_string(), workers: 1, commit: "c".to_string() };
        let reply = handle(&shared, "t", &request("POST", HELLO_PATH, None, other.text().as_bytes()), Instant::now());
        let other = HelloReply::parse(&String::from_utf8(lan_http::status_of(&reply).unwrap().1).unwrap()).unwrap().session;
        let second = chore(&post(&shared, CHORE_PATH, other.as_bytes()));
        assert_eq!(second.entries.iter().map(|entry| entry.count).sum::<u32>(), 2, "the lost chore's tasks dealt again");
        let pair = |chore: &str, session: &str| {
            format!(
                "{}{}",
                game(chore, session, 0, "aa", "bb", "1-0", START, "1. e4"),
                game(chore, session, 0, "bb", "aa", "1-0", START, "1. e4")
            )
        };
        post(&shared, TASK_PATH, &task_body(&other, &pair(&second.id, &other)));

        // The lost arena back with the pair it played meanwhile: taken, not recorded.
        assert_eq!(word(&post(&shared, TASK_PATH, &task_body(&lost, &pair(&first.id, &lost)))), "OK");
        assert_eq!(shared.lock().unwrap().pairs[0], vec![0], "one pair, the first to arrive");
        assert_eq!(std::fs::read_to_string(&record).unwrap().lines().count(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_tune_deals_its_ks_tops_them_up_applies_each_once_and_stops_at_its_end() {
        let dir = scratch("tune");
        let shared = Mutex::new(Shared::default());
        {
            let mut shared = shared.lock().unwrap();
            shared.tune = Some(crate::tune::tests::tune_in(&dir, 4));
            shared.work = Some(Work::Tune { network: PESTO.to_string() });
        }
        let session = hello(&shared, 1);
        let dealt = chore(&post(&shared, CHORE_PATH, session.as_bytes()));
        assert!(dealt.adjudicate, "a tune's games end early");
        assert_eq!(dealt.reference_nps, None, "a depth needs no clock scaling");
        let ks: Vec<String> = dealt.entries.iter().map(|entry| entry.tags[0].1.clone()).collect();
        assert_eq!(ks, ["0", "1"], "KS_PER_WORKER k's, one entry each");
        assert!(dealt.entries[0].players[0].parameters.is_some(), "theta + c_k Delta, explicit");

        let pair = |k: &str, seq: u64, white_first: &str| -> Vec<u8> {
            let tag = |pgn: String| pgn.replace("[SetUp \"1\"]\n", &format!("[SetUp \"1\"]\n[{} \"{k}\"]\n", tune::K_TAG));
            let first = tag(game(&dealt.id, &session, seq, PESTO, PESTO, white_first, START, "1. e4"));
            let second = tag(game(&dealt.id, &session, seq, PESTO, PESTO, "1/2-1/2", START, "1. e4"));
            task_body(&session, &format!("{first}{second}"))
        };
        assert_eq!(post(&shared, TASK_PATH, &pair("3", 0, "1-0")).code, 400, "a k the chore does not hold");
        let reply = post(&shared, TASK_PATH, &pair("0", 0, "1-0"));
        assert_eq!(word(&reply), "ADD", "2 dealt, 1 arrived, 1 in flight: topped up");
        let added = protocol::parse_entries(&String::from_utf8(lan_http::status_of(&reply).unwrap().1).unwrap()).unwrap();
        assert_eq!(added.iter().map(|entry| entry.tags[0].1.as_str()).collect::<Vec<_>>(), ["2", "3"]);
        for (k, seq) in [("1", 1), ("2", 2)] {
            post(&shared, TASK_PATH, &pair(k, seq, "0-1"));
        }
        assert_eq!(word(&post(&shared, TASK_PATH, &pair("3", 3, "1/2-1/2"))), "STOP", "every k applied");
        let shared = shared.lock().unwrap();
        let tune = shared.tune.as_ref().unwrap();
        assert!(tune.done());
        assert_eq!(std::fs::read_to_string(tune.games_path()).unwrap().matches("[Event ").count(), 8);
        assert_eq!(shared.pair_times.iter().filter(|&&(_, pair)| pair).count(), 4, "each pair's arrival, for its ETA");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_finished_tune_still_shows_its_table_and_how_it_ended() {
        let dir = scratch("finished");
        let shared = Arc::new(Mutex::new(Shared::default()));
        {
            let mut shared = shared.lock().unwrap();
            shared.tune = Some(crate::tune::tests::tune_in(&dir, 4));
            shared.work = Some(Work::Idle);
            shared.finished = Some("4 pairs done".to_string());
        }
        let status = test_status(shared);
        let snapshot = status.snapshot(Instant::now(), false);
        assert!(matches!(snapshot.work, WorkView::Tune { pairs: 4, time: None, .. }), "the tune, though idle, with no time");
        assert_eq!(snapshot.finished.as_deref(), Some("4 pairs done"));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A tune, told apart from the defaults: what a teacher plays in the tests.
    const TEACHER_TUNE: SearchParameters = SearchParameters { reverse_futility_margin: 96, aspiration_delta: 7, ..SearchParameters::DEFAULT };

    #[test]
    fn a_generation_s_teacher_is_the_previous_winner_or_its_source_s_player() {
        assert_eq!(teacher_player("lc0-psq-r1", 2, Some(Source::Lc0)).as_deref(), Some("lc0-psq-g1-r1"));
        assert_eq!(teacher_player("pesto-psq-r2", 3, Some(Source::Pesto)).as_deref(), Some("pesto-psq-g2-r2"));
        assert_eq!(teacher_player("pesto-psq-r2", 1, Some(Source::Pesto)).as_deref(), Some(PESTO), "PeSTO taught a PeSTO run's first");
        assert_eq!(teacher_player("lc0-psq-r1", 1, Some(Source::Lc0)), None, "Lc0 is no Fattorini player: the defaults");
        // A network side of a gate or round robin plays the defaults, needing no tune.
        assert_eq!(side_parameters("lc0-psq-g1-r1", "a test"), Ok(SearchParameters::DEFAULT));
    }

    #[test]
    fn a_readable_generation_is_not_a_tune_s_folder_alone() {
        let dir = scratch("readable");
        let (local, remote) = (format!("{dir}/local"), format!("{dir}/remote"));
        let dirs = |root: &str| Dirs { root: root.to_string() };
        let read = |generation| readable_of(dirs(&local), dirs(&remote), generation).root;
        let make = |root: &str, generation: u32, inside: &str| {
            std::fs::create_dir_all(format!("{}/{inside}", dirs(root).generation(generation))).unwrap();
        };
        // Generation 1: a tune's folder alone here, the generation trained on the remote.
        make(&local, 1, "tune");
        make(&remote, 1, "lambda-polyline");
        // Generation 2: trained here, and on the remote too.
        make(&local, 2, "lambda-polyline");
        make(&remote, 2, "lambda-polyline");
        // Generation 3: running here, in its self-play, nothing on the remote yet.
        make(&local, 3, "test");
        // Generation 4: on the remote alone.
        make(&remote, 4, "lambda-polyline");
        assert_eq!(read(1), remote, "a tune's folder alone does not hide the remote's generation");
        assert_eq!(read(2), local, "a local copy that trained is read here");
        assert_eq!(read(3), local, "a running generation, no arm anywhere yet, stays local");
        assert_eq!(read(4), remote, "no local folder: the remote's");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A smithy's status over `shared`, with no job running and its own stops.
    fn test_status(shared: Arc<Mutex<Shared>>) -> Status {
        Status {
            shared,
            stop: Arc::new(AtomicBool::new(false)),
            job_stop: Arc::new(AtomicBool::new(false)),
            requested: Mutex::new(None),
            running: AtomicBool::new(false),
            control_token: "c0ntr0l".to_string(),
            page: "http://127.0.0.1:47821/".to_string(),
            history: Mutex::new(HashMap::new()),
            slot: Arc::new(SourceSlot::default()),
            claims: Claims::new(test_claim(), Vec::new()),
        }
    }

    fn test_claim() -> Claim {
        Claim { smithy: "beta".to_string(), host: LOOPBACK.to_string(), boot: "b00t".to_string(), commit: "c".to_string(), scaled: false, rank: None }
    }

    #[test]
    fn an_arena_lost_has_its_chore_closed_its_pairs_dealt_again_and_its_late_task_still_taken() {
        let dir = scratch("lost");
        let record = format!("{dir}/round-robin.tsv");
        let shared = Mutex::new(Shared::default());
        shared.lock().unwrap().work =
            Some(Work::Pairs { pairings: vec![("aa".to_string(), "bb".to_string())], wanted: 4, record: record.clone(), sides: [SearchParameters::DEFAULT; 2] });
        let session = hello(&shared, 1);
        let dealt = chore(&post(&shared, CHORE_PATH, session.as_bytes()));
        assert_eq!(shared.lock().unwrap().outstanding(0), 4);
        shared.lock().unwrap().lose("alpha");
        {
            let state = shared.lock().unwrap();
            assert_eq!(state.outstanding(0), 0, "its pairs wanted again");
            assert!(state.chores[&dealt.id].closed);
            assert!(state.sessions.contains_key(&session), "its session kept, for a task it sends once back");
        }
        let pair = format!(
            "{}{}",
            game(&dealt.id, &session, 0, "aa", "bb", "1-0", START, "1. e4"),
            game(&dealt.id, &session, 0, "bb", "aa", "1/2-1/2", START, "1. e4")
        );
        assert_eq!(word(&post(&shared, TASK_PATH, &task_body(&session, &pair))), "OK", "a late task of the closed chore, taken");
        assert_eq!(shared.lock().unwrap().pairs[0], vec![1]);
        assert_eq!(chore(&post(&shared, CHORE_PATH, session.as_bytes())).entries[0].count, 3, "the rest dealt again");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Whether the stand-in arena of `a_claim_round_claims_polls_and_loses_an_arena` disowns its claim.
    static DISOWNED: AtomicBool = AtomicBool::new(false);

    #[test]
    fn a_claim_round_claims_polls_and_loses_an_arena() {
        let listener = lan_http::Listener::bind(LOOPBACK, 0).unwrap();
        let mut claims = Claims::new(test_claim(), vec![LOOPBACK.to_string()]);
        claims.port = listener.port();
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                let answer = |request: &Request| -> Reply {
                    match request.path.as_str() {
                        CLAIM_PATH => Reply::status("OK", Hello { machine: "gamma".to_string(), workers: 3, commit: "c".to_string() }.text().as_bytes()),
                        _ if DISOWNED.load(Ordering::SeqCst) => Reply::status("UNCLAIMED", b""),
                        _ => {
                            let alive = Alive {
                                session: "-".to_string(),
                                chore: "-".to_string(),
                                in_flight: 0,
                                sent: 0,
                                outbox: 2,
                            };
                            Reply::status("OK", alive.text().as_bytes())
                        }
                    }
                };
                listener.serve(&answer, &stop);
            })
        };
        let shared = Mutex::new(Shared::default());
        let now = Instant::now();
        claims.round(&shared, now);
        assert!(claims.held().is_empty(), "a job dealing nothing claims nothing");
        shared.lock().unwrap().work = Some(Work::Tune { network: PESTO.to_string() });
        claims.round(&shared, now);
        assert_eq!(claims.views(now)[0].machine, "gamma", "claimed");
        claims.round(&shared, now);
        assert_eq!(claims.views(now)[0].outbox, 2, "polled");
        assert!(claims.views(now + LATE_AFTER + Duration::from_secs(1))[0].state.starts_with("late"));
        DISOWNED.store(true, Ordering::SeqCst);
        claims.round(&shared, now);
        assert!(claims.held().is_empty(), "disowned, lost");
        DISOWNED.store(false, Ordering::SeqCst);
        claims.round(&shared, now);
        assert_eq!(claims.held().len(), 1, "claimed again");

        stop.store(true, Ordering::SeqCst);
        thread.join().unwrap();
        claims.round(&shared, now + RELEASE_AFTER - Duration::from_secs(1));
        assert_eq!(claims.held().len(), 1, "unanswering, but not long enough to be lost");
        claims.round(&shared, now + RELEASE_AFTER + Duration::from_secs(1));
        assert!(claims.held().is_empty(), "silent, lost");
        assert!(claims.views(now)[0].state.starts_with("gamma silent for 61 s: lost"), "{}", claims.views(now)[0].state);
        claims.round(&shared, now);
        assert_eq!(claims.views(now)[0].state, "no arena answering; tried again in 10 s, waits doubling up to 5 min");
    }

    #[test]
    fn a_job_dealing_nothing_for_the_grace_releases_its_arenas() {
        // Two names for this machine, so nothing waits on a lookup.
        let (gamma, delta) = (LOOPBACK, "localhost");
        let claims = Claims::new(test_claim(), vec![gamma.to_string(), delta.to_string()]);
        let hello = |machine: &str| Hello { machine: machine.to_string(), workers: 1, commit: "c".to_string() };
        let now = Instant::now();
        for target in [gamma, delta] {
            claims.held().insert(target.to_string(), Held { hello: hello(target), answered: now, alive: None });
        }
        // Port 1: nothing answers, so each release and poll fails quietly.
        let mut claims = claims;
        claims.port = 1;
        let shared = Mutex::new(Shared::default());
        claims.round(&shared, now);
        assert_eq!(claims.held().len(), 2, "idle, but within the grace");
        claims.round(&shared, now + CLAIM_IDLE_GRACE);
        assert!(claims.held().is_empty(), "released");
    }

    #[test]
    fn a_commit_is_shown_by_its_hash_s_first_twelve_characters() {
        assert_eq!(short_commit("26ff1f838fd4362720d9dcc1eaf2987edfe4509c (modified)"), "26ff1f838fd4 (modified)");
        assert_eq!(short_commit("26ff1f838fd4362720d9dcc1eaf2987edfe4509c"), "26ff1f838fd4");
        assert_eq!(short_commit("unknown"), "unknown");
    }

    #[test]
    fn a_machine_failing_its_claims_is_tried_again_after_waits_doubling_up_to_the_cap() {
        let claims = Claims::new(test_claim(), vec!["gamma".to_string()]);
        let now = Instant::now();
        let wait = |claims: &Claims| claims.backoff()["gamma"].wait.as_secs();
        claims.failed("gamma", "no arena answering".to_string(), now);
        assert_eq!(wait(&claims), 10);
        claims.failed("gamma", "no arena answering".to_string(), now);
        assert_eq!(wait(&claims), 20, "the same failure again doubles the wait");
        for _ in 0..10 {
            claims.failed("gamma", "no arena answering".to_string(), now);
        }
        assert_eq!(wait(&claims), 300, "up to the cap");
        claims.failed("gamma", "held by the smithy on alpha".to_string(), now);
        assert_eq!(wait(&claims), 10, "another failure starts again");
        assert!(claims.views(now)[0].state.ends_with("tried again in 10 s, waits doubling up to 5 min"));

        // Not tried before its time: a round then sends nothing, so nothing new is said.
        let shared = Mutex::new(Shared::default());
        claims.round(&shared, now + Duration::from_secs(9));
        assert!(claims.views(now)[0].state.starts_with("held by the smithy on alpha"));
    }

    #[test]
    fn an_open_file_cut_short_keeps_its_whole_games() {
        let dir = scratch("open");
        let whole = self_play_game("o000001", "alpha-0", 0);
        std::fs::write(format!("{dir}/100{OPEN_SUFFIX}"), format!("{whole}{}", &whole[..whole.len() / 2])).unwrap();
        let writer = SegmentWriter::open(&dir).unwrap();
        assert_eq!((writer.games, writer.positions), (1, 3));
        assert_eq!(std::fs::read_to_string(&writer.path).unwrap(), whole);
        writer.close().unwrap();
        assert_eq!(positions_in(&dir).unwrap(), 2, "its one whole game's searched positions");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_closed_segment_is_uploaded_with_its_meta_and_never_the_open_file() {
        let dir = scratch("upload");
        let remote_dir = format!("{dir}/remote/train");
        let mut writer = SegmentWriter { path: format!("{dir}/200{OPEN_SUFFIX}"), positions: 0, games: 0 };
        writer.append(&self_play_game("u000001", "alpha-0", 0), 3).unwrap();
        writer.close().unwrap();
        std::fs::write(format!("{dir}/300{OPEN_SUFFIX}"), "still being played").unwrap();
        upload_segment(&format!("{dir}/200.pgn.zst"), &dir, &remote_dir).unwrap();
        let mut names: Vec<String> = std::fs::read_dir(&remote_dir).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, vec!["200.pgn.zst".to_string(), format!("200.pgn.zst{META_SUFFIX}")]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_remote_s_open_files_are_removed_and_nothing_else() {
        let dir = scratch("remote-open");
        for (set, name) in [("train", "100.pgn.open"), ("train", "100.pgn.zst"), ("test", "50.pgn.open"), ("test", "50.pgn.zst.meta")] {
            std::fs::create_dir_all(format!("{dir}/{set}")).unwrap();
            std::fs::write(format!("{dir}/{set}/{name}"), "x").unwrap();
        }
        assert_eq!(remove_remote_open_files(&dir).unwrap(), 2);
        assert!(Path::new(&format!("{dir}/train/100.pgn.zst")).is_file());
        assert!(Path::new(&format!("{dir}/test/50.pgn.zst.meta")).is_file());
        assert!(!Path::new(&format!("{dir}/train/100.pgn.open")).exists());
        assert_eq!(remove_remote_open_files(&format!("{dir}/missing")).unwrap(), 0, "no generation there yet");
        std::fs::remove_dir_all(&dir).ok();
    }

    fn request(method: &str, path: &str, token: Option<&str>, body: &[u8]) -> Request {
        Request { method: method.to_string(), path: path.to_string(), token: token.map(str::to_string), host: None, body: body.to_vec() }
    }

    #[test]
    fn the_loopback_host_is_the_listeners_address_or_localhost_at_its_port() {
        assert!(is_loopback_host(Some("127.0.0.1:47821"), 47821));
        assert!(is_loopback_host(Some("localhost:47821"), 47821));
        for host in [None, Some("rebound.example:47821"), Some("127.0.0.1"), Some("127.0.0.1:80"), Some("beta:47821")] {
            assert!(!is_loopback_host(host, 47821), "{host:?}");
        }
    }

    /// A request by hand, its `Host` set as a DNS-rebound page's would be: the loopback listener
    /// refuses it before any path, the state and its token included; the LAN listener does not look.
    #[test]
    fn the_loopback_listener_refuses_a_foreign_host() {
        use lan_http::Listener;
        use std::io::{Read, Write};
        let shared = Arc::new(Mutex::new(Shared::default()));
        let status = Arc::new(test_status(Arc::clone(&shared)));
        let listeners_stop = Arc::new(AtomicBool::new(false));
        let mut ports = Vec::new();
        let mut threads = Vec::new();
        for control_here in [true, false] {
            let listener = Listener::bind(LOOPBACK, 0).unwrap();
            ports.push(listener.port());
            let (status, listeners_stop) = (Arc::clone(&status), Arc::clone(&listeners_stop));
            threads.push(std::thread::spawn(move || serve(listener, status, control_here, listeners_stop)));
        }
        let get = |port: u16, host: &str| -> String {
            let mut stream = std::net::TcpStream::connect((LOOPBACK, port)).unwrap();
            write!(stream, "GET {STATE_PATH} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").unwrap();
            let mut answer = String::new();
            stream.read_to_string(&mut answer).unwrap();
            answer
        };
        let (control, lan) = (ports[0], ports[1]);
        let refused = get(control, &format!("rebound.example:{control}"));
        assert!(refused.starts_with("HTTP/1.1 403"), "{refused}");
        assert!(!refused.contains("c0ntr0l"), "no token to a foreign host");
        let served = get(control, &format!("localhost:{control}"));
        assert!(served.starts_with("HTTP/1.1 200") && served.contains("c0ntr0l"), "{served}");
        let lan_served = get(lan, &format!("rebound.example:{lan}"));
        assert!(lan_served.starts_with("HTTP/1.1 200") && !lan_served.contains("c0ntr0l"), "{lan_served}");
        listeners_stop.store(true, Ordering::SeqCst);
        for thread in threads {
            thread.join().unwrap();
        }
    }

    fn word(reply: &Reply) -> String {
        lan_http::status_of(reply).expect("a protocol answer").0
    }

    #[test]
    fn every_endpoint_but_hello_needs_the_token_which_hello_hands_out() {
        let shared = Mutex::new(Shared::default());
        let now = Instant::now();
        let hello = Hello { machine: "alpha".to_string(), workers: 2, commit: "c".to_string() };
        let reply = handle(&shared, "t0k3n", &request("POST", HELLO_PATH, None, hello.text().as_bytes()), now);
        let (status, body) = lan_http::status_of(&reply).unwrap();
        assert_eq!(status, "OK");
        let joined = HelloReply::parse(&String::from_utf8(body).unwrap()).unwrap();
        let store = remote::this_root().to_string();
        assert_eq!(joined, HelloReply { session: "alpha-0".to_string(), token: "t0k3n".to_string(), store }, "the store networks are read from");

        for token in [None, Some("old")] {
            assert_eq!(word(&handle(&shared, "t0k3n", &request("POST", CHORE_PATH, token, b"alpha-0"), now)), "TOKEN", "{token:?}");
        }
        assert_eq!(word(&handle(&shared, "t0k3n", &request("POST", CHORE_PATH, Some("t0k3n"), b"alpha-0"), now)), "WAIT", "nothing dealt");
        assert_eq!(word(&handle(&shared, "t0k3n", &request("POST", CHORE_PATH, Some("t0k3n"), b"alpha-9"), now)), "UNKNOWN");
        assert_eq!(word(&handle(&shared, "t0k3n", &request("POST", TASK_PATH, Some("t0k3n"), b"alpha-9\n"), now)), "UNKNOWN");
        assert_eq!(handle(&shared, "t0k3n", &request("GET", "/network/00ff", Some("t0k3n"), b""), now).code, 404, "networks are read from the store");
        assert_eq!(handle(&shared, "t0k3n", &request("GET", CHORE_PATH, Some("t0k3n"), b""), now).code, 404, "the wrong method");
        assert_eq!(handle(&shared, "t0k3n", &request("POST", "/elsewhere", Some("t0k3n"), b""), now).code, 404);
        assert_eq!(handle(&shared, "t0k3n", &request("POST", HELLO_PATH, None, b"no fields"), now).code, 400);
    }

    /// The control answers on the listener that has it, the loopback one, and is unknown on the other,
    /// the LAN's; the arenas' endpoints answer on both. A job is asked for only with the control token,
    /// which only the loopback's page is given, and only one at a time.
    #[test]
    fn the_control_answers_only_where_it_is_served() {
        use lan_http::{self as http, Listener};
        let shared = Arc::new(Mutex::new(Shared {
            token: "t0k3n".to_string(),
            job: Some("gen-test".to_string()),
            state_path: Some("no/such/state.txt".to_string()),
            ..Shared::default()
        }));
        let status = Arc::new(test_status(Arc::clone(&shared)));
        let listeners_stop = Arc::new(AtomicBool::new(false));
        let mut ports = Vec::new();
        let mut threads = Vec::new();
        for control_here in [true, false] {
            let listener = Listener::bind(LOOPBACK, 0).unwrap();
            ports.push(listener.port());
            let (status, listeners_stop) = (Arc::clone(&status), Arc::clone(&listeners_stop));
            threads.push(std::thread::spawn(move || serve(listener, status, control_here, listeners_stop)));
        }
        let timeout = Duration::from_secs(5);
        let (control, lan) = (ports[0], ports[1]);
        let reply = http::get(&http::url(LOOPBACK, control, STATUS_PATH), None, timeout).unwrap();
        let text = String::from_utf8(http::status_of(&reply).unwrap().1).unwrap();
        assert!(text.starts_with("job gen-test, state unreadable"), "{text}");
        assert!(text.contains("no arena"), "{text}");

        let job = b"--tune\npesto\n";
        assert_eq!(http::post(&http::url(LOOPBACK, lan, JOB_PATH), Some("c0ntr0l"), job, timeout).unwrap().code, 404);
        assert_eq!(http::post(&http::url(LOOPBACK, control, JOB_PATH), None, job, timeout).unwrap().code, 403, "no control token");
        assert_eq!(http::post(&http::url(LOOPBACK, control, JOB_PATH), Some("c0ntr0l"), b"--nothing\n", timeout).unwrap().code, 400);
        assert_eq!(word(&http::post(&http::url(LOOPBACK, control, JOB_PATH), Some("c0ntr0l"), job, timeout).unwrap()), "OK");
        assert_eq!(http::post(&http::url(LOOPBACK, control, JOB_PATH), Some("c0ntr0l"), job, timeout).unwrap().code, 409, "one job at a time");
        let play = b"--play\npesto\n";
        assert_eq!(http::post(&http::url(LOOPBACK, control, JOB_PATH), Some("c0ntr0l"), play, timeout).unwrap().code, 409, "PeSTO's play is an arena job");
        let asked = status.requested.lock().unwrap().clone();
        assert_eq!(asked, Some(["--tune", "pesto"].map(str::to_string).to_vec()));
        // lc0's stop is the source slot's, and leaves the arena job alone.
        assert_eq!(word(&http::post(&http::url(LOOPBACK, control, JOB_STOP_PATH), Some("c0ntr0l"), b"lc0", timeout).unwrap()), "OK");
        assert!(!status.job_stop.load(Ordering::SeqCst), "the conversion's stop is not the arena job's");
        assert_eq!(word(&http::post(&http::url(LOOPBACK, control, JOB_STOP_PATH), Some("c0ntr0l"), b"", timeout).unwrap()), "OK");
        assert!(status.job_stop.load(Ordering::SeqCst) && !status.stop.load(Ordering::SeqCst), "the job's stop, not the smithy's");

        assert_eq!(http::post(&http::url(LOOPBACK, lan, STOP_PATH), None, b"", timeout).unwrap().code, 404);
        assert!(!status.stop.load(Ordering::SeqCst), "no stop from the LAN's listener");
        assert_eq!(word(&http::post(&http::url(LOOPBACK, control, STOP_PATH), None, b"", timeout).unwrap()), "OK");
        assert!(status.stop.load(Ordering::SeqCst), "a stop from the loopback's");
        let hello = Hello { machine: "gamma".to_string(), workers: 1, commit: "c".to_string() };
        assert_eq!(word(&http::post(&http::url(LOOPBACK, lan, HELLO_PATH), None, hello.text().as_bytes(), timeout).unwrap()), "OK");
        // The status page, on both listeners and without a token, holding the arena just joined; the
        // control token on the loopback's alone.
        for (port, token) in [(control, serde_json::json!("c0ntr0l")), (lan, serde_json::Value::Null)] {
            let page = http::get(&http::url(LOOPBACK, port, PAGE_PATH), None, timeout).unwrap();
            assert_eq!(page.code, 200);
            assert!(String::from_utf8(page.body).unwrap().contains("state.json"));
            let state = http::get(&http::url(LOOPBACK, port, STATE_PATH), None, timeout).unwrap();
            let json: serde_json::Value = serde_json::from_slice(&state.body).unwrap();
            assert_eq!(json["job"], "gen-test");
            assert_eq!(json["work"]["kind"], "idle");
            assert_eq!(json["arenas"][0]["machine"], "gamma");
            assert_eq!(json["control"], token);
            assert_eq!(json["slot"]["running"], false);
            assert!(json["slot"]["holder"].is_null() && json["slot"]["kind"].is_null(), "no job in the source slot");
            assert!(json["slot"]["progress"].is_null() && json["slot"]["book"].is_null(), "no Convert nor book job running");
        }
        // The bars drawn, on every listener: one made here among them.
        let bar = progress::bar(8, progress::Unit::Count("steps"), "control test pass");
        bar.set_position(3);
        for port in [control, lan] {
            let reply = http::get(&http::url(LOOPBACK, port, PROGRESS_PATH), None, timeout).unwrap();
            let bars: serde_json::Value = serde_json::from_slice(&reply.body).unwrap();
            let ours = bars.as_array().unwrap().iter().find(|shown| shown["label"] == "control test pass").expect("the bar listed");
            assert_eq!(ours["counts"], "3/8 steps");
        }
        drop(bar);
        listeners_stop.store(true, Ordering::SeqCst);
        for thread in threads {
            thread.join().unwrap();
        }
    }
}
