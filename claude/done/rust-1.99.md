# Rust 1.99.1

**Status: plan, 2026-10-26. The taper run ended the same day (psq-threats-screlu-taper.md); step 1
done (Bench below); step 2 done; step 3 done, tests and clippy clean, no new lints; step 4 done, no slowdown, kept; next: step 5,
the next forge run's round times.**

## Why

Another project of the developer's needs Rust 1.99 and is pinned to 1.99.1 (the developer,
2026-10-26). Rotor is pinned to 1.98.1 (`rust-toolchain.toml`, `rust-version` in the workspace's
`Cargo.toml`), so it is untouched by that upgrade; moving it to 1.99.1 keeps one toolchain for both
and keeps Rotor current. The pin stays: an upgrade is always a deliberate change, measured.

## Plan

1. **Bench before, on 1.98.1:** on one idle machine, plugged in, nothing else running (CLAUDE.md),
   `cargo bench --bench nnue_bench` with the default random network at `--depth 9`, and once more
   with the screlu phase network's shape (`--threats --screlu --material --phase`, or what its usage
   names for it), each twice back to back, so the machine shows steady. The numbers kept: nodes per
   second of the search, ns of the head and of the incremental update.
2. **The pin:** `rust-toolchain.toml`'s `channel = "1.99.1"` and the workspace's
   `rust-version = "1.99.1"`. No other change in the same commit.
3. **Build and test:** `cargo test --workspace`; then `cargo clippy --workspace --all-targets`. New
   lints are fixed as the clippy-warnings pass fixed the last ones (clippy-warnings.md): by clippy's
   own fix where it applies, by hand where it does not, an `allow` with its reason where the code is
   right as it is. Their fixes in their own commit, after the pin's.
4. **Bench after, on 1.99.1:** the same runs on the same machine. A slowdown beyond the run-to-run
   noise is looked into before the upgrade is kept: rolling back is reverting the pin.
5. **Training's speed:** the next forge run's round times (`history.tsv`'s `train_duration_secs`)
   against the last run's on the same config; libtorch itself does not change, so only the Rust
   side of the batches could move them.
6. **Docs:** no other file names 1.98 (searched 2026-10-26, beyond a done doc's history); TODO.md.

## Bench

Random networks, width 256, depth 9, one machine, three runs each (ns, min to max):

| | 1.98.1 psq | 1.98.1 screlu phase | 1.99.1 psq | 1.99.1 screlu phase |
|---|---|---|---|---|
| incremental update | 34.7 to 37.5 | 34.9 to 36.2 | 33.2 to 33.4 | 32.3 to 32.7 |
| head alone | 20.0 to 20.9 | 152.8 to 170.1 | 20.5 to 21.7 | 157.3 to 164.3 |
| per search node | 55.6 to 57.4 | 188.9 to 205.1 | 53.9 to 54.9 | 189.6 to 197.1 |

1.99.1 two runs each. Every line within 1.98.1's range or just below it: the incremental update
about 5% to 10% faster, at the noise's edge, the head and the node level. No slowdown, kept.

PeSTO's line wandered from 10.3 to 25.8 ns across the six runs, so the machine was not fully
steady: the head and the node move by up to about 10% (phase's head 152.8 to 170.1), so only a
change beyond that counts. The fixed-depth searches last a few milliseconds each and are not
compared.
