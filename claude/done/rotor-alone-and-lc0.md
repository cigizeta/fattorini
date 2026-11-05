# rotor-alone and rotor-lc0; CCRL and TCEC deprecated; replay on any PGN

Plan, not yet implemented. Decided with the developer on 2026-10-16, which turns the TODO's "four
ways to use Rotor" item from a wish into work: CCRL and TCEC games stop being training data, and
`arena replay` is redesigned around one arbitrary PGN.

## Decisions (the developer, 2026-10-16)

- **Two label sources:** `alone`, Rotor's own labels (self-play from the PeSTO bootstrap), and
  `lc0`, Lc0's data (lc0-data.md). Hardware (CPU or GPU) is the other axis and is not in scope here.
- **Cutover at generation 4.** The four networks' generation 3, retraining on the training machine now, finishes on
  the archive corpus; its data is untouched.
- **rotor-alone's corpus is self-play only: 1B trainable positions per teacher** to begin with,
  refined later.
- **The archive path stays runnable until every network's generation 3 is decided**, promoted or
  not, in case a dataset must be rebuilt. Only then are the CCRL and TCEC code paths disconnected
  (step 4). Amanda failed her generation-3 gate (2026-10-16); if she is retried, it is on self-play
  alone (`labels: alone`), never on the archives again, so her failure already counts as decided.
- **Each network's `net.yaml` names its label source.**
- **The generic replay runs on one machine** and is invoked as `arena replay <player> <in> <out>`.
  The one-argument month form stays beside it until it is deleted.

## Why self-play rather than replay: the target must move (the developer, 2026-10-16)

Generation 3 trained on the archive corpus, 80% replay and 20% self-play, and neither amanda nor
betty gained on the board, though both lowered their test loss (generation-3.md). A replay corpus
is the same historical games every generation: only their search scores are relabelled by the new
teacher, and at the lambdas that win (0.98, 0.99) a label is mostly the game's result, fixed in the
archive. So the target barely moves from one generation to the next, and the student converges to
the same network. Self-play's games are new every generation, played by the current champion, so
their results move with her, and the target with them.

What replay gave and self-play must replace on purpose:

- **Positions from strong play** that Rotor's own games never reach. Not as training data any more:
  the generic replay labels an outside PGN as a fixed test set catching rotor-alone's blind spots.
- **Diversity:** many engines' openings and styles. Self-play has one player, so its openings, and
  any randomness in its move choice, must keep its games from narrowing.

Also to watch: draws grow as the champion improves, and a draw tells the value head less than a
decisive game; and test losses no longer compare across generations, each having its own test set
(round zero still compares within one; across generations, only the gate does).

## Design

### 1. `net.yaml`: `labels`

A new top-level key, required: `labels: archives | alone | lc0`.

- `archives`: deprecated, today's corpus (replayed months plus self-play at `self.portion`). The four
  networks carry it until their generation 3 is decided, promoted or not, then switch to `alone`.
- `alone`: the generation's dataset is its teacher's self-play only, `self.positions` of it.
- `lc0`: refused by `smithy prepare` ("the Lc0 corpus is not built yet, lc0-data.md") until that
  pipeline exists. It has no generations, so `smithy teacher` refuses it too.

`smithy prepare` reads `labels` from `net.yaml` and branches. The archive branch is today's
`prepare_dataset`, unchanged. The alone branch is `wait_for_self_play(source, self.positions)` and
`build_dataset_cache` over those segments alone. The `segment.starts_with("games/")` and shuffle checks
apply to both.

### 2. `configs/corpus.yaml`: an absolute self-play size

`self` gains `positions: 1000000000`, the target of every teacher's self-play under `alone`. `portion`
stays while `archives` exists, and goes with it. `corpus_config::CorpusConfig` gets
`self_positions: u64`, which must be at least `file`.

`arena self <player>` drops its archive dependency: the target is `self.positions`, not
`self_play_positions(window_positions)`. Generation 3's self-play is already played, so no running
network needs the old share from `arena self`. (`smithy prepare`'s archive branch still uses
`self_play_positions` to cut its segments, so a rebuild of generation 3 takes the same segments it
took before.)

### 3. `arena replay <player> <in> <out>`: any PGN

A new mode of `replay.rs`, chosen by arity: three arguments are the generic replay, one is the
deprecated months.

- **Input:** `.pgn` or `.pgn.zst`, a local path. **Output:** `<stem>.pgn.zst`, a local path, beside
  a record `<stem>.txt` and `<stem>.pgn.zst.count`, written last, as today. An output with its
  `.count` is finished and refused.
- **Player:** `hand-crafted` or `<network>-NNNN`, as today (`parse_player`, `load_champion`).
- **Prepared as today:** a game without a result, with an illegal move or with a bad `[FEN]` is
  thrown away and counted in the record.
