# Lc0 training data as a corpus

Design only: nothing here is implemented yet. First written 2026-10-15; revised 2026-10-16 (the
developer) for value only, a 10 GB cap and a held-out test set by game hash. The goal is to train Rotor's networks on
Lc0's self-play games, the value labelled by Lc0's own search (`best_q`) and blended with the
game's result by Rotor's AB(lambda), within the free space of Google Drive.

The aim is architecture comparison, not a network to ship (TODO, the Lc0 item): no generations,
one long training per architecture on one fixed corpus.

**Decided 2026-10-17 (the developer), superseding the Validation, Epochs, the Lichess machine and LAN sections
below:**

1. **Test set: whole tars held out,** as test-set-freeze.md decided: the earliest tars by date go
   whole to `data/lc0/test/` until it holds 65,536 games; `smithy freeze` runs on it unchanged. No
   hash pool, no `[Lc0Game]` header.
2. **Training: forge streams `data/lc0/train/`** through `PgnTrainStream`, as for self-play
   (forge-from-pgn.md). No epochs built on the Lichess machine, no LAN transfer.
3. **Converter: `smithy lc0`,** whole tars, `tars.tsv`, the 5 GB cap (below). Plan: lc0-converter.md.
4. **Names:** new configs `amanda-lc0.yaml` and `caroline-lc0.yaml`, `labels: lc0`, the same
   architectures, apart from the self-play lineages.

## Source

Lc0 publishes its self-play data as a plain directory listing, linked neither from
lczero.org/dev nor from the lczero-training README:

- https://storage.lczero.org/files/training_data/ -- one folder per run, `LICENSE.txt` (298
  bytes, 2019) at this level, not yet read.
