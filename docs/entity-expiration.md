# Entity expiration

How an entity's expiry is set, when the entity is removed, and what bounds
that work. This page describes the behaviour; the exact rules, encodings and
error cases are in the [block format](block-format.md#execution) and are the
specification. Rent is in [GLM gas](glm-gas-v1.md), the rule's rationale in
the [spill proposal](proposals/expiry-spill.md).

## In short

- Every entity stores one expiry height, `$expiresAt`. A finite height `E`
  means the entity is deleted in block `E`; `u64::MAX` means it never expires.
- Block `E` starts with the expiry sweep, which deletes every entity recorded
  at `E` before any transaction runs. Nothing expired is ever left in state.
- At most `expiry_per_block` entities (a genesis parameter, 1 to 128, default
  128) may be recorded at one height. An operation aiming at a full height is
  moved to the next height with room, up to 64 heights later; only if all of
  those are full does it fail with `ExpiryCapacityFull`.
- Events and reads report the height the entity was given, which can be later
  than the one asked for. Rent is charged for the height asked.

## Setting an expiry

A create and an `extendExpiry` carry `{ expiresAt, minLifetime }`. In block
`h` the node resolves it to `E = max(expiresAt, h + minLifetime)`:

| Case | Result |
| --- | --- |
| `h + minLifetime` overflows | `ExpiryInvalid(Overflow)` |
| `E == u64::MAX` | permanent: stored as `u64::MAX`, never scheduled, never swept |
| `E <= h` | `ExpiryInvalid(DeadOnArrival)` |
| otherwise | finite expiry `E`, then the capacity rule below |

The SDK's `ExpirationTime.fromBlocks(n)`/`fromHours(...)` send `minLifetime`
with `expiresAt = 0`, so `E` is the including block's height plus the
lifetime: every create of one block with the same lifetime asks for the same
height.

An entity has no maximum lifetime. `max_lifetime` in the chain parameters
bounds only generic records (the simulator's native record operations, which
answer `TooFar`); for entities, `max_rent_lifetime_seconds` caps what a long
lifetime costs, not how long it may be. SDK 1.0 refuses lifetimes past its own
`MAX_LIFETIME` client-side.

An extension may not shorten: a resolved height below the entity's recorded
one is `ExpiryShortened` (`ExpiryNotExtended` to the SDK). The same height is
accepted and changes nothing. Anyone may extend an entity created with the
permissionless-extension flag, up to one rent cap ahead; beyond that, and to
permanent, only the owner (`PermanenceOwnerOnly`). A patch or an ownership
transfer does not touch the expiry.

## The sweep

Each block runs, in order: admin inputs, the expiry sweep, user transactions.
The sweep of block `h` deletes every entity whose recorded height is `h`,
through the same engine path as a user's delete (cells, index terms, state
root), in (namespace, record id) order. Each deletion has a receipt in the
block's outcomes (`BlockOutcomes::expiry`) and its units count toward the
block. A transaction in block `h` that names such an entity gets
`EntityNotFound`, and its key can be created again in the same block.

There is no logical expiry and no lazy purge: an entity is in the state if
and only if it is live, so reads, counts and proofs need no liveness filter
and the state root commits only to live entities.

The node finds the due entities from two counters kept in consensus state:
`due(height, namespace)` and `total(height)`, incremented and decremented by
every operation that sets, moves or removes a finite expiry.

## `expiry_per_block`

`ChainParams::expiry_per_block` is the most entities that may be recorded at
one height, and therefore the most deletions one sweep performs.

**Configuring it.** It is a genesis parameter, so it is chosen when a chain
is created and cannot change afterwards:

- In a genesis bundle, `expiry_per_block` under `[chain_params]` of the
  profile. The builtin preset `arkiv-entity-v3/glm-v2@5` sets 128; a chain
  that wants another value uses `profile = { path = "..." }` with its own
  copy of [the profile](examples/genesis-bundle/profiles/fixed-glm-v2.toml).
  The earlier preset `arkiv-entity-v3/glm-v2@4` (16) remains accepted and
  expands to the same bytes as before.
- A producer started without a bundle uses 128.
- A signed chain accepts 1 to 128 (`MAX_EXPIRY_PER_BLOCK` in
  `signed/protocol.rs`). 128 is the most operations a block can hold
  (`max_ops_per_block`), so with 128 the creates of one block can share a
  lifetime without ever filling a height by themselves.

