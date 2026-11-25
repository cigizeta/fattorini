# TODO

Loose, running list of outstanding action items -- not full designs (those live in
`active/`/`done/`/`archived/`), just things not to forget.

**Pruned 2026-11-06 (the developer):** only open items are kept. Everything settled, superseded or
moot before then, the reasoning behind it included, is in git: the version of this file before the
commit that pruned it. The documents in `claude/` keep their own history as written.

**Names:** the project was Rotor until 2026-11-04, then Temper, Temper Games from 2026-11-05, and
Kalo Games from 2026-11-06, its engine Kalo Chess (kalo-chess-rename.md), and Kalo Chess alone,
with no `chess/` level in `data/` or `configs/`, since later that day (kalo-chess-repository.md),
and Fattorini from 2026-11-07 (fattorini-rename.md). Older documents use the older names and paths.

## 2026-11-25: screlu's and pairwise's inference optimized, then benched, then duelled

- **`-pairwise-hidden-512-8-budget` attempt 1, generation 1, ended 2026-11-25:** 592 rounds, 9h 30m;
  best test loss 0.614776 at round 590 (screlu-hidden's 0.614117: +0.00066). Round robin, 400 pairs a
  pairing: epoch 4.000 first, 51.3%, epoch 2.000 last, 47.0%, as screlu-hidden's. Gate passed, 42
  games, 88.1%. Its duels against screlu-hidden wait for the inference work (below).
- **Pairwise trails on loss at equal width (2026-11-25):** `-pairwise-hidden-512-8-budget` against
  `-screlu-hidden-512-8-budget`, step by step, test loss about 0.0004 behind at round 50, 0.00065 at
  round 497 (0.61491 against 0.61426), train loss as far behind: less capacity (its head reads 512
  products against 1024 squares), no overfitting. Peers use pairwise for speed at equal time, at
  wide transforms, so the duels wait for the engine to take that speed. Its training is not wasted:
  both optimizations are inference, and the finished networks load unchanged.
- **Optimize both screlu's and pairwise's inference** (the developer, 2026-11-25), a design in
  `claude/active/` first. Today the first hidden layer is dense i16 by i16 over every input, squares
  or products alike (`first_layer_values`, `dots_i16_8_avx2`), so pairwise gains only its halved
  inputs. Two optimizations, each for both kinds:
  1. **A sparse first layer:** the inputs' nonzero 4-byte groups found (compare, movemask, a
     lookup table), only their weight columns accumulated. Exact, the same integers. Both should gain;
     a square is zero when its lane is clipped, a product when either of its lanes is, so pairwise
     should save the larger fraction (a hypothesis, unmeasured; skipping works on groups of four,
     zero only if all four are). In absolute time screlu, its first layer reading twice the inputs,
     may save as much or more. First measured, for both: the inputs' zero fraction on test positions, and the
     first layer's share of an evaluation.
  2. **8-bit first layer:** inputs as u8 (squares and products both in 0..=127, `(a * b + 256) >> 9`),
     weights as i8, `maddubs` + `madd` (or `vpdpbusd`). Both gain the same fraction, so screlu saves
     about twice pairwise's time, narrowing pairwise's speed edge. Not exact: first measured
     on the existing networks (the drift, how often `maddubs` saturates); a weight clamp in smithy,
     and new attempts, only if the drift is too large.
- **The bench, a must (the developer, 2026-11-25):** nodes/s of both networks on one idle machine,
  before any optimization, after the sparse layer, after the 8-bit layer, so each step's gain is
  known for each kind; no claim on which kind gains more stands before it.
- **Then the duels,** pairwise against screlu-hidden, both `-512-8-budget`, at depth 9 and 10+0.1,
  on the optimized engine: equal time is what decides pairwise (2026-11-22's planned duels, moved
  after the optimization).

## 2026-11-25: halfk_threats_pairwise_dual

- **Designed and written 2026-11-25** (pairwise-dual-activation.md, its plan
  pairwise-dual-activation-implementation.md): pairwise on the transform; of the first hidden
  layer's 16 outputs, 15 clipped and squared side by side (30) into the second layer, the 16th raw
  added to the output. Tags 29 to 31; `configs/lc0-halfk-threats-pairwise-dual-512-8-budget.yaml`,
  `unit: elo`. Tests and clippy clean 2026-11-25 (the developer). **Next:** its attempt, when the
  training machine is free; its order against the piece-count buckets still open.

- **The piece-count buckets on dual, written ahead (2026-11-25)** (pairwise-dual-count.md, its plan
  pairwise-dual-count-implementation.md), the developer optimistic that dual wins: 8 independent
  dual heads by `(count - 1) / 4`, the transform shared; tags 32 to 34;
  `configs/lc0-halfk-threats-pairwise-dual-count-512-8-budget.yaml`, `unit: elo`. Tests and clippy clean
  2026-11-25 (the developer). **Next:** trained only after dual wins its duels (else the buckets
  go on the winner, as 2026-11-23 says).

## 2026-11-25: the networks' unit on Elo's scale

- **S = 400 / ln 10 centipawns (elo-scale.md), written, tests and clippy clean 2026-11-25:** the
  network's unit, a config's `unit:` key (`elo`, version 3; `lc0`, the old S, version 2); PeSTO's
  reading on Elo's scale; versions 1 and 2 still play. The attempt training now has no `unit:` in
  its frozen config, so it finishes under the old S on any build: absent, the unit is `lc0`. Every
  config in `configs/` writes `unit: lc0`; new ones write `unit: elo`. **The cp scale is judged by a
  full run only** (the developer, 2026-11-25): no scaled run, no rescaled network; a scaled network's
  evaluations spread less than a full one's. Order: the pairwise attempt and its duels; every arena
  and smithy on the new build (the old cannot read version 3); the winner's config copied with
  `unit: elo`; its full attempt, its scores watched in analysis and on Lichess. The most promising network is retrained under `unit: elo`, then duelled against the one
  it retrains. Later, everything retrained: `elo` alone, the historical networks deleted.

## 2026-11-22: the larger corpus, the first attempt on it

- **Lc0's Convert ended 2026-11-22.** Next: `lc0-halfk-threats-screlu-hidden-512-8-budget`
  (batch-8192.md), then its pairwise twin.
