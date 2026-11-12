# Code-quality review, 2026-11-17: Part II.5: other findings

Reviewed at `e6c49e1`; line numbers refer to that commit. Parts: [summary](README.md) · [duplication](duplication.md) · [tests](tests.md) · [comments](comments.md) · [sloppiness](sloppiness.md) · [other](other.md) · [appendices](appendices.md)

## II.5 Other wince items


### II.5 [N2] arkiv-node runtimes: signed / simulator / demo

*(paths relative to `crates/arkiv-node/`)*

1. **Lock API names lie.** signed/runtime.rs: `read_node()` (190) hands out a `MutexGuard<Node>` that is used to
   mutate - `h.submit(tx)` (777-778), `h.produce()`, `h.paused = ..`, `h.retry_pending()` (958-981) - while
   `write_node()` (180) is reserved for sync/production. The names describe queueing priority, not access. Rename
   (`poll_node`/`queue_node`, or `request_node`/`chain_node`) and fix the doc at 78-82 (Comments 1).
   `Runtime::node` is also `pub` and locked directly, bypassing the turn-taking the struct documents: in production
   by `signed/rpc_ws.rs:214` and `signed/metrics.rs:146` (both `try_lock`), in tests by `tests/signed_http.rs`
   (12 sites) and the test module of `signed/rpc.rs` (1961, 1996). **needs-care**.
2. **Task panics are reported as storage failures.** Every `spawn_blocking(..).await` maps `JoinError` to
   `"StorageUnavailable"`: simulator/runtime.rs:387, 464, 493, 730, 860, 883; signed/runtime.rs:296, 306, 665, 693,
   782, 929, 989; demo/runtime.rs:744 (and 801 `let _ =`). A panic in a handler disappears into a 503 with no log.
   One `fn joined<T>(r: Result<Result<T>, JoinError>) -> Result<T>` that logs and returns an `Internal` code.
   **behaviour-affecting [CONSENSUS]** (error body).
3. **Bearer tokens compared with `==`**: simulator/runtime.rs:474, signed/runtime.rs:933-934 (the latter also
   allocates a `format!("Bearer {token}")` per request). Use a constant-time comparison. **safe**.
4. **Positional-argument APIs.** `signed::runtime::fetch(client, peer, path, body: Option<Json>, token: Option<&str>, cap)`
   is called with adjacent `None, None` (signed/runtime.rs:831-832, 1113-1114); `verified(rt, r, inspect: bool, permit)`
   (859) and `account_response(height, actor, proof: bool)` (signed/host.rs:646) take bare bools;
   simulator `execute_publish` takes six positionals (simulator/host.rs:579-587). Small request structs or enums.
5. **Same JSON field from two sources.** `"gasPriceWei"` is `config.gas_price_wei` at signed/host.rs:656 (full node
   account response) and `crate::accounts::GAS_PRICE_WEI` at signed/runtime.rs:856 (light node verified account) and
   signed/host.rs:695. If the config value can differ from the constant, the two roles disagree; if it cannot, one of
   the two is redundant. Not resolved here (`accounts.rs` is outside the chunk). **[CONSENSUS]**.
6. demo/runtime.rs:771 `serde_json::to_vec(&response).unwrap_or_default()` turns a serialisation failure into an
   empty HTTP 200 body; 776 right below uses `expect("error response")` for the same operation. Pick one.
7. signed/protocol.rs:130-146 `default_native_genesis` mutates eleven fields of the demo genesis one by one; with the
   genesis moved to a neutral module (recommendation 2) write it as a struct literal with `..dev_genesis(..)` so the
   differences from the development profile are visible at a glance. **[CONSENSUS]** - values must not change.
8. simulator/workload.rs: `Workload` is at once the HTTP DTO, the stored canonical record and the domain object, so
   `seed` and `block_period_ms` are decimal `String`s re-parsed on every `inputs()` (33 via `validate`), `timestamp()`
   (161) and `update_allowed()`. Parse once into a validated struct at the boundary. **needs-care** (encoding **[CONSENSUS]**).

### II.5 [N1] arkiv-node signed JSON-RPC / HTTP reads

*(paths relative to `crates/arkiv-node/`)*

- **`rpc_reads.rs:597-626` `plan_entity` builds a query string to parse it back.** It validates the key, formats `"$key = key(0x..)"` and a JSON options object, then calls `plan_query`, which runs the lexer and parser on that string and re-parses the options it was just handed (`atBlock` goes number -> `quantity()` -> `json!` -> `quantity()` again, `:616` then `:650-654`). Split `plan_query` into "parse params" and `plan_statement(host, source, Statement, PageOptions, bounds)`; `plan_entity` passes `Query::term(Term::eq(..))` and a struct. Keep the same `source` text in the cursor binding. **needs-care**.
- **`rpc_reads.rs:278`: the cursor cache is process-global**, not per node: every `Host`/`Runtime` in the process (a full and a light node in one test binary, parallel tests) shares one 1024-entry FIFO. Bindings keep entries apart, but eviction is shared. It belongs in the runtime's read state (`reads::Reads` or `RpcState`), passed to `plan`/`run`. **needs-care**.
- **`rpc.rs:212-241` `transaction` calls `ethereum::parse(raw)` (`:215`) for display**, and `parse` ends in a secp256k1 `recover` (`ethereum.rs:174`). Every transaction in `eth_getBlockByNumber(.., true)` and every receipt pays a signature recovery to print `to`/`input`/`value`. Split `parse` into field decoding and recovery. `receipt` (`:504-537`) also replays the whole block's logs (`block_logs`) and filters by index, per receipt. **needs-care**.
- **`rpc_ws.rs:124-139`** asks its own node for the head through JSON (`eth_blockNumber`) and parses the hex string back, while the tick path reads it directly (`:214` `node.head().height`). If forwarding on a light node is the reason, say so in a comment; otherwise use one way.
- **`state_docs.rs:61-82, 120-135`**: two tests scrape Rust source text (`include_str!("runtime.rs")` split on `path=="`, and the `fn dispatch(` slicing described in S1). They are valuable doc-sync guards, but they depend on formatting and item order; a `pub(super) const METHODS: &[&str]` next to each `dispatch` (asserted against the OpenAPI document, and used for the `MethodNotFound` fallthrough) would make the guard structural.
- **`metrics.rs:187`, `reads.rs:196`**: `eprintln!` as the logging mechanism. The crate has no `log`/`tracing` dependency and `src/signed/` has only three such lines, so this is a policy, not leftover debugging; noted, no action.
- **`rpc.rs:54-108` `validate_filter`** is 55 lines of nested `if let`/`for`; an `fn each_or_one(value, max, |v| ..)` helper for "one hex value or an array of at most 64" would flatten the `address` and `topics` arms.

### II.5 [N3] arkiv-node binaries, genesis_*, query_language

*(paths relative to `crates/arkiv-node/src/`)*

1. **Every usage error is `InvalidRequest`.** bin/arkiv-proposer.rs has 57 `Error("InvalidRequest")` sites and
   bin/arkiv-simulator.rs 22; an unknown flag, a missing flag, a bad number and an unreadable file all exit
   with `arkiv-proposer: InvalidRequest`. bin/arkiv-demo.rs shows the alternative in the same crate
   (`"unknown option {key}"`, :57). Falls out of Duplication 2 + Sloppiness 1b.
2. **The flat whitelist accepts flags of other subcommands.** bin/arkiv-proposer.rs:660 checks all 36 flags for
   every command, so `keygen --out k --peer x` passes. One special case was added by hand for
   `--prebuilt-state` (:663-669, tested at tests/prebuilt_genesis_cli.rs:344-449), another for
   `--genesis-install` (:860-862) and `--state-retention` (:829-832). Per-subcommand lists make these three
   guards unnecessary.
3. **`map_err(|_| Error("StorageUnavailable"))` 30 times in bin/arkiv-proposer.rs, 9 in bin/arkiv-simulator.rs,
   often for things that are not storage:** `getrandom::fill` (:84, 678, 957), `reqwest::Client::build`
   (:771, 1088; simulator :323), `TcpListener::bind` (:1270, 1278; simulator :271, 278), a panicked blocking task
   (:1246), `axum::serve` (:1281), a poisoned mutex (:1193), a 10 s bootstrap timeout (simulator :345). A bind
   failure on a busy port prints `StorageUnavailable`. With `CliError` these carry the `io::Error`.
4. **One failure, four codes.** Failing to decode `genesis.bin` is `InvalidGenesisBundle`
   (bin/arkiv-proposer.rs:207), `InvalidGenesis` (:560, the only use of that code in the workspace),
   `InvalidRequest` (:687) and `IdentityMismatch` (:902, 972).
5. **`classify` recognises reserved features by message prefix.** simulator/query_language.rs:92-94 tests
   `error.message.starts_with("!= is not part")`, `"exists(…) is not supported"`, `"typeof(…) is not supported"`
   against text produced in parse.rs:228, 279, 283. A reworded upstream message silently turns "unsupported"
   into "invalid". Fix without touching the vendored parser: pre-scan `lexer::tokenize(input)` for
   `Token::Neq | Token::Exists | Token::TypeOf`, or keep the prefixes in one `const RESERVED: [&str; 3]`
   beside the test that pins them (:281).
