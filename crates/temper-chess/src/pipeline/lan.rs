//! The text helpers the training server's protocol (`protocol.rs`) and its records share: the text
//! messages are `name<TAB>value` lines, read by `fields` and `field`; a body may start with a text
//! line before bytes (`first_line_bytes`).

use std::collections::HashMap;
use std::path::Path;

/// `text`'s `name<TAB>value` lines, the first of each name.
pub fn fields(text: &str) -> HashMap<&str, &str> {
    let mut map = HashMap::new();
    for line in text.lines() {
        if let Some((name, value)) = line.split_once('\t') {
            map.entry(name).or_insert(value);
        }
    }
    map
}

/// The field `name` of `map`, parsed, or an error naming `what`.
pub fn field<T: std::str::FromStr>(map: &HashMap<&str, &str>, name: &str, what: &str) -> Result<T, String> {
    let value = map.get(name).ok_or_else(|| format!("{what}: no {name} line"))?;
    value.parse().map_err(|_| format!("{what}: {name} {value:?} is not valid"))
}

/// The first line of `bytes`, as text, and the rest, untouched: a binary body's header.
pub fn first_line_bytes(bytes: &[u8]) -> Result<(&str, &[u8]), String> {
    let end = bytes.iter().position(|&byte| byte == b'\n').unwrap_or(bytes.len());
    let line = std::str::from_utf8(&bytes[..end]).map_err(|_| "a header line is not text".to_string())?;
    Ok((line, bytes.get(end + 1..).unwrap_or(&[])))
}

/// Writes `text` to `path`, creating its directory, through `.partial` and a rename.
pub fn write_whole(path: &str, text: &str) -> Result<(), String> {
    if let Some(parent) = Path::new(path).parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let partial = format!("{path}.partial");
    std::fs::write(&partial, text).map_err(|error| format!("{partial}: {error}"))?;
    std::fs::rename(&partial, path).map_err(|error| format!("{path}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_binary_body_survives_its_header_line() {
        let bytes = [&b"o000001\n"[..], &[0, 159, 146, 150, b'\n', 255]].concat();
        assert_eq!(first_line_bytes(&bytes).unwrap(), ("o000001", &[0, 159, 146, 150, b'\n', 255][..]));
        assert_eq!(first_line_bytes(b"OK").unwrap(), ("OK", &[][..]));
        assert!(first_line_bytes(&[255, b'\n']).is_err());
    }

    #[test]
    fn fields_keep_the_first_of_each_name_and_field_parses_or_says_why() {
        let map = fields("a\t1\nb\tx\na\t2\nno tab\n");
        assert_eq!(field::<u32>(&map, "a", "test"), Ok(1));
        assert!(field::<u32>(&map, "b", "test").unwrap_err().contains("not valid"));
        assert!(field::<u32>(&map, "c", "test").unwrap_err().contains("no c line"));
    }
}
