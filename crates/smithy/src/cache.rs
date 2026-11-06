//! One training record, as every stream and test set hands it to a batch: a position and its labels;
//! and the on-disk formats of a frozen test set's positions and labels (`freeze.rs`).

use std::io::{Read, Write};

use kalo_chess::board::castling_rights::CastlingRights;
use kalo_chess::board::color::Color;
use kalo_chess::board::square::Square;
use kalo_chess::features::CanonicalPosition;

/// The maximum number of non-king pieces one canonical position's `pieces` list can ever have: a
/// legal position starts with exactly 32 pieces total (16 per side, including 2 kings, which are
/// never part of this list at all) and only loses pieces to captures, never gains beyond that --
/// a hard, well-known bound, not a guess.
pub const MAX_NON_KING_PIECES: usize = 30;

/// Identifies a positions file, and as which version of this format -- checked on read so an
/// incompatible or corrupt file fails loudly instead of silently misinterpreting its bytes.
const MAGIC: [u8; 4] = *b"CHFC";
/// 4: the row count left the header for a sidecar, now the frozen set's manifest.
const FORMAT_VERSION: u32 = 4;

/// A positions file's header. No row count: the frozen set's manifest holds it.
pub struct CacheHeader;

impl CacheHeader {
    pub fn write(&self, writer: &mut impl Write) {
        writer.write_all(&MAGIC).expect("failed to write the cache magic bytes");
        writer.write_all(&FORMAT_VERSION.to_le_bytes()).expect("failed to write the format version");
        writer.write_all(&[MAX_NON_KING_PIECES as u8]).expect("failed to write max non-king pieces");
    }

    pub fn read(reader: &mut impl Read) -> CacheHeader {
        let mut magic = [0u8; 4];
        reader.read_exact(&mut magic).expect("failed to read the cache magic bytes");
        assert_eq!(magic, MAGIC, "not a smithy feature cache file (bad magic bytes)");

        let mut version_bytes = [0u8; 4];
        reader.read_exact(&mut version_bytes).expect("failed to read the format version");
        let version = u32::from_le_bytes(version_bytes);
        assert_eq!(
            version, FORMAT_VERSION,
            "feature cache format version mismatch: file has {version}, this build expects {FORMAT_VERSION}"
        );

        let mut max_pieces = [0u8; 1];
        reader.read_exact(&mut max_pieces).expect("failed to read max non-king pieces");
        assert_eq!(
            max_pieces[0] as usize,
            MAX_NON_KING_PIECES,
            "feature cache built with a different MAX_NON_KING_PIECES than this build's {MAX_NON_KING_PIECES}"
        );
        CacheHeader
    }
}

/// Identifies a label file: one lambda's labels, one `f32` per position of the positions file beside
/// it, in the same order, with no position data at all.
const LABEL_MAGIC: [u8; 4] = *b"CHFL";
const LABEL_FORMAT_VERSION: u32 = 1;

/// A label file's header: its format, and the lambda its labels were computed at, so a file read
/// for one lambda can be checked to be that lambda's rather than trusted by its path alone.
pub struct LabelHeader {
    pub lambda: f64,
}

impl LabelHeader {
    pub fn write(&self, writer: &mut impl Write) {
        writer.write_all(&LABEL_MAGIC).expect("failed to write the label file magic bytes");
        writer.write_all(&LABEL_FORMAT_VERSION.to_le_bytes()).expect("failed to write the label format version");
        writer.write_all(&self.lambda.to_le_bytes()).expect("failed to write the label file's lambda");
    }