6. **retention.rs `Refusal(Cell<Option<(u64, u64)>>)`** (:26-39) exists because `Error(&'static str)` cannot
   carry `requested`/`floor`: the detail is parked in a per-host cell by `record` and collected later with
   `take`. Three functions take the extra `&Refusal` (:56, 65, 75). With an error enum
   (`HistoryPruned { requested, floor }`) the type, both methods and the parameter disappear. Otherwise the file is clean.
7. **query_language is ported GPL code.** README.md: "adapted from `Arkiv-Network/arkiv`, revision
   `0fd58ce5bd6a0a196b0079f60d4679539346adc6`, `crates/arkiv-query/src`", AST from
   `crates/arkiv-interfaces/src/query.rs` and `entity.rs`; "The source workspace declares GPL-3.0-or-later";
   LICENSE is the GPLv3 text and every file carries the SPDX header. Local adaptations per README: std for
   alloc, a minimal AST, no SDK RPC-code mapping, `BETWEEN` and ranges on every indexable type, one test
   changed. (The upstream repository is not on this machine, so the "tests preserved" claim was not diffed.)
   Consequences: (a) it is compiled into `arkiv-node`, and no `Cargo.toml` in the workspace has a `license`
   field (`rg -n license Cargo.toml crates/*/Cargo.toml` -> nothing) nor is there a top-level LICENSE:
   the owner should decide and declare the crate's licence; (b) treat lexer/literal/parse/error/limits as
   vendored (do not restyle), and keep local code in query_language.rs.
8. **Magic numbers.**
   - genesis_streaming.rs:481 `base.genesis.limits.max_cells = 64;` is `signed::protocol::MAX_CELLS`
     (signed/protocol.rs:121). Use the constant. `[CONSENSUS]`, **safe** (same value).
   - bin/arkiv-proposer.rs:711 `128 * 1024` is `arkiv::MAX_BYTES_LENGTH` (arkiv/schema.rs:85); the same number
     is spelled in error strings at genesis_bundle.rs:200, 210 and "32-attribute cap" at genesis_authoring.rs:809.
   - `1_700_000_000_000` (default genesis timestamp): bin/arkiv-proposer.rs:953, bin/arkiv-simulator.rs:193.
   - Default source id `"00000000-0000-4000-8000-000000000001"`: bin/arkiv-proposer.rs:1024, bin/arkiv-simulator.rs:181.
   - `1024 * 1024` download chunk twice: bin/arkiv-proposer.rs:460, 461.
9. **`genesis_bundle::report` returns `{"error": ..}` in band** (genesis_bundle.rs:651-657) and has
   `.expect("valid supply")` inside the `json!` at :659. Callers store the value in the lock
   (genesis_authoring.rs:1020) and compare it (:1244, 1249) without looking for `"error"`. Return `Result`.
   **needs-care**.
10. **genesis_import_resources.rs:57-60** depends on the machine: `check_disk(Path::new("missing-prefill-output/child"), 0).is_ok()`
    needs 1 GiB free in the working directory and an unset or valid `ARKIV_PREFILL_RESERVE_GIB`. Keep, but
    note it will fail on a full CI disk for a reason unrelated to the code.
11. **`genesis_command` accepts more than the usage says.** bin/arkiv-proposer.rs:590 runs `build` with or
    without `--locked` although the usage (:592, :615) prints `--locked` as mandatory; `out.unwrap()` at :590
    relies on the guard two tokens earlier. Match on `(command, out)` instead.
12. **Stringly node state in the binaries.** `host.health = "running"` / `"following"`, `h.role = "full"`
    (bin/arkiv-proposer.rs:1056, 1186; bin/arkiv-simulator.rs:210, 237-239, 251-253), `"paused" | "running"`
    parsed twice with different shapes (proposer :1052-1059, simulator :202-207).
13. **"v2" in user-visible text for code that also serves v4.** `"expected streaming prefill v2"`
    (genesis_streaming.rs:283, 413), `"streaming prefill exceeds version-two admission limits"` (:86).
    Changing them is **needs-care** (error text is an interface, see top).

### II.5 [N4] arkiv-node deterministic core

*(`src/` = `crates/arkiv-node/src/`)*

**W1. `src/validate.rs:197-202, 215-217` - every Arkiv validation failure is reported as "names a `$` cell".**
behaviour-affecting (error variant only; block validity unchanged)
`operation.validate().is_err()` (any `ArkivFault`: too many attributes, oversize value, duplicate, bad flags), a
wrong namespace, and a generic write into the entity namespace all return `OpFault::HostOwnedCell`, displayed as
"operation {op} names a `$` cell" (81). Add `OpFault::Arkiv(ArkivFault)` / `TxFault::Arkiv { op, fault }` and a
variant for the namespace rule. `tests/validation.rs:85, 110` and `tests/replay.rs:1206` pin the current variant.

**W2. `src/exec.rs:820-843` - observer bookkeeping inside the canonical sweep.** needs-care [CONSENSUS]
`self.expired` (a `RecordKey` clone per deletion) is collected on every path, including `execute_block` and replay
where the observer is a no-op, and the closure fails the block with `StorageErrorKind::Corrupt, "expiry metadata
exceeds genesis capacity"` when the block-wide `expired.len()` reaches `expiry_per_block`, a limit documented per
height (`src/block.rs:146`). Collect only when an observer is installed, and do not report an observer limit as
storage corruption.

**W3. `src/lib.rs:27-48` visibility is inconsistent.** needs-care
Every module is `pub mod` except `genesis_bulk` and `genesis_import_resources`, and the core types are also
re-exported at the root, so `arkiv_node::Block` and `arkiv_node::block::Block` both exist. `rg '\b<mod>::'` over
`crates/arkiv-node/{tests,examples,src/bin}`, `conformance`, `scripts` finds no path user of `block`, `identity`,
`value_json` or `retention`. Make those four `mod`/`pub(crate) mod` (root re-exports stay), or drop the root
re-exports; pick one. `src/arkiv/mod.rs:8 pub use schema::*;` is the same problem in small: a glob that exports the
test-only fns of S3.

**W4. Same-named constants with different values.** `src/block.rs:61-71` and `src/signed/protocol.rs:45-49` both
define `GENESIS_DOMAIN`, `TX_DOMAIN`, `INPUTS_DOMAIN`, `HEADER_DOMAIN` (`"arkiv/tx/v2"` vs `"arkiv/signed-tx/v1"`).
Prefix the signed ones (`SIGNED_TX_DOMAIN`) or make the block ones private (S3).

**W5. `src/arkiv/mod.rs:10-77 ArkivFault`** writes its 18 discriminants twice (`#[repr(u8)]` values and an 18-arm
decode `match` of bare literals) and keeps `PermanentStorageUnsupported = 14`, which `rg` shows is produced and
matched nowhere. A `const ALL: [Self; 18]` + `find(|f| *f as u8 == tag)` removes the second table; the retired
variant can only go with a format decision. [CONSENSUS]

**W6. `src/accounts.rs:59-69 AccountConfig::validate`** folds four different defects (wrong gas price, too many
allocations, unsorted or duplicate actors, a zero amount) into `AccountFault::Unsupported`, and decode (238-241)
folds that again into "invalid allocations or price". behaviour-affecting if split.

**W7. Positional arguments that need a name.** safe
- `src/exec.rs:702` `Step::Applied(OpOutcome, alloy_primitives::U256, u64)`: make it
  `Applied { outcome, rent_wei, new_units }`.
- `src/exec.rs:1002` passes `tx.ops.len() == 1 && spent == crate::gas::TX_INTRINSIC_GAS` as the sixth positional
  argument of `self.op(..)`; bind it to `let intrinsic_covers_transfer = ..` first.
- `src/identity.rs:84-108 of_block` yields `(Self, Scope, u32)`; the `Scope` always equals `identity.scope`.

**W8. `src/gas.rs:149 validate_schedule`** returns `bool` (it is an `is_canonical_schedule`); the caller turns it
into `GenesisError::InvalidGasSchedule` at `src/params.rs:319`.

### II.5 [S2] arkiv-db src: durable, proofs, small modules

*(paths relative to `crates/arkiv-db/src/`)*

#### W1. `Initialization` has no accessor, so about 30 call sites destructure it by hand (**needs-care**)
`rg -c "Initialization::Committed\(" crates conformance` counts 31 across 20 files (one of them is the
constructor at mod.rs:1052; the rest destructure). Four files define the same
helper: `fn engine(init: Initialization) -> DurableEngine` (durable/bulk.rs:737) and
`fn committed(i: Initialization) -> DurableEngine` (`crates/arkiv-db/tests/adopt.rs:20`,
`tests/streaming_import.rs:9`, `crates/arkiv-db-redb/tests/streaming_import.rs:10`). In this chunk the let-else
form is at mod.rs:1138-1145, mod.rs:1223-1230 and adopt.rs:735-739.
Fix: `impl Initialization { pub fn into_committed(self) -> Result<DurableEngine, Self> }` in durable/mod.rs.

#### W2. `DurableEngine::recover` returns a `Recovery` that can only be `Ready` (**needs-care**)
mod.rs:987-1005 ends in `Ok(Recovery::Ready)` and has no other `Ok` path. Callers either drop the value
(`conformance/golem-db/src/arkiv.rs:705, 720`) or assert it equals `Ready` (seven times in
`tests/durable_failures.rs`). Return `Result<(), DurableError>`; `Recovery` stays for `recover_pending`.

