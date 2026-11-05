//! `net.yaml`'s own schema, parsed via `serde`/`serde_yaml` rather than a hand-rolled parser --
//! unlike FEN (this project's own domain-specific format, CLAUDE.md rule 1's exact reason to
//! hand-write it), YAML is a generic, well-established format with a mature library already
//! available.
//!
//! The network's sections of a forge config, `configs/chess/<name>.yaml` (`preprocessing_config::net_config_path`),
//! one file holding the forge's own keys beside them (the smithy's `load_config`). One shared
//! schema for every architecture this project trains (NNUE's three kinds and the transformer
//! alike), not two
//! disjoint schemas picked by argument shape: `architecture.kind` is the one tag a reader (or
//! `main.rs`) checks to know both which concrete network this file describes and which of
//! `Architecture`'s own further fields (`num_layers`/`num_heads`/`feedforward_dim`/`dropout`) are
//! even present. Fields genuinely shared by every kind (`embedding_dim`, `value_head`, and every
//! `training` field) sit at the common level instead of being duplicated once per kind. See
//! `examples/net_transformer.yaml`/`net_psq.yaml` for worked examples.

use serde::Deserialize;

use super::stack_head::HeadShape;

/// One network's settings, grouped the way its sections are: `architecture` and `training` are
/// everything training reads (split between the model's own shape and how it's optimized).
#[derive(Debug, Deserialize)]
pub struct NetConfig {
    pub architecture: Architecture,
    pub training: Training,
}

/// The model's own shape. `embedding_dim` and `value_head` belong to every kind: each ends in the
/// same value-head MLP shape (`value_head.rs`) fed from some accumulator or embedding of this width.
/// `load` requires both; read them through `embedding_dim()` and `value_head_hidden_dim()`. `kind` decides which concrete network
/// this describes, and, for `Transformer`, which further fields exist alongside these two.
#[derive(Debug, Deserialize)]
pub struct Architecture {
    #[serde(default)]
    pub(super) embedding_dim: Option<i64>,
    #[serde(default)]
    pub(super) value_head: Option<ValueHeadConfig>,
    #[serde(flatten)]
    pub kind: ArchitectureKind,
}

impl Architecture {
    /// The accumulator or embedding width. `load` has checked it is present.
    pub fn embedding_dim(&self) -> i64 {
        self.embedding_dim.expect("every kind has an embedding_dim, checked by load")
    }

    /// The value head's hidden widths, in order. `load` has checked it is present.
    pub fn value_head_hidden_dim(&self) -> &[i64] {
        &self.value_head.as_ref().expect("every kind has a value_head, checked by load").hidden_dim
    }

    /// Whether each perspective's clipped transform is split in two halves multiplied element-wise
    /// before the head (`PsqPairwise`, `PsqThreatsPairwise`, and the stacks, which build on them).
    pub fn pairwise(&self) -> bool {
        matches!(
            self.kind,
            ArchitectureKind::PsqPairwise
                | ArchitectureKind::PsqThreatsPairwise
                | ArchitectureKind::PsqStack
                | ArchitectureKind::PsqThreatsStack
                | ArchitectureKind::PsqThreatsPairwiseMaterial
                | ArchitectureKind::PsqThreatsPairwiseHidden
                | ArchitectureKind::HalfkThreatsPairwiseHidden
        )
    }

    /// Whether each perspective's clipped transform is squared lane by lane before the head
    /// (`PsqThreatsScrelu`, `PsqThreatsScreluMaterial`, `PsqThreatsScreluPhase`,
    /// `PsqThreatsScreluCount`, `PsqThreatsScreluTaper`, `PsqThreatsScreluKings`,
    /// `PsqThreatsScreluHidden`).
    pub fn screlu(&self) -> bool {
        matches!(
            self.kind,
            ArchitectureKind::PsqThreatsScrelu
                | ArchitectureKind::HalfkThreatsScrelu
                | ArchitectureKind::HalfkThreatsScreluHidden
                | ArchitectureKind::PsqThreatsScreluMaterial
                | ArchitectureKind::PsqThreatsScreluPhase
                | ArchitectureKind::PsqThreatsScreluCount
                | ArchitectureKind::PsqThreatsScreluTaper
                | ArchitectureKind::PsqThreatsScreluKings
                | ArchitectureKind::PsqThreatsScreluHidden
        )
    }

    /// Whether the head is one hidden layer over the squares or the products beside the output's own
    /// reading of them (`PsqThreatsScreluHidden`, `PsqThreatsPairwiseHidden`).
    pub fn hidden_beside(&self) -> bool {
        matches!(self.kind, ArchitectureKind::PsqThreatsScreluHidden | ArchitectureKind::PsqThreatsPairwiseHidden)
    }

    /// Whether the head is the stack's two king stages over the squares into one plain output
    /// (`PsqThreatsScreluKings`).
    pub fn kings(&self) -> bool {
        matches!(self.kind, ArchitectureKind::PsqThreatsScreluKings)
    }

    /// Whether the head's layers are bucketed by our king, their king and the material (`PsqStack`,
    /// `PsqThreatsStack`).
    pub fn stack(&self) -> bool {
        matches!(self.kind, ArchitectureKind::PsqStack | ArchitectureKind::PsqThreatsStack)
    }

    /// Whether the head is one bucketed output, by the material (`PsqThreatsPairwiseMaterial`,
    /// `PsqThreatsScreluMaterial`), by the game phase (`PsqThreatsScreluPhase`) or by the piece count
    /// (`PsqThreatsScreluCount`), or two sets blended by the phase (`PsqThreatsScreluTaper`): the
    /// engine's material sets, whichever the key.
    pub fn material(&self) -> bool {
        matches!(
            self.kind,
            ArchitectureKind::PsqThreatsPairwiseMaterial
                | ArchitectureKind::PsqThreatsScreluMaterial
                | ArchitectureKind::PsqThreatsScreluPhase
                | ArchitectureKind::PsqThreatsScreluCount
                | ArchitectureKind::PsqThreatsScreluTaper
        )
    }

