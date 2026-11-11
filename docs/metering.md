# Metering: operation classes, charge points and receipts

Every data-plane operation of the engine counts deterministic cost units
(`crates/arkiv-db/src/meter.rs`). The counting rules below are normative:
receipts are compared byte for byte by replaying nodes, and the test suite
pins the literal vectors listed here. The read charges of proofs are in
[equality proof charges](eq-proof-charges.md) and
[range proofs](range-proofs.md); the record and query semantics the charges
refer to are in the [query model](query-model.md).

Metering is distinct from gas pricing. The host converts write receipts to
gas and requires a budget for consensus-path writes. Read and query receipts
are for local RPC limits or optional API billing; they never enter consensus
and do not imply a gas charge for reading.

The meter (counting-rule `MODEL_VERSION = 3`) sits at the logical operation
boundary above the maps, not at elapsed time, allocator calls or node
comparisons.

## Operation classes and fixture weights

Classes (`OperationClass`, 31 entries in ledger order): `DirectoryLookup`,
`NamespaceCreate`, `NamespaceTransfer`, `PutRecord`, `DeleteRecord`,
`WriteKeyByte`, `WriteValueByte`, `PointRead`, `ScanSeek`, `ScanEntry`,
`ReadByte`, `RecordCreate`, `RecordPatch`, `RecordDelete`, `CellWrite`,
`CellByte`, `CellRemove`, `IndexInsert`, `IndexTermCreate`, `IndexRemove`,
`QueryGroup`, `TermSeek`, `TermVisit`, `PostingEntry`, `RowVisit`,
`RecordProbe`, `ResultRow`, `RootUpdate`, `RowByte`, `ProofNode`,
`ProofByte`.

Fixture weights (`PriceSchedule::FIXTURE`, schedule id 1). **Uncalibrated**:
they exist so receipts can be checked by hand and must not be read as
production prices.

| Class | Weight | Class | Weight | Class | Weight |
| --- | --- | --- | --- | --- | --- |
| `DirectoryLookup` | 1 | `RecordCreate` | 20 | `QueryGroup` | 5 |
| `NamespaceCreate` | 50 | `RecordPatch` | 10 | `TermSeek` | 4 |
| `NamespaceTransfer` | 20 | `RecordDelete` | 20 | `TermVisit` | 3 |
| `PutRecord` | 10 | `CellWrite` | 4 | `PostingEntry` | 1 |
| `DeleteRecord` | 5 | `CellByte` | 1 | `RowVisit` | 1 |
| `WriteKeyByte` | 2 | `CellRemove` | 4 | `RecordProbe` | 2 |
| `WriteValueByte` | 1 | `IndexInsert` | 6 | `ResultRow` | 3 |
| `PointRead` | 3 | `IndexTermCreate` | 30 | `RootUpdate` | 2 |
| `ScanSeek` | 4 | `IndexRemove` | 6 | `RowByte` | 1 |
| `ScanEntry` | 2 | | | `ProofNode` | 2 |
| `ReadByte` | 1 | | | `ProofByte` | 1 |

## Root maintenance charges

The root-maintenance and proof classes:

| Class | Counts | Charged by |
| --- | --- | --- |
| `RootUpdate` | Each logical map-entry write that authenticated state must rehash: a raw entry, a key binding, a row, a posting entry, a term leaf or the namespace's catalog leaf | Every write, inside the group it already charged (table below) |
| `RowByte` | Each byte of the stored row a typed record write leaves behind: key bytes + Σ over the row's cells after the write of (name bytes + encoded value bytes), the same measure as a full-projection `p` | `CreateRecord` and `PatchRecord` record groups |
| `ProofNode` | Reserved in model 3; model-4 Eq proofs count visited witness nodes including inline nodes | [Eq proof reads](eq-proof-charges.md) |
| `ProofByte` | Reserved in model 3; model-4 Eq proofs count all emitted binary evidence bytes | [Eq proof reads](eq-proof-charges.md) |

