//! Converting a trained `tch` `VarStore` into the engine's own `kalo_chess::nnue::PsqNetwork`.
//!
//! The only place the two representations meet. Everything about the file format itself lives in
//! `kalo_chess::nnue`, so this module is purely the extraction: pull each parameter out of the store by
//! name, put the feature transform into the orientation the engine's gather wants, and hand over a
//! plain struct.

use std::collections::HashMap;

use tch::nn;
use tch::Tensor;

use kalo_chess::features::{halfk, psq, psq_threats};
use kalo_chess::nnue::{Layer, PsqNetwork, StackLayers};
use kalo_chess::stack_buckets::TAPER_SETS;
#[cfg(test)]
use kalo_chess::stack_buckets::{COUNT_BUCKETS, MATERIAL_BUCKETS, PHASE_BUCKETS};

use super::nnue_network::{FIRST_HIDDEN_WEIGHT, FIRST_HIDDEN_WEIGHT_BOUND};
use super::stack_head::{BUCKETS, STAGES};

/// One parameter, by the name `VarStore` filed it under.
///
/// Panics listing every name the store actually holds when one is missing, rather than returning a
/// default or a zero tensor. A renamed layer on the training side would otherwise export a network
/// that loads perfectly and evaluates to nonsense -- and the failure has to name the alternatives,
/// because the exact spelling is `tch`'s own path convention rather than anything this project
/// chooses.
fn parameter<'a>(variables: &'a HashMap<String, Tensor>, name: &str) -> &'a Tensor {
    variables.get(name).unwrap_or_else(|| {
        let mut available: Vec<&String> = variables.keys().collect();
        available.sort();
        panic!(
            "the checkpoint has no parameter named {name:?} -- it holds {:?}. The exporter's names have to \
             match the ones nnue_network.rs registers.",
            available
        )
    })
}

/// A parameter's values, flattened row-major.
fn values(tensor: &Tensor) -> Vec<f32> {
    Vec::try_from(&tensor.contiguous().view([-1])).expect("failed to read a parameter's values")
}

/// Builds the engine-side network from a `VarStore` already loaded with a psq checkpoint:
/// `psq::ROWS` transform rows, and a head reading both perspectives' accumulators,
/// `2 * embedding_dim` values.
///
/// `embedding_dim`/`hidden_dims` come from the same `net.yaml` the checkpoint was trained under;
/// they are not recoverable from the store alone in a way worth trusting (shapes would have to be
/// reverse-engineered), and a mismatch surfaces immediately as a wrong parameter shape here rather
/// than as a wrong number later.
///
/// With `pairwise`, the head reads `embedding_dim` values, each perspective's halves multiplied, so
/// a flag that disagrees with the checkpoint fails on the first head layer's shape.
pub fn psq_network_from(
    vs: &nn::VarStore, pairwise: bool, embedding_dim: i64, hidden_dims: &[i64], step: u64, learning_rate: f64,
) -> PsqNetwork {
    network_from(vs, false, pairwise, embedding_dim, hidden_dims, step, learning_rate)
}

/// `psq_network_from` for a psq_threats checkpoint: `psq_threats::ROWS` transform rows, the same
/// head, pairwise or not.
pub fn psq_threats_network_from(
    vs: &nn::VarStore, pairwise: bool, embedding_dim: i64, hidden_dims: &[i64], step: u64, learning_rate: f64,
) -> PsqNetwork {
    network_from(vs, true, pairwise, embedding_dim, hidden_dims, step, learning_rate)
}

/// `psq_threats_network_from` for a psq_threats_screlu checkpoint: the same parameters under the same
/// names, no hidden layer, the network marked as squaring its lanes. A checkpoint of another shape
/// fails on its head's.
pub fn screlu_network_from(vs: &nn::VarStore, embedding_dim: i64, step: u64, learning_rate: f64) -> PsqNetwork {
    let mut network = network_from(vs, true, false, embedding_dim, &[], step, learning_rate);
    network.screlu = true;
    network
}

/// `screlu_network_from` for a halfk_threats_screlu checkpoint: its transform holds
/// `halfk::TRAINING_ROWS` rows, the engine's and then the factor's. Each piece row becomes its own
/// weights plus its factor row's (`halfk::factor_row`), and the factor rows go, so the engine reads
/// plain per-bucket rows and never the factorisation; the threat rows and the bias as they are.
/// With `hidden_dims` (halfk_threats_screlu_hidden's two), the head's hidden layers as they are; with
/// `pairwise` too (halfk_threats_pairwise_hidden), the first reading `embedding_dim` products.
pub fn halfk_network_from(
    vs: &nn::VarStore, pairwise: bool, embedding_dim: i64, hidden_dims: &[i64], step: u64, learning_rate: f64,
) -> PsqNetwork {
    let head_input = if pairwise { embedding_dim } else { 2 * embedding_dim };
    let (training, transform_bias, hidden, output) =
        weights_from(vs, halfk::TRAINING_ROWS as i64, embedding_dim, head_input, hidden_dims, false);
    let dim = embedding_dim as usize;
    let mut transform = training[..halfk::ROWS * dim].to_vec();
    for row in 0..halfk::HALFK_ROWS {
        let factor = (halfk::ROWS + halfk::factor_row(row)) * dim;
        for (weight, shared) in transform[row * dim..(row + 1) * dim].iter_mut().zip(&training[factor..factor + dim]) {
            *weight += shared;
        }
    }
    PsqNetwork {
        embedding_dim: dim,
        transform,
        transform_bias,
        hidden,
        output,
        step,
        learning_rate,
        threats: true,
        pairwise,
        screlu: !pairwise,
        stack: None,
        material: None,
        halfk: true,
    }
}

fn network_from(
    vs: &nn::VarStore, threats: bool, pairwise: bool, embedding_dim: i64, hidden_dims: &[i64], step: u64, learning_rate: f64,
) -> PsqNetwork {
    let rows = if threats { psq_threats::ROWS } else { psq::ROWS };
    let head_input = if pairwise { embedding_dim } else { 2 * embedding_dim };
    let (transform, transform_bias, hidden, output) = weights_from(vs, rows as i64, embedding_dim, head_input, hidden_dims, true);
    PsqNetwork {
        embedding_dim: embedding_dim as usize,
        transform,
        transform_bias,
        hidden,
        output,
        step,
        learning_rate,
        threats,
        pairwise,
        screlu: false,
        stack: None,
        material: None,
        halfk: false,
    }
}

