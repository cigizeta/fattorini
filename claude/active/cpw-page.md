# The Chess Programming Wiki page

**Status:** drafted 2026-11-11, not waiting for CCRL (the developer, 2026-11-11: the wiki page
should not wait that long). To post once a wiki account is approved (TODO.md, item 6). Facts
only, from the README and the release.

The author's name is the developer's to fill in on the wiki, never in this file: `[author]` below.
The author page beside it (the name, the GitHub profile, other public chess projects) is written
by the developer on the wiki alone.

**No rating, ever** (the developer, 2026-11-11): the page claims no strength, neither the
estimate nor a CCRL or Lichess rating. Links to the rating lists and the Lichess account are
fine; the numbers are theirs to show. To add once known: a link to the CCRL listing under External
Links; the forum topic of the CCRL announcement (ccrl-announcement.md) under Forum Posts.

## The draft

Page title: `Fattorini`

```text
'''Fattorini''',<br/>
an [[Open Source Engines|open source chess engine]] by [[author]], written in [[Rust]] and licensed
under the [[Free Software Foundation#AGPL|GNU AGPL v3]]. Fattorini is a [[UCI]] engine for
[[Windows]] x64, with an [[NNUE]] evaluation whose networks are trained on a limited compute
budget. Its first release, 2026.11.0, was published on GitHub on November 11, 2026, as a single
executable with the network embedded <ref>[https://github.com/cigizeta/fattorini/releases/tag/v2026.11.0 Release Fattorini 2026.11.0]</ref>.
Fattorini plays on [[Lichess]] as ''fattorini-bot''.

=Features=
==Evaluation==
The first release's network, ''lc0-psq-threats-screlu'' generation 1, has 4,876 inputs per side:
780 piece-square inputs (pieces on squares, castling rights, the en passant file) and 4,096
simplified [[Attacks|threat]] inputs, one for each square pair where a piece of the side attacks
an opponent's piece, without pins or x-rays. Each side's inputs feed 256 neurons, activated by
[[SCReLU]], straight to the output, in integer arithmetic with [[AVX2]]. The piece-square part of
the accumulator is updated incrementally. The network was trained on positions from
[[Leela Chess Zero]]'s test91 training data <ref>[https://github.com/cigizeta/fattorini Fattorini on GitHub], README</ref>.

==Training==
Networks are trained one generation at a time: a teacher, at first the [[PeSTO]] evaluation, plays
games against itself, its searches label the positions, and a new network replaces the teacher
only if it beats it over the board. Along a second route, the first generation trains on Leela
Chess Zero's published games. Each label blends the teacher's search score with the game result;
the weight of the result, lambda, is not chosen but measured from the teacher's own games in ten
stages of the game, as the inverse-noise weighting of the two.

==Search==
* [[Alpha-Beta]] with [[Iterative Deepening]] and [[Aspiration Windows]]
* [[Transposition Table]]
* [[Lazy SMP]]
* [[Null Move Pruning]]
* [[Late Move Reductions]]
* [[Futility Pruning]]
* [[Razoring]]
* [[Static Exchange Evaluation|SEE]] pruning
* [[Killer Heuristic]], [[History Heuristic]]
* [[Singular Extensions]]
* [[Quiescence Search]]
* [[Syzygy Bases]], WDL and DTZ
* Search parameters tuned by [[SPSA]] together with the network

=See also=
* [[Leela Chess Zero]]
* [[NNUE]]

=Forum Posts=

=External Links=
* [https://github.com/cigizeta/fattorini Fattorini on GitHub]
* [https://github.com/cigizeta/fattorini/releases Releases]
* [https://lichess.org/@/fattorini-bot fattorini-bot on Lichess]

=References=
<references />

'''[[Engines|Up one Level]]'''
[[Category:UCI]]
[[Category:Open Source]]
[[Category:Rust]]
[[Category:Windows]]
[[Category:NNUE]]
```

## Before posting

- Check each `[[...]]` link against the wiki's existing pages, and drop or rename any that do not
  exist there (SCReLU, the AGPL anchor, the Rust category may not).
- Compare with a few recent engine pages for the wiki's current layout, then follow theirs.
- No rating or strength figure anywhere on the page, now or later (decided above).