    pub fn read(reader: &mut impl Read) -> LabelHeader {
        let mut magic = [0u8; 4];
        reader.read_exact(&mut magic).expect("failed to read the label file magic bytes");
        assert_eq!(magic, LABEL_MAGIC, "not a smithy label file (bad magic bytes)");
        let mut version_bytes = [0u8; 4];
        reader.read_exact(&mut version_bytes).expect("failed to read the label format version");
        let version = u32::from_le_bytes(version_bytes);
        assert_eq!(
            version, LABEL_FORMAT_VERSION,
            "label file format version mismatch: file has {version}, this build expects {LABEL_FORMAT_VERSION}"
        );
        let mut lambda_bytes = [0u8; 8];
        reader.read_exact(&mut lambda_bytes).expect("failed to read the label file's lambda");
        LabelHeader { lambda: f64::from_le_bytes(lambda_bytes) }
    }
}

/// One label, 4 bytes, `f32`.
pub fn write_label(writer: &mut impl Write, label: f32) {
    writer.write_all(&label.to_le_bytes()).expect("failed to write a label");
}

pub fn read_label(reader: &mut impl Read) -> f32 {
    let mut bytes = [0u8; 4];
    reader.read_exact(&mut bytes).expect("failed to read a label");
    f32::from_le_bytes(bytes)
}

/// One position's on-disk encoding, with no label alongside it: a side-to-move byte, a
/// variable-length canonical piece list (a count byte plus that many packed `u16`s, no padding), both
/// kings' absolute squares (1 byte each), then one byte packing castling rights and the en passant
/// file together (bits 0-3 the four castling rights, bit 4 an en-passant-present flag, bits 5-7 the
/// en passant file when present).
pub fn write_position(writer: &mut impl Write, canonical: &CanonicalPosition) {
    writer.write_all(&[canonical.side_to_move as u8]).expect("failed to write side to move");
    write_pieces(writer, &canonical.pieces);
    writer
        .write_all(&[canonical.white_king as u8, canonical.black_king as u8])
        .expect("failed to write both kings' squares");
    let packed = encode_castling_and_en_passant(canonical.castling_rights, canonical.en_passant_file);
    writer.write_all(&[packed]).expect("failed to write castling rights and en passant file");
}

fn write_pieces(writer: &mut impl Write, pieces: &[u16]) {
    assert!(
        pieces.len() <= MAX_NON_KING_PIECES,
        "more than {MAX_NON_KING_PIECES} non-king pieces on the board: {}",
        pieces.len()
    );
    writer.write_all(&[pieces.len() as u8]).expect("failed to write a piece count");
    for &packed in pieces {
        writer.write_all(&packed.to_le_bytes()).expect("failed to write a packed piece");
    }
}

/// Packs `rights` (via its public `kingside`/`queenside` accessors, since its internal byte is
/// private to `kalo_chess::board`) into bits 0-3, White-kingside/White-queenside/Black-kingside/
/// Black-queenside, then folds in `en_passant_file` as bit 4 (present flag) plus bits 5-7 (the file
/// itself, only meaningful when bit 4 is set).
fn encode_castling_and_en_passant(rights: CastlingRights, en_passant_file: Option<u8>) -> u8 {
    let mut byte = 0u8;
    if rights.kingside(Color::White) {
        byte |= 1 << 0;
    }
    if rights.queenside(Color::White) {
        byte |= 1 << 1;
    }
    if rights.kingside(Color::Black) {
        byte |= 1 << 2;
    }
    if rights.queenside(Color::Black) {
        byte |= 1 << 3;
    }
    if let Some(file) = en_passant_file {
        assert!(file < 8, "en passant file out of range: {file}");
        byte |= 1 << 4;
        byte |= file << 5;
    }
    byte
}

/// The exact inverse of `encode_castling_and_en_passant`.
fn decode_castling_and_en_passant(byte: u8) -> (CastlingRights, Option<u8>) {
    let mut rights = CastlingRights::NONE;
    rights.set_kingside(Color::White, byte & (1 << 0) != 0);
    rights.set_queenside(Color::White, byte & (1 << 1) != 0);
    rights.set_kingside(Color::Black, byte & (1 << 2) != 0);
    rights.set_queenside(Color::Black, byte & (1 << 3) != 0);

    let en_passant_file = if byte & (1 << 4) != 0 { Some((byte >> 5) & 0b111) } else { None };

    (rights, en_passant_file)
}

