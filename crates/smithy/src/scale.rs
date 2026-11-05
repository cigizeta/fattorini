//! A scaled run's scale, chosen at New run (`GenConfig::scale`): one factor `s` shrinks a config, each
//! size by the kind of quantity it is, down to the floor below which its phase means nothing. The shape of a run (its
//! grid, its warm-up and patience, its rate stages, its gate) never scales, so every stage, early stop
//! and gate rule still runs; `s = 1` is the config itself.
//!
//! - **Data volumes,** positions and bytes, by `1/s`: a phase's cost is about linear in them.
//! - **Work a round,** steps and batch sizes, by `1/s`, for the same reason.
//! - **Game counts,** pairs, by `sqrt(s)`: a tournament's statistics need far fewer games than a
//!   training set needs positions, so a linear cut would leave a round robin of no games.
//! - **Search effort,** depth, by `log2(s) / 2` plies: each ply about doubles a search's cost.
//! - **A tune's clock,** its base and increment, by `sqrt(s)`, as its pairs: a game's time and the
//!   tune's pairs shrink alike.

/// Every new scaled run's factor, fixed (the developer, 2026-10-29): minutes a phase on an ordinary
/// CPU, the floors below chosen for it. A run records its own in its config, so changing this never
/// changes a run already started.
pub(crate) const DEFAULT: u64 = 1024;

/// The fewest positions in a batch, training or test: below this a batch's loss is mostly noise.
pub(crate) const MIN_BATCH: u64 = 256;

/// The fewest positions in a training set or a corpus: a few batches of `MIN_BATCH` for each of a few
/// rounds, so the bucket scatter, the stream and the rounds all see more than one batch.
pub(crate) const MIN_POSITIONS: u64 = 16 * MIN_BATCH;

/// The fewest pairs each round robin pairing plays: two, so a pairing has more than one result.
pub(crate) const MIN_PAIRS: u32 = 2;

/// The shallowest search a scaled game plays: below it, the engine's labels are hardly a search.
const MIN_DEPTH: u32 = 2;

/// The shortest base and increment a scaled clock gives, in seconds: below them a move is mostly the
/// engine's overhead, and games are lost on time.
const MIN_BASE_SECONDS: f64 = 1.0;
const MIN_INCREMENT_SECONDS: f64 = 0.01;

/// `value` scaled by `1/s`, never below `floor`, nor above `value` itself.
pub(crate) fn volume(value: u64, s: u64, floor: u64) -> u64 {
    (value / s.max(1)).max(floor.min(value))
}

/// `value`, a count of games, scaled by `1/sqrt(s)`, never below `floor`, nor above `value` itself.
pub(crate) fn games(value: u32, s: u64, floor: u32) -> u32 {
    let scaled = (f64::from(value) / (s.max(1) as f64).sqrt()).floor() as u32;
    scaled.max(floor.min(value))
}

/// A search `depth` scaled by `s`: `log2(s) / 2` plies fewer, never below `MIN_DEPTH`, nor above `depth`.
pub(crate) fn depth(depth: u32, s: u64) -> u32 {
    let plies = (s.max(1) as f64).log2() / 2.0;
    let scaled = (f64::from(depth) - plies).round().max(0.0) as u32;
    scaled.max(MIN_DEPTH.min(depth))
}

/// A tune's control scaled by `s`: a clock `base+inc`, each by `1/sqrt(s)`, never below
/// `MIN_BASE_SECONDS` and `MIN_INCREMENT_SECONDS`, nor above itself; `depth <d>` as `depth` does.
/// Anything else is returned as it is, for the tune's own checks to refuse.
pub(crate) fn control(control: &str, s: u64) -> String {
    if let Some(d) = control.strip_prefix("depth ").and_then(|d| d.parse::<u32>().ok()) {
        return format!("depth {}", depth(d, s));
    }
    let Some((Ok(base), Ok(increment))) = control.split_once('+').map(|(base, increment)| (base.parse::<f64>(), increment.parse::<f64>())) else {
        return control.to_string();
    };
    let root = (s.max(1) as f64).sqrt();
    let scaled = |value: f64, floor: f64| -> String {
        let seconds = format!("{:.3}", (value / root).max(floor.min(value)));
        seconds.trim_end_matches('0').trim_end_matches('.').to_string()
    };
    format!("{}+{}", scaled(base, MIN_BASE_SECONDS), scaled(increment, MIN_INCREMENT_SECONDS))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tune_s_clock_shrinks_as_its_pairs_down_to_its_floors() {
        assert_eq!(control("10+0.1", 1), "10+0.1", "a scale of one is the control itself");
        assert_eq!(control("10+0.1", 4), "5+0.05");
        assert_eq!(control("10+0.1", DEFAULT), "1+0.01", "both floors");
        assert_eq!(control("0.5+0", DEFAULT), "0.5+0", "a floor never raises a clock past the config's");
        assert_eq!(control("depth 9", DEFAULT), "depth 4");
        assert_eq!(control("nonsense", DEFAULT), "nonsense");
    }

    #[test]
    fn a_scale_of_one_is_the_config_itself() {
        assert_eq!(volume(100_663_296, 1, MIN_POSITIONS), 100_663_296);
        assert_eq!(games(400, 1, MIN_PAIRS), 400);
        assert_eq!(depth(9, 1), 9);
    }

    #[test]
    fn each_kind_scales_its_own_way() {
        assert_eq!(volume(100_663_296, DEFAULT, MIN_POSITIONS), 98_304);
        assert_eq!(games(400, DEFAULT, MIN_PAIRS), 12);
        assert_eq!(depth(9, DEFAULT), 4);
    }

    #[test]
    fn every_floor_holds_at_a_huge_scale_and_never_raises_a_size() {
        let huge = u64::MAX;
        assert_eq!(volume(100_663_296, huge, MIN_POSITIONS), MIN_POSITIONS);
        assert_eq!(games(400, huge, MIN_PAIRS), MIN_PAIRS);
        assert_eq!(depth(9, huge), MIN_DEPTH);
        assert_eq!(volume(100, huge, MIN_POSITIONS), 100, "a floor never raises a size past the config's");
        assert_eq!(games(1, huge, MIN_PAIRS), 1);
        assert_eq!(depth(1, huge), 1);
    }
}