- **Book:** the first `{ Book exit }` comment, if any (today's `BookRule::Marker`), otherwise none,
  so every ply is searched. A game whose marker follows its last move keeps its final position, as
  TCEC's did.
- **Named:** `[Source "<input file name> <index>"]`, the index from 1.
- **Searched and written as today:** depth 9, tablebase labels, `format_output`.
- **Resume, in-order writer, two-stage Ctrl+C:** reused unchanged. The staging and progress files go
  beside the output (`<stem>.staging.pgn`, `<stem>.progress`), not under `data/games/replayed/`.
- **Not used:** months, claims, `machines.txt`, the graph, `.count` matching against an archive. The
  per-machine lock is reused, so two replays on one machine are still refused.
- **Not a corpus:** `smithy prepare` never reads these outputs. They serve as an outside test set for
  rotor-alone's blind spots, the Lc0 ranking check, or Rotor's own Lichess losses.

To share code (as built, 2026-10-16): a small `Job`, the `[Source]` prefix and the book rule, is
what `prepare_game` and `prepare_month` take; a month builds one with `Month::job`. The worker pool
and in-order writer moved out of `replay_month` into `replay_games`, which both modes call;
`resume` and `write_in_order` already took paths. The record's game counts are `games_record`,
shared. Sealing in file mode is local only, inline in `run_file`: no upload, no claim.

### 4. Deprecation, after the generation-3 promotions

When all four networks are promoted at generation 3 and switched to `labels: alone`:

- **In code:** remove `labels: archives`, the month mode of `arena replay`, `arena count`,
  `prepare`'s archive branch, `corpus.yaml`'s windows, `book` and `portion`, `corpus_config`'s
  `SOURCES`, `Window`, `Month` and `BookRule::Graph`/`None`. `BookRule::Marker` becomes the generic
  replay's only rule.
- **Scripts:** `bin/ccrl-download.sh`, `bin/ccrl-total.sh`, `bin/tcec-months.sh` and
  `bin/lib/glacier.sh`'s archive parts are deleted with them.
- **Data:** `data/games/original/` and `data/games/replayed/` on Drive are left for the developer to
  delete by hand. **Deleted 2026-10-17** (2.5 GB), with `data/archived/` (495 MB, the tree from
  before corpus-windows.md, read by nothing), Drive's Trash emptied: Drive from 4.6 GB to about
  1.6 GB used. `data/games/self/` (285 MB) can go: the Lc0 ranking check trains on new self-play
  alone (archives-removal.md). The archive code paths remain until step 4 removes them.
- **Docs:** ccrl-sources.md (generation 4's archive plan, marked deprecated 2026-10-16) moves to
  `archived/` with "superseded, see rotor-alone-and-lc0.md"; lumbra-book.md is already there. DEV.md, README.md and the companion book's corpus
  chapters lose the archives.

**Done 2026-10-17, in two scopes (the developer).** **A, the builders, removed and tested 2026-10-17:**
`arena replay`'s month mode and `arena count`, with the machine list (`machine::load_replay`) that
shared the months; `corpus_config.rs` and `configs/corpus.yaml` whole (the generic replay keeps only
the `{ Book exit }` rule, inline); `prepare`'s build and label passes, so it only fetches `teacher.txt`
and checks the dataset built at every lambda; `dataset_cache.rs` down to the readers (`Manifest`'s
rows and order, `built_manifest`, `missing_label_sets`, `splitmix64`, `TEST_POSITIONS_CAP`);
`ccrl-download.sh`, `ccrl-total.sh`, `tcec-months.sh`; the glacier whole (`archive.sh`, `restore.sh`,
`lib/glacier.sh`, and `machines.txt` on Drive, read by nothing else since the machine list went), the
developer having retired it (it moved only the legacy `data/games/self`); DEV.md's
archives section and generation steps. Kept for now: `labels: archives`, its datasets, `regroup`,
forge's dataset path, `buckets` and `stream-bench`'s legacy branch (`data/games/self` is still on
Drive). **B, the readers (archives-removal.md):** every NNUE on `labels: self`, `prepare` and forge's
dataset path gone; `labels: archives` stays for the transformer alone until it streams.

Until then, each deprecated item's doc comment opens with "Deprecated (rotor-alone-and-lc0.md): ...".
That rule applies only to `claude/`: in source code it reads "Deprecated: CCRL and TCEC are no
longer training data", with no reference to the document.

## Steps

1. `net.yaml`'s `labels`, with `archives` on every current network, and `smithy prepare`'s branch
   (alone, plus lc0 refused). Tests: each value parses, a missing or unknown one is refused, and the
   alone branch takes segments up to `self.positions`.
2. `corpus.yaml`'s `self.positions` and `arena self` without the archives. Tests: the committed
   config loads, and `positions < file` is refused.
3. The generic `arena replay`, with the `Job` refactor. Tests: today's replay tests still pass, and
   a two-game PGN fixture (one with `{ Book exit }`, one without) gives the expected labels count,
   `[Source]` names and resume after a cut.
4. The deprecation notes, then, after the generation-3 promotions, the deletions above.

## Open points

- Whether the generic replay should also accept a fixed book length, e.g. for Lichess games without
  a marker (`--book-plies N`). Not now.
- Where rotor-lc0's networks live and are named (by step rather than `-000N`, TODO), which belongs
  to lc0-data.md's design.
