//! Step 4 (the transformer plan's Plan): the Kalo Chess Transformer's own model --
//! see the transformer design's "Shared transformer encoder" and "Value head" sections for the
//! settled design this implements. Its own standalone type, not a `network::Network` variant
//! (design point H): nothing here is reached through `FeatureSet`/`DenseBatch` at all.
//!
//! The binary only builds it, for the parameters `StackedTransformer` stacks and a fresh run starts
//! from; its forward passes are the stack's test reference and inference's future entry point, so
//! outside the tests they are unread.
#![cfg_attr(not(test), allow(dead_code))]

use tch::nn;
use tch::Tensor;

use super::net_config::{Architecture, ArchitectureKind};
use super::transformer_batch::TransformerBatch;
use super::transformer_vocab::VOCAB_SIZE;
use super::value_head::{Activation, ValueHead};

/// `1` `[CLS]` + up to `2` kings + up to `cache::MAX_NON_KING_PIECES` non-king pieces = `33`, the
/// longest sequence `transformer_vocab::token_sequence` can ever produce (the opening position,
/// all `32` pieces on the board) -- the learned positional embedding table's own fixed width. Not
/// imported from `length_cache::MAX_PIECE_COUNT` (that module is about cache-file bucketing, a
/// different concern) -- derived directly from the one constant both actually share.
pub const MAX_SEQUENCE_LENGTH: usize = 1 + 2 + super::cache::MAX_NON_KING_PIECES;

/// Token embedding (a real lookup table, indexed by arbitrary vocabulary indices) plus learned
/// positional embedding (the transformer design: "learned, not fixed/sinusoidal") -- added
/// together, the standard BERT-style input to the first transformer block.
pub struct Embeddings {
    token_embedding: nn::Embedding,
    /// `[MAX_SEQUENCE_LENGTH, embedding_dim]`, a plain learnable parameter rather than a second
    /// `nn::Embedding`: every real sequence's positions are always the contiguous prefix
    /// `0..seq_len` (nothing is ever offset partway into this table), so a lookup-by-arbitrary-
    /// index table would be solving a problem this input never actually has.
    positional_embedding: Tensor,
}

impl Embeddings {
    pub fn new(vs: &nn::Path, vocab_size: i64, embedding_dim: i64) -> Embeddings {
        let token_embedding = nn::embedding(vs / "token_embedding", vocab_size, embedding_dim, Default::default());
        let positional_embedding = vs.var(
            "positional_embedding",
            &[MAX_SEQUENCE_LENGTH as i64, embedding_dim],
            nn::Init::Randn { mean: 0., stdev: 0.02 },
        );
        Embeddings { token_embedding, positional_embedding }
    }

    /// `tokens`: `[batch, seq_len]`, `Kind::Int64` vocabulary indices (`transformer_batch::
    /// TransformerBatch::tokens`). Returns `[batch, seq_len, embedding_dim]`. `seq_len` is read
    /// directly off `tokens`' own shape, never passed separately -- there is no other value it
    /// could legitimately be. `dropout`/`train`: standard BERT embedding dropout, applied right
    /// after summing the token and positional embeddings -- a no-op whenever `train` is `false`
    /// (`Tensor::dropout`'s own eval-mode behavior), so a play-time/eval forward pass never needs
    /// a separate code path to skip it.
    pub fn forward(&self, tokens: &Tensor, dropout: f64, train: bool) -> Tensor {
        let (_batch, seq_len) = tokens.size2().unwrap();
        let token_embedded = tokens.apply(&self.token_embedding);
        let positions = self.positional_embedding.narrow(0, 0, seq_len).unsqueeze(0);
        (token_embedded + positions).dropout(dropout, train)
    }
}

