# Bulk construction for streaming genesis

Version-two prefill must construct the final authenticated state without running
one normal entity write per input row. The incremental implementation took
21m 10.6s to start a producer from 445,583 entities (100,000 contracts), writing
168.5 GB for 1.86 GB of canonical input. Those process-wide writes include
compaction and final installation; they are not a measurement of index writes
alone. Increasing replay batch sizes was abandoned before experiments ran.

## Construction contract

The input format, signed genesis, record IDs, state root, sponsor debit, rent and
storage units must remain unchanged. Normal live transaction execution and the
small version-one builder retain their existing behavior. Incremental replay is
an equivalence oracle for tests, not a fallback for version-two import.

Construction has four parts:

1. Stream and validate canonical frames, assigning record IDs in input order.
   Validate entity attributes, expiry and engine admission limits, calculate
   storage units and rent, and retain only the current creator's nonce count.
2. Spill bounded sorted runs for key bindings, term/record memberships and host
   entries. Merge with bounded fan-in. Reduce expiry counts with checked sums
   and enforce the per-height expiry capacity. Reject duplicate entity keys.
3. Build the canonical Merkle Patricia maps from sorted entries. Complete each
   posting list before inserting its root into the term map. Keep only bounded
   tree frontiers and object-write buffers, including for very common terms.
4. Flush final objects and atomically publish the completed private genesis.
   Check the derived commitments before installing the node's height-zero state.

Internal sort order must use the existing trie key encodings, not incidental
Rust ordering. Catalog and host state matter as much as query indexes: creator
nonces, expiry counters, namespace/record allocators and sponsor accounting all
contribute to the final state.

## Row format

Bulk construction consumes the same canonical rows as the small builder:
`ARKPFL02 ‖ u32(3) ‖ u64(row_count)`, then `u32(frame_length) ‖ Entity` frames
in row codec 3 (user-defined columns). The exact byte contract a converter must
emit, including the per-attribute indexed flag (cell kind `01` indexed, `02`
unindexed), user `bytes` (always unindexed, at most 128 KiB per value and per
row), at most 32 attributes, and the absence of any payload or content type, is
in [the genesis bundle contract](genesis-bundle-v1.md#versions-and-identity).
Each row is validated exactly as a live `Create` of its attributes, and its
cells are the seven system cells followed by those attributes. Indexed cells
become term/posting memberships; unindexed cells are written to the row only.

## Resource and recovery requirements

Memory usage is bounded by configured sorting, merging and object-write buffers,
trie frontiers, and storage caches. Whole-dataset entity arrays, posting lists,
creator maps and expiry maps are not allowed. Temporary disk usage and free-space
reserve remain operational constraints.

Incomplete bulk scratch can be rebuilt from the canonical input. This is safe
restart behavior, not the incremental importer's committed-prefix resumption.
Completed scratch may be reopened only when bound to the same input and genesis;
partial state must never become a live node. Existing incremental scratch must
not be mistaken for completed bulk state.

Final installation may still copy reachable objects into the headless node
store in bounded batches. Removing that copy requires separate treatment of
node identity and atomic metadata publication; bulk construction alone does not
claim to eliminate it. Bulk construction itself now runs once per launch:
`stream-build --state-out` hands its completed store to `producer
--prebuilt-state` (see [Build once](streaming-genesis-v2.md#build-once-prebuilt-state)).

## Measurements

The 100,000-contract producer measurement against the earlier incremental
importer is in
[benchmarks/2026-10-20-streaming-genesis.md](benchmarks/2026-10-20-streaming-genesis.md#bulk-construction-against-incremental-replay-2026-10-19);
the matched 100k/200k comparison is in
[benchmarks/2026-10-19-bulk-prefill-scaling.md](benchmarks/2026-10-19-bulk-prefill-scaling.md).
