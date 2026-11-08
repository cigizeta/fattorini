//! Tests between the smithy's networks, asked from its page: a duel, a match or an SPRT of two
//! players, played as a job by the arenas in colour-reversed pairs (`smithy::test_job`), and their
//! log, the latest `KEPT` tests each with its games.
//!
//! **The duel** is arena's rule (`arena/src/duel.rs`), copied as `sprt.rs` copies arena's SPRT, the two
//! binaries sharing no code beyond `fattorini`: per pair, S the mean pair score (the challenger's points
//! in a pair, halved), an engine past `0.5 + C * sigma_hat / sqrt(P)` wins, `sigma_hat` the pair
//! scores' deviation with a 0-2 and a 2-0 pair added; at `DUEL_MAX_PAIRS` with neither past it the
//! duel is drawn, and it is drawn at once when neither could pass its bound at any later check,
//! whatever the pairs to come (curtailed). The derivation is in arena's header. **The match** plays a fixed number of pairs and estimates
//! the Elo between the two. **The SPRT** is the gate's, of H0 `elo0` against H1 `elo1`.

use serde::Serialize;

use fattorini::pipeline::protocol::Limit;

use super::sprt::{Sprt, Verdict};

/// Where the tests are kept, locally: `tests.tsv`, and each test's pairs and games.
pub const TESTS_DIR: &str = "data/tests";

/// The tests kept, with their games: the oldest goes as a new one starts past it.
pub const KEPT: usize = 20;

/// arena's `duel::C`, `Phi^-1(0.999)` rounded.
pub const DUEL_C: f64 = 3.09;
/// arena's `duel::MAX_PAIRS`: the duel ends here regardless.
pub const DUEL_MAX_PAIRS: u64 = 400;
/// An SPRT's pairs at most, as the gate's (`GATE_MAX_PAIRS`): inconclusive past them.
pub const SPRT_MAX_PAIRS: u32 = 5_000;

/// A test's kind and its settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Duel,
    Match { pairs: u32 },
    Sprt { elo0: f64, elo1: f64 },
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Duel => "duel",
            Kind::Match { .. } => "match",
            Kind::Sprt { .. } => "sprt",
        }
    }

    /// Its settings as the job's words and the log spell them: `-`, the pairs, or `elo0,elo1`.
    pub fn settings(self) -> String {
        match self {
            Kind::Duel => "-".to_string(),
            Kind::Match { pairs } => pairs.to_string(),
            Kind::Sprt { elo0, elo1 } => format!("{elo0},{elo1}"),
        }
    }

    fn parse(kind: &str, settings: &str) -> Result<Kind, String> {
        match kind {
            "duel" => Ok(Kind::Duel),
            "match" => match settings.parse::<u32>() {
                Ok(pairs) if pairs > 0 => Ok(Kind::Match { pairs }),
                _ => Err(format!("a match's pairs must be a positive whole number, not {settings:?}")),
            },
            "sprt" => {
                let parsed = settings.split_once(',').and_then(|(elo0, elo1)| Some((elo0.trim().parse::<f64>().ok()?, elo1.trim().parse::<f64>().ok()?)));
                match parsed {
                    Some((elo0, elo1)) if elo0.is_finite() && elo1.is_finite() && elo1 > elo0 => Ok(Kind::Sprt { elo0, elo1 }),
                    _ => Err(format!("an SPRT's settings are elo0,elo1 with elo1 above elo0, not {settings:?}")),
                }
            }
            other => Err(format!("a test is a duel, a match or an sprt, not {other:?}")),
        }
    }

    /// The pairs it may play at most, which the smithy deals no further than.
    pub fn max_pairs(self) -> u32 {
        match self {
            Kind::Duel => DUEL_MAX_PAIRS as u32,
            Kind::Match { pairs } => pairs,
            Kind::Sprt { .. } => SPRT_MAX_PAIRS,
        }
    }
}

/// One side of a test: a network, `pesto` or a forge run's (`players::network_player_name`), and
/// whether it plays its tune (`parameters.txt`) or the defaults. Written as a player, the network
/// with what it searches with: `<network>@tuned` or `<network>@untuned`.
#[derive(Debug, Clone, PartialEq)]
pub struct Side {
    pub player: String,
    pub tuned: bool,
}

