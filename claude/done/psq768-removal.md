# Removing psq768

**Status: agreed 2026-10-20 (the developer); implemented, tested and committed the same day.**
As built: the `FEATURE_SET_PSQ768` constant stays, out of `shape_of`, so a file under tag 10 is
refused with "holds psq768 weights, which this build cannot run" (a test checks it).

## Why

psq768 (psq768.md) drew psq on 2026-10-19 (906 games, S 0.4989), its speed the same within noise,
so by the developer's rule psq keeps its 780 rows, and both the _move and _mcts lines build on 780
(architectures.md). Nothing builds on 768. Its code is a flag, `castling_en_passant`, threaded
through the engine's incremental update, the path every move runs, and a kind in every match of the
trainer. The developer accepts that `lc0-psq768-0001` becomes unloadable (2026-10-20): its result
stays recorded, and git history keeps the code should the MCTS line ever test 768 for itself.

## Changes

Found by searching `psq768` and `castling_en_passant` (2026-10-20, 15 files).

### Engine (`crates/rotor`)

1. **`features.rs`:** the `psq768` module goes.
2. **`nnue.rs`:**
   - `FEATURE_SET_PSQ768` (tag 10) goes; the tag left unused, as tags 0 and 4 were, so an old file
     is refused by name ("an unknown feature set" until then: `feature_set_name` keeps "psq768"
     so the refusal names it).
   - `PsqNetwork::castling_en_passant` and `QuantizedPsqNetwork::castling_en_passant` go, and with
     them `piece_square_rows`' branch, `piece_square_active_rows`' parameter, `ChangedRows::between`'s
     flag (its castling and en passant changes always counted), `validate`'s psq768 refusal,
     `write`'s and `from_bytes`' psq768 branches.
   - The tests that build or load a psq768 network (`sample_psq768_network` and its users) go.
3. **`benches/nnue_bench.rs`:** `--psq768`, `random_psq768_network`, the `castling_en_passant`
   fields and the `psq768` label go.
4. **`pipeline/preprocessing_config.rs`:** the two configs leave its lambda and label test lists.

### Training (`crates/smithy`)

5. **`feature_set.rs`:** `FeatureSet::Psq768` goes.
6. **`batch.rs`:** `DenseBatch::Psq768`, its indices and its tests go.
7. **`nnue_network.rs`, `stacked_network.rs`:** `Network::Psq768` and every arm naming it.
8. **`export.rs`:** `psq768_network_from`, `network_from`'s `castling_en_passant` parameter, the
   `castling_en_passant` fields, and the psq768 agreement test.
9. **`net_config.rs`:** `ArchitectureKind::Psq768`; the two configs leave the experiments test.
10. **`main.rs`:** every match arm and `feature_set_of`'s line, and its test assertion.

### Configs and arena

11. **`configs/lc0-psq768.yaml`, `configs/self-psq768.yaml`:** deleted (`git rm`).
12. **`configs/lc0-psq.yaml`:** its two comments citing lc0-psq768's lambda grid and stage round
    robin stay as history, reworded to say psq768 was retired.
13. **`crates/arena/src/config.rs`:** the stage-round-robin assertion on `lc0-psq768.yaml` goes;
    the stage round robin itself stays, other configs may still ask for one.

### Docs

14. psq768.md to `done/`, its status saying retired; TODO's item 1b says so.

## Tests

Nothing new: the suite shrinks by psq768's own tests; every other test must pass unchanged, psq's
castling and en passant rows now always present.

## Steps for the developer

1. `git rm` the two configs and move psq768.md (commands given with the change).
2. `cargo test -p rotor -p smithy -p arena`.
3. `bin/rotor bench`: the node count must not change, the search untouched.
