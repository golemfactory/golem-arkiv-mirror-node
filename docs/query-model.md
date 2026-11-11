# Record and query model

The typed record model and the query subset of the engine
(`crates/arkiv-db/src/record.rs`, `query.rs`; every type is re-exported from
`arkiv_db`). Term bytes decide index order, and the term map's keys carry
the same bytes, so the state root commits to them. Charges are in
[metering](metering.md); trie key layouts in
[key encodings](experimental-prekucki-v1.md); the wire codec and the type
table in the [block format](block-format.md).

## Records

- `RecordKey`: 1 to 32 caller-assigned bytes (`RecordKey::MAX_LEN = 32`),
  unique among the live records of a namespace. The canonical encoding (in the
  codec and as a trie key) is `len ‖ bytes`, 2 to 33 bytes, a bijection:
  `decode` rejects a length byte of 0 or above 32. Vectors: `b"a"` →
  `01 61`; a 32-byte key `k` → `20 ‖ k`; an empty key and a 33-byte key are
  not keys. The SDK's 32-byte entity keys fit as they are. Charges
  (`RowByte`, `ReadByte`, projected bytes) count the raw key length, never
  the encoding.
- `RecordId`: the dense per-namespace creation ordinal from 1, never reused.
  A deleted key recreated gets a new id, so a reused key never joins two
  histories. Ids are provisional until publication: rollback restores the
  allocator like the namespace ordinal. Display `rec:{n}`. The id is the
  only result order of queries.
- `CellName`: ASCII, 1 to 32 bytes (writes: `Limits::max_cell_name_len`); the first character is a letter or `$`
  followed by a letter; every later character is a letter, digit, `_`, `-`,
  `.` or `:`; case-sensitive, ordered bytewise. `#`, `@`, spaces and
  non-ASCII are invalid, so engine-reserved names are unreachable from the
  data plane. A `$`-name is an ordinary name to the engine.
- `TypeTag`: golem-db's cell type ids (`Bool = 1`, `Str = 2`, `Bytes = 3`,
  `U64 = 13`, `I64 = 17`, …; see `docs/block-format.md`), not the SDK's.
  `Bytes` is field-only; every other type is ordered by its term bytes;
  `Str` also supports prefixes.
- `Value`: 25 variants, one per `TypeTag`. The encoded length is the type's
  fixed width (1 for `Bool`, 8 for `I64` and `U64`, …) or the byte length
  (`Str`, `Bytes`). Term bytes are order-preserving: `Bool` `00`/`01`;
  signed integers big-endian with the sign bit flipped; unsigned integers
  big-endian; `Str` its UTF-8 bytes; none for `Bytes`. Vectors: `I64(-1)` → `7f ff ff ff ff ff ff ff`, `I64(0)` →
  `80 00 00 00 00 00 00 00`, `I64(i64::MIN)` → `00…00`, `I64(i64::MAX)` →
  `ff…ff`, `U64(258)` → `00 00 00 00 00 00 01 02`, `Str("ab")` → `61 62`.
  Values of different types never compare equal or order against each other.
  Indexed strings obey `Limits::indexed_value_len()` =
  `min(max_indexed_len, max_str_len)` (128 by default,
  `Limits::DEFAULT_INDEXED_LEN`; never above
  `Value::INDEXED_LEN_CEILING = 512`), measured in UTF-8 bytes, on writes,
  predicates, range bounds, prefixes and term bounds. Field values obey
  `max_str_len` and `max_bytes_len` (4096 by default).
- `Cell { kind: Attribute | Field, value }`. Attributes are indexed and
  filterable; fields are stored and returned only. Kind and type are bound
  per record, not per name. An attribute holding `Bytes` is invalid.
- `Row { id, key: Option<RecordKey>, cells }`: the id is always returned; the
  key and the cells are projected; cells are sorted by name.