impl Side {
    /// As a player's name: `<network>@tuned` or `<network>@untuned`.
    pub fn text(&self) -> String {
        format!("{}@{}", self.player, if self.tuned { "tuned" } else { "untuned" })
    }

    /// `text` read back.
    pub fn parse(text: &str) -> Result<Side, String> {
        let wrong = || format!("a side is <network>@tuned or <network>@untuned, not {text:?}");
        let (player, suffix) = text.split_once('@').ok_or_else(wrong)?;
        if player.is_empty() {
            return Err(wrong());
        }
        let tuned = match suffix {
            "tuned" => true,
            "untuned" => false,
            _ => return Err(wrong()),
        };
        Ok(Side { player: player.to_string(), tuned })
    }
}

/// A test as the page asks for it, the challenger White in each pair's first game.
#[derive(Debug, Clone, PartialEq)]
pub struct Spec {
    pub kind: Kind,
    pub challenger: Side,
    pub champion: Side,
    pub limit: Limit,
}

impl Spec {
    /// The job's words after `--test`: kind, settings, challenger, champion, each a player
    /// (`Side::text`), and the limit (`depth <d>` or `clock <base+inc>`).
    pub fn parse(words: &[String]) -> Result<Spec, String> {
        let [kind, settings, challenger, champion, limit] = words else {
            return Err(
                "a test is --test <duel|match|sprt> <settings> <challenger>@<tuned|untuned> <champion>@<tuned|untuned> <limit>".to_string()
            );
        };
        Ok(Spec {
            kind: Kind::parse(kind, settings)?,
            challenger: Side::parse(challenger)?,
            champion: Side::parse(champion)?,
            limit: Limit::parse(limit)?,
        })
    }

    /// Whether one side plays its tune and the other the defaults: allowed, and always said.
    pub fn mixed(&self) -> bool {
        self.challenger.tuned != self.champion.tuned
    }
    pub fn name(&self) -> String {
        format!("{} of {} against {}", self.kind.name(), self.champion.text(), self.challenger.text())
    }
}

/// Where a gauge's mark stands between its two ends, each named.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Gauge {
    pub value: f64,
    pub lower: f64,
    pub upper: f64,
    pub left: &'static str,
    pub right: &'static str,
}

/// A test as its pairs so far have it.
#[derive(Debug, Clone, PartialEq)]
pub struct Judged {
    /// The pairs it counts: those up to its verdict, any after it dropped.
    pub pairs: usize,
    pub games: u64,
    pub challenger_points: f64,
    /// The rule's own state, as its log and terminal say it.
    pub status: String,
    pub gauge: Gauge,
    /// The games it will likely have played in all, for its ETA: a match's own, a duel's projection
    /// (`duel_projection`); none for an SPRT, which no count bounds well.
    pub expected_games: Option<u64>,
    /// `None` while undecided.
    pub verdict: Option<String>,
}

/// `kind` over `pairs`, each the challenger's `r` in -2..=2, in their order.
pub fn judge(kind: Kind, pairs: &[i32]) -> Judged {
    match kind {
        Kind::Duel => judge_duel(pairs),
        Kind::Match { pairs: total } => judge_match(total, pairs),
        Kind::Sprt { elo0, elo1 } => judge_sprt(elo0, elo1, pairs),
    }
}

/// The duel's threshold after `pairs` pairs of deviation `sigma`, the mean pair score either side
/// has to pass.
pub fn duel_threshold(pairs: u64, sigma: f64) -> f64 {
    0.5 + DUEL_C * sigma / (pairs as f64).sqrt()
}

/// A duel's pairs for one side, arena's `duel::Pairs`: a pair's half-points `q` (0 to 4), their
/// count, sum and sum of squares.
#[derive(Clone, Copy, Debug, PartialEq)]
struct DuelPairs {
    count: u64,
    sum: u64,
    squares: u64,
}

impl DuelPairs {
    /// `sigma_hat` of `y = q / 4`, a 0-2 and a 2-0 pseudo-pair added.
    fn sigma(self) -> f64 {
        let n = (self.count + 2) as f64;
        let sum = (self.sum + 4) as f64;
        let squares = (self.squares + 16) as f64;
        ((squares - sum * sum / n) / (n - 1.0)).max(0.0).sqrt() / 4.0
    }

