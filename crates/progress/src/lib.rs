//! The progress of `arena` and `smithy`, as their page shows it: every bar made here is registered,
//! and `live` lists the bars under way, each as one line's texts,
//!
//! `{label} [{elapsed}] [{bar}] {pos}/{len} {unit} ({percent}%) ETA {eta} ({rate})`
//!
//! No bar is drawn on a terminal: `smithy start` and `arena start`'s terminal is a log of events
//! (`log_events`), and their bars are the page's alone.
//!
//! The ETA and the rate are unsmoothed: the plain average since the bar was made, measured from its
//! start position. The start position matters after a resume: work already done before this process
//! started took no time this run, so counting it would make the rate look artificially fast right
//! after resuming. Before any work this run, both read `-`.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// What a bar counts: a plural noun (`positions`, `games`, `steps`), or bytes, printed
/// human-readable in the counts and the rate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Count(&'static str),
    Bytes,
}

/// Makes this process's terminal a log of events: the `log` macros print one line each to stdout,
/// `[<UTC time> <level>] <message>`, at `info` and above unless `RUST_LOG` says otherwise. For a
/// server, `smithy start` or `arena start`, called once at its start.
pub fn log_events() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .target(env_logger::Target::Stdout)
        // Its own line rather than env_logger's, which pads the level to `ERROR`'s width (`INFO `); the
        // level in env_logger's colour all the same, dropped where the output is no terminal.
        .format(|buf, record| {
            let style = buf.default_level_style(record.level());
            std::io::Write::write_fmt(
                buf,
                format_args!("[{} {style}{}{style:#}] {}\n", buf.timestamp_seconds(), record.level(), record.args()),
            )
        })
        .init();
}

/// A duration as `HH:MM:SS`, or `Nd HH:MM:SS` from a day up.
pub fn duration_text(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let (days, hours, minutes, seconds) = (seconds / 86_400, seconds / 3_600 % 24, seconds / 60 % 60, seconds % 60);
    if days > 0 { format!("{days}d {hours:02}:{minutes:02}:{seconds:02}") } else { format!("{hours:02}:{minutes:02}:{seconds:02}") }
}

/// Bytes in binary units: `512 B`, `3.00 MiB`.
pub fn bytes_text(bytes: u64) -> String {
    const PREFIXES: [&str; 6] = ["Ki", "Mi", "Gi", "Ti", "Pi", "Ei"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut amount = bytes as f64 / 1024.0;
    let mut prefix = 0;
    while amount >= 1024.0 && prefix + 1 < PREFIXES.len() {
        amount /= 1024.0;
        prefix += 1;
    }
    format!("{amount:.2} {}B", PREFIXES[prefix])
}

/// The time left at this run's average rate: `observed` units done in `elapsed`, `remaining` to go.
pub fn eta_text(observed: u64, remaining: u64, elapsed: Duration) -> String {
    let seconds = elapsed.as_secs_f64();
    if observed == 0 || seconds <= 0.0 {
        return "-".to_string();
    }
    duration_text(Duration::from_secs_f64(remaining as f64 * seconds / observed as f64))
}

/// This run's average rate: `observed` units in `elapsed`. A count under one a second is given as
/// seconds per unit, as tqdm does, so a slow bar (an SPSA's pairs, seconds apart) reads as a number
/// rather than `0.0/s`; its noun singular, the plural's final `s` dropped.
pub fn rate_text(observed: u64, elapsed: Duration, unit: Unit) -> String {
    let seconds = elapsed.as_secs_f64();
    if observed == 0 || seconds <= 0.0 {
        return "-".to_string();
    }
    let per_second = observed as f64 / seconds;
    match unit {
        Unit::Count(noun) if per_second >= 1.0 => format!("{per_second:.1} {noun}/s"),
        Unit::Count(noun) => format!("{:.1} s/{}", 1.0 / per_second, noun.strip_suffix('s').unwrap_or(noun)),
        Unit::Bytes => format!("{}/s", bytes_text(per_second as u64)),
    }
}

/// What a bar holds, shared between its owner and the registry.
struct Shared {
    label: String,
    unit: Unit,
    length: u64,
    /// Where this run's rate and ETA are measured from.
    start: u64,
    made: Instant,
    position: AtomicU64,
    finished: AtomicBool,
}

/// A bar under way, listed by `live` until finished or dropped.
pub struct Bar(Arc<Shared>);

impl Bar {
    pub fn inc(&self, delta: u64) {
        self.0.position.fetch_add(delta, Ordering::Relaxed);
    }

    pub fn set_position(&self, position: u64) {
        self.0.position.store(position, Ordering::Relaxed);
    }

    pub fn position(&self) -> u64 {
        self.0.position.load(Ordering::Relaxed)
    }

    /// Leaves `live`'s list: the work is done.
    pub fn finish(&self) {
        self.0.finished.store(true, Ordering::Relaxed);
    }
}

/// Every bar made in this process, held weakly, in the order made.
static REGISTRY: Mutex<Vec<Weak<Shared>>> = Mutex::new(Vec::new());

fn registry() -> std::sync::MutexGuard<'static, Vec<Weak<Shared>>> {
    REGISTRY.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A bar being drawn, as its line reads: `{label} [{elapsed}] [bar] {counts} ({percent}%) ETA {eta}
/// ({rate})`. `position` and `length` are for the bar's fill.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LiveBar {
    pub label: String,
    pub elapsed: String,
    /// `pos/len noun`; in bytes, human-readable.
    pub counts: String,
    pub position: u64,
    /// Always some since every bar has a length; kept optional for the page, which reads it so.
    pub length: Option<u64>,
    /// To one decimal (`percent_done`).
    pub percent: Option<f64>,
    pub eta: Option<String>,
    pub rate: String,
}

