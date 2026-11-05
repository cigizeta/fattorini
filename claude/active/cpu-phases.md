# A full run's non-training phases without CUDA: design and implementation plan

Status: designed 2026-11-03 with the developer; steps 1 to 4 applied the same day, tests and
clippy passing; the page checked on a machine without CUDA for a run at `train` (Resume disabled).
Left: a run at `play`, `round-robin` or `gate` seen resuming there, then this document to `done/`. Answers the TODO's "A full run's
non-training phases on a machine without CUDA".

## The problem

`forge` refuses every Resume of a full run where CUDA is missing (`lacks_gpu`, after the lease and
the sync down), and the page disables its Resume button (`forgeDisabled(!run.scale)`), whatever
the run's phase. Yet only training needs the GPU: self-play, the round robin and the gate are
played by the arenas on `.nnue` files, and the promotion copies files. Generation 2's self-play
could have started on any machine and moved later.

## The design

- **`forge`:** an existing full run is refused without CUDA only when its state's phase is
  `train`, with the same `NO_GPU` words; any other phase starts. A new full run is still refused:
  generation 1 begins at training.
- **`run_generations`:** at the boundary from Play to Train, without CUDA, the boundary is recorded
  as today (phase `train`, the run's files copied up), then the job ends cleanly: "generation N:
  self-play done; training waits for Resume on a machine with CUDA". The same `lacks_gpu(scale)`, so
  scaled runs are untouched.
- **The page:** `forgeDisabled(!run.scale && run.phase === "train")` on Resume; New run unchanged.
  The note under Runs: "no CUDA GPU on this machine: a full run's training is not available here,
  its other phases are".
- **Moving the run:** nothing new. The lease and the copy up at the job's end already hand it over;
  the training machine's smithy syncs it down at its Resume.
- **Left as is:** `has_gpu` is probed once a process, so a smithy started before its GPU was usable
  says no until restarted, as today.

## Steps

### 1. The decision (`crates/smithy/src/smithy.rs`)

- A function deciding whether a run may go on here: `needs_gpu_now(scale: Option<u64>, phase:
  Phase, gpu: bool) -> bool`, true for a full run in `Phase::Train` without a GPU; `lacks_gpu` kept
  for the new-run check.
- `forge`'s check on an existing run reads the run's state (`run_state(&dirs)`) and uses it; the
  phase read once, after the sync down, as the check is today.
- `run_generations`: after the Play to Train `run.boundary(state)?`, if the decision says so, return
  `Ok` with the words above.

### 2. The page (`crates/smithy/src/page.html`)

- `resumeButton`: the phase in `forgeDisabled`'s condition; the note's words changed.

### 3. Tests

- `needs_gpu_now` over full and scaled, with and without a GPU, every phase.
- The page checked by hand on a machine without CUDA: Resume enabled for a run at `play`,
  `round-robin` or `gate`, disabled at `train`.

### 4. TODO.md

The item marked done, pointing here.

## Commands for the developer

```
cargo test -p smithy
```

```
cargo clippy -p smithy --all-targets
```
