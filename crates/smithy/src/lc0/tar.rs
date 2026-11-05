//! A tar archive read member by member, as Lc0 publishes its training data: a few dozen lines rather
//! than a dependency. Each member is a 512-byte header (name, octal size, type) followed by its data,
//! padded to 512; two zero blocks end the archive. Only regular files are returned; directories and
//! any extended header are skipped.

use std::io::Read;

const BLOCK: usize = 512;

pub(super) struct TarReader<R: Read> {
    inner: R,
}

impl<R: Read> TarReader<R> {
    pub(super) fn new(inner: R) -> TarReader<R> {
        TarReader { inner }
    }

    /// The next regular file, its full name and its contents; `None` at the end of the archive.
    pub(super) fn next_file(&mut self) -> Result<Option<(String, Vec<u8>)>, String> {
        loop {
            let mut header = [0u8; BLOCK];
            if !read_block(&mut self.inner, &mut header)? || header.iter().all(|&byte| byte == 0) {
                return Ok(None);
            }
            let name = field_text(&header[0..100]);
            let prefix = if &header[257..262] == b"ustar" { field_text(&header[345..500]) } else { String::new() };
            let full_name = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
            let size = parse_octal(&header[124..136]).map_err(|message| format!("{full_name}: {message}"))?;
            let padded = size.div_ceil(BLOCK as u64) * BLOCK as u64;
            let mut data = vec![0u8; padded as usize];
            self.inner
                .read_exact(&mut data)
                .map_err(|error| format!("{full_name}: the archive ends inside its data: {error}"))?;
            data.truncate(size as usize);
            let kind = header[156];
            if kind == b'0' || kind == 0 {
                return Ok(Some((full_name, data)));
            }
        }
    }
}

/// Fills `block`, or returns false at a clean end of the stream before its first byte.
fn read_block<R: Read>(reader: &mut R, block: &mut [u8; BLOCK]) -> Result<bool, String> {
    let mut filled = 0;
    while filled < BLOCK {
        let read = reader.read(&mut block[filled..]).map_err(|error| format!("failed to read the archive: {error}"))?;
        if read == 0 {
            if filled == 0 {
                return Ok(false);
            }
            return Err("the archive ends inside a header".to_string());
        }
        filled += read;
    }
    Ok(true)
}

/// A NUL-terminated header field.
fn field_text(field: &[u8]) -> String {
    let end = field.iter().position(|&byte| byte == 0).unwrap_or(field.len());
    String::from_utf8_lossy(&field[..end]).into_owned()
}

/// An octal header number, padded with spaces or NULs.
fn parse_octal(field: &[u8]) -> Result<u64, String> {
    let text = field_text(field);
    let digits = text.trim();
    u64::from_str_radix(digits, 8).map_err(|_| format!("the header's size {digits:?} is not octal"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One member: its header (name, size, type) and its data padded to a block.
    fn member(name: &str, kind: u8, data: &[u8]) -> Vec<u8> {
        let mut header = [0u8; BLOCK];
        header[..name.len()].copy_from_slice(name.as_bytes());
        let size = format!("{:011o}\0", data.len());
        header[124..136].copy_from_slice(size.as_bytes());
        header[156] = kind;
        header[257..262].copy_from_slice(b"ustar");
        let mut bytes = header.to_vec();
        bytes.extend_from_slice(data);
        bytes.resize(bytes.len().div_ceil(BLOCK) * BLOCK, 0);
        bytes
    }

    #[test]
    fn a_tar_yields_its_regular_files_in_order_and_skips_the_rest() {
        let mut archive = member("run/", b'5', b"");
        archive.extend(member("run/training.1.gz", b'0', b"abc"));
        archive.extend(member("run/training.2.gz", b'0', &[7u8; 600]));
        archive.extend([0u8; 2 * BLOCK]);

        let mut reader = TarReader::new(&archive[..]);
        assert_eq!(reader.next_file().unwrap(), Some(("run/training.1.gz".to_string(), b"abc".to_vec())));
        assert_eq!(reader.next_file().unwrap(), Some(("run/training.2.gz".to_string(), vec![7u8; 600])));
        assert_eq!(reader.next_file().unwrap(), None);
    }

    #[test]
    fn a_truncated_tar_is_an_error() {
        let archive = member("run/training.1.gz", b'0', &[1u8; 1000]);
        let mut reader = TarReader::new(&archive[..700]);
        assert!(reader.next_file().is_err());
    }
}
