# The CCRL announcement

**Status:** drafted 2026-11-11, to post once the board account is approved (TODO.md, item 5).

Where: the CCRL Discussion Board, forum CCRL Public, **a new topic** (Graham Banks, in the pinned
"How to get your engine tested for CCRL": "To authors of new engines. Please create a new post to
announce your engine details."). The first two posts of a new account are approved before they
appear.

## What the pinned guide asks

A download link, plus:

- name of the engine plus the version number;
- estimated strength;
- the author's name and nationality;
- whether it is open source;
- homepage, if any;
- if closed source, credit where it's due;
- coded on one's own or AI assisted.

Also: exe files; the testers all use AVX2; helpful but not necessary, default settings of 256 MB
hash and 1 core. Later posts in that thread: the version in both the file name and the UCI id, and
no separate network file or folder to set up. Fattorini 2026.11.0 meets both.

## The draft

The author's name and nationality are public (the developer, 2026-11-11: "It is not a secret"),
the name already in the README's copyright line. The code is declared AI assisted (the developer,
2026-11-11).

**Subject:** Fattorini 2026.11.0, a new UCI engine in Rust

    Hello,

    I'd like to submit my engine for testing.

    - Engine: Fattorini 2026.11.0, a UCI engine written in Rust. It is named after Antonio
      Fattorini, an Italian immigrant to Yorkshire whose family's business made the first chess
      clock in 1883 (https://en.wikipedia.org/wiki/Fattorini_%26_Sons).
    - Download: https://github.com/cigizeta/fattorini/releases/tag/v2026.11.0, one Windows x64
      .exe with the network embedded and no other files. It needs AVX2 (x86-64-v3). The version
      is in both the file name and the UCI id name.
    - Estimated strength: about 3250 on CCRL Blitz, one CPU. That comes from 400-game matches
      against Weiss 1.3, Halogen 10 and Velvet 3.2.0 at 2'+1" scaled to my machine by
      Stockfish 10's bench (1'+0.5"), with 128 MB hash and 6-piece Syzygy tablebases. The full
      method is in the README.
    - Author: Calogero Zarba, Italy.
    - Open source: yes, AGPLv3. The homepage is https://github.com/cigizeta/fattorini.
    - Credits: the evaluation network was trained on positions from Leela Chess Zero's test91
      training data (ODbL), converted by Fattorini's own code. The search and the training
      pipeline are Fattorini's own, not taken from another engine.
    - Coding: AI assisted.

    The default hash is 16 MB and the default is 1 thread. Both can be set through the usual
    Hash and Threads options, along with SyzygyPath for tablebases. It has no own book and
    pondering is off by default.

    Thank you for your time and your testing.

    Calogero Zarba

## TalkChess, first (the developer, 2026-11-13)

**Posted 2026-11-13,** with its own subject, in BBCode (`[list]`, `[url=...]`), each paragraph
one line: phpBB keeps every line break and reads no Markdown, so the indented text below, pasted
as it is, previewed badly.

The CCRL account still waits for activation; TalkChess is announced first, **as a reply in the
one dedicated topic, "New engine releases 2026 H2"**
(https://talkchess.com/viewtopic.php?t=86481), not a topic of its own; the subject line below
serves as the reply's title. The same facts as the CCRL post, for engine authors and enthusiasts rather than
testers. The Lichess rating is left out: which build played the past week's games is unchecked
(reckless-only.md).

**Subject:** Fattorini 2026.11.0, a new UCI engine in Rust

    Hello everyone,

    I'd like to announce Fattorini, a new open-source UCI engine written in Rust. The first
    release, 2026.11.0, is out.

    It is named after Antonio Fattorini, an Italian immigrant to Yorkshire whose family's
    business made the first chess clock in 1883
    (https://en.wikipedia.org/wiki/Fattorini_%26_Sons).

    - Download: https://github.com/cigizeta/fattorini/releases/tag/v2026.11.0, one Windows x64
      .exe with the network embedded and no other files. It needs AVX2 (x86-64-v3).
    - Source: https://github.com/cigizeta/fattorini, AGPLv3.
    - Search: alpha-beta with the usual pruning and reductions, Lazy SMP, Syzygy tablebases at
      the root and in search, pondering.
    - Evaluation: an NNUE with piece-square inputs and simplified threat features, SCReLU,
      trained on positions from Leela Chess Zero's test91 training data (ODbL), converted by
      Fattorini's own code. The project's aim is networks trained on a limited compute budget.
    - Estimated strength: about 3250 on CCRL Blitz, one CPU, from 400-game matches against
      Weiss 1.3, Halogen 10 and Velvet 3.2.0 at a scaled 2'+1". The method is in the README.
    - Author: Calogero Zarba, Italy. The code is AI assisted.

    Options: Hash (16 MB by default), Threads (1), MultiPV, Ponder, SyzygyPath and Move
    Overhead. No own book.

    Feedback and test results are very welcome.

    Calogero Zarba

## Decided

- **2026.11.0 is submitted as drafted (the developer, 2026-11-13),** though Reckless's time
  management has since replaced Classic on `bartolomeo` (reckless-only.md): the post describes
  the released build, credits included. A later version's announcement credits Reckless's time
  management.

- **The default hash stays 16 MB** (the developer, 2026-11-11): the pinned guide's 256 MB is only
  "helpful (but not necessary)", and testers set `Hash` themselves, CCRL's conditions fixing it for
  every engine.