/// The read-side counterpart to `write_position`.
pub fn read_position(reader: &mut impl Read) -> CanonicalPosition {
    let mut side_byte = [0u8; 1];
    reader.read_exact(&mut side_byte).expect("failed to read side to move");
    let side_to_move = Color::try_from(side_byte[0]).expect("bad side-to-move byte in a feature cache record");

    let pieces = read_pieces(reader);

    let mut kings = [0u8; 2];
    reader.read_exact(&mut kings).expect("failed to read both kings' squares");
    let white_king = Square::try_from(kings[0]).expect("bad white king square in a feature cache record");
    let black_king = Square::try_from(kings[1]).expect("bad black king square in a feature cache record");

    let mut packed_byte = [0u8; 1];
    reader.read_exact(&mut packed_byte).expect("failed to read castling rights and en passant file");
    let (castling_rights, en_passant_file) = decode_castling_and_en_passant(packed_byte[0]);

    CanonicalPosition { side_to_move, pieces, white_king, black_king, castling_rights, en_passant_file }
}

fn read_pieces(reader: &mut impl Read) -> Vec<u16> {
    let mut count_byte = [0u8; 1];
    reader.read_exact(&mut count_byte).expect("failed to read a piece count");
    let count = count_byte[0] as usize;
    assert!(count <= MAX_NON_KING_PIECES, "piece count {count} exceeds MAX_NON_KING_PIECES");

    let mut pieces = Vec::with_capacity(count);
    for _ in 0..count {
        let mut value_bytes = [0u8; 2];
        reader.read_exact(&mut value_bytes).expect("failed to read a packed piece");
        pieces.push(u16::from_le_bytes(value_bytes));
    }
    pieces
}

/// One record. Embeds the same `CanonicalPosition` each feature set's own `expand` consumes, rather
/// than duplicating its fields, so `batch.rs` can hand `&record.canonical` straight to it, no adapter
/// step.
///
/// `labels` holds one label per arm when every lambda of a generation trains in lockstep over one
/// stream, in arm order; a single lambda's test set gives exactly one.
#[derive(Clone)]
pub struct CacheRecord {
    pub canonical: CanonicalPosition,
    pub labels: Vec<f32>,
    /// Where the position stands in its game, for a label blended by progress: set by the PGN stream
    /// and kept in the buckets; zero (`Progress::UNKNOWN`) wherever nothing needs it.
    pub progress: Progress,
}

/// The i-th of a game's n searched positions, counted from the book exit (`polyline`). n is 0 when
/// unknown.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Progress {
    pub i: u16,
    pub n: u16,
}

impl Progress {
    pub const UNKNOWN: Progress = Progress { i: 0, n: 0 };
}

impl CacheRecord {
    /// A record with one label.
    #[cfg(test)]
    pub fn new(canonical: CanonicalPosition, label: f32) -> CacheRecord {
        CacheRecord { canonical, labels: vec![label], progress: Progress::UNKNOWN }
    }

    /// The label of a one-label record. Panics on a lockstep record, whose callers must say which
    /// arm's label they mean.
    #[cfg(test)]
    pub fn label(&self) -> f32 {
        assert_eq!(self.labels.len(), 1, "a record of {} labels has no single label", self.labels.len());
        self.labels[0]
    }
}

/// Whether `position` is K vs K: no piece beside the kings (`pieces` holds every other one). The
/// search scores it as a draw by insufficient material before evaluating, so no network ever sees
/// it: the grouped test set refuses it, and the transformer's training stream leaves it out. It
/// still occurs in the games, as the final position of a game whose last capture left the kings alone.
pub fn is_bare_kings(position: &CanonicalPosition) -> bool {
    position.pieces.is_empty()
}

