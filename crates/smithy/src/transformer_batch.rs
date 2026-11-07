//! The last piece of step 3 (the transformer plan's Plan): turning a batch of
//! same-length `cache::CacheRecord`s -- already guaranteed same-length by construction, since a
//! batch only ever comes from one of `length_cache.rs`'s `31` piece-count buckets -- into the
//! tensors `TransformerNetwork::forward` (step 4) will actually consume. The transformer's own
//! standalone batch type (design point H): no `DenseBatch` variant, no `FeatureSet` dependency.

use tch::{Device, Tensor};

use super::cache::{label_matrix, CacheRecord};
use super::net_config::Attention;
use super::transformer_vocab::{is_king_token, token_sequence};

/// `tokens`: `[batch, seq_len]`, `Kind::Int64`, ready for an embedding lookup -- `seq_len` itself
/// isn't stored anywhere separately, it's just `tokens.size()[1]`. `attention_mask`: `None` for
/// full attention (every pair is allowed, so there is nothing to restrict -- cheaper than building
/// an all-`true` tensor just to pass it to `Tensor::scaled_dot_product_attention` for no reason),
/// `Some([batch, 1, seq_len, seq_len], Kind::Bool)` for kings-only, one full `seq_len * seq_len`
/// mask *per record*, not shared across the batch (`build_kings_only_mask`'s own doc comment
/// covers why: the opponent king's own sequence position varies per record even within one
/// piece-count bucket). The leading `1` (not folded away) is the attention-heads axis, sized so it
/// broadcasts against `scaled_dot_product_attention`'s own `[batch, heads, seq_len, seq_len]`
/// score tensor without silently misaligning against the batch axis instead. `labels`:
/// `[batch, arms]`, column `k` the `k`-th label of every record, one column per lambda when they
/// train in lockstep, as a `DenseBatch`'s labels are.
pub struct TransformerBatch {
    pub tokens: Tensor,
    pub attention_mask: Option<Tensor>,
    pub labels: Tensor,
}

impl TransformerBatch {
    /// `to_transformer_batch` always builds on the CPU (`Tensor::from_slice`'s own default
    /// device), regardless of where the network's own weights live -- this moves every tensor
    /// field over to `device` so a training step never mixes a CPU batch against CUDA weights
    /// (libtorch rejects that outright rather than silently copying across devices for you).
    #[cfg(test)]
    #[allow(clippy::wrong_self_convention)]
    pub fn to_device(self, device: Device) -> TransformerBatch {
        TransformerBatch {
            tokens: self.tokens.to_device(device),
            attention_mask: self.attention_mask.map(|mask| mask.to_device(device)),
            labels: self.labels.to_device(device),
        }
    }
}

/// A `TransformerBatch`'s contents before any tensor exists: what `host_batch` builds on a worker
/// thread and `upload` turns into tensors on the training thread (`round::BatchBuilder`), and what
/// the test cache keeps between rounds.
pub struct HostTransformerBatch {
    rows: usize,
    seq_len: usize,
    tokens: Vec<i64>,
    mask: Option<Vec<bool>>,
    labels: Vec<f32>,
    arms: usize,
}

impl HostTransformerBatch {
    /// Its size in memory, for the test cache's budget.
    pub fn bytes(&self) -> usize {
        let mask = self.mask.as_ref().map_or(0, |mask| mask.len());
        self.tokens.len() * std::mem::size_of::<i64>() + mask + self.labels.len() * std::mem::size_of::<f32>()
    }
}

/// `to_transformer_batch`'s CPU half, with no tensor.
pub fn host_batch(records: &[CacheRecord], attention: Attention) -> HostTransformerBatch {
    assert!(!records.is_empty(), "cannot build a transformer batch from zero records");
    let rows = records.len();

    let sequences: Vec<Vec<usize>> = records.iter().map(|record| token_sequence(&record.canonical)).collect();
    let seq_len = sequences[0].len();
    for (row, sequence) in sequences.iter().enumerate() {
        assert_eq!(
            sequence.len(), seq_len,
            "record {row} has a {}-token sequence, expected {seq_len} -- every record in one batch must come from \
             the same piece-count bucket", sequence.len()
        );
    }

    let mut tokens = vec![0i64; rows * seq_len];
    for (row, sequence) in sequences.iter().enumerate() {
        for (col, &vocabulary_index) in sequence.iter().enumerate() {
            tokens[row * seq_len + col] = vocabulary_index as i64;
        }
    }
    let mask = match attention {
        Attention::Full => None,
        Attention::KingsOnly => Some(kings_only_mask(&sequences, seq_len)),
    };
    let (labels, arms) = label_matrix(records);
    HostTransformerBatch { rows, seq_len, tokens, mask, labels, arms }
}

