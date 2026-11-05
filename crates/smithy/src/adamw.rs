use std::collections::HashMap;

use tch::nn;
use tch::Tensor;

/// A hand-rolled AdamW (Loshchilov & Hutter, "Decoupled Weight Decay Regularization"), reimplementing
/// `tch`'s own built-in `nn::AdamW`/`nn::Optimizer` rather than using it directly. The reason: that
/// built-in optimizer has no way to save or restore its own internal state at all -- checked directly
/// against `torch-sys`'s complete FFI surface for optimizers (every `ato_*` function in
/// `torch-sys/src/lib.rs`: the four optimizer constructors, `add_parameters`, the learning-rate/
/// momentum/weight-decay setters, `zero_grad`, `step`, `free` -- no `state_dict` equivalent anywhere).
/// `training.md`'s "Run shape" needs a checkpoint to include the optimizer's own moment estimates,
/// not just the model's weights, so a cold-started optimizer never resets its adaptive step sizing on
/// every restart (a real cost paid on every one of potentially hundreds of epoch boundaries, not just
/// on an actual crash). Owning the moment buffers as plain tensors, instead of hidden inside `tch`'s
/// opaque C++ optimizer object, is what makes that possible: they serialize via the exact same
/// primitive `VarStore::save`/`load` already use internally (`Tensor::save_multi`/`load_multi`).
///
/// Cross-checked against `tch`'s own built-in `nn::AdamW` for several steps on an identical parameter
/// and loss (`tests::matches_tchs_builtin_adamw_over_several_steps`) -- CLAUDE.md rule 7's "never
/// assumed to agree" discipline, applied here to a hand-derived formula rather than a bitboard/torch
/// pair, since a transcription slip in the update rule below is exactly the kind of bug that
/// "compiles and looks right" without actually matching real AdamW.
pub struct AdamW {
    lr: f64,
    beta1: f64,
    beta2: f64,
    eps: f64,
    wd: f64,
    /// Step count -- named `t` rather than `step`, to avoid colliding with the `step` method.
    t: i64,
    /// Steps over which the rate ramps linearly from `lr / warmup_steps` up to `lr`; `0` for none.
    /// Derived from `t` at every step rather than persisted, so a resume partway through the ramp
    /// continues it.
    warmup_steps: i64,
    parameters: HashMap<String, Tensor>,
    moments: HashMap<String, (Tensor, Tensor)>,
    /// Parameters kept within `±bound` after every step (`bound`). Not persisted: the caller sets
    /// them again on every start, from the network's shape.
    bounds: HashMap<String, f64>,
}

impl AdamW {
    /// `beta1 = 0.9`, `beta2 = 0.999`, `eps = 1e-8`, `wd = 0.01` all match `tch`'s own
    /// `nn::AdamW::default()` (and PyTorch's own real default), so the cross-check test above is
    /// comparing like for like. `lr` is `training.md`'s own tunable ("starting learning rate
    /// `1e-3`"), passed in rather than hardcoded.
    pub fn new(vs: &nn::VarStore, lr: f64) -> AdamW {
        let parameters = vs.variables();
        let moments =
            parameters.iter().map(|(name, tensor)| (name.clone(), (tensor.zeros_like(), tensor.zeros_like()))).collect();
        AdamW {
            lr,
            beta1: 0.9,
            beta2: 0.999,
            eps: 1e-8,
            wd: 0.01,
            t: 0,
            warmup_steps: 0,
            parameters,
            moments,
            bounds: HashMap::new(),
        }
    }

    /// Keeps parameter `name` within `±bound` from now on: clamped at once, so weights loaded from
    /// elsewhere start inside, and again after every step. Call after the weights are loaded. The
    /// moments are left as they are, so a weight held at the bound stays there while its gradient
    /// pushes outward, as nnue-pytorch's clipping does.
    ///
    /// This and the rate and warmup setters below are the NNUE track's, which now trains through
    /// `StackedAdamW`; they stay as the reference its tests compare against.
    #[cfg(test)]
    pub fn bound(&mut self, name: &str, bound: f64) {
        assert!(bound.is_finite() && bound > 0.0, "a bound of {bound} is meaningless");
        let param = self.parameters.get_mut(name).unwrap_or_else(|| panic!("no parameter named {name} to bound"));
        tch::no_grad(|| {
            let _ = param.clamp_(-bound, bound);
        });
        self.bounds.insert(name.to_string(), bound);
    }

