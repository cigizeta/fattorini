//! Every arm of a lockstep generation as one network, each parameter one tensor with a leading arm
//! dimension.
//!
//! Lockstep shares each batch between the arms, but one network per arm still launches every kernel
//! of a step once per arm, from one thread. For psq at batch 4096 each kernel is tiny, so the time
//! went on the launches: betty's eleven arms took about 35 s of training a round, 3 ms per arm-step,
//! against a few seconds of arithmetic. Stacked, a step launches its kernels once for every arm.
//!
//! The function is `nnue_network::Network`'s, arm by arm: the same parameters under the same names,
//! the same activations, the same losses. Each arm's parameters reach only its own loss, so the
//! training objective is the *sum* of the arms' losses, whose gradient on arm `k`'s slice is exactly
//! the gradient of arm `k`'s loss alone.

use std::collections::HashMap;

use tch::{nn, Device, Kind, Reduction, Tensor};

use super::batch::{DenseBatch, DenseBatchPsq};
use super::feature_set::FeatureSet;
use super::net_config::Architecture;
use super::nnue_network::{pairwise_products, Network, FIRST_HIDDEN_WEIGHT, FIRST_HIDDEN_WEIGHT_BOUND};
use temper_chess::labels::S_PAWNS;
use super::stack_head::{hidden_output_input, kings_output_input, kings_stage_input, stacked_stage_forward, taper, their_own_frame, HeadShape, TAPER_PHASE_COLUMN};
use super::value_head::Activation;

pub struct StackedNetwork {
    feature_set: FeatureSet,
    /// Every parameter, `[arms, ...]`, under the name an unstacked `Network`'s `VarStore` gives it.
    parameters: HashMap<String, Tensor>,
    arms: i64,
    hidden_layers: usize,
    activation: Activation,
    /// Each perspective's activated transform split in two halves multiplied element-wise before
    /// the head, as `NetworkPsq::pawns` does.
    pairwise: bool,
    /// Each perspective's activated transform squared lane by lane before the head, as
    /// `NetworkPsq::pawns` does.
    screlu: bool,
    /// The head: plain, or bucketed per position (`stack_head`), a stack's or a material output's,
    /// as `NetworkPsq`'s.
    head: HeadShape,
}

impl StackedNetwork {
    /// `arms` arms of the network `architecture` describes over `feature_set`, on `device`. Their
    /// weights are fresh initialisations, one per arm; a caller loads every slice before training
    /// (`arm_slice::load_arm`).
    pub fn new(feature_set: FeatureSet, architecture: &Architecture, arms: i64, device: Device) -> StackedNetwork {
        StackedNetwork::with_options(
            feature_set, architecture.pairwise(), architecture.screlu(), architecture.head_shape(), architecture.embedding_dim(),
            architecture.value_head_hidden_dim(), arms, device,
        )
    }

    /// `new` from the two widths, as `Network::new` takes them.
    #[cfg(test)]
    pub fn with_shape(
        feature_set: FeatureSet, embedding_dim: i64, value_head_hidden_dims: &[i64], arms: i64, device: Device,
    ) -> StackedNetwork {
        StackedNetwork::with_head_shape(feature_set, false, HeadShape::Plain, embedding_dim, value_head_hidden_dims, arms, device)
    }

    /// `with_shape`, pairwise or not and with its head, as `Network::with_head` takes them. The
    /// parameters are an unstacked `Network`'s, built once per arm and stacked, so their names and
    /// shapes cannot drift from it.
    #[cfg(test)]
    pub fn with_head_shape(
        feature_set: FeatureSet, pairwise: bool, head: HeadShape, embedding_dim: i64, value_head_hidden_dims: &[i64], arms: i64,
        device: Device,
    ) -> StackedNetwork {
        StackedNetwork::with_options(feature_set, pairwise, false, head, embedding_dim, value_head_hidden_dims, arms, device)
    }

