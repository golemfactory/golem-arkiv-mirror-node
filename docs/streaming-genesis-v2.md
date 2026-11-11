# Streaming genesis v2

The signed node now supports a file-backed, bounded-memory genesis prefill in
addition to the original small v1 builder. Both paths initialize the same entity
engine. Version two uses dedicated bulk construction, not incremental entity
replay. Private scratch is never chain history. The final node starts at height
zero. See [the bulk construction design](bulk-prefill.md).

## Build, verify and start

First convert a completed processor bundle: a v4 (UDC) bundle with
[the v4 converter](sourcify-v4-prefill.md), which emits row codec 3, the only
one the node accepts. Preserve the original source.
Create a base `genesis.bin` using the existing TOML genesis commands; its
allocations must fund the selected rent sponsor, and its proposer must match the
key used to start the producer. The streaming builder discards the base's old
prefill, selects 64 total cells and recalculates the expiry reservation.

```sh
export ARKIV_PREFILL_RESERVE_GIB=80
arkiv-proposer genesis stream-build /data/base/genesis.bin \
  --prefill /data/converted/prefill.bin --out /data/bundle \
  --sponsor 0xYOUR_FUNDED_SPONSOR
arkiv-proposer genesis stream-verify /data/bundle
arkiv-proposer producer --bundle /data/bundle --run-dir /data/producer \
  --key-file /private/proposer.key --listen 127.0.0.1:9500
```

To build the genesis state only once, keep the bundle builder's verified state
and hand it to the producer. The bundle is byte-identical either way:

```sh
arkiv-proposer genesis stream-build /data/base/genesis.bin \
  --prefill /data/converted/prefill.bin --out /data/bundle \
  --sponsor 0xYOUR_FUNDED_SPONSOR --state-out /data/genesis-state
arkiv-proposer producer --bundle /data/bundle --prebuilt-state /data/genesis-state \
  --run-dir /data/producer --key-file /private/proposer.key --listen 127.0.0.1:9500
```