    fn mean(self) -> f64 {
        self.sum as f64 / (4 * self.count) as f64
    }

    fn other_side(self) -> DuelPairs {
        DuelPairs { count: self.count, sum: 4 * self.count - self.sum, squares: 16 * self.count + self.squares - 8 * self.sum }
    }

    fn passes(self) -> bool {
        self.count > 0 && self.mean() > duel_threshold(self.count, self.sigma())
    }

    /// Whether this side could still pass at a later check, whatever the pairs to come: arena's
    /// `can_still_win`, the scores to come as equal as the grid allows for each total.
    fn can_still_win(self) -> bool {
        let left = DUEL_MAX_PAIRS.saturating_sub(self.count);
        for more in (1..=left).rev() {
            for half_points in (0..=4 * more).rev() {
                let (base, extra) = (half_points / more, half_points % more);
                let after = DuelPairs {
                    count: self.count + more,
                    sum: self.sum + half_points,
                    squares: self.squares + more * base * base + extra * (2 * base + 1),
                };
                if after.passes() {
                    return true;
                }
            }
        }
        false
    }
}

/// The pairs a duel at mean pair score `mean`, deviation `sigma`, after `pairs` will likely have
/// played in all, arena's `pairs_left_estimate`: to the leader's crossing, `(C * sigma / d)^2`, or
/// the curtailment, `MAX_PAIRS * (1 - bound) / (1 - p)`, or the cap, whichever comes first. A
/// projection, not a bound: early on the score still moves, and so does the ETA.
pub fn duel_projection(mean: f64, sigma: f64, pairs: u64) -> u64 {
    if pairs == 0 {
        return DUEL_MAX_PAIRS;
    }
    let distance = (mean - 0.5).abs();
    let won = if distance > 0.0 { (DUEL_C * sigma / distance).powi(2).ceil() } else { f64::INFINITY };
    let bound = duel_threshold(DUEL_MAX_PAIRS, sigma);
    let curtailed = (DUEL_MAX_PAIRS as f64 * (1.0 - bound) / (0.5 - distance)).ceil();
    (won.min(curtailed).min(DUEL_MAX_PAIRS as f64) as u64).max(pairs)
}

fn judge_duel(pairs: &[i32]) -> Judged {
    let mut side = DuelPairs { count: 0, sum: 0, squares: 0 };
    let (mut counted, mut verdict) = (0usize, None);
    for &r in pairs {
        let q = (r + 2) as u64;
        side = DuelPairs { count: side.count + 1, sum: side.sum + q, squares: side.squares + q * q };
        counted += 1;
        // A win before the cap, so a duel crossing on its last pair is won; curtailed after both.
        verdict = if side.passes() {
            Some("won by the challenger".to_string())
        } else if side.other_side().passes() {
            Some("won by the champion".to_string())
        } else if side.count >= DUEL_MAX_PAIRS {
            Some("drawn at the cap".to_string())
        } else if !side.can_still_win() && !side.other_side().can_still_win() {
            Some(format!("drawn, curtailed: neither can pass its bound by {} pairs", progress::grouped(DUEL_MAX_PAIRS)))
        } else {
            None
        };
        if verdict.is_some() {
            break;
        }
    }
    let games = 2 * side.count;
    let challenger_points = side.sum as f64 / 2.0;
    let score = if side.count == 0 { 0.5 } else { side.mean() };
    let sigma = side.sigma();
    let threshold = if side.count == 0 { 1.0 } else { duel_threshold(side.count, sigma).min(1.0) };
    // Said from the challenger's side, as its gauge reads: the question a test asks is the challenger's.
    // The challenger's score is left out: the table's challenger column already shows it.
    Judged {
        pairs: counted,
        games,
        challenger_points,
        status: format!(
            "thresholds {:.1}% and {:.1}%, sigma {sigma:.3}",
            100.0 * (1.0 - threshold),
            100.0 * threshold
        ),
        gauge: Gauge { value: score, lower: 1.0 - threshold, upper: threshold, left: "champion wins", right: "challenger wins" },
        expected_games: Some(if verdict.is_some() { games } else { 2 * duel_projection(score, sigma, side.count) }),
        verdict,
    }
}

