# psq_threats_screlu_hidden

**Archived 2026-10-27, abandoned (the developer):** its generation 1 never ran. The psq line ends at
SCReLU: every hidden layer on psq inputs lost at equal depth (betty, dorothy, pairwise_hidden, the
last even with its quantization tail fixed), see psq-threats-pairwise-hidden.md and
hidden-beside-f32.md. Its code and config stay in the tree (tag 18).

**Status:** designed 2026-10-26, revised the same day (one layer of 16 over all the squares); plan
written. Step 1 (engine) done the same day, rotor's tests and clippy passing; step 2 benched
(below). Steps 3 (trainer, export), 4 (config) and 5 (docs) done the same day, tests and clippy
passing. Next: generation 1.

## The bench (step 2, 2026-10-26, one machine, random networks, width 256, depth 9, two runs each)

| | head alone | per search node | parameters |
|---|---|---|---|
| psq_threats_screlu | 147.2, 147.7 ns | 183.9, 186.2 ns | 1,249,025 |
| psq_threats_screlu_hidden | 280.0, 283.9 ns | 315.8, 320.4 ns (1.7x) | 1,257,265 |
| psq_threats_screlu_kings | 320.6, 320.9 ns | 355.6, 357.6 ns (1.9x) | 1,775,425 |

The hidden layer itself is most of kings' cost: about 133 ns of the head against kings' 173; the
king sets about 40 ns more, an upper bound, the bench's positions cycling through distinct kings
as a search's rarely do. Both heads square every lane twice, once into the stored squares the layer
reads and once more in the skip's kernel (`screlu_dot_i16_avx2` from the lanes): a dot product of
the stored squares would spare the second, worth doing if either network wins its duel. Runs after kings' generation 1 (psq-threats-screlu-kings2.md) frees
the training machine.

## Why

Kings with its squares detached before the king layers trains steadily about 1.5% below SCReLU in
test loss, at every piece count (psq-threats-screlu-kings2.md, rounds 4 to 9). Two things differ
from SCReLU at once: a detached hidden layer beside the skip, and its weights picked by the king.
This network tests the first alone (the developer, 2026-10-26: "test the detached idea without the
kings first"): SCReLU's config with `hidden_dim: [16]`, plus the skip and the detach.

## Decisions (the developer, 2026-10-26)

1. **SCReLU with one hidden layer of 16 over all 512 squares**, as `hidden_dim: [16]` says, and
   the output reading the hidden layer and, directly, every square: the skip, SCReLU's own output.
2. **The hidden layer's input detached in training,** so the transform learns from the skip alone,
   as SCReLU's. Without the skip a detach would leave the transform with no gradient at all, so the
   two come together.
3. **Its own tag, 18, `psq_threats_screlu_hidden`** (kind, `ArchitectureKind::PsqThreatsScreluHidden`,
   config `configs/lc0-psq-threats-screlu-hidden.yaml`, SCReLU's with its kind and `hidden_dim:
   [16]`); kings' tag 17 untouched.
4. **Order:** after kings' generation 1 ends; compared round by round with kings' history and
   SCReLU's.

## Design

```
512 squares --(detached)--> 16 units, clipped, and squared beside them --> 32 values --+
     |                                                                                  +--> 1
     +----------------------------------------------------------------------------------+
```

- **Both activations after the 16,** as kings and Stockfish: each unit clipped, then squared beside
  it, so the output reads 32 values and the 512 squares, 544 inputs, one bias.
- **Trainer:** the hidden layer a plain `nn::linear`, `value_head.hidden0` (512 -> 16), the
  output `value_head.output` (544 -> 1); the detach `stack_head::kings_stage_input`, the output's
  input built as kings' (`kings_output_input`'s order: clipped, squared, then the squares).
  Initialisation every network's.
- **Engine:** the hidden layer quantized as a first hidden layer (`QuantizedRows`, input the
  squares at a product's scale), the 32 values and the f32 output weights on them, the skip
  quantized as a screlu output, summed in f32 with the head, as kings' head. One hidden width in
  the header.
- **Expected cost:** the engine's head 512 x 16 = 8,192 multiply-adds, as kings', with no set
  changes when a king moves; training near SCReLU's, about 92 to 100 s a round.

## Implementation plan

Each step with its tests; the developer runs them and commits between steps.

1. **Engine** (`crates/rotor/src/nnue.rs`): tag 18, its name and shape; its layers held as a
   `hidden` layer (512 -> 16) and `output` (544 -> 1) on a screlu network, `validate` accepting that
   for 18 alone; the f32 reference and the quantized head (a `QuantizedHead` variant beside
   `Kings`, sharing its arithmetic); load and write. Tests: round trip as 18; wrong shapes refused;
   an f64 reference; quantized close to f32, vectorised equal to scalar; incremental from scratch.
2. **Bench:** `--threats --screlu --head "16"` builds one at random (today refused); the developer
   benches it against SCReLU and kings, back to back.
3. **Trainer** (`smithy`): the kind, `HeadShape::Hidden`, its head in `nnue_network.rs` and the
   stacked network, the export writing 18, the config's one width. Tests as kings': one row by hand,
   each stacked arm against its own network with gradients, the detach's gradient test, the engine
   against tch through a file.
4. **Config:** `configs/lc0-psq-threats-screlu-hidden.yaml`; the config tests.
5. **Docs:** architectures.md, TODO.md.
6. Build, test, then generation 1 once kings' ends; judged by loss against kings and SCReLU at the
   same rounds; if it matches kings, a duel decides which of the two to keep.
