use std::sync::LazyLock;

use crate::board::color::Color;

/// A label scale: the curve `score(pawns) = atan(pawns / a) / k`, clamped to `[-1, 1]`, and its
/// inverse `pawns(q) = a * tan(k * q)`, the same shape Lc0 uses to display its Q as centipawns. Two
/// numbers fix one: **the ceiling**, the evaluation in pawns the curve reaches 1 at
/// (`atan(ceiling / a) = k`), and **the score of one pawn** (`atan(1 / a) / k`). The search needs no
/// curve, it compares centipawns; a scale is used only where centipawns meet the result in a label.
///
/// Odd around `0` (`score(-pawns) == -score(pawns)`), which negamax needs: a network trained to
/// predict a label is a drop-in evaluation for a search that flips perspective by negation. `atan`'s
/// tail is polynomial, not exponential as the earlier `tanh` scale's was, so large advantages stay
/// apart in label space. Past the ceiling the label is exactly `±1`, mate scores included.
///
/// Two roles, two scales (the developer, 2026-11-03): `NETWORK`, fixed, reads every network's output
/// in the loss and so defines what a network's centipawn means; a teacher's own scale, fitted on its
/// games, reads that teacher's centipawns into labels (the score of one pawn at which its games' mean
/// result is best matched), since a pawn up converts far more reliably in depth-9 self-play than
/// `NETWORK` says. `LC0_FILES` reads Lc0's converted corpus, written through it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale {
    a: f64,
    k: f64,
    ceiling: f64,
}

impl Scale {
    /// The scale whose score of one pawn is `one_pawn`, reaching 1 at `ceiling` pawns: `a` by
    /// bisection, the score of one pawn falling as `a` grows, `k = atan(ceiling / a)`. `one_pawn`
    /// must lie strictly between `1 / ceiling` and `1`.
    pub fn with_one_pawn(one_pawn: f64, ceiling: f64) -> Scale {
        assert!(ceiling > 1.0 && one_pawn > 1.0 / ceiling && one_pawn < 1.0, "no atan scale has one pawn at {one_pawn} and a ceiling of {ceiling}");
        let at_one = |a: f64| (1.0 / a).atan() / (ceiling / a).atan();
        let (mut low, mut high): (f64, f64) = (1e-6, 1e6);
        for _ in 0..200 {
            let middle = (low * high).sqrt();
            if at_one(middle) > one_pawn { low = middle } else { high = middle }
        }
        let a = (low * high).sqrt();
        Scale { a, k: (ceiling / a).atan(), ceiling }
    }

    /// The scale of one pawn at exactly 1/2 and `ceiling`, in closed form: with `t = tan(k / 2)`, the
    /// anchor gives `atan(1 / a) = k / 2`, so `a = 1 / t`; the ceiling with `tan(k) = 2t / (1 - t^2)`
    /// gives `ceiling = 2 / (1 - t^2)`, so `t = sqrt(1 - 2 / ceiling)`, and `k = 2 atan(t)`.
    fn half_at_one_pawn(ceiling: f64) -> Scale {
        let t = (1.0 - 2.0 / ceiling).sqrt();
        Scale { a: 1.0 / t, k: 2.0 * t.atan(), ceiling }
    }

    /// A pawn evaluation, White-relative or the mover's, as a score in `[-1, 1]`.
    pub fn score(&self, pawns: f64) -> f64 {
        ((pawns / self.a).atan() / self.k).clamp(-1.0, 1.0)
    }

    /// The exact inverse of `score` on `[-1, 1]`, in pawns, `±ceiling` at `±1`; clamped first, since
    /// past `±1` `tan` wraps around.
    pub fn pawns(&self, score: f64) -> f64 {
        self.a * (self.k * score.clamp(-1.0, 1.0)).tan()
    }

    /// Where the curve bends, in pawns; for the trainer, which computes `score` on tensors.
    pub fn a(&self) -> f64 {
        self.a
    }

    /// `atan(ceiling / a)`, below `pi / 2`; for the trainer, as `a`.
    pub fn k(&self) -> f64 {
        self.k
    }

