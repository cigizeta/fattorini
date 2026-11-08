# Self-play targets on the smithy page

Agreed with the developer, 2026-11-08.

## Problem

1. The page shows a set's target as "held before this play" plus "left to play". A play resumed
   after the test set's last game overshot its target has nothing left, so the target shown is the
   overshoot itself: 4,194,449 of 4,194,449 instead of 4,194,449 of 4,194,304.
2. The page clamps each set's percentage to 100.0%.
3. The Corpus card prints its counts without thousands separators.

## Plan

1. `Work::SelfPlay` gains `targets: (u64, u64)`, the config's test and training targets, set where
   self-play starts (a generation's play and PeSTO's play). The page's view and the status line
   show them as the wanted counts. `test_left` and `train_left` still decide when dealing ends.
2. `page.html` drops the `Math.min(1, ...)`: an overshoot reads above 100%, rounded to one decimal
   as every other percentage (so 145 over 4,194,304 still reads 100.0%, accepted). The bar stays
   full.
3. `pesto::summary` groups its counts with `progress::grouped`, both the config's and those read
   back from the complete file.
4. Tests: the resumed view test gains `targets`; a new one resumes past the test target and still
   shows the config's target.
