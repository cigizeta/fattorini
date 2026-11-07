//! The files below a forge run's generation that the arena reads to name a player:
//! `data/networks/<config>/run-<n>/gen-<g>/` (`players.rs`).
//!
//! Each lambda's run sits in its own `lambda_dir`, `lambda-<l>/` (`model.vs`, `optimizer.opt`,
//! `history.tsv`, `checkpoints/`, `candidate.nnue`, `candidate.txt`).
//!
//! Plain filesystem work with no `tch` involvement.

/// The fixed depth of every search training runs: the self-play games whose scores become the labels,
/// and the gate that decides a promotion. One constant for both, because the gate asks whether a
/// network plays better under exactly the search whose results it was trained to reproduce.
/// `arena` reads it.
pub const SEARCH_DEPTH: u32 = 9;

/// `gen-<g>/lambda-<l>/`, `l` with two decimals (`lambda-0.70`) or `polyline`: the training run of
/// one arm, with the files of one run and its candidate.
pub fn lambda_dir(generation_dir: &str, lambda: super::preprocessing_config::ArmLambda) -> String {
    format!("{generation_dir}/{}", super::preprocessing_config::lambda_dir_name(lambda))
}

/// A lambda's candidate, exported for the engine when its run stops: the run's lowest-test-loss
/// checkpoint. The round robin chooses among the candidates.
pub fn candidate_network_path(lambda_dir: &str) -> String {
    format!("{lambda_dir}/candidate.nnue")
}

/// `candidate.txt`: which checkpoint a lambda's `candidate.nnue` holds. Written last by `smithy`'s
/// export, so its presence marks the lambda's run finished.
pub fn candidate_record_path(lambda_dir: &str) -> String {
    format!("{lambda_dir}/candidate.txt")
}

/// The value of `text`'s `name<TAB>value` line, `path` naming the file in the error. The shape of
/// every record in a generation's directory.
pub fn record_field<'a>(text: &'a str, name: &str, path: &str) -> Result<&'a str, String> {
    let value = text.lines().find_map(|line| line.strip_prefix(name).and_then(|rest| rest.strip_prefix('\t')));
    value.ok_or_else(|| format!("{path} has no {name} line"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_lambda_file_sits_in_its_two_decimal_directory() {
        let run = lambda_dir("data/networks/self-psq/run-1/gen-1", super::super::preprocessing_config::ArmLambda::Constant(0.7));
        assert_eq!(run, "data/networks/self-psq/run-1/gen-1/lambda-0.70");
        assert_eq!(candidate_network_path(&run), format!("{run}/candidate.nnue"));
        assert_eq!(candidate_record_path(&run), format!("{run}/candidate.txt"));
        assert_eq!(record_field("step\t12\nloss\t0.5\n", "loss", "x"), Ok("0.5"));
        assert!(record_field("step\t12\n", "loss", "x").is_err());
    }
}
