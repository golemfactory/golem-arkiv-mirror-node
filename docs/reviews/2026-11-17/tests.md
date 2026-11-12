# Code-quality review, 2026-11-17: Part II.2: unneeded or obvious tests

Reviewed at `e6c49e1`; line numbers refer to that commit. Parts: [summary](README.md) · [duplication](duplication.md) · [tests](tests.md) · [comments](comments.md) · [sloppiness](sloppiness.md) · [other](other.md) · [appendices](appendices.md)

## II.2 Unneeded / obvious tests


### II.2 [T3] arkiv-db tests: queries / records / pagination / terms

*(paths relative to `crates/arkiv-db/tests/`; `qm` = `query_metering.rs`)*

No finding in this file is tagged `[CONSENSUS]`: none touches production code. 2.1, 2.2, 2.3, 2.6 and 4.6 edit tests that pin receipts or ledgers and are tagged needs-care for that reason.

Nothing below asks to drop a pinned receipt, ledger, encoding or cursor rule without naming the place where the same literal stays pinned. "DELETE" is used only where I read the surviving test and found the same operation on the same fixture with the same numbers; "MERGE" means move the literal vector into the named test, then delete the shell.

#### 2.1 `records.rs` sections 2-4 duplicate `query_metering.rs` on the same fixture - needs-care, biggest payoff (~900 lines)

Both files build the identical fixture (`records.rs:142-149` `fixture_cells` == `qm:355-362`; records a, b, c = `qm:365-366`), both sweep every budget against a model, and both spell the same derivations in comments. `qm`'s harness is strictly stronger: `assert_write_vector` (`qm:808-900`) runs budgets `0..=total+1` traced and untraced, checks the ledger prefix at every abort point, the recorded receipt, the block total and the unchanged state; `records.rs`'s `assert_write_sweep` (539-595) runs `0..=total` untraced and checks receipt + model.

| `records.rs` test | lines | what it pins | where `qm` pins it | relation | verdict |
|---|---|---|---|---|---|
| `create_with_every_term_new_charges_lookup_record_group_then_each_term` | 877-933 (57) | create a: 1, 4, 139, 179 .. 379; ledger | `create_vectors_on_an_empty_namespace_match_the_worked_examples`, "create a" `qm:1029-1050`, ledger `qm:1117-1139` | same op, sums and ledger (one extra assert, 928-932, "the raw keyspace is untouched", is what `raw_bytes_and_typed_records_..` 2265 is about) | DELETE |
| `create_with_existing_terms_charges_creation_only_for_the_new_one` | 936-1002 (67) | create b 229 (sums + ledger), create c 257, 9 terms | same `qm` test: "create b" `qm:1052-1078`, "create c" `qm:1080-1105`, 9 terms `qm:1107-1115` | same ops and sums; b's ledger is checked at every budget by `ledger_after` | DELETE |
| `patch_changing_only_the_value_replaces_the_value_term_and_keeps_existence` | 1005-1086 (82) | a.price := I64(11): 1, 4, 82, 92, 132 + ledger; same value again: 82 + ledger | `patch_changing_only_a_value_swaps_the_value_term_and_keeps_existence` `qm:1580-1612` (same op, same sums); no-op rule: `patch_setting_an_identical_attribute_touches_no_term` `qm:1913-1933` (kind := "book", 77) | first vector identical; second is the same rule on another cell | DELETE lines 1005-1056; MERGE the 82 no-op vector into `qm:1913` (or drop it) |
| `patch_changing_the_type_replaces_both_terms_and_removes_are_charged_per_name` | 1089-1150 (62) | a.price := U64(10), remove [note, missing]: 1, 4, 83, 93, 103, 143, 183; ledger; `row.cells.len() == 3` | `patch_with_removals_charges_absent_names_and_reindexes_in_name_order` `qm:1706-1770` | same op, sums, ledger and the same `cells.len() == 3` assert | DELETE |
| `patch_turning_an_attribute_into_a_field_removes_both_terms` | 1153-1200 (48) | c.kind := field "pen": 1, 4, 75, 85, 95 + ledger | `patch_flipping_kind_in_both_directions_charges_the_index_each_way` `qm:1811-1880` (a.kind := field "book": 77, 87, 97) | same rule, other record | MERGE into that test |
| `patch_turning_a_field_into_an_attribute_inserts_both_terms` | 1203-1249 (47) | a.note := attribute "memo": 1, 4, 78, 118, 158 + ledger | same `qm` test, `qm:1883-1910` (note := attribute "xyz": 76, 116, 156) | same rule, value one byte longer | MERGE (or DELETE: the only difference is `"memo"` vs `"xyz"`) |
| `patch_removing_names_charges_each_name_and_only_held_terms` | 1252-1313 (62) | remove [missing] 69, [note] 62, [kind] 74 | `patch_removing_an_attribute_drops_its_value_then_existence_term` `qm:1776-1808` (remove [price]: 56, 66, 76); absent and field removal inside `qm:1706-1770` | same rules, other numbers | MERGE |
| `delete_charges_lookup_delete_group_then_each_held_term` | 1316-1384 (69) | delete a: 1, 4, 46, 56 .. 106 + ledger; c and b at 106; zero terms after | `delete_charges_the_removal_group_then_one_unit_per_held_term` `qm:2067-2118` (same sums, same ledger), a/b/c at 106 and zero terms `qm:2153-2162` | identical | DELETE |
| `already_exists_is_charged_after_the_key_lookup_and_recorded` | 1391-1419 (29) | create of bound a: 4, ledger, recorded, estimate == execute | `create_of_a_bound_key_is_already_exists_after_the_binding_lookup` `qm:1548-1573`; `estimate_matches_execute_for_a_typed_operation_script` `qm:4029` | subset | DELETE |
| `not_found_is_charged_after_the_key_lookup_for_patch_and_delete` | 1422-1455 (34) | patch/delete of an unbound key: 4 | `patch_of_a_missing_key_..` `qm:1935-1957`, `delete_of_a_missing_key_..` `qm:2176-2213` | subset | DELETE |
| `unauthorized_and_unknown_namespace_are_charged_the_directory_lookup` | 1847-1894 (48) | create/patch/delete by BOB: Unauthorized 1; on an unknown namespace: UnknownNamespace 1; ledgers; recorded | `typed_writes_reject_wrong_owner_and_unknown_namespace_after_the_lookup` `qm:2216-2267` | subset (one extra line, 1893: `typed_record_count(unknown) == None`) | DELETE |
| `record_full_is_charged_after_the_lookup_but_too_many_cells_is_an_admission_fault` | 1458-1569 (112) | max_cells 4: RecordFull at 4 twice, room after a removal (150), replace keeps count (92), 5-cell create TooManyCells | `patch_that_would_overfill_the_record_is_record_full_after_the_lookup` `qm:1959-2058` (same max_cells 4 story: full, full with an absent removal, applies after removing `note` at 158, 5 names TooManyCells) | same rules, other numbers | MERGE the 150/92 vectors; the projection tail 1553-1568 is a third copy of 2.12 |
| `host_record_writes_match_the_owner_path_and_skip_only_the_owner_check` | 2064-2133 (70) | host == owner for delete a 106, create d 229, patch b 182, NotFound 4; estimate_host == execute_host; BOB refused at 1 | `execute_host_receipts_are_identical_to_execute_for_the_same_op` `qm:3866-3959` (create d 199, patch 183, delete c 106, eight budgets each, estimate_host, BOB refused) | same rule; the NotFound-on-host vector is extra | MERGE |
| `namespace_and_raw_ops_on_the_host_path_are_not_host_operations` | 2136-2192 (57) | CreateNamespace/Transfer/Put/Delete on the host path: NotHostOperation before the block budget, spent 0, nothing recorded, `namespace_count() == 1`, typed op gets ExceedsBlockBudget | same `qm` test, `qm:3961-4022` (same four ops, three budgets, same follow-up asserts) | near-subset; extras: put on unknown namespace (2151), bad cells on the host path (2166-2172), descriptor unchanged (2183-2190) | MERGE the three extras, DELETE the rest |
| `get_record_projections_charge_only_the_projected_bytes` | 2432-2537 (106) | ALL 54, KEY_ONLY 8, ID_ONLY 7, Named[price, missing] 20, Named key[kind, note] 23, Named[] 7, unbound 4 under every budget | `get_record_vectors_for_every_projection` `qm:2274-2363` (the same seven vectors plus `all c`, snapshot and branch, traced too) | everything up to 2511 is covered: the vectors and their traced ledgers at every budget by `qm:2274-2363` (`ledger_after`), the unknown namespace by `typed_reads_on_an_unknown_namespace_charge_the_lookup_only` `qm:3650-3694` | TIGHTEN to the unique part: keep 2513-2536 (ProjectionTooLarge at 65 names, 64 admitted) as its own small test; MERGE the one extra vector `Named key [absent] = 8` into the `qm` table; delete 2437-2511 (~75 lines) |

After this, `WriteVector`/`assert_write_sweep` (526-595) has no caller and goes too, and `create_abc` (303-316) can stop asserting 379/229/257 (pinned at `qm:683`).

Totals for 2.1: 7 tests DELETE outright (877, 936, 1089, 1316, 1391, 1422, 1847; 366 lines), 7 MERGE (1005, 1153, 1203, 1252, 1458, 2064, 2136; 478 lines, of which the distinct literals are about a dozen vectors), 1 TIGHTEN (-75), plus the 70-line sweep harness. Alternative if the owner prefers the reverse direction: keep `records.rs` as the readable "worked examples" file and cut the matching vectors from `qm`; either way one copy should go. Not `[CONSENSUS]` in itself (no production code changes), but the surviving copy is the only pin, so do it test by test.

#### 2.2 `queries.rs:2093-2709` (7 cost tests, 617 lines): a second fixture for rules `query_metering.rs` pins under every budget - needs-care

`queries.rs`'s own header calls these "a handful of hand-derived query charges" (line 2-3, 34-37). Each rule has a counterpart in `qm` section 5 (I read `qm:2426-2470` in full and the derivation comments of the others):

| `queries.rs` test | lines | rule | `qm` counterpart |
|---|---|---|---|
| `eq_pages_charge_the_head_beyond_the_page` | 2213-2340 | page of `limit` pulls `limit + 1` posting entries; last page only the remainder; missing term = seek only; Exists; count; OutOfBudget returns no partial page | `eq_query_pages_charge_the_head_beyond_a_full_page` `qm:2426`, `exists_and_not_eq_queries_seek_the_existence_posting` `qm:2603`, `count_vectors_walk_every_head_without_row_charges` `qm:3518` |
| `range_runs_charge_only_the_qualifying_term_keys` | 2343-2397 | excluded bound stepped over free; terminating key free; `lower > upper` and empty runs = seek only; existence term never visited | `range_queries_visit_only_the_qualifying_terms` `qm:2703` (the same cases; see its derivation comments at `qm:2705`, `2743`, `2766`, `2788`, `2817`, `2839`) |
| `prefix_runs_visit_every_matching_str_term` | 2400-2433 | empty prefix visits every Str term; prefix == value; prefix past the run = seek only | `starts_with_queries_visit_the_prefix_run` `qm:2918` |
| `not_eq_and_negation_probe_every_candidate` | 2436-2523 | NotEq probes the existence posting; a negated first literal walks the row map; head beyond a page of one | `qm:2603` (NotEq), `negated_first_literal_and_match_all_walk_the_row_map` `qm:3035` |
| `match_all_pages_walk_the_row_map` | 2526-2562 | RowVisit per row + head beyond the page; continuation; count | `qm:3035` (its match-all part, comments at `qm:3097`, `3125`, `3143`, `3169`), `qm:3518` |
| `and_groups_probe_candidate_major_until_a_literal_fails` | 2565-2633 | candidate-major probing stops at the first failing literal | `and_groups_probe_each_candidate_until_a_literal_fails` `qm:3213` |
| `or_groups_merge_by_id_and_advance_only_emitted_heads` | 2636-2709 | merge by id, an id in two groups emitted once, a head beyond the page is not advanced | `or_groups_open_in_order_and_merge_interleaved_heads` `qm:3325` |

Verdict: **kept: pins receipts**, but MERGE: move the k1..k6 vectors that add something (`ink`/U64 record 5 not being a candidate, lines 2384-2388 and 2469-2480; record 6 lacking `kind`, 2481-2508) into the matching `qm` tests as extra `QueryVector`s and delete this section together with `cost_fixture`, `kind_eq`, `price_range`, `inc`, `exc`, `page` (2097-2210). `queries.rs` then is what its title says: semantics against the reference + validation order. -617 lines here, roughly +80 in `qm`.

If the section stays, TABLE-DRIVE it (see 4.3): 22 calls of the 7-argument `page(..)` at 9 lines each are ~200 lines of call syntax.

#### 2.3 `terms.rs` cost tests: four tests, one table - TABLE-DRIVE, safe (~200 lines)

`an_i64_run_charges_the_lookup_the_seek_and_each_returned_term` (1355-1430), `an_excluded_existing_bound_is_free_and_an_included_missing_bound_starts_at_the_next` (1433-1503), `a_str_run_charges_read_bytes_per_value_length` (1506-1573) and `a_missing_name_or_tag_costs_the_seek_only` (1576-1630) are 25 repetitions (`awk 'NR>=1355 && NR<=1630 && /let got = read\(/' terms.rs | wc -l`, three of them inside small loops) of

```rust
let got = read(view, ns, &name, tag, lower, limit);
assert_eq!(got.value, <values>, "{label}");
assert_eq!(got.receipt, read_applied(<cost>), "{label}");
```

each 9-12 lines after rustfmt. One table keeps every vector and the derivation comments:

```rust
// (name, tag, lower, limit, values, cost)
let cases: &[(&str, TypeTag, Bound<Value>, usize, Vec<Value>, u64)] = &[
    ("price", I64, Unbounded,          200, i64s(&[5, 10, 20]), 38), // 1 + 4 + 3 * (3 + 8)
    ("price", I64, Unbounded,            3, i64s(&[5, 10, 20]), 38), // no peek charge past the third
    ("price", I64, Excluded(i64v(5)),    1, i64s(&[10]),        16), // 5 stepped over free
    ("kind",  Str, Excluded(strv("")), 200, strs(&["a", "ab"]), 14),
    ...
];
```

Keep the three test names as three tables if the grouping matters. Five of these rows are asserted again by `every_budget_below_the_total_rejects_..` (1763-1807: its sweep includes the unlimited budget and checks value + total): `price Unbounded 200 = 38`, `Unbounded 1 = 16`, `Excluded(5) 200 = 27`, `kind Unbounded 200 = 17`, `price/U64 = 5`.

`a_traced_terms_read_lists_classes_in_first_charge_order` (1633-1760): four identical 24-line blocks plus one rejection; same treatment, `[(name, tag, limit, values, total, visits, read_bytes)]`. -70 lines. **kept: pins ledgers.**

#### 2.4 `queries.rs:1939` `a_cursor_is_bound_to_its_context_query_and_projection` - DELETE (153 lines), subset of `pagination.rs` - needs-care

`queries.rs`'s header says it does not establish "the full cursor-binding matrix"; `pagination.rs` does, and every assertion of this test has a counterpart there:

| `queries.rs` lines | assertion | `pagination.rs` |
|---|---|---|
| 1957-1969 | same request continues; the limit is not bound | 1785-1800; `a_walk_may_change_its_limit_between_pages` 1082-1129 |
| 1971-1978 | other projection | 1748-1765 |
| 1979-1986 | other query | 1684-1713 |
| 1987-1994 | other namespace (unknown ordinal 9) | 1671-1682 (a namespace that exists), 1885-1898 (unknown, foreign cursor) |
| 1996-2006 | snapshot cursor on a branch | 1533-1544 |
| 2008-2022 | branch cursor on its base snapshot | 1545-1561 |
| 2023-2047 | branch cursor survives a rejected write | `branch_cursor_continues_on_the_same_untouched_branch` 1947-2073 (Unauthorized, NotFound twice, NotHostOperation, estimates, checkpoint) |
| 2048-2065 | invalidated by an applied write | `branch_cursor_is_invalidated_by_every_applied_mutation` 2076-2105 |
| 2066-2090 | later snapshot refuses, original still accepts | `cursor_from_another_snapshot_or_a_branch_is_a_mismatch` 1470-1528 |

Only nuance not literally there: "same-snapshot cursor + unknown namespace" (1987-1994). Add it as a fourth row to the loop at `pagination.rs:1671-1682` if wanted. Also removes the `mismatch` closure (1951-1955), a fifth spelling of "CursorMismatch at spent 0".

#### 2.5 `terms.rs`: literal spot checks implied by "engine == model at every probe" + "model == literals" - TIGHTEN, safe (~95 lines)

- `terms_equal_the_models_distinct_attribute_values_from_every_bound_and_limit`: 880-902 pin the model's `distinct` sets as literals; 906-907 prove the engine equals the model for every `probes(tag)` x `LIMITS`. Lines 908-958 then re-assert four of those very probes by hand (`Unbounded` limit 2 -> `[MIN, -5]`; `Excluded(7)` -> `[MAX]`; `Excluded(1000)` -> `[u64::MAX]`; `Included("")` limit 1 -> `[""]`; all four bounds are in `probes`, 493-497) plus two `!contains` checks (42 and "zzz" are excluded by the literals at 882 and 890). 51 lines, zero added coverage.
- `terms_follow_deletes_and_patches_and_an_emptied_value_disappears`: same pattern, 994-1036 (43 lines) after 969-986.

Verdict: delete both blocks, or keep one 6-line "extremes order numerically" check.

#### 2.6 `pagination.rs`: the same two charge rules a third time - safe to shrink

- `eq_walk_charges_limit_plus_one_entries_then_exactly_the_remainder` (1136-1291, 156 lines) pins "limit + 1 entries, then the remainder" on posting `[1, 2, 4]`: 19/14, 22, 15/15/14, 10, 63. Same rule as `queries.rs:2257-2281` (15/14) and `qm:2426`.
- `a_group_whose_head_is_beyond_the_page_end_does_no_further_work` (1294-1380, 87 lines) pins 31/32/25. Same rule as `queries.rs:2680-2708` (25/31) and `qm:3325` (comments at `qm:3381`, `3401`).

