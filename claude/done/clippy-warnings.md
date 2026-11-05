# Clippy's warnings addressed

**Status: planned 2026-10-26 (the developer: "Please, address the warnings"). Step 1 run the same
day: `--fix` applied to smithy and arena, not to rotor (its fixes rolled back, 48 warnings left);
step 2 done by hand on the 96 warnings left; step 3 the same day: clippy clean in every crate,
the tests passing, the bench's nodes unchanged. Done.**

**As applied, where it differs from the decisions below:** `.min(INFINITY)` was a no-op (`INFINITY`
is `i32::MAX`), so the two aspiration bounds are `saturating_add`, the clamp they meant, the same
value wherever nothing overflows. `PonderState` and `SyzygyTablebase` are allowed (a state already
pondering is no neutral default; the tablebase's `new` carries an unsafe contract); `Engine` got its
`Default`. One `if` in the quiescence's move cap stays nested and allowed: its comment says it
mirrors Stockfish's nesting on purpose. Float match guards (`score if score == 1.0`) became float
literal patterns, the same comparison.

## Why

`cargo clippy --workspace --all-targets` reports well over a hundred warnings, nearly all from lints
newer toolchains added (`is_multiple_of`, `if let` chains, `is_none_or`, `repeat_n`, `as_chunks`).
They hide any new warning that matters. The workspace denies dead code only, on purpose
(`Cargo.toml`: a new toolchain's lint must not break the build), and that stays: clippy stays a
warning, made quiet once.

## Decisions

1. **Clippy's own fixes first,** by `cargo clippy --fix`, run by the developer on a clean tree after
   the previous commit, so the diff is clippy's alone and reviewable. Its fixes are the
   machine-applicable ones, which keep the meaning: no search or evaluation change in them.
2. **The rest by hand, each its lint's way:**
   - `too_many_arguments`: `#[allow(clippy::too_many_arguments)]` on the function, as nine
     functions already have it. The search's are hot and positional by design; bundling them is a
     refactoring of its own, not a lint fix.
   - `type_complexity`: a type alias where the type names a thing (the search's root result); else
     allowed.
   - `unnecessary_min_or_max` (`search.rs`, `.min(INFINITY)` on an aspiration bound): read first:
     dropped if the bound truly cannot exceed `INFINITY`, kept with an allow and why if it can.
   - `neg_cmp_op_on_partial_ord` (`arena/config.rs`, `!(elo_d > 0.0)`): it refuses NaN too, on
     purpose: allowed with that reason.
   - `new_without_default`: a `Default` that calls `new`, unless `new` does real work (the
     tablebase's mmap, the engine's state): then allowed.
   - `needless_range_loop` in `simd.rs`: the scalar fallback indexes several arrays by one lane;
     allowed if an iterator reads worse.
   - `wrong_self_convention` (`TransformerBatch::to_device(self)`): it consumes the batch on
     purpose; allowed.
   - `assertions_on_constants`: a `const { assert!(..) }`.
3. **Checks:** the tests of every crate, clippy clean, and the bench's node count unchanged (the
   search's code is touched by the `if let` chains alone, which keep its meaning).

## Steps

1. The developer: commit, then `cargo clippy --fix`, then the tests and the bench, then commit.
2. Claude: the warnings left, by the list clippy prints then.
3. The developer: the tests, clippy, the bench; commit.
