//! A forge config's `labels:`, the polyline's spelling, and the paths of configs and of the sources'
//! games -- part of the pipeline contract, read by `smithy` and `arena`.
//!
//! Top-level directories, by role: `configs/` holds hand-written, checked-in configs (real source
//! code, unlike everything below -- `/data/` is entirely gitignored, `.gitignore`); `data/lc0/` holds
//! Lc0's games, converted by the smithy's Convert job, and `data/pesto/` PeSTO's, played by its Play
//! job, each with its frozen test set; `data/networks/` holds the training server's attempts, each with
//! its own games (`players::attempt_dir`).

use serde::Deserialize;

/// The lambda every training run labels its positions at, each by its progress through its game (the
/// smithy's `polyline`), as the records spell it: an attempt's frozen `attempt_config.txt`, its
/// `generations.tsv`'s winners, a candidate's name (`players::NetworkName`). The only one since the
/// grid's constant lambdas were removed (2026-11-26).
pub const POLYLINE_TEXT: &str = "polyline";

/// The directory of a generation's training run (`generation::run_dir`): the polyline's, as the grid
/// named each lambda's, kept so that attempts begun under it resume and read unchanged.
pub const RUN_DIR_NAME: &str = "lambda-polyline";

/// Where a network's generation 1 takes its labels from, a forge config's top-level `labels`: its
/// source, required. From generation 2 on, every run is self-play.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Labels {
    /// Lc0's corpus in `data/lc0`: generation 1 trains on it, judged on its frozen test set.
    Lc0,
    /// PeSTO's corpus in `data/pesto`, its self-play played once for every `pesto-` config:
    /// generation 1 trains on it, judged on its frozen test set.
    Pesto,
}

#[derive(Debug, Deserialize)]
struct LabelsHeader {
    labels: Option<Labels>,
}

/// `path`'s `labels`. Panics on a missing or unknown value.
pub fn load_labels(path: &str) -> Labels {
    let contents = std::fs::read_to_string(path).unwrap_or_else(|error| panic!("failed to read {path}: {error}"));
    parse_labels(&contents).unwrap_or_else(|message| panic!("{path}: {message}"))
}

fn parse_labels(contents: &str) -> Result<Labels, String> {
    let header: LabelsHeader = serde_yaml::from_str(contents).map_err(|error| format!("failed to parse: {error}"))?;
    header.labels.ok_or_else(|| "no labels: give it labels: lc0 or pesto".to_string())
}

/// A config's name to its file, `configs/{name}.yaml`, flat: a hand-written, checked-in config, not
/// generated data, so it lives under a real source directory rather than gitignored `data/`.
pub fn net_config_path(name: &str) -> String {
    format!("configs/{name}.yaml")
}

/// Where Lc0's converted games live, on the remote. Its `train/`, `test/` and `frozen/` sit under it.
/// Every comment carries the visit counts a policy is trained on; a value network reads and drops them.
pub const LC0_ROOT: &str = "data/lc0";

/// Where PeSTO's played games live, on the remote, as Lc0's do.
pub const PESTO_ROOT: &str = "data/pesto";

/// Where a source's training games live: `{root}/train/`, in day directories.
pub fn source_train_dir(root: &str) -> String {
    format!("{root}/train")
}

/// Where a source's test games live: `{root}/test/`, in day directories. No training reads them: the
/// test set is frozen from them alone.
pub fn source_test_dir(root: &str) -> String {
    format!("{root}/test")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn net_config_path_resolves_a_config_name_to_its_file() {
        assert_eq!(net_config_path("pesto-psq"), "configs/pesto-psq.yaml");
    }

    #[test]
    fn a_source_has_a_training_and_a_test_directory() {
        assert_eq!(source_train_dir(LC0_ROOT), "data/lc0/train");
        assert_eq!(source_test_dir(PESTO_ROOT), "data/pesto/test");
    }

    #[test]
    fn labels_name_one_of_two_sources() {
        assert!(parse_labels("labels: self\n").is_err(), "every forge has a source");
        assert_eq!(parse_labels("labels: lc0\n"), Ok(Labels::Lc0));
        assert_eq!(parse_labels("labels: pesto\n"), Ok(Labels::Pesto));
        assert!(parse_labels("labels: archives\n").is_err(), "the archives are gone");
        assert!(parse_labels("grid_center: 0.95\n").unwrap_err().contains("no labels"));
        assert!(parse_labels("labels: ccrl\n").is_err());
    }

    #[test]
    fn the_committed_forge_configs_name_their_labels() {
        crate::pipeline::repository_root::enter_for_tests();
        for (config, labels) in [("pesto-psq-threats-screlu", Labels::Pesto), ("lc0-halfk-threats-screlu-hidden-512-8-budget", Labels::Lc0)] {
            assert_eq!(load_labels(&net_config_path(config)), labels, "{config}");
        }
    }
}
