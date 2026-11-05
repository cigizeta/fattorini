# Replay: sampling CCRL games to a position budget

Status: design, not implemented. Follows ccrl-monthly-archives.md.

## Context

Self-play is no longer needed. Every generation, the champion re-searches replayed games at
depth 9, and its own scores (never CCRL's) make the labels. Two sources:

- TCEC traditional competition games: all of them, every generation (4,413,680 trainable
  positions).
- CCRL 40/15 commented monthly archives: far more than a generation can afford (97.6M trainable
  positions up to 2018, an estimated 280-330M in all), so each generation samples them.

Trainable positions are counted as in ccrl-monthly-archives.md: every position from the first
non-book move to the final position, inclusive.

## Decisions

- Budget: 20M trainable positions per generation in all. TCEC is taken whole, so the CCRL budget
  is 20M minus the TCEC count: 20,000,000 - 4,413,680 = 15,586,320.
- No rating filter, no result filter, no draw sampling. Measured on the whole archive (249 months,
  285,615,946 positions) by bin/ccrl-results.sh, draw shares by positions: CCRL 54.9% overall,
  42-49% in 2006-2019, 58-61% in 2021-2023, 67-68% in 2024-2026; TCEC 54.2%. The recent years
  passed the 55% set as the point to reconsider, and uniform sampling was kept anyway, decided
  with the developer: CCRL games are balanced, so draws are expected; the draw rate is not
  expected to rise much further; and a result-dependent threshold (two probabilities, a target
  draw share) is a complication not worth it. It stays the contained change to make if a network
  turns out too drawish.
- Rejected: the top games by minimum rating. Ratings drift with time, so it selects years, not
  quality; it raises the draw rate; it loses the unbalanced positions of weaker games, which the
  network meets in search; and it is the same set every generation.
- Whole games only: a game is taken with all its trainable positions, or not at all.
- Uniform over games: every game has the same chance, whatever its length.
- A new sample every generation, reproducible from the generation.

## The selection: an independent hash threshold

Each game is decided on its own, with nothing kept in memory:

1. N = the total trainable CCRL positions, the sum of the `.count` files of the months used
   (bin/ccrl-total.sh prints it).
2. p = CCRL budget / N (for 15.6M out of about 300M, p is about 0.052).
3. For each game, u = hash(seed, month, index in the month's file), read as a number in [0, 1).
   The game is taken when u < p.
4. seed = a hash of the player folder name, the player whose search labels the sample, as
   `generation::self_play_source` spells it: `hand-crafted` for the first generations, shared
   by every network, otherwise `<network>/gen-NNNN` (the champion of that generation, labelling
   the next). So the hand-crafted replay has one shared
   sample for all networks, each later generation gets fresh games, two networks at the same
   generation replay different games, and a rerun selects the same ones. Never the machine
   name: the sample is chosen once and all machines replay it, so it must not depend on which
   machine runs `sample`.

The hash is a fixed 64-bit mixer (for example SplitMix64 over the seed, the month as YYYYMM, and
the index), u = the top 53 bits / 2^53. It must not depend on the reading order or the machine.

### Why it fills the budget

The expected total is exact, for any mix of game lengths:

    E[total] = sum over games of p * positions(game) = p * N = budget

The lengths only set the spread. Each game is an independent Bernoulli(p), so

    Var[total] = sum of p (1 - p) positions(game)^2  <=  p * sum of positions(game)^2
               ~  budget * (mean + sd^2 / mean)

With a mean of about 120 positions per game and a standard deviation of about 50, the standard
deviation is about sqrt(15.6M * 141) ~ 47k positions, 0.3%: in practice the CCRL budget within
about +-140k. An approximate budget is enough, since it is a compute budget.

The alternative, exact to one game, is bottom-k by positions: give each game the key u, keep in a
max-heap the games with the smallest keys while their positions minus the largest-key game's
still reach the budget, and take the heap. It is uniform over games too (a game is taken when the
games ahead of it in key order sum to less than the budget, which does not depend on its own
length), but it needs a pass over everything before any game is known, and about 24 bytes per
game in memory (about 4 MB). The threshold is preferred: one pass, no state, and months split
between machines with nothing to merge.

### Requirements on N

N must be the sum over exactly the months being sampled. A month added to the archive changes N,
and so p and the sample: the sample of a generation is fixed by its seed and its months, which the
generation must record.

## Steps

The CCRL months and the TCEC file are read where they are, on Drive
(`/mnt/g/My Drive/rotor/data/games/all/`), streamed through the zstd decoder.

0. Once, the first time `sample` runs: chunk all of TCEC into `sampled/tcec/` (about 44 chunks),
   with its own `sample.txt`. Never redone, never deleted.
1. Count: T = the TCEC chunks' positions (from `sampled/tcec/sample.txt`); the CCRL budget is
   20M - T; N is the sum of the CCRL `.count` files.
2. Select: one pass over every CCRL month, hashing each game, gives the selected games as
   "month index" lines: the list of game indices.
3. Write: traverse the list, copying the selected games' PGN text, whole, into chunk files
   `.pgn.zst` (see Layout). A chunk is closed after the game that takes it to 100,000 trainable
   positions or more, so each holds just over 100,000 (the last one fewer). About 156 chunks.

A chunk's trainable positions are p - b + 1 per game, as in the archive's `.count` files: the
final position is a training position too (replay-command.md).

### The command: `rotor-train sample <player>`

The player is named as `rotor-arena replay` names it: `hand-crafted`, or a promoted champion
`<network>-NNNN` (folder `<network>/gen-NNNN`), refused unless its `champion.txt` exists. A bare
network name is refused, never resolved to its current generation, which could quietly sample for
a player the operator did not name; so is a candidate `<network>-NNNN-<lambda>`.

- Rust, not bash: it runs every generation, belongs with the generation layout, gets an exact
  64-bit hash (SplitMix64), and can be unit tested. `rotor-train` already reads `.pgn.zst`
  segments (`prepare`) and includes `pgn_replay.rs` with pgn-reader. Not `rotor-curate`, which is
  to be retired (a separate cleanup); not `rotor-arena`, which keeps what plays and searches.
- The budget and the chunk size come from `configs/self_play.yaml`: `corpus_positions` (20M) and
  `file_positions` (100,000).
- One Rust module owns both book rules (CCRL and TCEC), unit tested on the games checked by hand
  (2006-03 game 1729: 184 plies, 177 positions; the `{1s}` and `{+0.00/1 0s}` books). The
  selection pass tokenizes every month anyway, so it checks each month's total against its
  `.count` and stops on a mismatch: a cross-check of the awk and the Rust rules on the whole
  archive.
- The zstd decoder sets `window_log_max(27)` explicitly, to match `--long=27`.
- Each copied game gets exactly one comment, `{ Book exit }`, after its b-th ply, b being the book
  plies under its source's rule (TCEC's own marker convention, now used for both sources). The
  replay searches from the position the marker follows, without knowing the rules. Every game
  in a chunk has it, before the first move when b = 0.
- Each copied game also gets `[Source "..."]`: `ccrl YYYY-MM <index>` or `tcec <index>`, the index
  of the game in its file, from 0, as in `selected.txt.zst`. Any trained position can be traced
  back to its original game.
- All other comments are stripped, CCRL and TCEC alike: the replay never uses them (CCRL's
  evaluations are never used, and the marker replaces the book detection), the chunks get much
  smaller, and no stray engine score can reach the labels. Headers and moves are kept,
  variations dropped. The comments are only read while copying, to place the marker, before
  they are removed.
- A corrupt game is thrown away: every selected game is played on Rotor's board, and one with a
  move that is illegal or cannot be resolved from its SAN, or a `[Result]` other than `1-0`,
  `0-1` or `1/2-1/2` (the final position's label), is left out of its chunk and listed in
  `sample.txt` with its `[Source]`. It does not stop the run, and `replay` can assume every game
  is legal. The per-month check against the `.count` files counts every game, thrown away or
  not, since the awk counted them all.
- No FENs: every game of both sources starts from the standard position. A game with a `[FEN]`
  header stops `sample` with an error, naming it.
- TCEC and CCRL are chunked apart: TCEC once for all, CCRL per player.
- Games in archive order inside the chunks: the training data is shuffled later anyway.
- The selected list is saved as `selected.txt.zst` (about 130k lines, "month index" per game),
  written as `.tmp` and renamed once step 2 is complete: step 3 resumes from it without redoing
  step 2, and it records exactly what each generation saw.

### Layout

Three stages under `data/games/`, each with one writer, written locally and copied to Drive with
`remote.rs` (Drive mirrors `data/`):

    data/games/all/ccrl/YYYY-MM.commented.pgn.zst (+ .count)          downloaded, bin/ scripts
    data/games/all/tcec/TCEC-everything-compet-traditional.pgn.zst (+ .count)

    data/games/sampled/tcec/chunk-NNNN.pgn.zst (+ .count)             rotor-train sample, once
    data/games/sampled/tcec/sample.txt
    data/games/sampled/ccrl/hand-crafted/chunk-NNNN.pgn.zst (+ .count) rotor-train sample, per player
    data/games/sampled/ccrl/hand-crafted/sample.txt, selected.txt.zst
    data/games/sampled/ccrl/amanda/gen-0001/...

    data/games/replayed/tcec/hand-crafted/chunk-NNNN.pgn.zst (+ .count)  rotor-arena replay
    data/games/replayed/tcec/amanda/gen-0001/...
    data/games/replayed/ccrl/hand-crafted/...
    data/games/replayed/ccrl/amanda/gen-0001/...

- The player folder is `hand-crafted` or `<network>/gen-NNNN`, as for the seed.
- `sample.txt`: the seed, the months, N, p, the budget, the totals, and per chunk its games and
  trainable positions.
- The chunk number links a sampled chunk to its searched output across the two trees. The claims
  live next to the outputs, in `replayed/<ccrl|tcec>/<player>/`.
- Moving the existing Drive folders (`data/games/ccrl-40-15/` to `all/ccrl/`, `data/games/tcec/`
  to `all/tcec/`) waits until `ccrl-download.sh` has finished; then the bin/ scripts' paths are
  updated. The other files in `data/games/` stay until `rotor-curate` is retired.

Resumable: chunks are written as `.tmp` and renamed. Each sealed chunk, and each game thrown away,
is appended to `chunks.txt` (synced), which records the entries of the selected list each chunk
consumed; a stopped run skips those and continues with the next chunk. Once complete, its lines
become the body of `sample.txt` and it is removed. Corrupt games are detected while copying, not
while selecting, so the selection pass only counts.

Retention: the sampled CCRL chunks (and their `.count`) may be deleted once every one of them has
its searched output, since the outputs hold the same games with the scores, and
`selected.txt.zst` with `sample.txt` records what the generation used; those two are kept. The
sampled TCEC chunks are never deleted, since every player searches them. Nothing under
`replayed/` is ever deleted.

`rotor-train prepare` does the deletion, locally and on Drive, only after it has checked that the
replay has really completed, for TCEC and CCRL alike. `sample.txt` records, per chunk, its number
of games and of trainable positions, so the check does not depend on the sampled chunks, which
may already be gone. For every chunk recorded:

1. its replayed output and the output's `.count` exist, with no `.tmp` left;
2. the output's `.count` equals the chunk's recorded positions;
3. reading the output (which `prepare` does anyway; zstd's checksum proves it decodes whole), its
   games and positions equal the recorded ones, and so does its `.count`.

When the sampled chunk is still there, `prepare` also compares the `[Source]` headers of the
chunk and of its output, game by game, in order. Any failure stops `prepare`, naming the chunk,
and deletes nothing. For the
shared hand-crafted sample, the first network to prepare deletes it; that is safe, since the
check has already passed, and the others need only the replayed outputs.

### The cycle of a generation

1. `rotor-train sample`: select the games and write the chunks.
2. `rotor-arena replay`: the champion searches the chunks at depth 9, on every machine.
3. `rotor-train prepare`, then `train`.
4. `rotor-arena ladder`.
5. `rotor-train promote`.

The replay itself, how machines share the chunks, and the labels (the final position is a
training position, labelled with the result) are in replay-command.md.