- `Limits { namespace_cap: 2^32, max_cells: 64, max_cell_name_len: 32,
  max_str_len: 4096, max_bytes_len: 4096, max_indexed_len: 128,
  max_page: 200, max_groups: 8, max_literals: 8 }` (`Limits::DEFAULT`) are
  instance parameters, fixed for an engine's lifetime
  (`Engine::with_limits`, `Engine::limits`, `Branch::limits`;
  `Engine::with_namespace_cap` changes only the cap) and checked at admission
  before any charge. `max_cells` also bounds the names of a `Named`
  projection. `Limits::MAX_RAW_KEY_LEN = 20` is a hard bound on the local
  raw byte key and scan prefix: the namespace ID is passed separately. Raw
  `Put`, `Delete`, `get` and `scan_prefix` reject longer inputs with
  `InvalidArgument(KeyTooLong)` before lookup, charging or recording a write.
  Raw values keep their byte charges; the separate low-level `Database`
  facade keeps its arbitrary-byte storage contract.
- `Term::{Eq { name, value }, NotEq { name, value }, Range { name, lower,
  upper }, StartsWith { name, prefix }, Exists { name, tag }}`; `Literal {
  term, negated }`; `Query { groups: Vec<Vec<Literal>> }`, an ordered flat DNF
  (`Query::match_all()` is one empty group); `Projection { key, cells:
  CellSelect::{None, All, Named} }` with `ALL`, `ID_ONLY` and `KEY_ONLY`;
  `Cursor` (opaque; exposes `after`, `snapshot`, `namespace`, `query`,
  `projection`); `QueryPage { rows, next: Option<Cursor> }`.

There is no engine-assigned key mode: every key is caller-assigned.

## Query semantics

Semantics, for one live record with cell `c` under the term's name:

- `Eq`: `c` is an attribute of the value's type holding that value. `NotEq`:
  an attribute of that type holding another value; a record lacking the
  attribute, or holding another type, does not match. `Range`: an attribute
  of the bounds' type inside the bounds, honouring `Included`, `Excluded` and
  `Unbounded`; a lower bound above the upper bound is simply empty.
  `StartsWith`: a `Str` attribute whose UTF-8 bytes start with the prefix
  bytes; an empty prefix matches every `Str` attribute of that name.
  `Exists`: an attribute of that type.
- Field cells never satisfy a term. A negated literal is the complement over
  all live records of the namespace. A group is the AND of its literals, the
  query the OR of its groups, an empty group matches every live record. The
  result is the matching live records in ascending id, the only order.
- A page holds the first `limit` matches with an id greater than the
  cursor's `after` (all matches from the start without a cursor). `next` is
  `Some`, with `after` set to the last row's id, if and only if at least one
  further match exists: an exact claim. A page carries no total;
  `count(namespace, query, budget)` is a separate metered read returning the
  number of distinct matches. The page limit is not bound by the cursor, so
  the pages of one walk may use different limits.
- `get_record(namespace, key, projection, budget)` returns `None` for an
  unbound key; `Named` projects only the names the record has.
