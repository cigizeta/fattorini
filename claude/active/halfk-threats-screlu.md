# halfk_threats_screlu

Status: designed 2026-11-02 (the developer), no code yet. Step 1 of the halfk line (TODO.md), the
baseline that steps 2 and 3 are judged against. Its implementation plan is to be written next,
after this design is agreed.

## What

`psq_threats_screlu` with its psq rows replaced by halfk's: halfka_v2_hm (called halfk for short),
psq_threats' threat rows kept, SCReLU on the accumulator, no hidden layer, one output. The
32 king buckets factorised in training. Nothing else changes, so the duel against step 0 measures
the inputs alone.

## Decisions (the developer, 2026-11-02)

1. **Inputs: Stockfish's halfka_v2_hm exactly.** Per perspective, a feature is (own king's bucket,
   piece square, piece identity). There are 11 identities: 5 piece types times 2 colours, plus one
   for both kings sharing a plane. The board is mirrored left-right when the perspective's own king
   is on files e to h, so the king takes 32 buckets (4 files times 8 ranks). That gives
   32 x 11 x 64 = 22,528 rows per perspective, shared by both perspectives as psq's are.
2. **Threats: psq_threats' rows as they are,** shared across king buckets, not factorised (they do
   not depend on the king).
3. **Factorisation in training:** a factor of 704 rows (11 x 64, no king), each real feature also
   lighting its factor row, so every bucket learns from every position through the shared part.
   **At export** each real row becomes its own weights plus its factor row's, and the factor is
   dropped: the engine never sees it and pays nothing for it.
4. **Initialisation:** the 22,528 real rows start at zero and the factor random, so training begins
   as a psq network and specialises by king bucket (Stockfish's choice). No warm start from step 0:
   the Lc0 line trains from scratch, and a warm start would confound the duel.
5. **No regularisation of the per-bucket rows:** zero initialisation is enough, one fewer
   hyperparameter.
6. **Width: `psq_threats_screlu`'s, 256** (`configs/lc0-psq-threats-screlu.yaml`): one change from
   the baseline.
7. **Overflow refuses the export.** Export checks the folded network against the engine's i16
   range and fails with a message, never saturating. (Refined in the implementation plan: the
   engine already quantises at load and refuses what does not fit, so the export runs that same
   check on the folded network; no clip in training.)
8. **Engine: two accumulators, summed at evaluation.** Halfk's, rebuilt when the king's bucket
   or mirror changes, and threats', updated as today and never rebuilt by a king move. This costs
   one extra add of the width per evaluation.
9. **Engine: Stockfish's refresh cache in the first version.** One cached halfk accumulator per
   king bucket and perspective, per thread, with the pieces it was computed from: a refresh applies
   only the difference. Holds halfk only (decision 8).
10. **Bench before training:** `nnue_bench` with a random network of this shape against
    `psq_threats_screlu`, on one idle machine. The cost is reported and the developer decides; no
    threshold agreed in advance.
11. **Two runs, Lc0's first:**
    - `lc0-halfk-threats-screlu`: Lc0's labels on `data/lc0-counts`, lambdas 0.01 to 0.09,
      after step 0 (`psq_threats_screlu` retrained on the same corpus). Gated against PeSTO once,
      then duelled against step 0's winner, both `@default`, at depth 9 and then 10+0.1.
    - `pesto-halfk-threats-screlu`, the identity line: PeSTO's self-play, lambdas 0.91 to 0.99.
      The game result carries the label at 91% to 99% (`labels::compute_labels_direct`), PeSTO's
      score being weak. Trained after the Lc0 run; its self-play may play on the arenas meanwhile.

## What exists, and what does not carry over

- **The trainer's `halfka_v2_hm` is not this one.** `FeatureSet::HalfKav2Hm`
  (`crates/smithy/src/feature_set.rs`, `batch.rs`) is the old pipeline's (rotor-halfkav2hm.md):
  Rotor's own additive crossed table, the king's file and rank as separate lanes, 2,112 rows, dense
  batches, `256 -> 32 -> 32 -> 1` head, never loaded by the engine. It shares the name and the
  mirror rule, nothing else. **The old code is overwritten (the developer, 2026-11-02):** the old
  `HalfKp` and `HalfKav2Hm` go (feature sets, dense batches, `rotor::features::{halfkp,
  halfka_v2_hm}` and their tests), and the new halfk takes the name `HalfKav2Hm`, with sparse
  batches as psq's. Nothing trained with the old code is kept.
- **The engine:** the tags `FEATURE_SET_HALFKP` and `FEATURE_SET_HALFKA_V2_HM`
  (`crates/rotor/src/nnue.rs`) name the old formats, and loading them is refused. Both go,
  with the test refusing a halfkp file; this architecture takes a new tag, never reusing 1 or 2,
  so a file of the old format can never be read as the new one.
- **Factorised training exists for heads only** (`stack_head.rs`: a shared part plus a delta per
  bucket, summed at export). The input transform's factorisation is new, but its export follows the
  same pattern.
- **Precedent for the king in the head:** psq-threats-screlu-kings.md and its successor brought the
  king in through the head and lost (7.7% higher loss at round 10). Halfk brings it in through the
  input, as Stockfish does: this run is that comparison's other side.

## Sizes

- **First layer:** 22,528 x 256 i16 = 11.5 MB of halfk rows, plus psq_threats' threat rows as
  today. psq's 768 rows are about 0.4 MB, so the transform no longer fits in cache: a refresh's
  and an update's rows come from memory. The bench (decision 10) measures this.
- **Training:** the factor adds 704 rows, a few percent. Sparse batches keep the active count
  about 32 halfk features plus the factor's 32, per perspective, so a step's work grows less than
  the parameter count. The optimiser state grows with the rows (about 23k x 256 x 3 floats, about
  70 MB): no concern on the GPU.

## Open, for the implementation plan

- How the batches carry the halfk index: computed in the reader from the canonical position, as
  psq's are, with the factor index derived from it on the GPU or on the host.
- The engine's mirror flip: whether a king crossing the d/e line goes through the refresh cache
  like a bucket change (Stockfish: yes).
- The bench's positions: the kings must move between buckets often enough that the refresh cache is
  exercised, unlike kings' bench, whose 7 positions measured the worst case.
- Tests: halfk's index against a hand-computed position, mirrored and not; the folded export
  against the factorised network's forward pass; incremental against from-scratch along random
  games with king moves across buckets and the mirror line; the refresh cache against a full
  rebuild.
