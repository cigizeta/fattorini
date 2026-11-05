# Betty and caroline, generation 1: training and selection on the training machine

Decided with the developer 2026-10-05. Caroline (`configs/caroline.yaml`, psq) and betty
(`configs/betty.yaml`, psq plus a hidden layer of 16) train their generation 1 on the whole new
corpus, labelled by the hand-crafted evaluation. This plan takes over caroline-generation-1.md from
its step 5 (`prepare`) on, and adds betty beside her.

## Decided (the developer, 2026-10-05)

- **Everything on the training machine, today**, and nothing before self-play ends: prepare, both trainings,
  both selections, the duels. The Lichess machine takes no part.
- **The rates are amanda's, 3 and 1:** `[3e-3, 1e-3, 3e-4, 1e-4, 3e-5]`, as both configs already
  hold. Not doubled for the 4x batch (caroline-generation-1.md's "Revised" section said doubled;
  corrected there). The two configs stay field for field the same but for `hidden_dim`, so
  caroline against betty measures the hidden layer alone.
- **One after the other, not at once.** Two trainings at once would compete for the cores that
  build batches, and each takes half of RAM for its test cache, so the second would get what is left.

## What the code settles

- **One dataset for both.** At generation 1 `generation::self_play_source` names the dataset after
  its labeller, so both train on `data/datasets/hand-crafted/`, and their lambda grids are the same
  (0.70 to 0.95), so one `prepare` serves both; `prepare betty` afterwards finds it built.
- **No stale dataset on the training machine.** `%LOCALAPPDATA%\rotor` holds no `data/` (checked 2026-10-05), so
  caroline-generation-1.md's trap (amanda's 104M dataset under the same name) cannot bite here.
- **Disk:** an estimate from the record layout, not measured: positions at most about 65 bytes
  (`cache.rs`), six label sets at 4 bytes each, so about 25 to 35 GB for about 365M positions.
  The training machine has 526 GB free. The dataset is local; Drive is only read.
- **Windows training is wired** (`bin/rotor-train` under Git Bash, `torch_cuda.dll` in `.rotor`),
  but this is its first real run on Windows: the first rounds are the test.

## The steps

1. **Self-play ends** (TODO.md: 72.5M positions).
2. **`bin/rotor-train prepare caroline`**: the shared dataset, six label sets. Note its size on
   disk and its time.
3. **`bin/rotor-train train caroline`**: six lambdas in lockstep. The first real run of
   lockstep-lambdas.md and test-features-once.md. Record from the first rounds: seconds per round
   (training and test), GPU utilization, and the `test cache:` line. Estimate 3 to 5 h (TODO.md).
4. **`bin/rotor-train train betty`**: the same, with the hidden layer; somewhat slower expected.
5. **`bin/rotor-arena select caroline`**: the round robin at depth 9, then the gate against the
   hand-crafted evaluation, an SPRT at elo_d 35. Promote on a pass (`bin/rotor-train promote
   caroline`).
6. **`bin/rotor-arena select betty`**: the same, then promote on a pass.
7. **Duels at 10+0.1, both on the training machine:**
   - caroline against betty: `caroline-0001` champion, `betty-0001` challenger, the hidden layer
     alone.
   - caroline against amanda: `amanda-0001` champion, `caroline-0001` challenger (decided in
     caroline-generation-1.md). Amanda's champion is not on the training machine (no `data/networks`): check how
     `rotor-arena duel` resolves `amanda-0001` before step 7, fetching it from Drive if needed.

## Results (2026-10-06)

- **Training:** both ran eight lambdas (0.60 to 0.95), not six. Best test loss: caroline 0.04578
  at 0.70, 0.05023 at 0.85, 0.07052 at 0.95; betty about 7% lower at every lambda (0.04206,
  0.04632, about 0.0657). Rounds on the training machine: caroline about 15-17 s of training plus 3.5-4 s of test
  (7-8 s plus 2 s once self-play stopped), betty about 26 s plus 5 s, slower when the machine had
  other load.
- **Round robins at depth 9, 200 games a pairing:** both won by 0.95, with 0.90 second and within
  the margin (caroline 54.3% and 53.3%, betty 52.7% and 52.4%, +/-2.2%); 0.65 last in both.
  Betty's spread is flatter (46.8% to 52.7%, against 43.7% to 54.3%).
- **Gates at depth 9 against hand-crafted, SPRT elo_d 35:** both pass. Caroline-0001-0.95 84/132
  (about 64%), betty-0001-0.95 88.5/140 (about 63%): both roughly +100 Elo.
- **Duels at 10+0.1 against hand-crafted** (step 7 replaced, the developer: both against PeSTO
  first), 10 pairs on the training machine (clock-pairs-half-logical.md):
  - caroline-0001-0.95: 52.5% (+484 -433 =83), drawn at 1000 games, about +17 Elo.
  - betty-0001-0.95: 44.1% (+274 -356 =70), hand-crafted shown stronger at 700 games, about -41
    Elo.
- **Reading:** at a fixed depth both networks evaluate far better than PeSTO and nearly alike;
  under a clock each gives back its advantage in proportion to how much slower it evaluates, betty
  most. Inference speed, not evaluation quality, now limits strength at real time controls. A
  caroline-betty duel at 10+0.1 would mostly measure the speed difference; at depth 9 it would
  isolate the hidden layer's worth as an evaluation.

## Afterwards

- lockstep-lambdas.md and test-features-once.md move to `done/` once caroline's run confirms them.
- TODO.md's larger-dataset item is decided from caroline's numbers: whether the GPU or the data is
  the limit.
- The label path unfreezes: tt-mate-distance.md and the DTZ tables come after both trainings.
