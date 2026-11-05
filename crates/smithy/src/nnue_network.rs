//! The one-network NNUE, arm by arm what `StackedNetwork` trains. The binary only builds it, for the
//! parameters a checkpoint is loaded into and exported from; its forward passes are the stack's test
//! reference, so outside the tests they are unread.
#![cfg_attr(not(test), allow(dead_code))]

use tch::nn;
use tch::Tensor;

use super::batch::DenseBatch;
use super::feature_set::FeatureSet;
use super::net_config::Architecture;
use temper_chess::features::{halfk, psq, psq_threats};
use temper_chess::labels::S_PAWNS;
use temper_chess::stack_buckets::TAPER_SETS;
pub use super::stack_head::HeadShape;
use super::stack_head::{hidden_output_input, kings_stage_input, taper, StackHead, TAPER_PHASE_COLUMN};
use super::value_head::{Activation, ValueHead};

/// `labels::LOGISTIC`'s score on a tensor of evaluations in pawns, `tanh(y / (2 S))`, in `[-1, 1]`:
/// the networks' unit, the prediction every network's output stands for.
pub fn score(pawns: &Tensor) -> Tensor {
    (pawns / (2.0 * S_PAWNS)).tanh()
}

/// The loss every network trains on, from its evaluation in pawns and the label `t` in `[-1, 1]`:
/// the binary cross entropy of `sigma(y / S)` against the expected score `(t + 1) / 2`, computed from
/// the logit `y / S` for numerical stability. Its gradient on the pawns is `(sigma(y / S) - (t + 1) / 2)
/// / S`: bounded, never zero while the prediction is wrong, so every position keeps a gradient, and a
/// decisive label (`t = ±1`) pushes the output ever more weakly as it grows, never to a finite optimum.
/// Training computes it per arm (`stacked_network::arm_training_losses`, for the NNUE's stack and the
/// transformer's alike), tested against this one-arm form.
#[cfg(test)]
pub fn bce_loss(pawns: &Tensor, label: &Tensor) -> Tensor {
    (pawns / S_PAWNS).binary_cross_entropy_with_logits(&((label + 1.0) / 2.0), None::<Tensor>, None::<Tensor>, tch::Reduction::Mean)
}

/// The bound on a psq network's first hidden layer weights: what an `i8` holds at a fixed scale of
/// 64, Stockfish's (and nnue-pytorch's) choice, so that the engine can read that layer as `i8`
/// and multiply it with `maddubs`. Kept in training (`Network::weight_bounds`) rather than imposed
/// at export, so the network learns within it. Betty's generation 1, trained without it, already
/// fit: its largest weight was 1.943 (2026-10-07).
pub const FIRST_HIDDEN_WEIGHT_BOUND: f64 = 127.0 / 64.0;

/// The name `VarStore` files a psq network's first hidden layer weights under.
pub const FIRST_HIDDEN_WEIGHT: &str = "value_head.hidden0.weight";

/// Temper's own eval network, one variant per feature set (there is no bypass, no
/// piece-square-table term, and no MoE mixture any more).
pub enum Network {
    Psq(NetworkPsq),
    /// psq's shape over `psq_threats::ROWS` rows: the threat rows go through the same transform
    /// as the piece-square ones, added before the activation, so one neuron can see a threat and
    /// the pieces on its squares together.
    PsqThreats(NetworkPsq),
    /// psq_threats_screlu's shape over `halfk::TRAINING_ROWS` rows: halfk's, then the factor's
    /// (`with_options` sets their starting weights).
    HalfKav2Hm(NetworkPsq),
}

impl Network {
    /// `embedding_dim`/`value_head_hidden_dims` come from `net.yaml`'s `architecture.embedding_dim`/
    /// `architecture.value_head.hidden_dim` -- the one shape both NNUE and the transformer read the
    /// same way (`net_config::Architecture`'s own doc comment). Every other width (the feature
    /// transformer's own input width) stays fixed by the feature set itself, in code, not
    /// configurable.
    #[cfg(test)]
    pub fn new(vs: &nn::Path, feature_set: FeatureSet, embedding_dim: i64, value_head_hidden_dims: &[i64]) -> Network {
        Network::with_pairwise(vs, feature_set, false, embedding_dim, value_head_hidden_dims)
    }

    /// `new`, with each perspective's clipped transform split in two halves multiplied element-wise
    /// when `pairwise` (`NetworkPsq::pawns`). Psq and psq_threats only.
    #[cfg(test)]
    pub fn with_pairwise(
        vs: &nn::Path, feature_set: FeatureSet, pairwise: bool, embedding_dim: i64, value_head_hidden_dims: &[i64],
    ) -> Network {
        Network::with_head(vs, feature_set, pairwise, HeadShape::Plain, embedding_dim, value_head_hidden_dims)
    }

    /// `with_pairwise`, its head `head`: plain, bucketed by our king, their king and the material
    /// (`HeadShape::Stack`, `stack_head::StackHead`, two hidden layers), or a single output bucketed
    /// by the material (`HeadShape::Material`, no hidden layer). A bucketed head is pairwise, psq or
    /// psq_threats; a material one psq_threats.
    #[cfg(test)]
    pub fn with_head(
        vs: &nn::Path, feature_set: FeatureSet, pairwise: bool, head: HeadShape, embedding_dim: i64, value_head_hidden_dims: &[i64],
    ) -> Network {
        Network::with_options(vs, feature_set, pairwise, false, head, embedding_dim, value_head_hidden_dims)
    }

