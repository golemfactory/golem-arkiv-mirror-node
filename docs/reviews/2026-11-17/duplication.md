# Code-quality review, 2026-11-17: Part II.1: duplication

Reviewed at `e6c49e1`; line numbers refer to that commit. Parts: [summary](README.md) · [duplication](duplication.md) · [tests](tests.md) · [comments](comments.md) · [sloppiness](sloppiness.md) · [other](other.md) · [appendices](appendices.md)

# Part II: complete findings by category

Every finding of every area, grouped by category; inside a category the areas are ordered by payoff (lines removable or clarity gained), and inside an area the findings keep the reviewer's own payoff order and numbering. Path conventions are stated in each area heading. Cross-references such as "see 1.7" refer to the same area's numbering.

## II.1 Duplication


### II.1 [T2] arkiv-db tests: history / isolation

*(paths relative to `crates/arkiv-db/tests/`; `h` = history.rs, `qh` = query_history.rs, `i` = isolation.rs, `qi` = query_isolation.rs)*

#### 1.1 history.rs <-> query_history.rs (same oracle harness twice) - biggest payoff

Measured with `diff` on the paired line ranges ("common" = lines identical in both):

| item | history.rs | query_history.rs | common | note |
|---|---|---|---|---|
| `run_seed` | 1511-1856 (346) | 3340-3742 (403) | 173 | same phase skeleton: genesis, admin, begin, op loop with checkpoints/doomed, unwind, seal, staged, loser, publish, applied, retain |
| `publish_admin` | 1332-1398 (67) | 3045-3104 (60) | 55 | identical but for 2 extra asserts in h (1370-1374, 1377) and the `check_reads` arity |
| `verify_history` | 1858-1945 (88) | 3744-3812 (69) | 49 | same 3-handle loop (`blocks.at`, `engine.snapshot`, retained) |
| `build_loser` | 1400-1504 (105) | 3106-3279 (174) | 44 | same frame, different steps |
| `execute_checked` | 1219-1283 (65) | 2648-2800 (153) | 33 | same frame |
| `Flat`, `flat_write`, `flat_read` | 691-717 | 1331-1357 | 27/27 | verbatim |
| `expected_flat` | 719-758 | 1365-1399 | 24 | differs only in `Expected` field names |
| `rollback_checked` | 1285-1326 | 2802-2846 | 24 | |
| `assert_replay` | 1974-2006 | 3859-3890 | 24 | |
| `Ctx` + `Display` | 821-835 | 1474-1488 | 15/15 | verbatim |
| `seed_tests!` | 2059-2073 | 4263-4277 | 15/15 | verbatim |
| `Oracle` + `new` | 424-438 | 1079-1093 | 14/15 | `View::default()` vs `View::new()` |
| `verdict` | 253-271 | 438-455 | 13 | h: `semantic: Option<Rejection>` fixed at charge 0; qh: `Option<(usize, Rejection)>`. h's is the special case `Some((0, r))` |
| `expected_read` | 760-775 | 1401-1416 | 13 | same difference as `verdict` |
| `Generator::{next,index}` | 1054-1066 | 2214-2226 | 13/13 | verbatim xorshift32 (also verbatim in metering.rs:1668-1676 and model.rs:71-83, which additionally has h's `value()` 1068-1072 verbatim) |
| `receipt` | 273-282 | 457-466 | 10/10 | verbatim |
| `Log::schedule_at` | 414-421 | 1069-1076 | 8/8 | verbatim |
| `block_size` | 1079-1085 | 2488-2494 | 7/7 | verbatim (SIZES differ) |
| `count`, `total` | 72-78 | 165-171 | 7/7 | verbatim |
| `Transcript` | 806-811 | 1444-1449 | 6/6 | verbatim |
| `receipt_of` | 1014-1018 | 1359-1363 | 5/5 | verbatim |
| ALICE/BOB/CAROL/OWNERS | 49-52 | 97-100 | 4/4 | verbatim |
| `Run` | 1506-1509 | 3293-3296 | 4/4 | verbatim |
| `full_run` verdict loop | 1947-1972 | 3814-3857 | 12 | |

Fix, in two steps:

- **Step A (safe, ~150 lines removed):** move the verbatim items into one module (see 1.7 `support/oracle.rs`):
  `count`, `total`, `receipt`, `Flat`/`flat_write`/`flat_read`/`receipt_of`, `verdict` (keep qh's signature; h passes
  `semantic.map(|r| (0, r))`), `expected_read`, `Ctx`+`Display`, `Transcript<E>` (generic over the event enum),
  `Run`, `Generator::{next,index}` plus qh's `pick`/`chance` (h's `owner()` 1074-1076 is `pick(&OWNERS)`),
  `block_size(block, &SIZES)`, `seed_tests!`, the owner constants. **safe**
- **Step B (needs-care, roughly another 250 lines):** extract the engine-facing phases that are literally the
  same as plain functions, no trait needed:
  `publish_pricing_candidate(engine, current_id, schedule, id, next_snapshot, &ctx) -> SnapshotId` (h 1353-1392 = qh 3065-3096),
  `assert_sealed(&branch -> sealed, base, &receipts, spent, &ctx)` (h 1696-1702 = qh 3550-3555),
  `publish_block(engine, blocks, &sealed, loser, base, block, next_snapshot, &ctx) -> (SnapshotId, Option<EngineError>)` (h 1735-1789 / qh 3588-3610),
  `assert_replay<E: PartialEq + Debug>(a, b, seed)`, `assert_verdicts_seen(&map, &[..], seed)` (h 1950-1962 = qh 3817-3837).
  A full generic driver (`trait Workload` with two impls) would remove more but is a larger rewrite; not required.

#### 1.2 isolation.rs <-> query_isolation.rs (same phase harness twice)

| item | isolation.rs | query_isolation.rs | common | note |
|---|---|---|---|---|
| `trait Reader` + 2 impls | 176-251 (76) | 851-926 (76) | 36 | see 1.3 |
| `publish_phase0` | 940-982 | 2735-2805 | 28 | |
| test 4 (pinned snapshot) | 1150-1193 | 3027-3106 | 28 | |
| test 3 (directory growth) | 1117-1148 (32) | 2993-3025 (33) | 25 | bodies differ in 8 lines |
| test 2 (growth of B) | 1086-1115 (30) | 2957-2991 (35) | 23 | |
| `grow_b` | 984-1013 | 2807-2867 | 21 | |
| `create_namespaces` | 1015-1034 | 2869-2888 | 19/20 | differs only in `create` vs `create_namespace` |
| `Observed`/`Step`/`Expected` | 150-174 | 815-849 | 17 | i: `receipt: Receipt, physical: PhysicalTrace`; qi: `receipts: Vec<Receipt>, physical: Option<PhysicalTrace>` |
| `create`/`create_namespace` | 906-920 | 2700-2714 | 14/15 | identical body, different name |
| `assert_matches_expected` | 623-638 | 2506-2521 | 14/16 | `receipt` vs `receipts` |
| `assert_receipts_and_results_equal` | 640-655 | 2523-2538 | 14/16 | same |
| `run_workload` | 362-376 | 1373-1414 | 13 | |
| traced `receipt` | 584-610 | 1420-1451 | 13 | i takes `(class, count, unit, subtotal)`; qi takes `(class, count)` and derives the rest from `fixture_price`, asserting the sum |
| `read_step`, `write_step` | 253-285 | 928-943, 973-988 | 12+12 | |
| `assert_directory_nodes_follow_the_trie` | 721-737 | 2590-2615 | 12 | qi adds the `spent == 0 -> 0` arm; a superset of i's cases |
| `assert_a_untouched_since` | 1047-1065 | 2894-2935 | 11 | |
| `descriptor` | 845-852 | 2691-2698 | 8/8 | verbatim |
| `assert_directory_nodes_equal` | 667-675 | 2561-2572 | 8 | |
| `put` | 854-860 | 383-389 | 7/7 | verbatim |
| `find` | 616-621 | 2499-2504 | 6/6 | verbatim |
| `apply` | 922-927 | 2716-2721 | 6/6 | verbatim |
| `DIRECTORY_PATH_BOUND = 17` | 717 | 2588 | | also `DIRECTORY_READ_BOUND = 2 * 8 + 1` at history.rs:289: one constant, three names |

Fix: `support/isolation.rs` (see 1.7) with qi's shape of `Step`/`Expected` (i wraps its single receipt in
`vec![..]` and its trace in `Some(..)`), made generic over the `Observed` enum: `Step<O>`, `Expected<O>`,
`read_step`, `write_step`, `find`, `assert_matches_expected`, `assert_receipts_and_results_equal`,
`assert_directory_nodes_equal`, `assert_directory_nodes_follow_the_trie` (qi's version), `DIRECTORY_PATH_BOUND`,
`descriptor`, `create_namespace`, `apply`, `put`, and qi's self-checking `(class, count)` ledger `receipt`
(then i's 14 hand-typed 4-tuples at 387-565 lose their two derivable columns). ~140 lines removed. **safe**
(the `Step` shape change is **needs-care**: touches every `step.receipt` in i).

#### 1.3 `trait Reader` written four times in this chunk (354 lines of pure delegation)

- raw trait, 3 methods, same signatures: history.rs:849-923 (`get_metered`/`scan_metered`/`descriptor_metered`)
  and isolation.rs:176-251 (`metered_get`/`metered_scan`/`metered_descriptor`); a third copy is blocks.rs:192
  (`read_get`/`read_scan`/`read_descriptor`, there already via an `impl_reader!` macro).
- typed trait, same 3 signatures: query_history.rs:1494-1620 (`get_record_metered`/`query_metered`/`count_metered`
  + `pinned`/`typed_records`/`terms`/`consistent`) and query_isolation.rs:851-926 (`record`/`page`/`total`);
  a third copy is queries.rs:901 (`page_with`/`total_with`/`pinned`).

Fix: one `support/reader.rs` with `RawReader` and `TypedReader`, each implemented for `Snapshot` and `Branch`
by one macro (as blocks.rs:213-249 does). ~90 lines once instead of 354 here. **safe**, call sites renamed.
Root cause is in category 5 (no shared read trait in the library).

#### 1.4 The FIXTURE price table is transcribed three times as code (plus once as prose)

history.rs:103-117 (`FIXTURE: Weights`, 12 classes), query_history.rs:226-250 (22 classes),
query_isolation.rs:108-142 (`fixture_price`, all 31 classes), and prose at isolation.rs:78-82.
`Weights` + `uniform` is defined twice (history.rs:85-136, query_history.rs:196-279) with five fields in
common; rg shows a third `struct Weights` in arkiv-node/tests/block_model.rs:117 (not read).
Fix: one `Weights` with a field per `OperationClass` in `support/oracle.rs`; `fixture_price(class)` becomes
`FIXTURE.price(class)`. ~75 lines. **safe** (the oracle stays independent of the engine's pricing API).

#### 1.5 Reference evaluator and `project` written twice here (and again elsewhere)

query_history.rs:703-809 (`attribute`, `ordered`, `bound_tag`, `within`, `term_matches`, `literal_matches`,
`query_matches`, `project`) and query_isolation.rs:545-636 (`ordered`, `bound_tag`, `above`, `below`,
`term_holds`, `query_matches`, `project`). pagination.rs:526-606 holds a third copy (read: `project` is
line-for-line qi's 616-636; `term_holds`/`ordered`/`within` same logic); rg also finds `fn term_holds` in
queries.rs:285 and query_metering.rs:556 (not read). `encoded_len` is identical in query_history.rs:474-487
and query_isolation.rs:146-159 (`count(x)` vs `x as u64`).
Only qh self-tests its copy (`reference_evaluator_follows_the_brief_semantics`, 4015).
Fix: `support/typed_model.rs` with one evaluator, one `project` (qh's, returning `(Row, u64)`), `encoded_len`,
`cell_bytes`; keep the self-test once. ~105 lines in this chunk. **needs-care** (qi's `Eq` arm at 587 omits the
tag comparison qh has at 756; equivalent only because `Value` equality already distinguishes variants).

#### 1.6 Repetition inside one file

query_history.rs
- `Ctx { seed, block, phase: ".." }` 5-line literal built 13 times (`rg -n 'let ctx = Ctx \{' query_history.rs`),
  12 times in history.rs. Add `Ctx::new(seed, block, phase)` or `ctx.phase("seal")`; ~50 lines per file. **safe**
- `check_page(..)` is called with 13 arguments at 2155-2169, 2176-2190, 3177-3191, 3210-3224; only `limit`
  and `cursor` vary inside a walk. Bundle `{reader, model, weights, namespace, label, query, projection, all, ctx}`
  into a `Walk<'a>` with `fn page(&self, limit, cursor, probe_budget, out)`; ~35 lines and one `#[allow]`. **safe**
- `transcript.events.extend(checked.receipts...map(Event::Read))` at 3101-3103, 3268-3270, 3483-3485,
  3566-3568, 3623-3625 and the `Event::Page` twin at 3271-3276, 3626-3632. One `Transcript::record(&Checked)`. **safe**
- The expected `get_record` result is computed twice with the same 18 lines: 2067-2086 and 2772-2790.
  Extract `expected_get_record(model: Option<&NsModel>, key, projection, weights) -> Flat<Option<Row>>`. **safe**
- The `CursorMismatch` expectation (`Err((Rejection::CursorMismatch, receipt(Read, id, 0, Rejected(..))))`) is
  spelled out at 1970-1982, 3241-3253, 3709-3721 (and in qi at 2437-2442, 3085-3093, 3281-3286). One helper. **safe**
- `open_group`: the `visited` set is built identically in the `Range` and `StartsWith` arms (885-892, 901-908). **safe**
- `expire_checked` builds `receipt(ReceiptKind::Write, pending.weights.id, spent, outcome)` three times
  (2945-2953, 2961-2969, 2973-2981) right after `pending.record(..)` pushed the same receipt; let
  `Pending::record` return it. **safe**

query_isolation.rs
- `steps.push(page_step(NAME, reader.page(a, &q, proj, MAX_PAGE, None, budget)).0)` is written out 9-13 lines
  at a time at 1142-1162 and 1316-1369 (7 blocks, 68 lines); a `full_page(reader, a, name, &query, projection, budget) -> Step`
  helper makes each one line. ~55 lines. **safe**
- test at 3109: the 10-line `head.query(ns, &q, &Projection::ID_ONLY, MAX_PAGE, None, traced).expect(..)` block
  appears at 3132-3141, 3170-3179, 3202-3211, 3234-3243; a local closure removes ~28 lines. **safe**

isolation.rs
- `assert_matches_expected` (623-638) and `assert_receipts_and_results_equal` (640-655) have the same body over
  `Expected` vs `Step`; `assert_record_nodes_equal` (657-665) and `assert_directory_nodes_equal` (667-675) differ
  in one field name. **safe**, ~20 lines.
- `transfer_work_is_independent_of_namespace_size`: 1203-1212 is repeated at 1249-1258 (see category 2).

#### 1.7 What `tests/support` should hold

The crate already shares test code with `#[path = "support/durable_history.rs"] mod ..;` (durable.rs:123), so
the mechanism exists. Proposed files (each test includes only what it needs, which avoids a blanket
`#![allow(dead_code)]`):

- `support/vocab.rs`: owners, `ns`, `rid`, `name`, `key`, `attr`, `field`, raw `put`/`delete`/`transfer`, typed
  `create_record`/`patch_record`/`delete_record`, `descriptor`, `create_namespace`, `apply`, untraced `receipt`.
- `support/reader.rs`: `RawReader`, `TypedReader` (1.3).
- `support/oracle.rs`: everything in Step A of 1.1, the unified `Weights`/`FIXTURE` (1.4), then Step B.
- `support/typed_model.rs`: 1.5.
- `support/isolation.rs`: 1.2.

Locally defined generic helpers, exact signatures (for the workspace-level helper inventory):

| helper | file:lines | signature | differs from siblings |
|---|---|---|---|
| `ns` | h:64-66 | `fn ns(ordinal: u64) -> NamespaceId` | body `NamespaceId::from_bytes(ordinal.to_be_bytes())` |
| `ns` | qh:102-104 | same | body `NamespaceId::from_ordinal(ordinal)` |
| `CAROL` | h:51, qh:99 | `OwnerId::new([0xca; 20])` | i:49 is `[0xc0; 20]` |
| `DAVE` | i:50, qi:67 | `OwnerId::new([0xd0; 20])` | |
| `owner_name` | h:54-62 | `fn owner_name(owner: OwnerId) -> &'static str` | one use (1239); `OwnerId` implements `Debug` (src/namespace.rs:141), so `{actor:?}` would do if a raw id in the message is acceptable |
| `len`, `count`, `total` | h:68-78; qh:165-171 | `fn len(&[u8]) -> u64`, `fn count(usize) -> u64`, `fn total(&[u64]) -> u64` | qi uses bare `as u64` instead of `count` |
| `receipt` (untraced) | h:273-282; qh:457-466 | `fn receipt(kind: ReceiptKind, schedule_id: u32, spent: u64, outcome: Outcome) -> Receipt` | |
| `receipt` (traced) | i:586-610 | `fn receipt(kind, spent: u64, outcome, lines: &[(OperationClass, u64, u64, u64)]) -> Receipt` | 4-tuple lines |
| `receipt` (traced) | qi:1423-1451 | `fn receipt(kind, spent: u64, outcome, lines: &[(OperationClass, u64)]) -> Receipt` | 2-tuple, self-checking |
| `read_receipt` | qh:1869-1871 | `fn read_receipt(weights: &Weights, spent: u64) -> Receipt` | qi:1453 is `fn read_receipt(spent: u64, lines: &[(OperationClass, u64)])` |
| `write_receipt`, `aborted_read`, `rejected_write`, `plain_write_receipt` | qi:1457-1484 | `(spent, lines)`, `(spent, lines)`, `(spent, reason, lines)`, `(spent)` | |
| `put` | h:1105-1111 | `fn put(namespace: NamespaceId, key: &[u8], value: Vec<u8>) -> WriteOp` | i:854 and qi:383 take `value: &[u8]` |
| `delete` | h:1113-1118; i:862-867 | `fn delete(namespace: NamespaceId, key: &[u8]) -> WriteOp` (raw `Delete`) | qh:2383 `delete` builds `DeleteRecord` |
| `transfer` | i:869-875 | `fn transfer(namespace: NamespaceId, expected_revision: u64, new_owner: OwnerId) -> WriteOp` | |
| `entry` | i:838-843 | `fn entry(key: &[u8], value: &[u8]) -> Entry` | |
| `descriptor` | i:845-852; qi:2691-2698 | `fn descriptor(id: NamespaceId, owner: OwnerId, revision: u64) -> NamespaceDescriptor` | |
| `create` / `create_namespace` | i:906-920 / qi:2700-2714 | `fn (branch: &mut Branch, owner: OwnerId) -> NamespaceId` | qh:2361 `create` builds `CreateRecord` |
| `apply` | i:922-927; qi:2716-2721 | `fn apply(branch: &mut Branch, actor: OwnerId, op: WriteOp) -> Effect` | pagination.rs:608 returns `Executed` |
| `name` | qh:153-155; qi:252-254 | `fn name(text: &str) -> CellName` | |
| `key` | qh:157-159 | `fn key(bytes: &[u8]) -> RecordKey` | qi:256 takes `text: &str` |
| `rid` | qh:161-163; qi:260-262 | `fn rid(ordinal: u64) -> RecordId` | |
| `attr`, `field` | qh:175-181; qi:264-270 | `fn attr(value: Value) -> Cell`, `fn field(value: Value) -> Cell` | |
| `cells_of` | qh:183-185 | `fn cells_of(list: &[(CellName, Cell)]) -> Cells` | |
| `create`/`patch`/`delete` (typed) | qh:2361-2388 | `(namespace, bytes: &[u8], ..) -> WriteOp` | qi:354-381 `create_record`/`patch_record`/`delete_record` take `key: &RecordKey` |
| `NamespaceName::new("test").unwrap()` | h x4, qh x4, i x2, qi x2 | inline literal | worth a `test_name()` |

No `impl VersionedStore for <Wrapper>` doubles and no `ledger`/`traced`/`fixture`/`genesis`/`signer`/`hex`/`head`
helpers in these four files (`World::head` in i:1036 / qi:2890 is a method).

### II.1 [T4] arkiv-db tests: codec / namespaces / blocks / rollback / roots / proofs

*(paths relative to `crates/arkiv-db/tests/`)*

#### 1.1 The engine-test helper set, typed out three times (namespaces.rs, blocks.rs, rollback.rs) and partially eight more - ~290 lines here - needs-care
Single home: `crates/arkiv-db/tests/support/engine.rs`. A `support/` dir already exists but holds only `durable_history.rs`, pulled in by `#[path = "support/durable_history.rs"]` (durable.rs:123); there is no `support/mod.rs` yet. Each test file is its own crate, so helpers a file does not use will warn as dead code: either keep the module split small (`engine.rs`, `proofs.rs`, `object_map.rs`) and include per file with `#[path]`, or put one `#![allow(dead_code)]` at the top of `support/mod.rs` and nowhere else.

Signatures as they exist today, with the differences a shared module must settle:

| helper | where (lines) | exact signature / difference |
|---|---|---|
| `ns` | codec.rs:80-82, namespaces.rs:44-46, blocks.rs:44-46, rollback.rs:32-34, roots.rs:411-413, eq_proofs.rs:19-21, range_proofs.rs:14-16, prefix_free_terms.rs:27-29, host_state.rs:47-49 | `fn ns(ordinal: u64) -> NamespaceId`. Body is `NamespaceId::from_bytes(ordinal.to_be_bytes())` in namespaces/blocks/rollback and `NamespaceId::from_ordinal(ordinal)` in the other six. ordered_ranges.rs:26-28 is `fn ns() -> NamespaceId` (always 1). |
| cell name | codec.rs:88-90, eq_proofs.rs:25-27, range_proofs.rs:17-19, prefix_free_terms.rs:30-32, ordered_ranges.rs:30-32, golem_types.rs:18-20, host_state.rs:51-53 | `fn name(text: &str) -> CellName`; roots.rs:415-417 calls the same thing `cell_name`. |
| record key | codec.rs:92-94, host_state.rs:55-57 | `fn key(bytes: &[u8]) -> RecordKey`; roots.rs:419-421 `record_key`; eq_proofs.rs:28-30 is `fn key(n: u64) -> RecordKey` (same name, different parameter). |
| record id | codec.rs:84-86 `rid(ordinal: u64) -> RecordId`; eq_proofs.rs:22-24 `id(n: u64) -> RecordId` | two names. |
| `entry` | namespaces.rs:57-62, blocks.rs:145-150, rollback.rs:59-64 | `fn entry(key: &[u8], value: &[u8]) -> Entry`, identical. |
| `put` / `delete` / `transfer` | namespaces.rs:124-145, blocks.rs:59-80, rollback.rs:36-57 | `fn put(namespace: NamespaceId, key: &[u8], value: &[u8]) -> WriteOp`, `fn delete(namespace: NamespaceId, key: &[u8]) -> WriteOp`, `fn transfer(namespace: NamespaceId, expected_revision: u64, new_owner: OwnerId) -> WriteOp`, identical. host_state.rs:431-437 has `raw_put(namespace: u64, ..)`, and host_state.rs:73-78 has a **different** `put(branch: &mut Branch, key: HostKey, value: &[u8]) -> Option<Vec<u8>>`. |
| `descriptor` | namespaces.rs:48-55, blocks.rs:48-55 | `fn descriptor(id: NamespaceId, owner: OwnerId, revision: u64) -> NamespaceDescriptor`; rollback.rs:66-73 takes `ordinal: u64` instead of `id`. |
| `receipt` | codec.rs:932-941, blocks.rs:84-93 | `fn receipt(kind: ReceiptKind, schedule_id: u32, spent: u64, outcome: Outcome) -> Receipt`; namespaces.rs:64-73 drops `schedule_id` (fixed 1). |
| `applied` / `rejected` | namespaces.rs:75-81, blocks.rs:96-103, rollback.rs:76-93 | `fn applied(spent: u64) -> Receipt`, `fn rejected(spent: u64, reason: Rejection) -> Receipt`. |
| `read` / `read_rejected` | namespaces.rs:83-89, blocks.rs:106-113 | `fn read(spent: u64) -> Receipt`, `fn read_rejected(spent: u64, reason: Rejection) -> Receipt`. |
| `traced` | namespaces.rs:109-122 | `fn traced(receipt: Receipt, lines: &[(OperationClass, u64, u64, u64)]) -> Receipt`; rollback.rs:900-905 builds the same with a local `ledger_entry` closure. |
| result projections | namespaces.rs:149-160, blocks.rs:119-139, rollback.rs:189-192 | **same names, three meanings**: `logical` takes a `WriteResult` in namespaces.rs:149 but a `ReadResult<T>` in blocks.rs:121; `value` returns `Result<(T, Receipt), (Rejection, Receipt)>` in namespaces.rs:156, plain `T` in blocks.rs:128, and is `value(view, namespace, key) -> Option<Vec<u8>>` in rollback.rs:189. |
| begin / publish | namespaces.rs:162-174, blocks.rs:154-165 | `begin(engine: &Engine) -> Branch` vs `branch(engine: &Engine, block_budget: Option<u64>) -> Branch`; `publish(&mut Engine, Branch)` returns `Snapshot` in namespaces.rs:168 but `SnapshotId` in blocks.rs:160. |
| execute-and-require-applied | namespaces.rs:201-205 `apply`, rollback.rs:118-122 `apply`, blocks.rs:168-173 `execute` | `fn (branch: &mut Branch, actor: OwnerId, op: WriteOp) -> Executed`; rollback.rs:125-130 adds `reject(branch, actor, op, budget: Budget) -> Rejected`; blocks.rs:134 `rejection(WriteResult) -> (Rejection, Receipt)`. |
| `create` | namespaces.rs:177-199, blocks.rs:175-188 | `fn create(branch: &mut Branch, actor: OwnerId) -> NamespaceId`; the namespaces.rs one also asserts the receipt is `applied(53)`. rollback.rs:97-115 `engine_with_namespaces(owners: &[OwnerId]) -> Engine`. |
| owners | all | `ALICE = [0xa1; 20]`, `BOB = [0xb0; 20]` everywhere; `CAROL` is `[0xca; 20]` in namespaces.rs:31 but `[0xc0; 20]` in blocks.rs:33, rollback.rs:25, roots.rs:53; `DAVE` is `[0xda; 20]` in namespaces.rs:32 but `[0xd0; 20]` in blocks.rs:34, rollback.rs:26. `OWNER` is `[7; 20]` in eq_proofs.rs:18, prefix_free_terms.rs:25, ordered_ranges.rs:24 (inline in range_proofs.rs:24) but `[1; 20]` in golem_types.rs:16. |

Risk: mechanical, but the name clashes (`logical`, `value`, `publish`, `put`, `key`) mean call sites must be touched one file at a time. No golden value depends on any of these.

#### 1.2 A test-only "one read interface over `Branch` and `Snapshot`" trait, written three times here - 145 lines - needs-care
- blocks.rs:192-249 `trait Reader { read_get, read_scan, read_descriptor, namespaces }` + `impl_reader!` macro (58 lines)
- namespaces.rs:209-241 `trait Reads { lookup, scan, describe }` + two impls (33 lines)
- rollback.rs:133-186 `trait View { value, entries, desc, count }` + two impls whose bodies are identical line for line (54 lines)
- same root cause in host_state.rs: `all` 86-90 vs `all_at` 92-96 (same body, `&Branch` vs `&Snapshot`) and `observe_branch` 544-588 vs `observe_snapshot` 591-645 (100 lines; a diff with the receiver renamed differs in only four places: the extra `k2` get, one page vs a keyset walk, and the `terms` attribute/type).
- `rg -n "^trait [A-Za-z]+ \{" crates/arkiv-db/tests` shows seven more (history.rs:849, isolation.rs:177, queries.rs:901, query_history.rs:1494, query_isolation.rs:852, records.rs:433, terms.rs:428).
Home: one trait in `tests/support` at minimum; better, see 5.1 (the library has no such trait).

#### 1.3 Proof-test doubles and negative-test scaffolding shared by eq_proofs.rs, range_proofs.rs, prefix_free_terms.rs - ~165 lines - needs-care
a) **The store double** (the "~820 / ~607" pair). It is `impl ObjectReader for Store`, not `VersionedStore`. eq_proofs.rs:818-836 and range_proofs.rs:605-623 are byte-identical: fn-local `struct Store(Mutex<BTreeMap<(SpaceId, ObjectId), Vec<u8>>>)` with `read_object` and a `read_object_bounded` that delegates. The same double exists a third time in rehash.rs:82-101 (`HashMap` instead of `BTreeMap`), a fourth in storage_faults.rs:72-149 (`TestStore`: same map + read counter + fail-after-N), a fifth in `crates/arkiv-db/src/engine.rs:3005-3035` (unit-test `Reader`).
   - Is there an equivalent in `crates/arkiv-db-storage/src/testing/`? **No.** `faulty.rs` `FaultyStore<S: VersionedStore>` (195-334) wraps a complete `VersionedStore` and injects *errors* by `ReadScope::{AllObjects, Space, Records, Head, Everything}` (48-58, `fail_reads` at 245); it cannot be built from a bag of staged objects and cannot make one object absent (`Ok(None)`), which is what eq_proofs.rs:850 (`clear()`), range_proofs.rs:687-691 (`remove(&(SpaceId::Namespace(1), id))`) and the selective load at range_proofs.rs:663-676 rely on. `contract.rs` `TestStore` (26-34) is a `Box<dyn VersionedStore>` + reopen closure for the contract suite; `contract::objects_of` (1203-1209) returns the same map type but from `CommitBatch`es.
   - Home: one map-backed reader. Either `tests/support/object_map.rs` in arkiv-db (`ObjectMap::from_staged(Vec<StagedObject>) -> Arc<Self>`, `remove`, `clear`, `reads()`, `fail_after(n, kind)`), or `arkiv_db_storage::testing::ObjectMap` built from `impl IntoIterator<Item = ((SpaceId, ObjectId), Vec<u8>)>` (`StagedObject` is an arkiv-db type). Removes 38 lines here, ~20 in rehash.rs, ~75 in storage_faults.rs.
b) **The reopen recipe** around it is also written twice: eq_proofs.rs:837-847 and range_proofs.rs:663-684 (`staged_objects()` -> map -> `Arc<Store>` -> `Engine::from_state(&original.state_header(), Limits::DEFAULT, store.clone())` -> `head_snapshot()`). `fn reopen(original: &Snapshot, keep: impl FnMut(&StagedObject) -> bool) -> (Arc<ObjectMap>, Snapshot)`.
c) **Parallel tamper tests -> one harness.** `fixed_root_rejects_omissions_membership_only_pages_and_all_witness_mutations` (eq_proofs.rs:340-399), `omitted_terms_postings_rows_and_tampered_bindings_cannot_verify` (range_proofs.rs:442-595) and `tampered_or_misdirected_proofs_never_verify_to_another_result` (prefix_free_terms.rs:510-642) all run the same stages:
   - structural mutations of a parsed `Wire` must not verify (eq 345-380; range 478-525)
   - every strict prefix must not verify (eq 382-387; range 526-536; prefix_free 611-621)
   - a trailing byte must not verify (eq 388-390; range 537-539)
   - flipped bytes must not verify, or must verify to the honest answer (eq 394-398 at `[0, 3, 7, 50, len-1]` with `^0xff`; range 540-544 every 31st byte with `^128`; prefix_free 561-573 and 600-610 every byte)
   - a changed request must not verify (eq 317-331; range 545-577; prefix_free 536-559 and 624-641)
   Home `tests/support/proofs.rs`:
   - `fn assert_bytes_are_tamper_evident(bytes: &[u8], flips: impl Iterator<Item = usize>, verify: impl Fn(&[u8]) -> Option<bool>)` covering prefixes, trailing byte, flips (`Some(true)` = "verified to the honest answer").
   - `fn assert_every_mutation_is_rejected<W: Clone>(original: &W, cases: &[(&str, fn(&mut W))], verifies: impl Fn(&W) -> bool)`. In range_proofs.rs the nine hand-unrolled `let mut w = wire.clone(); <one change>; reject(w);` triplets (493-525) and three index loops (478-492) become a table; in eq_proofs.rs the `for mutation in 0..20 { match mutation { .. } }` (345-380) becomes the same labelled table, so a failure reads "accepted: duplicated row witness" instead of "accepted mutation 6".
   Keep every case when converting (eq: 20; range: 3 loops + 9 singles). Tests stay three; ~60 lines go.
d) range_proofs.rs has no `verify` wrapper like eq_proofs.rs:54-77 (default limits + cross-check of the `_inspected` entry point), so `verify_range(.., RangeProofLimits::default(), Limits::DEFAULT)` is spelled out at ten call sites outside the `verified` helper (243-249, 277, 469-475, 528-534, 539, 543, 548-554, 559-565, 570-576, 587-593; six of them over 7-8 lines) plus the local `reject` closure 458-477. Add `fn verify(r: &RangeRequest, bytes: &[u8]) -> Result<VerifiedRange, ProofError>` mirroring eq's. ~45 lines. safe.
e) Identical closure `let count = |class| ledger.entries.iter().find(|e| e.class == class).map_or(0, |e| e.count);` at eq_proofs.rs:653-659 and range_proofs.rs:639-645 (rehash.rs:~218 has it as a fn). One `fn charged(ledger: &Ledger, class: OperationClass) -> u64`.
f) Identical `type RowWitness = (RecordId, Vec<Vec<u8>>, Vec<Vec<u8>>);` at eq_proofs.rs:163 and range_proofs.rs:381, and identical row-witness decode/encode loops (eq 189-192 + 221-226; range 411-415 + 431-436). The two `Wire` structs are different formats: keep both, share the row-witness part.

#### 1.4 "One namespace + N records + model rows" fixture, six shapes - ~180 lines - safe
- eq_proofs.rs:87-131 `fn fixture(count: u64) -> (Engine, Vec<Row>)`
- range_proofs.rs:21-65 `fn fixture(values: Vec<(Value, bool)>) -> (Engine, Vec<Row>)`
- prefix_free_terms.rs:72-107 `fn fixture(limits: Limits, values: &[Value]) -> (Engine, Vec<Row>)`
- ordered_ranges.rs:73-122 `fn fixture(values: &[Value]) -> Fixture`
- golem_types.rs:244-279 `engine_with_namespace() -> (Engine, Branch, NamespaceId)` + `create(branch, ns, key: &[u8], cells)`
- host_state.rs:105-119 `create_namespace(branch, text)` + `create_record(branch, namespace, record: &[u8], height: u64)`
All are built from the same two calls. With `fn create_namespace(branch: &mut Branch, owner: OwnerId, name: &str) -> NamespaceId` and `fn create_record(branch: &mut Branch, owner: OwnerId, namespace: NamespaceId, key: RecordKey, cells: Vec<(CellName, Cell)>) -> RecordId` in support:
- eq_proofs.rs writes the 9-10 line `branch.execute(OWNER, WriteOp::CreateRecord { .. }, Budget::unlimited()).unwrap()` block **8 times** (110-121, 445-455, 462-472, 869-879, 916-926, 986-996, 1008-1018, 1060-1070), the `CreateNamespace` block 4 times (91-99, 900-908, 976-984, 1034-1042) and `DeleteRecord` twice (410-419, 434-443): ~80 lines.
- prefix_free_terms.rs:669-701 is an inline copy of its own `fixture` (only the attribute name differs), 704-722 an inline copy of `eq_rows` (198-208) and 725-742 of `range_rows` (158-183). Give the three helpers an `attribute: &str` parameter: ~70 lines. prefix_free_terms.rs:339-349 builds a second engine + namespace by hand where `fixture(limits, &[])` would do.

#### 1.5 `WriteOp::CreateNamespace { name: arkiv_db::NamespaceName::new("test").unwrap() }` spelled out 20 times in three files - ~40 lines - safe
rollback.rs x11 (104, 376, 389, 402, 476, 507, 724, 773, 877, 1081, 1097), namespaces.rs x5 (180, 1026, 1038, 1062, 1099), blocks.rs x4 (179, 288, 295, 818). `rg -c 'NamespaceName::new\("test"\)' crates/arkiv-db/tests` totals 66 in the directory. One `fn create_namespace_op() -> WriteOp` (and import `NamespaceName`; it is fully qualified in all 23 places in these three files).

#### 1.6 Range-proof helpers repeated across range_proofs.rs, prefix_free_terms.rs, ordered_ranges.rs - ~60 lines - safe
- prove + verify + unwrap `Complete`: range_proofs.rs:82-99 (`proof` + `verified`), prefix_free_terms.rs:158-183 (`range_rows`), ordered_ranges.rs:190-205 (`Fixture::proven`).
- the bound predicate: range_proofs.rs:100-111 `matches(value, lower, upper)` and prefix_free_terms.rs:136-147 `in_bounds(..)` have identical bodies. (ordered_ranges.rs:49-62 `in_range` deliberately orders by raw bytes instead of `cmp_ordered`: keep.) Same signature again in pagination.rs:589, queries.rs:306, query_metering.rs:586, query_history.rs:730 (bodies not compared, other chunks).
- "walk every page, collect ids": prefix_free_terms.rs:109-130 `query_ids` and ordered_ranges.rs:138-162 `Fixture::paged`.
- `SnapshotRef` literal: prefix_free_terms.rs:149-156 `snapshot_ref`, inline at eq_proofs.rs:33-38, range_proofs.rs:68-73, ordered_ranges.rs:174-179 (different constants each: genesis `0x11`/`1`, height `sequence`/`7`/`3`/`1`).

#### 1.7 Four hand-maintained "sample values per scalar type" tables - ~60 lines - needs-care
golem_types.rs:43-102 `ordered_samples()` (16 types, strictly increasing), prefix_free_terms.rs:414-469 (23 groups), range_proofs.rs:166-195 (5 groups), range_proofs.rs:983-999 (11 families). Move `ordered_samples()` to support and drive the range tests from it (see 2.6).