#### W3. Two anonymous numbers in the genesis version tuple (**[CONSENSUS on-disk]**: values must not change)
format.rs:127-134 and 156-163: `[LAYOUT_VERSION, CODEC_VERSION, STATE_FORMAT, 3, 2, PROOF_VERSION]`. The `3`
equals `MODEL_VERSION` today (meter.rs:27) without being tied to it; the `2` matches no named constant I could
find. They are part of the genesis id preimage. Name both, or tie the `3` to `MODEL_VERSION` only if a
counting-rule bump is meant to change the genesis id.

#### W4. A wrong range-query version is reported as a different error from a wrong equality version (**behaviour-affecting**)
proof.rs:311-317 `version(d, kind)` returns `DecodeError::UnsupportedVersion`, which maps to
`ProofError::UnsupportedVersion` (proof.rs:221-229). proof/range.rs:148-150 instead returns
`invalid_codec("unknown range profile")`, which maps to `ProofError::InvalidProof`. Also `invalid_codec`
(proof.rs:305-310) stamps `kind: "eq-page-v1"` on every range error (proof/range.rs:131, 149, 158, 162).
Fix: give `invalid_codec` a `kind` parameter and use `version` in range.rs.

#### W5. Wire tags and versions as bare literals (**safe [CONSENSUS]** proof bytes: values must not change)
- Response kind: `1` namespace absent, `2` page (proof.rs:631, 651, 929, 1044); `3` range (proof/range.rs:467, 682).
- "ordering" and "projection" bytes `e.u8(1); e.u8(1);` (proof.rs:74-75, 87; proof/range.rs:141-142, 157).
- Bound tags `0`/`1`/`2` (proof/range.rs:108-131).
- Host proof version `1`: verifier at proof.rs:1074, generator at engine.rs:808, with no shared constant.
- `RANGE_VERSION` (request, `= 1`) and `RANGE_PROOF_VERSION` (response, `= LAYOUT_VERSION`) at
  proof/range.rs:12 and 15 are easy to swap. Rename `RANGE_QUERY_VERSION` / `RANGE_RESPONSE_VERSION`.
- Host-run value tags `0`/`1` and the length `17` in bulk.rs:471, 487, 584, 590, 595 (scratch only, not consensus).

#### W6. A worker panic hangs adoption instead of surfacing (**needs-care**)
adopt.rs:202-209: `queue.active` is decremented only on the normal path after `self.part(..)`. With two or more
workers, if `part` panics, `active` never reaches zero, the other workers stay in `self.wake.wait(queue)` (adopt.rs:196-199), and
`std::thread::scope` (adopt.rs:163-167) waits for them forever. Fix: decrement in a drop guard that also sets
`failed` and calls `notify_all`. Related: adopt.rs:120 reuses `DurableError::WriterUnavailable` as an internal
"another part already failed" sentinel; it is never reported (`fail` keeps the first error), so a private
marker would be clearer.

#### W7. Error causes dropped with `map_err(|_| ..)`
- simulator.rs:78, 245, 388: `DecodeError` (see S9).
- mod.rs:429 and streaming.rs:194: the `validate_create` fault, replaced by "row exceeds admission limits" even
  when the fault is not a size.
- format.rs:70: the `DecodeError` of an invalid initial header.
Fix: include the cause with `format!("..: {e}")`. **behaviour-affecting** (text only).

#### W8. API shape: mismatched setters and a copied limits struct
- `set_disk_check(Box<dyn Fn() -> Result<(), String>>)` (bulk.rs:406) and
  `set_progress(impl Fn(&str, u64) + 'static)` (bulk.rs:414): two conventions for two setters on one type, and a
  `String` error.
- `RangeProofLimits` (proof/range.rs:191-199) repeats five of the six `ProofLimits` fields and converts with
  `common()` (216-225). Embedding `pub common: ProofLimits` would remove the copy; **needs-care**, public struct.

#### W9. `host.rs`: two adjacent `Budget` parameters on three public functions
`expire_records`, `try_expire_records` and `try_expire_records_observed` (host.rs:124-131, 151-158, 175-183) all
take `query_budget: Budget, delete_budget: Budget` in a row; swapping them compiles. Production calls only the
third. If the three are kept, a `struct ExpiryBudgets { query, delete }` removes the hazard and one parameter.

#### W10. Inconsistent defensive check in the cache
cache.rs:450-456 re-checks `bytes.len() > self.max_object_bytes` after `read_object_bounded(.., self.max_object_bytes)`
has returned; `Context::read_object` (cache.rs:483-495) makes the same bounded call without the re-check.
Either the store contract is trusted (drop the check) or it is not (add it to `read_object`).

### II.5 [S3] arkiv-db-storage, arkiv-db-memory, arkiv-db-redb src

*(repo-relative paths)*

**W1. `stage_objects` and `relabel` have default bodies that answer "unsupported", and wrappers silently inherit them.** `versioned.rs:736-741`, `:747-752`. Both real backends override both, so the defaults serve only wrappers - and that is where they bite:
- `FaultyStore` (`testing/faulty.rs:334-444`) overrides neither. `FaultyStore<MemoryVersionedStore>::relabel(..)` returns `NotCommitted(Incompatible, "store relabelling unsupported")` although the inner store supports it, contradicting its doc (`:159-160`, C5).
- of the 8 hand-written wrappers (W2), 5 forward neither, 2 forward only `stage_objects`, 1 forwards both.
- nothing notices: the contract suite has no case for either method (T2), and no test asserts the "unsupported" strings (`rg 'relabelling unsupported|staging unsupported'` -> only the two definitions). `crates/arkiv-db/tests/adopt.rs:337` has its own `Faults` double, presumably because `FaultyStore` cannot be used on the relabel path.

The trait already knows the remedy; `read_object_bounded` was made a required method with the note "Wrappers must forward this method." (`versioned.rs:630`). Fix: remove the two default bodies, forward both in `FaultyStore` and in the D1 macro, add the T2 contract cases. **needs-care** (every wrapper gains two methods unless W2 is done first); **behaviour-affecting** for `FaultyStore` only.

**W2. What `FaultyStore` would need to replace the hand-written `impl VersionedStore` doubles.** Eight wrappers in seven files, about 420 lines of impl blocks of which about 60 are behaviour:

| double | struct / impls | what it adds | `FaultyStore` needs |
| --- | --- | --- | --- |
| `BoundaryStore` | `crates/arkiv-db-redb/tests/engine_process.rs:35` / `:39`, `:52` | parks the process after a `Committed` commit | (c) `on_commit` hook |
| `Probe` | `crates/arkiv-db/src/durable/adopt.rs:625` / `:630`, `:656` | records every object read; hides one id or flips its last byte | (b) object override, (c) read hook |
| `SlowStore` | `crates/arkiv-node/tests/signed_reads.rs:70` / `:89`, `:104` | sleeps and counts on namespace object reads | (c) read hook |
| `Corruptible` | `crates/arkiv-db/tests/durable_failures.rs:382` / `:403`, `:420` | replaces or hides objects, records, the head; panics on unbounded reads | (b) overrides, (d) |
| `Hooked` | `crates/arkiv-db/tests/durable_failures.rs:1242` / `:1266`, `:1284` | closure before each object read; logs `recover`; already wraps a `FaultyStore` | (c) read + recover hooks |
| `InterruptCopy` | `crates/arkiv-db/tests/streaming_import.rs:286` / `:295`, `:308` | fails the Nth `stage_objects` | (a) |
| `Faults` | `crates/arkiv-db/tests/adopt.rs:337` / `:343`, `:359` | hides one object; injects a `relabel` outcome, optionally after applying it | (a), (b) |
| `Overlay` | `crates/arkiv-db/tests/simulator_composition.rs:307` / `:311`, `:324` | replaces or hides records; panics on unbounded record reads | (b), (d) |

