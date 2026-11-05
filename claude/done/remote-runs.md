# A run on Drive alone, viewed, tuned and installed: design and implementation plan

Status: done 2026-11-03: designed with the developer, applied, tests and clippy passing, and a run
started on the training machine viewed on another one (generation 1 shown, no error). The
tests changed while applying: the remote root is set once a process (`remote::set_root`) and
shared by every test, so no test stands a scratch remote in; the choice is factored into
`frozen_config(roots)` and `home_of(local, remote)`, tested on scratch directories, and
`tune_args`' own test dropped. Install needed nothing more: it resolves the network through
`players::RotorPlayer::resolve`, and copies its records up only when they are local.

## The problem

The page lists every run found here or on the remote (`catalog`), so a run started on another
machine shows here too. But three paths look for the run on this machine alone:

- **The View** (`generation_view`): refused with "<run> is no forge run" (`runs_of`, local only),
  though its other reads mostly fall back to the remote already (`readable_dirs`).
- **The Tune** of a winner (`tune_args`): refused with "<run> is no forge run on this machine", and
  its winners read from the local `generations.tsv` (`run_winners`).
- **The Install** (`installable`): the winner read from the local run directory
  (`players::run_winner`).

And beneath them, `load_run_config` reads the run's frozen config here, else falls back to
`configs/`: for a run on the remote alone, it skips the run's own frozen copy there. In the catalog
too, a scaled run started elsewhere shows here as a full one (`scale:` is only in its frozen
config), and its lambdas, grid and schedule are today's config's, not the run's.

## The design

- **`load_run_config`:** the frozen config here, else the frozen config on the remote, else
  `configs/` (kept for runs moved there by hand).
- **One helper for a run's home**, `run_home(run) -> Dirs`: this machine's run directory while it
  has one (`state.txt` there), else the remote's. Used where a run's whole-run files are read:
  `generations.tsv` (`run_winners`), `state.txt` and the winner (`players::run_winner`).
  `readable_dirs` stays as it is, per generation.
- **`generation_view`:** a run known here or on the remote, the same union as `catalog` (one
  function both call); the current state read from `run_home`. A run not running here shows its
  files only, as today (`WorkView::Idle`); one live on another machine shows the remote's last copy
  (training copied up every 30 minutes and at each boundary). No note on the page (the developer,
  2026-11-03).
- **`tune_args`:** the same union instead of `runs_of`; winners from `run_home`. The tune reads the
  network from the remote already.
- **`installable`:** the winner from `run_home`. The install itself copies the network from the
  remote already (to check while applying; adapt if it reads the local run directory).

## Steps

### 1. The reads (`crates/smithy/src/smithy.rs`)

- `load_run_config`'s fallback through the remote's frozen copy.
- `run_home`, and a `known_runs(config) -> Vec<u32>` (here and on the remote, sorted, deduplicated),
  `catalog` switched to it.
- `generation_view`, `tune_args` and `installable` on `known_runs` and `run_home`.

### 2. Tests

- `load_run_config` reads the remote's frozen copy when the run has none here, and this machine's
  when both exist (a scratch remote, as existing tests stand one in; to find while applying).
- `run_home`: local when the run's `state.txt` is here, the remote otherwise.
- `tune_args` on a winner whose run is on the scratch remote alone returns its arguments, its
  scale the frozen config's.

### 3. TODO.md

An entry pointing here, done once checked: a run started on the training machine viewed here.

## Commands for the developer

```
cargo test -p smithy
```

```
cargo clippy -p smithy --all-targets
```