/// Builds the engine-side network from a `VarStore` loaded with a psq_stack checkpoint, or with
/// `threats` a psq_threats_stack one: its transform, and for each stage (`stack_head::STAGES`) and
/// bucket the summed weights `shared + delta[bucket]`, so the engine sees plain per-bucket layers and
/// never the factorisation.
pub fn stack_network_from(
    vs: &nn::VarStore, threats: bool, embedding_dim: i64, hidden_dims: &[i64], step: u64, learning_rate: f64,
) -> PsqNetwork {
    assert_eq!(hidden_dims.len(), 2, "a stack has two hidden layers, not {hidden_dims:?}");
    let variables = vs.variables();
    let transform_weight = parameter(&variables, "transform.weight");
    let rows = if threats { psq_threats::ROWS } else { psq::ROWS };
    assert_eq!(
        transform_weight.size(), [embedding_dim, rows as i64],
        "transform.weight has an unexpected shape -- does this checkpoint match the config's architecture?"
    );
    let transform = values(&transform_weight.transpose(0, 1));
    let transform_bias = values(parameter(&variables, "transform.bias"));

    let widths = [embedding_dim, hidden_dims[0], hidden_dims[1], 1];
    let mut stages: Vec<Vec<Layer>> =
        (0..3).map(|index| summed_stage(&variables, STAGES[index], widths[index], widths[index + 1], BUCKETS[index])).collect();
    let output = stages.pop().expect("three stages");
    let l2 = stages.pop().expect("three stages");
    let l1 = stages.pop().expect("three stages");
    PsqNetwork {
        embedding_dim: embedding_dim as usize,
        transform,
        transform_bias,
        hidden: Vec::new(),
        output: Layer::default(),
        step,
        learning_rate,
        threats,
        pairwise: true,
        screlu: false,
        stack: Some(StackLayers { l1, l2, output }),
        material: None,
        halfk: false,
    }
}

/// One factorised stage of a stack head, `value_head.{stage}`, as the engine's per-bucket layers:
/// for each of `buckets`, the summed `shared + delta[bucket]`, `in_dim` to `out_dim`.
fn summed_stage(variables: &HashMap<String, Tensor>, stage: &str, in_dim: i64, out_dim: i64, buckets: i64) -> Vec<Layer> {
    let get = |part: &str, shape: &[i64]| -> Tensor {
        let name = format!("value_head.{stage}.{part}");
        let tensor = parameter(variables, &name);
        assert_eq!(tensor.size(), shape, "{name} has an unexpected shape");
        tensor.shallow_clone()
    };
    let weight = get("delta.weight", &[buckets, out_dim, in_dim]) + get("shared.weight", &[out_dim, in_dim]);
    let bias = get("delta.bias", &[buckets, out_dim]) + get("shared.bias", &[out_dim]);
    (0..buckets)
        .map(|bucket| Layer {
            in_dim: in_dim as usize,
            out_dim: out_dim as usize,
            weight: values(&weight.get(bucket)),
            bias: values(&bias.get(bucket)),
        })
        .collect()
}

/// Builds the engine-side network from a `VarStore` loaded with a psq_threats_screlu_kings
/// checkpoint: psq_threats' transform, each side's stage summed per bucket as a stack's
/// (`summed_stage`), reading its `embedding_dim` squares, our side's as the stack's first layer and
/// their side's as its second, and the plain output (`value_head.output`, not factorised, over both
/// sides' values clipped and squared and every square) as the stack's one output set.
pub fn kings_network_from(vs: &nn::VarStore, embedding_dim: i64, hidden_dims: &[i64], step: u64, learning_rate: f64) -> PsqNetwork {
    assert_eq!(hidden_dims.len(), 1, "a kings head has one hidden width, a side's, not {hidden_dims:?}");
    let width = hidden_dims[0];
    let variables = vs.variables();
    let transform_weight = parameter(&variables, "transform.weight");
    assert_eq!(
        transform_weight.size(), [embedding_dim, psq_threats::ROWS as i64],
        "transform.weight has an unexpected shape -- does this checkpoint match the config's architecture?"
    );
    let transform = values(&transform_weight.transpose(0, 1));
    let transform_bias = values(parameter(&variables, "transform.bias"));

    let l1 = summed_stage(&variables, STAGES[0], embedding_dim, width, BUCKETS[0]);
    let l2 = summed_stage(&variables, STAGES[1], embedding_dim, width, BUCKETS[1]);
    let input = 4 * width + 2 * embedding_dim;
    let weight = parameter(&variables, "value_head.output.weight");
    assert_eq!(weight.size(), [1, input], "value_head.output.weight has an unexpected shape");
    let bias = parameter(&variables, "value_head.output.bias");
    assert_eq!(bias.size(), [1], "value_head.output.bias has an unexpected shape");
    let output = vec![Layer { in_dim: input as usize, out_dim: 1, weight: values(weight), bias: values(bias) }];
    PsqNetwork {
        embedding_dim: embedding_dim as usize,
        transform,
        transform_bias,
        hidden: Vec::new(),
        output: Layer::default(),
        step,
        learning_rate,
        threats: true,
        pairwise: false,
        screlu: true,
        stack: Some(StackLayers { l1, l2, output }),
        material: None,
        halfk: false,
    }
}

/// Builds the engine-side network from a `VarStore` loaded with a psq_threats_screlu_hidden
/// checkpoint, or with `pairwise` a psq_threats_pairwise_hidden one: psq_threats' transform, the
/// hidden layer (`value_head.hidden0`, over the `2 * embedding_dim` squares or the `embedding_dim`
/// products) and the output (`value_head.output`, over the hidden values clipped and squared and
/// every square or product), both copied as they are, nothing factorised.
pub fn hidden_beside_network_from(
    vs: &nn::VarStore, pairwise: bool, embedding_dim: i64, hidden_dims: &[i64], step: u64, learning_rate: f64,
) -> PsqNetwork {
    assert_eq!(hidden_dims.len(), 1, "a hidden-beside head has one hidden layer, not {hidden_dims:?}");
    let width = hidden_dims[0];
    let variables = vs.variables();
    let transform_weight = parameter(&variables, "transform.weight");
    assert_eq!(
        transform_weight.size(), [embedding_dim, psq_threats::ROWS as i64],
        "transform.weight has an unexpected shape -- does this checkpoint match the config's architecture?"
    );
    let transform = values(&transform_weight.transpose(0, 1));
    let transform_bias = values(parameter(&variables, "transform.bias"));

    let layer = |name: &str, in_dim: i64, out_dim: i64| -> Layer {
        let weight = parameter(&variables, &format!("value_head.{name}.weight"));
        assert_eq!(weight.size(), [out_dim, in_dim], "value_head.{name}.weight has an unexpected shape");
        let bias = parameter(&variables, &format!("value_head.{name}.bias"));
        assert_eq!(bias.size(), [out_dim], "value_head.{name}.bias has an unexpected shape");
        Layer { in_dim: in_dim as usize, out_dim: out_dim as usize, weight: values(weight), bias: values(bias) }
    };
    let inputs = if pairwise { embedding_dim } else { 2 * embedding_dim };
    let hidden = layer("hidden0", inputs, width);
    let output = layer("output", 2 * width + inputs, 1);
    PsqNetwork {
        embedding_dim: embedding_dim as usize,
        transform,
        transform_bias,
        hidden: vec![hidden],
        output,
        step,
        learning_rate,
        threats: true,
        pairwise,
        screlu: !pairwise,
        stack: None,
        material: None,
        halfk: false,
    }
}