An existing chain keeps the value in its genesis. Changing it means a new
genesis, that is, a new chain.

**The budget reservation.** The sweep's work is reserved out of the block
budget so that it can never crowd out transactions or halt a block: a genesis
(and every pricing change) must satisfy

```text
block_budget >= user_budget + expiry_per_block × max_expiry_delete_cost
```

where `max_expiry_delete_cost` is the delete charge of a record with
`max_cells` cells, all indexed. Under the default schedule and 39 cells it is
53,400 units, so 128 deletions reserve 6,835,200 of the 10,000,000 units the
default budgets leave (`block_budget` 100,000,000, `user_budget` 90,000,000).
The cost grows by 1,100 units per cell: 128 deletions fit up to 61 cells per
record, and a 64-cell profile (80,900 units) needs `expiry_per_block <= 123`
or a smaller `user_budget`. A genesis that violates the reservation is
refused.

**Other bounds.** 128 deletions add about 5 KB to a block's outcomes (the
largest block, 128 creates beside 128 deletions, has under 16 KiB of outcomes
against the 64 KiB the body cap reserves for them) and at most 128 small
entries to the block's metadata feed.

**Time.** Measured on this repository's machine (Ryzen 5 3600, NVMe, ext4),
release build, durable redb stores: the time `produce` takes for the block
whose sweep deletes a full height, commit included. An empty block takes 5 to
8 ms on the same stores.

| Entities at the height | Typical | 31 indexed strings | Worst case |
| --- | --- | --- | --- |
| 16 | 14 ms | 14 ms | 26 ms |
| 32 | 13 ms | 19 ms | 50 ms |
| 64 | 13 ms | 36 ms | 134 ms |
| 128 | 19 ms | 67 ms | 260 ms |

"Typical" is the SDK benchmark's entity (4 indexed attributes and a 256-byte
body). "Worst case" is the largest entity the node admits: 31 indexed
128-byte strings and one 128 KiB `bytes` value; the middle column is the same
entity with a 256-byte body. Time is linear in the number of entities: about
0.1 ms per typical entity and 2 ms per worst-case one, three quarters of
which is the 128 KiB value.

So 128 typical entities take under a tenth of a 250 ms block, but **128
worst-case entities take a whole 250 ms block** (260 ms) and a quarter of a
1 s block. Filling a height that way takes 16 MiB of entities, at least 26
blocks of writes (five such entities fit one block) and their rent. The
default of 128 is sized for block periods of 1 s and more. A chain with
250 ms blocks that must bound the worst case should set `expiry_per_block =
32` (50 ms, a fifth of a block); thanks to the spill rule a lower value no
longer makes batches fail, it only spreads their expiries over more heights.
Reproduce with:

```sh
ARKIV_BENCH_DIR=/path/on/a/real/disk cargo test --release -p arkiv-node \
    --test signed_host expiry_sweep_timing -- --ignored --nocapture
```

The delete charge does not depend on the size of an unindexed value, while
the measured time does; see the open items at the end.

## The spill rule

When an operation resolves to a finite height `E`:

1. If the entity is already recorded at `E` (an extension to its own height),
   nothing changes.
2. Otherwise the entity is recorded at the first height in `E ..= E + 64`
   whose total is below `expiry_per_block`. The search reads one counter per
   height, in order, so its result depends only on the state and on the
   operations before this one: every node resolves the same height.
3. If all 65 heights are full, the operation fails with `ExpiryCapacityFull`
   and, as any failed operation, rolls its transaction back.

The window is the constant `EXPIRY_SPILL_WINDOW` (64, in `exec.rs`). It is
not a genesis parameter: the chain parameters' encoding is fixed, and adding
a field is a new block format.

Details:

- **Order.** Operations take heights in execution order. With 128 per height,
  256 creates aimed at `E` over two blocks get `E` (the first 128) and
  `E + 1` (the rest); a 257th gets `E + 2`.
- **Extensions.** An extension onto a full height spills the same way and
  frees the entity's old height. The no-shortening check uses the height
  asked for, and the spilled height is never earlier than that.
- **Permanent.** `u64::MAX` is not a height: permanent entities are not
  counted and never spill, and no finite expiry spills into `u64::MAX` (a
  full `u64::MAX − 1` fails).
