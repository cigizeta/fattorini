//! Retention for the per-round weight checkpoints training writes in each lambda's directory.
//!
//! `checkpoints/` gets one bare weight file per completed evaluation round, named by step so a
//! step resolves straight to a path. Weights only, with no
//! optimizer state beside them. After each round only the run's best survives (`keep_only`): it is
//! the one checkpoint anything reads, a stage switch rolling back to it, the candidate exported from
//! it, the next generation warm-started from the winner's.
//!
//! Everything here is plain filesystem work with no `tch` involvement, which is what lets it be
//! tested against a temporary directory without building a `VarStore`.

pub fn checkpoint_path(checkpoints_dir: &str, step: u64) -> String {
    format!("{checkpoints_dir}/step_{step:09}.vs")
}

/// Every step a `checkpoints/` directory currently holds a weight file for, ascending.
pub fn checkpoint_steps(checkpoints_dir: &str) -> Vec<u64> {
    let Ok(entries) = std::fs::read_dir(checkpoints_dir) else { return Vec::new() };
    let mut found: Vec<u64> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_string();
            name.strip_prefix("step_")?.strip_suffix(".vs")?.parse::<u64>().ok()
        })
        .collect();
    found.sort_unstable();
    found
}

/// Deletes every checkpoint but `best`'s, the run's best as `stages::Stages::best_step` says. Read
/// from the stages rather than ranked again by test loss here: the two broke a tie in loss
/// differently (a ranking towards the newer step, `Stages` towards the older), and with one kept a
/// tie would have deleted the very checkpoint a rollback and the export load.
pub fn keep_only(checkpoints_dir: &str, best: u64) {
    for step in checkpoint_steps(checkpoints_dir) {
        if step != best {
            std::fs::remove_file(checkpoint_path(checkpoints_dir, step)).ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> String {
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("smithy-checkpoint-store-{pid}-{label}"));
        std::fs::remove_dir_all(&path).ok();
        std::fs::create_dir_all(&path).expect("failed to create the fixture directory");
        path.to_str().unwrap().to_string()
    }

    fn write_checkpoint(checkpoints_dir: &str, step: u64) {
        std::fs::create_dir_all(checkpoints_dir).unwrap();
        std::fs::write(checkpoint_path(checkpoints_dir, step), b"weights").unwrap();
    }

    #[test]
    fn keep_only_leaves_the_best_alone_wherever_it_is() {
        for best in [0u64, 20, 40] {
            let checkpoints_dir = format!("{}/checkpoints", temp_dir(&format!("keep-only-{best}")));
            for step in [0u64, 20, 40] {
                write_checkpoint(&checkpoints_dir, step);
            }
            keep_only(&checkpoints_dir, best);
            assert_eq!(checkpoint_steps(&checkpoints_dir), vec![best]);
        }
    }

    #[test]
    fn keep_only_on_an_empty_or_missing_directory_does_nothing() {
        let base = temp_dir("keep-only-empty");
        keep_only(&format!("{base}/checkpoints"), 0);
        std::fs::create_dir_all(format!("{base}/checkpoints")).unwrap();
        keep_only(&format!("{base}/checkpoints"), 0);
        assert!(checkpoint_steps(&format!("{base}/checkpoints")).is_empty());
    }
}
