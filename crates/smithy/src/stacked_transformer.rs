//! Every lambda of a lockstep transformer run as one network, each parameter one tensor with a
//! leading arm dimension, as `stacked_network.rs` stacks the NNUE's arms: a step launches its kernels
//! once for every arm instead of once per arm, and shares the batch's upload and embedding lookup.
//!
//! The function is `transformer_network::TransformerNetwork`'s, arm by arm: the same parameters under
//! the same names, the same blocks, the same dropout. Each arm's parameters reach only its own loss, so
//! the training objective is the *sum* of the arms' losses, whose gradient on arm `k`'s slice is
//! exactly the gradient of arm `k`'s loss alone.
//!
//! One shortcut the unstacked network does not take: the last block computes only `[CLS]`. Its keys
//! and values still come from every token, but its query, projection, feed-forward part and norms run
//! on `[CLS]` alone, the one row the value head reads, so the last block costs about one sequence
//! length less.

use std::collections::HashMap;

use tch::{nn, Device, Tensor};

use super::net_config::{Architecture, ArchitectureKind};
use super::stacked_network::{arm_labels, arm_test_losses, arm_training_losses, retained_index};
use super::transformer_batch::TransformerBatch;
use super::transformer_network::TransformerNetwork;

/// `nn::LayerNorm`'s default epsilon, which `TransformerNetwork`'s norms use.
const LAYER_NORM_EPS: f64 = 1e-5;

pub struct StackedTransformer {
    /// Every parameter, `[arms, ...]`, under the name an unstacked `TransformerNetwork`'s `VarStore`
    /// gives it.
    parameters: HashMap<String, Tensor>,
    arms: i64,
    num_layers: i64,
    num_heads: i64,
    hidden_layers: usize,
    dropout: f64,
}

impl StackedTransformer {
    /// `arms` arms of the transformer `architecture` describes, on `device`. Their weights are fresh
    /// initialisations, one per arm, built through an unstacked `TransformerNetwork` each and stacked,
    /// so names and shapes cannot drift from it; a caller loads every slice before training
    /// (`arm_slice::load_arm`, `arm_slice::fill_slice`).
    pub fn new(architecture: &Architecture, arms: i64, device: Device) -> StackedTransformer {
        let ArchitectureKind::Transformer { num_layers, num_heads, dropout, .. } = &architecture.kind else {
            panic!("StackedTransformer::new requires architecture.kind to be transformer");
        };
        assert!(arms >= 1, "a stack needs at least one arm");
        let stores: Vec<HashMap<String, Tensor>> = (0..arms)
            .map(|_| {
                let vs = nn::VarStore::new(device);
                let _ = TransformerNetwork::new(&vs.root(), architecture);
                vs.variables()
            })
            .collect();
        let parameters = stores[0]
            .keys()
            .map(|name| {
                let slices: Vec<&Tensor> = stores.iter().map(|arm| &arm[name]).collect();
                let stacked = tch::no_grad(|| Tensor::stack(&slices, 0)).set_requires_grad(true);
                (name.clone(), stacked)
            })
            .collect();
        StackedTransformer {
            parameters,
            arms,
            num_layers: *num_layers,
            num_heads: *num_heads,
            hidden_layers: architecture.value_head_hidden_dim().len(),
            dropout: *dropout,
        }
    }

    pub fn parameters(&self) -> &HashMap<String, Tensor> {
        &self.parameters
    }

    /// Keeps only the arms `keep` (strictly increasing indices into the current arms), in that order,
    /// as `StackedNetwork::retain` does. The optimizer is given the new tensors next
    /// (`StackedAdamW::retain`).
    pub fn retain(&mut self, keep: &[usize]) {
        let index = retained_index(keep, self.arms as usize, self.parameters.values().next().expect("parameters").device());
        for parameter in self.parameters.values_mut() {
            *parameter = tch::no_grad(|| parameter.index_select(0, &index)).set_requires_grad(true);
        }
        self.arms = keep.len() as i64;
    }

    fn parameter(&self, name: &str) -> &Tensor {
        self.parameters.get(name).unwrap_or_else(|| panic!("the stacked transformer has no {name}"))
    }

