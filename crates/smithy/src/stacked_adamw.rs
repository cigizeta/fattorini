//! `adamw::AdamW` over a stack of arms (`stacked_network.rs`): the same update, written once over
//! tensors with a leading arm dimension, so a step launches its kernels once for every arm.
//!
//! Every operation of AdamW is elementwise, so the arms cannot reach each other. What is a scalar in
//! `AdamW` becomes one value per arm here: the rate (arms change stage at different rounds), the step
//! count, and with it the warmup factor and the bias corrections. The counts live on the device and
//! the factors are computed there every step, a handful of `[arms]` kernels: uploading them from the
//! host each step would make this thread wait for the GPU to drain the queue before the copy.
//!
//! An arm can be held still (`set_stepping`): its parameters, moments and count do not move, which is
//! how an arm ahead of the stream waits for it (`round::run_round`).
//!
//! On disk each arm's state is an `AdamW` file (`m.<name>`, `v.<name>`, `t`), so either optimizer
//! reads what the other wrote.

use std::collections::HashMap;

use tch::{Device, Kind, Tensor};

use super::arm_slice;
use super::stacked_network::retained_index;

pub struct StackedAdamW {
    beta1: f64,
    beta2: f64,
    eps: f64,
    wd: f64,
    warmup_steps: i64,
    parameters: HashMap<String, Tensor>,
    moments: HashMap<String, (Tensor, Tensor)>,
    bounds: HashMap<String, f64>,
    /// Each arm's rate, and the same as an `[arms]` tensor on the device, rewritten when one changes.
    rates: Vec<f64>,
    rates_on_device: Tensor,
    /// Each arm's step count, and the same as an `[arms]` double tensor on the device: the host copy
    /// answers `step_count` without waiting for the GPU, the device one feeds the factors.
    counts: Vec<i64>,
    counts_on_device: Tensor,
    /// Each arm's run step: the batches it has stepped on, which only grows. Equal to its count until a
    /// rollback to an earlier best (`restore_arm`, `reset_arm`) puts the count back: the count drives
    /// the bias corrections and warmup, which belong to the restored state, while the run step drives
    /// where the arm stands in the stream, which a rollback does not rewind.
    run_steps: Vec<i64>,
    /// Which arms the next steps move, and the same on the device as `1.0`/`0.0`. `None` when every
    /// arm steps, the usual case, which skips the masking kernels altogether.
    stepping: Vec<bool>,
    stepping_on_device: Option<Tensor>,
    device: Device,
}

impl StackedAdamW {
    /// The defaults of `AdamW::new` (`beta1 = 0.9`, `beta2 = 0.999`, `eps = 1e-8`, `wd = 0.01`), over
    /// `parameters`, each `[arms, ...]`, every arm at rate `rate` and step 0.
    pub fn new(parameters: &HashMap<String, Tensor>, rate: f64) -> StackedAdamW {
        let first = parameters.values().next().expect("an optimizer needs parameters");
        let (arms, device) = (first.size()[0], first.device());
        let moments = parameters
            .iter()
            .map(|(name, tensor)| {
                assert_eq!(tensor.size()[0], arms, "{name} holds a different number of arms");
                (name.clone(), (tensor.zeros_like().detach(), tensor.zeros_like().detach()))
            })
            .collect();
        let rates = vec![rate; arms as usize];
        let counts = vec![0; arms as usize];
        StackedAdamW {
            beta1: 0.9,
            beta2: 0.999,
            eps: 1e-8,
            wd: 0.01,
            warmup_steps: 0,
            parameters: parameters.iter().map(|(name, tensor)| (name.clone(), tensor.shallow_clone())).collect(),
            moments,
            bounds: HashMap::new(),
            rates_on_device: Self::upload_f64(&rates, device),
            rates,
            counts_on_device: Self::upload_counts(&counts, device),
            run_steps: counts.clone(),
            counts,
            stepping: vec![true; arms as usize],
            stepping_on_device: None,
            device,
        }
    }

    fn upload_f64(values: &[f64], device: Device) -> Tensor {
        Tensor::from_slice(values).to_device(device)
    }

    fn upload_counts(counts: &[i64], device: Device) -> Tensor {
        Tensor::from_slice(counts).to_kind(Kind::Double).to_device(device)
    }

