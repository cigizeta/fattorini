# test-loss-by-pieces

## Goal (the developer, 2026-10-17)

Every round's test pass reports, beside the global test loss, one test loss per piece count. That
makes 30 per lambda, 3 to 32 pieces with the kings, since K vs K is never frozen
(transformer-streaming.md). It is meant first as a check of that plan's stream: a bucket that
never trains shows as a piece count whose loss stays at its starting level while the others fall,
which the global loss hides. Beyond that, it shows where a shape is weak, which experiment 5
compares across shapes.

## Design

- **A key per test batch.** `round::run_test_pass` notes each batch's piece count while preparing
  it: `Some(count)`, kings included, when every record of the batch has the same count, otherwise
  `None`. `GroupedTestSet` gives only single-count batches. A mixed batch, as `round.rs`'s tests over
  flat files make, counts toward the global loss only.
- **Kept batches keep their key.** `TestCache` stores each kept batch's key beside its rows, so a
  pass replaying kept batches attributes them as the first pass did.
- **One running sum per piece count and arm,** on the device like the global one, with the rows it
  covers, read once at the end of the pass. `run_test_pass` returns `TestLosses`: the global loss
  per arm, as before, and for each piece count its loss per arm.
  `RoundOutcome` carries the latter as `test_loss_by_pieces`.
- **The global loss is unchanged,** and so are `history.tsv` and everything that reads it:
  patience, stages, retention, the candidate.
- **`pieces.tsv`, wide, one per lambda run directory,** beside `history.tsv`: the header `step`,
  `p3` ... `p32`, then one row per recorded round, step 0 included, appended right after that
  round's `history.tsv` row. A piece count with no test position gets an empty field. A crash
  between the two appends leaves a `history.tsv` row without its `pieces.tsv` row, which is
  harmless: the file is for reading, and nothing is replayed from it.

## Test

- `round`: a batch's key is its count when uniform, `None` when mixed.
- `history`: `pieces.tsv` gets its header once, one row per call, and empty fields for missing
  counts.
- `test_cache`: the existing tests, with the key passed through.
- The existing `run_test_pass` tests: the global loss unchanged.
- First real run: `pieces.tsv` appears beside `history.tsv`, its rows falling round by round.