Verdict: **kept: pins receipts and is the only place a three-page OR walk is derived**; keep `a_group_whose_head..` as is. In `eq_walk..` MERGE the five scenarios into one table `[(limit, cursor_after, rows, next_after, total, posting_entries, result_rows)]` driven by the existing `read` closure: the five ledger blocks differ only in two counts. ~70 lines.

#### 2.7 `queries.rs:1774-1828` (inside `query_faults_are_reported_..`): cursor-order block duplicates `pagination.rs:1804-1899` and contains dead code - DELETE 55 lines, safe

Steps "7. ProjectionTooLarge comes after every literal and before the cursor" and "8. CursorMismatch is last" build a second engine just to obtain a foreign cursor and assert two things that `cursor_check_comes_after_query_and_projection_validation` (`pagination.rs:1804-1899`) asserts with NoGroups, ZeroLimit, LimitTooLarge, ProjectionTooLarge and then the mismatch. The block also starts with a leftover (see 4.5). Keep the standalone ProjectionTooLarge check without a cursor (one `expect_fault`, 4 lines).

#### 2.8 Tests of the test's own constants - DELETE the assertions, safe (~45 lines)

These never touch the engine; they check one test-local constant against another:

- `terms.rs:2271` `assert_eq!(EXPIRY_TERMS_READ, 16);` where `const EXPIRY_TERMS_READ: u64 = 16;` (799) has no other use (`rg -n '\bEXPIRY_TERMS_READ\b' terms.rs` -> 799, 2271). Delete the const and the assert; the comment at 2269 carries the derivation.
- `terms.rs:2063-2066` `EXPIRY_DELETES.iter().sum() == EXPIRY_TOTAL`. Define `EXPIRY_TOTAL` once as the literal with the sum in its doc (already there, 795) and drop the assert.
- `terms.rs:2035-2049` recomputes `[78, 54, 82, 102, 58, 54]` from the weight consts; `terms.rs:2162-2173` and `2216-2227` do the same for `DELETE_R2` / `DELETE_R5`. `DELETE_R5` (805) is used only by its own self-check (`rg` -> 805, 2219); the engine is compared with the literal `48` at 2232.
- `pagination.rs:112-197`: twelve `const _: () = assert!(TOTAL == DIRECTORY_LOOKUP + ..)` blocks; the weight consts at 100-106 are used nowhere else (ledger lines use bare `1, 5, 4`), and each total's doc comment already states the same sum in prose.

Four conventions for one need: `records.rs` derives in comments only, `terms.rs` re-derives at run time, `pagination.rs` at compile time, `qm` builds totals from group tables. Pick one. Cheapest: keep the literal and the one-line derivation comment (records.rs style); or, if the arithmetic should be machine-checked, keep `pagination.rs`'s `const _` form and delete the prose sums. Either removes ~45 lines here.

#### 2.9 `terms.rs:2143` `a_starved_delete_budget_halts_before_any_delete` - TIGHTEN, safe (~35 lines)

Budget 0 (2145-2157) and budget 53 (2174-2184) are two iterations of the loop `for budget in 0..54` at 2187-2207, which already checks `applied`, the receipt, `spent`, record count and consistency. Move the three asserts the loop lacks (`halt.rejected.reason`, `branch.receipts() == [rejected(spent, ..)]`, `assert_branch_matches_model`) into the loop and delete the two hand-written blocks. The literals 0 and 44 stay pinned through `DELETE_R2 = [1, 4, 34, 44, 54]`.

#### 2.10 `pagination.rs:1387` `replaying_a_page_on_the_same_snapshot_is_identical` - MERGE, safe (~35 lines)

`pinned_walks_match_the_reference_..` (1009) already replays every page of 8 shapes x 4 limits on the same snapshot and requires `disturbed == quiet` including cursors and receipts (1042-1045). What 1387 adds: traced receipts, and "another handle to the same snapshot accepts the cursor" (1424-1431). Lines 1411-1414 assert `receipt.kind == Read`, `model_version`, `schedule_id`, `ledger.is_some()` on a receipt the next lines compare whole. Move 1424-1431 into `cursor_from_another_snapshot_or_a_branch_is_a_mismatch` (next to "Each cursor continues at home", 1508-1529) and delete the rest.

#### 2.11 `records.rs` section 1 vs unit tests in `crates/arkiv-db/src/record.rs:989-1084` - MERGE, safe

The integration tests are supersets of the unit tests (same vectors, same order):

| `tests/records.rs` | `src/record.rs` | note |
|---|---|---|
| `record_keys_take_1_to_32_bytes_and_encode_as_length_and_bytes` 643-666 | `keys_hold_one_to_thirty_two_bytes` 993-999 | unit test is a strict subset. **kept: pins the key encoding** (integration copy) |
| `record_ids_are_positive_ordinals` 669-676 | `ids_start_at_one` 1076-1083 | same three facts (`from_ordinal(0)`, `ordinal()`, `"rec:7"`); line 675 `assert_eq!(rid(7), seven)` compares a value with itself. DELETE the integration copy (8 lines); move `rid(1) < rid(2) < rid(10)` to the unit test |
| `cell_names_follow_the_grammar_and_order_bytewise` 679-716 | `names_follow_the_grammar` 1001-1018 | unit test's good/bad lists are subsets of 681-704. **kept: pins the name grammar** (integration copy) |
| `values_have_frozen_term_bytes_and_encoded_lengths` 750-819 | `term_bytes_preserve_order` 1020-1074 | the nine term-byte vectors (752-763 vs 1023-1034), `cmp_ordered` and `encoded_len` asserts are the same; unit test alone has the `from_term_bytes` round trip. **kept: pins term bytes** |

Fix: keep one copy of each. Simplest: keep the integration tests, move the `from_term_bytes` round trip (`src/record.rs:1035-1054`) and the `Debug` format assert (`:1082`) into them, delete the four unit tests (~90 lines in the other chunk). In `values_have_frozen_..` DELETE lines 785-789 (`Value::Bool(true).tag() == TypeTag::Bool` x5: getter of a constant; `golem_types.rs:152-154` checks `tag()` for every variant).

#### 2.12 `ProjectionTooLarge` for `get_record` is tested three times - safe

- `records.rs:2513-2536` (default limits: 65 names refused for snapshot, unknown namespace, branch, at budgets unlimited and 0; 64 names read for 7),
- `records.rs:1553-1568` (max_cells 4: 5 refused, 4 read for 7) - bolted onto the unrelated `record_full..` test,
- `queries.rs:1914-1935` inside `count_and_get_record_validate_like_query` (max_cells 3: 4 refused incl. unknown namespace at budget 0, 3 read).

Verdict: keep `records.rs:2513-2536` and `queries.rs:1914-1935` (custom limit); DELETE `records.rs:1553-1568` (16 lines, strict subset of the other two).

#### 2.13 `pagination.rs:1785-1800` - DELETE (16 lines), safe

"The limit is not bound" at the end of `cursor_bound_to_another_namespace_query_or_projection_is_a_mismatch`: already shown by `a_walk_may_change_its_limit_between_pages` (cycle 1, 3, 7, 2, 5 at 1097-1110 and `MAX_PAGE` after 3 at 1112-1128).

#### 2.14 `records.rs:822` `cells_carry_a_kind_and_a_value` - DELETE (14 lines), safe

Asserts that `Cell::attribute(v)` has `kind == Attribute` and `value == v`, that `is_attribute()` reads `kind`, and that the derived `PartialEq` sees `kind`. Constructor/getter/derive only; every write test in the file would fail first if any of it broke.

#### 2.15 `queries.rs:1252-1264` asserts nothing about what its comment says - fix or DELETE (13 lines), safe

```rust
// A named projection skips names the record lacks without charging them.
let page = snapshot.query(ns, &eq("n", s("only")), &Projection::named(true, [name("n"), name("q")]), 5, None, ..)
assert!(page.rows.is_empty());
```

No fixture record has `n = "only"` (r35 holds `k = "only"`, line 462), so the page is empty for any projection, and no receipt is looked at. Either change the query to `eq("k", s("only"))` and assert the row `{ id 35, key, cells: [] }`, or delete the block: the loop above (1247-1251) already runs `Exists(k, Str)` (matches r35) under `named(false, [n, q])` against the model.

#### 2.16 `records.rs:719` `type_tags_have_frozen_ids_and_capabilities` - kept: pins type ids; TIGHTEN, safe

Lines 733-741 (`!is_indexable()` only for Bytes; `is_ordered() == is_indexable()`) repeat `golem_types.rs:141` and `golem_types.rs:181-183` for all 25 tags. Drop them or keep; in either case rename the binding at 733: `let indexable = ..filter(|t| !t.is_indexable())` holds the tags that are *not* indexable.

#### 2.17 Tests I checked and would keep as they are

`records.rs`: `limits_are_fixed_per_engine` (838; kept: pins `Limits::DEFAULT`), `argument_faults_are_admission_rejections_with_nothing_recorded` (1579; the only fault-precedence table), `argument_validation_precedes_every_charge_and_the_block_budget_check` (1791), `ids_are_dense_and_never_reused_across_delete_and_recreate` (1901), `rollback_releases_ids_and_restores_rows_keys_and_terms_together` (1990), `a_transfer_shares_the_typed_state_and_visits_no_record_or_index_node` (2195), `raw_bytes_and_typed_records_share_a_namespace_but_never_a_key` (2265), `a_pinned_snapshot_answers_get_record_after_later_patches_and_deletes` (2540), `rollback_makes_terms_created_inside_the_transaction_new_again` (2671; reads like a regression test). `queries.rs`: tests 1-4, 6, 7, 9 and the fault-order part of 8. `pagination.rs`: 1009, 1082, 1294, 1439, 1578, 1648, 1804, 1947, 2076, 2169, 2324. `terms.rs`: the semantics, validation, isolation and expiry tests.

### II.2 [T5] arkiv-db tests: durable / faults / KV; examples

*(paths relative to `crates/arkiv-db/`)*

#### 2.1 tests/fallible_api.rs - the file compares a function with itself (465 lines)

Every infallible method is literally `in_memory(self.try_x(..))`: src/engine.rs:166-171 defines `in_memory` (unwrap or panic) and `rg -c "in_memory\(" crates/arkiv-db/src/engine.rs` -> 30 call sites (e.g. 913, 925, 1046, 1075, 1102, 1137, 1179, 1245, 1272, 1714, 1740, 1877, 1894, 2042, 2063, 2082, 2102); `expire_records` is the same at src/host.rs:124-139.

| Test | Line | Verdict | Reason |
|---|---|---|---|
| `writes_estimates_reads_and_statistics_match_the_infallible_forms` | tests/fallible_api.rs:385 | DELETE | Asserts `x() == try_x().unwrap()` where `x` is defined as `try_x().unwrap()`. Pins no number (both sides come from the code under test). The only thing it can catch is an infallible wrapper wired to the wrong `try_` form, and each infallible form is asserted against expected values elsewhere: between 9 (`estimate_host`) and 323 (`execute`) call sites per method in the other files of tests/ (`rg -c "\.<method>\(" tests/*.rs -g '!fallible_api.rs'`) |
| `the_expiry_sweep_matches_its_fallible_form` | tests/fallible_api.rs:422 | DELETE | Same, for `expire_records` (src/host.rs:132) |
| `storage_errors_are_the_storage_contract_type` | tests/fallible_api.rs:457 | DELETE, move one line | The type identity is the re-export at src/lib.rs:90-93 and is checked by the compiler. The `Display` assertion (`"Corrupt: hash mismatch"`, line 464) is the only place that format is pinned (`rg -n '"Corrupt: ' crates/` -> this line only); move that single assertion next to `impl Display for StorageError` (crates/arkiv-db-storage/src/versioned.rs:598) |

Risk: **needs-care** - what is lost is a tripwire for a future refactor in which the infallible forms stop delegating. If that matters, keep a 30-line smoke (one write, one read, one statistic) rather than 465 lines. Also drop the row at docs/testing.md:300. Real failure behaviour of the `try_` forms is covered by storage_faults.rs, which this file's header already says (lines 14-17).

#### 2.2 KV facade tests (kv.rs, semantics.rs, backend.rs, model.rs: 673 lines)

Context the owner should decide on first: `Database` is five forwarding methods (src/lib.rs:124-178); `KvStore` has one production impl, `MemoryStore` (`rg -n "impl.*KvStore for" crates/` -> crates/arkiv-db-memory/src/lib.rs:34 and the test backend); nothing outside these four tests and examples/basic.rs constructs either (`rg -n "Database::(in_memory|from_store)|\bKvStore\b|\bWriteBatch\b" crates/ conformance scripts -g '!crates/arkiv-db/tests/**' -g '!crates/arkiv-db/examples/**'` -> definitions and docs only). docs/architecture.md:213-214 calls it "the contract a later durable backend implements", but the durable backend that was built implements `VersionedStore` (crates/arkiv-db-redb/src/store.rs:1063). If the facade is retired, all 673 lines and examples/basic.rs go with it. If it stays, tests/model.rs alone covers most of the rest:

tests/model.rs:182 runs 4 seeds x 256 blocks over 18 boundary keys (`""`, `\0`, `\xff`, `a/`, `\xff\xff\0`, ...), block sizes `[0, 1, 2, 8, 32, 128]`, value sizes `[0, 1, 2, 7, 32, 257]`, put / delete / re-put of one key inside a block, deletes of absent keys, and checks `get` for every key and `scan_prefix` for every prefix of every key against an independent log oracle after every block.

| Test | Line | Verdict | Reason |
|---|---|---|---|
| `consecutive_block_packets_preserve_operation_order` | tests/kv.rs:6 | DELETE | Everything it checks is generated and checked by model.rs: last write wins inside a block, delete of a missing key, empty key and empty value, an empty block, re-delete. Lines 16/22 (`original` unchanged) and 34-36 are true by ownership, see 2.3 |
| `prefix_scans_are_byte_ordered_and_return_owned_results` | tests/kv.rs:59 | MERGE with `prefix_scans_respect_binary_neighbors_without_exact_keys` (tests/semantics.rs:27) -> one table-driven test | Same test twice: insert entries in reverse, scan a list of prefixes, compare slices. One table `[(prefix, expected range)]` over the union of both key sets keeps the readable worked example, including the all-`0xff` prefix (no successor key), which is the one edge worth a named case |
| `block_replacements_preserve_value_sizes_and_neighboring_keys` | tests/kv.rs:117 | kept: only test with values over 257 bytes (4097, 8193), relevant once a paged backend exists | |
| `returned_buffers_do_not_alias_stored_data` | tests/semantics.rs:6 | DELETE | `get` returns `Option<Vec<u8>>` and `scan_prefix` returns `Vec<Entry>` (src/lib.rs:142, 175): owned values cannot alias the store in safe Rust; the test checks the type system |
| `block_packets_own_inputs_and_discarded_candidates_do_not_publish` | tests/semantics.rs:62 | DELETE | First half: `WriteBatch::put(impl Into<Vec<u8>>)` (crates/arkiv-db-storage/src/batch.rs:36) copies a slice, and `key.fill(..)` on line 73-74 compiles only because the batch holds no borrow. Second half: a dropped `WriteBatch` has no reference to the database |
| `wrapping_preserves_backend_contents_and_databases_are_independent` | tests/semantics.rs:96 | kept (low value): first half pins that `from_store` keeps contents; the "two databases are independent" half (lines 113-130) is true of any two values without shared state and can go | |
| `backend_errors_are_preserved_and_rejected_blocks_can_be_retried` | tests/backend.rs:77 | kept, trim | Lines 93, 95, 104 assert the state is unchanged after a rejected write, but `TestBackend::write_batch` returns before touching `inner` (backend.rs:56-60, its own comment says so): they test the test double. Keep the error pass-through and the "put/delete go through `write_batch`" part |
| `each_block_reaches_the_backend_once_including_empty_blocks` | tests/backend.rs:115 | kept: pins one batch per block, unsplit and in order | |
| `block_packets_match_a_log_model_before_and_after_each_block` | tests/model.rs:182 | kept, trim | See 2.3 |

Risk: **needs-care** (test semantics); update docs/testing.md:112-115. With the two deletions in semantics.rs and the merge, kv.rs + semantics.rs fit in one file and the duplicated `entry` helper disappears.

#### 2.3 Assertions that cannot fail

- tests/model.rs:193-202 - the `"staged"` and `"discarded"` phases call `assert_model` after building (and dropping) a `WriteBatch`. A `WriteBatch` is a detached `Vec<WriteOperation>` (crates/arkiv-db-storage/src/batch.rs:23-25); nothing touched `db` since the previous `"applied"` check. Each `assert_model` is 19 `get`s plus one `scan_prefix` per prefix case, so this roughly doubles the test's work for nothing. Fix: delete lines 193-202, keep `db.write_batch(block.batch())`. **safe**. The same vacuous "building a batch exposes nothing" checks: tests/kv.rs:34-36, 98, 134; tests/semantics.rs:76.
- tests/adopt.rs:321 `assert!(!error.to_string().is_empty());` - asserts that an error has a message. The meaningful checks are the `unwrap_err()` above it and the profile check below it. Fix: delete the line, or assert the variant (the three cases are refusals for a different genesis, identity and packet). **safe**.
- tests/durable_failures.rs:314-316 - "Keep the memory facade's ordinary publication path covered unchanged." followed by a memory-engine `seal` + `publish` at the end of a durable-engine test. Unrelated to the test's name and covered by every memory-engine test. Fix: delete the three lines. **safe**.

#### 2.4 Optional merge

- `oversized_raw_value_is_rejected_without_storage_work_or_state_publication` (tests/durable_failures.rs:696-711) is the rejecting half of `encoded_object_ceiling_includes_trie_framing_and_persists_on_reopen` (860-890, `deficit == 1` arm at 882-888) with a looser ceiling (1024 vs `largest - 1`). MERGE: move its four extra assertions (candidate returned with its receipts, `object_reads` unchanged, head 0, `last_preparation.preflight_visits == 0`) into that arm and delete the first test. 17 lines. **needs-care** (I did not run it; confirm `preflight_visits == 0` also holds at `largest - 1`).

#### 2.5 Kept on purpose (not candidates)

