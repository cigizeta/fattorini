# Corpus size in positions, not files -- rotor-self

`configs/self_play.yaml` now says:

- `file_positions`: the target size of one PGN file, in trainable positions. A file closes at the
  first game boundary at or past it (unchanged behavior, only renamed from
  `positions / files`).
- `corpus_positions`: the target size of one generation's whole corpus. Self-play stops once the
  positions across all finished files exceed it.

`files` is gone: the number of files is whatever it takes to pass `corpus_positions`.

Scope: rotor-self only. rotor-prepare is handled next (see "Out of scope").

## How the corpus total is known

Every finished segment already has a companion `{segment}.pgn.zst.count` with its trainable
positions, written by `compress_and_clear` and uploaded with the segment. The total is the sum of
these over the union of local and remote segment names:

- For each `.pgn.zst` name in the union, read `{name}.count` locally if present, otherwise from
  the remote (`remote::cat`).
- A segment whose count cannot be read (companion missing or unparsable, remote failing): a
  warning, and it contributes 0. Undercounting only means one more batch, never a premature stop.
- Remote listing failing: a warning and local only, as today.

## Changes

1. `src/self_play_config.rs`
   - Fields `file_positions: u64`, `corpus_positions: u64`; drop `positions`, `files`,
     `segment_positions()`.
   - Validation: both at least 1; `corpus_positions >= file_positions`. No divisibility rule.
   - Module doc and tests updated (fixtures use the new keys).
2. `src/self_play/self_play.rs`
   - `segment_count` becomes `corpus_positions_so_far(games_dir, online) -> u64` as above.
   - The completion check in `run` compares it to `config.corpus_positions` (complete when
     strictly greater, "surpassed"), and the messages print positions instead of files, e.g.
     `the hand-crafted games are complete (17 412 000 of 17 301 504 positions in ...)`.
   - Before every batch, whether it then stops or not, it prints the corpus so far, the target
     and the percentage, e.g. `amanda's corpus in ... holds 8650752 of 17301504 positions (50.0%)`.
   - `play_batch` receives `config.file_positions`; `segment_positions` parameters and their
     docs are renamed to `file_positions`.
   - Doc comments mentioning `config.files` / `positions / files` updated.
3. `configs/amanda.yaml`: the comment referring to `positions` now refers to `corpus_positions`.
4. `README.md` (around line 291) and the companion book: describe the two new keys.

## Out of scope: rotor-prepare

`src/prepare.rs:147` uses `config.files`. So the build keeps compiling, it temporarily uses
`corpus_positions.div_ceil(file_positions)` as the file count, which is 128 with the current
config, i.e. today's behavior. The real rework (choosing files by positions, handling overshoot)
is the next step.

## Tests

- Config: the committed config loads; missing/unknown keys and zeros refused;
  `corpus_positions < file_positions` refused.
- `corpus_positions_so_far`: sums local companions; a segment without a companion counts 0; the
  offline path ignores the remote.
