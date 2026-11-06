//! What a finished lambda's run leaves behind: its candidate, the best checkpoint of the run, exported
//! to the engine's `.nnue` format beside `candidate.txt` (`export_best`), and the helpers every export
//! shares (`write_engine_network`, `write_record`, `feature_set_of`).

use std::process;

use kalo_chess::nnue::QuantizedPsqNetwork;
use kalo_chess::pipeline::{generation, preprocessing_config};

use crate::feature_set::FeatureSet;
use crate::net_config::{ArchitectureKind, NetConfig};
use crate::stack_head::HeadShape;
use crate::{checkpoint_store, export, history, nnue_network, stages};

/// Exports one lambda's candidate to `gen-<g>/lambda-<l>/candidate.nnue`, with `candidate.txt`
/// beside it naming the checkpoint. Called by `export_best` when the lambda's run stops, so the
/// round robin always has every candidate without a separate command.
///
/// The candidate is the lowest-test-loss checkpoint over every round of the lambda's run. The
/// checkpoint cannot have been pruned: pruning keeps the best rounds. `learning_rate` is the rate
/// that round trained at, written into the `.nnue` header only: `candidate.txt` records none, as
/// `champion.txt` does not (a run steps down through every rate; `history.tsv` keeps each round's).
///
/// Written under a staging name and renamed into place, so the round robin never loads a half-written
/// file, and `candidate.txt` last, since its presence is what marks the lambda's run as finished
/// (`read_candidate_step`). The `.nnue` is written for psq and psq_threats networks only: the engine
/// has no forward pass for the other kinds yet, so for them the record alone ends the run, with a
/// note, rather than failing a run that trained correctly or training it again when the server resumes.
/// The line saying what was exported, for the caller to print.
fn export_candidate(
    config: &NetConfig, run_dir: &str, lambda: preprocessing_config::ArmLambda, checkpoints_dir: &str, learning_rate: f64, step: u64,
    test_loss: f64,
) -> String {
    let checkpoint_path = checkpoint_store::checkpoint_path(checkpoints_dir, step);
    let output_path = generation::candidate_network_path(run_dir);
    let unsupported = match &config.architecture.kind {
        ArchitectureKind::Psq => None,
        ArchitectureKind::PsqThreats => None,
        ArchitectureKind::PsqPairwise => None,
        ArchitectureKind::PsqThreatsPairwise => None,
        ArchitectureKind::PsqStack => None,
        ArchitectureKind::PsqThreatsStack => None,
        ArchitectureKind::PsqThreatsPairwiseMaterial => None,
        ArchitectureKind::PsqThreatsScrelu => None,
        ArchitectureKind::HalfkThreatsScrelu => None,
        ArchitectureKind::HalfkThreatsScreluHidden => None,
        ArchitectureKind::HalfkThreatsPairwiseHidden => None,
        ArchitectureKind::PsqThreatsScreluMaterial => None,
        ArchitectureKind::PsqThreatsScreluPhase => None,
        ArchitectureKind::PsqThreatsScreluCount => None,
        ArchitectureKind::PsqThreatsScreluTaper => None,
        ArchitectureKind::PsqThreatsScreluKings => None,
        ArchitectureKind::PsqThreatsScreluHidden => None,
        ArchitectureKind::PsqThreatsPairwiseHidden => None,
        ArchitectureKind::Transformer { .. } => Some("transformer"),
    };
    let exported = match unsupported {
        None => {
            let staging_path = format!("{run_dir}/candidate.partial.nnue");
            write_engine_network(config, &checkpoint_path, &staging_path, step, learning_rate);
            std::fs::rename(&staging_path, &output_path)
                .unwrap_or_else(|error| panic!("failed to rename {staging_path} to {output_path}: {error}"));
            format!("exported to {output_path}")
        }
        Some(kind) => {
            log::info!("no candidate.nnue: the engine has no forward pass for {kind} networks yet, only psq ones");
            format!("recorded in {}", candidate_record_path(run_dir))
        }
    };
    write_record(
        &candidate_record_path(run_dir),
        &[
            ("lambda", preprocessing_config::lambda_text(lambda)),
            ("step", step.to_string()),
            ("test_loss", test_loss.to_string()),
            ("checkpoint", checkpoint_path),
        ],
    );
    format!("candidate at lambda {lambda}: step {step} (test loss {test_loss:.7}), {exported}")
}

