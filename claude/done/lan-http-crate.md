# The HTTP code in a crate of its own, out of the engine's dependencies

Status: implemented 2026-10-25, steps 1 to 5 (step 6 found no doc to change outside `claude/`);
`pipeline/http.rs` deleted by the developer. Checks passed the same day: the four crates' tests,
the bench's 3,117,647 nodes, and `Cargo.lock` listing no `tiny_http` or `ureq` under `rotor`. Done.

## Why

`rotor::pipeline::http` is the only reason the `rotor` crate depends on `tiny_http` and `ureq`, and
through `ureq` on `url`, `idna` and 15 ICU crates. The engine never calls it: a search of the built
`rotor.exe` (2026-10-25) found none of its strings, the linker having dropped it. But the release's
notices are generated from the dependency graph (`cargo-about`, TODO's "AGPLv3 for a binary"), which
would list all of them. Moved into a crate that only `arena` and `smithy` depend on, the engine's
graph loses about 25 crates and says what it ships. The same reason `progress` is its own crate
(`indicatif` never links into the engine).

## Decisions

1. **A new workspace crate, `crates/lan-http`, library `lan_http`.** Not `http`, which is a
   crates.io crate's name (`ureq` 3 depends on it), so a later dependency would collide.
2. **What moves:** `pipeline/http.rs` whole, with its tests, as the crate's `lib.rs`; and
   `protocol::control_request`, its only user inside `rotor`, the local status and stop both
   binaries send. `lan_http` depends on `rotor` for the protocol's constants it needs (`LOOPBACK`,
   `STOP_PATH`), as `arena` and `smithy` already do; `rotor` depends on nothing of it.
3. **What stays in `rotor::pipeline::protocol`:** everything else, the ports, paths, tags and the
   messages' text forms, which hold no HTTP and which the protocol's two sides share.

## Steps

1. **`crates/lan-http/Cargo.toml`**, as `progress`'s: workspace package fields, a comment saying
   why it is its own crate (above); dependencies `rotor`, `tiny_http`, `ureq` (workspace), each
   with the comment it has in `rotor`'s `Cargo.toml` today; `[lints] workspace = true`;
   `[lib] bench = false`.
2. **`crates/lan-http/src/lib.rs`:** `http.rs`'s text, its module comment adjusted (the protocol
   is `rotor::pipeline::protocol`); then `control_request`, using `lan_http`'s own `url`, `get`,
   `post` and `status_of` and `rotor::pipeline::protocol::{LOOPBACK, STOP_PATH}`.
3. **`rotor`:** delete `pipeline/http.rs` and `pub mod http;` in `pipeline/mod.rs`; delete
   `control_request` from `protocol.rs`, and its `Duration` import if nothing else uses it; the
   module comment's `http.rs` and `http::TOKEN_HEADER` become `lan_http` and
   `lan_http::TOKEN_HEADER`; remove `tiny_http` and `ureq` with their comment from
   `crates/rotor/Cargo.toml`.
4. **Workspace `Cargo.toml`:** `lan-http = { path = "crates/lan-http" }` beside `progress`.
5. **`arena` and `smithy`:** `lan-http.workspace = true` in each `Cargo.toml`;
   `rotor::pipeline::http` becomes `lan_http` throughout (`arena/src/chores.rs`,
   `smithy/src/smithy.rs`, its tests included), and `protocol::control_request` becomes
   `lan_http::control_request`.
6. **Docs:** DEV.md and any comment naming `pipeline/http.rs` or `rotor::pipeline::http` (a search
   then).

## Checks

- `Cargo.lock`: `rotor`'s dependencies no longer list `tiny_http` or `ureq`.
- `cargo test --release` for `rotor`, `lan-http`, `arena` and `smithy`, after the phase run's
  training ends (smithy's tests build against libtorch, and the build competes with training).
- `bin/rotor bench`: 3,117,647 nodes, as before.
- Behaviour unchanged, the code only moved: no smithy or arena run needed beyond the tests, which
  cover the claim, the work and the control over real loopback listeners.
