# Removing labels: archives (step 4, scope B)

Implemented, tested and committed 2026-10-17; `data/games/self/` deleted from Drive the same day. Scope B of rotor-alone-and-lc0.md's step 4, after scope A
removed the builders (2026-10-17).

## Decisions (the developer, 2026-10-17)

- **Every NNUE trains on `self` or `lc0`.** amanda, betty, dorothy, hilda and greta move to `self`.
  greta and zelda are new architectures, not comparisons tied to amanda-0002's dataset.
- **The transformer's streaming is postponed.** It needs 31 buffers (one per piece count, 2 to 32; 30 with K vs K skipped, transformer-streaming.md),
  and one day the policy too. Until then **zelda stays on `archives`**, and so does what she reads:
  `regroup`, the length buckets, `dataset_cache`'s `Manifest` and the built dataset of amanda-0002.
  Deleting them now would leave the transformer's whole training stack unreachable, which the
  workspace's `dead_code = deny` refuses, while its network, batch and vocabulary are what the
  streaming rewrite reuses.
- **`check`, `calibrate` and `correlate` read the frozen test set**, all three. `calibrate` swept a
  built dataset's training half; a streamed generation has no fixed one.
- **`smithy buckets` is ported to `data/self`**, not deleted.
- **The Lc0 ranking check compares Lc0's labels with new self-play only**, caroline-0001's in
  `data/self`, streamed: never generation 3's datasets, which mixed in CCRL and TCEC. So nothing of
  the archive path waits for it.

## Done

- **Configs:** amanda, betty, dorothy, greta and hilda say `labels: self`; zelda keeps `archives`,
  its comments without `prepare`. `the_committed_configs_name_their_labels` follows.
- **`smithy prepare` deleted** (`prepare.rs`). `fetch_teacher` moved to `teacher.rs`; forge fetches
  the teacher itself, on both tracks.
- **Forge's dataset branch deleted:** `dataset_halves`, `DatasetHalves`, `NnueStream`,
  `is_streamed`, `train_stream::ShuffledStream`, `shuffle_buffer.rs`, and the reader's
  `read_records_cyclic` and `row_count_of`, whose only user it was. An NNUE under `labels: archives`
  is refused by `streamed_inputs`. `train_lambdas_in_lockstep` takes its `SelfPlayInputs` from
  `run_train_generation`, at the pending lambdas, so its end-to-end test streams local segments and
  a local test set, in place of the remote's.
- **Inspectors:** `frozen_test_half` in place of `dataset_halves`.
- **`dataset_cache.rs`:** `built_manifest` and `missing_label_sets` deleted; `Manifest`,
  `TEST_POSITIONS_CAP` and `splitmix64` stay. `dataset_paths`'s flat-dataset paths are `cfg(test)`,
  spelling `regroup::Layout`'s for its test.
- **Legacy self-play tree:** `legacy_self_play_dir`, `stream-bench`'s legacy branch, `read_number`,
  `remote::is_segment_file`'s `.count` and `.games`, and `arena self`'s publish of
  `data/games/self` deleted. `buckets` and `stream-bench` read `pgn_train_stream::training_files`.
- **Messages and docs:** every "smithy prepare" in messages and doc comments; USAGE; DEV.md's
  `smithy prepare` step.

## Left

- **When the transformer streams:** `Labels::Archives`, `regroup.rs`, `dataset_cache.rs`,
  `dataset_paths`'s datasets and `lengths`, `TransformerLengths`, `BucketStream` if the streaming
  does not reuse it; zelda to `self` or `lc0`.
- **Data, by hand:** `data/datasets/` on the training machine except
  amanda-0002's, which zelda reads until she streams.

