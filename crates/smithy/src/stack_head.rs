//! A `psq_stack` network's head: two hidden layers and the output, each one of several weight sets,
//! picked per position by its buckets (`temper_chess::stack_buckets`): the first layer by our king, the
//! second by their king, the output by the material.
//!
//! Trained factorised: each stage holds a shared weight and bias, trained by every position, and a
//! delta per bucket, trained only by its bucket's positions and starting at 0, so a rare bucket stays
//! close to the shared weights instead of learning noise. A bucket's weights are `shared +
//! delta[bucket]`; the export writes that sum, so the engine never sees the split.
//!
//! No weight matrix is built per row: a stage computes every bucket's delta output at once, one
//! matrix product against all the deltas side by side, and gathers each row's own. For the first
//! layer, 256 -> 16 over 64 buckets at a batch of 4096, that is about 1.1 G multiply-adds, little for
//! the GPU, and a `[batch, 64 * 16]` intermediate.

use tch::{nn, Kind, Tensor};

use temper_chess::pesto::PHASE_MAX;
use temper_chess::stack_buckets::{COUNT_BUCKETS, MATERIAL_BUCKETS, OUR_KING_BUCKETS, PHASE_BUCKETS, TAPER_SETS, THEIR_KING_BUCKETS};

use super::value_head::Activation;

/// The stages, in order, as their parameters are named under `value_head`.
pub const STAGES: [&str; 3] = ["l1", "l2", "output"];

/// Each stage's bucket count: by our king, by their king, by the material. A row's buckets come in
/// this order too (`DenseBatchPsq::buckets`), the phase and the piece count after them.
pub const BUCKETS: [i64; 3] = [OUR_KING_BUCKETS as i64, THEIR_KING_BUCKETS as i64, MATERIAL_BUCKETS as i64];

/// A psq network's head: one MLP (`value_head::ValueHead`), the stack's three bucketed stages, or
/// a single bucketed output by the material over the pairwise products or the squares
/// (`psq_threats_pairwise_material`, `psq_threats_screlu_material`), or by the game phase or the
/// piece count over the squares (`psq_threats_screlu_phase`, `psq_threats_screlu_count`), or two
/// output sets over the squares blended by the phase (`psq_threats_screlu_taper`, `taper`), or each
/// side's squares through a stage by its own king, then one plain output over those stages' values
/// and every square (`psq_threats_screlu_kings`, `StackHead::kings`), or one hidden layer over every
/// square or product, then one plain output over its values and every one of them
/// (`psq_threats_screlu_hidden`, `psq_threats_pairwise_hidden`, `hidden_output_input`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadShape {
    Plain,
    Stack,
    Material,
    Phase,
    Count,
    Taper,
    Kings,
    Hidden,
}

impl HeadShape {
    /// Whether the head reads a row's buckets, a stack's, a single output's, or a taper's phase, or
    /// is otherwise not `ValueHead`'s MLP (a screlu hidden head, which reads none).
    pub fn bucketed(self) -> bool {
        self != HeadShape::Plain
    }

    /// Whether the head is a single output layer's sets, bucketed by the material, the phase or the
    /// piece count, or a taper's two.
    pub fn output_only(self) -> bool {
        matches!(self, HeadShape::Material | HeadShape::Phase | HeadShape::Count | HeadShape::Taper)
    }

    /// Whether the head is one only screlu has: an output keyed by the phase, the piece count or a
    /// taper, or the king stages.
    pub fn screlu_only(self) -> bool {
        matches!(self, HeadShape::Phase | HeadShape::Count | HeadShape::Taper | HeadShape::Kings)
    }

    /// A single output's number of weight sets: its buckets, or a taper's two.
    pub fn output_buckets(self) -> usize {
        match self {
            HeadShape::Material => MATERIAL_BUCKETS,
            HeadShape::Phase => PHASE_BUCKETS,
            HeadShape::Count => COUNT_BUCKETS,
            HeadShape::Taper => TAPER_SETS,
            HeadShape::Plain | HeadShape::Stack | HeadShape::Kings | HeadShape::Hidden => {
                panic!("{self:?} is not a single bucketed output")
            }
        }
    }

