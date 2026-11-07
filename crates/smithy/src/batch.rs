use tch::{Device, Kind, Tensor};

use super::cache::{label_matrix, CacheRecord};
use super::feature_set::FeatureSet;
use fattorini::features::{halfk, psq, psq_threats, CanonicalPosition};
use fattorini::stack_buckets::{buckets_of_canonical, count_bucket, count_of_canonical, phase_bucket, phase_of_canonical};

/// A psq batch's bucket columns: our king, their king, material, phase, piece count
/// (`DenseBatchPsq::buckets`, `stack_head::HeadShape::stages`).
const BUCKET_COLUMNS: i64 = 5;

/// Every tensor `network::Network::forward` needs for one batch, expanded from a slice of
/// `cache::CacheRecord`s -- the on-disk format stores one canonical (feature-set-agnostic) piece
/// list per position, to keep the feature cache small and reusable across feature sets, so
/// something has to derive both perspectives under whichever feature set the network trains on
/// before a batch can reach the network; this is that step, and nothing else. Not itself a
/// training-loop concern -- reading records off disk (`reader::CacheFile`, sequentially or
/// cyclically) is a separate piece.
pub enum DenseBatch {
    Psq(DenseBatchPsq),
    /// psq's shape at `psq_threats::ROWS` wide.
    PsqThreats(DenseBatchPsq),
    /// psq's shape at `halfk::TRAINING_ROWS` wide: halfk's rows, each piece row's factor row too.
    HalfKav2Hm(DenseBatchPsq),
}

impl DenseBatch {
    /// `[batch, 1]`: arm `arm`'s already-labeled signed advantage in `[-1, 1]`, a column
    /// of the batch's `[batch, arms]` labels, regardless of which variant `self` is -- `round.rs`'s
    /// own training/test loss computation needs this one tensor and nothing else about either
    /// variant's own dense shape. A batch of one-label records has one arm, `0`. Used by the unstacked
    /// `Network`'s losses, the reference the stacked network is tested against.
    #[cfg(test)]
    pub fn label(&self, arm: usize) -> Tensor {
        let labels = match self {
            DenseBatch::Psq(batch) | DenseBatch::PsqThreats(batch) | DenseBatch::HalfKav2Hm(batch) => &batch.label,
        };
        labels.narrow(1, arm as i64, 1)
    }

    /// `[batch, arms]`: every arm's labels, the matrix `label` takes one column of. The stacked
    /// network selects its arms' columns from it at once.
    pub fn labels(&self) -> &Tensor {
        match self {
            DenseBatch::Psq(batch) | DenseBatch::PsqThreats(batch) | DenseBatch::HalfKav2Hm(batch) => &batch.label,
        }
    }

    pub fn feature_set(&self) -> FeatureSet {
        match self {
            DenseBatch::Psq(_) => FeatureSet::Psq,
            DenseBatch::PsqThreats(_) => FeatureSet::PsqThreats,
            DenseBatch::HalfKav2Hm(_) => FeatureSet::HalfKav2Hm,
        }
    }

    /// The batch's active rows and buckets, whichever kind: every kind is psq's form.
    pub fn psq(&self) -> &DenseBatchPsq {
        match self {
            DenseBatch::Psq(batch) | DenseBatch::PsqThreats(batch) | DenseBatch::HalfKav2Hm(batch) => batch,
        }
    }

    /// Every tensor field moved to `device`. Named after `Tensor::to_device`, it consumes the batch,
    /// whose tensors it moves. Training builds its batches on the device already (`psq_on`).
    #[cfg(test)]
    #[allow(clippy::wrong_self_convention)]
    pub fn to_device(self, device: Device) -> DenseBatch {
        match self {
            DenseBatch::Psq(batch) => DenseBatch::Psq(batch.to_device(device)),
            DenseBatch::PsqThreats(batch) => DenseBatch::PsqThreats(batch.to_device(device)),
            DenseBatch::HalfKav2Hm(batch) => DenseBatch::HalfKav2Hm(batch.to_device(device)),
        }
    }
}