    /// Ramps the rate up linearly over the first `steps` steps of this optimizer's life.
    ///
    /// A zero-moment AdamW's first updates are close to `lr * sign(grad)` on every parameter at once,
    /// because `v_hat` is estimated from a handful of gradients: harmless for random weights, a jolt for
    /// trained ones. The ramp absorbs it until `v` has seen about `1 / (1 - beta2)` gradients.
    #[cfg(test)]
    pub fn set_warmup_steps(&mut self, steps: i64) {
        assert!(steps >= 0, "a warmup of {steps} steps is meaningless");
        self.warmup_steps = steps;
    }

    /// Changes the rate from the next step on, keeping the moments: moving to the next rate of a run's
    /// list is an abrupt change of step size, not a fresh start. The warmup factor still applies.
    #[cfg(test)]
    pub fn set_learning_rate(&mut self, lr: f64) {
        assert!(lr.is_finite() && lr > 0.0, "a learning rate of {lr} is meaningless");
        self.lr = lr;
    }

    #[cfg(test)]
    pub fn learning_rate(&self) -> f64 {
        self.lr
    }

    /// The fraction of `lr` step `t` takes, for `t >= 1`.
    fn warmup_factor(&self) -> f64 {
        if self.warmup_steps == 0 {
            return 1.0;
        }
        (self.t as f64 / self.warmup_steps as f64).min(1.0)
    }

    /// Zeroes every tracked parameter's own gradient. Call before `loss.backward()` each step (the
    /// same zero-then-backward-then-step order `tch`'s own `Optimizer::backward_step` uses
    /// internally), so a previous step's gradient never accumulates into this one.
    pub fn zero_grad(&mut self) {
        for param in self.parameters.values_mut() {
            param.zero_grad();
        }
    }

    /// The total number of `step()` calls this optimizer has made, across every epoch it has ever
    /// trained (restored by `load`, not reset by it): a resumed run derives where it stands from it,
    /// rather than persisting a separate counter of its own.
    pub fn step_count(&self) -> i64 {
        self.t
    }

    /// One AdamW update, in place, over every tracked parameter's current gradient -- call once per
    /// batch, after `loss.backward()` has populated gradients.
    pub fn step(&mut self) {
        self.t += 1;
        // `bias_correction` counteracts `m`/`v`'s own zero initialization: without it, early steps
        // would systematically underestimate both moments (nothing to average yet), most visibly at
        // `t == 1` where `m`/`v` would otherwise be `(1 - beta) * grad` instead of the true `grad`
        // itself -- exactly what `tests::step_matches_the_closed_form_update_at_t_equals_1` checks.
        let bias_correction1 = 1.0 - self.beta1.powi(self.t as i32);
        let bias_correction2 = 1.0 - self.beta2.powi(self.t as i32);
        let lr = self.lr * self.warmup_factor();

        tch::no_grad(|| {
            for (name, param) in self.parameters.iter_mut() {
                let grad = param.grad();
                assert!(grad.defined(), "AdamW::step found an undefined gradient for {name} -- was backward() run first?");

                let (m, v) = self.moments.get_mut(name.as_str()).expect("moment buffers missing for a tracked parameter");
                let new_m = &*m * self.beta1 + &grad * (1.0 - self.beta1);
                let new_v = &*v * self.beta2 + &grad * &grad * (1.0 - self.beta2);
                m.copy_(&new_m);
                v.copy_(&new_v);

                let m_hat = &*m / bias_correction1;
                let v_hat = &*v / bias_correction2;
                // Decoupled weight decay (the "W" in AdamW): applied straight to `param`, not
                // folded into `grad` before the moment updates above the way plain L2-regularized
                // Adam would -- exactly what distinguishes the two per Loshchilov & Hutter.
                let update = &m_hat / (&v_hat.sqrt() + self.eps) + &*param * self.wd;
                let new_param = &*param - &update * lr;
                param.copy_(&new_param);
                if let Some(&bound) = self.bounds.get(name.as_str()) {
                    let _ = param.clamp_(-bound, bound);
                }
            }
        });
    }