Why these two write charges. Root maintenance is prepaid per logical entry
write: `RootUpdate` counts entries, not trie nodes, so a receipt depends only
on the operation and the logical state, never on the shape of a structure,
a cache or the disk layout. `RowByte` prepays re-encoding and rewriting the
whole row leaf: a patch rewrites every byte of its row even when it sets one
small cell, which `CellByte` (the bytes written) does not cover. The
`RootUpdate` counts are the logical entry writes: a namespace creation or
transfer writes the catalog leaf (1); a raw put or delete writes the raw
entry and the catalog leaf (2), a delete whether or not the key existed; a
record creation or deletion writes the key binding, the row and the catalog
leaf (3); a patch writes the row and the catalog leaf (2), the key binding
being unchanged; an index insert or removal writes the posting entry and
the term leaf (2). The engine computes the roots these
charges prepay. The host-owned map (`Branch::try_host_put` and its siblings)
is unmetered, and the engine does not limit how often the host writes it.
The bound is the host's own rule: the block executor
(`arkiv_node::execute_block`) writes at most 4 due-count entries per user
operation (a patch moving a record's expiry height updates the old and new
due and total entries) and at most 2 per swept (height, namespace) (the
removed due entry and, once per height, the removed total).

The work behind one `RootUpdate` is bounded by the longest key of each map
(`L` bytes, `2L` nibbles). A lookup path holds at most `2L + 1` nodes,
but the prepaid work is the nodes an entry write **creates**, which the next
root computation encodes and, at 32 bytes or more, hashes: an insert creates
at most `2L + 2` (the path plus a leaf or extension split below the new
branch), a replacement `2L + 1`, a removal `2L` (see the
[state trie notes](state-trie.md#nodes-an-update-creates), where the trie
crate reaches each bound exactly):

| Map | Longest key, bytes | Nodes created per entry write, at most (`2L + 2`) |
| --- | --- | --- |
| Catalog (`NamespaceId`) | 8 | 18 |
| Rows and postings (`RecordId`) | 8 | 18 |
| Host map (raw `[u8; 32]`, uncharged) | 32 | 66 |
| Key bindings (`len ‖ bytes`) | 33 | 68 |
| Raw records (escaped key and terminator) | 41 | 84 |
| Terms under the default limits (name and terminator, tag, presence, a 128-byte `Str` and terminator) | 164 | 330 |

Keys are variable-length and prefix-free (see the
[key encodings](experimental-prekucki-v1.md)); a term key under the hard
caps (`Value::INDEXED_LEN_CEILING = 512`) is at most 548 bytes.

Every namespace-local operation also rewrites its namespace's catalog leaf,
which is one of its `RootUpdate` units (the catalog row above), and a block
adds one state-header digest. `crates/arkiv-db/tests/rehash.rs` commits every
block of a 20-block mixed workload to an object store, reopens it, and counts
per map the new nodes each block hashes (one keccak256 each when the root is
computed) and the distinct objects it stages: each map's hashed nodes stay
below its entry writes times its bound, and its staged objects never exceed
its hashed nodes (identical nodes built by different writes, such as the
one-entry posting maps of a new record's terms, are hashed each time and
stored once). Crafted deep paths reach the bounds closely: 66 record keys
that branch at every possible nibble make one more key rehash exactly 66
key-map nodes (bound 68; the new leaf is inline), and 214 terms whose spine
term branches at every nibble a valid term can branch on (a 175-node path)
make one more record holding the spine term rehash exactly 176 term nodes.
These are logical counts, not time: hashing cost per node grows with node
size and no throughput is claimed.

Two properties of the fixture that a production schedule must keep:

- `IndexTermCreate > 0`. Creating a distinct term must cost more than adding
  a membership to an existing posting; the engine charges the class whenever
  a term is new, but only a positive weight makes the rule hold. This is a
  schedule invariant, not something the counting rules can enforce.
- Symmetric structural delete weights: `RecordDelete = RecordCreate`,
  `CellRemove = CellWrite` and `IndexRemove = IndexInsert`, so a delete costs
  the create minus its bytes (`CellByte`, `RowByte`) and term creation; the
  `RootUpdate` counts of a create and of the delete of the same record are
  equal. Expiry deletion runs through the same charges under host authority;
  Arkiv funds it through its create/lifetime policy.

## Charge points

Charge points, in order; each charge precedes the work it covers. Braces mark
an atomic group: either every class in it fits and is charged, or nothing in
the group is charged and the operation stops. `k` and `v` are key and value
lengths in bytes; `c` = cells of the record, `a` = its attribute cells, `s`
and `r` = a patch's set and remove counts, `b` = Σ over the cells written of
(name bytes + encoded value bytes), `n` = the stored row after the write =
key bytes + Σ over the row's cells after the write of (name bytes + encoded
value bytes), `p` = projected bytes = key bytes if the key is projected + Σ
over projected cells of (name bytes + encoded value bytes), `g` = groups of
a query.

