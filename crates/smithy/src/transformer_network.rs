//! The Fattorini Transformer's own model: token plus positional embeddings, `num_layers` pre-norm
//! blocks, a final LayerNorm on `[CLS]`, and the value head reading it. Its own standalone type, not
//! a `network::Network` variant: nothing here is reached through `FeatureSet`/`DenseBatch` at all.
//!
//! **Pre-norm, GPT-2's initialisation, no dropout (2026-11-18).** The first version was post-norm
//! BERT, `LayerNorm(x + sublayer(x))`, with libtorch's default initialisation (token embeddings of
//! spread 1) and dropout 0.1. Trained alone on Lc0's data it barely moved from a constant
//! prediction, and not at all with its dropout, where the NNUEs learned at once from the same
//! positions; on a toy batch with a sharp signal it learned. A post-norm stack trains poorly from
//! scratch: the norm after every residual weakens the gradient through it. So each block is
//! `x + attention(LayerNorm(x))`, then `x + feed_forward(LayerNorm(x))`; every embedding and linear
//! weight starts from a normal of spread `INIT_STDEV`, every bias at 0; and nothing is dropped.
//!
//! The transformer network training steps (`training.rs`).
#![cfg_attr(not(test), allow(dead_code))]

use tch::nn;
use tch::Tensor;

use super::net_config::{Architecture, ArchitectureKind};
use super::nnue_network::bce_loss;
use super::transformer_batch::TransformerBatch;
use fattorini::labels::S_PAWNS;
use super::transformer_vocab::VOCAB_SIZE;
use super::value_head::{Activation, ValueHead};

/// `1` `[CLS]` + up to `2` kings + up to `cache::MAX_NON_KING_PIECES` non-king pieces = `33`, the
/// longest sequence `transformer_vocab::token_sequence` can ever produce (the opening position,
/// all `32` pieces on the board) -- the learned positional embedding table's own fixed width.
pub const MAX_SEQUENCE_LENGTH: usize = 1 + 2 + super::cache::MAX_NON_KING_PIECES;

/// GPT-2's spread for every embedding and linear weight.
pub const INIT_STDEV: f64 = 0.02;

fn normal() -> nn::Init {
    nn::Init::Randn { mean: 0.0, stdev: INIT_STDEV }
}

/// A linear layer's configuration: weights from `normal`, biases 0.
pub fn linear_config() -> nn::LinearConfig {
    nn::LinearConfig { ws_init: normal(), bs_init: Some(nn::Init::Const(0.0)), bias: true }
}

fn linear(vs: nn::Path, input: i64, output: i64) -> nn::Linear {
    nn::linear(vs, input, output, linear_config())
}

/// Token embedding plus learned positional embedding, added together: the first block's input.
pub struct Embeddings {
    token_embedding: nn::Embedding,
    /// `[MAX_SEQUENCE_LENGTH, embedding_dim]`, a plain learnable parameter rather than a second
    /// `nn::Embedding`: every real sequence's positions are always the contiguous prefix
    /// `0..seq_len`, so a lookup by arbitrary index would solve a problem this input never has.
    positional_embedding: Tensor,
}

impl Embeddings {
    pub fn new(vs: &nn::Path, vocab_size: i64, embedding_dim: i64) -> Embeddings {
        let config = nn::EmbeddingConfig { ws_init: normal(), ..Default::default() };
        let token_embedding = nn::embedding(vs / "token_embedding", vocab_size, embedding_dim, config);
        let positional_embedding = vs.var("positional_embedding", &[MAX_SEQUENCE_LENGTH as i64, embedding_dim], normal());
        Embeddings { token_embedding, positional_embedding }
    }

    /// `tokens`: `[batch, seq_len]`, `Kind::Int64` vocabulary indices. Returns `[batch, seq_len,
    /// embedding_dim]`.
    pub fn forward(&self, tokens: &Tensor) -> Tensor {
        let (_batch, seq_len) = tokens.size2().unwrap();
        let positions = self.positional_embedding.narrow(0, 0, seq_len).unsqueeze(0);
        tokens.apply(&self.token_embedding) + positions
    }
}

/// Multi-head self-attention, shared unchanged between full and kings-only attention: what differs
/// between the two is only the `attention_mask` passed into `forward`. Built on `Tensor::
/// scaled_dot_product_attention`.
pub struct SelfAttention {
    qkv: nn::Linear,
    proj: nn::Linear,
    num_heads: i64,
}

