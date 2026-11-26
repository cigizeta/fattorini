//! The book's own file: every searched position with the engine's evaluation, no moves (the moves
//! come from the openings tree when the Polyglot book is made). Little-endian.
//!
//! Header: magic `FUCB` (4), format version (2), book version (2), depth (1), reserved (3), the
//! engine's `id name` as length (2) and UTF-8 bytes, entry count (4).
//!
//! Entry, 16 bytes, sorted by key: the position's Polyglot key (8), its evaluation from White's side
//! (2, signed: centipawns within +/-`MAX_CP`, a mate in N as +/-(`MATE` - N)), depth (1),
//! reserved (5).

const MAGIC: &[u8; 4] = b"FUCB";
pub const FORMAT_VERSION: u16 = 1;
pub const ENTRY_SIZE: usize = 16;
pub const MAX_CP: i32 = 30_000;
pub const MATE: i32 = 32_000;

/// An evaluation from White's side: positive is good for White, whoever is to move, as GUIs show
/// it. A mate's N is positive when White mates and never 0: a position where the side to move is
/// checkmated is a mate in 1 for the side that gave it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eval {
    Cp(i32),
    Mate(i32),
}

impl Eval {
    pub fn encode(self) -> i16 {
        let raw = match self {
            Eval::Cp(cp) => cp.clamp(-MAX_CP, MAX_CP),
            Eval::Mate(n) => {
                assert!(n != 0 && n.abs() < MATE - MAX_CP, "a mate in {n} cannot be stored");
                if n > 0 { MATE - n } else { -MATE - n }
            }
        };
        raw as i16
    }

    pub fn decode(raw: i16) -> Eval {
        let raw = raw as i32;
        if raw > MAX_CP {
            Eval::Mate(MATE - raw)
        } else if raw < -MAX_CP {
            Eval::Mate(-MATE - raw)
        } else {
            Eval::Cp(raw)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub book_version: u16,
    pub depth: u8,
    pub engine: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub key: u64,
    pub eval: Eval,
    pub depth: u8,
}

/// The file's bytes, its entries sorted by key.
pub fn write(header: &Header, entries: &[Entry]) -> Vec<u8> {
    let mut sorted = entries.to_vec();
    sorted.sort_by_key(|entry| entry.key);
    let engine = header.engine.as_bytes();
    let engine_len = u16::try_from(engine.len()).expect("an engine name under 64 KB");
    let mut out = Vec::with_capacity(20 + engine.len() + sorted.len() * ENTRY_SIZE);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&header.book_version.to_le_bytes());
    out.push(header.depth);
    out.extend_from_slice(&[0; 3]);
    out.extend_from_slice(&engine_len.to_le_bytes());
    out.extend_from_slice(engine);
    out.extend_from_slice(&(sorted.len() as u32).to_le_bytes());
    for entry in &sorted {
        out.extend_from_slice(&entry.key.to_le_bytes());
        out.extend_from_slice(&entry.eval.encode().to_le_bytes());
        out.push(entry.depth);
        out.extend_from_slice(&[0; 5]);
    }
    out
}

/// The header and the entries of a file's bytes, or why they are not a book of this format.
pub fn read(data: &[u8]) -> Result<(Header, Vec<Entry>), String> {
    let mut reader = Reader { data, pos: 0 };
    if reader.take(4)? != MAGIC {
        return Err("not a UCI-judged book (wrong magic)".into());
    }
    let format = reader.u16()?;
    if format != FORMAT_VERSION {
        return Err(format!("book format {format} is not supported (expected {FORMAT_VERSION})"));
    }
    let book_version = reader.u16()?;
    let depth = reader.take(1)?[0];
    reader.take(3)?;
    let engine_len = reader.u16()? as usize;
    let engine = String::from_utf8(reader.take(engine_len)?.to_vec()).map_err(|_| "the engine name is not UTF-8".to_string())?;
    let count = reader.u32()? as usize;
    if data.len() - reader.pos != count * ENTRY_SIZE {
        return Err("the book's size does not match its entry count".into());
    }
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let key = u64::from_le_bytes(reader.take(8)?.try_into().unwrap());
        let eval = Eval::decode(i16::from_le_bytes(reader.take(2)?.try_into().unwrap()));
        let depth = reader.take(1)?[0];
        reader.take(5)?;
        entries.push(Entry { key, eval, depth });
    }
    Ok((Header { book_version, depth, engine }, entries))
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.pos + n;
        let bytes = self.data.get(self.pos..end).ok_or_else(|| "the book ends too early".to_string())?;
        self.pos = end;
        Ok(bytes)
    }

    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> Header {
        Header { book_version: 3, depth: 20, engine: "Stockfish 17".into() }
    }

    #[test]
    fn a_book_reads_back_sorted_by_key() {
        let entries = [
            Entry { key: 9, eval: Eval::Cp(-35), depth: 20 },
            Entry { key: 2, eval: Eval::Mate(3), depth: 20 },
            Entry { key: 5, eval: Eval::Mate(-1), depth: 0 },
        ];
        let (read_header, read_entries) = read(&write(&header(), &entries)).unwrap();
        assert_eq!(read_header, header());
        assert_eq!(read_entries.iter().map(|entry| entry.key).collect::<Vec<_>>(), [2, 5, 9]);
        assert_eq!(read_entries[0].eval, Eval::Mate(3));
        assert_eq!(read_entries[1].eval, Eval::Mate(-1));
        assert_eq!(read_entries[2].eval, Eval::Cp(-35));
    }

    #[test]
    fn mates_and_large_scores_keep_apart() {
        for eval in [Eval::Mate(1), Eval::Mate(-1), Eval::Mate(200), Eval::Mate(-200), Eval::Cp(MAX_CP), Eval::Cp(-MAX_CP), Eval::Cp(0)] {
            assert_eq!(Eval::decode(eval.encode()), eval);
        }
        assert_eq!(Eval::decode(Eval::Cp(99_999).encode()), Eval::Cp(MAX_CP), "clamped");
    }

    #[test]
    fn a_foreign_or_short_file_is_refused() {
        assert!(read(b"A64B").is_err());
        let mut bytes = write(&header(), &[Entry { key: 1, eval: Eval::Cp(0), depth: 20 }]);
        bytes.pop();
        assert!(read(&bytes).is_err());
    }
}
