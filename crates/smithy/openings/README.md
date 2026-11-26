# Openings snapshot

`a.tsv` to `e.tsv` are Lichess's chess-openings files (https://github.com/lichess-org/chess-openings),
released under CC0 1.0 (public domain). They are a snapshot: copied as they were and never
downloaded at build time; a refresh is a deliberate new snapshot, which changes the book's positions.

Snapshot: taken from aio64's copy, 2026-11-26.

The smithy embeds them (`include_str!`) as the positions of the UCI-judged book.