/// The bars under way in this process, in the order made: a finished one left out, a dropped one
/// forgotten. Then the notices (`notice`), each as a bar with no length whose `counts` is its text and
/// whose `rate` is empty, which the page draws as its line alone.
pub fn live() -> Vec<LiveBar> {
    let mut registry = registry();
    registry.retain(|shared| shared.strong_count() > 0);
    let mut live: Vec<LiveBar> = registry
        .iter()
        .filter_map(|shared| {
            let shared = shared.upgrade()?;
            if shared.finished.load(Ordering::Relaxed) {
                return None;
            }
            Some(snapshot(&shared))
        })
        .collect();
    drop(registry);
    let mut notices = notices();
    notices.retain(|text| text.strong_count() > 0);
    live.extend(notices.iter().filter_map(Weak::upgrade).map(|text| {
        let text = text.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
        LiveBar { label: String::new(), elapsed: String::new(), counts: text, position: 0, length: None, percent: None, eta: None, rate: String::new() }
    }));
    live
}

/// A line of text shown among the live bars while held, its text replaced at will: a state with no
/// count to draw, such as a read waiting for Google Drive.
pub struct Notice(Arc<Mutex<String>>);

impl Notice {
    pub fn set(&self, text: String) {
        *self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = text;
    }
}

/// Every notice made in this process, held weakly: a dropped one forgotten.
static NOTICES: Mutex<Vec<Weak<Mutex<String>>>> = Mutex::new(Vec::new());

fn notices() -> std::sync::MutexGuard<'static, Vec<Weak<Mutex<String>>>> {
    NOTICES.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A new notice reading `text`, listed by `live` until dropped.
pub fn notice(text: String) -> Notice {
    let shared = Arc::new(Mutex::new(text));
    let mut notices = notices();
    notices.retain(|text| text.strong_count() > 0);
    notices.push(Arc::downgrade(&shared));
    Notice(shared)
}

/// `n` with its thousands grouped by commas, `1,199,290,155`, as the page shows every other count.
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut text = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            text.push(',');
        }
        text.push(digit);
    }
    text
}

/// `position` of `length` in percent, to one decimal, rounded down so that only a finished bar reads
/// 100.0; a bar of no length is done.
fn percent_done(position: u64, length: u64) -> f64 {
    let tenths = (1000 * u128::from(position.min(length))).checked_div(u128::from(length)).unwrap_or(1000);
    tenths as f64 / 10.0
}

/// A bar as `live` lists it.
fn snapshot(bar: &Shared) -> LiveBar {
    let (position, length, elapsed) = (bar.position.load(Ordering::Relaxed), bar.length, bar.made.elapsed());
    let counts = match bar.unit {
        Unit::Count(noun) => format!("{}/{} {noun}", grouped(position), grouped(length)),
        Unit::Bytes => format!("{}/{}", bytes_text(position), bytes_text(length)),
    };
    let observed = position.saturating_sub(bar.start);
    LiveBar {
        label: bar.label.clone(),
        elapsed: duration_text(elapsed),
        counts,
        position,
        length: Some(length),
        percent: Some(percent_done(position, length)),
        eta: Some(eta_text(observed, length.saturating_sub(position), elapsed)),
        rate: rate_text(observed, elapsed, bar.unit),
    }
}

/// A new bar of `len` units at 0, labelled `label` (empty for none).
pub fn bar(len: u64, unit: Unit, label: &str) -> Bar {
    resumed(0, len, unit, label)
}