/// `host`'s tensors, on `device`.
pub fn upload(host: &HostTransformerBatch, device: Device) -> TransformerBatch {
    let (rows, seq_len) = (host.rows as i64, host.seq_len as i64);
    TransformerBatch {
        tokens: Tensor::from_slice(&host.tokens).view([rows, seq_len]).to_device(device),
        attention_mask: host.mask.as_ref().map(|mask| Tensor::from_slice(mask).view([rows, 1, seq_len, seq_len]).to_device(device)),
        labels: Tensor::from_slice(&host.labels).view([rows, host.arms as i64]).to_device(device),
    }
}

/// Builds one `TransformerBatch` from `records` -- every record must share the same token-sequence
/// length (guaranteed by the length-bucketed cache files this is always fed from in practice, and
/// checked here regardless, per CLAUDE.md rule 4).
#[cfg(test)]
pub fn to_transformer_batch(records: &[CacheRecord], attention: Attention) -> TransformerBatch {
    upload(&host_batch(records, attention), Device::Cpu)
}

/// One `[batch, 1, seq_len, seq_len]` boolean mask, row-major, `allowed[b, 0, i, j] = is_king(sequences[b][i])
/// || is_king(sequences[b][j])` -- the transformer design's own settled rule ("zeroes out every
/// pair that doesn't include a king row or column"). Built per record, not once for the whole
/// batch and broadcast: the own king always sits at sequence position `1` (right after `[CLS]`,
/// `K < Q < R < B < N < P` puts it first in its own block), but the *opponent* king's position is
/// `1 + (own side's own token count)`, which varies from record to record even within one
/// piece-count bucket -- two records with the same *total* piece count can still split it
/// differently between own and opponent (`12` own `+ 8` opponent vs. `6` own `+ 14` opponent both
/// total `20`). So which two positions are "a king row or column" is genuinely per-record
/// information, not a fixed shape this function could compute once and reuse.
fn kings_only_mask(sequences: &[Vec<usize>], seq_len: usize) -> Vec<bool> {
    let batch_size = sequences.len();
    let mut mask_buffer = vec![false; batch_size * seq_len * seq_len];
    for (row, sequence) in sequences.iter().enumerate() {
        let is_king: Vec<bool> = sequence.iter().map(|&vocabulary_index| is_king_token(vocabulary_index)).collect();
        for i in 0..seq_len {
            for j in 0..seq_len {
                mask_buffer[row * seq_len * seq_len + i * seq_len + j] = is_king[i] || is_king[j];
            }
        }
    }
    mask_buffer
}

#[cfg(test)]
mod tests {
    use super::*;
    use fattorini::features::{extract_canonical, CanonicalPosition};
    use fattorini::board::castling_rights::CastlingRights;
    use fattorini::board::color::Color;
    use fattorini::board::square::Square;
    use fattorini::board::Board;
    use tch::Kind;

    const STARTING_POSITION_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

    fn sample_record(pieces: Vec<u16>, label: f32) -> CacheRecord {
        let canonical = CanonicalPosition {
            side_to_move: Color::White,
            pieces,
            white_king: Square::E1,
            black_king: Square::E8,
            castling_rights: CastlingRights::NONE,
            en_passant_file: None,
        };
        CacheRecord::new(canonical, label)
    }

    #[test]
    fn to_transformer_batch_shapes_tokens_and_label_for_full_attention() {
        let records = vec![sample_record(vec![], 0.5), sample_record(vec![], 0.25)];
        let batch = to_transformer_batch(&records, Attention::Full);

        // [CLS] + 1 king each side = 3 tokens, for a bare-kings position.
        assert_eq!(batch.tokens.size(), [2, 3]);
        assert_eq!(batch.labels.size(), [2, 1]);
        assert!(batch.attention_mask.is_none());
    }

    #[test]
    fn to_transformer_batch_shapes_the_mask_for_kings_only_attention() {
        let records = vec![sample_record(vec![], 0.5)];
        let batch = to_transformer_batch(&records, Attention::KingsOnly);

        let mask = batch.attention_mask.expect("kings-only must produce a mask");
        assert_eq!(mask.size(), [1, 1, 3, 3]);
        assert_eq!(mask.kind(), Kind::Bool);
    }

