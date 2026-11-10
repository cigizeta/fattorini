# CCRL's testing conditions, and playing them here

Written 2026-11-10. Replaces gemini-ccrl-scaling.md, whose figures are right but came without
their source; this document gives each fact with where it comes from.

**Sources** (read 2026-11-10):

- CCRL Blitz: https://computerchess.org.uk/ccrl/404/about.html
- CCRL 40/15: https://computerchess.org.uk/ccrl/4040/about.html
- The reference bench: TalkChess, "CCRL Blitz conditions", https://talkchess.com/viewtopic.php?t=79168
  (January 2022)

## The conditions, as CCRL states them

| | Blitz | 40/15 |
| --- | --- | --- |
| Time control | 2'+1" | 40 moves in 15 minutes, **repeating** (another 15 minutes for each next 40 moves) |
| Reference machine | Intel i7-4770K | Intel i7-4770K |
| Hardware scaling | Stockfish 10 as the benchmark | Stockfish 10 as the benchmark |
| Hash | 128 or 256 MB, the same for all engines | 256 or 512 MB, the same for all engines |
| Tablebases | 4, 5 or 6 pieces; the list's header says "up to 6 piece" | 4, 5 or 6 pieces; the list's header says "3-4-5 piece" |
| Tablebase hash | 32 or 64 MB | 32 or 64 MB |
| Pondering | off | off |
| Book | any generic book, at most 12 moves per side, the same for all engines | the same |
| Engine's own book | disabled; an engine that cannot disable it cannot take part | the same |
| Learning | book and position learning off | the same |
| GUI | any (Winboard, Arena, Shredder, ChessBase, Cutechess, ...) | the same |
| Format | any (match, round-robin, gauntlet, Swiss, ...) | the same |

Hash exceptions on both pages: an engine on 4 CPUs may use up to four times the hash of the
single-CPU engines, and a smaller hash is allowed when an engine has problems with a size or
cannot set it.

## What CCRL does not state

- **The benchmark's figures and formula:** not on CCRL's pages; see "The reference bench" below
  for where they are.
- **Threads:** no rule, beyond the hash exception for 4-CPU engines.
- **Adjudication** rules.
- **Engine requirements:** no protocol, platform or licence requirement, and no submission
  procedure. Testers choose the engines they test; the only route given is the forum linked from
  both pages. CCRL also says it does not judge an engine's originality.

## Fattorini against the conditions

From `crates/fattorini/src/uci.rs`:

- **Hash:** `min 1 max 1024`, so 128, 256 and 512 MB are all accepted. The default is 16 MB:
  testers set it, since the conditions require it.