- **Generic records** (simulator chains) follow the same rule, and their
  search also stops at `h + max_lifetime`: a full last admissible height
  fails rather than exceeding the maximum lifetime.
- **Cost.** The probes are host reads of the schedule counters and are not
  charged gas; they are bounded at 65 per operation. Filling the heights that
  make another user's write probe costs the entities' gas and rent.
- **Rent.** Rent is charged for the height asked for, `E`; the blocks from
  `E` to the recorded height are free (at most 64). A later extension is
  charged from the recorded height, so no block is paid twice. There are no
  refunds on delete, so the free blocks cannot be turned into a payout. A
  non-owner's extension is checked against the rent cap by the height asked,
  so its recorded height can exceed `h + cap_blocks` by up to 64 blocks.
- **Estimation.** `eth_estimateGas` and the admission quote price the height
  asked, which is what execution charges.

When the search fails, `eth_call` and `eth_estimateGas` answer `execution
reverted` with a Solidity `Error(string)` in `data`, for example `Arkiv:
operation 0 cannot expire at block 50: blocks 50 to 114 each already have the
128 expiring entities one block may have; choose another expiry`. The SDK
shows it as the revert reason when it replays a reverted transaction.

## What applications observe

- **Events.** `EntityCreated` and `ExpiryExtended` carry the recorded height.
  SDK 1.0 returns that value as `expiresAt` from `createEntity` and
  `extendEntity`; it does not compare it with what was asked.
- **Reads.** `arkiv_getEntity`, `arkiv_query` (`$expiresAt`, also as a filter
  or sort key), proofs and the metadata feed all read the stored cell, so
  they agree with the events.
- **Up to 64 blocks late.** An application must read the expiry back rather
  than compute it; an entity lives at least as long as asked, never less.
- **No purge event.** The sweep emits no log, as on the Arkiv network. An
  entity is gone from every read from its expiry block on; indexers track
  `$expiresAt`. The native block outcomes and the metadata feed do list each
  deletion (`expireRecord`).
- **Receipts.** A block's `gasUsed` includes the sweep's units; a
  transaction's does not.

## Genesis prefill

Prefill rows are explicit and stay strict: a row whose height already holds
`expiry_per_block` rows is an error at build time, never moved, in the small
builder, the bulk builder and the streaming builder alike. Permanent rows
(`u64::MAX`, in a permanent-prefill launch) are not counted. The Sourcify
converter packs 16 rows per height, which fits any `expiry_per_block >= 16`.
See [genesis bundle](genesis-bundle-v1.md).

## Compared with the Arkiv network

Upstream (`arkiv-network/arkiv`, `docs/api-spec.md` and
`crates/arkiv-reth-payload-builder`) expires logically and purges lazily: an
entity is dead once the block number reaches `expiresAt`, and the block
producer later removes at most 10 (`MAX_PURGE_KEYS`) per block with a
`purgeExpired` protocol transaction whose keys it selects from a local
database. Any number of entities may share an expiry block.

| | This node | Arkiv network |
| --- | --- | --- |
| Removal | exactly at `expiresAt`, by the sweep | later, 10 per block, producer-selected |
| Expired entities in state | never | until purged; reads filter them |
| Entities per expiry block | `expiry_per_block` (1 to 128), then spill | unbounded |
| `expiresAt` vs. asked | equal, or up to 64 blocks later | equal |
| Which entities a block removes | a function of the state | the producer's choice |
| Purge event | none | none |

This node keeps exact purge because state then holds only live entities:
counts come from index sizes, proofs and the state root need no liveness
rule, removal is replayable from the state alone, and deletion keeps up with
any write rate instead of falling behind at 10 per block. The price is the
per-height bound, which the spill rule turns from a failed write into a
slightly later expiry.

## Open items

- The spill probes are bounded but not charged. Charging them needs a host
  charge beside the engine receipt.
- A delete costs the same units whatever the size of the entity's unindexed
  values, but a 128 KiB value makes it four times slower. Either the delete
  path should stop reading the value or the reservation should price it.
- A per-operation "fail instead of spill" flag, for applications that need an
  exact expiry block, is not offered.
- `revert_data` builds the `ExpiryNotExtended` arguments from the heights
  asked, not the recorded ones, for operations earlier in the same batch.
- The admission quote's view of an entity created earlier in the same
  transaction uses the height asked, so a later extension in that transaction
  can be quoted a few blocks of rent off.