/// The Elo of a score fraction, from the challenger's side; infinite at 0 and 1.
fn elo_of(score: f64) -> f64 {
    -400.0 * (1.0 / score - 1.0).log10()
}

/// The Elo estimate of `pairs`, from the challenger's side, and its 95% margin from the pairs'
/// spread (a pair, one opening with colours swapped, the unit, as the round robin's standard error).
pub fn elo_estimate(pairs: &[i32]) -> Option<(f64, f64)> {
    if pairs.is_empty() {
        return None;
    }
    let n = pairs.len() as f64;
    let scores: Vec<f64> = pairs.iter().map(|&r| f64::from(r + 2) / 4.0).collect();
    let mean = scores.iter().sum::<f64>() / n;
    let variance = (scores.iter().map(|s| s * s).sum::<f64>() / n - mean * mean).max(0.0);
    let error = 1.96 * (variance / n).sqrt();
    // Kept off 0 and 1, where the Elo is infinite.
    let clamp = |score: f64| score.clamp(0.5 / (2.0 * n), 1.0 - 0.5 / (2.0 * n));
    // `+ 0.0` turns an even score's -0.0 into 0.0, which prints without its sign.
    let elo = elo_of(clamp(mean)) + 0.0;
    let margin = (elo_of(clamp(mean + error)) - elo_of(clamp(mean - error))) / 2.0;
    Some((elo, margin))
}

fn judge_match(total: u32, pairs: &[i32]) -> Judged {
    let counted = pairs.len().min(total as usize);
    let pairs = &pairs[..counted];
    let challenger_points = pairs.iter().map(|&r| f64::from(r + 2) / 2.0).sum::<f64>();
    let games = 2 * counted as u64;
    // Said from the challenger's side, as a duel's.
    let estimate = match elo_estimate(pairs) {
        Some((elo, margin)) => format!("challenger Elo {elo:+.1} +/-{margin:.1}"),
        None => "no games yet".to_string(),
    };
    let score = if games == 0 { 0.0 } else { challenger_points / games as f64 };
    let done = counted >= total as usize;
    Judged {
        pairs: counted,
        games,
        challenger_points,
        // The challenger's score is left out: the table's challenger column already shows it.
        status: format!("{} of {} pairs, {estimate}", progress::grouped(counted), progress::grouped(total)),
        // Not a gauge but progress: the page draws it as a bar of pairs.
        gauge: Gauge { value: counted as f64, lower: 0.0, upper: f64::from(total), left: "", right: "" },
        expected_games: Some(2 * u64::from(total)),
        verdict: done.then(|| format!("challenger {:.1}%, {estimate}", 100.0 * score)),
    }
}

fn judge_sprt(elo0: f64, elo1: f64, pairs: &[i32]) -> Judged {
    let mut sprt = Sprt::between(elo0, elo1);
    let mut counted = 0;
    for &r in pairs {
        sprt.add_pair(r);
        counted += 1;
        if sprt.verdict() != Verdict::Continue {
            break;
        }
    }
    let (lower, upper) = sprt.bounds();
    let verdict = match sprt.verdict() {
        Verdict::Promote => Some(format!("H1: the challenger is stronger by {elo1} Elo or more")),
        Verdict::Fail => Some(format!("H0: the challenger is not stronger by more than {elo0} Elo")),
        Verdict::Continue if counted as u32 >= SPRT_MAX_PAIRS => Some(format!("inconclusive after {} pairs", progress::grouped(SPRT_MAX_PAIRS))),
        Verdict::Continue => None,
    };
    Judged {
        pairs: counted,
        games: sprt.games(),
        challenger_points: sprt.score(),
        // The challenger's score is left out: the table's challenger column already shows it.
        status: format!("llr {:+.2} [{lower:+.2}, {upper:+.2}]", sprt.llr()),
        gauge: Gauge { value: sprt.llr(), lower, upper, left: "fail", right: "pass" },
        expected_games: None,
        verdict,
    }
}