    /// Persists this optimizer's own state (every parameter's `m`/`v` moment buffers, plus the step
    /// count) to `path` -- deliberately separate from `VarStore::save` (which persists the model's
    /// weights, not this). Named `m.<parameter name>`/`v.<parameter name>` so `load` below can
    /// re-attach each buffer to the right parameter regardless of `HashMap` iteration order.
    pub fn save(&self, path: &str) {
        let mut named_tensors: Vec<(String, Tensor)> = Vec::with_capacity(self.moments.len() * 2 + 1);
        for (name, (m, v)) in &self.moments {
            named_tensors.push((format!("m.{name}"), m.shallow_clone()));
            named_tensors.push((format!("v.{name}"), v.shallow_clone()));
        }
        named_tensors.push(("t".to_string(), Tensor::from_slice(&[self.t])));
        Tensor::save_multi(&named_tensors, path)
            .unwrap_or_else(|error| panic!("failed to save optimizer state to {path}: {error}"));
    }

    /// The read-side counterpart to `save`: restores every `m`/`v` buffer and the step count from
    /// `path`, in place. Panics loudly (CLAUDE.md rule 4) if `path` was saved from a differently
    /// shaped network -- a mismatched checkpoint is a real bug, never something to silently ignore.
    pub fn load(&mut self, path: &str) {
        let loaded: HashMap<String, Tensor> = Tensor::load_multi(path)
            .unwrap_or_else(|error| panic!("failed to load optimizer state from {path}: {error}"))
            .into_iter()
            .collect();

        for (name, (m, v)) in self.moments.iter_mut() {
            let loaded_m = loaded.get(&format!("m.{name}")).unwrap_or_else(|| panic!("optimizer checkpoint {path} is missing m.{name}"));
            let loaded_v = loaded.get(&format!("v.{name}")).unwrap_or_else(|| panic!("optimizer checkpoint {path} is missing v.{name}"));
            m.copy_(loaded_m);
            v.copy_(loaded_v);
        }
        let t_tensor = loaded.get("t").unwrap_or_else(|| panic!("optimizer checkpoint {path} is missing its step count"));
        self.t = t_tensor.int64_value(&[0]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tch::nn::OptimizerConfig;
    use tch::{Device, Kind};

    #[test]
    fn step_matches_the_closed_form_update_at_t_equals_1() {
        // At `t == 1`, bias correction exactly cancels `m`/`v`'s own `(1 - beta)` scaling
        // (`m_hat == grad`, `v_hat == grad^2`), so the first update has a clean closed form --
        // independent of `step`'s own implementation, this is a real, separately-derivable check
        // that bias correction is wired in at all (without it, the first step would be
        // systematically `(1 - beta)`-scaled too small).
        let vs = nn::VarStore::new(Device::Cpu);
        let initial = Tensor::from_slice(&[1.0f32, -2.0, 3.0]);
        let w = vs.root().var_copy("w", &initial);
        let mut optimizer = AdamW::new(&vs, 0.1);

        // A linear loss w.r.t `w`, weighted by `coefficients`, so `w.grad()` after `backward()` is
        // exactly `coefficients` -- an arbitrary, independently-known gradient, not one that
        // depends on `step`'s own arithmetic to derive.
        let coefficients = Tensor::from_slice(&[0.5f32, 1.0, -1.5]);
        let loss = (&w * &coefficients).sum(Kind::Float);
        optimizer.zero_grad();
        loss.backward();
        optimizer.step();

        let (eps, wd, lr) = (1e-8f64, 0.01f64, 0.1f64);
        let initial_values = [1.0f64, -2.0, 3.0];
        let coefficient_values = [0.5f64, 1.0, -1.5];
        let expected: Vec<f32> = initial_values
            .iter()
            .zip(coefficient_values.iter())
            .map(|(&w0, &grad)| {
                let m_hat = grad; // (1 - beta1) * grad / (1 - beta1)
                let v_hat = grad * grad; // (1 - beta2) * grad^2 / (1 - beta2)
                let update = m_hat / (v_hat.sqrt() + eps) + wd * w0;
                (w0 - lr * update) as f32
            })
            .collect();

        let expected_tensor = Tensor::from_slice(&expected);
        assert!(w.allclose(&expected_tensor, 1e-5, 1e-5, false));
    }

    #[test]
    fn zero_grad_actually_clears_the_gradient() {
        let vs = nn::VarStore::new(Device::Cpu);
        let w = vs.root().var_copy("w", &Tensor::from_slice(&[1.0f32]));
        let mut optimizer = AdamW::new(&vs, 0.1);

        let loss = (&w * &w).sum(Kind::Float);
        optimizer.zero_grad();
        loss.backward();
        assert!(w.grad().defined());

        optimizer.zero_grad();
        // `zero_grad` zeroes the existing gradient tensor rather than undefining it -- confirmed
        // directly against `Tensor::zero_grad`'s own implementation, not assumed -- so the check
        // here is "all zero," not "undefined."
        let zero = w.grad().zeros_like();
        assert!(w.grad().allclose(&zero, 1e-8, 1e-8, false));
    }

    #[test]
    fn matches_tchs_builtin_adamw_over_several_steps() {
        // The real cross-check (CLAUDE.md rule 7's discipline, applied to this hand-derived
        // formula): two separate `VarStore`s, each with its own single-parameter variable seeded
        // to the identical starting value, one driven by `tch`'s own trusted built-in `nn::AdamW`,
        // the other by this module's `AdamW` -- run the same quadratic loss, the same number of
        // steps, and confirm they track each other at every single step, not just at the end.
        const STEPS: usize = 5;
        const TARGET: f64 = 2.0;
        const LR: f64 = 0.1;

        let builtin_vs = nn::VarStore::new(Device::Cpu);
        let builtin_w = builtin_vs.root().var_copy("w", &Tensor::from_slice(&[5.0f32]));
        let mut builtin_optimizer = nn::AdamW::default().build(&builtin_vs, LR).expect("failed to build the built-in AdamW");

        let custom_vs = nn::VarStore::new(Device::Cpu);
        let custom_w = custom_vs.root().var_copy("w", &Tensor::from_slice(&[5.0f32]));
        let mut custom_optimizer = AdamW::new(&custom_vs, LR);

        for step in 0..STEPS {
            let builtin_diff = &builtin_w - TARGET;
            let builtin_loss = (&builtin_diff * &builtin_diff).sum(Kind::Float);
            builtin_optimizer.backward_step(&builtin_loss);

            let custom_diff = &custom_w - TARGET;
            let custom_loss = (&custom_diff * &custom_diff).sum(Kind::Float);
            custom_optimizer.zero_grad();
            custom_loss.backward();
            custom_optimizer.step();

            assert!(
                builtin_w.allclose(&custom_w, 1e-4, 1e-4, false),
                "step {step}: builtin = {:?}, custom = {:?}",
                f64::try_from(&builtin_w),
                f64::try_from(&custom_w)
            );
        }
    }

    #[test]
    fn save_and_load_round_trips_optimizer_state() {
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("smithy-adamw-test-{pid}.optstate"));

        let vs = nn::VarStore::new(Device::Cpu);
        let w = vs.root().var_copy("w", &Tensor::from_slice(&[1.0f32, 2.0]));
        let mut optimizer = AdamW::new(&vs, 0.1);
        for _ in 0..3 {
            let loss = (&w * &w).sum(Kind::Float);
            optimizer.zero_grad();
            loss.backward();
            optimizer.step();
        }
        optimizer.save(path.to_str().unwrap());

        // A real resume also reloads the model's own weights separately, via `VarStore::load` --
        // so `w2` must start from `w`'s current (post-3-step) value, not its original one, or
        // this wouldn't actually simulate a resume at all.
        let current_w: Vec<f32> = Vec::try_from(&w).expect("failed to read back w's current value");
        let vs2 = nn::VarStore::new(Device::Cpu);
        let w2 = vs2.root().var_copy("w", &Tensor::from_slice(&current_w));
        let mut restored_optimizer = AdamW::new(&vs2, 0.1);
        restored_optimizer.load(path.to_str().unwrap());
        assert_eq!(restored_optimizer.t, 3);

        // Continuing training from the restored optimizer must reproduce the exact same next
        // update the original optimizer would have made, proving the restored `m`/`v` buffers
        // are the real ones, not freshly zeroed placeholders that happen not to panic.
        let loss = (&w * &w).sum(Kind::Float);
        optimizer.zero_grad();
        loss.backward();
        optimizer.step();

        let loss2 = (&w2 * &w2).sum(Kind::Float);
        restored_optimizer.zero_grad();
        loss2.backward();
        restored_optimizer.step();

        assert!(w.allclose(&w2, 1e-6, 1e-6, false));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn warmup_ramps_the_rate_linearly_then_holds_it() {
        let vs = nn::VarStore::new(Device::Cpu);
        let _ = vs.root().var_copy("w", &Tensor::from_slice(&[1.0f32]));
        let mut optimizer = AdamW::new(&vs, 0.1);
        assert_eq!(optimizer.warmup_factor(), 1.0, "no warmup unless one is set");

        optimizer.set_warmup_steps(4);
        let factors: Vec<f64> = [1, 2, 4, 5, 100]
            .iter()
            .map(|&t| {
                optimizer.t = t;
                optimizer.warmup_factor()
            })
            .collect();
        assert_eq!(factors, vec![0.25, 0.5, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn a_first_step_under_warmup_moves_the_weights_by_that_fraction_of_the_full_step() {
        // Same parameter, same gradient, one optimizer ramping over 4 steps and one not: the whole
        // update, weight decay included, is scaled by the rate, so the first step's displacement is
        // exactly a quarter of the unramped one.
        let displacement = |warmup_steps: i64| {
            let vs = nn::VarStore::new(Device::Cpu);
            let w = vs.root().var_copy("w", &Tensor::from_slice(&[1.0f32, -2.0, 3.0]));
            let mut optimizer = AdamW::new(&vs, 0.1);
            optimizer.set_warmup_steps(warmup_steps);
            let loss = (&w * &Tensor::from_slice(&[0.5f32, 1.0, -1.5])).sum(Kind::Float);
            optimizer.zero_grad();
            loss.backward();
            optimizer.step();
            &w - &Tensor::from_slice(&[1.0f32, -2.0, 3.0])
        };

        let full = displacement(0);
        let ramped = displacement(4);
        assert!(ramped.allclose(&(&full * 0.25), 1e-6, 1e-6, false));
    }

    #[test]
    fn a_new_rate_scales_the_next_step_and_keeps_the_moments() {
        // Two optimizers take one identical step at 0.1; then one moves to 0.01. The second step's
        // displacement must be exactly a tenth of the other's: same moments, only the rate changed.
        fn take_step(w: &Tensor, optimizer: &mut AdamW) -> Tensor {
            let before = w.detach().copy();
            let loss = (w * &Tensor::from_slice(&[0.5f32, 1.0, -1.5])).sum(Kind::Float);
            optimizer.zero_grad();
            loss.backward();
            optimizer.step();
            w - &before
        }
        let second_displacement = |second_rate: f64| {
            let vs = nn::VarStore::new(Device::Cpu);
            let w = vs.root().var_copy("w", &Tensor::from_slice(&[1.0f32, -2.0, 3.0]));
            let mut optimizer = AdamW::new(&vs, 0.1);
            let _ = take_step(&w, &mut optimizer);
            optimizer.set_learning_rate(second_rate);
            assert_eq!(optimizer.learning_rate(), second_rate);
            take_step(&w, &mut optimizer)
        };

        let full = second_displacement(0.1);
        let reduced = second_displacement(0.01);
        assert!(reduced.allclose(&(&full * 0.1), 1e-6, 1e-6, false));
    }

    #[test]
    fn a_bound_clamps_at_once_and_after_every_step_and_touches_nothing_else() {
        let vs = nn::VarStore::new(Device::Cpu);
        let bounded = vs.root().var_copy("bounded", &Tensor::from_slice(&[3.0f32, -0.5, -4.0]));
        let free = vs.root().var_copy("free", &Tensor::from_slice(&[3.0f32, -0.5, -4.0]));
        let mut optimizer = AdamW::new(&vs, 10.0);

        optimizer.bound("bounded", 1.0);
        assert_eq!(Vec::<f32>::try_from(&bounded).unwrap(), vec![1.0, -0.5, -1.0], "clamped when the bound is set");

        // A rate of 10 moves every weight by about 10, far past the bound.
        for _ in 0..3 {
            let loss = (&bounded * &Tensor::from_slice(&[-1.0f32, 1.0, -1.0])).sum(Kind::Float)
                + (&free * &Tensor::from_slice(&[-1.0f32, 1.0, -1.0])).sum(Kind::Float);
            optimizer.zero_grad();
            loss.backward();
            optimizer.step();
            assert_eq!(Vec::<f32>::try_from(&bounded).unwrap(), vec![1.0, -1.0, 1.0], "held on the bound");
        }
        let free_values = Vec::<f32>::try_from(&free).unwrap();
        assert!(free_values.iter().all(|value| value.abs() > 10.0), "an unbounded parameter moves freely: {free_values:?}");
    }

    #[test]
    #[should_panic(expected = "no parameter named missing")]
    fn a_bound_on_an_unknown_parameter_panics() {
        let vs = nn::VarStore::new(Device::Cpu);
        let _ = vs.root().var_copy("w", &Tensor::from_slice(&[1.0f32]));
        AdamW::new(&vs, 0.1).bound("missing", 1.0);
    }
}
