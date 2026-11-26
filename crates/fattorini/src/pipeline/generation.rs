//! The files below a forge attempt's generation that the arena reads to name a player:
//! `data/networks/<config>/attempt-<n>/gen-<g>/` (`players.rs`).
//!
//! The generation's training run sits in its `run_dir`, `lambda-polyline/` (`model.vs`,
//! `optimizer.opt`, `history.tsv`, `checkpoints/`, `candidate.nnue`, `candidate.txt`). The name is the
//! grid's, whose every lambda had a directory of its own, kept so that the attempts begun under it
//! resume and read unchanged.
//!
//! Plain filesystem work with no `tch` involvement.

/// The fixed depth of every search training runs: the self-play games whose scores become the labels,
/// and the gate that decides a promotion. One constant for both, because the gate asks whether a
/// network plays better under exactly the search whose results it was trained to reproduce.
/// `arena` reads it.
pub const SEARCH_DEPTH: u32 = 9;

/// `gen-<g>/lambda-polyline/`: the generation's training run, with its files and its candidate.
pub fn run_dir(generation_dir: &str) -> String {
    format!("{generation_dir}/{}", super::preprocessing_config::RUN_DIR_NAME)
}

/// The run's candidate, exported for the engine: its budget's round robin's winner.
pub fn candidate_network_path(run_dir: &str) -> String {
    format!("{run_dir}/candidate.nnue")
}

/// `candidate.txt`: which checkpoint the run's `candidate.nnue` holds. Written last by `smithy`'s
/// export, so its presence marks the candidate chosen.
pub fn candidate_record_path(run_dir: &str) -> String {
    format!("{run_dir}/candidate.txt")
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
    fn every_run_file_sits_in_the_polyline_s_directory() {
        let run = run_dir("data/networks/self-psq/attempt-1/gen-1");
        assert_eq!(run, "data/networks/self-psq/attempt-1/gen-1/lambda-polyline");
        assert_eq!(candidate_network_path(&run), format!("{run}/candidate.nnue"));
        assert_eq!(candidate_record_path(&run), format!("{run}/candidate.txt"));
        assert_eq!(record_field("step\t12\nloss\t0.5\n", "loss", "x"), Ok("0.5"));
        assert!(record_field("step\t12\n", "loss", "x").is_err());
    }
}