    /// `with_head`, each perspective's clipped transform squared lane by lane when `screlu`
    /// (`NetworkPsq::pawns`): psq_threats only, unpaired, its head plain or material sets reading the
    /// squares with no hidden layer, or the kings' two sides, one hidden width.
    pub fn with_options(
        vs: &nn::Path, feature_set: FeatureSet, pairwise: bool, screlu: bool, head: HeadShape, embedding_dim: i64,
        value_head_hidden_dims: &[i64],
    ) -> Network {
        let halfk = feature_set == FeatureSet::HalfKav2Hm;
        let halfk_activation = match value_head_hidden_dims.len() {
            0 => screlu && !pairwise,
            2 => screlu != pairwise,
            _ => false,
        };
        assert!(
            !halfk || (halfk_activation && head == HeadShape::Plain),
            "a halfk network is halfk_threats_screlu, halfk_threats_screlu_hidden or halfk_threats_pairwise_hidden: \
             a plain head, screlu with no hidden layer or two, or pairwise with two"
        );
        assert!(
            !screlu
                || (matches!(feature_set, FeatureSet::PsqThreats | FeatureSet::HalfKav2Hm)
                    && !pairwise
                    && head != HeadShape::Stack
                    && (value_head_hidden_dims.is_empty() || halfk || matches!(head, HeadShape::Kings | HeadShape::Hidden))),
            "a screlu network is psq_threats or halfk with no hidden layer outside halfk's plain head, the kings' stages or the hidden head, not pairwise or a stack"
        );
        assert!(
            head != HeadShape::Hidden || (value_head_hidden_dims.len() == 1 && feature_set == FeatureSet::PsqThreats && (screlu || pairwise)),
            "a hidden-beside head is psq_threats screlu's or pairwise's, with one hidden layer"
        );
        assert!(
            !pairwise || matches!(feature_set, FeatureSet::Psq | FeatureSet::PsqThreats | FeatureSet::HalfKav2Hm),
            "pairwise products exist for psq, psq_threats and halfk only, not {feature_set:?}"
        );
        // A screlu material, phase, count or kings head reads the squares instead.
        assert!(
            !head.bucketed() || pairwise || (screlu && (head.output_only() || matches!(head, HeadShape::Kings | HeadShape::Hidden))),
            "a bucketed head reads pairwise products"
        );
        assert!(!head.screlu_only() || screlu, "a {head:?} head is screlu's output");
        assert!(
            !head.output_only() || (feature_set == FeatureSet::PsqThreats && value_head_hidden_dims.is_empty()),
            "a material or phase head is psq_threats pairwise's or screlu's output, with no hidden layer"
        );
        match feature_set {
            FeatureSet::Psq => {
                Network::Psq(NetworkPsq::with_rows(vs, psq::ROWS, pairwise, false, head, embedding_dim, value_head_hidden_dims))
            }
            FeatureSet::PsqThreats => Network::PsqThreats(NetworkPsq::with_rows(
                vs, psq_threats::ROWS, pairwise, screlu, head, embedding_dim, value_head_hidden_dims,
            )),
            FeatureSet::HalfKav2Hm => {
                let network =
                    NetworkPsq::with_rows(vs, halfk::TRAINING_ROWS, pairwise, screlu, head, embedding_dim, value_head_hidden_dims);
                network.start_factorised();
                Network::HalfKav2Hm(network)
            }
        }
    }

    /// `new` for a network described by `architecture`.
    pub fn of(vs: &nn::Path, feature_set: FeatureSet, architecture: &Architecture) -> Network {
        Network::with_options(
            vs, feature_set, architecture.pairwise(), architecture.screlu(), architecture.head_shape(), architecture.embedding_dim(),
            architecture.value_head_hidden_dim(),
        )
    }

    /// The parameters training keeps within a bound (`AdamW::bound`), by name: a psq or psq_threats
    /// network's first hidden layer weights, for the engine's `i8` layer. None for any other, a stack
    /// included: the bound served the shelved `i8` layer, and one on its shared part and its deltas
    /// apart would not bound their sum. Nor halfk's: the engine quantizes its first hidden layer on
    /// one scale per row, which fits any range. `StackedNetwork::weight_bounds` is tested to agree
    /// with it.
    #[cfg(test)]
    pub fn weight_bounds(&self) -> Vec<(&'static str, f64)> {
        match self {
            Network::Psq(network) | Network::PsqThreats(network) => match &network.head {
                Head::Plain(value_head) if value_head.has_hidden_layer() => vec![(FIRST_HIDDEN_WEIGHT, FIRST_HIDDEN_WEIGHT_BOUND)],
                _ => Vec::new(),
            },
            Network::HalfKav2Hm(_) => Vec::new(),
        }
    }

    /// The loss one training step descends, for every kind: `bce_loss` on the pawns, against arm
    /// `arm`'s labels (`DenseBatch::label`). Training steps the stacked network; this is the reference
    /// it is tested against.
    #[cfg(test)]
    pub fn training_loss(&self, batch: &DenseBatch, arm: usize) -> Tensor {
        bce_loss(&self.pawns(batch), &batch.label(arm))
    }

    /// The test loss against arm `arm`'s label, for every kind: the same binary cross entropy as
    /// `training_loss`, the number the learning-rate stages and early stopping read, comparable across
    /// architectures. The reference `StackedNetwork::test_losses` is tested against.
    #[cfg(test)]
    pub fn test_loss(&self, batch: &DenseBatch, arm: usize) -> Tensor {
        bce_loss(&self.pawns(batch), &batch.label(arm))
    }

    /// The label-space prediction, `score(pawns)`: `[batch, 1]`.
    pub fn forward(&self, batch: &DenseBatch) -> Tensor {
        score(&self.pawns(batch))
    }

    /// The evaluation in pawns, from the side to move's view: `[batch, 1]`, unbounded (the engine
    /// clamps it to `±CEILING_PAWNS`; in training a clamp would zero the gradient of any overshoot,
    /// while `score` saturates anyway). Dispatches to whichever concrete network `self` is, matched
    /// against the same-variant `DenseBatch`. The mismatched combinations (a psq network handed a
    /// psq_threats batch) have no real arm -- training builds one `Network` and feeds it batches
    /// built under that same `FeatureSet` for the run's whole lifetime, so the wildcard arm below is
    /// an assertion against a logic bug.
    pub fn pawns(&self, batch: &DenseBatch) -> Tensor {
        match (self, batch) {
            (Network::Psq(network), DenseBatch::Psq(batch))
            | (Network::PsqThreats(network), DenseBatch::PsqThreats(batch))
            | (Network::HalfKav2Hm(network), DenseBatch::HalfKav2Hm(batch)) => {
                let (own_dense, opponent_dense) = batch.dense();
                network.pawns(&own_dense, &opponent_dense, batch.buckets())
            }
            _ => panic!("Network/DenseBatch feature-set mismatch"),
        }
    }
}

