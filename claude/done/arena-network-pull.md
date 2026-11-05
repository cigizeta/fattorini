# rotor-arena pulls promoted networks from Drive

**Done:** a network player (`amanda-NNNN`) whose generation lacks `champion.txt` locally is filled in from Drive (`match_player.rs`): only `champion.nnue` and `champion.txt`, and only after checking Drive is mounted.

## Problem

`rotor-arena match`, `sprt` and `duel` resolve a network player (`amanda-0001`) from the local
`data/networks/` only (`match_player.rs::network_path`). A machine whose local generation directory
is incomplete refuses the player even though Drive holds the promotion: seen with
`data/networks/amanda/gen-0001/`, which held `champion.nnue` locally but not `champion.txt`, so the
arena reported "generation 1 has no champion and its ladder has named no winner yet".

## Decisions

- `data/` stays the working copy. Drive (`remote.rs`, `/mnt/g/My Drive/rotor/data/`) is the
  source of truth only for what it already owns.
- `results.db` and everything under `data/matches/` stay local to each machine, never on Drive.
- For networks, Drive holds only what defines a promoted generation: `champion.nnue`,
  `champion.txt` and `champion.vs`. Candidates, checkpoints and ladder folders stay on the machine
  that produced them.
- The arena pulls only what it needs to play: `champion.nnue` and `champion.txt`, never
  `champion.vs`.
- Every time Drive is needed, it must be mounted: the arena checks right before it reads Drive,
  and panics with `remote::check_mounted`'s message if it isn't. A run that never needs Drive
  (`hand-crafted` against Stockfish, or a network whose promotion is already complete locally)
  never checks.

Consequences:

- `<network>-NNNN` works on any machine once the generation is promoted on Drive.
- `<network>-NNNN-<lambda>` naming a candidate works only on the training machine, since
  candidates never reach Drive. It still works everywhere when that lambda is the champion's.

## Implementation plan

### Step 1 -- pulling a promotion

In `match_player.rs::network_path`, before `promoted` is computed: if the local generation directory
lacks `champion.txt`, Drive is needed, so first check it's mounted, panicking if not:

```rust
if let Err(error) = remote::check_mounted() {
    panic!("{error}");
}
```

then pull the promotion from Drive:

```rust
remote::copy_down(&generation_dir, &generation_dir, &|name| name == "champion.nnue" || name == "champion.txt")
```

- `copy_down` already copies through `.partial` and a rename, skips a file present with the same
  size, and refuses one present with another size. That refusal becomes `network_path`'s `Err`,
  naming both paths: a local `champion.nnue` that disagrees with Drive's is reported, never
  replaced.
- If Drive holds no promotion either, nothing is copied and resolution goes on as today (a ladder
  winner, or today's error).
- `champion.txt` is copied after `champion.nnue` only because `copy_down` copies in name order
  (`champion.nnue` sorts first), so a `champion.txt` never exists locally without its network. The
  code says so where the order is relied on.

`generation_dir` there is the local path (`data/networks/<network>/gen-NNNN`), which is what
`remote_path` expects. `remote` is already a module of `rotor-arena` (`#[allow(dead_code)]`
today); the attribute stays, since the arena still uses only part of it.

### Tests

`network_path` takes the local network directory already; it now also takes the network's Drive
directory and the Drive root, which `resolve_player` fills in from `remote::remote_path` and
`remote::ROOT` (a small `Drive` struct), and the tests with temporary directories. `remote.rs`'s
`copy_files(source, target, accept)` and `check_mounted_under(root)` become public for that.

- A local generation with only `champion.nnue`, and a remote one with `champion.nnue` (same size)
  and `champion.txt`: `network_path` returns the champion, and `champion.txt` is now local.
- A local `champion.nnue` of another size than the remote's: `network_path` is an `Err` naming both,
  and the local file is untouched.
- No promotion on either side and a ladder winner locally: the winner's candidate, as today.
- A local promotion already complete: nothing is read from the remote, and the mount isn't
  checked (the remote root can be missing).
- A local generation lacking `champion.txt` with the remote root's parent missing: panics with the
  "not there" message (`#[should_panic]`). The mount check, like the copy, takes the root, through
  `remote`'s existing `check_mounted_under`, made test-visible.

### Documentation

- `match_player.rs`'s module doc: `<network>-NNNN` is pulled from Drive when the local generation
  lacks its promotion.
- `README.md`'s Google Drive section: what Drive holds for networks (the three champion files),
  and that `data/matches/` stays local.

### Verification

```
cargo test --release --bin rotor-arena match_player
```

Then, with the incomplete local `gen-0001/`:

```
bin/rotor-arena duel hand-crafted amanda-0001 --time 3+2
```

should start, and `data/networks/amanda/gen-0001/champion.txt` should now exist locally.
