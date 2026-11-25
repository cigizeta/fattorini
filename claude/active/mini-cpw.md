# A mini CPW: Fattorini's techniques on GitHub Pages

**Status:** design agreed 2026-11-25 (the developer); implementation plan next, no pages yet.
Revives the dropped companion book (companion-book.md, archived), whose scope, conventions and table
of contents it takes over, as a site instead of a Quarto book.

## Why

The README describes what Fattorini is; a site repeating it would be a second copy to keep in step
(the developer, 2026-11-25: no duplicate of the README). A site of results alone has too little
material yet. What justifies one: a small Chess Programming Wiki restricted to the techniques
Fattorini uses. The real CPW is effectively dead (read-only since 2026-11-17), so such pages are
useful to others, and pages others link to help Fattorini be found. A "play with me" page was
considered and dropped: in the browser it needs a WebAssembly build beside the Windows-only rule,
and the Lichess bot is seldom online, so a challenge button would mostly fail.

## Decisions (the developer, 2026-11-25)

- **Techniques only, described with pseudocode at best** (the developer: "a mini CPW, restricted to
  the techniques used by Fattorini"). No Rust, no links into the code, no measurements, Elo or test
  results: those stay in the repository. As companion-book.md's decisions 1, 3 and 4.
- **Scope: what Fattorini uses today.** A technique gets a page if the engine or its training
  pipeline uses it; techniques it does not use are left out, however famous, and so is work
  written but not in use (self-play while paused, the transformer track).
- **For anyone interested in chess programming:** each page starts in plain terms, readable by a
  newcomer, then gives the idea precisely (a formula or a few lines of pseudocode).
- **Generic, not tied to any release:** the site describes the techniques as Fattorini uses them,
  updated when there is time; published from `main`, so a merge updates it. Each page shows when it
  was last revised.
- **Hand-written HTML**, no generator: served by GitHub Pages from `docs/` on `main`, no build step,
  no Action. Navigation, one shared stylesheet and every page written and kept by hand.
- **One page per technique**, as CPW, grouped by topic and linking to each other, so each can be
  linked to from outside.
- **Plain text:** text, formulas and pseudocode, in the logo's style, light and dark; no diagrams,
  boards or scripts.
- **Launched with four topics:** the network; training on Lc0's data; search; tuning and time.

## Conventions (from companion-book.md)

- **Every page the same shape:** what it is (one or two sentences), why it works, the idea itself
  (a formula or a few lines of pseudocode), its parameters and pitfalls. One small example at
  most, where the idea is otherwise hard to see.
- **No references or citations** (the developer, 2026-11-25): no source can claim to be first any
  more; what matters is the content. Unlike companion-book.md's "where it comes from".
- **Pseudocode language-neutral**, short, only where it says more than a formula or a sentence.
- **Notation fixed once**, on a page of its own: depth, ply, alpha and beta, side to move,
  centipawns, and the two ranges of a game's outcome (the developer, 2026-11-25):
  **expected score**, from 0 to 1, draws counted as half (Elo's term; never "winning
  percentage", which reads as wins only); **expected value**, from -1 to 1, a game counted +1, 0, -1
  (the developer: a word, not Lc0's letter q; it equals the chance of winning minus the chance of
  losing), with `expected value = 2 * expected score - 1`. Never "score" alone for either:
  centipawns are a score too.
- **No em or en dashes** (CLAUDE.md).
- **Nothing private:** CLAUDE.md's rules apply as for any committed file. No history of the project
  and no former names.

## Publishing (GitHub Pages)

- `docs/` on `main`: `index.html`, `style.css`, one folder per topic, `.nojekyll` (served as is).
  A file's path under `docs/` is its address under the site's root.
- Relative links only (the site lives under the repository's name, not at the domain's root).
- Turned on once, after the first page reaches `main`: Settings, Pages, "Deploy from a branch",
  `main`, `/docs`. GitHub's own `pages-build-deployment` republishes on each push to `main`.
- Previewed locally by opening `docs/index.html` in a browser.

## Open, for the plan

- The page list: companion-book.md's table of contents, parts 2 to 5 for the four launch topics,
  checked against today's code (the budget schedule and the polyline replaced patience and the
  lambda grid; Reckless's time management replaced Classic; halfk, king buckets and pairwise
  joined the network). Which pages the launch needs.
- The home page: a one-paragraph description and the topics, linking to the README for the rest.
- The style: the logo's palette and the README's badge colours, one stylesheet.