impl SelfAttention {
    pub fn new(vs: &nn::Path, embedding_dim: i64, num_heads: i64) -> SelfAttention {
        assert_eq!(
            embedding_dim % num_heads, 0,
            "embedding_dim ({embedding_dim}) must divide evenly across num_heads ({num_heads})"
        );
        let qkv = linear(vs / "qkv", embedding_dim, embedding_dim * 3);
        let proj = linear(vs / "proj", embedding_dim, embedding_dim);
        SelfAttention { qkv, proj, num_heads }
    }

    /// `hidden`: `[batch, seq_len, embedding_dim]`. `attention_mask`: `None` for full attention,
    /// `Some([batch, 1, seq_len, seq_len] Bool)` for kings-only, broadcasting over the heads.
    /// Returns `[batch, seq_len, embedding_dim]`.
    pub fn forward(&self, hidden: &Tensor, attention_mask: Option<&Tensor>) -> Tensor {
        let (batch, seq_len, embedding_dim) = hidden.size3().unwrap();
        let head_dim = embedding_dim / self.num_heads;
        // One projection producing Q, K and V together, `[batch, seq, 3, heads, head_dim]`, permuted
        // so the `3` axis comes first.
        let qkv = hidden.apply(&self.qkv).reshape([batch, seq_len, 3, self.num_heads, head_dim]).permute([2, 0, 3, 1, 4]);
        let (query, key, value) = (qkv.get(0), qkv.get(1), qkv.get(2));
        let attended = Tensor::scaled_dot_product_attention(&query, &key, &value, attention_mask, 0.0, false, None, false);
        attended.transpose(1, 2).reshape([batch, seq_len, embedding_dim]).apply(&self.proj)
    }
}

/// Two linear layers with a GELU between, widening to `feedforward_dim` and back, applied to every
/// position independently.
pub struct FeedForward {
    fc1: nn::Linear,
    fc2: nn::Linear,
}

impl FeedForward {
    pub fn new(vs: &nn::Path, embedding_dim: i64, feedforward_dim: i64) -> FeedForward {
        let fc1 = linear(vs / "fc1", embedding_dim, feedforward_dim);
        let fc2 = linear(vs / "fc2", feedforward_dim, embedding_dim);
        FeedForward { fc1, fc2 }
    }

    pub fn forward(&self, hidden: &Tensor) -> Tensor {
        hidden.apply(&self.fc1).gelu("none").apply(&self.fc2)
    }
}

/// One pre-norm block: `x + attention(norm1(x))`, then `x + feed_forward(norm2(x))`. The residual
/// path carries the input through untouched, so the gradient reaches every block undiminished.
pub struct TransformerBlock {
    attention: SelfAttention,
    norm1: nn::LayerNorm,
    feed_forward: FeedForward,
    norm2: nn::LayerNorm,
}

impl TransformerBlock {
    pub fn new(vs: &nn::Path, embedding_dim: i64, num_heads: i64, feedforward_dim: i64) -> TransformerBlock {
        let attention = SelfAttention::new(&(vs / "attention"), embedding_dim, num_heads);
        let norm1 = nn::layer_norm(vs / "norm1", vec![embedding_dim], Default::default());
        let feed_forward = FeedForward::new(&(vs / "feed_forward"), embedding_dim, feedforward_dim);
        let norm2 = nn::layer_norm(vs / "norm2", vec![embedding_dim], Default::default());
        TransformerBlock { attention, norm1, feed_forward, norm2 }
    }

    /// `hidden`: `[batch, seq_len, embedding_dim]`, unchanged in shape.
    pub fn forward(&self, hidden: &Tensor, attention_mask: Option<&Tensor>) -> Tensor {
        let hidden = hidden + self.attention.forward(&hidden.apply(&self.norm1), attention_mask);
        &hidden + self.feed_forward.forward(&hidden.apply(&self.norm2))
    }
}

/// The whole model: embeddings, `architecture.num_layers` blocks, a final LayerNorm on `[CLS]`, and
/// the value head reading it. The same for full and kings-only attention: only the batch's
/// `attention_mask` differs.
pub struct TransformerNetwork {
    embeddings: Embeddings,
    blocks: Vec<TransformerBlock>,
    /// A pre-norm stack's residual stream is never normalised by its blocks: this is what the value
    /// head reads it through.
    final_norm: nn::LayerNorm,
    value_head: ValueHead,
}

