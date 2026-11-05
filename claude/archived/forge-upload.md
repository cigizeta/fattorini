# Forge results on Google Drive: implementation plan

**Superseded, 2026-10-22: see forge-cleanup.md**, which removed the single-run `smithy forge` and
`arena select`; a forge is now the smithy's job, its runs local to the smithy.

**Status: implemented 2026-10-20, tests passing on the Lichess machine** (one unrelated flaky failure,
`bucketed_train_stream`'s fixture not found once, passed on rerun); not yet run for real.
Also from 2026-10-20: forge no longer exports stage candidates, only `candidate.nnue`. The
shared code is `rotor::pipeline::generation_sync`; `remote::copy_replacing` is the replacing copy.
Beyond the plan: `arena challenge` also pulls the selection and the winner's candidate, and a stage
round robin uploads the lambda's new candidate at once, since every select pulls a candidate whose
record differs from Drive's (a failed upload exits, and the rerun pulls the old one back and replays
the stage round robin from its games). Stage candidates (`stage-*.nnue`) are not uploaded, so a
select with `stage_round_robin_pairs` still runs on the training machine only.

## Why

`smithy forge` leaves its candidates on the training machine's disk alone, so `arena select`, its
duels and `arena margin` can run only there. On 2026-10-20 `arena select lc0-psq-cut` failed on
the Lichess machine (`candidate.nnue does not exist yet`) while the training machine, which holds the candidates, was
unavailable. Only `promote` and `upload` reach Drive today (`UPLOADED_GENERATION_FILES`,
`upload_or_exit` in smithy's `main.rs`), and only for champions.

## Decisions (the developer, 2026-10-20)

- **Only critical results go to Drive, copied once the computation ends; never a partial file.**
  The forge uploads when every lambda has exported its candidate, the select when it has named its
  winner. Nothing still being written (a running `history.tsv`, a checkpoint, a match in progress)
  is ever uploaded, so any machine with Drive can run the select from finished results alone.
- **The arena checks Drive's `candidate.txt` every time** it resolves a candidate, one small read.
- **A candidate replaces its remote copy whole (option a).** `copy_up` skips a remote file of the
  same size, and a `.nnue` file's size depends only on the architecture, so a candidate exported
  again (a forge resumed or extended) would leave a stale copy on Drive silently. Candidates join the
  opening books as remote files that are replaced whole, via `.partial` and a rename, so no reader
  ever sees a half-written file.
- **The select's results go to Drive too**, so a select run on the Lichess machine is visible to the training machine's
  `promote`.
- **Transformers are out of scope.** The engine cannot run them and they export no `.nnue`; the
  transformer track is to be redesigned with a policy head and GPU play, and decides then what it
  stores on Drive. Nothing below touches a transformer's run.

## Changes

1. **`remote.rs`: a copy that replaces without removing.** `copy_up_replacing(local_file)`:
   `move_replacing`'s copy and rename, without removing the source, which the forge keeps. The
   module comment's list of rewritten remote files gains candidates and the select's records.
2. **The forge uploads at its end.** Once every lambda of the generation has exported its
   candidate (NNUE kinds only), and not before, it uploads each lambda's files in this order:
   `candidate.nnue`, `history.tsv` (complete by then), then `candidate.txt` last, since its
   presence marks the upload complete, as `champion.txt` does for a promotion. A forge stopped
   early uploads nothing. A failure warns and leaves the local run standing, naming the rerun:
   `smithy forge <network>`.
3. **No separate upload command (the developer, 2026-10-20): a rerun of `smithy forge` is the
   retry.** On a generation whose every lambda has its `candidate.txt`, the forge trains nothing and
   goes straight to the upload, which is safe to repeat since candidates replace their remote copy
   whole. To check while implementing: that a forge on a finished generation really trains nothing
   (no lambda resumed, no new rate started); if not, add that early exit here. Also what the training machine runs
   once for `lc0-psq-cut`, whose forge ended before this code existed.
4. **The arena pulls candidates from Drive.** When a lambda's local `candidate.txt` is missing,
   `match_player::network_path` (players named `<network>-NNNN-<lambda>`) and `arena select`'s
   check copy `candidate.nnue` and then `candidate.txt` down from Drive, as `pull_promotion` does
   for a champion. A local candidate whose `candidate.txt` differs from Drive's is replaced, since
   Drive's is the latest export.
5. **The select's results go up.** Once `arena select` has named its winner: `standings.txt`, then
   `selection.txt` last, replaced whole. The match folders under `selection/` stay local: not
   critical, and written while games are played. A select stopped early uploads nothing.
6. **`promote` pulls the selection.** When the local `selection.txt` is missing, `promote` reads
   Drive's, so it can promote on the training machine a select run elsewhere.

## Tests

- `copy_up_replacing` replaces a remote file of the same size and different content, and keeps the
  source.
- Nothing is uploaded while a lambda lacks its candidate.
- A forge on a finished generation trains nothing and uploads.
- The forge's upload order puts `candidate.txt` last (a fixture over temporary directories, as
  `remote.rs`'s tests use).
- `network_path` pulls a missing candidate from a temporary "Drive", and refuses nothing when both
  match.

## Steps for the developer

1. Build and test (`cargo test -p rotor -p smithy -p arena`).
2. On the training machine, once: `bin/smithy forge lc0-psq-cut`, which only uploads.
3. On the Lichess machine: `bin/arena select lc0-psq-cut`.
