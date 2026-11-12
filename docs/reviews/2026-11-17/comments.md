# Code-quality review, 2026-11-17: Part II.3: comments

Reviewed at `e6c49e1`; line numbers refer to that commit. Parts: [summary](README.md) · [duplication](duplication.md) · [tests](tests.md) · [comments](comments.md) · [sloppiness](sloppiness.md) · [other](other.md) · [appendices](appendices.md)

## II.3 Comments


### II.3 [T3] arkiv-db tests: queries / records / pagination / terms

*(paths relative to `crates/arkiv-db/tests/`; `qm` = `query_metering.rs`)*

#### 3.1 Stale or wrong comments (quote + contradicting code) - safe

1. `records.rs:5-6` module doc: "`RecordKey` (`len ‖ bytes ‖ zero padding`, 33 bytes, keys of 1..=32 bytes)". The test under it says and asserts the opposite, `records.rs:647-651`: "`len ‖ bytes`, no padding. b\"a\" -> 01 61" / `assert_eq!(arkiv_db::encode(&short), [0x01, 0x61])`; `src/layout.rs:126` agrees ("`len ‖ bytes` (2 to 33 bytes)"). `docs/testing.md:170` describes this test with similar wording ("the length-prefixed 33-byte form"); check it in the same pass. TIGHTEN to "`RecordKey` (`len ‖ bytes`, 2 to 33 bytes)".
2. `queries.rs:2186` doc on `page`: "Runs one page and returns `(ids, next.after, receipt)`"; the signature at 2195 returns a 4-tuple `(Vec<u64>, Option<u64>, Receipt, Option<Cursor>)`. Fix with 4.3 (return a struct) or update the doc.
3. `queries.rs:1252` "A named projection skips names the record lacks without charging them." The code below matches no record and reads no receipt (see 2.15). DELETE with the block.
4. `queries.rs:1775-1779`: `.expect("one row, one more to come")` followed by `assert_eq!(bogus, None, "the small fixture has one record")`. The two messages contradict each other; the block is a leftover (4.5).
5. `terms.rs:1083` "// max_value_len 4: \"abcd\" is the longest admissible Str bound." There is no `max_value_len` (`rg -n max_value_len crates/arkiv-db` finds only this line); the code sets `max_str_len: 4, max_bytes_len: 4`. TIGHTEN to "max_str_len 4: ...".
6. `records.rs:1571` doc on `many_cells(count)`: "Sixty-five distinct names, `c00`..`c64`, each an indexed `Bool`." The function takes `count` and is called with 65, 64 and 33. TIGHTEN to "`count` distinct names `c00`.., each an indexed `Bool`."
7. `terms.rs:844` doc on `expected_deletes(deletes: &[(u64, u64)])`: "The deletes `EXPIRY_DELETES[range]` as the sweep reports them." There is no `range` parameter. TIGHTEN to "`deletes` as `(RecordId, receipt)` pairs, the way the sweep reports them."

#### 3.2 References a new reader cannot resolve - safe

- "the brief" / "the brief's ..." 17 times, plus one test name: `rg -n '\bbrief' crates/arkiv-db/tests/{queries,records,pagination,terms}.rs` -> `records.rs:59, 81, 301, 332, 1091`; `terms.rs:16, 63, 272, 294, 1100` and the test name `terms_validation_faults_are_admission_rejections_in_the_briefs_order` (1082); `queries.rs:17, 31, 141, 284`; `pagination.rs:8, 96, 446`. No such document is in the repo (`docs/` has none; `docs/testing.md` and `docs/block-format.md` mention it the same way). TIGHTEN: say what is meant ("the record semantics", "the validation order `NotIndexable, MixedBounds, ValueTooLarge, ZeroLimit, LimitTooLarge`") or link the in-repo doc that now holds the charge table. Rename the test to `.._in_validation_order`.
- `pagination.rs:8` "milestone-2 brief": milestone label. DELETE the qualifier.
- `pagination.rs:10-12` "the model-version-3 charge points (the read charge points are those of version 2)": history. TIGHTEN to "the model-version-3 charge points".
- `records.rs:92-94` "Model 3 adds RootUpdate 3 to the record group ... and RootUpdate 2 to every index group": phrased as a diff against an earlier model. TIGHTEN to "Every record group also charges RootUpdate 3 (key binding, row, catalog leaf) and every index group RootUpdate 2 (posting entry, term leaf), 2 units each."
- `terms.rs:763` "Delete cost (model 3) =" and `terms.rs:2035` "the model-3 delete formula": fine once `MODEL_VERSION` is the one place that says 3; drop "(model 3)".

#### 3.3 Section banners - safe

`rg -c '^\s*// -{20,}\s*$'`: `queries.rs` 18, `records.rs` 18, `pagination.rs` 28, `terms.rs` 22 = 86 rule lines in 43 banner blocks (166 lines with their text).

- 31 are bare labels (3 lines each, 93 lines). DELETE: `queries.rs:63-65, 342-344, 896-898, 1097-1099, 1285-1287, 1542-1544, 2093-2095`; `records.rs:111-113, 638-640, 870-872, 1386-1388, 1896-1898, 2253-2255`; `pagination.rs:60-62, 201-203, 356-358, 611-613, 701-703, 796-798, 1004-1006, 1131-1133, 1382-1384, 1434-1436, 1901-1903, 2164-2166`; `terms.rs:84-86, 871-873, 1077-1079, 1350-1352, 1884-1886, 2028-2030`. If the grouping is wanted, `mod encodings { .. }`, `mod write_vectors { .. }` inside the test file gives it for free and shows up in test names (`records::write_vectors::delete_..`).
- 12 carry real content (fixture derivations): `records.rs:58-61, 80-95, 331-334`; `terms.rs:62-65, 271-275, 591-597, 708-712, 758-768`; `queries.rs:139-142, 639-642`; `pagination.rs:94-98, 444-447`. TIGHTEN: drop the two rule lines and make each a doc comment on the item it describes (`fixture_cells`, `mixed_records`, `cost_records`, `expiry_records`, `struct Model`).

#### 3.4 Over-long module docs - safe

`records.rs:1-34` (34 lines), `terms.rs:1-42` (42), `queries.rs:1-42` (42), `pagination.rs:1-48` (48): 166 lines of "What this file establishes / does not establish" that restate the test names one by one, and are restated a third time in `docs/testing.md:170-176`. They also rot (3.1 item 1 is in one). TIGHTEN each to <= 10 lines: one sentence on the subject, the oracle rule ("rows come from the BTreeMap model, costs are hand-derived literals"), and a pointer to `docs/testing.md`. ~120 lines.

#### 3.5 Derivations stated twice - safe

`pagination.rs:108-197`: every cost constant has a doc comment that spells the sum ("1 + 5 + 4, PostingEntry 3 ..., ResultRow 2 = 19") and then a `const _` assert that spells it again in code. Keep one (see 2.8). In `records.rs` the derivation comment of each write vector is repeated in the test body and in the fixture banner (80-95) and again in `qm`; goes away with 2.1.

Individual restating comments are rare in these files; I am not listing `/// Snapshot 1: ...` style fixture docs, which are useful.

### II.3 [T2] arkiv-db tests: history / isolation

*(paths relative to `crates/arkiv-db/tests/`; `h` = history.rs, `qh` = query_history.rs, `i` = isolation.rs, `qi` = query_isolation.rs)*

Stale or inaccurate (quote, then the contradicting code):
1. query_history.rs:12-14 "stamps all of its keys to expire in the next, empty block" and 2510-2511 "the next block (an empty, starved one)". The block after a stamping block is 20, 40, 60, 80, 100, 120; `block_size` (2488-2494) gives `SIZES[(block - 2) % 6]` = 0, 4, 16, 0, 4, 16. Empty only twice in six. TIGHTEN: drop "empty".
2. query_history.rs:3340-3342 "`thorough` adds estimates ... the transcript is the same either way". 3476-3486 pushes the branch-phase read receipts into the transcript only when `thorough`. Nothing compares the two, and `full_run` never reads `run.transcript`, so delete the push at 3483-3485 (then the comment is true). **safe**
3. query_history.rs:2390 "An operation that fails admission, chosen by `which`." Case 4 (2426-2427) is an empty patch, commented "golem-db's no-op (or NotFound)"; `argument_fault` (573-596) returns `None` for it and `patch_charges` (341-344) prices it. TIGHTEN the doc or move case 4 into `plan_op`.
4. query_history.rs:472-473 and query_isolation.rs:144-145 "1 for Bool, 8 for the integers, the byte length for Str and Bytes". The body also returns 4, 16, 20 and 32 for sixteen other variants. TIGHTEN to "the type's fixed width, or the byte length for Str and Bytes".
5. query_isolation.rs step labels (strings that appear in assertion messages) name budgets the code no longer uses: 791 "with budget 150" vs `limited(200)` at 1207; 796 "budget 30" vs `limited(80)` at 1270; 799 "budget 60" vs `limited(80)` at 1283; 802 "delete budget 80" vs `limited(110)` at 1295. The derivation comments at 2134, 2244, 2292, 2339 have the current numbers. Fix the four strings. **safe**

References to documents that are not in the repository (`find . -iname '*decision*' -o -iname '*brief*'` outside target/vendor: nothing):
- "the decisions file": history.rs:6, 102, 1269, 2015, and the test name at 2013.
- "decisions2.md": query_isolation.rs:103, 1498.
- "the brief" / "milestone-2 brief": query_history.rs:22, 34, 36, 44, 189, 223, 469, 546, 600, 745, 812, 3896, 4101, 4183; query_isolation.rs:107, 144, 530; fn `brief_vectors` (qh:3899) and two test names (qh:3921, 4015).
  TIGHTEN: name an in-repo source (`rg -l "IndexTermCreate|DirectoryLookup" docs` gives architecture.md, block-format.md, testing.md) or say "the charge model" without a file. `rg -n "decisions2\.md|decisions file|\bbrief\b" crates/arkiv-db/tests/{history,query_history,query_isolation}.rs`

Milestone / past-version labels:
- "milestone-1 oracle" in three `unreachable!` messages: history.rs:213, 597, 676. TIGHTEN to "this oracle issues no typed record operations".
- "milestone 2" / "milestone-2": query_history.rs:1, 22, 189; query_isolation.rs:2. DELETE the label.
- query_isolation.rs:218-222 "the model-2 totals 551, 263, 262, 313, 233 plus RowByte ..." and 225-231 "Model 2 gave 442 040 ... model 3 adds per record ...": derivations written as old-version total plus delta. TIGHTEN to a from-scratch derivation (as 2082-2084 does).
- Same style at query_history.rs:3934-3935 ("302 + RowByte 47 + RootUpdate 2 * (3 + 2 * 6) = 379; 152 + 47 + 30 = 229; 181 + 46 + 30 = 257"), 3973-3974 ("123 + RowByte 40 ..."), 3989 ("72 + RootUpdate ...; 76 + 30"): the leading 302/152/181/123/72/76 are previous-model totals a new reader cannot reproduce. TIGHTEN.
  ("model version 3" by itself is fine: it is the `model_version` field of the receipts.)