(a) Forward `stage_objects` and `relabel` (W1) and let the existing `CommitFault` queue target them (`IndeterminateApplied` is `Faults`' `apply_then_report`), with a skip count ("fail the Nth").
(b) Read overrides: `override_object(space, id, Option<Vec<u8>>)`, `override_record(table, key, Option<Vec<u8>>)`, `override_head(Option<Head>)`, answered through the shared `copy_bounded` (D3) so the length check still precedes the copy.
(c) Observers: `on_object_read(impl Fn(SpaceId, &ObjectId) + Send + Sync)`, `on_commit(impl Fn(&CommitOutcome))`, `on_recover(impl Fn())`.
(d) `deny_unbounded_reads()`: `read_object` / `read_record` panic, as `Corruptible` and `Overlay` do today.
(e) For the `ObjectReader`-only doubles: a skip count on read faults ("fail every read after the next N") replaces `crates/arkiv-db/src/engine.rs:3006` and `crates/arkiv-db/tests/storage_faults.rs:73`; the three plain maps (`tests/rehash.rs:84`, `tests/eq_proofs.rs:818`, `tests/range_proofs.rs:605`) are `MemoryVersionedStore` + `stage_objects` already.

If that is more surface than wanted, the cheap alternative is an exported item-position macro, `forward_versioned_store!(self.inner; database_id, head, read_record_bounded, ..)`, that emits the listed forwarding methods inside an `impl` block and leaves the overridden ones to the author; D1 can use the same macro.

**W5. The `KvStore` stack is a legacy API kept alive by tests and one example.** In this chunk: `KvStore` (`crates/arkiv-db-storage/src/lib.rs:30-56`), `WriteBatch` / `WriteOperation` (`batch.rs`, 59 lines), `MemoryStore` (`crates/arkiv-db-memory/src/lib.rs:15-72`); about 150 lines. Users (`rg -n '\bKvStore\b|\bMemoryStore\b|Database::(in_memory|from_store)' crates conformance`): the `Database` facade in `crates/arkiv-db/src/lib.rs:124-135`, the tests `crates/arkiv-db/tests/{kv,model,semantics,backend}.rs`, `crates/arkiv-db/examples/basic.rs`. `KvStore` has one real implementation plus a test backend (`tests/backend.rs:32`). `docs/architecture.md:207-214` states that the engine "never constructs a `MemoryStore` or `Database`" and keeps the facade deliberately ("the contract a later durable backend implements") - but the durable backend that arrived implements `VersionedStore`. Only `Entry` (`lib.rs:21-28`) is used by the engine (`crates/arkiv-db/src/engine.rs:97`). Owner's call; if it goes, arkiv-db-memory becomes just `MemoryVersionedStore` and `arkiv-db` no longer needs it as a normal dependency (`crates/arkiv-db/Cargo.toml:11`, used at `src/lib.rs:89` only for this re-export). **needs-care**.

**W3. Overlap between the three wrappers (`counting.rs`, redb `testing.rs`, `testing/faulty.rs`).** No code is literally copied: `CountingBackend` and `CrashBackend` sit at redb's `StorageBackend` (byte I/O) layer, `FaultyStore` at the `VersionedStore` layer. The overlaps are of concept:
- two kill switches at the I/O layer: `IoProbe::kill` (`counting.rs:76-78`, "process death") and `CrashDisk::cut_off` (`testing.rs:369-371`, "power loss"). `kill` is a test hook inside the production wrapper: `alive()` (`counting.rs:114-120`) is checked on every backend call, and its only callers are 5 redb tests and one example (`rg -n 'probe\(\)\.kill\(\)' crates`). It exists because file-backed stores cannot use `CrashDisk`. Moving it to a `KillableBackend` in `testing.rs` costs those tests `RedbStore::create`'s code path; so: keep, but say in its doc that it is test support. **needs-care** if moved.
- two sets of I/O counters: `IoProbe` (`counting.rs:23-33`) and `DiskCounters` (`testing.rs:136-148`); only `mutating_calls` is not derivable from the probe (S2).
- two fault vocabularies: `FaultyStore::fail_reads(scope, kind, count)` counts "the next N fail" (`faulty.rs:245`); `CrashDisk::fail(op, skip, kind)` / `fail_all(op, kind)` count "skip N, then fail once / forever" (`testing.rs:290`, `:320`). Both have a `clear_faults`. `FaultyStore`'s `DiskFull::{BeforeCommit, DuringCommit}` (`faulty.rs:35-44`) hard-codes what `CrashDisk`'s length quota and allocation quota produce through real redb. One vocabulary (skip + count) on both would read better, and (e) above needs the skip anyway.
- the latching predicate is duplicated (D9).

**W4. Triple tuples as public fields.** `CommitBatch::objects: Vec<(SpaceId, ObjectId, Vec<u8>)>`, `records: Vec<(Table, Vec<u8>, Vec<u8>)>` (`versioned.rs:315-317`), `Relabel::retired` (`:431`), and `stage_objects(&[(SpaceId, ObjectId, Vec<u8>)])` (`:736`). Callers index them positionally: `forged.records[0].2` (`contract.rs:505`), `fork.objects[0].1` (`:291`), `differs.retired[0].2` (`arkiv-db-memory/src/versioned.rs:464`). Named structs (`ObjectEntry { space, id, bytes }`, `RecordEntry { table, key, value }`) are the usual shape; in-memory only, no format change. **needs-care** (many call sites); low priority.

**W6. `IoProbe::new() -> Arc<Self>`** (`counting.rs:37-39`): a `new` that does not return `Self` (clippy `new_ret_no_self`). Call it `IoProbe::shared()` or return `Self` and let the two callers (`counting.rs:101`, `store.rs:314`) wrap it.

### II.5 [S1] arkiv-db src: engine, typed, planner, record, layout, codec, meter

*(paths relative to `crates/arkiv-db/src/`)*

5.1 **The node's panic ban has two holes.** `crates/arkiv-node/clippy.toml` says a storage error "must ... never be unwrapped into a panic", but `Branch::checkpoint` (engine.rs:1963, panics on a poisoned branch) and `Branch::seal` (2313) are not in the list, and non-test node code calls them: `crates/arkiv-node/src/exec.rs:960`, `crates/arkiv-node/src/arkiv/mod.rs:839` (`checkpoint()`), `crates/arkiv-node/src/genesis_bundle.rs:578` (`seal()`); `try_seal` is used at exec.rs:680. Add both to clippy.toml and switch the three sites to `try_checkpoint()?` / `try_seal()?`. **needs-care** (differs only on a poisoned branch: error instead of panic). Outside my files; noted because the API shape here causes it.

5.2 engine.rs:83-84 `#[path = "explore.rs"] pub mod explore;` makes a sibling file a child module so that it can reach `Snapshot`'s private fields, and explore.rs opens with `use super::*;`. Conventional location is `engine/explore.rs` (falls out of 4.1).

5.3 engine.rs:1431-1432 `Effect::BalanceTransferred { to: OwnerId, amount: [u8; 32] }` ("A host-account transfer, represented in GLM base units"): the engine never produces it (`apply` has no such arm); the only constructor is `crates/arkiv-node/src/accounts.rs:201`. A node/token concept sits in the database's effect enum, it is the only variant with undocumented fields, and it is declared first while its tag is 8 (codec.rs:86). Do not touch the encoding `[CONSENSUS]`; at least document the fields and say who emits it, or (larger, **behaviour-affecting**) let the node wrap `Effect` in its own enum.

5.4 record.rs:922-932 `Limits::validate -> Result<(), &'static str>` with the caps typed into the messages ("(32)", "(512)") beside `CellName::MAX_LEN` and `Value::INDEXED_LEN_CEILING`; consumed by a `panic!` at engine.rs:472-474. A two-variant error enum, or at least messages without the literals.

5.5 `ArgumentFault` (record.rs:973) and `QueryFault` (query.rs:251) implement `Display` but not `std::error::Error`, unlike `NameFault` (record.rs:115), `Rejection` (meter.rs:499), `DecodeError` (codec.rs:236), `EngineError` (engine.rs:261). One line each. **safe**.

5.6 codec.rs `MIN_ENCODED_LEN` values are hand-summed literals: :766 `8 + 5 + 20 + 8`, :929 `3`, :1011 `1 + 5`, :1358 `1 + 4 + 4 + 8 + 1`, :1428 `9 * 8`. state.rs:85 already shows the better form (`<PriceSchedule as Decode>::MIN_ENCODED_LEN`). They only bound allocation, so a wrong value is not a consensus fault, but they will not follow a layout change.

5.7 meter.rs:225-260 `PriceSchedule::FIXTURE` is a positional 31-element array whose class names are trailing comments; nothing ties line N to variant N except the eye (the full vector is pinned by `tests/codec.rs:98-101`). `Self::uniform(1, 1).with_price(OperationClass::NamespaceCreate, 50)..` (both `const fn`) would bind each weight to its class. Also used as real pricing by `crates/arkiv-node/src/demo/workload.rs:29` although documented "Test fixture weights. Not calibrated". **needs-care** `[CONSENSUS]` (values must not move).

5.8 record.rs: `TypeTag::is_ordered` (389-391) is `self.is_indexable()`; two public names for one predicate, and `tests/records.rs:740` asserts they are equal. Fine to keep for readability, but then `QueryFault::NotIndexable` is unreachable for range bounds (typed.rs:304 returns `NotOrdered` first). Just be aware; no change proposed (`[CONSENSUS]`-adjacent: rejection codes).

### II.5 [S5] arkiv-db-trie

*(paths relative to `crates/arkiv-db-trie/`)*

- **`ProofError::EntryLimitExceeded` does double duty** (`src/interval.rs:446-447` vs `:500-501`): it is returned both
  when the caller's `max_entries` is exceeded and when an *inspection trace* reaches `MAX_INTERVAL_TRACE_NODES`,
  while its doc (`src/proof.rs:139`) only says "more entries than the caller permits" and the enum doc says
  "Why `walk` rejected a proof". arkiv-db matches on it by equality (`crates/arkiv-db/src/proof/range.rs:343`).
  A separate `TraceLimitExceeded` variant (enum is `#[non_exhaustive]`) would be honest. **behaviour-affecting**.
- **`Node::encode` silently encodes an invalid extension** (`src/node.rs:272-273`):
  `child.as_deref().unwrap_or(&[EMPTY_STRING_CODE])` writes an extension with no child, a node the crate's own
  decoder rejects ("extension without a child", `node.rs:408`). An extension never has an empty child
  (`Node` doc, `node.rs:253`); `expect("an extension has a child")` or a `debug_assert!` states the invariant
  instead of hashing garbage. No change for valid tries. **needs-care [CONSENSUS]**.
- **Public iterator named `Range`** (`src/range.rs:21`, re-exported at `lib.rs:105`) next to `KeyRange` (an
  interval) and std's `Range`; arkiv-db has `use arkiv_db_trie::{Probe, Range}` (`typed.rs:27`) in a crate that
  also uses `std::ops::Range` (`engine.rs:1999`). `MapRange`/`Entries` would read better; 1 import + 2 aliases outside. **needs-care**.
- `src/sorted.rs:170-190`: the poison flag is set and cleared by hand around the fallible section
  (`self.failed = true; ...?...; self.failed = false;`). Works; with 1.14 it shrinks to one call between the two lines.
- `src/order.rs:245-251` `KeyReader::u64`: `take(8)?` already guarantees 8 bytes, then
  `.try_into().map_err(|_| CodecError::new("key is too short"))` adds an unreachable second error path with the
  same message. Use `split_first_chunk::<8>()` or `expect`. **safe**.
- `tests/adversarial_bounds.rs:65`: clippy's "suspicious operators" is a false positive (the symmetric
  `stored.starts_with(key) || key.starts_with(stored)` is intended). No action.

