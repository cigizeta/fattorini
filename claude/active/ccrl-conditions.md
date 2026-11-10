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

## What it is for

- **Not the Skill Level matches:** both sides share the machine, so they need no scaling.
- **The repeating-control check** (TODO): 40/15 scaled to the machine, Fattorini against itself,
  with Hash 256 and no options otherwise, as a tester would set.
- **An estimate of Fattorini's CCRL rating** before testers play it, against engines on the list,
  Blitz conditions scaled.
