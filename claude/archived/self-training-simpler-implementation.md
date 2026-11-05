# Simplified self-training: implementation plan

**Archived 2026-11-04 with self-training-simpler-attempt.md: `gen-server` became the forge runs.**

**Status: written 2026-10-21, implementing at once (the developer: "write the implementation,
implement, then a smoke test, then the real test").** The design is self-training-simpler-attempt.md,
"Decisions and remarks"; this plan does not reopen it.

## Shape

One new command, `smithy gen-server <config>`, in a new module `crates/smithy/src/gen_server.rs`;
every game played by the existing `arena loop-client`, unchanged. Nothing old changes: the server
calls existing code (forge's `train_in_lockstep` and its parts, the LAN transport and protocol) and
copies the one piece it cannot call, the SPRT (`arena/src/sprt.rs`, into `gen_sprt.rs`).

## The config

`configs/gen-<name>.yaml`, its own schema:

```yaml
network: self-psq-direct     # the forge config: architecture and training (rates, patience, rounds)
grid_center: 0.95            # generation 1's centre; later ones centre on the previous winner
corpus_positions: 100663296  # 96 x 1024 x 1024 training positions an attempt adds
test_positions: 4194304      # 4 x 1024 x 1024, each teacher's frozen test set
round_robin_pairs: 400
gate_elo_d: 35
max_failed_gates: 3          # the second patience; empirical
```

The grid: 9 lambdas, step 0.01, centred on the centre, clamped to [0.01, 0.99] by sliding.

## The run directory

The data root's `gen/<name>/`, local to the server:

- `state.txt`: the generation (1 is taught by PeSTO), the attempt, the phase, the grid's centre,
  the failed gates so far.
- `gen-NNNN/teacher.nnue`, `teacher.vs`: absent for generation 1 (PeSTO).
- `gen-NNNN/test/*.pgn.zst` and `gen-NNNN/corpus/*.pgn.zst`: one file per self-play job, with its
  `.meta`, so a stop loses only the jobs in flight.
- `gen-NNNN/attempt-K/lambda-L/`: forge's own arm directories (history, checkpoints, candidate).
- `gen-NNNN/attempt-K/round-robin.tsv`, `gate.tsv`: every pair result as it arrives, the resume.

## A generation's phases

1. **Play:** self-play jobs with the teacher (its hash, or `hand-crafted`), the test set first
   until `test_positions`, then the corpus until `attempt x corpus_positions`.
2. **Train:** `train_in_lockstep` over every corpus file, the 9 lambdas stacked, the direct blend,
   forge's patience and rollback on the frozen test set, warm-started from the teacher's `.vs`
   (random weights for generation 1). Each lambda exports its candidate as forge does.
3. **Round robin:** pair jobs, candidate against candidate, 400 pairs a pairing, most points wins.
4. **Gate:** pair jobs, the winner against the teacher (PeSTO for generation 1), the copied SPRT
   fed two games a pair; once it decides, late results are ignored.
5. **Decide:** a pass copies the winner to `gen-(N+1)/teacher.*`, centres the next grid on its
   lambda, and resets the failed gates; a fail counts one, and below `max_failed_gates` starts the
   next attempt with the same teacher, test set and grid, the corpus growing by
   `corpus_positions`; at it, the lineage stops.

## Reuse, checked in the code (2026-10-21)

- `train_in_lockstep` (`main.rs`) is generic: run directories, stream, test readers and starting
  weights all come through its arguments, so the server calls it with its own `lambda_dir`, a
  `PgnTrainStream` over the corpus files, an in-memory test reader, and the teacher's `.vs`.
  `open_arm` freezes `run_config.txt` with the row counts, which is why each attempt has its own
  directories.
- Every game's job kind exists in `loop_protocol` (`SelfPlay`, `Gate` = challenger against
  teacher), so the client needs nothing new.

## The log

A header line per generation and attempt, one line per phase start and end with its numbers,
forge's own round block during training, and running standings: the round robin's table after each
pairing completes, the SPRT's score and LLR after each job.

## Steps for the developer

1. `cargo test -p smithy gen_`
2. Smoke test on the Lichess machine: `configs/gen-psq-smoke.yaml`, everything tiny.
3. The real run on the training machine: `configs/gen-psq.yaml`, clients on every machine.
