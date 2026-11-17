# Gentle rate steps

**Status:** config written 2026-11-17 (`configs/lc0-halfk-threats-pairwise-hidden-gentle.yaml`), not
yet run. No code change. **Tried on pairwise hidden only if it turns out the best network** of the
halfk line (the developer, 2026-11-17): the schedule is measured on the architecture that will ship.

## Why

Run 2 of `lc0-halfk-threats-screlu` (`history.tsv` on Drive): 271 rounds, about 2.6 hours, five rates
from 3e-3 to 3e-5 at a patience of 6. Each rate kept training long after its gains had become small:
3e-3 ran 57 rounds, gaining about 0.0006 in its last 40, while the first round at 1e-3 gained 0.0021.
The test loss moves about 5e-5 from round to round, so in a slow tail noise keeps setting new bests
and patience keeps restarting. The rollbacks themselves cost little: 30 of 271 rounds.

Considered and set aside, for now:

- **Schedule-free AdamW** (Defazio et al., 2024): too new to trust first (the developer,
  2026-11-17), though nnue-pytorch offers it as an option.
- **A fixed decay** (bullet's step schedule, nnue-pytorch's 0.992 per epoch): both fix the length in
  advance, which is not known here.
- **A slope rule** (a line fitted to the current stage's last rounds, a cut when it flattens): its
  threshold had no trustworthy reference. A slope relative to the stage's first rounds is noisy; one
  relative to the previous cut's drop is the candidate if this note's rule is not enough.

## The rule tried

The existing rule, other numbers: rates from 3e-3 to 3e-5 in steps of about 0.7 (14 rates instead
of 5), patience 3 instead of 6, rollback unchanged. Gentle steps make an early switch cheap, so a
shorter patience is safe. On run 2's first stage, patience 3 would have switched at about round 47
instead of 57: noise still renews the best, so most of the gain is expected from the steps, not
the patience. To watch: whether the low rates' stages stay long, noise renewing their best more
easily as their gains shrink.

## The comparison

Against `lc0-halfk-threats-pairwise-hidden`'s run 1 winner (training on 2026-11-17), same corpus:

1. Final best test loss and rounds (time) to reach it; the rounds each rollback threw away.
2. A depth-9 duel of the two winners, then 10+0.1 if it is close.

Histories depend on the architecture, so a win here is confirmed once on another architecture before
the gentle steps become every config's.