/// A psq-form batch (psq, or psq_threats at its width) as its active rows, not dense rows: at most
/// 37 of psq's 780 inputs a perspective are active, and about 60 of psq_threats' 4,876, so a
/// dense transform multiplies mostly zeros. The stacked network sums the active weight columns with
/// one `embedding_bag` instead, both perspectives in one call: the bags are the own rows, then the
/// opponent's. Every active input is exactly 1.0: a row belongs to one square or one square pair,
/// so no two features share one.
pub struct DenseBatchPsq {
    rows: i64,
    width: i64,
    /// `Int64 [nnz]`: every active input's flat index into `[2, rows, width]`, own then opponent,
    /// ascending. Only the dense view (`dense`) reads it.
    flat: Tensor,
    /// `Int64 [nnz]`: `flat`'s feature, the `embedding_bag` indices.
    features: Tensor,
    /// `Int64 [2 * rows]`: where each bag's features start in `features`.
    offsets: Tensor,
    /// `[rows, arms]`.
    pub label: Tensor,
    /// `Int64 [rows, 5]`: each row's buckets, our king, their king, material, phase, piece count
    /// (`fattorini::stack_buckets`). Carried by every psq batch, read only by a bucketed head.
    buckets: Tensor,
}

impl DenseBatchPsq {
    /// From `flat` (as the field: ascending, own then opponent), the labels and the buckets, on
    /// `flat`'s device.
    fn from_flat(rows: i64, width: i64, flat: Tensor, label: Tensor, buckets: Tensor) -> DenseBatchPsq {
        let features = flat.remainder(width);
        let counts = flat.floor_divide_scalar(width).bincount(None::<Tensor>, 2 * rows);
        let offsets = counts.cumsum(0, Kind::Int64) - &counts;
        DenseBatchPsq { rows, width, flat, features, offsets, label, buckets }
    }

    /// From dense 0/1 rows, `[rows, width]` each, for tests that write their inputs by hand; every
    /// row in bucket 0 of each key (`with_buckets` sets others).
    #[cfg(test)]
    pub fn from_dense(own: &Tensor, opponent: &Tensor, label: Tensor) -> DenseBatchPsq {
        let (rows, width) = (own.size()[0], own.size()[1]);
        let both = Tensor::cat(&[own.view([-1]), opponent.view([-1])], 0);
        assert!(both.eq(0.0).logical_or(&both.eq(1.0)).all().int64_value(&[]) == 1, "psq inputs are 0 or 1");
        let buckets = Tensor::zeros([rows, BUCKET_COLUMNS], (Kind::Int64, own.device()));
        DenseBatchPsq::from_flat(rows, width, both.nonzero().view([-1]), label, buckets)
    }

    /// `Int64 [rows, 5]`: each row's buckets, our king, their king, material, phase, piece count.
    pub fn buckets(&self) -> &Tensor {
        &self.buckets
    }

    pub fn rows(&self) -> i64 {
        self.rows
    }

    pub fn features(&self) -> &Tensor {
        &self.features
    }

    pub fn offsets(&self) -> &Tensor {
        &self.offsets
    }

    /// The dense `[rows, width]` rows of both perspectives, own first: what the unstacked `Network`
    /// (export, and the reference of the stacked network's tests) multiplies.
    pub fn dense(&self) -> (Tensor, Tensor) {
        let both = Tensor::zeros([2 * self.rows * self.width], (Kind::Float, self.flat.device()))
            .index_fill(0, &self.flat, 1.0)
            .view([2, self.rows, self.width]);
        (both.get(0), both.get(1))
    }

    #[cfg(test)]
    #[allow(clippy::wrong_self_convention)]
    fn to_device(self, device: Device) -> DenseBatchPsq {
        DenseBatchPsq {
            rows: self.rows,
            width: self.width,
            flat: self.flat.to_device(device),
            features: self.features.to_device(device),
            offsets: self.offsets.to_device(device),
            label: self.label.to_device(device),
            buckets: self.buckets.to_device(device),
        }
    }
}

/// A batch's plain host-side vectors, before any tensor exists: everything that costs CPU time, built
/// off the main thread by `pipeline.rs`'s workers (`host_batch`), then made into tensors on the main
/// thread (`upload`).
pub enum HostBatch {
    /// The flat indices of the active rows (`psq_indices`), the labels, and three `psq_stack`
    /// buckets a row (`fattorini::stack_buckets`).
    Psq { rows: usize, own: Vec<i32>, opponent: Vec<i32>, label: Vec<f32>, buckets: Vec<i32> },
    /// As psq's, at psq_threats' width (`psq_threats_indices`).
    PsqThreats { rows: usize, own: Vec<i32>, opponent: Vec<i32>, label: Vec<f32>, buckets: Vec<i32> },
    /// As psq's, at halfk's training width (`halfk_indices`).
    HalfKav2Hm { rows: usize, own: Vec<i32>, opponent: Vec<i32>, label: Vec<f32>, buckets: Vec<i32> },
}

