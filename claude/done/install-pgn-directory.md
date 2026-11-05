# `rotor-train install` creates lichess-bot's PGN directory

The developer, 2026-10-06: the directory lichess-bot writes its games to (`pgn_directory` in its
`config.yml`) is created by `install`, never by hand. Found setting up the Lichess machine, where it did not
exist.

## The change

`src/train/mod.rs` only:

1. One helper, `create_dir_or_exit(dir)`, taking over the `create_dir_all` and its error exit that
   `run_install` and `run_install_hand_crafted` now each spell out for `bin/`.
2. Both installs also create `{data_root}/lichess/games` with it, beside `bin/`. `create_dir_all`
   leaves an existing directory and its games untouched.
3. `run_install`'s printed `config.yml` lines gain the top-level
   `pgn_directory: "{data_root}/lichess/games"`, so the path is named in one place.
4. The doc comment of `run_install` says it also creates the games directory.

## Verify

```
bin/rotor-train install amanda
```

It prints the `pgn_directory` line, and `$LOCALAPPDATA/rotor/lichess/games` exists.