/// Multi-head self-attention, shared unchanged between full and kings-only attention (design
/// point C of the transformer design's Attention section) -- what differs between the two is only the
/// `attention_mask` passed into `forward`, never this struct or its weights. Built on `Tensor::
/// scaled_dot_product_attention` (the built-in ATen op) rather than hand-rolled matmul/softmax,
/// per design point C: this is standard, generically-documented ML machinery, not specific to Kalo Chess
/// logic CLAUDE.md rule 3 requires hand-deriving.
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
        let qkv = nn::linear(vs / "qkv", embedding_dim, embedding_dim * 3, Default::default());
        let proj = nn::linear(vs / "proj", embedding_dim, embedding_dim, Default::default());
        SelfAttention { qkv, proj, num_heads }
    }

    /// `hidden`: `[batch, seq_len, embedding_dim]`. `attention_mask`: `transformer_batch::
    /// TransformerBatch::attention_mask`'s own `Option<&Tensor>` directly -- `None` for full
    /// attention (every pair allowed, nothing to restrict), `Some([batch, 1, seq_len, seq_len]
    /// Bool)` for kings-only, broadcasting over the heads axis below. Returns `[batch, seq_len,
    /// embedding_dim]`, the same shape `hidden` came in as (a transformer block's own residual
    /// connection needs this to add cleanly back onto its input). `dropout`/`train`: BERT's own
    /// two attention dropouts -- on the attention *probabilities* themselves (`scaled_dot_
    /// product_attention`'s own `dropout_p`, only nonzero while training: this op has no eval-mode
    /// awareness of its own, unlike `Tensor::dropout`, so the caller must gate it) and again on
    /// the output projection, before it's handed back to the block's own residual add.
    pub fn forward(&self, hidden: &Tensor, attention_mask: Option<&Tensor>, dropout: f64, train: bool) -> Tensor {
        let (batch, seq_len, embedding_dim) = hidden.size3().unwrap();
        let head_dim = embedding_dim / self.num_heads;

        // One projection producing Q, K, and V together, then split apart -- same shape as
        // `vision::dinov2::Attention::forward`'s own `qkv` layer, adapted from its 4D (image
        // patch) case to our 3D (token sequence) one: `[batch, seq, 3, heads, head_dim]`,
        // permuted so the `3` (query/key/value) axis comes first and can be indexed off directly.
        let qkv = hidden.apply(&self.qkv).reshape([batch, seq_len, 3, self.num_heads, head_dim]).permute([2, 0, 3, 1, 4]);
        let query = qkv.get(0);
        let key = qkv.get(1);
        let value = qkv.get(2);

        let attention_dropout_p = if train { dropout } else { 0.0 };
        let attended = Tensor::scaled_dot_product_attention(
            &query, &key, &value, attention_mask, attention_dropout_p, false, None, false,
        );
        attended.transpose(1, 2).reshape([batch, seq_len, embedding_dim]).apply(&self.proj).dropout(dropout, train)
    }
}

/// The position-wise feed-forward sublayer every transformer block has alongside self-attention --
/// the transformer design's "BERT-like" setup: two linear layers with a GELU in between, widening
/// to `feedforward_dim` and back down to `embedding_dim`, applied identically and independently to
/// every sequence position (nothing here mixes information across positions -- that's attention's
/// own job, not this sublayer's).
pub struct FeedForward {
    fc1: nn::Linear,
    fc2: nn::Linear,
}

impl FeedForward {
    pub fn new(vs: &nn::Path, embedding_dim: i64, feedforward_dim: i64) -> FeedForward {
        let fc1 = nn::linear(vs / "fc1", embedding_dim, feedforward_dim, Default::default());
        let fc2 = nn::linear(vs / "fc2", feedforward_dim, embedding_dim, Default::default());
        FeedForward { fc1, fc2 }
    }

    /// `hidden`: `[batch, seq_len, embedding_dim]`, unchanged in and out. `dropout`/`train`:
    /// applied once, right after `fc2`, before the block's own residual add -- same placement and
    /// eval-mode-is-a-no-op behavior as `SelfAttention::forward`'s own output dropout.
    pub fn forward(&self, hidden: &Tensor, dropout: f64, train: bool) -> Tensor {
        hidden.apply(&self.fc1).gelu("none").apply(&self.fc2).dropout(dropout, train)
    }
}