    /// The head's shape, as `nnue_network::Network::with_head` takes it.
    pub fn head_shape(&self) -> HeadShape {
        if self.stack() {
            HeadShape::Stack
        } else if self.kings() {
            HeadShape::Kings
        } else if self.hidden_beside() {
            HeadShape::Hidden
        } else if matches!(self.kind, ArchitectureKind::PsqThreatsScreluPhase) {
            HeadShape::Phase
        } else if matches!(self.kind, ArchitectureKind::PsqThreatsScreluCount) {
            HeadShape::Count
        } else if matches!(self.kind, ArchitectureKind::PsqThreatsScreluTaper) {
            HeadShape::Taper
        } else if self.material() {
            HeadShape::Material
        } else {
            HeadShape::Plain
        }
    }
}

/// The value head MLP's own shape (`value_head::ValueHead`) -- `hidden_dim` is one width per
/// hidden layer, in order; its length is the depth, so there is no separate `depth` field to keep
/// in sync with it, and layers need not share one width.
#[derive(Debug, Deserialize)]
pub struct ValueHeadConfig {
    pub hidden_dim: Vec<i64>,
}

/// `architecture.kind`'s own four values -- an internally-tagged enum (`#[serde(tag = "kind")]`),
/// flattened into `Architecture` above so `kind` reads as a plain sibling field in `net.yaml`
/// rather than a nested wrapper. The NNUE kinds carry no further fields of their own:
/// unlike the transformer, none of NNUE's architectures expose any shape beyond
/// `embedding_dim`/`value_head` -- every other width (the feature transformer's own input width) is
/// fixed by the feature set itself, in code, not configurable. `Transformer` alone carries the
/// fields specific to that architecture (the transformer design's "Shared transformer encoder").
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ArchitectureKind {
    Psq,
    /// Psq's rows plus each perspective's own threats (`temper_chess::features::psq_threats`), with
    /// psq's `embedding_dim` and `value_head`.
    PsqThreats,
    /// Psq's rows and transform, each perspective's clipped output split in two halves multiplied
    /// element-wise, so the head reads `embedding_dim` values instead of `2 * embedding_dim`. Same
    /// data as psq (`FeatureSet::Psq`).
    PsqPairwise,
    /// `PsqThreats`' rows and transform with `PsqPairwise`'s products: each perspective's clipped
    /// output, threats included, split in two halves multiplied element-wise. Same data as
    /// psq_threats (`FeatureSet::PsqThreats`).
    PsqThreatsPairwise,
    /// `PsqPairwise`'s products into a head of two hidden layers and the output, each layer one of
    /// several weight sets: the first by our king's square (64), the second by their king's (64), the
    /// output by the material (78), `temper_chess::stack_buckets`. `value_head.hidden_dim` is the two hidden
    /// widths. Trained factorised, each set a shared part plus its own delta (`stack_head.rs`).
    PsqStack,
    /// `PsqThreatsPairwise`'s products into `PsqStack`'s head. Same data as psq_threats
    /// (`FeatureSet::PsqThreats`), whose batches carry the buckets too.
    PsqThreatsStack,
    /// `PsqThreatsPairwise` with its one output replaced by 78 weight sets, one per material value
    /// (`temper_chess::stack_buckets::material_bucket`), no king buckets and no hidden layer
    /// (`value_head.hidden_dim` empty). Trained factorised, as `PsqStack`'s output stage
    /// (`stack_head::StackHead::material`). Same data as psq_threats.
    PsqThreatsPairwiseMaterial,
    /// `PsqThreats` with each perspective's clipped output squared lane by lane, `clamp(x, 0, 1)^2`,
    /// the squared clipped ReLU of single-layer networks (screlu.md); the head reads
    /// `2 * embedding_dim` squares, with no hidden layer (`value_head.hidden_dim` empty). Same data
    /// as psq_threats.
    PsqThreatsScrelu,
    /// `PsqThreatsScrelu` with psq's rows replaced by halfk's (`temper_chess::features::halfk`): Stockfish's
    /// HalfKAv2_hm, 32 king buckets, the threats mirrored with the king. Its piece rows trained
    /// factorised, each a bucket's own part plus a factor shared by every bucket, summed at export.
    /// Batches of its own (`FeatureSet::HalfKav2Hm`).
    HalfkThreatsScrelu,
    /// `HalfkThreatsScrelu` with its one output replaced by two hidden layers, each clipped to
    /// `[0, 1]`, then the output: `value_head.hidden_dim` the two widths, no skip, trained end to end.
    HalfkThreatsScreluHidden,
    /// `HalfkThreatsScreluHidden` with each perspective's clipped lanes split in two halves multiplied
    /// in place of the squares, as `PsqThreatsPairwise`'s: the head reads `embedding_dim` products.
    HalfkThreatsPairwiseHidden,
    /// `PsqThreatsScrelu` with its one output replaced by `PsqThreatsPairwiseMaterial`'s 78 weight
    /// sets, each reading the `2 * embedding_dim` squares (psq-threats-screlu-material.md). No hidden
    /// layer; trained factorised. Same data as psq_threats.
    PsqThreatsScreluMaterial,
    /// `PsqThreatsScreluMaterial` with 25 weight sets by the game phase instead of the material
    /// (knight and bishop 1, rook 2, queen 4, capped at 24, `temper_chess::stack_buckets::phase_bucket`):
    /// fewer buckets, each with more data. The phase is the fourth of the buckets the batches carry.
    PsqThreatsScreluPhase,
    /// `PsqThreatsScreluMaterial` with 8 weight sets by the piece count instead, Stockfish's key:
    /// every piece, kings included, `(count - 1) / 4` (`temper_chess::stack_buckets::count_bucket`). The
    /// fewest buckets of the three. The count is the fifth of the buckets the batches carry.
    PsqThreatsScreluCount,
    /// `PsqThreatsScrelu` with two output sets, a middlegame and an endgame one, both read at every
    /// position and blended by the game phase as PeSTO blends its tables (`stack_head::taper`), the
    /// phase being the fourth of the buckets the batches carry. Not factorised: both sets see every
    /// position (psq-threats-screlu-taper.md).
    PsqThreatsScreluTaper,
    /// `PsqThreatsScrelu` with a king-sensitive hidden layer: each side's squares through a stage
    /// picked by its own king's square (64 each), factorised, then one plain output over those
    /// stages' values and every square (`stack_head::StackHead::kings`). `value_head.hidden_dim` is
    /// one width, a side's. Same data as psq_threats, whose batches carry both kings' buckets.
    PsqThreatsScreluKings,
    /// `PsqThreatsScrelu` with one hidden layer beside its output: every square through the hidden
    /// layer, its input detached in training, then one output over its values clipped and squared and
    /// every square. `value_head.hidden_dim` is its one width. Kings' layers without the king.
    PsqThreatsScreluHidden,
    /// `PsqThreatsScreluHidden`'s head over `PsqThreatsPairwise`'s products instead of the squares:
    /// every product through the hidden layer, detached, then one output over its values clipped and
    /// squared and every product (psq-threats-pairwise-hidden.md). Same data as psq_threats.
    PsqThreatsPairwiseHidden,
    Transformer { num_layers: i64, num_heads: i64, feedforward_dim: i64, dropout: f64 },
}

