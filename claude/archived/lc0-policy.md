# Lc0's data for value and policy networks

**Archived 2026-11-04 (the developer): it assumed `-quiet` succeeded, which failed; the quiet filter
was removed and value networks read `data/lc0` unfiltered (lc0-counts-forges.md). A policy head will
need a fresh design.**

**Status:** designed 2026-10-29 with the developer, sizes measured; plan in
lc0-policy-implementation.md. Assumes `-quiet` succeeds
(quiet-filter.md): quiet positions become the standard for NNUEs without a policy head.

## Goal

One Lc0 corpus serving every kind of network: NNUEs without a policy head, trained on quiet
positions only; NNUEs with a policy head (mcts-nnue.md); transformers, which today have no policy
head but will.

## Decisions

1. **The corpus converted again with the policy** (mcts-nnue.md, "The policy target's format"):
   each comment carries every legal move's visit count, recovered losslessly as share times
   `visits - 1`, the legal moves sorted by their UCI text. The raw tars kept, never downloaded again.
   The corpus itself is never filtered.
2. **Bucket folders by the reader's kind, not by filter (the developer, 2026-10-29):**

   | folder | holds | read by |
   |---|---|---|
   | `data/buckets/lc0` | every position, labels only | old configs, temporarily |
   | `data/buckets/lc0-value` | quiet positions, labels only | NNUEs without a policy head |
   | `data/buckets/lc0-policy` | every position, labels and counts | NNUEs with a policy head |
   | none | the corpus | transformers, with or without a policy head |

   Value-only runs read slim records, never paying for counts they ignore; the positions stored
   twice cost about 23 GB, acceptable now that disk is no constraint.
3. **The filter stays at the scatter,** as built (quiet-filter-implementation.md): no quiet flag in
   a record, no change to the batch reader.
4. **No filter with a policy head:** dropping positions whose best move is a capture or a promotion
   would teach a policy never to capture. Masking the value loss instead was rejected (the developer,
   2026-10-29): it shrinks the value's effective batch by about a quarter, varying batch to batch.
   A policy network's value head trained on quiet positions only, if ever wanted, needs a design of
   its own.
5. **`lc0-value` is today's `lc0-quiet` renamed:** its records hold no counts, so the reconversion
   leaves them unchanged and nothing is scattered again. The forge picks `-value` or `-policy` by the
   network's kind instead of `-quiet` by the filter.
6. **Test sets:** `frozen-quiet` goes with `lc0-value`; `frozen`, every position, with `lc0-policy`
   and the transformers. Both are sampled from the corpus, not from the buckets, so neither changes.
7. **The quiet filter implied for value-only NNUEs:** a new config of that kind is quiet whether or
   not it says so; one turning the filter off is refused. Old networks are still played and scored,
   not retrained.
8. **`lc0` kept for backward compatibility, temporarily:** deleted later, for example once no active
   config reads it.
9. **Transformers:** `pgn_train_stream.rs` passes each position's counts beside its labels once a
   transformer has a policy head; one without ignores them. The head and its loss belong to the
   transformer's own design.

## Measured (2026-10-29)

`lc0::policy_size` over the two test tars (66,542 standard games, 2,800 skipped, 7,910,812 records),
no record refused: every share is an exact count of `visits - 1`, every policy entry a legal move and
every legal move an entry.

- **Every legal move visited:** 27.67 counts above zero for 27.67 legal moves a record, so a sparse
  encoding saves nothing. Visits less one 799 to 830 at every percentile (BT4's debug run at 800).
- **The largest count:** fits 8 bits in 29.3% of records, 16 bits in the rest, never wider.
- **The corpus:** 4.85 compressed bytes a position without counts, 21.45 with, +16.60 (+342%): about
  5 GB to 21 GB over Lc0's 1B positions.
- **A record's counts, uncompressed:** 29.09 bytes as varints (about 1.05 a count), 48.23 at one or
  two bytes a count, 56.76 sparse with an index a count.

**The counts' encoding (proposed 2026-10-29):** one LEB128 varint a legal move, in the comment's order
(sorted UCI text), its length the position's legal moves, so nothing more is stored. A policy record
is a type of its own beside today's, so `lc0`, `lc0-value` and every value-only path stay untouched.
`lc0-policy` estimated at about 40 to 50 GB.

## Open

- **The counts in a bucket record:** encoding proposed above; what the shuffle and the batch reader
  need from a variable-length record.
- **A frozen test set with counts:** a policy network's test loss needs them, and today's frozen
  sets store positions and labels only.
- **The new corpus replacing the old segments, or beside them** until verified.
- **The target's shaping:** a temperature, a weight by total visits; Lc0's visits include its root
  Dirichlet noise, acceptable for a teacher but to be stated.
- **The output encoding** (Lc0's 1858 indices or the legal moves scored directly): each
  architecture's own design, not this one's.
- **When `lc0` is deleted.**