| Operation | Charges and checks, in order | Total under FIXTURE |
| --- | --- | --- |
| `CreateNamespace` | `DirectoryLookup` 1; `DirectoryFull` check; {`NamespaceCreate` 1, `RootUpdate` 1}; allocate ordinal, store validated name, owner = actor, revision 0 | 53 |
| `TransferNamespace` | `DirectoryLookup` 1; `UnknownNamespace`, then `Unauthorized`, then `StaleRevision` (each rejects with spent 1); {`NamespaceTransfer` 1, `RootUpdate` 1}; owner := new owner, revision += 1; record map and typed store shared | 23 |
| `Put` | `DirectoryLookup` 1; `UnknownNamespace`/`Unauthorized` (spent 1); {`PutRecord` 1, `WriteKeyByte` k, `WriteValueByte` v, `RootUpdate` 2}; insert | 15 + 2k + v |
| `Delete` | `DirectoryLookup` 1; `UnknownNamespace`/`Unauthorized`; {`DeleteRecord` 1, `WriteKeyByte` k, `RootUpdate` 2}, charged even if the key is missing; remove (a missing key succeeds with `existed = false`) | 10 + 2k |
| `get` | `DirectoryLookup` 1; `UnknownNamespace`; `PointRead` 1; lookup; if present `ReadByte` v, charged before the value is copied | present 4 + v; missing 4 |
| `scan_prefix` | `DirectoryLookup` 1; `UnknownNamespace`; `ScanSeek` 1; seek; per matching entry {`ScanEntry` 1, `ReadByte` k_i + v_i} charged before the entry is materialized | 5 + Σ(2 + k_i + v_i) |
| `descriptor` | `DirectoryLookup` 1; `UnknownNamespace` | 1 |
| `CreateRecord` | `DirectoryLookup` 1; `UnknownNamespace`/`Unauthorized` (spent 1); `PointRead` 1 (key binding); `AlreadyExists` (spent 4); `CounterExhausted` (spent 4); {`RecordCreate` 1, `CellWrite` c, `CellByte` b, `RowByte` n, `RootUpdate` 3}; per attribute cell in name order, its value term then its existence term: lookup, {`IndexInsert` 1, `IndexTermCreate` 1 if the term is new, `RootUpdate` 2}, insert; bind key, store row. No key-byte charge beyond `RowByte` | 30 + 4c + b + n + 20a, + 30 per new term |
| `PatchRecord` | `DirectoryLookup` 1; `UnknownNamespace`/`Unauthorized`; `PointRead` 1; `NotFound` (spent 4); `RecordFull` (spent 4, recorded) if the result would exceed `max_cells`; {`RecordPatch` 1, `CellWrite` s, `CellByte` b, `CellRemove` r, `RowByte` n, `RootUpdate` 2}; per name in sorted order over set ∪ remove, with old = the terms the record's cell holds and new = the terms the resulting cell holds: {`IndexRemove` 1, `RootUpdate` 2} per term in old − new (value term first), then per term in new − old: lookup, {`IndexInsert` 1, `IndexTermCreate` 1 if new, `RootUpdate` 2}, insert; store row | 18 + 4s + b + 4r + n, + 10 per removed term, + 10 per inserted term, + 30 per new term |
| `DeleteRecord` | `DirectoryLookup` 1; `UnknownNamespace`/`Unauthorized`; `PointRead` 1; `NotFound` (spent 4); {`RecordDelete` 1, `CellRemove` c, `RootUpdate` 3}; per attribute cell in name order: {`IndexRemove` 1, `RootUpdate` 2} (value term), {`IndexRemove` 1, `RootUpdate` 2} (existence term); unbind key, remove row | 30 + 4c + 20a |
| `get_record` | `DirectoryLookup` 1; `UnknownNamespace`; `PointRead` 1; unbound → `Ok(None)` (spent 4); {`ResultRow` 1, `ReadByte` p}; materialize the row | bound 7 + p; unbound 4 |
| `query` | `DirectoryLookup` 1; `UnknownNamespace`; per group `QueryGroup` 1 plus the source opening and enumeration below; per emitted row {`ResultRow` 1, `ReadByte` p} | 1 + 5g + 4 per seek + 3 per term visited + 1 per candidate + 2 per probe + Σ(3 + p) over rows |
| `count` | as `query` with no cursor and no row charges, until every group is exhausted | 1 + 5g + seeks + term visits + candidates + probes |
| `terms` | `DirectoryLookup` 1; `UnknownNamespace`; `TermSeek` 1; per returned term, after the node step that reveals it and before it is decoded, {`TermVisit` 1, `ReadByte` encoded value bytes (1 `Bool`, 8 `I64`/`U64`, byte length `Str`)}; an `Excluded` lower bound equal to an existing term and the key that ends the `(name, type)` run are uncharged; a missing `(name, type)` costs the seek only; stops after `limit` terms with no peek charge | 5 + Σ(3 + bytes) |

