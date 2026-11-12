# Code-quality review, 2026-11-17: Appendices

Reviewed at `e6c49e1`; line numbers refer to that commit. Parts: [summary](README.md) · [duplication](duplication.md) · [tests](tests.md) · [comments](comments.md) · [sloppiness](sloppiness.md) · [other](other.md) · [appendices](appendices.md)

# Appendices


## Appendix A: area notes and direct answers

The preamble of each area: what was read, conventions, and the reviewer's direct answers to the questions put to that area.


### A [T1] arkiv-db tests: metering family

Files read in full (all under `crates/arkiv-db/tests/`): `query_metering.rs` (4804 lines, 38 tests), `metering.rs` (2088, 35), `trace_regression.rs` (872, 7), `query_planner.rs` (938, 7), `scalar_profile.rs` (337, 7), `cell_limits.rs` (340, 6), `bounds.rs` (355, 3). 103 tests.

Short paths below (`metering.rs:77`) mean `crates/arkiv-db/tests/metering.rs:77`. "qm" = `query_metering.rs`.

Headline answer to "does metering.rs overlap query_metering.rs": no test in either file is a subset of a test in the other. metering.rs covers the raw ops (Put/Delete/Transfer/CreateNamespace/get/scan/descriptor), qm the typed ops, and every parallel pair pins different charge classes. What is duplicated between them is the harness (section 1), and the real test redundancy is inside metering.rs itself and inside qm's vector literals (section 2).

### A [T2] arkiv-db tests: history / isolation

All four files read in full (2080 + 4283 + 1374 + 3455 = 11 192 lines). Paths below are relative to
`crates/arkiv-db/tests/`. "h" = history.rs, "qh" = query_history.rs, "i" = isolation.rs, "qi" = query_isolation.rs.
Nothing proposed here changes a pinned receipt, id, cost literal or node count; no `[CONSENSUS]` item.

Headline: the two "query_*" files were written by copying their plain sibling and adding the typed
dimension. Plain `diff` LCS: 1004 of h's 2080 lines recur in order in qh (649 excluding blank/brace-only
lines); 515 of i's 1374 lines recur in qi (327 non-trivial). The scenarios themselves are NOT subsets
(raw Put/Delete/Transfer/scan vs typed records/queries/expiry), so no test can be dropped on that
ground; it is the scaffolding that is duplicated.

### A [T3] arkiv-db tests: queries / records / pagination / terms