/// Builds the engine-side network from a `VarStore` loaded with a psq_threats_pairwise_material
/// checkpoint, or with `screlu` a psq_threats_screlu_material, _phase or _count one:
/// psq_threats' transform, and for each of the `buckets` buckets (`MATERIAL_BUCKETS`,
/// `PHASE_BUCKETS` or `COUNT_BUCKETS`) its output, the summed `shared + delta[bucket]` of the head's one stage
/// (`stack_head::StackHead::output_only`), so the engine sees plain per-bucket layers, as a stack's.
/// Each set reads `embedding_dim` products, or `2 * embedding_dim` squares.
pub fn material_network_from(
    vs: &nn::VarStore, screlu: bool, buckets: usize, embedding_dim: i64, step: u64, learning_rate: f64,
) -> PsqNetwork {
    let variables = vs.variables();
    let transform_weight = parameter(&variables, "transform.weight");
    assert_eq!(
        transform_weight.size(), [embedding_dim, psq_threats::ROWS as i64],
        "transform.weight has an unexpected shape -- does this checkpoint match the config's architecture?"
    );
    let transform = values(&transform_weight.transpose(0, 1));
    let transform_bias = values(parameter(&variables, "transform.bias"));

    let buckets = buckets as i64;
    let get =|part: &str, shape: &[i64]| -> Tensor {
        let name = format!("value_head.output.{part}");
        let tensor = parameter(&variables, &name);
        assert_eq!(tensor.size(), shape, "{name} has an unexpected shape");
        tensor.shallow_clone()
    };
    let in_dim = if screlu { 2 * embedding_dim } else { embedding_dim };
    let weight = get("delta.weight", &[buckets, 1, in_dim]) + get("shared.weight", &[1, in_dim]);
    let bias = get("delta.bias", &[buckets, 1]) + get("shared.bias", &[1]);
    let material = (0..buckets)
        .map(|bucket| Layer {
            in_dim: in_dim as usize,
            out_dim: 1,
            weight: values(&weight.get(bucket)),
            bias: values(&bias.get(bucket)),
        })
        .collect();
    PsqNetwork {
        embedding_dim: embedding_dim as usize,
        transform,
        transform_bias,
        hidden: Vec::new(),
        output: Layer::default(),
        step,
        learning_rate,
        threats: true,
        pairwise: !screlu,
        screlu,
        stack: None,
        material: Some(material),
        halfk: false,
    }
}

/// Builds the engine-side network from a `VarStore` loaded with a psq_threats_screlu_taper
/// checkpoint: psq_threats' transform, and the head's one layer of two outputs (`value_head.output`,
/// `[2, 2 * embedding_dim]`, not factorised) split into the engine's two sets, middlegame then
/// endgame, which it blends by the phase (`kalo_chess::stack_buckets::taper`).
pub fn taper_network_from(vs: &nn::VarStore, embedding_dim: i64, step: u64, learning_rate: f64) -> PsqNetwork {
    let variables = vs.variables();
    let transform_weight = parameter(&variables, "transform.weight");
    assert_eq!(
        transform_weight.size(), [embedding_dim, psq_threats::ROWS as i64],
        "transform.weight has an unexpected shape -- does this checkpoint match the config's architecture?"
    );
    let transform = values(&transform_weight.transpose(0, 1));
    let transform_bias = values(parameter(&variables, "transform.bias"));

    let (sets, in_dim) = (TAPER_SETS as i64, 2 * embedding_dim);
    let weight = parameter(&variables, "value_head.output.weight");
    assert_eq!(weight.size(), [sets, in_dim], "value_head.output.weight has an unexpected shape");
    let bias = parameter(&variables, "value_head.output.bias");
    assert_eq!(bias.size(), [sets], "value_head.output.bias has an unexpected shape");
    let material = (0..sets)
        .map(|set| Layer {
            in_dim: in_dim as usize,
            out_dim: 1,
            weight: values(&weight.get(set)),
            bias: values(&bias.narrow(0, set, 1)),
        })
        .collect();
    PsqNetwork {
        embedding_dim: embedding_dim as usize,
        transform,
        transform_bias,
        hidden: Vec::new(),
        output: Layer::default(),
        step,
        learning_rate,
        threats: true,
        pairwise: false,
        screlu: true,
        stack: None,
        material: Some(material),
        halfk: false,
    }
}

/// The extraction: the transform (transposed), its bias, each head layer and the output.
/// `head_input` is what the first head layer reads; `bounded` when training kept its weights within
/// `FIRST_HIDDEN_WEIGHT_BOUND` (`Network::weight_bounds`).
fn weights_from(
    vs: &nn::VarStore, rows: i64, embedding_dim: i64, head_input: i64, hidden_dims: &[i64], bounded: bool,
) -> (Vec<f32>, Vec<f32>, Vec<Layer>, Layer) {
    let variables = vs.variables();

    // `nn::linear` stores its weight as `[out, in]`, so the transform arrives as
    // `[embedding_dim, rows]`. The engine wants `[rows, embedding_dim]`: its first layer is a
    // gather over the handful of active feature rows, and each row's whole contribution has to be
    // one contiguous run rather than a strided column walk. Transposed once here instead of at
    // every evaluation for the rest of the network's life.
    let transform_weight = parameter(&variables, "transform.weight");
    assert_eq!(
        transform_weight.size(), [embedding_dim, rows],
        "transform.weight has an unexpected shape -- does this checkpoint match the config's architecture?"
    );
    let transform = values(&transform_weight.transpose(0, 1));
    let transform_bias = values(parameter(&variables, "transform.bias"));

    let mut hidden = Vec::with_capacity(hidden_dims.len());
    let mut input_dim = head_input;
    for (index, &out_dim) in hidden_dims.iter().enumerate() {
        let weight = parameter(&variables, &format!("value_head.hidden{index}.weight"));
        assert_eq!(weight.size(), [out_dim, input_dim], "value_head.hidden{index}.weight has an unexpected shape");
        let weight = values(weight);
        // Training keeps it within the bound (`Network::weight_bounds`), so this only fails on a
        // checkpoint that bypassed it, which the engine's `i8` layer could not hold.
        if index == 0 && bounded {
            let largest = weight.iter().fold(0.0f32, |most, w| most.max(w.abs()));
            assert!(
                largest as f64 <= FIRST_HIDDEN_WEIGHT_BOUND,
                "{FIRST_HIDDEN_WEIGHT} reaches {largest}, beyond the bound of {FIRST_HIDDEN_WEIGHT_BOUND} training keeps"
            );
        }
        hidden.push(Layer {
            in_dim: input_dim as usize,
            out_dim: out_dim as usize,
            weight,
            bias: values(parameter(&variables, &format!("value_head.hidden{index}.bias"))),
        });
        input_dim = out_dim;
    }

    let output_weight = parameter(&variables, "value_head.output.weight");
    assert_eq!(output_weight.size(), [1, input_dim], "value_head.output.weight has an unexpected shape");
    let output = Layer {
        in_dim: input_dim as usize,
        out_dim: 1,
        weight: values(output_weight),
        bias: values(parameter(&variables, "value_head.output.bias")),
    };

    (transform, transform_bias, hidden, output)
}