Unchanged terms of a patch cost nothing: the same value charges nothing; the
same `(name, type)` with a new value removes and inserts the value term and
leaves the existence term untouched; a type or kind change removes and
inserts both terms. Term newness follows one rule: a value term `(name,
type, value)` is new iff no live record of the namespace holds an attribute
with exactly that name, type and value at the moment of the insert; an
existence term `(name, type)` is new iff no live record holds an attribute
of that name and type.

## Query evaluation

Query evaluation is lazy and positioned after the cursor; every source yields
ids greater than `after` only:

1. `DirectoryLookup` 1; `UnknownNamespace`.
2. For each group in order, open it: `QueryGroup` 1, then by the group's
   first literal. An empty group (match-all) or a negated first literal has
   no seek: the row map is its source. `Eq`: `TermSeek` 1 on the value term.
   `Exists`: `TermSeek` 1 on the existence term. `NotEq`: `TermSeek` 1 on the
   existence term of `(name, value type)`. `Range` and `StartsWith`:
   `TermSeek` 1, then `TermVisit` 1 for each term key of `(name, type)` whose
   value satisfies the bounds or starts with the prefix, in key order; the
   existence term, a stepped-over `Excluded` endpoint, the key that ends the
   run and other types are never charged. A missing term or an empty run
   costs the seek only.
3. For each group in order, pull its first head. Candidates come from the
   source in id order, each charged `PostingEntry` 1 (a posting or a merged
   run) or `RowVisit` 1 (the row map) as it is revealed. Each candidate is
   probed candidate-major by the group's probe list in order until a probe
   fails, `RecordProbe` 1 per probe. The probe list is the first literal
   itself when it is `NotEq` or negated (its source is only a domain),
   followed by every further literal of the group in order; a negated
   further literal is just a probe. A candidate that fails a probe is not
   probed by later literals. The head is the first candidate passing every
   probe.
