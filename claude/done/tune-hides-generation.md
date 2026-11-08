# A tune's local folder hides its generation

**Status:** fixed in code 2026-11-08, awaiting the developer's check.

## The bug

After the lc0 network's tune ended, `lc0-psq-threats-screlu` vanished from the Tests form: no
network of it could be chosen, tuned or not.

1. Generation 1 of the run lives on the remote alone (closed generations move there).
2. The tune works in `gen-1/tune/` locally, which creates a local `gen-1/`. At its end
   `hand_over` moves `tune/` to the remote, but at every smithy start the page's last job is
   rebuilt (`last_job`), and `Tune::open` fetches the tune back from the remote to show it.
3. `readable_dirs` read a generation locally whenever its local folder existed. The local `gen-1/`
   held only `tune/`, no `lambda-*` folder, so the generation counted as untrained: no networks
   in the catalog, and the config gone from the form.

Removing the folder by hand lasts only until the next smithy start.

## The fix

`readable_dirs` reads the remote's generation when the local one has no arm (`lambda-*` folder)
**and** the remote's has one; otherwise the local one, as before. A running generation, local and
without arms during its self-play, has none on the remote either, so it is still read locally.

The rule is a pure function of the two `Dirs` (`readable_of`), tested on scratch folders: an
arm-less local folder beside a remote with arms reads the remote; a local folder with arms, or an
arm-less one with no remote copy, reads the local; no local folder reads the remote.

## Check (the developer)

```
cargo test -p smithy readable
```

```
cargo clippy --workspace --all-targets
```

Then the smithy restarted: `lc0-psq-threats-screlu` offered on the Tests form, generation 1 with
"its tune".