/// `records`' labels as a row-major `[rows, arms]` matrix, and `arms`: a batch's labels, one column
/// per arm. Every record must carry the same number of labels.
pub fn label_matrix(records: &[CacheRecord]) -> (Vec<f32>, usize) {
    let arms = records.first().map_or(1, |record| record.labels.len());
    let mut matrix = Vec::with_capacity(records.len() * arms);
    for record in records {
        assert_eq!(record.labels.len(), arms, "a batch mixes records of {arms} and {} labels", record.labels.len());
        matrix.extend_from_slice(&record.labels);
    }
    (matrix, arms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_round_trips() {
        let mut buffer = Vec::new();
        CacheHeader.write(&mut buffer);
        CacheHeader::read(&mut buffer.as_slice());
    }

    #[test]
    fn a_label_file_header_and_its_labels_round_trip() {
        let mut buffer = Vec::new();
        LabelHeader { lambda: 0.65 }.write(&mut buffer);
        write_label(&mut buffer, -0.25);
        write_label(&mut buffer, 1.0);
        let mut reader = buffer.as_slice();
        assert_eq!(LabelHeader::read(&mut reader).lambda, 0.65);
        assert_eq!(read_label(&mut reader), -0.25);
        assert_eq!(read_label(&mut reader), 1.0);
        assert!(reader.is_empty());
    }

    #[test]
    #[should_panic(expected = "not a smithy label file")]
    fn a_positions_file_is_not_a_label_file() {
        let mut buffer = Vec::new();
        CacheHeader.write(&mut buffer);
        LabelHeader::read(&mut buffer.as_slice());
    }

    #[test]
    #[should_panic(expected = "bad magic bytes")]
    fn header_read_rejects_wrong_magic() {
        let garbage = [0u8; 32];
        CacheHeader::read(&mut garbage.as_slice());
    }

    fn sample_canonical(pieces: Vec<u16>, white_king: Square, black_king: Square) -> CanonicalPosition {
        CanonicalPosition {
            side_to_move: Color::White,
            pieces,
            white_king,
            black_king,
            castling_rights: CastlingRights::NONE,
            en_passant_file: None,
        }
    }

    #[test]
    fn position_round_trips_with_a_full_30_piece_position() {
        let canonical = sample_canonical((0..30).collect(), Square::E1, Square::E8);
        let mut buffer = Vec::new();
        write_position(&mut buffer, &canonical);
        let read_back = read_position(&mut buffer.as_slice());
        assert_eq!(read_back.pieces, canonical.pieces);
        assert_eq!(read_back.white_king, Square::E1);
        assert_eq!(read_back.black_king, Square::E8);
        // side (1) + count (1) + 30 pieces * 2 bytes + kings (2) + castling/en passant (1)
        assert_eq!(buffer.len(), 65);
    }

    #[test]
    fn position_round_trips_castling_rights_and_en_passant_file() {
        let mut rights = CastlingRights::NONE;
        rights.set_kingside(Color::White, true);
        rights.set_queenside(Color::Black, true);
        let mut canonical = sample_canonical(vec![], Square::A1, Square::H8);
        canonical.castling_rights = rights;
        canonical.en_passant_file = Some(4);

        let mut buffer = Vec::new();
        write_position(&mut buffer, &canonical);
        let read_back = read_position(&mut buffer.as_slice());

        assert!(read_back.castling_rights.kingside(Color::White));
        assert!(!read_back.castling_rights.queenside(Color::White));
        assert!(!read_back.castling_rights.kingside(Color::Black));
        assert!(read_back.castling_rights.queenside(Color::Black));
        assert_eq!(read_back.en_passant_file, Some(4));
    }

    #[test]
    #[should_panic(expected = "more than 30 non-king pieces")]
    fn write_position_rejects_too_many_pieces() {
        let canonical = sample_canonical((0..31).collect(), Square::A1, Square::H8);
        let mut buffer = Vec::new();
        write_position(&mut buffer, &canonical);
    }
}
