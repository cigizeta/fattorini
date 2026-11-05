//! Finished segments in a forge run's games and `data/chess/lc0`, as both `arena` and `smithy` see them: one
//! directory per day, `{dir}/YYYYMMDD/{name}.pgn.zst`, each segment beside its `{name}.pgn.zst.meta`,
//! which records its trainable positions and games. The day is the segment's own date, so no
//! directory grows without end.
//!
//! The `.meta` is a segment's completion marker: it is written before the rename that makes the
//! segment visible, so a visible segment always has one.

use std::fs;

use super::remote::{self, RemoteError};

pub const SEGMENT_SUFFIX: &str = ".pgn.zst";
/// Appended to a segment's whole name: `{name}.pgn.zst.meta`.
pub const META_SUFFIX: &str = ".meta";

/// What a segment's `.meta` records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegmentMeta {
    pub positions: u64,
    pub games: u64,
}

impl SegmentMeta {
    pub fn text(&self) -> String {
        format!("positions\t{}\ngames\t{}\n", self.positions, self.games)
    }

    pub fn parse(text: &str) -> Result<SegmentMeta, String> {
        let (mut positions, mut games) = (None, None);
        for line in text.lines() {
            let (key, value) = line.split_once('\t').ok_or_else(|| format!("malformed line {line:?}"))?;
            let number: u64 = value.parse().map_err(|_| format!("{key} is not a number: {value:?}"))?;
            match key {
                "positions" => positions = Some(number),
                "games" => games = Some(number),
                _ => return Err(format!("unknown key {key:?}")),
            }
        }
        match (positions, games) {
            (Some(positions), Some(games)) => Ok(SegmentMeta { positions, games }),
            _ => Err("positions and games are both required".to_string()),
        }
    }

    /// The `.meta` at `path`.
    pub fn read(path: &str) -> Result<SegmentMeta, String> {
        let text = fs::read_to_string(path).map_err(|error| format!("failed to read {path}: {error}"))?;
        SegmentMeta::parse(&text).map_err(|message| format!("{path}: {message}"))
    }
}

/// The day directory under `dir` of a segment dated `date`, which starts `YYYYMMDD`.
pub fn day_dir(dir: &str, date: &str) -> String {
    let day = date.get(..8).filter(|day| is_day(day)).unwrap_or_else(|| panic!("{date:?} does not start with YYYYMMDD"));
    format!("{dir}/{day}")
}

/// Whether `name` is a day directory's name, `YYYYMMDD`.
pub fn is_day(name: &str) -> bool {
    name.len() == 8 && name.bytes().all(|byte| byte.is_ascii_digit())
}

/// Every finished segment under the remote counterpart of `dir`, as `YYYYMMDD/{name}` without
/// `.pgn.zst`, sorted: the day directories in date order, each one's segments by name. Empty when the
/// remote directory does not exist yet.
pub fn list_remote(dir: &str) -> Result<Vec<String>, RemoteError> {
    let mut segments = Vec::new();
    for day in remote::list_dirs(dir, &is_day)? {
        for name in remote::list(&format!("{dir}/{day}"), &|name| name.ends_with(SEGMENT_SUFFIX))? {
            segments.push(format!("{day}/{}", name.strip_suffix(SEGMENT_SUFFIX).expect("listed by that suffix")));
        }
    }
    Ok(segments)
}

/// The `.meta` of `segment` (as `list_remote` names it) under the remote counterpart of `dir`.
pub fn read_remote_meta(dir: &str, segment: &str) -> Result<SegmentMeta, String> {
    SegmentMeta::read(&format!("{}/{segment}{SEGMENT_SUFFIX}{META_SUFFIX}", remote::remote_path(dir)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_meta_reads_back_what_it_wrote_and_refuses_anything_else() {
        let meta = SegmentMeta { positions: 1_002_041, games: 6991 };
        assert_eq!(meta.text(), "positions\t1002041\ngames\t6991\n");
        assert_eq!(SegmentMeta::parse(&meta.text()), Ok(meta));
        for text in ["positions\t1\n", "positions\t1\ngames\tx\n", "positions\t1\ngames\t2\ntar\t3\n", "positions 1\n"] {
            assert!(SegmentMeta::parse(text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn a_segment_s_day_is_its_date_s_first_eight_digits() {
        assert_eq!(day_dir("data/chess/lc0/test", "20261017-030803"), "data/chess/lc0/test/20261017");
        assert!(is_day("20261017"));
        assert!(!is_day("2026101"));
        assert!(!is_day("frozen"));
    }
}
