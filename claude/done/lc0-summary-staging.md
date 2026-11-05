# The Lc0 box counts local staging only beside a running Convert

**Problem (2026-11-01).** The page's Lc0 line, `lc0::summary`, counts "downloaded" as the remote
`tars.tsv` rows plus the `.tar` files in `data/lc0-counts/staging`, a local directory. On a machine
whose smithy is not running the Convert, those staged tars are leftovers of an earlier run there,
most of them already converted elsewhere and counted again: the line disagreed with the machine
running the Convert.

**Fix.** The staged tars are added only while this smithy's Convert child runs (`lc0::kept()` is
`Some`). Otherwise the line says only what the remote knows: complete or not, and the tars converted.

**Tests.** `summary`'s wording split into a pure function of the rows, the staged count and whether
the Convert runs here, tested both ways.