/// Which checkpoint `generation::candidate_network_path` holds. Written last by the export, so its
/// presence is what marks the lambda's run as finished.
pub(crate) fn candidate_record_path(lambda_dir: &str) -> String {
    generation::candidate_record_path(lambda_dir)
}

/// The step of a lambda's candidate, from the `candidate.txt` its export writes: `name<TAB>value`
/// lines, among them `step`. What the server's promotion reads.
pub(crate) fn read_candidate_step(lambda_dir: &str) -> Result<u64, String> {
    let path = candidate_record_path(lambda_dir);
    let text = std::fs::read_to_string(&path).map_err(|error| format!("failed to read {path}: {error}"))?;
    generation::record_field(&text, "step", &path)?.parse().map_err(|error| format!("{path}: invalid step: {error}"))
}

/// Writes `fields` to `path` as one `name<TAB>value` line each, like `run_config.txt`: a value may hold
/// spaces but never a tab. Under a staging name and renamed into place, so a reader never sees a
/// half-written record.
pub(crate) fn write_record(path: &str, fields: &[(&str, String)]) {
    let rendered: String = fields.iter().map(|(name, value)| format!("{name}\t{value}\n")).collect();
    let staging_path = format!("{path}.partial");
    std::fs::write(&staging_path, rendered).unwrap_or_else(|error| panic!("failed to write {staging_path}: {error}"));
    std::fs::rename(&staging_path, path).unwrap_or_else(|error| panic!("failed to rename {staging_path} to {path}: {error}"));
}

/// Exports one psq or psq_threats checkpoint, pairwise or stack or not, to the engine's `.nnue`
/// format at `output_path`.
///
/// On the CPU whatever the machine has: this reads weights and writes a file, with no forward or
/// backward pass to accelerate, and a CUDA context would be pure startup cost.
pub(crate) fn write_engine_network(config: &NetConfig, checkpoint_path: &str, output_path: &str, step: u64, learning_rate: f64) {
    let mut vs = tch::nn::VarStore::new(tch::Device::Cpu);
    let feature_set = feature_set_of(&config.architecture.kind);
    let _ = nnue_network::Network::of(&vs.root(), feature_set, &config.architecture);
    vs.load(checkpoint_path).unwrap_or_else(|error| panic!("failed to load {checkpoint_path}: {error}"));

    let written = match feature_set {
        FeatureSet::Psq | FeatureSet::PsqThreats if config.architecture.stack() => {
            let (embedding_dim, hidden_dims) = (config.architecture.embedding_dim(), config.architecture.value_head_hidden_dim());
            let threats = feature_set == FeatureSet::PsqThreats;
            export::stack_network_from(&vs, threats, embedding_dim, hidden_dims, step, learning_rate).write(output_path)
        }
        FeatureSet::PsqThreats if config.architecture.kings() => {
            let (embedding_dim, hidden_dims) = (config.architecture.embedding_dim(), config.architecture.value_head_hidden_dim());
            export::kings_network_from(&vs, embedding_dim, hidden_dims, step, learning_rate).write(output_path)
        }
        FeatureSet::PsqThreats if config.architecture.hidden_beside() => {
            let (embedding_dim, hidden_dims) = (config.architecture.embedding_dim(), config.architecture.value_head_hidden_dim());
            let pairwise = config.architecture.pairwise();
            export::hidden_beside_network_from(&vs, pairwise, embedding_dim, hidden_dims, step, learning_rate).write(output_path)
        }
        FeatureSet::PsqThreats if config.architecture.head_shape() == HeadShape::Taper => {
            export::taper_network_from(&vs, config.architecture.embedding_dim(), step, learning_rate).write(output_path)
        }
        FeatureSet::PsqThreats if config.architecture.material() => {
            let (screlu, buckets) = (config.architecture.screlu(), config.architecture.head_shape().output_buckets());
            export::material_network_from(&vs, screlu, buckets, config.architecture.embedding_dim(), step, learning_rate)
                .write(output_path)
        }
        FeatureSet::Psq => {
            let (embedding_dim, hidden_dims) = (config.architecture.embedding_dim(), config.architecture.value_head_hidden_dim());
            let pairwise = config.architecture.pairwise();
            export::psq_network_from(&vs, pairwise, embedding_dim, hidden_dims, step, learning_rate).write(output_path)
        }
        FeatureSet::PsqThreats if config.architecture.screlu() => {
            export::screlu_network_from(&vs, config.architecture.embedding_dim(), step, learning_rate).write(output_path)
        }
        FeatureSet::PsqThreats => {
            let (embedding_dim, hidden_dims) = (config.architecture.embedding_dim(), config.architecture.value_head_hidden_dim());
            let pairwise = config.architecture.pairwise();
            export::psq_threats_network_from(&vs, pairwise, embedding_dim, hidden_dims, step, learning_rate).write(output_path)
        }
        // The engine's own quantization first, so that a network whose folded rows do not fit
        // `i16` is refused here, by name, never written to be refused at load.
        FeatureSet::HalfKav2Hm => {
            let (embedding_dim, hidden_dims) = (config.architecture.embedding_dim(), config.architecture.value_head_hidden_dim());
            let pairwise = config.architecture.pairwise();
            let network = export::halfk_network_from(&vs, pairwise, embedding_dim, hidden_dims, step, learning_rate);
            QuantizedPsqNetwork::quantize(&network).and_then(|_| network.write(output_path))
        }
    };
    written.unwrap_or_else(|problem| {
        log::error!("failed to write {output_path}: {problem}");
        process::exit(1);
    });
}