    pub fn arms(&self) -> usize {
        self.rates.len()
    }

    /// `AdamW::bound`, for every arm alike.
    pub fn bound(&mut self, name: &str, bound: f64) {
        assert!(bound.is_finite() && bound > 0.0, "a bound of {bound} is meaningless");
        let param = self.parameters.get_mut(name).unwrap_or_else(|| panic!("no parameter named {name} to bound"));
        tch::no_grad(|| {
            let _ = param.clamp_(-bound, bound);
        });
        self.bounds.insert(name.to_string(), bound);
    }

    /// `AdamW::set_warmup_steps`, for every arm alike: each ramps by its own count.
    pub fn set_warmup_steps(&mut self, steps: i64) {
        assert!(steps >= 0, "a warmup of {steps} steps is meaningless");
        self.warmup_steps = steps;
    }

    /// `AdamW::set_learning_rate` for arm `arm` alone.
    pub fn set_learning_rate(&mut self, arm: usize, lr: f64) {
        assert!(lr.is_finite() && lr > 0.0, "a learning rate of {lr} is meaningless");
        self.rates[arm] = lr;
        self.rates_on_device = Self::upload_f64(&self.rates, self.device);
    }

    pub fn learning_rate(&self, arm: usize) -> f64 {
        self.rates[arm]
    }

    /// Arm `arm`'s count, `AdamW`'s `t`: what its bias corrections and warmup read. Only the tests
    /// read it: the training loop counts the run's step (`run_step`).
    #[cfg(test)]
    pub fn step_count(&self, arm: usize) -> i64 {
        self.counts[arm]
    }

    /// Arm `arm`'s run step: the batches it has stepped on, never put back by a rollback.
    pub fn run_step(&self, arm: usize) -> i64 {
        self.run_steps[arm]
    }

    /// Which arms the next steps move: an arm left out keeps its parameters, moments and count.
    pub fn set_stepping(&mut self, stepping: &[bool]) {
        assert_eq!(stepping.len(), self.arms(), "one flag per arm");
        if stepping == self.stepping {
            return;
        }
        self.stepping = stepping.to_vec();
        self.stepping_on_device = Self::upload_stepping(stepping, self.device);
    }

    /// `stepping` as `1.0`/`0.0` on the device, or `None` when every arm steps.
    fn upload_stepping(stepping: &[bool], device: Device) -> Option<Tensor> {
        if stepping.iter().all(|&steps| steps) {
            return None;
        }
        let flags: Vec<f64> = stepping.iter().map(|&steps| if steps { 1.0 } else { 0.0 }).collect();
        Some(Self::upload_f64(&flags, device))
    }

    pub fn zero_grad(&mut self) {
        for param in self.parameters.values_mut() {
            param.zero_grad();
        }
    }

    /// `[arms]` as `[arms, 1, ...]`, to broadcast against `like`.
    fn per_arm(values: &Tensor, like: &Tensor) -> Tensor {
        let mut shape = vec![values.size()[0]];
        shape.extend(std::iter::repeat_n(1, like.dim() - 1));
        values.view(shape.as_slice())
    }

