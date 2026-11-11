# Tuned self-play: implementation plan

**Status:** planned and implemented 2026-11-08, awaiting the developer's check. The design and its
seven decisions are in tuned-self-play.md.

## 1. The tune's length and start (`spsa.rs`, `tune.rs`, `smithy.rs`)

- `spsa::DEFAULT_TUNE_PAIRS` 16,384 to 32,768 (decision 4): every config's `length: default` follows.
- `spsa::specs_from(start)`: `SPECS` with each start replaced by `start`'s value, clamped into the
  parameter's range; ranges and `c_end` unchanged.
- `TuneArgs::start: Option<SearchParameters>`: a new tune's starting values, `None` for the defaults.
  `Tune::open` builds the specs from it at every start, and `check_spec` compares them with the
  tune's `spec.txt`: a tune begun under other starting values (the old defaults, or a teacher since
  retuned) is refused rather than resumed. No tune is under way at the change; the lc0 network's
  finished 16k tune is moved aside before its new one (section 4).
- `tune_args` (smithy.rs) sets it from the teacher's tune (decision 5), `teacher_tune`:
  - PeSTO itself: `None`, the defaults.
  - Generation g > 1 of a run: generation g - 1's winner's tune; refused, saying so, while it has
    none.
  - Generation 1: the source's player. A PeSTO run: PeSTO's tune, refused while PeSTO has none. An
    Lc0 run: `None`, the defaults (Lc0 is no Fattorini player).

## 2. The forge's parameters (`smithy.rs`, decisions 3 and 7)

`FORGE_PARAMETERS` goes. Each phase says what its sides play:

- `Work::SelfPlay` gains `parameters`, what its teacher plays. A forge's self-play: the teacher's
  tune, its generation's teacher (`teacher_parameters`): generation g - 1's winner's tune, or
  PeSTO's for a generation taught by PeSTO; refused, with a message naming the tune to run, while
  there is none (decision 7). PeSTO's Play: PeSTO's tune, refused likewise (decision 6).
- `Work::Pairs` gains `sides`, the two sides' parameters for a gate or round robin (a test keeps its
  own, `Shared::test`). A round robin: the defaults on both sides. A gate: the challenger the
  defaults; the teacher the defaults, or PeSTO's tune when PeSTO is the teacher (decision 3).
- The test that pinned the old values goes; tests check that self-play deals its `parameters`, a
  gate its `sides`, a round robin the defaults, and which player taught a generation
  (`teacher_player`).

Read with the decisions: a scaled run's network is tuned (scaled) only to debug the tune, so its
self-play is refused like any other until its teacher's scaled tune is finished.

## 3. Docs

- `spsa.rs` and `tune.rs` module docs, `TuneConfig`'s: every tune starts from its teacher's tune.
- DEV.md's "Every tune starts from the default parameters": now from the teacher's tune, and a
  forge's self-play plays the teacher's tune, which must be finished first.
- The configs' `tune:` comments: 32,768 pairs, from the teacher's tune.

## 4. By hand (the developer), after the check

The order of work (tuned-self-play.md):

1. The lc0 network's generation-1 tune, 32,768 pairs from the new defaults: its finished 16k tune's
   `gen-1/tune/` moved aside first, here and on the remote (one tune a network).
2. PeSTO's tune, 32,768 pairs from the defaults.
3. PeSTO's old corpus, played with the old defaults, set aside on the remote, with any local copy
   and its buckets, then PeSTO's Play again: commands given at that step, after a look at the
   folders.