    /// The bucketed stages, in order, each with its bucket column in a row's buckets
    /// (`DenseBatchPsq::buckets`): the stack's three, the material output alone, column 2, or the
    /// phase output alone, column 3, or the piece-count output alone, column 4, or the kings' two
    /// sides, our side's by column 0 and their side's by column 1 in their own frame
    /// (`their_own_frame`), then their plain output (`value_head.output`, not a stage).
    /// None for a taper, which is one plain layer of two outputs, not factorised, blended by
    /// `TAPER_PHASE_COLUMN`.
    pub fn stages(self) -> &'static [(&'static str, i64)] {
        match self {
            HeadShape::Plain | HeadShape::Taper | HeadShape::Hidden => &[],
            HeadShape::Stack => &[("l1", 0), ("l2", 1), ("output", 2)],
            HeadShape::Kings => &[("l1", 0), ("l2", 1)],
            HeadShape::Material => &[("output", 2)],
            HeadShape::Phase => &[("output", 3)],
            HeadShape::Count => &[("output", 4)],
        }
    }
}

/// The bucket column a taper reads its phase from: the phase output's (`HeadShape::Phase`).
pub const TAPER_PHASE_COLUMN: i64 = 3;

/// Their king's buckets (`Int64`) in their own frame, as the engine's `nnue::their_own_frame`: the
/// batch gives both kings from the side to move, and their side's stage reads their squares, which
/// are in theirs, the ranks flipped.
pub fn their_own_frame(their_king: &Tensor) -> Tensor {
    their_king.bitwise_xor(56i64)
}

/// What a kings head's two sides' stages read: the squares, detached, so their gradient never reaches
/// the transform, which learns from the output's direct reading of the squares alone, as a plain
/// screlu network's. Trained undetached, the stages pulled the shared features their way and the
/// network, though it holds screlu's own output, trailed screlu by 2% to 3% in training loss from
/// round 4 on (2026-10-26), most in the openings, where the king matters least. Training only: the
/// function computed is the same.
pub fn kings_stage_input(squares: &Tensor) -> Tensor {
    squares.detach()
}

/// A screlu hidden head's input to its output from its hidden layer's output, `[..., width]`, and
/// every square, `[..., 2 * embedding]`: the values clipped, then squared, then the squares, along
/// the last dimension, the engine's order (`nnue::hidden_beside_head`).
pub fn hidden_output_input(values: &Tensor, squares: &Tensor) -> Tensor {
    let clipped = values.clamp(0.0, 1.0);
    let squared = clipped.square();
    Tensor::cat(&[clipped, squared, squares.shallow_clone()], -1)
}

/// A kings head's input to its output from each side's stage output, `[..., width]`, and every
/// square, `[..., 2 * embedding]`: each side's values clipped, then squared, ours first, then the
/// squares, along the last dimension, the engine's order (`nnue::kings_head`).
pub fn kings_output_input(ours: &Tensor, theirs: &Tensor, squares: &Tensor) -> Tensor {
    let side = |values: &Tensor| {
        let clipped = values.clamp(0.0, 1.0);
        let squared = clipped.square();
        [clipped, squared]
    };
    let [ours_clipped, ours_squared] = side(ours);
    let [theirs_clipped, theirs_squared] = side(theirs);
    Tensor::cat(&[ours_clipped, ours_squared, theirs_clipped, theirs_squared, squares.shallow_clone()], -1)
}

/// A taper's two outputs, `[..., 2]`, middlegame then endgame, blended by `phase` (`Int64 [rows]`,
/// the batch's capped phase column, broadcast over any leading arm dimension):
/// `(phase * middlegame + (24 - phase) * endgame) / 24`, `[...]`, as `temper_chess::stack_buckets::taper`
/// computes it in the engine. The gradient reaches each set scaled by its weight, by autograd.
pub fn taper(outputs: &Tensor, phase: &Tensor) -> Tensor {
    let weight = phase.clamp_max(PHASE_MAX as i64).to_kind(Kind::Float) / PHASE_MAX as f64;
    let (middlegame, endgame) = (outputs.select(-1, 0), outputs.select(-1, 1));
    &endgame + (middlegame - &endgame) * weight
}