    /// One AdamW update of every stepping arm, over the gradients `backward()` left: `AdamW::step`'s
    /// arithmetic, with each arm's count, warmup and bias corrections its own.
    pub fn step(&mut self) {
        for ((count, run_step), &steps) in self.counts.iter_mut().zip(self.run_steps.iter_mut()).zip(&self.stepping) {
            if steps {
                *count += 1;
                *run_step += 1;
            }
        }
        tch::no_grad(|| {
            self.counts_on_device = match &self.stepping_on_device {
                Some(flags) => &self.counts_on_device + flags,
                None => &self.counts_on_device + 1.0,
            };
            let t = &self.counts_on_device;
            // Clamped away from zero: an arm held still at step 0 has nothing to correct, and its
            // update is discarded below, but a division by zero there would still be computed.
            let bias_correction1 = (-(t * self.beta1.ln()).exp() + 1.0).clamp_min(1e-30).to_kind(Kind::Float);
            let bias_correction2 = (-(t * self.beta2.ln()).exp() + 1.0).clamp_min(1e-30).to_kind(Kind::Float);
            let warmup = if self.warmup_steps == 0 {
                t.ones_like()
            } else {
                (t / self.warmup_steps as f64).clamp_max(1.0)
            };
            let lr = (&self.rates_on_device * warmup).to_kind(Kind::Float);
            let stepping = self.stepping_on_device.as_ref().map(|flags| flags.to_kind(Kind::Bool));

            for (name, param) in self.parameters.iter_mut() {
                let grad = param.grad();
                assert!(grad.defined(), "StackedAdamW::step found an undefined gradient for {name} -- was backward() run first?");
                let (m, v) = self.moments.get_mut(name.as_str()).expect("moment buffers missing for a tracked parameter");

                let new_m = &*m * self.beta1 + &grad * (1.0 - self.beta1);
                let new_v = &*v * self.beta2 + &grad * &grad * (1.0 - self.beta2);
                let m_hat = &new_m / Self::per_arm(&bias_correction1, param);
                let v_hat = &new_v / Self::per_arm(&bias_correction2, param);
                let update = &m_hat / (v_hat.sqrt() + self.eps) + &*param * self.wd;
                let mut new_param = &*param - update * Self::per_arm(&lr, param);
                if let Some(&bound) = self.bounds.get(name.as_str()) {
                    new_param = new_param.clamp(-bound, bound);
                }
                let (new_m, new_v, new_param) = match &stepping {
                    Some(stepping) => {
                        let stepping = Self::per_arm(stepping, param);
                        (
                            new_m.where_self(&stepping, m),
                            new_v.where_self(&stepping, v),
                            new_param.where_self(&stepping, param),
                        )
                    }
                    None => (new_m, new_v, new_param),
                };
                m.copy_(&new_m);
                v.copy_(&new_v);
                param.copy_(&new_param);
            }
        });
    }

    /// Keeps only the arms `keep`, in that order, as `StackedNetwork::retain` did with `parameters`,
    /// the network's new tensors: their moments, rates, counts and holds go with them.
    pub fn retain(&mut self, keep: &[usize], parameters: &HashMap<String, Tensor>) {
        let index = retained_index(keep, self.arms(), self.device);
        assert_eq!(parameters.len(), self.parameters.len(), "the network's parameters are not this optimizer's");
        for (name, tensor) in parameters {
            assert!(self.parameters.contains_key(name), "{name} is not one of this optimizer's parameters");
            assert_eq!(tensor.size()[0], keep.len() as i64, "{name} does not hold the arms kept");
        }
        self.parameters = parameters.iter().map(|(name, tensor)| (name.clone(), tensor.shallow_clone())).collect();
        for (m, v) in self.moments.values_mut() {
            *m = m.index_select(0, &index);
            *v = v.index_select(0, &index);
        }
        self.rates = keep.iter().map(|&arm| self.rates[arm]).collect();
        self.counts = keep.iter().map(|&arm| self.counts[arm]).collect();
        self.run_steps = keep.iter().map(|&arm| self.run_steps[arm]).collect();
        let stepping: Vec<bool> = keep.iter().map(|&arm| self.stepping[arm]).collect();
        self.rates_on_device = Self::upload_f64(&self.rates, self.device);
        self.counts_on_device = Self::upload_counts(&self.counts, self.device);
        self.stepping_on_device = Self::upload_stepping(&stepping, self.device);
        self.stepping = stepping;
    }

    /// Writes arm `arm`'s state to `path` in `AdamW::save`'s format, plus its run step as `step`, in
    /// the same file so that a crash can never leave the two counters disagreeing. `AdamW::load` reads
    /// only the keys it knows, so the file still restores an unstacked `AdamW`.
    pub fn save_arm(&self, arm: usize, path: &str) {
        let mut named: Vec<(String, Tensor)> = Vec::with_capacity(self.moments.len() * 2 + 2);
        for (name, (m, v)) in &self.moments {
            named.push((format!("m.{name}"), arm_slice::slice_of(m, arm as i64)));
            named.push((format!("v.{name}"), arm_slice::slice_of(v, arm as i64)));
        }
        named.push(("t".to_string(), Tensor::from_slice(&[self.counts[arm]])));
        named.push(("step".to_string(), Tensor::from_slice(&[self.run_steps[arm]])));
        Tensor::save_multi(&named, path).unwrap_or_else(|error| panic!("failed to save optimizer state to {path}: {error}"));
    }