/// One full transformer block: self-attention then feed-forward, each wrapped in its own residual
/// connection and followed by `LayerNorm` -- classic (post-norm) BERT, `LayerNorm(x +
/// Sublayer(x))` for each of the two sublayers in turn, not the pre-norm ordering newer
/// architectures (GPT-2 onward, `vision::dinov2::Block`'s own `x + Sublayer(LayerNorm(x))`) use
/// instead. The transformer design calls for "standard BERT-like encoding" specifically, and
/// original BERT is post-norm, so that's what this is.
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

    /// `hidden`: `[batch, seq_len, embedding_dim]`, unchanged in and out -- `attention_mask` is
    /// passed straight through to `SelfAttention::forward` unchanged, this block has no opinion
    /// of its own about it. `dropout`/`train` are passed to both sublayers identically -- one
    /// dropout rate for the whole block, matching `net.yaml`'s own single `architecture.dropout`
    /// field (no per-sublayer rate is exposed to configure separately).
    pub fn forward(&self, hidden: &Tensor, attention_mask: Option<&Tensor>, dropout: f64, train: bool) -> Tensor {
        let attended = self.attention.forward(hidden, attention_mask, dropout, train);
        let hidden = (hidden + attended).apply(&self.norm1);
        let fed_forward = self.feed_forward.forward(&hidden, dropout, train);
        (&hidden + fed_forward).apply(&self.norm2)
    }
}

/// The whole model: embeddings, `architecture.num_layers` transformer blocks in sequence, then the
/// value head reading `[CLS]`'s own final hidden state -- everything the transformer design's
/// "Shared transformer encoder" and "Value head" sections settle, assembled. Shared unchanged
/// between full and kings-only attention (design point A): both are this exact same struct, with
/// the same weights shape, trained as two separate runs -- only `TransformerBatch::
/// attention_mask` (built differently upstream, by `transformer_batch::to_transformer_batch`,
/// depending on `net.yaml`'s own `training.attention`) ever differs between the two.
pub struct TransformerNetwork {
    embeddings: Embeddings,
    blocks: Vec<TransformerBlock>,
    value_head: ValueHead,
    dropout: f64,
}

impl TransformerNetwork {
    /// Panics if `architecture.kind` isn't `Transformer` -- `main.rs` only ever calls this once
    /// `architecture.kind` has already been matched to route here in the first place, so this is
    /// an assertion against a caller bug (CLAUDE.md rule 4), never a real code path.
    pub fn new(vs: &nn::Path, architecture: &Architecture) -> TransformerNetwork {
        let ArchitectureKind::Transformer { num_layers, num_heads, feedforward_dim, dropout } = &architecture.kind
        else {
            panic!("TransformerNetwork::new requires architecture.kind to be transformer");
        };
        let embedding_dim = architecture.embedding_dim();
        let embeddings = Embeddings::new(&(vs / "embeddings"), VOCAB_SIZE as i64, embedding_dim);
        let mut blocks = Vec::with_capacity(*num_layers as usize);
        for i in 0..*num_layers {
            blocks.push(TransformerBlock::new(&(vs / format!("block{i}")), embedding_dim, *num_heads, *feedforward_dim));
        }
        let value_head =
            ValueHead::new(&(vs / "value_head"), embedding_dim, architecture.value_head_hidden_dim(), Activation::Relu);
        TransformerNetwork { embeddings, blocks, value_head, dropout: *dropout }
    }

    /// `batch.tokens`/`batch.attention_mask` flow straight through every block in turn; the final
    /// hidden state's own `[CLS]` row (sequence position `0`, `Tensor::select`) is what the value
    /// head actually reads -- every other position's final representation is computed (attention
    /// needs the whole sequence to produce even `[CLS]`'s own row correctly) but never read
    /// directly by anything downstream of this function. Returns `[batch, 1]`, a raw, unbounded
    /// value (`ValueHead::forward`'s own doc comment) -- no sigmoid, no further conversion needed
    /// by callers. `train` gates every dropout call transitively (`Embeddings`/
    /// `SelfAttention`/`FeedForward`'s own `forward` methods) -- `false` for play/eval, `true`
    /// only while actually training.
    ///
    /// Training runs `StackedTransformer`, every lambda at once; this one-network forward is the
    /// reference it is tested against, and what inference will run.
    pub fn forward(&self, batch: &TransformerBatch, train: bool) -> Tensor {
        let mut hidden = self.embeddings.forward(&batch.tokens, self.dropout, train);
        for block in &self.blocks {
            hidden = block.forward(&hidden, batch.attention_mask.as_ref(), self.dropout, train);
        }
        let cls = hidden.select(1, 0);
        self.value_head.forward(&cls)
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
        let output = embeddings.forward(&tokens, 0.0, false);

        assert_eq!(output.size(), [2, 3, 16]);
    }