/// One stage's parameters. `Stage`, `StackHead` and `stage_forward` are the one-network head
/// (`nnue_network::Network`): built for its parameters, its forward pass the stacked one's test
/// reference, so outside the tests unread.
#[cfg_attr(not(test), allow(dead_code))]
struct Stage {
    shared: nn::Linear,
    /// `[buckets, out, in]`.
    delta_weight: Tensor,
    /// `[buckets, out]`.
    delta_bias: Tensor,
}

#[cfg_attr(not(test), allow(dead_code))]
pub struct StackHead {
    stages: Vec<Stage>,
    /// Each stage's bucket column (`HeadShape::stages`).
    columns: Vec<i64>,
    /// A kings head's output after its two sides' stages, one plain set for every position; `None`
    /// when the last stage is the output.
    output: Option<nn::Linear>,
}

#[cfg_attr(not(test), allow(dead_code))]
impl StackHead {
    /// The head on `input` values (the pairwise products of both perspectives), its two hidden layers
    /// `hidden_dims` wide. The parameters are `{stage}.shared.weight|bias` and
    /// `{stage}.delta.weight|bias` under `vs`, the deltas zero.
    pub fn new(vs: &nn::Path, input: i64, hidden_dims: &[i64]) -> StackHead {
        assert_eq!(hidden_dims.len(), 2, "a stack head has two hidden layers, not {hidden_dims:?}");
        let widths = [input, hidden_dims[0], hidden_dims[1], 1];
        let stages = (0..3).map(|stage| Stage::new(&(vs / STAGES[stage]), widths[stage], widths[stage + 1], BUCKETS[stage])).collect();
        StackHead { stages, columns: vec![0, 1, 2], output: None }
    }

    /// The kings head on `input` values (both perspectives' squares, own first): our side's squares
    /// through `l1`, by our king, their side's through `l2`, by their king in their own frame, each
    /// `hidden_dims`' one width, factorised as `new`'s; then one plain output layer over both sides'
    /// values clipped and squared and every square (`kings_output_input`), `output.weight|bias`
    /// under `vs`, with nothing to factorise.
    pub fn kings(vs: &nn::Path, input: i64, hidden_dims: &[i64]) -> StackHead {
        assert_eq!(hidden_dims.len(), 1, "a kings head has one hidden width, a side's, not {hidden_dims:?}");
        let width = hidden_dims[0];
        let stages = (0..2).map(|stage| Stage::new(&(vs / STAGES[stage]), input / 2, width, BUCKETS[stage])).collect();
        let output = nn::linear(vs / "output", 4 * width + input, 1, Default::default());
        StackHead { stages, columns: vec![0, 1], output: Some(output) }
    }

    /// A single bucketed output on `input` values (the pairwise products or the squares), `shape`
    /// `Material`, `Phase` or `Count`: one stage, `output`, from `input` to one value, a weight set per bucket,
    /// factorised as the stack's (`output.shared.weight|bias`, `output.delta.weight|bias` under `vs`,
    /// the deltas zero).
    pub fn output_only(vs: &nn::Path, input: i64, shape: HeadShape) -> StackHead {
        let path = vs / "output";
        let delta = &path / "delta";
        let buckets = shape.output_buckets() as i64;
        let stage = Stage {
            shared: nn::linear(&path / "shared", input, 1, Default::default()),
            delta_weight: delta.zeros("weight", &[buckets, 1, input]),
            delta_bias: delta.zeros("bias", &[buckets, 1]),
        };
        StackHead { stages: vec![stage], columns: vec![shape.stages()[0].1], output: None }
    }