    #[test]
    #[should_panic(expected = "must come from the same piece-count bucket")]
    fn to_transformer_batch_rejects_records_with_different_sequence_lengths() {
        // Packed values 2/12/22 all decode to a White Knight (type_and_color 2) on squares
        // A1/A2/A3 -- deliberately not pawns, which `pawn_token_index` would reject outright on
        // ranks 1/8 (0/12/22 % 10 == 2, / 10 == 0/1/2), unrelated to the length check this test
        // actually exercises.
        let records = vec![sample_record(vec![], 0.5), sample_record(vec![2, 12, 22], 0.5)];
        let _ = to_transformer_batch(&records, Attention::Full);
    }

    #[test]
    fn to_device_moves_every_tensor_field_and_preserves_an_absent_mask() {
        let records = vec![sample_record(vec![], 0.5)];
        let batch = to_transformer_batch(&records, Attention::Full).to_device(tch::Device::Cpu);

        assert_eq!(batch.tokens.device(), tch::Device::Cpu);
        assert_eq!(batch.labels.device(), tch::Device::Cpu);
        assert!(batch.attention_mask.is_none());
    }

    #[test]
    fn to_device_moves_the_attention_mask_when_present() {
        let records = vec![sample_record(vec![], 0.5)];
        let batch = to_transformer_batch(&records, Attention::KingsOnly).to_device(tch::Device::Cpu);

        assert_eq!(batch.attention_mask.expect("kings-only must produce a mask").device(), tch::Device::Cpu);
    }

    #[test]
    fn to_transformer_batch_carries_the_label_through_unchanged() {
        let records = vec![sample_record(vec![], 0.42)];
        let batch = to_transformer_batch(&records, Attention::Full);

        let expected = Tensor::from_slice(&[0.42f32]).view([1, 1]);
        assert!(batch.labels.allclose(&expected, 1e-6, 1e-6, false));
    }

    #[test]
    fn a_lockstep_batch_has_one_label_column_per_lambda() {
        let record = |labels: Vec<f32>| CacheRecord { labels, ..sample_record(vec![], 0.0) };
        let records = vec![record(vec![0.1, 0.2, 0.3]), record(vec![0.4, 0.5, 0.6])];
        let host = host_batch(&records, Attention::KingsOnly);

        let batch = upload(&host, tch::Device::Cpu);

        let expected = Tensor::from_slice(&[0.1f32, 0.2, 0.3, 0.4, 0.5, 0.6]).view([2, 3]);
        assert!(batch.labels.allclose(&expected, 1e-6, 1e-6, false));
        // Two rows of 3 tokens: 6 tokens, 18 mask entries, 6 labels.
        assert_eq!(host.bytes(), 6 * 8 + 18 + 6 * 4);
    }

    #[test]
    fn kings_only_mask_matches_the_settled_row_or_column_rule_on_the_starting_position() {
        let (board, _) = Board::from_fen(STARTING_POSITION_FEN).unwrap();
        let canonical = extract_canonical(&board);
        let batch = to_transformer_batch(&[CacheRecord::new(canonical, 0.0)], Attention::KingsOnly);
        let mask = batch.attention_mask.unwrap().to_kind(Kind::Int64);

        // White to move, unmirrored: own (White) king at position 1, own side has 16 tokens
        // (K<Q<R<R<B<B<N<N<8 pawns, local order), so Black's king sits at position 1 + 16 = 17.
        // Position 5 (own side, local index 4 = first bishop) and position 20 (opponent side,
        // local index 3 = second rook) are both ordinary, non-king positions.
        assert_eq!(mask.int64_value(&[0, 0, 1, 5]), 1); // own king row -> always allowed
        assert_eq!(mask.int64_value(&[0, 0, 5, 1]), 1); // own king column -> always allowed
        assert_eq!(mask.int64_value(&[0, 0, 17, 20]), 1); // opponent king row
        assert_eq!(mask.int64_value(&[0, 0, 5, 17]), 1); // opponent king column
        assert_eq!(mask.int64_value(&[0, 0, 1, 17]), 1); // king-king pair
        assert_eq!(mask.int64_value(&[0, 0, 5, 20]), 0); // two ordinary, non-king positions
    }
}
