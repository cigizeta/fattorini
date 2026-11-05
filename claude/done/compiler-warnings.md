# Compiler warnings

Implementation plan. Branch: `alessio`. `cargo test --release --no-run` and the same with
`--features training` report 27 warnings. Settled in discussion, one decision per warning:

1. `eval.rs`, 18 dead-code warnings: the terms disabled when the bootstrap became PeSTO's material
   and tables only (pawn structure, mobility, king safety, bishop pair). Delete them and their tests.
   The big five has its own of each in `big_five.rs`; `eval_terms.rs`, which it uses, stays.
2. `train_stream.rs`, `rows_per_epoch`: kept for an epoch-driven gate that does not exist. Delete it
   from the trait and both implementations.
3. `checkpoint_store.rs`, `RESUME_GENERATIONS_KEPT` and `ResumeFiles`' `optimizer` and
   `stream_checkpoint`: an older checkpoint scheme. Delete them, and whatever else of it the plain
   build shows dead.
4. `results_db.rs`, `GameRow::termination`: never read. Drop the field and its query column.
5. `config.rs`, `TournamentConfig::win_threshold`: never called, while `pairing.rs` computes the same
   formula inline. One function, called by `pairing.rs`; the method goes.
6. `generation.rs`, `SEARCH_DEPTH`: used by `rotor-arena`, warned in `rotor-train`, which compiles the
   same shared file. `#[allow(dead_code)]` with the reason.
7. `sample.rs:682`, `return fail(...)`: `fail` diverges, so the `return` is unreachable. A plain
   `fail(...);`.

## As implemented

- `eval.rs` keeps PeSTO only. Its tests of `eval_terms.rs`' definitions (mobility squares, the king
  zone, pure opposite bishops) moved there. `cpw.rs` and `tuned.rs`, the dead terms' tables, are
  public modules, so never warned, but nothing else reads them: their `pub mod` lines go, and the
  files are for the developer to `git rm`.
- `checkpoint_store.rs`: the whole `resume/` generation scheme was dead, called only from its own
  tests (which is why the test build flagged two fields only). Only the `checkpoints/` half stays.
- `win_threshold(best_of)` is a free function in `config.rs`, which `pairing.rs` calls.

- `nnue_network.rs`' dense-batch type imports, used only by its tests, moved into its test module.
- **Out of scope, found by `cargo check --all-targets`:** 25 warnings in `rotor-arena` and 41 in
  `rotor-train` that the test builds hide, none from this plan. They are the shared `pipeline/`
  files each binary uses part of, and the remains of older pipelines (the length-bucketed caches,
  the self-play featurizing, the `feature_set.state` marker, `EarlyStopping::replay`,
  `load_preprocessing`). The transformer part waits for its redesign, after the NNUEs are a strong
  baseline (`TODO.md`); the rest, if worth it, is a separate cleanup.

## Verification

Both builds with no warnings, and every test passing:

```
cargo test --release
```

```
cargo test --release --features training
```

and the plain, non-test builds, which see dead code the tests hide (`bin/rotor`,
`bin/rotor-train`, and `bin/rotor-arena` build their binaries when run).