    /// `input` `[batch, in]` through the stages, each row through its own buckets' weights
    /// (`buckets`, `Int64 [batch, 3]`, each stage reading its own column), clamped after each hidden
    /// layer: `[batch, 1]`, the evaluation in pawns, unbounded. A kings head's sides read their own
    /// half of `input` each, detached (`kings_stage_input`), then its plain output.
    pub fn forward(&self, input: &Tensor, buckets: &Tensor) -> Tensor {
        let stage = |index: usize, x: &Tensor, bucket: &Tensor| {
            let stage = &self.stages[index];
            let bias = stage.shared.bs.as_ref().expect("a linear layer with a bias");
            stage_forward(x, &stage.shared.ws, bias, &stage.delta_weight, &stage.delta_bias, bucket)
        };
        if let Some(output) = &self.output {
            let half = input.size()[1] / 2;
            let sides = kings_stage_input(input);
            let ours = stage(0, &sides.narrow(1, 0, half), &buckets.select(1, self.columns[0]));
            let theirs = stage(1, &sides.narrow(1, half, half), &their_own_frame(&buckets.select(1, self.columns[1])));
            return kings_output_input(&ours, &theirs, input).apply(output);
        }
        let mut x = input.shallow_clone();
        for index in 0..self.stages.len() {
            x = stage(index, &x, &buckets.select(1, self.columns[index]));
            if index + 1 < self.stages.len() {
                x = Activation::ClippedRelu.apply(&x);
            }
        }
        x
    }
}

impl Stage {
    /// A stage at `path`, `inputs` to `outputs`, one weight set per bucket of `buckets`: the shared
    /// part `shared.weight|bias`, `nn::linear`'s initialisation, and the deltas `delta.weight|bias`,
    /// zero.
    fn new(path: &nn::Path, inputs: i64, outputs: i64, buckets: i64) -> Stage {
        let delta = path / "delta";
        Stage {
            shared: nn::linear(path / "shared", inputs, outputs, Default::default()),
            delta_weight: delta.zeros("weight", &[buckets, outputs, inputs]),
            delta_bias: delta.zeros("bias", &[buckets, outputs]),
        }
    }
}

/// One stage for one network: `x` `[rows, in]` through `weight` `[out, in]` and `bias` `[out]`, plus
/// each row's delta, `delta_weight` `[buckets, out, in]` and `delta_bias` `[buckets, out]` at its
/// `bucket` (`Int64 [rows]`): `[rows, out]`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn stage_forward(x: &Tensor, weight: &Tensor, bias: &Tensor, delta_weight: &Tensor, delta_bias: &Tensor, bucket: &Tensor) -> Tensor {
    let (rows, inputs) = (x.size()[0], x.size()[1]);
    let (buckets, outputs) = (delta_weight.size()[0], delta_weight.size()[1]);
    let shared = x.matmul(&weight.tr()) + bias;
    let every_bucket = x.matmul(&delta_weight.view([buckets * outputs, inputs]).tr()).view([rows, buckets, outputs]);
    let index = bucket.view([rows, 1, 1]).expand([rows, 1, outputs], false);
    let delta = every_bucket.gather(1, &index, false).squeeze_dim(1);
    shared + delta + delta_bias.index_select(0, bucket)
}

