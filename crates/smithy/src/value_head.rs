//! The value head MLP shared by every architecture this project trains -- NNUE's kinds and the
//! transformer alike all end in the same shape: some number
//! of hidden layers reading whatever accumulator/embedding the rest of the network produced,
//! ending in a single raw logit output (see `forward`'s own doc comment for why this is a logit,
//! not a sigmoid-squashed probability). Previously hand-rolled once per NNUE struct (hardcoded
//! widths) and once more inside `transformer_network.rs` (configurable, but transformer-only) --
//! moved here once both became configurable, so there is exactly one implementation instead of
//! four near-identical copies.

use tch::nn;
use tch::Tensor;

/// The nonlinearity after each hidden layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activation {
    Relu,
    /// clamp(x, 0, 1). `psq`'s, so a network trained in float is the same function, up to
    /// rounding, as its later integer version, where the clamp is what keeps every value in range.
    ClippedRelu,
}

impl Activation {
    pub fn apply(&self, input: &Tensor) -> Tensor {
        match self {
            Activation::Relu => input.relu(),
            Activation::ClippedRelu => input.clamp(0.0, 1.0),
        }
    }
}

pub struct ValueHead {
    hidden_layers: Vec<nn::Linear>,
    output: nn::Linear,
    activation: Activation,
}

impl ValueHead {
    /// `hidden_dims` is one width per hidden layer, in order -- its own length is this value
    /// head's depth, so there is no separate `depth` field to keep in sync with it (an earlier
    /// version had both, and required every hidden layer to share one width; the list form allows
    /// layers of different widths and removes the redundancy). Empty is allowed: the output layer
    /// then reads `embedding_dim` directly.
    pub fn new(vs: &nn::Path, embedding_dim: i64, hidden_dims: &[i64], activation: Activation) -> ValueHead {
        let mut hidden_layers = Vec::with_capacity(hidden_dims.len());
        let mut input_dim = embedding_dim;
        for (i, &hidden_dim) in hidden_dims.iter().enumerate() {
            hidden_layers.push(nn::linear(vs / format!("hidden{i}"), input_dim, hidden_dim, Default::default()));
            input_dim = hidden_dim;
        }
        let output = nn::linear(vs / "output", input_dim, 1, Default::default());
        ValueHead { hidden_layers, output, activation }
    }

    #[cfg(test)]
    pub fn has_hidden_layer(&self) -> bool {
        !self.hidden_layers.is_empty()
    }

    /// `input`: `[batch, embedding_dim]` -- the caller's own accumulator or `[CLS]` row, already
    /// sliced/concatenated out. Returns `[batch, 1]`: a raw, unbounded real value, never squashed
    /// through a sigmoid or any other bounding function anywhere in this network. For the NNUE
    /// networks it is the evaluation in pawns: `nnue_network::Network` predicts `score` of it and
    /// trains it with the binary cross entropy of `sigma(pawns / S)` (`nnue_network::bce_loss`), which
    /// keeps the label a score rather than converting it to pawns, where the outcome blend would make
    /// it a heavy-tailed regression target. The transformer trains on the same loss.
    pub fn forward(&self, input: &Tensor) -> Tensor {
        let mut hidden = input.shallow_clone();
        for layer in &self.hidden_layers {
            hidden = self.activation.apply(&hidden.apply(layer));
        }
        hidden.apply(&self.output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_produces_one_raw_unbounded_value_per_row() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let value_head = ValueHead::new(&vs.root(), 16, &[8, 8], Activation::Relu);

        let input = Tensor::randn([3, 16], (tch::Kind::Float, tch::Device::Cpu));
        let output = value_head.forward(&input);

        assert_eq!(output.size(), [3, 1]);
        let values: Vec<f32> = output.reshape([3]).try_into().unwrap();
        for value in values {
            assert!(value.is_finite(), "expected a finite unbounded value, got {value}");
        }
    }

    #[test]
    fn works_with_a_single_hidden_layer_not_just_deeper() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let value_head = ValueHead::new(&vs.root(), 16, &[8], Activation::Relu);

        let input = Tensor::randn([2, 16], (tch::Kind::Float, tch::Device::Cpu));
        let output = value_head.forward(&input);

        assert_eq!(output.size(), [2, 1]);
    }

    #[test]
    fn works_with_hidden_layers_of_different_widths() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let value_head = ValueHead::new(&vs.root(), 16, &[12, 6, 3], Activation::Relu);

        let input = Tensor::randn([2, 16], (tch::Kind::Float, tch::Device::Cpu));
        let output = value_head.forward(&input);

        assert_eq!(output.size(), [2, 1]);
    }

    #[test]
    fn works_with_no_hidden_layer() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let value_head = ValueHead::new(&vs.root(), 16, &[], Activation::ClippedRelu);

        let input = Tensor::randn([2, 16], (tch::Kind::Float, tch::Device::Cpu));
        let output = value_head.forward(&input);

        assert_eq!(output.size(), [2, 1]);
    }

    #[test]
    fn clipped_relu_clamps_to_the_unit_interval() {
        let input = Tensor::from_slice(&[-2.0f32, 0.0, 0.25, 1.0, 3.0]);
        let output: Vec<f32> = Activation::ClippedRelu.apply(&input).try_into().unwrap();
        assert_eq!(output, vec![0.0, 0.0, 0.25, 1.0, 1.0]);
    }
}