Section banners (`rg -n '^// -{10,}' <file>`):
- history.rs: 8 banners, 25 lines (80-83, 296-298, 687-689, 845-847, 1050-1052, 1215-1217, 1328-1330, 2008-2010).
- query_history.rs: 14 banners, 52 lines (67, 187, 468, 545, 598, 811, 1018, 1327, 1490, 2210, 2630, 3041, 3281, 3892).
- isolation.rs: 7 banners, 25 lines (43, 77, 131, 612, 677, 834, 1068).
- query_isolation.rs: 10 banners, 31 lines (61, 102, 248, 391, 528, 744, 1416, 2495, 2687, 2938), plus 8 inline `// --- ` sub-banners inside `expected_baseline` (1534, 1576, 1644, 1808, 1825, 1862, 2081, 2409) that become function names after the split in category 4.
- DELETE all; where a banner carries real content (history.rs:80-83, query_history.rs:187-194 and 811-816, isolation.rs:77-83) move the sentence into a doc comment on the item below. The prose block isolation.rs:680-710 (derivation of the pinned node counts) is a keeper; attach it to `A_TRIE_NODES`.

Module-doc template: the "What this file establishes: ... What it does not establish: ..." layout is in
isolation.rs (32 lines), query_history.rs (50), query_isolation.rs (46) and 16 more test files
(`rg -c "What this file establishes|What it does not establish" crates`). TIGHTEN each to fixture shape plus
the invariants; drop the negative-scope paragraphs (i:29-32, qh:48-50, qi:42-46). history.rs:1-7 is the model to follow.

### II.3 [T4] arkiv-db tests: codec / namespaces / blocks / rollback / roots / proofs

*(paths relative to `crates/arkiv-db/tests/`)*

#### 3.1 Section banners - 125 lines - DELETE - safe
`rg -n '^// -{20,}$|^// --- .* -{3,}$' crates/arkiv-db/tests/{codec,namespaces,blocks,rollback,roots,host_state}.rs`

| file | banners | lines | note |
|---|---|---|---|
| codec.rs | 11 three-line | 33 | |
| namespaces.rs | 11 three-line | 33 | ten are numbered "1." .. "10." |
| rollback.rs | 8 three-line | 24 | numbered "1." .. "8." |
| roots.rs | 6 three-line | 18 | |
| blocks.rs | 14 one-line (`// --- identities ---…`) | 14 | lines 40, 57, 82, 115, 152, 190, 274, 630, 718, 919, 1014, 1176, 1221, 1314; eight numbered |
| host_state.rs | 1 three-line | 3 | |
| eq_proofs, range_proofs, prefix_free_terms, ordered_ranges, golem_types | 0 | 0 | |

The numbers ("// 4. Same-block authority change") refer to a list that is not in the file; test names already carry the grouping.

#### 3.2 Stale (comment contradicts the code next to it)
- roots.rs:76 `/// The type id: Bool 1, I64 2, U64 3, Str 4, Bytes 5.` vs the match directly below (79-83): `Bool => 1, Str => 2, Bytes => 3, U64 => 13, I64 => 17`. TIGHTEN to `/// The golem type id: Bool 1, Str 2, Bytes 3, U64 13, I64 17.` safe
- golem_types.rs:6 `//! Runs under both profiles: every type id is admitted by both.` and range_proofs.rs:975-976 `/// Every golem-grid ordered type proves complete ranges in value order, in both profiles; …`. Both tests build exactly one engine with default limits (`Engine::new(PriceSchedule::FIXTURE)`, golem_types.rs:245, range_proofs.rs:22) and verify with `Limits::DEFAULT`; no Cargo feature exists (`rg -n '^\[features\]' crates/*/Cargo.toml` -> none) and scalar_profile.rs:1 says "The single engine profile". DELETE the "both profiles" clauses. safe
- blocks.rs:274 `// --- 1. the six-step deliverable flow`, with step comments at 283 ("Step 1"), 327 ("Step 2"), 441 ("Step 3"), 533 ("Step 5"), 558 ("Step 6"): there is no Step 4, and "six-step" / "deliverable" name a plan, not anything visible in the test. TIGHTEN to block-oriented comments ("Block 1: one namespace per owner") when splitting (2.3). safe

#### 3.3 Plan-step / milestone labels - TIGHTEN - safe
`rg -n "node step|N0b|N3b|N3d|deliverable" crates/arkiv-db/tests/{codec,roots,host_state,blocks}.rs`
- codec.rs:33-35 "block formats (node step N0b-3), trie leaf layouts or roots (`roots.rs` and the `layout` unit tests cover those since N3b)" -> "What it does not establish: block formats, trie leaf layouts or roots (see `roots.rs` and the `layout` unit tests), or storage."
- roots.rs:1 "State roots of the engine (node step N3b) against an independent model." -> drop the parenthesis. roots.rs:35-36 "proofs (node step N3d), or anything about header v2 (a later node step)" -> "proofs (see `eq_proofs.rs`, `range_proofs.rs`)".
- host_state.rs:1 "The engine's host-state map (node step N0b-2)." -> drop the parenthesis.

#### 3.4 References to how the code used to be - TIGHTEN - safe
- ordered_ranges.rs:1 "Range predicates on the types that used to be equality-only: `Bool`, …" -> "Range predicates on the byte-ordered types: `Bool`, …".
- range_proofs.rs:724-725 "(the old fixed-width predecessor arithmetic had to open it)" -> delete the parenthesis.
- codec.rs:1043 "Model-2 schedules had 27 classes: never silently padded." -> "A shorter class table is never silently padded." (keep the vector).

#### 3.5 Over-long module docs that restate the test list - TIGHTEN - safe - ~60 lines
- codec.rs:1-35 (35 lines), roots.rs:1-36 (36), host_state.rs:1-33 (33): "What this file establishes" bullets paraphrase the test names one by one and are repeated a third time in docs/testing.md (rows at docs/testing.md:298 codec.rs, :299 host_state.rs, :331 roots.rs). codec.rs:20-25 additionally hard-codes counts ("12 types", "9 types", "1,000 seeded mutations of each of 17 seed vectors (the 8 `WriteOp` vectors and 9 others)") that have to be kept equal to codec.rs:1114-1138 and 1208-1233 by hand (they are correct today). Suggested shape, 6-8 lines each: where the expected side comes from (literal bytes / `BTreeMap` model / independent trie) and why it never touches the code under test, then the "does not establish" line. Drop the numbers.

#### 3.6 Doc comments that repeat the item - DELETE - safe
- codec.rs:68 `/// Concatenation of pieces.` on `fn cat`.
- roots.rs:536 `/// Applies `op` to the model.` on `fn apply(model: &mut Model, op: &Op)`.
- blocks.rs:95, 100, 105, 110: four one-liners on `applied` / `rejected` / `read` / `read_rejected` ("An applied write receipt under the FIXTURE schedule (id 1)." …). One sentence above the group is enough.
- codec.rs:1307-1309 goes with the test deleted in 2.1 (and is inaccurate, see there).

### II.3 [T1] arkiv-db tests: metering family

*(paths relative to `crates/arkiv-db/tests/`; `qm` = `query_metering.rs`)*

1. **Header prose that restates docs/** - **TIGHTEN**.
   - qm:1-47 (47 lines). Lines 23-47 ("What this file establishes" / "What it does not establish") are the row at docs/testing.md:173; the weight list at 8-12 is the table at docs/snapshot-query-plan.md:358-372. Keep ~12 lines: what the file is, the pointer to the plan, and the one rule a reader needs (totals are written as explicit sums and the group table must reproduce them, qm:16-21). -30 lines.
   - trace_regression.rs:1-47 (47 lines). Lines 11-47 are docs/testing.md:233 almost sentence for sentence. Keep the bug statement (3-9) and "every count is a hand-derived literal". -25 lines. The 80-line fixture doc at 80-159 stays: docs/testing.md:317 defers to it ("drawn node by node in the file") and the literals cannot be checked without it.
   - metering.rs:1-12: fine in length; see item 3 for the wording.
   - query_planner.rs:1-35: no row in docs/testing.md for this file (`rg -n query_planner docs/testing.md` is empty), so the header is the only description. Keep.

2. **"the brief"** - qm, 27 lines (`rg -n -i brief crates/arkiv-db/tests/query_metering.rs`): 13, 18, 36, 74, 77, 351, 418, 554, 662, 681, 797, 1708, 1719, 2120-2122, 2133, 2305, 2429, 2706, 3037, 3526, 3539, 3868, 3874, 3878, 4063. No document called a brief exists in the repo; the charge points are docs/snapshot-query-plan.md:476-484 and the worked vectors :643-662, which the same file calls "the plan" at 4753/4761. **TIGHTEN**: name the doc once in the header, say "the plan's" elsewhere, rename `brief_patch` (2122, 3868) and the vector label `"patch a (brief vector)"` (1719).

3. **Milestone and step labels** - **TIGHTEN/DELETE**.
   - trace_regression.rs, 13 lines mention `N0a`/`N3b` (`rg -n 'N0a|N3b' crates/arkiv-db/tests/trace_regression.rs`): 1, 5, 37, 40, 41, 450, 457, 669, 674, 725, 767, 808, 847. The labels are defined only in docs/node-storage-plan.md:374. Header: describe the bug without the label. Per-row notes ("Before N0a only the lookup was reported"): keep the information, reword to "Regression: ...". DELETE the pure history at 36-37 ("They are the same receipts as before the engine moved to tries (node step N3b)") and 40-41 ("node step N3b re-derived them from the AVL-era literals of N0a").
   - metering.rs:1 "the milestone-1 engine"; 9-11 "the model-version-3 charge points of the raw operations: the version-1 points plus ..." -> state the current rule only (`RootUpdate 1` in the namespace group, `RootUpdate 2` in the put/delete group).
   - qm:1 "every milestone-2 charge point", 13 "the milestone-2 brief's points, with `RowByte n` and `RootUpdate` joining the record groups" -> same treatment.
   - trace_regression.rs:111-113 "(The fixture uses `rise` rather than layout 1's `rose` ...)": keep the why, drop "layout 1's".

4. **Stale or wrong** - fix.
   - bounds.rs:1 `//! Admission boundaries applied to real block packets and historical states.` The file never builds a block; it uses `Engine`/`Branch`/`Snapshot` only. -> "Admission bounds on names, raw keys and indexed values, on branches and published snapshots."
   - bounds.rs:203 trailing `// 32 characters, 64 UTF-8 bytes.` beside `"é".repeat(arkiv_db::Limits::DEFAULT_INDEXED_LEN / 2)`; `DEFAULT_INDEXED_LEN` is 128 (`src/record.rs:868`), so it is 64 characters, 128 bytes. Also the only line over 100 columns in the chunk (117); moving the comment above the statement fixes both.
   - bounds.rs:189 test name `index_values_have_a_hard_byte_cap_while_payloads_keep_their_own_limit` and 190 "indexed-value cap": the indexed limit is configurable (`Limits::max_indexed_len`, "An explicit admission policy, no longer a key width", `src/record.rs:866-868`); what the test shows is that raising `max_str_len`/`max_bytes_len` does not raise it. Rename, e.g. `raising_payload_limits_does_not_raise_the_indexed_string_limit`.
   - scalar_profile.rs:5 test name `profile_limits_are_explicit_and_namespaces_stay_independent`: nothing in the body (6-22) concerns namespace independence. Rename `profile_constants_and_name_length_bounds`.
   - qm:4772 `// The written charges of the same reads are untouched: 9 and 13.` Only the 9 is asserted (4773-4781). Add the `Exists(price, I64)` assertion or drop "and 13".
   - qm:2305 `// p = 1 + 18 + 8 + 7 + 13 = 47 (the brief's 54).` reads as if 47 were 54; 54 is the total `1 + 3 + (3 + 47)`. -> "(total 54, the plan's vector)".

5. **Section banners** (`rg -c '^\s*// -{10,}'`, two dash lines per banner): metering.rs 11 banners / 33 lines (39-41, 341-343, 552-554, 667-669, 838-840, 987-989, 1046-1048, 1295-1297, 1624-1626, 1836-1838, 1992-1994); qm 18 banners / 55 lines (73-75, 260-262, 323-325, 416-419, 627-629, 728-730, 779-781, 1012-1014, 1575-1577, 2062-2064, 2269-2271, 2390-2392, 3513-3515, 3696-3698, 3861-3863, 4024-4026, 4319-4321, 4537-4539); trace_regression.rs 6 / 18; query_planner.rs 6 / 18; the other three 0. 124 lines. The numbered ones have drifted: qm's last two tests (4710, 4752) sit under "11. Pricing pinning". **DELETE**; where a grouping matters, a `mod` does it (see 5.1).

