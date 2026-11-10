# SDK 1.0 compatibility: open items

The node serves what `@arkiv-network/sdk` 1.0.x sends (see
[SDK compatibility](sdk-compatibility.md)): `arkiv_getEntity`, `limit`,
`offset`, `select.count`, `ORDER BY`, `BETWEEN` and ranges on every
indexable type. This page records what is still open: divergences from the
upstream node ([`arkiv` `docs/api-spec.md`](https://github.com/Arkiv-Network/arkiv)),
costs, and follow-ups. Nothing here blocks the SDK smoke.

## Ordering

- **No cursor with `ORDER BY`.** Sorted pages are offset windows, as
  upstream's are (its cursor encodes `offset + limit`). They carry no
  `cursor`, a `cursor` beside `ORDER BY` is refused (`-32005`), and a sorted
  window past the 8 MiB response cap is refused (`-32004`) rather than
  shortened, since nothing could carry the cut rows. Consistent sorted paging
  needs `atBlock`, which the SDK does not send. A keyset variant was built
  and set aside: the cursor was the last row's sort value and record id, and
  a page kept `limit + 1` matches after it. It can come back if sorted
  cursors are wanted.
- **Fetch-and-sort cost.** Every sorted page reads the key and sort cell of
  every match (a named projection) and keeps the first `offset + limit` in a
  heap, so its cost grows with the match set, not the page; it is bounded only
  by the 25-million-unit read budget (`-32004` past it). For an indexed sort
  attribute held by every match (system attributes always are), an index walk
  over the term map in value order would read only the terms up to the window;
  golemdb's `docs/sort-implementation-plan.md` (phase 2, `IndexWalk`) has the
  design. `DESC` would need a reverse `terms` read in the engine.
- **Creation order inside one transaction.** Without `ORDER BY`, and for
  `ORDER BY` ties, matches come in record-id order: block, transaction,
  operation. Upstream orders by creating transaction, then entity key, so the
  two differ only among entities one transaction creates (an `executeBatch`
  with several creates). Record ids are committed state; matching upstream
  would mean allocating them in key order within a transaction, a consensus
  change.
- **`ORDER BY $createdAt`** is accepted here (as `$createdAt` predicates are);
  upstream answers a type error.

## Offset and count

- **Offset is a walk.** An offset reads the skipped matches' ids, one engine
  page (`max_page`, at most 64 on signed chains) at a time, all charged to the
  read budget. Skipping by posting sizes or an order-statistics trie would make
  deep offsets cheap; nothing does that yet.
- **A shortened unsorted page and SDK offsets.** An unsorted page whose
  entities pass the 8 MiB cap is shortened and continues by cursor. The SDK
  ignores cursors and computes the next offset as `n * size`, so it would skip
  the rows cut from such a page. Refusing instead of shortening when the
  request carries an `offset` would make the SDK see the problem.
- **Counts beyond the read bounds.** `select.count` of a query beyond the group
  and literal bounds counts by the bounded scan (at most 4,096 rows), as its
  page reads; `arkiv_getEntityCount` refuses the same query. An untyped
  `EXISTS(x)` (the SDK's `exists()`) expands to nine groups, past the genesis
  bound of eight, so it always takes the scan; an engine term for "an
  attribute of any type" would keep it on the index.
- **`select` defaults.** A `select` without `key` still returns the key unless
  it selects `count` alone; upstream turns every field off once `select` is
  given. The SDK always sends every key, so only hand-written clients see it.
- **`offset` beside a cursor** is ignored (the cursor carries its position);
  upstream leaves the combination unspecified and its implementation does the
  same.

## Supersets of the network

SDK 1.0 removed `ne()`/`!=`, `not()`/`NOT`, `atBlock`, cursors and
`getEntityCount()` because the network it targets answers at the head only
and has no negation. The node keeps all of them, plus `arkiv_getEntity`'s
optional block, `$createdAt` predicates and `limit` up to 200 (the SDK and
upstream stop at 100). Clients relying on them will not work against the
network; whether to keep, flag or drop each is open. SDK 1.0.1 maps only codes
`-32001` to `-32004` to `QueryError` kinds, so `-32005` (cursor) and `-32006`
(block) reach it as plain RPC errors.

## Language and parser

- **Two parsers.** `signed/rpc_reads.rs` has its own Boolean parser (to DNF,
  with `ORDER BY`), which hands each predicate to the adapted GPL copy of the
  upstream grammar in `simulator/query_language` (revision `0fd58ce5`). That
  copy gained `BETWEEN` and ranges on every type, modelled on upstream's
  `parse_between`; the `ORDER BY` clause is written by hand in `rpc_reads.rs`
  after upstream's `parse/order.rs` semantics, not copied. Upstream's
  `arkiv-query` crate is now MIT and has all of these; vendoring it would drop
  the GPL copy and the duplication.
- **Reserved words.** `between` is now a keyword in queries, but writes still
  accept it as an attribute name (a write rule is consensus), so such an
  attribute cannot be filtered on by name. The SDK also reserves `order`,
  `by`, `asc` and `desc`; the node reads them as the clause only where it
  stands.

## Expiry and permanence

SDK 1.0 removed `ExpirationTime.permanent()` and refuses lifetimes past its
`MAX_LIFETIME` (7,884,000 blocks) client-side. This node still admits
permanent entities and caps rent at a year (63,072,000 blocks at 500 ms), so
the SDK smoke no longer covers permanence or `PermanenceOwnerOnly`; the Rust
tests do. Whether chains should advertise the SDK's lifetime cap is open.