    /// Every arm's evaluation in pawns, `[arms, batch]`, as `TransformerNetwork::forward` is per arm.
    /// `train` turns every dropout on; the test pass and inference pass `false`.
    pub fn pawns(&self, batch: &TransformerBatch, train: bool) -> Tensor {
        let mut hidden = self.embed(&batch.tokens, train);
        let mask = batch.attention_mask.as_ref().map(|mask| mask.repeat([self.arms, 1, 1, 1]));
        for layer in 0..self.num_layers {
            let cls_only = layer + 1 == self.num_layers;
            hidden = self.block(&format!("block{layer}"), &hidden, mask.as_ref(), cls_only, train);
        }
        let mut cls = hidden.select(2, 0);
        for layer in 0..self.hidden_layers {
            cls = self.linear(&format!("value_head.hidden{layer}"), &cls).relu();
        }
        self.linear("value_head.output", &cls).squeeze_dim(2)
    }

    /// Token plus positional embedding for every arm, `[arms, batch, length, width]`. The tokens are
    /// the same for every arm, so the arms' tables side by side, `[vocab, arms * width]`, serve all
    /// of them in one lookup.
    fn embed(&self, tokens: &Tensor, train: bool) -> Tensor {
        let table = self.parameter("embeddings.token_embedding.weight");
        let (arms, vocab, width) = (table.size()[0], table.size()[1], table.size()[2]);
        let (batch, length) = tokens.size2().expect("tokens are [batch, length]");
        let side_by_side = table.permute([1, 0, 2]).reshape([vocab, arms * width]);
        let embedded = Tensor::embedding(&side_by_side, tokens, -1, false, false)
            .reshape([batch, length, arms, width])
            .permute([2, 0, 1, 3]);
        let positions = self.parameter("embeddings.positional_embedding").narrow(1, 0, length).unsqueeze(1);
        (embedded + positions).dropout(self.dropout, train)
    }

    /// One post-norm block for every arm, `TransformerBlock::forward`'s function. `hidden` is
    /// `[arms, batch, length, width]`; with `cls_only` the result holds `[CLS]` alone, length 1.
    fn block(&self, name: &str, hidden: &Tensor, mask: Option<&Tensor>, cls_only: bool, train: bool) -> Tensor {
        let size = hidden.size();
        let (arms, batch, length, width) = (size[0], size[1], size[2], size[3]);
        let heads = self.num_heads;
        let head_dim = width / heads;
        // Every arm and row together, `[arms * batch, heads, length, head_dim]`, the arms folded into
        // the batch so one fused attention kernel serves them all.
        let qkv = self
            .linear(&format!("{name}.attention.qkv"), &hidden.reshape([arms, batch * length, width]))
            .view([arms, batch, length, 3, heads, head_dim])
            .permute([3, 0, 1, 4, 2, 5])
            .reshape([3, arms * batch, heads, length, head_dim]);
        let (mut query, key, value) = (qkv.get(0), qkv.get(1), qkv.get(2));
        let mut mask = mask.map(|mask| mask.shallow_clone());
        let mut residual = hidden.shallow_clone();
        let query_length = if cls_only { 1 } else { length };
        if cls_only {
            query = query.narrow(2, 0, 1);
            mask = mask.map(|mask| mask.narrow(2, 0, 1));
            residual = hidden.narrow(2, 0, 1);
        }
        let attention_dropout = if train { self.dropout } else { 0.0 };
        let attended = Tensor::scaled_dot_product_attention(&query, &key, &value, mask.as_ref(), attention_dropout, false, None, false)
            .reshape([arms, batch, heads, query_length, head_dim])
            .permute([0, 1, 3, 2, 4])
            .reshape([arms, batch * query_length, width]);
        let attended = self.linear(&format!("{name}.attention.proj"), &attended).dropout(self.dropout, train);
        let hidden =
            self.layer_norm(&format!("{name}.norm1"), &(residual + attended.reshape([arms, batch, query_length, width])));
        let widened = self
            .linear(&format!("{name}.feed_forward.fc1"), &hidden.reshape([arms, batch * query_length, width]))
            .gelu("none");
        let fed_forward = self.linear(&format!("{name}.feed_forward.fc2"), &widened).dropout(self.dropout, train);
        self.layer_norm(&format!("{name}.norm2"), &(&hidden + fed_forward.reshape([arms, batch, query_length, width])))
    }