impl TransformerNetwork {
    /// Panics if `architecture.kind` isn't `Transformer`: a caller bug.
    pub fn new(vs: &nn::Path, architecture: &Architecture) -> TransformerNetwork {
        let ArchitectureKind::Transformer { num_layers, num_heads, feedforward_dim } = &architecture.kind else {
            panic!("TransformerNetwork::new requires architecture.kind to be transformer");
        };
        let embedding_dim = architecture.embedding_dim();
        let embeddings = Embeddings::new(&(vs / "embeddings"), VOCAB_SIZE as i64, embedding_dim);
        let blocks = (0..*num_layers)
            .map(|i| TransformerBlock::new(&(vs / format!("block{i}")), embedding_dim, *num_heads, *feedforward_dim))
            .collect();
        let final_norm = nn::layer_norm(vs / "final_norm", vec![embedding_dim], Default::default());
        let value_head = ValueHead::with_config(
            &(vs / "value_head"),
            embedding_dim,
            architecture.value_head_hidden_dim(),
            Activation::Relu,
            linear_config,
        );
        TransformerNetwork { embeddings, blocks, final_norm, value_head }
    }

    /// `[batch, 1]`, the evaluation in pawns, raw and unbounded: what training steps and what
    /// inference will run.
    pub fn forward(&self, batch: &TransformerBatch) -> Tensor {
        let mut hidden = self.embeddings.forward(&batch.tokens);
        for block in &self.blocks {
            hidden = block.forward(&hidden, batch.attention_mask.as_ref());
        }
        let cls = hidden.select(1, 0).apply(&self.final_norm);
        self.value_head.forward(&cls)
    }

    /// The loss one training step descends: `nnue_network::bce_loss` on `forward`, against label
    /// column `column` (a one-arm batch has column `0` alone), in Elo's unit.
    pub fn training_loss(&self, batch: &TransformerBatch, column: usize) -> Tensor {
        bce_loss(&self.forward(batch), &batch.labels.narrow(1, column as i64, 1), S_PAWNS)
    }

    /// The test loss against label column `column`: the same binary cross entropy as `training_loss`.
    pub fn test_loss(&self, batch: &TransformerBatch, column: usize) -> Tensor {
        bce_loss(&self.forward(batch), &batch.labels.narrow(1, column as i64, 1), S_PAWNS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_produces_the_expected_shape() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let embeddings = Embeddings::new(&vs.root(), 100, 16);

        let tokens = Tensor::from_slice(&[1i64, 2, 3, 4, 5, 6]).view([2, 3]);
        let output = embeddings.forward(&tokens);

        assert_eq!(output.size(), [2, 3, 16]);
    }

    #[test]
    fn forward_gives_the_same_token_at_different_positions_different_embeddings() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let embeddings = Embeddings::new(&vs.root(), 100, 16);

        let tokens = Tensor::from_slice(&[7i64, 1, 1, 7]).view([2, 2]);
        let output = embeddings.forward(&tokens);

        let token_7_at_position_0 = output.get(0).get(0);
        let token_7_at_position_1 = output.get(1).get(1);
        assert!(!token_7_at_position_0.allclose(&token_7_at_position_1, 1e-6, 1e-6, false));
    }

    #[test]
    fn forward_gives_the_same_token_at_the_same_position_the_same_embedding() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let embeddings = Embeddings::new(&vs.root(), 100, 16);

        let tokens = Tensor::from_slice(&[3i64, 9, 3, 1]).view([2, 2]);
        let output = embeddings.forward(&tokens);