    /// Reads arm `arm`'s whole state from `path`, an `AdamW::save` file of one unstacked network or a
    /// `save_arm` one: moments, count and run step. A file without a run step (an `AdamW` one, or a
    /// backup written before the two counters existed) holds a count that was also the run step.
    pub fn load_arm(&mut self, arm: usize, path: &str) {
        let loaded = self.restore_from(arm, path);
        self.run_steps[arm] = loaded.get("step").map_or(self.counts[arm], |step| step.int64_value(&[0]));
    }

    /// Rolls arm `arm` back to the state in `path`, a `save_arm` file of an earlier best: moments and
    /// count, so its bias corrections and warmup are that state's again, but not its run step, which
    /// keeps counting the batches it has stepped on.
    pub fn restore_arm(&mut self, arm: usize, path: &str) {
        let _ = self.restore_from(arm, path);
    }

    /// Rolls arm `arm` back to a fresh start, as `new` left it: zero moments and count 0, so that its
    /// warmup ramps again, up to its current rate. Its run step is kept.
    pub fn reset_arm(&mut self, arm: usize) {
        for (name, (m, v)) in &self.moments {
            for (prefix, moment) in [("m", m), ("v", v)] {
                let zeros = arm_slice::slice_of(moment, arm as i64).zeros_like();
                arm_slice::fill_slice(moment, arm as i64, &zeros, &format!("{prefix}.{name}"));
            }
        }
        self.counts[arm] = 0;
        self.counts_on_device = Self::upload_counts(&self.counts, self.device);
    }