#[cfg(test)]
mod tests {
    use super::*;

    use kalo_chess::board::Board;

    use super::super::batch::to_dense_batch;
    use super::super::cache::CacheRecord;
    use super::super::feature_set::FeatureSet;
    use super::super::nnue_network::{HeadShape, Network};

    /// One randomly initialised psq network, and the store holding it.
    ///
    /// Both are returned together because they have to be the *same* network. Calling
    /// `Network::new` a second time against the same `VarStore` root does not hand back the
    /// existing parameters -- `tch` mangles the colliding names and initialises fresh ones -- so an
    /// earlier version of this helper, which returned only the store and let each test build its
    /// own `Network`, silently compared two unrelated random networks and failed with values that
    /// merely looked like a forward-pass bug.
    fn store(embedding_dim: i64, hidden_dims: &[i64]) -> (nn::VarStore, Network) {
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let network = Network::new(&vs.root(), FeatureSet::Psq, embedding_dim, hidden_dims);
        (vs, network)
    }

    /// Reads `path` back, insisting it is a psq network.
    fn load_psq(path: &str) -> PsqNetwork {
        let kalo_chess::nnue::Network::Psq(engine, _) = kalo_chess::nnue::Network::load(path).expect("what the exporter wrote should load");
        engine
    }

    /// Positions chosen to reach every part of psq's encoding rather than to be interesting
    /// chess: both sides to move, full and partial and absent castling rights, an en passant file,
    /// and a sparse endgame where almost every row is zero.
    const AGREEMENT_POSITIONS: [&str; 8] = [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR b KQkq - 0 1",
        "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1",
        "r1bqkb1r/pppp1ppp/2n2n2/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4",
        "r3k2r/8/8/8/8/8/8/R3K2R w Kq - 0 1",
        "8/8/8/4k3/8/4K3/8/8 w - - 0 1",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        "4k3/8/8/8/8/8/PPPPPPPP/4K3 b - - 0 1",
    ];

    /// The same position's value in pawns as `tch` computes it, through the exact path training
    /// uses: `extract_canonical`, `to_dense_batch`, `Network::pawns`.
    fn oracle_value(network: &Network, board: &Board) -> f32 {
        let feature_set = match network {
            Network::Psq(_) => FeatureSet::Psq,
            Network::PsqThreats(_) => FeatureSet::PsqThreats,
            Network::HalfKav2Hm(_) => FeatureSet::HalfKav2Hm,
        };
        let record = CacheRecord::new(kalo_chess::features::extract_canonical(board), 0.0);
        let batch = to_dense_batch(&[record], feature_set);
        let output = tch::no_grad(|| network.pawns(&batch));
        f32::try_from(&output.view([-1])).expect("the network produces one value per row")
    }

    /// Asserts two `f32` forward passes agree.
    ///
    /// Not bitwise equality, deliberately: the two implementations sum their products in
    /// different orders, and libtorch is free to vectorize or block its matrix multiplies however
    /// it likes, so the last bits legitimately differ. `1e-4` relative is far tighter than any real
    /// defect -- a missed transpose, a wrong row or a dropped feature all move the value by whole
    /// units, not by parts per ten thousand -- so the tolerance separates float noise from bugs
    /// without being able to hide one.
    fn assert_agrees(engine: f32, oracle: f32, what: &str) {
        let tolerance = 1e-4 * oracle.abs().max(1.0);
        assert!(
            (engine - oracle).abs() <= tolerance,
            "{what}: the engine computed {engine}, tch computed {oracle} (tolerance {tolerance})"
        );
    }

    #[test]
    fn the_engines_psq_forward_pass_agrees_with_tch() {
        // Betty's own shape, (780 -> 256) x 2 -> 1, and a small one with hidden layers, so both
        // the no-hidden-layer head and the clipped ReLU between layers are covered.
        for (embedding_dim, hidden_dims) in [(256, &[][..]), (16, &[12, 8][..])] {
            let (vs, oracle) = store(embedding_dim, hidden_dims);
            let engine = psq_network_from(&vs, false, embedding_dim, hidden_dims, 0, 0.0);

            for fen in AGREEMENT_POSITIONS {
                let (board, _) = Board::from_fen(fen).expect("fixture FENs are valid");
                assert_agrees(engine.evaluate_pawns(&board), oracle_value(&oracle, &board), fen);
            }
        }
    }

    #[test]
    fn the_engines_pairwise_forward_pass_agrees_with_tch_through_a_file() {
        // Greta's shape, (780 -> 256) x 2 with halves multiplied -> 1, and a small one with a
        // hidden layer: exporter, file, loader, f32 forward pass.
        for (embedding_dim, hidden_dims) in [(256, &[][..]), (16, &[8][..])] {
            let path = std::env::temp_dir()
                .join(format!("kalo-chess-export-pairwise-agreement-{embedding_dim}-{}.nnue", std::process::id()))
                .to_str()
                .unwrap()
                .to_string();
            let vs = nn::VarStore::new(tch::Device::Cpu);
            let oracle = Network::with_pairwise(&vs.root(), FeatureSet::Psq, true, embedding_dim, hidden_dims);

            psq_network_from(&vs, true, embedding_dim, hidden_dims, 512, 1e-3).write(&path).expect("export should succeed");
            let engine = load_psq(&path);
            assert!(engine.pairwise, "a pairwise export must load as pairwise");

            for fen in AGREEMENT_POSITIONS {
                let (board, _) = Board::from_fen(fen).expect("fixture FENs are valid");
                assert_agrees(engine.evaluate_pawns(&board), oracle_value(&oracle, &board), fen);
            }
            std::fs::remove_file(&path).ok();
        }
    }