/// Everything about *how* the model trains, as opposed to its shape. `attention` is the one field
/// here that isn't shared by every kind -- `Some` only makes sense when `architecture.kind` is
/// `Transformer` (design point A: full and kings-only select which of two architectures sharing
/// one implementation gets trained, never a weight-shape difference), so `load` below asserts the
/// two agree rather than leaving a `None`/`Some` mismatch to surface later as a confusing panic
/// somewhere in `main.rs`.
///
/// **The training fields are read-only once a run has started.** The NNUE track derives where it
/// reads from and how far into the current round it is from `optimizer.opt`'s persisted step count
/// alone, so changing `training_batch_size` or `steps_per_round` between two invocations of one run
/// would reinterpret every step already taken. `run_config.rs` freezes them beside the weights on the
/// first invocation and refuses a later one that disagrees; the fix is to restore the value or start
/// the run over, never to edit around it.
#[derive(Debug, Deserialize)]
pub struct Training {
    /// The rates one run steps down through, in order: training starts at the first, moves abruptly
    /// to the next each time `patience_rounds` rounds pass without a new best test loss, and stops when
    /// the last one runs out of patience (`stages.rs`). Strictly decreasing. A scalar in the file is a
    /// one-stage run.
    #[serde(deserialize_with = "one_or_many_rates")]
    pub learning_rate: Vec<f64>,
    /// Rows per gradient step. Together with `steps_per_round` this fixes one evaluation round's own
    /// train volume (`training_batch_size * steps_per_round`).
    ///
    /// The test set's size is not configurable and does not appear in this file: a frozen test set
    /// holds exactly `TEST_POSITIONS_CAP` (4Mi) positions, and a dataset's test half about as many.
    /// So a round's test pass costs the same whatever the corpus size, rather than growing with it
    /// past the training it measures.
    pub training_batch_size: i64,
    /// Rows per batch during a no-grad pass (an evaluation round's own test pass). Deliberately
    /// separate from, and typically much
    /// larger than, `training_batch_size`: nothing here holds a backward pass's worth of
    /// intermediate activations, so a bigger batch costs only memory for the rows themselves and
    /// spends less time per row.
    pub test_batch_size: i64,
    /// Gradient steps in one evaluation round -- the round being the unit both tracks measure in: one
    /// full no-grad pass over the test set follows every one of them, and `warmup_rounds` and
    /// `patience_rounds` count in it too.
    pub steps_per_round: u64,
    pub attention: Option<Attention>,
    /// Rounds over which the rate ramps linearly up from a fresh optimizer. A zero-moment AdamW's
    /// first updates are close to `lr * sign(grad)` on every parameter, a jolt to the champion's
    /// trained weights at the start of every promoted generation; the ramp absorbs it until the second
    /// moment has seen about `1 / (1 - beta2)` = 1000 gradients. `0` for none. Every kind; `load`
    /// requires it.
    pub warmup_rounds: Option<u64>,
    /// Rounds a run may go without a new best test loss at one rate before moving to the next, or
    /// stopping at the last. Counted from the latest of the last new best, the last switch of rate and
    /// the end of warmup. At least 1. Every kind; `load` requires it.
    pub patience_rounds: Option<u64>,
    /// How many positions a streamed transformer's shuffle buffers hold in all, one buffer per piece
    /// count. A transformer only: an NNUE trains on its training set fully shuffled, scattered into
    /// buckets and shuffled again every epoch, with no buffer to size, so `load` refuses the key on
    /// one rather than silently ignoring a value meant to take effect.
    pub shuffle_buffer: Option<u64>,
}

