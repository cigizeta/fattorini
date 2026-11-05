# `rotor-train promote` takes the challenger that passed its gate

Today `rotor-train promote <network> <step> --lr <rate>` promotes any checkpoint the developer
names. The developer wants it to promote the challenger that passed the gate, with nothing to
specify. Builds on gate-before-epoch.md (the gate writes `epoch-NNNN/verdict.txt`).

## New command

`rotor-train promote <network> [--offline]`.

1. The current generation's latest epoch (`generation::latest_epoch`). None: exit, nothing has been
   exported yet.
2. Its verdict (`generation::read_verdict`) must be `pass`:
   - `fail`: exit, the challenger failed its gate; train another epoch.
   - missing: exit, run `rotor-evolve gate` first.
3. The learning rate and step come from that epoch's `challenger.txt` (`learning_rate`, `step`
   lines, as `export_challenger` writes them). A missing or malformed file: exit.
4. From there, exactly today's `run_promote`: the checkpoint from `lr-<rate>/checkpoints`, its test
   loss from `history.tsv`, the staging generation, `initial.vs`, `initial.nnue`, `initial.txt`,
   the commit, then `publish` unless `--offline`.

Only the latest epoch: every earlier epoch's gate failed, or training would have refused to go past
it, so the latest is the only one that can hold a pass.

The `<step> --lr <rate>` form goes. No override, the same choice as for the gate check.

## Code

- `generation.rs`: `read_challenger_record(epoch_dir) -> Result<(f64, u64), String>`, the rate and
  the step. `promotable_challenger(generation_dir) -> Result<(u32, f64, u64), String>`, steps 1-3 as
  a pure function over the directory, so it is testable without `tch`.
- `main.rs`: the `promote` arm parses `<network> [--offline]` only; `run_promote` takes no step or
  rate and calls `promotable_challenger`, printing which epoch, rate and step it promotes. USAGE
  updated. The gate's closing hint (`rotor-evolve`) becomes `rotor-train promote <network>`.
- `export_challenger`'s printed hint, if it mentions the old form.
- README's `rotor-train promote` line.

## Tests (generation.rs)

- `read_challenger_record`: a record as `export_challenger` writes it; missing file; missing line.
- `promotable_challenger`: no epoch; latest epoch without verdict; with `fail`; with `pass`
  returns its epoch, rate and step; an earlier `pass` does not count when a later epoch exists.