- **test91** (https://storage.lczero.org/files/training_data/test91/) is the run chosen: hourly
  files `training-run2-test91-YYYYMMDD-HHMM.tar`, from 2025-12-20, still produced.

### test91 against test90 (fetched 2026-10-16, https://training.lczero.org/training_runs)

Both are debug runs for training BT4 by self-play, with the same opening book
(`run2_book.pgn.gz`), resignation and pruning settings.

| | test90 (Run3) | test91 (Run2) |
|---|---|---|
| Active | no | yes |
| Self-play visits | 400 | 800 |
| cpuct (at root) | 1.20 (1.414) | 0.65 (2.53) |
| FPU value | 0.49 | 0.23 |
| Data | every tar listed, July to October 2026, is empty | real, hourly since 2025-12-20 |

test90 is useless: it still writes an empty tar (10240 bytes) every hour. test91's 800 visits
also make a stronger teacher.

**Tar sizes vary widely:** about 0.75-0.92 GB an hour in December and January, about 0.12-0.14 GB
in October, dips to 8 MB. A file count says nothing about positions. Rough estimate, from about
500-1000 bytes per gzipped record (unverified): 1 to 2 billion positions a year at today's rate,
several times more at January's.

### What a tar holds

As remembered from lczero-training, unverified against a test91 file:

- Many `training.<game-id>.gz` members, **one per game**, each a flat array of fixed-size
  records, one per ply, in game order.
- The V6 record is 8356 bytes: 1858 policy floats, 104 bitboards (8 history positions x 13
  planes), castling, side to move or en passant, rule50, `invariance_info`, then the search's
  values (`root_q/d`, `best_q/d`, `played_q/d`, `orig_q/d`), the game's `result_q/d`, and
  `played_idx`/`best_idx`.
- `q` is W - L in [-1, +1] and `d` the draw probability, both from the side to move.

**First thing to check, on one early (January 2026) and one current tar:** `version` and
`input_format` in the first record of a game, and whether a decompressed member's size is a
multiple of the record size (8356 for V6). Within one run the format is expected not to change;
the converter still checks `version` on every game and stops on anything unexpected. The same
two tars give the real positions per tar and bytes per position.

**Checked 2026-10-17 on `training-run2-test91-20261017-0417.tar` (123,299,840 bytes):**

- One directory, `training-run2-test91-20261017-0417/`, holding 4,911 `training.<game-id>.gz`
  members (game ids like 229828019): one per game, as remembered.
- The first game decompresses to 576,564 bytes = 69 x 8356 exactly: **V6 records**, 69 plies.
- Its first record opens with `version` 6 and `input_format` 1 (the classical 112-plane input).
- About 210 gzipped bytes a ply on that game (14,478 / 69), so roughly 0.6M positions in this tar,
  about 120 a game: to be replaced by the converter's own count (`tars.tsv`).
- `training-run2-test91-20260101-0017.tar` (411,152,384 bytes): its first game decompresses to
  551,496 bytes = 66 x 8356, `version` 6, `input_format` 1. **The same format from January to
  October:** the converter decodes V6 with the classical input only, and stops on anything else.

**The book is never in the records (checked 2026-10-17 in Lc0's `src/selfplay/game.cc`, master).**
`SelfPlayGame`'s constructor plays the opening's moves with `MakeMove`, no search, possibly cut
short at random (`opening-stop-prob`); the only `training_data_.Add` is in `Play`, after each
search. So a game's first record is the first searched position, after the book, and every record
is a searched position, the played move and the best one kept apart (temperature changes the
move played, not whether the ply is recorded). The terminal position is never recorded, nor one
adjudicated at 450 plies. The converter's `[FEN]` is the first record's position: its book is 0
plies, as in `arena self`. Not ruled out: filtering inside `V6TrainingDataArray::Add`
(`src/trainingdata/`), not read.

**The first tar converted (2026-10-17, the training machine):** `training-run2-test91-20260101-0017`, 824 MiB:
33,216 standard games (1,399 Chess960 or Chess324 skipped, 4%), 3,986,540 positions, about 120 a
game, 19,789,036 PGN bytes in 4 segments: **about 5.0 bytes a position**, above the 3-4 guessed, and
2.3% of the tar's size. So 5 GB holds about 1.0B positions, some 250 January-sized tars, about ten
days of test91. Download 7 min at about 2 MB/s, conversion 2.5 min: about 40 hours for the cap.

### Few-piece positions

Self-play seems not to adjudicate by tablebase (no source found saying it does), and Lc0's
rescorer rewrites tablebase positions' values, which implies such positions exist. But games
mostly end by resignation, so positions with 7 or fewer pieces are expected to be present but
underrepresented, the smallest counts rare. Nothing to do about it here: Rotor probes Syzygy.
Conversion yields the exact piece-count histogram for free.

## Pipeline

```
for each test91 tar from 2026-01-01 on, in date order:
    skip it if empty (10240 bytes)
    download -> convert its games to compressed PGN -> store on Drive -> delete the tar
    stop before the tar that would take the stored PGN past 5 GB
once: reservoir-sample the frozen test set of 4 * 2^20 positions from the mod-64 test games
each epoch: reservoir-sample 96 * 2^20 positions from the training games -> shuffle
           -> group by piece count -> train
```

Only one tar is ever on disk; time is no constraint, space is.

### The window and the teacher

From the first tar of 2026-01-01 forward, until the cap or the latest tar. The teacher is test91's
networks over those months, not one network (accepted, the developer, 2026-10-16: they are all
strong). Every architecture trains on the same corpus, so comparisons stay fair.

### Conversion: Lc0 game -> self-play PGN

Each game becomes one PGN game in exactly the shape self-play already writes (`arena`'s
`pgn.rs`): a `[FEN]` header for the start position, a `[Result]`, and a bare `{<score>}`
comment after every move, compressed as `arena` already compresses. The existing replay and
AB(lambda) labelling (`labels.rs`'s `compute_labels`) then read Lc0 games like any other PGN,
which is also the generic-PGN replay the TODO wants.

- **Moves:** decode `played_idx` from Lc0's 1858 move index, or diff consecutive boards. During
  conversion, check every record's board against the replayed one: it catches decoding bugs.
- **Start position:** decoded from the first record's planes, since games start from a book.
- **Score:** `best_q`, the value under the search's best move, matching Rotor's best-line label;
  `d` unused. Converted to centipawns with `labels.rs`'s exact inverse,
  `100 * pawns_from_score(q)`, rounded; `q` = +-1 maps to the ceiling. Rounding costs little:
  near equality a centipawn is a tiny step of `q`, and far from it the curve is steeper still.
- **Result:** from `result_q`, converted to White's point of view.
- **Policy: not kept.** Adding it later means downloading the tars again; accepted for this
  first corpus.

### Size: a 5 GB cap

**Decided 2026-10-17 (the developer): Drive's space is split 5 GB for self-play (`data/self`) and
5 GB for Lc0 (`data/lc0`),** the rest for Gmail and Photos. This replaces the 10 GB cap of
2026-10-16, set before self-play moved to `data/self`.

**Whole tars only, predicted before download (the developer, 2026-10-17).** A tar is never
consumed half-way, so raising the cap later (more Drive space bought) resumes with the next tar
as if nothing had stopped, and no conversion is ever thrown away:

- **The statistics:** `data/lc0/tars.tsv` on Drive, one row per converted tar: its name, its
  bytes, the PGN bytes it produced, its games and positions. A row is appended only once every
  segment of that tar is on Drive, so the file is also the list of tars done.
- **The prediction:** the tar's size from the directory listing, before download, times the
  largest ratio of PGN bytes to tar bytes in `tars.tsv`. If that would take `data/lc0` past the cap,
  the converter stops without downloading. With no row yet, the first tar always converts.
- **A started tar always finishes:** a low prediction overshoots the cap by its error on one tar.
- **Ctrl+C or a crash mid-tar:** the tar has no row, so the next run deletes its staging and starts
  it over, the only repeated work.
- **The cap** is a config value, not a constant in code.

About 10 bytes of PGN text per ply, an estimated 3-4 bytes per position compressed, so 5 GB holds
roughly 1.25 to 1.7 billion positions. For self-play, 5 GB holds about one teacher's 1B training
positions plus its test set: a teacher's `train/` segments are deleted by hand once its students
are trained, before the next teacher plays. Measured on the first converted tar; one of generation
3's self-play PGN files gives a first check of the same shape.

### Labels

Not stored. Training replays every game and runs AB(lambda)'s backward recursion over its scores
for each lambda, so the lambda set is never baked into the data. At lambda = 1.00 the label is
the game result.

### Epochs: reservoir sampling

The tars are streamed once, into the PGNs; the PGNs are then streamed at the start of every
epoch (the developer, 2026-10-16).

- **One epoch is 96 * 2^20 = 100,663,296 positions**, divisible by every power-of-two batch size
  up to 2^25. With 2.5 to 3.3 billion stored positions an epoch sees about 1 in 25 to 1 in 33.
- **Before each epoch, a fresh reservoir sample** (Algorithm R, Vitter 1985; Algorithm L skips
  faster but every ply is replayed anyway) of exactly 96 * 2^20 positions over the stream of all
  plies of all training games, those with `hash(game-id) mod 64 != 0` (Validation). It needs no
  total count, so games ingested between epochs simply join the next draw.
- Sampling **positions, not games**: each game contributes a small share of its plies, so
  neighbouring, near-identical plies rarely share an epoch.
- **Shuffle the reservoir** afterwards: its slots are not in random order.
- **Group by piece count** (2 to 32) for alpha_pieces with a stable partition, keeping the
  shuffled order within each count; every batch then has one length, as
  `alpha-pieces-value-test.md` requires. psq uses the shuffled array directly. **No `smithy
  regroup` for this corpus.**
- Epochs overlap at random: a position is in some epochs and not others, not exactly once every
  25 to 33.
- A run ends as today, when patience runs out after the last learning-rate stage.

### Building an epoch: on the Lichess machine, out of RAM

The training machine's RAM is too small for an in-RAM reservoir of about 5 GB (100M x a packed board
of ~24 B and 11 labels of 2 B), twice that when the next epoch builds beside it. So epochs are
built on the Lichess machine (the developer, 2026-10-16: CPU work, it needs no GPU), on its local disk, and
the training machine only trains:

1. **A reservoir of references:** Algorithm R over 8-byte references (game index, ply), about
   0.8 GB for 96 * 2^20; the same sample as a reservoir of positions.
2. **Sorted into stream order**, then one replay of the PGNs emits only the selected positions,
   with every lambda's label from the per-game backward recursion.
3. **Shuffled on disk:** each emitted position goes to one of, say, 64 bucket files chosen at
   random; each bucket (about 1.5M positions, about 70 MB) is shuffled in RAM and appended to the
   epoch's file. For alpha_pieces, the stable partition by piece count is one set of buckets per
   count.
4. **The epoch's file** is in the seekable zstd-frame format of `smithy forge`'s caches, so
   training reads it as it reads a prepared cache today, and resumes still seek.

Time is no constraint: the Lichess machine builds epochs ahead of training, whenever Lichess leaves it idle.

### Transfer: over the LAN

The developer, 2026-10-16. Finished epochs go from the Lichess machine to the training machine's local disk over the local
network, to be set up; Drive holds only the Lc0 PGNs and the frozen test file, never an epoch. The test file is
backed up on Drive because it is the most precious: every architecture is judged on it.

### Validation

One fixed held-out test set of exactly 4 x 2^20 positions, the same for every architecture
(the developer, 2026-10-16):

- **A key per game:** the conversion writes the game id of `training.<game-id>.gz` into a PGN
  header, e.g. `[Lc0Game "<game-id>"]`.
- **The test pool by hash:** every game with `hash(game-id) mod 64 == 0`, about 1/64 of the
  corpus. Membership depends on the game alone, not on download order or where the cap stops, and
  the pool spreads over the whole window, every teacher included.
- **A pool game is never a training game:** training skips every game with `mod == 0`, not only
  the positions sampled from it, so no near-identical neighbouring ply leaks into training.
- **Exactly 4 x 2^20 positions:** one reservoir sample over the pool's positions, with a fixed
  seed; positions across many games are less correlated than whole games.
- **Frozen:** written once as its own file (positions, `best_q` and result, so every lambda's
  label can still be computed), and reused even if the corpus later grows past the cap.

## Open points

- **`LICENSE.txt`** (read 2026-10-17): ODbL 1.0 for the database, DbCL 1.0 for its contents, as
  expected. As read, not legal advice: a network trained on it is likely a Produced Work, shipped
  with a notice naming the Lc0 training data as its source and its licence; the converted PGNs are
  a Derivative Database, to be offered under ODbL if ever published (on Drive, private, they are
  not). The networks' own licence (AGPLv3, with Rotor) is unaffected by either.
- **Record version** of test91: settled, V6 with `input_format` 1 in both tars checked (above).
  Whether its records carry board transforms (`invariance_info`) is still to see, in the
  converter's own decoding.
- **The frozen test file's size** on Drive, beside the 10 GB cap.
- **The PGNs on the Lichess machine:** read from `G:` (Drive kept offline there) or a local copy.
- **The LAN transfer:** its setup (e.g. a shared folder on the training machine), how the training machine learns an epoch is
  ready, and when an epoch is deleted on each side.
- **Where it lives:** which `smithy` subcommands do download/conversion and per-epoch building,
  and how an epoch's file feeds `smithy forge` in place of the prepared caches.