4. Merge loop: let m be the smallest head over the groups. If there is
   none, the page is done with `next = None`. If the page already holds
   `limit` rows, it is done with `next = Some`, `after` = the last row's
   id. Otherwise charge {`ResultRow` 1, `ReadByte` p}, materialize row m,
   then advance every group whose head equals m, in group order (each pull
   charges as in step 3 until the group's next head or its exhaustion), and
   repeat.

`count` runs steps 1 to 3 with no cursor, then the merge loop without row
charges until every head is exhausted; the total is the number of distinct
ids emitted.

Consequences. An `Eq` page of `limit` rows over a posting with more entries
charges `limit + 1` `PostingEntry` (the head beyond the page); the last page
charges exactly the remaining entries. A group is advanced only when its head
is emitted, so a group whose head lies beyond the page end does no further
work. The cost statements are these. Positive `Eq`/`Exists` sources, the
row map (match-all, negation) and the existence posting (`NotEq`) are
positional: a page charges the candidates scanned to fill it plus each
group's first match beyond it. A `Range`/`StartsWith` source additionally
charges `TermSeek` 1 and `TermVisit` 1 for every qualifying term of the run
on every page (the id-ordered merge consults every qualifying term's
posting per page), so a walk of P matches over T qualifying terms with page
limit L charges ceil(P/L) × T `TermVisit` in total; T is bounded by the
namespace's own distinct values. Match-all, negated and `NotEq` first
literals are linear in the namespace's own live rows or existence posting
per page scanned. None of this depends on other namespaces. A page carries
no total because a total would need the full walk these rules avoid;
`count` is the separate metered read for it. As written, nothing is
reordered or planned: the charged cost and the abort point are a pure
function of the state and the query text, so a caller chooses each group's
index source by putting the selective positive literal first. A local read
may instead opt in to planned evaluation (below), which every read of block
execution, the expiry sweep included, never does. A budget end anywhere rejects with
`OutOfBudget`, the units spent so far and no partial page. Every per-entry
class (`PostingEntry`, `TermVisit`, `RowVisit`, `RecordProbe`) is charged one
unit at a time immediately before the entry is used, after the node step that
reveals it, like `scan_prefix`: an `OutOfBudget` at entry k has charged
exactly k−1 entries of that kind.

## Planned evaluation

Planned evaluation (local reads only). A query or count whose budget opts
in (`Budget::planned`) returns exactly the rows, order, `next` claims and
counts above, but chooses each group's index source by size; its charges are
again a pure function of the state and the query (sizes are the posting entry
counts stored in the term leaves), under these rules (the full statement is
the `typed::planner` module documentation):

1. Per group `QueryGroup` 1. A group no record can satisfy by the
   one-cell-per-name rule (two types of one name, two `Eq` values of one
   name, `Eq` with `NotEq` of the same value, a literal with its negation, a
   positive literal with `NOT Exists` of its name and type) costs nothing
   more. Otherwise, in written order, `TermSeek` 1 per term lookup: positive
   `Eq`/`Exists` (a missing term empties the group), positive `NotEq` (its
   existence term, then its value term), negated `Eq`/`Exists`. `Range`
   literals on one `(name, type)` form one run over their intersected bounds;
   each run is walked as above (`TermSeek` 1, `TermVisit` 1 per qualifying
   term) and summed, stopping (the run becomes a row probe) once its matches
   plus terms reach the cheapest source so far or after 65,536 terms while
   another source competes.
2. The driver is the smallest positive posting or the cheapest completed run
   (ties: a posting, then the earlier literal); with neither, the smallest
   `NotEq` existence posting, else the row map. Other positive postings
   follow, smallest first, except one holding every live record; absent
   postings (negated `Eq`/`Exists`, a `NotEq`'s value term) are probed largest
   first; the remaining literals are row probes in written order.
3. Candidates come from the driver as before (`PostingEntry` 1 or
   `RowVisit` 1 each). Each follower is moved to the candidate by one step
   (`PostingEntry` 1, only for a follower at most twice the driver's size)
   and otherwise one repositioning from its root (`RecordProbe` 1); a
   follower beyond the candidate moves the driver to its entry (a posting
   driver steps once, then repositions with `RecordProbe` 1), and an
   exhausted follower ends the group. An absent
   posting is probed by id (`RecordProbe` 1); row probes load the row once,
   `RecordProbe` 1 per literal.
4. `count`: pairwise disjoint groups (by the rule of step 1 on their union)
   are summed; a group left with its driver alone is its posting size, its
   run's summed matches or the live row count, less absent postings that are
   pairwise disjoint and inside the driver; any other group, and overlapping
   groups, are enumerated as in step 3 without row charges.

Under the vectors below, a planned count of match-all is 1 + 5 = 6, of
`Exists(price, I64)` 1 + 5 + 4 = 10, and of `NotEq(kind, "pen")` 1 + 5 + 4 + 4
= 14 (the existence posting of 3 less the value posting of 1: 2).

## Covered work

Covered work. The following bounded steps are not charged separately; each
is covered by the charge next to it, and none can grow with another
namespace or with uncharged data:

- The term-map lookup that decides whether `IndexTermCreate` applies precedes
  the `IndexInsert` group and is covered by it: one O(log T) lookup over the
  namespace's own terms.
- The row-map load after a key lookup is covered by `PointRead`: two
  O(log n) lookups over the namespace's key and row maps for one unit.
- The patch cell merge (the record's sorted cells with the sorted `set`,
  minus `remove`, and the resulting-size check) is one linear merge,
  O(cells + set + remove) with at most 2 × `max_cells` entries, and
  precedes the `RecordPatch` group; the `RowByte` count `n` is a sum over
  the merged cells, computed before the group is charged. A create sums `n`
  over its own sorted cells before its group in the same way.
- Row emission computes p before cloning any cell, and a row already loaded
  by a probe or by a row-map source is reused for emission rather than
  loaded again (fewer `record_nodes`; charges unchanged).
- Each `TermVisit` covers one comparison of at most the indexed-string
  limit (128 bytes by default; fixed-width terms are at most 32 bytes) and
  the positioning of that posting after the cursor.
- The run merge (a heap over the run's postings) is O(log k) per charged
  `PostingEntry`, with k at most the charged `TermVisit`s.
- The union over groups (the minimum over at most `max_groups` heads per
  emitted row, and the advance of every group whose head was emitted) works
  only over charged candidates.
- Admission validation is O(`max_groups` × `max_literals` + `max_cells`)
  before any charge, bounded by the instance parameters.
- The node step that ends a run (the first key outside it) and a stepped-over
  `Excluded` endpoint are one uncharged step each.

## Counting vectors

A namespace with three records created in order `a`, `b`,
`c` under 1-byte keys, each with attributes `kind: Str`, `price: I64`,
`$expiresAt: U64(5)` and a field `note: Bytes("xyz")`; kinds `book`, `book`,
`pen`; prices 10, 20, 5:

- create `a` (4 cells, b = 46, n = 1 + 46 = 47, all 6 terms new): 1 + 3 +
  20 + 16 + 46 + `RowByte` 47 + 36 + 180 + `RootUpdate` 2 × (3 + 2·6) =
  379; create `b` (only `price: 20` new): 229; create `c` (b = 45, n = 46;
  `kind: pen` and `price: 5` new): 257.
- query `Eq(kind, "book")`, `Projection::ALL`, limit 1, no cursor: 1 + 5 + 4
  + `PostingEntry` 2 + 3 + `ReadByte` 47 = 62, rows [1], `next` after 1; the
  second page with that cursor: 1 + 5 + 4 + `PostingEntry` 1 + 3 + 47 = 61,
  rows [2], `next = None`.
- query `Range(price, Excluded(I64(5)), Unbounded)`, `ID_ONLY`, limit 10:
  1 + 5 + 4 + `TermVisit` 2 (10, 20) + `PostingEntry` 2 + `ResultRow` 2 =
  24, rows [1, 2].
- query `NOT StartsWith(kind, "bo")`, `KEY_ONLY`, limit 10: 1 + 5 +
  `RowVisit` 3 + `RecordProbe` 3 + `ResultRow` 1 + `ReadByte` 1 = 19, rows
  [3].
- count match-all: 1 + 5 + 3 = 9; count `Exists(price, I64)`: 1 + 5 + 4 + 3
  = 13.
- patch `a`: set `price := attribute U64(10)`, remove `[note, missing]`:
  1 + 3 + 10 + 4 + 13 + 8 + `RowByte` 40 (key 1 + `$expiresAt` 18 + `kind`
  8 + `price` 13) + `IndexRemove` 12 + `IndexInsert` 12 + `IndexTermCreate`
  60 (the `U64` value and existence terms are new) + `RootUpdate` 2 × (2 +
  4·2) = 183.
- `expire_records(ns, $expiresAt, block 5)` afterwards deletes 1 (now 3
  cells, 3 attributes: 4 + 20 + 12 + 36 + `RootUpdate` 2 × (3 + 6·2) =
  102), then 2 and 3 (4 cells, 3 attributes: 76 + 30 = 106 each), in
  (height, id) order (one height, 5, here; the `terms` and `Eq` page read
  receipts are discarded).
- `get_record` of `a` at the first snapshot, `ALL`: 1 + 3 + 3 + 47 = 54.

## Meter rule, receipts and budgets

Meter rule: `subtotal = count × price` (checked; overflow rejects with
`CostOverflow`); `next = spent + subtotal` (checked; overflow rejects with
`CostOverflow`); if `next > limit` reject with `OutOfBudget` and leave `spent`
unchanged; otherwise `spent = next`. An exact budget succeeds. A rejection at
the first charge is a zero-cost rejection. A budget end in the middle of a
scan or query rejects with `OutOfBudget`, the units spent so far and no
partial result. Every `execute` mutates an O(1) clone of the working state;
a rejected operation restores it, so its logical changes never apply.

Receipts and budgets:

- `Receipt { kind: Write | Read, model_version, schedule_id, spent, outcome:
  Applied | Rejected(reason), ledger: Option<Ledger> }` is canonical, derives
  `Eq`, and is issued for every operation, rejected or not. Receipts are never
  withdrawn by rollback or a discarded candidate.
- `Budget::limit(n)`, `Budget::unlimited()`, `.traced()`. A ledger is present
  only when tracing was requested; it aggregates entries per class in
  first-charge order and omits zero-count classes. Within an atomic group
  the classes are charged in the order the table lists them, so a record
  create's ledger shows `RowByte` and `RootUpdate` before its first
  `IndexInsert`, and later `RootUpdate` charges add to that line. Tracing
  never changes charges.
- Effective write limit: admission first. Malformed arguments
  (`InvalidArgument`) and a non-record operation on the host path are
  rejected before any charge with a receipt of spent 0 that is not recorded.
  Then, if the operation budget has a limit `l` and `l > remaining`, the
  operation is rejected with `ExceedsBlockBudget { remaining }` before any
  charge, with a receipt of spent 0 that is **not** recorded in the branch's
  receipts or spent total; the operation belongs in a later block. Otherwise
  the limit is the operation's limit if it has one, else the block's
  remaining budget. `remaining = block_limit − spent`, where `block_limit` is
  `u64::MAX` when `begin` was given no block budget.
- Block aggregate: the branch's `spent()` equals the sum of its recorded
  write receipts, including rejected operations, and equals the sealed
  candidate's `spent()`. Rejected operations' spent units count against the
  block. `RecordFull` is a post-charge rejection and is recorded like
  `NotFound` or `AlreadyExists`.
- Reads (`get`, `scan_prefix`, `descriptor`, `get_record`, `query`, `count`
  and `terms` on a branch or snapshot) are limited by their own budget
  only; they never deduct from the block and are never stored. Their
  receipts have `kind = Read` and are not gas. An invalid query or `terms`
  request, an oversized projection or a mismatched cursor is rejected at
  admission with a read receipt of spent 0.
- Pricing is pinned at the branch base: writes, estimates and branch reads
  use the base snapshot's schedule; reads at a snapshot use that snapshot's
  schedule. `Branch::set_pricing(schedule)` commits a new schedule into the
  candidate for branches based on the published result; its id must exceed
  the currently committed id (`StaleSchedule` otherwise), so every receipt
  names a unique pricing context, and it is rolled back by checkpoints like
  any other state.
- Estimation parity: `estimate` and `estimate_host` run the same code as
  `execute` and `execute_host`, including the admission checks and the
  block's current remaining budget, on a scratch O(1) copy; nothing is
  changed, recorded or charged to the block. They equal execution only for
  the same starting state, operation and pricing.
- Physical trace: `PhysicalTrace { directory_nodes, record_nodes,
  index_nodes }` is returned next to every receipt but is not part of it. It
  is non-canonical instrumentation of the trie layout for isolation
  measurements and changes with the data structure. Every trie node an operation examines counts
  once per step that examines it, inline children included, whether it was
  in memory, served by a cache or loaded: a lookup or update counts the
  key's path (a write's directory count is its lookup plus its update path),
  a removal also the sibling it loads when a branch collapses, and an
  iterator its seek path plus every node it steps into. `record_nodes`
  counts the selected namespace's byte map, key map and row map visits;
  `index_nodes` counts its term map and posting map visits (term lookups,
  run scans and posting traversals); a transfer visits zero record and
  index nodes. Iterators are lazy: a posting or the row map is stepped first
  at its group's first head pull, a term run steps each qualifying posting
  once as it opens. A scan or posting traversal that runs out of budget at
  entry k has charged k−1 entries; the nodes stepped into to reveal entry k
  were visited but its entry was neither charged nor returned. A traversal
  run to its end visits every node at or above its seek position once. An
  aborted `get` has the same trace as a successful one. A `query` or `count` rejected at
  any charge reports every node visited before that charge, once: the lookup
  path and the posting or row-map traversal of every group already open; the
  term run and the postings positioned by a `Range`/`StartsWith` run that
  stopped at a `TermVisit`; the steps of head pulls and advances; and the rows
  loaded for probes and emission. `trace_regression.rs` pins this.

## Outside these rules

- Projection is charged for the projected bytes only, and predicates are
  evaluated on loaded rows under `RecordProbe`.
- `begin`, checkpoint/rollback, seal and publication are host-controlled
  lifecycle operations, with work covered by the write schedule rather than
  separate caller charges. With structural sharing each of them is an O(1)
  clone or append in memory. Root maintenance is prepaid by `RootUpdate` and
  `RowByte`. Durable commit time and hashing time are not part of any
  receipt.
- A historical read is priced with the selected snapshot's schedule. Any
  different RPC pricing policy must be explicit and must not change which
  block supplies the query data or proof. A query result proof does not
  authenticate its cost receipt.
- The fixture weights are uncalibrated; production calibration of the engine
  schedule is not done. The node's gas rules are separate
  ([GLM gas](glm-gas-v1.md)).