6. Doc comments duplicated along with the helpers they sit on (`/// Counting-rule version recorded in every receipt.`, `/// The id PriceSchedule::FIXTURE carries.`, `/// The canonical part of a write result ...`) go away with 1.1.

Derivation comments inside the tests (the `// Put(k3, v4): DirectoryLookup 1 -> 1; ...` kind) are the reason the literals are checkable. Not listed; keep.

### II.3 [S3] arkiv-db-storage, arkiv-db-memory, arkiv-db-redb src

*(repo-relative paths)*

Section banners (`// -----`): 0 in every file of this chunk. `rg -n 'missing_docs' crates Cargo.toml` finds nothing, so no lint forces a doc on every item.

**C2. The contract's semantics are stated three times in `versioned.rs`, and the first is stale.** Module doc `:7-31`, `CommitBatch` `:296-307`, `VersionedStore` `# Semantics` `:677-705` (plus the variant docs of `CommitOutcome` `:527-538`). Same sentence three ways: "A commit puts objects **if absent**" (`:12`), "identical entries are idempotent, different bytes under the same object id or record key are a conflict" (`:306-307`), "the same bytes again are idempotent, different bytes are `Conflict`" (`:684-686`).
Stale part - the module doc says:
- `:14` "(this contract has no deletion)"
- `:16-17` "A record never changes once committed."
- `:18-20` "**One durable head** ([`Head`]), changed only by a compare-and-set in [`VersionedStore::commit`]"

but `VersionedStore::relabel` (`:743-752`) removes records and replaces the head - its own doc at `:422-423` says "This is the only operation that removes records or gives a head a value that does not advance" - and `stage_objects` (`:733-741`) stores objects outside `commit`. TIGHTEN: module doc to a ~10-line overview that names the five ideas and links to the trait; the trait's `# Semantics` is the single normative list and gains one bullet each for `relabel` and `stage_objects`. About -25 lines.

**C5. `FaultyStore` struct doc, `testing/faulty.rs:157-193` (37 lines).** It re-lists what the types above it already document one by one (`CommitFault` `:13-31`, `DiskFull` `:33-44`, `ReadScope` `:46-59`) and the latching rule (C3), and its second sentence is false: "Without armed faults every call is forwarded unchanged, so a wrapped store passes the whole behavioural [contract]" (`:159-160`) - `stage_objects` and `relabel` are not forwarded (W1). TIGHTEN to about 8 lines: what it wraps, that faults are armed through the `fail_*` / `push_*` / `set_*` methods, that latching mode follows the rule in arkiv-db-redb's `error` module with `set_latch_corrupt` as the one extension, and that an unarmed store is transparent (once W1 makes that true).

**C3. `RedbStore`'s latching rule is spelled out in six places.** `crates/arkiv-db-redb/src/error.rs:12-24` (the right home) and `:160-162`; `crates/arkiv-db-redb/src/lib.rs:51-60`; `crates/arkiv-db-storage/src/versioned.rs:691-699`; `testing/faulty.rs:122-124`, `:175-193`, `:230-231`; `testing/contract.rs:941-944`. The trait doc in the contract crate should not describe one backend: DELETE the clause "`RedbStore` latches `Io` and `Full` on any call, every error of redb's commit, and a failed `recover`." (`versioned.rs:698-699`), keep "Which of these failures latch is up to the store". `lib.rs:51-60` -> three lines + link to the `error` module. For faulty.rs see C5. About -20 lines.

**C1. The engine-only-profile paragraph is written four times.** TIGHTEN to one.
- `crates/arkiv-db-storage/src/versioned.rs:39-44` (module): "The usual node profile identifies heads by block height/hash. Engine storage v1 explicitly identifies an engine-only profile in metadata and uses snapshot sequence/manifest digest instead, with manifests and their digest index in `Manifests`/`BlockIndex`, leaving `Blocks`/`Inclusions` unused. The adapter's byte shapes do not change; profile validation and interpretation belong to the caller. An engine commit is not a block."
- `versioned.rs:140-143` (`Head`), `versioned.rs:197-200` (`Table`), `crates/arkiv-db-redb/src/lib.rs:23-28` ("Engine storage v1 uses an explicitly identified engine-only profile in caller metadata: ... These shapes require no adapter-format change ...").

Keep the `Head` one (its field docs `:146-149` already carry it). DELETE `versioned.rs:39-44` and `lib.rs:23-28`; cut `Table`'s to its two own facts (key widths unchanged; a state root is never a commit identity). "Engine storage v1" and "do not change / require no ... change" are change-narration. -14 lines.

**C4. "This bounds the value copy, not the pager's buffers" - five times.** `versioned.rs:619-630` (keep, on `read_object_bounded`), `versioned.rs:716-722`, `crates/arkiv-db-redb/src/lib.rs:47-50`, `crates/arkiv-db-redb/src/store.rs:1226-1227`, `crates/arkiv-db-memory/src/versioned.rs:183` (and `:20`). TIGHTEN `read_record_bounded` to: "The record counterpart of [`ObjectReader::read_object_bounded`]: same inclusive limit, `Corrupt` when exceeded, same counter rules." DELETE the rest (D3 removes two of them anyway). -12 lines.

**C6. The create-in-a-temp-file story is told three times in redb.** `store.rs:249-264` (`RedbStore::create`, 16 lines), `store.rs:815-831` (`create_file`, 17 lines, the right home: it explains the mechanism at the code), `lib.rs:37-40`; "redb writes its magic number last" also at `:290`. TIGHTEN `create`'s doc to what a caller sees: never a partial file under `path`; `Locked` for a concurrent create or another process; `Incompatible` for a foreign file; "see `create_file`". -10 lines.

**C7. Stale.**
- `crates/arkiv-db-memory/src/lib.rs:1-3`: "the immutable, structurally shared map used for engine snapshots" and `crates/arkiv-db-memory/src/persistent.rs:6-8`: "This is the snapshot primitive for the block engine: a published state keeps its map forever, while the next candidate builds a new version on top of it." Contradicted by `docs/architecture.md:57` ("reference AVL structure; no longer used by the engine") and by rg (S1). DELETE with the file, or rewrite.
- `testing/contract.rs:1201-1202`: "(helper for store crates' own tests)" on `objects_of` - no crate uses it (S2).
- `crates/arkiv-db-redb/src/store.rs:351`: "Timings of all attempted store operations" - `stage_objects` (`:1147-1153`) is not timed, unlike `commit` and `relabel` (both `timings[3]`).

**C8. Milestone / history labels.**
- `crates/arkiv-db-redb/src/lib.rs:5-6`: "It is the storage candidate of node step N2; `docs/redb-experiment.md` states its pass criteria and the measured results." TIGHTEN to "`docs/redb-experiment.md` has the measurements."
- `crates/arkiv-db-redb/src/lib.rs:5`: "(redb 4.3.0, pinned exactly)" repeats the root `Cargo.toml:30-31` and goes stale on the next bump. DELETE.
- `crates/arkiv-db-storage/src/lib.rs:5`: "the original ordered byte-key map"; `:45`: "This initial interface materializes all matching entries". Drop "original" / "initial".
- `versioned.rs:33-37`: the list of implementations by name in the contract crate's module doc. DELETE; it names types of two downstream crates and goes stale with them.

**C9. Docs that only repeat the item name** (about 30 lines; DELETE, or keep deliberately as a house rule): `versioned.rs:589` "Creates an error.", `:1070` "Creates zeroed counters.", `:955-962` "Read calls." / "Bytes read." / "Write calls." / "Bytes written."; `batch.rs:28` "Creates an empty batch."; `arkiv-db-memory/src/lib.rs:26` "Creates an empty store."; `persistent.rs:28`, `:181`; `counting.rs:36`, `:109` "The shared counters."; `store.rs:336` "The configuration.", `:136-143` "Leaf pages." / "Branch pages." / "Entries."; `testing.rs:72-80`, `:124-131` ("`read`." "`write`." "`sync_data`." "`set_len`."), `:195` "An empty device.".

Not listed, because they explain a non-obvious why and should stay: `reconcile`'s "Durability after a failed sync" (`versioned.rs:903-913`), the lock-forwarding note on `CountingBackend` (`counting.rs:86-91`), every inline comment in `create_file` (`store.rs:853-870`), `recover`'s lock-order comment (`store.rs:1175-1180`), the `failed` flag doc (`store.rs:205-208`).

### II.3 [N4] arkiv-node deterministic core

*(`src/` = `crates/arkiv-node/src/`)*

**C1. Stale (comment contradicts the code).** safe
- `src/lib.rs:7` "`[`block`]: block format version 1" - `src/block.rs:47` `FORMAT_VERSION: u32 = 5`. TIGHTEN to
  "the block format ([`FORMAT_VERSION`])".
- `src/identity.rs:6-7` "its digest (`arkiv/tx/v1` or `arkiv/admin/v1`)" - `src/block.rs:67`
  `TX_DOMAIN = "arkiv/tx/v2"`. TIGHTEN to "([`crate::block::TX_DOMAIN`] or [`crate::block::ADMIN_DOMAIN`])".
- `src/block.rs:606` "`None` for a creation" - the code at 610 also returns `None` for `Transfer`. TIGHTEN to
  "`None` for a namespace creation and a native transfer".