/// `net.yaml`'s `training.attention` field -- `full` attends every token pair, `kings-only`
/// attends only pairs touching a king. Same parameter count either way (design point A) -- this
/// only ever changes which mask a forward pass is given, never a weight shape. `rename_all =
/// "kebab-case"` is what makes `serde_yaml` accept the file's own lowercase, hyphenated spellings
/// (`full`, `kings-only`) directly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Attention {
    Full,
    KingsOnly,
}

/// Accepts `learning_rate: 3.0e-4` and `learning_rate: [3.0e-3, 1.0e-3]` alike, widening the scalar
/// into a one-element list. An untagged enum rather than a custom visitor: `serde` tries the variants
/// in order, and a float and a sequence of floats cannot be confused for one another.
fn one_or_many_rates<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<f64>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(f64),
        Many(Vec<f64>),
    }
    return Ok(match OneOrMany::deserialize(deserializer)? {
        OneOrMany::One(rate) => vec![rate],
        OneOrMany::Many(rates) => rates,
    });
}

/// Reads and parses a real, in-use `net.yaml` at `path`. Panics on anything wrong with it (`read`'s
/// errors), for tests that cannot go on without it.
#[cfg(test)]
pub fn load(path: &str) -> NetConfig {
    read(path).unwrap_or_else(|message| panic!("{message}"))
}