### II.5 [T4] arkiv-db tests: codec / namespaces / blocks / rollback / roots / proofs

*(paths relative to `crates/arkiv-db/tests/`)*

#### 5.1 `Branch` and `Snapshot` share a read API but no trait - needs-care (public API)
Both expose the same inherent methods with the same signatures (`get`, `scan_prefix`, `descriptor`, `namespace_count`, `record_count`, `get_record`, `query`, `count`, `terms`, `try_host_get`, `try_host_range`, …; the macro at blocks.rs:213-249 and the twin fns at host_state.rs:544-645 only compile because of that). The crate exports no trait for it (`rg -n "pub trait " crates/arkiv-db/src` -> only `Encode`, `Decode`), so ten test files each declare a private delegating trait with two impls (three in this chunk, 1.2; seven more listed there), and host_state.rs duplicates whole functions per receiver. A `pub trait StateRead` (or `ReadView`) implemented by both in `arkiv-db` removes all of them and the same duplication for any host code that reads "branch or snapshot". No behaviour change.

#### 5.2 Mutations selected by bare integers - needs-care
- eq_proofs.rs:345-375 `for mutation in 0..20 { match mutation { 0 => .., 18 => .., _ => .. } }`, 317-331 `0..6`.
- eq_proofs.rs:672 `fn independent_witness(mutation: u8)`: cases 1-12 are applied in five separate places of a 117-line fn (683-693, 700, 709-713, 715-717, 720) and driven by `for mutation in 1..=12` (805); nothing ties "9" to "catalog id" except reading the body. Adding a case 13 that nobody handles silently yields the honest witness and the test fails with "accepted independent inconsistent state 13".
Replace with a labelled table (1.3c) / `enum Inconsistency { FieldNotAttribute, WrongValue, WrongType, KeyWithoutBinding, OversizedName, TooManyCells, TrailingRowBytes, BindingToOtherId, WrongCatalogId, PostingCountTooHigh, PostingCountOverCap, PostingRootFlipped }` with an `ALL` array.

#### 5.3 Range proofs have no literal wire vector; equality proofs have three - [CONSENSUS] (adding pins only)
eq_proofs.rs pins `EqQuery`/`SnapshotRef`/`EqCursor` bytes (577-622), charges (625-667) and a whole hand-built proof (1104-1148). For ranges the only codec check is a self round trip `decode_exact::<RangeRequest>(&encode(&r)) == r` (range_proofs.rs:143, executed 63 times inside the bounds loop), nothing computes `arkiv/range-query/v1` independently (`rg -n "arkiv/range" crates` -> only `crates/arkiv-db/src/proof/range.rs:34`), and `src/proof.rs` / `src/proof/range.rs` have no unit tests. A reorder of `RangeQuery` fields or a changed bound tag would pass every arkiv-db test. Add the mirror of eq_proofs.rs:577-622 and 1104-1148, and move the round trip out of the loop. (crates/arkiv-node/tests/signed_http.rs mentions the range types; not inspected, other chunk.)

#### 5.4 Magic offset - safe
eq_proofs.rs:562 `let count_at = 4 + 108 + 32 + 1 + StateHeader::MIN_ENCODED_LEN;` - version 4, `SnapshotRef` 4 + 32 + 8 + 32 + 32 = 108, digest 32, tag 1, unexplained. Write it as `4 + encode(&req.snapshot).len() + 32 + 1 + StateHeader::MIN_ENCODED_LEN` or name the parts.

#### 5.5 Control flow that hides what is tested - needs-care
range_proofs.rs:845-866: for `distinct == false` the loop builds 1,001 rows, checks the overflow, then builds a second 1,000-row fixture; for `distinct == true` it keeps the first and slices `expected[..1_000]` at 899 (`let (engine, expected) = if distinct { (engine, expected) } else { fixture(..) }`). Split into `one_row_over_the_cap_fails_closed` (1,001 rows, both shapes) and `a_thousand_rows_verify_completely` (1,000 rows, both shapes).

### II.5 [NT3] arkiv-node tests: genesis / simulator / demo / CLI; examples; Cargo.toml; clippy.toml

*(paths relative to `crates/arkiv-node/`)*

**W1. Two "is refused" helpers hang forever if the refusal regresses.** behaviour-affecting for the tests only (failure mode), needs-care
`genesis_adopt.rs:722-732` and `retention_cli.rs:13-17` (`fails`) use `Command::output()`. Several of the argument lists are otherwise complete producer command lines (`genesis_adopt.rs:684-710`; `retention_cli.rs:33-48` `producer --unsigned-simulator ... --state-retention -1`): if the guard under test is ever lost, the process starts serving and the test blocks with no deadline. `prebuilt_genesis_cli.rs:416-446` already does it right (spawn, poll, 20 s deadline, kill, `panic!("{args:?} was accepted")`) and its comment at :344-346 explains why. Use that one implementation everywhere (D1 `refused`).

**W2. Listening ports are probed, released, then handed to a child (bind-after-close race).** needs-care
`free_port()` (`genesis_adopt.rs:403`, `prebuilt_genesis_cli.rs:144`), `free_address()` (`simulator_http.rs:372`), `retention_cli.rs:81-84`. There are no fixed listening ports in this chunk (every bind is `127.0.0.1:0`), but nothing reserves the probed port until the child binds, and tests in one binary run in parallel. `genesis_adopt.rs:452` `(free_port(), free_port())` drops the first probe before taking the second, so the two may even coincide. Cheap mitigation: hold both probe listeners until both ports are read. Real fix (outside this chunk, behaviour-affecting): let the binaries accept port 0 and log the bound address. `http://127.0.0.1:9` (`genesis_adopt.rs:663,678`, `prebuilt_genesis_cli.rs:373,375,396`) is a deliberately dead peer in command lines that must be refused before any connection; fine.

**W3. Sleeps: none is a fixed "wait N then assert"; all seven are poll intervals, but the budgets are inconsistent and some are tight.** safe
`rg -n 'sleep\('`: `genesis_adopt.rs:493`, `prebuilt_genesis_cli.rs:213` (50 ms, 120 s wall-clock deadline), `prebuilt_genesis_cli.rs:435` (20 ms, 20 s), `simulator_http.rs:469` (100 x 20 ms = 2 s for a debug child to start, fetch genesis, fail and exit), `:497`, `:515` (150 x 20 ms = 3 s to start serving), `retention_cli.rs:114` (200 x 25 ms = 5 s). The iteration-counted 2-5 s budgets are the flaky ones on a loaded CI runner; use one `wait_until(Duration)` with a generous wall-clock deadline. `retention_cli.rs:103` builds its `reqwest::Client` without a timeout (the other async tests set 1-5 s at `simulator_http.rs:148,345,485`).

**W4. The hand-rolled HTTP client swallows every failure.** safe
`genesis_adopt.rs:410-433` / `prebuilt_genesis_cli.rs:151-163`: each step ends in `.ok()?`, the status line is ignored, and the body is whatever follows the first blank line. A 500 with a non-JSON body is indistinguishable from "not listening yet", so `ready()` polls for the full 120 s; callers then `.unwrap()` the `Option` (`genesis_adopt.rs:508,517,575`) and panic with no context. Return `Result<Value, String>` and treat only connection-refused as "not ready". (The two sync files hand-roll HTTP because `reqwest` has no `blocking` feature here; `retention_cli.rs` and `simulator_http.rs` use `#[tokio::test]` + reqwest for the same job - pick one style.)

**W5. `prebuilt_genesis_cli.rs:103-126`** `fn run(args: &[&Path])` forces flags and a hex address to be typed as paths: `Path::new("genesis")`, `Path::new("--prefill")`, `let sponsor = PathBuf::from(address())`. Take `&[&OsStr]` / build with `Command::arg` as `Producer::start` does. safe.