- `src/block.rs:521-523` "One user operation of a transaction: an engine write plus ..." - `Transfer` and `Arkiv`
  are not engine writes (`to_write_op` panics for both, 642-643). TIGHTEN.
- `src/replay.rs:248-249` "(including protocol-2 terminal root)" - `PROTOCOL_VERSION` is 5 (`src/block.rs:58`).
  DELETE the parenthesis.
- `src/arkiv/schema.rs:402` "Production's natural-width stored value bytes" - no production caller exists (S3).

**C2. Milestone / step labels** (`rg -n -i 'N0b|step N[0-9]|milestone' crates/arkiv-node/src/{lib,exec,identity,replay}.rs`):
4, all DELETE. `src/lib.rs:2-3` "(node step N0b-3)"; `src/exec.rs:94-95` "(and, in step N4, for a durable commit)";
`src/identity.rs:23-24` "the node runtime (step N1) wraps it with the queue"; `src/replay.rs:12-13` "the durable
node (step N4)". None in the other ten files. safe

**C3. History narration.** safe
- `src/block.rs:38-58` `FORMAT_VERSION` / `PROTOCOL_VERSION` docs are a changelog ("version 1 carried bare values",
  "Versions 3 and 4 (earlier drafts of these) were never released"). TIGHTEN to what version 5 is; history belongs in
  `CHANGELOG.md`.
- `src/gas.rs:15-17` "It is the `glm-rent-v2` rate at 2-second blocks (8 gas at 1 gwei per 17,280 blocks of 2 s),
  now fixed per second so ..." and the tests at 338-339, 492-493 ("what glm-rent-v2 charged"). TIGHTEN to
  "20 gwei per unit per day; 7,300 gwei per unit per year". `src/gas.rs:136` "Counts remain model 3; this schedule
  gives its logical work Ethereum-sized units." TIGHTEN to "The GLM gas schedule: Ethereum-sized prices for the
  engine's operation classes."
- `src/arkiv/mod.rs:26-28` "Retired: live permanent entities are supported since protocol 5." TIGHTEN to "Reserved
  tag, never produced."
- `src/arkiv/tests.rs:81` "// No payload or content type: ..." and the test name at 481 (T8).
- `src/lib.rs:25` "The signed demo remains an independent volatile profile." DELETE or reword without "remains".

**C4. Field and accessor docs that repeat the name: 62 lines, DELETE** (no `missing_docs` lint: workspace
`[workspace.lints.rust]` only forbids `unsafe_code`, and e.g. `src/block.rs:1166-1167` and `src/exec.rs:417-421` are
already undocumented). safe
`rg -n '^\s*/// (The namespace|Record key|The header|Why|What to do|The operation|The recipient|The change|Its write receipt|Chain identity|Execution parameters|Committed or failed|The new owner|Value|The recorded (inputs|outcomes)|The height|The transaction|The cap|The rejection|The block.s header|The due count|The records deleted|The entry.s key|A broken invariant|Index of the operation|Its position|Its scope|The genesis|The chain id|The protocol version|Engine limits|The chain|The block height|Empty indexes)\.$' crates/arkiv-node/src`
- `src/block.rs` 26: 245 342 450 529 538 548 552 557 561 566 573 575 585 587 598 600 1171 1173 1181 1187 1196 1202
  1340 1426 1428 1430 ("The namespace." alone 8x).
- `src/exec.rs` 17: 102 115 156 161 163 170 172 174 176 181 190 204 206 213 250 265 268.
- `src/validate.rs` 11: 54 59 61 67 69 103 105 110 112 119 126. `src/params.rs` 5: 340 345 350 355 360.
  `src/identity.rs` 3: 72 137 209.
The comment budget is spent in the wrong place: `src/gas.rs:8-13` (`SCHEDULE_ID = 0x474c4d01`, i.e. "GLM" + 1,
explained only in `docs/glm-gas-v1.md:23`; `TX_INTRINSIC_GAS`, `ENTITY_OVERHEAD_UNITS`, `INDEX_OVERHEAD_UNITS`),
`src/accounts.rs:10-14` (`GAS_PRICE_WEI`, `MAX_ALLOCATIONS`, `BURN_KEY`) and every flag bit in `src/arkiv` are
consensus constants with no doc at all.

**C5. `src/block.rs` - each hash domain literal is written 3-4 times.** TIGHTEN, safe
Const (61-71), module table (16-23), fn doc (252, 361, 455, 851, 893, 1393) and `Header` field docs (351, 354); the
table also exists in `docs/block-format.md:549-554`. `identity.rs:6-7` (C1) shows how the copies drift. Keep the
module table; fn docs become "The [`TX_DOMAIN`] digest of the encoding."

**C6. Section banners:** `src/block.rs` 4 (132, 336, 405, 918), `src/exec.rs` 4 inside `impl Executor` (717, 745,
793, 909); 2 lines each, 16 lines, DELETE (the `exec.rs` ones go away with the split in S2). Other files: 0.

**C7. Misplaced or mis-aimed.** safe
- `src/exec.rs:453-454`: the doc of `execute_block_hooked` describes its `after_tx` parameter, not the fn.
- `src/exec.rs:1156-1158`: a 3-line comment sits between the operands of an `if` condition (1155-1159). Move above.
- `src/arkiv/tests.rs:370-371, 380-381`: production-style docs on test helpers that only test themselves (T3).
- `src/arkiv/schema.rs:33, 334, 414`: "// i64 and the golem-grid scalars are not SDK types." 3x; say it once on
  `ArkivTypeId`.

**C8. `impl Encode` layout docs in `src/block.rs`** (287-290, 367-368, 417, 461, 502, 697-698, 857, 899, 952, 993,
1030, 1062, 1123, 1222, 1309, 1354, 1399, 1441) restate the three to eight `encoder.*` lines below them. Keep those
that add something (899 "empty inputs are the 8 bytes of two zero counts", 287-290 version rejection, 1354 width of
`rent_wei`); the pure `a ‖ b ‖ c` ones (461, 502, 857, 1030, 1399, 1441) can go since `docs/block-format.md` is the
spec. Low priority.

### II.3 [S5] arkiv-db-trie

*(paths relative to `crates/arkiv-db-trie/`)*

**3.1 Stale**
- `tests/store.rs:23-26` (header): "hand-built tries ... (an extension over a leaf, a short node referenced by
  hash, a leaf at the wrong depth) are reported as inconsistent by the map and rejected by `walk`". For the
  wrong-depth leaf the code asserts the opposite: `store.rs:928`
  `assert_eq!(map.get(&key, &store, &mut Probe::new()), Ok(None));` and `store.rs:934`
  `assert_eq!(walk(root, &key, &[short]), Ok(None));` (only iteration errors, :929-933). TIGHTEN the header
  (and the test name `hash_valid_but_noncanonical_tries_are_inconsistent`) to say so.
- `tests/encoding.rs:145` "The planned term-key building blocks" - the engine's term key exists
  (`crates/arkiv-db/src/layout.rs:289 impl TrieKey for TermKey`) and does not use `KeyWriter`. Drop "planned".
- `src/lib.rs:37-39` + `Cargo.toml` description: the crate doc's feature list names only point proofs; the
  580-line interval module (7 re-exports, lib.rs:94-97), `explore` and `walk_stored` are not mentioned. Add one bullet.

**3.2 Milestone / history labels**
- `src/lib.rs:10-12`: "(node step N3a of `docs/node-storage-plan.md`; the engine uses it since N3b, and its
  persistence and proofs are N3c–N3d)". DELETE the parenthesis; keep "It is the one structure the engine's state is built from."
  (`rg -n 'N3[a-d]' crates/arkiv-db-trie` -> only these lines.)
- `tests/interval_model.rs:6`, `tests/interval_inspection.rs:7`, `tests/interval_security.rs:4`:
  "The closed-interval form these tests were written against." DELETE with the shims (1.1).
- `src/reach.rs:1-2`: "for storage reports and (later) the mark phase of history pruning". TIGHTEN to what it does today.

**3.3 Same rationale stated three times**
- Why staging does not stop at an id the sink already holds: `src/stage.rs:15-21` (module doc),
  `src/map.rs:505-507` (`stage` doc) and the inline comment `src/map.rs:871-875` (near-verbatim copy of the
  module doc). DELETE the inline comment or cut it to "// Keep walking: an equal id does not prove the nested maps were staged (see `stage`)."
- "handles/links/nodes hold no loader or store": `src/lib.rs:19`, `src/node.rs:38-40` (Link), `src/node.rs:106-108`
  (NodeBox), `src/loader.rs:15-19` (NodeLoader), `src/map.rs:162-165` (MerkleMap);
  "a failed read is never an absent key": `src/lib.rs:21-22`, `src/error.rs:10-11`, `src/loader.rs:39-40`, `src/map.rs:164-165`
  (`rg -n "no loader|hold no store|holds no store|never an absent|never reported as an absent|never means the key is absent" src`).
  TIGHTEN: keep in the crate doc and on `NodeLoader`/`TrieError`; drop the restatement from `Link`, `NodeBox`, `MerkleMap` (~8 lines).