/// The rows and batches each NNUE `architecture.kind` trains on: separate types, since
/// `net_config::ArchitectureKind` also has to carry a non-NNUE value (`Transformer`) that
/// `FeatureSet` has no business knowing about.
pub(crate) fn feature_set_of(kind: &ArchitectureKind) -> FeatureSet {
    match kind {
        ArchitectureKind::Psq => FeatureSet::Psq,
        ArchitectureKind::PsqThreats => FeatureSet::PsqThreats,
        // The same rows and batches as psq: the product is the network's, not the data's.
        ArchitectureKind::PsqPairwise => FeatureSet::Psq,
        // psq_threats' rows and batches, for the same reason.
        ArchitectureKind::PsqThreatsPairwise => FeatureSet::PsqThreats,
        // The same rows again; the buckets every psq batch carries are the stack head's.
        ArchitectureKind::PsqStack => FeatureSet::Psq,
        // psq_threats' rows, whose batches carry the buckets as psq's do.
        ArchitectureKind::PsqThreatsStack => FeatureSet::PsqThreats,
        // psq_threats' rows; the material bucket is the third of the buckets its batches carry.
        ArchitectureKind::PsqThreatsPairwiseMaterial => FeatureSet::PsqThreats,
        // psq_threats' rows and batches: the square is the network's, as the product is.
        ArchitectureKind::PsqThreatsScrelu => FeatureSet::PsqThreats,
        // halfk's rows, and batches of its own carrying each piece row's factor row.
        ArchitectureKind::HalfkThreatsScrelu => FeatureSet::HalfKav2Hm,
        // halfk's rows and batches; the hidden layers read no bucket.
        ArchitectureKind::HalfkThreatsScreluHidden => FeatureSet::HalfKav2Hm,
        ArchitectureKind::HalfkThreatsPairwiseHidden => FeatureSet::HalfKav2Hm,
        // psq_threats' rows; the square the network's, the bucket the batches' third.
        ArchitectureKind::PsqThreatsScreluMaterial => FeatureSet::PsqThreats,
        // psq_threats' rows; the phase bucket is the fourth of the buckets its batches carry.
        ArchitectureKind::PsqThreatsScreluPhase => FeatureSet::PsqThreats,
        // psq_threats' rows; the piece-count bucket is the fifth of the buckets its batches carry.
        ArchitectureKind::PsqThreatsScreluCount => FeatureSet::PsqThreats,
        // psq_threats' rows; the taper reads the phase, the fourth of the buckets its batches carry.
        ArchitectureKind::PsqThreatsScreluTaper => FeatureSet::PsqThreats,
        // psq_threats' rows; the kings' stages read the first two of the buckets its batches carry.
        ArchitectureKind::PsqThreatsScreluKings => FeatureSet::PsqThreats,
        // psq_threats' rows and batches; the hidden layer reads no bucket.
        ArchitectureKind::PsqThreatsScreluHidden => FeatureSet::PsqThreats,
        ArchitectureKind::PsqThreatsPairwiseHidden => FeatureSet::PsqThreats,
        ArchitectureKind::Transformer { .. } => panic!("feature_set_of called with a transformer architecture"),
    }
}

