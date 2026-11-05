//! What a run was started with, frozen on its first invocation and verified on every later one.
//!
//! A run's configuration is read-only once it has started, and this is where that is enforced. The
//! reason is that the NNUE track derives, rather than persists, everything it can: the training
//! stream's read position is `training_batch_size * optimizer.step_count()` taken modulo the train
//! cache's row count, how far into the current round a resumed run is is `step_count %
//! steps_per_round`, and which rate of its list it trains at is replayed from `history.tsv` with
//! `warmup_rounds` and `patience_rounds`. Deriving beats persisting -- a second counter beside the step count can only
//! ever disagree with it -- but it makes the derivation's inputs load-bearing *across* invocations.
//! Lower `steps_per_round` between two runs of one job and the persisted step count is reinterpreted
//! into a different position in a different round; change `training_batch_size` and the same step
//! count names a different row of the cache. `game_labels::RoundProgress`'s own doc comment records
//! what that costs when it happens: a `steps_per_eval` lowered between two runs left a stale, larger
//! persisted count already past the new round size, and the next round completed with zero further
//! training.
//!
//! So the fields the derivation and the two boundaries depend on, plus the ones that identify the
//! dataset and the model, are written once beside `model.vs` and compared field by field from then
//! on. A mismatch is fatal and names the field, what the run recorded and what the configuration says
//! now.
//!
//! Plain text, not the magic-bytes-plus-version binary format the other state files in a run
//! directory use, and human-ordered rather than sorted: its entire purpose is to be read by a
//! developer who has just been told that one of its fields disagrees.

use std::fs;
use std::path::Path;

/// Writes `fields` to `path` if this is the run's first invocation, or verifies that `path` already
/// records exactly them if it is not.
///
/// Panics on any disagreement, naming the field. Deliberately not a `Result`: there is no way for a
/// caller to carry on usefully past "this run is not the run you are resuming", and continuing would
/// corrupt a checkpoint series rather than fail it.
pub fn freeze_or_verify(path: &str, fields: &[(&str, String)]) {
    let rendered = render(fields);

    if !Path::new(path).exists() {
        fs::write(path, &rendered).unwrap_or_else(|error| panic!("failed to write {path}: {error}"));
        return;
    }

    let recorded_text =
        fs::read_to_string(path).unwrap_or_else(|error| panic!("failed to read {path}: {error}"));
    if recorded_text == rendered {
        return;
    }

    // Byte equality already failed, so the only job left is to say precisely which field it was.
    let recorded = parse(&recorded_text, path);
    for (name, value) in fields {
        let Some((_, recorded_value)) = recorded.iter().find(|(recorded_name, _)| recorded_name.as_str() == *name)
        else {
            panic!(
                "{path} does not record {name}, so this run was started by a build that froze a different set of \
                 fields. Delete the directory holding {path} and start the run over."
            );
        };
        assert!(
            recorded_value == value,
            "{path}: this run was started with {name} = {recorded_value}, but the configuration now says \
             {name} = {value}. A run's configuration is read-only once it has started: the training cursor and the \
             round boundary are derived from the persisted step count, and the current rate from the recorded \
             losses, so changing this would reinterpret every step already taken. Restore the old value, or delete the directory \
             holding {path} and start the run over."
        );
    }
    for (name, _) in &recorded {
        assert!(
            fields.iter().any(|(current, _)| *current == name.as_str()),
            "{path} records {name}, which this build no longer freezes. Delete the directory holding {path} and \
             start the run over."
        );
    }

    panic!("{path} disagrees with the current configuration field for field in no single field -- it is reordered \
            or holds a duplicate name, so it was not written by this build. Delete the directory holding {path} \
            and start the run over.");
}

/// One `name<TAB>value` line per field, in the order given. Tab-separated for the same reason
/// `history.tsv` is: a value may contain spaces (`Transformer { num_layers: 6, .. }` does) but never
/// a tab, so the split point is unambiguous without quoting.
fn render(fields: &[(&str, String)]) -> String {
    let mut rendered = String::new();
    for (name, value) in fields {
        assert!(
            !name.contains('\t') && !value.contains('\t') && !value.contains('\n'),
            "a frozen field must survive a round trip through one line of text, and {name} = {value} does not"
        );
        rendered.push_str(name);
        rendered.push('\t');
        rendered.push_str(value);
        rendered.push('\n');
    }
    rendered
}

fn parse(text: &str, path: &str) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    for line in text.lines() {
        let (name, value) =
            line.split_once('\t').unwrap_or_else(|| panic!("{path} holds a line with no tab in it: {line:?}"));
        fields.push((name.to_string(), value.to_string()));
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("smithy-run-config-{}-{name}.txt", std::process::id()))
    }

    fn fields(batch_size: u64) -> Vec<(&'static str, String)> {
        vec![("architecture", "Psq".to_string()), ("training_batch_size", batch_size.to_string())]
    }

    #[test]
    fn the_first_invocation_writes_the_file_and_the_second_accepts_it() {
        let path = temp_path("agrees");
        std::fs::remove_file(&path).ok();

        freeze_or_verify(path.to_str().unwrap(), &fields(1024));
        freeze_or_verify(path.to_str().unwrap(), &fields(1024));

        let written = std::fs::read_to_string(&path).expect("the first invocation should have written the file");
        assert_eq!(written, "architecture\tPsq\ntraining_batch_size\t1024\n");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    #[should_panic(expected = "this run was started with training_batch_size = 1024")]
    fn a_changed_field_is_rejected_by_name() {
        let path = temp_path("changed");
        std::fs::remove_file(&path).ok();

        freeze_or_verify(path.to_str().unwrap(), &fields(1024));
        freeze_or_verify(path.to_str().unwrap(), &fields(512));
    }

    #[test]
    #[should_panic(expected = "does not record patience_rounds")]
    fn a_field_this_build_freezes_and_the_file_lacks_is_rejected() {
        let path = temp_path("missing");
        std::fs::remove_file(&path).ok();

        freeze_or_verify(path.to_str().unwrap(), &fields(1024));
        let mut extended = fields(1024);
        extended.push(("patience_rounds", "8".to_string()));
        freeze_or_verify(path.to_str().unwrap(), &extended);
    }

    #[test]
    #[should_panic(expected = "records training_batch_size, which this build no longer freezes")]
    fn a_field_the_file_holds_and_this_build_dropped_is_rejected() {
        let path = temp_path("dropped");
        std::fs::remove_file(&path).ok();

        freeze_or_verify(path.to_str().unwrap(), &fields(1024));
        freeze_or_verify(path.to_str().unwrap(), &[("architecture", "Psq".to_string())]);
    }
}