    pub fn ceiling(&self) -> f64 {
        self.ceiling
    }

    pub fn one_pawn(&self) -> f64 {
        self.score(1.0)
    }
}

/// **The networks' unit, f: one pawn at 1/2, the ceiling at 128 pawns (the developer, 2026-11-03).**
/// One pawn a 75% expected score, the popular reading of `+1.00`. The ceiling: the largest material
/// advantage chess allows is 103 pawns (nine queens, two rooks, two bishops and two knights against
/// a bare king), and 128 the first power of two above it, 12,800 centipawns far below the tablebase
/// band, so a network's evaluation is never mistaken for a proven result. It was 32 pawns (one side's
/// non-pawn army, the queen counted 10) until 2026-11-03, exceeded by ordinary material; below 3 pawns
/// the two curves differ by under 0.01.
pub const CEILING_PAWNS: f64 = 128.0;

/// f (`CEILING_PAWNS`): what every network's output is read through in the loss, so what a network's
/// centipawn means. Fixed: a teacher's calibration corrects the labels, never this.
pub static NETWORK: LazyLock<Scale> = LazyLock::new(|| Scale::half_at_one_pawn(CEILING_PAWNS));

/// **The scale Lc0's corpus is written in, frozen:** one pawn at 1/2, the ceiling at 32 pawns, the
/// networks' scale until 2026-11-03. Convert writes Lc0's Q as pawns through it and the labels read
/// them back through it, so a label's score is Q exactly; never changed, so every file converted, and
/// every frozen test set, keeps its meaning. `4 / sqrt(15)` and `acos(1 / 31)` as literals, asserted
/// by a test.
pub const LC0_FILES: Scale = Scale { a: 1.0327955589886445, k: 1.538532665126657, ceiling: 32.0 };

/// Lc0's own centipawn curve, `cp = A * tan(B * q)` (score type `centipawn` in Lc0's
/// `src/mcts/search.cc` as of v0.31.2), A in pawns: 90 centipawns.
pub const LC0_A_PAWNS: f64 = 0.90;

/// Lc0's B, below `pi / 2`, so `q = 1` sits at the finite `A * tan(B)`, about 127.8 pawns.
pub const LC0_B: f64 = 1.5637541897;

/// **Lc0's curve as a `Scale`:** `score(pawns) = atan(pawns / A) / B`, clamped to `[-1, 1]`, and
/// `pawns(q) = A * tan(B * q)`. Lc0's files are written and read through it once rewritten from
/// `LC0_FILES`, so their centipawns are Lc0's own.
pub static LC0: LazyLock<Scale> = LazyLock::new(|| Scale { a: LC0_A_PAWNS, k: LC0_B, ceiling: LC0_A_PAWNS * LC0_B.tan() });

/// **S, the networks' unit under the logistic curve, in pawns: `A * B / 2`, about 0.7037 (70.37
/// centipawns).** A network is trained so that `sigma(pawns / S)` is its expected score, that is
/// `q = tanh(pawns / (2 S))`. S cannot be derived from training: with a linear output layer, scaling
/// S and the output weights together changes neither predictions nor loss, so S only fixes the unit,
/// by convention. The convention: near equality a network's centipawns read as Lc0's. Lc0's curve has
/// slope `1 / (A B)` at `q = 0`, the logistic `1 / (2 S)`, so `2 S = A B`. Strength does not depend
/// on it: the search's margins are tuned afterwards to whatever unit results.
pub const S_PAWNS: f64 = LC0_A_PAWNS * LC0_B / 2.0;

/// A curve from pawns to a score in `[-1, 1]`: the atan of a `Scale`, or the logistic of scale `s`
/// pawns, `score(pawns) = 2 sigma(pawns / s) - 1 = tanh(pawns / (2 s))`. Both odd around `0`, as
/// negamax needs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    Atan(Scale),
    Logistic { s: f64 },
}

impl Curve {
    /// A pawn evaluation, White-relative or the mover's, as a score in `[-1, 1]`.
    pub fn score(&self, pawns: f64) -> f64 {
        match self {
            Curve::Atan(scale) => scale.score(pawns),
            Curve::Logistic { s } => (pawns / (2.0 * s)).tanh(),
        }
    }