The producer then skips normalization and index construction; see
[Build once](#build-once-prebuilt-state). Without `--prebuilt-state` it rebuilds
the state from the stream as before.

Use the normal signed-node flags for the bearer-protected private listener,
full replication and explicitly pinned full/light identities. The producer
starts paused unless `--start running` is given. Do not reuse fixture keys for a
public deployment. No existing explorer or indexer needs to be redeployed to
bootstrap the node.

The full follower can use `--bundle` or download the pinned canonical stream
from its peer, independently build its state, and compare the result with genesis.
`GET /signed/v1/prefill/chunk?offset=N&length=M` serves at most 1 MiB binary
bytes per request under the existing four-worker limit. Downloads retain a
partial file for resumption; the complete hash and canonical framing are checked
before installation. Corrupt completed partial downloads are discarded for a
fresh retry. The original small-v1 JSON endpoint is retained.

The light follower downloads only bounded genesis/certificate metadata and
headers. It verifies requested proofs against authenticated roots; it does not
import the dataset or independently establish dataset completeness.

## Canonical identity and memory

V2 has an explicit launch version, `ARKPFL02` stream framing and separate hash
domains. Its header is `ARKPFL02 ‖ u32(3) ‖ u64(row_count)`: row codec 3, the
user-defined-columns row of [the v1 contract](genesis-bundle-v1.md), which both
framings share (codec 2, the payload-era row, is refused). Each frame is
`u32(frame_length) ‖ Entity`, exactly the small stream's row encoding.
Each frame is bounded to 8 MiB and decoded independently. Frames have increasing
creator addresses and consecutive per-creator creation nonces. Attribute order,
value encoding, row counts, total length, trailing bytes and the incremental
Keccak commitment are checked.

The bulk builder assigns record IDs in canonical input order, validates entity
cells and expiry, and calculates rent and storage units. It spills key bindings,
rows, term/record memberships and host entries into sorted runs. The sorting
budget is 64 MiB total across four sorters, plus bounded individual entries and
merge buffers. Each merge opens at most sixteen runs. Run lengths and streaming
Keccak digests are verified before completion. A common posting list is streamed;
it is never accumulated as a whole in memory.

A sorted Merkle Patricia builder retains only its current key/value and a bounded
frontier of child references. Completed nodes are emitted to bounded object
batches (normally 32 MiB and 100,000 objects). This avoids incremental per-entity
index updates and intermediate committed roots. Existing codecs determine the
same final node encodings and state root.

Import redb handles have 256 MiB page caches. Decoded-node caching for the opened
staged engine is capped at 256 MiB and 262,144 entries. Final copying uses two
redb handles and one source decoded cache: a combined configured ceiling of
768 MiB, excluding object buffers, traversal stacks, ordinary history/runtime
caches and allocator overhead. A copy also uses a fixed 16,384-slot, 768 KiB
cache of successfully traversed posting roots, namespaces and validated lengths.
Collisions repeat work without omitting validation. Existing node opens and
history/header stores keep the default 8 MiB redb cache. The installing process
retains its larger destination cache until exit.

Buffer/cache settings do not enter genesis identity. Roots and accounting are
compared with the existing v1 builder on compatible fixtures. A frame carries
at most 32 user attributes, as a live create does; with no payload or content
type cells, the seven system cells leave all 32 to the user. Indexed
attributes (cell kind `01`) are posted; unindexed ones (`02`, and every
`bytes` value) are stored only. A row's `bytes` values hold at most 128 KiB
together.

## Row expiry

A row's expiry is a block height or the `u64::MAX` never-expires sentinel.

**Finite rows.** The sponsor prepays each row's rent at genesis under the
chain's rent rule, capped like a live create: a row at or beyond the rent cap
pays the cap (see [genesis bundle](genesis-bundle-v1.md#state-rent-and-first-live-block)),
so a height far in the future costs no more than a year. A height holds at
most the genesis `expiry_per_block` rows; prefill rows over that capacity are
refused, not moved to a later height as live writes are.
[`examples/far-future-prefill`](examples/far-future-prefill) is a base genesis
for such a stream.

**Permanent rows.** A base genesis authored with
`[prefill] allow_permanent = true`
([`examples/unlimited-prefill`](examples/unlimited-prefill)) compiles to prefill
launch version 3, and `genesis stream-build` on it commits version 4. Only such
a launch admits `u64::MAX` rows; a finite base (version 2) rejects the stream
before any state is built, with an error naming `allow_permanent`. A permanent
row is created with `$expiresAt = u64::MAX`, never enters the expiry schedule
(the per-height capacity does not apply and the expiry sweep never visits it)
and prepays no rent, so an all-permanent prefill burns 0. Its storage units
still count toward `storageUnits` and the declared limit. Finite rows in the
same stream are scheduled and charged exactly as in version 2. RPC reads return
`expiresAt: "0xffffffffffffffff"`; `/signed/v1` query proofs return the decimal
string.

The launch version and recipe domain enter the genesis hash, so the policy is
part of chain identity; the stream format and bulk algorithm are the same.
After genesis a permanent row is an ordinary permanent entity under the
[live rent rules](glm-gas-v1.md).

## Recovery and publication

Bulk construction uses separate `bulk-v1` scratch with an input/genesis identity
and an exclusive lock. Incomplete scratch is rebuilt from the beginning after an
interruption. This is safe restart behavior, not committed-prefix resumption.
Completed state has a durable engine head and a checksummed receipt binding the
genesis, entity count, storage units and rent. It is selected by a directory
rename only after metadata is synced. Restart reaudits the canonical stream and
validates the receipt and engine association before reusing completed state.
Old incremental checkpoints are never interpreted as bulk output.

Final installation copies reachable objects in bounded durable batches into a
separate headless store, then atomically writes the sole height-zero manifest and
node metadata. A failed copy can restart idempotently; it currently traverses
again from the beginning. Normal node open refuses an unfinished headless store.
Successful installation removes importer scratch. Bundle publication uses synced
files and an atomic no-replace directory rename.

`producer --genesis-install adopt` installs without the copy. It reads and checks
the same reachable objects in parallel without writing them, relabels the staged
store in one durable commit, and renames it over the headless `node.redb`. No
second database copy is written or stored. See
[Adopting the staged genesis store](genesis-adoption.md).

A fresh producer supplied with `--bundle` performs three standalone stream
audits: the supplied bundle, the actual local copy, and the retained serving
file after final database installation. The local audit retains an open file
descriptor and a private summary bound to the signed genesis. An inode check
ensures the published hard link names that descriptor, and construction consumes that
same descriptor instead of reopening its pathname. Construction checks every
frame, the full stream hash/count/length, and derived state, rent and storage
units. The final audit catches pathname or
content changes during the final copy. Public standalone verification continues
to audit its input on every call. These checks replace the former immediate
duplicate local-file audit and the audit at replay entry; no filename or
timestamp is accepted as proof of file contents.

## Build once (prebuilt state)

Bundle authoring already constructs the complete genesis state database to
derive the signed state commitment. Without a handoff, the producer audits the
same stream and constructs the same database again: on the full Ethereum run,
`stream-build` took 7 h 29 m and the producer repeats its roughly 7.2 h of
normalization and index construction before installation even starts.

`stream-build --state-out DIR` keeps the completed private bulk directory
instead of deleting it. Before the stream audit begins, it checks that `DIR`
does not exist, that its parent exists, and that it does not overlap the bundle
or its work directory. It also probes a real rename from the work directory's
parent into `DIR`'s parent, so separate mounts are refused within seconds. After
construction, a synced `prebuilt.json` binds the store to the bundle's genesis hash,
state root, stream hash, row count and import identity. The bundle is published
next, then the state directory is renamed into place with no-replace semantics.
If that last rename fails, the bundle is valid and the error names the bound
store left at `.<bundle>.stream-work/bulk-v1`. That directory is itself a valid
prebuilt state directory: move it or pass it to `--prebuilt-state`, or delete it
and let the producer rebuild.

`producer --bundle B --prebuilt-state DIR` is accepted only by a producer with a
streaming bundle. While holding the node database lock, a fresh node checks that
`DIR` is a real directory with the expected import identity, manifest and
completed store files. A mismatched or incomplete store is refused before
anything moves. The store is then renamed into `prefill-work/bulk-v1`, or on
another filesystem copied into an unselected directory, synced and renamed. The
rename consumes `DIR`; a copy leaves it. An interrupted copy is discarded and
repeated, and a restart after the rename reuses the selected store. Once the
node has a head, the option is ignored. If reuse or installation rejects the
selected store (for example `StorageCorrupt` or a receipt mismatch), every restart
fails the same way until the run directory's `prefill-work` is deleted. The
headless `node.redb` may stay. Its copied objects are content-addressed and are
only published by a later successful installation.

From there the producer takes its existing completed-state restart path, so the
checks before serving are unchanged:

1. The canonical stream is audited three times as before (bundle, local copy,
   retained file after installation), plus once when reusing the completed store.
2. The store's import identity binds the base genesis, limits, sponsor and stream
   summary. Its checksummed receipt must match the row count and admit the storage
   units and rent. The engine genesis must match the expected deployment, limits
   and pricing.
3. The store's committed state header, rent and storage units must equal the signed
   genesis commitment.
4. Installation copies every object reachable from that root into the headless
   node database. Every load checks `keccak256(bytes) == id`, map entry counts,
   row admission limits and host value bounds. Only then is the height-zero
   manifest, identity and certificate packet published. A tampered object fails
   with `StorageCorrupt` and no head is ever published.

What the producer no longer does is derive the state from the stream a second
time with the same binary on the same input. That rebuild was a determinism
self-check within the operator's own trust domain, the domain that also produced
the commitment, rather than an independent verification. **Use only a state
directory from your own `stream-build --state-out` run for exactly this bundle.**
Its identity and receipt digests are unkeyed: they catch mix-ups and corruption
of the store, but not a forger who also controls `genesis.bin`. Independent
derivation remains with full followers, `genesis stream-verify`, and any producer
started without `--prebuilt-state`. The prebuilt directory is private local
state. Never distribute it as part of a bundle, and never give it to a follower;
the follower roles refuse the option.

## Resource policy

Run heavy phases one at a time under an external 32 GiB cgroup limit, with swap
disabled. For example, on a systemd host use `systemd-run` with `MemoryMax=32G`,
`MemorySwapMax=0`, an explicit unprivileged user and
`--setenv=ARKIV_PREFILL_RESERVE_GIB=80`. The code's caches and batches provide the
memory bounds; the cgroup is an additional backstop.

The Rust library's portable default disk reserve is 1 GiB. **Set the environment
variable to 80 or higher for the real dataset.** Checks cover ingestion, sort
flushes, merges, object batches and stream copies/downloads. Long merges recheck
the reserve periodically; final installation projects another database copy.
These projections are conservative checks, not a disk quota or proof of full-dataset fit. Account for input, converter
scratch, canonical output, sorted runs, staged/final databases,
transport copies and reserve concurrently. Live history still has no physical
pruning.

The processor-to-stream converter has atomic publication but is not resumable.

## Progress and measurement

The importer writes JSON progress records to **stderr**, leaving stdout for
result JSON. Stream phases use `event: "prefill_progress"` with `audit`,
`bulk_normalize` and `bulk_indexes`. Counts describe phase progress, not committed
chain blocks. A completed phase does not assert bundle publication or full
acceptance: require successful command completion and the final result.
`prefill_bulk_phase` records distinguish keys, rows, postings, host construction
and long sort merges, with phase-local entry counts.

The deprecated `stream-build --batch-bytes` option is accepted for compatibility
and controls ingestion progress sampling only. It does not enable incremental
replay or change the fixed sort-buffer budget.

Capture stderr alongside external measurements of cgroup memory/peak and events,
process RSS, wall time, free disk, and allocated bytes for the staging and output
directories. Keep raw samples and the exact command, binary revision, input hash,
cache/buffer settings, and restart history with the result. Measure physical disk
allocation separately from logical storage units. Run one heavy phase at a time;
progress rates from small or synthetic samples do not establish a full-dataset
completion estimate.

## Measurements

Dated runs (build once against producer rebuild, adoption against copy, bulk
construction against the earlier incremental importer) are in
[benchmarks/2026-10-20-streaming-genesis.md](benchmarks/2026-10-20-streaming-genesis.md).
