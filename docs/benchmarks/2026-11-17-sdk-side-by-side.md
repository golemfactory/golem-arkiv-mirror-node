# SDK side-by-side: this node and arkiv-reth-dev (2026-11-17)

A small benchmark of the same workload through `@arkiv-network/sdk` 1.0.1
against this node and the reference dev image. One run each, 2,000 entities:
it shows orders of magnitude and design limits, not tuned capacity.

## Setup

- Machine: `rogue-one`, 12 cores, 62 GB RAM, both nodes on loopback.
- This node: `arkiv-proposer producer`, release build of `4f07c3d`, one
  producer, **250 ms blocks**, default limits (`--read-concurrency 4`).
- Reference: `ghcr.io/arkiv-network/arkiv-reth-dev:latest`
  (`sha256:99a7539a1805…`) in Docker with a published port; it produces a
  block every 250 ms.
- Client: `scripts/sdk-side-by-side-bench.ts` (Bun 1.4.4), SDK v1.0.1, one
  funded account, receipt polling every 50 ms (viem's 4-second default
  otherwise dominates every write latency).
- Workload: entities with four indexed attributes (`run`, `category`, `score`,
  `title`) and a 256-byte unindexed `body`. Writes: `executeBatch` of 16
  creates, 8 transactions in flight with explicit nonces (`BATCH=16 WAVE=8`),
  each entity with its own expiry height. Reads: 300 requests one at a time
  (p50/p95), then 300 requests from 4 concurrent callers (requests per second).

## Results

| Writes, 2,000 entities | This node | Reference |
| --- | --- | --- |
| Wall time | 11.8 s | 4.0 s |
| Entities per second | 170 | 499 |
| Entities per block | 43 | 125 |
| Transaction latency p50 / p95 | 610 / 795 ms | 256 / 291 ms |
| Gas per entity | 41,816 | 28,648 |

| Reads (rows returned) | This node p50 / p95 | at 4 callers | Reference p50 / p95 | at 4 callers |
| --- | --- | --- | --- | --- |
| `getEntity` (1) | 0.3 / 0.3 ms | 13,744 req/s | 0.4 / 0.5 ms | 6,558 req/s |
| equality, limit 100 (100) | 3.4 / 4.4 ms | 873 req/s | 11.7 / 12.7 ms | 282 req/s |
| range, limit 100 (100) | 3.5 / 4.6 ms | 845 req/s | 19.2 / 20.7 ms | 189 req/s |
| count of 2,000 | 0.2 / 0.2 ms | 16,890 req/s | 9.5 / 10.0 ms | 371 req/s |
| `ORDER BY score DESC`, limit 20 | 3.8 / 4.8 ms | 430 req/s | 11.1 / 11.8 ms | 322 req/s |
| offset 1,950, limit 50 | 3.4 / 4.0 ms | 428 req/s | 10.6 / 11.3 ms | 323 req/s |

Reads are 3 to 50 times faster here; the count is answered from stored posting
sizes, where the reference's cost grows with the match set (a 256-entity trial
on the reference answered the same queries in about 1 ms). Writes are about
three times slower: a wave of 8 transactions lands in one block on the
reference and takes two to three blocks here. Why was not investigated.

## Limits the workload hit on this node

These are differences an application sees, found while making the workload
run; the reference accepted every one of them.

1. **At most 16 entities may expire at one block** (`expiry_per_block`,
   bounded at 16 by `signed/protocol.rs`). Creates sharing a lifetime
   (`ExpirationTime.fromHours(2)`) in one block fail after the first 16: of 8
   concurrent 16-create transactions, one succeeded and seven reverted at
   21,000 gas with no reason the SDK could show. The benchmark gives every
   entity a different expiry height to get past this.
2. **At most 16 operations per transaction and 128 per block**
   (`max_ops_per_tx`, `max_ops_per_block`). An `executeBatch` of 50 creates
   fails in `eth_estimateGas` with `InvalidTransaction`; so does any batch
   over 16. This also caps writes at 128 entities per block.
3. **Excess concurrent reads are refused, not queued.** With 16 concurrent
   callers `arkiv_getEntity` answered `ServerBusy` (`-32010`): at most
   `--read-concurrency` (4) detached reads run at once. The reference served
   16 callers. The SDK does not retry this error.

## Caveats

- One run, small data, no warm-up beyond one request per read; the reference
  runs behind Docker's port mapping and is a dev image.
- This node's state held 4,256 entities during the measured run (two earlier
  trial runs), the reference's 2,512; every read filters on the run's tag.
- Rent, fees and gas schedules differ, so gas per entity is not a cost
  comparison.
