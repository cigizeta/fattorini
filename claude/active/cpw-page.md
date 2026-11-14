# The Chess Programming Wiki page

**Status:** drafted 2026-11-11, not waiting for CCRL (the developer, 2026-11-11: the wiki page
should not wait that long). To post once a wiki account is approved (TODO.md, item 6). Facts
only, from the README and the release.

The author's name and chess work are public, as in the LICENSE (the developer, 2026-11-14,
reverting the earlier rule that kept them out of this file): both pages are drafted here. Nothing
beyond the chess work goes on either page.

**No rating, ever** (the developer, 2026-11-11): the page claims no strength, neither the
estimate nor a CCRL or Lichess rating. Links to the rating lists and the Lichess account are
fine; the numbers are theirs to show. To add once known: a link to the CCRL listing under External
Links. The release's announcement on TalkChess (ccrl-announcement.md), posted 2026-11-13, is
already under Forum Posts.

## The draft

Page title: `Fattorini`

```text
'''Fattorini''',<br/>
an [[:Category:Open Source|open source chess engine]] by [[Calogero Zarba]], written in [[Rust]] and licensed
under the [[Free Software Foundation|GNU AGPL v3]]. Fattorini is a [[UCI]] engine for
[[Windows]] x64, with an [[NNUE]] evaluation whose networks are trained on a limited compute
budget. Its first release, 2026.11.0, was published on GitHub in November 2026, as a single
executable with the network embedded <ref>[https://github.com/cigizeta/fattorini/releases/tag/v2026.11.0 Release Fattorini 2026.11.0]</ref>.
Fattorini plays on Lichess as ''fattorini-bot''.
The engine is named after Antonio Fattorini, of a family of Italian immigrants to Yorkshire, who
opened a shop in Harrogate in 1831; in 1883 the family firm made the first chess clock <ref>[https://en.wikipedia.org/wiki/Fattorini_%26_Sons Fattorini & Sons from Wikipedia]</ref>.

=Features=
==Evaluation==
The first release's network, ''lc0-psq-threats-screlu'' generation 1, has 4,876 inputs per side:
780 piece-square inputs (pieces on squares, castling rights, the en passant file) and 4,096
simplified [[Attacks|threat]] inputs, one for each square pair where a piece of the side attacks
an opponent's piece, without pins or x-rays. Each side's inputs feed 256 neurons, activated by
SCReLU, straight to the output, in integer arithmetic with [[AVX2]]. The piece-square part of
the accumulator is updated incrementally. The network was trained on positions from
[[Leela Chess Zero]]'s test91 training data <ref>[https://github.com/cigizeta/fattorini Fattorini on GitHub], README</ref>.

==Training==
The first release's network is trained on positions from [[Leela Chess Zero]]'s self-play games,
converted by Fattorini's own tools, each labelled with a dynamic blend of Leela's search score and the
game result.

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
* [[Pondering]]
* [[Syzygy Bases]], WDL and DTZ
* Search parameters tuned by [[SPSA]] for each network, after it is trained

=See also=
* [[Leela Chess Zero]]
* [[NNUE]]

=Forum Posts=
* [https://talkchess.com/viewtopic.php?p=999558#p999558 Fattorini 2026.11.0, a new UCI engine in Rust] by [[Calogero Zarba]], [[CCC]], November 13, 2026

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
[[Category:X64]]
[[Category:PC]]
[[Category:Windows]]
[[Category:NNUE]]
```

## The author page

Page title: `Calogero Zarba`, linked from the wiki's People list in its alphabetical place.

```text
'''Calogero Zarba''',<br/>
author of the [[:Category:Open Source|open source]] chess engine [[Fattorini]].

=Chess Projects=
* [[Fattorini]]

=External Links=
* [https://github.com/cigizeta cigizeta on GitHub]

'''[[People|Up one Level]]'''
[[Category:Programmer]]
```

## Before posting

- **The title:** `Fattorini`, as engine pages carry the bare name (no "Chess Engine" suffix) and
  person pages a full name, so no clash is expected (2026-11-11). Search the wiki for an existing
  `Fattorini` first; if there is one, a parenthesized qualifier such as `Fattorini (engine)`, as
  the wiki does for clashes, rather than a suffix.
- Links checked 2026-11-14, while the wiki showed itself as a temporary, read-only recovery:
  SCReLU, Lichess and Open Source Engines had no page, nor the Free Software Foundation page an
  AGPL section, so they are unlinked or retargeted as Black Marlin and Rustic do. Some engine pages
  were missing too, so check again once the wiki is fully back; relink Lichess if its page returns.
  Not yet checked: `Pondering` and `CCC`.
- Compare with a few recent engine pages for the wiki's current layout, then follow theirs.
- No rating or strength figure anywhere on the page, now or later (decided above).
- The author page: copy the categories of a recent programmer's page (`Category:Programmer` is
  unverified), and confirm the People list's title and place before adding the name there.
