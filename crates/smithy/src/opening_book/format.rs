//! A fragment's file: the positions one fragment expanded, each with its evaluation and its moves.
//! Little-endian.
//!
//! Header: magic `FBKF` (4), format version (2), fragment index (2), budget (4, f32), charge (4,
//! f32), threshold in points (4, f32), loss weight (4, f32), depth (1), the decided positions'
//! cutoff in cp (2),
//! reserved (1), the engine's `id name` as length (2) and UTF-8 bytes, entry count (4).
//!
//! Entry, sorted by key: the position's Polyglot key (8); its evaluation from White's side (4);
//! the number of moves (1); per move, best first, its Polyglot encoding (2) and its score from
//! White's side (4). A score is its centipawns within +/-`MAX_CP`, or a mate in N as
//! +/-(`MATE` - N) (2, signed), then its expected score in half-thousandths (2): Stockfish's own,
//! which cp alone cannot give back, and which a replay needs for the losses.

use super::unit::{Judged, Score};

const MAGIC: &[u8; 4] = b"FBKF";
pub const FORMAT_VERSION: u16 = 1;
const MAX_CP: i32 = 30_000;
const MATE: i32 = 32_000;

/// What every fragment of one book must agree on, and which fragment this is.
#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    pub index: u16,
    pub budget: f32,
    pub charge: f32,
    pub threshold: f32,
    pub loss_weight: f32,
    pub depth: u8,
    pub cutoff: u16,
    pub engine: String,
}

impl Header {
    /// Why two fragments cannot belong to one book, if they cannot: a different charge, threshold,
    /// depth, cutoff or engine.
    pub fn mismatch(&self, other: &Header) -> Option<String> {
        let differs = |what: &str, a: String, b: String| (a != b).then(|| format!("{what} {a} against {b}"));
        differs("charge", self.charge.to_string(), other.charge.to_string())
            .or_else(|| differs("threshold", self.threshold.to_string(), other.threshold.to_string()))
            .or_else(|| differs("loss weight", self.loss_weight.to_string(), other.loss_weight.to_string()))
            .or_else(|| differs("depth", self.depth.to_string(), other.depth.to_string()))
            .or_else(|| differs("cutoff", self.cutoff.to_string(), other.cutoff.to_string()))
            .or_else(|| differs("engine", self.engine.clone(), other.engine.clone()))
    }
}

/// One expanded position, scores from White's side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Position {
    pub key: u64,
    pub eval: Judged,
    /// Polyglot move encoding and score, best first.
    pub moves: Vec<(u16, Judged)>,
}

pub fn encode(score: Score) -> i16 {
    let raw = match score {
        Score::Cp(cp) => cp.clamp(-MAX_CP, MAX_CP),
        Score::Mate(n) => {
            assert!(n != 0 && n.abs() < MATE - MAX_CP, "a mate in {n} cannot be stored");
            if n > 0 { MATE - n } else { -MATE - n }
        }
    };
    raw as i16
}

pub fn decode(raw: i16) -> Score {
    let raw = raw as i32;
    if raw > MAX_CP {
        Score::Mate(MATE - raw)
    } else if raw < -MAX_CP {
        Score::Mate(-MATE - raw)
    } else {
        Score::Cp(raw)
    }
}

/// The fragment's bytes, its positions sorted by key.
pub fn write(header: &Header, positions: &[Position]) -> Vec<u8> {
    let mut sorted: Vec<&Position> = positions.iter().collect();
    sorted.sort_by_key(|position| position.key);
    let engine = header.engine.as_bytes();
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&header.index.to_le_bytes());
    out.extend_from_slice(&header.budget.to_le_bytes());
    out.extend_from_slice(&header.charge.to_le_bytes());
    out.extend_from_slice(&header.threshold.to_le_bytes());
    out.extend_from_slice(&header.loss_weight.to_le_bytes());
    out.push(header.depth);
    out.extend_from_slice(&header.cutoff.to_le_bytes());
    out.push(0);
    out.extend_from_slice(&u16::try_from(engine.len()).expect("an engine name under 64 KB").to_le_bytes());
    out.extend_from_slice(engine);
    out.extend_from_slice(&(sorted.len() as u32).to_le_bytes());
    let judged = |out: &mut Vec<u8>, judged: Judged| {
        out.extend_from_slice(&encode(judged.score).to_le_bytes());
        out.extend_from_slice(&judged.expected.to_le_bytes());
    };
    for position in sorted {
        out.extend_from_slice(&position.key.to_le_bytes());
        judged(&mut out, position.eval);
        out.push(u8::try_from(position.moves.len()).expect("at most 218 legal moves"));
        for &(mv, score) in &position.moves {
            out.extend_from_slice(&mv.to_le_bytes());
            judged(&mut out, score);
        }
    }
    out
}

