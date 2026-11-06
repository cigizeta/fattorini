# The rename to kalo-chess: implementation

**Status:** planned 2026-11-06, not started. Design: kalo-chess-rename.md.

The developer runs every command (Claude has no Bash); Claude does the hand edits of step 3 and
the review of step 4. Every command from the repository root, in Git Bash.

## Step 0: before anything

- **Steps 1 to 4 may run beside PeSTO's self-play (2026-11-06):** they change sources and build
  only into `target/debug`, never the running release arena. Its games keep the old PGN event,
  which nothing parses.
- **Before step 5:** PeSTO's self-play finished (`complete.txt`) or stopped from the page, every
  smithy and arena stopped on every machine; then `cargo clean -p temper-chess` and
  `cargo clean --release -p temper-chess` on each (a running `.exe` is locked on Windows), so no
  binary of the old name stays in `target/`.
- The training machine's networks listed (the same `find` as on 2026-11-06): only
  `lc0-psq-threats-screlu`'s expected.

## Step 1: the moves (the developer)

```
git mv crates/temper-chess crates/kalo-chess
```

```
git mv bin/temper-chess bin/kalo-chess
```

## Step 2: the text (the developer)

One `sed` over every tracked file outside `claude/` that names Temper (`temper[^a]` leaves
"temperature" alone). The rules run in order, the specific ones before the bare word:

```
git grep -lIiE 'temper([^a]|$)' -- ':!claude' | xargs sed -i -E -e 's/temper-chess/kalo-chess/g' -e 's/temper_chess/kalo_chess/g' -e 's/TEMPER_CHESS_/KALO_CHESS_/g' -e 's/Temper Chess/Kalo Chess/g' -e 's/Temper Go/Kalo Go/g' -e 's/Temper smithy/Kalo Games smithy/g' -e 's/TemperPlayer/KaloChessPlayer/g' -e 's/\bTemper\b/Kalo Chess/g'
```

What it covers: the crate's name and paths (`Cargo.toml`s, `Cargo.lock`, `bin/`, `.cargo/`,
`.gitattributes`), `temper_chess::`, `TEMPER_CHESS_EVALFILE`, `_SELF_PLAYER`, `_SELF_SECONDS`, the
installed `temper-chess.*` names, the Lichess mutex `Local\temper-chess-lichess-group-<n>`, the PGN
events, the UCI `id name`, the page's title, `TemperPlayer`, and the bare "Temper" of docs and
comments (about 150).

## Step 3: by hand (Claude)

- **The magic** (`crates/kalo-chess/src/nnue.rs`): `pub const MAGIC: &[u8; 4] = b"KALO";`. Its
  message "not a Kalo Chess network file" comes from the `sed`.
- **The header's offsets in tests**, shifted by 4 (the version now at 4..8, the feature set at
  8..12): `bytes[8..12]` to `bytes[4..8]` first, then `bytes[12..16]` to `bytes[8..12]`, in
  `nnue.rs` (about 20 tests) and `crates/smithy/src/export.rs` (8). `book.rs` and `lc0/record.rs`
  read other formats and are left alone.
- **What the `sed` worded badly:** CLAUDE.md's opening (Kalo Games the project, Kalo Chess its
  engine, Kalo Go future work; Licensing names the project, not the engine), README's and DEV.md's
  headings, "Kalo Chess's", any sentence where "Kalo Games" reads better.
- **`claude/TODO.md`:** a note at the top, as for each earlier rename.

**Done 2026-11-06.** What the `sed` broke, fixed by hand: the HTTP header `X-Temper-Token`, turned
into `X-Kalo Chess-Token` (invalid), is now `X-Kalo-Games-Token` in `lan-http` and the page; a few
comments joined by hyphens reworded; three more header offsets in tests (`bytes[20..24]` to
`bytes[16..20]`). The smithies' header changes too, so every machine switches together (step 5).

## Step 4: check (the developer, then Claude)

Must print nothing:

```
git grep -nIiE 'temper([^a]|$)' -- ':!claude'
```

```
cargo test --workspace
```

```
cargo clippy --workspace --all-targets
```

Claude reviews `git diff --stat` and the hand edits; the developer commits:

```
git commit -am "The engine is Kalo Chess: crate, binary, variables, magic KALO, docs"
```

## Step 5: the switch, every machine

1. Pull, then build through the wrappers (`bin/kalo-chess`, `bin/arena`, `bin/smithy`).
2. **The networks, once, on Drive** (and on the training machine's `data/`, whose copies would
   otherwise be read first). Each file into a new copy, checked, then put in place:

   ```
   find "/g/My Drive/kalo-games" data -name '*.nnue' -exec sh -c 'test "$(head -c 8 "$1")" = TEMPERCH && { printf KALO; tail -c +9 "$1"; } > "$1.new" && mv "$1.new" "$1" && echo "converted $1"' _ {} \;
   ```

   Then the listing again, every file now starting `KALO`:

   ```
   find "/g/My Drive/kalo-games" data -name '*.nnue' -exec sh -c 'printf "%s  %s\n" "$(head -c 4 "$1")" "$1"' _ {} \;
   ```

3. `bin/arena` and `bin/smithy` start; the page shows run 1's networks; a quick duel or `bin/kalo-chess
   bench` loads one.
4. On the Lichess machine: an install from the page, lichess-bot's configuration pointed at
   `kalo-chess.exe` and at the account `kalo-chess` (made a bot first, its token in that
   configuration alone), `temper-chess` closed.
5. Any gitignored local file naming `temper-chess` (as `data/chess/engines.yaml`) fixed by hand.

## Done when

The step 4 search prints nothing, tests and clippy pass, both networks load on the new build, and
`kalo-chess` plays its first Lichess game. Then this document and kalo-chess-rename.md go to
`done/`, with the earlier rename entries of `TODO.md`'s Next section.
