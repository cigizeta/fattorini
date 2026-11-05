# Caroline, generation 1

**Superseded, see betty-caroline-training.md** (archived 2026-10-12). This caroline (amanda's
shape) was retired in generation 2; the name now belongs to the psq_threats lineage (psq-threats.md).

Decided with the developer 2026-10-03: caroline (`configs/caroline.yaml`) mirrors amanda field for
field and trains once on the whole new corpus (corpus-windows.md), labelled by the hand-crafted
evaluation, as every lineage's first generation is. If caroline's generation 1 beats amanda's,
there is no amanda generation 2: this plan is the project's next training.

## A trap found first: the dataset is named after its labeller

`smithy prepare caroline` builds the dataset of the player that labels caroline's current
generation (`generation::self_play_source`): at generation 1, `hand-crafted`. That name already
holds amanda's generation-1 dataset, `data/datasets/hand-crafted/`: 104,456,800 positions, from
chunks in the old layout (`games/replayed/tcec/hand-crafted/chunk-0000`, no month folder).
`prepare_dataset` finds it built, says "already built" and returns, so caroline would silently train
on the old 104M corpus.

The fix: **amanda's generation-1 dataset is deleted** (the developer, 2026-10-03: old caches are
not worth keeping), `data/datasets/hand-crafted`, by hand, on every machine that holds it (the training machine,
here; check the Lichess machine and Drive). No guard in `prepare`: once it is gone the collision cannot recur,
since every later generation is labelled by a champion under its own name; a stale copy left on one
machine is a matter of care, not code.

## The steps

Steps 5 to 8 are superseded by betty-caroline-training.md (2026-10-05), which runs them on the training machine
with betty beside caroline.

1. **The search settles first** (TODO.md's order): the singular-extensions duel and the margin
   merge, so the replay labels with the search that stays.
2. **The old dataset deleted** (above), on every machine.
3. **Replay of the new corpus by `hand-crafted`**: 2,897 chunks, 289.9M positions, about 80 h on
   the Lichess machine alone (533 positions/s). `arena replay` shares chunks between machines through
   claims on Drive, so the training machine can join and shorten it (the machine list on Drive, `machine::MACHINE_LIST_PATH`,
   decides the split).
4. **Self-play by `hand-crafted` to the corpus's 20%**: about 72M positions, `arena self`.
   To check before starting: how much hand-crafted self-play already exists from generation 1
   (`data/games/self/hand-crafted`), and the rate, to know how long the rest takes.
5. **`smithy prepare caroline`**: one dataset of about 365M positions, six label sets.
6. **`smithy forge caroline`**: six lambdas, 0.70 to 0.95. At today's speed about 90 h
   (TODO.md); lockstep lambdas (implemented, not yet run on the training machine) build each batch once for all
   six, and the test features once (planned) save 15-18 s a round. Both are due before this run,
   and caroline's run is their first real use; amanda's training fields stay mirrored (the 1e-5
   rate is dropped from both, or kept in both, before the run starts).
7. **Selection**: the round robin at depth 9, then the gate against the hand-crafted evaluation,
   an SPRT at elo_d 35.
8. **Caroline against amanda**: `caroline-0001` against `amanda-0001`. Open: the rule and the
   control (a duel at 10+0.1, as the search work, is the default proposal).

## Decided (the developer, 2026-10-03)

- **The training machine joins the replay** (step 3), with the Lichess machine: training waits for it anyway.
- **The 1e-5 rate is dropped**, from `caroline.yaml` and `amanda.yaml` alike, so the two stay
  mirrored: `[3e-3, 1e-3, 3e-4, 1e-4, 3e-5]`.
- **Caroline against amanda (step 8): a duel at 10+0.1**, `caroline-0001` challenger,
  `amanda-0001` champion.

## Revised: the batch scaled to the corpus (the developer, 2026-10-03)

Caroline no longer mirrors amanda's training fields. Her corpus (290M replayed positions plus
self-play) is about 3.5 times amanda's 104M, and on one PC it pays only if training gets through
it, so the batch grows to fill the GPU:

- `training_batch_size` 1024 -> 4096, `steps_per_round` 4096 -> 1024: a round stays 2^22
  positions, so warmup, patience and the test cadence cover the same data as amanda's.
- Learning rates: amanda's, `[3e-3, 1e-3, 3e-4, 1e-4, 3e-5]`, not doubled by the square-root rule
  for Adam at a 4x batch (corrected 2026-10-05: the configs kept amanda's, and the developer
  confirmed them, for betty's sake too).
- `shuffle_buffer` 2^19 -> 2^21 (about 200 MB): 512 batches' worth, as amanda's.

Architecture, labels and selection stay amanda's. The duel of step 8 therefore measures caroline's
recipe (the new corpus with a batch scaled to it) against amanda's, not the data alone: that is the
question that decides which lineage continues.