/// Builds `records`' host-side vectors under `feature_set`: all the CPU work of a batch.
pub fn host_batch(records: &[CacheRecord], feature_set: FeatureSet) -> HostBatch {
    match feature_set {
        FeatureSet::Psq => {
            let (own, opponent, label, buckets) = psq_indices(records);
            HostBatch::Psq { rows: records.len(), own, opponent, label, buckets }
        }
        FeatureSet::PsqThreats => {
            let (own, opponent, label, buckets) = psq_threats_indices(records);
            HostBatch::PsqThreats { rows: records.len(), own, opponent, label, buckets }
        }
        FeatureSet::HalfKav2Hm => {
            let (own, opponent, label, buckets) = halfk_indices(records);
            HostBatch::HalfKav2Hm { rows: records.len(), own, opponent, label, buckets }
        }
    }
}

impl HostBatch {
    /// The bytes its vectors hold, what keeping it costs (`test_cache.rs`).
    pub fn bytes(&self) -> usize {
        match self {
            HostBatch::Psq { own, opponent, label, buckets, .. }
            | HostBatch::PsqThreats { own, opponent, label, buckets, .. }
            | HostBatch::HalfKav2Hm { own, opponent, label, buckets, .. } => {
                (own.len() + opponent.len() + buckets.len()) * std::mem::size_of::<i32>() + label.len() * std::mem::size_of::<f32>()
            }
        }
    }
}

/// Makes `host`'s tensors on `device`. Borrows `host`, so a kept test batch is uploaded again every
/// round (`test_cache.rs`).
pub fn upload(host: &HostBatch, device: Device) -> DenseBatch {
    match host {
        HostBatch::Psq { rows, own, opponent, label, buckets } => {
            DenseBatch::Psq(psq_on(*rows, psq::ROWS, own, opponent, label, buckets, device))
        }
        HostBatch::PsqThreats { rows, own, opponent, label, buckets } => {
            DenseBatch::PsqThreats(psq_on(*rows, psq_threats::ROWS, own, opponent, label, buckets, device))
        }
        HostBatch::HalfKav2Hm { rows, own, opponent, label, buckets } => {
            DenseBatch::HalfKav2Hm(psq_on(*rows, halfk::TRAINING_ROWS, own, opponent, label, buckets, device))
        }
    }
}

/// The psq batch's host half; `psq_on` builds it directly on `device`. The CPU writes only the
/// flat index of every active row (`row * psq::ROWS + feature`, at most 37 per perspective) and
/// copies those. A dense `[batch, 780]` pair is about 96% zeros, so filling it on the CPU and
/// copying it over was most of the cost of a round: about 6.4 MB per training batch of 1024,
/// against about 300 KB of indices.
///
/// The indices cross as i32, half the bytes of i64, and are widened on `device`, since
/// `embedding_bag` takes i64.
fn psq_indices(records: &[CacheRecord]) -> (Vec<i32>, Vec<i32>, Vec<f32>, Vec<i32>) {
    active_indices(records, psq::ROWS, |canonical| {
        let features = psq::expand(canonical);
        (features.own, features.opponent)
    })
}

/// `psq_indices` for psq_threats: the same form at its own width, one in every row a single
/// square or a single square pair, so no two features share a row there either.
fn psq_threats_indices(records: &[CacheRecord]) -> (Vec<i32>, Vec<i32>, Vec<f32>, Vec<i32>) {
    active_indices(records, psq_threats::ROWS, |canonical| {
        let features = psq_threats::expand(canonical);
        (features.own, features.opponent)
    })
}

/// `psq_indices` for halfk, at its training width: halfk's rows, then every piece row's factor row,
/// the same identity and square whatever the king's bucket (`halfk::factor_row`), past the engine's
/// rows. The factor rows are distinct within a perspective as the piece rows are, so every
/// active input is still exactly 1.
fn halfk_indices(records: &[CacheRecord]) -> (Vec<i32>, Vec<i32>, Vec<f32>, Vec<i32>) {
    active_indices(records, halfk::TRAINING_ROWS, |canonical| {
        let features = halfk::expand(canonical);
        (with_factor_rows(features.own), with_factor_rows(features.opponent))
    })
}

