//! The one-arm training step against the stack's, before the stack goes: from the same weights, a
//! one-arm `StackedNetwork` or `StackedTransformer` under `StackedAdamW`, and the unstacked network
//! under `AdamW`, both on one budget, take the same steps on one label column and must end with the
//! same losses and the same weights, within float tolerance. Deleted with the stack, once the
//! one-arm loop has replaced it.

use tch::{nn, Device, Kind, Tensor};

use super::adamw::AdamW;
use super::arm_slice;
use super::batch::{self, DenseBatch};
use super::budget::Budget;
use super::cache::CacheRecord;
use super::feature_set::FeatureSet;
use super::net_config::{Architecture, ArchitectureKind, Attention, ValueHeadConfig};
use super::nnue_network::{HeadShape, Network};
use super::stacked_adamw::StackedAdamW;
use super::stacked_network::StackedNetwork;
use super::stacked_transformer::StackedTransformer;
use super::transformer_batch::{to_transformer_batch, TransformerBatch};
use super::transformer_network::TransformerNetwork;
use fattorini::board::Board;
use fattorini::features::{extract_canonical, halfk};
use fattorini::labels::S_PAWNS;

/// Steps taken: past the budget's warmup of 2, into its fall.
const STEPS: usize = 6;

/// 8 steps an epoch, rounds of 2, 2 epochs: 16 steps, the first 2 the warmup, falling from 1e-2 to
/// 1e-4, so that the weights move far enough for a difference to show.
fn budget() -> Budget {
    Budget::new(80, 10, 2, 1, 2.0, &[1e-2, 1e-4])
}

/// `fens` with one label each, as the polyline gives a single column.
fn records(fens: &[&str]) -> Vec<CacheRecord> {
    fens.iter()
        .enumerate()
        .map(|(index, fen)| {
            let canonical = extract_canonical(&Board::from_fen(fen).unwrap().0);
            CacheRecord::new(canonical, [0.6f32, -0.4, 0.2, -0.9, 0.8, 0.0][index % 6])
        })
        .collect()
}

fn close(a: &Tensor, b: &Tensor, what: &str) {
    assert!(a.allclose(b, 1e-4, 1e-5, false), "{what}: {a:?} against {b:?}");
}

/// Each weight of `vs` against its slice of the stack's.
fn assert_weights_agree(vs: &nn::VarStore, stacked: &std::collections::HashMap<String, Tensor>, what: &str) {
    for (name, value) in vs.variables() {
        close(&stacked[&name].get(0), &value, &format!("{what} {name}"));
    }
}

/// Two batches of three positions each, taken in turn.
const NNUE_FENS: [&str; 6] = [
    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3",
    "8/5k2/8/3P4/8/8/2K5/8 b - - 0 1",
    "r3k2r/ppp2ppp/2n1bn2/3pp3/3PP3/2N1BN2/PPP2PPP/R3K2R w KQkq d6 0 8",
    "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1",
    "rnbqkb1r/pppp1ppp/5n2/4p3/2B1P3/8/PPPP1PPP/RNBQK1NR w KQkq - 2 3",
];

struct NnueCase {
    what: &'static str,
    feature_set: FeatureSet,
    pairwise: bool,
    screlu: bool,
    head: HeadShape,
    embedding_dim: i64,
    hidden: &'static [i64],
}

