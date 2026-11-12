# Code-quality review, 2026-11-17

**Date:** 2026-11-17. **Commit reviewed:** `e6c49e1` on `main`. Every line
number and path below refers to that commit.

Every Rust file of the six crates, every script and every document was read
in full, each by one of 19 reviewers with a disjoint set of files. Each
claim had to be backed by evidence (the duplicate's `file:line`, the `rg`
showing an item is unused, the comment quoted beside the code it
contradicts), and the highest-payoff and most surprising claims were checked
again against the source.

**Status (2026-11-18).** The findings were acted on in four commits:
`a1da683` (dead code, duplicate tests, stale comments and docs), `c2116ee`
(licence, test-only code), `01bf735` and `b47fa99` (refactors and the five
behaviour-affecting defects). Since then many paths have moved: integration
tests live under `tests/it/`, `engine.rs` is `engine/`, `signed/rpc.rs` is
`signed/rpc/`. What is still open is in
[code-quality-todo.md](../../code-quality-todo.md).

Parts: [summary](README.md) · [duplication](duplication.md) · [tests](tests.md) · [comments](comments.md) · [sloppiness](sloppiness.md) · [other](other.md) · [appendices](appendices.md)

---

## How to read this file

- **Part I** (this part) is the curated result: verdict, top items, totals, the do-not-touch list, then the highest-payoff findings per category, ordered by payoff.
- **Part II** is the complete inventory: every finding of every area, grouped by the same five categories, each with `file:line`, the fix and a risk tag. Part I points into it as `[T3 §2.1]` = area T3, heading 2.1.
- **Appendices**: A area notes and direct answers, B helper inventories for the shared test-support modules, C per-area "already fine" lists, D per-area totals, E filtered tool output (long functions, all 93 redundant clones, rustdoc warnings, long lines).

Risk tags: **safe** (no behaviour change) · **needs-care** (mechanical, but touches many call sites or test pins) · **behaviour-affecting**. `[CONSENSUS]` marks anything whose fix could change canonical encoding, hashes, state roots, receipts or metering numbers, RPC/HTTP response bodies or accepted inputs, or the on-disk format.