All 29 tests of durable_failures.rs other than the above, all 7 of storage_faults.rs, 11 of simulator_composition.rs, 3 of adopt.rs, 4 of streaming_import.rs, 8 of durable_retention.rs, 4 of durable.rs, 3 of rehash.rs, 2 of interrupt.rs: they pin crash / recovery / fault behaviour, on-disk vectors (`hand_built_profile_and_manifest_vectors`, `hand_built_profile_identity_block_and_inclusion_vectors`, `prefilled_genesis_has_distinct_encoding_..`), `RootUpdate` bounds or receipts. `preparation_caps_fail_before_commit_and_return_the_candidate` (durable_failures.rs:661) looks like a subset of `exact_preparation_boundaries_accept_and_one_less_rejects` (819) but is kept: it pins that a cap of 0 is a hard cap (`prepare_objects == 0` is special-cased in src/durable/bulk.rs:341 and streaming.rs:207). `ten_thousand_publications_..` (durable.rs:207) and `long_history_keeps_one_manifest_..` (simulator_composition.rs:405) look alike but go through `prepare` vs `prepare_simulator`.

### II.2 [T1] arkiv-db tests: metering family

*(paths relative to `crates/arkiv-db/tests/`; `qm` = `query_metering.rs`)*

Numbers in metering receipts were treated as pins; nothing below removes the last pin of a charge point, and each entry says where the surviving pin is.

#### DELETE (2 tests)

- `exact_budget_succeeds_while_one_short_and_zero_fail` - metering.rs:991-1044 - **DELETE** (and the then-empty banner 987-989). Every receipt in it is one row of the six every-budget sweeps at 346-437, which run each op under all budgets `0..=total`: put 25 / 24 -> spent 1 / 0 -> spent 0 (346-358), delete 16 (375-387), transfer 23 (405-419), create 53 and 52 -> spent 1 (422-437). The running `branch.spent()` over mixed outcomes is pinned at 880/936 and 1448. -57 lines. **needs-care** `[CONSENSUS]`.
- `transfer_one_unit_short_spends_only_the_lookup_and_changes_nothing` - metering.rs:743-759 - **DELETE**. Budget 22 is a row of `transfer_charges_lookup_then_the_transfer_group` (405-419), whose helper asserts reason, receipt `(1, OutOfBudget)` and `observe(..) == baseline` (which includes the descriptor) at 255-275. -17 lines. **needs-care** `[CONSENSUS]`.

#### MERGE (5 tests fold into existing ones)

- `estimate_runs_on_scratch_state_that_never_leaks` - metering.rs:1568-1622 - **MERGE into** `estimate_has_no_side_effects` (1456-1532). Both estimate put/delete/transfer/create and assert `observe(&branch, ns) == before` and `namespace_count() == 1`; `observe` already scans every record, so the two `get` checks at 1578-1590 are implied. Unique content: `descriptor(ns_id(2))` is `UnknownNamespace` after an estimated create (1605-1611) and the head snapshot is untouched (1613-1621). Move those two assertions to after 1506. -48 lines. **safe**.
- `untraced_receipts_have_no_ledger_and_otherwise_equal_traced_ones` - metering.rs:616-665 - **MERGE into** `tracing_never_changes_charges` (1785-1834). The latter is already a table of 11 writes (including the same `put(ns, K3, V4)`) plus one read, asserting "traced minus ledger == untraced". Add the scan (636-645) and the unknown-namespace descriptor with its literal ledger (647-664) as read rows. -30 lines. **safe** `[CONSENSUS]` (the `[(DirectoryLookup, 1, 1, 1)]` rejected-read ledger must move with it).
- `signed_and_unsigned_order_vectors` - scalar_profile.rs:95-118 - **MERGE into** golem_types.rs `ordered_samples()` (golem_types.rs:43-102) / `ordered_types_sort_by_term_bytes_and_cmp_ordered` (158). That test already asserts term-byte order and `cmp_ordered` for I32 `[MIN, -1, 0, 1, MAX]` and Dec `[min, -1, 0, 1, max]` (identical to 101-114 here). Not covered there: I32 +-100 and the U256 pair `0x7f ff.. < 0x80 00..` (115-117, i.e. U256 is not sign-flipped). Add those samples to the I32 row (golem_types.rs:49) and the U256 row (58-60), then delete. -23 lines. **needs-care** `[CONSENSUS]` (term-byte order is the index key order).
- `genesis_and_query_have_literal_profile_vectors` - scalar_profile.rs:311-337 - **MERGE**. Genesis half (314-321: the 32-byte prefix `ARKENG01 ‖ [2,1,2,3,2,2]` and a round trip) is a strict subset of durable.rs:37-58 `hand_built_profile_and_manifest_vectors`, which pins the whole genesis encoding with the same prefix loop. EqQuery half (322-336) is eq_proofs.rs:576-587 with `I32(-2)` instead of `I64(-2)`; add it there as a second vector (eq_proofs.rs:612-618 already rejects a corrupted version byte, `at = 3` set to 99; the layout-1 value `1` checked at 335-336 here is a one-line addition there). -27 lines here. **needs-care** `[CONSENSUS]`.
- `scalar_and_string_admission_obeys_the_caps` - scalar_profile.rs:248-309 - **MERGE into** cell_limits.rs `str_bytes_and_indexed_strings_have_separate_limits` (209-244) as a second table under `Limits::DEFAULT`. It asserts only `result.is_ok() == accepted` with no message (306), while cell_limits.rs checks the exact `ArgumentFault`. The é×64 accept and é×64+"x" reject rows are also in bounds.rs:203-256 with exact reasons. Rows to keep: ASCII 127/128/129 and the fixed-width types accepted. The `Value::bytes` attribute row (294) is pinned with its exact reason `ArgumentFault::BytesNotIndexable` at codec.rs:726-736, so it can go. -40 lines. **needs-care**.

#### TABLE-DRIVE

- metering.rs:345-437 - `put_replacing_a_record_charges_lookup_then_the_record_group`, `put_inserting_a_record_charges_the_same_and_reports_no_replacement`, `delete_of_a_present_key_charges_lookup_then_the_delete_group`, `delete_of_a_missing_key_costs_the_same_and_reports_existed_false`, `transfer_charges_lookup_then_the_transfer_group`, `create_namespace_charges_lookup_then_the_create_group` (and the first half of `put_with_empty_key_and_value_costs_fifteen_and_omits_zero_count_classes`, 517-527) - **TABLE-DRIVE** into one `raw_write_vectors_under_every_budget`. Six bodies that are one `assert_write_vector` call differing in `(name, op, prefix_sums, effect)`; the helper already threads `name` into every message. 6 tests -> 1, -45 lines, all 7 vectors kept with their derivation comments. **needs-care** `[CONSENSUS]`.
- qm - ten "the seek only" query vectors with identical `groups: vec![lookup(), query_group(), term_seek()], total: 1 + 5 + 4, ids: &[], next: None`, 17-22 lines each (186 lines): 2540-2556 `Eq(kind, ink)`, 2557-2573 `Eq(price, U64 10)`, 2634-2650 `Exists(kind, I64)`, 2682-2699 `NotEq(price, U64 5)`, 2817-2838 `Range(price, 20..=10)`, 2839-2860 `Range($expiresAt, >=6)`, 2861-2881 `Range(price, U64)`, 2980-2997 `StartsWith(kind, c)`, 2998-3014 `StartsWith(kind, z)`, 3015-3031 `StartsWith(price, 1)`. **TABLE-DRIVE** as `for (name, query) in [..]` (either one test `terms_nobody_holds_cost_the_seek_only`, or one small loop per thematic test). The projection differs between them only by accident (no row is emitted). ~-115 lines, all 10 vectors kept. This also shortens three of the over-long fns in 4.1. **needs-care** `[CONSENSUS]`.
- qm - `create_of_a_bound_key_is_already_exists_after_the_binding_lookup` (1547-1573), `patch_of_a_missing_key_is_not_found_after_the_binding_lookup` (1934-1956), `delete_of_a_missing_key_is_not_found_after_the_binding_lookup` (2175-2213, two vectors) - **TABLE-DRIVE** into one `typed_writes_rejected_after_the_binding_lookup_spend_four` over `(name, setup, op, reason)`. All four are `groups: vec![lookup(), point_read()], total: 1 + 3, expect: Expect::Rejected(..)` followed by the same `observe == modelled` / `branch.receipts() == [write_receipt(4, ..)]` checks. The two `RecordFull` vectors at 1971-2003 have the same shape but need `fixture_with(limits)`; loop them over `[&[][..], &["missing"]]`. 3 tests -> 1, ~-55 lines. **needs-care** `[CONSENSUS]`.
- qm:2216-2267 `typed_writes_reject_wrong_owner_and_unknown_namespace_after_the_lookup` - already a table, but the op list is built twice (2218-2225 for `ns`, 2244-2251 for `unknown`) and the loop written twice. One closure `ops(target)` and one loop over `[(ns, BOB, Unauthorized), (unknown, ALICE, UnknownNamespace)]`. -20 lines. **safe**.

#### TIGHTEN (redundant halves and tautological assertions)

- `scan_aborting_between_entries_keeps_earlier_charges_and_returns_nothing` - metering.rs:761-800 - **TIGHTEN** to the traced budget-14 case (768-783). Budgets 19, 20 and 13 (784-799) are rows of `scan_prefix_charges_seek_then_one_group_per_entry` (469-483, prefix sums `[1, 5, 14, 20]`). Kept: the partial ledger `ScanEntry 1 / ReadByte 7` of a rejected scan is pinned only here. -16 lines. **safe**.
- `.all(|receipt| receipt.kind == ReceiptKind::..)` after an `assert_eq!` on the whole receipts: metering.rs:979-984, 2044-2048, 2081-2086, and 2062-2064; qm:3849-3854. `write_receipt`/`read_receipt` set `kind`, so the preceding equality already compared it. **DELETE** the assertions, -27 lines. **safe**.
- Field asserted, then the whole struct: metering.rs:1010 and 1082 (`receipt.outcome`, then the receipt on the next line); 629 and 642 (`ledger == None`, then `== write_receipt(..)` whose ledger is `None`). **DELETE** 4 lines. **safe**.
- qm:4720, 4745, 4748 (`range_walk_pages_revisit_every_term_of_the_run`): `walked.push(expected_id)` pushes the loop's own literal and `assert_eq!(walked, [1, 2, 3])` compares the literal list with itself; the real check is `assert_eq!(ids, [expected_id])` at 4733. **DELETE** 3 lines. **safe**.
- qm:4734-4744 same test: the receipt is compared against `traced(read_receipt(cost, ..), ledger)` where `ledger` was cloned out of that same receipt. Write `assert_eq!(without_ledger(&page.receipt), read_receipt(expected_cost, Outcome::Applied))`. **safe**.
- Constant pins duplicated: `CellName::MAX_LEN == 32`, `Limits::DEFAULT_INDEXED_LEN == 128`, `Value::INDEXED_LEN_CEILING == 512` are asserted at scalar_profile.rs:10-12 and again as a tuple at cell_limits.rs:87-94 (and `MAX_LEN` at records.rs:680); `CODEC_VERSION == 1` at scalar_profile.rs:7 and codec.rs:155. Keep the cell_limits.rs/codec.rs copies, delete scalar_profile.rs:7 and 10-12. The name-length loop at scalar_profile.rs:13-22 (lengths 0, 20, 21, 32, 33) overlaps bounds.rs:34-41 (33, empty); the `"é".repeat(20)` row at bounds.rs:36 (40 bytes: the bound counts bytes, not characters) exists only there. Add that row to the loop's file or leave bounds.rs:34-41 alone; only the constant asserts are pure duplicates. -4 lines. **safe**.

#### Kept (looked redundant, is not)

- Block-budget pair `block_budget_vector_admits_charges_and_records_exactly_as_specified` (metering.rs:842) / `block_budget_vector_for_typed_ops_admits_charges_and_records_as_specified` (qm:3700): admission is one op-agnostic function (`src/engine.rs:2356 fn admit`), so "limit above remaining is refused unrecorded", "limit == remaining is admitted", "0 remaining" and "estimates obey admission" are exercised twice. Kept: each also pins charges the other cannot (qm:3749-3765 is the only pin of a delete stopped between index removes, ledger `IndexRemove 5`).
- Estimate, overflow and pricing pairs (metering.rs:1299/1456/1050/1125/1196/1840 vs qm:4028/4225/4432/4323/4541): parallel structure, disjoint numbers. Kept: pin raw vs typed receipts under `uniform(2, 100)` and `u64::MAX` schedules.
- `repeated_reads_on_a_snapshot_yield_identical_receipts` (metering.rs:1628-1665): receipts repeat 440-452 and 593-614, but it is the only check that the `physical` trace of a repeated read is stable. Kept.
- `identical_op_sequences_on_fresh_engines_are_identical` (metering.rs:1759): kept, pins cross-instance determinism.
- `create_with_no_new_term_charges_lookup_binding_group_and_six_inserts` (qm:1142) vs row 0 of `each_new_term_costs_exactly_thirty_more_than_the_same_create` (qm:1464): the former sweeps every budget, the latter anchors the +30 series. Kept.
- qm literal traced ledgers written after a vector sweep (1117-1139, 1175-1195, 1309-1329, 1372-1395, 1422-1441, 1745-1772, 2100-2118, 2574-2599; ~150 lines): each equals what the sweep's traced leg already asserted through `ledger_after` (qm:849-851), so they pin the helper rather than the engine. Kept: they are the only hand-written check of first-charge ordering (e.g. `RootUpdate` sitting before `IndexInsert`). Owner's call whether three (create a, the patch, the query page) are enough; not counted in the totals. The 11-line patch ledger is written twice (1758-1770 and 4078-4090).
- `planned_reads_match_the_plans_worked_vectors` (qm:4752-4804) repeats the three count fast paths of query_planner.rs `counts_are_answered_from_stored_sizes` (672, 676, 695-698: `1 + 5 + 4`, `1 + 5`, `1 + 5 + 4 + 4`) on the 3-record fixture. Kept: only place the planned charges are checkable by hand. It is the one planned-evaluation test outside query_planner.rs and sits under qm's "11. Pricing pinning" banner.
- `projections_and_rows_are_byte_identical` (query_planner.rs:552): kept, only test comparing full rows under four projections.
- `query_strings_obey_the_indexed_limit` (cell_limits.rs:247) vs bounds.rs:261-319: same fault for `eq`/`starts_with`/`terms`, but configured vs default limit, and cell_limits adds `Term::range`. Kept.
- `all_scalars_prove_equality_and_reject_typed_substitution` (scalar_profile.rs:121): eq_proofs.rs:1031 covers only Bool/I64/U64/Str. Kept; it would read better in eq_proofs.rs using that file's `request`/`produce`/`verify` helpers (eq_proofs.rs:31-76).
- `profile_limits_are_explicit_and_namespaces_stay_independent` (scalar_profile.rs:5), `foreign_genesis_magic_and_state_format_are_rejected` (233), `defaults_are_explicit_and_below_the_caps` (cell_limits.rs:82): constant-looking, but kept: pin `LAYOUT_VERSION`/`STATE_FORMAT`/`PROOF_VERSION`, the genesis magic and the default limits that are part of the genesis encoding.
- All 7 tests in trace_regression.rs: kept, regression tests for a real bug with hand-derived traces.

### II.2 [S3] arkiv-db-storage, arkiv-db-memory, arkiv-db-redb src

*(repo-relative paths)*

Context for all of T1: `crates/arkiv-db-memory/tests/versioned_contract.rs:65-67` already runs the 30 contract cases against `MemoryVersionedStore` in the same `cargo test -p arkiv-db-memory`.

**T1. `crates/arkiv-db-memory/src/versioned.rs` unit tests (6) - 5 are copies of contract cases.**

| test | line | verdict | reason |
| --- | --- | --- | --- |
| `objects_are_put_if_absent_per_space` | `:318` | MERGE into contract `objects_put_if_absent` (`contract.rs:310`), then DELETE | same scenario as `objects_put_if_absent` + `object_spaces_are_separate` (`contract.rs:344`); its only extra is `objects_written == 1` after an idempotent repeat (`:331-332`). Move those two asserts into the contract case so redb is held to them too. |
| `records_are_insert_only` | `:356` | MERGE into contract `records_are_insert_only` (`contract.rs:363`), then DELETE | same name, same scenario; only extra is `records_written == 1` (`:365`). |
| `head_is_compare_and_set` | `:386` | MERGE into contract `head_compare_and_set` (`contract.rs:268`), then DELETE | only extra is `(committed, not_committed) == (2, 3)` (`:409-410`). |
| `identical_retry_is_never_applied_twice` | `:414` | DELETE | strict subset of contract `retry_after_commit_is_rejected` (`contract.rs:461`); add `assert_eq!(after.records_written, 0)` there. |
| `identity_is_fixed_and_health_never_fails` | `:529` | DELETE | getter returns the constructor argument; `health()` is a constant (`:168-170`); `recover` on an empty store is `empty_store_answers_absent` (`contract.rs:203-204`), the `recoveries` counter is `recover_is_idempotent` (`contract.rs:860`); the `assert_ne!` of two random ids is the same probabilistic check as T3. |
| `relabel_replaces_exactly_the_retired_records_and_keeps_objects` | `:431` | MOVE to the contract suite (T2) | kept: the only test of memory `relabel`. |

If T2 is taken the whole `mod tests` (`:295-541`, 247 lines) goes; otherwise -125 lines.

**T2. `relabel` and `stage_objects` have no contract case; each backend has a private twin.** The memory unit test above (`versioned.rs:431-526`) and `crates/arkiv-db-redb/tests/relabel.rs:101-150` (`relabel_replaces_only_an_exact_publication_and_survives_reopen`) build the same `refused` list (stale head, one retired record missing, one differing, one extra, ...), assert "unchanged" after each refusal, commit, and assert the repeat is refused. `stage_objects` is tested only for redb (`crates/arkiv-db-redb/tests/streaming_import.rs:16-39`). Fix: two contract cases, `relabel_replaces_exactly_the_retired_records` and `stage_objects_is_headless_and_refuses_a_published_store`; redb keeps only its crash/reopen half. This is also what would have caught W1. **needs-care**; roughly -95 (memory) -50 (redb twin) +70 (contract).