/// `load`'s checks as an error: a missing file, invalid YAML, a missing/mistyped field, an unknown
/// kind, or `training.attention` disagreeing with `architecture.kind`. The smithy's server reads every
/// config in `configs/chess/`, and one written for a newer binary must not bring it down.
pub fn read(path: &str) -> Result<NetConfig, String> {
    macro_rules! ensure {
        ($condition:expr, $($message:tt)+) => {{
            let holds: bool = $condition;
            if !holds {
                return Err(format!($($message)+));
            }
        }};
    }
    let contents = std::fs::read_to_string(path).map_err(|error| format!("failed to read {path}: {error}"))?;
    let config: NetConfig = serde_yaml::from_str(&contents).map_err(|error| format!("failed to parse {path}: {error}"))?;
    ensure!(config.architecture.embedding_dim.is_some(), "{path}: architecture.embedding_dim is missing");
    ensure!(config.architecture.value_head.is_some(), "{path}: architecture.value_head is missing");
    if config.architecture.pairwise() {
        ensure!(
            config.architecture.embedding_dim() % 2 == 0,
            "{path}: architecture.kind splits the transform in two halves, but embedding_dim {} is odd",
            config.architecture.embedding_dim()
        );
    }
    if config.architecture.stack() {
        ensure!(
            config.architecture.value_head_hidden_dim().len() == 2,
            "{path}: architecture.kind is psq_stack, whose head has two hidden layers (by our king, then by their king), \
             but value_head.hidden_dim has {} widths",
            config.architecture.value_head_hidden_dim().len()
        );
    }
    if config.architecture.material() {
        ensure!(
            config.architecture.value_head_hidden_dim().is_empty(),
            "{path}: architecture.kind has material sets, which read the products or the squares directly, \
             but value_head.hidden_dim has {} widths",
            config.architecture.value_head_hidden_dim().len()
        );
    }
    if config.architecture.kings() {
        ensure!(
            config.architecture.value_head_hidden_dim().len() == 1,
            "{path}: architecture.kind is psq_threats_screlu_kings, whose head has one hidden width (a side's, each \
             side by its own king), but value_head.hidden_dim has {} widths",
            config.architecture.value_head_hidden_dim().len()
        );
    } else if config.architecture.hidden_beside() {
        ensure!(
            config.architecture.value_head_hidden_dim().len() == 1,
            "{path}: architecture.kind is psq_threats_screlu_hidden or psq_threats_pairwise_hidden, whose head has \
             one hidden layer, but value_head.hidden_dim has {} widths",
            config.architecture.value_head_hidden_dim().len()
        );
    } else if matches!(config.architecture.kind, ArchitectureKind::HalfkThreatsScreluHidden | ArchitectureKind::HalfkThreatsPairwiseHidden) {
        ensure!(
            config.architecture.value_head_hidden_dim().len() == 2,
            "{path}: architecture.kind is halfk_threats_screlu_hidden or halfk_threats_pairwise_hidden, whose head has two \
             hidden layers, but value_head.hidden_dim has {} widths",
            config.architecture.value_head_hidden_dim().len()
        );
    } else if config.architecture.screlu() {
        ensure!(
            config.architecture.value_head_hidden_dim().is_empty(),
            "{path}: architecture.kind is a screlu one, whose output reads the squares directly, but \
             value_head.hidden_dim has {} widths",
            config.architecture.value_head_hidden_dim().len()
        );
    }
    match (&config.architecture.kind, config.training.attention) {
        (ArchitectureKind::Transformer { .. }, Some(_)) => {}
        (ArchitectureKind::Transformer { .. }, None) => {
            return Err(format!("{path}: architecture.kind is transformer, but training.attention is missing"))
        }
        (_, None) => {}
        (_, Some(_)) => {
            return Err(format!("{path}: training.attention is only meaningful when architecture.kind is transformer"))
        }
    }
    // `shuffle_buffer`: a transformer has one when it streams (`labels: self` or `lc0`, its buffers
    // sized by it); the smithy refuses a transformer without one when it trains. An NNUE has none: its training set
    // is fully shuffled through buckets.
    match (&config.architecture.kind, config.training.shuffle_buffer) {
        (ArchitectureKind::Transformer { .. }, Some(value)) => {
            ensure!(value >= 1, "{path}: training.shuffle_buffer must be at least 1, got {value}")
        }
        (ArchitectureKind::Transformer { .. }, None) => {}
        (_, Some(_)) => {
            return Err(format!(
                "{path}: training.shuffle_buffer is read by no NNUE any more: an NNUE trains on its training set fully \
                 shuffled through buckets; delete it"
            ))
        }
        (_, None) => {}
    }
    // Every kind trains its lambdas in lockstep through the same schedule (`stages.rs`).
    for (name, value, least) in
        [("warmup_rounds", config.training.warmup_rounds, 0), ("patience_rounds", config.training.patience_rounds, 1)]
    {
        match value {
            Some(value) => ensure!(value >= least, "{path}: training.{name} must be at least {least}, got {value}"),
            None => return Err(format!("{path}: training.{name} is missing")),
        }
    }

    let rates = &config.training.learning_rate;
    ensure!(!rates.is_empty(), "{path}: training.learning_rate is an empty list, so there is no rate to train at");
    for rate in rates {
        ensure!(
            rate.is_finite() && *rate > 0.0,
            "{path}: training.learning_rate holds {rate}, which is not a finite, strictly positive rate. Zero would \
             train nothing and a negative rate would ascend the gradient."
        );
    }
    for pair in rates.windows(2) {
        ensure!(
            pair[1] < pair[0],
            "{path}: training.learning_rate must be strictly decreasing, the order training steps down through it, \
             but {:e} is followed by {:e}",
            pair[0],
            pair[1]
        );
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_parses_the_committed_transformer_example() {
        temper_chess::pipeline::repository_root::enter_for_tests();
        let config = load("examples/net_transformer.yaml");

        assert_eq!(config.architecture.embedding_dim(), 256);
        assert_eq!(config.architecture.value_head_hidden_dim(), &[128, 64]);
        match config.architecture.kind {
            ArchitectureKind::Transformer { num_layers, num_heads, feedforward_dim, dropout } => {
                assert_eq!(num_layers, 6);
                assert_eq!(num_heads, 8);
                assert_eq!(feedforward_dim, 1024);
                assert_eq!(dropout, 0.1);
            }
            _ => panic!("expected architecture.kind to be Transformer"),
        }
        assert_eq!(config.training.attention, Some(Attention::Full));
        assert_eq!(config.training.training_batch_size, 1024);
        assert_eq!(config.training.test_batch_size, 16384);
        assert_eq!(config.training.steps_per_round, 8192);
        assert_eq!(config.training.learning_rate, vec![1.0e-4], "a scalar in the file widens to one stage");
        assert_eq!(config.training.shuffle_buffer, None, "the transformer reads bucketed files, with no buffer to size");
        assert_eq!(config.training.warmup_rounds, Some(1));
        assert_eq!(config.training.patience_rounds, Some(5));
    }

    #[test]
    fn load_parses_the_committed_psq_example() {
        temper_chess::pipeline::repository_root::enter_for_tests();
        let config = load("examples/net_psq.yaml");

        assert_eq!(config.architecture.embedding_dim(), 256);
        assert!(config.architecture.value_head_hidden_dim().is_empty());
        assert!(matches!(config.architecture.kind, ArchitectureKind::Psq));
        assert_eq!(config.training.attention, None);
        assert_eq!(config.training.shuffle_buffer, None);
        assert_eq!(config.training.learning_rate, vec![1.0e-3, 3.0e-4, 1.0e-4]);
        assert_eq!(config.training.warmup_rounds, Some(2));
        assert_eq!(config.training.patience_rounds, Some(8));
    }

    #[test]
    #[should_panic(expected = "attention is missing")]
    fn load_rejects_a_transformer_kind_with_no_attention_field() {
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("smithy-net-config-test-{pid}-no-attention.yaml"));
        std::fs::write(
            &path,
            "architecture:\n  kind: transformer\n  embedding_dim: 8\n  \
             value_head:\n    hidden_dim: [4]\n  num_layers: 1\n  num_heads: 1\n  feedforward_dim: 8\n  \
             dropout: 0.0\ntraining:\n  learning_rate: 0.1\n  training_batch_size: 1\n  test_batch_size: 1\n  \
             steps_per_round: 1\n  warmup_rounds: 0\n  patience_rounds: 1\n",
        )
        .expect("failed to write the fixture file");

        let _ = load(path.to_str().unwrap());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    #[should_panic(expected = "only meaningful when architecture.kind is transformer")]
    fn load_rejects_a_non_transformer_kind_with_an_attention_field() {
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("smithy-net-config-test-{pid}-stray-attention.yaml"));
        std::fs::write(
            &path,
            "architecture:\n  kind: psq\n  embedding_dim: 8\n  \
             value_head:\n    hidden_dim: [4]\ntraining:\n  attention: full\n  learning_rate: 0.1\n  \
             training_batch_size: 1\n  test_batch_size: 1\n  steps_per_round: 1\n",
        )
        .expect("failed to write the fixture file");

        let _ = load(path.to_str().unwrap());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    #[should_panic(expected = "training.shuffle_buffer is read by no NNUE any more")]
    fn load_rejects_an_nnue_kind_with_a_shuffle_buffer_field() {
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("smithy-net-config-test-{pid}-nnue-shuffle-buffer.yaml"));
        std::fs::write(
            &path,
            "architecture:\n  kind: psq\n  embedding_dim: 8\n  \
             value_head:\n    hidden_dim: [4]\ntraining:\n  learning_rate: 0.1\n  training_batch_size: 1\n  \
             test_batch_size: 1\n  steps_per_round: 1\n  shuffle_buffer: 16\n  warmup_rounds: 0\n  patience_rounds: 1\n",
        )
        .expect("failed to write the fixture file");

        let _ = load(path.to_str().unwrap());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    #[should_panic(expected = "training.patience_rounds is missing")]
    fn load_rejects_an_nnue_kind_with_no_patience_rounds_field() {
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("smithy-net-config-test-{pid}-no-patience-rounds.yaml"));
        std::fs::write(
            &path,
            "architecture:\n  kind: psq\n  embedding_dim: 8\n  \
             value_head:\n    hidden_dim: [4]\ntraining:\n  learning_rate: 0.1\n  training_batch_size: 1\n  \
             test_batch_size: 1\n  steps_per_round: 1\n  warmup_rounds: 0\n",
        )
        .expect("failed to write the fixture file");

        let _ = load(path.to_str().unwrap());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn load_takes_a_streamed_transformer_s_shuffle_buffer() {
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("smithy-net-config-test-{pid}-streamed-transformer.yaml"));
        std::fs::write(
            &path,
            "architecture:\n  kind: transformer\n  \
             embedding_dim: 8\n  value_head:\n    hidden_dim: [4]\n  num_layers: 1\n  num_heads: 1\n  \
             feedforward_dim: 8\n  dropout: 0.0\ntraining:\n  attention: full\n  shuffle_buffer: 16\n  \
             learning_rate: 0.1\n  training_batch_size: 1\n  test_batch_size: 1\n  steps_per_round: 1\n  \
             warmup_rounds: 0\n  patience_rounds: 1\n",
        )
        .expect("failed to write the fixture file");

        assert_eq!(load(path.to_str().unwrap()).training.shuffle_buffer, Some(16));
        std::fs::remove_file(&path).ok();
    }

    /// A transformer fixture starting with `head` verbatim (empty for none), and whose `training`
    /// section ends with `schedule` verbatim.
    fn transformer_fixture_with(name: &str, head: &str, schedule: &str) -> std::path::PathBuf {
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("smithy-net-config-test-{pid}-{name}.yaml"));
        std::fs::write(
            &path,
            format!(
                "{head}architecture:\n  kind: transformer\n  embedding_dim: 8\n  value_head:\n    hidden_dim: [4]\n  \
                 num_layers: 1\n  num_heads: 1\n  feedforward_dim: 8\n  dropout: 0.1\ntraining:\n  attention: full\n  \
                 learning_rate: [0.1, 0.01]\n  training_batch_size: 1\n  test_batch_size: 1\n  steps_per_round: 1\n\
                 {schedule}"
            ),
        )
        .expect("failed to write the fixture file");
        path
    }

    /// `transformer_fixture_with` the schedule every kind needs.
    fn transformer_fixture(name: &str, head: &str) -> std::path::PathBuf {
        transformer_fixture_with(name, head, "  warmup_rounds: 0\n  patience_rounds: 1\n")
    }

    #[test]
    fn load_accepts_a_transformer_stepping_down_rates_with_dropout() {
        let path = transformer_fixture("lockstep", "");
        let config = load(path.to_str().unwrap());
        assert_eq!(config.training.learning_rate, vec![0.1, 0.01]);
        assert!(matches!(config.architecture.kind, ArchitectureKind::Transformer { dropout, .. } if dropout == 0.1));
    }

    #[test]
    #[should_panic(expected = "training.patience_rounds is missing")]
    fn load_rejects_a_transformer_without_a_patience() {
        let path = transformer_fixture_with("no-patience", "", "  warmup_rounds: 0\n");
        let _ = load(path.to_str().unwrap());
    }

    /// One NNUE fixture body, with `learning_rate` and `patience_rounds` spelled however a test needs
    /// them, so the tests below differ only in the line they are about.
    fn nnue_fixture(name: &str, learning_rate: &str, patience_rounds: &str) -> std::path::PathBuf {
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("smithy-net-config-test-{pid}-{name}.yaml"));
        std::fs::write(
            &path,
            format!(
                "architecture:\n  kind: psq\n  \
                 embedding_dim: 8\n  value_head:\n    hidden_dim: [4]\ntraining:\n  learning_rate: {learning_rate}\n  \
                 training_batch_size: 1\n  test_batch_size: 1\n  steps_per_round: 1\n  \
                 warmup_rounds: 0\n  patience_rounds: {patience_rounds}\n"
            ),
        )
        .expect("failed to write the fixture file");
        path
    }

    #[test]
    fn load_widens_one_rate_into_one_stage_and_allows_no_warmup() {
        let path = nnue_fixture("one-rate", "3.0e-4", "12");

        let config = load(path.to_str().unwrap());

        assert_eq!(config.training.learning_rate, vec![3.0e-4]);
        assert_eq!(config.training.warmup_rounds, Some(0));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn load_keeps_the_stages_in_order() {
        let path = nnue_fixture("stages", "[3.0e-3, 1.0e-3, 3.0e-4]", "8");

        let config = load(path.to_str().unwrap());

        assert_eq!(config.training.learning_rate, vec![3.0e-3, 1.0e-3, 3.0e-4]);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    #[should_panic(expected = "must be strictly decreasing")]
    fn load_rejects_a_rate_that_goes_back_up() {
        let path = nnue_fixture("rising", "[1.0e-3, 3.0e-3]", "8");

        let _ = load(path.to_str().unwrap());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    #[should_panic(expected = "must be strictly decreasing")]
    fn load_rejects_a_repeated_rate() {
        let path = nnue_fixture("repeated", "[3.0e-4, 0.0003]", "8");

        let _ = load(path.to_str().unwrap());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    #[should_panic(expected = "is an empty list")]
    fn load_rejects_an_empty_list() {
        let path = nnue_fixture("no-rates", "[]", "8");

        let _ = load(path.to_str().unwrap());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    #[should_panic(expected = "not a finite, strictly positive rate")]
    fn load_rejects_a_zero_rate() {
        let path = nnue_fixture("zero-rate", "[3.0e-4, 0.0]", "12");

        let _ = load(path.to_str().unwrap());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    #[should_panic(expected = "training.patience_rounds must be at least 1")]
    fn load_rejects_a_patience_of_zero() {
        let path = nnue_fixture("zero-patience", "3.0e-4", "0");

        let _ = load(path.to_str().unwrap());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    #[should_panic(expected = "embedding_dim 7 is odd")]
    fn load_rejects_a_pairwise_kind_with_an_odd_embedding_dim() {
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("smithy-net-config-test-{pid}-odd-pairwise.yaml"));
        std::fs::write(
            &path,
            "architecture:\n  kind: psq_pairwise\n  embedding_dim: 7\n  value_head:\n    hidden_dim: []\ntraining:\n  \
             learning_rate: 0.1\n  training_batch_size: 1\n  test_batch_size: 1\n  steps_per_round: 1\n  \
             warmup_rounds: 0\n  patience_rounds: 1\n",
        )
        .expect("failed to write the fixture file");

        let _ = load(path.to_str().unwrap());
    }

    #[test]
    fn load_reads_screlu_and_rejects_it_with_a_hidden_layer() {
        let pid = std::process::id();
        let fixture = |name: &str, hidden: &str| -> std::path::PathBuf {
            let path = std::env::temp_dir().join(format!("smithy-net-config-test-{pid}-{name}.yaml"));
            std::fs::write(
                &path,
                format!(
                    "architecture:\n  kind: psq_threats_screlu\n  embedding_dim: 8\n  value_head:\n    hidden_dim: {hidden}\n\
                     training:\n  learning_rate: 0.1\n  training_batch_size: 1\n  test_batch_size: 1\n  steps_per_round: 1\n  \
                     warmup_rounds: 0\n  patience_rounds: 1\n"
                ),
            )
            .expect("failed to write the fixture file");
            path
        };
        let plain = fixture("screlu", "[]");
        let config = load(plain.to_str().unwrap());
        assert!(config.architecture.screlu() && !config.architecture.pairwise());
        assert!(config.architecture.head_shape() == HeadShape::Plain);
        std::fs::remove_file(&plain).ok();

        let hidden = fixture("screlu-hidden", "[4]");
        let refused = std::panic::catch_unwind(|| load(hidden.to_str().unwrap()));
        std::fs::remove_file(&hidden).ok();
        assert!(refused.is_err(), "a hidden layer refused");
    }

    #[test]
    fn load_reads_screlu_phase_as_a_phase_head() {
        let path = std::env::temp_dir().join(format!("smithy-net-config-test-{}-screlu-phase.yaml", std::process::id()));
        std::fs::write(
            &path,
            "architecture:\n  kind: psq_threats_screlu_phase\n  embedding_dim: 8\n  value_head:\n    hidden_dim: []\n\
             training:\n  learning_rate: 0.1\n  training_batch_size: 1\n  test_batch_size: 1\n  steps_per_round: 1\n  \
             warmup_rounds: 0\n  patience_rounds: 1\n",
        )
        .expect("failed to write the fixture file");
        let config = load(path.to_str().unwrap());
        std::fs::remove_file(&path).ok();
        assert!(config.architecture.screlu() && config.architecture.material() && !config.architecture.pairwise());
        assert!(config.architecture.head_shape() == HeadShape::Phase);
        assert_eq!(config.architecture.head_shape().output_buckets(), 25);
    }

    #[test]
    fn load_reads_screlu_count_as_a_count_head() {
        let path = std::env::temp_dir().join(format!("smithy-net-config-test-{}-screlu-count.yaml", std::process::id()));
        std::fs::write(
            &path,
            "architecture:\n  kind: psq_threats_screlu_count\n  embedding_dim: 8\n  value_head:\n    hidden_dim: []\n\
             training:\n  learning_rate: 0.1\n  training_batch_size: 1\n  test_batch_size: 1\n  steps_per_round: 1\n  \
             warmup_rounds: 0\n  patience_rounds: 1\n",
        )
        .expect("failed to write the fixture file");
        let config = load(path.to_str().unwrap());
        std::fs::remove_file(&path).ok();
        assert!(config.architecture.screlu() && config.architecture.material() && !config.architecture.pairwise());
        assert!(config.architecture.head_shape() == HeadShape::Count);
        assert_eq!(config.architecture.head_shape().output_buckets(), 8);
    }

    #[test]
    fn load_reads_screlu_taper_as_a_taper_head() {
        let path = std::env::temp_dir().join(format!("smithy-net-config-test-{}-screlu-taper.yaml", std::process::id()));
        std::fs::write(
            &path,
            "architecture:\n  kind: psq_threats_screlu_taper\n  embedding_dim: 8\n  value_head:\n    hidden_dim: []\n\
             training:\n  learning_rate: 0.1\n  training_batch_size: 1\n  test_batch_size: 1\n  steps_per_round: 1\n  \
             warmup_rounds: 0\n  patience_rounds: 1\n",
        )
        .expect("failed to write the fixture file");
        let config = load(path.to_str().unwrap());
        std::fs::remove_file(&path).ok();
        assert!(config.architecture.screlu() && config.architecture.material() && !config.architecture.pairwise());
        assert!(config.architecture.head_shape() == HeadShape::Taper);
        assert_eq!(config.architecture.head_shape().output_buckets(), 2);
    }

    #[test]
    fn load_reads_screlu_kings_as_a_kings_head_and_rejects_it_without_one_hidden_width() {
        let pid = std::process::id();
        let fixture = |name: &str, hidden: &str| -> std::path::PathBuf {
            let path = std::env::temp_dir().join(format!("smithy-net-config-test-{pid}-{name}.yaml"));
            std::fs::write(
                &path,
                format!(
                    "architecture:\n  kind: psq_threats_screlu_kings\n  embedding_dim: 8\n  value_head:\n    hidden_dim: {hidden}\n\
                     training:\n  learning_rate: 0.1\n  training_batch_size: 1\n  test_batch_size: 1\n  steps_per_round: 1\n  \
                     warmup_rounds: 0\n  patience_rounds: 1\n"
                ),
            )
            .expect("failed to write the fixture file");
            path
        };
        let one = fixture("screlu-kings", "[16]");
        let config = load(one.to_str().unwrap());
        std::fs::remove_file(&one).ok();
        assert!(config.architecture.screlu() && config.architecture.kings() && !config.architecture.pairwise());
        assert!(!config.architecture.stack() && !config.architecture.material());
        assert!(config.architecture.head_shape() == HeadShape::Kings);

        for (name, hidden) in [("screlu-kings-none", "[]"), ("screlu-kings-two", "[16, 16]")] {
            let path = fixture(name, hidden);
            let refused = std::panic::catch_unwind(|| load(path.to_str().unwrap()));
            std::fs::remove_file(&path).ok();
            assert!(refused.is_err(), "{hidden} refused");
        }
    }

    #[test]
    fn load_reads_screlu_and_pairwise_hidden_as_a_hidden_head_and_rejects_them_without_one_hidden_width() {
        let pid = std::process::id();
        let fixture = |name: &str, kind: &str, hidden: &str| -> std::path::PathBuf {
            let path = std::env::temp_dir().join(format!("smithy-net-config-test-{pid}-{name}.yaml"));
            std::fs::write(
                &path,
                format!(
                    "architecture:\n  kind: {kind}\n  embedding_dim: 8\n  value_head:\n    hidden_dim: {hidden}\n\
                     training:\n  learning_rate: 0.1\n  training_batch_size: 1\n  test_batch_size: 1\n  steps_per_round: 1\n  \
                     warmup_rounds: 0\n  patience_rounds: 1\n"
                ),
            )
            .expect("failed to write the fixture file");
            path
        };
        for (kind, screlu) in [("psq_threats_screlu_hidden", true), ("psq_threats_pairwise_hidden", false)] {
            let one = fixture(kind, kind, "[16]");
            let config = load(one.to_str().unwrap());
            std::fs::remove_file(&one).ok();
            assert!(config.architecture.hidden_beside() && !config.architecture.kings(), "{kind}");
            assert_eq!((config.architecture.screlu(), config.architecture.pairwise()), (screlu, !screlu), "{kind}");
            assert!(!config.architecture.stack() && !config.architecture.material(), "{kind}");
            assert!(config.architecture.head_shape() == HeadShape::Hidden, "{kind}");

            for (name, hidden) in [("none", "[]"), ("two", "[16, 16]")] {
                let path = fixture(&format!("{kind}-{name}"), kind, hidden);
                let refused = std::panic::catch_unwind(|| load(path.to_str().unwrap()));
                std::fs::remove_file(&path).ok();
                assert!(refused.is_err(), "{kind} {hidden} refused");
            }
        }
    }

    #[test]
    fn load_reads_screlu_material_and_rejects_it_with_a_hidden_layer() {
        let pid = std::process::id();
        let fixture = |name: &str, hidden: &str| -> std::path::PathBuf {
            let path = std::env::temp_dir().join(format!("smithy-net-config-test-{pid}-{name}.yaml"));
            std::fs::write(
                &path,
                format!(
                    "architecture:\n  kind: psq_threats_screlu_material\n  embedding_dim: 8\n  value_head:\n    hidden_dim: {hidden}\n\
                     training:\n  learning_rate: 0.1\n  training_batch_size: 1\n  test_batch_size: 1\n  steps_per_round: 1\n  \
                     warmup_rounds: 0\n  patience_rounds: 1\n"
                ),
            )
            .expect("failed to write the fixture file");
            path
        };
        let plain = fixture("screlu-material", "[]");
        let config = load(plain.to_str().unwrap());
        assert!(config.architecture.screlu() && config.architecture.material() && !config.architecture.pairwise());
        assert!(config.architecture.head_shape() == HeadShape::Material);
        std::fs::remove_file(&plain).ok();

        let hidden = fixture("screlu-material-hidden", "[4]");
        let refused = std::panic::catch_unwind(|| load(hidden.to_str().unwrap()));
        std::fs::remove_file(&hidden).ok();
        assert!(refused.is_err(), "a hidden layer refused");
    }

    #[test]
    #[should_panic(expected = "value_head.hidden_dim has 1 widths")]
    fn load_rejects_a_stack_without_exactly_two_hidden_widths() {
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("smithy-net-config-test-{pid}-one-width-stack.yaml"));
        std::fs::write(
            &path,
            "architecture:\n  kind: psq_stack\n  embedding_dim: 8\n  value_head:\n    hidden_dim: [16]\ntraining:\n  \
             learning_rate: 0.1\n  training_batch_size: 1\n  test_batch_size: 1\n  steps_per_round: 1\n  \
             warmup_rounds: 0\n  patience_rounds: 1\n",
        )
        .expect("failed to write the fixture file");

        let _ = load(path.to_str().unwrap());
    }

    #[test]
    fn every_committed_forge_config_s_network_loads() {
        temper_chess::pipeline::repository_root::enter_for_tests();
        for name in ["pesto-psq-threats-screlu", "lc0-psq-threats-screlu"] {
            let config = load(&format!("configs/chess/{name}.yaml"));
            assert!(matches!(config.architecture.kind, ArchitectureKind::PsqThreatsScrelu), "{name}");
            assert!(config.training.patience_rounds.is_some(), "{name}");
        }
    }
}
