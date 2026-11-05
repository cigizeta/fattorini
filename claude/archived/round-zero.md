# Round zero: the loss of the starting weights

**Superseded, see rollback-to-best.md (2026-10-16).** Written and applied on the training machine the same day,
before another machine's rollback work was pulled there; that work already holds round zero (its step 3), so
this code was dropped unapplied. One decision recorded here differs from rollback-to-best.md and is
still the developer's: a run whose best is still round zero at its end -- here it failed and
exported no candidate; there it exports the starting weights.

Also learned here: the lockstep tests need learnable labels and a gentle schedule to beat round
zero reliably (labels from a row's index never can).