- **Tablebases:** `SyzygyPath` is supported.
- **Ponder:** an option, default off.
- **Own book:** none, so nothing to disable.
- **Repeating control:** 40/15 is exactly the untested `movestogo` path (TODO, "A repeating-control
  check"). Blitz is Fischer, the path Lichess and the arena already play.

## The reference bench

From the TalkChess thread above:

- **Gabor Szots, signing "CCRL testing group"**, 2022-01-19, gives Stockfish 10's `bench` on an
  i7-4770K: total time 2,054 ms, 3,939,338 nodes, 1,917,886 nodes/s. In his words the machine is
  "not mine, I've looked up an old post". Not stated: the build (x64, popcnt, bmi2), the compiler,
  the bench's arguments (presumably none).
- **Graham Banks**, same thread: "We no longer use the Crafty bench, only the Stockfish 10 one",
  and for 40/15 he plays "40 moves in 16 minutes repeating (rounded up)": testers scale, then round
  to a convenient control.
- 3,939,338 is Stockfish 10's official bench signature (its release notes), so the figure is
  plain Stockfish 10.

It is a figure recalled by a CCRL tester, not one CCRL publishes, but it is the one its testers
use. For comparison, the same thread has an i7-2600K at 3,010 ms (x64) and 2,903 ms (popcnt): the
build moves the figure by a few percent, less than the rounding testers apply.

## Scaling a control to a machine here

Multiply the base time and the increment by

    S = our bench time / 2,054 ms

so a faster machine plays a shorter clock. For 40/15 the base is scaled and the 40 moves are kept.
Rounding, as testers do, is fine.

Our side:

- Stockfish 10, the official release (tag `sf_10`), unpacked in `data/stockfish-10/` (gitignored),
  never registered in `data/engines.yaml`: it is only benched.
- The `popcnt` build (`bmi2` is very slow on AMD before Zen 3, and the reference build is not
  known; the builds differ by a few percent).
- `bench` with no arguments (16 MB, 1 thread, depth 13), run 3 to 5 times on an idle machine as
  CLAUDE.md defines it, plugged in, the average taken. Its node count must be 3,939,338; any other
  count means another build or version.

## Measured (2026-11-10)

On the machine for these matches, idle, `stockfish_10_x64_popcnt.exe bench`, five runs: 1,049,
1,047, 1,044, 1,062 and 1,057 ms, every run 3,939,338 nodes. Mean 1,051.8 ms, about 3.75M nodes/s.

    S = 1,051.8 / 2,054 = 0.512

| CCRL | Scaled here | Rounded |
| --- | --- | --- |
| Blitz 2'+1" | 61.4 s + 0.512 s | 1'+0.5" |
| 40/15 repeating | 40 moves in 7 min 41 s | 40/8 repeating, rounded up as testers do |

S belongs to this machine: another machine is benched again.

**Decided (the developer, 2026-11-10): 1'+0.5" is CCRL Blitz here.** The bench ran alone, while a
match runs 10 games at once, so each core is somewhat slower under that load and the control a
little shorter than CCRL's in real terms; accepted as a fair estimate, no loaded bench taken.

## Anchors for a rating estimate

Three engines from CCRL Blitz (list of 2026-11-07, single CPU), all GPLv3, their official Windows
builds from GitHub in `data/anchors/` (gitignored) and named in `data/engines.yaml`:

| Arena name | Engine | Build | CCRL Blitz | Games |
| --- | --- | --- | --- | --- |
| `velvet-3.0.0` | Velvet 3.0.0 | x86_64-avx2 | 3082 +/- 17 | 900 |
| `weiss-1.3` | Weiss 1.3 | popcnt (no AVX2 build) | 3137 +/- 15 | 1217 |
| `halogen-10` | Halogen 10 | x64-popcnt-avx2 | 3210 +/- 10 | 3007 |
| `velvet-3.2.0` | Velvet 3.2.0 | x86_64-avx2 | 3303 +/- 16 | 983 |

Velvet 3.2.0 added after Fattorini beat Weiss and Halogen; Igel 2.7.0 (3303 +/- 15, GPLv3, official
Windows builds) is the alternative at the same rating.

Chosen to span about 130 Elo before Fattorini's place on CCRL's scale is known; a short scout
against each shows whether the set must move. No `pext` builds: they are very slow on AMD before
Zen 3. Scouts go down from Weiss: if Fattorini also loses to Velvet, a weaker anchor is found
below 3071, where the list has not been read yet (the developer, 2026-11-10). The list was read only down to about 3071; no Stockfish older than 10 was found on it.

## Scouts

| Date | Anchor | Control | Games | Fattorini | Score | Result |
| --- | --- | --- | --- | --- | --- | --- |
| 2026-11-10 | `weiss-1.3` (3137) | 1'+0.5", Hash 128 | 82, stopped once decided | `lc0-psq-threats-screlu-g1-r1@tuned` | 45.5 - 36.5 (+34 -25 =23), 55.5% | `data/matches/weiss-1.3-vs-lc0-psq-threats-screlu-g1-r1@tuned/20261110-1018/` |

| 2026-11-10 | `halogen-10` (3210) | 1'+0.5", Hash 128 | 78, stopped once decided | `lc0-psq-threats-screlu-g1-r1@tuned` | 46.0 - 32.0 (+35 -21 =22), 59.0% | `data/matches/halogen-10-vs-lc0-psq-threats-screlu-g1-r1@tuned/20261110-1050/` |

Weiss: about +38 Elo, so about 3175. Halogen: about +63 Elo, so about 3273. Each has a 95% interval
of roughly +/- 65 Elo; together, about 3225 +/- 50. A match stopped once decided slightly flatters
its winner. Fattorini beat both anchors, so the set moves up: an anchor near 3300 next, and a full
match against one anchor to narrow the margin.

Velvet 3.2.0, scouted the same day: Velvet clearly stronger (score not recorded yet).

**Decided (the developer, 2026-11-10): three full matches of 400 games**, 1'+0.5", Hash 128,
`--full`, against Weiss 1.3, Halogen 10 and Velvet 3.2.0, which bracket Fattorini. The estimate:
each match's score turned into an Elo difference and added to the anchor's CCRL rating; the three
combined weighted by the inverse of their variance, each variance counting both the match's
margin and the anchor's own CCRL error.

## Full matches

| Date | Anchor | Games | Score for Fattorini | Elo vs anchor | Estimate | Record |
| --- | --- | --- | --- | --- | --- | --- |
| 2026-11-10 | `weiss-1.3` (3137 +/- 15) | 400, 116 min | 255.5 - 144.5 (+213 -102 =85), 63.9% | +99 +/- 31 | 3236 +/- 34 | `data/matches/weiss-1.3-vs-lc0-psq-threats-screlu-g1-r1@tuned/20261110-1148/` |
| 2026-11-10 | `halogen-10` (3210 +/- 10) | 400, 115 min | 218.0 - 182.0 (+154 -118 =128), 54.5% | +31 +/- 28 | 3241 +/- 30 | `data/matches/halogen-10-vs-lc0-psq-threats-screlu-g1-r1@tuned/20261110-1345/` |

Margins are 95%; the estimate's adds the anchor's CCRL error to the match's.

## What it is for

- **Not the Skill Level matches:** both sides share the machine, so they need no scaling.
- **The repeating-control check** (TODO): 40/15 scaled to the machine, Fattorini against itself,
  with Hash 256 and no options otherwise, as a tester would set.
- **An estimate of Fattorini's CCRL rating** before testers play it, against engines on the list,
  Blitz conditions scaled.
