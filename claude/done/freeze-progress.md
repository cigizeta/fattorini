# smithy freeze: a progress bar for every phase

Implementation plan, 2026-10-17, agreed with the developer. After the sampling bar, `smithy freeze`
printed nothing while it wrote the frozen files and uploaded them (freeze-implementation.md), so a
slow phase could not be told from a hung one.

## Changes, all in `crates/smithy/src/freeze.rs`

- **`write_positions`:** a bar over the rows, labelled `writing positions`.
- **`write_label_sets`:** one bar per lambda over the same rows, labelled `writing lambda-X`.
- **`upload`:** a bar over files, labelled `uploading`. It lists each directory's files locally
  (data files first, then `manifest.txt`, as today) and calls `remote::copy_up` once per file with a
  filter accepting that name alone, so `remote` is unchanged. Counting bytes would need a copy loop of
  its own in `remote`: not worth it (the developer, 2026-10-17).

Every bar is hidden under `cfg!(test)`, as the sampling bar is.

## Verification

```bash
cargo test -p smithy freeze
```

Then the next `bin/smithy freeze` shows a bar in every phase.

## Verified (2026-10-17, the training machine)

`bin/smithy freeze caroline-0001 caroline` showed every bar: reading 11 s, writing positions 53 s,
each label set 2 s (1.00 6 s), uploading 28 files in 1 s. The sampling bar had no label; it is now
`reading games`, like the others.