#### 1.8 Cross-crate copies (other reviewers own the second file) - ~90 lines - needs-care
- Byte builders `be32` / `be64` / `lp` / `cat`: codec.rs:53-71 and `crates/arkiv-node/tests/block_format.rs:34-51`, same names, same bodies. roots.rs:164-168 `prefixed` is `lp` again; roots.rs:159 `be32(value: usize) -> [u8; 4]` has a different signature.
- Hand-built state-format-2 leaf construction (the oracle for state roots): roots.rs:159-248 (`prefixed`, `escaped`, `value_bytes`, `term_bytes`, `term_key`, `handle`) vs `crates/arkiv-node/tests/support/block_roots.rs:8-75` (`bytes`, `escaped`, `tag`, `value`, `term_value`, `term`, `handle`), identical logic over a different model type; `escaped` is identical statement for statement (roots.rs:171-182, block_roots.rs:14-25; a third in `crates/arkiv-db-trie/tests/encoding.rs:58`). Both files already `#[path]`-include `arkiv-db-trie/tests/support/mod.rs`; the byte-level fns can live next to it. Keep them independent of the engine (they are the oracle).
- Seeded generators: codec.rs:1141-1155 `Rng` (xorshift64) is the same algorithm as `Generator` in queries.rs:~790 and query_planner.rs:~135; SplitMix64 `Rng` is in rehash.rs:350, `crates/arkiv-db/src/layout.rs:649`, `crates/arkiv-db-trie/src/node.rs:515`, `crates/arkiv-db-trie/tests/support/mod.rs:23`, `crates/arkiv-node/tests/block_model.rs:1396`. roots.rs:49 already reuses the trie-support one via `#[path]`. If unified, note that the algorithm + seed decide the generated corpus; in codec.rs nothing but `accepted_count > 0` depends on it.

#### 1.9 The FIXTURE cost table in three module docs - ~35 lines - safe
namespaces.rs:4-21 (list), blocks.rs:5-22 (table), rollback.rs:3-15 (table, last two rows mis-aligned at 14-15) restate the same weights and formulas. One copy on the shared support module (or `docs/testing.md`), a one-line pointer in each file.

### II.1 [T3] arkiv-db tests: queries / records / pagination / terms

*(paths relative to `crates/arkiv-db/tests/`; `qm` = `query_metering.rs`)*

#### 1.1 The `BTreeMap` reference model exists four times in this chunk (seven times in the crate) - needs-care

All four are "records as sorted cell maps + create/patch/delete + project a row", differing only in container layout and argument types:

| file:lines | size | storage | create / patch / delete signatures | extras |
|---|---|---|---|---|
| `records.rs:336-422` | 87 | `live: BTreeMap<RecordKey,(RecordId, BTreeMap<CellName,Cell>)>`, `next_id` starts 0, pre-increment | `create(&mut self, RecordKey, Cells) -> RecordId`; `patch(&mut self, &RecordKey, Cells, &[CellName])`; `delete(&mut self, &RecordKey) -> RecordId` | `row(&RecordKey, &Projection) -> Option<Row>`, `all_rows`, `record_count`, `term_count` (via `term_bytes()`) |
| `terms.rs:277-425` | 149 | identical struct | **byte-identical** create/patch/delete (`terms.rs:311-338` == `records.rs:343-370`, doc comments included; verified with `diff`) | `id`, `row(&RecordKey) -> Option<Row>` (always full projection), `keys`, `distinct`, `terms`, `terms_cost`, `term_count` (a second algorithm, via `distinct`) |
| `queries.rs:144-340` | 197 | `live: BTreeMap<RecordId,(RecordKey, Cells)>`, `next` starts 1, key lookup by linear `find` | `create(RecordKey, Vec<(CellName,Cell)>) -> RecordId`; `patch(&RecordKey, Vec<..>, &[CellName]) -> RecordId`; `delete(&RecordKey) -> RecordId` | `matches(&Query) -> Vec<RecordId>`, `row(RecordId, &Projection) -> Row`, `term_count` (third algorithm, `HashSet`), free fns `type_of`, `term_holds`, `in_range` |
| `pagination.rs:449-609` | 161 | `live: BTreeMap<RecordId, Record{key,cells}>` + `keys: BTreeMap<RecordKey,RecordId>`, `next` starts 1 | `create(&RecordKey, &[(CellName,Cell)]) -> RecordId`; `patch(&RecordKey, &[..], &[CellName])`; `delete(&RecordKey) -> RecordId` | `is_live`, `key_of`, `len`, `evaluate(&Query,&Projection) -> Vec<Row>`, free fns `project`, `query_holds`, `term_holds`, `ordered`, `within` |

Same thing again outside the chunk (read to confirm): `query_isolation.rs:533-637` (`ModelRecord`, `Model`, `ordered`, `bound_tag`, `above`, `below`, `term_holds`, `query_matches`, `project`), `query_metering.rs:421-610` (`Model`, `term_holds`, `ordered`, `in_range`); rg also shows `query_history.rs:730` (`within`) and `:784` (`project`).

Sub-blocks that are literally the same code:

- **Projection -> Row** (`match &projection.cells { None / All / Named }` + `projection.key.then(..)`): `records.rs:372-388`, `queries.rs:215-234`, `pagination.rs:526-546`, `query_isolation.rs:616-637`.
- **Term semantics** (`term_holds`): `queries.rs:285-302`, `pagination.rs:560-578`, `query_isolation.rs:578-604`, `query_metering.rs:556-573`. Subtle differences: `queries.rs` compares types with its own `type_of` and tests `cell.kind != CellKind::Attribute`; `pagination.rs`/`qm` use `.tag()`; `pagination.rs` uses `!cell.is_attribute()`; `query_isolation.rs` writes `Eq` as plain `held == value` (no tag check).
- **Range check**: `queries.rs:306-340` (`in_range`, the I64 arm copy-pasted for U64, 34 lines), `pagination.rs:581-609` (`ordered` + `within`, i128 widening), `query_metering.rs:576-610`, `query_isolation.rs:545-575`. All four only order `I64`/`U64`.
- **Query = OR of AND groups**: `queries.rs:204-208` (inline in `matches`), `pagination.rs:550-556`, `query_isolation.rs:606-614`.

Fix: one `tests/support/model.rs` (the directory exists: `tests/support/durable_history.rs`, wired with `#[path]` at `durable.rs:123`) holding

```rust
pub type Cells = Vec<(CellName, Cell)>;
pub struct Model { /* id-keyed map + key index, as pagination.rs:455-460 */ }
impl Model {
    pub fn create(&mut self, key: &RecordKey, cells: &[(CellName, Cell)]) -> RecordId;
    pub fn patch(&mut self, key: &RecordKey, set: &[(CellName, Cell)], remove: &[CellName]) -> RecordId;
    pub fn delete(&mut self, key: &RecordKey) -> RecordId;
    pub fn row(&self, key: &RecordKey, projection: &Projection) -> Option<Row>;
    pub fn evaluate(&self, query: &Query, projection: &Projection) -> Vec<Row>;   // ascending id
    pub fn distinct(&self, name: &CellName, tag: TypeTag) -> Vec<Value>;          // terms.rs:364-375
    pub fn term_count(&self) -> usize;                                            // one algorithm, not three
    pub fn len(&self) -> usize;
}
```

`pagination.rs`'s layout (id map + key index) serves all four files; `terms.rs` keeps only `terms`/`terms_cost` locally. Removes about 290 lines from this chunk (87 + ~50 + 197 + 161 minus one ~200-line shared copy) and about as much again from `query_isolation.rs`/`query_metering.rs`/`query_history.rs`. Risk: needs-care (the oracle must stay independent of the engine; keep it free of `Engine`/`Branch` types).

#### 1.2 `records.rs` and `terms.rs` share ~180 byte-identical helper lines - safe

Verified with `diff` on the line ranges:

| block | `records.rs` | `terms.rs` |
|---|---|---|
| `ns_id`, `name`, `key`, `rid`, `attr`, `field` | 115-137 | 88-110 |
| `type Cells` | 139 | 128 |
| `create`, `patch`, `delete_record` | 151-170 | 130-149 |
| `receipt`, `applied`, `rejected`, `read_applied`, `read_rejected`, `ledger`, `traced` | 195-242 | 151-198 |
| `read_admission` (doc comment differs by word order only) | 2422-2429 | 200-207 |
| `apply` | 263-267 | 209-213 |
| `fresh_branch` | 276-280 | 215-219 |
| `Model::{create,patch,delete}` | 343-370 | 311-338 |
| `largest_prefix_sum_at_most` | 515-524 | 539-548 (third copy: `metering.rs:209-218`, doc differs by "`b`") |
| `assert_read_sweep` | 597-636 | 550-589 |

Fix: move to `tests/support/typed.rs`; delete both copies. ~180 lines removed from this chunk.

#### 1.3 Inventory of generic helpers defined locally (for the shared `tests/support` design)

Copies counted with `rg -n '<signature>' crates/arkiv-db/tests`.

| helper | `records.rs` | `terms.rs` | `queries.rs` | `pagination.rs` | differences that matter |
|---|---|---|---|---|---|
| `const ALICE` / `BOB` | 50-51 | 54-55 | 57 (no BOB) | 64-65 | identical (`[0xa1;20]`, `[0xb0;20]`); 22 files define ALICE, `bounds.rs:6` uses `[1;20]` |
| `const MODEL_VERSION = 3`, `FIXTURE_ID = 1` | 54, 56 | 58, 60 | 59, 61 | 68, 70 | 7 files each |
| FIXTURE weight consts | 63-78 (16) | 67-75 (9) | none: bare literals in ledgers | 100-106 (7) | three partial transcriptions of one table |
| `name(&str) -> CellName` | 119-121 | 92-94 | 67-69 | 205-207 | records/terms panic with the fault; queries/pagination use `.expect(..)` with two different messages. 19 files |
| `key(&str) -> RecordKey` | 123-125 | 96-98 | 71-73 | 209-211 | identical. 6 files |
| `rid(u64) -> RecordId` | 127-129 | 100-102 | **`id`** 75-77 | 213-215 | name differs (`id` in queries/qm/trace_regression). 9 files |
| `ns_id(u64) -> NamespaceId` | 115-117 | 88-90 | 79-81 | none (`NamespaceId::from_ordinal(99)` inline at 1887) | queries builds it with `from_bytes(ordinal.to_be_bytes())`, the others with `from_ordinal` |
| `attr`, `field` | 131-137 | 104-110 | 83-89 | 217-223 | identical one-line wrappers of `Cell::attribute`/`Cell::field`. 7 files |
| `create`/`patch`/`delete_record` -> `WriteOp` | 151-170 | 130-149 | none (struct literals inline, 11 sites incl. `World` 547-596) | none (struct literals inline, 16 sites) | qm's take `&str` keys |
| `put`, `raw_delete`, `transfer`, `entry` | 172-193, 2257-2262 | - | - | - | |
| `receipt(..)` | 195-204 `(kind, spent, outcome)` | 151-160 same | `read_receipt(spent, outcome)` 102-111 | 360-369 `(kind, schedule_id, spent, outcome)` | pagination needs the schedule id (it tests schedule 2). 12 files |
| `applied(spent)` | 206: **Write** receipt | 162: Write | 113: **Read** receipt | - | same name, opposite `ReceiptKind`. Rename queries' to `read_applied` |
| `rejected`, `read_applied`, `read_rejected` | 210-220 | 166-176 | `zero_cost_read(reason)` 1591-1593 | `read_receipt` 371, `read_applied` 375 | |
| `ledger`/`traced` | 223-242: lines are `(class, count, unit_price)`, subtotal computed | 179-198 same | 118-133: `(class, count, unit_price, subtotal)` | 380-399: 4-tuple | 3-tuple vs 4-tuple; `metering.rs:97`, `namespaces.rs:109` use the 4-tuple, `qm:283` takes a `Ledger` |
| `admission(reason) -> Rejected` / `read_admission` | 245-251 / 2423-2429 | - / 201-207 | `expect_fault(result, QueryFault)` 1595-1607 | `rejection` 402-410, `assert_zero_cost_under` 414-434, `assert_zero_cost` 436, `assert_mismatch` 440 | four ways to say "rejected at admission, spent 0" |
| `canonical(&WriteResult)` | 254-261 | - | - | - | also `metering.rs:129`, `qm:316` |
| `apply(&mut Branch, OwnerId, WriteOp) -> Executed` | 263-267 | 209-213 | none | 615-620 | pagination's has a different panic text; `isolation.rs:922`/`query_isolation.rs:2716` return `Effect` |
| `reject(..) -> Rejected` | 269-274 | - | - | - | `rollback.rs:125` takes a `Budget` |
| `fresh_branch(&Engine) -> Branch` | 276-280 | 215-219 | none: `engine.begin(engine.head(), None).expect(..)` inline 6x | none: inline 10x | `metering.rs:168`/`qm:707` take `block_budget: Option<u64>` |
| `publish(&mut Engine, Branch) -> SnapshotId` | none (inline 9x) | 221-225 | none | `Head::publish` 830 | `namespaces.rs:168` returns `Snapshot` |
| empty-namespace fixture | `fixture()` 283-299 | `fixture_with_limits` 229-242, `fixture()` 245-247 | inline in `semantics_fixture` 613-637, `small_fixture` 1558-1589, `cost_fixture` 2112-2168 | `create_namespace` 622-635, `fixture()` 647-699 | `WriteOp::CreateNamespace { name: arkiv_db::NamespaceName::new("test").unwrap() }` is spelled out 10x in this chunk (66x in 13 test files), always with the full path |
| Branch/Snapshot read abstraction | `trait TypedView` 433-488 | `trait TermsView` 428-466, `for_each_view` 469-472 | `trait Reader` 901-972 | none (takes `&Snapshot`) | three single-file traits with two identical impl bodies each; `Reader` also at `query_isolation.rs:852`, `isolation.rs:177`, `query_history.rs:1494`, `history.rs:849`, `blocks.rs:192` |
| budget sweep | `assert_write_sweep` 539-595, `assert_read_sweep` 599-636 | `assert_read_sweep` 552-589 | - | - | strict subsets of `qm`'s `assert_write_vector` (`qm:808-900`) and `assert_read_vector` (`qm:905-964`), which also run every budget traced and at `total + 1` |
| cursor walk | - | - | `walk<R: Reader>(reader, ns, query, projection, limits: &[usize], budget) -> Vec<Row>` 981-1030 | `walk(&Snapshot, ns, query, projection, limit_of: impl FnMut(usize)->usize, between: impl FnMut(&[Row])) -> Vec<Page>` 726-794 | same six cursor invariants asserted in both; also `query_planner.rs:469`, `eq_proofs.rs:143` |

#### 1.4 Three per-file traits that only forward to `Branch`/`Snapshot` - safe

`records.rs:433-488` (`TypedView`, 56 lines), `terms.rs:428-466` (`TermsView`, 39), `queries.rs:901-972` (`Reader`, 72). In each, the `impl .. for Branch` and `impl .. for Snapshot` bodies are character-for-character the same forwarding calls (`records.rs:445-464` vs `468-487`; `terms.rs:441-451` vs `455-465`; `queries.rs:931-945` vs `953-967`). Fix: one `support::View` trait (`query`, `count`, `get_record`, `terms`, `typed_record_count`, `term_count`, `index_consistent`, `pinned() -> Option<SnapshotId>`) implemented once with a 10-line `macro_rules!` for the two types. ~100 lines removed here, more in the five other files that define a `Reader`.

#### 1.5 `walk` duplicated between `queries.rs` and `pagination.rs` - safe

`queries.rs:981-1030` and `pagination.rs:726-794` assert the same invariants in the same order (page <= limit; a continued page is non-empty; ids ascend past the cursor; `next` only on a full page; `next.after()` is the last row; `next` names namespace/query/projection/snapshot). Keep `pagination.rs`'s (it returns the pages and has the `between` hook), make it generic over the shared view trait, and express `queries.rs`'s as `rows_of(&walk(reader, .., |i| limits[i % limits.len()], |_| {}))`. ~50 lines.

#### 1.6 `queries.rs:254-282` `type_of` re-implements `Value::tag()` - safe

A 25-arm match "decided by variant here rather than by the engine". The fixture only uses Bool/I64/U64/Str/Bytes; the models in `records.rs:416`, `terms.rs:369`, `pagination.rs:569-576` and `qm:566-572` all trust `.tag()`, which `golem_types.rs:152-154` pins for every variant. Fix: delete `type_of`, use `.tag()` (call sites: 243, 294, 295, 300). -29 lines.

#### 1.7 `queries.rs:306-340` `in_range`: the I64 arm pasted for U64 - safe

Fix (if the model is not shared per 1.1): one generic helper,

```rust
fn bounded<T: Ord>(held: &T, lower: Bound<&T>, upper: Bound<&T>) -> bool { (lower, upper).contains(held) }
```

fed by a `fn as_i64(&Value) -> Option<&i64>` / `as_u64` mapping of the bounds, or adopt `pagination.rs:581-609`. -20 lines.

#### 1.8 Four "cost fixtures" and three charge-table transcriptions

`records.rs:80-109` (a, b, c with kind/price/$expiresAt/note), `terms.rs:708-726` (a, b, c with price 10/20/5 and kind "", "a", "ab"), `queries.rs:2097-2168` (k1..k6), `pagination.rs:82-92` (a..f). `records.rs`'s is also `qm:355-366` verbatim (`fixture_cells`, `FIXTURE_RECORDS`). Not a mechanical fix; listed because 2.1/2.2 remove two of them.

### II.1 [S1] arkiv-db src: engine, typed, planner, record, layout, codec, meter

*(paths relative to `crates/arkiv-db/src/`)*

#### 1.1 engine.rs - the infallible / `try_` API pairs (the biggest single item, ~230 lines)

Measured:
- 30 infallible methods whose entire body is `in_memory(self.try_x(args))` (`in_memory` = engine.rs:166-171, `match` + `panic!`). Snapshot 14: lines 912, 924, 943, 966, 984, 1000, 1025, 1040, 1068, 1101, 1128, 1170, 1239, 1267. Branch 16: 1630, 1642, 1657, 1675, 1690, 1713, 1739, 1876, 1893, 2036, 2057, 2077, 2095, 2129, 2155, 2193. They match the 30 Branch/Snapshot entries of `crates/arkiv-node/clippy.toml` exactly.
- Plus `Branch::checkpoint` (1963-1968) and `Branch::seal` (2313-2318), which hand-roll the same `match try_x() { Ok(v) => v, Err(e) => panic!(..) }` instead of a shared helper, and `host::expire_records` (host.rs:132).
- Size: 162 lines of wrapper code + 30 blank separators + 32 doc blocks "Fallible form of [`x`](Self::x)." (40 lines) = ~232 lines. Every signature is written twice.
- Used outside tests? No. `rg -n "\.(execute|execute_host|estimate|estimate_host|get_record|descriptor|namespace_count|record_count|typed_record_count|term_count|index_consistent|records_shared_with|typed_shared_with|scan_prefix|query|terms)\(" crates/*/src | grep -v try_` hits only `#[cfg(test)]` modules (durable/mod.rs:1150,1159 after `cfg(test)`@1126; engine.rs:3071-3111 after `cfg(test)`@2992). No `allow(clippy::disallowed_methods)` exists anywhere. Consumers are tests/examples/conformance only: ~1127 call lines in 45 files (`crates/*/tests`, `crates/*/examples`, `conformance/`).
- `Snapshot::try_namespace_count` (918-920) cannot fail (`Ok(count(..))`); it exists only for symmetry.

Can they be generated? Yes - **needs-care, no behaviour change, no call-site churn**: one `macro_rules!` with two arms (`&self`, `&mut self`) that takes the `try_` definition and emits the infallible twin plus its one-line doc:
```rust
fallible! {
    /// Returns the value of `key` in `namespace`, metered as a read.
    pub fn get / try_get(&self, namespace: NamespaceId, key: &[u8], budget: Budget)
        -> ReadResult<Option<Vec<u8>>> { /* body returning Result<_, StorageError> */ }
}
```
Method paths stay `arkiv_db::engine::Branch::get` etc., so clippy.toml keeps working. Saves ~200 lines and makes it impossible for a pair to drift.
Can they be removed? Only by rewriting ~1127 test call lines to `try_x(..).unwrap()`; not worth it. Cheaper middle step if the macro is unwanted: move all 30 wrappers into one trailing `impl Snapshot` / `impl Branch` block (or `engine/infallible.rs`) so the main impls read as the real API.

#### 1.2 engine.rs - read plumbing repeated per method (~100 lines)
- All nine `read_*`/helper fns take the same leading triple `(state: &WorkingState, nodes: &Nodes, pricing: &PriceSchedule, ..)` (engine.rs:2340, 2357, 2657, 2704, 2774, 2790, 2885, 2921, 2962) and `limits` is threaded separately or inside `QueryRequest`/`TermsRequest`.
- The 14 `try_` read methods are positional re-listings of those arguments: Snapshot 1050-1065, 1079-1097, 1106-1122, 1141-1160, 1183-1207, 1249-1264, 1276-1283 (121 lines); Branch 2046-2053, 2067-2074, 2086-2092, 2106-2125, 2134-2151, 2168-2186, 2206-2228 (103 lines).
- Fix: `struct View<'a> { state: &'a WorkingState, nodes: &'a Nodes, pricing: &'a PriceSchedule, limits: &'a Limits }` with methods `get/scan/descriptor/record/query/count/terms`; `Snapshot::view()` builds it once, `Branch::read` hands it to the closure. Removes the `#[allow(clippy::too_many_arguments)]` at engine.rs:2786 and the `limits` field of both request structs. **needs-care** `[CONSENSUS]` (read receipts; no charge moves).

#### 1.3 codec.rs - 36 `Encode` + 34 `Decode` impls; two shapes are mechanical (~150 lines)
Breakdown: primitives 5+5, containers 8+6, newtypes 9+9, fieldless enums 4+4, structs 5+5, data enums 5+5.
- **Fieldless enums** (`CellKind` 897-918, `ArgumentFault` 1135-1168, `QueryFault` 1170-1209, `ReceiptKind` 1320-1341) plus their const tables (65-67, 88-96, 98-109, 131-133): 145 lines in which every tag appears three times (const, encode arm, decode arm). One macro `unit_enum_codec!(QueryFault { NoGroups = 1, TooManyGroups = 2, .. })` emitting both impls with `kind: stringify!($ty)` -> ~45 lines. -100 lines, and encode/decode can no longer disagree. **needs-care** `[CONSENSUS]`: tags must stay identical; `crates/arkiv-db/tests/codec.rs` goldens pin them.
- **Primitives** `u8/u32/u64/i64/bool` (502-566, 65 lines): `primitive_codec!(u32 => u32, 4; ..)` -> ~15 lines. -50. **safe**.
- `NamespaceName` (740-753) and `CellName` (778-791) are identical bodies. -10 with the same macro or a tiny helper. **safe**.
- Leave `Value`, `WriteOp`, `Effect`, `Rejection`, `Outcome`, `Receipt`, `PriceSchedule`, `Limits` hand-written: they are genuinely different and a macro would hide the layout.

#### 1.4 typed.rs + typed/planner.rs - walking a term run, three copies (~70 lines)
- Lower bound -> `(start, skip_start)`: typed.rs:746-756, typed.rs:1096-1106, planner.rs:548-559. Identical 11-line `match` (only the `expect` text differs). Home: `TermKey::run_from(name, tag, lower: &Bound<Value>) -> (TermKey, bool)` next to `TermKey::run_start` (layout.rs:244). -22. **safe**.
- Loop preamble (next / storage error / left the `(name, tag)` run / existence term / skip excluded start): typed.rs:763-776 (`terms`), typed.rs:975-988 (`open_run`), planner.rs:580-593 (`walk_run`); upper-end test typed.rs:989-1001 (`RunEnd`) vs planner.rs:565-572 + 594-602 (`Shape` + `end`). Home: one `ValueRun` iterator in typed.rs yielding `(value bytes, Posting)` and exposing `probe()`; each caller keeps only its own charge and body. -40. **needs-care** `[CONSENSUS]`: `TermVisit` must stay charged after the end test and before the posting is opened.
- Posting priming for the merge: typed.rs:1005-1013 vs planner.rs:852-863 (same 8 lines).
- Term lookup: `open_posting` typed.rs:1025-1040 and `planner::lookup` planner.rs:446-457 are the same four statements (`TermSeek` charge, probe, get, note); `open_posting` should be `lookup(..)?.map_or(Source::Empty, |p| Source::Posting(p.range_from(..)))`. -8. **safe** (charge order identical).

#### 1.5 typed/planner.rs - `plan()` (621-794, 174 lines; clippy too_many_lines 161/100) (~55 lines)
- Five match arms (640-700) each do `lookup` + push `Found { term, posting, order }`; fold into a local `find(term) -> Result<Option<Found>, Fault>`. -30.
- The dedup closure is pasted three times (748-752, 753-757, 782-786) -> `fn dedup_terms(found: &mut Vec<Found>, seen: &mut Vec<TermKey>)`. -10.
- `tighter_lower` / `tighter_upper` (459-491) are mirror images -> one fn taking the `Ordering` to keep. -14.
- `GroupStream { followers: Vec::new(), absent: Vec::new(), head: None, head_row: None, .. }` literal three times (typed.rs:1133-1140, planner.rs:907-914, 929-936) -> `GroupStream::new(source, probes)`.
- Obvious split of `plan`: `classify` (633-706), `pick_run` (711-738), `assemble` (739-793). All **needs-care** (planned reads are local-only, not canonical, but results must stay equal to written evaluation; `tests/query_planner.rs` covers).

#### 1.6 engine.rs - write path (~75 lines)
- `DirectoryLookup` charge immediately followed by `resolve` occurs 14 times: `apply` 2487, 2527, 2556, 2589, 2604, 2615 (each then `authorize`), `get_inner` 2682, `scan_inner` 2729, and as `.charge(..).map_err(Fault::from).and_then(|()| resolve(..))` at 2779-2782, 2803-2806, 2895-2898, 2933-2936, 2974-2977. Home: `fn lookup(state, nodes, namespace, meter) -> Result<NamespaceState, Fault>` and `fn owned(.., authority, ..)` = lookup + authorize. -25. **needs-care** `[CONSENSUS]` (order unchanged).
- `apply` (2440-2625, clippy too_many_lines 170/100): the three typed arms 2584-2597, 2598-2613, 2614-2623 differ only in the `typed::` call and the `Effect` variant; Put/Delete (2522-2583) share their tail. Split into `create_namespace`, `transfer_namespace`, `put`, `delete`, `typed_write(.., f)`. -20. **needs-care** `[CONSENSUS]`.
- `execute_as` (1805-1868) and `estimate_as` (1906-1953) repeat admit + `Meter::new` + `apply` + receipt assembly; the `(receipt, physical) = meter.finish(..)` -> `Executed/Rejected/Metered { .. }` block is written 7 times (1847-1853, 1861-1867, 1935-1940, 1943-1949, 2345-2351, 2635-2640, 2643-2648); the `self.spent = self.spent.checked_add(meter.spent()).expect(..)` statement is duplicated at 1842-1845 and 1856-1859. Fix: `finish_write(meter, Result<Effect, Rejection>) -> WriteResult` mirroring `finish_read` (2629), and hoist the spend update. -30. **needs-care** `[CONSENSUS]`.

#### 1.7 engine.rs - statistics and constructors (~45 lines)
- Six copies of `namespace(nodes, ns)?.map(|s| count(<len>))`: Snapshot 929-935, 971-980, 989-995; Branch 1647-1653, 1662-1671, 1680-1686. `try_records_shared_with` (948-962) and `try_typed_shared_with` (1005-1019) differ in one expression. Home: `WorkingState::stat(nodes, ns, f)` and a private `Snapshot::shared_with(other, ns, same)`. -30. **safe**.
- Header -> `WorkingState` twice: `Engine::from_state` 500-512 and `Snapshot::from_header` 714-724, with different validation (see 4.6). Home: `WorkingState::from_header(&StateHeader)`. -12. **needs-care**.

#### 1.8 Smaller
- `RecordKey` bytes are produced/checked twice: `TrieKey for RecordKey` layout.rs:132-150 and `Encode/Decode for RecordKey` codec.rs:795-813, same `len ‖ bytes`. Layout should delegate to the codec as `TrieValue for RecordId` already does (layout.rs:407-418). -12. **needs-care** `[CONSENSUS]` (bytes identical; pinned by layout.rs:754 test; only error text changes).
- layout.rs:336-339 re-implements `TypeTag::from_id` (record.rs:284): use `TypeTag::from_id(tag_id).filter(|t| t.is_indexable())`. Same in the test helper layout.rs:780-783. **safe**.
- record.rs: the tag table is written four times (enum 195-246, `ALL` 250-276, `from_id` 284-313, `name` 321-349). `from_id` can be a `const` `while` loop over `ALL` comparing `as u8`. -22. **needs-care** `[CONSENSUS]` (ids pinned by `tests/codec.rs:402 type_tags_and_values`).
- meter.rs: `check_capacity` 626-639 repeats the checked subtotal/sum/limit arithmetic of `charge_group` 655-673. Home: private `fn priced(&self, items) -> Result<Cost, Rejection>`. -8. **needs-care** `[CONSENSUS]`.
- `fn units(length: usize) -> u64` is identical at engine.rs:2386-2388 and typed.rs:144-146 (a third copy lives in `crates/arkiv-node/src/params.rs:154`). Keep one `pub(crate)`. **safe**.
- typed.rs admission: page-limit checks 285-290 == 719-724; the indexed-`Str` test at 232 == 715; `validate_create` (195-201) is a pure pass-through to `validate_cells`. -12. **safe**.
- layout.rs:84-86 `encode_leaf` is an alias of `codec::encode` with one caller (408); `decode_leaf` (80-82) has one caller (417). Inline. **safe**.

### II.1 [NT2] arkiv-node tests: signed

*(paths relative to `crates/arkiv-node/tests/`)*

#### 1.1 `signer` / `genesis` re-definitions (emphasis a)

`signed_support/mod.rs` exports `signer(byte: u8)` (:28-30, key `[byte; 32]`) and
`genesis(chain_id, timestamp_ms, run_id, proposer: u8, block_period_ms, funded: &[u8])` (:61-76, default native genesis,
every listed actor + the proposer funded with `balance()` = 10^24). Only signed_host.rs, signed_history.rs,
ethereum_transactions.rs and gas_execution.rs include the module.

`signer`:

| where | signature | key | differs from support how |
|---|---|---|---|
| signed_http.rs:55-59 | `fn signer(who: u8) -> Signer` | 31 zero bytes + `who` (private key = who) | different key bytes only; no address/hash literal in the file depends on it (rg `"(0x)?[0-9a-f]{16,}"` finds none) |
| signed_retention.rs:27-31 | `fn signer(who: u8) -> Signer` | same | byte-identical body to signed_http.rs:55-59; no pinned literals |
| signed_protocol.rs:16-20 | `fn signer(n: u8) -> Signer` | same | PINNED: address literal :164-166 (`7e5f4552...`), golden hashes/signatures :215-242 |
| signed_pool.rs:11-15 | `fn signer(n: u16) -> Signer` | `n` big-endian in the last 2 bytes | same key as the three above for n < 256; needs u16 (senders 1..=257 at :342-346) |
| signed_reads.rs:139-141 | `fn signer() -> Signer` | `[1; 32]` | exactly `signed_support::signer(1)` |
| ethereum_transactions.rs | none; `Signer::from_bytes([N; 32]).unwrap()` written out 8x (:30, :145, :177, :186, :212, :318, :319, :364) | `[1; 32]`, `[2; 32]` | exactly `signed_support::signer(N)`, and `mod signed_support` is already declared at :10 |
| signed_host.rs:26, signed_history.rs:27 | use the support fn | | fine |
| outside chunk | accounts.rs:21-23 `signer(n: u8)` `[n; 32]`; gas_execution.rs:18-20 `signer()` `[1; 32]` (the same file also calls `signed_support::signer(7)`); genesis_prefill.rs:22-24 `signer(n: u8)`; genesis_adopt.rs:57-59 and prebuilt_genesis_cli.rs:41-43 `signer()` | | all equal to the support fn |

`genesis`:

| where | signature | content | differs how |
|---|---|---|---|
| signed_host.rs:27-29 | `fn genesis() -> SignedGenesis` | `signed_support::genesis(9100, 1000, [7; 16], 1, 100, &[1, 2, 3])` | thin wrapper, fine |
| signed_history.rs:24-29 | `fn signed_genesis() -> (SignedGenesis, Signer)` | support `(9003, 1000, [7; 16], 1, 100, &[1])` + `signer(1)` | fine; every caller destructures and re-threads both (see 1.4) |
| ethereum_transactions.rs:11-13 | `fn genesis()` | support `(9300, 1000, [7; 16], 2, 1000, &[2])` - proposer **2** | :178 and :311 build the same chain inline with proposer **1** (comment :310 says why 1 is needed). Three constructions, two proposers; the proposer-2 one is used only by decode/validate tests. Looks accidental: one `genesis()` with proposer 1. needs-care (verify the four decode tests do not need proposer 2) |
| signed_http.rs:60-75 | `fn genesis()` | hand-built: chain 7, ts 1_000, period 250, run `[0x42; 16]`, proposer 1, funds 1,2,3 with `U256::from(10u64).pow(U256::from(24u64))` | re-implements `balance()` + `allocations()` + `genesis()`; equals `signed_support::genesis(7, 1_000, [0x42; 16], 1, 250, &[1, 2, 3])` except for the key bytes of 1.1 |
| signed_retention.rs:32-47 | `fn genesis()` | same as signed_http except run `[0x43; 16]` and funds 1,2 | same remark |
| signed_protocol.rs:24-37 | `fn genesis()` | chain 7, 1_000, 250, `[0x42; 16]`, proposer 1, funds 1,2 with 1 GLM (`glm()` :21-23) | PINNED by the golden genesis hash :215-218 and the hand-built preimage :140-157. Keep local. `[CONSENSUS]` |
| signed_pool.rs:16-29 | `fn genesis()` | chain 7, 1_000, 250, `[0x42; 16]`, proposer 1, funds only 1 with an inline `1_000_000_000_000_000_000u64` | third balance spelling in the chunk |
| signed_reads.rs:175-187 (inside `node`) | struct literal `SignedGenesis { .. }` | chain 9515, 1_000, 1_000, run `[7; 16]`, proposer `[1; 32]`, 10^24 inlined (:183), `limits: Default::default()` | it is the unvalidated base for `build_genesis`; could still take `accounts` from `signed_support::allocations` |
| outside chunk | accounts.rs:27-40 (9300, 1000, 1000, `[7; 16]`, 10 GLM / 2 GLM); gas_execution.rs:21-23 (support wrapper) | | |

Fix (needs-care, test keys change but nothing is pinned to them in these files): signed_http.rs:55-75 and
signed_retention.rs:27-47 become `use signed_support::signer;` + a 3-line `genesis()` wrapper (-36 lines);
signed_reads.rs:139-141 -> `signed_support::signer(1)`; ethereum_transactions.rs 8 inline signers -> `signed_support::signer(n)`.
signed_protocol.rs keeps its own genesis (pinned) but takes the scalar signer from support (below); signed_pool.rs likewise.

#### 1.2 Every other locally re-defined generic helper

Host construction over two fresh memory stores with the genesis certificate signed by signer 1 - 8 sites in the chunk:
- signed_http.rs:76-89 `fn host(producer: bool) -> Host` (source id `[1; 16]`)
- signed_http.rs:944-960 `fn full_with_retained_history() -> (Host, Arc<MemoryVersionedStore>)` = `host(false)` that also returns the history store
- signed_host.rs:30-47 `fn create(state: Arc<dyn VersionedStore>, history: Arc<dyn VersionedStore>, producer: bool) -> Host` (`[3; 16]`); :48-54 `memory(producer)`; :55-69 `reopen(state, history, producer)`; :898-910 `chain(genesis: &SignedGenesis, producer: bool)` (= `create`+`memory` with the genesis as a parameter); inline again :1299-1309
- signed_retention.rs:83-97 inside `node(retention: Retention) -> (Shared, String)` (only `Options { retention, .. }` differs)
- ethereum_transactions.rs:179-190 and :312-323: the same 12-line `make_host` closure twice (`[9; 16]`)
- signed_reads.rs:218-234: `create_with_prefill` over `SlowStore` (genuinely different)
- outside chunk: accounts.rs:41 `create_host(g, p, state, history)` and :59 `host(g, p)`, gas_execution.rs:24-39 `host`/`host_of`

Signed transaction builders - 8 sites:
- signed_http.rs:90-128 `fn create(who: u8, nonce: u64, salt: u128) -> SignedTx` (budget 2_000_000; request id = `who` in byte 0, nonce BE in 24..)
- signed_host.rs:70-91 `fn tx(key: u8, nonce: u64, request: u8, operations: Vec<ArkivOperation>) -> SignedTx` (budget 10_000_000; request id `[request; 32]`)
- signed_host.rs:911-939 `fn tx_on(genesis: &SignedGenesis, key: u8, nonce: u64, operations: Vec<ArkivOperation>) -> SignedTx` (request id `[key; 32]` with nonce BE in ..8)
- signed_host.rs:1324-1343 inline (request id `[9; 32]` + nonce; budget 10M/20M)
- signed_reads.rs:262-287 `fn create(genesis: &SignedGenesis, nonce: u64, value: u64) -> SignedTx` (request id nonce BE in 24..)
- signed_retention.rs:48-79 `fn create(nonce: u64) -> SignedTx` (same request-id layout and budget as signed_reads)
- signed_pool.rs:46-65 `fn tx(g: &SignedGenesis, who: u16, nonce: u64, budget: u64) -> SignedTx` (CreateNamespace; budget in intrinsic-gas units) and an inline re-sign :358-375
- signed_protocol.rs:38-51 `envelope(who)` / `signed()` - golden-pinned, leave

`ArkivOperation::Create` builders: signed_http.rs:103-122 (inline), signed_host.rs:93-112 `create_entity(salt: u128, body: &[u8])`,
:884-897 `create_until(salt: u128, height: u64)`, :1278-1286, signed_reads.rs:272-283, signed_retention.rs:62-73, signed_pool.rs:360-374;
outside: gas_execution.rs:41-60 `create(salt: u128, body: usize, lifetime: u64)`, accounts.rs:90 `create(salt: u128) -> TxOp`.
The `Vec<ArkivOperation> -> Vec<TxOp::Arkiv>` map is written 3x in signed_host.rs alone (:80-86, :928-934, `ops_of` :1390-1398, which is defined after its only use at :1339).

HTTP server on an ephemeral port:
- signed_http.rs:40-54 `struct Server { url, task }` + `Drop` + `async fn serve(router: Router) -> Server`
- signed_reads.rs:156-170: identical (diff shows only `Router` vs `axum::Router`)
- signed_retention.rs:272-278 `async fn serve(rt: Shared) -> (String, JoinHandle<()>)`, aborted by hand at :368
- arkiv_profile.rs:249-256 `async fn server(router) -> (String, JoinHandle<()>)` nested inside one test, so the next test writes the same four lines out twice (:457-458 + :468, :469-470 + :474); aborted by hand at :427-428, :491-492
- outside: simulator_http.rs:27-40 (`Server` + `serve` again)