    /// Fills arm `arm`'s moments and count from `path`, returning the file's tensors by name.
    fn restore_from(&mut self, arm: usize, path: &str) -> HashMap<String, Tensor> {
        let loaded = arm_slice::read_named(path, self.device);
        for (name, (m, v)) in &self.moments {
            for (prefix, moment) in [("m", m), ("v", v)] {
                let key = format!("{prefix}.{name}");
                let value = loaded.get(&key).unwrap_or_else(|| panic!("optimizer checkpoint {path} is missing {key}"));
                arm_slice::fill_slice(moment, arm as i64, value, &key);
            }
        }
        let t = loaded.get("t").unwrap_or_else(|| panic!("optimizer checkpoint {path} is missing its step count"));
        self.counts[arm] = t.int64_value(&[0]);
        self.counts_on_device = Self::upload_counts(&self.counts, self.device);
        loaded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::adamw::AdamW;
    use tch::nn;

    const ARMS: usize = 3;

    /// Each arm's own weights and the target its loss pulls them to, different per arm so that every
    /// arm takes a different path.
    fn start(arm: usize) -> (Vec<f32>, Vec<f32>) {
        let a = arm as f32;
        (vec![1.0 + a, -2.0 + 0.5 * a, 3.0 - a, 0.2 * a], vec![0.5 - a, 1.0, -1.5 * a, 2.0])
    }

    /// One unstacked `AdamW` per arm, each over its own `w` of shape `[2, 2]`.
    struct Unstacked {
        weights: Vec<Tensor>,
        optimizers: Vec<AdamW>,
    }

    fn unstacked() -> Unstacked {
        let mut weights = Vec::new();
        let mut optimizers = Vec::new();
        for arm in 0..ARMS {
            let vs = nn::VarStore::new(Device::Cpu);
            let w = vs.root().var_copy("w", &Tensor::from_slice(&start(arm).0).view([2, 2]));
            optimizers.push(AdamW::new(&vs, 0.1));
            weights.push(w);
        }
        Unstacked { weights, optimizers }
    }

    /// The same weights stacked, `[ARMS, 2, 2]`, and the stacked targets.
    fn stacked() -> (HashMap<String, Tensor>, Tensor, StackedAdamW) {
        let weights: Vec<Tensor> = (0..ARMS).map(|arm| Tensor::from_slice(&start(arm).0).view([2, 2])).collect();
        let w = Tensor::stack(&weights, 0).set_requires_grad(true);
        let targets: Vec<Tensor> = (0..ARMS).map(|arm| Tensor::from_slice(&start(arm).1).view([2, 2])).collect();
        let parameters: HashMap<String, Tensor> = [("w".to_string(), w)].into_iter().collect();
        let optimizer = StackedAdamW::new(&parameters, 0.1);
        (parameters, Tensor::stack(&targets, 0), optimizer)
    }

    fn step_unstacked(unstacked: &mut Unstacked, arm: usize) {
        let target = Tensor::from_slice(&start(arm).1).view([2, 2]);
        let loss = (&unstacked.weights[arm] - target).square().sum(Kind::Float);
        unstacked.optimizers[arm].zero_grad();
        loss.backward();
        unstacked.optimizers[arm].step();
    }

    /// The summed loss: each arm's slice gets exactly its own loss's gradient.
    fn step_stacked(parameters: &HashMap<String, Tensor>, targets: &Tensor, optimizer: &mut StackedAdamW) {
        let loss = (&parameters["w"] - targets).square().sum(Kind::Float);
        optimizer.zero_grad();
        loss.backward();
        optimizer.step();
    }

    fn assert_arms_agree(parameters: &HashMap<String, Tensor>, unstacked: &Unstacked, what: &str) {
        for arm in 0..ARMS {
            let stacked = parameters["w"].get(arm as i64);
            let own = &unstacked.weights[arm];
            assert!(stacked.allclose(own, 1e-5, 1e-6, false), "{what}, arm {arm}: {stacked:?} against {own:?}");
        }
    }

    #[test]
    fn every_arm_follows_its_own_adamw_with_its_own_rate_warmup_and_bound() {
        let mut unstacked = unstacked();
        let (parameters, targets, mut optimizer) = stacked();
        optimizer.set_warmup_steps(3);
        optimizer.bound("w", 2.5);
        for (arm, own) in unstacked.optimizers.iter_mut().enumerate() {
            own.set_warmup_steps(3);
            own.bound("w", 2.5);
            own.set_learning_rate(0.1 * (arm + 1) as f64);
            optimizer.set_learning_rate(arm, 0.1 * (arm + 1) as f64);
        }
        for step in 0..8 {
            if step == 4 {
                // Arm 1 moves to its next rate; the others keep theirs.
                unstacked.optimizers[1].set_learning_rate(0.01);
                optimizer.set_learning_rate(1, 0.01);
            }
            for arm in 0..ARMS {
                step_unstacked(&mut unstacked, arm);
            }
            step_stacked(&parameters, &targets, &mut optimizer);
            assert_arms_agree(&parameters, &unstacked, &format!("step {step}"));
        }
        for arm in 0..ARMS {
            assert_eq!(optimizer.step_count(arm), unstacked.optimizers[arm].step_count());
        }
    }

    #[test]
    fn an_arm_held_still_keeps_its_weights_moments_and_count_then_resumes_its_own_path() {
        let mut unstacked = unstacked();
        let (parameters, targets, mut optimizer) = stacked();
        optimizer.set_warmup_steps(4);
        for own in unstacked.optimizers.iter_mut() {
            own.set_warmup_steps(4);
        }
        // Arm 2 waits the first three steps out, as an arm ahead of the stream does.
        for step in 0..7 {
            let held = step < 3;
            optimizer.set_stepping(&[true, true, !held]);
            let before = parameters["w"].get(2).copy();
            for arm in 0..ARMS {
                if arm != 2 || !held {
                    step_unstacked(&mut unstacked, arm);
                }
            }
            step_stacked(&parameters, &targets, &mut optimizer);
            if held {
                assert!(parameters["w"].get(2).equal(&before), "step {step}: a held arm does not move");
                assert_eq!(optimizer.step_count(2), 0, "step {step}: nor does its count");
            }
            assert_arms_agree(&parameters, &unstacked, &format!("step {step}"));
        }
        assert_eq!(optimizer.step_count(0), 7);
        assert_eq!(optimizer.step_count(2), 4);
    }

    #[test]
    fn an_arms_state_moves_between_the_two_optimizers_through_their_files() {
        let dir = std::env::temp_dir();
        let pid = std::process::id();
        let mut unstacked = unstacked();
        let (parameters, targets, mut optimizer) = stacked();
        for _ in 0..3 {
            for arm in 0..ARMS {
                step_unstacked(&mut unstacked, arm);
            }
            step_stacked(&parameters, &targets, &mut optimizer);
        }

        // Stacked to unstacked: arm 1's file restores a fresh AdamW, which then steps as the
        // original does.
        let out_path = dir.join(format!("smithy-stacked-adamw-{pid}-out.opt"));
        let out_path = out_path.to_str().unwrap();
        optimizer.save_arm(1, out_path);
        let vs = nn::VarStore::new(Device::Cpu);
        let w = vs.root().var_copy("w", &parameters["w"].get(1).detach());
        let mut restored = AdamW::new(&vs, 0.1);
        restored.load(out_path);
        assert_eq!(restored.step_count(), 3);
        let loss = (&w - Tensor::from_slice(&start(1).1).view([2, 2])).square().sum(Kind::Float);
        restored.zero_grad();
        loss.backward();
        restored.step();
        step_unstacked(&mut unstacked, 1);
        assert!(w.allclose(&unstacked.weights[1], 1e-5, 1e-6, false), "a stacked arm's file restores an AdamW");

        // Unstacked to stacked: arm 0's AdamW file, saved at step 3, restores slot 0 of a stack whose
        // other arms stand at step 0; every arm then steps on its own count.
        let path = dir.join(format!("smithy-stacked-adamw-{pid}-in.opt"));
        let path = path.to_str().unwrap();
        unstacked.optimizers[0].save(path);
        let (fresh, targets, mut fresh_optimizer) = stacked();
        arm_slice::fill_slice(&fresh["w"], 0, &unstacked.weights[0].detach(), "w");
        fresh_optimizer.load_arm(0, path);
        assert_eq!((fresh_optimizer.step_count(0), fresh_optimizer.step_count(1)), (3, 0));
        assert_eq!(fresh_optimizer.run_step(0), 3, "a file without a run step holds a count that was one");
        let mut reference = unstacked;
        let mut beside = self::unstacked();
        step_unstacked(&mut reference, 0);
        step_unstacked(&mut beside, 1);
        step_stacked(&fresh, &targets, &mut fresh_optimizer);
        assert!(fresh["w"].get(0).allclose(&reference.weights[0], 1e-5, 1e-6, false), "arm 0 continues at step 4");
        assert!(fresh["w"].get(1).allclose(&beside.weights[1], 1e-5, 1e-6, false), "arm 1 takes its first step");
        std::fs::remove_file(out_path).ok();
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn the_arms_left_after_one_leaves_step_as_if_it_had_never_been_there() {
        // A three-arm stack and a two-arm one holding arms 0 and 2: four steps together, then arm 1
        // leaves the first, and four more. Rates differ per arm, so a mixed-up slot cannot pass.
        let (mut parameters, mut targets, mut optimizer) = stacked();
        let keep = [0usize, 2];
        let index = Tensor::from_slice(&[0i64, 2]);
        let (all, all_targets, _) = stacked();
        let pair_parameters: HashMap<String, Tensor> = [(
            "w".to_string(),
            all["w"].detach().index_select(0, &index).set_requires_grad(true),
        )]
        .into_iter()
        .collect();
        let pair_targets = all_targets.index_select(0, &index);
        let mut pair = StackedAdamW::new(&pair_parameters, 0.1);
        for arm in 0..ARMS {
            optimizer.set_learning_rate(arm, 0.05 * (arm + 1) as f64);
        }
        for (slot, &arm) in keep.iter().enumerate() {
            pair.set_learning_rate(slot, 0.05 * (arm + 1) as f64);
        }
        for step in 0..8 {
            if step == 4 {
                let retained: HashMap<String, Tensor> = [(
                    "w".to_string(),
                    tch::no_grad(|| parameters["w"].index_select(0, &index)).set_requires_grad(true),
                )]
                .into_iter()
                .collect();
                optimizer.retain(&keep, &retained);
                parameters = retained;
                targets = targets.index_select(0, &index);
                assert_eq!(optimizer.arms(), 2);
                assert_eq!((optimizer.learning_rate(0), optimizer.learning_rate(1)), (0.05 * 1.0, 0.05 * 3.0));
            }
            step_stacked(&parameters, &targets, &mut optimizer);
            step_stacked(&pair_parameters, &pair_targets, &mut pair);
            if step >= 4 {
                assert!(parameters["w"].allclose(&pair_parameters["w"], 1e-6, 1e-7, false), "step {step}");
            }
        }
        assert_eq!((optimizer.step_count(0), optimizer.step_count(1)), (8, 8));
    }

    #[test]
    fn the_run_step_advances_with_the_count_and_holds_with_it() {
        let (parameters, targets, mut optimizer) = stacked();
        for step in 0..5 {
            optimizer.set_stepping(&[true, step >= 2, true]);
            step_stacked(&parameters, &targets, &mut optimizer);
        }
        for arm in 0..ARMS {
            assert_eq!(optimizer.run_step(arm), optimizer.step_count(arm), "arm {arm}");
        }
        assert_eq!((optimizer.run_step(0), optimizer.run_step(1)), (5, 3));
    }

    #[test]
    fn a_rollback_to_a_saved_best_continues_from_it_while_the_run_step_goes_on() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("smithy-stacked-adamw-{}-best.opt", std::process::id()));
        let path = path.to_str().unwrap();
        let mut reference = unstacked();
        let (parameters, targets, mut optimizer) = stacked();
        optimizer.set_warmup_steps(4);
        for own in reference.optimizers.iter_mut() {
            own.set_warmup_steps(4);
        }
        for _ in 0..3 {
            step_unstacked(&mut reference, 1);
            step_stacked(&parameters, &targets, &mut optimizer);
        }
        // Arm 1's best, at step 3: its weights and its optimizer.
        optimizer.save_arm(1, path);
        let best_weights = arm_slice::slice_of(&parameters["w"], 1);
        for _ in 0..3 {
            step_stacked(&parameters, &targets, &mut optimizer);
        }
        let others = [parameters["w"].get(0).copy(), parameters["w"].get(2).copy()];

        arm_slice::fill_slice(&parameters["w"], 1, &best_weights, "w");
        optimizer.restore_arm(1, path);
        assert_eq!((optimizer.step_count(1), optimizer.run_step(1)), (3, 6), "the count goes back, the run step does not");
        assert!(parameters["w"].get(0).equal(&others[0]) && parameters["w"].get(2).equal(&others[1]), "the other arms stay");

        step_unstacked(&mut reference, 1);
        step_stacked(&parameters, &targets, &mut optimizer);
        assert!(
            parameters["w"].get(1).allclose(&reference.weights[1], 1e-5, 1e-6, false),
            "the restored arm steps as if the three steps since its best had never been taken"
        );
        assert_eq!((optimizer.step_count(1), optimizer.run_step(1)), (4, 7));

        // The two counters survive a backup and a resume apart.
        optimizer.save_arm(1, path);
        let (_, _, mut resumed) = stacked();
        resumed.load_arm(0, path);
        assert_eq!((resumed.step_count(0), resumed.run_step(0)), (4, 7));
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn a_rollback_to_the_start_warms_up_again_at_the_current_rate() {
        let (parameters, targets, mut optimizer) = stacked();
        optimizer.set_warmup_steps(4);
        for _ in 0..6 {
            step_stacked(&parameters, &targets, &mut optimizer);
        }
        // Arm 0 back to its starting weights and a fresh optimizer, now at a lower rate.
        arm_slice::fill_slice(&parameters["w"], 0, &Tensor::from_slice(&start(0).0).view([2, 2]), "w");
        optimizer.reset_arm(0);
        optimizer.set_learning_rate(0, 0.03);
        assert_eq!((optimizer.step_count(0), optimizer.run_step(0)), (0, 6));
        step_stacked(&parameters, &targets, &mut optimizer);

        let mut fresh = unstacked();
        fresh.optimizers[0].set_warmup_steps(4);
        fresh.optimizers[0].set_learning_rate(0.03);
        step_unstacked(&mut fresh, 0);
        assert!(
            parameters["w"].get(0).allclose(&fresh.weights[0], 1e-5, 1e-6, false),
            "the first step after the reset is a fresh optimizer's, ramped to a quarter of the new rate"
        );
        assert_eq!((optimizer.step_count(0), optimizer.run_step(0)), (1, 7));
    }

    #[test]
    #[should_panic(expected = "no parameter named missing")]
    fn a_bound_on_an_unknown_parameter_panics() {
        let (_, _, mut optimizer) = stacked();
        optimizer.bound("missing", 1.0);
    }
}
