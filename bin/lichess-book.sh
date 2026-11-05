#!/usr/bin/env bash
# Downloads arena's opening book into the data root's chess/books/ directory, regardless of where this
# script is invoked from. The data root holds the third-party downloads development and deployment
# share: %LOCALAPPDATA%\temper-games. The same rule is in
# src/pipeline/data_root.rs, and the two must agree.
set -euo pipefail

DATA_ROOT="$(cygpath -u "$LOCALAPPDATA")/temper-games"
BOOKS="$DATA_ROOT/chess/books"

# UHO_Lichess_4852_v1: Unbalanced Human Openings, positions from real Lichess games in which one side
# already has a clear edge, so that close engines play decisive games rather than draws; with colours
# reversed per opening, neither engine profits from the edge. The book the Stockfish project itself
# uses for its engine-strength testing. Source: https://github.com/official-stockfish/books
# (CC0-1.0).
URL="https://github.com/official-stockfish/books/raw/refs/heads/master/UHO_Lichess_4852_v1.epd.zip"
ARCHIVE="$BOOKS/UHO_Lichess_4852_v1.epd.zip"

mkdir -p "$BOOKS"
curl -fL -o "$ARCHIVE" "$URL"
unzip -o "$ARCHIVE" -d "$BOOKS"
rm "$ARCHIVE"
