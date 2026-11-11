# Proposal: exact expiry with a per-block cap and spill

Status: draft, 2026-11-17. The rule is being implemented in this repository
(issue #9); sweep timings and the SDK check are pending and will be added
here. Written for the people designing Arkiv's expiry and purge.

## Summary

Delete every entity exactly at its recorded expiry block, bound the deletions
of one block by a chain parameter, and when a write aims at a block that is
already full, move its expiry to the next block with room instead of
rejecting it. Deletion work per block stays bounded, no expired entity is
ever left in state, and writes do not fail under ordinary load.

## The problem

Removing expired entities costs work, and a block can only do so much of it.
The two designs in use bound it differently, and each has a cost.

**Arkiv today: logical expiry, lazy purge.** An entity is dead once the block
number reaches `expiresAt`; the producer later removes at most 10 per block
(`MAX_PURGE_KEYS`) with a protocol transaction (`tokenomics_v1.md` §2.10).

- The purge rate is 10 per block, 40 per second at 250 ms blocks. A small
  benchmark created 499 entities per second on `arkiv-reth-dev`
  ([results](../benchmarks/2026-11-17-sdk-side-by-side.md)). At that rate the
  backlog grows by about 460 dead entities per second once they start
  expiring, and it only shrinks when writes stop.
- Dead entities stay in the state and its indexes, so every read, count and
  proof has to exclude them, and the state root commits to rows that no
  longer exist for applications.
- The producer chooses the keys from a local side database, so which entities
  a block removes is not a function of the state.
- The purge is unfunded (`purge_gas` is 0).

**This node until now: exact expiry, hard cap.** The sweep at the start of
block `h` deletes every entity with `expiresAt == h`. To bound it, at most
`expiry_per_block` (16) entities may share an expiry block; a write that
would exceed it fails with `ExpiryCapacityFull`. Applications give every
entity the same lifetime, so all creates of one block share an expiry block:
of 8 concurrent 16-create transactions, 7 reverted.

## The rule

Parameters, fixed in the chain's genesis:

- `C` = `expiry_per_block`: the most entities that may expire at one block
  (1 to 128; proposed default 128, the most operations a block can hold).
- `W`: the spill window, in blocks (proposed 64).

For an operation that sets an entity's expiry (create, extend) and resolves,
by the existing rules, to block `E`:

1. Let `E'` be the smallest block in `E ..= E + W` that fewer than `C`
   entities expire at. Record `expiresAt = E'`.
2. If every block in the window is full, the operation fails with
   `ExpiryCapacityFull`.
3. A permanent entity has no expiry block and never spills.

The sweep is unchanged: block `h` deletes exactly the entities recorded at
`h`, at most `C` of them, before any user transaction.

Rent is charged for the requested block `E`; the blocks up to `E'` are free.
The event, the receipt and every read report `E'`.

## Properties

- **Bounded work.** A block deletes at most `C` entities; the block budget
  reserves `C × max deletion cost`, as it does today.
- **No dead state.** An entity is in the state if and only if it is live, so
  reads, counts and proofs need no liveness filter and the state root commits
  only to live entities. Counts can be answered from index sizes.
- **Deterministic.** `E'` is a function of the state (the per-block expiry
  counts), so every node resolves the same block.
- **No hidden backlog.** Where the lazy purge lets dead entities pile up, this
  rule pushes expiry later, visibly, by at most `W` blocks, and then refuses
  writes. With `C` at least the operations a block can hold, creates that
  share a lifetime never spill at all: one block cannot add more than `C`.
- **Lifetime is a floor.** An entity lives at least as long as it paid for,
  never less.

## What it costs

- `expiresAt` can be up to `W` blocks later than requested (16 seconds at
  250 ms blocks). A client must read it back rather than assume it.
- An application that needs an entity gone by an exact block cannot rely on
  it when blocks are full. A per-operation "fail instead of spill" flag would
  cover that.
- Spilled blocks are stored without rent.
- Each probed block is a state read, so a spilling write costs up to `W`
  extra reads; these are metered.
- The expiry schedule (entities per block) is consensus state. This node
  already keeps it; Arkiv's executor does not.

## Open questions

1. The values of `C` and `W`. `C = 128` needs the sweep of 128 worst-case
   entities to fit the block time; measurement pending.
2. Whether spilled blocks should be charged.
3. Whether the SDK or any application compares the requested expiry with the
   reported one; check against SDK 1.0.1 pending.
4. Whether to offer the "fail instead of spill" flag from the start.
5. For Arkiv itself: adopting this means exact deletion in the executor and
   an expiry schedule in state, replacing `purgeExpired` and the producer's
   pruning database.
