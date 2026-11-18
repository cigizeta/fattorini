# Fewer king buckets for halfk: the implementation

**Status:** plan, 2026-11-18, approved (the tags, not a `VERSION` 3: the developer); no code. The design: halfk-king-buckets.md.

## What the code is today

- `features::halfk` holds a `pub const` chain: `BUCKETS` 32, then `HALFK_ROWS`, `THREAT_ROW`,
  `ROWS`, `TRAINING_ROWS`, `CASTLING_ROW`, `ROWS_WITH_STATE`, `TRAINING_ROWS_WITH_STATE`. They are
  plain `usize` arithmetic everywhere: no const generic and no array sized by the bucket count but
  the census's.
- `king_bucket(king) -> (bucket, mirrored)` computes `rank * 4 + file` after the mirror;
  `piece_row` and `factor_row` depend on `FACTOR_ROWS` (704) alone.
- The `.nnue` header has no spare field: magic, `VERSION` 2, a `FEATURE_SET_*` tag that implies the
  row count (`shape_of`), then the shapes. The file's own rule: a new variant gets a new tag,
  `VERSION` stays.
- The engine already refreshes a perspective only when `halfk_king` (bucket, mirror) changes, so
  fewer buckets refresh less with no change there. The refresh cache is keyed by the raw king
  square (`perspective * 64 + square`).
- Checkpoints (tch VarStore, the optimizers' state) hold `transform.weight` as `[dim,
  TRAINING_ROWS]`: the config must rebuild the same width to resume or export.
- Batches cache positions, not indices: the data caches are unaffected.

## Decisions in this plan

- **A layout value, `halfk::Layout`,** an enum `Squares` (today's 32), `Eight`, `Ten`, with its
  32-entry table and methods for every count of the chain: `buckets()`, `halfk_rows()`,
  `threat_row()`, `rows()`, `training_rows()`, `castling_row()`, `rows_with_state()`,
  `training_rows_with_state()`, and `king_bucket(king)`. `FACTOR_ROWS`, `IDENTITIES`, `STATE_ROWS`
  and the 4,096 threat rows stay constants. The `BUCKETS`-derived constants go: every use takes
  the network's layout.
- **The header: a tag for each kind and layout,** the file's convention: the three halfk tags (20
  to 22) stay `Squares`, and the eight and ten layouts of each get tags 23 to 28. `shape_of` returns
  the layout with the rows. Every `.nnue` today loads unchanged.
- **The refresh cache keyed by `(perspective, mirrored, bucket)`:** `perspective * 64 + mirrored *
  32 + bucket`, still 128 entries; with `Squares` it holds exactly what the square did, and with
  coarse buckets every king square of a bucket shares its entry.
- **The config:** `king_buckets: 32 | 8 | 10` on `Architecture`, `#[serde(default)]`, 32 when
  absent; refused on a kind that is not halfk. `run_config.txt` freezes it, so a resume rebuilds the
  same width.
- **The smithy's `FeatureSet::HalfKav2Hm` and `HalfKav2HmState`** carry the layout as a field.

## Steps

1. **`features::halfk`:** `Layout`, its tables (from the design's maps), its methods; `king_bucket`,
   `active_rows`, `piece_rows`, `expand_with_state`, `push_state_rows`, `factor_row`'s check take
   it. Tests: `Squares` reproduces today's buckets and counts exactly (the existing tests, rewritten
   against it); the 8 and 10 tables match the design's maps square by square; every table's buckets
   are `0..buckets()` with none empty; mirrored squares share the unmirrored's bucket.
2. **`nnue.rs`, the network:** `PsqNetwork` and `QuantizedPsqNetwork` gain `layout:
   Option<halfk::Layout>` in place of `halfk: bool` (`None` for the non-halfk kinds); `rows()`,
   `quantize` (lanes bounded bucket by bucket over `layout.buckets()`), `push_threat_rows`,
   `halfk_state_rows`, `halfk_king` read it. The tags 23 to 28, `shape_of`, the names, `write`'s
   choice of tag. Tests: a round trip of each layout through `write` and `from_bytes`; an old tag
   reads as `Squares`.
3. **`nnue.rs`, the refresh:** the cache's key; `ChangedRows::between_halfk` with the layout. The
   exactness test (incremental against from scratch over random games) runs for every layout; its
   `refreshes > 20` floor kept for `Squares`, a smaller one for the coarse layouts, with a test that
   a king move inside its bucket is incremental.
4. **The bench** (`nnue_bench.rs`): `--halfk` takes the layout (`--king-buckets 8`), so the speed
   of each is measured.
5. **The smithy's config:** `king_buckets` on `Architecture`, its validation, `run_config.txt`.
   Tests: absent is 32; 8 and 10 accepted on each halfk kind; 7 refused; refused on a psq kind.
6. **The smithy's trainer:** `FeatureSet` with the layout; `nnue_network.rs` (the transform's
   width, its initialisation, the dummy batches), `batch.rs` (the dense width, `halfk_indices`,
   `with_factor_rows`), `export.rs` (`halfk_network_from`'s slices and the factor's fold),
   `candidates.rs`, `stacked_network.rs`. Tests: the existing shape tests for each layout; an export
   of an 8-bucket training net evaluates as the trained net does (the export test, per layout).
7. **The census:** the layout as an option (`--king-buckets 8`), the counts a `Vec`, the boards
   printing each bucket's share on its squares.
8. **The configs:** `lc0-halfk-threats-screlu-hidden-budget-8.yaml` and `...-budget-10.yaml`, the
   budget config with `king_buckets` set, nothing else changed.
9. **Run:** `cargo test` for both crates, clippy; a scaled run of the 8 to its gate; the bench of
   32, 8 and 10. Then the experiments, on the full corpus (lc0-position-cap.md).

## Not in this plan

- Layouts other than the three; a layout table in the config. A fourth layout is one enum arm and
  two tags.
- Converting today's networks to a coarser layout.
