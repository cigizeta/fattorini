//! What training reports of its passes: a round's pass, as a plain record the page's training box
//! draws (`print_progress`, `note_pass`, `current_pass`); and each round's time, `timing_line`, its
//! event's first words.

/// A round's pass under way, as the page shows it in the training box. In memory alone: no file
/// holds a round until it ends, and a restart redoes the round in flight anyway.
struct RoundPass {
    /// `"training"` or `"test"` (`note_pass`).
    half: &'static str,
    /// The suffix, `round N`.
    round: String,
    unit: &'static str,
    position: u64,
    total: u64,
    /// When the pass began, and at what position: a resumed round's steps already done are no part
    /// of its rate.
    began: std::time::Instant,
    began_at: u64,
}

static ROUND_PASS: std::sync::Mutex<Option<RoundPass>> = std::sync::Mutex::new(None);

/// The round's pass under way, for the page.
#[derive(Clone, serde::Serialize)]
pub(crate) struct RoundPassView {
    half: &'static str,
    round: String,
    unit: &'static str,
    done: u64,
    total: u64,
    /// Units a second since the pass began; none in its first second.
    rate: Option<f64>,
    eta_seconds: Option<f64>,
}

/// Notes the training's pass under way at `position` of `total`: `half` is `"training"` or `"test"`
/// within round `round`. A pass continues while its half, round and total stay and it moves forward.
pub(crate) fn note_pass(half: &'static str, round: &str, unit: &'static str, position: u64, total: u64) {
    let mut pass = ROUND_PASS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let same = pass.as_ref().is_some_and(|p| p.half == half && p.round == round && p.total == total && position >= p.position);
    match pass.as_mut() {
        Some(pass) if same => pass.position = position,
        _ => {
            *pass = Some(RoundPass {
                half,
                round: round.to_string(),
                unit,
                position,
                total,
                began: std::time::Instant::now(),
                began_at: position,
            });
        }
    }
}

/// Forgets the pass `note_pass` keeps: it ended.
pub(crate) fn end_pass() {
    *ROUND_PASS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
}

/// The round's pass under way, none between passes or outside training.
pub(crate) fn current_pass() -> Option<RoundPassView> {
    let current = ROUND_PASS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    current.as_ref().map(|pass| {
        let seconds = pass.began.elapsed().as_secs_f64();
        let moved = pass.position.saturating_sub(pass.began_at);
        let rate = (seconds >= 1.0 && moved > 0).then(|| moved as f64 / seconds);
        RoundPassView {
            half: pass.half,
            round: pass.round.clone(),
            unit: pass.unit,
            done: pass.position,
            total: pass.total,
            rate,
            eta_seconds: rate.map(|rate| pass.total.saturating_sub(pass.position) as f64 / rate),
        }
    })
}

/// How long round `round` took, training and test.
pub(crate) fn timing_line(round: u64, train_secs: f64, test_secs: f64) -> String {
    format!("round {round} took {:.1}s: training {train_secs:.1}s, test {test_secs:.1}s", train_secs + test_secs)
}

/// Notes one pass's live progress for the page (`note_pass`). `label` is `"train"` or `"test"`
/// (`round::run_round`'s own two passes' names); `round` is which round this is, `round N`.
///
/// The training half's position is the one within the round, which is exactly `steps_per_round`
/// steps. `processed` is the absolute step count -- that is what `run_round` reports -- so the
/// position within the round is taken modulo `total` here.
pub(crate) fn print_progress(label: &str, processed: u64, total: u64, round: &str) {
    let (position, unit) = if label == "train" {
        // A round that has just landed on the boundary reads as `0` into the round, not as complete,
        // so the last redraw of a round would otherwise jump back to 0% instead of finishing at 100%.
        let into_round = if total == 0 { 0 } else { processed % total };
        (if into_round == 0 && processed > 0 { total } else { into_round }, "steps")
    } else {
        (processed, "rows")
    };
    // The page words the pass "training", while `run_round` keeps calling it "train".
    note_pass(if label == "train" { "training" } else { "test" }, round, unit, position, total);
}

/// Ends the page's pass when dropped, so training leaving by any path (finished, stopped, a panic)
/// leaves no round on the page for the next training to show before its own first pass.
pub(crate) struct PassEnd;

impl Drop for PassEnd {
    fn drop(&mut self) {
        end_pass();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_s_pass_stays_until_training_leaves() {
        end_pass();
        print_progress("train", 2 * 1024 + 500, 1024, "round 3");
        let pass = current_pass().expect("a pass under way");
        assert_eq!((pass.half, pass.round.as_str(), pass.done), ("training", "round 3", 500));
        drop(PassEnd);
        assert!(current_pass().is_none(), "training left: no pass");
    }

    #[test]
    fn a_round_s_time_reads_with_no_space_before_its_seconds() {
        assert_eq!(timing_line(14, 84.0, 5.5), "round 14 took 89.5s: training 84.0s, test 5.5s");
    }
}