/// One test as the log keeps it: one tab-separated line.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Record {
    pub number: u32,
    pub started: String,
    pub ended: String,
    pub kind: String,
    pub settings: String,
    /// Each side as a player, `<network>@tuned` or `<network>@untuned` (`Side::text`).
    pub challenger: String,
    pub champion: String,
    pub limit: String,
    pub games: u64,
    pub challenger_points: f64,
    /// The verdict, or `stopped` with what it had.
    pub verdict: String,
    /// One side tuned and the other not.
    pub mixed: bool,
}

const HEADER: &str = "number\tstarted\tended\tkind\tsettings\tchallenger\tchampion\tlimit\tgames\tchallenger_points\tverdict\tmixed";

impl Record {
    /// Whether it was stopped before its verdict, and so may be resumed while it is kept.
    pub fn stopped(&self) -> bool {
        self.verdict.starts_with("stopped")
    }

    /// The test it records, as the words that asked for it said.
    pub fn spec(&self) -> Result<Spec, String> {
        let words = [&self.kind, &self.settings, &self.challenger, &self.champion, &self.limit];
        Spec::parse(&words.map(String::clone)).map_err(|message| format!("test {}: {message}", self.number))
    }

    fn line(&self) -> String {
        let clean = |text: &str| text.replace(['\t', '\n'], " ");
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            self.number,
            clean(&self.started),
            clean(&self.ended),
            clean(&self.kind),
            clean(&self.settings),
            clean(&self.challenger),
            clean(&self.champion),
            clean(&self.limit),
            self.games,
            self.challenger_points,
            clean(&self.verdict),
            u8::from(self.mixed)
        )
    }

    fn parse(line: &str) -> Option<Record> {
        let fields: Vec<&str> = line.split('\t').collect();
        let [number, started, ended, kind, settings, challenger, champion, limit, games, points, verdict, mixed] = fields.as_slice() else {
            return None;
        };
        Some(Record {
            number: number.parse().ok()?,
            started: started.to_string(),
            ended: ended.to_string(),
            kind: kind.to_string(),
            settings: settings.to_string(),
            challenger: challenger.to_string(),
            champion: champion.to_string(),
            limit: limit.to_string(),
            games: games.parse().ok()?,
            challenger_points: points.parse().ok()?,
            verdict: verdict.to_string(),
            mixed: *mixed == "1",
        })
    }
}

fn log_path(dir: &str) -> String {
    format!("{dir}/tests.tsv")
}

/// Test `number`'s pairs, beside which its games are (`test-NNNN.pgn`).
pub fn pairs_path(dir: &str, number: u32) -> String {
    format!("{dir}/test-{number:04}.tsv")
}

fn games_path(dir: &str, number: u32) -> String {
    format!("{dir}/test-{number:04}.pgn")
}

/// The logged tests in `dir`, oldest first; an unreadable line skipped.
pub fn read_log(dir: &str) -> Vec<Record> {
    let Ok(text) = std::fs::read_to_string(log_path(dir)) else {
        return Vec::new();
    };
    text.lines().skip(1).filter_map(Record::parse).collect()
}

/// The numbers of the tests with files in `dir`, a test stopped before it was logged among them.
fn numbers_on_disk(dir: &str) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let stem = name.strip_suffix(".tsv").or_else(|| name.strip_suffix(".pgn"))?;
            stem.strip_prefix("test-")?.parse().ok()
        })
        .collect()
}

/// The next test's number: past every test logged or with files.
pub fn next_number(dir: &str) -> u32 {
    let logged = read_log(dir).iter().map(|record| record.number).max().unwrap_or(0);
    let on_disk = numbers_on_disk(dir).into_iter().max().unwrap_or(0);
    logged.max(on_disk) + 1
}

/// Keeps the latest `keep` tests in `dir`, logged or with files: every older one's line, pairs and
/// games removed together.
pub fn prune(dir: &str, keep: usize) -> Result<(), String> {
    let records = read_log(dir);
    let mut numbers: Vec<u32> = records.iter().map(|record| record.number).chain(numbers_on_disk(dir)).collect();
    numbers.sort_unstable();
    numbers.dedup();
    let Some(&oldest_kept) = numbers.iter().rev().take(keep).next_back() else {
        return Ok(());
    };
    let gone: Vec<u32> = numbers.iter().copied().filter(|&number| number < oldest_kept).collect();
    if gone.is_empty() {
        return Ok(());
    }
    for &number in &gone {
        for path in [pairs_path(dir, number), games_path(dir, number)] {
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("{path}: {error}")),
            }
        }
    }
    let kept: Vec<&Record> = records.iter().filter(|record| !gone.contains(&record.number)).collect();
    let mut text = format!("{HEADER}\n");
    for record in kept {
        text.push_str(&record.line());
        text.push('\n');
    }
    std::fs::write(log_path(dir), text).map_err(|error| format!("{}: {error}", log_path(dir)))
}

