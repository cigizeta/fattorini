# No new epoch before the gate

Today `rotor-train train` derives its epoch from the optimizer's step count, exports the
challenger to `gen-NNNN/epoch-NNNN/` at every epoch boundary and exits; running it again trains
the next epoch with nothing checking that the gate ran. `rotor-evolve gate` records its games in
`epoch-NNNN/gate.db` and prints PASS or FAIL, but writes the verdict nowhere.

## Decisions (with the developer)

- The gate writes its verdict to `epoch-NNNN/verdict.txt`.
- No override flag.
- A verdict file, not rotor-train replaying `gate.db`: that would pull the SPRT and the results
  database into the trainer.

## Changes

1. `src/train/generation.rs` (shared by both binaries):
   - `verdict_path(epoch_dir) -> String`: `{epoch_dir}/verdict.txt`.
   - `enum GateVerdict { Pass, Fail }`.
   - `write_verdict(epoch_dir, verdict)`: `pass` or `fail` and a newline, via `.tmp` and rename.
   - `read_verdict(epoch_dir) -> Option<GateVerdict>`: `None` when the file is missing; panics on
     anything other than the two spellings.
2. `src/evolve/main.rs`, `run_gate`: after `play_gate` returns a winner, writes `pass` when it is
   the challenger and `fail` otherwise. An interrupted gate (`None`) writes nothing; resuming it
   to a verdict writes the file then. The verdict is also written when `play_gate` resolves from
   the recorded games alone, so rerunning a finished gate recreates a missing file.
3. `src/train/main.rs`, `run_train_nnue`: once the arms are loaded and `steps_done` is known, when
   `steps_done > 0` and `steps_done % steps_per_epoch == 0` (the run stands at the boundary after
   epoch `E = steps_done / steps_per_epoch`), reads `read_verdict(epoch_dir(E))`:
   - `Fail`: go on and train epoch `E + 1`, as today.
   - `Pass`: exit with an error: the challenger passed, promote it
     (`rotor-train promote ...`, from `epoch-E/challenger.txt`).
   - `None`: exit with an error: run `rotor-evolve gate <network>` first. This also covers an
     epoch that ended without exporting a challenger (no `epoch-E` directory at all).
   A run resumed inside an epoch is not affected.
4. Docs: `run_train_nnue`'s doc ("A failed gate re-invokes this for another epoch" becomes enforced),
   `run_gate`'s doc, the README's `rotor-train train` / `rotor-evolve gate` lines.

## Tests

- `generation.rs`: verdict round trip for both values; missing file reads `None`; a malformed file
  panics.
- The boundary check as a pure function, `epoch_gate_check(steps_done, steps_per_epoch,
  generation_dir) -> Result<(), String>`, tested for: mid-epoch (Ok without reading), step 0
  (Ok), boundary with `fail` (Ok), with `pass` (Err mentioning promote), with nothing (Err
  mentioning the gate).