- `terms(namespace, name, tag, lower, limit, budget)` returns the distinct
  indexed values of attribute `name` with type `tag` held by live records,
  in value order starting at `lower` (`Included`, `Excluded` or
  `Unbounded`), at most `limit` of them: a positional read of the term map
  that charges the seek and the returned terms only (see the [charge table](metering.md#charge-points);
  `Value::from_term_bytes` is the inverse of `term_bytes`).
- Create: `AlreadyExists` if the key is bound; otherwise the next id is
  minted, the cells stored sorted by name and the attribute cells indexed.
  Patch: `NotFound` if unbound; `set` replaces or adds cells (kind and type
  may change), `remove` drops names (an absent name is a charged no-op); the
  result may hold at most `max_cells` cells (`RecordFull`, decided after the
  key lookup and therefore recorded); id and key never change. Delete:
  `NotFound` if unbound; unbinds the key and removes the row and every index
  membership. An emptied term is dropped from the term map, so deletes,
  patches and rollback make terms new again.
- Cursor binding: a cursor from `Snapshot::query` binds the engine instance
  and the snapshot id; one from `Branch::query` binds the branch and its
  mutation serial, which advances on every applied `execute`/`execute_host`
  (rejected operations do not advance it), every `rollback_to` and every
  `set_pricing`. Both also bind the namespace, query and projection. Any
  mismatch is `Rejection::CursorMismatch` at admission: spent 0, receipt
  issued, nothing recorded. A cursor never re-resolves head: the caller pins
  the snapshot the cursor names, so a walk is identical whether or not new
  snapshots are published between its pages.
- Host path: `Branch::execute_host` and `estimate_host` accept
  `CreateRecord`, `PatchRecord` and `DeleteRecord` only; any other operation
  is `InvalidArgument(NotHostOperation)` at admission (spent 0, not
  recorded). The owner check is skipped; every charge, admission check and
  receipt is identical to `execute`. Descriptors are never host-writable.
- Expiry: the engine has no clock. `host::expire_records(branch, namespace,
  name, BlockNumber(b), query_budget, delete_budget)` sweeps height by
  height: `terms(namespace, name, U64, Unbounded, 1, query_budget)` reads
  the smallest live height; the run stops when there is none or it exceeds
  `b`; otherwise the rows of that height are read as `Eq(name, U64(h))`
  pages (`Projection::KEY_ONLY`, limit `max_page`, no cursor,
  `query_budget`) and every row of each page is deleted through
  `execute_host(DeleteRecord)` with `delete_budget`, until a page is empty,
  and the next smallest height is read. Read receipts are discarded.
  Deletes are reported in (height, id) order, and the work is linear in the
  distinct expired heights plus the deleted rows. Any rejection (read or
  delete) returns `ExpiryHalt { applied, rejected }` with the deletes
  applied so far, which stay applied in the branch and keep their receipts
  there; a halted run resumes at the smallest live height in a later block,
  so the host reserves block budget for expiry (runs it first, or sizes the
  block budget for it). Run in the block where records expire, snapshot B
  lacks them while snapshot B−1 keeps them, and no read consults the head.
  The host stores the expiry height as an ordinary `U64` attribute and must
  itself reject user writes of the `$`-cells it relies on; the engine
  assigns `$`-names no meaning.

## Admission validation

Admission validation, before any charge (spent 0, receipt issued, not
recorded):

- Writes, in order: the host-path restriction, then the arguments.
  `CreateRecord`: more than `max_cells` cells → `TooManyCells`; then per cell
  in submitted order `DuplicateCell`, a name longer than
  `max_cell_name_len` → `NameTooLong`, a `Str`/`Bytes` value longer than
  `max_str_len`/`max_bytes_len` or a `Str` attribute longer than the
  indexed-string bound → `ValueTooLarge`,
  an attribute holding `Bytes` →
  `BytesNotIndexable`. `PatchRecord`: both lists empty is valid (golem-db's
  no-op: `NotFound` for a missing key, otherwise applied with DirectoryLookup,
  PointRead and RecordPatch 1 charged and nothing written); `set.len() + remove.len() > max_cells` → `TooManyCells`; `set` validated
  like create cells; a duplicate in `remove` → `DuplicateCell`, a name in
  `remove` longer than `max_cell_name_len` → `NameTooLong`; a name in
  both lists → `SetAndRemoveOverlap`. Then the block admission
  (`ExceedsBlockBudget`).
- Reads: `get_record` checks a `Named` projection's size
  (`ProjectionTooLarge`). `query` checks, in order: `NoGroups`,
  `TooManyGroups`, `TooManyLiterals`, `ZeroLimit`, `LimitTooLarge`; per
  literal in order: `Eq`/`NotEq` with a `Bytes` value → `NotIndexable`, with
  a `Str` value longer than the indexed-string bound → `ValueTooLarge`; `Range` with
  both bounds `Unbounded` → `UnboundedRange`, bounds of different types →
  `MixedBounds`, a `Bytes` bound → `NotOrdered`, a `Str` bound longer than the
  indexed-string bound → `ValueTooLarge`; `StartsWith` with a prefix longer than the indexed-string bound →
  `ValueTooLarge`; `Exists` with `Bytes` → `NotIndexable`; then
  `ProjectionTooLarge`; then the cursor check. `count` validates the query
  the same way with an implicit limit of 1. `terms` checks, in order: a
  type that is not indexable → `NotIndexable`; a bound value of another
  type → `MixedBounds`; a `Str` bound longer than the indexed-string bound →
  `ValueTooLarge`; `ZeroLimit`; `LimitTooLarge`.