/// `stage_forward` for every arm of a stack at once: `x` `[arms, rows, in]`, each parameter with a
/// leading arm dimension, `bucket` (`Int64 [rows]`) the same for every arm: `[arms, rows, out]`.
pub fn stacked_stage_forward(
    x: &Tensor, weight: &Tensor, bias: &Tensor, delta_weight: &Tensor, delta_bias: &Tensor, bucket: &Tensor,
) -> Tensor {
    let (arms, rows, inputs) = (x.size()[0], x.size()[1], x.size()[2]);
    let (buckets, outputs) = (delta_weight.size()[1], delta_weight.size()[2]);
    let shared = bias.unsqueeze(1).baddbmm(x, &weight.transpose(1, 2), 1.0, 1.0);
    let every_bucket = x.bmm(&delta_weight.view([arms, buckets * outputs, inputs]).transpose(1, 2)).view([arms, rows, buckets, outputs]);
    let index = bucket.view([1, rows, 1, 1]).expand([arms, rows, 1, outputs], false);
    let delta = every_bucket.gather(2, &index, false).squeeze_dim(2);
    shared + delta + delta_bias.index_select(1, bucket)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tch::{Device, Kind};

    fn head(input: i64) -> (nn::VarStore, StackHead) {
        let vs = nn::VarStore::new(Device::Cpu);
        let head = StackHead::new(&(vs.root() / "value_head"), input, &[4, 3]);
        (vs, head)
    }

    #[test]
    fn its_parameters_are_named_by_stage_and_shaped_by_buckets() {
        let (vs, _) = head(8);
        let variables = vs.variables();
        assert_eq!(variables["value_head.l1.shared.weight"].size(), [4, 8]);
        assert_eq!(variables["value_head.l1.delta.weight"].size(), [64, 4, 8]);
        assert_eq!(variables["value_head.l2.delta.bias"].size(), [64, 3]);
        assert_eq!(variables["value_head.output.delta.weight"].size(), [78, 1, 3]);
        assert_eq!(variables.len(), 12);
        for stage in STAGES {
            let delta = &variables[&format!("value_head.{stage}.delta.weight")];
            assert_eq!(delta.abs().sum(Kind::Float).double_value(&[]), 0.0, "{stage}'s deltas start at 0");
        }
    }

    /// One row through the three stages with its buckets' summed weights, by hand.
    fn by_hand(vs: &nn::VarStore, input: &Tensor, buckets: [i64; 3]) -> Tensor {
        let variables = vs.variables();
        let mut x = input.shallow_clone();
        for (index, stage) in STAGES.iter().enumerate() {
            let get = |name: &str| variables[&format!("value_head.{stage}.{name}")].shallow_clone();
            let weight = get("shared.weight") + get("delta.weight").get(buckets[index]);
            let bias = get("shared.bias") + get("delta.bias").get(buckets[index]);
            x = x.matmul(&weight.tr()) + bias;
            if index < 2 {
                x = x.clamp(0.0, 1.0);
            }
        }
        x
    }

    #[test]
    fn each_row_goes_through_its_own_buckets_summed_weights() {
        let (vs, head) = head(8);
        tch::no_grad(|| {
            for (_, variable) in vs.variables() {
                variable.shallow_clone().copy_(&(Tensor::randn_like(&variable) * 0.5));
            }
        });
        let input = Tensor::rand([3, 8], (Kind::Float, Device::Cpu));
        let rows: [[i64; 3]; 3] = [[0, 0, 0], [12, 63, 77], [12, 5, 40]];
        let buckets = Tensor::from_slice(&rows.concat()).view([3, 3]);
        let output = head.forward(&input, &buckets);
        assert_eq!(output.size(), [3, 1]);
        for (row, row_buckets) in rows.iter().enumerate() {
            let expected = by_hand(&vs, &input.narrow(0, row as i64, 1), *row_buckets);
            assert!(output.get(row as i64).allclose(&expected.view([1]), 1e-5, 1e-6, false), "row {row}");
        }
    }

    #[test]
    fn the_shared_weights_learn_from_every_row_and_a_delta_only_from_its_buckets() {
        let (vs, head) = head(8);
        let input = Tensor::rand([2, 8], (Kind::Float, Device::Cpu));
        // Both rows in our-king bucket 3; their-king buckets 1 and 2; material 10 and 20.
        let buckets = Tensor::from_slice(&[3i64, 1, 10, 3, 2, 20]).view([2, 3]);
        head.forward(&input, &buckets).sum(Kind::Float).backward();
        let variables = vs.variables();
        let grad = |name: &str| variables[&format!("value_head.{name}")].grad();
        assert!(grad("output.shared.bias").abs().sum(Kind::Float).double_value(&[]) > 0.0);
        let output_delta = grad("output.delta.bias");
        for bucket in 0..78 {
            let touched = output_delta.get(bucket).abs().sum(Kind::Float).double_value(&[]) > 0.0;
            assert_eq!(touched, bucket == 10 || bucket == 20, "material bucket {bucket}");
        }
    }

    #[test]
    fn a_kings_head_reads_each_side_by_its_own_king_then_the_squares_too_whatever_the_material() {
        let vs = nn::VarStore::new(Device::Cpu);
        let head = StackHead::kings(&(vs.root() / "value_head"), 8, &[3]);
        let variables = vs.variables();
        assert_eq!(variables["value_head.l1.delta.weight"].size(), [64, 3, 4]);
        assert_eq!(variables["value_head.l2.delta.weight"].size(), [64, 3, 4]);
        assert_eq!(variables["value_head.output.weight"].size(), [1, 4 * 3 + 8]);
        assert_eq!(variables.len(), 10);
        tch::no_grad(|| {
            for (_, variable) in vs.variables() {
                variable.shallow_clone().copy_(&(Tensor::randn_like(&variable) * 0.5));
            }
        });
        let input = Tensor::rand([3, 8], (Kind::Float, Device::Cpu));
        // The third column, the material, differs from the kings', so reading it anywhere shows.
        let rows: [[i64; 3]; 3] = [[12, 63, 5], [0, 7, 5], [12, 63, 77]];
        let buckets = Tensor::from_slice(&rows.concat()).view([3, 3]);
        let output = head.forward(&input, &buckets);
        let get = |name: &str| variables[&format!("value_head.{name}")].shallow_clone();
        for (row, row_buckets) in rows.iter().enumerate() {
            let x = input.narrow(0, row as i64, 1);
            // Written out by hand: ours by our king, theirs by their king rank-flipped.
            let side = |stage: &str, squares: &Tensor, bucket: i64| -> Vec<Tensor> {
                let weight = get(&format!("{stage}.shared.weight")) + get(&format!("{stage}.delta.weight")).get(bucket);
                let bias = get(&format!("{stage}.shared.bias")) + get(&format!("{stage}.delta.bias")).get(bucket);
                let clipped = (squares.matmul(&weight.tr()) + bias).clamp(0.0, 1.0);
                vec![clipped.shallow_clone(), &clipped * &clipped]
            };
            let mut parts = side("l1", &x.narrow(1, 0, 4), row_buckets[0]);
            parts.extend(side("l2", &x.narrow(1, 4, 4), row_buckets[1] ^ 56));
            parts.push(x.shallow_clone());
            let expected = Tensor::cat(&parts, 1).matmul(&get("output.weight").tr()) + get("output.bias");
            assert!(output.get(row as i64).allclose(&expected.view([1]), 1e-5, 1e-6, false), "row {row}");
        }
        let same_kings = input.narrow(0, 0, 1).repeat([2, 1]);
        let by_material = head.forward(&same_kings, &Tensor::from_slice(&[12i64, 63, 5, 12, 63, 77]).view([2, 3]));
        assert!(by_material.get(0).allclose(&by_material.get(1), 1e-6, 1e-7, false), "the material picks nothing");
    }

    #[test]
    fn a_kings_heads_squares_learn_from_the_output_alone_never_from_the_sides_stages() {
        // The output reads the squares linearly, so their gradient is exactly the output's weights on
        // them, row by row, whatever the stages: the stages' own gradient is cut.
        let vs = nn::VarStore::new(Device::Cpu);
        let head = StackHead::kings(&(vs.root() / "value_head"), 8, &[3]);
        tch::no_grad(|| {
            for (_, variable) in vs.variables() {
                variable.shallow_clone().copy_(&(Tensor::randn_like(&variable) * 0.5));
            }
            // `l1`'s values inside the clamp's open range, so its gradient flows whatever the draw:
            // with random weights, all six could land outside it and the last check fail by chance.
            let variables = vs.variables();
            for name in ["value_head.l1.shared.weight", "value_head.l1.delta.weight", "value_head.l1.delta.bias"] {
                let variable = &variables[name];
                variable.shallow_clone().copy_(&(variable * 0.01));
            }
            let _ = variables["value_head.l1.shared.bias"].shallow_clone().fill_(0.5);
        });
        let input = Tensor::rand([2, 8], (Kind::Float, Device::Cpu)).set_requires_grad(true);
        let buckets = Tensor::from_slice(&[12i64, 63, 5, 0, 7, 5]).view([2, 3]);
        head.forward(&input, &buckets).sum(Kind::Float).backward();
        let on_squares = vs.variables()["value_head.output.weight"].narrow(1, 4 * 3, 8);
        assert!(input.grad().allclose(&on_squares.expand([2, 8], false), 1e-6, 1e-7, false));
        // The stages still learn.
        assert!(vs.variables()["value_head.l1.shared.weight"].grad().abs().sum(Kind::Float).double_value(&[]) > 0.0);
    }

    #[test]
    fn a_material_head_is_one_output_stage_read_by_the_material_column() {
        let vs = nn::VarStore::new(Device::Cpu);
        let head = StackHead::output_only(&(vs.root() / "value_head"), 8, HeadShape::Material);
        let variables = vs.variables();
        assert_eq!(variables["value_head.output.shared.weight"].size(), [1, 8]);
        assert_eq!(variables["value_head.output.delta.weight"].size(), [78, 1, 8]);
        assert_eq!(variables.len(), 4);
        tch::no_grad(|| {
            for (_, variable) in vs.variables() {
                variable.shallow_clone().copy_(&(Tensor::randn_like(&variable) * 0.5));
            }
        });
        let input = Tensor::rand([2, 8], (Kind::Float, Device::Cpu));
        // The king columns differ from the material one, so reading the wrong column shows.
        let rows: [[i64; 3]; 2] = [[5, 9, 40], [60, 2, 7]];
        let buckets = Tensor::from_slice(&rows.concat()).view([2, 3]);
        let output = head.forward(&input, &buckets);
        let get = |name: &str| variables[&format!("value_head.output.{name}")].shallow_clone();
        for (row, row_buckets) in rows.iter().enumerate() {
            let weight = get("shared.weight") + get("delta.weight").get(row_buckets[2]);
            let bias = get("shared.bias") + get("delta.bias").get(row_buckets[2]);
            let expected = input.narrow(0, row as i64, 1).matmul(&weight.tr()) + bias;
            assert!(output.get(row as i64).allclose(&expected.view([1]), 1e-5, 1e-6, false), "row {row}");
        }
    }

    #[test]
    fn a_phase_head_is_one_output_stage_read_by_the_phase_column() {
        let vs = nn::VarStore::new(Device::Cpu);
        let head = StackHead::output_only(&(vs.root() / "value_head"), 8, HeadShape::Phase);
        let variables = vs.variables();
        assert_eq!(variables["value_head.output.delta.weight"].size(), [25, 1, 8]);
        assert_eq!(variables.len(), 4);
        tch::no_grad(|| {
            for (_, variable) in vs.variables() {
                variable.shallow_clone().copy_(&(Tensor::randn_like(&variable) * 0.5));
            }
        });
        let input = Tensor::rand([2, 8], (Kind::Float, Device::Cpu));
        // Every other column differs from the phase one, so reading the wrong column shows.
        let rows: [[i64; 4]; 2] = [[5, 9, 20, 3], [60, 2, 7, 24]];
        let buckets = Tensor::from_slice(&rows.concat()).view([2, 4]);
        let output = head.forward(&input, &buckets);
        let get = |name: &str| variables[&format!("value_head.output.{name}")].shallow_clone();
        for (row, row_buckets) in rows.iter().enumerate() {
            let weight = get("shared.weight") + get("delta.weight").get(row_buckets[3]);
            let bias = get("shared.bias") + get("delta.bias").get(row_buckets[3]);
            let expected = input.narrow(0, row as i64, 1).matmul(&weight.tr()) + bias;
            assert!(output.get(row as i64).allclose(&expected.view([1]), 1e-5, 1e-6, false), "row {row}");
        }
    }

    #[test]
    fn a_count_head_is_one_output_stage_read_by_the_count_column() {
        let vs = nn::VarStore::new(Device::Cpu);
        let head = StackHead::output_only(&(vs.root() / "value_head"), 8, HeadShape::Count);
        let variables = vs.variables();
        assert_eq!(variables["value_head.output.delta.weight"].size(), [8, 1, 8]);
        assert_eq!(variables.len(), 4);
        tch::no_grad(|| {
            for (_, variable) in vs.variables() {
                variable.shallow_clone().copy_(&(Tensor::randn_like(&variable) * 0.5));
            }
        });
        let input = Tensor::rand([2, 8], (Kind::Float, Device::Cpu));
        // Every other column differs from the count one, so reading the wrong column shows.
        let rows: [[i64; 5]; 2] = [[5, 9, 20, 3, 6], [60, 2, 7, 24, 1]];
        let buckets = Tensor::from_slice(&rows.concat()).view([2, 5]);
        let output = head.forward(&input, &buckets);
        let get = |name: &str| variables[&format!("value_head.output.{name}")].shallow_clone();
        for (row, row_buckets) in rows.iter().enumerate() {
            let weight = get("shared.weight") + get("delta.weight").get(row_buckets[4]);
            let bias = get("shared.bias") + get("delta.bias").get(row_buckets[4]);
            let expected = input.narrow(0, row as i64, 1).matmul(&weight.tr()) + bias;
            assert!(output.get(row as i64).allclose(&expected.view([1]), 1e-5, 1e-6, false), "row {row}");
        }
    }

    #[test]
    fn the_taper_is_the_middlegame_at_24_the_endgame_at_0_and_their_mean_at_12() {
        let outputs = Tensor::from_slice(&[3.0f32, -5.0, 3.0, -5.0, 3.0, -5.0, 3.0, -5.0]).view([4, 2]).set_requires_grad(true);
        let phase = Tensor::from_slice(&[24i64, 0, 12, 30]);
        let blended = taper(&outputs, &phase);
        let expected = [3.0f32, -5.0, -1.0, 3.0];
        assert_eq!(Vec::<f32>::try_from(&blended).unwrap(), expected, "the last capped at 24");
        // Each set's gradient is its weight in the blend.
        blended.sum(Kind::Float).backward();
        let grad = Vec::<f32>::try_from(&outputs.grad().view([-1])).unwrap();
        assert_eq!(grad, [1.0, 0.0, 0.0, 1.0, 0.5, 0.5, 1.0, 0.0]);
        // Over arms: `[arms, rows, 2]`, one phase per row.
        let arms = Tensor::stack(&[outputs.detach(), outputs.detach() * 2.0], 0);
        let doubled: Vec<f32> = expected.iter().map(|value| 2.0 * value).collect();
        assert_eq!(Vec::<f32>::try_from(&taper(&arms, &phase).get(1)).unwrap(), doubled);
    }

    #[test]
    fn every_arm_of_a_stacked_stage_computes_its_own_stage() {
        let (arms, rows, inputs, outputs, buckets) = (2, 5, 6, 4, 7);
        let options = (Kind::Float, Device::Cpu);
        let x = Tensor::randn([arms, rows, inputs], options);
        let weight = Tensor::randn([arms, outputs, inputs], options);
        let bias = Tensor::randn([arms, outputs], options);
        let delta_weight = Tensor::randn([arms, buckets, outputs, inputs], options);
        let delta_bias = Tensor::randn([arms, buckets, outputs], options);
        let bucket = Tensor::from_slice(&[0i64, 6, 3, 3, 1]);
        let stacked = stacked_stage_forward(&x, &weight, &bias, &delta_weight, &delta_bias, &bucket);
        for arm in 0..arms {
            let alone = stage_forward(&x.get(arm), &weight.get(arm), &bias.get(arm), &delta_weight.get(arm), &delta_bias.get(arm), &bucket);
            assert!(stacked.get(arm).allclose(&alone, 1e-5, 1e-6, false), "arm {arm}");
        }
    }
}