/// A bar of `len` units already at `start`, its ETA and rate counting only what this run does.
pub fn resumed(start: u64, len: u64, unit: Unit, label: &str) -> Bar {
    let start = start.min(len);
    let shared = Arc::new(Shared {
        label: label.to_string(),
        unit,
        length: len,
        start,
        made: Instant::now(),
        position: AtomicU64::new(start),
        finished: AtomicBool::new(false),
    });
    let mut registry = registry();
    registry.retain(|shared| shared.strong_count() > 0);
    registry.push(Arc::downgrade(&shared));
    Bar(shared)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn before_any_work_the_eta_and_rate_are_a_dash() {
        assert_eq!(eta_text(0, 100, Duration::from_secs(5)), "-");
        assert_eq!(eta_text(10, 100, Duration::ZERO), "-");
        assert_eq!(rate_text(0, Duration::from_secs(5), Unit::Count("games")), "-");
        assert_eq!(rate_text(10, Duration::ZERO, Unit::Bytes), "-");
    }

    #[test]
    fn the_eta_and_rate_are_this_runs_plain_average() {
        // 10 units in 5 s: 2 a second, so 100 more take 50 s.
        assert_eq!(eta_text(10, 100, Duration::from_secs(5)), "00:00:50");
        assert_eq!(rate_text(10, Duration::from_secs(5), Unit::Count("positions")), "2.0 positions/s");
        assert_eq!(rate_text(3 * 1024 * 1024, Duration::from_secs(1), Unit::Bytes), "3.00 MiB/s");
    }

    #[test]
    fn a_count_under_one_a_second_is_given_as_seconds_per_unit() {
        // 40 pairs in 5 minutes, 7.5 s each; exactly one a second stays a rate.
        assert_eq!(rate_text(40, Duration::from_secs(300), Unit::Count("pairs")), "7.5 s/pair");
        assert_eq!(rate_text(3, Duration::from_secs(600), Unit::Count("games")), "200.0 s/game");
        assert_eq!(rate_text(60, Duration::from_secs(60), Unit::Count("games")), "1.0 games/s");
    }

    #[test]
    fn durations_and_bytes_read_as_the_page_always_showed_them() {
        assert_eq!(duration_text(Duration::ZERO), "00:00:00");
        assert_eq!(duration_text(Duration::from_secs(3 * 3600 + 25 * 60 + 7)), "03:25:07");
        assert_eq!(duration_text(Duration::from_secs(2 * 86_400 + 3600 + 1)), "2d 01:00:01");
        assert_eq!(bytes_text(0), "0 B");
        assert_eq!(bytes_text(1023), "1023 B");
        assert_eq!(bytes_text(1024), "1.00 KiB");
        assert_eq!(bytes_text(23 * 1024 * 1024 * 1024 + 200 * 1024 * 1024), "23.20 GiB");
    }

    /// The live bar labelled `label`, if listed. Tests run in parallel, sharing the registry, so each
    /// looks for its own bars by a label of its own.
    fn listed(label: &str) -> Option<LiveBar> {
        live().into_iter().find(|bar| bar.label == label)
    }

    #[test]
    fn a_resumed_bar_starts_at_its_position_capped_at_its_length() {
        assert_eq!(resumed(30, 100, Unit::Count("steps"), "capped train").position(), 30);
        assert_eq!(resumed(130, 100, Unit::Count("steps"), "").position(), 100);
        assert_eq!(bar(7, Unit::Bytes, "").position(), 0);
    }

    #[test]
    fn a_count_groups_its_thousands() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1000), "1,000");
        assert_eq!(grouped(1_199_290_155), "1,199,290,155");
    }

    #[test]
    fn the_percentage_has_one_decimal_and_reads_100_only_when_done() {
        assert_eq!(percent_done(1, 3), 33.3);
        assert_eq!(percent_done(9_999, 10_000), 99.9);
        assert_eq!(percent_done(10_000, 10_000), 100.0);
        assert_eq!(percent_done(130, 100), 100.0);
        assert_eq!(percent_done(0, 0), 100.0);
        assert_eq!(percent_done(u64::MAX - 1, u64::MAX), 99.9);
    }

    #[test]
    fn a_bar_made_is_live_until_finished_or_dropped() {
        let train = resumed(30, 100, Unit::Count("steps"), "live train");
        train.set_position(80);
        let shown = listed("live train").expect("listed");
        assert_eq!((shown.counts.as_str(), shown.position, shown.length, shown.percent), ("80/100 steps", 80, Some(100), Some(80.0)));
        assert!(shown.eta.is_some());
        train.finish();
        assert_eq!(listed("live train"), None, "finished, left out");

        let read = bar(10, Unit::Bytes, "live read");
        read.inc(3);
        assert_eq!(listed("live read").expect("listed").counts, "3 B/10 B");
        drop(read);
        assert_eq!(listed("live read"), None, "dropped, forgotten");
    }

    fn noticed(text: &str) -> Option<LiveBar> {
        live().into_iter().find(|bar| bar.counts == text)
    }

    #[test]
    fn a_notice_is_a_line_alone_while_held() {
        let waiting = notice("notice test: first".to_string());
        let shown = noticed("notice test: first").expect("listed");
        assert_eq!((shown.label.as_str(), shown.length, shown.rate.as_str()), ("", None, ""));
        waiting.set("notice test: second".to_string());
        assert!(noticed("notice test: first").is_none());
        assert!(noticed("notice test: second").is_some());
        drop(waiting);
        assert!(noticed("notice test: second").is_none(), "dropped, forgotten");
    }
}