/// `rows` with each piece row's factor row added.
fn with_factor_rows(rows: Vec<u16>) -> Vec<u16> {
    let factors: Vec<u16> = rows
        .iter()
        .filter(|&&row| (row as usize) < halfk::HALFK_ROWS)
        .map(|&row| (halfk::ROWS + halfk::factor_row(row as usize)) as u16)
        .collect();
    let mut rows = rows;
    rows.extend(factors);
    rows
}

/// The flat index of every active row of both perspectives (`row * width + feature`), the labels,
/// and each row's five buckets: what `expand` and `buckets_of_canonical` give each record, side to
/// move first, then the phase bucket and the piece-count bucket. The buckets for every psq batch,
/// bucketed or not: five integers a row, cheaper than carrying the architecture into batch building.
fn active_indices(
    records: &[CacheRecord], width: usize, expand: impl Fn(&CanonicalPosition) -> (Vec<u16>, Vec<u16>),
) -> (Vec<i32>, Vec<i32>, Vec<f32>, Vec<i32>) {
    let rows = records.len();
    assert!(
        rows * width <= i32::MAX as usize,
        "a batch of {rows} rows of width {width} has flat indices past i32, which carries them to the device"
    );
    let mut own: Vec<i32> = Vec::new();
    let mut opponent: Vec<i32> = Vec::new();
    let mut buckets: Vec<i32> = Vec::with_capacity(BUCKET_COLUMNS as usize * rows);
    let (label, _) = label_matrix(records);
    for (row, record) in records.iter().enumerate() {
        let (own_rows, opponent_rows) = expand(&record.canonical);
        let offset = (row * width) as i32;
        own.extend(own_rows.iter().map(|&active| offset + active as i32));
        opponent.extend(opponent_rows.iter().map(|&active| offset + active as i32));
        buckets.extend(buckets_of_canonical(&record.canonical).iter().map(|&bucket| bucket as i32));
        buckets.push(phase_bucket(phase_of_canonical(&record.canonical)) as i32);
        buckets.push(count_bucket(count_of_canonical(&record.canonical)) as i32);
    }
    (own, opponent, label, buckets)
}

/// A psq-form batch's tensors on `device` from its flat active indices (`active_indices`), each
/// perspective `width` wide. The opponent's indices are shifted past the own rows, so the two
/// perspectives are one index list of `2 * rows` bags; the features and offsets are derived on
/// `device`.
fn psq_on(
    rows: usize, width: usize, own: &[i32], opponent: &[i32], label: &[f32], buckets: &[i32], device: Device,
) -> DenseBatchPsq {
    let (rows, width) = (rows as i64, width as i64);
    let own = Tensor::from_slice(own).to_device(device).to_kind(Kind::Int64);
    let opponent = Tensor::from_slice(opponent).to_device(device).to_kind(Kind::Int64) + rows * width;
    let label = Tensor::from_slice(label).view([rows, -1]).to_device(device);
    let buckets = Tensor::from_slice(buckets).view([rows, BUCKET_COLUMNS]).to_device(device).to_kind(Kind::Int64);
    DenseBatchPsq::from_flat(rows, width, Tensor::cat(&[own, opponent], 0), label, buckets)
}

/// Expands `records` into one `DenseBatch` on the CPU, for tests.
#[cfg(test)]
pub fn to_dense_batch(records: &[CacheRecord], feature_set: FeatureSet) -> DenseBatch {
    to_dense_batch_on(records, feature_set, Device::Cpu)
}