/// psq's shape: one shared transform (`psq::ROWS -> embedding_dim`) applied to both
/// perspectives, concatenated side to move first. Clipped ReLU, after the transform and in the
/// value head, so the integer version computes the same function up to rounding.
///
/// With `pairwise`, each perspective's clipped output is split in two halves multiplied
/// element-wise, `a * b`, with no weights of their own: every product is a learned "group A present
/// AND group B present" over pairs of pieces, and the head reads `embedding_dim` values, half from
/// each perspective. The integer rescale of the product lives in the engine alone.
///
/// With `screlu`, each perspective's clipped output is squared lane by lane, `a * a`, the squared
/// clipped ReLU; the head reads `2 * embedding_dim` squares. The engine computes it as a pairwise
/// product of a lane with itself.
pub struct NetworkPsq {
    transform: nn::Linear,
    head: Head,
    pairwise: bool,
    screlu: bool,
}

/// A psq network's head: one MLP, or bucketed stages per position (`stack_head::StackHead`), a
/// stack's or a material output's, or a taper's two outputs (`value_head.output`, `[2, in]`,
/// middlegame then endgame, not factorised) blended by the phase (`stack_head::taper`), or a
/// hidden-beside head: `value_head.hidden0` over the squares or the products, detached, and
/// `value_head.output` over its values and them (`stack_head::hidden_output_input`).
enum Head {
    Plain(ValueHead),
    Stack(StackHead),
    Taper(nn::Linear),
    HiddenBeside { hidden: nn::Linear, output: nn::Linear },
}

impl NetworkPsq {
    /// psq's shape over a transform of `rows` rows: psq_threats' is psq's rows plus its threats.
    fn with_rows(
        vs: &nn::Path, rows: usize, pairwise: bool, screlu: bool, shape: HeadShape, embedding_dim: i64, value_head_hidden_dims: &[i64],
    ) -> NetworkPsq {
        assert!(!pairwise || embedding_dim % 2 == 0, "pairwise products need an even embedding_dim, not {embedding_dim}");
        let transform = nn::linear(vs / "transform", rows as i64, embedding_dim, Default::default());
        let head_input = if pairwise { embedding_dim } else { embedding_dim * 2 };
        let head = match shape {
            HeadShape::Stack => Head::Stack(StackHead::new(&(vs / "value_head"), head_input, value_head_hidden_dims)),
            HeadShape::Kings => Head::Stack(StackHead::kings(&(vs / "value_head"), head_input, value_head_hidden_dims)),
            HeadShape::Material | HeadShape::Phase | HeadShape::Count => Head::Stack(StackHead::output_only(&(vs / "value_head"), head_input, shape)),
            HeadShape::Taper => {
                Head::Taper(nn::linear(vs / "value_head" / "output", head_input, TAPER_SETS as i64, Default::default()))
            }
            HeadShape::Hidden => {
                let width = value_head_hidden_dims[0];
                let hidden = nn::linear(vs / "value_head" / "hidden0", head_input, width, Default::default());
                let output = nn::linear(vs / "value_head" / "output", 2 * width + head_input, 1, Default::default());
                Head::HiddenBeside { hidden, output }
            }
            HeadShape::Plain => {
                Head::Plain(ValueHead::new(&(vs / "value_head"), head_input, value_head_hidden_dims, Activation::ClippedRelu))
            }
        };
        NetworkPsq { transform, head, pairwise, screlu }
    }

    /// A halfk network's starting weights, factorised: its 22,528 piece rows at zero, so that every
    /// bucket starts as the factor and learns only how it differs from it, and its threat rows, factor
    /// rows and bias drawn as a fresh psq_threats network's would be. `nn::linear` draws every row
    /// within a bound proportional to `1 / sqrt(rows)`, here over the training width; rescaled to
    /// psq_threats' width, a fresh halfk network starts as a fresh psq_threats one in distribution.
    /// Stockfish's choice: a rare king bucket keeps the factor's weights until its own data moves it.
    fn start_factorised(&self) {
        let scale = (halfk::TRAINING_ROWS as f64 / psq_threats::ROWS as f64).sqrt();
        tch::no_grad(|| {
            let mut weight = self.transform.ws.shallow_clone();
            let _ = weight.g_mul_scalar_(scale);
            let _ = weight.narrow(1, 0, halfk::HALFK_ROWS as i64).zero_();
            if let Some(bias) = &self.transform.bs {
                let _ = bias.shallow_clone().g_mul_scalar_(scale);
            }
        });
    }

    /// `buckets` (`Int64 [batch, 5]`) are read by a bucketed head only.
    fn pawns(&self, own_dense: &Tensor, opponent_dense: &Tensor, buckets: &Tensor) -> Tensor {
        let own = Activation::ClippedRelu.apply(&own_dense.apply(&self.transform));
        let opponent = Activation::ClippedRelu.apply(&opponent_dense.apply(&self.transform));
        let (own, opponent) = if self.pairwise {
            (pairwise_products(&own, 1), pairwise_products(&opponent, 1))
        } else if self.screlu {
            (own.square(), opponent.square())
        } else {
            (own, opponent)
        };
        let accumulator = Tensor::cat(&[own, opponent], 1);
        match &self.head {
            Head::Plain(value_head) => value_head.forward(&accumulator),
            Head::Stack(stack_head) => stack_head.forward(&accumulator, buckets),
            Head::Taper(output) => taper(&accumulator.apply(output), &buckets.select(1, TAPER_PHASE_COLUMN)).unsqueeze(1),
            Head::HiddenBeside { hidden, output } => {
                let values = kings_stage_input(&accumulator).apply(hidden);
                hidden_output_input(&values, &accumulator).apply(output)
            }
        }
    }
}

