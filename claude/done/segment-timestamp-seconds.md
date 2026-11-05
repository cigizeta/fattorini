# Seconds in self-play segment names

**Status:** done; generation 3's self-play ran with it (verified 2026-10-14).

## Problem

A segment is named `{stem}-YYYYMMDD-HHMM.pgn.zst`, the minute its batch is compressed. The name
assumed a batch takes hours, but with `file: 100000` in `configs/corpus.yaml` a batch fills in
about 30 seconds on 20 threads. Two batches finished in the same minute, and `compress_and_clear`
correctly refused to overwrite the first segment's kept `.count`, stopping `arena self`.
Nothing was lost: the refusal comes before the staging file is sealed.

## Decision

Segment timestamps become `YYYYMMDD-HHMMSS` (UTC), in `self_play.rs` only:
`current_timestamp`, `TIMESTAMP_LEN` (13 to 15), `sealed_stem`'s doc, and the tests' sealed names.

- Segments already written keep their minute-only names. Nothing parses a finished segment's
  timestamp: only a local *sealed* `{stem}-{timestamp}.pgn` is parsed (`sealed_stem`), and none
  exists across the switch unless a compression crashed right before upgrading. Such a leftover
  would then read as another stem's unfinished batch (`other_active_stem`) and stop the run with
  its name printed, to be compressed by hand; not worth permanent code for two formats.
- Match folders (`match_player.rs`) keep `YYYYMMDD-HHMM`: changing `is_timestamp` would stop an
  unfinished match in an old folder from resuming, and a same-minute clash there is already
  refused with a clear message.
