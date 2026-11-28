# The dual head on the fast path

**Why (2026-11-28):** the dual networks draw their twins at fixed depths 9 and 12 and lose at
10+0.1, spending the same time a move but reaching about 0.75 plies less (pgn-stats.md, trials 27
and 29). `nnue_bench` on screlu dual against screlu-hidden: the head alone 416.9 ns against 321.4,
a search node 487.2 against 394.8 (23% more); the transform's update the same (70.2 against 73.3).

## The cause

- `second_layer_and_output` takes its eight-rows-at-a-time kernel only when its input is a multiple
  of 16. Screlu-hidden's is its first layer's 16 values; the dual's is 15 clipped values and their
  15 squares, 30: the scalar fallback, 32 dot products of 30 each with a horizontal sum apiece.
- `dual_head_value` turns the first layer's sums into values row by row (`hidden_value`), where
  `first_layer_values` does all 16 at once (`hidden_values_avx2`, equal bit for bit).

## The change

1. **The second layer padded at load,** in `QuantizedHead::Dual` and in every set of `DualCount`:
   its input 30 laid out as 32, `[15 clipped, 0, 15 squared, 0]`, the two new columns' weights 0
   (`padded_dual_second`). `dual_head_value` fills the same layout, so the second layer reads 32, a
   multiple of 16, through the kernel. The padding adds exactly zero; the order of the sums changes,
   so the value may move in f32's last bits against today's, as any vectorized path did when added.
2. **The first layer's values eight at a time** where its rows allow (16 do): `hidden_values_avx2`
   over every row, then the last, the shortcut, overwritten with its raw value as today.

The f32 reference (`dual_head`) is untouched; the network files are untouched; no retraining.

## Tests

The existing ones comparing a dual network's quantized evaluation with its f32 one, kept (their
tolerance covers f32's last bits). New: `padded_dual_second` puts every weight where the padded
input meets it (a second layer of known weights, its value over a padded input equal to the
unpadded one's).

## Then

`nnue_bench` again on both networks: the dual's head near screlu-hidden's. Then trials 27 and 29
rerun at 10+0.1, the dual head's first fair test on the clock.