    /// The inverse of `score`, in pawns. The logistic reaches `±1` only at infinity, so its inverse of
    /// `±1` is infinite.
    pub fn pawns(&self, score: f64) -> f64 {
        match self {
            Curve::Atan(scale) => scale.pawns(score),
            Curve::Logistic { s } => 2.0 * s * score.clamp(-1.0, 1.0).atanh(),
        }
    }

    /// The curve in words, as a record of how labels were made (`buckets.tsv`): two curves with the
    /// same description give the same labels.
    pub fn describe(&self) -> String {
        match self {
            Curve::Atan(scale) => format!("one_pawn {:.6} ceiling {}", scale.one_pawn(), scale.ceiling()),
            Curve::Logistic { s } => format!("logistic s {s:.10}"),
        }
    }
}

/// **The logistic curve at S:** how every network's output is read in the loss, and how every
/// teacher that gives centipawns (PeSTO, our networks) is read into labels, with no fitted scale.
/// For a network the round trip is exact: it was trained to mean `sigma(pawns / S)`.
pub const LOGISTIC: Curve = Curve::Logistic { s: S_PAWNS };

/// `NETWORK.score`: a network's evaluation in pawns as a score, for the engine and the diagnostics.
pub fn score(pawns: f64) -> f64 {
    NETWORK.score(pawns)
}

/// `NETWORK.pawns`, `score`'s inverse.
pub fn pawns_from_score(p: f64) -> f64 {
    NETWORK.pawns(p)
}

/// Whether `cp` is a forced mate or a tablebase result, not merely a large positional advantage --
/// the one threshold `compute_labels_direct` and `length_cache.rs::is_trainable` both need to agree on,
/// defined once here rather than twice. `TB_WIN_SCORE - MAX_PLY` catches both bands at once -- the
/// tablebase band sits strictly below the mate band (`TB_WIN_SCORE`'s own doc comment in
/// `search.rs`), so anything at or beyond the tablebase band's own lower bound is decisive either
/// way. `pub`, not private: `length_cache.rs::is_trainable` calls this directly (its own doc
/// comment covers why decisive plies are trainable now, not excluded), and that caller now sits in
/// a different crate from this module.
pub fn is_decisive(cp: i32) -> bool {
    const DECISIVE_THRESHOLD: i32 = crate::search::TB_WIN_SCORE - crate::search::MAX_PLY as i32;
    cp.abs() >= DECISIVE_THRESHOLD
}

/// The direct blend, Lc0's own (`qMix` in lczero-training's `tfprocess.py`), and the only labelling:
/// every ply's label is its own score blended with the game's result alone, `(1 - lambda) *
/// score(scores[t]) + lambda * outcome`, onto the bounded scale `[-1, 1]` the network's raw output is
/// trained to match. No ply's label depends on another ply's score, so a temperature move's drop
/// reaches no ply before it. AB(lambda), which blended each label with the next one back from the
/// game's end, and its cut at costly non-best moves, were retired on 2026-10-22.
///
/// The blend happens in a bounded scale, the only space where a discrete `-1`/`0`/`1` outcome and a
/// search estimate can be interpolated coherently; `[-1, 1]` rather than `[0, 1]` so the label is a
/// signed, negamax-compatible evaluation. `scores` is one game's raw, White-relative search scores in
/// centipawns, in ply order, converted to pawns here and read through `curve`, the teacher's
/// (`Curve`: Lc0's for Lc0's corpus, `LOGISTIC` for PeSTO and our networks), the one place this
/// module's centipawn/pawns boundary sits. `outcome` keeps the PGN result's `0`/`0.5`/`1`, remapped here by
/// `outcome_label`.
///
/// A decisive score (`is_decisive`, a forced mate or a tablebase result straight from the search, not
/// an estimate) takes exactly `1.0`/`-1.0` by sign, unblended: diluting a certainty with an estimate
/// would only make it noisier. At `lambda = 0` every other label is the ply's own score; at
/// `lambda = 1`, the result. Clamped to `[-1, 1]`, which only absorbs floating-point rounding.
/// White-relative in and out (`side_to_move_relative` turns each to its mover). Panics on an empty
/// `scores`.
pub fn compute_labels_direct(scores: &[i32], outcome: f64, lambda: f64, curve: &Curve) -> Vec<f64> {
    assert!(!scores.is_empty(), "compute_labels_direct called with no scores");
    let outcome = outcome_label(outcome);
    scores
        .iter()
        .map(|&cp| {
            let raw = if is_decisive(cp) {
                if cp > 0 { 1.0 } else { -1.0 }
            } else {
                lambda * outcome + (1.0 - lambda) * curve.score(cp as f64 / 100.0)
            };
            raw.clamp(-1.0, 1.0)
        })
        .collect()
}