    /// Layer `name` for every arm at once: `[arms, rows, in]` to `[arms, rows, out]`.
    fn linear(&self, name: &str, input: &Tensor) -> Tensor {
        let weight = self.parameter(&format!("{name}.weight"));
        let bias = self.parameter(&format!("{name}.bias"));
        bias.unsqueeze(1).baddbmm(input, &weight.transpose(1, 2), 1.0, 1.0)
    }

    /// `LayerNorm` `name` for every arm over the width of `[arms, batch, length, width]`: normalised
    /// without an affine part, then each arm's own weight and bias.
    fn layer_norm(&self, name: &str, input: &Tensor) -> Tensor {
        let weight = self.parameter(&format!("{name}.weight"));
        let bias = self.parameter(&format!("{name}.bias"));
        let (arms, width) = (weight.size()[0], weight.size()[1]);
        let normalised = input.layer_norm([width], None::<Tensor>, None::<Tensor>, LAYER_NORM_EPS, false);
        normalised * weight.view([arms, 1, 1, width]) + bias.view([arms, 1, 1, width])
    }

    /// Every arm's training loss, `[arms]`, against label column `columns[k]` (`Int64`, on the
    /// network's device), dropout on: `stacked_network::arm_training_losses`, the NNUE stack's.
    pub fn training_losses(&self, batch: &TransformerBatch, columns: &Tensor) -> Tensor {
        arm_training_losses(&self.pawns(batch, true), &arm_labels(&batch.labels, columns))
    }