HTTP request helpers:
- signed_http.rs:129-136 `post(client, url, path, body) -> reqwest::Response`; :137-145 `get(client, url, path) -> Value`; :146-157 `step(client, url)`; :520-527 `spawn_post`
- signed_reads.rs:305-315 `submit`, :316-325 `step(client, node)` (retries 429 through `until_admitted` :328-340; signed_http's `step` does not)
- signed_retention.rs has none: 8 hand-written `client.get/post(..).send().await.unwrap()` chains (:285-292, :300-307, :310-317, :321-326, :329-334, :338-346, :347-352, :362-366)
- arkiv_profile.rs has none: 10 such chains (:291-298, :312-319, :325-332, :334-342, :357-365, :373-380, :387-394, :407-417, :419-425, :475-479)

JSON-RPC wrappers:
- signed_reads.rs:342-351 `async fn rpc(client, url, method, params) -> (u16, Value)` (over HTTP)
- signed_retention.rs:115-121 `async fn call(rt: &Shared, method: &str, params: Value) -> Value` (in-process `rpc::handle_request`)
- ethereum_transactions.rs:368-373: the same body as a closure
- signed_http.rs:626-633 and :674-680: the envelope and send written inline

Polling / waiting: signed_http.rs:510-519 `until(what, condition)` (10 s limit, 1 ms poll) vs signed_reads.rs:357-363 `wait_for(what, limit, condition)` (5 ms poll).

Smaller ones:
- `fn message<T>(result: arkiv_node::signed::Result<T>) -> &'static str`: signed_host.rs:149-151 == signed_history.rs:46-48
- `const TOKEN`: signed_http.rs:39 == signed_reads.rs:66 (same literal)
- `head(rt)`: signed_http.rs:158-160 returns the `Header`, signed_reads.rs:291-293 returns `u64` (same name, different type)
- "lock the runtime, take the full host or panic": signed_http.rs:698-704 (`panic!("producer")`), :1072 (`panic!()`), signed_reads.rs:373-377 (`panic!("full node")`), signed_retention.rs:253-259 (`unreachable!()`)
- `EqRequest` at a host's head: signed_host.rs:256-270, :1090-1104 (15 lines each), signed_history.rs:691-705
- `hex` / `unhex`: signed_protocol.rs:123-125 (no `0x`; same body as tests/support/mod.rs:318-320, genesis_adopt.rs:60-62, prebuilt_genesis_cli.rs:47-49) and :126-133 (`unhex<const N: usize>(s) -> [u8; N]`). The library's `simulator::wire::hex`/`unhex` (crates/arkiv-node/src/simulator/wire.rs:13-18) prefix/require `0x` and return `Vec`, so they are not drop-in; `alloy_primitives::hex::encode` and `alloy_primitives::hex!` are (already used at crates/arkiv-node/src/genesis_bundle.rs:750, ethereum_transactions.rs:15, crates/arkiv-db/tests/codec.rs:39/:1244). -11 lines, safe (the pinned strings do not change).
- temp paths, three styles, none removed when the test panics: signed_http.rs:1553-1556 + :1610 (`getrandom`), signed_history.rs:147-150 + :168 (`DatabaseId::random()` used as a unique token), signed_host.rs:1250-1253 + :1388 (process id). Outside the chunk genesis_adopt.rs:42-55 and prebuilt_genesis_cli.rs:26-38 hold two identical `struct Temp(PathBuf)` with `Drop`; that is the one to promote.

#### 1.3 Proposed single home

Two files, because the chain builders are signed-specific and the socket plumbing is not (arkiv_profile.rs and simulator_http.rs need it too).
`#[path]` inclusion already has a precedent (block_model.rs:2085).

`tests/signed_support/mod.rs` (exists; add):

```rust
pub fn signer(byte: u8) -> Signer;                 // exists: [byte; 32]
pub fn scalar_signer(n: u16) -> Signer;            // private key = n; n = 1 is the 0x7e5f.. address pinned in signed_protocol.rs
pub fn glm(n: u64) -> U256;                        // n * 10^18; balance() == glm(1_000_000)
pub fn host(genesis: &SignedGenesis, producer: bool) -> Host;                    // fresh memory stores, proposer = genesis.proposer's signer
pub fn host_on(genesis: &SignedGenesis, state: Arc<dyn VersionedStore>, history: Arc<dyn VersionedStore>,
               producer: bool, options: Options) -> Host;
pub fn reopen(genesis: &SignedGenesis, state: Arc<dyn VersionedStore>, history: Arc<dyn VersionedStore>, producer: bool) -> Host;
pub fn arkiv_ops(operations: Vec<ArkivOperation>) -> Vec<TxOp>;
pub fn request_id(tag: u8, nonce: u64) -> [u8; 32];
pub fn sign(genesis: &SignedGenesis, signer: &Signer, nonce: u64, request_id: [u8; 32], tx_budget: u64, ops: Vec<TxOp>) -> SignedTx;
pub fn create_op(salt: u128, expiry: Expiry, attributes: Vec<(CellName, Cell)>) -> ArkivOperation;
pub fn eq_at_head(host: &Host, attribute: &str, value: Value) -> EqRequest;
pub fn message<T>(result: arkiv_node::signed::Result<T>) -> &'static str;
pub fn with_host<R>(rt: &Shared, f: impl FnOnce(&mut Host) -> R) -> R;
pub async fn call(rt: &Shared, method: &str, params: serde_json::Value) -> serde_json::Value;   // rpc::handle_request
```

The proposer's signer has to be known to `host`: either pass `proposer: u8` as today's `genesis()` does, or return a
`struct Chain { genesis: SignedGenesis, proposer: Signer }` from the genesis builders (signed_history.rs already wants exactly that pair, :24-29).
While there: `genesis()` takes six positional arguments, four of them integers (`genesis(9100, 1000, [7; 16], 1, 100, &[1, 2, 3])`);
`timestamp_ms` is `1000` at all six call sites and `run_id` is `[7; 16]` at five (rg `signed_support::genesis\(`), so both can become fixed
and the call reads `genesis(9100, 100, &[1, 2, 3])` (proposer 1 everywhere once ethereum_transactions.rs:12 is fixed). needs-care.

`tests/support/http.rs` (new; included with `#[path = "support/http.rs"] mod http;` by signed_http, signed_reads, signed_retention, arkiv_profile, simulator_http):

```rust
pub struct Server { pub url: String, task: JoinHandle<()> }      // Drop aborts
pub async fn serve(router: axum::Router) -> Server;
pub fn client() -> reqwest::Client;                               // with a timeout, see 5.2
pub async fn get_json(client: &reqwest::Client, url: &str) -> (reqwest::StatusCode, Value);
pub async fn post_json(client: &reqwest::Client, url: &str, body: &Value) -> (reqwest::StatusCode, Value);
pub async fn rpc(client: &reqwest::Client, url: &str, method: &str, params: Value) -> (u16, Value);
pub async fn until(what: &str, limit: Duration, condition: impl FnMut() -> bool);
pub struct TempPath(PathBuf);                                     // Drop removes file or dir
```

What does not fit and should stay local: signed_protocol.rs `genesis()`/`envelope()`/`signed()` (golden inputs), `SlowStore` and `node()` in
signed_reads.rs, `hold_node`/`light_rt`/`light_chain` in signed_http.rs, `relay`/`recertify` in signed_host.rs.
Request-id layouts differ per file (1.2); the only look-ups by literal id are signed_host.rs:345, :386, :404-406, :815 (`[n; 32]`) and
signed_pool.rs:314 (`[0; 32]`), so moving to one `request_id(tag, nonce)` touches those five lines. needs-care.

Estimated effect across the ten files once both files exist: about 510 lines leave the test files
(signer/genesis ~50, Server/serve ~50, host construction ~85, tx/create builders ~120, request chains and JSON-RPC wrappers ~165, the rest ~40)
and about 110 are added to the two support files: net about -400.

#### 1.4 Duplication inside a file (by payoff)

1. signed_protocol.rs:388-472 and :548-575 - 14 + 8 blocks of `let mut v = tx.clone(); <one mutation>; variants.push(v);` followed by a loop whose
   failure message does not say which variant failed (:471, :574). Fix: `fn rejected(tx: &SignedTx, g: &SignedGenesis, change: impl FnOnce(&mut SignedTx))`
   (and the header twin) called once per mutation, plus `fn first_create(tx: &mut SignedTx) -> (&mut u128, &mut u8, &mut Vec<(CellName, Cell)>)`
   for the five identical `if let TxOp::Arkiv { operation: ArkivOperation::Create { .. }, .. } = &mut v.tx.ops[0]` destructurings (:416-460).
   -55 lines, and each case fails on its own line. safe.
2. signed_http.rs - a `post(..)` followed by `.json::<Value>().await.unwrap()` in 12 places (:255, :296, :341, :384, :402, :463, :474, :481, :484, :1202, :1223, :1455;
   four more read the status first: :831, :1312, :1450, :1531) and `post(&client, <url>, "transactions", json!({"transactionBytes":pack(&x)}))` in 8
   (:247-253, :276-282, :289-295, :311-318, :334-340, :377-383, :395-401, :822-828).
   Fix: `post_json` + `async fn submit(client, url, tx: &SignedTx) -> (StatusCode, Value)`. -60 lines (counted in the 1.3 estimate). safe.
3. signed_http.rs:973-1003 == :1032-1062 (31 lines: body counter, mock `/signed/v1/headers` + counted `/signed/v1/replication/blocks/1`, `Runtime::new(full, Some(url), Some(url), None)`);
   :1095-1126 is the same shape with a two-phase headers handler. Fix: `async fn follower_of(headers: impl Fn() -> Value + .., body: Option<SignedBlock>, full: Host) -> (Shared, Server, Arc<AtomicUsize>)`. -50 lines. safe.
4. signed_host.rs - `chain` (:898-910) vs `create`/`memory` (:30-54); `tx_on` (:911-939) vs `tx` (:70-91); both inlined once more in the benchmark (:1299-1309, :1324-1343). -45 lines with the 1.3 helpers. safe.
5. signed_history.rs - `let store = Arc::new(FaultyStore::latching(MemoryVersionedStore::new())); let mut history = History::create(store.clone(), ..)` 7x
   (:267-268, :291-292, :320-321, :334-335, :347-348, :364-365, :643-645; the same store under a `Light` at :666); `Light::create(..)` after
   `signed_genesis()` + `certificate(..)` 10x; `&genesis, &signer` threaded through 31 `next`/`conflict`/`certificate` calls; `store.head().unwrap().unwrap().height` 11x.
   Fix: `struct Chain { genesis, signer, root }` with `next(&self, parent, tag)`, `conflict(&self, header)`, `light(&self, store)`; `fn faulty_history()`; `fn commits(store) -> u64`. -35 lines. safe.
6. signed_host.rs:256-270 == :1090-1104 (EqRequest at the head) and :271-275 == :1105-1109 (proof equal on the follower, `light.verify` -> `Page`). -25 lines. safe.
7. arkiv_profile.rs - `Host::create` + produce N + `Light::create` + append headers: :257-281 (15 blocks), :435-456 (3), :184-200 (2). Fix: `fn chain(blocks: u64, ids: ([u8; 16], [u8; 16])) -> (Host, Light)`. -25 lines. safe.
8. signed_http.rs:882-897 == :1287-1302 (16 identical lines: append certificates 1..=2 to a light whose source is a bad server). Fix: `fn light_at_2(source: &str, producer: &Shared) -> Shared`. -16. safe.
9. signed_reads.rs:396-404, :449-457, :475-483 - set the delay, spawn the heavy count, wait for its admission (9 lines x3; the wait alone again :604-607). Fix: `async fn start_heavy_count(client, node) -> JoinHandle<(u16, Value)>`. -16. safe.
10. signed_http.rs:857-862, :1150-1151 + :1169-1174, :1319-1326 redo by hand what `light_chain()` (:539-550) does (produce, submit, produce, `Runtime::new`, serve). Fix: `producer_chain(txs: impl IntoIterator<Item = SignedTx>) -> (Shared, Server)` and build `light_chain` on it. -15. safe.
11. signed_http.rs:944-960 `full_with_retained_history` vs `host(false)` :76-89. Fix: `host_on(history_store, producer)`. -12. safe.
12. signed_http.rs:1469-1479 + :1506-1507 hand-inline `hold_node` (:498-509, same file). Fix: `let held = hold_node(&rt); .. drop(held);`. -11. safe.
13. ethereum_transactions.rs - `make_host` twice (:179-190, :312-323, -12); the 9-key `select` literal twice (:228, :375); `quantity` closure (:439-440) then the same parse written out for `estimate` (:461-468, -7); the fixture file parsed by hand a second time (:454-455) beside `fixture()` (:17-21). safe.
14. signed_http.rs:916-918, :967-969, :1027-1029, :1089-1091 - `changed.inputs_digest = B256::repeat_byte(n); SignedHeader::sign(&genesis(), changed, &signer(1))`. Fix: `fn conflicting(header: Header, byte: u8) -> SignedHeader` (signed_history.rs:41-45 `conflict` is the same idea). -8. safe.
15. signed_reads.rs:580-587 == :617-624 ("the walk stopped": read count, 200 ms sleep, compare). -8. safe.
16. signed_host.rs:1209-1215 derives the entity key by hand with a literal `9100`; `entity_key(2, offset, u128::from(offset) + 1)` (:119-127) is the same. -6. safe.
17. signed_host.rs:142-148 `creation_nonce` and :940-946 `expiry_total`: same "host_get, big-endian u64 or 0" body. -5. safe.
18. signed_host.rs:997-1004 and :1127-1132: the "8 transactions of 16 creates" loop twice.
19. signed_host.rs + signed_http.rs: `History::open(history, genesis().hash())` is `ChainConflict` tail 6x (signed_host.rs:665-668, :693-696, :782-785; signed_http.rs:1015-1018, :1076-1079, :1141-1144): `fn assert_frozen(history_store)`.
20. signed_host.rs:705-710, :734-739, :794-800, :830-835, :861-866: the same six-line opening of five fault tests (stores, `create`, bootstrap block, one pending tx). Optional `producer_with_pending(state, history) -> (Host, SignedTx)`. -15.
21. signed_retention.rs:299-319 and :321-335: each request written twice for heights 3 and 4.

#### 1.5 `SlowStore` vs `arkiv_db_storage::testing::FaultyStore` (emphasis d)

signed_reads.rs:68-137 is 70 lines. :68-88 (21 lines) is the struct and its two methods (count namespace object reads, sleep `delay_us`);
:89-137 (49 lines) only forwards `read_object`, `read_object_bounded`, `database_id`, `head`, `read_record`, `read_record_bounded`, `commit`,
`stage_objects`, `health`, `recover`, `stats` to `inner`.
`FaultyStore` (crates/arkiv-db-storage/src/testing/faulty.rs:195-444) cannot replace it today: it has no delay and no per-space read
counter. Its `gate` (:291-314) only fails reads, and it runs under the state mutex, so a sleep would have to happen after the guard is dropped.
Two differences between the wrappers that I checked and that a shared one should settle:
- `FaultyStore`'s `impl VersionedStore` (:333-444) forwards neither `stage_objects` nor `relabel`, so the trait defaults
  (crates/arkiv-db-storage/src/versioned.rs:736-741, :747-752) answer `Incompatible`.
- `SlowStore` forwards `stage_objects` (:125-127) but not `relabel`. I found no caller of `stage_objects` on this file's path: rg shows it is
  called only from crates/arkiv-db/src/durable/bulk.rs:326 and durable/streaming.rs:92, while the fixture goes through
  `Host::create_with_prefill` -> `DurableEngine::initialize_simulator_prefilled` (crates/arkiv-db/src/durable/simulator.rs:312, one atomic commit).
  So :125-127 looks like forwarding that is not needed here (not confirmed by running anything).
Fix (needs-care, touches the shared testing module): give `arkiv_db_storage::testing` one forwarding wrapper with an object-read hook, e.g.
`Hooked<S, F: Fn(SpaceId) + Send + Sync>` that forwards every trait method including `stage_objects` and `relabel` (or add
`delay_reads(scope, Duration)` and a read counter to `FaultyStore`); `SlowStore` then is the two atomics and a 10-line closure.
-45 lines here, and the same wrapper serves the other hand-written test doubles (known workspace item).
For whoever owns faulty.rs: a `FaultyStore` placed under a bulk-built, streamed or adopted store refuses staging and relabel; that is a behaviour gap, not style.

### II.1 [D2] Scripts, CI, build config, vendor/reth-trie, conformance

*(repo-relative paths)*

#### D1. Node-launcher boilerplate re-implemented in 8 smoke scripts (2,074 lines; about 450-500 removable) - needs-care

Each smoke script carries its own copy of "free port, REST helper with 429 retry, wait-until, run CLI, spawn node with
log, stop all, wait followers, build producer/full/light argv, dump logs on failure".

| helper | copies (file:lines) |
| --- | --- |
| free port | signed-proposer-smoke.py:18-21, genesis-bundle-smoke.py:158-165, streaming-prefill-smoke.py:84-87, full-indexer-smoke.py:57-64, udc-sdk-smoke.py:47-54, sdk-compat-smoke.py:55-62, run-full-prefill.py:92-95 (+ dedupe loop 194-196), full-light-demo.py:64-67 |
| REST `request` | signed-proposer-smoke.py:24-44, genesis-bundle-smoke.py:172-194, streaming-prefill-smoke.py:137-158, full-indexer-smoke.py:66-72, udc-sdk-smoke.py:109-128, sdk-compat-smoke.py:104-123, create-sample-namespace.py:14-19 (+ retry wrapper seed-bricks.py:20-29) |
| `wait` | signed-proposer-smoke.py:47-57, genesis-bundle-smoke.py:196-206, streaming-prefill-smoke.py:160-173, full-indexer-smoke.py:74-85, udc-sdk-smoke.py:130-140, sdk-compat-smoke.py:125-135, full-light-demo.py:107-118, arkiv-v2-demo.py:50-62 |
| `cli` / `command` | signed-proposer-smoke.py:68-69, glm-accounts-smoke.py:25-26, range-proof-smoke.py:28-29, genesis-bundle-smoke.py:208-220, streaming-prefill-smoke.py:121-135, udc-sdk-smoke.py:99-107, sdk-compat-smoke.py:94-102 |
| `start` | signed-proposer-smoke.py:71-76, glm-accounts-smoke.py:28-31, range-proof-smoke.py:31-34, genesis-bundle-smoke.py:222-227, streaming-prefill-smoke.py:175-180, udc-sdk-smoke.py:142-147, sdk-compat-smoke.py:137-142, arkiv-v2-demo.py:44-49, full-light-demo.py:141-163 |
| `stop` | signed-proposer-smoke.py:78-83, glm-accounts-smoke.py:33-38, range-proof-smoke.py:117-120, genesis-bundle-smoke.py:229-238, streaming-prefill-smoke.py:182-191, udc-sdk-smoke.py:149-158, sdk-compat-smoke.py:144-153, arkiv-v2-demo.py:83-92, full-light-demo.py:171-180 |
| `sync` / `step` | glm-accounts-smoke.py:40-45, range-proof-smoke.py:36-41, genesis-bundle-smoke.py:240-249, streaming-prefill-smoke.py:193-197, udc-sdk-smoke.py:160-163, sdk-compat-smoke.py:155-158 |
| producer / pins / full / light argv | signed-proposer-smoke.py:88-100, glm-accounts-smoke.py:62-73, range-proof-smoke.py:54-64, genesis-bundle-smoke.py:362-414 (53 lines), streaming-prefill-smoke.py:212-214 + 229-236, udc-sdk-smoke.py:175-226 (52), sdk-compat-smoke.py:170-221 (52) |
| log dump on failure | signed-proposer-smoke.py:145-150, glm-accounts-smoke.py:136-141, range-proof-smoke.py:112-115, genesis-bundle-smoke.py:504-509, udc-sdk-smoke.py:265-268, sdk-compat-smoke.py:298-301, full-indexer-smoke.py:223-230 |

Single home: `scripts/lib/node.py` (importable name; a script run as `python3 scripts/x.py` has `scripts/` as
`sys.path[0]`, so `from lib.node import ...` works without packaging). full-light-demo.py already has the right shape
and is the model: `free_port` 64-67, `wait_until` 107-118 (keeps the last error for the timeout message), `Cluster`
131-202 (start/stop/restart/wait_height/close, context manager). Proposed surface:
`free_port(reserved)`, `Rest(base, token=None).request(path, body=None)`, `wait_until(predicate, timeout, processes=())`
(fails fast when a child exited, as streaming-prefill-smoke.py:163-165 does), and
`class Trio(binary, work, env)` with `cli(*args)`, `start(role, argv)`, `pins(status)`, `stop()`, `sync()`, `step()`,
`dump_logs()`. glm-accounts-smoke.py and range-proof-smoke.py already admit the need: they load
signed-proposer-smoke.py as a module just to reuse `request, wait, port`.

The copies **differ subtly**; decide these on purpose when merging:
- Retry policy of `request`: 20 x 25 ms and only if the 429 body says `ServerBusy` (signed-proposer-smoke.py:31-43);
  30 x 100 ms and *returns* `{"httpStatus", "body"}` instead of raising (genesis-bundle-smoke.py:181-194, callers test
  `"httpStatus" not in v` at :248); 40 x 100 ms and also retries a 200 body `{"error": "ServerBusy"}`, with an 8 MiB cap
  (streaming-prefill-smoke.py:143-158); 30 x 100 ms raising (udc-sdk-smoke.py:118-128, sdk-compat-smoke.py:113-123); no
  retry (full-indexer-smoke.py:66-72, create-sample-namespace.py:14-19); 10 x 2 s on 429/503 (seed-bricks.py:20-29).
- Timeouts of `wait`: 20 s (signed-proposer-smoke.py:48), 30 s (genesis-bundle-smoke.py:197, udc-sdk-smoke.py:131,
  sdk-compat-smoke.py:126), 90 s (full-indexer-smoke.py:74), `--timeout` default 600 s (streaming-prefill-smoke.py:53,161),
  150 x 0.1 s (arkiv-v2-demo.py:51). Swallowed exceptions differ too: `(OSError, URLError)` / `OSError` only
  (genesis-bundle-smoke.py:203) / `(OSError, ValueError, KeyError, TypeError)` (full-indexer-smoke.py:82).
- `stop` without a kill fallback in three copies: signed-proposer-smoke.py:78-83, glm-accounts-smoke.py:33-38,
  range-proof-smoke.py:117-120 call `process.wait(timeout=10)` bare. It runs inside `finally`, so a node that ignores
  SIGTERM for 10 s raises `TimeoutExpired`, replaces the real failure and leaves that process alive. The other six
  copies kill. **behaviour-affecting** (failure path only); the shared `stop()` fixes it.
- Naming of the same things: binary is `BINARY` / `B` (genesis-bundle-smoke.py:24) / `args.bin`
  (streaming-prefill-smoke.py:63) / `--binary`; temp dir is `directory` / `D` (genesis-bundle-smoke.py:144) / `work`.

#### D2. udc-sdk-smoke.py is a copy of sdk-compat-smoke.py - safe
274 vs 307 lines; `diff scripts/sdk-compat-smoke.py scripts/udc-sdk-smoke.py | grep -c '^[<>]'` = 77, i.e. about 245 lines
are byte-identical (port, command, request, wait, start, stop, sync, the three argv lists, the bun stage runs, the
except/finally). Even the temp-dir prefix `"arkiv-sdk-compat-"` was carried over (udc-sdk-smoke.py:42). After D1 the
UDC launcher is about 60 lines. If S2a is taken (delete the compat pair) this duplication disappears by deletion.

#### D3. The "UDC smoke chain" fixture is patched by string replace in three scripts - needs-care
udc-sdk-smoke.py:68-97, full-indexer-smoke.py:129-145 (its comment at :127 says "The chain of scripts/udc-sdk-smoke.py"),
sdk-compat-smoke.py:76-92. All three import genesis-bundle-smoke.py through importlib only to take its `FIXTURE` string,
then `.replace('block_period_ms = "5000"', ...)`, rewrite the three `expires_at_height` values, swap the profile preset
for a patched copy of `docs/examples/genesis-bundle/profiles/fixed-glm-v2.toml`, write `0x01..01` as the proposer key
and symlink `node_modules`. Single home: `scripts/lib/fixtures.py` with
`genesis_fixture(block_period_ms=5000, expiries=(4, 5, 6), storage_cap=None) -> str` and
`link_sdk(work, sdk_dir, smoke_ts)`; genesis-bundle-smoke.py imports the same module instead of being imported.
About 45 lines removed, and the fixture stops being edited by text search.

#### D4. sourcify-v4-prefill.py vs sourcify-prefill.py: about 110 lines copy-pasted - see S2b
Identical or near-identical (v4 lines / old lines): `Invalid` + `require` 185-195 / 84-90; `keccak` 198-202 / 93-98;
`unique_object` 214-219 / 113-118; canonical JSON 259-267 / 101-110; `sized` 270-271 / 341-342; `open_relative`
624-641 / 142-160; `read_small` 674-678 / 163-167; SHA256SUMS parsing 683-690 / 171-178; `check_disk` 753-754 / 398-399;
`sync_json`, `sync_dir`, `publish` 757-783 / 829-855 (27 lines byte-identical apart from the JSON dump); `RESERVED`
108-109 / 81; key derivation 205-207 / 392-394; keccak self-test and output checks 913-924 / 877-882; `__main__`
1057-1062 / 967-972. The two converters share no code otherwise (different input formats, SQLite vs in-memory). A shared
module is awkward because the v4 tool is documented to run with `python3 -I` (docs/sourcify-v4-prefill.md:11), which
drops the script directory from `sys.path`. The clean fix is S2b (retire the old converter); if both stay, accept this.

#### D5. Loading a sibling hyphen-named script: 12 copies of the importlib idiom, in two flavours - safe
glm-accounts-smoke.py:12-14 (`runpy.run_path`) and range-proof-smoke.py:12-16 (`importlib`) load the *same* file two
different ways; also full-indexer-smoke.py:47-49, udc-sdk-smoke.py:33-37, sdk-compat-smoke.py:41-45, seed-bricks.py:14-16,
test-sourcify-prefill.py:21-23, test-sourcify-v4-prefill.py:32-36, test-run-full-prefill.py:15-17,
test-full-prefill-bootstrap.py:3-4, test-full-light-demo.py:21-23, full-prefill-bootstrap.py:95. With D1/D3 the five
smoke copies go away; for the tests add `load_script(name)` to `scripts/lib`. range-proof-smoke.py:17 also re-derives
`BINARY` that it could take from the loaded module (signed-proposer-smoke.py:15).

#### D6. Inside sourcify-prefill.py (only if it is kept) - safe
`uniform_minimum` 764-766 is the expression at 744 (`assign_expiry` should call it); the capacity check 492-495 repeats
at 725-728; the synthetic staging key `sha256(b"arkiv-prefill.imported-staging.v1:" + sid.hex().encode())` is spelled at
326, 457 and 702; the creators-nonce upsert 460-462 repeats at 704-708; the inventory dict 496-500 repeats at 730-738;
`if n % 1024 == 0: db.commit(); check_disk(...); progress(...)` appears six times (475-478, 532-535, 676-679, 713-716,
720-723, 756-758). About 30 lines.

#### D7. Test helpers - safe
- test-sourcify-prefill.py: the SHA256SUMS writer expression is repeated 4 times (65, 96, 120, 225) -> `write_sums(root, names)`;
  `[json.loads(line) for line in (out / "keys.jsonl")...]` 5 times (239, 359, 547, 641, 652) -> `keys(out)`;
  `same = lambda path: [...]` 3 times (647, 699, 715); the forbid-by-monkeypatch block twice (530-537, 624-631) ->
  `unittest.mock.patch.object(p, "dependencies", forbidden)`.
- test-run-full-prefill.py: three hand-written fake `arkiv-proposer` scripts (100-108, 143-158, 232-244) and the same
  subprocess argv four times (27-31, 46-51, 58-62, 72-74); the fake converter source string twice (44, 71).
- test-sourcify-v4-prefill.py: `manifest_spaced` 584-588 repeats the tail of `recompile` 210-214 (re-hash the manifest
  and propagate `compilationhash`); `resum` 666-669 repeats `write_bundle` 268-270.
- The `entity_key` formula exists 4 times in Python: sourcify-v4-prefill.py:205-207, sourcify-prefill.py:392-394,
  sourcify-v4-node-check.py:35-36, test-sourcify-v4-prefill.py:48-50. Keep the test's copy (it is the independent
  oracle); the node-check copy is the removable one.

#### D8. bench - safe
scripts/bench/summarize.py:95-100 `read` duplicates `parse_run.read` (parse_run.py:24-29), which is already imported at
:18; `med` / `lo` / `hi` (40-52) repeat the same None filter. `SCENARIO_ORDER` (summarize.py:103-119) mirrors the two
scenario arrays in bench-compare.sh:183-201, and `ENGINE_ORDER` (summarize.py:20) mirrors `KNOWN_ENGINES`
(bench-compare.sh:113-115); a new scenario has to be added in both.

#### D9. Applying the conformance kit, and its pinned revisions - needs-care
`git apply harness.patch` + copy `src/*.rs` + copy `scenarios-scaled` is written three times: conformance/golem-db/ci.sh:53-55,
scripts/bench-compare.sh:435-437, conformance/golem-db/README.md:55-57. Give ci.sh an `apply <harness-dir>` subcommand
and call it from bench-compare.sh. The pinned harness/golem-db revisions live in .github/workflows/golem-conformance.yml:23-24,
scripts/bench-compare.sh:27 (`CONF_REV="686ddfc"`), conformance/golem-db/README.md:26-32, 51-52 and 192, and
docs/testing.md:100; the test count 21 lives in ci.sh:24, README.md:142 and 186, check-tests.py:7. A
`conformance/golem-db/revisions.env` sourced by ci.sh and bench-compare.sh and read by the workflow would be the one home.

#### D10. golem-conformance.yml: job `golem-latest` repeats `pinned` step for step - safe
.github/workflows/golem-conformance.yml:63-96 vs 29-61: the same seven steps; only the two refs and the cache key differ.
A local composite action (`.github/actions/conformance/action.yml`, inputs `harness-ref`, `golem-ref`, `cache-key`) leaves
each job at about 8 lines. About 30 lines; optional.

#### D11. conformance/golem-db/src - safe, small
- arkiv.rs: "lock the engine, heal if unhealthy" three times (924-927, 1200-1203, 1306-1309) -> one
  `fn backend(&self) -> Result<MutexGuard<'_, Backend>>`; the closure prologue
  `let slot = slot.as_mut().ok_or(ApiError::HandleInvalid)?;` under `with_slot(branch, || ApiError::HandleInvalid, ...)`
  at 1187-1197, 1335-1347, 1394-1401 and inside `with_open` 816-828 -> a `with_live` helper that `with_open` builds on;
  "drop the engine branch if poisoned" four times (840-845, 899-901, 992-994, 1279-1281); the `Durable { .. }` literal
  twice (420-426, 455-461). About 25 lines.
- golem_tests.rs: `BACKEND.with(|b| *b.borrow_mut() = "...")` 18 times
  (`rg -c 'BACKEND\.with\(\|b\| \*b\.borrow_mut\(\) = ' conformance/golem-db/src/golem_tests.rs`) -> `fn backend(name: &'static str)`;
  the "read of a sealed branch" match three times (330-333, 596-599, 1291-1299); the `#key` identity check three times
  (190-196, 539-545, 652-658). The cache defaults `8 << 20` / `16_384` are written in arkiv.rs:110-111 (with a comment
  saying they are `durable::Options::default()`) and again in golem_tests.rs:109-110; take them from
  `Options::default()` (crates/arkiv-db/src/durable/mod.rs:53-54) so they cannot drift.
- No duplication with crates/arkiv-node found: `rg 'recover_pending|discard_prepared|WriterState::' crates/arkiv-node/src`
  hits only signed/host.rs:1120 and simulator/host.rs:717, which are the node's own recovery paths, not copies of the adapter.

### II.1 [T5] arkiv-db tests: durable / faults / KV; examples

*(paths relative to `crates/arkiv-db/`)*

#### 1.1 Hand-written store test doubles (7 in this chunk, 443 lines)

| # | Double | Location | Lines | Wraps | What it adds over plain delegation | Silently NOT forwarded (trait default refuses with `Incompatible`) |
|---|---|---|---|---|---|---|
| 1 | `Corruptible` | tests/durable_failures.rs:375-456 | 82 | `MemoryVersionedStore` | Read overrides for records, objects and the head (`Some(bytes)` replaces, `None` hides); length check before copy on overridden bytes; unbounded `read_object` / `read_record` panic; `recover()` returns the (possibly overridden) head | `stage_objects`, `relabel` |
| 2 | `Hooked` | tests/durable_failures.rs:1238-1315 | 78 | `FaultyStore<Memory>` | Runs a callback before every object read (called outside any lock, the callback blocks); pushes `"store recover"` to an event log in `recover()`. The other 7 methods are pure forwarding | `stage_objects`, `relabel` |
| 3 | `Overlay` | tests/simulator_composition.rs:305-360 | 56 | `MemoryVersionedStore` | Record overrides only; `read_record` panics. Strict subset of #1: same `RecordOverrides` alias (305 vs durable_failures.rs:375), same lookup, same oversize -> `Corrupt` | `stage_objects`, `relabel` |
| 4 | `Faults` | tests/adopt.rs:336-403 | 68 | `Arc<Memory>` | Hides one object id on read (`Ok(None)`); one-shot injected `relabel` outcome, optionally after really applying it (`apply_then_report`) | none (forwards both) |
| 5 | `InterruptCopy` | tests/streaming_import.rs:286-347 | 62 | `Arc<Memory>` | Counts `stage_objects` calls and fails call number `fail_on` with `NotCommitted(Io)`. The other 10 methods are pure forwarding | `relabel` |
| 6 | `TestStore` | tests/storage_faults.rs:72-149 | 78 | own `HashMap` (`ObjectReader` only) | Read counter; "let N reads through, then fail every read with `kind`" (`fail_after` / `heal`); tests remove / replace single objects through the public `objects` mutex | n/a |
| 7 | `Store` | tests/rehash.rs:83-101 | 19 | own `HashMap` (`ObjectReader` only) | Nothing | n/a |

