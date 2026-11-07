# The engine becomes Fattorini

**Status:** steps 1 to 4 done 2026-11-07 on this machine (tests passed); steps 5 to 7 wait until every
machine is idle. The GitHub repository, `origin`
and the Drive root were already renamed.

## Decisions (the developer, 2026-11-07)

- **One name, everywhere:** crate and binary `fattorini`, `bin/fattorini`, library paths
  `fattorini::`, venv `.fattorini`, Drive root `G:/My Drive/fattorini`, variables `FATTORINI_*`, header
  `X-Fattorini-Token`, UCI `id name Fattorini`, snapshots `fattorini-<player>-...exe`, firewall rules
  `fattorini smithy`, `fattorini arena`. This supersedes the name of kalo-chess-repository.md.
- **The magic becomes `FATT`:** same length, so every existing network is patched in place, with
  no compatibility path in the reader. This reverses that document's decision to keep `KALO`.
- **Kept:** the file tags `CHFC`, `CHFL`, `CHTR`, which do not carry the name.
- **History stays:** `claude/` documents keep what they say, and so do `CHANGELOG`-like entries of
  releases already published, if `RELEASE-NOTES.md` has any (reviewed in step 3).

## Facts checked

- Nothing hashes a network's bytes, so patching the magic invalidates nothing.
- Every network file ends in `.nnue` (`teacher.nnue`, `candidate.nnue`, `data/networks/`).
- A snapshot `.exe` embeds both its network and its own reader, so existing snapshots keep working,
  old magic included, and lichess-bot's configured path only needs the new Drive root. Snapshots
  keep their old file names; `arena snapshot`'s cleanup of older ones will not match them.
- The Lichess account is a separate question: README.md links to it, and the `sed` below must not
  be allowed to change that link to an account that does not exist. Settled 2026-11-07: the
  account is `fattorini-bot`, and README.md links to it.

## Order

The new code refuses any `KALO` network, and old binaries refuse any `FATT` network. So every smithy,
arena and lichess-bot is stopped, and Drive fully synced, before step 5; every machine then updates
before anything starts again.

## Steps

1. The developer, the moves:

   ```
   git mv crates/kalo-chess crates/fattorini
   ```

   ```
   git mv bin/kalo-chess bin/fattorini
   ```

2. The developer, the text, outside `claude/`:

   ```
   git grep -lIiE 'kalo' -- ':!claude' | xargs sed -i -e 's/KALO_CHESS/FATTORINI/g' -e 's/b"KALO"/b"FATT"/g' -e 's/Kalo-Chess/Fattorini/g' -e 's/KaloChess/Fattorini/g' -e 's/Kalo Chess/Fattorini/g' -e 's/kalo-chess/fattorini/g' -e 's/kalo_chess/fattorini/g'
   ```

3. Claude, by hand: restore the Lichess link in README.md, the prose the `sed` leaves wrong
   (CLAUDE.md, README.md, DEV.md, RELEASE-NOTES.md, the bad-magic messages), the comment on `MAGIC`, and
   the TODO.md note. Add to DEV.md's firewall section the deletion of the old rules. A search for
   `kalo` must find only `claude/` and the Lichess link.
4. The developer, on this machine: recreate the venv as `.fattorini` (DEV.md), then tests and clippy,
   then commit. A new venv is needed because a venv records its own path.

   ```
   cargo test
   ```

   ```
   cargo clippy --all-targets
   ```

5. **When every machine is idle:** on any one machine, patch the networks on Drive; on every machine,
   patch the local `data/` too. Each file is checked first, so a run over patched files changes nothing:

   ```
   find "/g/My Drive/fattorini" data -type f -name '*.nnue' | while IFS= read -r file; do if [ "$(head -c 4 "$file")" = KALO ]; then printf FATT | dd of="$file" bs=1 conv=notrunc status=none; fi; done
   ```

   Then this must print nothing:

   ```
   find "/g/My Drive/fattorini" data -type f -name '*.nnue' | while IFS= read -r file; do [ "$(head -c 4 "$file")" = FATT ] || echo "$file"; done
   ```

   Found on 2026-11-07: two files on Drive, `lc0-psq-threats-screlu/run-1/gen-1/lambda-polyline/candidate.nnue`
   and `gen-2/teacher.nnue`, still carried the older 8-byte magic `TEMPERCH`, so no `KALO` build could
   have read them. They are rewritten as `FATT` plus the bytes after those eight, 4 bytes shorter.

6. On every machine, in a Git Bash run as administrator, replace the firewall rules:

   ```
   netsh advfirewall firewall delete rule name="kalo-chess smithy"
   ```

   ```
   netsh advfirewall firewall delete rule name="kalo-chess arena"
   ```

   ```
   netsh advfirewall firewall add rule name="fattorini smithy" dir=in action=allow protocol=TCP localport=47821 profile=private program="C:\dev\fattorini\target\release\smithy.exe"
   ```

   ```
   netsh advfirewall firewall add rule name="fattorini arena" dir=in action=allow protocol=TCP localport=47823 profile=private program="C:\dev\fattorini\target\release\arena.exe"
   ```

7. Each other machine: pull, rename the folder, recreate the venv, rebuild. lichess-bot's
   configuration follows the new Drive root.

## Verification

`cargo test` passes; `bin/smithy` starts with the new venv active; `bin/arena` sees the Drive store and
loads a patched network; `bin/fattorini` answers `uci` with `id name Fattorini`.