**W6. Consequences of the stringly-typed errors visible in these tests.**
- `tests/simulator.rs` compares `err.0` to string literals 12 times (`rg -c 'unwrap_err\(\)\.0|e\.0=='`), `tests/genesis_adopt.rs:384` once; `tests/demo_auth.rs` matches the typed `DemoError` enum 13 times. The older demo profile has the typed error; the newer simulator/signed profiles do not, and `src/simulator/wire.rs:17,20` throws the typed `DemoError` away with `map_err(|_| Error("InvalidRequest"))`.
- The genesis modules return `Result<T, String>` (`src/genesis_*.rs` `type Result`), so tests assert on substrings: `"profile"` (`genesis_adopt.rs:363,591` - loose enough to match several unrelated messages in `crates/arkiv-db/src/durable/`), `"Corrupt"` (:636), `"StorageCorrupt"` and `"identity does not match"` (`prebuilt_genesis_cli.rs:338,312`), `"exceeds 1 MiB"` (`genesis_prefill.rs:228,230`), `"InvalidRequest"` in stderr (`genesis_adopt.rs:731`, `prebuilt_genesis_cli.rs:445`, `retention_cli.rs:50,65`).
- Stringly-typed state poked from tests: `follower.role = "full"` (`simulator_http.rs:264`), `h.health == "storage-fenced"` (`simulator.rs:253`), `action: "step".into()`.

**W7. `Runtime::new(node, peer, replication_peer)`** is called 12 times in `simulator_http.rs` (two positional `Option<String>`s, `src/simulator/runtime.rs:45-49`; 6 calls pass `None, None`), and the tests then set fields through `Arc::get_mut(&mut rt).unwrap().admin_token = ..` (:108, :129, :272). The test needs a builder (or a `RuntimeConfig` argument); API is outside this chunk, noted for its owner.

**W8. `demo` is a misnomer for shared infrastructure.** `demo::workload::genesis` is the base of every signed genesis (`src/signed/protocol.rs:131`) and of the simulator fixture used by all three simulator test files and two examples; `demo::wire::{hex, unhex, number}` are the crate's hex/decimal codec. That is why the question "is `demo/` legacy" cannot be answered by deleting the directory (see (c)). Moving those two pieces out of `demo` would make the profile boundary real. needs-care.

### II.5 [NT2] arkiv-node tests: signed

*(paths relative to `crates/arkiv-node/tests/`)*

#### 5.1 Sleeps, timeouts, ports (emphasis c)

- Ports: every listener in the chunk binds `127.0.0.1:0` (signed_http.rs:50, signed_reads.rs:166, signed_retention.rs:273, arkiv_profile.rs:250, :457, :469). No fixed port. Fine.
- signed_reads.rs is slow and wall-clock sensitive by construction:
  - `SlowStore` sleeps on the calling thread for every namespace object read (:81); the delay is set to 10 ms at :396, :449, :475, :555, so one heavy count over 256 rows runs for seconds (module doc :5-8) and the first test runs two.
  - absolute bounds: a refused read must come back within 500 ms (:494, :523); the deadline answer must arrive in `[1 s, 2 s)` (:569-572); the permit must be back within 500 ms (:574-578) and 900 ms (:611-615).
  - three fixed `sleep(200 ms)` to show that nothing more was read (:581, :609, :618).
  - :446 is relative (`latency < count_time / 4`) and is the robust form.
  Fix (needs-care, changes what the tests accept): replace :494 and :523 with the structural fact already asserted at :528
  (`reads_in_flight() == 1`, i.e. the refusal arrived while the heavy count was still running), and express :569-572, :576, :613 relative to the measured `full_walk`/count time or widen them; a loaded CI box running other 4-thread runtimes beside these sleeps can miss a 500 ms bound.
- signed_http.rs:792-817: up to 100 rounds of status probe + four 1 ms `peek` timeouts + `sleep(10 ms)` to saturate the four admission permits. The race is real and explained (:804-807); bounded at about 1.5 s. Acceptable, better isolated (4.1).
- signed_http.rs:510-519 `until`: 1 ms poll, 10 s deadline, condition-driven. Fine.

#### 5.2 Requests that can hang instead of failing

Only the first test of signed_http.rs builds a client with a timeout (:221-224). The other nine and signed_reads.rs, signed_retention.rs,
arkiv_profile.rs use `reqwest::Client::new()`, and `read.await` after `spawn_post` (signed_http.rs:587, :593, :619) has no bound, so a
regression in lock hand-over hangs the test binary instead of failing it. Fix: the shared `client()` of 1.3 with a 10-30 s timeout. safe.

#### 5.3 Integer-indexed case loops

`for mutation in 0..N { match mutation { 0 => .., 1 => .., _ => .. } }` hides what each case is and reports "mutation 3":
signed_host.rs:488-526 (7 cases; `recertify(&mut invalid)` repeated in six arms), :598-611 (three `if mutation == n`), signed_history.rs:718-724,
ethereum_transactions.rs:58-65, and the reader roles in signed_reads.rs:655-663 with the matching :695-710.
Fix: an array of `(&str, fn(&mut SignedBlock))` (or a small enum for the reader roles); the name goes into the assertion message. safe. About -15 lines in signed_host.rs.

#### 5.4 Weak assertion

signed_retention.rs:164-171 checks an `eth_call` result with `.chars().last() == Some('2')`. The value is the ABI word for entity nonce 2;
`0x..12` would pass. Compare with `format!("0x{:064x}", 2)`. safe (strengthens a response pin).

#### 5.5 Inconsistent in-file fixtures

- ethereum_transactions.rs: three constructions of the chain with two proposers (1.1).
- signed_http.rs `step(client, url)` (:146-157) and signed_reads.rs `step(client, node)` (:316-325) have the same name and endpoint but only the second retries `429`.
- `head(rt)` returns a `Header` in signed_http.rs:158 and a `u64` in signed_reads.rs:291.

### II.5 [NT1] arkiv-node tests: core

*(`T/` = `crates/arkiv-node/tests/`)*

