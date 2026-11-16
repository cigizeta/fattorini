# halfk's castling and en passant rows: implementation plan

Status: written 2026-11-16, decisions approved by the developer, implemented the same day, to test.
As built, against the steps below: decision 4's flag is not a stored field but derived,
`PsqNetwork::halfk_state` = halfk and pairwise, the very test that writes tag 22 (copied into
`QuantizedPsqNetwork::halfk_state`); the trainer's `with_options` asserts the same pairing. The
engine's update and refresh cache diff the state rows as sets (`halfk_state_rows`), only when the
rights or the en passant square changed; the quantization bound counts the 12 rows beside every
bucket. The trainer's `Network` keeps one halfk variant; `DenseBatch` and `HostBatch` gained one. Implements decision 7 of
halfk-threats-pairwise-hidden.md: psq's 12 castling and en passant rows in tag 22's halfk network,
the network step 3 trains. The pairwise head itself is already implemented and tested
(halfk-threats-pairwise-hidden-implementation.md); this plan is the rows alone. Tags 20 and 21 keep
their layout: their networks are trained and must still load.

## Findings that shape the plan (2026-11-16, from the code)

- **Engine layout** (`features::halfk`): piece rows `0..22,528` in the king's bucket, then threat
  rows `22,528..26,624` (`ROWS`). Nothing after.
- **Trainer layout:** the engine's rows, then the 704 factor rows at `halfk::ROWS..TRAINING_ROWS`
  (`batch.rs` `with_factor_rows`, `export.rs`, `nnue_network.rs`' start and its tests).
- **psq's rows** (`features::psq`): 4 castling rights relative to the perspective (own kingside,
  own queenside, opponent kingside, opponent queenside), then 8 en passant files; not mirrored, psq
  having no mirror. The engine's update compares the boards' rights and files (`ChangedRows::between`).
- **The engine's halfk update** (`between_halfk`) compares piece bitboards only; a perspective whose
  king changes bucket or mirror is refreshed through the refresh cache, whose entries remember piece
  bitboards only.
- `MAX_ACTIVE_ROWS` (37) already counts 4 rights and a file: the accumulator bound holds.

## Decisions (proposed, for the developer)

1. **Engine layout:** the 12 rows after the threats: castling at `26,624`, en passant at `26,628`,
   `26,636` rows (`halfk::ROWS_WITH_STATE`). Tags 20 and 21 keep `26,624`.
2. **Trainer layout:** the 12 rows after the factor rows, `27,328..27,340`, so the factor rows
   stay at `halfk::ROWS` for every halfk kind and none of their code moves; the export copies the 12
   into the engine's place. No factor rows for them: they are outside the buckets already.
3. **The en passant file mirrored with the perspective's king** (`file ^ 7` when the king is on
   files e to h), as every halfk piece square and threat is: the file names a square of the
   mirrored board the rest of the network sees. **Castling rights not mirrored:** kingside means the
   king's rook side whatever the mirror, as psq has them. psq mirrors neither, having no mirror.
4. **One flag**, `PsqNetwork::castling_en_passant` (true for tag 22 only), read wherever the layout
   differs; no new architecture.
5. **The trainer's feature set:** `FeatureSet::HalfKav2HmState`, halfk's rows plus the 12, chosen by
   `HalfkThreatsPairwiseHidden`; the other two halfk kinds keep `HalfKav2Hm`.

## Steps

### 1. Features (`crates/fattorini/src/features.rs`, `halfk`)

- `CASTLING_ROW = ROWS`, `EN_PASSANT_ROW = CASTLING_ROW + 4`, `ROWS_WITH_STATE = ROWS + 12`,
  `TRAINING_ROWS_WITH_STATE = TRAINING_ROWS + 12`.
- `state_rows(castling_rights, en_passant_file, perspective, mirrored, base) -> rows`, one function
  used by the features, the engine's refresh and its update, so the three agree.
- `active_rows_with_state` and `expand_with_state`: `active_rows` plus the 12.
- Tests: the start position has 4 castling rows a perspective and no file; after 1. e4 d5 2. e5 f5
  the file is f, mirrored or not as each king's file says; rights lost on a king move.

### 2. Engine (`crates/fattorini/src/nnue.rs`)

- `shape_of`: tag 22 `(ROWS_WITH_STATE, 1)`; `from_bytes` and `write` set and check the flag.
- Refresh (`refresh`, `refresh_by_color`, `piece_square_active_rows`): the 12 added when flagged.
- Update: `between_halfk` also compares rights and the en passant file when flagged; a mirror
  change already refreshes the perspective.
- Refresh cache: each entry also remembers its rights and file, and diffs them as it diffs pieces.
- Tests: write and load at the new shape; the incremental accumulator equal to a refresh along a
  line with castling both ways, a double pawn push, an en passant capture, and a king crossing the
  mirror; a tag 21 file still loads at `26,624`.

### 3. Trainer (`crates/smithy`)

- `FeatureSet::HalfKav2HmState`: `batch.rs` its indices at `TRAINING_ROWS_WITH_STATE`, the factor rows
  as halfk's, the 12 after them.
- `nnue_network.rs`: the transform `TRAINING_ROWS_WITH_STATE` wide for the kind; the start as
  halfk's for the piece rows, the 12 drawn as the threat rows are.
- `export.rs`: the 12 copied from `TRAINING_ROWS..` to `ROWS..` in the engine's transform.
- Tests: the batch's rows against `expand_with_state`; the tch value equal to the engine's f32 value
  through a file, on positions with rights and a file.

### 4. Bench and docs

- `nnue_bench --halfk --hidden --pairwise`: the new shape.
- architectures.md, halfk-threats-pairwise-hidden.md's status.

## After the code

The bench before training (halfk-threats-pairwise-hidden.md): speed at depth 9 against
`lc0-halfk-threats-screlu-hidden-g1-r1`, and the share of zero products. Then the training, then
the duels against that network, depth 9 then 10+0.1, both untuned.

## Commands for the developer

```bash
cargo test --workspace --release
```

```bash
cargo clippy --workspace --all-targets --release -- -D warnings
```
