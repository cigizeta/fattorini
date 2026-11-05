# Config clean-up after the full shuffle

**Status: agreed with the developer, 2026-10-22, in this order: (1) `shuffle_buffer`, (2) the rest
of the corpus rename, (3) docs housekeeping. All three done the same day: (1) and (2) tested and
committed, (2)'s folder rename seen on the Lichess machine; (3) as listed below.** Follows gen-prepare.md, whose buckets made the NNUE
shuffle buffer dead.

## 1. `shuffle_buffer` leaves the NNUE configs

Every NNUE forge trains on fully shuffled buckets (gen-prepare.md); `PgnTrainStream`, the only NNUE
reader of `training.shuffle_buffer`, survives for tests alone. A transformer still streams through
its buffers (transformer-streaming.md) and keeps the key.

1. `net_config::load` refuses `training.shuffle_buffer` on an NNUE kind, saying why ("an NNUE trains
   on fully shuffled buckets; delete it"), instead of requiring it. A transformer: as today.
2. Deleted from every NNUE config in `configs/` and `examples/`, the retired ones included, since
   they are still loaded by `check`, `install` and the arena's player names.
3. Tests: the NNUE fixtures lose the key; the test that required it now checks the refusal.

## 2. The rest of the corpus rename

Agreed 2026-10-22. A forge's training games are its **training set**, beside its test set.

1. **The key:** `corpus_positions` becomes `training_positions` in the `gen-*` configs, the code
   and its messages; `corpus_positions` is still read, as a serde alias.
2. **The folder:** `gen-NNNN/corpus/` becomes `gen-NNNN/train/`, beside `test/`. A forge renames
   a generation's `corpus/` to `train/` once, as it starts, and says so. Archived generations keep
   `corpus/`: nothing reads them back. The buckets record the old paths, so each existing run
   scatters once more.
3. **The names** in `smithy.rs`, `buckets.rs` and the page: `corpus_*` to `train_*`,
   `Dirs::corpus` to `Dirs::train`; the page's JSON fields alike, which only the page reads.
4. **Stays corpus:** Lc0's converted games as a data source (`lc0/`, `preprocessing_config.rs`,
   `data/lc0`), the rotor and arena crates, older docs.
5. Tests, then on the Lichess machine `gen-psq-smoke` started once: its `corpus/` renamed, a scatter again.

## 3. Docs housekeeping

Finished plans move from `active/` to `done/`, each checked first (approved 2026-10-22):

- **To `done/`:** chores.md, status-page.md, http-transport.md, orders.md, smithy-and-arenas.md,
  lambda-zero.md, and this one.
- **To `archived/`, each with its "superseded, see" note:** lan-only-servers.md,
  distributed-spsa.md, distributed-spsa-plan.md, spsa-live-table.md, forge-upload.md,
  self-training-implementation.md, continuous-self-play.md.
- **Kept:** self-training.md, the parent design; forge-cleanup.md, its status brought up to date.