1. accounts.rs and gas_execution.rs read like a different author from the other eight files: single-letter bindings throughout (`g`, `p`, `f`, `l`, `ls`, `hs`, `h`, `b`, `s`; `p: bool` for "producer" at `T/accounts.rs:43` next to `p` the producer host at :148), bare `.unwrap()` on everything, no module doc, no blank lines. exec.rs / replay.rs / validation.rs use full words and `.expect("why")`. Rename to `genesis` / `producer` / `follower` / `light` when the helpers move to signed_support (1.1 #4). **safe**.

2. Stringly-typed errors, consequence in this chunk: 15 assertions match `Error.0` against a string literal - `rg -n '"(InsufficientBalance|IdentityMismatch|InvalidSignature|UnfundedBootstrap|UnsupportedGasOperation|GasLimitBelowIntrinsic|ExceedsStorageCap|InvalidBlock)"' T/accounts.rs T/gas_execution.rs` (e.g. accounts.rs:230, :241, :382, :385, :405, :668; gas_execution.rs:370, :385, :521, :872, :1008, :1029, :1063, :1067, :1138). A typo in the code or in the test is a test failure at best, a silently different code at worst. `T/signed_host.rs:149` has a `message(result) -> &'static str` helper these files could at least share.

3. Magic numbers in the two signed files:
   - `21_000` x25 in accounts.rs, `21000` x4 + `21_000` x1 in gas_execution.rs, while `gas::TX_INTRINSIC_GAS` exists and is used 2 and 3 times in the same files. Two spellings of one number in one file.
   - `17280` (blocks per day at 5 s) x6 in gas_execution.rs, x1 in accounts.rs (:98); `Some(17282)` at gas_execution.rs:554, :577 is "17280 + height 2" with no hint.
   - `9200` (chain id) x7 in gas_execution.rs; `7_300_000_000_000u64` (20 gwei per unit-day x 365 = `gas::RENT_WEI_PER_UNIT_DAY * 365`) at :249 and :307; `[3; 16]` node id in every `Host::create`.
   Name them (`const CHAIN_ID`, `const DAY_BLOCKS`, `const RENT_WEI_PER_UNIT_YEAR`). **safe**.

4. gas_execution.rs asserts gas mostly by loose ranges (`(50_000..=1_000_000).contains(&gas)` :164, `< 200_000` :243, `> 50_000` :283, `< 1_000_000` :299, `> 21000` :557, :580, `>= 21000` :601, :616; accounts.rs:179 `> 21_000 && < 1_000_000`). For a file named gas_execution these pin no metering number; only rent is exact. If exact gas is pinned in another test file (arkiv_profile.rs, not my chunk) say so in the module doc; if not, that is a coverage gap rather than a style point. No change proposed.

5. `T/block_model.rs:1469-1486` - coverage labels are made by formatting `Debug` output and splitting it on `(`, ` ` and `{` (`format!("applied {effect:?}").split(['(', ' ', '{']).take(2)...`). It works because a wrong label fails the required list, but it ties the test to `Debug` formatting. A `match` returning `&'static str` per variant is shorter and exact. **safe**.

6. Test names that are lists: `funded_lifecycle_fee_burn_replay_proofs_and_restart` (`T/accounts.rs:143`, 74 lines), `insufficient_funds_pending_reservation_and_exact_zero_balance` (:218), `overflow_and_insufficient_value_are_atomic_with_charged_account_receipt` (:442), `durable_reopen_light_proof_and_reserved_expiry_sweep` (`T/gas_execution.rs:632`), `continuous_demo_200_blocks_match_independent_roots_outcomes_and_pages` (`T/block_model.rs:2089`). Each is several scenarios sharing a chain; when one fails the name does not say which. The native-chain files name one behaviour per test.

7. `T/replay.rs:1298-1330` `admission_rejection_is_an_invariant_error` and `:1335-1388` `halted_expiry_aborts_the_candidate` use neither `Fixture` nor a golden; they are executor failure paths ("no candidate, no outcome"), which is exactly the stated subject of exec.rs (`T/exec.rs:1-8`). MOVE them there. **safe**.

8. `T/support/block_roots.rs` - besides its location (4.2): single-letter names everywhere (`b`, `v`, `x`, `t`, `m`, `w`, `k`), and :124-128 builds the same length-prefixed key twice (`bound` and the head of `leaf`).

### II.5 [S4] arkiv-db-redb tests and examples

*(paths relative to `crates/arkiv-db-redb/`)*

#### 5.1 The six examples: which earn their place  (emphasis d)

| example | referenced from | verdict |
| --- | --- | --- |
| redb_experiment.rs (610) | README.md:265, 272; docs/redb-experiment.md:218, 585-586; docs/testing.md:364; three test module docs | keep: regenerates docs/redb-experiment.md and is the reason for tests/common |
| engine_storage_experiment.rs (533) | README.md:266; docs/engine-storage-v1-report.md:3, 6, 197-199 | keep: its stdout is that report (see 4.12) |
| bench_readers.rs (360) | scripts/bench-compare.sh:304-311, 472-494, 617; scripts/bench/parse_run.py, summarize.py; docs/benchmarking.md:71-74, 101 | keep, and keep self-contained (the script copies this one file into old revisions) |
| reachability_report.rs (189) | docs/pruning-design.md:163-167 | keep: a diagnostic tool, not an experiment |
| key_size_report.rs (332) | only docs/benchmarks/2026-10-11-prekucki-v1.md:105 ("built unchanged against `187b349` and against this branch") | one-off layout-1 vs layout-2 comparison whose numbers are recorded in that doc: **DELETE** (the doc can cite the commit), or move to `scripts/bench/`. 332 lines, and with it the second copy of `Rng`/`commit`/bootstrap (1.14) and items 4.6, 4.13, 4.14 |
| durable_throughput.rs (185) | nothing (`rg -n 'durable_throughput\|PERF_CACHE_MB\|PERF_RECORDS' --glob '!crates/arkiv-db-redb/examples/**'` over the repo: no hit); its table row has no header and no doc that records it | orphan: **DELETE**, or reference it from docs/benchmarking.md and print the header row. 185 lines |

#### 5.2 Two error styles across the examples
bench_readers.rs and key_size_report.rs return `Res<T> = Result<T, Box<dyn Error + Send + Sync>>` and wrap
every engine call in `.map_err(|e| format!("begin: {e:?}"))` (16 times each:
`rg -c 'map_err\(\|e\| format!' crates/arkiv-db-redb/examples`); the other four use `unwrap`/`expect`.
In a benchmark binary the `map_err` strings add nothing over `expect("begin")`; a
`fn ctx<T, E: Debug>(what: &str, r: Result<T, E>) -> Res<T>` would at least remove the repetition.

#### 5.3 `engine_support` imports
All five includers use `use engine_support::*;` (clippy lists the 7-16 names each needs). With a single
test crate (4.1) the two test files can import by name.

#### 5.4 Magic host key
examples/reachability_report.rs:80 writes a literal host key `[0x48; 32]` although
`engine_support::HOST_KEY` (`[19; 32]`, mod.rs:15) is in scope and is the key `base()` writes. Use the constant. **safe**.

### II.5 [T5] arkiv-db tests: durable / faults / KV; examples

*(paths relative to `crates/arkiv-db/`)*

1. **`FaultyStore` drops `stage_objects` and `relabel`** (crates/arkiv-db-storage/src/testing/faulty.rs:334-444; details in 1.1). Outside my chunk, but it is the root cause of two doubles here and a trap for the next test that wraps a store used for bulk staging or adoption: the wrapped store answers `Incompatible` instead of forwarding. Fix: add the two forwarding methods and a contract case. **behaviour-affecting** for tests only.
2. **The doubles disagree on what they forward** (last column of the table in 1.1): seven of the eight hand-written `impl VersionedStore` wrappers in the workspace omit `stage_objects` and/or `relabel` (all but tests/adopt.rs `Faults`). Today's tests do not hit it; it is an accident of which paths each test exercises. Fixed structurally by either option in 1.1.
3. **The assertions in the examples never run in automation.** examples/records.rs has 67 `check(..)` / `assert*!` lines (hand-derived receipt costs: 53, 383, 233, 261, 297, 1174, 120, 31, 34, 28, 24, 37, 27, 14, 183, 245, 102, 106, ...) and examples/blocks.rs has 5; docs/testing.md:246-251 and docs/node-storage-plan.md:409-411 cite `examples/records.rs` as evidence for the charge table. CI (.github/workflows/ci.yml:45-49) runs `cargo clippy --all-targets` and `cargo test --workspace`, which compile examples but do not execute them; README.md:262-264 lists them as manual commands. `rg -n '\b(383|261|297|1174|245)\b' crates/arkiv-db/tests/` -> no hits, so these particular literals are pinned nowhere that runs. The examples are in sync with the API (they compile under the clippy run; clippy-uniq.txt has only style lints for them). Fix, cheapest first: add three `cargo run -p arkiv-db --example {basic,blocks,records} --offline` steps to ci.yml; or move the asserting flow of records.rs into `tests/records_flow.rs` and leave a 100-line printing example. Do not delete the literals. **needs-care** `[CONSENSUS]` (receipt numbers).
4. **examples/basic.rs** demonstrates only the raw KV facade discussed in 2.2 and is redundant with tests/kv.rs as documentation; it lives or dies with that decision (README.md:262, 393).
5. **tests/durable_failures.rs:895-900** - the test re-derives the size of the cache's private slot type as `size_of::<((SpaceId, MapKind, ObjectId), Arc<dyn Any + Send + Sync>, usize, bool)>()` to bound `container_bytes`, which the implementation computes as `capacity * size_of::<Entry>()` (src/durable/cache.rs:404). If `Entry` gains a field the integration test bounds the wrong thing with no compile error. Fix: expose the slot size (e.g. `CacheStats::slot_bytes` or a `pub const`) and use it, or drop the `container_bytes` line here and rely on the unit tests at src/durable/cache.rs:742 and 782. **needs-care**.
6. **Matching on error text**: tests/adopt.rs:258 `error.to_string().contains("single height-zero publication")` and tests/simulator_composition.rs:877 `detail.contains("configured initial state")` tell one refusal from another by its prose, so rewording a message breaks a test. Fix: assert the kind as well (`Incompatible` / `Corrupt`) and keep the text check only where two refusals of the same kind must be told apart, or give those two refusals their own `DurableError` variants. tests/storage_faults.rs:549 `error.detail.contains("poisoned")` is justified and stays: a poisoned branch re-issues the original kind and only prefixes the detail (src/engine.rs:1599-1609), so the text is the only marker. **needs-care**.
7. **Magic numbers without a derivation**: tests/simulator_composition.rs:287 `let cap = 600;` (a `prepare_bytes` cap that must sit below one packet's payload; the prefilled variant at 775-780 derives its caps from `last_preparation` instead: do the same or say why 600); tests/support/durable_history.rs:323 `delta.record_reads <= 9` while tests/simulator_composition.rs:195 and 424 pin `== 10` with a comment. **needs-care**.
8. **tests/storage_faults.rs:42-43** pulls all of `crates/arkiv-db-trie/tests/support/mod.rs` (two root oracles, an RNG; it needs `#![allow(dead_code)]` at its line 15) into this test binary by cross-crate `#[path]` for two functions, `reference::list` and `reference::string` (used at line 638). Acceptable, but it is the second cross-crate `#[path]` include in the chunk (the other direction is support/durable_history.rs into arkiv-db-redb); a small `test-support` dev-dependency crate would give all of 1.2 a real home and remove both. **needs-care**.

### II.5 [T3] arkiv-db tests: queries / records / pagination / terms

*(paths relative to `crates/arkiv-db/tests/`; `qm` = `query_metering.rs`)*

1. **`applied` means opposite things in sibling files**: `records.rs:206`/`terms.rs:162` build a `ReceiptKind::Write` receipt, `queries.rs:113` a `ReceiptKind::Read` one. A reader moving between files will misread `assert_eq!(receipt, applied(18))`. Rename `queries.rs`'s to `read_applied` (34 uses). safe.
2. **`id` vs `rid`**, **`ns_id` via `from_bytes(ordinal.to_be_bytes())`** (`queries.rs:79-81`) vs `from_ordinal` (`records.rs:116`, `terms.rs:89`): two spellings of the same constructor; the `to_be_bytes` version quietly depends on the id encoding being a big-endian u64. Use `from_ordinal`. safe.
3. **Three budget-sweep harnesses of different strength** for the same contract: `records.rs:539-636`, `terms.rs:552-589`, `qm:808-964`. The weaker two do not run traced budgets or `total + 1`. After 1.2/2.1 only `terms.rs` still needs the read sweep; point it at the strong one. needs-care.
4. **The oracle is "independent" in name only where it calls the engine's own classification**: `records.rs:415-417` (`term_bytes()`, `tag()`), `terms.rs:369` (`tag()`), `pagination.rs:569-576` (`tag()`), while `queries.rs:253-282` insists on a private `type_of`. Decide once what the oracle may use (my suggestion: `Value::tag()` and `Cell::is_attribute()` yes, `term_bytes()`/`cmp_ordered()` no) and write that sentence in the shared model's doc.
5. **Range semantics of every model stop at I64/U64** (`queries.rs:338` `_ => false`, `pagination.rs:585`), although `queries.rs:1764-1770` shows Str, Bool and Addr ranges are valid queries. The generated sets only produce integer ranges (`queries.rs:744-745`), so nothing is wrong today, but a Str range added to `single_terms` would make the model disagree with a correct engine. Either extend the shared model with `value_order` from `terms.rs:284-292` or `panic!` on a non-integer range in the model instead of answering `false`. needs-care.
6. `pagination.rs:999-1001` `Head::finish(self) -> (Model, u64, usize)` and `queries.rs:2195` return bare tuples of same-typed things; destructure the struct fields instead (`let Head { model, published, stale_candidates, .. } = head;`). safe.
7. `records.rs:139`/`terms.rs:128` `type Cells = Vec<(CellName, Cell)>` vs `queries.rs:144` `type Cells = BTreeMap<CellName, Cell>`: same alias name, different type, in files that share half their helpers. Rename one (`CellList` / `CellMap`) before sharing. safe.

### II.5 [T2] arkiv-db tests: history / isolation

*(paths relative to `crates/arkiv-db/tests/`; `h` = history.rs, `qh` = query_history.rs, `i` = isolation.rs, `qi` = query_isolation.rs)*

- **API shape (library):** `Snapshot` and `Branch` expose the same read methods (`get`, `scan_prefix`, `descriptor`, `get_record`, `query`, `count`) with identical signatures but share no trait (`rg "pub trait" crates/arkiv-db/src` finds only `Encode`/`Decode`). Six test files therefore hand-write a delegating `trait Reader` (the four here, blocks.rs:192, queries.rs:901) under five different method-naming schemes. Either add a small read trait to arkiv-db or keep it in `tests/support/reader.rs` (1.3). **needs-care** if done in the library (public API).
- Two parallel vocabularies for the same thing across sibling files: `create`/`patch`/`delete` (qh) vs `create_record`/`patch_record`/`delete_record` (qi); `create` (i) vs `create_namespace` (qi); `delete` means raw `Delete` in h and i but `DeleteRecord` in qh; `receipt` has four signatures (1.7). Unify when moving to `support`.
- `seed_tests!` (h:2075-2080, qh:4279-4283) pairs a hand-typed hex suffix in the test name with `SEEDS[n]`; name and seed can drift. Pass the literal (`.. => 0x9e37_79b9;`) and drop `SEEDS`.
- Magic step counts: isolation.rs:1164 `assert_eq!(reread.len(), 8, ..)`, query_isolation.rs:813 `READ_STEPS = 39`; derive from the expectation tables.

### II.5 [T1] arkiv-db tests: metering family

*(paths relative to `crates/arkiv-db/tests/`; `qm` = `query_metering.rs`)*

1. **File size.** qm is 4,804 lines, of which the first 1,010 are helpers before the first `#[test]` (21%). Its own banners already name the split: `query_metering/writes.rs` (1012-2267, create/patch/delete, ~1,255), `reads.rs` (2269-3694, get_record/query/count, ~1,425), `rules.rs` (3696-4804, block budgets, host path, estimation, overflow, pricing, planned, ~1,110), over `support/{metering,model}.rs` (1.2 items 2-3). metering.rs is 2,088 with 339 lines of helpers; after section 2 it is ~1,750 and fine as one file.
2. **Slack constants in a file that promises hand-derived charges** (query_planner.rs:33-35): `planned_broad <= written_selective + 16` (632), `written_broad > 50 * planned_broad` (634), `< 1 + 5 + 8 + 3 * both + 40` (801), `3 * clus > 3 * both + 40 + 400` (802, which tests the fixture, not the engine). Name them (`const CLUSTER_JUMP_SLACK: u64 = 40;`) with the one-line reason each already has in the comment at 794-796, or derive the exact charge like 631 does.
3. **cell_limits.rs:110-133 pins error message strings** (`"max_cell_name_len must be positive"` etc.) because `Limits::validate` returns `Result<(), &'static str>` (`src/record.rs:922`). Consequence for whoever types that error: this table is the only caller comparing the text. Line 134-137 also pins that `Engine::with_limits` panics on invalid limits (`catch_unwind(..).is_err()`), i.e. a constructor `expect` reachable from configuration.
4. **bounds.rs:25 `assert_admission(error: Rejected, ..)`** checks `spent == 0` and an empty trace but not the receipt's `outcome`/`kind`; qm:2050-2053 and 3990-3994 show the full form (`write_receipt(0, Outcome::Rejected(reason))`). Strengthening it would let its 15 call sites pin the whole zero-cost receipt.
5. **Three neighbouring files, three spellings of the same record-creation helper** (1.1): `create(ns, key_text, cells) -> WriteOp` (qm), `create(branch, key, cells) -> Result<(), Rejection>` (cell_limits.rs), closure `create(branch, ordinal, cells) -> RecordId` (query_planner.rs:254-270). Worth fixing the vocabulary (`create_op` vs `create`) when the support module is written.

### II.5 [D2] Scripts, CI, build config, vendor/reth-trie, conformance

*(repo-relative paths)*

- **CI covers what README claims.** README.md:258-261 lists fetch, test, clippy and fmt; .github/workflows/ci.yml:39-49
  runs the same four with the same package list (ci.yml:21-22 == README.md:260-261) on the MSRV toolchain with `--locked`.
  `cargo test --workspace` does include the vendor crates while fmt/clippy exclude alloy-trie: that is deliberate and
  documented (vendor/README.md:47-49 "run upstream unit tests plus our database tests"), at the cost of compiling
  alloy-trie's dev-dependencies (criterion, hash-db, plain_hasher, triehash). The one inconsistency is
  docs/signed-proposer-v1.md:272, which tells the reader to run `cargo test --workspace --exclude reth-trie --exclude alloy-trie`.
- **Missing checks a reviewer would expect** (short): (1) none of the script tests run; test-run-full-prefill.py and
  test-full-prefill-bootstrap.py are stdlib-only and would run as they are, while the two sourcify tests need OpenSSL's
  `KECCAK-256` (sourcify-v4-prefill.py:198-202 says OpenSSL 3.5+), which a stock ubuntu-24.04 runner may not have;
  (2) no smoke at all against a built `arkiv-proposer`, although signed-proposer-smoke.py needs only python3 and the
  binary; (3) no `cargo doc` with `RUSTDOCFLAGS=-D warnings`; (4) no `docker build -f Dockerfile.simulator` (S3 would have
  been caught); (5) no shellcheck for the two bash files or a Python linter; (6) no `rust-toolchain.toml`, so a local
  `cargo fmt` can disagree with CI's 1.90.0 rustfmt.
- **golem-conformance.yml is sane**: revisions pinned by full SHA (:23-24), the checkout path matches what ci.sh:35 demands,
  the weekly non-gating job has `continue-on-error`, the test total 21 matches
  `rg -c '#\[test\]' conformance/golem-db/src/golem_tests.rs` = 21 and the five names in expected-failures.txt exist.
  Only D10 (the duplicated job) and D9 (the pins in four places).
- **deploy/README.md:34-67** is a dated deployment log ("Deployment verification (2026-09-29)", "The same URLs now cover
  30 paths / 33 HTTP operations") with one machine's hostname (`full-node.rogue-one.eu`, :36-37) and remarks such as
  "then run with the since-removed `--features arkiv-v2`" (:41, :57). The how-to ends at :32. Move 34-67 to
  docs/rogue-one-deployment.md or the changelog. deploy/nginx-api-docs.conf is fine.
- **Hand-encoded wire formats in Python**: create-sample-namespace.py:68-69 and seed-bricks.py:66-90, 122-124 build the
  native transaction envelope byte by byte, and full-light-demo.py:75-104 (`VALUE_WIDTHS`, `page_size`) walks a canonical
  `EqRequest` by fixed offsets (`108 + 4 + 8`). They duplicate the Rust codec and nothing ties them to it; a codec change
  breaks them silently. If `arkiv-proposer` grew `namespace create` / `create --attr` subcommands (it already has
  `create` and `transfer`), the first two scripts would shrink to HTTP calls. Not a quick fix; listed so it is known.
- **conformance/golem-db/README.md:20** is a single table row of about 700 characters; split the description out of the table.
- **udc-sdk-smoke.ts:44-45 and 230-231**: `rpc(q, opts)` is `call("arkiv_query", [q, opts])`; define `call` first and
  derive `rpc`. `pages` (51-59) and `qPosts` (70-72) build the same query with different selections.