Chunk: `crates/arkiv-db/tests/{queries,records,pagination,terms}.rs` (10 131 lines, 78 tests), all read in full.
Paths below are relative to `crates/arkiv-db/tests/` unless they start with `crates/` or `docs/`.
`qm` = `query_metering.rs` (another reviewer's chunk; I read the parts cited here only to verify duplication).

The two biggest results, both crossing chunk borders:

- **A. `records.rs` re-pins what `query_metering.rs` already pins, on the same fixture with the same numbers** (15 of its 30 tests, ~950 lines). Details in 2.1.
- **B. One record/query reference model is hand-copied into each of my four files** (and, by rg, three more files). Details in 1.1.

### A [T4] arkiv-db tests: codec / namespaces / blocks / rollback / roots / proofs

Scope: `crates/arkiv-db/tests/{codec,namespaces,blocks,rollback,roots,eq_proofs,range_proofs,prefix_free_terms,ordered_ranges,golem_types,host_state}.rs`, 10,910 lines, 118 `#[test]` fns. Every file was read top to bottom. All paths below are relative to `crates/arkiv-db/tests/` unless they start with `crates/`. Nothing in the repo was modified; cargo was not run.

Short version: the tests themselves are mostly strong (hand-built vectors, independent models, exact receipts). The slop is around them: the same ~25 helpers, a store double and a Branch/Snapshot reader trait re-typed per file under clashing names, 125 lines of banners, plan-step labels, and a handful of redundant assertions/tests. Very little can be deleted outright; a lot can be shared.

### A [T5] arkiv-db tests: durable / faults / KV; examples

Scope (all read in full): `crates/arkiv-db/tests/` durable_failures.rs (1580), storage_faults.rs (897), simulator_composition.rs (879), rehash.rs (730), adopt.rs (489), fallible_api.rs (465), streaming_import.rs (430), durable_retention.rs (326), support/durable_history.rs (336), durable.rs (244), model.rs (209), interrupt.rs (179), backend.rs (165), kv.rs (162), semantics.rs (137); `crates/arkiv-db/examples/` records.rs (513), blocks.rs (181), basic.rs (28). Also read for comparison: `crates/arkiv-db-storage/src/testing/faulty.rs` (444), `crates/arkiv-db-memory/src/versioned.rs`.

All paths below are relative to `crates/arkiv-db/` unless they start with `crates/`.

### A [S1] arkiv-db src: engine, typed, planner, record, layout, codec, meter

All seven files read in full (10,085 lines). Paths below are relative to `crates/arkiv-db/src/` unless they start with `crates/`.
Nearly everything here is consensus-relevant; every fix proposed is a pure refactor unless tagged otherwise. `[CONSENSUS]` = the fix sits on a path that produces canonical bytes, charges or receipts, so it must be byte/charge-identical and checked against the golden tests.

Comment share (lines starting `//`): engine 19% (599/3140), typed 13%, planner 18%, record 23%, layout 17%, codec 14%, meter 27%.

### A [S2] arkiv-db src: durable, proofs, small modules

All paths are relative to `crates/arkiv-db/src/` unless they start with `crates/`.
All 20 files were read in full (9,764 lines). Nothing was compiled or run.

Direct answers to the task's lettered questions, up front:

- **(a) parallel import paths and eq/range proofs**: see Duplication D1 (proofs), D2-D4 (the four import paths).
- **(b) `impl VersionedStore` in `durable/adopt.rs`**: it is test-only. `Probe` and both trait impls sit at
  adopt.rs:625-686, inside `#[cfg(test)] mod tests` (adopt.rs:611-873). It cannot be replaced by
  `arkiv_db_storage::testing::FaultyStore` as that type stands: `FaultyStore` fails reads by scope and count
  (`ReadScope::{AllObjects, Space, Records, Head, Everything}`, faulty.rs:48-59) but cannot (1) record which
  object ids were read, (2) return `None` for one chosen id, or (3) flip a byte of one chosen object, which are
  the three things `Probe` exists for. See Duplication D15 for the fix.
- **(c) `durable/simulator.rs:146`**: not a bug. The line is
  `if self.height != manifest.sequence || self.state_root != manifest.state_root`. Clippy's nursery lint
  `suspicious_operation_groupings` fires because the second comparison uses the same field name on both sides
  and the first does not. `EngineManifest` has no `height` field (format.rs:188-194); `sequence` is the right
  one. The cause is the naming split `SimulatorPacket::height` vs `EngineManifest::sequence`. Leave the code.
- **(d) `#[allow]` sites**: five of the six `#[allow]` attributes in this chunk hide nothing. All four
  `too_many_arguments` are on 7-parameter functions (the lint fires above 7); two of the three
  `result_large_err` are on functions whose outer `Err` is the small `StorageError`. See Sloppiness S1.
- **(e) comments**: see Comments C1-C7. query.rs has 23 restating doc lines; bulk.rs needs about six more, not fewer.
- **(f) dead pub items and trivial cache.rs tests**: see Sloppiness S2-S6 and section 2. None of the 11 cache.rs
  tests is trivial; one has a redundant assertion and a misleading name.
- **(g) lib.rs re-exports**: `STATE_ROOT_DOMAIN` is used nowhere outside the crate; the `Database` / `KvStore`
  group and `BlockMap` / `HostError` are used only by arkiv-db's own tests and examples. See S4-S6.

### A [S3] arkiv-db-storage, arkiv-db-memory, arkiv-db-redb src

All 16 files read in full: `crates/arkiv-db-storage/src/{versioned.rs,batch.rs,lib.rs,testing/mod.rs,testing/faulty.rs,testing/contract.rs}`, `crates/arkiv-db-memory/{src/lib.rs,src/versioned.rs,src/persistent.rs,tests/versioned_contract.rs}`, `crates/arkiv-db-redb/src/{lib.rs,error.rs,metrics.rs,counting.rs,store.rs,testing.rs}` (7213 lines). Paths below are repo-relative. Other crates were only rg'd / spot-read to verify use and duplication.

Where the questions put to this area are answered: (a) D2-D6, T2 · (b) D1 · (c) W2, W3, D9 · (d) S1, W5 · (e) C1-C9 · (f) T1-T6 · (g) S3 · (h) S2.

Inside each category the findings are ordered by payoff; the ids (D1, T3, ...) are stable labels used for cross-references, not ranks.

Top three, in order of importance:
1. **W1** - `FaultyStore` silently drops `stage_objects` / `relabel` (trait default bodies answer "unsupported"); its doc claims the opposite and no contract case would notice.
2. **S1** - `arkiv-db-memory/src/persistent.rs` (691 lines) is exported and has no user anywhere.
3. **D2-D5** - the two backends share four hand-copied pieces that belong in `arkiv-db-storage`.

### A [S4] arkiv-db-redb tests and examples

Scope read in full: tests/common/* (8 files, 1,579 lines), tests/engine_support/mod.rs (226),
18 tests/*.rs (2,558), 6 examples (2,209). All paths below are relative to
`crates/arkiv-db-redb/` unless they start with `crates/` or `docs/`.
No section-banner comments (`// -----`) in this chunk: 0.

### A [S5] arkiv-db-trie

Every file under `src/` and `tests/` was read in full. Paths below are relative to
`crates/arkiv-db-trie/`. "rg" claims were run over `crates/ conformance/ scripts/` (and `docs/` where stated).
Clippy note up front: the `redundant clone` (node.rs:677,729,813) and single-item `into_iter`
(node.rs:687,711,717) warnings are all inside `mod tests` of node.rs, not in the encoder (see 4.9).

### A [N1] arkiv-node signed JSON-RPC / HTTP reads

Files read in full (all under `crates/arkiv-node/src/signed/`): `rpc.rs` (2301; 1488 code + 813 tests),
`rpc_reads.rs` (3227; 1787 + 1440), `rpc_ws.rs` (369), `ethereum.rs` (352), `range_api.rs` (193),
`state_api.rs` (102), `state_docs.rs` (180), `reads.rs` (210), `metrics.rs` (622).
Paths below are relative to `crates/arkiv-node/` unless they start with `crates/`.
RPC/HTTP response bodies are an external contract: anything tagged `[CONSENSUS]` changes one.

### A [N2] arkiv-node runtimes: signed / simulator / demo

Paths are relative to `crates/arkiv-node/`. All 22 files in the chunk were read in full
(signed: runtime, host, protocol, light, pool, history, crypto, mod; simulator: runtime, host,
light, wire, metadata, inspection, workload, profile, mod; demo: runtime, wire, auth, workload, mod).

#### [N2] Answer to the key question: are demo/ and simulator/ legacy?

Neither is dead, but the layering is upside down and each generation re-typed the previous one.

**demo/ (1603 lines)** - still a shipped product: `src/bin/arkiv-demo.rs` (167), `tests/demo_auth.rs` (443),
`tests/block_model.rs:2088-`, `scripts/full-light-demo.py`, `scripts/test-full-light-demo.py`,
`docs/full-light-demo*.md`, `README.md:157-207`, `CHANGELOG.md:379-391` (release artifact).
Inside the library only two small pieces of it are used by anything else:
- `demo::wire::{hex, unhex, number}` (demo/wire.rs:14-40, 53-61) - used by `simulator/wire.rs:13-21` and `value_json.rs:21`.
- `demo::workload::{genesis, ALICE, BOB}` (demo/workload.rs:11-50) - used by `simulator/host.rs:49`,
  `simulator/runtime.rs:150`, `simulator/workload.rs:31`, `signed/protocol.rs:131`, `bin/arkiv-simulator.rs:191`,
  `arkiv/tests.rs:846`, 4 simulator test files and 2 examples.
  **[CONSENSUS]** `signed::protocol::default_native_genesis` (protocol.rs:130-146) starts from this function, and
  `simulator::host::RunConfig::validate` (host.rs:49-55) rejects any genesis whose limits/params differ from it.
  So the literals in demo/workload.rs:20-47 are chain parameters of all three profiles, living in "demo".
Everything else in demo/ (`runtime.rs`, `auth.rs`, the DTO half of `wire.rs`, `workload::inputs`: ~1525 lines)
is reachable only from the demo binary and its tests. `lib.rs:25` ("The signed demo remains an independent
volatile profile") holds only for `demo::{runtime, auth}`: simulator and signed both import from demo (line is
outside my chunk; noted because it answers the question).

**simulator/ (3381 lines in chunk)** - also shipped (`bin/arkiv-simulator.rs`, `Dockerfile.simulator`,
`docs/simulator-node.md`, `scripts/arkiv-v2-demo.py`, `tests/simulator*.rs`, `tests/retention_cli.rs`), and it is the
foundation of signed/: `signed/mod.rs:3` re-exports `simulator::{Error, Result}`; signed uses `simulator::wire`
(14 use sites), `inspection` (10), `light::{VerifiedRequest, resolve}`, `metadata::{capture, record_change}`,
`profile`, `query_language`, `runtime::validate_peer`, `host::uuid`.
Profile-specific and used only by the simulator binary/tests/examples: `runtime.rs` except 75-112, `host.rs` except
`uuid` (59-70), `light.rs:87-456` (`Light`), `workload.rs` - about 2180 lines.
There is one back-edge: `simulator/light.rs:417` calls `crate::signed::rpc_reads::verified_attributes`, so
simulator and signed are mutually dependent.

**Recommendation (needs-care, no behaviour change if values are kept):** move the shared pieces to neutral homes so
the three profiles become leaves:
1. `src/wire.rs` (new): `hex`, `unhex`, `number`, `pack`, `unpack`, `hash`, `json_bytes`, the caps. One error enum
   (`Malformed | TooLong | UnsupportedVersion`) with `From` for `DemoError` and `simulator::Error`.
2. `src/dev_genesis.rs` (or under `genesis_presets/`): `genesis()`, `ALICE`, `BOB` from demo/workload.rs:11-50.
3. `src/error.rs`: `simulator::Error`/`Result` + the three `From` impls (simulator/mod.rs:12-60).
4. `src/http.rs` (new, shared by simulator and signed runtimes): see Duplication 2.
5. `src/query/` : `VerifiedRequest`, `resolve` (simulator/light.rs:21-86), `inspection.rs`, `profile.rs`,
   `query_language/`, the verified-page JSON helpers of Duplication 3, and `verified_attributes`.
6. `src/feed.rs`: `FeedBlock` + DTOs (simulator/wire.rs:69-340) and `metadata.rs`.
If the owner is willing to retire the volatile demo (the signed profile already has producer/full/light with real
signatures), that removes ~1525 src lines + the 167-line binary + 443 test lines + 2 scripts; that is a product
decision, not something the code forces.

### A [N3] arkiv-node binaries, genesis_*, query_language

All paths are relative to `crates/arkiv-node/src/` unless they start with `crates/` or `tests/`.
Every file of the chunk was read top to bottom (bin/arkiv-proposer.rs 1345, bin/arkiv-simulator.rs 346,
bin/arkiv-demo.rs 167, genesis_authoring.rs 1853, genesis_streaming.rs 1616, genesis_prebuilt.rs 820,
genesis_bundle.rs 769, genesis_bulk.rs 368, genesis_adopt.rs 130, genesis_import_resources.rs 61,
retention.rs 109, simulator/query_language.rs 358, query_language/{parse 1096, literal 678, lexer 627,
ast 130, error 98, limits 19, README.md, LICENSE}).

Two facts that constrain most fixes below:

- **Error text is an interface.** About 45 test assertions match genesis error text: 8 in
  genesis_authoring.rs (:1415-1837), 8 in genesis_streaming.rs (four of them exact,
  `assert_eq!(error, "..")` at :1039, 1057, 1098, 1108), 17 in genesis_prebuilt.rs (the `needle` tables at
  :679-709 and :753-759 plus :736, 788, 797), 3 in tests/genesis_prefill.rs (:228, 230, 865), and the CLI
  tests match stderr (tests/prebuilt_genesis_cli.rs:312 `"identity does not match"`, :338 `"StorageCorrupt"`,
  :445 `"InvalidRequest"`; tests/genesis_adopt.rs:363, 591, 636, 731). Any error-type change must keep the
  message strings byte for byte.
- **stderr JSON events are an interface.** `prefill_progress`, `prefill_bulk_phase`,
  `prefill_prebuilt_install`, `prefill_genesis_adopt` are parsed by `scripts/run-full-prefill.py:82-85`
  and asserted by tests/prebuilt_genesis_cli.rs:251-254 and tests/genesis_adopt.rs:552,580. They are
  not leftover debug output.

#### [N3] Genesis layering (answer to "what are the seven files")

| file | role | depends on |
|---|---|---|
| genesis_bundle.rs | (1) shared types of *both* paths: `PrefillEntity` row codec, `PrefillLimits`, `PrefillLaunch` (launch versions 1-4), `recipe_hash`, `prefill_rent_wei`, `rent_clock`; (2) the *small* path: `ARKPFL01` stream (<= 256 rows, 1 MiB) built by live execution (`construct`), verify, JSON `report` | genesis_streaming (`validate_limits`, :318) |
| genesis_authoring.rs | TOML/JSONL front end of the small path: `lock` / `build` / `verify` of a bundle directory | genesis_bundle |
| genesis_streaming.rs | file-backed `ARKPFL02` path: `StreamReader`, audit, `VerifiedStream`, `import_identity`, `prepare_*`, `build_bundle_with_state` (CLI `stream-build`), `summary_report` | genesis_bundle, genesis_bulk, genesis_prebuilt, genesis_import_resources |
| genesis_bulk.rs | the bulk builder behind the streaming path (`build_file`) | genesis_streaming (8 items), genesis_bundle |
| genesis_prebuilt.rs | hand-off of a finished bulk directory (`--state-out` / `--prebuilt-state`) | genesis_streaming |
| genesis_adopt.rs | rename the staged store over `node.redb` (`--genesis-install adopt`) | genesis_streaming (`check_disk`) |
| genesis_import_resources.rs | cache sizes and the free-disk check; private module, re-exported through genesis_streaming.rs:20 | - |

These are not dead generations: versions 1/3 (small) and 2/4 (streaming) are both live, and the small
builder is the oracle the bulk builder is tested against. What *is* generational:

1. the `X` / `X_with(.., permanent)` pairs left by the permanent-prefill change (Duplication 3);
2. remnants of an earlier checkpointed importer: `--batch-bytes`, `Progress::report(.., checkpoint, ..)`,
   the "old incremental CURRENT/checkpoints" comment and `"bulk-v1"` (Sloppiness 4, 13);
3. "v2" wording for code that now also serves v4 (Comments 2, Other 13).

Three module cycles: genesis_bundle <-> genesis_streaming (genesis_bundle.rs:318),
genesis_streaming <-> genesis_bulk (genesis_streaming.rs:372,414 / genesis_bulk.rs:6-9),
genesis_streaming <-> genesis_prebuilt (genesis_streaming.rs:468,534,578 / genesis_prebuilt.rs:22).
Proposed tree (needs-care, 38 references in arkiv-proposer.rs and 32 in tests/; keep `pub use` shims or
do it in one sweep): `genesis/{error, fs, layout, row, launch, small, authoring, stream, bulk, bundle,
prebuilt, adopt, resources}.rs`, where `row` = `PrefillEntity` + its one validator, `launch` =
`PrefillLimits`/`PrefillLaunch`/`recipe_hash`/both limit validators/report JSON, `stream` = reader + audit
+ `VerifiedStream`, `bulk` = `build_file` + `import_identity` + `Progress`, `bundle` =
`build_bundle_with_state`. That removes all three cycles.

### A [N4] arkiv-node deterministic core

Files read in full (8850 lines): `exec.rs` 1305, `block.rs` 1461, `gas.rs` 720, `params.rs` 379, `validate.rs` 364,
`identity.rs` 325, `accounts.rs` 316, `value_json.rs` 300, `replay.rs` 282, `select.rs` 156, `lib.rs` 60,
`arkiv/mod.rs` 875, `arkiv/schema.rs` 437, `arkiv/tests.rs` 1870.

Path convention: `src/` below means `crates/arkiv-node/src/`. Everything else is repo-relative.

Headline: the chunk has two house styles. `block.rs`, `exec.rs`, `params.rs`, `validate.rs`, `identity.rs`,
`replay.rs`, `select.rs`, `value_json.rs` are spaced, documented, use `encoder`/`decoder`/`formatter` and named tag
constants. `gas.rs`, `accounts.rs`, `arkiv/mod.rs`, `arkiv/schema.rs`, `arkiv/tests.rs` have no blank line between
items (15 / 16 / 25 / 16 / 24 item boundaries), one-letter bindings (`e d c p f r n h v t k b o w`), bare tag
literals and mostly undocumented pub items. The second group is where the "AI slop" reading comes from, and it is the
consensus-critical entity/rent/account code. No line in the chunk exceeds 150 chars.

### A [NT1] arkiv-node tests: core

Chunk: `crates/arkiv-node/tests/` - block_model.rs (2246), replay.rs (1399), gas_execution.rs (1226), block_format.rs (788), accounts.rs (670), exec.rs (483), genesis.rs (328), validation.rs (281), identity.rs (269), selection.rs (206), support/mod.rs (320), support/block_roots.rs (197). All twelve read top to bottom.

Path convention: `T/` = `crates/arkiv-node/tests/`. Every other path is repo-relative. Line numbers are as read on 2026-11-17.

Nothing here changes production code. `[CONSENSUS]` below marks test refactors that must leave block inputs byte-identical, because golden hashes / hand-derived receipts depend on them.

### A [NT2] arkiv-node tests: signed

Read in full, top to bottom: signed_http.rs (1611 lines, 14 tests), signed_host.rs (1398, 19), signed_reads.rs (776, 5),
signed_history.rs (739, 22), signed_protocol.rs (585, 8), signed_pool.rs (392, 9), signed_retention.rs (388, 4),
signed_support/mod.rs (76), ethereum_transactions.rs (485, 7), arkiv_profile.rs (494, 4).
Paths below are relative to `crates/arkiv-node/tests/` unless they start with `crates/`.
No cargo was run; clippy-uniq.txt was used as signal and every clone/long-fn claim below was re-checked in the source.
Note: arkiv_profile.rs is on the unsigned `simulator::` stack (`/sim/v1`), not the signed one; it shares only the HTTP plumbing.

### A [NT3] arkiv-node tests: genesis / simulator / demo / CLI; examples; Cargo.toml; clippy.toml

Read in full, top to bottom (all paths below are relative to `crates/arkiv-node/` unless they start with `crates/`, `docs/`, `scripts/`):
`tests/genesis_prefill.rs` (878), `tests/genesis_adopt.rs` (733), `tests/prebuilt_genesis_cli.rs` (449), `tests/simulator_http.rs` (790),
`tests/simulator.rs` (558), `tests/simulator_crash.rs` (147), `tests/demo_auth.rs` (443), `tests/retention_cli.rs` (122),
`examples/inspect_eq.rs` (59), `examples/gas_calibration.rs` (122), `examples/verify_inspection.rs` (185), `Cargo.toml` (37), `clippy.toml` (39).
Cross-checked (read the cited ranges only): `tests/support/mod.rs`, `tests/signed_support/mod.rs`, `tests/support/block_roots.rs:1-8`,
`src/genesis_bundle.rs:644-769`, `src/genesis_streaming.rs:605-895,1110-1207`, `src/genesis_prebuilt.rs:361-738`,
`src/bin/arkiv-proposer.rs:270-440`, `crates/arkiv-db/src/engine.rs` (method list + 838-892, 1950-1985, 2296-2335), `.github/workflows/ci.yml`.
cargo was not run.

### A [D1] Documentation

Read-only review. Every "stale" item below quotes the doc line and the code line that
contradicts it; items I could not verify are marked "unverified". Paths are repo-relative.
Categories follow the documentation task, not the five code categories.

(target layout and totals are at the end)

### A [D2] Scripts, CI, build config, vendor/reth-trie, conformance

Scope read in full: all 29 files under scripts/ (10,480 lines), .github/workflows/{ci,golem-conformance}.yml,
Dockerfile.simulator, deploy/, .gitignore, root + 6 crate Cargo.toml, vendor/README.md, vendor/reth-trie (243 lines),
conformance/golem-db (README, ci.sh, check-tests.py, harness.patch, expected-failures.txt, scenarios-scaled, src/arkiv.rs 1489,
src/golem_tests.rs 1539). Nothing was run (no cargo, no scripts, no docker); every claim below is from reading + rg.
Paths are repo-relative. Line numbers are from the files as they are on disk today.

Answers to the direct questions, up front:

- **scripts/__pycache__**: not tracked (`git ls-files scripts | grep pyc` = empty; `.gitignore:3` has `__pycache__/`;
  `git status --ignored scripts` shows `!! scripts/__pycache__/`). Disk-only junk, not a repo problem.
- **Secrets**: none in the tree. Node tokens are `secrets.token_hex(32)` per run; keys are `0x01..01`-style and labelled
  disposable. But see S1: bench-compare.sh hard-codes where a GitHub token lives on one machine.
- **Which scripts/test-*.py does CI run?** None. `ci.yml` has only cargo steps; `golem-conformance.yml` runs
  `conformance/golem-db/check-tests.py` (a result checker, not a script test). The five files (2,306 lines, 63 test
  methods + one process acceptance) are manual only (docs/testing.md:469 lists test-full-light-demo.py as a manual step).
- **conformance/golem-db has no Cargo.toml by design**: `ci.sh setup` (ci.sh:33-56) clones golem-db-conformance-testing
  next to the repo, `git apply harness.patch` adds path dependencies on `../../../arkiv-state-db-experiment/crates/*`
  (harness.patch:15-17) and `mod arkiv; #[cfg(test)] mod golem_tests;` (harness.patch:35-39), then copies `src/*.rs` into
  the harness's `crates/harness/src/`. It is built as part of the harness crate `golemdb-harness`. README.md:15-16 says so.
- **vendor/reth-trie** is real vendored code (two unmodified reth files + a 20-line re-export lib.rs), but nothing uses
  it: see S4.
- **Two languages for the SDK smokes**: the `.ts` files must be TypeScript (they exercise `@arkiv-network/sdk` under Bun);
  the `.py` files are the node launchers (they copy the `.ts` into a temp dir with a symlinked `node_modules` and run
  `bun smoke.ts <stage>`). Not thin: 274 / 307 lines, about 80% of which is the launcher boilerplate of D1.

## Appendix B: helper inventories for shared test-support modules

Further inventories are inside the Duplication sections: [T1] 1.1, [T2] 1.7, [T3] 1.3, [T4] 1.1, [T5] 1.2, [NT2] 1.1-1.3, [NT3] D1.


### B [S4] Helper inventory for a shared tests/support

tests/common/mod.rs (all `pub`):
`BenchDir` 30-60 (`new(name: &str) -> Self`, `path(&self) -> &Path`, `file(&self, name: &str) -> PathBuf`, removes dir on drop);
`base_dir() -> PathBuf` 63-71; `filesystem_of(path: &Path) -> String` 74-89; `keccak(bytes: &[u8]) -> ObjectId` 92-94;
`object(tag: u64, n: u64, len: usize) -> (ObjectId, Vec<u8>)` 97-113; `head(height: u64, fork: u8) -> Head` 116-120;
`block_bytes(height: u64, fork: u8, len: usize) -> Vec<u8>` 123-127;
`block(parent: Option<Head>, height: u64, fork: u8, objects: &[(SpaceId, u64, usize)]) -> CommitBatch` 132-173;
`commit(store: &dyn VersionedStore, batch: CommitBatch)` 176-179;
`assert_applied(store: &dyn VersionedStore, batch: &CommitBatch)` 183-200; `assert_absent(..)` same signature 204-221;
`crash_store(disk: &CrashDisk, config: RedbConfig) -> RedbStore` 224-227;
`timed<T>(work: impl FnOnce() -> T) -> (T, Duration)` 230-234; `quantile(samples: &mut [Duration], q: f64) -> Duration` 237-244;
`mb(bytes: u64) -> f64` 247-249.

tests/engine_support/mod.rs (all `pub`):
`OWNER: OwnerId` 14, `HOST_KEY: [u8; 32]` 15, `ns(n: u64) -> NamespaceId` 16-18, `name(s: &str) -> CellName` 19-21,
`key(n: u64) -> RecordKey` 22-24 (big-endian u64), `genesis() -> EngineGenesis` 25-27, `options(cache_bytes: usize) -> Options` 28-33,
`config() -> RedbConfig` 34-36, `disk_store(disk: &CrashDisk) -> Arc<RedbStore>` 38-41,
`initialize(store: Arc<dyn VersionedStore>, cache_bytes: usize) -> DurableEngine` 42-47,
`execute(branch: &mut Branch, op: WriteOp)` 48-53, `namespace(branch: &mut Branch, text: &str)` 54-61,
`row(branch: &mut Branch, namespace: NamespaceId, n: u64)` 62-74,
`put(branch: &mut Branch, namespace: NamespaceId, key: &[u8], value: &[u8])` 75-84 - differs from the
arkiv-db tests' `put(namespace, key, value) -> WriteOp` (e.g. crates/arkiv-db/tests/metering.rs:47): this one executes;
`publish(engine: &mut DurableEngine, branch: Branch)` 112-115,
`proof_request(snapshot: &Snapshot, namespace: NamespaceId) -> EqRequest` 127-144, `proof(..) -> Vec<u8>` 145-160.

Local to one test file: streaming_import.rs:10-15 `committed(i: Initialization) -> DurableEngine`;
reconciliation.rs:55 `config`, 59 `chain`, 74 `parent_image`; latching.rs:48 `uncached`; backends.rs:45
`open_file(path: &Path) -> File`; history.rs:28 `history_block(parent, height)`; relabel.rs:23 `publication`,
56 `is_old`, 69 `is_new`, 95 `objects_kept`; contract.rs:32 `config`, 36 `file_store`; engine_crashes.rs:13 `image`, 22 `open`.

### B [S5] Test-helper inventory for a shared `tests/support` (workspace item)

None of the workspace-known names (`ns`, `receipt`, `ledger`, `traced`, `put`, `fixture`, ...) or delegating
`impl VersionedStore` doubles occur in this crate. Locally defined generic helpers:

| Helper | Where (lines) | Signature / note |
|---|---|---|
| `build` | store.rs:45-54, proofs.rs:37-46 | `fn build<const N: usize>(entries: &BTreeMap<[u8; N], Vec<u8>>) -> Map<N>` - identical |
| `map` | interval_inspection.rs:52-62 | `fn map<const N: usize>(entries: impl IntoIterator<Item = ([u8; N], Vec<u8>)>) -> MerkleMap<[u8; N], Vec<u8>>` |
| `one` | bounded_stage.rs:47-52 | `fn one<V: TrieValue>(value: V) -> MerkleMap<[u8; 1], V>` (key `[0x12]`) |
| `type Map<const N>` | store.rs:43, proofs.rs:32 | identical; variable_keys.rs:32 and adversarial_bounds.rs:50 define a non-generic `Map` |
| `staged_ids` | store.rs:56-58 (inline copy variable_keys.rs:472-477) | `fn staged_ids(staged: &Staged) -> BTreeSet<B256>` |
| `check_staged` | store.rs:61-66 | `fn check_staged(staged: &Staged, root: B256)` |
| `put_all` | store.rs:872-879 | `fn put_all(store: &mut MemoryNodeStore, nodes: &[Vec<u8>]) -> B256` |
| `prove` | proofs.rs:59-61 | `fn prove<const N: usize>(map: &Map<N>, key: &[u8; N]) -> Vec<Vec<u8>>` |
| `random_entries` | proofs.rs:48-57 | `fn random_entries<const N: usize>(rng: &mut Rng, count: usize, alphabet: u8, max_value: usize) -> BTreeMap<[u8; N], Vec<u8>>`; same expression inline at store.rs:70-72, 409-411, 534-536, 607-609 |
| `enc` / `escaped` | variable_keys.rs:37-41, encoding.rs:58-62, src/order.rs:303-307 | `fn enc(raw: &[u8]) -> Vec<u8>` (`encode_bytes` into a new Vec) - same body, two names |
| `random_raw` | variable_keys.rs:43-50 `(rng)`, adversarial_bounds.rs:55-60 `(rng, max)` | different alphabets and length bias |
| `random_bound` / `bound` | variable_keys.rs:139-161, adversarial_bounds.rs:102-108 | both `-> Bound<Vec<u8>>` with the same 1/5, 2/5, 2/5 split |
| `check_interval` | variable_keys.rs:163-209, adversarial_bounds.rs:110-165 | `fn check_interval(map: &Map, model: &Model, range: &KeyRange)` - shared first half (1.7) |
| `proof` | interval_inspection.rs:64-72 | `fn proof<const N: usize>(map: &MerkleMap<[u8; N], Vec<u8>>, lower: &[u8; N], upper: &[u8; N]) -> Vec<Vec<u8>>` |
| `walk_interval` / `inspect_interval` shims | interval_model.rs:6-49, interval_inspection.rs:7-50, interval_security.rs:4-47 | identical (1.1) |
| `Raw` / `Loose` | sorted.rs:15-28, adversarial_bounds.rs:32-48, variable_keys.rs:517-528 | raw-bytes `TrieKey`; decode bound differs (1.4) |
| `Sink` | sorted.rs:30-48, bounded_stage.rs:77-103 | differ in error type and fail-once vs fail-from (1.6) |
| `Postings` / `Nested<V>` | store.rs:643-667, bounded_stage.rs:216-234 | nested-map `TrieValue` (1.6) |
| `unpack` | adversarial_bounds.rs:68-73 | duplicate of `support::reference::unpack` |
| `Rng` | src/node.rs:515-550 | duplicate of `support::Rng` plus `nibbles`/`some_nibbles` |

## Appendix C: already fine, per area


### C [T1] arkiv-db tests: metering family

- **trace_regression.rs** - all 7 tests are distinct regression vectors; the `Point` tables with running `spent` are self-checking (289-298) and the success traces are written as sums of named parts then pinned to a literal. Only the step labels, the header and one two-scenario test need attention.
- **query_planner.rs** - the seven tests each establish something different; the random-query generator and model comparison are sound. Issues are local (debug print, re-implemented `walk`/`spent`/`tag_of`, slack constants).
- **cell_limits.rs** - small, table-driven where it should be, exact `ArgumentFault`/`StorageErrorKind` assertions throughout. Two redundant clones and one duplicated constant tuple.
- **metering.rs sections 6-9** (overflow 1050-1293, estimation script 1299-1454, determinism 1759-1783, pricing 1840-1990): dense but each assertion pins a distinct receipt; no change proposed beyond the shared `create_namespace()` builder.
- **qm vector helpers** (`assert_write_vector`/`assert_read_vector`, 808-964): checking the hand-written total against the group table before sweeping (819-823, 913-917) is a good design; worth making it the single harness (1.2 item 2).
- Derivation comments on the vectors in metering.rs and qm, and the trie-shape doc at trace_regression.rs:80-159.
- No `#[allow(..)]`, no dead helpers, no `impl VersionedStore` doubles and (bar bounds.rs:203) no rustfmt-defeating lines in the seven files.

### C [T2] arkiv-db tests: history / isolation

- The hand-derivation comments beside pinned numbers (isolation.rs:85-129, 680-710, 742-781; query_isolation.rs:210-246 apart from the model-2 phrasing, 1535-2476) explain a non-obvious why and should stay.
- history.rs module doc (1-7) is short and accurate; h's item-level comments are almost all "why".
- isolation.rs `expected_baseline` (379-568) is long but is a plain 14-row table; the `Observed`/`Step` design reads well.
- The oracle design (log replay, `Pending` with marks, coverage counters asserted per seed) is sound; `seed_tests!` is a reasonable way to get one named test per seed.
- No test here is trivial (no getter/derive/constant tests); all pin receipts, effects, ids, sharing or node counts.

### C [T3] arkiv-db tests: queries / records / pagination / terms

- No `println!`/`eprintln!`/`dbg!`/`todo!`/`#[ignore]` in any of the four files; no line over 110 columns (`awk 'length > 110'` is empty), no place where rustfmt gave up.
- One justified `#[allow]`: `terms.rs:826` (`result_large_err`, mirrors `expire_records`), with the reason on the line above.
- `queries.rs:788-812` (xorshift `Generator`), `combined_queries`, `all_bound_pairs`: deterministic generation, compact, no dependency; `hand_picked_operator_cases_hold` (1129-1228) is a good independent cross-check of the generated oracle with literal ids.
- `pagination.rs:803-1002` (`Head`, the seven disturbances) and `pinned_walks_match_the_reference_whether_or_not_the_head_advances`: the strongest test in the chunk; the `disturbed == quiet` comparison is the right assertion.
- `records.rs:1579-1775` fault-precedence table and `1791-1844` ordering against block budget/owner/namespace: unique coverage, labelled cases.
- `terms.rs` expiry tests (2033-2303): unique coverage of `expire_records` halts; literals with derivations.
- Test names are full sentences and accurate (two exceptions: 3.2 `.._in_the_briefs_order`; `cells_carry_a_kind_and_a_value`).
- Derivation comments next to cost literals ("1 + 5 + 4 + PostingEntry 2 + ResultRow 6 = 18") are the kind of comment to keep.

### C [T4] arkiv-db tests: codec / namespaces / blocks / rollback / roots / proofs

- **codec.rs** - expected bytes are literals or concatenations of literals, never produced by the codec; one generic harness (`golden`/`rejects`/`accepted`) instead of per-type tests; exhaustive 1- and 2-byte canonicity counts derived from the tag tables; seeded mutation fuzz. Apart from the two small tests in 2.1/2.2 and the banners this file needs nothing.
- **roots.rs** - a `BTreeMap` model, leaves built by hand, an independent yellow-paper trie cross-checked against `alloy_trie::HashBuilder`; engine and model driven by one `Op` enum; reuses arkiv-db-trie's test support through `#[path]` instead of copying it. Three tests, each with a distinct claim (oracle, history independence, sensitivity).
- **rollback.rs** - `assert_rejection_is_atomic` + `observe`/`expected_view` (whole-state comparison over probed ordinals) is the right shape; the per-test cost tables in comments (435-444, 853-861, 1051-1063) are useful "why" and should stay.
- **namespaces.rs** - `SHARED_KEYS` table with per-row cost derivations; every receipt exact; the ten tests do not overlap.
- **ordered_ranges.rs** - the `Fixture` with built-in distractors and `check_all` (paged query at three page sizes, `count`, proof, all against a model that orders by raw bytes rather than `cmp_ordered`) is compact and independent of the engine's encoding.
- **golem_types.rs** - `term_bytes_are_golem_stored_forms` and `ordered_types_sort_by_term_bytes_and_cmp_ordered` are table-driven pins; nothing to merge.
- **host_state.rs** - the two-engine isolation test compares against a run without host state rather than asserting costs, and says why; the short module-level helpers are specific to this file.
- **blocks.rs** tests 2-8 (`empty_blocks_…`, `a_block_touching_four_namespaces_…`, `a_stale_candidate_…`, `block_numbers_and_snapshot_ids_…`, `candidates_sealed_by_another_engine_…`, `every_one_of_fifty_…`, `branch_reads_…`) each make one claim with literal receipts.
- No commented-out code, no `#[ignore]` other than the one experiment, no `#[allow]`, no `unwrap` hiding a real failure mode (all are test fixtures).

### C [T5] arkiv-db tests: durable / faults / KV; examples

- tests/interrupt.rs - two focused tests, short accurate header, no duplication beyond `ALICE`.
- tests/durable_retention.rs - clear, each test pins one retention rule; only the shared helpers (1.3 item 1) and one phrase (3.3).
- tests/durable.rs test bodies - the hand-built vector test is exactly what a format pin should look like; only the helper block and the trailing `use`.
- tests/storage_faults.rs and tests/rehash.rs test logic - systematic (every k-th read, every operation; hand-derived node counts with the derivation beside each literal). The findings are about the doubles, `calls()` and the stale header, not the tests.
- tests/adopt.rs, tests/streaming_import.rs test logic - real interruption / relabel-outcome coverage; findings are the doubles and style.
- tests/durable_failures.rs test logic - 29 tests, none trivial; the concurrency tests (1387-1548) use rendezvous and condvars rather than sleeps, apart from one deliberate 200 ms negative check at 1474.
- `println!` in tests/rehash.rs:304 (`report`) is intentional: the header and README.md:267 document `--nocapture` output that docs/state-trie.md quotes.
- No `dbg!`/`eprintln!`, no `#[allow]`, no `#[ignore]` in any file of the chunk.

### C [S1] arkiv-db src: engine, typed, planner, record, layout, codec, meter

- **record.rs** production code (1-987): tight; newtypes with validated constructors, the `float_scalar!` macro is the right size, encodings documented where they are defined. Only the table repetition (1.8) and comments (3.1, 3.2).
- **layout.rs** production code (1-632): the module-doc table is the on-disk format spec and earns its length; `encodes_empty` overrides are deliberate and each says why. Tests are substantive.
- **meter.rs**: the charge table on `OperationClass` (67-93) and the `Meter` are clear; `charge_group` atomicity is easy to audit. No dead code beyond the three getters.
- **codec.rs** `Encoder`/`Decoder` (255-456), `count_of`/`reservable` allocation bounds, `digest` domain check: careful and well explained.
- **typed.rs** write path (`create`/`patch`/`delete`, 459-656): charge-before-work is consistently visible; the "N logical entry writes" comments justify each `RootUpdate` count and should stay.
- No `println!`/`eprintln!`/`dbg!`, no `todo!`, no `unsafe`, no swallowed errors in any of the seven files. Every `expect` in non-test code that I traced is guarded by admission or by construction (e.g. `term_bytes().expect(..)` after `validate_query`/`validate_terms`; `checked_add(..).expect(..)` in `execute_as` bounded by the meter limit).
- No section banners in engine.rs, record.rs, meter.rs.

### C [S2] arkiv-db src: durable, proofs, small modules

- **durable/cache.rs** (non-test part, 1-525): careful, well-explained, no duplication. The module doc states the lock order and the eviction policy; every comment explains a why.
- **durable/retention.rs**: clean apart from the lock idiom (D11) and one dated phrase (C7). The long module doc is the specification and earns its length.
- **durable/reachability.rs**: clean apart from D10 and one clone.
- **interrupt.rs**: clean. One test, and it covers nesting, unwinding and thread isolation.
- **nodes.rs**: clean. The module doc and the `LoadError` / `Fault` split are clear.
- **namespace.rs**: clean apart from one weak test and three restating docs.
- **state.rs**: clean apart from C2 and S6.
- **query.rs**: the types and the module doc are good; only the restating field docs (C3) need to go.
- **durable/format.rs**: small and clear apart from the literals (S10, W3) and the `for b in PROFILE { e.u8(*b) }` /
  `for b in self.deployment { e.u8(b) }` / byte-wise decode loops at 124-126, 137-139 and 172-175, which could be
  `e.fixed(PROFILE)`, `e.fixed(&self.deployment)` and `deployment: d.array()?` (`Encoder::fixed` appends raw
  bytes, codec.rs:293-295; **safe**, identical bytes, **[CONSENSUS on-disk]**).
- `unwrap`/`expect` in library paths: I checked each one in the 20 files (mod.rs:908, 980; bulk.rs:266, 591-599;
  adopt.rs:358; streaming.rs:47; host.rs:219, 293; namespace.rs:45). All are guarded by a check a few lines
  above and cannot fire on valid or invalid input.
- No section banners, no debug output, no over-long lines in any of the 20 files.

### C [S3] arkiv-db-storage, arkiv-db-memory, arkiv-db-redb src

- `crates/arkiv-db-redb/src/error.rs`: the mapping table in the module doc matches the `From` impls arm for arm; the `Fault` newtype that lets redb errors use `?` is a tidy pattern. Nothing to change beyond D9.
- `crates/arkiv-db-redb/src/counting.rs`: forwards every lock method and says why (`:86-91`, `:177`, `:206-207`).
- `crates/arkiv-db-redb/src/metrics.rs`: small, one focused test; only the literals in S4.
- `crates/arkiv-db-redb/src/testing.rs`: the durability model is stated once, precisely, in the module doc, and the code follows it; its 7 unit tests are worth their lines.
- `crates/arkiv-db-storage/src/batch.rs`, `testing/mod.rs`: clean.
- `crates/arkiv-db-memory/src/persistent.rs`: well written and well tested; its only problem is that nobody uses it (S1).
- Shared error constructors (`head_mismatch`, `object_conflict`, `record_conflict`, `relabel_mismatch`, `versioned.rs:464-504`) are already used by both backends, so the two return the same errors for the same refusal.
- Across the chunk: no `#[allow]`, no `println!` / `dbg!`, no section banners, no `unwrap` / `expect` that can fire in library paths (the four `expect`s in `persistent.rs:82-128` state AVL invariants), no line over 110 columns outside the Markdown table at `crates/arkiv-db-redb/src/lib.rs:14`.
- `create_file` / `recover` / `transact` in `store.rs`: the inline comments explain ordering and crash-safety decisions that are not recoverable from the code. Keep them.
- Contract case docs (`testing/contract.rs`): one or two lines each stating the rule under test; the expected values are written out, not derived from another store.

### C [S4] arkiv-db-redb tests and examples

- tests/latching.rs, tests/relabel.rs, tests/durability.rs, tests/initialization.rs, tests/recovery.rs,
  tests/isolation.rs (apart from the one stale sentence), tests/history.rs, tests/bounded_reads.rs,
  tests/disk_full.rs: clear names, distinct tests, assertion messages carry context, no trivial tests.
- tests/common/crash.rs `check_image` / `run_crash_points` / `Start`: well factored; the comments explain
  why (allocator state, steady state vs reopened).
- tests/common/creation.rs, lost_sync.rs, disk_full.rs: short, documented, one purpose each.
- tests/backends.rs `NoLocks` control and `every_lock_method_is_forwarded` (apart from line 106).
- No `dbg!`, no leftover debug output beyond the intended tallies; no section banners; the only lines over
  150 characters are example string literals (4.12).
- examples/bench_readers.rs being self-contained is correct given scripts/bench-compare.sh.
- The long module docs in the test files state model, claims and scope; they read as written by someone
  who ran the experiment.

### C [S5] arkiv-db-trie

- **`src/nibbles.rs` vs alloy `Nibbles`**: not a duplicate to remove. The vendored alloy-trie 0.9.4 resolves
  `nybbles` 0.4.7 (`Cargo.lock:1981`), whose `Nibbles` is capped at 64 nibbles, and alloy's `encode_path_leaf`
  takes that type; engine keys exceed 32 bytes. The module doc says exactly this, and
  `node.rs::encoding_matches_alloy_nodes` pins byte equality where alloy can represent the node.
- **Three ways of building the trie** share the encoders: `Node::encode` and `SortedTrieBuilder` both go through
  `encode_leaf` / `encode_extension` / `encode_branch` (`node.rs:286-328`). The only real duplication is the
  staging walk (1.9) and the seal loop (1.14).
- **Loader-side walkers** (`get`, `insert_at`, `remove_at`, `ProofIter::step`, `Range::seek/step`,
  `IntervalProofIter::step`) already share `resolve`, `Role`, `check_leaf_end`, `check_inner_end`; merging them
  further would cost clarity.
- **Error enums**: every variant of `TrieError` (9), `ProofError` (7), `StageError` (3) and `SortedTrieError` (7) is
  constructed in `src/` (rg per variant); none is dead. `src/error.rs` is clean.
- `src/codec.rs`, `src/loader.rs`, `src/range.rs`, `src/reach.rs`, `src/stage.rs` (apart from 1.9/4.6),
  `src/node.rs` production half: clean. Long doc comments on `TrieKey`, `TrieValue::encodes_empty`, `Probe`,
  `walk`, `KeyRange::may_hold` explain non-obvious contracts and should stay.
- ~25 one-line docs restate a name ("Creates an empty store.", "What was wrong." x3); conventional for pub API, leave.
- `tests/proofs.rs`, `tests/sorted.rs`, `tests/interval_inspection.rs` (after 1.1), `tests/bounded_stage.rs`
  assertions, `tests/support/mod.rs` oracle: well-targeted; golden vectors and tamper tests all kept.
- No `println!`/`dbg!`, no section banners, no lines over 120 chars, no `unwrap`/`expect` in library paths that
  can fire on input: the three `expect`s (`node.rs:149`, `sorted.rs:174, 186`) guard internal invariants and the
  `assert!`s (`map.rs:139`, `node.rs:265`, `order.rs:284`, `loader.rs:95`) are documented codec-contract panics.

### C [N1] arkiv-node signed JSON-RPC / HTTP reads

- `reads.rs` code (`ReadLimits`, `Reads`, `Admitted::run`, `RaiseOnDrop`): small, typed, well-bounded; only comment and constant nits above.
- `state_docs.rs` production code (17 lines): nothing to change. Clippy `:36` ("looks like a formatting argument") is a false positive on a route literal.
- `ethereum.rs` ABI decoding (`operations`, `triples`, `abi_config`, the canonical re-encode checks at `:224, 235`): careful and well-commented; the `decode!` macro is justified.
- `range_api.rs` `resolve` (`:43-86`): clear. Only the unformatted closure (S2). `PROFILE` (`:17`) is used by `runtime.rs:426`, so it is not dead.
- `rpc.rs` `ExpiryReplay` (`:288-383`): the doc comment explains a non-obvious replay; keep as is apart from `&mut self` on `expiry`.
- `rpc_reads.rs` evaluation core (`read_page`, `sorted_page`, `Ranked`, `Order::cmp`, budget charging): dense but coherent, comments explain why; `Window` is a sensible parameter object. The Boolean bounding (`bounded`/`union`/`intersect`/`complement`) is correct to bound before expanding.
- No section-banner comments, no `#[allow(..)]`, no `dbg!`/`println!` in any of the nine files.
- `rpc.rs:1250`: not a bug (see S6).

### C [N2] arkiv-node runtimes: signed / simulator / demo

- signed/crypto.rs (67): tight, comments explain the non-obvious (high-S, x-reduced recovery id). Nothing to change.
- simulator/profile.rs (54), signed/mod.rs, demo/mod.rs: fine apart from the `DemoError`-vs-`Error` point.
- demo/auth.rs (171): clear and small; only the duplicated `version()` helper.
- signed/pool.rs (476): well documented, sensible structure; items above are local.
- signed/history.rs (471): careful recovery logic; only small repeats, bare entry tags and one misplaced doc.
- signed/light.rs:20-395 (`Light`): readable; `validate_append`/`commit_catalog` are a good split.
- signed/protocol.rs encode/decode impls (250-287, 398-441, 484-508, 550-570, 618-644): consistent, bounded, no changes
  wanted **[CONSENSUS]**.
- signed/runtime.rs:77-307 (lock discipline, `Count`, `with_node`, `writing`): the best-documented code in the chunk.
- simulator/wire.rs `json_bytes` (43-67) and `metadata_bytes` (264-334): fine **[CONSENSUS]**.
- Comments overall: few, mostly a real why; no banners, no milestone labels in these files.

### C [N3] arkiv-node binaries, genesis_*, query_language

- retention.rs: small, documented, one job (see Other 6 for the only wart, which is imposed from outside).
- genesis_adopt.rs: clear crash-safety contract in the module doc, three small functions; only shares
  `sync_dir`/`parent` (Duplication 1) and one over-visible constant.
- genesis_import_resources.rs: fine apart from being re-exported through another module.
- genesis_prebuilt.rs non-test code: careful (symlink refusal, rename probe, restartable copy); only the shared
  helpers and layout constants move.
- genesis_authoring.rs path handling (`relative`, `open_directory`, `open_relative`, `Inputs::read`, :616-726),
  `timestamp` (:453-510), `StrictJson` (:558-614) and its 17 tests: deliberate and well tested.
- genesis_bundle.rs `PrefillLaunch` doc table (:274-292) and the three pin tests.
- genesis_streaming.rs `StreamReader::next_row` (:147-195) and `VerifiedStream` (:264-307): tight and well covered.
- bin/arkiv-proposer.rs `ProductionCadence` and its three tests (:1284-1345), `allocations` duplicate-key
  visitor (:139-185), `write_new`'s no-clobber design.
- simulator/query_language.rs tests (6, table-driven) and the README's description of the adaptation.
- query_language/limits.rs, error.rs, ast.rs: small and clear (modulo upstream-only doc references).

### C [N4] arkiv-node deterministic core

- `src/select.rs`: clean; the module doc states the rules, the code is the rules. Only D12.
- `src/params.rs`: the reservation math, its docs (formulas per function) and `ChainSpec` are good. Only D13 and
  five trivial accessor docs.
- `src/identity.rs`: the state table in the module doc and `RequestIndex` are clear and minimal.
- `src/replay.rs`: structure (`prepare`/`finalize`/`apply`) and error types are fine; only the two clones and labels.
- `src/exec.rs`: the module doc (order within a block, expiry schedule layout, failure rules) is the kind of comment
  to keep; inline comments explain why, not what. `spill_expiry` and `resolve` are small and well specified.
- `src/block.rs`: module doc and hash table, tag constants, strict decoders; the size is mostly the format itself.
- `src/gas.rs`: `rent` / `RentClock` docs and the unit tests (exhaustive walk + seeded random walk against an
  independent per-layer model) are the best tests in the chunk.
- `src/value_json.rs`: module doc is the wire contract; two table-driven tests cover every type.
- No `println!`/`eprintln!`/`dbg!`, no `TODO`/`FIXME`, no line over 150 chars in any of the 14 files.

### C [NT1] arkiv-node tests: core

- `T/validation.rs` - 7 tests, each a loop over a case table with the op in the failure message; no trivial test, nothing to merge. Only the one redundant clone (:46).
- `T/selection.rs` - 5 tests; byte sizes derived in comments and checked against the encoder. One redundant clone (:49), one self-check line (:130).
- `T/identity.rs` - apart from the one subset test, the state-machine tests are tight and readable.
- `T/genesis.rs` - hand-derived cost comments are the useful kind; the invariant table is right, just long.
- `T/exec.rs` - clear module doc that matches its tests; failure paths well chosen.
- `T/block_format.rs` - the best file of the chunk: byte builders independent of the encoder, `golden` checks encode / decode / trailing byte / every truncation, independent keccak for digests. Only the stale first line and the history comments.
- `T/replay.rs` - `Fixture` is a good abstraction; the derivation comments beside every number are exactly what a reviewer needs; goldens are correctly described as determinism pins.
- `T/support/mod.rs` - small, documented, no logic; `PARAMS` doc derives the 888 reservation. Two dead helpers and one stale sentence.
- `T/block_model.rs` - the oracle's charge-group design (`verdict`) and the coverage assertions are sound engineering; the problems are size, the mid-file `#[path]` module and the listed leftovers, not the idea.
- No `dbg!`/`eprintln!` in the chunk; one `println!`. No lines over 120 characters.

### C [NT2] arkiv-node tests: signed

- signed_support/mod.rs: 76 lines, clear, every item used (rg: `balance`, `allocations`, `actors` in gas_execution.rs; `genesis_from` in signed_host.rs:1176, :1295). Only the positional `genesis` signature (1.3) and the missing pieces listed there.
- signed_history.rs: the tests themselves. Each pins a storage outcome, a crash/recovery rule or a trust boundary, fault families already iterate over `CommitFault`/`ReadScope` lists, comments explain the why. Only the fixture threading (1.4 item 5) and one clone.
- signed_pool.rs: tight; one merge, one stray comment.
- signed_retention.rs: tests are precise response pins; only request boilerplate, one 202-char line and one weak assertion.
- signed_protocol.rs golden section (:136-243) and the signature edge cases (:292-354): explicit, independent preimages; leave as is.
- signed_http.rs: the lock hand-over tests (:559-715) with `hold_node`/`until`/`reads_waiting` are condition-driven rather than sleep-driven; good pattern, and the one to copy into signed_reads.rs where possible.
- No section banners, no milestone/step labels, no `dbg!`, no fixed ports anywhere in the chunk.

### C [NT3] arkiv-node tests: genesis / simulator / demo / CLI; examples; Cargo.toml; clippy.toml

- `examples/inspect_eq.rs` - clean, small, documented invocation matches the docs (only the shared fixture literal, D4).
- CLI tests locate binaries through `env!("CARGO_BIN_EXE_*")`; no `cargo run`, no target-dir guessing.
- No fixed listening ports; no fixed-duration sleeps; every child process is killed on drop.
- No `#[allow]`, `#[ignore]`, `todo!`, `dbg!` or commented-out code in any of the eleven files; zero section banners.
- Comment quality is good overall: almost every comment states a why (e.g. `genesis_prefill.rs:25-27,182-183,302-303,505`, `prebuilt_genesis_cli.rs:344-346`, `demo_auth.rs:63,170-171`, `simulator.rs:112-113`).
- Golden/pinning tests are well formed: `demo_auth.rs:59` builds the layout by hand independently of the encoder; `genesis_prefill.rs:271` pins three hashes; `simulator.rs:362` pins the TypeScript golden.
- `tests/simulator_crash.rs` and `tests/retention_cli.rs` are small and focused (apart from S8/S9/W1).
- `clippy.toml`: all 31 entries are current; the header comment explains the policy well (minus its last sentence).
- `Cargo.toml`: no unused dependency; features are minimal (`default-features = false` on k256 and reqwest).

### C [D1] Documentation

- CHANGELOG.md entries (content): precise, consensus impact and upgrade rules stated, measurements attributed. Only structure issues (O13, O14, release cut).
- docs/entity-expiration.md, docs/proposals/expiry-spill.md, docs/sdk-1.0-compatibility-todo.md, docs/benchmarks/2026-11-17-sdk-side-by-side.md: recent, concrete, read as hand-written; only the `#expiry` anchor is broken.
- docs/genesis-bundle-v1.md, docs/glm-gas-v1.md, docs/glm-accounts-v1.md, docs/range-proofs.md, docs/eq-proof-charges.md, docs/pruning-design.md, docs/node-api-reference.md, docs/sourcify-v4-prefill.md, docs/genesis-adoption.md, docs/simulator-proof-inspection.md, docs/simulator-storage-profile.md: checked statements match the code (constants, flags, presets, defaults cited above); keep with at most a rename.
- docs/block-format.md and docs/state-trie.md: normative and consistent with the code where checked (type table, 25 value types, key layouts, 31 price classes, versions in Part 2); one stale line (O1) and the step labels.
- docs/engine-storage-run.md: every default checked matches `durable/mod.rs`, `cache.rs`, `format.rs`.
- docs/signed-proposer-v1.md: pool, size and concurrency limits all match `signed/pool.rs:16-20`, `signed/protocol.rs:27-44`, `signed/reads.rs:90-95`; CLI examples match the help text.
- docs/simulator-node.md: flags, roles and env vars match `arkiv-simulator.rs`.
- docs/full-light-demo-run.md: commands and options match the scripts and `arkiv-demo.rs`; one stale number.
- docs/benchmarking.md and docs/benchmarks/: correct place for dated, generated results (paths aside).
- docs/examples/genesis-bundle/README.md: commands match the CLI (`lock | build --locked | verify`), preset `@5` current.
- deploy/README.md:1-32, vendor/README.md:8-52 (provenance, revisions, licences), query_language/README.md (provenance, limits verified: 8192 bytes, 64 predicates, depth 32, 32-byte names, 128-byte strings = `limits.rs:9-19`, `literal.rs:33`).
- Link hygiene: 0 broken relative file links in 67 markdown files; all doc-named env vars and CLI flags exist in code or scripts.

### C [D2] Scripts, CI, build config, vendor/reth-trie, conformance

- **conformance/golem-db/src/arkiv.rs** (1,489 lines): reads as careful hand-written Rust. Comments explain golem-db's
  rules and why the adapter differs; error mapping keeps sources; no dead code found; `expect`s are on invariants stated
  in the message. Only the micro-repeats of D11 and the nits in S9.
- **conformance/golem-db/src/golem_tests.rs**: a faithful port with soft assertions; the header (1-24) states the porting
  rules; divergences are marked `// golem-internal`. Nothing to delete.
- **conformance/golem-db/check-tests.py, expected-failures.txt, harness.patch, scenarios-scaled/*.toml, ci.sh** (except
  ci.sh:70): small, purposeful, documented.
- **scripts/sourcify-v4-prefill.py** and **scripts/test-sourcify-v4-prefill.py**: the best pair in scripts/. Every check
  names the upstream rule it mirrors, the tests are table-driven (`subTest`) with an independent writer and reader, and
  they pin the codec bytes. Only the 10-tuple (S12) and two 5-line repeats (D7).
- **scripts/full-light-demo.py**: well structured (`Cluster`); the model for D1.
- **scripts/bench/parse_run.py, scripts/bench/summarize.py**: clear and idempotent (two nits in D8 and S9).
- **scripts/udc-sdk-smoke.ts, scripts/sdk-side-by-side-bench.ts**: readable; each step says what it proves.
- **scripts/bench-compare.sh** apart from S1: defensive in the right places (lock file, resume, load gate, free-disk
  check, stamps to skip rebuilds).
- **.github/workflows/ci.yml**: pinned toolchain, `--locked`, concurrency group, one job; matches the docs.
- **Root Cargo.toml**: workspace dependencies, the redb pin and the dev-profile overrides each carry a reason.
  **crates/arkiv-db*, arkiv-db-trie Cargo.toml**: every dependency is used (checked with `rg -l '\b<crate>\b' crates/<name>/{src,tests,examples}`).
  **crates/arkiv-node/clippy.toml**: a good use of `disallowed-methods`.
- **.gitignore**; no tracked build output, bytecode, secrets or tokens.

## Appendix D: totals, per area


### D [T1] arkiv-db tests: metering family

Tests (103 in the chunk): **2 delete**; **5 merge** into existing tests (2 within metering.rs, 3 from scalar_profile.rs into cell_limits.rs / golem_types.rs / durable.rs + eq_proofs.rs); **9 table-drive -> 2** (6 -> 1 in metering.rs, 3 -> 1 in qm). 103 -> 89, every metering vector preserved. Plus ~12 over-long tests to split (adds ~12 small ones).

Approximate lines removable:

| category | lines | main items |
|---|---|---|
| 1. Duplication | ~650 | vector-literal defaults 140, raw-op sweepers 125, evaluator copies 120, identical receipt helpers 75, `create_namespace()` 55, ids helper 36, generators 30, verify closure 25, walk/spent/tag_of 33 |
| 2. Tests | ~525 | seek-only table 115, exact-budget delete 57, binding-lookup table 55, estimate merge 48, write-vector table 45, admission merge 40, tracing merge 30, tautological asserts 34, profile-vector merge 27, order-vector merge 23, the rest 51 |
| 3. Comments | ~180 | banners 124, headers 55 |
| 4. Sloppiness | ~15 | debug print, double scan |
| **total** | **~1,370 of 9,734** | |

Not counted: the optional thinning of qm's literal ledgers (~100 more), and the workspace-level sharing of `name`/`id`/`ns_id`/`MODEL_VERSION`.

### D [T2] arkiv-db tests: history / isolation

- Tests: 0 delete; 1 split -> 3 (qi:3109); 1 internal merge (i:1196, ~35 lines); `host_accepts` fragment removed (14 lines); ~27 lines of strictly redundant asserts, ~25 more with the i/qi alignment; optional merge of 4 -> 1 phase tests in each isolation file.
- Duplication, approx. lines removable: safe ~1050 (verbatim oracle helpers ~150, isolation helpers ~140, `Reader` ~260, price tables ~75, evaluator/`encoded_len` ~105, in-file repetition ~320); needs-care ~350 more (shared phase functions for the history pair ~250, `World`/phase tests for the isolation pair ~100).
- Comments: 133 banner lines + 8 inline sub-banners; 5 stale items; 23 lines (plus 3 test names and 1 fn name) naming documents absent from the repo; 7 milestone labels; ~20 lines of old-model-delta derivations; ~60 lines of module-doc template.
- Sloppiness: 8 `too_many_arguments` allows (1 stale, 7 removable by threading `&mut Run`); 6 functions over 150 lines with named splits; 18 lines of key re-derivation; 3 label-string dispatches.

### D [T3] arkiv-db tests: queries / records / pagination / terms

Tests (78 in chunk): **10 DELETE** (`records.rs` 877, 936, 1089, 1316, 1391, 1422, 1847 [each pinned identically in `qm`], 669 [duplicate of a unit test], 822 [trivial]; `queries.rs` 1939 [subset of `pagination.rs`]); **19 MERGE -> 1** (`records.rs` 1005, 1153, 1203, 1252, 1458, 2064, 2136 fold into existing `qm` tests; `queries.rs` 2213, 2343, 2400, 2436, 2526, 2565, 2636 fold into `qm` section 5; `pagination.rs` 1387 folds into 1439; `terms.rs` 1355 + 1433 + 1506 + 1576 become one table test); **11 TIGHTEN** in place (`records.rs` 719, 750, 2432; `queries.rs` 1231, 1614; `pagination.rs` 1136, 1648; `terms.rs` 876, 963, 1633, 2143). 78 -> 50 tests in this chunk if everything is taken. No pinned literal is dropped without a named surviving pin.

Approximate lines removable in this chunk (10 131 now):

| category | lines |
|---|---|
| 1. Duplication (shared model ~290, shared helpers ~310, view traits ~100, walk ~50, `type_of`/`in_range` ~50) | ~800 |
| 2. Tests (2.1 ~920, 2.2 ~617, 2.3 ~270, 2.4 153, 2.5 ~95, 2.6 ~70, 2.7 ~50, 2.8-2.10 ~115, 2.11-2.16 ~80) | ~2 300 |
| 3. Comments (banners ~110, module docs ~120, misc ~15) | ~245 |
| 4. Sloppiness (4.1 ~150, 4.2 ~120, 4.3 ~110 if 2.2 is not taken, rest ~50) | ~320-430 |
| total (overlaps removed) | ~3 400-3 600, i.e. about a third of the chunk |

Cross-area notes: (a) 2.1/2.2 need the `query_metering.rs` reviewer's agreement on direction; (b) 2.11 deletes four unit tests in `src/record.rs:993-1083`; (c) 1.1/1.3/1.4 also apply to `query_isolation.rs`, `query_metering.rs`, `query_history.rs`, `metering.rs`, `isolation.rs`, `history.rs`, `blocks.rs`; (d) `docs/testing.md:170` uses the "33-byte form" wording of 3.1 item 1 for the key encoding; `rehash.rs:507` (not my file, not read) speaks of a padded "v2 encoding" and may want the same check.

### D [T4] arkiv-db tests: codec / namespaces / blocks / rollback / roots / proofs

- Tests (118 in the chunk): **4 delete** (`values_round_trip_through_shared_storage`, `every_indexable_type_is_ordered`, `a_fresh_state_has_no_host_entries`, and `range_scaling_experiment` moved to examples/ or deleted); **8 merge -> 3** (four `*_domain_panics` -> 1; `keccak_is_the_trusted_primitive` + `digests_hash_…` -> 1; the two per-type range-proof tests -> 1); **1 split -> 2-3** (`six_step_flow_…`, optionally dropping its two restated blocks). No golden vector, receipt, state root or proof-byte pin is removed.
- Approximate lines removable (of 10,910):
  - 1 Duplication: ~850 (helper set ~170 net in three files, read trait ~145, proof doubles/harness ~165, fixtures ~180, `CreateNamespace` literal ~40, range helpers ~60, sample tables ~60, cost-table docs ~35; cross-crate ~90 counted separately)
  - 2 Tests: ~215 (deletes 71, merges ~80, redundant assertions ~65); the six-step split is roughly line-neutral unless the two restated blocks (blocks.rs:533-556 and 558-616, 83 lines) are dropped
  - 3 Comments: ~200 (banners 125, module docs ~60, the rest ~15)
  - 4 Sloppiness: ~45 (the function splits are line-neutral)
  - 5 Other: line-neutral in this chunk; 5.1 is what makes the ~145 lines of 1.2 (and seven more traits elsewhere) disappear for good
- Risk mix: almost everything is **safe** or **needs-care**; nothing here is behaviour-affecting. The only [CONSENSUS] note is 5.3, which adds pins rather than changing any.

### D [T5] arkiv-db tests: durable / faults / KV; examples

- **Tests: 6 delete, 4 merge -> 2.** Delete: fallible_api.rs x3 (one assertion moves to arkiv-db-storage), kv.rs `consecutive_block_packets_..`, semantics.rs `returned_buffers_..` and `block_packets_own_inputs_..`. Merge: the two prefix-scan tests (kv.rs:59 + semantics.rs:27) -> 1; durable_failures.rs:696 into :860 -> 1 (optional). Everything else kept. If the raw KV facade is retired instead: all 10 of its tests (kv.rs 3, semantics.rs 4, backend.rs 2, model.rs 1) and examples/basic.rs go with it.
- **Approx. lines removable**
  - 1. Duplication: ~440 from the seven doubles (net ~330 after ~110 added to faulty.rs; ~190 more outside this chunk), ~250 from shared helpers and fixtures (1.3 items 1-8).
  - 2. Tests: ~465 (fallible_api.rs, or ~435 with a smoke kept) + ~130 (KV facade tests) + ~30 (vacuous assertions, optional merge). With the facade retired: ~700 instead of ~130.
  - 3. Comments: ~34 banner lines, ~30 from module docs; the rest are corrections, not removals.
  - 4. Sloppiness: ~60 (`calls()`), ~40 (literals, let-else, clones); the function splits are neutral.

### D [S1] arkiv-db src: engine, typed, planner, record, layout, codec, meter

- Tests (17 unit tests in chunk): 4 DELETE (record x3, codec x1), 1 MERGE -> `tests/records.rs:750` (3 lines survive), 1 TRIM+rename (meter), 1 optional DELETE (layout), 2 kept with a small fix, 8 kept as is. ~115 lines removable (+25 optional).
- Approx. lines removable by category:
  - Duplication: ~770 (API pairs ~200, read plumbing ~100, codec macros ~160, term-run walking ~70, planner ~55, engine write path ~75, stats/constructors ~45, smaller ~65).
  - Tests: ~115.
  - Comments: ~90 (banners 40, stale/history ~25, long prose ~25; the 40 "Fallible form of" lines are counted under duplication).
  - Sloppiness: ~60 (dead items ~40-60 depending on the `i64` codec decision, closures/clones ~10).
- Stale comments found: 9 sites. `#[allow]` sites: 4 (3 fixable by a params struct, 1 apparently unneeded).

### D [S2] arkiv-db src: durable, proofs, small modules

- Tests: 0 delete; 2 tighten (cache.rs:854, namespace.rs:200); 1 merge into a `const` assertion (streaming.rs:603); 1 to table-drive properly (bulk.rs:854). About 11 lines.
- Approximate lines removable:
  - Duplication: 150-170 (D1) + 75 (D2) + 30 (D3) + 20 (D4) + 35 (D5) + 15 (D6) + 10 (D7) + 10 (D9) + 10 (D10) + 10 (D11) + 45 (D12) + 70 (D15) = about 480-500.
  - Comments: about 35 deleted (C3 and C7), about 10 added (C6), about 15 rewritten (C1, C2, C4, C5).
  - Sloppiness: 6 (S1) + 14 (S2) + 9 (S6) + 10 (S13) = about 40 without owner decisions. With them: +60 (S3), +145 (S4), +70 (S5).
  - Other: about 60 across the workspace for W1 (four helper functions and ~25 let-else blocks), most of it outside this chunk.
- Items tagged [CONSENSUS]: D1.5, D1.6, D2, D8, D9, D12 (version list), S10 (format.rs and path depths), W3, W5, and the format.rs loops under "Already fine". All are refactors that keep today's bytes and charges; none proposes changing a value.

### D [S3] arkiv-db-storage, arkiv-db-memory, arkiv-db-redb src

- Tests: **4 delete** outright (memory `identical_retry_is_never_applied_twice`, `identity_is_fixed_and_health_never_fails`; storage `random_database_ids_differ`; contract `faulty_without_faults_is_transparent`); **5 merge -> 4 existing contract cases + 1 new shared case** (memory `objects_are_put_if_absent_per_space`, `records_are_insert_only`, `head_is_compare_and_set` into their contract namesakes, 5 assertion lines moved; `load_object_reports_missing` into `empty_store_answers_absent`; the memory relabel test and its redb twin into one new contract case). Test instances: 60 duplicate (memory suite run twice) + 33 double-wrapped `faulty_*` + 4 vacuous `reopen_preserves_state`.
- Approximate lines removable in this chunk:
  - Duplication: about 175 (D1 48, D2 30, D3 13, D4 43, D6 20, D7 14, D8 8), plus about 55 in test files outside the chunk (D2, D3).
  - Tests: about 160-280 (T1 125 or 247, T3 5, T4 23, T5 9).
  - Comments: about 120, or 150 with C9 (C1 14, C2 25, C3 20, C4 12, C5 29, C6 10, C8 8, C9 up to 30).
  - Dead code / unused pub: 691 (S1) + about 50 (S2), about 25 more if `set_latch_corrupt` goes.
  - Legacy `KvStore` stack (W5, owner's call): about 150 here, plus the `Database` facade and four test files outside.
- Outside the chunk but enabled by it: about 360 lines of forwarding boilerplate in 8 test doubles (W2).

### D [S4] arkiv-db-redb tests and examples

- Tests: 1 delete candidate by name (`a_killed_store_releases_its_file_lock`, optional) + 2 generated
  modules deleted (`memory`, `faulty_memory`: 60 cases); 0 merges; 0 table-drives. Everything else kept.
- Approx. lines removable:
  - Duplication: ~230 in tests + tests/common (1.4-1.12, 1.15), ~120 in redb_experiment.rs (1.3),
    ~10 in durable_throughput.rs (`Keys`), 39 `.clone()` calls (1.2).
  - Tests: ~48 (37 in contract.rs + 11 in backends.rs) and one dev-dependency.
  - Comments: ~25 deleted, 5 stale ones rewritten.
  - Sloppiness: ~25 (dead retain, `BackendError` line, unused `mod common`, `Arc`, three never-read fields).
  - Examples, if the two unreferenced/one-off ones go: 517 (key_size_report 332 + durable_throughput 185).
- Structural: one integration-test crate turns 18 compilations of the 1,579-line `common` tree into one and
  reduces two file-level `#![allow(dead_code)]` to five struct-level allows plus one per example include.

### D [S5] arkiv-db-trie

- Tests: **1 delete (optional)**, **1 merge -> existing unit test**, **1 split/trim**, **2 tail trims (8 lines)**,
  **3 table-drive**. Approx. -90 lines if all applied; everything pinning encoding/proofs/metering kept.
- Duplication: approx. **-315 lines in tests** (shims 115, build/put 110, commit/reopen 25, raw keys 25,
  interval helpers 25, misc 15) and **-135 lines in src** (staging walk 30, PathNode 25, decisions 14, terminals 12,
  contains 11, seal loop 10, supplied-node 10, shape checks 10, small 13).
- Comments: approx. **-100 lines** (test headers 70, order.rs 10, repeated rationale 15, labels 5); 3 stale statements.
- Sloppiness: 3 dead pub fns (-13 lines), 1 visibility tightening, `KeyWriter`/`KeyReader` decision (-107 if deleted),
  explore.rs tidy, 2 long fns in src + 1 in tests to split.

### D [N1] arkiv-node signed JSON-RPC / HTTP reads

- Tests (43 unit tests in the chunk: rpc.rs 17, rpc_reads.rs 20, rpc_ws.rs 1, metrics.rs 2, state_docs.rs 3): **1 delete** (`estimate_has_no_nonce_balance_or_state_effect`), **1 table-drive**, **2 merge/trim -> shorter** (`sdk_operators_and_bounds`, `malformed_hex_and_log_ranges_...`), **1 split**, 4 single-line/row deletions, 2 moves. ~70 test lines removed, no coverage lost.
- Approx. lines removable:
  - Duplication: ~445 in the chunk (metrics ~95; rpc_reads parser plumbing ~85; rpc.rs repeated blocks ~60; hex/quantity ~45, plus ~25 outside the chunk; test scaffolding ~160 once the host builder is shared).
  - Unneeded tests: ~70.
  - Comments: ~10 (few tells here).
  - Sloppiness: ~55 deleted outright (dead arm + `locked` simplification ~17, side-channel fields/resets/out-param ~20, dead checks ~12, unused field/param ~5); the `dispatch`/`simulation`/`revert_data`/module splits and the reformatting of `state_api.rs:72-101`, `rpc_ws.rs:177-262` and the eight long `json!` lines are line-neutral or add lines but are the largest readability gain in the chunk.
- Items that change a response body or accepted input (`[CONSENSUS]`, do not apply casually): strict `uint` (D1), unified op-selection error codes (D4/S4.5), `ethereum.rs:160` parity code (S4.6), WS/HTTP batch alignment (D5), `TypeTag::name()` coupling (D3), `ethereum::parse` restructure (S6).

### D [N2] arkiv-node runtimes: signed / simulator / demo

- Tests: 0 delete, 0 merge (2 in-chunk tests, both kept). No test helpers defined in the chunk.
- Approx. lines removable: Duplication ~520; Comments ~5 (mostly rewrites); Sloppiness ~110 (dead/unused items and
  wrappers ~60, `Launch`/`Prefill` forwarders and allows ~25, misc ~25). Formatting fixes (Sloppiness 1) add lines.
- Optional, product decision: retiring the volatile demo removes ~1525 src lines + 167 (bin) + 443 (test).
- Parallel code between generations: about 330 lines of signed/runtime.rs restate simulator/runtime.rs and
  simulator/light.rs logic; about 85 lines of simulator/ restate demo/ helpers; signed/host.rs repeats its own
  genesis setup three times (~95 lines).

### D [N3] arkiv-node binaries, genesis_*, query_language

- Tests (genesis, bins, local query_language): 0 delete, 0 merge. Hygiene only: 1 test-only trait deleted
  (12 lines), 4 redundant clones, 1 `unused self`, 1 loop dropped with `--batch-bytes`.
- Tests (imported query_language, only if upstream parity is abandoned): 1 delete, 20 merge -> 9
  (lexer 9 -> 3, literal 7 -> 4, parse 4 -> 2), 1 table-driven in place; recommended: leave all 58.
- Approximate lines removable:
  - Duplication: ~560 (fs helpers 90, arg parsing 45, `_with` wrappers 55, row/launch/report/preamble 60,
    proposer repeats 80, simulator repeats 15, layout/limit/number constants 25, unit-test fixtures 190).
  - Sloppiness: ~130 (error aliases/`err`/`map_err(authoring_error)` 60, `--batch-bytes` chain 25, double
    work and dead checks 20, unused items and `checkpoint` 15, misc 10). The `run()` / `build_file` / `compile`
    splits move lines, they do not remove them.
  - Comments: ~10 lines changed, 4 deleted.
  - Other: ~25 (three hand-written flag guards, `Refusal` if the node error becomes an enum).
- `[CONSENSUS]`-tagged items: Duplication 4, 5, 9; Comments 4; Sloppiness 7, 10, 18; Other 8.

### D [N4] arkiv-node deterministic core

Tests (45 in chunk: gas.rs 12, arkiv/tests.rs 33; value_json.rs 2 not counted):
- delete: 2 outright (`scalar_types_stay_distinct`, `natural_storage_is_not_abi_or_signed_order_encoding`) + 1
  conditional on D5 (`glm_failed_batches_roll_back_rent_debits`).
- merge: 3 -> 2 (`sdk_abi_and_tombstone_validation` into `every_type_id_decodes_...` and `tombstones_are_...`);
  one duplicated block removed between two GLM tests (T6); one tautological block removed (T3); one rename (T8);
  one fixture stripped of asserts (T7).
- gas.rs: 0 deleted, 2 tightened. 5 redundant clones.

Approximate lines removable:
- Duplication: ~330 without the optional codec macro (tests boilerplate ~140, exec entry points ~58 (+12 in
  signed/host.rs), decimals ~40, schedule ~25, accounts ~18, quote/execute ~20, hex ~16, expiry ~10, value_json 12,
  width helpers 12, misc ~10); +~95 with D3. `execute_batch` (41) moves rather than disappears.
- Tests: ~110 (T1 7, T2 ~25, T3 ~33, T4 10, T5 33 conditional, T6 ~15 minus re-homed lines).
- Comments: ~110 (62 restating docs, 16 banner lines, ~20 history/labels, ~12 layout one-liners).
- Sloppiness: ~70 deleted (dead fns 34 + 19 test-only, `seen` 4, small unused fns ~12) and 4 `#[allow]`s gone;
  the S1/S2 splits move lines rather than remove them (S1 removes ~40 through `?`).

### D [NT1] arkiv-node tests: core

- Tests: 3 delete (`in_flight_identities_stay_pending`, `the_spec_exposes_the_genesis`, `headers_carry_the_format_version_and_the_state_root`); 3 merge -> 1 (exec.rs due-schedule trio); 1 rewrite as differential (`oracle_reservation_matches_the_parameters`); 3 partial merges across accounts.rs / gas_execution.rs; 2 single asserts dropped; 2 tests moved replay.rs -> exec.rs. Everything in block_format.rs, the replay.rs fixtures and the oracle itself is kept as consensus pins.
- Approx lines removable:
  - Duplication: ~330 (gas_execution.rs ~125, block_model.rs + block_roots.rs ~85, accounts.rs ~35, replay.rs ~40, exec.rs ~30, shared small helpers ~15)
  - Unneeded tests: ~110
  - Comments: ~40 (banners ~24, repeated namespace note ~10, history comments ~6); 4 stale comments to correct
  - Sloppiness: ~50 (dead support helpers 26, clones / allows / re-imports / leftovers ~25); block_model.rs split is a move, not a removal
- Risk: everything is test-only. `[CONSENSUS]` care is needed only for 1.1 #6 and #7 (helpers must build byte-identical receipts and inputs, or golden hashes move).

### D [NT2] arkiv-node tests: signed

- Tests: 0 whole tests to delete. 1 trivial assertion block deleted (signed_protocol.rs:105-108). 2 partial merges
  (signed_pool.rs:92-103 into :185; signed_http.rs:1280-1309 into :856). 2 tests table-driven into one helper (signed_http.rs:963 + :1022).
  1 block that is a subset of unit tests split out and optionally trimmed (ethereum_transactions.rs:393-431).
  Splits: 5 giant tests -> about 15 (signed_http.rs:185 -> 4, :718 -> 3; ethereum_transactions.rs:172 -> 2, :299 -> 3; arkiv_profile.rs:247 -> 3-4).
- Approx lines removable:
  - Duplication, shared support (1.1-1.3): ~400 net (about 510 out of the test files, about 110 into the two support files)
  - Duplication inside files (1.4, not already counted above): ~300 (signed_http ~100, signed_protocol ~65, signed_host ~60, signed_history ~35, arkiv_profile ~25, signed_reads ~25)
  - `SlowStore` delegation (1.5): ~45, given a shared forwarding wrapper
  - Tests (2): ~80-110
  - Comments (3): ~20
  - Sloppiness (4): ~50 (helpers in 4.1) + reformatting of 9 lines over 150 chars
  Total about 900 of the 6,944 lines in the chunk. The splits in 4.1 add lines back (one fixture call and a fn header per new test).

### D [NT3] arkiv-node tests: genesis / simulator / demo / CLI; examples; Cargo.toml; clippy.toml

- Tests: **0 delete**, 0 merge now (1 possible after D3: CLI adopt phases 583-613), 1 move (`demo_auth.rs:33` -> `tests/replay.rs`), **8 split** recommendations (genesis_prefill :150, :328 optional, `exercise_permanent_chain` :657; genesis_adopt :617; prebuilt :290; simulator_http :126, :524; demo_auth :263 optional); one test that never runs (S1).
- Approx. lines removable: Duplication ~-470 (D1 ~-245, D2 ~-90, D4 ~-40, D5 -30 across three files, D6 ~-45, D7 ~-30, D8 ~-40, D10 ~-35; D3 moves ~55 test lines into the library rather than deleting); Tests ~-6; Comments ~0 (7 tighten/delete); Sloppiness ~-15 (clones, Cargo.toml lines, eprintln) plus pure reformatting.
- Config: `clippy.toml` +2 entries (8 call sites to convert); `Cargo.toml` -2 lines, +4-5 one-line comments, +1 dev feature.

### D [D1] Documentation

- Numbered findings in section 1: R1-R13, A1-A11, T1-T13, O1-O14, 5 rows for arkiv-v2-compatibility.md, 5 small-README items = 61. About 43 of them are direct doc-vs-code (or doc-vs-doc) contradictions with both sides quoted; the rest are structure, scope or wording. Plus 10 broken anchors (3 targets) and 9 dangling cross-repo paths.
- Redundant groups: 9 (G1-G9) covering 42 of the 54 docs/*.md files.
- Plan/todo documents fully done: 2 to delete (672 lines), 6 plan sections to archive; 1 unimplemented plan to move to proposals/; 1 genuine todo kept.
- Step labels: 51 uses in crates/ source comments (top: N0a 11, N3b 8, milestone-2 8), about 365 in docs; defined in three plan files only.
- Lines leaving the live docs tree: about 6,150 of 15,899 (deleted about 1,100, archived about 2,650, moved to runs/benchmarks/proposals about 1,400, trimmed or merged away about 1,000) plus about 230 from README.
- Not verified (said so above): T13 (isolation.rs padded-key derivation), whether `scripts/sdk-compat-smoke.{py,ts}` still pass, every gas weight in glm-gas-v1.md:301-304, the byte layouts inside block-format.md Part 2 and simulator-storage-profile.md against the encoders.

### D [D2] Scripts, CI, build config, vendor/reth-trie, conformance

- Tests: **0 delete** outright (1 redundant assert line); **1 merge** (test_genesis_install... onto `run_with_fake_bin`, about -35
  lines); **3 table-drive -> 1 helper** (about -25 lines); 32 methods / 738 lines leave with S2b if the codec-2 converter is retired.
  CI runs none of the five scripts/test-*.py.
- Approximate lines removable:
  - Duplication: about 700 (smoke launcher library 450-500; fixture patching 45; test helpers 60; conformance 40;
    workflow 30; bench 15; sourcify-prefill internal 30, moot if S2b).
  - Obsolete / dead (Sloppiness): up to about 3,700: sdk-compat pair 762; codec-2 pipeline 2,446 (or 1,710 if the runner is
    ported to v4); query-order-bench.py 262; vendor/reth-trie 243 + 2 licence files; genesis report bookkeeping 8;
    Cargo.toml 3 lines.
  - Comments: about 25 lines (plus the 65-line docstring if tightened).
- Things that are wrong rather than untidy: S1 (session scratch path and a token file path as defaults), S3
  (Dockerfile.simulator cannot build: `include_str!` of a file under `docs/` that is not copied), S2b (the full-prefill
  runner can only drive a converter whose output the node refuses), three `stop()` copies without a kill fallback (D1),
  ci.sh:70.

## Appendix E: tool output (filtered)

All commands were run read-only with a separate `CARGO_TARGET_DIR`, toolchain 1.90.0.

### E.1 Functions over 100 lines in `src/` (clippy `too_many_lines`, code lines / limit)

```
crates/arkiv-node/src/bin/arkiv-proposer.rs:604:1 (664/100)
crates/arkiv-node/src/signed/rpc.rs:942:1 (319/100)
crates/arkiv-node/src/simulator/metadata.rs:71:1 (275/100)
crates/arkiv-node/src/bin/arkiv-simulator.rs:38:1 (267/100)
crates/arkiv-node/src/signed/metrics.rs:215:5 (263/100)
crates/arkiv-node/src/arkiv/mod.rs:394:1 (245/100)
crates/arkiv-node/src/genesis_bulk.rs:118:1 (228/100)
crates/arkiv-db-trie/src/interval.rs:364:1 (209/100)
crates/arkiv-db/src/proof/range.rs:644:1 (203/100)
crates/arkiv-node/src/genesis_authoring.rs:834:1 (192/100)
crates/arkiv-node/src/signed/host.rs:906:5 (181/100)
crates/arkiv-db/src/proof/range.rs:453:1 (181/100)
crates/arkiv-db/src/proof.rs:606:1 (174/100)
crates/arkiv-db/src/engine.rs:2440:1 (170/100)
crates/arkiv-db/src/typed/planner.rs:621:1 (161/100)
crates/arkiv-db/src/proof.rs:887:1 (157/100)
crates/arkiv-node/src/signed/rpc.rs:782:1 (153/100)
crates/arkiv-node/src/signed/rpc.rs:611:1 (144/100)
crates/arkiv-db-trie/src/proof.rs:316:1 (144/100)
crates/arkiv-node/src/simulator/runtime.rs:732:1 (131/100)
crates/arkiv-db/src/durable/bulk.rs:494:5 (131/100)
crates/arkiv-node/src/bin/arkiv-demo.rs:33:7 (130/100)
crates/arkiv-node/src/arkiv/mod.rs:699:1 (122/100)
crates/arkiv-node/src/exec.rs:560:1 (118/100)
crates/arkiv-node/src/genesis_streaming.rs:456:1 (117/100)
crates/arkiv-node/src/exec.rs:1099:5 (117/100)
crates/arkiv-db/src/explore.rs:39:5 (115/100)
crates/arkiv-db/src/durable/mod.rs:455:5 (111/100)
crates/arkiv-node/src/simulator/workload.rs:30:5 (109/100)
crates/arkiv-node/src/demo/workload.rs:51:1 (109/100)
crates/arkiv-node/src/demo/runtime.rs:480:5 (109/100)
crates/arkiv-node/src/signed/runtime.rs:494:1 (108/100)
crates/arkiv-node/src/genesis_bundle.rs:465:1 (108/100)
crates/arkiv-node/src/exec.rs:796:5 (108/100)
crates/arkiv-node/src/exec.rs:948:5 (106/100)
crates/arkiv-node/src/signed/ethereum.rs:84:1 (105/100)
crates/arkiv-node/src/genesis_streaming.rs:779:5 (105/100)
crates/arkiv-node/src/signed/rpc_reads.rs:1543:5 (104/100)
crates/arkiv-db/src/durable/mod.rs:782:5 (103/100)
```

### E.2 Test functions over 150 lines (same lint)

```
crates/arkiv-db/tests/query_isolation.rs:1514:1 (854/100)
crates/arkiv-db/tests/query_history.rs:3343:1 (363/100)
crates/arkiv-db/tests/history.rs:1513:1 (321/100)
crates/arkiv-db/tests/blocks.rs:277:1 (317/100)
crates/arkiv-node/tests/signed_http.rs:185:7 (307/100)
crates/arkiv-db/examples/records.rs:93:1 (281/100)
crates/arkiv-db/tests/queries.rs:1614:1 (243/100)
crates/arkiv-db/tests/query_history.rs:1624:1 (228/100)
crates/arkiv-db/tests/query_isolation.rs:3109:1 (218/100)
crates/arkiv-node/tests/genesis_prefill.rs:657:1 (207/100)
crates/arkiv-db/tests/records.rs:1579:1 (198/100)
crates/arkiv-db/tests/query_isolation.rs:1177:1 (193/100)
crates/arkiv-db/tests/query_metering.rs:2703:1 (192/100)
crates/arkiv-db/tests/isolation.rs:379:1 (188/100)
crates/arkiv-node/tests/gas_execution.rs:719:1 (185/100)
crates/arkiv-db/tests/query_metering.rs:4029:1 (180/100)
crates/arkiv-node/tests/arkiv_profile.rs:247:7 (179/100)
crates/arkiv-db/tests/support/durable_history.rs:158:1 (173/100)
crates/arkiv-node/tests/block_model.rs:1757:1 (172/100)
crates/arkiv-db/tests/query_history.rs:2853:1 (172/100)
crates/arkiv-db/tests/queries.rs:1290:1 (172/100)
crates/arkiv-node/tests/signed_host.rs:980:1 (171/100)
crates/arkiv-node/tests/ethereum_transactions.rs:299:7 (171/100)
crates/arkiv-db/tests/queries.rs:347:1 (169/100)
crates/arkiv-node/tests/simulator_http.rs:126:7 (168/100)
crates/arkiv-node/tests/simulator.rs:390:1 (166/100)
crates/arkiv-db/tests/query_metering.rs:3325:1 (165/100)
crates/arkiv-db-redb/examples/bench_readers.rs:182:1 (165/100)
crates/arkiv-node/tests/signed_http.rs:1148:7 (163/100)
crates/arkiv-db/tests/bounds.rs:189:1 (163/100)
crates/arkiv-node/tests/genesis.rs:127:1 (162/100)
crates/arkiv-db/tests/query_history.rs:2036:1 (161/100)
crates/arkiv-db/tests/query_metering.rs:3035:1 (159/100)
crates/arkiv-db-redb/examples/key_size_report.rs:166:1 (159/100)
crates/arkiv-db-trie/tests/store.rs:68:1 (157/100)
crates/arkiv-node/tests/block_model.rs:2089:1 (156/100)
crates/arkiv-db/tests/query_metering.rs:2426:1 (156/100)
crates/arkiv-db/tests/query_history.rs:3111:1 (156/100)
crates/arkiv-node/tests/genesis_prefill.rs:328:1 (153/100)
crates/arkiv-db/examples/blocks.rs:11:1 (153/100)
crates/arkiv-node/tests/prebuilt_genesis_cli.rs:290:1 (150/100)
```

### E.3 Clippy `redundant_clone` sites (all 93)

```
crates/arkiv-db/examples/records.rs:299:26
crates/arkiv-db/examples/records.rs:339:35
crates/arkiv-db/examples/records.rs:357:18
crates/arkiv-db-redb/examples/durable_throughput.rs:125:59
crates/arkiv-db-redb/tests/disk_full.rs:76:31
crates/arkiv-db-redb/tests/latching.rs:141:52
crates/arkiv-db-redb/tests/reconciliation.rs:153:43
crates/arkiv-db/src/layout.rs:717:39
crates/arkiv-db/src/layout.rs:942:49
crates/arkiv-db/src/layout.rs:949:31
crates/arkiv-db/src/layout.rs:959:31
crates/arkiv-db/src/layout.rs:1025:21
crates/arkiv-db/src/proof/range.rs:387:25
crates/arkiv-db-storage/src/testing/contract.rs:407:59
crates/arkiv-db-storage/src/testing/contract.rs:504:26
crates/arkiv-db-storage/src/testing/contract.rs:766:60
crates/arkiv-db-storage/src/testing/contract.rs:958:52
crates/arkiv-db/tests/adopt.rs:261:42
crates/arkiv-db/tests/adopt.rs:261:60
crates/arkiv-db/tests/adopt.rs:279:20
crates/arkiv-db/tests/adopt.rs:281:21
crates/arkiv-db/tests/adopt.rs:282:19
crates/arkiv-db/tests/adopt.rs:293:22
crates/arkiv-db/tests/adopt.rs:294:23
crates/arkiv-db/tests/cell_limits.rs:164:21
crates/arkiv-db/tests/cell_limits.rs:178:27
crates/arkiv-db/tests/codec.rs:983:18
crates/arkiv-db/tests/durable_failures.rs:560:38
crates/arkiv-db/tests/durable_failures.rs:596:44
crates/arkiv-db/tests/durable_failures.rs:1394:53
crates/arkiv-db/tests/durable_failures.rs:1444:20
crates/arkiv-db/tests/durable_failures.rs:1557:14
crates/arkiv-db/tests/eq_proofs.rs:391:34
crates/arkiv-db/tests/eq_proofs.rs:568:24
crates/arkiv-db/tests/ordered_ranges.rs:351:38
crates/arkiv-db/tests/pagination.rs:2202:28
crates/arkiv-db/tests/prefix_free_terms.rs:668:27
crates/arkiv-db/tests/prefix_free_terms.rs:668:46
crates/arkiv-db/tests/query_isolation.rs:2297:22
crates/arkiv-db/tests/query_metering.rs:1088:34
crates/arkiv-db/tests/query_metering.rs:1636:29
crates/arkiv-db/tests/query_metering.rs:1672:29
crates/arkiv-db/tests/query_metering.rs:1736:29
crates/arkiv-db/tests/query_metering.rs:1800:29
crates/arkiv-db/tests/query_metering.rs:1906:29
crates/arkiv-db/tests/query_metering.rs:2096:29
crates/arkiv-db/tests/query_planner.rs:616:24
crates/arkiv-db/tests/query_planner.rs:617:26
crates/arkiv-db/tests/range_proofs.rs:523:21
crates/arkiv-db/tests/records.rs:1715:44
crates/arkiv-db/tests/records.rs:1737:52
crates/arkiv-db/tests/rehash.rs:687:15
crates/arkiv-db/tests/rollback.rs:760:38
crates/arkiv-db/tests/simulator_composition.rs:202:44
crates/arkiv-db/tests/simulator_composition.rs:274:21
crates/arkiv-db/tests/simulator_composition.rs:290:14
crates/arkiv-db/tests/terms.rs:690:31
crates/arkiv-db/tests/terms.rs:1156:33
crates/arkiv-db-trie/src/node.rs:677:31
crates/arkiv-db-trie/src/node.rs:729:36
crates/arkiv-db-trie/src/node.rs:813:23
crates/arkiv-db-trie/tests/map.rs:136:21
crates/arkiv-db-trie/tests/store.rs:452:42
crates/arkiv-db-trie/tests/variable_keys.rs:206:26
crates/arkiv-node/src/arkiv/tests.rs:194:23
crates/arkiv-node/src/arkiv/tests.rs:1130:41
crates/arkiv-node/src/arkiv/tests.rs:1565:27
crates/arkiv-node/src/arkiv/tests.rs:1629:27
crates/arkiv-node/src/arkiv/tests.rs:1642:25
crates/arkiv-node/src/genesis_prebuilt.rs:753:22
crates/arkiv-node/src/genesis_streaming.rs:865:27
crates/arkiv-node/src/genesis_streaming.rs:1044:28
crates/arkiv-node/src/genesis_streaming.rs:1103:28
crates/arkiv-node/src/signed/rpc_reads.rs:3003:31
crates/arkiv-node/src/signed/rpc.rs:2086:51
crates/arkiv-node/src/signed/rpc.rs:2086:82
crates/arkiv-node/tests/demo_auth.rs:181:34
crates/arkiv-node/tests/demo_auth.rs:216:44
crates/arkiv-node/tests/demo_auth.rs:216:57
crates/arkiv-node/tests/gas_execution.rs:475:24
crates/arkiv-node/tests/identity.rs:193:14
crates/arkiv-node/tests/selection.rs:49:27
crates/arkiv-node/tests/signed_history.rs:184:31
crates/arkiv-node/tests/signed_host.rs:198:34
crates/arkiv-node/tests/signed_host.rs:319:28
crates/arkiv-node/tests/signed_host.rs:381:33
crates/arkiv-node/tests/signed_host.rs:643:14
crates/arkiv-node/tests/signed_protocol.rs:476:29
crates/arkiv-node/tests/signed_protocol.rs:497:44
crates/arkiv-node/tests/simulator.rs:222:33
crates/arkiv-node/tests/simulator.rs:224:32
crates/arkiv-node/tests/simulator.rs:453:31
crates/arkiv-node/tests/validation.rs:46:25
```

### E.4 Other clippy signal read and classified

| Site | Lint | Classification |
|---|---|---|
| `crates/arkiv-db/src/durable/simulator.rs:146` | suspicious_operation_groupings | false positive (`height` vs `sequence` naming) |
| `crates/arkiv-node/src/demo/auth.rs:93` | suspicious_operation_groupings | false positive (suggestion would not compile) |
| `crates/arkiv-node/src/genesis_bulk.rs:97` | suspicious_operation_groupings | false positive |
| `crates/arkiv-node/src/signed/rpc.rs:1250` | suspicious_operation_groupings | not a bug; `!n.is_finite()` is merely redundant |
| `crates/arkiv-node/src/signed/runtime.rs:221` | collection is never read | false positive (lazy RAII guard), restructuring is clearer |
| `crates/arkiv-node/src/signed/pool.rs:383` | unnecessarily wrapped by `Result` | correct: `insert_checked` only ever returns `Ok(Admission::Inserted)` |
| `crates/arkiv-node/src/genesis_authoring.rs:1293` | unused `self` | correct (in a test helper) |
| `crates/arkiv-db-trie/src/node.rs:677,687,711,717,729,813` | redundant clone, single-item `into_iter` | all inside `mod tests`; the production encoder is clean |

### E.5 rustdoc warnings (`cargo doc --no-deps --document-private-items`)

```
crates/arkiv-db-trie/src/stage.rs:8:36: warning: redundant explicit link target: explicit target is redundant
crates/arkiv-db/src/durable/adopt.rs:498:22: warning: unresolved link to `initialize_simulator_from_staged`: no item named `initialize_simulator_from_staged` in scope
crates/arkiv-db/src/durable/adopt.rs:510:20: warning: unresolved link to `initialize_simulator_from_staged`: no item named `initialize_simulator_from_staged` in scope
crates/arkiv-db/src/layout.rs:10:19: warning: unresolved link to `order::encode_bytes`: no item named `order` in scope
crates/arkiv-db/src/layout.rs:11:35: warning: unresolved link to `order::encode_shifted`: no item named `order` in scope
crates/arkiv-db/src/layout.rs:154:41: warning: unresolved link to `order::encode_bytes`: no item named `order` in scope
crates/arkiv-db/src/codec.rs:794:31: warning: unresolved link to `RecordKey::encoded`: the struct `RecordKey` has no field or associated item named `encoded`
crates/arkiv-node/src/signed/runtime.rs:80:16: warning: public documentation for `node` links to private item `Self::read_node`: this item is private
crates/arkiv-node/src/signed/runtime.rs:81:11: warning: public documentation for `node` links to private item `Self::rpc_node`: this item is private
crates/arkiv-node/src/signed/runtime.rs:162:42: warning: public documentation for `reads_waiting` links to private item `Self::read_node`: this item is private
crates/arkiv-node/src/signed/runtime.rs:166:54: warning: public documentation for `requests_waiting` links to private item `Self::rpc_node`: this item is private
crates/arkiv-node/src/lib.rs:14:9: warning: `select` is both a function and a module: ambiguous link
crates/arkiv-node/src/lib.rs:19:9: warning: `replay` is both a function and a module: ambiguous link
```

### E.6 Lines over 150 characters, per file

```
20 lines (longest 1634) crates/arkiv-db-redb/examples/engine_storage_experiment.rs
16 lines (longest 725) crates/arkiv-node/src/signed/runtime.rs
9 lines (longest 299) crates/arkiv-db-redb/examples/redb_experiment.rs
8 lines (longest 764) crates/arkiv-node/src/signed/rpc.rs
7 lines (longest 684) crates/arkiv-node/src/signed/state_api.rs
5 lines (longest 203) crates/arkiv-node/tests/signed_http.rs
4 lines (longest 868) crates/arkiv-node/src/simulator/runtime.rs
4 lines (longest 475) crates/arkiv-node/src/signed/host.rs
4 lines (longest 393) crates/arkiv-db/src/meter.rs
4 lines (longest 1155) crates/arkiv-node/src/bin/arkiv-simulator.rs
3 lines (longest 464) crates/arkiv-node/src/demo/runtime.rs
3 lines (longest 3633) crates/arkiv-node/src/bin/arkiv-proposer.rs
3 lines (longest 355) crates/arkiv-node/src/signed/metrics.rs
3 lines (longest 351) crates/arkiv-node/examples/verify_inspection.rs
3 lines (longest 227) crates/arkiv-node/src/signed/rpc_reads.rs
3 lines (longest 190) crates/arkiv-node/tests/ethereum_transactions.rs
3 lines (longest 173) crates/arkiv-node/src/signed/rpc_ws.rs
2 lines (longest 583) crates/arkiv-node/src/genesis_bundle.rs
2 lines (longest 565) crates/arkiv-node/src/simulator/light.rs
2 lines (longest 251) crates/arkiv-node/tests/simulator_http.rs
2 lines (longest 186) crates/arkiv-db/src/layout.rs
2 lines (longest 180) crates/arkiv-node/src/simulator/query_language.rs
1 lines (longest 466) crates/arkiv-node/src/genesis_streaming.rs
1 lines (longest 420) crates/arkiv-node/examples/gas_calibration.rs
1 lines (longest 277) crates/arkiv-db-redb/examples/reachability_report.rs
1 lines (longest 202) crates/arkiv-node/tests/signed_retention.rs
1 lines (longest 200) crates/arkiv-db-redb/src/lib.rs
1 lines (longest 185) crates/arkiv-db-redb/examples/durable_throughput.rs
1 lines (longest 174) crates/arkiv-node/src/genesis_authoring.rs
1 lines (longest 166) crates/arkiv-db/tests/range_proofs.rs
1 lines (longest 162) crates/arkiv-node/tests/demo_auth.rs
1 lines (longest 155) crates/arkiv-node/src/simulator/host.rs
1 lines (longest 151) crates/arkiv-node/tests/genesis_adopt.rs
1 lines (longest 151) crates/arkiv-db/tests/simulator_composition.rs
```

### E.7 Item boundaries with no blank line (files with more than 10)

```
45 crates/arkiv-node/src/genesis_authoring.rs
33 crates/arkiv-node/src/signed/runtime.rs
31 crates/arkiv-node/src/signed/rpc_reads.rs
30 crates/arkiv-node/src/simulator/runtime.rs
30 crates/arkiv-node/src/signed/rpc.rs
25 crates/arkiv-db/src/proof.rs
21 crates/arkiv-node/tests/gas_execution.rs
21 crates/arkiv-node/src/genesis_bundle.rs
21 crates/arkiv-node/src/arkiv/tests.rs
20 crates/arkiv-db/src/durable/bulk.rs
19 crates/arkiv-node/src/simulator/wire.rs
19 crates/arkiv-node/src/bin/arkiv-proposer.rs
18 crates/arkiv-node/tests/signed_http.rs
18 crates/arkiv-node/tests/accounts.rs
18 crates/arkiv-node/src/signed/protocol.rs
17 crates/arkiv-db-redb/tests/engine_support/mod.rs
16 crates/arkiv-node/src/demo/wire.rs
16 crates/arkiv-node/src/arkiv/mod.rs
15 crates/arkiv-node/tests/signed_reads.rs
15 crates/arkiv-node/tests/signed_host.rs
15 crates/arkiv-node/tests/genesis_prefill.rs
15 crates/arkiv-node/src/accounts.rs
14 crates/arkiv-node/src/simulator/host.rs
13 crates/arkiv-db/src/durable/mod.rs
12 crates/arkiv-node/src/signed/ethereum.rs
12 crates/arkiv-node/src/demo/runtime.rs
12 crates/arkiv-db/src/proof/range.rs
11 crates/arkiv-node/tests/simulator_http.rs
11 crates/arkiv-node/src/signed/metrics.rs
11 crates/arkiv-node/src/arkiv/schema.rs
11 crates/arkiv-db/tests/eq_proofs.rs
```

### E.8 Tests listed by `cargo test --workspace -- --list`

1,324 tests in 117 binaries: vendor 35 (alloy-trie 29 + 1 doctest, reth-trie 5); arkiv-db 55 unit + 432 integration in 40 binaries; arkiv-db-memory 11 unit + 60 contract; arkiv-db-redb 10 unit + 207 integration in 18 binaries (150 of them the contract suite); arkiv-db-storage 6 unit; arkiv-db-trie 19 unit + 76 integration in 12 binaries + 2 doctests; arkiv-node 202 unit + 3 (proposer bin) + 232 integration in 27 binaries.

### E.9 Stringly error codes in arkiv-node (construction sites, top 20 of 107)

```
    180 Error("InvalidRequest")
    107 Error("StorageCorrupt")
     99 Error("StorageUnavailable")
     75 Error("InvalidParams")
     41 Error("IdentityMismatch")
     40 Error("ChainConflict")
     36 Error("CapacityReached")
     28 Error("CommitUncertain")
     20 Error("ServerBusy")
     17 Error("InvalidProof")
     13 Error("BlockNotSynced")
     12 Error("UnsupportedNodeRole")
     12 Error("InvalidPrefillBundle")
     11 Error("RequestConflict")
      7 Error("UnsupportedProof")
      7 Error("UnknownBlock")
      7 Error("CursorMismatch")
      6 Error("ProducerOnly")
      6 Error("InvalidSignature")
      5 Error("PrefillBundleRequired")
```