**T6. Instantiation overlap (test instances, not source lines).** `versioned_store_contract!` is instantiated 7 times (`rg -n 'versioned_store_contract!\(' crates`): 2 in `arkiv-db-memory/tests/versioned_contract.rs:66,70`, 5 in `arkiv-db-redb/tests/contract.rs:129-145`. 30 cases x 7 = 210 tests.
- `memory` and `faulty_memory` run twice: `arkiv-db-redb/tests/contract.rs:98-126, :140-146` repeats them with keccak ids, which the memory store never inspects (`arkiv-db-memory/src/versioned.rs:21-22`). 60 duplicate instances, 36 lines. That file is another reviewer's; flagged here because its twin is mine. DELETE the redb copy.
- the 11 `faulty_*` cases wrap the factory's store themselves (`contract.rs:870-873`), so under the three `Wrapped*` factories they test `FaultyStore<Box<FaultyStore<_>>>`: 33 instances that add nothing. Fix: split the macro's list into `@base` and `@faulty` and give wrapped factories only `@base`.
- `reopen_preserves_state` returns at once for volatile stores (`contract.rs:555-557`): 4 of its 7 instances pass without asserting anything.

**T4. `faulty_without_faults_is_transparent`** (`testing/contract.rs:875-897`) vs the `Wrapped*` factories. `WrappedMemory` (`arkiv-db-memory/tests/versioned_contract.rs:50-63`; again `arkiv-db-redb/tests/contract.rs:113-126`) and `WrappedRedb` (`arkiv-db-redb/tests/contract.rs:78-96`) run all 19 base cases through an unarmed `FaultyStore`, which is the strong form of this case; its one extra assert is `injected() == Default::default()`. Two mechanisms for one property. Verdict: DELETE the case (keep the wrapped runs), or keep the 23-line case and drop the three wrapped instantiations (-90 test instances, about -60 lines). Either; not both.

**T5. `load_object_reports_missing`** (`testing/contract.rs:657-665`). MERGE into `empty_store_answers_absent` (`:180-205`) as one assert: a 5-line case that costs a fresh store (a redb file with synced init) in each of 7 instantiations. `bounded_reads_enforce_lengths` already asserts the bounded twin (`:721-727`).

**T3. `random_database_ids_differ`** - `crates/arkiv-db-storage/src/versioned.rs:1294`. DELETE. `assert_ne!(DatabaseId::random(), DatabaseId::random())` cannot fail short of a 2^-128 event or a broken OS, and the second line asserts the `Display` of a constant under an unrelated name. If the hex form matters for log greps, keep that one line as `database_id_displays_as_lowercase_hex`.

**Kept, with reason:**
- `versioned.rs` `head_encoding_round_trips_and_rejects_other_lengths` (`:1156`): pins the on-disk head encoding. `record_shapes_are_checked_per_table` (`:1172`), `validate_rejects_non_advancing_heads_and_batch_conflicts` (`:1191`), `relabel_validation_requires_a_first_commit_and_distinct_retired_records` (`:1227`), `stats_differences_saturate_and_drop_idle_spaces` (`:1268`): each is the only direct test of its function; the contract cases `malformed_batches_are_not_committed` and `batch_duplicates` re-check a subset through a store, which is the point of those cases.
- redb `testing.rs` (7 tests, `:687-853`): none trivial. They pin the crash-device model (sync vs log, torn subsets, cut-off, crash-point numbering, both quotas, lost sync) that every durability test stands on. Only remark: `quota_and_injected_faults` (`:779`) is two unrelated tests in one body.
- `metrics.rs:105`, `store.rs:1247` and `:1289`: keep; the last pins the initial file layout (7 tables, 2 entries).
- `persistent.rs` (5 tests, `:546-690`): good model-based tests; they go only if S1 deletes the file.
- contract `retry_after_commit_is_rejected` (`:461`), `empty_batch_advances_head` (`:450`), `faulty_indeterminate_not_applied_retries_once` (`:1048`, close to the `DuringCommit` arm of `faulty_disk_full_then_retry_commits_once` `:1129`): overlapping but each asserts something the other does not (stats delta; non-contiguous heights; `Io` vs `Full` and the final `Committed` reconcile). Not worth touching.

### II.2 [T4] arkiv-db tests: codec / namespaces / blocks / rollback / roots / proofs

*(paths relative to `crates/arkiv-db/tests/`)*

Golden vectors, receipts, state roots and proof bytes are all kept. What can go:

#### 2.1 DELETE
- `values_round_trip_through_shared_storage` - codec.rs:1306-1315 - DELETE - strict subset of `type_tags_and_values` (codec.rs:446 `golden(&Value::str("hé"), ..)` does encode, decode, re-encode, trailing byte and truncation); `Value::str` is `Self::Str(Arc::from(text))` (`crates/arkiv-db/src/record.rs:587`), so the hand-built `Value::Str(Arc::clone(&text))` is the same value. Its comment promises "decoding yields equal values with fresh buffers" and "Str and Bytes", but nothing checks buffer identity and only `Str` is used. -10 lines and the `sync::Arc` import. **safe**
- `every_indexable_type_is_ordered` - ordered_ranges.rs:247-252 - DELETE - identical to golem_types.rs:181-183, and both restate the implementation (`is_ordered() { self.is_indexable() }`, `is_indexable() { !matches!(self, Self::Bytes) }`, `crates/arkiv-db/src/record.rs:379-391`). Keep the golem_types copy as the one pin of "only Bytes is unindexed". -6 lines. **safe**
- `a_fresh_state_has_no_host_entries` - host_state.rs:121-130 - DELETE - subset: genesis empty is asserted at host_state.rs:342 (`snapshots_keep_their_own_host_state`), a branch with no host writes stays empty after every step at 736-740 and its snapshot at 791 (`the_data_plane_never_sees_host_state`), absent-key gets at 195, 271, 353. -10 lines. **safe**
- `range_scaling_experiment` - range_proofs.rs:929-973 - MOVE to `crates/arkiv-db/examples/` (arkiv-db-redb keeps its experiments there) or DELETE - an `#[ignore]`d benchmark driven by `RANGE_BENCH_ROWS` that prints timings; its doc says "Never runs in CI". Also the only line over 150 chars in the chunk (931, 166 chars). -45 lines. **safe**