**3.4 order.rs (25% doc lines): fn docs restate the module doc**
- Module doc `src/order.rs:7-25` already explains escape/shift, prefix-freeness and the prefix-run property.
  Restated in `escape_bytes` (:44-45), `shift_bytes` (:66-67, same sentence again) and `encode_shifted` (:80-85,
  "For UTF-8 text, which never contains f5–ff ... no escape can double ... The terminator is the smallest byte ...").
  TIGHTEN each fn doc to what/failure ("Appends every byte plus one, then a `00` terminator. Fails, appending
  nothing, if `value` holds `ff`."), keep the why in the module doc. ~10 lines.
- `src/order.rs:32-33` "the tests check the order claims ... on random inputs": the unit tests below are fixed
  vectors; the random ones are `tests/encoding.rs`. Name the file.

**3.5 Test-file headers: one-sentence "Establishes:" paragraphs of 13-26 lines**
- `tests/store.rs:1-26` (a single 23-line sentence), `tests/proofs.rs:1-21`, `tests/map.rs:1-15`,
  `tests/variable_keys.rs:1-16`, `tests/adversarial_bounds.rs:1-16`, `tests/encoding.rs:1-13`
  (`rg -n Establishes tests`). They enumerate what each test fn name already says, drift (3.1), and are duplicated
  again in the table at `docs/state-trie.md:430-434`. TIGHTEN to 3-5 lines naming the oracle and the scope
  ("in memory only; stored links are in store.rs"). ~70 lines.

**3.6 Small**
- `src/node.rs:285` `/// The shared encoders for persistent and streaming construction.` is a doc comment on
  `encode_leaf` used as a heading for three fns. Make it `//` or document each.
- Section banners: 0 in every file of this chunk.

### II.3 [S1] arkiv-db src: engine, typed, planner, record, layout, codec, meter

*(paths relative to `crates/arkiv-db/src/`)*

#### 3.1 Stale (comment contradicts the code) - fix first
- engine.rs:31-32 "with the key and leaf encodings of state format 1" - `STATE_FORMAT = crate::LAYOUT_VERSION` (state.rs:32) and `LAYOUT_VERSION: u32 = 2` (lib.rs:184); layout.rs:1 says "(state format 2)". TIGHTEN: drop the number, link `[`STATE_FORMAT`]`.
- engine.rs:69-70 "a test aid until node step N3c adds durable manifests", :484 "Test aid, until node step N3c opens durable manifests", :860 "Test aid, until node step N3c stages commits" - N3c has landed (README.md:170 "Durable engine storage (N3c). `durable::DurableEngine` persists..", docs/architecture.md:22 "N3c now integrates"); `from_state` is used only by tests (`rg "from_state\("`: tests/rehash, range_proofs, eq_proofs, storage_faults). TIGHTEN to "Test aid:" in all three. `rg -n "N3c" crates/arkiv-db/src/engine.rs`.
- engine.rs:80-81 "Durability is not implemented: nothing here survives process termination." - `crate::durable` builds on this file (`Snapshot::from_header` 705-738, the retention pin at 433). TIGHTEN: "This engine is volatile; `crate::durable` persists the same state."
- meter.rs:24-25 "`ProofNode`/`ProofByte` are reserved for point proofs (not charged yet)", :162-163 and :165-166 "Reserved; no operation charges it yet." - charged at proof.rs:817, proof.rs:861 and proof/range.rs:714. TIGHTEN: "Charged by proof reads (`crate::proof`); no block operation charges it."
- codec.rs:793-794 "the fixed 33-byte [`RecordKey::encoded`] form is for ordered keys" - no such method (`rg "RecordKey::encoded|fn encoded"` finds only this line); the trie key is the same variable `len ‖ bytes` (layout.rs:126-138). DELETE the second sentence.
- layout.rs:39 "attributes indexable and at most 128 bytes" - the check is `> Value::INDEXED_LEN_CEILING` (512) at layout.rs:529-533. TIGHTEN: "at most the indexed-string ceiling".
- layout.rs:1085 "Attributes must be indexable and at most 64 bytes; fields need not." - the test rejects `INDEXED_LEN_CEILING + 1` and merely accepts 64. TIGHTEN likewise.
- record.rs:11-12 "a later milestone hashes the same bytes into roots" - roots exist (`Snapshot::state_root`). TIGHTEN: "and the same bytes are hashed into the state root."
- typed.rs:872 `Source::Empty` "Nothing: the cursor is past every possible id." - also returned for a missing term (typed.rs:1038). TIGHTEN: "Nothing to yield."

#### 3.2 Milestone / history comments (mean nothing to a new reader)
- record.rs: the disclaimer "not a key width" four times: :129-130, :576-577, :825-826, :867 ("An explicit admission policy, no longer a key width."). Leftover of the padded-key layout. Keep the one in the `Limits` doc (825-826), DELETE the other three sentences.
- layout.rs:636-637 "the orders the engine relied on before it moved to tries", :774-775 "exactly the old derived `TermKey` order", :927 "the default instance bound equals the old fixed width". TIGHTEN to state the property without "before/old".
- meter.rs:733 test name `model_three_appends_four_classes..` and :753 "The version-2 weights are unchanged" (see section 2).
- codec.rs:1374 "(31 under counting-rule version 3)" and meter.rs:69 "(model version 3)": numbers that will rot beside `OperationClass::ALL.len()` / `MODEL_VERSION`. DELETE the parentheticals.
- rg: `rg -n -i "no longer a|before it moved|the old |later milestone|until node step|version-2" crates/arkiv-db/src/{engine,record,layout,meter}.rs`.

#### 3.3 Docs that restate
- engine.rs: 32 blocks "Fallible form of [`x`](Self::x)." (40 lines; `rg -c "Fallible form of" engine.rs`). They vanish with 1.1.
- Private/crate-private one-liners that repeat the fn name - DELETE: typed.rs:195 "Validates a create's cells.", :203 "Validates a patch's set and remove lists.", :789 "Reads one record by key.", :1143 "Opens one group as `evaluation` asks."; "Physical node visits of the .. traversals so far." three times (typed.rs:927, :1223, planner.rs:259).
- About 32 public one-liners of the same kind (`rg -n "^\s*/// (Returns|Reads|Validates|Opens|Creates|Wraps|Appends|The [a-z ]{3,30}\.)$"`), e.g. meter.rs:37 "Wraps a unit count.", :42 "Returns the unit count.", record.rs:162 "The name text.", :278 "The type id.", :606 "The value's type." No `missing_docs` lint is enabled, so they are optional; low priority, leave unless touching the item.

#### 3.4 Over-long prose - TIGHTEN
- engine.rs:873-883: an 11-line paragraph on `Snapshot::hashed_nodes`, a `#[doc(hidden)]` method with no caller (see 4.2). It re-explains the `Staging` doc (377-382).
- typed.rs: the "who notes the physical visits" invariant is explained four times: :954-957 (`open_run`), :1234-1236 (`Evaluator`), :1242-1245 (`Evaluator::open`), :1313-1323 (`evaluate`). Keep the `evaluate` version, reduce the others to "see [`evaluate`]". ~10 lines.
- typed/planner.rs:1-87: an 87-line module doc (9% of the file). It is the only spec of the planned charges, so keep the content; but :61-74 is repeated on `PostingCursor::seek` (201-205), `reposition` (231-232), `DENSE_RATIO` (114-117) and `DRIVER_GALLOP` (107-111), and :76-87 on `count` (942-945) / `closed_count` (796-799). Pick one home per rule.
- engine.rs:17-27 (list of metered entry points and statistics) is repeated almost verbatim in lib.rs:22-30, and "Fallible surface" (56-78) in lib.rs:49-53 and on `in_memory` (161-165). Keep engine.rs, shorten the others.
- record.rs:121: a 129-character doc line (the only line over 100 columns in the chunk outside markdown tables); re-wrap.

#### 3.5 Section banners
15 banners, 40 lines: typed.rs 4 (169-171, 331-333, 422-424, 658-660), planner.rs 4 (126-128, 280-282, 371-373, 835-837), layout.rs 2 (88-90, 366-368), codec.rs 5 two-line banners (499-500, 690-691, 831-832, 939-940, 1132-1133). engine.rs, record.rs, meter.rs: none. They mark exactly the seams of the module split in 4.1; DELETE when splitting.

### II.3 [T5] arkiv-db tests: durable / faults / KV; examples

*(paths relative to `crates/arkiv-db/`)*

1. **tests/rehash.rs:1-45, stale numbers in the module doc** (the code and docs/state-trie.md agree with each other, the header does not). TIGHTEN to the current values:
   - Line 7-8: "raw records (21) 44 ... terms (88) 178". Code: `MapKind::Records => 41` (line 67) and `MapKind::Terms => MAX_LEN + 3 + DEFAULT_INDEXED_LEN + 1` = 164 (lines 72-74), so 84 and 330; the test asserts `allowed(Terms) == 2 * 330` (line 728); docs/state-trie.md:156, 159 say 84 and 330.
   - Line 36: "214 terms force a 175-node path". The test asserts `1 + 1 + 130 + 2 + 2 + 2 * 41` = 218 (line 706); docs/state-trie.md:498 says 218.
   - Line 40-41: "adding a record to that term rehashes exactly 177 term nodes". The test asserts 176 (lines 726-727); docs/state-trie.md:491, 503 say 176.
   - Line 35-36: "deleting the deepest crafted key exactly 65". In the test, deleting that crafted key (last byte `01`) is asserted at 66 (lines 571-572), because the key inserted just before keeps the branch alive; 65 (lines 584-585) is the later removal of the inserted key `02`.
   - Line 507: "(the v2 encoding is `len ‖ bytes ‖ 0x00 pad`, 33 bytes, 66 nibbles)". `RecordKey::encode` is `len ‖ bytes`, unpadded, 2 to 33 bytes (src/layout.rs:126-137; src/lib.rs:181-183 "Version 2 stores term, record and raw keys unpadded"). The derivation only uses 32-byte keys, so drop "‖ 0x00 pad" and the "…" after `01 00` on line 511.
   **safe**.
2. **Milestone / step labels** (`rg -n "node step|milestone-|N3b|N3c|N0b" crates/arkiv-db/tests crates/arkiv-db/examples` finds, in my files): tests/storage_faults.rs:1 "(node step N3b)" and :40 "(node step N3c)"; tests/rehash.rs:1 "(node step N3b)"; tests/fallible_api.rs:1 "(node step N0b-2)" and :15-16 "(node step N3b's `Engine::from_state` test aid)"; examples/blocks.rs:1 "The milestone-1 flow:"; examples/records.rs:1 "The milestone-2 flow:". DELETE the labels (e.g. "Storage failures beneath engine state.", "The block flow: two namespaces, ..."). **safe**.
3. **Temporal wording**: tests/durable_retention.rs:3 "Nothing is deleted in this phase." -> "Retention only refuses reads below the floor; no object is deleted."; tests/model.rs:47-48 "A block is test workload metadata for now ... It does not yet retain block snapshots." -> "A block is one `WriteBatch`; the facade exposes only the resulting head."; tests/durable_failures.rs:314 (deleted with 2.3). **safe**.
4. **tests/storage_faults.rs:678-679, stale doc**: "One namespace holding records created in `order`, the first deleted again, so the survivor has id 2." The parameters are `first` and `survivor` (line 680); there is no `order`. TIGHTEN: "One namespace where `first` is created and deleted again, so `survivor` has id 2." **safe**.
5. **Over-long module docs that restate the test names**: tests/storage_faults.rs:1-40 (40 lines for 7 tests; the five "What this file establishes" bullets are the seven test names in prose) and tests/fallible_api.rs:1-17. TIGHTEN storage_faults to the first paragraph (how the reopened engine is built and what the reader controls, lines 3-9) plus the last one (what is not covered): about 12 lines. tests/rehash.rs:1-45 is long but mostly a non-obvious why (what "hashed" and "staged" count); keep after fixing item 1. **safe**, ~30 lines.
6. **tests/support/durable_history.rs:329-330** - `compare(&memory.snapshot(id).unwrap(), &snapshot);` twice in a row with no comment. If the second pass is there to read through the warm cache (the loop is over cache sizes), say so in one line; otherwise delete it. **safe**.
7. **Section banners** (`rg -c '^\s*// ?-{6,}' <file>`): tests/storage_faults.rs 10 lines (5 banners: 68-70, 151-153, 225-227, 424-426, 629-631); tests/rehash.rs 10 (5 banners: 79-81, 314-316, 366-368, 502-504, 592-594; 314-316 is `// Helpers`); tests/durable_failures.rs 2 (1 banner: 1234-1236); examples/records.rs 12 (6 banners at 38-40, 171-176, 279-293, 326-328, 409-412, 449-460; four of them frame real derivations, drop only the dashes). 34 dashed lines in the chunk.

### II.3 [NT1] arkiv-node tests: core

*(`T/` = `crates/arkiv-node/tests/`)*

1. `T/block_model.rs:86-92` - STALE / misplaced. The five-line doc "The block budget is `user_budget + expiry_per_block x` ... so 50,000 + 3 x 546 = 51,638." sits on `const SPILL_WINDOW`, glued to that constant's own one-liner (:91); `const PARAMS` (:94), which it describes (`block_budget: 51_638` at :95), has no doc. Fix: move :86-90 onto `PARAMS`, leave :91 on `SPILL_WINDOW`. **safe**.

2. `T/block_format.rs:1` - STALE. "Golden vectors for block format version 1." The file encodes `be32(5)` (:159, :257) and rejects version 1 (:210); `FORMAT_VERSION = 5` (`crates/arkiv-node/src/block.rs:47`). TIGHTEN to "Golden vectors for the block format (`FORMAT_VERSION`)." **safe**.

3. "the brief" - 7 occurrences, `rg -n "the brief|The brief" crates/arkiv-node/tests/block_model.rs` -> :15, :21, :23, :38, :325, :644, :1378. A task brief no reader has. TIGHTEN to the documents that exist: `docs/snapshot-query-plan.md` (charge table) and `docs/block-format.md` (expiry, reservation, due schedule). **safe**.

4. Version-history comments, all describing what a version used to be or introduced; they go stale at the next bump exactly as #2 did:
   - `T/block_format.rs:157-158` "Format 5: chain parameters carry the rent clock and the storage cap, outcomes the rent." and `:160-161` "Protocol 5: user-defined columns, live permanent entities, ..." -> TIGHTEN to `// format version` / `// protocol version`.
   - `T/block_format.rs:209` "Format 1 is the retired bare-value entity operation layout.", `:221` "Protocol 1 is the retired payload entity model." -> DELETE (the loop rejects seven values; the comment explains one).
   - `T/genesis.rs:306-307` "Protocols 1 (payload entities) and 2 (uncapped rent) are retired; 3 and 4 (earlier drafts of the rent rules) were never released." -> TIGHTEN to "Only protocol 5 is accepted."
   **safe**.

5. `T/gas_execution.rs:1147-1149` - "The reviewer's case: `[create E, patch E]` in one transaction now quotes the patch on the create, so ..." - past-refactor narration ("the reviewer's", "now"). TIGHTEN to "`[create E, patch E]` in one transaction: the quote covers the patch on the create, so 32 senders and a heavy prefix build every block in one execution." **safe**.

6. `T/gas_execution.rs:317` - STALE. "A chain funding the proposer and `tight` with exactly `balance`." The parameter is `funds` (:318). **safe**.

7. `T/support/mod.rs:247-248` - STALE. "walked page by page with the instance's largest page." `page` is a parameter (:254); the caller chooses it (`T/block_model.rs:1363`). TIGHTEN to "walked `page` rows at a time." **safe**.

8. The note "namespace 1 is the Arkiv entity namespace, generic operations use namespace 2" is restated five times: `T/validation.rs:3-4`, `T/selection.rs:19-20`, `T/exec.rs:35-37`, `T/replay.rs:19-21`, `T/block_model.rs:68-70`. Fix: say it once in `T/support/mod.rs` on two constants (`ENTITY_NS: u64 = 1`, `GENERIC_NS: u64 = 2`) and delete the copies; the bare `2` in about 100 call sites (replay.rs ~64, validation.rs ~24, exec.rs ~11, selection.rs 3) then has a name. ~10 comment lines. **needs-care** (many call sites if the constant is adopted; comment-only variant is safe).

9. Section banners (`rg -c '^\s*// -{10,}'`): block_model.rs 8 (:113, :377, :562, :578, :641, :1190, :1274, :1393), block_format.rs 3 (:31, :100, :192), replay.rs 1 (:168, a bare rule with no label). The other nine files: 0. In block_model.rs the banners are doing the job of modules (see 4.2). DELETE once split; ~24 lines with their label lines.

10. `T/block_model.rs:1-38` - the module doc describes only the seeded-history test; nothing mentions the third test (:2088, the 200-block demo against independent roots and proofs) or the `root_model` submodule. Add two lines or move that test (4.2).

11. `T/accounts.rs` and `T/gas_execution.rs` have no `//!` module doc; the other eight test files and both support files have one.

### II.3 [S2] arkiv-db src: durable, proofs, small modules

*(paths relative to `crates/arkiv-db/src/`)*

#### C1. `lib.rs` crate docs contradict the code in four places (**safe**, TIGHTEN)
- lib.rs:5-7: "[`Database`]: ... It is the storage foundation and the boundary a future durable backend
  implements." The durable backend exists and is built on `arkiv_db_storage::versioned::VersionedStore`
  (durable/mod.rs:23-25, 447-451). `rg -nw "KvStore|Database" crates/arkiv-db/src --glob '!lib.rs'` finds only a
  doc mention at engine.rs:21. Rewrite as "a raw byte-key KV facade, unrelated to the engine" (or remove with S5).
- lib.rs:14 "integer ranges", lib.rs:66-67 "complete numeric ranges", proof.rs:1 "unpaged numeric ranges"
  (and engine.rs:780 "numeric range", outside this chunk). proof/range.rs:1-3 says "over any ordered (indexable)
  type", and `TypeTag::is_ordered` is `self.is_indexable()` (record.rs:389-391), so `Str`, `Bool` and the
  fixed-width byte types range too. Say "ordered ranges".
- lib.rs:69-70: "bounded encoded caching". The cache holds decoded nodes: durable/cache.rs:1 "The bounded
  decoded-node cache".
- lib.rs:71-72: "Broader proofs and durable node integration remain separate work." `DurableEngine` is used by
  seven arkiv-node source files (`rg -l DurableEngine crates/arkiv-node/src`). DELETE the sentence.
- lib.rs:66-68 is also ungrammatical ("..., and stateless verifier, with separately versioned ...").

#### C2. Milestone label and two stale "test aid" notes in `state.rs` (**safe**)
- state.rs:181-183: "A test aid until node step N3c stages commits; see `Snapshot::staged_objects`." "N3c" means
  nothing to a new reader, and commits are staged today by `durable::prepare::Staging` (prepare.rs:104-184).
  TIGHTEN to "One trie node that exists only in memory. Returned by the hidden test aid
  `Snapshot::staged_objects`." The same label is at engine.rs:69, 484 and 860 (`rg -n "N3c" crates/arkiv-db/src`: 4 hits).
- state.rs:122-124: "A test aid; see `Snapshot::try_namespace_roots`." `NamespaceRoots` is a field of the public
  `EqInspection` (proof.rs:292) and `RangeInspection` (proof/range.rs:244) and is imported by
  `crates/arkiv-node/src/simulator/inspection.rs`. DELETE "A test aid; see ..".

#### C3. Doc comments that restate the item (`query.rs` is the worst; **safe**, DELETE)
The crate has no `missing_docs` lint (`rg -n missing_docs Cargo.toml crates/arkiv-db` is empty) and most of
`durable/` is undocumented, so nothing forces these.
- query.rs, 23 lines: 39, 47, 55, 64, 71 (`/// Attribute name.` five times), 49 (`/// Typed value.`),
  57 (`/// Lower bound.`), 59 (`/// Upper bound.`), 73 (`/// Required type.`), 79 (`/// Equality on \`name\`.`),
  89 (`/// A range on \`name\`.`), 94, 123 (`/// The predicate.`), 130, 138, 178 (`/// No cells.`),
  190 (`/// Return the key.`), 192, 275 (`/// Engine instance.`), 277, 323, 328, 333.
  Keep 41, 44-45, 52-53, 62, 66, 119-120, 147-149, 186-187, 284-286, 291-299, 339-343: they carry semantics.
- query.rs:215: `/// The key plus the named cells.` on `named(key: bool, ..)` is wrong when `key` is false.
  TIGHTEN to "The named cells, and the key if `key`."
- Six more of the same kind: host.rs:256 (`/// Creates an empty table.` on `BlockMap::new`), 314
  (`/// Number of recorded blocks.` on `len`), 319 (`/// Whether no block has been recorded.` on `is_empty`);
  namespace.rs:130 (`/// Wraps raw principal bytes.` on `OwnerId::new`), 135 (`/// Returns the raw principal bytes.`
  on `as_bytes`), 170 (`/// Immutable identity.` on `id: NamespaceId`).

#### C4. Broken rustdoc links in `durable/adopt.rs` (**safe**)
adopt.rs:498-499 and 510-511 split an inline link across two lines:
```
/// checked as [`initialize_simulator_from_staged`]
/// (Self::initialize_simulator_from_staged) checks it while copying:
```
Markdown needs `(` directly after `]`. As written, rustdoc sees a shortcut link it cannot resolve and prints the
parenthesis as text. Fix: use `[`initialize_simulator_from_staged`]` in the prose and add one reference line
`/// [`initialize_simulator_from_staged`]: Self::initialize_simulator_from_staged` at the end of the doc block.

#### C5. Doc comment on the wrong item (**safe**)
durable/streaming.rs:22-25 documents the cache ("A direct-mapped cache bounds duplicate posting work ... A
collision only repeats a traversal ...") but is attached to `const POSTING_CACHE_SLOTS`. `struct PostingCache`
(streaming.rs:35) has no doc. Move the first three sentences to the struct; leave "At 48 bytes per slot ...
768 KiB" on the constant.

#### C6. `durable/bulk.rs` needs about six "why" comments, not fewer
706 non-test lines carry six doc comments and no line comment. A reader has to reverse-engineer:
- bulk.rs:58-87: the run-file frame (`u32 len ‖ key ‖ u32 len ‖ value`, EOF only between entries).
- bulk.rs:153-159: why integrity is checked only when `read` returns 0.
- bulk.rs:193: why an entry is charged `size_of::<Entry>() * 2`.
- bulk.rs:393: why `cap = sort_buffer_bytes / 4` (four sorters share the buffer).
- bulk.rs:471, 487, 584, 590: the host-run value tags `0` (value) and `1` (count ‖ cap), and why 17.
- bulk.rs:340: why `+ 64` (see D4).
One line each.

#### C7. Prose that will date or drift (**safe**, TIGHTEN)
- durable/retention.rs:49-50: "`Window(0)` is head-only, like golem-db today." Drop "like golem-db today".
- proof.rs:676-677: "(329 for the default limits)". A hand-computed number nothing checks. Drop the parenthesis
  (docs/eq-proof-charges.md:98 already states it).
- proof.rs:12 and 38: "model-3 write/read rules are unchanged", "Model-3 writes are unchanged". Link
  [`MODEL_VERSION`](crate::MODEL_VERSION) instead of the bare 3.
- proof/range.rs:11-15: the comments on `RANGE_VERSION` / `RANGE_PROOF_VERSION` narrate layout history
  ("version 2: unpadded term keys and inclusive/exclusive endpoints") and do not say that one constant versions
  the request and the other the response. Rename (Other W5) and TIGHTEN to one line each.

Section banners (`// -----`): 0 in all 20 files (`rg -c "^\s*// ?[-=]{4,}"` is empty).
Leftover debug output (`println!`/`eprintln!`/`dbg!`): none. Lines over 120 characters: none.

### II.3 [S4] arkiv-db-redb tests and examples

*(paths relative to `crates/arkiv-db-redb/`)*

Stale (quote + contradicting code):
1. tests/common/mod.rs:181-182 `/// Asserts that every object and record of `batch` is stored and the head
   is at least its new head.` - the body (183-200) only reads objects and records; `store.head()` is never
   called. TIGHTEN to "Asserts that every object and record of `batch` is stored." **safe**.
2. examples/key_size_report.rs:280-281 `// Range proofs over the first attribute, from each sampled value up
   to (excluding) the next larger sample, capped by the proof limits.` - the request at 294-295 is
   `lower: Bound::Included(v), upper: Bound::Included(v)`, a point range. TIGHTEN to
   "Range proofs for the point range [v, v] of sampled values of the first attribute." **safe**.
3. tests/isolation.rs:25-27 `...and the cost of the shared catalog object table, which the engine does
   not use yet (N3).` - the engine writes `SpaceId::Catalog` now (crates/arkiv-db/src/durable/prepare.rs:133,
   crates/arkiv-db/src/engine.rs:337), and "N3" is a plan label. TIGHTEN to "...and the cost of the shared
   catalog object table." **safe**.
4. tests/common/mod.rs:1 `//! Helpers shared by the redb experiment tests and `examples/redb_experiment.rs`.`
   - also included by engine_storage_experiment.rs:6, reachability_report.rs:20, durable_throughput.rs:12.
   TIGHTEN to "...and the examples". **safe**.
5. tests/common/crash.rs:16-18: `pub const SEEDS` has no doc and the next line is
   `/// The same for the `#[ignore]`d heavy run.` - "the same" as what. Write
   `/// Torn-subset seeds per granularity in the default runs.` on `SEEDS` and
   `/// ... in the `#[ignore]`d heavy runs.` on `HEAVY_SEEDS`. **safe**.

Restating the code:
6. Field/const docs that repeat the identifier (DELETE; the neighbouring structs `Tally`, `Sweep`,
   `CreationImages.calls` carry none and read fine): tests/common/isolation.rs:26 `/// Namespace A.`,
   28 `/// Namespace B.`, 34 `/// Objects in A.`, 36 `/// Bytes per object.`, 56 `/// B's objects.`,
   60 `/// Committed blocks.`, 62 `/// File size.`, 66 `/// Tables in the file.`, 68 `/// A's table height.`;
   tests/common/recovery.rs:13 `/// Quick repair on every commit.`, 18 `/// Blocks committed.`,
   36 `/// Commits averaged.`; tests/common/amplification.rs:15 `/// Blocks committed.`,
   21 `/// File size at the end.`, 23 `/// Total commit time.`; tests/common/mod.rs:229 `/// Wall-clock timer.`
   16 lines. Keep the ones that add a unit or a definition (e.g. isolation.rs:38-39, recovery.rs:15-17, 22-26).
7. examples/bench_readers.rs:203 `// Namespace.`, 247 `// Readers.`, 285 `// Producer (measured).` -
   labels for the three phases of a 179-line `main` (182-360). DELETE when the phases become
   `create_namespace`, `spawn_readers`, `run_producer` (4.11).
8. tests/atomicity.rs:70, 164, 172: the same `/// `cargo test -p arkiv-db-redb --release --test atomicity -- --ignored``
   three times; the `#[ignore = "heavy: .."]` reason already says why. Keep one in the module doc. 2 lines.

Missing where a reader needs it (the opposite problem, same files): tests/engine_support/mod.rs has no
module doc and no doc on `base` (85), `candidate` (98) or `check` (161), the oracle of every engine crash
test; tests/streaming_import.rs has no module doc. One line each, e.g. on `check`:
"Snapshot 1 must equal `parent`; the head must equal `parent` or `candidate` in every observable."

Module docs of atomicity (47 lines), reconciliation (33), disk_full (29), isolation (27), initialization
(26), latching (25): long, but they state the device model, what is and is not established, and scope.
Not listed for change.

### II.3 [D2] Scripts, CI, build config, vendor/reth-trie, conformance

*(repo-relative paths)*

- **scripts/bench-compare.sh:241** - stale. Comment: "--full-large together with a filter: also allow large/* ids by name."
  The code under it (242-244) only dies when nothing was selected; the large ids are handled by `pool+=` at :227. DELETE. safe
- **scripts/bench-compare.sh** - 9 section banners (`rg -c '^# -{8,}' scripts/bench-compare.sh`; lines 21, 134, 181, 255, 290,
  550, 605, 674, 793). TIGHTEN to plain `# Logging`-style headings or leave; bash convention, low priority.
- **scripts/sdk-side-by-side-bench.ts:51, 116** - `// ---- writes ... ----`, `// ---- reads ----` banners. DELETE.
  **:53-55** narrates a past run ("which is what the first run needed: the node then admitted at most 16 entities expiring
  at one block and failed the rest"). TIGHTEN to: "SAME_LIFETIME=1 gives every entity the same expiry
  (ExpirationTime.fromHours(2)); otherwise each gets its own expiry height." The env list in the header (:4) omits
  `SAME_LIFETIME` and `LIFETIME_BLOCKS`, which are read at :56-57: add them and move the three late constants up to 13-17.
- **scripts/run-full-prefill.py:6-7, 163-164, 172** - "restores the older independent producer rebuild", "the default keeps
  the original command line", "The default policy keeps the original converter command line." Past-tense rationale that
  means nothing to a new reader. TIGHTEN to what the flag does ("--rebuild-genesis-in-producer: the producer rebuilds the
  state from the stream instead of installing the built one"); DELETE the two inline ones.
- **scripts/sdk-compat-smoke.py:4-7 and scripts/sdk-compat-smoke.ts:4-7** - a tombstone: "PRE-UDC ... expected to fail
  against them; a separate UDC smoke replaces it." A file whose header says it is expected to fail should be deleted, not
  annotated (S2a).
- **scripts/udc-sdk-smoke.ts:214** - "The SDK no longer writes permanent entities." TIGHTEN to "SDK 1.0.x cannot write
  permanent entities." **:60** - `// biome-ignore lint/suspicious/noExplicitAny` for a linter this repo does not configure
  (`rg -l biome` hits only this file). DELETE unless the file is linted in the SDK checkout.
- **"since state layout 2"** - conformance/golem-db/src/arkiv.rs:267-268 ("the generic profile ceiling of state layout 2",
  followed by a broken reflow: "is refused rather" / "than silently narrowed" at 268-269), golem_tests.rs:975-977,
  conformance/golem-db/README.md:212. Version history inside a rule statement. TIGHTEN to "512-byte indexed values" and
  re-wrap arkiv.rs:261-270.
- **conformance/golem-db/src/golem_tests.rs** - 7 banners (`rg -c '^// ---- ' ...`; lines 664, 746, 873, 1020, 1153, 1318, 1437).
  These name the upstream file each block was ported from, so they carry information: KEEP, or turn each into a `mod`.
  The `use arkiv_db_redb::...` pair in the middle of the file (1155-1156) should move to the top with the other imports.
- **scripts/sourcify-v4-prefill.py:2-66** - a 65-line module docstring that restates docs/sourcify-v4-prefill.md; it is
  also the `--help` text (:1042). A stray blank line at :47 splits the bullet list in two. TIGHTEN to the wire layout
  (15-24) plus a pointer to the doc, or keep and fix :47. Low priority.
- **scripts/genesis-bundle-smoke.py:479** - progress string "PASS live nonce0/creationnonce2, ordinary block1, transfers
  and restart" is shorthand only its author can read. TIGHTEN ("PASS live create (creation nonce 2), transfer, restart").
- **scripts/full-prefill-bootstrap.py, scripts/test-full-prefill-bootstrap.py** - no shebang and no module docstring; every
  other script has both. Add one line saying it is the container entry point of the full-prefill job.

### II.3 [NT2] arkiv-node tests: signed

*(paths relative to `crates/arkiv-node/tests/`)*

No section-banner comments in any of the ten files (rg `^\s*// ?[-=]{4,}`: 0). Comments are sparse and mostly explain a why. The exceptions:

- signed_reads.rs:12-26 - TIGHTEN. The "What this file establishes" list is the five test names again in prose, one bullet per test
  (bullet 1 = :381, 2 = :471, 3 = :546, 4 = :633, 5 = :717). Keep :1-10 (how the fixture makes a read slow, which is not obvious) and move each
  bullet to a `///` on its test or drop the list. -15. safe.
- arkiv_profile.rs:1 - stale/misleading: `//! Isolated Arkiv entity producer/full/light acceptance. No running service or network.` but :246-494
  bind loopback sockets and drive two axum servers with reqwest (:249-256, :282-290, :457-477). TIGHTEN to
  "Arkiv entity profile on the simulator stack: producer, full and light, in process (loopback HTTP only)."
- signed_reads.rs:326-327 - `/// Writers take the node lock without waiting, as before detached reads, and answer ...`: "as before detached reads"
  describes a past design. TIGHTEN: "Writers never wait for the node lock: they answer `ServerBusy` while a request holds it, and clients retry."
- signed_reads.rs:371 - `/// The same read served the old way: planned and run under the node lock.` TIGHTEN: "The reference answer: the same read planned and run under the node lock."
- signed_reads.rs:145 - `/// Prefilled entity `n`: attribute `n` holds `n % 4`.` sits on `entity_key`, which returns a key; the sentence describes
  the rows built at :188-206. Move it there; on the fn: "The key of prefilled entity `nonce`."
- signed_protocol.rs:139 - `// These bytes are built independently of the new types' encoders.` ("new" relative to nothing). TIGHTEN: "Built by hand, not with the `Encode` impls."
- signed_protocol.rs:58-59 - `Chains created with the earlier bound of 16 remain valid.` History; the table row `(16, true)` is not special any more
  (1 and 128 are valid too). TIGHTEN to the first sentence, or DELETE the second.
- signed_host.rs:463-464 - `instead of a misleading grep over engine pages that intentionally include entity state` argues with a discarded approach.
  TIGHTEN: "Decode the host packet exactly; engine pages hold entity state, so searching them proves nothing."
- signed_pool.rs:123 - trailing `// repeated construction is non-destructive` is attached to `assert_eq!(pool.len(), 4)` but describes the two
  `select` calls on :124-126. Move above :124.

### II.3 [N1] arkiv-node signed JSON-RPC / HTTP reads

*(paths relative to `crates/arkiv-node/`)*

This chunk is mostly good: comments explain why (retention floors, cursor leases, bounded work per tick). No section banners in any of the nine files (`rg -c "^\s*// ?[-=]{4,}"`: 0). The tells are few:

- `range_api.rs:1` `//! Small, unpaged range-proof transport. The text query language is unchanged.` - "unchanged" relative to a past change. TIGHTEN to the first sentence.
- `reads.rs:48` `Metering, budgets and every successful response are unchanged.` - same. DELETE the sentence.
- `rpc_reads.rs:11-14` `Both evaluate with planned budgets ... so the clause order of a query no longer decides its cost, and the rows, cursors and counts are exactly the written evaluation's.` - stale and refactor-relative: the paragraph above (`:7`) lists three methods (`arkiv_query`, `arkiv_getEntity` and `arkiv_getEntityCount`), so "Both" has no antecedent; "no longer" narrates history. TIGHTEN: "All three evaluate with planned budgets ([`Budget::planned`]): each AND-group is driven by its cheapest index source and counts use stored posting sizes, so clause order does not decide a query's cost."
- `reads.rs:4-6` lists the detached reads as "`arkiv_query`, `arkiv_getEntityCount`, equality and range proofs, the state tree explorer"; `rpc_reads.rs:286-291` `detached` also has `arkiv_getEntity`. Add it.
- "the removed `payload` and `contentType`" / "The removed payload model": `rpc_reads.rs:541-542`, `:1855`, `:2298`, `rpc.rs:2262`. A new reader has no "payload model" to remember. TIGHTEN to state the rule ("Any other key is refused", "`payload`/`contentType` are not select keys"); the tests themselves are fine as unknown-key cases.
- `rpc.rs:1310-1311` repeats the doc comment of `SERVER_BUSY_CODE` 37 lines above (`:1272-1273`) almost word for word. DELETE.
- `rpc_ws.rs:40` `// None denotes newHeads, Some denotes logs with address/topics filters.` - a comment standing in for a type. `enum Subscription { NewHeads, Logs(Value) }` (plus `next_height`) removes it and the three `filter.is_some()` / `if let Some(ref filter)` reads (`:140, 222, 243`). **safe**.
- `reads.rs:1-48`: 48 lines of module prose for 160 lines of code. It is accurate and explains real design (the redb epilogue-commit paragraph `:29-35` is operational knowledge, keep), but `:19-27` and `:11-17` could each lose a sentence. Optional.

### II.3 [N3] arkiv-node binaries, genesis_*, query_language

*(paths relative to `crates/arkiv-node/src/`)*

No `// -----` banners in the bins or genesis files. Box-drawing banners `// ── … ──`:
query_language/literal.rs 2 (:247, :394), query_language/parse.rs 7 (:565, 689, 784, 896, 925, 976, 1041)
(`rg -n '──' crates/arkiv-node/src/simulator/query_language`); upstream style, leave with the vendored code.

1. genesis_bundle.rs:1 - STALE. `//! Bounded, deterministic height-zero entity builder, protocol 4.` but
   block.rs:58 has `pub const PROTOCOL_VERSION: u32 = 5;` and no constant in the crate is a protocol 4.
   TIGHTEN to "Small (<= 256 rows) height-zero prefill: row codec, launch commitment and the live-execution builder."
2. genesis_streaming.rs:1 - STALE. `//! Version-two file-backed prefill …` while the file builds and verifies
   versions 2 and 4 (:516 `PrefillLaunch::version_for(true, permanent)`). TIGHTEN to "File-backed (`ARKPFL02`) prefill, launch versions 2 and 4".
3. genesis_streaming.rs:334-335 - MISPLACED/STALE. "Retain the CLI byte option as a bounded ingestion tuning
   input. It never selects transaction replay or changes canonical state." sits on a `#[cfg(test)]` wrapper.
   DELETE with the wrapper (Duplication 3).
4. genesis_streaming.rs:479-480 - CONTRADICTS the next lines. "pricing and expiry reservation must already be
   selected by the caller's native genesis/profile constructor", then :482-487 compute the expiry reservation
   and raise `block_budget` to it. TIGHTEN to what the code does: "A streaming launch always uses the widest
   cell profile; raise the block budget so the expiry reservation still fits." `[CONSENSUS]` (comment only).
5. genesis_bundle.rs:19-20 - CONFUSING. "Codecs 1 (`ARKPFL01`) and 2 (`ARKPFL02`) carried a payload and
   content type and are refused." Both magics are still the accepted framings (:229, :251,
   genesis_streaming.rs:120); only row-codec numbers 1 and 2 are refused. TIGHTEN to "Row codecs 1 and 2
   (payload and content type) are refused."
6. genesis_bulk.rs:142-143 - HISTORY. "A separate marker prevents old incremental CURRENT/checkpoints from ever
   being interpreted as bulk output." TIGHTEN to "`bulk-v1/` keeps bulk output apart from anything else in the
   work directory. The lock also covers scratch deletion." (the regression test at genesis_streaming.rs:1585 stays).
7. genesis_prebuilt.rs:548, :629 and the test name at tests/prebuilt_genesis_cli.rs:236 call the default
   rebuild path "legacy". It is the default, not a legacy path. TIGHTEN to "the rebuild path".
8. genesis_authoring.rs:1672 - HISTORY in a test: "The removed payload model is not a spelling of anything."
   TIGHTEN to "Unknown keys are refused."
9. query_language/literal.rs:241-242 - STALE first line. "A bare decimal for a system attribute — a `u64`
   block height as a `u256`." is followed by "An untagged number literal — always `i32`" on a function that
   returns `AttributeValue::Int`. DELETE :241.
10. query_language/parse.rs:406-407 - two summary lines for one function ("The type a system attribute is
    fixed to carry." / "A system attribute's type **as the client writes it**."). DELETE :406.
11. Doc references to things that exist only upstream: query_language/ast.rs:47 "the `annotations` constants",
    ast.rs:68 "`AttributeValue::index_bytes`" (`rg index_bytes crates` -> this line only), parse.rs:424
    "`annotation::entity_annotations`", parse.rs:427 "`arkiv-engine.md` §2", literal.rs:88 lists `$updatedAt`
    as taking `u64(..)` although parse.rs:497-500 rejects it. Either leave as vendored text or TIGHTEN to the
    README wording ("this module does not implement the source index encoding").
12. bin/arkiv-proposer.rs, bin/arkiv-simulator.rs, genesis_adopt.rs, genesis_prebuilt.rs, retention.rs: the
    comments explain why (crash ordering, trust boundary, floors). Keep.

### II.3 [NT3] arkiv-node tests: genesis / simulator / demo / CLI; examples; Cargo.toml; clippy.toml

*(paths relative to `crates/arkiv-node/`)*

Section banners (`// ----`): **0** in all eleven files (`rg -c '^\s*// ?[-=]{4,}'`). Comments in this chunk mostly explain a why; only these:
- `tests/genesis_prefill.rs:164-165` "Beyond max_lifetime is no longer too far, and every finite expiry ..." - past-refactor wording. TIGHTEN to "An expiry beyond `max_lifetime` is accepted; every finite expiry at or beyond the rent cap burns the same capped rent." (and replace the magic `6_307_201` at :172 with `base().genesis.params.max_lifetime + 1`; `src/signed/protocol.rs:143` sets 6_307_200).
- `tests/genesis_prefill.rs:231-232` "Payload-era rows are not a spelling of anything: a stream with the old codec is refused, and so are the removed system names." - history jargon. TIGHTEN to "Row codec 1 and the `$payload` / `$contentType` system names are refused."
- `tests/prebuilt_genesis_cli.rs:3` and `:270` call the producer's rebuild "legacy" (also the bindings `legacy_node`, `legacy` 271-275 and the log tag); `tests/genesis_adopt.rs:152,353,587` call the same path "the default installation" / "the default copy installation". It is what runs when no flag is given, so "legacy" is wrong. TIGHTEN to "default rebuild" and rename the bindings.
- `tests/genesis_adopt.rs:401` `// The producer binary.` - a section label (the only one in the chunk). DELETE once the CLI half uses `cli_support`, or keep if the file stays in two halves.
- `clippy.toml:5-6` "Tests that inspect the memory engine may allow the lint explicitly." - stale: `rg -n 'disallowed_methods' crates/arkiv-node` finds no `allow`, and `tests/support/mod.rs:3-4` states the opposite ("bans the infallible forms everywhere, tests included"). DELETE the sentence.
- `Cargo.toml:32` "Independent Keccak-256 for the golden digests of `tests/block_format.rs`." sits above a dev-dependency that is already a normal dependency (line 10) and above `alloy-trie`, which it does not explain. See S3.
- `Cargo.toml:3` description ("block format, deterministic block execution, request identity, selection and replay") predates the three binaries, the signed proposer, the simulator and genesis tooling. TIGHTEN.

### II.3 [N2] arkiv-node runtimes: signed / simulator / demo

*(paths relative to `crates/arkiv-node/`)*

Comments here are sparse (392 comment lines in ~10k) and mostly explain a why. No section banners (0). Specific items:

1. signed/runtime.rs:78-82 - **stale**. "transaction admission and control try it once and answer `ServerBusy`".
   The code takes `rt.read_node()` for both (776-777 transactions, 958 control), which polls for up to
   `READ_LOCK_WAIT`; the only `try_lock` in the file is inside `poll_node` (227) and `refusal` (445). TIGHTEN to
   what happens, or restore the try-once behaviour.
2. signed/history.rs:137-138 - **misplaced**. "Idempotently persists the exact framed bytes. Different candidates at
   the same chain height never compete for a storage key." sits on `reference()` (139-142), which persists nothing;
   it describes `persist()` (144), which has no doc. Move it; give `reference()` "The reference `persist` would
   return for `bytes`, without writing."
3. simulator/wire.rs:262-263 - typo: "depth64 and8MiB output bounds". TIGHTEN: "depth 64, 8 MiB output".
4. simulator/host.rs:50-52 - sentences 2 and 3 ("Unindexed field values become bounded descriptors in the feed. Every
   published feed is separately capped at 4 MiB.") have nothing to do with the limits/params comparison under them.
   TIGHTEN to "Only the development workload's limits and params are supported."
5. simulator/runtime.rs:78-79 - doc of `parse_private_http` describes the system ("This never accepts peer URLs from
   public HTTP request bodies"), not the function. TIGHTEN: "Parses `SIMULATOR_ALLOW_PRIVATE_HTTP`: only `true`
   permits cleartext http to a non-loopback peer."
6. simulator/wire.rs:1 - module doc "Closed, payload-free explorer DTOs..." covers a third of the file (the rest is
   hex/caps/codec/proof limits). Fix by the split in the recommendation, or TIGHTEN.
7. simulator/runtime.rs:810 - "No light path reaches here." restates the `return` at 783. DELETE that sentence.
8. signed/host.rs:136 - "canonical v2 stream": "v2" means nothing without the genesis-stream format doc; name it
   (`genesis_streaming`) or drop "v2".
9. Style tell, not a defect list: 31 comments use the slogan form "X. Never Y." / "never ..." (`rg -n "//.*\b[Nn]ever\b"`
   over the chunk). Most state a real invariant; the module docs at demo/mod.rs:1, simulator/mod.rs:1,
   signed/history.rs:1 ("Presence is never inclusion."), simulator/profile.rs:1-2, demo/wire.rs:1 read as slogans
   rather than descriptions. Worth one editing pass for tone; no line is wrong.
