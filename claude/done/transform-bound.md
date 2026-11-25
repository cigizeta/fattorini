# The transform bounded in training

**Status:** decided by the developer 2026-11-16 ("Safety is more important"), implemented the same
day; the first `lc0-halfk-threats-screlu` run stopped in its round 2 to restart with it. Tests to
run.

## Why

The engine quantizes a network at load (`QuantizedPsqNetwork::quantize`) and the halfk export runs
the same check before writing. It picks the finest scale at which every accumulator lane's worst
case fits `i16`, and refuses the network when even the coarsest does not: when, for some lane, the
bias plus its `MAX_ACTIVE_ROWS` (37) largest rows within one group (halfk: one king bucket) exceeds
`32,767 / 255`, about 128.5, for SCReLU and pairwise networks (`/ 127` for plain ReLU). Nothing in
training prevented that: a refusal was only unlikely, and it would come at the end of a run, after
hours of training, the export exiting with the trained network unusable as it stands. A network's
safety must hold by construction (the developer, 2026-11-16).

## Decisions

1. **Every NNUE network's transform clipped to plus or minus `TRANSFORM_BOUND` = 1.5,** its weights
   and its bias, after every optimizer step: the optimizers' existing bounds
   (`weight_bounds`, as the first hidden layer's `FIRST_HIDDEN_WEIGHT_BOUND`), in every
   architecture, not halfk's alone, so no trained network can be refused.
2. **The bound's margin, halfk being the worst case:** an exported halfk row is its trained row plus
   its factor row, so a lane reaches at most `B + 37 * 2B = 75B`; at the strictest scale, 255, that
   fits for `B` up to `(32,767 - 19) / (255 * 75)`, about 1.71 (19 the rounding allowance). 1.5
   leaves about 12% of margin; a psq network (`38B`) has twice as much.
3. **Not a regulariser:** trained transform weights are typically far below 0.5, so the clip
   should never bind, and halfk still differs from its psq baseline by its inputs alone. The psq
   baseline, trained without it, is not retrained.
4. **Proved by a test, not by argument:** a halfk network whose every transform parameter is at the
   bound quantizes; the same at 1.8 is refused, so the test can fail.
5. **The refusal path stays as it is** (`candidates.rs`: the error logged, the training process
   exiting): with the bound it is unreachable, and its message already names the cause.