    #[test]
    fn the_engines_stack_forward_pass_agrees_with_tch_through_a_file() {
        // Hilda's shape at a small width, and the same over psq_threats' rows, the deltas randomised
        // so that every position's buckets matter: exporter (shared + delta), file, loader, f32
        // forward pass.
        for feature_set in [FeatureSet::Psq, FeatureSet::PsqThreats] {
            let threats = feature_set == FeatureSet::PsqThreats;
            let path = std::env::temp_dir()
                .join(format!("kalo-chess-export-stack-agreement-{threats}-{}.nnue", std::process::id()))
                .to_str()
                .unwrap()
                .to_string();
            let vs = nn::VarStore::new(tch::Device::Cpu);
            let oracle = Network::with_head(&vs.root(), feature_set, true, HeadShape::Stack, 16, &[8, 8]);
            tch::no_grad(|| {
                for (name, variable) in vs.variables() {
                    if name.contains(".delta.") {
                        variable.shallow_clone().copy_(&(Tensor::randn_like(&variable) * 0.3));
                    }
                }
            });

            stack_network_from(&vs, threats, 16, &[8, 8], 512, 1e-3).write(&path).expect("export should succeed");
            let kalo_chess::nnue::Network::Psq(engine, _) = kalo_chess::nnue::Network::load(&path).expect("the export should load");
            assert!(engine.stack.is_some() && engine.pairwise, "a stack export must load as a stack");
            assert_eq!(engine.threats, threats, "{feature_set:?}");

            for fen in AGREEMENT_POSITIONS {
                let (board, _) = Board::from_fen(fen).expect("fixture FENs are valid");
                assert_agrees(engine.evaluate_pawns(&board), oracle_value(&oracle, &board), &format!("{feature_set:?} {fen}"));
            }
            std::fs::remove_file(&path).ok();
        }
    }

    #[test]
    fn the_engines_material_forward_pass_agrees_with_tch_through_a_file() {
        // psq_threats_pairwise_material at a small width, the deltas randomised so that every
        // position's material bucket matters: exporter (shared + delta), file under its own tag,
        // loader, f32 forward pass. The fixtures span many materials, the start to bare kings.
        let path = std::env::temp_dir()
            .join(format!("kalo-chess-export-material-agreement-{}.nnue", std::process::id()))
            .to_str()
            .unwrap()
            .to_string();
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let oracle = Network::with_head(&vs.root(), FeatureSet::PsqThreats, true, HeadShape::Material, 16, &[]);
        tch::no_grad(|| {
            for (name, variable) in vs.variables() {
                if name.contains(".delta.") {
                    variable.shallow_clone().copy_(&(Tensor::randn_like(&variable) * 0.3));
                }
            }
        });

        material_network_from(&vs, false, MATERIAL_BUCKETS, 16, 512, 1e-3).write(&path).expect("export should succeed");
        let kalo_chess::nnue::Network::Psq(engine, _) = kalo_chess::nnue::Network::load(&path).expect("the export should load");
        assert!(engine.material.is_some() && engine.pairwise && engine.threats, "a material export must load as one");

        for fen in AGREEMENT_POSITIONS {
            let (board, _) = Board::from_fen(fen).expect("fixture FENs are valid");
            assert_agrees(engine.evaluate_pawns(&board), oracle_value(&oracle, &board), fen);
        }
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn the_engines_screlu_material_forward_pass_agrees_with_tch_through_a_file() {
        // psq_threats_screlu_material at a small width, as the pairwise one: the deltas randomised,
        // exporter (shared + delta over the squares), file under tag 13, loader, f32 forward pass.
        let path = std::env::temp_dir()
            .join(format!("kalo-chess-export-screlu-material-agreement-{}.nnue", std::process::id()))
            .to_str()
            .unwrap()
            .to_string();
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let oracle = Network::with_options(&vs.root(), FeatureSet::PsqThreats, false, true, HeadShape::Material, 16, &[]);
        tch::no_grad(|| {
            for (name, variable) in vs.variables() {
                if name.contains(".delta.") {
                    variable.shallow_clone().copy_(&(Tensor::randn_like(&variable) * 0.3));
                }
            }
        });

        material_network_from(&vs, true, MATERIAL_BUCKETS, 16, 512, 1e-3).write(&path).expect("export should succeed");
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), kalo_chess::nnue::FEATURE_SET_PSQ_THREATS_SCRELU_MATERIAL);
        let kalo_chess::nnue::Network::Psq(engine, _) = kalo_chess::nnue::Network::load(&path).expect("the export should load");
        assert!(engine.material.is_some() && engine.screlu && !engine.pairwise && engine.threats, "a screlu material export must load as one");