/// Puts `record` in the log in `dir`: in place of the line of its number, a resumed test's, else
/// after the others; the log rewritten whole, a few lines.
pub fn put(dir: &str, record: &Record) -> Result<(), String> {
    let mut records = read_log(dir);
    match records.iter_mut().find(|held| held.number == record.number) {
        Some(held) => *held = record.clone(),
        None => records.push(record.clone()),
    }
    let mut text = format!("{HEADER}\n");
    for record in &records {
        text.push_str(&record.line());
        text.push('\n');
    }
    let path = log_path(dir);
    std::fs::write(&path, text).map_err(|error| format!("{path}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The job's words: `head`'s, then the limit, one word though it holds a space.
    fn words(head: &str, limit: &str) -> Vec<String> {
        head.split_whitespace().map(str::to_string).chain([limit.to_string()]).collect()
    }

    #[test]
    fn a_test_s_words_read_back() {
        let spec = Spec::parse(&words("sprt 0,5 lc0-psq-g1-a1-l0.03-r1@tuned pesto@untuned", "clock 10+0.1")).unwrap();
        assert_eq!(spec.kind, Kind::Sprt { elo0: 0.0, elo1: 5.0 });
        assert_eq!(spec.limit, Limit::Clock("10+0.1".to_string()));
        assert_eq!(spec.challenger, Side { player: "lc0-psq-g1-a1-l0.03-r1".to_string(), tuned: true });
        assert_eq!((spec.challenger.text(), spec.champion.text()), ("lc0-psq-g1-a1-l0.03-r1@tuned".to_string(), "pesto@untuned".to_string()));
        assert!(spec.mixed());
        assert_eq!(Spec::parse(&words("match 500 aa@untuned bb@untuned", "depth 9")).unwrap().kind, Kind::Match { pairs: 500 });
        for wrong in [
            "match 0 aa@untuned bb@untuned",
            "sprt 5,0 aa@untuned bb@untuned",
            "trio - aa@untuned bb@untuned",
            // The old words, a side's parameters apart from it.
            "duel - aa untuned bb untuned",
            "duel - aa bb@untuned",
            "duel - aa@150 bb@untuned",
            "duel - aa@t2 bb@untuned",
            "duel - @tuned bb@untuned",
            // The search switches, removed 2026-11-02.
            "duel - aa@untuned+blend bb@untuned",
        ] {
            assert!(Spec::parse(&words(wrong, "depth 9")).is_err(), "{wrong}");
        }
        assert!(Spec::parse(&words("duel - aa@untuned bb@untuned", "nodes 9")).is_err());
    }

    /// arena's duel's own cases: six pairs 2-0 the earliest win, an even duel (every pair 1-1)
    /// curtailed before the cap.
    #[test]
    fn the_duel_decides_as_arena_s() {
        assert!((duel_threshold(100, 0.5) - 0.6545).abs() < 1e-4);
        assert_eq!(judge(Kind::Duel, &[2; 5]).verdict, None, "5 pairs 2-0 are not enough");
        let won = judge(Kind::Duel, &[2; 6]);
        assert_eq!((won.verdict.as_deref(), won.games), (Some("won by the challenger"), 12));
        assert_eq!(judge(Kind::Duel, &[-2; 6]).verdict.as_deref(), Some("won by the champion"));
        assert_eq!(judge(Kind::Duel, &[0; 10]).verdict, None);
        let even = judge(Kind::Duel, &[0; 400]);
        assert!(even.verdict.unwrap().starts_with("drawn, curtailed"));
        assert!(even.games < 800, "{}", even.games);
        assert_eq!(judge(Kind::Duel, &[2; 9]).pairs, 6, "pairs after the verdict not counted");
    }

    /// A clear lead projects its crossing soon; an even score its curtailment, before the cap.
    #[test]
    fn a_duel_s_length_follows_its_score() {
        assert!(duel_projection(0.875, 0.284, 20) <= 25);
        let even = duel_projection(0.5, 0.0704, 100);
        assert!(even > 100 && even < DUEL_MAX_PAIRS, "{even}");
        assert_eq!(duel_projection(0.5, 0.5, 0), DUEL_MAX_PAIRS);
        assert_eq!(judge(Kind::Match { pairs: 7 }, &[]).expected_games, Some(14));
    }

    #[test]
    fn a_match_plays_its_pairs_and_estimates_the_elo() {
        assert_eq!(judge(Kind::Match { pairs: 4 }, &[0, 0, 0]).verdict, None);
        let done = judge(Kind::Match { pairs: 4 }, &[0, 0, 0, 0, 2]);
        assert_eq!((done.pairs, done.games), (4, 8));
        assert!(done.verdict.unwrap().contains("challenger Elo +0.0"));
        assert!(judge(Kind::Match { pairs: 4 }, &[2, 0, 2, 0]).verdict.unwrap().starts_with("challenger 75.0%, challenger Elo +190.8"));
        // 75% of the points: +190.8 Elo.
        let (elo, margin) = elo_estimate(&[2, 0, 2, 0]).unwrap();
        assert!((elo - 190.85).abs() < 0.1, "{elo}");
        assert!(margin > 0.0);
    }

    #[test]
    fn an_sprt_decides_between_its_two_hypotheses() {
        let strong = judge(Kind::Sprt { elo0: 0.0, elo1: 35.0 }, &[2; 200]);
        assert!(strong.verdict.unwrap().starts_with("H1"));
        let even = judge(Kind::Sprt { elo0: 0.0, elo1: 35.0 }, &[0; 2000]);
        assert!(even.verdict.unwrap().starts_with("H0"));
    }

    fn record(number: u32) -> Record {
        Record {
            number,
            started: "s".to_string(),
            ended: "e".to_string(),
            kind: "duel".to_string(),
            settings: "-".to_string(),
            challenger: "aa@untuned".to_string(),
            champion: "bb@tuned".to_string(),
            limit: "depth 9".to_string(),
            games: 10,
            challenger_points: 7.5,
            verdict: "won by the challenger".to_string(),
            mixed: true,
        }
    }

    #[test]
    fn the_log_keeps_the_latest_tests_with_their_games() {
        let dir = std::env::temp_dir().join(format!("fattorini-smithy-matches-{}", std::process::id())).display().to_string();
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        for number in 1..=4 {
            put(&dir, &record(number)).unwrap();
            std::fs::write(pairs_path(&dir, number), "").unwrap();
            std::fs::write(games_path(&dir, number), "").unwrap();
        }
        // Test 5 stopped before it was logged: it counts, by its files.
        std::fs::write(games_path(&dir, 5), "").unwrap();
        assert_eq!(next_number(&dir), 6);
        assert_eq!(read_log(&dir)[0], record(1), "a line reads back");

        prune(&dir, 2).unwrap();
        assert_eq!(read_log(&dir).iter().map(|record| record.number).collect::<Vec<_>>(), [4]);
        assert!(!std::path::Path::new(&games_path(&dir, 3)).exists());
        assert!(std::path::Path::new(&games_path(&dir, 4)).exists() && std::path::Path::new(&games_path(&dir, 5)).exists());
        assert_eq!(next_number(&dir), 6);

        // Test 4 stopped, then resumed to its verdict: its line replaced, not added.
        let stopped = Record { verdict: "stopped: no games yet".to_string(), ..record(4) };
        put(&dir, &stopped).unwrap();
        assert!(read_log(&dir)[0].stopped());
        assert_eq!(read_log(&dir)[0].spec().unwrap(), Spec::parse(&words("duel - aa@untuned bb@tuned", "depth 9")).unwrap());
        put(&dir, &record(4)).unwrap();
        assert_eq!(read_log(&dir), [record(4)]);
        std::fs::remove_dir_all(&dir).ok();
    }
}