    #[test]
    fn forward_gives_the_same_token_at_different_positions_different_embeddings() {
        // Token `7` appearing at sequence position `0` in one row and position `1` in another
        // must come out different -- if positional information were silently dropped, both rows
        // would be identical (same token embedding, same missing positional term).
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let embeddings = Embeddings::new(&vs.root(), 100, 16);

        let tokens = Tensor::from_slice(&[7i64, 1, 1, 7]).view([2, 2]);
        let output = embeddings.forward(&tokens, 0.0, false);

        let token_7_at_position_0 = output.get(0).get(0);
        let token_7_at_position_1 = output.get(1).get(1);
        assert!(!token_7_at_position_0.allclose(&token_7_at_position_1, 1e-6, 1e-6, false));
    }

    #[test]
    fn forward_gives_the_same_token_at_the_same_position_the_same_embedding() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let embeddings = Embeddings::new(&vs.root(), 100, 16);

        let tokens = Tensor::from_slice(&[3i64, 9, 3, 1]).view([2, 2]);
        let output = embeddings.forward(&tokens, 0.0, false);

        let first_row_position_0 = output.get(0).get(0);
        let second_row_position_0 = output.get(1).get(0);
        assert!(first_row_position_0.allclose(&second_row_position_0, 1e-6, 1e-6, false));
    }

    #[test]
    fn forward_dropout_is_a_no_op_when_not_training_but_randomizes_when_training() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let embeddings = Embeddings::new(&vs.root(), 100, 16);
        let tokens = Tensor::from_slice(&[1i64, 2, 3, 4]).view([1, 4]);

        // train = false: dropout must be inert, so two calls with the very same input are
        // identical -- if this weren't true, `train`'s own eval-mode gating would be broken.
        let first = embeddings.forward(&tokens, 0.9, false);
        let second = embeddings.forward(&tokens, 0.9, false);
        assert!(first.allclose(&second, 1e-6, 1e-6, false));

        // train = true with a high dropout rate: two calls must differ (each draws its own random
        // dropout mask) -- otherwise `dropout` would be silently ignored even while training.
        let third = embeddings.forward(&tokens, 0.9, true);
        let fourth = embeddings.forward(&tokens, 0.9, true);
        assert!(!third.allclose(&fourth, 1e-6, 1e-6, false));
    }

    #[test]
    fn self_attention_forward_produces_the_expected_shape() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let attention = SelfAttention::new(&vs.root(), 16, 4);

        let hidden = Tensor::randn([2, 5, 16], (tch::Kind::Float, tch::Device::Cpu));
        let output = attention.forward(&hidden, None, 0.0, false);

        assert_eq!(output.size(), [2, 5, 16]);
    }

    #[test]
    #[should_panic(expected = "must divide evenly")]
    fn self_attention_rejects_a_head_count_that_does_not_divide_the_embedding_dimension() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        SelfAttention::new(&vs.root(), 15, 4);
    }

    #[test]
    fn self_attention_mask_actually_changes_the_output() {
        // An identity-only mask (every position may attend only itself) restricts information
        // flow far more than no mask at all (every position attends everywhere) -- the two must
        // produce different outputs, or the mask argument would be silently doing nothing.
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let attention = SelfAttention::new(&vs.root(), 16, 4);
        let hidden = Tensor::randn([1, 4, 16], (tch::Kind::Float, tch::Device::Cpu));

        let seq_len = 4usize;
        let mut identity_mask = vec![false; seq_len * seq_len];
        for i in 0..seq_len {
            identity_mask[i * seq_len + i] = true;
        }
        let mask = Tensor::from_slice(&identity_mask).view([1, 1, seq_len as i64, seq_len as i64]);

        let masked_output = attention.forward(&hidden, Some(&mask), 0.0, false);
        let full_output = attention.forward(&hidden, None, 0.0, false);
        assert!(!masked_output.allclose(&full_output, 1e-6, 1e-6, false));
    }

    #[test]
    fn self_attention_dropout_is_a_no_op_when_not_training_but_randomizes_when_training() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let attention = SelfAttention::new(&vs.root(), 16, 4);
        let hidden = Tensor::randn([1, 4, 16], (tch::Kind::Float, tch::Device::Cpu));

        let first = attention.forward(&hidden, None, 0.9, false);
        let second = attention.forward(&hidden, None, 0.9, false);
        assert!(first.allclose(&second, 1e-6, 1e-6, false));

        let third = attention.forward(&hidden, None, 0.9, true);
        let fourth = attention.forward(&hidden, None, 0.9, true);
        assert!(!third.allclose(&fourth, 1e-6, 1e-6, false));
    }

    #[test]
    fn feed_forward_forward_produces_the_expected_shape() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let feed_forward = FeedForward::new(&vs.root(), 16, 64);

        let hidden = Tensor::randn([2, 5, 16], (tch::Kind::Float, tch::Device::Cpu));
        let output = feed_forward.forward(&hidden, 0.0, false);

        // In and out at embedding_dim (16), not feedforward_dim (64) -- the widen-then-narrow
        // shape must round back to the block's own width.
        assert_eq!(output.size(), [2, 5, 16]);
    }

    #[test]
    fn feed_forward_dropout_is_a_no_op_when_not_training_but_randomizes_when_training() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let feed_forward = FeedForward::new(&vs.root(), 16, 64);
        let hidden = Tensor::randn([2, 5, 16], (tch::Kind::Float, tch::Device::Cpu));

        let first = feed_forward.forward(&hidden, 0.9, false);
        let second = feed_forward.forward(&hidden, 0.9, false);
        assert!(first.allclose(&second, 1e-6, 1e-6, false));

        let third = feed_forward.forward(&hidden, 0.9, true);
        let fourth = feed_forward.forward(&hidden, 0.9, true);
        assert!(!third.allclose(&fourth, 1e-6, 1e-6, false));
    }

    #[test]
    fn transformer_block_forward_produces_the_expected_shape() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let block = TransformerBlock::new(&vs.root(), 16, 4, 64);

        let hidden = Tensor::randn([2, 5, 16], (tch::Kind::Float, tch::Device::Cpu));
        let output = block.forward(&hidden, None, 0.0, false);

        assert_eq!(output.size(), [2, 5, 16]);
    }

    #[test]
    fn transformer_block_mask_actually_changes_the_output() {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let block = TransformerBlock::new(&vs.root(), 16, 4, 64);
        let hidden = Tensor::randn([1, 4, 16], (tch::Kind::Float, tch::Device::Cpu));

        let seq_len = 4usize;
        let mut identity_mask = vec![false; seq_len * seq_len];
        for i in 0..seq_len {
            identity_mask[i * seq_len + i] = true;
        }
        let mask = Tensor::from_slice(&identity_mask).view([1, 1, seq_len as i64, seq_len as i64]);

        let masked_output = block.forward(&hidden, Some(&mask), 0.0, false);
        let full_output = block.forward(&hidden, None, 0.0, false);
        assert!(!masked_output.allclose(&full_output, 1e-6, 1e-6, false));
    }

    fn sample_architecture() -> super::super::net_config::Architecture {
        super::super::net_config::Architecture {
            embedding_dim: Some(16),
            value_head: Some(super::super::net_config::ValueHeadConfig { hidden_dim: vec![8, 8] }),
            kind: super::super::net_config::ArchitectureKind::Transformer {
                num_layers: 2, num_heads: 4, feedforward_dim: 32, dropout: 0.0,
            },
        }
    }

    fn sample_batch(attention: super::super::net_config::Attention) -> TransformerBatch {
        use super::super::cache::CacheRecord;
        use kalo_chess::features::CanonicalPosition;
        use kalo_chess::board::castling_rights::CastlingRights;
        use kalo_chess::board::color::Color;
        use kalo_chess::board::square::Square;

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

        let output = network.forward(&batch, false);

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
        use kalo_chess::board::castling_rights::CastlingRights;
        use kalo_chess::board::color::Color;
        use kalo_chess::board::piece_type::PieceType;
        use kalo_chess::board::square::Square;
        use kalo_chess::features::{pack_canonical_piece, CanonicalPosition};

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
            .map(|position| network.forward(&to_transformer_batch(std::slice::from_ref(position), Attention::Full), false).double_value(&[0, 0]))
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

        let together = network.forward(&padded, false);

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

        let output = network.forward(&batch, false);

        assert_eq!(output.size(), [2, 1]);
        let values: Vec<f32> = output.reshape([2]).try_into().unwrap();
        for value in values {
            assert!(value.is_finite(), "expected a finite unbounded value, got {value}");
        }
    }
}
