# Multi-lambda datasets: one positions file, one label file per lambda

## Status

Done: implemented, and `cargo test --features training --bin rotor-train` passes (306 tests). `prepare` builds the positions and every
lambda of `label.lambda` in one pass, or adds only the missing label sets to built positions.
`train`, `check`, `calibrate` and `correlate` read the positions with one lambda's labels.

Until one training run per lambda exists, `train` refuses a network whose `label.lambda` names more
than one value (`mod.rs`'s `training_lambda`): amanda (`[0.7]`) trains, betty's grid waits for that
step. The deprecated curated path keeps its single label set at a private `CURATED_LAMBDA = 0.7`. First step of the lambda grid
(`TODO.md`, the dataset-cache item): `rotor-train prepare` must build a label set for every lambda
of the network's `label.lambda` before training and the ladder can use them.

## Today

`prepare` builds one dataset per self-play source, in one pass (`dataset_cache.rs`):

```
data/datasets/<source>/train.cache      position + one f32 label per record (cache.rs write_record)
data/datasets/<source>/test.cache
data/datasets/<source>/manifest.txt     positions, rows, segments; written last, the completion marker
```

Labelled at the fixed `preprocessing_config::LAMBDA = 0.7`. A built dataset is final.

The dataset belongs to the **source**, not to a network: at generation 1 amanda (`[0.7]`) and betty
(`[0.65, 0.7, 0.75, 0.8, 0.85]`) both train on `data/datasets/hand-crafted/`.

## Decided

**One file for the positions, one file for each lambda**, per split:

```
data/datasets/<source>/manifest.txt                  as today: positions, rows, segments
data/datasets/<source>/train.positions               cache.rs write_position, no label
data/datasets/<source>/test.positions
data/datasets/<source>/lambda-0.7/train.labels       one f32 per train position, same order
data/datasets/<source>/lambda-0.7/test.labels
data/datasets/<source>/lambda-0.7/manifest.txt       this label set's completion marker
data/datasets/<source>/lambda-0.65/...
```

Consequences:

- **The positions are stored once**, whatever the number of lambdas.
- **Each lambda is independent.** A network asking for a lambda the dataset lacks gets it added,
  and the existing files stay untouched. So a shared dataset needs no agreement between networks,
  and refining the grid later adds label sets rather than rebuilding.
- **The positions stay final**, as today. A label set is final once its own manifest is written.

## Proposed details

1. **Building.** `prepare <network>`:
   - if the positions are not built, one pass over the segments writes the positions and, in the
     same pass, the labels of every lambda in the network's `label.lambda` (the AB(lambda)
     recursion run once per lambda over scores already in memory);
   - if the positions are built, one pass over the same segments (the manifest's list, in its
     order) writes only the missing label sets, and does not touch the positions.

   Labels need each game's scores and outcome, which live in the PGN, not in the positions, so
   adding a lambda replays the segments but writes nothing else.
2. **Alignment by construction and by check.** The split is `is_test_position(global index)`, so a
   label pass in the same segment order puts every label in the same file and row as its position.
   Each label manifest records its row counts and segment list; the build asserts both equal the
   positions manifest, and so does the reader before training.
3. **Label file format.** A small header (magic, version, lambda) and then raw `f32`s,
   zstd-compressed like the positions for consistency. A row count sidecar as the caches have now.
4. **The lambda in a path.** Written with Rust's shortest `f64` formatting (`0.7`, `0.65`), so the
   config's `0.7` and a directory name agree. Refused unless strictly between 0 and 1.
5. **Reading.** The train and test readers open the positions file and one label file and read them
   in lockstep, yielding the same `CacheRecord` as today, so everything downstream is unchanged.
   Which lambda is a parameter of the reader, given by whichever training run asks.
6. **Config.** `preprocessing_config::LAMBDA` and the refusal of a configurable lambda go;
   `label.lambda` is read from the network's config, a non-empty list, each value in (0, 1), no
   duplicates.
7. **Format versions.** `train.cache`/`test.cache` are not read any more; the record-with-label
   format stays only for the transformer's length-bucketed caches. No backward compatibility: a
   dataset built the old way is rebuilt.

## Approved

The layout and names above; a label set may be added to an existing dataset, with one more pass over
the segments; the positions pass builds only the network's own lambdas. With every lambda known up
front the PGNs are read exactly once.