    /// `with_head_shape`, squaring each lane when `screlu`, as `Network::with_options` takes it.
    #[allow(clippy::too_many_arguments)]
    pub fn with_options(
        feature_set: FeatureSet, pairwise: bool, screlu: bool, head: HeadShape, embedding_dim: i64, value_head_hidden_dims: &[i64],
        arms: i64, device: Device,
    ) -> StackedNetwork {
        assert!(arms >= 1, "a stack needs at least one arm");
        let stores: Vec<HashMap<String, Tensor>> = (0..arms)
            .map(|_| {
                let vs = nn::VarStore::new(device);
                let _ = Network::with_options(&vs.root(), feature_set, pairwise, screlu, head, embedding_dim, value_head_hidden_dims);
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
        let activation = match feature_set {
            FeatureSet::Psq | FeatureSet::PsqThreats | FeatureSet::HalfKav2Hm => Activation::ClippedRelu,
        };
        StackedNetwork {
            feature_set, parameters, arms, hidden_layers: value_head_hidden_dims.len(), activation, pairwise, screlu, head,
        }
    }

    #[cfg(test)]
    pub fn arms(&self) -> i64 {
        self.arms
    }

    pub fn parameters(&self) -> &HashMap<String, Tensor> {
        &self.parameters
    }

    /// Keeps only the arms `keep` (strictly increasing indices into the current arms), in that order:
    /// an arm that finished leaves the stack, so a step costs what the arms still training cost. The
    /// parameters become new tensors; the optimizer is given them next (`StackedAdamW::retain`).
    pub fn retain(&mut self, keep: &[usize]) {
        let index = retained_index(keep, self.arms as usize, self.parameters.values().next().expect("parameters").device());
        for parameter in self.parameters.values_mut() {
            *parameter = tch::no_grad(|| parameter.index_select(0, &index)).set_requires_grad(true);
        }
        self.arms = keep.len() as i64;
    }

    /// `Network::weight_bounds`, which holds for every arm alike.
    pub fn weight_bounds(&self) -> Vec<(&'static str, f64)> {
        let psq = matches!(self.feature_set, FeatureSet::Psq | FeatureSet::PsqThreats);
        if psq && self.hidden_layers > 0 && !self.head.bucketed() {
            return vec![(FIRST_HIDDEN_WEIGHT, FIRST_HIDDEN_WEIGHT_BOUND)];
        }
        Vec::new()
    }

    fn parameter(&self, name: &str) -> &Tensor {
        self.parameters.get(name).unwrap_or_else(|| panic!("the stacked network has no {name}"))
    }

    /// One dense perspective through every arm's transform and activation: `[arms, batch,
    /// embedding]`, the reference `sparse_transform` is tested against.
    /// The input is the same for every arm, so this is one matmul against the arms' weights side by
    /// side, not a batched one.
    #[cfg(test)]
    fn transform(&self, input: &Tensor) -> Tensor {
        let weight = self.parameter("transform.weight");
        let bias = self.parameter("transform.bias");
        let (arms, embedding, width) = (weight.size()[0], weight.size()[1], weight.size()[2]);
        let rows = input.size()[0];
        let flat = bias.view([arms * embedding]).addmm(input, &weight.view([arms * embedding, width]).tr());
        self.activation.apply(&flat.view([rows, arms, embedding]).permute([1, 0, 2]))
    }

    /// Both perspectives of a psq-form batch through every arm's transform and activation, each
    /// `[arms, batch, embedding]`: what `transform` computes on the dense rows, as a sum of the
    /// active weight columns instead of a product mostly by zeros.
    ///
    /// The weight is `[arms, embedding, width]`; as a `[width, arms * embedding]` table, row
    /// `feature` holds every arm's column for it, so one `embedding_bag` serves every arm and both
    /// perspectives (the batch's bags are the own rows, then the opponent's). It fuses the gather
    /// and the sum, so the `[nnz, arms * embedding]` gather (1.7 GB for psq at batch 4096 and eleven
    /// arms) never exists. Its gradient is dense (`sparse = false`), zero on the rows no position
    /// touched, as the matmul's: the optimizer and the training are unchanged.
    fn sparse_transform(&self, batch: &DenseBatchPsq) -> (Tensor, Tensor) {
        let weight = self.parameter("transform.weight");
        let bias = self.parameter("transform.bias");
        let (arms, embedding, width) = (weight.size()[0], weight.size()[1], weight.size()[2]);
        let table = weight.view([arms * embedding, width]).tr().contiguous();
        let (sums, _, _, _) =
            Tensor::embedding_bag(&table, batch.features(), batch.offsets(), false, 0, false, None::<Tensor>, false);
        let flat = sums + bias.view([arms * embedding]);
        let both = self.activation.apply(&flat.view([2, batch.rows(), arms, embedding]).permute([0, 2, 1, 3]));
        (both.get(0), both.get(1))
    }

    /// Every arm's evaluation in pawns, from the side to move's view: `[arms, batch]`, unbounded, as
    /// `Network::pawns` is per arm.
    pub fn pawns(&self, batch: &DenseBatch) -> Tensor {
        assert_eq!(batch.feature_set(), self.feature_set, "StackedNetwork/DenseBatch feature-set mismatch");
        let batch = batch.psq();
        let (own, opponent) = self.sparse_transform(batch);
        self.head(own, opponent, Some(batch.buckets()))
    }

    /// `pawns` of a psq-form batch through the dense transform, the matmul `sparse_transform`
    /// replaces: the reference its tests compare against.
    #[cfg(test)]
    fn dense_pawns(&self, batch: &DenseBatchPsq) -> Tensor {
        let (own, opponent) = batch.dense();
        self.head(self.transform(&own), self.transform(&opponent), Some(batch.buckets()))
    }

    /// The value head on both perspectives' activated transforms, `[arms, batch, embedding]` each.
    /// `buckets` (`Int64 [batch, 5]`, a psq-form batch's) are read by a bucketed head only.
    fn head(&self, own: Tensor, opponent: Tensor, buckets: Option<&Tensor>) -> Tensor {
        let (own, opponent) = if self.pairwise {
            (pairwise_products(&own, 2), pairwise_products(&opponent, 2))
        } else if self.screlu {
            (own.square(), opponent.square())
        } else {
            (own, opponent)
        };
        if self.head == HeadShape::Kings {
            return self.kings_head(&own, &opponent, buckets.expect("a kings head reads a psq-form batch's kings"));
        }
        if self.head == HeadShape::Hidden {
            // The hidden layer over every square or product, detached, then the output over its
            // values and every one of them, as `NetworkPsq`'s hidden-beside head.
            let inputs = Tensor::cat(&[own, opponent], 2);
            let values = self.linear("value_head.hidden0", &kings_stage_input(&inputs));
            return self.linear("value_head.output", &hidden_output_input(&values, &inputs)).squeeze_dim(2);
        }
        let mut hidden = Tensor::cat(&[own, opponent], 2);
        if self.head == HeadShape::Taper {
            let buckets = buckets.expect("a taper reads a psq-form batch's phase");
            return taper(&self.linear("value_head.output", &hidden), &buckets.select(1, TAPER_PHASE_COLUMN));
        }
        if self.head.bucketed() {
            let buckets = buckets.expect("a bucketed head reads a psq-form batch's buckets");
            let stages = self.head.stages();
            for (index, (stage, column)) in stages.iter().enumerate() {
                hidden = self.stage(stage, &hidden, &buckets.select(1, *column));
                if index + 1 < stages.len() {
                    hidden = self.activation.apply(&hidden);
                }
            }
            return hidden.squeeze_dim(2);
        }
        for layer in 0..self.hidden_layers {
            hidden = self.activation.apply(&self.linear(&format!("value_head.hidden{layer}"), &hidden));
        }
        self.linear("value_head.output", &hidden).squeeze_dim(2)
    }

    /// The factorised stage `value_head.{stage}` for every arm at once, each row through its own
    /// `bucket`'s weights (`stack_head::stacked_stage_forward`).
    fn stage(&self, stage: &str, input: &Tensor, bucket: &Tensor) -> Tensor {
        let name = |part: &str| self.parameter(&format!("value_head.{stage}.{part}"));
        stacked_stage_forward(input, name("shared.weight"), name("shared.bias"), name("delta.weight"), name("delta.bias"), bucket)
    }

    /// A kings head for every arm at once (`stack_head::StackHead::kings`): each side's squares,
    /// `[arms, batch, embedding]`, detached (`stack_head::kings_stage_input`), through its stage by its
    /// own king, their king in their own frame, then the plain output over both sides' values clipped
    /// and squared and every square.
    fn kings_head(&self, own: &Tensor, opponent: &Tensor, buckets: &Tensor) -> Tensor {
        let ours = self.stage("l1", &kings_stage_input(own), &buckets.select(1, 0));
        let theirs = self.stage("l2", &kings_stage_input(opponent), &their_own_frame(&buckets.select(1, 1)));
        let input = kings_output_input(&ours, &theirs, &Tensor::cat(&[own, opponent], 2));
        self.linear("value_head.output", &input).squeeze_dim(2)
    }

    /// The value head's layer `name` for every arm at once: `[arms, batch, in]` to `[arms, batch, out]`.
    fn linear(&self, name: &str, input: &Tensor) -> Tensor {
        let weight = self.parameter(&format!("{name}.weight"));
        let bias = self.parameter(&format!("{name}.bias"));
        bias.unsqueeze(1).baddbmm(input, &weight.transpose(1, 2), 1.0, 1.0)
    }

    /// Every arm's training loss, `[arms]`: `nnue_network::bce_loss` per arm, against the label
    /// column `columns[k]` (an `Int64` tensor of `arms` entries, on the network's device). Its sum is
    /// what a step descends.
    pub fn training_losses(&self, batch: &DenseBatch, columns: &Tensor) -> Tensor {
        arm_training_losses(&self.pawns(batch), &arm_labels(batch.labels(), columns))
    }

    /// Every arm's test loss, `[arms]`: `Network::test_loss` per arm, the binary cross entropy.
    pub fn test_losses(&self, batch: &DenseBatch, columns: &Tensor) -> Tensor {
        arm_test_losses(&self.pawns(batch), &arm_labels(batch.labels(), columns))
    }
}

/// The labels of the arms, `[arms, batch]`: column `columns[k]` of a batch's `[batch, columns]`
/// labels for arm `k`.
pub fn arm_labels(labels: &Tensor, columns: &Tensor) -> Tensor {
    labels.index_select(1, columns).tr()
}

/// Every arm's training loss, `[arms]`, from its evaluations in pawns and its labels, both
/// `[arms, batch]`: `nnue_network::bce_loss` per arm, the binary cross entropy of `sigma(pawns / S)`
/// against `(label + 1) / 2`. Every stack trains on this one, so an NNUE's and a transformer's losses
/// cannot differ.
pub fn arm_training_losses(pawns: &Tensor, labels: &Tensor) -> Tensor {
    (pawns / S_PAWNS)
        .binary_cross_entropy_with_logits(&((labels + 1.0) / 2.0), None::<Tensor>, None::<Tensor>, Reduction::None)
        .mean_dim(1, false, Kind::Float)
}

/// Every arm's test loss, `[arms]`: the same binary cross entropy as `arm_training_losses`.
pub fn arm_test_losses(pawns: &Tensor, labels: &Tensor) -> Tensor {
    arm_training_losses(pawns, labels)
}

/// `keep` as an `Int64` index on `device`, after checking it names at least one of `arms` arms, each
/// once, in increasing order.
pub fn retained_index(keep: &[usize], arms: usize, device: Device) -> Tensor {
    assert!(!keep.is_empty(), "a stack keeps at least one arm");
    assert!(keep.windows(2).all(|pair| pair[0] < pair[1]), "arms to keep must be strictly increasing: {keep:?}");
    assert!(keep.iter().all(|&arm| arm < arms), "arms to keep {keep:?} out of {arms}");
    let index: Vec<i64> = keep.iter().map(|&arm| arm as i64).collect();
    Tensor::from_slice(&index).to_device(device)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::arm_slice;
    use super::super::batch::to_dense_batch;
    use super::super::cache::CacheRecord;
    use temper_chess::board::Board;
    use temper_chess::features::{extract_canonical, halfk};

    const FENS: [&str; 4] = [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3",
        "8/5k2/8/3P4/8/8/2K5/8 b - - 0 1",
        "r3k2r/ppp2ppp/2n1bn2/3pp3/3PP3/2N1BN2/PPP2PPP/R3K2R w KQkq d6 0 8",
    ];

    /// A batch of `FENS` with three label columns.
    fn batch(feature_set: FeatureSet) -> DenseBatch {
        let labels = [[0.1f32, -0.3, 0.6], [0.2, 0.5, -0.7], [-0.9, 0.0, 0.4], [0.3, 0.8, -0.2]];
        let records: Vec<CacheRecord> = FENS
            .iter()
            .zip(labels)
            .map(|(fen, labels)| CacheRecord {
                canonical: extract_canonical(&Board::from_fen(fen).unwrap().0),
                labels: labels.to_vec(),
                progress: crate::cache::Progress::UNKNOWN,
            })
            .collect();
        to_dense_batch(&records, feature_set)
    }

    /// `arms` unstacked networks, and a stack holding each one's weights in its slice.
    fn networks_and_stack(feature_set: FeatureSet, hidden: &[i64], arms: i64) -> (Vec<(nn::VarStore, Network)>, StackedNetwork) {
        pairwise_networks_and_stack(feature_set, false, hidden, arms)
    }

    /// `networks_and_stack`, pairwise or not.
    fn pairwise_networks_and_stack(
        feature_set: FeatureSet, pairwise: bool, hidden: &[i64], arms: i64,
    ) -> (Vec<(nn::VarStore, Network)>, StackedNetwork) {
        head_networks_and_stack(feature_set, pairwise, HeadShape::Plain, hidden, arms)
    }

    /// `networks_and_stack`, pairwise or not and with its head. A bucketed head's deltas are
    /// randomised, so that a row's buckets matter.
    fn head_networks_and_stack(
        feature_set: FeatureSet, pairwise: bool, head: HeadShape, hidden: &[i64], arms: i64,
    ) -> (Vec<(nn::VarStore, Network)>, StackedNetwork) {
        let networks: Vec<(nn::VarStore, Network)> = (0..arms)
            .map(|_| {
                let vs = nn::VarStore::new(Device::Cpu);
                let network = Network::with_head(&vs.root(), feature_set, pairwise, head, 8, hidden);
                if head.bucketed() {
                    tch::no_grad(|| {
                        for (name, variable) in vs.variables() {
                            if name.contains(".delta.") {
                                variable.shallow_clone().copy_(&(Tensor::randn_like(&variable) * 0.3));
                            }
                        }
                    });
                }
                (vs, network)
            })
            .collect();
        let stack = StackedNetwork::with_head_shape(feature_set, pairwise, head, 8, hidden, arms, Device::Cpu);
        for (arm, (vs, _)) in networks.iter().enumerate() {
            for (name, value) in vs.variables() {
                arm_slice::fill_slice(&stack.parameters()[&name], arm as i64, &value, &name);
            }
        }
        (networks, stack)
    }

    fn close(stacked: &Tensor, unstacked: &Tensor, what: &str) {
        assert!(stacked.allclose(unstacked, 1e-5, 1e-6, false), "{what}: {stacked:?} against {unstacked:?}");
    }

    #[test]
    fn every_arm_computes_what_its_own_unstacked_network_does() {
        let kinds = [FeatureSet::Psq, FeatureSet::PsqThreats];
        for feature_set in kinds {
            for hidden in [&[][..], &[4][..], &[6, 3][..]] {
                let (networks, stack) = networks_and_stack(feature_set, hidden, 3);
                let batch = batch(feature_set);
                // Arm k reads label column 2 - k, so a mixed-up column cannot pass.
                let columns = Tensor::from_slice(&[2i64, 1, 0]);
                let pawns = stack.pawns(&batch);
                let training = stack.training_losses(&batch, &columns);
                let test = stack.test_losses(&batch, &columns);
                for (arm, (_, network)) in networks.iter().enumerate() {
                    let what = format!("{feature_set:?} {hidden:?} arm {arm}");
                    let column = 2 - arm;
                    close(&pawns.get(arm as i64), &network.pawns(&batch).view([-1]), &format!("{what} pawns"));
                    close(&training.get(arm as i64), &network.training_loss(&batch, column), &format!("{what} training"));
                    close(&test.get(arm as i64), &network.test_loss(&batch, column), &format!("{what} test"));
                }
            }
        }
    }

    #[test]
    fn every_pairwise_arm_computes_what_its_own_unstacked_network_does() {
        for feature_set in [FeatureSet::Psq, FeatureSet::PsqThreats] {
            for hidden in [&[][..], &[4][..], &[6, 3][..]] {
                let (networks, stack) = pairwise_networks_and_stack(feature_set, true, hidden, 3);
                let batch = batch(feature_set);
                let columns = Tensor::from_slice(&[2i64, 1, 0]);
                let pawns = stack.pawns(&batch);
                let training = stack.training_losses(&batch, &columns);
                let test = stack.test_losses(&batch, &columns);
                for (arm, (_, network)) in networks.iter().enumerate() {
                    let what = format!("pairwise {feature_set:?} {hidden:?} arm {arm}");
                    let column = 2 - arm;
                    close(&pawns.get(arm as i64), &network.pawns(&batch).view([-1]), &format!("{what} pawns"));
                    close(&training.get(arm as i64), &network.training_loss(&batch, column), &format!("{what} training"));
                    close(&test.get(arm as i64), &network.test_loss(&batch, column), &format!("{what} test"));
                }
                close(
                    &pawns,
                    &stack.dense_pawns(psq_batch(&batch)),
                    &format!("pairwise {feature_set:?} {hidden:?} sparse against dense"),
                );
            }
        }
    }

    #[test]
    fn every_screlu_arm_computes_what_its_own_unstacked_network_does() {
        let feature_set = FeatureSet::PsqThreats;
        let networks: Vec<(nn::VarStore, Network)> = (0..3)
            .map(|_| {
                let vs = nn::VarStore::new(Device::Cpu);
                let network = Network::with_options(&vs.root(), feature_set, false, true, HeadShape::Plain, 8, &[]);
                (vs, network)
            })
            .collect();
        let stack = StackedNetwork::with_options(feature_set, false, true, HeadShape::Plain, 8, &[], 3, Device::Cpu);
        for (arm, (vs, _)) in networks.iter().enumerate() {
            for (name, value) in vs.variables() {
                arm_slice::fill_slice(&stack.parameters()[&name], arm as i64, &value, &name);
            }
        }
        let batch = batch(feature_set);
        let columns = Tensor::from_slice(&[2i64, 1, 0]);
        let pawns = stack.pawns(&batch);
        let training = stack.training_losses(&batch, &columns);
        let test = stack.test_losses(&batch, &columns);
        for (arm, (_, network)) in networks.iter().enumerate() {
            let what = format!("screlu arm {arm}");
            let column = 2 - arm;
            close(&pawns.get(arm as i64), &network.pawns(&batch).view([-1]), &format!("{what} pawns"));
            close(&training.get(arm as i64), &network.training_loss(&batch, column), &format!("{what} training"));
            close(&test.get(arm as i64), &network.test_loss(&batch, column), &format!("{what} test"));
        }
        close(&pawns, &stack.dense_pawns(psq_batch(&batch)), "screlu sparse against dense");
    }

    #[test]
    fn every_stack_arm_computes_what_its_own_unstacked_network_does() {
        for feature_set in [FeatureSet::Psq, FeatureSet::PsqThreats] {
            let (networks, stack) = head_networks_and_stack(feature_set, true, HeadShape::Stack, &[4, 3], 3);
            let batch = batch(feature_set);
            let columns = Tensor::from_slice(&[2i64, 1, 0]);
            let pawns = stack.pawns(&batch);
            let training = stack.training_losses(&batch, &columns);
            let test = stack.test_losses(&batch, &columns);
            for (arm, (_, network)) in networks.iter().enumerate() {
                let what = format!("{feature_set:?} stack arm {arm}");
                let column = 2 - arm;
                close(&pawns.get(arm as i64), &network.pawns(&batch).view([-1]), &format!("{what} pawns"));
                close(&training.get(arm as i64), &network.training_loss(&batch, column), &format!("{what} training"));
                close(&test.get(arm as i64), &network.test_loss(&batch, column), &format!("{what} test"));
            }
            close(&pawns, &stack.dense_pawns(psq_batch(&batch)), &format!("{feature_set:?} stack sparse against dense"));
            assert!(stack.weight_bounds().is_empty(), "{feature_set:?}: a stack's weights are not bounded");
        }
    }

    #[test]
    fn the_summed_objective_gives_every_stack_arm_its_own_gradient() {
        for feature_set in [FeatureSet::Psq, FeatureSet::PsqThreats] {
            let (networks, stack) = head_networks_and_stack(feature_set, true, HeadShape::Stack, &[4, 3], 2);
            let batch = batch(feature_set);
            let columns = Tensor::from_slice(&[0i64, 2]);
            stack.training_losses(&batch, &columns).sum(Kind::Float).backward();
            for (arm, (vs, network)) in networks.iter().enumerate() {
                network.training_loss(&batch, [0, 2][arm]).backward();
                for (name, value) in vs.variables() {
                    let stacked_grad = stack.parameters()[&name].grad().get(arm as i64);
                    close(&stacked_grad, &value.grad(), &format!("{feature_set:?} stack arm {arm} {name}"));
                }
            }
        }
    }

    #[test]
    fn every_material_arm_computes_what_its_own_unstacked_network_does_and_gets_its_own_gradient() {
        let (networks, stack) = head_networks_and_stack(FeatureSet::PsqThreats, true, HeadShape::Material, &[], 3);
        let batch = batch(FeatureSet::PsqThreats);
        let columns = Tensor::from_slice(&[2i64, 1, 0]);
        let pawns = stack.pawns(&batch);
        let test = stack.test_losses(&batch, &columns);
        for (arm, (_, network)) in networks.iter().enumerate() {
            let what = format!("material arm {arm}");
            close(&pawns.get(arm as i64), &network.pawns(&batch).view([-1]), &format!("{what} pawns"));
            close(&test.get(arm as i64), &network.test_loss(&batch, 2 - arm), &format!("{what} test"));
        }
        close(&pawns, &stack.dense_pawns(psq_batch(&batch)), "material sparse against dense");
        assert!(stack.weight_bounds().is_empty(), "a material head's weights are not bounded");

        stack.training_losses(&batch, &columns).sum(Kind::Float).backward();
        for (arm, (vs, network)) in networks.iter().enumerate() {
            network.training_loss(&batch, 2 - arm).backward();
            for (name, value) in vs.variables() {
                let stacked_grad = stack.parameters()[&name].grad().get(arm as i64);
                close(&stacked_grad, &value.grad(), &format!("material arm {arm} {name}"));
            }
        }
    }

    #[test]
    fn every_screlu_material_arm_computes_what_its_own_unstacked_network_does_and_gets_its_own_gradient() {
        // The deltas randomised per arm, so that each position's material bucket matters.
        let feature_set = FeatureSet::PsqThreats;
        let networks: Vec<(nn::VarStore, Network)> = (0..3)
            .map(|_| {
                let vs = nn::VarStore::new(Device::Cpu);
                let network = Network::with_options(&vs.root(), feature_set, false, true, HeadShape::Material, 8, &[]);
                tch::no_grad(|| {
                    for (name, variable) in vs.variables() {
                        if name.contains(".delta.") {
                            variable.shallow_clone().copy_(&(Tensor::randn_like(&variable) * 0.3));
                        }
                    }
                });
                (vs, network)
            })
            .collect();
        let stack = StackedNetwork::with_options(feature_set, false, true, HeadShape::Material, 8, &[], 3, Device::Cpu);
        for (arm, (vs, _)) in networks.iter().enumerate() {
            for (name, value) in vs.variables() {
                arm_slice::fill_slice(&stack.parameters()[&name], arm as i64, &value, &name);
            }
        }
        let batch = batch(feature_set);
        let columns = Tensor::from_slice(&[2i64, 1, 0]);
        let pawns = stack.pawns(&batch);
        let test = stack.test_losses(&batch, &columns);
        for (arm, (_, network)) in networks.iter().enumerate() {
            let what = format!("screlu material arm {arm}");
            close(&pawns.get(arm as i64), &network.pawns(&batch).view([-1]), &format!("{what} pawns"));
            close(&test.get(arm as i64), &network.test_loss(&batch, 2 - arm), &format!("{what} test"));
        }
        close(&pawns, &stack.dense_pawns(psq_batch(&batch)), "screlu material sparse against dense");
        assert!(stack.weight_bounds().is_empty(), "a material head's weights are not bounded");

        stack.training_losses(&batch, &columns).sum(Kind::Float).backward();
        for (arm, (vs, network)) in networks.iter().enumerate() {
            network.training_loss(&batch, 2 - arm).backward();
            for (name, value) in vs.variables() {
                let stacked_grad = stack.parameters()[&name].grad().get(arm as i64);
                close(&stacked_grad, &value.grad(), &format!("screlu material arm {arm} {name}"));
            }
        }
    }

    #[test]
    fn every_screlu_taper_arm_computes_what_its_own_unstacked_network_does_and_gets_its_own_gradient() {
        // `FENS` hold phases 24, 24, 0 and 14, so both ends and a blend are read. Each arm's output
        // is `nn::linear`'s own random initialisation, distinct per arm and per set.
        let feature_set = FeatureSet::PsqThreats;
        let networks: Vec<(nn::VarStore, Network)> = (0..3)
            .map(|_| {
                let vs = nn::VarStore::new(Device::Cpu);
                let network = Network::with_options(&vs.root(), feature_set, false, true, HeadShape::Taper, 8, &[]);
                (vs, network)
            })
            .collect();
        let stack = StackedNetwork::with_options(feature_set, false, true, HeadShape::Taper, 8, &[], 3, Device::Cpu);
        for (arm, (vs, _)) in networks.iter().enumerate() {
            for (name, value) in vs.variables() {
                arm_slice::fill_slice(&stack.parameters()[&name], arm as i64, &value, &name);
            }
        }
        assert_eq!(stack.parameters()["value_head.output.weight"].size(), [3, 2, 16]);
        let batch = batch(feature_set);
        let columns = Tensor::from_slice(&[2i64, 1, 0]);
        let pawns = stack.pawns(&batch);
        let test = stack.test_losses(&batch, &columns);
        for (arm, (_, network)) in networks.iter().enumerate() {
            let what = format!("screlu taper arm {arm}");
            close(&pawns.get(arm as i64), &network.pawns(&batch).view([-1]), &format!("{what} pawns"));
            close(&test.get(arm as i64), &network.test_loss(&batch, 2 - arm), &format!("{what} test"));
        }
        close(&pawns, &stack.dense_pawns(psq_batch(&batch)), "screlu taper sparse against dense");
        assert!(stack.weight_bounds().is_empty(), "a taper's weights are not bounded");

        stack.training_losses(&batch, &columns).sum(Kind::Float).backward();
        for (arm, (vs, network)) in networks.iter().enumerate() {
            network.training_loss(&batch, 2 - arm).backward();
            for (name, value) in vs.variables() {
                let stacked_grad = stack.parameters()[&name].grad().get(arm as i64);
                close(&stacked_grad, &value.grad(), &format!("screlu taper arm {arm} {name}"));
            }
        }
    }

    #[test]
    fn every_kings_arm_computes_what_its_own_unstacked_network_does_and_gets_its_own_gradient() {
        // The deltas randomised per arm, so that each position's kings matter; the plain output is
        // `nn::linear`'s own random initialisation, distinct per arm.
        let feature_set = FeatureSet::PsqThreats;
        let networks: Vec<(nn::VarStore, Network)> = (0..3)
            .map(|_| {
                let vs = nn::VarStore::new(Device::Cpu);
                let network = Network::with_options(&vs.root(), feature_set, false, true, HeadShape::Kings, 8, &[4]);
                tch::no_grad(|| {
                    for (name, variable) in vs.variables() {
                        if name.contains(".delta.") {
                            variable.shallow_clone().copy_(&(Tensor::randn_like(&variable) * 0.3));
                        }
                    }
                });
                (vs, network)
            })
            .collect();
        let stack = StackedNetwork::with_options(feature_set, false, true, HeadShape::Kings, 8, &[4], 3, Device::Cpu);
        for (arm, (vs, _)) in networks.iter().enumerate() {
            for (name, value) in vs.variables() {
                arm_slice::fill_slice(&stack.parameters()[&name], arm as i64, &value, &name);
            }
        }
        // Each side's stage reads its 8 squares; the output, both sides' 4 values clipped and
        // squared and all 16 squares.
        assert_eq!(stack.parameters()["value_head.l1.delta.weight"].size(), [3, 64, 4, 8]);
        assert_eq!(stack.parameters()["value_head.l2.delta.weight"].size(), [3, 64, 4, 8]);
        assert_eq!(stack.parameters()["value_head.output.weight"].size(), [3, 1, 32]);
        let batch = batch(feature_set);
        let columns = Tensor::from_slice(&[2i64, 1, 0]);
        let pawns = stack.pawns(&batch);
        let test = stack.test_losses(&batch, &columns);
        for (arm, (_, network)) in networks.iter().enumerate() {
            let what = format!("screlu kings arm {arm}");
            close(&pawns.get(arm as i64), &network.pawns(&batch).view([-1]), &format!("{what} pawns"));
            close(&test.get(arm as i64), &network.test_loss(&batch, 2 - arm), &format!("{what} test"));
        }
        close(&pawns, &stack.dense_pawns(psq_batch(&batch)), "screlu kings sparse against dense");
        assert!(stack.weight_bounds().is_empty(), "a kings head's weights are not bounded");

        stack.training_losses(&batch, &columns).sum(Kind::Float).backward();
        for (arm, (vs, network)) in networks.iter().enumerate() {
            network.training_loss(&batch, 2 - arm).backward();
            for (name, value) in vs.variables() {
                let stacked_grad = stack.parameters()[&name].grad().get(arm as i64);
                close(&stacked_grad, &value.grad(), &format!("screlu kings arm {arm} {name}"));
            }
        }
    }

    #[test]
    fn every_screlu_or_pairwise_hidden_arm_computes_what_its_own_unstacked_network_does_and_gets_its_own_gradient() {
        // Each arm `nn::linear`'s own random initialisation, distinct per arm. The hidden layer reads
        // all 16 squares, or all 8 products; the output, its 4 values clipped and squared and every
        // square or product.
        let feature_set = FeatureSet::PsqThreats;
        for (kind, pairwise, inputs) in [("screlu", false, 16), ("pairwise", true, 8)] {
            let screlu = !pairwise;
            let networks: Vec<(nn::VarStore, Network)> = (0..3)
                .map(|_| {
                    let vs = nn::VarStore::new(Device::Cpu);
                    let network = Network::with_options(&vs.root(), feature_set, pairwise, screlu, HeadShape::Hidden, 8, &[4]);
                    (vs, network)
                })
                .collect();
            let stack = StackedNetwork::with_options(feature_set, pairwise, screlu, HeadShape::Hidden, 8, &[4], 3, Device::Cpu);
            for (arm, (vs, _)) in networks.iter().enumerate() {
                for (name, value) in vs.variables() {
                    arm_slice::fill_slice(&stack.parameters()[&name], arm as i64, &value, &name);
                }
            }
            assert_eq!(stack.parameters()["value_head.hidden0.weight"].size(), [3, 4, inputs]);
            assert_eq!(stack.parameters()["value_head.output.weight"].size(), [3, 1, 8 + inputs]);
            let batch = batch(feature_set);
            let columns = Tensor::from_slice(&[2i64, 1, 0]);
            let pawns = stack.pawns(&batch);
            let test = stack.test_losses(&batch, &columns);
            for (arm, (_, network)) in networks.iter().enumerate() {
                let what = format!("{kind} hidden arm {arm}");
                close(&pawns.get(arm as i64), &network.pawns(&batch).view([-1]), &format!("{what} pawns"));
                close(&test.get(arm as i64), &network.test_loss(&batch, 2 - arm), &format!("{what} test"));
            }
            close(&pawns, &stack.dense_pawns(psq_batch(&batch)), &format!("{kind} hidden sparse against dense"));
            assert!(stack.weight_bounds().is_empty(), "a {kind} hidden head's weights are not bounded");

            stack.training_losses(&batch, &columns).sum(Kind::Float).backward();
            for (arm, (vs, network)) in networks.iter().enumerate() {
                network.training_loss(&batch, 2 - arm).backward();
                for (name, value) in vs.variables() {
                    let stacked_grad = stack.parameters()[&name].grad().get(arm as i64);
                    close(&stacked_grad, &value.grad(), &format!("{kind} hidden arm {arm} {name}"));
                }
            }
        }
    }

    #[test]
    fn every_halfk_arm_computes_what_its_own_unstacked_network_does_and_gets_its_own_gradient() {
        // halfk_threats_screlu's output alone, and halfk_threats_screlu_hidden's two hidden layers,
        // trained end to end: the transform's gradient reaches every arm through them. The piece rows
        // start at zero, so they are drawn here, for a gradient that depends on them. The hidden
        // biases at 0.5 and their weights within 0.02, so that every unit stays inside its clip
        // (16 inputs of at most 1 move it by at most 0.32): a random draw can leave every unit of a
        // layer clipped on every position, and no gradient past it.
        let feature_set = FeatureSet::HalfKav2Hm;
        for (pairwise, hidden) in [(false, &[][..]), (false, &[8, 4][..]), (true, &[8, 4][..])] {
            let networks: Vec<(nn::VarStore, Network)> = (0..3)
                .map(|_| {
                    let vs = nn::VarStore::new(Device::Cpu);
                    let network = Network::with_options(&vs.root(), feature_set, pairwise, !pairwise, HeadShape::Plain, 8, hidden);
                    tch::no_grad(|| {
                        let variables = vs.variables();
                        let mut pieces = variables["transform.weight"].narrow(1, 0, halfk::HALFK_ROWS as i64);
                        let _ = pieces.uniform_(-0.05, 0.05);
                        for (name, variable) in &variables {
                            if name.starts_with("value_head.hidden") && name.ends_with(".bias") {
                                let _ = variable.shallow_clone().fill_(0.5);
                            } else if name.starts_with("value_head.hidden") {
                                let _ = variable.shallow_clone().uniform_(-0.02, 0.02);
                            }
                        }
                    });
                    (vs, network)
                })
                .collect();
            let stack = StackedNetwork::with_options(feature_set, pairwise, !pairwise, HeadShape::Plain, 8, hidden, 3, Device::Cpu);
            if pairwise {
                assert_eq!(stack.parameters()["value_head.hidden0.weight"].size(), [3, 8, 8], "the products, half as many");
            }
            for (arm, (vs, _)) in networks.iter().enumerate() {
                for (name, value) in vs.variables() {
                    arm_slice::fill_slice(&stack.parameters()[&name], arm as i64, &value, &name);
                }
            }
            let batch = batch(feature_set);
            let columns = Tensor::from_slice(&[2i64, 1, 0]);
            let pawns = stack.pawns(&batch);
            for (arm, (_, network)) in networks.iter().enumerate() {
                close(&pawns.get(arm as i64), &network.pawns(&batch).view([-1]), &format!("halfk {hidden:?} arm {arm} pawns"));
            }
            close(&pawns, &stack.dense_pawns(psq_batch(&batch)), &format!("halfk {hidden:?} sparse against dense"));
            assert!(stack.weight_bounds().is_empty(), "a halfk network's weights are not bounded");

            stack.training_losses(&batch, &columns).sum(Kind::Float).backward();
            for (arm, (vs, network)) in networks.iter().enumerate() {
                network.training_loss(&batch, 2 - arm).backward();
                for (name, value) in vs.variables() {
                    let stacked_grad = stack.parameters()[&name].grad().get(arm as i64);
                    close(&stacked_grad, &value.grad(), &format!("halfk {hidden:?} arm {arm} {name}"));
                }
                let transform = vs.variables()["transform.weight"].grad().abs().sum(Kind::Float).double_value(&[]);
                assert!(transform > 0.0, "halfk {hidden:?} arm {arm}: the transform learns through the head");
            }
        }
    }

    #[test]
    fn the_summed_objective_gives_every_pairwise_arm_its_own_gradient() {
        let (networks, stack) = pairwise_networks_and_stack(FeatureSet::Psq, true, &[], 2);
        let batch = batch(FeatureSet::Psq);
        let columns = Tensor::from_slice(&[0i64, 2]);
        stack.training_losses(&batch, &columns).sum(Kind::Float).backward();
        for (arm, (vs, network)) in networks.iter().enumerate() {
            network.training_loss(&batch, [0, 2][arm]).backward();
            for (name, value) in vs.variables() {
                let stacked_grad = stack.parameters()[&name].grad().get(arm as i64);
                close(&stacked_grad, &value.grad(), &format!("pairwise arm {arm} {name}"));
            }
        }
    }

    /// The psq-form batch inside `batch(feature_set)`.
    fn psq_batch(batch: &DenseBatch) -> &DenseBatchPsq {
        batch.psq()
    }

    #[test]
    fn the_sparse_transform_computes_what_the_dense_one_does() {
        for feature_set in [FeatureSet::Psq, FeatureSet::PsqThreats] {
            for hidden in [&[][..], &[4][..]] {
                let stack = StackedNetwork::with_shape(feature_set, 8, hidden, 3, Device::Cpu);
                let batch = batch(feature_set);
                let dense = stack.dense_pawns(psq_batch(&batch));
                close(&stack.pawns(&batch), &dense, &format!("{feature_set:?} {hidden:?}"));
            }
        }
    }

    #[test]
    fn the_sparse_transform_on_cuda_computes_what_it_does_on_the_cpu() {
        if !tch::Cuda::is_available() {
            return;
        }
        for feature_set in [FeatureSet::Psq, FeatureSet::PsqThreats] {
            let cpu = StackedNetwork::with_shape(feature_set, 8, &[4], 3, Device::Cpu);
            let cuda = StackedNetwork::with_shape(feature_set, 8, &[4], 3, Device::Cuda(0));
            for (name, value) in cpu.parameters() {
                tch::no_grad(|| cuda.parameters()[name].shallow_clone().copy_(value));
            }
            let on_cuda = cuda.pawns(&batch(feature_set).to_device(Device::Cuda(0))).to_device(Device::Cpu);
            close(&on_cuda, &cpu.pawns(&batch(feature_set)), &format!("{feature_set:?}"));
        }
    }

    #[test]
    fn sparse_and_dense_stay_together_over_optimizer_steps() {
        // Two stacks from the same weights, one trained through the sparse transform and one
        // through the dense matmul, same batches, same optimizer: the training is the same, not
        // only one step. Most rows are touched by no position, so their gradient is zero and
        // only weight decay and the moments move them, on both sides alike.
        use super::super::stacked_adamw::StackedAdamW;
        for feature_set in [FeatureSet::Psq, FeatureSet::PsqThreats] {
            let sparse = StackedNetwork::with_shape(feature_set, 8, &[4], 2, Device::Cpu);
            let dense = StackedNetwork::with_shape(feature_set, 8, &[4], 2, Device::Cpu);
            for (name, value) in sparse.parameters() {
                tch::no_grad(|| dense.parameters()[name].shallow_clone().copy_(value));
            }
            let mut sparse_optimizer = StackedAdamW::new(sparse.parameters(), 1e-2);
            let mut dense_optimizer = StackedAdamW::new(dense.parameters(), 1e-2);
            let batch = batch(feature_set);
            let labels = arm_labels(batch.labels(), &Tensor::from_slice(&[0i64, 2]));
            for _ in 0..5 {
                sparse_optimizer.zero_grad();
                (sparse.pawns(&batch) - &labels).square().mean(Kind::Float).backward();
                sparse_optimizer.step();
                dense_optimizer.zero_grad();
                (dense.dense_pawns(psq_batch(&batch)) - &labels).square().mean(Kind::Float).backward();
                dense_optimizer.step();
            }
            // Looser than `close`: Adam divides each moment by the root of the second, so an element
            // whose gradient is tiny turns the two sums' rounding (sparse against dense, about 1e-7)
            // into a visible share of its step; a wrong training would differ by the steps themselves,
            // up to 5e-2 here.
            for (name, value) in sparse.parameters() {
                let what = format!("{feature_set:?} {name} after 5 steps");
                assert!(value.allclose(&dense.parameters()[name], 1e-4, 1e-4, false), "{what}: {value:?} against {:?}", dense.parameters()[name]);
            }
        }
    }

    #[test]
    fn the_summed_objective_gives_every_arm_its_own_gradient() {
        // The unstacked network multiplies dense rows, so for psq and psq_threats this also checks
        // the sparse transform's gradient, zero on every row no position touches.
        for feature_set in [FeatureSet::Psq, FeatureSet::PsqThreats] {
            let (networks, stack) = networks_and_stack(feature_set, &[4], 2);
            let batch = batch(feature_set);
            let columns = Tensor::from_slice(&[0i64, 2]);
            stack.training_losses(&batch, &columns).sum(Kind::Float).backward();
            for (arm, (vs, network)) in networks.iter().enumerate() {
                network.training_loss(&batch, [0, 2][arm]).backward();
                for (name, value) in vs.variables() {
                    let stacked_grad = stack.parameters()[&name].grad().get(arm as i64);
                    close(&stacked_grad, &value.grad(), &format!("{feature_set:?} arm {arm} {name}"));
                }
            }
        }
    }

    #[test]
    fn its_parameters_carry_an_unstacked_networks_names() {
        let vs = nn::VarStore::new(Device::Cpu);
        let _ = Network::new(&vs.root(), FeatureSet::Psq, 8, &[4]);
        let stack = StackedNetwork::with_shape(FeatureSet::Psq, 8, &[4], 5, Device::Cpu);
        let unstacked = vs.variables();
        let mut names: Vec<&String> = stack.parameters().keys().collect();
        let mut expected: Vec<&String> = unstacked.keys().collect();
        names.sort();
        expected.sort();
        assert_eq!(names, expected);
        for (name, tensor) in stack.parameters() {
            let mut shape = vec![5];
            shape.extend(unstacked[name].size());
            assert_eq!(tensor.size(), shape, "{name}");
            assert!(tensor.requires_grad(), "{name}");
        }
    }

    #[test]
    fn its_bounds_are_an_unstacked_networks() {
        for feature_set in [FeatureSet::Psq, FeatureSet::PsqThreats] {
            for hidden in [&[][..], &[4][..]] {
                let vs = nn::VarStore::new(Device::Cpu);
                let network = Network::new(&vs.root(), feature_set, 8, hidden);
                let stack = StackedNetwork::with_shape(feature_set, 8, hidden, 2, Device::Cpu);
                assert_eq!(stack.weight_bounds(), network.weight_bounds(), "{feature_set:?} {hidden:?}");
            }
        }
    }

    #[test]
    fn the_arms_kept_compute_what_they_did_and_still_train() {
        let (networks, mut stack) = networks_and_stack(FeatureSet::Psq, &[4], 3);
        let batch = batch(FeatureSet::Psq);
        stack.retain(&[0, 2]);
        assert_eq!(stack.arms(), 2);
        let pawns = stack.pawns(&batch);
        for (slot, arm) in [0usize, 2].into_iter().enumerate() {
            close(&pawns.get(slot as i64), &networks[arm].1.pawns(&batch).view([-1]), &format!("arm {arm} in slot {slot}"));
        }
        for (name, parameter) in stack.parameters() {
            assert_eq!(parameter.size()[0], 2, "{name}");
            assert!(parameter.requires_grad(), "{name}");
        }
    }

    #[test]
    #[should_panic(expected = "strictly increasing")]
    fn arms_to_keep_out_of_order_panic() {
        let mut stack = StackedNetwork::with_shape(FeatureSet::Psq, 8, &[4], 3, Device::Cpu);
        stack.retain(&[2, 0]);
    }

    #[test]
    #[should_panic(expected = "feature-set mismatch")]
    fn a_batch_of_another_kind_panics() {
        let stack = StackedNetwork::with_shape(FeatureSet::Psq, 8, &[4], 2, Device::Cpu);
        let _ = stack.pawns(&batch(FeatureSet::PsqThreats));
    }
}