/// Expands `records` into one `DenseBatch` on `device`: `host_batch`, then `upload`, on the calling
/// thread, for tests; training calls the two apart. psq builds on `device` from active indices
/// (`psq_on`), and so does psq_threats.
#[cfg(test)]
pub fn to_dense_batch_on(records: &[CacheRecord], feature_set: FeatureSet, device: Device) -> DenseBatch {
    upload(&host_batch(records, feature_set), device)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fattorini::features::{pack_canonical_piece, CanonicalPosition};
    use fattorini::board::castling_rights::CastlingRights;
    use fattorini::board::color::Color;
    use fattorini::board::piece_type::PieceType;
    use fattorini::board::square::Square;

    fn sample_record(canonical_pieces: Vec<u16>, white_king: Square, black_king: Square, label: f32) -> CacheRecord {
        let canonical = CanonicalPosition {
            side_to_move: Color::White,
            pieces: canonical_pieces,
            white_king,
            black_king,
            castling_rights: CastlingRights::NONE,
            en_passant_file: None,
        };
        CacheRecord::new(canonical, label)
    }

    #[test]
    fn to_device_moves_every_tensor_field_of_every_variant() {
        let records = vec![sample_record(vec![], Square::E1, Square::E8, 0.5)];

        let DenseBatch::Psq(batch) = to_dense_batch(&records, FeatureSet::Psq).to_device(Device::Cpu) else {
            panic!("expected a Psq batch");
        };
        assert_eq!(batch.flat.device(), Device::Cpu);
        assert_eq!(batch.features.device(), Device::Cpu);
        assert_eq!(batch.offsets.device(), Device::Cpu);
        assert_eq!(batch.label.device(), Device::Cpu);
        assert_eq!(batch.buckets.device(), Device::Cpu);
    }

    #[test]
    fn a_psq_batch_carries_each_rows_stack_buckets_and_counts_their_bytes() {
        // White to move: e1 and e8, a white rook (material 5, phase 2, 3 pieces); a1 and h8, no
        // material (bucket 0 with 1, phase 0, 2 pieces): count bucket 0 both.
        let records = vec![
            sample_record(vec![pack_canonical_piece(Square::A1, PieceType::Rook, Color::White)], Square::E1, Square::E8, 0.5),
            sample_record(vec![], Square::A1, Square::H8, 0.3),
        ];
        let DenseBatch::Psq(batch) = to_dense_batch(&records, FeatureSet::Psq) else {
            panic!("expected a Psq batch");
        };
        let buckets: Vec<i64> = Vec::try_from(batch.buckets().view([-1])).unwrap();
        assert_eq!(buckets, [Square::E1 as i64, Square::E8 as i64, 4, 2, 0, Square::A1 as i64, Square::H8 as i64, 0, 0, 0]);

        let host = host_batch(&records, FeatureSet::Psq);
        let HostBatch::Psq { own, opponent, label, buckets, .. } = &host else { panic!("expected a Psq host batch") };
        assert_eq!(buckets.len(), 10);
        assert_eq!(host.bytes(), (own.len() + opponent.len() + 10) * 4 + label.len() * 4);
    }

    #[test]
    fn to_dense_batch_sets_exactly_the_psq_active_rows_of_each_perspective() {
        // A white pawn on e4, kings on e1/e8, White kingside castling, en passant on the e-file,
        // White to move: own is White's view, opponent Black's (mirrored), both at 1.0 exactly
        // on `psq::active_rows` and 0.0 everywhere else.
        let mut rights = CastlingRights::NONE;
        rights.set_kingside(Color::White, true);
        let canonical = CanonicalPosition {
            side_to_move: Color::White,
            pieces: vec![pack_canonical_piece(Square::E4, PieceType::Pawn, Color::White)],
            white_king: Square::E1,
            black_king: Square::E8,
            castling_rights: rights,
            en_passant_file: Some(4),
        };
        let mut expected_own = vec![0f32; psq::ROWS];
        for row in psq::active_rows(&canonical, Color::White) {
            expected_own[row as usize] = 1.0;
        }
        let mut expected_opponent = vec![0f32; psq::ROWS];
        for row in psq::active_rows(&canonical, Color::Black) {
            expected_opponent[row as usize] = 1.0;
        }
        assert_eq!(expected_own.iter().sum::<f32>(), 5.0, "test's own premise: 3 pieces, 1 castling, 1 en passant");

        let DenseBatch::Psq(batch) = to_dense_batch(&[CacheRecord::new(canonical, 0.0)], FeatureSet::Psq) else {
            panic!("expected a Psq batch");
        };

        let (own_dense, opponent_dense) = batch.dense();
        assert_eq!(own_dense.size(), [1, psq::ROWS as i64]);
        let own: Vec<f32> = own_dense.reshape([-1]).try_into().unwrap();
        let opponent: Vec<f32> = opponent_dense.reshape([-1]).try_into().unwrap();
        assert_eq!(own, expected_own);
        assert_eq!(opponent, expected_opponent);
    }

    /// Positions that between them exercise every kind of psq row: both sides to move, pieces of
    /// both colours, castling rights, en passant.
    fn varied_psq_records() -> Vec<CacheRecord> {
        let mut rights = CastlingRights::NONE;
        rights.set_kingside(Color::White, true);
        rights.set_queenside(Color::Black, true);
        let positions = [
            (Color::White, vec![pack_canonical_piece(Square::E4, PieceType::Pawn, Color::White)], CastlingRights::NONE, None),
            (
                Color::Black,
                vec![
                    pack_canonical_piece(Square::D5, PieceType::Pawn, Color::Black),
                    pack_canonical_piece(Square::E4, PieceType::Pawn, Color::White),
                    pack_canonical_piece(Square::A1, PieceType::Rook, Color::White),
                    pack_canonical_piece(Square::H8, PieceType::Rook, Color::Black),
                ],
                rights,
                Some(4),
            ),
            (
                Color::White,
                vec![
                    pack_canonical_piece(Square::G5, PieceType::Queen, Color::Black),
                    pack_canonical_piece(Square::C3, PieceType::Knight, Color::White),
                ],
                rights,
                Some(0),
            ),
        ];
        positions
            .into_iter()
            .enumerate()
            .map(|(index, (side_to_move, pieces, castling_rights, en_passant_file))| {
                let canonical = CanonicalPosition {
                    side_to_move,
                    pieces,
                    white_king: Square::E1,
                    black_king: Square::E8,
                    castling_rights,
                    en_passant_file,
                };
                CacheRecord::new(canonical, index as f32 / 10.0)
            })
            .collect()
    }

    /// The dense psq batch as it used to be built, one float at a time on the CPU: the reference
    /// the indices route must match exactly.
    fn psq_by_element(records: &[CacheRecord]) -> (Vec<f32>, Vec<f32>) {
        let mut own = vec![0f32; records.len() * psq::ROWS];
        let mut opponent = vec![0f32; records.len() * psq::ROWS];
        for (row, record) in records.iter().enumerate() {
            let features = psq::expand(&record.canonical);
            for &active in &features.own {
                own[row * psq::ROWS + active as usize] = 1.0;
            }
            for &active in &features.opponent {
                opponent[row * psq::ROWS + active as usize] = 1.0;
            }
        }
        (own, opponent)
    }

    #[test]
    fn psq_from_indices_matches_the_element_by_element_fill_exactly() {
        let records = varied_psq_records();
        let (expected_own, expected_opponent) = psq_by_element(&records);

        let DenseBatch::Psq(batch) = to_dense_batch(&records, FeatureSet::Psq) else {
            panic!("expected a Psq batch");
        };

        let (own_dense, opponent_dense) = batch.dense();
        assert_eq!(own_dense.size(), [records.len() as i64, psq::ROWS as i64]);
        let own: Vec<f32> = own_dense.reshape([-1]).try_into().unwrap();
        let opponent: Vec<f32> = opponent_dense.reshape([-1]).try_into().unwrap();
        assert_eq!(own, expected_own);
        assert_eq!(opponent, expected_opponent);
        let labels: Vec<f32> = batch.label.view([-1]).try_into().unwrap();
        assert_eq!(labels, vec![0.0, 0.1, 0.2]);
    }

    #[test]
    fn psq_threats_from_indices_matches_the_element_by_element_fill_exactly() {
        let records = varied_psq_records();
        let width = psq_threats::ROWS;
        let mut expected_own = vec![0f32; records.len() * width];
        let mut expected_opponent = vec![0f32; records.len() * width];
        for (row, record) in records.iter().enumerate() {
            let features = psq_threats::expand(&record.canonical);
            for active in features.own {
                expected_own[row * width + active as usize] = 1.0;
            }
            for active in features.opponent {
                expected_opponent[row * width + active as usize] = 1.0;
            }
        }
        assert!(
            expected_own.iter().chain(&expected_opponent).filter(|&&value| value == 1.0).count()
                > psq_by_element(&records).0.iter().chain(&psq_by_element(&records).1).filter(|&&value| value == 1.0).count(),
            "the records hold no threat, so the threat rows go untested"
        );

        let DenseBatch::PsqThreats(batch) = to_dense_batch(&records, FeatureSet::PsqThreats) else {
            panic!("expected a PsqThreats batch");
        };

        let (own_dense, opponent_dense) = batch.dense();
        assert_eq!(own_dense.size(), [records.len() as i64, width as i64]);
        let own: Vec<f32> = own_dense.reshape([-1]).try_into().unwrap();
        let opponent: Vec<f32> = opponent_dense.reshape([-1]).try_into().unwrap();
        assert_eq!(own, expected_own);
        assert_eq!(opponent, expected_opponent);
        let labels: Vec<f32> = batch.label.view([-1]).try_into().unwrap();
        assert_eq!(labels, vec![0.0, 0.1, 0.2]);
    }

    #[test]
    fn psq_built_on_cuda_equals_psq_built_on_the_cpu() {
        if !tch::Cuda::is_available() {
            return;
        }
        let records = varied_psq_records();
        let DenseBatch::Psq(cpu) = to_dense_batch(&records, FeatureSet::Psq) else {
            panic!("expected a Psq batch");
        };
        let DenseBatch::Psq(cuda) = to_dense_batch_on(&records, FeatureSet::Psq, Device::Cuda(0)) else {
            panic!("expected a Psq batch");
        };

        assert_eq!(cuda.features.device(), Device::Cuda(0));
        assert!(cuda.flat.to_device(Device::Cpu).equal(&cpu.flat));
        assert!(cuda.features.to_device(Device::Cpu).equal(&cpu.features));
        assert!(cuda.offsets.to_device(Device::Cpu).equal(&cpu.offsets));
        assert!(cuda.label.to_device(Device::Cpu).equal(&cpu.label));
    }

    #[test]
    fn each_bag_holds_exactly_its_rows_active_features() {
        // Bag `row` is the own perspective of `row`, bag `rows + row` its opponent's; a position
        // with no threats and the first and last rows are covered by `varied_psq_records`.
        // Halfk's bags hold its rows, then every piece row's factor row.
        for feature_set in [FeatureSet::Psq, FeatureSet::PsqThreats, FeatureSet::HalfKav2Hm] {
            let records = varied_psq_records();
            let batch = to_dense_batch(&records, feature_set);
            let batch = batch.psq();
            let features: Vec<i64> = Vec::try_from(&batch.features).unwrap();
            let offsets: Vec<i64> = Vec::try_from(&batch.offsets).unwrap();
            assert_eq!(offsets.len(), 2 * records.len());
            let mut expected: Vec<Vec<i64>> = Vec::new();
            for perspective in 0..2 {
                for record in &records {
                    let (own, opponent) = match feature_set {
                        FeatureSet::Psq => {
                            let features = psq::expand(&record.canonical);
                            (features.own, features.opponent)
                        }
                        FeatureSet::PsqThreats => {
                            let features = psq_threats::expand(&record.canonical);
                            (features.own, features.opponent)
                        }
                        FeatureSet::HalfKav2Hm => {
                            let features = halfk::expand(&record.canonical);
                            let with_factors = |rows: Vec<u16>| {
                                let pieces: Vec<u16> = rows.iter().copied().filter(|&row| (row as usize) < halfk::HALFK_ROWS).collect();
                                assert!(pieces.len() >= 2, "both kings at least");
                                let factors = pieces.iter().map(|&row| (halfk::ROWS + row as usize % halfk::FACTOR_ROWS) as u16);
                                rows.iter().copied().chain(factors).collect::<Vec<u16>>()
                            };
                            (with_factors(features.own), with_factors(features.opponent))
                        }
                    };
                    let rows = if perspective == 0 { own } else { opponent };
                    expected.push(rows.iter().map(|&row| row as i64).collect());
                }
            }
            for (bag, expected) in expected.iter().enumerate() {
                let end = offsets.get(bag + 1).map_or(features.len(), |&end| end as usize);
                let mut got = features[offsets[bag] as usize..end].to_vec();
                let mut expected = expected.clone();
                got.sort();
                expected.sort();
                assert_eq!(got, expected, "{feature_set:?} bag {bag}");
            }
        }
    }

    #[test]
    fn to_dense_batch_carries_the_label_through_unchanged() {
        let records = vec![sample_record(vec![], Square::A1, Square::H8, 0.42)];
        let DenseBatch::Psq(batch) = to_dense_batch(&records, FeatureSet::Psq) else {
            panic!("expected a Psq batch");
        };

        let expected_label = Tensor::from_slice(&[0.42f32]).view([1, 1]);
        assert!(batch.label.allclose(&expected_label, 1e-6, 1e-6, false));
    }
}