/// The header and positions of a fragment's bytes, or why they are not a fragment.
pub fn read(data: &[u8]) -> Result<(Header, Vec<Position>), String> {
    let mut reader = Reader { data, pos: 0 };
    if reader.take(4)? != MAGIC {
        return Err("not a book fragment (wrong magic)".into());
    }
    let format = reader.u16()?;
    if format != FORMAT_VERSION {
        return Err(format!("fragment format {format} is not supported (expected {FORMAT_VERSION})"));
    }
    let index = reader.u16()?;
    let budget = reader.f32()?;
    let charge = reader.f32()?;
    let threshold = reader.f32()?;
    let loss_weight = reader.f32()?;
    let depth = reader.take(1)?[0];
    let cutoff = reader.u16()?;
    reader.take(1)?;
    let engine_len = reader.u16()? as usize;
    let engine = String::from_utf8(reader.take(engine_len)?.to_vec()).map_err(|_| "the engine name is not UTF-8".to_string())?;
    let count = reader.u32()? as usize;
    let mut positions = Vec::with_capacity(count);
    for _ in 0..count {
        let key = reader.u64()?;
        let eval = reader.judged()?;
        let moves = (0..reader.take(1)?[0]).map(|_| Ok((reader.u16()?, reader.judged()?))).collect::<Result<_, String>>()?;
        positions.push(Position { key, eval, moves });
    }
    if reader.pos != data.len() {
        return Err("the fragment has bytes past its last position".into());
    }
    Ok((Header { index, budget, charge, threshold, loss_weight, depth, cutoff, engine }, positions))
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.pos + n;
        let bytes = self.data.get(self.pos..end).ok_or_else(|| "the fragment ends too early".to_string())?;
        self.pos = end;
        Ok(bytes)
    }

    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn i16(&mut self) -> Result<i16, String> {
        Ok(i16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn judged(&mut self) -> Result<Judged, String> {
        let score = decode(self.i16()?);
        let expected = self.u16()?;
        if expected > super::unit::WHOLE {
            return Err(format!("an expected score of {expected} half-thousandths"));
        }
        Ok(Judged { score, expected })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> Header {
        Header { index: 2, budget: 140.0, charge: 10.0, threshold: 7.0, loss_weight: 1.0, depth: 20, cutoff: 300, engine: "Judge 1".into() }
    }

    fn judged(score: Score, expected: u16) -> Judged {
        Judged { score, expected }
    }

    #[test]
    fn a_fragment_reads_back_sorted_by_key() {
        let positions = [
            Position {
                key: 9,
                eval: judged(Score::Cp(-35), 940),
                moves: vec![(100, judged(Score::Cp(-35), 940)), (200, judged(Score::Cp(-60), 901))],
            },
            Position { key: 2, eval: judged(Score::Mate(-1), 0), moves: Vec::new() },
        ];
        let (read_header, read_positions) = read(&write(&header(), &positions)).unwrap();
        assert_eq!(read_header, header());
        assert_eq!(read_positions, [positions[1].clone(), positions[0].clone()]);
    }

    #[test]
    fn scores_and_mates_keep_apart() {
        for score in [Score::Mate(1), Score::Mate(-1), Score::Mate(200), Score::Mate(-200), Score::Cp(MAX_CP), Score::Cp(-MAX_CP), Score::Cp(0)] {
            assert_eq!(decode(encode(score)), score);
        }
        assert_eq!(decode(encode(Score::Cp(99_999))), Score::Cp(MAX_CP), "clamped");
    }

    #[test]
    fn a_foreign_or_damaged_fragment_is_refused() {
        assert!(read(b"FUCB").is_err());
        let even = judged(Score::Cp(0), 1000);
        let mut bytes = write(&header(), &[Position { key: 1, eval: even, moves: vec![(5, even)] }]);
        bytes.pop();
        assert!(read(&bytes).is_err());
        bytes.extend_from_slice(&[0, 0, 0]);
        assert!(read(&bytes).is_err());
    }

    #[test]
    fn fragments_of_another_book_are_named_by_their_difference() {
        let other = Header { depth: 18, ..header() };
        assert_eq!(header().mismatch(&other), Some("depth 20 against 18".into()));
        assert_eq!(header().mismatch(&Header { index: 3, budget: 150.0, ..header() }), None);
    }
}