/// The first half of `x` along `dim` times its second half, element-wise: a pairwise network's
/// perspective, from its clipped transform. `StackedNetwork::head` uses it too.
pub fn pairwise_products(x: &Tensor, dim: i64) -> Tensor {
    let half = x.size()[dim as usize] / 2;
    x.narrow(dim, 0, half) * x.narrow(dim, half, half)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tch::Kind;
    use super::super::batch::DenseBatchPsq;

    fn dense_row(active_rows: &[usize], width: usize) -> Tensor {
        let mut row = vec![0f32; width];
        for &index in active_rows {
            row[index] = 1.0;
        }
        Tensor::from_slice(&row).view([1, width as i64])
    }

    fn zero_batch(feature_set: FeatureSet, batch_size: i64) -> DenseBatch {
        match feature_set {
            FeatureSet::Psq => {
                let zeros = Tensor::zeros([batch_size, psq::ROWS as i64], (tch::Kind::Float, tch::Device::Cpu));
                let label = Tensor::zeros([batch_size, 1], (tch::Kind::Float, tch::Device::Cpu));
                DenseBatch::Psq(DenseBatchPsq::from_dense(&zeros, &zeros, label))
            }
            FeatureSet::PsqThreats => {
                let zeros = Tensor::zeros([batch_size, psq_threats::ROWS as i64], (tch::Kind::Float, tch::Device::Cpu));
                let label = Tensor::zeros([batch_size, 1], (tch::Kind::Float, tch::Device::Cpu));
                DenseBatch::PsqThreats(DenseBatchPsq::from_dense(&zeros, &zeros, label))
            }
            FeatureSet::HalfKav2Hm => {
                let zeros = Tensor::zeros([batch_size, halfk::TRAINING_ROWS as i64], (tch::Kind::Float, tch::Device::Cpu));
                let label = Tensor::zeros([batch_size, 1], (tch::Kind::Float, tch::Device::Cpu));
                DenseBatch::HalfKav2Hm(DenseBatchPsq::from_dense(&zeros, &zeros, label))
            }
        }
    }

    #[test]
    fn forward_produces_the_expected_shape_for_every_feature_set() {
        for feature_set in [FeatureSet::Psq, FeatureSet::PsqThreats, FeatureSet::HalfKav2Hm] {
            let vs = nn::VarStore::new(tch::Device::Cpu);
            let network = if feature_set == FeatureSet::HalfKav2Hm {
                Network::with_options(&vs.root(), feature_set, false, true, HeadShape::Plain, 8, &[])
            } else {
                Network::new(&vs.root(), feature_set, 8, &[4])
            };
            let batch = zero_batch(feature_set, 3);

            let output = network.forward(&batch);
            assert_eq!(output.size(), [3, 1]);
        }
    }

    #[test]
    fn forward_produces_one_finite_unbounded_value_per_row_for_every_feature_set() {
        for feature_set in [FeatureSet::Psq, FeatureSet::PsqThreats] {
            let vs = nn::VarStore::new(tch::Device::Cpu);
            let network = Network::new(&vs.root(), feature_set, 8, &[4]);

            let width = match feature_set {
                FeatureSet::Psq => psq::ROWS,
                FeatureSet::PsqThreats => psq_threats::ROWS,
                FeatureSet::HalfKav2Hm => unreachable!("halfk has its own forward test, `forward_produces_the_expected_shape_for_every_feature_set`"),
            };
            let batch = match feature_set {
                FeatureSet::HalfKav2Hm => unreachable!(),
                FeatureSet::Psq => DenseBatch::Psq(DenseBatchPsq::from_dense(
                    &dense_row(&[0, 53, width - 1], width),
                    &Tensor::zeros([1, width as i64], (tch::Kind::Float, tch::Device::Cpu)),
                    Tensor::zeros([1, 1], (tch::Kind::Float, tch::Device::Cpu)),
                )),
                FeatureSet::PsqThreats => DenseBatch::PsqThreats(DenseBatchPsq::from_dense(
                    &dense_row(&[0, 53, width - 1], width),
                    &Tensor::zeros([1, width as i64], (tch::Kind::Float, tch::Device::Cpu)),
                    Tensor::zeros([1, 1], (tch::Kind::Float, tch::Device::Cpu)),
                )),
            };

            let output = network.pawns(&batch);
            let value = f64::try_from(&output).expect("network output should reduce to a scalar");
            assert!(value.is_finite(), "expected a finite unbounded value, got {value} for {feature_set:?}");
        }
    }

    /// The gradient on the pawns is `(sigma(y / S) - (t + 1) / 2) / S`; a decisive label far past the
    /// prediction keeps a gradient, vanishing only as the output grows.
    #[test]
    fn the_bce_loss_has_the_logistic_gradient() {
        let sigma = |x: f64| 1.0 / (1.0 + (-x).exp());
        let (pawns, labels) = ([0.3f64, -2.0, 1.0, 20.0], [0.1f64, 0.5, 1.0, 1.0]);
        let tensor = Tensor::from_slice(&pawns.map(|p| p as f32)).set_requires_grad(true);
        // `mean` divides the gradient by the row count, so it is scaled back here.
        (bce_loss(&tensor, &Tensor::from_slice(&labels.map(|t| t as f32))) * 4.0).backward();
        let gradient = Vec::<f32>::try_from(&tensor.grad()).unwrap();
        for i in 0..4 {
            let expected = (sigma(pawns[i] / S_PAWNS) - (labels[i] + 1.0) / 2.0) / S_PAWNS;
            assert!((gradient[i] as f64 - expected).abs() < 1e-5, "{i}: {} against {expected}", gradient[i]);
        }
        assert!(gradient[2] < 0.0, "a won label above the prediction pushes the output up");
        assert!(gradient[3].abs() < 1e-9, "far past a won label the push has died away");
    }

    /// The loss's value is the binary cross entropy written out, `-(p log s + (1 - p) log(1 - s))`
    /// with `s = sigma(y / S)` and `p = (t + 1) / 2`.
    #[test]
    fn the_bce_loss_is_the_binary_cross_entropy_written_out() {
        let sigma = |x: f64| 1.0 / (1.0 + (-x).exp());
        let (pawns, labels) = ([0.3f64, -2.0, 1.0], [0.1f64, 0.5, -1.0]);
        let expected = pawns
            .iter()
            .zip(labels)
            .map(|(&y, t)| {
                let (s, p) = (sigma(y / S_PAWNS), (t + 1.0) / 2.0);
                -(p * s.ln() + (1.0 - p) * (1.0 - s).ln())
            })
            .sum::<f64>()
            / 3.0;
        let loss = bce_loss(&Tensor::from_slice(&pawns.map(|p| p as f32)), &Tensor::from_slice(&labels.map(|t| t as f32)));
        let got = f64::try_from(&loss).unwrap();
        assert!((got - expected).abs() < 1e-5, "{got} against {expected}");
    }

    #[test]
    fn a_dense_networks_training_loss_is_its_test_loss() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let network = Network::new(&vs.root(), FeatureSet::Psq, 8, &[4]);
        let width = psq::ROWS;
        let batch = DenseBatch::Psq(DenseBatchPsq::from_dense(
            &dense_row(&[0, 53, width - 1], width),
            &dense_row(&[7], width),
            Tensor::from_slice(&[0.4f32]).view([1, 1]),
        ));
        let test = network.test_loss(&batch, 0);
        let loss = network.training_loss(&batch, 0);
        let (got, expected) = (f64::try_from(&loss).unwrap(), f64::try_from(&test).unwrap());
        assert!((got - expected).abs() < 1e-6, "{got} against {expected}");
    }

    #[test]
    fn each_arms_losses_equal_those_of_a_batch_built_from_its_labels_alone() {
        use super::super::batch::to_dense_batch;
        use super::super::cache::CacheRecord;
        use temper_chess::board::Board;
        use temper_chess::features::extract_canonical;

        let fens = [
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3",
            "8/5k2/8/3P4/8/8/2K5/8 b - - 0 1",
        ];
        let arm_labels = [[0.1f32, -0.3, 0.6], [0.2, 0.5, -0.7]];
        let canonicals: Vec<_> = fens.iter().map(|fen| extract_canonical(&Board::from_fen(fen).unwrap().0)).collect();
        let lockstep: Vec<CacheRecord> = canonicals
            .iter()
            .enumerate()
            .map(|(row, canonical)| CacheRecord {
                canonical: canonical.clone(),
                labels: arm_labels.iter().map(|labels| labels[row]).collect(),
                progress: crate::cache::Progress::UNKNOWN,
            })
            .collect();

        for feature_set in [FeatureSet::Psq, FeatureSet::PsqThreats] {
            let vs = nn::VarStore::new(tch::Device::Cpu);
            let network = Network::new(&vs.root(), feature_set, 8, &[4]);
            let batch = to_dense_batch(&lockstep, feature_set);
            for (arm, labels) in arm_labels.iter().enumerate() {
                let single: Vec<CacheRecord> =
                    canonicals.iter().zip(labels).map(|(canonical, &label)| CacheRecord::new(canonical.clone(), label)).collect();
                let single = to_dense_batch(&single, feature_set);
                assert!(batch.label(arm).equal(&single.label(0)), "arm {arm}'s label column");
                assert!(network.training_loss(&batch, arm).equal(&network.training_loss(&single, 0)));
                assert!(network.test_loss(&batch, arm).equal(&network.test_loss(&single, 0)));
            }
        }
    }

    #[test]
    fn swapping_which_perspective_is_active_changes_the_output() {
        // `own_dense`/`opponent_dense` are concatenated in a fixed order before the MLP, so
        // moving the same active input from one slot to the other must generally change the
        // final output -- if it didn't, the MLP would be structurally blind to which perspective
        // is which, silently discarding real information `forward`'s own signature promises to
        // use. Checked against psq alone -- the concatenation order is the same logic for every
        // feature set.
        //
        // A single random weight draw can, by pure chance, land every hidden unit on the
        // same side of its clip for both perspectives at once, collapsing both forward
        // passes to the exact same value regardless of which perspective is active -- not a
        // structural bug, just an unlucky init. Retried over several independent draws: that
        // coincidence landing on every single one is vanishingly unlikely, so this only fails
        // when the structural property itself is actually broken.
        let width = psq::ROWS;
        let input = dense_row(&[10, 200], width);
        let zero = Tensor::zeros([1, width as i64], (tch::Kind::Float, tch::Device::Cpu));
        let label = || Tensor::zeros([1, 1], (tch::Kind::Float, tch::Device::Cpu));

        for _ in 0..20 {
            let vs = nn::VarStore::new(tch::Device::Cpu);
            let network = Network::new(&vs.root(), FeatureSet::Psq, 8, &[4]);

            let own_active = DenseBatch::Psq(DenseBatchPsq::from_dense(&input, &zero, label()));
            let opponent_active = DenseBatch::Psq(DenseBatchPsq::from_dense(&zero, &input, label()));

            let own_active_value = f64::try_from(&network.forward(&own_active)).unwrap();
            let opponent_active_value = f64::try_from(&network.forward(&opponent_active)).unwrap();

            if own_active_value != opponent_active_value {
                return;
            }
        }
        panic!("own/opponent perspective produced the same output on 20 independent random inits in a row");
    }

    #[test]
    fn a_halfk_network_starts_factorised_as_a_fresh_psq_threats_one() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let _ = Network::with_options(&vs.root(), FeatureSet::HalfKav2Hm, false, true, HeadShape::Plain, 64, &[]);
        let weight = &vs.variables()["transform.weight"];
        assert_eq!(weight.size(), [64, halfk::TRAINING_ROWS as i64]);
        let pieces = weight.narrow(1, 0, halfk::HALFK_ROWS as i64);
        assert_eq!(pieces.abs().max().double_value(&[]), 0.0, "every bucket starts as the factor");
        // The rest within a fresh psq_threats transform's bound, and filling it, not
        // `1 / sqrt(TRAINING_ROWS)`'s narrower one.
        let rest = weight.narrow(1, halfk::HALFK_ROWS as i64, (halfk::TRAINING_ROWS - halfk::HALFK_ROWS) as i64);
        let reference = nn::VarStore::new(tch::Device::Cpu);
        let _ = Network::with_options(&reference.root(), FeatureSet::PsqThreats, false, true, HeadShape::Plain, 64, &[]);
        let bound = reference.variables()["transform.weight"].abs().max().double_value(&[]);
        let largest = rest.abs().max().double_value(&[]);
        assert!(largest <= bound * 1.001 && largest > bound * 0.9, "{largest} against psq_threats' {bound}");
    }

    #[test]
    #[should_panic(expected = "a halfk network is halfk_threats_screlu")]
    fn a_halfk_network_of_another_shape_is_refused() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let _ = Network::with_options(&vs.root(), FeatureSet::HalfKav2Hm, false, false, HeadShape::Plain, 8, &[]);
    }

    #[test]
    #[should_panic(expected = "a halfk network is halfk_threats_screlu")]
    fn a_halfk_network_with_one_hidden_layer_is_refused() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let _ = Network::with_options(&vs.root(), FeatureSet::HalfKav2Hm, false, true, HeadShape::Plain, 8, &[4]);
    }

    #[test]
    fn a_halfk_networks_hidden_layers_are_not_bounded() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let network = Network::with_options(&vs.root(), FeatureSet::HalfKav2Hm, false, true, HeadShape::Plain, 8, &[8, 4]);
        assert!(vs.variables().contains_key(FIRST_HIDDEN_WEIGHT));
        assert!(network.weight_bounds().is_empty(), "the engine quantizes each row on its own scale");
    }

    #[test]
    fn only_a_psq_networks_first_hidden_layer_is_bounded() {
        for feature_set in [FeatureSet::Psq, FeatureSet::PsqThreats] {
            let vs = nn::VarStore::new(tch::Device::Cpu);
            let network = Network::new(&vs.root(), feature_set, 8, &[4, 4]);
            let bounds = network.weight_bounds();
            assert_eq!(bounds, vec![(FIRST_HIDDEN_WEIGHT, FIRST_HIDDEN_WEIGHT_BOUND)]);
            assert!(vs.variables().contains_key(FIRST_HIDDEN_WEIGHT), "the name tch registers it under");

            let vs = nn::VarStore::new(tch::Device::Cpu);
            assert!(Network::new(&vs.root(), feature_set, 8, &[]).weight_bounds().is_empty(), "no hidden layer");
        }
    }

    /// A pairwise network's value, computed by hand from its parameters: each perspective's clipped
    /// transform, its halves multiplied, both into a linear head. For psq_threats too, its last row a
    /// threat's.
    #[test]
    fn a_pairwise_networks_value_is_its_halves_products_through_the_head() {
        for feature_set in [FeatureSet::Psq, FeatureSet::PsqThreats] {
            let width = if feature_set == FeatureSet::Psq { psq::ROWS } else { psq_threats::ROWS };
            let own_dense = dense_row(&[0, 53, width - 1], width);
            let opponent_dense = dense_row(&[7, 300], width);
            let label = Tensor::zeros([1, 1], (Kind::Float, tch::Device::Cpu));
            let dense = DenseBatchPsq::from_dense(&own_dense, &opponent_dense, label);
            let batch = if feature_set == FeatureSet::Psq { DenseBatch::Psq(dense) } else { DenseBatch::PsqThreats(dense) };

            let vs = nn::VarStore::new(tch::Device::Cpu);
            let network = Network::with_pairwise(&vs.root(), feature_set, true, 8, &[]);
            let variables = vs.variables();
            let (weight, bias) = (&variables["transform.weight"], &variables["transform.bias"]);
            let clipped = |dense: &Tensor| -> Tensor { (dense.matmul(&weight.tr()) + bias).clamp(0.0, 1.0) };
            let (own, opponent) = (clipped(&own_dense), clipped(&opponent_dense));
            let products = Tensor::cat(
                &[own.narrow(1, 0, 4) * own.narrow(1, 4, 4), opponent.narrow(1, 0, 4) * opponent.narrow(1, 4, 4)],
                1,
            );
            assert_eq!(variables["value_head.output.weight"].size(), [1, 8], "{feature_set:?}: the head reads embedding_dim values");
            let expected = products.matmul(&variables["value_head.output.weight"].tr()) + &variables["value_head.output.bias"];

            let got = tch::no_grad(|| network.pawns(&batch));
            assert!(got.allclose(&expected, 1e-5, 1e-6, false), "{feature_set:?}: {got:?} against {expected:?}");
        }
    }

    #[test]
    fn a_pairwise_networks_gradient_reaches_both_halves() {
        let width = psq::ROWS;
        let batch = DenseBatch::Psq(DenseBatchPsq::from_dense(
            &dense_row(&[0, 53, width - 1], width),
            &dense_row(&[7, 300], width),
            Tensor::from_slice(&[0.4f32]).view([1, 1]),
        ));
        let vs = nn::VarStore::new(tch::Device::Cpu);
        // Biases at 0.5 keep every lane inside the clamp, so no gradient is cut there by chance.
        let network = Network::with_pairwise(&vs.root(), FeatureSet::Psq, true, 8, &[]);
        tch::no_grad(|| {
            let _ = vs.variables()["transform.bias"].shallow_clone().fill_(0.5);
            let _ = vs.variables()["transform.weight"].shallow_clone().fill_(0.01);
        });
        network.training_loss(&batch, 0).backward();
        let gradient = vs.variables()["transform.weight"].grad();
        for (half, lanes) in [("first", 0..4), ("second", 4..8)] {
            let magnitude = f64::try_from(gradient.narrow(0, lanes.start, 4).abs().sum(Kind::Float)).unwrap();
            assert!(magnitude > 0.0, "no gradient reached the {half} half");
        }
    }

    /// A one-row psq_threats batch, its last row a threat's, and a screlu network of width 8 over it.
    fn screlu_network_and_batch() -> (nn::VarStore, Network, DenseBatch, Tensor, Tensor) {
        let width = psq_threats::ROWS;
        let own_dense = dense_row(&[0, 53, width - 1], width);
        let opponent_dense = dense_row(&[7, 300], width);
        let label = Tensor::zeros([1, 1], (Kind::Float, tch::Device::Cpu));
        let batch = DenseBatch::PsqThreats(DenseBatchPsq::from_dense(&own_dense, &opponent_dense, label));
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let network = Network::with_options(&vs.root(), FeatureSet::PsqThreats, false, true, HeadShape::Plain, 8, &[]);
        (vs, network, batch, own_dense, opponent_dense)
    }

    /// A screlu network's value, computed by hand from its parameters: each perspective's clipped
    /// transform squared, both into a linear head reading 16 values.
    #[test]
    fn a_screlu_networks_value_is_its_squares_through_the_head() {
        let (vs, network, batch, own_dense, opponent_dense) = screlu_network_and_batch();
        let variables = vs.variables();
        let (weight, bias) = (&variables["transform.weight"], &variables["transform.bias"]);
        let squared = |dense: &Tensor| -> Tensor { (dense.matmul(&weight.tr()) + bias).clamp(0.0, 1.0).square() };
        let squares = Tensor::cat(&[squared(&own_dense), squared(&opponent_dense)], 1);
        assert_eq!(variables["value_head.output.weight"].size(), [1, 16], "the head reads 2 * embedding_dim squares");
        let expected = squares.matmul(&variables["value_head.output.weight"].tr()) + &variables["value_head.output.bias"];
        let got = tch::no_grad(|| network.pawns(&batch));
        assert!(got.allclose(&expected, 1e-5, 1e-6, false), "{got:?} against {expected:?}");
    }

    /// Inside the clamp a lane's square passes back `2a` times its output weight, outside nothing.
    #[test]
    fn a_screlu_lanes_gradient_is_twice_its_value_inside_the_clamp() {
        let (vs, network, batch, own_dense, opponent_dense) = screlu_network_and_batch();
        let variables = vs.variables();
        tch::no_grad(|| {
            let _ = variables["transform.weight"].shallow_clone().fill_(0.01);
            // Lanes 0 to 5 inside the clamp, 6 below it, 7 above it.
            let biases = Tensor::from_slice(&[0.1f32, 0.2, 0.3, 0.4, 0.5, 0.6, -2.0, 3.0]);
            variables["transform.bias"].shallow_clone().copy_(&biases);
        });
        network.pawns(&batch).sum(Kind::Float).backward();
        let gradient = Vec::<f32>::try_from(&variables["transform.bias"].grad()).unwrap();
        let output = Vec::<f32>::try_from(&variables["value_head.output.weight"].view([-1])).unwrap();
        let lanes = |dense: &Tensor| -> Vec<f32> {
            let z = dense.matmul(&variables["transform.weight"].tr()) + &variables["transform.bias"];
            Vec::<f32>::try_from(&z.view([-1]).detach()).unwrap()
        };
        let (own, opponent) = (lanes(&own_dense), lanes(&opponent_dense));
        for lane in 0..8 {
            let inside = |z: f32| if (0.0..1.0).contains(&z) { 2.0 * z } else { 0.0 };
            let expected = inside(own[lane]) * output[lane] + inside(opponent[lane]) * output[8 + lane];
            assert!((gradient[lane] - expected).abs() < 1e-6, "lane {lane}: {} against {expected}", gradient[lane]);
        }
        assert_eq!((gradient[6], gradient[7]), (0.0, 0.0), "outside the clamp, no gradient");
    }

    /// A screlu hidden network's value, computed by hand: the squares through the hidden layer,
    /// clipped and squared beside it, then the squares, into the output. And the squares' gradient,
    /// with the transform's: the output's weights on the squares alone, the hidden layer's cut.
    #[test]
    fn a_screlu_hidden_networks_value_reads_the_squares_twice_and_learns_through_the_output_alone() {
        let width = psq_threats::ROWS;
        let own_dense = dense_row(&[0, 53, width - 1], width);
        let opponent_dense = dense_row(&[7, 300], width);
        let label = Tensor::zeros([1, 1], (Kind::Float, tch::Device::Cpu));
        let batch = DenseBatch::PsqThreats(DenseBatchPsq::from_dense(&own_dense, &opponent_dense, label));
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let network = Network::with_options(&vs.root(), FeatureSet::PsqThreats, false, true, HeadShape::Hidden, 8, &[4]);
        let variables = vs.variables();
        assert_eq!(variables["value_head.hidden0.weight"].size(), [4, 16]);
        assert_eq!(variables["value_head.output.weight"].size(), [1, 2 * 4 + 16]);
        tch::no_grad(|| {
            // Lanes inside the clamp, so the squares and their gradient show.
            let _ = variables["transform.weight"].shallow_clone().fill_(0.01);
            variables["transform.bias"].shallow_clone().copy_(&Tensor::from_slice(&[0.1f32, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8]));
            // The hidden layer inside its clamp too: at random, all four of its values could fall
            // outside it, and then none of its weights would have a gradient.
            let _ = variables["value_head.hidden0.weight"].shallow_clone().fill_(0.1);
            let _ = variables["value_head.hidden0.bias"].shallow_clone().fill_(0.1);
        });
        let (weight, bias) = (&variables["transform.weight"], &variables["transform.bias"]);
        let squared = |dense: &Tensor| -> Tensor { (dense.matmul(&weight.tr()) + bias).clamp(0.0, 1.0).square() };
        let squares = Tensor::cat(&[squared(&own_dense), squared(&opponent_dense)], 1);
        let values = (squares.matmul(&variables["value_head.hidden0.weight"].tr()) + &variables["value_head.hidden0.bias"]).clamp(0.0, 1.0);
        let inputs = Tensor::cat(&[values.shallow_clone(), values.square(), squares.shallow_clone()], 1);
        let expected = inputs.matmul(&variables["value_head.output.weight"].tr()) + &variables["value_head.output.bias"];
        let got = network.pawns(&batch);
        assert!(got.allclose(&expected.detach(), 1e-5, 1e-6, false), "{got:?} against {expected:?}");

        got.sum(Kind::Float).backward();
        // Each lane z inside the clamp passes back 2z times the output's weight on its square.
        let gradient = Vec::<f32>::try_from(&variables["transform.bias"].grad()).unwrap();
        let on_squares = Vec::<f32>::try_from(&variables["value_head.output.weight"].narrow(1, 8, 16).view([-1])).unwrap();
        let lanes = |dense: &Tensor| -> Vec<f32> { Vec::<f32>::try_from(&(dense.matmul(&weight.tr()) + bias).view([-1]).detach()).unwrap() };
        let (own, opponent) = (lanes(&own_dense), lanes(&opponent_dense));
        for lane in 0..8 {
            let expected = 2.0 * own[lane] * on_squares[lane] + 2.0 * opponent[lane] * on_squares[8 + lane];
            assert!((gradient[lane] - expected).abs() < 1e-6, "lane {lane}: {} against {expected}", gradient[lane]);
        }
        assert!(variables["value_head.hidden0.weight"].grad().abs().sum(Kind::Float).double_value(&[]) > 0.0, "the hidden layer learns");
    }

    /// A psq_threats_pairwise_hidden network's value by hand: each perspective's halves clipped and
    /// multiplied, own first, through the hidden layer, each value clipped and squared beside it, then
    /// the products, into the output. And the transform's gradient: the output's weights on the
    /// products alone, the hidden layer's cut.
    #[test]
    fn a_pairwise_hidden_networks_value_reads_the_products_twice_and_learns_through_the_output_alone() {
        let width = psq_threats::ROWS;
        let own_dense = dense_row(&[0, 53, width - 1], width);
        let opponent_dense = dense_row(&[7, 300], width);
        let label = Tensor::zeros([1, 1], (Kind::Float, tch::Device::Cpu));
        let batch = DenseBatch::PsqThreats(DenseBatchPsq::from_dense(&own_dense, &opponent_dense, label));
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let network = Network::with_options(&vs.root(), FeatureSet::PsqThreats, true, false, HeadShape::Hidden, 8, &[4]);
        let variables = vs.variables();
        assert_eq!(variables["value_head.hidden0.weight"].size(), [4, 8]);
        assert_eq!(variables["value_head.output.weight"].size(), [1, 2 * 4 + 8]);
        tch::no_grad(|| {
            // Lanes inside the clamp, so the products and their gradient show.
            let _ = variables["transform.weight"].shallow_clone().fill_(0.01);
            variables["transform.bias"].shallow_clone().copy_(&Tensor::from_slice(&[0.1f32, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8]));
            // The hidden layer inside its clamp too: at random, all four of its values could fall
            // outside it, and then none of its weights would have a gradient.
            let _ = variables["value_head.hidden0.weight"].shallow_clone().fill_(0.1);
            let _ = variables["value_head.hidden0.bias"].shallow_clone().fill_(0.1);
        });
        let (weight, bias) = (&variables["transform.weight"], &variables["transform.bias"]);
        let multiplied = |dense: &Tensor| -> Tensor {
            let lanes = (dense.matmul(&weight.tr()) + bias).clamp(0.0, 1.0);
            lanes.narrow(1, 0, 4) * lanes.narrow(1, 4, 4)
        };
        let products = Tensor::cat(&[multiplied(&own_dense), multiplied(&opponent_dense)], 1);
        let values = (products.matmul(&variables["value_head.hidden0.weight"].tr()) + &variables["value_head.hidden0.bias"]).clamp(0.0, 1.0);
        let inputs = Tensor::cat(&[values.shallow_clone(), values.square(), products.shallow_clone()], 1);
        let expected = inputs.matmul(&variables["value_head.output.weight"].tr()) + &variables["value_head.output.bias"];
        let got = network.pawns(&batch);
        assert!(got.allclose(&expected.detach(), 1e-5, 1e-6, false), "{got:?} against {expected:?}");

        got.sum(Kind::Float).backward();
        // Lane i of the first half passes back the output's weight on product i times lane i + 4,
        // and lane i + 4 the same weight times lane i, each perspective's own.
        let gradient = Vec::<f32>::try_from(&variables["transform.bias"].grad()).unwrap();
        let on_products = Vec::<f32>::try_from(&variables["value_head.output.weight"].narrow(1, 8, 8).view([-1])).unwrap();
        let lanes = |dense: &Tensor| -> Vec<f32> { Vec::<f32>::try_from(&(dense.matmul(&weight.tr()) + bias).view([-1]).detach()).unwrap() };
        let (own, opponent) = (lanes(&own_dense), lanes(&opponent_dense));
        for (lane, &got) in gradient.iter().enumerate() {
            let (product, partner) = (lane % 4, if lane < 4 { lane + 4 } else { lane - 4 });
            let expected = on_products[product] * own[partner] + on_products[4 + product] * opponent[partner];
            assert!((got - expected).abs() < 1e-6, "lane {lane}: {got} against {expected}");
        }
        assert!(variables["value_head.hidden0.weight"].grad().abs().sum(Kind::Float).double_value(&[]) > 0.0, "the hidden layer learns");
    }

    #[test]
    #[should_panic(expected = "a hidden-beside head is psq_threats screlu's or pairwise's")]
    fn a_hidden_beside_head_on_plain_psq_threats_is_refused() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let _ = Network::with_options(&vs.root(), FeatureSet::PsqThreats, false, false, HeadShape::Hidden, 8, &[4]);
    }

    #[test]
    #[should_panic(expected = "a screlu network is psq_threats or halfk with no hidden layer")]
    fn a_screlu_network_with_a_hidden_layer_is_refused() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let _ = Network::with_options(&vs.root(), FeatureSet::PsqThreats, false, true, HeadShape::Plain, 8, &[4]);
    }

    #[test]
    #[should_panic(expected = "a screlu network is psq_threats or halfk with no hidden layer")]
    fn a_screlu_psq_network_is_refused() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let _ = Network::with_options(&vs.root(), FeatureSet::Psq, false, true, HeadShape::Plain, 8, &[]);
    }

    #[test]
    fn a_non_pairwise_psq_network_reads_twice_its_embedding() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let _ = Network::new(&vs.root(), FeatureSet::Psq, 8, &[]);
        assert_eq!(vs.variables()["value_head.output.weight"].size(), [1, 16]);
    }

    #[test]
    #[should_panic(expected = "a bucketed head reads pairwise products")]
    fn a_stack_without_pairwise_products_is_refused() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let _ = Network::with_head(&vs.root(), FeatureSet::PsqThreats, false, HeadShape::Stack, 8, &[4, 4]);
    }

    #[test]
    #[should_panic(expected = "a bucketed head reads pairwise products")]
    fn a_material_head_without_pairwise_products_is_refused() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let _ = Network::with_head(&vs.root(), FeatureSet::PsqThreats, false, HeadShape::Material, 8, &[]);
    }

    #[test]
    #[should_panic(expected = "a material or phase head is psq_threats pairwise's or screlu's output")]
    fn a_material_head_over_psq_is_refused() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let _ = Network::with_head(&vs.root(), FeatureSet::Psq, true, HeadShape::Material, 8, &[]);
    }

    #[test]
    #[should_panic(expected = "a Phase head is screlu's output")]
    fn a_pairwise_phase_head_is_refused() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let _ = Network::with_head(&vs.root(), FeatureSet::PsqThreats, true, HeadShape::Phase, 8, &[]);
    }

    #[test]
    #[should_panic(expected = "a Count head is screlu's output")]
    fn a_pairwise_count_head_is_refused() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let _ = Network::with_head(&vs.root(), FeatureSet::PsqThreats, true, HeadShape::Count, 8, &[]);
    }

    #[test]
    #[should_panic(expected = "a Taper head is screlu's output")]
    fn a_pairwise_taper_head_is_refused() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let _ = Network::with_head(&vs.root(), FeatureSet::PsqThreats, true, HeadShape::Taper, 8, &[]);
    }

    #[test]
    #[should_panic(expected = "feature-set mismatch")]
    fn forward_panics_on_a_mismatched_network_and_batch() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let network = Network::new(&vs.root(), FeatureSet::Psq, 8, &[4]);
        let mismatched_batch = zero_batch(FeatureSet::PsqThreats, 1);

        let _ = network.forward(&mismatched_batch);
    }
}
