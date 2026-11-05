//! A forge config's `labels:`, the lambdas' spelling, and the paths of configs and of the sources'
//! games -- part of the pipeline contract, read by `smithy` and `arena`.
//!
//! Top-level directories, by role: `configs/chess/` holds hand-written, checked-in configs (real source
//! code, unlike everything below -- `/data/` is entirely gitignored, `.gitignore`); `data/chess/lc0/` holds
//! Lc0's games, converted by the smithy's Convert job, and `data/chess/pesto/` PeSTO's, played by its Play
//! job, each with its frozen test set; `data/chess/networks/` holds the training server's runs, each with its
//! own games (`players::run_dir`).

use serde::Deserialize;

/// What an arm of a generation trains at: one lambda for every position (the grid's arms), or each
/// position at its own lambda by its progress through its game (`lambda: polyline`, smithy's
/// `polyline`). It names the arm everywhere: its directory, its records, the round robin's players.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ArmLambda {
    Constant(f64),
    Polyline,
}

impl ArmLambda {
    /// The constant lambda, `None` for the polyline.
    pub fn constant(self) -> Option<f64> {
        match self {
            ArmLambda::Constant(lambda) => Some(lambda),
            ArmLambda::Polyline => None,
        }
    }

    /// `lambda_text`'s spelling read back: a constant's two decimals, or `polyline`.
    pub fn parse(text: &str) -> Result<ArmLambda, String> {
        if text == POLYLINE_TEXT {
            return Ok(ArmLambda::Polyline);
        }
        text.parse().map(ArmLambda::Constant).map_err(|_| format!("{text:?} is neither a lambda nor {POLYLINE_TEXT}"))
    }
}

impl std::fmt::Display for ArmLambda {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&lambda_text(*self))
    }
}

/// For a page: a constant as its number, the polyline as the string `polyline`.
impl serde::Serialize for ArmLambda {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            ArmLambda::Constant(lambda) => serializer.serialize_f64(*lambda),
            ArmLambda::Polyline => serializer.serialize_str(POLYLINE_TEXT),
        }
    }
}

/// How the polyline arm is spelled where a constant's two decimals would be.
const POLYLINE_TEXT: &str = "polyline";

/// Where a network's generation 1 takes its labels from, a forge config's top-level `labels`: its
/// source, required. From generation 2 on, every run is self-play.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Labels {
    /// Lc0's corpus in `data/chess/lc0`: generation 1 trains on it, judged on its frozen test set.
    Lc0,
    /// PeSTO's corpus in `data/chess/pesto`, its self-play played once for every `pesto-` config:
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

/// An arm's lambda as every name and record spells it: a constant with two decimals, `0.70`, `0.65`,
/// fixed width, so the run directories, the test set's label sets and the player names line up and
/// sort in the grid's numeric order; the polyline as `polyline`.
pub fn lambda_text(lambda: ArmLambda) -> String {
    match lambda {
        ArmLambda::Constant(lambda) => format!("{lambda:.2}"),
        ArmLambda::Polyline => POLYLINE_TEXT.to_string(),
    }
}

/// A list of arms' lambdas as the console shows it, each by `lambda_text`: `[0.90, 0.95, 1.00]`, not
/// `{:?}`'s `[0.9, 0.95, 1.0]`.
pub fn lambdas_text(lambdas: &[ArmLambda]) -> String {
    format!("[{}]", lambdas.iter().map(|&lambda| lambda_text(lambda)).collect::<Vec<String>>().join(", "))
}

/// An arm's lambda as its directory names it, `lambda-0.70`, `lambda-polyline` (`lambda_text`).
pub fn lambda_dir_name(lambda: ArmLambda) -> String {
    format!("lambda-{}", lambda_text(lambda))
}

/// A config's name to its file, `configs/chess/{name}.yaml`, flat: a hand-written, checked-in config, not
/// generated data, so it lives under a real source directory rather than gitignored `data/`.
pub fn net_config_path(name: &str) -> String {
    format!("configs/chess/{name}.yaml")
}

/// Where Lc0's converted games live, on the remote. Its `train/`, `test/` and `frozen/` sit under it.
/// Every comment carries the visit counts a policy is trained on; a value network reads and drops them.
pub const LC0_ROOT: &str = "data/chess/lc0";

/// Where PeSTO's played games live, on the remote, as Lc0's do.
pub const PESTO_ROOT: &str = "data/chess/pesto";

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
        assert_eq!(net_config_path("pesto-psq"), "configs/chess/pesto-psq.yaml");
    }

    #[test]
    fn a_source_has_a_training_and_a_test_directory() {
        assert_eq!(source_train_dir(LC0_ROOT), "data/chess/lc0/train");
        assert_eq!(source_test_dir(PESTO_ROOT), "data/chess/pesto/test");
    }

    use ArmLambda::{Constant, Polyline};

    #[test]
    fn a_list_of_lambdas_is_spelled_with_two_decimals_each() {
        assert_eq!(lambdas_text(&[Constant(0.9), Constant(0.95), Constant(1.0)]), "[0.90, 0.95, 1.00]");
        assert_eq!(lambdas_text(&[]), "[]");
    }

    #[test]
    fn a_lambda_is_spelled_with_two_decimals_and_the_polyline_by_its_name() {
        assert_eq!(lambda_text(Constant(0.7)), "0.70");
        assert_eq!(lambda_text(Constant(0.65)), "0.65");
        assert_eq!(lambda_dir_name(Constant(0.8)), "lambda-0.80");
        assert_eq!(lambda_text(Polyline), "polyline");
        assert_eq!(lambda_dir_name(Polyline), "lambda-polyline");
        assert_eq!(Polyline.to_string(), "polyline");
    }

    #[test]
    fn a_lambda_s_spelling_reads_back() {
        assert_eq!(ArmLambda::parse("0.70"), Ok(Constant(0.7)));
        assert_eq!(ArmLambda::parse("polyline"), Ok(Polyline));
        assert!(ArmLambda::parse("poly").is_err());
        assert_eq!(Constant(0.3).constant(), Some(0.3));
        assert_eq!(Polyline.constant(), None);
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
        for (config, labels) in [("pesto-psq-threats-screlu", Labels::Pesto), ("lc0-psq-threats-screlu", Labels::Lc0)] {
            assert_eq!(load_labels(&net_config_path(config)), labels, "{config}");
        }
    }
}
