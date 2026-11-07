//! `data/machine.yaml`: which machine this is, written once by hand on each PC.
//!
//! The name becomes the stem of every self-play segment this machine writes
//! (`{name}-{YYYYMMDD-HHMMSS}.pgn.zst`), and several machines publish segments into the same
//! directories on the remote, so the name must be unique across them. It lives in a file rather than
//! on the command line so it cannot be mistyped from one run to the next, and under `data/`, which is
//! gitignored, so the file never travels with the repository to another machine.

use serde::Deserialize;

pub const MACHINE_CONFIG_PATH: &str = "data/machine.yaml";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MachineConfig {
    name: String,
}

/// This machine's name, from `path`. `Err` carries a message for the operator that says what is
/// wrong and how to write the file, since a missing file is the expected state of a fresh checkout.
pub fn load(path: &str) -> Result<String, String> {
    let contents = std::fs::read_to_string(path).map_err(|error| {
        format!("failed to read {path}: {error}. {}", how_to_create(path))
    })?;
    let config: MachineConfig = serde_yaml::from_str(&contents).map_err(|error| {
        format!("failed to parse {path}: {error}. {}", how_to_create(path))
    })?;
    validate_name(&config.name).map_err(|error| format!("{path}: {error}. {}", how_to_create(path)))?;
    Ok(config.name)
}

/// Lowercase ASCII letters and digits only, and not empty, as the protocol's `smithy.yaml` checks
/// too. No `-`: it separates the stem from the timestamp in a segment's name. Nothing that would need
/// quoting in a path.
pub fn validate_name(name: &str) -> Result<(), String> {
    fattorini::pipeline::protocol::validate_machine_name(name)
}

fn how_to_create(path: &str) -> String {
    format!(
        "Write it once on each machine, with a name unique across every machine that publishes games, e.g. \
         `echo 'name: alpha' > {path}`"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str, contents: &str) -> String {
        let path = std::env::temp_dir().join(format!("fattorini-machine-test-{name}-{}.yaml", std::process::id()));
        std::fs::write(&path, contents).expect("failed to write the fixture file");
        path.to_str().unwrap().to_string()
    }

    #[test]
    fn load_reads_a_valid_name() {
        let path = fixture("valid", "name: alpha\n");
        assert_eq!(load(&path), Ok("alpha".to_string()));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn load_explains_how_to_create_a_missing_file() {
        let error = load("fattorini-machine-test-no-such-file.yaml").unwrap_err();
        assert!(error.contains("echo 'name: alpha'"), "{error}");
    }

    #[test]
    fn load_refuses_an_unknown_key() {
        let path = fixture("unknown-key", "name: alpha\nthreads: 16\n");
        assert!(load(&path).is_err());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn load_refuses_an_invalid_name() {
        let path = fixture("invalid-name", "name: Alpha-2\n");
        assert!(load(&path).is_err());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn validate_name_accepts_lowercase_letters_and_digits() {
        assert_eq!(validate_name("alpha"), Ok(()));
        assert_eq!(validate_name("pc2"), Ok(()));
    }

    #[test]
    fn validate_name_refuses_empty_names_hyphens_uppercase_and_path_characters() {
        for name in ["", "alpha-2", "Alpha", "alpha 2", "alpha/2", "àlpha", "alpha_2"] {
            assert!(validate_name(name).is_err(), "{name:?} should be refused");
        }
    }
}