        let first_row_position_0 = output.get(0).get(0);
        let second_row_position_0 = output.get(1).get(0);
        assert!(first_row_position_0.allclose(&second_row_position_0, 1e-6, 1e-6, false));
    }

    #[test]
    fn every_weight_starts_small_every_bias_at_zero_every_norm_at_identity() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let _ = TransformerNetwork::new(&vs.root(), &sample_architecture());
        for (name, value) in vs.variables() {
            if name.contains("norm") {
                let expected = if name.ends_with("weight") { 1.0 } else { 0.0 };
                assert!(value.allclose(&value.full_like(expected), 0.0, 0.0, false), "{name}");
            } else if name.ends_with("bias") {
                assert_eq!(value.abs().max().double_value(&[]), 0.0, "{name}");
            } else if value.numel() >= 64 {
                // Too few values in a smaller one for its sample spread to say much.
                let spread = value.std(true).double_value(&[]);
                assert!((0.01..0.03).contains(&spread), "{name}: spread {spread}");
            }
        }
    }

    #[test]
    fn self_attention_forward_produces_the_expected_shape() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let attention = SelfAttention::new(&vs.root(), 16, 4);

        let hidden = Tensor::randn([2, 5, 16], (tch::Kind::Float, tch::Device::Cpu));
        let output = attention.forward(&hidden, None);

        assert_eq!(output.size(), [2, 5, 16]);
    }

    #[test]
    #[should_panic(expected = "must divide evenly")]
    fn self_attention_rejects_a_head_count_that_does_not_divide_the_embedding_dimension() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        SelfAttention::new(&vs.root(), 15, 4);
    }

    /// An identity-only mask (every position attends only itself) against none at all: the outputs
    /// must differ, or the mask would be silently doing nothing.
    fn identity_mask(seq_len: usize) -> Tensor {
        let mut mask = vec![false; seq_len * seq_len];
        for i in 0..seq_len {
            mask[i * seq_len + i] = true;
        }
        Tensor::from_slice(&mask).view([1, 1, seq_len as i64, seq_len as i64])
    }

    #[test]
    fn self_attention_mask_actually_changes_the_output() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let attention = SelfAttention::new(&vs.root(), 16, 4);
        let hidden = Tensor::randn([1, 4, 16], (tch::Kind::Float, tch::Device::Cpu));

        let masked_output = attention.forward(&hidden, Some(&identity_mask(4)));
        let full_output = attention.forward(&hidden, None);
        assert!(!masked_output.allclose(&full_output, 1e-6, 1e-6, false));
    }

    #[test]
    fn feed_forward_forward_produces_the_expected_shape() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let feed_forward = FeedForward::new(&vs.root(), 16, 64);

        let hidden = Tensor::randn([2, 5, 16], (tch::Kind::Float, tch::Device::Cpu));
        let output = feed_forward.forward(&hidden);

        // In and out at embedding_dim (16), not feedforward_dim (64).
        assert_eq!(output.size(), [2, 5, 16]);
    }

    #[test]
    fn transformer_block_forward_produces_the_expected_shape() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let block = TransformerBlock::new(&vs.root(), 16, 4, 64);

        let hidden = Tensor::randn([2, 5, 16], (tch::Kind::Float, tch::Device::Cpu));
        let output = block.forward(&hidden, None);

        assert_eq!(output.size(), [2, 5, 16]);
    }

    #[test]
    fn transformer_block_mask_actually_changes_the_output() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let block = TransformerBlock::new(&vs.root(), 16, 4, 64);
        let hidden = Tensor::randn([1, 4, 16], (tch::Kind::Float, tch::Device::Cpu));

        let masked_output = block.forward(&hidden, Some(&identity_mask(4)));
        let full_output = block.forward(&hidden, None);
        assert!(!masked_output.allclose(&full_output, 1e-6, 1e-6, false));
    }

    fn sample_architecture() -> super::super::net_config::Architecture {
        super::super::net_config::Architecture {
            embedding_dim: Some(16),
            value_head: Some(super::super::net_config::ValueHeadConfig { hidden_dim: vec![8, 8] }),
            king_buckets: None,
            kind: super::super::net_config::ArchitectureKind::Transformer { num_layers: 2, num_heads: 4, feedforward_dim: 32 },
        }
    }

    fn sample_batch(attention: super::super::net_config::Attention) -> TransformerBatch {
        use super::super::cache::CacheRecord;
        use fattorini::board::castling_rights::CastlingRights;
        use fattorini::board::color::Color;
        use fattorini::board::square::Square;
        use fattorini::features::CanonicalPosition;

        let record = |label: f32| {
            let canonical = CanonicalPosition {
                side_to_move: Color::White,
                pieces: vec![],
                white_king: Square::E1,
                black_king: Square::E8,
                castling_rights: CastlingRights::NONE,
                en_passant_file: None,
            };
            CacheRecord::new(canonical, label)
        };
        let records = vec![record(0.5), record(0.25)];
        super::super::transformer_batch::to_transformer_batch(&records, attention)
    }

    #[test]
    fn transformer_network_forward_produces_one_finite_unbounded_value_per_row_for_full_attention() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let network = TransformerNetwork::new(&vs.root(), &sample_architecture());
        let batch = sample_batch(super::super::net_config::Attention::Full);

        let output = network.forward(&batch);

        assert_eq!(output.size(), [2, 1]);
        let values: Vec<f32> = output.reshape([2]).try_into().unwrap();
        for value in values {
            assert!(value.is_finite(), "expected a finite unbounded value, got {value}");
        }
    }

    /// Training never pads, but inference will: a GPU batch of search leaves mixes piece counts, so
    /// shorter positions are padded with tokens no query may attend to. That must not change any
    /// position's value: LayerNorm, the feed-forward part and the value head act per token, the real
    /// tokens keep their positional embeddings, and the mask closes attention, the one way between
    /// tokens.
    #[test]
    fn a_padded_position_masked_gives_the_value_it_gives_alone() {
        use super::super::cache::CacheRecord;
        use super::super::net_config::Attention;
        use super::super::transformer_batch::to_transformer_batch;
        use fattorini::board::castling_rights::CastlingRights;
        use fattorini::board::color::Color;
        use fattorini::board::piece_type::PieceType;
        use fattorini::board::square::Square;
        use fattorini::features::{pack_canonical_piece, CanonicalPosition};

        let record = |pawns: &[Square]| {
            let canonical = CanonicalPosition {
                side_to_move: Color::White,
                pieces: pawns.iter().map(|&square| pack_canonical_piece(square, PieceType::Pawn, Color::White)).collect(),
                white_king: Square::E1,
                black_king: Square::E8,
                castling_rights: CastlingRights::NONE,
                en_passant_file: None,
            };
            CacheRecord::new(canonical, 0.0)
        };
        let positions = [record(&[]), record(&[Square::D2]), record(&[Square::D2, Square::E2, Square::F3])];
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let network = TransformerNetwork::new(&vs.root(), &sample_architecture());
        let alone: Vec<f64> = positions
            .iter()
            .map(|position| network.forward(&to_transformer_batch(std::slice::from_ref(position), Attention::Full)).double_value(&[0, 0]))
            .collect();

        let sequences: Vec<Tensor> =
            positions.iter().map(|position| to_transformer_batch(std::slice::from_ref(position), Attention::Full).tokens).collect();
        let lengths: Vec<i64> = sequences.iter().map(|tokens| tokens.size()[1]).collect();
        let longest = *lengths.iter().max().unwrap();
        // Padding with the first real token: any valid index will do, since the mask hides it.
        let tokens = Tensor::stack(
            &sequences
                .iter()
                .zip(&lengths)
                .map(|(tokens, &length)| {
                    let padding = tokens.narrow(1, 0, 1).repeat([1, longest - length]);
                    Tensor::cat(&[tokens.shallow_clone(), padding], 1).squeeze_dim(0)
                })
                .collect::<Vec<Tensor>>(),
            0,
        );
        // Every query may attend to the real keys of its own position, and to nothing else.
        let keys = Tensor::arange(longest, (tch::Kind::Int64, tch::Device::Cpu)).view([1, longest]);
        let real = keys.lt_tensor(&Tensor::from_slice(&lengths).view([-1, 1]));
        let mask = real.view([lengths.len() as i64, 1, 1, longest]).expand([lengths.len() as i64, 1, longest, longest], false);
        let padded = TransformerBatch { tokens, attention_mask: Some(mask), labels: Tensor::zeros([3, 1], (tch::Kind::Float, tch::Device::Cpu)) };

        let together = network.forward(&padded);

        for (row, &value) in alone.iter().enumerate() {
            let padded_value = together.double_value(&[row as i64, 0]);
            assert!((padded_value - value).abs() < 1e-5, "position {row}: {padded_value} padded, {value} alone");
        }
    }

    #[test]
    fn transformer_network_forward_produces_one_finite_unbounded_value_per_row_for_kings_only_attention() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let network = TransformerNetwork::new(&vs.root(), &sample_architecture());
        let batch = sample_batch(super::super::net_config::Attention::KingsOnly);

        let output = network.forward(&batch);

        assert_eq!(output.size(), [2, 1]);
        let values: Vec<f32> = output.reshape([2]).try_into().unwrap();
        for value in values {
            assert!(value.is_finite(), "expected a finite unbounded value, got {value}");
        }
    }
}