/// A game's outcome as a White-relative label: `games.outcome`'s own `0`/`0.5`/`1` mapped to this
/// module's native `-1`/`0`/`1`. What every label blends toward, and the label of the game's final
/// position, which is trained on but never searched.
pub fn outcome_label(outcome: f64) -> f64 {
    2.0 * outcome - 1.0
}

/// Converts one White-relative label (`compute_labels_direct`'s output) into the side-to-move-relative
/// form every network is actually trained against, given the side to move in the position that label
/// describes.
///
/// **Why this exists as its own step.** The labels are blended in a colour-invariant frame: one
/// game-level outcome serves plies of both colours. But every feature the networks read is side-to-move relative --
/// `features.rs`'s `oriented_square` flips the board for the mover, and every identity is "own" or
/// "opponent" rather than White or Black -- and the psq feature set has no side-to-move input at
/// all beyond which perspective comes first, on the grounds that the label already means "expected score for the side to move".
/// Writing a White-relative label against those features asks the network to predict a sign that
/// depends on information it was never given, which caps what it can learn at whatever part of the
/// label survives own/opponent relabeling. Measured, before this existed: `r_squared = 0.073` where
/// material alone should have scored far higher.
///
/// **Why `side_to_move` and not ply parity.** The caller always has the very `CanonicalPosition` whose
/// features are about to be written, and that position states whose move it is. Deriving it instead
/// from a starting colour plus `t % 2` is a second derivation of the same fact, which can drift from
/// the first; taking it from the position makes the label and the features unable to disagree.
///
/// Composing this with `compute_labels_direct` is exactly the blend done in the mover's own frame --
/// `S_t = lambda * outcome_t + (1 - lambda) * score_t`, both the mover's -- since the clamp is
/// symmetric and so commutes with the negation, and the decisive branch takes its sign from the same
/// White-relative score. The test below asserts that equivalence directly rather than leaving it as a
/// claim.
pub fn side_to_move_relative(white_relative: f64, side_to_move: Color) -> f64 {
    if side_to_move == Color::White { white_relative } else { -white_relative }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_to_move_relative_keeps_whites_sign_and_flips_blacks() {
        assert_eq!(side_to_move_relative(0.42, Color::White), 0.42);
        assert_eq!(side_to_move_relative(0.42, Color::Black), -0.42);
        assert_eq!(side_to_move_relative(0.0, Color::Black), 0.0, "a balanced position is balanced for either side");
    }

    #[test]
    fn flipping_each_ply_equals_blending_in_the_mover_s_own_frame() {
        // The equivalence the pipeline rests on: blending in the White frame and flipping each label
        // by its own side to move is the same thing as `S_t = lambda * outcome_t + (1 - lambda) *
        // score_t`, both the mover's. Asserted rather than argued.
        let lambda = 0.7;
        // One decisive ply among the ordinary ones, at an odd (Black-to-move) index, so the equivalence
        // is checked through both branches rather than only the blend.
        let decisive = crate::search::TB_WIN_SCORE - crate::search::MAX_PLY as i32;
        let white_relative_scores = [120, -decisive, 250, -30, 60];
        let outcome = 1.0; // White won
        let flipped: Vec<f64> = compute_labels_direct(&white_relative_scores, outcome, lambda, &Curve::Atan(*NETWORK))
            .iter()
            .enumerate()
            .map(|(t, &label)| {
                // The fixture starts from White to move, so even plies are White's.
                side_to_move_relative(label, if t % 2 == 0 { Color::White } else { Color::Black })
            })
            .collect();

        // The blend in the mover's frame, written out independently of `compute_labels_direct`.
        let sign = |t: usize| if t.is_multiple_of(2) { 1.0 } else { -1.0 };
        for (t, &white_score) in white_relative_scores.iter().enumerate() {
            let stm_score = white_score as f64 * sign(t);
            let stm_outcome = (2.0 * outcome - 1.0) * sign(t);
            let raw = if is_decisive(white_score) {
                if stm_score > 0.0 { 1.0 } else { -1.0 }
            } else {
                lambda * stm_outcome + (1.0 - lambda) * score(stm_score / 100.0)
            };
            let expected = raw.clamp(-1.0, 1.0);
            assert!((flipped[t] - expected).abs() < 1e-12, "ply {t}: flipped {} against {expected}", flipped[t]);
        }
    }

    #[test]
    fn lc0_s_literals_are_the_ones_its_ceiling_and_anchor_derive() {
        // The literals stand in for a derivation `const` cannot run: 4 / sqrt(15) and acos(1 / 31).
        let derived = Scale::half_at_one_pawn(32.0);
        assert!((LC0_FILES.a - derived.a).abs() < 1e-12, "a = {}, derived {}", LC0_FILES.a, derived.a);
        assert!((LC0_FILES.k - derived.k).abs() < 1e-12, "k = {}, derived {}", LC0_FILES.k, derived.k);
        assert!((LC0_FILES.k - (1.0f64 / 31.0).acos()).abs() < 1e-12, "k must also equal acos(1/31)");
        assert_eq!(LC0_FILES.ceiling, 32.0);
    }

    #[test]
    fn the_network_s_scale_is_one_half_at_a_pawn_and_one_at_128() {
        assert!((NETWORK.one_pawn() - 0.5).abs() < 1e-12);
        assert!((NETWORK.score(128.0) - 1.0).abs() < 1e-12);
        assert!(NETWORK.score(127.0) < 1.0, "just below the ceiling is still graded");
        // t = sqrt(63) / 8: a = 8 / sqrt(63), k = acos(1 / 127).
        assert!((NETWORK.a() - 8.0 / 63f64.sqrt()).abs() < 1e-12);
        assert!((NETWORK.k() - (1.0f64 / 127.0).acos()).abs() < 1e-12);
        assert!(NETWORK.k() < std::f64::consts::FRAC_PI_2, "tan(k * q) must stay finite on [-1, 1]");
        assert!(!is_decisive((100.0 * CEILING_PAWNS) as i32), "the ceiling sits far below the tablebase band");
    }

    /// Below 3 pawns, where nearly every position is, the networks' unit barely moved from 32 to 128.
    #[test]
    fn the_ceiling_s_move_changes_little_where_positions_are() {
        for pawns in [0.25, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0] {
            assert!((NETWORK.score(pawns) - LC0_FILES.score(pawns)).abs() < 0.01, "{pawns} pawns");
        }
    }

    /// A fitted scale is the closed form's where both apply, and puts one pawn where it is asked.
    #[test]
    fn a_scale_with_its_one_pawn_fitted() {
        for ceiling in [32.0, 128.0] {
            let fitted = Scale::with_one_pawn(0.5, ceiling);
            let closed = Scale::half_at_one_pawn(ceiling);
            for pawns in [0.1, 1.0, 4.0, 20.0] {
                assert!((fitted.score(pawns) - closed.score(pawns)).abs() < 1e-12, "{ceiling}: {pawns}");
            }
        }
        let teacher = Scale::with_one_pawn(0.62, 128.0);
        assert!((teacher.one_pawn() - 0.62).abs() < 1e-12);
        assert!((teacher.score(128.0) - 1.0).abs() < 1e-12);
        for pawns in [-5.0, 0.3, 2.0, 40.0] {
            assert!((teacher.pawns(teacher.score(pawns)) - pawns).abs() < 1e-9, "{pawns}");
        }
    }

    /// `LC0_FILES` reads Lc0's files back to its Q: today's curve, frozen, off by no more than the
    /// half centipawn the files round to, times the curve's steepest slope, `1 / (a k)` at 0.
    #[test]
    fn lc0_s_files_round_trip_their_q() {
        let tolerance = 0.005 / (LC0_FILES.a() * LC0_FILES.k()) + 1e-9;
        for q in [-0.99, -0.5, 0.0, 0.123, 0.7, 1.0] {
            let pawns = (LC0_FILES.pawns(q) * 100.0).round() / 100.0;
            assert!((LC0_FILES.score(pawns) - q).abs() <= tolerance, "{q}");
        }
    }

    #[test]
    fn score_of_zero_is_exactly_zero() {
        // A balanced position (raw eval 0) must land on exactly 0 -- atan's own fixed point.
        assert_eq!(score(0.0), 0.0);
    }

    #[test]
    fn score_of_one_pawn_is_one_half() {
        // The anchor: one pawn is a 75% expected score, 2 * 0.75 - 1 == 1/2 on the [-1, 1] scale.
        assert!((score(1.0) - 0.5).abs() < 1e-12);
    }

    #[test]
    fn score_reaches_exactly_one_at_the_ceiling_and_stays_there() {
        // Unclamped, atan(pawns / A) / K would keep climbing towards (pi / 2) / K ~ 1.021.
        assert!((score(CEILING_PAWNS) - 1.0).abs() < 1e-12);
        assert_eq!(score(CEILING_PAWNS + 1.0), 1.0);
        assert_eq!(score(-(CEILING_PAWNS + 1.0)), -1.0);
        assert!(score(CEILING_PAWNS - 1.0) < 1.0, "just below the ceiling is still graded");
    }

    #[test]
    fn score_keeps_a_queen_and_a_queen_and_rook_apart() {
        // The reason for the atan scale: tanh left these two 0.004 apart.
        assert!(score(14.0) - score(9.0) > 0.02, "queen {}, queen and rook {}", score(9.0), score(14.0));
    }

    #[test]
    fn score_is_antisymmetric_around_zero() {
        // score(-pawns) == -score(pawns): atan is an odd function and the clamp is symmetric, so a
        // position that looks exactly as good for White as another looks bad must give the two
        // colors exactly opposite signed scores (`score`'s own doc comment covers why negamax needs it).
        for pawns in [0.01, 1.0, 5.0, 320.0] {
            assert!((score(-pawns) + score(pawns)).abs() < 1e-9, "failed for pawns = {pawns}");
        }
    }

    #[test]
    fn score_of_mate_scale_is_exactly_minus_one_or_one() {
        let mate_in_3_pawns = (crate::search::MATE_SCORE - 3) as f64 / 100.0;
        assert_eq!(score(mate_in_3_pawns), 1.0);
        assert_eq!(score(-mate_in_3_pawns), -1.0);
    }

    #[test]
    fn pawns_from_score_is_the_exact_inverse_of_score() {
        for pawns in [-20.0, -5.0, -1.0, -0.01, 0.0, 0.01, 1.0, 5.0, 20.0] {
            assert!((pawns_from_score(score(pawns)) - pawns).abs() < 1e-9, "failed for pawns = {pawns}");
        }
    }

    #[test]
    fn pawns_from_score_of_plus_or_minus_one_is_the_ceiling() {
        assert!((pawns_from_score(1.0) - CEILING_PAWNS).abs() < 1e-9, "got {}", pawns_from_score(1.0));
        assert!((pawns_from_score(-1.0) + CEILING_PAWNS).abs() < 1e-9, "got {}", pawns_from_score(-1.0));
    }

    #[test]
    fn pawns_from_score_of_zero_is_exactly_zero() {
        assert_eq!(pawns_from_score(0.0), 0.0);
    }

    /// At lambda 0 every label is its ply's own score, the last included: the result never reaches it.
    #[test]
    fn the_direct_blend_at_zero_is_each_ply_s_own_score() {
        let labels = compute_labels_direct(&[100, -200, 50], 1.0, 0.0, &Curve::Atan(*NETWORK));
        assert_eq!(labels, vec![score(1.0), score(-2.0), score(0.5)]);
        let teacher = Scale::with_one_pawn(0.62, 128.0);
        assert_eq!(compute_labels_direct(&[100], 1.0, 0.0, &Curve::Atan(teacher)), vec![teacher.score(1.0)], "read through the teacher's scale");
        assert_eq!(compute_labels_direct(&[100], 1.0, 0.0, &LOGISTIC), vec![LOGISTIC.score(1.0)], "read through the logistic");
    }

    #[test]
    fn a_curve_describes_itself_for_the_record() {
        assert_eq!(Curve::Atan(LC0_FILES).describe(), "one_pawn 0.500000 ceiling 32");
        assert!(LOGISTIC.describe().starts_with("logistic s 0.70368"), "{}", LOGISTIC.describe());
        assert_ne!(Curve::Atan(*LC0).describe(), Curve::Atan(LC0_FILES).describe());
    }

    #[test]
    fn the_direct_blend_handles_a_single_ply_game() {
        assert_eq!(compute_labels_direct(&[77], 1.0, 0.5, &Curve::Atan(*NETWORK)), vec![0.5 * 1.0 + 0.5 * score(0.77)]);
    }

    /// A forced-mate or tablebase-scale score is exactly 1.0/-1.0 by sign, never diluted by lambda,
    /// even at 0.9 against a result that disagrees with it entirely (a draw).
    #[test]
    fn the_direct_blend_gives_a_decisive_score_a_hard_unblended_label() {
        let decisive = crate::search::TB_WIN_SCORE - crate::search::MAX_PLY as i32;
        assert_eq!(compute_labels_direct(&[decisive, -decisive], 0.5, 0.9, &Curve::Atan(*NETWORK)), vec![1.0, -1.0]);
    }

    /// Every ply blends its own score with the result, the scores after it ignored: changing ply 2's
    /// score changes ply 2's label alone.
    #[test]
    fn the_direct_blend_takes_each_ply_s_score_and_the_result_alone() {
        let labels = compute_labels_direct(&[200, -100, 50], 1.0, 0.25, &Curve::Atan(*NETWORK));
        assert_eq!(labels[0], 0.25 * 1.0 + 0.75 * score(2.0));
        assert_eq!(labels[1], 0.25 * 1.0 + 0.75 * score(-1.0));
        assert_eq!(labels[2], 0.25 * 1.0 + 0.75 * score(0.5));
        let changed = compute_labels_direct(&[200, -100, 900], 1.0, 0.25, &Curve::Atan(*NETWORK));
        assert_eq!(changed[..2], labels[..2], "a later score reaches no earlier ply");
        assert_eq!(compute_labels_direct(&[200, -100, 50], 0.0, 1.0, &Curve::Atan(*NETWORK)), vec![-1.0; 3], "at 1 every label is the result");
    }

    /// A decisive ply takes its own hard label, which no other ply takes from it.
    #[test]
    fn a_direct_decisive_ply_keeps_its_hard_label_alone() {
        let decisive = crate::search::TB_WIN_SCORE - crate::search::MAX_PLY as i32;
        let labels = compute_labels_direct(&[50, decisive], 0.5, 0.5, &Curve::Atan(*NETWORK));
        assert_eq!(labels[1], 1.0);
        assert_eq!(labels[0], 0.5 * 0.0 + 0.5 * score(0.5), "a draw's result, not the mate after it");
    }

    #[test]
    fn s_is_lc0_s_a_times_b_over_two() {
        assert!((S_PAWNS - 0.90 * 1.5637541897 / 2.0).abs() < 1e-15);
        assert!((S_PAWNS * 100.0 - 70.37).abs() < 0.01, "S = {} centipawns", S_PAWNS * 100.0);
    }

    /// The logistic is `2 sigma(pawns / S) - 1`, written as a tanh.
    #[test]
    fn the_logistic_is_twice_a_sigmoid_less_one() {
        let sigma = |x: f64| 1.0 / (1.0 + (-x).exp());
        for pawns in [-5.0, -1.0, 0.0, 0.3, 1.0, 4.0] {
            assert!((LOGISTIC.score(pawns) - (2.0 * sigma(pawns / S_PAWNS) - 1.0)).abs() < 1e-12, "{pawns}");
        }
    }

    /// The convention S rests on: at equality the logistic and Lc0's curve have the same slope.
    #[test]
    fn the_logistic_matches_lc0_s_slope_at_equality() {
        let h = 1e-6;
        let slope = |curve: &dyn Fn(f64) -> f64| (curve(h) - curve(-h)) / (2.0 * h);
        let logistic = slope(&|p| LOGISTIC.score(p));
        let lc0 = slope(&|p| LC0.score(p));
        assert!((logistic - lc0).abs() < 1e-6, "logistic {logistic}, Lc0 {lc0}");
        assert!((lc0 - 1.0 / (LC0_A_PAWNS * LC0_B)).abs() < 1e-6);
    }

    #[test]
    fn the_logistic_is_odd_and_inverted_by_its_pawns() {
        for pawns in [0.01, 0.7, 1.0, 3.0, 10.0] {
            assert!((LOGISTIC.score(-pawns) + LOGISTIC.score(pawns)).abs() < 1e-12, "{pawns}");
            assert!((LOGISTIC.pawns(LOGISTIC.score(pawns)) - pawns).abs() < 1e-9, "{pawns}");
        }
        assert_eq!(LOGISTIC.score(0.0), 0.0);
        assert!(LOGISTIC.score(1000.0) <= 1.0 && LOGISTIC.pawns(1.0).is_infinite());
    }

    /// Lc0's curve: 100 centipawns at Q about 0.536, Q = 1 at `A tan(B)`, about 127.8 pawns.
    #[test]
    fn lc0_s_curve_is_lc0_s_formula() {
        assert!((LC0.score(1.0) - (1.0f64 / 0.90).atan() / 1.5637541897).abs() < 1e-12);
        assert!((LC0.score(1.0) - 0.536).abs() < 0.001, "{}", LC0.score(1.0));
        assert!((LC0.pawns(1.0) - 0.90 * 1.5637541897f64.tan()).abs() < 1e-9);
        assert!((LC0.pawns(1.0) - 127.8).abs() < 0.1, "{}", LC0.pawns(1.0));
        assert_eq!(LC0.score(200.0), 1.0, "clamped past q = 1");
        assert!(!is_decisive((100.0 * LC0.pawns(1.0)) as i32), "Lc0's q = 1 sits far below the tablebase band");
    }

    /// The rewrite of Lc0's files: a stored score decoded through `LC0_FILES` and re-encoded through
    /// `LC0`, both at 2 decimals, gives Q back within the two roundings, each half a centipawn times its
    /// curve's steepest slope, `1 / (a k)` at 0.
    #[test]
    fn lc0_s_files_rewritten_keep_their_q() {
        let round = |pawns: f64| (pawns * 100.0).round() / 100.0;
        let tolerance = 0.005 / (LC0_FILES.a() * LC0_FILES.k()) + 0.005 / (LC0.a() * LC0.k()) + 1e-9;
        for q in [-0.99, -0.5, 0.0, 0.123, 0.7, 0.999, 1.0] {
            let stored = round(LC0_FILES.pawns(q));
            let rewritten = round(LC0.pawns(LC0_FILES.score(stored)));
            assert!((LC0.score(rewritten) - q).abs() <= tolerance, "{q}");
        }
        assert!(tolerance < 0.007);
    }

    #[test]
    fn is_decisive_matches_the_documented_threshold_and_is_symmetric() {
        let threshold = crate::search::TB_WIN_SCORE - crate::search::MAX_PLY as i32;
        assert!(!is_decisive(threshold - 1), "one below the threshold -- not decisive");
        assert!(is_decisive(threshold), "exactly at the threshold -- decisive");
        assert!(is_decisive(-threshold), "same threshold, losing side");
        assert!(!is_decisive(2000), "an ordinary large material edge -- not decisive");
    }
}