/// Ends one lambda's run: exports its candidate, the best checkpoint of the whole run, into the run's
/// directory. Returns the line saying so, and the best round's test loss, for the server to say in
/// its own way.
pub(crate) fn export_best(config: &NetConfig, run_dir: &str, lambda: preprocessing_config::ArmLambda, stages: &stages::Stages) -> (String, f64) {
    let history_path = format!("{run_dir}/history.tsv");
    let Some(best) = history::read_all(&history_path).into_iter().find(|record| record.step == stages.best_step) else {
        log::error!("{history_path} records no finite test loss, so there is no candidate to export. Stop and look at the run.");
        process::exit(1);
    };
    let checkpoints_dir = format!("{run_dir}/checkpoints");
    let line = export_candidate(config, run_dir, lambda, &checkpoints_dir, best.learning_rate, best.step, best.test_loss);
    (line, best.test_loss)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_set_of_maps_every_nnue_kind() {
        assert_eq!(feature_set_of(&ArchitectureKind::Psq), FeatureSet::Psq);
        assert_eq!(feature_set_of(&ArchitectureKind::PsqThreats), FeatureSet::PsqThreats);
        assert_eq!(feature_set_of(&ArchitectureKind::PsqThreatsPairwise), FeatureSet::PsqThreats);
        assert_eq!(feature_set_of(&ArchitectureKind::PsqPairwise), FeatureSet::Psq);
        assert_eq!(feature_set_of(&ArchitectureKind::PsqStack), FeatureSet::Psq);
        assert_eq!(feature_set_of(&ArchitectureKind::PsqThreatsStack), FeatureSet::PsqThreats);
        assert_eq!(feature_set_of(&ArchitectureKind::PsqThreatsPairwiseMaterial), FeatureSet::PsqThreats);
        assert_eq!(feature_set_of(&ArchitectureKind::PsqThreatsScrelu), FeatureSet::PsqThreats);
        assert_eq!(feature_set_of(&ArchitectureKind::HalfkThreatsScrelu), FeatureSet::HalfKav2Hm);
        assert_eq!(feature_set_of(&ArchitectureKind::HalfkThreatsScreluHidden), FeatureSet::HalfKav2Hm);
        assert_eq!(feature_set_of(&ArchitectureKind::HalfkThreatsPairwiseHidden), FeatureSet::HalfKav2Hm);
        assert_eq!(feature_set_of(&ArchitectureKind::PsqThreatsScreluMaterial), FeatureSet::PsqThreats);
        assert_eq!(feature_set_of(&ArchitectureKind::PsqThreatsScreluPhase), FeatureSet::PsqThreats);
        assert_eq!(feature_set_of(&ArchitectureKind::PsqThreatsScreluCount), FeatureSet::PsqThreats);
        assert_eq!(feature_set_of(&ArchitectureKind::PsqThreatsScreluTaper), FeatureSet::PsqThreats);
        assert_eq!(feature_set_of(&ArchitectureKind::PsqThreatsScreluKings), FeatureSet::PsqThreats);
        assert_eq!(feature_set_of(&ArchitectureKind::PsqThreatsScreluHidden), FeatureSet::PsqThreats);
        assert_eq!(feature_set_of(&ArchitectureKind::PsqThreatsPairwiseHidden), FeatureSet::PsqThreats);
    }

    #[test]
    fn read_candidate_step_takes_the_step() {
        let generation = std::env::temp_dir().join(format!("smithy-candidates-{}-candidate-record", std::process::id()));
        let generation = generation.to_str().unwrap();
        std::fs::remove_dir_all(generation).ok();
        let dir = generation::lambda_dir(generation, kalo_chess::pipeline::preprocessing_config::ArmLambda::Constant(0.7));
        assert!(read_candidate_step(&dir).is_err(), "no candidate.txt at all");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(candidate_record_path(&dir), "lambda\t0.70\nstep\t15872\ntest_loss\t0.04\ncheckpoint\tx/step_000015872.vs\n").unwrap();
        assert_eq!(read_candidate_step(&dir), Ok(15872));

        std::fs::write(candidate_record_path(&dir), "learning_rate\t3e-4\n").unwrap();
        assert!(read_candidate_step(&dir).unwrap_err().contains("no step line"));
        std::fs::remove_dir_all(generation).ok();
    }
}