Area codes (each area's files were read in full, top to bottom):

| Code | Files | Path base used in its findings |
|---|---|---|
| T1 | arkiv-db tests: query_metering, metering, trace_regression, query_planner, scalar_profile, cell_limits, bounds | `crates/arkiv-db/tests/`; `qm` = query_metering.rs |
| T2 | arkiv-db tests: query_history, history, query_isolation, isolation | `crates/arkiv-db/tests/`; `h`, `qh`, `i`, `qi` |
| T3 | arkiv-db tests: queries, records, pagination, terms | `crates/arkiv-db/tests/`; `qm` = query_metering.rs |
| T4 | arkiv-db tests: codec, namespaces, blocks, rollback, roots, eq_proofs, range_proofs, prefix_free_terms, ordered_ranges, golem_types, host_state | `crates/arkiv-db/tests/` |
| T5 | arkiv-db tests: durable_*, storage_faults, simulator_composition, rehash, adopt, fallible_api, streaming_import, model, interrupt, backend, kv, semantics, support/; examples/ | `crates/arkiv-db/` |
| S1 | arkiv-db src: engine, typed, typed/planner, record, layout, codec, meter | `crates/arkiv-db/src/` |
| S2 | arkiv-db src: durable/*, proof, proof/range, query, host, state, namespace, nodes, lib, explore, interrupt | `crates/arkiv-db/src/` |
| S3 | arkiv-db-storage (all), arkiv-db-memory (all), arkiv-db-redb/src | repo-relative |
| S4 | arkiv-db-redb tests/ and examples/ | `crates/arkiv-db-redb/` |
| S5 | arkiv-db-trie src/ and tests/ | `crates/arkiv-db-trie/` |
| N1 | arkiv-node src/signed: rpc_reads, rpc, rpc_ws, ethereum, range_api, state_api, state_docs, reads, metrics | `crates/arkiv-node/` |
| N2 | arkiv-node src/signed (runtime, host, protocol, light, pool, history, crypto), src/simulator (non-QL), src/demo | `crates/arkiv-node/` |
| N3 | arkiv-node src/bin/*, genesis_*.rs, retention.rs, simulator/query_language* | `crates/arkiv-node/src/` |
| N4 | arkiv-node src: exec, block, gas, params, validate, identity, accounts, value_json, replay, select, lib, arkiv/* | `src/` = `crates/arkiv-node/src/` |
| NT1 | arkiv-node tests: block_model, replay, gas_execution, block_format, accounts, exec, genesis, validation, identity, selection, support/ | `T/` = `crates/arkiv-node/tests/` |
| NT2 | arkiv-node tests: signed_*, signed_support/, ethereum_transactions, arkiv_profile | `crates/arkiv-node/tests/` |
| NT3 | arkiv-node tests: genesis_prefill, genesis_adopt, prebuilt_genesis_cli, simulator*, demo_auth, retention_cli; examples/; Cargo.toml; clippy.toml | `crates/arkiv-node/` |
| D1 | README, CHANGELOG, AGENTS, docs/**, small READMEs | repo-relative |
| D2 | scripts/**, CI workflows, Dockerfile, deploy/, Cargo.toml files, vendor/reth-trie, conformance/golem-db | repo-relative |

## Method and coverage

- Size: about 150,000 lines of own Rust (arkiv-db 19.8k src + 49.9k tests/examples; arkiv-node 36.6k + 19.8k; arkiv-db-trie 5.2k + 5.0k; arkiv-db-redb 2.8k + 6.6k; arkiv-db-storage 3.1k; arkiv-db-memory 1.4k; conformance 3.0k), 10.5k lines of scripts, 17.7k lines of markdown.
- Every `.rs` file of the six crates, every script and every doc was assigned to exactly one of 19 reviewers with a disjoint file list; each read its files in full and had to verify each claim (name the duplicate with file:line, show the `rg` for "unused", quote the comment and the contradicting code for "stale"). I then re-checked the highest-payoff and the surprising claims against the source myself; those are marked **(re-verified)** below. Nothing I re-checked was wrong.
- Tool signal (filtered by hand, not pasted):
  - `cargo +1.90.0 clippy --all-targets -- -W clippy::pedantic -W clippy::nursery`: 3,158 distinct warnings, about 90% doc/must_use/const-fn/cast noise. Used: 93 `redundant clone`, the `too many lines` list, `unused self`, `unnecessarily wrapped`, `identical bodies`. Four `suspicious_operation_groupings` hits (durable/simulator.rs:146, demo/auth.rs:93, genesis_bulk.rs:97, signed/rpc.rs:1250) were read and are all false positives.
  - `cargo doc --no-deps --document-private-items`: 13 warnings, 7 of them broken or stale intra-doc links (listed in §3).
  - `cargo test --workspace -- --list`: **1,324 tests** in 117 binaries. 35 are vendor tests (alloy-trie 30, reth-trie 5), so 1,289 are the repo's own; 1,072 are `#[test]` attributes and 210 are generated by `versioned_store_contract!` (7 instantiations x 30 cases).
  - `cargo fmt --check` exits 0, yet 295 lines exceed rustfmt's 100 columns (193 in `src/`, 74 over 150, the longest 3,633): rustfmt silently skips any expression containing a line it cannot break.

## Verdict

The core is not slop. `arkiv-db` (engine, codec, layout, proofs), `arkiv-db-trie`, `arkiv-db-storage`, the redb store and the node's `block`/`exec`/`params`/`validate`/`identity`/`replay`/`select` are careful: hand-derived golden vectors, independent oracles, exact receipts, crash and fault models, almost no reachable `unwrap`. Very little of that test suite is trivial.

What reads as machine-assisted is around it, in four shapes:

1. **Copy-evolved scaffolding.** Each new test file re-typed its helpers, its reference model and its `Branch`/`Snapshot` reader trait; each new node generation (demo, simulator, signed) re-typed the previous one's runtime; each new script re-typed the node launcher. This is where most removable lines are.
2. **Two house styles.** The older files are spaced, documented and banner-heavy. The newer arkiv-node files (`signed/`, `simulator/`, `demo/`, `bin/`, `genesis_*`, `gas.rs`, `accounts.rs`, `arkiv/*`) have no blank lines between items (about 810 such item boundaries workspace-wide, most of them there), one-letter bindings, bare tag literals, string error codes, and regions rustfmt gave up on.
3. **Accumulated narration.** Milestone and step labels (`N0a`, `N3b`, `milestone-2`), references to "the brief" and "decisions2.md" that are not in the repo, plan documents beside their implementations, run logs and one-host deployment notes in `docs/`, and about 43 doc statements that contradict the code.
4. **Leftovers.** A 691-line unused module, a raw KV facade only its own tests use, a vendored crate nothing depends on, an obsolete script pipeline, a Dockerfile that cannot build, test-only API in the public surface.

On tests specifically: the expectation of large fat by test count is only partly met. About **86 named tests plus 60 generated instances (roughly 146 of 1,289, 11%)** can go with no loss of coverage. The larger fat is in test *lines*: roughly 4,500 lines of redundant test bodies and 6,600 lines of duplicated test scaffolding.

## Top 15 by payoff

| # | Item | Payoff | Risk | Detail |
|---|---|---|---|---|
| 1 | One shared test-support layer per crate. arkiv-db tests re-type the helper vocabulary (`ns` in 23 files with 5 bodies, `receipt` in 12 with 6 signatures, `traced` 8, `put` 10, `delete` 13, owner constants in 22 with different bytes for `CAROL`/`DAVE`), the record/query reference model (7 copies), the oracle harness (`history.rs` to `query_history.rs`: 1,004 of 2,080 lines recur; `isolation.rs` to `query_isolation.rs`: 515 of 1,374) and a delegating `trait Reader` (about 10 files). arkiv-node tests ignore `signed_support` in 6+ files and carry 4 CLI harnesses. | about 6,600 test lines | needs-care (shared helpers must build byte-identical receipts and inputs) | §1.1-1.3 |
| 2 | `records.rs` and `queries.rs` re-pin charge vectors that `query_metering.rs` already pins on the same fixture with the same numbers, more strictly. | about 1,570 lines, 14 to 21 tests | needs-care `[CONSENSUS]` (the surviving copy is the only pin) | §2 A |
| 3 | Dead and legacy code: `crates/arkiv-db-memory/src/persistent.rs` (691 lines, exported, no user; the architecture doc itself says the engine no longer uses it) **(re-verified)**; the raw KV facade `Database`/`KvStore`/`MemoryStore` used only by its own 4 test files and one example; `vendor/reth-trie` (workspace member, no dependent) **(re-verified)**; about 30 dead `pub` items. | 740 lines certain, about 1,100 more on owner decision | safe | §4.1 |
| 4 | `tests/fallible_api.rs` (465 lines) asserts `x() == try_x()` where every `x` is literally `in_memory(self.try_x(..))` **(re-verified)**. The 30 infallible twins themselves (about 232 lines in `engine.rs`) are called by no non-test code. | 465 + about 200 lines | safe / needs-care | §2 B, §1.4 |
| 5 | The storage contract suite runs redundantly: `memory` and `faulty_memory` are instantiated in both `arkiv-db-memory/tests/versioned_contract.rs` and `arkiv-db-redb/tests/contract.rs` **(re-verified)**; the `faulty_*` cases wrap an already wrapped store. | 60 test instances (up to 97), one dev-dependency | safe | §2 B |
| 6 | `FaultyStore` does not forward `stage_objects`/`relabel` (trait defaults answer "unsupported") although its doc says every call is forwarded **(re-verified)**. Eight test files therefore hand-write their own `impl VersionedStore`; seven have the same gap. | about 630 lines of doubles for about 110 added | needs-care | §1.2, §5.3 |
| 7 | `Error(pub &'static str)` in arkiv-node: 1,000+ construction sites, 107 distinct codes, 305 `map_err(\|_\| ..)`, HTTP status chosen by string match. Consequences found: a dead arm, a wrong code, status decided from message text, side-channel fields to carry detail, five spellings of one error. | clarity; removes about 150 lines of tables and side channels | needs-care `[CONSENSUS]` (wire strings must stay byte-identical) | §5.1 |
| 8 | Three node generations with inverted layering: production `signed` imports `demo::workload::genesis` and `demo::wire::hex`; about 330 lines of `signed/runtime.rs` restate `simulator/`; hex/quantity parsing exists in about 20 places with different strictness. | about 600 lines | needs-care | §1.5, §1.6 |
| 9 | `signed/host.rs`: the height-zero setup is written three times (197-237, 294-336, 386-427) **(re-verified)**, the 16-field `Host` literal three times, four `too_many_arguments` constructors, pub fields set after construction. | about 120 lines, 4 allows | needs-care | §1.5 |
| 10 | `bin/arkiv-proposer.rs`: `run()` (604-1307) is 664 code lines; the help text sits on lines of 3,633 and 538 characters (`:615`, `:618`; `arkiv-simulator.rs` has one of 1,155); one flat 36-flag whitelist accepts e.g. `keygen --peer x`; 123 `Error("..")` and 61 `map_err(\|_\| ..)`; filesystem helpers are copied across five genesis files. | about 300 lines, large readability gain | needs-care (help text and error strings are asserted by tests) | §4.3, §1.7 |
| 11 | Formatting the newer arkiv-node files: rustfmt gave-up regions, 74 src lines over 150 chars, about 810 item boundaries with no blank line, one-letter bindings, a 319-line `dispatch` match, a 263-line `render`. | readability; adds lines | safe (`[CONSENSUS]` care inside `json!` bodies) | §4.3 |
| 12 | Stale and narrating comments: about 30 verified stale comments in code, 7 broken rustdoc links, 399 banner lines, about 55 milestone-label lines, 70 lines citing a "brief" that is not in the repo. | about 1,500 comment lines | safe | §3 |
| 13 | Docs: 61 numbered findings, about 43 of them direct contradictions with the code; 54 files / 15,899 lines can become about 31 / 9,700; README 432 to about 200. | about 6,400 lines out of the live tree | safe | §4.6 |
| 14 | Scripts and build: a 2,446-line prefill pipeline emitting a row codec the node no longer accepts; `sdk-compat-smoke.{py,ts}` documented as "expected to fail"; 8 smoke launchers with the same harness; `bench-compare.sh` defaulting to a temporary session directory on one machine **(re-verified)**; `Dockerfile.simulator` cannot build **(re-verified by reading)**. | up to about 4,400 script lines | safe | §4.7 |
| 15 | Consensus logic written twice in arkiv-node: due-schedule maintenance, expiry resolution (the copies differ at `u64::MAX`), account debit/burn, prefill row rules, a second transaction executor used only by tests; and tests that re-implement the binary's install sequence and have drifted. | about 200 lines; removes drift risk | needs-care `[CONSENSUS]` | §1.8 |

Also worth knowing before starting: eight small real defects surfaced during the read (§5.2), and `simulator/query_language/` is ported GPL-3.0-or-later code in a workspace that declares no licence (§5.8).

## Totals (rough)

| What | Removable | Notes |
|---|---|---|
| Tests by count | about 86 named test fns + 60 generated instances = about 146 of 1,289 (11%) | 36 outright deletions, about 50 folded away by merges and tables, 60 duplicate contract instances. Optional on owner decisions: +10 (KV facade), +12 (imported query-language tests, only if upstream parity is dropped), +8 (`demo_auth.rs`, only if the demo goes), +37 more contract instances. About 30 over-long tests should be *split*, which adds roughly 40 small tests back. |
| Test bodies (category 2) | about 4,500 lines | T3 2,300; T5 625; T1 525; T4 215; S3 200; S1 115; N4 110; NT1 110; NT2 95; S5 90; N1 70; others 110 |
| Test scaffolding (category 1, tests) | about 6,600 lines | T2 1,050 (+350 with care); T4 850; T3 800; NT2 745; T1 650; T5 580; NT3 470; S4 360; NT1 330; S5 315; unit-test fixtures in N1/N3/N4 490 |
| Source duplication (category 1, src) | about 2,900 lines | S1 770; N2 520; S2 490; N3 370; N1 285; N4 190; S3 175; S5 135 |
| Comments (category 3) | about 1,500 lines | 399 banner lines, about 450 of module-doc templates, the rest restating or historical |
| Dead code and sloppiness (category 4) | about 1,900 lines | `persistent.rs` 691; T3 375; N3 130; N2 110; T5 100; others |
| **Rust total** | **about 15,000 to 17,000 of about 150,000 lines (10-11%)** | categories overlap slightly, hence the range |
| Owner decisions on top | about 4,000 lines | demo profile 2,135 (src + bin + test); KV facade about 850; two redb examples 517; `vendor/reth-trie` 243; three test-only durable entry points about 275; `KeyWriter`/`KeyReader` 107 |
| Scripts | about 700 duplicated + up to 3,700 obsolete of 10,480 | |
| Docs | about 6,150 of 15,899 lines leave the live tree, + 230 from README | 1,100 deleted, 2,650 archived, 1,400 moved to runs/benchmarks/proposals, 1,000 trimmed |

## Do not touch casually `[CONSENSUS]`

These findings are refactors that must leave bytes, charges and responses identical, or they are behaviour changes that need a deliberate decision. Each is tagged in Part II.

- **Canonical encoding and tags**: any `codec.rs` macro for fieldless enums or primitives [S1 §1.3]; `block.rs` codec macro [N4]; `block.rs:306` stale `48` (replace with `Limits::MIN_ENCODED_LEN`, a lower bound only) [N4]; `MAX_ADMIN_BYTES` must stay 309.
- **Metering and receipts**: the `View` refactor of engine read plumbing [S1 §1.2]; shared proof-generator helpers, where charge order must not move [S2 D1.5, D1.6]; every test merge that moves a hand-counted vector (items 2 and the table-drives) must move the literal, not re-derive it.
- **State roots and genesis**: `demo::workload::genesis` feeds `signed::protocol::default_native_genesis`, so moving it must not change one field [N2]; `docs/examples/genesis-bundle/profiles/fixed-glm-v2.toml` is compiled into the binary as the current preset (`genesis_authoring.rs:49`), so editing that "example" changes genesis hashes [D1]; `toml = "=0.8.23"` is pinned because its output is hashed into genesis locks [NT3]; prefill row rules written twice [N3].
- **Consensus rules written twice**: due schedule, expiry resolution, debit/burn [N4]. Unify only with the replay and block-model tests green and the `u64::MAX` difference understood first.
- **RPC/HTTP contract**: error code strings (the enum must render the same text); making `uint` strict (`"0x+1"` is accepted today); `ethereum.rs:160` error code; unifying `NonpayableCall`/`NonzeroArkivValue` and `UnsupportedContractCall`/`UnsupportedEthereumCall`; WS vs HTTP batch alignment; any reformatting inside `json!` bodies; `state_docs.rs` scrapes the text of `fn dispatch(`, so method literals must stay in that function [N1].
- **Interfaces that look like debug output**: stderr JSON events `prefill_progress`, `prefill_bulk_phase`, `prefill_prebuilt_install`, `prefill_genesis_adopt` are parsed by scripts and asserted by tests; about 45 test assertions match genesis error text; the help text is asserted [N3].
- **On-disk format**: `durable/format.rs` sizes `256`/`348`, the four height-zero publication paths, the version list [S2 D2, D8, D9, D12, S10].
- **Never delete**: `tests/block_format.rs`, `tests/codec.rs` goldens, `trace_regression.rs` (regression tests for a real bug), all crash/recovery/fault tests in arkiv-db-redb and `durable_failures.rs`, proof tamper tests, `block_model.rs` (its charge-table half and independent state root are real differential evidence), all 12 `gas.rs` tests (they pin rent numbers).

## Suggested order of work

1. Pure deletions with no dependants: `persistent.rs`, `fallible_api.rs`, the duplicate contract modules, dead `pub` items, dead `#[allow]`s, debug prints, banners, stale comments, broken doc links, unused scripts, `vendor/reth-trie`. All **safe**.
2. Fix the eight defects in §5.2 (each small).
3. `FaultyStore` extension, then delete the hand-written store doubles.
4. arkiv-db `tests/support` (vocabulary first, then reader trait, then model/oracle), ideally together with collapsing each crate's integration tests into one binary. Then the `records.rs`/`queries.rs` merges into `query_metering.rs`, test by test.
5. arkiv-node: neutral `hex.rs`/`wire.rs`/`error.rs`, error enum, `Launch` struct for `signed::Host`, proposer split, then reformat.
6. Docs and README last, once names and module paths have settled.

---

# Part I by category

## 1. Duplication (curated, by payoff)

### 1.1 arkiv-db integration tests: helpers, models and oracles re-typed per file (about 3,900 lines) - needs-care

- **Helper vocabulary.** Measured over `crates/arkiv-db/tests`: `fn ns`/`ns_id` in 23 files (5 distinct bodies: `from_bytes` vs `from_ordinal`), `fn receipt` in 12 (6 distinct signatures), `traced` 8 (3-tuple slice vs 4-tuple slice vs built `Ledger`), `put` 10, `delete` 13 (raw delete in some files, record delete in others), `entry` 8, `create` 14, `attr`/`field` 8, `descriptor` 7, `read_receipt` 7, owner constants in 22 files (`CAROL` is `0xca` in one file and `0xc0` in another). The same name meaning different things per file (`logical`, `value`, `publish`, `put`, `key`) is itself a finding [T4 §1.1].
- **Oracle harness copied.** `history.rs` to `query_history.rs` (`run_seed` 173 common lines, `publish_admin` 55, `verify_history` 49, `Flat`/`flat_write`/`flat_read`, `Ctx`, `seed_tests!`, `Generator` verbatim) and `isolation.rs` to `query_isolation.rs` [T2 §1.1, §1.2]. The xorshift `Generator` is also verbatim in `metering.rs:1668-1676` and `model.rs:71-83`.
- **Reference model copied.** The `BTreeMap` record/query model exists at `records.rs:336`, `terms.rs:277`, `queries.rs:144`, `pagination.rs:449` and in `query_metering.rs`, `query_history.rs`, `query_isolation.rs`; `records.rs` and `terms.rs` share about 180 byte-identical helper lines [T3 §1.1, §1.2].
- **`trait Reader` copied.** `Branch` and `Snapshot` expose the same read methods but share no trait, so about 10 test files hand-write a delegating trait (354 lines in T2's four files, 145 in T4's three, three more in T3) and `host_state.rs:544-645` duplicates a whole function per receiver. Fix in the library: a `pub trait` (or the `View` of §1.4) implemented by both [T4 §5.1].
- **Price table copied.** The FIXTURE charge table is transcribed three times as code and again as prose in module docs [T2 §1.4, T4 §1.9].
- **Single home**: `crates/arkiv-db/tests/support/{vocab,reader,oracle,typed_model,isolation}.rs`. The design, with every helper's exact signature, line range and per-file difference, is in [T2 §1.7], [T1 §1.1], [T3 §1.3], [T4 §1.1], [T5 §1.2].
- Do this together with **one integration-test binary per crate** (`tests/it/main.rs` with modules). Today there are 98 test binaries (arkiv-db 40, arkiv-node 27, arkiv-db-redb 18, arkiv-db-trie 12, arkiv-db-memory 1); each recompiles its support tree, which is why five files need `#![allow(dead_code)]` and why helpers get copied instead of shared. In arkiv-db-redb, 18 binaries each compile the 1,579-line `common` tree and use at most one of its seven submodules [S4].

### 1.2 Store test doubles: eight hand-written `impl VersionedStore` and five `ObjectReader` doubles (about 630 lines) - needs-care

- `VersionedStore` wrappers: `crates/arkiv-db/tests/durable_failures.rs:420` and `:1284`, `adopt.rs:359`, `streaming_import.rs:308`, `simulator_composition.rs:324`, `crates/arkiv-db-redb/tests/engine_process.rs:52`, `crates/arkiv-node/tests/signed_reads.rs:104` (49 lines of pure forwarding plus a 10 ms delay), `crates/arkiv-db/src/durable/adopt.rs:656` (inside `#[cfg(test)]`).
- `ObjectReader` doubles: byte-identical at `eq_proofs.rs:818-836` and `range_proofs.rs:605-623`; again at `rehash.rs:82-101`, `storage_faults.rs:72-149`, `src/engine.rs:3005`.
- **Single home**: `crates/arkiv-db-storage/src/testing/faulty.rs`. It needs: forwarding of `stage_objects`/`relabel` plus faults for them, read overrides (hide or flip one object), three hooks (read, recover, delay), a deny-unbounded flag and a skip count [S3 W2], [T5 §1.1], [S2 D15]. Five of the hand-written doubles implement the bounded read by ignoring the limit, which the trait doc forbids [S3 D2].

### 1.3 arkiv-node tests (about 1,500 lines) - needs-care

- `signed_support::{signer, genesis}` exists, yet `signer` is defined 11 times and `genesis` 9 times: `signed_http.rs:55-75`, `signed_retention.rs:27-47`, `signed_pool.rs:11-29`, `signed_protocol.rs:16-37` (keep: pinned by golden vectors), `signed_reads.rs:139-141`, `accounts.rs:21`, `gas_execution.rs:18`, `genesis_prefill.rs:22`, `genesis_adopt.rs:57`, `prebuilt_genesis_cli.rs:41`; `ethereum_transactions.rs` declares `mod signed_support` and still inlines `Signer::from_bytes([N; 32])` 8 times and builds its chain three times with two different proposers [NT2 §1.1].
- 8 host-construction sites, 8 signed-tx builders, 4 copies of `Server`/`serve`, 3 JSON-RPC wrappers [NT2 §1.2-1.3]. Home: `signed_support/mod.rs` plus a stack-neutral `support/http.rs`.
- CLI harness (temp-dir guard, free-port probe, kill-on-drop child, readiness poll, HTTP probe, `Producer`, refused-args runner) in `genesis_adopt.rs`, `prebuilt_genesis_cli.rs`, `retention_cli.rs`, `simulator_http.rs`. Home: `tests/cli_support/mod.rs`, about 245 lines net [NT3 D1].
- Prefill fixture and `ARKPFL02` framing hand-rolled five times (three integration tests, two unit-test modules); the library has only the reader. Add a writer beside `StreamReader` [NT3 D2].
- `gas_execution.rs`: four hand-rolled `SignedGenesis::new` rebuilds and four copies of the signing envelope (about 125 lines); the `Receipt` literal is written in `replay.rs`, `block_format.rs`, `block_model.rs`, `exec.rs`; `until` is copied verbatim at `block_model.rs:1746` [NT1 §1].

### 1.4 `engine.rs`: 30 infallible twins and positional read plumbing (about 300 lines) - needs-care `[CONSENSUS]`

- 30 methods whose whole body is `in_memory(self.try_x(args))` (`engine.rs:166-171`): Snapshot 14, Branch 16, about 232 lines with their "Fallible form of" docs. No non-test `src/` code calls them; tests, examples and conformance do, on about 1,127 call lines. A two-arm `macro_rules!` generating each pair keeps the method paths that `crates/arkiv-node/clippy.toml` bans (about -200 lines). Cheaper alternative: move the 30 wrappers into one trailing `impl` block so the main impls read as the real API [S1 §1.1].
- The 14 `try_` read methods re-list the same arguments (224 lines) into nine `read_*` fns that all take `(state, nodes, pricing, ..)`. A `struct View<'a> { state, nodes, pricing, limits }` removes about 100 lines and the allow at `engine.rs:2786` [S1 §1.2].

### 1.5 arkiv-node runtimes: three generations (about 520 lines) - needs-care

- `signed/host.rs:197-237`, `294-336`, `386-427`: certificate/signer check, `SimulatorIdentity`, identity, genesis `SignedBlock`, `history.persist`, `HeaderRecord`, `SimulatorPacket`, three times; the `Initialization` match at 256-260 and 344-348 (and `simulator/host.rs:333-336`); the 16-field `Host` literal at 262-279, 349-366, 524-541. Fix: `struct Launch` + `enum Prefill { None, Small(Vec<u8>), Staged{..} }` and one `fn genesis_packet(..)`; this also removes the pub-field writes (`host.prefill_file = ..`) in the proposer [N2].
- About 330 lines of `signed/runtime.rs` restate `simulator/runtime.rs` and `simulator/light.rs` (response mapping, fetch, body parse, auth, text query, continuation cache, entity JSON). `try_acquire_owned` + `ServerBusy` appears 7 times (`simulator/runtime.rs:372-375`, `398-401`, `480-483`; `signed/runtime.rs:617-620`, `680-683`, `740-743`, `946-949`); the freshness predicate 4 times (`signed/light.rs:426-428`, `462-464`, `simulator/light.rs:413`, `signed/runtime.rs:924`).
- `demo/runtime.rs:99-117` vs `138-156`: the 17-field `Node` literal twice; `unpack_hash`, `proof_limits`, `proof_error` copied into `simulator/` [N2].
- **Single homes** (neutral modules, so `signed` stops importing from `demo`): `wire.rs` (hex, number, pack/unpack, caps), `dev_genesis.rs` (`demo::workload::genesis`, `[CONSENSUS]`), `error.rs`, `http.rs` (response mapping, body parse, permits), `query/`, `feed.rs` [N2, "Answer to the key question"].

### 1.6 Hex, quantity and number parsing: six jobs, about twenty implementations - needs-care / behaviour-affecting

Full table in [N1 D1]. The real `hex` is `src/demo/wire.rs:14-23`; `simulator/wire.rs:13-15` delegates to it and `signed/rpc.rs:122-124` delegates to the delegate. Decoders: `demo/wire.rs:24-43`, `value_json.rs:124-139`, `signed/rpc.rs:135-149`, `genesis_authoring.rs:334-351`, `query_language/literal.rs:336`, plus inline copies and four test copies. The copies disagree: `rpc.rs:128` `uint` accepts `"0x+1"` **(re-verified: `u64::from_str_radix` accepts a leading `+`)** while `rpc_reads.rs:486` `quantity` is strict; `rpc::quantity` formats and `rpc_reads::quantity` parses. **Single home**: `crates/arkiv-node/src/hex.rs` (`alloy_primitives::hex` is already a dependency). Merging is needs-care; making `uint` strict is behaviour-affecting `[CONSENSUS]`.

### 1.7 Genesis files and binaries (about 370 src lines + 190 in unit-test fixtures) - needs-care

- `write_new`, `sync_dir`, `read_bounded`, `parent`, `rename_noreplace` are copied across `genesis_bulk.rs`, `genesis_prebuilt.rs`, `genesis_streaming.rs`, `genesis_adopt.rs` and the proposer (about 90 lines). The proposer has three "write temp, fsync, hard_link, remove, fsync dir" sequences (`write_new` 81-115, `local_prefill` 238-254, `download_prefill` 493-506). One `link_into_place(temp, target)` in a small `fsutil` module [N3].
- Four hand-rolled argument parsers across three binaries; six `X` / `X_with(.., permanent)` pairs whose short form only tests call; launch assembly and report JSON written twice; `max_cells = 64` hard-coded although `signed::protocol::MAX_CELLS` exists [N3].

### 1.8 Consensus logic written twice - needs-care `[CONSENSUS]`

- Due-schedule maintenance: `arkiv/mod.rs:635-654` vs `exec.rs:748-791`, with different error mapping. Expiry resolution: `arkiv/mod.rs:341-353` vs `exec.rs:368-386`, with a subtle difference at `u64::MAX`. Debit and burn: `accounts.rs:118-127` == `144-153`. `arkiv::execute_batch` (832-872) is a second transaction executor used only by tests [N4].
- `tests/genesis_adopt.rs:153-241` `install_adopt`/`install_copy` re-implement `adopt_host`/`create_host` from `src/bin/arkiv-proposer.rs`, so the "library" tests exercise a copy that has already drifted. Move the sequencing into `arkiv_node::genesis_adopt` and call it from both [NT3 D3].
- `block_model.rs`: the expiry and reservation half of the oracle is a statement-for-statement transcription of `src/exec.rs:368-386`, `admit_expiry` and `src/params.rs:163-213`; `oracle_reservation_matches_the_parameters` (`block_model.rs:2053`) calls no production code [NT1].

### 1.9 Proofs and the durable engine (about 490 lines) - needs-care, generator parts `[CONSENSUS]`

- `proof.rs` and `proof/range.rs` repeat nine blocks between the two verifiers and the two generators (row verification, row emission, posting rebuild, witness loop; the `NamespaceRoots` literal four times including `engine.rs` and `explore.rs`). Shared helpers remove 150 to 170 lines and bring the four clippy-flagged functions (`proof.rs:606`, `:887`, `proof/range.rs:453`, `:644`) under 100 lines [S2 D1].
- Four height-zero publication paths (`durable/mod.rs`, `bulk.rs`, `streaming.rs`, `adopt.rs`) hand-build the same manifest, batch, metadata checks and `InitializationPending` (about 75 lines); three state walkers with four copies of the same error newtype; two object-batch flushers that disagree on per-object overhead (64 vs 32) [S2 D2-D5].

### 1.10 Smaller, still worth doing

| Where | What | Fix | Lines | Risk |
|---|---|---|---|---|
| `crates/arkiv-db/src/codec.rs` 897-918, 1135-1209, 1320-1341 + const tables | four fieldless enums write every tag three times; five primitive impls | `unit_enum_codec!` / `primitive_codec!` | -150 | needs-care `[CONSENSUS]` |
| `typed.rs` (`terms`, `open_run`), `typed/planner.rs` (`walk_run`) | term-run walk three times; `plan()` 174 lines with five near-identical arms and a dedup closure pasted three times | one `walk_run` iterator; one arm helper | -125 | needs-care `[CONSENSUS]` |
| `crates/arkiv-db-storage/src/versioned.rs:771-873` | identical `Arc<T>` / `Box<T>` forwarding impls | one macro | -48 | safe |
| arkiv-db-memory vs arkiv-db-redb | `read_object`/`read_record` should be provided trait methods; `copy_bounded` three times; result-to-outcome-to-stats block five times (memory does not count a refused `stage_objects`, redb does) | helpers in arkiv-db-storage | -110 | needs-care |
| `crates/arkiv-db-trie/tests/interval_{model,inspection,security}.rs` 6-49 / 7-50 / 4-47 | same 44-line shim three times; unused in the third | move to `tests/support` | -115 | safe |
| `crates/arkiv-db-trie` | `stage_link` (`src/map.rs:851-887`) == `try_stage_link` (`src/stage.rs:185-221`); trace-node construction `proof.rs:354-388` / `interval.rs:449-479`; `KeyRange::contains` == `Nibbled::contains` (`interval.rs:81-91` / `143-153`); seal loop `sorted.rs:171-178` / `211-217` | one fn each | -135 | needs-care `[CONSENSUS]` for the verifier parts |
| `crates/arkiv-db-redb/tests` | fault-injection sweep x4, parent image x7, chain-building loop x11, `chain()` twice, five equal config helpers; `common::commit` takes the batch by value (39 `.clone()`); `examples/redb_experiment.rs` hand-copies four test bodies | helpers in `tests/common`; `&CommitBatch` | -360 | safe |
| `crates/arkiv-node/src/exec.rs:425-560` | `execute_block_hooked` and `build_block_hooked` are identical 9-argument forwards differing in `Mode` | `Mode` + `struct BlockEnv { spec, parent, height, timestamp_ms }` | -58, 4 allows | needs-care `[CONSENSUS]` |
| `arkiv/schema.rs:131-177` vs `value_json.rs:45-93` | decimal parse/format twice | one module | -40 | needs-care `[CONSENSUS]` |
| `signed/metrics.rs:215-478` | 31 seven-line `gauge(`/`counter(` calls, then `group_families` re-parses its own output with unwraps | collect families in a map | -95 | needs-care (exporter output) |
| scripts | 8 smoke launchers (2,074 lines) re-implement free-port, REST+retry, wait, cli, start, stop, sync; `udc-sdk-smoke.py` is about 245 of 274 lines identical to `sdk-compat-smoke.py` | `scripts/lib/node.py`, modelled on `Cluster` in `full-light-demo.py` | -450 to -500 | safe |
| cross-crate `#[path]` | `arkiv-db/tests/{roots,eq_proofs,storage_faults}.rs` include `../../arkiv-db-trie/tests/support/mod.rs`; `arkiv-db-redb/tests/engine_history.rs` includes `../../arkiv-db/tests/support/durable_history.rs`; redb examples include `tests/common` | a `testing` feature on the owning crate, or one dev-only `arkiv-db-testkit` crate | structural | needs-care |

## 2. Unneeded / obvious tests (curated)

The suite is strong; very few tests assert trivial things. What can go falls into four groups. Every deletion below names where the same fact stays pinned; details and the per-vector mapping are in Part II.

### A. The same hand-counted vector pinned in two files - needs-care `[CONSENSUS]` (about 1,800 lines)

`records.rs` and `query_metering.rs` build the identical a/b/c fixture and pin the same receipts and ledgers (379/229/257, 132, 183, 106, 54/8/7/20/23); `query_metering.rs` checks them under every budget, traced and untraced **(re-verified on "create a": `records.rs:877-933` vs `query_metering.rs:1029-1050` and `1117-1139`, same derivation, same ledger)**. Keep one copy. The natural survivor is `query_metering.rs`, whose harness is strictly stronger; if the owner prefers `records.rs` as the readable worked-examples file, cut the matching vectors from `query_metering.rs` instead. Either way, move literals test by test and never re-derive them.

| Test (`crates/arkiv-db/tests/`) | Verdict | Stays pinned in |
|---|---|---|
| `records.rs:877` `create_with_every_term_new_charges_lookup_record_group_then_each_term` | DELETE | `qm:1029-1050`, `1117-1139` |
| `records.rs:936` `create_with_existing_terms_charges_creation_only_for_the_new_one` | DELETE | `qm:1052-1115` |
| `records.rs:1089` `patch_changing_the_type_replaces_both_terms_and_removes_are_charged_per_name` | DELETE | `qm:1706-1770` |
| `records.rs:1316` `delete_charges_lookup_delete_group_then_each_held_term` | DELETE | `qm:2067-2162` |
| `records.rs:1391` `already_exists_is_charged_after_the_key_lookup_and_recorded` | DELETE | `qm:1548-1573`, `qm:4029` |
| `records.rs:1422` `not_found_is_charged_after_the_key_lookup_for_patch_and_delete` | DELETE | `qm:1935-1957`, `2176-2213` |
| `records.rs:1847` `unauthorized_and_unknown_namespace_are_charged_the_directory_lookup` | DELETE | `qm:2216-2267` |
| `records.rs:1005`, `1153`, `1203`, `1252`, `1458`, `2064`, `2136` | MERGE the dozen distinct vectors into the named `qm` tests, delete the shells | [T3 §2.1] |
| `records.rs:2432` `get_record_projections_charge_only_the_projected_bytes` | TIGHTEN to the `ProjectionTooLarge` part (2513-2536) | `qm:2274-2363` |
| `queries.rs:2213`, `2343`, `2400`, `2436`, `2526`, `2565`, `2636` (7 cost tests, 617 lines, second fixture) | MERGE the vectors that add something into `qm` section 5, delete the section and its fixture | [T3 §2.2] |
| `queries.rs:1939` `a_cursor_is_bound_to_its_context_query_and_projection` (153 lines) | DELETE, subset of the `pagination.rs` cursor tests | [T3 §2.4] |
| `pagination.rs:1136`, `1294` | shrink: the same two charge rules a third time | [T3 §2.6] |

### B. Tests that cannot fail, or run twice - safe

| What | Verdict | Why |
|---|---|---|
| `crates/arkiv-db/tests/fallible_api.rs` (3 tests, 465 lines) | DELETE, or keep a 30-line smoke; move the one `StorageError` type assertion to arkiv-db-storage | every infallible method is `in_memory(self.try_x(..))`, so the file compares a function with itself [T5 §2.1] |
| `crates/arkiv-db-redb/tests/contract.rs:98-125`, `140-146`, import at `:20`, doc lines 9-10 (`Memory`, `WrappedMemory`, `mod memory`, `mod faulty_memory`) | DELETE (60 generated tests) | the same two suites run in `crates/arkiv-db-memory/tests/versioned_contract.rs`; only the object-id hash differs and the memory store never verifies ids. Drops the crate's only use of the `arkiv-db-memory` dev-dependency [S4], [S3 T1] |
| contract case `faulty_without_faults_is_transparent` and the 11 `faulty_*` cases inside the `faulty_memory` / `faulty_redb` modules | DELETE the case; run `faulty_*` cases only in the unwrapped modules | 33 instances test a `FaultyStore` wrapped in a `FaultyStore`; 4 `reopen_preserves_state` instances are vacuous on volatile stores [S3 T1-T2] |
| five of six unit tests in `crates/arkiv-db-memory/src/versioned.rs` (`identical_retry_is_never_applied_twice`, `identity_is_fixed_and_health_never_fails`: DELETE; `objects_are_put_if_absent_per_space`, `records_are_insert_only`, `head_is_compare_and_set`: move 5 assertion lines into their contract namesakes) | DELETE / MERGE | duplicate contract cases already run against the same store [S3] |
| `crates/arkiv-db-storage` `random_database_ids_differ` | DELETE | tests the RNG [S3] |
| `crates/arkiv-node/tests/block_model.rs:2053` `oracle_reservation_matches_the_parameters` | rewrite as a differential check against `params` | calls no production code [NT1] |
| `crates/arkiv-db/tests/history.rs` replay tests' event comparison | TIGHTEN | only the physical-trace comparison can fail [T2 §2] |
| `crates/arkiv-db/tests/queries.rs:1252-1264` | fix or DELETE | queries cell `n` where the fixture has `k`; the page is always empty, so the assertion is vacuous [T3 §2.15] |
| `crates/arkiv-db/tests/terms.rs:2271`, `signed_protocol.rs:105-108`, `scalar_profile.rs:7`, `10-12` | DELETE the assertions | a constant asserted against its own definition, or pinned again elsewhere |
| tautologies: `.all(kind == Write)` after whole-receipt equality (`metering.rs:979-984`, `2044-2048`, `2062-2064`, `2081-2086`, `qm:3849-3854`); `qm:4720-4748` compares `walked` with itself | DELETE the assertions (about 34 lines) | [T1 §2] |
| KV facade tests: `kv.rs` (3), `semantics.rs` (4), `backend.rs` (2), `model.rs` (1), `examples/basic.rs` | owner decision; if the facade stays: DELETE `consecutive_block_packets_..`, `returned_buffers_..`, `block_packets_own_inputs_..`, merge the two prefix-scan tests | they assert ownership facts the type system guarantees; no production code uses the facade [T5 §2.2] |

### C. Families that should be one table - safe (vectors preserved)

| Tests | Into | Lines |
|---|---|---|
| `terms.rs:1355`, `1433`, `1506`, `1576` (25 near-identical read-and-assert blocks) | one `(name, tag, lower, limit, values, cost)` table | -200 |
| `metering.rs:345-437` six one-call write-vector tests (+ first half of 517-527) | `raw_write_vectors_under_every_budget` | -45 |
| `query_metering.rs`: ten "seek only" query vectors (2540-3031, 186 lines) | one loop over `(name, query)` | -115 |
| `query_metering.rs:1547`, `1934`, `2175`: rejected after the binding lookup | one table over `(name, setup, op, reason)` | -55 |
| `codec.rs:1282-1304` four `*_domain_panics` | `invalid_domains_panic` with `catch_unwind` | -13 |
| `range_proofs.rs:165-222` + `977-1021` per-type range tests | one test over a shared `ordered_samples()` | -60 |
| `crates/arkiv-node/tests/exec.rs:85-133` three due-schedule tests | one table | -30 |
| `signed_http.rs:963` + `:1022` | one helper | -40 |
| `crates/arkiv-node/src/signed/rpc_reads.rs` `engine_rejections_map_onto_the_sdk_table` | make it the table it describes | -15 |

### D. Subsets and trivial tests to delete by name - safe unless noted

| Test | Reason |
|---|---|
| `metering.rs:991` `exact_budget_succeeds_while_one_short_and_zero_fail` | every receipt is a row of the six every-budget sweeps at 346-437 (needs-care) |
| `metering.rs:743` `transfer_one_unit_short_spends_only_the_lookup_and_changes_nothing` | budget 22 is a row of the sweep at 405-419 (needs-care) |
| `metering.rs:1568`, `616`; `scalar_profile.rs:95`, `248`, `311` | MERGE into `estimate_has_no_side_effects`, `tracing_never_changes_charges`, `golem_types.rs`, `cell_limits.rs`, `durable.rs` + `eq_proofs.rs` [T1 §2] |
| `codec.rs:1306` `values_round_trip_through_shared_storage` | subset of `type_tags_and_values` (`codec.rs:446`); its comment promises a buffer-identity check that is not made **(re-verified)** |
| `codec.rs:1240` `keccak_is_the_trusted_primitive` | tests alloy's keccak; fold its vector into the digest test at 1249 |
| `ordered_ranges.rs:247` `every_indexable_type_is_ordered` | identical to `golem_types.rs:181-183`; both restate `is_ordered() { self.is_indexable() }` |
| `host_state.rs:121` `a_fresh_state_has_no_host_entries` | asserted again at 342, 736-740, 791 |
| `range_proofs.rs:929` `range_scaling_experiment` | `#[ignore]`d benchmark that prints timings: move to `examples/` or delete |
| `records.rs:669` | duplicate of a unit test in `src/record.rs` |
| `records.rs:822` `cells_carry_a_kind_and_a_value` | constructor/getter |
| `src/record.rs:993-1083` three unit tests, `src/codec.rs` one | strict subsets of named tests in `tests/records.rs` / `tests/codec.rs` [S1 §2] |
| `crates/arkiv-node/tests/identity.rs:148` `in_flight_identities_stay_pending` | subset |
| `crates/arkiv-node/tests/genesis.rs:318` `the_spec_exposes_the_genesis` | getter |
| `crates/arkiv-node/tests/replay.rs:1390` `headers_carry_the_format_version_and_the_state_root` | subset of `block_format.rs` |
| `crates/arkiv-node/src/arkiv/tests.rs` `scalar_types_stay_distinct`, `natural_storage_is_not_abi_or_signed_order_encoding` | restate type definitions; `sdk_abi_and_tombstone_validation` merges into two neighbours [N4] |
| `crates/arkiv-node/src/signed/rpc.rs` `estimate_has_no_nonce_balance_or_state_effect` | subset [N1] |
| `crates/arkiv-db-redb/tests` `a_killed_store_releases_its_file_lock` | optional: subsumed by a durability test [S4] |

### E. Tests that never run, or that hang instead of failing - needs-care

- `crates/arkiv-node/examples/verify_inspection.rs:114-185`: a 72-line `#[cfg(test)]` module in an example; examples default to `test = false`, so it never runs [NT3 S1].
- `crates/arkiv-db/examples/records.rs`: 67 hand-derived cost assertions that docs cite as evidence, but CI only compiles examples; five literals (383, 261, 297, 1174, 245) appear in no test [T5].
- `tests/genesis_adopt.rs:722`, `tests/retention_cli.rs:13`: refusal tests use `Command::output()` on command lines that would start serving if the guard regressed; `prebuilt_genesis_cli.rs:416-446` already has the deadline version [NT3].
- `tests/signed_reads.rs`: absolute wall-clock bounds at `:494`, `:523`, `:569-572`, `:576`, `:613` and three fixed 200 ms sleeps; most HTTP tests use a client with no timeout [NT2].
- `src/signed/state_docs.rs:58-70`, `118-135`: tests that `include_str!` `runtime.rs`/`rpc.rs` and split the source text on `path=="` and `fn dispatch(` to discover routes **(re-verified)**. They break on reformatting. A route table shared by dispatcher and test is the fix.

### What to keep (looked redundant, is not)

All of `trace_regression.rs`; the block-budget pairs in `metering.rs`/`query_metering.rs`; the literal traced ledgers in `query_metering.rs` (the only hand-written check of first-charge ordering); `validation.rs`, `identity.rs`, `selection.rs` (already table-driven); `signed_history.rs` (22) and `signed_pool.rs` (9), nothing trivial; all 11 `durable/cache.rs` unit tests; all crash/recovery/durability tests; host-level and HTTP-level versions of the same signed scenario (two layers, keep both); the 58 imported query-language tests (keep diffable against upstream). One gap worth *adding*: range proofs have no literal wire vector, equality proofs have three [T4 §5.3].

## 3. Comments (curated)

### 3.1 Stale: the comment contradicts the code - safe, fix each

| Where | Says | Code |
|---|---|---|
| `crates/arkiv-db/src/engine.rs:32` | "state format 1" | `layout.rs:1`: state format 2 **(re-verified)** |
| `engine.rs:69`, `:484`, `:860`; `state.rs:182` | "a test aid until node step N3c adds durable manifests" | the durable engine exists |
| `engine.rs:80-81` | "Durability is not implemented: nothing here survives process termination" | `durable/` **(re-verified)** |
| `meter.rs:24`, `:162`, `:165` | `ProofNode`/`ProofByte` "reserved ... not charged yet" | charged at `proof.rs:817`, `:852-853`, `proof/range.rs:704-714` **(re-verified)** |
| `codec.rs:794` | links `RecordKey::encoded` | no such item (rustdoc warning) |
| `layout.rs:10`, `:11`, `:154` | links `order::encode_bytes`, `order::encode_shifted` | unresolved (rustdoc warning) |
| `layout.rs:39`, `:1085` | 128 and 64 bytes | the check is 512 |
| `durable/adopt.rs:498`, `:510` | links `initialize_simulator_from_staged` | no such item (rustdoc warning) |
| `crates/arkiv-db/src/lib.rs` crate docs | four statements | contradict the code [S2 C1] |
| `crates/arkiv-db-storage/src/versioned.rs` module doc | "this contract has no deletion" | `relabel` retires records [S3 C1] |
| `crates/arkiv-db-storage/src/testing/faulty.rs:159-160` | "every call is forwarded unchanged" | `stage_objects`/`relabel` are not |
| `crates/arkiv-db-memory/src/persistent.rs` docs | "the engine's snapshot primitive" | unused |
| `crates/arkiv-node/src/lib.rs:7` | "block format version 1" | `FORMAT_VERSION = 5` |
| `crates/arkiv-node/src/identity.rs:6` | `arkiv/tx/v1` | v2 |
| `crates/arkiv-node/src/replay.rs:249` | "protocol-2" | protocol 5 |
| `crates/arkiv-node/src/signed/runtime.rs:78-82` | admission "tries once and answers ServerBusy" | it polls via `read_node()` |
| `crates/arkiv-node/src/signed/history.rs:137-138` | doc sits on the wrong function | |
| genesis module docs | "protocol 4", "Version-two" | [N3 Comments 4] |
| `crates/arkiv-node/tests/block_format.rs:1` | "format version 1" | 5 |
| `crates/arkiv-node/tests/block_model.rs:86-90` | doc on `SPILL_WINDOW` describes `PARAMS` | |
| `crates/arkiv-node/tests/arkiv_profile.rs:1` | "No running service or network" | runs loopback HTTP servers |
| `crates/arkiv-db/tests/rehash.rs` header | 44/178/214/177 and a "0x00 pad" | asserts 84/330/218/176; keys are unpadded |
| `crates/arkiv-db/tests/records.rs:5-6` | zero-padded 33-byte keys | the test at 647-651 asserts no padding |
| `crates/arkiv-db/tests/isolation.rs:698-699` (and `docs/testing.md:318`) | raw keys "encoded as `key ‖ 0x00 pad to 20 ‖ len` (21 bytes, 42 nibbles)" | `layout.rs:153-179`: `bytes⁰(key)`, escaped and terminated, 1 to 41 bytes **(re-verified)**; the node-count derivation that follows (698-710) needs re-deriving in the comment, the asserted literals are what the engine produces |
| `crates/arkiv-db/tests/bounds.rs:1`, `:203` | "real block packets"; "32 characters, 64 bytes" | 64 and 128 |
| `crates/arkiv-db/tests/query_history.rs:12-14`, `2510-2511`, `3340`, `2390` | four statements about the generated workload | [T2 §3] |
| `crates/arkiv-db/tests/roots.rs:76`, `golem_types.rs:6`, `range_proofs.rs:975` | type ids; "both profiles" | [T4 §3.2] |
| `crates/arkiv-db-trie/tests/store.rs:23-26` | header | contradicts the assertions at `:928`, `:934` |
| `crates/arkiv-db-redb/tests` `assert_applied` doc, `tests/isolation.rs:25-27`, `examples/key_size_report.rs:280` | head check that is not there; "engine does not use yet (N3)"; describes a range | [S4 §3] |

### 3.2 Narration a new reader cannot resolve - safe

- **"the brief", "the decisions file", "decisions2.md"**: 70 lines in 9 test files (`query_metering.rs` 23, `query_history.rs` 14, `block_model.rs` 7, `terms.rs` 5, `records.rs` 5, `query_isolation.rs` 5, `queries.rs` 4, `history.rs` 4, `pagination.rs` 3) cite a document that is not in the repo. `rg -n -i '\bbrief\b|decisions2?\.md|decisions file' crates -g '*.rs'`. TIGHTEN: cite `docs/` (after the charge table moves to `docs/metering.md`, §4.6) or state the rule inline.
- **Milestone and step labels**: about 55 lines (`N0a` 11, `N3b` 8, `milestone-2` 8, `N0b-2`, `N0b-3`, `N3c`, `N3d`, "Phase 0/1/2" inside the isolation files is fine, it is local). `rg -n -i 'milestone[- ][0-9]|\bN[0-6][a-d]?(-[0-9])?\b|node step' crates -g '*.rs'`. DELETE the label, keep the sentence.
- **History**: "Before N0a ..." in `trace_regression.rs` is the one place where history is the point (it is a regression file): keep the fact, drop the label. Elsewhere "as before", "no longer", "was `(1, 10, 15)`", "re-derived from the AVL-era literals" should go.

### 3.3 Banners and templates - safe, DELETE

- **Section banners**: 399 lines, `rg -c '^\s*// ?[-=]{8,}' crates -g '*.rs'`. Worst: `query_metering.rs` 36, `query_history.rs` 28, `pagination.rs` 28, `terms.rs` 22, `namespaces.rs` 22, `metering.rs` 22, `codec.rs` 22, `query_isolation.rs` 20. In src: `typed/planner.rs` 8, `typed.rs` 8, `codec.rs` 5, `exec.rs` 4, `block.rs` 4, `layout.rs` 4. Replace a banner by a module (when the file is split) or by nothing.
- **Module-doc templates** ("What this file establishes / What it does not establish", 13 to 40 lines, restating the test names): about 450 lines across the arkiv-db and arkiv-db-trie test files (T3 166, S5 70, T4 60, T2 60, T1 55). TIGHTEN each to 3-6 lines: what is independent about the oracle and what the file deliberately leaves to another file.

### 3.4 Docs that restate - safe, DELETE or TIGHTEN

- `engine.rs`: 32 "Fallible form of [`x`](Self::x)." blocks (40 lines); they disappear with the macro of §1.4.
- arkiv-node: 62 field docs that repeat the field name (`block.rs`, `params.rs`, `exec.rs`) [N4 §3].
- `query.rs`: 23 restating doc lines [S2 C3].
- `versioned.rs` and the two backends: the engine-only profile, the redb latching rule and the bounded-read caveat are each repeated four to six times across files (about 120 lines). State each once on the trait and link [S3 C2-C6].
- arkiv-db-trie: `order.rs` 10 lines, repeated rationale 15 [S5 §3].

### 3.5 Under-commented (add, do not remove)

- `durable/bulk.rs`: 4 blank lines and 6 comment lines in 922; needs about six "why" comments [S2 C6].
- `gas.rs`: the 31 gas-schedule weights are unlabelled, and no test pins them although a test name claims so; creation-flag bits and ABI type ids are bare literals in `arkiv/*` [N4].
- arkiv-node `signed/` and `simulator/` are sparse but what is there explains why; leave it.

## 4. Sloppiness (curated)

### 4.1 Dead code and unused public items - safe (each verified with `rg` over `crates/ conformance/ scripts/`)

| Item | Size | Note |
|---|---|---|
| `crates/arkiv-db-memory/src/persistent.rs` (`PersistentMap`, `Probe`, `Range`) | 691 lines | exported at `lib.rs:12`, no user anywhere **(re-verified)**; `docs/architecture.md:57` already calls it "no longer used by the engine", while its own docs still call it the engine's snapshot primitive. Delete it and the mentions at `README.md:236`, `docs/architecture.md:57`, `:86`, `:189` |
| `vendor/reth-trie` | 243 lines + workspace member, CI/README/docs package lists | zero `reth_trie` hits in `crates/` **(re-verified)**; owner decision |
| raw KV facade: `Database`, `KvStore`, `MemoryStore`, `BlockMap`, `HostError` | about 150 src + 673 test lines + `examples/basic.rs` | used only by arkiv-db's own tests and one example; `docs/architecture.md` retains it deliberately: owner decision [S2 S4-S5], [S3 W5] |
| `prepare_import`, `import_checkpoint`, `initialize_from_staged` (arkiv-db durable) | about 275 with their tests | no production caller; owner decision [S2 S3-S5] |
| `PreparationStats::emitted_by_map` | | written per staged object, read nowhere [S2] |
| `STATE_ROOT_DOMAIN`, `MapKind::ALL` | | unused [S2] |
| `Snapshot::hashed_nodes`, `TypeTag::decimal_scale`, `Receipt::is_applied`, `Budget::is_traced`, `Budget::is_interruptible`; the `i64` codec (golden tests only) | about 50 | [S1 §4] |
| `contract::objects_of`, `FaultyStore::clear_faults`, `ReadScope::Everything`, `RedbStore::config`, `CrashDisk::disarm_crash` + three more | about 50 | [S3 S2] |
| `MemoryNodeStore::ids`, `CodecError::reason`, `KeyRange::all`; `KeyWriter`/`KeyReader` (107 lines, own tests only); `NodeBox::reference` leaks a vendored type (make `pub(crate)`) | 13 to 120 | [S5 §4] |
| `rpc.rs:1384` unreachable arm, `rpc.rs:581-583` unreachable check, `EthereumTransaction::gas_price` never read, `_genesis` parameter of `call_ops`, four needlessly `pub` fns | about 55 | [N1 §4] |
| `simulator::wire::ResolveRequest`, `demo::workload::DEFAULT_SEED`; `signed/runtime.rs:714-718` re-declares `wire::ProofRequest`; `SignedTx::from_bytes`, `SignedBlock::from_bytes` test-only; `"CoverageUnavailable"` arm (`simulator/runtime.rs:338`) never constructed **(re-verified)** | about 60 | [N2 §4] |
| `StreamReader::prefix_hash`; the deprecated `--batch-bytes` threaded through five signatures | about 40 | [N3] |
| `encode_natural_value`/`decode_natural_value` (34 lines) and `arkiv::execute_batch` (41) called only from tests | 75 | [N4] |
| `tests/support/mod.rs` `scan`, `smallest_u64` (hidden by `#![allow(dead_code)]`); `inspect_interval` in `tests/interval_security.rs`; `mod common;` in `engine_crashes.rs:5` uses nothing; dead `retain` at `tests/common/isolation.rs:282-284` | about 60 | [NT1], [S5], [S4] |
| `crates/arkiv-db-redb/examples/durable_throughput.rs` (referenced nowhere), `key_size_report.rs` (one-off, result recorded in one benchmark doc) | 517 | owner decision [S4] |
| arkiv-node dev-dependencies: `alloy-primitives` redundant with a stale comment; `alloy-trie` is needed only through a `#[path]` include and needs a comment saying so | | [NT3], [D2] |

### 4.2 `#[allow(..)]` - safe

- **Dead allows** (the function has 7 parameters; the lint fires above 7): `signed/host.rs:165`, four in arkiv-db `durable/`/`proof` [S2 S1], `typed.rs:1324`, `query_history.rs:2802`, `queries.rs:902`. Two of three `result_large_err` in `host.rs` sit on functions whose `Err` is the small `StorageError` [S2].
- **Live allows with an obvious struct**: `exec.rs:425-559` x4 (`BlockEnv`), `engine.rs:2786` (`View`), `signed/host.rs:186`, `:282`, `:375` (`Launch`), `arkiv-proposer.rs:275`, `:344` (same `Launch`), `genesis_streaming.rs:336`, `:360`, `genesis_bulk.rs:117`, seven in `query_history.rs` (thread the existing `Run` pair).
- **`#![allow(dead_code)]`** in `tests/support/mod.rs`, `tests/signed_support/mod.rs`, `arkiv-db-trie/tests/support/mod.rs`, `arkiv-db-redb/tests/common/mod.rs`, `tests/engine_support/mod.rs` and six item-level ones in the trie interval tests hide real dead helpers (§4.1). One test binary per crate removes the need.

### 4.3 Size and formatting

| File | Lines | Problem | Split |
|---|---|---|---|
| `crates/arkiv-db/src/engine.rs` | 3,140 | Snapshot (600 lines, 42 pub fns) and Branch (790, 48) in one file with staging, write path, read path and tests | `engine/{mod,staging,snapshot,ops,branch,write,read,explore}.rs`, ranges in [S1 §4] |
| `crates/arkiv-node/src/signed/rpc_reads.rs` | 3,227 | 1,787 code + 1,440 tests | move tests to `rpc_reads/tests.rs`; parser plumbing out [N1] |
| `crates/arkiv-node/src/signed/rpc.rs` | 2,301 | `dispatch` 942-1268 is one 319-line `match method` with inline bodies and 11 inner `method ==` re-tests; `simulation` 153, `revert_data` 144 | one fn per arm, module split [N1 S1] (keep method literals inside `dispatch`, see §2 E) |
| `crates/arkiv-node/src/bin/arkiv-proposer.rs` | 1,345 | `run()` 604-1307; help on one 3,633-char line (`:615`) | `cmd_keygen`, `cmd_sign`, `cmd_submit`, `cmd_producer`, `cmd_follower`; `enum Role`; per-subcommand flag sets; help as a `concat!` const (bytes identical) [N3] |
| `crates/arkiv-node/src/genesis_*.rs` | 7 files, 5,617 | two live paths (small v1/v3, streaming v2/v4) plus prebuilt, adopt, resources; three module cycles | a `genesis/` tree [N3, "Genesis layering"] |
| `crates/arkiv-node/src/arkiv/mod.rs:394` | 270-line `execute_operation` | 18 early returns, 4 `expect`s, an `unreachable!()` | six named phases [N4 S1] |
| `crates/arkiv-node/src/simulator/metadata.rs:71` | 275-line `capture` | control flow on a status string the function just produced | [N2] |
| `crates/arkiv-node/src/signed/metrics.rs:215` | 263-line `render` | §1.10 | |
| `crates/arkiv-db/tests/query_metering.rs` | 4,804 | 1,010 lines of helpers before the first test | three files [T1 §4] |
| `crates/arkiv-db/tests/query_isolation.rs:1514` | one 854-line fn (`expected_baseline`) | splits along its own eight sub-banners [T2] |
| `crates/arkiv-db/tests/blocks.rs:277` | 352-line `six_step_flow_..` | split along its step comments; two of its blocks restate other tests [T4 §2.3] |
| `crates/arkiv-node/tests/signed_http.rs:185` | 307 lines, four unrelated stages | split into 4 [NT2] |

Formatting, all **safe** except inside `json!` bodies:

- **rustfmt gave up** (whole statements left unformatted, because one line in them cannot be broken): `bin/arkiv-proposer.rs:586-592` (the `match command` arms of `genesis_command`) and the help strings at `:615`, `:618`; `signed/runtime.rs:756-783`, `917-929`; `simulator/runtime.rs:488-493`; `simulator/inspection.rs:58-190`; `signed/state_api.rs:72-101`; `signed/rpc_ws.rs:177-262`; parts of `signed/range_api.rs:154-190`; `signed/rpc.rs:1312` (a whole JSON-RPC error on one line) and seven more `json!` lines. Fix the long literal (split with `concat!` or `\` continuation, or pull the `json!` into a `let`), then run `cargo fmt`. `list: awk 'length > 150 {print FILENAME":"FNR": "length}' $(find crates -name '*.rs')` (74 src lines).
- **No blank line between items**: about 810 boundaries; worst `genesis_authoring.rs` 45, `signed/runtime.rs` 33, `signed/rpc_reads.rs` 31, `signed/rpc.rs` 30, `simulator/runtime.rs` 30, `arkiv-db/src/proof.rs` 25, `durable/bulk.rs` 20. rustfmt will not add them; it is a manual pass per file.
- **One-letter bindings** (`e d c p f r n h v t k b o w`) in `gas.rs`, `accounts.rs`, `arkiv/*`, `simulator_composition.rs`, `src/explore.rs` of the trie; `full-prefill-bootstrap.py` is code-golfed with lines up to 370 chars.
- `cargo fmt --check` cannot catch any of this. Optional guard in CI: a three-line script failing on `.rs` lines over 120 chars.

### 4.4 Leftover output, clones, small waste - safe

- Debug output in tests: `crates/arkiv-db/tests/query_planner.rs:797` (`eprintln!`), `crates/arkiv-node/tests/gas_execution.rs:163` (`println!`), `crates/arkiv-db/tests/range_proofs.rs:873-898` (timing + `eprintln!` in a regular test). Library: `signed/host.rs:442` and `signed/reads.rs:196` print JSON events from library code; the genesis ones are an interface (see the do-not-touch list), these two should go through one `event(..)` function at least.
- Clippy `redundant clone`: 93 sites, all listed in Appendix E.3. In src: `layout.rs:717`, `942`, `949`, `959`, `1025`; `proof/range.rs:387`; `genesis_streaming.rs:865`, `1044`, `1103`; `genesis_prebuilt.rs:753`; `signed/rpc.rs:2086`; `signed/rpc_reads.rs:3003`; `testing/contract.rs:407`, `504`, `766`, `958`. The rest are in tests (`query_metering.rs` 7, `adopt.rs` 7, `arkiv/tests.rs` 5, `durable_failures.rs` 5).
- Real waste: `replay.rs:230` clones the whole inclusion index on every block [N4]; `signed/host.rs:844-845` deep-clones the cached block twice to compare two fields [N2]; `meter.rs:656` zeroes a 248-byte array on every charge; `engine.rs:715` validates by encode-then-decode; `engine.rs:896` identity closure [S1]; both `stage_objects` deep-clone every staged object to reuse a `headless: bool` signature [S3 D5]; `SignedGenesis::hash()`/`spec()`/`validate()` re-encode and re-digest the genesis on every `authenticate` and `tx.validate` [N2].
- Hidden configuration: `simulator::Runtime::new` reads three environment variables inside a library constructor (`simulator/runtime.rs:50-68`) [N2].
- Magic numbers next to their named constants: `64` x4 in `signed/runtime.rs` beside `wire::HEADER_BATCH`; `Semaphore::new(4)` beside `READ_CONCURRENCY`; `156`/`164` header sizes in four files; `8 * 1024 * 1024 - 116` beside `ENVELOPE_CAP`; path depths `17`/`67`/`65` at 11 verifier sites while the generator derives them [N2], [S2 S10].

### 4.5 Test support compiled into production - needs-care

`arkiv_db_storage::testing` (contract.rs 1,209, faulty.rs 444) and `arkiv_db_redb::testing` (854) are ordinary `pub mod`s, so 2,523 lines of test support compile into `arkiv-node` release builds. Gate both behind a `testing` cargo feature enabled from `[dev-dependencies]` [S3 S3]. (`CrashDisk` is used by an example; give that example `required-features`.)

### 4.6 Documentation - safe (full tables in Part II §4 [D1])

- **61 numbered findings, about 43 of them direct doc-vs-code contradictions with both sides quoted**: README 13, `docs/architecture.md` 11, `docs/testing.md` 13, other docs 14, `arkiv-v2-compatibility.md` 5, small READMEs 5; plus 10 broken anchors (0 broken file links) and 9 dangling cross-repo paths. The recurring ones: cell names "20" (is 32), indexed values "64 bytes" (default 128, ceiling 512), padded 33-byte record keys (unpadded since the prefix-free change), block format "version 1/2" (is 5), state format 1 (is 2), range-proof caps 64/64 (are 1,000/1,000), "31 JSON-RPC methods" (32), `Value::MAX_INDEXED_LEN` (no longer exists).
- **README** (432 lines): 229 lines of status prose before the first heading; lines 279-390 describe 31 of the 98 integration-test files and duplicate `docs/testing.md`; no quickstart for `arkiv-proposer`; the crate table describes `arkiv-node` as it was three generations ago. Target about 200 lines.
- **`docs/testing.md`** (570 lines) is a build diary organised by step: it covers 63 of 98 test files, names a file (`protocol_two.rs`) and a test fn that do not exist, and still has "Planned" sections for proofs and durability that are implemented.
- **Plans beside implementations**: DELETE `signed-proposer-v1-plan.md` (528) and `arkiv-system-compatibility-plan.md` (144); split the still-normative charge table and record model out of `snapshot-query-plan.md` and `node-storage-plan.md`, then archive the rest; `base-fee-v1-plan.md` is unimplemented and belongs in `docs/proposals/`.
- **Run logs and one-host notes** in `docs/` (about 1,400 lines, two JSON blobs, a 39 KB HTML slide deck `node-design-review.html`): `/home/ubuntu/...` paths on 27 lines in 9 files, `rogue-one` host names, 12 "N tests passed" claims with nine different totals. Move to `docs/runs/` or out of the repo.
- **Tells**: `vendor/README.md:3-6` "The user explicitly authorized copying ..." (assistant-to-user phrasing in a provenance file); `AGENTS.md` at the repo root; `README.md:405` "Work proceeds in small commits on `main`"; step labels about 365 times in docs, defined only in the plan files proposed for archiving.
- Proposed target layout: 54 files / 15,899 lines to about 31 / 9,700 [D1 "Proposed target layout"].

### 4.7 Scripts, CI and build - safe (full detail in Part II [D2])

- `scripts/bench-compare.sh:24-28`, `:260`: defaults to `/home/ubuntu/...`, to a temporary session directory on one machine and reads `GITHUB_TOKEN` from `/home/ubuntu/arkiv-network/.env` **(re-verified)**. Make them required env vars with no default. No secret is in the tree.
- `Dockerfile.simulator` copies only `crates/` and `vendor/`, but the lib does `include_str!("../../../docs/examples/genesis-bundle/profiles/fixed-glm-v2.toml")` (`genesis_authoring.rs:48-49`) **(re-verified by reading; not built)**. It is referenced by no doc or workflow: fix (copy `docs/examples`) and add a CI build, or delete. Better: move that preset next to `genesis_presets/` so `docs/examples` stops being a compile-time input `[CONSENSUS]`.
- Obsolete: `sdk-compat-smoke.{py,ts}` (762 lines; own header says "expected to fail" against current nodes **(re-verified)**); the codec-2 prefill pipeline `sourcify-prefill.py`, `run-full-prefill.py`, `full-prefill-bootstrap.py` and their tests (2,446 lines; the node accepts only row codec 3, and `full-prefill-bootstrap.py` loads a `runtime.py` that is not in the repo); `query-order-bench.py` (262) is referenced nowhere.
- CI runs fmt, clippy and tests as the README says. It runs none of the five `scripts/test-*.py` (2,306 lines, 63 test methods), no `cargo doc`, no Docker build. `cargo test --workspace` includes the vendor crates while fmt and clippy exclude them.
- Cargo: four unexplained `=` pins in arkiv-node (`toml` has a real reason, say it); `[workspace.lints]` only forbids `unsafe`. Reasonable additions: `clippy::redundant_clone`, `clippy::needless_pass_by_value` as warnings, `RUSTDOCFLAGS=-D warnings` on a `cargo doc` step.

## 5. Other things that make a reviewer wince (curated)

### 5.1 String-typed errors and state in arkiv-node - needs-care `[CONSENSUS]`

`simulator/mod.rs:15` defines `pub struct Error(pub &'static str)`; `signed/mod.rs:3` re-exports it; `demo/mod.rs:9-29` has a real 19-variant enum with the same spellings. Measured in `crates/arkiv-node/src`: 107 distinct codes, 180 `Error("InvalidRequest")`, 107 `Error("StorageCorrupt")`, 305 `map_err(|_| ..)`. `simulator/mod.rs:46-59` maps `ProofError` variant by variant to the same spelling.

Consequences found:

- Dead or unreachable arms nobody can see: `"CoverageUnavailable"` (`simulator/runtime.rs:338`), `"UnknownRun"` (335, startup only).
- Wrong codes: `ethereum.rs:160` reports a bad y-parity as `GasPriceBelowMinimum` **(re-verified)**; `rpc.rs:1034` labels any future drop reason as insufficient balance; `validate.rs:197-217` reports every Arkiv validation failure as `OpFault::HostOwnedCell` ("names a `$` cell").
- Status decided from text: `rpc.rs:1480` picks HTTP 429 from the message; `simulator/runtime.rs:581-598` is a 15-arm `Some("X") => "X"` table needed only because the error is `&'static str`.
- Side channels to carry detail past the type: the `"QueryRejected"` placeholder with `RpcState.query_error`/`revert_data` [N1]; `HistoryPruned` details travel through a `Cell` on the host plus a `try_lock` after the request, so they can be lost or attributed to the wrong request [N2].
- One mistake, several codes: `NonpayableCall` vs `NonzeroArkivValue`, `UnsupportedContractCall` vs `UnsupportedEthereumCall`; "wrong node role" has five spellings.
- `health` and `role` are `pub &'static str` / `String` fields compared by literal on 76 lines; `demo/runtime.rs` stores `DemoError::to_string()` in `health` and later compares against variant names.
- The seven genesis files use `Result<_, String>` with a local `err`; the proposer's `authoring_error` prints the cause and returns `Error("InvalidGenesisBundle")` at 24 sites.

Fix: `enum ErrorCode` with `as_str()` returning today's exact strings, `struct Error { code, detail: Option<..> }`, `fn status(code) -> StatusCode` as an exhaustive match; `GenesisError(String)` plus a `CliError` enum in the binary (message strings byte-identical: about 45 assertions and the CLI tests match them); `enum Health`, `enum Role`.

### 5.2 Real defects found on the way (not style)

| # | Where | Defect | Risk of the fix |
|---|---|---|---|
| 1 | `crates/arkiv-db/src/durable/adopt.rs:202-209` | `queue.active` is decremented only on the normal path; if `part` panics with two or more workers, the others wait on the condvar forever and `thread::scope` never returns **(re-verified)**. Decrement in a drop guard that also sets `failed` and notifies. | needs-care |
| 2 | `crates/arkiv-db-storage/src/testing/faulty.rs:334-444` | `FaultyStore` does not forward `stage_objects`/`relabel`, contrary to its doc; no contract case covers either method **(re-verified)** | needs-care |
| 3 | `crates/arkiv-node/clippy.toml` | `Branch::checkpoint` and `Branch::seal` panic on a poisoned branch (`engine.rs:1963`, `2313`) and are missing from the ban list; node code calls them at `exec.rs:960`, `arkiv/mod.rs:839`, `genesis_bundle.rs:578` **(re-verified)** | behaviour-affecting (panic becomes an error) |
| 4 | `crates/arkiv-node/src/signed/rpc.rs:128-134` | `uint` accepts `"0x+1"` (and `rpc.rs:799-811` does no leading-zero check) | behaviour-affecting `[CONSENSUS]` |
| 5 | `crates/arkiv-node/src/signed/ethereum.rs:160` | bad y-parity reported as `GasPriceBelowMinimum` | behaviour-affecting `[CONSENSUS]` |
| 6 | `crates/arkiv-node/src/signed/runtime.rs:776` + `signed/host.rs:720` | an HTTP-submitted transaction is validated twice (signature recovery included), the second time under the node lock | needs-care |
| 7 | `crates/arkiv-node/src/demo/runtime.rs:771` | `serde_json::to_vec(&response).unwrap_or_default()`: a serialisation failure becomes an empty 200 body | behaviour-affecting |
| 8 | `crates/arkiv-node/src/block.rs:306` | `Genesis::MIN_ENCODED_LEN` uses a stale `48` for `Limits` (it is `9 * 8 = 72`) **(re-verified)**; a lower bound only, nothing observable changes | safe |

Also: the vacuous test, the tests that never run and the tests that hang instead of failing (§2 E); the drifted report text `engine_storage_experiment.rs:531` ("eight record reads" vs "seven" in the checked-in report); `ethereum_transactions.rs` building its chain with two different proposers; `signed_retention.rs:164-171` checking only the last hex digit of a result.

### 5.3 API shape

- `Branch` and `Snapshot` share a read API but no trait (§1.1).
- Configuration by mutating public fields after construction: `host.prefill_file`, `host.prefill_stream`, `paused`/`health`/`role`, `storage_path` through `Arc::get_mut` [N2].
- Test-only items in the public API: the 30 infallible engine methods, `SignedTx::from_bytes`, `arkiv::execute_batch`, the natural-value codec, `KeyWriter`/`KeyReader`, the durable import entry points.
- `signed/runtime.rs:2-7` keeps child modules flat with `#[path = "range_api.rs"] mod ..` x3; `lib.rs` makes everything `pub mod` except `genesis_bulk` and `genesis_import_resources`.
- A back-edge: `simulator/light.rs:417` calls `crate::signed::rpc_reads::verified_attributes` while `signed` depends on `simulator` everywhere else.
- `--batch-bytes` is deprecated, still parsed, still threaded through five signatures.

### 5.4 Error swallowing

305 `map_err(|_| ..)` in arkiv-node, 61 of them in the proposer, most replacing an `io::Error` or `DecodeError` by `StorageUnavailable`/`InvalidRequest`. In arkiv-db: `durable/simulator.rs:78`, `:245`, `:388` (drops `DecodeError`), `durable/mod.rs:429` and `streaming.rs:194` (replace any `validate_create` fault by "row exceeds admission limits" even when the fault is not a size), `format.rs:70` [S2 W7]. With an error that can carry detail (§5.1) these become `map_err(|e| Error::storage(e))`.

### 5.5 Tests of the test, and tests of the source text

`state_docs.rs` scraping `rpc.rs` (§2 E); `oracle_reservation_matches_the_parameters`; `durable_failures.rs:895-900` mirrors the cache's private slot layout; `TermLedger` keyed by `Debug` output and three dispatches on display-label strings in `query_history.rs` [T2]; mutations selected by bare integers in the proof tamper tests [T4 §5.2].

### 5.6 Machine-specific residue

`bench-compare.sh` session-directory default; `/home/ubuntu` paths in 9 docs; `rogue-one` in doc names (`rogue-one-metrics.md`, `rogue-one-deployment.md`) and in the Prometheus metric prefix `rogue_one_` (`[CONSENSUS]`-adjacent: it is an external metric name, change only deliberately); `vendor/README.md` "The user explicitly authorized"; `AGENTS.md` instructing agents to push to `main` without confirmation, at the repo root.

### 5.7 Test-suite mechanics

98 integration-test binaries (§1.1); wall-clock assertions (§2 E); ports probed then released before the child binds (a small race, no fixed ports anywhere, which is good); simulator tests use 2-5 s iteration budgets against 20-120 s elsewhere [NT3].

### 5.8 Licence - owner decision

`crates/arkiv-node/src/simulator/query_language/` is adapted from `Arkiv-Network/arkiv@0fd58ce`, `crates/arkiv-query`, declared GPL-3.0-or-later, and carries its own `LICENSE` and `README.md` **(re-verified)**. No `Cargo.toml` in the workspace declares a `license`, and there is no root licence file. Whatever the answer is, it should be stated once at the root and in `[workspace.package]`.

## What is already fine (do not spend time here)

- **arkiv-db src**: the canonical codec's hand-written struct and data-enum impls, `layout.rs` encodings, `meter.rs` charge model, `durable/cache.rs` and its 11 tests, `durable/format.rs` loops, `interrupt.rs`, `nodes.rs`. Comment share of 13-27% is mostly real "why" documentation; the cuts in §3 are the exceptions.
- **arkiv-db-trie src**: `nibbles.rs` is justified (the vendored `Nibbles` is capped at 64 nibbles); no dead error variants; the production node encoder is clean (the clippy hints are in its test module).
- **arkiv-db-storage / redb**: `batch.rs`, redb `error.rs`, `counting.rs`, `metrics.rs`, the `CrashDisk` model and its 7 tests; no banners, no `#[allow]`, no debug output, no reachable unwraps in that chunk.
- **arkiv-node core**: `block.rs`, `exec.rs`, `params.rs`, `validate.rs`, `identity.rs`, `replay.rs`, `select.rs`, `value_json.rs` read as careful code; `validation.rs`, `identity.rs`, `selection.rs` tests are already table-driven; `block_format.rs` goldens.
- **Tests worth their weight**: `trace_regression.rs`, `codec.rs` (one generic `golden<T>` harness), the proof tamper tests, the redb crash/recovery/durability suite, `signed_history.rs`, `signed_pool.rs`, `block_model.rs`' charge table and independent root.
- **Outside the crates**: `conformance/golem-db/src/*.rs` (careful, well commented), `check-tests.py`, `sourcify-v4-prefill.py` and its test, `scripts/bench/*.py`, `ci.yml`, root `Cargo.toml`, `CHANGELOG.md` content, `docs/block-format.md`, `docs/state-trie.md`, `docs/genesis-bundle-v1.md`, `docs/glm-gas-v1.md`, `docs/range-proofs.md`, `docs/pruning-design.md`, `docs/node-api-reference.md`, `docs/entity-expiration.md`, `docs/proposals/expiry-spill.md`. Zero broken relative file links in 67 markdown files.
- **Demo and simulator profiles are not legacy**: both are shipped binaries with docs, scripts and tests, and `signed` builds on them. Retiring the demo is a product decision, not a clean-up.