fn nnue_steps_alike(case: &NnueCase) {
    let NnueCase { what, feature_set, pairwise, screlu, head, embedding_dim, hidden } = *case;
    // Not Elo's unit, so a loss that ignored `with_s` would show.
    let s_pawns = 1.3 * S_PAWNS;
    let vs = nn::VarStore::new(Device::Cpu);
    let network =
        Network::with_options(&vs.root(), feature_set, pairwise, screlu, head, embedding_dim, hidden).with_s(s_pawns);
    let stack = StackedNetwork::with_options(feature_set, pairwise, screlu, head, embedding_dim, hidden, 1, Device::Cpu)
        .with_s(s_pawns);
    for (name, value) in vs.variables() {
        arm_slice::fill_slice(&stack.parameters()[&name], 0, &value, &name);
    }
    assert_eq!(network.weight_bounds(), stack.weight_bounds(), "{what}: the bounds");

    let mut optimizer = AdamW::new(&vs, 1e-3);
    let mut stacked_optimizer = StackedAdamW::new(stack.parameters(), 1e-3);
    for (name, bound) in network.weight_bounds() {
        optimizer.bound(name, bound);
        stacked_optimizer.bound(name, bound);
    }
    optimizer.set_budget(budget());
    stacked_optimizer.set_budget(budget());

    let batches: Vec<DenseBatch> =
        NNUE_FENS.chunks(3).map(|fens| batch::to_dense_batch(&records(fens), feature_set)).collect();
    // The sparse transform computes what the dense one does.
    close(&network.pawns(&batches[0]), &network.dense_pawns(&batches[0]), &format!("{what}: sparse against dense"));
    let column = Tensor::from_slice(&[0i64]);
    for step in 0..STEPS {
        let batch = &batches[step % batches.len()];
        let what = format!("{what}, step {step}");

        let loss = network.training_loss(batch, 0);
        optimizer.zero_grad();
        loss.backward();
        optimizer.step();

        let stacked_loss = stack.training_losses(batch, &column);
        stacked_optimizer.zero_grad();
        stacked_loss.sum(Kind::Float).backward();
        stacked_optimizer.step();

        close(&stacked_loss.get(0).detach(), &loss.detach(), &format!("{what}: training loss"));
    }
    assert_weights_agree(&vs, stack.parameters(), what);
    tch::no_grad(|| {
        for (index, batch) in batches.iter().enumerate() {
            close(&stack.test_losses(batch, &column).get(0), &network.test_loss(batch, 0), &format!("{what}: test loss {index}"));
        }
    });
}

#[test]
fn a_one_arm_nnue_steps_as_its_stack_does() {
    let layout = halfk::Layout::ALL[0];
    let cases = [
        NnueCase {
            what: "psq",
            feature_set: FeatureSet::Psq,
            pairwise: false,
            screlu: false,
            head: HeadShape::Plain,
            embedding_dim: 8,
            hidden: &[4],
        },
        NnueCase {
            what: "halfk screlu-hidden",
            feature_set: FeatureSet::HalfKav2Hm(layout),
            pairwise: false,
            screlu: true,
            head: HeadShape::Plain,
            embedding_dim: 8,
            hidden: &[4, 4],
        },
        NnueCase {
            what: "halfk pairwise dual",
            feature_set: FeatureSet::HalfKav2HmState(layout),
            pairwise: true,
            screlu: false,
            head: HeadShape::Dual,
            embedding_dim: 8,
            hidden: &[4, 4],
        },
    ];
    for case in &cases {
        nnue_steps_alike(case);
    }
}

/// Same piece count, so one batch: 32 pieces each.
const TRANSFORMER_FENS: [&str; 3] = [
    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3",
    "rnbqkb1r/pppppppp/5n2/8/3P4/8/PPP1PPPP/RNBQKBNR w KQkq - 1 2",
];

#[test]
fn a_one_arm_transformer_steps_as_its_stack_does() {
    let architecture = Architecture {
        embedding_dim: Some(16),
        value_head: Some(ValueHeadConfig { hidden_dim: vec![8] }),
        king_buckets: None,
        kind: ArchitectureKind::Transformer { num_layers: 2, num_heads: 4, feedforward_dim: 32 },
    };
    let vs = nn::VarStore::new(Device::Cpu);
    let network = TransformerNetwork::new(&vs.root(), &architecture);
    let stack = StackedTransformer::new(&architecture, 1, Device::Cpu);
    for (name, value) in vs.variables() {
        arm_slice::fill_slice(&stack.parameters()[&name], 0, &value, &name);
    }
    let mut optimizer = AdamW::new(&vs, 1e-3);
    let mut stacked_optimizer = StackedAdamW::new(stack.parameters(), 1e-3);
    optimizer.set_budget(budget());
    stacked_optimizer.set_budget(budget());

    let batch: TransformerBatch = to_transformer_batch(&records(&TRANSFORMER_FENS), Attention::Full);
    let column = Tensor::from_slice(&[0i64]);
    for step in 0..STEPS {
        let loss = network.training_loss(&batch, 0);
        optimizer.zero_grad();
        loss.backward();
        optimizer.step();

        let stacked_loss = stack.training_losses(&batch, &column);
        stacked_optimizer.zero_grad();
        stacked_loss.sum(Kind::Float).backward();
        stacked_optimizer.step();

        close(&stacked_loss.get(0).detach(), &loss.detach(), &format!("transformer, step {step}: training loss"));
    }
    assert_weights_agree(&vs, stack.parameters(), "transformer");
    tch::no_grad(|| close(&stack.test_losses(&batch, &column).get(0), &network.test_loss(&batch, 0), "transformer: test loss"));
}