- **`-hidden-512-8-budget` attempt 1, generation 1, ended 2026-11-23:** 592 rounds, 4 epochs, 9h 31m;
  best test loss 0.614117, still falling at the end (not comparable with the 1.2B attempts'). Lambda
  from 0.012 to 0.030, 0 from mid-training on. Round robin, 400 pairs a pairing: epoch 4.000 first,
  51.3%, the top four within 0.6%, epoch 2.000 last, 47.6%. Gate passed, 36 games, 94.4%. Duels
  against `-hidden-512-g1-a1`, both won by the challenger: trial 19, depth 9, 252 games, 58.1%
  (about +57 Elo); trial 20, 10+0.1, 34 games, 70.6% (+18 -4 =12; a short sequential match, so
  the size is uncertain and likely overstated). The new champion of the halfk line. The gain mixes
  four changes, not separated: Lc0's larger corpus, 8 king buckets, batch 8192, the budget
  schedule. Next: its pairwise twin, `lc0-halfk-threats-pairwise-hidden-512-8-budget`, started
  2026-11-23, then their duels, which test the prediction of width 512 ("Screlu-hidden and
  pairwise, both at transform width 512"). **A confound:** pairwise alone has psq's 12 castling and
  en passant rows (`HalfKav2HmState`), screlu-hidden none, so the duels measure pairwise plus those
  rows.
- **Next, output buckets by piece count, the mainstream way, on the best network (the developer,
  2026-11-23):** on the winner of pairwise against screlu-hidden at `-512-8-budget`. Expected
  different from October's nulls (material, 78 buckets, and phase, 25: drawn; taper: a null), which
  bucketed a psq network's one output layer on 1.2B positions: here the head is hidden and the
  corpus about 5B. Mainstream (Reckless, Stockfish, PlentyChess, Alexandria, Obsidian): 8 buckets
  by the count of pieces on the board, each a set of the whole head's weights. The design to write
  in `claude/active/` before any code. The screlu networks are not retrained for castling and en
  passant: deleted once superseded.
- **Lc0's data: test91 alone, up to 10B, but not soon (the developer, 2026-11-23).** The next
  release trains on today's corpus, about 5B positions of test91 from 2026-01-01 into July.
  test91 (BT4 in RL mode, 800 visits, active) has data up to today; at about 0.75B a month,
  roughly 8.5B now and 10B around January 2027, a rough estimate. Later: the cap in
  `configs/lc0.yaml` from 5B to 10B, taking effect at a Convert restarted from the page, not while
  an attempt trains. Not test90 (400 visits, another cpuct: a second teacher, against the measured
  polyline; its tars seen were 10,240 bytes, perhaps empty). A later run (test92) judged on its
  description, visits and format before any use: a run's number says nothing of its strength.
  **When data is added: the skipped tars retried** (the developer, 2026-11-23: Lc0's server may have
  solved its speed problem). `data/lc0/skipped.tsv` on Drive holds 122 rows of tars skipped as too
  slow; deleting a tar's rows has the next Convert try it again, no code needed. Tars over
  `max_tar_bytes` (4 GiB) are a separate pass-over, kept unless that limit is raised.
- **Every network always encodes castling rights and en passant (the developer, 2026-11-23).**
  Why (the developer, 2026-11-25): the planned MCTS experiments need them.
  A network without them is not compliant: it will be retrained with them, and deleted. Today
  that is every halfk network but pairwise's, `-hidden-512-8-budget` included.
- **The polyline re-measured on the whole corpus** (the developer, 2026-11-22: "we absolutely need
  to re-measure"): `data/lc0/polyline.txt` (measured on 1.19B) and the frozen set's
  `lambda-polyline/` deleted on Drive, so the first attempt measures it on the finished buckets and
  labels the frozen set again. Test losses from here on are not comparable with the 1.2B attempts';
  only duels compare across the two corpora.
- **A budget's round robin sized by `round_robin_pairs`** (budget-round-robin-pairs.md): 400 pairs a
  pairing, as the grid's, not 125. Its step 4 to see on the next scaled budget attempt.

## 2026-11-20: tuned units, the shown score, the material question, the logo

- **The logo** (logo.md): a tumbling chess clock, in the README, on smithy's page, as the `.exe`'s
  icon and GitHub's social preview; the name and logo under AGPLv3 section 7(e). Done.
- **The score a GUI is shown** (shown-score.md): Lc0's centipawns for the network's expected score
  (`y` to `q = tanh(y/2S)` to `90 tan(1.5637 q)`), on Lichess since that day's snapshot. Small scores
  unchanged (the slopes match at zero), large ones larger. Done.
- **Why a pawn is worth about 30** (material-value.md, `smithy material`): the network follows its
  labels, and Lc0's games score a pawn up about 60%, the exchange up about 59%: the teacher's data,
  not training. Nothing to fix in the pipeline.
- **The search's constants in the network's unit** (tuned-units.md, its plan
  tuned-units-implementation.md): SPSA tunes them as they are; the pawn scale only translates the
  classical defaults for an untuned network and a new tune's starting point. Three constants
  tuned for the first time: delta pruning's victim value, the correction cap, the time manager's
  score window. Tune files carry a units line; `smithy translate-tunes` converted the release
  network's (at 30%), the old files kept as `.classical`. Lichess on the snapshot of that code.
  **Next:** a search tune of the release network, then its time tune, on idle machines only
  (after the Convert and halfk's training): a few Elo at most.
- **README heading:** logo, title and three live badges (Lichess blitz, release, license) through
  shields.io, as io-chess's README. **Two CCRL badges, 40/15 and Blitz, single CPU, newest rated
  version, live through a daily GitHub Action** (the developer: no weekly hand edits), designed in
  ccrl-badges.md. CCRL's 40/15 list of 2026-11-20 already rates Fattorini 2026.11.0 (3227 +80 -80,
  31 games). **The 40/15 badge written 2026-11-25** (ccrl-badges-implementation.md), then **given up
  the same day (the developer):** GitHub disables a scheduled workflow after 60 days without commits,
  which a mature project will reach; every way around it (a keepalive commit, the API's re-enable, an
  external converter) was refused, and a link-only badge declined. The badge, the workflow,
  `bin/ccrl-badge` and `badges/` removed; the README's Strength section states the CCRL estimate.
- **CCRL's entry: authors and links. Sent 2026-11-25** as a new topic in the CCRL Public forum,
  waiting for a moderator's approval (a new account's first two posts are approved by hand). CCRL's
  per-version page shows "Authors:" and "Links:" lines under the rating (as Stockfish 19's), and
  Fattorini's has neither. The message, for CCRL's forum or Discord, or the tester:
  "Hello, thank you for testing Fattorini 2026.11.0 in Cloaked In Red. Could you add its author and
  homepage to its CCRL entry? Author: Calogero Zarba (Italy). Link: Homepage,
  https://github.com/cigizeta/fattorini. Thank you!" The country is the developer's choice
  (2026-11-20), as the other entries give theirs.
- **Moved to done/ 2026-11-25** (the developer), with the other documents finished and in use since.

## Run renamed attempt (2026-11-19, attempt-rename.md)

Done: code, page and documents renamed, every `data/` and Drive migrated, the one-off command
removed, `lc0-halfk-threats-screlu-hidden-budget-8/` (two small attempts under the config's old name)
deleted; lichess-bot's config pointed at the renamed snapshot.
**Checked 2026-11-19, no bug:** those two small attempts seemed to lack `scale:` in their frozen
`config.yaml`, but `forge` takes a scale only for a new attempt, in a fresh directory, and
`freeze_attempt_config` always appends it; their 4,096 test positions (1/1024 of the frozen set) say
they ran scaled, so read it there. Most likely the search for it, a glob over subfolders, missed.

## Next (the developer, 2026-11-06)

In this order:

**The trials renamed on disk (trial-rename.md): done 2026-11-18** on the machine training
`lc0-halfk-threats-screlu-hidden-512`, while its smithy (the old build) trained: 16 trials, `trials.tsv`
and its `.bak`; trial-rename.md to done/ 2026-11-19 (the developer).

**`-hidden-512` run 1 ended 2026-11-19** at round 268, its best test loss 0.616992 at round 262
(width 256's best: 0.617875); its gate passed (46 games, 89.3%). Its duels against `-hidden-g1-r1`,
both drawn and curtailed: trial 17, depth 9, 52.5% for 512 (about +17 Elo, the better evaluation);
trial 18, 10+0.1, 49.3% (its slower evaluation gives the gain back). So width 512 does not pay on
1.2B positions. **Next (the developer, 2026-11-19, batch-8192.md): more data, then two attempts**
(an attempt is a training run, from 2026-11-19), at width 512 with 8 king buckets (halfk-king-buckets.md),
batch 8192: `lc0-halfk-threats-screlu-hidden-512-8-budget`, then `lc0-halfk-threats-pairwise-hidden-512-8-budget`.
Width 256 may never run.

**While Lc0's Convert runs (the developer, 2026-11-19):** about 2 days left then, the 8 king
buckets ready (halfk-king-buckets-implementation.md, step 9), so work needing no GPU, in this order:

1. **Batch 8192: designed 2026-11-19** (batch-8192.md): adopted, no duel; the rate unchanged, 4096
   steps a round. Its two configs written the same day; no smoke attempt, the first full attempt
   checks it.
2. **Speed: the profile: done 2026-11-19** (speed-profile.md). The network 437 ns a node, PeSTO
   226: evaluation 38.7% (finding the threats 16.5%), accumulator updates 11.8% (half of it
   `ChangedRows::between` diffing the boards), move picking 16.3% (whole-list sorting 6.3%, PeSTO's
   too). `between` from the move: kept, exact, no measurable gain; partial sorting: no gain and a
   29% larger tree, reverted (speed-profile.md). Incremental threats stay closed for today's
   simplified threats (the developer). Left: the halfk network's profile.
3. **Lazy SMP, measured: done 2026-11-19** ("Search"): 2.86 times faster to depth 22 at 8 threads,
   each thread 15% slower than alone. Nothing to optimize; the UCI node count undercounts.
4. **The test's Stop that did nothing: fixed 2026-11-19** ("Data and tools"): a hidden box's stale
   Stop shared the card's id and took its click.
5. **Won endgames: dropped 2026-11-19** (the developer): endgames are now better on Lichess.

**Then, more of Lc0's data (the developer, 2026-11-18):** once `-hidden-512` run 1 ends and its
duels are played (its gate, then against `lc0-halfk-threats-screlu-hidden-g1-r1` at depth 9 and
10+0.1), Lc0's data loaded towards 4 * 1024^3 positions. Why: width 512 overfits from its third pass
over today's 1.2B (a pass about 71 rounds of 4096 x 4096). At round 143, as pass 3 began, its train
loss fell 0.0010 in three rounds and went below the test loss, which rose from its best; width 256
shows the same drop at the start of its passes 4 and 5 (rounds about 214 and 285), its pass 3
hidden by 1e-3's noise. So at 1.2B, width 512 trains well for about 2 passes, and a width-512
budget run of 4 passes is not worth starting before the larger corpus.

**Tomorrow, 2026-11-18 (the developer, 2026-11-17):** `lc0-halfk-threats-screlu-hidden-512` run 1
finishes under patience (started 2026-11-17; at round 49 0.62355 against 256's 0.62444 at the same
step, still falling), for a clean comparison with screlu-hidden at 256. Meanwhile: the implementation
plan of fixed-budget-schedule.md, then the code.

**Tomorrow, 2026-11-16 (the developer, 2026-11-15), in this order:**

1. **The PV-node TT cutoff: kept, done 2026-11-16** (pv-node-tt-cutoff.md): test 10 drawn at
   10+0.1 (48.8%, about -8 Elo, within the noise), the temporary `+pvcut` switch then removed.
2. **Lichess on `lc0-psq-threats-screlu-g1-r1@search-time-tuned`: done 2026-11-16.** The snapshot
   of cb71819 (the fix kept, the switch removed) plays since then, with the stricter, shorter book
   and matchmaking on (below), PGNs to `data/lichess`. Five changes at once: a rating move is not
   pinned on any one of them.
3. **The halfk line starts** ("The halfk line" below, step 1: halfk-threats-screlu.md, its plan
   halfk-threats-screlu-implementation.md). **Started 2026-11-16:** `lc0-halfk-threats-screlu`,
   run 1, on the lambda polyline, no scaled run first (the developer); stopped in its round 2 the
   same day for the transform bounded in training (transform-bound.md: no network can be refused by
   the engine's quantization any more), then **run 2 started fresh** (the developer), the whole run
   under the bound: its winner is `lc0-halfk-threats-screlu-g1-r2`. Run 1 abandoned, its folder to
   delete. Then: its gate against PeSTO, its duels against
   `lc0-psq-threats-screlu-g1-r1`, both `@untuned`, at depth 9 then 10+0.1.

**The time tune: done and accepted, 2026-11-15.** 32,768 pairs at 10+0.1, its values in the
network's `time-parameters.txt` on Drive (`hard_soft_ratio` 3.0 to 4.2, `hard_fraction` 0.728 to
0.675, `nodes_a`, `multiplier_min`, `changes_a`/`b` down 9 to 16%, each a steady trend, not drift).
Test 9, STC: `@search-time-tuned` 424.5 of 798 (53.2%, about +22 Elo) against `@search-tuned`,
drawn by the duel's 3.09-sigma rule (about 2 sigma). **The 180+2 duel is dropped** (the developer,
2026-11-15): its time costs more than it would tell.

**Where things stand (2026-11-11).** **Fattorini 2026.11.0 is released** (item 4), published on
GitHub on 2026-11-11. Next: the CCRL testers (item 5), then the wiki page (item 6). Done on
2026-11-10: the CCRL Blitz estimate, about 3250 (ccrl-conditions.md); the root's tablebase score
(root-tablebase-score.md), its snapshot on Lichess since 2026-11-11, waiting to be seen in a
tablebase ending.

**Time management, 2026-11-13: Reckless's hand-picked set beat Classic at 180+2** (test 8, 53.4%
over 654 games, reckless-time-management.md), and ships untuned: Classic, Split and the
`TimeManagement` option removed (reckless-only.md), Lichess on the new snapshot. **lichess-bot's
config was stale:** an old engine, `move_overhead` 2000, and a `SyzygyPath` to a tree that no
longer exists, so no tablebases; all fixed the same day. To check in its log which binary played
the past week's games before trusting them (the 2612, the +1.59 in KQ vs KN). Every arena to be
updated to the commit before the next gate or tune.

**Order (the developer, 2026-11-13):** `lc0-psq-threats-screlu-g1-r1`, search-tuned, is time-tuned
(done 2026-11-15, accepted: "Tomorrow" above) (time-management-spsa.md, its plan time-management-spsa-implementation.md, the suffixes of
tuning-suffixes.md). Every later network: trained, then its search tuned, then its time.

**Blitz time management (2026-11-11 to 11-13): superseded.** Split (blitz-time-management.md,
blitz-time-management-v2.md) never had its 180+2 duel: Reckless's set replaced both it and Classic
(reckless-only.md, above). Also waiting: the CCRL forum account's activation.

1. **Tune the lc0 network** (`lc0-psq-threats-screlu-g1-r1`): done 2026-11-09, 32,768 pairs at
   10+0.1 in name only: **its clocks were scaled to about 0.67** by the smithy's low bench of then
   (smithy-bench.md, fixed 2026-11-13), so it played about 6.7+0.067, which is also why it took
   about 12 hours where the time tune of 2026-11-13, at a true 10+0.1, projects about a day (the
   developer, 2026-11-13: "it was the old search tune that was broken"). Its values still won test
   4. No second search tune for it at the true control (the developer, 2026-11-14): the next
   search tune is a halfk network's. Its result is the network's `parameters.txt` on Drive (razoring 111, reverse futility
   64, futility 214, null move 185, delta 206, SEE prune 90, SEE 46, aspiration 8, history
   pruning -35, good quiet -128); the previous 16k tune's is kept in `tune-16k-old-defaults/`. Then,
   in this order (the developer, 2026-11-09):
   1. **Duel of the new tune: done.** Against the defaults (`SearchParameters::DEFAULT`, which are
      the previous 16k tune hand-rounded), test 4: won by the new tune, 53.5% over 712 games at
      10+0.1, about +24 Elo. It ships. The duel against the previous tune itself is dropped: the
      defaults already were it, near enough.
   2. **Move Overhead: done** (move-overhead.md), default 20 ms, 100 ms since Reckless's time management (2026-11-13); its measurement is under "Before
      the release or CCRL".
   3. **Dropped (the developer, 2026-11-11):** Stockfish's Skill Level is a handicap, not an
      anchor; the release claims the CCRL Blitz estimate from the three anchors (ccrl-conditions.md).
      The matches played, for the record. **The strength assessment against Stockfish**, the release's two rows (binary-release.md,
      decision 10: 400 games all played, 10+0.1, one thread, 64 MB, UHO book, Syzygy to 6 pieces).
      So far: Skill Level 10, a full match, Fattorini 300.5 to 99.5 (+274 -73 =53, 75.1%),
      `data/matches/stockfish-10-vs-lc0-psq-threats-screlu-g1-r1@tuned/20261109-1747/`; Skill
      Level 12, a full match, Fattorini 226.5 to 173.5 (+183 -130 =87, 56.6%),
      `data/matches/stockfish-12-vs-lc0-psq-threats-screlu-g1-r1@tuned/20261110-0859/`; Skill
      Level 13, a full match, Fattorini 205.5 to 194.5 (+154 -143 =103, 51.4%),
      `data/matches/stockfish-13-vs-lc0-psq-threats-screlu-g1-r1@tuned/20261110-0948/` (its
      90-game scout had Fattorini at 43.3%, within the scout's noise). Skill Level 14 never played:
      the item was dropped first. For the developer only, never the README: Stockfish 19's own skill formula
      puts 12 and 13 at about 2910 and 2965, so about 2940 on the scale where amanda-0002 had
      2795 (about level 10).
   4. **Lichess installation** with the tuned network (item 3): done 2026-11-10.
2. **Train and tune a PeSTO network:** a forge run on PeSTO's played corpus, its winner tuned.
   **Already measured (2026-11-08, `data/tests/tests.tsv`, tests 1 and 2):** `pesto-psq-threats-screlu`
   run 2's generation 1 (its gate passed, 91.25% against PeSTO), untuned, lost both duels to
   `lc0-psq-threats-screlu-g1-r1`, untuned: 11.5 of 36 at depth 9 (32%), 5.5 of 24 at 10+0.1
   (23%), about 130 to 200 Elo. Same architecture and pipeline, so the gap is the teacher's.
   **To run again (the developer, 2026-11-10):** PeSTO's hand-crafted evaluation tuned first, then
   the pipeline resumed from there, its corpus played by tuned PeSTO (tuned-self-play.md).
   PeSTO's play is filling the training set (about 47% on 2026-11-06), until `complete.txt`.
   Before the training machine trains on it: delete any local `data/pesto` and
   `data/buckets/pesto*` there, left from the old partial corpus.
3. **Lichess, the tuned lc0 network: done 2026-11-10.** `fattorini-bot` plays the `@tuned`
   snapshot of 97071e5 since 2026-11-11 (the root's tablebase score in); the untuned one on Drive can go. Left: `temper-chess` to close. **The
   mark to beat (the developer, 2026-11-10):** `rotor-bot`, retired, ended at 2395 in blitz (RD 51,
   161 rated games, +101 -39 =21, last seen 2026-11-07), from Lichess's public profile data;
   six weeks of mixed engines, the rating still rising when it retired. **Probably beaten, not yet
   settled:** `fattorini-bot` at 2612.8 in blitz on 2026-11-11 (RD 109, just past provisional; +18
   over the last 12 games), better than 99.9% of Lichess's blitz players (Lichess's own figure),
   its day including a draw against a 2819. **Settled 2026-11-17: Rotor is beaten,** `fattorini-bot`
   at 2608.7 in blitz with RD 45 (+4 over the last 12 games), about 214 points over Rotor's 2395. Matchmaking on from 2026-11-16 (the developer), so
   the RD falls faster: rated bot challenges at 3+2 to 5+3, within 300 points, after 5 idle minutes.
4. **The first release: done 2026-11-11.** Fattorini 2026.11.0, `lc0-psq-threats-screlu-g1-r1@tuned`,
   published on GitHub with `fattorini-v2026.11.0.exe`. Candidate `v2026.11.0-rc.1` checked
   (`about`) and its draft deleted, then `v2026.11.0` tagged on the same squashed commit of `main`;
   `gh`'s first real use worked. PeSTO's network not waited for (the developer, 2026-11-11: PeSTO
   has no chance against Lc0). For the next release, DEV.md's steps: the version of that month,
   the CCRL anchors' matches again if the network or the search changed.
5. **CCRL testers:** tell them of the release, in a topic of its own in the CCRL Discussion Board's
   CCRL Public forum (linked from CCRL's pages). Registered 2026-11-11, under the developer's real
   name as the board prefers; waiting for the administrators' activation, then the first two posts
   are approved before they appear. The post drafted, ready to paste: ccrl-announcement.md.
   **Announced on TalkChess 2026-11-13** meanwhile, in "New engine releases 2026 H2", asking the
   CCRL testers to include it; to watch for replies and test results. **Activation asked
   2026-11-17**, still pending then: TalkChess refuses private messages to an account with few
   posts, so the request to Graham Banks is a public reply in the same thread. **Activated
   2026-11-17**, but no CCRL topic needed (the developer, 2026-11-17): the TalkChess announcement
   was enough, and the engine is already competing. Left: watch for its results and rating.
6. **A Chess Programming Wiki page** (the developer, 2026-11-08), **next after the TalkChess post
   (the developer, 2026-11-13):** facts, politely, no claims. Drafted 2026-11-11 in
   cpw-page.md, not waiting for CCRL (the developer, 2026-11-11), claiming no rating ever; a wiki
   account next. **Blocked 2026-11-17:** the wiki is a temporary read-only recovery, editing to
   return later; the draft waits. As planned: facts only (author, first release, AGPLv3, Rust, UCI; the NNUE and how
   it is trained on a limited compute budget; links to the repository, releases and ratings).
   Claude drafts it from the README and `claude/` for the developer to edit; an account is needed.
   Beside it, a minimal author page: the developer's name and GitHub profile, filled in by the
   developer on the wiki and never in a file here, and their other public chess projects, as Aio64
   (a chess tool, not an engine) once its repository is public.

## Before the release or CCRL

- **The 8-thread, repeating-control and Move Overhead checks: passed 2026-11-11**
  (release-checks.md), cutechess-cli smoke tests on the dry run's `.exe`, no failure of any kind.
- **Clock safety at default settings: answered on `bartolomeo` (2026-11-17)** by Reckless's time
  management (reckless-only.md): a fixed 100 ms `Move Overhead` by default, kept back once from the
  clock, and `EXTRA_MARGIN_MS`, 15 ms, from both bounds. Classic's concerns (no lag margin,
  `MAX_CLOCK_FRACTION`, `(alpha + 1/M) * T`) went with it. The released 2026.11.0, the build the
  CCRL testers have, is still Classic with 20 ms: the margin reaches them with the next release.
  `CHECK_INTERVAL`, 1024 nodes, re-checked the same day: at most 0.4 ms at today's speed, 10 ms
  only below about 100k nodes/s, so it stays. **Depth 1 on a near-empty clock: closed 2026-11-20.**
  It always completes by construction, and two tests check it: a 30 ms clock under a 100 ms
  overhead (`a_clock_below_the_overhead_still_plays_a_legal_move`) and an exhausted node budget
  (`choose_best_move_with_an_exhausted_budget_still_completes_depth_one`). Its cost was never timed
  on its own: a few thousand nodes at most, about a millisecond, inside the 15 ms margin; the
  1+0.01 check at `timemargin=0` (release-checks.md) and Lichess's blitz lost nothing on time.
- **README at the release** (readme-for-humans.md): Strength, the network named: the CCRL Blitz
  estimate alone, no Stockfish (the developer, 2026-11-11), with its full methodology (the developer,
  2026-11-10): the Stockfish 10 bench and S, the scaled control (1'+0.5"), the 10 games at once
  and that the bench ran alone, the anchors with their CCRL ratings and builds, the book, hash,
  tablebases, the games played and the error margin (ccrl-conditions.md); Inside brought to the released architecture; the opening paragraph
  up to date; "Download" linking to this repository's releases page.
- **Say what ships (the developer, 2026-11-10):** the README and `RELEASE-NOTES.md` name the
  network, `lc0-psq-threats-screlu` generation 1: piece-square inputs with simplified threat
  features, SCReLU, trained on Lc0's data. The likely main source of its strength, so said plainly.
- **Credit Lc0's data only if an Lc0 network ships:** `about.hbs` and `RELEASE-NOTES.md` credit it
  whatever the network.
- **A short UCI `id name` for the next release** (the developer, 2026-11-14): 2026.11.0 reports
  `Fattorini 2026.11.0 lc0-psq-threats-screlu-g1-r1@tuned v2026.11.0 2026-11-11 08:20 UTC`, which
  every tester's PGN carries as the player's name (seen in CCRL's "Cloaked In Red"). To be
  `Fattorini 2026.11.0` alone, the network and build moved to an `info string` or `about`.
  **Done 2026-11-16:** `id name` is `Fattorini <version>` alone for a release, with `-dev` for
  every other build (dev-name.md); the description stays in the startup `info string` and
  `about`, both already there. Takes effect with the next snapshot or
  release (Lichess's snapshot of cb71819 still has the long name).

## Code to clean up

- **Atan support and lockstep training of several arms:** deprecated by the 2026-11-05 verdict
  (training-corrections.md), kept "for now". Old networks play from a build of the archive, not
  from the new binaries. To remove, after checking nothing still needs them. **The grid to go soon
  (the developer, 2026-11-16):** no config uses it since that day (the four halfk configs moved to
  `lambda: polyline`, before the halfk line's first run), so `grid_center`, the lockstep arms and
  the per-lambda candidates (`<config>-g<g>-l<lambda>-r<n>`) can go; a design note first, saying
  what still reads old runs' candidates (arena players, the tests form, freeze). **Not before the
  budget has proved itself on full runs** (the developer, 2026-11-18): no code removed while the
  schedule that replaces patience is unproven.
- **The Tests form's network select, with the lambda grid** (the developer, 2026-11-08): once a
  generation trains the polyline's one arm alone, "winner (lpolyline)" chooses among one, so the
  select goes when the grid goes, not before.

## Written, not yet seen working for real

Each to confirm the first time it happens:

- A scatter resuming after a stop (resumable-scatter.md).
- A failed segment read waited out, the wait on the page (segment-read-wait.md).
- A non-training phase resumed on a machine without CUDA (cpu-phases.md).
- The polyline measurement's live bar and its Stop (polyline-from-buckets.md).
- The frozen test set's read in a box of its own, at a resume.
- A test killed without ending (a power cut) still listed and resumable (test-row-at-start.md).

## To design

- **A scaled run's share of a partial source.** `scaled_share` takes 1/s of the positions that
  exist, so on an incomplete corpus a scaled generation 1 trains on almost nothing. Proposed: 1/s of
  the full corpus's size, capped at what exists. Not decided (the developer, 2026-11-04).
- **Continuous training** (continuous-training.md): designed, not planned.
- **A fixed budget and a falling schedule** (fixed-budget-schedule.md, decided 2026-11-17): 4 passes
  over the training set (about 270 rounds), the rate falling with the step from 1e-3 to 1e-5,
  the decay geometric; the candidate chosen by a depth-9 round robin among the checkpoints at every
  quarter epoch from 2 to 4 (nine). Patience, the rollback and the lowest-loss candidate kept,
  deprecated, selectable per config. Plan written 2026-11-17
  (fixed-budget-schedule-implementation.md). **Implemented 2026-11-18**, a scaled smoke run end to
  end; next, `lc0-halfk-threats-screlu-hidden-budget` on the training machine, against
  screlu-hidden at 256 under patience.
- **A batch of 8192: decided 2026-11-19, see batch-8192.md.** The record (the developer,
  2026-11-18): Measured on 2026-11-18 with one network training (`-hidden-512`, batch 4096): the GPU
  85 to 88% busy, steadily, at about 70% of its power limit and most of its memory free, so the feed
  is not the bound and a larger batch is the way to fill it. At 4 passes over about 5B positions,
  about 4.9M steps at 4096 against 2.4M at 8192 (corrected 2026-11-23 from 4.3B, the cap before
  lc0-position-cap.md raised it); the rate schedule's endpoints to be checked at the
  new size, and the change judged by duels, not by speed alone.
- **Another learning-rate rule, much later** (the developer, 2026-11-17): the fixed budget above
  replaces patience; the adaptive rules stay shelved.
  Schedule-free AdamW, gentle rate steps, a sign-driven rule and a slope test were discussed and
  shelved as too experimental, kept for later (learning-rate-rules.md, gentle-rate-steps.md). Why
  they came up: in `lc0-halfk-threats-screlu` run 2 each rate kept training long after its gains
  had become small (3e-3 for 57 rounds), noise renewing its best.

## When self-play comes back

Self-play is paused since 2026-11-02: training starts from Lc0's data or PeSTO's.

- **A warm start fine-tunes, so its stages begin at 3e-4** (3e-4, 1e-4, 3e-5), generation 1 from
  random weights keeping all five. Seen once (`-quiet-r1`'s generation 2: the first two stages only
  led back to the teacher's weights); not built.
- **A 1B-position generation 2**, matching generation 1's corpus size, so a verdict on self-play
  after Lc0 does not hang on data size; a failed gate ends it.
- **A self-play generation's buckets outlive it:** a run's `data/buckets/<run>/gen-<g>/` go only when
  the next generation trains, the last never. Delete them once the generation is decided and at the
  run's end.
- **Self-play segments by time, not positions:** a batch stops starting games after some minutes and
  is sealed once its games end, so every machine stops near the target and publishes often. To
  design: elapsed play across a restart, Drive's many more files.

## The halfk line (designed and written 2026-11-02, waiting for training time)

Lc0's labels, one stage, each step one change from the previous winner, duelled at depth 9 then
10+0.1:

1. `halfk_threats_screlu` against `lc0-psq-threats-screlu-g1-r1` (halfk-threats-screlu.md).
   **Done 2026-11-16:** `lc0-halfk-threats-screlu-g1-r2` won at depth 9 (test 11, 54.7%) and leaned
   to halfk at 10+0.1, drawn (test 12, 52.7%): it is step 2's opponent. Not tuned (the developer).
2. `halfk_threats_screlu_hidden` against step 1's winner (halfk-threats-screlu-hidden.md).
   **Done 2026-11-16:** `lc0-halfk-threats-screlu-hidden-g1-r1` drew both duels against
   `lc0-halfk-threats-screlu-g1-r2`, leaning its way: depth 9, test 13, 425.5 of 794 (53.6%);
   10+0.1, test 14, 386 of 754 (51.2%). Not lost at depth 9, so step 3 runs; drawn at 10+0.1, so
   against it.
3. `halfk_threats_pairwise_hidden` (halfk-threats-pairwise-hidden.md), **unless step 2 loses at
   depth 9** (the developer, 2026-11-16, replacing "only if step 2 wins or draws"): pairwise halves
   the first hidden layer's cost, so a step 2 lost at 10+0.1 alone, to its speed, is the case it is
   for. Against step 2's winner if step 2 won or drew at 10+0.1, otherwise against step 1's.
   **Run 1 trained 2026-11-17:** best test loss 0.61901 at step 1089536 (round 266), its gate
   against PeSTO passed (89.3% over 44 games). **Lost at depth 9** to
   `lc0-halfk-threats-screlu-hidden-g1-r1`, both `@untuned`: test 15, 326 of 718 (45.4%, +212 -278
   =228). **Lost at 10+0.1 too:** test 16, 265.5 of 578 (45.9%, +139 -186 =253). Its speed did not
   make up for its evaluation: screlu-hidden at 256 is the halfk line's best.
4. **Screlu-hidden and pairwise, both at transform width 512** (the developer, 2026-11-17):
   `embedding_dim` 512 for both, to be fair: pairwise at 512 against screlu-hidden at 256 would mix
   the width into the verdict. At 256, pairwise lost at depth 9 (test 15), structurally it seems:
   its head reads half as many inputs (256 products against 512 squares), and a product is dead
   whenever either of its lanes is. Each network twice as costly to train and every accumulator
   update twice as wide: a bench of each, then the two duelled at depth 9 and 10+0.1, and the
   winner against screlu-hidden at 256, which tells whether width 512 pays at all. **Screlu-hidden
   first** (the developer, 2026-11-17): `configs/lc0-halfk-threats-screlu-hidden-512.yaml`.
   **The theory, set before the result** (the developer, 2026-11-17): at width 256 pairwise is in
   effect 128 per perspective, too few; at 512 it is 256, which may hold; Stockfish uses pairwise
   only at widths in the thousands. Supported by the psq line too (SCReLU beat pairwise at 256,
   55.4%). Against it: the transform itself is not halved, only the head's inputs, which its first
   layer compresses to 16 anyway; a product is dead whenever either of its lanes is, at any width;
   and wider networks want more data than Lc0's 1.2B positions. **The prediction,** pairwise against
   screlu-hidden, both at 512, depth 9: clearly better than test 15's -30 Elo (drawn or won), the
   theory holds; about -30 Elo again, it fails, and the cause lies in pairwise itself.
   psq-threats-screlu-512.md has the psq line's notes on width 512.

## Speed (the developer, 2026-11-09)

**Measured 2026-11-13, one thread, a quiet machine: the goal is met.** Fattorini with
`lc0-psq-threats-screlu-g1-r1` (today's snapshot) benched 2.46M nodes/s, Stockfish's own `bench`
1.76M, PeSTO 6.04M (`bin/fattorini bench` embeds no network). The 1.15M below was taken under load.
Threats (threats-timing.md, `--threat-parts`, depth 13): three quarters of an evaluation (101 of
137 ns), but at most 19% of a search, evaluation at most 25%: incremental threats would gain at
most about 23%. The network's cost over PeSTO's (2.5 times per node) lies mostly outside the
evaluation timed, the accumulator updates the likely share: a profile to say. Not urgent.

**The goal: nodes/s at least Stockfish's, one thread, on the same machine.** A shallower depth than
Stockfish's is acceptable; a lower speed is not. With `lc0-psq-threats-screlu-g1-r1`, the arena
benched about 1.15M nodes/s per thread on a quiet machine (the smithy's 841,150 was taken under
load, see "Data and tools"). node-cost.md had amanda-0002, without threats, at 3.32M against
Stockfish 19's 2.03M on the same machine: the threats, recomputed at every evaluation, cost about
two thirds of the speed. Years of Stockfish's optimization are no argument: its
techniques are public, and porting them is quick; what costs time is checking each one (exact
against a from-scratch evaluation) and measuring it (a bench on an idle machine, a duel for search
changes).

1. **Measure:** `bin/fattorini bench` and Stockfish's `bench` on the same idle machine, then the
   bench timing the threat step alone (threats-timing.md), so the share of each cost is known.
2. **Threats optimization: closed 2026-11-13, for today's threats.** The fused kernel
   (fused-threats.md, fused-threats-implementation.md) was exact (565,412 nodes) but gained nothing
   (2,465,798 nodes/s against 2.46M), and was reverted; pawn shifts and incremental threats were
   dropped with it. Today's threats are sparse (a side's pieces attacking the opponent's, 1.6 rows a
   perspective), so adding them is already cheap. **Reopened with richer threats:** a features
   experiment adding more rows (attacks on own pieces, x-rays, pins) designs incremental threats
   with it.
3. **Then down the profile:** accumulator handling, move generation, make/unmake and legality, TT,
   `quiescence_from`; each kept only if a bench shows it pays.

## Evaluation

- **X-rays and pins in the threats:** a features experiment of its own, its inference cost timed.
  **Confirmed for the halfk line (the developer, 2026-11-25):** richer threats, x-rays, pins and
  attacks on one's own pieces, on top of the final halfk architecture (pairwise, dual head,
  piece-count buckets, if they win), one change against it. More rows make incremental threats worth
  designing with it (under "Speed", item 2). A design in `claude/active/` first.
- **Width 512** (psq-threats-screlu-512.md): its state is there.
- **The transformer track, later** (rotor-transformer.md): not before the NNUEs are its baseline; MCTS,
  PUCT with certainty propagation; a policy head (lc0-policy.md, mcts-nnue.md). Its code stays as
  it is until then.
- **The move head** (best-eval-together-with-best-move.md): a move-ordering hint for alpha-beta,
  when started.

## Search

- **Optimize Lazy SMP** (the developer, 2026-11-11): in the release check, 8 threads did not crush 1
  (20 games at 10+0.1, too few to measure, release-checks.md). First measure, on an idle machine
  with enough physical cores: nodes/s at `Threads=1` against `8` (near 8 times is ideal; much less
  points to shared tables contended or hyperthreads), and time to a fixed depth on a few positions
  (about 2 to 3 times faster at 8 is typical; less means the helpers repeat the main thread's
  work). Then the fixes it points to: helper threads diversified (depth offsets, skipped
  iterations), shared counters and histories made per-thread, and a duel at a longer control,
  8 threads against 1, as the measure. **Measured 2026-11-19, nothing to fix:** the 2026-11-16
  snapshot through UCI, six positions (the start, Kiwipete, three middlegames, an endgame) to depth
  22, Hash 256, a new game each, on an idle machine with enough physical cores: 1 thread 21.0 s
  (49.6M nodes, 2.36M nodes/s, twice within 0.5%), 8 threads 7.31 s and 7.36 s, **2.86 times faster
  to depth**, the top of Lazy SMP's usual 2 to 3; each thread about 2.0M nodes/s, 15% below alone
  (the shared table, memory, all-core clocks).
- **The UCI `nodes` and `nps` undercount with several threads** (found 2026-11-19): only thread 0
  prints `info` lines (`uci.rs`, `go_search`), with its own nodes, so 8 threads report about 2M nps
  where engines that sum their threads report about 16M; testers and GUIs read the low figure.
  **Fixed 2026-11-19** (threaded-node-count.md): the helpers' nodes summed into thread 0's line, the
  arena left alone. To see on the next snapshot: about 16M nps at 8 threads.
- **An SPSA of time management, after the search's** (the developer, 2026-11-11): the time rules
  depend on the search's curves (a depth's cost, how often the best move changes, the score's
  swings), so they are tuned against the search that ships. Only after Split's verdict
  (blitz-time-management-v2.md) and the smithy's bench on an idle machine (below), ideally after the
  speed work too. **Designed 2026-11-13: time-management-spsa.md.** **Tuned (the developer,
  2026-11-13): the 18 SPSA-born constants of reckless-time-management.md,** from Reckless's values; never its hand-picked ones, the overhead
  and margin among them, which a lag-free tune would drive to zero. At 10+0.1, scaled by the bench as every tune: its increment is 1/100 of the base against
  3+2's 1/90, so the clock goes through the same phases, and a few Elo needs 32k pairs or more. On
  every available arena, each idle: each game is its own comparison, both sides on one machine.
  Then a 180+2 duel of the tuned values against the untuned, for the depth 10+0.1 does not reach.
  **Done 2026-11-15:** tuned, accepted on test 9 at STC, the 180+2 duel dropped ("Tomorrow" above).
  **Budgets** (the developer, 2026-11-11), fixed in games, not hours (as spsa.md, 2026-10-16):
  - **The search's SPSA: 32k pairs**, as the last tune, which found +24 Elo with about 10
    parameters; 64k only if the parameters' traces are still climbing at the end.
  - **Time management's SPSA: also 32k pairs,** kept for its 18 parameters (the developer,
    2026-11-13), noisier values accepted. Sized when it had only 4: their optimum is flat (20%
    off costs perhaps 1 or 2 Elo), a weak signal needing many games for its direction. Values ending
    near the start are an answer too: the hand-chosen ones were fine, and the 180+2 duel can go.
  - About 12 hours each over every arena, as the last tune's 32k pairs took (the developer,
    2026-11-11).
- **`blend` as two weights tuned by SPSA**, when SPSA tunes search code.
- **Singular extensions and capture history, retried with a network on both sides:** both were
  judged with the hand-crafted evaluation; one duel each.
- **A `[from][to]` history table** beside or instead of `[piece][to]`, decided by a match.
- **Per-technique benchmarking:** a switch per technique and the bench positions with it on and off.
- **To measure, on an idle machine:** today's search against the old one with a network, nodes at
  depth 9 on the bench positions, and positions per game in the two runs' segments.

## Data and tools

- **A test's Stop once did nothing** (2026-11-13, test 6): clicked on the test's screen, opened
  before the smithy restarted, it sent no request the smithy logged (no "stop asked from the
  page"); a reload, then Stop, worked. **Fixed 2026-11-19, not reproduced first** (the developer):
  the job's box, hidden on a trial's screen, kept its stale Stop, and both buttons were `stopJob`, so
  `bindStop`'s `getElementById` bound the hidden one, first in the page, and the card's had no
  handler. A hidden box is now emptied (the Convert slot's too), and each Stop has its own id
  (`stopJob`, `stopTrial`, `stopSlot`). To see working on the next trial stopped from its screen,
  having opened the Trials list first.

- **A failed claim's reason, and a shorter wait** (2026-11-13, after a power cut): an arena restarted
  and answering (checked with curl on loopback and the LAN) read "no arena answering" for minutes,
  then was claimed with nothing changed, most likely at the next retry of a backoff grown to 5 min.
  `claim_one` reports every `post` error as "no arena answering", an HTTP error code or a timeout
  included, so the page could not say which. **Written 2026-11-18:** an HTTP error reads "the arena
  answered HTTP <code>: <body>", no answer "no arena answering: <the transport's reason>", and
  `CLAIM_BACKOFF_CAP` is 1 min. To see working on the next failed claim, then this item goes.

- **The GPU's idle 12 to 15% in training** (2026-11-18, `-hidden-512`, one network, batch 4096): busy
  85 to 88% steadily, no dips, so the feed is not starving it; the gap is between kernels. A profile
  of one training step (a CUDA kernel timeline) to say whether it is a per-step sync or a host to
  device copy not overlapped with compute: if so, a cheap fix worth a few percent. Postponed (the
developer, 2026-11-18): Nsight Systems cannot attach to a running process on Windows, so it launches
a short scaled run of the same config, on an idle machine, never beside a training. Not urgent; the
  larger lever is the batch of 8192 (under "To design").
- **Lc0's Convert, if it runs again:** skip a slow tar only while another worker downloads at normal
  speed (a stall reads as slowness today), and stop retrying a tar skipped N times.
- **The scatter, faster:** only Drive's download bounds it; a wired line or a local copy helps a
  second scatter. Then the single scattering thread.
- **One smithy on any machine** (smithy-anywhere.md): step 7, reproducible buckets; step 8, checks
  across two machines.
- **The smithy as a window** (smithy-window.md): an idea, not urgent.
- **An analysis tool, for Mentes, a separate project:** label any PGN with a player's search.
- **Fattorini owns book generation (the developer, 2026-11-25):** building an opening book is engine
  work, engine-agnostic, while Aio64 sits on top of engines and only consumes books. So one book
  builder, here, driving any UCI engine (Fattorini or Stockfish): today's Polyglot book for
  lichess-bot, and in time Aio64's engine book (now Aio64's `bookGen`: Lichess's CC0 openings,
  Stockfish 19 at depth 20, evaluations only, no game data), which then moves here. To design
  before Aio64's book expansion is planned. Aio64's book avoids game data over licence terms;
  Fattorini's Lichess book is built from LumbrasGigaBase 2400+ games: fine while it only feeds
  the bot and is never published, to reconsider when the builders merge.
- **Book lines judged by search, not game results:** to discuss. Fattorini's own book, designed
  here rather than a ready-made one such as Cerebellum (the developer, 2026-11-11), for Lichess
  alone: CCRL testers use their own books, and the arena's matches keep the UHO book. Seen on Lichess:
  the human OTB book (LumbrasGigaBase 2400+) sometimes leaves the bot worse out of the opening;
  meanwhile lichess-bot's `min_weight` raised from 1 to 10 (a book move needs 10% of its position's
  weight) and `max_depth` lowered from 20 to 8 full moves (the developer, 2026-11-16; planned as 12
  but never applied until then), to see whether that alone helps. Its book path moved to the
  `fattorini` tree and its `pgn_directory` to `data/lichess` the same day, both stale.

## After chess settles

- **Other games** (temper-games-restart.md): Go first, not started.
- **Chess variants**, only where a rating list of independently built engines exists. Chess960 is
  dropped for now.
- **Four ways to use the engine,** label source (its own or Lc0's) by hardware (CPU or GPU): details
  later, among them a GPU network in a release without Python or libtorch.