    /// Every arm's test loss, `[arms]`, dropout off: the binary cross entropy, the NNUE stack's.
    pub fn test_losses(&self, batch: &TransformerBatch, columns: &Tensor) -> Tensor {
        arm_test_losses(&self.pawns(batch, false), &arm_labels(&batch.labels, columns))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tch::Kind;
    use super::super::arm_slice;
    use super::super::cache::CacheRecord;
    use super::super::net_config::{Attention, ValueHeadConfig};
    use super::super::transformer_batch::to_transformer_batch;
    use temper_chess::board::Board;
    use temper_chess::features::extract_canonical;

    /// Same piece count, so one batch: 32 pieces each.
    const FENS: [&str; 3] = [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3",
        "rnbqkb1r/pppppppp/5n2/8/3P4/8/PPP1PPPP/RNBQKBNR w KQkq - 1 2",
    ];

    fn architecture(num_layers: i64, hidden: &[i64], dropout: f64) -> Architecture {
        Architecture {
            embedding_dim: Some(16),
            value_head: Some(ValueHeadConfig { hidden_dim: hidden.to_vec() }),
            kind: ArchitectureKind::Transformer { num_layers, num_heads: 4, feedforward_dim: 32, dropout },
        }
    }

    /// `FENS` with three label columns.
    fn batch(attention: Attention) -> TransformerBatch {
        let labels = [[0.1f32, -0.3, 0.6], [0.2, 0.5, -0.7], [-0.9, 0.0, 0.4]];
        let records: Vec<CacheRecord> = FENS
            .iter()
            .zip(labels)
            .map(|(fen, labels)| CacheRecord { canonical: extract_canonical(&Board::from_fen(fen).unwrap().0), labels: labels.to_vec(), progress: crate::cache::Progress::UNKNOWN })
            .collect();
        to_transformer_batch(&records, attention)
    }

    /// `arms` unstacked transformers, and a stack holding each one's weights in its slice.
    fn networks_and_stack(architecture: &Architecture, arms: i64) -> (Vec<(nn::VarStore, TransformerNetwork)>, StackedTransformer) {
        let networks: Vec<(nn::VarStore, TransformerNetwork)> = (0..arms)
            .map(|_| {
                let vs = nn::VarStore::new(Device::Cpu);
                let network = TransformerNetwork::new(&vs.root(), architecture);
                (vs, network)
            })
            .collect();
        let stack = StackedTransformer::new(architecture, arms, Device::Cpu);
        for (arm, (vs, _)) in networks.iter().enumerate() {
            for (name, value) in vs.variables() {
                arm_slice::fill_slice(&stack.parameters()[&name], arm as i64, &value, &name);
            }
        }
        (networks, stack)
    }

    fn close(stacked: &Tensor, unstacked: &Tensor, what: &str) {
        assert!(stacked.allclose(unstacked, 1e-4, 1e-5, false), "{what}: {stacked:?} against {unstacked:?}");
    }

    #[test]
    fn every_arm_computes_what_its_own_unstacked_transformer_does() {
        // The unstacked network runs its last block on every token, so this also checks that the
        // stack's last block on `[CLS]` alone gives the same `[CLS]`.
        for attention in [Attention::Full, Attention::KingsOnly] {
            for (layers, hidden) in [(1, &[][..]), (2, &[8][..]), (3, &[8, 4][..])] {
                let (networks, stack) = networks_and_stack(&architecture(layers, hidden, 0.0), 3);
                let batch = batch(attention);
                // Arm k reads label column 2 - k, so a mixed-up column cannot pass.
                let columns = Tensor::from_slice(&[2i64, 1, 0]);
                let pawns = stack.pawns(&batch, false);
                let test = stack.test_losses(&batch, &columns);
                for (arm, (_, network)) in networks.iter().enumerate() {
                    let what = format!("{attention:?} {layers} layers {hidden:?} arm {arm}");
                    let unstacked = network.forward(&batch, false).view([1, -1]);
                    close(&pawns.get(arm as i64), &unstacked.get(0), &format!("{what} pawns"));
                    let labels = batch.labels.select(1, 2 - arm as i64).view([1, -1]);
                    close(&test.get(arm as i64), &arm_test_losses(&unstacked, &labels).get(0), &format!("{what} test"));
                }
            }
        }
    }

    #[test]
    fn a_kings_only_stack_differs_from_a_full_one() {
        // The mask must reach every arm, not only the first.
        let (_, stack) = networks_and_stack(&architecture(2, &[8], 0.0), 2);
        let full = stack.pawns(&batch(Attention::Full), false);
        let kings_only = stack.pawns(&batch(Attention::KingsOnly), false);
        for arm in 0..2 {
            assert!(!full.get(arm).allclose(&kings_only.get(arm), 1e-6, 1e-6, false), "arm {arm}");
        }
    }

    #[test]
    fn the_summed_objective_gives_every_arm_its_own_gradient() {
        let (networks, stack) = networks_and_stack(&architecture(2, &[8], 0.0), 2);
        let batch = batch(Attention::Full);
        let columns = Tensor::from_slice(&[0i64, 2]);
        stack.training_losses(&batch, &columns).sum(Kind::Float).backward();
        for (arm, (vs, network)) in networks.iter().enumerate() {
            let labels = batch.labels.select(1, [0, 2][arm]).view([1, -1]);
            arm_training_losses(&network.forward(&batch, false).view([1, -1]), &labels).sum(Kind::Float).backward();
            for (name, value) in vs.variables() {
                let stacked_grad = stack.parameters()[&name].grad().get(arm as i64);
                close(&stacked_grad, &value.grad(), &format!("arm {arm} {name}"));
            }
        }
    }

    #[test]
    fn dropout_acts_while_training_only_and_independently_per_arm() {
        let (_, stack) = networks_and_stack(&architecture(2, &[8], 0.5), 2);
        // Both arms from the same weights: only dropout can tell them apart.
        for (name, value) in stack.parameters() {
            arm_slice::fill_slice(value, 1, &arm_slice::slice_of(value, 0), name);
        }
        let batch = batch(Attention::Full);

        let evaluated = [stack.pawns(&batch, false), stack.pawns(&batch, false)];
        let trained = [stack.pawns(&batch, true), stack.pawns(&batch, true)];

        close(&evaluated[0], &evaluated[1], "evaluation is deterministic");
        close(&evaluated[0].get(0), &evaluated[0].get(1), "equal weights evaluate equally");
        assert!(!trained[0].allclose(&trained[1], 1e-6, 1e-6, false), "training forwards differ between calls");
        assert!(!trained[0].get(0).allclose(&trained[0].get(1), 1e-6, 1e-6, false), "arms drop independently");
    }

    #[test]
    fn retain_keeps_the_named_arms_weights_in_order() {
        let (_, mut stack) = networks_and_stack(&architecture(1, &[], 0.0), 3);
        let batch = batch(Attention::Full);
        let before = stack.pawns(&batch, false);

        stack.retain(&[0, 2]);

        let after = stack.pawns(&batch, false);
        close(&after.get(0), &before.get(0), "arm 0");
        close(&after.get(1), &before.get(2), "arm 2");
    }
}
