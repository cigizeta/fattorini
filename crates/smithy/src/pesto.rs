//! PeSTO's source, `configs/pesto.yaml`: the PeSTO evaluation, the player `pesto`, whose self-play
//! the smithy's Play job has the arenas play once into `data/pesto/test` and `data/pesto/train` on the
//! remote, in day directories as Lc0's corpus is, for every `pesto-` config's generation 1; and its
//! tune, as a forge config's `tune:`.
//!
//! The test games come first, to `test_positions`, then the training games, to
//! `training_positions`. A segment is made locally and moved to the remote once closed, so the remote
//! holds only finished segments; `complete.txt` on the remote says the corpus is whole, and only then
//! may a `pesto-` run train on it.

use serde::Deserialize;

use kalo_chess::pipeline::{preprocessing_config, remote, segments};

use super::tune::TuneConfig;

pub(crate) const CONFIG_PATH: &str = "configs/pesto.yaml";

/// Written to the remote when the Play job has played the whole corpus.
const COMPLETE_PATH: &str = "data/pesto/complete.txt";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PestoConfig {
    /// The corpus's training positions, every `pesto-` run's generation 1 training on all of them.
    pub(crate) training_positions: u64,
    /// The test positions the corpus holds, and the frozen test set every `pesto-` run's generation 1
    /// is judged on.
    pub(crate) test_positions: u64,
    pub(crate) tune: TuneConfig,
}

/// `CONFIG_PATH`, its sizes positive. A source is never scaled: a scaled run reads a share of it.
pub(crate) fn load() -> Result<PestoConfig, String> {
    let text = std::fs::read_to_string(CONFIG_PATH).map_err(|error| format!("{CONFIG_PATH}: {error}"))?;
    let config: PestoConfig = serde_yaml::from_str(&text).map_err(|error| format!("{CONFIG_PATH}: {error}"))?;
    if config.training_positions == 0 || config.test_positions == 0 {
        return Err(format!("{CONFIG_PATH}: training_positions and test_positions must be positive"));
    }
    Ok(config)
}

/// Whether the Play job has played the whole corpus.
pub(crate) fn is_complete() -> bool {
    std::path::Path::new(&remote::remote_path(COMPLETE_PATH)).exists()
}

/// Records on the remote that the corpus is whole, with what it holds.
pub(crate) fn write_complete(test_positions: u64, training_positions: u64) -> Result<(), String> {
    let text = format!("test_positions\t{test_positions}\ntraining_positions\t{training_positions}\n");
    remote::write_small(COMPLETE_PATH, &text).map_err(|error| error.to_string())
}

/// Where its test games and its training games are kept, on the remote, and made, locally.
pub(crate) fn test_dir() -> String {
    preprocessing_config::source_test_dir(preprocessing_config::PESTO_ROOT)
}

pub(crate) fn train_dir() -> String {
    preprocessing_config::source_train_dir(preprocessing_config::PESTO_ROOT)
}

/// The positions of the finished segments under the remote counterpart of `dir`.
pub(crate) fn remote_positions(dir: &str) -> Result<u64, String> {
    let names = segments::list_remote(dir).map_err(|error| error.to_string())?;
    let mut positions = 0;
    for name in names {
        positions += segments::read_remote_meta(dir, &name)?.positions;
    }
    Ok(positions)
}

/// The page's line for the corpus: complete, with what it holds, or what it is to hold.
pub(crate) fn summary() -> String {
    let wanted = match load() {
        Ok(config) => format!("{} test and {} training positions", config.test_positions, config.training_positions),
        Err(message) => return message,
    };
    if is_complete() {
        return match remote::cat(COMPLETE_PATH) {
            Ok(text) => format!("complete: {}", text.trim().replace('\t', " ").replace('\n', ", ")),
            Err(_) => format!("complete: {wanted}"),
        };
    }
    format!("not complete: Play plays {wanted}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pesto_s_config_sizes_its_corpus_and_holds_its_tune() {
        kalo_chess::pipeline::repository_root::enter_for_tests();
        let config = load().unwrap();
        assert!(config.training_positions > 0 && config.test_positions > 0);
        assert!(crate::tune::args_of(&config.tune, kalo_chess::pipeline::protocol::PESTO).is_ok());
    }

    #[test]
    fn its_games_go_under_data_pesto() {
        assert_eq!((test_dir(), train_dir()), ("data/pesto/test".to_string(), "data/pesto/train".to_string()));
    }
}
