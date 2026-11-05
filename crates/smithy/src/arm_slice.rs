//! One arm's slice of a stack of arms, in the files an unstacked arm reads and writes.
//!
//! The stacked network and optimizer hold every parameter as one tensor with a leading arm dimension,
//! under the name an unstacked `VarStore` gives it. On disk nothing is stacked: each lambda keeps its
//! own `model.vs`, checkpoints and `optimizer.opt`, so that export, promote and the arena read them as
//! before, and a run begun with one network per arm resumes with the stack and back. These functions
//! are the whole of that conversion.
//!
//! `VarStore::save` is `Tensor::save_multi` over its named variables and `VarStore::load` copies each
//! variable from the file by name (tch 0.24, `nn/var_store.rs`), so a slice saved with `save_multi`
//! under the same names is a `.vs` file like any other.

use std::collections::HashMap;

use tch::Tensor;

/// Arm `arm`'s slice of `stacked`, as a tensor of its own. A copy rather than a view: a view shares
/// the whole stack's storage, which the serializer would write out in full.
pub fn slice_of(stacked: &Tensor, arm: i64) -> Tensor {
    stacked.select(0, arm).copy()
}

/// Writes `value` into arm `arm`'s slice of `stacked`, in place, outside autograd. Panics if `value`
/// is not shaped like one slice: a file from a differently shaped network is a bug to stop at.
pub fn fill_slice(stacked: &Tensor, arm: i64, value: &Tensor, name: &str) {
    let mut slice = stacked.select(0, arm);
    assert_eq!(
        slice.size(),
        value.size(),
        "{name}: a slice of the stack is shaped {:?}, the value given for it {:?}",
        slice.size(),
        value.size()
    );
    tch::no_grad(|| {
        slice.copy_(value);
    });
}

/// Every tensor of `stacked` cut to arm `arm`'s slice, under the same names.
pub fn arm_tensors(stacked: &HashMap<String, Tensor>, arm: i64) -> Vec<(String, Tensor)> {
    stacked.iter().map(|(name, tensor)| (name.clone(), slice_of(tensor, arm))).collect()
}

/// Saves arm `arm`'s slice of `stacked` to `path`, as `VarStore::save` writes an unstacked network.
pub fn save_arm(stacked: &HashMap<String, Tensor>, arm: i64, path: &str) {
    Tensor::save_multi(&arm_tensors(stacked, arm), path).unwrap_or_else(|error| panic!("failed to save {path}: {error}"));
}

/// Reads the tensors of `path` by name, onto the device of `stacked`.
pub fn read_named(path: &str, device: tch::Device) -> HashMap<String, Tensor> {
    Tensor::load_multi_with_device(path, device)
        .unwrap_or_else(|error| panic!("failed to load {path}: {error}"))
        .into_iter()
        .collect()
}

/// Loads `path`, a `.vs` of one unstacked network, into arm `arm`'s slice of `stacked`. Every tensor
/// of `stacked` must be in the file, shaped as one slice: unlike `VarStore::load`, which leaves a
/// missing variable as it was, a missing one here panics.
pub fn load_arm(stacked: &HashMap<String, Tensor>, arm: i64, path: &str) {
    let Some(device) = stacked.values().next().map(Tensor::device) else {
        return;
    };
    let loaded = read_named(path, device);
    for (name, tensor) in stacked {
        let value = loaded.get(name).unwrap_or_else(|| panic!("{path} holds no {name}"));
        fill_slice(tensor, arm, value, name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::feature_set::FeatureSet;
    use super::super::nnue_network::Network;
    use tch::{nn, Device, Kind};

    /// Two independently initialised psq networks, and their variables stacked arm 0 then arm 1.
    fn two_networks_and_their_stack() -> (Vec<nn::VarStore>, HashMap<String, Tensor>) {
        let stores: Vec<nn::VarStore> = (0..2)
            .map(|_| {
                let vs = nn::VarStore::new(Device::Cpu);
                let _ = Network::new(&vs.root(), FeatureSet::Psq, 8, &[4]);
                vs
            })
            .collect();
        let variables: Vec<HashMap<String, Tensor>> = stores.iter().map(nn::VarStore::variables).collect();
        let stacked = variables[0]
            .keys()
            .map(|name| {
                let slices: Vec<&Tensor> = variables.iter().map(|arm| &arm[name]).collect();
                (name.clone(), Tensor::stack(&slices, 0))
            })
            .collect();
        (stores, stacked)
    }

    fn temp_path(tag: &str) -> String {
        let path = std::env::temp_dir().join(format!("smithy-arm-slice-{}-{tag}.vs", std::process::id()));
        path.to_str().unwrap().to_string()
    }

    #[test]
    fn a_saved_slice_loads_into_an_unstacked_network_as_that_arm() {
        let (stores, stacked) = two_networks_and_their_stack();
        let path = temp_path("save");
        save_arm(&stacked, 1, &path);

        let mut fresh = nn::VarStore::new(Device::Cpu);
        let _ = Network::new(&fresh.root(), FeatureSet::Psq, 8, &[4]);
        fresh.load(&path).unwrap();
        let expected = stores[1].variables();
        for (name, tensor) in fresh.variables() {
            assert!(tensor.equal(&expected[&name]), "{name}");
        }
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn an_unstacked_save_loads_into_one_slice_and_leaves_the_others() {
        let (stores, stacked) = two_networks_and_their_stack();
        let zeros: HashMap<String, Tensor> =
            stacked.iter().map(|(name, tensor)| (name.clone(), tensor.zeros_like())).collect();
        let path = temp_path("load");
        stores[0].save(&path).unwrap();

        load_arm(&zeros, 0, &path);
        let expected = stores[0].variables();
        for (name, tensor) in &zeros {
            assert!(tensor.select(0, 0).equal(&expected[name]), "{name}: arm 0 is the file's");
            let untouched = tensor.select(0, 1);
            assert!(untouched.equal(&untouched.zeros_like()), "{name}: arm 1 is untouched");
        }
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_saved_slice_is_as_small_as_an_unstacked_save() {
        let (stores, stacked) = two_networks_and_their_stack();
        let (sliced, unstacked) = (temp_path("size-sliced"), temp_path("size-unstacked"));
        save_arm(&stacked, 0, &sliced);
        stores[0].save(&unstacked).unwrap();
        let size = |path: &str| std::fs::metadata(path).unwrap().len();
        // Equal up to the archive's own framing, never the whole stack's storage.
        assert!(size(&sliced) * 3 < size(&unstacked) * 4, "{} against {}", size(&sliced), size(&unstacked));
        std::fs::remove_file(&sliced).ok();
        std::fs::remove_file(&unstacked).ok();
    }

    #[test]
    #[should_panic(expected = "holds no extra")]
    fn a_file_missing_a_stacked_tensor_panics() {
        let (stores, mut stacked) = two_networks_and_their_stack();
        stacked.insert("extra".to_string(), Tensor::zeros([2, 3], (Kind::Float, Device::Cpu)));
        let path = temp_path("missing");
        stores[0].save(&path).unwrap();
        load_arm(&stacked, 0, &path);
    }

    #[test]
    #[should_panic(expected = "a slice of the stack is shaped")]
    fn a_misshapen_value_panics() {
        let stacked = Tensor::zeros([2, 3], (Kind::Float, Device::Cpu));
        fill_slice(&stacked, 0, &Tensor::zeros([4], (Kind::Float, Device::Cpu)), "w");
    }
}