Same shapes outside my chunk (verified by reading them, for the design below only): `Store` in tests/eq_proofs.rs:818-836 and tests/range_proofs.rs:605-623 (identical to #7), `Reader` in src/engine.rs:3005-3035 (same allow-N-then-fail as #6), `Probe` in src/durable/adopt.rs:624-686 (hide / flip one object + read set, like #4), `SlowStore` in crates/arkiv-node/tests/signed_reads.rs:70-137 (sleep + count per object read, like #2), `BoundaryStore` in crates/arkiv-db-redb/tests/engine_process.rs:33-86.

**What `FaultyStore` (crates/arkiv-db-storage/src/testing/faulty.rs) provides today:** injected read *errors* by scope for the next N reads or until cleared, a commit-fault queue, disk full, failed recoveries, latching. It provides none of the seven behaviours above, for three concrete reasons:

- It never changes what a read returns (no content override / hide) -> #1, #3, #4.
- `ReadFault::remaining` means "fail the next N", there is no "let N through first" -> #6.
- **It does not forward `stage_objects` or `relabel` at all.** `impl VersionedStore for FaultyStore<S>` (faulty.rs:334-444) defines `database_id, head, read_record, read_record_bounded, commit, health, recover, stats` only, so a wrapped store answers both with the trait defaults at crates/arkiv-db-storage/src/versioned.rs:736-752 ("headless object staging unsupported" / "store relabelling unsupported"). That is why #4 and #5 had to be written by hand, and it contradicts the type's own doc (faulty.rs:159-160: "Without armed faults every call is forwarded unchanged, so a wrapped store passes the whole behavioural contract"). The contract suite does not notice: `rg -n "relabel|stage_objects" crates/arkiv-db-storage/src/testing/contract.rs` -> no hits.

**Proposed single home: `arkiv_db_storage::testing::FaultyStore`**, extended by about 110 lines (444 -> ~555), which removes the 443 lines above plus roughly 190 more outside this chunk:

| Extension | Size | Replaces |
|---|---|---|
| (A) Forward `stage_objects` and `relabel`; run `relabel` through the existing `CommitFault` machinery (factor the body of `commit`, faulty.rs:359-413, into `fn faulted(&self, apply: impl FnOnce() -> CommitOutcome) -> CommitOutcome`); `push_relabel_fault(CommitFault)`; `fail_stage_objects_at(call: u64, kind)` + `FaultCounters::stage_calls` | ~35 | #5 entirely; #4's `relabel` / `apply_then_report` (`NotCommitted(Io)` -> `CommitFault::NotCommitted(Io)`, `Indeterminate` unapplied -> `IndeterminateNotApplied(Io)`, `apply_then_report` + `Indeterminate` -> `IndeterminateApplied(Io)`) |
| (B) `override_record(table, key, Option<Vec<u8>>)`, `override_object(space, id, Option<Vec<u8>>)`, `override_head(Option<Head>)` (must apply to `head()` **and** `recover()`, see #1), `clear_overrides()`, `forbid_unbounded_reads(bool)`; overridden bytes go through the same "longer than `max_bytes` -> `Corrupt`" check | ~45 | #1, #3, #4's `hide` (`override_object(SpaceId::Catalog, root.0, None)`), src/durable/adopt.rs `Probe.target` |
| (C) `set_object_read_hook(Option<Arc<dyn Fn(SpaceId) + Send + Sync>>)` and `set_recover_hook(..)`; clone the `Arc` out of the state mutex before calling, as durable_failures.rs:1276-1279 does, because the hook blocks | ~20 | #2, `SlowStore` |
| (D) `fail_reads_after(scope, allowance: u64, kind)` (a `skip: u64` field on `ReadFault`, decremented in `gate`, faulty.rs:291-314) | ~10 | #6's `fail_after` / `heal` (`heal` = `clear_read_faults`), src/engine.rs `Reader` |
| (E) No new code: use `MemoryVersionedStore::stage_objects` as the object map. It is headless (`expected_head: None`), put-if-absent and does not verify content addresses (crates/arkiv-db-memory/src/versioned.rs:21-22; `CommitBatch::validate`, crates/arkiv-db-storage/src/versioned.rs:370-400, checks no hashes), and `stats().object_reads()` already counts reads | 0 | #7, #6's storage and counter, eq_proofs.rs / range_proofs.rs `Store` |

Risk: **needs-care**. (A) changes a shared test utility used by 9 other test files (`rg -l FaultyStore crates/`), though only by adding forwarding that the doc already promises. In #6 the read counter is only ever sampled while the store is healthy (storage_faults.rs:303-318, 343-345, 584-586), so counting only reads that reach the inner store is equivalent. In #1 the unbounded-read panics are an assertion about the facade ("a facade regression cannot accidentally exercise a post-copy size check", durable_failures.rs:378-380); keep it as `forbid_unbounded_reads(true)` in `corruptible()`.

If `FaultyStore` should stay fault-only, the fallback is a forwarding layer in the same module (a trait with every method defaulted to `self.inner().method(..)` including `stage_objects` / `relabel`, plus one blanket `impl VersionedStore`). Each double then overrides only its 1-4 real methods, which also fixes the last column of the first table structurally. That removes ~300 of the 443 lines instead of all of them.

#### 1.2 Locally defined generic helpers (for the shared `tests/support` design)

| Helper | Signature | Defined at | Notes |
|---|---|---|---|
| `ns` | `fn ns(ordinal: u64) -> NamespaceId` | storage_faults.rs:155-157, rehash.rs:318-320, fallible_api.rs:30-32, durable.rs:12-14, support/durable_history.rs:11-13 | identical bodies (fallible_api uses `expect`) |
| `namespace` | `fn namespace() -> NamespaceId` (ordinal 1) | durable_failures.rs:42-44, durable_retention.rs:26-28 | identical |
| `id` | `fn id(sequence: u64) -> SnapshotId` | durable_failures.rs:39-41, durable_retention.rs:23-25 | identical |
| `name` | `fn name(text: &str) -> CellName` | storage_faults.rs:159-161, rehash.rs:322-324, fallible_api.rs:34-36, adopt.rs:36-38, durable.rs:15-17, support/durable_history.rs:14-16, examples/records.rs:42-44 | identical |
| `key` | **three different signatures under one name**: `(bytes: &[u8]) -> RecordKey` storage_faults.rs:163-165, fallible_api.rs:38-40; `(n: u64) -> RecordKey` (big-endian) durable.rs:18-20, support/durable_history.rs:17-19; `(text: &str)` examples/records.rs:46-48 | | differ subtly |
| `genesis` | `fn genesis() -> EngineGenesis` | durable_failures.rs:36-38 (`[0x42; 32]`), durable_retention.rs:20-22 (`[0x52; 32]`), durable.rs:21-23 and support/durable_history.rs:20-22 (`[9; 32]`), simulator_composition.rs:34-36 (simulator deployment) | differ only in the deployment bytes, which no test depends on |
| `initialize` | **five signatures**: `(Arc<dyn VersionedStore>, EngineGenesis, Options)` durable_failures.rs:46-55; `(Arc<MemoryVersionedStore>, Retention)` durable_retention.rs:36-41; `(Arc<MemoryVersionedStore>, Options)` durable.rs:24-29; `(Arc<dyn VersionedStore>, Options)` support/durable_history.rs:23-28; `(Arc<dyn VersionedStore>)` simulator profile simulator_composition.rs:52-65 | | all are "unwrap `Initialization::Committed`" |
| `committed` | `fn committed(i: Initialization) -> DurableEngine` | adopt.rs:20-25, streaming_import.rs:9-14 | byte-identical; the same match is inlined in every `initialize` above and as 5 `let Initialization::Committed(..) = .. else { panic!("initialization") }` in simulator_composition.rs:54-63, 578-582, 698-702, 766-774, 844-848 |
| `put_candidate` / `candidate` | `fn (engine: &DurableEngine, value: &[u8]) -> SealedCandidate` | durable_failures.rs:63-88, durable_retention.rs:43-68 | **identical 26-line bodies under two names**; simulator_composition.rs:66-79 `candidate(engine)` is a third, different one |
| `value` | `fn value(snapshot: &Snapshot) -> Vec<u8>` | durable_failures.rs:90-97, durable_retention.rs:80-87 | identical |
| `commit` / `publish` | `commit(&mut DurableEngine, SealedCandidate) -> CommitInfo` durable_failures.rs:57-61; `publish(&mut DurableEngine, Branch)` durable.rs:30-36; `publish(&mut DurableEngine, n: u64)` durable_retention.rs:70-78 | | `publish` means two different things |
| `put` | `fn put(namespace: u64, raw: &[u8], value: &[u8]) -> WriteOp` | storage_faults.rs:179-185; closure fallible_api.rs:70-74 | identical |
| `record` | `(namespace: u64, record: &[u8], kind: &str, height: u64) -> WriteOp` storage_faults.rs:167-177; `(record: &[u8], height: u64, tag: &str)` fallible_api.rs:54-64 | | argument order differs |
| `traced` | `fn traced() -> Budget` | storage_faults.rs:236-238 | |
| `entry` | `fn entry(key: &[u8], value: &[u8]) -> Entry` | kv.rs:157-162, semantics.rs:132-137 | identical |
| `hash` (domain-separated keccak) | `fn hash(domain: &str, bytes: &[u8]) -> B256` | simulator_composition.rs:80-85, closure durable.rs:59-64 | identical; hand-built on purpose (format vectors), share it |
| `ALICE` / `BOB` | `OwnerId::new([0xa1; 20])` / `([0xb0; 20])` | storage_faults.rs:65-66, rehash.rs:59-60, fallible_api.rs:27-28, interrupt.rs:20, examples/records.rs:30 | |
| `OWNER` | `OwnerId::new([7; 20])` | durable_failures.rs:33, durable_retention.rs:18, durable.rs:11, support/durable_history.rs:10; spelled inline at simulator_composition.rs:70, 435 | |
| RNG | SplitMix64 `struct Rng(u64)` rehash.rs:349-364; xorshift32 `struct Generator(u32)` model.rs:71-89; inline xorshift64 support/durable_history.rs:182-184 | | rehash.rs's is a copy of crates/arkiv-db-trie/tests/support/mod.rs:22-36 except `below(u64) -> u64` vs `below(usize) -> usize`; storage_faults.rs:42-43 already includes that module by `#[path]` |

#### 1.3 Other duplicated blocks, by payoff

1. **tests/durable_failures.rs:36-97 vs tests/durable_retention.rs:20-87** - `genesis`, `id`, `namespace`, `put_candidate`/`candidate`, `value` are the same ~45 lines. Fix: one `tests/support/durable.rs` (included by `#[path]` like durable_history.rs) exporting them plus `committed`. ~45 lines. **safe**.
2. **The "create namespace `alpha`, then `Put`" literal** (10 + 11 lines each time): durable_failures.rs:66-86, 910-930, 959-978, 1323-1343; durable_retention.rs:46-66, 270-280; simulator_composition.rs:69-76, 437-445, 468-478; streaming_import.rs:163-171; durable.rs:139-146; storage_faults.rs:193-196, 683-686, 788-791 (rehash.rs:326-333 has a `create_namespace` helper the others lack). Fix: `fn create_namespace(branch: &mut Branch, owner: OwnerId, name: &str)` and `fn put(branch: &mut Branch, owner: OwnerId, ns: NamespaceId, key: &[u8], value: &[u8])` in the shared support module; each site becomes one line. ~110 lines over the chunk. **safe**.
3. **tests/durable.rs:11-29 vs tests/support/durable_history.rs:10-28** - `OWNER`, `ns`, `name`, `key`, `genesis`, `initialize` are repeated verbatim (only `initialize`'s store type differs) although durable.rs already declares `mod durable_history` at line 123. Fix: make them `pub` in the support file and `use durable_history::{..}`. 18 lines. **needs-care** (the file is also `#[path]`-included by crates/arkiv-db-redb/tests/engine_history.rs:5; an item unused there becomes a `dead_code` error under `-D warnings`).
4. **"nothing was written" assertions** in tests/simulator_composition.rs: `store.head() == None` + `records_written == 0` at 675-677, 697, 710-711, 743-744, 806-807, 835-836, and the 7-line `stats().spaces.values().all(|s| s.objects_written == 0)` at 678-684 and 745-751; same intent in streaming_import.rs:230-236 and 407-413 (`head == None` + no `engine/genesis` record). Fix: `fn assert_untouched(store: &dyn VersionedStore)`. ~30 lines. **safe**.
5. **Hand-encoded catalog leaf**: tests/durable_failures.rs:1098-1110 `catalog(handles)` and tests/storage_faults.rs:642-654 `catalog_value(descriptor_id, next_record, handles)` build the same bytes (id, `5u32`, `"alpha"`, owner, revision 0, next_record, four `root ‖ len`). Fix: keep the hand encoding (it pins the leaf layout) but have one `catalog_leaf(id, owner, next_record, handles) -> Vec<u8>` in support. 13 lines. **safe** `[CONSENSUS]` (do not swap in the production encoder).
6. **The four-fault matrix** `[NotCommitted(Full), IndeterminateNotApplied(Io), IndeterminateApplied(Io), AckLost]`: simulator_composition.rs:213-218 and 716-721, split 2+2 in durable_failures.rs:185-188 / 235-238 and 321-324 / 347-350; and `matches!(fault, CommitFault::IndeterminateApplied(_) | CommitFault::AckLost)` at simulator_composition.rs:229-232 and 732-735 (also crates/arkiv-node/tests/simulator.rs:257). Fix: `impl CommitFault { pub const fn applies(self) -> bool }` in faulty.rs, and a `const COMMIT_FAULTS: [CommitFault; 4]` in support. ~15 lines. **safe**.
7. **Fixtures repeated across files**: `NamespaceDescriptor { id: 1, name: "entities", owner: [1; 20], revision: INITIAL }` at adopt.rs:27-34 and streaming_import.rs:355-360 (and src/durable/adopt.rs:704-709); `SimulatorIdentity { source_id: [1; 16], run_id: [2; 16], node_genesis_hash: 3.., .. }` at simulator_composition.rs:26-33 and adopt.rs:75-80; the bulk-staged fixture `staged()` at adopt.rs:41-71 has a sibling of the same shape in the unit tests at src/durable/adopt.rs:697-742. **safe**, ~20 lines.
8. **tests/rehash.rs:349-364 `Rng`** - copy of crates/arkiv-db-trie/tests/support/mod.rs:22-36. No number in the mixed-workload test depends on the stream (only bounds are asserted), so including that module (as storage_faults.rs:42-43 does) is behaviour-neutral. 16 lines. **safe**.
9. **tests/fallible_api.rs:179-272 `compare_branch_reads` vs 276-382 `compare_snapshot_reads`** - 200 lines of `assert_eq!(ok(x.try_f(args)), x.f(args))` with every argument list written twice. Moot if the file goes (see 2.1); if it stays, a 6-line `macro_rules! same` taking the receiver, the method pair and the args once removes about 80 lines. **needs-care**.

### II.1 [T1] arkiv-db tests: metering family

*(paths relative to `crates/arkiv-db/tests/`; `qm` = `query_metering.rs`)*

#### 1.1 Helper inventory (for the `tests/support` design)

The only existing shared module is `tests/support/durable_history.rs`, included as `#[path = "support/durable_history.rs"] mod durable_history;` (durable.rs:123). Same mechanism would work for a `support/metering.rs`.

**Identical in metering.rs and qm (one copy is pure waste, ~75 lines):**

| helper | metering.rs | qm | note |
|---|---|---|---|
| `const MODEL_VERSION: u32 = 3`, `FIXTURE_ID: u32 = 1`, `OUT_OF_BUDGET`, `COST_OVERFLOW` | 31-37 | 65-71 | also trace_regression.rs:59-62 (first two). 14 files in arkiv-db/tests define `MODEL_VERSION`/`FIXTURE_ID` (`rg -n '^const (MODEL_VERSION\|FIXTURE_ID)' crates/arkiv-db/tests`) |
| `fn receipt(kind: ReceiptKind, schedule_id: u32, spent: u64, outcome: Outcome) -> Receipt` | 77-86 | 264-273 | byte-identical |
| `fn write_receipt(spent: u64, outcome: Outcome) -> Receipt` / `fn read_receipt(..)` | 88-94 | 275-281 | byte-identical. trace_regression.rs:259-268 has a third `read_receipt` with the struct written out inline |
| `fn without_ledger(receipt: &Receipt) -> Receipt` | 118-123 | 290-295 | byte-identical |
| `type Canonical = Result<(Effect, Receipt), (Rejection, Receipt)>` + `fn canonical(result: &WriteResult) -> Canonical` | 125-134 | 312-321 | byte-identical incl. doc comment; also records.rs:256 |
| `fn fresh_branch(engine: &Engine, block_budget: Option<u64>) -> Branch` | 168-172 | 707-711 | byte-identical. records.rs:276 and terms.rs:215 have a one-arg `fresh_branch(engine: &Engine)` |

**Same job, subtly different (must be reconciled before sharing):**

| helper | metering.rs | qm | difference |
|---|---|---|---|
| ledger-from-tuples | `fn ledger(lines: &[(OperationClass, u64, u64, u64)]) -> Ledger` 97-109 | `fn lines(lines: &[(OperationClass, u64, u64, u64)]) -> Ledger` 298-310 | same body, different name. Both 4-tuples `(class, count, unit_price, subtotal)` |
| `traced` | `fn traced(receipt: Receipt, lines: &[(OperationClass, u64, u64, u64)]) -> Receipt` 111-116 | `fn traced(receipt: Receipt, ledger: Ledger) -> Receipt` 283-288 | qm takes a built `Ledger` because it also passes `ledger_after(..)`; every literal call site therefore writes `traced(r, lines(&[..]))`. Shared form: keep qm's signature + `lines` |
| `ns_id` | `NamespaceId::from_bytes(ordinal.to_be_bytes())` 43-45 | `NamespaceId::from_ordinal(ordinal)` 631-633 | two constructors for the same value. cell_limits.rs:28 has zero-arg `fn ns() -> NamespaceId`; eq_proofs.rs:19 `fn ns(n: u64)` |
| every-budget write sweep | `fn assert_write_vector(engine: &Engine, ns: NamespaceId, name: &str, op: &WriteOp, prefix_sums: &[u64], effect: Effect)` 224-298 | `fn assert_write_vector(engine: &Engine, ns: NamespaceId, vector: &WriteVector<'_>) -> Branch` 808-900 | see 1.2 |
| every-budget read sweep | `fn assert_read_vector<T: Debug + PartialEq>(name: &str, prefix_sums: &[u64], expected: &T, read: impl Fn(Budget) -> ReadResult<T>)` 302-339 | `fn assert_read_vector<T: Debug + PartialEq>(name: &str, groups: &[Group], total: u64, expected: &T, read: impl Fn(Budget) -> ReadResult<T>)` 905-964 | see 1.2. Third variant: trace_regression.rs:283-347 `fn assert_every_budget<T: Debug + PartialEq>(what: &str, points: &[Point], success: &Success<T>, read: impl Fn(Budget) -> ReadResult<T>)` (adds per-point `PhysicalTrace`, no ledger) |
| `Observed` / `observe(branch: &Branch, ns: NamespaceId) -> Observed` | 175-194 (namespace count, descriptor, raw scan) | 741-764 (typed rows of `OBSERVED_KEYS`, record/term counts, index consistency) | domain-specific; not shareable as is |
| `fixture()` | `-> (Engine, NamespaceId)` 138-166 | `-> (Engine, NamespaceId, Model)` 703-705 (`fixture_with(limits)` 665-701, `bare_with` 637-656, `bare` 658-660) | trace_regression.rs:160-203 `-> (Snapshot, NamespaceId)`; query_planner.rs:236-323 `-> Fixture { engine, ns, live }` |

**Other generic-looking helpers, by file:**

- metering.rs: `put(namespace: NamespaceId, key: &[u8], value: &[u8]) -> WriteOp` 47-53; `delete(namespace: NamespaceId, key: &[u8]) -> WriteOp` 55-60; `transfer(namespace: NamespaceId, expected_revision: u64, new_owner: OwnerId) -> WriteOp` 62-68 (same three signatures at namespaces.rs:124/132/139); `entry(key: &[u8], value: &[u8]) -> Entry` 70-75; `fixture_descriptor(ns)` 196-203; `fixture_records()` 205-207; `largest_prefix_sum_at_most(prefix_sums: &[u64], budget: u64) -> u64` 211-218 (byte-identical bodies at records.rs:517-524 and terms.rs:541-548); `struct Generator(u32)` 1668-1682 (`next`, `bytes(max_len)`); `type BlockRecord` + `run_script(engine: &mut Engine)` 1684-1757.
- qm: `const fn price(class: OperationClass) -> u64` 78-112; `type Group = Vec<(OperationClass, u64)>` 116 and 14 group constructors 118-206; `group_cost` 208-213; `prefix_sums(groups: &[Group]) -> Vec<u64>` 217-225; `groups_fitting(groups: &[Group], budget: u64) -> usize` 229-234; `ledger_after(groups: &[Group], applied: usize) -> Ledger` 238-258; typed builders 327-414: `name(text: &str) -> CellName`, `key(text: &str) -> RecordKey`, `id(ordinal: u64) -> RecordId`, `attr(Value) -> Cell`, `field(Value) -> Cell`, `cell(text: &str, cell: Cell) -> (CellName, Cell)`, `create(ns, key_text: &str, cells) -> WriteOp`, `patch(ns, key_text, set, remove: &[&str]) -> WriteOp`, `delete(ns, key_text) -> WriteOp`, `eq/not_eq(text, value) -> Term`, `range(text, lower, upper) -> Term`, `starts_with(text, prefix) -> Term`, `exists(text, tag) -> Term`, `named(key: bool, names: &[&str]) -> Projection`; `Model` 421-552; `term_holds` 556-574, `ordered` 578-584, `in_range` 586-609; `summarize(page: Metered<QueryPage>) -> Metered<PageSummary>` 614-621; `ids_of(rows: &[Row]) -> Vec<u64>` 623-625; `prepared(engine: &Engine, setup: &[WriteOp]) -> (Branch, u64)` 716-726; `modelled(model: &Model) -> Observed` 767-777; `assert_query_vector` 980-1010; `assert_query_on_both` 2396-2423.
- trace_regression.rs: `name` 205-207 and `id` 209-211 (identical to qm 327-329 / 335-337); `eq_c(value: &str) -> Literal` 213-215; `const fn nodes(d, r, i) -> PhysicalTrace` 222-228; `Point`/`point`/`Success<T>` 231-257; `within(smaller, larger) -> bool` 271-275; `check_success` 349-358; `type Page = (Vec<(u64, Option<Vec<u8>>)>, bool)` + `page(result: ReadResult<QueryPage>) -> ReadResult<Page>` 361-382 (same purpose as qm `summarize`/`PageSummary = (Vec<Row>, Option<RecordId>)`, different shape); `first_page` 385-392; `rows(ids: &[u64], keys: bool) -> Page` 395-404.
- query_planner.rs: `name` 47-49; `key(ordinal: u64) -> RecordKey` 51-53 (**takes a number**, qm's takes `&str`, eq_proofs.rs:28 encodes `n.to_be_bytes()`); `s(text) -> Value` 55-57; `tag_of`/`above`/`holds`/`matches` 65-129; `struct Generator(u64)` 135-154 (`next`, `below`, `pick`); `eq(text: &str, value: Value) -> Literal` 329-331 (**returns `Literal`**, qm's `eq` returns `Term`); `reversed(query) -> Query` 440-448; `page(snapshot, ns, query, limit, cursor, budget) -> QueryPage` 454-466; `walk(snapshot, ns, query, limits: &[usize], budgets: &[Budget]) -> Vec<(Vec<u64>, Option<u64>)>` 469-496; `count(snapshot, ns, query, budget) -> (u64, u64)` 498-503; `flat` 505-510; `spent(snapshot, ns, query, limit, budget) -> u64` 584-591.
- cell_limits.rs: `ns()` 28-30; `name` 32-34 (`unwrap`, others `expect`); `engine(limits: Limits) -> (Engine, Branch)` 36-49; `create(branch: &mut Branch, key: &[u8], cells) -> Result<(), Rejection>` 51-68 (**executes**, qm's `create` only builds a `WriteOp`); `kind(error: DurableError) -> StorageErrorKind` 70-75 (only copy: `rg -n 'fn kind\(' crates` finds no other); `invalid(fault) -> Result<(), Rejection>` 77-79.
- bounds.rs: `create_namespace(branch: &mut Branch, name: &str) -> NamespaceId` 9-23 (other signatures: query_isolation.rs:2700 and pagination.rs:622 take `OwnerId`, host_state.rs:105 returns `()`); `assert_admission(error: Rejected, reason: Rejection)` 25-29 (cousins: pagination.rs:436 `assert_zero_cost`, records.rs:245 `admission`).
- scalar_profile.rs: `values() -> Vec<Value>` 25-51.
- No hand-written `impl VersionedStore` doubles in this chunk.

`name()` has 23 top-level copies in arkiv-db/tests, `id()` 6 (`rg -c '^fn name\(' crates/arkiv-db/tests`).

#### 1.2 Findings, biggest first

1. **qm:968-978 + 791-802 - `QueryVector`/`WriteVector` literals repeat defaults ~145 times.** Counts in qm: `after: None,` 34, `next: None,` 34, `limit: 10,` 28, `setup: &[],` 23, `actor: ALICE,` 27. `assert_query_on_both` even asserts at runtime that `after` is `None` (2397-2400), i.e. the field must be `None` for 35 of 37 literals. Fix: `QueryVector::new(name, query, projection, groups, total, ids)` defaulting `limit: 10, after: None, next: None` with `.limit(n)`, `.next(id)`, `.after(id)`; `WriteVector::by_alice(name, op, groups, total, expect)` with `.setup(..)`, `.actor(..)`. ~140 lines removable, no vector changes. **needs-care** (37 + 28 call sites).

2. **metering.rs:209-339 - the raw-op sweepers are a weaker copy of qm:783-964.** metering's versions take bare `prefix_sums`, run untraced only and stop at `total`; qm's take `Group`s, also run traced (checking the ledger via `ledger_after`), run `total + 1`, and check the recorded receipt. qm's `price()` table already carries the raw classes (qm:81-90 `NamespaceCreate`..`ScanEntry`, 109-110 `ProofNode`/`ProofByte`) but no `Group` in qm ever uses them (`rg -n 'OperationClass::(NamespaceCreate|PutRecord|WriteKeyByte|ScanSeek)' crates/arkiv-db/tests/query_metering.rs` hits only the match arms). Fix: move `price`, `Group`, `prefix_sums`, `groups_fitting`, `ledger_after` and the two sweepers to `tests/support/metering.rs`, parametrise the state probe (`observe: impl Fn(&Branch) -> O`), and express metering.rs's vectors as groups (`[lookup(), vec![(PutRecord,1),(WriteKeyByte,3),(WriteValueByte,4),(RootUpdate,2)]]`). Removes ~125 lines from metering.rs (211-339) and gives the raw ops the traced-ledger leg for free. **needs-care** `[CONSENSUS]` (harness only; every literal total stays).

3. **Reference evaluator copied five times.** qm:556-609 (`term_holds`, `ordered`, `in_range`, 54 lines) and query_planner.rs:63-129 (`tag_of`, `above`, `holds`, `matches`, 67 lines) re-implement what queries.rs:285-335, pagination.rs:560-600 and query_isolation.rs:546-600 also have. The copies disagree on corner cases nobody tests: qm `in_range` takes the type from one bound only and returns `false` for `(Unbounded, Unbounded)` (qm:590-594); query_planner `holds` hits `unreachable!("ranges have a bound")` for the same input (query_planner.rs:111); pagination.rs `within` checks each bound's type separately; queries.rs matches on the variant. Home: `tests/support/model.rs` with one `term_holds(term: &Term, cells: &BTreeMap<CellName, Cell>) -> bool`. ~120 lines removable from this chunk. **needs-care**.

4. **metering.rs - `WriteOp::CreateNamespace { name: arkiv_db::NamespaceName::new("test").unwrap() }` written out 17 times** (146-148, 431-433, 887-889, 910-912, 1025-1027, 1035-1037, 1062-1064, 1371-1373, 1487-1489, 1496-1498, 1511-1513, 1522-1524, 1597-1599, 1697-1699, 1704-1706, 1711-1713, 1796-1798); 3 more in qm (645-647, 674-676, 3965-3967). The file has `put`/`delete`/`transfer` builders but none for this. Fix: `fn create_namespace() -> WriteOp`. Each 8-9 line `.execute(ALICE, WriteOp::CreateNamespace {..}, Budget::..)` block reflows to 1-3 lines: ~55 lines. **safe**.

5. **qm - "run this query, give me the ids" block written out 4 times**: 1275-1285, 1679-1689, 1692-1702, 1843-1853 (10 lines each: `branch.query(ns, &query, &Projection::ID_ONLY, 10, None, Budget::unlimited()).expect("applies")` then `ids_of(&page.value.rows)`). Fix: `fn matching(branch: &Branch, ns: NamespaceId, query: &Query) -> Vec<u64>`. ~36 lines. **safe**.

6. **query_planner.rs:896-916 re-implements `walk` + `flat`** (defined at 469-510 in the same file): the hand-rolled cursor loop collecting ids is `flat(&walk(&snapshot, ns, query, &[200], &[widened]))`. -19 lines. query_planner.rs:814-819 re-implements `spent()` (584-591): `let full = spent(&snapshot, ns, &query, 200, PLANNED);`. -5 lines. **safe**.

7. **scalar_profile.rs:182-228 - `verify_eq(.., None, .., ProofLimits::default(), Limits::DEFAULT)` spelled out 4 times** (182-188, 196-202, 207-213, 220-226), 7 lines each after rustfmt. Fix: `let verify = |request: &EqRequest, proof: &[u8]| verify_eq(request, None, proof, ProofLimits::default(), Limits::DEFAULT);`. Also hoist the `SnapshotRef` (164-169) out of the loop. -25 lines and the fn drops under clippy's 100-line limit (flagged at 121). **safe**.

8. **xorshift `Generator` twice in this chunk** (metering.rs:1667-1682 `u32`; query_planner.rs:135-154 `u64`), six times in arkiv-db/tests (model.rs:71, history.rs:1054, query_history.rs:2214, queries.rs:790). metering.rs's own comment says "as in `model.rs`". Home: support. ~30 lines here. **safe**.

9. **Estimate-vs-execute script driver**: metering.rs:1439-1447 and qm:4204-4215 are the same loop over the same tuple type `Vec<(&str, OwnerId, WriteOp, Budget, Canonical)>` (metering.rs:1313, qm:4039). Fix: `fn assert_estimate_matches_execute(branch: &mut Branch, script: Vec<..>)` in support. ~10 lines. **safe**.

10. **"commit `uniform(7, u64::MAX)` in a second block" setup**: metering.rs:1129-1138 and qm:4327-4336 are the same 10 lines (and 1199-1210 a third variant). Fix: `fn publish_pricing(engine: &mut Engine, schedule: PriceSchedule) -> SnapshotId`. ~20 lines. **safe**.

11. **qm:825-838 vs 918-929** - the budget enumeration (`(0..=total + 1).map(Some).chain([None]).flat_map(..)`), the label, the `Budget` construction and `applied`/`spent` are duplicated between the write and the read sweeper. Fix: `fn budget_cases(total: u64) -> impl Iterator<Item = BudgetCase>`. ~12 lines. **safe**.

12. **query_planner.rs:65-73 `tag_of`** re-implements `Value::tag()` (`src/record.rs:607`, already used at qm:479/565). Delete and call `.tag()`. -9 lines. **safe**.

13. **`"é".repeat(arkiv_db::Limits::DEFAULT_INDEXED_LEN / 2)`** six times: bounds.rs:203, 224, 330 and scalar_profile.rs:144, 279, 285. Bind once per test (`let at_cap = ..`). **safe**.

### II.1 [N3] arkiv-node binaries, genesis_*, query_language

*(paths relative to `crates/arkiv-node/src/`)*

1. **Crash-safe file helpers, re-implemented in five library files and the proposer.** (~90 lines)
   - `write_new(path, bytes)` (create_new, write_all, sync_all), byte-identical bodies:
     genesis_bulk.rs:48-56, genesis_prebuilt.rs:120-128, genesis_streaming.rs:586-594.
     (genesis_authoring.rs:1037-1041 is the dirfd/`O_NOFOLLOW` variant: keep.)
   - `sync_dir(path)`, identical: genesis_bulk.rs:57-61, genesis_prebuilt.rs:129-133,
     genesis_adopt.rs:83-87; written inline again at genesis_streaming.rs:560, 569, 583,
     genesis_prebuilt.rs:355, bin/arkiv-proposer.rs:109-113, 251-253, 503-505.
   - `read_bounded(path, cap)`: genesis_bulk.rs:36-47, genesis_prebuilt.rs:111-119, inline
     genesis_streaming.rs:470-475, bin/arkiv-proposer.rs:46-57 (`read_file`), bin/arkiv-demo.rs:19-28.
   - `parent(path)` ("parent, or `.`"): genesis_prebuilt.rs:134-138, genesis_adopt.rs:88-92, inline
     genesis_authoring.rs:835-838 and 1119-1122, genesis_streaming.rs:494-497.
   - `rename_noreplace`: genesis_prebuilt.rs:148-156, inline genesis_streaming.rs:561-568.
   - "publish a synced temporary under its final name" (hard_link, remove temporary, fsync dir):
     bin/arkiv-proposer.rs:107-113 (`write_new`), 246-253 (`local_prefill`), 501-505 (`download_prefill`).
   - Fix: one `pub mod fsutil` (or `genesis::fs`) returning `io::Result`:
     `read_bounded(&Path, u64)`, `write_new(&Path, &[u8])`, `sync_dir(&Path)`, `parent_or_cwd(&Path) -> &Path`,
     `rename_noreplace(&Path, &Path)`, and **`link_into_place(temporary: &Path, target: &Path)`** for the
     three proposer sequences. Callers keep their own `map_err`. **safe** for the library helpers;
     **needs-care** for `local_prefill`, where `check_path_identity` runs between the link and the unlink
     (bin/arkiv-proposer.rs:246-250): call it after the helper, or split the helper.
   - Same family, five temp-name schemes: random hex `.pending` (proposer:83-91), `temporary_name`
     pid+counter (genesis_authoring.rs:1030-1036), `.{name}.publish-{pid}` (genesis_streaming.rs:536),
     `.state-probe-{pid}` (genesis_prebuilt.rs:229-230), `.adopt-probe-{pid}` (genesis_adopt.rs:98-99).

2. **Hand-rolled argument parsing, four copies in three binaries.** (~45 lines net)
   - bin/arkiv-proposer.rs:31-45 `arguments()`; :508-596 `genesis_command()` re-reads
     `std::env::args().skip(2)` and has two more flag loops (:519-535, :579-587);
     bin/arkiv-simulator.rs:15-30 `args()`; bin/arkiv-demo.rs:42-65 (inline, whitelist inline at :44-55).
   - Whitelists: proposer `FLAGS` :622-659, simulator `FLAGS` :50-69, demo :44-55.
   - Accessors: proposer closures `required` / `get` :670-675; simulator closure `get` :79 plus 7 x
     `args.get(..).ok_or(Error("InvalidRequest"))?` (:80, 122-130, 218-226); demo `option()` :29-31.
   - Fix: `arkiv_node::cli::Args { command, positional, flags }` with
     `from_env(switches: &[&str])` (simulator's valueless `--unsigned-simulator`/`--resume`),
     `allow(&[&str])` (names the unknown flag), `required(key)`, `get(key)`, `or(key, default)`, `has(key)`.
     No clap needed. **needs-care** (error text for bad flags improves; exit codes unchanged).

3. **`X` / `X_with(.., permanent)` pairs whose short form only tests call.** (~55 lines of wrappers)
   - genesis_bundle.rs: `encode_stream` :221-223 -> `encode_stream_with` :226; `decode_stream` :241-243 ->
     `decode_stream_with` :245; `build_genesis` :584-591 -> `build_genesis_with` :594.
   - genesis_streaming.rs: `StreamReader::new` :111-113 -> `with_policy` :116; `audit_stream` :235-237 ->
     `audit_stream_with` :239; `build_bundle` :439-447 -> `build_bundle_with_state` :456;
     `#[cfg(test)] bulk_build` :336-359 -> `bulk_build_with` :360-383 (itself a one-call wrapper of
     `genesis_bulk::build_file`).
   - genesis_prebuilt.rs: `install` :288-290 -> `install_with` :292; `#[cfg(test)] install_by_copy` :361-368.
   - Verified with `rg -c 'encode_stream\(|decode_stream\(|build_genesis\(|StreamReader::new\(|audit_stream\(|build_bundle\(' crates conformance scripts`:
     every non-`_with` hit is a definition, a `#[cfg(test)]` module (genesis_streaming.rs, genesis_prebuilt.rs,
     genesis_authoring.rs:1604, signed/rpc_reads.rs:1939,1958 after its `#[cfg(test)]` at :1788) or tests/.
     Production uses only the `_with` forms (genesis_authoring.rs:1007-1009, genesis_bulk.rs:208,
     genesis_streaming.rs:253, 493, 504, 553, bin/arkiv-proposer.rs:537).
   - Fix: one function each; pass the policy explicitly (a two-variant `enum RowPolicy { Finite, Permanent }`
     reads better than `false`/`true` at ~60 test call sites). Move `bulk_build` into `mod tests` and call
     `genesis_bulk::build_file` directly from `build_bundle_with_state`. **needs-care** (test churn only).

4. **Prefill row rules written twice.** `[CONSENSUS]`
   - genesis_bundle.rs:186-217 (`canonical_entities`) and genesis_streaming.rs:211-234 (`validate_row`)
     both check: attribute count, name order/duplicates, `$` system names, value size, `bytes` never
     indexed, entity byte cap, `flags & !3`, expiry 0, permanent expiry only under the permanent policy.
   - Fix: `PrefillEntity::validate(&self, permanent: bool) -> Result<()>` beside the type; the small path
     calls it after sorting, the reader after decoding. The streaming path collapses all failures into
     `"invalid streaming prefill row"`; keep that mapping. **needs-care**: acceptance must stay bit-identical
     (the small path sorts attributes then rejects equal neighbours, the reader requires strictly ascending).

5. **Launch assembly, launch report and "is streaming" preamble, each two or three times.**
   - `PrefillLaunch { .. recipe_hash: B256::ZERO .. }` then `p.recipe_hash = recipe_hash(&base, &p)`:
     genesis_bundle.rs:607-620 and genesis_streaming.rs:515-527 (and again by hand in tests:
     genesis_streaming.rs:703-711, 740-749, tests/genesis_prefill.rs:603-614). Fix: `PrefillLaunch::commit(..)`.
     **safe**, `[CONSENSUS]` (same bytes).
   - Report JSON: genesis_bundle.rs:659 and genesis_streaming.rs:597 emit the same nine keys
     (`genesisHash stateRoot recipeHash streamHash rowCount canonicalBytes storageUnits burnedWei sponsor`);
     one uses `wire::hex`, the other `B256::to_string()` and a hand-rolled
     `p.sponsor.iter().map(|b| format!("{b:02x}"))`. Fix: `PrefillLaunch::summary_json(&self, g)` used by both.
     **needs-care** `[CONSENSUS]`: report.json and the lock's `resolved` table are compared in `verify`.
   - `g.prefill.as_ref().filter(|p| p.is_streaming()).ok_or("expected streaming prefill v2")`:
     genesis_streaming.rs:279-283, 409-413; genesis_prebuilt.rs:82-86. Fix: `fn streaming_launch(g) -> Result<&PrefillLaunch>`. **safe**.

6. **bin/arkiv-proposer.rs internal repeats.** (~80 lines)
   - Decode `genesis.bin`: :201-207, 554-560, 686-687, 898-902, 968-972. Fix `fn read_genesis(&Path) -> Result<SignedGenesis>`.
   - `retain_public` (:116-129) re-implemented inline at :1060-1070 (11 lines -> 2 calls). **needs-care**:
     read cap 65536 vs `MAX_CANONICAL_BYTES`, so an oversized foreign file reports `IdentityMismatch` instead of `InvalidRequest`.
   - `read_file` takes `&str`, forcing `.to_str().ok_or(Error("InvalidRequest"))?` at :119, 202-204, 265-268,
     555-557, 899, 969, 1067, 1092. Fix: take `&Path`. **safe**.
   - `unhex(x, N)?.try_into().map_err(|_| Error(..))?`: :176-178, 536+542, 697-699, 703-705, 712-714, 1074-1076.
     Fix: `fn fixed<const N: usize>(text: &str) -> Result<[u8; N]>` next to `wire::hash` (simulator/wire.rs:36).
   - History open-or-create: :1019-1023 and :1160-1164. Fix `fn history(store, hash)`.
   - Identity checks of a producer start: :913-932 (resume) and :977-994 (fresh) test chain id, block period,
     allocations and bundle equality with the same expressions. Fix `fn check_producer_flags(args, &genesis, bundle)`.
   - `bundle.as_ref().is_some_and(|(g, _)| g != &genesis)` :930, 977, 1104;
     `local_prefill(&path, &genesis, bundle.as_ref().map(|(_, b)| b.as_path()))` :942, 1013, 1154.
   - `reqwest::Client::builder().redirect(none).timeout(5s).build()`: :767-771, :1084-1088,
     bin/arkiv-simulator.rs:319-323, and outside the chunk signed/runtime.rs:131-135, simulator/runtime.rs:57-61.
     Fix: one `pub fn http_client()` (demo/wire.rs:264 already has the shape).
   - Bearer-token length check `is_none_or(|s| s.len() < 32)`: proposer :813, 1125; simulator :264, 273.

7. **bin/arkiv-simulator.rs internal repeats.** `Identity { source_id, run_id, genesis_hash, chain_id }`
   built twice with the same 12 lines (:121-132, :217-228); follower setup
   `h.role = "full"; h.paused = false; h.health = "following";` twice (:237-239, :251-253). **safe**, ~15 lines.

8. **Work-directory layout as scattered literals.** `"bulk-v1"`, `"complete"`, `"pending"`, `"IDENTITY"`,
   `"state.redb"`, `"genesis.bin"`, `"complete.json"` are literals in genesis_bulk.rs:144, 155, 162, 170, 171,
   333, 351, 353; constants in genesis_prebuilt.rs:40-44 (whose comment at :38-39 says they mirror
   `build_file`); `STAGED_STATE` in genesis_adopt.rs:42; literal again in bin/arkiv-proposer.rs:300
   (`"prefill-work/bulk-v1/complete/state.redb"`, although `genesis_adopt::staged_path` exists and is used at :368).
   `".{name}.stream-work"` is formatted at genesis_streaming.rs:503 and genesis_prebuilt.rs:216.
   `"prefill-work"`: proposer :295, 300, 321, 367, 1004. Fix: `pub(crate) mod layout` with the constants and
   `work_dir(output)`, owned by genesis_bulk. **safe**.

9. **Number / hex / decimal text parsers.**
   - genesis_authoring.rs:334-351 `fixed_hex` + `hex_digit` is `value_json::exact_hex::<N>` (value_json.rs:141-144,
     private) with an error message. Make `exact_hex` `pub(crate)` and wrap. **safe**.
   - genesis_authoring.rs:307-321 `decimal_integer` + :322-325 `number`, `demo::wire::number` (demo/wire.rs:53-61)
     and bin/arkiv-proposer.rs:130-138 `amount` are the same "canonical decimal, no leading zero" test.
   - genesis_authoring.rs:359-388 `decimal_units` re-does `value_json::parse_scaled(s, 18, 256)`
     (value_json.rs:62-93) and adds two stricter rules (canonical whole part, no negative zero). Fix: apply the
     two checks, then call `parse_scaled`. **needs-care** `[CONSENSUS]`: the boundary cases pinned by
     `integer_decimal_time_and_hex_boundaries_are_exact` (genesis_authoring.rs:1709) must still pass.

10. **genesis_authoring.rs re-declares the small-path limits.** `ROW_CAP` / `DOCUMENT_CAP` / `INPUT_CAP` (:30-32)
    shadow `genesis_bundle::{MAX_ENTITIES, MAX_CANONICAL_BYTES, MAX_STORAGE_UNITS}`, and :948-953 re-implements
    `PrefillLimits::validate` (genesis_bundle.rs:47-56) using `INPUT_CAP`, a source-bytes cap, as the
    storage-unit cap only because both are 8 MiB. Fix: `limits.validate()?` (it runs anyway in
    `construct`, genesis_bundle.rs:472). **safe** apart from the wording of one error string (no test matches it:
    `rg 'prefill limits exceed' crates` hits only the two definitions).

11. **Unit-test fixtures copied between genesis_streaming.rs, genesis_prebuilt.rs and tests/.** (~190 lines)
    - `Temp` (pid + counter temp dir with `Drop`): genesis_streaming.rs:612-629, genesis_prebuilt.rs:386-403;
      genesis_authoring.rs `Fixture` :1260-1315 is the same plus example files.
    - `base()`: genesis_streaming.rs:630-644 and `base(run: u8)` genesis_prebuilt.rs:404-419
      (differs: run id parameter, also funds the proposer).
    - `ARKPFL02` framing by hand: genesis_streaming.rs:680-690, genesis_prebuilt.rs:423-449,
      tests/genesis_prefill.rs:594-602, tests/genesis_adopt.rs:79-110, tests/prebuilt_genesis_cli.rs:63-.
      The crate has a reader for this format and no writer. Fix: `pub fn write_stream(rows, impl Write)`
      in the streaming module (real API, not test-only); the five builders become one call.
    - The small `PrefillLimits { 256, bundle::MAX_CANONICAL_BYTES, bundle::MAX_STORAGE_UNITS }` literal:
      genesis_streaming.rs:694-698, 730-734, 898-902, 1216-1220; genesis_prebuilt.rs:565-569.
    - `signed_stream` (genesis_streaming.rs:691-713) and `signed_permanent_stream` (:727-751) differ in a
      bool and a version number: merge.
    - The 10-line `bulk_build(&base(), limits(), [2; 20], &path, &work, Options::default(), N, &summary)`
      call appears 11 times (genesis_streaming.rs:853, 1229, 1257, 1269, 1298, 1311, 1322, 1346, 1362, 1525,
      1549): a 3-argument test helper saves ~80 lines.
    - The 11-line `try_get_record(entity_namespace, key, Projection::ALL, unlimited)...value.unwrap()` chain:
      genesis_streaming.rs:804-814 and :1434-1445.

12. **`65536` as the genesis.bin size cap, ten times.** bin/arkiv-proposer.rs:205, 558, 686, 900, 970, 1067;
    genesis_authoring.rs:1169; genesis_bulk.rs:91; genesis_streaming.rs:471 (+ `65537` at :475).
    Fix `pub const MAX_GENESIS_BYTES`. **safe**.

### II.1 [N2] arkiv-node runtimes: signed / simulator / demo

*(paths relative to `crates/arkiv-node/`)*

Ordered by payoff. Estimated total removable: ~520 lines.

1. **signed/host.rs:197-237, 294-336, 386-427 - height-zero setup written three times** (`create_with_prefill`,
   `create_from_staged`, `adopt_staged`). Lines 206-237 and 305-336 are byte-identical (32 lines; diffed); 398-427
   differs only in `&genesis` vs `genesis`. Each: certificate/signer identity check, `SimulatorIdentity`, `identity`,
   empty genesis `SignedBlock`, `history.persist`, `HeaderRecord`, `SimulatorPacket`. Also the `Initialization`
   match 256-260 == 344-348 (== simulator/host.rs:333-336) and the 16-field `Host { .. }` literal at 262-279,
   349-366, 524-541.
   Fix: `fn genesis_packet(history: &mut History, genesis: &SignedGenesis, source: [u8; 16], certificate: SignedHeader,
   signer: Option<&Signer>) -> Result<(SimulatorIdentity, Identity, SimulatorPacket)>`, `fn committed(i: Initialization)
   -> Result<DurableEngine>`, and one private `fn assemble(engine, genesis, identity, history, signer, head) -> Self`.
   ~95 lines. **needs-care** (genesis record bytes are on-disk format **[CONSENSUS]**: move, do not change).

2. **HTTP plumbing re-typed from simulator/runtime.rs into signed/runtime.rs.** Single home: a new `src/http.rs`.
   - `refusal`/`respond`/`response`/`response_with`: simulator/runtime.rs:305-360 vs signed/runtime.rs:439-486.
     `refusal` differs in 2 lines. Keep the two status tables as data (`fn status_of(code) -> StatusCode` per
     profile) - they differ on purpose or by accident (simulator default 400, signed default 422; signed has no 404/409
     arms) and are **[CONSENSUS]** (HTTP status). ~40 lines.
   - bounded fetch: simulator/runtime.rs:542-601 `fetch_authorized` vs signed/runtime.rs:1039-1090 `fetch_once`
     (same URL join, body/token builders, chunk loop; differ in the over-cap code "InvalidRequest" vs
     "CapacityReached" and in the error-body table) vs demo/wire.rs:225-263 `rpc` (third copy of the chunk loop).
     Share `async fn fetch_bytes(client, url, body, token, cap) -> Result<(StatusCode, Vec<u8>)>`. ~45 lines.
   - body parse: simulator/runtime.rs:390-396 `parse_body` == signed/runtime.rs:696-702 `parse` (cap is a
     parameter in one); the same timeout+`to_bytes` is inlined again at simulator/runtime.rs:431-435 and
     signed/runtime.rs:770. ~15 lines.
   - `authorized`: simulator/runtime.rs:467-475 vs signed/runtime.rs:931-936. ~6 lines.
   - `head_json` (simulator/runtime.rs:113-115) == `snapshot` (signed/runtime.rs:354-356), and written inline a
     third time at simulator/light.rs:439. ~4 lines.
   - light-node retention nulls + merge: simulator/runtime.rs:117-122 + 151-153 == signed/runtime.rs:428-436.
     Belongs in `crate::retention` (`fn status_fields(Option<&DurableEngine>) -> Map`). ~8 lines.
   - reqwest client: simulator/runtime.rs:57-61 == signed/runtime.rs:131-135 == demo/wire.rs:264-270 (and
     `bin/arkiv-simulator.rs:319`, `bin/arkiv-proposer.rs:767, 1084`). One `http::client()`. ~10 lines in chunk.
   - admission block (`match rt.<sem>.clone().try_acquire_owned() { Ok(p) => p, Err(_) => return response(Err(Error("ServerBusy"))) }`):
     simulator/runtime.rs:372-375, 398-401, 480-483; signed/runtime.rs:617-620, 680-683, 740-743, 946-949 (seven
     copies). The demo already has the better shape: `.try_acquire_owned().map_err(|_| DemoError::ServerBusy)?`
     inside a `Result` block (demo/runtime.rs:648-652, 752-756). Fix: `fn admit(&self) -> Result<OwnedSemaphorePermit>`
     on each `Runtime`, and give every handler an inner `async fn(..) -> Result<Json>` so the permit is taken with
     `?`. ~20 lines.
   **needs-care**; status codes and error bodies **[CONSENSUS]**.

3. **Verified-page building, twice** - simulator/light.rs vs signed/runtime.rs. Single home: the `query` module of
   the recommendation above.
   - continuation lookup: simulator/light.rs:338-354 == signed/runtime.rs:880-899 (same tuple
     `VecDeque<(String, EqRequest, VerifiedContinuation)>`, same three-field comparison). Make it a type
     `Continuations` with `resume(&mut request, cursor) -> Result<Option<VerifiedContinuation>>` and
     `remember(id, request, token)` (cap 64: simulator/light.rs:432, signed/runtime.rs:925).
   - entity JSON: simulator/light.rs:411-418 == signed/runtime.rs:924 (same keys, written in a different order;
     `serde_json` is built without `preserve_order` - `Cargo.lock` shows no `indexmap` dependency for it - so
     objects serialise key-sorted and the two produce identical bytes).
   - inspection attachment: simulator/light.rs:388-393 + 442-450 == signed/runtime.rs:927.
   - liveness predicate `created_at > h || updated_at > h || (expires_at != u64::MAX && expires_at <= h)`: four
     copies - signed/light.rs:426-428, 462-464, simulator/light.rs:413, signed/runtime.rs:924. Home:
     `ArkivEntityProjection::live_at(height)` in `crate::arkiv`.
   ~45 lines. **needs-care**.

4. **Text query / classify / DTOs** - simulator/runtime.rs:604-673 vs signed/runtime.rs:729-738 + 759-767.
   Same gate (language valid -> proof supported -> execution supported -> selection present), same
   `VerifiedRequest` construction, same `"resource_limit"`/`"failed"` mapping (simulator 667-669, signed 766).
   DTO pairs: `TextQueryRequest` (simulator 604-613) == `TextQuery` (signed 729-738, 2 differing lines);
   `ClassifyRequest` (simulator 403-407) == `Classify` (signed 719-723);
   `struct ProofRequest` at signed/runtime.rs:714-718 re-declares `simulator::wire::ProofRequest` (wire.rs:342-346)
   that is already in scope through `wire::*` (signed/runtime.rs:18) - the local one silently shadows the glob.
   Fix: one `text_gate(&TextQuery, &Classification) -> Option<&'static str>` + shared DTOs; delete
   signed/runtime.rs:714-718 outright (**safe**). The two profiles answer a rejected text query differently
   (simulator: HTTP 422 with body; signed: HTTP 200 with an `error` field) - keep that **[CONSENSUS]**. ~30 lines.

5. **simulator/runtime.rs - `match node { Node::Full(h) => .., Node::Light(h) => .. }` projecting the same field**,
   seven times: 123-149, 198-201, 256-259, 260-263, 452-455, 735-738, 890-893. signed/runtime.rs:51-76 already
   solved this with `Node::{genesis, identity, head, certificate}`. Add `identity()`, `head()`, `header(height)`,
   `run()`, `paused()`, `health_mut()` to the simulator `Node`. ~30 lines. **safe**.

6. **demo/ helpers duplicated in simulator/** (the simulator kept `hex/unhex/number` by delegation but re-typed
   the rest): `packed`/`unpack` demo/wire.rs:41-52 vs `pack`/`unpack` simulator/wire.rs:22-35 (also a naming split:
   `packed` vs `pack`); `unpack_hash` demo/runtime.rs:603-609 vs `hash` simulator/wire.rs:36-42; `proof_limits`
   demo/runtime.rs:610-615 vs simulator/wire.rs:358-363; `proof_error` demo/runtime.rs:616-628 vs
   `From<ProofError>` simulator/mod.rs:46-59; `"eq-page-v1"` literal at demo/runtime.rs:195, 571, 685 vs
   `simulator::profile::PROOF_PROFILE`; `version()` demo/auth.rs:14-20 vs signed/protocol.rs:51-60.
   Home: the neutral `wire.rs`. ~40 lines. **safe**.

7. **Snapshot-reference check, five copies**: demo/runtime.rs:358-368, simulator/host.rs:723-730,
   signed/host.rs:1194-1200, signed/light.rs:407-412, 444-449 (`genesis_hash`, `block_hash`, `state_root` against a
   header). One `fn names(snapshot: &SnapshotRef, genesis_hash: B256, header: &Header) -> bool` next to `Header`
   in `block.rs`; callers keep their own error code. ~20 lines. **safe**.

8. **signed/light.rs:400-435 `verify_eq_at` vs 438-471 `verify_range_at`**: 407-412 == 444-449 and 420-433 ==
   456-469 (2 differing lines). Extract the snapshot check (item 7) and
   `fn check_entity_rows(rows: &[Row], height: u64) -> Result<()>`. ~18 lines. **safe**.

9. **simulator/inspection.rs** internal: `NodeKind`/`PathSource` name matches 84-85 == 151-152; posting-set JSON
   96-103 == 133-140; node JSON 87-90 == 167-170; `Bound::Included`/`Excluded` arms 179-180 differ only in the
   bool. Fix: `fn kind_name(NodeKind)`, `fn source_name(PathSource)`, `fn posting_json(&InspectedPostingSet)`,
   `fn node_json(&PathNode, index) -> Value` (range adds its extra keys). ~25 lines. **safe** if keys unchanged
   **[CONSENSUS]** (response body).

10. **signed/history.rs** small repeats: frozen/observation checks 183-188 == 248-253 (0 differing lines);
    `self.head.height.checked_add(1).ok_or(Error("CapacityReached"))?` at 163-167, 289-293, 384-388;
    `read_object_bounded(SpaceId::Host, &r.id.0, r.len as usize + FRAME_OVERHEAD)` at 204-211, 229-236, 358-365;
    identity bytes 69-70 == 448-449. Fix: `fn retryable(&self)`, `fn next_sequence(&self)`,
    `fn read_frame(&self, &BodyRef)`, `fn identity_bytes(store, genesis_hash)`. ~25 lines. **safe**.

11. **demo/runtime.rs**: 17-field `Node` literal at 99-117 and 138-156; `sync_once` light/full branches repeat
    sample parsing (885-890 vs 923-928) and authenticate + gap check (892-901 vs 930-938). ~30 lines. **safe**.

12. **simulator/host.rs vs signed/host.rs**: `engine_genesis` 90-98 vs 103-111 (differ in `initial_state`);
    `identity` 82-89 vs 112-119 (equal output: signed sets `node_genesis_hash: genesis.hash()` at 217);
    `block()` 407-419 vs `record()` 564-576; control-key bytes simulator/host.rs:428-429 == 669-670.
    ~15 lines. **safe**.

13. **Headers pagination**: simulator/runtime.rs:264-275 vs signed/runtime.rs:518-531 (parse `afterHeight`,
    `BlockNotSynced`, compare `afterHash`, `ChainConflict`, `checked_add(1)`). ~10 lines. **needs-care**.

14. **signed/host.rs:577-590 vs 599-612**: the same bounded `RefCell<BTreeMap<u64, T>>` cache (lookup, load,
    insert, `pop_first` over cap). One generic `fn cached<T: Clone>(cache, cap, height, load)`. ~10 lines. **safe**.

15. **Health assignment** `if head == sampled { "following" } else { "stalled" }`: simulator/runtime.rs:794-798,
    852-856, simulator/light.rs:326-330, signed/light.rs:259-263. Goes away with the `Health` enum (Sloppiness 2).

16. **Duplicate constants** (**safe**): `HEADER_BATCH = 64` at simulator/wire.rs:10 and signed/light.rs:18, while
    signed/runtime.rs uses the literal `64` (514, 515, 1111, 1118) with `wire::HEADER_BATCH` in scope;
    `CERTIFICATE_LEN = 4 + 32 + 156 + 65` at signed/history.rs:21 and signed/light.rs:16, both equal to
    `SignedHeader::MIN_ENCODED_LEN` (protocol.rs:559); `BODY_CAP` at signed/history.rs:13 (`usize`) and
    signed/protocol.rs:33 (`u32`); `Duration::from_secs(5)` at simulator/runtime.rs:59, 391, 432 and
    signed/runtime.rs:133, 697, 770, 1025 while `demo::wire::DEADLINE` (demo/wire.rs:12) is the same value;
    `"entities"` at signed/protocol.rs:470, signed/host.rs:810, simulator/workload.rs:49, demo/workload.rs:88
    (and `genesis_bundle.rs:486`, `genesis_bulk.rs:190` outside the chunk) - `crate::arkiv` has
    `entity_namespace()` but no name constant;
    `"arkiv-entity-v3"` at simulator/profile.rs:24, simulator/light.rs:440, signed/runtime.rs:926 (and
    `genesis_authoring.rs:44 ENTITY_RULES`); `8 * 1024 * 1024` at simulator/wire.rs:266, simulator/host.rs:96,
    signed/host.rs:109, 1057 next to `RESPONSE_CAP`.

### II.1 [S2] arkiv-db src: durable, proofs, small modules

*(paths relative to `crates/arkiv-db/src/`)*

Payoff order (lines removable): D1 (150-170), D2 (75), D15 (70), D12 (45), D5 (35), D3 (30), D4 (20), D6 (15),
then D7, D9, D10, D11 (10 each). D8 and D13 remove no lines but remove a drift hazard. D14 is the helper inventory.

#### D1. Equality proofs (`proof.rs`) and range proofs (`proof/range.rs`) repeat each other block for block
This is the cause of all four `too many lines` warnings (proof.rs:606 174 lines, proof.rs:887 157,
proof/range.rs:453 181, proof/range.rs:644 203). The two verifiers and the two generators share these blocks:

| Block | eq | range | Lines each |
|---|---|---|---|
| Verify one row and its key binding (read id, row path, `proven` Rows, `decode_row`, key path, `proven` Keys, compare bound id) | proof.rs:747-781 | proof/range.rs:596-634 | ~35 |
| Emit one row (cell-limit check, `read_bytes`, charge, `append(8)`, row bound, row path, key path) | proof.rs:983-1030 | proof/range.rs:801-850 | ~48 |
| Rebuild a posting map from ordered ids and compare its root | proof.rs:709-730 | proof/range.rs:546-567 | ~20 |
| Emit posting ids (`append(8)`, `charge(PostingEntry)`, `iter.next()`) | proof.rs:961-970 | proof/range.rs:765-777 | ~10 |
| Catalog path, `proven` Catalog, decode `NamespaceState`, id check | proof.rs:641-662 | proof/range.rs:482-499 | ~18 |
| `NamespaceRoots { next_record, records, keys, rows, terms }` literal | proof.rs:664-673 | proof/range.rs:501-510 | 10 |
| Witness-node loop (`append_capacity`, `check_capacity(ProofNode)`, node cap, probe delta charge, size check, `append`, `nodes += 1`) | proof.rs:849-875 (`Serving::path_bounded`) | proof/range.rs:697-725 (inline) | ~27 |
| Generator setup (validate, root check, `Serving {..}`, overhead `append`, `DirectoryLookup`, catalog `get` + path) | proof.rs:896-919 | proof/range.rs:653-679 | ~24 |
| Generator tail (length check, `finish`, `model_version`, `GeneratedProof`) | proof.rs:1048-1059 | proof/range.rs:853-867 | ~12 |

Inside the row-emission block three sub-blocks are character-identical: the cell-limit check
(proof.rs:993-1000 = proof/range.rs:813-820), `read_bytes` (proof.rs:1001-1006 = proof/range.rs:821-826) and the
two `serving.path` + `write_path` calls. The row bound is the same formula written two ways
(proof.rs:1016-1020 `checked_add(128).and_then(.. checked_mul(10)?)` vs proof/range.rs:838-841
`checked_add(128 + row.cells.len() * 10)`).

The `NamespaceRoots` literal has a third copy at engine.rs:901-907 (`Snapshot::try_namespace_roots`) and a
fourth, as a `Vec<(&str, B256, u64)>`, at explore.rs:114-119.

Fix, all in `proof.rs` (range.rs already does `use super::*`):
1. `impl NamespaceState { pub(crate) fn roots(&self) -> NamespaceRoots }` in layout.rs; use it at the four sites. **safe**, -30 lines.
2. `fn proven_namespace(d, header, id, inspection, meter) -> Result<Option<NamespaceState>, ProofError>` for the catalog block. **safe**, -15.
3. `fn rebuild_posting(d, count, meter, visit: impl FnMut(RecordId) -> Result<(), ProofError>) -> Result<B256, ProofError>` for the id loop. **needs-care** (the two loops differ in how they detect duplicates: `ids.last()` vs `last` plus `selected.contains_key`), -15.
4. `fn verified_row(d, namespace, id, attribute: &CellName, value: &Value, limits, inspection, meter) -> Result<Row, ProofError>`. This also removes the per-row `EqQuery { attribute: request.query.attribute.clone(), .. }` built at proof/range.rs:612-616 only to satisfy `decode_row`'s signature (`decode_row` reads just `query.attribute` and `query.value`, proof.rs:550-552). **safe**, -35.
5. `Serving::emit_row(..)`, `Serving::emit_posting_ids(..)`, `Serving::finish(self, e, posting_count)`. **needs-care [CONSENSUS]**: receipts. The charge sequences differ on purpose (range adds `(Op::ReadByte, row_bound)` to `check_capacity` at proof/range.rs:801-805 and has the extra `guard` accounting at 787-800, 827-832). Keep each generator's charge order byte for byte; share only the parts after the charge. -50.
6. Make `Serving::path_bounded` take the per-step visit bound so the range witness loop can call it. **needs-care [CONSENSUS]** (range charges `visit_bound = (2*max_key_len+1) * META_NODE_BYTES`, eq charges `structural`). -20.

Estimated total: 150-170 lines, and all four functions drop under 100 lines.

#### D2. Four height-zero publication paths rebuild the same batch by hand
`initialize_inner` (durable/mod.rs:455-577), `initialize_bulk` (durable/bulk.rs:651-704), `publish_staged`
(durable/streaming.rs:295-392) and `adopt_staged_simulator` (durable/adopt.rs:515-608).

| Fragment | Sites |
|---|---|
| Source must be a ready engine-only stage whose state, limits and pricing match `genesis` | streaming.rs:303-316 = adopt.rs:523-536 (14 lines, only the first message differs) |
| Target has no head and no `PROFILE_KEY` / `GENESIS_KEY` / `IDENTITY_KEY` record | head check bulk.rs:668-670 and streaming.rs:317-321; then the identical key loop bulk.rs:671-682 = streaming.rs:322-333 (same odd cap `GENESIS_MAX_BYTES + IDENTITY_MAX_BYTES + 32`); third spelling at mod.rs:482-506 |
| `EngineManifest { genesis: genesis.id(), sequence: 0, parent: None, state_root, state }` | mod.rs:529-535, bulk.rs:683-689, streaming.rs:337-343, adopt.rs:553-559 |
| `manifest_batch(None, ..)` + profile + genesis (+ identity) records (+ packet) + `logical_bytes() > prepare_bytes` | mod.rs:536-553, bulk.rs:690-696, streaming.rs:344-374, adopt.rs:561-571 |
| `InitializationPending { engine, batch, retry_allowed: true }.commit()` | mod.rs:571-576, bulk.rs:698-703, streaming.rs:386-391 |
| `packet.reserved_bytes()? > options.prepare_bytes` | mod.rs:475-481, mod.rs:793-800, streaming.rs:358-362, adopt.rs:548-552 (two messages: "host packet exceeds preparation cap" / "genesis packet exceeds batch cap") |
| `simulator.map_or_else(profile_bytes, \|_\| simulator::profile_bytes())` | mod.rs:270, mod.rs:537-539, streaming.rs:349-351 |
| `CommitInfo { snapshot, commit_id: manifest.id(), state_root: manifest.state_root }` | mod.rs:647-651, mod.rs:870-874, adopt.rs:589-593, simulator.rs:255-259 |

Fix, single home `durable/mod.rs` (or `format.rs` for the two `EngineManifest` methods):
`EngineManifest::genesis(&EngineGenesis, StateHeader) -> Self`, `EngineManifest::commit_info(&self) -> CommitInfo`,
`fn genesis_batch(manifest, genesis, identity: Option<&SimulatorIdentity>, packet: Option<&SimulatorPacket>, options) -> Result<CommitBatch, DurableError>`,
`fn ensure_unpublished(store: &dyn VersionedStore) -> Result<(), DurableError>`,
`DurableEngine::ensure_staging_source(&self, genesis) -> Result<(), DurableError>`,
`InitializationPending::first_commit(engine, batch) -> Result<Initialization, DurableError>`,
`SimulatorPacket::check_fits(&self, options)`.
**needs-care [CONSENSUS on-disk]**: the batch contents and record order must stay identical; the error texts would
be unified (they are not matched anywhere: `rg -n "exceeds preparation cap|exceeds batch cap|bulk target" crates conformance scripts` hits only the definitions). About 75 lines.

#### D3. Three walkers of a staged state, each with its own leaf checks, counting loop and error newtype
- `verify_genesis_objects` (mod.rs:343-442), `Copier` (streaming.rs:116-175), `Verifier` (adopt.rs:244-277, 402-432).
- streaming.rs and adopt.rs already share `check_namespace` / `check_row` / `check_host` (streaming.rs:179-201).
  mod.rs re-implements all three inline at mod.rs:418-420, 427-430 and 436-441, with different text and a
  different error kind (`Incompatible` where the shared ones say `Corrupt` for the namespace and host checks).
- The count-and-compare loop is written three times: mod.rs:390-400, streaming.rs:126-136, adopt.rs:416-431.
- The error newtype is written four times: `VerificationError` (mod.rs:358-365), `CopyFailure`
  (streaming.rs:15-20), `Stop` (adopt.rs:39-44), `Fail` (reachability.rs:79-89). `CopyFailure` and `Stop` are
  identical except for the name, down to the message `"import traversal: {e}"`.

Fix: one `pub(super) struct WalkError(pub DurableError)` with `From<TrieError>` in streaming.rs; delete `Stop`
and `CopyFailure` (**safe**, -12). Have `verify_genesis_objects` call the three shared `check_*` functions
(**behaviour-affecting**: error text and kind for a bad genesis candidate change; tests match only
`"entry count mismatch"`, at streaming.rs:544, 553 and adopt.rs:847). About 30 lines in total.

#### D4. Two "buffer objects, flush with `stage_objects`" implementations with different overhead constants
`Sink` (bulk.rs:301-355) and `Copier::flush`/`push` (streaming.rs:87-115). The flush bodies match
(bulk.rs:326-333 = streaming.rs:92-99). The per-object overhead charged against `prepare_bytes` is `+ 64` in
bulk.rs:340 and `+ 32` in streaming.rs:103, streaming.rs:205 and prepare.rs:200. Nothing explains the 64.
Fix: one `struct ObjectBatch { pending, bytes }` with `push`/`flush` and one `const OBJECT_OVERHEAD: usize = 32`
in prepare.rs; `Sink` and `Copier` own one each. **behaviour-affecting** only in when the bulk build flushes
(not canonical: objects are content-addressed). About 20 lines.

#### D5. `durable/bulk.rs` repeats small blocks inside itself
- `if let Some(check) = &self.disk_check { check().map_err(io)?; }` five times: bulk.rs:208-210, 218-220, 244-246, 270-272, 323-325.
- The "check disk every 64 MiB written" block twice: bulk.rs:217-222 = 269-274, with the literal `64 * 1024 * 1024` in both. Home: a method on `RunWriter`.
- The poison-flag protocol (`if self.failed { return Err(..) } self.failed = true; .. self.failed = false;`) three times plus one check: bulk.rs:426-429+460, 464-467+474, 483-486+491, 500-502. Home: `fn guarded<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T, DurableError>) -> Result<T, DurableError>`.
- Flush of the current posting into `terms`, twice: bulk.rs:532-539 = 545-552.
- `set_disk_check` and `set_progress` each write the same `Rc` into four sorters (bulk.rs:408-411, 416-419), and `finish` reads them back out of `self.keys` (bulk.rs:514, 516). Home: one `Rc<Hooks { disk_check, progress }>` shared by the four sorters and the sink.

**safe**, about 35 lines.

#### D6. Record comparison loops in the two recovery functions
"every record of the pending batch is stored with the same bytes": mod.rs:951-955 = mod.rs:1099-1104.
"no record of the pending batch exists": mod.rs:958-966 = mod.rs:1077-1087.
Fix: `fn records_present(store, batch, what: &'static str) -> Result<(), DurableError>` and `fn records_absent(..)`
beside `required` (mod.rs:209). **safe** (keep the four distinct `corrupt` texts as the `what` argument), -15 lines.

#### D7. Installing a prepared batch
mod.rs:768-779 (`prepare_packet`) = streaming.rs:250-261 (`prepare_import`): the same `match` that stores
`pending`, sets `WriterState::Prepared` and boxes the candidate on failure. Fix: give `prepare_packet` a
`checkpoint: Option<&[u8]>` parameter and have `prepare_import` call it after its size check. **safe**, -10.

#### D8. `durable/simulator.rs` writes every magic and digest domain twice
`b"ARKSIMI1"` (simulator.rs:54, 65), `"arkiv/simulator-profile/v1"` (59, 66), `b"ARKSIMB1"` (150, 234),
`"arkiv/simulator-node-record/v1"` (158, 235), `b"ARKSIMR1"` (162, 376), `"arkiv/simulator-inclusion/v1"`
(171, 377). The encoder and decoder of one record can drift apart with a one-character typo.
Fix: six `const`s at the top of the file. **safe [CONSENSUS on-disk]** (the bytes must not change), no line change.
Same file: `if self.simulator.is_none() { return Err(incompatible("expected simulator profile")) }` at 349-351 and 360-362;
`if let Health::Failed(e) = store.health() { return Err(e.into()) }` at simulator.rs:211-213, 280-282 and mod.rs:263-265.

#### D9. Profile record: two builders and a bare length
`profile_bytes()` exists twice (format.rs:286-288, simulator.rs:17-19), each allocating a `Vec` per call; the
simulator one inlines `b"ARKSIM01"` where format.rs has `PROFILE`. The record length `12` is a bare literal at
mod.rs:266, 267, 497 and simulator.rs:283, 293. Fix in format.rs: `pub const PROFILE_LEN: usize = 12;`,
`pub const ENGINE_PROFILE: [u8; 12]`, `pub const SIMULATOR_PROFILE: [u8; 12]`, and
`fn profile_for(simulator: bool) -> &'static [u8; 12]` (also removes the three `map_or_else` sites in D2).
**safe [CONSENSUS on-disk]**, -10.

#### D10. `reachability::manifest` is the first half of `read_manifest`
reachability.rs:170-183 and mod.rs:231-240 both do "bounded read of `Table::Manifests`, `from_envelope`, check
`manifest.sequence`". Fix: `fn load_manifest(store, sequence)` in mod.rs, called by both; `read_manifest` adds
the genesis and index association checks. **safe**, -10.

#### D11. Poison-tolerant locking spelled three ways
`.lock().unwrap_or_else(PoisonError::into_inner)` four times in retention.rs (144-147, 156-158, 164-166,
186-190); a private `fn lock<T>` helper in adopt.rs:133-135 (and the same idiom on the condvar at 196-199);
`.unwrap_or_else(|e| e.into_inner())` in cache.rs:403. Fix: move adopt.rs's `lock` to durable/mod.rs as
`pub(super) fn lock` and use it in retention.rs and cache.rs:403. **safe**, -10.

#### D12. Smaller repeats
- prepare.rs: `u32::try_from(len).map_err(|_| DurableError::ResourceLimit("..".into()))?;` four times (33-34, 39-40, 47-48, 52-53); the `Value::Str` and `Value::Bytes` arms (46-55) differ only in the message. Fix: `fn fits_u32(len: usize, what: &'static str) -> Result<(), DurableError>` and one `Value::Str(v) | Value::Bytes(v)`-style arm via a helper taking `len`. **safe**, -10.
- proof/range.rs: `[&self.lower, &self.upper].into_iter().filter_map(bound_value)` at 49-51 and 68-70. Fix: `fn bounds(&self) -> impl Iterator<Item = &Value>`. **safe**, -4.
- format.rs: the six-element version list at 127-134 and again at 156-163. Fix: one `const PROFILE_VERSIONS: [u32; 6]`. **safe [CONSENSUS on-disk]**, -7. (See Other W3 for the two anonymous entries.)
- format.rs: "encode, then append `id()`" at 100-104 and 200-204; simulator.rs `seal` (184-188) is the same shape with a domain argument.
- adopt.rs: `Part { map, space, lower: None, upper: None, total: None }` at 155-161 and 233-239 (fix: `Part::whole(map, space)`); `Total { expected: len, seen: AtomicU64::new(0), parts: AtomicUsize::new(n) }` at 314-318 and 340-344 (fix: `Total::new(len, parts)`); `match part.space { SpaceId::Namespace(n) => n, _ => 0 }` at 248-251 and 301-304. **safe**, -12.
- mod.rs: `Snapshot::from_header(id, parent, instance, limits, &manifest.state, Nodes::durable(ctx.clone()), Some(pin))` at 616-624, 728-737, 875-883. Fix: a private `fn snapshot_of(context, instance, limits, manifest, parent, pin)`. **safe**, -10.
- reachability.rs: `recent.iter().map(|n| (*n).max(1))` at 213 and 226.

#### D13. Two `corrupt` helpers in one crate with the same name and different return types
`nodes::corrupt -> StorageError` (nodes.rs:208-210) and `durable::corrupt -> DurableError` (mod.rs:114-116,
which builds the same `StorageError` and converts). Fix: `durable::corrupt(s)` becomes
`crate::nodes::corrupt(s).into()`, or rename one. **safe**.

#### D14. Unit-test helpers in `src/` (for the shared `tests/support` design)
| Helper | Location | Signature | Notes |
|---|---|---|---|
| `Temp` (scratch dir, removed on drop) | bulk.rs:712-728 (17 lines) | `struct Temp(PathBuf)`, `Temp::new() -> Self`, static `SERIAL: AtomicU64`, prefix `arkiv-bulk-` | does not create the dir |
| `Temp` | adopt.rs:688-693 + inline construction 698-703 | `struct Temp(std::path::PathBuf)`, no constructor, fn-local `static NEXT`, prefix `arkiv-adopt-` | differs: no `new` |
| (same shape elsewhere) | `crates/arkiv-node/src/genesis_prebuilt.rs:387`, `genesis_streaming.rs:613`, `crates/arkiv-node/tests/prebuilt_genesis_cli.rs:25`, `tests/genesis_adopt.rs:42` | | 6 copies in the workspace |
| `descriptor()` | bulk.rs:729-736 | `fn descriptor() -> NamespaceDescriptor` (ordinal 1, name "entities", owner `[1; 20]`, `Revision::INITIAL`) | same literal inline at adopt.rs:704-709; same fn at `crates/arkiv-db/tests/adopt.rs:27` |
| `engine(init)` | bulk.rs:737-742 | `fn engine(init: Initialization) -> DurableEngine` | same thing as let-else at adopt.rs:735-739, mod.rs:1138-1145, mod.rs:1223-1230; see Other W1 |
| `staged(records)` | adopt.rs:697-741 | `fn staged(records: u64) -> (Arc<MemoryVersionedStore>, DurableEngine, EngineGenesis)` | specific to adoption |
| `probe(store)` | adopt.rs:743-749 | `fn probe(store: &Arc<MemoryVersionedStore>) -> Probe` | |
| `posting(first, count)` | streaming.rs:403-426 | `fn posting(first: u64, count: u64) -> (PostingMap, Vec<(ObjectId, Vec<u8>)>)` | |
| `stage(store, ns, objects)` | streaming.rs:428-434 | `fn stage(store: &MemoryVersionedStore, namespace: u64, objects: &[(ObjectId, Vec<u8>)])` | |
| `copier(..)` | streaming.rs:436-451 | `fn copier<'a>(source, target, options, slots: usize) -> Copier<'a>` | |
| `object(i)`, `store_with`, `options`, `context`, `load`, `charge`, `present`, `assert_consistent` | cache.rs:538-635 | see file | specific to the cache; `object` and streaming's `posting` both do "build map, `Staged::new()`, `map.stage`, `into_objects()`" |

#### D15. The delegating store double in adopt.rs
adopt.rs:656-686 is 31 lines of pure delegation (`database_id`, `head`, `read_record`, `read_record_bounded`,
`commit`, `health`, `recover`, `stats`) so that `Probe` can override two object-read methods. The workspace has
eight such hand-written doubles (`rg -n "impl VersionedStore for" crates`: adopt.rs:656,
`tests/durable_failures.rs:420` and `:1284`, `tests/simulator_composition.rs:324`, `tests/adopt.rs:359`,
`tests/streaming_import.rs:308`, `crates/arkiv-db-redb/tests/engine_process.rs:52`,
`crates/arkiv-node/tests/signed_reads.rs:104`).
Fix: add to `arkiv_db_storage::testing` a `TamperStore<S>` (or two methods on `FaultyStore`):
`objects_read() -> HashSet<(SpaceId, ObjectId)>`, `hide_object(id)`, `flip_object(id)`. Then delete
adopt.rs:625-686 and `probe()` (-70 lines here). `arkiv-db-storage` is already a normal dependency of
`arkiv-db`, so `arkiv_db_storage::testing` is reachable from this unit-test module without a new dev-dependency.
**needs-care** (test semantics). Also: the name `Probe` shadows `arkiv_db_trie::Probe`, which the rest of the
crate uses for something unrelated; call it `ReadSpy`.

### II.1 [NT3] arkiv-node tests: genesis / simulator / demo / CLI; examples; Cargo.toml; clippy.toml

*(paths relative to `crates/arkiv-node/`)*

#### D1. CLI process harness is written four times -> one `tests/cli_support/mod.rs`  (biggest payoff, ~-230..-260 lines net) - needs-care
All CLI tests locate the binary correctly with `env!("CARGO_BIN_EXE_arkiv-proposer")` / `env!("CARGO_BIN_EXE_arkiv-simulator")`
(`genesis_adopt.rs:36`, `prebuilt_genesis_cli.rs:22`, `retention_cli.rs:21,52,87`, `simulator_http.rs:436`) - that part is fine.
Everything around it is copy-pasted. `simulator_crash.rs` has no process/port/sleep code at all (in-process `CrashDisk`).

| piece | genesis_adopt.rs | prebuilt_genesis_cli.rs | retention_cli.rs | simulator_http.rs |
|---|---|---|---|---|
| temp dir | `struct Temp(PathBuf)` + `new(tag)` + `Drop` 42-55 (14 l) | same 25-39 (15 l); `diff` = only the name prefix | `run_dir(tag) -> PathBuf` 7-12, manual cleanup 67-68, 121 (no guard) | inline 102-104 + 122, 427-432 + 520 (no guard) |
| free port | `fn free_port() -> u16` 403-409 | 144-150, byte-identical | inline 81-84 (returns `String` addr) | `fn free_address() -> String` 372-375 |
| kill-on-drop child | `impl Drop for Producer` 523-528 | 221-226 | `struct Child(std::process::Child)` 71-77 | `struct Child` 365-371, byte-identical to retention_cli |
| readiness poll | `Producer::ready` 484-496 (120 s deadline, 50 ms) | 204-216, identical except the probe call | 104-116 (200 x 25 ms) | 463-471 (100 x 20 ms), 489-499 and 505-517 (150 x 20 ms, the two are the same loop) |
| HTTP probe | `fn http(port: u16, method: &str, path: &str, body: Option<&str>, token: Option<&str>) -> Option<serde_json::Value>` 410-433 | `fn status(port: u16) -> Option<serde_json::Value>` 151-163 = the GET-only subset of `http` | reqwest | reqwest |
| "CLI refuses these args" | `Command::output()` 722-732 | spawn + `try_wait` + 20 s deadline + kill 416-446 | `fn fails(binary: &str, args: &[&str]) -> String` 13-17 (`output()`) | - |
| `struct Producer { child, log, port[, private] }` + `start`/`ready`/`log` | 437-522 (86 l), 7 call sites | 165-220 (56 l), 5 call sites; `diff` = extra `--private-listen`/token/`--genesis-install` vs `--prebuilt-state` | - | - |
| `fn assert_genesis_head(status: &Value, report: &Value)` | 530-535 | 228-233, byte-identical | - | - |
| flip the tamper marker in a redb file | 623-632 | 322-331, identical except the variable name | - | - |

Fix: new `tests/cli_support/mod.rs` (same convention as `tests/support/mod.rs` / `tests/signed_support/mod.rs`, both `#![allow(dead_code)]`), ~200 lines:
`pub struct TempDir(pub PathBuf)` (name = tag + pid + an `AtomicU64` counter, as `src/genesis_streaming.rs:612-629` already does; `Drop` removes),
`pub struct Spawned(pub Child)` (kill + wait on drop), `pub fn free_port() -> u16`,
`pub fn wait_until<T>(what: &str, timeout: Duration, probe: impl FnMut() -> Option<T>) -> T`,
`pub fn refused(bin: &str, args: &[String], env: &[(&str, &str)]) -> String` (the deadline version from `prebuilt_genesis_cli.rs:416-446`; returns stderr),
`pub fn http(..)`, `pub struct Producer` with `start(dir, node, bundle, key, extra: &[&OsStr], tag)`, `assert_genesis_head`, `flip_marker(path, marker) -> usize`.
Workspace note for whoever designs the shared module: there are four more private `struct Temp` in src unit tests
(`src/genesis_prebuilt.rs:387`, `src/genesis_streaming.rs:613`, `crates/arkiv-db/src/durable/bulk.rs:713`, `crates/arkiv-db/src/durable/adopt.rs:688`).

#### D2. The prefill fixture (`inputs`) and the `ARKPFL02` file framing are hand-rolled five times - needs-care, ~-90 lines
- `genesis_adopt.rs:68-121` `fn inputs(dir: &Path) -> (PathBuf, PathBuf, PathBuf)` (54 l) vs `prebuilt_genesis_cli.rs:52-101` `fn inputs(dir: &Path, run: u8) -> (PathBuf, PathBuf, PathBuf)` (50 l).
  `diff` shows only: chain id 9452/9451, `run_id`, row count `ROWS`=48 / literal 24, `expires_at_height: 1000 + n / 4` vs `1000 + n`, and one extra `mod` cell. Everything else (base genesis, marker in row 5, key file) is identical.
  Fix: one `cli_support::prefill_inputs(dir, chain_id, run, rows, marker, cells: impl Fn(u64) -> Vec<(CellName, Cell)>) -> Inputs { base, stream, key }` (a struct, not a 3-tuple of `PathBuf`).
- The framing itself (`b"ARKPFL02"` + `ROW_CODEC` BE + row count BE + per row `u32` length + `encode(row)`) is written out in
  `tests/genesis_prefill.rs:595-602`, `tests/genesis_adopt.rs:79-82,109-110`, `tests/prebuilt_genesis_cli.rs:63-65,89-90`,
  `src/genesis_streaming.rs:680-690` (`fn stream`), `src/genesis_prebuilt.rs:423-449` (`fn stream_expiring`).
  `rg -n 'ARKPFL02' crates/arkiv-node/src` shows the library only has the reader (`genesis_streaming.rs:120`); the only writers are these five test copies and the Python converters.
  Fix: add the writer next to `StreamReader`, e.g. `pub fn encode_file_stream(rows: &[PrefillEntity]) -> Vec<u8>` in `src/genesis_streaming.rs` (10 lines, also gives the magic a named const), and call it from all five. **safe** as long as the bytes are unchanged `[CONSENSUS]` (the stream hash is committed in genesis; do not alter the framing while moving it).
- The "relabel a small v3 launch as a file-backed v4 launch" dance `genesis_prefill.rs:603-615` is the same code as `src/genesis_streaming.rs:727-751` (`signed_permanent_stream`), and `base()`/`row()`/`limits()` at `genesis_prefill.rs:28-68` mirror `src/genesis_streaming.rs:630-679`. Unit and integration tests cannot share today; if a `#[doc(hidden)] pub mod testing` is added to arkiv-node (the pattern `arkiv_db_storage::testing` and `arkiv_db_redb::testing` already use, see `tests/simulator.rs:10`, `tests/simulator_crash.rs:9`), these belong there.

#### D3. `install_adopt` / `install_copy` re-implement the binary's `adopt_host` / `create_host` - needs-care
`genesis_adopt.rs:187-241` (`fn install_adopt(bundle: &Path, run: &Path, genesis: &SignedGenesis, stop: Stop) -> Option<Host>`, 55 l) repeats, call for call,
`src/bin/arkiv-proposer.rs:345-421` (`inspect` -> `prepare_*streaming` -> `check_rename` -> `Host::adopt_staged` -> `Host::check_adopted` -> `publish` -> `Host::open` -> set `prefill_file`);
`genesis_adopt.rs:153-175` (`install_copy`) repeats `src/bin/arkiv-proposer.rs:292-315`. The doc comments say so ("as the producer's `adopt_host` runs it", "as `create_host` runs it").
Consequence: the three "library" tests (`:300`, `:320`, `:372`) exercise the test's copy of the sequencing, not the producer's; the copy already differs (`3` threads at `:215` vs `genesis_adopt::verify_threads()`, no final `verify_stream_file`).
Fix: move the sequencing out of `src/bin` into `arkiv_node::genesis_adopt` (two public steps, e.g. `relabel(..)` and `publish_and_open(..)`, so a test can stop between them), call it from the binary and from the tests; delete `install_adopt`/`Stop`. Then phases 2 and 3 of the CLI test (`genesis_adopt.rs:583-613`, four extra process launches) can shrink to one resume check (see category 2).

#### D4. Simulator fixture spelled six times - safe, ~-40 lines net
`RunConfig { genesis: arkiv_node::demo::workload::genesis(9001, 1700000000000), workload: Workload::default() }` + `Host::create(store, run, [1; 16], [7; 16], options)`:
`tests/simulator.rs:24-32` (`run()`, `host(store)`), `tests/simulator_http.rs:42-54` (`host()`) and again inline `:713-726` (only to pass a `Retention` option),
`tests/simulator_crash.rs:54-64`, `examples/inspect_eq.rs:36-45`, `examples/verify_inspection.rs:124-134` (the bin spells it once more at `src/bin/arkiv-simulator.rs:191-195`).
The "group = 1" `EqRequest` literal: `tests/simulator.rs:33-50` (`fn request(host: &Host, height: u64, limit: u32) -> EqRequest`) and inline `tests/simulator_crash.rs:112-126`.
"Light that follows `h`" (`Light::create(store, h.durable_identity.clone())` + collect headers `1..=n` + `append`): `simulator_http.rs:322-323+349-350`, `:534-537+548-549`, `:677-686`, `:730-732+769-770`, `simulator.rs:282-287`.
Fix: `tests/simulator_support/mod.rs` with `run()`, `host_on(store, options)`, `produced(n) -> Host`, `light_of(&Host) -> Light`, `group_one(&Host, height, limit) -> EqRequest`; `simulator_http.rs:713-726` becomes `host_on(store, Options { retention: .. })`.
For the two examples, a library constructor (`RunConfig::development(chain_id)`, used by the bin too) removes the last three copies and the bare `1700000000000` (written `1_700_000_000_000` in `simulator.rs:26` and the bin, without separators in the other five places).

#### D5. `struct Server` + `async fn serve(router) -> Server` three times - safe, -30 lines
`tests/simulator_http.rs:27-41`, `tests/signed_http.rs:39-53`, `tests/signed_reads.rs:156-170`: identical (abort-on-drop axum server on `127.0.0.1:0`). Home: the same shared HTTP test module as D1.

#### D6. Repeats inside `tests/genesis_prefill.rs` - safe, ~-45 lines
- `fn host(g, stream, producer: bool, state: Arc<MemoryVersionedStore>, history: Arc<MemoryVersionedStore>) -> Host` (72-90): five of six call sites pass two fresh `Arc::new(MemoryVersionedStore::new())` (252-258, 291-297, 336-342, 578-584, 867-873); only `:335` keeps the stores (restart). Make `host(g, stream, producer)` create the stores and keep `host_on(.., state, history)` for `:335`. -20 lines.
- Sponsor `*signer(2).address().as_bytes()` is written 11 times (`rg -c` = 11) and `build_genesis(base(), limits(), <sponsor>, &rows)` is spelled out raw at 142, 155-161, 186, 189, 196-202, 206, 209 next to the `build()` helper (69-71) that unwraps. Add `fn sponsor() -> [u8; 20]` and `fn try_build(base: SignedGenesis, limits: PrefillLimits, rows: &[PrefillEntity]) -> Result<SignedGenesis, String>`; `build` = `try_build(base(), limits(), rows).unwrap()`. -15 lines and the rejection cases become one line each.
- "read entity by key": 370-380, 436-448 and the `live` closure 703-716 are the same `try_get_record(arkiv::entity_namespace(), key, &Projection::ALL, Budget::unlimited()).unwrap().unwrap().value` chain (`tests/support/mod.rs:199-210` `record()` is the same helper for a numeric namespace). One `fn entity(snapshot: &Snapshot, key: &RecordKey) -> Option<Row>`. -15 lines.
- `try_typed_record_count(arkiv::entity_namespace())` assert blocks at 259-265, 464-471, 472-480, 809-816: `fn entities(&Snapshot) -> usize`.

#### D7. Repeats inside `tests/demo_auth.rs` - safe, ~-30 lines
- `Node::follower(Role::Full, trust.clone(), SignedHeader::sign(trust.genesis_hash, trust.genesis.header(), &key), 1000, "unused".into()).unwrap()` at 188-195 and 199-206 (identical), third variant 350-351. One closure/helper.
- `TxEnvelope { chain_id: 7, actor: workload::ALICE, request_id, tx_budget: 1000, ops: vec![CreateNamespace("queue")] }` at 405-413 and 418-426; a `tx` closure already exists in the previous test (269-275). Hoist one `fn tx(id: [u8; 32], budget: u64, op: TxOp) -> TxEnvelope` to file level.
- 402-404 hand-builds "24 zero bytes then n big-endian", which is `support::request(n)` (`tests/support/mod.rs:102-106`).

#### D8. Repeats inside `tests/simulator_http.rs` - safe, ~-40 lines
- 234-255: three 7-line asserts differing only in the key; `let n = |k: &str| diagnostics[k].as_str().unwrap().parse::<u64>().unwrap();` then `assert!(n("proofBytes") > 0); n("fetchMs"); n("verifyMs");` (-16).
- 165-171 and 174-180: the same authenticated `POST /sim/v1/control`; `request()` (68-80) lacks a token parameter. Add `Option<&str>` token to `request` (-8).
- 489-499 and 505-517: same wait-for-status loop -> `wait_until` from D1 (-15).

#### D9. `signer` / `hex` / `address` inventory (for the shared-support design)
| file | helper | lines | signature / body | note |
|---|---|---|---|---|
| genesis_prefill.rs | `signer` | 22-24 | `fn signer(n: u8) -> Signer` = `Signer::from_bytes([n; 32]).unwrap()` | identical to `signed_support::signer` (`tests/signed_support/mod.rs:28-30`); file does not `mod signed_support` |
| genesis_adopt.rs | `signer` | 57-59 | `fn signer() -> Signer` = `[1; 32]` | = `signed_support::signer(1)` |
| prebuilt_genesis_cli.rs | `signer` | 41-43 | same as genesis_adopt | |
| genesis_adopt.rs | `hex` | 60-62 | `fn hex(bytes: &[u8]) -> String`, no `0x` | identical to `tests/support/mod.rs:318-320` and `tests/signed_protocol.rs:123-125`; used at `:64` and `:551` |
| prebuilt_genesis_cli.rs | `hex` | 47-49 | same | used once, only to build `address()` |
| genesis_adopt.rs / prebuilt_genesis_cli.rs | `address` | 63-65 / 44-46 | `fn address() -> String` = `"0x" + hex(signer().address())` | equals the public `arkiv_node::simulator::wire::hex(signer().address().as_bytes())` (0x-prefixed, `src/demo/wire.rs:14-22`); `tests/signed_retention.rs:133` has a third `address()` |
The other `signer` variants in the crate differ subtly and are outside this chunk: `signed_retention.rs:27` and `signed_http.rs:55` set only `key[31] = who`; `signed_pool.rs:11` takes `u16`.
Other local helpers that are file-specific (not generic): genesis_prefill `base` 28-42, `limits` 43-49, `row(n, height)` 50-68, `build` 69-71, `account_proof` 91-106, `permanent_row` 484-486, `build_permanent` 487-496, `entity_tx` 497-515, `permanent_chain_rows` 654-656;
genesis_adopt `bundle` 123-136 (returns a 4-tuple `(PathBuf, PathBuf, SignedGenesis, Value)`; `src/genesis_prebuilt.rs:450-455` uses a `Built` struct for the same data), `history` 138-147, `certificate` 148-150, `reopen` 243-252, `assert_same` 254-297;
prebuilt `run` 103-105, `stream_build` 106-142; simulator_http `query` 55-57, `get` 58-67, `request` 68-80, `height` 81-87, `mutate` 299-315; simulator `control` 51-60; simulator_crash `store` 17-26, `options` 27-32, `step` 33-42, `open` 43-48; demo_auth `fixture` 12-23, `header_chain` 24-31.

#### D10. "Otherwise complete command line" tables - safe, ~-35 lines
`genesis_adopt.rs:657-681` has a `follower(role)` closure building the full/light argument list; `prebuilt_genesis_cli.rs:368-409` writes the same two lists out by hand (42 lines). One `cli_support::follower_args(role, run_dir, genesis_hash, chain_id, run_id, listen) -> Vec<String>`.

### II.1 [S5] arkiv-db-trie

*(paths relative to `crates/arkiv-db-trie/`)*

#### Tests

**1.1 Closed-interval shims copy-pasted into three test files (biggest single item, ~115 lines)**
- `tests/interval_model.rs:6-49`, `tests/interval_inspection.rs:7-50`, `tests/interval_security.rs:4-47`:
  44 byte-identical lines each - `fn walk_interval(root, lower, upper, nodes, max_entries)` and
  `fn inspect_interval(...)` that adapt an older signature onto `KeyRange::closed` +
  `IntervalLimits { max_entries, max_key_len: 256 }`, each under `#[allow(dead_code)]` and the comment
  "The closed-interval form these tests were written against."
- In `interval_security.rs` the `inspect_interval` shim is never called
  (`rg -n inspect_interval tests/interval_security.rs` -> only :25 and :38, the definition), so the
  `#[allow(dead_code)]` hides real dead code; the same file then defines a third wrapper, the
  `bounded` closure at :59-69, which takes the `max_key_len` the shim fixes at 256.
- Fix: one pair in `tests/support/mod.rs`, e.g.
  `pub fn walk_closed<N: AsRef<[u8]>>(root: ObjectId, lower: &[u8], upper: &[u8], nodes: &[N], limits: IntervalLimits) -> Result<IntervalEntries, ProofError>`
  and `inspect_closed` with the same shape; add `mod support;` to the three files; delete the six
  `#[allow(dead_code)]`, the three history comments and the `bounded` closure. **safe**.

**1.2 "Build a map from entries" re-defined / inlined everywhere (~110 lines)**
- Identical: `tests/store.rs:45-54` and `tests/proofs.rs:37-46`
  `fn build<const N: usize>(entries: &BTreeMap<[u8; N], Vec<u8>>) -> Map<N>`, plus the identical alias
  `type Map<const N: usize> = MerkleMap<[u8; N], Vec<u8>>` (`store.rs:43`, `proofs.rs:32`).
- Same thing, different spelling: `tests/interval_inspection.rs:52-62`
  `fn map<const N: usize>(entries: impl IntoIterator<Item = ([u8; N], Vec<u8>)>) -> MerkleMap<[u8; N], Vec<u8>>` (fold);
  `tests/bounded_stage.rs:47-52` `fn one<V: TrieValue>(value: V) -> MerkleMap<[u8; 1], V>` (single entry);
  `tests/store.rs:671-680` `fn postings(ids: &BTreeSet<u64>) -> Postings`.
- Inlined `map = map.insert(&k, v, &MemoryOnly, &mut Probe::new()).unwrap().0;` loops (4-9 lines each
  after rustfmt): `rg -U -c '\.insert\([^;]*?\)\s*\.unwrap\(\)\s*\.0' tests` -> map.rs 9, variable_keys.rs 8,
  store.rs 7, bounded_stage.rs 5, proofs.rs 3, and 1 each in encoding/explore/interval_inspection/
  interval_model/sorted (37 total). Worst: `variable_keys.rs:268-276, 340-349, 359-368, 399-407, 457-465, 534-542`
  (the two folds at 340-349 and 359-368 are the same expression twice in one test);
  `bounded_stage.rs:194-200, 263-269, 313-324, 350-356, 392-403, 481-487`.
- Fix: in `tests/support/mod.rs` add
  `pub fn put<K: TrieKey, V: TrieValue>(map: MerkleMap<K, V>, key: &K, value: V) -> MerkleMap<K, V>` and
  `pub fn build<K: TrieKey, V: TrieValue>(entries: impl IntoIterator<Item = (K, V)>) -> MerkleMap<K, V>`;
  delete the five local helpers. **needs-care** (many call sites, no semantic change).

**1.3 Stage -> commit -> reopen sequence repeated (~25 lines)**
- `let mut staged = Staged::new(); map.stage(&mut staged); let mut store = MemoryNodeStore::new(); store.commit(&staged); let stored = Map::from_root(map.root_hash(), len)`
  at `tests/store.rs:290-295, 336-341, 414-427, 538-542, 611-615`, `tests/variable_keys.rs:468-480`,
  `tests/explore.rs:13-17`, `tests/bounded_stage.rs:357-361, 404-408`, `tests/sorted.rs:89-91, 214-216`,
  `src/reach.rs:80-83`, `src/map.rs:949-953`.
- Fix: `support::committed(&map) -> (MemoryNodeStore, MerkleMap<K, V>)` (stage, commit, `from_root`).
  `staged_ids` (`store.rs:56-58`) is re-implemented inline at `variable_keys.rs:472-477`; move it to support too. **safe**.

**1.4 Three raw-bytes `TrieKey` test types (~25 lines)**
- `tests/sorted.rs:15-28` `Raw` (MAX_LEN 2048, decode accepts anything),
  `tests/adversarial_bounds.rs:32-48` `Raw` (MAX_LEN 6, decode rejects `> MAX`),
  `tests/variable_keys.rs:517-528` `Loose` (MAX_LEN 8, decode accepts anything).
- Subtle difference: only the adversarial one bounds `decode`.
- Fix: `support::RawKey<const MAX: usize>(pub Vec<u8>)` with the bounded `decode`. **safe**.

**1.5 Helpers that already exist in `tests/support` but are re-written locally**
- `tests/adversarial_bounds.rs:68-73` `fn unpack` == `support::reference::unpack` (`tests/support/mod.rs:154-159`);
  the file already has `mod support;`. Delete the local one (6 lines). **safe**.
- `tests/interval_security.rs:150-158`: inline xorshift generator; `support::Rng` exists (file lacks `mod support;`). **safe**.
- `tests/adversarial_bounds.rs:254-270` (`strings` closure over `bytes`) and `:278-291` (inline loop over `nibbles`)
  are the same "all strings up to length n over an alphabet" loop; give `strings` an alphabet parameter (-13 lines). **safe**.
- `src/node.rs:515-550` `Rng` duplicates `tests/support/mod.rs:23-70` (SplitMix64, `below`, `bytes`, `some_bytes`).
  Forced by unit-vs-integration visibility; only fix is `#[path]`-including support. Noted, leave.

**1.6 Sink / nested-value test doubles**
- `TryObjectSink` doubles: `tests/sorted.rs:30-48` `Sink { staged, calls, fail_at: Option<usize> }`
  (`Error = StageError`, fails only on the exact call `fail_at`), `tests/bounded_stage.rs:77-103`
  `Sink { objects, calls, allowed_calls }` (`Error = local Error`, fails on every call past the limit),
  `src/sorted.rs:289-301` `Discard { calls }`. One `support::CountingSink { staged, calls, fail_from }` covers
  the two integration ones; note the "fail once" vs "fail from then on" difference when merging.
- Nested-map values: `tests/store.rs:643-667` `Postings(MerkleMap<u64, Vec<u8>>)` and
  `tests/bounded_stage.rs:216-234` `Nested<V>(MerkleMap<[u8; 1], V>)` have the same `encode`
  (root ‖ len BE) and `stage_nested`; `Nested::decode` always errors. Could share; low payoff. **needs-care**.

**1.7 `check_interval` twice**
- `tests/variable_keys.rs:163-209` and `tests/adversarial_bounds.rs:110-165` share their first ~18 lines
  (expected = model filtered by `range.contains`; `interval_proof(..).collect::<Result<_, TrieError>>().unwrap()`;
  `IntervalLimits`; `assert_eq!(walk_interval(..), Ok(expected))`), and `tests/interval_model.rs:59-102`
  repeats the tail assertions of the first (tight limit -> `EntryLimitExceeded`, each omitted node -> err,
  extra `vec![0xc0]` -> err). The proof-collect expression alone appears 7x
  (`rg -n -U 'interval_proof\([^;]*?collect' tests`). Fix: `support::interval_witness(map, range)` and
  `support::assert_complete_interval(root, range, proof, expected, limits)`. **safe**, ~25 lines.

**1.8 "Wrap payload in an RLP list header" three times in one test module**
- `src/node.rs:694-701` (`branch_with` closure), `:742-749` (`extension` block), `:853-860` (`reencode` tail).
  One `fn rlp_list(payload: &[u8]) -> Vec<u8>` in the test module (-12 lines). **safe**.

#### Source

**1.9 Two copies of the staging walk: `stage_link` (map.rs:851-887) vs `try_stage_link` (stage.rs:185-221)**
- 37 lines each, same structure line for line; the fallible one adds `budget.visit()?` and `?` on the sink.
  The walk that `stage.rs`'s module doc describes ("The staging walk (`MerkleMap::stage`) ...") lives in map.rs.
- Minimum fix: move `stage_link` into stage.rs beside its twin. **safe**.
- Better: delete `stage_link`; implement `stage`/`stage_with` on `try_stage_link` with a private adapter
  `struct Infallible<'a>(&'a mut dyn ObjectSink)` implementing `TryObjectSink<Error = StageError>` and a
  `StageBudget::new(u64::MAX)` (one `expect("unbounded budget, infallible sink")`). -30 lines.
  `tests/bounded_stage.rs::fallible_staging_matches_infallible_and_skips_committed_subtrees` then compares
  the walk with itself (keep it for the committed-subtree half). **needs-care [CONSENSUS]** (decides the staged object set).
- Related one-liner repeated 3x: `reference.as_hash().unwrap_or_else(|| keccak256(..))` at `map.rs:254-256`,
  `map.rs:868-870`, `stage.rs:204-206` (and the `match` form at `sorted.rs:219-227`). A
  `fn object_id(reference: &RlpNode) -> ObjectId` in node.rs would name it.

**1.10 Trace-node construction duplicated between the two verifiers (~25 lines)**
- `src/proof.rs:354-388` and `src/interval.rs:449-479`: the same `match &raw` producing
  `(kind, path, children, value_bytes)` for Leaf/Extension/Branch and the same `PathNode { .. }` literal;
  only `selected_nibble` and `depth` differ.
- Fix: `impl PathNode { fn from_raw(raw: &RawNode<'_>, bytes: &[u8], proof_index: usize, depth: usize, source: PathSource) -> Self }`
  in proof.rs (caller sets `selected_nibble`). **needs-care** (pure refactor; inspection output feeds RPC, covered by
  `tests/proofs.rs::inspected_paths_...` and `tests/interval_inspection.rs`).

**1.11 "Next supplied node" check duplicated (~10 lines)**
- `src/proof.rs:445-462` and `src/interval.rs:412-437`: `nodes.get(index).ok_or(Incomplete { index })`,
  `len() < 32 -> InvalidNode { "hash-referenced node shorter than 32 bytes" }`, `keccak256 != hash -> HashMismatch`.
  The reason string exists a third time at `src/map.rs:86`.
- Fix: `fn supplied<'a, N: AsRef<[u8]>>(nodes: &'a [N], index: usize, hash: B256) -> Result<&'a [u8], ProofError>`
  in proof.rs; the interval walk keeps its root special case around it. **needs-care [CONSENSUS]** (verifier accept set).

**1.12 `KeyRange::contains` (interval.rs:81-91) == `Nibbled::contains` (interval.rs:143-153)**
- Byte-identical 11-line bodies; `Nibbled` (interval.rs:120-123) has exactly `KeyRange`'s two fields.
- Fix: make `Nibbled` a newtype `struct Nibbled(KeyRange)` (so `contains` delegates), or one private
  `fn within(lower: &Bound<Vec<u8>>, upper: &Bound<Vec<u8>>, key: &[u8]) -> bool`. -11 lines. **safe**.

**1.13 Shape checks re-implemented in the verifiers and `explore` instead of `check_leaf_end` / `check_inner_end`**
- `map.rs:103-125` defines them (returning `TrieError::Inconsistent`). Re-written inline with their own
  strings at `interval.rs:491-496` (same two leaf strings), `interval.rs:515-517` ("extension reaches the key
  length bound"), `interval.rs:542-544` ("branch reaches ..."), `proof.rs:395-397`, and
  `explore.rs:78-83, 112-116` ("explored leaf has wrong key length", "explored path exceeds key length").
  So one violation has three wordings ("inner node reaches" / "extension reaches" / "branch reaches").
- Fix: let the two helpers return `Result<(), &'static str>`; loader side wraps with
  `TrieError::inconsistent`, verifier side with `invalid(..)`; explore calls them. -10 lines.
  **needs-care**: `tests/store.rs:896,915,942,953,958` assert the exact strings (they do not leave the crate:
  arkiv-db collapses trie `ProofError`s to `InvalidProof`, `crates/arkiv-db/src/proof/range.rs:342-348`).

**1.14 `SortedTrieBuilder::push` and `finish` contain the same seal loop (sorted.rs:171-178 vs 211-217)**
- `finish` is `push`'s loop with the floor at 0. With `floor = common + 1` (push) / `0` (finish):
  start = `branches.last().map_or(floor, |b| (b.depth + 1).max(floor))`, loop while `b.depth >= floor`.
- Fix: `fn seal_down_to(&mut self, floor: usize, sink: &mut S) -> Result<RlpNode, SortedTrieError<S::Error>>`
  used by both; inside it `while let Some(branch) = self.branches.pop_if(|b| b.depth >= floor)` also removes
  `.expect("branch was checked")` (sorted.rs:174; `Vec::pop_if` is stable since 1.86, MSRV is 1.90). -10 lines.
  **needs-care [CONSENSUS]** (builds roots; pinned by `tests/sorted.rs` object-set equality).

**1.15 Interval trace bookkeeping repeated inside `walk_interval_inner`**
- `if let Some(node) = trace.as_deref_mut().and_then(|t| t.nodes.last_mut()) { .. }` three times
  (`interval.rs:505-512, 520-529, 551-560`); the two decision blocks are identical
  (`if open { Open } else { Outside }`). Fix: `fn record_decision(trace: Option<&mut VerifiedInterval>, slot: usize, open: bool)`. -14 lines. **safe**.

**1.16 Smaller**
- `src/node.rs:219-224` and `:491-496`: the same `Arc::new(NodeBox { retained: measure(&node), node, rlp: OnceLock::from(rlp), from_store: true })`.
  Add `fn stored(node: Node<V>, rlp: RlpNode) -> Arc<Self>` next to `NodeBox::new`. **safe**.
- `src/node.rs:286-296` / `:298-308` `encode_leaf` / `encode_extension` share the first 7 lines
  (temp path Vec, header, path string); a private `encode_short_header(path, leaf, second_len, out)` removes
  ~8 lines. Low payoff. **needs-care [CONSENSUS]**.
- "Is this node an object of its own" predicate written four times: `proof.rs:107`, `interval.rs:241`
  (`role == Role::Root || node.reference().is_hash()`), `map.rs:865`, `stage.rs:201` (`root || reference.is_hash()`).
- `src/explore.rs:84` `n.chunks_exact(2).map(|c| c[0] * 16 + c[1])` re-implements `nibbles::pack` (nibbles.rs:20). **safe**.
- `tests/support/mod.rs:181-187` and `:255-261`: same "shared prefix of first and last leaf" block in
  `reference::node` and `reference::proof`.

### II.1 [N1] arkiv-node signed JSON-RPC / HTTP reads

*(paths relative to `crates/arkiv-node/`)*

#### D1. Hex / quantity / param parsing: 6 encoders-decoders re-implemented, no single home (emphasis a)

Inventory (every definition I found with `rg -n "from_str_radix|strip_prefix\(\"0x\"\)|0123456789abcdef|fn (hex|unhex|number|uint|bytes|quantity)\b" crates/arkiv-node`):

| Job | Implementations |
|---|---|
| bytes -> `"0x.."` | `src/demo/wire.rs:14-23` `hex` (the real one); `src/simulator/wire.rs:13-15` `hex` (delegates to demo); `src/signed/rpc.rs:122-124` `hex` (delegates to the delegate: 3 hops); `src/genesis_streaming.rs:597` inline `format!("{b:02x}")` loop; `src/signed/state_api.rs:32-37` `nibbles` == `src/simulator/inspection.rs:19-24` `nibbles` (byte-identical); tests: `tests/support/mod.rs:318`, `tests/signed_protocol.rs:123`, `tests/genesis_adopt.rs:60`, `tests/prebuilt_genesis_cli.rs:47` |
| `"0x.."` -> bytes | `src/demo/wire.rs:24-43` `unhex(s, cap)` (lower-case only); `src/simulator/wire.rs:16-18` wrapper (error mapping only); `src/value_json.rs:124-139` `hex_bytes` (same nibble match as demo/wire.rs:29-35, no cap); `src/signed/rpc.rs:135-149` `bytes(v, len)` (mixed case, per-byte `u8::from_str_radix`); `src/genesis_authoring.rs:334-351` `fixed_hex::<N>` + `hex_digit`; `src/simulator/query_language/literal.rs:336` `decode_hex` + `hex_nibble`; `src/signed/rpc_reads.rs:602-606` inline "0x + 64 hexdigits" check; tests: `tests/signed_protocol.rs:126` `unhex::<N>`, `tests/ethereum_transactions.rs:14` `bytes` (uses `alloy_primitives::hex::decode`) |
| u64 -> `"0x{:x}"` | `src/signed/rpc.rs:125-127` `quantity` (39 uses); inline `format!("0x{..:x}")` at `rpc_reads.rs:826, 828, 1200, 1240, 1241, 1242`, `rpc_ws.rs:142, 160, 224, 228`; U256 variant inline at `rpc.rs:218, 221, 222, 226, 527, 970` (`{:#x}`) and `rpc_reads.rs:1201` |
| `"0x.."` -> u64 | `rpc.rs:128-134` `uint`; `rpc_reads.rs:486-501` `quantity` (also takes JSON numbers); `rpc_ws.rs:132-136` inline; `rpc.rs:799-811` inline U256; tests: `tests/ethereum_transactions.rs:439-440` closure `quantity`, `:461-468` inline |
| decimal text -> u64 | `src/demo/wire.rs:53-61` `number`; `src/simulator/wire.rs:19-21` wrapper; `src/genesis_authoring.rs:322` `number<T>` |
| 20-byte address | `rpc.rs:150-156` `actor(&Value)`; `src/signed/runtime.rs:487-493` `actor(&str)` |

`src/bin/arkiv-proposer.rs` is clean here: it imports `wire::{hash, hex, number, unhex}` (line 22), no local copies.

The copies are not equivalent, which is the actual cost:
- `rpc.rs:128` `uint` has no hex-digit check, and `u64::from_str_radix` accepts a leading `+`: `"0x+1"` parses as 1 (so `eth_getBlockByNumber("0x+1")`, `fromBlock:"0x+1"`, filter ids, `gas` all accept it). `rpc_reads.rs:486` `quantity` checks `is_ascii_hexdigit` and does not. No test covers it (`malformed_hex_and_log_ranges_are_rejected_without_panics` only tests `bytes`).
- `rpc.rs:799-811` (`value` of a call) does no leading-zero check at all; `uint` does.
- `rpc.rs:135` `bytes` accepts upper-case hex (needed: clients send EIP-55 addresses); `demo/wire.rs:24` `unhex` and `value_json.rs:124` reject it.
- Same name, opposite direction: `rpc::quantity` formats, `rpc_reads::quantity` parses.

Fix (single home): a crate-level `src/hex.rs` (today the real `hex` lives in `demo/wire.rs`, which `simulator` and `signed` reach backwards into):
`hex(&[u8]) -> String` (one line: `alloy_primitives::hex::encode_prefixed`, already a dependency and already used at `tests/ethereum_transactions.rs:15`, `src/genesis_bundle.rs:750`), `unhex_lower(&str, cap) -> Option<Vec<u8>>`, `unhex_any_case(&str, cap) -> Option<Vec<u8>>`, `quantity(u64) -> String`, `quantity_u256(U256) -> String`, `parse_quantity(&str) -> Option<u64>` (strict: `0x`, >=1 hex digit, no leading zero, digits only). Each caller maps `None` to its own error. Then:
- delete `rpc.rs:122-124` and import `hex`; switch `tests/signed_host.rs:950, 1079` (the only outside users of `rpc::hex`) to it;
- `rpc.rs` keeps only the JSON-param layer (`uint`, `bytes`, `actor`, `at`, `args`) as 2-3 line wrappers; `rpc_reads.rs:486-501` becomes `value.as_u64().or_else(|| parse_quantity(value.as_str()?))`, renamed `parse_quantity`/`uint` so the name no longer collides with the formatter;
- replace the 10 inline `format!("0x{..:x}")` and 7 inline U256 formats listed above;
- `value_json.rs:124-139`, `state_api.rs:32-37` (import the `inspection` one or move both to `hex.rs`) go away.
Risk: **needs-care** for the mechanical part (keep `unhex_any_case` behind `rpc::bytes`; do not merge strictness). Making `uint` strict is **behaviour-affecting** `[CONSENSUS]` (rejects `"0x+1"`, which is malformed and currently accepted). ~45 lines removed in this chunk, ~25 outside it.

#### D2. `metrics.rs`: 31 seven-line `gauge(`/`counter(` calls, then the output is re-parsed to fix what the calls got wrong (emphasis f)

- `metrics.rs:215-479` `render` is 263 lines (clippy: 263/100), of which 217 are 21 `gauge(` + 10 `counter(` calls, each spread by rustfmt over 7 lines because of the 5 positional args `(&mut out, name, help, labels, value)`.
- `metrics.rs:85-90` `gauge` and `:91-96` `counter` are the same function except for one word in the format string; `counter` takes `u64` instead of `impl Display`, which is the only reason `:328-332` hand-writes a third copy of the HELP/TYPE/sample triple for an `f64` counter.
- Because `gauge`/`counter` emit `# HELP`/`# TYPE` on every call, the per-database loop (`:344-477`) emits each header once per database and `timings` (`:97-122`) interleaves `_errors_total` with histogram lines. `group_families` (`:481-520`, 40 lines) then re-parses the whole text to dedupe headers and regroup samples, with `unwrap`/`expect` on its own output (`:489, 498, 508, 510`). The test re-implements the same parse a third time (`:574-590`).

Fix: collect families instead of text. `struct Families(BTreeMap<&'static str, Family>)` with `Family { help: &'static str, kind: &'static str, samples: Vec<String> }` and one method `fn sample(&mut self, kind, name, help, labels: &str, value: impl Display)` (plus a `histogram` helper for `timings`); `render` writes the map once at the end. `BTreeMap` order equals today's (`group_families` already sorts by family name), so the body is unchanged. Group the homogeneous calls into arrays so each metric is 1-5 lines: e.g. the five `DataStats` gauges (`:395-429`, all `u64`), the four activity counters (`:300-327`), the commit counters (`:456-476`):
`for (name, help, value) in [("db_stored_bytes", "...", data.stored_bytes), ...] { f.gauge(name, help, &labels, value) }`.
Removes `group_families`, the hand-written triple at `:328-333`, all four unwraps: ~90-100 lines. Risk: **needs-care** (`/metrics` text is scraped; `cached_scrape_preserves_sample_on_failure_and_declares_families_once` pins contiguity and single declaration).

#### D3. `rpc_reads.rs` parser: the same error construction and lookups written out many times

- `rpc_reads.rs:1372-1384` vs `:1410-1419` in `leaf`: the "an attribute name" and "a block height that fits in u64" error closures are written twice (BETWEEN branch and the general branch).
- `rpc_reads.rs:1394-1400, 1420-1437`: the comparison operator is carried as an integer `0..=5` between two `match`es (`Ast::Eq => (key, value, 0)` ... `0 if neq => Term::not_eq`, `_ => match v { Value::Str .. }`). Fix both at once: two local closures `attr(&key)` / `val(&key, &value)` and one `match ast { Ast::Between{..} => .., Ast::Eq{..} if neq => .., Ast::Gt{..} => Term::range(n, Bound::Excluded(v), Bound::Unbounded), .. }` that builds the `Term` directly. Evaluate `attr` before `val` to keep the error precedence. ~71 lines -> ~40. **safe** (errors identical if the order is kept).
- `QueryError::at(QueryErrorKind::X, self.source, pos, "..")` is spelled out as a 6-line call 11 times inside `impl Parser` (`:1558, 1573, 1588, 1597, 1613, 1623, 1700, 1722, 1746, 1758, 1769`) although `parse_error` (`:1519-1526`) already wraps the one case "Parse at current position". Add `fn error_at(&self, kind, position, message) -> QueryError`; each site becomes 1-3 lines. ~35 lines. **safe**.
- `TAGS.iter().find(|(s, _)| s.eq_ignore_ascii_case(&tag)).map(|(_, t)| *t)` at `:1583-1586` and `:1717-1720` -> `fn tag_named(&str) -> Option<TypeTag>`. The two error messages beside them (`:1592-1593`, `:1726-1727`) each hand-list "bool, i32, u64, u256, dec, bytes32, str, addr or key", a third and fourth copy of `TAGS` (`:1441-1451`); `scalar` (`:1196-1211`) is a fifth, in the other direction. All nine names equal `arkiv_db::TypeTag::name()` (`crates/arkiv-db/src/record.rs:321`), so `TAGS` can be `[TypeTag; 9]` and `scalar` can return `value.tag().name()`. **needs-care** `[CONSENSUS]`: couples the SDK wire names to arkiv-db's; only do it if that coupling is wanted.
- `std::result::Result<_, QueryError>` is written in full 12 times (`:1273-1686`) while `type Read<T>` exists for the other error. Add `type Parsed<T> = std::result::Result<T, QueryError>;`. **safe**.
- Attribute -> JSON closure three times: `:1217-1221` (`verified_attributes`), `:1249-1253` (`attributeSchema`), `:1260-1262` (`attributes`). One `fn attribute_json(name, cell, with: Fields)` or at least share the `scalar(&c.value).map(..)` shape. ~10 lines. **safe**.
- `rpc_reads.rs:512` `R::OutOfBudget => limits("the read budget is exhausted").into()` duplicates `exhausted()` (`:383-385`). Use it. **safe**.
- The select key list appears three times: `FIELDS` (`:86-96`), `plan_entity`'s literal (`:608-612`), test `sdk_select` (`:1803`) and again inline at `:2082` and `:2794`.

#### D4. `rpc.rs`: small blocks repeated 3-11 times

- **Record fetch + project** three times: `rpc.rs:272-287` (`previous_owner`), `:316-328` (`ExpiryReplay::expiry`), `:646-657` (`revert_data`), each `try_get_record(entity_namespace(), key, &Projection::ALL, Budget::limit(25_000_000))? .map_err(StorageCorrupt)? .value .map(project) ..`. One `fn entity(snapshot: &Snapshot, key: &RecordKey) -> Result<Option<ArkivEntityProjection>>`; the three callers become one-liners. `25_000_000` (x3) is `rpc_reads.rs:72` `READ_BUDGET`. ~25 lines. **safe**.
- **Expiry target** `e.expires_at.max(height.saturating_add(e.min_lifetime))` five times: `rpc.rs:632, 672, 678, 714, 751` (and the checked form at `:338-342`); the same formula is in `src/exec.rs:369-372` and `src/arkiv/mod.rs:342-346`. Add `impl Expiry { pub fn target(&self, height: u64) -> u64 }` at `src/block.rs:494`. **safe** for the five saturating copies; leave exec/arkiv (consensus path) alone.
- **Block-tag resolution** twice: `rpc.rs:111-121` `filter_height` and `:157-173` `at` both match `"latest" | "pending" | "safe" | "finalized"` / `"earliest"` / `uint`. `at` = `filter_height` + "not above head". Make `at` call a shared `block_tag(head, v)`. In `at`, `uint(v.ok_or(Error("InvalidParams"))?)` (`:166`) cannot fail (a `None` took the `"latest"` arm); in `filter_height` the `s.len() <= 18` guard (`:117`) is redundant with `uint`'s leading-zero rule plus `from_str_radix` overflow. ~10 lines. **safe**.
- **Op selection for a call** twice with different error codes: `rpc.rs:812-822` (`simulation`) and `ethereum.rs:319-332` (`make_envelope`): "to == ARKIV_ADDRESS ? (value must be 0, decode ops) : (no calldata, Transfer)". See S-list item on the diverging codes. One `ethereum::ops_for(to, value, input) -> Result<Vec<TxOp>>`. **behaviour-affecting** `[CONSENSUS]` (error message of `eth_call`/`eth_estimateGas` changes).
- **"string or array of strings, case-insensitive"** twice in `matches_log`: `rpc.rs:541-548` (address) and `:562-570` (topic, also tolerating `null`). ~8 lines with a helper `one_of(selector, actual)`. **needs-care** (keep the null rule topic-only).
- **JSON-RPC envelopes** hand-built at `rpc.rs:1279, 1295, 1312, 1373, 1418, 1472` and `rpc_ws.rs:45, 92, 168`; notifications three times at `rpc_ws.rs:234, 246, 252`. `rpc_ws.rs:44-46` `failure` already is the error helper: move it to `rpc.rs`, add `success(id, result)` and (ws) `notification(subscription, field, value)`. **safe** (same bodies).
- `a.first().ok_or(Error("InvalidParams"))?` 11 times in `dispatch` (`rpc.rs:966-1235`); `Error("InvalidParams")` 51 times in `rpc.rs` non-test code and 21 in `rpc_reads.rs`. `const INVALID_PARAMS: Error = Error("InvalidParams");` (as `ethereum.rs:44` `bad()` does for its code) and `fn first(a: &[Value]) -> Result<&Value>`. **safe**.
- **Response cap 8 MiB** as four separate literals in this chunk: `rpc.rs:1339`, `rpc.rs:1425` (`CAP`), `rpc_ws.rs:17` (`RESPONSE_BYTES`), `rpc_reads.rs:73` (`RESPONSE_CAP`), while `simulator::wire::RESPONSE_CAP` (`src/simulator/wire.rs:8`) is public and already imported by `range_api.rs:7`. Request cap 1 MiB: `rpc.rs:1460` and `rpc_ws.rs:16`. `reads.rs:94-95` `MAX_GROUPS = 64`/`MAX_LITERALS = 256` restate `rpc_reads.rs:78-79` `GROUP_CAP`/`LITERAL_CAP` ("the SDK query parser's own bounds"). One constant each. **safe**.
- `rpc.rs:29` `ADDRESS` is `arkiv::ARKIV_ADDRESS` (`src/arkiv/mod.rs:242`) retyped as a string; use `hex(&arkiv::ARKIV_ADDRESS)` at its two uses (`:213`, `:475`). **safe**.

#### D5. WebSocket batch loop re-implements `rpc::handle_request`'s with different limits

`rpc_ws.rs:190-205` vs `rpc.rs:1426-1446`: cap 32 (`rpc_ws.rs:19`) vs 64 (`rpc.rs:1427`); size counted with `response.to_string().len()` vs `serde_json::to_vec(..).len() + 1`; overflow closes the socket (`rpc_ws.rs:200` `return`) vs answers `ResponseTooLarge`. The loop itself must stay (subscribe/unsubscribe are connection-local) but the three differences look accidental. Aligning them is **behaviour-affecting** `[CONSENSUS]`; at least share the cap constant.

#### D6. Test scaffolding (for the shared support module)

Unit tests cannot use `tests/signed_support/mod.rs` (`signer(byte)`, `genesis(chain_id, timestamp_ms, run_id, proposer, block_period_ms, funded)`), so each module rebuilt it. A `#[cfg(test)] pub(crate) mod testing` under `src/signed/` would hold one host builder.

Four copies of "signer -> SignedGenesis -> SignedHeader::sign -> History::create(MemoryVersionedStore) -> Host::create":
- `rpc.rs:1503-1533` `fn host_with(expiry_per_block: u32) -> Host` (31 lines; chain 9300, signer `[2;32]`, `SignedGenesis::new`, produces block 1) + `:1500-1502` `fn host() -> Host`
- `rpc_reads.rs:2410-2441` `fn bare_fixture() -> Host` (32; chain 9515, signer `[1;32]`, struct literal with `limits: Default::default(), prefill: None`)
- `rpc_reads.rs:1876-1961` `fn fixture_with(period_ms: u64, extra: Vec<Vec<&str>>) -> Host` (86; same genesis literal as `bare_fixture` plus prefill rows, `max_page = 2`) + `:1868-1873` `fixture()`, `fixture_with_period(period_ms: u64)`
- `rpc_ws.rs:280-308` `fn runtime() -> Shared` (29; chain 9405, signer `[1;32]`, wraps in `Runtime::new`)
Subtle differences: `SignedGenesis::new(..)` (rpc.rs, rpc_ws.rs) vs struct literal (rpc_reads.rs); only `host_with` produces a block.

Other local helpers that look generic:
- `rpc.rs:1534-1557` `fn native(host: &Host, nonce: u64, ops: Vec<ArkivOperation>) -> SignedTx` (request id = nonce in bytes 0..8) vs `rpc_reads.rs:2605-2633` `fn submit(host: &mut Host, nonce: &mut u64, ops: Vec<arkiv::ArkivOperation>)` (request id = `0x66..` + nonce in bytes 24..32; also submits and produces every 4th). Same "wrap ops in `TxOp::Arkiv`, sign" core.
- `rpc.rs:1836-1892` `fn signed_transfer(signer: &Signer, nonce: u64, to: OwnerId, value: U256) -> Value` / `fn signed_transfer_with_gas(.., gas: u64, typed: bool) -> Value` (54 lines of hand-rolled RLP; no second copy found: `rg "sign_digest" tests src`).
- `rpc.rs:2108-2124` `fn create_filter_event(h: &mut Host, nonce: u64)`.
- `rpc_reads.rs:1792-1810` `bounds(&Host) -> QueryLimits`, `entity_count(&Host, &Json) -> Read<Json>`, `query(&Host, &Json) -> Read<Json>`, `sdk_select(Json) -> Json`, `kind(Read<Json>) -> QueryErrorKind`; `:2008` `get_entity(&Host, &Json) -> Read<Json>`; `:2634-2647` `create(salt: u128, attributes: Vec<(&str, arkiv_db::Cell)>) -> ArkivOperation`; `:2648-2653` `delete(key: &Json) -> ArkivOperation`; `:2655-2662` `keys(page: &Json) -> Vec<Json>`; `:2665-2680` `walk(host: &Host, source: &str, options: Json) -> (Vec<Json>, Vec<Json>)`.

Inside the test modules:
- `rpc_reads.rs:2515-2553` and `:2569-2586` (`a_page_past_the_response_cap_is_shortened_and_its_cursor_continues`) hand-build the `TxEnvelope` and the cursor walk that `submit`/`create` (`:2605-2647`) and `walk` (`:2665`) provide (same budget, lifetime, produce-every-4th). Use them: ~35 lines. **safe**.
- "nine clauses" builder `std::iter::repeat_n("$createdAt = u64(0)", 8).chain([..]).collect::<Vec<_>>().join(" AND ")` four times: `rpc_reads.rs:2184-2187, 2727-2732, 2826-2828, 2921-2924`; "explosive" builder twice: `:1834-1836, 2351-2353`. One `fn nine(last: &str) -> String`. ~15 lines.
- `rpc.rs` tests: `Runtime::new(Node::Full(Box::new(h)), None, None, None).unwrap()` six times (`:1702, 1947, 2037, 2052, 2218, 2234`); `Signer::from_bytes([N; 32]).unwrap()` 17 times; the `ArkivOperation::Create { salt, flags: 0, expiry: Expiry { expires_at: 0, min_lifetime: 20 }, attributes: vec![] }` literal five times (`:1563, 1614, 1649, 1684, 2112`). Helpers `runtime(h)`, `signer(n)`, `create(salt, min_lifetime)`: ~40 lines.

### II.1 [S4] arkiv-db-redb tests and examples

*(paths relative to `crates/arkiv-db-redb/`)*

#### 1.1 Layout: tests/X.rs + tests/common/X.rs  (emphasis a)

Every pair, and who else uses the common half:

| tests/common/X.rs (lines) | thin test file (lines, tests) | other users |
| --- | --- | --- |
| crash.rs (457) | atomicity.rs (180, 9 tests: 8 of 3-10 lines + one 37-line) | relabel.rs (`SEEDS, assert_integrity, config, modes`), examples/redb_experiment.rs |
| creation.rs (131) | initialization.rs (112, 4 tests) | redb_experiment.rs |
| disk_full.rs (160) | disk_full.rs (112, 3 tests) | redb_experiment.rs |
| isolation.rs (299) | isolation.rs (112, 1 test) | redb_experiment.rs |
| lost_sync.rs (69) | reconciliation.rs (248, 5 tests, 1 uses it) | redb_experiment.rs |
| recovery.rs (121) | recovery.rs (49, 1 test) | redb_experiment.rs |
| amplification.rs (93) | write_amplification.rs (53, 1 test) | redb_experiment.rs |

Verdict: the hop is **not** "shared across backends" (every module is hard-wired to
`RedbStore`/`CrashDisk`). It is justified by exactly one second consumer,
`examples/redb_experiment.rs:17` (`#[path = "../tests/common/mod.rs"] mod common;`), which re-runs
the same sweeps at release scale for docs/redb-experiment.md. That is a legitimate reason; keep the
split, but the scheme is applied inconsistently (1.3) and costs a blanket `#![allow(dead_code)]` (4.1).

#### 1.2 `common::commit` takes the batch by value: 39 forced `.clone()`s
- tests/common/mod.rs:176 `pub fn commit(store: &dyn VersionedStore, batch: CommitBatch)` only does
  `store.commit(&batch)` (clippy: "passed by value, but not consumed"). 39 of ~60 call sites clone a
  whole batch just to call it (`rg -n 'commit\(&?[a-z_.()]+, [a-z_]+\.clone\(\)\)' crates/arkiv-db-redb/tests crates/arkiv-db-redb/examples`),
  including inside the crash-point loops (crash.rs:269 clones 12 parents per crash point, crash.rs:290
  the 1,600-object growing candidate).
- Fix: `pub fn commit(store: &dyn VersionedStore, batch: &CommitBatch)`; drop the 39 `.clone()`s, add `&`
  at the ~20 by-value sites. **needs-care** (60 call sites, mechanical). Also removes the 3 clippy
  "redundant clone" hits that are really this (disk_full.rs:76).

#### 1.3 examples/redb_experiment.rs re-implements four test bodies instead of sharing them (~170 lines)
Seven report sections call into tests/common; these four are hand copies that have already drifted:
- `reconciliation()` redb_experiment.rs:401-449 = tests/reconciliation.rs:84-147
  (`io_errors_at_every_commit_call_reconcile_without_re_execution`). Its chain (403-412) is
  `lost_sync::chain()` **minus** the `(SpaceId::Host, 2, 64)` entry, and line 450 then calls
  `lost_sync::chain()` for the second half of the same section.
- `history()` 323-399 = tests/history.rs:72-127 (same `published: Arc<RwLock<Vec<CommitBatch>>>`,
  `done`, `verified`, 4 scoped `SplitMix` readers).
- `durability()` 244-268 = tests/durability.rs:28-65 loop; `latching()` 466-484 = tests/latching.rs:53-103 in small.
- Fix: move the two sweeps into `tests/common/reconcile.rs` (`pub fn io_error_sweep() -> Counts`) and
  `tests/common/history.rs` (`pub fn concurrent_history(blocks) -> Verified`) like the other seven, and
  have both the test and the example call them; or drop the four sections from the report (the tests
  already assert them). **needs-care**. ~120 lines net.

#### 1.4 Fault-injection sweep scaffolding copied 4x (store level) + 1x (engine level)
"Capture the mutating trace of one complete commit, then for each call k fail the k-th occurrence":
- tests/reconciliation.rs:88-100, tests/disk_full.rs:79-91, examples/redb_experiment.rs:419-431,
  tests/engine_crashes.rs:125-137. The line
  `let skip = trace[..k].iter().filter(|earlier| *earlier == op).count() as u64;` is verbatim in the first
  three (reconciliation.rs:97, disk_full.rs:88, redb_experiment.rs:428) and the same expression with
  `position` for `k` at engine_crashes.rs:133-136.
- Trace capture alone also at tests/common/crash.rs:288-292, tests/relabel.rs:175-180,
  tests/atomicity.rs:117-122 (this one counts with `disk.counters().mutating_calls` instead of
  `mutating_trace().len()`; two ways to get the same number).
- Fix, in tests/common/mod.rs:
  `pub fn commit_trace(image: &[u8], config: RedbConfig, batch: &CommitBatch) -> Vec<DiskOp>` and
  `pub fn fail_call(disk: &CrashDisk, trace: &[DiskOp], k: usize, kind: io::ErrorKind)`.
  **safe**. ~30 lines.

#### 1.5 "Parent image" built 7x
`CrashDisk::new()` -> `crash_store` -> commit batches -> `disk.crash(store, CrashMode::DurableOnly)`:
tests/common/crash.rs:97-105 (`parent_image`), tests/reconciliation.rs:74-81 (`parent_image`, same name,
different signature), tests/common/disk_full.rs:79-84, tests/disk_full.rs:73-78,
tests/common/lost_sync.rs:41-46, tests/relabel.rs:170-173, examples/redb_experiment.rs:413-418.
Fix: one `common::durable_image(config: RedbConfig, batches: &[CommitBatch]) -> Vec<u8>`. **safe**. ~25 lines.

#### 1.6 `chain()` defined twice with identical contents
tests/reconciliation.rs:59-72 and tests/common/lost_sync.rs:16-29 build byte-identical batches
(`block(None,0,1,&[(Namespace(1),30,200)])` + candidate `[(Namespace(1),40,200),(Namespace(2),20,700),(Host,2,64)]`);
only the return shape differs (`(Vec<CommitBatch>, CommitBatch)` vs `(CommitBatch, CommitBatch)`), and
reconciliation.rs:195 already calls `lost_sync::chain()` as well. Fix: delete the local one, use
`let (genesis, candidate) = lost_sync::chain();`. **safe**. 14 lines.

#### 1.7 Chain-building loop written 11x; `isolation::Chain` is the private twelfth
`let mut parent = None; for height in .. { let batch = block(parent, height, ..); parent = Some(batch.new_head); .. }`:
crash.rs:29-46 and 54-72 (`parents`/`steady_parents`, same shape), durability.rs:36-53 and 87-94,
history.rs:47-54 and 113-119, recovery.rs (common):62-73 and 76-82, amplification.rs:63-79,
redb_experiment.rs:248-253 and 360-374. tests/common/isolation.rs:90-113 has a private
`struct Chain { parent, height }` doing the same with a workaround (see 4.3).
Fix: one `pub struct Chain { parent: Option<Head>, height: u64 }` in common/mod.rs with
`fn next(&mut self, objects: &[(SpaceId, u64, usize)]) -> CommitBatch`. **safe**. ~30 lines.

#### 1.8 Crash-mode lists built 3x
tests/common/crash.rs:123-136 (`modes(seeds)`: seeds `s` sector, `s+1000` byte),
tests/common/creation.rs:16-29 (`modes()`: 8 seeds, `s` sector, `s+500` byte),
tests/common/crash.rs:433-443 (inline in `run_spilling`: `7+s`, alternating granularity).
Fix: creation.rs calls `crash::modes(8)`; `run_spilling` gets a `fn alternating_modes(seeds)` next to
`modes`. **needs-care** (changes which torn subsets creation samples; seeds are arbitrary). ~14 lines.

#### 1.9 Config helpers that are the same value
- `RedbConfig { cache_size: 1 << 20, quick_repair }`: crash.rs:20-25 (`config`), recovery.rs (common):57-60,
  lost_sync.rs:36-39 (spelled `..with_cache_size(1 << 20)`); reconciliation.rs:55-57 is `crash::config(true)`.
- `RedbConfig::with_cache_size(64 << 10)`: creation.rs:12-14 and disk_full.rs (common):12-14.
- 9 functions named `config()`/`uncached()`/`growing_config()`/`redb_config()` across the tree
  (`rg -n 'fn (config|uncached|growing_config|redb_config)\(' crates/arkiv-db-redb/tests crates/arkiv-db-redb/examples`).
Fix: `common::config(cache_size: usize, quick_repair: bool)`; delete the five equal helpers. **safe**. ~15 lines.

#### 1.10 Small repeated assertion idioms -> helpers in common/mod.rs
- "retry is refused by the head check"
  `assert!(matches!(store.commit(b), CommitOutcome::NotCommitted(e) if e.kind == StorageErrorKind::HeadMismatch))`:
  crash.rs:217-220, disk_full.rs (common):154-157, reconciliation.rs:116-119, 222-225.
  -> `assert_already_committed(store, batch)`.
- "power cut now" `crash_store(&CrashDisk::from_image(disk.crash_image(CrashMode::DurableOnly)), config)`:
  reconciliation.rs:163-166, 182-185, disk_full.rs (common):143-146, durability.rs:56-57,
  lost_sync.rs:62-63, redb_experiment.rs:254-257. -> `power_cut(&disk, config) -> RedbStore`.
- "find the object's bytes in the image and flip a bit": latching.rs:222-227, bounded_reads.rs:59-63
  (offset 300 vs 500). -> `damage_object(&mut image, &bytes)`.
- "write into the store's `meta` table through plain redb": latching.rs:307-318, bounded_reads.rs:85-94.
  -> `write_meta(path, key, value)`.
- `RedbStore::with_backend(move || Ok(disk.backend()), config)` hand-rolled although `crash_store` exists:
  creation.rs:46-47, 57-58, 89; lost_sync.rs:62-63; initialization.rs:64-67; engine_support/mod.rs:38-41.
  Three of them need the `Result`: add `try_crash_store(..) -> Result<RedbStore, StorageError>` and let
  `crash_store` be `try_crash_store(..).expect(..)`.
- `fn file_len(path) -> u64`: isolation.rs (common):121-123 = recovery.rs (common):40-42; inlined again at
  amplification.rs:88.
All **safe**. ~45 lines.

#### 1.11 Crash-point selection closure 3x (+1 variant)
tests/atomicity.rs:100-105 and 154-159 are identical except the stride (61 vs 97);
examples/redb_experiment.rs:216-223 is the 97 one again; tests/engine_crashes.rs:51-61 is the same idea
(`k % 16`, neighbours of a non-`Write`). Fix: `crash::sampled(stride: u64) -> impl Fn(u64, &[DiskOp]) -> bool`
in tests/common/crash.rs. **safe**. ~14 lines.

#### 1.12 tests/engine_crashes.rs: "power cut, reopen the engine" 4x
Lines 82-85, 161-164, 191-194, 229-231 (`disk.crash((engine, store), DurableOnly)` ->
`CrashDisk::from_image` -> `disk_store` -> `DurableEngine::open(.., genesis(), options(0))`).
Fix: `fn reopen(image: Vec<u8>) -> (Arc<RedbStore>, DurableEngine)` beside `image()`/`open()`. **safe**. ~9 lines.

#### 1.13 Helpers duplicated with other crates  (emphasis b)
- `head(height: u64, fork: u8) -> Head`: tests/common/mod.rs:116-120 (keccak of height||fork, `hash[0] = fork`)
  vs `crates/arkiv-db-storage/src/testing/contract.rs:100-104` (pub; `[fork; 32]` with the height in the
  first 8 bytes). Same signature and purpose, different bytes; nothing in this chunk depends on the
  derivation (relabel.rs:38, 133 only need distinct heads). Fix: `pub use arkiv_db_storage::testing::contract::head;`
  in common/mod.rs and delete 116-120. **needs-care** (changes `BlockIndex` keys, so table shapes in the
  measurement tests; the assertions there are on A's table and the directory, not on `BlockIndex`).
  Two more private `head` fixtures exist in unit tests: crates/arkiv-db-storage/src/versioned.rs:1148,
  crates/arkiv-db-memory/src/versioned.rs:301.
- `object(..)`: common/mod.rs:97-113 (`tag: u64, n, len` -> exactly `len` bytes) vs contract.rs:107-117
  (`factory, tag: &str, n, len` -> about `len`): same xorshift (13/7/17) and constant
  `0x9e37_79b9_7f4a_7c15`. `block(..)` common/mod.rs:132-173 vs private `block_batch` contract.rs:147-177;
  `commit` mod.rs:176 vs `committed` contract.rs:119. Two parallel fixture sets; if a shared
  `arkiv_db_storage::testing::fixtures` is created, `head` + `object(id_fn, ..)` + the Blocks/Manifests/BlockIndex
  record triple belong there. Left as a note; only `head` is a drop-in today.
- Engine fixtures, third copy: tests/engine_support/mod.rs:14-27, 42-47, 112-115
  (`OWNER = OwnerId::new([7; 20])`, `ns`, `name`, `key`, `genesis`, `initialize`, `publish`) =
  crates/arkiv-db/tests/durable.rs:11-36 = crates/arkiv-db/tests/support/durable_history.rs:10-28.
  Subtle differences: genesis seed `[0x71; 32]` vs `[9; 32]`; `initialize(store, cache_bytes: usize)` vs
  `initialize(store, options: Options)`; `publish` here uses `try_seal` and asserts
  `Publication::Committed(info)` equals `prepare`'s info, durable.rs:30 uses `seal()` + `matches!`.
- `fn committed(i: Initialization) -> DurableEngine`: tests/streaming_import.rs:10-15 =
  crates/arkiv-db/tests/streaming_import.rs:9-14 = crates/arkiv-db/tests/adopt.rs:20; and the same match
  again in engine_support/mod.rs:43-46 and examples/durable_throughput.rs:125-128.
  Since tests/engine_history.rs:5 already `#[path]`-includes `../../arkiv-db/tests/support/durable_history.rs`,
  a `crates/arkiv-db/tests/support/engine.rs` (ns/name/key/initialize(options)/publish/committed) included
  the same way is the consistent single home.

#### 1.14 Duplication among the six examples  (emphasis d)
- xorshift64* `Rng`: bench_readers.rs:90-104 = key_size_report.rs:42-53. SplitMix64 `Keys`:
  durable_throughput.rs:37-46 is `arkiv_db_redb::testing::SplitMix` (src/testing.rs:465-486, same
  constants; `Keys(seed).next(bound)` = `SplitMix::new(seed).below(bound)`), which tests/history.rs:24
  and redb_experiment.rs:32 already import. Fix for durable_throughput: delete `Keys`. **safe**. 10 lines.
- Engine bootstrap (create store -> `DurableEngine::initialize` -> match `Committed` -> create one
  namespace by hand -> seal/prepare/commit): bench_readers.rs:187-224 = key_size_report.rs:185-217
  (verbatim modulo names). `commit_block` bench_readers.rs:154-172 = `commit` key_size_report.rs:146-164.
  Both files are deliberately self-contained (bench_readers is copied into old revisions by
  scripts/bench-compare.sh:304-311; key_size_report's doc line 5-7 says the same), so this copy is
  only removable by deleting key_size_report (5.1).
- Report header (date, rustc, uname, CPU model from /proc/cpuinfo, file system): redb_experiment.rs:90-123 +
  141-163 vs engine_storage_experiment.rs:25-29 + 512-527 (`command` unwraps where `command_output`
  falls back to "unknown"). `"redb 4.3.0"` is hard-coded in both (redb_experiment.rs:155,
  engine_storage_experiment.rs:520); the pin lives in the workspace Cargo.toml:31 (`redb = "=4.3.0"`).
- Churn workload: reachability_report.rs:53-58 + 79-82 = engine_storage_experiment.rs:77-86.
- Timed create/patch loops in durable_throughput.rs:133-147 and 151-169 are the same loop with a
  different body.
- `percentile` bench_readers.rs:174-180 = `common::quantile` mod.rs:237-244 (kept: self-contained).
- Argument handling, four conventions: flags with values (bench_readers.rs:52-87), flags where every
  flag must have a value and there is no `--help` (key_size_report.rs:171-181), flags + positional with
  `expect` (reachability_report.rs:88-105), env vars `PERF_*` (durable_throughput.rs:29-34, 112-119),
  `--quick` (redb_experiment.rs:130). Not worth a shared parser; listed so the env-var one can be
  aligned if durable_throughput is kept.
- Markdown tables: header line, rule line and positional `{}` row are kept in sync by hand in 12 tables
  (redb_experiment.rs:181-184, 290-295, 515-520, 562-567; engine_storage_experiment.rs:163-165, 176-178,
  185-187, 204-206, 294-296, 378-380, 490-492, 499-501). I counted header cells against row cells in all
  12: they match today.

#### 1.15 tests/contract.rs: five factories with the same `object_id`
Lines 49-126: `RedbFile`, `RedbCrashDisk`, `WrappedRedb`, `Memory`, `WrappedMemory` each repeat
`fn object_id(&self, bytes) -> ObjectId { keccak(bytes) }`. After 2.1 three remain; replace with
`struct Keccak(fn() -> TestStore); impl StoreFactory for Keccak { .. (self.0)() }` and
`versioned_store_contract!(super::Keccak(super::file_store))`. **safe**. ~25 lines.

#### 1.16 Hand-written delegating store (known pattern; location only)
tests/engine_process.rs:35-86 `BoundaryStore` (52 lines; `ObjectReader` 2 methods + `VersionedStore`
8 methods, only `commit` at 70-76 is not pure delegation).

### II.1 [NT1] arkiv-node tests: core

*(`T/` = `crates/arkiv-node/tests/`)*

#### 1.0 Answer to "which support helpers are re-defined locally" (emphasis a)

`T/support/mod.rs` is used by 8 files (`rg -l "^mod support;" T/` -> identity, replay, selection, exec, validation, genesis, block_format, block_model). accounts.rs and gas_execution.rs are signed-chain tests and do not use it (gas_execution.rs uses `T/signed_support/mod.rs`; accounts.rs uses neither).

| helper | shared definition | local re-definitions | difference |
|---|---|---|---|
| `until` | `T/support/mod.rs:190-195` | `T/block_model.rs:1746-1751` | byte-identical `const fn`. block_model.rs imports support selectively (`:55-57`) and simply left `until` out. |
| `signer` | `T/signed_support/mod.rs:28-30` `signer(byte: u8)` | `T/accounts.rs:21-23` `signer(n: u8)`; `T/gas_execution.rs:18-20` `signer()`; `T/gas_execution.rs:824` inline `Signer::from_bytes([2; 32]).unwrap()` | accounts.rs copy is identical; gas_execution.rs `signer()` is `signed_support::signer(1)` and line 824 is `signed_support::signer(2)` (the file already calls `signed_support::signer(n)` at :359, :398, :449, :932, :1016, :1077, :1153). |
| `genesis` | `T/support/mod.rs:73-75` | `T/block_model.rs:1947-1949` | not a copy: `genesis_with(PARAMS, LIMITS, FIXTURE.schedule())` with the file's own `PARAMS`/`LIMITS` (`:75`, `:94`), which shadow the support constants only because of the selective import. Fine, but rename the local consts (`MODEL_PARAMS`, `MODEL_LIMITS`) so a reader does not assume the support ones. `T/accounts.rs:27-40` and `T/gas_execution.rs:21-23` return `SignedGenesis` (different type). |
| `receipt` | none | `T/replay.rs:37-46` `fn receipt(schedule_id: u32, spent: u64, outcome: Outcome) -> Receipt`; `T/block_format.rs:172-181` `fn receipt(spent: u64, outcome: Outcome) -> Receipt`; `T/block_model.rs:532-541` `Model::receipt(&self, spent: u64, outcome: Outcome)`; `T/exec.rs:463-470` inline literal | all four are `Receipt { kind: Write, model_version: 3, schedule_id, spent: Cost::new(spent), outcome, ledger: None }`; they differ only in where `schedule_id` comes from (param / hard-coded 1 / `self.in_force.id` / literal 2). |
| `request` | `T/support/mod.rs:102-106` (24 zero bytes + n BE) | no fn re-definition. Inline variants: `T/accounts.rs:68-69` (`[who; 32]` then nonce BE in the last 8 - fill byte is the signer, not 0); `T/gas_execution.rs:71`, `:735` (`[nonce as u8; 32]`), `:512` `[1; 32]`, `:865` `[3; 32]`; `T/block_model.rs:1708` `[0; 32]` (== `request(0)`) | three different request-id schemes across the two signed files. `T/block_model.rs:1507-1511` `Generator::request` wraps support's, fine. |
| `ns` | `T/support/mod.rs:77-79` | no fn re-definition. `T/accounts.rs:645` `let ns = arkiv_db::NamespaceId::from_ordinal(2).unwrap()`; `T/gas_execution.rs:727` `NamespaceId::from_bytes(2u64.to_be_bytes()).unwrap()`, `:874` same with 999 | two different constructors for the same id 2 in sibling files. |
| `hex` | `T/support/mod.rs:318-320` | none in this chunk. `T/gas_execution.rs:1062` calls `arkiv_node::simulator::wire::hex`. | support's `hex` re-implements `alloy_primitives::hex::encode` (alloy-primitives is already a dev-dependency, `crates/arkiv-node/Cargo.toml` `[dev-dependencies]`). The lib itself has three pub copies (`src/demo/wire.rs:14`, `src/signed/rpc.rs:122`, `src/simulator/wire.rs:13`) - other chunks. |
| `name` | `T/support/mod.rs:85-87` | `T/block_model.rs:1194` closure `let name = \|text\| CellName::new(text).unwrap()`, `:406-408` `expires_at()`; plus 7 inline `CellName::new(..).unwrap()` in block_model.rs, 8 in gas_execution.rs, 2 in accounts.rs | same body. |

Unused support helpers hidden by `#![allow(dead_code)]` (`T/support/mod.rs:6`): see 4.1.

#### 1.1 Findings, biggest first

1. `T/gas_execution.rs:320-331`, `:912-919`, `:1101-1108`, `:1170-1177` - four hand-written `SignedGenesis::new(base.genesis, base.run_id, base.proposer, base.block_period_ms, <allocations>).unwrap()` rebuilds whose only purpose is to swap the allocations. `SignedGenesis::new` is a struct literal plus `validate()` (`crates/arkiv-node/src/signed/protocol.rs:148-166`) and every field is pub; `T/accounts.rs:219-222` and `:487-488` already do the same job as `g.accounts = AccountConfig::new(..).unwrap();`. Fix: one local `fn genesis_funding(extra: impl IntoIterator<Item = (OwnerId, U256)>) -> SignedGenesis` (proposer funded with `signed_support::balance()`, as all three custom sites do at :326, :1099, :1165), and `capped` (:909-920) becomes `signed_support::genesis_from(native, [8; 16], 1, 5000, &[1,2,3,4,5,6,7,8,9])` after setting `native.params.max_new_storage_units_per_block`. ~25 lines. **safe**.

2. `T/gas_execution.rs:64-84` `tx_by`, `:728-744` (closure `submit`), `:859-871`, `:509-522` - the same `SignedTx::sign(&genesis, nonce, TxEnvelope { chain_id: 9200, actor, request_id: [nonce as u8; 32], tx_budget, ops }, by)` written four times, because `tx_by` only accepts `Vec<ArkivOperation>` and unwraps. Fix: `fn sign(g: &SignedGenesis, by: &Signer, nonce: u64, budget: u64, ops: Vec<TxOp>) -> arkiv_node::signed::Result<SignedTx>`; `tx_by` maps the Arkiv ops and unwraps it; the closure body becomes 3 lines; :859-871 becomes `sign(..).unwrap_err()`. ~30 lines. **safe**.

3. `T/block_model.rs:117-150` (struct), `:153-186` (`FIXTURE`), `:194-226` (`schedule`), `:230-265` (`uniform`) and `T/support/block_roots.rs:79-111` (`schedule`) - the 31 weight fields are listed five times; the lists at block_model.rs:194-226 and block_roots.rs:79-111 are the same fields in the same order. Fix: `fn in_order(&self) -> [u64; 31]` on `Weights`; `schedule()` = `PriceSchedule::new(self.id, self.in_order())`; block_roots iterates `w.in_order()`. ~33 lines, and one fewer place to get the class order wrong. **safe** (the order is checked by every seeded block: a wrong order changes receipts).

4. `T/accounts.rs:41-66` (`create_host`, `host`) and `:134-141` (`relay`) duplicate `T/signed_host.rs:30-53` (`create`, `memory`) and `T/signed_host.rs:152-165` (`relay`; the accounts.rs one is a subset - it omits the block and feed equality). `T/gas_execution.rs:27-39` `host_of` is the third copy of the same `Host::create(state, History::create(..), g, [3; 16], cert, producer.then(..), Options::default())`, and :640-649 and `T/accounts.rs:513-522` write it out inline again. Single home: `T/signed_support/mod.rs` - `pub fn host(g: &SignedGenesis, state, history, producer: bool) -> Host`, `pub fn memory_host(g, producer)`, `pub fn relay(producer, full, light)`. ~45 lines across accounts.rs + gas_execution.rs. **needs-care** (touches signed_host.rs, another chunk).

5. `T/exec.rs:192-220` and `:241-269` - identical 28-line setup (engine with `LIMITS`, two namespaces, one host-created record carrying a `$expiresAt` cell, publish, begin a block-budget branch). They differ in the cell and in `key(b"h")` (:210) vs `RecordKey::new(b"h").expect("key")` (:259). Fix: `fn branch_over_host_record(expires_at: Cell) -> Branch`. ~27 lines. **safe**.

6. `Receipt` literal x4 (table above). Single home `T/support/mod.rs`: `pub fn write_receipt(schedule_id: u32, spent: u64, outcome: Outcome) -> Receipt`. Removes `model_version: 3` from four places. ~25 lines. **safe**; `[CONSENSUS]` only in the sense that the receipts feed golden comparisons - the helper must build the identical value.

7. `T/replay.rs` - the block-1 literal `fixture.produce(txs(vec![tx(ALICE, 1, 1_000, vec![create_namespace("entities"), create_namespace("alpha")])]))` appears at :259-264, :324-329, :1022-1027, :1176-1181 (identical, which is why the four golden constants `GOLDEN_ROLLBACK_1` :314, `GOLDEN_TRANSFER_1` :391, `GOLDEN_TOO_FAR_1` :1056, `GOLDEN_INVALID_1` :1291 are the same hash `3910dca5...7667`), and with other params/budgets at :972-977, :1310-1315, :1350-1355. Fix: `fn two_namespaces(budget: u64) -> BlockInputs` and one `const GOLDEN_TWO_NAMESPACES`. ~25 lines. **needs-care** `[CONSENSUS]`: inputs must stay byte-identical or the goldens move.

8. `T/replay.rs:402`, `:730`, `:799`, `:875`, `:978`, `:1344` - `let tag = || vec![("tag", attr(Value::Bool(true)))];` defined six times. Fix: one module-level `fn tag() -> Vec<(&'static str, Cell)>`. 5 lines. **safe**.

9. `T/block_model.rs:2105-2133` - four `Node::follower(Role::X, trust.clone(), signed.clone(), 202, "unused".into()).unwrap()` (29 lines). Fix: `let follower = |role| Node::follower(role, trust.clone(), signed.clone(), 202, "unused".into()).unwrap();` and `[follower(Role::Full), follower(Role::Full)]`. ~22 lines. **safe**.

10. `EqRequest` on `group == U64(1)` built three times: `T/gas_execution.rs:677-691`, `:796-810`, `T/block_model.rs:2179-2193`. In gas_execution.rs both are "head of this host": `fn group_eq(h: &Host, namespace: NamespaceId) -> EqRequest`. ~14 lines there. **safe**.

11. `T/gas_execution.rs:488-493`, `:956-962`, `:1126-1132`, `:1214-1220` - `block.outcomes.txs.iter().all(|t| t.status == TxStatus::Committed)` wrapped in a 6-line `assert!` four times. Fix: `fn assert_all_committed(block: &SignedBlock)`. ~18 lines. Also `:1064-1068` and `:1135-1139` (dropped status + reason tuple) twice. **safe**.

12. `T/gas_execution.rs:398-407` vs `:449-458` and `:414-420` vs `:462-468` - the two rent tests (`rent_the_sender_cannot_pay_at_execution_...` :397, `the_producer_drops_a_transaction_...` :448) open with the same 17 lines (tight signer, entity key, dry run of create + grow, rents, host + follower, the two submits). Fix: `fn create_then_grow(tight: &Signer) -> (RecordKey, [(U256, u64); 2])` and `fn submit_create_then_grow(h: &mut Host, tight: &Signer, entity: &RecordKey)`. ~15 lines. **safe**.

13. `T/block_model.rs:493-503` `charged(&OpOutcome) -> u64` is line-for-line `OpOutcome::spent()` (`crates/arkiv-node/src/block.rs:1211-1219`), which the sibling tests already use (`T/gas_execution.rs:171`, `T/accounts.rs:470`). It reads a receipt the oracle itself predicted, so no independence is lost. Fix: delete it, call `outcome.spent()` at :1124. 12 lines. **safe**.

14. `T/gas_execution.rs:219-228` - `at_cap` builds `create(9, 1024, 0)` and then patches the expiry through an `if let`; `create(salt, body, lifetime)` (:41-60) already sets `Expiry { expires_at: 0, min_lifetime: lifetime }`, so this is exactly `let at_cap = create(9, 1024, cap);`. 9 lines. **safe**.

15. `T/block_model.rs:1513-1521` `Generator::envelope` and `:1705-1711` (`trial`) are `support::tx(actor, n, budget, ops)` (`T/support/mod.rs:108-116`) written out; `T/support/block_roots.rs:174-185` rebuilds the `'D'`/`'T'` host keys that `support::due_key` / `total_key` (`T/support/mod.rs:301-316`, already imported by the parent at block_model.rs:56) provide. ~16 lines. **safe**.

16. Small generic helpers with more than one copy (single home `T/support/mod.rs`):
    - `fn spec() -> ChainSpec { ChainSpec::new(genesis()).expect("valid genesis") }` - `T/validation.rs:15-17` and `T/exec.rs:23-25` identical; inline at `T/identity.rs:254`, `T/block_format.rs:289`, `T/block_model.rs:1954`; `T/replay.rs:96-98` `chain_of(&Genesis) -> MemoryChain` is the next step up.
    - `fn id(ordinal: u64) -> RecordId` - `T/replay.rs:84-86` (`.expect("nonzero")`) and `T/block_format.rs:556-558` (`.unwrap()`); `RecordId::from_ordinal(..).unwrap()` inline 4x in block_model.rs (:627, :886, :950, :1025).
    - `B256::new([byte; 32])` - `T/block_format.rs:188-190` `hash(byte)` and `T/identity.rs:23-25` `digest(byte)`: same body, two names; both can be `B256::repeat_byte(byte)` with no helper.
    - big-endian count bytes - `T/exec.rs:74-76` `count(u64) -> Vec<u8>`, `T/block_format.rs:38-40` `be64` (same body), `T/replay.rs:624` closure `count`, `:441-442` `two`/`one`, `:931` `one`, `:244-245`, `:820-821` inline. Note `T/block_model.rs:410-412` has an unrelated `count(usize) -> u64`.
    ~20 lines. **safe**.

17. `T/exec.rs:398-420` and `T/genesis.rs:54-69` build the same edge case (block budget `u64::MAX`, `expiry_per_block: 1`, `max_page: 1`, PointRead `2^63 - 20`, ResultRow `2^63`, "over" at `- 19`). One is the pure-function pin, the other the through-the-chain pin; keep both tests, share `fn edge_read_schedule() -> (ChainParams, Limits, PriceSchedule, PriceSchedule)` in support. ~12 lines. **safe**.

18. `T/block_format.rs` - the header byte layout `cat(&[&be32(5), &be64(CHAIN), &be64(height), parent, &be64(ts), inputs, outcomes, root])` is written three times (:256-265, :303-312, :695-704) and the retired-version list `[0u32, 1, 2, 3, 4, 6, u32::MAX]` three times (:210, :222, :269; `[0, 1, 2, 3, 4, 6]` again at `T/genesis.rs:308`). Optional: `fn header_bytes(..)` and `const OTHER_VERSIONS`. ~18 lines. **safe**; left to taste because golden vectors read well when literal.

#### 1.2 Locally defined generic helpers (inventory for a shared support design)

Native-chain files (already on `T/support`):

| file:lines | signature | note |
|---|---|---|
| `T/validation.rs:15-17` | `fn spec() -> ChainSpec` | == exec.rs:23-25 |
| `T/validation.rs:19-21` | `fn one(op: TxOp) -> TxEnvelope` | `tx(ALICE, 1, 1_000, vec![op])` |
| `T/validation.rs:23-28` | `fn cells(count: usize) -> Vec<(&'static str, arkiv_db::Cell)>` | |
| `T/exec.rs:27-32` | `fn branch(chain: &MemoryChain) -> Branch` | block-budget branch at head |
| `T/exec.rs:61-72` | `fn run(chain: &MemoryChain, branch: Branch, inputs: &BlockInputs) -> Result<(), ExecError>` | |
| `T/exec.rs:74-76` | `fn count(value: u64) -> Vec<u8>` | == block_format.rs `be64` |
| `T/exec.rs:78-83` | `fn put_host(branch: &mut Branch, key: [u8; 32], value: &[u8])` | |
| `T/identity.rs:14-21` | `fn identity(scope: Scope, actor: OwnerId, n: u64) -> RequestIdentity` | |
| `T/identity.rs:23-25` | `fn digest(byte: u8) -> B256` | == block_format.rs `hash` |
| `T/selection.rs:22-40` | `puts`, `put_tx_bytes`, `admins` | file-specific |
| `T/genesis.rs:13-15` | `fn spec(params: ChainParams, limits: Limits) -> Result<ChainSpec, GenesisError>` | same name as validation/exec `spec()`, different shape |
| `T/block_format.rs:34-98` | `be32`, `be64`, `lp`, `cat`, `seq`, `digest_of`, `golden`, `rejects`, `unknown_tag` | file-specific (independent byte builders) |
| `T/block_format.rs:172-181` | `fn receipt(spent: u64, outcome: Outcome) -> Receipt` | schedule 1 |
| `T/block_format.rs:188-190`, `:556-558` | `hash(byte)`, `id(ordinal)` | see 1.1 #16 |
| `T/replay.rs:37-94` | `receipt(schedule_id, spent, outcome)`, `applied(effect, spent)`, `rejected(reason, spent)`, `committed(spent, ops)`, `failed(op_index, spent, ops)`, `id(ordinal)`, `deletion(namespace, record, schedule_id, spent)` | `applied`/`rejected` hard-code schedule 1, which forces the literal at :766-774 |
| `T/replay.rs:96-166` | `chain_of`, `struct Fixture { new, produce, empty, snapshot, finish }`, `outcomes(txs)` | |
| `T/block_model.rs:406-412` | `expires_at() -> CellName`, `count(usize) -> u64` | |

Signed-chain files:

| file:lines | signature | note |
|---|---|---|
| `T/accounts.rs:21-23` | `fn signer(n: u8) -> Signer` | == signed_support::signer |
| `T/accounts.rs:24-26` | `fn glm(n: u64) -> U256` | `T/signed_protocol.rs:21` has `glm() -> U256` (no arg) |
| `T/accounts.rs:27-40` | `fn genesis() -> SignedGenesis` | chain 9300, 10 GLM + 2 GLM |
| `T/accounts.rs:41-66` | `create_host(g: &SignedGenesis, p: bool, state, history) -> Host`, `host(g, p) -> Host` | `p` = producer |
| `T/accounts.rs:67-83` | `fn tx(g: &SignedGenesis, who: u8, nonce: u64, budget: u64, ops: Vec<TxOp>) -> SignedTx` | request id `[who; 24] ++ nonce BE` |
| `T/accounts.rs:84-133` | `transfer(who, amount)`, `create(salt)`, `balance(h, n: u8)`, `proofs(h, l, n, height)` | |
| `T/accounts.rs:134-141` | `fn relay(p: &Host, f: &mut Host, l: &mut Light)` | subset of signed_host.rs:152 |
| `T/gas_execution.rs:18-39` | `signer()`, `genesis()`, `host(producer: bool)`, `host_of(g: SignedGenesis, producer: bool)` | |
| `T/gas_execution.rs:41-60` | `fn create(salt: u128, body: usize, lifetime: u64) -> ArkivOperation` | accounts.rs `create(salt)` (:90-112) builds the same two cells (`body` field, `group` attribute) as a `TxOp` |
| `T/gas_execution.rs:61-84` | `tx(h: &Host, nonce, budget, ops: Vec<ArkivOperation>)`, `tx_by(h, by: &Signer, nonce, budget, ops)` | request id `[nonce as u8; 32]` |
| `T/gas_execution.rs:85-144` | `key(nonce, salt)`, `key_of(by, nonce, salt)`, `clock(h)`, `balance(h, actor: OwnerId)`, `burnt(h)`, `permanent(salt, body)`, `grow(key, bytes)`, `cells(h, key)` | `balance(h, actor)` here vs `balance(h, n: u8)` in accounts.rs vs `signed_support::balance() -> U256` (a constant amount): three meanings of one name |
| `T/gas_execution.rs:318-356`, `:909-923` | `tight_host`, `dry_run`, `capped`, `create_bytes` | |

### II.1 [N4] arkiv-node deterministic core

*(`src/` = `crates/arkiv-node/src/`)*

**D1. `src/arkiv/tests.rs` - copy-pasted fixtures and literals (~140 lines).** safe (tests only)
- `Expiry { expires_at: 10, min_lifetime: 0 }` spelled out 8x (4 lines each): 46-49, 106-109, 182-185, 506-509,
  556-559, 632-635, 731-734, 1755-1758, although `fn at(expires_at: u64) -> Expiry` exists at 949. Move `at` to the
  top, use `at(10)`. -24.
- `ArkivOperation::Create { salt, flags: 0, expiry: <10>, attributes }` literal: 503-511, 553-561, 1752-1760 and two
  identical `create_with` closures 629-637 and 728-736. One `fn create_with(attributes: Vec<(CellName, Cell)>)`
  (and `create(flags)` 42-52 / `create_until(salt, expiry)` 941-948 expressed through it). -35.
- Entity key: `RecordKey::new(&derive_entity_key(live.chain_id, &live.caller, N, SALT)).unwrap()` at 1301, 1344,
  1419, 1483, 1542, 1620, 1685, 1700 (1685 and 1700 build the same key twice in one test) plus two identical
  closures `k` 1146-1148 and 1786-1788, while `fn key(c, nonce)` (53-55) hard-codes salt 7. Make it
  `key(c, nonce, salt)`. -14.
- Raw row read `b.try_get_record(entity_namespace(), k, &Projection::ALL, Budget::unlimited()).unwrap().unwrap().value`
  at 62-67 (`read`), 141-150, 221-231, 525-535, 957-966 (`units_of`), 1342-1351. One `fn row(b, k) -> Option<Row>`;
  `read`/`units_of` build on it. -30.
- Namespace bootstrap (`engine.begin` + `try_execute(CreateNamespace{"entities"})`): 19-29, 707-716, 867-877 (and
  824-833 with another name). `fn fresh(pricing, limits) -> (Engine, Branch)`; then `setup()` and `glm_permanent()`
  share it, and `system_cells_leave_room_for_every_user_attribute` (699) stops calling `setup()` only to steal the
  context (717 `let (_e, _b, c) = setup();`). -22.
- Index-count closure `b.try_count(entity_namespace(), &Query::term(Term::eq(..)), Budget::unlimited())...value`:
  566-575, 595-604, 1762-1770. One `fn count_eq(b, name, value) -> u64`. -18.
- `extend` closure 3x: 1155-1158, 1420-1423, 1810-1813; `ExtendExpiry {` literal 16x. One `fn extend(key, expiry)`. -8.
- Ten-value list duplicated: 267-278 inline vs `every_type()` 391-404. See T2.

**D2. `src/exec.rs:396-557` - three pass-through entry points around `execute_block_inner`.** needs-care
`execute_block_hooked` (455-484) and `build_block_hooked` (486-522) are the same 9-argument forward and differ only
in `Mode::Strict` / `Mode::Build`; `signed/host.rs:962-986` then repeats the 9-argument call twice just to pick one.
The observer closure type is written out 4x (433-438, 465-470, 503-508, 570-575). Fix in two steps:
1. Make `Mode` `pub(crate)`, rename `execute_block_inner` to `execute_block_with` (`pub(crate)`), delete
   `execute_block_hooked` and `build_block_hooked`; `signed/host.rs` passes
   `if expected.is_some() { Mode::Strict } else { Mode::Build }`, `signed/rpc.rs:881` passes `Mode::Strict`. Removes
   ~58 lines here, ~12 in `signed/host.rs`, and two of the four `#[allow(clippy::too_many_arguments)]` (455, 493).
2. The obvious context struct: `(spec, parent, height, timestamp_ms)` travel together through all six signatures and
   every caller (`replay.rs:209`, `simulator/host.rs:613`, `signed/host.rs:963/975`, `signed/rpc.rs:872/881`,
   `tests/exec.rs` 7x, `tests/simulator.rs:488`). `pub struct BlockEnv<'a> { spec: &'a ChainSpec, parent: &'a Header,
   height: u64, timestamp_ms: u64 }` gives `execute_block(env, branch, inputs)`,
   `execute_block_observed(env, branch, inputs, observer)`,
   `execute_block_with(env, branch, inputs, accounts, mode, after_tx, observer)` (7 args) and
   `estimate_transaction(env, branch, tx, accounts)`: the remaining two allows (425, 559) go.

**D3. `src/block.rs` - 19 hand-written `Encode`/`Decode` pairs; 8 are pure field sequences (~95 lines).**
needs-care [CONSENSUS], optional
Generatable: `ChainParams` 187-225, `AdminInput` 462-480, `Expiry` 503-519, `TxEnvelope` 858-880, `BlockInputs`
900-916, `ExpiryDeletion` 1031-1049, `BlockOutcomes` 1400-1420, `Block` 1442-1461 (plus `RequestIdentity`
`src/identity.rs:112-132` and `Inclusion` 148-168). `u32`/`u64` implement `Encode`/`Decode` with the same bytes as
`encoder.u32/u64` (`crates/arkiv-db/src/codec.rs:514-540`), so one
`codec_struct!(Ty { f1, f2, .. }, min = N)` emitting `encoder.put(&self.f)` / `f: decoder.get()?` in order is
byte-identical. Honest verdict: borderline. `crates/arkiv-db/src/codec.rs` is hand-written in exactly this style
(no codec macro anywhere in the workspace), every layout is pinned by `tests/block_format.rs`, and the enum impls
(tag tables + per-variant fields) are not worth generating. Do it only if the line count matters; otherwise leave.
The five `digest()` fns + `Header::hash` (253, 362, 456, 852, 894, 1394) are 3 lines each: a trait for them would
be an abstraction with no payoff. The real repetition there is in the docs, see C5.

**D4. `src/arkiv/schema.rs:131-177` re-implements `src/value_json.rs:45-93` (~40 lines).** needs-care
`format_decimal` (159-177) is `value_json::scaled_text(*bytes, 18)` (45-57) statement for statement.
`parse_decimal` (131-158) is `value_json::parse_scaled(input, 18, 256)` (62-93) except that it also accepts one
leading `+` (134). Fix: `format_decimal(b) = scaled_text(*b, 18)`; `parse_decimal` strips an optional `+` (and then
refuses a following `-`, to keep `"+-1"` invalid) and calls `parse_scaled(.., 18, 256).ok_or(ArkivFault::Value)`.
Pinned by `decimal_exact_extremes_and_sdk_names` and the SDK fixture test. A third parser of the same shape is
`src/genesis_authoring.rs:359 decimal_units` (other chunk). `src/gas.rs:288-298 glm(wei)` is the same
whole/fraction/trim formatter again at 18 places for `u128`: `scaled_text(U256::from(wei).to_be_bytes(), 18)`.

**D5. `src/arkiv/mod.rs:832-872 execute_batch` - a second transaction executor, used only by tests (41 lines).**
needs-care
`rg execute_batch crates conformance scripts` finds callers only in `src/arkiv/tests.rs` (132, 1379, 1397, 1650).
Its rollback block (858-867: `rollback_to`, rewrite `Applied` -> `RolledBack`, `resize(.., NotExecuted)`) is a copy of
`src/exec.rs:1026-1037` in `Executor::tx`, which is what production runs. Move it into `tests.rs` as a helper (or
rewrite the four tests on `exec::execute_block`); the library should export one transaction path.

**D6. `src/arkiv/mod.rs:635-654` vs `src/exec.rs:748-791` - the due-schedule state machine exists twice (~25 lines).**
needs-care [CONSENSUS]
Arkiv: two inline loops over `[due_key, total_key]` with `count`/`put_count` (318-335). Exec:
`Executor::schedule`/`unschedule` with `read_count`/`write_count` + `parse_count` (1278-1285). They differ subtly:
exec rejects a stored zero or wrong-length count as `ScheduleInconsistent(MalformedEntry)` and reports
`ScheduleFault::Overflow`/`Underflow`; arkiv accepts a stored zero and reports every problem as
`ExecError::Storage(Corrupt, "Arkiv committed state is malformed")`. Single home: free functions
`schedule(branch, height, namespace)` / `unschedule(..)` / `read_count` / `write_count` in `exec.rs` (or a small
`schedule.rs`), used by both. Only the error returned for already-corrupt state changes.

**D7. `src/accounts.rs` - debit and burn written twice (~18 lines).** safe [CONSENSUS] (pure refactor)
118-127 (`settle`) == 144-153 (`burn_from`): add to the burn counter, cap at `initial_supply()`, write `BURN_KEY`.
94-99 (`reserve`) == 138-143 (`burn_from`): read, `checked_sub -> InsufficientBalance`, `put`. Extract
`fn debit(&self, branch, actor, amount)` and `fn burn(&self, branch, amount)`; `reserve = debit(fee(budget))`,
`burn_from = debit + burn`, `settle = credit + burn`.

**D8. Hex: one encoder behind a 3-level forwarding chain, four lowercase decoders.** needs-care
Encode: `src/demo/wire.rs:14-23` (impl) <- `src/simulator/wire.rs:13-15` (forwards) <- `src/signed/rpc.rs:122-124`
(forwards to simulator); `src/value_json.rs:21` imports `crate::demo::wire::hex`, so the core JSON form depends on
the demo module. Decode (`0x`, even length, lowercase): `src/value_json.rs:124-139 hex_bytes` + `141-143 exact_hex<N>`,
`src/demo/wire.rs:24-40 unhex(s, cap)` (same nibble table), `src/genesis_authoring.rs:334-351 fixed_hex<N>` +
`hex_digit`, `src/signed/rpc.rs:135-149 bytes(v, len)` (`from_str_radix`, also accepts upper case). Single home: a
`pub(crate) mod hex` at crate root with `encode(&[u8]) -> String`, `decode(&str) -> Option<Vec<u8>>`,
`decode_array::<N>(&str) -> Option<[u8; N]>`; the wire modules keep only their cap check and error mapping (so no
response body changes). ~16 lines in this chunk, ~30 overall.

**D9. `src/arkiv/mod.rs:759-793` (`quote_tx`) rebuilds what `execute_operation` 492-567 builds (~20 lines).**
needs-care [CONSENSUS on the execute side]
Both construct the resulting cells: `create_cells(..)`; `$updatedAt` stamp + `set`; `$updatedAt` + `$expiresAt`.
A quote that must equal execution (`the_admission_quote_matches_execution`) should share the builder:
`fn patch_set(op, height, new_expiry) -> Vec<(CellName, Cell)>` used by both. Also the `units` closure is defined
identically at 577 and 708 (`|cells| crate::gas::storage_units(cells).map(|u| u.total)`): make it a fn.

**D10. Expiry resolution twice: `src/arkiv/mod.rs:341-353 expiry()` vs `src/exec.rs:368-386 resolve()`.**
needs-care [CONSENSUS]
Same `floor = height.checked_add(min_lifetime)` / `max(expires_at, floor)` / dead-on-arrival; `resolve` adds
`Permanent` and `TooFar`. Put the shared arithmetic on the type: `Expiry::resolve_at(&self, height) ->
Result<u64, ExpiryFault>` in `block.rs`. Subtle difference to preserve: at `height == u64::MAX` with a resolved
`u64::MAX`, exec returns `Permanent` (the `== u64::MAX` test comes first, 373) while arkiv returns `DeadOnArrival`
(347).

**D11. `src/value_json.rs:172-185` - the U256/I256 arms inline `canonical<T>` (146-149).** safe, -12
`TypeTag::U256 => Value::U256(canonical::<U256>(text)?.to_be_bytes()),` and the same for `I256`.

**D12. Integer-width helpers, three copies each.** safe, -12
- usize -> u32: `src/exec.rs:388-390 fn position(index)`, `src/validate.rs:167-169 fn index(position)` (same body,
  argument and fn names swapped), `src/identity.rs:88` closure. Keep one `pub(crate)` fn.
- usize -> u64 `expect("usize fits in u64")`: `src/params.rs:154-156 fn units`, `src/select.rs:73-75 fn wide`
  (identical), inline `src/exec.rs:855`.

**D13. Hand-copied constants.**
- Price-schedule encoded length, three times: `src/block.rs:130 SCHEDULE_LEN = 4 + 4 + 8 * 31`, `src/params.rs:21-24
  MAX_ADMIN_BYTES = 20 + 32 + 1 + 256`, and the source `crates/arkiv-db/src/codec.rs:1386
  PriceSchedule::MIN_ENCODED_LEN`. Use the arkiv-db constant in `block.rs`; `MAX_ADMIN_BYTES =
  AdminInput::MIN_ENCODED_LEN as u64` (block.rs:471, same 309, pinned by `tests/block_format.rs:338-339`).
  safe [CONSENSUS: value must stay 309].
- `src/block.rs:306` uses `48` for `Limits`; `crates/arkiv-db/src/codec.rs:1428` says `Limits::MIN_ENCODED_LEN =
  9 * 8` (72). Stale hand copy. It is only a lower bound and no sequence of `Genesis` is decoded, so replacing it with
  `Limits::MIN_ENCODED_LEN` changes nothing observable. safe.
- `src/params.rs:27 EMPTY_INPUTS_BYTES = 8` == `src/block.rs:908 BlockInputs::MIN_ENCODED_LEN`. safe.
- `src/accounts.rs:10 GAS_PRICE_WEI = 1_000_000_000` vs `src/gas.rs:11 REFERENCE_GAS_PRICE_WEI = 1_000_000_000`,
  and `accounts.rs:292 fee(gas)` vs `gas.rs:281 reference_fee_wei(gas)` (same product, `U256` vs `u128`). Define one
  from the other. safe [CONSENSUS: value].
- System cell names as string literals 26x in `src/arkiv/mod.rs` (366-373, 526, 543, 550, 755, 789) and
  `src/arkiv/schema.rs` (220-230, 258-266); `"$expiresAt"` 5x although `crate::EXPIRES_AT` exists
  (`src/block.rs:76`); `SYSTEM_CELLS = 7` (schema.rs:78) is a hand count. Name them once in `schema.rs`. safe.

**D14. Duplicate-identity loop twice.** `src/validate.rs:354-362` and `src/identity.rs:262-266` (a `BTreeSet` over
`RequestIdentity::of_block` -> `DuplicateIdentity`). One `fn first_duplicate(chain_id, inputs) -> Option<(Scope, u32)>`
in `identity.rs`. safe, -6.

**D15. Small ones in `src/exec.rs`.** safe
- "Applied or Failed" wrap at 1129-1133 and 1149-1153: `Step::of(outcome, rent, units)`.
- `malformed` closure 1279 and 1288, written out again at 1300-1302: one `fn malformed(key: &HostKey) -> ExecError`.
- `self.accounts.filter(|_| !self.estimate)` at 949 and 1141: a `charged_accounts()` method.
- `(0, 0)` expiry test: `src/validate.rs:178` and 205-207: `Expiry::is_unset()`.

**D16. U256 <-> 32 big-endian bytes, 12 inline sites.** `src/accounts.rs` 125, 151, 203, 215, 232, 254, 268,
287-289; `src/block.rs` 705, 787, 1360, 1372. Two helpers (`put_u256(encoder, v)` / `get_u256(decoder)`) next to
the block codec. safe.

### II.1 [S3] arkiv-db-storage, arkiv-db-memory, arkiv-db-redb src

*(repo-relative paths)*

**D2. `read_object` / `read_record` are hand-forwarded to the bounded variant in every implementation.**
- memory: `crates/arkiv-db-memory/src/versioned.rs:70-72`, `:103-105`
- redb: `crates/arkiv-db-redb/src/store.rs:1018-1020`, `:1084-1086`
- test doubles doing the same: `crates/arkiv-db/src/durable/adopt.rs:631-633`, `crates/arkiv-db/tests/durable_failures.rs:1267-1269`, `crates/arkiv-db/tests/adopt.rs:344-346`
- five `ObjectReader`-only doubles do the *inverse* (`read_object_bounded` ignores `_max_bytes` and calls `read_object`): `crates/arkiv-db/src/engine.rs:3027-3034`, `crates/arkiv-db/tests/rehash.rs:93-100`, `tests/storage_faults.rs:141-148`, `tests/eq_proofs.rs:828-835`, `tests/range_proofs.rs:615-622`. That is what `versioned.rs:629-630` forbids: "Reading first and checking the allocated `Vec` later does not satisfy the contract."

Fix: make them provided methods on the traits - `ObjectReader::read_object` = `self.read_object_bounded(space, id, usize::MAX)`, `VersionedStore::read_record` = `self.read_record_bounded(table, key, usize::MAX)`. Then delete the four backend methods, the `FaultyStore` pair (`testing/faulty.rs:318-321`, `:344-347`) and four methods of D1. About -30 lines in this chunk, about -40 in the doubles. **needs-care**: `Corruptible` (`durable_failures.rs:404-406`, `:430-432`) and `Overlay` (`simulator_composition.rs:331-333`) deliberately `panic!` in the unbounded methods and keep their overrides.

**D1. `Arc<T>` / `Box<T>` forwarding impls** - `crates/arkiv-db-storage/src/versioned.rs:771-821` and `:823-873`. Two 51-line blocks, identical except for the pointer type. Yes, macro-able, with a private macro in the same file:

```rust
macro_rules! forward_to_pointee {
    ($($ptr:ident),+) => {$(
        impl<T: ObjectReader + ?Sized> ObjectReader for $ptr<T> { /* 2 methods */ }
        impl<T: VersionedStore + ?Sized> VersionedStore for $ptr<T> { /* 10 methods */ }
    )+};
}
forward_to_pointee!(Arc, Box);
```

There is no `&T` impl and none is needed (`reconcile` takes `&S`). Both impls are used (`Arc<MemoryVersionedStore>` everywhere; `FaultyStore<Box<dyn VersionedStore>>` at `testing/contract.rs:865-868`). About -48 lines, and the two can no longer drift when a trait method is added. **safe**.

**D4. Result -> `CommitOutcome` -> stats is written five times, with one divergence.**
- memory `commit` (`versioned.rs:123-137`) and `relabel` (`:152-166`) are the same 13 lines.
- memory `stage_objects` (`:139-150`) is a third copy that does not count a refusal (`:148` returns `NotCommitted` without `self.stats.commit`); redb counts it (`store.rs:1151`). The two backends therefore disagree on `StoreStats::not_committed` after a refused `stage_objects`.
- redb `transact` (`store.rs:497-549`) returns `(CommitOutcome, Vec<(SpaceId, usize)>, u64)` and spells `(.., Vec::new(), 0)` six times (`:505, :508, :513, :527, :539, :546`), only for its three callers (`:1139-1140`, `:1150-1151`, `:1157-1159`) to hand the triple straight to `self.stats.commit`.
- the anonymous `(Vec<(SpaceId, usize)>, u64)` appears 8 times (memory `:204, :214, :252`; redb `:491, :500, :501, :557, :657`).

Fix, beside `StatsRecorder` (`versioned.rs:1065`):

```rust
/// What a committed batch newly stored.
#[derive(Debug, Default)]
pub struct Written { pub objects: Vec<(SpaceId, usize)>, pub records: u64 }

impl StatsRecorder {
    /// Counts an applied or refused batch and returns its outcome.
    pub fn applied(&self, result: Result<Written, StorageError>) -> CommitOutcome { .. }
}
```

memory: `commit`, `relabel`, `stage_objects` each become one line (`self.stats.applied(self.apply(batch, false))`), -28 lines. redb: `transact` records the stats itself through the existing `StatsRecorder::commit` (it has the `Indeterminate` arm) and returns `CommitOutcome`, -15 lines. **behaviour-affecting** only for the memory `not_committed` counter (a non-canonical measurement, `versioned.rs:985-987`); redb's numbers, which feed the node's metrics page (`crates/arkiv-node/src/signed/metrics.rs:455`), do not change.

**D6. redb-internal triplicates** (`crates/arkiv-db-redb/src/store.rs`), about -20 lines, **safe**:
- reading the head from `META`: `:559-562`, `:660-663`, `:1073-1076` -> `fn read_head(meta: &impl ReadableTable<&'static str, &'static [u8]>) -> Result<Option<Head>, Fault>`.
- `if table == Table::Blocks { BLOCKS } else { MANIFESTS }`: `:610-614`, `:700-704`, `:1109-1113` -> `fn height_table(table: Table) -> TableDefinition<'static, u64, &'static [u8]>` (all three callers are already inside the `Blocks | Manifests` arm).
- `builder.create_with_backend(CountingBackend::with_probe(backend, probe.clone()))?` + `prepare(&db)?`: `:305-308`, `:798-801`, `:872-873` -> `fn open_counted(..) -> Result<(Database, Prepared), Fault>`.
- the tails of `commit` (`:1137-1145`) and `relabel` (`:1155-1164`) fold into D4.

**D7. `CommitBatch::validate` duplicate detection** - `versioned.rs:383-394` and `:396-408` are the same 10-line `Entry::Vacant / Occupied` match. `or_insert` does each in three lines:

```rust
if *objects.entry((*space, id)).or_insert(bytes) != bytes.as_slice() {
    return Err(object_conflict(*space, id));
}
```

-14 lines and the `btree_map` import goes. **safe**.

**D3. `copy_bounded` exists three times.** `crates/arkiv-db-memory/src/versioned.rs:183-195` and `crates/arkiv-db-redb/src/store.rs:1226-1239` have the same body and the same message (`"stored value has {} bytes, exceeds read limit {max_bytes}"`); they differ only in `StorageError` vs `Fault`. A third variant is `crates/arkiv-db/tests/durable_failures.rs:389-401`. Home: `pub fn copy_bounded(bytes: &[u8], max_bytes: usize) -> Result<Vec<u8>, StorageError>` in `versioned.rs` beside `object_conflict` (`:472`). redb call sites stay one-liners because `Fault: From<StorageError>` (`error.rs:40`): `Ok(table.get(id)?.map(|v| copy_bounded(v.value(), max_bytes)).transpose()?)`. -13 lines here, -13 in the test. **safe**.

**D5. `stage_objects` preamble** - memory `versioned.rs:140-141` and redb `store.rs:1148-1149` both write `CommitBatch::new(None, Head::default())` then `batch.objects = objects.to_vec()`. That deep-copies every staged object (the bulk-import path: `crates/arkiv-db/src/durable/bulk.rs:326`, `streaming.rs:92`) only to reuse a `(batch, headless: bool)` signature with a dummy head. Fix: split `CommitBatch::validate` (`versioned.rs:370-410`) so its object half is a pub `validate_objects(&[(SpaceId, ObjectId, Vec<u8>)])`, and give the two internal apply functions (memory `apply_locked` `:248-292`, redb `stage` `store.rs:553-650`) the shape `(expected: Option<Head>, objects: &[..], records: &[..], new_head: Option<Head>)`. `stage_objects` passes `(None, objects, &[], None)`: no clone, no bool flag, no dummy head. **needs-care**.

**D8. `load_object` / `load_object_bounded`** (`versioned.rs:640-650`, `:656-672`) build the same `Missing` error. Add `fn missing_object(space: SpaceId, id: &ObjectId) -> StorageError` beside the other constructors (`:462-504`). -8 lines. **safe**.

**D9. The latching predicate lives in two crates.** `FaultState::fail` (`testing/faulty.rs:126-130`, `Io | Full => true`) re-implements `latches` (`crates/arkiv-db-redb/src/error.rs:163-165`). Home: `impl StorageErrorKind { pub const fn is_medium_failure(self) -> bool }` in `versioned.rs`. The contract case `faulty_latching_kinds_follow_the_redb_rule` (`contract.rs:945`) then checks one definition instead of two that must agree by hand. **safe**.

**D10. Seeded RNGs.** Three in this chunk: `SplitMix` (`crates/arkiv-db-redb/src/testing.rs:463-491`), xorshift `Generator` (`crates/arkiv-db-memory/src/persistent.rs:461-470`), inline xorshift in `testing/contract.rs:109-114`. `rg -n 'struct (Rng|Generator|SplitMix)\b' crates` finds 13 more (arkiv-db tests `codec`, `history`, `metering`, `model`, `queries`, `query_planner`, `rehash`; `arkiv-db/src/layout.rs:649`; `arkiv-db-trie/src/node.rs:515` and `tests/support/mod.rs:23`; `arkiv-node/tests/block_model.rs:1396, :1500`; redb examples). Belongs with the shared test-support work; not in the brief's known list.

#### Helper inventory for a shared `tests/support` (this chunk)

| helper | location | size | signature | note |
| --- | --- | --- | --- | --- |
| `head` | `arkiv-db-storage/src/versioned.rs:1148-1153` | 6 | `fn head(height: u64, tag: u8) -> Head` | hash = `[tag; 32]` |
| `head` | `arkiv-db-storage/src/testing/contract.rs:100-104` | 5 | `pub fn head(height: u64, fork: u8) -> Head` | hash = `[fork; 32]` with the height in bytes 0..8 |
| `head` | `arkiv-db-memory/src/versioned.rs:301-306` | 6 | `fn head(height: u64) -> Head` | hash = `[height as u8 + 1; 32]` |
| `kind` | `arkiv-db-memory/src/versioned.rs:308-315` | 8 | `fn kind(outcome: CommitOutcome) -> Option<StorageErrorKind>` | |
| `object` | `testing/contract.rs:107-117` | 11 | `pub fn object(factory: &dyn StoreFactory, tag: &str, n: u64, len: usize) -> (ObjectId, Vec<u8>)` | "about `len`" bytes |
| `committed` / `not_committed` / `indeterminate` | `testing/contract.rs:119-135` | 17 | `fn committed(outcome: CommitOutcome)`, `fn not_committed(outcome: CommitOutcome, kind: StorageErrorKind)`, `fn indeterminate(..same..)` | |
| `read` / `record` | `testing/contract.rs:137-143` | 7 | `fn read(store: &dyn VersionedStore, space: SpaceId, id: &ObjectId) -> Option<Vec<u8>>`, `fn record(store: &dyn VersionedStore, table: Table, key: &[u8]) -> Option<Vec<u8>>` | |
| `block_batch` | `testing/contract.rs:147-177` | 31 | `fn block_batch(factory: &dyn StoreFactory, parent: Option<Head>, height: u64, objects: u64) -> CommitBatch` | |
| `fnv_id` | `arkiv-db-memory/tests/versioned_contract.rs:22-33` | 12 | `fn fnv_id(bytes: &[u8]) -> ObjectId` | keccak stand-in; the crate is std-only |
| `Memory`, `WrappedMemory` factories | `arkiv-db-memory/tests/versioned_contract.rs:35-63` | 29 | `impl StoreFactory` | same two structs again at `arkiv-db-redb/tests/contract.rs:98-126` (keccak ids) |

`contract::head` and `contract::object` are `pub` "for store crates" but nobody imports them (`rg -n 'contract::' crates conformance` shows only `{StoreFactory, TestStore}`); `crates/arkiv-db-redb/tests/common/mod.rs:97-120` has its own `object(tag: u64, n: u64, len: usize)` (exact length) and `head(height: u64, fork: u8)` (keccak-based hash). Four `head` helpers, four hash derivations.