        for fen in AGREEMENT_POSITIONS {
            let (board, _) = Board::from_fen(fen).expect("fixture FENs are valid");
            assert_agrees(engine.evaluate_pawns(&board), oracle_value(&oracle, &board), fen);
        }
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn the_engines_screlu_phase_forward_pass_agrees_with_tch_through_a_file() {
        // psq_threats_screlu_phase, as the material one: the trainer's bucket from the batch's phase
        // column, the engine's from the board, through a file under tag 14.
        let path = std::env::temp_dir()
            .join(format!("kalo-chess-export-screlu-phase-agreement-{}.nnue", std::process::id()))
            .to_str()
            .unwrap()
            .to_string();
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let oracle = Network::with_options(&vs.root(), FeatureSet::PsqThreats, false, true, HeadShape::Phase, 16, &[]);
        tch::no_grad(|| {
            for (name, variable) in vs.variables() {
                if name.contains(".delta.") {
                    variable.shallow_clone().copy_(&(Tensor::randn_like(&variable) * 0.3));
                }
            }
        });

        material_network_from(&vs, true, PHASE_BUCKETS, 16, 512, 1e-3).write(&path).expect("export should succeed");
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), kalo_chess::nnue::FEATURE_SET_PSQ_THREATS_SCRELU_PHASE);
        let kalo_chess::nnue::Network::Psq(engine, _) = kalo_chess::nnue::Network::load(&path).expect("the export should load");
        assert_eq!(engine.material.as_ref().map(|material| material.len()), Some(PHASE_BUCKETS));

        for fen in AGREEMENT_POSITIONS {
            let (board, _) = Board::from_fen(fen).expect("fixture FENs are valid");
            assert_agrees(engine.evaluate_pawns(&board), oracle_value(&oracle, &board), fen);
        }
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn the_engines_screlu_count_forward_pass_agrees_with_tch_through_a_file() {
        // psq_threats_screlu_count, as the phase one: the trainer's bucket from the batch's count
        // column, the engine's from the board, through a file under tag 15.
        let path = std::env::temp_dir()
            .join(format!("kalo-chess-export-screlu-count-agreement-{}.nnue", std::process::id()))
            .to_str()
            .unwrap()
            .to_string();
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let oracle = Network::with_options(&vs.root(), FeatureSet::PsqThreats, false, true, HeadShape::Count, 16, &[]);
        tch::no_grad(|| {
            for (name, variable) in vs.variables() {
                if name.contains(".delta.") {
                    variable.shallow_clone().copy_(&(Tensor::randn_like(&variable) * 0.3));
                }
            }
        });

        material_network_from(&vs, true, COUNT_BUCKETS, 16, 512, 1e-3).write(&path).expect("export should succeed");
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), kalo_chess::nnue::FEATURE_SET_PSQ_THREATS_SCRELU_COUNT);
        let kalo_chess::nnue::Network::Psq(engine, _) = kalo_chess::nnue::Network::load(&path).expect("the export should load");
        assert_eq!(engine.material.as_ref().map(|material| material.len()), Some(COUNT_BUCKETS));

        for fen in AGREEMENT_POSITIONS {
            let (board, _) = Board::from_fen(fen).expect("fixture FENs are valid");
            assert_agrees(engine.evaluate_pawns(&board), oracle_value(&oracle, &board), fen);
        }
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn the_engines_screlu_taper_forward_pass_agrees_with_tch_through_a_file() {
        // psq_threats_screlu_taper: the trainer blends its two outputs by the batch's phase column,
        // the engine its two sets by the board's phase, through a file under tag 16. The two sets
        // start from `nn::linear`'s random initialisation, so they differ and a swap shows.
        let path = std::env::temp_dir()
            .join(format!("kalo-chess-export-screlu-taper-agreement-{}.nnue", std::process::id()))
            .to_str()
            .unwrap()
            .to_string();
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let oracle = Network::with_options(&vs.root(), FeatureSet::PsqThreats, false, true, HeadShape::Taper, 16, &[]);

        taper_network_from(&vs, 16, 512, 1e-3).write(&path).expect("export should succeed");
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), kalo_chess::nnue::FEATURE_SET_PSQ_THREATS_SCRELU_TAPER);
        let kalo_chess::nnue::Network::Psq(engine, _) = kalo_chess::nnue::Network::load(&path).expect("the export should load");
        assert_eq!(engine.material.as_ref().map(|material| material.len()), Some(TAPER_SETS));

        for fen in AGREEMENT_POSITIONS {
            let (board, _) = Board::from_fen(fen).expect("fixture FENs are valid");
            assert_agrees(engine.evaluate_pawns(&board), oracle_value(&oracle, &board), fen);
        }
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn the_engines_kings_forward_pass_agrees_with_tch_through_a_file() {
        // psq_threats_screlu_kings at a small width, the deltas randomised so that every position's
        // kings matter: exporter (shared + delta for each side's stage, the plain output as is),
        // file under tag 17, loader, f32 forward pass, their king flipped on both sides alike.
        let path = std::env::temp_dir()
            .join(format!("kalo-chess-export-screlu-kings-agreement-{}.nnue", std::process::id()))
            .to_str()
            .unwrap()
            .to_string();
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let oracle = Network::with_options(&vs.root(), FeatureSet::PsqThreats, false, true, HeadShape::Kings, 16, &[8]);
        tch::no_grad(|| {
            for (name, variable) in vs.variables() {
                if name.contains(".delta.") {
                    variable.shallow_clone().copy_(&(Tensor::randn_like(&variable) * 0.3));
                }
            }
        });

        kings_network_from(&vs, 16, &[8], 512, 1e-3).write(&path).expect("export should succeed");
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), kalo_chess::nnue::FEATURE_SET_PSQ_THREATS_SCRELU_KINGS);
        let kalo_chess::nnue::Network::Psq(engine, _) = kalo_chess::nnue::Network::load(&path).expect("the export should load");
        assert!(engine.screlu && engine.threats && !engine.pairwise, "a kings export must load as one");
        assert_eq!(engine.stack.as_ref().map(|stack| stack.output.len()), Some(1));

        for fen in AGREEMENT_POSITIONS {
            let (board, _) = Board::from_fen(fen).expect("fixture FENs are valid");
            assert_agrees(engine.evaluate_pawns(&board), oracle_value(&oracle, &board), fen);
        }
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn the_engines_screlu_and_pairwise_hidden_forward_passes_agree_with_tch_through_a_file() {
        // psq_threats_screlu_hidden and psq_threats_pairwise_hidden at a small width: exporter (the
        // hidden layer and the output as they are), file under tag 18 or 19, loader, f32 forward pass.
        for (kind, pairwise, tag) in [
            ("screlu", false, kalo_chess::nnue::FEATURE_SET_PSQ_THREATS_SCRELU_HIDDEN),
            ("pairwise", true, kalo_chess::nnue::FEATURE_SET_PSQ_THREATS_PAIRWISE_HIDDEN),
        ] {
            let path = std::env::temp_dir()
                .join(format!("kalo-chess-export-{kind}-hidden-agreement-{}.nnue", std::process::id()))
                .to_str()
                .unwrap()
                .to_string();
            let vs = nn::VarStore::new(tch::Device::Cpu);
            let oracle = Network::with_options(&vs.root(), FeatureSet::PsqThreats, pairwise, !pairwise, HeadShape::Hidden, 16, &[8]);

            hidden_beside_network_from(&vs, pairwise, 16, &[8], 512, 1e-3).write(&path).expect("export should succeed");
            let bytes = std::fs::read(&path).unwrap();
            assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), tag, "{kind}");
            let kalo_chess::nnue::Network::Psq(engine, _) = kalo_chess::nnue::Network::load(&path).expect("the export should load");
            assert!(
                engine.screlu == !pairwise && engine.pairwise == pairwise && engine.threats && engine.hidden.len() == 1,
                "a {kind} hidden export must load as one"
            );

            for fen in AGREEMENT_POSITIONS {
                let (board, _) = Board::from_fen(fen).expect("fixture FENs are valid");
                assert_agrees(engine.evaluate_pawns(&board), oracle_value(&oracle, &board), fen);
            }
            std::fs::remove_file(&path).ok();
        }
    }

    #[test]
    #[should_panic(expected = "unexpected shape")]
    fn a_pairwise_flag_that_disagrees_with_the_checkpoint_is_refused() {
        let (vs, _) = store(8, &[]);
        let _ = psq_network_from(&vs, true, 8, &[], 0, 0.0);
    }

    #[test]
    fn the_engines_psq_threats_forward_pass_agrees_with_tch_through_a_file() {
        // Emily's shape, (4876 -> 256) x 2 -> 16 -> 1, end to end: trained weights, exporter,
        // file, loader, and the f32 forward pass. The fixtures hold threats of both sides. The
        // search's quantized path cannot meet this tolerance by design; `nnue.rs` tests it exactly
        // against its own from-scratch reference instead.
        let path = std::env::temp_dir()
            .join(format!("kalo-chess-export-psq-threats-agreement-{}.nnue", std::process::id()))
            .to_str()
            .unwrap()
            .to_string();
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let oracle = Network::new(&vs.root(), FeatureSet::PsqThreats, 256, &[16]);

        psq_threats_network_from(&vs, false, 256, &[16], 512, 1e-3).write(&path).expect("export should succeed");
        let kalo_chess::nnue::Network::Psq(engine, _) = kalo_chess::nnue::Network::load(&path).expect("the export should load");
        assert!(engine.threats && !engine.pairwise, "a psq_threats export must load as psq_threats");

        for fen in AGREEMENT_POSITIONS {
            let (board, _) = Board::from_fen(fen).expect("fixture FENs are valid");
            assert_agrees(engine.evaluate_pawns(&board), oracle_value(&oracle, &board), fen);
        }
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn the_engines_psq_threats_pairwise_forward_pass_agrees_with_tch_through_a_file() {
        // The experiments' shape, (4876 -> 256) x 2 with halves multiplied -> 1, and a small one
        // with a hidden layer: exporter, file, loader, f32 forward pass, threats of both sides.
        for (embedding_dim, hidden_dims) in [(256, &[][..]), (16, &[8][..])] {
            let path = std::env::temp_dir()
                .join(format!("kalo-chess-export-psq-threats-pairwise-agreement-{embedding_dim}-{}.nnue", std::process::id()))
                .to_str()
                .unwrap()
                .to_string();
            let vs = nn::VarStore::new(tch::Device::Cpu);
            let oracle = Network::with_pairwise(&vs.root(), FeatureSet::PsqThreats, true, embedding_dim, hidden_dims);

            psq_threats_network_from(&vs, true, embedding_dim, hidden_dims, 512, 1e-3).write(&path).expect("export should succeed");
            let kalo_chess::nnue::Network::Psq(engine, _) = kalo_chess::nnue::Network::load(&path).expect("the export should load");
            assert!(engine.threats && engine.pairwise, "a psq_threats_pairwise export must load as both");

            for fen in AGREEMENT_POSITIONS {
                let (board, _) = Board::from_fen(fen).expect("fixture FENs are valid");
                assert_agrees(engine.evaluate_pawns(&board), oracle_value(&oracle, &board), fen);
            }
            std::fs::remove_file(&path).ok();
        }
    }

    #[test]
    fn the_engines_psq_threats_screlu_forward_pass_agrees_with_tch_through_a_file() {
        // The experiment's shape, (4876 -> 256) x 2 squared -> 1: exporter, file, loader, f32 forward
        // pass, threats of both sides.
        let path = std::env::temp_dir()
            .join(format!("kalo-chess-export-psq-threats-screlu-agreement-{}.nnue", std::process::id()))
            .to_str()
            .unwrap()
            .to_string();
        let vs = nn::VarStore::new(tch::Device::Cpu);
        let oracle = Network::with_options(&vs.root(), FeatureSet::PsqThreats, false, true, crate::stack_head::HeadShape::Plain, 256, &[]);

        screlu_network_from(&vs, 256, 512, 1e-3).write(&path).expect("export should succeed");
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), kalo_chess::nnue::FEATURE_SET_PSQ_THREATS_SCRELU);
        let kalo_chess::nnue::Network::Psq(engine, _) = kalo_chess::nnue::Network::load(&path).expect("the export should load");
        assert!(engine.threats && engine.screlu && !engine.pairwise, "a psq_threats_screlu export must load as one");

        for fen in AGREEMENT_POSITIONS {
            let (board, _) = Board::from_fen(fen).expect("fixture FENs are valid");
            assert_agrees(engine.evaluate_pawns(&board), oracle_value(&oracle, &board), fen);
        }
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn the_engines_halfk_forward_pass_agrees_with_tch_through_a_file() {
        // The factorised training network against the folded engine one: exporter, file, loader,
        // f32 forward pass, on positions whose kings stand in many buckets, mirrored and not. With
        // no hidden layer and with two, over the squares and over the products, the first hidden
        // layer's weights drawn beyond psq's bound, which halfk does not keep.
        for (pairwise, hidden_dims, tag) in [
            (false, &[][..], kalo_chess::nnue::FEATURE_SET_HALFK_THREATS_SCRELU),
            (false, &[16, 32][..], kalo_chess::nnue::FEATURE_SET_HALFK_THREATS_SCRELU_HIDDEN),
            (true, &[16, 32][..], kalo_chess::nnue::FEATURE_SET_HALFK_THREATS_PAIRWISE_HIDDEN),
        ] {
            let path = std::env::temp_dir()
                .join(format!("kalo-chess-export-halfk-agreement-{tag}-{}.nnue", std::process::id()))
                .to_str()
                .unwrap()
                .to_string();
            let vs = nn::VarStore::new(tch::Device::Cpu);
            let oracle =
                Network::with_options(&vs.root(), FeatureSet::HalfKav2Hm, pairwise, !pairwise, HeadShape::Plain, 32, hidden_dims);
            // The piece rows start at zero; drawn here, so that a row of the wrong bucket, or a factor
            // folded into the wrong row, moves the value.
            tch::no_grad(|| {
                let variables = vs.variables();
                let mut pieces = variables["transform.weight"].narrow(1, 0, halfk::HALFK_ROWS as i64);
                let _ = pieces.uniform_(-0.05, 0.05);
                if let Some(first) = variables.get(FIRST_HIDDEN_WEIGHT) {
                    let _ = first.shallow_clone().uniform_(-3.0, 3.0);
                }
            });

            halfk_network_from(&vs, pairwise, 32, hidden_dims, 512, 1e-3).write(&path).expect("export should succeed");
            let bytes = std::fs::read(&path).unwrap();
            assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), tag);
            let engine = load_psq(&path);
            assert!(engine.halfk && engine.threats && engine.screlu != engine.pairwise, "a halfk export must load as one");
            assert_eq!(engine.pairwise, pairwise);
            assert_eq!(engine.hidden.len(), hidden_dims.len());

            for fen in AGREEMENT_POSITIONS {
                let (board, _) = Board::from_fen(fen).expect("fixture FENs are valid");
                assert_agrees(engine.evaluate_pawns(&board), oracle_value(&oracle, &board), fen);
            }
            std::fs::remove_file(&path).ok();
        }
    }

    #[test]
    fn psq_agreement_survives_the_round_trip_through_a_file() {
        // The whole chain end to end, which is what the gate actually runs: trained weights ->
        // exporter -> bytes on disk -> engine loader -> forward pass. The test above uses an
        // in-memory `PsqNetwork`, so it would not notice a format that wrote or read a block in
        // the wrong place.
        let path = std::env::temp_dir()
            .join(format!("kalo-chess-export-psq-agreement-{}.nnue", std::process::id()))
            .to_str()
            .unwrap()
            .to_string();
        let (vs, oracle) = store(16, &[8]);

        psq_network_from(&vs, false, 16, &[8], 512, 1e-3).write(&path).expect("export should succeed");
        let engine = load_psq(&path);

        for fen in AGREEMENT_POSITIONS {
            let (board, _) = Board::from_fen(fen).expect("fixture FENs are valid");
            assert_agrees(engine.evaluate_pawns(&board), oracle_value(&oracle, &board), fen);
        }
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn the_psq_agreement_test_would_notice_swapped_perspectives() {
        // A mistake specific to two perspectives: concatenating the two accumulators in the wrong
        // order. Evaluating the engine with its accumulators swapped must disagree with tch on some
        // position. The weights are random and unseeded (`tch::manual_seed` is one global the
        // parallel tests share), and a draw whose hidden layer saturates everywhere answers the same
        // value whatever its input, swapped or not (seen 2026-10-22): such a draw is drawn again.
        let boards: Vec<Board> = AGREEMENT_POSITIONS.iter().map(|fen| Board::from_fen(fen).unwrap().0).collect();
        for _ in 0..10 {
            let (vs, oracle) = store(16, &[8]);
            let truths: Vec<f32> = boards.iter().map(|board| oracle_value(&oracle, board)).collect();
            if truths.iter().all(|truth| (truth - truths[0]).abs() <= 1e-6) {
                continue;
            }
            let engine = psq_network_from(&vs, false, 16, &[8], 0, 0.0);
            let noticed = boards.iter().zip(&truths).any(|(board, &truth)| {
                let side_to_move = board.side_to_move();
                let own = engine.refresh(board, side_to_move);
                let opponent = engine.refresh(board, side_to_move.opposite());
                let swapped = engine.evaluate_accumulators(&opponent, &own);
                (swapped - truth).abs() > 1e-4 * truth.abs().max(1.0)
            });
            assert!(noticed, "swapped perspectives agreed with tch on every position -- the agreement test cannot tell");
            return;
        }
        panic!("ten random networks in a row answered every position alike");
    }

    #[test]
    fn the_agreement_test_would_actually_notice_a_scrambled_transform() {
        // A test that only ever passes proves nothing, so this checks the check: skipping the
        // transpose is the single most likely real mistake on this path, and it produces a file of
        // exactly the right size full of misplaced weights. Reconstructing that mistake here must
        // make the comparison fail.
        let embedding_dim = 16;
        let hidden_dims = [12, 8];
        let (vs, oracle) = store(embedding_dim, &hidden_dims);
        let mut engine = psq_network_from(&vs, false, embedding_dim, &hidden_dims, 0, 0.0);
        engine.transform = values(parameter(&vs.variables(), "transform.weight"));

        let (board, _) = Board::from_fen(AGREEMENT_POSITIONS[0]).unwrap();
        let untransposed = engine.evaluate_pawns(&board);
        let truth = oracle_value(&oracle, &board);
        assert!(
            (untransposed - truth).abs() > 1e-4 * truth.abs().max(1.0),
            "an untransposed transform produced {untransposed} against tch's {truth} -- the agreement test \
             cannot tell the two apart, so it is not testing what it claims to"
        );
    }

    #[test]
    fn the_transform_comes_out_transposed_element_for_element() {
        // The one layout change between the two representations, and the one that no shape check
        // could catch on its own: `[embedding_dim, ROWS]` and `[ROWS, embedding_dim]` hold the same
        // count of floats, so a missed transpose produces a perfectly well-formed file whose
        // weights are scrambled. Checked exhaustively at a small width rather than spot-checked.
        let embedding_dim = 3;
        let (vs, _) = store(embedding_dim, &[4]);
        let trained = values(parameter(&vs.variables(), "transform.weight"));

        let network = psq_network_from(&vs, false, embedding_dim, &[4], 512, 1e-3);

        for row in 0..psq::ROWS {
            for column in 0..embedding_dim as usize {
                assert_eq!(
                    network.transform[row * embedding_dim as usize + column],
                    trained[column * psq::ROWS + row],
                    "transform mismatch at row {row}, column {column}"
                );
            }
        }
    }

    #[test]
    fn every_value_head_layer_arrives_with_its_declared_shape() {
        let (vs, _) = store(8, &[6, 4]);
        let network = psq_network_from(&vs, false, 8, &[6, 4], 0, 0.0);

        assert_eq!(network.embedding_dim, 8);
        assert_eq!(network.hidden.len(), 2);
        assert_eq!((network.hidden[0].in_dim, network.hidden[0].out_dim), (16, 6));
        assert_eq!((network.hidden[1].in_dim, network.hidden[1].out_dim), (6, 4));
        assert_eq!((network.output.in_dim, network.output.out_dim), (4, 1));
    }

    #[test]
    #[should_panic(expected = "beyond the bound")]
    fn a_first_hidden_weight_beyond_the_bound_is_refused() {
        let (vs, _) = store(8, &[4]);
        tch::no_grad(|| {
            let _ = vs.variables()[FIRST_HIDDEN_WEIGHT].get(0).get(0).fill_(2.5);
        });
        let _ = psq_network_from(&vs, false, 8, &[4], 0, 0.0);
    }

    #[test]
    fn an_exported_network_survives_a_write_and_a_load() {
        // Ties this module to the format: what the exporter builds has to be something
        // `Network::load` accepts, including every shape agreement `validate` enforces.
        let path = std::env::temp_dir()
            .join(format!("kalo-chess-export-round-trip-{}.nnue", std::process::id()))
            .to_str()
            .unwrap()
            .to_string();
        let (vs, _) = store(8, &[6, 4]);
        let network = psq_network_from(&vs, false, 8, &[6, 4], 80896, 1e-3);

        network.write(&path).expect("an exported network should be writable");
        assert_eq!(load_psq(&path), network);

        std::fs::remove_file(&path).ok();
    }
}