#### 2.2 MERGE / TABLE-DRIVE
- `an_empty_domain_panics`, `a_256_byte_domain_panics`, `a_domain_with_a_space_panics`, `a_non_ascii_domain_panics` - codec.rs:1282-1304 - TABLE-DRIVE into one `invalid_domains_panic`: `for domain in ["", &"a".repeat(256), "arkiv header/v1", "arkiv/é/v1"] { let panic = std::panic::catch_unwind(|| digest(domain, b"x")).unwrap_err(); assert!(panic.downcast_ref::<String>().is_some_and(|m| m.contains("invalid digest domain"))); }`. 23 -> ~10 lines. **safe**
- `keccak_is_the_trusted_primitive` - codec.rs:1240-1246 - MERGE into `digests_hash_a_length_prefixed_domain_then_the_bytes` (1249) as its first assertion: it tests alloy's `keccak256`, not this crate, and exists only to anchor that test. The vector is preserved. -5 lines. **safe**
- `signed_numeric_extremes_and_each_supported_numeric_index` (range_proofs.rs:165-222) + `golem_grid_ordered_types_prove_complete_ranges` (range_proofs.rs:977-1021) - TABLE-DRIVE into one test over a shared `ordered_samples()` (1.7): one mixed-type fixture (keeps the second test's "other types in the same attribute never leak in"), and per family the first test's `(b, Unbounded)`, `(Unbounded, b)`, `(b, b)` for inclusive and exclusive `b` plus the second's `[f[1], f[last])`. Covers 16 types instead of 5 + 11 with two ad-hoc tables. Model-based, no golden bytes involved. ~-60 lines. **needs-care**
  - Related overlap, kept: `every_scalar_type_orders_and_proves_like_the_model` (prefix_free_terms.rs:409-507) also proves four half-open ranges per value for 23 types. Its `terms` ordering and equality proofs per type are unique; its range half (488-504) would become redundant with the merged test above.

#### 2.3 SPLIT (kept: pins receipts)
- `six_step_flow_publishes_ordered_blocks_over_two_namespaces` - blocks.rs:277-628 - 352 lines, clippy cognitive complexity 81. Pins receipts 53/53, 19/12/22/26/26/28, 23, `rejected(1)` twice and the 0-cost unrecorded `ExceedsBlockBudget`, so it stays. Split along its own step comments, each from a small fixture fn:
  - `fn two_shops(engine: &mut Engine) -> (NamespaceId, NamespaceId)` = 283-325
  - `last_write_per_key_wins_and_head_never_shows_an_open_candidate` = 327-437
  - `transfer_then_three_kinds_of_rejection_under_a_block_budget` = 441-531
  - 533-556 ("Step 5": block-2 state through the retained handle, `engine.snapshot`, `blocks.at`) restates what `every_one_of_fifty_published_blocks_stays_readable_and_unchanged` (blocks.rs:1224) checks through the same three paths for 51 blocks, and its descriptor part is `descriptor_history_reflects_published_transfers_only` (namespaces.rs:1119): candidate to drop.
  - 558-616 ("Step 6": one block over two namespaces is all-or-nothing) is the weaker form of `a_block_touching_four_namespaces_becomes_visible_all_at_once` (blocks.rs:771); its two receipts (12, 30) use formulas already pinned at 343-350 and 375-382: candidate to drop.
  README.md:285 and docs/testing.md:150 name "the six-step flow"; update them with the rename. **needs-care**

#### 2.4 Redundant assertions inside otherwise good tests (all **safe**)
- namespaces.rs:966-979 - two `assert_ne!(.. owner, Some(CAROL))` / `Some(DAVE)` blocks implied by the exact `assert_eq!(value(snapshot.describe(ns(4))), expected)` at 961-965. DELETE 14 lines.
- Assertions whose own comment calls them vacuous: blocks.rs:909-914 ("C holds no records, so shared-root identity is vacuous here") and namespaces.rs:851-856 ("A is empty, so this identity check is vacuous"). DELETE 6 + 6 lines.
- codec.rs:1015-1020 - `fixture.len() == 4 + 4 + 31 * 8` and three slice checks on bytes the test just assembled with its own `be64`; the pin is `golden(&PriceSchedule::FIXTURE, &fixture)` at 1021. DELETE 6 lines.
- blocks.rs:1259-1263 - five asserts on `expected_k`, a vector the test built itself. DELETE 5 lines (or keep one).
- blocks.rs:1151-1154 (`assert_ne!(resolve(2), resolve(1))`, implied by 1063 + 1150) and blocks.rs:1171 (`assert_ne!(sequence, block)` right before `assert_eq!(sequence, block + 1)`). DELETE 5 lines.
- rollback.rs:942 - `assert_eq!(format!("{:?}", branch.receipts()), format!("{before:?}"))` directly after `assert_eq!(branch.receipts(), before.as_slice())`: derived `Debug` of equal values. DELETE 1 line.
- rollback.rs:293, 308, 497-498 - `namespace_count()` / `desc(..)` asserts implied by the `observe(..) == expected_view(..)` just above (which covers the count and the descriptor of ordinals 1..=6). DELETE 4 lines. (The neighbouring `value(..)` asserts go through `get`, a different read path: keep.)
- namespaces.rs:471 - `format!("{ghost}")` after `ghost.to_string()` at 470, same `Display` path. DELETE 1 line. namespaces.rs:834 - `empty.receipt == large.receipt` after both were compared to `expected_receipt` at 832-833. DELETE 1 line.
- roots.rs:1060 - `assert_ne!(sealed.state_root(), base.state_root())` implied by 1062 `assert!(roots.insert(..))` (the set is seeded with the base root at 1051). DELETE 1 line.
- host_state.rs:205-208 - `HostStateError::ValueTooLarge { len: 257 }.to_string() == "host value of 257 bytes exceeds 256 bytes"`: `Display` text of an error; `rg -n "HostStateError" crates` finds no consumer outside `crates/arkiv-db/src/engine.rs`. DELETE 4 lines unless the text is meant as API.
- golem_types.rs:137, 139, 143-146 in `every_type_id_is_admitted_and_named` - `TypeTag::ALL.len() == 25` cannot fail (`ALL: [Self; 25]`, record.rs:250); `from_id(tag.id()) == Some(tag)` and `from_id(unknown) == None` are covered more strongly by codec.rs:403-442 (literal id per tag; `Decode for TypeTag` is `TypeTag::from_id`, `crates/arkiv-db/src/codec.rs:841-848`) and codec.rs:1116 (exactly 25 of 256 bytes accepted). Keep the name round trip (unique names), the id ordering and the `one_of_each` coverage check. DELETE 7 lines.
- eq_proofs.rs:282-288 - first block of `namespace_absence_term_absence_and_genesis_are_distinct` (genesis -> `NamespaceAbsent`) is re-proved byte-exactly by `empty_namespace_proof_has_hand_built_canonical_bytes` (1137-1147). Optional: DELETE 7 lines and drop `_and_genesis` from the name.

#### 2.5 Misplaced, kept
- namespaces.rs:464-475 and 487-490 - `NamespaceId` byte / `Display` / `Ord` pins sit inside `unknown_namespaces_are_rejected_after_the_directory_charge`. kept: pins id bytes and the `ns:…` text. Move to their own engine-free `namespace_ids_are_eight_big_endian_bytes`. **safe**
- codec.rs:155 - `assert_eq!(arkiv_db::CODEC_VERSION, 1)` sits inside `integers_are_fixed_width_big_endian`. kept: pins the codec version (scalar_profile.rs:7 pins it too). Move out of the integer test. **safe**

#### 2.6 Looked at and kept as they are
- codec.rs: the other 23 tests are already one generic harness (`golden<T>` codec.rs:110-129, `rejects<T>` 133, `accepted<T>` 139) applied to literal vectors; there is no per-type round-trip copy-paste to collapse and no test of derive behaviour beyond the items above.
- rollback.rs:313-364 - four one-call tests already sharing `assert_rejection_is_atomic` (248-311). `published_snapshot_omits_rolled_back_ops_and_keeps_later_ones` (1050) overlaps `nested_checkpoints_..` (430) but is the only test that publishes after rolling back a *delete*.
- eq_proofs.rs:577 (literal EqQuery/SnapshotRef/EqCursor bytes), 625 (literal charges 497/498/7/1/1/40), 1104 (hand-built proof bytes); golem_types.rs:189 (golem stored forms); roots.rs (all three): consensus pins.

### II.2 [S1] arkiv-db src: engine, typed, planner, record, layout, codec, meter

*(paths relative to `crates/arkiv-db/src/`)*

Unit tests in this chunk: engine 2, record 4, layout 8, codec 2, meter 4 (typed, planner: none).

| Test | Location | Verdict | Reason |
| --- | --- | --- | --- |
| `keys_hold_one_to_thirty_two_bytes` | record.rs:993-999 | DELETE | Strict subset of `crates/arkiv-db/tests/records.rs:643 record_keys_take_1_to_32_bytes_and_encode_as_length_and_bytes` (same `MAX_LEN == 32`, empty, 33-byte asserts; that test round-trips every length). |
| `names_follow_the_grammar` | record.rs:1001-1018 | DELETE | Strict subset of `tests/records.rs:679 cell_names_follow_the_grammar_and_order_bytewise`: every good name, every bad name, `Empty`, `TooLong` and both ordering asserts appear there. |
| `ids_start_at_one` | record.rs:1076-1083 | DELETE | Subset of `tests/records.rs:669 record_ids_are_positive_ordinals`; the only extra is the `Debug` string `"RecordId(7)"`. |
| `term_bytes_preserve_order` | record.rs:1020-1074 | MERGE into `tests/records.rs:750 values_have_frozen_term_bytes_and_encoded_lengths` | Same golden vectors (I64 -1/0/MIN/MAX, U64 258, "ab", bools, Bytes -> None), same ordering, `cmp_ordered` and `encoded_len` asserts; round-trips for all types are in `tests/golem_types.rs:158`. Unique here: the malformed-input rejections at 1051-1052 and 1054 (Bytes tag, bool byte 2, invalid UTF-8) - move those three lines, delete the rest. Also clippy cognitive-complexity 26/25. The encoding stays pinned by the integration test. |
| `value_tags_are_the_type_ids` | codec.rs:1456-1462 | DELETE | Subset of `tests/codec.rs:402 type_tags_and_values`, which goldens every tag against a literal id (encode and decode) and rejects unknown ids. |
| `model_three_appends_four_classes_with_their_fixture_weights` | meter.rs:732-756 | TRIM + rename (e.g. `classes_are_listed_in_index_order`) | Keep the `class.index() == index` loop (745-747) and the `MODEL_VERSION` pin: kept, pins class order and counting-rule version. Drop 736-744 (restates the `ALL` literal) and 748-755 (FIXTURE weights, already pinned in full by `tests/codec.rs:1005 price_schedules_carry_exactly_31_weights`). Name and the comment at 753 describe a past version bump. |
| `a_term_map_iterates_in_model_order` | layout.rs:877-901 | kept (optional DELETE, 25 lines) | Implied by `term_order_is_the_derived_tuple_order` (layout.rs:845) plus `crates/arkiv-db-trie/tests/encoding.rs:251 a_map_over_composite_keys_iterates_in_tuple_order`; kept by default: pins index iteration order end to end. |
| `a_failed_operation_leaves_the_branch_state_as_it_was` | engine.rs:3063-3139 | kept; hoist | Recovery behaviour. The second half of the loop body (3129-3137, failing host put) does not depend on `allowance` and runs identically `needed` times: move it after the loop. **safe**. |
| `str_prefix_runs_hold_exactly_the_strings_with_the_prefix` | layout.rs:980-1019 | kept; move helper | Pins the prefix-run property of the key encoding, but exercises `TermKey::str_prefix_run`, a `#[cfg(test)]` fn sitting inside the production impl (layout.rs:262-271). Move that fn into `mod tests`. **safe**. |

Kept without change: `engine_handles_are_send_and_sync` (compile-time contract), layout `raw_key_order_is_bytewise_order`, `record_keys_are_length_prefixed`, `term_order_is_the_derived_tuple_order`, `term_keys_have_the_documented_layout`, `leaf_values_never_encode_empty` (pins the hard-coded `encodes_empty` overrides against the real encodings), `leaf_values_round_trip_and_reject_noncanonical_bytes` (all pin on-disk key/leaf bytes); codec `sequences_reserve_at_most_the_remaining_input` (allocation-bound guard, needs the private `reservable`); meter `groups_charge_atomically_..`, `overflow_is_reported_..`, `ledger_aggregates_..` (metering, `Meter` is crate-private).

Test-local helpers that duplicate ones elsewhere: layout.rs:648-663 `struct Rng(u64)` SplitMix64 with `next()`/`below(usize)` (`rg "SplitMix64|0x9e37_79b9_7f4a_7c15"` hits 14 files in the workspace); engine.rs:3005-3035 `struct Reader` (fail-after-N `ObjectReader` double; 14 `impl ObjectReader for` test doubles exist workspace-wide), engine.rs:3037-3039 `fn name(text: &str) -> CellName` (29 copies of that signature in the workspace), engine.rs:3041-3050 `fn record(key: &[u8], kind: &str) -> WriteOp`.

### II.2 [N4] arkiv-node deterministic core

*(`src/` = `crates/arkiv-node/src/`)*

#### `src/gas.rs` (12 tests, 316-719): keep all
All twelve pin rent arithmetic that reaches `TxOutcome::rent_wei`. `tests/gas_execution.rs` (16 tests) is end-to-end
through the signed `Host` and uses `gas::rent` as its oracle (e.g. 554, 578), so there is no strict-subset overlap.
Only tightening:
- `the_default_storage_cap_binds_no_block_the_input_cap_admits` 409-421: line 413 restates the implementation
  (`786_432 + 128 * (1_460 + 32 * 128)`) right before 414 pins the number (`1_497_600`): DELETE 413. Lines 415-418
  `assert_eq!(DEFAULT.max(bound), DEFAULT)` is `assert!(bound <= DEFAULT_MAX_NEW_STORAGE_UNITS_PER_BLOCK)`. safe
- `quote_is_exact_and_schedule_identity_is_pinned` 708-719: nothing in it pins the schedule identity (neither
  `SCHEDULE_ID` nor the 31 weights; it only calls `validate_schedule`), and 709-712 test `reference_fee_glm`, a pub
  fn with no other caller. Rename to what it tests, and add the missing pin (see S6). kept: pins fee text.
- `the_clock_bounds_the_period_and_the_cap` 424-425 restate two constants, kept: they bound which genesis is valid.

#### `src/value_json.rs` (2 tests): keep both (pin the wire form of every scalar).

#### `src/arkiv/tests.rs` (33 tests)
- **T1 `scalar_types_stay_distinct` 353-359: DELETE** after moving line 354 (`I64` is not an SDK type) into
  `every_type_id_decodes_with_and_without_the_indexed_flag`. Lines 355-358 assert that two variants of an enum with
  explicit discriminants differ; that test already decodes `Bytes32` and `Key` back to distinct values.
- **T2 `sdk_abi_and_tombstone_validation` 266-298: MERGE into two existing tests, then delete.** 267-284 (round trip
  of ten values) is a strict subset of `every_type_id_decodes_...` 407-431, which feeds the same ten values through
  `encode_abi_value` -> `decode_protocol_triples` -> `decode_abi_value`. 288-296 (type-0 tombstone refused in a
  create, accepted in a patch) is a strict subset of `tombstones_are_unflagged_empty_and_patch_only` 461-477. Unique
  and to move: 285-287 (malformed bool / i32 / addr words) into the first, 297 (duplicate name) into the second,
  asserting `Err(ArkivFault::Duplicate)` instead of `is_err()`.
- **T3 `every_type_id_decodes_with_and_without_the_indexed_flag` 432-444: DELETE the block** and its two helpers
  `type_id` (370-379) and `encode_protocol_triple` (380-390), ~33 lines. Line 438 asserts
  `encoded.type_id == type_id(&cell)` where `encode_protocol_triple` sets exactly `type_id: type_id(cell)?` (387);
  439 restates the helper's own `if cell.is_attribute()`; 440-443 decode the triple again, which 412-430 already do.
  The block tests test-local code.
- **T4 `natural_storage_is_not_abi_or_signed_order_encoding` 746-755: DELETE together with the dead
  `encode_natural_value`/`decode_natural_value`** (S3). Move the one ABI pin (749: `I32(-1)` is 32 bytes of `ff`)
  next to the malformed-word asserts of T2. needs-care: confirm first that the "natural" encoding is not a planned API.
- **T5 `glm_failed_batches_roll_back_rent_debits` 1370-1402: DELETE if `execute_batch` leaves the library (D5),
  otherwise keep.** It drives the test-only `execute_batch`; the same scenario (create + delete of a missing key ->
  `RolledBack`, `rent_wei` zero, balance and burn restored) is asserted through the production executor at
  `tests/gas_execution.rs:266-285`, and the success half by 1261-1313 and 1647-1656. needs-care
- **T6 `glm_growth_of_a_permanent_entity_pays_the_capped_lifetime` 1225 duplicates
  `glm_updates_permanent_entities_with_capped_rent` 1048-1062** (grow `note = "grown"` at height 5 pays
  `rent(0, 11, MAX, MAX, 5)` == `CLOCK.wei(11 * CAP)`). MERGE: drop 1048-1062 from the latter (keep the removal
  patch by first setting `note` without asserting). kept: the number stays pinned once.
- **T7 `glm_permanent()` 864-921 is a fixture that tests**: it asserts (865, 893-917) that the waived permanent
  create is metered but rent-free and that nothing is scheduled at `u64::MAX`, and those asserts re-run for each of
  its 11 direct callers plus 5 `glm_funded` calls. Move the asserts into one test
  (`a_sponsored_permanent_prefill_row_is_metered_but_rent_free`); the fixture only builds. safe
- **T8 `payload_and_content_type_are_gone_from_every_write_path` 481-542: keep, RENAME.** It is the only test of a
  `$` name in `decode_protocol_triples`, in a `Create` and in a patch `remove`; but it is framed as the absence of a
  removed feature. Call it `host_owned_names_are_refused_on_every_write_path`. Same history tell in the comment at 81.
- **T9 `batch_failure_rolls_back_cells_indexes_nonce_and_schedule` 124-157: keep** (only test of creation-nonce and
  due-schedule rollback), but it also depends on `execute_batch` (D5).
- Kept as they are (pin rent numbers, outcomes or the SDK fixture): `lifecycle_is_committed_owner_authority`,
  `readonly_permissionless_and_exact_expiry`, `authority_and_projection_reject_corrupt_system_cells`,
  `decimal_exact_extremes_and_sdk_names`, `nonce_exhaustion_and_expiry_overflow_do_not_write`,
  `sdk_generated_key_and_decimal_fixtures`, `tombstones_are_unflagged_empty_and_patch_only`,
  `indexed_and_unindexed_attributes_are_attributes_and_fields`,
  `a_patch_writes_each_attribute_with_the_flag_of_its_new_triple`,
  `the_entity_bytes_cap_holds_on_create_and_across_patches`, `system_cells_leave_room_for_every_user_attribute`,
  `authenticated_engine_rows_with_forged_system_types_fail_arkiv_verification`,
  `generic_namespaces_without_glm_cannot_receive_arkiv_entities`,
  `glm_prices_every_lifetime_at_or_beyond_the_cap_alike`, `glm_extends_finite_entities_up_to_the_cap_and_no_further`,
  `glm_rent_is_debited_and_burned_outside_the_gas_budget`, `glm_rent_without_the_balance_is_refused_before_any_write`,
  `only_the_owner_extends_beyond_one_rent_cap`, `the_prefill_waiver_precedes_any_quote`,
  `the_admission_quote_matches_execution`, `a_transaction_quote_sees_its_own_earlier_operations`,
  `the_quote_margin_covers_an_extension_crossing_the_cap`, `str_values_take_any_utf8_within_the_length_cap`,
  `a_full_expiry_height_spills_to_the_next_with_room`. The GLM ones repeat scenarios of the `gas.rs` unit tests one
  layer up (they check that `execute_operation` passes the right old/new/height to `gas::rent`): not a subset.
- No name overlap with `tests/arkiv_profile.rs` (4 simulator acceptance tests).
- Coverage note: `ArkivFault::Flags`, `Owner` and `GasOverflow` are asserted nowhere (`rg` over `src/arkiv/tests.rs`
  and `tests/`), `Duplicate` only as `is_err()` (297).

#### Clippy redundant clones (all safe, remove `.clone()`)
`src/arkiv/tests.rs:194` (`key: k.clone()`), `:1130` (`Delete { key: k.clone() }`), `:1565` and `:1629`
(`key: permanent.clone()`), `:1642` (`key: created.clone()`): each is the last use of the binding.

#### Local helpers (for a shared-support design)
`src/arkiv/tests.rs` is a unit-test module using private items (`count`, `put_count`, `rejected`, `name`,
`create_cells`), so it cannot move to `tests/support`; the names collide with `tests/gas_execution.rs`:
- `const CLOCK: RentClock` 6-9 = `(1_000 ms, 1_000 s)`; `src/gas.rs:305` has a `CLOCK` = `(2_000 ms, 1 year)`.
- `fn setup() -> (Engine, Branch, ArkivExecutionContext<'static>)` 10-41
- `fn create(flags: u8) -> ArkivOperation` 42-52 (gas_execution.rs:41 is `create(salt: u128, body: usize, lifetime: u64)`)
- `fn key(c: &ArkivExecutionContext, nonce: u64) -> RecordKey` 53-55 (gas_execution.rs:85 is `key(nonce: u64, salt: u128)`)
- `fn run(b: &mut Branch, c: &ArkivExecutionContext, op: &ArkivOperation) -> OpOutcome` 56-60;
  `fn run_rent(..same..) -> ArkivOutcome` 922-924 (`run` is `run_rent(..).outcome`)
- `fn read(b: &Branch, k: &RecordKey) -> ArkivEntityProjection` 61-70; `fn applied(o: OpOutcome)` 71-73
- `fn triple(name_text: &str, type_id: u8, value: Vec<u8>) -> AbiAttribute` 361-369; `fn every_type() -> [Value; 10]` 391-404
- `fn glm_permanent() -> (Engine, Branch, ArkivExecutionContext<'static>, RecordKey)` 864-921;
  `fn glm_funded(balance: U256) -> (Engine, Branch, ArkivExecutionContext<'static>)` 1237-1248
- `fn spent(outcome: &OpOutcome) -> u64` 927-932; `fn rent_of(executed: &ArkivOutcome) -> u128` 934-940;
  `fn rent_charged(b, c, op) -> u128` 1136-1138
- `fn create_until(salt: u128, expiry: Expiry) -> ArkivOperation` 941-948; `fn at(expires_at: u64) -> Expiry` 949-954
- `fn units_of(b: &Branch, key: &RecordKey) -> u64` 955-971
- `fn balance(b: &Branch, c: &ArkivExecutionContext, actor: [u8; 20]) -> U256` 1249-1255 and
  `fn burnt(b: &Branch) -> U256` 1256-1258 (gas_execution.rs:101/104 take `&Host` and `OwnerId`)
`src/gas.rs` tests: `fn clock(period_ms: u64, cap_seconds: u64) -> RentClock` 312-314 (gas_execution.rs:98 is
`clock(h: &Host)`), `struct Walk` 532-605, `fn later(expiry: u64, by: u64) -> u64` 606-612, an inline splitmix64
closure 659-667 (one more local PRNG next to the xorshift `Generator` copies in arkiv-db tests).
Banner comments in test code of this chunk: 0.

### II.2 [NT1] arkiv-node tests: core

*(`T/` = `crates/arkiv-node/tests/`)*

Headline: validation.rs, identity.rs and selection.rs are NOT one-assert-per-test families. All their tests are already loops over case tables (e.g. `T/validation.rs:206-256`, `T/selection.rs:131-148`, `T/genesis.rs:129-228`). Three tests in the chunk are deletable, three merge into one, one tests only test code.

| test | file:line | verdict | reason |
|---|---|---|---|
| `in_flight_identities_stay_pending` | `T/identity.rs:148-180` (doc + fn) | DELETE (33 lines) | No call models "taken into a candidate" or "candidate discarded": :159-162 only builds a `BlockInputs` value. Every assert is already made on the same transitions by `unknown_identities_are_accepted_once_then_pending` (:32 Accepted, :38 Pending, :40-45 Conflict, :46-49 status) and `committed_identities_move_to_the_inclusion_index` (:80-88 commit -> Included, stronger). If the sentence "the index has no in-flight state" matters, put it as a comment on the first test. **safe** |
| `the_spec_exposes_the_genesis` | `T/genesis.rs:318-328` | DELETE (11 lines) | Getter test. Each value is pinned elsewhere: protocol 5 by the genesis golden bytes (`T/block_format.rs:162`), the genesis hash by `GENESIS_HASH` (`T/block_format.rs:323`) and by every block-1 golden in replay.rs, `is_admin` through `check_admin` (`crates/arkiv-node/src/validate.rs:290`) in `T/validation.rs:273-281`. **safe** |
| `headers_carry_the_format_version_and_the_state_root` | `T/replay.rs:1390-1399` | DELETE (10 lines) | Strict subset of `headers_and_block_hashes` (`T/block_format.rs:245-280`), which asserts the full 156-byte golden encoding: `be32(5)` first, state root last, length 156 (:266). **safe** |
| `a_due_count_without_records_is_inconsistent`, `a_total_that_disagrees_with_its_counts_is_inconsistent`, `a_total_without_counts_is_inconsistent` | `T/exec.rs:85-133` | TABLE-DRIVE into one `inconsistent_due_schedules_abort_the_block` | Same chain, same `run`, same assert shape; only the host entries and the `ScheduleFault` differ. Table of `(Vec<([u8; 32], u64)>, ScheduleFault)`, like the neighbour `malformed_schedule_entries_are_inconsistent` (:135-163) already is. 49 -> ~25 lines. kept: pins the three `ExecError::ScheduleInconsistent` abort paths. **safe** |
| `oracle_reservation_matches_the_parameters` | `T/block_model.rs:2053-2083` | REWRITE as a differential check, or DELETE | It calls only `Weights::reservation` (:332-374), i.e. test code; no production function runs. The Applied / ReservationViolated / ReservationOverflow classification is already forced through the real chain by the coverage list (:2010-2012). The one fact it adds is the `tight` case (:2075-2082). Fix: `assert_eq!(w.reservation().is_ok(), arkiv_node::params::check_reservation(&PARAMS, &LIMITS, &w.schedule()).is_ok())` for the seven schedules - then the name is true. **needs-care** |
| `signed_genesis_rejects_other_wire_versions` | `T/gas_execution.rs:620-630` | MERGE with `T/accounts.rs:375-377` | The first three asserts of `genesis_and_signature_commit_accounts` (encode, 4-byte version prefix, decode round trip) are the first three asserts here (:623-625); one uses `VERSION`, the other the literal `1u32`. Keep the gas_execution.rs test (it adds the rejection loop), use `VERSION` in it, delete accounts.rs:375-377. It is about the signed wire format, not gas: its natural home is signed_protocol.rs (other chunk). **safe** |
| tail of `named_namespace_charges_fees_replays_and_rolls_back_on_low_budget` | `T/accounts.rs:659-669` | MERGE into the loop at `T/gas_execution.rs:842-873` | Both assert `"UnsupportedGasOperation"` for a generic op on the named namespace `bricks` (Put here; PatchRecord and TransferNamespace there). One table of three ops. accounts.rs:660 also signs a throwaway transaction just to steal its envelope. 11 lines. **safe** |
| second half of `permanent_create_pays_rent_outside_gas_and_rollback_retains_gas_and_nonce` | `T/gas_execution.rs:266-285` | MERGE into `failed_operations_rollback_value_and_entities_but_pay_and_consume_nonce` (`T/accounts.rs:278-342`) | Both submit `[rent-bearing create, delete of a missing key]` and assert `ops[0]` RolledBack, entity absent, nonce consumed, `balance == before - fee(spent)` (accounts.rs:308-321, :329-342). Unique here: `rent_wei == 0` (:279) and the burnt total (:282). Move those two asserts, delete the rest. kept: pins "a rollback refunds rent, not gas". ~18 lines. **needs-care** |
| `selection.rs` line | `T/selection.rs:130` | DELETE line | `assert_eq!(put_tx_bytes(1, 100), 190)` checks the test's own arithmetic helper against a literal; the helper is validated against the real encoder at :45. **safe** |
| `gas_execution.rs` line | `T/gas_execution.rs:373-374` | DELETE or make real | `assert!(accounts::fee(1_000_000) < need - U256::from(1u64))` is arithmetic on two test values (true iff `rent > 1`, and :362 already asserts `rent > 0`); the host is not involved although the comment says "Enough for the gas reservation alone is not enough either". Either submit against `tight_host(&tight, accounts::fee(1_000_000))` and expect `"InsufficientBalance"`, or drop both lines. **needs-care** |

Checked and kept (overlap is across layers, by design):
- `blocks_must_follow_their_parent` (`T/exec.rs:330-363`, `ExecError::InvalidBlock`) vs the parent-hash / timestamp cases of `replay_rejects_invalid_blocks` (`T/replay.rs:1243-1262`, `ChainError::InvalidBlock`): two code paths. kept.
- `blocks_with_repeated_or_included_identities_are_invalid` (`T/identity.rs:182-249`, `RequestIndex` directly) vs `replay_rejects_duplicate_identities` (`T/replay.rs:1091-1165`, chain and replay wiring). kept.
- The static-validation cases inside `replay_rejects_invalid_blocks` (`T/replay.rs:1199-1242`) repeat validation.rs / selection.rs faults one layer up; they pin that replay calls `check_tx` / `check_admin` / `select::check`. kept; the budget loop `[100_001, 0]` (:1210) could be one value, since `T/validation.rs:41-45` owns the edges.
- `the_host_expiry_read_cannot_overflow` (`T/exec.rs:396`) vs `the_expiry_read_must_be_representable` (`T/genesis.rs:52`): kept: pins metering edge at two layers (see 1.1 #17).
- `chains_record_inclusions` (`T/identity.rs:251-269`): small, pins the recorded height. kept.
- All 13 tests of block_format.rs and the 12 fixture tests of replay.rs: kept: pin canonical encoding, digests, golden block hashes, hand-derived receipts.

#### 2.1 block_model.rs: is the reference model a restatement of the executor? (emphasis c)

Size: the oracle is `T/block_model.rs:113-1188` (Weights 263 lines, record model 127, `Model` 684) plus `T/support/block_roots.rs` (197) = about 1,270 lines of second implementation; the harness around it is another ~970 (query catalogue 83, `verify` 118, Rng + Coverage 99, `Generator` 259, `generate` 193, tests 300).

Verdict: KEEP, it earns its weight, with three caveats.
- The charge-table half (`delete_charges` :581, `engine` :763, `create_charges` :911, `patch_charges` :955, `verdict` :475) is a genuinely different shape from the engine (charge groups with semantic check points), predicts every receipt of 4 x 60 seeded blocks and 200 demo blocks, and block_roots.rs is the only independent construction of the state root. kept: pins receipt metering and the state-format-2 root.
- The host-rule half is a transcription of the node source, so it gives little independent evidence: `Model::resolve` (`T/block_model.rs:645-660`) is statement-for-statement `resolve` in `crates/arkiv-node/src/exec.rs:368-386` (same `floor`, same `checked_add(..).ok_or(Overflow)`, same check order); `enum Life { Until, Forever }` (:400-404) mirrors `enum Lifetime { Until, Permanent }` (src/exec.rs:349-352) down to the derive-ordering trick; the pre-step :688-727 mirrors `admit_expiry` (src/exec.rs:1244-1276); `Weights::reservation` (:332-374) mirrors `max_expiry_delete_cost` / `sweep_read_bound` (`crates/arkiv-node/src/params.rs:163-213`). A wrong rule would be wrong on both sides. The independent pins for those rules are the literal expectations in replay.rs (`expiry_at_the_boundary`, `expiry_at_capacity`, `expiry_extends_only`, `permanent_expiry`, `unresolvable_expiry_is_host_rejected`) and genesis.rs - which is the reason not to thin replay.rs. Say so in the module doc instead of "The oracle never calls the engine's pricing or execution" alone.
- Trim rather than delete: 1.1 #3, #13, #15 and 4.2-4.5 below remove about 90 lines without touching what it checks.
- `docs/testing.md:257` mentions other oracles that also transcribe the charge table (arkiv-db tests, not read by me); whether those can share one transcription is a cross-chunk question.

### II.2 [NT2] arkiv-node tests: signed

*(paths relative to `crates/arkiv-node/tests/`)*

No whole test in this chunk is trivial; signed_history.rs (22) and signed_pool.rs (9) in particular contain no constructor/getter/derive tests.
Actions are partial deletes, merges and splits.

Partial DELETE / MERGE:
- signed_protocol.rs:105-108, in `signed_genesis_bounds_native_state_and_reserves_complete_outcomes` - DELETE the assertion
  `MIN_ENTITY_CELLS == SYSTEM_CELLS + MAX_ATTRIBUTES`: it restates the definition (crates/arkiv-node/src/arkiv/schema.rs:81
  `pub const MIN_ENTITY_CELLS: usize = SYSTEM_CELLS + MAX_ATTRIBUTES;`). The next assertion (:109-114, the default genesis uses it) is real; keep. -4. safe.
- signed_pool.rs:92-103, tail of `duplicates_conflicts_and_failed_admission_preserve_pending` - MERGE into
  `byte_sender_entry_and_per_sender_limits_are_inclusive_and_do_not_evict` (:185): its first case (`max_entries: 1`, :190-193, loop :203-213)
  builds the same pool and asserts the same `PoolFull`; add the one missing line (`admit(first.clone(), 0) == Ok(Admission::Duplicate)` after the refusal)
  to that loop, which then checks it for all three limits. -11. safe.
- signed_http.rs:1280-1309 (malformed range witness, inside the 163-line range test) - MERGE into
  `altered_or_truncated_proof_never_returns_verified_rows` (:856) as a third bad source: same loop body as :880-907
  (bad source, light with certificates 1..=2, POST, none of `rows`/`verification`/`inspection`/`canonicalProof` present); the row is
  `(route "/signed/v1/query/range", endpoint "query/range/inspect", range_query())`. -20. needs-care (the check then runs on the `group` chain of that test instead of the `price` chain).

TABLE-DRIVE:
- signed_http.rs `future_sampled_and_advertised_conflict_freezes_before_replication` (:963) with
  `authentic_replication_certificate_conflict_persists_freeze_before_execution` (:1022): they differ in which certificate conflicts
  (:967-972 vs :1027-1031), the expected body fetches (0 at :1008, 1 at :1067) and one nonce assertion (:1072-1073). One helper
  `frozen_follower(certificates, sampled, block) -> (Shared, bodies, history_store)` and two short tests, or one table. -45. safe. kept: both pin where the freeze happens relative to the body fetch.
- signed_protocol.rs `every_authority_field_and_operation_is_signed` (:357, 122 lines) and
  `header_certificates_authenticate_all_native_fields_and_genesis` (:542): table of named mutations (1.4 item 1). kept: pins what the signature covers.

Subset of a unit test - SPLIT OUT, trim optional:
- ethereum_transactions.rs:393-431, inside `sdk_permanent_create_is_stored_at_the_sentinel` ("SDK 1.0.1's query shapes"): nothing here is about a
  permanent create, and each assertion has a stricter twin in the unit tests:
  count beside a page, offset past the end, `select: {count: true}` alone -> `select_count_is_the_match_set_on_every_page` (crates/arkiv-node/src/signed/rpc_reads.rs:2759);
  `$key BETWEEN key(..) AND key(..)` -> `between_and_ranges_take_every_indexable_type` (rpc_reads.rs:2986, line 3008);
  `ORDER BY $expiresAt:u64` -> rpc_reads.rs:2872; `* ORDER BY score` is a type error -> `order_by_errors_use_the_sdk_code_table` (rpc_reads.rs:2947);
  `STARTSWITH u64(1)` is -32002 through `handle_request` -> `sdk_query_refusals_answer_with_the_sdk_code_table` (crates/arkiv-node/src/signed/rpc.rs:2233).
  Verdict: move to its own test `sdk_1_0_1_query_shapes`; if the literal SDK request (hex `limit`/`offset` with `count`, :399-406) is the pin, keep that
  one request and drop the other 30 lines. needs-care (SDK compatibility pin).
- kept: ethereum_transactions.rs:385-392 (`arkiv_getEntity`). Its semantics are covered by `get_entity_returns_the_full_live_entity_or_null`
  (rpc_reads.rs:2013), but it is the only place the method is reached through `rpc::handle_request` (rg `arkiv_getEntity"` over src and tests: only :389, :391).
- kept: ethereum_transactions.rs:238-279 (attribute schema of the viem-encoded entity). `unindexed_attributes_are_returned_but_never_matched`
  (rpc_reads.rs:2246) covers the rule on a hand-built host; this pins the SDK tag bytes -> indexed flag on producer and follower.

Same scenario at host level and through HTTP (signed_host.rs vs signed_http.rs) - all kept, no test is a strict subset:
- signed_host.rs `signed_lifecycle_replays_bootstrap_ownership_failures_and_native_proofs` (:168) and signed_http.rs
  `external_admission_full_replay_and_light_paginated_proofs` (:185) walk the same pipeline. Shared assertions:
  `ChainInitializing` (host :180-183, http :247-257), `ProducerOnly` (host :184, http :376-406), duplicate stays `pending` (host :193-200, http :289-299),
  `included` after the block (host :255, http :334-345), follower head equals producer head, light verifies the Eq proof (host :271-280, http :429-447).
  HTTP adds the wire shape and the body-fetch counts; the host test adds ownership, patch and the failed delete.
- Conflict freeze: signed_host.rs :630, :672, :756 exercise `Host::import`/`observe_certificate`; signed_http.rs :911, :963, :1022, :1083 exercise the order
  in which `runtime::sync_once` consults the sampled header, the advertised certificates and the body. Different code; same six-line tail (1.4 item 19).
- signed_host.rs `pool_gaps_wait_and_contiguous_nonces_execute_in_one_block` (:539) is signed_pool.rs
  `gaps_become_ready_only_after_the_missing_nonce_and_order_by_arrival` (:107) through `produce`; 15 lines, keep.
- signed_host.rs `a_spill_stops_after_the_search_window` (:1170) and the unit test `a_full_expiry_search_reverts_with_a_readable_reason`
  (crates/arkiv-node/src/signed/rpc.rs:1561) build the same chain (one expiry per height, 65 creates fill the window, the 66th fails).
  The integration test pins the outcome, schedule totals and follower replay; the unit test pins logs and the revert string. Keep both.
- Two stacks: signed_http.rs:467-487 (classify table, text query verified/unsupported) and :238-243 (status capabilities) mirror
  arkiv_profile.rs:346-406 and :301-310 line for line. The routers are separate (crates/arkiv-node/src/signed/runtime.rs:759-761,
  crates/arkiv-node/src/simulator/runtime.rs:402-409) and both call `query_language::classify`; keep both.

kept, by file:
- signed_history.rs, all 22: on-disk frame and `BodyRef` encoding (:51, :172), restore/reopen (:89, :145), corruption (:201), uncertain commits and retries
  (:262, :286, :319, :332, :363, :642, :660), conflict evidence and freeze (:380, :483, :548, :568, :616), light trust boundary (:400, :437, :509, :595, :683).
  The fault families already loop (`for fault in [..]`); the only gain left is the fixture of 1.4 item 5.
- signed_pool.rs, all 9. `PoolLimits::default()` equality (:237-245) kept: pins operator-visible defaults that the loop at :246-265 builds on.
- signed_host.rs, 18 + the ignored benchmark: crash/recovery (:700, :732, :789, :825, :856), replay rejection (:480, :594), restart (:364), envelope format (:414).
- signed_retention.rs, all 4: pin the `HistoryPruned` JSON-RPC error object and the REST 410 body. `HISTORY_PRUNED_CODE == -32006` (:146) and the
  archive default (:373) are one-line pins.
- signed_reads.rs (5), arkiv_profile.rs (4), signed_protocol.rs golden/decoding/recovery tests (:136, :264, :293, :338).

### II.2 [S5] arkiv-db-trie

*(paths relative to `crates/arkiv-db-trie/`)*

Counts: 76 integration tests in 12 files, 19 unit tests in 6 src modules. Most are oracle- or
hand-derived pins and must stay. Findings:

| Test | Location | Verdict | Reason |
|---|---|---|---|
| `byte_string_encoding_edge_cases` | `tests/encoding.rs:126-143` | **MERGE** into `byte_field_vectors` (`src/order.rs:310`) | Every assertion but one restates the unit vectors: ordering/prefix lines 129-133 == the `ordered` window check at `order.rs:330-344`; `[0x01, 0x00]` rejection == `order.rs:323`; over-long rejection == `order.rs:324`; shifted `ff`/missing-terminator/over-long == `order.rs:363, 369, 370`; `len + 1` == `tests/encoding.rs:102`. Move only `escaped(&[0; 64]).len() == 129` (worst-case doubling, ideally as `== encoded_bytes_max(64)`). -17 lines. safe |
| `replacing_a_value_keeps_the_length_and_old_versions` | `tests/map.rs:408-442` | **SPLIT + trim** | Lines 415-425 (replaced value, `len`, old handle still reads) are a strict subset of `random_history` (`map.rs:102, 106, 115-117`). Unique parts: `same_root` true/false (426-428) and u64 keys iterating numerically (429-441, the only test of `impl TrieKey for u64` order). Rename to those two properties, drop the rest (-10). safe |
| `temporary_entries_collapse_back_to_the_original_root` | `tests/variable_keys.rs:393-447` | **DELETE (optional, owner's call)** | Implied by `random_histories_match_the_model` in the same file (root == both oracles after insert/overwrite/remove histories, 24 seeds) and by `tests/map.rs::root_depends_only_on_the_key_value_set` (decoys inserted and removed). No golden value. If deleted, update the header line `variable_keys.rs:10-11` and `docs/state-trie.md:432`. -55 lines. needs-care |
| tail of `missing_and_corrupt_objects_are_errors_not_absence` | `tests/store.rs:495-496` | **DELETE 2 lines** | `empty_store.commit(&Staged::new()); assert!(empty_store.is_empty());` tests the `MemoryNodeStore` test double, unrelated to the test. safe |
| tail of `stage_with_routes_nested_maps_to_another_space` | `tests/store.rs:809-814` | **DELETE 6 lines** | `from_root(EMPTY_ROOT_HASH, 0).root_hash() == EMPTY_ROOT_HASH` is unrelated to the test name and already in `tests/map.rs:392-393`. safe |
| `decoder_rejects_noncanonical_nodes` | `src/node.rs:685-759` | **TABLE-DRIVE** | 14 sequential cases, 12 of them only `is_err()`. A table `(name, bytes, expected_reason)` asserting the reason (as lines 725 and 753 already do) would pin *why* each is rejected; today e.g. `junk_padding` (:729-732) passes if rejected for any reason. needs-care |
| `self_consistent_malformed_intersecting_nodes_are_rejected` | `tests/interval_security.rs:49-118` | **TABLE-DRIVE** | Seven `bounded(root, lower, upper, nodes, max_key_len)` calls differing only in arguments/expectation (70-114). ~20 lines. safe |
| `oversize_overflow_and_zero_visit_budget_never_encode` | `tests/bounded_stage.rs:105-133` | **TABLE-DRIVE (small)** | Loops over `[1024, usize::MAX]` then branches on `len == usize::MAX` inside the loop (113-119); put the expected error in the tuple. safe |

Kept on purpose (looked like candidates, are not):
- `src/node.rs::alloy_node_decoder_panics_on_paths_beyond_32_bytes` (:805-821) tests vendored alloy, and its
  own-crate assertion is a subset of `long_paths_extend_the_same_format` (:617); **kept: cited as design evidence
  in `docs/state-trie.md:49`**.
- `tests/proofs.rs::alloy_verify_proof_accepts_*` (3 tests): kept: pin that `walk` rejects each forgery alloy accepts.
- `tests/map.rs::reference_oracle_agrees_with_hash_builder`: kept: validates the oracle used for keys > 32 bytes.
- `tests/store.rs::a_short_root_is_stored_by_hash`, `tests/bounded_stage.rs::short_roots_are_emitted_...`,
  `tests/sorted.rs::empty_single_inline_root_...`: same property on three code paths (stage_link / try_stage_link /
  builder); kept: pins on-disk object set. If 1.9 merges the walks, the first two cover one path.
- `tests/variable_keys.rs::stored_maps_reopen_walk_and_prove_identically` overlaps `store.rs::store_and_reopen`
  but is the only stored-link coverage for variable-length keys; kept.
- The three random-history drivers (`tests/map.rs:77-125`, `tests/variable_keys.rs:211-258`,
  `tests/adversarial_bounds.rs:167-243`) check the same model property on three key domains (fixed arrays,
  escaped `ByteKey`, raw prefix-violating keys); not subsets of each other; kept.
- `src/nibbles.rs` and `src/order.rs` vector tests: kept: pin hex-prefix and key encodings.
- `tests/map.rs::deep_path`, `tests/store.rs::update_creation_bounds`, `a_collapse_loads_the_stored_sibling`:
  kept: pin visit/load counts (metering).

### II.2 [N1] arkiv-node signed JSON-RPC / HTTP reads

*(paths relative to `crates/arkiv-node/`)*

Checked against `tests/signed_http.rs` and `tests/signed_reads.rs` (`rg -n "eth_|arkiv_|-320" tests/signed_http.rs tests/signed_reads.rs`): they drive `eth_blockNumber`, `arkiv_query`, `arkiv_getEntityCount` for lock/admission/concurrency behaviour and pin `ServerBusy` (-32010 / HTTP 429) and `ReadDeadlineExceeded`. None of the 37 unit tests in `rpc.rs`/`rpc_reads.rs` is a subset of them; `tests/ethereum_transactions.rs` overlaps only in using the same fixture file. So the findings are internal overlaps.

| Test | Location | Verdict | Reason |
|---|---|---|---|
| `estimate_has_no_nonce_balance_or_state_effect` | `rpc.rs:1725-1736` | DELETE | `simulation` takes `&Host`, so head/root cannot change by construction; the 21 000 estimate for a funded value transfer is asserted at `rpc.rs:1901-1905`, "accounts and head unchanged after estimates" at `rpc.rs:1794-1807` and `:1829-1834`. |
| `sdk_operators_and_bounds` | `rpc_reads.rs:1812-1838` | MERGE (trim to ~10 lines) | `:1832-1837` (three `is_err`) are the same inputs `query_errors_use_the_sdk_code_table` checks with the exact kind (`:2341, 2351-2360`). Of the parse-OK list `:1813-1825`, every form but `NOT (a = i32(1) OR b = true)` is executed against data in `actual_snapshot_reads_...` (`:2105-2113`). Keep that one string and the two structural asserts (`:1826-1831`). |
| `scalar_encodings_do_not_depend_on_magnitude` | `rpc_reads.rs:1840-1867` | SPLIT + trim | Lines `:1853-1866` test `selection`, not scalars; `:1853` (`{"payload":1}` refused) is implied by the loop `:1856-1862`. Split into `scalar_encodings_..` (kept: pins the SDK value encoding) and `select_accepts_exactly_the_sdk_keys`. |
| `actual_snapshot_reads_extended_semantics_projection_and_pinned_cursors` | `rpc_reads.rs:2120-2125` | DELETE those 6 lines | `arkiv_getBlockTiming` duration 0 at genesis is the first assertion of `timing_uses_numeric_height_and_observed_whole_seconds` (`:3212-3215`), which checks the whole object. |
| `malformed_hex_and_log_ranges_are_rejected_without_panics` | `rpc.rs:2013-2034` | MERGE one case | Move the `{"blockHash", "fromBlock"}` case (`:2018-2025`) into the table of `filters_reject_unknown_fields_and_bound_every_retained_dimension` (`:2087-2099`), which already has the `toBlock` twin and also runs `logs`. Kept: the hex list is a regression test for slicing non-ASCII input. |
| `engine_rejections_map_onto_the_sdk_table` | `rpc_reads.rs:2466-2506` | TABLE-DRIVE | Five single asserts + two loops over one mapping; one `[(R, Option<QueryErrorKind>)]` table, ~40 -> ~25 lines. Kept: pins which engine rejection maps to which SDK code. |
| `sdk_query_refusals_answer_with_the_sdk_code_table` | `rpc.rs:2270` | DELETE the line | `assert_eq!(HISTORY_PRUNED_CODE, QueryErrorKind::Block.code())` is the constant's definition (`rpc.rs:1271`). The real pin is `tests/signed_retention.rs:146` (`== -32006`). Rest kept: pins the six codes end to end. |
| `query_errors_use_the_sdk_code_table` | `rpc_reads.rs:2382-2393` | TIGHTEN | 12 lines to compare six codes; `assert_eq!([Parse, Type, ..].map(QueryErrorKind::code), [-32001, ..])`. Kept: pins the frozen SDK table. |
| `order_by_errors_use_the_sdk_code_table` | `rpc_reads.rs:2969` | DELETE the row | `("n = u64(1) n = u64(2)", Parse)` is row `:2336` of `query_errors_use_the_sdk_code_table`. |
| `clause_order_and_read_bounds_do_not_change_answers` | `rpc_reads.rs:2228-2243` | MOVE | Two `ReadLimits { .. }.validate().is_err()` asserts test `reads.rs:97`, which has no test module; they are unrelated to clause order. |
| `a_page_past_the_response_cap_...` | `rpc_reads.rs:2599-2602` | MOVE | Three `rows_within` unit asserts tacked onto a 96-line integration-style test; give them their own 5-line test. |

Kept, not trivial (each pins an external contract or is a named regression): `a_full_expiry_search_reverts_with_a_readable_reason` (revert data bytes), `log_owners_follow_operations_and_failed_batches_emit_nothing` (log topics, `rentFee`), `a_dropped_transaction_answers_with_a_definite_error`, `unfunded_estimates_match_execution_gas_without_mutating_state`, `transfer_estimation_ignores_fee_and_value_affordability`, `legacy_and_eip1559_transfers_execute_with_exact_intrinsic_gas`, `raw_send_enforces_balance_and_pending_reservations_after_estimation`, `jsonrpc_batches_notifications_and_errors_keep_protocol_shape`, `concurrent_rpc_receipt_reads_do_not_use_rest_admission_slots`, `finite_and_block_hash_filters_...`, `filters_expire_release_capacity_and_refresh_on_poll`, `execute_call_returns_abi_keys_and_missing_entity_errors_decode`, `fee_history_rejects_allocation_amplification_before_building_rewards`, all remaining `rpc_reads.rs` tests, `rpc_ws.rs:311`, both `metrics.rs` tests, the three `state_docs.rs` tests.

Minor test hygiene:
- `rpc.rs:2101` builds a fresh `host()` (genesis, sign, produce) for each of the 11 bad filters; hoist it above the loop. **safe**.
- `rpc_reads.rs:3003` `let mut sorted = every.clone();` - `every` is not used again (clippy `redundant clone`); bind `sorted` directly. `rpc_reads.rs:2585` `.map(|k| k.to_string())` -> `.map(Json::to_string)` as `:3150` already does.
- Test comments at `rpc_reads.rs:1855`, `:2298`, `rpc.rs:2262` and rows `:2344-2345`, `:2956` describe "the removed payload model"; see Comments.

### II.2 [S4] arkiv-db-redb tests and examples

*(paths relative to `crates/arkiv-db-redb/`)*

Almost everything here pins crash, recovery, durability or on-disk behaviour and stays.

1. **`memory::*` and `faulty_memory::*`** (2 x 30 generated cases), tests/contract.rs:98-126 (factories) +
   140-146 (modules) - **DELETE**. The same two factories run the same 30 cases in
   crates/arkiv-db-memory/tests/versioned_contract.rs:35-71; the only difference is `object_id`
   (keccak here, FNV there), and that file's header (lines 9-11) states the memory store never verifies
   ids. Also drop `arkiv-db-memory` from `[dev-dependencies]` (its only use in this crate:
   `rg -n arkiv_db_memory crates/arkiv-db-redb` -> tests/contract.rs:20) and the fifth bullet of the
   module doc (contract.rs:9-10). 60 tests, 37 lines. **safe**.
2. **`a_killed_store_releases_its_file_lock`**, tests/backends.rs:127-137 - **DELETE (strictly subsumed)**
   or keep as a named property. Sequence: create, commit, `probe().kill()`, drop, `RedbStore::open`,
   head is `Some`. tests/durability.rs:81-104 (`a_killed_process_loses_no_committed_block_on_a_real_file`)
   runs the identical kill/drop/open sequence with stronger assertions (id, exact head, no full repair,
   every block), as does reconciliation.rs:205-227; a lock that is not released fails their `open(..).unwrap()`
   too. 11 lines. **needs-care** (owner's call; if kept: pins lock release after kill).
3. `faulty_redb::*` (30 cases, contract.rs:136-138) - kept: pins `FaultyStore<RedbStore>` forwarding
   including reopen. Note only that it exercises the test double, and
   `faulty_without_faults_is_transparent` already runs inside `redb_file`.
4. tests/atomicity.rs:99-108 `growing_commit_in_steady_state_is_parent_or_candidate` and 153-162
   `growing_commit_crash_points_are_parent_or_candidate` - kept (different start state); share the
   closure (1.11). The three `every_crash_point_*` tests (63, 81, 88) differ by one argument but are long
   sweeps that benefit from running as separate tests - kept, not table-driven.
5. tests/engine_process.rs:89 `engine_process_child` is not a test: it returns immediately unless
   `ARKIV_ENGINE_CHILD_PATH` is set and is the body of the child spawned at line 119-126. Kept (needed),
   but it has no comment saying so; add `/// Runs only as the child process spawned by the test below.`
6. Everything in latching.rs (8), reconciliation.rs (5), relabel.rs (3), durability.rs (3),
   bounded_reads.rs (3), initialization.rs (4), disk_full.rs (3), engine_crashes.rs (5), history.rs (2),
   streaming_import.rs (2), engine_history.rs (1), isolation.rs / recovery.rs / write_amplification.rs
   (1 each), backends.rs (other 3) - kept: each pins a distinct failure, recovery, atomicity or
   on-disk-format behaviour; no two have the same body.

### II.2 [T2] arkiv-db tests: history / isolation

*(paths relative to `crates/arkiv-db/tests/`; `h` = history.rs, `qh` = query_history.rs, `i` = isolation.rs, `qi` = query_isolation.rs)*

No test in this chunk is deletable: every one pins receipts, effects, ids or node counts. Findings are
redundant asserts, one test that should be split, and optional merges.

| test | file:line | verdict | reason |
|---|---|---|---|
| `identical_terms_of_another_namespace_are_invisible_and_miss_at_fixed_cost` | qi:3109 (242 lines) | SPLIT into 3 | 3109-3274 is the named claim; 3276-3304 "a cursor binds its namespace and its projection" and 3306-3349 "raw keyspaces are separate" are different claims. The cursor part needs neither `grow_b` nor `create_namespaces`. **safe** |
| `transfer_work_is_independent_of_namespace_size` | i:1196 (112 lines) | MERGE its two halves | 1203-1245 runs both transfers on a dropped branch, 1249-1285 runs the same two again and publishes. Move the `record_nodes == 0` and effect asserts (1220-1232) onto the published run; drop 1203-1219 and 1233-1245. Dropped-branch behaviour of a transfer is already covered by `run_workload` + `assert_a_untouched_since` in tests 1-3. ~35 lines. **needs-care** |
| `a_field_expires_at_survives_expiry_without_host_filtering` | qh:4105 | keep (pins receipts 53/269/149, 94, 82); DELETE 4106-4115 and 4181-4184 | `host_accepts` is a function defined in the test and asserted once on a literal defined in the test; it exercises no engine or host code (arkiv-node has the real filter: `host_owned`, arkiv-node/src/validate.rs:171). **safe**, 14 lines |
| `replay_is_deterministic_seed_*` (x4) | h:2067 via macro | keep (pins physical-trace determinism); TIGHTEN `assert_replay` | In h every transcript event is asserted inside `run_seed` against an oracle value that depends on the seed only (`Published`: 1379-1383/1736-1740, `Write`: 1235, `Rollback`: 1298, `StaleHead`: 1766, `Read`: 942/973/987/1004), and `SnapshotId` is a bare `u64` (engine.rs:178). The event loop 1978-1988 therefore cannot fail; only the `physical` comparison 1989-1999 adds coverage. Reduce to `assert_eq!(a.physical, b.physical)` (as qh:3874 does) and move the "records the losers" check to `full_run`. **safe** |
| `replay_is_deterministic_seed_*` (x3) | qh:4271 via macro | keep | multi-group receipts are not predicted by the oracle (module doc 35-37), so the event comparison is real here |
| `charge_table_reproduces_the_decisions_file_totals` | h:2013 | keep | pins the oracle's charge table and verdict rules to literals (never touches the engine) |
| `charge_table_reproduces_the_brief_worked_vectors`, `reference_evaluator_follows_the_brief_semantics` | qh:3921, 4015 | keep | same; the second is the only self-test of any of the reference evaluators (1.5) |
| `baseline_matches_hand_derived_receipts_and_the_reference_model` | qi:2943 | keep | implied by tests 2/3 (they re-assert the literals at 2982 and 3015) except `snapshot_count() == 2`, but it is the cheap direct signal. kept: pins receipts |
| tests 1-4 of i (1073, 1087, 1118, 1151) and of qi (2943, 2958, 2994, 3028) | | optional MERGE into one phased test per file | four checkpoints of one phase sequence; each rebuilds phase 0 and three of four redo the 10 000-record `grow_b`. One test `phases 0 -> 1 -> 2 -> pinned` saves ~40 lines and 5 fixture builds per file but loses failure granularity. **needs-care**, owner's call |

Redundant assertions (strict subsets of an assertion beside them):
- isolation.rs:790-796 - `record_nodes <= 43` for three steps whose exact values 8, 5, 2 were asserted at 783-789. DELETE. **safe**
- isolation.rs:822-831 - `TRANSFER` and `DESCRIPTOR` `record_nodes == 0`; both are rows of the table (750, 780) checked at 783-789. DELETE. **safe**
- isolation.rs:797-821 - receipt/result checks of the miss and the aborted scan inside `assert_absolute_record_bounds`; in phase 0 they repeat `assert_matches_expected`. If tests 2-3 also asserted the rerun against `expected_baseline` (as qi does at 2982, 3015) these 25 lines go everywhere. **needs-care**
- isolation.rs:1143-1144 and query_isolation.rs:3020-3021 - `namespace_count() == PHASE2_NAMESPACES`, already asserted inside `create_namespaces` (i:1033, qi:2887). DELETE. **safe**
- isolation.rs:1219 - `small.receipt == large.receipt` after both were compared with `transfer_receipt()` at 1213-1218. DELETE. **safe**
- isolation.rs:1326, 1335 - assert properties of the test's own `b_key` helper; 1337 proves the same against the engine. DELETE. **safe**
- isolation.rs:1355 - `.spent` of a receipt just compared whole at 1354. DELETE. **safe**
- isolation.rs:725, query_isolation.rs:2597 - `assert!(path <= DIRECTORY_PATH_BOUND)` compares a literal 3 or 5 with a literal 17 at run time. Make it `const _: () = assert!(DIRECTORY_PATH_503 <= DIRECTORY_PATH_BOUND);` or delete. **safe**

### II.2 [D2] Scripts, CI, build config, vendor/reth-trie, conformance

*(repo-relative paths)*

There is little to delete here: the Python tests are mostly table-driven refusal cases and encoding pins, and the Rust
tests are ports of golem-db's suite.

| test | file:line | verdict | reason |
| --- | --- | --- | --- |
| `test_genesis_install_is_forwarded_to_the_producer_only_for_adopt` | scripts/test-run-full-prefill.py:225-275 | MERGE onto `run_with_fake_bin` (138-171) | Re-implements the fake proposer and the CLI call that `run_with_fake_bin(root, '--genesis-install', 'adopt')` already provides (it returns the producer argv and status). About 35 lines. needs-care |
| `test_conversion_failure_stops_pipeline_and_records_exit`, `test_expiry_policy_is_forwarded_to_converter`, `test_unlimited_policy_is_forwarded_without_finite_options` | scripts/test-run-full-prefill.py:21-38, 40-65, 67-86 | TABLE-DRIVE the invocation with one `run_cli(root, converter_source, *extra)` | Same fake converter, same argv, different flags. About 25 lines. safe |
| assert `p.PERMANENT_EXPIRY == 0xFFFFFFFFFFFFFFFF` | scripts/test-sourcify-prefill.py:640 | DELETE the line | Constant equals its own definition (sourcify-prefill.py:72); the emitted sentinel is already checked at :635 and :642. safe |
| `ConversionTests` (32 methods) | scripts/test-sourcify-prefill.py (738 lines) | DELETE with its subject if S2b is taken | Tests only the codec-2 converter that docs call outdated. Not trivial tests; listed for the size. |
| `test_existing_output_is_never_overwritten` | scripts/test-run-full-prefill.py:220-223 | kept: pins the never-overwrite contract of run-full-prefill.py:105 | |
| assert `p.STREAM_ROW_LIMIT == 16 * p.MAX_EXPIRY` | scripts/test-sourcify-prefill.py:677 | kept: pins the documented relation to the node limit | |
| `test_keccak_and_safeproxy_key`, `test_cells_follow_the_pinned_codec`, `test_ethereum_keccak_known_vectors` | test-sourcify-v4-prefill.py:375-399, test-sourcify-prefill.py:317-319 | kept: pin hashes and canonical cell encoding | |
| all 21 tests | conformance/golem-db/src/golem_tests.rs | kept: 14 ports of golem-db's API tests + 7 adapter tests, gated by expected-failures.txt | Their internal repetition mirrors upstream and makes re-porting a diff. |

Generic helpers defined locally in the test files (for a shared test-support module, if one is wanted):
- test-sourcify-prefill.py: `canonical(value)` 27-28, `entity(content, creator=CREATOR)` 31-49, `bundle(root, rows, lines=None)` 52-65,
  `helper_bundle(root, rows)` 68-96, `imported_bundle(root, rows)` 99-120, `uploaded_entity(kind, document, attrs, dependencies=None)` 123-141,
  `uploaded_fixture()` 144-159, `options(source, out, *extra)` 162-163, `frames(path)` 166-203 (independent codec-2 reader).
- test-sourcify-v4-prefill.py: `keccak` 44-45, `key_of(chain, owner, nonce)` 48-50, `cjson` 53-60, `deflate` 63-66,
  `class SyntheticBundle` 69-165, `noise(size)` 168-174, `decode` 177-181, `attr`/`raw`/`set_raw`/`find` 184-197,
  `recompile` 200-214, `kind_of` 224-225, `units` 228-236, `write_bundle` 239-270, `decode_stream(data)` 273-311
  (independent codec-3 reader), `line_cells` 314-325, `build` 328-340, `run(args, check=True)` 343-347.
  `canonical`/`cjson` and `frames`/`decode_stream` are the same idea for two codecs; they differ on purpose.
- golem_tests.rs: `check!` / `check_eq!` / `record_failure` / `finish` 39-83 (soft assertions), `limits` 90-96, `genesis` 98-100,
  `config` 102-104, `options` 106-113, `open` 116-118, `reopen` 121-126, `input(key, price)` 128-134, `each_backend(contract: fn(Db))` 136-147,
  `get(api, target)` 149-151, `allocator` 154-163, `crash_db(disk, genesis)` 1160-1178, `diagnostic(line, error)` 1182-1195, `kind(result)` 1320-1331.

### II.2 [S2] arkiv-db src: durable, proofs, small modules

*(paths relative to `crates/arkiv-db/src/`)*

This chunk has 35 unit tests. None is a pure constructor/getter test worth deleting outright. Three need work.

| Test | Location | Verdict | Reason |
|---|---|---|---|
| `raw_reads_pass_the_health_gate_without_the_cache` | durable/cache.rs:854-863 | TIGHTEN: delete lines 859-862, rename to `raw_reads_bypass_the_cache` | The second `assert_eq!` (859-862) compares `context.store.read_object` with `context.read_object`, which line 857 already established (`== Some(bytes)`). The name promises a health-gate check, but no store in the test ever fails. Either add a latched `FaultyStore` case or drop "health gate" from the name. -4 lines. |
| `owner_ids_are_opaque_bytes` | namespace.rs:200-205 | TIGHTEN to one assertion | Line 202 is `new` then `as_bytes` (constructor/getter round trip). Line 204 tests derived `PartialEq`. Only line 203 tests hand-written code (the hex `Debug` impl at namespace.rs:141-149), and it only checks a prefix. Keep one exact assertion: `assert_eq!(format!("{:?}", OwnerId::new([0xab; 20])), format!("OwnerId(0x{})", "ab".repeat(20)))`. -2 lines. |
| `posting_cache_capacity_is_fixed` | durable/streaming.rs:603-607 | MERGE into a compile-time assertion | It asserts `size_of_val(&*cache.slots) == 768 * 1024`, a fact about a struct layout and a constant. kept: pins the "exactly 768 KiB" bound stated in the doc at streaming.rs:25. Better as `const _: () = assert!(size_of::<CopiedPosting>() * POSTING_CACHE_SLOTS == 768 * 1024);` beside the constant: fails at build time and needs no allocation. -5 lines. |

Test-quality note, not a deletion:
- `rejects_duplicates_caps_and_failed_builders_without_publication` (durable/bulk.rs:854-894) drives five cases with
  `for mode in 0..5 { match mode { 0 => .., 1 => .., _ => .. } }`. A failure reports no case. TABLE-DRIVE it
  properly: `for (case, fill) in [("duplicate record", fill_dup as fn(&mut BulkGenesisBuilder)), ..]` and put
  `"{case}"` in both asserts. **safe**.

Kept, with what they pin:
- namespace.rs:185 `ids_encode_as_big_endian_ordinals_and_reject_zero`: kept: pins canonical `NamespaceId` encoding and its `Display`.
- retention.rs:220, 232, 240, 259: kept: pin the CLI grammar, the window arithmetic (including `Window(0)`), pin counting, and the monotonic floor.
- durable/mod.rs:1136 and 1217: kept: ownership of dirty nodes after prepare, and the pin-before-floor-check order.
- durable/adopt.rs:754, 789, 829, 868 and durable/streaming.rs:454, 490, 532, 560: kept: verification completeness and cache soundness.
- durable/bulk.rs:744, 767, 896: kept: run-file integrity, bulk root equals incremental root, no publication on failure.
- host.rs:344 `block_numbers_cannot_wrap`: kept: overflow regression. host.rs:370 overlaps with
  `block_numbers_and_snapshot_ids_are_distinct_sequences` (`crates/arkiv-db/tests/blocks.rs:1017`, which asserts
  the same two error variants at 1114-1140) but adds a first block other than zero and `resolve` below the
  first block; kept.
- interrupt.rs:81: kept: unwind restore and thread isolation.
- The other ten cache.rs tests: kept: eviction, CLOCK, index consistency, recovery.

### II.2 [NT3] arkiv-node tests: genesis / simulator / demo / CLI; examples; Cargo.toml; clippy.toml

*(paths relative to `crates/arkiv-node/`)*

Result: no test in this chunk is trivial, a derive/getter test, or a strict subset of another. I recommend **0 deletions**. The problem here is the opposite: grab-bag scenario functions. Verdict per test:

**tests/genesis_prefill.rs**
- `canonical_stream_and_launch_are_order_independent_and_strict` :108 - kept: pins canonical order, `VERSION` prefix, strict decode, tamper rejection.
- `invalid_nonces_system_cells_capacity_and_sponsor_fail` :150 (95 l) - SPLIT into four along its own comment boundaries: nonce/expiry/system-cell/removed-name rejections (151-163, 176-181, 231-243); rent cap (164-175); per-height capacity + sponsor funding (182-209); value/stream byte caps (210-230). Keep every assertion (pins `cap == 31_536_000`, the 128 capacity, codec refusal). Lines 171-175 overlap `src/genesis_bundle.rs:673` `permanent_rows_pay_no_time_rent_only_under_the_permanent_policy` (same "at or beyond the cap pays the cap" equality on `prefill_rent_wei`), but go through `build_genesis`, so not a subset. safe.
- `full_small_row_envelope_installs_without_truncation` :247 - kept: 256-row boundary.
- `empty_prefill_has_namespace_and_ordinary_block_one` :271 - kept: pins three hashes (state root, genesis hash, stream hash).
- `prefills_replay_proofs_next_create_expiry_and_restart` :328 (155 l, clippy cognitive 27) - SPLIT (optional): genesis-state half (349-394: account proofs, nonce key, per-row projection, due totals) into its own test; the 5-block produce/import/light/restart loop stays one scenario. kept: replay + restart behaviour.
- `permanent_policy_is_explicit_and_changes_identity_only` :517 - kept: pins launch versions 1/3 and report fields. Inline the single-use helper `has_no_permanent_fields` (568-571, defined after its caller) as two asserts (-4 l).
- `permanent_genesis_entities_survive_blocks_and_take_capped_live_updates` :573 and `streaming_permanent_genesis_installs_staged_state_and_behaves_the_same` :591 - kept. Both call `exercise_permanent_chain` (657-878, 222 l, clippy cognitive 30). SPLIT that function: `assert_only_finite_row_scheduled` (671-690), the live-operations + 6-block loop (691-825), `assert_reads_report_sentinel` (826-847), and move the status checks (848-877) into their own `#[test]`: lines 866-877 build a separate finite v1 host and assert its status, which has nothing to do with the host under test and currently runs twice with identical inputs. safe.

**tests/genesis_adopt.rs**
- `adopted_node_equals_the_copy_restarts_and_keeps_producing` :300, `an_interrupted_adoption_resumes_from_each_durable_state` :320, `a_foreign_relabelled_store_or_initialized_node_is_never_replaced` :372 - kept: crash/recovery and state equality (but see D3: they drive the test's copy of the sequencing).
- `producer_adopts_serves_the_copy_s_state_and_resumes` :538 (77 l, 7 process launches) - KEEP for now. Its phases 2-3 (583-613) repeat the two cuts of `an_interrupted_adoption_resumes_from_each_durable_state` through the real binary; after D3 they can be MERGED into one resume check.
- `producer_refuses_a_damaged_staged_store_and_misplaced_flags` :617 (117 l) - SPLIT in two: damaged staged store (618-646) / `--genesis-install` refused outside a bundle producer (648-732). The halves share only `tmp`, `bundle`, `key`. safe.

**tests/prebuilt_genesis_cli.rs**
- `producer_serves_the_prebuilt_state_without_rebuilding_and_legacy_rebuild_agrees` :236 - kept: only test of `--state-out` / `--prebuilt-state` wiring and of byte-identical bundle files through the binary. (Library-level twin: `src/genesis_prebuilt.rs:501` `kept_state_is_reused_without_rebuild_and_matches_the_rebuild_path`; overlap is intentional lib-vs-CLI.)
- `tampered_or_foreign_prebuilt_state_never_serves` :290 (160 l) - SPLIT in two: foreign/tampered state (291-342) / `--prebuilt-state` refused for non-bundle producer, full, light, keygen (344-447). The foreign half (307-318) is the CLI twin of `src/genesis_prebuilt.rs:668` line 679 (same "identity does not match" needle); keep it as the wiring check. Line 448 `assert_eq!(report["rowCount"], "24")` is a stray assertion unrelated to either half: move it next to `stream_build` in the first test.

**tests/simulator_http.rs**
- `status_reports_storage_bytes_only_for_a_known_readable_file` :90 - kept.
- `full_and_light_use_disjoint_endpoints_control_auth_and_chunked_limit` :126 (172 l) - SPLIT in three: control needs the private listener + bearer and replication is not public (127-191); light syncs with zero body fetches and pages verified rows, full follower fetches exactly 40 bodies (192-277); chunked request over `REQUEST_CAP` gets 400 (278-296, needs nothing from the rest). needs-care (shared producer fixture).
- `response_mutation_and_dropped_body_never_produce_verified_rows` :317 - kept.
- `private_network_http_is_explicit_and_unknown_configuration_fails_closed` :377 - kept: only test of `validate_peer*` / `parse_private_http` (`rg` finds no unit test in `src/simulator/runtime.rs`).
- `failed_and_busy_bootstrap_does_not_poison_directory_and_reopen_needs_no_genesis_fetch` :397 (125 l) - KEEP as one scenario (each phase depends on the directory state left by the previous); only extract the wait loops (D8).
- `inspection_returns_exact_verified_witness_and_historical_results_with_bounds` :524 (146 l) - SPLIT off the tail: cursor continuation / `CursorMismatch` (633-656) and the saturated-reads 429 (657-668) are independent of the height table (554-632).
- `inspection_rejects_oversized_witness_before_returning_any_proof_data` :672, `state_retention_refuses_pruned_proofs_with_410_on_full_and_light` :711 - kept.

**tests/simulator.rs**
- `two_hundred_blocks_match_memory_roots_receipts_history_and_proof_bytes` :63 (138 l) - KEEP whole: one expensive 200-block run pins state roots against `MemoryChain`, proof bytes, and cold-open read counts.
- `durable_controls_replay_exactly_after_restart_and_configuration_is_adopted_in_block` :203, `failure_outcomes_never_expose_uncertain_body_or_head_and_exact_retry_keeps_command` :240, `light_pins_history_verifies_pages_rejects_mutations_and_persists_conflict` :276 - kept: restart / uncertain-commit / conflict persistence.
- `bounded_serialization_and_profile_reject_unserviceable_genesis` :332 - grab-bag of five unrelated checks (json cap, `RunConfig::validate`, `Workload::validate`, hex/decimal strictness, `value_digest`). Lines 345-346 (`unhex("0xAB", 1)`, `number("01")`) are a subset of `demo_auth.rs:252-256`: `simulator::wire::{unhex, number}` only delegate to `demo::wire` (`src/simulator/wire.rs:16-21`). DELETE those two lines or keep them as the wrapper check; low value either way. Rename/split is optional.
- `metadata_binary_codec_matches_typescript_golden_and_detects_tampering` :362 - kept: golden bytes + digest.
- `collector_handles_same_block_recreation_rollback_id_reuse_and_every_field_type` :390 (169 l) - KEEP: one block scenario, long only because of the literal inputs.

**tests/simulator_crash.rs**
- `every_redb_mutating_boundary_atomically_keeps_state_body_outbox_and_inclusions` :51 - kept: crash matrix.

**tests/demo_auth.rs**
- `empty_genesis_root_and_prepared_publication` :33 - kept, but misfiled: it tests `MemoryChain::prepare/produce/apply` and `ChainSpec::new`, nothing of demo auth; it is the only caller of `.prepare(` in `tests/` (`rg -n '\.prepare\(' crates/arkiv-node/tests`). MOVE to `tests/replay.rs`.
- `literal_authentication_layout_vectors` :59 - kept: pins encoding and three goldens.
- `signature_linkage_equivocation_and_false_root_trust_boundary` :118, `unservable_candidate_and_capacity_never_publish` :214, `strict_wire_unknown_fields_versions_hex_and_decimal` :238, `queue_count_is_bounded_and_duplicate_retries_do_not_consume_capacity` :398, `workload_keys_remain_bounded_for_the_full_seed_and_height_width` :432 - kept.
- `largest_admitted_body_queue_identity_and_prepublication_failure` :263 (133 l, clippy too-many-lines) - SPLIT (optional) into queue identity + pre-publication failure (309-335) and largest admitted body + follower replay + request status (336-394).

**tests/retention_cli.rs** - both tests kept.

#### (c) Are the demo / simulator profiles legacy? Evidence says no - do not remove their tests
- Shipped and documented: `CHANGELOG.md:378-379,390-391` (release assets are `arkiv-proposer`, `arkiv-simulator` and `arkiv-demo`); `README.md:157-163` (demo) and `:178-184` (simulator) list both as features; run docs `docs/full-light-demo-run.md`, `docs/full-light-demo.md`, `docs/simulator-node.md`, `docs/simulator-proof-inspection.md`; launchers `scripts/full-light-demo.py:25,71` (+ `scripts/test-full-light-demo.py`), `scripts/arkiv-v2-demo.py:34-35`; `Dockerfile.simulator:6`.
- Load-bearing for the signed profile: `src/signed/protocol.rs:131` builds `default_native_genesis` on `demo::workload::genesis`; `src/value_json.rs:21` and `src/simulator/wire.rs:14-21` use `demo::wire::{hex, unhex, number}`; `rg -c 'simulator::' src/signed/*.rs` totals 32 references (the `Error` type, `wire::hex`, `runtime::validate_peer`).
- What *is* demo-only, if the owner ever retires that binary: `src/demo/auth.rs` (171), `src/demo/runtime.rs` (965), `src/bin/arkiv-demo.rs` (167) - `rg 'demo::(runtime|auth)'` finds users only in `src/bin/arkiv-demo.rs`, `tests/demo_auth.rs`, `tests/block_model.rs:2091`; the `ed25519-dalek` dependency has no other user. In that case `demo_auth.rs` tests at :59, :118, :214, :263, :398 go with it; :33 (MemoryChain), :238 (hex/decimal strictness used by simulator and signed) and :432 (`demo::workload`, shared) must stay.

### II.2 [N3] arkiv-node binaries, genesis_*, query_language

*(paths relative to `crates/arkiv-node/src/`)*

Genesis and binaries: **nothing to delete.** Every unit test pins behaviour that is either consensus-relevant
or crash/recovery behaviour, and none is a strict subset of an integration test (the overlap with
tests/genesis_prefill.rs, tests/genesis_adopt.rs, tests/prebuilt_genesis_cli.rs is in fixtures, Duplication 11,
not in assertions: the unit tests drive the builders, the integration tests drive `Host` or the process).

- kept: genesis_bundle.rs:673, 710, 756 - pin rent numbers, row-codec bytes, launch version numbers
  (`launch_versions_select_layout_and_permanence` restates two 4-row tables, but those numbers are signed).
- kept: genesis_authoring.rs 17 tests - pin preset hashes (:1318), a genesis hash (:1348), lock/build/verify
  and path-safety behaviour.
- kept: genesis_streaming.rs 18 tests - pin a v2 genesis hash/state root/burn (:942-955) and inode, truncation,
  restart behaviour. `bulk_matches_v1_oracle_across_progress_settings_and_reopen` (:1208): the
  `for batch in [1, 512, 65536]` loop only varies progress sampling; drop the loop (keep the reopen loop)
  when `--batch-bytes` goes (Sloppiness 4).
- kept: genesis_prebuilt.rs 7, genesis_import_resources.rs 1, bin/arkiv-proposer.rs `cadence_tests` 3,
  simulator/query_language.rs 6 (already table-driven).

Test hygiene in those modules (all **safe**):

- genesis_authoring.rs:1841-1852 `trait ErrorString` (12 lines, "Avoid requiring a Debug implementation")
  has one caller (:1703). Replace the call with `compile(&fixture.chain()).err().unwrap()` and delete the trait.
- genesis_authoring.rs:1293 `Fixture::change(&self, ..)` never reads `self` (clippy `unused self`, 22 call
  sites): make it a free `fn replace_in(path, from, to)`.
- Redundant clones (clippy): genesis_streaming.rs:865, 1044, 1103 (`g.clone()` where `g` is not used again),
  genesis_prebuilt.rs:753 (`existing.clone()`).
- genesis_streaming.rs:892-894 `summary_of` is defined between two tests and used once (:861): inline it.

query_language (58 of the 64 tests are imported, see Other 7): **recommended verdict KEEP unchanged**, because
README.md states "upstream tests are preserved" and restyling them makes the next re-sync a manual merge.
If upstream parity is given up, these are the candidates:

- lexer.rs TABLE-DRIVE `comments_run_to_end_of_line` :485, `keywords_are_case_insensitive` :495,
  `doubled_quotes_escape` :546, `names_take_dots_dashes_and_underscores` :552, `system_names_drop_the_dollar`
  :562, `signed_and_unsigned_bare_integers` :568, `comparison_operators` :587 -> one `(src, tokens)` table plus
  one `rejects` table for their four `is_err` rows (7 -> 2).
- lexer.rs MERGE `tagged_body_is_captured_raw` :530 with `a_paren_inside_a_string_does_not_close_the_literal`
  :538 (same three lines, different input).
- literal.rs MERGE `bare_numbers_are_i32` :640 into the i32 tests (`parse_bare_i32` is
  `parse_i32(..).map(AttributeValue::Int)`, literal.rs:243-245); MERGE `u256_rejects_signs` :518 (two asserts)
  into `u256_holds_its_maximum_and_rejects_more` :503; MERGE `dec_rejects_excess_precision_rather_than_rounding`
  :557 (one assert) into `dec_rejects_malformed_shapes` :563.
- parse.rs TABLE-DRIVE `every_tag_parses_to_its_own_type` :568 (nine same-shaped asserts, 65 lines -> ~20).
- parse.rs MERGE `literal_validation_errors_are_their_own_kind` :673 and
  `a_system_attribute_rejects_the_wrong_type` :869 into one `(query, kind)` table.
- parse.rs MERGE `the_removed_symbol_operators_are_gone` :912 into
  `malformed_queries_are_syntax_errors_with_positions` :1017: four of its five inputs repeat
  lexer.rs:602-609; only the `IN` row is new.
- parse.rs DELETE `attribute_names_are_case_sensitive` :991: two different names give two different ASTs
  because the name is copied verbatim.
- parse.rs:645-647 (local adaptation, so in scope either way): the comment "And their index bytes differ, so
  they hash to different buckets" no longer describes `assert_ne!(left, right)`, which is already implied by
  :640 and :645. DELETE both lines. **safe**.

Generic-looking unit-test helpers (for a shared support module):

| helper | where | signature / note |
|---|---|---|
| `Temp` | genesis_streaming.rs:612-629; genesis_prebuilt.rs:386-403 | `Temp::new() -> Temp(PathBuf)`, `Drop` removes; differ only in the directory prefix |
| `Fixture` | genesis_authoring.rs:1260-1315 | `new()`, `chain()`, `inline()`, `bundle() -> PathBuf`, `change(&self, &Path, &str, &str)`, `jsonl_manifest()` |
| `base` | genesis_streaming.rs:630-644; genesis_prebuilt.rs:404-419 | `() -> SignedGenesis` vs `(run: u8) -> SignedGenesis` (second also funds `[1; 20]`) |
| `row`, `mixed`, `flip_body` | genesis_streaming.rs:645-663, 715-723, 665-672 | `(n: u64) -> PrefillEntity`; `(&mut PrefillEntity)` |
| `limits` | genesis_streaming.rs:673-679 | `() -> PrefillLimits` (streaming maxima) |
| `stream` | genesis_streaming.rs:680-690; genesis_prebuilt.rs:420-449 | `(&[PrefillEntity]) -> Vec<u8>` vs `(rows: u64) -> Vec<u8>` / `stream_expiring(rows, impl Fn(u64) -> u64)` |
| `signed_stream`, `signed_permanent_stream` | genesis_streaming.rs:691-713, 727-751 | `(&[PrefillEntity]) -> SignedGenesis`; `(SignedGenesis, &[PrefillEntity]) -> SignedGenesis` |
| `author`, `inode`, `names`, `state_of` | genesis_prebuilt.rs:456-498 | `author(root, tag, run, rows, keep) -> Built` |
| `kinds` | query_language/lexer.rs:476-482 | `(&str) -> Vec<Token>` |
| `i32_of`, `dec_of`, `u256_of`, `word` | query_language/literal.rs:450-467 | upstream |
| `user`, `built_in`, `kind_of`, `addr_hex`, `word_hex` | query_language/parse.rs:544-563 | upstream |

### II.2 [N2] arkiv-node runtimes: signed / simulator / demo

*(paths relative to `crates/arkiv-node/`)*

Only two inline tests live in this chunk; both stay.
- `range_policy_tests::configurable_policy_rejects_invalid_values` simulator/wire.rs:409 - kept: pins env policy parsing.
- `fetch_retry_tests::explicit_busy_get_retries_but_posts_never_do` signed/runtime.rs:1196 - kept: pins the GET-only retry rule.
No test helpers are defined in these files.
Observation for the owners of tests/ (not my chunk): `SignedTx::from_bytes` / `SignedBlock::from_bytes`
(signed/protocol.rs:391-396, 611-616) have no caller in `src/` (`rg "Signed(Tx|Block)::from_bytes" crates/arkiv-node/src`
-> only the definitions); they are exercised only by `tests/signed_protocol.rs:285-289, 331` and
`tests/ethereum_transactions.rs:55`. Production decodes through `wire::unpack` (signed/runtime.rs:775, 1169) with
different error codes. Those tests pin an API nothing uses.
